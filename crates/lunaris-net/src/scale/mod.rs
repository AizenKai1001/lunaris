//! Netcode for large matches: a hundred players and more.
//!
//! The deterministic lockstep and rollback of the rest of this crate make every player's computer run the whole game, which is exact and cheap for a few players and cannot be
//! made to work for a hundred: the work and the trust grow with every player. This is the other design, the usual one for a battle royale:
//!
//! * **A server that owns the match.** Each client runs its own character with the game's own movement code, and tells the server where it is thirty times a second; the server
//!   refuses a move the character could not have made ([`server::ServerConfig::max_speed`] and the limits up and down), keeps what is left (the other players, the loot, the zone, health)
//!   and decides shots from where everyone was when the shot was fired ([`server::World::hitscan`]).
//! * **UDP, with what a game needs of it and no more.** Every packet carries acknowledgements both ways ([`proto::Acks`]), so a lost packet costs nothing for what is sent again
//!   anyway (positions) and a reliable message (a shot, a hit, a kill) is sent again until a packet with it is acknowledged, and handed over once, in order ([`proto::ReliableOut`],
//!   [`proto::ReliableIn`]). The handshake gives a spoofed address nothing ([`proto`]).
//! * **Only what matters, as often as it matters.** A client is told about the entities within reach of its character and each at a rate that falls with distance
//!   ([`interest`]): a hundred players in one square still fit every client's packet (the nearest and the stalest first), and a hundred spread over a map send each client the few
//!   near it. The records are the same whole, small, quantised ones every time ([`state`]), so nothing has to be agreed on after a loss.
//! * **No wallhack for the asking.** A game can have the server tell a client only about the players its character could see (or is about to, or is next to): [`occlusion`]. Off by default.
//! * **Smooth for the eye.** The client keeps a few records of each entity and shows it a tenth of a second in the past, between two of them ([`client::NetClient::sample`]).
//!
//! The server and the client know only the [`transport::Transport`] trait: a UDP socket in a game, a network in memory (with latency, jitter, loss and doubling from a seed) in
//! the tests, where a hundred bots play a match against a server in a second ([`swarm`]).

pub mod admin;
pub mod bytes;
pub mod play;
pub mod client;
pub mod interest;
pub mod netinv;
pub mod netsync;
pub mod occlusion;
pub mod party;
pub mod proto;
pub mod secure;
pub mod server;
pub mod simgame;
pub mod state;
pub mod swarm;
pub mod transport;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod simgame_tests;
#[cfg(test)]
mod party_tests;
#[cfg(test)]
mod simgame_party_tests;
#[cfg(test)]
mod simgame_gear_tests;
#[cfg(test)]
mod netsync_tests;
#[cfg(test)]
mod occlusion_tests;
#[cfg(test)]
mod occlusion_sim_tests;
#[cfg(test)]
mod occlusion_bench_tests;
#[cfg(test)]
mod occlusion_equiv_tests;
#[cfg(test)]
mod secure_tests;
#[cfg(test)]
mod secure_net_tests;
#[cfg(test)]
mod fuzz_tests;
