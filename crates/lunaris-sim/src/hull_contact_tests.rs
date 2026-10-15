//! The contact of convex solids on their own: faces on faces, edges across edges, corners on faces, balls and capsules against a hull, a hull
//! on the triangles of a floor. The solids are placed by hand, no simulation runs.
use crate::components::Transform;
use crate::hull::Hull;
use crate::hull_contact::{capsule_poly, poly_poly, sphere_poly, Poly};
use crate::meshcol::{CollisionMesh, PlacedMesh};
use std::sync::Arc;
use crate::meshcol_shapes::MeshPt;

type V3 = [f32; 3];

const ID: [V3; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn cube_points(h: V3) -> Vec<V3> {
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

fn cube(h: V3) -> Hull {
    Hull::build(&cube_points(h), [1.0; 3]).unwrap()
}

/// A square pyramid, base 1 x 1 at y = 0 (the model's origin is the middle of its base), the tip 1 up.
fn pyramid() -> Hull {
    Hull::build(&[[-0.5, 0.0, -0.5], [0.5, 0.0, -0.5], [0.5, 0.0, 0.5], [-0.5, 0.0, 0.5], [0.0, 1.0, 0.0]], [1.0; 3]).unwrap()
}

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn near3(a: V3, b: V3, tol: f32) -> bool {
    (0..3).all(|k| near(a[k], b[k], tol))
}

fn about_z(a: f32) -> [V3; 3] {
    let (s, c) = (a.sin(), a.cos());
    [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]]
}

fn about_x(a: f32) -> [V3; 3] {
    let (s, c) = (a.sin(), a.cos());
    [[1.0, 0.0, 0.0], [0.0, c, s], [0.0, -s, c]]
}

/// A hull placed the way an entity with this `Transform` places it (the model's origin at `pos`).
fn placed(h: &Hull, pos: V3, q: [f32; 4]) -> Poly<'_> {
    let (centre, qq) = h.pose(&Transform { pos, rot: q, scale: [1.0; 3] });
    Poly::from_hull(h, centre, &crate::physics_rot::axes_of(qq))
}

#[test]
fn two_cubes_face_to_face_touch_at_four_points_with_the_normal_from_the_first_to_the_second() {
    let h = cube([0.5; 3]);
    let a = Poly::from_hull(&h, [0.0, 0.0, 0.0], &ID);
    let b = Poly::from_hull(&h, [0.0, 0.98, 0.0], &ID);
    let pts = poly_poly(&a, &b);
    assert_eq!(pts.len(), 4, "{pts:?}");
    assert!(pts.iter().all(|p| near3(p.n, [0.0, 1.0, 0.0], 1e-5) && near(p.depth, 0.02, 1e-4) && near(p.p[1], 0.49, 1e-3)), "{pts:?}");
    // one point at each corner of the overlap
    let xs: Vec<f32> = pts.iter().map(|p| p.p[0]).collect();
    assert!(xs.iter().any(|&x| x > 0.45) && xs.iter().any(|&x| x < -0.45), "{xs:?}");
    // swapped: the normal points the other way
    let back = poly_poly(&b, &a);
    assert_eq!(back.len(), 4);
    assert!(back.iter().all(|p| near3(p.n, [0.0, -1.0, 0.0], 1e-5)));
    // apart, and far away
    assert!(poly_poly(&a, &Poly::from_hull(&h, [0.0, 1.05, 0.0], &ID)).is_empty());
    assert!(poly_poly(&a, &Poly::from_hull(&h, [9.0, 0.0, 0.0], &ID)).is_empty());
    // slid half off: the overlap is the half face over the first cube
    let slid = poly_poly(&a, &Poly::from_hull(&h, [0.5, 0.98, 0.0], &ID));
    assert_eq!(slid.len(), 4, "{slid:?}");
    assert!(slid.iter().all(|p| p.p[0] > -0.01 && p.p[0] < 0.51 && near3(p.n, [0.0, 1.0, 0.0], 1e-5)), "{slid:?}");
    // side by side: the normal is along x
    let side = poly_poly(&a, &Poly::from_hull(&h, [0.97, 0.0, 0.0], &ID));
    assert_eq!(side.len(), 4);
    assert!(side.iter().all(|p| near3(p.n, [1.0, 0.0, 0.0], 1e-5) && near(p.depth, 0.03, 1e-4)), "{side:?}");
}

#[test]
fn a_cube_on_an_edge_touches_along_the_edge_and_two_edges_across_each_other_touch_at_one_point() {
    let h = cube([0.5; 3]);
    let a = Poly::from_hull(&h, [0.0, 0.0, 0.0], &ID);
    // turned 45 degrees about z, standing on an edge that runs along z: the points lie along the edge
    let r = std::f32::consts::FRAC_1_SQRT_2;
    let b = Poly::from_hull(&h, [0.0, 0.5 + r - 0.02, 0.0], &about_z(std::f32::consts::FRAC_PI_4));
    let pts = poly_poly(&a, &b);
    assert!(!pts.is_empty() && pts.iter().all(|p| near3(p.n, [0.0, 1.0, 0.0], 1e-3) && near(p.depth, 0.02, 2e-3)), "{pts:?}");
    assert!(pts.iter().any(|p| p.p[2] > 0.4) && pts.iter().any(|p| p.p[2] < -0.4), "along the whole edge: {pts:?}");
    // a real crossing: cube A turned 45 degrees about z (its top edge runs along z, at height 0.707), cube B turned 45 degrees about x (its
    // low edge runs along x): B lies across A's top edge
    let top = Poly::from_hull(&h, [0.0, 0.0, 0.0], &about_z(std::f32::consts::FRAC_PI_4));
    let over = Poly::from_hull(&h, [0.0, 2.0 * r - 0.03, 0.0], &about_x(std::f32::consts::FRAC_PI_4));
    let cross = poly_poly(&top, &over);
    assert_eq!(cross.len(), 1, "two edges that cross touch at one point: {cross:?}");
    assert!(near3(cross[0].n, [0.0, 1.0, 0.0], 1e-3) && near(cross[0].depth, 0.03, 3e-3) && cross[0].p[0].abs() < 0.01 && cross[0].p[2].abs() < 0.01, "{:?}", cross[0]);
    // and the same pair the other way round
    let flipped = poly_poly(&over, &top);
    assert_eq!(flipped.len(), 1);
    assert!(near3(flipped[0].n, [0.0, -1.0, 0.0], 1e-3), "{:?}", flipped[0]);
}

#[test]
fn a_pyramid_touches_a_floor_with_its_whole_base_or_with_its_tip() {
    let p = pyramid();
    let floor = cube([5.0, 0.5, 5.0]);
    let fl = Poly::from_hull(&floor, [0.0, -0.5, 0.0], &ID);
    // standing on its base (sunk 1 cm): four points under the base's corners
    let base = poly_poly(&fl, &placed(&p, [0.0, -0.01, 0.0], [0.0, 0.0, 0.0, 1.0]));
    assert!(base.len() == 4 && base.iter().all(|c| near3(c.n, [0.0, 1.0, 0.0], 1e-4) && near(c.depth, 0.01, 1e-3)), "{base:?}");
    // upside down (turned a half turn about z), the tip 2 cm into the floor: one point, under the tip
    let tip = poly_poly(&fl, &placed(&p, [0.0, 1.0 - 0.02, 0.0], [0.0, 0.0, 1.0, 0.0]));
    assert_eq!(tip.len(), 1, "{tip:?}");
    assert!(near3(tip[0].n, [0.0, 1.0, 0.0], 1e-3) && near(tip[0].depth, 0.02, 2e-3) && tip[0].p[0].abs() < 0.01 && tip[0].p[2].abs() < 0.01, "{:?}", tip[0]);
    // the other way round the normal points down
    let flipped = poly_poly(&placed(&p, [0.0, 1.0 - 0.02, 0.0], [0.0, 0.0, 1.0, 0.0]), &fl);
    assert!(flipped.len() == 1 && near3(flipped[0].n, [0.0, -1.0, 0.0], 1e-3));
}

#[test]
fn a_ball_against_a_hull_is_pushed_along_the_face_the_edge_the_corner_or_out_through_the_nearest_face() {
    let h = cube([0.5; 3]);
    let c = Poly::from_hull(&h, [0.0, 0.0, 0.0], &ID);
    // over a face
    let p = sphere_poly(&c, [0.1, 0.7, 0.2], 0.3).unwrap();
    assert!(near3(p.n, [0.0, 1.0, 0.0], 1e-5) && near(p.depth, 0.1, 1e-5) && p.face, "{p:?}");
    // over an edge (the edge between the +x and +y faces): the push points away from the edge, at 45 degrees
    let p = sphere_poly(&c, [0.5 + 0.2, 0.5 + 0.2, 0.1], 0.35).unwrap();
    let s = std::f32::consts::FRAC_1_SQRT_2;
    assert!(near3(p.n, [s, s, 0.0], 1e-4) && near(p.depth, 0.35 - 0.2 * 2.0f32.sqrt(), 1e-4) && !p.face, "{p:?}");
    // over a corner
    let p = sphere_poly(&c, [0.7, 0.7, 0.7], 0.6).unwrap();
    let t = 1.0 / 3.0f32.sqrt();
    assert!(near3(p.n, [t, t, t], 1e-4) && !p.face, "{p:?}");
    // the centre inside: out through the nearest face (the +x face, 0.1 away), with the depth of the radius and the distance
    let p = sphere_poly(&c, [0.4, 0.1, 0.0], 0.25).unwrap();
    assert!(near3(p.n, [1.0, 0.0, 0.0], 1e-5) && near(p.depth, 0.35, 1e-5), "{p:?}");
    // clear of it, far from it, and nonsense
    assert!(sphere_poly(&c, [0.0, 0.9, 0.0], 0.3).is_none());
    assert!(sphere_poly(&c, [4.0, 4.0, 4.0], 0.3).is_none());
    assert!(sphere_poly(&c, [f32::NAN, 0.0, 0.0], 0.3).is_none() && sphere_poly(&c, [0.0, 0.7, 0.0], f32::NAN).is_none());
}

#[test]
fn a_capsule_against_a_hull_lies_on_it_with_two_points_and_rests_across_a_ridge_with_the_middle_of_its_axis() {
    let h = cube([0.5; 3]);
    let c = Poly::from_hull(&h, [0.0, 0.0, 0.0], &ID);
    // lying along x on the top face: a ball at each end
    let pts = capsule_poly(&c, [-0.3, 0.7, 0.0], [0.3, 0.7, 0.0], 0.25);
    assert_eq!(pts.len(), 2, "{pts:?}");
    assert!(pts.iter().all(|p| near3(p.n, [0.0, 1.0, 0.0], 1e-4) && near(p.depth, 0.05, 1e-4)));
    // a long pole across the top of the cube, both ends in the air: the middle of the pole rests on it
    let pole = capsule_poly(&c, [-1.5, 0.8, 0.0], [1.5, 0.8, 0.0], 0.35);
    assert!(pole.iter().any(|p| near3(p.n, [0.0, 1.0, 0.0], 1e-3) && p.p[0].abs() < 0.6 && near(p.depth, 0.05, 1e-3)), "{pole:?}");
    assert!(capsule_poly(&c, [-1.5, 2.0, 0.0], [1.5, 2.0, 0.0], 0.35).is_empty());
    assert!(capsule_poly(&c, [f32::NAN, 0.8, 0.0], [1.5, 0.8, 0.0], 0.35).len() <= 3);
}


/// A flat floor of sixteen triangles at y = 0, the seams running through the middle, as a mesh placed in the world.
fn floor_mesh() -> PlacedMesh {
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    for i in -2..2 {
        for j in -1..1 {
            let (x0, x1, z0, z1) = (i as f32, i as f32 + 1.0, j as f32, j as f32 + 1.0);
            let k = verts.len() as u32;
            verts.extend([[x0, 0.0, z0], [x0, 0.0, z1], [x1, 0.0, z0], [x1, 0.0, z1]]);
            tris.push([k, k + 1, k + 2]);
            tris.push([k + 2, k + 1, k + 3]);
        }
    }
    PlacedMesh::new(Arc::new(CollisionMesh::new(verts, tris).unwrap()), &Transform::default()).unwrap()
}

fn mesh_of(tris: &[[V3; 3]]) -> PlacedMesh {
    let verts: Vec<V3> = tris.iter().flatten().copied().collect();
    let idx: Vec<[u32; 3]> = (0..tris.len() as u32).map(|i| [3 * i, 3 * i + 1, 3 * i + 2]).collect();
    PlacedMesh::new(Arc::new(CollisionMesh::new(verts, idx).unwrap()), &Transform::default()).unwrap()
}

#[test]
fn a_hull_on_the_triangles_of_a_floor_is_held_at_its_corners_whatever_the_seams_and_goes_back_up_if_it_fell_through() {
    let floor = floor_mesh();
    let h = cube([0.5, 0.3, 0.4]);
    for (x, z) in [(0.0, 0.0), (0.37, 0.12), (-0.5, -0.5), (0.001, 0.3)] {
        let pts: Vec<MeshPt> = floor.poly(&Poly::from_hull(&h, [x, 0.298, z], &ID), [0.0; 3]);
        assert!(!pts.is_empty(), "no contact at ({x}, {z})");
        assert!(pts.iter().all(|c| near3(c.n, [0.0, 1.0, 0.0], 1e-4) && (c.depth - 0.002).abs() < 1e-3), "{pts:?}");
        // the four corners of the box are among the points (at most four are kept: they are the four corners)
        assert_eq!(pts.len(), 4, "{pts:?}");
        for (sx, sz) in [(1.0f32, 1.0f32), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            assert!(pts.iter().any(|c| near(c.p[0], x + sx * 0.5, 2e-3) && near(c.p[2], z + sz * 0.4, 2e-3)), "corner ({sx}, {sz}) at ({x}, {z}): {pts:?}");
        }
    }
    // clear of the floor: nothing
    assert!(floor.poly(&Poly::from_hull(&h, [0.0, 0.35, 0.0], &ID), [0.0; 3]).is_empty());
    // the centre went through the floor during the last tick: pushed back up, to the side it came from
    let pts = floor.poly(&Poly::from_hull(&h, [0.0, -0.05, 0.0], &ID), [0.0, -0.4, 0.0]);
    assert!(!pts.is_empty() && pts.iter().all(|c| near(c.n[1], 1.0, 1e-4)), "{pts:?}");
}

#[test]
fn pointed_solids_on_a_flat_floor_made_of_triangles_feel_no_ghosts_from_the_seams() {
    let floor = floor_mesh();
    let p = pyramid();
    // on its base
    let base = floor.poly(&placed(&p, [0.0, -0.01, 0.0], [0.0, 0.0, 0.0, 1.0]), [0.0; 3]);
    assert!(base.len() == 4 && base.iter().all(|c| near3(c.n, [0.0, 1.0, 0.0], 1e-4) && near(c.depth, 0.01, 1e-3)), "the base: {base:?}");
    // on its tip, which is right over a corner where six triangles meet, then over the middle of a seam, then over the inside of a triangle
    for (x, z) in [(0.0, 0.0), (0.5, 0.0), (0.0, 0.5), (0.3, 0.2), (-1.0, 0.0)] {
        let tip = floor.poly(&placed(&p, [x, 1.0 - 0.02, z], [0.0, 0.0, 1.0, 0.0]), [0.0; 3]);
        assert!(!tip.is_empty() && tip.iter().all(|c| near3(c.n, [0.0, 1.0, 0.0], 1e-3) && near(c.depth, 0.02, 2e-3)), "the tip over ({x}, {z}): {tip:?}");
    }
    // a cube on a corner (turned so that its body diagonal is vertical): the same
    let h = cube([0.5; 3]);
    let diag = 0.5 * 3.0f32.sqrt();
    // the turn that takes the corner (+, +, +) to straight down: about the axis (1,1,1) x (0,-1,0), by the angle between them
    let angle = (-1.0f32 / 3.0f32.sqrt()).acos();
    let axis = [1.0 / 2.0f32.sqrt(), 0.0, -1.0 / 2.0f32.sqrt()];
    let q = [axis[0] * (angle * 0.5).sin(), axis[1] * (angle * 0.5).sin(), axis[2] * (angle * 0.5).sin(), (angle * 0.5).cos()];
    for (x, z) in [(0.0, 0.0), (0.5, 0.0), (0.3, 0.2)] {
        let corner = floor.poly(&Poly::from_hull(&h, [x, diag - 0.02, z], &crate::physics_rot::axes_of(q)), [0.0; 3]);
        assert!(!corner.is_empty() && corner.iter().all(|c| near3(c.n, [0.0, 1.0, 0.0], 5e-3) && near(c.depth, 0.02, 4e-3)), "a corner over ({x}, {z}): {corner:?}");
    }
}

#[test]
fn at_the_rim_of_a_floor_and_against_a_wall_the_real_edge_pushes_sideways() {
    // one triangle only: a floor that ends. A pyramid whose tip is right at the rim is held by the rim: the way out is along its slope
    let one = mesh_of(&[[[0.0, 0.0, 0.0], [0.0, 0.0, 2.0], [2.0, 0.0, 0.0]]]);
    let p = pyramid();
    let tip = one.poly(&placed(&p, [0.004, 1.0 - 0.02, 0.5], [0.0, 0.0, 1.0, 0.0]), [0.0; 3]);
    assert!(!tip.is_empty(), "something holds it");
    assert!(tip.iter().any(|c| c.n[0] < -0.3), "at the open edge of the floor the push has a sideways part (no neighbour holds the tip): {tip:?}");
    // a wall (x = 0.4, facing -x) and a cube whose side is 0.03 into it
    let wall = mesh_of(&[[[0.4, 0.0, -2.0], [0.4, 3.0, 2.0], [0.4, 3.0, -2.0]], [[0.4, 0.0, -2.0], [0.4, 0.0, 2.0], [0.4, 3.0, 2.0]]]);
    let h = cube([0.5; 3]);
    let out = wall.poly(&Poly::from_hull(&h, [-0.07, 1.0, 0.0], &ID), [0.0; 3]);
    assert!(!out.is_empty() && out.iter().all(|c| near3(c.n, [-1.0, 0.0, 0.0], 1e-4) && near(c.depth, 0.03, 1e-3)), "{out:?}");
}

#[test]
fn nothing_odd_makes_a_hull_contact_panic() {
    let h = cube([0.5; 3]);
    let c = Poly::from_hull(&h, [0.0; 3], &ID);
    let floor = floor_mesh();
    for v in [f32::NAN, f32::INFINITY, 1.0e30, -1.0e30, 0.0] {
        let odd = Poly::from_hull(&h, [v, 0.5, 0.0], &ID);
        let _ = poly_poly(&c, &odd);
        let _ = poly_poly(&odd, &c);
        let _ = sphere_poly(&odd, [0.0, 0.6, 0.0], 0.3);
        let _ = capsule_poly(&odd, [0.0, 0.6, 0.0], [0.0, 1.6, 0.0], 0.3);
        let _ = floor.poly(&odd, [v, 0.0, 0.0]);
        let _ = floor.poly(&Poly::from_hull(&h, [0.0, 0.3, 0.0], &ID), [v, v, v]);
    }
    // a hull on top of itself: some answer, no panic
    let _ = poly_poly(&c, &Poly::from_hull(&h, [0.0; 3], &ID));
}
