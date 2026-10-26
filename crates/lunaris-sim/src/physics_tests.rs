use super::*;
use components::{Transform, Velocity};
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = j(comps) else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// A big static floor whose top surface is at y = 0.
fn floor(s: &mut Sim) {
    spawn(s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[60,1,60]}}"#);
}

fn ball(s: &mut Sim, uid: u64, pos: &str, extra_body: &str) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":{pos}}},"Velocity":{{}},"Collider":{{"shape":0,"size":[1,1,1]}},"RigidBody":{extra_body}}}"#));
}

fn y(s: &Sim, uid: u64) -> f32 {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos[1]
}

fn vel(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Velocity>(s.world.entity(uid).unwrap()).unwrap().lin
}

#[test]
fn a_dropped_ball_comes_to_rest_on_the_floor() {
    let mut s = Sim::new(1);
    floor(&mut s);
    ball(&mut s, 2, "[0,3,0]", "{}");
    s.step_n(60 * 5);
    assert!((y(&s, 2) - 0.5).abs() < 0.02, "resting height {}", y(&s, 2));
    assert!(vel(&s, 2)[1].abs() < 0.2, "{:?}", vel(&s, 2));
    assert!(!s.contacts.is_empty(), "contacts are reported");
    assert_eq!((s.contacts[0].a, s.contacts[0].b), (1, 2));
    assert!(s.contacts[0].normal[1] > 0.99, "normal from the floor (a) to the ball (b) points up");
}

#[test]
fn restitution_sets_the_bounce_height() {
    let bounce = |e: f32| {
        let mut s = Sim::new(1);
        floor(&mut s);
        ball(&mut s, 2, "[0,5.5,0]", &format!(r#"{{"restitution":{e}}}"#));
        // fall, bounce once, record the highest point afterwards
        let mut hit = false;
        let mut peak = 0.0f32;
        for _ in 0..400 {
            s.step();
            if vel(&s, 2)[1] > 0.5 {
                hit = true;
            }
            if hit {
                peak = peak.max(y(&s, 2));
            }
        }
        peak
    };
    let (low, high) = (bounce(0.3), bounce(0.8));
    // drop height above the resting position is 5, the rebound height is about e^2 * 5 above it
    assert!((high - 0.5 - 0.64 * 5.0).abs() < 0.5, "e = 0.8 peaked at {high}");
    assert!((low - 0.5 - 0.09 * 5.0).abs() < 0.3, "e = 0.3 peaked at {low}");
    assert!(high > low);
}

#[test]
fn equal_masses_exchange_velocity_in_an_elastic_head_on_collision() {
    let mut s = Sim::new(1);
    spawn(&mut s, 9, r#"{"PhysicsSettings":{"gravity":[0,0,0]}}"#);
    ball(&mut s, 2, "[-3,0,0]", r#"{"restitution":1,"friction":0}"#);
    ball(&mut s, 3, "[3,0,0]", r#"{"restitution":1,"friction":0}"#);
    s.apply(&Command::Set { uid: 2, component: "Velocity".into(), value: j(r#"{"lin":[4,0,0]}"#) }).unwrap();
    s.apply(&Command::Set { uid: 3, component: "Velocity".into(), value: j(r#"{"lin":[-2,0,0]}"#) }).unwrap();
    let p0 = vel(&s, 2)[0] + vel(&s, 3)[0];
    s.step_n(150);
    let (v2, v3) = (vel(&s, 2), vel(&s, 3));
    assert!((v2[0] + 2.0).abs() < 0.05 && (v3[0] - 4.0).abs() < 0.05, "velocities after the collision {v2:?} {v3:?}");
    assert!((v2[0] + v3[0] - p0).abs() < 1e-3, "momentum is conserved");
}

#[test]
fn a_heavy_body_barely_moves_when_hit_by_a_light_one() {
    let mut s = Sim::new(1);
    spawn(&mut s, 9, r#"{"PhysicsSettings":{"gravity":[0,0,0]}}"#);
    ball(&mut s, 2, "[-3,0,0]", r#"{"mass":0.1,"restitution":0.5,"friction":0}"#);
    ball(&mut s, 3, "[0,0,0]", r#"{"mass":1000,"restitution":0.5,"friction":0}"#);
    s.apply(&Command::Set { uid: 2, component: "Velocity".into(), value: j(r#"{"lin":[5,0,0]}"#) }).unwrap();
    s.step_n(120);
    assert!(vel(&s, 3)[0].abs() < 0.01);
    assert!(vel(&s, 2)[0] < -1.5, "the light ball rebounds: {:?}", vel(&s, 2));
}

#[test]
fn a_stack_of_boxes_stays_a_stack() {
    let mut s = Sim::new(1);
    floor(&mut s);
    for (i, uid) in (2u64..=4).enumerate() {
        spawn(&mut s, uid, &format!(r#"{{"Transform":{{"pos":[0,{}, 0]}},"Velocity":{{}},"Collider":{{"shape":1,"size":[1,1,1]}},"RigidBody":{{"restitution":0,"friction":0.8}}}}"#, 0.5 + i as f32 * 1.001));
    }
    s.step_n(60 * 6);
    for (i, uid) in (2u64..=4).enumerate() {
        let want = 0.5 + i as f32;
        assert!((y(&s, uid) - want).abs() < 0.05, "box {uid} at {} (wanted about {want})", y(&s, uid));
        assert!(vel(&s, uid).iter().all(|v| v.abs() < 0.3), "box {uid} still moving {:?}", vel(&s, uid));
    }
}

#[test]
fn friction_slows_a_sliding_box() {
    let mut s = Sim::new(1);
    floor(&mut s);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0.5,0]},"Velocity":{"lin":[6,0,0]},"Collider":{"shape":1,"size":[1,1,1]},"RigidBody":{"friction":0.6,"restitution":0}}"#);
    s.step_n(30);
    let early = vel(&s, 2)[0];
    s.step_n(120);
    let late = vel(&s, 2)[0];
    assert!(early < 6.0 && early > 0.5, "{early}");
    assert!(late < early && late < 0.2, "friction should bring it to a stop, speed {late}");
}

#[test]
fn triggers_report_overlaps_without_pushing() {
    let mut s = Sim::new(1);
    spawn(&mut s, 9, r#"{"PhysicsSettings":{"gravity":[0,0,0]}}"#);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Collider":{"shape":1,"size":[4,4,4],"trigger":true}}"#);
    ball(&mut s, 2, "[0,0,0]", "{}");
    s.step_n(5);
    assert!(s.contacts.iter().any(|c| c.trigger && c.a == 1 && c.b == 2));
    assert_eq!(vel(&s, 2), [0.0, 0.0, 0.0]);
    assert!(s.world.get::<Transform>(s.world.entity(2).unwrap()).unwrap().pos == [0.0, 0.0, 0.0]);
}

#[test]
fn the_outcome_depends_neither_on_spawn_order_nor_on_thread_count() {
    let build = |reverse: bool, threads: usize| {
        let mut s = Sim::new(3);
        s.threads = threads;
        floor(&mut s);
        // four walls around the floor
        for (i, (p, size)) in [([-8.0, 2.0, 0.0], [1.0, 4.0, 18.0]), ([8.0, 2.0, 0.0], [1.0, 4.0, 18.0]), ([0.0, 2.0, -8.0], [18.0, 4.0, 1.0]), ([0.0, 2.0, 8.0], [18.0, 4.0, 1.0])].iter().enumerate() {
            spawn(&mut s, 100 + i as u64, &format!(r#"{{"Transform":{{"pos":{p:?}}},"Collider":{{"shape":1,"size":{size:?}}}}}"#));
        }
        let mut uids: Vec<u64> = (0..120).collect();
        if reverse {
            uids.reverse();
        }
        for i in uids {
            let (gx, gz, gy) = ((i % 6) as f32 * 1.7 - 4.0, ((i / 6) % 5) as f32 * 1.7 - 3.0, 1.0 + (i / 30) as f32 * 1.6);
            let shape = if i % 3 == 0 { r#"{"shape":1,"size":[0.9,0.9,0.9]}"# } else { r#"{"shape":0,"size":[1,1,1]}"# };
            spawn(&mut s, 1000 + i, &format!(r#"{{"Transform":{{"pos":[{gx},{gy},{gz}]}},"Velocity":{{"lin":[{},0,{}]}},"Collider":{shape},"RigidBody":{{"restitution":0.4}}}}"#, (i % 5) as f32 - 2.0, (i % 7) as f32 - 3.0));
        }
        s.step_n(240);
        s
    };
    let a = build(false, 1);
    assert_eq!(a.state_hash(), build(true, 1).state_hash(), "spawn order changed the result");
    assert_eq!(a.state_hash(), build(false, 4).state_hash(), "thread count changed the result");
    // and it behaved physically: everything is inside the walls and above the floor
    for uid in 1000..1120u64 {
        let t = s_pos(&a, uid);
        assert!(t[1] > -0.1 && t[0].abs() < 8.0 && t[2].abs() < 8.0, "body {uid} escaped to {t:?}");
    }
}

fn s_pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

#[test]
fn physics_scales_to_thousands_of_bodies() {
    let mut s = Sim::new(1);
    floor(&mut s);
    for i in 0..3000u64 {
        let (gx, gz, gy) = ((i % 20) as f32 * 1.2 - 12.0, ((i / 20) % 20) as f32 * 1.2 - 12.0, 0.6 + (i / 400) as f32 * 1.1);
        ball(&mut s, 10 + i, &format!("[{gx},{gy},{gz}]"), "{}");
    }
    let t = std::time::Instant::now();
    s.step_n(30);
    let per_tick = t.elapsed().as_secs_f64() * 1000.0 / 30.0;
    assert!(per_tick < 150.0, "3000 bodies took {per_tick:.1} ms per tick");
}

/// Every overlapping pair of unit spheres (at least one of them dynamic), found the slow way.
fn brute_force_pairs(centres: &[[f32; 3]], first_uid: u64) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    for i in 0..centres.len() {
        for k in i + 1..centres.len() {
            let d: f32 = (0..3).map(|a| (centres[i][a] - centres[k][a]).powi(2)).sum();
            if d < 1.0 {
                out.push((first_uid + i as u64, first_uid + k as u64));
            }
        }
    }
    out
}

#[test]
fn the_broad_phase_finds_exactly_the_overlapping_pairs_on_any_layout() {
    // a tower (all x equal), a row, a flat sheet and a scattered cloud: the sweep axis changes, the pairs must not
    let mut rng = 0x1234_5678u32;
    let mut next = move || {
        rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (rng >> 8) as f32 / (1 << 24) as f32
    };
    let layouts: Vec<(&str, Vec<[f32; 3]>)> = vec![
        ("tower", (0..300).map(|i| [0.0, i as f32 * 0.7 + next() * 0.1, 0.0]).collect()),
        ("row", (0..300).map(|i| [i as f32 * 0.7 + next() * 0.1, 0.0, 0.0]).collect()),
        ("sheet", (0..300).map(|i| [(i % 20) as f32 * 0.7 + next() * 0.1, 0.0, (i / 20) as f32 * 0.7]).collect()),
        ("cloud", (0..300).map(|_| [next() * 8.0, next() * 8.0, next() * 8.0]).collect()),
    ];
    for (name, centres) in layouts {
        let mut s = Sim::new(1);
        for (i, c) in centres.iter().enumerate() {
            ball(&mut s, 100 + i as u64, &format!("[{},{},{}]", c[0], c[1], c[2]), r#"{"gravity_scale":0}"#);
        }
        let stored: Vec<[f32; 3]> = (0..centres.len()).map(|i| s_pos(&s, 100 + i as u64)).collect();
        s.step();
        let mut got: Vec<(u64, u64)> = s.contacts.iter().map(|c| (c.a, c.b)).collect();
        got.sort();
        let want = brute_force_pairs(&stored, 100);
        assert!(!want.is_empty(), "{name}: the layout should have some overlaps");
        assert_eq!(got, want, "{name}");
    }
}

#[test]
fn the_grid_broad_phase_finds_exactly_the_contacts_the_sweep_of_all_bodies_finds() {
    use crate::physics::{BROAD_SWEEP_ALL, GRID_BUILT, GRID_CHECK, GRID_PATCHED};
    // a scene that takes the reference path (one body spins) with hundreds of static colliders of every size (a floor and a long wall among them: wider than the grid's cells), a dozen movers of each shape,
    // triggers, collision layers and bodies that fall, slide, are thrown through the scenery and bump into each other: with the grid and with the sweep of all bodies the contacts of every step and the world
    // after 150 steps are the same
    let build = |sweep_all: bool| -> (Vec<u64>, Vec<usize>, u32, u32) {
        BROAD_SWEEP_ALL.with(|f| f.set(sweep_all));
        GRID_CHECK.with(|f| f.set(!sweep_all));
        GRID_BUILT.with(|f| f.set(0));
        GRID_PATCHED.with(|f| f.set(0));
        let mut s = Sim::new(1);
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut rnd = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ((state >> 40) as f32) / 16_777_216.0
        };
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[400,1,400]}}"#);
        spawn(&mut s, 2, r#"{"Transform":{"pos":[0,3,-60]},"Collider":{"shape":1,"size":[300,6,1]}}"#);
        let mut uid = 10u64;
        for _ in 0..260 {
            let (x, z) = ((rnd() - 0.5) * 120.0, (rnd() - 0.5) * 120.0);
            let (w, h, d) = (0.5 + rnd() * 6.0, 0.5 + rnd() * 5.0, 0.5 + rnd() * 6.0);
            let shape = if rnd() < 0.2 { 0 } else { 1 };
            let layer = if rnd() < 0.3 { r#","CollisionLayer":{"layer":2,"mask":7}"# } else { "" };
            spawn(&mut s, uid, &format!(r#"{{"Transform":{{"pos":[{x},{},{z}]}},"Collider":{{"shape":{shape},"size":[{w},{h},{d}]}}{layer}}}"#, h * 0.5));
            uid += 1;
        }
        // movers: boxes, balls, a capsule, a spinning box; thrown about
        for k in 0..14u64 {
            let (x, z) = ((rnd() - 0.5) * 100.0, (rnd() - 0.5) * 100.0);
            let (vx, vz) = ((rnd() - 0.5) * 30.0, (rnd() - 0.5) * 30.0);
            let shape = [0, 1, 2][(k % 3) as usize];
            let spin = if k == 3 { r#","AngularVelocity":{"damping":0}"# } else { "" };
            let layer = if k % 4 == 0 { r#","CollisionLayer":{"layer":1,"mask":3}"# } else { "" };
            spawn(&mut s, 1000 + k, &format!(r#"{{"Transform":{{"pos":[{x},{},{z}]}},"Velocity":{{"lin":[{vx},{},{vz}]}},"Collider":{{"shape":{shape},"size":[1,1.4,1]}},"RigidBody":{{"mass":1,"restitution":0.4}}{spin}{layer}}}"#, 2.0 + rnd() * 6.0, rnd() * 8.0));
        }
        // triggers: one small, one wide
        spawn(&mut s, 2000, r#"{"Transform":{"pos":[5,1,5]},"Collider":{"shape":1,"size":[4,3,4],"trigger":true}}"#);
        spawn(&mut s, 2001, r#"{"Transform":{"pos":[-20,1,10]},"Collider":{"shape":1,"size":[150,3,20],"trigger":true}}"#);
        let (mut hashes, mut contacts) = (Vec::new(), Vec::new());
        for step in 0..150u64 {
            // a dozen of the static colliders are doors and platforms that are moved about (a step in two), one goes at step 60 and one comes at step 90 (the numbers of the rest shift)
            if step % 2 == 0 {
                for k in 0..12u64 {
                    let e = s.world.entity(10 + k).unwrap();
                    let a = (step * 5 + k * 11) as f32;
                    let p = &mut s.world.get_mut::<Transform>(e).unwrap().pos;
                    p[0] += a.sin() * 3.0;
                    p[2] += a.cos() * 3.0;
                }
            }
            if step == 60 {
                s.apply(&Command::Despawn { uid: 40 }).unwrap();
            }
            if step == 90 {
                spawn(&mut s, 3000, r#"{"Transform":{"pos":[3,1,3]},"Collider":{"shape":1,"size":[3,2,3]}}"#);
            }
            s.step();
            contacts.push(s.contacts.len());
            if step % 10 == 9 {
                hashes.push(s.state_hash());
            }
        }
        BROAD_SWEEP_ALL.with(|f| f.set(false));
        GRID_CHECK.with(|f| f.set(false));
        (hashes, contacts, GRID_BUILT.with(|f| f.get()), GRID_PATCHED.with(|f| f.get()))
    };
    let (grid, c_grid, built, patched) = build(false);
    let (sweep, c_sweep, _, _) = build(true);
    assert!(built <= 4 && patched > 100, "the grid is kept between steps, made again only when the numbers of the statics shift and patched for the doors: built {built}, patched {patched}");
    assert_eq!(grid, sweep, "the world after every 10 steps");
    assert_eq!(c_grid, c_sweep, "the number of contacts of every step");
    assert!(c_grid.iter().sum::<usize>() > 300, "the scene has contacts to compare: {}", c_grid.iter().sum::<usize>());
}
