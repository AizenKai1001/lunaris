use super::*;
use components::{CharacterBody, Transform};
use lunaris_json::{parse, Json};

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

fn set_body(s: &mut Sim, uid: u64, f: impl FnOnce(&mut CharacterBody)) {
    let e = s.world.entity(uid).unwrap();
    f(s.world.get_mut::<CharacterBody>(e).unwrap());
}

/// A floor whose top is at y = 0, and a hero standing on it at the origin.
fn world_with_hero() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0.5,0]},"CharacterBody":{}}"#);
    s
}

#[test]
fn a_jump_that_bonks_the_corner_of_a_ceiling_is_nudged_round_it_and_goes_on_up() {
    // a slab 1.9 m above the floor whose edge is at x = 0.2: the capsule (radius 0.35) at x = 0 has its head 0.15 m into it
    let mut s = world_with_hero();
    s.step_n(30);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[2.2,2.1,0],"scale":[4,0.4,4]},"Collider":{"shape":1}}"#);
    set_body(&mut s, 2, |b| b.jump = 5.0);
    let mut top = 0.0f32;
    for _ in 0..50 {
        s.step();
        top = top.max(pos(&s, 2)[1]);
    }
    assert!(top > 0.8, "the jump went on past the corner: apex {top}");
    // a slab over the whole head is still a ceiling: it stops the jump (nothing to nudge round)
    let mut s = world_with_hero();
    s.step_n(30);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,2.1,0],"scale":[4,0.4,4]},"Collider":{"shape":1}}"#);
    set_body(&mut s, 2, |b| b.jump = 5.0);
    let mut top = 0.0f32;
    for _ in 0..50 {
        s.step();
        top = top.max(pos(&s, 2)[1]);
    }
    assert!(top < 0.4, "a full ceiling stops it: apex {top}");
}

#[test]
fn a_character_falls_onto_the_floor_and_stands_there() {
    let mut s = world_with_hero();
    s.step_n(60);
    assert!(pos(&s, 2)[1].abs() < 1e-3, "{:?}", pos(&s, 2));
    let b = body(&s, 2);
    assert!(b.grounded && b.fall == 0.0, "{b:?}");
    // and stays put for as long as nothing asks it to move
    let before = pos(&s, 2);
    s.step_n(600);
    assert!((0..3).all(|k| (pos(&s, 2)[k] - before[k]).abs() < 1e-4), "{:?} vs {before:?}", pos(&s, 2));
}

#[test]
fn walking_moves_at_the_asked_speed_and_a_wall_stops_it_but_not_the_sliding() {
    let mut s = world_with_hero();
    s.step_n(30);
    set_body(&mut s, 2, |b| b.walk = [3.0, 0.0]);
    s.step_n(60); // one second
    assert!((pos(&s, 2)[0] - 3.0).abs() < 0.01, "{:?}", pos(&s, 2));
    // a wall across the way at x = 5 (faces at 4.5 and 5.5)
    spawn(&mut s, 3, r#"{"Transform":{"pos":[5,1,0],"scale":[1,2,20]},"Collider":{"shape":1}}"#);
    s.step_n(120);
    let x = pos(&s, 2)[0];
    assert!((x - 4.15).abs() < 0.02, "stopped against the wall at 4.5 - 0.35: {x}");
    // walking diagonally into the wall slides along it
    set_body(&mut s, 2, |b| b.walk = [3.0, 3.0]);
    s.step_n(60);
    let p = pos(&s, 2);
    assert!((p[0] - 4.15).abs() < 0.02 && p[2] > 2.5, "slides along the wall: {p:?}");
}

#[test]
fn low_steps_are_climbed_and_tall_blocks_are_not() {
    let mut s = world_with_hero();
    s.step_n(5);
    s.step_n(25);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[3,0.15,0]},"Collider":{"shape":1,"size":[2,0.3,4]}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[-3,0.5,0]},"Collider":{"shape":1,"size":[2,1,4]}}"#);
    set_body(&mut s, 2, |b| b.walk = [3.0, 0.0]);
    let mut top = 0.0f32;
    for _ in 0..90 {
        s.step();
        top = top.max(pos(&s, 2)[1]);
    }
    assert!(pos(&s, 2)[0] > 4.0 && (top - 0.3).abs() < 0.02, "stepped up onto the 0.3 high block and on across it (highest {top}): {:?}", pos(&s, 2));
    assert!(pos(&s, 2)[1].abs() < 0.02 && body(&s, 2).grounded, "and walked down off the far side: {:?}", pos(&s, 2));
    // the other way: a 1.0 high block is a wall
    set_body(&mut s, 2, |b| b.walk = [-3.0, 0.0]);
    s.step_n(300);
    let p = pos(&s, 2);
    assert!((p[0] - (-2.0 + 0.35)).abs() < 0.03 && p[1].abs() < 0.02, "{p:?}");
}

#[test]
fn a_fat_capsule_walking_slowly_climbs_stairs_and_walks_down_them_without_falling() {
    // a flight of 8 steps, 0.235 high and 0.25 deep, along +z from z = 2 (the capsule: radius 0.34, step height 0.45, at 0.5 m/s: its round foot meets each edge long before it is over it), then a landing and the same flight down
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1}}"#);
    for i in 1..=8 {
        let top = 0.235 * i as f32;
        spawn(&mut s, 10 + i, &format!(r#"{{"Transform":{{"pos":[0,{},{}]}},"Collider":{{"shape":1,"size":[3,{},0.25]}}}}"#, top / 2.0, 2.0 + (i as f32 - 0.5) * 0.25, top));
    }
    spawn(&mut s, 30, &format!(r#"{{"Transform":{{"pos":[0,{},{}]}},"Collider":{{"shape":1,"size":[3,{},3]}}}}"#, 0.235 * 8.0 / 2.0, 4.0 + 1.5, 0.235 * 8.0));
    for j in 1..8 {
        let top = 0.235 * (8 - j) as f32;
        spawn(&mut s, 40 + j, &format!(r#"{{"Transform":{{"pos":[0,{},{}]}},"Collider":{{"shape":1,"size":[3,{},0.25]}}}}"#, top / 2.0, 7.0 + (j as f32 - 0.5) * 0.25, top));
    }
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,0,0]},"CharacterBody":{"radius":0.34,"height":1.76,"step_height":0.45,"max_slope":45}}"#);
    s.step_n(20);
    set_body(&mut s, 3, |b| b.walk = [0.0, 0.5]);
    let mut top = 0.0f32;
    let mut ungrounded = 0;
    let mut reached = None;
    for t in 0..1800 {
        s.step();
        let p = pos(&s, 3);
        top = top.max(p[1]);
        if !body(&s, 3).grounded {
            ungrounded += 1;
        }
        if reached.is_none() && p[2] > 7.0 {
            reached = Some(t);
        }
        if p[2] > 9.2 {
            break;
        }
    }
    let p = pos(&s, 3);
    assert!((top - 0.235 * 8.0).abs() < 0.03, "up to the landing (highest {top}): {p:?}");
    assert!(reached.is_some_and(|t| t < 1000), "the way up took {reached:?} ticks");
    assert!(p[2] > 9.0 && p[1] < 0.05, "and down the other side: {p:?}");
    assert!(ungrounded <= 3, "walking up and down stairs does not leave the ground: {ungrounded} ticks in the air");
}

#[test]
fn a_fat_capsule_that_stops_on_the_rim_of_a_step_stays_there() {
    // stairs of 4 steps (0.235 high, 0.25 deep); the capsule (radius 0.34) is walked slowly until its round foot is on the nose of the second step, then it is let go of: it must not roll back down
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1}}"#);
    for i in 1..=4 {
        let top = 0.235 * i as f32;
        spawn(&mut s, 10 + i, &format!(r#"{{"Transform":{{"pos":[0,{},{}]}},"Collider":{{"shape":1,"size":[3,{},0.25]}}}}"#, top / 2.0, 2.0 + (i as f32 - 0.5) * 0.25, top));
    }
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,0,0]},"CharacterBody":{"radius":0.34,"height":1.76,"step_height":0.45,"max_slope":45}}"#);
    s.step_n(20);
    set_body(&mut s, 3, |b| b.walk = [0.0, 0.5]);
    // walk until it is at least a quarter of the way up the second step (somewhere on the rim of the first or second)
    for _ in 0..600 {
        s.step();
        if pos(&s, 3)[1] > 0.3 {
            break;
        }
    }
    set_body(&mut s, 3, |b| b.walk = [0.0, 0.0]);
    s.step_n(5);
    let at = pos(&s, 3);
    s.step_n(240);
    let p = pos(&s, 3);
    assert!((p[2] - at[2]).abs() < 0.01 && (p[1] - at[1]).abs() < 0.01, "it stays where it stopped: {at:?} then {p:?}");
    assert!(body(&s, 3).grounded, "and is on the ground");
    // asked to walk on, it goes on up
    set_body(&mut s, 3, |b| b.walk = [0.0, 0.5]);
    let mut top = 0.0f32;
    for _ in 0..400 {
        s.step();
        top = top.max(pos(&s, 3)[1]);
    }
    assert!(top > 0.9, "walking on it reaches the top (0.94): {top}");
}

#[test]
fn a_jump_needs_the_ground_and_is_one_request() {
    let mut s = world_with_hero();
    s.step_n(40);
    set_body(&mut s, 2, |b| b.jump = 5.0);
    s.step_n(1);
    assert!(body(&s, 2).fall > 4.0 && !body(&s, 2).grounded && body(&s, 2).jump == 0.0, "{:?}", body(&s, 2));
    // a request in mid-air does nothing
    s.step_n(5);
    let fall = body(&s, 2).fall;
    set_body(&mut s, 2, |b| b.jump = 5.0);
    s.step_n(1);
    assert!(body(&s, 2).fall < fall, "gravity kept pulling: {} then {}", fall, body(&s, 2).fall);
    // the apex is about v^2 / 2g = 1.28 and then it lands again
    let mut top = 0.0f32;
    for _ in 0..120 {
        s.step();
        top = top.max(pos(&s, 2)[1]);
    }
    assert!((top - 1.28).abs() < 0.15, "{top}");
    assert!(body(&s, 2).grounded && pos(&s, 2)[1].abs() < 1e-3);
}

#[test]
fn a_ceiling_stops_a_jump_triggers_and_other_characters_do_not_block_and_gravity_scale_zero_flies() {
    let mut s = world_with_hero();
    s.step_n(40);
    // a ceiling 2.2 above the floor: the 1.8 tall hero can rise only 0.4
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,2.7,0]},"Collider":{"shape":1,"size":[4,1,4]}}"#);
    set_body(&mut s, 2, |b| b.jump = 8.0);
    let mut top = 0.0f32;
    for _ in 0..60 {
        s.step();
        top = top.max(pos(&s, 2)[1]);
    }
    assert!(top < 0.45, "{top}");
    // a trigger zone and another character in the way are walked through
    spawn(&mut s, 4, r#"{"Transform":{"pos":[2,1,0],"scale":[1,2,4]},"Collider":{"shape":1,"trigger":true}}"#);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[3,0,0]},"CharacterBody":{}}"#);
    set_body(&mut s, 2, |b| b.walk = [3.0, 0.0]);
    s.step_n(120);
    assert!(pos(&s, 2)[0] > 5.0, "{:?}", pos(&s, 2));
    // no gravity: it stays where it is, in the air
    spawn(&mut s, 6, r#"{"Transform":{"pos":[0,5,10]},"CharacterBody":{"gravity_scale":0}}"#);
    s.step_n(60);
    assert_eq!(pos(&s, 6)[1], 5.0);
}

#[test]
fn bad_numbers_do_not_break_the_world_and_the_motion_is_replay_exact() {
    let mut s = world_with_hero();
    s.step_n(40);
    set_body(&mut s, 2, |b| {
        b.walk = [f32::NAN, f32::INFINITY];
        b.radius = -1.0;
        b.height = 0.0;
    });
    s.step_n(10);
    assert!(pos(&s, 2).iter().all(|v| v.is_finite()), "{:?}", pos(&s, 2));
    // the same inputs give the same state hash
    let run = || {
        let mut s = world_with_hero();
        s.step_n(5);
        set_body(&mut s, 2, |b| b.walk = [2.0, 1.0]);
        s.step_n(100);
        set_body(&mut s, 2, |b| b.jump = 6.0);
        s.step_n(100);
        s.state_hash()
    };
    assert_eq!(run(), run());
}

#[test]
fn a_wall_that_is_moved_added_or_taken_away_is_felt_at_the_next_step() {
    // (the grid of the obstacles is made again every step: a wall that is moved, added or taken away is felt at once)
    let mut s = world_with_hero();
    spawn(&mut s, 3, r#"{"Transform":{"pos":[40,1,0],"scale":[1,2,20]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,0.5,5]},"CharacterBody":{}}"#);
    let go = |s: &mut Sim, x: f32| {
        set_body(s, 2, |b| b.walk = [x, 0.0]);
        s.step_n(60);
    };
    go(&mut s, 4.0);
    assert!(pos(&s, 2)[0] > 3.0, "nothing is in the way: {:?}", pos(&s, 2));
    set_body(&mut s, 2, |b| b.walk = [0.0, 0.0]);
    s.step_n(3);
    // the wall that was far away is moved to just ahead of the hero
    let ahead = pos(&s, 2)[0] + 1.5;
    s.world.get_mut::<Transform>(s.world.entity(3).unwrap()).unwrap().pos = [ahead, 1.0, 0.0];
    go(&mut s, 4.0);
    let blocked = pos(&s, 2)[0];
    assert!(blocked < ahead - 0.3, "the wall that was moved stops the hero: x {blocked} wall at {ahead}");
    // a wall that is added behind a still world; then it is taken away
    set_body(&mut s, 2, |b| b.walk = [0.0, 0.0]);
    s.step_n(3);
    spawn(&mut s, 5, &format!(r#"{{"Transform":{{"pos":[{},1,0]}},"Collider":{{"shape":1}}}}"#, blocked - 1.2));
    go(&mut s, -4.0);
    assert!(pos(&s, 2)[0] > blocked - 1.2, "the wall that was added stops the hero going back: x {}", pos(&s, 2)[0]);
    let e = s.world.entity(5).unwrap();
    s.world.despawn(e);
    set_body(&mut s, 2, |b| b.walk = [0.0, 0.0]);
    s.step_n(3);
    go(&mut s, -4.0);
    assert!(pos(&s, 2)[0] < blocked - 2.0, "and goes through where the wall was when it is taken away: x {}", pos(&s, 2)[0]);
}

/// A crowd of capsules (some with `CharacterMotion`, some steered), walking about among walls and boxes, jumping now and then: the world after `ticks` ticks on `threads` threads.
fn crowd(threads: usize, people: usize, ticks: u64) -> Sim {
    let mut s = Sim::new(1);
    s.threads = threads;
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[200,1,200]},"Collider":{"shape":1}}"#);
    // walls round a yard, boxes in it, a ramp-like step
    for (i, (x, z, sx, sz)) in [(0.0, 12.0, 24.0, 0.5), (0.0, -12.0, 24.0, 0.5), (12.0, 0.0, 0.5, 24.0), (-12.0, 0.0, 0.5, 24.0), (3.0, 3.0, 2.0, 2.0), (-4.0, 2.0, 1.0, 3.0), (0.0, -5.0, 4.0, 0.8)].iter().enumerate() {
        spawn(&mut s, 10 + i as u64, &format!(r#"{{"Transform":{{"pos":[{x},1,{z}],"scale":[{sx},2,{sz}]}},"Collider":{{"shape":1}}}}"#));
    }
    spawn(&mut s, 30, r#"{"Transform":{"pos":[6,0.15,-6],"scale":[3,0.3,3]},"Collider":{"shape":1}}"#);
    for k in 0..people {
        let (x, z) = (-10.0 + (k % 10) as f32 * 2.2, -9.0 + (k / 10) as f32 * 2.3);
        let a = k as f32 * 0.9;
        let extra = match k % 5 {
            0 => r#","CharacterMotion":{}"#,
            3 => r#","CharacterMotion":{"report_contacts":false}"#,
            _ => "",
        };
        spawn(&mut s, 100 + k as u64, &format!(r#"{{"Transform":{{"pos":[{x},0.2,{z}]}},"CharacterBody":{{"walk":[{},{}]}}{extra}}}"#, a.cos() * 3.0, a.sin() * 3.0));
    }
    for tick in 0..ticks {
        if tick % 25 == 0 {
            for k in (0..people).step_by(3) {
                set_body(&mut s, 100 + k as u64, |b| b.jump = 5.0);
            }
        }
        if tick % 40 == 0 {
            for k in 0..people {
                let a = (k as u64 + tick) as f32 * 1.3;
                set_body(&mut s, 100 + k as u64, |b| b.walk = [a.cos() * 4.0, a.sin() * 4.0]);
            }
        }
        s.step();
    }
    s
}

#[test]
fn a_crowd_of_capsules_ends_up_the_same_whatever_the_number_of_threads() {
    // 90 capsules: more than the controller needs to hand the walking to its worker threads (24)
    let reference = crowd(1, 90, 160);
    let h1 = reference.state_hash();
    for threads in [2, 4, 8] {
        let s = crowd(threads, 90, 160);
        assert_eq!(s.state_hash(), h1, "{threads} threads");
        for k in 0..90u64 {
            assert_eq!(pos(&s, 100 + k), pos(&reference, 100 + k), "capsule {k} on {threads} threads");
        }
    }
    // the crowd really moved about and met the walls (a test that nothing happens in proves nothing)
    let moved = (0..90u64)
        .filter(|k| {
            let p = pos(&reference, 100 + k);
            let (x0, z0) = (-10.0 + (*k % 10) as f32 * 2.2, -9.0 + (*k / 10) as f32 * 2.3);
            ((p[0] - x0).powi(2) + (p[2] - z0).powi(2)).sqrt() > 1.5
        })
        .count();
    assert!(moved >= 8, "{moved} of 90 capsules went somewhere");
    assert_ne!(h1, crowd(1, 90, 0).state_hash(), "the walk changed the world");
    // fewer than the limit are walked on the calling thread whatever the setting: the same answer again
    assert_eq!(crowd(1, 12, 80).state_hash(), crowd(8, 12, 80).state_hash());
}

#[test]
fn the_grid_of_obstacles_is_laid_out_once_patched_where_some_moved_and_the_result_is_the_same_without_reuse() {
    use crate::character::{GRID_BUILDS, GRID_PATCHES, GRID_REUSE_OFF, GRID_VERIFY};
    let run = |reuse_off: bool, build: &dyn Fn() -> Sim| {
        GRID_REUSE_OFF.with(|f| f.set(reuse_off));
        GRID_VERIFY.with(|f| f.set(!reuse_off));
        GRID_BUILDS.with(|b| b.set(0));
        GRID_PATCHES.with(|b| b.set(0));
        let s = build();
        let out = (s, GRID_BUILDS.with(|b| b.get()), GRID_PATCHES.with(|b| b.get()));
        GRID_REUSE_OFF.with(|f| f.set(false));
        GRID_VERIFY.with(|f| f.set(false));
        out
    };
    // 1. nothing but the capsules moves: the obstacles are laid out in the first step and reused for the other 79
    let still = || crowd(1, 40, 80);
    let (a, builds_a, patches_a) = run(false, &still);
    let (b, builds_b, _) = run(true, &still);
    assert_eq!(a.state_hash(), b.state_hash(), "reusing the layout changes nothing");
    assert_eq!((builds_a, patches_a, builds_b), (1, 0, 80));
    // 2. a box slides along the yard for 50 steps and then stays, and a capsule walks into the wall: one obstacle changes in each of those steps, so its cells are patched (every patched layout is
    //    checked against a fresh one while this runs)
    let moving = || {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1}}"#);
        spawn(&mut s, 2, r#"{"Transform":{"pos":[6,1,0],"scale":[0.5,2,10]},"Collider":{"shape":1}}"#);
        spawn(&mut s, 3, r#"{"Transform":{"pos":[-3,0.5,3]},"Collider":{"shape":1,"size":[1,1,1]}}"#);
        for k in 0..12u64 {
            spawn(&mut s, 20 + k, &format!(r#"{{"Transform":{{"pos":[{},0.5,-8]}},"Collider":{{"shape":1,"size":[1,1,1]}}}}"#, -10.0 + k as f32 * 1.7));
        }
        spawn(&mut s, 4, r#"{"Transform":{"pos":[0,0.2,0]},"CharacterBody":{"walk":[4,0]}}"#);
        for tick in 0..90 {
            if tick < 50 {
                let e = s.world.entity(3).unwrap();
                s.world.get_mut::<components::Transform>(e).unwrap().pos[0] += 0.1;
            }
            s.step();
        }
        s
    };
    let (c, builds_c, patches_c) = run(false, &moving);
    let (d, builds_d, _) = run(true, &moving);
    assert_eq!(c.state_hash(), d.state_hash());
    assert_eq!((builds_c, patches_c, builds_d), (1, 49, 90));
    assert!(pos(&c, 4)[0] > 1.0 && pos(&c, 4)[0] < 6.0, "the capsule walked and was stopped by the wall: {:?}", pos(&c, 4));
    // 3. twenty boxes of different sizes (and a wide one) move about every step, some of them across cell borders and one out of the world's grid: the layouts stay exact and the capsules end up the same
    let busy = || {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[2000,1,2000]},"Collider":{"shape":1}}"#);
        spawn(&mut s, 2, r#"{"Transform":{"pos":[0,3,60],"scale":[120,6,2]},"Collider":{"shape":1}}"#);
        for k in 0..20u64 {
            let (x, z) = ((k % 5) as f32 * 7.0 - 14.0, (k / 5) as f32 * 7.0 - 10.0);
            spawn(&mut s, 10 + k, &format!(r#"{{"Transform":{{"pos":[{x},0.5,{z}],"scale":[{},1,{}]}},"Collider":{{"shape":1,"size":[1,1,1]}}}}"#, 1.0 + (k % 3) as f32, 1.0 + (k % 4) as f32 * 0.7));
        }
        for k in 0..60u64 {
            spawn(&mut s, 200 + k, &format!(r#"{{"Transform":{{"pos":[{},0.5,{}]}},"Collider":{{"shape":1,"size":[1,1,1]}}}}"#, -30.0 + (k % 10) as f32 * 6.0, 20.0 + (k / 10) as f32 * 5.0));
        }
        for k in 0..12u64 {
            spawn(&mut s, 100 + k, &format!(r#"{{"Transform":{{"pos":[{},0.2,{}]}},"CharacterBody":{{"walk":[{},{}]}}}}"#, -12.0 + k as f32 * 2.0, -12.0, (k as f32).cos() * 3.0, 2.0));
        }
        for tick in 0..120u64 {
            for k in 0..20u64 {
                if (tick + k) % 3 != 0 {
                    continue;
                }
                let e = s.world.entity(10 + k).unwrap();
                let a = (tick * 7 + k * 13) as f32;
                let p = &mut s.world.get_mut::<components::Transform>(e).unwrap().pos;
                p[0] += a.sin() * 0.9;
                p[2] += a.cos() * 0.9;
                if k == 7 && tick == 60 {
                    p[0] = 5000.0;
                }
            }
            s.step();
        }
        s
    };
    let (e, _, patches_e) = run(false, &busy);
    let (f, _, _) = run(true, &busy);
    assert_eq!(e.state_hash(), f.state_hash());
    assert!(patches_e > 300, "many obstacles were patched: {patches_e}");
}
