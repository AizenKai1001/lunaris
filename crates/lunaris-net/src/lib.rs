//! Deterministic multiplayer for Lunaris, with nothing but the standard library.
//!
//! The engine's promise (the same scene, seed and commands give the same state, bit for bit, on any machine) makes multiplayer cheap:
//! nobody sends world state, everybody sends **inputs**, and every player's copy of the game steps through the same inputs in the same
//! order. A [`Host`] is only a relay: it does not run the game. Two ways of playing share it:
//!
//! * **Lockstep** (the default, protocol 1): the host collects one input message per player per tick and, once it has all of them, sends
//!   the whole tick to everybody. A client steps a tick only when it has it. Exact and simple, but everybody waits for the slowest player.
//! * **Rollback** (opt-in, protocol 2, [`Mode::Rollback`], GGPO style): every client steps at once, guessing the inputs it has not
//!   received yet ("the player changes nothing"), and when the real input arrives and the guess was wrong it goes back to a saved
//!   [`Checkpoint`](lunaris_sim::Checkpoint) and plays the last few ticks again. Latency is hidden instead of waited for.
//!
//! Every few ticks the clients send their state hash so that a divergence (a bug, a different file, a cheat) is noticed within a second.
//!
//! The lines on the wire are JSON, one object per line (`docs/NETWORK.md` has the full description). Protocol 1 (lockstep):
//!
//! | who | message | meaning |
//! |---|---|---|
//! | client | `{"hello": {"name": "Ada", "version": 1}}` | I want to play |
//! | host | `{"welcome": {"player": 0, "players": 2, "seed": "0000000000000001", "delay": 3}}` | you are player 0 of 2; start your world with this seed |
//! | client | `{"ready": {"hash": "..."}}` | my world is loaded; this is its state hash |
//! | host | `{"start": {}}` then `{"tick": {"t": 0, "inputs": []}}` ... | everyone is ready; ticks 0 to `delay - 1` have no input |
//! | client | `{"input": {"tick": 3, "values": {"d": true}}}` | my keys that changed, for tick 3 |
//! | host | `{"tick": {"t": 3, "inputs": [[0, {"d": true}]]}}` | everything that happens in tick 3 |
//! | client | `{"hash": {"tick": 30, "hash": "..."}}` | my state hash after stepping tick 30 |
//! | host | `{"desync": {"tick": 30, "hashes": ["...", "..."]}}` | two copies of the game disagree |
//! | either | `{"bye": {"reason": "..."}}` | the game is over |
//!
//! Protocol 2 (rollback, lobby, watching, rejoining) keeps those lines and adds:
//!
//! | who | message | meaning |
//! |---|---|---|
//! | client | `{"hello": {"name": "Ada", "version": 2, "mode": "rollback", "role": "player", "token": "..."}}` | `mode` is `lockstep` or `rollback`; `role` is `player` or `watch`; `token` (optional) takes back a seat |
//! | host | `{"welcome": {..., "mode": "rollback", "window": 8, "token": "...", "lobby": false, "rejoin": false, "watch": false, "max_ticks": 500}}` | the same fields as before, plus the rollback window, the seat token, and what kind of join this is |
//! | host | `{"roster": {"players": [{"player": 0, "name": "Ada", "loaded": true, "ready": false}], "max": 2, "watchers": 0}}` | lobby: who is here |
//! | client | `{"lobby": {"ready": true}}`, `{"lobby": {"start": true}}` | lobby: I am ready; (first seat only) start now |
//! | host | `{"assign": {"player": 0, "players": 2}}` | lobby: your seat number and the number of players, which may have changed while people came and went |
//! | host | `{"start": {"player": 0, "players": 2, "tick": 0}}` | the game begins (a spectator's says `"watch": true`, a returning player's `"rejoin": true`) |
//! | host | `{"input": {"player": 1, "tick": 7, "values": {"d": true}}}` | rollback: another player's input for tick 7, forwarded at once |
//! | host | `{"seat": {"player": 1, "name": "Bo", "state": "empty", "reason": "..."}}` | a seat is empty, `frozen` (from `tick`: its last keys stay held), or `live` again; frozen and live carry a number `n`, applied in order |
//! | host | `{"log": {"n": 0, "upto": 120, "inputs": [[3, 0, {"d": true}], ...]}}` | a spectator or a returning player: key changes of the game so far (chunk number `n`, chunks are put in order); every tick below `upto` is complete |
//! | host | `{"frontier": {"tick": 120}}` | while a seat is frozen: how far the game is settled (players keep within a window of it) |
//! | client | `{"resume": {"tick": 120}}` | a returning player has replayed the log and wants its seat back |
//! | host | `{"go": {"tick": 124, "next": 124}}` | the log is complete up to tick 124; the seat is yours again, and your own first input is for tick `next` |
//! | host | `{"bye": {"reason": "...", "tick": 500}}` | the game is over; every copy ends on this (confirmed) tick |
//!
//! The first player's keys are the usual resources `input.<key>`; the second player's are `input.p2.<key>`, the third's `input.p3.<key>`
//! and so on, so a two-player scene reads `input.w` for the first and `input.p2.w` for the second.
mod book;
mod client;
pub mod scale;
mod host;
mod hostv2;
mod latency;
mod log;
mod rollback;
mod session;
mod watch;
mod wire;

pub use book::InputBook;
pub use client::{Lockstep, Welcome};
pub use host::{Host, HostConfig, HostReport};
pub use latency::{LatencyProxy, LinkSim, ProxyReport};
pub use log::{apply_tick_inputs, InputLog, LogEntry, Replayer};
pub use rollback::{Rollback, RollbackStats};
pub use session::{connect, JoinOptions, LobbyCommand, Roster, RosterEntry, Seat};
pub use watch::Watcher;

/// The version of the lockstep wire protocol; a host and a client with different versions refuse each other.
pub const VERSION: i64 = 1;

/// The version of the second protocol (rollback, lobby, watching, rejoining). A client that speaks it says so in its `hello`; a host
/// that runs plain lockstep refuses it with a message that says what to do, and a rollback host refuses version 1 the same way.
pub const VERSION_2: i64 = 2;

/// How a game is played over the network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Everybody waits until everybody's input for a tick has arrived (protocol 1, the default).
    Lockstep,
    /// Everybody steps at once with guessed inputs and goes back when a guess was wrong (protocol 2).
    Rollback,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Lockstep => "lockstep",
            Mode::Rollback => "rollback",
        }
    }

    pub fn from_name(s: &str) -> Option<Mode> {
        match s {
            "lockstep" => Some(Mode::Lockstep),
            "rollback" => Some(Mode::Rollback),
            _ => None,
        }
    }
}

/// The name a player's key has in the `input` tool: `<key>` for the first player, `p<n>.<key>` for player number `n` (2, 3, ...).
pub fn key_name(player: usize, key: &str) -> String {
    if player == 0 {
        key.to_string()
    } else {
        format!("p{}.{key}", player + 1)
    }
}

/// The resource a player's key is stored in: `input.<key>` for the first player, `input.p<n>.<key>` for player number `n` (2, 3, ...).
pub fn key_resource(player: usize, key: &str) -> String {
    format!("input.{}", key_name(player, key))
}
