//! `CameraCollide`: a third-person camera that stays out of walls and the ground (found by building the 3D game Caldera: the orbit camera went through trees and the side of hills).
use super::*;
use components::{CameraCollide, CameraRig, Transform};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn orbit_world() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Collider":{"shape":0,"size":[2,2,2]}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,1,8]},"Camera":{},"CameraRig":{"target":1,"mode":1,"yaw":0,"pitch":0,"distance":8,"follow_speed":0,"look_offset":[0,1,0]},"CameraCollide":{"radius":0.5,"min_distance":1.0}}"#);
    s
}

#[test]
fn a_camera_with_camera_collide_stops_in_front_of_a_wall_and_comes_back_when_it_is_gone() {
    let mut s = orbit_world();
    s.step();
    assert_eq!(pos(&s, 2), [0.0, 1.0, 8.0], "nothing in the way: the rig's place");
    // a wall five metres behind the hero, across the way of the camera
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,1,5]},"Collider":{"shape":1,"size":[10,10,1]}}"#);
    s.step();
    let p = pos(&s, 2);
    assert!((p[2] - 4.0).abs() < 0.02, "the camera is half a metre in front of the wall's face (z 4.5): {p:?}");
    assert!((p[0]).abs() < 1e-4 && (p[1] - 1.0).abs() < 1e-3, "and still on the line from the target: {p:?}");
    // a wall right behind the hero: the camera never comes nearer than `min_distance`
    s.apply(&Command::Despawn { uid: 3 }).unwrap();
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,1,1.4]},"Collider":{"shape":1,"size":[10,10,0.4]}}"#);
    s.step();
    let p = pos(&s, 2);
    assert!((p[2] - 1.0).abs() < 1e-3, "{p:?}");
    // the way is free again
    s.apply(&Command::Despawn { uid: 4 }).unwrap();
    s.step();
    assert_eq!(pos(&s, 2), [0.0, 1.0, 8.0]);
}

#[test]
fn a_camera_with_recover_is_pulled_in_at_once_and_let_out_slowly() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Collider":{"shape":0,"size":[2,2,2]}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,1,8]},"Camera":{},"CameraRig":{"target":1,"mode":1,"yaw":0,"pitch":0,"distance":8,"follow_speed":0,"look_offset":[0,1,0]},"CameraCollide":{"radius":0.5,"min_distance":1.0,"recover":4}}"#);
    s.step();
    assert_eq!(pos(&s, 2), [0.0, 1.0, 8.0], "nothing in the way: the rig's place");
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,1,5]},"Collider":{"shape":1,"size":[10,10,1]}}"#);
    s.step();
    let z = pos(&s, 2)[2];
    assert!((z - 4.0).abs() < 0.02, "pulled in at once, in front of the wall: {z}");
    // the wall goes: the camera comes out over about a second, never backwards, never past the rig's place
    s.apply(&Command::Despawn { uid: 3 }).unwrap();
    let mut last = z;
    let mut first = 0.0;
    for i in 0..300 {
        s.step();
        let z = pos(&s, 2)[2];
        assert!(z >= last - 1e-4 && z <= 8.0 + 1e-4, "tick {i}: {z} after {last}");
        if i == 0 {
            first = z;
        }
        last = z;
    }
    assert!(first > 4.0 && first < 4.3, "the first tick lets out a little: {first}");
    assert!((last - 8.0).abs() < 0.01, "and the way is free in the end: {last}");
    let held = s.world.get::<CameraCollide>(s.world.entity(2).unwrap()).unwrap().held;
    assert_eq!(held, 0.0, "nothing is held any more");
    // a nearer wall while it is coming out pulls it in again at once
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,1,3]},"Collider":{"shape":1,"size":[10,10,1]}}"#);
    s.step();
    assert!((pos(&s, 2)[2] - 2.0).abs() < 0.02, "{:?}", pos(&s, 2));
}

#[test]
fn a_camera_with_pull_in_rounds_off_a_small_step_and_never_goes_through_the_wall() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Collider":{"shape":0,"size":[2,2,2]}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,1,8]},"Camera":{},"CameraRig":{"target":1,"mode":1,"yaw":0,"pitch":0,"distance":8,"follow_speed":0,"look_offset":[0,1,0]},"CameraCollide":{"radius":0.5,"min_distance":1.0,"pull_in":15}}"#);
    s.step();
    // a wall whose near face (z 7.75) puts the camera 0.75 m nearer (it stops 0.5 m in front of it, at 7.25): the step is eased over a few ticks
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,1,8.25]},"Collider":{"shape":1,"size":[10,10,1]}}"#);
    // (the ease may leave it up to 0.35 m, 0.7 of the margin, short of that place on the first tick)
    let mut zs = Vec::new();
    for _ in 0..30 {
        s.step();
        zs.push(pos(&s, 2)[2]);
    }
    assert!(zs[0] > 7.25 && zs[0] <= 7.25 + 0.35 + 1e-3, "the first tick: {}", zs[0]);
    assert!(zs.windows(2).all(|w| w[1] <= w[0] + 1e-5), "only ever in: {zs:?}");
    assert!((zs[29] - 7.25).abs() < 0.02, "and there in the end: {}", zs[29]);
    // a wall that suddenly stands 5 m closer is not gone through: at most 0.7 of the margin in front of the camera's place
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,1,3.0]},"Collider":{"shape":1,"size":[10,10,1]}}"#);
    s.step();
    let z = pos(&s, 2)[2];
    assert!(z > 2.0 && z <= 2.0 + 0.35 + 1e-3, "the wall's face is at 2.5: the camera is at 2.0 and may be 0.35 m short of it: {z}");
}

#[test]
fn whiskers_pull_the_camera_in_early_for_a_wall_beside_the_line_and_not_for_a_far_one() {
    let mk = |whisker: f32, wall_x: f32| {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Collider":{"shape":0,"size":[2,2,2]}}"#);
        spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,1,8]}},"Camera":{{}},"CameraRig":{{"target":1,"mode":1,"yaw":0,"pitch":0,"distance":8,"follow_speed":0,"look_offset":[0,1,0]}},"CameraCollide":{{"radius":0.5,"min_distance":1.0,"whisker":{whisker}}}}}"#));
        // a wall across z = 5, from x = wall_x to the right
        spawn(&mut s, 3, &format!(r#"{{"Transform":{{"pos":[{},1,5]}},"Collider":{{"shape":1,"size":[10,10,1]}}}}"#, wall_x + 5.0));
        s.step();
        pos(&s, 2)[2]
    };
    assert_eq!(mk(0.0, 0.3), 8.0, "without whiskers the line is free: nothing happens");
    let near = mk(0.5, 0.3);
    assert!(near < 8.0 && near > 4.0, "a wall 0.3 m from the line pulls the camera in early, but not all the way: {near}");
    let nearer = mk(0.5, 0.1);
    assert!(nearer <= near + 1e-4, "the nearer to the line, the more: {nearer} against {near}");
    assert_eq!(mk(0.5, 1.0), 8.0, "a wall farther than a whisker is not felt");
    // a wall on the line: the whiskers add nothing to the middle line's answer
    let on = mk(0.5, -5.0);
    assert!((on - 4.0).abs() < 0.05, "{on}");
}

#[test]
fn a_trigger_and_the_target_do_not_hold_the_camera_back_and_a_hill_does() {
    let mut s = orbit_world();
    // a trigger volume in the way is not a wall; the target's own collider is not one for its camera
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,1,5]},"Collider":{"shape":1,"size":[10,10,1],"trigger":true}}"#);
    s.step();
    assert_eq!(pos(&s, 2), [0.0, 1.0, 8.0]);
    // a camera looking along the ground: a ball of ground-sized radius between the target and the camera
    spawn(&mut s, 6, r#"{"Transform":{"pos":[0,0.5,6]},"Collider":{"shape":0,"size":[3,3,3]}}"#);
    s.step();
    let p = pos(&s, 2);
    assert!(p[2] < 5.0 && p[2] > 3.0, "{p:?}");
    // the rig keeps its own numbers
    let r = *s.world.get::<CameraRig>(s.world.entity(2).unwrap()).unwrap();
    assert_eq!((r.distance, r.mode), (8.0, 1));
}
