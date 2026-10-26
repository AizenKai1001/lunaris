//! Traversal: sensing the obstacle, the kind of move, and a move played with motion warping.
use super::*;
use crate::components::{Animator, CharacterBody, Transform};
use crate::motionmatch::MotionMatcher;
use crate::traversal::{Traversal, HURDLE, MANTLE, VAULT};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// A world with the ground (a box whose top is y = 0), and obstacles: `(uid, centre, size)`.
fn world(obstacles: &[(u64, [f32; 3], [f32; 3])], ground: bool) -> Sim {
    let mut s = Sim::new(1);
    if ground {
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[40,1,40]}}"#);
    }
    for (uid, c, size) in obstacles {
        spawn(&mut s, *uid, &format!(r#"{{"Transform":{{"pos":[{},{},{}]}},"Collider":{{"shape":1,"size":[{},{},{}]}},"Traversable":{{}}}}"#, c[0], c[1], c[2], size[0], size[1], size[2]));
    }
    s
}

fn check(s: &mut Sim, feet: [f32; 3], fwd: [f32; 2], reach: f32) -> Option<traversal::Check> {
    let bx = traversal::boxes_for_tests(&mut s.world);
    traversal::sense_for_tests(&bx, feet, fwd, reach, 0.3)
}

#[test]
fn a_thin_wall_with_floor_behind_is_a_hurdle_a_thick_block_a_mantle_a_tall_one_a_climb() {
    // a wall 1 m high, 0.2 m thick, at z = 3
    let mut s = world(&[(10, [0.0, 0.5, 3.0], [4.0, 1.0, 0.2])], true);
    let c = check(&mut s, [0.0, 0.0, 0.0], [0.0, 1.0], 3.0).expect("the wall is in the way");
    assert_eq!(c.action, HURDLE, "{c:?}");
    assert!((c.height - 1.0).abs() < 1e-4 && (c.depth - 0.2).abs() < 1e-4 && c.has_floor && (c.drop - 1.0).abs() < 1e-3, "{c:?}");
    assert!((c.front[2] - 2.9).abs() < 1e-4 && (c.front[1] - 1.0).abs() < 1e-4 && (c.normal[2] + 1.0).abs() < 1e-5, "front ledge at the near edge: {c:?}");
    assert!((c.back[2] - 3.1).abs() < 1e-4, "{c:?}");
    assert!((c.distance - 2.9).abs() < 1e-4, "{c:?}");
    // too far to reach
    assert!(check(&mut s, [0.0, 0.0, -2.0], [0.0, 1.0], 3.0).is_none());
    // looking away
    assert!(check(&mut s, [0.0, 0.0, 0.0], [0.0, -1.0], 3.0).is_none());
    // no ground behind it: a vault
    let mut s = world(&[(10, [0.0, 0.5, 3.0], [4.0, 1.0, 0.2])], false);
    let c = check(&mut s, [0.0, 0.0, 0.0], [0.0, 1.0], 3.0).unwrap();
    assert_eq!(c.action, VAULT, "{c:?}");
    // a block 1.2 m deep is mantled, up to 2.75 m high; above 1.5 it is the climb (the move decides)
    let mut s = world(&[(10, [0.0, 0.5, 3.6], [4.0, 1.0, 1.2])], true);
    assert_eq!(check(&mut s, [0.0, 0.0, 0.0], [0.0, 1.0], 3.0).unwrap().action, MANTLE);
    let mut s = world(&[(10, [0.0, 1.0, 3.6], [4.0, 2.0, 1.2])], true);
    let c = check(&mut s, [0.0, 0.0, 0.0], [0.0, 1.0], 3.0).unwrap();
    assert_eq!((c.action, c.height as i32), (MANTLE, 2));
    // too tall (3 m): nothing; too low (0.3 m): not an obstacle at all
    let mut s = world(&[(10, [0.0, 1.5, 3.6], [4.0, 3.0, 1.2])], true);
    assert_eq!(check(&mut s, [0.0, 0.0, 0.0], [0.0, 1.0], 3.0).unwrap().action, 0);
    let mut s = world(&[(10, [0.0, 0.15, 3.6], [4.0, 0.3, 1.2])], true);
    assert!(check(&mut s, [0.0, 0.0, 0.0], [0.0, 1.0], 3.0).is_none(), "a step is not an obstacle");
    // the ledge is where the character is headed, kept inside the margin of the end of a narrow edge
    let mut s = world(&[(10, [0.0, 0.5, 3.0], [4.0, 1.0, 0.2])], true);
    let c = check(&mut s, [1.9, 0.0, 0.0], [0.0, 1.0], 3.0).unwrap();
    assert!(c.front[0] <= 1.8 + 1e-4 && c.front[0] > 1.5, "{c:?}");
}

#[test]
fn a_box_turned_a_quarter_round_is_sensed_the_same() {
    // the same wall along z instead of x, at x = 3, the character facing +x
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[40,1,40]}}"#);
    let q = (std::f32::consts::FRAC_PI_4).sin_cos();
    spawn(&mut s, 10, &format!(r#"{{"Transform":{{"pos":[3,0.5,0],"rot":[0,{},0,{}]}},"Collider":{{"shape":1,"size":[4,1,0.2]}},"Traversable":{{}}}}"#, q.0, q.1));
    let c = check(&mut s, [0.0, 0.0, 0.0], [1.0, 0.0], 3.0).expect("a wall turned by 90 degrees");
    assert_eq!(c.action, HURDLE, "{c:?}");
    assert!((c.front[0] - 2.9).abs() < 1e-3 && (c.normal[0] + 1.0).abs() < 1e-4, "{c:?}");
}

/// A move: runs 1.7 m up to the ledge in half a second (the ledge at x = 0, z = 0, 1 m high), over it and down, 1.2 m beyond.
fn move_json() -> String {
    let n = 61;
    let mut root = Vec::new();
    for k in 0..n {
        let t = k as f32 / 30.0;
        let z = -1.7 + 2.9 * (t / 2.0);
        // an arc over the ledge: up to 1 m at z = 0
        let y = if t > 0.5 && t < 1.2 { (1.0 - ((t - 0.85) / 0.35).powi(2)).max(0.0) } else { 0.0 };
        root.push(format!("[0,{y:.4},{z:.4},0]"));
    }
    format!(
        r#"{{"rate":30,"montages":{{"hurdle_run":{{"action":2,"kind":"Hurdle","height":1.0,"gait":"run","length":2.0,"rate":30,"root":[{}],"windows":[{{"target":"front","start":0.5,"end":0.7,"rotation":true,"ignore_z":false}},{{"target":"floor","start":1.0,"end":1.2,"rotation":false,"ignore_z":false}}],"branch_end":0.3,"blend_out":{{"at":1.4,"time":0.3,"wait_input":false}}}}}}}}"#,
        root.join(",")
    )
}

#[test]
fn a_move_is_laid_on_the_real_ledge_the_character_goes_over_and_lands_on_the_real_floor() {
    let dir = std::env::temp_dir().join(format!("lunaris_trav_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("traversal.json"), move_json()).unwrap();
    // a wall 0.8 m high (lower than the clip's ledge) at z = 3; the ground behind it is 0.15 m higher
    let mut s = world(&[(10, [0.0, 0.4, 3.0], [4.0, 0.8, 0.2])], false);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,-17]},"Collider":{"shape":1,"size":[40,1,40]}}"#);
    spawn(&mut s, 11, r#"{"Transform":{"pos":[0,-0.35,13]},"Collider":{"shape":1,"size":[40,1,20]}}"#);
    s.asset_root = Some(dir.clone());
    spawn(&mut s, 100, r#"{"Transform":{"pos":[0.0,0.0,0.0]},"CharacterBody":{"radius":0.3,"height":1.76}}"#);
    spawn(&mut s, 101, r#"{"Parent":{"target":100},"LocalTransform":{},"Transform":{"pos":[0,0,0]},"Animator":{"clip":"idle"},"MotionMatcher":{},"Traversal":{"data":"traversal.json"}}"#);
    for _ in 0..10 {
        s.step();
    }
    let e = s.world.entity(101).unwrap();
    let hero = s.world.entity(100).unwrap();
    // ask while running at 5 m/s (the wall is looked for 3.5 m ahead), 2.2 m from the wall's near face (z = 2.9): the clip begins 1.7 m before the ledge, so the character first has to run 0.5 m: it waits
    s.world.get_mut::<MotionMatcher>(e).unwrap().vel_now = [0.0, 5.0];
    s.world.get_mut::<Transform>(hero).unwrap().pos = [0.0, 0.0, 0.7];
    s.world.get_mut::<Traversal>(e).unwrap().try_for = 0.5;
    s.step();
    let t = *s.world.get::<Traversal>(e).unwrap();
    assert_eq!(t.action, HURDLE, "{t:?}");
    assert!(!t.active && !t.failed, "too far for the clip: it waits ({t:?})");
    // with the wall in reach and the clip's distance matching, it starts at once
    s.world.get_mut::<Transform>(hero).unwrap().pos = [0.0, 0.0, 1.5];
    s.world.get_mut::<Traversal>(e).unwrap().try_for = 0.5;
    s.step();
    let t = *s.world.get::<Traversal>(e).unwrap();
    assert!(t.active, "{t:?}");
    let name = s.world.strings().get(t.clip.0).to_string();
    assert_eq!(name, "hurdle_run");
    // play it out
    let mut top = 0.0f32;
    let mut exit_at = None;
    for k in 0..200 {
        s.step();
        let t = *s.world.get::<Traversal>(e).unwrap();
        let p = s.world.get::<Transform>(hero).unwrap().pos;
        if t.active {
            top = top.max(p[1]);
            assert!(p[0].abs() < 0.05, "straight on: {p:?}");
        }
        if t.exit {
            exit_at = Some((k, p, t));
            break;
        }
    }
    let (_, p, t) = exit_at.expect("the move ends");
    assert!(top > 0.5 && top < 1.1, "it went up and over: the top was {top}");
    assert!(p[2] > 3.1 && p[2] < 5.0, "it ends beyond the wall: {p:?}");
    assert!((p[1] - 0.15).abs() < 0.1, "and on the real floor (the ground behind the wall is 0.15 m higher): {p:?}");
    assert!(t.exit_vel[1] > 0.5, "it leaves with the clip's velocity forward: {:?}", t.exit_vel);
    // the character's body is free again
    s.step();
    let cb = s.world.get::<crate::components::CharacterBody>(hero).unwrap();
    assert_eq!(cb.gravity_scale, 1.0);
    let a = s.world.get::<Animator>(e).unwrap();
    assert!(a.time > 0.0);
    let _ = std::fs::remove_dir_all(dir);
}

/// A mantle onto a block 1 m high: runs up (0.5 s), puts the hands on the ledge (0.7 s, the end of the front window), pulls itself up (to 1.6 s) and stands.
fn mantle_json() -> String {
    let n = 61;
    let mut root = Vec::new();
    for k in 0..n {
        let t = k as f32 / 30.0;
        let (y, z) = if t < 0.5 {
            (0.0, -1.7 + 1.4 * (t / 0.5))
        } else if t < 0.8 {
            let u = (t - 0.5) / 0.3;
            (0.4 * u, -0.3 + 0.05 * u)
        } else if t < 1.6 {
            let u = (t - 0.8) / 0.8;
            (0.4 + 0.6 * u, -0.25 + 0.85 * u)
        } else {
            let u = (t - 1.6) / 0.4;
            (1.0, 0.6 + 0.3 * u)
        };
        root.push(format!("[0,{y:.4},{z:.4},0]"));
    }
    // the push off the wall: 2 m away from it in a second, in an arc 1 m high
    let jump: Vec<String> = (0..=30)
        .map(|k| {
            let t = k as f32 / 30.0;
            format!("[0,{:.4},{:.4},0]", 4.0 * t * (1.0 - t), -2.0 * t)
        })
        .collect();
    format!(
        r#"{{"rate":30,"shimmy":{{"left":"hang_l","right":"hang_r","speed":0.5,"hands_up":0.8,"hands_fwd":0.3,"feet_up":0.4}},"montages":{{"mantle_run":{{"action":3,"kind":"Mantle","height":1.0,"ledge":[0,1.0,0],"gait":"run","length":2.0,"rate":30,"root":[{}],"windows":[{{"target":"front","start":0.5,"end":0.7,"rotation":true,"ignore_z":false}}],"branch_end":0.3,"blend_out":{{"at":1.8,"time":0.3,"wait_input":false}}}},"wall_jump":{{"action":4,"kind":"WallJump","height":0,"gait":"stand","length":1.0,"rate":30,"root":[{}],"windows":[],"branch_end":0.0,"blend_out":{{"at":0.97,"time":0.2,"wait_input":false}}}}}}}}"#,
        root.join(","),
        jump.join(",")
    )
}

/// The block 1 m high and 3 m deep (front face at z = 3), the character 1.7 m before it, running.
fn mantle_scene(tag: &str, wall: bool) -> (Sim, lunaris_ecs::Entity, lunaris_ecs::Entity, std::path::PathBuf) {
    // (the tests of hanging on a ledge hang on this 1 m one: any ledge can be hung on, `hang_min` 0)
    mantle_scene_hang(tag, wall, 0.0)
}

fn mantle_scene_hang(tag: &str, wall: bool, hang_min: f32) -> (Sim, lunaris_ecs::Entity, lunaris_ecs::Entity, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("lunaris_trav_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("traversal.json"), mantle_json()).unwrap();
    let mut s = world(&[(10, [0.0, 0.5, 4.5], [4.0, 1.0, 3.0])], true);
    if wall {
        // a wall standing on the block, 1.2 m from its front edge
        spawn(&mut s, 12, r#"{"Transform":{"pos":[0,2.0,4.35]},"Collider":{"shape":1,"size":[4,2,0.3]}}"#);
    }
    s.asset_root = Some(dir.clone());
    spawn(&mut s, 100, r#"{"Transform":{"pos":[0.0,0.0,1.3]},"CharacterBody":{"radius":0.3,"height":1.76}}"#);
    spawn(&mut s, 101, &format!(r#"{{"Parent":{{"target":100}},"LocalTransform":{{}},"Transform":{{"pos":[0,0,1.3]}},"Animator":{{"clip":"idle"}},"MotionMatcher":{{}},"Traversal":{{"data":"traversal.json","hang_min":{hang_min}}}}}"#));
    for _ in 0..5 {
        s.step();
    }
    let e = s.world.entity(101).unwrap();
    let hero = s.world.entity(100).unwrap();
    s.world.get_mut::<Transform>(hero).unwrap().pos = [0.0, 0.0, 1.3];
    s.world.get_mut::<MotionMatcher>(e).unwrap().vel_now = [0.0, 5.0];
    (s, e, hero, dir)
}

fn ask(s: &mut Sim, e: lunaris_ecs::Entity) -> Traversal {
    s.world.get_mut::<Traversal>(e).unwrap().try_for = 0.5;
    s.step();
    *s.world.get::<Traversal>(e).unwrap()
}


/// A catch: falls (to 0.5 s) to the ledge (3 m up: the root 1.8 m under it, 0.3 m before it), hangs with the hands on it, pulls itself up (to 1.5 s) and stands on the top.
fn catch_json() -> String {
    catch_json_with(|u| 2.6 - 1.4 * u * u)
}

/// The same with the root's height before the catch moment (`u` is how far into the fall, 0 to 1) as given.
fn catch_json_with(fall: impl Fn(f32) -> f32) -> String {
    let n = 61;
    let mut root = Vec::new();
    for k in 0..n {
        let t = k as f32 / 30.0;
        let (y, z) = if t < 0.5 {
            let u = t / 0.5;
            (fall(u), -0.9 + 0.6 * u)
        } else if t < 0.6 {
            (1.2, -0.3)
        } else if t < 1.5 {
            let u = (t - 0.6) / 0.9;
            (1.2 + 1.8 * u, -0.3 + 0.9 * u)
        } else {
            (3.0, 0.6 + 0.3 * (t - 1.5) / 0.5)
        };
        root.push(format!("[0,{y:.4},{z:.4},0]"));
    }
    format!(
        r#"{{"rate":30,"montages":{{"catch_cliff":{{"action":3,"kind":"Mantle","height":1.8,"ledge":[0,3.0,0],"gait":"stand","length":2.0,"rate":30,"root":[{}],"windows":[{{"target":"front","start":0.3,"end":0.5,"rotation":true,"ignore_z":false}}],"branch_end":0.3,"blend_out":{{"at":1.8,"time":0.3,"wait_input":false}},"catch":true,"rel":1.8,"catch_dist":0.3}}}}}}"#,
        root.join(",")
    )
}

/// A platform 3 m high (its face at z = 3), the character falling beside it at `y`, 0.6 m from the face.
fn catch_scene(tag: &str, y: f32) -> (Sim, lunaris_ecs::Entity, lunaris_ecs::Entity, std::path::PathBuf) {
    catch_scene_with(tag, y, catch_json())
}

fn catch_scene_with(tag: &str, y: f32, json: String) -> (Sim, lunaris_ecs::Entity, lunaris_ecs::Entity, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("lunaris_catch_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("traversal.json"), json).unwrap();
    let mut s = world(&[(10, [0.0, 1.5, 4.5], [4.0, 3.0, 3.0])], true);
    s.asset_root = Some(dir.clone());
    spawn(&mut s, 100, &format!(r#"{{"Transform":{{"pos":[0.0,{y},2.4]}},"CharacterBody":{{"radius":0.3,"height":1.76}}}}"#));
    spawn(&mut s, 101, r#"{"Parent":{"target":100},"LocalTransform":{},"Transform":{"pos":[0,0,2.4]},"Animator":{"clip":"fall"},"MotionMatcher":{},"Traversal":{"data":"traversal.json","hang_min":1.8}}"#);
    let e = s.world.entity(101).unwrap();
    let hero = s.world.entity(100).unwrap();
    s.world.get_mut::<MotionMatcher>(e).unwrap().vel_now = [0.0, 1.0];
    (s, e, hero, dir)
}

#[test]
fn a_falling_character_beside_a_ledge_within_reach_catches_it_and_climbs_onto_the_platform() {
    let (mut s, e, hero, dir) = catch_scene("cliff", 1.3);
    s.world.get_mut::<Traversal>(e).unwrap().catch_try = true;
    s.step();
    let t = *s.world.get::<Traversal>(e).unwrap();
    assert!(t.active && t.caught && t.mv == 1, "a catch has begun: {t:?}");
    assert_eq!(clip_name(&s, e), "catch_cliff");
    // (the ledge is 1.7 m above the feet: it is hung on: the clip waits at the catch moment until forward is held)
    for _ in 0..40 {
        s.step();
    }
    let t = *s.world.get::<Traversal>(e).unwrap();
    assert!(t.active && t.hanging, "hanging on the ledge: {t:?}");
    assert!((s.world.get::<Transform>(hero).unwrap().pos[1] - 1.2).abs() < 0.3, "the root is under the ledge: {:?}", s.world.get::<Transform>(hero).unwrap().pos);
    s.world.get_mut::<Traversal>(e).unwrap().hold = true;
    for _ in 0..150 {
        s.step();
    }
    let t = *s.world.get::<Traversal>(e).unwrap();
    let p = s.world.get::<Transform>(hero).unwrap().pos;
    assert!(!t.active && !t.caught, "the move has ended: {t:?}");
    assert!((p[1] - 3.0).abs() < 0.15 && p[2] > 3.0, "on the platform: {p:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_way_into_a_catch_starts_at_the_speed_of_the_fall_it_comes_from_not_at_the_clips_much_faster_one() {
    // the clip falls from 6 m above its catch root, at about 9 m/s when the catch is entered; the character falls at 2.5 m/s
    let (mut s, e, hero, dir) = catch_scene_with("smooth", 1.3, catch_json_with(|u| 1.2 + 6.0 * (1.0 - u) * (1.0 - u)));
    s.world.get_mut::<CharacterBody>(hero).unwrap().fall = -2.5;
    s.world.get_mut::<Traversal>(e).unwrap().catch_try = true;
    s.step();
    assert!(s.world.get::<Traversal>(e).unwrap().caught, "a catch has begun");
    let mut ys = vec![1.3f32, s.world.get::<Transform>(hero).unwrap().pos[1]];
    for _ in 0..16 {
        s.step();
        ys.push(s.world.get::<Transform>(hero).unwrap().pos[1]);
    }
    let v0 = (ys[1] - ys[0]) * 60.0;
    assert!((v0 - -2.5).abs() < 1.2, "the first tick goes at about the fall's speed: {v0} m/s");
    let low = ys.iter().cloned().fold(f32::MAX, f32::min);
    assert!(low > 1.2 - 0.06, "the root does not dive under the catch height: {low}");
    let worst = (1..ys.len() - 1).map(|i| ((ys[i + 1] - 2.0 * ys[i] + ys[i - 1]) * 3600.0).abs()).fold(0.0f32, f32::max);
    assert!(worst < 260.0, "no jump of the speed on the way in: {worst} m/s2");
    let _ = std::fs::remove_dir_all(dir);
}

/// A catch over a thin wall (the clip's frame: the floor under the wall is y = 0, its top 1 m, its face z = 0): falls to the catch moment (0.5 s: the root 0.8 m under the top, 0.2 m before it), pulls itself up (the feet on
/// the top from 0.8 to 0.9 s) and then either puts its feet down on the floor beyond (a hurdle, down at 1.2 s) or swings over and falls on (a vault: the move ends at 1.23 s, when the root has dropped under the top).
fn thin_catch_json(hurdle: bool) -> String {
    let n = 55;
    let mut root = Vec::new();
    for k in 0..n {
        let t = k as f32 / 30.0;
        let (y, z) = if t < 0.5 {
            let u = t / 0.5;
            (0.2 + 3.0 * (1.0 - u) * (1.0 - u), -0.9 + 0.7 * u)
        } else if t < 0.8 {
            let u = (t - 0.5) / 0.3;
            (0.2 + 0.8 * u * u * (3.0 - 2.0 * u), -0.2 + 0.2 * u)
        } else if t < 0.9 {
            (1.0, (t - 0.8) * 3.0)
        } else if hurdle {
            let u = ((t - 0.9) / 0.3).min(1.0);
            (1.0 - u * u * (3.0 - 2.0 * u), 0.3 + 0.7 * u + (t - 1.2).max(0.0))
        } else {
            (1.0 - 6.0 * (t - 0.9) * (t - 0.9), 0.3 + 1.5 * (t - 0.9))
        };
        root.push(format!("[0,{y:.4},{z:.4},0]"));
    }
    let (action, kind, windows, blend) = if hurdle {
        (2, "Hurdle", r#"{"target":"front","start":0.15,"end":0.5,"rotation":true,"ignore_z":false},{"target":"back","start":0.8,"end":0.9,"rotation":false,"ignore_z":false},{"target":"floor","start":0.95,"end":1.2,"rotation":false,"ignore_z":false}"#, 1.3)
    } else {
        (1, "Vault", r#"{"target":"front","start":0.15,"end":0.5,"rotation":true,"ignore_z":false},{"target":"back","start":0.8,"end":0.9,"rotation":false,"ignore_z":false}"#, 1.23)
    };
    format!(
        r#"{{"rate":30,"montages":{{"thin_catch":{{"action":{action},"kind":"{kind}","height":0.8,"ledge":[0,1.0,0],"gait":"stand","length":1.7,"rate":30,"root":[{}],"windows":[{windows}],"branch_end":0.3,"blend_out":{{"at":{blend},"time":0.3,"wait_input":false}},"catch":true,"rel":0.8,"catch_dist":0.2}}}}}}"#,
        root.join(",")
    )
}

/// A thin wall at z = 3 (its face at 2.9), 1 m high, and the character falling beside it with its feet at `y` (0.6 m from the face).
fn thin_scene(tag: &str, obstacles: &[(u64, [f32; 3], [f32; 3])], pos: [f32; 3], hurdle: bool) -> (Sim, lunaris_ecs::Entity, lunaris_ecs::Entity, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("lunaris_thin_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("traversal.json"), thin_catch_json(hurdle)).unwrap();
    let mut s = world(obstacles, true);
    s.asset_root = Some(dir.clone());
    spawn(&mut s, 100, &format!(r#"{{"Transform":{{"pos":[{},{},{}]}},"CharacterBody":{{"radius":0.3,"height":1.76}}}}"#, pos[0], pos[1], pos[2]));
    spawn(&mut s, 101, r#"{"Parent":{"target":100},"LocalTransform":{},"Transform":{"pos":[0,0,2.4]},"Animator":{"clip":"fall"},"MotionMatcher":{},"Traversal":{"data":"traversal.json"}}"#);
    let e = s.world.entity(101).unwrap();
    let hero = s.world.entity(100).unwrap();
    s.world.get_mut::<MotionMatcher>(e).unwrap().vel_now = [0.0, 1.0];
    (s, e, hero, dir)
}

#[test]
fn a_falling_character_beside_a_thin_wall_catches_its_top_and_hurdles_it_onto_the_floor_beyond() {
    let (mut s, e, hero, dir) = thin_scene("hurdle", &[(10, [0.0, 0.5, 3.0], [4.0, 1.0, 0.2])], [0.0, 0.55, 2.3], true);
    s.world.get_mut::<CharacterBody>(hero).unwrap().fall = -2.0;
    s.world.get_mut::<Traversal>(e).unwrap().catch_try = true;
    s.step();
    let t = *s.world.get::<Traversal>(e).unwrap();
    assert!(t.active && t.caught && t.action == HURDLE, "a hurdle catch has begun: {t:?}");
    assert_eq!(clip_name(&s, e), "thin_catch");
    for _ in 0..140 {
        s.step();
    }
    let t = *s.world.get::<Traversal>(e).unwrap();
    let p = s.world.get::<Transform>(hero).unwrap().pos;
    assert!(!t.active && !t.caught, "the move has ended: {t:?}");
    assert!(p[2] > 3.2 && p[1].abs() < 0.12, "on the floor beyond the wall: {p:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_falling_character_beside_a_wall_with_nothing_to_land_on_beyond_it_catches_the_top_and_vaults_it() {
    // the wall (1 m high) stands at the far end of a platform 3 m high: beyond it the floor is 4 m down
    let (mut s, e, hero, dir) = thin_scene("vault", &[(10, [0.0, 1.5, 4.5], [4.0, 3.0, 3.0]), (11, [0.0, 3.5, 5.9], [4.0, 1.0, 0.2])], [0.0, 3.55, 5.2], false);
    s.world.get_mut::<CharacterBody>(hero).unwrap().fall = -2.0;
    s.world.get_mut::<Traversal>(e).unwrap().catch_try = true;
    s.step();
    let t = *s.world.get::<Traversal>(e).unwrap();
    assert!(t.active && t.caught && t.action == VAULT, "a vault catch has begun: {t:?}");
    let mut over = false;
    for _ in 0..90 {
        s.step();
        let p = s.world.get::<Transform>(hero).unwrap().pos;
        over |= p[2] > 6.0 && p[1] > 0.5;
    }
    let t = *s.world.get::<Traversal>(e).unwrap();
    assert!(!t.active, "the move has ended: {t:?}");
    assert!(over, "it went over the wall and was still up in the air beyond the platform");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_low_wall_with_the_floor_right_under_it_is_not_caught() {
    // (0.4 m high, the feet at its level: nothing to grab, the character walks on)
    let (mut s, e, hero, dir) = thin_scene("lowwall", &[(10, [0.0, 0.2, 3.0], [4.0, 0.4, 0.2])], [0.0, 0.55, 2.3], true);
    s.world.get_mut::<CharacterBody>(hero).unwrap().fall = -2.0;
    s.world.get_mut::<Traversal>(e).unwrap().catch_try = true;
    s.step();
    assert!(!s.world.get::<Traversal>(e).unwrap().active, "no catch");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_catch_of_a_low_ledge_can_be_let_go_of_too_while_the_hands_are_on_it() {
    // (a ledge that is climbed out in full and never hung on, `catch_hang` out of reach: let go of when asked right after the grab)
    let (mut s, e, hero, dir) = catch_scene("letgo", 1.3);
    s.world.get_mut::<CharacterBody>(hero).unwrap().fall = -1.0;
    s.world.get_mut::<Traversal>(e).unwrap().catch_hang = 5.0;
    s.world.get_mut::<Traversal>(e).unwrap().catch_try = true;
    s.world.get_mut::<Traversal>(e).unwrap().drop_tags = 1 << 13;
    s.step();
    assert!(s.world.get::<Traversal>(e).unwrap().caught);
    for _ in 0..14 {
        s.step();
    }
    assert!(s.world.get::<Traversal>(e).unwrap().caught, "still on the ledge");
    s.world.get_mut::<Traversal>(e).unwrap().cancel = true;
    s.step();
    let t = *s.world.get::<Traversal>(e).unwrap();
    assert!(t.exit && t.dropped && !t.active && !t.caught, "let go: {t:?}");
    assert_eq!(s.world.get::<MotionMatcher>(e).unwrap().tags, 1 << 13, "into the fall");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_catch_looks_where_the_keys_point_not_where_the_lagging_velocity_goes() {
    // (a character that has just let go of a ledge drifts back from it while the keys point at the ledge: it catches; with the keys pointing away it does not)
    let (mut s, e, hero, dir) = catch_scene("wish", 1.3);
    s.world.get_mut::<CharacterBody>(hero).unwrap().fall = -1.0;
    {
        let m = s.world.get_mut::<MotionMatcher>(e).unwrap();
        m.vel_now = [0.0, -0.8];
        m.vel = [0.0, 1.8];
    }
    s.world.get_mut::<Traversal>(e).unwrap().catch_try = true;
    s.step();
    assert!(s.world.get::<Traversal>(e).unwrap().caught, "the keys point at the ledge");
    let _ = std::fs::remove_dir_all(dir);
    let (mut s, e, hero, dir) = catch_scene("wish2", 1.3);
    s.world.get_mut::<CharacterBody>(hero).unwrap().fall = -1.0;
    {
        let m = s.world.get_mut::<MotionMatcher>(e).unwrap();
        m.vel_now = [0.0, 1.0];
        m.vel = [0.0, -1.8];
    }
    s.world.get_mut::<Traversal>(e).unwrap().catch_try = true;
    s.step();
    assert!(!s.world.get::<Traversal>(e).unwrap().active, "the keys point away from the ledge");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn no_catch_when_the_ledge_is_out_of_reach_the_character_is_on_the_ground_or_nobody_asks() {
    // the top is 2.6 m above the feet: out of a hand's reach
    let (mut s, e, _hero, dir) = catch_scene("high", 0.4);
    s.world.get_mut::<Traversal>(e).unwrap().catch_try = true;
    s.step();
    assert!(!s.world.get::<Traversal>(e).unwrap().active, "too high to catch");
    let _ = std::fs::remove_dir_all(dir);
    // nobody asks
    let (mut s, e, _hero, dir) = catch_scene("quiet", 1.3);
    s.step();
    assert!(!s.world.get::<Traversal>(e).unwrap().active);
    let _ = std::fs::remove_dir_all(dir);
    // on the ground (the character stands where it fell to: no catch though the ledge is near)
    let (mut s, e, hero, dir) = catch_scene("ground", 0.0);
    for _ in 0..10 {
        s.step();
    }
    assert!(s.world.get::<CharacterBody>(hero).unwrap().grounded);
    s.world.get_mut::<Traversal>(e).unwrap().catch_try = true;
    s.step();
    assert!(!s.world.get::<Traversal>(e).unwrap().active, "no catch on the ground");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_catch_move_is_never_chosen_by_the_check_on_the_ground() {
    // the file has only a catch: Space at a 3 m block asks for a climb and there is none
    let dir = std::env::temp_dir().join(format!("lunaris_catch_only_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("traversal.json"), catch_json()).unwrap();
    let mut s = world(&[(10, [0.0, 1.0, 4.5], [4.0, 2.0, 3.0])], true);
    s.asset_root = Some(dir.clone());
    spawn(&mut s, 100, r#"{"Transform":{"pos":[0.0,0.0,2.4]},"CharacterBody":{"radius":0.3,"height":1.76}}"#);
    spawn(&mut s, 101, r#"{"Parent":{"target":100},"LocalTransform":{},"Transform":{"pos":[0,0,2.4]},"Animator":{"clip":"idle"},"MotionMatcher":{},"Traversal":{"data":"traversal.json"}}"#);
    for _ in 0..5 {
        s.step();
    }
    let e = s.world.entity(101).unwrap();
    let t = ask(&mut s, e);
    assert!(!t.active && t.failed, "no ground move to climb a 2 m block: {t:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_low_ledge_is_climbed_out_in_full_whatever_is_held_only_a_tall_one_is_hung_on() {
    // the default `hang_min` (1.8 m): the 1 m block of this scene is a ledge to climb out in full, not to hang on, and letting go (S, C) does nothing on it
    let (mut s, e, _hero, dir) = mantle_scene_hang("low", false, 1.8);
    let t = ask(&mut s, e);
    assert!(t.active && t.action == MANTLE, "{t:?}");
    assert!(!t.hang_ok, "a 1 m ledge is not one to hang on: {t:?}");
    let mut left = None;
    for k in 0..300 {
        s.step();
        if k == 30 {
            s.world.get_mut::<Traversal>(e).unwrap().cancel = true;
        }
        let t = *s.world.get::<Traversal>(e).unwrap();
        assert!(!t.hanging, "it never waits on a low ledge (tick {k}): {t:?}");
        if t.exit {
            left = Some(t);
            break;
        }
    }
    let t = left.expect("the move plays out and ends");
    assert!(!t.dropped, "letting go does nothing on a low ledge: {t:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_mantle_waits_with_the_hands_on_the_ledge_until_forward_is_held() {
    let (mut s, e, hero, dir) = mantle_scene("hang", false);
    let t = ask(&mut s, e);
    assert!(t.active && t.action == MANTLE, "{t:?}");
    let mut hung = None;
    for k in 0..200 {
        s.step();
        let t = *s.world.get::<Traversal>(e).unwrap();
        assert!(!t.exit, "it must not leave before the hands are on the ledge (tick {k})");
        if t.hanging {
            hung = Some(t);
            break;
        }
    }
    let _ = hung.expect("with forward not held the character hangs on the ledge");
    // (the clip eases to a stop: it does not freeze from one picture to the next)
    for _ in 0..12 {
        s.step();
    }
    let t = *s.world.get::<Traversal>(e).unwrap();
    let p = s.world.get::<Transform>(hero).unwrap().pos;
    assert!(p[1] < 0.7 && p[2] > 2.5 && p[2] < 3.0, "hanging below the ledge, in front of it: {p:?}");
    assert_eq!(s.world.get::<Animator>(e).unwrap().speed, 0.0, "the pose is held");
    // a while later it is still there, the clip has not moved
    for _ in 0..90 {
        s.step();
    }
    let t2 = *s.world.get::<Traversal>(e).unwrap();
    let p2 = s.world.get::<Transform>(hero).unwrap().pos;
    assert!(t2.active && t2.hanging && !t2.exit, "{t2:?}");
    assert!((t2.time - t.time).abs() < 0.03 && (p2[1] - p[1]).abs() < 0.02 && (p2[2] - p[2]).abs() < 0.02, "held: {p:?} -> {p2:?}, {} -> {}", t.time, t2.time);
    // forward again: it pulls itself over and stands on the block, leaving a little before the end of the clip
    s.world.get_mut::<Traversal>(e).unwrap().hold = true;
    s.world.get_mut::<Traversal>(e).unwrap().move_input = true;
    let mut out = None;
    for _ in 0..200 {
        s.step();
        let t = *s.world.get::<Traversal>(e).unwrap();
        if t.exit {
            out = Some(t);
            break;
        }
    }
    let t = out.expect("it climbs over once forward is held");
    let p = s.world.get::<Transform>(hero).unwrap().pos;
    assert!(!t.dropped && (p[1] - 1.0).abs() < 0.15 && p[2] > 3.4, "standing on the block: {p:?}");
    assert!(t.time < 1.78, "it leaves before the clip's end ({}), so that the player moves on sooner", t.time);
    assert_eq!(s.world.get::<Animator>(e).unwrap().speed, 1.0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn holding_forward_all_the_way_climbs_without_stopping_and_leaves_early() {
    let (mut s, e, hero, dir) = mantle_scene("go", false);
    s.world.get_mut::<Traversal>(e).unwrap().hold = true;
    s.world.get_mut::<Traversal>(e).unwrap().move_input = true;
    let t = ask(&mut s, e);
    assert!(t.active, "{t:?}");
    let mut out = None;
    for _ in 0..200 {
        s.step();
        let t = *s.world.get::<Traversal>(e).unwrap();
        assert!(!t.hanging, "forward is held: no stop");
        if t.exit {
            out = Some(t);
            break;
        }
    }
    let t = out.expect("it ends");
    let p = s.world.get::<Transform>(hero).unwrap().pos;
    assert!(t.time < 1.78 && (p[1] - 1.0).abs() < 0.15, "{} {p:?}", t.time);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_mantle_can_be_let_go_of_and_the_character_drops_back_from_the_wall() {
    let (mut s, e, hero, dir) = mantle_scene("cancel", false);
    ask(&mut s, e);
    for _ in 0..200 {
        s.step();
        if s.world.get::<Traversal>(e).unwrap().hanging {
            break;
        }
    }
    assert!(s.world.get::<Traversal>(e).unwrap().hanging);
    s.world.get_mut::<Traversal>(e).unwrap().cancel = true;
    s.world.get_mut::<Traversal>(e).unwrap().drop_tags = 1 << 13;
    s.step();
    let t = *s.world.get::<Traversal>(e).unwrap();
    assert!(t.exit && t.dropped && !t.active, "{t:?}");
    assert_eq!(s.world.get::<MotionMatcher>(e).unwrap().tags, 1 << 13, "the matcher's first choice after letting go is a fall");
    assert!(t.exit_vel[1] < 0.0, "pushed away from the wall: {:?}", t.exit_vel);
    let p = s.world.get::<Transform>(hero).unwrap().pos;
    assert!(p[2] < 3.0, "not on the block: {p:?}");
    s.step();
    assert_eq!(s.world.get::<crate::components::CharacterBody>(hero).unwrap().gravity_scale, 1.0, "the body is free again");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_wall_on_the_block_stops_the_climb_from_taking_the_character_into_it() {
    // the room: the wall's face is 1.2 m from the front edge, the body 0.3 m wide, and a margin
    let mut s = world(&[(10, [0.0, 0.5, 4.5], [4.0, 1.0, 3.0])], true);
    spawn(&mut s, 12, r#"{"Transform":{"pos":[0,2.0,4.35]},"Collider":{"shape":1,"size":[4,2,0.3]}}"#);
    let c = check(&mut s, [0.0, 0.0, 1.3], [0.0, 1.0], 3.0).expect("the block");
    assert_eq!(c.action, MANTLE, "{c:?}");
    assert!(c.ahead > 0.4 && c.ahead < 0.9, "the room on the block: {}", c.ahead);
    // too little room to stand: not a mantle at all
    let mut s = world(&[(10, [0.0, 0.5, 4.5], [4.0, 1.0, 3.0])], true);
    spawn(&mut s, 12, r#"{"Transform":{"pos":[0,2.0,3.5]},"Collider":{"shape":1,"size":[4,2,0.3]}}"#);
    assert_eq!(check(&mut s, [0.0, 0.0, 1.3], [0.0, 1.0], 3.0).map(|c| c.action), Some(0));
    // the move: the root goes no further than the room
    let (mut s, e, hero, dir) = mantle_scene("wall", true);
    s.world.get_mut::<Traversal>(e).unwrap().hold = true;
    let t = ask(&mut s, e);
    assert!(t.active, "{t:?}");
    let mut far = 0.0f32;
    for _ in 0..200 {
        s.step();
        far = far.max(s.world.get::<Transform>(hero).unwrap().pos[2]);
        if s.world.get::<Traversal>(e).unwrap().exit {
            break;
        }
    }
    assert!(far < 3.0 + 0.9, "stopped short of the wall (its face is at 4.2): {far}");
    assert!(far > 3.2, "but it did go over the ledge: {far}");
    let _ = std::fs::remove_dir_all(dir);
}

/// Turns the model (and so the way the character faces) a number of degrees about y, and lets the transforms settle.
fn turn_model(s: &mut Sim, e: lunaris_ecs::Entity, degrees: f32) {
    let half = degrees.to_radians() * 0.5;
    s.world.get_mut::<crate::components::LocalTransform>(e).unwrap().rot = [0.0, half.sin(), 0.0, half.cos()];
    s.step();
}

#[test]
fn an_obstacle_that_is_not_faced_is_not_climbed_when_the_game_asks_to_face_it() {
    // (the character is 0.6 m from a 1 m block and faces it, or 55 degrees off its face: with `face_min` 0 both climb, with 0.65 only the one that faces it)
    for (face_min, degrees, climbs) in [(0.0f32, 0.0f32, true), (0.0, 55.0, true), (0.65, 0.0, true), (0.65, 55.0, false)] {
        let (mut s, e, hero, dir) = mantle_scene("faced", false);
        s.world.get_mut::<Transform>(hero).unwrap().pos = [0.0, 0.0, 2.4];
        s.world.get_mut::<Traversal>(e).unwrap().face_min = face_min;
        turn_model(&mut s, e, degrees);
        let t = ask(&mut s, e);
        assert_eq!(t.active, climbs, "face_min {face_min}, {degrees} degrees off: {t:?}");
        if !climbs {
            assert!(t.failed, "the answer is that there is nothing to climb (the game jumps): {t:?}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn a_ledge_that_is_not_faced_is_not_caught_and_one_the_keys_leave_is_not_either() {
    // (the keys point at the ledge in every case; the model faces it, or a quarter turn off it. A game whose character faces where the camera looks sets `face_min`)
    for (face_min, degrees, wish_z, catches) in [(0.0f32, 90.0f32, 1.8f32, true), (0.65, 0.0, 1.8, true), (0.65, 90.0, 1.8, false), (0.65, 0.0, -1.8, false)] {
        let (mut s, e, hero, dir) = catch_scene("faced", 1.3);
        s.world.get_mut::<CharacterBody>(hero).unwrap().fall = -1.0;
        {
            let m = s.world.get_mut::<MotionMatcher>(e).unwrap();
            m.vel_now = [0.0, 1.0];
            m.vel = [0.0, wish_z];
        }
        {
            let w = s.world.get_mut::<Traversal>(e).unwrap();
            w.face_min = face_min;
        }
        turn_model(&mut s, e, degrees);
        s.world.get_mut::<CharacterBody>(hero).unwrap().fall = -1.0;
        s.world.get_mut::<Traversal>(e).unwrap().catch_try = true;
        s.step();
        assert_eq!(s.world.get::<Traversal>(e).unwrap().caught, catches, "face_min {face_min}, {degrees} degrees, keys {wish_z}");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn space_pressed_far_from_the_obstacle_does_not_climb_it() {
    let (mut s, e, hero, dir) = mantle_scene("far", false);
    // 2.9 m from the ledge, running; the reach is set to 2.2 m at most
    s.world.get_mut::<Transform>(hero).unwrap().pos = [0.0, 0.0, 0.1];
    {
        let w = s.world.get_mut::<Traversal>(e).unwrap();
        w.reach_max = 2.2;
    }
    let t = ask(&mut s, e);
    assert!(!t.active && t.failed, "too far: the check says there is nothing (so the game jumps): {t:?}");
    // closer (1.7 m): it starts
    s.world.get_mut::<Transform>(hero).unwrap().pos = [0.0, 0.0, 1.3];
    let t = ask(&mut s, e);
    assert!(t.active, "{t:?}");
    let _ = std::fs::remove_dir_all(dir);
}

fn clip_name(s: &Sim, e: lunaris_ecs::Entity) -> String {
    let c = s.world.get::<Animator>(e).unwrap().clip;
    s.world.strings().get(c.0).to_string()
}

/// Run to the hands on the ledge (forward not held) and a little longer, until the clip has eased to a stop.
fn to_hang(s: &mut Sim, e: lunaris_ecs::Entity) {
    let t = ask(s, e);
    assert!(t.active && t.action == MANTLE, "{t:?}");
    for _ in 0..200 {
        s.step();
        if s.world.get::<Traversal>(e).unwrap().hanging {
            break;
        }
    }
    for _ in 0..12 {
        s.step();
    }
    assert!(s.world.get::<Traversal>(e).unwrap().hanging);
}

#[test]
fn space_while_hanging_pushes_the_character_off_the_wall_up_and_away() {
    let (mut s, e, hero, dir) = mantle_scene("push", false);
    to_hang(&mut s, e);
    let z0 = s.world.get::<Transform>(hero).unwrap().pos[2];
    s.world.get_mut::<Traversal>(e).unwrap().jump = true;
    s.step();
    let t = *s.world.get::<Traversal>(e).unwrap();
    assert!(t.active && !t.jump && t.mv == 2, "the wall jump is the move now: {t:?}");
    assert_eq!(clip_name(&s, e), "wall_jump");
    let (mut top, mut out) = (0.0f32, None);
    for _ in 0..80 {
        s.step();
        let p = s.world.get::<Transform>(hero).unwrap().pos;
        top = top.max(p[1]);
        let t = *s.world.get::<Traversal>(e).unwrap();
        if t.exit {
            out = Some((t, p));
            break;
        }
    }
    let (t, p) = out.expect("the push ends");
    assert!(t.pushed && !t.dropped, "{t:?}");
    assert!(p[2] < z0 - 1.5, "away from the wall: {z0} -> {p:?}");
    assert!(top > 0.25 + 0.5, "up: the top was {top}");
    assert!(t.exit_vy >= 2.39 && t.exit_vel[1] <= -4.49, "it leaves with momentum, going away and up: {:?} {}", t.exit_vel, t.exit_vy);
    s.step();
    let cb = s.world.get::<crate::components::CharacterBody>(hero).unwrap();
    assert_eq!(cb.gravity_scale, 1.0);
    assert!(cb.fall > 0.0 || cb.grounded, "it goes up with the push (or has landed: here the ledge is at the ground): {}", cb.fall);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_space_that_asked_for_the_move_does_not_push_off_at_once() {
    let (mut s, e, _hero, dir) = mantle_scene("push2", false);
    s.world.get_mut::<Traversal>(e).unwrap().jump = true;
    let t = ask(&mut s, e);
    assert!(t.active && t.mv == 1, "{t:?}");
    s.world.get_mut::<Traversal>(e).unwrap().jump = true;
    s.step();
    assert_eq!(s.world.get::<Traversal>(e).unwrap().mv, 1, "too soon after the start");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_sideways_keys_shimmy_along_the_ledge_while_hanging_and_the_climb_goes_on_from_there() {
    let (mut s, e, hero, dir) = mantle_scene("shim", false);
    to_hang(&mut s, e);
    let p0 = s.world.get::<Transform>(hero).unwrap().pos;
    // A: to the left (the character faces +z: its left is +x)
    s.world.get_mut::<Traversal>(e).unwrap().side = -1.0;
    for _ in 0..60 {
        s.step();
    }
    let t = *s.world.get::<Traversal>(e).unwrap();
    let p = s.world.get::<Transform>(hero).unwrap().pos;
    assert!(t.pose == 1 && t.hanging && t.shim > 0.35 && t.shim < 0.6, "{t:?}");
    assert_eq!(clip_name(&s, e), "hang_l");
    assert!(p[0] > p0[0] + 0.3 && (p[2] - p0[2]).abs() < 0.3, "along the ledge: {p0:?} -> {p:?}");
    // D: back to the right
    s.world.get_mut::<Traversal>(e).unwrap().side = 1.0;
    for _ in 0..30 {
        s.step();
    }
    let t2 = *s.world.get::<Traversal>(e).unwrap();
    assert!(t2.shim < t.shim - 0.15, "{} -> {}", t.shim, t2.shim);
    assert_eq!(clip_name(&s, e), "hang_r");
    // let go of the key: it stays where it is, still hanging
    s.world.get_mut::<Traversal>(e).unwrap().side = 0.0;
    for _ in 0..90 {
        s.step();
    }
    let t3 = *s.world.get::<Traversal>(e).unwrap();
    assert!(t3.active && t3.hanging && t3.pose == 1 && (t3.shim - t2.shim).abs() < 0.01, "{t3:?}");
    // forward: the climb goes on from the new place
    s.world.get_mut::<Traversal>(e).unwrap().hold = true;
    s.world.get_mut::<Traversal>(e).unwrap().move_input = true;
    let mut out = None;
    for _ in 0..200 {
        s.step();
        let t = *s.world.get::<Traversal>(e).unwrap();
        if t.exit {
            out = Some(t);
            break;
        }
    }
    let t4 = out.expect("it climbs over");
    let p = s.world.get::<Transform>(hero).unwrap().pos;
    assert!((p[0] - t3.shim).abs() < 0.1 && (p[1] - 1.0).abs() < 0.15 && p[2] > 3.4, "up on the block, where it was shifted to: {p:?} (shim {})", t3.shim);
    assert!(!t4.dropped);
    assert_eq!(s.world.get::<crate::components::CharacterBody>(hero).unwrap().gravity_scale, 1.0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_shimmy_stops_at_the_end_of_the_ledge() {
    let (mut s, e, hero, dir) = mantle_scene("shimend", false);
    to_hang(&mut s, e);
    s.world.get_mut::<Traversal>(e).unwrap().side = -1.0;
    for _ in 0..600 {
        s.step();
    }
    let t = *s.world.get::<Traversal>(e).unwrap();
    // the block is 4 m wide, the hands keep 0.2 m of margin and 0.3 m more from the end
    assert!((t.shim - t.shim_hi).abs() < 1e-3 && t.shim_hi > 1.0 && t.shim_hi < 1.6, "{t:?}");
    assert!(s.world.get::<Transform>(hero).unwrap().pos[0] < 1.6);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn sideways_keys_do_nothing_while_the_climb_goes_on() {
    // with forward held there is no hanging: A and D change nothing
    let (mut s, e, hero, dir) = mantle_scene("noside", false);
    s.world.get_mut::<Traversal>(e).unwrap().hold = true;
    s.world.get_mut::<Traversal>(e).unwrap().side = 1.0;
    let t = ask(&mut s, e);
    assert!(t.active);
    let mut max_x = 0.0f32;
    for _ in 0..200 {
        s.step();
        max_x = max_x.max(s.world.get::<Transform>(hero).unwrap().pos[0].abs());
        let t = *s.world.get::<Traversal>(e).unwrap();
        assert!(!t.hanging && t.pose == 0);
        if t.exit {
            break;
        }
    }
    assert!(max_x < 0.02, "the character did not move sideways: {max_x}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn once_above_the_ledge_the_climb_finishes_by_itself_and_quickly() {
    // forward is held only until the hands are on the ledge: after the pull-up has got the root over the top nothing more is needed
    let (mut s, e, hero, dir) = mantle_scene("above", false);
    to_hang(&mut s, e);
    s.world.get_mut::<Traversal>(e).unwrap().hold = true;
    let mut above_at = None;
    let mut out_at = None;
    for k in 0..200 {
        s.step();
        let t = *s.world.get::<Traversal>(e).unwrap();
        if t.above && above_at.is_none() {
            above_at = Some(k);
            s.world.get_mut::<Traversal>(e).unwrap().hold = false; // let go of forward: it is above now, it goes on
        }
        if t.exit {
            out_at = Some(k);
            break;
        }
    }
    let (a, o) = (above_at.expect("it gets above the ledge"), out_at.expect("and finishes by itself"));
    assert!(o - a <= 20, "{} ticks from above to the end", o - a);
    assert!((s.world.get::<Transform>(hero).unwrap().pos[1] - 1.0).abs() < 0.15);
    let _ = std::fs::remove_dir_all(dir);
}

fn goal(s: &Sim, uid: u64) -> (f32, [f32; 3], [f32; 3], bool) {
    let e = s.world.entity(uid).unwrap();
    let g = s.world.get::<crate::components::Ik>(e).unwrap();
    (g.weight, s.world.get::<Transform>(e).unwrap().pos, g.axis, g.one_sided)
}

fn ik_scene(tag: &str) -> (Sim, lunaris_ecs::Entity, lunaris_ecs::Entity, std::path::PathBuf) {
    let (mut s, e, hero, dir) = mantle_scene(tag, false);
    for (uid, joint) in [(201u64, "hand_l"), (202, "hand_r"), (203, "foot_l"), (204, "foot_r")] {
        spawn(&mut s, uid, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Ik":{{"skin":101,"joint":"{joint}","kind":2,"weight":0}}}}"#));
    }
    {
        let t = s.world.get_mut::<Traversal>(e).unwrap();
        t.ik_hand_l = lunaris_ecs::Uid(201);
        t.ik_hand_r = lunaris_ecs::Uid(202);
        t.ik_foot_l = lunaris_ecs::Uid(203);
        t.ik_foot_r = lunaris_ecs::Uid(204);
    }
    (s, e, hero, dir)
}

#[test]
fn the_hands_are_pinned_to_the_ledge_and_the_feet_to_the_wall_while_hanging_and_let_go_after() {
    let (mut s, e, _hero, dir) = ik_scene("ik");
    to_hang(&mut s, e);
    for _ in 0..20 {
        s.step();
    }
    // the front edge is at z = 3, y = 1: the hands' goal is a little behind the edge, just over the top; they are held along up and the wall's direction
    let (w, p, axis, one) = goal(&s, 201);
    assert!(w > 0.95 && (p[1] - 1.035).abs() < 1e-3 && (p[2] - 3.06).abs() < 1e-3 && axis == [0.0, 1.0, 0.0] && !one, "{w} {p:?} {axis:?}");
    let e1 = s.world.entity(201).unwrap();
    let g = s.world.get::<crate::components::Ik>(e1).unwrap();
    assert!(g.kind == 2 && (g.axis2[2] - 1.0).abs() < 1e-5, "{g:?}");
    // the feet of the climb clip's hanging pose are only kept out of the wall: one sided, the direction out of it
    let (wf, pf, axf, onef) = goal(&s, 203);
    assert!(wf > 0.95 && onef && (axf[2] + 1.0).abs() < 1e-5 && (pf[2] - 2.925).abs() < 1e-3, "{wf} {pf:?} {axf:?} {onef}");
    // in the shimmy's pose they are braced against the wall: on its plane, both sides
    s.world.get_mut::<Traversal>(e).unwrap().side = -1.0;
    for _ in 0..30 {
        s.step();
    }
    let (wf, _, axf, onef) = goal(&s, 204);
    assert!(wf > 0.95 && !onef && (axf[2] - 1.0).abs() < 1e-5, "{wf} {axf:?} {onef}");
    // the hands go along with the shimmy
    let (_, p, _, _) = goal(&s, 202);
    assert!(p[0] > 0.1, "{p:?}");
    // climbing on: the goals ease out, at the end they are off
    {
        let t = s.world.get_mut::<Traversal>(e).unwrap();
        t.side = 0.0;
        t.hold = true;
        t.move_input = true;
    }
    for _ in 0..200 {
        s.step();
        if s.world.get::<Traversal>(e).unwrap().exit {
            break;
        }
    }
    for uid in [201u64, 202, 203, 204] {
        assert_eq!(goal(&s, uid).0, 0.0, "goal {uid} is off after the move");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_shimmy_is_only_where_the_character_stopped_on_the_ledge_not_halfway_up() {
    let (mut s, e, hero, dir) = mantle_scene("midway", false);
    {
        let t = s.world.get_mut::<Traversal>(e).unwrap();
        t.hold = true;
    }
    let t = ask(&mut s, e);
    assert!(t.active);
    // pulling up: well past the hang, not above yet
    for _ in 0..200 {
        s.step();
        if s.world.get::<Animator>(e).unwrap().time > 1.1 {
            break;
        }
    }
    let x0 = s.world.get::<Transform>(hero).unwrap().pos[0];
    {
        let t = s.world.get_mut::<Traversal>(e).unwrap();
        t.hold = false;
        t.side = -1.0;
    }
    for _ in 0..40 {
        s.step();
    }
    let t = *s.world.get::<Traversal>(e).unwrap();
    assert!(t.hanging && t.pose == 0 && t.shim == 0.0, "paused halfway up, no shimmy: {t:?}");
    assert!((s.world.get::<Transform>(hero).unwrap().pos[0] - x0).abs() < 1e-3);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_wall_jump_is_quick_and_leaves_with_the_speed_of_its_playback() {
    let (mut s, e, hero, dir) = mantle_scene("fast", false);
    to_hang(&mut s, e);
    s.world.get_mut::<Traversal>(e).unwrap().jump = true;
    s.step();
    assert!(s.world.get::<Animator>(e).unwrap().speed > 1.5);
    let mut ticks = 0;
    let mut out = None;
    for k in 0..120 {
        s.step();
        ticks = k;
        let t = *s.world.get::<Traversal>(e).unwrap();
        if t.exit {
            out = Some(t);
            break;
        }
    }
    let t = out.expect("it ends");
    // the clip is a second long in the test data and plays 1.7 times as fast
    assert!(ticks <= 40, "{ticks} ticks");
    assert!(t.exit_vel[1] < -3.0, "away from the wall at the least speed or the path's: {:?}", t.exit_vel);
    let _ = hero;
    let _ = std::fs::remove_dir_all(dir);
}
