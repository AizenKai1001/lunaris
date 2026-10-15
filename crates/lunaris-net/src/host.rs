//! The relay: accepts the players, checks that they all loaded the same world, then collects one input message per player per tick and
//! sends each complete tick to everybody. It does not run the game.
//!
//! This file is plain **lockstep** (protocol 1), exactly as it always was. A host started in rollback mode, or with a lobby, is run by
//! `hostv2.rs`, which hands a lobby game back to `lockstep_relay` below when the mode is lockstep.
use crate::log::InputLog;
use crate::wire::{bye, hex, reason_of, send, spawn_reader};
use crate::{Mode, VERSION, VERSION_2};
use lunaris_json::Json;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What the host did and why it stopped.
#[derive(Debug, Default, Clone)]
pub struct HostReport {
    pub players: usize,
    /// Ticks sent to everybody (in a rollback game: the tick the game ended on, which every copy ends on).
    pub ticks: u64,
    /// One line per divergence found.
    pub desyncs: Vec<String>,
    /// Why the game ended.
    pub ended: String,
    /// Rollback games: every key change of the game (what a spectator is sent). Empty for lockstep.
    pub log: InputLog,
    /// Rollback games: what happened to the seats and the watchers, one line each, in order.
    pub events: Vec<String>,
    /// Rollback games: how many spectators joined, and how many times a player came back to a seat.
    pub watchers: usize,
    pub rejoins: usize,
}

#[derive(Clone, Debug)]
pub struct HostConfig {
    pub players: usize,
    /// How many ticks ahead of the simulation a player's input is scheduled: input typed now takes effect this many ticks later,
    /// which gives the network time to deliver everybody's input. 3 ticks is 50 ms at 60 Hz. (Rollback: the delay clients use unless
    /// they say otherwise; 0 to 60.)
    pub delay: u64,
    pub seed: u64,
    /// Stop after this many ticks (None: until a player leaves).
    pub max_ticks: Option<u64>,
    /// How long to wait for a player to join (all of them, in total).
    pub join_timeout: Duration,
    /// How long to wait for any message once the game runs; a player who sends nothing for this long ends the game.
    pub timeout: Duration,
    /// How the game is played: lockstep (the default, exactly as before) or rollback.
    pub mode: Mode,
    /// Rollback: how many ticks a client may run ahead of the oldest tick it is still waiting for somebody's input for. Also how far back a
    /// rollback can go. Announced to every client. 1 to 64.
    pub window: u64,
    /// Wait in a lobby instead of starting as soon as `players` seats are taken: players join with a name and mark themselves ready, and
    /// the game starts when all `players` seats are taken and everybody is ready, or when the first seat says `start` (then the players
    /// that have loaded the scene play, however many they are). Lockstep or rollback.
    pub lobby: bool,
    /// Rollback: how long an empty seat (a player who left or stopped sending) holds the game up before it is *frozen*: from then on it
    /// repeats its last input (the keys it held stay held) and the others play on. Everybody is told.
    pub grace: Duration,
    /// Rollback: the most memory the input log may use. A game that produces more keeps playing and the spectators already watching keep
    /// watching, but the oldest changes are thrown away, so nobody can join late or come back any more.
    pub max_log_bytes: usize,
    /// Rollback: how many spectators may watch at once.
    pub max_watchers: usize,
}

impl Default for HostConfig {
    fn default() -> HostConfig {
        HostConfig {
            players: 2,
            delay: 3,
            seed: 1,
            max_ticks: None,
            join_timeout: Duration::from_secs(600),
            timeout: Duration::from_secs(60),
            mode: Mode::Lockstep,
            window: 8,
            lobby: false,
            grace: Duration::from_secs(3),
            max_log_bytes: 64 << 20,
            max_watchers: 16,
        }
    }
}

pub struct Host {
    listener: TcpListener,
}

impl Host {
    /// Listen on `addr` (for example `0.0.0.0:7000`, or `127.0.0.1:0` for any free port on this machine).
    pub fn bind(addr: impl ToSocketAddrs) -> std::io::Result<Host> {
        Ok(Host { listener: TcpListener::bind(addr)? })
    }

    pub fn port(&self) -> u16 {
        self.listener.local_addr().map(|a| a.port()).unwrap_or(0)
    }

    /// Accept the players, then relay ticks until the game ends. Blocks.
    pub fn run(self, cfg: &HostConfig) -> Result<HostReport, String> {
        if cfg.players == 0 || cfg.players > 16 {
            return Err("a game has between 1 and 16 players".into());
        }
        if cfg.mode == Mode::Lockstep && !cfg.lobby {
            if cfg.delay == 0 || cfg.delay > 60 {
                return Err("the input delay is between 1 and 60 ticks".into());
            }
            return self.run_lockstep(cfg);
        }
        if cfg.delay > 60 {
            return Err("the input delay is at most 60 ticks".into());
        }
        if cfg.mode == Mode::Lockstep && cfg.delay == 0 {
            return Err("the input delay of a lockstep game is between 1 and 60 ticks".into());
        }
        if cfg.window == 0 || cfg.window > 64 {
            return Err("the rollback window is between 1 and 64 ticks".into());
        }
        crate::hostv2::run(self.listener, cfg)
    }

    /// Plain lockstep, protocol 1: the original host.
    fn run_lockstep(self, cfg: &HostConfig) -> Result<HostReport, String> {
        let mut report = HostReport { players: cfg.players, ..HostReport::default() };
        let mut streams: Vec<TcpStream> = Vec::new();
        let mut inbox: Vec<mpsc::Receiver<Json>> = Vec::new();

        // ---- join
        self.listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let deadline = Instant::now() + cfg.join_timeout;
        while streams.len() < cfg.players {
            let stream = match self.listener.accept() {
                Ok((s, _)) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(format!("only {} of {} players joined in time", streams.len(), cfg.players));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(e) => return Err(format!("cannot accept a player: {e}")),
            };
            stream.set_nonblocking(false).map_err(|e| e.to_string())?;
            stream.set_nodelay(true).ok();
            let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
            let rx = spawn_reader(stream);
            let Ok(hello) = rx.recv_timeout(Duration::from_secs(10)) else { continue }; // somebody who connects and says nothing
            let version = hello.get("hello").and_then(|h| h.get("version")).and_then(|v| v.as_i64());
            if version != Some(VERSION) {
                // a client that speaks protocol 2 wants rollback, a lobby or to watch: say what to do instead of just "no"
                let why = if version == Some(VERSION_2) {
                    format!("this host speaks lockstep version {VERSION} and has no lobby: rollback, lobby and watching need a host started with --rollback or --lobby (join without them to play lockstep)")
                } else {
                    format!("this host speaks lockstep version {VERSION}")
                };
                let _ = send(&mut writer, &bye(why));
                continue;
            }
            let player = streams.len();
            let welcome = Json::obj([("welcome", Json::obj([("player", Json::from(player)), ("players", Json::from(cfg.players)), ("seed", Json::str(hex(cfg.seed))), ("delay", Json::from(cfg.delay as i64))]))]);
            send(&mut writer, &welcome)?;
            streams.push(writer);
            inbox.push(rx);
        }

        // ---- everybody loads the scene and reports the hash of what they loaded
        let mut hashes: Vec<String> = Vec::new();
        for (p, rx) in inbox.iter().enumerate() {
            let ready = match rx.recv_timeout(cfg.join_timeout) {
                Ok(m) => m,
                Err(RecvTimeoutError::Disconnected) => return Err(format!("player {p} left before the game started")),
                Err(RecvTimeoutError::Timeout) => return Err(format!("player {p} never got ready")),
            };
            hashes.push(ready.get("ready").and_then(|r| r.get("hash")).and_then(|h| h.as_str()).unwrap_or("").to_string());
        }
        if hashes.iter().any(|h| h != &hashes[0]) {
            let msg = format!("the players' worlds are not the same at the start (hashes {hashes:?}): different scene files, seeds or engine builds");
            for s in &mut streams {
                let _ = send(s, &bye(msg.clone()));
            }
            report.desyncs.push(msg.clone());
            report.ended = msg;
            return Ok(report);
        }
        lockstep_relay(streams, inbox, cfg, report, Some(self.listener))
    }
}

/// Anybody who connects while a lockstep game is running is told why they cannot come in, instead of being left waiting for an answer
/// that never comes. Stops when the returned flag is set.
fn refuse_latecomers(listener: TcpListener) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        let _ = listener.set_nonblocking(true);
        while !flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((s, _)) => {
                    std::thread::spawn(move || {
                        let _ = s.set_nonblocking(false);
                        let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                        // read the hello first: answering before the client has finished sending it can make the answer get lost
                        let mut line = String::new();
                        let _ = BufReader::new(&s).read_line(&mut line);
                        let mut w = s;
                        let _ = send(&mut w, &bye("the game has already started (a lockstep host takes no spectators and nobody rejoins; a host started with --rollback does)"));
                    });
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    });
    stop
}

/// Lockstep from the moment everybody is ready: start, then collect one input per player per tick and release complete ticks.
/// `streams` and `inbox` are in player order. `listener`, when given, keeps answering latecomers with a refusal.
pub(crate) fn lockstep_relay(mut streams: Vec<TcpStream>, inbox: Vec<mpsc::Receiver<Json>>, cfg: &HostConfig, mut report: HostReport, listener: Option<TcpListener>) -> Result<HostReport, String> {
    let refusing = listener.map(refuse_latecomers);
    let result = lockstep_loop(&mut streams, inbox, cfg, &mut report);
    if let Some(flag) = refusing {
        flag.store(true, Ordering::SeqCst);
    }
    result.map(|()| report)
}

fn lockstep_loop(streams: &mut [TcpStream], inbox: Vec<mpsc::Receiver<Json>>, cfg: &HostConfig, report: &mut HostReport) -> Result<(), String> {
    for s in streams.iter_mut() {
        send(s, &Json::obj([("start", Json::Obj(Vec::new()))]))?;
    }
    // the first `delay` ticks have nobody's input: send them at once so that everybody can start
    let mut released = 0u64;
    for t in 0..cfg.delay {
        let tick = Json::obj([("tick", Json::obj([("t", Json::from(t as i64)), ("inputs", Json::arr([]))]))]);
        for s in streams.iter_mut() {
            send(s, &tick)?;
        }
        released += 1;
    }
    report.ticks = released;

    // ---- relay: one thread per player reads, this loop decides
    let (tx, rx) = mpsc::channel::<(usize, Json)>();
    for (p, r) in inbox.into_iter().enumerate() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            while let Ok(m) = r.recv() {
                if tx.send((p, m)).is_err() {
                    return;
                }
            }
            let _ = tx.send((p, Json::obj([("gone", Json::Bool(true))])));
        });
    }
    drop(tx);
    let mut pending: BTreeMap<u64, BTreeMap<usize, Json>> = BTreeMap::new();
    let mut state_hashes: BTreeMap<u64, BTreeMap<usize, String>> = BTreeMap::new();
    let end = loop {
        if cfg.max_ticks.is_some_and(|m| released >= m) {
            break "the game reached its tick limit".to_string();
        }
        let (p, msg) = match rx.recv_timeout(cfg.timeout) {
            Ok(x) => x,
            Err(_) => break "a player stopped sending (timeout)".to_string(),
        };
        if msg.get("gone").is_some() {
            break format!("player {p} left");
        }
        if msg.get("bye").is_some() {
            break format!("player {p} left: {}", reason_of(&msg));
        }
        if let Some(i) = msg.get("input") {
            let (Some(t), Some(values)) = (i.get("tick").and_then(|t| t.as_u64()), i.get("values")) else { continue };
            if t < released || t > released + 10_000 {
                continue; // late (the host already ran that tick), or absurdly early
            }
            pending.entry(t).or_default().entry(p).or_insert_with(|| values.clone());
            // release every tick that is complete, in order
            while pending.get(&released).is_some_and(|e| e.len() == cfg.players) {
                let entry = pending.remove(&released).unwrap_or_default();
                let inputs: Vec<Json> = entry.iter().filter(|(_, v)| v.as_object().is_some_and(|o| !o.is_empty())).map(|(pl, v)| Json::arr([Json::from(*pl), v.clone()])).collect();
                let tick = Json::obj([("tick", Json::obj([("t", Json::from(released as i64)), ("inputs", Json::Arr(inputs))]))]);
                for s in streams.iter_mut() {
                    send(s, &tick)?;
                }
                released += 1;
                report.ticks = released;
                if cfg.max_ticks.is_some_and(|m| released >= m) {
                    break;
                }
            }
        } else if let Some(h) = msg.get("hash") {
            let (Some(t), Some(hv)) = (h.get("tick").and_then(|t| t.as_u64()), h.get("hash").and_then(|v| v.as_str())) else { continue };
            let entry = state_hashes.entry(t).or_default();
            entry.insert(p, hv.to_string());
            if entry.len() == cfg.players {
                let all: Vec<String> = entry.values().cloned().collect();
                if all.iter().any(|x| x != &all[0]) {
                    report.desyncs.push(format!("tick {t}: the players' worlds differ ({all:?})"));
                    let m = Json::obj([("desync", Json::obj([("tick", Json::from(t as i64)), ("hashes", Json::arr(all.iter().map(|x| Json::str(x.clone()))))]))]);
                    for s in streams.iter_mut() {
                        let _ = send(s, &m);
                    }
                }
                state_hashes.remove(&t);
            }
        }
    };
    // a hash from the last ticks may still be on its way: give the players a moment to report before we say goodbye
    let grace = Instant::now() + Duration::from_millis(300);
    while report.desyncs.is_empty() && Instant::now() < grace && !state_hashes.is_empty() {
        let Ok((p, msg)) = rx.recv_timeout(Duration::from_millis(50)) else { continue };
        if let Some(h) = msg.get("hash") {
            if let (Some(t), Some(hv)) = (h.get("tick").and_then(|t| t.as_u64()), h.get("hash").and_then(|v| v.as_str())) {
                let entry = state_hashes.entry(t).or_default();
                entry.insert(p, hv.to_string());
                if entry.len() == cfg.players {
                    let all: Vec<String> = entry.values().cloned().collect();
                    if all.iter().any(|x| x != &all[0]) {
                        report.desyncs.push(format!("tick {t}: the players' worlds differ ({all:?})"));
                    }
                    state_hashes.remove(&t);
                }
            }
        }
    }
    for s in streams.iter_mut() {
        let _ = send(s, &bye(end.clone()));
    }
    report.ended = end;
    Ok(())
}
