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
use std::collections::BTreeSet;

type V3 = [f32; 3];

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

struct Solid {
    uid: u64,
    filter: Filter,
    kind: Kind,
    lo: V3,
    hi: V3,
}

/// The static things a wheel can stand on.
#[derive(Default)]
pub struct RayWorld {
    solids: Vec<Solid>,
    grounds: Vec<(Filter, Ground)>,
    level: Vec<(Filter, Instance)>,
    hulls: Vec<(Filter, HullInstance)>,
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
fn ray_sphere(o: V3, d: V3, c: V3, r: f32) -> Option<(f32, V3)> {
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
fn ray_obb(o: V3, d: V3, c: V3, axes: &M3, h: V3) -> Option<(f32, V3)> {
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
fn ray_capsule(o: V3, d: V3, a: V3, b: V3, r: f32) -> Option<(f32, V3)> {
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
        let layers = Layers::gather(world);
        // what moves is not ground
        let mut movers: BTreeSet<u64> = BTreeSet::new();
        let _ = world.for_each_uid::<(&RigidBody, &Velocity), _>(|_, uid, (rb, _)| {
            if rb.mass > 0.0 {
                movers.insert(uid);
            }
        });
        let _ = world.for_each_uid::<(&CharacterBody,), _>(|_, uid, _| {
            movers.insert(uid);
        });
        let near = |lo: V3, hi: V3| reach.is_none_or(|(rl, rh)| (0..3).all(|a| lo[a] <= rh[a] && hi[a] >= rl[a]));
        let mut solids: Vec<Solid> = Vec::new();
        let mut raw: Vec<(u64, Transform, Collider)> = Vec::new();
        let _ = world.for_each_uid::<(&Transform, &Collider), _>(|_, uid, (t, c)| raw.push((uid, *t, *c)));
        raw.sort_by_key(|r| r.0);
        for (uid, t, c) in raw {
            if c.trigger || movers.contains(&uid) || !t.pos.iter().all(|v| v.is_finite()) {
                continue;
            }
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
            if near(lo, hi) {
                solids.push(Solid { uid, filter: layers.of(uid), kind, lo, hi });
            }
        }
        let mut rw = RayWorld { solids, ..RayWorld::default() };
        for g in grounds(world, fields) {
            let (lo_y, hi_y) = g.placed().y_range();
            let p = g.placed();
            let (lo, hi) = ([p.centre[0] - p.size[0] * 0.5, lo_y, p.centre[1] - p.size[1] * 0.5], [p.centre[0] + p.size[0] * 0.5, hi_y, p.centre[1] + p.size[1] * 0.5]);
            if near(lo, hi) {
                let f = layers.of(g.uid);
                rw.grounds.push((f, g));
            }
        }
        for inst in meshes.instances(world) {
            let b = inst.placed.bounds;
            if near(b.lo, b.hi) {
                let f = layers.of(inst.uid);
                rw.level.push((f, inst));
            }
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

    /// The nearest hit of the ray `o + t d` (`d` a unit vector), `0 <= t <= max`, among what `who` collides with. `skip` is an entity to ignore.
    pub fn cast(&self, o: V3, d: V3, max: f32, who: Filter, skip: u64) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        let mut take = |t: f32, normal: V3, uid: u64| {
            if best.is_none_or(|b| t < b.t) {
                best = Some(Hit { t, normal, uid });
            }
        };
        for s in &self.solids {
            if s.uid == skip || !who.collides(s.filter) || !touches_box(o, d, max, s.lo, s.hi) {
                continue;
            }
            if let Some((t, n)) = s.ray(o, d, max) {
                take(t, n, s.uid);
            }
        }
        for (f, g) in &self.grounds {
            if g.uid != skip && who.collides(*f) {
                if let Some((t, n)) = g.placed().ray(o, d, max) {
                    take(t, n, g.uid);
                }
            }
        }
        for (f, m) in &self.level {
            if m.uid != skip && who.collides(*f) {
                if let Some(h) = m.placed.ray(o, d, max) {
                    take(h.t, h.normal, m.uid);
                }
            }
        }
        for (f, h) in &self.hulls {
            if h.uid != skip && who.collides(*f) {
                if let Some((t, n)) = h.ray(o, d, max) {
                    take(t, n, h.uid);
                }
            }
        }
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
}
