//! The host of protocol 2: rollback games, lobbies, spectators and players who come back. (Plain lockstep with protocol 1 is `host.rs`.)
//!
//! The host still does not run the game. In a **rollback** game it forwards every player's input to the others the moment it arrives
//! (nobody waits for a complete tick), and it keeps two things to be useful to people who arrive later:
//!
//! * an [`InputBook`] with everything it knows about every player's inputs, whose *frontier* is the tick below which everything is settled;
//! * the **input log**: the key changes of every tick below the frontier, in order. A spectator, or a player who comes back, is sent the log
//!   from tick 0 and plays it at full speed to catch up, then follows the frontier live.
//!
//! One loop owns all of it; connections only read (one thread each, tagging each message with the connection it came from) and write
//! (a queue and a thread each once the game runs, so that one slow client cannot hold the others up).
use crate::book::{InputBook, OPEN};
use crate::host::{lockstep_relay, HostConfig, HostReport};
use crate::log::{InputLog, LogEntry};
use crate::wire::{bye, bye_at, reason_of, send, spawn_line_reader, Outbox, Xorshift};
use crate::{Mode, VERSION, VERSION_2};
use lunaris_json::Json;
use std::collections::{BTreeMap, VecDeque};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How many bytes may wait in a spectator's queue before the host stops filling it (it is filled again as the queue drains).
const PUMP_QUEUE: usize = 256 << 10;
/// The most input changes and the most bytes in one `log` message.
const CHUNK_ENTRIES: usize = 200;
const CHUNK_BYTES: usize = 48_000;
/// How far ahead of what a seat has sent an input may be and still be believed.
const MAX_AHEAD: u64 = 10_000;
/// The state hashes the host remembers to compare later reports with.
const MAX_HASHES: usize = 200_000;

enum Ev {
    Hello { id: u64, stream: TcpStream, hello: Json },
    Msg(u64, Json),
    Gone(u64),
}

/// A way to write to a connection: straight (blocking, with a timeout) while the game has not started and messages are few, through a
/// queue and a writer thread once it runs.
struct Link {
    stream: TcpStream,
    out: Option<Outbox>,
}

impl Link {
    fn new(stream: TcpStream) -> Link {
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        Link { stream, out: None }
    }

    /// From now on, writes are queued (and never block the host).
    fn queue(&mut self) {
        if self.out.is_none() {
            let _ = self.stream.set_write_timeout(None);
            self.out = self.stream.try_clone().ok().and_then(|s| Outbox::new(s).ok());
        }
    }

    /// False when the connection is broken or hopelessly behind.
    fn send(&mut self, v: &Json) -> bool {
        match &self.out {
            Some(o) => o.send(v),
            None => send(&mut self.stream, v).is_ok(),
        }
    }

    fn queued(&self) -> usize {
        self.out.as_ref().map_or(0, |o| o.queued_bytes())
    }

    /// Finish politely: what is queued is still sent.
    fn close(&mut self) {
        match &mut self.out {
            Some(o) => o.close(),
            None => {
                let _ = self.stream.shutdown(Shutdown::Write);
            }
        }
    }

    fn kill(&mut self) {
        match &mut self.out {
            Some(o) => o.kill(),
            None => {
                let _ = self.stream.shutdown(Shutdown::Both);
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Player,
    Watcher,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Stage {
    /// Welcomed; waiting for the player to say its world is loaded.
    Loading,
    /// Loaded, waiting in the lobby for the game to start.
    Lobby,
    /// A spectator, or a returning player who is catching up: being sent the log. `cursor` counts log entries sent; `sent_upto` is the
    /// last `upto` told.
    Following { cursor: u64, sent_upto: u64 },
    /// A returning player who says it has caught up and wants its seat: still being sent the log until it is complete.
    Resuming { cursor: u64, sent_upto: u64 },
    /// A player in the game.
    Live,
}

struct Conn {
    link: Link,
    name: String,
    kind: Kind,
    stage: Stage,
    /// The seat of a player (once the game has started or the seat is being taken back).
    seat: Option<usize>,
    /// How many `log` messages this connection has been sent: each carries its number, so that a receiver can put them in order even if
    /// the network (or the latency simulator) delivers them in another.
    chunks: u64,
}

/// A player waiting in the lobby.
struct Waiting {
    id: u64,
    name: String,
    token: String,
    loaded: bool,
    ready: bool,
}

struct Seat {
    name: String,
    token: String,
    conn: Option<u64>,
    /// Since when nobody is in the seat (None while somebody is, or the seat is frozen).
    empty_since: Option<Instant>,
    /// The last time this seat made progress (an input that was new).
    last_input: Instant,
    /// How many times the seat has been frozen or given back: each of those messages carries its number so that receivers apply them in order.
    events: u64,
    /// The number of the message that froze the seat the last time.
    freeze_n: u64,
}

/// What the lobby decided.
enum Go {
    Rollback,
    Lockstep { streams: Vec<TcpStream>, ids: Vec<u64> },
}

struct Host2<'a> {
    cfg: &'a HostConfig,
    rx: mpsc::Receiver<Ev>,
    conns: BTreeMap<u64, Conn>,
    waiting: Vec<Waiting>,
    start_requested: bool,
    started: bool,
    players: usize,
    seats: Vec<Seat>,
    world_hash: Option<String>,
    book: InputBook,
    /// The frontier: every tick below it is settled and in the log.
    released: u64,
    log: VecDeque<(LogEntry, usize)>,
    /// How many entries were thrown away from the front of the log (0: the log is complete).
    log_first: u64,
    log_bytes: usize,
    /// The frontier last announced to the players (they need it only while a seat is frozen: see `Rollback::frontier`).
    announced: u64,
    /// For each tick whose hash was reported: who reported first and what.
    hash_ref: BTreeMap<u64, (String, String)>,
    desynced: Vec<u64>,
    last_activity: Instant,
    rng: Xorshift,
    report: HostReport,
}

fn clean_name(raw: Option<&str>) -> String {
    let s: String = raw.unwrap_or("player").chars().filter(|c| !c.is_control()).take(32).collect();
    if s.trim().is_empty() {
        "player".to_string()
    } else {
        s
    }
}

/// How far a spectator (or a player coming back) is told that the game is complete: the frontier, but never past the tick the game ends on. The frontier moves in jumps and can stand a tick or two beyond the
/// limit when the game is over; a follower told that those ticks are complete plays them before it hears where the game ended (`bye`), and ends on another tick than everybody else.
fn told_to_followers(released: u64, max_ticks: Option<u64>) -> u64 {
    max_ticks.map_or(released, |m| released.min(m))
}

pub(crate) fn run(listener: TcpListener, cfg: &HostConfig) -> Result<HostReport, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let (tx, rx) = mpsc::channel::<Ev>();
    let stop = Arc::new(AtomicBool::new(false));
    let acceptor = spawn_acceptor(listener, tx, stop.clone());
    // unpredictable enough for seat tokens, which are nicknames for a seat and not secrets (nothing on this wire is encrypted)
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) ^ 0xA5A5_5A5A_1234_8765;
    let mut host = Host2 {
        cfg,
        rx,
        conns: BTreeMap::new(),
        waiting: Vec::new(),
        start_requested: false,
        started: false,
        players: cfg.players,
        seats: Vec::new(),
        world_hash: None,
        book: InputBook::new(cfg.players, 0),
        released: 0,
        log: VecDeque::new(),
        log_first: 0,
        log_bytes: 0,
        announced: 0,
        hash_ref: BTreeMap::new(),
        desynced: Vec::new(),
        last_activity: Instant::now(),
        rng: Xorshift::new(seed),
        report: HostReport { players: cfg.players, ..HostReport::default() },
    };
    let out = match host.lobby_phase() {
        Err(e) => Err(e),
        Ok(Go::Rollback) => host.game_phase(),
        Ok(Go::Lockstep { streams, ids }) => {
            // a lockstep game that waited in a lobby: from here on it is the plain lockstep relay, fed from this loop's channel
            stop.store(true, Ordering::SeqCst);
            let listener = acceptor.join().ok();
            let mut cfg2 = host.cfg.clone();
            cfg2.players = streams.len();
            let report = HostReport { players: streams.len(), events: std::mem::take(&mut host.report.events), ..HostReport::default() };
            let mut senders = BTreeMap::new();
            let mut inbox = Vec::new();
            for id in ids {
                let (tx, rx) = mpsc::channel::<Json>();
                senders.insert(id, tx);
                inbox.push(rx);
            }
            let events = host.rx;
            std::thread::spawn(move || forward_to_seats(events, senders));
            lockstep_relay(streams, inbox, &cfg2, report, listener)
        }
    };
    stop.store(true, Ordering::SeqCst);
    out
}

/// After the lobby of a lockstep game: messages of each connection go to that seat's channel (which the lockstep relay reads); a connection
/// that ends closes its channel (which the relay reads as "the player left"); newcomers are refused.
fn forward_to_seats(rx: mpsc::Receiver<Ev>, mut seats: BTreeMap<u64, mpsc::Sender<Json>>) {
    while let Ok(ev) = rx.recv() {
        match ev {
            Ev::Msg(id, m) => {
                if let Some(tx) = seats.get(&id) {
                    if tx.send(m).is_err() {
                        seats.remove(&id);
                    }
                }
            }
            Ev::Gone(id) => {
                seats.remove(&id);
            }
            Ev::Hello { stream, .. } => refuse(stream, "the game has already started (a lockstep host takes no spectators and nobody rejoins; a host started with --rollback does)"),
        }
    }
}

fn spawn_acceptor(listener: TcpListener, tx: mpsc::Sender<Ev>, stop: Arc<AtomicBool>) -> JoinHandle<TcpListener> {
    std::thread::spawn(move || {
        let mut next_id = 1u64;
        while !stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let id = next_id;
                    next_id += 1;
                    let tx = tx.clone();
                    std::thread::spawn(move || connection(stream, id, tx));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(5)),
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        listener
    })
}

/// Read one connection: the first message must be the hello (within 10 s), everything after it goes to the host tagged with `id`.
fn connection(stream: TcpStream, id: u64, tx: mpsc::Sender<Ev>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let (Ok(writer), Ok(control)) = (stream.try_clone(), stream.try_clone()) else { return };
    let mut hello_seen = false;
    let mut writer = Some(writer);
    let tx2 = tx.clone();
    spawn_line_reader(
        stream,
        move |m| {
            if !hello_seen {
                hello_seen = true;
                let _ = control.set_read_timeout(None);
                match writer.take() {
                    Some(w) => tx.send(Ev::Hello { id, stream: w, hello: m }).is_ok(),
                    None => false,
                }
            } else {
                tx.send(Ev::Msg(id, m)).is_ok()
            }
        },
        move || {
            let _ = tx2.send(Ev::Gone(id));
        },
    );
}

fn refuse(mut stream: TcpStream, reason: impl Into<String>) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let _ = send(&mut stream, &bye(reason));
    let _ = stream.shutdown(Shutdown::Write);
}

impl<'a> Host2<'a> {
    fn make_token(&mut self) -> String {
        format!("{:016x}{:016x}", self.rng.next_u64(), self.rng.next_u64())
    }

    fn event(&mut self, line: String) {
        self.report.events.push(line);
    }

    // ------------------------------------------------------------------------------------------ hello
    /// Check a hello; the error says why it is refused. Returns the name, whether it wants to watch, and the seat token it offers.
    fn read_hello(&self, hello: &Json) -> Result<(String, bool, Option<String>), String> {
        let h = hello.get("hello").ok_or("the first message must be a hello")?;
        let version = h.get("version").and_then(|v| v.as_i64());
        let name = clean_name(h.get("name").and_then(|n| n.as_str()));
        if version == Some(VERSION) {
            return Err(if self.cfg.mode == Mode::Rollback {
                "this host runs a rollback game (protocol 2): join with --rollback (you joined as a lockstep player)".to_string()
            } else {
                "this host has a lobby (protocol 2): join with --lobby (you joined as a plain lockstep player)".to_string()
            });
        }
        if version != Some(VERSION_2) {
            return Err(format!("this host speaks protocol {VERSION_2} (rollback, lobby, watching) and lockstep protocol {VERSION}; you sent version {version:?}"));
        }
        let mode = match h.get("mode").and_then(|m| m.as_str()) {
            None => Mode::Lockstep,
            Some(m) => Mode::from_name(m).ok_or_else(|| format!("unknown mode `{m}` (lockstep or rollback)"))?,
        };
        let watch = h.get("role").and_then(|r| r.as_str()) == Some("watch");
        let token = h.get("token").and_then(|t| t.as_str()).map(str::to_string);
        if watch {
            if self.cfg.mode != Mode::Rollback {
                return Err("this host runs lockstep, which has no spectators: start it with --rollback to let people watch".to_string());
            }
        } else if mode != self.cfg.mode {
            return Err(format!("this host runs a {} game; you asked for {}: {}", self.cfg.mode.name(), mode.name(), if self.cfg.mode == Mode::Rollback { "join with --rollback" } else { "join without --rollback" }));
        }
        Ok((name, watch, token))
    }

    fn welcome(&self, player: usize, token: Option<&str>, watch: bool, rejoin: bool) -> Json {
        let mut w = Json::obj([
            ("player", Json::from(player)),
            ("players", Json::from(self.players)),
            ("seed", Json::str(format!("{:016x}", self.cfg.seed))),
            ("delay", Json::from(self.cfg.delay as i64)),
            ("mode", Json::str(self.cfg.mode.name())),
            ("window", Json::from(self.cfg.window as i64)),
            ("lobby", Json::Bool(self.cfg.lobby && !self.started)),
            ("watch", Json::Bool(watch)),
            ("rejoin", Json::Bool(rejoin)),
        ]);
        if let Some(t) = token {
            w.set("token", Json::str(t));
        }
        if let Some(m) = self.cfg.max_ticks {
            w.set("max_ticks", Json::from(m as i64));
        }
        Json::obj([("welcome", w)])
    }

    fn on_hello(&mut self, id: u64, stream: TcpStream, hello: Json) {
        let (name, watch, token) = match self.read_hello(&hello) {
            Ok(x) => x,
            Err(why) => return refuse(stream, why),
        };
        self.last_activity = Instant::now();
        if !self.started {
            if watch {
                return refuse(stream, "the game has not started yet: watch once it is running (a spectator is sent the whole game from tick 0)");
            }
            if token.is_some() {
                return refuse(stream, "the game has not started yet: there is no seat to take back");
            }
            if self.waiting.len() >= self.cfg.players {
                return refuse(stream, format!("the game is full ({} players)", self.cfg.players));
            }
            let token = self.make_token();
            let player = self.waiting.len();
            let welcome = self.welcome(player, Some(&token), false, false);
            let mut link = Link::new(stream);
            if !link.send(&welcome) {
                return;
            }
            self.conns.insert(id, Conn { link, name: name.clone(), kind: Kind::Player, stage: Stage::Loading, seat: None, chunks: 0 });
            self.waiting.push(Waiting { id, name, token, loaded: false, ready: false });
            self.send_roster();
            return;
        }
        // the game is running: a spectator, or a seat taken back
        if self.log_first > 0 {
            return refuse(stream, format!("the game is too old to join late: the host keeps at most {} MiB of input changes and has thrown the oldest away", self.cfg.max_log_bytes >> 20));
        }
        if watch {
            if self.conns.values().filter(|c| c.kind == Kind::Watcher).count() >= self.cfg.max_watchers {
                return refuse(stream, format!("too many spectators (at most {})", self.cfg.max_watchers));
            }
            let welcome = self.welcome(0, None, true, false);
            let mut link = Link::new(stream);
            if !link.send(&welcome) {
                return;
            }
            link.queue();
            self.conns.insert(id, Conn { link, name, kind: Kind::Watcher, stage: Stage::Loading, seat: None, chunks: 0 });
            return;
        }
        let Some(token) = token else {
            return refuse(stream, "the game has already started: you can watch it (lunaris watch), or take your old seat back with its token");
        };
        let Some(idx) = self.seats.iter().position(|s| s.token == token) else {
            return refuse(stream, "unknown seat token");
        };
        // somebody still sitting there (a connection that has not been noticed to be dead) is replaced
        if let Some(old) = self.seats[idx].conn.take() {
            if let Some(mut c) = self.conns.remove(&old) {
                let _ = c.link.send(&bye("this seat was taken back by a new connection with its token"));
                c.link.close(); // what is queued, the goodbye, is still sent
            }
            let line = format!("{} (seat {}): the old connection was replaced by a rejoin", self.seats[idx].name, idx + 1);
            self.event(line);
        }
        let welcome = self.welcome(idx, Some(&token), false, true);
        let mut link = Link::new(stream);
        if !link.send(&welcome) {
            self.seats[idx].empty_since = Some(Instant::now());
            return;
        }
        link.queue();
        self.seats[idx].conn = Some(id);
        self.seats[idx].empty_since = None;
        self.conns.insert(id, Conn { link, name, kind: Kind::Player, stage: Stage::Loading, seat: Some(idx), chunks: 0 });
    }

    // ------------------------------------------------------------------------------------------ the lobby
    fn roster(&self) -> Json {
        let players = self.waiting.iter().enumerate().map(|(i, w)| Json::obj([("player", Json::from(i)), ("name", Json::str(w.name.clone())), ("loaded", Json::Bool(w.loaded)), ("ready", Json::Bool(w.ready))]));
        Json::obj([("roster", Json::obj([("players", Json::arr(players)), ("max", Json::from(self.cfg.players)), ("watchers", Json::from(0usize))]))])
    }

    fn send_roster(&mut self) {
        if !self.cfg.lobby {
            return;
        }
        let r = self.roster();
        let ids: Vec<u64> = self.waiting.iter().map(|w| w.id).collect();
        for id in ids {
            if let Some(c) = self.conns.get_mut(&id) {
                if !c.link.send(&r) {
                    c.link.kill(); // its reader notices and the connection is removed
                }
            }
        }
    }

    /// A player in the lobby has gone (left, was refused, or its connection broke).
    fn lobby_remove(&mut self, id: u64) {
        let first = self.waiting.first().map(|w| w.id);
        self.waiting.retain(|w| w.id != id);
        if first == Some(id) {
            self.start_requested = false;
        }
        if let Some(mut c) = self.conns.remove(&id) {
            c.link.kill();
        }
        self.send_roster();
    }

    fn lobby_msg(&mut self, id: u64, msg: Json) {
        if let Some(r) = msg.get("ready") {
            let hash = r.get("hash").and_then(|h| h.as_str()).unwrap_or("").to_string();
            match self.world_hash.clone() {
                None => self.world_hash = Some(hash),
                Some(first) if first != hash => {
                    // the first player's world is the reference; anybody else with another one is sent away, the game goes on without them
                    let why = format!("your world is not the same as the first player's (hashes {hash} and {first}): different scene file, seed or engine build");
                    let who = self.conns.get(&id).map(|c| c.name.clone()).unwrap_or_default();
                    self.event(format!("refused {who}: its world is not the same as the first player's"));
                    if let Some(c) = self.conns.get_mut(&id) {
                        let _ = c.link.send(&bye(why));
                    }
                    self.lobby_remove(id);
                    return;
                }
                Some(_) => {}
            }
            if let Some(w) = self.waiting.iter_mut().find(|w| w.id == id) {
                w.loaded = true;
            }
            if let Some(c) = self.conns.get_mut(&id) {
                c.stage = Stage::Lobby;
            }
            self.send_roster();
        } else if let Some(l) = msg.get("lobby") {
            if !self.cfg.lobby {
                return;
            }
            if let Some(r) = l.get("ready").and_then(|r| r.as_bool()) {
                if let Some(w) = self.waiting.iter_mut().find(|w| w.id == id) {
                    w.ready = r;
                }
                self.send_roster();
            }
            if l.get("start").and_then(|s| s.as_bool()) == Some(true) && self.waiting.first().is_some_and(|w| w.id == id) {
                self.start_requested = true;
            }
        } else if msg.get("bye").is_some() {
            self.lobby_remove(id);
        }
    }

    fn ready_to_start(&self) -> bool {
        let n = self.waiting.len();
        if n == 0 {
            return false;
        }
        let full = n == self.cfg.players && self.waiting.iter().all(|w| w.loaded);
        if !self.cfg.lobby {
            return full;
        }
        (full && self.waiting.iter().all(|w| w.ready)) || (self.start_requested && self.waiting.first().is_some_and(|w| w.loaded))
    }

    /// Wait for the players (and, in a lobby, for them to be ready), then start.
    fn lobby_phase(&mut self) -> Result<Go, String> {
        let deadline = Instant::now() + self.cfg.join_timeout;
        loop {
            match self.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Ev::Hello { id, stream, hello }) => self.on_hello(id, stream, hello),
                Ok(Ev::Msg(id, msg)) => {
                    if self.conns.contains_key(&id) {
                        self.lobby_msg(id, msg);
                    }
                }
                Ok(Ev::Gone(id)) => {
                    if self.conns.contains_key(&id) {
                        self.lobby_remove(id);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err("the host's listener stopped".to_string()),
            }
            if self.ready_to_start() {
                break;
            }
            if Instant::now() >= deadline {
                return Err(format!("only {} of {} players joined in time", self.waiting.iter().filter(|w| w.loaded).count(), self.cfg.players));
            }
        }
        // who plays: the loaded ones, in the order they came; the others are told the game went on without them
        let dropped: Vec<u64> = self.waiting.iter().filter(|w| !w.loaded).map(|w| w.id).collect();
        for id in dropped {
            if let Some(c) = self.conns.get_mut(&id) {
                let _ = c.link.send(&bye("the game started without you: you had not loaded the scene in time"));
            }
            self.lobby_remove(id);
        }
        self.players = self.waiting.len();
        self.report.players = self.players;
        let players = self.players;
        let waiting = std::mem::take(&mut self.waiting);
        for (i, w) in waiting.iter().enumerate() {
            if let Some(c) = self.conns.get_mut(&w.id) {
                c.seat = Some(i);
                if self.cfg.lobby {
                    // the seat numbers may have shifted while people came and went
                    let _ = c.link.send(&Json::obj([("assign", Json::obj([("player", Json::from(i)), ("players", Json::from(players))]))]));
                }
            }
        }
        self.seats = waiting.iter().map(|w| Seat { name: w.name.clone(), token: w.token.clone(), conn: Some(w.id), empty_since: None, last_input: Instant::now(), events: 0, freeze_n: 0 }).collect();
        for (i, s) in self.seats.iter().enumerate() {
            self.report.events.push(format!("seat {} is {}", i + 1, s.name));
        }
        self.started = true;
        self.last_activity = Instant::now();
        if self.cfg.mode == Mode::Lockstep {
            // hand the seats to the lockstep relay: raw writers and, in `run`, one channel of messages per seat
            let mut streams = Vec::new();
            let mut ids = Vec::new();
            for w in &waiting {
                let Some(c) = self.conns.get(&w.id) else { return Err(format!("{} left before the game started", w.name)) };
                streams.push(c.link.stream.try_clone().map_err(|e| e.to_string())?);
                ids.push(w.id);
            }
            return Ok(Go::Lockstep { streams, ids });
        }
        self.book = InputBook::new(players, 0);
        for (i, w) in waiting.iter().enumerate() {
            if let Some(c) = self.conns.get_mut(&w.id) {
                c.stage = Stage::Live;
                c.link.queue();
                let _ = c.link.send(&Json::obj([("start", Json::obj([("player", Json::from(i)), ("players", Json::from(players)), ("tick", Json::from(0))]))]));
            }
        }
        Ok(Go::Rollback)
    }

    // ------------------------------------------------------------------------------------------ the game
    fn any_following(&self) -> bool {
        self.conns.values().any(|c| matches!(c.stage, Stage::Following { .. } | Stage::Resuming { .. }))
    }

    /// Relay until something ends the game.
    fn game_phase(&mut self) -> Result<HostReport, String> {
        let end = loop {
            // while somebody is being sent the log the loop wakes often, to keep their queue full
            let wait = if self.any_following() { Duration::from_millis(5) } else { Duration::from_millis(40) };
            match self.rx.recv_timeout(wait) {
                Ok(ev) => self.handle(ev),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break "the host's listener stopped".to_string(),
            }
            // whatever else is waiting is one round of work, not a round each
            for _ in 0..512 {
                match self.rx.try_recv() {
                    Ok(ev) => self.handle(ev),
                    Err(_) => break,
                }
            }
            if let Some(reason) = self.housekeeping() {
                break reason;
            }
        };
        Ok(self.finish(end))
    }

    fn handle(&mut self, ev: Ev) {
        match ev {
            Ev::Hello { id, stream, hello } => self.on_hello(id, stream, hello),
            Ev::Msg(id, msg) => self.on_msg(id, msg),
            Ev::Gone(id) => self.on_gone(id, "the connection ended"),
        }
    }

    fn on_msg(&mut self, id: u64, msg: Json) {
        let Some(c) = self.conns.get(&id) else { return };
        let (kind, stage, seat) = (c.kind, c.stage, c.seat);
        if kind == Kind::Player {
            self.last_activity = Instant::now(); // any word from a player is a sign of life
        }
        if msg.get("bye").is_some() {
            self.on_gone(id, &format!("said goodbye: {}", reason_of(&msg)));
            return;
        }
        match stage {
            Stage::Loading => {
                if let Some(r) = msg.get("ready") {
                    let hash = r.get("hash").and_then(|h| h.as_str()).unwrap_or("").to_string();
                    self.on_ready(id, hash);
                }
            }
            Stage::Live => {
                if let (Some(i), Some(seat)) = (msg.get("input"), seat) {
                    if let (Some(t), Some(values)) = (i.get("tick").and_then(|t| t.as_u64()), i.get("values")) {
                        self.on_input(seat, t, values.clone());
                    }
                } else if let Some(h) = msg.get("hash") {
                    self.on_hash_msg(id, h);
                }
            }
            Stage::Following { cursor, sent_upto } => {
                if msg.get("resume").is_some() && kind == Kind::Player {
                    if let Some(c) = self.conns.get_mut(&id) {
                        c.stage = Stage::Resuming { cursor, sent_upto };
                    }
                } else if let Some(h) = msg.get("hash") {
                    self.on_hash_msg(id, h);
                }
            }
            Stage::Resuming { .. } => {
                if let Some(h) = msg.get("hash") {
                    self.on_hash_msg(id, h);
                }
            }
            Stage::Lobby => {}
        }
    }

    /// A newcomer (a spectator, or a player taking its seat back) says its world is loaded.
    fn on_ready(&mut self, id: u64, hash: String) {
        let reference = self.world_hash.clone().unwrap_or_default();
        let Some(c) = self.conns.get(&id) else { return };
        let (kind, seat, name) = (c.kind, c.seat, c.name.clone());
        if hash != reference || self.log_first > 0 {
            let why = if hash != reference {
                format!("your world is not the same as the game's (hashes {hash} and {reference}): different scene file, seed or engine build")
            } else {
                "the game is too old to join late".to_string()
            };
            self.event(format!("refused {name}: {why}"));
            self.refuse_conn(id, why);
            return;
        }
        match kind {
            Kind::Watcher => {
                self.report.watchers += 1;
                self.event(format!("{name} is watching"));
                if let Some(c) = self.conns.get_mut(&id) {
                    c.stage = Stage::Following { cursor: 0, sent_upto: 0 };
                    let _ = c.link.send(&Json::obj([("start", Json::obj([("watch", Json::Bool(true)), ("players", Json::from(self.players)), ("tick", Json::from(0))]))]));
                }
            }
            Kind::Player => {
                let Some(seat) = seat else { return };
                // while the player replays the game its seat is frozen: the others must not wait for it
                if !self.book.is_frozen(seat) {
                    self.freeze_seat(seat, "its player is coming back and has to replay the game first");
                }
                let players = self.players;
                if let Some(c) = self.conns.get_mut(&id) {
                    c.stage = Stage::Following { cursor: 0, sent_upto: 0 };
                    let _ = c.link.send(&Json::obj([("start", Json::obj([("rejoin", Json::Bool(true)), ("player", Json::from(seat)), ("players", Json::from(players)), ("tick", Json::from(0))]))]));
                }
            }
        }
    }

    /// A player's input for `tick` reached the host.
    fn on_input(&mut self, seat: usize, tick: u64, values: Json) {
        if !matches!(values, Json::Obj(_)) || tick >= self.book.received_through(seat).saturating_add(MAX_AHEAD) {
            return;
        }
        let before = self.book.received_through(seat);
        if !self.book.put(tick, seat, values) {
            return; // a duplicate, or late for a stretch that was frozen
        }
        let after = self.book.received_through(seat);
        self.seats[seat].last_input = Instant::now();
        // only inputs that now form an unbroken run are forwarded: a seat can then be frozen from `through` without contradicting anything
        // a client has already been told
        let mut forward: Vec<Json> = Vec::new();
        for t in before..after {
            if let Some(v) = self.book.value_for(t, seat) {
                forward.push(Json::obj([("input", Json::obj([("player", Json::from(seat)), ("tick", Json::from(t as i64)), ("values", v.clone())]))]));
            }
        }
        let mut failed = Vec::new();
        for (cid, c) in self.conns.iter_mut() {
            if c.kind == Kind::Player && c.stage == Stage::Live && c.seat != Some(seat) {
                for m in &forward {
                    if !c.link.send(m) {
                        failed.push(*cid);
                        break;
                    }
                }
            }
        }
        for cid in failed {
            self.on_gone(cid, "its connection could not keep up");
        }
        self.advance_frontier();
    }

    /// Move the frontier forward as far as the inputs allow, and the settled changes into the log.
    fn advance_frontier(&mut self) {
        let f = self.book.frontier();
        if f == OPEN || f <= self.released {
            return;
        }
        for (t, p, v) in self.book.changes(self.released, f) {
            let size = 48 + v.to_string_lossy().len();
            self.log.push_back((LogEntry { tick: t, player: p, values: v }, size));
            self.log_bytes += size;
        }
        // bounded memory: when the log is too big the oldest changes go, and with them the possibility to join late
        while self.log_bytes > self.cfg.max_log_bytes && self.log.len() > 1 {
            if let Some((_, size)) = self.log.pop_front() {
                self.log_bytes -= size;
                self.log_first += 1;
                if self.log_first == 1 {
                    let line = format!("the input log reached its limit of {} MiB: the oldest changes are being thrown away, so nobody can join late or come back any more", self.cfg.max_log_bytes >> 20);
                    self.event(line);
                }
            }
        }
        self.released = f;
        self.book.prune(f);
        if self.book.any_frozen() {
            self.announce_frontier();
        }
    }

    /// Tell the players how far the game is settled. While a seat is frozen the players cannot work that out by themselves (the frozen seat's
    /// input counts as known for ever, so a player alone would run on without limit and settle ticks that the frozen seat's player, coming
    /// back, still changes): they keep within a window of this tick instead.
    fn announce_frontier(&mut self) {
        let m = Json::obj([("frontier", Json::obj([("tick", Json::from(self.released as i64))]))]);
        let mut failed = Vec::new();
        for (cid, c) in self.conns.iter_mut() {
            if c.kind == Kind::Player && c.stage == Stage::Live && !c.link.send(&m) {
                failed.push(*cid);
            }
        }
        self.announced = self.released;
        for cid in failed {
            self.on_gone(cid, "its connection could not keep up");
        }
    }

    fn on_hash_msg(&mut self, id: u64, h: &Json) {
        let (Some(t), Some(hv)) = (h.get("tick").and_then(|t| t.as_u64()), h.get("hash").and_then(|v| v.as_str())) else { return };
        let who = match self.conns.get(&id) {
            Some(c) if c.kind == Kind::Watcher => format!("spectator {}", c.name),
            Some(c) => format!("player {}", c.name),
            None => return,
        };
        self.on_hash(who, t, hv.to_string());
    }

    /// Compare a reported state hash with the first one reported for the same tick. Players report only settled ticks, so a difference is
    /// a real divergence, never a guess that was wrong.
    fn on_hash(&mut self, who: String, t: u64, h: String) {
        match self.hash_ref.get(&t) {
            None => {
                self.hash_ref.insert(t, (who, h));
                if self.hash_ref.len() > MAX_HASHES {
                    if let Some(first) = self.hash_ref.keys().next().copied() {
                        self.hash_ref.remove(&first);
                    }
                }
            }
            Some((who0, h0)) if *h0 != h && !self.desynced.contains(&t) => {
                let (who0, h0) = (who0.clone(), h0.clone());
                self.desynced.push(t);
                self.report.desyncs.push(format!("tick {t}: the players' worlds differ ({:?}): {who0} and {who}", [&h0, &h]));
                let m = Json::obj([("desync", Json::obj([("tick", Json::from(t as i64)), ("hashes", Json::arr([Json::str(h0), Json::str(h)]))]))]);
                for c in self.conns.values_mut() {
                    let _ = c.link.send(&m);
                }
            }
            Some(_) => {}
        }
    }

    // ------------------------------------------------------------------------------------------ seats
    /// Tell everybody in the game (players and spectators) something, except `except`.
    fn broadcast(&mut self, m: &Json, except: Option<u64>) {
        let mut failed = Vec::new();
        for (cid, c) in self.conns.iter_mut() {
            if Some(*cid) != except && matches!(c.stage, Stage::Live | Stage::Following { .. } | Stage::Resuming { .. }) && !c.link.send(m) {
                failed.push(*cid);
            }
        }
        for cid in failed {
            self.on_gone(cid, "its connection could not keep up");
        }
    }

    /// News about a seat. `frozen` and `live` messages carry `n`, the number of the change for that seat, so that they can be applied in
    /// order (a `live` that overtakes its `frozen` must not leave the seat frozen for ever).
    fn seat_message(&self, seat: usize, state: &str, tick: Option<u64>, reason: &str, n: Option<u64>) -> Json {
        let mut s = Json::obj([("player", Json::from(seat)), ("name", Json::str(self.seats[seat].name.clone())), ("state", Json::str(state)), ("reason", Json::str(reason))]);
        if let Some(t) = tick {
            s.set("tick", Json::from(t as i64));
        }
        if let Some(n) = n {
            s.set("n", Json::from(n as i64));
        }
        Json::obj([("seat", s)])
    }

    /// A connection has ended or was dropped.
    fn on_gone(&mut self, id: u64, why: &str) {
        self.drop_conn(id, why, false);
    }

    /// Send the reason and hang up (after the reason has been sent: a refusal nobody can read is no use).
    fn refuse_conn(&mut self, id: u64, reason: String) {
        if let Some(c) = self.conns.get_mut(&id) {
            let _ = c.link.send(&bye(reason));
        }
        self.drop_conn(id, "refused", true);
    }

    /// Forget a connection; a seat it held becomes empty. `graceful`: what is queued for it is still sent before the hang-up.
    fn drop_conn(&mut self, id: u64, why: &str, graceful: bool) {
        let Some(mut c) = self.conns.remove(&id) else { return };
        if graceful {
            c.link.close();
        } else {
            c.link.kill();
        }
        match c.kind {
            Kind::Watcher => self.event(format!("{} stopped watching ({why})", c.name)),
            Kind::Player => {
                let Some(seat) = c.seat else { return };
                if self.seats[seat].conn == Some(id) {
                    self.seats[seat].conn = None;
                    self.seats[seat].empty_since = Some(Instant::now());
                    let line = format!("{} left seat {} ({why}); the game waits {:.1} s for it, then the seat repeats its last keys", c.name, seat + 1, self.cfg.grace.as_secs_f64());
                    self.event(line);
                    let m = self.seat_message(seat, "empty", None, why, None);
                    self.broadcast(&m, None);
                }
            }
        }
    }

    /// From the tick up to which the seat's inputs have arrived on, its input is empty: the keys it held stay held, and nobody waits for it.
    fn freeze_seat(&mut self, seat: usize, why: &str) {
        if self.book.is_frozen(seat) {
            return;
        }
        let from = self.book.received_through(seat);
        self.book.freeze(seat, from);
        self.seats[seat].empty_since = None;
        let line = format!("seat {} ({}) is frozen from tick {from}: {why}; it repeats its last keys until its player comes back", seat + 1, self.seats[seat].name);
        self.event(line);
        let n = self.seats[seat].events;
        self.seats[seat].events += 1;
        self.seats[seat].freeze_n = n;
        let m = self.seat_message(seat, "frozen", Some(from), why, Some(n));
        self.broadcast(&m, None);
        self.advance_frontier();
        if self.announced != self.released {
            self.announce_frontier();
        }
    }

    /// Seats that have been empty for longer than the grace period are frozen; so are seats that are sitting there and have stopped sending
    /// while everybody else has gone on (a computer that hung rather than disconnected).
    fn freeze_check(&mut self, now: Instant) {
        for p in 0..self.players {
            if self.book.is_frozen(p) {
                continue;
            }
            match self.seats[p].conn {
                None => {
                    if self.seats[p].empty_since.is_some_and(|t| now.duration_since(t) >= self.cfg.grace) {
                        self.freeze_seat(p, "nobody was in it for the whole grace period");
                    }
                }
                Some(cid) => {
                    let live = self.conns.get(&cid).is_some_and(|c| c.stage == Stage::Live);
                    let mine = self.book.received_through(p);
                    let blocking = mine <= self.released && (0..self.players).any(|q| q != p && !self.book.is_frozen(q) && self.book.received_through(q) > mine);
                    if live && blocking && now.duration_since(self.seats[p].last_input) >= self.cfg.grace {
                        if let Some(mut c) = self.conns.remove(&cid) {
                            let _ = c.link.send(&bye("you stopped sending while everybody else went on: your seat is frozen"));
                            c.link.close();
                        }
                        self.seats[p].conn = None;
                        self.freeze_seat(p, "its player stopped sending");
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------------------------------ spectators and returning players
    /// Send a spectator, or a returning player, the next part of the log (as much as its queue takes, up to `limit` bytes waiting).
    fn pump(&mut self, id: u64, limit: usize) {
        let released = told_to_followers(self.released, self.cfg.max_ticks);
        let log_first = self.log_first;
        let end = log_first + self.log.len() as u64;
        let mut failed = false;
        let mut handoff = false;
        {
            let Some(c) = self.conns.get_mut(&id) else { return };
            let mut chunks = c.chunks;
            let (mut cursor, mut sent_upto, resuming) = match c.stage {
                Stage::Following { cursor, sent_upto } => (cursor, sent_upto, false),
                Stage::Resuming { cursor, sent_upto } => (cursor, sent_upto, true),
                _ => return,
            };
            if cursor < log_first {
                let _ = c.link.send(&bye("you fell too far behind the game and the host no longer has what you missed"));
                failed = true;
            } else {
                loop {
                    if c.link.queued() > limit {
                        break;
                    }
                    if cursor < end {
                        let mut inputs = Vec::new();
                        let mut bytes = 0;
                        let mut i = cursor;
                        while i < end && inputs.len() < CHUNK_ENTRIES && bytes < CHUNK_BYTES {
                            let (e, size) = &self.log[(i - log_first) as usize];
                            inputs.push(Json::arr([Json::from(e.tick as i64), Json::from(e.player), e.values.clone()]));
                            bytes += size;
                            i += 1;
                        }
                        // the ticks below the next change are complete (they had no changes), unless this chunk ends in the middle of a tick
                        let upto = (if i < end { self.log[(i - log_first) as usize].0.tick } else { released }).min(released);
                        let msg = Json::obj([("log", Json::obj([("n", Json::from(chunks as i64)), ("upto", Json::from(upto as i64)), ("inputs", Json::Arr(inputs))]))]);
                        chunks += 1;
                        if !c.link.send(&msg) {
                            failed = true;
                            break;
                        }
                        cursor = i;
                        sent_upto = upto;
                    } else if released > sent_upto {
                        let msg = Json::obj([("log", Json::obj([("n", Json::from(chunks as i64)), ("upto", Json::from(released as i64)), ("inputs", Json::arr([]))]))]);
                        chunks += 1;
                        if !c.link.send(&msg) {
                            failed = true;
                            break;
                        }
                        sent_upto = released;
                    } else {
                        break;
                    }
                }
                c.chunks = chunks;
                c.stage = if resuming { Stage::Resuming { cursor, sent_upto } } else { Stage::Following { cursor, sent_upto } };
                // a returning player that has been sent everything up to the frontier takes its seat back
                handoff = resuming && cursor == end && sent_upto == released && !failed;
            }
        }
        if failed {
            self.drop_conn(id, "its connection could not keep up", true);
        } else if handoff {
            self.complete_handoff(id);
        }
    }

    /// A returning player has the whole log up to the frontier `r`. It gets the seat back from tick `next`: the inputs that are known
    /// but not settled yet, the state of the other frozen seats, and the word to go.
    fn complete_handoff(&mut self, id: u64) {
        let Some(seat) = self.conns.get(&id).and_then(|c| c.seat) else { return };
        let r = self.released;
        // the seat's own inputs run through `f` (it is frozen from there); if the others have not settled that far, the player has to
        // carry on from `f`, not from `r`, because inputs it gave in an earlier life are already known to everybody
        let f = self.book.received_through(seat);
        let next = r.max(f);
        let frozen: Vec<(usize, u64)> = (0..self.players).filter(|&q| q != seat).filter_map(|q| self.book.frozen_from(q).map(|from| (q, from))).collect();
        self.book.unfreeze(seat, next);
        let held = self.book.held_from(r);
        let mut messages: Vec<Json> = Vec::new();
        for (q, from) in frozen {
            messages.push(self.seat_message(q, "frozen", Some(from), "frozen before you came back", Some(self.seats[q].freeze_n)));
        }
        for (t, p, v) in held {
            messages.push(Json::obj([("input", Json::obj([("player", Json::from(p)), ("tick", Json::from(t as i64)), ("values", v)]))]));
        }
        messages.push(Json::obj([("go", Json::obj([("tick", Json::from(r as i64)), ("next", Json::from(next as i64))]))]));
        let mut ok = true;
        if let Some(c) = self.conns.get_mut(&id) {
            for m in &messages {
                ok &= c.link.send(m);
            }
            c.stage = Stage::Live;
        }
        if !ok {
            self.on_gone(id, "its connection could not keep up");
            return;
        }
        self.seats[seat].last_input = Instant::now();
        self.seats[seat].empty_since = None;
        self.report.rejoins += 1;
        let line = format!("{} is back in seat {} from tick {next} (it replayed the game up to tick {r})", self.seats[seat].name, seat + 1);
        self.event(line);
        let n = self.seats[seat].events;
        self.seats[seat].events += 1;
        let m = self.seat_message(seat, "live", Some(next), "its player came back", Some(n));
        self.broadcast(&m, Some(id));
        self.advance_frontier();
    }

    fn pump_all(&mut self, limit: usize) {
        let ids: Vec<u64> = self.conns.iter().filter(|(_, c)| matches!(c.stage, Stage::Following { .. } | Stage::Resuming { .. })).map(|(id, _)| *id).collect();
        for id in ids {
            self.pump(id, limit);
        }
    }

    // ------------------------------------------------------------------------------------------ housekeeping and the end
    /// Things that happen with time: seats are frozen, spectators are sent more log, the game may be over.
    fn housekeeping(&mut self) -> Option<String> {
        let now = Instant::now();
        self.freeze_check(now);
        self.pump_all(PUMP_QUEUE);
        if self.cfg.max_ticks.is_some_and(|m| self.released >= m) {
            return Some("the game reached its tick limit".to_string());
        }
        if now.duration_since(self.last_activity) > self.cfg.timeout {
            return Some("a player stopped sending (timeout)".to_string());
        }
        None
    }

    /// End the game: everybody who is still catching up is sent the rest of the log, everybody is told the tick the game ended on (every
    /// copy ends on it, rolled back if it had gone further), and the last hashes are collected.
    fn finish(&mut self, reason: String) -> HostReport {
        let t = match self.cfg.max_ticks {
            Some(m) if self.released >= m => m,
            _ => self.released,
        };
        // a player who was about to take its seat back does not any more
        for c in self.conns.values_mut() {
            if let Stage::Resuming { cursor, sent_upto } = c.stage {
                c.stage = Stage::Following { cursor, sent_upto };
            }
        }
        self.pump_all(usize::MAX);
        for c in self.conns.values_mut() {
            let _ = c.link.send(&bye_at(reason.clone(), t));
            c.link.close();
        }
        // the last hashes may still be on their way: players that are still connected get a moment to report them
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline && self.conns.values().any(|c| c.kind == Kind::Player) {
            match self.rx.recv_timeout(Duration::from_millis(20)) {
                Ok(Ev::Msg(id, m)) => {
                    if let Some(h) = m.get("hash") {
                        self.on_hash_msg(id, h);
                    }
                }
                Ok(Ev::Gone(id)) => {
                    self.conns.remove(&id);
                }
                Ok(Ev::Hello { stream, .. }) => refuse(stream, "the game is over"),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        // the writer threads need a moment to put the last messages on the wire: a process that exits now would cut them off
        let drain = Instant::now() + Duration::from_secs(2);
        while Instant::now() < drain && self.conns.values().any(|c| c.link.queued() > 0) {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.report.ticks = t;
        self.report.ended = reason;
        self.report.log = InputLog { entries: self.log.iter().filter(|(e, _)| e.tick < t).map(|(e, _)| e.clone()).collect(), complete: self.log_first == 0 };
        std::mem::take(&mut self.report)
    }
}

#[cfg(test)]
mod tests {
    use super::told_to_followers;

    #[test]
    fn a_follower_is_never_told_that_a_tick_after_the_end_of_the_game_is_complete() {
        // the frontier stood at 602 when a game limited to 600 ticks was over
        assert_eq!(told_to_followers(602, Some(600)), 600);
        assert_eq!(told_to_followers(600, Some(600)), 600);
        assert_eq!(told_to_followers(150, Some(600)), 150, "while the game is on, everything the frontier says");
        assert_eq!(told_to_followers(9_999, None), 9_999, "a game with no limit has no end to hold back from");
    }
}
