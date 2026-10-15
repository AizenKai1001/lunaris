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
