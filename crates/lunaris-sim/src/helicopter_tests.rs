//! Helicopters: lift and hover, climbing and sinking, tilting to fly, turning, rotor speed, noise the AI hears, heat on the pilot, crashes, spinning parts. No art: boxes and numbers.
use super::*;
use crate::components::{Helicopter, HitPoints, Noisy, Perception, Transform, Velocity};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// 60 ticks a second, the ground a big box with its top at y = 0.
fn world() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[2000,1,2000]}}"#);
    s
}

/// A helicopter at height `y` with its rotors already at speed (unless `extra` says otherwise).
fn heli(s: &mut Sim, uid: u64, y: f32, extra: &str) {
    let spool = if extra.contains("\"spool\"") { "" } else { r#""spool":1"# };
    let sep = if extra.is_empty() || spool.is_empty() { "" } else { "," };
    spawn(
        s,
        uid,
        &format!(
            r#"{{"Transform":{{"pos":[0,{y},0]}},"Collider":{{"shape":1,"size":[2,2,6]}},"RigidBody":{{"mass":2000}},"Velocity":{{}},"AngularVelocity":{{"damping":0}},"HitPoints":{{"value":1000,"max":1000,"last":1000}},"Helicopter":{{{spool}{sep}{extra}}}}}"#
        ),
    );
}

fn h(s: &Sim, uid: u64) -> Helicopter {
    *s.world.get::<Helicopter>(s.world.entity(uid).unwrap()).unwrap()
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn vel(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Velocity>(s.world.entity(uid).unwrap()).unwrap().lin
}

fn hp(s: &Sim, uid: u64) -> f32 {
    s.world.get::<HitPoints>(s.world.entity(uid).unwrap()).unwrap().value
}

fn stick(s: &mut Sim, uid: u64, f: impl Fn(&mut Helicopter)) {
    let e = s.world.entity(uid).unwrap();
    f(s.world.get_mut::<Helicopter>(e).unwrap());
}

fn events(s: &Sim, name: &str) -> usize {
    s.events.iter().filter(|e| e.name == name).count()
}

#[test]
fn with_the_stick_centred_it_hovers() {
    let mut s = world();
    heli(&mut s, 1, 50.0, "");
    s.step_n(300);
    let p = pos(&s, 1);
    assert!((p[1] - 50.0).abs() < 1.0, "five seconds later it is where it was: {p:?}");
    assert!(h(&s, 1).speed < 1.0, "{}", h(&s, 1).speed);
    // (it is 50 m up: the altitude is the distance to the ground under its floor)
    assert!((h(&s, 1).altitude - 49.0).abs() < 1.5, "{}", h(&s, 1).altitude);
}

#[test]
fn the_collective_climbs_and_sinks() {
    let mut s = world();
    heli(&mut s, 1, 50.0, "");
    stick(&mut s, 1, |x| x.collective = 1.0);
    s.step_n(180);
    assert!(pos(&s, 1)[1] > 60.0, "full collective climbs: {:?}", pos(&s, 1));
    assert!(vel(&s, 1)[1] > 4.0 && vel(&s, 1)[1] < 14.0, "to a climb rate that the drag allows: {:?}", vel(&s, 1));
    stick(&mut s, 1, |x| x.collective = -1.0);
    let top = pos(&s, 1)[1];
    // (it coasts up a little first, then falls towards the speed the air allows)
    s.step_n(300);
    assert!(pos(&s, 1)[1] < top - 20.0, "no collective sinks: {top} -> {:?}", pos(&s, 1));
}

#[test]
fn tipping_the_nose_flies_it_forward_without_losing_height_and_levelling_stops_it() {
    let mut s = world();
    heli(&mut s, 1, 80.0, "");
    stick(&mut s, 1, |x| x.pitch = 1.0);
    s.step_n(300);
    let t = h(&s, 1).tilt;
    assert!((t[0] - 30.0).abs() < 2.0, "nose down by the maximum tilt: {t:?}");
    assert!(t[1].abs() < 2.0, "not rolled: {t:?}");
    let v = vel(&s, 1);
    assert!(v[2] < -12.0 && v[0].abs() < 1.0, "forward is -z: {v:?}");
    assert!((pos(&s, 1)[1] - 80.0).abs() < 4.0, "the assist keeps it up: {:?}", pos(&s, 1));
    stick(&mut s, 1, |x| x.pitch = 0.0);
    s.step_n(1200);
    assert!(h(&s, 1).tilt[0].abs() < 2.0, "stick back: level again: {:?}", h(&s, 1).tilt);
    assert!(h(&s, 1).speed < 3.0, "and the drag has stopped it: {}", h(&s, 1).speed);
    // rolling right goes right (+x) with the right side down
    let mut s = world();
    heli(&mut s, 1, 80.0, "");
    stick(&mut s, 1, |x| x.roll = 1.0);
    s.step_n(240);
    assert!((h(&s, 1).tilt[1] - 30.0).abs() < 2.0, "{:?}", h(&s, 1).tilt);
    assert!(vel(&s, 1)[0] > 8.0, "{:?}", vel(&s, 1));
}

#[test]
fn the_pedal_turns_it_and_the_heading_holds_when_it_is_released() {
    let mut s = world();
    heli(&mut s, 1, 80.0, "");
    stick(&mut s, 1, |x| x.yaw = 1.0);
    s.step_n(60);
    let after_a_second = h(&s, 1).heading;
    assert!((after_a_second + 80.0).abs() < 8.0, "right is the way the compass goes: -80 degrees a second: {after_a_second}");
    stick(&mut s, 1, |x| x.yaw = 0.0);
    s.step_n(30);
    let a = h(&s, 1).heading;
    s.step_n(120);
    assert!((h(&s, 1).heading - a).abs() < 1.5, "it does not drift: {a} -> {}", h(&s, 1).heading);
    // and left the other way
    let mut s = world();
    heli(&mut s, 1, 80.0, "");
    stick(&mut s, 1, |x| x.yaw = -1.0);
    s.step_n(60);
    assert!((h(&s, 1).heading - 80.0).abs() < 8.0, "{}", h(&s, 1).heading);
}

#[test]
fn the_rotors_spool_up_and_down_and_the_engine_off_brings_it_down() {
    let mut s = world();
    heli(&mut s, 1, 100.0, r#""spool":0,"engine":true"#);
    // `heli` puts spool 1 first: the later key wins, so it starts from 0
    assert_eq!(h(&s, 1).spool, 0.0);
    s.step_n(180);
    let half = h(&s, 1).spool;
    assert!((half - 0.5).abs() < 0.03, "three seconds of the six: {half}");
    s.step_n(240);
    assert_eq!(h(&s, 1).spool, 1.0);
    stick(&mut s, 1, |x| x.engine = false);
    s.step_n(300);
    assert!((h(&s, 1).spool - 0.5).abs() < 0.03, "five of ten seconds: {}", h(&s, 1).spool);
    s.step_n(330);
    assert_eq!(h(&s, 1).spool, 0.0);
    assert!(pos(&s, 1)[1] < 100.0, "and it came down");
}

#[test]
fn the_rotors_are_loud_and_the_machines_hear_them_from_far() {
    let radius = |s: &Sim| s.world.get::<Noisy>(s.world.entity(1).unwrap()).unwrap().radius;
    let mut s = world();
    heli(&mut s, 1, 60.0, "");
    stick(&mut s, 1, |x| x.collective = 1.0);
    s.step_n(5);
    assert!((radius(&s) - 160.0).abs() < 0.5, "full power at full speed is the noise: {}", radius(&s));
    stick(&mut s, 1, |x| x.collective = 0.0);
    s.step();
    assert!((radius(&s) - 160.0 * (0.35 + 0.65 * 0.5)).abs() < 0.5, "a hover is quieter: {}", radius(&s));
    stick(&mut s, 1, |x| x.collective = -1.0);
    s.step();
    assert!((radius(&s) - 160.0 * 0.35).abs() < 0.5, "at idle it is the idle share: {}", radius(&s));
    // an AI 130 m away, deaf to sight, hears it at full power and not with the engine off
    let mut s = world();
    heli(&mut s, 1, 60.0, r#""collective":1"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[130,0,0]},"Team":{"id":2},"Perception":{"sight_range":1,"hearing":1}}"#);
    s.step_n(5);
    let p = *s.world.get::<Perception>(s.world.entity(3).unwrap()).unwrap();
    assert!(p.heard, "{p:?}");
    let mut s = world();
    heli(&mut s, 1, 60.0, r#""engine":false,"spool":0"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[130,0,0]},"Team":{"id":2},"Perception":{"sight_range":1,"hearing":1}}"#);
    s.step_n(5);
    assert!(!s.world.get::<Perception>(s.world.entity(3).unwrap()).unwrap().heard, "a silent helicopter is not heard");
}

#[test]
fn flying_one_heats_up_the_pilot() {
    let mut s = world();
    spawn(&mut s, 5, r#"{"Transform":{},"Heat":{"decay":0}}"#);
    heli(&mut s, 1, 60.0, r#""collective":1,"heat":2,"pilot":5"#);
    s.step_n(60);
    let heat = s.world.get::<crate::components::Heat>(s.world.entity(5).unwrap()).unwrap().value;
    assert!((heat - 2.0).abs() < 0.1, "2 a second at full noise, for a second: {heat}");
    // with the engine off there is none
    let mut s = world();
    spawn(&mut s, 5, r#"{"Transform":{},"Heat":{"decay":0}}"#);
    heli(&mut s, 1, 60.0, r#""engine":false,"spool":0,"heat":2,"pilot":5"#);
    s.step_n(60);
    assert_eq!(s.world.get::<crate::components::Heat>(s.world.entity(5).unwrap()).unwrap().value, 0.0);
}

#[test]
fn a_fall_is_a_crash_that_hurts_and_a_landing_is_not() {
    let mut s = world();
    heli(&mut s, 1, 40.0, r#""engine":false,"spool":0"#);
    s.step_n(240);
    assert!(events(&s, "heli_crash") >= 1, "it hit the ground fast");
    assert!(hp(&s, 1) < 900.0, "it was hurt: {}", hp(&s, 1));
    // a gentle landing: a hover 1.2 m up, collective down a little
    let mut s = world();
    heli(&mut s, 1, 2.2, "");
    stick(&mut s, 1, |x| x.collective = -0.4);
    s.step_n(300);
    assert_eq!(events(&s, "heli_crash"), 0, "it settled on the ground");
    assert_eq!(hp(&s, 1), 1000.0);
    assert!(pos(&s, 1)[1] < 1.3, "{:?}", pos(&s, 1));
}

#[test]
fn a_destroyed_helicopter_loses_its_engine() {
    let mut s = world();
    heli(&mut s, 1, 100.0, "");
    s.step_n(5);
    let e = s.world.entity(1).unwrap();
    s.world.get_mut::<HitPoints>(e).unwrap().value = 0.0;
    s.step_n(700);
    assert_eq!(h(&s, 1).spool, 0.0, "the rotors stopped");
    assert!(pos(&s, 1)[1] < 100.0);
}

#[test]
fn the_rotor_parts_turn_with_the_rotors() {
    let mut s = world();
    spawn(&mut s, 7, r#"{"Transform":{"pos":[0,61,0]},"Parent":{"target":1},"LocalTransform":{"pos":[0,1,0]}}"#);
    heli(&mut s, 1, 60.0, r#""rotor":7,"rotor_rps":1"#);
    // 15 ticks at one turn a second: a quarter turn about the part's y axis
    s.step_n(15);
    let e = s.world.entity(7).unwrap();
    let q = s.world.get::<crate::components::LocalTransform>(e).unwrap().rot;
    let half = std::f32::consts::FRAC_1_SQRT_2;
    assert!(q[0].abs() < 1.0e-3 && (q[1] - half).abs() < 0.02 && q[2].abs() < 1.0e-3 && (q[3] - half).abs() < 0.02, "{q:?}");
    assert!((h(&s, 1).rotor_angle - std::f32::consts::FRAC_PI_2).abs() < 0.05, "{}", h(&s, 1).rotor_angle);
}

#[test]
fn a_helicopter_without_a_body_is_left_alone_and_the_same_flight_gives_the_same_hash() {
    let mut s = world();
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,50,0]},"Helicopter":{"spool":1}}"#);
    s.step_n(10);
    assert_eq!(h(&s, 1).altitude, 0.0, "nothing was done to it");
    let run = || {
        let mut s = world();
        heli(&mut s, 1, 80.0, "");
        stick(&mut s, 1, |x| {
            x.pitch = 0.7;
            x.roll = -0.3;
            x.yaw = 0.5;
            x.collective = 0.2;
        });
        s.step_n(400);
        s.world.state_hash()
    };
    assert_eq!(run(), run());
}
