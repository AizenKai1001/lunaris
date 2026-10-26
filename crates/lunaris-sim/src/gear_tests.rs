//! Worn gear that others see: the mask the server keeps for a character (a bit for each worn place that holds something), and the parts of a mannequin that a client shows by it.
use super::*;
use crate::armor::absorb;
use crate::components::{MeshRenderer, NetPuppet};
use crate::gear::{mask_of, WornPart, WornTell, MASK, MAX_PLACES};
use crate::inventory::Inventory;
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

// the items: 1 helmet (class 1), 2 vest (class 2), 3 pants (class 3), 4 boots (class 4), 5 pistol (no class)
const ITEMS: &str = "item helmet | Helmet | stack=1 class=1 power=35 dur=100\\nitem vest | Vest | stack=1 class=2 power=40 dur=50\\nitem pants | Pants | stack=1 class=3 power=25 dur=100\\nitem boots | Boots | stack=1 class=4 power=15 dur=100\\nitem pistol | Pistol | stack=1\\n";
// the places of the game: 12 head, 13 chest, 14 legs, 15 feet (a bag of 16)
const PLACES: &str = "slot 12 accepts=1 zones=head\\nslot 13 accepts=2 zones=body\\nslot 14 accepts=3 zones=legs\\nslot 15 accepts=4 zones=legs";

/// A character (uid 1) with a bag of 16 whose slots 12 to 15 are worn places (empty) and a pistol in slot 0; `tell` gives it a `WornTell`.
fn world(tell: bool) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 50, &format!(r#"{{"ItemTable":{{"text":"{ITEMS}"}}}}"#));
    let tell = if tell { r#","WornTell":{}"# } else { "" };
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"HitPoints":{{"value":1000,"max":1000}},"Inventory":{{"size":16,"bag":12}},"Equipment":{{"slots":"{PLACES}"}}{tell}}}"#));
    put(&mut s, 0, 5, 1, 0.0);
    s
}

/// Put `count` of item `id` (with `dur` of durability) into slot `k` of the character's bag.
fn put(s: &mut Sim, k: usize, id: u32, count: u32, dur: f32) {
    let e = s.world.entity(1).unwrap();
    let inv = s.world.get_mut::<Inventory>(e).unwrap();
    (inv.id[k], inv.count[k], inv.dur[k]) = (id, count, dur);
}

fn mask(s: &Sim) -> u32 {
    s.world.get::<WornTell>(s.world.entity(1).unwrap()).unwrap().mask
}

#[test]
fn the_mask_has_a_bit_for_each_worn_place_that_holds_something_and_follows_the_bag() {
    let mut s = world(true);
    s.step();
    assert_eq!(mask(&s), 0, "a pistol in a carried slot is not worn");
    put(&mut s, 12, 1, 1, 100.0);
    s.step();
    assert_eq!(mask(&s), 0b0001, "a helmet on the head");
    put(&mut s, 13, 2, 1, 50.0);
    put(&mut s, 15, 4, 1, 100.0);
    s.step();
    assert_eq!(mask(&s), 0b1011, "a helmet, a vest and boots");
    put(&mut s, 13, 0, 0, 0.0);
    s.step();
    assert_eq!(mask(&s), 0b1001, "the vest taken off: its bit goes at the next step");
    put(&mut s, 12, 1, 0, 100.0);
    s.step();
    assert_eq!(mask(&s), 0b1000, "an item with a count of 0 is nothing");
    put(&mut s, 14, 3, 1, 100.0);
    put(&mut s, 12, 1, 1, 100.0);
    put(&mut s, 13, 2, 1, 50.0);
    s.step();
    assert_eq!(mask(&s), 0b1111, "all four");
    assert_eq!(mask(&s) & !MASK, 0);
}

#[test]
fn only_a_character_with_a_worn_tell_gets_a_mask_and_a_character_with_no_places_wears_nothing() {
    let mut s = world(false);
    put(&mut s, 12, 1, 1, 100.0);
    s.step();
    let e = s.world.entity(1).unwrap();
    assert!(s.world.get::<WornTell>(e).is_none(), "nothing is added to a character that did not ask");
    assert_eq!(mask_of(&s.world, e), 0b0001, "the function still answers for it");
    // a character with a bag and a tell but no `Equipment`
    spawn(&mut s, 2, r#"{"Inventory":{"size":16,"bag":12},"WornTell":{}}"#);
    let bare = s.world.entity(2).unwrap();
    s.world.get_mut::<Inventory>(bare).unwrap().id[12] = 1;
    s.world.get_mut::<Inventory>(bare).unwrap().count[12] = 1;
    s.step();
    assert_eq!(s.world.get::<WornTell>(bare).unwrap().mask, 0, "no worn places, nothing worn");
    // a character with places but no bag
    spawn(&mut s, 3, &format!(r#"{{"Equipment":{{"slots":"{PLACES}"}},"WornTell":{{}}}}"#));
    s.step();
    assert_eq!(s.world.get::<WornTell>(s.world.entity(3).unwrap()).unwrap().mask, 0);
}

#[test]
fn a_piece_that_wears_out_is_gone_from_the_mask_and_only_the_first_seven_places_count() {
    let mut s = world(true);
    put(&mut s, 13, 2, 1, 12.0);
    put(&mut s, 12, 1, 1, 100.0);
    s.step();
    assert_eq!(mask(&s), 0b0011);
    // two body hits of 20: the vest (40 percent) stops 8 each and breaks on the second (12 points of durability)
    let mut ev: Vec<(String, u64, Option<[f32; 3]>)> = Vec::new();
    absorb(&mut s.world, 1, 20.0, 1, &mut ev);
    assert!(ev.is_empty(), "the first hit leaves the vest worn");
    s.step();
    assert_eq!(mask(&s), 0b0011, "damaged is still worn");
    absorb(&mut s.world, 1, 20.0, 1, &mut ev);
    assert_eq!(ev.iter().filter(|e| e.0 == "item_break").count(), 1, "the vest breaks");
    s.step();
    assert_eq!(mask(&s), 0b0001, "the helmet is left");
    // ten places: only the first seven can be told
    let mut text = String::new();
    for k in 0..10 {
        text.push_str(&format!("slot {} accepts=0 zones=body\\n", 20 + k));
    }
    spawn(&mut s, 4, &format!(r#"{{"Inventory":{{"size":32,"bag":12}},"Equipment":{{"slots":"{text}"}},"WornTell":{{}}}}"#));
    let e = s.world.entity(4).unwrap();
    for k in 20..30 {
        let inv = s.world.get_mut::<Inventory>(e).unwrap();
        (inv.id[k], inv.count[k]) = (1, 1);
    }
    s.step();
    assert_eq!(s.world.get::<WornTell>(e).unwrap().mask, (1 << MAX_PLACES) - 1);
}

/// A puppet as a client makes it: the root (uid `root`), a mannequin under it that is drawn or not, and one part under the mannequin for each of the places 0 to 3 and one for place 9 (never drawn).
fn puppet(s: &mut Sim, root: u64, anim: u32, drawn: bool) -> Vec<u64> {
    spawn(s, root, r#"{"Transform":{},"NetPuppet":{}}"#);
    let man = root + 1;
    spawn(s, man, &format!(r#"{{"Transform":{{}},"Parent":{{"target":{root}}},"MeshRenderer":{{"mesh":"cube","visible":{drawn}}},"NetPuppet":{{"anim":{anim}}}}}"#));
    let mut parts = Vec::new();
    for (k, place) in [0u32, 1, 2, 3, 9].into_iter().enumerate() {
        let uid = root + 2 + k as u64;
        spawn(s, uid, &format!(r#"{{"Transform":{{}},"Parent":{{"target":{man}}},"MeshRenderer":{{"mesh":"cube","visible":false}},"WornPart":{{"place":{place}}}}}"#));
        parts.push(uid);
    }
    parts
}

fn drawn(s: &Sim, uid: u64) -> bool {
    s.world.get::<MeshRenderer>(s.world.entity(uid).unwrap()).unwrap().visible
}

fn set_anim(s: &mut Sim, uid: u64, anim: u32) {
    s.world.get_mut::<NetPuppet>(s.world.entity(uid).unwrap()).unwrap().anim = anim;
}

fn set_drawn(s: &mut Sim, uid: u64, v: bool) {
    s.world.get_mut::<MeshRenderer>(s.world.entity(uid).unwrap()).unwrap().visible = v;
}

#[test]
fn a_part_is_drawn_exactly_while_its_bit_is_set_on_a_mannequin_that_is_drawn() {
    let mut s = Sim::new(1);
    let parts = puppet(&mut s, 100, 0b0101, true);
    s.step();
    let seen: Vec<bool> = parts.iter().map(|&u| drawn(&s, u)).collect();
    assert_eq!(seen, [true, false, true, false, false], "the helmet (bit 0) and the pants (bit 2); the vest, the boots and the part of place 9 are not");
    // the mask changes: a vest on, the helmet off
    set_anim(&mut s, 101, 0b0010);
    s.step();
    let seen: Vec<bool> = parts.iter().map(|&u| drawn(&s, u)).collect();
    assert_eq!(seen, [false, true, false, false, false]);
    // bits above the seven are not places, and a part of place 9 is never drawn even when the whole field is set
    set_anim(&mut s, 101, 0xffff);
    s.step();
    let seen: Vec<bool> = parts.iter().map(|&u| drawn(&s, u)).collect();
    assert_eq!(seen, [true, true, true, true, false]);
    // the mannequin goes (a far puppet is a box, a level of detail hides it): every part goes with it, and comes back with it
    set_drawn(&mut s, 101, false);
    s.step();
    assert!(parts.iter().all(|&u| !drawn(&s, u)), "no gear on a mannequin that is not drawn");
    set_drawn(&mut s, 101, true);
    s.step();
    let seen: Vec<bool> = parts.iter().map(|&u| drawn(&s, u)).collect();
    assert_eq!(seen, [true, true, true, true, false]);
    // nothing worn: nothing drawn
    set_anim(&mut s, 101, 0);
    s.step();
    assert!(parts.iter().all(|&u| !drawn(&s, u)));
}

#[test]
fn puppets_are_dressed_each_by_its_own_mannequin_whatever_the_order_of_their_parts() {
    let mut s = Sim::new(1);
    let a = puppet(&mut s, 100, 0b0001, true);
    let b = puppet(&mut s, 200, 0b1000, true);
    let c = puppet(&mut s, 300, 0b1111, false);
    s.step();
    let seen = |s: &Sim, p: &[u64]| p.iter().map(|&u| drawn(s, u)).collect::<Vec<_>>();
    assert_eq!(seen(&s, &a), [true, false, false, false, false]);
    assert_eq!(seen(&s, &b), [false, false, false, true, false]);
    assert_eq!(seen(&s, &c), [false; 5], "a hidden mannequin shows nothing, whatever its mask");
    // a part whose mannequin has been taken away is not drawn
    set_drawn(&mut s, a[0], true);
    let man = s.world.entity(101).unwrap();
    s.world.despawn(man);
    s.step();
    assert!(!drawn(&s, a[0]), "its mannequin is gone");
    assert_eq!(seen(&s, &b), [false, false, false, true, false], "the others are as they were");
}

/// The cost of a client with a crowd: 100 puppets in the three variants of the game (24 parts each, a third of the mannequins drawn, half of them armored), one `gear::update` a step.
#[test]
fn the_cost_of_the_gear_of_a_crowd_is_small_and_is_measured() {
    let mut s = Sim::new(1);
    let mut all: Vec<(u64, u64)> = Vec::new();
    for i in 0..100u64 {
        let root = 1000 + i * 100;
        spawn(&mut s, root, r#"{"Transform":{},"NetPuppet":{}}"#);
        for v in 0..3u64 {
            let man = root + 1 + v * 10;
            let drawn = v == (i % 3);
            let anim = if i % 2 == 0 { 0b1111 } else { 0 };
            spawn(&mut s, man, &format!(r#"{{"Transform":{{}},"Parent":{{"target":{root}}},"MeshRenderer":{{"mesh":"cube","visible":{drawn}}},"NetPuppet":{{"anim":{anim}}}}}"#));
            all.push((man, anim as u64));
            for (k, place) in [0u32, 1, 2, 2, 2, 2, 3, 3].into_iter().enumerate() {
                spawn(&mut s, man + 1 + k as u64, &format!(r#"{{"Transform":{{}},"Parent":{{"target":{man}}},"MeshRenderer":{{"mesh":"cube","visible":false}},"WornPart":{{"place":{place}}}}}"#));
            }
        }
    }
    let mut worn = 0;
    let _ = s.world.for_each::<(&WornPart, &MeshRenderer), _>(|_, _| worn += 1);
    assert_eq!(worn, 2400);
    crate::gear::update(&mut s.world);
    let mut on = 0;
    let _ = s.world.for_each::<(&WornPart, &MeshRenderer), _>(|_, (_, m)| on += m.visible as u32);
    // (the even puppets are armored, one mannequin of each is drawn, and all four places are worn: its 8 parts)
    let expect = (0..100u64).filter(|i| i % 2 == 0).count() as u32 * 8;
    assert_eq!(on, expect, "every part of an armored puppet's drawn mannequin is on, and nothing else");
    let t = std::time::Instant::now();
    let n = 300;
    for _ in 0..n {
        crate::gear::update(&mut s.world);
    }
    let per_step = t.elapsed().as_secs_f64() / n as f64;
    eprintln!("gear::update with 100 puppets, 2400 parts: {:.1} us a step, {:.0} ns a part", per_step * 1e6, per_step * 1e9 / 2400.0);
    assert!(per_step < 0.005, "a step of the gear of 100 puppets took {:.2} ms", per_step * 1e3);
    let _ = all;
}
