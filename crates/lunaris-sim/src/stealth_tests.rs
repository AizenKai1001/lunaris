//! Stealth: the noise a character makes by how it moves, how hard it is to see, and what an AI makes of both. No art: numbers.
use super::*;
use crate::components::{Noisy, Perception, Stealth};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// A character at (x, z) that moves along x at `speed` m/s (60 ticks a second), with a stealth, a body and a team.
fn runner(s: &mut Sim, uid: u64, x: f32, z: f32, speed: f32, extra: &str) {
    spawn(
        s,
        uid,
        &format!(r#"{{"Transform":{{"pos":[{x},0.9,{z}]}},"Velocity":{{"lin":[{speed},0,0]}},"Stealth":{{{extra}}},"Collider":{{"shape":2,"size":[0.7,1.8,0.7]}},"HitPoints":{{"value":100,"max":100}},"Team":{{"id":1}}}}"#),
    );
}

fn st(s: &Sim, uid: u64) -> Stealth {
    *s.world.get::<Stealth>(s.world.entity(uid).unwrap()).unwrap()
}

fn noisy(s: &Sim, uid: u64) -> f32 {
    s.world.get::<Noisy>(s.world.entity(uid).unwrap()).unwrap().radius
}

#[test]
fn the_noise_follows_how_fast_it_moves_and_crouching_quietens_it() {
    let at = |speed: f32, extra: &str| {
        let mut s = Sim::new(1);
        runner(&mut s, 1, 0.0, 0.0, speed, extra);
        s.step_n(90);
        (st(&s, 1), noisy(&s, 1))
    };
    let (still, n) = at(0.0, "");
    assert!(still.speed < 0.05 && n == 0.0, "{still:?}");
    let (w, n) = at(2.0, "");
    assert!((w.speed - 2.0).abs() < 0.2 && (n - 5.0).abs() < 0.8, "a walk is 5 m: {n} at {}", w.speed);
    let (_, n) = at(5.0, "");
    assert!((n - 14.0).abs() < 1.5, "a run is 14 m: {n}");
    let (_, n) = at(8.0, "");
    assert!((n - 22.0).abs() < 2.0, "a sprint is 22 m: {n}");
    let (_, n) = at(12.0, "");
    assert!(n <= 22.5, "no louder beyond the sprint: {n}");
    let (_, n) = at(5.0, r#""crouch":true"#);
    assert!((n - 14.0 * 0.3).abs() < 0.8, "crouched it is a third of that: {n}");
}

#[test]
fn it_is_hard_to_see_when_crouched_still_or_in_the_dark() {
    let vis = |speed: f32, extra: &str| {
        let mut s = Sim::new(1);
        runner(&mut s, 1, 0.0, 0.0, speed, extra);
        s.step_n(90);
        st(&s, 1).visibility
    };
    assert!((vis(5.0, "") - 1.0).abs() < 0.05, "running in the light: seen from the whole range");
    assert!((vis(0.0, "") - 0.7).abs() < 0.02, "standing still: 70 percent");
    assert!((vis(0.0, r#""crouch":true"#) - 0.42).abs() < 0.02, "crouched and still: 0.6 times 0.7");
    assert!((vis(5.0, r#""light":0"#) - 0.4).abs() < 0.03, "in the dark: 40 percent");
    assert!(vis(0.0, r#""crouch":true,"light":0"#) < 0.2, "all of them together");
}

#[test]
fn what_is_hard_to_see_is_seen_from_nearer() {
    let seen = |dist: f32, extra: &str| {
        let mut s = Sim::new(1);
        spawn(&mut s, 7, r#"{"Transform":{"pos":[0,0,0]},"Team":{"id":2},"Perception":{"sight_range":40,"sight_fov":360,"sight_eye":0.9,"los":false,"notice_time":0.2}}"#);
        runner(&mut s, 1, 0.0, -dist, 0.0, extra);
        s.step_n(90);
        s.world.get::<Perception>(s.world.entity(7).unwrap()).unwrap().visible
    };
    // standing still: 40 * 0.7 = 28 m; crouched and still: 40 * 0.42 = 16.8 m
    assert!(seen(25.0, ""), "standing still is seen at 25 m");
    assert!(!seen(31.0, ""), "but not at 31 m");
    assert!(!seen(20.0, r#""crouch":true"#), "crouched, it is not seen at 20 m");
    assert!(seen(14.0, r#""crouch":true"#), "but at 14 m it is");
    assert!(seen(1.2, r#""crouch":true,"light":0"#), "within arm's length nothing hides it");
    // a character with no Stealth is seen from the whole range
    let mut s = Sim::new(1);
    spawn(&mut s, 7, r#"{"Transform":{"pos":[0,0,0]},"Team":{"id":2},"Perception":{"sight_range":40,"sight_fov":360,"sight_eye":0.9,"los":false}}"#);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0.9,-38]},"Collider":{"shape":2,"size":[0.7,1.8,0.7]},"HitPoints":{"value":100,"max":100},"Team":{"id":1}}"#);
    s.step_n(60);
    assert!(s.world.get::<Perception>(s.world.entity(7).unwrap()).unwrap().visible);
}

#[test]
fn a_running_player_is_heard_further_than_a_walking_one() {
    let heard = |speed: f32, dist: f32| {
        let mut s = Sim::new(1);
        // deaf to sight (a narrow, short look the other way), sharp enough ears
        spawn(&mut s, 7, r#"{"Transform":{"pos":[0,0,0]},"Team":{"id":2},"Perception":{"sight_range":1,"sight_fov":10,"los":false,"hearing":1}}"#);
        runner(&mut s, 1, 0.0, dist, speed, "");
        let mut heard = false;
        for _ in 0..90 {
            s.step();
            heard |= s.world.get::<Perception>(s.world.entity(7).unwrap()).unwrap().heard;
        }
        heard
    };
    assert!(heard(5.0, 10.0), "a run (14 m) is heard at 10 m");
    assert!(!heard(2.0, 10.0), "a walk (5 m) is not");
}

#[test]
fn stealth_is_deterministic() {
    let run = || {
        let mut s = Sim::new(1);
        runner(&mut s, 1, 0.0, 0.0, 4.0, r#""crouch":true"#);
        spawn(&mut s, 7, r#"{"Transform":{"pos":[0,0,0]},"Team":{"id":2},"Perception":{"sight_range":40,"sight_fov":360,"los":false}}"#);
        s.step_n(120);
        s.world.state_hash()
    };
    assert_eq!(run(), run());
}
