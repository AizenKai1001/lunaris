//! Physics-lite: deterministic rigid bodies for spheres, boxes and capsules. It gives the world consequences (things fall,
//! collide, bounce and rest) with no dependencies. A full engine (Jolt) can replace it behind the same components later.
//!
//! * A **static collider** has a `Transform` and a `Collider` and no `RigidBody` (or `mass <= 0`, or no `Velocity`).
//! * A **dynamic body** has `Transform`, `Collider`, `RigidBody` (mass > 0) and `Velocity`.
//! * A dynamic body that also has an `AngularVelocity` **turns**: contacts push at a point, so spheres roll and boxes tip over. Boxes
//!   with a rotated `Transform` and capsules are oriented shapes. All of that lives in `physics_rot.rs`; this file keeps the
//!   plain spheres and axis-aligned boxes that do not turn exactly as they always were (same code, same bits).
//!
//! Contact materials combine like Box2D: restitution is the larger of the two, friction the geometric mean.
//!
//! Each tick: gravity and drag change velocities, bodies that overlap (found with a sort-and-sweep along the axis where the bodies are most spread out) are separated and
//! their velocities adjusted by sequential impulses with restitution and Coulomb friction. All pairs are processed in uid
//! order, so the outcome does not depend on how entities are stored or on the thread count.
use crate::components::{AngularVelocity, Collider, ConvexCollider, Joint, RigidBody, Terrain, Transform, Velocity};
use crate::contact_memory::ContactMemory;
use crate::hull::Hull;
use crate::layers::{Filter, Layers};
use crate::meshcol::{MeshStore, PlacedMesh};
use crate::physics_rot as rot;
use crate::terrain::{Placed, Terrains};
use lunaris_ecs::{Entity, World};
use std::sync::Arc;

mod fast;
pub use fast::{PhysicsCache, STAGES};
mod sleep;
pub use sleep::SleepState;
#[cfg(test)]
pub(crate) use fast::{AXIS_TICKS, CCD_FIXES, FAST_CONTACTS, FAST_TICKS, PAR_TICKS};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use sleep::{SLEPT, WOKE};

#[derive(Clone, Debug, PartialEq)]
pub struct Contact {
    pub a: u64,
    pub b: u64,
    /// Unit normal pointing from `a` to `b`.
    pub normal: [f32; 3],
    pub depth: f32,
    /// True when either collider is a trigger (overlap reported, no collision response).
    pub trigger: bool,
}

const ITERATIONS: usize = 6;
pub(crate) const SLOP: f32 = 0.002;
pub(crate) const CORRECTION: f32 = 0.8;
/// Approach speeds below this do not bounce (stops resting bodies from jittering).
pub(crate) const BOUNCE_THRESHOLD: f32 = 0.6;

#[derive(Clone, Copy)]
pub(crate) enum Shape {
    Sphere(f32),
    Box([f32; 3]), // half extents
    /// A heightmap ground: its index in the list of placed terrains, and the half extents of its bounds.
    Terrain(usize, [f32; 3]),
    /// An oriented box (half extents in its own axes): a box that is rotated or that turns. Handled by `physics_rot`.
    Obb([f32; 3]),
    /// A capsule along its own y axis: the radius, and half the length of the segment between the centres of the two ends.
    Capsule(f32, f32),
    /// A static triangle mesh (a `MeshCollider`): its index in the list of placed meshes. Its `pos` and `ext` are the middle and the
    /// half sizes of the box around it, which is all the broad phase needs. Contacts with it are made by `physics_rot`, for every shape.
    Mesh(usize),
    /// A convex hull (a `ConvexCollider`): its index in the list of hulls in use. The body's `pos` is the hull's centre of mass and its
    /// orientation (`rot`) the one of the hull's principal axes. Contacts with it are made by `physics_rot`, so it is always `rich`.
    Hull(usize),
}

/// The static things that bodies touch besides other bodies, the hulls that bodies are, and the length of the step (a body's velocity times it
/// is how far the body moved last tick, which tells a mesh which side of a thin surface the body came from).
pub(crate) struct Statics<'a> {
    pub ground: &'a [Placed<'a>],
    pub meshes: &'a [Arc<PlacedMesh>],
    /// The collision layer of each of `meshes` (the same order): a fast body is only caught by the levels it collides with.
    pub mesh_filters: &'a [Filter],
    pub hulls: &'a [Arc<Hull>],
    pub dt: f32,
}

/// One joint, ready to solve: the body it pulls (`a`), the other end (a body, or a fixed point), and what kind of link it is.
struct Link {
    a: usize,
    b: Option<usize>,
    fixed: [f32; 3],
    kind: u32,
    length: f32,
    stiffness: f32,
    damping: f32,
    /// The length was not given: it is written back (the distance the ends had).
    measured: bool,
}

impl Link {
    /// From a to the other end: the unit direction, and the distance.
    fn span(&self, bodies: &[Body]) -> Option<([f32; 3], f32)> {
        let end = self.b.map_or(self.fixed, |b| bodies[b].pos);
        let d = sub(end, bodies[self.a].pos);
        let dist = dot(d, d).sqrt();
        (dist > 1.0e-6).then(|| ([d[0] / dist, d[1] / dist, d[2] / dist], dist))
    }

    fn inv_masses(&self, bodies: &[Body]) -> (f32, f32) {
        (bodies[self.a].inv_mass, self.b.map_or(0.0, |b| bodies[b].inv_mass))
    }

    /// Speed at which the far end moves away from `a`, along `n`.
    fn closing(&self, bodies: &[Body], n: [f32; 3]) -> f32 {
        let vb = self.b.map_or([0.0; 3], |b| bodies[b].vel);
        dot(sub(vb, bodies[self.a].vel), n)
    }

    fn push(&self, bodies: &mut [Body], n: [f32; 3], j: f32) {
        let (ia, ib) = self.inv_masses(bodies);
        bodies[self.a].vel = axpy(bodies[self.a].vel, -j * ia, n);
        if let Some(b) = self.b {
            bodies[b].vel = axpy(bodies[b].vel, j * ib, n);
        }
    }
}

#[derive(Clone)]
pub(crate) struct Body {
    pub(crate) uid: u64,
    pub(crate) entity: Entity,
    pub(crate) pos: [f32; 3],
    pub(crate) vel: [f32; 3],
    pub(crate) shape: Shape,
    /// Half the size of the box around the shape along each world axis (what the broad phase sorts and sweeps by).
    ext: [f32; 3],
    pub(crate) inv_mass: f32,
    pub(crate) restitution: f32,
    pub(crate) friction: f32,
    gravity_scale: f32,
    drag: f32,
    pub(crate) trigger: bool,
    /// Its collision layer and mask (`CollisionLayer`; layer 0 and every layer without the component): a pair that does not both agree never meets.
    pub(crate) filter: Filter,
    // ---- rotation (`physics_rot`)
    /// The body has an `AngularVelocity` (and is dynamic): its orientation and spin are written back.
    pub(crate) spins: bool,
    /// Contacts of this body use the code of `physics_rot` (it turns, or it is an oriented box or a capsule).
    rich: bool,
    /// The body is asleep (`sleep.rs`): it is built as a body that never moves for this step, and `inv_mass` is 0.
    pub(crate) asleep: bool,
    /// What a body that is turned or turns knows beyond the plain ones: its orientation, spin and inertia. Boxed, so that the bodies of
    /// a world without rotation stay as small as they were (the sort and the sweep over thousands of them are made of cache misses).
    pub(crate) rot: Option<Box<rot::Rot>>,
}

impl Body {
    pub(crate) fn min_on(&self, axis: usize) -> f32 {
        self.pos[axis] - self.extent(axis)
    }
    pub(crate) fn max_on(&self, axis: usize) -> f32 {
        self.pos[axis] + self.extent(axis)
    }
    pub(crate) fn extent(&self, axis: usize) -> f32 {
        self.ext[axis]
    }
    /// The half sizes of the box around a shape (`axes`: its orientation).
    fn bounds(shape: Shape, axes: &rot::M3) -> [f32; 3] {
        match shape {
            Shape::Sphere(r) => [r, r, r],
            Shape::Box(h) => h,
            Shape::Terrain(_, h) => h,
            Shape::Obb(h) => [0, 1, 2].map(|axis| h[0] * axes[0][axis].abs() + h[1] * axes[1][axis].abs() + h[2] * axes[2][axis].abs()),
            Shape::Capsule(r, half) => [0, 1, 2].map(|axis| half * axes[1][axis].abs() + r),
            Shape::Mesh(_) | Shape::Hull(_) => [0.0; 3], // (their boxes are set where the bodies are made)
        }
    }
    /// The body's own x, y and z axes in world coordinates (the identity for a body that is not turned).
    pub(crate) fn axes(&self) -> &rot::M3 {
        match &self.rot {
            Some(r) => &r.axes,
            None => &rot::IDENTITY,
        }
    }
    /// The orientation (a unit quaternion).
    pub(crate) fn q(&self) -> [f32; 4] {
        self.rot.as_deref().map_or(rot::IDENTITY_Q, |r| r.q)
    }
    /// The spin, radians per second about the world axes.
    pub(crate) fn w(&self) -> [f32; 3] {
        self.rot.as_deref().map_or([0.0; 3], |r| r.w)
    }
    /// A body that never moves, standing for the world (or for an entity that is only a `Transform`) as the far end of a joint.
    fn frame(entity: Entity, pos: [f32; 3], q: [f32; 4]) -> Body {
        Body { uid: u64::MAX, entity, pos, vel: [0.0; 3], shape: Shape::Sphere(0.0), ext: [0.0; 3], inv_mass: 0.0, restitution: 0.0, friction: 0.0, gravity_scale: 0.0, drag: 0.0, trigger: true, filter: Filter::DEFAULT, spins: false, rich: false, asleep: false, rot: Some(Box::new(rot::Rot { q, axes: rot::axes_of(q), ..rot::Rot::default() })) }
    }
}

#[inline]
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
fn axpy(a: [f32; 3], s: f32, n: [f32; 3]) -> [f32; 3] {
    [a[0] + s * n[0], a[1] + s * n[1], a[2] + s * n[2]]
}

/// Narrow phase: contact normal (a to b) and penetration depth if the shapes overlap.
fn collide(a: &Body, b: &Body, ground: &[Placed]) -> Option<([f32; 3], f32)> {
    let flip = |r: Option<([f32; 3], f32)>| r.map(|(n, d)| ([-n[0], -n[1], -n[2]], d));
    match (a.shape, b.shape) {
        // (oriented boxes and capsules never get here: their contacts are made by `physics_rot::collide`)
        (Shape::Obb(_) | Shape::Capsule(..), _) | (_, Shape::Obb(_) | Shape::Capsule(..)) => None,
        // (a mesh body is always `rich`: its contacts too are made by `physics_rot::collide`)
        (Shape::Mesh(_), _) | (_, Shape::Mesh(_)) | (Shape::Hull(_), _) | (_, Shape::Hull(_)) => None,
        (Shape::Terrain(..), Shape::Terrain(..)) => None,
        (Shape::Terrain(k, _), Shape::Sphere(r)) => ground[k].sphere(b.pos, r),
        (Shape::Sphere(r), Shape::Terrain(k, _)) => flip(ground[k].sphere(a.pos, r)),
        (Shape::Terrain(k, _), Shape::Box(h)) => ground[k].aabb(b.pos, h),
        (Shape::Box(h), Shape::Terrain(k, _)) => flip(ground[k].aabb(a.pos, h)),
        (Shape::Sphere(ra), Shape::Sphere(rb)) => {
            let d = sub(b.pos, a.pos);
            let r = ra + rb;
            let d2 = dot(d, d);
            if d2 >= r * r {
                return None;
            }
            let dist = d2.sqrt();
            let n = if dist > 1e-9 { [d[0] / dist, d[1] / dist, d[2] / dist] } else { [0.0, 1.0, 0.0] };
            Some((n, r - dist))
        }
        (Shape::Sphere(r), Shape::Box(h)) => sphere_box(a.pos, r, b.pos, h),
        (Shape::Box(h), Shape::Sphere(r)) => sphere_box(b.pos, r, a.pos, h).map(|(n, d)| ([-n[0], -n[1], -n[2]], d)),
        (Shape::Box(ha), Shape::Box(hb)) => {
            let d = sub(b.pos, a.pos);
            let mut best = (f32::INFINITY, 0usize);
            for axis in 0..3 {
                let overlap = ha[axis] + hb[axis] - d[axis].abs();
                if overlap <= 0.0 {
                    return None;
                }
                if overlap < best.0 {
                    best = (overlap, axis);
                }
            }
            let mut n = [0.0; 3];
            n[best.1] = if d[best.1] >= 0.0 { 1.0 } else { -1.0 };
            Some((n, best.0))
        }
    }
}

/// Sphere centre `c` radius `r` against a box centred at `bc`; the returned normal points from the sphere to the box.
pub(crate) fn sphere_box(c: [f32; 3], r: f32, bc: [f32; 3], h: [f32; 3]) -> Option<([f32; 3], f32)> {
    let d = sub(c, bc);
    let closest = [d[0].clamp(-h[0], h[0]), d[1].clamp(-h[1], h[1]), d[2].clamp(-h[2], h[2])];
    let inside = d[0].abs() <= h[0] && d[1].abs() <= h[1] && d[2].abs() <= h[2];
    if inside {
        // centre inside the box: leave through the nearest face
        let mut best = (f32::INFINITY, 0usize);
        for axis in 0..3 {
            let to_face = h[axis] - d[axis].abs();
            if to_face < best.0 {
                best = (to_face, axis);
            }
        }
        let mut n = [0.0; 3]; // from the box to the sphere
        n[best.1] = if d[best.1] >= 0.0 { 1.0 } else { -1.0 };
        return Some(([-n[0], -n[1], -n[2]], best.0 + r));
    }
    let diff = sub(d, closest);
    let dist2 = dot(diff, diff);
    if dist2 >= r * r {
        return None;
    }
    let dist = dist2.sqrt();
    let n = [diff[0] / dist, diff[1] / dist, diff[2] / dist]; // box to sphere
    Some(([-n[0], -n[1], -n[2]], r - dist))
}

#[cfg(test)]
thread_local! {
    /// Tests: look for the pairs of every step with the sweep of all bodies, as the broad phase did before the grid of the statics.
    pub(crate) static BROAD_SWEEP_ALL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The static colliders of a step in a grid over the ground plane (x, z), so that a body that can move finds the statics whose boxes may overlap its own without looking at the others. A static
/// whose box is wider than `BIG` cells (a floor, terrain, a long mesh) is on a list that every mover looks at. The grid is kept from one step to the next (`StaticGrid::get` and `keep`): the colliders that
/// never move have the same boxes in every step, so its layout is the same, and it is only patched for the few whose boxes changed (a door, a car that is not a body).
struct StaticGrid {
    cells: std::collections::HashMap<(i32, i32), Vec<u32>, std::hash::BuildHasherDefault<StaticHasher>>,
    big: Vec<u32>,
    /// How many statics there are (a mover whose box covers very many cells looks at all of them, which is what the sweep would do for it).
    count: usize,
}

/// A cheap hash for the grid's keys (two small integers).
#[derive(Default, Clone, Copy)]
struct StaticHasher(u64);

impl std::hash::Hasher for StaticHasher {
    fn finish(&self) -> u64 {
        self.0.rotate_left(26)
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(5) ^ *b as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
    fn write_i32(&mut self, i: i32) {
        self.0 = (self.0.rotate_left(5) ^ i as u32 as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

thread_local! {
    /// The last grid of this thread and the boxes (x and z, with the room) it was made from.
    static STATIC_GRID: std::cell::RefCell<Option<(Vec<[f32; 4]>, StaticGrid)>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// Tests: grids made afresh, boxes patched, and whether every grid that is reused or patched is checked against a fresh one.
    pub(crate) static GRID_BUILT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    pub(crate) static GRID_PATCHED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    pub(crate) static GRID_CHECK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl StaticGrid {
    /// Side of a cell, in metres, and the widest span of cells a static may have to be put in them.
    const CELL: f32 = 16.0;
    const BIG: i32 = 8;
    /// The room added round every box when it is put in cells and looked up, so that no rounding of the sweep's own comparisons can lose a pair.
    const PAD: f32 = 0.05;

    /// The box of a body as the grid keeps it: x and z, start and end, with the room.
    fn key(b: &Body) -> [f32; 4] {
        [b.min_on(0) - Self::PAD, b.min_on(2) - Self::PAD, b.max_on(0) + Self::PAD, b.max_on(2) + Self::PAD]
    }

    fn span(k: [f32; 4]) -> ((i32, i32), (i32, i32)) {
        let c = |v: f32| (v / Self::CELL).floor() as i32;
        ((c(k[0]), c(k[1])), (c(k[2]), c(k[3])))
    }

    fn is_big(k: [f32; 4]) -> bool {
        let ((x0, z0), (x1, z1)) = Self::span(k);
        x1 - x0 >= Self::BIG || z1 - z0 >= Self::BIG
    }

    /// The grid of the statics whose boxes are `keys` (the lists hold the number of the static in `keys`).
    fn build(keys: &[[f32; 4]]) -> StaticGrid {
        #[cfg(test)]
        GRID_BUILT.with(|b| b.set(b.get() + 1));
        let mut grid = StaticGrid { cells: std::collections::HashMap::with_capacity_and_hasher(keys.len() * 2, Default::default()), big: Vec::new(), count: keys.len() };
        for (k, &key) in keys.iter().enumerate() {
            grid.insert(k as u32, key);
        }
        grid
    }

    fn insert(&mut self, k: u32, key: [f32; 4]) {
        if Self::is_big(key) {
            self.big.push(k);
            return;
        }
        let ((x0, z0), (x1, z1)) = Self::span(key);
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                self.cells.entry((cx, cz)).or_default().push(k);
            }
        }
    }

    fn remove(&mut self, k: u32, key: [f32; 4]) {
        if Self::is_big(key) {
            if let Some(p) = self.big.iter().position(|&x| x == k) {
                self.big.swap_remove(p);
            }
            return;
        }
        let ((x0, z0), (x1, z1)) = Self::span(key);
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                if let Some(list) = self.cells.get_mut(&(cx, cz)) {
                    if let Some(p) = list.iter().position(|&x| x == k) {
                        list.swap_remove(p);
                    }
                    if list.is_empty() {
                        self.cells.remove(&(cx, cz));
                    }
                }
            }
        }
    }

    /// The grid for these boxes: the last one when they are the same, the last one patched where few differ, a new one otherwise.
    fn get(keys: &[[f32; 4]]) -> StaticGrid {
        if let Some((old, mut grid)) = STATIC_GRID.with(|c| c.borrow_mut().take()) {
            if old.len() == keys.len() {
                let changed: Vec<usize> = (0..keys.len()).filter(|&i| old[i] != keys[i]).collect();
                if changed.len() * 4 <= keys.len().max(4) {
                    for &i in &changed {
                        grid.remove(i as u32, old[i]);
                    }
                    for &i in &changed {
                        grid.insert(i as u32, keys[i]);
                    }
                    #[cfg(test)]
                    {
                        GRID_PATCHED.with(|b| b.set(b.get() + changed.len() as u32));
                        if GRID_CHECK.with(|f| f.get()) {
                            let fresh = StaticGrid::build(keys);
                            GRID_BUILT.with(|b| b.set(b.get() - 1));
                            let sorted = |g: &StaticGrid| {
                                let mut cells: Vec<((i32, i32), Vec<u32>)> = g.cells.iter().map(|(c, l)| (*c, { let mut l = l.clone(); l.sort_unstable(); l })).collect();
                                cells.sort();
                                let mut big = g.big.clone();
                                big.sort_unstable();
                                (cells, big)
                            };
                            assert!(sorted(&fresh) == sorted(&grid), "a patched grid has the cells a fresh one has");
                        }
                    }
                    return grid;
                }
            }
        }
        StaticGrid::build(keys)
    }

    /// Hands the grid back for the next step.
    fn keep(self, keys: Vec<[f32; 4]>) {
        STATIC_GRID.with(|c| *c.borrow_mut() = Some((keys, self)));
    }

    /// The statics that may overlap `b` (each once: `stamp` marks the ones already listed with this `epoch`), in `out`.
    fn near(&self, b: &Body, stamp: &mut [u32], epoch: u32, out: &mut Vec<u32>) {
        out.clear();
        for &k in &self.big {
            stamp[k as usize] = epoch;
            out.push(k);
        }
        let ((x0, z0), (x1, z1)) = Self::span(Self::key(b));
        if (x1 as i64 - x0 as i64 + 1) * (z1 as i64 - z0 as i64 + 1) > 256 {
            out.clear();
            out.extend(0..self.count as u32);
            return;
        }
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                if let Some(list) = self.cells.get(&(cx, cz)) {
                    for &k in list {
                        if stamp[k as usize] != epoch {
                            stamp[k as usize] = epoch;
                            out.push(k);
                        }
                    }
                }
            }
        }
    }
}

/// The axis (0, 1 or 2) along which the body centres vary most; ties go to the lowest axis. Sums run in uid order, so the choice is
/// the same on every machine.
fn sweep_axis(bodies: &[Body]) -> usize {
    let n = bodies.len() as f64;
    let mut best = (-1.0f64, 0usize);
    for axis in 0..3 {
        let mean = bodies.iter().map(|b| b.pos[axis] as f64).sum::<f64>() / n;
        let var = bodies.iter().map(|b| (b.pos[axis] as f64 - mean).powi(2)).sum::<f64>();
        if var > best.0 {
            best = (var, axis);
        }
    }
    best.1
}

/// Advance the physics by one fixed step. Returns the contacts found this tick (in uid order).
///
/// Most worlds take the fast path (`fast.rs`): the colliders that never move are kept in a grid between ticks and bodies find their neighbours
/// there, so a tick costs what the moving bodies cost, not what the scenery costs. It gives the same bits as `step_reference`, which is the
/// simple version (everything sorted and swept every tick) that the fast path falls back to for the worlds it does not handle (terrain, meshes,
/// convex hulls, spinning bodies, joints) and that the tests compare it with.
///
/// When the world has a `PhysicsSleep`, groups of bodies that lie still go to sleep (`sleep.rs`); a step in which something wakes is made again
/// from the start (before anything is written), with that island awake.
pub fn step(world: &mut World, gravity: [f32; 3], dt: f32, terrains: &Terrains, meshes: &MeshStore, cache: &PhysicsCache, threads: usize, memory: &mut ContactMemory, sleep: &mut SleepState) -> Vec<Contact> {
    let cfg = sleep::Config::of(world);
    if cfg.is_none() && !sleep.is_empty() {
        sleep.clear();
    }
    for _ in 0..RESTARTS {
        sleep.restart = false;
        match fast::step(world, gravity, dt, terrains, meshes, cache, threads, memory, sleep, cfg) {
            Some(contacts) => return contacts,
            None if sleep.restart => continue,
            None => break,
        }
    }
    step_reference_with(world, gravity, dt, terrains, meshes, memory, sleep, cfg)
}

/// How many times a step is started again because an island woke before the step falls back to the reference, which wakes everything if it must.
const RESTARTS: usize = 8;

/// The parts of the reference step that `reference_stage_ns` times.
pub const REF_STAGES: [&str; 12] = ["gather", "joints, forces", "continuous collision", "broad phase", "solve", "corrections", "sleeping", "write back", "gather: queries", "gather: sort of the colliders", "gather: bodies of the colliders", "gather: bodies of the meshes"];
static REF_PROFILE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static REF_STAGE_NS: [std::sync::atomic::AtomicU64; 12] = [const { std::sync::atomic::AtomicU64::new(0) }; 12];

/// Time the parts of the reference step from now on (a few clock reads a step; for every world that takes the reference path, that is one with spinning bodies,
/// joints, terrain, meshes or hulls). `reference_stage_ns` reads the totals since the last call and clears them.
pub fn reference_profile(on: bool) {
    REF_PROFILE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The nanoseconds spent in each part of the reference step (see `REF_STAGES`) since the last call; all zero unless `reference_profile(true)`.
pub fn reference_stage_ns() -> [u64; 12] {
    std::array::from_fn(|i| REF_STAGE_NS[i].swap(0, std::sync::atomic::Ordering::Relaxed))
}

/// Solve the worlds that have joints (humanoids, ragdolls) in exactly `rounds` rounds every step, with no early stop (0 = back to the automatic number, which shares a budget of
/// about 20,000 row visits among everything in the world: the more joints, the fewer rounds each gets, down to 20, so a crowd of ragdolls sags). A fixed number makes the cost
/// of a step known and, because every body then gets the same rounds whatever else is in the world, makes bodies that touch nothing in common behave identically whether they
/// share a world or not. It is global (the engine has one solver), changes nothing for a world without joints and nothing at all until it is called.
pub fn set_fixed_rounds(rounds: usize) {
    rot::FIXED_ROUNDS.store(rounds, std::sync::atomic::Ordering::Relaxed);
}

/// The simple physics step: gathers every collider, sorts them all and sweeps them all, every tick. The oracle of the fast path.
pub fn step_reference(world: &mut World, gravity: [f32; 3], dt: f32, terrains: &Terrains, meshes: &MeshStore, memory: &mut ContactMemory, sleep: &mut SleepState) -> Vec<Contact> {
    let cfg = sleep::Config::of(world);
    if cfg.is_none() && !sleep.is_empty() {
        sleep.clear();
    }
    step_reference_with(world, gravity, dt, terrains, meshes, memory, sleep, cfg)
}

#[allow(clippy::too_many_arguments)]
fn step_reference_with(world: &mut World, gravity: [f32; 3], dt: f32, terrains: &Terrains, meshes: &MeshStore, memory: &mut ContactMemory, sleep: &mut SleepState, cfg: Option<sleep::Config>) -> Vec<Contact> {
    for _ in 0..RESTARTS {
        sleep.restart = false;
        if let Some(contacts) = step_reference_once(world, gravity, dt, terrains, meshes, memory, sleep, cfg) {
            return contacts;
        }
    }
    // (every restart wakes at least one island, so this is never reached with more than a few hundred sleepers in the way; nothing sleeps after this)
    sleep.clear();
    step_reference_once(world, gravity, dt, terrains, meshes, memory, sleep, None).unwrap_or_default()
}

/// One try at the reference step; `None` when an island woke and the step has to be made again.
#[allow(clippy::too_many_arguments)]
fn step_reference_once(world: &mut World, gravity: [f32; 3], dt: f32, terrains: &Terrains, meshes: &MeshStore, memory: &mut ContactMemory, sleep: &mut SleepState, cfg: Option<sleep::Config>) -> Option<Vec<Contact>> {
    // the contact impulses of the bodies that woke in the last try are given back first
    sleep.take_pending(memory);
    let mut lap_at = REF_PROFILE.load(std::sync::atomic::Ordering::Relaxed).then(std::time::Instant::now);
    // `lap!(n)`: the time since the last lap is part `n` of the step
    macro_rules! lap {
        ($n:expr) => {
            if let Some(t0) = lap_at {
                let now = std::time::Instant::now();
                REF_STAGE_NS[$n].fetch_add(now.duration_since(t0).as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
                lap_at = Some(now);
            }
        };
    }
    // ---- gather
    let mut raw: Vec<(u64, Entity, Transform, Collider)> = Vec::new();
    world.for_each_uid::<(&Transform, &Collider), _>(|e, uid, (t, c)| raw.push((uid, e, *t, *c))).expect("physics query is valid");
    let mut ground_raw: Vec<(u64, Entity, Transform, Terrain)> = Vec::new();
    if !terrains.is_empty() {
        world.for_each_uid::<(&Transform, &Terrain), _>(|e, uid, (t, g)| ground_raw.push((uid, e, *t, *g))).expect("terrain query is valid");
    }
    // the mesh colliders and the convex hulls (a world without any has an empty store and pays for nothing)
    let mesh_instances = if meshes.is_empty() { Vec::new() } else { meshes.instances(world) };
    let mut hull_raw: Vec<(u64, Entity, Transform, ConvexCollider)> = Vec::new();
    if !meshes.is_empty() {
        world.for_each_uid::<(&Transform, &ConvexCollider), _>(|e, uid, (t, c)| hull_raw.push((uid, e, *t, *c))).expect("hull query is valid");
    }
    if raw.is_empty() && ground_raw.is_empty() && mesh_instances.is_empty() && hull_raw.is_empty() {
        memory.clear();
        sleep.clear();
        return Some(Vec::new());
    }
    lap!(8);
    raw.sort_by_key(|r| r.0);
    lap!(9);
    // who collides with whom (a world without a `CollisionLayer` has an empty list and filters nothing)
    let layers = Layers::gather(world);
    // the bodies that turn (a world without any pays for nothing but this query)
    let mut spinners: Vec<(u64, ([f32; 3], f32, [f32; 4]))> = Vec::new();
    world.for_each_uid::<(&AngularVelocity,), _>(|_, uid, (a,)| spinners.push((uid, (a.w, a.damping, a.stash)))).expect("angular velocity query is valid");
    spinners.sort_by_key(|s| s.0);
    let mut bodies: Vec<Body> = Vec::with_capacity(raw.len());
    // the sleepers that were found and still lie as they did, and the islands to wake because one of theirs does not
    let mut seen = vec![false; sleep.sleeper_count()];
    let mut bad: Vec<u64> = Vec::new();
    // which colliders have a `RigidBody` (a few among thousands of scenery boxes: the others are not asked for it, or for a `Velocity`, one by one)
    let mut rigid: Vec<u64> = Vec::new();
    world.for_each_uid::<(&RigidBody,), _>(|_, uid, _| rigid.push(uid)).expect("rigid body query is valid");
    rigid.sort_unstable();
    for (uid, entity, t, c) in raw {
        let scale = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
        let (mut inv_mass, mut restitution, mut friction, mut gravity_scale, mut drag, mut vel) = (0.0, 0.3, 0.5, 1.0, 0.0, [0.0; 3]);
        let mut mass = 0.0;
        let mut asleep = false;
        if rigid.binary_search(&uid).is_ok() {
            if let (Some(rb), Some(v)) = (world.get::<RigidBody>(entity), world.get::<Velocity>(entity)) {
                if rb.mass > 0.0 {
                    inv_mass = 1.0 / rb.mass;
                    mass = rb.mass;
                    vel = v.lin;
                    // (only a body that is dynamic can be a sleeper; one that stopped being one is not found, and its island wakes)
                    if sleep.has_sleepers() {
                        let spin = spinners.binary_search_by_key(&uid, |s| s.0).map_or([0.0; 3], |i| spinners[i].1 .0);
                        asleep = sleep.gather(uid, &t, &c, v.lin, spin, &mut seen, &mut bad);
                    }
                }
                restitution = rb.restitution.clamp(0.0, 1.0);
                friction = rb.friction.max(0.0);
                gravity_scale = rb.gravity_scale;
                drag = rb.drag.max(0.0);
            } else if let Some(rb) = world.get::<RigidBody>(entity) {
                restitution = rb.restitution.clamp(0.0, 1.0);
                friction = rb.friction.max(0.0);
            }
        }
        if asleep {
            // (a sleeper is a body that never moves for this step; it keeps its orientation, which the box it is shaped by needs)
            inv_mass = 0.0;
            mass = 0.0;
            vel = [0.0; 3];
        }
        // a dynamic body with an `AngularVelocity` turns; an oriented box or a capsule is handled by `physics_rot`
        let spin = if inv_mass > 0.0 && !spinners.is_empty() { spinners.binary_search_by_key(&uid, |s| s.0).ok().map(|i| spinners[i].1) } else { None };
        let turned = t.rot[0] != 0.0 || t.rot[1] != 0.0 || t.rot[2] != 0.0;
        let mut rich = false;
        let shape = if c.shape == 2 {
            rich = true;
            let r = 0.5 * c.size[0].abs() * scale[0].max(scale[2]);
            Shape::Capsule(r, (0.5 * c.size[1].abs() * scale[1] - r).max(0.0))
        } else if c.shape == 0 {
            rich = spin.is_some();
            Shape::Sphere(0.5 * c.size[0].abs() * scale[0].max(scale[1]).max(scale[2]))
        } else {
            let h = [0.5 * c.size[0].abs() * scale[0], 0.5 * c.size[1].abs() * scale[1], 0.5 * c.size[2].abs() * scale[2]];
            if spin.is_some() || turned {
                rich = true;
                Shape::Obb(h)
            } else {
                Shape::Box(h)
            }
        };
        let mut state = None;
        if turned || spin.is_some() {
            let mut r = rot::Rot { q: rot::normalize_q(t.rot), ..rot::Rot::default() };
            if let Some((w0, damping, stash)) = spin {
                // the orientation moves on by last tick's spin, like the position by last tick's velocity (the `movement` system)
                r.w = rot::clean_spin(w0);
                // a ball that was slowed to arrive at a wall (the end of the last tick) gets its real velocity back, if nobody changed it since
                if stash[3] != 0.0 && rot::mul([stash[0], stash[1], stash[2]], stash[3]) == vel {
                    vel = [stash[0], stash[1], stash[2]];
                }
                r.q = rot::spin_q(r.q, r.w, dt);
                r.inv_i = rot::inv_inertia(&shape, mass);
                r.spin_damping = damping;
            }
            r.axes = rot::axes_of(r.q);
            state = Some(Box::new(r));
        }
        let ext = Body::bounds(shape, state.as_deref().map_or(&rot::IDENTITY, |r| &r.axes));
        bodies.push(Body { uid, entity, pos: t.pos, vel, shape, ext, inv_mass, restitution, friction, gravity_scale, drag, trigger: c.trigger, filter: layers.of(uid), spins: spin.is_some(), rich, asleep, rot: state });
    }
    lap!(10);

    // the terrain: a static body of its own, in uid order with the others
    let mut ground: Vec<Placed> = Vec::new();
    ground_raw.sort_by_key(|r| r.0);
    for (uid, entity, t, g) in ground_raw {
        if !g.solid {
            continue;
        }
        let name = world.strings().get(g.heightmap.0);
        let Some(field) = terrains.get(name) else { continue };
        let placed = Placed::new(field, t.pos, &g);
        let (lo, hi) = placed.y_range();
        let half = [placed.size[0] * 0.5, (hi - lo) * 0.5, placed.size[1] * 0.5];
        ground.push(placed);
        bodies.push(Body { uid, entity, pos: [placed.centre[0], (lo + hi) * 0.5, placed.centre[1]], vel: [0.0; 3], shape: Shape::Terrain(ground.len() - 1, half), ext: half, inv_mass: 0.0, restitution: g.restitution.clamp(0.0, 1.0), friction: g.friction.max(0.0), gravity_scale: 1.0, drag: 0.0, trigger: false, filter: layers.of(uid), spins: false, rich: false, asleep: false, rot: None });
    }
    // the convex hulls: a body each, at its centre of mass and turned to its principal axes (see `hull.rs`); always rich
    let mut placed_hulls: Vec<Arc<Hull>> = Vec::with_capacity(hull_raw.len());
    for (uid, entity, t, c) in hull_raw {
        let scale = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
        let name = world.strings().get(c.mesh.0).to_string();
        let Ok(hull) = meshes.hull(&name, scale) else { continue };
        let (mut inv_mass, mut restitution, mut friction, mut gravity_scale, mut drag, mut vel, mut mass) = (0.0, 0.3, 0.5, 1.0, 0.0, [0.0; 3], 0.0);
        if let (Some(rb), Some(v)) = (world.get::<RigidBody>(entity), world.get::<Velocity>(entity)) {
            if rb.mass > 0.0 {
                inv_mass = 1.0 / rb.mass;
                mass = rb.mass;
                vel = v.lin;
            }
            restitution = rb.restitution.clamp(0.0, 1.0);
            friction = rb.friction.max(0.0);
            gravity_scale = rb.gravity_scale;
            drag = rb.drag.max(0.0);
        } else if let Some(rb) = world.get::<RigidBody>(entity) {
            restitution = rb.restitution.clamp(0.0, 1.0);
            friction = rb.friction.max(0.0);
        }
        let spin = if inv_mass > 0.0 && !spinners.is_empty() { spinners.binary_search_by_key(&uid, |s| s.0).ok().map(|i| spinners[i].1) } else { None };
        let (pos, q0) = hull.pose(&t);
        if !pos.iter().all(|v| v.is_finite()) {
            continue;
        }
        let mut r = rot::Rot { q: q0, ..rot::Rot::default() };
        if let Some((w0, damping, _)) = spin {
            // (the orientation moves on by last tick's spin, like the position by last tick's velocity)
            r.w = rot::clean_spin(w0);
            r.q = rot::spin_q(r.q, r.w, dt);
            r.inv_i = hull.inv_inertia(mass);
            r.spin_damping = damping;
        }
        r.axes = rot::axes_of(r.q);
        let ext = hull.world_extent(&r.axes);
        placed_hulls.push(hull);
        bodies.push(Body { uid, entity, pos, vel, shape: Shape::Hull(placed_hulls.len() - 1), ext, inv_mass, restitution, friction, gravity_scale, drag, trigger: c.trigger, filter: layers.of(uid), spins: spin.is_some(), rich: true, asleep: false, rot: Some(Box::new(r)) });
    }

    // the mesh colliders: a static body each, whose box is the box around the placed mesh (rich: all its contacts come from `physics_rot`)
    let mut placed_meshes: Vec<Arc<PlacedMesh>> = Vec::with_capacity(mesh_instances.len());
    let mut mesh_filters: Vec<Filter> = Vec::with_capacity(mesh_instances.len());
    for inst in &mesh_instances {
        let (centre, half) = (inst.placed.bounds.centre(), inst.placed.bounds.half());
        placed_meshes.push(inst.placed.clone());
        mesh_filters.push(layers.of(inst.uid));
        bodies.push(Body { uid: inst.uid, entity: inst.entity, pos: centre, vel: [0.0; 3], shape: Shape::Mesh(placed_meshes.len() - 1), ext: half, inv_mass: 0.0, restitution: inst.restitution, friction: inst.friction, gravity_scale: 1.0, drag: 0.0, trigger: false, filter: layers.of(inst.uid), spins: false, rich: true, asleep: false, rot: None });
    }
    lap!(11);
    if bodies.is_empty() {
        memory.clear();
        sleep.clear();
        return Some(Vec::new());
    }
    bodies.sort_by_key(|b| b.uid);
    let statics = Statics { ground: &ground, meshes: &placed_meshes, mesh_filters: &mesh_filters, hulls: &placed_hulls, dt };

    // ---- sleepers: one that was moved, spun or pushed, or whose entity is gone, wakes its island; and when the scenery changed since the last
    // step, so do the islands whose sleepers see other scenery around them than when they lay down
    let mut scenery = 0u64;
    if cfg.is_some() {
        if sleep.has_sleepers() {
            sleep.finish_gather(&seen, &mut bad);
            if sleep.restart {
                return None;
            }
        }
        scenery = sleep::scenery_sig(&bodies);
        if sleep.has_sleepers() && scenery != sleep.statics() {
            let idx = sleep::scenery_idx(&bodies);
            sleep.check_scenery(&|pos, ext| sleep::touch_in(&bodies, &idx, pos, ext));
            if sleep.restart {
                return None;
            }
        }
    }

    lap!(0);
    // ---- joints: who is tied to what (owners in uid order)
    let mut joints: Vec<(u64, Entity, Joint)> = Vec::new();
    world.for_each_uid::<(&Joint,), _>(|e, uid, (j,)| joints.push((uid, e, *j))).expect("joint query is valid");
    joints.sort_by_key(|j| j.0);
    let find = |bodies: &[Body], uid: u64| bodies.binary_search_by_key(&uid, |b| b.uid).ok();
    let mut links: Vec<Link> = Vec::new();
    // ball-and-socket joints and hinges (`physics_rot`): (the body, the joint), and the pairs of uids that they keep from colliding
    let mut pin_joints: Vec<(usize, Joint)> = Vec::new();
    let mut pinned: Vec<(u64, u64)> = Vec::new();
    for (uid, _, j) in &joints {
        let Some(a) = find(&bodies, *uid).filter(|&a| bodies[a].inv_mass > 0.0) else { continue };
        if j.kind == 3 || j.kind == 4 {
            pin_joints.push((a, *j));
            if j.target.0 != 0 && find(&bodies, j.target.0).is_some() {
                pinned.push(((*uid).min(j.target.0), (*uid).max(j.target.0)));
            }
            continue;
        }
        let (b, fixed) = if j.target.0 != 0 {
            match find(&bodies, j.target.0) {
                Some(b) if b != a => (Some(b), [0.0; 3]),
                Some(_) => continue,
                None => match world.entity(j.target.0).and_then(|e| world.get::<Transform>(e)) {
                    Some(t) => (None, t.pos),
                    None => continue,
                },
            }
        } else {
            (None, j.anchor)
        };
        if !fixed.iter().all(|v| v.is_finite()) || !j.length.is_finite() || !j.stiffness.is_finite() || !j.damping.is_finite() {
            continue;
        }
        let mut link = Link { a, b, fixed, kind: j.kind, length: j.length, stiffness: j.stiffness.max(0.0), damping: j.damping.max(0.0), measured: false };
        if j.length <= 0.0 {
            let Some((_, dist)) = link.span(&bodies) else { continue };
            link.length = dist;
            link.measured = true;
        }
        links.push(link);
    }
    pinned.sort_unstable();

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
    if !spinners.is_empty() {
        for b in bodies.iter_mut().filter(|b| b.spins) {
            rot::spin_forces(b, dt);
        }
    }

    // springs: implicit Euler on the distance (stable however stiff), so the same ties never explode
    for l in links.iter().filter(|l| l.kind == 2) {
        let Some((n, dist)) = l.span(&bodies) else { continue };
        let (ia, ib) = l.inv_masses(&bodies);
        let inv_sum = ia + ib;
        if inv_sum == 0.0 {
            continue;
        }
        let m = 1.0 / inv_sum;
        let u0 = l.closing(&bodies, n);
        let u1 = (u0 - dt * (l.stiffness / m) * (dist - l.length)) / (1.0 + dt * l.damping / m + dt * dt * l.stiffness / m);
        l.push(&mut bodies, n, m * (u1 - u0));
    }

    lap!(1);
    // fast bodies against mesh colliders: one that went through a thin surface during the last tick is put back against it (`rot::catch_up`)
    if !placed_meshes.is_empty() {
        // (the statics are only read there, the bodies are written)
        rot::catch_up(&mut bodies, &statics);
    }
    // fast bodies against the colliders that never move: one that crossed a thin wall or floor between two ticks is put back against it
    let fixes = rot::tunnel_fixes(
        &bodies,
        &v0,
        &mut |b, lo, hi, out| {
            for o in bodies.iter() {
                if o.inv_mass == 0.0 && !o.trigger && !matches!(o.shape, Shape::Mesh(_)) && b.filter.collides(o.filter) && (0..3).all(|a| o.max_on(a) >= lo[a] && o.min_on(a) <= hi[a]) {
                    out.push(o.clone());
                }
            }
        },
        &statics,
    );
    for (i, p) in fixes {
        bodies[i].pos = p;
    }

    lap!(2);
    // ---- broad phase: the pairs of bodies whose boxes overlap, at least one of the two a body that can move (or a trigger): two static colliders never need resolving. The set of pairs does not
    // depend on how they are looked for, and `found` is sorted by uid below. Few movers among many statics (a city: thousands of walls, a few cars): the movers sweep among themselves and each
    // looks up the statics round it in a grid made for this step (`StaticGrid`), where sorting and sweeping every body, statics included, was most of what a step cost (1 ms of 4.9 in a game's
    // city server). Every candidate goes through the same tests whichever way it came, the sweep's own first, so the pairs are exactly the ones the sweep of all bodies found.
    let sweep = sweep_axis(&bodies);
    let mut found: Vec<(usize, usize, [f32; 3], f32)> = Vec::new();
    let mut woken: Vec<u64> = Vec::new();
    // contacts that involve a body that turns, an oriented box or a capsule: each has several points (`physics_rot`)
    let mut rich_found: Vec<(usize, usize, Vec<rot::Pt>)> = Vec::new();
    // `before(i, j)`: the sweep puts `i` before `j` (the smaller box start along the axis, the smaller uid on a tie)
    let before = |bodies: &[Body], i: usize, j: usize| bodies[i].min_on(sweep).total_cmp(&bodies[j].min_on(sweep)).then(bodies[i].uid.cmp(&bodies[j].uid)) != std::cmp::Ordering::Greater;
    let consider = |bodies: &[Body], i: usize, j: usize, found: &mut Vec<(usize, usize, [f32; 3], f32)>, rich_found: &mut Vec<(usize, usize, Vec<rot::Pt>)>, woken: &mut Vec<u64>| {
        let (a, b) = if bodies[i].uid < bodies[j].uid { (i, j) } else { (j, i) };
        if bodies[a].inv_mass == 0.0 && bodies[b].inv_mass == 0.0 && !bodies[a].trigger && !bodies[b].trigger {
            return; // two static colliders never need resolving
        }
        if bodies[a].uid == bodies[b].uid {
            return; // (one entity with both a Collider and a MeshCollider does not collide with itself)
        }
        if !bodies[a].filter.collides(bodies[b].filter) {
            return; // (the two collision layers do not both want the other: they pass through each other, triggers included)
        }
        // quick rejection on the other axes
        if (0..3).any(|axis| (bodies[a].pos[axis] - bodies[b].pos[axis]).abs() > bodies[a].extent(axis) + bodies[b].extent(axis)) {
            return;
        }
        if !pinned.is_empty() && pinned.binary_search(&(bodies[a].uid, bodies[b].uid)).is_ok() {
            return; // two bodies tied by a ball-and-socket or a hinge do not collide with each other
        }
        if bodies[a].asleep != bodies[b].asleep {
            // a sleeper that an awake body's box overlaps wakes with its island (the step is then made again)
            let (s, o) = if bodies[a].asleep { (a, b) } else { (b, a) };
            if bodies[o].inv_mass > 0.0 && !bodies[o].trigger {
                woken.extend(sleep.island_of(bodies[s].uid));
                return;
            }
        }
        if bodies[a].rich || bodies[b].rich {
            let pts = rot::collide(&bodies[a], &bodies[b], &statics);
            if !pts.is_empty() {
                rich_found.push((a, b, pts));
            }
        } else if let Some((n, depth)) = collide(&bodies[a], &bodies[b], &ground) {
            found.push((a, b, n, depth));
        }
    };
    let is_mover = |b: &Body| b.inv_mass > 0.0 || b.trigger;
    let movers: Vec<usize> = (0..bodies.len()).filter(|&i| is_mover(&bodies[i])).collect();
    #[cfg(test)]
    let sweep_all = BROAD_SWEEP_ALL.with(|f| f.get());
    #[cfg(not(test))]
    let sweep_all = false;
    // (a body with a number that is not finite is looked at the old way: its box overlaps whatever the sweep's comparisons say it does)
    let grid_ok = !sweep_all && movers.len() * 8 <= bodies.len() && bodies.iter().all(|b| b.pos.iter().chain(b.ext.iter()).all(|v| v.is_finite()));
    if grid_ok {
        // the movers sweep among themselves
        let mut order: Vec<usize> = movers.clone();
        order.sort_by(|&i, &j| bodies[i].min_on(sweep).total_cmp(&bodies[j].min_on(sweep)).then(bodies[i].uid.cmp(&bodies[j].uid)));
        for oi in 0..order.len() {
            let i = order[oi];
            let max_on = bodies[i].max_on(sweep);
            for &j in &order[oi + 1..] {
                if bodies[j].min_on(sweep) > max_on {
                    break;
                }
                consider(&bodies, i, j, &mut found, &mut rich_found, &mut woken);
            }
        }
        // and each looks up the statics round it
        let statics_idx: Vec<usize> = (0..bodies.len()).filter(|&i| !is_mover(&bodies[i])).collect();
        let keys: Vec<[f32; 4]> = statics_idx.iter().map(|&i| StaticGrid::key(&bodies[i])).collect();
        let grid = StaticGrid::get(&keys);
        let mut stamp = vec![0u32; statics_idx.len()];
        let mut query: Vec<u32> = Vec::new();
        for (epoch, &m) in movers.iter().enumerate() {
            grid.near(&bodies[m], &mut stamp, epoch as u32 + 1, &mut query);
            for &k in &query {
                let s = statics_idx[k as usize];
                let (i, j) = if before(&bodies, m, s) { (m, s) } else { (s, m) };
                if bodies[j].min_on(sweep) > bodies[i].max_on(sweep) {
                    continue;
                }
                consider(&bodies, i, j, &mut found, &mut rich_found, &mut woken);
            }
        }
        grid.keep(keys);
    } else {
        let mut order: Vec<usize> = (0..bodies.len()).collect();
        order.sort_by(|&i, &j| bodies[i].min_on(sweep).total_cmp(&bodies[j].min_on(sweep)).then(bodies[i].uid.cmp(&bodies[j].uid)));
        for oi in 0..order.len() {
            let i = order[oi];
            let max_on = bodies[i].max_on(sweep);
            for &j in &order[oi + 1..] {
                if bodies[j].min_on(sweep) > max_on {
                    break;
                }
                consider(&bodies, i, j, &mut found, &mut rich_found, &mut woken);
            }
        }
    }
    if !woken.is_empty() {
        sleep.wake_islands(&mut woken);
        return None;
    }
    found.sort_by_key(|f| (bodies[f.0].uid, bodies[f.1].uid));
    rich_found.sort_by_key(|f| (bodies[f.0].uid, bodies[f.1].uid));

    lap!(3);
    // ---- velocity solve: sequential impulses
    let solid: Vec<usize> = (0..found.len()).filter(|&k| !bodies[found[k].0].trigger && !bodies[found[k].1].trigger).collect();
    let rich_solid: Vec<usize> = (0..rich_found.len()).filter(|&k| !bodies[rich_found[k].0].trigger && !bodies[rich_found[k].1].trigger).collect();
    // ropes and rods: the direction and the stretch are those of the start of the tick
    let spans: Vec<Option<([f32; 3], f32)>> = links.iter().map(|l| if l.kind == 2 { None } else { l.span(&bodies) }).collect();
    // the contact points of the bodies that turn, one row each, and the ball-and-socket joints and hinges (the far end of one that has
    // no body is a frame that never moves, put after the bodies)
    let mut rows: Vec<rot::Row> = Vec::new();
    for &k in &rich_solid {
        let (a, b, ref pts) = rich_found[k];
        for pt in pts {
            rows.push(rot::prepare_row(&bodies, a, b, pt, dt));
        }
    }
    rot::apply_memory(&mut bodies, &mut rows, &std::mem::take(memory));
    rot::warm_start(&mut bodies, &mut rows, gravity, dt);
    let mut pins: Vec<rot::Pin> = Vec::new();
    for (a, j) in &pin_joints {
        let b = if j.target.0 == 0 {
            bodies.push(Body::frame(bodies[*a].entity, [0.0; 3], rot::IDENTITY_Q));
            bodies.len() - 1
        } else {
            match find(&bodies, j.target.0) {
                Some(b) if b != *a => b,
                Some(_) => continue,
                None => match world.entity(j.target.0).and_then(|e| world.get::<Transform>(e)) {
                    Some(t) => {
                        bodies.push(Body::frame(bodies[*a].entity, t.pos, rot::normalize_q(t.rot)));
                        bodies.len() - 1
                    }
                    None => continue,
                },
            }
        };
        let motor = world.get::<crate::components::JointMotor>(bodies[*a].entity).map(|m| (m.speed, m.max_torque));
        let cone = world.get::<crate::components::JointCone>(bodies[*a].entity).map(|c| c.angle);
        let spring = world.get::<crate::components::JointSpring>(bodies[*a].entity).map(|s| (s.stiffness, s.damping, s.angle));
        pins.extend(rot::make_pin(&bodies, *a, b, j, motor, cone, spring, dt));
    }
    let rounds = if rows.is_empty() && pins.is_empty() { ITERATIONS } else { rot::rounds_for_joints(rows.len(), pins.len()) };
    // (the rounds stop when a whole round changes the speeds by less than `SETTLED`, or by a thousandth of what the first round did; ropes do not tell, so a world
    // with any goes on to the end)
    let can_settle = links.is_empty() && !(rot::FIXED_ROUNDS.load(std::sync::atomic::Ordering::Relaxed) > 0 && !pins.is_empty());
    let mut settled_at = rot::SETTLED;
    for round in 0..rounds {
        let mut delta = 0.0f32;
        if round < ITERATIONS {
            for (l, span) in links.iter().zip(&spans) {
                let Some((n, dist)) = *span else { continue };
                let (ia, ib) = l.inv_masses(&bodies);
                let inv_sum = ia + ib;
                if inv_sum == 0.0 {
                    continue;
                }
                let u = l.closing(&bodies, n);
                // a rope pulls only when it is taut and the ends are moving apart; a rod holds the distance both ways
                let taut = if l.kind == 1 { true } else { dist >= l.length - SLOP };
                if taut && (l.kind == 1 || u > 0.0) {
                    l.push(&mut bodies, n, -u / inv_sum);
                }
            }
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
                // the bouncier of the two materials wins (Box2D's rule), so a bouncy ball on a default floor still bounces
                let e = if -vn < BOUNCE_THRESHOLD { 0.0 } else { bodies[ia].restitution.max(bodies[ib].restitution) };
                let j = -(1.0 + e) * vn / inv_sum;
                bodies[ia].vel = axpy(bodies[ia].vel, -j * inv_a, n);
                bodies[ib].vel = axpy(bodies[ib].vel, j * inv_b, n);
                // friction along the tangent, limited by the normal impulse
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
        for p in pins.iter_mut() {
            delta = delta.max(rot::solve_pin(&mut bodies, p));
        }
        for r in rows.iter_mut() {
            delta = delta.max(rot::solve_row(&mut bodies, r));
        }
        if round == 0 {
            settled_at = rot::SETTLED.max(delta * rot::SETTLED_REL);
            if can_settle && delta < rot::SETTLED {
                break;
            }
        } else if can_settle && delta < settled_at {
            break;
        }
    }

    *memory = rot::collect_memory(&bodies, &rows);
    lap!(4);
    // ---- joints: pull the ends back to the length they should have (once, like the contacts)
    for (l, span) in links.iter().zip(&spans) {
        let Some((n, dist)) = *span else { continue };
        let (ia, ib) = l.inv_masses(&bodies);
        let inv_sum = ia + ib;
        let stretch = dist - l.length;
        let off = if l.kind == 1 { stretch.abs() > SLOP } else { stretch > SLOP };
        if inv_sum == 0.0 || !off {
            continue;
        }
        let corr = (stretch.abs() - SLOP) / inv_sum * CORRECTION * stretch.signum();
        bodies[l.a].pos = axpy(bodies[l.a].pos, corr * ia, n);
        if let Some(b) = l.b {
            bodies[b].pos = axpy(bodies[b].pos, -corr * ib, n);
        }
    }

    // ball-and-socket joints and hinges: bring the two pivot points together
    for p in &pins {
        rot::correct_pin(&mut bodies, p);
    }

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
    // (a contact with several points is pushed apart by its deepest point, along that point's normal; against a mesh, where the points
    // may lie on a floor and a wall at once, by the deepest point of each direction)
    for &k in &rich_solid {
        let (ia, ib, ref pts) = rich_found[k];
        let inv_sum = bodies[ia].inv_mass + bodies[ib].inv_mass;
        if inv_sum == 0.0 {
            continue;
        }
        if matches!(bodies[ia].shape, Shape::Mesh(_)) || matches!(bodies[ib].shape, Shape::Mesh(_)) {
            for (n, depth) in rot::deepest_per_direction(pts) {
                let corr = (depth - SLOP).max(0.0) / inv_sum * CORRECTION;
                bodies[ia].pos = axpy(bodies[ia].pos, -corr * bodies[ia].inv_mass, n);
                bodies[ib].pos = axpy(bodies[ib].pos, corr * bodies[ib].inv_mass, n);
            }
            continue;
        }
        let (n, depth) = rot::deepest(pts);
        let corr = (depth - SLOP).max(0.0) / inv_sum * CORRECTION;
        bodies[ia].pos = axpy(bodies[ia].pos, -corr * bodies[ia].inv_mass, n);
        bodies[ib].pos = axpy(bodies[ib].pos, corr * bodies[ib].inv_mass, n);
    }

    // fast spinning balls: one that would hit something solid on its next step is slowed to arrive there exactly (`rot::sweep_ahead`)
    if !spinners.is_empty() {
        rot::sweep_ahead(&mut bodies, &statics, dt);
    }

    lap!(5);
    // ---- sleeping: islands that have lain still long enough go to sleep (their velocities are zeroed here, and zero is what is written back)
    if let Some(cfg) = &cfg {
        let pairs: Vec<(usize, usize)> = solid.iter().map(|&k| (found[k].0, found[k].1)).chain(rich_solid.iter().map(|&k| (rich_found[k].0, rich_found[k].1))).collect();
        // (a body tied by a joint or a link never sleeps)
        let mut never = vec![false; bodies.len()];
        for p in &pins {
            never[p.a] = true;
            never[p.b] = true;
        }
        for l in &links {
            never[l.a] = true;
            if let Some(b) = l.b {
                never[b] = true;
            }
        }
        sleep::update(sleep, cfg, dt, &mut bodies, &pairs, &never, &sleep::touch_in, scenery, memory);
    }

    lap!(6);
    // ---- write back (only dynamic bodies change)
    for b in bodies.iter().filter(|b| b.inv_mass > 0.0) {
        if let Some(t) = world.get_mut::<Transform>(b.entity) {
            t.pos = b.pos;
        }
        if let Some(v) = world.get_mut::<Velocity>(b.entity) {
            v.lin = b.vel;
        }
    }
    if !spinners.is_empty() {
        for b in bodies.iter().filter(|b| b.spins) {
            let Some(state) = b.rot.as_deref() else { continue };
            if let Some(t) = world.get_mut::<Transform>(b.entity) {
                t.rot = state.q;
            }
            if let Some(a) = world.get_mut::<AngularVelocity>(b.entity) {
                a.w = state.w;
                a.stash = state.stash;
            }
        }
    }
    // a convex hull body is at its centre of mass and turned to its principal axes: the entity's own position and rotation are worked back
    // from those (a hull that does not turn keeps its rotation exactly; one with its centre of mass at the model's origin keeps `pos` as is)
    for b in bodies.iter().filter(|b| b.inv_mass > 0.0) {
        let Shape::Hull(k) = b.shape else { continue };
        let hull = &placed_hulls[k];
        if b.spins {
            let (pos, rot_q) = hull.model_pose(b.pos, b.q());
            if let Some(t) = world.get_mut::<Transform>(b.entity) {
                t.pos = pos;
                t.rot = rot_q;
            }
        } else if let Some(t) = world.get_mut::<Transform>(b.entity) {
            t.pos = hull.model_pos(b.pos, t.rot);
        }
    }
    for l in links.iter().filter(|l| l.measured) {
        if let Some(j) = world.get_mut::<Joint>(bodies[l.a].entity) {
            j.length = l.length;
        }
    }
    for p in pins.iter().filter(|p| p.fresh) {
        if let Some(j) = world.get_mut::<Joint>(bodies[p.a].entity) {
            j.local_a = p.local_a;
            j.local_b = p.local_b;
            j.local_axis = p.local_axis;
            j.rest = p.rest;
        }
    }
    sleep.finish_new(world);
    lap!(7);
    let mut contacts: Vec<Contact> = found.iter().map(|&(a, b, n, depth)| Contact { a: bodies[a].uid, b: bodies[b].uid, normal: n, depth, trigger: bodies[a].trigger || bodies[b].trigger }).collect();
    if !rich_found.is_empty() {
        for (a, b, pts) in &rich_found {
            let (normal, depth) = rot::deepest(pts);
            contacts.push(Contact { a: bodies[*a].uid, b: bodies[*b].uid, normal, depth, trigger: bodies[*a].trigger || bodies[*b].trigger });
        }
        contacts.sort_by_key(|c| (c.a, c.b));
    }
    Some(contacts)
}
