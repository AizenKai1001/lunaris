//! World events (docs/EVENTS.md): things that happen in a persistent world by themselves, a reason for players to move and meet (Rust's airdrop, DayZ's helicopter crash). A scheduler starts them at random times from a
//! text table, picks a free place of the arena, spawns a prefab there and tells the clients (resources the game syncs), and removes it when its life is over. One finished event is built on it: the **supply drop**, a crate
//! that comes down on a parachute, makes noise that the machines hear, lands, can be searched and goes away.
//!
//! * [`WorldEvents`] is the scheduler (one entity of the scene, the lowest uid that is active counts). Its `table` is text, one event a line:
//!   `event NAME kind=N prefab=PREFAB marker=PREFAB every=600..900 first=240 alive=1 height=130 life=640 inside=34 clear=3.5`. `every` is the seconds between two starts (a range is drawn from a hash, a number is fixed), `first` the
//!   seconds of its clock before the first, `alive` how many of the line may be alive at once (a start that finds the line full is tried again in 10 s: two events of a kind never overlap), `height` how high above the ground it begins,
//!   `life` how long it lasts in seconds from its start (0 never ends), `inside` the half size of the square of the arena it may be placed in, `clear` the metres round the place that must be free of anything solid, `maxy` the highest ground that counts (a roof or the top of a building piece is higher; no limit by default). At most eight lines.
//! * [`WorldEvent`] is on the entity an event made (the engine adds it to the prefab it spawns): its number, kind, age, the place and the ground, and the end of its life, when the engine removes the entity and what hangs on it.
//!   `marker` is a second prefab spawned at the same place on the ground with a `WorldEvent` whose `owner` is the event: it lives as long as the event does (the smoke that marks a drop).
//! * [`AirDrop`] is the supply drop's own behaviour on the prefab: the descent, the noise, the heat, the landing, the claim and the clean-up when it has been emptied.
//!
//! Nothing here is random (the place and the times come from `hash_rand` of the sim tick), nothing scans the players unless an event is making noise and asks for heat, and nothing runs on a connected client (`replica`): the server's
//! state reaches the clients as the resources `ev.n` (events started), `ev.kind`, `ev.x`, `ev.z` (the newest one still alive, else the last) and `ev.on` (how many are alive), which the game lists in `NetHost.sync`.
use crate::components::{BuildPiece, Door, Heat, Interactable, NetEntity, NetPlayer, Noisy, Transform};
use crate::inventory::Inventory;
use crate::layers::Filter;
use crate::meshcol::MeshStore;
use crate::rng::hash_rand;
use crate::terrain::Terrains;
use crate::vehicle_ray::RayWorld;
use lunaris_ecs::{component, Entity, Sym, Uid, World};
use lunaris_json::Json;
use std::cell::RefCell;
use std::rc::Rc;

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

const SEED: u64 = 0xE7E4_75C0_5A11_0001;
/// The most lines of a table.
pub const MAX_LINES: usize = 8;
/// Seconds before a start that could not be made (the line is full, or no place was free) is tried again.
const RETRY: f32 = 10.0;
/// Places tried for one start.
const TRIES: u64 = 16;

component! {
    /// The scheduler of the world's events (docs/EVENTS.md), on one entity of the scene (the lowest uid that is `active` counts). `table` is text, a line `event NAME kind=N prefab=PREFAB marker=PREFAB every=A..B first=S alive=N height=H life=S inside=M clear=R maxy=Y`
    /// for each event (at most eight; `#` starts a comment). `players` is how many players (avatars with a `NetAvatar`) must be in for the clock to run (0: always). `force` set to a line's number (from 1) starts that event at once (it
    /// is cleared; the line's `alive` still counts). The engine keeps the rest: `clock` (seconds counted), `next` (the clock time each line starts next, -1 not drawn yet), `fired` (how many of each line started), `n` (events started in all), and
    /// `kind`, `x`, `z` and `alive` (the newest event still going, else the last that started, and how many are going). They are saved with the world: a restart goes on from the same clock.
    pub struct WorldEvents {
        pub active: bool = true,
        pub table: Sym = Sym(0),
        pub players: u32 = 0,
        pub force: u32 = 0,
        pub clock: f32 = 0.0,
        pub next: [f32; 8] = [-1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0],
        pub fired: [u32; 8] = [0, 0, 0, 0, 0, 0, 0, 0],
        pub n: u32 = 0,
        pub kind: u32 = 0,
        pub x: f32 = 0.0,
        pub z: f32 = 0.0,
        pub alive: u32 = 0,
    }
}

component! {
    /// An event that is going (docs/EVENTS.md): the scheduler puts it on the entity it spawns from the table's prefab. `n` is the number of the event (`WorldEvents.n` when it started), `kind` and `line` the table line's, `age` the seconds it has lasted,
    /// `life` the age at which the engine removes the entity (and what hangs on it; 0 never), `x`, `z` the place on the ground and `ground` its height, `height` how high above it the event began. A thing with an `owner` (the marker of a drop) ends when
    /// that entity is gone. Events: `world_event_start` (at the place), `world_event_end`.
    pub struct WorldEvent {
        pub n: u32 = 0,
        pub kind: u32 = 0,
        pub line: u32 = 0,
        pub age: f32 = 0.0,
        pub life: f32 = 0.0,
        pub x: f32 = 0.0,
        pub z: f32 = 0.0,
        pub ground: f32 = 0.0,
        pub height: f32 = 0.0,
        pub owner: Uid = Uid(0),
    }
}

component! {
    /// A supply drop (docs/EVENTS.md), beside a `WorldEvent`, a `Transform` and (for what it does) a `Noisy`, an `Interactable` and a `NetEntity`: it comes down from `WorldEvent.height` above the ground to rest `rest` metres above it
    /// over `fall` seconds on a straight line with a sway of up to `sway` metres to each side that dies away as it lands. While it falls its `Interactable` is switched off; at touchdown it is switched on, the `NetEntity` becomes `land_class` and `land_kind` (0 keeps
    /// the one it has: a falling thing is a moving class the clients see smoothly, a landed one is a still class) and the event `airdrop_landed` is told. The noise is made by the markers of the event (the `Noisy` of the entities whose `WorldEvent.owner` is this one: it stands on the ground at the place, where the machines should go) and by a `Noisy` on the crate itself: `noise` metres
    /// loud from the start until `noise_for` seconds after the touchdown, so the machines hear it come and go to it; with `heat` above 0 every `Heat` within `heat_range` metres gains up to that much a second (the nearer the more) while it is loud. `NetEntity.flags` says what it is doing for the clients: 8 falling, 4 loud.
    /// A player who has it open makes it `claimed` for `claim_for` seconds (event `airdrop_claimed` the moment it is first opened; `claimed_by`, `opened`), and a crate that is empty and shut for `empty_life` seconds is removed (0 keeps it for its `life`).
    /// The last fields are the engine's.
    pub struct AirDrop {
        pub fall: f32 = 20.0,
        pub sway: f32 = 2.5,
        pub rest: f32 = 0.5,
        pub noise: f32 = 80.0,
        pub noise_for: f32 = 20.0,
        pub heat: f32 = 0.0,
        pub heat_range: f32 = 60.0,
        pub land_class: u32 = 3,
        pub land_kind: u32 = 0,
        pub claim_for: f32 = 8.0,
        pub empty_life: f32 = 30.0,
        pub down: bool = false,
        pub claimed: bool = false,
        pub claim_left: f32 = 0.0,
        pub empty_for: f32 = 0.0,
        pub opened: u32 = 0,
        pub claimed_by: Uid = Uid(0),
    }
}

fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

/// One line of the table.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub name: String,
    pub kind: u32,
    pub prefab: String,
    pub marker: String,
    pub every: (f32, f32),
    pub first: f32,
    pub alive: u32,
    pub height: f32,
    pub life: f32,
    pub inside: f32,
    pub clear: f32,
    pub maxy: f32,
    /// `near=MIN..MAX`: instead of the square `inside`, a place MIN to MAX metres from a player (a random avatar of each try): for a world that is loaded only round its players (a streamed city: the ground, the buildings and the sky ray of a far place are not there). (0, 0) is off.
    pub near: (f32, f32),
}

/// Read the table: `event NAME key=value ...` (keys: kind, prefab, marker, every, first, alive, height, life, inside, clear, maxy, near). A line that is not understood is skipped and listed; a line needs a prefab; at most [`MAX_LINES`] lines count.
pub fn parse_table(text: &str) -> (Vec<Line>, Vec<String>) {
    let mut lines: Vec<Line> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let at = n + 1;
        let mut words = line.split_whitespace();
        if words.next() != Some("event") {
            problems.push(format!("line {at}: a line is `event NAME key=value ...`, not `{line}`"));
            continue;
        }
        let Some(name) = words.next().filter(|w| !w.contains('=')) else {
            problems.push(format!("line {at}: `event` needs a name"));
            continue;
        };
        let mut l = Line { name: name.to_string(), kind: lines.len() as u32 + 1, prefab: String::new(), marker: String::new(), every: (600.0, 600.0), first: 300.0, alive: 1, height: 0.0, life: 600.0, inside: 30.0, clear: 3.0, maxy: 1.0e9, near: (0.0, 0.0) };
        let mut bad = false;
        for w in words {
            let Some((k, v)) = w.split_once('=') else {
                problems.push(format!("line {at}: `{w}` is not key=value"));
                bad = true;
                continue;
            };
            let num = |v: &str| v.parse::<f32>().ok().filter(|x| x.is_finite());
            match k {
                "prefab" => l.prefab = v.to_string(),
                "marker" => l.marker = v.to_string(),
                "every" => {
                    let (a, b) = match v.split_once("..") {
                        Some((a, b)) => (num(a), num(b)),
                        None => (num(v), num(v)),
                    };
                    match (a, b) {
                        (Some(a), Some(b)) if a > 0.0 && b >= a => l.every = (a, b),
                        _ => {
                            problems.push(format!("line {at}: every needs a number of seconds above 0 or a range `A..B` with B not below A, not `{v}`"));
                            bad = true;
                        }
                    }
                }
                "near" => match v.split_once("..").and_then(|(a, b)| Some((num(a)?, num(b)?))) {
                    Some((a, b)) if a >= 0.0 && b >= a && b > 0.0 => l.near = (a, b),
                    _ => {
                        problems.push(format!("line {at}: near needs a range of metres `MIN..MAX` with MAX above 0 and not below MIN, not `{v}`"));
                        bad = true;
                    }
                },
                "maxy" => match num(v) {
                    Some(x) => l.maxy = x,
                    None => {
                        problems.push(format!("line {at}: maxy needs a number, not `{v}`"));
                        bad = true;
                    }
                },
                "kind" | "alive" | "first" | "height" | "life" | "inside" | "clear" => match num(v).filter(|x| *x >= 0.0) {
                    Some(x) => match k {
                        "kind" => l.kind = x as u32,
                        "alive" => l.alive = (x as u32).max(1),
                        "first" => l.first = x,
                        "height" => l.height = x,
                        "life" => l.life = x,
                        "inside" => l.inside = x,
                        _ => l.clear = x,
                    },
                    None => {
                        problems.push(format!("line {at}: {k} needs a number of 0 or more, not `{v}`"));
                        bad = true;
                    }
                },
                _ => {
                    problems.push(format!("line {at}: unknown key `{k}` (kind, prefab, marker, every, first, alive, height, life, inside, clear, maxy, near)"));
                    bad = true;
                }
            }
        }
        if l.prefab.is_empty() {
            problems.push(format!("line {at}: an event needs `prefab=NAME`"));
            bad = true;
        }
        if lines.iter().any(|o| o.name == l.name) {
            problems.push(format!("line {at}: there is already an event called `{}`", l.name));
            bad = true;
        }
        if bad {
            continue;
        }
        if lines.len() >= MAX_LINES {
            problems.push(format!("line {at}: at most {MAX_LINES} events"));
            continue;
        }
        lines.push(l);
    }
    (lines, problems)
}

thread_local! {
    /// The table last read and what it parsed to (the scheduler asks on every step).
    static TABLE: RefCell<Option<(String, Rc<Vec<Line>>)>> = const { RefCell::new(None) };
}

fn lines_of(text: &str) -> Rc<Vec<Line>> {
    TABLE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((k, p)) = c.as_ref() {
            if k == text {
                return p.clone();
            }
        }
        let p = Rc::new(parse_table(text).0);
        *c = Some((text.to_string(), p.clone()));
        p
    })
}

/// Whether this world mirrors a server (it is a connected client: its local player's `NetPlayer.connected`). The events are the server's: a client would only start its own.
fn replica(world: &mut World) -> bool {
    let mut on = false;
    let _ = world.for_each::<(&NetPlayer,), _>(|_, (p,)| on |= p.connected);
    on
}

/// How many entities have the component called `name`: the lengths of the archetypes that have it (no visit of any entity: the same cost with two players as with two thousand).
fn count_of(world: &World, name: &str) -> u32 {
    let Some(id) = world.component_id(name) else { return 0 };
    world.matching_archetypes(&[id], &[]).iter().map(|&i| world.archetypes()[i].len() as u32).sum()
}

/// The height of a descent `age` seconds in: a straight line from `height` above the rest place to the rest place at `fall`.
pub fn descent(height: f32, fall: f32, age: f32) -> f32 {
    if fall > 0.0 {
        height * (1.0 - (age / fall).clamp(0.0, 1.0))
    } else {
        0.0
    }
}

/// A place for an event of `line`: up to [`TRIES`] spots of the square `inside` metres round the origin (or, with `near`, of the ring MIN to MAX metres round one of the `anchors`, the players) (from a hash of the tick and the salt) where a ray down from the sky meets level ground that is not a building piece, a door, a container or
/// anything an `Interactable` is on, nothing solid is within `clear` metres at three heights round it, and nothing is over it. Returns (x, ground height, z).
fn place(world: &mut World, fields: &Terrains, meshes: &MeshStore, line: &Line, tick: u64, uid: u64, salt: u64, anchors: &[V3]) -> Option<V3> {
    let inside = finite(line.inside, 30.0).max(1.0);
    let clear = finite(line.clear, 3.0).max(0.5);
    // (`near`: the places are drawn round one player, drawn from the avatars for every start: the world that is loaded there is the one that is looked at)
    let near = line.near.1 > 0.0 && !anchors.is_empty();
    let (cx, cz, reach) = if near {
        let k = ((hash_rand(SEED, tick, uid, salt * 64 + 63) * anchors.len() as f32) as usize).min(anchors.len() - 1);
        (anchors[k][0], anchors[k][2], line.near.1)
    } else {
        (0.0, 0.0, inside)
    };
    let r = reach + clear + 3.0;
    let rw = RayWorld::gather(world, fields, meshes, Some(([cx - r, -50.0, cz - r], [cx + r, 600.0, cz + r])));
    let who = Filter::DEFAULT;
    for k in 0..TRIES {
        let a = hash_rand(SEED, tick, uid, salt * 64 + k * 2);
        let b = hash_rand(SEED, tick, uid, salt * 64 + k * 2 + 1);
        let (x, z) = if near {
            let d = line.near.0 + (line.near.1 - line.near.0) * a;
            let ang = std::f64::consts::TAU * b as f64;
            (cx + d * crate::detmath::cos(ang) as f32, cz + d * crate::detmath::sin(ang) as f32)
        } else {
            ((a * 2.0 - 1.0) * inside, (b * 2.0 - 1.0) * inside)
        };
        let Some(g) = rw.cast([x, 300.0, z], [0.0, -1.0, 0.0], 400.0, who, 0) else { continue };
        if g.normal[1] < 0.85 {
            continue;
        }
        // (a piece, a door or a container is not the ground)
        if let Some(e) = world.entity(g.uid) {
            if world.get::<BuildPiece>(e).is_some() || world.get::<Door>(e).is_some() || world.get::<Interactable>(e).is_some() || world.get::<Inventory>(e).is_some() {
                continue;
            }
        }
        let gy = 300.0 - g.t;
        // (the highest ground that counts: not a roof, not the top of a building piece)
        if gy > line.maxy {
            continue;
        }
        // (open sky over it: no roof, no overhang)
        if rw.cast([x, gy + 0.6, z], [0.0, 1.0, 0.0], 250.0, who, 0).is_some() {
            continue;
        }
        let mut free = true;
        for i in 0..8 {
            let ang = std::f64::consts::TAU * i as f64 / 8.0;
            let d = [crate::detmath::cos(ang) as f32, 0.0, crate::detmath::sin(ang) as f32];
            for h in [0.4f32, 1.4, 2.6] {
                free &= rw.cast([x, gy + h, z], d, clear, who, 0).is_none();
            }
        }
        if free {
            return Some([x, gy, z]);
        }
    }
    None
}

/// The scheduler of the scene: the active `WorldEvents` with the lowest uid, if any.
fn scheduler(world: &mut World) -> Option<(Entity, u64, WorldEvents)> {
    let mut best: Option<(u64, Entity, WorldEvents)> = None;
    let _ = world.for_each_uid::<(&WorldEvents,), _>(|e, uid, (c,)| {
        if c.active && best.as_ref().is_none_or(|b| uid < b.0) {
            best = Some((uid, e, *c));
        }
    });
    best.map(|(uid, e, c)| (e, uid, c))
}

/// Spawn the prefab `name` at `at` with the overrides (a `WorldEvent` above all): the uid of its root.
fn spawn(world: &mut World, scripts: &mut crate::ScriptSet, name: &str, at: V3, we: WorldEvent) -> Option<u64> {
    let num = |v: f32| Json::Float(v as f64);
    let mut ov: Vec<(String, Json)> = Vec::new();
    crate::prefab::merge_component(&mut ov, "Transform", &Json::obj([("pos", Json::arr(at.map(num)))]));
    crate::prefab::merge_component(
        &mut ov,
        "WorldEvent",
        &Json::obj([
            ("n", Json::from(we.n as i64)),
            ("kind", Json::from(we.kind as i64)),
            ("line", Json::from(we.line as i64)),
            ("age", num(0.0)),
            ("life", num(we.life)),
            ("x", num(we.x)),
            ("z", num(we.z)),
            ("ground", num(we.ground)),
            ("height", num(we.height)),
            ("owner", Json::from(we.owner.0 as i64)),
        ]),
    );
    let plan = crate::prefab::plan(&scripts.prefabs, name, &ov).ok()?;
    let base = world.next_uid();
    let cmds = crate::prefab::spawn_commands(&plan, base);
    crate::command::apply_batch(world, scripts, &cmds).ok()?;
    Some(base)
}

/// One step of the world's events: the ones that are going age and end, the supply drops fall, make noise and land, and the scheduler starts what is due. Events of its own: `world_event_start`, `world_event_end`, `airdrop_landed`, `airdrop_claimed`.
#[allow(clippy::too_many_arguments)]
pub fn update(world: &mut World, scripts: &mut crate::ScriptSet, tick: u64, dt: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    // ---- what is going: age, end
    let mut insts: Vec<(Entity, u64, WorldEvent, V3)> = Vec::new();
    let _ = world.for_each_uid::<(&WorldEvent, &Transform), _>(|e, uid, (w, t)| insts.push((e, uid, *w, t.pos)));
    let sch = scheduler(world);
    if insts.is_empty() && sch.is_none() {
        return;
    }
    if replica(world) {
        return;
    }
    insts.sort_by_key(|i| i.1);
    let mut doomed: Vec<u64> = Vec::new();
    let mut alive_by_line = [0u32; MAX_LINES];
    let mut newest: Option<(u32, u32, V3)> = None;
    let mut going = 0u32;
    // (the things that follow an event: a drop makes its noise from the one on the ground, where the machines should go, not from the crate in the air)
    let mut marks: Vec<(u64, Entity)> = Vec::new();
    for (e, uid, mut w, pos) in insts {
        w.age = (w.age + dt).min(1.0e7);
        let over = (w.life > 0.0 && w.age >= w.life) || (w.owner.0 != 0 && (world.entity(w.owner.0).is_none() || doomed.contains(&w.owner.0)));
        if over {
            doomed.push(uid);
            events.push(("world_event_end".to_string(), uid, Some(pos)));
            continue;
        }
        if w.owner.0 != 0 {
            marks.push((w.owner.0, e));
        } else {
            going += 1;
            if (1..=MAX_LINES as u32).contains(&w.line) {
                alive_by_line[w.line as usize - 1] += 1;
            }
            if newest.is_none_or(|n| w.n >= n.0) {
                newest = Some((w.n, w.kind, [w.x, w.ground, w.z]));
            }
        }
        if let Some(c) = world.get_mut::<WorldEvent>(e) {
            c.age = w.age;
        }
    }
    air_drops(world, dt, events, &marks);
    if !doomed.is_empty() {
        crate::gameplay::despawn_trees(world, doomed);
    }

    // ---- the scheduler
    let Some((se, suid, mut ev)) = sch else { return };
    let online = if ev.players > 0 { count_of(world, "NetAvatar") } else { 0 };
    if online >= ev.players {
        ev.clock = (ev.clock + dt).min(1.0e9);
    }
    let table = world.strings().get(ev.table.0).to_string();
    let lines = lines_of(&table);
    // (where the players are, for the lines that are placed `near` one: only looked up when such a line is due)
    let mut anchors: Option<Vec<V3>> = None;
    for (i, line) in lines.iter().enumerate() {
        if ev.next[i] < 0.0 {
            ev.next[i] = ev.clock + line.first;
        }
        let forced = ev.force as usize == i + 1;
        let due = ev.clock >= ev.next[i] && online >= ev.players;
        if !due && !forced {
            continue;
        }
        // (a start that cannot be made is tried again soon; one that was only asked for leaves the drawn time alone)
        let retry = if due { ev.clock + RETRY } else { ev.next[i] };
        if alive_by_line[i] >= line.alive.max(1) || !scripts.prefabs.contains_key(&line.prefab) {
            ev.next[i] = retry;
            continue;
        }
        let salt = ev.fired[i] as u64 * MAX_LINES as u64 + i as u64;
        if line.near.1 > 0.0 && anchors.is_none() {
            let mut at: Vec<(u64, V3)> = Vec::new();
            let _ = world.for_each_uid::<(&crate::components::NetAvatar, &Transform), _>(|_, uid, (_, t)| at.push((uid, t.pos)));
            at.sort_by_key(|a| a.0);   // (by uid: the same place whatever the order the world stores them in)
            anchors = Some(at.into_iter().map(|a| a.1).collect());
        }
        let Some(p) = place(world, fields, meshes, line, tick, suid, salt, anchors.as_deref().unwrap_or(&[])) else {
            ev.next[i] = retry;
            continue;
        };
        let (x, z) = ((p[0] * 10.0).round() / 10.0, (p[2] * 10.0).round() / 10.0);
        let n = ev.n.wrapping_add(1);
        let we = WorldEvent { n, kind: line.kind, line: i as u32 + 1, age: 0.0, life: line.life, x, z, ground: p[1], height: line.height, owner: Uid(0) };
        let Some(main) = spawn(world, scripts, &line.prefab, [x, p[1] + line.height, z], we) else {
            ev.next[i] = retry;
            continue;
        };
        if !line.marker.is_empty() && scripts.prefabs.contains_key(&line.marker) {
            let mk = WorldEvent { owner: Uid(main), life: if line.life > 0.0 { line.life + 5.0 } else { 0.0 }, ..we };
            spawn(world, scripts, &line.marker, [x, p[1], z], mk);
        }
        alive_by_line[i] += 1;
        going += 1;
        newest = Some((n, line.kind, [x, p[1], z]));
        ev.n = n;
        ev.fired[i] = ev.fired[i].wrapping_add(1);
        let span = (line.every.1 - line.every.0).max(0.0);
        ev.next[i] = ev.clock + line.every.0 + span * hash_rand(SEED, tick, suid, 1000 + i as u64);
        events.push(("world_event_start".to_string(), main, Some([x, p[1], z])));
    }
    ev.force = 0;
    // (what the clients are told: the newest event that is still going, else the last that started)
    if let Some((_, kind, at)) = newest {
        ev.kind = kind;
        ev.x = (at[0] * 10.0).round() / 10.0;
        ev.z = (at[2] * 10.0).round() / 10.0;
    }
    ev.alive = going;
    if let Some(w) = world.get_mut::<WorldEvents>(se) {
        *w = ev;
    }
    for (name, v) in [("ev.n", ev.n as f64), ("ev.kind", ev.kind as f64), ("ev.x", ev.x as f64), ("ev.z", ev.z as f64), ("ev.on", going as f64)] {
        if world.resource(name) != Some(v) {
            let _ = world.set_resource(name, Some(v));
        }
    }
}

/// The supply drops: the descent, the noise, the heat, the landing, the claim and the clean-up of an empty crate.
fn air_drops(world: &mut World, dt: f32, events: &mut Events, marks: &[(u64, Entity)]) {
    let mut list: Vec<(Entity, u64, AirDrop, WorldEvent, V3)> = Vec::new();
    let _ = world.for_each_uid::<(&AirDrop, &WorldEvent, &Transform), _>(|e, uid, (a, w, t)| list.push((e, uid, *a, *w, t.pos)));
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.1);
    // where the loud ones are, for the heat
    let mut loud: Vec<(V3, f32, f32)> = Vec::new();
    for (e, uid, mut a, w, _) in list {
        let fall = finite(a.fall, 20.0).max(0.0);
        let falling = w.age < fall;
        let t = if fall > 0.0 { (w.age / fall).clamp(0.0, 1.0) } else { 1.0 };
        // the sway: a slow swing that dies away as it comes down (the angle depends on the event's number so that two drops do not swing together)
        let swing = finite(a.sway, 0.0) * (1.0 - t);
        let phase = w.n as f64 * 1.7;
        let (sx, sz) = (swing * crate::detmath::sin(w.age as f64 * 0.8 + phase) as f32, swing * crate::detmath::cos(w.age as f64 * 0.6 + phase) as f32);
        let at = [w.x + sx, w.ground + finite(a.rest, 0.5) + descent(w.height, fall, w.age), w.z + sz];
        if let Some(tr) = world.get_mut::<Transform>(e) {
            tr.pos = at;
        }
        if !falling && !a.down {
            a.down = true;
            if let Some(ne) = world.get_mut::<NetEntity>(e) {
                if a.land_class != 0 {
                    ne.class = a.land_class;
                }
                if a.land_kind != 0 {
                    ne.kind = a.land_kind;
                }
            }
            events.push(("airdrop_landed".to_string(), uid, Some(at)));
        }
        // the noise: from the first moment until a while after it is down
        let loud_now = w.age < fall + finite(a.noise_for, 0.0).max(0.0);
        let r = if loud_now { finite(a.noise, 0.0).max(0.0) } else { 0.0 };
        for (_, me) in marks.iter().filter(|m| m.0 == uid) {
            if let Some(n) = world.get_mut::<Noisy>(*me) {
                if n.radius != r {
                    n.radius = r;
                }
            }
        }
        // (a `Noisy` on the crate itself is heard from where the crate is: its radius is lengthened by its height, so that the ground within `noise` metres of the place still hears it)
        if let Some(n) = world.get_mut::<Noisy>(e) {
            let up = (at[1] - w.ground).max(0.0);
            let r = if loud_now { (r * r + up * up).sqrt() } else { 0.0 };
            if n.radius != r {
                n.radius = r;
            }
        }
        if loud_now && a.heat > 0.0 {
            loud.push(([w.x, w.ground, w.z], finite(a.heat, 0.0), finite(a.heat_range, 60.0).max(1.0)));
        }
        if let Some(ne) = world.get_mut::<NetEntity>(e) {
            let f = if falling { 8 } else { 0 } | if loud_now { 4 } else { 0 };
            if ne.flags != f {
                ne.flags = f;
            }
        }
        // the container: shut while it falls; opened by a player it is claimed for a few seconds
        let mut open = false;
        let mut user = 0u64;
        if let Some(i) = world.get_mut::<Interactable>(e) {
            if i.enabled == falling {
                i.enabled = !falling;
            }
            open = i.open;
            user = i.user.0;
        }
        if open {
            if !a.claimed {
                a.claimed = true;
                a.claimed_by = Uid(user);
                a.opened = a.opened.wrapping_add(1);
                events.push(("airdrop_claimed".to_string(), uid, Some(at)));
            }
            a.claim_left = finite(a.claim_for, 8.0).max(0.0);
        } else if a.claim_left > 0.0 {
            a.claim_left = (a.claim_left - dt).max(0.0);
            if a.claim_left <= 0.0 {
                a.claimed = false;
            }
        }
        // an empty crate that nobody has open goes after a while
        if a.down && !open && finite(a.empty_life, 0.0) > 0.0 {
            let empty = world.get::<Inventory>(e).is_some_and(|i| (0..(i.size as usize).min(48)).all(|k| i.id[k] == 0 || i.count[k] == 0)) && world.get::<crate::components::LootContainer>(e).is_none_or(|c| c.filled);
            a.empty_for = if empty { a.empty_for + dt } else { 0.0 };
            if a.empty_for >= a.empty_life {
                if let Some(we) = world.get_mut::<WorldEvent>(e) {
                    we.life = we.age;
                }
            }
        }
        if let Some(c) = world.get_mut::<AirDrop>(e) {
            *c = a;
        }
    }
    if !loud.is_empty() {
        let _ = world.for_each::<(&mut Heat, &Transform), _>(|_, (h, t)| {
            for (at, rate, range) in &loud {
                let d = ((t.pos[0] - at[0]).powi(2) + (t.pos[2] - at[2]).powi(2)).sqrt();
                if d < *range {
                    h.value = (finite(h.value, 0.0) + rate * dt * (1.0 - d / range)).min(10.0);
                }
            }
        });
    }
}

/// The server's admin command `event` (docs/ADMIN.md): with no name it lists the events of the table (how soon each starts next, how many are going, how many started), with a name it starts that event at once (its `alive` limit still counts). The answer is the text for the admin's log.
pub fn admin(world: &mut World, args: &str) -> String {
    let Some((se, _, mut ev)) = scheduler(world) else { return "this world has no world events (a WorldEvents in the scene)".to_string() };
    let table = world.strings().get(ev.table.0).to_string();
    let lines = lines_of(&table);
    let name = args.split_whitespace().next().unwrap_or("");
    if name.is_empty() {
        let mut alive = [0u32; MAX_LINES];
        let _ = world.for_each::<(&WorldEvent,), _>(|_, (w,)| {
            if w.owner.0 == 0 && (1..=MAX_LINES as u32).contains(&w.line) {
                alive[w.line as usize - 1] += 1;
            }
        });
        if lines.is_empty() {
            return "the table of events is empty".to_string();
        }
        return lines
            .iter()
            .enumerate()
            .map(|(i, l)| format!("{} (kind {}): next in {:.0} s, {} going, {} started", l.name, l.kind, (ev.next[i] - ev.clock).max(0.0), alive[i], ev.fired[i]))
            .collect::<Vec<_>>()
            .join("
");
    }
    match lines.iter().position(|l| l.name == name) {
        Some(i) => {
            ev.force = i as u32 + 1;
            if let Some(w) = world.get_mut::<WorldEvents>(se) {
                w.force = ev.force;
            }
            format!("starting {name} at once (if it has no more going than its alive limit allows)")
        }
        None => format!("no event called {name} (they are: {})", lines.iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join(", ")),
    }
}
