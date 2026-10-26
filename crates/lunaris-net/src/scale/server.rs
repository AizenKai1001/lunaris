//! The server of a big match: it owns the world's entities, takes the state each client reports for its own character (after checking it is possible), takes the commands a client sends
//! (reliably: a shot, a pickup), lets the game act on them, and tells each client about the entities that matter to it, at the rate they matter ([`super::interest`]).
//!
//! It is authoritative about everything but where a client's own character is (that is the client's to say, within what the character can do: a movement budget, a vertical limit;
//! an impossible move is refused and the client is told to go back), so a game's movement code can run as it is on the client, with no copy of it here, and a hundred characters
//! cost the server a few table lookups each. Shots, damage, loot and the zone are decided here, from where the characters were when the shooter pulled the trigger
//! (a second of every character's places is kept: [`World::hitscan`]).
//!
//! The server does not read a clock: [`NetServer::update`] is given the time in milliseconds (a real one, or a test's own), and runs the ticks that are due.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use super::bytes::{R, W};
use super::interest::{is_static, Accounts, Candidate, Grid, Rates, Statics};
use super::occlusion::{Blocker, Counters, Eyes, Governor, Occlusion, Params, SharedEyes, Sightings};
use super::proto::{kind, msg, rto, Acks, ReliableIn, ReliableOut, HEADER, MAGIC, PROTOCOL};
use super::secure::{self, Channel};
use super::state::{self, class, Detail, EntityState};
use super::transport::{Addr, Transport, MAX_DATAGRAM};

/// The kinds of reliable message the library itself sends to a client (the game's are below 0xF0).
pub mod ev {
    /// "Your move was impossible: go back to this place" (three zigzag varints, the position in 1/64 m).
    pub const CORRECT: u8 = 0xF0;
}

#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// How many players at most.
    pub max_clients: usize,
    /// Ticks a second (each tick every client is sent a packet).
    pub tick_hz: u32,
    /// The most bytes of entity records in one packet.
    pub snapshot_budget: usize,
    /// A client is told about entities within this many metres of its character.
    pub interest: f32,
    pub rates: Rates,
    /// The size of the cells of the grid (metres).
    pub cell: f32,
    /// A client that has said nothing for this long is dropped.
    pub timeout_ms: u64,
    /// What a character may do: metres a second on the ground, up, and down (falling).
    pub max_speed: f32,
    pub max_up: f32,
    pub max_down: f32,
    /// A character may have saved up this many seconds of movement (a late packet brings a lot at once).
    pub burst_s: f32,
    /// A client with nobody near it (nothing is sent faster than the mid rate) is made a packet only once in this many ticks (3: ten a second at 30 ticks), which cuts the packets,
    /// and the work of making them, to a third for most of a big map's players; a reliable message is never held back. 1 makes a packet every tick for everybody.
    pub idle_every: u32,
    /// Measure where the time of a tick goes ([`Stats::phase_ns`]; a clock read between the steps of each client's packet, so it is off by default).
    pub profile: bool,
    /// Who may join (docs/SECURE.md): with this set only a player with a connect token for this server gets in, and everything the player and the server send each other is sealed (authenticated and encrypted).
    pub auth: Option<Auth>,
    /// How the wallhack defence works when the game turns it on ([`Game::occlusion`]); docs/NETWORK_SCALE.md, "Wallhacks".
    pub occlusion: Occlusion,
    /// How many threads make the packets of the clients (each client's packet is independent of the others': the entities near it, which of them it can see, what is due, the bytes): 1 makes them one after the other
    /// on the thread that runs the tick; 0 (the default) uses the machine's processors up to 8; with fewer than 24 clients it is always one thread. A game whose [`Game::blocker`] is `None` and that has the wallhack
    /// defence on is made packets for one after the other whatever this says (its level of sight cannot be shared).
    pub threads: usize,
    /// How many players one IP address may have in at once (0: no limit, the default: a swarm of test bots comes from one address). A server on the internet sets a few (the ops kit uses 6): one machine with a
    /// thousand sockets would otherwise fill every slot. The refused one is told `REFUSED_ADDRESS`.
    pub max_per_ip: usize,
}

impl ServerConfig {
    /// The number of threads that make packets, when there are `clients` clients.
    pub fn packet_threads(&self, clients: usize) -> usize {
        if clients < PARALLEL_FROM {
            return 1;
        }
        let want = if self.threads == 0 { std::thread::available_parallelism().map_or(1, |n| n.get()).min(8) } else { self.threads };
        want.clamp(1, clients / 8 + 1)
    }
}

/// The server's half of the connect tokens: the key its tokens are sealed under (shared with whoever makes them), the number that says which game they are for, and the clock to judge their expiry by.
#[derive(Clone)]
pub struct Auth {
    pub key: [u8; 32],
    pub protocol_id: u64,
    /// Seconds since 1970 to judge expiry by (`None`: the computer's clock; a test fixes it).
    pub unix_now: Option<u64>,
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Auth {{ key: <hidden>, protocol_id: {} }}", self.protocol_id)
    }
}

/// The steps of a tick that [`Stats::phase_ns`] has a place for.
pub mod phase {
    /// Reading the datagrams that arrived and acting on them.
    pub const RECV: usize = 0;
    /// The game's tick, the events it sent, the sweep for silent clients.
    pub const GAME: usize = 1;
    /// Copying the entities into the flat list and the grids.
    pub const GRIDS: usize = 2;
    /// Finding the candidates of a client (the grid queries).
    pub const GATHER: usize = 3;
    /// The ranks, the accounts, the order of what is due.
    pub const RANK: usize = 4;
    /// Writing the packet.
    pub const WRITE: usize = 5;
    /// Handing it to the network.
    pub const SEND: usize = 6;
    /// Deciding which of the candidates are out of sight (only with [`super::Game::occlusion`] on; it is not part of `GATHER`).
    pub const OCCLUDE: usize = 7;
    pub const NAMES: [&str; 8] = ["recv", "game", "grids", "gather", "rank", "write", "send", "occlude"];
}

impl Default for ServerConfig {
    fn default() -> ServerConfig {
        ServerConfig { max_clients: 128, tick_hz: 30, snapshot_budget: 900, interest: 150.0, rates: Rates::default(), cell: 32.0, timeout_ms: 5000, max_speed: 16.0, max_up: 14.0, max_down: 90.0, burst_s: 0.6, idle_every: 3, profile: false, auth: None, occlusion: Occlusion::default(), threads: 0, max_per_ip: 0 }
    }
}

pub struct Entity {
    pub state: EntityState,
    /// The slot of the client whose character this is.
    pub owner: Option<usize>,
    /// The game puts this character (a player in a vehicle): what the client reports of its place is not believed (and not corrected), the rest of what it reports is.
    pub locked: bool,
    // (tick, position) for the last second, to ask where a character was when a shot was fired
    history: VecDeque<(u32, [i32; 3])>,
}

pub struct World {
    pub entities: HashMap<u32, Entity>,
    next_id: u32,
    pub tick: u32,
    /// The smoothed round trip time of each client in milliseconds, by slot (0 for an empty slot or a client that has not been answered yet), brought up to date every tick before the game's `on_tick`: what lag compensation looks back by.
    pub rtt_ms: Vec<f32>,
}

impl Default for World {
    fn default() -> World {
        World { entities: HashMap::new(), next_id: 1, tick: 0, rtt_ms: Vec::new() }
    }
}

impl World {
    /// Adds an entity of `class` at a place; returns its id.
    pub fn spawn(&mut self, class_id: u8, pos_m: [f32; 3]) -> u32 {
        let mut s = EntityState { class: class_id, health: 100, ..Default::default() };
        s.set_pos_m(pos_m);
        self.spawn_state(s)
    }

    pub fn spawn_state(&mut self, mut s: EntityState) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        s.id = id;
        self.entities.insert(id, Entity { state: s, owner: None, locked: false, history: VecDeque::new() });
        id
    }

    /// Notes where every character is at `tick` (the server does this every tick; a game that moves characters itself may too).
    pub fn record(&mut self, tick: u32, keep: usize) {
        for e in self.entities.values_mut() {
            if e.owner.is_some() {
                e.history.push_back((tick, e.state.pos));
                while e.history.len() > keep {
                    e.history.pop_front();
                }
            }
        }
    }

    pub fn remove(&mut self, id: u32) -> bool {
        self.entities.remove(&id).is_some()
    }

    pub fn get(&self, id: u32) -> Option<&Entity> {
        self.entities.get(&id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Entity> {
        self.entities.get_mut(&id)
    }

    /// Where the character `id` was at `tick` (the nearest tick the history has; the present if the tick has not come).
    pub fn pos_at(&self, id: u32, tick: u32) -> Option<[f32; 3]> {
        let e = self.entities.get(&id)?;
        let mut best = None;
        for &(t, p) in e.history.iter() {
            if t <= tick {
                best = Some(p);
            } else {
                break;
            }
        }
        let p = best.or_else(|| e.history.front().map(|h| h.1)).unwrap_or(e.state.pos);
        let q = |v: i32| v as f32 / state::POS_SCALE;
        Some([q(p[0]), q(p[1]), q(p[2])])
    }

    /// A shot: the first character (other than `shooter`) that the ray from `origin` along `dir` (a unit vector) hits within `max_range` metres, at the places the characters had at `at_tick`
    /// (the shooter saw them in the past: the time it takes the world to get to it). A character is a vertical cylinder of 0.45 m radius and 1.8 m height standing on its position.
    /// `blocked` is the distance at which the ray hits the level (a wall), if the game has worked that out: a hit beyond it does not count. Returns the id and the distance.
    pub fn hitscan(&self, shooter: u32, origin: [f32; 3], dir: [f32; 3], at_tick: u32, max_range: f32, blocked: Option<f32>) -> Option<(u32, f32)> {
        let limit = blocked.map_or(max_range, |b| b.min(max_range));
        let mut best: Option<(u32, f32)> = None;
        for (&id, e) in self.entities.iter() {
            if id == shooter || e.owner.is_none() || e.state.health == 0 {
                continue;
            }
            let Some(c) = self.pos_at(id, at_tick) else { continue };
            if let Some(t) = ray_cylinder(origin, dir, c, 0.45, 0.0, 1.8) {
                if t <= limit && best.is_none_or(|b| t < b.1) {
                    best = Some((id, t));
                }
            }
        }
        best
    }
}

/// The distance along the ray (origin `o`, unit direction `d`) at which it enters the vertical cylinder standing on `c` (radius `r`, from `y0` to `y1` above it), or `None`.
fn ray_cylinder(o: [f32; 3], d: [f32; 3], c: [f32; 3], r: f32, y0: f32, y1: f32) -> Option<f32> {
    let (ox, oz) = (o[0] - c[0], o[2] - c[2]);
    let a = d[0] * d[0] + d[2] * d[2];
    let (mut t0, mut t1) = (0.0f32, f32::MAX);
    if a < 1.0e-8 {
        if ox * ox + oz * oz > r * r {
            return None;
        }
    } else {
        let b = 2.0 * (ox * d[0] + oz * d[2]);
        let cc = ox * ox + oz * oz - r * r;
        let disc = b * b - 4.0 * a * cc;
        if disc < 0.0 {
            return None;
        }
        let s = disc.sqrt();
        t0 = t0.max((-b - s) / (2.0 * a));
        t1 = t1.min((-b + s) / (2.0 * a));
    }
    let (lo, hi) = (c[1] + y0, c[1] + y1);
    if d[1].abs() < 1.0e-8 {
        if o[1] < lo || o[1] > hi {
            return None;
        }
    } else {
        let (ta, tb) = ((lo - o[1]) / d[1], (hi - o[1]) / d[1]);
        t0 = t0.max(ta.min(tb));
        t1 = t1.min(ta.max(tb));
    }
    if t0 <= t1 {
        Some(t0)
    } else {
        None
    }
}

/// Why a server refuses or removes a player (the code of the `REJECT` datagram; the first three are the handshake's: 1 another version, 2 full, 3 the name is in).
pub const REFUSED_BANNED: u8 = 4;
pub const REFUSED_KICKED: u8 = 5;
/// One address already has as many players in as the server allows (`ServerConfig::max_per_ip`).
pub const REFUSED_ADDRESS: u8 = 7;

/// The bans of a server (docs/ADMIN.md): names (kept in lower case: a name on a plain server is only what the client says, so the address counts too) and addresses (the IP, any port). A ban may run out:
/// `timed_names` and `timed_ips` hold the unix second each ends at (the clock of the machine, so that a ban goes on running while the server is stopped).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bans {
    pub names: std::collections::BTreeSet<String>,
    pub ips: std::collections::BTreeSet<std::net::IpAddr>,
    pub timed_names: std::collections::BTreeMap<String, u64>,
    pub timed_ips: std::collections::BTreeMap<std::net::IpAddr, u64>,
}

/// The unix second now (0 if the clock is before 1970).
pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Bans {
    /// Is this name, or this address, banned?
    pub fn contains(&self, name: &str, ip: std::net::IpAddr) -> bool {
        self.contains_at(name, ip, unix_now())
    }

    /// The same, at a given unix second (a timed ban holds while its end is later).
    pub fn contains_at(&self, name: &str, ip: std::net::IpAddr, now: u64) -> bool {
        let name = name.to_lowercase();
        self.ips.contains(&ip) || self.names.contains(&name) || self.timed_ips.get(&ip).is_some_and(|t| *t > now) || self.timed_names.get(&name).is_some_and(|t| *t > now)
    }

    /// How many bans there are (the permanent ones and the timed ones that have not run out).
    pub fn count(&self, now: u64) -> usize {
        self.names.len() + self.ips.len() + self.timed_names.values().filter(|t| **t > now).count() + self.timed_ips.values().filter(|t| **t > now).count()
    }

    /// The text of a ban file: a line `name NAME` or `ip ADDRESS` for each, with `until UNIXSECOND` after it for one that runs out; what is not understood is skipped.
    pub fn parse(text: &str) -> Bans {
        let mut b = Bans::default();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let (line, until) = match line.rsplit_once(" until ") {
                Some((l, t)) => match t.trim().parse::<u64>() {
                    Ok(t) => (l.trim(), Some(t)),
                    Err(_) => continue,
                },
                None => (line, None),
            };
            match line.split_once(char::is_whitespace).map(|(k, v)| (k, v.trim())) {
                Some(("name", n)) if !n.is_empty() => match until {
                    Some(t) => {
                        b.timed_names.insert(n.to_lowercase(), t);
                    }
                    None => {
                        b.names.insert(n.to_lowercase());
                    }
                },
                Some(("ip", a)) => {
                    if let Ok(ip) = a.parse() {
                        match until {
                            Some(t) => {
                                b.timed_ips.insert(ip, t);
                            }
                            None => {
                                b.ips.insert(ip);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        b
    }

    /// The text of the ban file: what has run out is left out.
    pub fn to_text(&self) -> String {
        let now = unix_now();
        let mut out = String::from("# bans of this server: `name NAME` or `ip ADDRESS` a line, `until UNIXSECOND` after it for a ban that runs out (docs/ADMIN.md)
");
        for n in &self.names {
            out.push_str(&format!("name {n}
"));
        }
        for i in &self.ips {
            out.push_str(&format!("ip {i}
"));
        }
        for (n, t) in self.timed_names.iter().filter(|(_, t)| **t > now) {
            out.push_str(&format!("name {n} until {t}
"));
        }
        for (i, t) in self.timed_ips.iter().filter(|(_, t)| **t > now) {
            out.push_str(&format!("ip {i} until {t}
"));
        }
        out
    }
}

/// A player that is in, as the admin sees it.
#[derive(Clone, Debug)]
pub struct PlayerRow {
    pub slot: usize,
    pub name: String,
    pub addr: Addr,
    pub rtt_ms: f64,
}

/// What a game says to the clients: reliable messages (the first byte of each is its kind, below 0xF0).
#[derive(Default)]
pub struct Outbox {
    pub to: Vec<(usize, Vec<u8>)>,
    pub all: Vec<Vec<u8>>,
    pub near: Vec<([f32; 3], f32, Vec<u8>)>,
}

impl Outbox {
    /// To one client (by its slot).
    pub fn to(&mut self, slot: usize, event: Vec<u8>) {
        self.to.push((slot, event));
    }

    /// To everybody.
    pub fn all(&mut self, event: Vec<u8>) {
        self.all.push(event);
    }

    /// To the clients whose character is within `radius` metres of a place.
    pub fn near(&mut self, pos: [f32; 3], radius: f32, event: Vec<u8>) {
        self.near.push((pos, radius, event));
    }
}

/// What the last change of a game's level touched ([`Game::level_change`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelChange {
    /// The [`Game::level_version`] the level had before the change.
    pub from: u32,
    /// Where it changed: a box (low corner, high corner) round each thing that was added, taken away or moved by the change, where it was and where it is. Empty: everywhere, or it is not known.
    pub regions: Vec<([f32; 3], [f32; 3])>,
}

/// What a game can reach when the server calls it.
pub struct Ctx<'a> {
    pub world: &'a mut World,
    pub out: &'a mut Outbox,
    pub tick: u32,
    pub tick_hz: u32,
}

/// The rules of the match. The server does the network; the game decides what a join, a command and a tick do.
pub trait Game {
    /// A client has connected: the state of its new character (the id is the server's to give).
    fn on_join(&mut self, world: &mut World, slot: usize, name: &str) -> EntityState;
    /// A client has gone (its character is removed afterwards).
    fn on_leave(&mut self, _world: &mut World, _slot: usize) {}
    /// A reliable message from a client: the first byte is the kind (below 0xF0), the rest is the game's.
    fn on_command(&mut self, _ctx: &mut Ctx, _slot: usize, _data: &[u8]) {}
    /// Every tick, before the clients are sent what they need.
    fn on_tick(&mut self, _ctx: &mut Ctx) {}
    /// An admin command the server does not know (docs/ADMIN.md): the game's own (`say`, `mute`, `save`, ...). `Some(answer)` when it was the game's, `None` when not.
    fn admin(&mut self, _ctx: &mut Ctx, _cmd: &str, _args: &str) -> Option<String> {
        None
    }
    /// Does the game want the server to stop now? Asked after every admin command: a command such as `wipe` ends in a stop (the world is saved the game's own way on the way out, and a supervisor starts the server again).
    fn wants_stop(&self) -> bool {
        false
    }

    /// **Wallhack defence** (docs/NETWORK_SCALE.md, "Wallhacks"): the safe radius in metres, and the switch. 0 (the default) tells every client about every player within the interest radius, through any
    /// wall; above 0 a player farther than this from a client is left out of the client's packets while [`Game::blocked`] says the level is in the way (and for a short while after it was last in sight).
    /// Asked every tick, so a game may change it.
    fn occlusion(&self) -> f32 {
        0.0
    }
    /// Whether entities of this class are hidden behind walls (when [`Game::occlusion`] is on): by default the players only. Loot, the zone and things in the world are always told.
    fn occludes(&self, class_id: u8) -> bool {
        class_id == class::PLAYER
    }
    /// Is the level (what does not move) in the way of the straight line from `from` (a place at an observer's eye) to `to` (a point on another's body)? Asked for a few rays for each pair of
    /// players that is not too near, at most [`Occlusion::rays_per_client`] a tick for each client, so it should be quick. The default is no: nothing hides anything.
    fn blocked(&self, _from: [f32; 3], _to: [f32; 3]) -> bool {
        false
    }
    /// The same answers as [`Game::blocked`], as something that can be asked from several threads at once ([`ServerConfig::threads`]): with it the server makes the packets of its clients on several threads. `None` (the
    /// default): [`Game::blocked`] is asked, from the one thread that runs the tick, so the packets are made one after the other. [`Game::occludes`] is asked for each class once a tick, whatever this is.
    fn blocker(&self) -> Option<&dyn Blocker> {
        None
    }
    /// A number that changes when the level changes in a way that matters to [`Game::blocked`] (a wall built or taken down, a door shut): what was found out about the lines of sight is asked again.
    fn level_version(&self) -> u32 {
        0
    }
    /// What the last change of the level touched: the version it had before and a box round each thing that changed. A pair of players whose answer was found at that version, and whose lines of sight (from where the observer's eye is
    /// or is about to be to where the other's body is or is about to be) do not go through any of the boxes, keeps its answer; any other is asked again. The default is "everywhere, or not known": every answer is asked again, as it was.
    fn level_change(&self) -> LevelChange {
        LevelChange::default()
    }
    /// The bits of a player's `flags` that mean it is low (crouched or prone): its head and chest are lower in the tests. 0: nobody is.
    fn low_flags(&self) -> u16 {
        0
    }
    /// Players that are on one side (the members of a party, docs/PARTIES.md): a number that changes whenever the list does, and the list, as (the server's number of a player, its group), sorted. With [`Game::occlusion`] on, the
    /// players of one group are always told about each other (within the interest radius), whatever is in the way: friends are not hidden from each other. The server looks at the list again only when the number changes;
    /// the default is nobody.
    fn allies(&self) -> (u32, &[(u32, u32)]) {
        (0, &[])
    }
    /// What the game spent its last [`Game::on_tick`] on, as (name, microseconds) pairs, for the profile of the slowest tick (`ServerConfig::profile`). Nothing by default.
    fn tick_parts(&self) -> Vec<(&'static str, u64)> {
        Vec::new()
    }
}

struct Pending {
    client_salt: u32,
    server_salt: u32,
    name: String,
    at: u64,
    // (with a connect token: the channel the token makes, who the player is, and which token)
    chan: Option<Channel>,
    client_id: u64,
    token_nonce: [u8; 24],
}

/// The most messages (movement records, reliable commands) a client may send in a second, as a burst and on average: ten times what the game needs (a record a tick and a few commands); the rest of a flood is dropped.
const MSG_RATE: f32 = 300.0;

/// The most reliable messages the server keeps for one client that has not acknowledged them: a client that sends packets (so it is not timed out) and never says which arrived would make the server
/// keep, and look through, every broadcast for ever. Past this it is removed. A client that acknowledges its packets is a few messages behind at most, even over a bad link.
pub const MAX_BACKLOG: usize = 1024;

/// How many handshakes may be half open at once, and how many from one IP address (a flood of CONNECTs from a spoofed address costs the sender nothing; ports of one address cost nothing either): a full table
/// drops its oldest eighth for the newcomer, so a real player gets in while it is under attack, and one address cannot hold more than 128 places in it (a swarm of bots on one machine still joins in seconds).
const MAX_PENDING: usize = 4096;
const MAX_PENDING_PER_IP: u16 = 128;

/// What a player's name is once the handshake has cleaned it: no control characters, no `#` (which starts a comment in the admin's files), none of the marks that change the direction of text or have no width
/// (they would reorder or hide the letters round them in the chat), white space made single spaces and none at the ends, at most 32 bytes. A name with nothing left in it becomes `player` and four digits from
/// `seed`. What reaches the admin console, the ban file, the save file and the chat is a name that cannot start a second line there.
pub fn clean_name(raw: &str, seed: u32) -> String {
    let mut out = String::new();
    let mut space = true;
    for c in raw.chars() {
        if c == '\u{FFFD}' || c == '#' || is_text_mark(c) || (c.is_control() && !c.is_whitespace()) {
            continue;
        }
        if c.is_whitespace() {
            if !space {
                out.push(' ');
                space = true;
            }
            continue;
        }
        if out.len() + c.len_utf8() > 32 {
            break;
        }
        out.push(c);
        space = false;
    }
    let out = out.trim_end().to_string();
    if out.is_empty() {
        format!("player{:04}", seed % 10_000)
    } else {
        out
    }
}

/// The characters that change how the text round them is laid out, or show nothing: the zero-width ones, the direction marks and overrides, the word joiner and the byte order mark.
pub fn is_text_mark(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

struct Client {
    addr: Addr,
    conn: u16,
    name: String,
    entity: u32,
    acks: Acks,
    rel_out: ReliableOut,
    rel_in: ReliableIn,
    last_recv: u64,
    last_sent: u64,
    accounts: Accounts,
    budget: f32,
    /// What the character may still climb and fall (metres), a pool like `budget`: a flat allowance for each STATE record would let many records of one datagram stack up.
    budget_up: f32,
    budget_down: f32,
    /// Messages this client may still send (a pool refilled over time): one client cannot force unbounded work into a tick.
    msg_tokens: f32,
    last_state_ms: u64,
    last_correct_ms: u64,
    /// What this client has been found to see and not see (only used with [`Game::occlusion`] on).
    seen: Sightings,
    /// The players the game says are on this client's side ([`Game::allies`]): never hidden from it.
    mates: Vec<u32>,
    /// The rays this client's packets may still cast, and the tick they were last topped up ([`Occlusion::rays_per_client`]).
    occ_rays: u32,
    occ_tick: u32,
    // the tick this client's packet was last made on, and whether anything near it needed a record that tick (then it gets a packet every tick; else one every `idle_every` ticks)
    last_eval: u32,
    fast: bool,
    chan: Option<Channel>,
    client_id: u64,
}

/// What the server has done, for a report.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub ticks: u64,
    pub packets_in: u64,
    pub packets_out: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub records_out: u64,
    pub corrections: u64,
    pub joins: u64,
    pub leaves: u64,
    pub refused: u64,
    /// Messages of a client that were dropped because it sent more than `MSG_RATE` a second.
    pub throttled: u64,
    /// Sealed packets and tokens that were refused: forged, changed, replayed, expired, for another game or key, or from a place that is not the player's.
    pub bad_packets: u64,
    /// Clients that were removed because they kept more than [`MAX_BACKLOG`] reliable messages unacknowledged (they send packets and never say which arrived).
    pub backlog_kicks: u64,
    /// The time the ticks took (microseconds): in all, and the longest.
    pub tick_us_total: u64,
    pub tick_us_max: u64,
    /// Nanoseconds spent in each step of the tick, by [`phase`] (only counted when [`ServerConfig::profile`] is on).
    pub phase_ns: [u64; 8],
    /// Wallhack defence ([`Game::occlusion`]), summed over the ticks: pairs of players whose lines of sight were tested, rays cast for them, pairs that kept their answer because neither had moved,
    /// players left out of a packet because they were out of sight, and tests the client had no rays for ([`Occlusion::rays_per_client`]).
    pub occ_pairs: u64,
    pub occ_rays: u64,
    pub occ_still: u64,
    pub occ_hidden: u64,
    pub occ_unaffordable: u64,
    /// The packets that were made with the rays of their client cut by the governor ([`Occlusion::governor_ms`]).
    pub occ_throttled: u64,
}

/// The slowest tick since the report was last taken (only kept when [`ServerConfig::profile`] is on): how long it took by the clock and by the processor time of the server's thread (a tick that
/// is much longer by the clock than by the processor was not running: the machine was busy with something else), the nanoseconds of each [`phase`] in it, and what the game says it did.
#[derive(Clone, Debug, Default)]
pub struct Worst {
    pub tick: u32,
    pub us: u64,
    pub cpu_us: u64,
    pub phase_ns: [u64; 8],
    pub parts: Vec<(&'static str, u64)>,
    pub players: usize,
    /// What the two canaries took at the start of this tick (nanoseconds): see [`canary`].
    pub canary_ns: [u64; 2],
    /// What the wallhack defence did in this tick: pairs tested, rays cast, pairs that kept their answer, players left out, tests with no rays; and whether the game said its level had changed (every answer is asked again).
    pub occ: [u64; 5],
    pub level_changed: bool,
}

/// A fixed piece of work, timed: a chain of integer operations (it runs at the speed of the core's clock, and slower when something else is using the same core) and a walk over a megabyte (it runs at the speed
/// of the caches and the memory, and slower when other programs are using them). With [`ServerConfig::profile`] the server runs it at the start of every tick: when the slowest tick of a report is slow in every phase
/// at once and the canaries are as slow, it was the machine and not the server's code.
pub fn canary(mem: &mut [u64]) -> [u64; 2] {
    let t = Instant::now();
    let mut x = 0x9E37_79B9u64;
    for i in 0..30_000u64 {
        x = (x.rotate_left(5) ^ i).wrapping_mul(0x9E37_79B1_85EB_CA87);
    }
    let alu = t.elapsed().as_nanos() as u64;
    let t = Instant::now();
    let mut sum = x;
    for (i, w) in mem.iter_mut().enumerate() {
        sum = sum.wrapping_add(*w ^ i as u64);
        *w = sum;
    }
    std::hint::black_box(sum);
    [alu, t.elapsed().as_nanos() as u64]
}

/// The processor time this thread has used (microseconds); 0 where it cannot be told.
#[cfg(target_os = "linux")]
pub fn thread_cpu_us() -> u64 {
    #[repr(C)]
    struct Timespec {
        sec: i64,
        nsec: i64,
    }
    extern "C" {
        fn clock_gettime(clock: i32, ts: *mut Timespec) -> i32;
    }
    // CLOCK_THREAD_CPUTIME_ID
    let mut ts = Timespec { sec: 0, nsec: 0 };
    if unsafe { clock_gettime(3, &mut ts) } == 0 {
        ts.sec as u64 * 1_000_000 + ts.nsec as u64 / 1000
    } else {
        0
    }
}

#[cfg(not(target_os = "linux"))]
pub fn thread_cpu_us() -> u64 {
    0
}

pub struct NetServer<T: Transport, G: Game> {
    pub cfg: ServerConfig,
    pub transport: T,
    pub game: G,
    pub world: World,
    /// The bans (docs/ADMIN.md): a banned name or address is refused at the end of the handshake.
    pub bans: Bans,
    clients: Vec<Option<Client>>,
    by_addr: HashMap<Addr, usize>,
    by_conn: HashMap<u16, usize>,
    pending: HashMap<Addr, Pending>,
    /// How many half-open handshakes each IP address has in `pending` (kept with it).
    pending_ips: HashMap<std::net::IpAddr, u16>,
    /// The player ids of the connect tokens of the clients that are in, and the tokens that have been used (until they expire): a token is for one connection.
    by_client: HashMap<u64, usize>,
    used_tokens: HashMap<[u8; 24], (u64, Addr)>,
    next_conn: u16,
    rng: u64,
    // what the clients are told from, rebuilt every tick: every entity's state in one flat list (the grids hold places in it), the ones that move in `grid` and the ones that do not in `statics`
    flat: Vec<EntityState>,
    grid: Grid,
    statics: Statics,
    // (the lists a packet is made from, kept between packets so that none is allocated again: one set for each thread that makes packets)
    scratch: Vec<Scratch>,
    // the wallhack defence of this tick (the safe radius is 0 when it is off), and the rays it may still cast
    occ: Params,
    now: u64,
    next_tick: f64,
    last_sweep: u64,
    pub stats: Stats,
    /// With [`ServerConfig::profile`]: the clock time and the processor time (microseconds) of every tick since the report was last taken, and the slowest of them (`take_tick_log`, `take_worst`).
    pub tick_log: Vec<(u32, u32)>,
    pub worst: Worst,
    /// How much of its rays each client may spend (1 unless the ticks are taking too long: [`Governor`]).
    governor: Governor,
    /// The canaries of every tick since the report was last taken, and the memory they walk over.
    pub canary_log: Vec<[u32; 2]>,
    canary_mem: Vec<u64>,
    /// The level version of the last tick (to notice that it changed).
    last_epoch: u32,
    /// The version of [`Game::allies`] the clients' `mates` were made from.
    allies_seen: u32,
}

impl<T: Transport, G: Game> NetServer<T, G> {
    pub fn new(cfg: ServerConfig, transport: T, game: G) -> NetServer<T, G> {
        let grid = Grid::new(cfg.cell);
        let statics = Statics::new(cfg.cell, cfg.rates.static_period(cfg.tick_hz));
        let clients = (0..cfg.max_clients).map(|_| None).collect();
        NetServer {
            cfg,
            transport,
            game,
            world: World::default(),
            bans: Bans::default(),
            clients,
            by_addr: HashMap::new(),
            by_conn: HashMap::new(),
            pending: HashMap::new(),
            pending_ips: HashMap::new(),
            by_client: HashMap::new(),
            used_tokens: HashMap::new(),
            next_conn: 1,
            rng: 0x9E37_79B9_7F4A_7C15,
            flat: Vec::new(),
            grid,
            statics,
            scratch: Vec::new(),
            occ: Params::default(),
            now: 0,
            next_tick: 0.0,
            last_sweep: 0,
            stats: Stats::default(),
            tick_log: Vec::new(),
            worst: Worst::default(),
            governor: Governor::default(),
            canary_log: Vec::new(),
            canary_mem: Vec::new(),
            last_epoch: 0,
            allies_seen: 0,
        }
    }

    pub fn client_count(&self) -> usize {
        self.clients.iter().filter(|c| c.is_some()).count()
    }

    /// How many handshakes are half open (a CONNECT answered with a challenge, no answer yet), and how many reliable messages a client's slot keeps for resending: what the soak and the fuzz tests watch for growth.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn reliable_backlog(&self, slot: usize) -> usize {
        self.clients.get(slot).and_then(|c| c.as_ref()).map_or(0, |c| c.rel_out.len())
    }

    /// The character of the client in a slot.
    pub fn entity_of(&self, slot: usize) -> Option<u32> {
        self.clients.get(slot)?.as_ref().map(|c| c.entity)
    }

    pub fn name_of(&self, slot: usize) -> Option<&str> {
        self.clients.get(slot)?.as_ref().map(|c| c.name.as_str())
    }

    /// The players that are in.
    pub fn players(&self) -> Vec<PlayerRow> {
        self.clients.iter().enumerate().filter_map(|(slot, c)| c.as_ref().map(|c| PlayerRow { slot, name: c.name.clone(), addr: c.addr, rtt_ms: c.acks.srtt })).collect()
    }

    /// The slot of the player called `name` (case does not matter).
    pub fn slot_of_name(&self, name: &str) -> Option<usize> {
        self.clients.iter().position(|c| c.as_ref().is_some_and(|c| c.name.eq_ignore_ascii_case(name)))
    }

    /// The player whose name `args` begins with (the longest such name, case does not matter, followed by the end of `args` or white space) and what is left of `args` after it: a name may have spaces in it, so the admin's
    /// `kick Big Bob` and `ban Big Bob 2h` find "Big Bob" (and not "Big" or "Bob").
    pub fn player_in(&self, args: &str) -> Option<(usize, String)> {
        let args = args.trim();
        let mut best: Option<(usize, usize)> = None;
        for (slot, c) in self.clients.iter().enumerate() {
            let Some(c) = c else { continue };
            let n = c.name.len();
            let fits = args.len() >= n && args.is_char_boundary(n) && args[..n].eq_ignore_ascii_case(&c.name) && args[n..].chars().next().is_none_or(char::is_whitespace);
            if fits && best.is_none_or(|(_, len)| n > len) {
                best = Some((slot, n));
            }
        }
        best.map(|(slot, n)| (slot, args[n..].trim().to_string()))
    }

    /// Remove a player now and tell its client why (`REFUSED_KICKED`, `REFUSED_BANNED`: the client shows a message instead of waiting for the silence to time out). False when nobody is in that slot.
    pub fn kick(&mut self, slot: usize, why: u8) -> bool {
        let Some(addr) = self.clients.get(slot).and_then(|c| c.as_ref()).map(|c| c.addr) else { return false };
        let mut w = W::new();
        w.u8(MAGIC);
        w.u8(kind::REJECT);
        w.u8(why);
        self.reply(addr, &w);
        self.drop_client(slot);
        true
    }

    /// Offer an admin command to the game (it may send messages to the clients); `None` when the game does not know it.
    pub fn admin_game(&mut self, cmd: &str, args: &str) -> Option<String> {
        let mut out = Outbox::default();
        let tick = self.world.tick;
        let hz = self.cfg.tick_hz;
        let answer = {
            let mut ctx = Ctx { world: &mut self.world, out: &mut out, tick, tick_hz: hz };
            self.game.admin(&mut ctx, cmd, args)
        };
        self.flush(out);
        answer
    }

    /// A reliable message to one client, now.
    pub fn send_event(&mut self, slot: usize, event: Vec<u8>) {
        if let Some(Some(c)) = self.clients.get_mut(slot) {
            c.rel_out.push(event);
        }
    }

    fn rand(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }

    /// Reads what has arrived and runs the ticks that are due at time `now` (milliseconds).
    pub fn update(&mut self, now: u64) {
        self.now = now;
        if self.next_tick == 0.0 {
            self.next_tick = now as f64;
        }
        let t_recv = self.cfg.profile.then(Instant::now);
        while let Some((from, data)) = self.transport.recv() {
            self.stats.packets_in += 1;
            self.stats.bytes_in += data.len() as u64;
            self.handle_datagram(from, &data);
        }
        if let Some(t) = t_recv {
            self.stats.phase_ns[phase::RECV] += t.elapsed().as_nanos() as u64;
        }
        let step = 1000.0 / self.cfg.tick_hz as f64;
        let mut guard = 0;
        while now as f64 >= self.next_tick && guard < 4 {
            self.tick();
            self.next_tick += step;
            guard += 1;
        }
        if guard == 4 && now as f64 >= self.next_tick {
            // too far behind to catch up: the match goes on from now
            self.next_tick = now as f64 + step;
        }
    }

    fn handle_datagram(&mut self, from: Addr, d: &[u8]) {
        if d.len() < 2 || d[0] != MAGIC {
            return;
        }
        // (a server that wants connect tokens answers only to the sealed kinds, one that does not only to the plain ones)
        let secure = self.cfg.auth.is_some();
        match d[1] {
            kind::CONNECT_TOKEN if secure => self.on_connect_token(from, &d[2..]),
            kind::RESPONSE_SEALED if secure => self.on_response_sealed(from, &d[2..]),
            kind::DATA_SEALED if secure => self.on_data_sealed(from, d),
            kind::DISCONNECT_SEALED if secure => self.on_disconnect_sealed(from, d),
            kind::CONNECT if !secure => self.on_connect(from, &d[2..]),
            kind::RESPONSE if !secure => self.on_response(from, &d[2..]),
            kind::DATA if !secure => self.on_data(from, d),
            kind::DISCONNECT if !secure => {
                let mut r = R::new(&d[2..]);
                if let Some(conn) = r.u16() {
                    if let Some(&slot) = self.by_conn.get(&conn) {
                        if self.clients[slot].as_ref().is_some_and(|c| c.addr == from) {
                            self.drop_client(slot);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn reply(&mut self, to: Addr, w: &W) {
        self.stats.packets_out += 1;
        self.stats.bytes_out += w.len() as u64;
        self.transport.send(to, &w.buf);
    }

    /// Room in the table of half-open handshakes for one from `from` (an address that has none in it): no when that address already has [`MAX_PENDING_PER_IP`]; and a full table drops its oldest eighth first.
    fn make_room_for_handshake(&mut self, from: Addr) -> bool {
        if self.pending_ips.get(&from.ip()).is_some_and(|n| *n >= MAX_PENDING_PER_IP) {
            return false;
        }
        if self.pending.len() >= MAX_PENDING {
            let mut by_age: Vec<(u64, Addr)> = self.pending.iter().map(|(a, p)| (p.at, *a)).collect();
            by_age.sort_unstable_by_key(|x| x.0);
            for (_, a) in by_age.iter().take(MAX_PENDING / 8) {
                self.pending.remove(a);
            }
            self.count_pending_ips();
        }
        true
    }

    fn add_pending(&mut self, from: Addr, p: Pending) {
        if self.pending.insert(from, p).is_none() {
            *self.pending_ips.entry(from.ip()).or_insert(0) += 1;
        }
    }

    fn forget_pending_ip(&mut self, from: Addr) {
        if let Some(n) = self.pending_ips.get_mut(&from.ip()) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                self.pending_ips.remove(&from.ip());
            }
        }
    }

    fn count_pending_ips(&mut self) {
        self.pending_ips.clear();
        for a in self.pending.keys() {
            *self.pending_ips.entry(a.ip()).or_insert(0) += 1;
        }
    }

    fn on_connect(&mut self, from: Addr, body: &[u8]) {
        let mut r = R::new(body);
        let (Some(proto), Some(client_salt), Some(name)) = (r.u8(), r.u32(), r.bytes()) else { return };
        if proto != PROTOCOL {
            let mut w = W::new();
            w.u8(MAGIC);
            w.u8(kind::REJECT);
            w.u8(1);
            self.reply(from, &w);
            return;
        }
        // a client that is in already and asks again (our accept got lost): the same answer
        if let Some(&slot) = self.by_addr.get(&from) {
            self.send_accept(slot);
            return;
        }
        if !self.pending.contains_key(&from) && !self.make_room_for_handshake(from) {
            return;
        }
        let name = clean_name(&String::from_utf8_lossy(&name[..name.len().min(96)]), client_salt);
        // (the same CONNECT again, doubled on the way or sent again because our challenge was lost, gets the same challenge: a new one would make the client's answer to the first one wrong)
        let server_salt = match self.pending.get(&from) {
            Some(p) if p.client_salt == client_salt => p.server_salt,
            _ => self.rand(),
        };
        self.add_pending(from, Pending { client_salt, server_salt, name, at: self.now, chan: None, client_id: 0, token_nonce: [0; 24] });
        let mut w = W::new();
        w.u8(MAGIC);
        w.u8(kind::CHALLENGE);
        w.u32(client_salt);
        w.u32(server_salt);
        self.reply(from, &w);
    }

    /// A connect token (docs/SECURE.md): who is asking, if the token is the server's and is good; the answer is the same challenge as ever.
    fn on_connect_token(&mut self, from: Addr, body: &[u8]) {
        let Some(auth) = self.cfg.auth.clone() else { return };
        let mut r = R::new(body);
        let (Some(proto), Some(client_salt)) = (r.u8(), r.u32()) else { return };
        let le64 = |b: Option<&[u8]>| b.and_then(|b| <[u8; 8]>::try_from(b).ok()).map(u64::from_le_bytes);
        let (Some(protocol_id), Some(expire)) = (le64(r.raw(8)), le64(r.raw(8))) else { return };
        let Some(nonce) = r.raw(24).and_then(|b| <[u8; 24]>::try_from(b).ok()) else { return };
        let Some(private) = r.bytes() else { return };
        let refuse = |s: &mut Self, why: u8| {
            s.stats.bad_packets += 1;
            let mut w = W::new();
            w.u8(MAGIC);
            w.u8(kind::REJECT);
            w.u8(why);
            s.reply(from, &w);
        };
        if proto != PROTOCOL {
            refuse(self, 1);
            return;
        }
        // a client that is in already and asks again (our accept got lost): the same answer
        if let Some(&slot) = self.by_addr.get(&from) {
            self.send_accept(slot);
            return;
        }
        // (the same token again from the same place, because our challenge was lost: the same challenge, the same channel)
        if let Some(p) = self.pending.get_mut(&from) {
            if p.client_salt == client_salt && p.token_nonce == nonce {
                p.at = self.now;
                let mut w = W::new();
                w.u8(MAGIC);
                w.u8(kind::CHALLENGE);
                w.u32(client_salt);
                w.u32(p.server_salt);
                self.reply(from, &w);
                return;
            }
        }
        let now_unix = auth.unix_now.unwrap_or_else(secure::unix_now);
        if protocol_id != auth.protocol_id {
            refuse(self, 3);
            return;
        }
        if expire <= now_unix {
            refuse(self, 6);
            return;
        }
        let Some(claims) = secure::open_token(&auth.key, protocol_id, expire, &nonce, private) else {
            refuse(self, 3);
            return;
        };
        // (a token is for one connection: used again from another place, it is refused)
        if self.used_tokens.get(&nonce).is_some_and(|(_, a)| *a != from) {
            refuse(self, 3);
            return;
        }
        if !self.pending.contains_key(&from) && !self.make_room_for_handshake(from) {
            return;
        }
        if self.used_tokens.len() < 100_000 {
            self.used_tokens.insert(nonce, (expire, from));
        }
        let server_salt = self.rand();
        let name = clean_name(&claims.name, client_salt);
        self.add_pending(from, Pending { client_salt, server_salt, name, at: self.now, chan: Some(secure::server_channel(&claims)), client_id: claims.client_id, token_nonce: nonce });
        let mut w = W::new();
        w.u8(MAGIC);
        w.u8(kind::CHALLENGE);
        w.u32(client_salt);
        w.u32(server_salt);
        self.reply(from, &w);
    }

    fn on_response_sealed(&mut self, from: Addr, body: &[u8]) {
        if let Some(&slot) = self.by_addr.get(&from) {
            self.send_accept(slot);
            return;
        }
        let Some(ch) = self.pending.get_mut(&from).and_then(|p| p.chan.as_mut()) else { return };
        let Some(plain) = ch.open(&[MAGIC, kind::RESPONSE_SEALED], body) else {
            self.stats.bad_packets += 1;
            return;
        };
        self.on_response(from, &plain);
    }

    fn on_data_sealed(&mut self, from: Addr, d: &[u8]) {
        let mut r = R::new(d);
        let _ = (r.u8(), r.u8());
        let Some(conn) = r.u16() else { return };
        let Some(&slot) = self.by_conn.get(&conn) else {
            self.stats.bad_packets += 1;
            return;
        };
        let Some(c) = self.clients[slot].as_mut() else { return };
        if c.addr != from {
            self.stats.bad_packets += 1;
            return;
        }
        let Some(ch) = c.chan.as_mut() else { return };
        let Some(plain) = ch.open(&d[..4], &d[4..]) else {
            self.stats.bad_packets += 1;
            return;
        };
        let mut w = W::new();
        w.u8(MAGIC);
        w.u8(kind::DATA);
        w.u16(conn);
        w.raw(&plain);
        self.on_data(from, &w.buf);
    }

    fn on_disconnect_sealed(&mut self, from: Addr, d: &[u8]) {
        let mut r = R::new(d);
        let _ = (r.u8(), r.u8());
        let Some(conn) = r.u16() else { return };
        let Some(&slot) = self.by_conn.get(&conn) else { return };
        let ok = match self.clients[slot].as_mut() {
            Some(c) if c.addr == from => c.chan.as_mut().is_some_and(|ch| ch.open(&d[..4], &d[4..]).is_some()),
            _ => false,
        };
        if ok {
            self.drop_client(slot);
        } else {
            self.stats.bad_packets += 1;
        }
    }

    fn on_response(&mut self, from: Addr, body: &[u8]) {
        let mut r = R::new(body);
        let Some(proof) = r.u32() else { return };
        if let Some(&slot) = self.by_addr.get(&from) {
            self.send_accept(slot);
            return;
        }
        let Some(p) = self.pending.get(&from) else { return };
        if proof != p.client_salt ^ p.server_salt {
            return;
        }
        let Some(p) = self.pending.remove(&from) else { return };
        self.forget_pending_ip(from);
        let (name, chan, client_id) = (p.name, p.chan, p.client_id);
        // (the same player coming back before the server has noticed the old connection is gone: the token says who they are, so the old one goes)
        if client_id != 0 {
            if let Some(&old) = self.by_client.get(&client_id) {
                self.drop_client(old);
            }
        }
        // (a banned name or address: refused with its own reason, docs/ADMIN.md)
        if self.bans.contains(&name, from.ip()) {
            self.stats.refused += 1;
            let mut w = W::new();
            w.u8(MAGIC);
            w.u8(kind::REJECT);
            w.u8(REFUSED_BANNED);
            self.reply(from, &w);
            return;
        }
        // (one address, too many players: `max_per_ip`)
        if self.cfg.max_per_ip > 0 && self.clients.iter().flatten().filter(|c| c.addr.ip() == from.ip()).count() >= self.cfg.max_per_ip {
            self.stats.refused += 1;
            let mut w = W::new();
            w.u8(MAGIC);
            w.u8(kind::REJECT);
            w.u8(REFUSED_ADDRESS);
            self.reply(from, &w);
            return;
        }
        // (one player under one name: a second connection with the name of one that is in is refused, so that nobody can push a player out by typing their name, and what the player kept is given to one avatar only;
        // the same player coming back with a token took over from the old connection just above)
        if self.clients.iter().flatten().any(|c| c.name == name) {
            self.stats.refused += 1;
            let mut w = W::new();
            w.u8(MAGIC);
            w.u8(kind::REJECT);
            w.u8(3);
            self.reply(from, &w);
            return;
        }
        let Some(slot) = self.clients.iter().position(|c| c.is_none()) else {
            self.stats.refused += 1;
            let mut w = W::new();
            w.u8(MAGIC);
            w.u8(kind::REJECT);
            w.u8(2);
            self.reply(from, &w);
            return;
        };
        let mut conn = self.next_conn;
        while conn == 0 || self.by_conn.contains_key(&conn) {
            conn = conn.wrapping_add(1);
        }
        self.next_conn = conn.wrapping_add(1);
        let spawn = self.game.on_join(&mut self.world, slot, &name);
        let entity = self.world.spawn_state(spawn);
        if let Some(e) = self.world.entities.get_mut(&entity) {
            e.owner = Some(slot);
            e.state.class = class::PLAYER;
        }
        let client = Client {
            addr: from,
            conn,
            name,
            entity,
            acks: Acks::new(),
            rel_out: ReliableOut::default(),
            rel_in: ReliableIn::default(),
            last_recv: self.now,
            last_sent: 0,
            accounts: Accounts::default(),
            budget: self.cfg.max_speed * self.cfg.burst_s,
            budget_up: self.cfg.max_up * self.cfg.burst_s,
            budget_down: self.cfg.max_down * self.cfg.burst_s,
            msg_tokens: MSG_RATE,
            last_state_ms: self.now,
            last_correct_ms: 0,
            seen: Sightings::default(),
            mates: Vec::new(),
            occ_rays: self.cfg.occlusion.rays_per_client.saturating_mul(4),
            occ_tick: self.world.tick,
            last_eval: self.world.tick,
            fast: true,
            chan,
            client_id,
        };
        self.clients[slot] = Some(client);
        self.by_addr.insert(from, slot);
        self.by_conn.insert(conn, slot);
        if client_id != 0 {
            self.by_client.insert(client_id, slot);
        }
        self.stats.joins += 1;
        self.send_accept(slot);
    }

    fn send_accept(&mut self, slot: usize) {
        let Some(c) = self.clients[slot].as_ref() else { return };
        let Some(e) = self.world.entities.get(&c.entity) else { return };
        let mut w = W::new();
        w.u8(MAGIC);
        w.u8(kind::ACCEPT);
        w.u16(c.conn);
        w.u16(slot as u16);
        w.u16(self.cfg.tick_hz as u16);
        w.u32(c.entity);
        w.u32(self.world.tick);
        for a in 0..3 {
            w.vari(e.state.pos[a] as i64);
        }
        let to = c.addr;
        if let Some(ch) = self.clients[slot].as_mut().and_then(|c| c.chan.as_mut()) {
            // (with a connect token the accept is sealed: only the token's holder can read where it starts)
            let mut out = vec![MAGIC, kind::ACCEPT_SEALED];
            ch.seal(&[MAGIC, kind::ACCEPT_SEALED], &w.buf[2..], &mut out);
            let mut sealed = W::new();
            sealed.raw(&out);
            self.reply(to, &sealed);
        } else {
            self.reply(to, &w);
        }
    }

    fn drop_client(&mut self, slot: usize) {
        let Some(c) = self.clients[slot].take() else { return };
        self.game.on_leave(&mut self.world, slot);
        self.world.remove(c.entity);
        self.by_addr.remove(&c.addr);
        self.by_conn.remove(&c.conn);
        if c.client_id != 0 && self.by_client.get(&c.client_id) == Some(&slot) {
            self.by_client.remove(&c.client_id);
        }
        self.stats.leaves += 1;
    }

    fn on_data(&mut self, from: Addr, d: &[u8]) {
        let mut r = R::new(d);
        let _ = (r.u8(), r.u8());
        let (Some(conn), Some(seq), Some(ack), Some(bits)) = (r.u16(), r.u16(), r.u16(), r.u32()) else { return };
        let Some(&slot) = self.by_conn.get(&conn) else { return };
        let Some(mut c) = self.clients[slot].take() else { return };
        if c.addr != from {
            self.clients[slot] = Some(c);
            return;
        }
        let since = self.now.saturating_sub(c.last_recv) as f32 / 1000.0;
        c.msg_tokens = (c.msg_tokens + MSG_RATE * since).min(MSG_RATE);
        c.last_recv = self.now;
        let fresh = c.acks.on_receive(seq);
        for rel in c.acks.on_ack(ack, bits, self.now) {
            c.rel_out.acked(rel);
        }
        let mut commands: Vec<Vec<u8>> = Vec::new();
        if fresh {
            while r.left() > 0 {
                let (Some(t), Some(body)) = (r.u8(), r.bytes()) else { break };
                if c.msg_tokens < 1.0 {
                    // (more than MSG_RATE messages a second: the rest of the datagram is dropped, the game goes on)
                    self.stats.throttled += 1;
                    break;
                }
                c.msg_tokens -= 1.0;
                match t {
                    msg::STATE => self.on_state(&mut c, body),
                    msg::RELIABLE => {
                        let mut rr = R::new(body);
                        if let Some(rseq) = rr.u16() {
                            let payload = body[2..].to_vec();
                            commands.extend(c.rel_in.receive(rseq, payload));
                        }
                    }
                    _ => {}
                }
            }
        }
        self.clients[slot] = Some(c);
        if !commands.is_empty() {
            let mut out = Outbox::default();
            let tick = self.world.tick;
            let hz = self.cfg.tick_hz;
            for cmd in commands {
                let mut ctx = Ctx { world: &mut self.world, out: &mut out, tick, tick_hz: hz };
                self.game.on_command(&mut ctx, slot, &cmd);
            }
            self.flush(out);
        }
    }

    /// The state a client says its character has: kept if the character could have got there.
    fn on_state(&mut self, c: &mut Client, body: &[u8]) {
        let mut r = R::new(body);
        let Some((s, _)) = state::read(&mut r) else { return };
        let dt = self.now.saturating_sub(c.last_state_ms) as f32 / 1000.0;
        c.last_state_ms = self.now;
        c.budget = (c.budget + self.cfg.max_speed * dt).min(self.cfg.max_speed * self.cfg.burst_s);
        c.budget_up = (c.budget_up + self.cfg.max_up * dt).min(self.cfg.max_up * self.cfg.burst_s);
        c.budget_down = (c.budget_down + self.cfg.max_down * dt).min(self.cfg.max_down * self.cfg.burst_s);
        let Some(e) = self.world.entities.get_mut(&c.entity) else { return };
        if e.locked {
            // (the game moves this one: the look, the buttons and the rest are the player's, the place is not)
            let st = &mut e.state;
            st.yaw = s.yaw;
            st.pitch = s.pitch;
            st.vel = s.vel;
            st.flags = s.flags;
            st.anim = s.anim;
            st.anim_t = s.anim_t;
            st.extra = s.extra;
            return;
        }
        let old = e.state.pos;
        // (the differences are worked out wider than the numbers: a place at the end of the range, from the client or from a game that put a character there, must not overflow)
        let q = |a: usize| (s.pos[a] as i64 - old[a] as i64) as f32 / state::POS_SCALE;
        let (dx, dz) = (q(0), q(2));
        let dy = q(1);
        let horiz = (dx * dx + dz * dz).sqrt();
        let ok = horiz <= c.budget + 0.05
            && dy <= c.budget_up + 0.05
            && -dy <= c.budget_down + 0.05
            && s.pos.iter().all(|p| p.unsigned_abs() < 6_000_000);
        if ok {
            c.budget = (c.budget - horiz).max(0.0);
            if dy > 0.0 {
                c.budget_up = (c.budget_up - dy).max(0.0);
            } else {
                c.budget_down = (c.budget_down + dy).max(0.0);
            }
            let st = &mut e.state;
            st.pos = s.pos;
            st.yaw = s.yaw;
            st.pitch = s.pitch;
            st.vel = s.vel;
            st.flags = s.flags;
            st.anim = s.anim;
            st.anim_t = s.anim_t;
            // (what it holds is for the others to see: the player's to say; its health is not)
            st.extra = s.extra;
        } else if self.now.saturating_sub(c.last_correct_ms) >= 250 {
            c.last_correct_ms = self.now;
            self.stats.corrections += 1;
            let mut w = W::new();
            w.u8(ev::CORRECT);
            for a in 0..3 {
                w.vari(old[a] as i64);
            }
            c.rel_out.push(w.buf);
        }
    }

    fn flush(&mut self, out: Outbox) {
        for (slot, ev) in out.to {
            self.send_event(slot, ev);
        }
        for ev in out.all {
            for c in self.clients.iter_mut().flatten() {
                c.rel_out.push_lazy(ev.clone());
            }
        }
        for (pos, radius, ev) in out.near {
            for c in self.clients.iter_mut().flatten() {
                let Some(e) = self.world.entities.get(&c.entity) else { continue };
                let p = e.state.pos_m();
                let d = ((p[0] - pos[0]).powi(2) + (p[1] - pos[1]).powi(2) + (p[2] - pos[2]).powi(2)).sqrt();
                if d <= radius {
                    c.rel_out.push_lazy(ev.clone());
                }
            }
        }
    }

    fn tick(&mut self) {
        let can = if self.cfg.profile {
            self.canary_mem.resize(128 * 1024, 1);
            canary(&mut self.canary_mem)
        } else {
            [0; 2]
        };
        let t0 = Instant::now();
        let (cpu0, ph0) = if self.cfg.profile { (thread_cpu_us(), self.stats.phase_ns) } else { (0, [0; 8]) };
        let occ0 = [self.stats.occ_pairs, self.stats.occ_rays, self.stats.occ_still, self.stats.occ_hidden, self.stats.occ_unaffordable];
        self.world.tick += 1;
        let tick = self.world.tick;
        self.world.record(tick, self.cfg.tick_hz as usize + 4);
        self.world.rtt_ms.resize(self.clients.len(), 0.0);
        for (r, c) in self.world.rtt_ms.iter_mut().zip(self.clients.iter()) {
            *r = c.as_ref().map_or(0.0, |c| c.acks.srtt as f32);
        }
        let mut out = Outbox::default();
        {
            let mut ctx = Ctx { world: &mut self.world, out: &mut out, tick, tick_hz: self.cfg.tick_hz };
            self.game.on_tick(&mut ctx);
        }
        self.flush(out);
        // timeouts, and the stale handshakes
        if self.now >= self.last_sweep + 500 {
            self.last_sweep = self.now;
            let dead: Vec<usize> = self.clients.iter().enumerate().filter_map(|(i, c)| c.as_ref().filter(|c| self.now.saturating_sub(c.last_recv) > self.cfg.timeout_ms).map(|_| i)).collect();
            for slot in dead {
                self.drop_client(slot);
            }
            // (a client that is not silent but never acknowledges anything: its backlog of reliable messages is what the server would keep for ever)
            let deaf: Vec<usize> = self.clients.iter().enumerate().filter_map(|(i, c)| c.as_ref().filter(|c| c.rel_out.len() > MAX_BACKLOG).map(|_| i)).collect();
            for slot in deaf {
                self.stats.backlog_kicks += 1;
                self.kick(slot, REFUSED_KICKED);
            }
            let now = self.now;
            let held = self.pending.len();
            self.pending.retain(|_, p| now.saturating_sub(p.at) < 10_000);
            if self.pending.len() != held {
                self.count_pending_ips();
            }
            if let Some(a) = &self.cfg.auth {
                let unix = a.unix_now.unwrap_or_else(secure::unix_now);
                self.used_tokens.retain(|_, (expire, _)| *expire > unix);
            }
            self.grid.prune();
            self.statics.prune();
        }
        if self.cfg.profile {
            self.stats.phase_ns[phase::GAME] += t0.elapsed().as_nanos() as u64;
        }
        let t_grids = self.cfg.profile.then(Instant::now);
        // the entities as they are now, in the grids, and a packet for each client
        self.flat.clear();
        self.grid.clear();
        self.statics.clear();
        for (&id, e) in self.world.entities.iter() {
            let p = e.state.pos_m();
            let at = self.flat.len() as u32;
            let mut s = e.state;
            s.id = id;
            self.flat.push(s);
            if is_static(s.class) {
                self.statics.insert(id, at, p[0], p[2]);
            } else {
                self.grid.insert(at, p[0], p[2]);
            }
        }
        if let Some(t) = t_grids {
            self.stats.phase_ns[phase::GRIDS] += t.elapsed().as_nanos() as u64;
        }
        // the wallhack defence of this tick: the game's switch and what it needs
        let safe = self.game.occlusion();
        self.occ = if safe > 0.0 && safe.is_finite() {
            let mut p = Params::new(&self.cfg.occlusion, tick, self.cfg.tick_hz, safe, self.cfg.rates.near_m, self.cfg.rates.mid_m, self.cfg.max_speed, self.game.level_version(), self.game.low_flags());
            let change = self.game.level_change();
            (p.prev_epoch, p.regions) = (change.from, change.regions);
            p
        } else {
            Params::default()
        };
        // the players the game says are on one side (a party) are never hidden from each other: each client's mates are found again when the game's list changes
        let (allies_version, pairs) = self.game.allies();
        if allies_version != self.allies_seen {
            self.allies_seen = allies_version;
            let pairs = pairs.to_vec();
            for c in self.clients.iter_mut().flatten() {
                c.mates.clear();
                if let Some(&(_, group)) = pairs.iter().find(|p| p.0 == c.entity) {
                    c.mates.extend(pairs.iter().filter(|p| p.1 == group && p.0 != c.entity).map(|p| p.0));
                }
            }
        }
        self.make_and_send_packets();
        self.stats.ticks += 1;
        let us = t0.elapsed().as_micros() as u64;
        self.stats.tick_us_total += us;
        self.stats.tick_us_max = self.stats.tick_us_max.max(us);
        self.governor.update(us, self.cfg.occlusion.governor_ms);
        if self.cfg.profile {
            let cpu = thread_cpu_us().saturating_sub(cpu0);
            self.tick_log.push((us as u32, cpu as u32));
            self.canary_log.push([can[0] as u32, can[1] as u32]);
            if us >= self.worst.us {
                let mut phase_ns = self.stats.phase_ns;
                for (p, b) in phase_ns.iter_mut().zip(ph0) {
                    *p -= b;
                }
                let s = &self.stats;
                let occ = [s.occ_pairs - occ0[0], s.occ_rays - occ0[1], s.occ_still - occ0[2], s.occ_hidden - occ0[3], s.occ_unaffordable - occ0[4]];
                self.worst = Worst { tick, us, cpu_us: cpu, phase_ns, parts: self.game.tick_parts(), players: self.client_count(), canary_ns: can, occ, level_changed: self.occ.epoch != self.last_epoch };
            }
            self.last_epoch = self.occ.epoch;
        }
    }

    /// Makes the packet of every client for this tick and hands it to the network: on this thread, one client after the other, or (with enough clients and [`ServerConfig::threads`]) on several threads at once, each taking a
    /// few clients at a time. Each packet depends only on its own client's state and on the entities as they are this tick, so the bytes are the same either way, and they go out in the order of the slots either way.
    fn make_and_send_packets(&mut self) {
        let occluding = self.occ.safe > 0.0;
        let mut threads = self.cfg.packet_threads(self.client_count());
        // (the wallhack defence's eyes, shared between threads: they need the game's blocker, which not every game has)
        let shared = if occluding && threads > 1 {
            let table: [bool; 256] = std::array::from_fn(|i| Game::occludes(&self.game, i as u8));
            self.game.blocker().map(|blocker| SharedEyes { blocker, occludes: table })
        } else {
            None
        };
        if occluding && threads > 1 && shared.is_none() {
            threads = 1;
        }
        if self.scratch.len() < threads.max(1) {
            self.scratch.resize_with(threads.max(1), Scratch::default);
        }
        let profile = self.cfg.profile;
        let frame = Frame { world: &self.world, flat: &self.flat, grid: &self.grid, statics: &self.statics, cfg: &self.cfg, occ: &self.occ, scale: self.governor.scale(), now: self.now, tick: self.world.tick };
        let mut made = Made::default();
        if threads <= 1 {
            let sc = &mut self.scratch[0];
            for slot in 0..self.clients.len() {
                let Some(c) = self.clients[slot].as_mut() else { continue };
                let packet = if occluding { make_packet(&frame, &self.game, slot, c, sc, &mut made) } else { make_packet(&frame, &NoEyes, slot, c, sc, &mut made) };
                if let Some(p) = packet {
                    let t = profile.then(Instant::now);
                    self.transport.send(p.to, &p.data);
                    if let Some(t) = t {
                        made.phase_ns[phase::SEND] += t.elapsed().as_nanos() as u64;
                    }
                }
            }
        } else {
            let todo: Vec<(usize, &mut Client)> = self.clients.iter_mut().enumerate().rev().filter_map(|(slot, c)| c.as_mut().map(|c| (slot, c))).collect();
            let results = match &shared {
                Some(eyes) => make_parallel(&frame, eyes, todo, &mut self.scratch[..threads]),
                None => make_parallel(&frame, &NoEyes, todo, &mut self.scratch[..threads]),
            };
            let mut packets = Vec::new();
            for (m, out) in results {
                // (the work of all the threads is added up: the phases of a parallel tick are processor time, not the clock time the tick took)
                for (a, b) in made.phase_ns.iter_mut().zip(m.phase_ns) {
                    *a += b;
                }
                made.packets_out += m.packets_out;
                made.bytes_out += m.bytes_out;
                made.records_out += m.records_out;
                made.occ.pairs += m.occ.pairs;
                made.occ.rays += m.occ.rays;
                made.occ.still += m.occ.still;
                made.occ.hidden += m.occ.hidden;
                made.occ.unaffordable += m.occ.unaffordable;
                made.occ_throttled += m.occ_throttled;
                packets.extend(out);
            }
            packets.sort_unstable_by_key(|p| p.slot);
            let t = profile.then(Instant::now);
            for p in packets {
                self.transport.send(p.to, &p.data);
            }
            if let Some(t) = t {
                made.phase_ns[phase::SEND] += t.elapsed().as_nanos() as u64;
            }
        }
        made.add_to(&mut self.stats);
    }

    /// The clock and processor times of the ticks since the last call (with [`ServerConfig::profile`]).
    pub fn take_tick_log(&mut self) -> Vec<(u32, u32)> {
        std::mem::take(&mut self.tick_log)
    }

    /// The share of its rays each client may spend now: 1, or less while the governor ([`Occlusion::governor_ms`]) cuts them to keep the ticks short.
    pub fn occlusion_scale(&self) -> f32 {
        self.governor.scale()
    }

    /// The canaries of the ticks since the last call (with [`ServerConfig::profile`]).
    pub fn take_canary_log(&mut self) -> Vec<[u32; 2]> {
        std::mem::take(&mut self.canary_log)
    }

    /// The slowest tick since the last call (with [`ServerConfig::profile`]).
    pub fn take_worst(&mut self) -> Worst {
        std::mem::take(&mut self.worst)
    }
}

/// The most clients that are made packets for on the calling thread alone: with fewer than this the threads cost more than they save.
const PARALLEL_FROM: usize = 24;

/// What a tick's packets are made from. Nothing in it is changed while the packets are made, so every thread that makes some can read it.
struct Frame<'a> {
    world: &'a World,
    flat: &'a [EntityState],
    grid: &'a Grid,
    statics: &'a Statics,
    cfg: &'a ServerConfig,
    occ: &'a Params,
    /// The share of its rays a client may spend ([`Governor`]).
    scale: f32,
    now: u64,
    tick: u32,
}

/// The lists a packet is made from, kept between ticks so that none is allocated again (one set for each thread).
#[derive(Default)]
struct Scratch {
    cands: Vec<Candidate>,
    statics: Vec<Candidate>,
    due: Vec<(f32, usize)>,
    dists: Vec<f32>,
}

/// What making packets did, to be added to the server's [`Stats`] (each thread counts its own).
#[derive(Default)]
struct Made {
    phase_ns: [u64; 8],
    packets_out: u64,
    bytes_out: u64,
    records_out: u64,
    occ: Counters,
    occ_throttled: u64,
}

impl Made {
    fn add_to(&self, st: &mut Stats) {
        for (a, b) in st.phase_ns.iter_mut().zip(self.phase_ns) {
            *a += b;
        }
        st.packets_out += self.packets_out;
        st.bytes_out += self.bytes_out;
        st.records_out += self.records_out;
        st.occ_pairs += self.occ.pairs;
        st.occ_rays += self.occ.rays;
        st.occ_still += self.occ.still;
        st.occ_hidden += self.occ.hidden;
        st.occ_unaffordable += self.occ.unaffordable;
        st.occ_throttled += self.occ_throttled;
    }
}

/// A packet that has been made, and where it goes.
struct Packet {
    slot: usize,
    to: Addr,
    data: Vec<u8>,
}

/// Nobody hides behind anything (the wallhack defence is off).
struct NoEyes;

impl Eyes for NoEyes {
    fn occludes(&self, _class_id: u8) -> bool {
        false
    }
    fn blocked(&self, _from: [f32; 3], _to: [f32; 3]) -> bool {
        false
    }
}

/// Makes the packets of the clients in `todo` on one thread each of `scratch` at once: each thread takes a few clients at a time off the queue until it is empty (so that a thread whose clients are cheap takes more of them).
fn make_parallel<E: Eyes + Sync + ?Sized>(f: &Frame, eyes: &E, todo: Vec<(usize, &mut Client)>, scratch: &mut [Scratch]) -> Vec<(Made, Vec<Packet>)> {
    const BATCH: usize = 4;
    let queue = std::sync::Mutex::new(todo);
    std::thread::scope(|s| {
        let handles: Vec<_> = scratch
            .iter_mut()
            .map(|sc| {
                let queue = &queue;
                s.spawn(move || {
                    let mut made = Made::default();
                    let mut out = Vec::new();
                    loop {
                        let batch: Vec<(usize, &mut Client)> = {
                            let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
                            let at = q.len().saturating_sub(BATCH);
                            q.split_off(at)
                        };
                        if batch.is_empty() {
                            break;
                        }
                        for (slot, c) in batch {
                            if let Some(p) = make_packet(f, eyes, slot, c, sc, &mut made) {
                                out.push(p);
                            }
                        }
                    }
                    (made, out)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("a thread that makes packets panicked")).collect()
    })
}

/// Makes the packet of the client in `slot` for this tick (`None` when it is not made one this tick or has nothing to say): the entities that matter to it, the ones out of its sight left out, in the order and at the rates
/// of [`Accounts`], reliable messages first. It reads the world and the grids of the frame and changes only the client, its own scratch lists and its own counts, so that the packets of several clients can be made at once
/// on several threads; handing the packet to the network is the caller's.
#[allow(unused_assignments)]
fn make_packet<E: Eyes + ?Sized>(f: &Frame, eyes: &E, slot: usize, c: &mut Client, sc: &mut Scratch, made: &mut Made) -> Option<Packet> {
    let own = f.world.entities.get(&c.entity)?;
    let op = own.state.pos_m();
    let own_state = own.state;
    // a client with nobody near it is made a packet once in `idle_every` ticks (its own tick of them: the slots are spread over the ticks), unless an urgent reliable message waits (a hit, a correction; not a broadcast to everybody)
    let tick = f.tick;
    let every = f.cfg.idle_every.max(1);
    if !c.fast && every > 1 && !c.rel_out.needs_packet_now() && tick.wrapping_add(slot as u32) % every != 0 {
        return None;
    }
    let (from, to) = (c.last_eval, tick);
    c.last_eval = tick;
    let mut lap = f.cfg.profile.then(Instant::now);
    macro_rules! lap {
        ($phase:expr) => {
            if let Some(since) = lap {
                let now = Instant::now();
                made.phase_ns[$phase] += now.duration_since(since).as_nanos() as u64;
                lap = Some(now);
            }
        };
    }
    // the entities that matter to it: the ones that move, all of them, and the things that do not, the share that is offered on this tick
    let interest = f.cfg.interest;
    let me = c.entity;
    let cands = &mut sc.cands;
    let statics = &mut sc.statics;
    let due = &mut sc.due;
    cands.clear();
    statics.clear();
    let flat = f.flat;
    let near = |idx: u32, out: &mut Vec<Candidate>| {
        let s = &flat[idx as usize];
        if s.id == me {
            return;
        }
        let p = s.pos_m();
        let dist = ((p[0] - op[0]).powi(2) + (p[2] - op[2]).powi(2)).sqrt();
        if dist <= interest {
            out.push(Candidate { idx, id: s.id, dist, class: s.class });
        }
    };
    f.grid.query(op[0], op[2], interest, |idx| near(idx, cands));
    // (the things on the ground that were offered on the ticks since this client's last packet: one tick's worth, or `every`)
    for t in (from + 1).max(to.saturating_sub(every - 1))..=to {
        f.statics.query(t, op[0], op[2], interest, |idx| near(idx, statics));
    }
    statics.sort_unstable_by(|a, b| a.dist.total_cmp(&b.dist));
    lap!(phase::GATHER);
    // the players that are out of this client's sight are not candidates (wallhack defence)
    if f.occ.safe > 0.0 {
        // (the rays this client has: topped up for the ticks since its last packet; the near pairs are tested before the far ones, so that when the rays run out it is the far end that waits)
        let scale = f.scale;
        let per = if scale < 1.0 { ((f.cfg.occlusion.rays_per_client as f32 * scale).round() as u32).max(1) } else { f.cfg.occlusion.rays_per_client };
        let mut rays = c.occ_rays.saturating_add(per.saturating_mul(tick.wrapping_sub(c.occ_tick).min(8))).min(per.saturating_mul(4));
        c.occ_tick = tick;
        let mut n = Counters::default();
        if c.mates.is_empty() {
            c.seen.filter(eyes, f.occ, &own_state, flat, cands, &mut rays, &mut n);
        } else {
            c.seen.filter_except(eyes, f.occ, &own_state, flat, cands, &mut rays, &mut n, &c.mates);
        }
        c.occ_rays = rays;
        made.occ.pairs += n.pairs;
        made.occ.rays += n.rays;
        made.occ.still += n.still;
        made.occ.hidden += n.hidden;
        made.occ.unaffordable += n.unaffordable;
        made.occ_throttled += (scale < 1.0) as u64;
        lap!(phase::OCCLUDE);
    }
    let dists = &mut sc.dists;
    dists.clear();
    dists.extend(cands.iter().filter(|c| c.class != class::PROJECTILE).map(|c| c.dist));
    let cuts = f.cfg.rates.cuts(dists);
    let fastest = c.accounts.earn(cands, &f.cfg.rates, f.cfg.tick_hz as f32, cuts, (from, to), due);
    c.fast = fastest >= f.cfg.rates.near;
    lap!(phase::RANK);
    // reliable messages first (a few hundred bytes at most), then as many records as the budget and the datagram allow
    let rel = c.rel_out.due(f.now, rto(c.acks.srtt), 400);
    let mut w = W::new();
    let (ack, bits) = c.acks.header();
    let seq = c.acks.local_seq;
    w.u8(MAGIC);
    w.u8(kind::DATA);
    w.u16(c.conn);
    w.u16(seq);
    w.u16(ack);
    w.u32(bits);
    let mut rel_seqs = Vec::with_capacity(rel.len());
    for (rseq, body) in &rel {
        w.u8(msg::RELIABLE);
        w.varu(body.len() as u64 + 2);
        w.u16(*rseq);
        w.raw(body);
        rel_seqs.push(*rseq);
    }
    let mut recs = W::new();
    let mut count = 0u64;
    let sealed = if c.chan.is_some() { secure::OVERHEAD } else { 0 };
    let room = MAX_DATAGRAM.saturating_sub(w.len() + 16 + sealed).min(f.cfg.snapshot_budget);
    // (the things on the ground first, the nearest, but at most a sixth of the room: a heap of loot must not push the players out of a packet)
    for cand in statics.iter() {
        let before = recs.len();
        state::write(&mut recs, &flat[cand.idx as usize], Detail::Basic);
        if recs.len() > room / 6 {
            recs.buf.truncate(before);
            break;
        }
        count += 1;
    }
    for &(_, i) in due.iter() {
        let cand = &cands[i];
        let detail = if f.cfg.rates.full(cand.class, cand.dist) { Detail::Full } else { Detail::Basic };
        let before = recs.len();
        state::write(&mut recs, &flat[cand.idx as usize], detail);
        if recs.len() > room {
            recs.buf.truncate(before);
            break;
        }
        count += 1;
        c.accounts.paid(cand.id);
    }
    lap!(phase::WRITE);
    let idle = f.now.saturating_sub(c.last_sent) >= 100;
    if count > 0 || !rel.is_empty() || idle {
        if count > 0 {
            let mut body = W::new();
            body.u32(f.tick);
            body.varu(count);
            body.raw(&recs.buf);
            w.u8(msg::SNAPSHOT);
            w.varu(body.len() as u64);
            w.raw(&body.buf);
        }
        debug_assert!(w.len() >= HEADER);
        c.acks.on_send(f.now, rel_seqs);
        c.last_sent = f.now;
        made.packets_out += 1;
        made.bytes_out += (w.len() + sealed) as u64;
        made.records_out += count;
        lap!(phase::WRITE);
        let data = if let Some(ch) = c.chan.as_mut() {
            // (the header with the connection number stays in the clear and is authenticated; the rest is sealed)
            let mut out = Vec::with_capacity(w.len() + secure::OVERHEAD);
            out.extend_from_slice(&[MAGIC, kind::DATA_SEALED, w.buf[2], w.buf[3]]);
            let aad = [MAGIC, kind::DATA_SEALED, w.buf[2], w.buf[3]];
            ch.seal(&aad, &w.buf[4..], &mut out);
            out
        } else {
            w.buf
        };
        lap!(phase::SEND);
        return Some(Packet { slot, to: c.addr, data });
    }
    None
}
