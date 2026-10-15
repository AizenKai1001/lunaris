//! Terrain in the simulation: heightmaps read from the workspace, things falling onto them and standing on them, and what the hash says.
use super::*;
use crate::png::tests::make_png;
use crate::terrain::{Heightfield, Placed};
use components::{CharacterBody, Terrain, Transform, Velocity};
use lunaris_json::{parse, Json};
use std::path::PathBuf;

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = j(comps) else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// A fresh workspace folder with the given 33 x 33 pictures in it.
fn workspace(name: &str, pictures: &[(&str, &dyn Fn(usize, usize) -> u16)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lunaris-terrain-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, f) in pictures {
        let samples: Vec<u16> = (0..33 * 33).map(|i| f(i % 33, i / 33)).collect();
        std::fs::write(dir.join(file), make_png(33, 33, 16, 0, 4, &samples)).unwrap();
    }
    dir
}

/// A round hill in the middle: 0 at the rim, 65535 at the top.
fn hill(x: usize, z: usize) -> u16 {
    let (dx, dz) = (x as f32 - 16.0, z as f32 - 16.0);
    let d = (dx * dx + dz * dz).sqrt() / 16.0;
    ((-3.0 * d * d).exp() * 65535.0) as u16
}

/// Level ground half way up, with a cliff down the middle.
fn cliff(x: usize, _z: usize) -> u16 {
    if x < 16 {
        0
    } else {
        65535
    }
}

fn sim_with(dir: &PathBuf, heightmap: &str) -> Sim {
    let mut s = Sim::new(1);
    s.threads = 1;
    s.asset_root = Some(dir.clone());
    // 64 x 64 metres, 10 metres of relief, ground level 0
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Terrain":{{"heightmap":"{heightmap}","size":[64,64],"height":10}}}}"#));
    s
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

/// The height of the ground under (x, z), asked of the same code the physics uses.
fn ground(s: &Sim, x: f32, z: f32) -> f32 {
    let e = s.world.entity(1).unwrap();
    let (t, g) = (*s.world.get::<Transform>(e).unwrap(), *s.world.get::<Terrain>(e).unwrap());
    let name = s.world.strings().get(g.heightmap.0).to_string();
    Placed::new(s.heightfields.get(&name).expect("loaded"), t.pos, &g).surface(x, z).expect("over the ground")
}

#[test]
fn the_heightmap_is_read_on_the_first_step_and_balls_come_to_rest_on_the_ground() {
    let dir = workspace("balls", &[("hill.png", &hill)]);
    let mut s = sim_with(&dir, "hill.png");
    assert!(s.heightfields.is_empty(), "nothing is read before the first step");
    for (uid, (x, z)) in [(10u64, (-24.0f32, -20.0f32)), (11, (0.0, 0.0)), (12, (9.0, -5.0)), (13, (-8.0, 12.0))] {
        spawn(&mut s, uid, &format!(r#"{{"Transform":{{"pos":[{x},25,{z}]}},"Velocity":{{}},"Collider":{{"shape":0,"size":[1,1,1]}},"RigidBody":{{"mass":1,"restitution":0.1,"friction":0.9}}}}"#));
    }
    s.step_n(400);
    assert!(s.heightfields.contains_key("hill.png"), "read once, kept");
    for uid in [10u64, 11, 12, 13] {
        let p = pos(&s, uid);
        let rest = ground(&s, p[0], p[2]) + 0.5;
        // on a slope the centre of a ball sits a little higher than "radius above the height under it" (it touches uphill of its centre)
        assert!(p[1] >= rest - 0.1 && p[1] < rest + 1.2, "ball {uid} at {p:?}, the ground under it gives {rest}");
        let v = s.world.get::<Velocity>(s.world.entity(uid).unwrap()).unwrap().lin;
        assert!(v[1].abs() < 0.5, "ball {uid} has settled vertically: {v:?}");
    }
    // the top of the hill is a real hill: the ball dropped there rests about 10 m up
    assert!(pos(&s, 11)[1] > 9.0 || (pos(&s, 11)[0].abs() + pos(&s, 11)[2].abs()) > 0.5, "{:?}", pos(&s, 11));
    // the contacts name the terrain (uid 1)
    assert!(s.contacts.iter().any(|c| c.a == 1 || c.b == 1), "{:?}", s.contacts);
}

#[test]
fn a_ball_set_on_a_slope_rolls_down_it_and_a_box_stays_on_level_ground() {
    let dir = workspace("slope", &[("hill.png", &hill)]);
    let mut s = sim_with(&dir, "hill.png");
    // 8 m from the middle: the slope there is about 20 degrees
    let y = ground(&{
        let mut t = Sim::new(1);
        t.asset_root = Some(dir.clone());
        spawn(&mut t, 1, r#"{"Transform":{"pos":[0,0,0]},"Terrain":{"heightmap":"hill.png","size":[64,64],"height":10}}"#);
        t.step();
        t
    }, 8.0, 0.0);
    spawn(&mut s, 5, &format!(r#"{{"Transform":{{"pos":[8,{},0]}},"Velocity":{{}},"Collider":{{"shape":0,"size":[1,1,1]}},"RigidBody":{{"mass":1,"restitution":0,"friction":0.05}}}}"#, y + 0.6));
    s.step_n(240);
    assert!(pos(&s, 5)[0] > 12.0, "it rolled away from the top: {:?}", pos(&s, 5));
    // a box on the flat rim of the ground (the heightmap is ~0 there)
    spawn(&mut s, 6, r#"{"Transform":{"pos":[-29,6,-29]},"Velocity":{},"Collider":{"shape":1,"size":[1,1,1]},"RigidBody":{"mass":1,"restitution":0,"friction":0.8}}"#);
    s.step_n(240);
    let p = pos(&s, 6);
    let g = ground(&s, p[0], p[2]);
    assert!(p[1] > g + 0.4 && p[1] < g + 0.6, "the box lies on the ground: {p:?} over {g}");
}

#[test]
fn a_character_walks_up_a_gentle_slope_and_is_stopped_by_a_cliff() {
    let dir = workspace("walk", &[("hill.png", &hill), ("cliff.png", &cliff)]);
    // up the hill
    let mut s = sim_with(&dir, "hill.png");
    spawn(&mut s, 7, r#"{"Transform":{"pos":[-24,0.5,0]},"CharacterBody":{"radius":0.4,"height":1.8,"walk":[4,0]}}"#);
    s.step_n(30);
    let start_y = pos(&s, 7)[1];
    s.step_n(300);
    let p = pos(&s, 7);
    let cb = *s.world.get::<CharacterBody>(s.world.entity(7).unwrap()).unwrap();
    assert!(p[0] > -10.0, "it walked on: {p:?}");
    assert!(p[1] > start_y + 3.0, "and went up with the ground: {p:?}, started at {start_y}");
    assert!(cb.grounded || (p[1] - ground(&s, p[0], p[2])).abs() < 1.0, "{cb:?}");
    // against a cliff of 10 m in 2 m
    let mut c = sim_with(&dir, "cliff.png");
    spawn(&mut c, 8, r#"{"Transform":{"pos":[-10,0.5,0]},"CharacterBody":{"radius":0.4,"height":1.8,"walk":[4,0]}}"#);
    c.step_n(300);
    let p = pos(&c, 8);
    assert!(p[0] < 0.5 && p[1] < 3.0, "the wall of ground holds it: {p:?}");
}

#[test]
fn what_the_hash_sees_is_the_picture_and_a_missing_one_is_a_message_not_a_crash() {
    let dir = workspace("hash", &[("a.png", &hill), ("b.png", &cliff)]);
    let (mut a1, mut a2, mut b) = (sim_with(&dir, "a.png"), sim_with(&dir, "a.png"), sim_with(&dir, "b.png"));
    for s in [&mut a1, &mut a2, &mut b] {
        spawn(s, 9, r#"{"Transform":{"pos":[0,30,0]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1}}"#);
        s.step_n(50);
    }
    assert_eq!(a1.state_hash(), a2.state_hash(), "the same ground, the same world");
    assert_ne!(a1.state_hash(), b.state_hash(), "another picture, another world");
    // a snapshot and a replay keep the heightmaps with them
    let copy = a1.clone();
    assert_eq!(copy.state_hash(), a1.state_hash());
    // a heightmap that is not there: no ground, an explanation, the simulation goes on
    let mut m = sim_with(&dir, "nowhere.png");
    spawn(&mut m, 9, r#"{"Transform":{"pos":[0,5,0]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1}}"#);
    m.step_n(120);
    assert!(pos(&m, 9)[1] < -5.0, "it fell through: {:?}", pos(&m, 9));
    assert!(m.terrain_errors.get("nowhere.png").is_some_and(|e| e.contains("cannot read")), "{:?}", m.terrain_errors);
    // and one that is not a picture
    std::fs::write(dir.join("junk.png"), b"hello").unwrap();
    let mut jk = sim_with(&dir, "junk.png");
    jk.step();
    assert!(jk.terrain_errors.get("junk.png").is_some_and(|e| e.contains("not a PNG")), "{:?}", jk.terrain_errors);
    // a path that leaves the workspace
    assert!(Heightfield::read(&dir, "../x.png").is_err() && Heightfield::read(&dir, "").is_err());
    // terrain that is not solid is not stood on
    let mut ns = sim_with(&dir, "a.png");
    ns.world.get_mut::<Terrain>(ns.world.entity(1).unwrap()).unwrap().solid = false;
    spawn(&mut ns, 9, r#"{"Transform":{"pos":[0,15,0]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1}}"#);
    ns.step_n(200);
    assert!(pos(&ns, 9)[1] < 0.0);
}

#[test]
fn terrain_in_the_world_does_not_change_what_a_world_without_it_does() {
    // the same falling ball with a plain floor, with and without a terrain far away (outside its extent): identical motion
    let dir = workspace("far", &[("hill.png", &hill)]);
    let run = |with: bool| {
        let mut s = Sim::new(1);
        s.threads = 1;
        s.asset_root = Some(dir.clone());
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[20,1,20]}}"#);
        if with {
            spawn(&mut s, 2, r#"{"Transform":{"pos":[500,0,500]},"Terrain":{"heightmap":"hill.png","size":[64,64],"height":10}}"#);
        }
        spawn(&mut s, 3, r#"{"Transform":{"pos":[1,6,1]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1,"restitution":0.5}}"#);
        s.step_n(200);
        pos(&s, 3)
    };
    assert_eq!(run(true), run(false));
}

/// A wall of ground 10 m high across the middle (columns 14 to 18) with a gap in its north end (picture rows 0 to 3, z from -16 to -13).
fn wall_with_gap(x: usize, z: usize) -> u16 {
    if (14..=18).contains(&x) && z > 3 {
        65535
    } else {
        0
    }
}

/// Ground that rises evenly from the west edge to the east edge.
fn ramp(x: usize, _z: usize) -> u16 {
    (x * 65535 / 32) as u16
}

#[test]
fn a_path_goes_through_the_gap_in_a_wall_of_ground_and_not_over_its_cliffs() {
    let dir = workspace("navgap", &[("wall.png", &wall_with_gap)]);
    let mut s = Sim::new(1);
    s.asset_root = Some(dir);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Terrain":{"heightmap":"wall.png","size":[32,32],"height":10}}"#);
    s.step(); // reads the picture
    let r = crate::nav::find_path_on(&mut s.world, [-10.0, 0.0, 8.0], [10.0, 0.0, 8.0], crate::nav::NavParams::default(), &s.heightfields).expect("a way round");
    assert!(r.points.iter().any(|p| p[2] < -12.0), "the way goes round through the gap in the north: {:?}", r.points);
    assert!(r.points.iter().all(|p| p[1] < 0.5), "and stays on the low ground: {:?}", r.points);
    assert!(r.length > 40.0, "round the wall, much longer than the 20 m between the points: {}", r.length);
    // without the picture being read, the engine does not know the wall: a straight line
    let blind = crate::nav::find_path(&mut s.world, [-10.0, 0.0, 8.0], [10.0, 0.0, 8.0], crate::nav::NavParams::default()).unwrap();
    assert!(blind.length < 21.0, "{}", blind.length);
}

#[test]
fn an_agent_walks_up_a_gentle_slope_to_a_target_on_it_and_replays_the_same() {
    let dir = workspace("navramp", &[("ramp.png", &ramp)]);
    let run = || {
        let mut s = Sim::new(1);
        s.threads = 1;
        s.asset_root = Some(dir.clone());
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Terrain":{"heightmap":"ramp.png","size":[32,32],"height":6}}"#);
        spawn(&mut s, 2, r#"{"Transform":{"pos":[-12,0.8,0]},"CharacterBody":{},"NavAgent":{"target":[12,5.25,0],"active":true,"speed":4}}"#);
        s.step_n(480);
        s
    };
    let s = run();
    let a = *s.world.get::<components::NavAgent>(s.world.entity(2).unwrap()).unwrap();
    let p = pos(&s, 2);
    assert_eq!(a.status, 2, "arrived: {a:?} at {p:?}");
    assert!((p[0] - 12.0).abs() < 0.6 && p[1] > 5.0 && p[1] < 5.5, "on the slope at the target: {p:?}");
    assert_eq!(s.state_hash(), run().state_hash());
}

#[test]
fn a_cliff_with_no_way_up_is_no_path() {
    let dir = workspace("navcliff", &[("cliff.png", &cliff)]);
    let mut s = Sim::new(1);
    s.asset_root = Some(dir);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Terrain":{"heightmap":"cliff.png","size":[32,32],"height":10}}"#);
    s.step();
    let r = crate::nav::find_path_on(&mut s.world, [-10.0, 0.0, 0.0], [10.0, 10.0, 0.0], crate::nav::NavParams::default(), &s.heightfields);
    assert!(r.is_err(), "ten metres up a wall is not a path: {:?}", r.map(|r| r.points));
}
