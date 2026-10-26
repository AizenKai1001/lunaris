//! Survival needs: hunger, thirst, stamina, bleeding, healing, and what an item does. No art.
use super::*;
use crate::components::{HitPoints, Vitals};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn vitals(s: &Sim, uid: u64) -> Vitals {
    *s.world.get::<Vitals>(s.world.entity(uid).unwrap()).unwrap()
}

fn hp(s: &Sim, uid: u64) -> f32 {
    s.world.get::<HitPoints>(s.world.entity(uid).unwrap()).unwrap().value
}

fn edit(s: &mut Sim, uid: u64, f: impl FnOnce(&mut Vitals)) {
    let e = s.world.entity(uid).unwrap();
    f(s.world.get_mut::<Vitals>(e).unwrap());
}

fn person(s: &mut Sim, uid: u64, vitals: &str, hp: f32) {
    spawn(s, uid, &format!(r#"{{"Vitals":{{{vitals}}},"HitPoints":{{"value":{hp},"max":100}}}}"#));
}

#[test]
fn hunger_and_thirst_fall_and_exertion_and_rest_change_how_fast() {
    let mut s = Sim::new(1);
    person(&mut s, 1, r#""hunger_rate":1,"thirst_rate":2"#, 100.0);
    person(&mut s, 2, r#""hunger_rate":1,"thirst_rate":2,"sprint":true"#, 100.0);
    person(&mut s, 3, r#""hunger_rate":1,"thirst_rate":2,"resting":true"#, 100.0);
    s.step_n(60);
    let (a, b, c) = (vitals(&s, 1), vitals(&s, 2), vitals(&s, 3));
    assert!((a.hunger - 99.0).abs() < 0.05 && (a.thirst - 98.0).abs() < 0.05, "{a:?}");
    assert!((b.hunger - 98.0).abs() < 0.05 && (b.thirst - 96.0).abs() < 0.05, "sprinting costs twice: {b:?}");
    assert!((c.hunger - 99.5).abs() < 0.05, "resting costs half: {c:?}");
}

#[test]
fn stamina_runs_out_in_a_sprint_makes_the_character_exhausted_and_comes_back() {
    let mut s = Sim::new(1);
    person(&mut s, 1, r#""sprint":true,"sprint_cost":50,"stamina_regen":20,"stamina_delay":1"#, 100.0);
    s.step_n(60);
    assert!(vitals(&s, 1).stamina > 40.0 && vitals(&s, 1).can_sprint, "a second of sprint is half the stamina");
    s.step_n(80);
    let v = vitals(&s, 1);
    assert!(v.exhausted && !v.can_sprint && v.status & 8 != 0, "{v:?}");
    assert!((v.speed_scale - 0.6).abs() < 1e-6, "exhausted: slower {}", v.speed_scale);
    assert_eq!(v.stamina, 0.0, "(it cannot sprint on nothing: the sprint does nothing more)");
    // let go of sprint: the delay, then 20 a second back to 25
    edit(&mut s, 1, |v| v.sprint = false);
    s.step_n(60);
    assert!(vitals(&s, 1).exhausted, "still in the delay");
    s.step_n(90);
    let v = vitals(&s, 1);
    assert!(!v.exhausted && v.can_sprint && v.stamina > 25.0 && v.speed_scale == 1.0, "{v:?}");
}

#[test]
fn an_empty_meter_hurts_and_a_fed_body_heals() {
    let mut s = Sim::new(1);
    person(&mut s, 1, r#""hunger":0,"hunger_rate":0,"thirst_rate":0,"starve_damage":6"#, 50.0);
    person(&mut s, 2, r#""hunger_rate":0,"thirst_rate":0,"heal_rate":3"#, 50.0);
    person(&mut s, 3, r#""hunger":50,"thirst":50,"hunger_rate":0,"thirst_rate":0,"heal_rate":3"#, 50.0);
    s.step_n(60);
    assert!((hp(&s, 1) - 44.0).abs() < 0.2, "starving costs 6 a second: {}", hp(&s, 1));
    assert!(vitals(&s, 1).status & 16 != 0 && vitals(&s, 1).status & 1 != 0);
    assert!((hp(&s, 2) - 53.0).abs() < 0.2, "fed and watered: heals 3 a second: {}", hp(&s, 2));
    assert_eq!(hp(&s, 3), 50.0, "half fed is not enough to heal");
    // thirst at 0 hurts too (0.8 a second by default), and the harm adds up with bleeding
    let mut s = Sim::new(1);
    person(&mut s, 1, r#""thirst":0,"hunger_rate":0,"thirst_rate":0,"dehydrate_damage":1,"bleeding":2"#, 50.0);
    s.step_n(60);
    assert!((hp(&s, 1) - 47.0).abs() < 0.2, "{}", hp(&s, 1));
    // the health never goes beyond its limits, and the dead are not healed
    let mut s = Sim::new(1);
    person(&mut s, 1, r#""hunger_rate":0,"thirst_rate":0,"heal_rate":1000"#, 99.0);
    s.step_n(5);
    assert_eq!(hp(&s, 1), 100.0);
    let mut s = Sim::new(1);
    person(&mut s, 1, r#""hunger_rate":0,"thirst_rate":0,"heal_rate":10"#, 0.0);
    s.step_n(3);
    assert_eq!(hp(&s, 1), 0.0, "dead stays dead");
}

#[test]
fn bleeding_hurts_until_a_bandage_and_items_are_requests_that_are_used_once() {
    let mut s = Sim::new(1);
    person(&mut s, 1, r#""bleeding":6,"hunger":60,"thirst":60,"hunger_rate":0,"thirst_rate":0"#, 80.0);
    s.step_n(60);
    assert!((hp(&s, 1) - 74.0).abs() < 0.2, "{}", hp(&s, 1));
    assert!(vitals(&s, 1).status & 4 != 0);
    edit(&mut s, 1, |v| {
        v.bandage = true;
        v.eat = 30.0;
        v.drink = 15.0;
        v.heal = 10.0;
    });
    s.step();
    let v = vitals(&s, 1);
    assert_eq!((v.bleeding, v.bandage, v.eat, v.drink, v.heal), (0.0, false, 0.0, 0.0, 0.0), "the requests are used up");
    assert!((v.hunger - 90.0).abs() < 0.01 && (v.thirst - 75.0).abs() < 0.01, "{v:?}");
    assert!(hp(&s, 1) > 83.5 && hp(&s, 1) < 85.0, "10 hit points at once: {}", hp(&s, 1));
    assert_eq!(v.status & 4, 0, "no longer bleeding");
    // the meters stop at 100
    edit(&mut s, 1, |v| v.eat = 500.0);
    s.step();
    assert_eq!(vitals(&s, 1).hunger, 100.0);
}

#[test]
fn the_events_come_when_a_need_turns_up_and_not_every_tick() {
    let mut s = Sim::new(1);
    spawn(&mut s, 9, r#"{"EventWatch":{"names":"hungry, thirsty, starving, exhausted"}}"#);
    person(&mut s, 1, r#""hunger":25.05,"hunger_rate":1,"thirst_rate":0"#, 100.0);
    let mut hungry = 0.0f64;
    for _ in 0..120 {
        s.step();
        hungry += s.world.resource("event.hungry").unwrap_or(0.0);
    }
    assert_eq!(hungry, 1.0, "announced once, when it crossed 25");
    assert!(vitals(&s, 1).status & 1 != 0);
}

#[test]
fn a_hit_of_enough_damage_may_start_a_bleed_that_a_bandage_stops() {
    use crate::survival::wound;
    let mut s = Sim::new(1);
    person(&mut s, 1, r#""wound_chance":1.0,"wound_min":8,"wound_rate":0.5,"bleed_max":1.2"#, 100.0);
    person(&mut s, 2, r#""wound_chance":0.0"#, 100.0);
    wound(&mut s.world, 1, 5.0, 1, 0);
    assert_eq!(vitals(&s, 1).bleeding, 0.0, "a graze below `wound_min` does not bleed");
    wound(&mut s.world, 1, 20.0, 1, 0);
    assert_eq!(vitals(&s, 1).bleeding, 0.5);
    wound(&mut s.world, 1, 20.0, 2, 0);
    wound(&mut s.world, 1, 20.0, 3, 0);
    assert_eq!(vitals(&s, 1).bleeding, 1.2, "capped at `bleed_max`");
    wound(&mut s.world, 2, 90.0, 1, 0);
    assert_eq!(vitals(&s, 2).bleeding, 0.0, "a body with no wound chance never bleeds from a hit");
    wound(&mut s.world, 99, 90.0, 1, 0);
    // a bleed hurts, slows, shows in the status, and a bandage stops it
    s.step_n(60);
    assert!(hp(&s, 1) < 99.0 && vitals(&s, 1).status & 4 != 0 && vitals(&s, 1).speed_scale < 1.0, "bleeding for a second takes about 1.2 hp: {}", hp(&s, 1));
    edit(&mut s, 1, |v| v.bandage = true);
    s.step();
    assert_eq!((vitals(&s, 1).bleeding, vitals(&s, 1).status & 4), (0.0, 0));
    // half the hits wound at a chance of one half, the same ones every time
    let count = |chance: f32| {
        let mut s = Sim::new(1);
        person(&mut s, 3, &format!(r#""wound_chance":{chance},"wound_min":1,"wound_rate":1,"bleed_max":100000"#), 100.0);
        for tick in 0..400u64 {
            wound(&mut s.world, 3, 10.0, tick, 7);
        }
        vitals(&s, 3).bleeding
    };
    let n = count(0.5);
    assert!((150.0..250.0).contains(&n), "about half of 400 hits wound: {n}");
    assert_eq!(count(0.5), n, "deterministic");
}

#[test]
fn a_real_shot_wounds_a_body_with_a_wound_chance_and_not_one_without() {
    let run = |chance: f32| {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,0,-10]}},"Collider":{{"shape":2,"size":[0.7,1.8,0.7]}},"HitPoints":{{"value":1000,"max":1000}},"Vitals":{{"wound_chance":{chance},"wound_min":8}}}}"#));
        spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0.9,0]},"Weapon":{"active":true,"damage":20,"spread":0,"rate":600,"mag":10,"mag_size":10,"auto":false,"head_mult":1,"limb_mult":1,"recoil":[0,0],"infinite":true}}"#);
        s.step();
        let e = s.world.entity(2).unwrap();
        s.world.get_mut::<crate::components::Weapon>(e).unwrap().fire = true;
        s.step_n(3);
        vitals(&s, 1).bleeding
    };
    assert_eq!(run(1.0), 0.5, "a 20 hp round into a body that always wounds");
    assert_eq!(run(0.0), 0.0, "and into one that never does");
}

