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
use crate::hull::Hull;
use crate::layers::{Filter, Layers};
use crate::meshcol::{MeshStore, PlacedMesh};
use crate::physics_rot as rot;
use crate::terrain::{Placed, Terrains};
use lunaris_ecs::{Entity, World};
use std::sync::Arc;

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
        Body { uid: u64::MAX, entity, pos, vel: [0.0; 3], shape: Shape::Sphere(0.0), ext: [0.0; 3], inv_mass: 0.0, restitution: 0.0, friction: 0.0, gravity_scale: 0.0, drag: 0.0, trigger: true, filter: Filter::DEFAULT, spins: false, rich: false, rot: Some(Box::new(rot::Rot { q, axes: rot::axes_of(q), ..rot::Rot::default() })) }
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
pub fn step(world: &mut World, gravity: [f32; 3], dt: f32, terrains: &Terrains, meshes: &MeshStore) -> Vec<Contact> {
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
        return Vec::new();
    }
    raw.sort_by_key(|r| r.0);
    // who collides with whom (a world without a `CollisionLayer` has an empty list and filters nothing)
    let layers = Layers::gather(world);
    // the bodies that turn (a world without any pays for nothing but this query)
    let mut spinners: Vec<(u64, ([f32; 3], f32, [f32; 4]))> = Vec::new();
    world.for_each_uid::<(&AngularVelocity,), _>(|_, uid, (a,)| spinners.push((uid, (a.w, a.damping, a.stash)))).expect("angular velocity query is valid");
    spinners.sort_by_key(|s| s.0);
    let mut bodies: Vec<Body> = Vec::with_capacity(raw.len());
    for (uid, entity, t, c) in raw {
        let scale = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
        let (mut inv_mass, mut restitution, mut friction, mut gravity_scale, mut drag, mut vel) = (0.0, 0.3, 0.5, 1.0, 0.0, [0.0; 3]);
        let mut mass = 0.0;
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
        bodies.push(Body { uid, entity, pos: t.pos, vel, shape, ext, inv_mass, restitution, friction, gravity_scale, drag, trigger: c.trigger, filter: layers.of(uid), spins: spin.is_some(), rich, rot: state });
    }

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
        bodies.push(Body { uid, entity, pos: [placed.centre[0], (lo + hi) * 0.5, placed.centre[1]], vel: [0.0; 3], shape: Shape::Terrain(ground.len() - 1, half), ext: half, inv_mass: 0.0, restitution: g.restitution.clamp(0.0, 1.0), friction: g.friction.max(0.0), gravity_scale: 1.0, drag: 0.0, trigger: false, filter: layers.of(uid), spins: false, rich: false, rot: None });
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
        bodies.push(Body { uid, entity, pos, vel, shape: Shape::Hull(placed_hulls.len() - 1), ext, inv_mass, restitution, friction, gravity_scale, drag, trigger: c.trigger, filter: layers.of(uid), spins: spin.is_some(), rich: true, rot: Some(Box::new(r)) });
    }

    // the mesh colliders: a static body each, whose box is the box around the placed mesh (rich: all its contacts come from `physics_rot`)
    let mut placed_meshes: Vec<Arc<PlacedMesh>> = Vec::with_capacity(mesh_instances.len());
    let mut mesh_filters: Vec<Filter> = Vec::with_capacity(mesh_instances.len());
    for inst in &mesh_instances {
        let (centre, half) = (inst.placed.bounds.centre(), inst.placed.bounds.half());
        placed_meshes.push(inst.placed.clone());
        mesh_filters.push(layers.of(inst.uid));
        bodies.push(Body { uid: inst.uid, entity: inst.entity, pos: centre, vel: [0.0; 3], shape: Shape::Mesh(placed_meshes.len() - 1), ext: half, inv_mass: 0.0, restitution: inst.restitution, friction: inst.friction, gravity_scale: 1.0, drag: 0.0, trigger: false, filter: layers.of(inst.uid), spins: false, rich: true, rot: None });
    }
    if bodies.is_empty() {
        return Vec::new();
    }
    bodies.sort_by_key(|b| b.uid);
    let statics = Statics { ground: &ground, meshes: &placed_meshes, mesh_filters: &mesh_filters, hulls: &placed_hulls, dt };

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

    // fast bodies against mesh colliders: one that went through a thin surface during the last tick is put back against it (`rot::catch_up`)
    if !placed_meshes.is_empty() {
        // (the statics are only read there, the bodies are written)
        rot::catch_up(&mut bodies, &statics);
    }

    // ---- broad phase: sort and sweep along the axis with the largest spread of centres (a tall tower sweeps along y, not x);
    // the set of pairs found does not depend on the axis, and candidate pairs are checked in a canonical order
    let sweep = sweep_axis(&bodies);
    let mut order: Vec<usize> = (0..bodies.len()).collect();
    order.sort_by(|&i, &j| bodies[i].min_on(sweep).total_cmp(&bodies[j].min_on(sweep)).then(bodies[i].uid.cmp(&bodies[j].uid)));
    let mut found: Vec<(usize, usize, [f32; 3], f32)> = Vec::new();
    // contacts that involve a body that turns, an oriented box or a capsule: each has several points (`physics_rot`)
    let mut rich_found: Vec<(usize, usize, Vec<rot::Pt>)> = Vec::new();
    for oi in 0..order.len() {
        let i = order[oi];
        let max_on = bodies[i].max_on(sweep);
        for &j in &order[oi + 1..] {
            if bodies[j].min_on(sweep) > max_on {
                break;
            }
            let (a, b) = if bodies[i].uid < bodies[j].uid { (i, j) } else { (j, i) };
            if bodies[a].inv_mass == 0.0 && bodies[b].inv_mass == 0.0 && !bodies[a].trigger && !bodies[b].trigger {
                continue; // two static colliders never need resolving
            }
            if bodies[a].uid == bodies[b].uid {
                continue; // (one entity with both a Collider and a MeshCollider does not collide with itself)
            }
            if !bodies[a].filter.collides(bodies[b].filter) {
                continue; // (the two collision layers do not both want the other: they pass through each other, triggers included)
            }
            // quick rejection on the other axes
            if (0..3).any(|axis| (bodies[a].pos[axis] - bodies[b].pos[axis]).abs() > bodies[a].extent(axis) + bodies[b].extent(axis)) {
                continue;
            }
            if !pinned.is_empty() && pinned.binary_search(&(bodies[a].uid, bodies[b].uid)).is_ok() {
                continue; // two bodies tied by a ball-and-socket or a hinge do not collide with each other
            }
            if bodies[a].rich || bodies[b].rich {
                let pts = rot::collide(&bodies[a], &bodies[b], &statics);
                if !pts.is_empty() {
                    rich_found.push((a, b, pts));
                }
            } else if let Some((n, depth)) = collide(&bodies[a], &bodies[b], &ground) {
                found.push((a, b, n, depth));
            }
        }
    }
    found.sort_by_key(|f| (bodies[f.0].uid, bodies[f.1].uid));
    rich_found.sort_by_key(|f| (bodies[f.0].uid, bodies[f.1].uid));

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
        pins.extend(rot::make_pin(&bodies, *a, b, j, dt));
    }
    let rounds = if rows.is_empty() && pins.is_empty() { ITERATIONS } else { rot::ITERATIONS };
    for round in 0..rounds {
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
            rot::solve_pin(&mut bodies, p);
        }
        for r in rows.iter_mut() {
            rot::solve_row(&mut bodies, r);
        }
    }

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
    let mut contacts: Vec<Contact> = found.iter().map(|&(a, b, n, depth)| Contact { a: bodies[a].uid, b: bodies[b].uid, normal: n, depth, trigger: bodies[a].trigger || bodies[b].trigger }).collect();
    if !rich_found.is_empty() {
        for (a, b, pts) in &rich_found {
            let (normal, depth) = rot::deepest(pts);
            contacts.push(Contact { a: bodies[*a].uid, b: bodies[*b].uid, normal, depth, trigger: bodies[*a].trigger || bodies[*b].trigger });
        }
        contacts.sort_by_key(|c| (c.a, c.b));
    }
    contacts
}
