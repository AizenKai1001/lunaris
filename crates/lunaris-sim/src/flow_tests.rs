//! Lifetimes, spawners, the day and night cycle and the time scale.
use super::*;
use components::{Environment, Light, PrefabSpawner, TimeOfDay, Transform, Velocity};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn alive(s: &Sim, uid: u64) -> bool {
    s.world.entity(uid).is_some()
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

#[test]
fn a_lifetime_ends_the_entity_and_what_is_attached_to_it() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{},"Lifetime":{"seconds":0.5}}"#);
    spawn(&mut s, 2, r#"{"Transform":{},"Parent":{"target":1}}"#);
    spawn(&mut s, 3, r#"{"Transform":{},"Parent":{"target":2}}"#);
    spawn(&mut s, 4, r#"{"Transform":{},"Lifetime":{"seconds":2}}"#);
    s.step_n(20);
    assert!(alive(&s, 1) && alive(&s, 2) && alive(&s, 3));
    s.step_n(12);
    assert!(!alive(&s, 1) && !alive(&s, 2) && !alive(&s, 3), "the child and the grandchild went with it");
    assert!(alive(&s, 4));
    s.step_n(100);
    assert!(!alive(&s, 4));
}

#[test]
fn a_spawner_makes_prefab_copies_in_place_aimed_and_within_its_limits() {
    let mut s = Sim::new(1);
    s.apply(&Command::DefinePrefab { name: "Bolt".into(), def: parse(r#"{"components":{"Name":{"value":"Bolt"},"Transform":{},"Lifetime":{"seconds":1}}}"#).unwrap() }).unwrap();
    // a turret at (5, 1, 5) that fires every half second at 10 m/s along its forward direction (-z, as it stands), at most 3 at a time
    spawn(&mut s, 1, r#"{"Transform":{"pos":[5,1,5]},"PrefabSpawner":{"prefab":"Bolt","interval":0.5,"max_alive":3,"speed":10}}"#);
    s.step();
    let bolts = |s: &Sim| (2..40u64).filter(|u| alive(s, *u)).count();
    assert_eq!(bolts(&s), 1);
    let v = s.world.get::<Velocity>(s.world.entity(2).unwrap()).unwrap().lin;
    assert!(v[0].abs() < 1e-4 && v[1].abs() < 1e-4 && (v[2] + 10.0).abs() < 1e-3, "{v:?}");
    assert_eq!(pos(&s, 2), [5.0, 1.0, 5.0]);
    s.step_n(33);
    assert_eq!(bolts(&s), 2);
    s.step_n(30);
    assert!(bolts(&s) <= 3, "never more than max_alive: {}", bolts(&s));
    // the copies expire by their lifetime, the spawner goes on; a total limit stops it
    s.world.get_mut::<PrefabSpawner>(s.world.entity(1).unwrap()).unwrap().limit = 5;
    s.step_n(600);
    assert_eq!(s.world.get::<PrefabSpawner>(s.world.entity(1).unwrap()).unwrap().spawned, 5);
    // a spread over a disc, deterministic
    let run = || {
        let mut t = Sim::new(1);
        t.apply(&Command::DefinePrefab { name: "Rock".into(), def: parse(r#"{"components":{"Transform":{}}}"#).unwrap() }).unwrap();
        spawn(&mut t, 1, r#"{"Transform":{"pos":[0,0,0]},"PrefabSpawner":{"prefab":"Rock","interval":0.1,"count":4,"radius":5,"limit":40}}"#);
        t.step_n(90);
        t
    };
    let (a, b) = (run(), run());
    assert_eq!(a.state_hash(), b.state_hash());
    let mut far = 0;
    for u in 2..60u64 {
        if alive(&a, u) {
            let p = pos(&a, u);
            assert!((p[0] * p[0] + p[2] * p[2]).sqrt() <= 5.001, "{p:?}");
            far += (p[0].abs() > 1.0) as u32;
        }
    }
    assert!(far > 5, "they are spread out");
    // an unknown prefab spawns nothing and does not stop the world
    let mut n = Sim::new(1);
    spawn(&mut n, 99, r#"{"Transform":{},"PrefabSpawner":{"prefab":"Nope"}}"#);
    n.step_n(5);
    assert_eq!(n.world.get::<PrefabSpawner>(n.world.entity(99).unwrap()).unwrap().spawned, 0);
}

#[test]
fn the_day_turns_the_sun_and_the_sky() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Environment":{}}"#);
    spawn(&mut s, 2, r#"{"Transform":{},"Light":{"kind":0,"intensity":1}}"#);
    spawn(&mut s, 3, r#"{"TimeOfDay":{"time":12,"day_seconds":0,"sun_intensity":4,"night_intensity":0.1}}"#);
    let sun = |s: &Sim| -> ([f32; 3], f32, [f32; 3]) {
        let e = s.world.entity(2).unwrap();
        let q = s.world.get::<Transform>(e).unwrap().rot;
        let fwd = [-(2.0 * (q[0] * q[2] + q[3] * q[1])), -(2.0 * (q[1] * q[2] - q[3] * q[0])), -(1.0 - 2.0 * (q[0] * q[0] + q[1] * q[1]))];
        let l = s.world.get::<Light>(e).unwrap();
        (fwd, l.intensity, s.world.get::<Environment>(s.world.entity(1).unwrap()).unwrap().sky_top)
    };
    s.step();
    let (f, i, top) = sun(&s);
    assert!(f[1] < -0.95, "at noon the sun shines almost straight down: {f:?}");
    assert!((i - 4.0).abs() < 0.01, "{i}");
    assert!(top[2] > 0.8, "a blue sky: {top:?}");
    s.world.get_mut::<TimeOfDay>(s.world.entity(3).unwrap()).unwrap().time = 0.0;
    s.step();
    let (f, i, top) = sun(&s);
    assert!(f[1] > 0.9, "at midnight the 'sun' is below the ground, so its light points up: {f:?}");
    assert!((i - 0.1).abs() < 0.01 && top[2] < 0.1, "{i} {top:?}");
    // 6 in the morning: low sun, warm
    s.world.get_mut::<TimeOfDay>(s.world.entity(3).unwrap()).unwrap().time = 7.0;
    s.step();
    let e = s.world.entity(2).unwrap();
    let l = *s.world.get::<Light>(e).unwrap();
    assert!(l.color[0] > l.color[2] + 0.2, "warm light at a low sun: {:?}", l.color);
    // time passes: 240 s a day, so 10 s are an hour
    s.world.get_mut::<TimeOfDay>(s.world.entity(3).unwrap()).unwrap().day_seconds = 240.0;
    s.world.get_mut::<TimeOfDay>(s.world.entity(3).unwrap()).unwrap().time = 7.0;
    s.step_n(600);
    let t = s.world.get::<TimeOfDay>(s.world.entity(3).unwrap()).unwrap().time;
    assert!((t - 8.0).abs() < 0.01, "{t}");
    // and it wraps around
    s.world.get_mut::<TimeOfDay>(s.world.entity(3).unwrap()).unwrap().time = 23.99;
    s.step_n(10);
    assert!(s.world.get::<TimeOfDay>(s.world.entity(3).unwrap()).unwrap().time < 1.0);
}

#[test]
fn the_time_scale_slows_pauses_and_speeds_the_world_up() {
    let build = || {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{},"Velocity":{"lin":[0,0,0]},"Tween":{"path":"Transform.pos[0]","to":10,"duration":2}}"#);
        spawn(&mut s, 2, r#"{"Transform":{},"Lifetime":{"seconds":5}}"#);
        s
    };
    let x = |s: &Sim| pos(s, 1)[0];
    let mut s = build();
    s.step_n(60);
    assert!((x(&s) - 5.0).abs() < 0.1, "{}", x(&s));
    s.world.set_resource("time.scale", Some(0.0)).unwrap();
    s.step_n(120);
    assert!((x(&s) - 5.0).abs() < 0.1, "paused: {}", x(&s));
    assert!(alive(&s, 2), "a lifetime does not run out while paused");
    s.world.set_resource("time.scale", Some(0.5)).unwrap();
    s.step_n(60);
    assert!((x(&s) - 7.5).abs() < 0.15, "slow motion: {}", x(&s));
    s.world.set_resource("time.scale", Some(4.0)).unwrap();
    s.step_n(30);
    assert!((x(&s) - 10.0).abs() < 0.01, "fast forward finishes the tween: {}", x(&s));
    assert_eq!(s.state_hash(), {
        let mut t = build();
        t.step_n(60);
        t.world.set_resource("time.scale", Some(0.0)).unwrap();
        t.step_n(120);
        t.world.set_resource("time.scale", Some(0.5)).unwrap();
        t.step_n(60);
        t.world.set_resource("time.scale", Some(4.0)).unwrap();
        t.step_n(30);
        t.state_hash()
    });
}
