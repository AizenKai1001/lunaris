//! Building convex hulls: the corners, the faces, the mass, and what happens to points that make no sense.
use crate::hull::{Hull, MAX_HULL_VERTICES};

type V3 = [f32; 3];

fn corners(h: V3) -> Vec<V3> {
    let mut v = Vec::new();
    for x in [-1.0f32, 1.0] {
        for y in [-1.0f32, 1.0] {
            for z in [-1.0f32, 1.0] {
                v.push([x * h[0], y * h[1], z * h[2]]);
            }
        }
    }
    v
}

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn rotate(q: [f32; 4], v: V3) -> V3 {
    let u = [q[0], q[1], q[2]];
    let c = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let t = c(u, v);
    let t = [2.0 * t[0], 2.0 * t[1], 2.0 * t[2]];
    let k = c(u, t);
    [v[0] + q[3] * t[0] + k[0], v[1] + q[3] * t[1] + k[1], v[2] + q[3] * t[2] + k[2]]
}

/// A small fixed pseudo-random sequence.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }
}

/// Checks that every part of a hull fits with the others: a closed surface of polygons that stand outside the middle.
fn assert_sound(h: &Hull) {
    // Euler's rule for a solid without holes: corners - edges + faces = 2
    assert_eq!(h.verts.len() as i64 - h.edges.len() as i64 + h.faces.len() as i64, 2, "{} corners, {} edges, {} faces", h.verts.len(), h.edges.len(), h.faces.len());
    for e in &h.edges {
        let on = |f: u16, a: u16, b: u16| {
            let v = &h.faces[f as usize].verts;
            (0..v.len()).any(|k| v[k] == a && v[(k + 1) % v.len()] == b)
        };
        assert!(on(e.fa, e.a, e.b) && on(e.fb, e.b, e.a), "edge {e:?}");
    }
    for f in &h.faces {
        assert!(f.verts.len() >= 3 && (f.n[0] * f.n[0] + f.n[1] * f.n[1] + f.n[2] * f.n[2] - 1.0).abs() < 1e-4);
        assert!(f.d > 0.0, "the middle is inside every face: {f:?}");
        for &v in &f.verts {
            let p = h.verts[v as usize];
            assert!((f.n[0] * p[0] + f.n[1] * p[1] + f.n[2] * p[2] - f.d).abs() < 2e-4, "a corner off its face's plane");
        }
        // no corner is outside any face
        for p in &h.verts {
            assert!(f.n[0] * p[0] + f.n[1] * p[1] + f.n[2] * p[2] - f.d < 2e-4, "a corner outside a face");
        }
    }
}

#[test]
fn a_cube_has_eight_corners_six_square_faces_twelve_edges_and_the_mass_of_a_cube() {
    let h = Hull::build(&corners([0.5, 0.5, 0.5]), [1.0; 3]).unwrap();
    assert_eq!((h.verts.len(), h.faces.len(), h.edges.len()), (8, 6, 12));
    assert!(h.faces.iter().all(|f| f.verts.len() == 4));
    assert_sound(&h);
    assert!(near(h.volume, 1.0, 1e-5));
    // I = m (a^2 + b^2) / 12 = m / 6 about every axis of a unit cube
    assert!(h.inertia.iter().all(|&i| near(i, 1.0 / 6.0, 1e-5)), "{:?}", h.inertia);
    assert_eq!(h.com, [0.0; 3]);
    assert_eq!(h.frame, [0.0, 0.0, 0.0, 1.0], "a symmetric solid keeps the model's axes exactly");
    assert!(near(h.inner, 0.5, 1e-5) && near(h.radius, 0.75f32.sqrt(), 1e-5));
    assert_eq!((h.points, h.dropped), (8, 0.0));
}

#[test]
fn a_box_that_is_not_in_the_middle_gets_its_centre_of_mass_and_its_three_different_moments() {
    let pts: Vec<V3> = corners([0.5, 1.0, 1.5]).iter().map(|p| [p[0] + 5.0, p[1] - 2.0, p[2] + 1.0]).collect();
    let h = Hull::build(&pts, [1.0; 3]).unwrap();
    assert_sound(&h);
    assert!(near(h.volume, 6.0, 1e-4));
    assert!(h.com.iter().zip([5.0, -2.0, 1.0]).all(|(a, b)| near(*a, b, 1e-5)), "{:?}", h.com);
    // 1 x 2 x 3: (4 + 9) / 12, (1 + 9) / 12, (1 + 4) / 12, in the model's own axes (the tensor is diagonal already)
    assert!(near(h.inertia[0], 13.0 / 12.0, 1e-4) && near(h.inertia[1], 10.0 / 12.0, 1e-4) && near(h.inertia[2], 5.0 / 12.0, 1e-4), "{:?}", h.inertia);
    assert_eq!(h.frame, [0.0, 0.0, 0.0, 1.0]);
    // the corners are now measured from the centre of mass
    assert!(h.verts.iter().all(|v| near(v[0].abs(), 0.5, 1e-4) && near(v[1].abs(), 1.0, 1e-4) && near(v[2].abs(), 1.5, 1e-4)));
}

#[test]
fn a_box_turned_in_its_model_comes_out_in_its_principal_frame_and_the_frame_gets_back_to_the_model() {
    let a = 0.5f32; // about z by 1 radian
    let q = [0.0, 0.0, a.sin(), a.cos()];
    let pts: Vec<V3> = corners([0.5, 1.0, 1.5]).iter().map(|p| rotate(q, *p)).collect();
    let h = Hull::build(&pts, [1.0; 3]).unwrap();
    assert_sound(&h);
    let mut sorted = h.inertia;
    sorted.sort_by(f32::total_cmp);
    assert!(near(sorted[0], 5.0 / 12.0, 1e-4) && near(sorted[1], 10.0 / 12.0, 1e-4) && near(sorted[2], 13.0 / 12.0, 1e-4), "{:?}", h.inertia);
    assert!(h.frame[3] < 0.9999, "the principal axes are not the model's: {:?}", h.frame);
    // every corner of the hull, put back into the model's coordinates, is one of the points
    for v in &h.verts {
        let m = rotate(h.frame, *v);
        let m = [m[0] + h.com[0], m[1] + h.com[1], m[2] + h.com[2]];
        assert!(pts.iter().any(|p| (0..3).all(|k| near(p[k], m[k], 2e-4))), "{m:?} is not a point of the model");
    }
    // in its principal frame the box is lined up with the axes again: every corner is at (+-a, +-b, +-c) for some assignment of 0.5, 1, 1.5
    let mut ext = [0.0f32; 3];
    for v in &h.verts {
        for k in 0..3 {
            ext[k] = ext[k].max(v[k].abs());
        }
    }
    ext.sort_by(f32::total_cmp);
    assert!(near(ext[0], 0.5, 1e-4) && near(ext[1], 1.0, 1e-4) && near(ext[2], 1.5, 1e-4), "{ext:?}");
}

#[test]
fn a_regular_tetrahedron_has_four_triangles_six_edges_and_the_same_inertia_about_every_axis() {
    let pts = [[1.0, 1.0, 1.0], [1.0, -1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, -1.0, 1.0]];
    let h = Hull::build(&pts, [1.0; 3]).unwrap();
    assert_eq!((h.verts.len(), h.faces.len(), h.edges.len()), (4, 4, 6));
    assert_sound(&h);
    assert!(near(h.volume, 8.0 / 3.0, 1e-4), "{}", h.volume);
    assert!(near(h.inertia[0], h.inertia[1], 1e-4) && near(h.inertia[1], h.inertia[2], 1e-4), "{:?}", h.inertia);
}

#[test]
fn a_model_with_more_corners_than_the_limit_is_simplified_and_says_how_much_was_lost() {
    // 3000 points on a sphere of radius 1 (a Fibonacci spiral)
    let n = 3000;
    let golden = std::f64::consts::PI * (3.0 - 5.0f64.sqrt());
    let pts: Vec<V3> = (0..n)
        .map(|i| {
            let y = 1.0 - 2.0 * (i as f64 + 0.5) / n as f64;
            let r = (1.0 - y * y).sqrt();
            let t = golden * i as f64;
            [(r * t.cos()) as f32, y as f32, (r * t.sin()) as f32]
        })
        .collect();
    let h = Hull::build(&pts, [1.0; 3]).unwrap();
    assert_eq!(h.verts.len(), MAX_HULL_VERTICES, "the greedy hull stops at the limit");
    assert_sound(&h);
    assert_eq!(h.points, n);
    // a 64 corner solid inside a unit ball: most of its volume, and the points that were left out are close to it
    assert!(h.volume > 0.8 * 4.18879 * 0.85 && h.volume < 4.18879, "{}", h.volume);
    assert!(h.dropped > 0.0 && h.dropped < 0.12, "{}", h.dropped);
    // (the corners are measured from the hull's own centre of mass, which is a little off the middle of the sphere)
    assert!(h.verts.iter().all(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() < 1.02) && h.com.iter().all(|c| c.abs() < 0.02), "{:?}", h.com);
    assert!(h.inertia.iter().all(|&i| near(i, 0.4, 0.05)), "a ball: 2/5 r^2 per unit mass: {:?}", h.inertia);
}

#[test]
fn the_same_points_in_any_order_and_with_points_inside_give_the_same_hull() {
    let mut pts = corners([0.6, 0.4, 0.5]);
    let plain = Hull::build(&pts, [1.0; 3]).unwrap();
    let mut r = Lcg(3);
    // points inside the box, on its faces and edges, and the corners twice
    for _ in 0..200 {
        pts.push([(r.next() - 0.5) * 1.2, (r.next() - 0.5) * 0.8, (r.next() - 0.5) * 1.0]);
    }
    pts.push([0.6, 0.0, 0.0]);
    pts.push([0.0, 0.4, 0.2]);
    pts.extend(corners([0.6, 0.4, 0.5]));
    let a = Hull::build(&pts, [1.0; 3]).unwrap();
    assert_eq!(a.hash, plain.hash);
    for i in (1..pts.len()).rev() {
        let j = (r.next() * (i + 1) as f32) as usize % (i + 1);
        pts.swap(i, j);
    }
    assert_eq!(Hull::build(&pts, [1.0; 3]).unwrap().hash, plain.hash);
    assert_eq!(a.faces.len(), 6);
    // another shape gives another hash
    assert_ne!(Hull::build(&corners([0.6, 0.4, 0.51]), [1.0; 3]).unwrap().hash, plain.hash);
}

#[test]
fn a_scale_stretches_the_hull_and_a_mirror_keeps_it_a_solid() {
    let cube = corners([0.5; 3]);
    let h = Hull::build(&cube, [2.0, 1.0, 0.5]).unwrap();
    assert!(near(h.volume, 1.0, 1e-5));
    assert!(h.verts.iter().all(|v| near(v[0].abs(), 1.0, 1e-5) && near(v[1].abs(), 0.5, 1e-5) && near(v[2].abs(), 0.25, 1e-5)));
    let mirrored = Hull::build(&cube, [-1.0, 1.0, 1.0]).unwrap();
    assert_sound(&mirrored);
    assert!(near(mirrored.volume, 1.0, 1e-5));
}

#[test]
fn points_that_do_not_make_a_solid_are_refused_with_a_reason() {
    let e = |p: &[V3], s: V3| Hull::build(p, s).err().unwrap_or_else(|| "(accepted)".to_string());
    assert!(e(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], [1.0; 3]).contains("at least four"));
    assert!(e(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]], [1.0; 3]).contains("one plane"));
    assert!(e(&[[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [3.0, 0.0, 0.0]], [1.0; 3]).contains("line"));
    assert!(e(&corners([0.5; 3]), [1.0, 0.0, 1.0]).contains("plane") || e(&corners([0.5; 3]), [1.0, 0.0, 1.0]).contains("line") || e(&corners([0.5; 3]), [1.0, 0.0, 1.0]).contains("four"));
    assert!(e(&[[f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [1.0; 3]).contains("not numbers"));
    assert!(e(&corners([0.5; 3]), [f32::INFINITY, 1.0, 1.0]).contains("not numbers"));
    assert!(e(&corners([1.0e30; 3]), [1.0; 3]).contains("too big"));
    assert!(e(&[], [1.0; 3]).contains("at least four"));
    // a sliver is a solid, if a thin one
    let sliver = Hull::build(&corners([1.0, 1.0, 0.001]), [1.0; 3]).unwrap();
    assert_sound(&sliver);
}

#[test]
fn rays_hit_the_faces_of_a_hull_from_outside_and_the_far_side_from_inside() {
    let h = Hull::build(&corners([0.5; 3]), [1.0; 3]).unwrap();
    let (t, n) = h.ray_local([0.2, 0.1, 3.0], [0.0, 0.0, -1.0], 100.0).unwrap();
    assert!(near(t, 2.5, 1e-5) && n == [0.0, 0.0, 1.0], "{t} {n:?}");
    let (t, n) = h.ray_local([-3.0, 0.2, 0.1], [1.0, 0.0, 0.0], 100.0).unwrap();
    assert!(near(t, 2.5, 1e-5) && n == [-1.0, 0.0, 0.0]);
    assert!(h.ray_local([0.2, 0.1, 3.0], [0.0, 0.0, -1.0], 2.0).is_none(), "too short");
    assert!(h.ray_local([0.8, 0.1, 3.0], [0.0, 0.0, -1.0], 100.0).is_none(), "beside it");
    assert!(h.ray_local([0.2, 0.1, 3.0], [0.0, 0.0, 1.0], 100.0).is_none(), "pointing away");
    assert!(h.ray_local([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 100.0).is_some_and(|(t, n)| near(t, 0.5, 1e-5) && n == [0.0, -1.0, 0.0]), "from inside: where it leaves, the normal faces the ray");
    assert!(h.ray_local([f32::NAN, 0.0, 3.0], [0.0, 0.0, -1.0], 100.0).is_none());
}
