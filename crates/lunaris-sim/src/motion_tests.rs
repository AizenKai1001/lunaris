//! Swimming, wading, rolling, ladders and ledges (`CharacterMotion`, `Climbable`): what a character does in a lake, on a roll, on a ladder and at a ledge.
use super::*;
use crate::png::tests::make_png;
use components::{CharacterBody, CharacterMotion, Transform};
use lunaris_json::{parse, Json};
use std::path::PathBuf;

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn body(s: &Sim, uid: u64) -> CharacterBody {
    *s.world.get::<CharacterBody>(s.world.entity(uid).unwrap()).unwrap()
}

fn motion(s: &Sim, uid: u64) -> CharacterMotion {
    *s.world.get::<CharacterMotion>(s.world.entity(uid).unwrap()).unwrap()
}

fn set_body(s: &mut Sim, uid: u64, f: impl FnOnce(&mut CharacterBody)) {
    let e = s.world.entity(uid).unwrap();
    f(s.world.get_mut::<CharacterBody>(e).unwrap());
}

fn set_motion(s: &mut Sim, uid: u64, f: impl FnOnce(&mut CharacterMotion)) {
    let e = s.world.entity(uid).unwrap();
    f(s.world.get_mut::<CharacterMotion>(e).unwrap());
}

/// A floor whose top is at y = 0, and a hero with a `CharacterMotion` standing on it at the origin.
fn flat() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0.5,0]},"CharacterBody":{},"CharacterMotion":{}}"#);
    s.step_n(30);
    s
}

// ---------------------------------------------------------------------------------------------- water

/// Distance of sample (x, z) of a 33 x 33 picture from its middle, in samples.
fn radius(x: usize, z: usize) -> f32 {
    ((x as f32 - 16.0).powi(2) + (z as f32 - 16.0).powi(2)).sqrt()
}

/// A bowl: 0 (black) in the middle, rising to 0.6 of the relief at the rim and beyond.
fn bowl(x: usize, z: usize) -> u16 {
    (0.6 * (radius(x, z) / 16.0).min(1.0) * 65535.0) as u16
}

/// A lake in the bowl: water at 0.3 of the relief (3 m of 10) within 8 samples (16 m) of the middle, dry elsewhere.
fn lake(x: usize, z: usize) -> u16 {
    if radius(x, z) <= 8.0 {
        (0.3 * 65535.0) as u16
    } else {
        0
    }
}

fn workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lunaris-motion-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, f) in [("bowl.png", &bowl as &dyn Fn(usize, usize) -> u16), ("lake.png", &lake)] {
        let samples: Vec<u16> = (0..33 * 33).map(|i| f(i % 33, i / 33)).collect();
        std::fs::write(dir.join(file), make_png(33, 33, 16, 0, 4, &samples)).unwrap();
    }
    dir
}

/// 64 x 64 metres of bowl (the lake's surface at 3 m, 3 m deep in the middle, the bank at 16 m from the middle where the bed is also 3 m).
fn lake_world(name: &str) -> Sim {
    let mut s = Sim::new(1);
    s.threads = 1;
    s.asset_root = Some(workspace(name));
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Terrain":{"heightmap":"bowl.png","size":[64,64],"height":10}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"WaterBody":{"heightmap":"lake.png","size":[64,64],"relief":10,"speed":0}}"#);
    s
}

#[test]
fn a_character_that_falls_into_a_lake_swims_and_floats_with_its_shoulders_at_the_surface() {
    let mut s = lake_world("float");
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,6,0]},"CharacterBody":{},"CharacterMotion":{}}"#);
    s.step_n(240);
    let m = motion(&s, 10);
    assert!(m.swimming && !m.wading, "{m:?}");
    // the feet rest `float_depth` (1.35 m) under the surface (3 m)
    assert!((pos(&s, 10)[1] - 1.65).abs() < 0.1, "{:?}", pos(&s, 10));
    assert!(!m.under, "the head is out of the water: {m:?}");
    assert!((m.depth - 1.35).abs() < 0.1, "{m:?}");
    // and it stays there
    let before = pos(&s, 10)[1];
    s.step_n(120);
    assert!((pos(&s, 10)[1] - before).abs() < 0.02, "{} vs {before}", pos(&s, 10)[1]);
}

#[test]
fn diving_goes_down_at_dive_speed_and_rising_stops_at_the_float_level() {
    let mut s = lake_world("dive");
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,6,0]},"CharacterBody":{},"CharacterMotion":{}}"#);
    s.step_n(240);
    let level = pos(&s, 10)[1];
    set_motion(&mut s, 10, |m| m.vertical = -1.0);
    s.step_n(30);
    let dived = pos(&s, 10)[1];
    assert!(level - dived > 0.7 && level - dived < 1.2, "half a second at 2 m/s: {level} -> {dived}");
    assert!(motion(&s, 10).under || dived < level, "{:?}", motion(&s, 10));
    set_motion(&mut s, 10, |m| m.vertical = 1.0);
    s.step_n(120);
    assert!((pos(&s, 10)[1] - level).abs() < 0.05, "back at the float level, not above it: {} vs {level}", pos(&s, 10)[1]);
    // the bed is at 0: diving for long rests on it and does not go through
    set_motion(&mut s, 10, |m| m.vertical = -1.0);
    s.step_n(300);
    assert!(pos(&s, 10)[1] > -0.05, "{:?}", pos(&s, 10));
}

#[test]
fn a_swimmer_is_no_faster_than_swim_speed_and_a_waders_walk_is_slowed_in_proportion_to_the_depth() {
    let mut s = lake_world("speeds");
    // two characters at the same distance from the middle (the bowl is round), one with the motion and one without
    spawn(&mut s, 10, r#"{"Transform":{"pos":[13.2,4,0]},"CharacterBody":{},"CharacterMotion":{}}"#);
    spawn(&mut s, 11, r#"{"Transform":{"pos":[0,4,13.2]},"CharacterBody":{}}"#);
    s.step_n(90);
    let (a, b) = (pos(&s, 10), pos(&s, 11));
    assert!((a[1] - b[1]).abs() < 0.05, "the same ground under both: {a:?} {b:?}");
    assert!(motion(&s, 10).wading, "{:?}", motion(&s, 10));
    set_body(&mut s, 10, |c| c.walk = [3.0, 0.0]);
    set_body(&mut s, 11, |c| c.walk = [0.0, 3.0]);
    s.step_n(30);
    let (a2, b2) = (pos(&s, 10), pos(&s, 11));
    let (da, db) = (a2[0] - a[0], b2[2] - b[2]);
    assert!(da < db * 0.95 && da > db * 0.6, "wading is slower: {da} against {db}");
    // swimming, walking at 6 m/s: no more than swim_speed (2.4 m/s)
    let mut t = lake_world("swimspeed");
    spawn(&mut t, 10, r#"{"Transform":{"pos":[0,6,0]},"CharacterBody":{},"CharacterMotion":{}}"#);
    t.step_n(240);
    let start = pos(&t, 10);
    set_body(&mut t, 10, |c| c.walk = [6.0, 0.0]);
    t.step_n(60);
    // (the walk is cleared by nobody: it stays 6.0)
    let went = pos(&t, 10)[0] - start[0];
    assert!(went > 2.0 && went < 2.6, "one second at 2.4 m/s: {went}");
    assert!(motion(&t, 10).swimming);
}

#[test]
fn a_swimmer_pushing_against_a_dock_pulls_itself_out_onto_it() {
    let mut s = lake_world("dock");
    // a dock: a box over the water from x = 4 to 6 with its top 0.6 m above the surface
    spawn(&mut s, 5, r#"{"Transform":{"pos":[5,1.8,0],"scale":[2,3.6,4]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,6,0]},"CharacterBody":{},"CharacterMotion":{}}"#);
    s.step_n(240);
    assert!(motion(&s, 10).swimming);
    set_body(&mut s, 10, |c| c.walk = [2.4, 0.0]);
    let mut mantled = false;
    for _ in 0..400 {
        s.step_n(1);
        if motion(&s, 10).mantling && !mantled {
            // (it has hold of the dock: stop pushing, or it walks on over it)
            set_body(&mut s, 10, |c| c.walk = [0.0, 0.0]);
        }
        mantled |= motion(&s, 10).mantling;
    }
    assert!(mantled, "it pulled itself up: {:?} {:?}", pos(&s, 10), motion(&s, 10));
    let p = pos(&s, 10);
    assert!((p[1] - 3.6).abs() < 0.1 && p[0] > 4.0, "standing on the dock: {p:?}");
    assert!(!motion(&s, 10).swimming && body(&s, 10).grounded, "{:?}", motion(&s, 10));
}

// ---------------------------------------------------------------------------------------------- roll

#[test]
fn a_roll_goes_along_the_walk_ignores_the_input_and_has_to_cool_down() {
    let mut s = flat();
    set_body(&mut s, 2, |c| c.walk = [0.0, 3.0]);
    set_motion(&mut s, 2, |m| m.roll = true);
    s.step_n(1);
    let m = motion(&s, 2);
    assert!(m.rolling && !m.roll && m.roll_left > 0.5, "{m:?}");
    let start = pos(&s, 2);
    // the input turns the other way: the roll does not care
    set_body(&mut s, 2, |c| c.walk = [-3.0, 0.0]);
    s.step_n(15);
    assert!(pos(&s, 2)[0] - start[0] > -0.01 && pos(&s, 2)[2] - start[2] > 0.5, "{:?} from {start:?}", pos(&s, 2));
    assert!(motion(&s, 2).rolling);
    let mut ticks = 0;
    while motion(&s, 2).rolling && ticks < 120 {
        s.step_n(1);
        ticks += 1;
    }
    let m = motion(&s, 2);
    assert!(!m.rolling && m.roll_wait > 0.0, "over, and cooling down: {m:?}");
    let end = pos(&s, 2);
    // the roll covers about 2.1 m (7 m/s slowing to a quarter over 0.6 s), along +z, and the walk after it moves it back
    assert!(end[2] - start[2] > 1.7 && end[2] - start[2] < 2.6, "{end:?} from {start:?}");
    // a roll asked for during the cooldown does nothing
    set_motion(&mut s, 2, |m| m.roll = true);
    s.step_n(1);
    assert!(!motion(&s, 2).rolling);
    s.step_n(30);
    set_body(&mut s, 2, |c| c.walk = [0.0, 0.0]);
    set_motion(&mut s, 2, |m| {
        m.roll = true;
        m.roll_dir = [1.0, 0.0];
    });
    s.step_n(2);
    assert!(motion(&s, 2).rolling && motion(&s, 2).roll_dir == [1.0, 0.0], "{:?}", motion(&s, 2));
}

#[test]
fn a_roll_needs_the_ground_and_a_direction() {
    let mut s = flat();
    // standing still with no roll_dir: no direction, no roll
    set_motion(&mut s, 2, |m| m.roll = true);
    s.step_n(2);
    assert!(!motion(&s, 2).rolling);
    // in the air: no roll
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<Transform>(e).unwrap().pos = [0.0, 8.0, 0.0];
    set_body(&mut s, 2, |c| c.walk = [0.0, 3.0]);
    s.step_n(5);
    assert!(!body(&s, 2).grounded, "falling");
    set_motion(&mut s, 2, |m| m.roll = true);
    s.step_n(1);
    assert!(!motion(&s, 2).rolling, "{:?}", motion(&s, 2));
}

// ---------------------------------------------------------------------------------------------- ladders and ledges

/// A tall wall (its front face at z = -1.8, top at y = 4) with a ladder on its front, and a hero in front of it.
fn ladder_world() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,2,-3.3],"scale":[6,4,3]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,0,-1.8]},"Climbable":{"height":4,"width":0.8}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0.2,-1.0]},"CharacterBody":{},"CharacterMotion":{}}"#);
    s.step_n(30);
    s
}

#[test]
fn walking_into_a_ladder_grabs_it_and_the_top_leads_onto_the_wall() {
    let mut s = ladder_world();
    set_body(&mut s, 2, |c| c.walk = [0.0, -1.5]);
    s.step_n(30);
    assert!(motion(&s, 2).climbing && motion(&s, 2).ladder.0 == 4, "{:?} at {:?}", motion(&s, 2), pos(&s, 2));
    set_body(&mut s, 2, |c| c.walk = [0.0, 0.0]);
    // climbing: up at 2 m/s
    let y0 = pos(&s, 2)[1];
    set_motion(&mut s, 2, |m| m.vertical = 1.0);
    s.step_n(30);
    let dy = pos(&s, 2)[1] - y0;
    assert!(dy > 0.9 && dy < 1.1, "half a second at 2 m/s: {dy}");
    // no input, no sliding
    set_motion(&mut s, 2, |m| m.vertical = 0.0);
    let held = pos(&s, 2)[1];
    s.step_n(60);
    assert!((pos(&s, 2)[1] - held).abs() < 0.01, "{} vs {held}", pos(&s, 2)[1]);
    // all the way up and onto the top
    set_motion(&mut s, 2, |m| m.vertical = 1.0);
    let mut mantled = false;
    for _ in 0..240 {
        s.step_n(1);
        mantled |= motion(&s, 2).mantling;
    }
    assert!(mantled, "{:?} at {:?}", motion(&s, 2), pos(&s, 2));
    let p = pos(&s, 2);
    assert!((p[1] - 4.0).abs() < 0.1 && p[2] < -1.9, "standing on the wall: {p:?}");
    assert!(!motion(&s, 2).climbing && body(&s, 2).grounded, "{:?}", motion(&s, 2));
}

#[test]
fn going_down_a_ladder_lets_go_at_the_ground_and_a_jump_lets_go_at_once() {
    let mut s = ladder_world();
    set_body(&mut s, 2, |c| c.walk = [0.0, -1.5]);
    s.step_n(20);
    set_body(&mut s, 2, |c| c.walk = [0.0, 0.0]);
    set_motion(&mut s, 2, |m| m.vertical = 1.0);
    s.step_n(60);
    assert!(pos(&s, 2)[1] > 1.5);
    // a jump lets go
    set_body(&mut s, 2, |c| c.jump = 4.0);
    s.step_n(1);
    assert!(!motion(&s, 2).climbing, "{:?}", motion(&s, 2));
    set_motion(&mut s, 2, |m| m.vertical = 0.0);
    s.step_n(90);
    assert!(pos(&s, 2)[1] < 0.05 && body(&s, 2).grounded, "{:?}", pos(&s, 2));
    // down to the ground from the ladder: climb up a little, then down; it lets go on the ground
    set_body(&mut s, 2, |c| c.walk = [0.0, -1.5]);
    s.step_n(30);
    set_body(&mut s, 2, |c| c.walk = [0.0, 0.0]);
    if motion(&s, 2).climbing {
        set_motion(&mut s, 2, |m| m.vertical = 1.0);
        s.step_n(30);
        set_motion(&mut s, 2, |m| m.vertical = -1.0);
        s.step_n(120);
        assert!(!motion(&s, 2).climbing && pos(&s, 2)[1] < 0.05, "{:?} at {:?}", motion(&s, 2), pos(&s, 2));
    }
}

#[test]
fn a_jump_against_a_ledge_pulls_the_character_up_when_it_is_in_reach_and_only_jumps_when_it_is_not() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1}}"#);
    // a low wall (1.2 m) and a high one (2.6 m), 1.5 m in front of two heroes
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,0.6,-3],"scale":[4,1.2,3]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[10,1.3,-3],"scale":[4,2.6,3]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0.2,0]},"CharacterBody":{},"CharacterMotion":{}}"#);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[10,0.2,0]},"CharacterBody":{},"CharacterMotion":{}}"#);
    s.step_n(30);
    for uid in [2, 5] {
        set_body(&mut s, uid, |c| c.walk = [0.0, -2.0]);
    }
    s.step_n(40);
    for uid in [2, 5] {
        set_body(&mut s, uid, |c| c.jump = 5.0);
    }
    let (mut low, mut high) = (false, false);
    for _ in 0..90 {
        s.step_n(1);
        low |= motion(&s, 2).mantling;
        high |= motion(&s, 5).mantling;
    }
    assert!(low && !high, "low {low}, high {high}");
    let p = pos(&s, 2);
    assert!((p[1] - 1.2).abs() < 0.1 && p[2] < -1.4, "on top of the low wall: {p:?}");
    assert!(pos(&s, 5)[1] < 1.0, "the high wall is not climbed: {:?}", pos(&s, 5));
}

#[test]
fn a_world_with_no_motion_component_walks_as_before_and_the_state_is_the_same_after_a_replay() {
    // a character with and without `CharacterMotion` on flat ground and out of the water does exactly the same
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0.5,0]},"CharacterBody":{}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,0.5,5]},"CharacterBody":{},"CharacterMotion":{}}"#);
    s.step_n(30);
    for uid in [2, 3] {
        set_body(&mut s, uid, |c| c.walk = [2.0, 1.0]);
    }
    s.step_n(120);
    let (a, b) = (pos(&s, 2), pos(&s, 3));
    assert!((a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3 && (a[2] + 5.0 - b[2]).abs() < 1e-3, "{a:?} {b:?}");
    // the same input twice gives the same hash
    let hash = |s: &Sim| s.state_hash();
    let (mut x, mut y) = (flat(), flat());
    for sim in [&mut x, &mut y] {
        set_body(sim, 2, |c| c.walk = [0.0, 3.0]);
        set_motion(sim, 2, |m| m.roll = true);
        sim.step_n(90);
    }
    assert_eq!(hash(&x), hash(&y));
}

#[test]
fn a_character_with_a_motion_reports_the_solids_it_touches_like_a_physics_contact() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[3,1,0],"scale":[1,2,20]},"Collider":{"shape":1}}"#);
    // one with a motion, one without: only the first is reported (a world that never used it keeps the contacts it had)
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0.2,0]},"CharacterBody":{},"CharacterMotion":{}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,0.2,6]},"CharacterBody":{}}"#);
    s.step_n(30);
    for uid in [2, 4] {
        set_body(&mut s, uid, |c| c.walk = [4.0, 0.0]);
    }
    s.step_n(120);
    s.step_n(1);
    let with: Vec<_> = s.contacts.iter().filter(|c| c.a == 2).collect();
    assert!(with.iter().any(|c| c.b == 3 && !c.trigger && c.normal[0] > 0.9), "the wall: {:?}", s.contacts);
    assert!(with.iter().any(|c| c.b == 1 && c.normal[1] < -0.9), "the floor: {:?}", s.contacts);
    assert!(s.contacts.iter().all(|c| c.a != 4 && c.b != 4), "the character without a motion reports nothing: {:?}", s.contacts);
    // and it can be turned off
    set_motion(&mut s, 2, |m| m.report_contacts = false);
    s.step_n(2);
    assert!(s.contacts.iter().all(|c| c.a != 2), "{:?}", s.contacts);
}

#[test]
fn a_steer_walks_a_character_without_moving_it_twice_and_a_wall_stops_the_velocity_it_keeps() {
    let mut s = flat();
    spawn(&mut s, 20, r#"{"Transform":{"pos":[6,0,5]}}"#);
    spawn(&mut s, 30, r#"{"Transform":{"pos":[0,0.2,5]},"CharacterBody":{},"CharacterMotion":{},"Velocity":{},"Steer":{"seek":20,"max_speed":3,"arrive":1}}"#);
    let mut last = pos(&s, 30);
    let mut fastest = 0.0f32;
    for _ in 0..240 {
        s.step_n(1);
        let p = pos(&s, 30);
        fastest = fastest.max(((p[0] - last[0]).powi(2) + (p[2] - last[2]).powi(2)).sqrt() * 60.0);
        last = p;
    }
    assert!((last[0] - 6.0).abs() < 0.5 && (last[2] - 5.0).abs() < 0.3 && last[1].abs() < 0.01, "arrived: {last:?}");
    assert!(fastest < 3.2, "never faster than max_speed (a double move would be 6): {fastest}");
    // a wall across the way at x = 3: it stops against it, and what the velocity says is what it really did
    let mut t = flat();
    spawn(&mut t, 3, r#"{"Transform":{"pos":[3,1,5],"scale":[1,2,30]},"Collider":{"shape":1}}"#);
    spawn(&mut t, 20, r#"{"Transform":{"pos":[6,0,5]}}"#);
    spawn(&mut t, 30, r#"{"Transform":{"pos":[0,0.2,5]},"CharacterBody":{},"CharacterMotion":{},"Velocity":{},"Steer":{"seek":20,"max_speed":3,"arrive":1}}"#);
    t.step_n(300);
    let p = pos(&t, 30);
    assert!(p[0] > 1.8 && p[0] < 2.3, "stopped at the wall (2.5 - 0.35): {p:?}");
    let v = t.world.get::<components::Velocity>(t.world.entity(30).unwrap()).unwrap().lin;
    assert!(v[0].abs() < 0.2, "the velocity is what it did, not what it wanted: {v:?}");
}
