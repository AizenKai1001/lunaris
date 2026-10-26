//! Building (docs/BUILDING.md): pieces that snap together at sockets, stand or fall by how well they are held, decay, and can be built onto what a generator made. The rules are text (`BuildRules`: what
//! kinds of piece there are, how big, where their sockets are and what plugs into what), the player's tool (`BuildTool`) aims along its entity and says where a piece would go and whether it may, and
//! `place` builds it. A piece's *stability* is how well the ground holds it: 1 for a piece resting on the ground (a foundation) or joined to an anchoring `BuildSocket`, and for every other piece the best of
//! what its neighbours have times the `carry` of the socket they are joined by, so a tower or a cantilever hangs on fewer and fewer threads until it is under the rules' `min` and collapses. It makes no
//! model and plays nothing: what a piece looks like, its sounds and its rubble are the game's prefabs.
//!
//! Deterministic: everything is derived from the world (nothing is cached between ticks), pieces are visited in uid order and ties are broken by uid, so the same world gives the same structure on every
//! machine. Cost: nothing when nobody builds and nothing changes; a tool costs one scan of the pieces near it per tick; a placement, a destruction and the one-second audit rebuild the index of all pieces.
use crate::components::{BuildPiece, BuildRules, BuildSocket, BuildTool, BuildUpkeep, HitPoints, NetAvatar, NetEntity, Parent, Transform};
use crate::inventory::{table_of, Bag, Inventory};
use crate::detmath;
use crate::gameplay::despawn_trees;
use crate::graph::ScriptSet;
use crate::layers::{Filter, Layers};
use crate::meshcol::MeshStore;
use crate::physics_rot::{axes_of, normalize_q};
use crate::terrain::Terrains;
use crate::vehicle_ray::{segment_box, RayWorld};
use lunaris_ecs::{component, Entity, Sym, Uid, World};
use lunaris_json::Json;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};
use std::hash::{BuildHasherDefault, Hasher};
use std::rc::Rc;

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

/// How near two sockets must be to count as joined (metres).
const LINK: f32 = 0.15;
/// The side of a cell of the index (metres).
const CELL: f32 = 8.0;
/// Not a piece: an item of the index that is a `BuildSocket` entity.
const NONE: usize = usize::MAX;
/// The most places the tool tries when the nearest is not allowed.
const TRIES: usize = 6;

#[derive(Default, Clone, Copy)]
struct Fx(u64);

impl Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.write_u64(*b as u64);
        }
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    fn write_u32(&mut self, v: u32) {
        self.write_u64(v as u64);
    }
    fn write_i32(&mut self, v: i32) {
        self.write_u64(v as u32 as u64);
    }
    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }
}

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<Fx>>;
type FxSet<K> = HashSet<K, BuildHasherDefault<Fx>>;

#[inline]
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
fn dist(a: V3, b: V3) -> f32 {
    let d = sub(a, b);
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// The cosine and the sine of `yaw` degrees (kept with every piece: a trigonometric function costs far more than the rest of a link test).
fn cos_sin(yaw: f32) -> (f32, f32) {
    let a = yaw as f64 * (std::f64::consts::PI / 180.0);
    (detmath::cos(a) as f32, detmath::sin(a) as f32)
}

/// `v` turned about the vertical axis by an angle with this cosine and sine.
#[inline]
fn rot_cs(v: V3, cs: f32, sn: f32) -> V3 {
    [v[0] * cs + v[2] * sn, v[1], -v[0] * sn + v[2] * cs]
}

/// `v` turned about the vertical axis by `yaw` degrees (a quarter turn to the left is positive: local -z, forward, goes to `(-sin, 0, -cos)`).
fn rot_y(v: V3, yaw: f32) -> V3 {
    let (cs, sn) = cos_sin(yaw);
    rot_cs(v, cs, sn)
}

fn wrap(d: f32) -> f32 {
    let mut x = d % 360.0;
    if x > 180.0 {
        x -= 360.0;
    }
    if x <= -180.0 {
        x += 360.0;
    }
    x
}

/// The turn about the vertical axis (degrees) of a rotation.
pub fn yaw_of(q: [f32; 4]) -> f32 {
    let (x, y, z, w) = (q[0] as f64, q[1] as f64, q[2] as f64, q[3] as f64);
    (detmath::atan2(2.0 * (x * z + w * y), 1.0 - 2.0 * (x * x + y * y)) * (180.0 / std::f64::consts::PI)) as f32
}

/// The rotation of a turn about the vertical axis of `yaw` degrees.
pub fn yaw_quat(yaw: f32) -> [f32; 4] {
    let h = yaw as f64 * (std::f64::consts::PI / 360.0);
    [0.0, detmath::sin(h) as f32, 0.0, detmath::cos(h) as f32]
}

component! {
    /// The player's side of upgrading a piece to the next tier (docs/RAIDING.md, "Tiers"), on the avatar: the game sets `want` for one tick and the engine looks at the piece aimed at (the same aim as a `BuildTool`'s: `reach` metres along the aim entity, `aim`, or the `NetAvatar`'s `head`,
    /// or this entity; a wall of the world in front hides it): if its rules line has `upgrade=PIECE` and the bag (`inventory`, 0: this entity's or its parent's) holds the `upgrade_cost`, the items are taken and the piece **becomes** the other piece, at once: its kind and its hit points (full) are the new
    /// piece's, its `Resist` too, and the `NetEntity.kind` of a piece that is replicated, so that the clients show the new model. It keeps its place, its owner and its links (a tier has the same size and sockets). Not on land that is another owner's cupboard (`owner` is the player's number, `NetAvatar.slot` + 1
    /// when 0). The answer is the event `build_upgraded` (the piece) and `build_upgrade_ok` (the player's aim entity), or `build_upgrade_refused` and `build_upgrade_refused_WHY` with `denied` for a tick: `no_aim` (no piece in reach), `no_upgrade`, `no_materials`, `not_yours`, `dead`. `ready` is the tick of the next
    /// request that is heard (a short pause, so that a client that asks in a loop is not answered in a loop), `upgraded` counts what it has done.
    pub struct Upgrader {
        pub want: bool = false,
        pub reach: f32 = 5.0,
        pub owner: u32 = 0,
        pub aim: Uid = Uid(0),
        pub inventory: Uid = Uid(0),
        pub ready: u32 = 0,
        pub upgraded: u32 = 0,
        pub denied: Sym = Sym(0),
    }
}

// ------------------------------------------------------------------------------------------------ the rules

#[derive(Clone, Debug, PartialEq)]
pub struct Socket {
    pub name: String,
    pub at: V3,
    pub yaw: f32,
    pub ty: String,
    pub mates: Vec<String>,
    pub carry: f32,
    pub carry_of: Vec<(String, f32)>,
    pub turn: f32,
}

impl Socket {
    /// The share of a neighbour's stability this socket takes from a socket of type `ty`.
    pub fn carry_for(&self, ty: &str) -> f32 {
        self.carry_of.iter().find(|c| c.0 == ty).map_or(self.carry, |c| c.1)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Kind {
    pub name: String,
    pub size: V3,
    pub hp: f32,
    pub free: bool,
    pub anchor: bool,
    pub loose: bool,
    pub decay: f32,
    pub inset: Option<f32>,
    pub prefab: String,
    pub debris: String,
    /// The share of a bullet's damage that gets through (`bullet=0.05`: a wall shrugs off most of it); 1 is all of it. A piece made from these rules gets a `Resist` when this is under 1.
    pub bullet: f32,
    /// The piece this one becomes when it is upgraded (`upgrade=wall_stone`), and what that takes out of the bag (`upgrade_cost=scrap:25,tape:4`): a tier of the same geometry (docs/RAIDING.md, "Tiers").
    pub upgrade: String,
    pub upgrade_cost: Vec<(String, u32)>,
    /// What repairing the piece from nothing to full costs (`repair_cost=scrap:15,tape:3`): a repair of part of it costs that share of it, rounded up (docs/DEPTH.md). Empty: the piece cannot be repaired.
    pub repair_cost: Vec<(String, u32)>,
    /// The share of a melee blow's damage that gets through (`melee=0.25`: stone shrugs off most of what a crowbar does; docs/DEPTH.md, `Striker`); 1 is all of it, 0 is immune to blows. Default 1.
    pub melee: f32,
    pub sockets: Vec<Socket>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rules {
    pub snap: f32,
    pub min: f32,
    pub gap: f32,
    pub slope: f32,
    pub step: f32,
    pub ground: f32,
    pub audit: f32,
    pub kinds: Vec<Kind>,
    /// What was wrong with the text (for `check`).
    pub problems: Vec<String>,
}

impl Default for Rules {
    fn default() -> Rules {
        Rules { snap: 1.2, min: 0.1, gap: 0.05, slope: 0.7, step: 15.0, ground: 0.5, audit: 1.0, kinds: Vec::new(), problems: Vec::new() }
    }
}

impl Rules {
    pub fn kind(&self, name: &str) -> Option<usize> {
        self.kinds.iter().position(|k| k.name == name)
    }
}

fn flag(v: &str) -> Option<bool> {
    match v {
        "1" | "true" | "yes" => Some(true),
        "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

fn nums(v: &str, n: usize) -> Option<Vec<f32>> {
    let p: Vec<f32> = v.split(',').map(|x| x.trim().parse::<f32>().ok().filter(|f| f.is_finite())).collect::<Option<Vec<_>>>()?;
    (p.len() == n).then_some(p)
}

/// Read the text of a `BuildRules`. `prefabs` are the prefab names of the world (to say which `prefab=` and `debris=` do not exist), when known.
pub fn parse_rules(text: &str, prefabs: Option<&[String]>) -> Rules {
    let mut r = Rules::default();
    let mut sized: Vec<bool> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let at = n + 1;
        let mut words = line.split_whitespace();
        let first = words.next().unwrap_or("");
        match first {
            "rules" => {
                for w in words {
                    let Some((k, v)) = w.split_once('=') else {
                        r.problems.push(format!("line {at}: `{w}` is not KEY=VALUE"));
                        continue;
                    };
                    let Some(x) = v.parse::<f32>().ok().filter(|x| x.is_finite()) else {
                        r.problems.push(format!("line {at}: {k}={v} is not a number"));
                        continue;
                    };
                    match k {
                        "snap" => r.snap = x.max(0.0),
                        "min" => r.min = x.clamp(0.0, 1.0),
                        "gap" => r.gap = x.max(0.0),
                        "slope" => r.slope = x.clamp(0.0, 1.0),
                        "step" => r.step = x.max(0.0),
                        "ground" => r.ground = x.max(0.0),
                        "audit" => r.audit = x.max(0.1),
                        _ => r.problems.push(format!("line {at}: `{k}` is not a rules setting (snap, min, gap, slope, step, ground, audit)")),
                    }
                }
            }
            "piece" => {
                let Some(name) = words.next() else {
                    r.problems.push(format!("line {at}: `piece` needs a name"));
                    continue;
                };
                let mut k = Kind {
                    name: name.to_string(),
                    size: [1.0, 1.0, 1.0],
                    hp: 0.0,
                    free: false,
                    anchor: false,
                    loose: false,
                    decay: 0.0,
                    inset: None,
                    prefab: String::new(),
                    debris: String::new(),
                    bullet: 1.0,
                    upgrade: String::new(),
                    upgrade_cost: Vec::new(),
                    repair_cost: Vec::new(),
                    melee: 1.0,
                    sockets: Vec::new(),
                };
                let mut has_size = false;
                for w in words {
                    let Some((key, v)) = w.split_once('=') else {
                        r.problems.push(format!("line {at}: `{w}` is not KEY=VALUE"));
                        continue;
                    };
                    match key {
                        "size" => match nums(v, 3).filter(|s| s.iter().all(|x| *x > 0.0)) {
                            Some(s) => {
                                k.size = [s[0], s[1], s[2]];
                                has_size = true;
                            }
                            None => r.problems.push(format!("line {at}: size={v} is not three sizes above 0 like 4,0.5,4")),
                        },
                        "hp" | "decay" | "inset" => match v.parse::<f32>().ok().filter(|x| x.is_finite() && *x >= 0.0) {
                            Some(x) => match key {
                                "hp" => k.hp = x,
                                "decay" => k.decay = x,
                                _ => k.inset = Some(x),
                            },
                            None => r.problems.push(format!("line {at}: {key}={v} is not a number from 0 up")),
                        },
                        "free" | "anchor" | "loose" => match flag(v) {
                            Some(b) => match key {
                                "free" => k.free = b,
                                "anchor" => k.anchor = b,
                                _ => k.loose = b,
                            },
                            None => r.problems.push(format!("line {at}: {key}={v} is not 1 or 0")),
                        },
                        "prefab" => k.prefab = v.to_string(),
                        "debris" => k.debris = v.to_string(),
                        "upgrade" => k.upgrade = v.to_string(),
                        "bullet" => match v.parse::<f32>().ok().filter(|x| x.is_finite() && (0.0..=1.0).contains(x)) {
                            Some(x) => k.bullet = x,
                            None => r.problems.push(format!("line {at}: bullet={v} is not a share from 0 to 1")),
                        },
                        "upgrade_cost" => {
                            for item in v.split(',').filter(|c| !c.is_empty()) {
                                match item.split_once(':').and_then(|(name, n)| Some((name, n.parse::<u32>().ok()?))).filter(|(name, n)| !name.is_empty() && *n > 0) {
                                    Some((name, n)) => k.upgrade_cost.push((name.to_string(), n)),
                                    None => r.problems.push(format!("line {at}: upgrade_cost `{item}` is not ITEM:COUNT (like scrap:25,tape:4)")),
                                }
                            }
                        }
                        "melee" => match v.parse::<f32>().ok().filter(|x| x.is_finite() && (0.0..=1.0).contains(x)) {
                            Some(x) => k.melee = x,
                            None => r.problems.push(format!("line {at}: melee={v} is not a share from 0 to 1")),
                        },
                        "repair_cost" => {
                            for item in v.split(',').filter(|c| !c.is_empty()) {
                                match item.split_once(':').and_then(|(name, n)| Some((name, n.parse::<u32>().ok()?))).filter(|(name, n)| !name.is_empty() && *n > 0) {
                                    Some((name, n)) => k.repair_cost.push((name.to_string(), n)),
                                    None => r.problems.push(format!("line {at}: repair_cost `{item}` is not ITEM:COUNT (like scrap:15,tape:3)")),
                                }
                            }
                        }
                        _ => r.problems.push(format!("line {at}: `{key}` is not a piece setting (size, hp, free, anchor, loose, decay, inset, prefab, debris, bullet, upgrade, upgrade_cost, repair_cost, melee)")),
                    }
                }
                if !has_size {
                    r.problems.push(format!("line {at}: the piece `{name}` has no size=X,Y,Z (1 metre cube assumed)"));
                }
                if let Some(old) = r.kinds.iter().position(|o| o.name == k.name) {
                    r.problems.push(format!("line {at}: the piece `{name}` is defined twice (the later one counts)"));
                    r.kinds[old] = k;
                    sized[old] = has_size;
                } else {
                    r.kinds.push(k);
                    sized.push(has_size);
                }
            }
            "socket" => {
                let Some(k) = r.kinds.last_mut() else {
                    r.problems.push(format!("line {at}: a socket belongs under a `piece`"));
                    continue;
                };
                let Some(name) = words.next() else {
                    r.problems.push(format!("line {at}: `socket` needs a name"));
                    continue;
                };
                let mut s = Socket { name: name.to_string(), at: [0.0; 3], yaw: 0.0, ty: String::new(), mates: Vec::new(), carry: 0.9, carry_of: Vec::new(), turn: 180.0 };
                let mut problems: Vec<String> = Vec::new();
                for w in words {
                    let Some((key, v)) = w.split_once('=') else {
                        problems.push(format!("line {at}: `{w}` is not KEY=VALUE"));
                        continue;
                    };
                    match key {
                        "at" => match nums(v, 3) {
                            Some(p) => s.at = [p[0], p[1], p[2]],
                            None => problems.push(format!("line {at}: at={v} is not three numbers like 2,0.25,0")),
                        },
                        "yaw" | "turn" => match v.parse::<f32>().ok().filter(|x| x.is_finite()) {
                            Some(x) => {
                                if key == "yaw" {
                                    s.yaw = x;
                                } else {
                                    s.turn = x;
                                }
                            }
                            None => problems.push(format!("line {at}: {key}={v} is not a number of degrees")),
                        },
                        "type" => s.ty = v.to_string(),
                        "mates" => s.mates = v.split(',').filter(|m| !m.is_empty()).map(|m| m.to_string()).collect(),
                        "carry" => {
                            for item in v.split(',').filter(|c| !c.is_empty()) {
                                let (ty, num) = match item.split_once(':') {
                                    Some((t, x)) => (Some(t), x),
                                    None => (None, item),
                                };
                                match num.parse::<f32>().ok().filter(|x| (0.0..=1.0).contains(x)) {
                                    Some(x) => match ty {
                                        Some(t) => s.carry_of.push((t.to_string(), x)),
                                        None => s.carry = x,
                                    },
                                    None => problems.push(format!("line {at}: carry `{item}` is not a share from 0 to 1 (or TYPE:share)")),
                                }
                            }
                        }
                        _ => problems.push(format!("line {at}: `{key}` is not a socket setting (at, yaw, type, mates, carry, turn)")),
                    }
                }
                if s.ty.is_empty() {
                    problems.push(format!("line {at}: the socket `{name}` has no type=NAME"));
                }
                r.problems.extend(problems);
                k.sockets.push(s);
            }
            _ => r.problems.push(format!("line {at}: `{first}` is not rules, piece or socket")),
        }
    }
    for k in &r.kinds {
        if !k.upgrade.is_empty() && !r.kinds.iter().any(|o| o.name == k.upgrade) {
            r.problems.push(format!("the piece `{}` has upgrade={}, which is not a piece of these rules", k.name, k.upgrade));
        }
        if !k.upgrade_cost.is_empty() && k.upgrade.is_empty() {
            r.problems.push(format!("the piece `{}` has an upgrade_cost but no upgrade=PIECE", k.name));
        }
        if !k.free && k.sockets.is_empty() {
            r.problems.push(format!("the piece `{}` can never be placed: it is not free=1 and has no socket", k.name));
        }
        if let Some(list) = prefabs {
            for (what, p) in [("prefab", &k.prefab), ("debris", &k.debris)] {
                if !p.is_empty() && !list.contains(p) {
                    r.problems.push(format!("the piece `{}` has {what}={p}, which is not a prefab of this scene", k.name));
                }
            }
        }
    }
    r
}

thread_local! {
    /// The text of the rules last read and what it parsed to (found again by comparing the text, which is cheaper than hashing it).
    static CACHE: RefCell<Option<(String, Rc<Rules>)>> = const { RefCell::new(None) };
}

/// The entity with the `BuildRules` (the lowest uid) and its parsed rules, or None when the world has none.
pub fn rules_of(world: &mut World) -> Option<(Entity, Rc<Rules>)> {
    let mut best: Option<(u64, Entity, BuildRules)> = None;
    let _ = world.for_each_uid::<(&BuildRules,), _>(|e, uid, (r,)| {
        if best.as_ref().is_none_or(|b| uid < b.0) {
            best = Some((uid, e, *r));
        }
    });
    let (_, e, br) = best?;
    let text = world.strings().get(br.text.0);
    Some((
        e,
        CACHE.with(|c| {
            let mut c = c.borrow_mut();
            if let Some((k, r)) = c.as_ref() {
                if k == text {
                    return r.clone();
                }
            }
            let r = Rc::new(parse_rules(text, None));
            *c = Some((text.to_string(), r.clone()));
            r
        }),
    ))
}

// ------------------------------------------------------------------------------------------------ geometry

/// An oriented box in the ground plane with a height: where a piece is, for overlap tests.
struct Box3 {
    c: V3,
    h: V3,
    /// Its x axis in the ground plane is (cs, -sn), its z axis (sn, cs).
    cs: f32,
    sn: f32,
}

fn box_cs(kind: &Kind, rules: &Rules, pos: V3, cs: f32, sn: f32) -> Box3 {
    let inset = kind.inset.unwrap_or(rules.gap);
    Box3 { c: pos, h: [(kind.size[0] * 0.5 - inset).max(0.001), (kind.size[1] * 0.5 - inset).max(0.001), (kind.size[2] * 0.5 - inset).max(0.001)], cs, sn }
}

fn boxes_overlap(a: &Box3, b: &Box3) -> bool {
    if (a.c[1] - b.c[1]).abs() >= a.h[1] + b.h[1] {
        return false;
    }
    let d = [b.c[0] - a.c[0], b.c[2] - a.c[2]];
    let axes = [(a.cs, -a.sn), (a.sn, a.cs), (b.cs, -b.sn), (b.sn, b.cs)];
    for n in axes {
        let proj = |bx: &Box3| {
            let ax = (bx.cs, -bx.sn);
            let az = (bx.sn, bx.cs);
            bx.h[0] * (n.0 * ax.0 + n.1 * ax.1).abs() + bx.h[2] * (n.0 * az.0 + n.1 * az.1).abs()
        };
        if (n.0 * d[0] + n.1 * d[1]).abs() >= proj(a) + proj(b) {
            return false;
        }
    }
    true
}

/// A ray against a box turned by an angle of cosine `cs` and sine `sn`: the distance and the normal (facing the ray). A ray that starts inside sees nothing.
fn ray_box(o: V3, d: V3, max: f32, c: V3, cs: f32, sn: f32, h: V3) -> Option<(f32, V3)> {
    let (oo, dd) = (rot_cs(sub(o, c), cs, -sn), rot_cs(d, cs, -sn));
    let (mut tmin, mut tmax) = (0.0f32, max);
    let mut hit: Option<(usize, f32)> = None;
    for a in 0..3 {
        if dd[a].abs() < 1.0e-9 {
            if oo[a].abs() > h[a] {
                return None;
            }
            continue;
        }
        let (t1, t2) = ((-h[a] - oo[a]) / dd[a], (h[a] - oo[a]) / dd[a]);
        let (tn, tf) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
        if tn > tmin {
            tmin = tn;
            hit = Some((a, if dd[a] > 0.0 { -1.0 } else { 1.0 }));
        }
        tmax = tmax.min(tf);
        if tmin > tmax {
            return None;
        }
    }
    let (axis, sign) = hit?;
    let mut n = [0.0; 3];
    n[axis] = sign;
    Some((tmin, rot_cs(n, cs, sn)))
}

// ------------------------------------------------------------------------------------------------ the index

#[derive(Clone)]
struct Item {
    uid: u64,
    e: Entity,
    pos: V3,
    yaw: f32,
    /// The cosine and the sine of `yaw`.
    cs: f32,
    sn: f32,
    /// The index of the piece's kind in the rules, `NONE` for a `BuildSocket` entity.
    kind: usize,
    /// A `BuildSocket` entity's type.
    ty: String,
    /// A piece: rests on the ground. A socket entity: it is ground to the structure.
    anchor: bool,
    checked: bool,
    stability: f32,
    on: u64,
    alive: bool,
}

impl Item {
    #[allow(clippy::too_many_arguments)]
    fn new(uid: u64, e: Entity, pos: V3, yaw: f32, kind: usize, ty: String, anchor: bool, checked: bool, stability: f32, on: u64) -> Item {
        let (cs, sn) = cos_sin(yaw);
        Item { uid, e, pos, yaw, cs, sn, kind, ty, anchor, checked, stability, on, alive: true }
    }
}

struct Index {
    items: Vec<Item>,
    grid: FxMap<(i32, i32), Vec<u32>>,
    by_uid: FxMap<u64, u32>,
}

#[inline]
fn cell_of(v: f32) -> i32 {
    (v / CELL).floor() as i32
}

impl Index {
    /// All the pieces and socket entities of the world, or those inside `area` (a box in the world) only.
    fn build(world: &mut World, rules: &Rules, area: Option<(V3, V3)>) -> Index {
        let inside = |p: V3| area.is_none_or(|(lo, hi)| (0..3).all(|a| p[a] >= lo[a] && p[a] <= hi[a]));
        let mut kind_of: FxMap<u32, usize> = FxMap::default();
        let mut raw: Vec<(Entity, u64, V3, [f32; 4], BuildPiece)> = Vec::new();
        let _ = world.for_each_uid::<(&BuildPiece, &Transform), _>(|e, uid, (p, t)| {
            if t.pos.iter().all(|v| v.is_finite()) && inside(t.pos) {
                raw.push((e, uid, t.pos, t.rot, *p));
            }
        });
        let mut socks: Vec<(Entity, u64, V3, [f32; 4], BuildSocket)> = Vec::new();
        let _ = world.for_each_uid::<(&BuildSocket, &Transform), _>(|e, uid, (s, t)| {
            if t.pos.iter().all(|v| v.is_finite()) && inside(t.pos) {
                socks.push((e, uid, t.pos, t.rot, *s));
            }
        });
        let mut idx = Index { items: Vec::with_capacity(raw.len() + socks.len()), grid: FxMap::default(), by_uid: FxMap::default() };
        for (e, uid, pos, rot, p) in raw {
            let kind = match kind_of.get(&p.kind.0) {
                Some(k) => *k,
                None => {
                    let k = rules.kind(world.strings().get(p.kind.0)).unwrap_or(NONE - 1);
                    kind_of.insert(p.kind.0, k);
                    k
                }
            };
            if kind == NONE - 1 {
                continue; // (a piece of a kind the rules do not know is left alone)
            }
            idx.push(rules, Item::new(uid, e, pos, yaw_of(rot), kind, String::new(), p.anchored, p.checked, p.stability, p.on.0));
        }
        for (e, uid, pos, rot, s) in socks {
            let ty = world.strings().get(s.kind.0).to_string();
            idx.push(rules, Item::new(uid, e, pos, yaw_of(rot), NONE, ty, s.anchor, true, 1.0, 0));
        }
        idx
    }

    fn radius(rules: &Rules, it: &Item) -> f32 {
        if it.kind == NONE {
            0.0
        } else {
            let s = rules.kinds[it.kind].size;
            (s[0] * s[0] + s[2] * s[2]).sqrt() * 0.5
        }
    }

    fn push(&mut self, rules: &Rules, it: Item) -> usize {
        let i = self.items.len();
        let r = Index::radius(rules, &it);
        let (x0, x1, z0, z1) = (cell_of(it.pos[0] - r), cell_of(it.pos[0] + r), cell_of(it.pos[2] - r), cell_of(it.pos[2] + r));
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                self.grid.entry((cx, cz)).or_default().push(i as u32);
            }
        }
        self.by_uid.insert(it.uid, i as u32);
        self.items.push(it);
        i
    }

    /// The items whose cells touch the square of half side `r` round `p` (each once, in the order they were added).
    fn near(&self, p: V3, r: f32, out: &mut Vec<u32>) {
        out.clear();
        let (x0, x1, z0, z1) = (cell_of(p[0] - r), cell_of(p[0] + r), cell_of(p[2] - r), cell_of(p[2] + r));
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                if let Some(v) = self.grid.get(&(cx, cz)) {
                    out.extend_from_slice(v);
                }
            }
        }
        if out.len() > 1 {
            out.sort_unstable();
            out.dedup();
        }
    }

    /// The items whose cells the segment `a`..`b` may touch (each once, in the order they were added).
    fn along(&self, a: V3, b: V3, out: &mut Vec<u32>) {
        out.clear();
        let (x0, x1, z0, z1) = (cell_of(a[0].min(b[0])), cell_of(a[0].max(b[0])), cell_of(a[2].min(b[2])), cell_of(a[2].max(b[2])));
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                if let Some(v) = self.grid.get(&(cx, cz)) {
                    out.extend_from_slice(v);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }

    /// What a piece of `kind` at `pos`, turned by an angle of cosine `cs` and sine `sn`, is joined to: the neighbour and the share of its stability the piece takes from it. `skip` is the piece's own item (`NONE` for one not built yet).
    #[allow(clippy::too_many_arguments)]
    fn find_links(&self, rules: &Rules, kind: &Kind, pos: V3, cs: f32, sn: f32, skip: usize, out: &mut Vec<(usize, f32)>) {
        out.clear();
        let mut cand: Vec<u32> = Vec::new();
        for s in &kind.sockets {
            let wp = add(pos, rot_cs(s.at, cs, sn));
            self.near(wp, LINK, &mut cand);
            for &j in &cand {
                let j = j as usize;
                let it = &self.items[j];
                if j == skip || !it.alive {
                    continue;
                }
                if it.kind == NONE {
                    if dist(wp, it.pos) <= LINK && s.mates.iter().any(|m| *m == it.ty) {
                        out.push((j, s.carry_for(&it.ty)));
                    }
                } else {
                    let kj = &rules.kinds[it.kind];
                    if kj.loose {
                        continue;
                    }
                    for t in &kj.sockets {
                        let wt = add(it.pos, rot_cs(t.at, it.cs, it.sn));
                        if dist(wp, wt) <= LINK && (s.mates.contains(&t.ty) || t.mates.contains(&s.ty)) {
                            out.push((j, s.carry_for(&t.ty)));
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)));
        out.dedup_by_key(|x| x.0);
    }

    /// Work out the stability of the structure the `seeds` belong to (every piece joined to them, through sockets) and say which pieces changed and which are under the rules' minimum.
    fn settle(&mut self, rules: &Rules, seeds: &[usize]) -> (Vec<usize>, Vec<usize>) {
        let mut sorted: Vec<usize> = seeds.to_vec();
        sorted.sort_by_key(|&i| self.items[i].uid);
        sorted.dedup();
        // the nodes: the pieces of the structure (found from the seeds by following the links) and the anchoring socket entities they touch
        let mut nodes: Vec<usize> = Vec::new();
        let mut node_of: FxMap<usize, u32> = FxMap::default();
        let mut queue: VecDeque<u32> = VecDeque::new();
        for i in sorted {
            let it = &self.items[i];
            if it.alive && it.kind != NONE && !rules.kinds[it.kind].loose && !node_of.contains_key(&i) {
                node_of.insert(i, nodes.len() as u32);
                queue.push_back(nodes.len() as u32);
                nodes.push(i);
            }
        }
        // (a piece, what it takes from, the share)
        let mut edges: Vec<(u32, u32, f32)> = Vec::new();
        let mut tmp: Vec<(usize, f32)> = Vec::new();
        while let Some(n) = queue.pop_front() {
            let i = nodes[n as usize];
            let (pos, cs, sn, k) = (self.items[i].pos, self.items[i].cs, self.items[i].sn, self.items[i].kind);
            self.find_links(rules, &rules.kinds[k], pos, cs, sn, i, &mut tmp);
            for &(j, w) in &tmp {
                let nj = match node_of.get(&j) {
                    Some(&x) => x,
                    None => {
                        let x = nodes.len() as u32;
                        node_of.insert(j, x);
                        nodes.push(j);
                        if self.items[j].kind != NONE {
                            queue.push_back(x);
                        }
                        x
                    }
                };
                edges.push((n, nj, w));
            }
        }
        // the ground: pieces resting on it, and anchoring socket entities
        let count = nodes.len();
        let mut stab = vec![0.0f32; count];
        let mut heap: BinaryHeap<(u32, Reverse<u64>, u32)> = BinaryHeap::new();
        for (n, &i) in nodes.iter().enumerate() {
            let it = &self.items[i];
            let root = if it.kind == NONE { it.anchor } else { it.anchor && rules.kinds[it.kind].anchor };
            if root {
                stab[n] = 1.0;
                heap.push((1.0f32.to_bits(), Reverse(it.uid), n as u32));
            }
        }
        // who takes from whom, by the one they take from
        let mut start = vec![0u32; count + 1];
        for &(_, src, _) in &edges {
            start[src as usize + 1] += 1;
        }
        for n in 0..count {
            start[n + 1] += start[n];
        }
        let mut fill = start.clone();
        let mut out = vec![(0u32, 0.0f32); edges.len()];
        for &(me, src, w) in &edges {
            out[fill[src as usize] as usize] = (me, w);
            fill[src as usize] += 1;
        }
        while let Some((bits, _, n)) = heap.pop() {
            if stab[n as usize].to_bits() != bits {
                continue;
            }
            let s = f32::from_bits(bits);
            for &(me, w) in &out[start[n as usize] as usize..start[n as usize + 1] as usize] {
                let cand = s * w;
                if cand > stab[me as usize] {
                    stab[me as usize] = cand;
                    heap.push((cand.to_bits(), Reverse(self.items[nodes[me as usize]].uid), me));
                }
            }
        }
        let (mut changed, mut collapse) = (Vec::new(), Vec::new());
        for (n, &i) in nodes.iter().enumerate() {
            if self.items[i].kind == NONE {
                continue;
            }
            let s = stab[n];
            if (s - self.items[i].stability).abs() > 1.0e-6 {
                self.items[i].stability = s;
                changed.push(i);
            }
            if s < rules.min {
                collapse.push(i);
            }
        }
        collapse.sort_by_key(|&i| self.items[i].uid);
        (changed, collapse)
    }

    /// What a piece put at this place would be held with (0 to 1).
    fn prospect(&self, rules: &Rules, kind: &Kind, pos: V3, yaw: f32, anchored: bool) -> f32 {
        if kind.loose || (kind.anchor && anchored) {
            return 1.0;
        }
        let mut links: Vec<(usize, f32)> = Vec::new();
        let (cs, sn) = cos_sin(yaw);
        self.find_links(rules, kind, pos, cs, sn, NONE, &mut links);
        let mut best = 0.0f32;
        for (j, w) in links {
            let it = &self.items[j];
            // (a socket entity is solid ground to what joins it only while it anchors: a loose one is a place to snap to and holds nothing, which `settle` says too: without this a piece at a loose socket was "valid", paid for, and collapsed at once)
            let s = if it.kind == NONE { if it.anchor { 1.0 } else { 0.0 } } else { it.stability };
            best = best.max(s * w);
        }
        best
    }
}

/// Does a piece of `kind` at `pos` rest on the ground? Five rays down (the middle and four corners), three of which, the middle among them, must meet a surface that is not too steep.
fn grounded(rw: &RayWorld, who: Filter, rules: &Rules, kind: &Kind, pos: V3, yaw: f32) -> bool {
    let bottom = pos[1] - kind.size[1] * 0.5;
    let (hx, hz) = (kind.size[0] * 0.4, kind.size[2] * 0.4);
    let (cs, sn) = cos_sin(yaw);
    let mut hits = 0;
    let mut middle = false;
    for (k, (sx, sz)) in [(0.0, 0.0), (hx, hz), (-hx, hz), (hx, -hz), (-hx, -hz)].into_iter().enumerate() {
        let p = add(pos, rot_cs([sx, 0.0, sz], cs, sn));
        let from = [p[0], bottom + 0.3, p[2]];
        if rw.cast(from, [0.0, -1.0, 0.0], 0.3 + rules.ground, who, 0).is_some_and(|h| h.normal[1] >= rules.slope) {
            hits += 1;
            if k == 0 {
                middle = true;
            }
        }
    }
    middle && hits >= 3
}

// ------------------------------------------------------------------------------------------------ the tool

struct Cup {
    uid: u64,
    e: Entity,
    pos: V3,
    radius: f32,
    owner: u32,
    exclusive: bool,
    protect: f32,
}

struct Eval {
    ok: bool,
    why: &'static str,
    pos: V3,
    yaw: f32,
    snapped: u64,
    stability: f32,
    anchored: bool,
    on: u64,
    aimed: bool,
}

struct Ctx<'a> {
    rules: &'a Rules,
    idx: &'a Index,
    rw: &'a RayWorld,
    who: Filter,
    cups: &'a [Cup],
    /// The owner of each tool whose holder is in a party, and the party (`BuildTool.party`, docs/PARTIES.md): empty when there is no land that is somebody's to keep out of.
    owners: &'a [(u32, u32)],
}

impl Ctx<'_> {
    /// Are the owners `a` and `b` in the same party? Land of one is land of the other.
    fn same_party(&self, a: u32, b: u32) -> bool {
        let party = |o: u32| self.owners.iter().find(|p| p.0 == o).map_or(0, |p| p.1);
        let pa = party(a);
        pa != 0 && pa == party(b)
    }
}

fn heading(d: V3) -> f32 {
    (detmath::atan2(-(d[0] as f64), -(d[2] as f64)) * (180.0 / std::f64::consts::PI)) as f32
}

/// May a piece of `kind` stand here? Fills in `why` when not, and what it would be held with.
#[allow(clippy::too_many_arguments)]
fn judge(cx: &Ctx, kind: &Kind, owner: u32, pos: V3, yaw: f32, free: bool, on: u64, snapped: u64) -> Eval {
    let mut ev = Eval { ok: false, why: "", pos, yaw, snapped, stability: 0.0, anchored: false, on, aimed: true };
    // not through another piece
    let (cs, sn) = cos_sin(yaw);
    let mine = box_cs(kind, cx.rules, pos, cs, sn);
    let reach = (kind.size[0] * kind.size[0] + kind.size[2] * kind.size[2]).sqrt() * 0.5;
    let mut cand: Vec<u32> = Vec::new();
    cx.idx.near(pos, reach, &mut cand);
    for &j in &cand {
        let it = &cx.idx.items[j as usize];
        if it.kind == NONE || !it.alive {
            continue;
        }
        if boxes_overlap(&mine, &box_cs(&cx.rules.kinds[it.kind], cx.rules, it.pos, it.cs, it.sn)) {
            ev.why = "blocked";
            return ev;
        }
    }
    // not on somebody else's land
    for c in cx.cups {
        if c.exclusive && c.owner != 0 && c.owner != owner && dist(c.pos, pos) <= c.radius && !cx.same_party(c.owner, owner) {
            ev.why = "not_yours";
            return ev;
        }
    }
    ev.anchored = kind.anchor && grounded(cx.rw, cx.who, cx.rules, kind, pos, yaw);
    if free && kind.anchor && !ev.anchored {
        ev.why = "no_ground";
        return ev;
    }
    ev.stability = cx.idx.prospect(cx.rules, kind, pos, yaw, ev.anchored);
    if ev.stability < cx.rules.min {
        ev.why = "unstable";
        return ev;
    }
    ev.ok = true;
    ev
}

/// Where would the tool's piece go, and may it?
fn evaluate(cx: &Ctx, tool: &BuildTool, tool_uid: u64, xf: &Transform, kind: &Kind) -> Eval {
    let none = Eval { ok: false, why: "no_aim", pos: tool.pos, yaw: tool.yaw, snapped: 0, stability: 0.0, anchored: false, on: 0, aimed: false };
    let o = xf.pos;
    let z = axes_of(normalize_q(xf.rot))[2];
    let d = [-z[0], -z[1], -z[2]];
    let reach = if tool.reach.is_finite() { tool.reach.clamp(0.1, 200.0) } else { 6.0 };
    // what the aim meets: the world, or a piece
    let mut best: Option<(f32, V3, Option<usize>)> = None;
    if let Some(h) = cx.rw.cast(o, d, reach, cx.who, tool_uid) {
        best = Some((h.t, h.normal, None));
    }
    let mut along: Vec<u32> = Vec::new();
    cx.idx.along(o, add(o, [d[0] * reach, d[1] * reach, d[2] * reach]), &mut along);
    for &j in &along {
        let it = &cx.idx.items[j as usize];
        if it.kind == NONE || !it.alive {
            continue;
        }
        let k = &cx.rules.kinds[it.kind];
        if let Some((t, n)) = ray_box(o, d, reach, it.pos, it.cs, it.sn, [k.size[0] * 0.5, k.size[1] * 0.5, k.size[2] * 0.5]) {
            let better = match best {
                None => true,
                Some((bt, _, bp)) => t < bt || (t == bt && bp.is_some_and(|p| it.uid < cx.idx.items[p].uid)),
            };
            if better {
                best = Some((t, n, Some(j as usize)));
            }
        }
    }
    let Some((t, normal, piece)) = best else { return none };
    let aim = add(o, [d[0] * t, d[1] * t, d[2] * t]);
    // the sockets round the aim that this piece can join
    let mut cand: Vec<u32> = Vec::new();
    cx.idx.near(aim, cx.rules.snap + 0.01, &mut cand);
    // (distance from the aim, uid of the neighbour, the neighbour, its socket, this piece's socket)
    let mut options: Vec<(f32, u64, usize, usize, usize)> = Vec::new();
    for &j in &cand {
        let it = &cx.idx.items[j as usize];
        if !it.alive {
            continue;
        }
        if it.kind == NONE {
            let dd = dist(it.pos, aim);
            if dd <= cx.rules.snap {
                for (gi, g) in kind.sockets.iter().enumerate() {
                    if g.mates.iter().any(|m| *m == it.ty) {
                        options.push((dd, it.uid, j as usize, 0, gi));
                    }
                }
            }
        } else {
            let kj = &cx.rules.kinds[it.kind];
            if kj.loose {
                continue;
            }
            for (ti, t) in kj.sockets.iter().enumerate() {
                let wt = add(it.pos, rot_cs(t.at, it.cs, it.sn));
                let dd = dist(wt, aim);
                if dd > cx.rules.snap {
                    continue;
                }
                for (gi, g) in kind.sockets.iter().enumerate() {
                    if g.mates.contains(&t.ty) || t.mates.contains(&g.ty) {
                        options.push((dd, it.uid, j as usize, ti, gi));
                    }
                }
            }
        }
    }
    options.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.3.cmp(&b.3)).then(a.4.cmp(&b.4)));
    let mut first: Option<Eval> = None;
    for &(_, _, j, ti, gi) in options.iter().take(TRIES) {
        let it = &cx.idx.items[j];
        let g = &kind.sockets[gi];
        // where the neighbour's socket is, and which way it faces
        let (wt, face) = if it.kind == NONE {
            (it.pos, it.yaw)
        } else {
            let t = &cx.rules.kinds[it.kind].sockets[ti];
            (add(it.pos, rot_cs(t.at, it.cs, it.sn)), it.yaw + t.yaw)
        };
        let mut yaw = wrap(face + g.turn - g.yaw);
        if tool.flip {
            yaw = wrap(yaw + 180.0);
        }
        let pos = sub(wt, rot_y(g.at, yaw));
        let ev = judge(cx, kind, tool.owner, pos, yaw, false, 0, it.uid);
        if ev.ok {
            return ev;
        }
        if first.is_none() {
            first = Some(ev);
        }
    }
    if let Some(ev) = first {
        return ev;
    }
    // open ground
    let mut ev = Eval { ok: false, why: "no_socket", pos: aim, yaw: tool.yaw, snapped: 0, stability: 0.0, anchored: false, on: 0, aimed: true };
    if !kind.free {
        return ev;
    }
    if normal[1] < cx.rules.slope {
        ev.why = "no_ground";
        return ev;
    }
    let on = match piece {
        Some(j) if kind.loose => cx.idx.items[j].uid,
        Some(_) => {
            ev.why = "no_ground";
            return ev;
        }
        None => 0,
    };
    let mut yaw = heading(d) + if tool.turn.is_finite() { tool.turn } else { 0.0 };
    if cx.rules.step > 0.0 {
        yaw = (yaw / cx.rules.step).round() * cx.rules.step;
    }
    let yaw = wrap(yaw);
    let pos = [aim[0], aim[1] + kind.size[1] * 0.5, aim[2]];
    judge(cx, kind, tool.owner, pos, yaw, true, on, 0)
}

// ------------------------------------------------------------------------------------------------ changes to the world

fn num(v: f32) -> Json {
    Json::Float(v as f64)
}

fn json_v3(v: V3) -> Json {
    Json::arr(v.map(num))
}

/// Make a copy of the prefab `name` (if the world has it) at a place; the root gets the uid `world.next_uid()`.
fn spawn_prefab(world: &mut World, scripts: &mut ScriptSet, name: &str, pos: V3, yaw: f32, more: &[(&str, Json)]) -> Option<u64> {
    spawn_prefab_rot(world, scripts, name, pos, yaw_quat(yaw), more)
}

/// As `spawn_prefab` with a rotation instead of a turn: any system that makes a copy of a prefab at a place (the corpse of a player) uses this.
pub(crate) fn spawn_prefab_rot(world: &mut World, scripts: &mut ScriptSet, name: &str, pos: V3, rot: [f32; 4], more: &[(&str, Json)]) -> Option<u64> {
    if name.is_empty() || !scripts.prefabs.contains_key(name) {
        return None;
    }
    let mut ov: Vec<(String, Json)> = Vec::new();
    crate::prefab::merge_component(&mut ov, "Transform", &Json::obj([("pos", json_v3(pos)), ("rot", Json::arr(rot.map(num)))]));
    for (c, v) in more {
        crate::prefab::merge_component(&mut ov, c, v);
    }
    let plan = crate::prefab::plan(&scripts.prefabs, name, &ov).ok()?;
    let base = world.next_uid();
    let cmds = crate::prefab::spawn_commands(&plan, base);
    crate::command::apply_batch(world, scripts, &cmds).ok()?;
    Some(base)
}

/// Build the piece the tool judged: from the kind's prefab, or a bare solid box with hit points. Returns its uid.
fn spawn_piece(world: &mut World, scripts: &mut ScriptSet, kind: &Kind, net_kind: u32, owner: u32, ev: &Eval) -> Option<u64> {
    let piece = Json::obj([
        ("kind", Json::str(&kind.name)),
        ("owner", Json::from(owner as i64)),
        ("stability", num(ev.stability)),
        ("anchored", Json::Bool(ev.anchored)),
        ("checked", Json::Bool(true)),
        ("on", Json::from(ev.on as i64)),
    ]);
    let mut more: Vec<(&str, Json)> = vec![("BuildPiece", piece)];
    if kind.hp > 0.0 {
        more.push(("HitPoints", Json::obj([("value", num(kind.hp)), ("max", num(kind.hp)), ("last", num(kind.hp))])));
    }
    if kind.bullet < 1.0 {
        more.push(("Resist", Json::obj([("bullet", num(kind.bullet))])));
    }
    // (a piece is told to the players of a game server (docs/GAME_SERVER.md) as a still thing of class 3, its kind the number of the piece in the rules from 1; a prefab that has its own `NetEntity` keeps it)
    let net = Json::obj([("class", Json::from(3i64)), ("kind", Json::from(net_kind as i64))]);
    if !kind.prefab.is_empty() && scripts.prefabs.contains_key(&kind.prefab) {
        let has_net = crate::prefab::plan(&scripts.prefabs, &kind.prefab, &[]).is_ok_and(|p| p[0].components.iter().any(|(n, _)| n == "NetEntity"));
        if !has_net {
            more.push(("NetEntity", net));
        }
        return spawn_prefab(world, scripts, &kind.prefab, ev.pos, ev.yaw, &more);
    }
    more.push(("NetEntity", net));
    let mut comps: Vec<(String, Json)> = Vec::new();
    crate::prefab::merge_component(&mut comps, "Name", &Json::obj([("value", Json::str(&kind.name))]));
    crate::prefab::merge_component(&mut comps, "Transform", &Json::obj([("pos", json_v3(ev.pos)), ("rot", Json::arr(yaw_quat(ev.yaw).map(num)))]));
    crate::prefab::merge_component(&mut comps, "Collider", &Json::obj([("shape", Json::from(1i64)), ("size", json_v3(kind.size))]));
    for (c, v) in &more {
        crate::prefab::merge_component(&mut comps, c, v);
    }
    let base = world.next_uid();
    crate::command::apply_batch(world, scripts, &[crate::command::Command::Spawn { uid: Some(base), components: comps }]).ok()?;
    Some(base)
}

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// What the pieces and socket entities of the index look like, as a count and a number that changes when one is added, removed or moved (not the hit points or the stability).
fn fingerprint(idx: &Index, skip: &BTreeSet<u64>) -> (u32, u32) {
    let (mut n, mut x) = (0u32, 0u64);
    for it in &idx.items {
        if !it.alive || skip.contains(&it.uid) {
            continue;
        }
        n += 1;
        x ^= mix(it.uid) ^ mix(it.pos[0].to_bits() as u64 | (it.pos[1].to_bits() as u64) << 32) ^ mix(it.pos[2].to_bits() as u64 ^ (it.kind as u64) << 32);
    }
    (n, (x ^ (x >> 32)) as u32)
}

fn gather_cups(world: &mut World) -> Vec<Cup> {
    let mut cups: Vec<Cup> = Vec::new();
    let _ = world.for_each_uid::<(&BuildUpkeep, &Transform), _>(|e, uid, (c, t)| {
        if t.pos.iter().all(|v| v.is_finite()) {
            cups.push(Cup { uid, e, pos: t.pos, radius: c.radius.max(0.0), owner: c.owner, exclusive: c.exclusive, protect: c.protect });
        }
    });
    cups.sort_by_key(|c| c.uid);
    cups
}

/// Take the pieces in `doomed` out of the world (the reason is the event each gets) and settle what is left: the structures round a destroyed piece are worked out again, pieces that are then
/// under the minimum collapse, and loose pieces whose base is gone fall. `seeds` are pieces whose surroundings changed (a new piece); with `global` every structure is settled.
#[allow(clippy::too_many_arguments)]
fn process(world: &mut World, scripts: &mut ScriptSet, rules: &Rules, idx: &mut Index, doomed: Vec<(usize, &'static str)>, mut seeds: Vec<usize>, global: bool, events: &mut Events) {
    if global {
        seeds = (0..idx.items.len()).filter(|&i| idx.items[i].alive && idx.items[i].kind != NONE).collect();
    }
    let mut removed: Vec<(usize, &'static str)> = Vec::new();
    let mut touched: Vec<usize> = Vec::new();
    let mut queue = doomed;
    loop {
        let mut gone: BTreeSet<u64> = BTreeSet::new();
        for (i, why) in queue.drain(..) {
            let it = &idx.items[i];
            if !it.alive || it.kind == NONE {
                continue;
            }
            if why == "build_destroyed" {
                // what it joined needs a new look
                let (pos, cs, sn, k) = (it.pos, it.cs, it.sn, it.kind);
                let mut links: Vec<(usize, f32)> = Vec::new();
                idx.find_links(rules, &rules.kinds[k], pos, cs, sn, i, &mut links);
                seeds.extend(links.iter().map(|l| l.0).filter(|&j| idx.items[j].kind != NONE));
            }
            gone.insert(idx.items[i].uid);
            idx.items[i].alive = false;
            removed.push((i, why));
        }
        if !gone.is_empty() {
            for i in 0..idx.items.len() {
                let it = &idx.items[i];
                if it.alive && it.kind != NONE && rules.kinds[it.kind].loose && it.on != 0 && gone.contains(&it.on) {
                    queue.push((i, "build_fell"));
                }
            }
        }
        let (changed, collapse) = idx.settle(rules, &seeds);
        touched.extend(changed);
        seeds.clear();
        queue.extend(collapse.into_iter().map(|i| (i, "build_collapse")));
        if queue.is_empty() {
            break;
        }
    }
    removed.sort_by_key(|r| idx.items[r.0].uid);
    let mut uids: Vec<u64> = Vec::new();
    let mut rubble: Vec<(String, V3, f32)> = Vec::new();
    for &(i, why) in &removed {
        let it = &idx.items[i];
        events.push((why.to_string(), it.uid, Some(it.pos)));
        uids.push(it.uid);
        rubble.push((rules.kinds[it.kind].debris.clone(), it.pos, it.yaw));
    }
    if !uids.is_empty() {
        despawn_trees(world, uids);
    }
    for (name, pos, yaw) in rubble {
        spawn_prefab(world, scripts, &name, pos, yaw, &[]);
    }
    touched.sort_by_key(|&i| idx.items[i].uid);
    touched.dedup();
    for i in touched {
        let it = &idx.items[i];
        if it.alive {
            if let Some(p) = world.get_mut::<BuildPiece>(it.e) {
                p.stability = it.stability;
            }
        }
    }
}

/// Is it time to look at the whole world, or did a piece die? Then: take the dead pieces out, let the pieces decay, count what the cupboards cover, find out which new anchors rest on the ground,
/// and settle the structures that need it.
#[allow(clippy::too_many_arguments)]
fn sweep(world: &mut World, scripts: &mut ScriptSet, rules: &Rules, rules_e: Entity, died: &[u64], audit: bool, elapsed: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    let mut idx = Index::build(world, rules, None);
    let nobody = BTreeSet::new();
    let before = fingerprint(&idx, &nobody);
    let known = world.get::<BuildRules>(rules_e).map(|r| (r.pieces, r.stamp));
    let mut global = known != Some(before);
    let mut doomed: Vec<(usize, &'static str)> = Vec::new();
    let mut marked: FxSet<usize> = FxSet::default();
    for u in died {
        if let Some(&i) = idx.by_uid.get(u) {
            if idx.items[i as usize].kind != NONE && marked.insert(i as usize) {
                doomed.push((i as usize, "build_destroyed"));
            }
        }
    }
    if audit {
        // cupboards: what they cover, and what that keeps from decaying
        let mut cups = gather_cups(world);
        let mut protected: FxSet<usize> = FxSet::default();
        let mut cand: Vec<u32> = Vec::new();
        for c in cups.iter_mut() {
            idx.near(c.pos, c.radius, &mut cand);
            let mut covered = 0u32;
            for &j in &cand {
                let it = &idx.items[j as usize];
                if it.kind != NONE && it.alive && dist(it.pos, c.pos) <= c.radius {
                    covered += 1;
                    if c.protect > 0.0 {
                        protected.insert(j as usize);
                    }
                }
            }
            if let Some(w) = world.get_mut::<BuildUpkeep>(c.e) {
                w.covered = covered;
                w.protect = (w.protect - elapsed).max(0.0);
            }
        }
        // decay, the dead, loose pieces without their base, anchors nobody has looked at
        let mut unchecked: Vec<usize> = Vec::new();
        for i in 0..idx.items.len() {
            let it = &idx.items[i];
            if it.kind == NONE || !it.alive {
                continue;
            }
            let k = &rules.kinds[it.kind];
            let mut dead = false;
            if let Some(h) = world.get_mut::<HitPoints>(it.e) {
                if k.decay > 0.0 && !h.dead && !protected.contains(&i) {
                    h.value = (h.value - k.decay * elapsed).max(0.0);
                    // (decay is not a hit: `last` follows, so that `HitPoints.hurt` is the time since a piece was really hit, which a repair asks, docs/DEPTH.md)
                    h.last = h.value;
                }
                dead = h.dead;
            }
            if dead && marked.insert(i) {
                doomed.push((i, "build_destroyed"));
            }
            if k.loose && it.on != 0 && world.entity(it.on).is_none() && marked.insert(i) {
                doomed.push((i, "build_fell"));
            }
            if k.anchor && !it.checked {
                unchecked.push(i);
            }
        }
        if !unchecked.is_empty() {
            let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
            for &i in &unchecked {
                for a in 0..3 {
                    lo[a] = lo[a].min(idx.items[i].pos[a] - 4.0);
                    hi[a] = hi[a].max(idx.items[i].pos[a] + 4.0);
                }
            }
            let except: BTreeSet<u64> = idx.items.iter().filter(|it| it.kind != NONE).map(|it| it.uid).collect();
            let rw = RayWorld::gather_except(world, fields, meshes, Some((lo, hi)), &except);
            let layers = Layers::gather(world);
            for i in unchecked {
                let it = &idx.items[i];
                let g = grounded(&rw, layers.of(it.uid), rules, &rules.kinds[it.kind], it.pos, it.yaw);
                if let Some(p) = world.get_mut::<BuildPiece>(it.e) {
                    p.anchored = g;
                    p.checked = true;
                }
                idx.items[i].anchor = g;
                idx.items[i].checked = true;
            }
            global = true;
        }
    }
    if doomed.is_empty() && !global {
        return;
    }
    process(world, scripts, rules, &mut idx, doomed, Vec::new(), global, events);
    if audit || known == Some(before) {
        let after = fingerprint(&idx, &nobody);
        if let Some(r) = world.get_mut::<BuildRules>(rules_e) {
            r.pieces = after.0;
            r.stamp = after.1;
        }
    }
}

fn reset_tool(world: &mut World, e: Entity) {
    if let Some(w) = world.get_mut::<BuildTool>(e) {
        w.valid = false;
        w.why = Sym(0);
        w.snapped = Uid(0);
        w.stability = 0.0;
        w.place = false;
    }
}

/// The tools: where each would build and whether it may, the ghost that follows, and the pieces whose `place` is set.
#[allow(clippy::too_many_arguments)]
fn run_tools(world: &mut World, scripts: &mut ScriptSet, rules: &Rules, rules_e: Entity, tools: &[(Entity, u64, BuildTool, Transform)], fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    let active: Vec<usize> = (0..tools.len()).filter(|&i| tools[i].2.kind.0 != 0).collect();
    for (i, t) in tools.iter().enumerate() {
        if !active.contains(&i) && (t.2.valid || t.2.why.0 != 0 || t.2.place || t.2.snapped.0 != 0) {
            reset_tool(world, t.0);
        }
    }
    if active.is_empty() {
        return;
    }
    // the box everything the tools can touch lies in
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for &i in &active {
        let (_, _, tool, xf) = &tools[i];
        let z = axes_of(normalize_q(xf.rot))[2];
        let reach = if tool.reach.is_finite() { tool.reach.clamp(0.1, 200.0) } else { 6.0 };
        let end = [xf.pos[0] - z[0] * reach, xf.pos[1] - z[1] * reach, xf.pos[2] - z[2] * reach];
        let (a, b) = segment_box(xf.pos, end, rules.snap + 12.0);
        for k in 0..3 {
            lo[k] = lo[k].min(a[k]);
            hi[k] = hi[k].max(b[k]);
        }
    }
    if !lo.iter().chain(hi.iter()).all(|v| v.is_finite()) {
        return;
    }
    let mut idx = Index::build(world, rules, Some((lo, hi)));
    let except: BTreeSet<u64> = idx.items.iter().filter(|it| it.kind != NONE).map(|it| it.uid).collect();
    let rw = RayWorld::gather_except(world, fields, meshes, Some((lo, hi)), &except);
    let layers = Layers::gather(world);
    let cups = gather_cups(world);
    // (land of a party's owner is every member's: which owners are in which party is only looked up when there is land that is somebody's)
    let owners: Vec<(u32, u32)> = if cups.iter().any(|c| c.exclusive && c.owner != 0) { tools.iter().filter(|t| t.2.party != 0 && t.2.owner != 0).map(|t| (t.2.owner, t.2.party)).collect() } else { Vec::new() };
    let mut fresh: BTreeSet<u64> = BTreeSet::new();
    for &t in &active {
        let (e, uid, tool, xf) = (tools[t].0, tools[t].1, tools[t].2, tools[t].3);
        let name = world.strings().get(tool.kind.0).to_string();
        let ki = rules.kind(&name);
        let ev = match ki {
            Some(k) => {
                let cx = Ctx { rules, idx: &idx, rw: &rw, who: layers.of(uid), cups: &cups, owners: &owners };
                evaluate(&cx, &tool, uid, &xf, &rules.kinds[k])
            }
            None => Eval { ok: false, why: "unknown_piece", pos: tool.pos, yaw: tool.yaw, snapped: 0, stability: 0.0, anchored: false, on: 0, aimed: false },
        };
        let (mut placed, mut built) = (tool.placed.0, tool.built);
        if tool.place {
            match ki.filter(|_| ev.ok) {
                Some(k) => {
                    let kind = &rules.kinds[k];
                    if let Some(new) = spawn_piece(world, scripts, kind, k as u32 + 1, tool.owner, &ev) {
                        if let Some(ne) = world.entity(new) {
                            idx.push(rules, Item::new(new, ne, ev.pos, ev.yaw, k, String::new(), ev.anchored, true, ev.stability, ev.on));
                            fresh.insert(new);
                            placed = new;
                            built = built.wrapping_add(1);
                            events.push(("build_placed".to_string(), new, Some(ev.pos)));
                        }
                    }
                }
                None => events.push(("build_refused".to_string(), uid, Some(ev.pos))),
            }
        }
        let why = if ev.why.is_empty() { Sym(0) } else { Sym(world.strings_mut().intern(ev.why)) };
        if let Some(w) = world.get_mut::<BuildTool>(e) {
            w.valid = ev.ok;
            w.why = why;
            w.pos = ev.pos;
            w.yaw = ev.yaw;
            w.snapped = Uid(ev.snapped);
            w.stability = ev.stability;
            w.place = false;
            w.placed = Uid(placed);
            w.built = built;
        }
        if tool.ghost.0 != 0 && ev.aimed {
            if let Some(ge) = world.entity(tool.ghost.0) {
                if let Some(t) = world.get_mut::<Transform>(ge) {
                    t.pos = ev.pos;
                    t.rot = yaw_quat(ev.yaw);
                }
            }
        }
    }
    if fresh.is_empty() {
        return;
    }
    // new pieces can hold up what hung loose: settle the structures they joined, over all the pieces
    let mut full = Index::build(world, rules, None);
    let known = world.get::<BuildRules>(rules_e).map(|r| (r.pieces, r.stamp));
    let was = fingerprint(&full, &fresh);
    let seeds: Vec<usize> = fresh.iter().filter_map(|u| full.by_uid.get(u).map(|&i| i as usize)).collect();
    process(world, scripts, rules, &mut full, Vec::new(), seeds, false, events);
    if known == Some(was) {
        let after = fingerprint(&full, &BTreeSet::new());
        if let Some(r) = world.get_mut::<BuildRules>(rules_e) {
            r.pieces = after.0;
            r.stamp = after.1;
        }
    }
}

/// Whose bag an upgrader uses: the one it names, its own, or its parent's.
fn bag_of(world: &World, e: Entity, named: Uid) -> Option<Entity> {
    let has = |b: Entity| world.get::<Inventory>(b).is_some();
    if named.0 != 0 {
        return world.entity(named.0).filter(|b| has(*b));
    }
    if has(e) {
        return Some(e);
    }
    world.get::<Parent>(e).and_then(|p| world.entity(p.target.0)).filter(|b| has(*b))
}

/// The building piece the entity `aim_e` (the avatar's head, a camera) looks at within `reach` metres, the nearest on the line and not behind a wall of the world: its uid, its entity, its place and its index in the rules' kinds.
/// What an upgrade (`upgrade_one`) and a repair (`depth.rs`) both ask first.
pub(crate) fn aimed_piece(world: &mut World, rules: &Rules, fields: &Terrains, meshes: &MeshStore, aim_e: Entity, uid: u64, reach: f32) -> Result<(u64, Entity, V3, usize), &'static str> {
    let xf = world.get::<Transform>(aim_e).copied().ok_or("no_aim")?;
    if !xf.pos.iter().chain(xf.rot.iter()).all(|v| v.is_finite()) {
        return Err("no_aim");
    }
    let z = axes_of(normalize_q(xf.rot))[2];
    let d = [-z[0], -z[1], -z[2]];
    let reach = if reach.is_finite() { reach.clamp(0.1, 50.0) } else { 5.0 };
    let o = xf.pos;
    let end = add(o, [d[0] * reach, d[1] * reach, d[2] * reach]);
    let (lo, hi) = segment_box(o, end, rules.snap + 2.0);
    let idx = Index::build(world, rules, Some((lo, hi)));
    let except: BTreeSet<u64> = idx.items.iter().filter(|it| it.kind != NONE).map(|it| it.uid).collect();
    let rw = RayWorld::gather_except(world, fields, meshes, Some((lo, hi)), &except);
    let layers = Layers::gather(world);
    let aim_uid = world.uid_of(aim_e).unwrap_or(uid);
    // the nearest piece on the line, and not behind a wall of the world
    let mut along: Vec<u32> = Vec::new();
    idx.along(o, end, &mut along);
    let mut best: Option<(f32, usize)> = None;
    for &j in &along {
        let it = &idx.items[j as usize];
        if it.kind == NONE || !it.alive {
            continue;
        }
        let k = &rules.kinds[it.kind];
        if let Some((t, _)) = ray_box(o, d, reach, it.pos, it.cs, it.sn, [k.size[0] * 0.5, k.size[1] * 0.5, k.size[2] * 0.5]) {
            if best.is_none_or(|b| t < b.0 || (t == b.0 && it.uid < idx.items[b.1].uid)) {
                best = Some((t, j as usize));
            }
        }
    }
    let (t, j) = best.ok_or("no_aim")?;
    if rw.cast(o, d, reach, layers.of(aim_uid), aim_uid).is_some_and(|h| h.t < t) {
        return Err("no_aim");
    }
    Ok((idx.items[j].uid, idx.items[j].e, idx.items[j].pos, idx.items[j].kind))
}

/// What one request to upgrade did: the reason it was refused, or the piece that was upgraded.
fn upgrade_one(world: &mut World, rules: &Rules, fields: &Terrains, meshes: &MeshStore, e: Entity, uid: u64, u: &Upgrader, events: &mut Events) -> Result<(u64, V3), &'static str> {
    let bag_e = bag_of(world, e, u.inventory);
    let body = bag_e.unwrap_or(e);
    if world.get::<HitPoints>(body).is_some_and(|h| h.dead || h.value <= 0.0) {
        return Err("dead");
    }
    let aim_e = if u.aim.0 != 0 { world.entity(u.aim.0) } else { None }.or_else(|| world.get::<NetAvatar>(e).filter(|n| n.head.0 != 0).and_then(|n| world.entity(n.head.0))).unwrap_or(e);
    let aim_uid = world.uid_of(aim_e).unwrap_or(uid);
    let (piece_uid, piece_e, piece_pos, kind_i) = aimed_piece(world, rules, fields, meshes, aim_e, uid, u.reach)?;
    let kind = &rules.kinds[kind_i];
    let Some(to_i) = (!kind.upgrade.is_empty()).then(|| rules.kind(&kind.upgrade)).flatten() else { return Err("no_upgrade") };
    let to = &rules.kinds[to_i];
    // not on land that is somebody else's
    let owner = if u.owner != 0 { u.owner } else { world.get::<NetAvatar>(e).map_or(0, |n| n.slot + 1) };
    if gather_cups(world).iter().any(|c| c.exclusive && c.owner != 0 && c.owner != owner && dist(c.pos, piece_pos) <= c.radius) {
        return Err("not_yours");
    }
    // what it costs: all of it is in the bag, or none is taken
    if !kind.upgrade_cost.is_empty() {
        let Some((table, _)) = table_of(world) else { return Err("no_materials") };
        let Some(bag_e) = bag_e else { return Err("no_materials") };
        let Some(mut inv) = world.get::<Inventory>(bag_e).copied() else { return Err("no_materials") };
        let mut bag = Bag::of(&inv);
        let ids: Vec<(u32, u32)> = kind.upgrade_cost.iter().map(|(name, n)| (table.id_of(name), *n)).collect();
        if ids.iter().any(|(id, n)| *id == 0 || bag.total(*id) < *n) {
            return Err("no_materials");
        }
        for (id, n) in ids {
            bag.remove(id, n);
        }
        bag.store(&mut inv);
        if let Some(w) = world.get_mut::<Inventory>(bag_e) {
            *w = inv;
        }
    }
    // the piece is the other piece now
    let name = Sym(world.strings_mut().intern(&to.name));
    if let Some(p) = world.get_mut::<BuildPiece>(piece_e) {
        p.kind = name;
    }
    if to.hp > 0.0 {
        if let Some(h) = world.get_mut::<HitPoints>(piece_e) {
            h.value = to.hp;
            h.max = to.hp;
            h.last = to.hp;
        }
    }
    if to.bullet < 1.0 {
        match world.get_mut::<crate::blast::Resist>(piece_e) {
            Some(r) => r.bullet = to.bullet,
            None => {
                let _ = world.insert(piece_e, crate::blast::Resist { bullet: to.bullet });
            }
        }
    } else {
        let _ = world.remove_typed::<crate::blast::Resist>(piece_e);
    }
    let number = to_i as u32 + 1;
    if let Some(n) = world.get_mut::<NetEntity>(piece_e) {
        n.kind = number;
    }
    events.push(("build_upgraded".to_string(), piece_uid, Some(piece_pos)));
    events.push(("build_upgrade_ok".to_string(), aim_uid, Some(piece_pos)));
    Ok((piece_uid, piece_pos))
}

/// The players' requests to upgrade a piece (`Upgrader`): cheap when nobody asks.
fn run_upgrades(world: &mut World, rules: &Rules, tick: u64, dt: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    let mut asks: Vec<(Entity, u64, Upgrader)> = Vec::new();
    let _ = world.for_each_uid::<(&Upgrader,), _>(|e, uid, (u,)| {
        if u.want || u.denied.0 != 0 {
            asks.push((e, uid, *u));
        }
    });
    asks.sort_by_key(|a| a.1);
    for (e, uid, mut u) in asks {
        let before = u;
        u.denied = Sym(0);
        let now = tick as u32;
        if u.want && now >= u.ready {
            let aim = if u.aim.0 != 0 { world.entity(u.aim.0) } else { None }.or_else(|| world.get::<NetAvatar>(e).filter(|n| n.head.0 != 0).and_then(|n| world.entity(n.head.0))).unwrap_or(e);
            let aim_uid = world.uid_of(aim).unwrap_or(uid);
            let pause = ((0.5 / dt).ceil() as u32).max(1);
            match upgrade_one(world, rules, fields, meshes, e, uid, &u, events) {
                Ok(_) => u.upgraded = u.upgraded.wrapping_add(1),
                Err(why) => {
                    u.denied = Sym(world.strings_mut().intern(why));
                    let at = world.get::<Transform>(e).map(|t| t.pos);
                    events.push(("build_upgrade_refused".to_string(), aim_uid, at));
                    events.push((format!("build_upgrade_refused_{why}"), aim_uid, at));
                }
            }
            u.ready = now.saturating_add(pause);
        }
        u.want = false;
        if u != before {
            if let Some(w) = world.get_mut::<Upgrader>(e) {
                *w = u;
            }
        }
    }
}

pub fn update(world: &mut World, scripts: &mut ScriptSet, tick: u64, dt: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let Some((rules_e, rules)) = rules_of(world) else { return };
    let rules: &Rules = &rules;
    let every = ((rules.audit / dt).round() as u64).max(1);
    let audit = tick % every == 0;
    let died: Vec<u64> = events.iter().filter(|e| e.0 == "died").map(|e| e.1).filter(|u| world.entity(*u).is_some_and(|e| world.get::<BuildPiece>(e).is_some())).collect();
    if audit || !died.is_empty() {
        sweep(world, scripts, rules, rules_e, &died, audit, dt * every as f32, fields, meshes, events);
    }
    run_upgrades(world, rules, tick, dt, fields, meshes, events);
    let mut tools: Vec<(Entity, u64, BuildTool, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&BuildTool, &Transform), _>(|e, uid, (b, t)| tools.push((e, uid, *b, *t)));
    if tools.is_empty() {
        return;
    }
    tools.sort_by_key(|t| t.1);
    run_tools(world, scripts, rules, rules_e, &tools, fields, meshes, events);
}

