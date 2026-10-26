//! Convex hulls as bodies: rocks and crates that fall, tumble, rest, stack and roll, against boxes, balls, capsules, other hulls, mesh levels and
//! terrain. The hulls are made from model files written by the tests (a few corners each).
use super::*;
use crate::meshcol_gltf::tests::glb_of;
use components::{AngularVelocity, Transform, Velocity};
use lunaris_json::{parse, Json};
use std::path::PathBuf;

type V3 = [f32; 3];

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn pos(s: &Sim, uid: u64) -> V3 {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn rot(s: &Sim, uid: u64) -> [f32; 4] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().rot
}

fn lin(s: &Sim, uid: u64) -> V3 {
    s.world.get::<Velocity>(s.world.entity(uid).unwrap()).unwrap().lin
}

fn spin(s: &Sim, uid: u64) -> V3 {
    s.world.get::<AngularVelocity>(s.world.entity(uid).unwrap()).unwrap().w
}

fn len(v: V3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn qz(a: f32) -> [f32; 4] {
    [0.0, 0.0, (a * 0.5).sin(), (a * 0.5).cos()]
}

fn axes(q: [f32; 4]) -> [V3; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    [[1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y)], [2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)], [2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y)]]
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("lunaris-hullphys-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The triangles of the solid whose corners are `points` (every three corners that have all the others on one side of them).
fn solid(points: &[V3]) -> Vec<[V3; 3]> {
    let sub = |a: V3, b: V3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut out = Vec::new();
    for i in 0..points.len() {
        for j in i + 1..points.len() {
            for k in j + 1..points.len() {
                let n = cross(sub(points[j], points[i]), sub(points[k], points[i]));
                if dot(n, n) < 1e-9 {
                    continue;
                }
                let (mut above, mut below) = (false, false);
                for (m, p) in points.iter().enumerate() {
                    if m == i || m == j || m == k {
                        continue;
                    }
                    let d = dot(n, sub(*p, points[i]));
                    above |= d > 1e-6;
                    below |= d < -1e-6;
                }
                if !(above && below) {
                    out.push(if above { [points[i], points[k], points[j]] } else { [points[i], points[j], points[k]] });
                }
            }
        }
    }
    out
}

fn cube(h: V3) -> Vec<V3> {
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

/// A square pyramid: base 1 x 1 at y = 0, the tip 1 up.
fn pyramid() -> Vec<V3> {
    vec![[-0.5, 0.0, -0.5], [0.5, 0.0, -0.5], [0.5, 0.0, 0.5], [-0.5, 0.0, 0.5], [0.0, 1.0, 0.0]]
}

/// A rock: an icosahedron stretched a little.
fn rock() -> Vec<V3> {
    let t = (1.0 + 5.0f32.sqrt()) / 2.0;
    let mut v = Vec::new();
    for (a, b) in [(-1.0f32, t), (1.0, t), (-1.0, -t), (1.0, -t)] {
        v.push([a, b, 0.0]);
        v.push([0.0, a, b]);
        v.push([b, 0.0, a]);
    }
    v.iter().map(|p| [p[0] * 0.25, p[1] * 0.2, p[2] * 0.3]).collect()
}

/// A world with the floor (a big box, top at y = 0) as entity 1 and the models written to a folder of its own.
fn scene(name: &str, models: &[(&str, Vec<V3>)]) -> Sim {
    let d = dir(name);
    for (file, pts) in models {
        std::fs::write(d.join(file), glb_of(&solid(pts))).unwrap();
    }
    let mut s = Sim::new(1);
    s.threads = 1;
    s.asset_root = Some(d);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[200,1,200]},"RigidBody":{"friction":0.8,"restitution":0}}"#);
    s
}

fn hull_body(s: &mut Sim, uid: u64, model: &str, p: V3, q: [f32; 4], scale: V3, rigid: &str) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":{p:?},"rot":{q:?},"scale":{scale:?}}},"Velocity":{{}},"AngularVelocity":{{"damping":0.1}},"ConvexCollider":{{"mesh":"{model}"}},"RigidBody":{rigid}}}"#));
}

const MAT: &str = r#"{"mass":1,"friction":0.6,"restitution":0}"#;

#[test]
fn a_cube_shaped_hull_falls_lands_flat_and_rests_exactly_like_a_box() {
    let mut s = scene("cube", &[("cube.glb", cube([0.5; 3]))]);
    hull_body(&mut s, 2, "cube.glb", [0.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3], MAT);
    spawn(&mut s, 3, &format!(r#"{{"Transform":{{"pos":[3,2,0]}},"Velocity":{{}},"AngularVelocity":{{"damping":0.1}},"Collider":{{"shape":1,"size":[1,1,1]}},"RigidBody":{MAT}}}"#));
    for _ in 0..180 {
        s.step();
    }
    let (h, b) = (pos(&s, 2), pos(&s, 3));
    assert!((h[1] - 0.5).abs() < 0.01 && (b[1] - 0.5).abs() < 0.01, "both rest on the floor: {h:?} {b:?}");
    assert!(len(lin(&s, 2)) < 0.02 && len(spin(&s, 2)) < 0.05, "and are still: {:?} {:?}", lin(&s, 2), spin(&s, 2));
    assert!(h[0].abs() < 0.01 && h[2].abs() < 0.01, "{h:?}");
    assert!(rot(&s, 2)[0].abs() < 0.01 && rot(&s, 2)[2].abs() < 0.01 && (rot(&s, 2)[3].abs() - 1.0).abs() < 0.001, "it has not turned: {:?}", rot(&s, 2));
}

#[test]
fn a_stack_of_hull_cubes_stays_a_stack_and_so_does_one_that_is_off_centre_in_its_model() {
    let mut s = scene("stack", &[("cube.glb", cube([0.5; 3])), ("off.glb", cube([0.5; 3]).iter().map(|p| [p[0] + 2.0, p[1], p[2] - 1.0]).collect())]);
    for i in 0..4u64 {
        hull_body(&mut s, 2 + i, "cube.glb", [0.0, 0.5 + i as f32 * 1.002, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3], r#"{"mass":1,"friction":0.7,"restitution":0}"#);
    }
    // a cube whose model is 2 m to the side of its origin: the entity is where the origin is
    for i in 0..3u64 {
        hull_body(&mut s, 10 + i, "off.glb", [10.0, 0.5 + i as f32 * 1.002, 5.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3], r#"{"mass":1,"friction":0.7,"restitution":0}"#);
    }
    s.step_n(60 * 6);
    for i in 0..4u64 {
        let p = pos(&s, 2 + i);
        assert!(p[0].abs() < 0.05 && p[2].abs() < 0.05 && (p[1] - (0.5 + i as f32)).abs() < 0.06, "box {i} at {p:?}");
    }
    for i in 0..3u64 {
        // (the centre of mass is 2 m east and 1 m north of the origin, at the height of the cube's middle: the entity's y is the cube's y)
        let p = pos(&s, 10 + i);
        assert!((p[0] - 10.0).abs() < 0.06 && (p[2] - 5.0).abs() < 0.06 && (p[1] - (0.5 + i as f32)).abs() < 0.06, "off-centre {i} at {p:?}");
        assert!(len(spin(&s, 10 + i)) < 0.2, "{:?}", spin(&s, 10 + i));
    }
}

#[test]
fn a_pyramid_dropped_on_its_tip_falls_over_and_comes_to_rest_on_a_face() {
    let mut s = scene("pyramid", &[("pyramid.glb", pyramid())]);
    // upside down, so that it lands on its tip
    hull_body(&mut s, 2, "pyramid.glb", [0.0, 3.0, 0.0], qz(std::f32::consts::PI - 0.1), [1.0; 3], MAT);
    s.step_n(60 * 6);
    let (p, q) = (pos(&s, 2), rot(&s, 2));
    assert!(len(lin(&s, 2)) < 0.05 && len(spin(&s, 2)) < 0.2, "it has come to rest: {:?} {:?}", lin(&s, 2), spin(&s, 2));
    // lying on its base (the model's origin is the middle of the base: the entity is 0 m up) or on a sloping side
    let a = axes(q);
    let up = |v: V3| v[1]; // the world height of a model direction
    let base_down = up(a[1]) > 0.99; // the model's +y (the tip) points up
    let side_down = a.iter().any(|ax| ax[1].abs() > 0.4 && ax[1].abs() < 0.99);
    assert!(base_down || side_down, "it lies on a face: {q:?}");
    assert!(p[1] > -0.01 && p[1] < 1.0, "{p:?}");
    assert!(p[0].abs() < 3.0 && p[2].abs() < 3.0);
}

#[test]
fn hulls_collide_with_boxes_balls_capsules_and_each_other_and_none_of_them_sinks_in() {
    let mut s = scene("mixed", &[("rock.glb", rock()), ("cube.glb", cube([0.4; 3]))]);
    hull_body(&mut s, 2, "rock.glb", [0.0, 1.0, 0.0], qz(0.3), [1.0; 3], MAT);
    hull_body(&mut s, 3, "rock.glb", [0.1, 2.5, 0.05], qz(1.1), [1.5; 3], MAT);
    hull_body(&mut s, 4, "cube.glb", [0.0, 4.0, 0.0], qz(0.2), [1.0; 3], MAT);
    spawn(&mut s, 5, &format!(r#"{{"Transform":{{"pos":[0.1,6,0]}},"Velocity":{{}},"AngularVelocity":{{"damping":0.1}},"Collider":{{"shape":0,"size":[0.5,0.5,0.5]}},"RigidBody":{MAT}}}"#));
    spawn(&mut s, 6, &format!(r#"{{"Transform":{{"pos":[0.0,8,0.1]}},"Velocity":{{}},"AngularVelocity":{{"damping":0.1}},"Collider":{{"shape":2,"size":[0.4,1.2,0.4]}},"RigidBody":{MAT}}}"#));
    spawn(&mut s, 7, &format!(r#"{{"Transform":{{"pos":[0.0,10,0.0]}},"Velocity":{{}},"AngularVelocity":{{"damping":0.1}},"Collider":{{"shape":1,"size":[0.8,0.5,0.8]}},"RigidBody":{MAT}}}"#));
    let mut lowest = [9.0f32; 6];
    for _ in 0..60 * 8 {
        s.step();
        for (k, uid) in (2u64..=7).enumerate() {
            lowest[k] = lowest[k].min(pos(&s, uid)[1]);
        }
    }
    for (k, uid) in (2u64..=7).enumerate() {
        let (p, v) = (pos(&s, uid), lin(&s, uid));
        // (the ball and the capsule may roll far away: nothing on a floor has rolling resistance)
        let room = if uid == 5 || uid == 6 { 60.0 } else { 6.0 };
        assert!(p.iter().all(|c| c.is_finite()) && p[1] > 0.0 && p[1] < 3.0 && p[0].abs() < room && p[2].abs() < room, "body {uid} at {p:?}");
        assert!(lowest[k] > -0.01, "body {uid} went into the floor: {}", lowest[k]);
        assert!(len(v) < if uid == 5 || uid == 6 { 4.0 } else { 0.3 }, "body {uid} settles: {v:?}");
    }
    // nobody stands inside another: the hulls are at least their inner radius apart
    for (a, b) in [(2u64, 3u64), (2, 4), (3, 4)] {
        let d = len([pos(&s, a)[0] - pos(&s, b)[0], pos(&s, a)[1] - pos(&s, b)[1], pos(&s, a)[2] - pos(&s, b)[2]]);
        assert!(d > 0.3, "{a} and {b} are {d} apart");
    }
}

#[test]
fn a_hull_on_a_slope_holds_with_enough_friction_and_slides_without() {
    let slope = |mu: f32| {
        let mut s = Sim::new(1);
        s.threads = 1;
        let d = dir(&format!("slope{mu}"));
        std::fs::write(d.join("cube.glb"), glb_of(&solid(&cube([0.5; 3])))).unwrap();
        s.asset_root = Some(d);
        let a = -20.0f32.to_radians();
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,0,0],"rot":{:?}}},"Collider":{{"shape":1,"size":[60,1,20]}},"RigidBody":{{"friction":{mu},"restitution":0}}}}"#, qz(a)));
        let (lx, ly) = (-5.0f32, 1.0f32);
        let p = [lx * a.cos() - ly * a.sin(), lx * a.sin() + ly * a.cos() + 0.002, 0.0];
        hull_body(&mut s, 2, "cube.glb", p, qz(a), [1.0; 3], &format!(r#"{{"mass":1,"friction":{mu},"restitution":0}}"#));
        s.step_n(60);
        (lin(&s, 2), pos(&s, 2), p)
    };
    let (v, p, start) = slope(0.8);
    assert!(len(v) < 0.1 && len([p[0] - start[0], p[1] - start[1], p[2] - start[2]]) < 0.15, "it holds: {v:?} {p:?} (from {start:?})");
    let (v, _, _) = slope(0.1);
    assert!(v[0] > 1.9 && v[0] < 2.7, "it slides, g (sin 20 - 0.1 cos 20) = 2.4 m/s^2: {v:?}");
}

#[test]
fn a_ball_that_hits_a_hull_bounces_off_it_and_a_hull_stays_put_when_it_is_static() {
    let mut s = scene("ballhit", &[("rock.glb", rock()), ("cube.glb", cube([0.5; 3]))]);
    // a static hull (no RigidBody mass, no Velocity): a block 1 m up the line of a ball that rolls at it
    spawn(&mut s, 2, r#"{"Transform":{"pos":[5,0.5,0]},"ConvexCollider":{"mesh":"cube.glb"},"RigidBody":{"friction":0.5,"restitution":0.5}}"#);
    spawn(&mut s, 3, &format!(r#"{{"Transform":{{"pos":[0,0.25,0]}},"Velocity":{{"lin":[6,0,0]}},"AngularVelocity":{{"damping":0}},"Collider":{{"shape":0,"size":[0.5,0.5,0.5]}},"RigidBody":{{"mass":1,"friction":0.3,"restitution":0.5}}}}"#));
    s.step_n(120);
    let b = pos(&s, 3);
    assert!(b[0] < 4.3, "the ball did not pass through the static block: {b:?}");
    assert!(lin(&s, 3)[0] < 0.0, "it bounced back: {:?}", lin(&s, 3));
    assert_eq!(pos(&s, 2), [5.0, 0.5, 0.0], "and the block did not move");
}

#[test]
fn a_hull_has_the_inertia_of_its_shape_so_a_tumbling_one_turns_like_the_same_box() {
    let (mut h, mut b) = (scene("tumble_h", &[("slab.glb", cube([0.5, 1.0, 1.5]))]), scene("tumble_b", &[]));
    for s in [&mut h, &mut b] {
        spawn(s, 9, r#"{"PhysicsSettings":{"gravity":[0,0,0]}}"#);
    }
    spawn(&mut h, 2, r#"{"Transform":{"pos":[0,20,0]},"Velocity":{},"AngularVelocity":{"w":[2.0,0.05,0.4],"damping":0},"ConvexCollider":{"mesh":"slab.glb"},"RigidBody":{"mass":3}}"#);
    spawn(&mut b, 2, r#"{"Transform":{"pos":[0,20,0]},"Velocity":{},"AngularVelocity":{"w":[2.0,0.05,0.4],"damping":0},"Collider":{"shape":1,"size":[1,2,3]},"RigidBody":{"mass":3}}"#);
    h.step_n(300);
    b.step_n(300);
    // the same angular momentum, the same wobble: the same orientation (to a small error, the two inertia tensors are worked out differently)
    let (qh, qb) = (rot(&h, 2), rot(&b, 2));
    let dot = (qh[0] * qb[0] + qh[1] * qb[1] + qh[2] * qb[2] + qh[3] * qb[3]).abs();
    assert!(dot > 0.999, "{qh:?} against {qb:?}: {dot}");
    assert!(len(spin(&h, 2)) > 1.0, "it is still tumbling: {:?}", spin(&h, 2));
}

#[test]
fn hulls_land_on_a_mesh_level_and_roll_down_its_ramp_and_do_not_fall_through() {
    let mut level = Vec::new();
    let mut quad = |a: V3, b: V3, c: V3, d: V3| level.extend([[a, b, c], [a, c, d]]);
    quad([-40.0, 0.0, -40.0], [-40.0, 0.0, 40.0], [40.0, 0.0, 40.0], [40.0, 0.0, -40.0]);
    // a ramp 15 degrees: from (5, 0) up to (15, 2.68), facing up
    let rise = 10.0 * 15.0f32.to_radians().tan();
    quad([5.0, 0.0, -3.0], [5.0, 0.0, 3.0], [15.0, rise, 3.0], [15.0, rise, -3.0]);
    let d = dir("meshlevel");
    std::fs::write(d.join("level.glb"), glb_of(&level)).unwrap();
    std::fs::write(d.join("cube.glb"), glb_of(&solid(&cube([0.5; 3])))).unwrap();
    std::fs::write(d.join("rock.glb"), glb_of(&solid(&rock()))).unwrap();
    let mut s = Sim::new(1);
    s.threads = 1;
    s.asset_root = Some(d);
    spawn(&mut s, 1, r#"{"Transform":{},"MeshCollider":{"mesh":"level.glb","friction":0.5,"restitution":0}}"#);
    hull_body(&mut s, 2, "cube.glb", [0.0, 3.0, 0.0], qz(0.5), [1.0; 3], MAT);
    hull_body(&mut s, 3, "rock.glb", [-4.0, 6.0, 0.0], qz(0.2), [1.0; 3], MAT);
    // a smooth hull (a 16-sided drum lying on its side) on the ramp, which rolls
    let mut drum = Vec::new();
    for k in 0..16 {
        let a = std::f32::consts::TAU * k as f32 / 16.0;
        for z in [-0.4f32, 0.4] {
            drum.push([0.5 * a.cos(), 0.5 * a.sin(), z]);
        }
    }
    std::fs::write(s.asset_root.clone().unwrap().join("drum.glb"), glb_of(&solid(&drum))).unwrap();
    let th = 15.0f32.to_radians();
    hull_body(&mut s, 4, "drum.glb", [7.0, (rise / 10.0) * 3.0 + 0.55, 0.0], qz(-th), [1.0; 3], r#"{"mass":1,"friction":0.8,"restitution":0}"#);
    let mut lowest = 99.0f32;
    for _ in 0..60 * 5 {
        s.step();
        for uid in [2u64, 3, 4] {
            lowest = lowest.min(pos(&s, uid)[1]);
        }
    }
    assert!(lowest > -0.05, "nothing went through the level: {lowest}");
    assert!((pos(&s, 2)[1] - 0.5).abs() < 0.01 && len(lin(&s, 2)) < 0.05, "the cube rests on the floor: {:?}", pos(&s, 2));
    let rock_y = pos(&s, 3)[1];
    assert!(rock_y > 0.1 && rock_y < 0.4 && len(lin(&s, 3)) < 0.05, "the rock rests on it: {:?}", pos(&s, 3));
    // the drum rolled down the ramp (towards -x, off it onto the floor) with a spin about z
    assert!(pos(&s, 4)[0] < 6.0, "{:?}", pos(&s, 4));
}

#[test]
fn hulls_are_deterministic_whatever_the_order_the_threads_or_a_snapshot() {
    let build = |reverse: bool, threads: usize| {
        let mut s = scene("det", &[("rock.glb", rock()), ("cube.glb", cube([0.4; 3])), ("pyramid.glb", pyramid())]);
        s.threads = threads;
        let mut uids: Vec<u64> = (0..24).collect();
        if reverse {
            uids.reverse();
        }
        for i in uids {
            let (gx, gz, gy) = ((i % 4) as f32 * 1.3 - 2.0, ((i / 4) % 3) as f32 * 1.3 - 1.3, 1.0 + (i / 12) as f32 * 1.6);
            let model = ["rock.glb", "cube.glb", "pyramid.glb"][(i % 3) as usize];
            hull_body(&mut s, 100 + i, model, [gx, gy, gz], qz(0.3 * i as f32), [1.0, 1.0 + (i % 2) as f32 * 0.3, 1.0], r#"{"mass":1,"restitution":0.2,"friction":0.5}"#);
        }
        s.step_n(240);
        s
    };
    let a = build(false, 1);
    assert_eq!(a.state_hash(), build(true, 1).state_hash(), "spawn order changed the result");
    assert_eq!(a.state_hash(), build(false, 4).state_hash(), "thread count changed the result");
    let mut b = build(false, 1);
    let snap = b.clone();
    b.step_n(60);
    let mut c = snap;
    c.step_n(60);
    assert_eq!(b.state_hash(), c.state_hash());
    for i in 0..24u64 {
        let p = pos(&a, 100 + i);
        assert!(p.iter().all(|v| v.is_finite()) && p[1] > -0.1 && p[1] < 5.0 && p[0].abs() < 8.0 && p[2].abs() < 8.0, "body {i} at {p:?}");
    }
}

#[test]
fn characters_stop_at_a_static_hull_agents_plan_round_it_and_sensors_cannot_see_through_it() {
    let mut s = scene("walkers", &[("cube.glb", cube([0.5; 3]))]);
    // a block 2 m wide (the cube stretched), standing on the floor at x = 5
    spawn(&mut s, 2, r#"{"Transform":{"pos":[5,0.5,0],"scale":[1,1,4]},"ConvexCollider":{"mesh":"cube.glb"}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,0,0]},"CharacterBody":{}}"#);
    s.step_n(5);
    s.world.get_mut::<components::CharacterBody>(s.world.entity(3).unwrap()).unwrap().walk = [3.0, 0.0];
    s.step_n(120);
    let p = pos(&s, 3);
    assert!((p[0] - (5.0 - 0.5 - 0.35)).abs() < 0.03 && p[1].abs() < 0.01, "stopped by the block (a box round the hull): {p:?}");
    // the planner goes round it: the block is 4 m across in z, so the way is longer than the straight line
    let r = crate::nav::find_path_with(&mut s.world, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0], crate::nav::NavParams::default(), &s.heightfields, &s.meshes).expect("a way round");
    assert!(r.length > 11.0 && r.points.len() >= 3, "{:?}", r);
    // a sensor looking at something behind the block does not see it
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,0,0]},"Sensor":{"radius":30,"fov":360,"filter":"Velocity","los":true,"eye":0.5}}"#);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[10,0,0]},"Velocity":{}}"#);
    spawn(&mut s, 6, r#"{"Transform":{"pos":[-10,0,0]},"Velocity":{}}"#);
    s.step();
    assert_eq!(s.world.get::<components::Sensor>(s.world.entity(4).unwrap()).unwrap().seen.0, 6, "only the one on this side of the block is in sight");
}

#[test]
fn a_hull_whose_model_is_missing_or_flat_makes_no_body_and_a_fast_one_does_not_fall_through_a_mesh_floor() {
    let d = dir("flat");
    std::fs::write(d.join("flat.glb"), glb_of(&[[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], [[1.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]])).unwrap();
    std::fs::write(d.join("cube.glb"), glb_of(&solid(&cube([0.5; 3])))).unwrap();
    std::fs::write(d.join("floor.glb"), glb_of(&[[[-50.0, 0.0, -50.0], [-50.0, 0.0, 50.0], [50.0, 0.0, 50.0]], [[-50.0, 0.0, -50.0], [50.0, 0.0, 50.0], [50.0, 0.0, -50.0]]])).unwrap();
    let mut s = Sim::new(1);
    s.threads = 1;
    s.asset_root = Some(d);
    spawn(&mut s, 1, r#"{"Transform":{},"MeshCollider":{"mesh":"floor.glb"}}"#);
    hull_body(&mut s, 2, "flat.glb", [0.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3], MAT);
    hull_body(&mut s, 3, "missing.glb", [3.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0; 3], MAT);
    hull_body(&mut s, 4, "cube.glb", [6.0, 90.0, 0.0], qz(0.4), [1.0; 3], MAT);
    let mut lowest = 99.0f32;
    for _ in 0..60 * 6 {
        s.step();
        lowest = lowest.min(pos(&s, 4)[1]);
    }
    assert!(pos(&s, 2)[1] == 3.0 && pos(&s, 3)[1] == 3.0, "no body: they hang in the air where they were put (no gravity acts on what is not a body)");
    assert!(lowest > 0.3, "the cube dropped from 90 m (42 m/s: 0.7 m a tick) did not go through the floor: {lowest}");
    assert!(pos(&s, 4).iter().all(|v| v.is_finite()) && pos(&s, 4)[1] > 0.3 && pos(&s, 4)[1] < 40.0, "{:?}", pos(&s, 4));
}
