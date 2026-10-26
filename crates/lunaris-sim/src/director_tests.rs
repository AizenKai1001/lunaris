//! The AI director: heat from a searchlight, from gunfire and from being seen; waves of hunters from a prefab; cooldown, caps and clean-up. The hunters are bare prefabs of components: no art.
use super::*;
use crate::components::{Director, Heat, Perception, SpawnedBy, Transform};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

const HUNTER: &str = r#"{"components":{"Name":{"value":"Hunter"},"Transform":{},"Team":{"id":2},"Perception":{"lose_time":1.0,"notice_time":0.5,"sight_range":10}}}"#;

fn world() -> Sim {
    let mut s = Sim::new(1);
    s.apply(&Command::DefinePrefab { name: "Hunter".into(), def: parse(HUNTER).unwrap() }).unwrap();
    s
}

/// A player (team 1, a capsule) at (0, 0.9, -20) that the director tracks.
fn player(s: &mut Sim, uid: u64, extra: &str) {
    let sep = if extra.is_empty() { "" } else { "," };
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[0,0.9,-20]}},"Collider":{{"shape":2,"size":[0.7,1.8,0.7]}},"HitPoints":{{"value":100,"max":100}},"Team":{{"id":1}},"Heat":{{}}{sep}{extra}}}"#));
}

fn director(s: &mut Sim, uid: u64, extra: &str) {
    let sep = if extra.is_empty() { "" } else { "," };
    spawn(s, uid, &format!(r#"{{"Director":{{"prefab":"Hunter"{sep}{extra}}}}}"#));
}

fn hunters(s: &mut Sim) -> Vec<(u64, [f32; 3])> {
    let mut v: Vec<(u64, [f32; 3])> = Vec::new();
    let _ = s.world.for_each_uid::<(&SpawnedBy, &Transform), _>(|_, uid, (_, t)| v.push((uid, t.pos)));
    v.sort_by_key(|h| h.0);
    v
}

fn heat(s: &Sim, uid: u64) -> Heat {
    *s.world.get::<Heat>(s.world.entity(uid).unwrap()).unwrap()
}

fn dir_of(s: &Sim, uid: u64) -> Director {
    *s.world.get::<Director>(s.world.entity(uid).unwrap()).unwrap()
}

fn set_heat(s: &mut Sim, uid: u64, v: f32) {
    let e = s.world.entity(uid).unwrap();
    s.world.get_mut::<Heat>(e).unwrap().value = v;
}

#[test]
fn a_searchlight_that_catches_a_player_brings_hunters_from_the_prefab_on_a_ring() {
    let mut s = world();
    // the Core's beam: it holds a player for a second to detect it; the director turns detection into heat
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.6,0]},"Team":{"id":2},"Searchlight":{"range":60,"angle":8,"hold":1.0}}"#);
    player(&mut s, 2, "");
    director(&mut s, 3, r#""light_heat":1.0,"min_dist":30,"max_dist":40,"cooldown":10,"max_alive":4"#);
    s.step_n(60);
    assert!(hunters(&mut s).is_empty(), "a second in the light is detection, not yet a wave");
    s.step_n(120);
    let h = hunters(&mut s);
    assert!(!h.is_empty(), "the player has been in the light for 3 s: hunters came");
    let p = [0.0f32, 0.9, -20.0];
    for (uid, pos) in &h {
        let d = ((pos[0] - p[0]).powi(2) + (pos[2] - p[2]).powi(2)).sqrt();
        assert!(d >= 29.0 && d <= 41.0, "hunter {uid} stands on the ring round the player: {d}");
        let e = s.world.entity(*uid).unwrap();
        let per = *s.world.get::<Perception>(e).unwrap();
        assert!(per.target.0 == 2 || per.state == 0, "it was sent for the player: {per:?}");
    }
    let d = dir_of(&s, 3);
    assert!(d.waves >= 1 && d.sent as usize >= h.len() && d.alive as usize == h.len(), "{d:?}");
    assert!(heat(&s, 2).waves >= 1);
}

#[test]
fn a_hunter_starts_out_knowing_where_the_player_is() {
    let mut s = world();
    player(&mut s, 2, "");
    director(&mut s, 3, r#""cooldown":100,"max_alive":1,"cleanup":0"#);
    set_heat(&mut s, 2, 2.0);
    s.step();
    let h = hunters(&mut s);
    assert_eq!(h.len(), 1);
    let e = s.world.entity(h[0].0).unwrap();
    let per = *s.world.get::<Perception>(e).unwrap();
    assert_eq!((per.state, per.target.0), (3, 2), "searching for the player: {per:?}");
    assert!((per.seen_pos[2] + 20.0).abs() < 0.1, "at the player's place: {:?}", per.seen_pos);
    // it faces the player: its forward (local -z) points at it
    let q = s.world.get::<Transform>(e).unwrap().rot;
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let (fx, fz) = (-(2.0 * (x * z + w * y)), -(1.0 - 2.0 * (x * x + y * y)));
    let (tx, tz) = (0.0 - h[0].1[0], -20.0 - h[0].1[2]);
    let dot = (fx * tx + fz * tz) / ((fx * fx + fz * fz).sqrt() * (tx * tx + tz * tz).sqrt());
    assert!(dot > 0.97, "it is turned towards the player: {dot}");
}

#[test]
fn gunfire_and_being_seen_raise_heat_and_it_falls_back() {
    // gunfire that can be heard: each tick the gun fires costs shot_heat
    let mut s = world();
    player(&mut s, 2, "");
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,1.2,-20]},"Weapon":{"owner":2,"rate":600,"noise":100,"spread":0,"recoil":[0,0],"fire":true}}"#);
    director(&mut s, 3, r#""shot_heat":0.3,"threshold":50"#);
    s.step_n(30);
    let shots = s.world.get::<crate::components::Weapon>(s.world.entity(4).unwrap()).unwrap().shots;
    let v = heat(&s, 2).value;
    assert!(shots >= 3 && v > 0.3 * (shots as f32) * 0.8 - 0.05, "each round that can be heard raised the heat: {shots} rounds, heat {v}");
    // a silenced gun adds none
    let mut s = world();
    player(&mut s, 2, "");
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,1.2,-20]},"Weapon":{"owner":2,"rate":600,"noise":0,"spread":0,"recoil":[0,0],"fire":true}}"#);
    director(&mut s, 3, r#""shot_heat":0.3,"threshold":50"#);
    s.step_n(30);
    assert_eq!(heat(&s, 2).value, 0.0);
    // being seen by a machine: an alert one with the player in sight
    let mut s = world();
    player(&mut s, 2, "");
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,0,0]},"Team":{"id":2},"Perception":{"sight_range":40,"notice_time":0.2}}"#);
    director(&mut s, 3, r#""seen_heat":1.0,"threshold":50"#);
    s.step_n(60);
    assert!(heat(&s, 2).value > 0.5, "seen for most of a second: {}", heat(&s, 2).value);
    // it falls back by its decay when nothing raises it
    let mut s = world();
    player(&mut s, 2, "");
    s.world.get_mut::<Heat>(s.world.entity(2).unwrap()).unwrap().decay = 1.0;
    director(&mut s, 3, r#""threshold":50"#);
    set_heat(&mut s, 2, 3.0);
    s.step_n(120);
    assert!(heat(&s, 2).value < 1.2, "{}", heat(&s, 2).value);
}

#[test]
fn a_wave_is_capped_by_the_heat_the_room_and_the_cooldown_and_a_bigger_heat_brings_more() {
    // heat 3 over a threshold of 1 with 2 extra hunters per point: 1 + 4 = 5, but only 3 may be alive
    let mut s = world();
    player(&mut s, 2, "");
    director(&mut s, 3, r#""max_alive":3,"cooldown":5,"cleanup":0,"wave_cost":0.1"#);
    set_heat(&mut s, 2, 3.0);
    s.step();
    assert_eq!(hunters(&mut s).len(), 3, "capped by max_alive");
    // the cooldown keeps the next wave away, and the room is full anyway
    set_heat(&mut s, 2, 5.0);
    s.step_n(400);
    assert_eq!(hunters(&mut s).len(), 3, "no room");
    assert_eq!(heat(&s, 2).waves, 1);
    // a smaller heat sends fewer: 1.5 over the threshold is 1 + 1
    let mut s = world();
    player(&mut s, 2, "");
    director(&mut s, 3, r#""max_alive":9,"cleanup":0"#);
    set_heat(&mut s, 2, 1.05);
    s.step();
    assert_eq!(hunters(&mut s).len(), 1, "at the threshold, one hunter");
    let mut s = world();
    player(&mut s, 2, "");
    director(&mut s, 3, r#""max_alive":9,"cleanup":0"#);
    set_heat(&mut s, 2, 2.05);
    s.step();
    assert_eq!(hunters(&mut s).len(), 3, "one over the threshold: 1 + 2");
    // below the threshold nothing comes
    let mut s = world();
    player(&mut s, 2, "");
    director(&mut s, 3, "");
    set_heat(&mut s, 2, 0.9);
    s.step_n(30);
    assert!(hunters(&mut s).is_empty());
}

#[test]
fn hunters_that_went_back_to_idle_are_removed_and_a_wave_may_come_again() {
    let mut s = world();
    player(&mut s, 2, "");
    director(&mut s, 3, r#""max_alive":2,"cooldown":1,"cleanup":2,"wave_cost":0.1"#);
    set_heat(&mut s, 2, 1.05);
    s.step();
    assert_eq!(hunters(&mut s).len(), 1);
    // nobody keeps the player in sight: the hunter's awareness drains in one second, it is idle, two seconds later it is removed
    s.step_n(60 * 8);
    assert!(hunters(&mut s).is_empty(), "the idle hunter was removed");
    assert_eq!(dir_of(&s, 3).alive, 0);
}

#[test]
fn with_clear_a_hunter_is_dropped_where_it_can_see_the_player_and_without_it_some_land_behind_a_wall() {
    // a wall 60 m high and 400 m long across the ring, between the player (z = -20) and the places beyond z = -33
    let run = |clear: u32| -> Vec<[f32; 3]> {
        let mut s = world();
        spawn(&mut s, 9, r#"{"Transform":{"pos":[0,30,-33]},"Collider":{"shape":1,"size":[400,60,2]}}"#);
        player(&mut s, 2, "");
        director(&mut s, 3, &format!(r#""min_dist":30,"max_dist":40,"cooldown":100,"max_alive":16,"cleanup":0,"per_heat":4,"clear":{clear}"#));
        set_heat(&mut s, 2, 6.0);
        s.step();
        hunters(&mut s).into_iter().map(|h| h.1).collect()
    };
    let behind = |v: &[[f32; 3]]| v.iter().filter(|p| p[2] < -32.0).count();
    let without = run(0);
    let with = run(8);
    assert_eq!(without.len(), 16, "a wave of sixteen");
    assert_eq!(with.len(), 16);
    assert!(behind(&without) >= 1, "the control: with no search some of the sixteen land behind the wall ({} of 16)", behind(&without));
    assert_eq!(behind(&with), 0, "with `clear` every one of them is dropped where it can see the player: {:?}", with.iter().filter(|p| p[2] < -32.0).collect::<Vec<_>>());
    assert_eq!(run(8), with, "and it is the same places every run");
    assert!(without.iter().filter(|p| p[2] >= -32.0).all(|p| with.contains(p)), "a place that already had a clear line is the place drawn first: kept as it was");
}
