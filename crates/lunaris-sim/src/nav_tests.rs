use super::*;
use crate::nav::{find_path, NavError, NavParams};
use components::{NavAgent, Transform};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// A floor whose top is at y = 0.
fn floor() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1}}"#);
    s
}

fn wall(s: &mut Sim, uid: u64, x: f32, z0: f32, z1: f32) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{x},1.5,{}],"scale":[1,3,{}]}},"Collider":{{"shape":1}}}}"#, (z0 + z1) / 2.0, z1 - z0));
}

fn path(s: &mut Sim, from: [f32; 3], to: [f32; 3]) -> Result<nav::PathResult, NavError> {
    find_path(&mut s.world, from, to, NavParams::default())
}

#[test]
fn on_open_ground_the_path_is_a_straight_line() {
    let mut s = floor();
    let r = path(&mut s, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]).unwrap();
    assert_eq!(r.points.len(), 2, "{:?}", r.points);
    assert!((r.length - 10.0).abs() < 0.01, "{}", r.length);
    assert!(r.exact);
    assert_eq!(r.points[0], [0.0, 0.0, 0.0]);
    assert_eq!(r.points[1], [10.0, 0.0, 10.0 * 0.0]);
}

#[test]
fn a_wall_is_walked_around_by_the_shortest_way_and_the_agents_radius_keeps_it_off_the_wall() {
    let mut s = floor();
    wall(&mut s, 2, 5.0, -5.0, 5.0); // x 4.5..5.5, z -5..5
    let r = path(&mut s, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]).unwrap();
    assert!(r.length > 14.0 && r.length < 17.0, "detour length {}", r.length);
    let widest = r.points.iter().map(|p| p[2].abs()).fold(0.0f32, f32::max);
    assert!(widest >= 5.0 + 0.4 - 0.3, "the path goes past the end of the wall (z = {widest})");
    // it is the shorter side that was taken, and the same path comes back every time
    let again = path(&mut s, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]).unwrap();
    assert_eq!(r.points, again.points);
    // a point behind the wall and the way back
    let back = path(&mut s, [10.0, 0.0, 0.0], [0.0, 0.0, 0.0]).unwrap();
    assert!((back.length - r.length).abs() < 0.6, "{} vs {}", back.length, r.length);
}

#[test]
fn stairs_are_climbed_but_a_platform_without_them_is_out_of_reach() {
    let mut s = floor();
    // five steps of 0.24 m (each within the 0.4 m an agent climbs), then a platform of the same height as the last
    for i in 0..5u64 {
        let h = 0.24 * (i + 1) as f32;
        spawn(&mut s, 10 + i, &format!(r#"{{"Transform":{{"pos":[{},{},0],"scale":[1,{h},5]}},"Collider":{{"shape":1}}}}"#, 4 + i, h / 2.0));
    }
    spawn(&mut s, 20, r#"{"Transform":{"pos":[13.5,0.6,0],"scale":[10,1.2,8]},"Collider":{"shape":1}}"#);
    let up = path(&mut s, [0.0, 0.0, 0.0], [12.0, 1.2, 0.0]).unwrap();
    assert!((up.points.last().unwrap()[1] - 1.2).abs() < 0.01, "ends on the platform: {:?}", up.points.last());
    assert!(up.length < 13.5, "straight up the stairs: {}", up.length);
    // the same platform with the stairs taken away: a 1.2 m wall, the agent cannot climb it
    let mut t = floor();
    spawn(&mut t, 20, r#"{"Transform":{"pos":[13.5,0.6,0],"scale":[10,1.2,8]},"Collider":{"shape":1}}"#);
    let err = path(&mut t, [0.0, 0.0, 0.0], [12.0, 1.2, 0.0]).unwrap_err();
    assert_eq!(err.code(), "nav_no_path", "{err:?}");
    // an agent that steps higher can
    let tall = find_path(&mut t.world, [0.0, 0.0, 0.0], [12.0, 1.2, 0.0], NavParams { step_height: 1.5, ..NavParams::default() });
    assert!(tall.is_ok(), "{tall:?}");
    // and coming down a 1.2 m drop is allowed, a 3 m one is not
    let down = path(&mut t, [12.0, 1.2, 0.0], [0.0, 0.0, 0.0]).unwrap();
    assert!(down.length < 14.0);
    let mut u = floor();
    spawn(&mut u, 20, r#"{"Transform":{"pos":[13.5,1.5,0],"scale":[10,3,8]},"Collider":{"shape":1}}"#);
    assert_eq!(path(&mut u, [12.0, 3.0, 0.0], [0.0, 0.0, 0.0]).unwrap_err().code(), "nav_no_path");
}

#[test]
fn enclosed_and_walled_in_places_have_no_path_and_the_error_says_where() {
    let mut s = floor();
    // a box of walls around (10, 0)
    wall(&mut s, 2, 7.0, -3.0, 3.0);
    wall(&mut s, 3, 13.0, -3.0, 3.0);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[10,1.5,3],"scale":[7,3,1]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[10,1.5,-3],"scale":[7,3,1]},"Collider":{"shape":1}}"#);
    let err = path(&mut s, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]).unwrap_err();
    assert_eq!(err.code(), "nav_no_path", "{err:?}");
    assert!(err.message().contains("10.0"), "{}", err.message());
    // a start inside a wall
    let err = path(&mut s, [7.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
    assert!(matches!(err, Ok(_) | Err(NavError::NoStart(_))), "{err:?}");
    // a target far off the map snaps to nothing
    let err = path(&mut s, [0.0, 0.0, 0.0], [500.0, 0.0, 0.0]).unwrap_err();
    assert!(matches!(err, NavError::NoGoal(_) | NavError::TooBig(_)), "{err:?}");
}

#[test]
fn a_target_inside_an_obstacle_ends_at_the_nearest_walkable_ground() {
    let mut s = floor();
    spawn(&mut s, 2, r#"{"Transform":{"pos":[10,1.5,0],"scale":[3,3,3]},"Collider":{"shape":1}}"#);
    let r = path(&mut s, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]).unwrap();
    assert!(!r.exact);
    let end = r.points.last().unwrap();
    assert!(end[0] < 10.0 && end[0] > 6.0, "stops in front of the box: {end:?}");
}

fn walker(s: &mut Sim, target: &str) {
    spawn(s, 100, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"CharacterBody":{{}},"NavAgent":{{"target":{target},"active":true,"speed":4}}}}"#));
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn agent(s: &Sim, uid: u64) -> NavAgent {
    *s.world.get::<NavAgent>(s.world.entity(uid).unwrap()).unwrap()
}

#[test]
fn a_character_walks_around_a_wall_to_its_target_and_reports_arrival() {
    let mut s = floor();
    wall(&mut s, 2, 5.0, -5.0, 5.0);
    walker(&mut s, "[10,0,0]");
    s.step_n(5);
    assert_eq!(agent(&s, 100).status, 1, "walking");
    assert!(agent(&s, 100).remaining > 14.0);
    s.step_n(600);
    let p = pos(&s, 100);
    assert!(((p[0] - 10.0).powi(2) + p[2].powi(2)).sqrt() < 0.5, "arrived at {p:?}");
    let a = agent(&s, 100);
    assert_eq!((a.status, a.active), (2, false));
    assert_eq!(a.remaining, 0.0);
    // it has stopped
    let before = pos(&s, 100);
    s.step_n(30);
    assert!(pos(&s, 100).iter().zip(before).all(|(a, b)| (a - b).abs() < 1.0e-3), "it kept walking: {:?} -> {:?}", before, pos(&s, 100));
    // a new target in the middle of the world walks it somewhere else
    let e = s.world.entity(100).unwrap();
    {
        let na = s.world.get_mut::<NavAgent>(e).unwrap();
        na.target = [-6.0, 0.0, 4.0];
        na.active = true;
    }
    s.step_n(500);
    let p = pos(&s, 100);
    assert!(((p[0] + 6.0).powi(2) + (p[2] - 4.0).powi(2)).sqrt() < 0.5, "second trip ended at {p:?}");
}

#[test]
fn a_character_climbs_the_stairs_to_a_platform() {
    let mut s = floor();
    for i in 0..5u64 {
        let h = 0.24 * (i + 1) as f32;
        spawn(&mut s, 10 + i, &format!(r#"{{"Transform":{{"pos":[{},{},0],"scale":[1,{h},5]}},"Collider":{{"shape":1}}}}"#, 4 + i, h / 2.0));
    }
    spawn(&mut s, 20, r#"{"Transform":{"pos":[13.5,0.6,0],"scale":[10,1.2,8]},"Collider":{"shape":1}}"#);
    walker(&mut s, "[12,1.2,0]");
    s.step_n(600);
    let p = pos(&s, 100);
    assert!((p[0] - 12.0).abs() < 0.5 && (p[1] - 1.2).abs() < 0.05, "on the platform at {p:?}");
    assert_eq!(agent(&s, 100).status, 2);
}

#[test]
fn an_unreachable_target_is_status_blocked_and_the_agent_stays_put() {
    let mut s = floor();
    spawn(&mut s, 20, r#"{"Transform":{"pos":[13.5,1.5,0],"scale":[10,3,8]},"Collider":{"shape":1}}"#);
    walker(&mut s, "[12,3,0]");
    s.step_n(30);
    assert_eq!(agent(&s, 100).status, 3);
    assert!(pos(&s, 100).iter().all(|v| v.abs() < 1.0e-3), "{:?}", pos(&s, 100));
}

#[test]
fn an_entity_with_a_velocity_is_steered_and_the_whole_run_is_deterministic_and_replays() {
    let run = |threads: usize| {
        let mut s = floor();
        s.threads = threads;
        wall(&mut s, 2, 5.0, -5.0, 5.0);
        spawn(&mut s, 100, r#"{"Transform":{"pos":[0,0,0]},"Velocity":{},"NavAgent":{"target":[10,0,0],"active":true,"speed":3}}"#);
        s.start_recording().unwrap();
        s.step_n(600);
        s
    };
    let mut a = run(1);
    let b = run(8);
    let p = pos(&a, 100);
    assert!(((p[0] - 10.0).powi(2) + p[2].powi(2)).sqrt() < 0.6, "{p:?}");
    assert_eq!(a.state_hash(), b.state_hash(), "thread count must not matter");
    let rec = a.stop_recording().unwrap();
    let template = Sim::new(1);
    let replayed = Sim::replay(&template, &rec).unwrap();
    assert_eq!(replayed.state_hash(), rec.final_hash);
}

#[test]
fn a_world_without_any_ground_is_an_open_floor_where_obstacles_still_block() {
    let mut s = Sim::new(1);
    // no floor at all: just a wall of spheres' worth of box
    spawn(&mut s, 2, r#"{"Transform":{"pos":[5,0.5,0],"scale":[1,1,10]},"Collider":{"shape":1}}"#);
    let flat = path(&mut s, [0.0, 0.5, 8.0], [10.0, 0.5, 8.0]).unwrap();
    assert_eq!(flat.points.len(), 2, "open ground: a straight line {:?}", flat.points);
    let around = path(&mut s, [0.0, 0.5, 0.0], [10.0, 0.5, 0.0]).unwrap();
    assert!(around.length > 14.0, "the wall is walked around: {}", around.length);
}

#[test]
fn a_detour_that_leaves_the_box_around_the_two_points_is_found_by_looking_at_the_whole_map() {
    let mut s = floor();
    wall(&mut s, 2, 5.0, -45.0, 40.0); // 85 m long: open beyond z = 40 and beyond z = -45 (the floor reaches +-50)
    let r = path(&mut s, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]).unwrap();
    assert!(r.length > 80.0 && r.length < 120.0, "walked all the way round the wall's end: {}", r.length);
    assert!(r.points.iter().any(|p| p[2].abs() > 40.0), "{:?}", r.points);
    // and when the way really is shut, the answer is still no path
    wall(&mut s, 3, 5.0, 40.0, 50.5);
    wall(&mut s, 4, 5.0, -50.5, -45.0);
    assert_eq!(path(&mut s, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]).unwrap_err().code(), "nav_no_path");
}

/// Two islands of floor 4 m apart (x from -10 to 0 and from 4 to 14), top at y = 0.
fn islands() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[-5,-0.5,0],"scale":[10,1,10]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[9,-0.5,0],"scale":[10,1,10]},"Collider":{"shape":1}}"#);
    s
}

#[test]
fn a_nav_link_is_a_way_across_a_gap_that_agents_plan_with_and_jump() {
    let mut s = islands();
    assert!(matches!(path(&mut s, [-5.0, 0.0, 0.0], [9.0, 0.0, 0.0]), Err(NavError::NoPath(_))), "no way without a link");
    spawn(&mut s, 3, r#"{"Transform":{"pos":[-1,0,0]},"NavLink":{"from":[0,0,0],"to":[6,0,0],"two_way":false}}"#);
    let r = path(&mut s, [-5.0, 0.0, 0.0], [9.0, 0.0, 0.0]).expect("across the link");
    assert!(r.jumps.iter().any(|j| j.is_some()), "{:?}", r.jumps);
    let k = r.jumps.iter().position(|j| j.is_some()).unwrap();
    assert!((r.points[k - 1][0] + 1.0).abs() < 1.0 && (r.points[k][0] - 5.0).abs() < 1.0, "{:?}", r.points);
    // one way only
    assert!(matches!(path(&mut s, [9.0, 0.0, 0.0], [-5.0, 0.0, 0.0]), Err(NavError::NoPath(_))));
    // a closed link is no link
    s.world.get_mut::<components::NavLink>(s.world.entity(3).unwrap()).unwrap().enabled = false;
    assert!(path(&mut s, [-5.0, 0.0, 0.0], [9.0, 0.0, 0.0]).is_err());
    s.world.get_mut::<components::NavLink>(s.world.entity(3).unwrap()).unwrap().enabled = true;
    // an agent walks to the edge, hops across and walks on to its target
    spawn(&mut s, 100, r#"{"Transform":{"pos":[-5,0,0]},"CharacterBody":{},"NavAgent":{"target":[9,0,0],"active":true,"speed":4}}"#);
    s.step_n(600);
    let a = agent(&s, 100);
    let p = s.world.get::<Transform>(s.world.entity(100).unwrap()).unwrap().pos;
    assert_eq!(a.status, 2, "arrived: {a:?} at {p:?}");
    assert!((p[0] - 9.0).abs() < 0.5 && p[1].abs() < 0.2, "{p:?}");
    // the same run twice is the same world
    let again = {
        let mut t = islands();
        spawn(&mut t, 3, r#"{"Transform":{"pos":[-1,0,0]},"NavLink":{"from":[0,0,0],"to":[6,0,0],"two_way":false}}"#);
        spawn(&mut t, 100, r#"{"Transform":{"pos":[-5,0,0]},"CharacterBody":{},"NavAgent":{"target":[9,0,0],"active":true,"speed":4}}"#);
        t.step_n(600);
        t
    };
    assert_eq!(s.state_hash(), again.state_hash());
}

#[test]
fn agents_with_avoidance_pass_each_other_and_without_it_walk_through() {
    let run = |avoid: bool| {
        let mut s = floor();
        let extra = if avoid { r#","NavAvoid":{}"# } else { "" };
        spawn(&mut s, 10, &format!(r#"{{"Transform":{{"pos":[-8,0,0]}},"CharacterBody":{{}},"NavAgent":{{"target":[8,0,0],"active":true,"speed":4}}{extra}}}"#));
        spawn(&mut s, 11, &format!(r#"{{"Transform":{{"pos":[8,0,0]}},"CharacterBody":{{}},"NavAgent":{{"target":[-8,0,0],"active":true,"speed":4}}{extra}}}"#));
        let mut closest = f32::INFINITY;
        for _ in 0..420 {
            s.step();
            let (a, b) = (s.world.get::<Transform>(s.world.entity(10).unwrap()).unwrap().pos, s.world.get::<Transform>(s.world.entity(11).unwrap()).unwrap().pos);
            closest = closest.min(((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt());
        }
        (closest, agent(&s, 10).status, agent(&s, 11).status)
    };
    let (blind, _, _) = run(false);
    assert!(blind < 0.2, "without avoidance they walk through each other: {blind}");
    let (closest, a, b) = run(true);
    assert!(closest > 0.7, "with avoidance they keep apart: {closest}");
    assert_eq!((a, b), (2, 2), "and both get there");
}

#[test]
fn a_target_with_no_way_is_not_searched_for_over_the_whole_of_a_world_wider_than_300_m() {
    // a streamed city: a wall 994 m long with a gap at each end, a start and a goal on its two sides 10 m apart. The first search (the box round the two points) finds no way; the second look over the whole world is only for a small level
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0],"scale":[1000,1,1000]},"Collider":{"shape":1}}"#);
    wall(&mut s, 2, 5.0, -497.0, 497.0);
    let r = path(&mut s, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
    assert!(matches!(r, Err(NavError::NoPath(_))), "no way within the box, and a world of a kilometre is not searched whole: {r:?}");
    // the same wall in a world of 100 m (the existing behaviour for a small level): the way round the end is found by the second look
    let mut t = floor();
    wall(&mut t, 2, 5.0, -47.0, 47.0);
    let r = path(&mut t, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]).expect("a small world is searched whole");
    assert!(r.length > 90.0, "round the end of the wall: {}", r.length);
}

// ------------------------------------------------------------------------------------------------------------- limits for crowds

/// `n` walkers in a row that all ask for a path in the same tick.
fn crowd(s: &mut Sim, first: u64, n: u64) {
    for i in 0..n {
        spawn(s, first + i, &format!(r#"{{"Transform":{{"pos":[{},0,0]}},"CharacterBody":{{}},"NavAgent":{{"target":[0,0,20],"active":true,"speed":4}}}}"#, i as f32 * 2.0 - 4.0));
    }
}

fn planned(s: &Sim, uid: u64) -> bool {
    s.nav.paths.contains_key(&uid)
}

fn who_has_a_path(s: &Sim, uids: &[u64]) -> Vec<u64> {
    uids.iter().copied().filter(|u| planned(s, *u)).collect()
}

#[test]
fn nav_limits_ration_the_searches_to_a_few_a_tick_and_the_one_that_asked_first_goes_first() {
    // no limits: everybody that asks has a path after one tick
    let mut free = floor();
    crowd(&mut free, 100, 5);
    free.step();
    assert_eq!(who_has_a_path(&free, &[100, 101, 102, 103, 104]).len(), 5);
    // two a tick: the others keep asking (their request stays on) and are served in the next ticks
    let mut s = floor();
    spawn(&mut s, 50, r#"{"NavLimits":{"searches_per_tick":2,"min_replan":0}}"#);
    crowd(&mut s, 100, 5);
    s.step();
    assert_eq!(who_has_a_path(&s, &[100, 101, 102, 103, 104]), [100, 101]);
    assert!(agent(&s, 102).active && agent(&s, 102).status == 0, "still waiting for its turn");
    s.step();
    assert_eq!(who_has_a_path(&s, &[100, 101, 102, 103, 104]), [100, 101, 102, 103]);
    s.step();
    assert_eq!(who_has_a_path(&s, &[100, 101, 102, 103, 104]).len(), 5);
    assert!(!agent(&s, 104).active, "the request is answered");
    // first come, first served: with one search a tick, an agent with a LOW uid that asks late waits behind the ones that asked before it
    let mut q = floor();
    spawn(&mut q, 50, r#"{"NavLimits":{"searches_per_tick":1,"min_replan":0}}"#);
    crowd(&mut q, 100, 4);
    q.step();
    assert_eq!(who_has_a_path(&q, &[99, 100, 101, 102, 103]), [100]);
    spawn(&mut q, 99, r#"{"Transform":{"pos":[0,0,3]},"CharacterBody":{},"NavAgent":{"target":[0,0,20],"active":true,"speed":4}}"#);
    for expect in [vec![99u64, 100, 101], vec![99, 100, 101, 102], vec![99, 100, 101, 102, 103]] {
        q.step();
        let got = who_has_a_path(&q, &[99, 100, 101, 102, 103]);
        // 99 asked last: it is served after the four that asked before it
        assert!(!got.contains(&99) || got.len() == 5, "agent 99 was served before the agents that asked earlier: {got:?} (wanted something like {expect:?})");
    }
    q.step();
    assert_eq!(who_has_a_path(&q, &[99, 100, 101, 102, 103]).len(), 5, "everybody was served in the end");
}

#[test]
fn nav_limits_keep_an_agent_that_asks_every_tick_to_one_search_in_the_least_time() {
    let count_plans = |limits: &str| -> usize {
        let mut s = floor();
        if !limits.is_empty() {
            spawn(&mut s, 50, limits);
        }
        walker(&mut s, "[0,0,40]");
        let e = s.world.entity(100).unwrap();
        let mut plans = 0;
        for _ in 0..90 {
            s.world.get_mut::<NavAgent>(e).unwrap().active = true; // a caller that asks again at every tick
            s.step();
            if !agent(&s, 100).active {
                plans += 1;
            }
        }
        plans
    };
    assert_eq!(count_plans(""), 90, "without limits every request is a search");
    // half a second at 60 ticks a second: a search at tick 1, 31, 61 (and the 91st is not reached)
    let limited = count_plans(r#"{"NavLimits":{"searches_per_tick":0,"min_replan":0.5}}"#);
    assert!((3..=4).contains(&limited), "{limited} searches in 1.5 s with a least time of 0.5 s");
}

#[test]
fn a_search_limited_in_distance_walks_a_far_target_in_parts_and_still_arrives() {
    let mut s = floor();
    spawn(&mut s, 50, r#"{"NavLimits":{"searches_per_tick":2,"min_replan":0,"max_distance":25}}"#);
    // 90 m to go on a floor 100 m long
    spawn(&mut s, 100, r#"{"Transform":{"pos":[0,0,-45]},"CharacterBody":{},"NavAgent":{"target":[0,0,45],"active":true,"speed":6}}"#);
    s.step();
    let first = s.nav.paths.get(&100).expect("a path").points.last().copied().unwrap();
    assert!((first[2] + 45.0 - 25.0).abs() < 1.5 && first[2] < -19.0, "the first search ends about 25 m on, not at the target: {first:?}");
    assert_eq!(agent(&s, 100).status, 1);
    let mut arrived = false;
    for _ in 0..2400 {
        s.step();
        if agent(&s, 100).status == 2 {
            arrived = true;
            break;
        }
    }
    let p = pos(&s, 100);
    assert!(arrived, "never arrived: {p:?}, {:?}", agent(&s, 100));
    assert!(((p[0]).powi(2) + (p[2] - 45.0).powi(2)).sqrt() < 0.6, "arrived at {p:?}");
    // the same journey without a limit is one search
    let mut free = floor();
    spawn(&mut free, 100, r#"{"Transform":{"pos":[0,0,-45]},"CharacterBody":{},"NavAgent":{"target":[0,0,45],"active":true,"speed":6}}"#);
    free.step();
    assert!(free.nav.paths.get(&100).unwrap().points.last().unwrap()[2] > 40.0);
}

#[test]
fn limited_navigation_is_deterministic_and_comes_back_from_a_copy() {
    let build = || {
        let mut s = floor();
        spawn(&mut s, 50, r#"{"NavLimits":{"searches_per_tick":2,"min_replan":0.2,"max_distance":30}}"#);
        wall(&mut s, 60, 5.0, -5.0, 5.0);
        crowd(&mut s, 100, 7);
        s
    };
    let (mut a, mut b) = (build(), build());
    a.step_n(120);
    b.step_n(40);
    let mut c = b.clone();
    b.step_n(80);
    c.step_n(80);
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!(b.state_hash(), c.state_hash(), "a copy of the simulation carries the waiting list");
}

// ------------------------------------------------------------------------------------------------------------- searches done a few cells a step

/// A walker that has to go round a wall to a target 20 m away, with `NavLimits` (`limits`) and, when `slice` is above 0, `nav.cells_per_step`.
fn sliced_world(slice: f64, limits: &str) -> Sim {
    let mut s = floor();
    spawn(&mut s, 50, limits);
    wall(&mut s, 60, 5.0, -8.0, 8.0);
    if slice > 0.0 {
        let _ = s.world.set_resource("nav.cells_per_step", Some(slice));
    }
    spawn(&mut s, 100, r#"{"Transform":{"pos":[0,0,0]},"CharacterBody":{},"NavAgent":{"target":[20,0,0],"active":true,"speed":4}}"#);
    s
}

#[test]
fn a_search_done_a_few_cells_a_step_finds_the_path_of_the_whole_search() {
    let limits = r#"{"NavLimits":{"searches_per_tick":2,"min_replan":0}}"#;
    let mut whole = sliced_world(0.0, limits);
    whole.step();
    let want = whole.nav.paths.get(&100).expect("the whole search finds a path in a step").points.clone();
    let budget = 40usize;
    let mut sliced = sliced_world(budget as f64, limits);
    let (mut steps, mut last) = (0, 0);
    while !sliced.nav.paths.contains_key(&100) && steps < 400 {
        sliced.step();
        steps += 1;
        if let Some(j) = sliced.nav.jobs.get(&100) {
            assert!(j.expanded() - last <= budget, "a step expanded {} cells with a budget of {budget}", j.expanded() - last);
            last = j.expanded();
        }
    }
    assert!(steps > 3, "a search of some hundreds of cells does not end in {steps} steps of {budget}");
    assert!(sliced.nav.jobs.is_empty(), "the finished search is forgotten");
    assert!(!agent(&sliced, 100).active, "the request is answered when the search is over");
    // (the world did not change meanwhile, so the search that stopped and went on is the search that did not stop)
    assert_eq!(sliced.nav.paths[&100].points, want);
}

#[test]
fn a_search_that_cannot_succeed_ends_as_blocked_in_the_end_and_costs_a_few_cells_a_step() {
    // the target in a closed box
    let limits = r#"{"NavLimits":{"searches_per_tick":2,"min_replan":0,"blocked_wait":2,"max_searched":900}}"#;
    let mut s = sliced_world(60.0, limits);
    wall(&mut s, 61, 17.0, -3.0, 3.0);
    wall(&mut s, 62, 23.0, -3.0, 3.0);
    spawn(&mut s, 63, r#"{"Transform":{"pos":[20,1.5,-3],"scale":[7,3,1]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 64, r#"{"Transform":{"pos":[20,1.5,3],"scale":[7,3,1]},"Collider":{"shape":1}}"#);
    let (mut steps, mut last, mut longest) = (0, 0, 0);
    while agent(&s, 100).status != 3 && steps < 400 {
        s.step();
        steps += 1;
        if let Some(j) = s.nav.jobs.get(&100) {
            longest = longest.max(j.expanded() - last);
            last = j.expanded();
        } else {
            last = 0;
        }
    }
    assert_eq!(agent(&s, 100).status, 3, "no way in after {steps} steps");
    assert!(steps >= 10, "900 cells at 60 a step are not over in {steps} steps");
    assert!(longest <= 60, "a step expanded {longest} cells with a budget of 60");
    assert!(s.nav.jobs.is_empty());
    // the same without slicing is one step
    let mut whole = sliced_world(0.0, limits);
    wall(&mut whole, 61, 17.0, -3.0, 3.0);
    wall(&mut whole, 62, 23.0, -3.0, 3.0);
    spawn(&mut whole, 63, r#"{"Transform":{"pos":[20,1.5,-3],"scale":[7,3,1]},"Collider":{"shape":1}}"#);
    spawn(&mut whole, 64, r#"{"Transform":{"pos":[20,1.5,3],"scale":[7,3,1]},"Collider":{"shape":1}}"#);
    whole.step();
    assert_eq!(agent(&whole, 100).status, 3);
}

#[test]
fn searches_done_a_few_cells_a_step_are_deterministic_and_come_back_from_a_copy() {
    let build = || {
        let mut s = sliced_world(25.0, r#"{"NavLimits":{"searches_per_tick":2,"min_replan":0.2,"max_distance":30}}"#);
        crowd(&mut s, 101, 5);
        s
    };
    let (mut a, mut b) = (build(), build());
    a.step_n(200);
    b.step_n(6);
    assert!(!b.nav.jobs.is_empty(), "a search is on when the copy is made");
    let mut c = b.clone();
    b.step_n(194);
    c.step_n(194);
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!(b.state_hash(), c.state_hash(), "a copy of the simulation carries the searches that are on");
}

// ------------------------------------------------------------------------------------------------------------- the grid is worked out lazily: the same paths as a grid cut whole

/// A building of triangles: four walls and a roof, every face cut into `n` by `n` squares.
fn shell_mesh(side: f32, height: f32, n: usize) -> crate::meshcol::CollisionMesh {
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut face = |o: [f32; 3], u: [f32; 3], v: [f32; 3]| {
        let base = verts.len() as u32;
        for j in 0..=n {
            for i in 0..=n {
                let (a, b) = (i as f32 / n as f32, j as f32 / n as f32);
                verts.push([o[0] + u[0] * a + v[0] * b, o[1] + u[1] * a + v[1] * b, o[2] + u[2] * a + v[2] * b]);
            }
        }
        let w = n as u32 + 1;
        for j in 0..n as u32 {
            for i in 0..n as u32 {
                let p = base + j * w + i;
                tris.push([p, p + 1, p + w]);
                tris.push([p + 1, p + w + 1, p + w]);
            }
        }
    };
    face([0.0, 0.0, 0.0], [side, 0.0, 0.0], [0.0, height, 0.0]);
    face([0.0, 0.0, side], [side, 0.0, 0.0], [0.0, height, 0.0]);
    face([0.0, 0.0, 0.0], [0.0, 0.0, side], [0.0, height, 0.0]);
    face([side, 0.0, 0.0], [0.0, 0.0, side], [0.0, height, 0.0]);
    face([0.0, height, 0.0], [side, 0.0, 0.0], [0.0, 0.0, side]);
    crate::meshcol::CollisionMesh::new(verts, tris).unwrap()
}

/// A scene with every kind of thing a search meets: a floor, walls and a stair of boxes, a raised platform with a link to the ground, a spherical obstacle, and a town of
/// 16 triangle shells; and 150 searches over it by agents of three sizes. Everything about every path, bit for bit, is hashed.
fn path_fingerprint() -> u64 {
    path_fingerprint_keeping(false)
}

/// The same, with the agents' searches keeping their cells (`NavParams.keep`) or not.
fn path_fingerprint_keeping(keep: bool) -> u64 {
    let mut s = floor();
    wall(&mut s, 2, 10.0, -20.0, 8.0);
    wall(&mut s, 3, -15.0, -8.0, 30.0);
    for i in 0..5 {
        spawn(&mut s, 10 + i, &format!(r#"{{"Transform":{{"pos":[30,{},{}],"scale":[3,{},2]}},"Collider":{{"shape":1}}}}"#, 0.2 * (i + 1) as f32 - 0.5 + 0.1, -4.0 + i as f32 * 2.0, 0.4 * (i + 1) as f32));
    }
    spawn(&mut s, 20, r#"{"Transform":{"pos":[-30,1.0,-30],"scale":[8,0.5,8]},"Collider":{"shape":1}}"#);
    spawn(&mut s, 21, r#"{"Transform":{"pos":[-20,0,-30]},"NavLink":{"from":[-3,0,0],"to":[-8,1.25,0],"two_way":true}}"#);
    spawn(&mut s, 22, r#"{"Transform":{"pos":[0,1,20]},"Collider":{"shape":0,"size":[4,4,4]}}"#);
    std::sync::Arc::make_mut(&mut s.meshes).by_name.insert("shell".to_string(), std::sync::Arc::new(shell_mesh(10.0, 8.0, 6)));
    for gz in 0..4 {
        for gx in 0..4 {
            spawn(&mut s, 100 + gz * 4 + gx, &format!(r#"{{"Transform":{{"pos":[{},0,{}]}},"MeshCollider":{{"mesh":"shell"}}}}"#, -45.0 + gx as f32 * 22.0, -45.0 + gz as f32 * 22.0));
        }
    }
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32
    };
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for k in 0..150 {
        let (radius, height, step) = [(0.4, 1.8, 0.4), (0.8, 2.2, 0.5), (0.25, 1.0, 0.3)][k % 3];
        let params = NavParams { radius, height, step_height: step, keep, ..NavParams::default() };
        let from = [next() * 90.0 - 45.0, 0.0, next() * 90.0 - 45.0];
        let to = [next() * 90.0 - 45.0, 0.0, next() * 90.0 - 45.0];
        match nav::find_path_with(&mut s.world, from, to, params, &s.heightfields, &s.meshes) {
            Ok(r) => {
                eat(1);
                eat(r.points.len() as u64);
                for p in &r.points {
                    for v in p {
                        eat(v.to_bits() as u64);
                    }
                }
                for j in &r.jumps {
                    eat(j.map(|a| a[0].to_bits() as u64 ^ (a[1].to_bits() as u64) << 32).unwrap_or(7));
                }
                eat(r.length.to_bits() as u64);
                eat(r.exact as u64);
                eat(r.searched as u64);
                eat(r.cell.to_bits() as u64);
            }
            Err(e) => {
                eat(2);
                for b in e.code().bytes() {
                    eat(b as u64);
                }
            }
        }
    }
    h
}

#[test]
fn cells_kept_between_searches_give_exactly_the_paths_of_cells_worked_out_fresh() {
    nav::keep_cells(false);
    let fresh = path_fingerprint_keeping(true);
    assert_eq!(nav::kept_cells(), 0, "nothing is kept while it is off");
    nav::keep_cells(true);
    let first = path_fingerprint_keeping(true);
    assert!(nav::kept_cells() > 1000, "the searches left their cells behind: {}", nav::kept_cells());
    // (the second run finds most of its cells made already)
    let second = path_fingerprint_keeping(true);
    // and a search that does not ask for it keeps nothing and leaves the corner of its box as the corner of its window
    nav::keep_cells(true);
    let before = nav::kept_cells();
    let _ = path_fingerprint_keeping(false);
    assert_eq!(nav::kept_cells(), before, "a search without `keep` leaves no cells");
    assert_eq!(fresh, first, "kept or fresh, the same paths");
    assert_eq!(first, second, "and the same again over the cells that were kept");
}

#[test]
fn a_wall_built_after_a_search_is_not_walked_through_by_the_next_one() {
    nav::keep_cells(true);
    let mut s = floor();
    let kept = NavParams { keep: true, ..NavParams::default() };
    let path = |s: &mut Sim, from: [f32; 3], to: [f32; 3]| find_path(&mut s.world, from, to, kept);
    let before = path(&mut s, [0.0, 0.0, 0.0], [20.0, 0.0, 0.0]).unwrap();
    assert!(before.length < 20.5, "straight: {}", before.length);
    let kept = nav::kept_cells();
    assert!(kept > 10, "{kept}");
    // a wall across the way, built after the cells were kept
    wall(&mut s, 2, 10.0, -6.0, 6.0);
    let after = path(&mut s, [0.0, 0.0, 0.0], [20.0, 0.0, 0.0]).unwrap();
    assert!(after.length > 24.0, "round the wall, not through it: {}", after.length);
    assert!(after.points.iter().all(|p| !(p[0] > 9.4 && p[0] < 10.6 && p[2].abs() < 6.0)), "{:?}", after.points);
    // and taking it away again gives the straight way back
    s.apply(&Command::Despawn { uid: 2 }).unwrap();
    let gone = path(&mut s, [0.0, 0.0, 0.0], [20.0, 0.0, 0.0]).unwrap();
    assert!(gone.length < 20.5, "straight again: {}", gone.length);
}

#[test]
fn the_lazy_grid_gives_the_paths_of_a_grid_cut_whole() {
    let fingerprint = path_fingerprint();
    eprintln!("nav path fingerprint: {fingerprint:016x}");
    // recorded 2026-10-25 from the grid that was cut whole over the window (before cells were worked out lazily): every path, jump, length, search count and error is the same
    const EXPECTED: u64 = 0x14d88eabbaded9d4;
    if EXPECTED != 0 {
        assert_eq!(fingerprint, EXPECTED, "the paths over the fixed scene changed");
    }
}

/// A closed square room of walls 9 m a side round the place (x, 0, 0), a target that no agent can reach.
fn closed_room(s: &mut Sim, x: f32) {
    spawn(s, 20, &format!(r#"{{"Transform":{{"pos":[{x},1.5,5],"scale":[10,3,1]}},"Collider":{{"shape":1}}}}"#));
    spawn(s, 21, &format!(r#"{{"Transform":{{"pos":[{x},1.5,-5],"scale":[10,3,1]}},"Collider":{{"shape":1}}}}"#));
    wall(s, 22, x - 5.0, -5.5, 5.5);
    wall(s, 23, x + 5.0, -5.5, 5.5);
}

#[test]
fn an_agent_whose_search_found_no_way_waits_before_it_asks_again() {
    let plans = |limits: &str| -> usize {
        let mut s = floor();
        closed_room(&mut s, 20.0);
        spawn(&mut s, 50, limits);
        walker(&mut s, "[20,0,0]");
        let e = s.world.entity(100).unwrap();
        let mut plans = 0;
        for _ in 0..60 {
            s.world.get_mut::<NavAgent>(e).unwrap().active = true; // the autopilot of a chaser asks again at every tick
            s.step();
            if !agent(&s, 100).active {
                plans += 1;
            }
        }
        plans
    };
    let every = plans(r#"{"NavLimits":{"searches_per_tick":0,"min_replan":0,"blocked_wait":0}}"#);
    assert!(every >= 55, "with no wait every request is a (failing) search: {every}");
    let waiting = plans(r#"{"NavLimits":{"searches_per_tick":0,"min_replan":0,"blocked_wait":2.0}}"#);
    assert_eq!(waiting, 1, "one failing search, then two seconds of waiting (its request stays on): {waiting}");
}

#[test]
fn a_search_may_be_capped_so_that_a_failure_costs_a_few_cells() {
    let mut s = floor();
    closed_room(&mut s, 20.0);
    let mut msg = |cap: usize| match nav::find_path_capped(&mut s.world, [0.0, 0.0, 0.0], [20.0, 0.0, 0.0], NavParams::default(), &s.heightfields, &s.meshes, None, cap) {
        Err(NavError::NoPath(m)) => m,
        other => panic!("a closed room has no way in: {other:?}"),
    };
    let whole = msg(0);
    assert!(whole.contains("searched") && !whole.contains("gave up"), "{whole}");
    let capped = msg(100);
    assert!(capped.contains("gave up after 100 cells"), "{capped}");
    // a way that exists and needs fewer cells than the cap is found as ever
    let r = nav::find_path_capped(&mut s.world, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0], NavParams::default(), &s.heightfields, &s.meshes, None, 2000);
    assert!(r.is_ok(), "{r:?}");
}
