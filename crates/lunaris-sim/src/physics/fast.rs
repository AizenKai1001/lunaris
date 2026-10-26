//! The fast path of the physics step. It gives exactly the bits of `step_reference` (the tests run both on random worlds and compare the state hash
//! of every tick) but its cost follows the bodies that move, not the scenery that does not:
//!
//! * The colliders that never move (a `Transform` and a `Collider`, no `RigidBody`) are read straight from their archetype columns and kept between
//!   ticks in a grid (`StaticSet`). Each tick the columns are compared with the copy the grid was made from (a memory compare, a few ns per collider);
//!   when they differ, only that archetype's grid is made again.
//! * The bodies that can move (everything else with a collider, triggers too) go in a grid made every tick, and look for their neighbours in it and in
//!   the grids of the scenery. A pair is evaluated by the very test the sweep of the reference uses (`accept`), so the pairs that reach the narrow
//!   phase are the same ones; the contacts are then solved, corrected and written back by the same code as in the reference.
//! * The reference sweeps along the axis where the bodies are most spread out, which decides only pairs whose boxes touch to within rounding. The fast
//!   path finds that axis (an exact sum over every body, in uid order, like the reference) only on a tick where such a pair turns up.
//!
//! What it does not handle goes to the reference (`step` returns `None` before changing anything): terrain, meshes, convex hulls, bodies that
//! turn (`AngularVelocity`), joints, and non-finite numbers.
use super::*;
use lunaris_ecs::{Archetype, ComponentId};
use crate::pool::Pool;
use std::sync::{Mutex, OnceLock};

/// What the tests count: steps that took the fast path, contacts they found, and steps on which the sweep axis had to be worked out.
#[cfg(test)]
pub(crate) static FAST_TICKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(test)]
pub(crate) static FAST_CONTACTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(test)]
pub(crate) static PAR_TICKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(test)]
pub(crate) static CCD_FIXES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(test)]
pub(crate) static AXIS_TICKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A body that covers more cells than this is kept out of the grid and tested against everything instead (a floor, a long wall).
const MAX_CELLS: i64 = 64;
const CELL_LIMIT: f32 = 1_048_575.0;
/// With fewer bodies than this a step is not spread over threads (waking them costs more than the work), and the tests use a tiny number so that the
/// threaded code runs in every test.
#[cfg(test)]
const PAR_MIN: usize = 8;
#[cfg(not(test))]
const PAR_MIN: usize = 1024;
/// How many bodies (or rows) one task of a threaded stage covers.
const CHUNK: usize = 64;
/// The largest number of cells a scenery grid may have as one array (more spread out than that, it is a hash table).
const DENSE_MAX: i64 = 1 << 22;

/// What the physics step keeps from one tick to the next: the grids of the scenery. Derived data that is checked against the world every tick, so
/// a stale copy is never used; a clone of it starts empty.
#[derive(Default)]
pub struct PhysicsCache {
    inner: Arc<Mutex<Inner>>,
}

impl Clone for PhysicsCache {
    fn clone(&self) -> PhysicsCache {
        PhysicsCache::default()
    }
}

/// The parts of a fast step that `PhysicsCache::stage_ns` times (while `set_profile(true)`).
pub const STAGES: [&str; 9] = ["gather", "sleepers, sort", "forces, ccd", "grid of bodies", "neighbour scan", "narrow phase", "solve", "positions", "sleep, write back"];

impl PhysicsCache {
    /// Time the parts of the fast step from now on (a few clock reads a tick); `stage_ns` reads the totals and clears them.
    pub fn set_profile(&self, on: bool) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).profile = on;
    }

    /// The nanoseconds spent in each part of the fast step since the last call (see `STAGES`); all zero unless `set_profile(true)`.
    pub fn stage_ns(&self) -> [u64; 9] {
        std::mem::take(&mut self.inner.lock().unwrap_or_else(|e| e.into_inner()).stage_ns)
    }

    /// Another handle to the same cache (what a tick's context holds while the simulation keeps its own).
    pub fn share(&self) -> PhysicsCache {
        PhysicsCache { inner: self.inner.clone() }
    }
}

#[derive(Default)]
struct Inner {
    sets: Vec<StaticSet>,
    /// Every collider of every set, in uid order, with its position: what the sweep axis is computed from (the reference sums over all bodies in uid order).
    by_uid: Vec<(u64, [f32; 3])>,
    /// Counts the ticks that touched scenery (a collider that is in a pair this tick has `stamp == cur`).
    cur: u32,
    /// Kept between ticks only to avoid allocating: the grid of the bodies and the cell ranges they cover.
    dgrid: DynGrid,
    /// How many bodies the last step gathered (to reserve room for them).
    last_n0: usize,
    /// The worker threads of the step, made when a step has enough bodies to want them.
    pool: Option<std::sync::Arc<Pool>>,
    dlo: Vec<[i32; 3]>,
    dhi: Vec<[i32; 3]>,
    dspan: Vec<u32>,
    /// The sleepers as a set of colliders that never move (what an awake body looks in to wake them), and the generation of the list of sleepers it was made from.
    sleep_set: Option<(u64, StaticSet)>,
    /// Timing of the parts of a step (`PhysicsCache::set_profile`).
    profile: bool,
    stage_ns: [u64; 9],
    stage_t: Option<std::time::Instant>,
}

/// One scenery collider as the broad phase needs it, kept in the cell lists (a collider that covers several cells is there once per cell).
#[derive(Clone, Copy)]
#[repr(C)]
struct Entry {
    uid: u64,
    pos: [f32; 3],
    ext: [f32; 3],
    /// The first cell it covers (a pair is looked at in the cell that holds the later corner of both boxes).
    lo: [i32; 3],
    /// Its number in the set (for the `stamp` and `slot` of this tick and for the row in the archetype).
    r: u32,
}

/// Cells of a grid: which of the things listed in `items` (indices) are in each cell. Open addressing on the packed cell coordinates.
#[derive(Default)]
struct Table {
    bits: u32,
    keys: Vec<u64>,
    start: Vec<u32>,
    len: Vec<u32>,
    items: Vec<u32>,
    /// The slot of every (thing, cell) pair, in the order the pairs are visited, so that the second pass does not look the cells up again.
    slots: Vec<u32>,
}

const EMPTY: u64 = u64::MAX;

#[inline]
fn cell_key(c: [i32; 3]) -> u64 {
    (c[0] as u32 as u64 & 0x1F_FFFF) | ((c[1] as u32 as u64 & 0x1F_FFFF) << 21) | ((c[2] as u32 as u64 & 0x1F_FFFF) << 42)
}

#[inline]
fn slot_of(key: u64, bits: u32) -> usize {
    (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - bits)) as usize
}

/// The cells (inclusive ranges per axis) that the box of a body covers, a little wider than the box so that rounding never loses a neighbour.
#[inline]
fn cell_range(pos: [f32; 3], ext: [f32; 3], inv: f32) -> ([i32; 3], [i32; 3]) {
    let (mut lo, mut hi) = ([0i32; 3], [0i32; 3]);
    for a in 0..3 {
        let eps = (pos[a].abs() + ext[a]) * 5.0e-7 + 1.0e-6;
        lo[a] = ((pos[a] - ext[a] - eps) * inv).floor().clamp(-CELL_LIMIT, CELL_LIMIT) as i32;
        hi[a] = ((pos[a] + ext[a] + eps) * inv).floor().clamp(-CELL_LIMIT, CELL_LIMIT) as i32;
    }
    (lo, hi)
}

#[inline]
fn span(lo: [i32; 3], hi: [i32; 3]) -> i64 {
    (0..3).map(|a| (hi[a] as i64 - lo[a] as i64 + 1).max(1)).product()
}

impl Table {
    /// Fills the table with the things `0..lo.len()` whose cell count `span[i]` is not 0 (0 stands for "too big for the grid"). Its buffers are
    /// kept, so that making it again every tick costs no allocation.
    fn rebuild(&mut self, lo: &[[i32; 3]], hi: &[[i32; 3]], span: &[u32]) {
        let total: usize = span.iter().map(|&s| s as usize).sum();
        let size = (total * 3 / 2).max(8).next_power_of_two().max(16);
        self.bits = size.trailing_zeros();
        self.keys.clear();
        self.keys.resize(size, EMPTY);
        self.len.clear();
        self.len.resize(size, 0);
        self.start.resize(size, 0);
        self.items.clear();
        self.items.resize(total, 0);
        self.slots.clear();
        for i in 0..lo.len() {
            if span[i] == 0 {
                continue;
            }
            for z in lo[i][2]..=hi[i][2] {
                for y in lo[i][1]..=hi[i][1] {
                    for x in lo[i][0]..=hi[i][0] {
                        let s = self.slot([x, y, z]);
                        self.len[s] += 1;
                        self.slots.push(s as u32);
                    }
                }
            }
        }
        let mut at = 0u32;
        for s in 0..size {
            self.start[s] = at;
            at += self.len[s];
            self.len[s] = 0;
        }
        let mut visit = 0usize;
        for i in 0..lo.len() {
            if span[i] == 0 {
                continue;
            }
            for _ in 0..span[i] {
                let s = self.slots[visit] as usize;
                visit += 1;
                let k = (self.start[s] + self.len[s]) as usize;
                self.items[k] = i as u32;
                self.len[s] += 1;
            }
        }
    }

    /// The slot of a cell (made if it is new).
    #[inline]
    fn slot(&mut self, c: [i32; 3]) -> usize {
        let key = cell_key(c);
        let mask = (1usize << self.bits) - 1;
        let mut s = slot_of(key, self.bits);
        loop {
            let k = self.keys[s];
            if k == EMPTY {
                self.keys[s] = key;
                return s;
            }
            if k == key {
                return s;
            }
            s = (s + 1) & mask;
        }
    }

    /// Where the things of a cell are in `items`: start and length.
    #[inline]
    fn range_of(&self, c: [i32; 3]) -> (usize, usize) {
        if self.keys.is_empty() {
            return (0, 0);
        }
        let key = cell_key(c);
        let mask = (1usize << self.bits) - 1;
        let mut s = slot_of(key, self.bits);
        loop {
            let k = self.keys[s];
            if k == key {
                return (self.start[s] as usize, self.len[s] as usize);
            }
            if k == EMPTY {
                return (0, 0);
            }
            s = (s + 1) & mask;
        }
    }

    #[inline]
    fn items_in(&self, c: [i32; 3]) -> &[u32] {
        let (a, n) = self.range_of(c);
        &self.items[a..a + n]
    }
}

/// The grid of the bodies, made every tick: one array of cells when the bodies are not spread too thin (cheap and compact), a hash table otherwise.
#[derive(Default)]
struct DynGrid {
    dense: bool,
    min: [i32; 3],
    dim: [i64; 3],
    start: Vec<u32>,
    fill: Vec<u32>,
    items: Vec<u32>,
    table: Table,
}

impl DynGrid {
    fn rebuild(&mut self, lo: &[[i32; 3]], hi: &[[i32; 3]], span: &[u32]) {
        let total: usize = span.iter().map(|&s| s as usize).sum();
        let (mut cmin, mut cmax) = ([i32::MAX; 3], [i32::MIN; 3]);
        for i in 0..lo.len() {
            if span[i] != 0 {
                for a in 0..3 {
                    cmin[a] = cmin[a].min(lo[i][a]);
                    cmax[a] = cmax[a].max(hi[i][a]);
                }
            }
        }
        let dim = if total == 0 { [0i64; 3] } else { [cmax[0] as i64 - cmin[0] as i64 + 1, cmax[1] as i64 - cmin[1] as i64 + 1, cmax[2] as i64 - cmin[2] as i64 + 1] };
        let cells = dim[0] * dim[1] * dim[2];
        if total > 0 && cells <= DENSE_MAX && cells <= 16 * total as i64 + 4096 {
            self.dense = true;
            self.min = cmin;
            self.dim = dim;
            self.start.clear();
            self.start.resize(cells as usize + 1, 0);
            let at = |c: [i32; 3]| ((c[0] as i64 - cmin[0] as i64) + dim[0] * ((c[1] as i64 - cmin[1] as i64) + dim[1] * (c[2] as i64 - cmin[2] as i64))) as usize;
            for i in 0..lo.len() {
                if span[i] == 0 {
                    continue;
                }
                for z in lo[i][2]..=hi[i][2] {
                    for y in lo[i][1]..=hi[i][1] {
                        for x in lo[i][0]..=hi[i][0] {
                            self.start[at([x, y, z]) + 1] += 1;
                        }
                    }
                }
            }
            for c in 0..cells as usize {
                self.start[c + 1] += self.start[c];
            }
            self.fill.clear();
            self.fill.extend_from_slice(&self.start);
            self.items.clear();
            self.items.resize(total, 0);
            for i in 0..lo.len() {
                if span[i] == 0 {
                    continue;
                }
                for z in lo[i][2]..=hi[i][2] {
                    for y in lo[i][1]..=hi[i][1] {
                        for x in lo[i][0]..=hi[i][0] {
                            let c = at([x, y, z]);
                            self.items[self.fill[c] as usize] = i as u32;
                            self.fill[c] += 1;
                        }
                    }
                }
            }
        } else {
            self.dense = false;
            self.table.rebuild(lo, hi, span);
        }
    }

    /// The bodies in a cell.
    #[inline]
    fn items_in(&self, c: [i32; 3]) -> &[u32] {
        if !self.dense {
            return self.table.items_in(c);
        }
        let (x, y, z) = (c[0] as i64 - self.min[0] as i64, c[1] as i64 - self.min[1] as i64, c[2] as i64 - self.min[2] as i64);
        if x < 0 || y < 0 || z < 0 || x >= self.dim[0] || y >= self.dim[1] || z >= self.dim[2] {
            return &[];
        }
        let i = (x + self.dim[0] * (y + self.dim[1] * z)) as usize;
        &self.items[self.start[i] as usize..self.start[i + 1] as usize]
    }
}

/// How a scenery grid finds the entries of a cell.
enum Index {
    /// One array of cells over the box that holds all the scenery: `start[i]..start[i + 1]` are the entries of cell `i` (x runs fastest, so the
    /// cells next to each other are next to each other in memory).
    Dense { min: [i32; 3], dim: [i64; 3], start: Vec<u32> },
    /// Scenery too spread out for that.
    Hash(Table),
}

/// The scenery of one archetype: colliders with no `RigidBody` that are not triggers.
struct StaticSet {
    arch: usize,
    comps: Vec<u32>,
    entities: Vec<Entity>,
    tf: Vec<Transform>,
    col: Vec<Collider>,
    /// For each collider of the grid (its number is `Entry::r`): its row in the archetype, and its entry.
    rows: Vec<u32>,
    recs: Vec<Entry>,
    inv: f32,
    grid: Grid,
    /// The numbers of the colliders that cover too many cells for the grid.
    big: Vec<u32>,
    stamp: Vec<u32>,
    slot: Vec<u32>,
    /// The sum of a mix of each collider's uid, place and size (`sleep::mix`): the signature of the scenery that the sleepers compare.
    sig: u64,
}

#[inline]
fn finite3(v: [f32; 3]) -> bool {
    v[0].is_finite() && v[1].is_finite() && v[2].is_finite()
}

/// The shape of a collider and the box around it, as the reference makes them (`step_reference`, the loop over `raw`) for a body that does not spin.
fn shape_parts(t: &Transform, c: &Collider) -> (Shape, [f32; 3], bool, bool) {
    let scale = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
    let turned = t.rot[0] != 0.0 || t.rot[1] != 0.0 || t.rot[2] != 0.0;
    let mut rich = false;
    let shape = if c.shape == 2 {
        rich = true;
        let r = 0.5 * c.size[0].abs() * scale[0].max(scale[2]);
        Shape::Capsule(r, (0.5 * c.size[1].abs() * scale[1] - r).max(0.0))
    } else if c.shape == 0 {
        Shape::Sphere(0.5 * c.size[0].abs() * scale[0].max(scale[1]).max(scale[2]))
    } else {
        let h = [0.5 * c.size[0].abs() * scale[0], 0.5 * c.size[1].abs() * scale[1], 0.5 * c.size[2].abs() * scale[2]];
        if turned {
            rich = true;
            Shape::Obb(h)
        } else {
            Shape::Box(h)
        }
    };
    let ext = if turned { Body::bounds(shape, &rot::axes_of(rot::normalize_q(t.rot))) } else { Body::bounds(shape, &rot::IDENTITY) };
    (shape, ext, rich, turned)
}

/// A body made like the reference makes it, for a collider whose body does not spin (`rb` and `v` are its `RigidBody` and `Velocity`, if it has them).
fn build_body(uid: u64, entity: Entity, t: &Transform, c: &Collider, rb: Option<&RigidBody>, v: Option<&Velocity>, filter: Filter) -> Body {
    let (mut inv_mass, mut restitution, mut friction, mut gravity_scale, mut drag, mut vel) = (0.0, 0.3, 0.5, 1.0, 0.0, [0.0; 3]);
    if let (Some(rb), Some(v)) = (rb, v) {
        if rb.mass > 0.0 {
            inv_mass = 1.0 / rb.mass;
            vel = v.lin;
        }
        restitution = rb.restitution.clamp(0.0, 1.0);
        friction = rb.friction.max(0.0);
        gravity_scale = rb.gravity_scale;
        drag = rb.drag.max(0.0);
    } else if let Some(rb) = rb {
        restitution = rb.restitution.clamp(0.0, 1.0);
        friction = rb.friction.max(0.0);
    }
    let (shape, ext, rich, turned) = shape_parts(t, c);
    let state = if turned {
        let mut r = rot::Rot { q: rot::normalize_q(t.rot), ..rot::Rot::default() };
        r.axes = rot::axes_of(r.q);
        Some(Box::new(r))
    } else {
        None
    };
    Body { uid, entity, pos: t.pos, vel, shape, ext, inv_mass, restitution, friction, gravity_scale, drag, trigger: c.trigger, filter, spins: false, rich, asleep: false, rot: state }
}

/// (The size is only ever used through its absolute value, so `==` on it is exact enough: -0.0 and 0.0 make the same shape. A NaN never compares
/// equal, which only makes the cache build again, and a NaN size makes the step go to the reference anyway.)
#[inline]
fn same_collider(a: &Collider, b: &Collider) -> bool {
    a.shape == b.shape && a.trigger == b.trigger && a.size == b.size
}

fn same_transforms(a: &[Transform], b: &[Transform]) -> bool {
    const _: () = assert!(std::mem::size_of::<Transform>() == 40, "Transform has no padding, so its bytes can be compared");
    a.len() == b.len() && {
        // SAFETY: a `Transform` is ten `f32`s with no padding, so every byte of the slices is initialised.
        let (x, y) = unsafe { (std::slice::from_raw_parts(a.as_ptr().cast::<u8>(), std::mem::size_of_val(a)), std::slice::from_raw_parts(b.as_ptr().cast::<u8>(), std::mem::size_of_val(b))) };
        x == y
    }
}

impl StaticSet {
    /// The sleepers as a set of colliders that never move, for the bodies that look for neighbours: an awake body that touches one wakes it, a trigger sees it
    /// like any scenery.
    fn of_sleepers(sleepers: &[sleep::Sleeper]) -> StaticSet {
        let m = sleepers.len();
        let uid_pos: Vec<(u64, [f32; 3])> = sleepers.iter().map(|s| (s.uid, s.pos())).collect();
        let exts: Vec<[f32; 3]> = sleepers.iter().map(|s| s.ext).collect();
        let sizes: Vec<f32> = exts.iter().map(|e| e[0].max(e[1]).max(e[2])).collect();
        let (recs, inv, grid, big) = make_grid(&uid_pos, &exts, sizes);
        StaticSet {
            arch: usize::MAX,
            comps: Vec::new(),
            entities: vec![Entity { index: 0, generation: 0 }; m],
            tf: sleepers.iter().map(|s| s.transform()).collect(),
            col: sleepers.iter().map(|s| s.collider()).collect(),
            rows: (0..m as u32).collect(),
            recs,
            inv,
            grid,
            big,
            stamp: vec![0; m],
            slot: vec![0; m],
            sig: 0,
        }
    }

    /// Does this set still describe the archetype (same entities, same colliders, same places)?
    fn matches(&self, ai: usize, a: &Archetype, tf: &[Transform], col: &[Collider]) -> bool {
        // (an entity handle never changes its uid, so equal entities have the same uids)
        self.arch == ai && self.entities.len() == a.len() && self.entities == a.entities() && same_transforms(&self.tf, tf) && self.col.iter().zip(col).all(|(x, y)| same_collider(x, y)) && self.comps.iter().copied().eq(a.component_ids().map(|c| c.0))
    }

    /// Reads the archetype; `None` when something in it is not a finite number (the reference decides what that does).
    fn build(world: &World, ai: usize, a: &Archetype, tf: &[Transform], col: &[Collider]) -> Option<StaticSet> {
        let n = a.len();
        let entities: Vec<Entity> = a.entities().to_vec();
        let mut rows: Vec<u32> = Vec::with_capacity(n);
        let mut uid_pos: Vec<(u64, [f32; 3])> = Vec::with_capacity(n);
        let mut exts: Vec<[f32; 3]> = Vec::with_capacity(n);
        let mut sizes: Vec<f32> = Vec::with_capacity(n);
        for row in 0..n {
            if col[row].trigger {
                continue; // (a trigger is a body of its own, made every tick)
            }
            let (_, ext, _, _) = shape_parts(&tf[row], &col[row]);
            if !finite3(tf[row].pos) || !finite3(ext) {
                return None;
            }
            rows.push(row as u32);
            uid_pos.push((world.uid_of(entities[row]).unwrap_or(u64::MAX), tf[row].pos));
            exts.push(ext);
            sizes.push(ext[0].max(ext[1]).max(ext[2]));
        }
        let (recs, inv, grid, big) = make_grid(&uid_pos, &exts, sizes);
        let m = rows.len();
        let sig = recs.iter().fold(0u64, |s, e| s.wrapping_add(sleep::mix(e.uid, e.pos, e.ext)));
        Some(StaticSet { arch: ai, comps: a.component_ids().map(|c| c.0).collect(), entities, tf: tf.to_vec(), col: col.to_vec(), rows, recs, inv, grid, big, stamp: vec![0; m], slot: vec![0; m], sig })
    }
}

/// The grid of a set of colliders (`uid_pos` and `exts`: one entry for each, `sizes`: their largest half size): the entries, the cell size's inverse, the grid and
/// the numbers of the colliders that cover too many cells for it. The scenery of an archetype and the sleepers are indexed the same way.
fn make_grid(uid_pos: &[(u64, [f32; 3])], exts: &[[f32; 3]], mut sizes: Vec<f32>) -> (Vec<Entry>, f32, Grid, Vec<u32>) {
        // the cell size: eight times the typical (median) half size, between 1 and 32 metres
        let cell = if sizes.is_empty() {
            4.0
        } else {
            let mid = sizes.len() / 2;
            sizes.select_nth_unstable_by(mid, |x, y| x.total_cmp(y));
            (sizes[mid] * 8.0).clamp(1.0, 32.0)
        };
        let inv = 1.0 / cell;
        let m = uid_pos.len();
        let (mut los, mut his, mut spans) = (Vec::with_capacity(m), Vec::with_capacity(m), Vec::with_capacity(m));
        let mut big = Vec::new();
        let mut total = 0usize;
        let (mut cmin, mut cmax) = ([i32::MAX; 3], [i32::MIN; 3]);
        for k in 0..m {
            let (lo, hi) = cell_range(uid_pos[k].1, exts[k], inv);
            let s = span(lo, hi);
            los.push(lo);
            his.push(hi);
            if s > MAX_CELLS {
                spans.push(0u32);
                big.push(k as u32);
            } else {
                spans.push(s as u32);
                total += s as usize;
                for a in 0..3 {
                    cmin[a] = cmin[a].min(lo[a]);
                    cmax[a] = cmax[a].max(hi[a]);
                }
            }
        }
        let entry = |k: usize| Entry { uid: uid_pos[k].0, pos: uid_pos[k].1, ext: exts[k], lo: los[k], r: k as u32 };
        let recs: Vec<Entry> = (0..m).map(entry).collect();
        let (index, entries) = if total == 0 {
            (Index::Dense { min: [0; 3], dim: [0; 3], start: vec![0, 0] }, Vec::new())
        } else {
            let dim = [(cmax[0] as i64 - cmin[0] as i64 + 1), (cmax[1] as i64 - cmin[1] as i64 + 1), (cmax[2] as i64 - cmin[2] as i64 + 1)];
            let cells = dim[0] * dim[1] * dim[2];
            if cells <= DENSE_MAX && cells <= 32 * total as i64 + 65_536 {
                // dense: count the entries of every cell, then place them
                let mut start = vec![0u32; cells as usize + 1];
                let at = |c: [i32; 3]| ((c[0] as i64 - cmin[0] as i64) + dim[0] * ((c[1] as i64 - cmin[1] as i64) + dim[1] * (c[2] as i64 - cmin[2] as i64))) as usize;
                for k in 0..m {
                    if spans[k] == 0 {
                        continue;
                    }
                    for z in los[k][2]..=his[k][2] {
                        for y in los[k][1]..=his[k][1] {
                            for x in los[k][0]..=his[k][0] {
                                start[at([x, y, z]) + 1] += 1;
                            }
                        }
                    }
                }
                for i in 0..cells as usize {
                    start[i + 1] += start[i];
                }
                let mut fill = start.clone();
                let mut entries = vec![entry(0); total];
                for k in 0..m {
                    if spans[k] == 0 {
                        continue;
                    }
                    for z in los[k][2]..=his[k][2] {
                        for y in los[k][1]..=his[k][1] {
                            for x in los[k][0]..=his[k][0] {
                                let c = at([x, y, z]);
                                entries[fill[c] as usize] = entry(k);
                                fill[c] += 1;
                            }
                        }
                    }
                }
                (Index::Dense { min: cmin, dim, start }, entries)
            } else {
                let mut table = Table::default();
                table.rebuild(&los, &his, &spans);
                let entries: Vec<Entry> = table.items.iter().map(|&k| entry(k as usize)).collect();
                (Index::Hash(table), entries)
            }
        };
        (recs, inv, Grid { index, entries }, big)
}

/// The cells of a scenery grid and what is in them.
struct Grid {
    index: Index,
    entries: Vec<Entry>,
}

impl Grid {
    /// The entries of one cell.
    #[inline]
    fn cell(&self, c: [i32; 3]) -> &[Entry] {
        match &self.index {
            Index::Dense { min, dim, start } => {
                let (x, y, z) = (c[0] as i64 - min[0] as i64, c[1] as i64 - min[1] as i64, c[2] as i64 - min[2] as i64);
                if x < 0 || y < 0 || z < 0 || x >= dim[0] || y >= dim[1] || z >= dim[2] {
                    return &[];
                }
                let i = (x + dim[0] * (y + dim[1] * z)) as usize;
                &self.entries[start[i] as usize..start[i + 1] as usize]
            }
            Index::Hash(t) => {
                let (a, n) = t.range_of(c);
                &self.entries[a..a + n]
            }
        }
    }
}

/// The few numbers the pair test needs about a body.
#[derive(Clone, Copy)]
struct P {
    uid: u64,
    pos: [f32; 3],
    ext: [f32; 3],
    stat: bool,
    trig: bool,
}

impl P {
    fn of(b: &Body) -> P {
        P { uid: b.uid, pos: b.pos, ext: b.ext, stat: b.inv_mass == 0.0, trig: b.trigger }
    }
}

/// The sweep axis of the reference, worked out when a pair needs it (and only then): the axis along which the positions of all the bodies vary
/// most, from the same sums over all the bodies in uid order.
struct LazyAxis<'a> {
    axis: OnceLock<usize>,
    bodies: &'a [Body],
    scenery: &'a [(u64, [f32; 3])],
    /// The sleepers (they are bodies of the reference, and are among the positions it sums).
    sleepers: &'a [sleep::Sleeper],
}

impl LazyAxis<'_> {
    fn get(&self) -> usize {
        *self.axis.get_or_init(|| {
            #[cfg(test)]
            AXIS_TICKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if self.sleepers.is_empty() {
                return sweep_axis_all(self.bodies, self.scenery);
            }
            // (the scenery and the sleepers, both in uid order, as one list in uid order)
            let mut all: Vec<(u64, [f32; 3])> = Vec::with_capacity(self.scenery.len() + self.sleepers.len());
            let (mut i, mut j) = (0, 0);
            while i < self.scenery.len() || j < self.sleepers.len() {
                if j >= self.sleepers.len() || (i < self.scenery.len() && self.scenery[i].0 < self.sleepers[j].uid) {
                    all.push(self.scenery[i]);
                    i += 1;
                } else {
                    all.push((self.sleepers[j].uid, self.sleepers[j].pos()));
                    j += 1;
                }
            }
            sweep_axis_all(self.bodies, &all)
        })
    }
}

/// What one task of the broad phase found: pairs of bodies, and (body, set of scenery, collider of the set) that passed `accept`.
#[derive(Default)]
struct Scan {
    dd: Vec<(u32, u32)>,
    sc: Vec<(u32, u16, u32)>,
    /// (awake body, sleeper's number in the set of sleepers): an awake body whose box overlaps a sleeper's.
    sl: Vec<(u32, u32)>,
}

/// Runs `f(0..tasks)` on the pool, or one after the other when there is none.
fn par(pool: &Option<std::sync::Arc<Pool>>, tasks: usize, f: &(dyn Fn(usize) + Sync)) {
    match pool {
        Some(p) => p.run(tasks, f),
        None => (0..tasks).for_each(f),
    }
}

/// The reference's test of one pair, exactly. In the reference the pair is met by the sweep when the one that comes later in the order (smaller box
/// start along the sweep axis, then smaller uid) starts no further than the other one ends; then two scenery colliders are skipped, the collision
/// layers must agree, and the boxes must overlap on every axis. The sweep test can only reject a pair whose boxes overlap on that axis by less than
/// the rounding of the numbers (in real numbers it follows from the overlap test), so when no axis is that close the answer does not depend on the
/// sweep axis and it is never worked out; when one is, the reference's test is made as it is.
#[inline]
fn accept(axis: &LazyAxis, p: &P, q: &P, fp: Filter, fq: Filter) -> bool {
    let (a, b, fa, fb) = if p.uid < q.uid { (p, q, fp, fq) } else { (q, p, fq, fp) };
    let mut close = false;
    for k in 0..3 {
        let d = (a.pos[k] - b.pos[k]).abs();
        let s = a.ext[k] + b.ext[k];
        if d > s {
            return false;
        }
        // (a gap of ten thousandths of a millimetre per metre of size and distance is far more than the rounding of f32 numbers)
        if s - d <= 1.0e-6 * (a.pos[k].abs() + b.pos[k].abs() + s) {
            close = true;
        }
    }
    if close {
        let sweep = axis.get();
        let (mp, mq) = (p.pos[sweep] - p.ext[sweep], q.pos[sweep] - q.ext[sweep]);
        let p_first = mp.total_cmp(&mq).then(p.uid.cmp(&q.uid)).is_le();
        let (i, j) = if p_first { (p, q) } else { (q, p) };
        if j.pos[sweep] - j.ext[sweep] > i.pos[sweep] + i.ext[sweep] {
            return false;
        }
    }
    if a.stat && b.stat && !a.trig && !b.trig {
        return false;
    }
    fa.collides(fb)
}

struct Found {
    found: Vec<(usize, usize, [f32; 3], f32)>,
    rich: Vec<(usize, usize, Vec<rot::Pt>)>,
}

/// The bodies that were gathered and, after them, the scenery colliders that turned up in a pair this tick.
struct View<'a> {
    base: &'a [Body],
    extra: &'a [Body],
}

impl View<'_> {
    #[inline]
    fn at(&self, i: usize) -> &Body {
        if i < self.base.len() {
            &self.base[i]
        } else {
            &self.extra[i - self.base.len()]
        }
    }
}

/// The narrow phase for two bodies that passed `accept`: exactly the reference's call.
fn narrow(v: &View, i: usize, j: usize, statics: &Statics, out: &mut Found) {
    let (a, b) = if v.at(i).uid < v.at(j).uid { (i, j) } else { (j, i) };
    let (ba, bb) = (v.at(a), v.at(b));
    if ba.rich || bb.rich {
        let pts = rot::collide(ba, bb, statics);
        if !pts.is_empty() {
            out.rich.push((a, b, pts));
        }
    } else if let Some((n, depth)) = collide(ba, bb, &[]) {
        out.found.push((a, b, n, depth));
    }
}

/// The positions of the bodies and of the scenery together, in uid order (both lists are in uid order).
fn for_each_pos(bodies: &[Body], scenery: &[(u64, [f32; 3])], mut f: impl FnMut([f32; 3])) {
    let (mut i, mut j) = (0, 0);
    while i < bodies.len() || j < scenery.len() {
        if j >= scenery.len() || (i < bodies.len() && bodies[i].uid < scenery[j].0) {
            f(bodies[i].pos);
            i += 1;
        } else {
            f(scenery[j].1);
            j += 1;
        }
    }
}

/// `sweep_axis` of the reference over all the bodies (the bodies of the fast path and the scenery), with the same sums in the same order.
fn sweep_axis_all(bodies: &[Body], scenery: &[(u64, [f32; 3])]) -> usize {
    let n = (bodies.len() + scenery.len()) as f64;
    let mut sum = [0.0f64; 3];
    for_each_pos(bodies, scenery, |p| {
        for a in 0..3 {
            sum[a] += p[a] as f64;
        }
    });
    let mean = [sum[0] / n, sum[1] / n, sum[2] / n];
    let mut var = [0.0f64; 3];
    for_each_pos(bodies, scenery, |p| {
        for a in 0..3 {
            var[a] += (p[a] as f64 - mean[a]).powi(2);
        }
    });
    let mut best = (-1.0f64, 0usize);
    for axis in 0..3 {
        if var[axis] > best.0 {
            best = (var[axis], axis);
        }
    }
    best.1
}

/// The signature of the scenery colliders of the grids that touch `me` (`sleep::near`: their boxes come within `sleep::TOUCH_MARGIN` of its box), the same sum
/// that `sleep::touch_in` makes over the scenery bodies of the reference.
fn touch_sets(sets: &[StaticSet], pos: [f32; 3], ext: [f32; 3]) -> u64 {
    // (the cells are looked at with twice the margin, so that no rounding at the edge of a cell can hide a collider that `near` accepts)
    let wide = [ext[0] + 2.0 * sleep::TOUCH_MARGIN, ext[1] + 2.0 * sleep::TOUCH_MARGIN, ext[2] + 2.0 * sleep::TOUCH_MARGIN];
    let mut sum = 0u64;
    for set in sets {
        let (lo, hi) = cell_range(pos, wide, set.inv);
        let mut look = |e: &Entry| {
            if (0..3).all(|k| (pos[k] - e.pos[k]).abs() <= ext[k] + e.ext[k] + sleep::TOUCH_MARGIN) {
                sum = sum.wrapping_add(sleep::mix(e.uid, e.pos, e.ext));
            }
        };
        if span(lo, hi) > MAX_CELLS {
            set.recs.iter().for_each(&mut look);
            continue;
        }
        for z in lo[2]..=hi[2] {
            for y in lo[1]..=hi[1] {
                for x in lo[0]..=hi[0] {
                    let cell = [x, y, z];
                    for e in set.grid.cell(cell) {
                        if [lo[0].max(e.lo[0]), lo[1].max(e.lo[1]), lo[2].max(e.lo[2])] != cell {
                            continue;
                        }
                        look(e);
                    }
                }
            }
        }
        for &k in set.big.iter() {
            look(&set.recs[k as usize]);
        }
    }
    sum
}

fn has(world: &World, id: ComponentId) -> bool {
    !world.matching_archetypes(&[id], &[]).is_empty()
}

/// The slice of a column of `a` (`n` rows).
///
/// # Safety
/// `T` must be the type of the component `id`.
unsafe fn column<'a, T>(a: &'a Archetype, id: ComponentId) -> Option<&'a [T]> {
    let base = a.column_base(id)?;
    // SAFETY: the column holds `a.len()` rows of `T`, contiguous and aligned (the caller says that `T` is the component's type).
    Some(unsafe { std::slice::from_raw_parts(base.cast::<T>().cast_const(), a.len()) })
}

pub(super) fn step(world: &mut World, gravity: [f32; 3], dt: f32, terrains: &Terrains, meshes: &MeshStore, cache: &PhysicsCache, threads: usize, memory: &mut ContactMemory, sleep: &mut SleepState, cfg: Option<sleep::Config>) -> Option<Vec<Contact>> {
    if !terrains.is_empty() || !meshes.is_empty() {
        return None;
    }
    let (tid, cid, rid, vid) = (world.component_id("Transform")?, world.component_id("Collider")?, world.component_id("RigidBody")?, world.component_id("Velocity")?);
    if has(world, world.component_id("Joint")?) || has(world, world.component_id("AngularVelocity")?) || has(world, world.component_id("Terrain")?) || has(world, world.component_id("ConvexCollider")?) {
        return None;
    }
    let layers = Layers::gather(world);
    let mut guard = cache.inner.lock().unwrap_or_else(|e| e.into_inner());
    let inner: &mut Inner = &mut guard;
    inner.stage_t = inner.profile.then(std::time::Instant::now);
    // `lap!(n)`: the time since the last lap is part `n` of the step
    macro_rules! lap {
        ($n:expr) => {
            if let Some(t0) = inner.stage_t {
                let now = std::time::Instant::now();
                inner.stage_ns[$n] += now.duration_since(t0).as_nanos() as u64;
                inner.stage_t = Some(now);
            }
        };
    }

    // the contact impulses of the bodies that woke in the last try are given back first
    sleep.take_pending(memory);

    // ---- gather: the bodies that can move, and the scenery
    let mut bodies: Vec<Body> = Vec::with_capacity(inner.last_n0 + inner.last_n0 / 8 + 16);
    // the sleepers that were found and still lie as they did, and the islands to wake because one of theirs does not
    let mut seen = vec![false; sleep.sleeper_count()];
    let mut bad: Vec<u64> = Vec::new();
    let sleepers = sleep.has_sleepers();
    let mut used = vec![false; inner.sets.len()];
    let mut changed = false;
    let mut fresh: Vec<StaticSet> = Vec::new();
    let mut ok = true;
    for (ai, a) in world.archetypes().iter().enumerate() {
        if a.is_empty() {
            continue;
        }
        // SAFETY: Transform, Collider, RigidBody and Velocity are the components of those names.
        let (Some(tf), Some(col)) = (unsafe { column::<Transform>(a, tid) }, unsafe { column::<Collider>(a, cid) }) else { continue };
        let rb = unsafe { column::<RigidBody>(a, rid) };
        let vel = unsafe { column::<Velocity>(a, vid) };
        let ents = a.entities();
        match rb {
            Some(rb) => {
                for row in 0..a.len() {
                    let uid = world.uid_of(ents[row]).unwrap_or(u64::MAX);
                    // a sleeper that still lies as it did is not a body of this step: the grid of the sleepers has it (only a dynamic body can be one)
                    if sleepers && rb[row].mass > 0.0 {
                        if let Some(v) = vel {
                            if sleep.gather(uid, &tf[row], &col[row], v[row].lin, [0.0; 3], &mut seen, &mut bad) {
                                continue;
                            }
                        }
                    }
                    let b = build_body(uid, ents[row], &tf[row], &col[row], Some(&rb[row]), vel.map(|v| &v[row]), layers.of(uid));
                    ok &= finite3(b.pos) && finite3(b.ext) && finite3(b.vel);
                    bodies.push(b);
                }
            }
            None => {
                // scenery, and triggers (which are bodies)
                let existing = inner.sets.iter().position(|s| s.arch == ai);
                let valid = existing.is_some_and(|k| inner.sets[k].matches(ai, a, tf, col));
                if valid {
                    used[existing.unwrap()] = true;
                } else {
                    let Some(set) = StaticSet::build(world, ai, a, tf, col) else { return None };
                    changed = true;
                    if let Some(k) = existing {
                        inner.sets[k] = set;
                        used[k] = true;
                    } else {
                        fresh.push(set);
                    }
                }
                for row in 0..a.len() {
                    if col[row].trigger {
                        let uid = world.uid_of(ents[row]).unwrap_or(u64::MAX);
                        let b = build_body(uid, ents[row], &tf[row], &col[row], None, None, layers.of(uid));
                        ok &= finite3(b.pos) && finite3(b.ext);
                        bodies.push(b);
                    }
                }
            }
        }
    }
    if !ok {
        return None;
    }
    // sets of archetypes that are gone, and the new ones
    if used.iter().any(|u| !u) {
        let mut k = 0;
        inner.sets.retain(|_| {
            k += 1;
            used[k - 1]
        });
        changed = true;
    }
    if !fresh.is_empty() {
        inner.sets.append(&mut fresh);
    }
    if changed {
        inner.by_uid.clear();
        for s in &inner.sets {
            inner.by_uid.extend(s.recs.iter().map(|e| (e.uid, e.pos)));
        }
        inner.by_uid.sort_unstable_by_key(|e| e.0);
    }
    let scenery_count: usize = inner.sets.iter().map(|s| s.rows.len()).sum();
    if bodies.is_empty() && scenery_count == 0 && !seen.iter().any(|&s| s) {
        memory.clear();
        sleep.clear();
        return Some(Vec::new());
    }
    bodies.sort_by_key(|b| b.uid);
    let n0 = bodies.len();
    inner.last_n0 = n0;

    lap!(0);
    // ---- sleepers: one that was moved, spun or pushed, or whose entity is gone, wakes its island; and when the scenery changed since the last step, so
    // do the islands whose sleepers see other scenery around them than when they lay down
    let mut scenery = 0u64;
    if cfg.is_some() {
        if sleepers {
            sleep.finish_gather(&seen, &mut bad);
            if sleep.restart {
                return None;
            }
        }
        scenery = inner.sets.iter().fold(sleep::scenery_sig(&bodies), |s, set| s.wrapping_add(set.sig));
        if sleep.has_sleepers() && scenery != sleep.statics() {
            let sets = &inner.sets;
            let idx = sleep::scenery_idx(&bodies);
            sleep.check_scenery(&|pos, ext| sleep::touch_in(&bodies, &idx, pos, ext).wrapping_add(touch_sets(sets, pos, ext)));
            if sleep.restart {
                return None;
            }
        }
    }
    // the grid of the sleepers, kept while the list of sleepers stays the same
    if sleep.has_sleepers() {
        let g = sleep.generation();
        if inner.sleep_set.as_ref().map(|(k, _)| *k) != Some(g) {
            inner.sleep_set = Some((g, StaticSet::of_sleepers(sleep.records())));
        }
    } else {
        inner.sleep_set = None;
    }
    let statics = Statics { ground: &[], meshes: &[], mesh_filters: &[], hulls: &[], dt };

    lap!(1);
    // ---- forces
    let v0: Vec<[f32; 3]> = bodies.iter().map(|b| b.vel).collect();
    for b in bodies.iter_mut().filter(|b| b.inv_mass > 0.0) {
        for axis in 0..3 {
            b.vel[axis] += gravity[axis] * b.gravity_scale * dt;
        }
        if b.drag > 0.0 {
            let k = 1.0 / (1.0 + b.drag * dt);
            for axis in 0..3 {
                b.vel[axis] *= k;
            }
        }
    }

    // ---- fast bodies against the colliders that never move: one that crossed a thin wall or floor between two ticks is put back against it
    let fixes = rot::tunnel_fixes(
        &bodies,
        &v0,
        &mut |b, lo, hi, out| {
            for o in bodies.iter() {
                if o.inv_mass == 0.0 && !o.trigger && b.filter.collides(o.filter) && (0..3).all(|a| o.max_on(a) >= lo[a] && o.min_on(a) <= hi[a]) {
                    out.push(o.clone());
                }
            }
            let centre = [0.5 * (lo[0] + hi[0]), 0.5 * (lo[1] + hi[1]), 0.5 * (lo[2] + hi[2])];
            let half = [0.5 * (hi[0] - lo[0]), 0.5 * (hi[1] - lo[1]), 0.5 * (hi[2] - lo[2])];
            let mut found: Vec<u32> = Vec::new();
            // (a sleeper is a collider that never moves: a fast body that would cross one is stopped against it, which wakes it, as in the reference)
            for set in inner.sets.iter().chain(inner.sleep_set.iter().map(|(_, s)| s)) {
                found.clear();
                let (clo, chi) = cell_range(centre, half, set.inv);
                if span(clo, chi) > MAX_CELLS {
                    found.extend(0..set.recs.len() as u32);
                } else {
                    for z in clo[2]..=chi[2] {
                        for y in clo[1]..=chi[1] {
                            for x in clo[0]..=chi[0] {
                                found.extend(set.grid.cell([x, y, z]).iter().map(|e| e.r));
                            }
                        }
                    }
                    found.extend(set.big.iter().copied());
                }
                found.sort_unstable();
                found.dedup();
                for &r in &found {
                    let e = &set.recs[r as usize];
                    let fq = layers.of(e.uid);
                    if b.filter.collides(fq) && (0..3).all(|a| e.pos[a] + e.ext[a] >= lo[a] && e.pos[a] - e.ext[a] <= hi[a]) {
                        let row = set.rows[r as usize] as usize;
                        out.push(build_body(e.uid, set.entities[row], &set.tf[row], &set.col[row], None, None, fq));
                    }
                }
            }
        },
        &statics,
    );
    #[cfg(test)]
    CCD_FIXES.fetch_add(fixes.len() as u64, std::sync::atomic::Ordering::Relaxed);
    for (i, p) in fixes {
        bodies[i].pos = p;
    }

    lap!(2);
    // ---- broad phase
    let pool: Option<std::sync::Arc<Pool>> = if threads > 1 && n0 >= PAR_MIN {
        if inner.pool.as_ref().map(|p| p.threads()) != Some(threads) {
            inner.pool = Some(std::sync::Arc::new(Pool::new(threads - 1)));
        }
        inner.pool.clone()
    } else {
        None
    };
    #[cfg(test)]
    if pool.is_some() {
        PAR_TICKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    let axis = LazyAxis { axis: OnceLock::new(), bodies: &bodies, scenery: &inner.by_uid, sleepers: sleep.records() };
    let mut out = Found { found: Vec::new(), rich: Vec::new() };
    let ps: Vec<P> = bodies.iter().map(P::of).collect();
    // the grid of the bodies: cell size four times their mean half size
    let mean_ext = if n0 == 0 { 1.0 } else { ps.iter().map(|p| p.ext[0].max(p.ext[1]).max(p.ext[2])).sum::<f32>() / n0 as f32 };
    let dyn_inv = 1.0 / (mean_ext * 4.0).clamp(0.5, 64.0);
    let mut dgrid = std::mem::take(&mut inner.dgrid);
    let (mut dlo, mut dhi, mut dspan) = (std::mem::take(&mut inner.dlo), std::mem::take(&mut inner.dhi), std::mem::take(&mut inner.dspan));
    dlo.clear();
    dhi.clear();
    dspan.clear();
    let mut dbig: Vec<usize> = Vec::new();
    for (k, p) in ps.iter().enumerate() {
        let (lo, hi) = cell_range(p.pos, p.ext, dyn_inv);
        let s = span(lo, hi);
        dlo.push(lo);
        dhi.push(hi);
        if s > MAX_CELLS {
            dspan.push(0u32);
            dbig.push(k);
        } else {
            dspan.push(s as u32);
        }
    }
    dgrid.rebuild(&dlo, &dhi, &dspan);
    lap!(3);
    let view0 = View { base: &bodies, extra: &[] };
    let no_layers = layers.is_empty();
    // Every body looks for its neighbours: bodies after it in the grid of the bodies (a pair is looked at in the cell that holds the later corner of both
    // boxes) and, when it can move (or is a trigger), the scenery. The looking is done in tasks of a few dozen bodies, on the pool; what a task finds is
    // kept in its own list, and the lists are used in the order of the tasks, so the threads cannot change the answer.
    let tasks = n0.div_ceil(CHUNK);
    let results: Vec<Mutex<Scan>> = (0..tasks).map(|_| Mutex::new(Scan::default())).collect();
    {
        let sets = &inner.sets;
        // (the sets of scenery, and then the set of the sleepers: number `sets.len()`)
        let sleep_set = inner.sleep_set.as_ref().map(|(_, s)| s);
        let sleep_at = sets.len();
        let (dgrid, dlo, dhi, dspan) = (&dgrid, &dlo, &dhi, &dspan);
        par(&pool, tasks, &|t| {
            let mut found = results[t].lock().unwrap_or_else(|e| e.into_inner());
            for i in t * CHUNK..((t + 1) * CHUNK).min(n0) {
                if dspan[i] != 0 {
                    for z in dlo[i][2]..=dhi[i][2] {
                        for y in dlo[i][1]..=dhi[i][1] {
                            for x in dlo[i][0]..=dhi[i][0] {
                                let cell = [x, y, z];
                                for &j in dgrid.items_in(cell) {
                                    let j = j as usize;
                                    if j <= i || [dlo[i][0].max(dlo[j][0]), dlo[i][1].max(dlo[j][1]), dlo[i][2].max(dlo[j][2])] != cell {
                                        continue;
                                    }
                                    if accept(&axis, &ps[i], &ps[j], bodies[i].filter, bodies[j].filter) {
                                        found.dd.push((i as u32, j as u32));
                                    }
                                }
                            }
                        }
                    }
                }
                if ps[i].stat && !ps[i].trig {
                    continue; // (two scenery colliders never need resolving)
                }
                let (p, fi) = (&ps[i], bodies[i].filter);
                for (si, set) in sets.iter().chain(sleep_set).enumerate() {
                    let (lo, hi) = cell_range(p.pos, p.ext, set.inv);
                    let mut look = |e: &Entry| {
                        let q = P { uid: e.uid, pos: e.pos, ext: e.ext, stat: true, trig: false };
                        let fq = if no_layers { Filter::DEFAULT } else { layers.of(e.uid) };
                        if accept(&axis, p, &q, fi, fq) {
                            if si == sleep_at && !p.trig {
                                // (an awake body that touches a sleeper wakes it; a trigger only sees it, like any scenery)
                                found.sl.push((i as u32, e.r));
                            } else {
                                found.sc.push((i as u32, si as u16, e.r));
                            }
                        }
                    };
                    if span(lo, hi) > MAX_CELLS {
                        set.recs.iter().for_each(&mut look);
                        continue;
                    }
                    for z in lo[2]..=hi[2] {
                        for y in lo[1]..=hi[1] {
                            for x in lo[0]..=hi[0] {
                                let cell = [x, y, z];
                                for e in set.grid.cell(cell) {
                                    if [lo[0].max(e.lo[0]), lo[1].max(e.lo[1]), lo[2].max(e.lo[2])] != cell {
                                        continue;
                                    }
                                    look(e);
                                }
                            }
                        }
                    }
                    for &k in set.big.iter() {
                        look(&set.recs[k as usize]);
                    }
                }
            }
        });
    }
    // a sleeper that an awake body's box overlaps wakes with its island, and the step is made again
    let mut woken: Vec<u64> = Vec::new();
    if let Some((_, set)) = &inner.sleep_set {
        for r in &results {
            for &(_, rec) in &r.lock().unwrap_or_else(|e| e.into_inner()).sl {
                woken.extend(sleep.island_of(set.recs[rec as usize].uid));
            }
        }
    }
    if !woken.is_empty() {
        drop(axis);
        inner.dgrid = dgrid;
        sleep.wake_islands(&mut woken);
        return None;
    }
    lap!(4);
    // the pairs the tasks found: the narrow phase, one after the other, in the order of the tasks
    for r in &results {
        for &(i, j) in &r.lock().unwrap_or_else(|e| e.into_inner()).dd {
            narrow(&view0, i as usize, j as usize, &statics, &mut out);
        }
    }
    // the big ones against all the others (and each other pair once)
    for (bi, &l) in dbig.iter().enumerate() {
        for j in 0..n0 {
            if j == l || (dspan[j] == 0 && dbig.iter().position(|&x| x == j).is_some_and(|bj| bj < bi)) {
                continue;
            }
            if accept(&axis, &ps[l], &ps[j], bodies[l].filter, bodies[j].filter) {
                narrow(&view0, l, j, &statics, &mut out);
            }
        }
    }
    inner.dgrid = dgrid;
    // a scenery collider that is in a pair becomes a body (after the others) for the rest of the step
    inner.cur = inner.cur.wrapping_add(1);
    if inner.cur == 0 {
        for s in inner.sets.iter_mut().chain(inner.sleep_set.iter_mut().map(|(_, s)| s)) {
            s.stamp.iter_mut().for_each(|x| *x = 0);
        }
        inner.cur = 1;
    }
    let cur = inner.cur;
    let mut extra: Vec<Body> = Vec::new();
    for r in &results {
        for &(i, si, rec) in &r.lock().unwrap_or_else(|e| e.into_inner()).sc {
            let set = if (si as usize) < inner.sets.len() { &mut inner.sets[si as usize] } else { &mut inner.sleep_set.as_mut().expect("a pair with the sleepers").1 };
            let rr = rec as usize;
            let bj = if set.stamp[rr] == cur {
                set.slot[rr] as usize
            } else {
                let (row, e) = (set.rows[rr] as usize, set.recs[rr]);
                let fq = if no_layers { Filter::DEFAULT } else { layers.of(e.uid) };
                extra.push(build_body(e.uid, set.entities[row], &set.tf[row], &set.col[row], None, None, fq));
                set.stamp[rr] = cur;
                set.slot[rr] = (n0 + extra.len() - 1) as u32;
                n0 + extra.len() - 1
            };
            narrow(&View { base: &bodies, extra: &extra }, i as usize, bj, &statics, &mut out);
        }
    }
    drop(results);
    drop(view0);
    drop(axis);
    inner.dlo = dlo;
    inner.dhi = dhi;
    inner.dspan = dspan;
    bodies.append(&mut extra);
    let Found { mut found, rich: mut rich_found } = out;
    found.sort_by_key(|f| (bodies[f.0].uid, bodies[f.1].uid));
    rich_found.sort_by_key(|f| (bodies[f.0].uid, bodies[f.1].uid));

    lap!(5);
    // ---- velocity solve: sequential impulses
    let solid: Vec<usize> = (0..found.len()).filter(|&k| !bodies[found[k].0].trigger && !bodies[found[k].1].trigger).collect();
    let rich_solid: Vec<usize> = (0..rich_found.len()).filter(|&k| !bodies[rich_found[k].0].trigger && !bodies[rich_found[k].1].trigger).collect();
    let mut rows: Vec<rot::Row> = Vec::new();
    for &k in &rich_solid {
        let (a, b, ref pts) = rich_found[k];
        for pt in pts {
            rows.push(rot::prepare_row(&bodies, a, b, pt, dt));
        }
    }
    rot::apply_memory(&mut bodies, &mut rows, &std::mem::take(memory));
    rot::warm_start(&mut bodies, &mut rows, gravity, dt);
    let rounds = if rows.is_empty() { ITERATIONS } else { rot::rounds_for(rows.len(), 0) };
    let mut settled_at = rot::SETTLED;
    for round in 0..rounds {
        let mut delta = 0.0f32;
        if round < ITERATIONS {
            for &k in &solid {
                let (ia, ib, n, _) = found[k];
                let (inv_a, inv_b) = (bodies[ia].inv_mass, bodies[ib].inv_mass);
                let inv_sum = inv_a + inv_b;
                if inv_sum == 0.0 {
                    continue;
                }
                let rv = sub(bodies[ib].vel, bodies[ia].vel);
                let vn = dot(rv, n);
                if vn >= 0.0 {
                    continue; // separating
                }
                delta = delta.max(-vn);
                let e = if -vn < BOUNCE_THRESHOLD { 0.0 } else { bodies[ia].restitution.max(bodies[ib].restitution) };
                let j = -(1.0 + e) * vn / inv_sum;
                bodies[ia].vel = axpy(bodies[ia].vel, -j * inv_a, n);
                bodies[ib].vel = axpy(bodies[ib].vel, j * inv_b, n);
                let rv = sub(bodies[ib].vel, bodies[ia].vel);
                let t = axpy(rv, -dot(rv, n), n);
                let tl = dot(t, t).sqrt();
                if tl > 1e-6 {
                    let tn = [t[0] / tl, t[1] / tl, t[2] / tl];
                    let mu = (bodies[ia].friction * bodies[ib].friction).sqrt();
                    let jt = (-tl / inv_sum).clamp(-mu * j, mu * j);
                    bodies[ia].vel = axpy(bodies[ia].vel, -jt * inv_a, tn);
                    bodies[ib].vel = axpy(bodies[ib].vel, jt * inv_b, tn);
                }
            }
        }
        for r in rows.iter_mut() {
            delta = delta.max(rot::solve_row(&mut bodies, r));
        }
        // (a round that changes the speeds by less than `SETTLED`, or by a thousandth of what the first round did, is followed by rounds that change nothing that
        // matters: the rest are not made)
        if round == 0 {
            settled_at = rot::SETTLED.max(delta * rot::SETTLED_REL);
            if delta < rot::SETTLED {
                break;
            }
        } else if delta < settled_at {
            break;
        }
    }

    *memory = rot::collect_memory(&bodies, &rows);
    lap!(6);
    // ---- position correction (once, from the depths found before the solve)
    for &k in &solid {
        let (ia, ib, n, depth) = found[k];
        let inv_sum = bodies[ia].inv_mass + bodies[ib].inv_mass;
        if inv_sum == 0.0 {
            continue;
        }
        let corr = (depth - SLOP).max(0.0) / inv_sum * CORRECTION;
        bodies[ia].pos = axpy(bodies[ia].pos, -corr * bodies[ia].inv_mass, n);
        bodies[ib].pos = axpy(bodies[ib].pos, corr * bodies[ib].inv_mass, n);
    }
    for &k in &rich_solid {
        let (ia, ib, ref pts) = rich_found[k];
        let inv_sum = bodies[ia].inv_mass + bodies[ib].inv_mass;
        if inv_sum == 0.0 {
            continue;
        }
        let (n, depth) = rot::deepest(pts);
        let corr = (depth - SLOP).max(0.0) / inv_sum * CORRECTION;
        bodies[ia].pos = axpy(bodies[ia].pos, -corr * bodies[ia].inv_mass, n);
        bodies[ib].pos = axpy(bodies[ib].pos, corr * bodies[ib].inv_mass, n);
    }

    lap!(7);
    // ---- sleeping: islands that have lain still long enough go to sleep (their velocities are zeroed here, and zero is what is written back)
    if let Some(cfg) = &cfg {
        let pairs: Vec<(usize, usize)> = solid.iter().map(|&k| (found[k].0, found[k].1)).chain(rich_solid.iter().map(|&k| (rich_found[k].0, rich_found[k].1))).filter(|&(a, b)| a < n0 && b < n0).collect();
        let sets = &inner.sets;
        // (only the bodies that were gathered take part: the scenery that joined this step as bodies is the grids' own, which `touch_sets` looks at)
        sleep::update(sleep, cfg, dt, &mut bodies[..n0], &pairs, &[], &|bodies: &[Body], extra: &[usize], pos: [f32; 3], ext: [f32; 3]| sleep::touch_in(bodies, extra, pos, ext).wrapping_add(touch_sets(sets, pos, ext)), scenery, memory);
    }

    // ---- write back (only dynamic bodies change)
    {
        let w: &World = world;
        let nb = bodies.len();
        let bodies = &bodies;
        par(&pool, nb.div_ceil(CHUNK * 4), &|t| {
            for b in bodies[t * CHUNK * 4..((t + 1) * CHUNK * 4).min(nb)].iter().filter(|b| b.inv_mass > 0.0) {
                // SAFETY: `tid` and `vid` are the ids of Transform and Velocity; `&mut World` gives exclusive access, and every body is another entity, so no two
                // tasks write the same row.
                if let Some(p) = w.component_ptr(b.entity, tid) {
                    unsafe { (*p.cast::<Transform>()).pos = b.pos };
                }
                if let Some(p) = w.component_ptr(b.entity, vid) {
                    unsafe { (*p.cast::<Velocity>()).lin = b.vel };
                }
            }
        });
    }
    sleep.finish_new(world);
    lap!(8);
    let mut contacts: Vec<Contact> = found.iter().map(|&(a, b, n, depth)| Contact { a: bodies[a].uid, b: bodies[b].uid, normal: n, depth, trigger: bodies[a].trigger || bodies[b].trigger }).collect();
    if !rich_found.is_empty() {
        for (a, b, pts) in &rich_found {
            let (normal, depth) = rot::deepest(pts);
            contacts.push(Contact { a: bodies[*a].uid, b: bodies[*b].uid, normal, depth, trigger: bodies[*a].trigger || bodies[*b].trigger });
        }
        contacts.sort_by_key(|c| (c.a, c.b));
    }
    #[cfg(test)]
    {
        FAST_TICKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        FAST_CONTACTS.fetch_add(contacts.len() as u64, std::sync::atomic::Ordering::Relaxed);
    }
    Some(contacts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sweep axis of the fast path is the one of the reference for any split of the bodies into "gathered" and "scenery".
    #[test]
    fn the_sweep_axis_is_the_one_of_the_reference() {
        let mut state = 0x1234_5678_9ABC_DEF0u64;
        let mut rnd = move || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((state >> 33) & 0xFF_FFFF) as f32 / (1u32 << 24) as f32
        };
        let mk = |uid: u64, pos: [f32; 3]| Body {
            uid,
            entity: Entity { index: uid as u32, generation: 0 },
            pos,
            vel: [0.0; 3],
            shape: Shape::Sphere(0.5),
            ext: [0.5; 3],
            inv_mass: 1.0,
            restitution: 0.3,
            friction: 0.5,
            gravity_scale: 1.0,
            drag: 0.0,
            trigger: false,
            filter: Filter::DEFAULT,
            spins: false,
            rich: false,
            asleep: false,
            rot: None,
        };
        for case in 0..3000 {
            let n = 1 + (rnd() * 300.0) as usize;
            let kind = case % 4;
            let scale = [1.0 + rnd() * 50.0, if kind == 1 { 0.0 } else { 1.0 + rnd() * 50.0 }, if kind == 2 { 0.0 } else { 1.0 + rnd() * 50.0 }];
            let side = 1 + (rnd() * 20.0) as usize;
            let all: Vec<Body> = (0..n)
                .map(|k| {
                    let pos = match kind {
                        // a regular grid: the variances of x and z are equal, down to the rounding
                        3 => [(k % side) as f32 * 40.0, 0.5, (k / side) as f32 * 40.0],
                        _ => [(rnd() - 0.5) * scale[0], (rnd() - 0.5) * scale[1], (rnd() - 0.5) * scale[2]],
                    };
                    mk(k as u64 + 1, pos)
                })
                .collect();
            let every = 1 + (rnd() * 14.0) as usize;
            let (mut mine, mut scenery) = (Vec::new(), Vec::new());
            for (k, b) in all.iter().enumerate() {
                if k % every == 0 {
                    mine.push(b.clone());
                } else {
                    scenery.push((b.uid, b.pos));
                }
            }
            assert_eq!(sweep_axis_all(&mine, &scenery), super::super::sweep_axis(&all), "case {case}: {n} bodies, kind {kind}");
        }
    }
}
