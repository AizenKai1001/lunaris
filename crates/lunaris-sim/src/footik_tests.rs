//! Feet on the ground: the model follows the ground under its feet, planted feet are held on it, no foot goes below it.
use super::*;
use crate::components::{Ik, LocalTransform, Transform};
use crate::footik::FootIk;
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// A clip of 1 s: the left foot planted at (0.1, 0.09, 0.2) (ankle on the ground), the right one lifted at (-0.1, 0.5, -0.2).
fn table() -> String {
    let pts = |p: [f32; 3]| format!("[{},{},{}]", p[0], p[1], p[2]);
    let l: Vec<String> = (0..3).map(|_| pts([0.1, 0.09, 0.2])).collect();
    let r: Vec<String> = (0..3).map(|_| pts([-0.1, 0.5, -0.2])).collect();
    format!(r#"{{"rate":2,"clips":{{"walk":{{"dur":1.0,"l":[{}],"r":[{}],"cl":[1,1,1],"cr":[0,0,0]}}}}}}"#, l.join(","), r.join(","))
}

fn scene(tag: &str, step: Option<(f32, [f32; 3], [f32; 3])>) -> (Sim, std::path::PathBuf) {
    scene_with(tag, step.into_iter().collect(), [0.0, 0.0, -1.0], [0.0, 0.0])
}

/// The scene with any number of boxes (top height, centre xz, size xz: standing on the ground y 0) and the capsule at `at` wanting to walk `walk`.
fn scene_with(tag: &str, boxes: Vec<(f32, [f32; 3], [f32; 3])>, at: [f32; 3], walk: [f32; 2]) -> (Sim, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("lunaris_footik_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("feet.json"), table()).unwrap();
    let mut s = Sim::new(1);
    s.asset_root = Some(dir.clone());
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[40,1,40]}}"#);
    for (i, (top, c, size)) in boxes.into_iter().enumerate() {
        spawn(&mut s, 2 + i as u64, &format!(r#"{{"Transform":{{"pos":[{},{},{}]}},"Collider":{{"shape":1,"size":[{},{},{}]}}}}"#, c[0], top / 2.0, c[2], size[0], top, size[2]));
    }
    // (a thin capsule standing a metre before the place of the step, which is under the left foot of the clip)
    spawn(&mut s, 100, &format!(r#"{{"Transform":{{"pos":[{},{},{}]}},"CharacterBody":{{"radius":0.1,"height":1.76,"walk":[{},{}]}}}}"#, at[0], at[1], at[2], walk[0], walk[1]));
    spawn(&mut s, 101, r#"{"Parent":{"target":100},"LocalTransform":{},"Transform":{"pos":[0,0,0]},"Animator":{"clip":"walk","looping":true},"FootIk":{"data":"feet.json","goal_l":201,"goal_r":202,"floor_l":203,"floor_r":204,"pole_l":205,"pole_r":206}}"#);
    for uid in [205u64, 206] {
        spawn(&mut s, uid, r#"{"Transform":{"pos":[0,0,0]}}"#);
    }
    for (uid, joint) in [(201u64, "foot_l"), (202, "foot_r"), (203, "foot_l"), (204, "foot_r")] {
        spawn(&mut s, uid, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Ik":{{"skin":101,"joint":"{joint}","kind":2,"weight":0}}}}"#));
    }
    (s, dir)
}

fn goal(s: &Sim, uid: u64) -> (f32, [f32; 3], bool) {
    let e = s.world.entity(uid).unwrap();
    let g = s.world.get::<Ik>(e).unwrap();
    (g.weight, s.world.get::<Transform>(e).unwrap().pos, g.one_sided)
}

#[test]
fn on_flat_ground_the_planted_foot_is_held_on_it_and_the_lifted_one_is_only_kept_out_of_it() {
    let (mut s, dir) = scene("flat", None);
    for _ in 0..30 {
        s.step();
    }
    let e = s.world.entity(101).unwrap();
    let fk = *s.world.get::<FootIk>(e).unwrap();
    assert!(fk.live && fk.off.abs() < 0.01, "the model rests on the capsule: {fk:?}");
    // the planted left foot: the ankle's height above the ground, the contact goal counts in full, the floor goal too
    let (w, p, one) = goal(&s, 201);
    assert!(w > 0.99 && !one && (p[1] - 0.09).abs() < 1e-3, "{w} {p:?}");
    let (wf, pf, onef) = goal(&s, 203);
    assert!(wf > 0.99 && onef && (pf[1] - 0.09).abs() < 1e-3, "{wf} {pf:?}");
    // the lifted right foot: the contact goal is off, the floor goal on
    assert!(goal(&s, 202).0 < 0.01 && goal(&s, 204).0 > 0.99);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_foot_over_the_edge_of_the_platform_the_character_stands_on_is_not_pulled_down_to_the_floor_below() {
    // (the character has just climbed onto a block a metre high and stands at its edge: the clip's planted foot is over the edge, the ground under it is a metre lower: the goal must not put the foot there)
    let (mut s, dir) = scene_with("edge", vec![(1.0, [-2.45, 0.0, -1.0], [5.0, 1.0, 8.0])], [0.0, 1.0, -1.0], [0.0, 0.0]);
    for _ in 0..40 {
        s.step();
    }
    for uid in [201u64, 202, 203, 204] {
        let (w, p, _) = goal(&s, uid);
        assert!(w < 0.01 || p[1] > 0.85, "goal {uid} (weight {w}) asks for a foot at {p:?}: on the floor below the platform");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn walking_away_from_the_edge_of_a_platform_the_model_is_not_lowered_by_the_floor_behind_it() {
    // (the line the ground makes under the character is read a metre behind it too: there the floor is a metre and a half lower, which pulled the pelvis down for as long as the character was near the edge)
    let (mut s, dir) = scene_with("edgewalk", vec![(1.0, [-2.45, 0.0, -1.0], [5.0, 1.0, 8.0])], [-0.2, 1.0, -1.0], [-1.5, 0.0]);
    let mut worst = 0.0f32;
    for k in 0..40 {
        s.step();
        if k >= 5 {
            worst = worst.max(s.world.get::<FootIk>(s.world.entity(101).unwrap()).unwrap().off.abs());
        }
    }
    assert!(worst < 0.06, "the model sits {worst} m under the capsule near the edge");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn on_level_ground_a_foot_is_neither_locked_nor_pinned_along_its_sole() {
    // (a lock would hold the planted foot at a place while the clip's own foot goes on as the clip's speed says and the leg is pulled out straight; a pin along the way the foot points would pull it away from the pose
    // on the screen after a switch of clips: on level ground only the height is pinned)
    let (mut s, dir) = scene("level", None);
    s.world.get_mut::<crate::components::CharacterBody>(s.world.entity(100).unwrap()).unwrap().walk = [0.0, 1.5];
    for _ in 0..60 {
        s.step();
    }
    let fk = *s.world.get::<FootIk>(s.world.entity(101).unwrap()).unwrap();
    assert!(!fk.locked_l && !fk.locked_r, "no lock on level ground: {fk:?}");
    for uid in [201u64, 203] {
        let ik = *s.world.get::<Ik>(s.world.entity(uid).unwrap()).unwrap();
        assert_eq!(ik.axis2, [0.0; 3], "goal {uid} pins only the height");
    }
    assert!(goal(&s, 201).0 > 0.99, "and the planted foot is still held on the ground");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn every_foot_goal_has_a_knee_pole_ahead_of_the_character_at_the_height_of_its_hips() {
    // (the knees bend towards the poles: a knee never bends backwards, whatever the clip has)
    let (mut s, dir) = scene("poles", None);
    for _ in 0..30 {
        s.step();
    }
    for (goal, pole) in [(201u64, 205u64), (203, 205), (202, 206), (204, 206)] {
        let ik = *s.world.get::<Ik>(s.world.entity(goal).unwrap()).unwrap();
        assert_eq!(ik.pole.0, pole, "goal {goal} bends its knee towards the pole {pole}");
    }
    let hero = s.world.get::<Transform>(s.world.entity(100).unwrap()).unwrap().pos;
    let fk = *s.world.get::<FootIk>(s.world.entity(101).unwrap()).unwrap();
    for (uid, side) in [(205u64, 1.0f32), (206, -1.0)] {
        let p = s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos;
        // (the character faces +z: its left, the model's +x, is the world's +x)
        assert!((p[2] - (hero[2] + 1.0)).abs() < 1e-3 && (p[0] - (hero[0] + 0.09 * side)).abs() < 1e-3, "the pole is a metre ahead, a hand's breadth to its side: {p:?} {hero:?}");
        assert!((p[1] - (hero[1] + fk.off + 0.9)).abs() < 1e-3, "at the height of the hips: {p:?}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_goals_wait_a_few_pictures_after_the_touch_down_before_they_come_on() {
    // (found by tools/transition_check.py: the picture of the touch-down is still the old clip's, the landing clip comes a picture later: a goal on for that picture pulled the foot back and let it go again)
    let (mut s, dir) = scene_with("grace", vec![], [0.0, 1.5, -1.0], [0.0, 0.0]);
    let e = s.world.entity(101).unwrap();
    let hero = s.world.entity(100).unwrap();
    let mut on_ground = 0;
    let mut weights = Vec::new();
    for _ in 0..200 {
        s.step();
        if s.world.get::<crate::components::CharacterBody>(hero).unwrap().grounded {
            on_ground += 1;
            weights.push(s.world.get::<FootIk>(e).unwrap().wk);
            if on_ground >= 8 {
                break;
            }
        }
    }
    assert!(weights.len() >= 8, "the character lands: {weights:?}");
    assert!(weights[..2].iter().all(|w| *w == 0.0), "no goal in the first two pictures on the ground: {weights:?}");
    assert!(weights[7] > 0.0, "and then they come on: {weights:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_goals_come_on_gradually_after_the_character_lands() {
    // (a foot that the clip has in the ground at the moment of landing must be brought out of it, not thrown out in one picture)
    let (mut s, dir) = scene_with("land", vec![], [0.0, 1.5, -1.0], [0.0, 0.0]);
    let e = s.world.entity(101).unwrap();
    let mut first = None;
    for _ in 0..200 {
        s.step();
        let wk = s.world.get::<FootIk>(e).unwrap().wk;
        if wk > 0.0 {
            first = Some(wk);
            break;
        }
    }
    let first = first.expect("the character lands");
    assert!(first <= 0.35, "the first tick on the ground has only a part of the weight: {first}");
    for _ in 0..12 {
        s.step();
    }
    assert!(s.world.get::<FootIk>(e).unwrap().wk > 0.99, "and in a tenth of a second all of it");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_step_under_one_foot_raises_its_goal_and_the_model_follows_the_mean_of_the_ground_smoothly() {
    // a step 0.2 m high and 0.5 m deep under the left foot's place (x 0.1, z -0.8: the whole foot is on it)
    let (mut s, dir) = scene("step", Some((0.2, [0.1, 0.0, -0.7], [0.4, 0.2, 0.5])));
    s.step();
    let e = s.world.entity(101).unwrap();
    let hero = s.world.entity(100).unwrap();
    // the capsule stands on the ground (the step is not under it)
    for _ in 0..30 {
        s.step();
    }
    let y = s.world.get::<Transform>(hero).unwrap().pos[1];
    assert!(y.abs() < 0.05, "the capsule is on the ground: {y}");
    let fk = *s.world.get::<FootIk>(e).unwrap();
    // the model follows the line the ground makes under it (the average along the way over nearly two metres: the half metre of step in it makes 5.5 cm), within the legs' reach of the planted foot (0.2), smoothly
    assert!((fk.off - 0.055).abs() < 0.02, "the model follows the line of the ground: {fk:?}");
    assert!((fk.off - fk.line_y).abs() < 0.01, "{fk:?}");
    assert!((s.world.get::<LocalTransform>(e).unwrap().pos[1] - fk.off).abs() < 1e-6);
    assert!((goal(&s, 201).1[1] - 0.29).abs() < 1e-3, "the left ankle on the step: {:?}", goal(&s, 201));
    assert!((goal(&s, 204).1[1] - 0.09).abs() < 1e-3, "the right one on the ground: {:?}", goal(&s, 204));
    // the correction the left foot needs: the clip has the ankle at gs + 0.09 = 0.145, the step wants 0.29
    assert!((fk.lift_l - 0.145).abs() < 0.03, "{fk:?}");
    // it does not jump: the first tick the model was at the capsule's height
    let (mut s2, dir2) = scene("step2", Some((0.2, [0.1, 0.0, -0.7], [0.4, 0.2, 0.5])));
    s2.step();
    let e2 = s2.world.entity(101).unwrap();
    s2.step();
    let first = s2.world.get::<FootIk>(e2).unwrap().off;
    assert!(first.abs() < 0.12, "{first}");
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(dir2);
}

#[test]
fn off_the_ground_or_without_weight_nothing_is_written() {
    let (mut s, dir) = scene("air", None);
    for _ in 0..20 {
        s.step();
    }
    assert!(goal(&s, 201).0 > 0.9);
    s.world.get_mut::<FootIk>(s.world.entity(101).unwrap()).unwrap().weight = 0.0;
    for _ in 0..40 {
        s.step();
    }
    for uid in [201u64, 202, 203, 204] {
        assert_eq!(goal(&s, uid).0, 0.0, "goal {uid} off");
    }
    let e = s.world.entity(101).unwrap();
    assert!(!s.world.get::<FootIk>(e).unwrap().live);
    let _ = std::fs::remove_dir_all(dir);
}

/// Six steps of 0.2 m up, then flat: the box of each from the ground to its top (a tread is 0.25 m deep, from z 1.0 on).
fn flight(up: bool) -> Vec<(f32, [f32; 3], [f32; 3])> {
    let mut v = Vec::new();
    if !up {
        v.push((1.2, [0.0, 0.0, -3.5], [4.0, 0.0, 9.0]));
    }
    for i in 1..=6 {
        let top = if up { i as f32 * 0.2 } else { 1.2 - i as f32 * 0.2 };
        if top > 0.01 {
            v.push((top, [0.0, 0.0, 1.0 + (i as f32 - 0.5) * 0.25], [3.0, 0.0, 0.25]));
        }
    }
    v
}

fn ahead_of(boxes: Vec<(f32, [f32; 3], [f32; 3])>, at: [f32; 3], walk: [f32; 2], tag: &str) -> (f32, f32) {
    let (mut s, dir) = scene_with(tag, boxes, at, walk);
    for _ in 0..20 {
        s.step();
    }
    let fk = *s.world.get::<FootIk>(s.world.entity(101).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(dir);
    (fk.ahead, fk.rise)
}

#[test]
fn stairs_ahead_are_seen_up_and_down_but_not_a_kerb_or_flat_ground() {
    // before a flight up: the rays reach the steps, two in a row
    let (a, rise) = ahead_of(flight(true), [0.0, 0.0, 0.7], [0.0, 0.5], "up");
    assert_eq!(a, 1.0, "up: rise {rise}");
    // on the flight (the ground behind is lower, the next step higher)
    let (a, _) = ahead_of(flight(true), [0.0, 0.6, 1.6], [0.0, 0.5], "mid");
    assert_eq!(a, 1.0);
    // down: the character on the platform before a flight down
    let (a, rise) = ahead_of(flight(false), [0.0, 1.2, 0.7], [0.0, 0.5], "down");
    assert_eq!(a, -1.0, "down: rise {rise}");
    // a single kerb 0.3 m high, a metre deep: not stairs
    let (a, _) = ahead_of(vec![(0.3, [0.0, 0.0, 1.5], [3.0, 0.0, 1.0])], [0.0, 0.0, 0.7], [0.0, 0.5], "kerb");
    assert_eq!(a, 0.0);
    // flat ground, and standing still before the flight
    assert_eq!(ahead_of(vec![], [0.0, 0.0, 0.7], [0.0, 0.5], "flat").0, 0.0);
    assert_eq!(ahead_of(flight(true), [0.0, 0.0, 0.7], [0.0, 0.0], "still").0, 0.0);
    // walking away from the flight: nothing ahead
    assert_eq!(ahead_of(flight(true), [0.0, 0.0, 0.7], [0.0, -0.5], "away").0, 0.0);
}

#[test]
fn a_stair_clip_puts_the_model_where_its_planted_foot_puts_it() {
    // a treadmill clip (its body keeps its height, the planted foot goes down under it): the planted left ankle is 0.4 above the model's origin, the lifted right one 0.6: the planted foot rests on the ground, so the origin
    // is 0.4 - 0.09 = 0.31 below it. The line the ground makes (flat: 0) is where the origin starts; it learns, slowly, how far the planted feet say it is from that line
    let (mut s, dir) = scene_with("stance", vec![], [0.0, 0.0, -1.0], [0.0, 0.0]);
    let l: Vec<String> = (0..3).map(|_| "[0.1,0.4,0.2]".to_string()).collect();
    let r: Vec<String> = (0..3).map(|_| "[-0.1,0.6,-0.2]".to_string()).collect();
    std::fs::write(dir.join("feet.json"), format!(r#"{{"rate":2,"clips":{{"walk":{{"stance":1,"dur":1.0,"l":[{}],"r":[{}],"cl":[1,1,1],"cr":[0,0,0]}}}}}}"#, l.join(","), r.join(","))).unwrap();
    for _ in 0..200 {
        s.step();
    }
    let fk = *s.world.get::<FootIk>(s.world.entity(101).unwrap()).unwrap();
    assert!((fk.off + 0.27).abs() < 0.02, "the model is lowered towards the planted foot (the dead band of 4 cm left over): {fk:?}");
    assert!((fk.lift_l + 0.04).abs() < 0.02, "and the planted foot is pulled down the rest: {fk:?}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A box on the ground: 0.2 high, 0.6 wide, from z -0.9 to z -0.4 (the left foot of the clip is 0.2 in front of the capsule, so with the capsule at z -0.7 its ankle is at z -0.5).
fn tread() -> Vec<(f32, [f32; 3], [f32; 3])> {
    vec![(0.2, [0.1, 0.0, -0.65], [0.6, 0.2, 0.5])]
}

#[test]
fn a_foot_that_would_land_across_the_edge_of_a_step_is_moved_onto_the_tread_and_held_there() {
    // the ankle at z -0.5 puts the heel on the box and the toe (0.21 in front) over its edge at z -0.4: the foot is moved back along the way the character faces until the toe is at the edge
    let (mut s, dir) = scene_with("fit", tread(), [0.0, 0.2, -0.7], [0.0, 0.0]);
    for _ in 0..30 {
        s.step();
    }
    let (w, p, _) = goal(&s, 201);
    assert!(w > 0.99, "{w}");
    assert!((p[2] - (-0.4 - 0.19 + 0.03)).abs() < 0.015, "the toe (less 2 cm) is at the edge, hanging 3 cm over its nose: the ankle at z -0.56, not -0.5: {p:?}");
    assert!((p[1] - 0.29).abs() < 1e-3, "on the tread: {p:?}");
    let e = s.world.entity(101).unwrap();
    let fk = *s.world.get::<FootIk>(e).unwrap();
    assert!(fk.locked_l && (fk.lk_l[1] - p[2]).abs() < 1e-4, "held there: {fk:?}");
    // the lock does not follow the capsule, which creeps on while the clip keeps the foot planted: only the clip's own foot (and the model's height) moves
    let hero = s.world.entity(100).unwrap();
    s.world.get_mut::<crate::components::CharacterBody>(hero).unwrap().walk = [0.0, 0.5];
    for _ in 0..30 {
        s.step();
    }
    let (_, p2, _) = goal(&s, 201);
    assert!((p2[2] - p[2]).abs() < 1e-4, "the planted foot stays where it was put: {p:?} {p2:?}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A clip of 1 s that steps: the left foot planted at z 0.2 for half a second, then swung 0.9 m forward (the right one lifted all the time).
fn stepping_table() -> String {
    let pts = |p: [f32; 3]| format!("[{},{},{}]", p[0], p[1], p[2]);
    let l = [pts([0.1, 0.09, 0.2]), pts([0.1, 0.09, 0.2]), pts([0.1, 0.2, 0.6]), pts([0.1, 0.25, 1.0]), pts([0.1, 0.09, 1.1])];
    let r: Vec<String> = (0..5).map(|_| pts([-0.1, 0.5, -0.2])).collect();
    format!(r#"{{"rate":4,"clips":{{"walk":{{"dur":1.0,"l":[{}],"r":[{}],"cl":[1,1,0,0,1],"cr":[0,0,0,0,0]}}}}}}"#, l.join(","), r.join(","))
}

#[test]
fn a_foot_that_was_held_on_a_tread_is_let_go_of_by_gliding_to_the_clips_foot_not_by_jumping_there() {
    // the foot is fitted to a tread (held there), then the clip swings it forward: the goal must not jump to where the clip has the foot (up to the lock's leash, 0.35 m, in one picture)
    let (mut s, dir) = scene_with("release", tread(), [0.0, 0.2, -0.7], [0.0, 0.0]);
    std::fs::write(dir.join("feet.json"), stepping_table()).unwrap();
    let mut last: Option<f32> = None;
    let mut worst = 0.0f32;
    let mut locked_seen = false;
    for _ in 0..150 {
        s.step();
        let (w, p, _) = goal(&s, 201);
        let fk = *s.world.get::<FootIk>(s.world.entity(101).unwrap()).unwrap();
        locked_seen |= fk.locked_l;
        if w > 0.5 {
            if let Some(l) = last {
                worst = worst.max((p[2] - l).abs());
            }
            last = Some(p[2]);
        } else {
            last = None;
        }
    }
    assert!(locked_seen, "the foot was held on the tread at some time");
    assert!(worst < 0.1, "the goal moved {worst} m in one picture");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_weight_of_a_goal_never_drops_in_one_picture_while_the_capsule_walks_to_the_edge_of_its_platform() {
    // (the capsule walks to the edge of a block 1 m high: the clip's left foot, 0.2 ahead of it, goes over the edge and the floor under it is far lower: its goal fades out where it was, it is not switched off in one picture)
    let (mut s, dir) = scene_with("edgefade", vec![(1.0, [0.0, 0.0, -1.0], [4.0, 1.0, 2.0])], [0.0, 1.0, -0.6], [0.0, 0.0]);
    // on the platform: the goal holds the foot
    for _ in 0..20 {
        s.step();
    }
    let (w0, _, _) = goal(&s, 201);
    assert!(w0 > 0.5, "held on the platform: {w0}");
    // the capsule goes on, the foot is over the void: the weight goes down gradually
    let hero = s.world.entity(100).unwrap();
    s.world.get_mut::<crate::components::CharacterBody>(hero).unwrap().walk = [0.0, 0.5];
    let mut last = w0;
    let mut biggest_drop = 0.0f32;
    let mut went_out = false;
    // (65 pictures: the capsule is still on the platform, whose edge it reaches after about 72)
    for _ in 0..65 {
        s.step();
        let (w, _, _) = goal(&s, 201);
        biggest_drop = biggest_drop.max(last - w);
        went_out |= w < 0.01;
        last = w;
    }
    let _ = went_out;
    assert!(biggest_drop < 0.35, "the weight went down by {biggest_drop} in one picture");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_goal_that_fades_out_over_the_edge_of_a_platform_keeps_its_pin_along_the_foot() {
    // (found by tools/transition_check.py: a foot held behind the clip's foot jumped 0.4 m forward in one picture as the character walked off a platform: the fading goal had lost the pin along the ground.
    // The capsule stands at the edge, the clip's left foot over the void; the goal was just at full weight where the foot was held)
    let (mut s, dir) = scene_with("edgepin", vec![(1.0, [0.0, 0.0, -1.0], [4.0, 1.0, 2.0])], [0.0, 1.0, -0.15], [0.0, 0.0]);
    for _ in 0..3 {
        s.step();
    }
    let rig = s.world.entity(101).unwrap();
    {
        let fk = s.world.get_mut::<FootIk>(rig).unwrap();
        fk.edge_l = 1.0;
        fk.hold_l = [0.1, 1.09, -0.3];
    }
    s.step();
    let e = s.world.entity(201).unwrap();
    let g = *s.world.get::<Ik>(e).unwrap();
    assert!(g.weight > 0.8 && g.weight < 0.95, "the goal fades from its full weight over a few pictures: {}", g.weight);
    let len = (g.axis2[0] * g.axis2[0] + g.axis2[1] * g.axis2[1] + g.axis2[2] * g.axis2[2]).sqrt();
    assert!(len > 0.5, "a fading goal has no pin along the foot: {:?}", g.axis2);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_toes_of_a_swinging_foot_do_not_go_into_the_riser_ahead() {
    // the right foot of the clip is lifted (0.5 up) at z 0.2 behind the capsule; a step 0.3 high from z 0.3 on: the toe tip at 0.21 in front of the ankle is over the step when the ankle is at 0.2 + 0.1
    let (mut s, dir) = scene_with("toes", vec![(0.3, [-0.1, 0.0, 0.8], [0.6, 0.3, 1.0])], [0.0, 0.0, 0.0], [0.0, 0.0]);
    for _ in 0..30 {
        s.step();
    }
    // the right ankle is at z -0.2 (clip) + 0 = -0.2: its toe tip at 0.01: not over the step (which starts at z 0.3): the floor goal asks for the ground (0.09)
    let (wf, pf, onef) = goal(&s, 204);
    assert!(wf > 0.99 && onef && (pf[1] - 0.09).abs() < 1e-3, "{wf} {pf:?}");
    // with the capsule 0.4 further forward the toe tip is at 0.41 over the step: the floor goal asks for its top (0.3) plus the ankle's height
    let hero = s.world.entity(100).unwrap();
    s.world.get_mut::<Transform>(hero).unwrap().pos = [0.0, 0.3, 0.45];
    for _ in 0..20 {
        s.step();
    }
    let (wf, pf, _) = goal(&s, 204);
    assert!(wf > 0.99 && (pf[1] - 0.39).abs() < 1e-3, "the toes are kept above the step: {pf:?}");
    let _ = std::fs::remove_dir_all(dir);
}

fn drive(s: &mut Sim, on: bool) {
    let e = s.world.entity(101).unwrap();
    s.world.get_mut::<FootIk>(e).unwrap().drive = on;
}

fn feet_of(s: &Sim) -> FootIk {
    *s.world.get::<FootIk>(s.world.entity(101).unwrap()).unwrap()
}

#[test]
fn the_gait_walks_each_foot_onto_its_own_tread_one_ahead_of_the_other_and_stands_still_with_the_character() {
    // a flight of six steps (0.2 high, 0.25 deep, from z 1.0); the capsule walks up it slowly
    let (mut s, dir) = scene_with("gait", flight(true), [0.0, 0.0, -0.2], [0.0, 0.0]);
    for _ in 0..10 {
        s.step();
    }
    drive(&mut s, true);
    let hero = s.world.entity(100).unwrap();
    s.world.get_mut::<crate::components::CharacterBody>(hero).unwrap().walk = [0.0, 0.4];
    let mut landings_l: Vec<[f32; 3]> = Vec::new();
    let mut landings_r: Vec<[f32; 3]> = Vec::new();
    let (mut prev_l, mut prev_r) = (-1.0f32, -1.0f32);
    for _ in 0..420 {
        s.step();
        let fk = feet_of(&s);
        assert!(fk.gait, "the gait runs: {fk:?}");
        // a foot on its tread has its ankle one ankle's height over the top of a step (a multiple of 0.2)
        for (sw, tg) in [(fk.sw_l, fk.tg_l), (fk.sw_r, fk.tg_r)] {
            if sw < 0.0 {
                let sole = tg[1] - 0.09;
                let k = (sole / 0.2).round();
                assert!((sole - k * 0.2).abs() < 0.01, "a foot on its tread rests on the top of a step: sole {sole}, {fk:?}");
            }
        }
        if prev_l >= 0.0 && fk.sw_l < 0.0 {
            landings_l.push(fk.tg_l);
        }
        if prev_r >= 0.0 && fk.sw_r < 0.0 {
            landings_r.push(fk.tg_r);
        }
        (prev_l, prev_r) = (fk.sw_l, fk.sw_r);
        if s.world.get::<Transform>(hero).unwrap().pos[2] > 2.2 {
            break;
        }
    }
    assert!(landings_l.len() >= 2 && landings_r.len() >= 2, "each foot made steps: {} and {}", landings_l.len(), landings_r.len());
    // a foot lands about two treads (0.5 m) further than the last time it did, never on the tread of the other foot
    for w in landings_l.windows(2).chain(landings_r.windows(2)) {
        let d = w[1][2] - w[0][2];
        assert!((d - 0.5).abs() < 0.08, "a foot goes two treads on: {d}");
    }
    // the character stops: a step in the air is finished and the feet stay where they are
    s.world.get_mut::<crate::components::CharacterBody>(hero).unwrap().walk = [0.0, 0.0];
    for _ in 0..60 {
        s.step();
    }
    let a = feet_of(&s);
    assert!(a.sw_l < 0.0 && a.sw_r < 0.0, "both feet are down: {a:?}");
    for _ in 0..120 {
        s.step();
    }
    let b = feet_of(&s);
    assert!(a.tg_l == b.tg_l && a.tg_r == b.tg_r && (a.theta - b.theta).abs() < 1e-6, "and they stay: {a:?} {b:?}");
    // the game lets go of the gait: the feet are no longer locked at the gait's places (a foot that has to be fitted to a tread is locked again, by the clip's own rules), the goals stay on the feet where the gait had them and ease, over a third of a second, to the places the clip that plays next has the feet at
    drive(&mut s, false);
    s.step();
    let c = feet_of(&s);
    assert!(!c.gait && c.smx > 0.9, "{c:?}");
    let (w, p, _) = goal(&s, 201);
    assert!(w > 0.9 && (p[2] - a.tg_l[2]).abs() < 0.05 && (p[1] - a.tg_l[1]).abs() < 0.05, "the goal is still on the gait's foot: {w} {p:?} {:?}", a.tg_l);
    for _ in 0..40 {
        s.step();
    }
    let d = feet_of(&s);
    assert!(d.smx == 0.0 && !d.gait, "the easing is over: {d:?}");
    let _ = std::fs::remove_dir_all(dir);
}
