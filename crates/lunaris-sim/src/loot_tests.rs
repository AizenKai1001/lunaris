//! Loot tables: parsing, weights, ranges, nesting, determinism, filling an inventory, refilling after a looting. No art: item keys and numbers.
use super::*;
use crate::inventory::Inventory;
use crate::components::LootDrop;
use crate::loot::{parse_loot, roll};
use lunaris_json::{parse, Json};

const ITEMS: &str = "item bandage | Bandage | stack=10\nitem pills | Pills | stack=10\nitem pistol | Pistol | stack=1\nitem rifle | Rifle | stack=1\nitem ammo | Ammo | stack=100\n";

const TABLES: &str = "# a clinic mostly has bandages\ntable clinic rolls=2-3 empty=0.0\n  bandage w=9 n=1-3\n  pills w=3\n  @guns w=1\ntable guns rolls=1\n  pistol w=3\n  rifle w=1\n";

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn world() -> Sim {
    let mut s = Sim::new(1);
    let items = ITEMS.replace('\n', "\\n");
    let tables = TABLES.replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"ItemTable":{{"text":"{items}"}}}}"#));
    spawn(&mut s, 2, &format!(r#"{{"LootTable":{{"text":"{tables}"}}}}"#));
    s
}

fn chest(s: &mut Sim, uid: u64, extra: &str) {
    let sep = if extra.is_empty() { "" } else { "," };
    spawn(s, uid, &format!(r#"{{"Inventory":{{"size":12}},"LootContainer":{{"table":"clinic"{sep}{extra}}}}}"#));
}

/// What a chest holds, as item key and count, by key order.
fn held(s: &mut Sim, uid: u64) -> Vec<(String, u32)> {
    let (table, _) = crate::inventory::table_of(&mut s.world).unwrap();
    let inv = *s.world.get::<Inventory>(s.world.entity(uid).unwrap()).unwrap();
    let mut by: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for k in 0..12 {
        if inv.id[k] != 0 {
            *by.entry(table.item(inv.id[k]).unwrap().key.clone()).or_default() += inv.count[k];
        }
    }
    by.into_iter().collect()
}

#[test]
fn the_text_is_read_and_what_is_wrong_with_it_is_said() {
    let items = crate::inventory::parse_table(ITEMS);
    let l = parse_loot(TABLES, Some(&items));
    assert!(l.problems.is_empty(), "{:?}", l.problems);
    assert_eq!(l.tables.len(), 2);
    let c = &l.tables["clinic"];
    assert_eq!((c.rolls, c.empty, c.entries.len()), ((2, 3), 0.0, 3));
    assert_eq!((c.entries[0].min, c.entries[0].max, c.entries[0].weight), (1, 3, 9.0));
    let bad = parse_loot("stray w=1\ntable t rolls=x\n  nothing w=1\n  @missing\ntable empty_one\n", Some(&items));
    let text = bad.problems.join("\n");
    assert!(text.contains("rolls=x") && text.contains("`nothing`") && text.contains("@missing") && text.contains("outside a table") && text.contains("no entries"), "{text}");
}

#[test]
fn a_chest_is_filled_from_its_table_the_same_every_time_and_each_key_differs() {
    let fill = |key: u32| {
        let mut s = world();
        chest(&mut s, 10, &format!(r#""key":{key}"#));
        s.step_n(3);
        held(&mut s, 10)
    };
    let a = fill(7);
    assert!(!a.is_empty(), "something is in it");
    assert_eq!(a, fill(7), "the same container holds the same things in every world");
    let keys: std::collections::BTreeSet<Vec<(String, u32)>> = (1..30).map(fill).collect();
    assert!(keys.len() > 8, "different containers hold different things: {} kinds in 29", keys.len());
    // it is filled once, not every tick
    let mut s = world();
    chest(&mut s, 10, r#""key":7"#);
    s.step_n(3);
    let before = held(&mut s, 10);
    s.step_n(60);
    assert_eq!(held(&mut s, 10), before);
    assert_eq!(s.world.get::<crate::components::LootContainer>(s.world.entity(10).unwrap()).unwrap().fills, 1);
}

#[test]
fn weights_ranges_and_nesting_are_respected() {
    let items = crate::inventory::parse_table(ITEMS);
    let l = parse_loot(TABLES, Some(&items));
    let (mut bandage, mut pills, mut pistol, mut rifle) = (0u32, 0u32, 0u32, 0u32);
    let mut ranges_ok = true;
    for key in 1..=2000u64 {
        let got = roll(&l, "clinic", key, 0);
        for (k, n) in &got {
            match k.as_str() {
                "bandage" => bandage += n,
                "pills" => pills += n,
                "pistol" => pistol += n,
                "rifle" => rifle += n,
                _ => ranges_ok = false,
            }
        }
        let rolls = got.iter().map(|(_, n)| *n).sum::<u32>();
        // 2 or 3 rolls of at most 3 bandages each
        ranges_ok &= rolls >= 2 && rolls <= 9;
    }
    assert!(ranges_ok);
    // by weight: bandage 9, pills 3, the nested table 1 (and in it pistol 3 to rifle 1): each roll is a bandage stack 69 percent of the time
    let rolls = 2500.0f32; // (2 or 3 rolls: about 2.5 each for 2000 containers is 5000 rolls; the ratios are what count)
    let _ = rolls;
    let ratio = |a: u32, b: u32| a as f32 / b.max(1) as f32;
    assert!(pills > 0 && pistol > 0 && rifle > 0, "every entry comes up: {pills} {pistol} {rifle}");
    assert!(ratio(pistol, rifle) > 2.0 && ratio(pistol, rifle) < 4.5, "the nested table keeps its own weights (3 to 1): {pistol} {rifle}");
    assert!(ratio(bandage, pills) > 4.0 && ratio(bandage, pills) < 8.0, "bandages (9, and 2 on average) against pills (3): {bandage} {pills}");
    // the empty chance
    let l = parse_loot("table t rolls=1 empty=0.5\n  pills\n", Some(&items));
    let nothing = (1..=2000u64).filter(|k| roll(&l, "t", *k, 0).is_empty()).count();
    assert!((nothing as f32 - 1000.0).abs() < 120.0, "half the rolls give nothing: {nothing}");
    // a loop of tables ends instead of running for ever
    let l = parse_loot("table a rolls=1\n  @b\ntable b rolls=1\n  @a\n", None);
    assert!(roll(&l, "a", 1, 0).is_empty());
}

#[test]
fn a_looted_chest_is_filled_again_after_the_refill_time_and_a_full_one_is_left_alone() {
    let mut s = world();
    chest(&mut s, 10, r#""key":3,"refill":5"#);
    s.step_n(3);
    let first = held(&mut s, 10);
    assert!(!first.is_empty());
    // not looted: nothing happens after the time is up
    s.step_n(60 * 7);
    assert_eq!(held(&mut s, 10), first);
    assert_eq!(s.world.get::<crate::components::LootContainer>(s.world.entity(10).unwrap()).unwrap().fills, 1);
    // looted: take everything, and after the refill time it is filled again, with the second fill's own roll
    let (table, _) = crate::inventory::table_of(&mut s.world).unwrap();
    {
        let e = s.world.entity(10).unwrap();
        let inv = s.world.get_mut::<Inventory>(e).unwrap();
        for k in 0..12 {
            inv.id[k] = 0;
            inv.count[k] = 0;
        }
    }
    let _ = table;
    s.step_n(4);
    let c = *s.world.get::<crate::components::LootContainer>(s.world.entity(10).unwrap()).unwrap();
    assert_eq!(c.fills, 2, "filled again");
    assert!(!held(&mut s, 10).is_empty());
    // set filled false to fill at once
    let e = s.world.entity(10).unwrap();
    s.world.get_mut::<crate::components::LootContainer>(e).unwrap().filled = false;
    s.step_n(3);
    assert_eq!(s.world.get::<crate::components::LootContainer>(e).unwrap().fills, 3);
}

/// The pickups on the ground (uid, item text of `Interactable.item`, position).
fn pickups(s: &mut Sim) -> Vec<(u64, String, [f32; 3])> {
    let mut out = Vec::new();
    let _ = s.world.for_each_uid::<(&crate::components::Interactable, &crate::components::Transform), _>(|_, uid, (i, t)| out.push((uid, i.item, t.pos)));
    out.into_iter().map(|(uid, sym, pos)| (uid, s.world.strings().get(sym.0).to_string(), pos)).collect()
}

#[test]
fn a_destroyed_machine_drops_what_its_table_rolls_once_in_a_ring_and_the_same_every_time() {
    let run = |key: u32| {
        let mut s = world();
        s.apply(&Command::DefinePrefab { name: "Pickup".into(), def: parse(r#"{"components":{"Name":{"value":"Pickup"},"Transform":{},"Interactable":{"kind":3,"prompt":"Take","radius":0.7}}}"#).unwrap() }).unwrap();
        // a floor 0.5 m thick whose top is at y = 0: the machine dies 7 m above it
        spawn(&mut s, 5, r#"{"Transform":{"pos":[0,-0.25,0],"scale":[40,0.5,40]},"Collider":{"shape":1,"size":[1,1,1]}}"#);
        spawn(&mut s, 10, &format!(r#"{{"Transform":{{"pos":[4,7,-2]}},"HitPoints":{{"value":30,"max":30}},"LootDrop":{{"table":"clinic","prefab":"Pickup","key":{key},"spread":1.0,"lift":0.5}}}}"#));
        s.step_n(3);
        assert!(pickups(&mut s).is_empty(), "nothing is dropped while it lives");
        s.world.get_mut::<crate::components::HitPoints>(s.world.entity(10).unwrap()).unwrap().value = 0.0;
        s.step_n(3);
        let first = pickups(&mut s);
        s.step_n(20);
        assert_eq!(pickups(&mut s), first, "dropped once, not again");
        assert!(s.world.get::<LootDrop>(s.world.entity(10).unwrap()).unwrap().done);
        first
    };
    let a = run(7);
    assert!(!a.is_empty() && a.len() <= 4, "{a:?}");
    for (_, item, pos) in &a {
        let d = ((pos[0] - 4.0).powi(2) + (pos[2] + 2.0).powi(2)).sqrt();
        assert!((d - 1.0).abs() < 1e-3 && (pos[1] - 0.5).abs() < 0.02, "a ring of 1 m, 0.5 m above the floor (not 7 m up in the air): {pos:?}");
        assert!(item.starts_with("bandage ") || item.starts_with("pills ") || item.starts_with("pistol ") || item.starts_with("rifle "), "{item}");
    }
    assert_eq!(run(7), a, "the same key drops the same things at the same places");
    assert_ne!(run(8).iter().map(|p| p.1.clone()).collect::<Vec<_>>(), a.iter().map(|p| p.1.clone()).collect::<Vec<_>>(), "another key, another roll");
}

#[test]
fn a_drop_without_its_prefab_or_table_makes_nothing_and_does_not_try_again() {
    let mut s = world();
    spawn(&mut s, 10, r#"{"Transform":{},"HitPoints":{"value":0,"max":10},"LootDrop":{"table":"clinic","prefab":"Missing"}}"#);
    spawn(&mut s, 11, r#"{"Transform":{},"HitPoints":{"value":0,"max":10},"LootDrop":{"table":"nothing_like_it","prefab":"Pickup"}}"#);
    s.step_n(5);
    assert!(pickups(&mut s).is_empty());
    assert!(s.world.get::<LootDrop>(s.world.entity(10).unwrap()).unwrap().done && s.world.get::<LootDrop>(s.world.entity(11).unwrap()).unwrap().done);
}

#[test]
fn a_drop_with_ground_off_falls_from_where_the_thing_was() {
    let mut s = world();
    s.apply(&Command::DefinePrefab { name: "Pickup".into(), def: parse(r#"{"components":{"Name":{"value":"Pickup"},"Transform":{},"Interactable":{"kind":3}}}"#).unwrap() }).unwrap();
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,-0.25,0],"scale":[40,0.5,40]},"Collider":{"shape":1,"size":[1,1,1]}}"#);
    spawn(&mut s, 10, r#"{"Transform":{"pos":[4,7,-2]},"HitPoints":{"value":0,"max":30},"LootDrop":{"table":"clinic","prefab":"Pickup","lift":0.5,"ground":false}}"#);
    s.step_n(5);
    let p = pickups(&mut s);
    assert!(!p.is_empty() && p.iter().all(|(_, _, pos)| (pos[1] - 7.5).abs() < 1e-3), "{p:?}");
}

#[test]
fn the_ring_of_a_drop_stops_in_front_of_a_wall() {
    let mut s = world();
    s.apply(&Command::DefinePrefab { name: "Pickup".into(), def: parse(r#"{"components":{"Name":{"value":"Pickup"},"Transform":{},"Interactable":{"kind":3,"prompt":"Take","radius":0.7}}}"#).unwrap() }).unwrap();
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,-0.25,0],"scale":[40,0.5,40]},"Collider":{"shape":1,"size":[1,1,1]}}"#);
    // a wall 4 m tall and 1 m thick whose near face is 0.5 m from the machine (ring point 0 is on its +x side)
    spawn(&mut s, 6, r#"{"Transform":{"pos":[5,2,-2]},"Collider":{"shape":1,"size":[1,4,12]}}"#);
    spawn(&mut s, 10, r#"{"Transform":{"pos":[4,1,-2]},"HitPoints":{"value":30,"max":30},"LootDrop":{"table":"clinic","prefab":"Pickup","key":7,"spread":1.0,"lift":0.5}}"#);
    s.step_n(3);
    s.world.get_mut::<crate::components::HitPoints>(s.world.entity(10).unwrap()).unwrap().value = 0.0;
    s.step_n(3);
    let p = pickups(&mut s);
    assert!(!p.is_empty());
    for (uid, item, pos) in &p {
        let inside = pos[0] > 4.5 && pos[0] < 5.5 && pos[1] > 0.0 && pos[1] < 4.0 && pos[2] > -8.0 && pos[2] < 4.0;
        assert!(!inside, "pickup {uid} `{item}` lies inside the wall at {pos:?}");
    }
}

#[test]
fn a_table_that_rolls_itself_many_times_ends_when_its_budget_is_spent_and_a_huge_range_does_not_overflow() {
    for (rolls, n) in [(3, 4), (4, 6)] {
        let text = format!("table a rolls={rolls}\n  @a n={n}\n");
        let loot = parse_loot(&text, None);
        let t = std::time::Instant::now();
        let _ = roll(&loot, "a", 1, 0);
        assert!(t.elapsed() < std::time::Duration::from_millis(250), "rolls={rolls} n={n} took {:?}", t.elapsed());
    }
    let loot = parse_loot("table a rolls=0-4294967295\n  bandage n=0-4294967295\n", None);
    let r = std::panic::catch_unwind(|| roll(&loot, "a", 1, 0));
    assert!(r.is_ok(), "a range up to u32::MAX must not overflow");
}
