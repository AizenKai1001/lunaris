//! Sleeping (`physics/sleep.rs`): a group of bodies that lies still goes to sleep and stays exactly where it is; it wakes, with everything that touches
//! it, when something hits it, moves it, or takes away or puts down what is around it. Every scenario is run with the fast physics path and with the
//! reference step side by side: the state hash (sleeping included) must be the same on every tick.
use super::*;
use crate::components::{Transform, Velocity};
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

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

fn set(s: &mut Sim, uid: u64, component: &str, value: &str) {
    s.apply(&Command::Set { uid, component: component.into(), value: j(value) }).unwrap();
}

/// A big floor (a collider with a `RigidBody` that never moves) whose top is at y = 0.
const FLOOR: &str = r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[200,1,200]},"RigidBody":{"friction":0.8,"restitution":0}}"#;

fn cube(s: &mut Sim, uid: u64, p: [f32; 3], turning: bool) {
    let spin = if turning { r#","AngularVelocity":{}"# } else { "" };
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":{p:?}}},"Velocity":{{}}{spin},"Collider":{{"shape":1,"size":[1,1,1]}},"RigidBody":{{"mass":1,"restitution":0,"friction":0.6}}}}"#));
}

fn sleepy(s: &mut Sim) {
    spawn(s, 900, r#"{"PhysicsSleep":{"time":0.3}}"#);
}

/// A stack of `n` cubes at x = `x`, uids from `first`.
fn stack(s: &mut Sim, first: u64, x: f32, n: u64, turning: bool) {
    for k in 0..n {
        cube(s, first + k, [x, 0.5 + k as f32 * 1.001, 0.0], turning);
    }
}

/// The same world twice, the second stepped by the reference.
fn pair(build: impl Fn(&mut Sim)) -> (Sim, Sim) {
    let (mut a, mut b) = (Sim::new(1), Sim::new(1));
    b.reference_physics = true;
    build(&mut a);
    build(&mut b);
    (a, b)
}

/// Steps both `n` ticks; on every tick the contacts and the state must be the same.
fn step_both(a: &mut Sim, b: &mut Sim, n: u32) {
    for t in 0..n {
        a.step();
        b.step();
        assert_eq!(a.contacts, b.contacts, "contacts differ at tick {}", a.tick);
        assert_eq!(a.state_hash(), b.state_hash(), "state differs at tick {} (step {t} of this run)", a.tick);
    }
}

fn all_asleep(s: &Sim, uids: impl IntoIterator<Item = u64>) -> bool {
    uids.into_iter().all(|u| s.sleep.is_asleep(u))
}

#[test]
fn a_settled_stack_falls_asleep_and_stays_exactly_where_it_is() {
    let (mut a, mut b) = pair(|s| {
        spawn(s, 1, FLOOR);
        sleepy(s);
        stack(s, 10, 0.0, 3, false);
    });
    step_both(&mut a, &mut b, 120);
    assert!(all_asleep(&a, 10..13), "the stack should be asleep after two seconds: {} sleepers", a.sleep.sleeper_count());
    let before: Vec<_> = (10..13).map(|u| pos(&a, u)).collect();
    step_both(&mut a, &mut b, 90);
    for (k, u) in (10..13).enumerate() {
        assert_eq!(pos(&a, u), before[k], "a sleeper does not move at all");
        assert_eq!(vel(&a, u), [0.0; 3]);
    }
    assert!(all_asleep(&a, 10..13));
}

#[test]
fn a_world_without_the_component_never_sleeps() {
    let (mut a, mut b) = pair(|s| {
        spawn(s, 1, FLOOR);
        stack(s, 10, 0.0, 3, false);
    });
    step_both(&mut a, &mut b, 200);
    assert!(a.sleep.is_empty() && a.sleep.sleeper_count() == 0);
}

#[test]
fn a_ball_that_hits_a_sleeping_stack_wakes_it_and_pushes_it() {
    let (mut a, mut b) = pair(|s| {
        spawn(s, 1, FLOOR);
        sleepy(s);
        stack(s, 10, 0.0, 3, false);
    });
    step_both(&mut a, &mut b, 120);
    assert!(all_asleep(&a, 10..13));
    let before = pos(&a, 10);
    let shot = r#"{"Transform":{"pos":[-6,0.5,0]},"Velocity":{"lin":[9,0,0]},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":3,"restitution":0,"friction":0.2}}"#;
    spawn(&mut a, 50, shot);
    spawn(&mut b, 50, shot);
    step_both(&mut a, &mut b, 50);
    // everything that was in the island woke together
    assert!((10..13).all(|u| !a.sleep.is_asleep(u)), "the stack woke when the ball came: {} sleepers", a.sleep.sleeper_count());
    step_both(&mut a, &mut b, 30);
    let now = pos(&a, 10);
    assert!(now[0] - before[0] > 0.05, "the bottom cube is pushed away: {before:?} to {now:?}");
    // and they lie down again in the end
    step_both(&mut a, &mut b, 400);
    assert!(all_asleep(&a, 10..13), "the pushed stack sleeps again: {} sleepers", a.sleep.sleeper_count());
}

#[test]
fn taking_the_floor_away_wakes_what_rests_on_it() {
    let (mut a, mut b) = pair(|s| {
        // (a deep ground, and a slab above it that the cube rests on)
        spawn(s, 1, r#"{"Transform":{"pos":[0,-20.5,0]},"Collider":{"shape":1,"size":[200,1,200]}}"#);
        spawn(s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[4,1,4]}}"#);
        sleepy(s);
        cube(s, 10, [0.0, 0.5, 0.0], false);
    });
    step_both(&mut a, &mut b, 90);
    assert!(a.sleep.is_asleep(10));
    for s in [&mut a, &mut b] {
        s.apply(&Command::Despawn { uid: 2 }).unwrap();
    }
    step_both(&mut a, &mut b, 60);
    assert!(!a.sleep.is_asleep(10), "the cube woke when its floor went");
    assert!(pos(&a, 10)[1] < -3.0, "and fell: {:?}", pos(&a, 10));
    step_both(&mut a, &mut b, 240);
    assert!(a.sleep.is_asleep(10), "and lies still on the ground below");
    assert!((pos(&a, 10)[1] - -19.5).abs() < 0.05, "{:?}", pos(&a, 10));
}

#[test]
fn a_collider_that_appears_next_to_a_sleeper_wakes_it_and_one_far_away_does_not() {
    let (mut a, mut b) = pair(|s| {
        spawn(s, 1, FLOOR);
        sleepy(s);
        stack(s, 10, 0.0, 2, false);
    });
    step_both(&mut a, &mut b, 100);
    assert!(all_asleep(&a, 10..12));
    // far away: the scenery changed, but nothing near the sleepers did
    let wall = |x: f32| format!(r#"{{"Transform":{{"pos":[{x},1,0]}},"Collider":{{"shape":1,"size":[1,2,3]}}}}"#);
    for s in [&mut a, &mut b] {
        spawn(s, 60, &wall(50.0));
    }
    step_both(&mut a, &mut b, 5);
    assert!(all_asleep(&a, 10..12), "a collider far away wakes nobody");
    // right next to the cubes
    for s in [&mut a, &mut b] {
        spawn(s, 61, &wall(1.0));
    }
    step_both(&mut a, &mut b, 2);
    assert!(!a.sleep.is_asleep(10) && !a.sleep.is_asleep(11), "a collider that touches them wakes them (both, as one island)");
    step_both(&mut a, &mut b, 120);
    assert!((pos(&a, 10)[0]).abs() < 0.01 && (pos(&a, 11)[0]).abs() < 0.01, "and they were not disturbed: {:?}", pos(&a, 10));
}

#[test]
fn moving_or_pushing_a_sleeper_wakes_it() {
    let (mut a, mut b) = pair(|s| {
        spawn(s, 1, FLOOR);
        sleepy(s);
        cube(s, 10, [0.0, 0.5, 0.0], false);
        cube(s, 11, [10.0, 0.5, 0.0], false);
    });
    step_both(&mut a, &mut b, 100);
    assert!(all_asleep(&a, [10, 11]));
    // a script gives one a push, and lifts the other into the air
    for s in [&mut a, &mut b] {
        set(s, 10, "Velocity", r#"{"lin":[3,0,0]}"#);
        set(s, 11, "Transform", r#"{"pos":[10,6,0]}"#);
    }
    step_both(&mut a, &mut b, 20);
    assert!(pos(&a, 10)[0] > 0.5, "the pushed cube moved: {:?}", pos(&a, 10));
    assert!(pos(&a, 11)[1] < 5.5, "the lifted cube fell: {:?}", pos(&a, 11));
    step_both(&mut a, &mut b, 300);
    assert!(all_asleep(&a, [10, 11]), "both lie down again");
}

#[test]
fn separate_piles_wake_separately() {
    let (mut a, mut b) = pair(|s| {
        spawn(s, 1, FLOOR);
        sleepy(s);
        stack(s, 10, 0.0, 2, false);
        stack(s, 20, 30.0, 2, false);
    });
    step_both(&mut a, &mut b, 100);
    assert!(all_asleep(&a, [10, 11, 20, 21]));
    let shot = r#"{"Transform":{"pos":[-6,0.5,0]},"Velocity":{"lin":[9,0,0]},"Collider":{"shape":0,"size":[1,1,1]},"RigidBody":{"mass":3,"restitution":0,"friction":0.2}}"#;
    spawn(&mut a, 50, shot);
    spawn(&mut b, 50, shot);
    step_both(&mut a, &mut b, 50);
    assert!(!a.sleep.is_asleep(10) && !a.sleep.is_asleep(11), "the hit pile is awake");
    assert!(a.sleep.is_asleep(20) && a.sleep.is_asleep(21), "the other pile sleeps on");
}

/// The turning boxes go through the reference step (a world with an `AngularVelocity` does): a tower of them sleeps as one and wakes as one.
#[test]
fn a_tower_of_turning_boxes_sleeps_as_one_and_wakes_as_one_without_a_wobble() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, FLOOR);
    sleepy(&mut s);
    stack(&mut s, 10, 0.0, 5, true);
    s.step_n(180);
    assert!(all_asleep(&s, 10..15), "{} of 5 asleep", s.sleep.sleeper_count());
    let before: Vec<_> = (10..15).map(|u| pos(&s, u)).collect();
    // a script nudges the top box: the whole tower wakes
    set(&mut s, 14, "Velocity", r#"{"lin":[0.05,0,0]}"#);
    s.step_n(2);
    assert!((10..15).all(|u| !s.sleep.is_asleep(u)), "the whole island woke");
    s.step_n(120);
    for (k, u) in (10..15).enumerate() {
        let p = pos(&s, u);
        assert!((p[0] - before[k][0]).abs() < 0.03 && (p[1] - before[k][1]).abs() < 0.02 && (p[2] - before[k][2]).abs() < 0.03, "box {u} moved from {:?} to {p:?}", before[k]);
    }
    s.step_n(240);
    assert!(all_asleep(&s, 10..15), "the tower sleeps again");
}

#[test]
fn sleeping_is_state_it_is_cloned_checkpointed_hashed_recorded_and_replayed() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, FLOOR);
    sleepy(&mut s);
    stack(&mut s, 10, 0.0, 4, false);
    stack(&mut s, 20, 10.0, 3, false);
    s.step_n(120);
    assert!(all_asleep(&s, [10, 11, 12, 13, 20, 21, 22]));
    // it is part of the hash
    let mut without = s.clone();
    without.sleep.clear();
    assert_ne!(without.state_hash(), s.state_hash());
    // the json of a recording round-trips every bit
    let back = crate::physics::SleepState::from_json(&s.sleep.to_json()).unwrap();
    assert_eq!(back, s.sleep);
    // a checkpoint brings the sleepers back, and the run from it is the same run
    let c = s.checkpoint();
    let h0 = s.state_hash();
    let fork = s.clone();
    let poke = |s: &mut Sim| {
        set(s, 12, "Velocity", r#"{"lin":[0.2,0,0]}"#);
        s.step_n(60);
    };
    set(&mut s, 12, "Velocity", r#"{"lin":[0.2,0,0]}"#);
    s.step_n(2);
    assert!(!s.sleep.is_asleep(10), "the poke woke the first stack");
    assert!(s.sleep.is_asleep(20), "and not the second");
    s.step_n(58);
    let h1 = s.state_hash();
    s.rewind_to(&c).unwrap();
    assert_eq!(s.state_hash(), h0);
    assert_eq!(s.sleep.sleeper_count(), 7);
    poke(&mut s);
    assert_eq!(s.state_hash(), h1, "the same history again after a rewind");
    let mut fork = fork;
    poke(&mut fork);
    assert_eq!(fork.state_hash(), h1, "a clone keeps in step");
    // a recording that starts while things sleep replays exactly, also through json
    let mut r = Sim::new(1);
    spawn(&mut r, 1, FLOOR);
    sleepy(&mut r);
    stack(&mut r, 10, 0.0, 3, false);
    r.step_n(100);
    assert!(all_asleep(&r, 10..13));
    r.start_recording().unwrap();
    r.step_n(10);
    set(&mut r, 11, "Velocity", r#"{"lin":[0.3,0,0]}"#);
    r.step_n(60);
    let rec = r.stop_recording().unwrap();
    assert!(!rec.start_sleep.is_empty());
    assert_eq!(Sim::replay(&r, &rec).unwrap().state_hash(), rec.final_hash);
    let text = rec.to_json().to_string_compact().unwrap();
    let again = crate::sim::Recording::from_json(&parse(&text).unwrap()).unwrap();
    assert_eq!(again.start_sleep, rec.start_sleep);
    assert_eq!(Sim::replay(&r, &again).unwrap().state_hash(), rec.final_hash);
}

#[test]
fn a_trigger_still_sees_a_sleeper_and_a_changed_collider_or_turn_wakes_it() {
    let (mut a, mut b) = pair(|s| {
        spawn(s, 1, FLOOR);
        sleepy(s);
        spawn(s, 70, r#"{"Transform":{"pos":[0,0.5,0]},"Collider":{"shape":1,"size":[3,3,3],"trigger":true}}"#);
        cube(s, 10, [0.0, 0.5, 0.0], false);
        cube(s, 20, [10.0, 0.5, 0.0], false);
        cube(s, 30, [20.0, 0.5, 0.0], false);
    });
    step_both(&mut a, &mut b, 100);
    assert!(all_asleep(&a, [10, 20, 30]));
    assert!(a.contacts.iter().any(|c| c.trigger && (c.a, c.b) == (10, 70)), "the trigger goes on seeing the sleeper: {:?}", a.contacts);
    // a script makes one cube bigger and turns another: each wakes, and nothing else does
    for s in [&mut a, &mut b] {
        set(s, 20, "Collider", r#"{"size":[1.2,1.2,1.2]}"#);
        set(s, 30, "Transform", r#"{"rot":[0,0.0998,0,0.995]}"#);
    }
    step_both(&mut a, &mut b, 2);
    assert!(a.sleep.is_asleep(10) && !a.sleep.is_asleep(20) && !a.sleep.is_asleep(30), "{} sleepers", a.sleep.sleeper_count());
    step_both(&mut a, &mut b, 200);
    assert!(all_asleep(&a, [10, 20, 30]), "and they lie down again");
}

#[test]
fn contact_memory_helpers_split_and_merge_in_uid_order() {
    use crate::contact_memory::{ContactMemory, Point};
    let pt = |jn: f32| Point { local: [0.0; 3], n: [0.0, 1.0, 0.0], jn, f: [0.0; 3] };
    let mut a = ContactMemory::default();
    for (x, y) in [(1u64, 2u64), (1, 5), (4, 5)] {
        a.begin_pair(x, y);
        a.push_point(pt(x as f32 + y as f32));
    }
    let (rest, taken) = a.partition(|x, _| x == 1);
    assert_eq!((rest.pair_count(), taken.pair_count()), (1, 2));
    assert!(rest.find(4, 5).is_some() && taken.find(1, 5).is_some() && taken.find(4, 5).is_none());
    assert_eq!(rest.merged(&taken), a, "the two halves merge back into the whole, in order");
    assert_eq!(taken.merged(&rest), a);
    // a pair in both comes from the first
    let mut other = ContactMemory::default();
    other.begin_pair(1, 2);
    other.push_point(pt(99.0));
    assert_eq!(a.merged(&other).find(1, 2).unwrap()[0].jn, 3.0);
    assert_eq!(other.merged(&a).find(1, 2).unwrap()[0].jn, 99.0);
}
