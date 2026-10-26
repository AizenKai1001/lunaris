//! Worn gear: the places are read, the cover is kept, a hit is lessened by what is worn where it landed, the pieces wear and break, and a real shot of the weapons system goes through it.
use super::*;
use crate::armor::{absorb, absorb_piercing, allows, parse_places, places};
use crate::components::{Equipment, HitPoints};
use crate::inventory::{table_of, Inventory};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

// the items are numbered from 1 in the order of the table: 1 helmet (class 1), 2 vest (class 2), 3 plate (class 2), 4 pistol (class 0)
const ITEMS: &str = "item helmet | Helmet | stack=1 class=1 power=50 dur=100\\nitem vest | Vest | stack=1 class=2 power=40 dur=50\\nitem plate | Plate | stack=1 class=2 power=50 dur=0\\nitem pistol | Pistol | stack=1\\n";
const PLACES: &str = "# the worn places\\nslot 4 accepts=1 zones=head\\nslot 5 accepts=2 zones=body\\nslot 6 accepts=2 zones=body\\nslot 7 accepts=3 zones=legs";

/// A character (uid 1) with a bag of 8: a helmet in slot 4, a vest in slot 5 (50 points of durability); slots 6 and 7 are worn places that are empty.
fn world() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 50, &format!(r#"{{"ItemTable":{{"text":"{ITEMS}"}}}}"#));
    let mut id = vec![0u32; 48];
    let mut count = vec![0u32; 48];
    let mut dur = vec![0.0f32; 48];
    (id[4], count[4], dur[4]) = (1, 1, 100.0);
    (id[5], count[5], dur[5]) = (2, 1, 50.0);
    let list = |v: &[String]| v.join(",");
    let (id, count, dur) = (list(&id.iter().map(|x| x.to_string()).collect::<Vec<_>>()), list(&count.iter().map(|x| x.to_string()).collect::<Vec<_>>()), list(&dur.iter().map(|x| x.to_string()).collect::<Vec<_>>()));
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,0,-10]}},"Collider":{{"shape":2,"size":[0.7,1.8,0.7]}},"HitPoints":{{"value":1000,"max":1000}},"Inventory":{{"size":8,"id":[{id}],"count":[{count}],"dur":[{dur}]}},"Equipment":{{"slots":"{PLACES}"}}}}"#));
    s
}

fn hit(s: &mut Sim, damage: f32, zone: u32) -> (f32, Vec<String>) {
    let mut ev: Vec<(String, u64, Option<[f32; 3]>)> = Vec::new();
    let left = absorb(&mut s.world, 1, damage, zone, &mut ev);
    (left, ev.into_iter().map(|e| e.0).collect())
}

fn slot(s: &Sim, k: usize) -> (u32, f32) {
    let inv = s.world.get::<Inventory>(s.world.entity(1).unwrap()).unwrap();
    (inv.id[k], inv.dur[k])
}

#[test]
fn the_places_of_the_text_are_read_and_its_mistakes_named() {
    let (p, problems) = parse_places("# a comment\nslot 4 accepts=1 zones=head\nslot 5 accepts=2 zones=body,legs # trailing\nslot 5 zones=head\nslot x\nthing 3\nslot 6 zones=nose bad\nslot 99\nslot 7 accepts=q");
    assert_eq!(p.len(), 4, "{p:?}");
    assert_eq!((p[0].slot, p[0].accepts, p[0].zones), (4, 1, 1 << 2));
    assert_eq!((p[1].slot, p[1].accepts, p[1].zones), (5, 2, (1 << 1) | (1 << 3)));
    assert_eq!((p[2].slot, p[3].slot), (6, 7), "a place with an unknown zone or a bad number is still a place, with what was understood");
    assert_eq!(problems.len(), 7, "{problems:#?}");
}

#[test]
fn what_is_worn_stops_its_share_of_a_hit_where_it_landed_and_nothing_elsewhere() {
    let mut s = world();
    s.step();
    let eq = *s.world.get::<Equipment>(s.world.entity(1).unwrap()).unwrap();
    let want = [0.0, 0.4, 0.5, 0.0];
    assert!(eq.cover.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-5), "the vest stops 40 percent of the body, the helmet 50 percent of the head, nothing covers the legs: {:?}", eq.cover);
    assert!((hit(&mut s, 100.0, 1).0 - 60.0).abs() < 1e-4, "body");
    assert!((hit(&mut s, 100.0, 2).0 - 50.0).abs() < 1e-4, "head");
    assert_eq!(hit(&mut s, 100.0, 3).0, 100.0, "legs: nothing is worn there");
    assert_eq!(hit(&mut s, 100.0, 0).0, 100.0, "a hit with no zone (a wall, nothing) is not a hit on the body");
    assert_eq!(hit(&mut s, 0.0, 1).0, 0.0);
    let absorbed = s.world.get::<Equipment>(s.world.entity(1).unwrap()).unwrap().absorbed;
    assert!((absorbed - (40.0 + 50.0)).abs() < 1e-3, "all that was stopped: {absorbed}");
}

#[test]
fn two_pieces_on_one_zone_multiply_and_a_slot_takes_only_its_class() {
    let mut s = world();
    // a plate (50 percent, no durability: it never wears) in the second body place
    let e = s.world.entity(1).unwrap();
    let inv = s.world.get_mut::<Inventory>(e).unwrap();
    (inv.id[6], inv.count[6]) = (3, 1);
    s.step();
    assert!((hit(&mut s, 100.0, 1).0 - 100.0 * 0.6 * 0.5).abs() < 1e-4, "40 and 50 percent stop 70 percent together");
    assert_eq!(slot(&s, 6), (3, 0.0), "a piece with no durability never wears");
    let (table, _) = table_of(&mut s.world).unwrap();
    assert!(allows(&s.world, e, &table, 4, 1), "a helmet in the head place");
    assert!(!allows(&s.world, e, &table, 4, 2), "a vest is not a helmet");
    assert!(!allows(&s.world, e, &table, 4, 4), "nor is a pistol (class 0)");
    assert!(allows(&s.world, e, &table, 4, 0), "emptying a place is always allowed");
    assert!(allows(&s.world, e, &table, 2, 4), "a slot that is no worn place takes anything");
    assert_eq!(places(&s.world, e).len(), 4);
}

#[test]
fn a_piece_wears_by_what_it_stopped_and_breaks_when_it_is_used_up() {
    let mut s = world();
    s.step();
    // the vest has 50 points: each 100-point hit on the body stops 40 of it
    let (left, ev) = hit(&mut s, 100.0, 1);
    assert!((left - 60.0).abs() < 1e-4 && ev.is_empty());
    assert!((slot(&s, 5).1 - 10.0).abs() < 1e-3, "50 - 40: {:?}", slot(&s, 5));
    let (left, ev) = hit(&mut s, 100.0, 1);
    assert!((left - 60.0).abs() < 1e-4, "the last hit it stops is whole");
    assert_eq!(ev, vec!["item_break".to_string()], "and it breaks");
    assert_eq!(slot(&s, 5), (0, 0.0), "the vest is gone from the place");
    s.step();
    let (left, _) = hit(&mut s, 100.0, 1);
    assert_eq!(left, 100.0, "a broken piece stops nothing");
    assert_eq!(s.world.get::<Equipment>(s.world.entity(1).unwrap()).unwrap().cover[1], 0.0, "and the cover follows");
    assert_eq!(slot(&s, 4).0, 1, "the helmet is not touched by hits on the body");
}

#[test]
fn a_character_with_no_equipment_takes_a_hit_whole_and_wear_can_be_switched_off() {
    let mut s = world();
    spawn(&mut s, 2, r#"{"HitPoints":{"value":100,"max":100}}"#);
    let mut ev = Vec::new();
    assert_eq!(absorb(&mut s.world, 2, 70.0, 1, &mut ev), 70.0);
    let e = s.world.entity(1).unwrap();
    s.world.get_mut::<Equipment>(e).unwrap().wear = 0.0;
    let (left, _) = hit(&mut s, 100.0, 1);
    assert!((left - 60.0).abs() < 1e-4, "it still stops");
    assert_eq!(slot(&s, 5).1, 50.0, "but nothing wears with `wear` 0");
}

#[test]
fn a_real_shot_is_lessened_by_the_vest_and_the_same_world_gives_the_same_state_hash() {
    let run = |armored: bool| {
        let mut s = world();
        if !armored {
            let e = s.world.entity(1).unwrap();
            s.world.get_mut::<Equipment>(e).unwrap().slots = lunaris_ecs::Sym(0);
        }
        // a shooter 10 m away looking at the target's body: one hit of 20 damage, no scatter
        spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0.9,0]},"Weapon":{"active":true,"damage":20,"spread":0,"rate":600,"mag":10,"mag_size":10,"auto":false,"head_mult":1,"limb_mult":1,"recoil":[0,0],"infinite":true}}"#);
        s.step();
        let e = s.world.entity(2).unwrap();
        s.world.get_mut::<components::Weapon>(e).unwrap().fire = true;
        s.step_n(3);
        let hp = s.world.get::<HitPoints>(s.world.entity(1).unwrap()).unwrap().value;
        (hp, s.state_hash())
    };
    let (open, _) = run(false);
    let (worn, h1) = run(true);
    assert!((1000.0 - open - 20.0).abs() < 1e-3, "without gear the shot takes 20: {open}");
    assert!((1000.0 - worn - 12.0).abs() < 1e-3, "the vest stops 40 percent of a body shot: it takes 12: {worn}");
    assert_eq!(run(true).1, h1, "deterministic");
}

#[test]
fn a_piercing_round_ignores_its_share_of_the_armor_and_wears_it_less() {
    let take = |s: &mut Sim, pierce: f32| {
        let mut ev: Vec<(String, u64, Option<[f32; 3]>)> = Vec::new();
        absorb_piercing(&mut s.world, 1, 100.0, 1, pierce, &mut ev)
    };
    let mut s = world();
    s.step();
    assert!((take(&mut s, 0.0) - 60.0).abs() < 1e-4, "no piercing: the vest stops 40 of 100");
    assert!((slot(&s, 5).1 - 10.0).abs() < 1e-3, "and wears 40 of its 50");
    let mut s = world();
    s.step();
    assert!((take(&mut s, 0.5) - 80.0).abs() < 1e-4, "half of it ignored: the vest stops 20");
    assert!((slot(&s, 5).1 - 30.0).abs() < 1e-3, "and wears 20");
    let mut s = world();
    s.step();
    assert_eq!(take(&mut s, 1.0), 100.0, "all of it ignored: the round goes through");
    assert_eq!(slot(&s, 5).1, 50.0, "and the vest does not wear");
    assert!((take(&mut s, f32::NAN) - 60.0).abs() < 1e-3, "a bad number is no piercing");
    assert_eq!(take(&mut s, 7.0), 100.0, "more than 1 is all of it");
}

#[test]
fn a_real_shot_of_a_piercing_gun_goes_through_the_vest() {
    let run = |pierce: f32| {
        let mut s = world();
        spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,0.9,0]}},"Weapon":{{"active":true,"damage":20,"spread":0,"rate":600,"mag":10,"mag_size":10,"auto":false,"head_mult":1,"limb_mult":1,"pierce":{pierce},"recoil":[0,0],"infinite":true}}}}"#));
        s.step();
        let e = s.world.entity(2).unwrap();
        s.world.get_mut::<components::Weapon>(e).unwrap().fire = true;
        s.step_n(3);
        1000.0 - s.world.get::<HitPoints>(s.world.entity(1).unwrap()).unwrap().value
    };
    assert!((run(0.0) - 12.0).abs() < 1e-3, "an ordinary round: the vest stops 40 percent");
    assert!((run(0.5) - 16.0).abs() < 1e-3, "a half-piercing round: 20 percent stopped");
    assert!((run(1.0) - 20.0).abs() < 1e-3, "a full piercing round is not stopped at all");
}
