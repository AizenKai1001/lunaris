//! The game server (docs/GAME_SERVER.md): a [`Game`] that is the real simulation. [`SimGame`] owns a headless [`Sim`] loaded from the game's own scene, makes an avatar in it for every player that
//! connects (from the scene's `NetHost.player` prefab), puts what each player reports on its avatar before every step (the place and look, the buttons as the actions of its `Weapon`), steps the simulation,
//! and copies what the players must see back into the big-match server's world: the things with a `NetEntity` (walls, machines, helicopters, loot, corpses), the avatars' health, and the events the scene
//! lists. The rules of the game (weapons, building, loot, the machines, respawn) are the engine's systems and the game's scripts running on the server, once, with nobody's computer to trust.
//!
//! What a client reports is trusted for exactly what the library already trusts it for: where its character is, within what a character can do. Everything else is decided here: a shot is the avatar's
//! `Weapon` firing in the simulation, a piece is built by the avatar's `BuildTool`, health is the avatar's `HitPoints`.
//!
//! Commands from a client are the reliable message [`msg::ACTION`] (a name and four numbers); events of the simulation reach the clients near them as [`msg::EVENT`], which a driver
//! ([`super::play::NetDriver`]) turns into events of the client's own simulation.
use super::bytes::{R, W};
use super::interest::is_static;
use super::netinv;
use super::netsync;
use super::server::{ev, Ctx, Game, LevelChange, World as NetWorld};
use super::state::{class, EntityState};
use lunaris_ecs::{Entity, Sym, Uid};
use lunaris_json::Json;
use lunaris_sim::inventory::{table_of, Inventory};
use lunaris_sim::components::{BuildTool, Controls, Door, Hacker, Heat, HitPoints, Interactable, Interactor, LocalTransform, NetAvatar, NetEntity, NetHost, Loadout, Parent, Seat, Seated, Transform, Velocity, Vitals, Weapon};
use lunaris_sim::sight::{Changed, Sight};
use super::occlusion::Blocker;
use lunaris_sim::Sim;

/// The level's line of sight is a table that is only read once it is gathered: any number of threads may ask it at once.
impl Blocker for Sight {
    fn blocked(&self, from: [f32; 3], to: [f32; 3]) -> bool {
        !self.clear(from, to)
    }
}
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

mod party_glue;

type V3 = [f32; 3];

/// The kinds of message that a game server and its players send each other (the first byte of a reliable message; the game's own may use any other below 0xF0).
pub mod msg {
    /// A player's command: this byte, the name (a length byte and the bytes) and four `f32`s (little-endian).
    pub const ACTION: u8 = 0x03;
    /// What a player's hands say while it sits in a vehicle (docs/SEATS.md): this byte and four signed bytes, the axes (127 is 1). Said again whenever they change; the server forgets them after two thirds of a second of silence.
    pub const CONTROL: u8 = 0x04;
    /// An event of the simulation, told to the players near it: this byte, the name (a length byte and the bytes), the place (three `f32`s) and, after them, the server's number of the entity the event is about (a
    /// variable-length integer, 0 or absent when it is not a replicated thing; an old client stops reading before it, an old server does not send it, so both ways work with each other).
    pub const EVENT: u8 = 0x40;
    /// To one player: it sits in (or, with a vehicle number 0, has got out of) a vehicle: this byte, the server's number for the vehicle (`u32`) and where the player sits in the vehicle's axes (three `f32`s).
    pub const RIDE: u8 = 0x45;
    /// To one player: it now controls (or, with a number 0, no longer controls) a machine of the server's with its goggles: this byte and the server's number for the machine (`u32`).
    pub const VIEW: u8 = 0x46;
    /// To one player, about itself (docs/GAME_SERVER.md, "What a player is told about itself"): this byte and what its interface shows, see [`super::Status`]. Sent when it changes, at most five times a second.
    pub const STATUS: u8 = 0x47;
    /// A player's chat message to the server (`NetHost.chat`): this byte and the text (UTF-8, at most 480 bytes; the server cleans it and keeps it to 120 characters).
    pub const SAY: u8 = 0x05;
    /// What the server tells the players within the chat distance of a speaker: this byte, the speaker's name (a length byte and the bytes) and the text (the rest of the message).
    pub const CHAT: u8 = 0x49;
}

/// What a player is told about its own character, for its interface (the server's vitals, its gun and its bag are not on the client): every number is a byte (0 to 255) except the magazine and the
/// reserve. The client puts them in its `NetPlayer` as fractions (`health`, `food`, `water`, `stamina` 0 to 1), `heat` (the director's meter, 0 to 10) and counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub health: u8,
    pub food: u8,
    pub water: u8,
    pub stamina: u8,
    /// The director's heat (0 to 10) times 25.5.
    pub heat: u8,
    /// Bit 0 bleeding, bit 1 exhausted (cannot sprint), bit 2 reloading, bits 3 to 5 the Core's alert tier for this player (0 to 7, docs/WALKER.md).
    pub flags: u8,
    /// The item in the hand when it is a gun of a `Loadout` (0: none; the item numbers of the `ItemTable`, up to 255).
    pub gun: u8,
    pub mag: u16,
    pub reserve: u16,
}

impl Status {
    pub const BLEEDING: u8 = 1;
    pub const EXHAUSTED: u8 = 2;
    pub const RELOADING: u8 = 4;
    /// The bit of `flags` the alert tier starts at (three bits).
    pub const TIER_SHIFT: u8 = 3;

    /// The message that tells a player this.
    pub fn message(&self) -> Vec<u8> {
        let mut w = W::new();
        w.u8(msg::STATUS);
        for b in [self.health, self.food, self.water, self.stamina, self.heat, self.flags, self.gun] {
            w.u8(b);
        }
        w.raw(&self.mag.to_le_bytes());
        w.raw(&self.reserve.to_le_bytes());
        w.buf
    }

    /// Read a message of [`msg::STATUS`].
    pub fn parse(data: &[u8]) -> Option<Status> {
        let mut r = R::new(data);
        if r.u8()? != msg::STATUS {
            return None;
        }
        let mut b = [0u8; 7];
        for x in b.iter_mut() {
            *x = r.u8()?;
        }
        let m = r.raw(2)?;
        let s = r.raw(2)?;
        Some(Status { health: b[0], food: b[1], water: b[2], stamina: b[3], heat: b[4], flags: b[5], gun: b[6], mag: u16::from_le_bytes([m[0], m[1]]), reserve: u16::from_le_bytes([s[0], s[1]]) })
    }
}

/// The message a server sends a player who has taken over a machine (or, with `machine` 0, has given it back).
pub fn view(machine: u32) -> Vec<u8> {
    let mut w = W::new();
    w.u8(msg::VIEW);
    w.raw(&machine.to_le_bytes());
    w.buf
}

/// Read a message of [`msg::VIEW`]: the machine's number (0: none).
pub fn parse_view(data: &[u8]) -> Option<u32> {
    let mut r = R::new(data);
    if r.u8()? != msg::VIEW {
        return None;
    }
    let b = r.raw(4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The message a player's game sends while it sits in a vehicle: the four axes of its hands.
/// The message a player sends to say something to the players near it (`NetHost.chat`).
pub fn say(text: &str) -> Vec<u8> {
    let mut w = W::new();
    w.u8(msg::SAY);
    let t = text.as_bytes();
    w.raw(&t[..t.len().min(480)]);
    w.buf
}

/// What the server tells the players near a speaker: the speaker's name and what was said.
pub fn chat(name: &str, text: &str) -> Vec<u8> {
    let mut w = W::new();
    w.u8(msg::CHAT);
    let n = name.as_bytes();
    let n = &n[..n.len().min(60)];
    w.u8(n.len() as u8);
    w.raw(n);
    w.raw(text.as_bytes());
    w.buf
}

/// Read a message of [`msg::CHAT`]: the speaker's name and the text.
pub fn parse_chat(data: &[u8]) -> Option<(String, String)> {
    let mut r = R::new(data);
    if r.u8()? != msg::CHAT {
        return None;
    }
    let n = r.u8()? as usize;
    let name = String::from_utf8_lossy(r.raw(n)?).into_owned();
    let text = String::from_utf8_lossy(&data[2 + n..]).into_owned();
    Some((name, text))
}

/// What a chat message says once the server has cleaned it: printable characters only, runs of white space made one space, nothing before the first or after the last word, at most 120 characters.
pub fn clean_chat(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(&body[..body.len().min(480)]).into_owned();
    let mut out = String::new();
    let mut space = true;
    for c in text.chars() {
        if c == '\u{FFFD}' || super::server::is_text_mark(c) || (c.is_control() && !c.is_whitespace()) {
            continue;
        }
        if c.is_whitespace() {
            if !space {
                out.push(' ');
                space = true;
            }
            continue;
        }
        out.push(c);
        space = false;
        if out.chars().count() >= 120 {
            break;
        }
    }
    out.trim_end().to_string()
}

pub fn control(axes: [f32; 4]) -> Vec<u8> {
    let mut w = W::new();
    w.u8(msg::CONTROL);
    for a in axes {
        let v = if a.is_finite() { (a.clamp(-1.0, 1.0) * 127.0).round() as i8 } else { 0 };
        w.u8(v as u8);
    }
    w.buf
}

/// Read a message of [`msg::CONTROL`].
pub fn parse_control(data: &[u8]) -> Option<[f32; 4]> {
    let mut r = R::new(data);
    if r.u8()? != msg::CONTROL {
        return None;
    }
    let mut axes = [0.0f32; 4];
    for a in axes.iter_mut() {
        *a = (r.u8()? as i8) as f32 / 127.0;
    }
    Some(axes)
}

/// The message a server sends a player who has sat down in a vehicle (or, with `vehicle` 0, got out).
pub fn ride(vehicle: u32, seat: V3) -> Vec<u8> {
    let mut w = W::new();
    w.u8(msg::RIDE);
    w.raw(&vehicle.to_le_bytes());
    for a in seat {
        w.f32(a);
    }
    w.buf
}

/// Read a message of [`msg::RIDE`]: the vehicle's number (0: none) and where the player sits.
pub fn parse_ride(data: &[u8]) -> Option<(u32, V3)> {
    let mut r = R::new(data);
    if r.u8()? != msg::RIDE {
        return None;
    }
    let b = r.raw(4)?;
    let id = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let seat = [r.f32()?, r.f32()?, r.f32()?];
    seat.iter().all(|v| v.is_finite()).then_some((id, seat))
}

/// The command a player's game sends: `name` (`build:wall`, `place`, or the game's own) and four numbers.
pub fn action(name: &str, args: [f32; 4]) -> Vec<u8> {
    let mut w = W::new();
    w.u8(msg::ACTION);
    let n = name.as_bytes();
    let n = &n[..n.len().min(60)];
    w.u8(n.len() as u8);
    w.raw(n);
    for a in args {
        w.f32(if a.is_finite() { a } else { 0.0 });
    }
    w.buf
}

/// An event as the server sends it (see [`msg::EVENT`]), about no entity in particular.
pub fn event(name: &str, pos: V3) -> Vec<u8> {
    event_about(name, pos, 0)
}

/// An event as the server sends it, with the server's number (`NetPlayer.id`, `NetPuppet.id`) of the entity it is about: the victim of a `hit` or a `kill`, the shooter of a `shot` (0: none).
pub fn event_about(name: &str, pos: V3, about: u32) -> Vec<u8> {
    let mut w = W::new();
    w.u8(msg::EVENT);
    let n = name.as_bytes();
    let n = &n[..n.len().min(60)];
    w.u8(n.len() as u8);
    w.raw(n);
    for p in pos {
        w.f32(p);
    }
    if about != 0 {
        w.varu(about as u64);
    }
    w.buf
}

/// Read a message of [`msg::EVENT`]: the name and the place.
pub fn parse_event(data: &[u8]) -> Option<(String, V3)> {
    parse_event_about(data).map(|(n, p, _)| (n, p))
}

/// Read a message of [`msg::EVENT`]: the name, the place and the server's number of the entity it is about (0 when the message has none).
pub fn parse_event_about(data: &[u8]) -> Option<(String, V3, u32)> {
    let mut r = R::new(data);
    if r.u8()? != msg::EVENT {
        return None;
    }
    let n = r.u8()? as usize;
    let name = String::from_utf8_lossy(r.raw(n)?).into_owned();
    let pos = [r.f32()?, r.f32()?, r.f32()?];
    let about = if r.left() > 0 { r.varu().and_then(|v| u32::try_from(v).ok()).unwrap_or(0) } else { 0 };
    pos.iter().all(|v| v.is_finite()).then_some((name, pos, about))
}

fn parse_action(data: &[u8]) -> Option<(String, [f32; 4])> {
    let mut r = R::new(data);
    if r.u8()? != msg::ACTION {
        return None;
    }
    let n = r.u8()? as usize;
    let name = String::from_utf8_lossy(r.raw(n)?).into_owned();
    let mut args = [0.0f32; 4];
    for a in args.iter_mut() {
        let v = r.f32()?;
        *a = if v.is_finite() { v } else { 0.0 };
    }
    Some((name, args))
}

#[derive(Default, Clone)]
struct Host {
    loaded: bool,
    player: String,
    forward: Vec<(String, f32)>,
    static_every: u32,
    save: String,
    save_every: f32,
    persist: Vec<String>,
    /// How long a stored character is kept after its player left, in seconds (0: for ever): `NetHost.character_days`.
    character_secs: u64,
    drop: String,
    /// The layout of the world the scene has (`NetHost.world_version`): a save of another one is not loaded.
    world_version: u32,
    /// What using an item does (`NetHost.uses`): per item key, what it adds to the avatar's `Vitals` requests.
    uses: Vec<ItemUse>,
    /// The picture of each item lying on the ground (`NetHost.item_kinds`): per item key, the `kind` its pickup is told to the clients with.
    item_kinds: Vec<(String, u32)>,
    /// Lag compensation: the most milliseconds a shot is looked back by (0: off) and the milliseconds the clients show others behind the present.
    lag_comp: f32,
    interp: f32,
    /// The resources every client is told (`NetHost.sync`) and the most often they are sent, in seconds.
    sync: Vec<String>,
    sync_every: f32,
    /// The components whose entities the server does not keep (`NetHost.skip`).
    skip: Vec<String>,
    /// The safe radius of the wallhack defence (`NetHost.occlusion`, 0: off) and whether machines hide behind walls too (`NetHost.occlude_machines`).
    occlusion: f32,
    occlude_machines: bool,
    /// How far a chat message is told (`NetHost.chat`, 0: chat is off).
    chat: f32,
    /// The most members a party has (`NetHost.party`, below 2: no parties).
    party: u32,
}

/// Where the things that can be shot were at one tick (after its steps), sorted by uid.
#[derive(Default)]
struct Frame {
    tick: u32,
    at: Vec<(u64, V3)>,
}

/// What a player's `extra` carries when its avatar has a `Loadout`: the slot of the bag it says is in its hand in the low `HELD_BITS`, and (the server's, written every tick) the item of the gun there above them,
/// so the other players see the gun that is really in the hand.
const HELD_BITS: u32 = 6;
const HELD_SLOT: u32 = (1 << HELD_BITS) - 1;
const HELD_ITEM_MAX: u32 = (1 << (16 - HELD_BITS)) - 1;

struct Avatar {
    uid: u64,
    net: u32,
    name: String,
    /// The child that carries the tools and turns with the pitch (0: the avatar itself).
    head: u64,
    /// Where the player was put at the last tick: a place the simulation moved the avatar to is a teleport.
    set: V3,
    /// What the player was last told about its bag and about the container it has open (a hash of the contents; 0: nothing told).
    told_bag: u64,
    told_container: u64,
    /// What the player's hands said last (while it sits in a vehicle) and the tick it was said.
    axes: [f32; 4],
    axes_tick: u32,
    /// The seat the player was last told it sits in (0: none), and the machine it was last told it controls.
    told_ride: u64,
    told_view: u64,
    /// What the player was last told about itself (see [`Status`]) and the tick it was told; `None` before the first.
    told_status: Option<Status>,
    status_tick: u32,
    /// Whether the player has been told every `NetHost.sync` resource since it joined.
    told_sync: bool,
    /// How many chat messages the player may still say at once (a burst of three, one more every 1.2 s), and the tick it was last worked out.
    chat_credit: f32,
    chat_tick: u32,
    /// The tick until which the player's chat is dropped (an admin's `mute`).
    muted_until: u32,
}

/// What the server has spent its time on (microseconds, summed) and what it holds, for a report.
#[derive(Clone, Debug, Default)]
pub struct SimStats {
    pub ticks: u64,
    pub avatars: usize,
    pub mirrored: usize,
    pub input_us: u64,
    pub step_us: u64,
    pub mirror_us: u64,
    pub events_sent: u64,
    pub saves: u64,
    /// Saves that could not be written (a full disk, a folder that cannot be written to): the save before is still there.
    pub saves_failed: u64,
    pub last_save_ms: u64,
    /// The longest the server's own thread was held up by starting a save (the copy of the world, `SimGame::save_in_background`), microseconds.
    pub save_copy_us_max: u64,
    /// The wallhack defence (`NetHost.occlusion`): how many times the static world was gathered for the line-of-sight rays, the microseconds that took in all, and how many shapes the last one held.
    pub sight_builds: u64,
    /// How many of those gatherings found the world changed (a pickup dropped or taken, a wall built): every answer the defence has kept is asked again then.
    pub sight_changes: u64,
    pub sight_us: u64,
    pub sight_shapes: usize,
    /// The microseconds `SimGame::tick_parties` took in all (only counted while a party or an invitation exists: with none it costs a test of one flag) and the ticks that were.
    pub party_us: u64,
    pub party_ticks: u64,
}

pub struct SimGame {
    pub sim: Sim,
    host: Host,
    avatars: BTreeMap<usize, Avatar>,
    /// The simulation's uid of every replicated entity and the server's number for it.
    mirror: HashMap<u64, u32>,
    /// What the players who are not here keep: the components `NetHost.persist` names, by the player's name.
    pub(crate) stored: BTreeMap<String, Vec<(String, Json)>>,
    /// How many strings the network has put into the world's table (see [`MAX_MINTED`]).
    minted: u32,
    ticks: u64,
    next_save: u64,
    pub stats: SimStats,
    /// The last half second or so of where characters and moving things were, a frame a tick, oldest first (lag compensation).
    hist: std::collections::VecDeque<Frame>,
    /// A save that is being written on its own thread, and when it started.
    saving: Option<(std::thread::JoinHandle<bool>, Instant)>,
    /// What the clients were last told of `NetHost.sync`.
    sync: netsync::SyncOut,
    /// The static world as the line-of-sight rays of the wallhack defence see it (`NetHost.occlusion`; `None` while that is off), its signature, a number that goes up whenever the signature changes
    /// (`Game::level_version`: what the server found out about who sees whom is asked again), and the tick it is gathered again at.
    sight: Option<Sight>,
    sight_sig: u64,
    sight_epoch: u32,
    sight_next: u64,
    /// What the last change of the static world touched (`Game::level_change`).
    sight_change: LevelChange,
    /// Where the last tick's time went (microseconds), for the profile of the slowest tick ([`Game::tick_parts`]).
    parts: TickParts,
    /// The parties (docs/PARTIES.md): the table, and what the server is told of it.
    party: party_glue::PartyState,
    /// A wipe was asked for (`wipe world yes`: `Some(false)`, `wipe all yes`: `Some(true)`, docs/ADMIN.md): what a save writes from then on, and that the server is to stop.
    wipe: Option<bool>,
}

/// The `world_version` a wiped save is written with: no scene has it, so the next start does not load the world from it (the players' characters are kept, as for any save of another layout).
const WIPED_LAYOUT: u32 = u32::MAX;

/// What `on_tick` spent its last run on, in microseconds.
#[derive(Clone, Copy, Debug, Default)]
struct TickParts {
    input: u64,
    rewind: u64,
    step: u64,
    tell: u64,
    mirror: u64,
    sight: u64,
    save: u64,
}

/// A turn about the local x axis: it looks up by `angle` radians (a negative angle looks down).
fn quat_x(angle: f32) -> [f32; 4] {
    let h = angle * 0.5;
    [h.sin(), 0.0, 0.0, h.cos()]
}

fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

fn yaw_rot(yaw: f32) -> [f32; 4] {
    let h = yaw * 0.5;
    [0.0, h.sin(), 0.0, h.cos()]
}

fn health_byte(world: &lunaris_ecs::World, e: Entity) -> u8 {
    match world.get::<HitPoints>(e) {
        Some(h) if h.dead || h.value <= 0.0 => 0,
        Some(h) => ((h.value / h.max.max(1.0)).clamp(0.0, 1.0) * 255.0).ceil().clamp(1.0, 255.0) as u8,
        None => 255,
    }
}

/// What using an item does: the item's key and what it adds to the user's `Vitals` requests.
#[derive(Clone, Debug, Default, PartialEq)]
struct ItemUse {
    key: String,
    eat: f32,
    drink: f32,
    heal: f32,
    bandage: bool,
    /// A tool: it is not used up.
    keep: bool,
}

/// Read `NetHost.uses`: one line an item, `use KEY eat=30 drink=0 heal=0 bandage=1 keep=0` (`#` starts a comment; what is not understood is skipped).
fn parse_uses(text: &str) -> Vec<ItemUse> {
    let mut out: Vec<ItemUse> = Vec::new();
    for line in text.lines() {
        let mut words = line.split('#').next().unwrap_or("").split_whitespace();
        if words.next() != Some("use") {
            continue;
        }
        let Some(key) = words.next() else { continue };
        let mut u = ItemUse { key: key.to_string(), ..Default::default() };
        for w in words {
            let Some((k, v)) = w.split_once('=') else { continue };
            let Some(n) = v.parse::<f32>().ok().filter(|n| n.is_finite()) else { continue };
            match k {
                "eat" => u.eat = n.clamp(0.0, 1000.0),
                "drink" => u.drink = n.clamp(0.0, 1000.0),
                "heal" => u.heal = n.clamp(0.0, 1000.0),
                "bandage" => u.bandage = n != 0.0,
                "keep" => u.keep = n != 0.0,
                _ => {}
            }
        }
        out.retain(|x| x.key != u.key);
        out.push(u);
    }
    out
}

/// Read `NetHost.item_kinds`: one line an item, `KEY KIND` (`#` starts a comment; a line that is not an item key and a number is skipped, a later line for the same key wins).
fn parse_item_kinds(text: &str) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::new();
    for line in text.lines() {
        let mut words = line.split('#').next().unwrap_or("").split_whitespace();
        let (Some(key), Some(kind)) = (words.next(), words.next().and_then(|k| k.parse::<u32>().ok())) else { continue };
        out.retain(|(k, _)| k != key);
        out.push((key.to_string(), kind));
    }
    out
}

/// What the player at `e` (with its tools on `tool`) is told about itself.
fn status_of(world: &lunaris_ecs::World, e: Entity, tool: Entity) -> Status {
    let byte = |v: f32| ((v / 100.0).clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut s = Status { health: health_byte(world, e), ..Default::default() };
    if let Some(v) = world.get::<Vitals>(e) {
        (s.food, s.water, s.stamina) = (byte(v.hunger), byte(v.thirst), byte(v.stamina));
        if v.bleeding > 0.0 {
            s.flags |= Status::BLEEDING;
        }
        if v.exhausted {
            s.flags |= Status::EXHAUSTED;
        }
    }
    if let Some(h) = world.get::<Heat>(e) {
        s.heat = (h.value.clamp(0.0, 10.0) * 25.5).round() as u8;
        s.flags |= (h.tier.min(7) as u8) << Status::TIER_SHIFT;
    }
    if let Some(w) = world.get::<Weapon>(tool).filter(|w| w.active) {
        // (the gun of a `Loadout`, or the fixed weapon of a character without one: no number for it)
        s.gun = world.get::<Loadout>(tool).map_or(0, |l| l.holding.min(255) as u8);
        s.mag = w.mag.min(65535) as u16;
        s.reserve = if w.infinite { u16::MAX } else { w.reserve.min(65535) as u16 };
        if w.reload_left > 0.0 {
            s.flags |= Status::RELOADING;
        }
    }
    s
}

/// The marker that starts the checksum member at the end of a save file.
const SUM_KEY: &str = ",\n  \"checksum\": \"";

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// A save file's text with a checksum of it added as the last member of the object (it stays JSON: a reader that does not know the member skips it). The text is changed in place: no second copy of it.
pub fn seal_save(mut text: String) -> String {
    let end = text.trim_end().len();
    text.truncate(end);
    if !text.ends_with('}') {
        return text;
    }
    text.pop();
    let end = text.trim_end().len();
    text.truncate(end);
    let sum = fnv1a(text.as_bytes());
    text.push_str(SUM_KEY);
    text.push_str(&format!("{sum:016x}\"\n}}\n"));
    text
}

/// The same check as [`check_save`] for a file, read in pieces: the checksum member is at the end, the hash runs over the rest, and the file is never held whole in memory.
fn check_save_file(path: &std::path::Path) -> Result<bool, String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    let tail_len = len.min(96);
    f.seek(SeekFrom::Start(len - tail_len)).map_err(|e| e.to_string())?;
    let mut tail = vec![0u8; tail_len as usize];
    f.read_exact(&mut tail).map_err(|e| e.to_string())?;
    let tail_text = String::from_utf8_lossy(&tail).into_owned();
    let Some(at_in_tail) = tail_text.rfind(SUM_KEY) else {
        return if tail_text.trim_end().ends_with('}') { Ok(false) } else { Err("it is cut short".to_string()) };
    };
    let after = &tail_text[at_in_tail + SUM_KEY.len()..];
    let Some(hex) = after.get(..16) else { return Err("its checksum is cut short".to_string()) };
    if after[16..].trim_end() != "\"\n}" {
        return Err("what follows its checksum is not the end of a save".to_string());
    }
    let want = u64::from_str_radix(hex, 16).map_err(|_| "its checksum is not a number".to_string())?;
    // (the marker's place in the file: the tail was read from the end, and lossy text of bytes that are not text could be longer than they were, so the place is found in bytes)
    let marker_at = tail.windows(SUM_KEY.len()).rposition(|w| w == SUM_KEY.as_bytes()).ok_or_else(|| "its checksum is not where it should be".to_string())? as u64 + (len - tail_len);
    f.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut left = marker_at;
    let mut buf = vec![0u8; 1 << 16];
    while left > 0 {
        let want = (left as usize).min(buf.len());
        let n = f.read(&mut buf[..want]).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("it is cut short".to_string());
        }
        for &b in &buf[..n] {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        left -= n as u64;
    }
    if h == want {
        Ok(true)
    } else {
        Err("its checksum does not match (the file was changed or damaged)".to_string())
    }
}

/// Is a save file's text whole? `Ok(true)`: its checksum is there and right; `Ok(false)`: it has none (written before there was one) but ends like a save; an error: it is cut short, or its checksum is wrong.
pub fn check_save(text: &str) -> Result<bool, String> {
    let Some(at) = text.rfind(SUM_KEY) else {
        return if text.trim_end().ends_with('}') { Ok(false) } else { Err("it is cut short".to_string()) };
    };
    let tail = &text[at + SUM_KEY.len()..];
    let Some(hex) = tail.get(..16) else { return Err("its checksum is cut short".to_string()) };
    if tail[16..].trim_end() != "\"\n}" {
        return Err("what follows its checksum is not the end of a save".to_string());
    }
    let want = u64::from_str_radix(hex, 16).map_err(|_| "its checksum is not a number".to_string())?;
    if fnv1a(text[..at].as_bytes()) == want {
        Ok(true)
    } else {
        Err("its checksum does not match (the file was changed or damaged)".to_string())
    }
}

/// The most strings the network may add to the world's table in a server's life (command names and building kinds that nothing in the scene says): a client chooses these and the table never gives one back.
const MAX_MINTED: u32 = 256;

/// The player whose name `args` begins with (the longest, case does not matter, followed by the end or white space) and the rest of `args`: names may have spaces in them (`mute Big Bob 600`).
fn avatar_in<'a>(game: &SimGame, args: &'a str) -> Option<(usize, &'a str)> {
    let args = args.trim();
    let mut best: Option<(usize, usize)> = None;
    for (slot, a) in &game.avatars {
        let n = a.name.len();
        let fits = args.len() >= n && args.is_char_boundary(n) && args[..n].eq_ignore_ascii_case(&a.name) && args[n..].chars().next().is_none_or(char::is_whitespace);
        if fits && best.is_none_or(|(_, len)| n > len) {
            best = Some((*slot, n));
        }
    }
    best.map(|(slot, n)| (slot, args[n..].trim()))
}

impl SimGame {
    /// What the server's world is made of, for the profile (`lunaris server --profile`): the things it tells the players about by class and kind, and the biggest groups of entities (an archetype is a set of components) by name. A world that grows
    /// with time (a streamed city, corpses, loot) shows what it grows with.
    pub fn census(&mut self) -> String {
        let mut kinds: std::collections::BTreeMap<(u32, u32), u32> = std::collections::BTreeMap::new();
        let _ = self.sim.world.for_each_uid::<(&NetEntity,), _>(|_, _, (n,)| *kinds.entry((n.class, n.kind)).or_default() += 1);
        let mut top: Vec<(usize, String)> = self.sim.world.archetypes().iter().filter(|a| !a.is_empty()).map(|a| (a.len(), a.component_ids().map(|c| self.sim.world.info(c).name.clone()).collect::<Vec<_>>().join("+"))).collect();
        top.sort_by(|a, b| b.0.cmp(&a.0));
        let kinds: Vec<String> = kinds.into_iter().filter(|(_, n)| *n >= 5).map(|((c, k), n)| format!("{c}/{k} x{n}")).collect();
        let top: Vec<String> = top.into_iter().take(8).map(|(n, names)| format!("{n} x [{}]", names.chars().take(140).collect::<String>())).collect();
        format!("things by class/kind: {} | biggest groups: {}", kinds.join(", "), top.join(" ; "))
    }

    pub fn new(sim: Sim) -> SimGame {
        SimGame { sim, host: Host::default(), avatars: BTreeMap::new(), mirror: HashMap::new(), stored: BTreeMap::new(), ticks: 0, next_save: 0, stats: SimStats::default(), hist: std::collections::VecDeque::new(), saving: None, sync: netsync::SyncOut::default(), sight: None, sight_sig: 0, sight_epoch: 0, sight_next: 0, sight_change: LevelChange::default(), parts: TickParts::default(), party: party_glue::PartyState::default(), wipe: None, minted: 0 }
    }

    /// Read the `NetHost` of the scene (once), and load the saved world if it names one that exists.
    pub(crate) fn start(&mut self) {
        if self.host.loaded {
            return;
        }
        self.read_host();
        // (the scene's own graph systems and prefabs are code: they are kept over a loaded save, so that a new version of the game reaches a world saved before it)
        let code = self.sim.scripts.clone();
        let scene_host = self.host.clone();
        // (the scene a client plays is this scene: its local player, the entity with a `NetPlayer`, is the client's own and has no place here)
        let mut stubs: Vec<u64> = Vec::new();
        let _ = self.sim.world.for_each_uid::<(&lunaris_sim::components::NetPlayer,), _>(|_, uid, _| stubs.push(uid));
        if !stubs.is_empty() {
            lunaris_sim::gameplay::despawn_trees(&mut self.sim.world, stubs);
        }
        if !self.host.save.is_empty() {
            if let Some(path) = self.save_path() {
                if let Some(json) = SimGame::read_save(&path) {
                    {
                        let saved_version = json.get("world_version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let same_layout = saved_version == self.host.world_version;
                        if !same_layout {
                            eprintln!("the saved world is of layout {saved_version} and this scene is {}: the world starts fresh from the scene, the players' characters are kept", self.host.world_version);
                        }
                        // (the saved world is tried on a copy: with the scene's own systems compiled again for its components; a save that does not fit the scene is not loaded, the world starts fresh)
                        let mut loaded: Option<Sim> = None;
                        if same_layout {
                            let mut trial = self.sim.clone();
                            if trial.load_save(&json).is_ok() {
                                trial.scripts = code.clone();
                                match trial.recompile_systems() {
                                    Ok(()) => loaded = Some(trial),
                                    Err(e) => eprintln!("the saved world does not fit this scene ({}): the world starts fresh from the scene, the players' characters are kept", e.message),
                                }
                            } else {
                                eprintln!("the saved world could not be read: the world starts fresh from the scene, the players' characters are kept");
                            }
                        }
                        if let Some(trial) = loaded {
                            self.sim = trial;
                            // (the host's own settings are the scene's too, like its systems: `forward`, `uses`, `item_kinds`, `skip`, `sync` of a new version of the game apply to a world saved before it)
                            self.host = scene_host.clone();
                        }
                        if let Some(Json::Obj(players)) = json.get("players") {
                            let now = super::server::unix_now();
                            for (name, comps) in players {
                                if let Json::Obj(c) = comps {
                                    let mut c = c.clone();
                                    // (a character saved before characters had times starts to age now: nobody is let go for a time nobody wrote down)
                                    if seen_of(&c).is_none() {
                                        stamp_seen(&mut c, now);
                                    }
                                    self.stored.insert(name.clone(), c);
                                }
                            }
                            self.expire_characters_at(now);
                        }
                    }
                }
            }
        }
        self.strip_skipped();
        self.next_save = self.sim.tick + (self.host.save_every.max(1.0) / self.sim.dt.max(1.0e-4)) as u64;
    }

    /// `NetHost.skip`: the entities that have one of the named components are not kept (with everything attached to them): the interface of a game the server has no screen for. Returns how many entities went.
    pub fn strip_skipped(&mut self) -> usize {
        if self.host.skip.is_empty() {
            return 0;
        }
        let before = self.sim.world.len();
        let world = &mut self.sim.world;
        let keep: Vec<lunaris_ecs::ComponentId> = ["NetHost", "NetEntity", "SpawnPoint"].iter().filter_map(|n| world.component_id(n)).collect();
        let mut doomed: Vec<u64> = Vec::new();
        for name in &self.host.skip {
            let Some(cid) = world.component_id(name) else { continue };
            for ai in world.matching_archetypes(&[cid], &[]) {
                for &e in world.archetypes()[ai].entities() {
                    if keep.iter().any(|&k| world.has(e, k)) {
                        continue;
                    }
                    if let Some(uid) = world.uid_of(e) {
                        doomed.push(uid);
                    }
                }
            }
        }
        doomed.sort_unstable();
        doomed.dedup();
        if !doomed.is_empty() {
            lunaris_sim::gameplay::despawn_trees(world, doomed);
        }
        before - self.sim.world.len()
    }

    fn read_host(&mut self) {
        let mut best: Option<(u64, NetHost)> = None;
        let _ = self.sim.world.for_each_uid::<(&NetHost,), _>(|_, uid, (h,)| {
            if best.as_ref().is_none_or(|b| uid < b.0) {
                best = Some((uid, *h));
            }
        });
        let mut host = Host { loaded: true, static_every: 15, save_every: 60.0, lag_comp: 250.0, interp: 100.0, ..Default::default() };
        if let Some((_, h)) = best {
            let strings = self.sim.world.strings();
            host.player = strings.get(h.player.0).to_string();
            host.save = strings.get(h.save.0).to_string();
            host.static_every = h.static_every.max(1);
            host.save_every = if h.save_every.is_finite() { h.save_every } else { 60.0 };
            host.persist = strings.get(h.persist.0).split_whitespace().map(|s| s.to_string()).collect();
            host.character_secs = if h.character_days.is_finite() && h.character_days > 0.0 { (h.character_days.min(36_500.0) as f64 * 86_400.0) as u64 } else { 0 };
            host.drop = strings.get(h.drop.0).to_string();
            host.world_version = h.world_version;
            host.uses = parse_uses(strings.get(h.uses.0));
            host.item_kinds = parse_item_kinds(strings.get(h.item_kinds.0));
            host.lag_comp = if h.lag_comp.is_finite() { h.lag_comp.clamp(0.0, 1000.0) } else { 250.0 };
            host.interp = if h.interp.is_finite() { h.interp.clamp(0.0, 1000.0) } else { 100.0 };
            host.sync = netsync::parse_names(strings.get(h.sync.0));
            host.sync_every = if h.sync_every.is_finite() { h.sync_every.clamp(0.0, 60.0) } else { 1.0 };
            host.skip = strings.get(h.skip.0).split(|c: char| c.is_whitespace() || c == ',').filter(|n| !n.is_empty()).map(|n| n.to_string()).collect();
            host.occlusion = if h.occlusion.is_finite() { h.occlusion.clamp(0.0, 10_000.0) } else { 0.0 };
            host.occlude_machines = h.occlude_machines;
            host.chat = if h.chat.is_finite() { h.chat.max(0.0) } else { 0.0 };
            host.party = h.party;
            for line in strings.get(h.forward.0).lines() {
                let mut w = line.split('#').next().unwrap_or("").split_whitespace();
                if let Some(name) = w.next() {
                    // (`NAME own`: told to the player the event is about, wherever that player is, and to nobody else: the radius is -1)
                    let word = w.next();
                    let radius = if word == Some("own") { -1.0 } else { word.and_then(|r| r.parse::<f32>().ok()).filter(|r| r.is_finite() && *r > 0.0).unwrap_or(150.0) };
                    host.forward.push((name.to_string(), radius));
                }
            }
        }
        self.party = party_glue::PartyState::new(host.party);
        self.host = host;
    }

    fn save_path(&self) -> Option<PathBuf> {
        if self.host.save.is_empty() {
            return None;
        }
        let p = PathBuf::from(&self.host.save);
        Some(match &self.sim.asset_root {
            Some(root) if p.is_relative() => root.join(p),
            _ => p,
        })
    }

    /// A copy of the world as a save takes it (without the avatars) and what the absent and the present players keep, as of now: all that has to be done on the server's own thread.
    fn save_copy(&self) -> (Sim, BTreeMap<String, Vec<(String, Json)>>) {
        let mut copy = self.sim.clone();
        let doomed: Vec<u64> = self.avatars.values().map(|a| a.uid).collect();
        lunaris_sim::gameplay::despawn_trees(&mut copy.world, doomed);
        // the avatars that are in the world now keep what is to be kept too, as of now
        let mut players = self.stored.clone();
        for a in self.avatars.values() {
            if let Some(c) = self.capture(a.uid) {
                players.insert(a.name.clone(), c);
            }
        }
        (copy, players)
    }

    /// Turn a save copy into the file: written beside it (and on to the disk), then renamed over it, so that a crash never leaves half a save; with a checksum as the last member (`check_save`), and the save that was there
    /// kept as `.bak` (a hard link: the old file stays whole under its new name while the new one is renamed over the old name, so there is no moment without a complete save) when it is itself whole. The part that
    /// takes the time, which can be on any thread.
    fn write_save(copy: Sim, players: BTreeMap<String, Vec<(String, Json)>>, path: PathBuf, world_version: u32) -> bool {
        let mut json = copy.save_json();
        drop(copy);
        json.set("world_version", Json::from(world_version as i64));
        json.set("players", Json::Obj(players.into_iter().map(|(n, c)| (n, Json::Obj(c))).collect()));
        let Ok(text) = json.to_string_pretty() else { return false };
        drop(json);
        let text = seal_save(text);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("tmp");
        let written = (|| -> std::io::Result<()> {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
            return false;
        }
        // (the save that is there: kept as the fallback if it is whole, put aside if it is not)
        if path.exists() {
            match check_save_file(&path) {
                Ok(_) => {
                    let bak = path.with_extension("bak");
                    let _ = std::fs::remove_file(&bak);
                    if std::fs::hard_link(&path, &bak).is_err() {
                        let _ = std::fs::copy(&path, &bak);
                    }
                }
                Err(why) => {
                    let aside = path.with_extension(format!("corrupt-{}.json", super::server::unix_now()));
                    eprintln!("the world save {} is damaged ({why}): kept as {}, the save before it is not touched", path.display(), aside.display());
                    let _ = std::fs::rename(&path, &aside);
                }
            }
        }
        let ok = std::fs::rename(&tmp, &path).is_ok();
        if ok {
            // (the rename itself on to the disk, where a directory can be opened)
            if let Some(dir) = path.parent() {
                let _ = std::fs::File::open(dir).and_then(|d| d.sync_all());
            }
        }
        ok
    }

    /// The saved world in `path`, or in `path.bak` (the save before it) when that one is damaged: text that is not text, a file cut short or changed (its checksum), JSON that does not parse. A damaged file is kept
    /// aside (`.corrupt-UNIXSECOND.json`, never overwritten and never read again), the server says what it found on its standard error, and starts from the save before it or, if there is none, fresh.
    /// `None`: no save, or none that could be used.
    fn read_save(path: &std::path::Path) -> Option<Json> {
        let mut damaged = false;
        for (label, file) in [("the world save", path.to_path_buf()), ("the save before it", path.with_extension("bak"))] {
            if !file.exists() {
                continue;
            }
            let loaded = std::fs::read(&file).map_err(|e| e.to_string()).and_then(|b| String::from_utf8(b).map_err(|_| "it is not text".to_string())).and_then(|text| {
                let sum = check_save(&text)?;
                lunaris_json::parse(&text).map(|j| (j, sum)).map_err(|e| format!("it does not parse: {}", e.message))
            });
            match loaded {
                Ok((json, sum)) => {
                    if damaged {
                        eprintln!("{label} {} is used instead: the world is as of the save before the damaged one", file.display());
                    } else if !sum {
                        eprintln!("{label} {} has no checksum (written by an older server): read as it is", file.display());
                    }
                    return Some(json);
                }
                Err(why) => {
                    damaged = true;
                    let aside = path.with_extension(format!("corrupt-{}.json", super::server::unix_now()));
                    if file == path {
                        let _ = std::fs::rename(&file, &aside);
                        eprintln!("{label} {} is damaged ({why}): kept as {}", file.display(), aside.display());
                    } else {
                        eprintln!("{label} {} is damaged too ({why}): it is left where it is", file.display());
                    }
                }
            }
        }
        if damaged {
            eprintln!("there is no usable save: the world starts fresh from the scene");
        }
        None
    }

    /// Write the world (without the avatars) and what the absent players keep, now, and wait for it (a save that is being written in the background is waited for first). Returns false if it could not be written.
    pub fn save_now(&mut self) -> bool {
        self.finish_saving(true);
        let Some(path) = self.save_path() else { return false };
        // (after `wipe all` nothing is written; after `wipe world` the save says it is of no layout any scene has)
        if self.wipe == Some(true) {
            return false;
        }
        let version = if self.wipe.is_some() { WIPED_LAYOUT } else { self.host.world_version };
        let t = Instant::now();
        self.expire_characters_at(super::server::unix_now());
        let (copy, players) = self.save_copy();
        let ok = SimGame::write_save(copy, players, path, version);
        if ok {
            self.stats.saves += 1;
        } else {
            self.stats.saves_failed += 1;
            eprintln!("the world could not be written to {}: the save before it is still there", self.host.save);
        }
        self.stats.last_save_ms = t.elapsed().as_millis() as u64;
        ok
    }

    /// Start a save that is written on a thread of its own: the server's thread only copies the world (two and a half milliseconds for a hundred thousand entities, where writing it takes a third of a second).
    /// One at a time: if the last is not written yet, nothing starts and this says false.
    pub fn save_in_background(&mut self) -> bool {
        self.finish_saving(false);
        if self.saving.is_some() || self.wipe == Some(true) {
            return false;
        }
        let Some(path) = self.save_path() else { return false };
        self.expire_characters_at(super::server::unix_now());
        let (copy, players) = self.save_copy();
        let version = if self.wipe.is_some() { WIPED_LAYOUT } else { self.host.world_version };
        match std::thread::Builder::new().name("lunaris-save".to_string()).spawn(move || SimGame::write_save(copy, players, path, version)) {
            Ok(h) => {
                self.saving = Some((h, Instant::now()));
                true
            }
            Err(_) => false,
        }
    }

    /// Whether a save is being written in the background.
    pub fn saving(&self) -> bool {
        self.saving.is_some()
    }

    /// Take in a background save that has finished (the server does this every tick).
    pub fn poll_save(&mut self) {
        self.finish_saving(false);
    }

    /// Take in a background save that has finished (with `wait`, wait for it).
    fn finish_saving(&mut self, wait: bool) {
        if let Some((h, started)) = self.saving.take() {
            if wait || h.is_finished() {
                match h.join() {
                    Ok(true) => self.stats.saves += 1,
                    Ok(false) => {
                        self.stats.saves_failed += 1;
                        eprintln!("the world could not be written to {}: the save before it is still there", self.host.save);
                    }
                    Err(_) => {
                        self.stats.saves_failed += 1;
                        eprintln!("the thread that writes the world panicked: the save before it is still there");
                    }
                }
                self.stats.last_save_ms = started.elapsed().as_millis() as u64;
            } else {
                self.saving = Some((h, started));
            }
        }
    }

    /// The components of an avatar that are kept for its player.
    fn capture(&self, uid: u64) -> Option<Vec<(String, Json)>> {
        if self.host.persist.is_empty() {
            return None;
        }
        let e = self.sim.world.entity(uid)?;
        let mut out = Vec::new();
        for name in &self.host.persist {
            if let Some(cid) = self.sim.world.component_id(name) {
                if let Some(j) = self.sim.world.get_json(e, cid) {
                    out.push((name.clone(), j));
                }
            }
        }
        // (when the player was last here: now; the character is let go `NetHost.character_days` after the last time it left)
        stamp_seen(&mut out, super::server::unix_now());
        Some(out)
    }

    /// Let go the stored characters whose players have not been here for `NetHost.character_days` days as of `now` (unix seconds). Returns how many went. (A character that has no time is given one: it ages from now.)
    pub(crate) fn expire_characters_at(&mut self, now: u64) -> usize {
        let secs = self.host.character_secs;
        if secs == 0 {
            return 0;
        }
        let before = self.stored.len();
        self.stored.retain(|_, c| seen_of(c).is_none_or(|t| now.saturating_sub(t) <= secs));
        before - self.stored.len()
    }

    fn others(&self) -> Vec<V3> {
        self.avatars.values().filter_map(|a| self.sim.world.entity(a.uid).and_then(|e| self.sim.world.get::<Transform>(e)).map(|t| t.pos)).collect()
    }

    /// Make the avatar of a player: from the scene's prefab (or a bare body without one), at `pos`.
    fn make_avatar(&mut self, slot: usize, name: &str, pos: V3, rot: [f32; 4]) -> Option<(u64, u64)> {
        let num = |v: f32| Json::Float(v as f64);
        let base = self.sim.world.next_uid();
        let mut ov: Vec<(String, Json)> = Vec::new();
        lunaris_sim::prefab::merge_component(&mut ov, "Transform", &Json::obj([("pos", Json::arr(pos.map(num))), ("rot", Json::arr(rot.map(num)))]));
        lunaris_sim::prefab::merge_component(&mut ov, "NetAvatar", &Json::obj([("slot", Json::from(slot as i64)), ("name", Json::str(name))]));
        // (the world is streamed round every player: a server has no camera, its avatars are what streaming works from)
        lunaris_sim::prefab::merge_component(&mut ov, "StreamAnchor", &Json::Obj(Vec::new()));
        let mut count = 1u64;
        if !self.host.player.is_empty() && self.sim.scripts.prefabs.contains_key(&self.host.player) {
            let plan = lunaris_sim::prefab::plan(&self.sim.scripts.prefabs, &self.host.player, &ov).ok()?;
            count = plan.len() as u64;
            let cmds = lunaris_sim::prefab::spawn_commands(&plan, base);
            lunaris_sim::apply_batch(&mut self.sim.world, &mut self.sim.scripts, &cmds).ok()?;
        } else {
            // a body with nothing but what the server needs: it can be shot and it is a capsule
            let mut comps = ov;
            lunaris_sim::prefab::merge_component(&mut comps, "Name", &Json::obj([("value", Json::str(format!("player {name}")))]));
            lunaris_sim::prefab::merge_component(&mut comps, "HitPoints", &Json::obj([("value", num(100.0)), ("max", num(100.0))]));
            lunaris_sim::prefab::merge_component(&mut comps, "Collider", &Json::obj([("shape", Json::from(2i64)), ("size", Json::arr([0.7, 1.8, 0.7].map(num)))]));
            lunaris_sim::apply_batch(&mut self.sim.world, &mut self.sim.scripts, &[lunaris_sim::Command::Spawn { uid: Some(base), components: comps }]).ok()?;
        }
        // the child that carries the tools
        let mut head = 0u64;
        for k in 1..count {
            let Some(e) = self.sim.world.entity(base + k) else { continue };
            let child_of_avatar = self.sim.world.get::<Parent>(e).is_some_and(|p| p.target.0 == base);
            if child_of_avatar && (self.sim.world.get::<Weapon>(e).is_some() || self.sim.world.get::<BuildTool>(e).is_some()) {
                head = base + k;
                break;
            }
        }
        if let Some(e) = self.sim.world.entity(base) {
            if let Some(n) = self.sim.world.get_mut::<NetAvatar>(e) {
                n.head = Uid(head);
            }
        }
        Some((base, head))
    }

    /// What the player reports, onto its avatar.
    fn apply_input(&mut self, uid: u64, head_uid: u64, st: &EntityState) {
        let Some(e) = self.sim.world.entity(uid) else { return };
        let (pos, yaw, pitch) = (st.pos_m(), st.yaw_rad(), st.pitch_rad());
        let head = if head_uid != 0 { self.sim.world.entity(head_uid) } else { None };
        // (a player in a vehicle is where the seat is: what its computer says of its place is not used)
        let seated = self.sim.world.get::<Seated>(e).is_some();
        if let Some(t) = self.sim.world.get_mut::<Transform>(e) {
            if !seated {
                t.pos = pos;
            }
            // (with a head the body only turns; without one the whole avatar looks, so that its tools aim)
            t.rot = if head.is_some() { yaw_rot(yaw) } else { quat_mul(yaw_rot(yaw), quat_x(pitch)) };
        }
        if !seated {
            if let Some(v) = self.sim.world.get_mut::<Velocity>(e) {
                v.lin = st.vel_ms();
            }
        }
        if let Some(h) = head {
            if let Some(l) = self.sim.world.get_mut::<LocalTransform>(h) {
                l.rot = quat_x(pitch);
            }
        }
        let na = self.sim.world.get::<NetAvatar>(e).copied();
        let Some(na) = na else { return };
        if let Some(n) = self.sim.world.get_mut::<NetAvatar>(e) {
            n.buttons = st.flags as u32;
            n.pitch = pitch;
            n.held = st.extra as u32;
        }
        let tool_e = head.unwrap_or(e);
        let buttons = st.flags as u32;
        // (the goggles: the hack key, and the way the player looks, which is where they aim; while a machine is controlled the buttons are the machine's, not the player's gun's)
        let mut controlling = false;
        if let Some(hk) = self.sim.world.get_mut::<Hacker>(e) {
            hk.hack = buttons & na.hack_mask != 0 && na.hack_mask != 0;
            let (sy, cy) = yaw.sin_cos();
            let (sp, cp) = pitch.sin_cos();
            hk.look = [-sy * cp, sp, -cy * cp];
            controlling = hk.state == 2;
        }
        if let Some(w) = self.sim.world.get_mut::<Weapon>(tool_e) {
            w.fire = buttons & na.fire_mask != 0 && na.fire_mask != 0 && !controlling;
            if buttons & na.reload_mask != 0 && na.reload_mask != 0 && !controlling {
                w.reload = true;
            }
        }
        if let Some(v) = self.sim.world.get_mut::<Vitals>(e) {
            v.sprint = buttons & na.sprint_mask != 0 && na.sprint_mask != 0;
        }
        if let Some(st) = self.sim.world.get_mut::<lunaris_sim::components::Stealth>(e) {
            st.crouch = buttons & na.crouch_mask != 0 && na.crouch_mask != 0;
        }
        if let Some(i) = self.sim.world.get_mut::<Interactor>(tool_e) {
            i.act = buttons & na.use_mask != 0 && na.use_mask != 0;
        }
        // (the slot of the bag the player says is in its hand, in the low bits of what it reports: what the `Weapon` is, with a `Loadout`, docs/LOADOUT.md)
        if let Some(l) = self.sim.world.get_mut::<Loadout>(tool_e) {
            l.held = st.extra as u32 & HELD_SLOT;
            // (aiming down the sights: the player's aim button, which narrows the gun's scatter and kick by its `aim` and `aim_recoil`)
            l.aiming = buttons & na.aim_mask != 0 && na.aim_mask != 0 && !controlling;
        }
    }

    /// The `kind` the host gives the item that the pickup `e` holds (`NetHost.item_kinds`); `None` for anything that is not a pickup or an item with no line.
    fn item_kind(&self, e: Entity) -> Option<u32> {
        let w = &self.sim.world;
        let i = w.get::<Interactable>(e).filter(|i| i.kind == 3 && i.item.0 != 0)?;
        let key = w.strings().get(i.item.0).split_whitespace().next()?;
        self.host.item_kinds.iter().find(|(k, _)| k == key).map(|(_, kind)| *kind)
    }

    /// The simulation's things that the players are told about, into the server's world.
    fn mirror(&mut self, world: &mut NetWorld, scan_static: bool, frame: &mut Vec<(u64, V3)>) {
        let mut list: Vec<(u64, Entity, NetEntity, Transform)> = Vec::new();
        let _ = self.sim.world.for_each_uid::<(&NetEntity, &Transform), _>(|e, uid, (n, t)| {
            if t.pos.iter().all(|v| v.is_finite()) {
                list.push((uid, e, *n, *t));
            }
        });
        let mut seen: HashSet<u64> = HashSet::with_capacity(list.len());
        let mut assigned: Vec<(Entity, u32)> = Vec::new();
        for (uid, e, n, t) in list {
            let cls = n.class.min(127) as u8;
            // (the moving things that can be shot are remembered for lag compensation)
            if !is_static(cls) && self.sim.world.get::<HitPoints>(e).is_some() {
                frame.push((uid, t.pos));
            }
            if is_static(cls) && !scan_static && self.mirror.contains_key(&uid) {
                seen.insert(uid);
                continue;
            }
            // (a door is told as open in bit 0 of its flags, a container as open in bit 1: the clients' copies swing and open by themselves)
            let mut state_bits = (n.flags.min(0xffff)) as u16;
            if self.sim.world.get::<Door>(e).is_some_and(|d| d.open) {
                state_bits |= 1;
            }
            if self.sim.world.get::<Interactable>(e).is_some_and(|i| i.open) {
                state_bits |= 2;
            }
            // (a pickup is told with the picture of the item it holds, if the host has one for it: `NetHost.item_kinds`)
            let kind = if is_static(cls) && !self.host.item_kinds.is_empty() { self.item_kind(e).unwrap_or(n.kind) } else { n.kind };
            let mut s = EntityState { class: cls, flags: state_bits, anim: n.anim.min(0xffff) as u16, extra: kind.min(0xffff) as u16, health: health_byte(&self.sim.world, e), ..Default::default() };
            s.set_pos_m(t.pos);
            s.set_yaw_rad(lunaris_sim::building::yaw_of(t.rot) * (std::f32::consts::PI / 180.0));
            s.set_anim_time(n.anim_t.max(0.0));
            seen.insert(uid);
            match self.mirror.get(&uid) {
                Some(&id) => {
                    if let Some(en) = world.get_mut(id) {
                        let keep = en.state.id;
                        en.state = s;
                        en.state.id = keep;
                    }
                }
                None => {
                    let id = world.spawn_state(s);
                    self.mirror.insert(uid, id);
                    assigned.push((e, id));
                }
            }
        }
        // what has gone: the moving things at once, the still ones when they have all been looked at
        let gone: Vec<(u64, u32)> = self
            .mirror
            .iter()
            .filter(|(u, _)| !seen.contains(u))
            .filter(|(_, id)| scan_static || world.get(**id).is_none_or(|en| !is_static(en.state.class)))
            .map(|(u, id)| (*u, *id))
            .collect();
        for (u, id) in gone {
            world.remove(id);
            self.mirror.remove(&u);
        }
        for (e, id) in assigned {
            if let Some(n) = self.sim.world.get_mut::<NetEntity>(e) {
                n.net = id;
            }
        }
    }
}

impl SimGame {
    /// A player's chat message (`NetHost.chat`): cleaned, limited to a burst of three and then one every 1.2 s, and told to the players within the chat distance of where the speaker stands.
    fn say(&mut self, ctx: &mut Ctx, slot: usize, body: &[u8]) {
        // (the party's channel and the commands first: docs/PARTIES.md)
        if self.party_say(ctx, slot, body) {
            return;
        }
        if !(self.host.chat > 0.0) {
            return;
        }
        let Some(av) = self.avatars.get_mut(&slot) else { return };
        if ctx.tick < av.muted_until {
            ctx.out.to(slot, chat("SERVER", "You are muted"));
            return;
        }
        let since = ctx.tick.wrapping_sub(av.chat_tick) as f32 / ctx.tick_hz.max(1) as f32;
        av.chat_credit = (av.chat_credit + since / 1.2).min(3.0);
        av.chat_tick = ctx.tick;
        if av.chat_credit < 1.0 {
            return;
        }
        let text = clean_chat(body);
        if text.is_empty() {
            return;
        }
        av.chat_credit -= 1.0;
        // (the name is what the player joined with: cleaned like the text, so that a name with a line break in it cannot make a second line that looks like somebody else's)
        let name: String = clean_chat(av.name.as_bytes()).chars().take(24).collect();
        let name = if name.is_empty() { "player".to_string() } else { name };
        let uid = av.uid;
        let pos = self.sim.world.entity(uid).and_then(|e| self.sim.world.get::<Transform>(e).map(|t| t.pos)).unwrap_or([0.0; 3]);
        ctx.out.near(pos, self.host.chat, chat(&name, &text));
    }

    /// The symbol for the name of a command or a building kind that a client sent: the string the scene already has, or (for the first [`MAX_MINTED`] such names, if it is a plain short word) a new one; `None` for the rest
    /// (a name a client invents is not a command of the game: no system of the scene could be waiting for it).
    fn sym_for(&mut self, text: &str) -> Option<Sym> {
        if text.len() > 40 || !text.bytes().all(|b| b.is_ascii_graphic()) {
            return None;
        }
        if let Some(id) = self.sim.world.strings().find(text) {
            return Some(Sym(id));
        }
        if self.minted >= MAX_MINTED {
            return None;
        }
        self.minted += 1;
        Some(Sym(self.sim.world.strings_mut().intern(text)))
    }

    /// An empty frame for `tick`: the oldest one's memory again when the history is as long as it is kept.
    fn take_frame(&mut self, tick: u32) -> Frame {
        let keep = ((self.host.lag_comp / 1000.0 * 60.0).ceil() as usize + 3).max(4);
        let mut f = if self.hist.len() >= keep { self.hist.pop_front().unwrap_or_default() } else { Frame::default() };
        f.tick = tick;
        f.at.clear();
        f
    }

    /// Put a finished frame at the end of the history (and drop the old ones when `lag_comp` has been made smaller).
    fn keep_frame(&mut self, mut f: Frame, tick_hz: u32) {
        if self.host.lag_comp <= 0.0 {
            self.hist.clear();
            return;
        }
        f.at.sort_unstable_by_key(|e| e.0);
        let keep = (self.host.lag_comp / 1000.0 * tick_hz.max(1) as f32).ceil() as usize + 3;
        self.hist.push_back(f);
        while self.hist.len() > keep.max(4) {
            self.hist.pop_front();
        }
    }

    /// Lag compensation (docs/LAG_COMPENSATION.md): for each avatar that holds its trigger, where the characters and moving things were when that player pulled it on its own screen, which was its half round
    /// trip, one tick and the interpolation delay of the clients ago, given to the weapons as the shooter's `Rewind`. Only what is within the weapon's range (and a few metres) of the shooter is listed.
    fn rewind_shooters(&mut self, ctx: &Ctx) {
        self.sim.scripts.rewind.clear();
        if self.host.lag_comp <= 0.0 || self.hist.len() < 2 {
            return;
        }
        let hz = ctx.tick_hz.max(1) as f32;
        let (oldest, newest) = (self.hist.front().map_or(0, |f| f.tick), self.hist.back().map_or(0, |f| f.tick));
        let mut view: Vec<(u64, V3)> = Vec::new();
        for (slot, av) in &self.avatars {
            let tool = if av.head != 0 { av.head } else { av.uid };
            let Some(range) = self.sim.world.entity(tool).and_then(|e| self.sim.world.get::<Weapon>(e)).filter(|w| w.fire).map(|w| w.range) else { continue };
            let rtt = ctx.world.rtt_ms.get(*slot).copied().unwrap_or(0.0);
            // (half the round trip, the clients' interpolation, and one tick: on average half a tick for the client's next send and half a tick for the server's next tick)
            let back = (rtt * 0.5 + self.host.interp + 1000.0 / hz).clamp(0.0, self.host.lag_comp) * hz / 1000.0;
            // (the world as it was published `back` ticks ago: the newest frame is the present, the tick before this one)
            let tv = ((ctx.tick as f32 - back).max(oldest as f32)).min(newest as f32);
            if tv >= newest as f32 {
                continue;
            }
            let i = ((tv.floor() as u32).saturating_sub(oldest) as usize).min(self.hist.len() - 2);
            let (a, b) = (&self.hist[i], &self.hist[i + 1]);
            let k = ((tv - a.tick as f32) / (b.tick - a.tick).max(1) as f32).clamp(0.0, 1.0);
            let reach = range.max(0.0) + 5.0;
            view.clear();
            let (mut ia, mut ib) = (0, 0);
            while ia < a.at.len() && ib < b.at.len() {
                let (ua, ub) = (a.at[ia].0, b.at[ib].0);
                if ua < ub {
                    ia += 1;
                } else if ub < ua {
                    ib += 1;
                } else {
                    let (pa, pb) = (a.at[ia].1, b.at[ib].1);
                    let p = [pa[0] + (pb[0] - pa[0]) * k, pa[1] + (pb[1] - pa[1]) * k, pa[2] + (pb[2] - pa[2]) * k];
                    let d2 = (0..3).map(|c| (p[c] - av.set[c]).powi(2)).sum::<f32>();
                    if ua != av.uid && d2 <= reach * reach {
                        view.push((ua, p));
                    }
                    ia += 1;
                    ib += 1;
                }
            }
            if !view.is_empty() {
                self.sim.scripts.rewind.shooters.insert(av.uid, view.clone());
            }
        }
    }

    /// The container the avatar has open: its entity, if the avatar still has it open.
    /// The static world for the wallhack defence's rays (`Game::blocked`): gathered when `NetHost.occlusion` is on, again every half second (a wall built, a wall shot down and a door that swings
    /// are in it by then) and no oftener than costs a hundredth of the time that passes, so that a big level is looked at less often. `Game::level_version` goes up when what was gathered is not what was.
    fn refresh_sight(&mut self, hz: u32) {
        if self.host.occlusion <= 0.0 {
            self.sight = None;
            return;
        }
        if self.sight.is_some() && self.ticks < self.sight_next {
            return;
        }
        let t = Instant::now();
        let sight = Sight::gather(&mut self.sim);
        let us = t.elapsed().as_micros() as u64;
        if self.sight.is_none() || sight.signature() != self.sight_sig {
            // (where the world changed: the answers of the pairs whose lines do not go through it stand)
            let regions = match self.sight.as_ref().map(|old| sight.changed_since(old)) {
                Some(Changed::Boxes(boxes)) => boxes,
                _ => Vec::new(),
            };
            self.sight_change = LevelChange { from: self.sight_epoch, regions };
            self.sight_sig = sight.signature();
            self.sight_epoch = self.sight_epoch.wrapping_add(1);
            self.stats.sight_changes += 1;
        }
        self.parts.sight = us;
        self.stats.sight_builds += 1;
        self.stats.sight_us += us;
        self.stats.sight_shapes = sight.shapes();
        self.sight = Some(sight);
        let tick_us = 1_000_000 / hz.max(1) as u64;
        self.sight_next = self.ticks + (hz.max(2) as u64 / 2).max(us / (tick_us / 100).max(1));
    }

    fn open_container(&self, uid: u64, head: u64) -> Option<Entity> {
        let e = self.sim.world.entity(uid)?;
        let tool = if head != 0 { self.sim.world.entity(head) } else { Some(e) }.unwrap_or(e);
        let open = self.sim.world.get::<Interactor>(tool)?.open.0;
        let ce = self.sim.world.entity(open).filter(|c| self.sim.world.get::<Inventory>(*c).is_some())?;
        // (the user of a container is the entity that has the `Interactor`: the head, or the avatar itself without one)
        let actor = if head != 0 { head } else { uid };
        self.sim.world.get::<Interactable>(ce).is_some_and(|i| i.open && i.user.0 == actor).then_some(ce)
    }

    /// `inv_move from to count`: a place is 100 times the bag (0 the player's own, 1 the container it has open) plus the slot.
    fn inv_move(&mut self, slot: usize, args: [f32; 4]) {
        let Some(av) = self.avatars.get_mut(&slot) else { return };
        // (what the player is told next is the truth, even when the move is refused: its screen moved the stack at once, and a refusal changes nothing that would be sent)
        av.told_bag = 0;
        av.told_container = 0;
        let (uid, head) = (av.uid, av.head);
        let place = |v: f32| (v.max(0.0) as usize / 100, v.max(0.0) as usize % 100);
        let ((ia, ka), (ib, kb)) = (place(args[0]), place(args[1]));
        let Some(me) = self.sim.world.entity(uid) else { return };
        let container = self.open_container(uid, head);
        let pick = |i: usize| if i == 0 { Some(me) } else if i == 1 { container } else { None };
        let (Some(ea), Some(eb)) = (pick(ia), pick(ib)) else { return };
        let Some((table, _)) = table_of(&mut self.sim.world) else { return };
        let count = args[2].max(0.0) as u32;
        // the worn places of a bag take only their class (docs/EQUIPMENT.md): what would end up in one, by this move or by the swap it makes, must fit it
        let id_at = |w: &lunaris_ecs::World, e: Entity, k: usize| w.get::<Inventory>(e).map_or(0, |i| if k < i.id.len() && i.count[k] > 0 { i.id[k] } else { 0 });
        let (ida, idb) = (id_at(&self.sim.world, ea, ka), id_at(&self.sim.world, eb, kb));
        if !lunaris_sim::armor::allows(&self.sim.world, eb, &table, kb, ida) || (idb != 0 && idb != ida && !lunaris_sim::armor::allows(&self.sim.world, ea, &table, ka, idb)) {
            return;
        }
        if ea == eb {
            let Some(mut inv) = self.sim.world.get::<Inventory>(ea).copied() else { return };
            if netinv::move_items(&mut inv, None, &table, ka, kb, count) {
                if let Some(w) = self.sim.world.get_mut::<Inventory>(ea) {
                    *w = inv;
                }
            }
        } else {
            let (Some(mut a), Some(mut b)) = (self.sim.world.get::<Inventory>(ea).copied(), self.sim.world.get::<Inventory>(eb).copied()) else { return };
            if netinv::move_items(&mut a, Some(&mut b), &table, ka, kb, count) {
                if let Some(w) = self.sim.world.get_mut::<Inventory>(ea) {
                    *w = a;
                }
                if let Some(w) = self.sim.world.get_mut::<Inventory>(eb) {
                    *w = b;
                }
            }
        }
    }

    /// `inv_use slot`: the item in that slot of the bag is used (`NetHost.uses`): what it does is asked of the avatar's `Vitals` and one of it is used up.
    fn inv_use(&mut self, slot: usize, args: [f32; 4]) {
        let Some(av) = self.avatars.get_mut(&slot) else { return };
        av.told_bag = 0;
        let uid = av.uid;
        let Some(me) = self.sim.world.entity(uid) else { return };
        let k = args[0].max(0.0) as usize;
        let Some(mut inv) = self.sim.world.get::<Inventory>(me).copied() else { return };
        if k >= (inv.size as usize).min(48) || inv.id[k] == 0 || inv.count[k] == 0 {
            return;
        }
        let Some((table, _)) = table_of(&mut self.sim.world) else { return };
        let Some(key) = table.item(inv.id[k]).map(|i| i.key.clone()) else { return };
        let Some(rule) = self.host.uses.iter().find(|u| u.key == key).cloned() else { return };
        // (what it does: a body that is dead eats nothing)
        if self.sim.world.get::<HitPoints>(me).is_some_and(|h| h.dead || h.value <= 0.0) {
            return;
        }
        if let Some(v) = self.sim.world.get_mut::<Vitals>(me) {
            v.eat += rule.eat;
            v.drink += rule.drink;
            v.heal += rule.heal;
            v.bandage |= rule.bandage;
        }
        if !rule.keep {
            inv.count[k] -= 1;
            if inv.count[k] == 0 {
                inv.id[k] = 0;
                inv.dur[k] = 0.0;
            }
            if let Some(w) = self.sim.world.get_mut::<Inventory>(me) {
                *w = inv;
            }
        }
    }

    /// `inv_drop slot count`: some of what the player carries is put on the ground in front of it, as the scene's `NetHost.drop` prefab.
    fn inv_drop(&mut self, slot: usize, args: [f32; 4]) {
        let Some(av) = self.avatars.get_mut(&slot) else { return };
        av.told_bag = 0;
        let uid = av.uid;
        let Some(me) = self.sim.world.entity(uid) else { return };
        if self.host.drop.is_empty() || !self.sim.scripts.prefabs.contains_key(&self.host.drop) {
            return;
        }
        let k = args[0].max(0.0) as usize;
        let Some(mut inv) = self.sim.world.get::<Inventory>(me).copied() else { return };
        if k >= (inv.size as usize).min(48) || inv.id[k] == 0 || inv.count[k] == 0 {
            return;
        }
        let Some((table, _)) = table_of(&mut self.sim.world) else { return };
        let Some(key) = table.item(inv.id[k]).map(|i| i.key.clone()) else { return };
        let want = args[1].max(0.0) as u32;
        let n = if want == 0 || want >= inv.count[k] { inv.count[k] } else { want };
        inv.count[k] -= n;
        if inv.count[k] == 0 {
            inv.id[k] = 0;
            inv.dur[k] = 0.0;
        }
        if let Some(w) = self.sim.world.get_mut::<Inventory>(me) {
            *w = inv;
        }
        // in front of the player, a little above its feet, turned the way the player looks (a gun model lies pointing where it was dropped)
        let t = self.sim.world.get::<Transform>(me).copied().unwrap_or_default();
        let degrees = lunaris_sim::building::yaw_of(t.rot);
        let yaw = degrees * (std::f32::consts::PI / 180.0);
        let at = [t.pos[0] - yaw.sin(), t.pos[1] + 0.3, t.pos[2] - yaw.cos()];
        let turn = lunaris_sim::building::yaw_quat(degrees);
        let num = |v: f32| Json::Float(v as f64);
        let mut ov: Vec<(String, Json)> = Vec::new();
        lunaris_sim::prefab::merge_component(&mut ov, "Transform", &Json::obj([("pos", Json::arr(at.map(num))), ("rot", Json::arr(turn.map(num)))]));
        lunaris_sim::prefab::merge_component(&mut ov, "Interactable", &Json::obj([("kind", Json::from(3i64)), ("item", Json::str(format!("{key} {n}")))]));
        if let Ok(plan) = lunaris_sim::prefab::plan(&self.sim.scripts.prefabs, &self.host.drop, &ov) {
            let base = self.sim.world.next_uid();
            let cmds = lunaris_sim::prefab::spawn_commands(&plan, base);
            let _ = lunaris_sim::apply_batch(&mut self.sim.world, &mut self.sim.scripts, &cmds);
        }
    }

    /// Tell each player what is in its bag, and in the container it has open, when that has changed.
    fn tell_inventories(&mut self, ctx: &mut Ctx) {
        let slots: Vec<usize> = self.avatars.keys().copied().collect();
        for slot in slots {
            let (uid, head) = match self.avatars.get(&slot) {
                Some(a) => (a.uid, a.head),
                None => continue,
            };
            let Some(e) = self.sim.world.entity(uid) else { continue };
            if let Some(inv) = self.sim.world.get::<Inventory>(e) {
                let h = netinv::hash(inv);
                if self.avatars[&slot].told_bag != h {
                    ctx.out.to(slot, netinv::inv_message(inv));
                    if let Some(a) = self.avatars.get_mut(&slot) {
                        a.told_bag = h;
                    }
                }
            }
            let ce = self.open_container(uid, head);
            let told = self.avatars[&slot].told_container;
            match ce {
                Some(ce) => {
                    if let Some(ci) = self.sim.world.get::<Inventory>(ce) {
                        let cuid = self.sim.world.uid_of(ce).unwrap_or(0);
                        let h = netinv::hash(ci) ^ cuid.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
                        if told != h {
                            let source = self.mirror.get(&cuid).copied().unwrap_or(0);
                            ctx.out.to(slot, netinv::cont_message(source, ci));
                            if let Some(a) = self.avatars.get_mut(&slot) {
                                a.told_container = h;
                            }
                        }
                    }
                }
                None => {
                    if told != 0 {
                        ctx.out.to(slot, netinv::cont_end());
                        if let Some(a) = self.avatars.get_mut(&slot) {
                            a.told_container = 0;
                        }
                    }
                }
            }
        }
    }
}

/// How many names a game server keeps a character for (`NetHost.persist`): beyond it, a name that was never kept is not kept.
const MAX_STORED: usize = 20_000;

/// The member of a stored character that says when its player was last here (unix seconds): not a component, a stored character is a list of components and this.
pub(crate) const SEEN: &str = "_seen";

pub(crate) fn seen_of(comps: &[(String, Json)]) -> Option<u64> {
    comps.iter().find(|(n, _)| n == SEEN).and_then(|(_, j)| match j {
        Json::Int(i) => u64::try_from(*i).ok(),
        Json::Float(f) if *f >= 0.0 => Some(*f as u64),
        _ => None,
    })
}

fn stamp_seen(comps: &mut Vec<(String, Json)>, now: u64) {
    comps.retain(|(n, _)| n != SEEN);
    comps.push((SEEN.to_string(), Json::Int(now as i64)));
}

impl Game for SimGame {
    fn on_join(&mut self, _world: &mut NetWorld, slot: usize, name: &str) -> EntityState {
        self.start();
        let others = self.others();
        let salt = (slot as u64).wrapping_mul(0x9E37_79B9) ^ name.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64));
        let place = lunaris_sim::respawn::spawn_place(&mut self.sim.world, &others, 0, 80.0, salt, self.sim.tick);
        let (mut pos, rot) = place.unwrap_or(([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]));
        let mut state = EntityState { class: class::PLAYER, health: 255, ..Default::default() };
        if let Some((uid, head)) = self.make_avatar(slot, name, pos, rot) {
            // what the player kept from the last time
            if let (Some(comps), Some(e)) = (self.stored.remove(name), self.sim.world.entity(uid)) {
                // (the bag's shape is the game's, not the player's: a character kept before the game gave its bags more places, worn ones for instance, gets the new shape with its old things)
                let shape = self.sim.world.get::<Inventory>(e).map(|i| (i.size, i.bag));
                for (cname, j) in comps {
                    if let Some(cid) = self.sim.world.component_id(&cname) {
                        let _ = self.sim.world.set_json(e, cid, &j);
                    }
                }
                if let (Some((size, bag)), Some(inv)) = (shape, self.sim.world.get_mut::<Inventory>(e)) {
                    inv.size = inv.size.max(size);
                    inv.bag = bag.min(inv.size);
                }
                if let Some(t) = self.sim.world.get::<Transform>(e) {
                    pos = t.pos;
                }
            }
            if let Some(e) = self.sim.world.entity(uid) {
                state.health = health_byte(&self.sim.world, e);
            }
            self.avatars.insert(slot, Avatar { uid, net: 0, name: name.to_string(), head, set: pos, told_bag: 0, told_container: 0, axes: [0.0; 4], axes_tick: 0, told_ride: 0, told_view: 0, told_status: None, status_tick: 0, told_sync: false, chat_credit: 3.0, chat_tick: 0, muted_until: 0 });
            self.party_join(slot, name);
        }
        state.set_pos_m(pos);
        state.set_yaw_rad(lunaris_sim::building::yaw_of(rot) * (std::f32::consts::PI / 180.0));
        state
    }

    fn on_leave(&mut self, _world: &mut NetWorld, slot: usize) {
        self.party_leave(slot);
        if let Some(av) = self.avatars.remove(&slot) {
            if let Some(c) = self.capture(av.uid) {
                // (a name that was never kept before is kept only while there is room: a plain server cannot be filled up with names, and its save file cannot grow without end)
                if self.stored.len() < MAX_STORED || self.stored.contains_key(&av.name) {
                    self.stored.insert(av.name.clone(), c);
                }
            }
            lunaris_sim::gameplay::despawn_trees(&mut self.sim.world, vec![av.uid]);
        }
    }

    fn on_command(&mut self, ctx: &mut Ctx, slot: usize, data: &[u8]) {
        if data.first() == Some(&msg::SAY) {
            self.say(ctx, slot, &data[1..]);
            return;
        }
        if let Some(axes) = parse_control(data) {
            if let Some(av) = self.avatars.get_mut(&slot) {
                av.axes = axes;
                av.axes_tick = ctx.tick;
            }
            return;
        }
        let Some((name, args)) = parse_action(data) else { return };
        let Some(av) = self.avatars.get(&slot) else { return };
        let Some(e) = self.sim.world.entity(av.uid) else { return };
        let tool = if av.head != 0 { self.sim.world.entity(av.head) } else { Some(e) }.unwrap_or(e);
        if let Some(kind) = name.strip_prefix("build:") {
            let Some(sym) = self.sym_for(kind) else { return };
            if let Some(t) = self.sim.world.get_mut::<BuildTool>(tool) {
                t.kind = sym;
                t.owner = slot as u32 + 1;
            }
        } else if name == "place" {
            if let Some(t) = self.sim.world.get_mut::<BuildTool>(tool) {
                t.place = true;
            }
        } else if name == "inv_move" {
            self.inv_move(slot, args);
        } else if name == "inv_drop" {
            self.inv_drop(slot, args);
        } else if name == "inv_use" {
            self.inv_use(slot, args);
        } else if name == "inv_craft" {
            if let Some(inv) = self.sim.world.get_mut::<Inventory>(e) {
                inv.craft = (args[0].max(0.0) as u32).saturating_add(1);
            }
        } else if name == "inv_craft_start" {
            // (a timed craft: the recipe number and how many; the server counts the seconds and makes the item)
            if let Some(inv) = self.sim.world.get_mut::<Inventory>(e) {
                inv.craft_start = (args[0].max(0.0) as u32).saturating_add(1);
                inv.craft_qty = (args[1].max(1.0) as u32).min(99);
            }
        } else if name == "inv_craft_cancel" {
            if let Some(inv) = self.sim.world.get_mut::<Inventory>(e) {
                inv.craft_cancel = true;
            }
        } else if self.host.party >= 2 && name.starts_with("party_") {
            // (a party's: docs/PARTIES.md)
            self.party_action(ctx, slot, &name, args);
        } else {
            let Some(sym) = self.sym_for(&name) else { return };
            if let Some(n) = self.sim.world.get_mut::<NetAvatar>(e) {
                n.cmd = sym;
                n.args = args;
            }
        }
    }

    fn admin(&mut self, ctx: &mut Ctx, cmd: &str, args: &str) -> Option<String> {
        match cmd {
            "say" => {
                let text = clean_chat(args.as_bytes());
                if text.is_empty() {
                    return Some("say TEXT".to_string());
                }
                ctx.out.all(chat("SERVER", &text));
                Some(format!("told {} players: {text}", self.avatars.len()))
            }
            "event" => Some(lunaris_sim::events::admin(&mut self.sim.world, args)),
            "mute" | "unmute" => {
                let Some((slot, rest)) = avatar_in(self, args) else { return Some(format!("no player called {}", args.split_whitespace().next().unwrap_or(""))) };
                let name = self.avatars.get(&slot).map(|a| a.name.clone()).unwrap_or_default();
                let secs = if cmd == "mute" { rest.split_whitespace().next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(600) } else { 0 };
                let until = if cmd == "mute" { ctx.tick.saturating_add(secs.saturating_mul(ctx.tick_hz.max(1))) } else { 0 };
                if let Some(a) = self.avatars.get_mut(&slot) {
                    a.muted_until = until;
                }
                Some(if cmd == "mute" { format!("{name} is muted for {secs} s") } else { format!("{name} can talk again") })
            }
            "save" => Some(if self.save_now() { "the world was saved".to_string() } else { "there is no save file (NetHost.save) or saving failed".to_string() }),
            "wipe" => {
                // `wipe world yes`: the world starts fresh from the scene at the next start and the players keep their characters; `wipe all yes`: the characters go too. The server stops (a supervisor starts it again).
                let mut w = args.split_whitespace();
                let (what, sure) = (w.next().unwrap_or(""), w.next() == Some("yes"));
                let characters = match what {
                    "world" => false,
                    "all" => true,
                    _ => return Some("wipe world yes (a fresh world, the characters are kept) or wipe all yes (the characters go too); the server stops afterwards".to_string()),
                };
                let Some(path) = self.save_path() else { return Some("there is no save file (NetHost.save): nothing to wipe".to_string()) };
                if !sure {
                    return Some(format!("this deletes the {} and stops the server: say `wipe {what} yes` to do it", if characters { "world and every character" } else { "world (the characters are kept)" }));
                }
                self.finish_saving(true);
                self.wipe = Some(characters);
                if characters {
                    let _ = std::fs::remove_file(&path);
                    let _ = std::fs::remove_file(path.with_extension("tmp"));
                    // (the save before it too: a world that is wiped must not come back from the fallback)
                    let _ = std::fs::remove_file(path.with_extension("bak"));
                    Some("wiped: the world and every character are gone, the server stops now".to_string())
                } else if self.save_now() {
                    Some("wiped: the world starts fresh at the next start and the characters are kept, the server stops now".to_string())
                } else {
                    self.wipe = None;
                    Some("the save could not be written: nothing was wiped".to_string())
                }
            }
            _ => None,
        }
    }

    fn wants_stop(&self) -> bool {
        self.wipe.is_some()
    }

    fn occlusion(&self) -> f32 {
        if self.host.loaded && self.sight.is_some() {
            self.host.occlusion
        } else {
            0.0
        }
    }

    fn occludes(&self, class_id: u8) -> bool {
        class_id == class::PLAYER || (self.host.occlude_machines && class_id >= 4 && !is_static(class_id))
    }

    fn blocked(&self, from: [f32; 3], to: [f32; 3]) -> bool {
        self.sight.as_ref().is_some_and(|s| !s.clear(from, to))
    }

    fn blocker(&self) -> Option<&dyn Blocker> {
        self.sight.as_ref().map(|s| s as &dyn Blocker)
    }

    fn level_version(&self) -> u32 {
        self.sight_epoch
    }

    fn level_change(&self) -> LevelChange {
        self.sight_change.clone()
    }

    fn low_flags(&self) -> u16 {
        // (the crouch button of the avatars: `NetAvatar.crouch_mask`, whose default is the one games use)
        NetAvatar::default().crouch_mask.min(0xffff) as u16
    }

    fn allies(&self) -> (u32, &[(u32, u32)]) {
        (self.party.allies_version, &self.party.allies)
    }

    fn on_tick(&mut self, ctx: &mut Ctx) {
        self.start();
        let hz = ctx.tick_hz.max(1);
        // (the simulation runs at 60 ticks a second where it can, in whole steps of a server tick)
        let steps = ((60.0 / hz as f32).round() as u32).max(1);
        self.sim.dt = 1.0 / (steps * hz) as f32;
        self.parts = TickParts::default();
        let t0 = Instant::now();
        // the avatars' server numbers
        for (slot, av) in self.avatars.iter_mut() {
            if av.net == 0 {
                if let Some((&id, _)) = ctx.world.entities.iter().find(|(_, e)| e.owner == Some(*slot)) {
                    av.net = id;
                    if let Some(e) = self.sim.world.entity(av.uid) {
                        if let Some(n) = self.sim.world.get_mut::<NetAvatar>(e) {
                            n.id = id;
                            n.slot = *slot as u32;
                        }
                    }
                }
            }
        }
        // what the players report, onto the avatars
        let list: Vec<(usize, u64, u32, u64)> = self.avatars.iter().map(|(s, a)| (*s, a.uid, a.net, a.head)).collect();
        for (slot, uid, net, head) in list {
            let Some(st) = ctx.world.get(net).map(|e| e.state) else { continue };
            self.apply_input(uid, head, &st);
            // what the player's hands say (forgotten after two thirds of a second of silence)
            if let Some(av) = self.avatars.get(&slot) {
                let axes = if ctx.tick.wrapping_sub(av.axes_tick) <= 20 { av.axes } else { [0.0; 4] };
                if let Some(e) = self.sim.world.entity(uid) {
                    match self.sim.world.get_mut::<Controls>(e) {
                        Some(c) => {
                            c.axes = axes;
                            c.buttons = st.flags as u32;
                        }
                        None => {
                            let _ = self.sim.world.insert(e, Controls { axes, buttons: st.flags as u32 });
                        }
                    }
                    // the machine the goggles have taken is driven by the hands: up, forward and right in the way the player looks; its weapon fires with the fire button; the reload button gives it back
                    let na = self.sim.world.get::<NetAvatar>(e).copied();
                    if let (Some(na), Some(hk)) = (na, self.sim.world.get_mut::<Hacker>(e)) {
                        if hk.state == 2 {
                            let (s, c) = st.yaw_rad().sin_cos();
                            hk.move_dir = [-s * axes[2] + c * axes[3], axes[1], -c * axes[2] - s * axes[3]];
                            hk.fire = st.flags as u32 & na.fire_mask != 0 && na.fire_mask != 0;
                            hk.release = st.flags as u32 & na.reload_mask != 0 && na.reload_mask != 0;
                        } else {
                            hk.move_dir = [0.0; 3];
                            hk.fire = false;
                        }
                    }
                }
            }
            if let Some(a) = self.avatars.get_mut(&slot) {
                a.set = st.pos_m();
            }
        }
        self.parts.input = t0.elapsed().as_micros() as u64;
        self.stats.input_us += self.parts.input;
        // lag compensation: where each shooter saw the others
        let t_rw = Instant::now();
        self.rewind_shooters(ctx);
        self.parts.rewind = t_rw.elapsed().as_micros() as u64;
        // the world
        let t1 = Instant::now();
        let first = self.sim.tick;
        for s in 0..steps {
            self.sim.step();
            if s == 0 {
                // (a command is there for the first step only)
                for av in self.avatars.values() {
                    if let Some(e) = self.sim.world.entity(av.uid) {
                        if let Some(n) = self.sim.world.get_mut::<NetAvatar>(e) {
                            n.cmd = Sym(0);
                        }
                        let tool = if av.head != 0 { self.sim.world.entity(av.head) } else { Some(e) }.unwrap_or(e);
                        if let Some(t) = self.sim.world.get_mut::<BuildTool>(tool) {
                            t.place = false;
                        }
                    }
                }
            }
        }
        self.sim.scripts.rewind.clear();
        self.parts.step = t1.elapsed().as_micros() as u64;
        self.stats.step_us += self.parts.step;
        let t_tell = Instant::now();
        self.tell_inventories(ctx);
        self.tick_parties(ctx);
        self.parts.tell = t_tell.elapsed().as_micros() as u64;
        // the avatars the simulation moved (a respawn) are told to go there; their health is the avatar's
        let t2 = Instant::now();
        let mut frame = self.take_frame(ctx.tick);
        for (slot, av) in self.avatars.iter_mut() {
            let Some(e) = self.sim.world.entity(av.uid) else { continue };
            let Some(pos) = self.sim.world.get::<Transform>(e).map(|t| t.pos) else { continue };
            if self.sim.world.get::<HitPoints>(e).is_some() {
                frame.at.push((av.uid, pos));
            }
            let health = health_byte(&self.sim.world, e);
            let moved = (0..3).map(|a| (pos[a] - av.set[a]).powi(2)).sum::<f32>().sqrt() > 0.5;
            let seat_uid = self.sim.world.get::<Seated>(e).map_or(0, |s| s.seat.0);
            let tool = if av.head != 0 { self.sim.world.entity(av.head) } else { Some(e) }.unwrap_or(e);
            let holding = self.sim.world.get::<Loadout>(tool).map(|l| l.holding);
            // (what its interface shows of it: when it changed, at most five times a second, at once when the gun or the magazine did)
            let status = status_of(&self.sim.world, e, tool);
            let due = ctx.tick.wrapping_sub(av.status_tick) >= (ctx.tick_hz / 5).max(1);
            let told = av.told_status;
            if told != Some(status) && (due || told.is_none_or(|t| t.mag != status.mag || t.gun != status.gun)) {
                ctx.out.to(*slot, status.message());
                av.told_status = Some(status);
                av.status_tick = ctx.tick;
            }
            // (what it wears, for the others to see: the server's mask in the low bits of `anim`, in place of what the player said; no more bytes on the wire: docs/GEAR_VISIBLE.md)
            let worn = self.sim.world.get::<lunaris_sim::gear::WornTell>(e).map(|t| (t.mask & lunaris_sim::gear::MASK) as u16);
            if let Some(en) = ctx.world.get_mut(av.net) {
                en.state.health = health;
                if let Some(m) = worn {
                    en.state.anim = m;
                }
                if let Some(item) = holding {
                    // (what the others see in its hand is the server's: the item of the gun, above the slot the player says)
                    en.state.extra = (item.min(HELD_ITEM_MAX) as u16) << HELD_BITS | (en.state.extra & HELD_SLOT as u16);
                }
                en.locked = seat_uid != 0;
                if seat_uid != 0 {
                    // (in a vehicle: where the seat is; the client puts itself there by what it is told below)
                    en.state.set_pos_m(pos);
                    av.set = pos;
                } else if moved {
                    en.state.set_pos_m(pos);
                    let mut w = W::new();
                    w.u8(ev::CORRECT);
                    for a in 0..3 {
                        w.vari(en.state.pos[a] as i64);
                    }
                    ctx.out.to(*slot, w.buf);
                    av.set = pos;
                }
            }
        }
        // the players who have sat down in a vehicle, or got out, are told
        for (slot, av) in self.avatars.iter_mut() {
            let Some(e) = self.sim.world.entity(av.uid) else { continue };
            let seat_uid = self.sim.world.get::<Seated>(e).map_or(0, |s| s.seat.0);
            if seat_uid == av.told_ride {
                continue;
            }
            if seat_uid == 0 {
                ctx.out.to(*slot, ride(0, [0.0; 3]));
                av.told_ride = 0;
                continue;
            }
            // (the vehicle is the seat's entity, or what it hangs on: whichever the players are shown)
            let Some(se) = self.sim.world.entity(seat_uid) else { continue };
            let offset = self.sim.world.get::<Seat>(se).map_or([0.0; 3], |s| s.offset);
            let parent = self.sim.world.get::<Parent>(se).map_or(0, |p| p.target.0);
            let net = self.mirror.get(&seat_uid).or_else(|| self.mirror.get(&parent)).copied();
            if let Some(id) = net {
                ctx.out.to(*slot, ride(id, offset));
                av.told_ride = seat_uid;
            }
        }
        // the players who have taken a machine, or given it back, are told
        for (slot, av) in self.avatars.iter_mut() {
            let Some(e) = self.sim.world.entity(av.uid) else { continue };
            let ctl = self.sim.world.get::<Hacker>(e).filter(|h| h.state == 2).map_or(0, |h| h.controlled.0);
            if ctl == av.told_view {
                continue;
            }
            if ctl == 0 {
                ctx.out.to(*slot, view(0));
                av.told_view = 0;
            } else if let Some(&id) = self.mirror.get(&ctl) {
                ctx.out.to(*slot, view(id));
                av.told_view = ctl;
            }
        }
        // the resources the scene wants every client to have (`NetHost.sync`): what changed to everybody, everything to a player who has just come
        if !self.host.sync.is_empty() {
            if let Some(m) = self.sync.due(&self.sim.world, &self.host.sync, ctx.tick, hz, self.host.sync_every) {
                ctx.out.all(m);
            }
            let full = self.sync.full(&self.sim.world, &self.host.sync);
            for (slot, av) in self.avatars.iter_mut().filter(|(_, a)| !a.told_sync) {
                if let Some(m) = &full {
                    ctx.out.to(*slot, m.clone());
                }
                av.told_sync = true;
            }
        }
        // the things the players see
        self.ticks += 1;
        let scan = self.ticks % self.host.static_every.max(1) as u64 == 0 || self.ticks == 1;
        self.mirror(ctx.world, scan, &mut frame.at);
        self.keep_frame(frame, ctx.tick_hz);
        // the events the scene lists, to the players near them
        if !self.host.forward.is_empty() {
            // (what each event is about, as the clients know it: the avatars by their server number, the shooter's gun being the avatar's head; the replicated things by the mirror)
            let mut who: Option<HashMap<u64, u32>> = None;
            for e in self.sim.events.iter().filter(|e| e.tick >= first) {
                let Some((_, radius)) = self.host.forward.iter().find(|(n, _)| *n == e.name) else { continue };
                let Some(pos) = e.pos else { continue };
                let who = who.get_or_insert_with(|| self.avatars.values().filter(|a| a.net != 0).flat_map(|a| [(a.uid, a.net), (a.head, a.net)]).filter(|(u, _)| *u != 0).collect());
                let about = who.get(&e.uid).or_else(|| self.mirror.get(&e.uid)).copied().unwrap_or(0);
                if *radius < 0.0 {
                    // a private event (`NAME own`): only the player it is about gets it, wherever it is; the place in it is the game's to say (a raid alarm: where the damage was)
                    let Some(slot) = self.avatars.iter().find(|(_, a)| a.uid == e.uid || a.head == e.uid).map(|(slot, _)| *slot) else { continue };
                    ctx.out.to(slot, event_about(&e.name, pos, about));
                } else {
                    ctx.out.near(pos, *radius, event_about(&e.name, pos, about));
                }
                self.stats.events_sent += 1;
            }
        }
        self.parts.mirror = t2.elapsed().as_micros() as u64;
        self.stats.mirror_us += self.parts.mirror;
        self.refresh_sight(ctx.tick_hz);
        self.stats.ticks += 1;
        self.stats.avatars = self.avatars.len();
        self.stats.mirrored = self.mirror.len();
        // a save, every so often (written on a thread of its own)
        let t_save = Instant::now();
        self.finish_saving(false);
        if !self.host.save.is_empty() && self.sim.tick >= self.next_save {
            self.next_save = self.sim.tick + (self.host.save_every.max(1.0) / self.sim.dt.max(1.0e-4)) as u64;
            self.save_in_background();
        }
        self.parts.save = t_save.elapsed().as_micros() as u64;
        self.stats.save_copy_us_max = self.stats.save_copy_us_max.max(self.parts.save);
    }

    fn tick_parts(&self) -> Vec<(&'static str, u64)> {
        let p = &self.parts;
        vec![("input", p.input), ("rewind", p.rewind), ("step", p.step), ("tell", p.tell), ("mirror", p.mirror), ("sight", p.sight), ("save", p.save)]
    }
}
