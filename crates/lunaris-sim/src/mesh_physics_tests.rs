//! Mesh colliders in whole scenes: balls, boxes and capsules on levels made of triangles, characters walking on them, agents finding
//! their way over them, sensors looking through them. Every level is written as a real `.glb` file into a folder of the test's own and read
//! by the simulation like any other model.
use super::*;
use crate::meshcol_gltf::tests::glb_of;
use components::{AngularVelocity, CharacterBody, NavAgent, Transform, Velocity};
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

/// How well one of the body's axes points straight up or down (1 = exactly).
fn level(q: [f32; 4]) -> f32 {
    let a = axes(q);
    a[0][1].abs().max(a[1][1].abs()).max(a[2][1].abs())
}

// ------------------------------------------------------------------------------------------------ levels made of triangles

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dotp(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[derive(Default)]
struct Level {
    tris: Vec<[V3; 3]>,
}

impl Level {
    fn new() -> Level {
        Level::default()
    }

    /// A four-cornered piece whose front faces the way `want` points (the corners are given in order round it).
    fn quad(&mut self, a: V3, b: V3, c: V3, d: V3, want: V3) {
        let n = cross(sub(b, a), sub(c, a));
        if dotp(n, want) >= 0.0 {
            self.tris.push([a, b, c]);
            self.tris.push([a, c, d]);
        } else {
            self.tris.push([a, c, b]);
            self.tris.push([a, d, c]);
        }
    }

    /// A flat piece of ground facing up.
    fn floor(&mut self, x0: f32, x1: f32, z0: f32, z1: f32, y: f32) -> &mut Level {
        self.quad([x0, y, z0], [x0, y, z1], [x1, y, z1], [x1, y, z0], [0.0, 1.0, 0.0]);
        self
    }

    /// A wall across x = `x` from z0 to z1, y0 to y1, facing -x (one thin sheet).
    fn wall_x(&mut self, x: f32, z0: f32, z1: f32, y0: f32, y1: f32) -> &mut Level {
        self.quad([x, y0, z0], [x, y1, z0], [x, y1, z1], [x, y0, z1], [-1.0, 0.0, 0.0]);
        self
    }

    /// A closed block, six faces facing outwards.
    fn block(&mut self, lo: V3, hi: V3) -> &mut Level {
        let c = |x: usize, y: usize, z: usize| [if x == 0 { lo[0] } else { hi[0] }, if y == 0 { lo[1] } else { hi[1] }, if z == 0 { lo[2] } else { hi[2] }];
        self.quad(c(0, 1, 0), c(0, 1, 1), c(1, 1, 1), c(1, 1, 0), [0.0, 1.0, 0.0]);
        self.quad(c(0, 0, 0), c(0, 0, 1), c(1, 0, 1), c(1, 0, 0), [0.0, -1.0, 0.0]);
        self.quad(c(0, 0, 0), c(0, 1, 0), c(0, 1, 1), c(0, 0, 1), [-1.0, 0.0, 0.0]);
        self.quad(c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1), [1.0, 0.0, 0.0]);
        self.quad(c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 0, 0), [0.0, 0.0, -1.0]);
        self.quad(c(0, 0, 1), c(0, 1, 1), c(1, 1, 1), c(1, 0, 1), [0.0, 0.0, 1.0]);
        self
    }

    /// A ramp across z0..z1 that rises from height 0 at x = `x0` to height `h` at x = `x1` (x1 may be smaller: it then rises toward -x).
    fn ramp(&mut self, x0: f32, x1: f32, z0: f32, z1: f32, h: f32) -> &mut Level {
        self.quad([x0, 0.0, z0], [x0, 0.0, z1], [x1, h, z1], [x1, h, z0], [0.0, 1.0, 0.0]);
        self
    }

    fn save(&self, dir: &std::path::Path, name: &str) {
        std::fs::write(dir.join(name), glb_of(&self.tris)).unwrap();
    }
}

fn dir_for(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lunaris-meshphys-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A simulation with the level as entity 1 (a `MeshCollider` with the given friction and restitution).
fn scene(name: &str, level: &Level, friction: f32, restitution: f32) -> Sim {
    let dir = dir_for(name);
    level.save(&dir, "level.glb");
    let mut s = Sim::new(1);
    s.threads = 1;
    s.asset_root = Some(dir);
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{}},"MeshCollider":{{"mesh":"level.glb","friction":{friction},"restitution":{restitution}}}}}"#));
    s
}

fn big_floor() -> Level {
    let mut l = Level::new();
    l.floor(-60.0, 60.0, -60.0, 60.0, 0.0);
    l
}

/// A dynamic body that turns.
fn spinning(s: &mut Sim, uid: u64, shape: u32, size: V3, p: V3, q: [f32; 4], rigid: &str) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":{p:?},"rot":{q:?}}},"Velocity":{{}},"AngularVelocity":{{"damping":0}},"Collider":{{"shape":{shape},"size":{size:?}}},"RigidBody":{rigid}}}"#));
}

/// A dynamic body that does not turn.
fn plain(s: &mut Sim, uid: u64, shape: u32, size: V3, p: V3, rigid: &str) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":{p:?}}},"Velocity":{{}},"Collider":{{"shape":{shape},"size":{size:?}}},"RigidBody":{rigid}}}"#));
}

// ------------------------------------------------------------------------------------------------ rigid bodies

const THETA_DEG: f32 = 15.0;

/// A ramp of 12 m along the slope, 4 m wide, falling toward +x at 15 degrees, made of two triangles; or the same ramp as a rotated box.
fn ramp_world(as_mesh: bool) -> Sim {
    let th = THETA_DEG.to_radians();
    let (run, h) = (12.0 * th.cos(), 12.0 * th.sin());
    let mut s;
    if as_mesh {
        let mut l = Level::new();
        l.quad([0.0, h, -2.0], [0.0, h, 2.0], [run, 0.0, 2.0], [run, 0.0, -2.0], [0.0, 1.0, 0.0]);
        s = scene("ramp", &l, 0.8, 0.0);
    } else {
        s = Sim::new(1);
        s.threads = 1;
        // the box's top face lies where the mesh's surface is: its centre is 0.1 below the middle of the slope, along the slope's normal
        let q = qz(-th);
        let c = [run * 0.5 - 0.1 * th.sin(), h * 0.5 - 0.1 * th.cos(), 0.0];
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":{c:?},"rot":{q:?}}},"Collider":{{"shape":1,"size":[12,0.2,4]}},"RigidBody":{{"friction":0.8,"restitution":0}}}}"#));
    }
    // a ball of radius 0.25, one metre down the slope from the top, resting on it
    let n = [th.sin(), th.cos()];
    let p0 = [th.cos(), h - th.sin()];
    let c = [p0[0] + n[0] * 0.252, p0[1] + n[1] * 0.252, 0.0];
    spinning(&mut s, 2, 0, [0.5; 3], c, [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.8,"restitution":0}"#);
    s
}

#[test]
fn a_ball_rolls_down_a_mesh_ramp_like_one_rolling_down_the_same_ramp_made_of_a_box() {
    let (mut mesh, mut boxed) = (ramp_world(true), ramp_world(false));
    let start = pos(&mesh, 2);
    let mut arrive = [0u32; 2];
    for tick in 1..=240u32 {
        mesh.step();
        boxed.step();
        for (k, s) in [&mesh, &boxed].iter().enumerate() {
            if arrive[k] == 0 && pos(s, 2)[0] > 8.0 {
                arrive[k] = tick;
            }
        }
        if tick == 90 {
            // one and a half seconds down the slope: the same place and the same speed
            let (pm, pb) = (pos(&mesh, 2), pos(&boxed, 2));
            let (vm, vb) = (len(lin(&mesh, 2)), len(lin(&boxed, 2)));
            assert!(len(sub(pm, pb)) < 0.1, "mesh {pm:?} against box {pb:?}");
            assert!((vm - vb).abs() < 0.04 * vb, "speed {vm} against {vb}");
            // and about what a solid ball does on a 15 degree slope: 5/7 of g sin(theta), for 1.5 s (a little less: the first ticks)
            let want = 5.0 / 7.0 * 9.8 * THETA_DEG.to_radians().sin() * 1.5;
            assert!(vm > 0.92 * want && vm < 1.05 * want, "speed {vm} against {want}");
            let w = spin(&mesh, 2);
            assert!((w[2].abs() * 0.25 - vm).abs() < 0.06 * vm, "it rolls without slipping: spin {w:?}, speed {vm}");
            assert!(w[0].abs() < 0.05 && w[1].abs() < 0.05, "and straight: {w:?}");
        }
    }
    assert!(pos(&mesh, 2)[0] > start[0] + 6.0, "it went down the slope");
    assert!(arrive[0] > 0 && arrive[1] > 0, "both reached x = 8: {arrive:?}");
    assert!((arrive[0] as i32 - arrive[1] as i32).abs() <= 3, "at about the same time: {arrive:?}");
}

#[test]
fn a_box_dropped_on_a_mesh_floor_comes_to_rest_on_it() {
    let mut s = scene("boxrest", &big_floor(), 0.6, 0.0);
    spinning(&mut s, 2, 1, [1.0, 0.5, 0.8], [0.0, 1.5, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.6,"restitution":0}"#);
    // one that lands tilted and has to settle on a face
    spinning(&mut s, 3, 1, [1.0, 0.5, 0.8], [5.0, 1.5, 0.0], qz(0.4), r#"{"mass":1,"friction":0.6,"restitution":0}"#);
    s.step_n(60 * 5);
    let p = pos(&s, 2);
    assert!((p[1] - 0.25).abs() < 0.01 && p[0].abs() < 0.02 && p[2].abs() < 0.02, "it lies on the floor: {p:?}");
    assert!(len(lin(&s, 2)) < 0.05 && len(spin(&s, 2)) < 0.2, "and is still: {:?} {:?}", lin(&s, 2), spin(&s, 2));
    let t = pos(&s, 3);
    assert!(level(rot(&s, 3)) > 0.999, "the tilted one lies flat on a face: {:?}", rot(&s, 3));
    let half = axes(rot(&s, 3)).iter().zip([0.5f32, 0.25, 0.4]).map(|(a, h)| a[1].abs() * h).sum::<f32>();
    assert!((t[1] - half).abs() < 0.015 && len(lin(&s, 3)) < 0.1, "at rest on it: {t:?} (half height {half})");
}

#[test]
fn a_capsule_slides_along_a_mesh_wall_and_does_not_stick() {
    let mut l = Level::new();
    l.wall_x(2.0, -20.0, 20.0, -1.0, 4.0);
    let mut s = scene("wall", &l, 0.0, 0.0);
    spawn(&mut s, 5, r#"{"PhysicsSettings":{"gravity":[0,0,0]}}"#);
    plain(&mut s, 2, 2, [0.5, 1.8, 0.5], [0.0, 0.9, 0.0], r#"{"mass":1,"friction":0,"restitution":0}"#);
    s.apply(&Command::Set { uid: 2, component: "Velocity".into(), value: parse(r#"{"lin":[3,0,2]}"#).unwrap() }).unwrap();
    s.step_n(90);
    let (p, v) = (pos(&s, 2), lin(&s, 2));
    assert!((p[0] - 1.75).abs() < 0.03, "stopped against the wall by its radius: {p:?}");
    assert!(v[0].abs() < 0.1 && (v[2] - 2.0).abs() < 0.1, "and slides on along it at the speed it had along it: {v:?}");
    assert!(p[2] > 2.5, "{p:?}");
}

#[test]
fn a_stack_of_boxes_on_a_mesh_floor_stays_a_stack() {
    let mut s = scene("stack", &big_floor(), 0.7, 0.0);
    for i in 0..4u64 {
        spinning(&mut s, 2 + i, 1, [1.0; 3], [0.0, 0.5 + i as f32 * 1.002, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.7,"restitution":0}"#);
    }
    s.step_n(60 * 6);
    for i in 0..4u64 {
        let p = pos(&s, 2 + i);
        assert!(p[0].abs() < 0.05 && p[2].abs() < 0.05 && (p[1] - (0.5 + i as f32)).abs() < 0.06, "box {i} at {p:?}");
        assert!(level(rot(&s, 2 + i)) > 0.999 && len(spin(&s, 2 + i)) < 0.2, "box {i} is still: {:?}", spin(&s, 2 + i));
    }
}

#[test]
fn bodies_without_an_angular_velocity_land_on_a_mesh_floor_and_stay_there() {
    let mut s = scene("plainbodies", &big_floor(), 0.6, 0.0);
    plain(&mut s, 2, 0, [1.0; 3], [0.0, 3.0, 0.0], r#"{"mass":1,"friction":0.6,"restitution":0}"#);
    plain(&mut s, 3, 1, [1.0, 0.6, 1.0], [4.0, 3.0, 0.0], r#"{"mass":1,"friction":0.6,"restitution":0}"#);
    plain(&mut s, 4, 2, [0.5, 2.0, 0.5], [8.0, 3.0, 0.0], r#"{"mass":1,"friction":0.6,"restitution":0}"#);
    s.step_n(240);
    assert!((pos(&s, 2)[1] - 0.5).abs() < 0.01, "ball {:?}", pos(&s, 2));
    assert!((pos(&s, 3)[1] - 0.3).abs() < 0.01, "box {:?}", pos(&s, 3));
    assert!((pos(&s, 4)[1] - 1.0).abs() < 0.01, "capsule {:?}", pos(&s, 4));
    for uid in [2u64, 3, 4] {
        assert!(len(lin(&s, uid)) < 0.05, "body {uid} is still: {:?}", lin(&s, uid));
    }
}

#[test]
fn a_bouncy_ball_bounces_on_a_mesh_to_the_height_its_restitution_says() {
    let mut s = scene("bounce", &big_floor(), 0.5, 0.0);
    // the restitution of the contact is the larger of the two bodies' (a floor of 0 does not deaden a ball of 0.8)
    spinning(&mut s, 2, 0, [0.4; 3], [0.0, 5.2, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.5,"restitution":0.8}"#);
    let mut highest_after = 0.0f32;
    let mut bounced = false;
    for _ in 0..200 {
        s.step();
        let (p, v) = (pos(&s, 2), lin(&s, 2));
        if v[1] > 1.0 {
            bounced = true;
        }
        if bounced {
            highest_after = highest_after.max(p[1]);
        }
    }
    // dropped 5 m (centre from 5.2 to 0.2); 0.8 of the speed means 0.64 of the height
    assert!(highest_after > 0.2 + 5.0 * 0.64 * 0.9 && highest_after < 0.2 + 5.0 * 0.64 * 1.1, "{highest_after}");
}

#[test]
fn things_do_not_go_through_a_thin_mesh_floor_however_fast_they_fall() {
    for (shape, size) in [(0u32, [0.2f32; 3]), (1, [0.3, 0.2, 0.3]), (2, [0.2, 0.9, 0.2])] {
        for spins in [false, true] {
            let mut s = scene("fall", &big_floor(), 0.5, 0.2);
            let rigid = r#"{"mass":1,"restitution":0.2,"friction":0.5}"#;
            if spins {
                spinning(&mut s, 2, shape, size, [0.0, 80.0, 0.0], [0.0, 0.0, 0.0, 1.0], rigid);
            } else {
                plain(&mut s, 2, shape, size, [0.0, 80.0, 0.0], rigid);
            }
            let mut lowest = 99.0f32;
            for _ in 0..300 {
                s.step();
                lowest = lowest.min(pos(&s, 2)[1]);
            }
            // (it fell 80 m: 40 m/s at the floor, 0.7 m a tick, several times its own size)
            assert!(lowest > 0.0, "shape {shape} spinning {spins}: went through the floor, lowest centre {lowest}");
            // (after the bounce it is on its way up again: 0.2 of 40 m/s is 8 m/s, 3 m high)
            assert!(pos(&s, 2)[1] > 0.0 && pos(&s, 2)[1] < 5.0, "shape {shape} spinning {spins}: {:?}", pos(&s, 2));
        }
    }
}

#[test]
fn a_fast_ball_cannot_pass_through_a_thin_mesh_wall_and_a_spinning_one_bounces_at_its_real_speed() {
    let mut l = Level::new();
    l.wall_x(10.0, -30.0, 30.0, -30.0, 30.0);
    for spinning_ball in [false, true] {
        let mut s = scene("shot", &l, 0.0, 1.0);
        let spin = if spinning_ball { r#""AngularVelocity":{"damping":0},"# } else { "" };
        spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Velocity":{{"lin":[150,0,0]}},{spin}"Collider":{{"shape":0,"size":[0.2,0.2,0.2]}},"RigidBody":{{"mass":0.1,"restitution":1,"friction":0,"gravity_scale":0}}}}"#));
        let mut bounced = false;
        for _ in 0..30 {
            s.step();
            assert!(pos(&s, 2)[0] < 10.0, "spinning {spinning_ball}: it stays on this side of the wall: {:?}", pos(&s, 2));
            bounced |= lin(&s, 2)[0] < -100.0;
        }
        assert!(bounced, "spinning {spinning_ball}: it bounced back at about its own speed: {:?}", lin(&s, 2));
    }
}

// ------------------------------------------------------------------------------------------------ determinism, reloading, errors

fn busy(threads: usize, reverse: bool) -> Sim {
    // a bumpy field of 48 x 48 squares (4608 triangles) with a lot of things dropped on it
    let mut l = Level::new();
    let h = |i: i32, j: i32| 0.35 * ((i as f32 * 0.7).sin() + (j as f32 * 0.5).cos());
    for i in -24..24 {
        for j in -24..24 {
            let (x0, x1, z0, z1) = (i as f32, i as f32 + 1.0, j as f32, j as f32 + 1.0);
            l.quad([x0, h(i, j), z0], [x0, h(i, j + 1), z1], [x1, h(i + 1, j + 1), z1], [x1, h(i + 1, j), z0], [0.0, 1.0, 0.0]);
        }
    }
    let mut s = scene("busy", &l, 0.6, 0.1);
    s.threads = threads;
    let mut uids: Vec<u64> = (0..36).collect();
    if reverse {
        uids.reverse();
    }
    for i in uids {
        let (gx, gz, gy) = ((i % 6) as f32 * 1.7 - 4.0, ((i / 6) % 3) as f32 * 1.7 - 1.7, 2.0 + (i / 18) as f32 * 1.8);
        let shape = [0u32, 1, 2][(i % 3) as usize];
        let size = [[0.8, 0.8, 0.8], [0.9, 0.5, 1.1], [0.5, 1.6, 0.5]][(i % 3) as usize];
        if i % 4 == 0 {
            plain(&mut s, 100 + i, shape, size, [gx, gy, gz], r#"{"mass":1,"restitution":0.2,"friction":0.5}"#);
        } else {
            let q = qz(0.1 * i as f32);
            spawn(&mut s, 100 + i, &format!(r#"{{"Transform":{{"pos":{:?},"rot":{q:?}}},"Velocity":{{"lin":[{},0,{}]}},"AngularVelocity":{{"w":[{},0.5,{}]}},"Collider":{{"shape":{shape},"size":{size:?}}},"RigidBody":{{"mass":1,"restitution":0.2,"friction":0.5}}}}"#, [gx, gy, gz], (i % 5) as f32 - 2.0, (i % 3) as f32 - 1.0, (i % 4) as f32 - 1.5, (i % 3) as f32 - 1.0));
        }
    }
    s
}

#[test]
fn the_same_scene_gives_the_same_bits_whatever_the_thread_count_the_order_or_a_snapshot() {
    let run = |threads: usize, reverse: bool| {
        let mut s = busy(threads, reverse);
        s.step_n(300);
        s
    };
    let a = run(1, false);
    assert_eq!(a.state_hash(), run(1, true).state_hash(), "spawn order changed the result");
    assert_eq!(a.state_hash(), run(4, false).state_hash(), "thread count changed the result");
    assert_eq!(a.state_hash(), run(8, true).state_hash());
    // a snapshot continues exactly like the original, two ways
    let mut b = busy(1, false);
    b.step_n(100);
    let snapshot = b.clone();
    b.step_n(120);
    let mut c = snapshot.clone();
    c.step_n(120);
    assert_eq!(b.state_hash(), c.state_hash());
    // (two steps from a snapshot twice give the same hash both times, and a snapshot taken after them continues the same way)
    let (mut x, mut y) = (snapshot.clone(), snapshot);
    x.step();
    x.step();
    y.step_n(2);
    assert_eq!(x.state_hash(), y.state_hash());
    let (mut x2, mut y2) = (x.clone(), y);
    x2.step_n(50);
    y2.step_n(50);
    assert_eq!(x2.state_hash(), y2.state_hash());
    for i in 0..36u64 {
        let p = pos(&a, 100 + i);
        assert!(p.iter().all(|v| v.is_finite()) && p[1] > -1.0 && p[1] < 8.0 && p[0].abs() < 12.0 && p[2].abs() < 12.0, "body {i} at {p:?}");
    }
    // and a recording replays to the same end
    let mut r = busy(2, false);
    r.start_recording().unwrap();
    r.step_n(150);
    let rec = r.stop_recording().unwrap();
    let again = Sim::replay(&r, &rec).unwrap();
    assert_eq!(again.state_hash(), rec.final_hash, "the replay of a run with a mesh collider ends in the same state");
}

#[test]
fn a_scene_without_a_mesh_collider_never_touches_any_mesh_code() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[50,1,50]}}"#);
    spinning(&mut s, 2, 0, [1.0; 3], [0.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1}"#);
    let before = s.state_hash();
    s.step_n(120);
    assert!(s.meshes.is_empty() && s.mesh_errors.is_empty() && s.used_meshes().is_empty());
    assert_ne!(before, s.state_hash());
}

#[test]
fn a_model_that_changes_on_disk_is_read_again_and_a_broken_one_keeps_the_last_good_copy() {
    let dir = dir_for("reload");
    let mut a = Level::new();
    a.floor(-10.0, 10.0, -10.0, 10.0, 0.0);
    a.save(&dir, "level.glb");
    let mut s = Sim::new(1);
    s.threads = 1;
    s.asset_root = Some(dir.clone());
    spawn(&mut s, 1, r#"{"Transform":{},"MeshCollider":{"mesh":"level.glb"}}"#);
    plain(&mut s, 2, 0, [0.5; 3], [0.0, 3.0, 0.0], r#"{"mass":1,"restitution":0}"#);
    s.step_n(130);
    assert!((pos(&s, 2)[1] - 0.25).abs() < 0.02, "the ball rests on the floor: {:?}", pos(&s, 2));
    let hash_a = s.meshes.by_name["level.glb"].hash;
    let state_a = s.state_hash();
    // the floor is replaced by one 5 m lower (the file is saved again later than it was)
    let mut b = Level::new();
    b.floor(-10.0, 10.0, -10.0, 10.0, -5.0);
    b.save(&dir, "level.glb");
    let f = std::fs::OpenOptions::new().write(true).open(dir.join("level.glb")).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(30)).unwrap();
    drop(f);
    s.step_n(120);
    assert_ne!(s.meshes.by_name["level.glb"].hash, hash_a, "the new file was read");
    assert!(pos(&s, 2)[1] < -4.0, "and the ball fell to the new floor: {:?}", pos(&s, 2));
    assert_ne!(s.state_hash(), state_a);
    // a half-written file: the old floor stays, the reason is kept
    std::fs::write(dir.join("level.glb"), b"glTF half a model").unwrap();
    let f = std::fs::OpenOptions::new().write(true).open(dir.join("level.glb")).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60)).unwrap();
    drop(f);
    let kept = s.meshes.by_name["level.glb"].hash;
    s.step_n(120);
    assert_eq!(s.meshes.by_name["level.glb"].hash, kept, "the last good copy stays");
    assert!(s.mesh_errors["level.glb"].contains("changed on disk"), "{:?}", s.mesh_errors);
    assert!(pos(&s, 2)[1] > -5.3 && pos(&s, 2)[1] < -4.5, "still on the floor: {:?}", pos(&s, 2));
}

#[test]
fn a_mesh_collider_that_cannot_be_used_says_why_and_collides_with_nothing() {
    let dir = dir_for("errors");
    std::fs::write(dir.join("empty.glb"), glb_of(&[[[0.0; 3]; 3]])).unwrap(); // one triangle with no area
    std::fs::write(dir.join("junk.glb"), b"not a model").unwrap();
    std::fs::write(dir.join("big.glb"), {
        // more triangles than the limit: 1,000,001 copies of one triangle, as a flat list
        let tri = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        glb_of(&vec![tri; crate::meshcol::MAX_TRIANGLES + 1])
    })
    .unwrap();
    let mut s = Sim::new(1);
    s.asset_root = Some(dir);
    for (i, name) in ["missing.glb", "empty.glb", "junk.glb", "big.glb", "cube", ""].iter().enumerate() {
        spawn(&mut s, 10 + i as u64, &format!(r#"{{"Transform":{{}},"MeshCollider":{{"mesh":"{name}"}}}}"#));
    }
    plain(&mut s, 2, 0, [0.5; 3], [0.0, 2.0, 0.0], r#"{"mass":1}"#);
    s.step_n(5);
    let why = |name: &str| s.mesh_errors.get(name).cloned().unwrap_or_default();
    assert!(why("missing.glb").contains("cannot read"), "{}", why("missing.glb"));
    assert!(why("empty.glb").contains("no usable triangles"), "{}", why("empty.glb"));
    assert!(why("junk.glb").contains("invalid") || why("junk.glb").contains("JSON"), "{}", why("junk.glb"));
    assert!(why("big.glb").contains("1000000"), "{}", why("big.glb"));
    assert!(why("cube").contains("not a model file"), "{}", why("cube"));
    assert!(s.meshes.is_empty(), "nothing was read");
    s.step_n(60);
    assert!(pos(&s, 2)[1] < 0.0, "so the ball falls through: {:?}", pos(&s, 2));
}

// ------------------------------------------------------------------------------------------------ characters

/// The ground, a ramp up (20 degrees), a steep slope (60), a 0.3 m ledge, a 0.8 m ledge, a wall and a pit, in a row along x with z = 0.
fn walkers_level() -> Level {
    let mut l = Level::new();
    // the ground is cut round the pit (x 40 to 44, z -3 to 3)
    l.floor(-20.0, 40.0, -20.0, 20.0, 0.0);
    l.floor(44.0, 60.0, -20.0, 20.0, 0.0);
    l.floor(40.0, 44.0, 3.0, 20.0, 0.0);
    l.floor(40.0, 44.0, -20.0, -3.0, 0.0);
    // the pit: 3 m deep with a floor and four walls
    l.floor(40.0, 44.0, -3.0, 3.0, -3.0);
    l.quad([40.0, -3.0, -3.0], [40.0, 0.0, -3.0], [40.0, 0.0, 3.0], [40.0, -3.0, 3.0], [1.0, 0.0, 0.0]);
    l.quad([44.0, -3.0, -3.0], [44.0, 0.0, -3.0], [44.0, 0.0, 3.0], [44.0, -3.0, 3.0], [-1.0, 0.0, 0.0]);
    l.quad([40.0, -3.0, -3.0], [40.0, 0.0, -3.0], [44.0, 0.0, -3.0], [44.0, -3.0, -3.0], [0.0, 0.0, 1.0]);
    l.quad([40.0, -3.0, 3.0], [40.0, 0.0, 3.0], [44.0, 0.0, 3.0], [44.0, -3.0, 3.0], [0.0, 0.0, -1.0]);
    // a ramp up at 20 degrees: 4 m along x, rising 1.456 m, then a platform on top of it
    let rise = 4.0 * 20.0f32.to_radians().tan();
    l.ramp(5.0, 9.0, -3.0, 3.0, rise);
    l.floor(9.0, 14.0, -3.0, 3.0, rise);
    // a steep slope (60 degrees): 1 m along x, rising 1.73 m, in its own lane (z 8 to 12), and a platform above it
    l.ramp(5.0, 6.0, 8.0, 12.0, 3.0f32.sqrt());
    l.floor(6.0, 10.0, 8.0, 12.0, 3.0f32.sqrt());
    // a ledge 0.3 m high and one 0.8 m high (lanes z -8 to -6 and z -12 to -10)
    l.block([5.0, 0.0, -8.0], [8.0, 0.3, -6.0]);
    l.block([5.0, 0.0, -12.0], [8.0, 0.8, -10.0]);
    // a wall across x = 20 (z -20 to 20, 3 m high)
    l.wall_x(20.0, -20.0, 20.0, 0.0, 3.0);
    l
}

fn hero(s: &mut Sim, uid: u64, p: V3) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":{p:?}}},"CharacterBody":{{}}}}"#));
}

fn body(s: &Sim, uid: u64) -> CharacterBody {
    *s.world.get::<CharacterBody>(s.world.entity(uid).unwrap()).unwrap()
}

fn walk(s: &mut Sim, uid: u64, v: [f32; 2]) {
    let e = s.world.entity(uid).unwrap();
    s.world.get_mut::<CharacterBody>(e).unwrap().walk = v;
}

#[test]
fn a_character_stands_on_a_mesh_floor_and_walks_on_it_at_the_asked_speed() {
    let mut s = scene("walk", &walkers_level(), 0.6, 0.0);
    hero(&mut s, 2, [-10.0, 1.0, 0.0]);
    s.step_n(60);
    assert!(pos(&s, 2)[1].abs() < 1e-3 && body(&s, 2).grounded, "{:?} {:?}", pos(&s, 2), body(&s, 2));
    let before = pos(&s, 2);
    s.step_n(300);
    assert!((0..3).all(|k| (pos(&s, 2)[k] - before[k]).abs() < 1e-4), "it stays put: {:?} vs {before:?}", pos(&s, 2));
    walk(&mut s, 2, [3.0, 0.0]);
    s.step_n(60);
    assert!((pos(&s, 2)[0] - (before[0] + 3.0)).abs() < 0.02 && pos(&s, 2)[1].abs() < 1e-3, "{:?}", pos(&s, 2));
}

#[test]
fn a_character_walks_up_a_gentle_mesh_slope_and_not_up_a_steep_one() {
    let mut s = scene("slopes", &walkers_level(), 0.6, 0.0);
    hero(&mut s, 2, [2.0, 0.0, 0.0]);
    hero(&mut s, 3, [2.0, 0.0, 10.0]);
    s.step_n(30);
    walk(&mut s, 2, [2.0, 0.0]);
    walk(&mut s, 3, [2.0, 0.0]);
    let mut highest = 0.0f32;
    for _ in 0..240 {
        s.step();
        highest = highest.max(pos(&s, 3)[1]);
    }
    let rise = 4.0 * 20.0f32.to_radians().tan();
    // the 20 degree ramp: it ends up on the platform on top, standing on it
    let p = pos(&s, 2);
    assert!(p[0] > 9.5 && (p[1] - rise).abs() < 0.02 && body(&s, 2).grounded, "{p:?} {:?}", body(&s, 2));
    // the 60 degree slope (max_slope 50): it does not get up (its feet stay low) and stays at the foot of it
    let q = pos(&s, 3);
    assert!(highest < 0.5 && q[0] < 5.6, "stopped at the steep slope: {q:?}, highest {highest}");
}

#[test]
fn a_character_steps_up_a_low_ledge_and_is_stopped_by_a_high_one_and_a_wall() {
    let mut s = scene("ledges", &walkers_level(), 0.6, 0.0);
    hero(&mut s, 2, [3.0, 0.0, -7.0]);
    hero(&mut s, 3, [3.0, 0.0, -11.0]);
    hero(&mut s, 4, [15.0, 0.0, 0.0]);
    s.step_n(30);
    for uid in [2u64, 3, 4] {
        walk(&mut s, uid, [2.0, 0.0]);
    }
    s.step_n(120); // (two seconds: 4 m, the first to x = 7 on top of the ledge that ends at x = 8)
    let (low, high) = (pos(&s, 2), pos(&s, 3));
    assert!(low[0] > 6.5 && (low[1] - 0.3).abs() < 0.02, "up the 0.3 m ledge and standing on it: {low:?}");
    assert!(high[0] < 4.7 && high[1].abs() < 0.02, "the 0.8 m ledge stops it: {high:?}");
    s.step_n(60);
    let wall = pos(&s, 4);
    assert!((wall[0] - 19.65).abs() < 0.03, "stopped by the wall (radius 0.35): {wall:?}");
    // walking diagonally into the wall slides along it
    walk(&mut s, 4, [2.0, 2.0]);
    s.step_n(60);
    let p = pos(&s, 4);
    assert!((p[0] - 19.65).abs() < 0.03 && p[2] > 1.5, "slides along the wall: {p:?}");
}

#[test]
fn a_character_falls_into_a_mesh_pit_and_lands_on_its_floor() {
    let mut s = scene("pit", &walkers_level(), 0.6, 0.0);
    hero(&mut s, 2, [36.0, 0.0, 0.0]);
    s.step_n(30);
    walk(&mut s, 2, [3.0, 0.0]);
    s.step_n(150);
    let p = pos(&s, 2);
    assert!(p[0] > 40.4 && p[0] < 43.7 && (p[1] + 3.0).abs() < 0.02 && body(&s, 2).grounded, "in the pit, on its floor: {p:?} {:?}", body(&s, 2));
    walk(&mut s, 2, [3.0, 0.0]);
    s.step_n(60);
    assert!(pos(&s, 2)[0] < 43.7 && pos(&s, 2)[1] < -2.9, "the pit's far wall holds it: {:?}", pos(&s, 2));
    // jumping out of a pit of 3 m is not possible with the default jump
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<CharacterBody>(e).unwrap().jump = 6.0;
    s.step_n(120);
    assert!(pos(&s, 2)[1] < -1.0, "{:?}", pos(&s, 2));
}

// ------------------------------------------------------------------------------------------------ navigation

fn path_over(s: &mut Sim, from: V3, to: V3) -> Result<crate::nav::PathResult, crate::nav::NavError> {
    s.load_meshes();
    crate::nav::find_path_with(&mut s.world, from, to, crate::nav::NavParams::default(), &s.heightfields, &s.meshes)
}

#[test]
fn a_path_over_a_mesh_floor_is_straight_and_goes_round_a_mesh_wall() {
    let mut s = scene("navflat", &walkers_level(), 0.6, 0.0);
    let r = path_over(&mut s, [-15.0, 0.0, 0.0], [-5.0, 0.0, 4.0]).unwrap();
    assert_eq!(r.points.len(), 2, "{:?}", r.points);
    assert!((r.length - (100.0f32 + 16.0).sqrt()).abs() < 0.05, "{}", r.length);
    // the wall at x = 20 spans the whole width of the level: there is no way round it
    let blocked = path_over(&mut s, [15.0, 0.0, 0.0], [25.0, 0.0, 0.0]);
    assert!(matches!(blocked, Err(crate::nav::NavError::NoPath(_))), "{blocked:?}");
}

#[test]
fn a_path_climbs_a_gentle_mesh_ramp_but_not_a_steep_one_and_keeps_out_of_a_pit() {
    let mut s = scene("navramp", &walkers_level(), 0.6, 0.0);
    let rise = 4.0 * 20.0f32.to_radians().tan();
    // onto the platform at the top of the 20 degree ramp
    let r = path_over(&mut s, [0.0, 0.0, 0.0], [12.0, rise, 0.0]).unwrap();
    assert!(r.exact && (r.points.last().unwrap()[1] - rise).abs() < 0.1, "{:?}", r.points);
    assert!(r.length > 12.0 && r.length < 12.6, "over the ramp in a straight line: {} {:?}", r.length, r.points);
    // the platform at the top of the 60 degree slope can only be reached by that slope
    let steep = path_over(&mut s, [0.0, 0.0, 10.0], [8.0, 3.0f32.sqrt(), 10.0]);
    assert!(steep.is_err(), "a slope of 60 degrees is not walked: {steep:?}");
    // across the pit (x 40 to 44): the way goes round it
    let r = path_over(&mut s, [34.0, 0.0, 0.0], [50.0, 0.0, 0.0]).unwrap();
    assert!(r.length > 16.5 && r.points.iter().all(|p| !(p[0] > 39.5 && p[0] < 44.5 && p[2].abs() < 3.0)), "round the pit: {:?}", r.points);
    // the bottom of the pit is ground, but 3 m is farther than an agent drops, and no ramp leads down
    let down = path_over(&mut s, [34.0, 0.0, 0.0], [42.0, -3.0, 0.0]);
    assert!(down.is_err(), "{down:?}");
}

#[test]
fn an_agent_walks_over_a_mesh_level_up_a_ramp_to_a_target() {
    let mut s = scene("navwalk", &walkers_level(), 0.6, 0.0);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"CharacterBody":{},"NavAgent":{"speed":3}}"#);
    s.step_n(10);
    let rise = 4.0 * 20.0f32.to_radians().tan();
    let e = s.world.entity(2).unwrap();
    {
        let a = s.world.get_mut::<NavAgent>(e).unwrap();
        a.target = [12.0, rise, 0.0];
        a.active = true;
    }
    for _ in 0..60 * 12 {
        s.step();
        if s.world.get::<NavAgent>(e).unwrap().status == 2 {
            break;
        }
    }
    let (a, p) = (*s.world.get::<NavAgent>(e).unwrap(), pos(&s, 2));
    assert_eq!(a.status, 2, "arrived: {a:?} at {p:?}");
    assert!((p[0] - 12.0).abs() < 0.5 && (p[1] - rise).abs() < 0.1, "{p:?}");
}

// ------------------------------------------------------------------------------------------------ rays and sensors

#[test]
fn rays_and_sensors_see_the_triangles_of_a_mesh_collider() {
    let mut l = Level::new();
    l.wall_x(5.0, -10.0, 10.0, -1.0, 5.0);
    l.floor(-10.0, 10.0, -10.0, 10.0, 0.0);
    let mut s = scene("rays", &l, 0.5, 0.0);
    s.step();
    let h = s.meshes.ray(&mut s.world, [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], 50.0).expect("the wall");
    assert!((h.0 - 5.0).abs() < 1e-4 && (h.1[0] + 1.0).abs() < 1e-4 && h.2 == 1, "{h:?}");
    let h = s.meshes.ray(&mut s.world, [-3.0, 4.0, 0.5], [0.0, -1.0, 0.0], 50.0).expect("the floor");
    assert!((h.0 - 4.0).abs() < 1e-4 && (h.1[1] - 1.0).abs() < 1e-4, "{h:?}");
    assert!(s.meshes.ray(&mut s.world, [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], 5.0).is_none(), "nothing that way within 5 m");
    // a sensor on one side of the wall sees a thing on the other only when the wall is not solid
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"Sensor":{"radius":20,"fov":360,"filter":"Velocity","los":true,"eye":1.0}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[9,0,0]},"Velocity":{}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[-9,0,0]},"Velocity":{}}"#);
    s.step();
    let seen = |s: &Sim| s.world.get::<components::Sensor>(s.world.entity(2).unwrap()).unwrap().seen.0;
    assert_eq!(seen(&s), 4, "the one on this side of the wall is seen, the one behind it is not");
    let e = s.world.entity(1).unwrap();
    s.world.get_mut::<components::MeshCollider>(e).unwrap().solid = false;
    s.step();
    assert_ne!(seen(&s), 0);
    // two things equally far: the lower uid is the one that is seen when both are in view
    assert_eq!(seen(&s), 3, "(both in view, 9 m away each: the lower uid)");
}

// ------------------------------------------------------------------------------------------------ cost

/// A bumpy field of `n` x `n` squares: 2 n^2 triangles.
fn big_field(n: i32) -> Level {
    let mut l = Level::new();
    let h = |i: i32, j: i32| 0.5 * (i as f32 * 0.11).sin() * (j as f32 * 0.13).cos();
    let half = n / 2;
    for i in -half..half {
        for j in -half..half {
            let (x0, x1, z0, z1) = (i as f32 * 0.25, (i + 1) as f32 * 0.25, j as f32 * 0.25, (j + 1) as f32 * 0.25);
            l.quad([x0, h(i, j), z0], [x0, h(i, j + 1), z1], [x1, h(i + 1, j + 1), z1], [x1, h(i + 1, j), z0], [0.0, 1.0, 0.0]);
        }
    }
    l
}

/// The twelve corners of an icosahedron (a rock) about 0.3 m across.
fn solid_icosa() -> Vec<V3> {
    let t = (1.0 + 5.0f32.sqrt()) / 2.0;
    let mut v = Vec::new();
    for (a, b) in [(-1.0f32, t), (1.0, t), (-1.0, -t), (1.0, -t)] {
        v.push([a * 0.15, b * 0.15, 0.0]);
        v.push([0.0, a * 0.15, b * 0.15]);
        v.push([b * 0.15, 0.0, a * 0.15]);
    }
    v
}

/// The numbers of docs/PHYSICS.md (run with `--nocapture` to see them): what each step of using a level of 100,000 triangles costs.
#[test]
fn what_each_question_to_a_level_of_a_hundred_thousand_triangles_costs() {
    use crate::meshcol::{CollisionMesh, PlacedMesh};
    let level = big_field(224);
    let bytes = glb_of(&level.tris);
    let (t0, mesh) = (std::time::Instant::now(), CollisionMesh::from_bytes(&bytes, None).unwrap());
    let from_file = t0.elapsed().as_secs_f64() * 1e3;
    let mesh = std::sync::Arc::new(mesh);
    let t1 = std::time::Instant::now();
    let rebuilt = CollisionMesh::new(mesh.verts.clone(), mesh.tris.clone()).unwrap();
    let tree = t1.elapsed().as_secs_f64() * 1e3;
    assert_eq!(rebuilt.hash, mesh.hash);
    let t2 = std::time::Instant::now();
    let placed = PlacedMesh::new(mesh.clone(), &Transform { pos: [1.0, 0.5, -2.0], rot: qz(0.3), scale: [1.0, 1.0, 1.0] }).unwrap();
    let place = t2.elapsed().as_secs_f64() * 1e3;
    // questions at 20,000 places on and just above the surface
    let mut seed = 12345u64;
    let mut rnd = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 40) as f32) / (1u64 << 24) as f32
    };
    // (each spot is just above the middle of a triangle chosen by chance: every question has something to find)
    let spots: Vec<V3> = (0..20_000)
        .map(|_| {
            let t = ((rnd() * placed.triangle_count() as f32) as usize).min(placed.triangle_count() - 1) as u32;
            let [a, b, c] = placed.tri(t);
            let mid = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0 + 0.15, (a[2] + b[2] + c[2]) / 3.0];
            mid
        })
        .collect();
    let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let per = |f: &mut dyn FnMut(V3) -> usize| {
        let t = std::time::Instant::now();
        let mut found = 0;
        for &s in &spots {
            found += f(s);
        }
        (t.elapsed().as_secs_f64() * 1e6 / spots.len() as f64, found)
    };
    let (ball, nb) = per(&mut |s| placed.sphere(s, 0.3, [0.0; 3]).len());
    let (capsule, nc) = per(&mut |s| placed.capsule(s, [s[0], s[1] + 1.0, s[2]], 0.3, [0.0; 3]).len());
    let (boxed, nx) = per(&mut |s| placed.obb(s, &ident, [0.4, 0.3, 0.4], [0.0; 3]).len());
    let (ray, nr) = per(&mut |s| usize::from(placed.ray([s[0], 6.0, s[2]], [0.0, -1.0, 0.0], 20.0).is_some()));
    // convex hulls: building one from 3,000 points (simplified to 64 corners), two of them against each other, and one against the level
    let golden = std::f64::consts::PI * (3.0 - 5.0f64.sqrt());
    let cloud: Vec<V3> = (0..3000)
        .map(|i| {
            let y = 1.0 - 2.0 * (i as f64 + 0.5) / 3000.0;
            let r = (1.0 - y * y).sqrt();
            [(0.3 * r * (golden * i as f64).cos()) as f32, (0.3 * y) as f32, (0.3 * r * (golden * i as f64).sin()) as f32]
        })
        .collect();
    let t3 = std::time::Instant::now();
    let hull64 = crate::hull::Hull::build(&cloud, [1.0; 3]).unwrap();
    let hull_build = t3.elapsed().as_secs_f64() * 1e3;
    let hull12 = crate::hull::Hull::build(&solid_icosa(), [1.0; 3]).unwrap();
    let (hh12, n12) = per(&mut |s| {
        let a = crate::hull_contact::Poly::from_hull(&hull12, s, &ident);
        let b = crate::hull_contact::Poly::from_hull(&hull12, [s[0] + 0.1, s[1] + 0.2, s[2] + 0.05], &ident);
        crate::hull_contact::poly_poly(&a, &b).len()
    });
    let (hh64, n64) = per(&mut |s| {
        let a = crate::hull_contact::Poly::from_hull(&hull64, s, &ident);
        let b = crate::hull_contact::Poly::from_hull(&hull64, [s[0] + 0.1, s[1] + 0.2, s[2] + 0.05], &ident);
        crate::hull_contact::poly_poly(&a, &b).len()
    });
    let (hm, nm) = per(&mut |s| placed.poly(&crate::hull_contact::Poly::from_hull(&hull12, s, &ident), [0.0; 3]).len());
    eprintln!("100k triangles ({} verts), measured on a 3.6 GHz desktop CPU:", mesh.verts.len());
    eprintln!("  convex hull from 3,000 points to 64 corners {hull_build:.2} ms; two hulls of 12 corners against each other {hh12:.2} us ({n12} contact points in all), two of 64 corners {hh64:.2} us ({n64}); a 12-corner hull on the level {hm:.2} us ({nm})");
    eprintln!("  read the .glb and build the tree {from_file:.1} ms (the tree alone {tree:.1} ms); place it with a Transform {place:.2} ms");
    eprintln!("  one ball {ball:.2} us ({nb} contacts in 20,000 questions), one capsule {capsule:.2} us ({nc}), one box {boxed:.2} us ({nx}), one ray {ray:.2} us ({nr} hits)");
    assert!(nb > 10_000 && nc > 10_000 && nx > 10_000 && nr > 14_000, "{nb} {nc} {nx} {nr}");
    assert!(ball < 100.0 && capsule < 200.0 && boxed < 300.0 && ray < 100.0, "{ball} {capsule} {boxed} {ray}");
}

#[test]
fn a_ball_rolling_on_a_level_of_a_hundred_thousand_triangles_costs_well_under_a_millisecond_a_tick() {
    let level = big_field(224); // 224 x 224 squares: 100,352 triangles
    assert!(level.tris.len() > 100_000);
    let mut s = scene("hundredk", &level, 0.6, 0.1);
    s.threads = 1;
    // a ball, a box and a capsule on the slopes
    spinning(&mut s, 2, 0, [0.5; 3], [3.0, 3.0, 3.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.6,"restitution":0.1}"#);
    spinning(&mut s, 3, 1, [0.6, 0.4, 0.6], [-5.0, 3.0, 2.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.6,"restitution":0.1}"#);
    spinning(&mut s, 4, 2, [0.4, 1.4, 0.4], [1.0, 3.0, -6.0], [0.0, 0.0, 0.0, 1.0], r#"{"mass":1,"friction":0.6,"restitution":0.1}"#);
    // the first step reads the file and builds the tree and the placed copy; the cost of that is not a per-tick cost
    let t0 = std::time::Instant::now();
    s.step();
    let first = t0.elapsed().as_secs_f64() * 1e3;
    s.step_n(30);
    s.profile = Some(Default::default());
    let t1 = std::time::Instant::now();
    s.step_n(300);
    let per_tick = t1.elapsed().as_secs_f64() * 1e3 / 300.0;
    let profile = s.profile.take().unwrap();
    let physics = profile.stages.get("system physics").map_or(f64::NAN, |st| st.total_ms / st.calls.max(1) as f64);
    eprintln!("100k triangles: first tick (read, build, place) {first:.1} ms; a tick with 3 bodies rolling: {per_tick:.4} ms in all, physics {physics:.4} ms");
    assert!(pos(&s, 2)[1] > -2.0 && pos(&s, 2)[1] < 4.0 && pos(&s, 3)[1] > -2.0 && pos(&s, 4)[1] > -2.0, "they stay on the field");
    assert!(per_tick < 1.0, "a tick costs {per_tick} ms");
}
