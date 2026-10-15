//! Physics v2, rotation: oriented boxes, capsules, bodies that spin, contact points with angular impulses, ball-and-socket joints and
//! hinges. `physics.rs` calls in here for every contact that involves a body that turns (an `AngularVelocity`) or a shape that is not a
//! sphere or an axis-aligned box (a rotated box, a capsule). Contacts between plain spheres and axis-aligned boxes stay on the old code
//! there, so a scene that does not use rotation keeps its exact bits.
//!
//! Everything is f32 arithmetic with `+ - * /` and `sqrt` in a fixed order (the one angle a hinge limit needs comes from `detmath`), and
//! nothing depends on the order bodies are stored in: the results are the same on every machine and thread count.
//!
//! The contact solver is sequential impulses with accumulated, clamped impulses: each contact point has a normal impulse (never pulls)
//! and two friction impulses (never more than the friction coefficient times the normal one). Nothing is remembered from one tick to the
//! next (a snapshot has to continue exactly like the run it was taken from), so it takes more rounds than the plain contacts do.
use crate::components::Joint;
use crate::hull_contact::{capsule_poly, corners_under, poly_poly, sphere_poly, Poly};
use crate::meshcol::PlacedMesh;
use crate::meshcol_shapes::MeshPt;
use crate::physics::{sphere_box, Body, Shape, Statics, BOUNCE_THRESHOLD, CORRECTION, SLOP};
use crate::terrain::Placed;

pub type V3 = [f32; 3];
/// A body's own x, y and z axes in world coordinates (the columns of its rotation matrix).
pub type M3 = [V3; 3];

pub const IDENTITY: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
pub const IDENTITY_Q: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// What a body that is turned (or that turns) has: its orientation `q` (a unit quaternion) and its own axes in world coordinates, its spin
/// `w` (radians per second about the world axes), the inverse of its moments of inertia about its own axes (zero: it cannot turn), the
/// damping of the spin, and the real velocity of a fast ball that was slowed down to arrive at a wall (`AngularVelocity.stash`).
#[derive(Clone)]
pub struct Rot {
    pub q: [f32; 4],
    pub axes: M3,
    pub w: V3,
    pub inv_i: V3,
    pub spin_damping: f32,
    pub stash: [f32; 4],
}

impl Default for Rot {
    fn default() -> Rot {
        Rot { q: IDENTITY_Q, axes: IDENTITY, w: [0.0; 3], inv_i: [0.0; 3], spin_damping: 0.0, stash: [0.0; 4] }
    }
}

/// Rounds of the velocity solver for contacts and joints that rotate.
pub const ITERATIONS: usize = 20;
/// The spin of a body never exceeds this (radians per second): a faster one turns more in a step than the contacts can follow.
pub const MAX_SPIN: f32 = 60.0;
/// Contact points of two boxes that are this close above each other still count (the solver only stops them closing in).
const MARGIN: f32 = 0.01;
/// Share of a joint's angular error that the velocity solver tries to remove per step.
const JOINT_BIAS: f32 = 0.2;

#[inline]
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
pub fn mul(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
pub fn neg(a: V3) -> V3 {
    [-a[0], -a[1], -a[2]]
}
#[inline]
pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit_or(a: V3, fallback: V3) -> V3 {
    let l2 = dot(a, a);
    if l2 > 1.0e-24 {
        let l = l2.sqrt();
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        fallback
    }
}

/// Two unit vectors that, with `n`, make a right-handed frame (the same two for the same `n` on every machine).
fn basis(n: V3) -> (V3, V3) {
    let h = if n[0].abs() < 0.57 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let t1 = unit_or(cross(n, h), [0.0, 0.0, 1.0]);
    (t1, cross(n, t1))
}

// ------------------------------------------------------------------------------------------------ rotations

pub fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

pub fn quat_conj(q: [f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}

/// The unit quaternion that points the same way as `q` (the identity for a quaternion that is zero or not finite).
pub fn normalize_q(q: [f32; 4]) -> [f32; 4] {
    let n2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if n2.is_finite() && n2 > 1.0e-12 {
        let inv = 1.0 / n2.sqrt();
        [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv]
    } else {
        IDENTITY_Q
    }
}

/// The axes of the body whose orientation is the unit quaternion `q`.
pub fn axes_of(q: [f32; 4]) -> M3 {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let (xx, yy, zz, xy, xz, yz, wx, wy, wz) = (x * x, y * y, z * z, x * y, x * z, y * z, w * x, w * y, w * z);
    [
        [1.0 - 2.0 * (yy + zz), 2.0 * (xy + wz), 2.0 * (xz - wy)],
        [2.0 * (xy - wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz + wx)],
        [2.0 * (xz + wy), 2.0 * (yz - wx), 1.0 - 2.0 * (xx + yy)],
    ]
}

/// A vector given in the body's own coordinates, in world coordinates.
#[inline]
pub fn rotate(m: &M3, v: V3) -> V3 {
    add(add(mul(m[0], v[0]), mul(m[1], v[1])), mul(m[2], v[2]))
}

/// A vector given in world coordinates, in the body's own coordinates.
#[inline]
pub fn unrotate(m: &M3, v: V3) -> V3 {
    [dot(m[0], v), dot(m[1], v), dot(m[2], v)]
}

/// The orientation `q` turned by the spin `w` (radians per second, about the world axes) for `dt` seconds. The same step as the
/// `movement` system takes for `Velocity.ang`.
pub fn spin_q(q: [f32; 4], w: V3, dt: f32) -> [f32; 4] {
    let dq = quat_mul([w[0], w[1], w[2], 0.0], q);
    normalize_q([q[0] + 0.5 * dt * dq[0], q[1] + 0.5 * dt * dq[1], q[2] + 0.5 * dt * dq[2], q[3] + 0.5 * dt * dq[3]])
}

/// A spin from the scene, made safe: zero when it is not a number, at most `MAX_SPIN`.
pub fn clean_spin(w: V3) -> V3 {
    let l2 = dot(w, w);
    if !l2.is_finite() {
        [0.0; 3]
    } else if l2 > MAX_SPIN * MAX_SPIN {
        mul(w, MAX_SPIN / l2.sqrt())
    } else {
        w
    }
}

type Mat3 = [[f32; 3]; 3]; // rows

fn mat_vec(m: &Mat3, v: V3) -> V3 {
    [dot(m[0], v), dot(m[1], v), dot(m[2], v)]
}

fn skew(a: V3) -> Mat3 {
    [[0.0, -a[2], a[1]], [a[2], 0.0, -a[0]], [-a[1], a[0], 0.0]]
}

fn inv3(m: Mat3) -> Option<Mat3> {
    let c00 = m[1][1] * m[2][2] - m[1][2] * m[2][1];
    let c01 = m[1][2] * m[2][0] - m[1][0] * m[2][2];
    let c02 = m[1][0] * m[2][1] - m[1][1] * m[2][0];
    let det = m[0][0] * c00 + m[0][1] * c01 + m[0][2] * c02;
    if !(det.abs() > 1.0e-18) {
        return None;
    }
    let id = 1.0 / det;
    Some([
        [c00 * id, (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * id, (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * id],
        [c01 * id, (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * id, (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * id],
        [c02 * id, (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * id, (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * id],
    ])
}

// ------------------------------------------------------------------------------------------------ mass

/// The inverse of the principal moments of inertia (about the body's own x, y and z axes) of a solid body of mass `m`.
pub fn inv_inertia(shape: &Shape, m: f32) -> V3 {
    let floor = 1.0e-6 * m;
    let inv = |i: f32| 1.0 / i.max(floor);
    match *shape {
        Shape::Sphere(r) => {
            let v = inv(0.4 * m * r * r);
            [v, v, v]
        }
        Shape::Box(h) | Shape::Obb(h) => {
            let (x2, y2, z2) = (h[0] * h[0], h[1] * h[1], h[2] * h[2]);
            [inv(m / 3.0 * (y2 + z2)), inv(m / 3.0 * (x2 + z2)), inv(m / 3.0 * (x2 + y2))]
        }
        Shape::Capsule(r, half) => {
            // a cylinder (length l) and two half-spheres, sharing the mass by volume
            let l = 2.0 * half;
            let (vc, vs) = (r * r * l, 4.0 / 3.0 * r * r * r);
            let mc = if vc + vs > 0.0 { m * vc / (vc + vs) } else { 0.0 };
            let ms = m - mc;
            let r2 = r * r;
            let iy = 0.5 * mc * r2 + 0.4 * ms * r2;
            let ix = mc * (0.25 * r2 + l * l / 12.0) + ms * (0.4 * r2 + 0.25 * l * l + 0.375 * l * r);
            [inv(ix), inv(iy), inv(ix)]
        }
        Shape::Terrain(..) | Shape::Mesh(_) | Shape::Hull(_) => [0.0; 3],
    }
}

#[inline]
fn apply_inv_i(inv_i: V3, axes: &M3, x: V3) -> V3 {
    let (s0, s1, s2) = (inv_i[0] * dot(axes[0], x), inv_i[1] * dot(axes[1], x), inv_i[2] * dot(axes[2], x));
    add(add(mul(axes[0], s0), mul(axes[1], s1)), mul(axes[2], s2))
}

/// The inverse inertia tensor of the body (in world coordinates) applied to `x`.
#[inline]
fn world_inv_i(b: &Body, x: V3) -> V3 {
    match b.rot.as_deref() {
        Some(r) => apply_inv_i(r.inv_i, &r.axes, x),
        None => [0.0; 3],
    }
}

/// Forces that act on the spin of a body, once per tick before the contacts: the gyroscopic term (what makes a tumbling box
/// wobble instead of turning about one fixed axis; Catto's implicit step, so it stays stable), the damping, and the limit.
pub fn spin_forces(b: &mut Body, dt: f32) {
    let Some(r) = b.rot.as_deref_mut() else { return };
    if r.inv_i[0] != r.inv_i[1] || r.inv_i[1] != r.inv_i[2] {
        let inertia = [1.0 / r.inv_i[0], 1.0 / r.inv_i[1], 1.0 / r.inv_i[2]];
        let wb = unrotate(&r.axes, r.w);
        let iw = [inertia[0] * wb[0], inertia[1] * wb[1], inertia[2] * wb[2]];
        let f = mul(cross(wb, iw), dt);
        let (sw, siw) = (skew(wb), skew(iw));
        let mut j: Mat3 = [[0.0; 3]; 3];
        for row in 0..3 {
            for col in 0..3 {
                j[row][col] = (if row == col { inertia[row] } else { 0.0 }) + dt * (sw[row][col] * inertia[col] - siw[row][col]);
            }
        }
        if let Some(inv) = inv3(j) {
            let d = mat_vec(&inv, f);
            r.w = rotate(&r.axes, sub(wb, d));
        }
    }
    let k = 1.0 / (1.0 + r.spin_damping.max(0.0) * dt);
    r.w = clean_spin(mul(r.w, k));
}

// ------------------------------------------------------------------------------------------------ contact points

/// One point where two bodies touch: where, the direction that separates them (from the first body to the second), how deep (a small
/// negative number: not touching yet).
#[derive(Clone, Copy, Debug)]
pub struct Pt {
    pub p: V3,
    pub n: V3,
    pub depth: f32,
}

/// The deepest point of a contact: its normal and depth (what a `Contact` reports and what pushes the bodies apart).
pub fn deepest(pts: &[Pt]) -> (V3, f32) {
    let mut best = pts[0];
    for p in &pts[1..] {
        if p.depth > best.depth {
            best = *p;
        }
    }
    (best.n, best.depth)
}

fn flip(mut v: Vec<Pt>) -> Vec<Pt> {
    for p in v.iter_mut() {
        p.n = neg(p.n);
    }
    v
}

/// Every contact point of two bodies, at least one of which is not a plain sphere or axis-aligned box.
pub(crate) fn collide(a: &Body, b: &Body, st: &Statics) -> Vec<Pt> {
    let ground = st.ground;
    match (a.shape, b.shape) {
        // a mesh against anything solid and static, or against another mesh, never makes contacts of its own: static things do not collide
        (Shape::Mesh(_), Shape::Mesh(_) | Shape::Terrain(..)) | (Shape::Terrain(..), Shape::Mesh(_)) => Vec::new(),
        (Shape::Mesh(k), _) => mesh_contacts(&st.meshes[k], b, st),
        (_, Shape::Mesh(k)) => flip(mesh_contacts(&st.meshes[k], a, st)),
        (Shape::Terrain(..), Shape::Terrain(..)) => Vec::new(),
        (Shape::Terrain(k, _), _) => terrain_contacts(&ground[k], b, st),
        (_, Shape::Terrain(k, _)) => flip(terrain_contacts(&ground[k], a, st)),
        // convex hulls (see `hull_contact.rs`): the contact of a solid with flat faces is the same code whether it is a hull or a box
        (Shape::Hull(k), Shape::Hull(l)) => from_mesh_pts(poly_poly(&hull_poly(st, k, a), &hull_poly(st, l, b))),
        (Shape::Hull(k), Shape::Box(h) | Shape::Obb(h)) => from_mesh_pts(poly_poly(&hull_poly(st, k, a), &Poly::from_box(b.pos, b.axes(), h))),
        (Shape::Box(h) | Shape::Obb(h), Shape::Hull(k)) => from_mesh_pts(poly_poly(&Poly::from_box(a.pos, a.axes(), h), &hull_poly(st, k, b))),
        (Shape::Hull(k), Shape::Sphere(r)) => sphere_poly(&hull_poly(st, k, a), b.pos, r).into_iter().map(|p| Pt { p: p.p, n: p.n, depth: p.depth }).collect(),
        (Shape::Sphere(r), Shape::Hull(k)) => flip(sphere_poly(&hull_poly(st, k, b), a.pos, r).into_iter().map(|p| Pt { p: p.p, n: p.n, depth: p.depth }).collect()),
        (Shape::Hull(k), Shape::Capsule(r, half)) => {
            let (p0, p1) = cap_ends(b, half);
            from_mesh_pts(capsule_poly(&hull_poly(st, k, a), p0, p1, r))
        }
        (Shape::Capsule(r, half), Shape::Hull(k)) => {
            let (p0, p1) = cap_ends(a, half);
            flip(from_mesh_pts(capsule_poly(&hull_poly(st, k, b), p0, p1, r)))
        }
        (Shape::Sphere(ra), Shape::Sphere(rb)) => pair_pt(a.pos, ra, b.pos, rb, [0.0, 1.0, 0.0]).into_iter().collect(),
        (Shape::Sphere(r), Shape::Box(h) | Shape::Obb(h)) => sphere_obb(a.pos, r, b, h).into_iter().collect(),
        (Shape::Box(h) | Shape::Obb(h), Shape::Sphere(r)) => flip(sphere_obb(b.pos, r, a, h).into_iter().collect()),
        (Shape::Sphere(r), Shape::Capsule(cr, ch)) => sphere_capsule(a.pos, r, b, cr, ch).into_iter().collect(),
        (Shape::Capsule(cr, ch), Shape::Sphere(r)) => flip(sphere_capsule(b.pos, r, a, cr, ch).into_iter().collect()),
        (Shape::Box(ha) | Shape::Obb(ha), Shape::Box(hb) | Shape::Obb(hb)) => obb_obb(a, ha, b, hb),
        (Shape::Box(h) | Shape::Obb(h), Shape::Capsule(r, half)) => obb_capsule(a, h, b, r, half),
        (Shape::Capsule(r, half), Shape::Box(h) | Shape::Obb(h)) => flip(obb_capsule(b, h, a, r, half)),
        (Shape::Capsule(ra, ha), Shape::Capsule(rb, hb)) => capsule_capsule(a, ra, ha, b, rb, hb),
    }
}

/// Two balls (centre, radius): the point between them, the normal from the first to the second.
fn pair_pt(pa: V3, ra: f32, pb: V3, rb: f32, fallback: V3) -> Option<Pt> {
    let d = sub(pb, pa);
    let r = ra + rb;
    let d2 = dot(d, d);
    if d2 >= r * r {
        return None;
    }
    let dist = d2.sqrt();
    let n = if dist > 1.0e-6 { [d[0] / dist, d[1] / dist, d[2] / dist] } else { fallback };
    let depth = r - dist;
    Some(Pt { p: add(pa, mul(n, ra - 0.5 * depth)), n, depth })
}

/// A ball at `c` against the oriented box of body `o` (half extents `h`); the normal points from the ball to the box.
fn sphere_obb(c: V3, r: f32, o: &Body, h: V3) -> Option<Pt> {
    let local = unrotate(o.axes(), sub(c, o.pos));
    let (nl, depth) = sphere_box(local, r, [0.0; 3], h)?;
    let n = rotate(o.axes(), nl);
    Some(Pt { p: add(c, mul(n, r - 0.5 * depth)), n, depth })
}

/// The ends of a capsule's axis (the centres of its two half-spheres).
fn cap_ends(c: &Body, half: f32) -> (V3, V3) {
    let axis = c.axes()[1];
    (sub(c.pos, mul(axis, half)), add(c.pos, mul(axis, half)))
}

/// The point of the segment p0 p1 nearest to `p`.
fn seg_point(p0: V3, p1: V3, p: V3) -> V3 {
    let d = sub(p1, p0);
    let l2 = dot(d, d);
    if l2 <= 1.0e-12 {
        return p0;
    }
    let t = (dot(sub(p, p0), d) / l2).max(0.0).min(1.0);
    add(p0, mul(d, t))
}

fn sphere_capsule(c: V3, r: f32, cap: &Body, cr: f32, half: f32) -> Option<Pt> {
    let (p0, p1) = cap_ends(cap, half);
    pair_pt(c, r, seg_point(p0, p1, c), cr, [0.0, 1.0, 0.0])
}

fn capsule_capsule(a: &Body, ra: f32, ha: f32, b: &Body, rb: f32, hb: f32) -> Vec<Pt> {
    let (a0, a1) = cap_ends(a, ha);
    let (b0, b1) = cap_ends(b, hb);
    let (d1, d2) = (sub(a1, a0), sub(b1, b0));
    let (la, lb) = (dot(d1, d1), dot(d2, d2));
    let fallback = unit_or(cross(d1, d2), [0.0, 1.0, 0.0]);
    // parallel axes that overlap along their length touch along a line: a contact point at each end of the overlap
    let cr = cross(d1, d2);
    if la > 1.0e-8 && lb > 1.0e-8 && dot(cr, cr) <= 1.0e-5 * la * lb {
        let len = la.sqrt();
        let u = mul(d1, 1.0 / len);
        let (t0, t1) = (dot(sub(b0, a0), u), dot(sub(b1, a0), u));
        let lo = t0.min(t1).max(0.0);
        let hi = t0.max(t1).min(len);
        if lo <= hi {
            let ends: Vec<f32> = if hi - lo > 1.0e-4 { vec![lo, hi] } else { vec![0.5 * (lo + hi)] };
            let mut out = Vec::new();
            for s in ends {
                let pa = add(a0, mul(u, s));
                out.extend(pair_pt(pa, ra, seg_point(b0, b1, pa), rb, fallback));
            }
            return out;
        }
    }
    // the nearest points of two segments (Ericson, Real-Time Collision Detection)
    let r = sub(a0, b0);
    let (f, c) = (dot(d2, r), dot(d1, r));
    let clamp01 = |x: f32| x.max(0.0).min(1.0);
    let (s, t): (f32, f32) = if la <= 1.0e-12 && lb <= 1.0e-12 {
        (0.0, 0.0)
    } else if la <= 1.0e-12 {
        (0.0, clamp01(f / lb))
    } else if lb <= 1.0e-12 {
        (clamp01(-c / la), 0.0)
    } else {
        let bb = dot(d1, d2);
        let den = la * lb - bb * bb;
        let mut s0 = if den > 0.0 { clamp01((bb * f - c * lb) / den) } else { 0.0 };
        let mut t0 = (bb * s0 + f) / lb;
        if t0 < 0.0 {
            t0 = 0.0;
            s0 = clamp01(-c / la);
        } else if t0 > 1.0 {
            t0 = 1.0;
            s0 = clamp01((bb - c) / la);
        }
        (s0, t0)
    };
    let (pa, pb) = (add(a0, mul(d1, s)), add(b0, mul(d2, t)));
    pair_pt(pa, ra, pb, rb, fallback).into_iter().collect()
}

/// An oriented box (body `o`, half extents `h`) against a capsule (body `c`): the normal points from the box to the capsule. The
/// capsule is a ball swept along a segment: the ball is tried at both ends and at the point of the segment nearest to the box.
fn obb_capsule(o: &Body, h: V3, c: &Body, r: f32, half: f32) -> Vec<Pt> {
    let (p0, p1) = cap_ends(c, half);
    // work in the box's own coordinates, where the box is axis-aligned and centred on the origin
    let (l0, l1) = (unrotate(o.axes(), sub(p0, o.pos)), unrotate(o.axes(), sub(p1, o.pos)));
    let dir = sub(l1, l0);
    let at = |t: f32| add(l0, mul(dir, t));
    let probe = |t: f32| -> Option<Pt> {
        let q = at(t);
        let (nl, depth) = sphere_box(q, r, [0.0; 3], h)?; // ball to box
        let n = rotate(o.axes(), neg(nl)); // box to ball
        let centre = add(o.pos, rotate(o.axes(), q));
        Some(Pt { p: sub(centre, mul(n, r - 0.5 * depth)), n, depth })
    };
    if dot(dir, dir) <= 1.0e-12 {
        return probe(0.0).into_iter().collect();
    }
    // does the axis pass through the box? then the ball in the middle of the part inside is the deepest
    let (mut t0, mut t1, mut inside) = (0.0f32, 1.0f32, true);
    for k in 0..3 {
        if dir[k].abs() < 1.0e-9 {
            if l0[k].abs() > h[k] {
                inside = false;
                break;
            }
        } else {
            let (mut a, mut b) = ((-h[k] - l0[k]) / dir[k], (h[k] - l0[k]) / dir[k]);
            if a > b {
                std::mem::swap(&mut a, &mut b);
            }
            t0 = t0.max(a);
            t1 = t1.min(b);
            if t0 > t1 {
                inside = false;
                break;
            }
        }
    }
    if inside {
        return probe(0.5 * (t0 + t1)).into_iter().collect();
    }
    // otherwise the distance from the box is a convex function of the position along the axis: find its lowest point
    let dist2 = |t: f32| {
        let q = at(t);
        let near = [q[0].max(-h[0]).min(h[0]), q[1].max(-h[1]).min(h[1]), q[2].max(-h[2]).min(h[2])];
        let d = sub(q, near);
        dot(d, d)
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..32 {
        let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
        if dist2(m1) <= dist2(m2) {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    let tm = 0.5 * (lo + hi);
    let mut out = Vec::new();
    out.extend(probe(0.0));
    if tm > 0.02 && tm < 0.98 {
        out.extend(probe(tm));
    }
    out.extend(probe(1.0));
    out
}

/// Keep at most four points: the deepest, then each time the one farthest from those already kept (so the four are spread out).
fn reduce4(c: Vec<Pt>) -> Vec<Pt> {
    if c.len() <= 4 {
        return c;
    }
    let mut first = 0;
    for i in 1..c.len() {
        if c[i].depth > c[first].depth {
            first = i;
        }
    }
    let mut picked: Vec<usize> = vec![first];
    while picked.len() < 4 {
        let mut best = (f32::NEG_INFINITY, 0usize);
        for i in 0..c.len() {
            if picked.contains(&i) {
                continue;
            }
            let mut nearest = f32::INFINITY;
            for &k in &picked {
                let d = sub(c[i].p, c[k].p);
                nearest = nearest.min(dot(d, d));
            }
            if nearest > best.0 {
                best = (nearest, i);
            }
        }
        if best.0 <= 1.0e-10 {
            break; // the rest are the same point again
        }
        picked.push(best.1);
    }
    picked.sort_unstable();
    picked.into_iter().map(|i| c[i]).collect()
}

/// Keep the part of a convex polygon where `d(point) >= 0`.
fn clip(poly: &[V3], d: impl Fn(V3) -> f32) -> Vec<V3> {
    let mut out = Vec::with_capacity(poly.len() + 2);
    let n = poly.len();
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        let (dp, dq) = (d(p), d(q));
        if dp >= 0.0 {
            out.push(p);
        }
        if (dp >= 0.0) != (dq >= 0.0) {
            let t = dp / (dp - dq);
            out.push(add(p, mul(sub(q, p), t)));
        }
    }
    out
}

/// Two oriented boxes (separating axis test; the contact points come from clipping the face that is turned toward the other box, or,
/// when two edges cross, from the nearest points of those edges).
fn obb_obb(a: &Body, ha: V3, b: &Body, hb: V3) -> Vec<Pt> {
    let t = sub(b.pos, a.pos);
    let (aa, ab) = (a.axes(), b.axes());
    // how far apart the two boxes are along the unit vector l (negative: they overlap that much)
    let sep = |l: V3| -> f32 {
        let ra = ha[0] * dot(aa[0], l).abs() + ha[1] * dot(aa[1], l).abs() + ha[2] * dot(aa[2], l).abs();
        let rb = hb[0] * dot(ab[0], l).abs() + hb[1] * dot(ab[1], l).abs() + hb[2] * dot(ab[2], l).abs();
        dot(t, l).abs() - ra - rb
    };
    let mut best_a = (f32::NEG_INFINITY, 0usize);
    let mut best_b = (f32::NEG_INFINITY, 0usize);
    for i in 0..3 {
        let s = sep(aa[i]);
        if s > 0.0 {
            return Vec::new();
        }
        if s > best_a.0 {
            best_a = (s, i);
        }
    }
    for j in 0..3 {
        let s = sep(ab[j]);
        if s > 0.0 {
            return Vec::new();
        }
        if s > best_b.0 {
            best_b = (s, j);
        }
    }
    let mut best_e = (f32::NEG_INFINITY, 0usize, 0usize, [0.0f32; 3]);
    for i in 0..3 {
        for j in 0..3 {
            let c = cross(aa[i], ab[j]);
            let l2 = dot(c, c);
            if l2 < 1.0e-6 {
                continue; // (nearly) parallel edges: the faces cover that case
            }
            let l = mul(c, 1.0 / l2.sqrt());
            let s = sep(l);
            if s > 0.0 {
                return Vec::new();
            }
            if s > best_e.0 {
                best_e = (s, i, j, l);
            }
        }
    }
    let a_is_ref = !(best_b.0 > best_a.0 + 1.0e-3);
    let (face_sep, face_axis) = if a_is_ref { best_a } else { best_b };
    if best_e.0 > face_sep + 1.0e-3 {
        // edge against edge: one contact point, halfway between the nearest points of the two edges
        let (s, i, j, mut l) = best_e;
        if dot(t, l) < 0.0 {
            l = neg(l);
        }
        let mut pa = a.pos;
        for k in 0..3 {
            if k != i {
                pa = add(pa, mul(aa[k], if dot(aa[k], l) > 0.0 { ha[k] } else { -ha[k] }));
            }
        }
        let mut pb = b.pos;
        for k in 0..3 {
            if k != j {
                pb = add(pb, mul(ab[k], if dot(ab[k], l) > 0.0 { -hb[k] } else { hb[k] }));
            }
        }
        let (ua, ub) = (aa[i], ab[j]);
        let r = sub(pa, pb);
        let (bb, c, f) = (dot(ua, ub), dot(ua, r), dot(ub, r));
        let den = 1.0 - bb * bb;
        let (mut sa, mut sb) = (0.0f32, f);
        if den > 1.0e-9 {
            sa = (bb * f - c) / den;
            sb = f + sa * bb;
        }
        sa = sa.max(-ha[i]).min(ha[i]);
        sb = sb.max(-hb[j]).min(hb[j]);
        let (qa, qb) = (add(pa, mul(ua, sa)), add(pb, mul(ub, sb)));
        return vec![Pt { p: mul(add(qa, qb), 0.5), n: l, depth: -s }];
    }
    // face against face: the reference face belongs to one box, the other box shows its most opposed face
    let (rb, rh, ob, oh) = if a_is_ref { (a, ha, b, hb) } else { (b, hb, a, ha) };
    let sgn = if dot(rb.axes()[face_axis], sub(ob.pos, rb.pos)) >= 0.0 { 1.0 } else { -1.0 };
    let nr = mul(rb.axes()[face_axis], sgn); // out of the reference face
    let fc = add(rb.pos, mul(nr, rh[face_axis]));
    let mut m = 0;
    let mut most = -1.0f32;
    for k in 0..3 {
        let d = dot(ob.axes()[k], nr).abs();
        if d > most {
            most = d;
            m = k;
        }
    }
    let inc_sgn = if dot(ob.axes()[m], nr) > 0.0 { -1.0 } else { 1.0 };
    let centre = add(ob.pos, mul(ob.axes()[m], inc_sgn * oh[m]));
    let (m1, m2) = ((m + 1) % 3, (m + 2) % 3);
    let (e1, e2) = (mul(ob.axes()[m1], oh[m1]), mul(ob.axes()[m2], oh[m2]));
    let mut poly = vec![add(add(centre, e1), e2), add(sub(centre, e1), e2), sub(sub(centre, e1), e2), sub(add(centre, e1), e2)];
    for side in [(face_axis + 1) % 3, (face_axis + 2) % 3] {
        let ax = rb.axes()[side];
        for sg in [1.0f32, -1.0] {
            let origin = rb.pos;
            let half = rh[side];
            poly = clip(&poly, |p| half - sg * dot(sub(p, origin), ax));
            if poly.is_empty() {
                return Vec::new();
            }
        }
    }
    let out_n = if a_is_ref { nr } else { neg(nr) };
    let mut pts = Vec::with_capacity(poly.len());
    for v in poly {
        let depth = -dot(sub(v, fc), nr);
        if depth >= -MARGIN {
            pts.push(Pt { p: add(v, mul(nr, 0.5 * depth)), n: out_n, depth });
        }
    }
    reduce4(pts)
}

/// How wide a cell of the ground is (the finer of the two directions).
fn cell_of(g: &Placed) -> f32 {
    (g.size[0] / g.nx as f32).min(g.size[1] / g.nz as f32).max(1.0e-3)
}

/// The contact points of a body with the ground; the normals point from the ground to the body.
fn terrain_contacts(g: &Placed, o: &Body, st: &Statics) -> Vec<Pt> {
    match o.shape {
        // a convex hull: its corners that are under the surface
        Shape::Hull(k) => {
            let poly = hull_poly(st, k, o);
            from_mesh_pts(corners_under(&poly, &|x, z| g.surface(x, z).map(|s| (s, g.normal(x, z)))))
        }
        Shape::Sphere(r) => g.sphere(o.pos, r).map(|(n, d)| Pt { p: sub(o.pos, mul(n, r - 0.5 * d)), n, depth: d }).into_iter().collect(),
        Shape::Capsule(r, half) => {
            // balls along the axis, about one for every cell of the ground
            let steps = if half > 1.0e-4 { ((2.0 * half / cell_of(g)).ceil().max(1.0).min(8.0) as usize) + 1 } else { 1 };
            let axis = o.axes()[1];
            let mut out = Vec::new();
            for i in 0..steps {
                let along = if steps > 1 { -half + 2.0 * half * i as f32 / (steps - 1) as f32 } else { 0.0 };
                let c = add(o.pos, mul(axis, along));
                if let Some((n, d)) = g.sphere(c, r) {
                    out.push(Pt { p: sub(c, mul(n, r - 0.5 * d)), n, depth: d });
                }
            }
            reduce4(out)
        }
        Shape::Box(h) | Shape::Obb(h) => obb_terrain(g, o, h),
        Shape::Terrain(..) | Shape::Mesh(_) => Vec::new(),
    }
}

/// The contact points of a body with a placed mesh; the normals point from the mesh to the body. A ball, a capsule (a segment with a
/// radius: balls at its ends and where it passes nearest) and a box (turned or not) are handled by `meshcol_shapes`. A body's velocity times
/// the step is how far it moved last tick, which lets a mesh tell which side of a thin surface the body came from.
fn mesh_contacts(m: &PlacedMesh, o: &Body, st: &Statics) -> Vec<Pt> {
    let back = mul(o.vel, st.dt);
    let pts = match o.shape {
        Shape::Sphere(r) => m.sphere(o.pos, r, back),
        Shape::Capsule(r, half) => {
            let (p0, p1) = cap_ends(o, half);
            m.capsule(p0, p1, r, back)
        }
        Shape::Box(h) | Shape::Obb(h) => m.obb(o.pos, o.axes(), h, back),
        Shape::Hull(k) => m.poly(&hull_poly(st, k, o), back),
        Shape::Terrain(..) | Shape::Mesh(_) => Vec::new(),
    };
    from_mesh_pts(pts)
}

/// The convex solid that hull `k` of the statics is for body `o` (at its centre of mass, turned to its principal axes).
fn hull_poly<'a>(st: &'a Statics, k: usize, o: &Body) -> Poly<'a> {
    Poly::from_hull(&st.hulls[k], o.pos, o.axes())
}

fn from_mesh_pts(pts: Vec<MeshPt>) -> Vec<Pt> {
    pts.into_iter().map(|p| Pt { p: p.p, n: p.n, depth: p.depth }).collect()
}

/// For each direction a contact pushes in (points whose normals agree within about 25 degrees are one direction): the normal and depth of
/// its deepest point. A body in the corner between a floor and a wall is pushed out of both.
pub fn deepest_per_direction(pts: &[Pt]) -> Vec<(V3, f32)> {
    let mut out: Vec<(V3, f32)> = Vec::new();
    for p in pts {
        match out.iter_mut().find(|(n, _)| dot(*n, p.n) > 0.9) {
            Some(slot) => {
                if p.depth > slot.1 {
                    *slot = (p.n, p.depth);
                }
            }
            None => out.push((p.n, p.depth)),
        }
    }
    out
}

/// An oriented box on the ground: points on all six faces (a grid as fine as the ground's cells) that are under the surface, the
/// four that best hold the box.
fn obb_terrain(g: &Placed, o: &Body, h: V3) -> Vec<Pt> {
    let (lo, hi) = g.y_range();
    let ext_y = o.extent(1);
    if !(o.pos[1] - ext_y <= hi && o.pos[1] + ext_y >= lo) {
        return Vec::new();
    }
    let cell = cell_of(g);
    let grid = |half: f32| (((2.0 * half / cell).ceil().max(0.0).min(6.0) as usize) + 1).clamp(3, 7) | 1;
    let mut cands: Vec<Pt> = Vec::new();
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let (nu, nv) = (grid(h[u]), grid(h[v]));
        for side in [-1.0f32, 1.0] {
            for i in 0..nu {
                for k in 0..nv {
                    let mut local = [0.0f32; 3];
                    local[axis] = side * h[axis];
                    local[u] = -h[u] + 2.0 * h[u] * i as f32 / (nu - 1) as f32;
                    local[v] = -h[v] + 2.0 * h[v] * k as f32 / (nv - 1) as f32;
                    let w = add(o.pos, rotate(o.axes(), local));
                    if let Some(s) = g.surface(w[0], w[2]) {
                        let pen = s - w[1];
                        if pen > 0.0 {
                            let n = g.normal(w[0], w[2]);
                            cands.push(Pt { p: [w[0], w[1] + 0.5 * pen, w[2]], n, depth: pen * n[1].max(0.2) });
                        }
                    }
                }
            }
        }
    }
    reduce4(cands)
}

// ------------------------------------------------------------------------------------------------ fast bodies and meshes

/// How far a body reaches from its centre in the direction `n` (a unit vector).
fn reach_along(b: &Body, n: V3, st: &Statics) -> f32 {
    match b.shape {
        Shape::Sphere(r) => r,
        Shape::Box(h) | Shape::Obb(h) => h[0] * dot(b.axes()[0], n).abs() + h[1] * dot(b.axes()[1], n).abs() + h[2] * dot(b.axes()[2], n).abs(),
        Shape::Capsule(r, half) => r + half * dot(b.axes()[1], n).abs(),
        Shape::Hull(k) => {
            // the corner farthest along `n`, from the centre of mass
            let local = unrotate(b.axes(), n);
            st.hulls[k].verts.iter().fold(0.0f32, |m, v| m.max(dot(*v, local)))
        }
        Shape::Terrain(..) | Shape::Mesh(_) => 0.0,
    }
}

/// A body that moved more than its own size in the last tick (a ball dropped from a height, a shot) may have crossed the thin surface of
/// a mesh between two ticks, where no contact could see it. Before the contacts are found, its path (from where it was a tick ago to where
/// it is) is tested against the triangles; if the path went through one, the body is put back against the side it came from, touching,
/// and its velocity is left alone, so the contact then bounces or stops it with the real speed. Only bodies that are fast for their size
/// are looked at, and only when the world has a mesh collider; spinning balls are additionally slowed ahead of a wall by `sweep_ahead`.
pub(crate) fn catch_up(bodies: &mut [Body], st: &Statics) {
    if st.meshes.is_empty() {
        return;
    }
    for i in 0..bodies.len() {
        let b = &bodies[i];
        if b.inv_mass == 0.0 || b.trigger {
            continue;
        }
        let size = match b.shape {
            Shape::Sphere(r) | Shape::Capsule(r, _) => r,
            Shape::Box(h) | Shape::Obb(h) => h[0].min(h[1]).min(h[2]),
            Shape::Hull(k) => st.hulls[k].inner,
            Shape::Terrain(..) | Shape::Mesh(_) => continue,
        };
        let back = mul(b.vel, st.dt);
        let travel2 = dot(back, back);
        if !(travel2 > size * size) || !travel2.is_finite() {
            continue;
        }
        let travel = travel2.sqrt();
        let dir = mul(back, 1.0 / travel);
        let prev = sub(b.pos, back);
        let mut best: Option<(f32, V3)> = None;
        for (k, m) in st.meshes.iter().enumerate() {
            if !b.filter.collides(st.mesh_filters[k]) {
                continue; // (a level on a layer that this body passes through does not catch it)
            }
            if let Some(h) = m.ray(prev, dir, travel) {
                if best.is_none_or(|x| h.t < x.0) {
                    best = Some((h.t, h.normal));
                }
            }
        }
        let Some((t, n)) = best else { continue };
        let reach = reach_along(&bodies[i], n, st);
        let hit = add(prev, mul(dir, t));
        // (a hair inside the surface, so that the contact is found and pushes with the body's real velocity)
        bodies[i].pos = add(hit, mul(n, reach * 0.98));
    }
}

// ------------------------------------------------------------------------------------------------ fast balls

/// Is the ball (`probe`, moved to `start + step * t`) touching `other`?
fn touches(probe: &mut Body, start: V3, step: V3, t: f32, other: &Body, st: &Statics) -> bool {
    probe.pos = add(start, mul(step, t));
    !collide(probe, other, st).is_empty()
}

/// Continuous collision for spinning balls (`AngularVelocity`): a ball that moves more than its radius in a step could pass through a thin
/// wall or floor between two ticks, where no tick sees it touch. After the tick is solved, each such ball's next step (its velocity times
/// the step) is tried against every solid static body it is not touching yet: if it would arrive at one, its velocity is cut to the share of
/// the step that ends exactly at the surface (the real velocity goes to `stash`, and the next tick starts from that, so the bounce is the bounce
/// of the real speed at the real place). The tests along the step are a radius apart, which is enough to find any wall however thin, and
/// the first touch is narrowed down by halving.
pub(crate) fn sweep_ahead(bodies: &mut [Body], ground: &Statics, dt: f32) {
    for i in 0..bodies.len() {
        let r = match bodies[i].shape {
            Shape::Sphere(r) if bodies[i].spins && bodies[i].inv_mass > 0.0 && r > 0.0 => r,
            _ => continue,
        };
        let v = bodies[i].vel;
        let step = mul(v, dt);
        let travel2 = dot(step, step);
        if !(travel2 > r * r) || !travel2.is_finite() {
            continue; // (a ball that moves less than its radius is seen by the contacts; so is nonsense)
        }
        let tests = ((travel2.sqrt() / r).ceil().min(64.0) as usize).max(2);
        let start = bodies[i].pos;
        let end = add(start, step);
        // the solid static bodies near the path
        let near: Vec<usize> = (0..bodies.len())
            .filter(|&k| {
                k != i
                    && bodies[k].inv_mass == 0.0
                    && !bodies[k].trigger
                    && bodies[i].filter.collides(bodies[k].filter)
                    && (0..3).all(|a| bodies[k].max_on(a) >= start[a].min(end[a]) - r && bodies[k].min_on(a) <= start[a].max(end[a]) + r)
            })
            .collect();
        if near.is_empty() {
            continue;
        }
        let mut probe = bodies[i].clone();
        let mut best = 1.0f32;
        for &k in &near {
            if touches(&mut probe, start, step, 0.0, &bodies[k], ground) {
                continue; // already in contact with it: the contacts deal with that one
            }
            let mut clear = 0.0f32;
            for j in 1..=tests {
                let t = j as f32 / tests as f32;
                if t >= best {
                    break;
                }
                if touches(&mut probe, start, step, t, &bodies[k], ground) {
                    let (mut a, mut b) = (clear, t);
                    for _ in 0..10 {
                        let m = 0.5 * (a + b);
                        if touches(&mut probe, start, step, m, &bodies[k], ground) {
                            b = m;
                        } else {
                            a = m;
                        }
                    }
                    best = best.min(b);
                    break;
                }
                clear = t;
            }
        }
        if best < 1.0 {
            if let Some(r) = bodies[i].rot.as_deref_mut() {
                r.stash = [v[0], v[1], v[2], best];
            }
            bodies[i].vel = mul(v, best);
        }
    }
}

// ------------------------------------------------------------------------------------------------ the contact solver

/// A contact point, ready to solve: the arms from the two centres, the directions, the effective masses and the impulses so far.
pub struct Row {
    a: usize,
    b: usize,
    ra: V3,
    rb: V3,
    n: V3,
    t1: V3,
    t2: V3,
    mass_n: f32,
    mass_1: f32,
    mass_2: f32,
    /// The speed along the normal that the solver aims for: the bounce, or a closing speed that still leaves the gap shut.
    target: f32,
    mu: f32,
    jn: f32,
    j1: f32,
    j2: f32,
}

/// Two different bodies of the list, both mutable.
pub fn pair_mut(bodies: &mut [Body], a: usize, b: usize) -> (&mut Body, &mut Body) {
    if a < b {
        let (l, r) = bodies.split_at_mut(b);
        (&mut l[a], &mut r[0])
    } else {
        let (l, r) = bodies.split_at_mut(a);
        (&mut r[0], &mut l[b])
    }
}

#[inline]
fn point_vel(b: &Body, r: V3) -> V3 {
    add(b.vel, cross(b.w(), r))
}

/// Push `-p` into body `ba` and `+p` into `bb`, at the points `ra` and `rb` from their centres.
#[inline]
fn impulse(ba: &mut Body, bb: &mut Body, ra: V3, rb: V3, p: V3) {
    ba.vel = sub(ba.vel, mul(p, ba.inv_mass));
    let (da, db) = (world_inv_i(ba, cross(ra, p)), world_inv_i(bb, cross(rb, p)));
    if let Some(r) = ba.rot.as_deref_mut() {
        r.w = sub(r.w, da);
    }
    bb.vel = add(bb.vel, mul(p, bb.inv_mass));
    if let Some(r) = bb.rot.as_deref_mut() {
        r.w = add(r.w, db);
    }
}

/// Turn the two bodies against each other: `-l` into `ba`'s spin and `+l` into `bb`'s (an angular impulse).
#[inline]
fn turn(ba: &mut Body, bb: &mut Body, l: V3) {
    let (da, db) = (world_inv_i(ba, l), world_inv_i(bb, l));
    if let Some(r) = ba.rot.as_deref_mut() {
        r.w = sub(r.w, da);
    }
    if let Some(r) = bb.rot.as_deref_mut() {
        r.w = add(r.w, db);
    }
}

pub fn prepare_row(bodies: &[Body], a: usize, b: usize, pt: &Pt, dt: f32) -> Row {
    let (ba, bb) = (&bodies[a], &bodies[b]);
    let (ra, rb) = (sub(pt.p, ba.pos), sub(pt.p, bb.pos));
    let n = pt.n;
    let (t1, t2) = basis(n);
    let effective = |d: V3| -> f32 {
        let (xa, xb) = (cross(ra, d), cross(rb, d));
        let k = ba.inv_mass + bb.inv_mass + dot(xa, world_inv_i(ba, xa)) + dot(xb, world_inv_i(bb, xb));
        if k > 1.0e-12 {
            1.0 / k
        } else {
            0.0
        }
    };
    let vn = dot(sub(point_vel(bb, rb), point_vel(ba, ra)), n);
    let e = if -vn < BOUNCE_THRESHOLD { 0.0 } else { ba.restitution.max(bb.restitution) };
    let target = if pt.depth < 0.0 { pt.depth / dt } else { -e * vn.min(0.0) };
    Row { a, b, ra, rb, n, t1, t2, mass_n: effective(n), mass_1: effective(t1), mass_2: effective(t2), target, mu: (ba.friction * bb.friction).sqrt(), jn: 0.0, j1: 0.0, j2: 0.0 }
}

/// A first guess for the impulses of the contacts that hold things up, made from the weights alone (the solver remembers nothing from the
/// last tick, and a stack of boxes would take hundreds of rounds from zero). A contact whose normal points (within 60 degrees) against
/// gravity carries the weight of the upper body and of whatever rests on that body, shared among the contacts that body has with
/// bodies below it. Each point gets its share of that weight as a normal impulse, and of the part of the weight along the surface as a
/// friction impulse (so a box resting on a slope starts at rest). A wrong guess costs nothing: the rounds that follow take back what
/// is too much.
pub fn warm_start(bodies: &mut [Body], rows: &mut [Row], gravity: V3, dt: f32) {
    let g2 = dot(gravity, gravity);
    if rows.is_empty() || !(g2 > 1.0e-12) {
        return;
    }
    let gl = g2.sqrt();
    let up = mul(gravity, -1.0 / gl);
    let height = |bodies: &[Body], k: usize| dot(bodies[k].pos, up);
    // each contact (the rows of one pair follow each other): who is under whom
    struct Support {
        first: usize,
        count: usize,
        below: usize,
        above: usize,
        cos: f32,
        carried: f32,
    }
    let mut pairs: Vec<Support> = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let (a, b) = (rows[i].a, rows[i].b);
        // (a body in the corner between a floor and a wall has contact points with different normals against one mesh: the guess is made
        // for each direction separately, so the wall does not get a share of the weight)
        let against_mesh = matches!(bodies[a].shape, Shape::Mesh(_)) || matches!(bodies[b].shape, Shape::Mesh(_));
        let mut j = i + 1;
        while j < rows.len() && rows[j].a == a && rows[j].b == b && (!against_mesh || dot(rows[j].n, rows[i].n) > 0.9) {
            j += 1;
        }
        let c = dot(rows[i].n, up);
        if c > 0.5 && height(bodies, a) < height(bodies, b) {
            pairs.push(Support { first: i, count: j - i, below: a, above: b, cos: c, carried: 0.0 });
        } else if c < -0.5 && height(bodies, b) < height(bodies, a) {
            pairs.push(Support { first: i, count: j - i, below: b, above: a, cos: -c, carried: 0.0 });
        }
        i = j;
    }
    if pairs.is_empty() {
        return;
    }
    // the weight each body carries, worked out from the top down: its own mass, plus what it is given by the bodies above it
    let mut load: Vec<f32> = bodies.iter().map(|b| if b.inv_mass > 0.0 { 1.0 / b.inv_mass } else { 0.0 }).collect();
    let mut points = vec![0usize; bodies.len()];
    for p in &pairs {
        points[p.above] += p.count;
    }
    let mut order: Vec<usize> = (0..pairs.len()).collect();
    order.sort_by(|&x, &y| {
        let (hx, hy) = (height(bodies, pairs[x].above), height(bodies, pairs[y].above));
        hy.total_cmp(&hx).then(bodies[pairs[x].above].uid.cmp(&bodies[pairs[y].above].uid)).then(bodies[pairs[x].below].uid.cmp(&bodies[pairs[y].below].uid))
    });
    for &k in &order {
        let (above, below, count) = (pairs[k].above, pairs[k].below, pairs[k].count);
        let carried = load[above] * count as f32 / points[above] as f32;
        pairs[k].carried = carried;
        if bodies[below].inv_mass > 0.0 {
            load[below] += carried;
        }
    }
    for p in &pairs {
        if bodies[p.above].inv_mass == 0.0 {
            continue;
        }
        let n = rows[p.first].n;
        let jn = p.carried * gl * dt * p.cos / p.count as f32;
        // what the surface must push the upper body with along itself to hold it: against the part of gravity that lies along it
        let gt = sub(gravity, mul(n, dot(gravity, n)));
        let need = mul(gt, -p.carried * dt / p.count as f32);
        let upper_is_b = p.above == rows[p.first].b;
        let push = if upper_is_b { need } else { neg(need) };
        for r in &mut rows[p.first..p.first + p.count] {
            let limit = r.mu * jn;
            r.jn = jn;
            r.j1 = dot(push, r.t1).max(-limit).min(limit);
            r.j2 = dot(push, r.t2).max(-limit).min(limit);
            let total = add(add(mul(r.n, r.jn), mul(r.t1, r.j1)), mul(r.t2, r.j2));
            let (ba, bb) = pair_mut(bodies, r.a, r.b);
            impulse(ba, bb, r.ra, r.rb, total);
        }
    }
}

pub fn solve_row(bodies: &mut [Body], r: &mut Row) {
    let (ba, bb) = pair_mut(bodies, r.a, r.b);
    // friction first (the normal impulse of the last round limits it), then the contact itself, which matters more
    let limit = r.mu * r.jn;
    let vt = dot(sub(point_vel(bb, r.rb), point_vel(ba, r.ra)), r.t1);
    let new = (r.j1 - vt * r.mass_1).max(-limit).min(limit);
    let l = new - r.j1;
    r.j1 = new;
    impulse(ba, bb, r.ra, r.rb, mul(r.t1, l));
    let vt = dot(sub(point_vel(bb, r.rb), point_vel(ba, r.ra)), r.t2);
    let new = (r.j2 - vt * r.mass_2).max(-limit).min(limit);
    let l = new - r.j2;
    r.j2 = new;
    impulse(ba, bb, r.ra, r.rb, mul(r.t2, l));
    let vn = dot(sub(point_vel(bb, r.rb), point_vel(ba, r.ra)), r.n);
    let new = (r.jn + (r.target - vn) * r.mass_n).max(0.0);
    let l = new - r.jn;
    r.jn = new;
    impulse(ba, bb, r.ra, r.rb, mul(r.n, l));
}

// ------------------------------------------------------------------------------------------------ ball-and-socket joints and hinges

/// A ball-and-socket (`Joint.kind` 3) or a hinge (4) between two bodies, ready to solve. `b` may be a frame that never moves (the
/// world, or an entity that is only a `Transform`).
pub struct Pin {
    pub a: usize,
    pub b: usize,
    hinge: bool,
    pub local_a: V3,
    pub local_b: V3,
    pub local_axis: V3,
    pub rest: [f32; 4],
    /// Bound just now: the joint component is to get `local_a`, `local_b`, `local_axis` and `rest` written back.
    pub fresh: bool,
    dt: f32,
    ra: V3,
    rb: V3,
    kinv: Mat3,
    u: V3,
    t1: V3,
    t2: V3,
    kang: [[f32; 2]; 2],
    ang_ok: bool,
    bias: [f32; 2],
    limit: Option<(f32, f32)>,
    theta: f32,
    axis_mass: f32,
    acc_lo: f32,
    acc_hi: f32,
}

/// The angle (radians) that body `b` has turned about the hinge axis relative to body `a`, since the joint was bound.
fn hinge_angle(qa: [f32; 4], qb: [f32; 4], rest: [f32; 4], local_axis: V3) -> f32 {
    let q = quat_mul(quat_conj(qa), qb);
    let d = quat_mul(q, quat_conj(rest));
    let (mut v, mut w) = ([d[0], d[1], d[2]], d[3]);
    if w < 0.0 {
        v = neg(v);
        w = -w;
    }
    (2.0 * crate::detmath::atan2(dot(v, local_axis) as f64, w as f64)) as f32
}

/// Set up the joint of body `a` to body `b` for this tick; binds it to the two bodies the first time (`rest` all zero).
pub fn make_pin(bodies: &[Body], a: usize, b: usize, j: &Joint, dt: f32) -> Option<Pin> {
    let finite = |v: &[f32]| v.iter().all(|x| x.is_finite());
    if !(finite(&j.anchor) && finite(&j.axis) && finite(&j.limit) && finite(&j.local_a) && finite(&j.local_b) && finite(&j.local_axis) && finite(&j.rest)) {
        return None;
    }
    let (ba, bb) = (&bodies[a], &bodies[b]);
    let fresh = j.rest.iter().all(|v| *v == 0.0);
    let (local_a, local_b, local_axis, rest) = if fresh {
        let axis = unit_or(j.axis, [0.0, 1.0, 0.0]);
        (unrotate(ba.axes(), sub(j.anchor, ba.pos)), unrotate(bb.axes(), sub(j.anchor, bb.pos)), unrotate(ba.axes(), axis), quat_mul(quat_conj(ba.q()), bb.q()))
    } else {
        (j.local_a, j.local_b, j.local_axis, j.rest)
    };
    let hinge = j.kind == 4;
    let (ra, rb) = (rotate(ba.axes(), local_a), rotate(bb.axes(), local_b));
    // the pivot: K maps an impulse at the pivot to the change of the relative velocity of the two points that must stay together
    let inv_m = ba.inv_mass + bb.inv_mass;
    let mut k: Mat3 = [[0.0; 3]; 3];
    for c in 0..3 {
        let mut e = [0.0f32; 3];
        e[c] = 1.0;
        let col = add(add(mul(e, inv_m), cross(world_inv_i(bb, cross(rb, e)), rb)), cross(world_inv_i(ba, cross(ra, e)), ra));
        for r in 0..3 {
            k[r][c] = col[r];
        }
    }
    let kinv = inv3(k)?;
    let mut p = Pin { a, b, hinge, local_a, local_b, local_axis, rest, fresh, dt, ra, rb, kinv, u: [0.0, 1.0, 0.0], t1: [1.0, 0.0, 0.0], t2: [0.0, 0.0, 1.0], kang: [[0.0; 2]; 2], ang_ok: false, bias: [0.0; 2], limit: None, theta: 0.0, axis_mass: 0.0, acc_lo: 0.0, acc_hi: 0.0 };
    if hinge {
        // the axis seen from body a, and from body b (which has turned as the rest pose says); the two must stay the same line
        let u = unit_or(rotate(ba.axes(), local_axis), [0.0, 1.0, 0.0]);
        let axis_in_b = rotate(&axes_of(quat_conj(rest)), local_axis);
        let ub = rotate(bb.axes(), axis_in_b);
        let phi = cross(ub, u);
        let (t1, t2) = basis(u);
        let (m1, m2) = (add(world_inv_i(ba, t1), world_inv_i(bb, t1)), add(world_inv_i(ba, t2), world_inv_i(bb, t2)));
        let (k00, k01, k11) = (dot(t1, m1), dot(t1, m2), dot(t2, m2));
        let det = k00 * k11 - k01 * k01;
        if det > 1.0e-12 {
            p.kang = [[k11 / det, -k01 / det], [-k01 / det, k00 / det]];
            p.ang_ok = true;
        }
        let s = JOINT_BIAS / dt;
        p.bias = [s * dot(phi, t1), s * dot(phi, t2)];
        p.u = u;
        p.t1 = t1;
        p.t2 = t2;
        if j.limit[0] < j.limit[1] {
            // (the limits are for the angle of the body relative to its target; `theta` is the other way round)
            p.limit = Some((-j.limit[1], -j.limit[0]));
            p.theta = hinge_angle(ba.q(), bb.q(), rest, local_axis);
            let am = dot(u, add(world_inv_i(ba, u), world_inv_i(bb, u)));
            p.axis_mass = if am > 1.0e-12 { 1.0 / am } else { 0.0 };
        }
    }
    Some(p)
}

pub fn solve_pin(bodies: &mut [Body], p: &mut Pin) {
    let (ba, bb) = pair_mut(bodies, p.a, p.b);
    // the two pivot points move together
    let vrel = sub(point_vel(bb, p.rb), point_vel(ba, p.ra));
    let imp = mat_vec(&p.kinv, neg(vrel));
    impulse(ba, bb, p.ra, p.rb, imp);
    if !p.hinge {
        return;
    }
    // the two bodies turn about the axis only: no relative spin about the two directions across it
    if p.ang_ok {
        let wrel = sub(bb.w(), ba.w());
        let (r1, r2) = (p.bias[0] - dot(wrel, p.t1), p.bias[1] - dot(wrel, p.t2));
        let (l1, l2) = (p.kang[0][0] * r1 + p.kang[0][1] * r2, p.kang[1][0] * r1 + p.kang[1][1] * r2);
        turn(ba, bb, add(mul(p.t1, l1), mul(p.t2, l2)));
    }
    if let Some((lo, hi)) = p.limit {
        // the angle may not leave [lo, hi]: a one-sided limit on the relative spin about the axis, which lets the angle close in on
        // the limit and no further (and pushes back when it is already past)
        let dt = p.dt;
        let c_lo = p.theta - lo;
        let target = if c_lo >= 0.0 { -c_lo / dt } else { -JOINT_BIAS * c_lo / dt };
        let spin = dot(sub(bb.w(), ba.w()), p.u);
        let new = (p.acc_lo + (target - spin) * p.axis_mass).max(0.0);
        let l = new - p.acc_lo;
        p.acc_lo = new;
        turn(ba, bb, mul(p.u, l));
        let c_hi = hi - p.theta;
        let target = if c_hi >= 0.0 { -c_hi / dt } else { -JOINT_BIAS * c_hi / dt };
        let spin = dot(sub(bb.w(), ba.w()), p.u);
        let new = (p.acc_hi + (target + spin) * p.axis_mass).max(0.0);
        let l = new - p.acc_hi;
        p.acc_hi = new;
        turn(ba, bb, mul(p.u, -l));
    }
}

/// Pull the two pivot points together (once per tick, after the velocities are solved, like the contacts).
pub fn correct_pin(bodies: &mut [Body], p: &Pin) {
    let (ba, bb) = pair_mut(bodies, p.a, p.b);
    let wa = add(ba.pos, rotate(ba.axes(), p.local_a));
    let wb = add(bb.pos, rotate(bb.axes(), p.local_b));
    let e = sub(wa, wb);
    let d2 = dot(e, e);
    let inv_sum = ba.inv_mass + bb.inv_mass;
    if !(d2 > SLOP * SLOP) || inv_sum == 0.0 {
        return;
    }
    let dist = d2.sqrt();
    let corr = mul(e, (dist - SLOP) / dist * CORRECTION);
    ba.pos = sub(ba.pos, mul(corr, ba.inv_mass / inv_sum));
    bb.pos = add(bb.pos, mul(corr, bb.inv_mass / inv_sum));
}
