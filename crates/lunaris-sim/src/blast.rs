//! Explosives and the tools that use them (docs/RAIDING.md): a grenade that is thrown, a charge that is put on a wall, a blast that hurts what is near it and breaks what people built.
//!
//! * An `Explosive` is a thing that goes off: after its `fuse`, when it touches something (`contact`), when its owner presses the trigger (`remote`), or when it is destroyed itself. The blast hurts every entity with
//!   `HitPoints` inside `radius` metres of it (measured to the nearest point of the thing, not to its middle): the damage is full inside `core` metres and falls in a straight line to `edge` times at `radius`;
//!   solid cover between the blast and the thing (a wall, a door, a crate: whatever the rays of the weapons see) lets only `cover` of it through; what is a `BuildPiece` or a `Door` takes `structure_mult` times as much,
//!   so a game makes a grenade a weapon against people and a satchel charge a tool against walls. A piece that is brought to 0 is destroyed by the building system the same tick (`build_destroyed`, and what it held up
//!   may collapse). Explosion damage is not a weapon hit: worn armor (`armor.rs`) does not lessen it, and teams do not matter (a blast hurts everyone, its owner too).
//! * A `Thrower` is the player's side: a text of what can be thrown (`throw KEY prefab=P speed=S lift=L cooldown=C`) or put on the surface in front of the player (`place KEY prefab=P reach=R cooldown=C`) and the
//!   caps, and two requests the game sets: `want` (the number of a line: take that item out of the bag and throw or place it) and `trigger` (set off the player's `remote` charges). The server enforces all of it: the item must be in
//!   the bag, the player alive, the cooldown over, the number of live explosives of the player and of the server under the caps, the surface within reach. Nothing here is the client's.
//!
//! Deterministic: everything is visited in uid order, nothing is random, the world is looked at through the same rays as a bullet (`RayWorld`), and the work of a blast is bounded: at most `MAX_VICTIMS` things (the nearest)
//! and two rays for each, and at most `MAX_BLASTS` explosions go off in a tick (the rest in the next).
use crate::components::{BuildPiece, CharacterBody, Collider, Door, HitPoints, NetAvatar, NetEntity, Parent, Transform, Velocity};
use crate::gameplay::despawn_trees;
use crate::graph::ScriptSet;
use crate::inventory::{table_of, Bag, Inventory};
use crate::layers::Layers;
use crate::meshcol::MeshStore;
use crate::physics_rot::{axes_of, normalize_q, rotate, unrotate, M3};
use crate::terrain::Terrains;
use crate::vehicle_ray::RayWorld;
use lunaris_ecs::{component, Entity, Sym, Uid, World};
use lunaris_json::Json;
use std::cell::RefCell;
use std::rc::Rc;

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

/// The most things one blast hurts (the nearest).
pub const MAX_VICTIMS: usize = 64;
/// The most explosions that go off in one tick (the rest are due again in the next).
pub const MAX_BLASTS: usize = 8;
/// The most `hit` events one blast makes (a blast in a base hurts many pieces; the clients are told of the people and machines it hurt, not of every wall).
const MAX_HIT_EVENTS: usize = 24;
/// The most `build_damaged` events one blast tells of (the nearest pieces first).
const MAX_PIECE_EVENTS: usize = 16;
/// A change of velocity in one step that is an impact and not gravity or the drag of the air (metres a second).
const CONTACT_DV: f32 = 2.0;
/// The most speed (m/s) a thrower's own movement adds to a throw, so that a client that claims to run at 100 m/s does not throw the length of the map.
const INHERIT_MAX: f32 = 8.0;
/// How far a trigger reaches (metres) unless the rules say.
const TRIGGER_RANGE: f32 = 100.0;
/// A request that is refused for any reason keeps the player from asking again for this long (seconds): a client that asks in a loop makes a few refusals a second, not thousands.
const REFUSED_PAUSE: f32 = 0.25;

component! {
    /// A thing that explodes (docs/RAIDING.md): a grenade in the air, a charge on a wall, a barrel. It goes off when `age` reaches `fuse` (seconds from its first tick; a negative fuse never does), when it touches something
    /// (`contact`: after `arm` seconds, a change of velocity of a body that moves), when the `Thrower` of its `owner` presses the trigger (`remote`, also after `arm`), or when its own `HitPoints` are gone. The blast hurts every entity with
    /// `HitPoints` within `radius` metres, by the distance to the nearest point of it: `damage` up to `core` metres and, in a straight line, `edge` times that at `radius`; `cover` of it gets through solid cover (the line from the
    /// blast to the thing, ending a hand before it, meets a wall: 0 stops it all); a `BuildPiece` or a `Door` takes `structure_mult` times as much. Worn armor does not lessen it and teams do not matter. `sticky` stops a body that moves at its
    /// first impact (it stays where it hit); `life` above 0 removes it unexploded after that many seconds. Events: `blast` and `blast_TAG` (where it went off), `hit` for each thing that is not a structure that took damage, `kill` for each thing it killed.
    /// `noise` above 0 is how far, in metres, the machines hear the blast (an AI's `Perception` hears it like a shot, on the owner's side: they go to where it went off); `owner` is the entity whose bag it came from (the player's avatar); `age`, `stuck` and `last_vel` are the engine's. With a `NetEntity` the engine writes the seconds left of the fuse into `anim_t`, for a blinking light on the clients.
    pub struct Explosive {
        pub fuse: f32 = 3.0,
        pub radius: f32 = 6.0,
        pub damage: f32 = 100.0,
        pub structure_mult: f32 = 1.0,
        pub core: f32 = 0.0,
        pub edge: f32 = 0.0,
        pub cover: f32 = 0.25,
        pub arm: f32 = 0.0,
        pub life: f32 = 0.0,
        pub contact: bool = false,
        pub sticky: bool = false,
        pub remote: bool = false,
        pub owner: Uid = Uid(0),
        pub tag: Sym = Sym(0),
        pub noise: f32 = 0.0,
        pub age: f32 = 0.0,
        pub stuck: bool = false,
        pub last_vel: [f32; 3] = [0.0, 0.0, 0.0],
    }
}

component! {
    /// What a player can throw and put down (docs/RAIDING.md), on the avatar (the entity with the bag): `rules` is a text, one line a thing, `throw KEY prefab=NAME speed=15 lift=0.15 cooldown=1.2` (the item `KEY` of the bag leaves the hand along the way the player
    /// looks, `speed` metres a second plus a little of the player's own movement, tilted up by `lift`; the prefab, which has the `Explosive`, is made there with a body that flies and bounces) and `place KEY prefab=NAME reach=3 cooldown=2 gap=0.05` (the item
    /// is put on the surface in front of the player, within `reach`, turned so that its up is the surface's normal), and `cap player=4 server=40` (the most live explosives one player, and the whole server, may have) and `trigger range=100` (how far
    /// a trigger reaches). The game asks by setting `want` (the number of a throw or place line, 1 for the first) for one tick or `trigger` (true: the owner's `remote` explosives within reach go off); the engine answers at once, clears the request, and
    /// refuses (the reason is in `denied` for a tick: `unknown`, `dead`, `no_item`, `cap`, `cap_server`, `no_surface`, `no_prefab`) when the player is dead, has no such item, is over a cap or has no surface; `ready` is the tick of the next request that is
    /// heard (a cooldown after an accepted one, a short pause after a refused one). `aim` is the entity that looks (the head: 0 uses the `NetAvatar`'s `head`, else this entity); `inventory` is whose bag it is (0: this entity's, else its parent's). `thrown` and `placed` count what it has done. Events (all about the player's aim entity, at the thing's place; a refusal at the player's): `throw`, `charge_placed`, `throw_refused` and, with the reason, `throw_refused_REASON`.
    pub struct Thrower {
        pub rules: Sym = Sym(0),
        pub want: u32 = 0,
        pub trigger: bool = false,
        pub aim: Uid = Uid(0),
        pub inventory: Uid = Uid(0),
        pub ready: u32 = 0,
        pub thrown: u32 = 0,
        pub placed: u32 = 0,
        pub denied: Sym = Sym(0),
    }
}

component! {
    /// What a thing shrugs off (docs/RAIDING.md): `bullet` is the share of a bullet's damage that gets through to its `HitPoints` (1 all of it, 0.05 a twentieth: a wall that it takes a magazine to scratch). The weapons system (`weapons.rs`) asks for it on every hit; explosions have their own number
    /// (`Explosive.structure_mult`). A piece built from rules with `bullet=0.05` gets one; a door or anything else gets it from its prefab.
    pub struct Resist {
        pub bullet: f32 = 1.0,
    }
}

/// A bullet's damage to `victim` after what the thing shrugs off (`Resist`), for the weapons system.
pub fn resist(world: &World, victim: u64, damage: f32) -> f32 {
    match world.entity(victim).and_then(|e| world.get::<Resist>(e)) {
        Some(r) if r.bullet.is_finite() => damage * r.bullet.clamp(0.0, 1.0),
        _ => damage,
    }
}

// ------------------------------------------------------------------------------------------------ the rules text

/// One line of the rules: a thing that is thrown or put down.
#[derive(Clone, Debug, PartialEq)]
pub struct Act {
    pub place: bool,
    pub key: String,
    pub prefab: String,
    pub speed: f32,
    pub lift: f32,
    pub cooldown: f32,
    pub reach: f32,
    pub gap: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rules {
    pub acts: Vec<Act>,
    pub cap_player: u32,
    pub cap_server: u32,
    pub trigger_range: f32,
    /// What was wrong with the text (for `check`).
    pub problems: Vec<String>,
}

impl Default for Rules {
    fn default() -> Rules {
        Rules { acts: Vec::new(), cap_player: 4, cap_server: 64, trigger_range: TRIGGER_RANGE, problems: Vec::new() }
    }
}

/// Read the text of a `Thrower`. Lines that are not understood are skipped and listed in `problems`.
pub fn parse_rules(text: &str) -> Rules {
    let mut r = Rules::default();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let at = n + 1;
        let mut words = line.split_whitespace();
        let first = words.next().unwrap_or("");
        let number = |v: &str| v.parse::<f32>().ok().filter(|x| x.is_finite() && *x >= 0.0);
        match first {
            "throw" | "place" => {
                let Some(key) = words.next() else {
                    r.problems.push(format!("line {at}: `{first}` needs the key of an item"));
                    continue;
                };
                let mut a = Act { place: first == "place", key: key.to_string(), prefab: String::new(), speed: 15.0, lift: 0.15, cooldown: 1.0, reach: 3.0, gap: 0.05 };
                for w in words {
                    let Some((k, v)) = w.split_once('=') else {
                        r.problems.push(format!("line {at}: `{w}` is not KEY=VALUE"));
                        continue;
                    };
                    match (k, number(v)) {
                        ("prefab", _) => a.prefab = v.to_string(),
                        ("speed", Some(x)) => a.speed = x,
                        ("lift", Some(x)) => a.lift = x,
                        ("cooldown", Some(x)) => a.cooldown = x,
                        ("reach", Some(x)) => a.reach = x,
                        ("gap", Some(x)) => a.gap = x,
                        ("speed" | "lift" | "cooldown" | "reach" | "gap", None) => r.problems.push(format!("line {at}: {k}={v} is not a number from 0 up")),
                        _ => r.problems.push(format!("line {at}: `{k}` is not a setting of `{first}` (prefab, speed, lift, cooldown, reach, gap)")),
                    }
                }
                if a.prefab.is_empty() {
                    r.problems.push(format!("line {at}: `{first} {key}` needs prefab=NAME"));
                }
                r.acts.push(a);
            }
            "cap" => {
                for w in words {
                    match w.split_once('=') {
                        Some(("player", v)) if v.parse::<u32>().is_ok() => r.cap_player = v.parse().unwrap_or(4),
                        Some(("server", v)) if v.parse::<u32>().is_ok() => r.cap_server = v.parse().unwrap_or(64),
                        _ => r.problems.push(format!("line {at}: `{w}` is not player=N or server=N")),
                    }
                }
            }
            "trigger" => {
                for w in words {
                    match w.split_once('=') {
                        Some(("range", v)) if number(v).is_some() => r.trigger_range = number(v).unwrap_or(TRIGGER_RANGE),
                        _ => r.problems.push(format!("line {at}: `{w}` is not range=METRES")),
                    }
                }
            }
            _ => r.problems.push(format!("line {at}: `{first}` is not throw, place, cap or trigger")),
        }
    }
    r
}

thread_local! {
    /// The text of the rules last read and what it parsed to (every request of every player asks).
    static CACHE: RefCell<Option<(String, Rc<Rules>)>> = const { RefCell::new(None) };
}

fn rules_of_text(text: &str) -> Rc<Rules> {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((k, r)) = c.as_ref() {
            if k == text {
                return r.clone();
            }
        }
        let r = Rc::new(parse_rules(text));
        *c = Some((text.to_string(), r.clone()));
        r
    })
}

// ------------------------------------------------------------------------------------------------ small vector helpers

#[inline]
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
fn finite3(a: V3) -> bool {
    a.iter().all(|v| v.is_finite())
}
fn num(v: f32) -> Json {
    Json::Float(v as f64)
}
fn json3(v: V3) -> Json {
    Json::arr(v.map(num))
}

/// The rotation that turns +Y into the unit vector `n` by the shortest way (a charge lies with its top out of the surface it is put on).
fn quat_up_to(n: V3) -> [f32; 4] {
    let c = n[1];
    if c < -0.9999 {
        return [1.0, 0.0, 0.0, 0.0];
    }
    // (the axis is y x n = (n.z, 0, -n.x), the angle's half is in the w: 1 + cos)
    normalize_q([n[2], 0.0, -n[0], 1.0 + c])
}

// ------------------------------------------------------------------------------------------------ bodies and the nearest point of one

#[derive(Clone, Copy, Debug)]
enum Body {
    Capsule(V3, V3, f32),
    Sphere(V3, f32),
    Obb(V3, M3, V3),
}

/// The point of the body nearest to `p` (`p` itself when it is inside).
fn nearest(b: &Body, p: V3) -> V3 {
    let ball = |c: V3, r: f32| {
        let d = sub(p, c);
        let l = len(d);
        if l <= r || l < 1.0e-9 {
            p
        } else {
            add(c, scale(d, r / l))
        }
    };
    match *b {
        Body::Sphere(c, r) => ball(c, r),
        Body::Capsule(a, b2, r) => {
            let ab = sub(b2, a);
            let t = (dot(sub(p, a), ab) / dot(ab, ab).max(1.0e-12)).clamp(0.0, 1.0);
            ball(add(a, scale(ab, t)), r)
        }
        Body::Obb(c, ax, h) => {
            let l = unrotate(&ax, sub(p, c));
            let k = [l[0].clamp(-h[0], h[0]), l[1].clamp(-h[1], h[1]), l[2].clamp(-h[2], h[2])];
            add(c, rotate(&ax, k))
        }
    }
}

/// What a blast sees of one entity.
struct Victim {
    uid: u64,
    e: Entity,
    body: Body,
    structure: bool,
    dist: f32,
}

/// The things with `HitPoints` that a blast at `centre` of `radius` reaches, the nearest `MAX_VICTIMS` of them, by distance and then uid.
fn victims(world: &mut World, centre: V3, radius: f32, skip: u64) -> Vec<Victim> {
    let rules = crate::building::rules_of(world).map(|(_, r)| r);
    let mut raw: Vec<(Entity, u64, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&HitPoints, &Transform), _>(|e, uid, (h, t)| {
        if !h.dead && uid != skip && finite3(t.pos) && len(sub(t.pos, centre)) <= radius + 40.0 {
            raw.push((e, uid, *t));
        }
    });
    raw.sort_by_key(|r| r.1);
    let mut out: Vec<Victim> = Vec::new();
    for (e, uid, t) in raw {
        let s = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
        let axes = axes_of(normalize_q(t.rot));
        let body = if let Some(cb) = world.get::<CharacterBody>(e) {
            let r = if cb.radius.is_finite() { cb.radius } else { 0.35 }.max(0.05);
            let h = if cb.height.is_finite() { cb.height } else { 1.8 }.max(2.0 * r);
            Body::Capsule([t.pos[0], t.pos[1] + r, t.pos[2]], [t.pos[0], t.pos[1] + h - r, t.pos[2]], r)
        } else if let Some(c) = world.get::<Collider>(e).filter(|c| !c.trigger) {
            if c.shape == 0 {
                Body::Sphere(t.pos, 0.5 * c.size[0].abs() * s[0].max(s[1]).max(s[2]))
            } else if c.shape == 2 {
                let r = 0.5 * c.size[0].abs() * s[0].max(s[2]);
                let half = (0.5 * c.size[1].abs() * s[1] - r).max(0.0);
                Body::Capsule(add(t.pos, scale(axes[1], -half)), add(t.pos, scale(axes[1], half)), r)
            } else {
                Body::Obb(t.pos, axes, [0.5 * c.size[0].abs() * s[0], 0.5 * c.size[1].abs() * s[1], 0.5 * c.size[2].abs() * s[2]])
            }
        } else if let (Some(p), Some(r)) = (world.get::<BuildPiece>(e), rules.as_ref()) {
            // (a piece with no collider of its own, made of parts: it is the box of the rules)
            let name = world.strings().get(p.kind.0);
            let Some(k) = r.kind(name) else { continue };
            let sz = r.kinds[k].size;
            Body::Obb(t.pos, axes, [sz[0] * 0.5, sz[1] * 0.5, sz[2] * 0.5])
        } else {
            continue;
        };
        let near = nearest(&body, centre);
        let dist = len(sub(near, centre));
        if dist <= radius {
            let structure = world.get::<BuildPiece>(e).is_some() || world.get::<Door>(e).is_some();
            out.push(Victim { uid, e, body, structure, dist });
        }
    }
    out.sort_by(|a, b| a.dist.total_cmp(&b.dist).then(a.uid.cmp(&b.uid)));
    out.truncate(MAX_VICTIMS);
    out
}

/// Damage at a distance: full inside `core`, falling in a straight line to `edge` times at `radius`, nothing beyond.
pub fn falloff(damage: f32, dist: f32, radius: f32, core: f32, edge: f32) -> f32 {
    if !(dist <= radius) || radius <= 0.0 {
        return 0.0;
    }
    let core = core.clamp(0.0, radius);
    if dist <= core || radius <= core {
        return damage;
    }
    damage * (1.0 - (1.0 - edge.clamp(0.0, 1.0)) * ((dist - core) / (radius - core)).clamp(0.0, 1.0))
}

/// Take `amount` off `HitPoints`; true when this brought it to 0 (a thing in its grace period, already dead or with nothing to take takes nothing).
fn hurt(world: &mut World, e: Entity, amount: f32) -> bool {
    let Some(h) = world.get_mut::<HitPoints>(e) else { return false };
    if h.dead || h.guard || h.invulnerable > 0.0 || !(amount > 0.0) {
        return false;
    }
    let before = h.value;
    h.value = (h.value - amount).max(0.0);
    before > 0.0 && h.value <= 0.0
}

/// Is something solid between `from` and `target` (stopping `pad` metres before the target, so that the thing itself and its parts do not count as cover)?
fn covered(rw: &RayWorld, who: crate::layers::Filter, from: V3, target: V3, pad: f32, victim: u64, bomb: u64) -> bool {
    let d = sub(target, from);
    let l = len(d);
    let max = l - pad;
    if !(max > 0.05) {
        return false;
    }
    rw.cast(from, scale(d, 1.0 / l), max, who, bomb).is_some_and(|h| h.uid != victim)
}

/// One explosion: hurt what is near, tell what happened, and take the explosive out of the world.
fn detonate(world: &mut World, fields: &Terrains, meshes: &MeshStore, uid: u64, x: &Explosive, centre: V3, events: &mut Events) {
    let tag = world.strings().get(x.tag.0).to_string();
    events.push(("blast".to_string(), uid, Some(centre)));
    if !tag.is_empty() {
        events.push((format!("blast_{tag}"), uid, Some(centre)));
    }
    // the sound: a `Noisy` of the owner's side that lives for this one step (the AI's `Perception` runs later in the same step and hears it, as it hears a shot)
    if x.noise.is_finite() && x.noise > 0.0 && finite3(centre) {
        let side = world.entity(x.owner.0).and_then(|oe| world.get::<crate::components::Team>(oe).map(|t| t.id)).unwrap_or(0);
        if let Ok(ne) = world.spawn_with3(crate::components::Transform { pos: centre, ..Default::default() }, crate::components::Noisy { radius: x.noise }, crate::components::Team { id: side }) {
            let _ = world.insert(ne, crate::components::Lifetime { seconds: 0.001 });
        }
    }
    let radius = if x.radius.is_finite() { x.radius.max(0.0) } else { 0.0 };
    if radius > 0.0 && x.damage > 0.0 && finite3(centre) {
        let list = victims(world, centre, radius, uid);
        if !list.is_empty() {
            let pad = 1.0;
            let (lo, hi) = ([centre[0] - radius - pad, centre[1] - radius - pad, centre[2] - radius - pad], [centre[0] + radius + pad, centre[1] + radius + pad, centre[2] + radius + pad]);
            let rw = RayWorld::gather(world, fields, meshes, Some((lo, hi)));
            let layers = Layers::gather(world);
            let who = layers.of(uid);
            // (a little above the blast: a grenade on the ground must not see the ground as a wall between it and a foot)
            let from = add(centre, [0.0, 0.1, 0.0]);
            let mut hit_events = 0;
            let mut piece_events = 0;
            for v in &list {
                let blocked = match v.body {
                    Body::Capsule(a, b, r) => {
                        let mid = scale(add(a, b), 0.5);
                        covered(&rw, who, from, mid, r + 0.1, v.uid, uid) && covered(&rw, who, from, b, r + 0.1, v.uid, uid)
                    }
                    _ => covered(&rw, who, from, nearest(&v.body, centre), 0.1, v.uid, uid),
                };
                let mut dmg = falloff(x.damage, v.dist, radius, x.core, x.edge);
                if blocked {
                    dmg *= x.cover.clamp(0.0, 1.0);
                }
                if v.structure {
                    dmg *= x.structure_mult.max(0.0);
                }
                let at = match v.body {
                    Body::Capsule(a, b, _) => scale(add(a, b), 0.5),
                    _ => nearest(&v.body, centre),
                };
                let before = world.get::<HitPoints>(v.e).map_or(0.0, |h| h.value);
                let killed = hurt(world, v.e, dmg);
                let took = world.get::<HitPoints>(v.e).is_some_and(|h| h.value < before);
                if took && !v.structure && hit_events < MAX_HIT_EVENTS {
                    hit_events += 1;
                    events.push(("hit".to_string(), v.uid, Some(at)));
                }
                // (a piece of a base that took damage: the event `build_damaged`, about the piece at the place that was hurt, for what watches the base (the raid alarm, `depth.rs`); at most MAX_PIECE_EVENTS a blast)
                if took && v.structure && piece_events < MAX_PIECE_EVENTS && world.get::<BuildPiece>(v.e).is_some() {
                    piece_events += 1;
                    events.push(("build_damaged".to_string(), v.uid, Some(at)));
                }
                if killed {
                    events.push(("kill".to_string(), v.uid, Some(at)));
                }
            }
        }
    }
    despawn_trees(world, vec![uid]);
}

// ------------------------------------------------------------------------------------------------ the players' requests

/// Whose bag a thrower uses: the one it names, its own, or its parent's.
fn bag_of(world: &World, e: Entity, named: Uid) -> Option<Entity> {
    if named.0 != 0 {
        return world.entity(named.0).filter(|b| world.get::<Inventory>(*b).is_some());
    }
    if world.get::<Inventory>(e).is_some() {
        return Some(e);
    }
    let parent = world.get::<Parent>(e)?.target.0;
    world.entity(parent).filter(|b| world.get::<Inventory>(*b).is_some())
}

/// The entity that looks: the one named, the `head` of the `NetAvatar`, or the thrower itself.
fn aim_of(world: &World, e: Entity, t: &Thrower) -> Entity {
    if t.aim.0 != 0 {
        if let Some(a) = world.entity(t.aim.0) {
            return a;
        }
    }
    if let Some(na) = world.get::<NetAvatar>(e) {
        if let Some(h) = (na.head.0 != 0).then(|| world.entity(na.head.0)).flatten() {
            return h;
        }
    }
    e
}

/// How many explosives are alive: all, and those of `owner`.
fn live(world: &mut World, owner: u64) -> (u32, u32) {
    let (mut all, mut mine) = (0u32, 0u32);
    let _ = world.for_each::<(&Explosive,), _>(|_, (x,)| {
        all += 1;
        if x.owner.0 == owner {
            mine += 1;
        }
    });
    (all, mine)
}

fn alive(world: &World, e: Entity) -> bool {
    world.get::<HitPoints>(e).is_none_or(|h| !h.dead && h.value > 0.0)
}

/// What a request did: the reason it was refused, or nothing.
struct Answer {
    denied: &'static str,
    /// (explosive made, where)
    made: Option<(u64, V3)>,
    place: bool,
}

#[allow(clippy::too_many_arguments)]
fn handle(world: &mut World, scripts: &mut ScriptSet, fields: &Terrains, meshes: &MeshStore, e: Entity, uid: u64, t: &Thrower, rules: &Rules) -> Answer {
    let refuse = |why: &'static str| Answer { denied: why, made: None, place: false };
    let Some(act) = rules.acts.get(t.want as usize - 1).cloned() else { return refuse("unknown") };
    let Some(bag_e) = bag_of(world, e, t.inventory) else { return refuse("no_item") };
    if !alive(world, bag_e) {
        return refuse("dead");
    }
    let owner = world.uid_of(bag_e).unwrap_or(0);
    let (all, mine) = live(world, owner);
    if mine >= rules.cap_player {
        return refuse("cap");
    }
    if all >= rules.cap_server {
        return refuse("cap_server");
    }
    if act.prefab.is_empty() || !scripts.prefabs.contains_key(&act.prefab) {
        return refuse("no_prefab");
    }
    let Some((table, _)) = table_of(world) else { return refuse("no_item") };
    let id = table.id_of(&act.key);
    let Some(mut inv) = world.get::<Inventory>(bag_e).copied() else { return refuse("no_item") };
    let mut bag = Bag::of(&inv);
    if id == 0 || bag.total(id) == 0 {
        return refuse("no_item");
    }
    // where the player looks
    let aim_e = aim_of(world, e, t);
    let Some(xf) = world.get::<Transform>(aim_e).copied() else { return refuse("no_surface") };
    if !finite3(xf.pos) || !xf.rot.iter().all(|v| v.is_finite()) {
        return refuse("no_surface");
    }
    let z = axes_of(normalize_q(xf.rot))[2];
    let fwd = [-z[0], -z[1], -z[2]];
    let o = xf.pos;
    let reach = if act.place { act.reach.clamp(0.1, 50.0) } else { 0.8 };
    let end = add(o, scale(fwd, reach));
    let (lo, hi) = ([o[0].min(end[0]) - 1.0, o[1].min(end[1]) - 1.0, o[2].min(end[2]) - 1.0], [o[0].max(end[0]) + 1.0, o[1].max(end[1]) + 1.0, o[2].max(end[2]) + 1.0]);
    let rw = RayWorld::gather(world, fields, meshes, Some((lo, hi)));
    let layers = Layers::gather(world);
    let hit = rw.cast(o, fwd, reach, layers.of(uid), owner);
    let (pos, rot, vel) = if act.place {
        let Some(h) = hit else { return refuse("no_surface") };
        (add(add(o, scale(fwd, h.t)), scale(h.normal, act.gap)), quat_up_to(h.normal), [0.0; 3])
    } else {
        // (it leaves a little in front of the eyes, and not through a wall that is right there)
        let out = hit.map_or(0.55, |h| (h.t - 0.15).clamp(0.0, 0.55));
        let mut dir = fwd;
        dir[1] += act.lift;
        let l = len(dir).max(1.0e-6);
        let mut v = scale(dir, act.speed / l);
        if let Some(own) = world.get::<Velocity>(bag_e) {
            let mut h = [own.lin[0], 0.0, own.lin[2]];
            let hl = len(h);
            if !(hl <= INHERIT_MAX) {
                h = if hl.is_finite() { scale(h, INHERIT_MAX / hl) } else { [0.0; 3] };
            }
            v = add(v, [h[0], own.lin[1].clamp(-4.0, 4.0), h[2]]);
        }
        (add(o, scale(fwd, out)), [0.0, 0.0, 0.0, 1.0], v)
    };
    let more = [
        ("Velocity", Json::obj([("lin", json3(vel))])),
        ("Explosive", Json::obj([("owner", Json::from(owner as i64)), ("age", num(0.0)), ("stuck", Json::Bool(false)), ("last_vel", json3(vel))])),
    ];
    let more: Vec<(&str, Json)> = if act.place { more[1..].to_vec() } else { more.to_vec() };
    let Some(made) = crate::building::spawn_prefab_rot(world, scripts, &act.prefab, pos, rot, &more) else { return refuse("no_prefab") };
    bag.remove(id, 1);
    bag.store(&mut inv);
    if let Some(w) = world.get_mut::<Inventory>(bag_e) {
        *w = inv;
    }
    Answer { denied: "", made: Some((made, pos)), place: act.place }
}

pub fn update(world: &mut World, scripts: &mut ScriptSet, tick: u64, dt: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    // ---- the players: what they ask (and the refusal of last tick, which is forgotten now)
    let mut asks: Vec<(Entity, u64, Thrower)> = Vec::new();
    let _ = world.for_each_uid::<(&Thrower,), _>(|e, uid, (t,)| {
        if t.want != 0 || t.trigger || t.denied.0 != 0 {
            asks.push((e, uid, *t));
        }
    });
    asks.sort_by_key(|a| a.1);
    // (the owners whose trigger was pressed: the bag's owner and where it stands)
    let mut triggers: Vec<(u64, V3, f32)> = Vec::new();
    for (e, uid, mut t) in asks {
        let before = t;
        t.denied = Sym(0);
        let text = world.strings().get(t.rules.0).to_string();
        let rules = rules_of_text(&text);
        let now = tick as u32;
        // (a request that comes before `ready` is not heard: no answer, no event)
        let heard = now >= t.ready;
        // (the trigger is not held by the cooldown of a throw: a charge that was put down a moment ago may be set off, and asking costs one look at the explosives)
        if t.trigger {
            // (the owner is whoever has the bag; a thrower with no bag is its own owner)
            let b = bag_of(world, e, t.inventory).unwrap_or(e);
            if alive(world, b) {
                let pos = world.get::<Transform>(b).map_or([0.0; 3], |x| x.pos);
                triggers.push((world.uid_of(b).unwrap_or(0), pos, rules.trigger_range));
            }
        }
        if heard && t.want != 0 {
            let aim_uid = world.uid_of(aim_of(world, e, &t)).unwrap_or(uid);
            let ans = handle(world, scripts, fields, meshes, e, uid, &t, &rules);
            let cooldown = |secs: f32| ((secs / dt).ceil() as u32).max(1);
            match ans.made {
                Some((_, pos)) => {
                    let act = &rules.acts[t.want as usize - 1];
                    t.ready = now.saturating_add(cooldown(act.cooldown));
                    if ans.place {
                        t.placed = t.placed.wrapping_add(1);
                        events.push(("charge_placed".to_string(), aim_uid, Some(pos)));
                    } else {
                        t.thrown = t.thrown.wrapping_add(1);
                        events.push(("throw".to_string(), aim_uid, Some(pos)));
                    }
                }
                None => {
                    t.ready = now.saturating_add(cooldown(REFUSED_PAUSE));
                    t.denied = Sym(world.strings_mut().intern(ans.denied));
                    let at = world.get::<Transform>(e).map_or([0.0; 3], |x| x.pos);
                    events.push(("throw_refused".to_string(), aim_uid, Some(at)));
                    events.push((format!("throw_refused_{}", ans.denied), aim_uid, Some(at)));
                }
            }
        }
        t.want = 0;
        t.trigger = false;
        if t != before {
            if let Some(w) = world.get_mut::<Thrower>(e) {
                *w = t;
            }
        }
    }

    // ---- the explosives: time, touch, trigger, damage
    let mut bombs: Vec<(Entity, u64, Explosive, V3)> = Vec::new();
    let _ = world.for_each_uid::<(&Explosive, &Transform), _>(|e, uid, (x, t)| bombs.push((e, uid, *x, t.pos)));
    if bombs.is_empty() {
        return;
    }
    bombs.sort_by_key(|b| b.1);
    let mut due: Vec<(u64, Explosive, V3)> = Vec::new();
    let mut gone: Vec<u64> = Vec::new();
    for (e, uid, mut x, pos) in bombs {
        let before = x;
        x.age += dt;
        let armed = x.age >= x.arm;
        let mut boom = x.fuse >= 0.0 && x.age >= x.fuse;
        if !boom && armed && x.remote && triggers.iter().any(|(o, p, range)| *o == x.owner.0 && len(sub(*p, pos)) <= *range) {
            boom = true;
        }
        // touch: a body that moves and whose velocity changed by more than gravity and the air do in one step
        if !x.stuck {
            if let Some(v) = world.get::<Velocity>(e).map(|v| v.lin) {
                if x.age > 2.0 * dt && finite3(v) && len(sub(v, x.last_vel)) > CONTACT_DV {
                    if x.sticky {
                        x.stuck = true;
                        x.last_vel = [0.0; 3];
                        // (without a `Velocity` it is a still thing: it stays where it hit)
                        let _ = world.remove_typed::<Velocity>(e);
                    }
                    if x.contact && armed {
                        boom = true;
                    }
                }
                if !x.stuck {
                    x.last_vel = v;
                }
            }
        }
        if world.get::<HitPoints>(e).is_some_and(|h| h.dead || h.value <= 0.0) {
            boom = true;
        }
        if boom {
            due.push((uid, x, pos));
        } else if x.life > 0.0 && x.age >= x.life {
            gone.push(uid);
        } else {
            if let Some(n) = world.get_mut::<NetEntity>(e) {
                n.anim_t = if x.fuse >= 0.0 { (x.fuse - x.age).max(0.0) } else { 0.0 };
            }
            if x != before {
                if let Some(w) = world.get_mut::<Explosive>(e) {
                    *w = x;
                }
            }
        }
    }
    if !gone.is_empty() {
        despawn_trees(world, gone);
    }
    for (k, (uid, x, pos)) in due.into_iter().enumerate() {
        if k >= MAX_BLASTS {
            // (it goes off in the next tick: the age it has is past its fuse still)
            if let Some(e) = world.entity(uid) {
                if let Some(w) = world.get_mut::<Explosive>(e) {
                    w.age = x.age;
                    w.fuse = if x.fuse >= 0.0 { x.fuse.min(x.age) } else { 0.0 };
                }
            }
            continue;
        }
        detonate(world, fields, meshes, uid, &x, pos, events);
    }
}
