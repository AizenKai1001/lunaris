//! Contacts between a placed mesh and the shapes that touch it: a ball, a capsule, an oriented box. Each returns a few [`MeshPt`]s: a
//! point, the direction that pushes the shape off the mesh (the contact normal) and how deep the shape is in.
//!
//! **Two-sided and thin.** A triangle has no inside: a ball is pushed out of whichever side it is on. A ball whose centre went through
//! the surface during the last tick (a thin floor, a fast ball) is pushed back to the side it came from, using how far it moved (`back`),
//! so a ball no faster than about twice its radius per tick cannot slip through a floor made of triangles.
//!
//! **Edges (the "ghost" problem).** A flat floor is many triangles. A ball rolling over the seam between two of them is, for the triangle
//! it is not above, closest to that triangle's *edge*, and the direction from the edge to the ball is slightly tilted: if that tilted
//! push were used, the ball would be nudged sideways at every seam and slow down. So a contact on an edge or a corner of a triangle is
//! thrown away when another triangle that shares that very point is closer to the ball (that triangle's own contact, usually straight
//! along its face, is the true one). Where the ball really touches an edge (the ridge of a roof, the lip of a step) nothing is closer, so
//! the contact stays and its normal points from the edge to the ball. No list of neighbours is stored: the test is made among the
//! triangles the question touched, so it works for any triangle soup, T-junctions included.
//!
//! **Boxes** (and convex hulls) are not done here: [`PlacedMesh::obb`] only turns the box into a flat-faced solid and hands it to
//! `PlacedMesh::poly` in `hull_contact.rs`, which tests it triangle by triangle with the separating-axis idea on four axes (the triangle's
//! own normal and the solid's face normals; edge against edge is left out on purpose: those axes are what makes a box catch on the seams
//! of a flat floor) and gets the contact points by clipping. A box edge that slices across a ridge may be pushed out along a face normal
//! a little farther than strictly needed; it never sticks. See docs/PHYSICS.md.
use crate::meshcol::{add, cross, dot, mul, sub, Aabb, PlacedMesh, V3};

/// One point of contact with a mesh.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshPt {
    /// Where (halfway into the overlap).
    pub p: V3,
    /// The unit direction that pushes the shape away from the mesh.
    pub n: V3,
    /// How deep the shape is in along `n` (a small negative number: not touching yet, but close).
    pub depth: f32,
    /// The push is along the face of a triangle (false: it comes from an edge or a corner of the mesh, where the direction is the one
    /// from that edge to the shape).
    pub face: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Feature {
    Face,
    Edge,
    Vertex,
}

/// The most triangles one ball question looks at (the nearest ones: a level that crams more than this into a ball's reach has no sensible
/// contacts anyway).
const MAX_CANDIDATES: usize = 128;
/// A contact on an edge or corner is thrown away when a triangle that shares its point is nearer by this fraction (of the squared distance).
const GHOST_REL: f32 = 1.0e-4;
/// A point this close (squared metres: 0.1 mm) to a triangle counts as on it.
const ON_TRIANGLE2: f32 = 1.0e-8;
/// Two contact normals that point within about 2.5 degrees of each other are one contact for a ball.
const SAME_NORMAL: f32 = 0.999;
/// A ball has at most this many contacts with different normals (a corner has three walls).
const MAX_NORMALS: usize = 4;
pub(crate) fn unit(a: V3) -> Option<V3> {
    let l2 = dot(a, a);
    (l2.is_finite() && l2 > 1.0e-30).then(|| mul(a, 1.0 / l2.sqrt()))
}

/// The unit normal of a triangle (counter-clockwise is the front); `[0, 1, 0]` for one with no area.
pub(crate) fn tri_normal(a: V3, b: V3, c: V3) -> V3 {
    unit(cross(sub(b, a), sub(c, a))).unwrap_or([0.0, 1.0, 0.0])
}

/// The point of triangle (a, b, c) nearest to `p` (Ericson, Real-Time Collision Detection) and what part of the triangle it is on.
pub(crate) fn closest_on_triangle(p: V3, a: V3, b: V3, c: V3) -> (V3, Feature) {
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, Feature::Vertex);
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return (b, Feature::Vertex);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (add(a, mul(ab, v)), Feature::Edge);
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return (c, Feature::Vertex);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (add(a, mul(ac, w)), Feature::Edge);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (add(b, mul(sub(c, b), w)), Feature::Edge);
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    (add(add(a, mul(ab, v)), mul(ac, w)), Feature::Face)
}

pub(crate) fn point_triangle_d2(p: V3, t: &[V3; 3]) -> f32 {
    let (q, _) = closest_on_triangle(p, t[0], t[1], t[2]);
    let d = sub(p, q);
    dot(d, d)
}

/// Which side of a triangle's plane a ball is on: +1 the side its normal points to, -1 the other. `s` is the centre's signed distance to
/// the plane now, `s_prev` where it was one tick ago: a centre that crossed the plane in between belongs to the side it came from.
pub(crate) fn side_of(s: f32, s_prev: f32) -> f32 {
    if s * s_prev < 0.0 {
        if s_prev > 0.0 {
            1.0
        } else {
            -1.0
        }
    } else if s > 0.0 {
        1.0
    } else if s < 0.0 {
        -1.0
    } else if s_prev < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// How much farther (squared metres) than the nearest triangle found so far the box round another triangle must be before that triangle is left unmeasured: the distances are worked
/// out in `f32` from the same coordinates as the boxes and differ from them by rounding (about 1e-4 of a square metre a few hundred metres from the origin; the margin also grows with the
/// distance from the origin).
const BOX_SLACK2: f32 = 2.0e-3;

/// The box round a triangle.
fn tri_box(tv: &[V3; 3]) -> Aabb {
    let mut b = Aabb::EMPTY;
    for p in tv {
        b.grow(*p);
    }
    b
}

/// The squared distance between two boxes (0 when they touch): never more than that between anything inside one and anything inside the other.
fn box_dist2(a: &Aabb, b: &Aabb) -> f32 {
    let mut d2 = 0.0;
    for k in 0..3 {
        let d = (a.lo[k] - b.hi[k]).max(b.lo[k] - a.hi[k]).max(0.0);
        d2 += d * d;
    }
    d2
}

struct Cand {
    tri: u32,
    q: V3,
    feature: Feature,
    d2: f32,
    tv: [V3; 3],
}

/// The nearest points of two segments (Ericson): the parameters along each (0 to 1) and the squared distance.
pub(crate) fn segment_segment(p1: V3, q1: V3, p2: V3, q2: V3) -> (f32, f32, f32) {
    let (d1, d2) = (sub(q1, p1), sub(q2, p2));
    let r = sub(p1, p2);
    let (a, e, f) = (dot(d1, d1), dot(d2, d2), dot(d2, r));
    let clamp01 = |x: f32| x.max(0.0).min(1.0);
    let (s, t) = if a <= 1.0e-12 && e <= 1.0e-12 {
        (0.0, 0.0)
    } else if a <= 1.0e-12 {
        (0.0, clamp01(f / e))
    } else {
        let c = dot(d1, r);
        if e <= 1.0e-12 {
            (clamp01(-c / a), 0.0)
        } else {
            let b = dot(d1, d2);
            let den = a * e - b * b;
            let mut s = if den > 0.0 { clamp01((b * f - c * e) / den) } else { 0.0 };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = clamp01(-c / a);
            } else if t > 1.0 {
                t = 1.0;
                s = clamp01((b - c) / a);
            }
            (s, t)
        }
    };
    let (c1, c2) = (add(p1, mul(d1, s)), add(p2, mul(d2, t)));
    let d = sub(c1, c2);
    (s, t, dot(d, d))
}

/// The closest approach of the segment `p0`-`p1` to a triangle: how far along the segment (0 to 1) and the squared distance (0 when the
/// segment passes through the triangle).
pub(crate) fn segment_triangle(p0: V3, p1: V3, t: &[V3; 3]) -> (f32, f32) {
    let d = sub(p1, p0);
    let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
    let den = dot(n, d);
    if den != 0.0 {
        let s = dot(n, sub(t[0], p0)) / den;
        if (0.0..=1.0).contains(&s) {
            let x = add(p0, mul(d, s));
            let inside = (0..3).all(|i| dot(cross(sub(t[(i + 1) % 3], t[i]), sub(x, t[i])), n) >= 0.0);
            if inside {
                return (s, 0.0);
            }
        }
    }
    let mut best = (0.0f32, point_triangle_d2(p0, t));
    let at_end = point_triangle_d2(p1, t);
    if at_end < best.1 {
        best = (1.0, at_end);
    }
    for i in 0..3 {
        let (s, _, d2) = segment_segment(p0, p1, t[i], t[(i + 1) % 3]);
        if d2 < best.1 {
            best = (s, d2);
        }
    }
    best
}

impl PlacedMesh {
    /// The contacts of a ball (centre `c`, radius `r`) with the mesh, deepest first, with at most four different normals. `back` is how
    /// far the ball moved during the last tick (its velocity times the step): see the module comment.
    pub fn sphere(&self, c: V3, r: f32, back: V3) -> Vec<MeshPt> {
        let mut out = Vec::new();
        if !(c.iter().all(|v| v.is_finite()) && back.iter().all(|v| v.is_finite()) && r.is_finite() && r > 0.0) {
            return out;
        }
        let r = r.min(1.0e6);
        let r2 = r * r;
        if !(self.bounds.dist2(c) < r2) {
            return out;
        }
        let mut tris: Vec<u32> = Vec::new();
        self.walk(&mut |b: &Aabb| b.dist2(c) < r2, &mut |t| tris.push(t));
        tris.sort_unstable();
        let mut cands: Vec<Cand> = Vec::new();
        for t in tris {
            let tv = self.tri(t);
            let (q, feature) = closest_on_triangle(c, tv[0], tv[1], tv[2]);
            let d = sub(c, q);
            let d2 = dot(d, d);
            if d2 < r2 {
                cands.push(Cand { tri: t, q, feature, d2, tv });
            }
        }
        if cands.len() > MAX_CANDIDATES {
            cands.sort_by(|x, y| x.d2.total_cmp(&y.d2).then(x.tri.cmp(&y.tri)));
            cands.truncate(MAX_CANDIDATES);
            cands.sort_by_key(|x| x.tri);
        }
        // contacts on an edge or a corner that a nearer triangle sharing that point overrules are ghosts
        let keep: Vec<bool> = (0..cands.len())
            .map(|i| {
                let a = &cands[i];
                a.feature == Feature::Face || !cands.iter().enumerate().any(|(j, b)| j != i && b.d2 < a.d2 * (1.0 - GHOST_REL) && point_triangle_d2(a.q, &b.tv) <= ON_TRIANGLE2)
            })
            .collect();
        let mut hits: Vec<(f32, u32, V3, V3, bool)> = Vec::new(); // depth, triangle, normal, point, along a face
        for (cand, keep) in cands.iter().zip(keep) {
            if !keep {
                continue;
            }
            let n = tri_normal(cand.tv[0], cand.tv[1], cand.tv[2]);
            let dist = cand.d2.sqrt();
            // the triangle's own direction, from the side the centre is on (or came from, if it went through during the last tick)
            let on_face = || -> (V3, f32) {
                let s = dot(sub(c, cand.tv[0]), n);
                let side = side_of(s, s - dot(back, n));
                (mul(n, side), r - side * s)
            };
            let (normal, depth, face) = match cand.feature {
                Feature::Face => {
                    let (n, d) = on_face();
                    (n, d, true)
                }
                // a centre right over an edge or a corner of the triangle (the push is along the triangle's normal anyway) is a face contact too
                _ if dist > 1.0e-6 && dot(sub(c, cand.q), n).abs() <= 0.9999 * dist => (mul(sub(c, cand.q), 1.0 / dist), r - dist, false),
                _ => {
                    let (n, d) = on_face();
                    (n, d, true)
                }
            };
            if depth > 0.0 {
                hits.push((depth, cand.tri, normal, sub(c, mul(normal, r - 0.5 * depth)), face));
            }
        }
        hits.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
        for (depth, _, n, p, face) in hits {
            if out.len() >= MAX_NORMALS {
                break;
            }
            if out.iter().all(|k: &MeshPt| dot(k.n, n) < SAME_NORMAL) {
                out.push(MeshPt { p, n, depth, face });
            }
        }
        out
    }

    /// The deepest contact of a ball: the push that moves it out (unit, away from the mesh), how deep it is, and whether it is along a face.
    pub fn sphere_push(&self, c: V3, r: f32) -> Option<(V3, f32, bool)> {
        self.sphere(c, r, [0.0; 3]).first().map(|p| (p.n, p.depth, p.face))
    }

    /// Does a ball lean on the *face* of a triangle that is steeper than `min_up` allows (the cosine of the steepest slope that counts as
    /// ground) but is not a ceiling? Edges and corners do not count: the rim of a ledge is not a slope. The character controller asks this
    /// to tell a step from a slope that is too steep to climb.
    pub fn leans_on_steep_face(&self, c: V3, r: f32, min_up: f32) -> bool {
        if !(c.iter().all(|v| v.is_finite()) && r.is_finite() && r > 0.0) {
            return false;
        }
        let r = r.min(1.0e6);
        let r2 = r * r;
        let mut tris: Vec<u32> = Vec::new();
        self.walk(&mut |b: &Aabb| b.dist2(c) < r2, &mut |t| tris.push(t));
        tris.sort_unstable();
        for t in tris {
            let tv = self.tri(t);
            let (q, feature) = closest_on_triangle(c, tv[0], tv[1], tv[2]);
            let d = sub(c, q);
            let d2 = dot(d, d);
            if feature != Feature::Face || !(d2 < r2) {
                continue;
            }
            let n = tri_normal(tv[0], tv[1], tv[2]);
            let s = dot(sub(c, tv[0]), n);
            let push_y = if s >= 0.0 { n[1] } else { -n[1] };
            if r - s.abs() > 1.0e-4 && push_y > -0.5 && push_y < min_up {
                return true;
            }
        }
        false
    }

    /// The point of the axis `p0`-`p1` of a capsule of radius `r` that comes nearest to the mesh, as a fraction of the way along it, when
    /// it is clearly nearer than both ends are (a pole across a ledge; not a pole lying flat on a floor, where every point is as near).
    ///
    /// Only the triangles that can still change the answer are measured: the box round a triangle is never farther from the axis (or from either end) than the triangle is, so a
    /// triangle whose box is farther than the nearest one found so far (with a margin for rounding, `BOX_SLACK2`) cannot be the nearest and is not looked at. The answer is the very
    /// same as measuring every triangle, bit for bit (`nearest_on_axis_reference` keeps the plain loop; a test compares them on a hundred thousand capsules); a capsule standing
    /// against a facade of a hundred triangles measures a handful.
    pub(crate) fn nearest_on_axis(&self, p0: V3, p1: V3, r: f32) -> Option<f32> {
        let mut axis = Aabb::EMPTY;
        axis.grow(p0);
        axis.grow(p1);
        let seg = axis.grown(r);
        let reach = (0..3).fold(0.0f32, |m, k| m.max(axis.lo[k].abs()).max(axis.hi[k].abs()));
        let slack = BOX_SLACK2 + reach * 4.0e-7;
        let r2 = r * r;
        let mut best: Option<(f32, f32)> = None;
        let mut ends = f32::INFINITY;
        for t in self.triangles_near(&seg) {
            let tv = self.tri(t);
            let tb = tri_box(&tv);
            // (a comparison with a number that is not a number is false: such a triangle is measured)
            let limit = best.map_or(r2, |b| b.1.min(r2));
            if !(box_dist2(&axis, &tb) > limit + slack) {
                let (s, d2) = segment_triangle(p0, p1, &tv);
                if d2 < r * r && best.is_none_or(|b| d2 < b.1) {
                    best = Some((s, d2));
                }
            }
            if !(tb.dist2(p0).min(tb.dist2(p1)) > ends + slack) {
                ends = ends.min(point_triangle_d2(p0, &tv)).min(point_triangle_d2(p1, &tv));
            }
        }
        best.filter(|b| b.1 < ends - 1.0e-5 * (1.0 + ends)).map(|b| b.0)
    }

    /// `nearest_on_axis` by measuring every triangle (what it was before it skipped the ones that cannot matter): the reference its test compares with.
    #[cfg(test)]
    pub(crate) fn nearest_on_axis_reference(&self, p0: V3, p1: V3, r: f32) -> Option<f32> {
        let mut seg = Aabb::EMPTY;
        seg.grow(p0);
        seg.grow(p1);
        let seg = seg.grown(r);
        let mut best: Option<(f32, f32)> = None;
        let mut ends = f32::INFINITY;
        for t in self.triangles_near(&seg) {
            let tv = self.tri(t);
            let (s, d2) = segment_triangle(p0, p1, &tv);
            if d2 < r * r && best.is_none_or(|b| d2 < b.1) {
                best = Some((s, d2));
            }
            ends = ends.min(point_triangle_d2(p0, &tv)).min(point_triangle_d2(p1, &tv));
        }
        best.filter(|b| b.1 < ends - 1.0e-5 * (1.0 + ends)).map(|b| b.0)
    }

    /// The contacts of a capsule (the axis `p0`-`p1`, radius `r`): a ball at each end and one where the axis passes nearest to the mesh
    /// (a pole across a ledge), cut down to at most four points per direction.
    pub fn capsule(&self, p0: V3, p1: V3, r: f32, back: V3) -> Vec<MeshPt> {
        if !(p0.iter().chain(p1.iter()).all(|v| v.is_finite()) && r.is_finite() && r > 0.0) {
            return Vec::new();
        }
        let mut all = self.sphere(p0, r, back);
        if p0 != p1 {
            all.extend(self.sphere(p1, r, back));
            if let Some(s) = self.nearest_on_axis(p0, p1, r.min(1.0e6)) {
                if s > 0.02 && s < 0.98 {
                    all.extend(self.sphere(add(p0, mul(sub(p1, p0), s)), r, back));
                }
            }
        }
        reduce_points(all, 3)
    }

    /// The deepest contact of a capsule: the push that moves it out and how deep it is.
    pub fn capsule_push(&self, p0: V3, p1: V3, r: f32) -> Option<(V3, f32, bool)> {
        self.capsule(p0, p1, r, [0.0; 3]).iter().max_by(|a, b| a.depth.total_cmp(&b.depth)).map(|p| (p.n, p.depth, p.face))
    }

    /// The contacts of an oriented box (centre, its three unit axes in world coordinates, half extents `h`).
    pub fn obb(&self, centre: V3, ax: &[V3; 3], h: V3, back: V3) -> Vec<MeshPt> {
        let finite = |v: &[f32]| v.iter().all(|x| x.is_finite());
        if !(finite(&centre) && finite(&h) && finite(&back) && ax.iter().all(|a| finite(a))) {
            return Vec::new();
        }
        // (a box is a convex solid with six faces: the same code as a hull, `hull_contact.rs`)
        self.poly(&crate::hull_contact::Poly::from_box(centre, ax, h), back)
    }
}

/// Keep the part of a convex polygon where `d(point) >= 0`.
pub(crate) fn clip(poly: &[V3], d: impl Fn(V3) -> f32) -> Vec<V3> {
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


/// Cut a pile of contact points down: group those whose normals agree (within about 11 degrees; at most `max_groups` groups, the deepest
/// points decide which), drop points that are the same place, and keep at most the four of each group that hold a body best (the deepest,
/// then each time the one farthest from those already kept). The order of the result is fixed by the points themselves.
pub(crate) fn reduce_points(mut pts: Vec<MeshPt>, max_groups: usize) -> Vec<MeshPt> {
    if pts.len() <= 1 {
        return pts;
    }
    pts.sort_by(|x, y| y.depth.total_cmp(&x.depth).then(x.p[0].total_cmp(&y.p[0])).then(x.p[1].total_cmp(&y.p[1])).then(x.p[2].total_cmp(&y.p[2])).then(x.n[0].total_cmp(&y.n[0])).then(x.n[1].total_cmp(&y.n[1])).then(x.n[2].total_cmp(&y.n[2])));
    let mut groups: Vec<Vec<MeshPt>> = Vec::new();
    for p in pts {
        match groups.iter().position(|g| dot(g[0].n, p.n) > 0.98) {
            Some(i) => {
                let g = &mut groups[i];
                if !g.iter().any(|k| {
                    let d = sub(k.p, p.p);
                    dot(d, d) < 1.0e-6
                }) {
                    g.push(p);
                }
            }
            None if groups.len() < max_groups => groups.push(vec![p]),
            None => {}
        }
    }
    let mut out = Vec::new();
    for g in groups {
        if g.len() <= 4 {
            out.extend(g);
        } else {
            out.extend(pick_spread(&g).into_iter().map(|i| g[i]));
        }
    }
    out
}

/// Four of the points (sorted deepest first) that hold a body best: the outline of the contact. First one of the deepest (the one farthest
/// from the middle of all the points, so that a flat box does not start from a point in the middle of its face just because rounding made
/// it a hair deeper), then the point farthest from it, then the one farthest from the line between those two, then the one that adds the
/// most area outside the triangle they make (none, if every other point is inside it: three points then hold the body as well).
fn pick_spread(g: &[MeshPt]) -> Vec<usize> {
    let d2 = |a: V3, b: V3| {
        let d = sub(a, b);
        dot(d, d)
    };
    let mut centre = [0.0f32; 3];
    for p in g {
        centre = add(centre, p.p);
    }
    centre = mul(centre, 1.0 / g.len() as f32);
    let deepest = g[0].depth;
    let (mut i0, mut far) = (0usize, -1.0f32);
    for (i, p) in g.iter().enumerate() {
        if p.depth < deepest - 1.0e-3 {
            break;
        }
        let d = d2(p.p, centre);
        if d > far {
            far = d;
            i0 = i;
        }
    }
    let farthest = |from: usize, skip: &[usize]| -> usize {
        let (mut best, mut bi) = (-1.0f32, 0usize);
        for (i, p) in g.iter().enumerate() {
            if skip.contains(&i) {
                continue;
            }
            let d = d2(p.p, g[from].p);
            if d > best {
                best = d;
                bi = i;
            }
        }
        bi
    };
    let i1 = farthest(i0, &[i0]);
    let e = sub(g[i1].p, g[i0].p);
    let (mut best, mut i2) = (-1.0f32, 0usize);
    for (i, p) in g.iter().enumerate() {
        if i == i0 || i == i1 {
            continue;
        }
        let c = cross(e, sub(p.p, g[i0].p));
        let a = dot(c, c);
        if a > best {
            best = a;
            i2 = i;
        }
    }
    let mut picked = vec![i0, i1, i2];
    let tn = cross(e, sub(g[i2].p, g[i0].p));
    let (mut best, mut i3) = (0.0f32, usize::MAX);
    for (i, p) in g.iter().enumerate() {
        if picked.contains(&i) {
            continue;
        }
        for (a, b) in [(i0, i1), (i1, i2), (i2, i0)] {
            let side = dot(cross(sub(g[b].p, g[a].p), sub(p.p, g[a].p)), tn);
            if -side > best {
                best = -side;
                i3 = i;
            }
        }
    }
    if i3 != usize::MAX {
        picked.push(i3);
    }
    picked.sort_unstable();
    picked
}
