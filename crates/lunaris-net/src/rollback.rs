//! Rollback netcode (GGPO style): one player's end of a game in which nobody waits.
//!
//! The idea in four sentences. Every tick, the client plays the game *now*, with the real input of every player it has heard from and a
//! **guess** for the others: "this player changes nothing", that is, their keys stay as they were. Before each tick it saves a
//! [`Checkpoint`] of the simulation (a few dozen microseconds). When a real input arrives for a tick that was already played with a guess,
//! and the guess was wrong, it goes back to the checkpoint of that tick and plays the ticks again with the truth. The picture on screen
//! jumps a little when that happens, but the game never waits for the network, and every copy still ends up in exactly the same state,
//! because the simulation is deterministic and the real inputs are the same for everybody.
//!
//! Three rules keep it honest:
//! * a tick is **settled** when every player's input for it is known (or the player's seat is frozen); the *frontier* is the first tick that
//!   is not settled. Nothing below it can change, so checkpoints below it are thrown away and state hashes are only compared there;
//! * a client never runs more than `window` ticks ahead of the frontier (it *stalls* instead), so a rollback is never deeper than `window`
//!   ticks and there is always a checkpoint to go back to;
//! * the game ends on a tick the host names (`bye` with a `tick`), a settled one, and a client that had played past it goes back to it.
use crate::book::{is_empty_input, InputBook};
use crate::client::Welcome;
use crate::log::{apply_tick_inputs, Replayer};
use crate::wire::{bye, hex, reason_of, send};
use crate::{key_resource, Mode};
use lunaris_api::Engine;
use lunaris_json::Json;
use lunaris_sim::Checkpoint;
use std::collections::{BTreeMap, VecDeque};
use std::net::TcpStream;
use std::sync::mpsc::{self, TryRecvError};

/// How much rolling back has happened (what a game's author wants to know about the network).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RollbackStats {
    /// Times the game went back to a saved tick because a guess about another player's input was wrong.
    pub rollbacks: u64,
    /// Ticks played again because of those rollbacks: the price of rollback, in simulation time.
    pub resimulated_ticks: u64,
    /// The most ticks one rollback went back.
    pub max_depth: u64,
    /// New ticks played while at least one player's input was still a guess.
    pub guessed_ticks: u64,
    /// Times a call wanted to play a tick and could not, because the game was `window` ticks ahead of the oldest tick that is not settled.
    pub stalls: u64,
    /// New ticks played (not counting the replays after a rollback).
    pub ticks: u64,
}

/// The state of the game at the start of `tick` (before its inputs are applied), and the hash of it when that is one that is reported.
struct Slot {
    tick: u64,
    point: Checkpoint,
    hash: Option<u64>,
}

/// A player who is taking its seat back first replays the game from the log, as a spectator would.
struct CatchUp {
    replay: Replayer,
    resume_sent: bool,
    /// The host's answer: `(r, next)`: play the log up to tick `r`, then go live; our first own input is for tick `next`.
    go: Option<(u64, u64)>,
}

pub struct Rollback {
    pub welcome: Welcome,
    stream: TcpStream,
    rx: mpsc::Receiver<Json>,
    me: usize,
    /// What everybody's inputs are known to be, and the frontier.
    book: InputBook,
    held: BTreeMap<String, f64>,
    sent: BTreeMap<String, f64>,
    /// The next tick this player owes the host an input for.
    next_send: u64,
    /// Our own keys take effect this many ticks after they are pressed (the first ticks of a new game have no keys).
    input_delay: u64,
    /// Ticks below this are sent empty without looking at the keys (a new game: nobody could have pressed anything yet).
    quiet_until: u64,
    window: u64,
    ring: VecDeque<Slot>,
    /// Send the state hash of every settled tick that is a multiple of this (0: never).
    pub hash_every: u64,
    /// Set when the host reports that two copies of the game disagree.
    pub desync: Option<String>,
    /// Set when the game is over, and this copy has reached the tick it ends on.
    pub finished: Option<String>,
    end_tick: Option<u64>,
    end_reason: Option<String>,
    pub stats: RollbackStats,
    events: Vec<String>,
    /// The frontier as the host last announced it. While a seat is frozen the frontier of this copy's own book is not to be trusted alone (see
    /// `frontier`), and the host says how far it has settled.
    host_frontier: u64,
    /// The earliest tick a kept input turned into a correction because the seat it belongs to was given back after the input had arrived.
    late_change: Option<u64>,
    /// Inputs and seat news that came while this copy was still replaying the log; handled when it goes live.
    deferred: Vec<Json>,
    /// Per seat: the number of the next freeze/unfreeze message to apply (they are applied in order; `None`: not known yet, the first one that
    /// comes sets it), and the ones that came early.
    seat_next: Vec<Option<u64>>,
    seat_held: Vec<BTreeMap<u64, (bool, u64, String)>>,
    catching_up: Option<CatchUp>,
    dropped: bool,
}

/// The keys of player `me` that are held down in the game right now (value exactly 1): what a returning player has to let go of.
fn held_keys(engine: &mut Engine, me: usize) -> Vec<String> {
    let Ok(r) = engine.call("resources", &Json::Null) else { return Vec::new() };
    let Some(all) = r.get("resources").and_then(|r| r.as_object()) else { return Vec::new() };
    let mine = key_resource(me, "");
    let other_player = |name: &str| -> bool {
        // `input.p<digits>.<key>` belongs to player 2 and up
        name.strip_prefix("input.p").is_some_and(|rest| rest.split('.').next().is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit())) && rest.contains('.'))
    };
    all.iter()
        .filter(|(name, v)| v.as_f64() == Some(1.0) && name.starts_with(&mine) && (me > 0 || !other_player(name)))
        .map(|(name, _)| name[mine.len()..].to_string())
        .collect()
}

impl Rollback {
    /// A seat whose handshake was done by `connect`. `input_delay` is how many ticks our own keys are held back (the same in every copy:
    /// the host and the others only see "input for tick t"). `catch_up`: a player coming back, who replays the game first.
    pub(crate) fn new(welcome: Welcome, stream: TcpStream, rx: mpsc::Receiver<Json>, input_delay: u64, catch_up: bool) -> Rollback {
        let me = welcome.player;
        let window = welcome.window.max(1);
        let players = welcome.players;
        Rollback {
            book: InputBook::new(welcome.players, 0),
            me,
            window,
            welcome,
            stream,
            rx,
            held: BTreeMap::new(),
            sent: BTreeMap::new(),
            next_send: 0,
            input_delay,
            quiet_until: if catch_up { 0 } else { input_delay },
            ring: VecDeque::new(),
            hash_every: 30,
            desync: None,
            finished: None,
            end_tick: None,
            end_reason: None,
            stats: RollbackStats::default(),
            events: Vec::new(),
            host_frontier: 0,
            late_change: None,
            deferred: Vec::new(),
            seat_next: vec![if catch_up { None } else { Some(0) }; players],
            seat_held: vec![BTreeMap::new(); players],
            catching_up: catch_up.then(|| CatchUp { replay: Replayer::new(), resume_sent: false, go: None }),
            dropped: false,
        }
    }

    /// The seat token: show it to the player, they need it to come back after a drop (`--token`).
    pub fn token(&self) -> Option<&str> {
        self.welcome.token.as_deref()
    }

    /// Press or release a key (or set an axis) for this player. It reaches the host with the next input message and takes effect
    /// `input_delay` ticks later, in every copy of the game.
    pub fn set_key(&mut self, key: &str, value: f64) {
        self.held.insert(key.to_string(), value);
    }

    /// Always 0: a rollback client plays what it can at once, there is no queue of ticks waiting to be played.
    pub fn ready_ticks(&self) -> usize {
        0
    }

    /// The first tick that is not settled: everything before it is final.
    pub fn confirmed_tick(&self) -> u64 {
        self.frontier()
    }

    /// The frontier this copy acts on. Normally it is the one of its own book: the first tick somebody's real input is missing for. While a
    /// seat is frozen, its input counts as known for every tick from the freeze on, which would let this copy run on and settle (and forget)
    /// ticks the host has not settled: the player of the frozen seat may come back, and its seat is given back from the host's frontier,
    /// so a tick that this copy had already settled could still change. So while a seat is frozen the host's announced frontier is a limit
    /// too, and a rollback is never deeper than the window, frozen seats or not.
    fn frontier(&self) -> u64 {
        let f = self.book.frontier();
        if self.book.any_frozen() {
            f.min(self.host_frontier)
        } else {
            f
        }
    }

    /// Is this copy still replaying the game before it takes its seat back?
    pub fn is_catching_up(&self) -> bool {
        self.catching_up.is_some()
    }

    /// News about the seats (somebody left, a seat froze, somebody came back), oldest first; reading it clears it.
    pub fn take_events(&mut self) -> Vec<String> {
        std::mem::take(&mut self.events)
    }

    /// A short line for a window title or a log.
    pub fn status(&self, engine: &Engine) -> String {
        if let Some(c) = &self.catching_up {
            return format!("replaying the game: tick {} of {}", engine.sim.tick, c.replay.upto);
        }
        let head = engine.sim.tick;
        let settled = self.frontier().min(head);
        format!("tick {head} ({} ahead of settled), {} rollbacks, deepest {}", head - settled, self.stats.rollbacks, self.stats.max_depth)
    }

    /// Send what is due, receive what has arrived, go back and play again if a guess was wrong, and play new ticks (at most `max_steps` of
    /// them). Returns how many new ticks were played; 0 means waiting: for the network (the window is full), or the game is over.
    pub fn advance(&mut self, engine: &mut Engine, max_steps: usize) -> Result<usize, String> {
        if self.finished.is_some() {
            return Ok(0);
        }
        let mut catching = self.catching_up.take();
        let out = if catching.is_some() { self.catch_up(engine, max_steps, &mut catching) } else { self.advance_live(engine, max_steps) };
        if catching.is_some() {
            self.catching_up = catching;
        }
        out
    }

    // ------------------------------------------------------------------------------------------ receiving
    fn receive(&mut self, head: u64, mismatch: &mut Option<u64>, catching: &mut Option<CatchUp>) {
        loop {
            match self.rx.try_recv() {
                Ok(m) => self.on_message(m, head, mismatch, catching, false),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.dropped {
                        self.dropped = true;
                        // the game is over for us wherever the host's last word got to: end on what is settled
                        self.end_reason.get_or_insert_with(|| "the connection to the host ended".to_string());
                        let settled = self.frontier().min(head);
                        self.end_tick.get_or_insert(settled);
                    }
                    break;
                }
            }
        }
    }

    /// `own_ok`: inputs that name our own seat are accepted (what the host hands a returning player: its own inputs from before it left).
    fn on_message(&mut self, m: Json, head: u64, mismatch: &mut Option<u64>, catching: &mut Option<CatchUp>, own_ok: bool) {
        if let Some(i) = m.get("input") {
            let (Some(p), Some(t), Some(v)) = (i.get("player").and_then(|v| v.as_u64()), i.get("tick").and_then(|v| v.as_u64()), i.get("values")) else { return };
            let p = p as usize;
            if catching.is_some() {
                // still replaying: everything the host forwards is kept until this copy is a player again (our own old inputs included)
                self.deferred.push(m.clone());
                return;
            }
            if p >= self.book.players() || (p == self.me && !own_ok) {
                return;
            }
            // A real input that arrives for a tick we already played: we played it with the guess "no change", so it only matters if it
            // changes something. (It cannot have been known before: each input arrives once, and the book ignores repeats.)
            let changes = !is_empty_input(v);
            // (kept even if the seat is frozen as far as we know: the news that it is back may come after its first inputs)
            if self.book.put_told(t, p, v.clone()) && changes && t < head && !self.book.is_known_empty_by_freeze(t, p) {
                *mismatch = Some(mismatch.map_or(t, |x| x.min(t)));
            }
        } else if let Some(s) = m.get("seat") {
            if catching.is_some() {
                self.deferred.push(m.clone());
            } else {
                self.on_seat(s);
            }
        } else if m.get("log").is_some() {
            if let Some(c) = catching.as_mut() {
                c.replay.add(&m);
            }
        } else if let Some(g) = m.get("go") {
            if let (Some(c), Some(r)) = (catching.as_mut(), g.get("tick").and_then(|v| v.as_u64())) {
                let next = g.get("next").and_then(|v| v.as_u64()).unwrap_or(r).max(r);
                c.go = Some((r, next));
            }
        } else if let Some(f) = m.get("frontier") {
            if let Some(t) = f.get("tick").and_then(|v| v.as_u64()) {
                self.host_frontier = self.host_frontier.max(t);
            }
        } else if let Some(d) = m.get("desync") {
            self.desync = Some(d.to_string_lossy());
        } else if m.get("bye").is_some() {
            self.end_reason = Some(reason_of(&m));
            let given = m.get("bye").and_then(|b| b.get("tick")).and_then(|t| t.as_u64());
            let settled = self.frontier().min(head);
            self.end_tick = Some(given.unwrap_or(settled));
        }
    }

    fn on_seat(&mut self, s: &Json) {
        let (Some(p), Some(state)) = (s.get("player").and_then(|v| v.as_u64()), s.get("state").and_then(|v| v.as_str())) else { return };
        let p = p as usize;
        let tick = s.get("tick").and_then(|v| v.as_u64());
        let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let reason = s.get("reason").and_then(|v| v.as_str()).unwrap_or("");
        if p >= self.book.players() {
            return;
        }
        match state {
            "empty" => self.events.push(format!("{name} (seat {}) left: {reason}; the game waits for it a moment, then the seat repeats its last keys", p + 1)),
            "frozen" | "live" => {
                let (freeze, tick) = (state == "frozen", tick.unwrap_or(0));
                match s.get("n").and_then(|v| v.as_u64()) {
                    None => self.apply_seat(p, freeze, tick, name),
                    Some(n) => {
                        // in the order the host numbered them, whatever order they arrived in
                        let expected = *self.seat_next[p].get_or_insert(n);
                        if n < expected {
                            return;
                        }
                        self.seat_held[p].insert(n, (freeze, tick, name.to_string()));
                        while let Some(e) = self.seat_next[p] {
                            let Some((f, t, who)) = self.seat_held[p].remove(&e) else { break };
                            self.apply_seat(p, f, t, &who);
                            self.seat_next[p] = Some(e + 1);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn apply_seat(&mut self, p: usize, freeze: bool, tick: u64, name: &str) {
        if freeze {
            if p != self.me {
                // everything this copy had settled before the freeze is settled for good (it is below the seat's last real input)
                let before = self.book.frontier();
                self.book.freeze(p, tick);
                self.host_frontier = self.host_frontier.max(before.min(tick));
            }
            self.events.push(format!("seat {} ({name}) is frozen from tick {tick}: it repeats its last keys until its player comes back", p + 1));
        } else {
            if p != self.me {
                self.book.unfreeze(p, tick);
                // inputs of the player who is back may have arrived before this news and been kept: the ones for ticks we have played with the
                // seat counted as empty are corrections now
                if let Some(t) = self.book.first_change_from(p, tick) {
                    self.late_change = Some(self.late_change.map_or(t, |x| x.min(t)));
                }
            }
            self.events.push(format!("{name} is back in seat {} from tick {tick}", p + 1));
        }
    }

    // ------------------------------------------------------------------------------------------ sending
    fn send_line(&mut self, v: &Json) {
        if let Err(e) = send(&mut self.stream, v) {
            self.dropped = true;
            self.end_reason.get_or_insert(e);
        }
    }

    fn send_hash(&mut self, tick: u64, hash: String) {
        self.send_line(&Json::obj([("hash", Json::obj([("tick", Json::from(tick as i64)), ("hash", Json::str(hash))]))]));
    }

    /// One input message for every tick up to `input_delay` ticks ahead of the simulation, the way the lockstep client does it.
    fn send_inputs(&mut self, head: u64) {
        while !self.dropped && self.next_send <= head + self.input_delay {
            let mut changed: Vec<(String, Json)> = Vec::new();
            if self.next_send >= self.quiet_until {
                for (k, v) in &self.held {
                    if self.sent.get(k) != Some(v) {
                        changed.push((k.clone(), if *v == 0.0 || *v == 1.0 { Json::Bool(*v == 1.0) } else { Json::from(*v) }));
                    }
                }
                for (k, _) in &changed {
                    if let Some(v) = self.held.get(k) {
                        self.sent.insert(k.clone(), *v);
                    }
                }
            }
            let values = Json::Obj(changed);
            // our own input is known as soon as it is decided: it is never a guess
            self.book.put(self.next_send, self.me, values.clone());
            let msg = Json::obj([("input", Json::obj([("tick", Json::from(self.next_send as i64)), ("values", values)]))]);
            self.send_line(&msg);
            self.next_send += 1;
        }
    }

    // ------------------------------------------------------------------------------------------ playing
    /// The last tick this copy may play up to (a tick limit, or the tick the game ends on).
    fn cap(&self) -> u64 {
        let mut cap = u64::MAX;
        if let Some(m) = self.welcome.max_ticks {
            cap = cap.min(m);
        }
        if let Some(e) = self.end_tick {
            cap = cap.min(e);
        }
        cap
    }

    /// Play one tick: save the state it starts from, apply every input we know for it (the others are guesses: no change), step.
    fn step_tick(&mut self, engine: &mut Engine) -> Result<bool, String> {
        let t = engine.sim.tick;
        if self.ring.back().map(|s| s.tick) != Some(t) {
            let point = engine.sim.checkpoint();
            // the hash of the state at the start of tick t is the hash "after tick t - 1": reported for the ticks that are multiples of
            // `hash_every`, but only once the tick is settled (see `settle`); computing it now is the cheapest moment, once every 30 ticks
            let hash = (self.hash_every > 0 && t >= 1 && (t - 1) % self.hash_every == 0).then(|| engine.sim.state_hash());
            self.ring.push_back(Slot { tick: t, point, hash });
        }
        let mut guessed = false;
        let mut inputs: Vec<(usize, &Json)> = Vec::new();
        for p in 0..self.book.players() {
            match self.book.value_for(t, p) {
                Some(v) => inputs.push((p, v)),
                None => guessed |= !self.book.is_known(t, p),
            }
        }
        apply_tick_inputs(engine, inputs)?;
        engine.sim.step();
        Ok(guessed)
    }

    /// Go back to the saved state of tick `t` and play up to where we were, with what we know now.
    fn rollback(&mut self, engine: &mut Engine, t: u64) -> Result<(), String> {
        let head = engine.sim.tick;
        let Some(idx) = self.ring.iter().position(|s| s.tick == t) else {
            return Err(format!("cannot go back to tick {t}: it is outside the window of {} saved ticks (a bug, or a host that does not match this client)", self.window));
        };
        engine.sim.rewind_to(&self.ring[idx].point).map_err(|e| e.to_string())?;
        // the saves after it are made again as the ticks are played again
        self.ring.truncate(idx + 1);
        self.stats.rollbacks += 1;
        self.stats.max_depth = self.stats.max_depth.max(head - t);
        while engine.sim.tick < head {
            self.step_tick(engine)?;
            self.stats.resimulated_ticks += 1;
        }
        Ok(())
    }

    /// Hashes of settled states are reported, saves of settled states are dropped, inputs nobody asks for again are forgotten.
    fn settle(&mut self, head: u64) {
        let c = self.frontier().min(head);
        let mut due: Vec<(u64, String)> = Vec::new();
        for s in self.ring.iter_mut() {
            if s.tick <= c {
                if let Some(h) = s.hash.take() {
                    due.push((s.tick - 1, hex(h)));
                }
            }
        }
        for (k, h) in due {
            self.send_hash(k, h);
        }
        while self.ring.front().is_some_and(|s| s.tick < c) {
            self.ring.pop_front();
        }
        self.book.prune(c);
    }

    fn advance_live(&mut self, engine: &mut Engine, max_steps: usize) -> Result<usize, String> {
        let mut mismatch = None;
        let mut none = None;
        self.receive(engine.sim.tick, &mut mismatch, &mut none);
        if let Some(t) = self.late_change.take() {
            if t < engine.sim.tick {
                mismatch = Some(mismatch.map_or(t, |x: u64| x.min(t)));
            }
        }
        // a guess was wrong: back to the earliest tick it affected
        if let Some(t) = mismatch {
            self.rollback(engine, t)?;
        }
        // the game is over: end exactly on the tick the host named (every copy must), going back if we had played past it
        if let Some(end) = self.end_tick {
            if engine.sim.tick > end {
                let Some(idx) = self.ring.iter().position(|s| s.tick == end) else { return Err(format!("cannot go back to the last tick of the game ({end}): the host's tick does not match this client's saved ticks")) };
                engine.sim.rewind_to(&self.ring[idx].point).map_err(|e| e.to_string())?;
                self.ring.truncate(idx + 1);
            }
        }
        self.settle(engine.sim.tick);
        self.send_inputs(engine.sim.tick);
        let mut stepped = 0;
        let mut stalled = false;
        while stepped < max_steps {
            let head = engine.sim.tick;
            if head >= self.cap() {
                break;
            }
            if head.saturating_sub(self.frontier()) >= self.window {
                stalled = true;
                break;
            }
            // our own input for the tick about to be played is normally known already (it was sent `input_delay` ticks ago); with no delay it
            // is the keys as they are now
            if !self.book.is_known(head, self.me) {
                self.send_inputs(head);
            }
            let guessed = self.step_tick(engine)?;
            stepped += 1;
            self.stats.ticks += 1;
            if guessed {
                self.stats.guessed_ticks += 1;
            }
        }
        if stalled && stepped == 0 {
            self.stats.stalls += 1;
        }
        self.settle(engine.sim.tick);
        self.check_finished(engine);
        Ok(stepped)
    }

    /// The game is over for this copy when it stands on the tick the host named and that tick is settled.
    fn check_finished(&mut self, engine: &mut Engine) {
        let Some(end) = self.end_tick else { return };
        if self.finished.is_some() || engine.sim.tick != end || self.book.frontier() < end {
            return;
        }
        // the last settled state may not have been saved (nothing was played from it): report its hash from the live state
        let k = end.wrapping_sub(1);
        if end >= 1 && self.hash_every > 0 && k % self.hash_every == 0 && !self.ring.iter().any(|s| s.tick == end) && !self.dropped {
            self.send_hash(k, hex(engine.sim.state_hash()));
        }
        self.finished = Some(self.end_reason.clone().unwrap_or_else(|| "the game is over".to_string()));
    }

    // ------------------------------------------------------------------------------------------ coming back
    /// Replay the log the host sends, as a spectator would; when there is nothing left to replay ask for the seat; when it is given, go live.
    fn catch_up(&mut self, engine: &mut Engine, max_steps: usize, catching: &mut Option<CatchUp>) -> Result<usize, String> {
        let mut mismatch = None;
        self.receive(engine.sim.tick, &mut mismatch, catching);
        let Some(c) = catching.as_mut() else { return Ok(0) };
        // never play past the tick the seat is given back at, or the end of the game
        let limit = match (c.go.map(|(r, _)| r), self.end_tick) {
            (Some(r), Some(e)) => Some(r.min(e)),
            (a, b) => a.or(b),
        };
        let (done, hashes) = c.replay.play(engine, max_steps, limit, self.hash_every)?;
        let reached = engine.sim.tick;
        let upto = c.replay.upto;
        let ask = !c.resume_sent && c.go.is_none() && c.replay.chunks > 0 && reached >= upto;
        if ask {
            c.resume_sent = true;
        }
        let go = c.go;
        for (k, h) in hashes {
            self.send_hash(k, h);
        }
        if ask {
            self.send_line(&Json::obj([("resume", Json::obj([("tick", Json::from(reached as i64))]))]));
        }
        if let Some((r, next)) = go {
            if reached == r && upto >= r {
                self.go_live(engine, r, next);
                *catching = None;
            }
        } else if let Some(end) = self.end_tick {
            // the game ended while we were still catching up: play what is there up to its last tick, then we are as finished as any
            if reached == end && upto >= end {
                *catching = None;
                self.book = InputBook::new(self.book.players(), end);
                self.finished = Some(self.end_reason.clone().unwrap_or_else(|| "the game is over".to_string()));
            }
        }
        Ok(done)
    }

    /// The replay is complete up to tick `r`: from here this copy is a player again, with the seat from tick `next`.
    fn go_live(&mut self, engine: &mut Engine, r: u64, next: u64) {
        self.book = InputBook::new(self.book.players(), r);
        self.host_frontier = self.host_frontier.max(r);
        self.ring.clear();
        self.next_send = next;
        self.quiet_until = 0;
        // keys the old life of this seat left held would stay held for ever (nobody releases them): the first input lets go of them
        for k in held_keys(engine, self.me) {
            self.sent.insert(k.clone(), 1.0);
            self.held.entry(k).or_insert(0.0);
        }
        self.events.push(format!("back in the game at tick {r} (own inputs from tick {next})"));
        let deferred = std::mem::take(&mut self.deferred);
        let mut mismatch = None;
        let mut none = None;
        for m in deferred {
            self.on_message(m, r, &mut mismatch, &mut none, true);
        }
    }

    /// Leave the game politely. The seat stays yours: come back with the token.
    pub fn leave(&mut self, reason: &str) {
        let _ = send(&mut self.stream, &bye(reason));
        self.finished.get_or_insert_with(|| reason.to_string());
    }

    /// Does this copy play with the guesses of `Mode::Rollback`? (Always: a convenience for code that handles both kinds of seat.)
    pub fn mode(&self) -> Mode {
        Mode::Rollback
    }
}

impl Drop for Rollback {
    /// Dropping a seat hangs up, like a program that ends: the reader thread holds the socket too, so without this the connection would
    /// stay open and the host would not notice that the player is gone.
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}
