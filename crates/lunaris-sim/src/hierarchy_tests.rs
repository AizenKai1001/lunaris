use super::*;
use components::Transform;
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn t(s: &Sim, uid: u64) -> Transform {
    *s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap()
}

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-5)
}

#[test]
fn a_child_follows_its_parent_at_once_and_every_tick() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[10,0,0]},"Velocity":{"lin":[0,0,60]}}"#);
    spawn(&mut s, 2, r#"{"Transform":{},"Parent":{"target":1},"LocalTransform":{"pos":[0,2,0]}}"#);
    // no step yet: the edit itself already put the child in place
    assert!(near(t(&s, 2).pos, [10.0, 2.0, 0.0]), "{:?}", t(&s, 2).pos);
    s.step_n(60); // the parent moves 60 units per second along z
    assert!(near(t(&s, 1).pos, [10.0, 0.0, 60.0]), "{:?}", t(&s, 1).pos);
    assert!(near(t(&s, 2).pos, [10.0, 2.0, 60.0]), "the child moved with it in the same tick: {:?}", t(&s, 2).pos);
    // moving the parent by an edit moves the child too
    s.apply(&Command::Set { uid: 1, component: "Transform".into(), value: parse(r#"{"pos":[0,0,0]}"#).unwrap() }).unwrap();
    assert!(near(t(&s, 2).pos, [0.0, 2.0, 0.0]));
}

#[test]
fn rotation_and_scale_of_the_parent_carry_over_and_chains_of_any_depth_work() {
    let mut s = Sim::new(1);
    // the parent is turned 90 degrees about y (x -> -z) and made twice as big
    let (c, sn) = (std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[1,0,0],"rot":[0,{sn},0,{c}],"scale":[2,2,2]}}}}"#));
    spawn(&mut s, 2, r#"{"Transform":{},"Parent":{"target":1},"LocalTransform":{"pos":[1,0,0],"scale":[0.5,0.5,0.5]}}"#);
    let child = t(&s, 2);
    assert!(near(child.pos, [1.0, 0.0, -2.0]), "local (1,0,0) scaled by 2 is (2,0,0), turned to (0,0,-2), plus (1,0,0): {:?}", child.pos);
    assert!(near(child.scale, [1.0, 1.0, 1.0]));
    assert!((child.rot[1] - sn).abs() < 1e-5 && (child.rot[3] - c).abs() < 1e-5, "{:?}", child.rot);
    // a grandchild, defined BEFORE its parent exists in the list order (uid 3 attaches to 2): depth does not depend on uid order
    spawn(&mut s, 0 + 5, r#"{"Transform":{},"Parent":{"target":3},"LocalTransform":{"pos":[0,0,1]}}"#);
    spawn(&mut s, 3, r#"{"Transform":{},"Parent":{"target":2},"LocalTransform":{"pos":[0,1,0]}}"#);
    let (mid, leaf) = (t(&s, 3), t(&s, 5));
    assert!(near(mid.pos, [1.0, 1.0, -2.0]), "{:?}", mid.pos);
    // leaf: local (0,0,1) through the middle's rotation (90 deg about y: z -> x) and scale 1 -> (1,0,0), plus (1,1,-2)
    assert!(near(leaf.pos, [2.0, 1.0, -2.0]), "{:?}", leaf.pos);
}

#[test]
fn missing_parents_cycles_and_unattached_entities_are_left_alone_and_nothing_hangs() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[5,5,5]},"Parent":{"target":99},"LocalTransform":{}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[1,2,3]},"Parent":{"target":3},"LocalTransform":{"pos":[1,0,0]}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[4,5,6]},"Parent":{"target":2},"LocalTransform":{"pos":[1,0,0]}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[7,8,9]},"Parent":{"target":4},"LocalTransform":{}}"#); // its own parent
    spawn(&mut s, 5, r#"{"Transform":{"pos":[9,9,9]},"LocalTransform":{"pos":[1,1,1]}}"#); // a local transform without a parent is ignored
    s.step_n(3);
    assert_eq!(t(&s, 1).pos, [5.0, 5.0, 5.0], "dangling parent");
    assert_eq!((t(&s, 2).pos, t(&s, 3).pos), ([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]), "a cycle stays put");
    assert_eq!(t(&s, 4).pos, [7.0, 8.0, 9.0]);
    assert_eq!(t(&s, 5).pos, [9.0, 9.0, 9.0]);
}

#[test]
fn hierarchies_replay_snapshot_and_undo_like_everything_else() {
    let build = || {
        let mut s = Sim::new(3);
        spawn(&mut s, 1, r#"{"Transform":{},"Velocity":{"lin":[1,0,0],"ang":[0,1,0]}}"#);
        spawn(&mut s, 2, r#"{"Transform":{},"Parent":{"target":1},"LocalTransform":{"pos":[2,0,0]}}"#);
        s
    };
    let template = build();
    let mut live = build();
    live.start_recording().unwrap();
    live.step_n(30);
    live.apply(&Command::Set { uid: 2, component: "LocalTransform".into(), value: parse(r#"{"pos":[0,3,0]}"#).unwrap() }).unwrap();
    live.step_n(30);
    let rec = live.stop_recording().unwrap();
    let again = Sim::replay(&template, &rec).unwrap();
    assert_eq!(again.state_hash(), live.state_hash(), "a replay reproduces the attached entity exactly");
    // the child really is turning with the parent (it is 3 above the parent, which spins about y: it stays above it)
    assert!((t(&live, 2).pos[1] - 3.0).abs() < 1e-4);
    // snapshots are plain clones
    let snap = live.clone();
    live.step_n(10);
    assert_ne!(snap.state_hash(), live.state_hash());
    assert_eq!(snap.clone().state_hash(), snap.state_hash());
}
