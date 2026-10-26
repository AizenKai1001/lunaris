//! Death and respawn: the corpse and what it holds, the empty bag, the wait, the choice of spawn point (own, shared, far from the others), the new life. No art: names and numbers.
use super::*;
use crate::components::{Heat, HitPoints, Respawn, SpawnPoint, Transform, Vitals};
use crate::inventory::Inventory;
use lunaris_json::{parse, Json};

const ITEMS: &str = "item bandage | Bandage | stack=10\nitem ammo | Ammo | stack=100\n";

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn world() -> Sim {
    let mut s = Sim::new(1);
    let items = ITEMS.replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"ItemTable":{{"text":"{items}"}}}}"#));
    s.apply(&Command::DefinePrefab { name: "corpse".into(), def: parse(r#"{"components":{"Name":{"value":"Corpse"}}}"#).unwrap() }).unwrap();
    s
}

/// A player at (x, 0, z) with 100 hit points, a bag with 3 bandages and 40 rounds, vitals and a heat meter.
fn player(s: &mut Sim, uid: u64, x: f32, z: f32, respawn: &str) {
    let sep = if respawn.is_empty() { "" } else { "," };
    spawn(
        s,
        uid,
        &format!(
            r#"{{"Transform":{{"pos":[{x},0,{z}]}},"HitPoints":{{"value":100,"max":100,"last":100}},"Inventory":{{"size":12,"fill":"bandage 3, ammo 40"}},"Vitals":{{"hunger_rate":0,"thirst_rate":0,"hunger":10,"thirst":10,"stamina":5}},"Heat":{{"value":4,"decay":0}},"Respawn":{{"corpse":"corpse","delay":2{sep}{respawn}}}}}"#
        ),
    );
}

fn point(s: &mut Sim, uid: u64, x: f32, z: f32, extra: &str) {
    let sep = if extra.is_empty() { "" } else { "," };
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{x},0,{z}]}},"SpawnPoint":{{{extra}{sep}}}}}"#).replace(",}", "}"));
}

fn kill(s: &mut Sim, uid: u64) {
    let e = s.world.entity(uid).unwrap();
    s.world.get_mut::<HitPoints>(e).unwrap().value = 0.0;
}

fn pos(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

fn respawn(s: &Sim, uid: u64) -> Respawn {
    *s.world.get::<Respawn>(s.world.entity(uid).unwrap()).unwrap()
}

fn hp(s: &Sim, uid: u64) -> HitPoints {
    *s.world.get::<HitPoints>(s.world.entity(uid).unwrap()).unwrap()
}

fn items_of(s: &Sim, uid: u64) -> u32 {
    let Some(inv) = s.world.get::<Inventory>(s.world.entity(uid).unwrap()) else { return 0 };
    (0..48).map(|k| if inv.id[k] != 0 { inv.count[k] } else { 0 }).sum()
}

fn corpses(s: &Sim) -> Vec<u64> {
    let mut out = Vec::new();
    for e in s.world.entities_by_uid() {
        if s.world.get::<crate::components::Name>(e).is_some_and(|n| s.world.strings().get(n.value.0) == "Corpse") {
            out.push(s.world.uid_of(e).unwrap());
        }
    }
    out
}

fn events(s: &Sim, name: &str) -> Vec<u64> {
    s.events.iter().filter(|e| e.name == name).map(|e| e.uid).collect()
}

#[test]
fn the_dead_leave_a_corpse_with_what_they_carried_and_an_empty_bag() {
    let mut s = world();
    player(&mut s, 10, 5.0, 7.0, "");
    s.step_n(3);
    assert_eq!(items_of(&s, 10), 43, "the starting bag: 3 bandages and 40 rounds");
    kill(&mut s, 10);
    s.step();
    let r = respawn(&s, 10);
    assert_eq!((r.state, r.deaths), (1, 1));
    assert_eq!(r.died_at, [5.0, 0.0, 7.0]);
    assert_eq!(items_of(&s, 10), 0, "their own bag is empty");
    let c = corpses(&s);
    assert_eq!(c, vec![r.corpse_uid.0], "one corpse, and the player knows it");
    assert_eq!(items_of(&s, c[0]), 43, "the corpse holds what they had");
    assert_eq!(pos(&s, c[0]), [5.0, 0.0, 7.0], "where they fell");
    assert_eq!(events(&s, "corpse"), c);
    // it is removed after `corpse_time` seconds (a `Lifetime`)
    let life = s.world.get::<crate::components::Lifetime>(s.world.entity(c[0]).unwrap()).unwrap().seconds;
    assert_eq!(life, 300.0);
    // the corpse's bag is a copy, it neither publishes nor asks for anything
    let inv = *s.world.get::<Inventory>(s.world.entity(c[0]).unwrap()).unwrap();
    assert_eq!((inv.publish.0, inv.give_id, inv.fill.0), (0, 0, 0));
    // dying once is one corpse, not one a tick
    s.step_n(10);
    assert_eq!(corpses(&s).len(), 1);
    assert_eq!(respawn(&s, 10).deaths, 1);
}

#[test]
fn after_the_delay_they_get_up_at_a_spawn_point_with_a_fresh_start() {
    let mut s = world();
    player(&mut s, 10, 5.0, 7.0, r#""food":60,"protect":3"#);
    point(&mut s, 20, 100.0, 50.0, "");
    s.step_n(2);
    kill(&mut s, 10);
    s.step_n(60);
    assert_eq!(respawn(&s, 10).state, 1, "a second in: still dead");
    assert!(hp(&s, 10).dead);
    s.step_n(80);
    let r = respawn(&s, 10);
    assert_eq!(r.state, 0, "{r:?}");
    let h = hp(&s, 10);
    assert!(!h.dead && h.value == 100.0, "{h:?}");
    assert!(h.invulnerable > 1.0, "a few seconds of grace: {}", h.invulnerable);
    assert_eq!(pos(&s, 10), [100.0, 0.0, 50.0]);
    let v = *s.world.get::<Vitals>(s.world.entity(10).unwrap()).unwrap();
    assert_eq!((v.hunger, v.thirst, v.stamina, v.bleeding), (60.0, 60.0, 100.0, 0.0));
    assert_eq!(s.world.get::<Heat>(s.world.entity(10).unwrap()).unwrap().value, 0.0, "a new life starts with a clean record");
    assert_eq!(events(&s, "respawned"), vec![10]);
    assert_eq!(s.world.get::<SpawnPoint>(s.world.entity(20).unwrap()).unwrap().uses, 1);
    // the corpse stays where it was
    assert_eq!(corpses(&s).len(), 1);
    // and they can die again (once the grace of the new life has run out: hits in it are undone)
    s.step_n(200);
    kill(&mut s, 10);
    s.step_n(200);
    assert_eq!((respawn(&s, 10).deaths, respawn(&s, 10).state), (2, 0));
    assert_eq!(corpses(&s).len(), 2);
}

#[test]
fn a_shared_spawn_point_far_from_the_living_is_chosen() {
    // an enemy 10 m from the first point: the player starts at the second, every time
    for round in 0..6 {
        let mut s = world();
        s.step_n(round); // (a different tick each time: the choice must not depend on luck)
        player(&mut s, 10, 20.0, 0.0, r#""avoid":80"#);
        player(&mut s, 11, 10.0, 0.0, "");
        point(&mut s, 20, 0.0, 0.0, "");
        point(&mut s, 21, 500.0, 0.0, "");
        kill(&mut s, 10);
        s.step_n(200);
        assert_eq!(pos(&s, 10), [500.0, 0.0, 0.0], "round {round}");
    }
    // nowhere is far enough: the farthest from the others
    let mut s = world();
    player(&mut s, 10, 20.0, 0.0, r#""avoid":80"#);
    player(&mut s, 11, 10.0, 0.0, "");
    point(&mut s, 20, 0.0, 0.0, "");
    point(&mut s, 21, 40.0, 0.0, "");
    kill(&mut s, 10);
    s.step_n(200);
    assert_eq!(pos(&s, 10), [40.0, 0.0, 0.0]);
    // several points that are all fine: they all get used by different players over time
    let mut seen = std::collections::BTreeSet::new();
    for uid in 0..12u64 {
        let mut s = world();
        player(&mut s, 30 + uid, 0.0, 0.0, r#""avoid":10"#);
        for k in 0..4 {
            point(&mut s, 20 + k, 200.0 * k as f32, 300.0, "");
        }
        kill(&mut s, 30 + uid);
        s.step_n(200);
        seen.insert(pos(&s, 30 + uid)[0] as i32);
    }
    assert!(seen.len() >= 2, "the choice varies: {seen:?}");
}

#[test]
fn a_players_own_place_is_used_whoever_is_near_and_a_used_one_is_gone() {
    let mut s = world();
    player(&mut s, 10, 20.0, 0.0, r#""group":7"#);
    player(&mut s, 11, 3.0, 3.0, "");
    point(&mut s, 20, 500.0, 0.0, "");
    point(&mut s, 21, 3.0, 4.0, r#""group":7,"once":true"#);
    point(&mut s, 22, 9.0, 9.0, r#""group":8"#);
    kill(&mut s, 10);
    s.step_n(200);
    assert_eq!(pos(&s, 10), [3.0, 0.0, 4.0], "their sleeping bag, right beside an enemy");
    assert!(!s.world.get::<SpawnPoint>(s.world.entity(21).unwrap()).unwrap().active, "used up");
    // the next time the bag is gone: a shared point (the other player's bag is not theirs)
    s.step_n(200);
    kill(&mut s, 10);
    s.step_n(200);
    assert_eq!(pos(&s, 10), [500.0, 0.0, 0.0]);
}

#[test]
fn a_point_scatters_the_player_over_its_disc() {
    let mut inside = 0;
    for k in 0..10u64 {
        let mut s = world();
        player(&mut s, 10 + k, 0.0, 0.0, "");
        point(&mut s, 50, 100.0, 100.0, r#""radius":8"#);
        kill(&mut s, 10 + k);
        s.step_n(200);
        let p = pos(&s, 10 + k);
        if ((p[0] - 100.0).powi(2) + (p[2] - 100.0).powi(2)).sqrt() <= 8.0 + 1.0e-3 {
            inside += 1;
        }
    }
    assert_eq!(inside, 10);
    // different players land in different places on the disc
    let mut s = world();
    for k in 0..6u64 {
        player(&mut s, 10 + k, 1000.0 * k as f32, 0.0, r#""avoid":0"#);
    }
    point(&mut s, 50, 100.0, 100.0, r#""radius":8"#);
    for k in 0..6u64 {
        kill(&mut s, 10 + k);
    }
    s.step_n(200);
    let places: std::collections::BTreeSet<i32> = (0..6u64).map(|k| (pos(&s, 10 + k)[0] * 100.0) as i32).collect();
    assert!(places.len() >= 4, "{places:?}");
}

#[test]
fn a_death_screen_can_hold_the_respawn_until_the_player_asks() {
    let mut s = world();
    player(&mut s, 10, 5.0, 7.0, r#""hold":true"#);
    point(&mut s, 20, 100.0, 50.0, "");
    kill(&mut s, 10);
    s.step_n(400);
    assert_eq!(respawn(&s, 10).state, 1, "it waits");
    let e = s.world.entity(10).unwrap();
    s.world.get_mut::<Respawn>(e).unwrap().request = true;
    s.step_n(2);
    assert_eq!(respawn(&s, 10).state, 0);
    assert_eq!(pos(&s, 10), [100.0, 0.0, 50.0]);
    assert!(!respawn(&s, 10).request, "the request is used up");
}

#[test]
fn the_items_can_stay_and_without_a_spawn_point_they_get_up_where_they_fell() {
    let mut s = world();
    player(&mut s, 10, 5.0, 7.0, r#""keep_items":true"#);
    s.step_n(3);
    kill(&mut s, 10);
    s.step_n(200);
    assert_eq!(items_of(&s, 10), 43, "they keep their bag");
    assert_eq!(items_of(&s, corpses(&s)[0]), 43, "and the corpse has a copy (a game that wants one or the other sets keep_items)");
    assert_eq!(pos(&s, 10), [5.0, 0.0, 7.0], "no spawn point: where they fell");
    assert!(!hp(&s, 10).dead);
}

#[test]
fn a_player_with_no_corpse_prefab_just_loses_the_bag_and_one_the_game_revived_is_alive_again() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, &format!(r#"{{"ItemTable":{{"text":"{}"}}}}"#, ITEMS.replace('\n', "\\n")));
    spawn(&mut s, 10, r#"{"Transform":{},"HitPoints":{"value":100,"max":100,"last":100},"Inventory":{"size":12,"fill":"bandage 3"},"Respawn":{"delay":100}}"#);
    s.step_n(3);
    kill(&mut s, 10);
    s.step_n(3);
    assert_eq!(respawn(&s, 10).corpse_uid.0, 0);
    assert_eq!(items_of(&s, 10), 0);
    assert!(s.events.iter().all(|e| e.name != "corpse"));
    // a game that heals them (a revive item) before the delay: no respawn needed
    let e = s.world.entity(10).unwrap();
    let h = s.world.get_mut::<HitPoints>(e).unwrap();
    h.value = 50.0;
    h.dead = false;
    s.step_n(2);
    assert_eq!(respawn(&s, 10).state, 0);
}

#[test]
fn dying_and_getting_up_is_deterministic() {
    let run = || {
        let mut s = world();
        player(&mut s, 10, 20.0, 0.0, "");
        player(&mut s, 11, 10.0, 0.0, "");
        for k in 0..3 {
            point(&mut s, 20 + k, 100.0 * k as f32, 100.0, r#""radius":5"#);
        }
        kill(&mut s, 10);
        s.step_n(150);
        kill(&mut s, 11);
        s.step_n(150);
        s.world.state_hash()
    };
    assert_eq!(run(), run());
}

fn timed_world() -> Sim {
    let mut s = Sim::new(1);
    let items = "item wood | Wood | stack=50\nitem plank | Plank | stack=50\nrecipe plank 2 = wood 3 ~ 5\n".replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"ItemTable":{{"text":"{items}"}}}}"#));
    s.apply(&Command::DefinePrefab { name: "corpse".into(), def: parse(r#"{"components":{"Name":{"value":"Corpse"}}}"#).unwrap() }).unwrap();
    s
}

fn count_of(s: &Sim, uid: u64, id: u32) -> u32 {
    let inv = s.world.get::<Inventory>(s.world.entity(uid).unwrap()).unwrap();
    (0..48).filter(|&k| inv.id[k] == id).map(|k| inv.count[k]).sum()
}

#[test]
fn a_craft_in_hand_when_they_die_leaves_its_ingredients_in_the_corpse() {
    // (the ingredients are taken when a timed craft starts: a player who dies mid-craft used to lose them with the life)
    let mut s = timed_world();
    spawn(&mut s, 10, r#"{"Transform":{"pos":[1,0,2]},"HitPoints":{"value":100,"max":100,"last":100},"Inventory":{"size":12,"fill":"wood 7"},"Respawn":{"corpse":"corpse","delay":2}}"#);
    s.step_n(3);
    let e = s.world.entity(10).unwrap();
    s.world.get_mut::<Inventory>(e).unwrap().craft_start = 1;
    s.step_n(3);
    assert_eq!(count_of(&s, 10, 1), 4, "three wood are taken at the start (7 -> 4)");
    assert_eq!(s.world.get::<Inventory>(e).unwrap().crafting, 1, "a craft is in hand");
    kill(&mut s, 10);
    s.step();
    let c = corpses(&s);
    assert_eq!(c.len(), 1);
    assert_eq!(count_of(&s, c[0], 1), 7, "the corpse holds what was in the bag and the three wood of the craft");
    assert_eq!(count_of(&s, c[0], 2), 0, "nothing was made");
    assert_eq!(items_of(&s, 10), 0, "their own bag is empty, and no craft goes on");
    assert_eq!(s.world.get::<Inventory>(e).unwrap().crafting, 0);
}

#[test]
fn a_craft_in_hand_when_the_items_stay_goes_back_into_the_bag_that_stays() {
    let mut s = timed_world();
    spawn(&mut s, 10, r#"{"Transform":{"pos":[1,0,2]},"HitPoints":{"value":100,"max":100,"last":100},"Inventory":{"size":12,"fill":"wood 3"},"Respawn":{"corpse":"corpse","delay":2,"keep_items":true}}"#);
    s.step_n(3);
    let e = s.world.entity(10).unwrap();
    s.world.get_mut::<Inventory>(e).unwrap().craft_start = 1;
    s.step_n(3);
    assert_eq!(count_of(&s, 10, 1), 0, "all three wood are in the craft");
    kill(&mut s, 10);
    s.step();
    assert_eq!(count_of(&s, 10, 1), 3, "the bag that is kept has them back");
    assert_eq!(s.world.get::<Inventory>(e).unwrap().crafting, 0);
}

#[test]
fn a_death_with_nothing_in_hand_and_nothing_carried_still_leaves_no_items() {
    let mut s = timed_world();
    spawn(&mut s, 10, r#"{"Transform":{"pos":[1,0,2]},"HitPoints":{"value":100,"max":100,"last":100},"Inventory":{"size":12},"Respawn":{"corpse":"corpse","delay":2}}"#);
    s.step_n(3);
    kill(&mut s, 10);
    s.step();
    let c = corpses(&s);
    assert_eq!(c.len(), 1);
    assert_eq!(items_of(&s, c[0]), 0);
}
