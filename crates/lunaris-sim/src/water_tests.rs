//! Water bodies in the simulation: a character that walks into a lake gets `InWater` and loses it on the way out, bodies float to the depth their density says, the waves keep time with the world,
//! and a scene's hash knows the water's picture.
use super::*;
use crate::png::tests::make_png;
use components::{Buoyant, InWater, Transform, Velocity, WaterBody};
use lunaris_json::{parse, Json};
use std::path::PathBuf;

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = j(comps) else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// Distance of sample (x, z) of a 33 x 33 picture from its middle, in samples.
fn radius(x: usize, z: usize) -> f32 {
    ((x as f32 - 16.0).powi(2) + (z as f32 - 16.0).powi(2)).sqrt()
}

/// A bowl: 0 (black) in the middle, rising to 0.6 of the relief at the rim and beyond.
fn bowl(x: usize, z: usize) -> u16 {
    (0.6 * (radius(x, z) / 16.0).min(1.0) * 65535.0) as u16
}

/// A lake in the bowl: water at 0.3 of the relief (3 m of 10) within 8 samples of the middle, dry elsewhere.
fn lake(x: usize, z: usize) -> u16 {
    if radius(x, z) <= 8.0 {
        (0.3 * 65535.0) as u16
    } else {
        0
    }
}

/// A fresh workspace with the bowl and the lake pictures.
fn workspace(name: &str, lake_picture: &dyn Fn(usize, usize) -> u16) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lunaris-water-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, f) in [("bowl.png", &bowl as &dyn Fn(usize, usize) -> u16), ("lake.png", lake_picture)] {
        let samples: Vec<u16> = (0..33 * 33).map(|i| f(i % 33, i / 33)).collect();
        std::fs::write(dir.join(file), make_png(33, 33, 16, 0, 4, &samples)).unwrap();
    }
    dir
}

/// 64 x 64 metres of bowl (10 m of relief: 6 m at the rim), the lake's surface at 3 m, nobody in it yet.
fn world(dir: &PathBuf) -> Sim {
    let mut s = Sim::new(1);
    s.threads = 1;
    s.asset_root = Some(dir.clone());
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Terrain":{"heightmap":"bowl.png","size":[64,64],"height":10}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"WaterBody":{"heightmap":"lake.png","size":[64,64],"relief":10,"speed":2}}"#);
    s
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn in_water(s: &Sim, uid: u64) -> Option<InWater> {
    s.world.get::<InWater>(s.world.entity(uid).unwrap()).copied()
}

fn walk(s: &mut Sim, uid: u64, x: f32, z: f32) {
    s.world.get_mut::<components::CharacterBody>(s.world.entity(uid).unwrap()).unwrap().walk = [x, z];
}

#[test]
fn a_character_that_walks_into_the_lake_gets_in_water_with_its_depth_and_loses_it_on_the_way_out() {
    let dir = workspace("swim", &lake);
    let mut s = world(&dir);
    // the hero starts on the east bank, well above the surface (the ground there is 4.5 m up at x = 24)
    spawn(&mut s, 10, r#"{"Transform":{"pos":[24,6,0]},"CharacterBody":{"radius":0.3,"height":1.8}}"#);
    s.step_n(60);
    assert!(in_water(&s, 10).is_none(), "dry land: {:?}", in_water(&s, 10));
    assert_eq!(s.world.get::<components::CharacterBody>(s.world.entity(10).unwrap()).unwrap().grounded, true);
    // west into the bowl at 3 m/s: the ground falls away and the lake closes over the feet (the shore is where the ground is 3 m up: 16 m from the middle)
    walk(&mut s, 10, -3.0, 0.0);
    let mut first_wet: Option<f32> = None;
    for _ in 0..300 {
        s.step();
        if first_wet.is_none() && in_water(&s, 10).is_some() {
            first_wet = Some(pos(&s, 10)[0]);
        }
    }
    let shore = first_wet.expect("the hero got wet");
    assert!(shore < 17.5 && shore > 14.0, "it got wet where the ground meets the surface, about 16 m from the middle: {shore}");
    let p = pos(&s, 10);
    let w = in_water(&s, 10).expect("it is standing in the lake");
    // the feet are on the ground, the surface is 3 m up: the depth is the difference
    assert!((w.surface - 3.0).abs() < 0.01, "{w:?}");
    assert!((w.depth - (3.0 - p[1])).abs() < 1e-4, "depth {} at feet {}", w.depth, p[1]);
    assert!(w.depth > 1.0 && w.depth < 3.01, "{w:?} at {p:?}");
    assert_eq!(w.body.0, 2, "the water body it stands in");
    // and back out: east again, and the component goes
    walk(&mut s, 10, 3.0, 0.0);
    s.step_n(400);
    assert!(in_water(&s, 10).is_none(), "out of the water at {:?}", pos(&s, 10));
    assert!(pos(&s, 10)[0] > 17.0);
}

#[test]
fn the_state_hash_knows_the_water_picture_and_the_waves_keep_time_with_the_world() {
    let dir_a = workspace("hash-a", &lake);
    let dir_b = workspace("hash-b", &|x, z| if radius(x, z) <= 6.0 { (0.3 * 65535.0) as u16 } else { 0 });
    let hash = |dir: &PathBuf| {
        let mut s = world(dir);
        s.step_n(10);
        s.state_hash()
    };
    assert_eq!(hash(&dir_a), hash(&dir_a), "the same water, the same hash");
    assert_ne!(hash(&dir_a), hash(&dir_b), "another picture of the water, another hash");
    // without any water body nothing about it is in the hash: the same world with the water entity removed is as it was
    let mut dry = world(&dir_a);
    dry.apply(&Command::Despawn { uid: 2 }).unwrap();
    let mut plain = Sim::new(1);
    plain.threads = 1;
    plain.asset_root = Some(dir_a.clone());
    spawn(&mut plain, 1, r#"{"Transform":{"pos":[0,0,0]},"Terrain":{"heightmap":"bowl.png","size":[64,64],"height":10}}"#);
    plain.world.set_next_uid(dry.world.next_uid());
    dry.step_n(5);
    plain.step_n(5);
    assert_eq!(dry.state_hash(), plain.state_hash());
    // the waves' clock runs with the simulation and stops when it is paused
    let mut s = world(&dir_a);
    let phase = |s: &Sim| s.world.get::<WaterBody>(s.world.entity(2).unwrap()).unwrap().phase;
    s.step_n(60);
    assert!((phase(&s) - 2.0).abs() < 1e-3, "{}", phase(&s));
    s.world.set_resource("time.scale", Some(0.0)).unwrap();
    let held = phase(&s);
    s.step_n(30);
    assert_eq!(phase(&s), held);
}

/// A ball of the given density and a box, dropped into the middle of the lake.
fn floaters(dir: &PathBuf) -> Sim {
    let mut s = world(dir);
    // radius 0.5 and density 500: half under; density 3000: sinks; a 1 m cube of 250: a quarter of it under; a ball over the dry bank: a plain ball
    spawn(&mut s, 20, r#"{"Transform":{"pos":[0,7,0]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1,"restitution":0.1},"Buoyant":{"density":500}}"#);
    spawn(&mut s, 21, r#"{"Transform":{"pos":[3,7,3]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":3,"restitution":0.1},"Buoyant":{"density":3000}}"#);
    spawn(&mut s, 22, r#"{"Transform":{"pos":[-3,7,-3]},"Velocity":{},"Collider":{"shape":1,"size":[1,1,1]},"RigidBody":{"mass":1,"restitution":0.1},"Buoyant":{"density":250}}"#);
    spawn(&mut s, 23, r#"{"Transform":{"pos":[24,9,0]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1,"restitution":0.1},"Buoyant":{"density":500}}"#);
    s
}

#[test]
fn bodies_float_to_the_depth_their_density_says_heavy_ones_sink_and_dry_land_is_just_ground() {
    let dir = workspace("float", &lake);
    let mut s = floaters(&dir);
    s.step_n(600);
    let share = |uid: u64| s.world.get::<Buoyant>(s.world.entity(uid).unwrap()).unwrap().submerged;
    // density 500: half of the ball under the surface at 3 m, so its middle is at the surface
    assert!((pos(&s, 20)[1] - 3.0).abs() < 0.06, "the ball floats with its middle at the surface: {:?}", pos(&s, 20));
    assert!((share(20) - 0.5).abs() < 0.05, "{}", share(20));
    // density 250: a quarter of the cube under: its bottom is 0.25 m below the surface, its middle 0.25 m above
    assert!((pos(&s, 22)[1] - 3.25).abs() < 0.06, "the cube sits high: {:?}", pos(&s, 22));
    assert!((share(22) - 0.25).abs() < 0.05, "{}", share(22));
    // density 3000: heavier than water, it goes down to the bottom of the bowl (the ground there is near 0): a ball resting on it
    assert!(pos(&s, 21)[1] < 1.6, "the heavy ball sank to the bowl (the ground at 4 m from the middle is 0.8 m up): {:?}", pos(&s, 21));
    assert_eq!(share(21), 1.0, "all of it is under");
    // over the bank there is no water: the ball lies on the ground (4.5 m up at x = 24, a little more on its slope), nothing floats it
    assert_eq!(share(23), 0.0);
    assert!(pos(&s, 23)[1] < 6.5 && pos(&s, 23)[1] > 4.5, "{:?}", pos(&s, 23));
    // the same world is the same world (the floating is deterministic)
    let mut t = floaters(&dir);
    t.step_n(600);
    assert_eq!(s.state_hash(), t.state_hash());
}

#[test]
fn a_floating_body_is_slowed_by_the_water_and_a_body_without_buoyancy_ignores_it() {
    let dir = workspace("drag", &lake);
    let mut s = world(&dir);
    spawn(&mut s, 30, r#"{"Transform":{"pos":[0,12,0]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1,"restitution":0},"Buoyant":{"density":1000,"drag":6}}"#);
    // no Buoyant: falls through the surface as if it were not there (it stops on the ground)
    spawn(&mut s, 31, r#"{"Transform":{"pos":[2,12,0]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1,"restitution":0}}"#);
    let speed = |s: &Sim, uid: u64| s.world.get::<Velocity>(s.world.entity(uid).unwrap()).unwrap().lin[1];
    let mut slowest_diff = 0.0f32;
    for _ in 0..200 {
        s.step();
        if pos(&s, 30)[1] < 2.8 && pos(&s, 31)[1] > 0.6 {
            slowest_diff = slowest_diff.max(speed(&s, 30) - speed(&s, 31));
        }
    }
    assert!(slowest_diff > 1.0, "the buoyant ball fell slower than the plain one while both were in the water: {slowest_diff}");
    assert!(pos(&s, 31)[1] < 1.2, "the plain ball is on the bottom: {:?}", pos(&s, 31));
    // density 1000 is neutral: once its speed is gone the water neither lifts it nor lets it fall, and it never shoots out of the water
    s.step_n(600);
    assert!(pos(&s, 30)[1] > 0.3 && pos(&s, 30)[1] < 3.6, "a neutral ball stays in the water: {:?}", pos(&s, 30));
}

#[test]
fn a_water_body_whose_picture_is_missing_floats_nothing_and_odd_numbers_never_panic() {
    let dir = workspace("odd", &lake);
    let mut s = Sim::new(1);
    s.threads = 1;
    s.asset_root = Some(dir.clone());
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"WaterBody":{"heightmap":"nowhere.png","size":[64,64],"relief":10}}"#);
    spawn(&mut s, 20, r#"{"Transform":{"pos":[0,3,0]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1},"Buoyant":{"density":500}}"#);
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,0,0]},"CharacterBody":{}}"#);
    s.step_n(30);
    assert!(s.terrain_errors.contains_key("nowhere.png"));
    assert!(in_water(&s, 10).is_none());
    // numbers that mean nothing
    for (size, relief, density) in [("[0,0]", "0", "0"), ("[1e30,1e30]", "1e30", "-5"), ("[-10,10]", "-3", "1e30")] {
        let mut t = Sim::new(1);
        t.asset_root = Some(dir.clone());
        spawn(&mut t, 2, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"WaterBody":{{"heightmap":"lake.png","size":{size},"relief":{relief}}}}}"#));
        spawn(&mut t, 20, &format!(r#"{{"Transform":{{"pos":[0,3,0]}},"Velocity":{{}},"Collider":{{"shape":0,"size":[1,1,1]}},"RigidBody":{{"mass":1}},"Buoyant":{{"density":{density},"drag":-4}}}}"#));
        spawn(&mut t, 10, r#"{"Transform":{"pos":[0,-1,0]},"CharacterBody":{}}"#);
        t.step_n(20);
        let p = pos(&t, 20);
        assert!(p.iter().all(|v| v.is_finite()), "{p:?}");
    }
}
