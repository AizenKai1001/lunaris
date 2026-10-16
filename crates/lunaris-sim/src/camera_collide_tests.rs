//! `CameraCollide`: a third-person camera that stays out of walls and the ground (found by building the 3D game Caldera: the orbit camera went through trees and the side of hills).
use super::*;
use components::{CameraRig, Transform};
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
