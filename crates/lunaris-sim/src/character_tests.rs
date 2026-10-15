use super::*;
use components::{CharacterBody, Transform};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn body(s: &Sim, uid: u64) -> CharacterBody {
    *s.world.get::<CharacterBody>(s.world.entity(uid).unwrap()).unwrap()
}

fn set_body(s: &mut Sim, uid: u64, f: impl FnOnce(&mut CharacterBody)) {
    let e = s.world.entity(uid).unwrap();
    f(s.world.get_mut::<CharacterBody>(e).unwrap());
}

/// A floor whose top is at y = 0, and a hero standing on it at the origin.
fn world_with_hero() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0.5,0]},"CharacterBody":{}}"#);
    s
}

#[test]
fn a_character_falls_onto_the_floor_and_stands_there() {
    let mut s = world_with_hero();
    s.step_n(60);
    assert!(pos(&s, 2)[1].abs() < 1e-3, "{:?}", pos(&s, 2));
    let b = body(&s, 2);
    assert!(b.grounded && b.fall == 0.0, "{b:?}");
    // and stays put for as long as nothing asks it to move
    let before = pos(&s, 2);
    s.step_n(600);
    assert!((0..3).all(|k| (pos(&s, 2)[k] - before[k]).abs() < 1e-4), "{:?} vs {before:?}", pos(&s, 2));
}

#[test]
fn walking_moves_at_the_asked_speed_and_a_wall_stops_it_but_not_the_sliding() {
    let mut s = world_with_hero();
    s.step_n(30);
    set_body(&mut s, 2, |b| b.walk = [3.0, 0.0]);
    s.step_n(60); // one second
    assert!((pos(&s, 2)[0] - 3.0).abs() < 0.01, "{:?}", pos(&s, 2));
    // a wall across the way at x = 5 (faces at 4.5 and 5.5)
    spawn(&mut s, 3, r#"{"Transform":{"pos":[5,1,0],"scale":[1,2,20]},"Collider":{"shape":1}}"#);
    s.step_n(120);
    let x = pos(&s, 2)[0];
    assert!((x - 4.15).abs() < 0.02, "stopped against the wall at 4.5 - 0.35: {x}");
    // walking diagonally into the wall slides along it
    set_body(&mut s, 2, |b| b.walk = [3.0, 3.0]);
    s.step_n(60);
    let p = pos(&s, 2);
    assert!((p[0] - 4.15).abs() < 0.02 && p[2] > 2.5, "slides along the wall: {p:?}");
}

#[test]
fn low_steps_are_climbed_and_tall_blocks_are_not() {
    let mut s = world_with_hero();
    s.step_n(5);
    s.step_n(25);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[3,0.15,0]},"Collider":{"shape":1,"size":[2,0.3,4]}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[-3,0.5,0]},"Collider":{"shape":1,"size":[2,1,4]}}"#);
    set_body(&mut s, 2, |b| b.walk = [3.0, 0.0]);
    let mut top = 0.0f32;
    for _ in 0..90 {
        s.step();
        top = top.max(pos(&s, 2)[1]);
    }
    assert!(pos(&s, 2)[0] > 4.0 && (top - 0.3).abs() < 0.02, "stepped up onto the 0.3 high block and on across it (highest {top}): {:?}", pos(&s, 2));
    assert!(pos(&s, 2)[1].abs() < 0.02 && body(&s, 2).grounded, "and walked down off the far side: {:?}", pos(&s, 2));
    // the other way: a 1.0 high block is a wall
    set_body(&mut s, 2, |b| b.walk = [-3.0, 0.0]);
    s.step_n(300);
    let p = pos(&s, 2);
    assert!((p[0] - (-2.0 + 0.35)).abs() < 0.03 && p[1].abs() < 0.02, "{p:?}");
}

#[test]
fn a_jump_needs_the_ground_and_is_one_request() {
    let mut s = world_with_hero();
    s.step_n(40);
    set_body(&mut s, 2, |b| b.jump = 5.0);
    s.step_n(1);
    assert!(body(&s, 2).fall > 4.0 && !body(&s, 2).grounded && body(&s, 2).jump == 0.0, "{:?}", body(&s, 2));
    // a request in mid-air does nothing
    s.step_n(5);
    let fall = body(&s, 2).fall;
    set_body(&mut s, 2, |b| b.jump = 5.0);
    s.step_n(1);
    assert!(body(&s, 2).fall < fall, "gravity kept pulling: {} then {}", fall, body(&s, 2).fall);
    // the apex is about v^2 / 2g = 1.28 and then it lands again
    let mut top = 0.0f32;
    for _ in 0..120 {
        s.step();
        top = top.max(pos(&s, 2)[1]);
    }
    assert!((top - 1.28).abs() < 0.15, "{top}");
    assert!(body(&s, 2).grounded && pos(&s, 2)[1].abs() < 1e-3);
}

#[test]
fn a_ceiling_stops_a_jump_triggers_and_other_characters_do_not_block_and_gravity_scale_zero_flies() {
    let mut s = world_with_hero();
    s.step_n(40);
    // a ceiling 2.2 above the floor: the 1.8 tall hero can rise only 0.4
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,2.7,0]},"Collider":{"shape":1,"size":[4,1,4]}}"#);
    set_body(&mut s, 2, |b| b.jump = 8.0);
    let mut top = 0.0f32;
    for _ in 0..60 {
        s.step();
        top = top.max(pos(&s, 2)[1]);
    }
    assert!(top < 0.45, "{top}");
    // a trigger zone and another character in the way are walked through
    spawn(&mut s, 4, r#"{"Transform":{"pos":[2,1,0],"scale":[1,2,4]},"Collider":{"shape":1,"trigger":true}}"#);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[3,0,0]},"CharacterBody":{}}"#);
    set_body(&mut s, 2, |b| b.walk = [3.0, 0.0]);
    s.step_n(120);
    assert!(pos(&s, 2)[0] > 5.0, "{:?}", pos(&s, 2));
    // no gravity: it stays where it is, in the air
    spawn(&mut s, 6, r#"{"Transform":{"pos":[0,5,10]},"CharacterBody":{"gravity_scale":0}}"#);
    s.step_n(60);
    assert_eq!(pos(&s, 6)[1], 5.0);
}

#[test]
fn bad_numbers_do_not_break_the_world_and_the_motion_is_replay_exact() {
    let mut s = world_with_hero();
    s.step_n(40);
    set_body(&mut s, 2, |b| {
        b.walk = [f32::NAN, f32::INFINITY];
        b.radius = -1.0;
        b.height = 0.0;
    });
    s.step_n(10);
    assert!(pos(&s, 2).iter().all(|v| v.is_finite()), "{:?}", pos(&s, 2));
    // the same inputs give the same state hash
    let run = || {
        let mut s = world_with_hero();
        s.step_n(5);
        set_body(&mut s, 2, |b| b.walk = [2.0, 1.0]);
        s.step_n(100);
        set_body(&mut s, 2, |b| b.jump = 6.0);
        s.step_n(100);
        s.state_hash()
    };
    assert_eq!(run(), run());
}
