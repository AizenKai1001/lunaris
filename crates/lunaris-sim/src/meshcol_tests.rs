//! Triangle meshes: the tree, the rays, and the contacts of balls, capsules and boxes. Everything here is made of triangles written
//! in the test, no file is needed (the file reader has its own tests, and `mesh_physics_tests.rs` plays whole scenes).
use crate::components::Transform;
use crate::meshcol::{cross, dot, is_degenerate, sub, CollisionMesh, PlacedMesh, MAX_TRIANGLES};
use crate::meshcol_shapes::{closest_on_triangle, segment_triangle, Feature, MeshPt};
use std::sync::Arc;

type V3 = [f32; 3];

/// A flat floor of n x n squares of `cell` metres, centred on the origin at height `y`, two triangles a square, facing up.
pub(crate) fn floor_tris(n: usize, cell: f32, y: f32) -> (Vec<V3>, Vec<[u32; 3]>) {
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    let half = n as f32 * cell * 0.5;
    for j in 0..=n {
        for i in 0..=n {
            verts.push([-half + i as f32 * cell, y, -half + j as f32 * cell]);
        }
    }
    let at = |i: usize, j: usize| (j * (n + 1) + i) as u32;
    for j in 0..n {
        for i in 0..n {
            // (x to the right, z towards the viewer: this winding faces up)
            tris.push([at(i, j), at(i, j + 1), at(i + 1, j)]);
            tris.push([at(i + 1, j), at(i, j + 1), at(i + 1, j + 1)]);
        }
    }
    (verts, tris)
}

pub(crate) fn placed(verts: Vec<V3>, tris: Vec<[u32; 3]>) -> PlacedMesh {
    PlacedMesh::new(Arc::new(CollisionMesh::new(verts, tris).unwrap()), &Transform::default()).unwrap()
}

fn floor(n: usize, cell: f32) -> PlacedMesh {
    let (v, t) = floor_tris(n, cell, 0.0);
    placed(v, t)
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-4
}

fn near3(a: V3, b: V3, tol: f32) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < tol)
}

/// A small fixed pseudo-random sequence (so every machine tests the same triangles).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

fn soup(n: usize, seed: u64) -> (Vec<V3>, Vec<[u32; 3]>) {
    let mut r = Lcg(seed);
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    for i in 0..n {
        let c = [r.range(-20.0, 20.0), r.range(-5.0, 5.0), r.range(-20.0, 20.0)];
        for _ in 0..3 {
            verts.push([c[0] + r.range(-1.5, 1.5), c[1] + r.range(-1.5, 1.5), c[2] + r.range(-1.5, 1.5)]);
        }
        tris.push([3 * i as u32, 3 * i as u32 + 1, 3 * i as u32 + 2]);
    }
    (verts, tris)
}

/// The straightforward answer a ray has: look at every triangle.
fn brute_ray(m: &CollisionMesh, o: V3, d: V3, max: f32) -> Option<(f32, u32)> {
    let mut best: Option<(f32, u32)> = None;
    for i in 0..m.tris.len() as u32 {
        let [a, b, c] = m.tri(i);
        let (e1, e2) = (sub(b, a), sub(c, a));
        let p = cross(d, e2);
        let det = dot(e1, p);
        if det.abs() < 1e-12 {
            continue;
        }
        let inv = 1.0 / det;
        let tv = sub(o, a);
        let u = dot(tv, p) * inv;
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = cross(tv, e1);
        let v = dot(d, q) * inv;
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        let t = dot(e2, q) * inv;
        if t >= 0.0 && t <= max && best.is_none_or(|(bt, _)| t < bt) {
            best = Some((t, i));
        }
    }
    best
}

#[test]
fn the_tree_finds_the_same_ray_hits_as_looking_at_every_triangle() {
    let (v, t) = soup(3000, 7);
    let m = CollisionMesh::new(v, t).unwrap();
    let mut r = Lcg(99);
    let mut hits = 0;
    for _ in 0..600 {
        let o = [r.range(-25.0, 25.0), r.range(-8.0, 8.0), r.range(-25.0, 25.0)];
        let d = [r.range(-1.0, 1.0), r.range(-1.0, 1.0), r.range(-1.0, 1.0)];
        let max = r.range(1.0, 60.0);
        let want = brute_ray(&m, o, d, max);
        let got = m.ray_local(o, d, max).map(|h| (h.t, h.tri));
        assert_eq!(got, want, "ray from {o:?} along {d:?}");
        hits += usize::from(want.is_some());
    }
    assert!(hits > 100, "the rays should hit something often enough to mean something: {hits}");
}

#[test]
fn the_same_triangles_always_give_the_same_tree_and_hash_and_a_changed_one_a_different_hash() {
    let (v, t) = soup(2000, 3);
    let a = CollisionMesh::new(v.clone(), t.clone()).unwrap();
    let b = CollisionMesh::new(v.clone(), t.clone()).unwrap();
    assert_eq!(a.hash, b.hash);
    assert_eq!(a.order, b.order);
    assert!(a.nodes.iter().zip(&b.nodes).all(|(x, y)| x.b == y.b && x.link == y.link && x.count == y.count));
    let mut v2 = v;
    v2[10][0] += 0.001;
    assert_ne!(CollisionMesh::new(v2, t).unwrap().hash, a.hash);
    // every triangle is in exactly one leaf
    let mut seen = vec![0u32; a.tris.len()];
    for n in a.nodes.iter().filter(|n| n.count > 0) {
        for k in 0..n.count {
            seen[a.order[(n.link + k) as usize] as usize] += 1;
        }
    }
    assert!(seen.iter().all(|&c| c == 1));
}

#[test]
fn a_large_flat_mesh_builds_fast_and_a_ray_goes_through_it() {
    // 100 x 100 squares = 20,000 triangles
    let (v, t) = floor_tris(100, 0.5, 0.0);
    let m = CollisionMesh::new(v, t).unwrap();
    assert_eq!(m.tris.len(), 20_000);
    let h = m.ray_local([3.3, 5.0, -2.2], [0.0, -1.0, 0.0], 100.0).unwrap();
    assert!(near(h.t, 5.0) && near3(h.normal, [0.0, 1.0, 0.0], 1e-6), "{h:?}");
    // from below: the normal faces the ray
    let h = m.ray_local([3.3, -5.0, -2.2], [0.0, 1.0, 0.0], 100.0).unwrap();
    assert!(near(h.t, 5.0) && near(h.normal[1], -1.0));
    assert!(m.ray_local([3.3, 5.0, -2.2], [0.0, 1.0, 0.0], 100.0).is_none(), "pointing away");
    assert!(m.ray_local([300.0, 5.0, 0.0], [0.0, -1.0, 0.0], 100.0).is_none(), "beside it");
    assert!(m.ray_local([0.0, 5.0, 0.0], [0.0, -1.0, 0.0], 4.0).is_none(), "too short");
}

#[test]
fn degenerate_triangles_are_left_out_and_bad_models_are_refused_with_a_reason() {
    let verts = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [2.0, 0.0, 0.0], [3.0, 0.0, 0.0]];
    // the second triangle has three points on a line, the third repeats a corner
    let m = CollisionMesh::new(verts.clone(), vec![[0, 1, 2], [0, 1, 3], [0, 0, 1]]).unwrap();
    assert_eq!(m.tris.len(), 1);
    assert_eq!(m.degenerate, 2);
    assert!(is_degenerate([0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]));
    let err = |r: Result<CollisionMesh, String>| r.err().unwrap_or_else(|| "(accepted)".to_string());
    assert!(err(CollisionMesh::new(verts.clone(), vec![])).contains("no triangles"));
    assert!(err(CollisionMesh::new(verts.clone(), vec![[0, 1, 3]])).contains("no usable triangles"));
    assert!(err(CollisionMesh::new(verts.clone(), vec![[0, 1, 9]])).contains("does not exist"));
    assert!(err(CollisionMesh::new(vec![[f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], vec![[0, 1, 2]])).contains("not numbers"));
    assert!(err(CollisionMesh::new(vec![[f32::INFINITY, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], vec![[0, 1, 2]])).contains("not numbers"));
    let many = vec![[0u32, 1, 2]; MAX_TRIANGLES + 1];
    let e = err(CollisionMesh::new(verts, many));
    assert!(e.contains("1000000") || e.contains("1,000,000") || e.contains(&MAX_TRIANGLES.to_string()), "{e}");
}

#[test]
fn placing_a_mesh_moves_rotates_and_scales_it_and_refuses_numbers_that_are_not_sane() {
    let (v, t) = floor_tris(2, 1.0, 0.0);
    let mesh = Arc::new(CollisionMesh::new(v, t).unwrap());
    let tr = Transform { pos: [10.0, 2.0, -3.0], rot: [0.0, 0.0, 0.0, 1.0], scale: [2.0, 1.0, 3.0] };
    let p = PlacedMesh::new(mesh.clone(), &tr).unwrap();
    assert!(near3(p.bounds.lo, [8.0, 2.0, -6.0], 1e-5) && near3(p.bounds.hi, [12.0, 2.0, 0.0], 1e-5), "{:?}", p.bounds);
    // turned a quarter about x: the floor becomes a wall
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let p = PlacedMesh::new(mesh.clone(), &Transform { pos: [0.0; 3], rot: [s, 0.0, 0.0, s], scale: [1.0; 3] }).unwrap();
    assert!(p.bounds.hi[1] > 0.99 && p.bounds.lo[1] < -0.99 && p.bounds.hi[2].abs() < 1e-4, "{:?}", p.bounds);
    // the world ray agrees with the model-space ray moved by hand
    let h = p.ray([0.0, 0.2, 5.0], [0.0, 0.0, -1.0], 20.0).unwrap();
    assert!(near(h.t, 5.0) && near(h.normal[2], 1.0), "{h:?}");
    // not numbers, or far too big: no placement (the entity then simply has no collision)
    for bad in [
        Transform { pos: [f32::NAN, 0.0, 0.0], ..Transform::default() },
        Transform { scale: [f32::INFINITY, 1.0, 1.0], ..Transform::default() },
        Transform { rot: [f32::NAN, 0.0, 0.0, 1.0], ..Transform::default() },
        Transform { pos: [1.0e30, 0.0, 0.0], ..Transform::default() },
        Transform { scale: [1.0e38, 1.0e38, 1.0e38], ..Transform::default() },
    ] {
        assert!(PlacedMesh::new(mesh.clone(), &bad).is_none(), "{bad:?}");
    }
    // a zero rotation counts as no rotation, a zero scale squashes the mesh flat without a panic
    assert!(PlacedMesh::new(mesh.clone(), &Transform { rot: [0.0; 4], ..Transform::default() }).is_some());
    let flat = PlacedMesh::new(mesh, &Transform { scale: [0.0; 3], ..Transform::default() }).unwrap();
    assert!(flat.sphere([0.0, 0.1, 0.0], 0.5, [0.0; 3]).len() <= 4);
}

#[test]
fn the_nearest_point_of_a_triangle_knows_whether_it_is_on_the_face_an_edge_or_a_corner() {
    let (a, b, c) = ([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 0.0, 4.0]);
    let (q, f) = closest_on_triangle([1.0, 3.0, 1.0], a, b, c);
    assert!(near3(q, [1.0, 0.0, 1.0], 1e-6) && f == Feature::Face);
    let (q, f) = closest_on_triangle([2.0, 1.0, 2.0 + 3.0], a, b, c);
    assert!(f == Feature::Edge && near3(q, [0.5, 0.0, 3.5], 1e-5), "{q:?} {f:?}");
    let (q, f) = closest_on_triangle([-3.0, 2.0, -3.0], a, b, c);
    assert!(f == Feature::Vertex && q == a);
    let (q, f) = closest_on_triangle([9.0, 1.0, 0.5], a, b, c);
    assert!(f == Feature::Vertex && q == b);
}

#[test]
fn a_segment_finds_the_triangle_it_passes_through_and_the_closest_approach_otherwise() {
    let t = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 0.0, 4.0]];
    let (s, d2) = segment_triangle([1.0, -1.0, 1.0], [1.0, 3.0, 1.0], &t);
    assert!(near(s, 0.25) && d2 == 0.0, "{s} {d2}");
    // beside it, parallel and above: nearest at one end, a metre up
    let (_, d2) = segment_triangle([1.0, 1.0, 1.0], [2.0, 1.0, 1.0], &t);
    assert!(near(d2, 1.0));
    // a slanted segment that stays above the triangle: the answer is the smallest of the distances along it
    let (p0, p1) = ([2.0, 3.0, -1.0], [2.0, 1.0, 3.0]);
    let (s, d2) = segment_triangle(p0, p1, &t);
    let mut want = f32::INFINITY;
    for i in 0..=2000 {
        let k = i as f32 / 2000.0;
        let p = [p0[0] + (p1[0] - p0[0]) * k, p0[1] + (p1[1] - p0[1]) * k, p0[2] + (p1[2] - p0[2]) * k];
        let (q, _) = closest_on_triangle(p, t[0], t[1], t[2]);
        let d = sub(p, q);
        want = want.min(dot(d, d));
    }
    assert!((d2 - want).abs() < 2.0e-3 && (0.0..=1.0).contains(&s), "{s} {d2} vs {want}");
    // and one whose nearest point is in the middle: it passes above the triangle's edge nearer in the middle than at its ends
    let (p0, p1) = ([2.0, 2.0, 0.0], [2.0, 2.0, 8.0]);
    let (s, d2) = segment_triangle(p0, p1, &t);
    assert!(d2 > 3.9 && d2 < 4.1 && (0.0..=1.0).contains(&s), "{s} {d2}");
}

fn only(pts: &[MeshPt]) -> MeshPt {
    assert_eq!(pts.len(), 1, "{pts:?}");
    pts[0]
}

#[test]
fn a_ball_on_a_flat_floor_has_one_contact_straight_up_and_none_when_it_is_clear() {
    let m = floor(10, 1.0);
    let c = only(&m.sphere([0.3, 0.45, 0.2], 0.5, [0.0; 3]));
    assert!(near3(c.n, [0.0, 1.0, 0.0], 1e-5) && near(c.depth, 0.05), "{c:?}");
    assert!(near(c.p[1], -0.025), "the point is halfway into the overlap: {c:?}");
    assert!(m.sphere([0.3, 0.55, 0.2], 0.5, [0.0; 3]).is_empty(), "clear of the floor");
    assert!(m.sphere([30.0, 0.1, 0.2], 0.5, [0.0; 3]).is_empty(), "beside the floor");
    // from below the normal points down
    let c = only(&m.sphere([0.3, -0.45, 0.2], 0.5, [0.0; 3]));
    assert!(near(c.n[1], -1.0) && near(c.depth, 0.05), "{c:?}");
}

#[test]
fn a_ball_rolling_over_the_seams_of_a_flat_floor_never_feels_them() {
    // The seams of the squares and the diagonals: at every place the ball may be, the one contact is straight up. (If the edge of the
    // neighbouring triangle counted, the push would be tilted sideways here and a rolling ball would slow down at every seam.)
    let m = floor(8, 1.0);
    let mut worst: f32 = 1.0;
    let mut count = 0;
    for ix in -60..=60 {
        for iz in -60..=60 {
            let c = [ix as f32 * 0.05 + 0.001, 0.4, iz as f32 * 0.05 + 0.002];
            let pts = m.sphere(c, 0.5, [0.0; 3]);
            assert_eq!(pts.len(), 1, "at {c:?}: {pts:?}");
            worst = worst.min(pts[0].n[1]);
            count += 1;
        }
    }
    assert!(worst > 0.9999, "the push is tilted by the seam: {worst} after {count} places");
}

#[test]
fn a_ball_on_the_ridge_of_a_roof_is_pushed_along_the_bisector_and_in_a_valley_by_two_walls() {
    // a roof: two 45 degree slopes meeting at x = 0 (the ridge runs along z)
    let verts = vec![[-5.0, 0.0, -5.0], [0.0, 5.0, -5.0], [5.0, 0.0, -5.0], [-5.0, 0.0, 5.0], [0.0, 5.0, 5.0], [5.0, 0.0, 5.0]];
    let roof = placed(verts, vec![[0, 3, 1], [1, 3, 4], [1, 4, 2], [2, 4, 5]]);
    // a ball above the ridge, touching only the ridge line
    let c = only(&roof.sphere([0.0, 5.4, 0.0], 0.5, [0.0; 3]));
    assert!(near3(c.n, [0.0, 1.0, 0.0], 1e-4) && near(c.depth, 0.1), "{c:?}");
    // a ball a little to the side of the crest: it touches the slope's face and nothing else (the other slope's nearest point is the
    // ridge, farther away than that slope's face is: that contact would be a ghost and is not made)
    let c = only(&roof.sphere([-0.6, 4.9, 0.0], 0.5, [0.0; 3]));
    let s = std::f32::consts::FRAC_1_SQRT_2;
    assert!(near3(c.n, [-s, s, 0.0], 1e-3), "{c:?}");
    // a valley: the same two slopes seen from the other side (swapped heights): the ball lies on both walls and each pushes it
    let vale = placed(vec![[-5.0, 5.0, -5.0], [0.0, 0.0, -5.0], [5.0, 5.0, -5.0], [-5.0, 5.0, 5.0], [0.0, 0.0, 5.0], [5.0, 5.0, 5.0]], vec![[0, 3, 1], [1, 3, 4], [1, 4, 2], [2, 4, 5]]);
    for x in [0.0f32, 0.2, -0.3] {
        let pts = vale.sphere([x, 0.5, 0.0], 0.8, [0.0; 3]);
        assert_eq!(pts.len(), 2, "at x = {x} both walls push: {pts:?}");
        assert!(pts[0].n[0] * pts[1].n[0] < 0.0 && pts.iter().all(|p| p.n[1] > 0.7), "{pts:?}");
    }
}

#[test]
fn a_ball_that_went_through_a_thin_floor_in_one_tick_is_put_back_on_the_side_it_came_from() {
    let m = floor(4, 1.0);
    // the centre is 0.1 below the floor now, and was 0.3 above it a tick ago: it fell through, so it belongs on top
    let c = only(&m.sphere([0.0, -0.1, 0.0], 0.25, [0.0, -0.4, 0.0]));
    assert!(near(c.n[1], 1.0) && near(c.depth, 0.35), "{c:?}");
    // the same place but it was already below the floor: it stays below
    let c = only(&m.sphere([0.0, -0.1, 0.0], 0.25, [0.0, 0.0, 0.0]));
    assert!(near(c.n[1], -1.0) && near(c.depth, 0.15), "{c:?}");
    // moving up through it from below
    let c = only(&m.sphere([0.0, 0.1, 0.0], 0.25, [0.0, 0.4, 0.0]));
    assert!(near(c.n[1], -1.0) && near(c.depth, 0.35), "{c:?}");
}

#[test]
fn a_ball_in_a_corner_of_a_floor_and_two_walls_touches_all_three() {
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    let mut quad = |a: V3, b: V3, c: V3, d: V3| {
        let k = verts.len() as u32;
        verts.extend([a, b, c, d]);
        tris.push([k, k + 1, k + 2]);
        tris.push([k, k + 2, k + 3]);
    };
    quad([0.0, 0.0, 0.0], [0.0, 0.0, 5.0], [5.0, 0.0, 5.0], [5.0, 0.0, 0.0]); // floor, up
    quad([0.0, 0.0, 0.0], [0.0, 5.0, 0.0], [0.0, 5.0, 5.0], [0.0, 0.0, 5.0]); // wall at x = 0, facing +x
    quad([0.0, 0.0, 0.0], [5.0, 0.0, 0.0], [5.0, 5.0, 0.0], [0.0, 5.0, 0.0]); // wall at z = 0, facing +z
    let m = placed(verts, tris);
    let pts = m.sphere([0.4, 0.4, 0.4], 0.5, [0.0; 3]);
    assert_eq!(pts.len(), 3, "{pts:?}");
    for want in [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]] {
        assert!(pts.iter().any(|p| near3(p.n, want, 1e-4)), "{want:?} missing in {pts:?}");
    }
}

#[test]
fn a_capsule_lying_on_a_floor_has_two_points_and_one_standing_on_it_has_one() {
    let m = floor(10, 1.0);
    // lying along x, radius 0.25, axis 0.2 above the floor
    let pts = m.capsule([-0.8, 0.2, 0.0], [0.8, 0.2, 0.0], 0.25, [0.0; 3]);
    assert_eq!(pts.len(), 2, "one at each end and none between (the middle is no nearer than the ends): {pts:?}");
    assert!(pts.iter().all(|p| near3(p.n, [0.0, 1.0, 0.0], 1e-4) && near(p.depth, 0.05)));
    assert!((pts[0].p[0] - pts[1].p[0]).abs() > 1.5, "one at each end: {pts:?}");
    // standing
    let pts = m.capsule([0.0, 0.2, 0.0], [0.0, 1.4, 0.0], 0.25, [0.0; 3]);
    assert_eq!(pts.len(), 1, "{pts:?}");
    // a pole across a ledge: a thin wall top at height 1 under the middle of a long horizontal capsule
    let wall = placed(vec![[-0.1, 0.0, -3.0], [-0.1, 1.0, -3.0], [-0.1, 1.0, 3.0], [-0.1, 0.0, 3.0], [0.1, 1.0, -3.0], [0.1, 1.0, 3.0]], vec![[0, 1, 2], [0, 2, 3], [1, 4, 5], [1, 5, 2]]);
    let pts = wall.capsule([-2.0, 1.1, 0.0], [2.0, 1.1, 0.0], 0.25, [0.0; 3]);
    assert!(pts.iter().any(|p| near3(p.n, [0.0, 1.0, 0.0], 1e-3) && p.p[0].abs() < 0.2), "the middle of the pole rests on the wall's top: {pts:?}");
}

#[test]
fn a_box_lying_on_a_floor_has_four_corners_of_contact_all_pointing_up_whatever_the_seams() {
    let m = floor(6, 1.0);
    let ax = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for (cx, cz) in [(0.0, 0.0), (0.37, 0.12), (-0.5, 0.5), (1.0, 1.0), (0.001, -0.3)] {
        let pts = m.obb([cx, 0.298, cz], &ax, [0.5, 0.3, 0.4], [0.0; 3]);
        assert_eq!(pts.len(), 4, "box at ({cx}, {cz}): {pts:?}");
        for p in &pts {
            assert!(near3(p.n, [0.0, 1.0, 0.0], 1e-4) && (p.depth - 0.002).abs() < 1e-3, "{p:?}");
        }
        // the four are the four corners: spread over the box's footprint
        let (xs, zs): (Vec<f32>, Vec<f32>) = (pts.iter().map(|p| p.p[0] - cx).collect(), pts.iter().map(|p| p.p[2] - cz).collect());
        assert!(xs.iter().cloned().fold(f32::MIN, f32::max) > 0.45 && xs.iter().cloned().fold(f32::MAX, f32::min) < -0.45, "{xs:?}");
        assert!(zs.iter().cloned().fold(f32::MIN, f32::max) > 0.35 && zs.iter().cloned().fold(f32::MAX, f32::min) < -0.35, "{zs:?}");
    }
    assert!(m.obb([0.0, 0.35, 0.0], &ax, [0.5, 0.3, 0.4], [0.0; 3]).is_empty(), "clear of the floor");
    assert!(m.obb([40.0, 0.2, 0.0], &ax, [0.5, 0.3, 0.4], [0.0; 3]).is_empty(), "beside the floor");
}

#[test]
fn a_box_tipped_on_an_edge_touches_with_its_edge_and_one_pushed_into_a_wall_pushes_sideways() {
    let m = floor(6, 1.0);
    // turned 45 degrees about z: it stands on a bottom edge, the contact is two points along that edge
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let q = [0.0, 0.0, (std::f32::consts::FRAC_PI_8).sin(), (std::f32::consts::FRAC_PI_8).cos()];
    let ax = crate::physics_rot::axes_of(q);
    let low = 0.5 * s * 2.0; // the half diagonal of a unit-side square section
    let pts = m.obb([0.0, low - 0.003, 0.0], &ax, [0.5, 0.5, 0.4], [0.0; 3]);
    assert!(!pts.is_empty() && pts.len() <= 4, "{pts:?}");
    assert!(pts.iter().all(|p| near3(p.n, [0.0, 1.0, 0.0], 1e-3)), "{pts:?}");
    assert!(pts.iter().any(|p| p.p[2] > 0.3) && pts.iter().any(|p| p.p[2] < -0.3), "along the whole edge: {pts:?}");
    // a wall (x = 0.5, facing -x) and a box whose side is 0.02 into it: pushed back along -x
    let wall = placed(vec![[0.5, 0.0, -3.0], [0.5, 3.0, -3.0], [0.5, 3.0, 3.0], [0.5, 0.0, 3.0]], vec![[0, 2, 1], [0, 3, 2]]);
    let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let pts = wall.obb([0.52 - 0.3, 1.0, 0.0], &ident, [0.3, 0.4, 0.3], [0.0; 3]);
    assert!(!pts.is_empty() && pts.iter().all(|p| near3(p.n, [-1.0, 0.0, 0.0], 1e-4) && near(p.depth, 0.02)), "{pts:?}");
}

#[test]
fn a_box_that_fell_through_a_thin_floor_in_one_tick_is_put_back_on_top() {
    let m = floor(6, 1.0);
    let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    // the centre is below the floor now and was above it a tick ago
    let pts = m.obb([0.0, -0.05, 0.0], &ident, [0.2, 0.1, 0.2], [0.0, -0.3, 0.0]);
    assert!(!pts.is_empty() && pts.iter().all(|p| near(p.n[1], 1.0)), "{pts:?}");
}

#[test]
fn nothing_that_asks_a_question_with_odd_numbers_panics() {
    let m = floor(4, 1.0);
    let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let odd = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.0e30, -1.0e30, 0.0, -1.0, 1.0e-30];
    for &a in &odd {
        for &b in &odd {
            let v = [a, b, 0.5];
            let _ = m.sphere(v, 0.5, [0.0; 3]);
            let _ = m.sphere([0.0, 0.2, 0.0], a, [0.0; 3]);
            let _ = m.sphere([0.0, 0.2, 0.0], 0.5, v);
            let _ = m.capsule(v, [0.0, 1.0, 0.0], b, [0.0; 3]);
            let _ = m.capsule([0.0, 0.2, 0.0], [0.0, 1.0, 0.0], 0.3, v);
            let _ = m.obb(v, &ident, [0.3, 0.3, 0.3], [0.0; 3]);
            let _ = m.obb([0.0, 0.2, 0.0], &ident, [a, b, 0.3], v);
            let _ = m.ray(v, [0.0, -1.0, 0.0], 10.0);
            let _ = m.ray([0.0, 5.0, 0.0], v, a);
            let _ = m.sphere_push(v, b);
            let _ = m.capsule_push(v, [0.0; 3], b);
        }
    }
    // a ray with a zero direction hits nothing instead of dividing by zero
    assert!(m.ray([0.0, 5.0, 0.0], [0.0; 3], 10.0).is_none());
    // and a ray that runs exactly along the floor's plane
    let _ = m.ray([-10.0, 0.0, 0.0], [1.0, 0.0, 0.0], 100.0);
}

#[test]
fn contacts_do_not_depend_on_the_order_the_triangles_were_listed_in() {
    let (v, t) = soup(400, 5);
    let mut shuffled = t.clone();
    let mut r = Lcg(11);
    for i in (1..shuffled.len()).rev() {
        let j = (r.next() * (i + 1) as f32) as usize % (i + 1);
        shuffled.swap(i, j);
    }
    let a = placed(v.clone(), t);
    let b = placed(v, shuffled);
    let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut found = 0;
    for _ in 0..300 {
        let c = [r.range(-15.0, 15.0), r.range(-4.0, 4.0), r.range(-15.0, 15.0)];
        let (sa, sb) = (a.sphere(c, 1.2, [0.0; 3]), b.sphere(c, 1.2, [0.0; 3]));
        // the same set of pushes (the order of equally deep ones may differ with the triangle numbers)
        assert_eq!(sa.len(), sb.len(), "{c:?}");
        for p in &sa {
            assert!(sb.iter().any(|q| near3(q.n, p.n, 1e-4) && near(q.depth, p.depth)), "{c:?}: {p:?} not in {sb:?}");
        }
        found += sa.len();
        let (oa, ob) = (a.obb(c, &ident, [0.6, 0.5, 0.4], [0.0; 3]), b.obb(c, &ident, [0.6, 0.5, 0.4], [0.0; 3]));
        assert_eq!(oa.len(), ob.len(), "{c:?}");
    }
    assert!(found > 20, "the test should touch something: {found}");
}

#[test]
fn the_pruned_search_for_the_nearest_point_of_a_capsule_axis_gives_exactly_the_answer_of_measuring_every_triangle() {
    // meshes: a rough soup of triangles, a flat floor, a tall thin soup (a wall of poles), and the soup far from the origin (the margin for rounding grows with the distance)
    let meshes: Vec<(PlacedMesh, V3)> = {
        let mut v = Vec::new();
        let (a, b) = soup(300, 7);
        v.push((placed(a, b), [0.0, 0.0, 0.0]));
        let (a, b) = floor_tris(24, 0.5, 0.0);
        v.push((placed(a, b), [0.0, 0.0, 0.0]));
        let (a, b) = soup(120, 99);
        let squeezed: Vec<V3> = a.iter().map(|p| [p[0] * 0.15, p[1] * 2.0, p[2] * 0.15]).collect();
        v.push((placed(squeezed, b), [0.0, 0.0, 0.0]));
        let (a, b) = soup(200, 3);
        let far: Vec<V3> = a.iter().map(|p| [p[0] + 3000.0, p[1], p[2] - 2000.0]).collect();
        v.push((placed(far, b), [3000.0, 0.0, -2000.0]));
        v
    };
    let mut rng = Lcg(0xC0FFEE);
    let (mut some, mut total, mut mid) = (0usize, 0usize, 0usize);
    for (mesh, base) in &meshes {
        for i in 0..25_000 {
            let p0 = [base[0] + rng.range(-9.0, 9.0), base[1] + rng.range(-1.0, 6.0), base[2] + rng.range(-9.0, 9.0)];
            // vertical capsules (a character) most of the time, any slant for the rest
            let p1 = if i % 4 != 0 { [p0[0], p0[1] + rng.range(0.1, 1.6), p0[2]] } else { [p0[0] + rng.range(-1.5, 1.5), p0[1] + rng.range(-1.5, 1.5), p0[2] + rng.range(-1.5, 1.5)] };
            let r = rng.range(0.15, 0.9);
            let fast = mesh.nearest_on_axis(p0, p1, r);
            let slow = mesh.nearest_on_axis_reference(p0, p1, r);
            assert_eq!(fast.map(f32::to_bits), slow.map(f32::to_bits), "capsule {p0:?} {p1:?} r {r}");
            total += 1;
            some += fast.is_some() as usize;
            mid += fast.is_some_and(|s| s > 0.02 && s < 0.98) as usize;
        }
    }
    // the test means something only if the answer is often a point of the axis and often nothing
    assert!(some > total / 50 && some < total - total / 50, "{some} of {total} capsules have a nearest point");
    assert!(mid > 200, "{mid} of them in the middle of the axis");
}
