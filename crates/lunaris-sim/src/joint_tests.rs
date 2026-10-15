//! Joints: ropes, rods and springs between points and bodies.
use super::*;
use components::{Joint, Transform, Velocity};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn ball(s: &mut Sim, uid: u64, pos: [f32; 3], joint: &str) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{},{},{}]}},"Velocity":{{}},"Collider":{{"shape":0,"size":[1,1,1]}},"RigidBody":{{"mass":1,"restitution":0}},"Joint":{joint}}}"#, pos[0], pos[1], pos[2]));
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[test]
fn a_rod_swings_like_a_pendulum_at_a_constant_distance_from_its_anchor() {
    let mut s = Sim::new(1);
    ball(&mut s, 1, [3.0, 0.0, 0.0], r#"{"kind":1,"anchor":[0,0,0]}"#);
    // the length was not given: it is the distance at the start, written back
    s.step();
    assert!((s.world.get::<Joint>(s.world.entity(1).unwrap()).unwrap().length - 3.0).abs() < 1e-4);
    let mut lowest = 0.0f32;
    for _ in 0..360 {
        s.step();
        let p = pos(&s, 1);
        assert!((dist(p, [0.0; 3]) - 3.0).abs() < 0.08, "the rod holds: {p:?}");
        lowest = lowest.min(p[1]);
    }
    assert!(lowest < -2.8, "it swung down to the bottom of the arc: {lowest}");
    // never above where it started (no energy appears): a little slack for the correction
    assert!(pos(&s, 1)[1] < 0.35);
}

#[test]
fn a_rope_is_slack_while_close_and_stops_a_fall_when_taut() {
    let mut s = Sim::new(1);
    ball(&mut s, 1, [0.0, -1.0, 0.0], r#"{"kind":0,"length":4,"anchor":[0,0,0]}"#);
    s.step_n(20);
    // 0.33 s of free fall: the same as without a rope (it is 1 m from the anchor of a 4 m rope)
    let free = {
        let mut f = Sim::new(1);
        spawn(&mut f, 1, r#"{"Transform":{"pos":[0,-1,0]},"Velocity":{},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":1}}"#);
        f.step_n(20);
        pos(&f, 1)
    };
    assert!((pos(&s, 1)[1] - free[1]).abs() < 1e-5, "{:?} {free:?}", pos(&s, 1));
    s.step_n(240);
    let p = pos(&s, 1);
    assert!(dist(p, [0.0; 3]) < 4.1 && p[1] < -3.5, "it hangs at the end of the rope: {p:?}");
}

#[test]
fn a_spring_settles_where_its_pull_balances_the_weight() {
    let mut s = Sim::new(1);
    // 1 kg on a spring of rest length 2 and stiffness 100 N/m hangs 9.8/100 m below that
    ball(&mut s, 1, [0.0, 7.0, 0.0], r#"{"kind":2,"length":2,"stiffness":100,"damping":6,"anchor":[0,10,0]}"#);
    s.step_n(900);
    let p = pos(&s, 1);
    assert!((p[1] - (10.0 - 2.0 - 0.098)).abs() < 0.01, "{p:?}");
    assert!(s.world.get::<Velocity>(s.world.entity(1).unwrap()).unwrap().lin[1].abs() < 0.01);
}

#[test]
fn a_stiff_spring_does_not_explode() {
    let mut s = Sim::new(1);
    ball(&mut s, 1, [0.0, 6.0, 0.0], r#"{"kind":2,"length":1,"stiffness":1000000,"damping":50,"anchor":[0,10,0]}"#);
    for _ in 0..300 {
        s.step();
        let p = pos(&s, 1);
        assert!(p.iter().all(|v| v.is_finite()) && p[1] > 5.0 && p[1] < 10.0, "{p:?}");
    }
}

#[test]
fn a_chain_of_rods_between_bodies_keeps_every_link_and_a_tied_pair_keeps_apart() {
    let mut s = Sim::new(1);
    // an anchor entity (only a Transform: a fixed point) and four balls hanging in a row from it
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,10,0]}}"#);
    ball(&mut s, 11, [2.0, 10.0, 0.0], r#"{"kind":1,"target":10,"length":2}"#);
    for k in 1..4u64 {
        ball(&mut s, 11 + k, [2.0 + 2.0 * k as f32, 10.0, 0.0], &format!(r#"{{"kind":1,"target":{},"length":2}}"#, 10 + k));
    }
    for _ in 0..300 {
        s.step();
        let mut prev = [0.0, 10.0, 0.0];
        for k in 0..4u64 {
            let p = pos(&s, 11 + k);
            assert!((dist(p, prev) - 2.0).abs() < 0.35, "link {k} stretched: {p:?} from {prev:?}");
            prev = p;
        }
    }
    // two free balls tied by a rod, one thrown sideways: they keep 4 m apart and drift together
    let mut t = Sim::new(1);
    spawn(&mut t, 1, r#"{"Transform":{"pos":[0,0,0]},"Velocity":{"lin":[3,0,0]},"Collider":{"shape":0},"RigidBody":{"mass":1,"gravity_scale":0},"Joint":{"kind":1,"target":2,"length":4}}"#);
    spawn(&mut t, 2, r#"{"Transform":{"pos":[0,0,4]},"Velocity":{},"Collider":{"shape":0},"RigidBody":{"mass":1,"gravity_scale":0}}"#);
    t.step_n(240);
    assert!((dist(pos(&t, 1), pos(&t, 2)) - 4.0).abs() < 0.1, "{:?} {:?}", pos(&t, 1), pos(&t, 2));
    assert!(pos(&t, 2)[0] > 1.0, "the far end was dragged along: {:?}", pos(&t, 2));
}

#[test]
fn bad_joints_are_ignored_and_the_result_is_deterministic() {
    let run = || {
        let mut s = Sim::new(1);
        // owner without a body part, a target that does not exist, itself as target, a nan anchor
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,5,0]},"Joint":{"kind":1,"length":2,"anchor":[0,0,0]}}"#);
        ball(&mut s, 2, [0.0, 5.0, 0.0], r#"{"kind":1,"target":999,"length":2}"#);
        ball(&mut s, 3, [4.0, 5.0, 0.0], r#"{"kind":1,"target":3,"length":2}"#);
        ball(&mut s, 4, [8.0, 5.0, 0.0], r#"{"kind":0,"length":2,"anchor":[0,0,0]}"#);
        s.step_n(120);
        s
    };
    let (a, b) = (run(), run());
    assert_eq!(a.state_hash(), b.state_hash());
    assert!(pos(&a, 3)[1] < 0.0, "a joint to itself does nothing: {:?}", pos(&a, 3));
    assert!(pos(&a, 2)[1] < 0.0 && pos(&a, 1)[1] == 5.0);
}
