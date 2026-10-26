//! Continuous collision for fast bodies (`rot::tunnel_fixes`): a ball, box or capsule that moves more than its own size in a tick must not pass
//! through a thin wall or floor that never moves. Each scenario is run with the fast physics path and with the reference step: both must stop the
//! body, and must give the same state.
use super::*;
use crate::components::{Transform, Velocity};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, json: &str) {
    let Json::Obj(fields) = parse(json).unwrap() else { panic!("not an object") };
    s.apply(&Command::Spawn { uid: Some(uid), components: fields }).unwrap();
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn vel(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Velocity>(s.world.entity(uid).unwrap()).unwrap().lin
}

/// Runs `build` in a world stepped by the fast path and in one stepped by the reference; `check` looks at each tick of each; the two must agree.
fn both(build: impl Fn(&mut Sim), ticks: u32, check: impl Fn(&Sim, u32)) {
    let mut hashes = Vec::new();
    for reference in [false, true] {
        let mut s = Sim::new(1);
        s.reference_physics = reference;
        build(&mut s);
        for t in 0..ticks {
            s.step();
            check(&s, t);
        }
        hashes.push(s.state_hash());
    }
    assert_eq!(hashes[0], hashes[1], "the fast path and the reference must agree");
}

/// A world with many boxes that never move (so that the scenery grid is in use) and a thin wall at x = 10.
fn thin_wall(s: &mut Sim, rotated: bool) {
    for k in 0..40u64 {
        spawn(s, 100 + k, &format!(r#"{{"Transform":{{"pos":[{},0.5,{}]}},"Collider":{{"shape":1,"size":[1,1,1]}}}}"#, -30.0 + k as f32 * 1.5, 40.0 + k as f32));
    }
    let tf = if rotated { r#"{"pos":[10,10,0],"rot":[0,0,0,1]}"# } else { r#"{"pos":[10,10,0]}"# };
    spawn(s, 3, &format!(r#"{{"Transform":{tf},"Collider":{{"shape":1,"size":[0.05,20,20]}},"RigidBody":{{"restitution":1,"friction":0}}}}"#));
}

fn shot(s: &mut Sim, collider: &str, speed: f32) {
    spawn(
        s,
        2,
        &format!(r#"{{"Transform":{{"pos":[0,10,0]}},"Velocity":{{"lin":[{speed},0,0]}},"Collider":{collider},"RigidBody":{{"mass":0.5,"restitution":0.8,"friction":0,"gravity_scale":0}}}}"#),
    );
}

#[test]
fn a_fast_ball_a_fast_box_and_a_fast_capsule_cannot_pass_through_a_thin_wall() {
    for (name, collider) in [("ball", r#"{"shape":0,"size":[0.2,0.2,0.2]}"#), ("box", r#"{"shape":1,"size":[0.3,0.3,0.3]}"#), ("capsule", r#"{"shape":2,"size":[0.2,0.8,0.2]}"#)] {
        both(
            |s| {
                thin_wall(s, false);
                shot(s, collider, 160.0);
            },
            30,
            |s, t| assert!(pos(s, 2)[0] < 10.0, "the {name} went through the wall at tick {t}: {:?}", pos(s, 2)),
        );
        // and it came back
        let mut s = Sim::new(1);
        thin_wall(&mut s, false);
        shot(&mut s, collider, 160.0);
        s.step_n(30);
        assert!(vel(&s, 2)[0] < -100.0, "the {name} bounced back: {:?}", vel(&s, 2));
    }
}

#[test]
fn a_ball_dropped_from_a_great_height_does_not_fall_through_a_thin_floor() {
    both(
        |s| {
            spawn(s, 1, r#"{"Transform":{"pos":[0,-0.025,0]},"Collider":{"shape":1,"size":[50,0.05,50]}}"#);
            for k in 0..30u64 {
                spawn(s, 100 + k, &format!(r#"{{"Transform":{{"pos":[{},0.5,30]}},"Collider":{{"shape":1,"size":[1,1,1]}}}}"#, k as f32 * 2.0));
            }
            spawn(s, 2, r#"{"Transform":{"pos":[0,120,0]},"Velocity":{},"Collider":{"shape":0,"size":[0.2,0.2,0.2]},"RigidBody":{"mass":1,"restitution":0.3}}"#);
        },
        600,
        |s, t| assert!(pos(s, 2)[1] > -0.2, "the ball fell through the floor at tick {t}: {:?}", pos(s, 2)),
    );
}

#[test]
fn a_turned_thin_wall_stops_a_fast_ball_too() {
    both(
        |s| {
            // a wall turned 45 degrees about the vertical axis
            let (sn, cs) = ((std::f32::consts::FRAC_PI_4 * 0.5).sin(), (std::f32::consts::FRAC_PI_4 * 0.5).cos());
            spawn(s, 3, &format!(r#"{{"Transform":{{"pos":[10,10,0],"rot":[0,{sn},0,{cs}]}},"Collider":{{"shape":1,"size":[0.05,20,40]}},"RigidBody":{{"restitution":1,"friction":0}}}}"#));
            shot(s, r#"{"shape":0,"size":[0.2,0.2,0.2]}"#, 160.0);
        },
        30,
        // (the ball is turned away to the side, not through: it must stay on this side of the plane of the wall, x + z = 10 here)
        |s, t| {
            let p = pos(s, 2);
            assert!(p[0] - p[2] < 10.05, "the ball went through the turned wall at tick {t}: {p:?}");
        },
    );
}

#[test]
fn collision_layers_and_triggers_are_still_passed_through() {
    // a wall on a layer that the ball does not collide with
    both(
        |s| {
            spawn(s, 3, r#"{"Transform":{"pos":[10,10,0]},"Collider":{"shape":1,"size":[0.05,20,20]},"CollisionLayer":{"layer":1,"mask":2}}"#);
            spawn(s, 2, r#"{"Transform":{"pos":[0,10,0]},"Velocity":{"lin":[150,0,0]},"Collider":{"shape":0,"size":[0.2,0.2,0.2]},"RigidBody":{"mass":0.5,"gravity_scale":0},"CollisionLayer":{"layer":2,"mask":4}}"#);
        },
        30,
        |_, _| {},
    );
    let mut s = Sim::new(1);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[10,10,0]},"Collider":{"shape":1,"size":[0.05,20,20]},"CollisionLayer":{"layer":1,"mask":2}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,10,0]},"Velocity":{"lin":[150,0,0]},"Collider":{"shape":0,"size":[0.2,0.2,0.2]},"RigidBody":{"mass":0.5,"gravity_scale":0},"CollisionLayer":{"layer":2,"mask":4}}"#);
    s.step_n(30);
    assert!(pos(&s, 2)[0] > 20.0, "it passes through a wall it does not collide with: {:?}", pos(&s, 2));
    // a trigger zone does not stop it
    let mut s = Sim::new(1);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[10,10,0]},"Collider":{"shape":1,"size":[0.05,20,20],"trigger":true}}"#);
    shot(&mut s, r#"{"shape":0,"size":[0.2,0.2,0.2]}"#, 150.0);
    s.step_n(30);
    assert!(pos(&s, 2)[0] > 20.0, "it passes through a trigger: {:?}", pos(&s, 2));
}

#[test]
fn a_slow_body_is_left_to_the_contacts() {
    // a ball rolling at 3 m/s into a wall: nothing for the continuous collision to do, and the result is the one of the reference
    both(
        |s| {
            thin_wall(s, false);
            spawn(s, 4, r#"{"Transform":{"pos":[9,10,0]},"Velocity":{"lin":[3,0,0]},"Collider":{"shape":0,"size":[0.5,0.5,0.5]},"RigidBody":{"mass":1,"restitution":0.5,"gravity_scale":0}}"#);
        },
        60,
        |s, t| assert!(pos(s, 4)[0] < 10.0, "tick {t}: {:?}", pos(s, 4)),
    );
}
