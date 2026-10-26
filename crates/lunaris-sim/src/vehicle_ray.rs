//! Rays against the static world, for the wheels of a `Vehicle` (docs/PHYSICS.md). A wheel stands on whatever a short ray down its strut hits: the
//! static `Collider`s (boxes, turned or not, spheres and capsules), `ConvexCollider` hulls that do not move, `MeshCollider` levels and `Terrain`,
//! with the same shapes and the same rules as the rest of the physics. What the chassis does not collide with (`CollisionLayer`) is not there for the wheels.
//! Bodies that move (anything with a `RigidBody` of mass above 0 and a `Velocity`, characters) are not ground: a wheel does not push on them.
//!
//! The world is gathered once for all the vehicles of a tick, cut down to the boxes the rays can reach, in uid order; a hit at the same distance goes to the
//! lowest uid, so the answer never depends on how things are stored. A ray that starts *inside* a solid does not see it.
use crate::components::{Collider, CharacterBody, ConvexCollider, RigidBody, Transform, Velocity};
use crate::layers::{Filter, Layers};
use crate::meshcol::{HullInstance, Instance, MeshStore};
use crate::physics_rot::{axes_of, normalize_q, M3};
use crate::terrain::{grounds, Ground, Terrains};
use lunaris_ecs::World;
use std::collections::{BTreeSet, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

type V3 = [f32; 3];

/// A hasher for uids (one multiplication and a fold of the high half over the low). A set of the uids of the things that move is made for every gathering of the world, which most systems that look at the
/// world do every step, and is only asked whether it holds a uid: a `BTreeSet` of 200 characters costs 25 microseconds to make, this 3.
#[derive(Default, Clone, Copy)]
pub(crate) struct UidHasher(u64);

impl Hasher for UidHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(self.0 ^ b as u64);
        }
    }
    fn write_u64(&mut self, i: u64) {
        let h = i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.0 = h ^ (h >> 32);
    }
}

/// A set of uids that is only asked whether it holds one (nothing depends on its order).
pub(crate) type UidSet = HashSet<u64, BuildHasherDefault<UidHasher>>;

/// What a ray hit: the distance along it, the unit normal (facing the ray) and the entity.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub t: f32,
    pub normal: V3,
    /// What the ray hit (not used by the wheels, which only need where and how it leans; it is there for whoever asks what a wheel stands on).
    #[allow(dead_code)]
    pub uid: u64,
}

#[derive(Clone, Copy)]
enum Kind {
    /// An oriented box: its centre, its own axes in the world, its half sizes.
    Obb(V3, M3, V3),
    Sphere(V3, f32),
    /// A capsule: the two centres of its ends and its radius.
    Capsule(V3, V3, f32),
}

#[derive(Clone)]
struct Solid {
    uid: u64,
    filter: Filter,
    kind: Kind,
    lo: V3,
    hi: V3,
    /// `lo` and `hi` pushed out on every side by `SLOP` (and a little more for a far place), for the quick test of [`Quick::may_touch`].
    plo: V3,
    phi: V3,
    /// An `Obb` whose axes are the world's own (a wall, a floor): the exact test of a ray against it needs no turning of the ray into the box's frame.
    aligned: bool,
}

/// What is different between two gatherings of the same world ([`RayWorld::changed_since`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Changed {
    /// The same solids, the same terrain, meshes and hulls.
    Nothing,
    /// Boxes, balls and capsules were added, taken away or changed: these are the boxes (low corner, high corner) round each, where it was and where it is (at most 16 of them: with more it is `Everything`).
    Boxes(Vec<([f32; 3], [f32; 3])>),
    /// Something else changed (terrain, a mesh level, a hull), or the change is not known.
    Everything,
}

/// How far (metres) the quick test of a ray against a box looks past the box on every side: far more than the rounding of what it does, so that it only says "no" when [`touches_box`] says "no".
const SLOP: f32 = 1.0e-3;

impl Solid {
    /// Whether the other is the very same solid (the same entity, shape, place and layer, to the last bit).
    fn same_as(&self, other: &Solid) -> bool {
        // (the shape as the bits of its numbers, as the signature takes it)
        let bits = |k: &Kind| -> (u8, [u32; 15]) {
            let mut out = [0u32; 15];
            let tag = match *k {
                Kind::Obb(c, axes, h) => {
                    for a in 0..3 {
                        out[a] = c[a].to_bits();
                        out[3 + a] = h[a].to_bits();
                        for q in 0..3 {
                            out[6 + 3 * a + q] = axes[a][q].to_bits();
                        }
                    }
                    1
                }
                Kind::Sphere(c, r) => {
                    out[..3].iter_mut().zip(c).for_each(|(o, v)| *o = v.to_bits());
                    out[3] = r.to_bits();
                    2
                }
                Kind::Capsule(a, b, r) => {
                    for q in 0..3 {
                        out[q] = a[q].to_bits();
                        out[3 + q] = b[q].to_bits();
                    }
                    out[6] = r.to_bits();
                    3
                }
            };
            (tag, out)
        };
        self.uid == other.uid && self.filter == other.filter && bits(&self.kind) == bits(&other.kind)
    }

    fn new(uid: u64, filter: Filter, kind: Kind, lo: V3, hi: V3) -> Solid {
        let big = lo.iter().chain(hi.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
        let pad = SLOP + 2.0e-6 * big;
        let aligned = matches!(kind, Kind::Obb(_, axes, _) if axes == [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        Solid { uid, filter, kind, lo, hi, plo: [lo[0] - pad, lo[1] - pad, lo[2] - pad], phi: [hi[0] + pad, hi[1] + pad, hi[2] + pad], aligned }
    }
}

/// What a ray needs again and again while it is tried against many solids: its reciprocal direction, so that the test of a box is multiplications where [`touches_box`] divides.
struct Quick {
    o: V3,
    inv: V3,
    parallel: [bool; 3],
    /// The same pad as the solids' (for the size of the numbers the ray starts from) and the length of the ray.
    pad: f32,
    max: f32,
}

impl Quick {
    #[inline]
    fn new(o: V3, d: V3, max: f32) -> Quick {
        let parallel = [d[0].abs() < 1.0e-12, d[1].abs() < 1.0e-12, d[2].abs() < 1.0e-12];
        let inv = [if parallel[0] { 0.0 } else { 1.0 / d[0] }, if parallel[1] { 0.0 } else { 1.0 / d[1] }, if parallel[2] { 0.0 } else { 1.0 / d[2] }];
        let big = o[0].abs().max(o[1].abs()).max(o[2].abs());
        Quick { o, inv, parallel, pad: 2.0e-6 * big, max }
    }

    /// `touches_box(o, d, max, lo, hi)` for the box `plo`..`phi` of a [`Solid`]: false only where that is false. (Where the ray runs along an axis it is inside the slab or not; elsewhere the times it is in the slabs must overlap.)
    #[inline]
    fn may_touch(&self, plo: V3, phi: V3) -> bool {
        let (mut t0, mut t1) = (-SLOP, self.max + SLOP);
        for a in 0..3 {
            if self.parallel[a] {
                if self.o[a] < plo[a] - self.pad || self.o[a] > phi[a] + self.pad {
                    return false;
                }
            } else {
                let (mut ta, mut tb) = ((plo[a] - self.pad - self.o[a]) * self.inv[a], (phi[a] + self.pad - self.o[a]) * self.inv[a]);
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                }
                t0 = t0.max(ta);
                t1 = t1.min(tb);
                if t0 > t1 {
                    return false;
                }
            }
        }
        true
    }
}

/// `ray_obb(o, d, c, &axes, h).is_some_and(|(t, _)| t <= max)` for the axes of the world (the same arithmetic, without the 18 multiplications that turn the ray into the box's frame: with those axes they change nothing).
fn aligned_hits(o: V3, d: V3, c: V3, h: V3, max: f32) -> bool {
    let lo = sub(o, c);
    let (mut t0, mut t1) = (f32::NEG_INFINITY, f32::INFINITY);
    for a in 0..3 {
        if d[a].abs() < 1.0e-12 {
            if lo[a].abs() > h[a] {
                return false;
            }
            continue;
        }
        let (mut ta, mut tb) = ((-h[a] - lo[a]) / d[a], (h[a] - lo[a]) / d[a]);
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        if ta > t0 {
            t0 = ta;
        }
        t1 = t1.min(tb);
        if t0 > t1 {
            return false;
        }
    }
    !(t0 < 0.0 || !t0.is_finite()) && t0 <= max
}

/// The static things a wheel can stand on.
#[derive(Default)]
pub struct RayWorld {
    /// The solids that stay where they are, in uid order. Shared between gatherings of a world that has not changed (see `StaticSolids`).
    solids: std::sync::Arc<Vec<Solid>>,
    /// The solids that fly on their own (a collider and a velocity and no rigid body: a drone), in uid order: few, and different at every step, so they are kept apart from the rest and looked at one by one.
    dynamic: Vec<Solid>,
    grounds: Vec<(Filter, Ground)>,
    level: Vec<(Filter, Instance)>,
    /// With many mesh levels (a streamed city's shells, a few hundred): a grid over their boxes, so that a ray asks only the meshes in the cells it passes through and not every one (it was three quarters of the time of a ray in a city).
    level_grid: Option<std::sync::Arc<SolidGrid>>,
    hulls: Vec<(Filter, HullInstance)>,
    /// With many solids: a grid over them (x and z), so that a ray looks only at the solids in the cells it passes through (and at the big ones, which are in none).
    grid: Option<std::sync::Arc<SolidGrid>>,
}

thread_local! {
    /// While a profile runs: whether `RayWorld::cast` times its parts, and what it added up since the profile took it: the solids' grid, the flying solids, terrain, mesh levels, hulls, all of the casts, and how many there were.
    static CAST_TIMING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static CAST_MS: std::cell::RefCell<[f64; 17]> = const { std::cell::RefCell::new([0.0; 17]) };
}

/// What `CAST_MS` counts, in the order it holds them (the last is a count).
pub(crate) const CAST_PARTS: [&str; 17] = ["solids", "flying solids", "terrain", "mesh levels", "hulls", "all casts", "casts (a count)", "gathers", "gathers: mesh levels", "gathers (a count)", "gathers: layers and sets", "gathers: key of the solids", "gathers: kept solids", "gathers: terrain", "gathers: solids of a small box", "gathers: static solids made again", "gathers: static solids made again (a count)"];

/// Which of `CAST_PARTS` are counts and not milliseconds.
pub(crate) fn cast_part_is_count(k: usize) -> bool {
    k == 6 || k == 9 || k == 16
}

pub(crate) fn set_cast_timing(on: bool) {
    CAST_TIMING.with(|t| t.set(on));
    CAST_MS.with(|m| *m.borrow_mut() = [0.0; 17]);
}

pub(crate) fn take_cast_ms() -> [f64; 17] {
    CAST_MS.with(|m| std::mem::take(&mut *m.borrow_mut()))
}

thread_local! {
    /// The grids over the mesh levels of the last gatherings on this thread, by what they were made from (two hashes of the boxes and uids in order): the systems that cast each gather the same levels again and again.
    static LEVEL_GRIDS: std::cell::RefCell<Vec<([u64; 2], std::sync::Arc<SolidGrid>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The most grids over mesh levels that are kept on a thread.
const LEVEL_GRIDS_KEPT: usize = 4;

/// A grid over the boxes of these mesh levels, or none when there are too few for it to pay (the order of the list is the order a ray looks at them in).
fn level_grid_for(level: &[(Filter, Instance)]) -> Option<std::sync::Arc<SolidGrid>> {
    if level.len() < GRID_FROM {
        return None;
    }
    let mut key = [0xcbf2_9ce4_8422_2325u64, 0x9E37_79B9_7F4A_7C15u64];
    for (_, m) in level {
        let b = &m.placed.bounds;
        let mut h = m.uid;
        for v in b.lo.iter().chain(b.hi.iter()) {
            h = (h ^ v.to_bits() as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h ^= h >> 29;
        }
        key[0] = (key[0] ^ h).wrapping_mul(0x0000_0100_0000_01b3);
        key[1] = key[1].rotate_left(17) ^ h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    }
    if let Some(g) = LEVEL_GRIDS.with(|c| c.borrow().iter().find(|e| e.0 == key).map(|e| e.1.clone())) {
        return Some(g);
    }
    let boxes: Vec<Bx> = level.iter().map(|(_, m)| Bx { lo: m.placed.bounds.lo, hi: m.placed.bounds.hi }).collect();
    let g = std::sync::Arc::new(SolidGrid::build_boxes(&boxes)?);
    LEVEL_GRIDS.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= LEVEL_GRIDS_KEPT {
            c.remove(0);
        }
        c.push((key, g.clone()));
    });
    Some(g)
}

/// The static solids of the last gathering of a wide box on this thread and what they were made from: a gathering asks for the same thing again (the whole world, or a big part of it) several times a step, from the
/// vehicles, the helicopters, the flying agents, perception and the probes, and the world does not change from one step to the next unless a door swings or a thing is built. The key is what makes the solids: a sum of
/// hashes of every static collider's place, shape and size (the order of the world does not matter), how many there are, and the layers; if it is the same, so are the solids.
struct StaticSolids {
    key: [u64; 4],
    solids: std::sync::Arc<Vec<Solid>>,
    grid: Option<std::sync::Arc<SolidGrid>>,
}

/// The static solids without the entities that were excepted, as the last gathering that excepted some made them (see `StaticSolids`: the key is that of the static solids and a hash of the excepted ones that are among them).
struct DroppedSolids {
    key: [u64; 5],
    solids: std::sync::Arc<Vec<Solid>>,
    grid: Option<std::sync::Arc<SolidGrid>>,
}

thread_local! {
    static STATIC_SOLIDS: std::cell::RefCell<Option<StaticSolids>> = const { std::cell::RefCell::new(None) };
    static DROPPED_SOLIDS: std::cell::RefCell<Option<DroppedSolids>> = const { std::cell::RefCell::new(None) };
}

/// Two hashes of a collider's uid, place, turn, scale, size and shape (added up over all of them, so that the order does not matter).
#[inline]
fn collider_key(uid: u64, t: &Transform, c: &Collider) -> (u64, u64) {
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ uid;
    for v in t.pos.iter().chain(t.rot.iter()).chain(t.scale.iter()).chain(c.size.iter()) {
        h = (h ^ v.to_bits() as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h = (h ^ c.shape as u64).wrapping_mul(0x0000_0100_0000_01b3);
    (h ^ (h >> 29), h.rotate_left(31).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// The solid of a collider (its shape in the world and the box round it).
fn solid_of(uid: u64, t: &Transform, c: &Collider, filter: Filter) -> Solid {
    let s = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
    let axes = axes_of(normalize_q(t.rot));
    let (kind, lo, hi) = if c.shape == 0 {
        let r = 0.5 * c.size[0].abs() * s[0].max(s[1]).max(s[2]);
        (Kind::Sphere(t.pos, r), [t.pos[0] - r, t.pos[1] - r, t.pos[2] - r], [t.pos[0] + r, t.pos[1] + r, t.pos[2] + r])
    } else if c.shape == 2 {
        let r = 0.5 * c.size[0].abs() * s[0].max(s[2]);
        let half = (0.5 * c.size[1].abs() * s[1] - r).max(0.0);
        let (a, b) = (axpy(t.pos, -half, axes[1]), axpy(t.pos, half, axes[1]));
        let (lo, hi) = segment_box(a, b, r);
        (Kind::Capsule(a, b, r), lo, hi)
    } else {
        let h = [0.5 * c.size[0].abs() * s[0], 0.5 * c.size[1].abs() * s[1], 0.5 * c.size[2].abs() * s[2]];
        let e = [0, 1, 2].map(|a| h[0] * axes[0][a].abs() + h[1] * axes[1][a].abs() + h[2] * axes[2][a].abs());
        (Kind::Obb(t.pos, axes, h), [t.pos[0] - e[0], t.pos[1] - e[1], t.pos[2] - e[2]], [t.pos[0] + e[0], t.pos[1] + e[1], t.pos[2] + e[2]])
    };
    Solid::new(uid, filter, kind, lo, hi)
}

/// The solids of some colliders, in uid order, those that pass `keep` (the order is found by sorting the small keys, not the 80-byte records: with thousands of colliders in the world that is most of what sorting costs).
fn solids_of(raw: &[(u64, Transform, Collider)], layers: &Layers, keep: impl Fn(&Solid) -> bool) -> Vec<Solid> {
    let mut order: Vec<(u64, u32)> = raw.iter().enumerate().map(|(i, r)| (r.0, i as u32)).collect();
    order.sort_unstable();
    let mut out = Vec::new();
    for (_, i) in order {
        let (uid, t, c) = &raw[i as usize];
        let s = solid_of(*uid, t, c, layers.of(*uid));
        if keep(&s) {
            out.push(s);
        }
    }
    out
}

/// What a ray keeps between rays, for each thread (so that none allocates, and so that one `RayWorld` can be asked from several threads at once, as a game server does for the lines of sight of all its clients).
struct RayScratch {
    /// The solids a ray has found in its cells.
    cells: Vec<u32>,
    /// The (place + 1) of the solids that blocked the last few [`RayWorld::occluded`] questions, the newest first (0: none): rays that are asked one after the other mostly go through the same wall. (The places belong to
    /// whichever `RayWorld` was asked last on this thread: one that is not in the one being asked is skipped.)
    recent: [u32; 4],
}

thread_local! {
    static RAY_SCRATCH: std::cell::RefCell<RayScratch> = const { std::cell::RefCell::new(RayScratch { cells: Vec::new(), recent: [0; 4] }) };
}

/// Puts a place (plus one) at the front of the list of the ones that blocked the last rays (it moves up if it is in the list, and pushes the oldest out if it is not).
fn remember(recent: &mut [u32; 4], place_plus_one: u32) {
    let at = recent.iter().position(|&x| x == place_plus_one).unwrap_or(recent.len() - 1);
    recent.copy_within(0..at, 1);
    recent[0] = place_plus_one;
}


/// The fewest solids that a grid is made for: with fewer, looking at every one is as quick.
const GRID_FROM: usize = 48;

/// The two corners of something that a `SolidGrid` is made over.
#[derive(Clone, Copy)]
struct Bx {
    lo: V3,
    hi: V3,
}

/// A uniform grid over the footprints (x and z) of the solids that are small beside its cells; each cell lists, in order, the solids that touch it.
struct SolidGrid {
    cell: f64,
    x0: f64,
    z0: f64,
    nx: i32,
    nz: i32,
    /// The solids of cell `c` (in order) are `items[starts[c]..starts[c + 1]]`: one flat list for the whole grid, made in two passes (a count, then the fill), so that a grid of a hundred thousand cells costs two
    /// allocations and not one for every cell that has something in it.
    starts: Vec<u32>,
    items: Vec<u32>,
    /// The solids that are too big or not finite: every ray looks at them.
    big: Vec<u32>,
}

impl SolidGrid {
    /// A grid over the footprints of these solids (none when there are too few for it to pay).
    fn build(solids: &[Solid]) -> Option<SolidGrid> {
        if solids.len() < GRID_FROM {
            return None;
        }
        let boxes: Vec<Bx> = solids.iter().map(|s| Bx { lo: s.lo, hi: s.hi }).collect();
        SolidGrid::build_boxes(&boxes)
    }

    /// A grid over the footprints of boxes (the solids' or the mesh levels': what a ray has to look at is whatever is in the cells it passes through).
    fn build_boxes(solids: &[Bx]) -> Option<SolidGrid> {
        if solids.len() < GRID_FROM {
            return None;
        }
        let finite = |s: &Bx| (0..3).all(|a| s.lo[a].is_finite() && s.hi[a].is_finite());
        let mut ext: Vec<f32> = solids.iter().filter(|s| finite(s)).map(|s| (s.hi[0] - s.lo[0]).max(s.hi[2] - s.lo[2])).collect();
        if ext.len() < GRID_FROM {
            return None;
        }
        // (the median: found by selecting it, not by sorting thousands of sizes)
        let mid = ext.len() / 2;
        let median = *ext.select_nth_unstable_by(mid, |a, b| a.total_cmp(b)).1;
        let mut cell = ((median as f64) * 2.0).clamp(2.0, 64.0);
        let pad = 0.05f64;
        for _ in 0..8 {
            let small = |s: &Bx| finite(s) && ((s.hi[0] - s.lo[0]).max(s.hi[2] - s.lo[2]) as f64) <= cell * 8.0;
            let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
            let mut count = 0;
            for s in solids.iter().filter(|s| small(s)) {
                lo[0] = lo[0].min(s.lo[0] as f64 - pad);
                lo[1] = lo[1].min(s.lo[2] as f64 - pad);
                hi[0] = hi[0].max(s.hi[0] as f64 + pad);
                hi[1] = hi[1].max(s.hi[2] as f64 + pad);
                count += 1;
            }
            if count < GRID_FROM / 2 {
                return None;
            }
            let nx = (((hi[0] - lo[0]) / cell).floor() as i64 + 1).max(1);
            let nz = (((hi[1] - lo[1]) / cell).floor() as i64 + 1).max(1);
            if nx * nz > 1 << 18 {
                cell *= 2.0;
                continue;
            }
            let (nx, nz) = (nx as i32, nz as i32);
            let mut big = Vec::new();
            // the cells each small solid covers, found once (a big one has none: [-1; 4])
            let mut spans: Vec<[i32; 4]> = Vec::with_capacity(solids.len());
            for s in solids.iter() {
                if !small(s) {
                    spans.push([-1; 4]);
                    continue;
                }
                let cx0 = (((s.lo[0] as f64 - pad - lo[0]) / cell).floor() as i32).clamp(0, nx - 1);
                let cx1 = (((s.hi[0] as f64 + pad - lo[0]) / cell).floor() as i32).clamp(0, nx - 1);
                let cz0 = (((s.lo[2] as f64 - pad - lo[1]) / cell).floor() as i32).clamp(0, nz - 1);
                let cz1 = (((s.hi[2] as f64 + pad - lo[1]) / cell).floor() as i32).clamp(0, nz - 1);
                spans.push([cx0, cx1, cz0, cz1]);
            }
            // pass one: how many solids each cell has; then the start of each cell in the flat list
            let mut starts = vec![0u32; (nx * nz) as usize + 1];
            for &[cx0, cx1, cz0, cz1] in spans.iter() {
                if cx0 < 0 {
                    continue;
                }
                for cz in cz0..=cz1 {
                    for cx in cx0..=cx1 {
                        starts[(cz * nx + cx) as usize + 1] += 1;
                    }
                }
            }
            for c in 0..(nx * nz) as usize {
                starts[c + 1] += starts[c];
            }
            // pass two: the fill, in the order of the solids (the order a ray finds them in, as before)
            let mut items = vec![0u32; starts[(nx * nz) as usize] as usize];
            let mut at = starts.clone();
            for (i, &[cx0, cx1, cz0, cz1]) in spans.iter().enumerate() {
                if cx0 < 0 {
                    big.push(i as u32);
                    continue;
                }
                for cz in cz0..=cz1 {
                    for cx in cx0..=cx1 {
                        let c = (cz * nx + cx) as usize;
                        items[at[c] as usize] = i as u32;
                        at[c] += 1;
                    }
                }
            }
            return Some(SolidGrid { cell, x0: lo[0], z0: lo[1], nx, nz, starts, items, big });
        }
        None
    }

    /// Put in `out` the solids that a ray `o + t d`, `0 <= t <= max`, may touch: the big ones and the ones in the cells it passes through (some more than once).
    fn collect(&self, o: V3, d: V3, max: f32, out: &mut Vec<u32>) {
        out.extend_from_slice(&self.big);
        let (ox, oz) = (o[0] as f64, o[2] as f64);
        let (dx, dz) = (d[0] as f64 * max as f64, d[2] as f64 * max as f64);
        // the part of the segment inside the grid
        let (gx1, gz1) = (self.x0 + self.nx as f64 * self.cell, self.z0 + self.nz as f64 * self.cell);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        for (p, dv, lo, hi) in [(ox, dx, self.x0, gx1), (oz, dz, self.z0, gz1)] {
            if dv.abs() < 1.0e-12 {
                if p < lo || p > hi {
                    return;
                }
            } else {
                let (a, b) = ((lo - p) / dv, (hi - p) / dv);
                let (a, b) = if a < b { (a, b) } else { (b, a) };
                t0 = t0.max(a);
                t1 = t1.min(b);
                if t0 > t1 {
                    return;
                }
            }
        }
        let (sx, sz) = (ox + dx * t0, oz + dz * t0);
        let (ex, ez) = (ox + dx * t1, oz + dz * t1);
        let cx_of = |x: f64| (((x - self.x0) / self.cell).floor() as i32).clamp(0, self.nx - 1);
        let cz_of = |z: f64| (((z - self.z0) / self.cell).floor() as i32).clamp(0, self.nz - 1);
        let (mut cx, mut cz) = (cx_of(sx), cz_of(sz));
        let (ecx, ecz) = (cx_of(ex), cz_of(ez));
        let (wx, wz) = (ex - sx, ez - sz);
        let (step_x, step_z) = (if wx > 0.0 { 1 } else { -1 }, if wz > 0.0 { 1 } else { -1 });
        // (the parameter, along the clipped segment, at which the next vertical and horizontal cell edge is crossed, and the parameter between two of them)
        let next = |c: i32, step: i32, origin: f64| origin + (if step > 0 { c + 1 } else { c }) as f64 * self.cell;
        let (mut tmx, mut tdx) = if wx.abs() > 1.0e-12 { ((next(cx, step_x, self.x0) - sx) / wx, self.cell / wx.abs()) } else { (f64::INFINITY, f64::INFINITY) };
        let (mut tmz, mut tdz) = if wz.abs() > 1.0e-12 { ((next(cz, step_z, self.z0) - sz) / wz, self.cell / wz.abs()) } else { (f64::INFINITY, f64::INFINITY) };
        for _ in 0..(self.nx + self.nz + 4) {
            let c = (cz * self.nx + cx) as usize;
            out.extend_from_slice(&self.items[self.starts[c] as usize..self.starts[c + 1] as usize]);
            if cx == ecx && cz == ecz {
                break;
            }
            if tmx < tmz {
                cx += step_x;
                tmx += tdx;
            } else {
                cz += step_z;
                tmz += tdz;
            }
            if cx < 0 || cz < 0 || cx >= self.nx || cz >= self.nz {
                break;
            }
        }
    }
}

#[inline]
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
fn axpy(a: V3, s: f32, d: V3) -> V3 {
    [a[0] + s * d[0], a[1] + s * d[1], a[2] + s * d[2]]
}

/// The box around a segment `a`..`b` fattened by `pad` on every side.
pub fn segment_box(a: V3, b: V3, pad: f32) -> (V3, V3) {
    ([a[0].min(b[0]) - pad, a[1].min(b[1]) - pad, a[2].min(b[2]) - pad], [a[0].max(b[0]) + pad, a[1].max(b[1]) + pad, a[2].max(b[2]) + pad])
}

/// Does the ray `o + t d`, `0 <= t <= max`, touch the box `lo`..`hi`? (A quick test: it only has to say "no" when the answer is no.)
fn touches_box(o: V3, d: V3, max: f32, lo: V3, hi: V3) -> bool {
    let (mut t0, mut t1) = (0.0f32, max);
    for a in 0..3 {
        if d[a].abs() < 1.0e-12 {
            if o[a] < lo[a] || o[a] > hi[a] {
                return false;
            }
        } else {
            let (mut ta, mut tb) = ((lo[a] - o[a]) / d[a], (hi[a] - o[a]) / d[a]);
            if ta > tb {
                std::mem::swap(&mut ta, &mut tb);
            }
            t0 = t0.max(ta);
            t1 = t1.min(tb);
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// A ray against a sphere; the nearest hit in front of the origin (a ray that starts inside sees nothing).
pub(crate) fn ray_sphere(o: V3, d: V3, c: V3, r: f32) -> Option<(f32, V3)> {
    let oc = sub(o, c);
    let b = dot(oc, d);
    let cc = dot(oc, oc) - r * r;
    if cc <= 0.0 {
        return None;
    }
    let disc = b * b - cc;
    if disc < 0.0 || b >= 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    if t < 0.0 {
        return None;
    }
    let p = axpy(oc, t, d);
    Some((t, [p[0] / r, p[1] / r, p[2] / r]))
}

/// A ray against an oriented box.
pub(crate) fn ray_obb(o: V3, d: V3, c: V3, axes: &M3, h: V3) -> Option<(f32, V3)> {
    let rel = sub(o, c);
    let lo = [dot(axes[0], rel), dot(axes[1], rel), dot(axes[2], rel)];
    let ld = [dot(axes[0], d), dot(axes[1], d), dot(axes[2], d)];
    let (mut t0, mut t1) = (f32::NEG_INFINITY, f32::INFINITY);
    let mut hit_axis = 0usize;
    for a in 0..3 {
        if ld[a].abs() < 1.0e-12 {
            if lo[a].abs() > h[a] {
                return None;
            }
            continue;
        }
        let (mut ta, mut tb) = ((-h[a] - lo[a]) / ld[a], (h[a] - lo[a]) / ld[a]);
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        if ta > t0 {
            t0 = ta;
            hit_axis = a;
        }
        t1 = t1.min(tb);
        if t0 > t1 {
            return None;
        }
    }
    if t0 < 0.0 || !t0.is_finite() {
        return None; // (the origin is inside the box, or the ray is parallel to everything)
    }
    let sign = if ld[hit_axis] > 0.0 { -1.0 } else { 1.0 };
    let n = [axes[hit_axis][0] * sign, axes[hit_axis][1] * sign, axes[hit_axis][2] * sign];
    Some((t0, n))
}

/// A ray against a capsule (the segment `a`..`b` thickened by `r`).
pub(crate) fn ray_capsule(o: V3, d: V3, a: V3, b: V3, r: f32) -> Option<(f32, V3)> {
    let mut best: Option<(f32, V3)> = None;
    let mut take = |h: Option<(f32, V3)>| {
        if let Some((t, n)) = h {
            if best.is_none_or(|(bt, _)| t < bt) {
                best = Some((t, n));
            }
        }
    };
    // the two ends
    take(ray_sphere(o, d, a, r));
    take(ray_sphere(o, d, b, r));
    // the side: the infinite cylinder round the axis, between the two ends
    let ba = sub(b, a);
    let baba = dot(ba, ba);
    if baba > 1.0e-12 {
        let oa = sub(o, a);
        let (bard, baoa) = (dot(ba, d), dot(ba, oa));
        let aa = baba - bard * bard;
        if aa > 1.0e-9 {
            let bb = baba * dot(d, oa) - baoa * bard;
            let cc = baba * dot(oa, oa) - baoa * baoa - r * r * baba;
            let disc = bb * bb - aa * cc;
            if disc >= 0.0 && cc > 0.0 {
                let t = (-bb - disc.sqrt()) / aa;
                let y = baoa + t * bard;
                if t >= 0.0 && y > 0.0 && y < baba {
                    let p = axpy(oa, t, d);
                    let on_axis = [ba[0] * y / baba, ba[1] * y / baba, ba[2] * y / baba];
                    let n = sub(p, on_axis);
                    take(Some((t, [n[0] / r, n[1] / r, n[2] / r])));
                }
            }
        }
    }
    best
}

impl Solid {
    /// `self.ray(o, d, max).is_some()`, quicker for a box that is not turned.
    #[inline]
    fn blocks(&self, o: V3, d: V3, max: f32) -> bool {
        match self.kind {
            Kind::Obb(c, _, half) if self.aligned => aligned_hits(o, d, c, half, max),
            _ => self.ray(o, d, max).is_some(),
        }
    }

    fn ray(&self, o: V3, d: V3, max: f32) -> Option<(f32, V3)> {
        let h = match self.kind {
            Kind::Obb(c, axes, half) => ray_obb(o, d, c, &axes, half),
            Kind::Sphere(c, r) => ray_sphere(o, d, c, r),
            Kind::Capsule(a, b, r) => ray_capsule(o, d, a, b, r),
        };
        h.filter(|(t, _)| *t <= max)
    }
}

impl RayWorld {
    /// The static things of the world that overlap `reach` (a box in the world; `None`: all of them), with their layers.
    pub fn gather(world: &mut World, fields: &Terrains, meshes: &MeshStore, reach: Option<(V3, V3)>) -> RayWorld {
        RayWorld::gather_except(world, fields, meshes, reach, &BTreeSet::new())
    }

    /// As [`RayWorld::gather`], without the entities in `except` (what a weapon shoots at is tested as a target of its own, not as a wall).
    ///
    /// The solids that stay where they are come from a gathering that is kept between calls (`StaticSolids`) when the box asked for is big or none: looking at all the colliders of the world, sorting them and making the grid
    /// is most of what a gathering costs, and the answer is the same unless something static moved or a layer changed (found out by one pass over the colliders that adds up a hash of each). What flies on its own (a drone) is
    /// gathered afresh every time, apart from them.
    pub fn gather_except(world: &mut World, fields: &Terrains, meshes: &MeshStore, reach: Option<(V3, V3)>, except: &BTreeSet<u64>) -> RayWorld {
        if !CAST_TIMING.with(|t| t.get()) {
            return RayWorld::gather_untimed(world, fields, meshes, reach, except);
        }
        let t0 = std::time::Instant::now();
        let rw = RayWorld::gather_untimed(world, fields, meshes, reach, except);
        CAST_MS.with(|m| {
            let mut m = m.borrow_mut();
            m[7] += t0.elapsed().as_secs_f64() * 1e3;
            m[9] += 1.0;
        });
        rw
    }

    fn gather_untimed(world: &mut World, fields: &Terrains, meshes: &MeshStore, reach: Option<(V3, V3)>, except: &BTreeSet<u64>) -> RayWorld {
        let timed = CAST_TIMING.with(|t| t.get());
        let mut clock = timed.then(std::time::Instant::now);
        let lap = |part: usize, since: &mut Option<std::time::Instant>| {
            if let Some(t) = since {
                let now = std::time::Instant::now();
                CAST_MS.with(|m| m.borrow_mut()[part] += now.duration_since(*t).as_secs_f64() * 1e3);
                *t = now;
            }
        };
        let layers = Layers::gather(world);
        // what moves is not ground
        let mut bodies = UidSet::default();
        let _ = world.for_each_uid::<(&RigidBody, &Velocity), _>(|_, uid, (rb, _)| {
            if rb.mass > 0.0 {
                bodies.insert(uid);
            }
        });
        let _ = world.for_each_uid::<(&CharacterBody,), _>(|_, uid, _| {
            bodies.insert(uid);
        });
        let mut movers: UidSet = except.iter().copied().collect();
        movers.extend(bodies.iter().copied());
        // what flies on its own: a collider and a velocity, no rigid body (the physics does not move it; an agent does)
        let mut rigid = UidSet::default();
        let _ = world.for_each_uid::<(&RigidBody,), _>(|_, uid, _| {
            rigid.insert(uid);
        });
        let mut fliers = UidSet::default();
        let _ = world.for_each_uid::<(&Collider, &Velocity), _>(|_, uid, _| {
            if !rigid.contains(&uid) && !bodies.contains(&uid) {
                fliers.insert(uid);
            }
        });
        lap(10, &mut clock);
        let near = |lo: V3, hi: V3| reach.is_none_or(|(rl, rh)| (0..3).all(|a| lo[a] <= rh[a] && hi[a] >= rl[a]));
        let wall = |t: &Transform, c: &Collider| !c.trigger && t.pos.iter().all(|v| v.is_finite());
        // a box that is wide is asked for often and from most of the world; a small one (a car's, a gun's) is cheaper to gather afresh than to hold the whole world for
        let wide = reach.is_none_or(|(l, h)| !((h[0] - l[0]) * (h[2] - l[2]) < 22_500.0));
        let mut fly_raw: Vec<(u64, Transform, Collider)> = Vec::new();
        let (solids, grid) = if wide {
            // the key of what the static solids would be made of now
            let (mut a, mut b, mut n) = (0u64, 0u64, 0u64);
            let _ = world.for_each_uid::<(&Transform, &Collider), _>(|_, uid, (t, c)| {
                if !wall(t, c) || bodies.contains(&uid) {
                    return;
                }
                if fliers.contains(&uid) {
                    fly_raw.push((uid, *t, *c));
                    return;
                }
                let (x, y) = collider_key(uid, t, c);
                a = a.wrapping_add(x);
                b = b.wrapping_add(y);
                n += 1;
            });
            let key = [a, b, n, layers.digest()];
            lap(11, &mut clock);
            let kept = STATIC_SOLIDS.with(|c| c.borrow().as_ref().filter(|s| s.key == key).map(|s| (s.solids.clone(), s.grid.clone())));
            let (all, all_grid) = match kept {
                Some(k) => k,
                None => {
                    let t_made = timed.then(std::time::Instant::now);
                    let mut raw: Vec<(u64, Transform, Collider)> = Vec::with_capacity(n as usize);
                    let _ = world.for_each_uid::<(&Transform, &Collider), _>(|_, uid, (t, c)| {
                        if wall(t, c) && !bodies.contains(&uid) && !fliers.contains(&uid) {
                            raw.push((uid, *t, *c));
                        }
                    });
                    let all = std::sync::Arc::new(solids_of(&raw, &layers, |_| true));
                    let grid = SolidGrid::build(&all).map(std::sync::Arc::new);
                    STATIC_SOLIDS.with(|c| *c.borrow_mut() = Some(StaticSolids { key, solids: all.clone(), grid: grid.clone() }));
                    if let Some(t) = t_made {
                        CAST_MS.with(|m| {
                            let mut m = m.borrow_mut();
                            m[15] += t.elapsed().as_secs_f64() * 1e3;
                            m[16] += 1.0;
                        });
                    }
                    (all, grid)
                }
            };
            // (all of it, shared, unless an entity that is excepted is one of the solids (a door that can be shot has hit points, and the targets are what perception excepts): then all of it but those, which is the same
            // for every gathering while the same ones are excepted, so it is kept as the solids are (making it, a copy of a few thousand solids and a grid over them, was half of what a gathering cost in the city);
            // it holds the solids outside the box too: a ray that is asked inside the box has the same answer)
            let ex: Vec<u64> = except.iter().copied().filter(|u| all.binary_search_by_key(u, |s| s.uid).is_ok()).collect();
            if ex.is_empty() {
                (all, all_grid)
            } else {
                let mut h = 0xcbf2_9ce4_8422_2325u64 ^ ex.len() as u64;
                for u in &ex {
                    h = (h ^ u).wrapping_mul(0x0000_0100_0000_01b3);
                    h ^= h >> 29;
                }
                let dkey = [key[0], key[1], key[2], key[3], h];
                let kept = DROPPED_SOLIDS.with(|c| c.borrow().as_ref().filter(|d| d.key == dkey).map(|d| (d.solids.clone(), d.grid.clone())));
                match kept {
                    Some(k) => k,
                    None => {
                        let part: Vec<Solid> = all.iter().filter(|s| ex.binary_search(&s.uid).is_err()).cloned().collect();
                        let part = std::sync::Arc::new(part);
                        let grid = SolidGrid::build(&part).map(std::sync::Arc::new);
                        DROPPED_SOLIDS.with(|c| *c.borrow_mut() = Some(DroppedSolids { key: dkey, solids: part.clone(), grid: grid.clone() }));
                        (part, grid)
                    }
                }
            }
        } else {
            // (with a reach, what is farther from it than the biggest the collider can be is left out before anything is worked out for it: a gather for a car or a drone looks at a few dozen of a few thousand
            // colliders, and this keeps the rest from being sorted and measured; the exact test below is the same as ever)
            let mut raw: Vec<(u64, Transform, Collider)> = Vec::new();
            if let Some((rl, rh)) = reach {
                let _ = world.for_each_uid::<(&Transform, &Collider), _>(|_, uid, (t, c)| {
                    if !wall(t, c) || bodies.contains(&uid) {
                        return;
                    }
                    let s = t.scale[0].abs().max(t.scale[1].abs()).max(t.scale[2].abs());
                    let r = 0.5 * (c.size[0].abs() + c.size[1].abs() + c.size[2].abs()) * s * 1.001 + 0.01;
                    if !(r.is_finite() && (0..3).all(|a| t.pos[a] + r >= rl[a] && t.pos[a] - r <= rh[a])) {
                        return;
                    }
                    if fliers.contains(&uid) {
                        fly_raw.push((uid, *t, *c));
                    } else {
                        raw.push((uid, *t, *c));
                    }
                });
            }
            let part = solids_of(&raw, &layers, |s| near(s.lo, s.hi) && !except.contains(&s.uid));
            let grid = SolidGrid::build(&part).map(std::sync::Arc::new);
            (std::sync::Arc::new(part), grid)
        };
        lap(if wide { 12 } else { 14 }, &mut clock);
        let dynamic = solids_of(&fly_raw, &layers, |s| near(s.lo, s.hi) && !except.contains(&s.uid));
        let mut rw = RayWorld { solids, dynamic, grid, ..RayWorld::default() };
        for g in grounds(world, fields) {
            let (lo_y, hi_y) = g.placed().y_range();
            let p = g.placed();
            let (lo, hi) = ([p.centre[0] - p.size[0] * 0.5, lo_y, p.centre[1] - p.size[1] * 0.5], [p.centre[0] + p.size[0] * 0.5, hi_y, p.centre[1] + p.size[1] * 0.5]);
            if near(lo, hi) {
                let f = layers.of(g.uid);
                rw.grounds.push((f, g));
            }
        }
        lap(13, &mut clock);
        let t_levels = CAST_TIMING.with(|t| t.get()).then(std::time::Instant::now);
        for inst in meshes.instances(world) {
            let b = inst.placed.bounds;
            if near(b.lo, b.hi) {
                let f = layers.of(inst.uid);
                rw.level.push((f, inst));
            }
        }
        rw.level_grid = level_grid_for(&rw.level);
        if let Some(t) = t_levels {
            CAST_MS.with(|m| m.borrow_mut()[8] += t.elapsed().as_secs_f64() * 1e3);
        }
        if !meshes.is_empty() {
            let mut any = false;
            let _ = world.for_each_uid::<(&ConvexCollider,), _>(|_, _, _| any = true);
            if any {
                for h in meshes.hull_instances(world) {
                    let b = h.bounds();
                    if !h.trigger && !movers.contains(&h.uid) && near(b.lo, b.hi) {
                        let f = layers.of(h.uid);
                        rw.hulls.push((f, h));
                    }
                }
            }
        }
        rw
    }

    /// A number that is the same for two worlds made of the same things in the same places and different when one of the shapes was moved, turned, added or taken away (a game server that
    /// remembers the answers of line-of-sight tests uses it to know when to forget them). Terrain is counted by where it is, not by its height values.
    pub fn signature(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        self.mix_solids(&mut h);
        self.mix_rest(&mut h);
        h
    }

    /// The part of [`RayWorld::signature`] that is not boxes, balls and capsules: terrain, mesh levels and hulls.
    fn rest_signature(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        self.mix_rest(&mut h);
        h
    }

    /// The terrain alone (where each ground is and how high it reaches).
    fn ground_signature(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for (_, g) in &self.grounds {
            let p = g.placed();
            let (lo, hi) = p.y_range();
            for v in [g.uid, p.centre[0].to_bits() as u64, p.centre[1].to_bits() as u64, p.size[0].to_bits() as u64, p.size[1].to_bits() as u64, lo.to_bits() as u64, hi.to_bits() as u64] {
                h = (h ^ v).wrapping_mul(0x0000_0100_0000_01b3);
                h ^= h >> 29;
            }
        }
        h
    }

    /// The mesh levels and the hulls as (uid, low corner, high corner), in uid order.
    fn placed_boxes(&self) -> Vec<(u64, V3, V3)> {
        let mut v: Vec<(u64, V3, V3)> = self.level.iter().map(|(_, m)| (m.uid, m.placed.bounds.lo, m.placed.bounds.hi)).collect();
        v.extend(self.hulls.iter().map(|(_, h)| {
            let b = h.bounds();
            (h.uid, b.lo, b.hi)
        }));
        v.sort_by_key(|x| x.0);
        v
    }

    fn mix_solids(&self, h: &mut u64) {
        let mut mix = |v: u64| {
            *h = (*h ^ v).wrapping_mul(0x0000_0100_0000_01b3);
            *h ^= *h >> 29;
        };
        let f = |v: f32| v.to_bits() as u64;
        for s in self.solids.iter().chain(self.dynamic.iter()) {
            mix(s.uid);
            match s.kind {
                Kind::Obb(c, axes, half) => {
                    mix(1);
                    c.iter().chain(axes.iter().flatten()).chain(half.iter()).for_each(|&v| mix(f(v)));
                }
                Kind::Sphere(c, r) => {
                    mix(2);
                    c.iter().chain([r].iter()).for_each(|&v| mix(f(v)));
                }
                Kind::Capsule(a, b, r) => {
                    mix(3);
                    a.iter().chain(b.iter()).chain([r].iter()).for_each(|&v| mix(f(v)));
                }
            }
        }
    }

    fn mix_rest(&self, h: &mut u64) {
        let mut mix = |v: u64| {
            *h = (*h ^ v).wrapping_mul(0x0000_0100_0000_01b3);
            *h ^= *h >> 29;
        };
        let f = |v: f32| v.to_bits() as u64;
        for (_, g) in &self.grounds {
            let p = g.placed();
            let (lo, hi) = p.y_range();
            mix(g.uid);
            [p.centre[0], p.centre[1], p.size[0], p.size[1], lo, hi].iter().for_each(|&v| mix(f(v)));
        }
        for (_, m) in &self.level {
            mix(m.uid);
            let b = m.placed.bounds;
            b.lo.iter().chain(b.hi.iter()).for_each(|&v| mix(f(v)));
        }
        for (_, hull) in &self.hulls {
            mix(hull.uid);
            let b = hull.bounds();
            b.lo.iter().chain(b.hi.iter()).for_each(|&v| mix(f(v)));
        }
    }

    /// What is different in this world from `old`, the same scene gathered earlier: nothing, the boxes round everything that was added, taken away or changed (the place it was and the place it is now: boxes, balls
    /// and capsules, mesh levels and hulls), or, when the terrain is not what it was or the changes are too scattered to be told as boxes, everything. A line of sight that does not go through any of the boxes has the
    /// answer it had. More than sixteen boxes are merged by the 64 m square their middles are in (a building that comes in with fifty doors is one box, not "everything"); more than sixteen of those is everything.
    pub fn changed_since(&self, old: &RayWorld) -> Changed {
        if self.ground_signature() != old.ground_signature() {
            return Changed::Everything;
        }
        let mut boxes: Vec<([f32; 3], [f32; 3])> = Vec::new();
        let mut take = |s: &Solid| boxes.push((s.lo, s.hi));
        // (the solids of both are in the order of their uids)
        let (mut i, mut j) = (0, 0);
        loop {
            match (self.solids.get(i), old.solids.get(j)) {
                (Some(x), Some(y)) if x.uid == y.uid => {
                    if !x.same_as(y) {
                        take(x);
                        take(y);
                    }
                    i += 1;
                    j += 1;
                }
                (Some(x), Some(y)) if x.uid < y.uid => {
                    take(x);
                    i += 1;
                }
                (Some(_), Some(y)) => {
                    take(y);
                    j += 1;
                }
                (Some(x), None) => {
                    take(x);
                    i += 1;
                }
                (None, Some(y)) => {
                    take(y);
                    j += 1;
                }
                (None, None) => break,
            }
        }
        // what flies on its own, the same walk
        let (mut i, mut j) = (0, 0);
        loop {
            match (self.dynamic.get(i), old.dynamic.get(j)) {
                (Some(x), Some(y)) if x.uid == y.uid => {
                    if !x.same_as(y) {
                        take(x);
                        take(y);
                    }
                    i += 1;
                    j += 1;
                }
                (Some(x), Some(y)) if x.uid < y.uid => {
                    take(x);
                    i += 1;
                }
                (Some(_), Some(y)) => {
                    take(y);
                    j += 1;
                }
                (Some(x), None) => {
                    take(x);
                    i += 1;
                }
                (None, Some(y)) => {
                    take(y);
                    j += 1;
                }
                (None, None) => break,
            }
        }
        // the mesh levels and the hulls, the same walk (by uid; the same bounds are the same thing)
        let (new_p, old_p) = (self.placed_boxes(), old.placed_boxes());
        let (mut i, mut j) = (0, 0);
        loop {
            match (new_p.get(i), old_p.get(j)) {
                (Some(x), Some(y)) if x.0 == y.0 => {
                    if x.1.map(f32::to_bits) != y.1.map(f32::to_bits) || x.2.map(f32::to_bits) != y.2.map(f32::to_bits) {
                        boxes.push((x.1, x.2));
                        boxes.push((y.1, y.2));
                    }
                    i += 1;
                    j += 1;
                }
                (Some(x), Some(y)) if x.0 < y.0 => {
                    boxes.push((x.1, x.2));
                    i += 1;
                }
                (Some(_), Some(y)) => {
                    boxes.push((y.1, y.2));
                    j += 1;
                }
                (Some(x), None) => {
                    boxes.push((x.1, x.2));
                    i += 1;
                }
                (None, Some(y)) => {
                    boxes.push((y.1, y.2));
                    j += 1;
                }
                (None, None) => break,
            }
        }
        if boxes.is_empty() {
            return Changed::Nothing;
        }
        if !boxes.iter().all(|(lo, hi)| (0..3).all(|a| lo[a].is_finite() && hi[a].is_finite())) {
            return Changed::Everything;
        }
        if boxes.len() <= 16 {
            return Changed::Boxes(boxes);
        }
        // too many to tell one by one: one box for each 64 m square that has any
        let mut cells: std::collections::BTreeMap<(i32, i32), ([f32; 3], [f32; 3])> = std::collections::BTreeMap::new();
        for (lo, hi) in boxes {
            let key = (((lo[0] + hi[0]) * 0.5 / 64.0).floor() as i32, ((lo[2] + hi[2]) * 0.5 / 64.0).floor() as i32);
            let e = cells.entry(key).or_insert((lo, hi));
            for a in 0..3 {
                e.0[a] = e.0[a].min(lo[a]);
                e.1[a] = e.1[a].max(hi[a]);
            }
        }
        if cells.len() <= 16 {
            Changed::Boxes(cells.into_values().collect())
        } else {
            Changed::Everything
        }
    }

    /// How many shapes the world holds (boxes, balls and capsules, terrains, mesh levels and hulls).
    pub fn shapes(&self) -> usize {
        self.solids.len() + self.dynamic.len() + self.grounds.len() + self.level.len() + self.hulls.len()
    }

    /// Does the segment meet a mesh level that `who` collides with? (Through the grid over the levels when there is one.)
    fn level_hit(&self, o: V3, d: V3, max: f32, who: Filter) -> bool {
        match &self.level_grid {
            Some(g) => RAY_SCRATCH.with(|scratch| {
                let cells = &mut scratch.borrow_mut().cells;
                cells.clear();
                g.collect(o, d, max, cells);
                cells.iter().any(|&i| {
                    let (f, m) = &self.level[i as usize];
                    who.collides(*f) && m.placed.ray(o, d, max).is_some()
                })
            }),
            None => self.level.iter().any(|(f, m)| who.collides(*f) && m.placed.ray(o, d, max).is_some()),
        }
    }

    /// Is anything that `who` collides with on the segment `o + t d` (`d` a unit vector), `0 <= t <= max`? The same answer as `cast(..).is_some()`, but it stops at the first thing it finds and tries the
    /// solids that blocked the last few questions first, so a run of lines of sight that go through the same wall costs one search.
    pub fn occluded(&self, o: V3, d: V3, max: f32, who: Filter) -> bool {
        if !(max >= 0.0) {
            return false;
        }
        let q = Quick::new(o, d, max);
        let blocks = |s: &Solid| who.collides(s.filter) && q.may_touch(s.plo, s.phi) && s.blocks(o, d, max);
        let solved = RAY_SCRATCH.with(|scratch| {
            let scratch = &mut *scratch.borrow_mut();
            let recent = scratch.recent;
            for (k, &r) in recent.iter().enumerate() {
                if r != 0 && (r as usize) <= self.solids.len() && blocks(&self.solids[r as usize - 1]) {
                    if k > 0 {
                        remember(&mut scratch.recent, r);
                    }
                    return true;
                }
            }
            let found = match &self.grid {
                Some(g) => {
                    let cells = &mut scratch.cells;
                    cells.clear();
                    g.collect(o, d, max, cells);
                    cells.iter().copied().find(|&i| blocks(&self.solids[i as usize]))
                }
                None => self.solids.iter().position(blocks).map(|i| i as u32),
            };
            if let Some(i) = found {
                remember(&mut scratch.recent, i + 1);
                return true;
            }
            false
        });
        solved
            || self.dynamic.iter().any(blocks)
            || self.grounds.iter().any(|(f, g)| who.collides(*f) && g.placed().ray(o, d, max).is_some())
            || self.level_hit(o, d, max, who)
            || self.hulls.iter().any(|(f, h)| who.collides(*f) && h.ray(o, d, max).is_some())
    }

    /// Tests: this world without its grid over the mesh levels (every ray then asks every level, as it always did).
    #[cfg(test)]
    pub(crate) fn without_level_grid(mut self) -> RayWorld {
        self.level_grid = None;
        self
    }

    /// Tests: how many mesh levels this world has and whether it made a grid over them.
    #[cfg(test)]
    pub(crate) fn levels(&self) -> (usize, bool) {
        (self.level.len(), self.level_grid.is_some())
    }

    /// The nearest hit of the ray `o + t d` (`d` a unit vector), `0 <= t <= max`, among what `who` collides with. `skip` is an entity to ignore.
    pub fn cast(&self, o: V3, d: V3, max: f32, who: Filter, skip: u64) -> Option<Hit> {
        if CAST_TIMING.with(|t| t.get()) {
            return self.cast_timed(o, d, max, who, skip);
        }
        self.cast_untimed(o, d, max, who, skip)
    }

    /// `cast`, with the time of its parts added to `CAST_MS` (while a profile is running).
    fn cast_timed(&self, o: V3, d: V3, max: f32, who: Filter, skip: u64) -> Option<Hit> {
        let t0 = std::time::Instant::now();
        let r = self.cast_parts(o, d, max, who, skip, true);
        CAST_MS.with(|m| {
            let mut m = m.borrow_mut();
            m[5] += t0.elapsed().as_secs_f64() * 1e3;
            m[6] += 1.0;
        });
        r
    }

    fn cast_untimed(&self, o: V3, d: V3, max: f32, who: Filter, skip: u64) -> Option<Hit> {
        self.cast_parts(o, d, max, who, skip, false)
    }

    #[inline(always)]
    fn cast_parts(&self, o: V3, d: V3, max: f32, who: Filter, skip: u64, timed: bool) -> Option<Hit> {
        let lap = |part: usize, since: &mut Option<std::time::Instant>| {
            if let Some(t) = since {
                let now = std::time::Instant::now();
                CAST_MS.with(|m| m.borrow_mut()[part] += now.duration_since(*t).as_secs_f64() * 1e3);
                *t = now;
            }
        };
        let mut clock = timed.then(std::time::Instant::now);
        let mut best: Option<Hit> = None;
        let mut take = |t: f32, normal: V3, uid: u64| {
            if best.is_none_or(|b| t < b.t) {
                best = Some(Hit { t, normal, uid });
            }
        };
        let mut test = |s: &Solid| {
            if s.uid == skip || !who.collides(s.filter) || !touches_box(o, d, max, s.lo, s.hi) {
                return;
            }
            if let Some((t, n)) = s.ray(o, d, max) {
                take(t, n, s.uid);
            }
        };
        match &self.grid {
            Some(g) => {
                // (the solids the ray may touch, in the order of the list: the same ones, in the same order, as looking at them all, so that the nearest of two equally near is the same)
                RAY_SCRATCH.with(|scratch| {
                    let found = &mut scratch.borrow_mut().cells;
                    found.clear();
                    g.collect(o, d, max, found);
                    found.sort_unstable();
                    found.dedup();
                    for &i in found.iter() {
                        test(&self.solids[i as usize]);
                    }
                });
            }
            None => {
                for s in self.solids.iter() {
                    test(s);
                }
            }
        }
        lap(0, &mut clock);
        for s in &self.dynamic {
            test(s);
        }
        lap(1, &mut clock);
        for (f, g) in &self.grounds {
            if g.uid != skip && who.collides(*f) {
                if let Some((t, n)) = g.placed().ray(o, d, max) {
                    take(t, n, g.uid);
                }
            }
        }
        lap(2, &mut clock);
        match &self.level_grid {
            Some(g) => {
                // (the levels the ray may touch, in the order of the list: the same ones in the same order as asking them all, so that the nearest of two equally near is the same)
                RAY_SCRATCH.with(|scratch| {
                    let found = &mut scratch.borrow_mut().cells;
                    found.clear();
                    g.collect(o, d, max, found);
                    found.sort_unstable();
                    found.dedup();
                    for &i in found.iter() {
                        let (f, m) = &self.level[i as usize];
                        if m.uid != skip && who.collides(*f) {
                            if let Some(h) = m.placed.ray(o, d, max) {
                                take(h.t, h.normal, m.uid);
                            }
                        }
                    }
                });
            }
            None => {
                for (f, m) in &self.level {
                    if m.uid != skip && who.collides(*f) {
                        if let Some(h) = m.placed.ray(o, d, max) {
                            take(h.t, h.normal, m.uid);
                        }
                    }
                }
            }
        }
        lap(3, &mut clock);
        for (f, h) in &self.hulls {
            if h.uid != skip && who.collides(*f) {
                if let Some((t, n)) = h.ray(o, d, max) {
                    take(t, n, h.uid);
                }
            }
        }
        lap(4, &mut clock);
        best
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    #[test]
    fn rays_hit_the_faces_of_boxes_spheres_and_capsules_and_start_outside() {
        // a box 2 x 2 x 2 at the origin, from above and from inside
        let (t, n) = ray_obb([0.3, 5.0, 0.2], [0.0, -1.0, 0.0], [0.0; 3], &ID, [1.0; 3]).unwrap();
        assert!((t - 4.0).abs() < 1e-5 && n == [0.0, 1.0, 0.0], "{t} {n:?}");
        assert!(ray_obb([0.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0; 3], &ID, [1.0; 3]).is_none(), "a ray that starts inside sees nothing");
        assert!(ray_obb([3.0, 5.0, 0.0], [0.0, -1.0, 0.0], [0.0; 3], &ID, [1.0; 3]).is_none(), "a miss");
        // a box turned 45 degrees about z: its corner points down
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let q = [0.0, 0.0, (std::f32::consts::FRAC_PI_8).sin(), (std::f32::consts::FRAC_PI_8).cos()];
        let axes = axes_of(q);
        let (t, n) = ray_obb([0.0, 5.0, 0.0], [0.0, -1.0, 0.0], [0.0; 3], &axes, [1.0; 3]).unwrap();
        assert!((t - (5.0 - 2.0f32.sqrt())).abs() < 1e-4, "the corner at y = sqrt(2): {t}");
        assert!(n[1] > 0.5, "{n:?}");
        let _ = s;
        // a ball
        let (t, n) = ray_sphere([0.0, 5.0, 0.0], [0.0, -1.0, 0.0], [0.0; 3], 1.0).unwrap();
        assert!((t - 4.0).abs() < 1e-5 && (n[1] - 1.0).abs() < 1e-5);
        assert!(ray_sphere([0.0, 0.5, 0.0], [0.0, -1.0, 0.0], [0.0; 3], 1.0).is_none());
        // a capsule lying along x: the side, the end, and a miss over the end
        let (t, n) = ray_capsule([0.0, 5.0, 0.0], [0.0, -1.0, 0.0], [-2.0, 0.0, 0.0], [2.0, 0.0, 0.0], 0.5).unwrap();
        assert!((t - 4.5).abs() < 1e-4 && (n[1] - 1.0).abs() < 1e-4, "{t} {n:?}");
        let (t, _) = ray_capsule([2.3, 5.0, 0.0], [0.0, -1.0, 0.0], [-2.0, 0.0, 0.0], [2.0, 0.0, 0.0], 0.5).unwrap();
        assert!(t > 4.5 && t < 5.0, "the round end: {t}");
        assert!(ray_capsule([2.6, 5.0, 0.0], [0.0, -1.0, 0.0], [-2.0, 0.0, 0.0], [2.0, 0.0, 0.0], 0.5).is_none());
    }

    /// A small xorshift so that the test needs nothing else.
    struct R(u64);
    impl R {
        fn f(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 40) as f32 / (1u64 << 24) as f32
        }
        fn between(&mut self, a: f32, b: f32) -> f32 {
            a + (b - a) * self.f()
        }
    }

    #[test]
    fn a_grid_over_the_solids_finds_exactly_what_looking_at_all_of_them_finds() {
        let mut r = R(0x2545_F491_4F6C_DD1D);
        let mut solids: Vec<Solid> = Vec::new();
        for uid in 0..600u64 {
            let c = [r.between(-120.0, 120.0), r.between(0.0, 12.0), r.between(-120.0, 120.0)];
            let (kind, lo, hi) = match uid % 3 {
                0 => {
                    let h = [r.between(0.4, 3.0), r.between(0.4, 4.0), r.between(0.4, 3.0)];
                    (Kind::Obb(c, ID, h), [c[0] - h[0], c[1] - h[1], c[2] - h[2]], [c[0] + h[0], c[1] + h[1], c[2] + h[2]])
                }
                1 => {
                    let rad = r.between(0.3, 2.0);
                    (Kind::Sphere(c, rad), [c[0] - rad, c[1] - rad, c[2] - rad], [c[0] + rad, c[1] + rad, c[2] + rad])
                }
                _ => {
                    let (a, b, rad) = ([c[0], c[1] - 1.0, c[2]], [c[0], c[1] + 1.0, c[2]], r.between(0.3, 1.0));
                    let (lo, hi) = segment_box(a, b, rad);
                    (Kind::Capsule(a, b, rad), lo, hi)
                }
            };
            solids.push(Solid::new(uid, Filter::default(), kind, lo, hi));
        }
        // a big floor (in no cell) and a very big wall
        solids.push(Solid::new(1000, Filter::default(), Kind::Obb([0.0, -0.5, 0.0], ID, [300.0, 0.5, 300.0]), [-300.0, -1.0, -300.0], [300.0, 0.0, 300.0]));
        solids.push(Solid::new(1001, Filter::default(), Kind::Obb([0.0, 20.0, 150.0], ID, [200.0, 20.0, 1.0]), [-200.0, 0.0, 149.0], [200.0, 40.0, 151.0]));
        let grid = SolidGrid::build(&solids);
        assert!(grid.is_some(), "a grid is made for this many solids");
        let n = solids.len();
        let with = RayWorld { solids: std::sync::Arc::new(solids.iter().map(|s| Solid::new(s.uid, s.filter, s.kind, s.lo, s.hi)).collect()), grid: grid.map(std::sync::Arc::new), ..RayWorld::default() };
        let without = RayWorld { solids: std::sync::Arc::new(solids), ..RayWorld::default() };
        let (mut hits, mut misses) = (0, 0);
        for k in 0..6000 {
            let o = [r.between(-160.0, 160.0), r.between(-2.0, 30.0), r.between(-160.0, 160.0)];
            // (mostly flat and slanted, some straight down and up, some very short and some very long)
            let mut d = match k % 5 {
                0 => [0.0, -1.0, 0.0],
                1 => [0.0, 1.0, 0.0],
                _ => [r.between(-1.0, 1.0), r.between(-0.4, 0.4), r.between(-1.0, 1.0)],
            };
            let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1.0e-6);
            d = [d[0] / l, d[1] / l, d[2] / l];
            let max = [3.0, 12.0, 40.0, 150.0, 400.0][(k / 5) % 5];
            let a = with.cast(o, d, max, Filter::default(), u64::MAX);
            let b = without.cast(o, d, max, Filter::default(), u64::MAX);
            assert_eq!(a.map(|h| (h.t.to_bits(), h.uid)), b.map(|h| (h.t.to_bits(), h.uid)), "ray {k}: from {o:?} along {d:?} for {max}");
            assert_eq!(with.occluded(o, d, max, Filter::default()), a.is_some(), "occluded, ray {k}: from {o:?} along {d:?} for {max}");
            assert_eq!(without.occluded(o, d, max, Filter::default()), a.is_some(), "occluded without the grid, ray {k}: from {o:?} along {d:?} for {max}");
            if a.is_some() {
                hits += 1;
            } else {
                misses += 1;
            }
        }
        assert!(hits > 800 && misses > 800, "the rays are a fair mix of hits and misses: {hits} {misses} among {n} solids");
    }

    /// The answer of `occluded` before the quick tests of boxes were put in (every solid by the dividing `touches_box` and the general `ray`).
    fn occluded_by_the_book(rw: &RayWorld, o: V3, d: V3, max: f32, who: Filter) -> bool {
        if !(max >= 0.0) {
            return false;
        }
        rw.solids.iter().any(|s| who.collides(s.filter) && touches_box(o, d, max, s.lo, s.hi) && s.ray(o, d, max).is_some())
    }

    /// The same world with every box, ball and capsule `by` bigger (smaller with a negative `by`) all round.
    fn grown(rw: &RayWorld, by: f32) -> RayWorld {
        let solids = rw
            .solids
            .iter()
            .map(|s| {
                let kind = match s.kind {
                    Kind::Obb(c, axes, h) => Kind::Obb(c, axes, [h[0] + by, h[1] + by, h[2] + by]),
                    Kind::Sphere(c, r) => Kind::Sphere(c, r + by),
                    Kind::Capsule(a, b, r) => Kind::Capsule(a, b, r + by),
                };
                Solid::new(s.uid, s.filter, kind, [s.lo[0] - by, s.lo[1] - by, s.lo[2] - by], [s.hi[0] + by, s.hi[1] + by, s.hi[2] + by])
            })
            .collect();
        RayWorld { solids: std::sync::Arc::new(solids), ..RayWorld::default() }
    }

    #[test]
    fn the_quick_tests_of_occluded_give_the_answer_of_the_slow_ones_for_every_ray_that_is_not_within_a_hair_of_an_edge() {
        let mut r = R(0x9E37_79B9_7F4A_7C15);
        let mut solids: Vec<Solid> = Vec::new();
        let mut aligned_boxes = Vec::new();
        for uid in 0..150u64 {
            // (a world far from the origin too: the rounding of the quick test must not matter there)
            let far = if uid % 5 == 0 { 8000.0 } else { 0.0 };
            let c = [r.between(-60.0, 60.0) + far, r.between(0.0, 6.0), r.between(-60.0, 60.0) - far];
            let (kind, lo, hi) = match uid % 6 {
                0..=2 => {
                    let h = [r.between(0.2, 6.0), r.between(0.2, 3.0), r.between(0.2, 6.0)];
                    aligned_boxes.push((c, h));
                    (Kind::Obb(c, ID, h), [c[0] - h[0], c[1] - h[1], c[2] - h[2]], [c[0] + h[0], c[1] + h[1], c[2] + h[2]])
                }
                3 => {
                    // (a box turned about y by some angle: not `aligned`)
                    let a = r.between(0.05, 3.0);
                    let axes = axes_of([0.0, (a * 0.5).sin(), 0.0, (a * 0.5).cos()]);
                    let h = [r.between(0.5, 5.0), r.between(0.5, 3.0), r.between(0.5, 5.0)];
                    let e = [0, 1, 2].map(|k| h[0] * axes[0][k].abs() + h[1] * axes[1][k].abs() + h[2] * axes[2][k].abs());
                    (Kind::Obb(c, axes, h), [c[0] - e[0], c[1] - e[1], c[2] - e[2]], [c[0] + e[0], c[1] + e[1], c[2] + e[2]])
                }
                4 => {
                    let rad = r.between(0.3, 2.0);
                    (Kind::Sphere(c, rad), [c[0] - rad, c[1] - rad, c[2] - rad], [c[0] + rad, c[1] + rad, c[2] + rad])
                }
                _ => {
                    let (a, b, rad) = ([c[0], c[1] - 1.0, c[2]], [c[0] + 1.5, c[1] + 1.0, c[2]], r.between(0.3, 1.0));
                    let (lo, hi) = segment_box(a, b, rad);
                    (Kind::Capsule(a, b, rad), lo, hi)
                }
            };
            solids.push(Solid::new(uid, Filter::default(), kind, lo, hi));
        }
        assert!(solids.iter().any(|s| s.aligned) && solids.iter().any(|s| !s.aligned && matches!(s.kind, Kind::Obb(..))), "both kinds of box are in the world");
        let rw = RayWorld { solids: std::sync::Arc::new(solids), ..RayWorld::default() };
        // (the answer of a ray that passes within 4 mm of the face or edge of something is not to be relied on in either version: it depends on how the last bit of a division was rounded. The other rays must all agree)
        let (big, small) = (grown(&rw, 4.0e-3), grown(&rw, -4.0e-3));
        let (mut hit, mut miss, mut hair) = (0u32, 0u32, 0u32);
        for k in 0..60_000u32 {
            let far = if k % 5 == 0 { 8000.0 } else { 0.0 };
            let mut o = [r.between(-80.0, 80.0) + far, r.between(-1.0, 8.0), r.between(-80.0, 80.0) - far];
            let mut d = [r.between(-1.0, 1.0), r.between(-0.3, 0.3), r.between(-1.0, 1.0)];
            // a ray along an axis (or nearly) and a ray that starts in line with a face or an edge of a box: the cases where a quick test may be wrong
            match k % 7 {
                0 => d = [1.0, 0.0, 0.0],
                1 => d = [0.0, 0.0, -1.0],
                2 => d = [0.0, 1.0e-13, 1.0],
                3 => {
                    let (c, h) = aligned_boxes[(k as usize / 7) % aligned_boxes.len()];
                    // starts level with the top edge of a box and runs along its top, or just above or below the face (a hair from the edge)
                    let hair = [0.0, 1.0e-5, -1.0e-5, 1.0e-3, -1.0e-3, 2.0e-7, -2.0e-7, 1.0e-2, -1.0e-2][(k as usize / 49) % 9];
                    o = [c[0] - h[0] - 5.0, c[1] + h[1] + hair, c[2] + hair];
                    d = [1.0, 0.0, 0.0];
                }
                4 => {
                    let (c, h) = aligned_boxes[(k as usize / 7) % aligned_boxes.len()];
                    // aimed at a corner of a box
                    let corner = [c[0] + h[0] * [1.0, -1.0][k as usize % 2], c[1] + h[1], c[2] - h[2]];
                    d = [corner[0] - o[0], corner[1] - o[1], corner[2] - o[2]];
                }
                _ => {}
            }
            let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1.0e-6);
            d = [d[0] / l, d[1] / l, d[2] / l];
            let max = [0.5, 3.0, 12.0, 40.0, 150.0][(k / 3) as usize % 5];
            let want = occluded_by_the_book(&rw, o, d, max, Filter::default());
            let robust = occluded_by_the_book(&big, o, d, max, Filter::default()) == occluded_by_the_book(&small, o, d, max, Filter::default());
            let got = rw.occluded(o, d, max, Filter::default());
            if robust {
                assert_eq!(got, want, "ray {k} from {o:?} along {d:?} for {max}");
                if want {
                    hit += 1;
                } else {
                    miss += 1;
                }
            } else {
                hair += 1;
            }
        }
        assert!(hit > 8000 && miss > 8000 && hair > 100, "a fair mix, with rays at the edges too: {hit} {miss} {hair}");
    }

    #[test]
    fn the_static_solids_of_a_wide_gather_are_kept_while_nothing_static_changes_and_answer_as_a_fresh_gather_does() {
        use crate::{Command, Sim};
        let mut s = Sim::new(1);
        let spawn = |s: &mut Sim, uid: u64, comps: &str| {
            let lunaris_json::Json::Obj(components) = lunaris_json::parse(comps).unwrap() else { panic!() };
            s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
        };
        for k in 0..200u64 {
            spawn(&mut s, 1 + k, &format!(r#"{{"Transform":{{"pos":[{},1,{}]}},"Collider":{{"shape":1,"size":[2,2,2]}}}}"#, (k % 20) * 5, (k / 20) * 5));
        }
        // a drone: a collider and a velocity and no rigid body
        spawn(&mut s, 900, r#"{"Transform":{"pos":[2,3,2]},"Collider":{"shape":1,"size":[1,1,1]},"Velocity":{"lin":[1,0,0]}}"#);
        let gather = |s: &mut Sim, reach: Option<(V3, V3)>| RayWorld::gather(&mut s.world, &s.heightfields, &s.meshes, reach);
        let a = gather(&mut s, None);
        let b = gather(&mut s, None);
        assert!(std::sync::Arc::ptr_eq(&a.solids, &b.solids), "nothing static changed: the same solids");
        assert_eq!((a.solids.len(), a.dynamic.len()), (200, 1));
        // the drone moves: the static solids are still the same, the drone is where it is now
        let e = s.world.entity(900).unwrap();
        s.world.get_mut::<Transform>(e).unwrap().pos[0] = 40.0;
        let c = gather(&mut s, None);
        assert!(std::sync::Arc::ptr_eq(&a.solids, &c.solids), "a drone is not a static solid");
        assert!(c.dynamic[0].lo[0] > 30.0 && a.dynamic[0].lo[0] < 10.0);
        // a static collider moves: other solids
        let e = s.world.entity(7).unwrap();
        s.world.get_mut::<Transform>(e).unwrap().pos[1] = 5.0;
        let d = gather(&mut s, None);
        assert!(!std::sync::Arc::ptr_eq(&a.solids, &d.solids), "a static collider moved");
        // a box that is small (gathered afresh, with no cache) and one that is wide give the same answer for every ray
        let small = gather(&mut s, Some(([-5.0, -5.0, -5.0], [100.0, 20.0, 100.0])));
        let wide = gather(&mut s, Some(([-500.0, -5.0, -500.0], [500.0, 20.0, 500.0])));
        assert_eq!((small.solids.len(), wide.solids.len()), (200, 200));
        for k in 0..400 {
            let f = k as f32 * 0.713;
            let o = [(f.sin() * 0.5 + 0.5) * 100.0, 8.0, (f.cos() * 0.5 + 0.5) * 100.0];
            let dir = {
                let v = [f.cos() * 0.4, -1.0, f.sin() * 0.4];
                let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                [v[0] / l, v[1] / l, v[2] / l]
            };
            let (x, y, z) = (small.cast(o, dir, 20.0, Filter::DEFAULT, 0), wide.cast(o, dir, 20.0, Filter::DEFAULT, 0), d.cast(o, dir, 20.0, Filter::DEFAULT, 0));
            assert_eq!(x.map(|h| (h.uid, h.t.to_bits())), y.map(|h| (h.uid, h.t.to_bits())), "ray {k}");
            assert_eq!(x.map(|h| (h.uid, h.t.to_bits())), z.map(|h| (h.uid, h.t.to_bits())), "ray {k} (the shared world)");
            assert_eq!(small.occluded(o, dir, 20.0, Filter::DEFAULT), x.is_some());
        }
        // a part of a kept world, and the kept world with entities left out
        let part = gather(&mut s, Some(([0.0, -5.0, 0.0], [30.0, 20.0, 30.0])));
        assert!(part.solids.len() < 200 && part.solids.len() >= 36, "{}", part.solids.len());
        let without: std::collections::BTreeSet<u64> = [1u64, 2].into_iter().collect();
        let gone = RayWorld::gather_except(&mut s.world, &s.heightfields, &s.meshes, None, &without);
        assert_eq!(gone.solids.len(), 198);
        assert!(gone.solids.iter().all(|x| x.uid != 1 && x.uid != 2));
        // the same entities left out again: the same solids, kept (not made again); other ones: others; a box with the exceptions answers as the world without them
        let again = RayWorld::gather_except(&mut s.world, &s.heightfields, &s.meshes, Some(([-500.0, -5.0, -500.0], [500.0, 20.0, 500.0])), &without);
        assert!(std::sync::Arc::ptr_eq(&gone.solids, &again.solids), "the same exceptions of the same static world are made once");
        let other: std::collections::BTreeSet<u64> = [3u64].into_iter().collect();
        let third = RayWorld::gather_except(&mut s.world, &s.heightfields, &s.meshes, None, &other);
        assert!(!std::sync::Arc::ptr_eq(&gone.solids, &third.solids) && third.solids.len() == 199);
        for k in 0..400 {
            let f = k as f32 * 0.713;
            let o = [(f.sin() * 0.5 + 0.5) * 100.0, 8.0, (f.cos() * 0.5 + 0.5) * 100.0];
            let v = [f.cos() * 0.4, -1.0, f.sin() * 0.4];
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            let dir = [v[0] / l, v[1] / l, v[2] / l];
            let by_hand = RayWorld { solids: std::sync::Arc::new(d.solids.iter().filter(|x| x.uid != 1 && x.uid != 2).cloned().collect()), ..RayWorld::default() };
            assert_eq!(again.cast(o, dir, 30.0, Filter::DEFAULT, 0).map(|h| (h.uid, h.t.to_bits())), by_hand.cast(o, dir, 30.0, Filter::DEFAULT, 0).map(|h| (h.uid, h.t.to_bits())), "ray {k} with entities left out");
        }
    }

    #[test]
    fn what_changed_between_two_gatherings_is_the_box_round_what_was_added_taken_away_or_moved() {
        let boxed = |uid: u64, c: V3, h: V3| Solid::new(uid, Filter::default(), Kind::Obb(c, ID, h), [c[0] - h[0], c[1] - h[1], c[2] - h[2]], [c[0] + h[0], c[1] + h[1], c[2] + h[2]]);
        let world = |solids: Vec<Solid>| RayWorld { solids: std::sync::Arc::new(solids), ..RayWorld::default() };
        let base = || vec![boxed(1, [0.0, 1.0, 0.0], [5.0, 1.0, 0.5]), boxed(4, [20.0, 1.0, 0.0], [0.5, 1.0, 5.0]), boxed(9, [-30.0, 1.0, 10.0], [1.0, 1.0, 1.0])];
        let old = world(base());
        assert_eq!(world(base()).changed_since(&old), Changed::Nothing, "the same world");
        // a box added (uid 6, between the others in the order): the box round it
        let mut added = base();
        added.insert(2, boxed(6, [50.0, 1.0, 50.0], [0.5, 0.5, 0.5]));
        assert_eq!(world(added).changed_since(&old), Changed::Boxes(vec![([49.5, 0.5, 49.5], [50.5, 1.5, 50.5])]));
        // one taken away (the last, and the first)
        let mut gone = base();
        gone.pop();
        assert_eq!(world(gone).changed_since(&old), Changed::Boxes(vec![([-31.0, 0.0, 9.0], [-29.0, 2.0, 11.0])]));
        let mut gone = base();
        gone.remove(0);
        assert_eq!(world(gone).changed_since(&old), Changed::Boxes(vec![([-5.0, 0.0, -0.5], [5.0, 2.0, 0.5])]));
        // one moved: the box holds where it was and where it is
        let mut moved = base();
        moved[1] = boxed(4, [20.0, 1.0, 8.0], [0.5, 1.0, 5.0]);
        assert_eq!(world(moved).changed_since(&old), Changed::Boxes(vec![([19.5, 0.0, 3.0], [20.5, 2.0, 13.0]), ([19.5, 0.0, -5.0], [20.5, 2.0, 5.0])]));
        // one that has changed its layer only is a change too, and two changes are in one box
        let mut layer = base();
        layer[0].filter = Filter::new(3, 0);
        let mut both = layer;
        both.push(boxed(11, [0.0, 0.0, 100.0], [1.0, 1.0, 1.0]));
        assert_eq!(world(both).changed_since(&old), Changed::Boxes(vec![([-5.0, 0.0, -0.5], [5.0, 2.0, 0.5]), ([-5.0, 0.0, -0.5], [5.0, 2.0, 0.5]), ([-1.0, -1.0, 99.0], [1.0, 1.0, 101.0])]));
        // (more than sixteen are merged by the 64 m squares they are in: twenty in a row 190 m long are three boxes)
        let mut crowd = base();
        crowd.extend((0..20).map(|k| boxed(100 + k, [k as f32 * 10.0, 0.0, 200.0], [1.0, 1.0, 1.0])));
        match world(crowd).changed_since(&old) {
            Changed::Boxes(b) => {
                assert_eq!(b.len(), 3, "{b:?}");
                assert!(b.iter().all(|(lo, hi)| lo[2] == 199.0 && hi[2] == 201.0), "{b:?}");
            }
            other => panic!("{other:?}"),
        }
        // (scattered over more than sixteen squares it is a change everywhere)
        let mut scattered = base();
        scattered.extend((0..40).map(|k| boxed(200 + k, [k as f32 * 100.0, 0.0, 400.0], [1.0, 1.0, 1.0])));
        assert_eq!(world(scattered).changed_since(&old), Changed::Everything);
        // and the signature, which is what a game compares, is what it was for all of them
        assert_eq!(old.signature(), world(base()).signature());
    }
}
