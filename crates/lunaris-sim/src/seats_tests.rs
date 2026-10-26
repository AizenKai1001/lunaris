//! Seats: sitting down by the use button, being carried with the vehicle, getting out, the pilot's seat flying a helicopter from the sitter's controls, a helicopter too high to leave, a sitter who dies. No art: boxes.
use super::*;
use crate::components::{Collider, Controls, Helicopter, HitPoints, Interactable, Interactor, Seat, Seated, Transform, Vehicle, Velocity};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// A character at (x, y, 0) looking down -z, with a use button and hit points.
fn who(s: &mut Sim, uid: u64, x: f32, y: f32) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{x},{y},0]}},"Velocity":{{}},"Interactor":{{"reach":3}},"HitPoints":{{"value":100,"max":100}},"Controls":{{}}}}"#));
}

/// A vehicle: a one metre box at (0, y, -2) with a seat and a way in. `seat` is more fields of the `Seat`, `more` more components.
fn car(s: &mut Sim, uid: u64, y: f32, seat: &str, more: &str) {
    let offset = if seat.contains("\"offset\"") { "" } else { r#""offset":[0,0.2,0.3]"# };
    let sep = if seat.is_empty() || offset.is_empty() { "" } else { "," };
    let sep2 = if more.is_empty() { "" } else { "," };
    spawn(
        s,
        uid,
        &format!(r#"{{"Transform":{{"pos":[0,{y},-2]}},"Collider":{{"shape":1,"size":[1,1,1]}},"Interactable":{{"kind":0,"prompt":"Enter"}},"Seat":{{{offset}{sep}{seat}}}{sep2}{more}}}"#),
    );
}

fn button(s: &mut Sim, uid: u64, on: bool) {
    let e = s.world.entity(uid).unwrap();
    s.world.get_mut::<Interactor>(e).unwrap().act = on;
}

/// Press the use button of a character for a tick and let it go for a tick.
fn press(s: &mut Sim, uid: u64) {
    button(s, uid, true);
    s.step();
    button(s, uid, false);
    s.step();
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn seat(s: &Sim, uid: u64) -> Seat {
    *s.world.get::<Seat>(s.world.entity(uid).unwrap()).unwrap()
}

fn seated(s: &Sim, uid: u64) -> bool {
    s.world.get::<Seated>(s.world.entity(uid).unwrap()).is_some()
}

fn events(s: &Sim, name: &str) -> usize {
    s.events.iter().filter(|e| e.name == name).count()
}

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1.0e-3)
}

#[test]
fn a_character_that_uses_a_seat_sits_in_it_and_goes_where_the_vehicle_goes() {
    let mut s = Sim::new(1);
    who(&mut s, 1, 0.0, 1.6);
    car(&mut s, 5, 1.6, "", r#""Velocity":{"lin":[0,0,-2]}"#);
    press(&mut s, 1);
    assert!(seated(&s, 1), "the character sat down");
    assert_eq!(seat(&s, 5).occupant.0, 1);
    assert_eq!(events(&s, "seat_enter"), 1);
    for _ in 0..60 {
        s.step();
    }
    let (v, c) = (pos(&s, 5), pos(&s, 1));
    assert!(v[2] < -3.5, "the vehicle drove on: {v:?}");
    assert!(near(c, [v[0], v[1] + 0.2, v[2] + 0.3]), "and the sitter is at the seat's place in it: {c:?} against {v:?}");
    let e = s.world.entity(1).unwrap();
    assert!((s.world.get::<Velocity>(e).unwrap().lin[2] + 2.0).abs() < 1.0e-3, "with the vehicle's speed");
}

#[test]
fn an_occupied_seat_is_not_for_a_second_character_and_does_not_offer_itself() {
    let mut s = Sim::new(1);
    who(&mut s, 1, 0.0, 1.6);
    who(&mut s, 2, 0.3, 1.6);
    car(&mut s, 5, 1.6, "", "");
    press(&mut s, 1);
    assert!(s.world.get::<Interactable>(s.world.entity(5).unwrap()).is_some_and(|i| !i.enabled), "the prompt goes while somebody sits");
    press(&mut s, 2);
    assert!(!seated(&s, 2) && seated(&s, 1), "the second character stays out");
    assert_eq!(seat(&s, 5).occupant.0, 1);
}

#[test]
fn the_use_button_pressed_again_gets_the_sitter_out_and_the_press_that_sat_it_down_does_not() {
    let mut s = Sim::new(1);
    who(&mut s, 1, 0.0, 1.6);
    car(&mut s, 5, 1.6, r#""exit":[2,-1,0]"#, "");
    // the button is held down for a long time after it sits down: still sitting
    button(&mut s, 1, true);
    for _ in 0..30 {
        s.step();
    }
    assert!(seated(&s, 1), "a held button that sat it down does not get it out");
    // let go, press again: out, at the exit
    button(&mut s, 1, false);
    s.step();
    button(&mut s, 1, true);
    s.step();
    button(&mut s, 1, false);
    s.step();
    assert!(!seated(&s, 1), "the second press gets it out");
    assert!(near(pos(&s, 1), [2.0, 0.6, -2.0]), "at the exit, in the vehicle's axes: {:?}", pos(&s, 1));
    assert_eq!(seat(&s, 5).occupant.0, 0);
    assert_eq!(events(&s, "seat_exit"), 1);
    assert!(s.world.get::<Interactable>(s.world.entity(5).unwrap()).is_some_and(|i| i.enabled), "the seat is on offer again");
}

#[test]
fn the_seat_and_its_exit_turn_with_the_vehicle() {
    let mut s = Sim::new(1);
    who(&mut s, 1, 0.0, 1.6);
    car(&mut s, 5, 1.6, r#""offset":[0,0.2,1],"exit":[0,0,1]"#, "");
    // a quarter turn to the left about the vertical (a local back, +z, becomes world +x)
    let e = s.world.entity(5).unwrap();
    let h = std::f32::consts::FRAC_1_SQRT_2;
    s.world.get_mut::<Transform>(e).unwrap().rot = [0.0, h, 0.0, h];
    press(&mut s, 1);
    assert!(seated(&s, 1));
    assert!(near(pos(&s, 1), [1.0, 1.8, -2.0]), "{:?}", pos(&s, 1));
    press(&mut s, 1);
    assert!(!seated(&s, 1));
    assert!(near(pos(&s, 1), [1.0, 1.6, -2.0]), "{:?}", pos(&s, 1));
}

#[test]
fn the_pilots_seat_flies_the_helicopter_from_the_sitters_controls_and_leaving_stops_it() {
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[2000,1,2000]}}"#);
    who(&mut s, 1, 0.0, 1.6);
    car(&mut s, 5, 1.0, r#""drive":1"#, r#""RigidBody":{"mass":2000},"Velocity":{},"AngularVelocity":{"damping":0},"HitPoints":{"value":1000,"max":1000,"last":1000},"Helicopter":{"engine":false}"#);
    s.world.get_mut::<Collider>(s.world.entity(5).unwrap()).unwrap().size = [2.0, 2.0, 2.0];
    let e = s.world.entity(1).unwrap();
    press(&mut s, 1);
    assert!(seated(&s, 1));
    let h = *s.world.get::<Helicopter>(s.world.entity(5).unwrap()).unwrap();
    assert!(h.engine && h.pilot.0 == 1, "sitting in the pilot's seat starts the engine and makes the sitter the pilot: {h:?}");
    s.world.get_mut::<Controls>(e).unwrap().axes = [0.2, 0.7, -0.3, 0.5];
    s.step();
    let h = *s.world.get::<Helicopter>(s.world.entity(5).unwrap()).unwrap();
    assert_eq!((h.roll, h.collective, h.pitch, h.yaw), (0.2, 0.7, -0.3, 0.5), "the sitter's hands are the helicopter's stick");
    // values beyond the stick's travel are held to it
    s.world.get_mut::<Controls>(e).unwrap().axes = [9.0, -9.0, f32::NAN, 0.0];
    s.step();
    let h = *s.world.get::<Helicopter>(s.world.entity(5).unwrap()).unwrap();
    assert_eq!((h.roll, h.collective, h.pitch), (1.0, -1.0, 0.0));
    // on the ground: out. The stick goes back, the engine is shut, nobody pilots it
    s.world.get_mut::<Controls>(e).unwrap().axes = [0.0; 4];
    s.step();
    press(&mut s, 1);
    assert!(!seated(&s, 1), "{:?}", pos(&s, 1));
    let h = *s.world.get::<Helicopter>(s.world.entity(5).unwrap()).unwrap();
    assert!(!h.engine && h.pilot.0 == 0 && h.collective == 0.0 && h.roll == 0.0, "{h:?}");
}

#[test]
fn a_helicopter_in_the_air_does_not_let_its_pilot_out() {
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[2000,1,2000]}}"#);
    who(&mut s, 1, 0.0, 41.6);
    car(&mut s, 5, 41.6, r#""drive":1"#, r#""RigidBody":{"mass":2000},"Velocity":{},"AngularVelocity":{"damping":0},"HitPoints":{"value":1000,"max":1000,"last":1000},"Helicopter":{"spool":1}"#);
    s.world.get_mut::<Collider>(s.world.entity(5).unwrap()).unwrap().size = [2.0, 2.0, 2.0];
    press(&mut s, 1);
    assert!(seated(&s, 1));
    for _ in 0..10 {
        s.step();
    }
    press(&mut s, 1);
    assert!(seated(&s, 1), "40 m up: it stays in");
    assert!(events(&s, "seat_denied") >= 1, "and is told so");
    assert_eq!(seat(&s, 5).occupant.0, 1);
}

#[test]
fn a_sitter_who_dies_or_goes_leaves_the_seat_where_it_is() {
    let mut s = Sim::new(1);
    who(&mut s, 1, 0.0, 1.6);
    car(&mut s, 5, 1.6, r#""exit":[5,0,0]"#, "");
    press(&mut s, 1);
    assert!(seated(&s, 1));
    let e = s.world.entity(1).unwrap();
    s.world.get_mut::<HitPoints>(e).unwrap().value = 0.0;
    s.world.get_mut::<HitPoints>(e).unwrap().dead = true;
    s.step();
    assert!(!seated(&s, 1), "a dead sitter is let go");
    assert!(near(pos(&s, 1), [0.0, 1.8, -1.7]), "where it sat, not at the exit: {:?}", pos(&s, 1));
    assert_eq!(seat(&s, 5).occupant.0, 0);
    // another one sits (the seat's use has a cool-down of a third of a second), and is removed from the world
    who(&mut s, 3, 0.0, 1.6);
    for _ in 0..30 {
        s.step();
    }
    press(&mut s, 3);
    assert_eq!(seat(&s, 5).occupant.0, 3);
    let e3 = s.world.entity(3).unwrap();
    s.world.despawn(e3);
    s.step();
    assert_eq!(seat(&s, 5).occupant.0, 0, "a sitter that is gone leaves the seat free");
    assert_eq!(events(&s, "seat_exit"), 2);
}

#[test]
fn the_same_world_gives_the_same_state_hash() {
    let run = || {
        let mut s = Sim::new(1);
        who(&mut s, 1, 0.0, 1.6);
        who(&mut s, 2, 0.3, 1.6);
        car(&mut s, 5, 1.6, "", r#""Velocity":{"lin":[0,0,-1]}"#);
        press(&mut s, 1);
        press(&mut s, 2);
        for _ in 0..30 {
            s.step();
        }
        s.state_hash()
    };
    assert_eq!(run(), run());
}

// ------------------------------------------------------------------------------------------------ a car

const WHEELS: &str = "defaults: radius=0.35 rest=0.3 travel=0.2 spring=35000 damper=3200 grip=1.2\nfl: -0.8 -0.15 -1.3 steer=1 brake=1\nfr: 0.8 -0.15 -1.3 steer=1 brake=1\nrl: -0.8 -0.15 1.3 drive=1 brake=0.7 handbrake=1\nrr: 0.8 -0.15 1.3 drive=1 brake=0.7 handbrake=1";

/// A ground, a character at (0, 0.8, 0) looking down -z and a family car whose rear is a metre in front of it, with the driver's seat.
fn car_world() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[2000,1,2000]}}"#);
    who(&mut s, 1, 0.0, 0.8); // (its eyes are at the car's height: the car is under a metre tall)
    // (the wheels are a text with a line each: in JSON the line ends are written out)
    let wheels = WHEELS.replace('\n', "\\n");
    spawn(
        &mut s,
        5,
        &format!(
            r#"{{"Transform":{{"pos":[0,0.74,-3.0]}},"Collider":{{"shape":1,"size":[1.8,0.7,4.2]}},"RigidBody":{{"mass":1200,"friction":0.3}},"Velocity":{{}},"AngularVelocity":{{"damping":0.05}},
            "Vehicle":{{"wheels":"{wheels}"}},"Interactable":{{"kind":0,"prompt":"Drive"}},"Seat":{{"offset":[-0.4,0.45,0.2],"exit":[-2.2,-0.3,0],"drive":2}}}}"#
        ),
    );
    s
}

fn vehicle(s: &Sim) -> Vehicle {
    *s.world.get::<Vehicle>(s.world.entity(5).unwrap()).unwrap()
}

fn hands(s: &mut Sim, axes: [f32; 4]) {
    let e = s.world.entity(1).unwrap();
    s.world.get_mut::<Controls>(e).unwrap().axes = axes;
}

#[test]
fn the_drivers_seat_drives_the_vehicle_from_the_sitters_controls() {
    let mut s = car_world();
    for _ in 0..30 {
        s.step();
    }
    assert!(vehicle(&s).handbrake == 0.0 && vehicle(&s).speed.abs() < 0.1, "a parked car: {:?}", vehicle(&s).speed);
    press(&mut s, 1);
    assert!(seated(&s, 1));
    hands(&mut s, [0.0, 0.0, 1.0, 0.3]);
    for _ in 0..90 {
        s.step();
    }
    let v = vehicle(&s);
    assert_eq!((v.throttle, v.steer), (1.0, 0.3), "the sitter's hands are the pedals and the wheel");
    assert!(v.speed > 3.0, "and the car drives forward: {}", v.speed);
    assert!(pos(&s, 5)[2] < -4.5, "{:?}", pos(&s, 5));
    // the pedals: brake below 0, handbrake above a half, held to what a pedal can do
    hands(&mut s, [0.0, -1.0, 0.0, 0.0]);
    s.step();
    assert_eq!((vehicle(&s).brake, vehicle(&s).handbrake), (1.0, 0.0));
    hands(&mut s, [0.0, 1.0, 9.0, f32::NAN]);
    s.step();
    let v = vehicle(&s);
    assert_eq!((v.handbrake, v.throttle, v.steer), (1.0, 1.0, 0.0));
}

#[test]
fn a_moving_car_does_not_let_its_driver_out_and_a_stopped_one_does_and_is_left_parked() {
    let mut s = car_world();
    press(&mut s, 1);
    // the cool-down of the seat's use is over by the time the driver has been going a while
    hands(&mut s, [0.0, 0.0, 1.0, 0.0]);
    for _ in 0..120 {
        s.step();
    }
    assert!(vehicle(&s).speed > 6.0, "{}", vehicle(&s).speed);
    press(&mut s, 1);
    assert!(seated(&s, 1), "at speed it stays in");
    assert!(events(&s, "seat_denied") >= 1);
    // brake to a stop, then out
    hands(&mut s, [0.0, -1.0, 0.0, 0.0]);
    for _ in 0..240 {
        s.step();
    }
    assert!(vehicle(&s).speed.abs() < 1.0, "{}", vehicle(&s).speed);
    press(&mut s, 1);
    assert!(!seated(&s, 1), "stopped: out");
    let v = vehicle(&s);
    assert_eq!((v.throttle, v.brake, v.handbrake), (0.0, 0.0, 1.0), "the handbrake is on");
    let (c, car) = (pos(&s, 1), pos(&s, 5));
    assert!((c[0] - car[0]).abs() > 1.5 && (c[0] - car[0]).abs() < 3.0, "beside the car, on its left: {c:?} {car:?}");
}
