//! Floaters in the simulation: a body floats to the draft its density says, a tipped plank rights itself, waves roll a plank and a flat surface does not, a current carries a floater and a splash is reported, all of it the same on every run.
use super::*;
use components::{AngularVelocity, Floater, Transform, Velocity};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn world(water: &str) -> Sim {
    let mut s = Sim::new(1);
    s.threads = 1;
    spawn(&mut s, 1, water);
    s
}

const FLAT: &str = r#"{"Transform":{"pos":[0,0,0]},"Water":{"size":[200,200],"wave_height":0}}"#;

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn floater(s: &Sim, uid: u64) -> Floater {
    *s.world.get::<Floater>(s.world.entity(uid).unwrap()).unwrap()
}

/// How far the body's y axis leans from the vertical, in degrees.
fn lean(s: &Sim, uid: u64) -> f32 {
    let q = s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().rot;
    let up_y = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
    up_y.clamp(-1.0, 1.0).acos().to_degrees()
}

fn cube(density: f32, y: f32) -> String {
    format!(r#"{{"Transform":{{"pos":[0,{y},0]}},"Velocity":{{}},"Collider":{{"shape":1,"size":[1,1,1]}},"RigidBody":{{"mass":1,"restitution":0}},"Floater":{{"density":{density}}}}}"#)
}

/// A plank 2 m long, 1 m wide and 0.2 m thick that can tip, turned `deg` degrees about z.
fn plank(deg: f32, y: f32, extra: &str) -> String {
    let (s, c) = (deg.to_radians() * 0.5).sin_cos();
    format!(r#"{{"Transform":{{"pos":[0,{y},0],"rot":[0,0,{s},{c}]}},"Velocity":{{}},"AngularVelocity":{{}},"Collider":{{"shape":1,"size":[2,0.2,1]}},"RigidBody":{{"mass":1,"restitution":0}},"Floater":{{"density":500{extra}}}}}"#)
}

#[test]
fn a_body_settles_at_the_draft_its_density_gives() {
    let mut s = world(FLAT);
    spawn(&mut s, 10, &cube(500.0, 3.0));
    spawn(&mut s, 11, &cube(250.0, 3.0).replace("[0,3,0]", "[20,3,0]"));
    spawn(&mut s, 12, &cube(1500.0, 3.0).replace("[0,3,0]", "[40,3,0]"));
    s.step_n(600);
    // half under: the middle at the surface; a quarter under: the middle a quarter of a metre above it; heavier than water: still sinking
    assert!(pos(&s, 10)[1].abs() < 0.1, "{:?}", pos(&s, 10));
    assert!((pos(&s, 11)[1] - 0.25).abs() < 0.1, "{:?}", pos(&s, 11));
    assert!(pos(&s, 12)[1] < -2.0, "{:?}", pos(&s, 12));
    assert!((floater(&s, 10).submerged - 0.5).abs() < 0.1 && (floater(&s, 11).submerged - 0.25).abs() < 0.1);
}

#[test]
fn a_tipped_plank_rights_itself_and_a_ball_floats_at_its_cap() {
    let mut s = world(FLAT);
    spawn(&mut s, 10, &plank(35.0, 0.3, ""));
    spawn(&mut s, 11, r#"{"Transform":{"pos":[20,2,0]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1,"restitution":0},"Floater":{"density":500}}"#);
    let start = lean(&s, 10);
    assert!(start > 30.0);
    s.step_n(420);
    assert!(lean(&s, 10) < 4.0, "the plank still leans {} degrees", lean(&s, 10));
    assert!(pos(&s, 10)[1].abs() < 0.12, "{:?}", pos(&s, 10));
    assert!(pos(&s, 11)[1].abs() < 0.1, "a ball of 500 floats with its middle at the surface: {:?}", pos(&s, 11));
}

#[test]
fn waves_roll_and_lift_a_plank_and_a_flat_surface_does_not() {
    let swing = |water: &str, extra: &str| -> (f32, f32) {
        let mut s = world(water);
        spawn(&mut s, 10, &plank(0.0, 0.0, extra));
        s.step_n(240);
        let (mut y0, mut y1, mut l1) = (f32::MAX, f32::MIN, 0.0f32);
        for _ in 0..360 {
            s.step();
            let y = pos(&s, 10)[1];
            y0 = y0.min(y);
            y1 = y1.max(y);
            l1 = l1.max(lean(&s, 10));
        }
        (y1 - y0, l1)
    };
    let rough = r#"{"Transform":{"pos":[0,0,0]},"Water":{"size":[200,200],"wave_height":1.0,"wave_length":4,"speed":3}}"#;
    let (heave, tilt) = swing(rough, "");
    assert!(heave > 0.25 && tilt > 3.0, "heave {heave} tilt {tilt}");
    // the same sea, but the plank takes no notice of waves
    let (heave0, tilt0) = swing(rough, r#","waves":0"#);
    assert!(heave0 < 0.02 && tilt0 < 0.5, "heave {heave0} tilt {tilt0}");
    let (heave1, tilt1) = swing(FLAT, "");
    assert!(heave1 < 0.02 && tilt1 < 0.5, "heave {heave1} tilt {tilt1}");
}

#[test]
fn a_current_carries_a_floater_and_current_zero_ignores_it() {
    let river = r#"{"Transform":{"pos":[0,0,0]},"Water":{"size":[400,400],"wave_height":0},"WaterFlow":{"velocity":[2,-1]}}"#;
    let mut s = world(river);
    spawn(&mut s, 10, &cube(500.0, 0.0));
    spawn(&mut s, 11, &cube(500.0, 0.0).replace(r#""density":500"#, r#""density":500,"current":0"#).replace("[0,0,0]", "[0,0,30]"));
    s.step_n(600);
    let (a, b) = (pos(&s, 10), pos(&s, 11));
    assert!(a[0] > 15.0 && a[2] < -7.0, "carried: {a:?}");
    assert!(b[0].abs() < 0.5 && (b[2] - 30.0).abs() < 0.5, "held: {b:?}");
}

#[test]
fn a_body_that_hits_the_water_fast_reports_a_splash_once() {
    let mut s = world(FLAT);
    spawn(&mut s, 10, r#"{"Transform":{"pos":[3,6,0]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1,"restitution":0},"Floater":{"density":500}}"#);
    let mut splashes = Vec::new();
    for _ in 0..240 {
        s.step();
        let f = floater(&s, 10);
        if f.splash > 0.0 {
            splashes.push((f.splash, pos(&s, 10)));
        }
    }
    assert_eq!(splashes.len(), 1, "{splashes:?}");
    // 5.5 m of free fall is about 10 m/s
    assert!(splashes[0].0 > 8.0 && splashes[0].0 < 12.5, "{splashes:?}");
}

#[test]
fn a_body_away_from_the_water_is_left_alone_and_the_same_run_gives_the_same_hash() {
    let run = || {
        let mut s = world(FLAT);
        spawn(&mut s, 10, &cube(500.0, 40.0).replace("[0,40,0]", "[500,40,0]"));
        spawn(&mut s, 11, &plank(20.0, 0.5, ""));
        s.step_n(200);
        s
    };
    let a = run();
    // 500 m from the water the body is in plain free fall: 200 ticks at 1/60 s of -9.8 m/s2 (no drag)
    let v = a.world.get::<Velocity>(a.world.entity(10).unwrap()).unwrap().lin;
    assert!((v[1] + 9.8 * 200.0 / 60.0).abs() < 0.2 && v[0] == 0.0 && floater(&a, 10).submerged == 0.0, "{v:?}");
    let _ = a.world.get::<AngularVelocity>(a.world.entity(11).unwrap()).unwrap();
    assert_eq!(a.state_hash(), run().state_hash());
}

/// `cargo test --release -p lunaris-sim floaters_cost -- --ignored --nocapture`: what the floaters cost a tick.
#[test]
#[ignore]
fn floaters_cost() {
    let rough = r#"{"Transform":{"pos":[0,0,0]},"Water":{"size":[400,400],"wave_height":1.0,"wave_length":6,"speed":2}}"#;
    for (label, count, near) in [("2000 planks in rough water", 2000usize, true), ("2000 crates 1 km away", 2000, false), ("2000 planks deep under still water", 2000, true)] {
        let water = if label.contains("deep") { FLAT } else { rough };
        let mut s = world(water);
        for i in 0..count {
            let (x, z) = ((i % 50) as f32 * 7.0 - 175.0, (i / 50) as f32 * 7.0 - 140.0);
            let y = if label.contains("deep") { -8.0 } else { 0.0 };
            let (x, z) = if near { (x, z) } else { (x + 1000.0, z + 1000.0) };
            spawn(&mut s, 100 + i as u64, &plank(0.0, y, "").replace("[0,", &format!("[{x},")).replace(&format!(",{y},0]"), &format!(",{y},{z}]")));
        }
        s.step_n(30);
        let mut events = Vec::new();
        let t = std::time::Instant::now();
        let ticks = 200;
        for _ in 0..ticks {
            crate::floaters::step(&mut s.world, &s.heightfields, 1.0 / 60.0, &mut events);
        }
        let per = t.elapsed().as_secs_f64() * 1000.0 / ticks as f64;
        println!("{label}: {per:.3} ms per tick ({:.0} ns per body)", per * 1.0e6 / count as f64);
    }
}
