//! Flying machines: going to a place, holding a height over uneven ground, steering round a pillar, following and circling a target, patrolling, being driven by `Perception`, keeping apart. No art: boxes.
use super::*;
use crate::components::{FlyAgent, Perception, Transform};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// 60 ticks a second, the ground a big box with its top at y = 0.
fn world() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[1000,1,1000]}}"#);
    s
}

fn drone(s: &mut Sim, uid: u64, pos: [f32; 3], agent: &str) {
    let sep = if agent.is_empty() { "" } else { "," };
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{},{},{}]}},"Velocity":{{}},"FlyAgent":{{{agent}{sep}}}}}"#, pos[0], pos[1], pos[2]).replace(",}", "}"));
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn agent(s: &Sim, uid: u64) -> FlyAgent {
    *s.world.get::<FlyAgent>(s.world.entity(uid).unwrap()).unwrap()
}

fn dist_xz(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[test]
fn it_goes_where_it_is_told_stops_there_and_holds_its_height() {
    let mut s = world();
    drone(&mut s, 1, [0.0, 6.0, 0.0], r#""mode":1,"goal":[30,0,0],"speed":10,"hover":6"#);
    s.step_n(60 * 8);
    let p = pos(&s, 1);
    assert!(dist_xz(p, [30.0, 0.0, 0.0]) < 2.5, "{p:?}");
    assert!((p[1] - 6.0).abs() < 0.6, "it holds 6 m above the ground: {p:?}");
    let a = agent(&s, 1);
    assert!(a.arrived && a.speed_now < 1.5, "{a:?}");
    assert!((a.altitude - 6.0).abs() < 0.6, "{}", a.altitude);
    // it faced the way it went (-z is forward: going +x is a turn to the right, about -90 degrees)
    let yaw = crate::building::yaw_of(s.world.get::<Transform>(s.world.entity(1).unwrap()).unwrap().rot);
    assert!((yaw + 90.0).abs() < 8.0, "{yaw}");
}

#[test]
fn the_hover_follows_the_ground_up_and_down() {
    let mut s = world();
    // a platform 4 m high from x = 10 on
    spawn(&mut s, 3, r#"{"Transform":{"pos":[60,2,0]},"Collider":{"shape":1,"size":[100,4,40]}}"#);
    drone(&mut s, 1, [0.0, 6.0, 0.0], r#""mode":1,"goal":[40,0,0],"speed":8,"hover":6,"avoid":0"#);
    s.step_n(60 * 8);
    let p = pos(&s, 1);
    assert!((p[1] - 10.0).abs() < 0.8, "over the platform it is 6 m above the platform: {p:?}");
    assert!((agent(&s, 1).altitude - 6.0).abs() < 0.8);
}

#[test]
fn it_steers_round_a_pillar_in_its_way_and_still_gets_there() {
    let mut s = world();
    spawn(&mut s, 3, r#"{"Transform":{"pos":[15,20,0]},"Collider":{"shape":1,"size":[2,40,2]}}"#);
    drone(&mut s, 1, [0.0, 6.0, 0.0], r#""mode":1,"goal":[30,0,0],"speed":10,"hover":6,"avoid":10"#);
    let mut closest = f32::MAX;
    let mut blocked_seen = false;
    for _ in 0..(60 * 12) {
        s.step();
        let p = pos(&s, 1);
        // the pillar's footprint is 2 x 2: how far outside it (on the ground plane) the drone is
        let out = ((p[0] - 15.0).abs() - 1.0).max(((p[2]).abs() - 1.0).max(0.0)).max(0.0);
        if (p[0] - 15.0).abs() < 1.0 && p[2].abs() < 1.0 {
            closest = 0.0;
        } else {
            closest = closest.min(out.max(0.0));
        }
        blocked_seen |= agent(&s, 1).blocked;
    }
    assert!(blocked_seen, "it saw the pillar");
    assert!(closest > 0.0, "it never went through the pillar");
    assert!(dist_xz(pos(&s, 1), [30.0, 0.0, 0.0]) < 3.0, "{:?}", pos(&s, 1));
}

#[test]
fn it_follows_a_target_at_a_distance_and_circles_it() {
    let mut s = world();
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,0,-40]},"Velocity":{"lin":[2,0,0]}}"#);
    drone(&mut s, 1, [0.0, 6.0, 0.0], r#""mode":2,"target":5,"range":10,"speed":10"#);
    s.step_n(60 * 20);
    let d = dist_xz(pos(&s, 1), pos(&s, 5));
    assert!((d - 10.0).abs() < 3.0, "it keeps the range: {d}");
    // circling a thing that stands still: it stays on the ring and goes round it
    let mut s = world();
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,0,-40]}}"#);
    drone(&mut s, 1, [0.0, 6.0, 0.0], r#""mode":3,"target":5,"range":10,"orbit":6,"speed":10"#);
    s.step_n(60 * 8);
    let start = {
        let p = pos(&s, 1);
        (p[2] + 40.0).atan2(p[0])
    };
    let mut swept = 0.0f32;
    let mut last = start;
    for _ in 0..(60 * 10) {
        s.step();
        let p = pos(&s, 1);
        let a = (p[2] + 40.0).atan2(p[0]);
        let mut d = a - last;
        while d > std::f32::consts::PI {
            d -= std::f32::consts::TAU;
        }
        while d < -std::f32::consts::PI {
            d += std::f32::consts::TAU;
        }
        swept += d;
        last = a;
        let r = dist_xz(p, pos(&s, 5));
        assert!((r - 10.0).abs() < 3.5, "on the ring: {r}");
    }
    assert!(swept.abs() > 3.0, "it went round it: {swept} radians in ten seconds");
    // the other way with a negative speed
    let mut s2 = world();
    spawn(&mut s2, 5, r#"{"Transform":{"pos":[0,0,-40]}}"#);
    drone(&mut s2, 1, [0.0, 6.0, 0.0], r#""mode":3,"target":5,"range":10,"orbit":-6,"speed":10"#);
    s2.step_n(60 * 8);
    let mut swept2 = 0.0f32;
    let mut last2 = {
        let p = pos(&s2, 1);
        (p[2] + 40.0).atan2(p[0])
    };
    for _ in 0..(60 * 10) {
        s2.step();
        let p = pos(&s2, 1);
        let a = (p[2] + 40.0).atan2(p[0]);
        let mut d = a - last2;
        while d > std::f32::consts::PI {
            d -= std::f32::consts::TAU;
        }
        while d < -std::f32::consts::PI {
            d += std::f32::consts::TAU;
        }
        swept2 += d;
        last2 = a;
    }
    assert!(swept * swept2 < 0.0, "opposite ways: {swept} and {swept2}");
}

#[test]
fn it_patrols_its_points_and_waits_at_each() {
    let mut s = world();
    let text = "0 0 -20\\n30 0 -20 # the second\\n";
    drone(&mut s, 1, [0.0, 6.0, 0.0], &format!(r#""mode":4,"patrol":"{text}","wait":1.0,"speed":10"#));
    let mut visits: Vec<u32> = Vec::new();
    let mut waited = 0;
    for _ in 0..(60 * 20) {
        s.step();
        let a = agent(&s, 1);
        if visits.last() != Some(&a.patrol_at) {
            visits.push(a.patrol_at);
        }
        if a.wait_left > 0.0 {
            waited += 1;
        }
    }
    assert!(visits.len() >= 3 && visits[..3] == [0, 1, 0], "it goes to the first, the second, the first again: {visits:?}");
    assert!(waited > 60, "it waits a second at each point: {waited} ticks of waiting");
}

#[test]
fn its_perception_chooses_calm_it_patrols_a_noise_it_goes_to_the_target_it_sees_it_circles() {
    // a drone that is calm patrols
    let mut s = world();
    spawn(&mut s, 7, r#"{"Transform":{"pos":[0,6,0]},"Velocity":{},"Team":{"id":2},"Perception":{"sight_fov":360,"sight_range":60,"hearing":1},"FlyAgent":{"perception":true,"patrol":"0 0 -30\n40 0 -30\n","speed":10,"range":12,"orbit":5}}"#);
    s.step_n(60 * 10);
    assert!(dist_xz(pos(&s, 7), [0.0, 0.0, -30.0]) < 15.0 || dist_xz(pos(&s, 7), [40.0, 0.0, -30.0]) < 15.0, "it is on its patrol: {:?}", pos(&s, 7));
    assert!(agent(&s, 7).patrol_at <= 1);
    // a noise far off: it searches there
    let mut s = world();
    spawn(&mut s, 7, r#"{"Transform":{"pos":[0,6,0]},"Velocity":{},"Team":{"id":2},"Perception":{"sight_fov":360,"sight_range":10,"hearing":1,"lose_time":30},"FlyAgent":{"perception":true,"speed":10}}"#);
    spawn(&mut s, 8, r#"{"Transform":{"pos":[0,0,-50]},"Noisy":{"radius":80},"Team":{"id":1}}"#);
    s.step_n(60 * 12);
    assert!(dist_xz(pos(&s, 7), [0.0, 0.0, -50.0]) < 4.0, "it went to where it heard the noise: {:?}", pos(&s, 7));
    // a hostile in sight: it circles it
    let mut s = world();
    spawn(&mut s, 7, r#"{"Transform":{"pos":[0,6,0]},"Velocity":{},"Team":{"id":2},"Perception":{"sight_fov":360,"sight_range":60,"notice_time":0.2,"los":false},"FlyAgent":{"perception":true,"speed":10,"range":12,"orbit":5}}"#);
    spawn(&mut s, 8, r#"{"Transform":{"pos":[0,0.9,-30]},"Collider":{"shape":2,"size":[0.7,1.8,0.7]},"HitPoints":{"value":100,"max":100},"Team":{"id":1}}"#);
    s.step_n(60 * 10);
    let p = *s.world.get::<Perception>(s.world.entity(7).unwrap()).unwrap();
    assert_eq!(p.state, 2, "{p:?}");
    let d = dist_xz(pos(&s, 7), pos(&s, 8));
    assert!((d - 12.0).abs() < 4.0, "it holds the ring round the target: {d}");
}

#[test]
fn agents_on_the_same_spot_move_apart_and_an_inactive_one_is_left_alone() {
    let mut s = world();
    drone(&mut s, 11, [0.0, 6.0, 0.0], r#""separation":6"#);
    drone(&mut s, 12, [0.0, 6.0, 0.0], r#""separation":6"#);
    drone(&mut s, 13, [50.0, 6.0, 0.0], r#""active":false,"mode":1,"goal":[0,0,0]"#);
    s.step_n(60 * 4);
    assert!(dist_xz(pos(&s, 11), pos(&s, 12)) > 4.5, "{:?} {:?}", pos(&s, 11), pos(&s, 12));
    assert_eq!(pos(&s, 13), [50.0, 6.0, 0.0], "an inactive agent does nothing");
}

#[test]
fn flying_is_deterministic() {
    let run = || {
        let mut s = world();
        spawn(&mut s, 3, r#"{"Transform":{"pos":[15,20,0]},"Collider":{"shape":1,"size":[2,40,2]}}"#);
        drone(&mut s, 11, [0.0, 6.0, 0.0], r#""mode":1,"goal":[30,0,0],"speed":10,"avoid":10"#);
        drone(&mut s, 12, [0.0, 6.0, 1.0], r#""mode":3,"target":11,"range":8,"orbit":4"#);
        s.step_n(60 * 6);
        s.world.state_hash()
    };
    assert_eq!(run(), run());
}

#[test]
fn a_toolkit_that_updates_every_few_steps_is_due_in_one_step_of_them_with_their_time_and_odd_values_mean_every_step() {
    use crate::gameplay::due;
    let mut s = Sim::new(1);
    let dt = 1.0 / 60.0;
    assert_eq!(due(&s.world, "gameplay.every.x", 5, dt, 0), Some(dt), "no resource: every step");
    s.world.set_resource("gameplay.every.x", Some(3.0)).unwrap();
    let runs: Vec<u64> = (0..9).filter(|t| due(&s.world, "gameplay.every.x", *t, dt, 0).is_some()).collect();
    assert_eq!(runs, [0, 3, 6]);
    assert_eq!(due(&s.world, "gameplay.every.x", 3, dt, 0), Some(dt * 3.0), "it is given the time of the steps it stands for");
    let staggered: Vec<u64> = (0..9).filter(|t| due(&s.world, "gameplay.every.x", *t, dt, 1).is_some()).collect();
    assert_eq!(staggered, [2, 5, 8], "a phase puts it in another step");
    for odd in [0.0, 0.5, -4.0] {
        s.world.set_resource("gameplay.every.x", Some(odd)).unwrap();
        assert_eq!(due(&s.world, "gameplay.every.x", 7, dt, 0), Some(dt), "{odd} means every step");
    }
    s.world.set_resource("gameplay.every.x", Some(1.0e12)).unwrap();
    assert_eq!(due(&s.world, "gameplay.every.x", 600, dt, 0), Some(dt * 600.0), "at most every 600 steps");
}

#[test]
fn a_drone_that_decides_every_second_step_changes_its_state_in_half_the_steps_and_arrives_all_the_same() {
    let build = |every: Option<f64>| {
        let mut s = world();
        drone(&mut s, 1, [0.0, 6.0, 0.0], r#""mode":1,"goal":[30,0,0],"speed":10,"hover":6"#);
        if let Some(e) = every {
            s.world.set_resource("gameplay.every.flyagent", Some(e)).unwrap();
        }
        s
    };
    let (mut every, mut half) = (build(None), build(Some(2.0)));
    let (mut changes_every, mut changes_half) = (0, 0);
    let (mut last_every, mut last_half) = (agent(&every, 1).speed_now, agent(&half, 1).speed_now);
    for _ in 0..60 * 8 {
        every.step_n(1);
        half.step_n(1);
        let (a, b) = (agent(&every, 1).speed_now, agent(&half, 1).speed_now);
        changes_every += (a != last_every) as u32;
        changes_half += (b != last_half) as u32;
        (last_every, last_half) = (a, b);
    }
    assert!(changes_half <= 60 * 8 / 2 + 1, "its own state changes only in the steps it is due in: {changes_half}");
    assert!(changes_every > changes_half + 100, "{changes_every} against {changes_half}");
    for (name, s) in [("every step", &every), ("every second step", &half)] {
        let p = pos(s, 1);
        assert!(dist_xz(p, [30.0, 0.0, 0.0]) < 2.5, "{name}: {p:?}");
        assert!((p[1] - 6.0).abs() < 0.8, "{name} holds its height: {p:?}");
        assert!(agent(s, 1).arrived, "{name}");
    }
}
