//! Joining a protocol-2 game (rollback, a lobby, watching, taking a seat back) and the one type that stands for "my place in the game",
//! whatever kind it is: [`Seat`]. Plain lockstep without a lobby still goes through `Lockstep::join`, exactly as before.
use crate::client::{Lockstep, Welcome};
use crate::rollback::Rollback;
use crate::watch::Watcher;
use crate::wire::{bye, hex, reason_of, send, spawn_reader};
use crate::{Mode, VERSION_2};
use lunaris_api::Engine;
use lunaris_json::Json;
use std::net::TcpStream;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// What a player in a lobby can ask for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LobbyCommand {
    /// Mark myself ready (or not ready) to start.
    Ready(bool),
    /// Start the game now (only the first seat's request counts): whoever has loaded the scene plays.
    Start,
}

/// One line of the lobby's list of players.
#[derive(Clone, Debug, PartialEq)]
pub struct RosterEntry {
    pub player: usize,
    pub name: String,
    /// The player has loaded the scene (its world matches the first player's).
    pub loaded: bool,
    pub ready: bool,
}

/// Who is in the lobby, as the host reports it to everybody whenever it changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Roster {
    pub players: Vec<RosterEntry>,
    /// How many seats the game has.
    pub max: usize,
}

impl Roster {
    fn parse(r: &Json) -> Roster {
        let players = r
            .get("players")
            .and_then(|p| p.as_array())
            .map(|a| {
                a.iter()
                    .map(|e| RosterEntry {
                        player: e.get("player").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
                        name: e.get("name").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
                        loaded: e.get("loaded").and_then(|v| v.as_bool()).unwrap_or(false),
                        ready: e.get("ready").and_then(|v| v.as_bool()).unwrap_or(false),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Roster { players, max: r.get("max").and_then(|v| v.as_u64()).unwrap_or(0) as usize }
    }

    /// A line for a terminal: `lobby (2 of 3): Ada [ready], Bo [loading]`.
    pub fn describe(&self) -> String {
        let list: Vec<String> = self.players.iter().map(|p| format!("{} [{}]", p.name, if !p.loaded { "loading" } else if p.ready { "ready" } else { "not ready" })).collect();
        format!("lobby ({} of {}): {}", self.players.len(), self.max, list.join(", "))
    }
}

/// How to join (see [`connect`]).
pub struct JoinOptions {
    /// The name the others see.
    pub name: String,
    /// The mode the host must be running; a host in the other mode refuses with a message that says so.
    pub mode: Mode,
    /// Take no seat: watch the game from tick 0 (rollback hosts only).
    pub watch: bool,
    /// The seat token a previous join was given: take that seat back (rollback hosts only).
    pub token: Option<String>,
    /// Rollback: how many ticks your own keys are held back so that everybody can have them in time (default: what the host suggests).
    pub input_delay: Option<u64>,
    /// Send the state hash of every settled tick that is a multiple of this (0: never).
    pub hash_every: u64,
    /// Speak protocol 2 even in lockstep mode: needed for a host with a lobby.
    pub lobby: bool,
    /// Lobby commands, if the player controls them; without it a client marks itself ready at once (a bot).
    pub commands: Option<mpsc::Receiver<LobbyCommand>>,
    /// Called with every new roster while waiting in a lobby.
    pub on_roster: Option<Box<dyn FnMut(&Roster) + Send>>,
}

impl JoinOptions {
    pub fn new(name: &str, mode: Mode) -> JoinOptions {
        JoinOptions { name: name.to_string(), mode, watch: false, token: None, input_delay: None, hash_every: 30, lobby: false, commands: None, on_roster: None }
    }
}

/// My place in a game, whatever kind: lockstep, rollback, or a spectator. One type so that a window or a bot is written once.
pub enum Seat {
    Lockstep(Lockstep),
    Rollback(Rollback),
    Watch(Watcher),
}

impl Seat {
    pub fn welcome(&self) -> &Welcome {
        match self {
            Seat::Lockstep(s) => &s.welcome,
            Seat::Rollback(s) => &s.welcome,
            Seat::Watch(s) => &s.welcome,
        }
    }

    /// Press or release a key. (A spectator has no keys: this does nothing.)
    pub fn set_key(&mut self, key: &str, value: f64) {
        match self {
            Seat::Lockstep(s) => s.set_key(key, value),
            Seat::Rollback(s) => s.set_key(key, value),
            Seat::Watch(_) => {}
        }
    }

    /// Send what is due, take in what has arrived and play ticks: at most `max_steps` new ones. 0 means waiting (or over).
    pub fn advance(&mut self, engine: &mut Engine, max_steps: usize) -> Result<usize, String> {
        match self {
            Seat::Lockstep(s) => s.advance(engine, max_steps),
            Seat::Rollback(s) => s.advance(engine, max_steps),
            Seat::Watch(s) => s.advance(engine, max_steps),
        }
    }

    /// The host's report of a divergence, once.
    pub fn take_desync(&mut self) -> Option<String> {
        match self {
            Seat::Lockstep(s) => s.desync.take(),
            Seat::Rollback(s) => s.desync.take(),
            Seat::Watch(s) => s.desync.take(),
        }
    }

    /// Why the game is over, once this copy has played all of it.
    pub fn finished(&self) -> Option<&str> {
        match self {
            Seat::Lockstep(s) => s.finished.as_deref(),
            Seat::Rollback(s) => s.finished.as_deref(),
            Seat::Watch(s) => s.finished.as_deref(),
        }
    }

    /// Is this copy far behind the game and replaying it from the log (a spectator that has just arrived, a player taking its seat back)?
    /// A window should then play as many ticks per frame as it can instead of one tick per tick of the clock.
    pub fn catching_up(&self, engine: &Engine) -> bool {
        match self {
            Seat::Lockstep(_) => false,
            Seat::Rollback(s) => s.is_catching_up(),
            Seat::Watch(s) => s.lag(engine) > 120,
        }
    }

    /// Ticks that have arrived and wait to be played (lockstep only; 0 for the others).
    pub fn ready_ticks(&self) -> usize {
        match self {
            Seat::Lockstep(s) => s.ready_ticks(),
            _ => 0,
        }
    }

    pub fn leave(&mut self, reason: &str) {
        match self {
            Seat::Lockstep(s) => s.leave(reason),
            Seat::Rollback(s) => s.leave(reason),
            Seat::Watch(s) => s.leave(reason),
        }
    }

    /// News about the seats and the game (somebody left, a seat froze, somebody came back), oldest first; reading it clears it.
    pub fn take_events(&mut self) -> Vec<String> {
        match self {
            Seat::Lockstep(_) => Vec::new(),
            Seat::Rollback(s) => s.take_events(),
            Seat::Watch(s) => s.take_events(),
        }
    }

    /// The seat token, to come back with.
    pub fn token(&self) -> Option<&str> {
        self.welcome().token.as_deref()
    }

    /// A short line for a window title.
    pub fn status(&self, engine: &Engine) -> String {
        match self {
            Seat::Lockstep(_) => format!("lockstep, tick {}", engine.sim.tick),
            Seat::Rollback(s) => format!("rollback, {}", s.status(engine)),
            Seat::Watch(s) => format!("watching, tick {} ({} behind)", engine.sim.tick, s.lag(engine)),
        }
    }

    pub fn set_hash_every(&mut self, every: u64) {
        match self {
            Seat::Lockstep(s) => s.hash_every = every,
            Seat::Rollback(s) => s.hash_every = every,
            Seat::Watch(s) => s.hash_every = every,
        }
    }

    /// Rolling-back numbers (rollback seats only).
    pub fn rollback_stats(&self) -> Option<crate::RollbackStats> {
        match self {
            Seat::Rollback(s) => Some(s.stats),
            _ => None,
        }
    }
}

/// A receiver that gives `early` first and then everything that comes from `rx`.
fn replay_first(early: Vec<Json>, rx: mpsc::Receiver<Json>) -> mpsc::Receiver<Json> {
    let (tx, out) = mpsc::channel();
    std::thread::spawn(move || {
        for m in early {
            if tx.send(m).is_err() {
                return;
            }
        }
        while let Ok(m) = rx.recv() {
            if tx.send(m).is_err() {
                return;
            }
        }
    });
    out
}

/// Join a game with protocol 2 (or plain lockstep when nothing asks for more): connect, learn the seed, let `prepare` build this copy's
/// engine (create it with `welcome.seed` and load the scene), report the state hash of the loaded world, wait in the lobby if there is one,
/// and return my place in the game with the engine. A host that refuses (another mode, another world, a full game, an unknown token) is an
/// error that says why. Blocks until the game starts.
pub fn connect(addr: &str, mut opts: JoinOptions, prepare: impl FnOnce(&Welcome) -> Result<Engine, String>) -> Result<(Seat, Engine), String> {
    let plain = opts.mode == Mode::Lockstep && !opts.lobby && !opts.watch && opts.token.is_none();
    if plain {
        let (mut ls, engine) = Lockstep::join(addr, &opts.name, prepare)?;
        ls.hash_every = opts.hash_every;
        return Ok((Seat::Lockstep(ls), engine));
    }
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("cannot connect to {addr}: {e}"))?;
    stream.set_nodelay(true).ok();
    let rx = spawn_reader(stream.try_clone().map_err(|e| e.to_string())?);
    let mut hello = Json::obj([("name", Json::str(opts.name.clone())), ("version", Json::from(VERSION_2)), ("mode", Json::str(opts.mode.name())), ("role", Json::str(if opts.watch { "watch" } else { "player" }))]);
    if let Some(t) = &opts.token {
        hello.set("token", Json::str(t.clone()));
    }
    send(&mut stream, &Json::obj([("hello", hello)]))?;
    let first = rx.recv_timeout(Duration::from_secs(600)).map_err(|_| "the host did not answer".to_string())?;
    if first.get("bye").is_some() {
        return Err(format!("the host refused: {}", reason_of(&first)));
    }
    let w = first.get("welcome").ok_or("the host did not welcome us")?;
    let mut welcome = Welcome::parse(w)?;
    if welcome.mode != opts.mode && !opts.watch {
        let _ = send(&mut stream, &bye("this client plays another mode"));
        return Err(format!("the host runs {} and this client was asked for {}", welcome.mode.name(), opts.mode.name()));
    }
    let engine = match prepare(&welcome) {
        Ok(e) => e,
        Err(e) => {
            let _ = send(&mut stream, &bye(format!("player {} could not load the game: {e}", welcome.player)));
            return Err(e);
        }
    };
    send(&mut stream, &Json::obj([("ready", Json::obj([("hash", Json::str(hex(engine.sim.state_hash())))]))]))?;

    // wait for the start (other players may take a while to join; in a lobby they have to be ready)
    let deadline = Instant::now() + Duration::from_secs(1800);
    let mut auto_ready_sent = false;
    // Messages that come before `start` are not ours to drop: on a network that reorders them (or the latency simulator, which holds some
    // messages back longer than others) another player's first inputs can overtake the `start` that was sent before them.
    let mut early: Vec<Json> = Vec::new();
    loop {
        if Instant::now() >= deadline {
            return Err("the game never started".to_string());
        }
        if welcome.lobby {
            if opts.commands.is_none() && !auto_ready_sent {
                auto_ready_sent = true;
                send(&mut stream, &Json::obj([("lobby", Json::obj([("ready", Json::Bool(true))]))]))?;
            }
            if let Some(cmds) = &opts.commands {
                while let Ok(c) = cmds.try_recv() {
                    let m = match c {
                        LobbyCommand::Ready(r) => Json::obj([("lobby", Json::obj([("ready", Json::Bool(r))]))]),
                        LobbyCommand::Start => Json::obj([("lobby", Json::obj([("start", Json::Bool(true))]))]),
                    };
                    send(&mut stream, &m)?;
                }
            }
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(m) => {
                if let Some(r) = m.get("roster") {
                    if let Some(cb) = opts.on_roster.as_mut() {
                        cb(&Roster::parse(r));
                    }
                } else if let Some(a) = m.get("assign") {
                    // the seat numbers shift while people come and go in a lobby: the last word is the one that counts
                    if let (Some(p), Some(n)) = (a.get("player").and_then(|v| v.as_u64()), a.get("players").and_then(|v| v.as_u64())) {
                        welcome.player = p as usize;
                        welcome.players = n as usize;
                    }
                } else if let Some(s) = m.get("start") {
                    if let (Some(p), Some(n)) = (s.get("player").and_then(|v| v.as_u64()), s.get("players").and_then(|v| v.as_u64())) {
                        welcome.player = p as usize;
                        welcome.players = n as usize;
                    }
                    break;
                } else if m.get("bye").is_some() {
                    return Err(format!("the host refused: {}", reason_of(&m)));
                } else {
                    early.push(m);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err("the host closed the connection before the game started".to_string()),
        }
    }
    welcome.lobby = false;
    let rx = if early.is_empty() { rx } else { replay_first(early, rx) };
    let seat = if welcome.watching {
        Seat::Watch(Watcher::new(welcome, stream, rx, opts.hash_every))
    } else if welcome.mode == Mode::Rollback {
        let delay = opts.input_delay.unwrap_or(welcome.delay);
        let rejoin = welcome.rejoin;
        let mut r = Rollback::new(welcome, stream, rx, delay, rejoin);
        r.hash_every = opts.hash_every;
        Seat::Rollback(r)
    } else {
        let mut ls = Lockstep::from_parts(welcome, stream, rx);
        ls.hash_every = opts.hash_every;
        Seat::Lockstep(ls)
    };
    Ok((seat, engine))
}
