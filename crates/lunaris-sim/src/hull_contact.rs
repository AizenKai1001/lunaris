//! Contacts of convex solids (`ConvexCollider`, see `hull.rs`): a hull against a box, another hull, a ball, a capsule and the triangles of a
//! mesh collider. A box is handled as a hull of eight corners and six faces, so there is one code for all the solids with flat faces.
//!
//! **Hull against hull** is the separating axis test of Gregorius (GDC 2013): the faces of each against the corners of the other, and the pairs
//! of edges that make a face of the Minkowski difference (the Gauss-map test keeps it to the few pairs that matter). If a face axis separates the
//! two, there is no contact. Otherwise the least overlap decides: a **face contact** (one solid's face against the most opposed face of the other,
//! clipped to the first face's outline: up to four points that keep a box flat on a floor) or, when two edges cross and that is clearly the
//! shallowest way out, an **edge contact** (one point). Edge contacts need a margin to win (2 mm), because a stack of faces is steadier than a
//! tip balanced on an edge. **Accuracy limits:** a solid pushed deeper than its own thickness in one tick may be pushed out the wrong way (use
//! smaller steps or the catch-up for fast bodies); a face that is very nearly parallel to another and slightly bent may give a contact patch
//! that is a bit small.
//!
//! **Ball and capsule against a hull**: the nearest point of the hull to the centre decides (a centre inside leaves through the nearest face); a
//! capsule is a ball at each end plus one where its axis passes nearest. **A hull against the triangles of a mesh**: each triangle near it is
//! tried like a box is (the triangle's own normal and the hull's face normals as axes; no edge against edge, which is what makes things
//! catch on the seams of a flat floor), and the points come from clipping.
use crate::hull::{Hull, HullEdge};
use crate::meshcol::{add, cross, dot, mul, sub, Aabb, PlacedMesh, V3};
use crate::meshcol_shapes::{clip, closest_on_triangle, reduce_points, segment_segment, unit, MeshPt};
use std::sync::OnceLock;

/// Contact points this far above the surface still count (the solver only stops them closing in).
const MARGIN: f32 = 0.01;
/// How much shallower than the best face an edge contact must be to be chosen.
const EDGE_BIAS: f32 = 0.002;
/// Two face overlaps this close count as equal (the first solid's face then wins).
const FACE_TIE: f32 = 0.001;

fn neg(a: V3) -> V3 {
    [-a[0], -a[1], -a[2]]
}

/// A convex solid with flat faces standing in the world: corners, outward face normals and planes (`dot(n, x) = d`), the corners of each face in
/// counter-clockwise order seen from outside, and the edges with the two faces next to each.
pub(crate) struct Poly<'a> {
    pub centre: V3,
    pub v: Vec<V3>,
    pub n: Vec<V3>,
    pub d: Vec<f32>,
    pub faces: Vec<&'a [u16]>,
    pub edges: &'a [HullEdge],
}

struct BoxTopology {
    faces: Vec<Vec<u16>>,
    edges: Vec<HullEdge>,
}

/// The edges of a closed solid given its faces: each edge once, with the face that runs along it each way.
fn edges_of(faces: &[Vec<u16>]) -> Vec<HullEdge> {
    let mut by_edge: std::collections::BTreeMap<(u16, u16), u16> = std::collections::BTreeMap::new();
    for (fi, f) in faces.iter().enumerate() {
        for k in 0..f.len() {
            by_edge.insert((f[k], f[(k + 1) % f.len()]), fi as u16);
        }
    }
    let mut out = Vec::new();
    for (&(a, b), &fa) in &by_edge {
        if a < b {
            if let Some(&fb) = by_edge.get(&(b, a)) {
                out.push(HullEdge { a, b, fa, fb });
            }
        }
    }
    out
}

/// A box's corners are numbered by their signs: bit 0 is +x, bit 1 +y, bit 2 +z. Its faces are +x, -x, +y, -y, +z, -z.
fn box_topology() -> &'static BoxTopology {
    static BOX: OnceLock<BoxTopology> = OnceLock::new();
    BOX.get_or_init(|| {
        let faces: Vec<Vec<u16>> = vec![vec![1, 3, 7, 5], vec![0, 4, 6, 2], vec![2, 6, 7, 3], vec![0, 1, 5, 4], vec![4, 5, 7, 6], vec![0, 2, 3, 1]];
        let edges = edges_of(&faces);
        BoxTopology { faces, edges }
    })
}

impl<'a> Poly<'a> {
    /// A hull whose principal frame has its centre at `pos` and its axes `axes` (the columns of the rotation) in the world.
    pub fn from_hull(h: &'a Hull, pos: V3, axes: &[V3; 3]) -> Poly<'a> {
        let rot = |p: V3| add(add(mul(axes[0], p[0]), mul(axes[1], p[1])), mul(axes[2], p[2]));
        let v: Vec<V3> = h.verts.iter().map(|p| add(pos, rot(*p))).collect();
        let n: Vec<V3> = h.faces.iter().map(|f| rot(f.n)).collect();
        let d: Vec<f32> = h.faces.iter().zip(&n).map(|(f, nw)| f.d + dot(*nw, pos)).collect();
        Poly { centre: pos, v, n, d, faces: h.faces.iter().map(|f| f.verts.as_slice()).collect(), edges: &h.edges }
    }

    /// A box with half sizes `h` about `pos`, turned by `axes`.
    pub fn from_box(pos: V3, axes: &[V3; 3], h: V3) -> Poly<'static> {
        let topo = box_topology();
        let s = |bit: usize, i: usize| if (i >> bit) & 1 == 1 { 1.0f32 } else { -1.0 };
        let v: Vec<V3> = (0..8).map(|i| add(pos, add(add(mul(axes[0], s(0, i) * h[0]), mul(axes[1], s(1, i) * h[1])), mul(axes[2], s(2, i) * h[2])))).collect();
        let mut n = Vec::with_capacity(6);
        let mut d = Vec::with_capacity(6);
        for k in 0..3 {
            for sg in [1.0f32, -1.0] {
                let nw = mul(axes[k], sg);
                d.push(dot(nw, pos) + h[k]);
                n.push(nw);
            }
        }
        Poly { centre: pos, v, n, d, faces: topo.faces.iter().map(|f| f.as_slice()).collect(), edges: &topo.edges }
    }

    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for p in &self.v {
            b.grow(*p);
        }
        b
    }

    /// The interval of the corners' positions along a direction.
    fn extent(&self, dir: V3) -> (f32, f32) {
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for p in &self.v {
            let x = dot(*p, dir);
            lo = lo.min(x);
            hi = hi.max(x);
        }
        (lo, hi)
    }
}

// (The test of Gregorius for two edges to make a face of the Minkowski difference: the arc between the normals of the faces next to the first edge
// and the arc between the negated normals of the faces next to the second must cross on the unit sphere. With a, b the first pair and c, d the
// second: c and d on opposite sides of the plane of a and b, a and b on opposite sides of the plane of c and d, and the same orientation at both
// crossings (not at antipodal points). It is written out in `poly_poly`.)

/// The contact of two convex solids (the normal points from `a` to `b`), empty when they do not overlap.
pub(crate) fn poly_poly(a: &Poly, b: &Poly) -> Vec<MeshPt> {
    // face axes: how far, at the least, the other solid's corners are in front of each face (above 0: apart)
    let face_query = |p: &Poly, q: &Poly| -> (f32, usize) {
        let mut best = (f32::NEG_INFINITY, 0usize);
        for f in 0..p.n.len() {
            let mut m = f32::INFINITY;
            for v in &q.v {
                m = m.min(dot(p.n[f], *v) - p.d[f]);
            }
            if m > best.0 {
                best = (m, f);
            }
        }
        best
    };
    let (sa, fa) = face_query(a, b);
    if sa > 0.0 || !sa.is_finite() {
        return Vec::new();
    }
    let (sb, fb) = face_query(b, a);
    if sb > 0.0 || !sb.is_finite() {
        return Vec::new();
    }
    // edge axes: only pairs of edges that make a face of the Minkowski difference. The arcs of the faces next to each edge are worked out once
    // (not for every pair), and a pair is dropped at the first test that fails: most are.
    let arcs = |p: &Poly, sign: f32| -> Vec<(V3, V3, V3)> {
        p.edges
            .iter()
            .map(|e| {
                let (u, v) = (mul(p.n[e.fa as usize], sign), mul(p.n[e.fb as usize], sign));
                (u, v, cross(v, u))
            })
            .collect()
    };
    let (arcs_a, arcs_b) = (arcs(a, 1.0), arcs(b, -1.0));
    let mut edge: Option<(f32, usize, usize, V3)> = None;
    for (i, ea) in a.edges.iter().enumerate() {
        let (u1, v1, v1_x_u1) = arcs_a[i];
        let da = sub(a.v[ea.b as usize], a.v[ea.a as usize]);
        for (j, eb) in b.edges.iter().enumerate() {
            let (u2, v2, v2_x_u2) = arcs_b[j];
            let (cba, dba) = (dot(u2, v1_x_u1), dot(v2, v1_x_u1));
            if cba * dba >= 0.0 {
                continue;
            }
            let (adc, bdc) = (dot(u1, v2_x_u2), dot(v1, v2_x_u2));
            if adc * bdc >= 0.0 || cba * bdc <= 0.0 {
                continue;
            }
            let db = sub(b.v[eb.b as usize], b.v[eb.a as usize]);
            let Some(mut axis) = unit(cross(da, db)) else { continue };
            // (pointing from a to b)
            if dot(axis, sub(a.v[ea.a as usize], a.centre)) < 0.0 {
                axis = neg(axis);
            }
            let s = dot(axis, sub(b.v[eb.a as usize], a.v[ea.a as usize]));
            if s > 0.0 {
                return Vec::new();
            }
            if edge.is_none_or(|e| s > e.0) {
                edge = Some((s, i, j, axis));
            }
        }
    }
    let face_sep = sa.max(sb);
    if let Some((s, i, j, axis)) = edge {
        if s > face_sep + EDGE_BIAS {
            let (ea, eb) = (a.edges[i], b.edges[j]);
            let (a0, a1, b0, b1) = (a.v[ea.a as usize], a.v[ea.b as usize], b.v[eb.a as usize], b.v[eb.b as usize]);
            let (s1, s2, _) = segment_segment(a0, a1, b0, b1);
            let (pa, pb) = (add(a0, mul(sub(a1, a0), s1)), add(b0, mul(sub(b1, b0), s2)));
            return vec![MeshPt { p: mul(add(pa, pb), 0.5), n: axis, depth: -s, face: false }];
        }
    }
    // a face contact: the reference face belongs to one solid, the other shows its most opposed face
    let a_is_ref = sa >= sb - FACE_TIE;
    let (rp, ip, rf) = if a_is_ref { (a, b, fa) } else { (b, a, fb) };
    let nref = rp.n[rf];
    let mut g = 0usize;
    let mut least = f32::INFINITY;
    for (k, n) in ip.n.iter().enumerate() {
        let dn = dot(*n, nref);
        if dn < least {
            least = dn;
            g = k;
        }
    }
    let mut poly: Vec<V3> = ip.faces[g].iter().map(|&i| ip.v[i as usize]).collect();
    let ring = rp.faces[rf];
    for k in 0..ring.len() {
        let (p0, p1) = (rp.v[ring[k] as usize], rp.v[ring[(k + 1) % ring.len()] as usize]);
        let Some(inward) = unit(cross(nref, sub(p1, p0))) else { continue };
        poly = clip(&poly, |x| dot(sub(x, p0), inward));
        if poly.is_empty() {
            return Vec::new();
        }
    }
    let out_n = if a_is_ref { nref } else { neg(nref) };
    let mut pts = Vec::with_capacity(poly.len());
    for x in poly {
        let depth = rp.d[rf] - dot(nref, x);
        if depth >= -MARGIN {
            pts.push(MeshPt { p: add(x, mul(nref, 0.5 * depth)), n: out_n, depth, face: true });
        }
    }
    reduce_points(pts, 1)
}

/// The nearest point of a convex solid to `x` (the solid's surface when `x` is outside it): how far `x` is in front of the faces at the most (a
/// negative number: inside), and when outside the squared distance and the point.
fn nearest(p: &Poly, x: V3) -> (f32, usize, Option<(f32, V3)>) {
    let mut front = (f32::NEG_INFINITY, 0usize);
    for f in 0..p.n.len() {
        let s = dot(p.n[f], x) - p.d[f];
        if s > front.0 {
            front = (s, f);
        }
    }
    if front.0 <= 0.0 {
        return (front.0, front.1, None);
    }
    // outside: the nearest point lies on a face that x is in front of; faces are convex polygons, cut into a fan of triangles
    let mut best: Option<(f32, V3)> = None;
    for f in 0..p.n.len() {
        if dot(p.n[f], x) - p.d[f] <= 0.0 {
            continue;
        }
        let ring = p.faces[f];
        for k in 1..ring.len().saturating_sub(1) {
            let (a, b, c) = (p.v[ring[0] as usize], p.v[ring[k] as usize], p.v[ring[k + 1] as usize]);
            let (q, _) = closest_on_triangle(x, a, b, c);
            let d = sub(x, q);
            let d2 = dot(d, d);
            if best.is_none_or(|bb| d2 < bb.0) {
                best = Some((d2, q));
            }
        }
    }
    (front.0, front.1, best)
}

/// A ball against a convex solid: the normal points from the solid to the ball.
pub(crate) fn sphere_poly(p: &Poly, c: V3, r: f32) -> Option<MeshPt> {
    if !(c.iter().all(|v| v.is_finite()) && r.is_finite() && r > 0.0) {
        return None;
    }
    let (s, f, near) = nearest(p, c);
    if s > r {
        return None;
    }
    let (n, depth, face) = match near {
        // the centre is inside: out through the nearest face
        None => (p.n[f], r - s, true),
        Some((d2, q)) => {
            let dist = d2.sqrt();
            if dist >= r {
                return None;
            }
            match unit(sub(c, q)) {
                Some(n) => (n, r - dist, dot(n, p.n[f]) > 0.9999),
                None => (p.n[f], r, true),
            }
        }
    };
    Some(MeshPt { p: sub(c, mul(n, r - 0.5 * depth)), n, depth, face })
}

/// A capsule (axis `p0`-`p1`, radius `r`) against a convex solid: a ball at each end and one where the axis passes nearest to it.
pub(crate) fn capsule_poly(p: &Poly, p0: V3, p1: V3, r: f32) -> Vec<MeshPt> {
    let mut all: Vec<MeshPt> = Vec::new();
    all.extend(sphere_poly(p, p0, r));
    if p0 != p1 {
        all.extend(sphere_poly(p, p1, r));
        // the distance from the solid is a convex function along the axis: find its lowest point
        let d2 = |t: f32| {
            let x = add(p0, mul(sub(p1, p0), t));
            match nearest(p, x) {
                (_, _, None) => 0.0,
                (_, _, Some((d, _))) => d,
            }
        };
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..32 {
            let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if d2(m1) <= d2(m2) {
                hi = m2;
            } else {
                lo = m1;
            }
        }
        let tm = 0.5 * (lo + hi);
        if tm > 0.02 && tm < 0.98 && d2(tm) < d2(0.0).min(d2(1.0)) - 1.0e-5 {
            all.extend(sphere_poly(p, add(p0, mul(sub(p1, p0), tm)), r));
        }
    }
    reduce_points(all, 3)
}

/// What one triangle makes of a convex solid. Two kinds of contact can come out of the same overlap:
/// * **along the triangle's own normal** (`plane_kind`): the face of the solid that lies against the triangle, cut to the triangle's outline;
/// * **the triangle's outline poking into a face of the solid** (`face_kind`, each point with the point of the triangle it comes from): what
///   the shortest way out is when the solid is pointed (a tip, a corner) and the triangle only just reaches it.
/// `chose_face` says which of the two is the shorter way out. The second kind is a *ghost* when the triangle's point is on a flat joint of the
/// mesh (see `PlacedMesh::poly`), so both are kept until the neighbouring triangles have been looked at.
pub(crate) struct TriContact {
    pub chose_face: bool,
    /// The triangle's unit normal and the direction the solid is pushed along it (when the solid overlaps the triangle's plane).
    plane: Option<(V3, V3)>,
    /// The face of the solid the triangle pokes into by the least.
    face: Option<usize>,
}

impl TriContact {
    /// The contact points along the triangle's own normal.
    pub fn plane_kind(&self, p: &Poly, tv: [V3; 3]) -> Vec<MeshPt> {
        match self.plane {
            Some((n, push)) => plane_manifold(p, tv, n, push),
            None => Vec::new(),
        }
    }

    /// The contact points of the triangle's outline against the face, each with the point of the triangle it comes from.
    pub fn face_kind(&self, p: &Poly, tv: [V3; 3]) -> Vec<(MeshPt, V3)> {
        match self.face {
            Some(f) => face_manifold(p, tv, f),
            None => Vec::new(),
        }
    }
}

/// One triangle against a convex solid. The normals point from the triangle to the solid (the way the solid has to move), like a box's. `back`
/// is how far the solid moved during the last tick. `None` when they do not overlap.
pub(crate) fn poly_triangle(p: &Poly, tv: [V3; 3], back: V3) -> Option<TriContact> {
    let [a, b, c] = tv;
    let nr = cross(sub(b, a), sub(c, a));
    let n_unit = unit(nr);
    // along the triangle's own normal
    let mut plane: Option<(f32, V3)> = None;
    let mut went_through = false;
    if let Some(n) = n_unit {
        let (lo, hi) = p.extent(n);
        let t = dot(n, a);
        let (dp, dm) = (t - lo, hi - t); // how far to move along +n and along -n to be clear of the triangle's plane
        if dp <= 0.0 || dm <= 0.0 {
            return None;
        }
        let s = dot(sub(p.centre, a), n);
        let s_prev = s - dot(back, n);
        went_through = s * s_prev < 0.0;
        plane = Some(if went_through {
            if s_prev > 0.0 {
                (dp, n)
            } else {
                (dm, neg(n))
            }
        } else if dp <= dm {
            (dp, n)
        } else {
            (dm, neg(n))
        });
    }
    // along the normal of a face of the solid: the triangle pokes into that face from outside
    let mut face: Option<(f32, usize)> = None;
    for f in 0..p.n.len() {
        let nf = p.n[f];
        let (ta, tb, tc) = (dot(nf, a), dot(nf, b), dot(nf, c));
        let (tri_lo, tri_hi) = (ta.min(tb).min(tc), ta.max(tb).max(tc));
        let dm = p.d[f] - tri_lo; // the solid moves along -n by this much to be clear of the triangle
        let (lo, _) = p.extent(nf);
        let dp = tri_hi - lo;
        if dm <= 0.0 || dp <= 0.0 {
            return None;
        }
        if face.is_none_or(|b| dm < b.0) {
            face = Some((dm, f));
        }
    }
    let chose_face = match (plane, face) {
        (None, None) => return None,
        (None, Some(_)) => true,
        (Some(_), None) => false,
        // the triangle's own normal wins a near tie (a solid lying flat on a floor is held by the floor's plane) and a solid that went through
        (Some((dt, _)), Some((df, _))) => !went_through && df < dt - 1.0e-3,
    };
    let plane = match (plane, n_unit) {
        (Some((_, push)), Some(n)) => Some((n, push)),
        _ => None,
    };
    Some(TriContact { chose_face, plane, face: face.map(|f| f.1) })
}

/// The face of the solid that lies against the triangle (its outward normal most against the push), cut to the triangle's outline; every
/// corner of what is left that is below the triangle's plane is a contact.
fn plane_manifold(p: &Poly, tv: [V3; 3], n: V3, push: V3) -> Vec<MeshPt> {
    let a = tv[0];
    let mut g = 0usize;
    let mut least = f32::INFINITY;
    for (k, nk) in p.n.iter().enumerate() {
        let dn = dot(*nk, push);
        if dn < least {
            least = dn;
            g = k;
        }
    }
    let mut poly: Vec<V3> = p.faces[g].iter().map(|&i| p.v[i as usize]).collect();
    for i in 0..3 {
        let (e0, e1, third) = (tv[i], tv[(i + 1) % 3], tv[(i + 2) % 3]);
        let Some(mut inward) = unit(cross(n, sub(e1, e0))) else { return Vec::new() };
        if dot(inward, sub(third, e0)) < 0.0 {
            inward = neg(inward);
        }
        // (a little slack, so that a corner right on the seam between two triangles is seen by both)
        poly = clip(&poly, |x| dot(sub(x, e0), inward) + 1.0e-4);
        if poly.is_empty() {
            return Vec::new();
        }
    }
    let mut out = Vec::new();
    for x in poly {
        let depth = dot(sub(a, x), push);
        if depth >= -MARGIN {
            out.push(MeshPt { p: add(x, mul(push, 0.5 * depth)), n: push, depth, face: true });
        }
    }
    out
}

/// The triangle cut to the outline of the face `f` it pokes into; every corner of what is left that is inside that face's plane is a contact.
fn face_manifold(p: &Poly, tv: [V3; 3], f: usize) -> Vec<(MeshPt, V3)> {
    let nf = p.n[f];
    let ring = p.faces[f];
    let mut poly = vec![tv[0], tv[1], tv[2]];
    for k in 0..ring.len() {
        let (p0, p1) = (p.v[ring[k] as usize], p.v[ring[(k + 1) % ring.len()] as usize]);
        let Some(inward) = unit(cross(nf, sub(p1, p0))) else { continue };
        poly = clip(&poly, |x| dot(sub(x, p0), inward));
        if poly.is_empty() {
            return Vec::new();
        }
    }
    let mut out = Vec::new();
    for x in poly {
        let depth = p.d[f] - dot(nf, x);
        if depth >= -MARGIN {
            out.push((MeshPt { p: add(x, mul(nf, 0.5 * depth)), n: neg(nf), depth, face: true }, x));
        }
    }
    out
}

impl PlacedMesh {
    /// The contacts of a convex solid (a hull or a box) with the mesh; the normals point from the mesh to the solid.
    ///
    /// **Ghosts.** A pointed solid (a pyramid on its tip, a box on a corner) standing on a flat floor touches the corners and edges of the floor's
    /// triangles; seen from one triangle alone the shortest way out of it can be sideways along a face of the solid. Such a contact is a ghost
    /// when the point of the triangle it is made at lies on a joint with another triangle of the same plane: the joint is no feature of the
    /// mesh, the neighbour is there to hold the solid. So those points are dropped, and the contact along the triangle's own normal (the
    /// solid's tip against the floor's face) is used instead. At the rim of a floor, the ridge of a roof or the corner of a step there is no
    /// coplanar neighbour and the contact stays.
    pub(crate) fn poly(&self, p: &Poly, back: V3) -> Vec<MeshPt> {
        if p.v.iter().any(|v| !v.iter().all(|c| c.is_finite())) || !back.iter().all(|c| c.is_finite()) {
            return Vec::new();
        }
        let bx = p.bounds();
        if !bx.overlaps(&self.bounds) {
            return Vec::new();
        }
        let tris = self.triangles_near(&bx);
        let tv: Vec<[V3; 3]> = tris.iter().map(|&t| self.tri(t)).collect();
        let normals: Vec<V3> = tv.iter().map(|t| unit(cross(sub(t[1], t[0]), sub(t[2], t[0]))).unwrap_or([0.0; 3])).collect();
        let mut pts: Vec<MeshPt> = Vec::new();
        for (i, t) in tv.iter().enumerate() {
            let Some(c) = poly_triangle(p, *t, back) else { continue };
            if !c.chose_face {
                pts.extend(c.plane_kind(p, *t));
                continue;
            }
            // a point of the triangle on a joint with a coplanar neighbour: a ghost
            let flat_joint = |x: V3| (0..tv.len()).any(|j| j != i && dot(normals[i], normals[j]).abs() > 0.9999 && crate::meshcol_shapes::point_triangle_d2(x, &tv[j]) <= 1.0e-8);
            let real: Vec<MeshPt> = c.face_kind(p, *t).iter().filter(|(_, x)| !flat_joint(*x)).map(|(m, _)| *m).collect();
            if real.is_empty() {
                pts.extend(c.plane_kind(p, *t));
            } else {
                pts.extend(real);
            }
        }
        reduce_points(pts, 3)
    }
}

/// The points of a hull that are under the surface of a terrain: for the solver, with the normals from the ground to the hull. (A hull has at most
/// 64 corners, so every one is tried.)
pub(crate) fn corners_under(p: &Poly, surface: &dyn Fn(f32, f32) -> Option<(f32, V3)>) -> Vec<MeshPt> {
    let mut cands = Vec::new();
    for w in &p.v {
        if let Some((s, n)) = surface(w[0], w[2]) {
            let pen = s - w[1];
            if pen > 0.0 {
                cands.push(MeshPt { p: [w[0], w[1] + 0.5 * pen, w[2]], n, depth: pen * n[1].max(0.2), face: true });
            }
        }
    }
    reduce_points(cands, 3)
}
