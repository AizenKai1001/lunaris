//! Inverse kinematics: reaching with a chain of joints, looking at a point, and the entity-level `Ik` goals in the picture.
use super::*;
use crate::math::*;
use crate::rig::{IkGoal, IkKind, PoseLayer};
use lunaris_json::{parse, Json};
use lunaris_sim::{Command, Sim};

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The column model: four joints one above the other, 0.8 apart (base, lower, middle, top).
fn column() -> (std::sync::Arc<crate::mesh::Mesh>, TextureCache) {
    let mut c = TextureCache::new(Some(repo_root()));
    (c.model("examples/models/swayer.glb").expect("the model loads"), c)
}

fn rest_layers() -> Vec<PoseLayer> {
    vec![PoseLayer { clip: None, time: 0.0, looping: false, weight: 1.0, root: None, inertia: None }]
}

/// Where joint `j` (resting at `rest`) is after posing with `m`.
fn at(m: &[M4], j: usize, rest: V3) -> V3 {
    m[j].transform_point(rest)
}

const REST: [V3; 4] = [[0.0, 0.0, 0.0], [0.0, 0.8, 0.0], [0.0, 1.6, 0.0], [0.0, 2.4, 0.0]];

#[test]
fn a_chain_of_two_bones_reaches_its_target_keeping_its_bone_lengths() {
    let (model, _c) = column();
    let rig = model.rig.as_ref().unwrap();
    let top = rig.joint_index("top").unwrap();
    let goal = |target: V3, weight: f32, pole: Option<V3>| IkGoal { joint: top, kind: IkKind::Reach, target, pin: None, pole, weight };
    // the tip (joint `top`) is asked to go to (1, 1.5, 0): reachable (1.2 from the lower joint, whose bones are 0.8 + 0.8)
    let m = rig.skin_matrices_ik(&rest_layers(), &[goal([1.0, 1.5, 0.0], 1.0, None)]);
    let (pa, pb, pc) = (at(&m, 1, REST[1]), at(&m, 2, REST[2]), at(&m, 3, REST[3]));
    let near = |a: V3, b: V3, e: f32| (0..3).all(|k| (a[k] - b[k]).abs() < e);
    assert!(near(pc, [1.0, 1.5, 0.0], 2e-3), "the tip arrives: {pc:?}");
    assert!((length(sub(pb, pa)) - 0.8).abs() < 2e-3 && (length(sub(pc, pb)) - 0.8).abs() < 2e-3, "bones keep their length: {} {}", length(sub(pb, pa)), length(sub(pc, pb)));
    assert!(near(pa, REST[1], 1e-4), "the root of the chain stays put");
    assert!(near(at(&m, 0, REST[0]), REST[0], 1e-6), "joints below the chain do not move");
    // the same goal again gives the same pose, exactly
    let again = rig.skin_matrices_ik(&rest_layers(), &[goal([1.0, 1.5, 0.0], 1.0, None)]);
    assert_eq!(m.iter().map(|x| x.0).collect::<Vec<_>>(), again.iter().map(|x| x.0).collect::<Vec<_>>());
    // too far: the chain stretches out towards the target and stops at its full length
    let far = rig.skin_matrices_ik(&rest_layers(), &[goal([10.0, 0.8, 0.0], 1.0, None)]);
    let (fa, fc) = (at(&far, 1, REST[1]), at(&far, 3, REST[3]));
    assert!((length(sub(fc, fa)) - 1.6).abs() < 3e-3, "straight, 1.6 long: {}", length(sub(fc, fa)));
    assert!(fc[0] > 1.5 && (fc[1] - 0.8).abs() < 0.05, "pointing at the target: {fc:?}");
    // half weight: halfway between the animation (straight up) and the goal
    let half = rig.skin_matrices_ik(&rest_layers(), &[goal([1.0, 1.5, 0.0], 0.5, None)]);
    let hc = at(&half, 3, REST[3]);
    assert!(hc[0] > 0.15 && hc[0] < 0.9, "somewhere between: {hc:?}");
    // weight 0 and bad targets change nothing
    for bad in [goal([1.0, 1.5, 0.0], 0.0, None), goal([f32::NAN, 0.0, 0.0], 1.0, None)] {
        let m0 = rig.skin_matrices_ik(&rest_layers(), &[bad]);
        assert!(near(at(&m0, 3, REST[3]), REST[3], 1e-5));
    }
    // the pole decides which way the middle joint bows out: towards +z or towards -z
    let bow = |pole: V3| at(&rig.skin_matrices_ik(&rest_layers(), &[goal([0.0, 2.0, 0.0], 1.0, Some(pole))]), 2, REST[2]);
    assert!(bow([0.0, 1.5, 5.0])[2] > 0.1 && bow([0.0, 1.5, -5.0])[2] < -0.1, "{:?} {:?}", bow([0.0, 1.5, 5.0]), bow([0.0, 1.5, -5.0]));
}

#[test]
fn a_joint_looks_at_a_point_but_not_further_round_than_it_may() {
    let (model, _c) = column();
    let rig = model.rig.as_ref().unwrap();
    let top = rig.joint_index("top").unwrap();
    // the axis along the bone is +y; asked to face +x (90 degrees from where it points), with room to turn and then with a limit of 30
    let facing = |max_angle: f32| {
        let g = IkGoal { joint: top, kind: IkKind::Look { axis: [0.0, 1.0, 0.0], max_angle }, target: [5.0, 2.4, 0.0], pin: None, pole: None, weight: 1.0 };
        let m = rig.skin_matrices_ik(&rest_layers(), &[g]);
        normalize(sub(at(&m, 3, [0.0, 3.4, 0.0]), at(&m, 3, REST[3])))
    };
    let free = facing(180.0);
    assert!(free[0] > 0.999, "faces +x: {free:?}");
    let limited = facing(30.0);
    assert!((limited[1] - 0.866).abs() < 5e-3 && (limited[0] - 0.5).abs() < 5e-3, "30 degrees from up, towards +x: {limited:?}");
}

#[test]
fn a_joint_turns_by_an_angle_about_an_axis_whatever_the_pose() {
    let (model, _c) = column();
    let rig = model.rig.as_ref().unwrap();
    let top = rig.joint_index("top").unwrap();
    // the bone points along +y; turned 90 degrees about +z (right-hand rule) it points along -x; half the weight, half the angle; the joint stays where it was
    let pointing = |degrees: f32, weight: f32, axis: V3| {
        let g = IkGoal { joint: top, kind: IkKind::Turn { axis, degrees }, target: [0.0; 3], pin: None, pole: None, weight };
        let m = rig.skin_matrices_ik(&rest_layers(), &[g]);
        assert!(length(sub(at(&m, 3, REST[3]), REST[3])) < 1e-5, "the joint stays in place");
        normalize(sub(at(&m, 3, [0.0, 3.4, 0.0]), at(&m, 3, REST[3])))
    };
    let full = pointing(90.0, 1.0, [0.0, 0.0, 1.0]);
    assert!(full[0] < -0.999 && full[1].abs() < 1e-3, "{full:?}");
    let half = pointing(90.0, 0.5, [0.0, 0.0, 2.0]);
    assert!((half[0] + 0.7071).abs() < 2e-3 && (half[1] - 0.7071).abs() < 2e-3, "{half:?}");
    let back = pointing(-90.0, 1.0, [0.0, 0.0, 1.0]);
    assert!(back[0] > 0.999, "{back:?}");
    for (deg, axis) in [(f32::NAN, [0.0, 0.0, 1.0]), (90.0, [0.0, 0.0, 0.0])] {
        let none = pointing(deg, 1.0, axis);
        assert!(none[1] > 0.9999, "nothing happens: {none:?}");
    }
}

#[test]
fn an_ik_goal_entity_bends_the_model_in_the_picture_and_follows_the_goal() {
    let mut s = Sim::new(1);
    let spawn = |s: &mut Sim, uid: u64, comps: &str| {
        let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    };
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.5,6]},"Camera":{}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"MeshRenderer":{"mesh":"examples/models/swayer.glb"},"Animator":{"clip":"","playing":false}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[1.0,1.5,0]},"Ik":{"skin":2,"joint":"top"}}"#);
    let tip_x = |s: &mut Sim| {
        let mut tex = TextureCache::new(Some(repo_root()));
        let scene = render::gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 64, ..RenderOptions::default() }).unwrap();
        assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
        let m = scene.meshes[0].mesh.get();
        let top: Vec<f32> = m.pos.iter().filter(|p| p[1] > 1.0).map(|p| p[0]).collect();
        top.iter().sum::<f32>() / top.len().max(1) as f32
    };
    let bent = tip_x(&mut s);
    assert!(bent > 0.4, "the upper part leans towards the goal at x = 1: {bent}");
    // move the goal to the other side and the model follows; switch it off and it stands straight again
    s.apply(&Command::Set { uid: 3, component: "Transform".into(), value: parse(r#"{"pos":[-1.0,1.5,0]}"#).unwrap() }).unwrap();
    assert!(tip_x(&mut s) < -0.4);
    s.apply(&Command::Set { uid: 3, component: "Ik".into(), value: parse(r#"{"active":false}"#).unwrap() }).unwrap();
    // without the goal the model is exactly what the animation alone makes of it
    let (model, _c) = column();
    let plain = model.rig.as_ref().unwrap().pose(&model, Some(0), 0.0, true);
    let upper: Vec<f32> = plain.pos.iter().filter(|p| p[1] > 1.0).map(|p| p[0]).collect();
    let expected = upper.iter().sum::<f32>() / upper.len() as f32;
    assert!((tip_x(&mut s) - expected).abs() < 1e-5, "{} vs {expected}", tip_x(&mut s));
    // a joint that does not exist is a warning, not a crash
    s.apply(&Command::Set { uid: 3, component: "Ik".into(), value: parse(r#"{"active":true,"joint":"elbow"}"#).unwrap() }).unwrap();
    let mut tex = TextureCache::new(Some(repo_root()));
    let scene = render::gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 64, ..RenderOptions::default() }).unwrap();
    assert!(scene.warnings.iter().any(|w| w.contains("no joint called `elbow`")), "{:?}", scene.warnings);
}

#[test]
fn a_thing_fixed_to_a_joint_is_drawn_where_the_joint_is_in_the_pose_and_otherwise_at_its_own_place() {
    let mut s = Sim::new(1);
    let spawn = |s: &mut Sim, uid: u64, comps: &str| {
        let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    };
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.5,6]},"Camera":{}}"#);
    // the column model, bent towards x = 1 by an Ik goal, standing at x = 5; a small cube is fixed to its top joint with an offset of 0.5 up
    spawn(&mut s, 2, r#"{"Transform":{"pos":[5,0,0]},"MeshRenderer":{"mesh":"examples/models/swayer.glb"},"Animator":{"clip":"","playing":false}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[6.0,1.5,0]},"Ik":{"skin":2,"joint":"top"}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[-20,0,0]},"MeshRenderer":{"mesh":"cube"},"BoneAttach":{"target":2,"joint":"top","pos":[0,0.5,0]}}"#);
    let gather = |s: &mut Sim| {
        let mut tex = TextureCache::new(Some(repo_root()));
        render::gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 64, ..RenderOptions::default() }).unwrap()
    };
    let scene = gather(&mut s);
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    let cube = scene.meshes.iter().find(|m| m.uid == 4).expect("the cube is drawn");
    let (model, _c) = column();
    let rig = model.rig.as_ref().unwrap();
    let top = rig.joint_index("top").unwrap();
    // the same pose by hand: the first clip at time 0 (what an empty clip name plays) and the IK goal in the model's own space
    let goal = IkGoal { joint: top, kind: IkKind::Reach, target: [1.0, 1.5, 0.0], pin: None, pole: None, weight: 1.0 };
    let first_clip = [PoseLayer { clip: Some(0), time: 0.0, looping: true, weight: 1.0, root: None, inertia: None }];
    let worlds = rig.joint_worlds(&first_clip, &[goal]);
    let expect = M4::from_trs([5.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0]).mul(&worlds[top]).mul(&M4::from_trs([0.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0]));
    for k in 12..15 {
        assert!((cube.model.0[k] - expect.0[k]).abs() < 1e-3, "element {k}: {} vs {}", cube.model.0[k], expect.0[k]);
    }
    assert!(cube.model.0[12] > 5.3 && cube.model.0[12] < 6.5, "near the leaning tip, not at -20: {}", cube.model.0[12]);
    // the thing is drawn after the model, and a joint that does not exist is a warning that leaves it at its own place
    spawn(&mut s, 5, r#"{"Transform":{"pos":[-3,0,0]},"MeshRenderer":{"mesh":"cube"},"BoneAttach":{"target":2,"joint":"elbow"}}"#);
    let scene = gather(&mut s);
    assert!(scene.warnings.iter().any(|w| w.contains("no joint called `elbow` for BoneAttach")), "{:?}", scene.warnings);
    let lost = scene.meshes.iter().find(|m| m.uid == 5).expect("drawn");
    assert!((lost.model.0[12] + 3.0).abs() < 1e-5, "{}", lost.model.0[12]);
    // a target that is not an animated model leaves the thing where it is
    spawn(&mut s, 6, r#"{"Transform":{"pos":[7,0,0]},"MeshRenderer":{"mesh":"cube"},"BoneAttach":{"target":1,"joint":"top"}}"#);
    let scene = gather(&mut s);
    let plain = scene.meshes.iter().find(|m| m.uid == 6).expect("drawn");
    assert!((plain.model.0[12] - 7.0).abs() < 1e-5);
}

#[test]
fn a_thing_fixed_to_a_joint_with_no_target_follows_the_model_it_is_a_child_of() {
    // (a prefab cannot name the uid of its own root, so an attachment with `target` 0 is fixed to its parent: the same drawing as with the parent's uid named)
    let mut s = Sim::new(1);
    let spawn = |s: &mut Sim, uid: u64, comps: &str| {
        let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    };
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.5,6]},"Camera":{}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[5,0,0]},"MeshRenderer":{"mesh":"examples/models/swayer.glb"},"Animator":{"clip":"","playing":false}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[6.0,1.5,0]},"Ik":{"skin":2,"joint":"top"}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[-20,0,0]},"MeshRenderer":{"mesh":"cube"},"BoneAttach":{"target":2,"joint":"top","pos":[0,0.5,0]}}"#);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[-20,0,0]},"MeshRenderer":{"mesh":"cube"},"Parent":{"target":2},"BoneAttach":{"joint":"top","pos":[0,0.5,0]}}"#);
    // an entity with no target and no parent stays where it is
    spawn(&mut s, 6, r#"{"Transform":{"pos":[-3,0,0]},"MeshRenderer":{"mesh":"cube"},"BoneAttach":{"joint":"top"}}"#);
    let mut tex = TextureCache::new(Some(repo_root()));
    let scene = render::gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 64, ..RenderOptions::default() }).unwrap();
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    let named = scene.meshes.iter().find(|m| m.uid == 4).expect("drawn");
    let child = scene.meshes.iter().find(|m| m.uid == 5).expect("drawn");
    for k in 12..15 {
        assert!((named.model.0[k] - child.model.0[k]).abs() < 1e-5, "element {k}: {} vs {}", named.model.0[k], child.model.0[k]);
    }
    assert!(child.model.0[12] > 5.3, "at the leaning tip, not at -20: {}", child.model.0[12]);
    let lone = scene.meshes.iter().find(|m| m.uid == 6).expect("drawn");
    assert!((lone.model.0[12] + 3.0).abs() < 1e-5);
}

#[test]
fn a_pin_moves_a_joint_only_along_its_directions_and_one_sided_only_from_the_wrong_side() {
    use crate::rig::IkPin;
    let (model, _c) = column();
    let rig = model.rig.as_ref().unwrap();
    let top = rig.joint_index("top").unwrap();
    let near = |a: V3, b: V3, e: f32| (0..3).all(|k| (a[k] - b[k]).abs() < e);
    let pin = |target: V3, n1: V3, n2: Option<V3>, one_sided: bool| IkGoal { joint: top, kind: IkKind::Reach, target, pin: Some(IkPin { n1, n2, one_sided }), pole: None, weight: 1.0 };
    let tip = |g: IkGoal| at(&rig.skin_matrices_ik(&rest_layers(), &[g]), 3, REST[3]);
    // the tip rests at (0, 2.4, 0): level with y = 2.0 only, wherever the goal is in x and z: it comes down, the rest is as the animation has it
    assert!(near(tip(pin([5.0, 2.0, 5.0], [0.0, 1.0, 0.0], None, false)), [0.0, 2.0, 0.0], 4e-3));
    // two directions: y = 2.0 and z = 0.6, x stays
    assert!(near(tip(pin([5.0, 2.0, 0.6], [0.0, 1.0, 0.0], Some([0.0, 0.0, 1.0]), false)), [0.0, 2.0, 0.6], 4e-3));
    // one-sided, from above the plane (a goal at y = 2.0 and the direction down): the tip is on the wrong side and is pushed to it
    assert!(near(tip(pin([0.0, 2.0, 0.0], [0.0, -1.0, 0.0], None, true)), [0.0, 2.0, 0.0], 4e-3));
    // already on the right side (the plane at y = 2.6): nothing changes
    assert!(near(tip(pin([0.0, 2.6, 0.0], [0.0, -1.0, 0.0], None, true)), REST[3], 1e-3));
    // the weight blends it with the animation
    let mut half = pin([0.0, 2.0, 0.0], [0.0, 1.0, 0.0], None, false);
    half.weight = 0.5;
    let h = tip(half);
    assert!(h[1] < 2.39 && h[1] > 2.0, "between: {h:?}");
}

#[test]
fn a_goal_with_no_skin_bends_the_model_it_is_a_child_of() {
    let mut s = Sim::new(1);
    let spawn = |s: &mut Sim, uid: u64, comps: &str| {
        let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    };
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.5,6]},"Camera":{}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"MeshRenderer":{"mesh":"examples/models/swayer.glb"},"Animator":{"clip":"","playing":false}}"#);
    // the goal has no skin: it is a child of the model (a prefab could not know the model's uid), at x = 1 in the world
    spawn(&mut s, 3, r#"{"Transform":{"pos":[1.0,1.5,0]},"Parent":{"target":2},"LocalTransform":{"pos":[1.0,1.5,0]},"Ik":{"joint":"top"}}"#);
    let tip_x = |s: &mut Sim| {
        let mut tex = TextureCache::new(Some(repo_root()));
        let scene = render::gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 64, ..RenderOptions::default() }).unwrap();
        let m = scene.meshes[0].mesh.get();
        let top: Vec<f32> = m.pos.iter().filter(|p| p[1] > 1.0).map(|p| p[0]).collect();
        top.iter().sum::<f32>() / top.len().max(1) as f32
    };
    assert!(tip_x(&mut s) > 0.4, "the upper part leans towards the goal as it does with `skin` set");
    // an orphan goal with no skin does nothing (and is no crash)
    spawn(&mut s, 4, r#"{"Transform":{"pos":[-3.0,1.5,0]},"Ik":{"joint":"top"}}"#);
    assert!(tip_x(&mut s) > 0.4);
}
