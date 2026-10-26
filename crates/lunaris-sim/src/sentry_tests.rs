//! A turret that is turned by the goggles instead of taken (`Hackable.mode` 1, docs/SENTRY.md): it changes sides for its time with its own AI running, spares its hacker, and goes back. No art: boxes, capsules and bare components.
use super::*;
use crate::components::{CameraRig, Hackable, Hacker, HitPoints, Perception, Team, Weapon};
use lunaris_ecs::Uid;
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn res(s: &Sim, name: &str) -> f64 {
    s.world.resource(name).unwrap_or(0.0)
}

/// uid 5: a camera on the hacker; uid 1: the hacker (team `hacker_team`, a big body, the goggles 8 m in front of the turret); uid 2: the turret (team 2, an engaged Perception that sees 30 m all around, a gun, `Hackable` mode 1 and
/// `duration` seconds); uid 3: a machine of the old side (team 2, 1000 hit points) 8 m to the side; uid 7 watches the events.
fn world(hacker_team: u32, duration: f32) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 7, r#"{"EventWatch":{"names":"hacked, link_lost"}}"#);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,3,5]},"CameraRig":{"target":1}}"#);
    spawn(
        &mut s,
        1,
        &format!(r#"{{"Transform":{{"pos":[0,0,8]}},"Team":{{"id":{hacker_team}}},"HitPoints":{{"value":100000,"max":100000}},"CharacterBody":{{"radius":0.35,"height":1.8,"gravity_scale":0}},"Hacker":{{"camera":5,"target":2,"hack":true}}}}"#),
    );
    spawn(
        &mut s,
        2,
        &format!(
            r#"{{"Transform":{{"pos":[0,1,0]}},"Team":{{"id":2}},"HitPoints":{{"value":500,"max":500}},"Collider":{{"shape":1,"size":[0.9,1.5,0.9]}},"Perception":{{"engage":true,"chase":false,"sight_range":30,"sight_fov":360,"attack_range":30,"los":false,"sight_eye":0,"notice_time":0.3,"hearing":0}},"Weapon":{{"damage":10,"rate":600,"spread":0,"infinite":true,"owner":2,"noise":0,"recoil":[0,0]}},"Hackable":{{"difficulty":2,"range":20,"duration":{duration},"mode":1}}}}"#
        ),
    );
    spawn(
        &mut s,
        3,
        r#"{"Transform":{"pos":[8,0,-2]},"Team":{"id":2},"HitPoints":{"value":1000,"max":1000},"CharacterBody":{"radius":0.35,"height":1.8,"gravity_scale":0}}"#,
    );
    s
}

fn hp(s: &Sim, uid: u64) -> f32 {
    s.world.get::<HitPoints>(s.world.entity(uid).unwrap()).unwrap().value
}
fn team(s: &Sim, uid: u64) -> u32 {
    s.world.get::<Team>(s.world.entity(uid).unwrap()).unwrap().id
}
fn hacker(s: &Sim) -> Hacker {
    *s.world.get::<Hacker>(s.world.entity(1).unwrap()).unwrap()
}
fn turret(s: &Sim) -> Hackable {
    *s.world.get::<Hackable>(s.world.entity(2).unwrap()).unwrap()
}
fn perception(s: &Sim) -> Perception {
    *s.world.get::<Perception>(s.world.entity(2).unwrap()).unwrap()
}
fn rig_target(s: &Sim) -> u64 {
    s.world.get::<CameraRig>(s.world.entity(5).unwrap()).unwrap().target.0
}

/// Step until the turret is turned (at most 4 s); the event `hacked` must have been announced on the way.
fn until_turned(s: &mut Sim) {
    let mut seen = 0.0f64;
    for _ in 0..240 {
        s.step();
        seen = seen.max(res(s, "event.hacked"));
        if turret(s).hacked {
            // (the event reaches the resource a tick later)
            s.step();
            seen = seen.max(res(s, "event.hacked"));
            assert!(seen >= 1.0, "hacked was announced");
            return;
        }
    }
    panic!("the turret was not turned: {:?} {:?}", hacker(s), turret(s));
}

#[test]
fn a_turret_is_turned_not_taken() {
    let mut s = world(1, 30.0);
    // before the hack it is hostile to the hacker (team 1): it shoots it
    s.step_n(30);
    assert!(hp(&s, 1) < 100000.0, "a turret of the machines shoots a player");
    until_turned(&mut s);
    let (h, k, p) = (hacker(&s), turret(&s), perception(&s));
    assert_eq!((h.state, h.controlled.0), (0, 0), "the hacker is free: {h:?}");
    assert_eq!(rig_target(&s), 1, "the camera stays on the hacker");
    assert!(k.hacked && k.controller.0 == 1 && k.saved_team == 2 && k.time_left > 29.0, "{k:?}");
    assert_eq!(team(&s, 2), 1, "it is on the hacker's side");
    assert!(p.active && p.engage, "its own AI keeps working: {p:?}");
    assert!(h.blocked, "the hack key that is still held does not start another hack");
    // (a hacker that holds the key does not hack a turret that is turned: nothing fills)
    s.step_n(60);
    assert_eq!((hacker(&s).state, hacker(&s).progress), (0, 0.0));
}

#[test]
fn a_turned_turret_shoots_the_old_side_and_spares_a_hacker_who_has_no_team() {
    let mut s = world(0, 30.0);
    until_turned(&mut s);
    assert_eq!(team(&s, 2), 0, "a free player has no team: the turret is hostile to everybody that is not its hacker");
    let (hacker_hp, victim_hp) = (hp(&s, 1), hp(&s, 3));
    s.step_n(180);
    assert_eq!(hp(&s, 1), hacker_hp, "it never shoots the one who turned it");
    assert!(hp(&s, 3) < victim_hp - 50.0, "the machine of the old side takes the fire: {} -> {}", victim_hp, hp(&s, 3));
    assert_eq!(perception(&s).target.0, 3);
    // and the turret itself is a target of the machines now (it is on another team than they are)
    assert!(turret(&s).hacked);
}

#[test]
fn a_turned_turret_goes_back_when_its_time_runs_out() {
    let mut s = world(1, 3.0);
    until_turned(&mut s);
    s.step_n(120);
    let k = turret(&s);
    assert!(k.hacked && k.time_left < 1.2 && k.time_left > 0.5, "the time runs down: {k:?}");
    let mut lost = 0.0f64;
    for _ in 0..90 {
        s.step();
        lost = lost.max(res(&s, "event.link_lost"));
    }
    let k = turret(&s);
    assert!(!k.hacked && k.controller.0 == 0 && k.progress == 0.0 && k.time_left == 0.0, "{k:?}");
    assert_eq!(team(&s, 2), 2, "back on its own side");
    assert!(lost >= 1.0, "link_lost was announced");
    // and it shoots the player again
    let before = hp(&s, 1);
    s.step_n(60);
    assert!(hp(&s, 1) < before, "back against the players");
    // it can be hacked again
    goggles_hack(&mut s, false);
    s.step_n(5);
    goggles_hack(&mut s, true);
    until_turned(&mut s);
    assert_eq!(team(&s, 2), 1);
}

fn goggles_hack(s: &mut Sim, on: bool) {
    let e = s.world.entity(1).unwrap();
    let h = s.world.get_mut::<Hacker>(e).unwrap();
    h.hack = on;
    h.target = Uid(2);
}

#[test]
fn a_destroyed_turret_goes_back_but_a_dead_hacker_does_not_end_it() {
    let mut s = world(1, 30.0);
    until_turned(&mut s);
    // the hacker dies: the defence goes on
    let e = s.world.entity(1).unwrap();
    s.world.get_mut::<HitPoints>(e).unwrap().value = 0.0;
    s.step_n(30);
    assert!(turret(&s).hacked && team(&s, 2) == 1, "a dead hacker does not end it");
    // the turret is destroyed: it is given back (the machines' team, for the wreck)
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<HitPoints>(e).unwrap().value = 0.0;
    s.step_n(3);
    assert!(!turret(&s).hacked);
    assert_eq!(team(&s, 2), 2);
}

#[test]
fn a_machine_in_the_old_mode_is_still_taken_and_driven() {
    // (mode 0 is the engine's old behaviour: the hacker holds the machine; docs/AI.md)
    let mut s = world(1, 30.0);
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<Hackable>(e).unwrap().mode = 0;
    for _ in 0..240 {
        s.step();
        if hacker(&s).state == 2 {
            break;
        }
    }
    assert_eq!((hacker(&s).state, hacker(&s).controlled.0), (2, 2));
    assert_eq!(rig_target(&s), 2);
    assert!(!perception(&s).active);
    let w = s.world.get::<Weapon>(s.world.entity(2).unwrap()).unwrap();
    assert!(!w.fire);
}
