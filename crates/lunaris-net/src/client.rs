//! One player's end of a lockstep game: the connection, the keys held, and the ticks that have arrived but not been stepped yet.
use crate::wire::{bye, hex, reason_of, send, spawn_reader};
use crate::{key_name, Mode, VERSION};
use lunaris_api::Engine;
use lunaris_json::Json;
use std::collections::BTreeMap;
use std::net::TcpStream;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

/// What a client learns when it joins.
#[derive(Clone, Debug)]
pub struct Welcome {
    /// This player's number, from 0.
    pub player: usize,
    pub players: usize,
    /// The seed every copy of the game starts with.
    pub seed: u64,
    /// How many ticks an input is scheduled ahead (see `HostConfig::delay`).
    pub delay: u64,
    /// How the game is played (a protocol-1 host is always lockstep).
    pub mode: Mode,
    /// Rollback: how far ahead of the oldest unsettled tick a client may run, and so how far back a rollback can go.
    pub window: u64,
    /// Rollback: the game stops at this tick, when the host was started with a tick limit.
    pub max_ticks: Option<u64>,
    /// The seat token (protocol 2): present it to take the seat back after a drop. A nickname for the seat, not a secret: the wire is
    /// not encrypted.
    pub token: Option<String>,
    /// The host has a lobby: the game starts when the players are ready.
    pub lobby: bool,
    /// This is a spectator: it has no seat and never sends input.
    pub watching: bool,
    /// This is a player taking its old seat back: the game is replayed from tick 0 first.
    pub rejoin: bool,
}

impl Welcome {
    /// Read the `welcome` object of either protocol.
    pub(crate) fn parse(w: &Json) -> Result<Welcome, String> {
        let flag = |k: &str| w.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
        Ok(Welcome {
            player: w.get("player").and_then(|v| v.as_u64()).ok_or("bad welcome")? as usize,
            players: w.get("players").and_then(|v| v.as_u64()).ok_or("bad welcome")? as usize,
            seed: u64::from_str_radix(w.get("seed").and_then(|v| v.as_str()).ok_or("bad welcome")?, 16).map_err(|_| "bad seed in the welcome")?,
            delay: w.get("delay").and_then(|v| v.as_u64()).ok_or("bad welcome")?,
            mode: w.get("mode").and_then(|v| v.as_str()).and_then(Mode::from_name).unwrap_or(Mode::Lockstep),
            window: w.get("window").and_then(|v| v.as_u64()).unwrap_or(8),
            max_ticks: w.get("max_ticks").and_then(|v| v.as_u64()),
            token: w.get("token").and_then(|v| v.as_str()).map(str::to_string),
            lobby: flag("lobby"),
            watching: flag("watch"),
            rejoin: flag("rejoin"),
        })
    }
}

pub struct Lockstep {
    pub welcome: Welcome,
    stream: TcpStream,
    rx: mpsc::Receiver<Json>,
    /// Ticks that have arrived from the host, waiting to be stepped.
    ready: BTreeMap<u64, Vec<(usize, Json)>>,
    held: BTreeMap<String, f64>,
    sent: BTreeMap<String, f64>,
    next_send: u64,
    /// Send the state hash every this many ticks (0: never).
    pub hash_every: u64,
    /// Set when the host reports that two copies of the game disagree.
    pub desync: Option<String>,
    /// Set when the game is over (the host said so, or the connection ended). Ticks that already arrived can still be stepped.
    pub finished: Option<String>,
}

impl Lockstep {
    /// Connect, learn the seed, let `prepare` build this player's engine (create it with that seed and load the scene), report the state
    /// hash of the loaded world and wait for the start. Blocks until every player has joined.
    pub fn join(addr: &str, name: &str, prepare: impl FnOnce(&Welcome) -> Result<Engine, String>) -> Result<(Lockstep, Engine), String> {
        let mut stream = TcpStream::connect(addr).map_err(|e| format!("cannot connect to {addr}: {e}"))?;
        stream.set_nodelay(true).ok();
        let rx = spawn_reader(stream.try_clone().map_err(|e| e.to_string())?);
        send(&mut stream, &Json::obj([("hello", Json::obj([("name", Json::str(name)), ("version", Json::from(VERSION))]))]))?;
        let w = rx.recv_timeout(Duration::from_secs(600)).map_err(|_| "the host did not answer".to_string())?;
        if w.get("bye").is_some() {
            return Err(format!("the host refused: {}", reason_of(&w)));
        }
        let w = w.get("welcome").ok_or("the host did not welcome us")?;
        let welcome = Welcome::parse(w)?;
        let engine = match prepare(&welcome) {
            Ok(e) => e,
            Err(e) => {
                let _ = send(&mut stream, &bye(format!("player {} could not load the game: {e}", welcome.player)));
                return Err(e);
            }
        };
        send(&mut stream, &Json::obj([("ready", Json::obj([("hash", Json::str(hex(engine.sim.state_hash())))]))]))?;
        // wait for the start (other players may take a while to join)
        loop {
            let m = rx.recv_timeout(Duration::from_secs(1800)).map_err(|_| "the game never started".to_string())?;
            if m.get("start").is_some() {
                break;
            }
            if m.get("bye").is_some() {
                return Err(format!("the game was cancelled: {}", reason_of(&m)));
            }
        }
        Ok((Lockstep::from_parts(welcome, stream, rx), engine))
    }

    /// A seat whose handshake was done elsewhere (the protocol-2 handshake of `connect`, for a lockstep game that waited in a lobby).
    pub(crate) fn from_parts(welcome: Welcome, stream: TcpStream, rx: mpsc::Receiver<Json>) -> Lockstep {
        let delay = welcome.delay;
        Lockstep { welcome, stream, rx, ready: BTreeMap::new(), held: BTreeMap::new(), sent: BTreeMap::new(), next_send: delay, hash_every: 30, desync: None, finished: None }
    }

    /// Press or release a key (or set an axis) for this player. It reaches the host with the next tick message, and takes effect
    /// `delay` ticks later in every copy of the game.
    pub fn set_key(&mut self, key: &str, value: f64) {
        self.held.insert(key.to_string(), value);
    }

    /// The tick the simulation has to reach before the host can have everybody's input for it: how far this player is ahead of the
    /// slowest one is `ready_ticks()`.
    pub fn ready_ticks(&self) -> usize {
        self.ready.len()
    }

    /// Send the inputs this player owes the host, and step every tick whose inputs have all arrived (at most `max_steps`). Returns how
    /// many ticks were stepped; 0 means still waiting for somebody (or the game is over).
    pub fn advance(&mut self, engine: &mut Engine, max_steps: usize) -> Result<usize, String> {
        // what has arrived
        loop {
            match self.rx.try_recv() {
                Ok(m) => {
                    if let Some(t) = m.get("tick") {
                        let (Some(n), Some(inputs)) = (t.get("t").and_then(|v| v.as_u64()), t.get("inputs").and_then(|v| v.as_array())) else { continue };
                        let list: Vec<(usize, Json)> = inputs.iter().filter_map(|e| Some((e.index(0)?.as_u64()? as usize, e.index(1)?.clone()))).collect();
                        self.ready.insert(n, list);
                    } else if let Some(d) = m.get("desync") {
                        self.desync = Some(d.to_string_lossy());
                    } else if m.get("bye").is_some() {
                        self.finished = Some(reason_of(&m));
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.finished.get_or_insert_with(|| "the connection to the host ended".to_string());
                    break;
                }
            }
        }
        // what we owe: one input message for every tick up to `delay` ticks ahead of the simulation
        while self.finished.is_none() && self.next_send <= engine.sim.tick + self.welcome.delay {
            let mut changed: Vec<(String, Json)> = Vec::new();
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
            let msg = Json::obj([("input", Json::obj([("tick", Json::from(self.next_send as i64)), ("values", Json::Obj(changed))]))]);
            if let Err(e) = send(&mut self.stream, &msg) {
                self.finished.get_or_insert(e);
                break;
            }
            self.next_send += 1;
        }
        // step
        let mut stepped = 0;
        while stepped < max_steps {
            let tick = engine.sim.tick;
            let Some(inputs) = self.ready.remove(&tick) else { break };
            for (player, values) in inputs {
                let Some(obj) = values.as_object() else { continue };
                let mapped: Vec<(String, Json)> = obj.iter().map(|(k, v)| (key_name(player, k), v.clone())).collect();
                engine.call("input", &Json::obj([("values", Json::Obj(mapped))])).map_err(|e| e.to_string())?;
            }
            engine.sim.step();
            stepped += 1;
            if self.hash_every > 0 && tick % self.hash_every == 0 {
                let msg = Json::obj([("hash", Json::obj([("tick", Json::from(tick as i64)), ("hash", Json::str(hex(engine.sim.state_hash())))]))]);
                if let Err(e) = send(&mut self.stream, &msg) {
                    self.finished.get_or_insert(e);
                }
            }
        }
        Ok(stepped)
    }

    /// Leave the game politely (the host ends the game for everybody).
    pub fn leave(&mut self, reason: &str) {
        let _ = send(&mut self.stream, &bye(reason));
        self.finished.get_or_insert_with(|| reason.to_string());
    }
}
