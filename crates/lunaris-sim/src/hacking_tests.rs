//! Hacking: the goggles, a machine that is taken, driven, given back or lost. No art: boxes, capsules and bare components.
use super::*;
use crate::components::{CameraRig, CharacterBody, Hackable, Hacker, Heat, Perception, Team, Transform, Velocity, Weapon};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn res(s: &Sim, name: &str) -> f64 {
    s.world.resource(name).unwrap_or(0.0)
}

/// uid 1: the hacker (team 1, with a Heat), uid 5: a camera on it; uid 2: a machine 10 m ahead (team 2, an engaged Perception, a Velocity, a Weapon).
fn world(machine_extra: &str, hackable: &str) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,3,5]},"CameraRig":{"target":1}}"#);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Team":{"id":1},"Heat":{},"Hacker":{"camera":5,"trace_heat":0.5}}"#);
    let sep = if machine_extra.is_empty() { "" } else { "," };
    let hsep = if hackable.is_empty() { "" } else { "," };
    spawn(
        &mut s,
        2,
        &format!(r#"{{"Transform":{{"pos":[0,1,-10]}},"Team":{{"id":2}},"Perception":{{"engage":true}},"Velocity":{{}},"Weapon":{{"spread":0}},"Hackable":{{"difficulty":2,"range":20{hsep}{hackable}}}{sep}{machine_extra}}}"#),
    );
    s
}

fn hacker(s: &Sim) -> Hacker {
    *s.world.get::<Hacker>(s.world.entity(1).unwrap()).unwrap()
}

fn hackable(s: &Sim) -> Hackable {
    *s.world.get::<Hackable>(s.world.entity(2).unwrap()).unwrap()
}

fn team(s: &Sim, uid: u64) -> u32 {
    s.world.get::<Team>(s.world.entity(uid).unwrap()).unwrap().id
}

fn rig_target(s: &Sim) -> u64 {
    s.world.get::<CameraRig>(s.world.entity(5).unwrap()).unwrap().target.0
}

fn goggles(s: &mut Sim, f: impl FnOnce(&mut Hacker)) {
    let e = s.world.entity(1).unwrap();
    f(s.world.get_mut::<Hacker>(e).unwrap());
}

fn aim_and_hack(s: &mut Sim) {
    goggles(s, |h| {
        h.target = Uid(2);
        h.hack = true;
    });
}

use lunaris_ecs::Uid;

#[test]
fn hacking_fills_the_progress_leaves_a_trace_and_takes_the_machine() {
    let mut s = world("", "");
    aim_and_hack(&mut s);
    s.step_n(60);
    let (h, k) = (hacker(&s), hackable(&s));
    assert_eq!(h.state, 1);
    assert!((h.progress - 0.5).abs() < 0.04 && (k.progress - h.progress).abs() < 1e-6, "half the difficulty after a second: {h:?}");
    assert!(h.in_range);
    let heat = s.world.get::<Heat>(s.world.entity(1).unwrap()).unwrap().value;
    assert!(heat > 0.35, "hacking leaves a trace: {heat}");
    assert_eq!((team(&s, 2), k.hacked), (2, false), "not yet taken");
    s.step_n(65);
    let (h, k) = (hacker(&s), hackable(&s));
    assert_eq!((h.state, h.controlled.0), (2, 2), "{h:?}");
    assert!(k.hacked && k.controller.0 == 1 && k.saved_team == 2);
    assert_eq!(team(&s, 2), 1, "it is on the hacker's side now");
    let p = *s.world.get::<Perception>(s.world.entity(2).unwrap()).unwrap();
    assert!(!p.active && !p.engage, "its own AI is off");
    assert_eq!(rig_target(&s), 2, "the camera follows the machine");
    // a second hacker cannot take a machine that is taken (and the progress does not move)
}

#[test]
fn it_needs_range_and_a_line_of_sight() {
    // too far
    let mut s = world("", "");
    set_machine(&mut s, [0.0, 1.0, -40.0]);
    aim_and_hack(&mut s);
    s.step_n(200);
    assert_eq!((hacker(&s).state, hackable(&s).progress, hacker(&s).in_range), (0, 0.0, false));
    // a wall between them
    let mut s = world("", "");
    spawn(&mut s, 9, r#"{"Transform":{"pos":[0,2,-5]},"Collider":{"shape":1,"size":[8,4,0.3]}}"#);
    aim_and_hack(&mut s);
    s.step_n(200);
    assert_eq!((hacker(&s).state, hackable(&s).progress), (0, 0.0), "the wall blocks it");
    // a machine that does not need a line of sight is hacked through it
    let mut s = world("", r#""los":false"#);
    spawn(&mut s, 9, r#"{"Transform":{"pos":[0,2,-5]},"Collider":{"shape":1,"size":[8,4,0.3]}}"#);
    aim_and_hack(&mut s);
    s.step_n(150);
    assert_eq!(hacker(&s).state, 2);
    // hacking nothing, or a thing that is not hackable, does nothing
    let mut s = world("", "");
    goggles(&mut s, |h| {
        h.target = Uid(1);
        h.hack = true;
    });
    s.step_n(60);
    assert_eq!(hacker(&s).state, 0);
}

fn set_machine(s: &mut Sim, pos: [f32; 3]) {
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<Transform>(e).unwrap().pos = pos;
}

#[test]
fn a_taken_machine_is_driven_and_fires_and_is_given_back_whole() {
    let mut s = world("", "");
    spawn(&mut s, 7, r#"{"EventWatch":{"names":"hack_start, hacked, released, link_lost"}}"#);
    aim_and_hack(&mut s);
    let mut seen = [0.0f64; 2];
    for _ in 0..130 {
        s.step();
        seen[0] = seen[0].max(res(&s, "event.hack_start"));
        seen[1] = seen[1].max(res(&s, "event.hacked"));
    }
    assert_eq!(hacker(&s).state, 2);
    assert!(seen[0] >= 1.0 && seen[1] >= 1.0, "hack_start and hacked announced: {seen:?}");
    goggles(&mut s, |h| {
        h.move_dir = [1.0, 0.0, 0.0];
        h.fire = true;
    });
    s.step_n(40);
    let v = s.world.get::<Velocity>(s.world.entity(2).unwrap()).unwrap().lin;
    assert!((v[0] - 8.0).abs() < 0.01 && v[2].abs() < 1e-4, "it flies at its speed in the direction asked: {v:?}");
    assert!(s.world.get::<Weapon>(s.world.entity(2).unwrap()).unwrap().fire, "the hacker holds its trigger");
    // the direction is limited to length 1, and a longer one is not faster
    goggles(&mut s, |h| h.move_dir = [10.0, 0.0, 0.0]);
    s.step_n(40);
    let v = s.world.get::<Velocity>(s.world.entity(2).unwrap()).unwrap().lin;
    assert!((v[0] - 8.0).abs() < 0.01, "{v:?}");
    // let go
    let mut released = 0.0f64;
    goggles(&mut s, |h| h.release = true);
    for _ in 0..3 {
        s.step();
        released = released.max(res(&s, "event.released"));
    }
    let (h, k) = (hacker(&s), hackable(&s));
    assert_eq!((h.state, h.controlled.0), (0, 0));
    assert!(!k.hacked && k.controller.0 == 0);
    assert_eq!(team(&s, 2), 2, "back on its own side");
    let p = *s.world.get::<Perception>(s.world.entity(2).unwrap()).unwrap();
    assert!(p.active && p.engage, "its AI works again");
    assert_eq!(rig_target(&s), 1, "the camera is home");
    assert!(!s.world.get::<Weapon>(s.world.entity(2).unwrap()).unwrap().fire);
    assert!(released >= 1.0, "released was announced");
}

#[test]
fn a_character_body_machine_walks_and_the_control_has_a_time_limit() {
    let mut s = world("", "");
    // a walker instead of a flyer
    let e = s.world.entity(2).unwrap();
    s.world.despawn(e);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,1,-10]},"Team":{"id":2},"Perception":{"engage":true},"CharacterBody":{},"Hackable":{"difficulty":1,"range":20,"duration":1.0,"speed":3}}"#);
    aim_and_hack(&mut s);
    s.step_n(80);
    assert_eq!(hacker(&s).state, 2);
    goggles(&mut s, |h| h.move_dir = [0.0, 0.0, 1.0]);
    s.step_n(5);
    assert_eq!(s.world.get::<CharacterBody>(s.world.entity(2).unwrap()).unwrap().walk, [0.0, 3.0], "a walker is told to walk");
    // one second of control, then the link is lost and the machine is its own again
    s.step_n(80);
    let h = hacker(&s);
    assert_eq!(h.state, 0, "the link ran out: {h:?}");
    assert_eq!(team(&s, 2), 2);
}

#[test]
fn a_destroyed_machine_or_a_dead_hacker_ends_the_control_and_keep_leaves_it_on_your_side() {
    let take = |machine_extra: &str, hackable: &str| {
        let mut s = world(machine_extra, hackable);
        aim_and_hack(&mut s);
        s.step_n(130);
        assert_eq!(hacker(&s).state, 2);
        s
    };
    let mut s = take(r#""HitPoints":{"value":50,"max":50}"#, "");
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<crate::components::HitPoints>(e).unwrap().value = 0.0;
    s.step_n(4);
    assert_eq!(hacker(&s).state, 0, "the machine was destroyed: the link is lost");
    // the hacker dies
    let mut s = take("", "");
    spawn(&mut s, 8, r#"{"Transform":{"pos":[0,0,0]}}"#);
    let e = s.world.entity(1).unwrap();
    s.world.insert(e, crate::components::HitPoints { value: 0.0, max: 100.0, ..Default::default() }).unwrap();
    s.step_n(4);
    assert_eq!(hacker(&s).state, 0, "a dead hacker holds nothing");
    assert_eq!(team(&s, 2), 2);
    // keep: it stays on the hacker's team and its AI runs again
    let mut s = take("", r#""keep":true"#);
    goggles(&mut s, |h| h.release = true);
    s.step_n(3);
    assert_eq!(team(&s, 2), 1, "kept");
    assert!(s.world.get::<Perception>(s.world.entity(2).unwrap()).unwrap().active);
}

#[test]
fn a_hack_that_is_dropped_falls_back_and_hackers_do_not_double_take() {
    let mut s = world("", "");
    aim_and_hack(&mut s);
    s.step_n(60);
    assert!((hackable(&s).progress - 0.5).abs() < 0.04);
    goggles(&mut s, |h| h.hack = false);
    s.step_n(60);
    let p = hackable(&s).progress;
    assert!((p - 0.25).abs() < 0.05, "half a second of hacking is lost each second: {p}");
    assert_eq!(hacker(&s).state, 0);
    s.step_n(200);
    assert_eq!(hackable(&s).progress, 0.0);
}

// ------------------------------------------------------------------------------------------------ aiming

#[test]
fn a_hacker_that_aims_picks_the_machine_it_looks_at_and_hacks_it() {
    let mut s = world("", "");
    // a second machine off to the right, 10 m away
    spawn(&mut s, 3, r#"{"Transform":{"pos":[8,1,-6]},"Team":{"id":2},"Velocity":{},"Hackable":{"difficulty":2,"range":20}}"#);
    // the goggles are at 1.6 m, the machine 2 at 1 m, 10 m ahead: looking down -z with a little downward tilt
    goggles(&mut s, |h| {
        h.aim = true;
        h.hack = true;
        h.look = [0.0, -0.06, -1.0];
    });
    s.step();
    assert_eq!(hacker(&s).target.0, 2, "{:?}", hacker(&s));
    assert!(hackable(&s).progress > 0.0, "the hack fills");
    // looking at the second one picks it, and the first one's progress falls back
    goggles(&mut s, |h| h.look = [8.0, -0.6, -6.0]);
    s.step();
    assert_eq!(hacker(&s).target.0, 3);
    // looking at nothing: no target and no hack
    goggles(&mut s, |h| h.look = [0.0, 1.0, 0.0]);
    s.step();
    assert_eq!(hacker(&s).target.0, 0);
    assert_eq!(hacker(&s).state, 0);
}

#[test]
fn aiming_takes_the_machine_after_its_difficulty_and_never_looks_past_the_cone_or_the_range() {
    let mut s = world("", "");
    goggles(&mut s, |h| {
        h.aim = true;
        h.hack = true;
        h.look = [0.0, -0.06, -1.0];
    });
    for _ in 0..140 {
        s.step();
    }
    assert_eq!(hacker(&s).state, 2, "taken after its 2 seconds: {:?}", hacker(&s));
    assert_eq!(hacker(&s).controlled.0, 2);
    // a machine just outside the cone (looking 6 degrees to the side of it) is not picked
    let mut s = world("", "");
    goggles(&mut s, |h| {
        h.aim = true;
        h.hack = true;
        h.aim_cone = 4.0;
        h.look = [1.05, -0.06, -10.0]; // about 6 degrees off
    });
    s.step();
    assert_eq!(hacker(&s).target.0, 0, "outside the cone");
    goggles(&mut s, |h| h.aim_cone = 8.0);
    s.step();
    assert_eq!(hacker(&s).target.0, 2, "inside a wider cone");
    // out of the goggles' range
    let mut s = world("", "");
    goggles(&mut s, |h| {
        h.aim = true;
        h.hack = true;
        h.range = 5.0;
        h.look = [0.0, -0.06, -1.0];
    });
    s.step();
    assert_eq!(hacker(&s).target.0, 0, "10 m away is beyond a range of 5");
}

#[test]
fn without_aim_the_game_chooses_the_target_as_before() {
    let mut s = world("", "");
    goggles(&mut s, |h| {
        h.hack = true;
        h.target = lunaris_ecs::Uid(2);
        h.look = [0.0, 1.0, 0.0]; // (not used: no aim)
    });
    s.step();
    assert_eq!(hacker(&s).target.0, 2);
    assert!(hackable(&s).progress > 0.0);
}
