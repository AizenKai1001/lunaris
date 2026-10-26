//! Guns that are items: the weapon is the gun in hand, a magazine that is kept for each slot, a reload that takes the ammunition out of the bag, nothing in hand or no gun in hand and the weapon is off. No art: numbers.
use super::*;
use crate::components::{Loadout, Weapon};
use crate::inventory::Inventory;
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

const ITEMS: &str = "item pistol | Pistol | stack=1\\nitem rifle | Rifle | stack=1\\nitem ammo9 | 9 mm | stack=60\\nitem ammo556 | 5.56 | stack=90\\nitem bandage | Bandage | stack=5\\nitem flare | Flare gun | stack=1\\n";
const GUNS: &str = "# the guns of the test\\ngun pistol ammo=ammo9 damage=20 rate=300 mag=10 reload=1.0 auto=0 spread=0\\ngun rifle ammo=ammo556 damage=35 rate=600 mag=30 reload=2.0 auto=1 spread=0 pellets=1\\ngun flare spread=0 mag=2\\n";

// the items are numbered from 1 in the order of the table
const PISTOL: u32 = 1;
const RIFLE: u32 = 2;
const AMMO9: u32 = 3;
const AMMO556: u32 = 4;
const BANDAGE: u32 = 5;
const FLARE: u32 = 6;

/// A character (uid 1) with a bag of 12 and a head (uid 2) that carries the weapon and the loadout, looking down -z at a dummy target 10 m away.
fn world() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 50, &format!(r#"{{"ItemTable":{{"text":"{ITEMS}"}}}}"#));
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Inventory":{"size":12}}"#);
    spawn(
        &mut s,
        2,
        &format!(r#"{{"Parent":{{"target":1}},"Transform":{{"pos":[0,1.2,0]}},"LocalTransform":{{"pos":[0,1.2,0]}},"Weapon":{{"active":false,"spread":0,"recoil":[0,0]}},"Loadout":{{"guns":"{GUNS}"}}}}"#),
    );
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,0.9,-10]},"Collider":{"shape":2,"size":[0.7,1.8,0.7]},"HitPoints":{"value":1000,"max":1000}}"#);
    s
}

fn bag(s: &mut Sim, slots: &[(usize, u32, u32)]) {
    let e = s.world.entity(1).unwrap();
    let inv = s.world.get_mut::<Inventory>(e).unwrap();
    inv.id = [0; 48];
    inv.count = [0; 48];
    for &(k, id, n) in slots {
        inv.id[k] = id;
        inv.count[k] = n;
    }
}

fn held(s: &mut Sim, slot: u32) {
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<Loadout>(e).unwrap().held = slot;
}

fn weapon(s: &Sim) -> Weapon {
    *s.world.get::<Weapon>(s.world.entity(2).unwrap()).unwrap()
}

fn load(s: &Sim) -> Loadout {
    *s.world.get::<Loadout>(s.world.entity(2).unwrap()).unwrap()
}

fn count(s: &Sim, item: u32) -> u32 {
    let inv = s.world.get::<Inventory>(s.world.entity(1).unwrap()).unwrap();
    (0..12).filter(|&k| inv.id[k] == item).map(|k| inv.count[k]).sum()
}

fn trigger(s: &mut Sim, on: bool) {
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<Weapon>(e).unwrap().fire = on;
}

#[test]
fn the_gun_in_hand_is_the_weapon_and_nothing_else_in_hand_switches_it_off() {
    let mut s = world();
    bag(&mut s, &[(0, PISTOL, 1), (1, RIFLE, 1), (2, BANDAGE, 3), (3, AMMO9, 30), (4, AMMO556, 90)]);
    held(&mut s, 9);
    s.step();
    assert!(!weapon(&s).active, "the weapon of a character whose hand is empty is off");
    held(&mut s, 0);
    s.step();
    let w = weapon(&s);
    assert!(w.active && (w.damage, w.rate, w.mag_size, w.mag, w.reload_time, w.auto) == (20.0, 300.0, 10, 10, 1.0, false), "{w:?}");
    assert_eq!(w.reserve, 30, "its reserve is the ammunition in the bag");
    held(&mut s, 1);
    s.step();
    let w = weapon(&s);
    assert!((w.damage, w.rate, w.mag_size, w.auto, w.reserve) == (35.0, 600.0, 30, true, 90), "{w:?}");
    held(&mut s, 2);
    s.step();
    assert!(!weapon(&s).active, "a bandage is no gun");
    held(&mut s, 7);
    s.step();
    assert!(!weapon(&s).active, "an empty slot is no gun");
}

#[test]
fn firing_empties_the_magazine_and_a_reload_takes_the_ammunition_out_of_the_bag() {
    let mut s = world();
    bag(&mut s, &[(0, RIFLE, 1), (1, AMMO556, 50)]);
    held(&mut s, 0);
    s.step();
    assert_eq!(weapon(&s).mag, 30);
    trigger(&mut s, true);
    for _ in 0..60 {
        s.step();
    }
    trigger(&mut s, false);
    let w = weapon(&s);
    assert!(w.mag < 30 && w.shots >= 10, "it fired: {w:?}");
    let fired = 30 - w.mag;
    // nothing was taken from the bag yet: the rounds in the magazine are the gun's
    assert_eq!(count(&s, AMMO556), 50);
    // reload: the magazine is full again and what it took is gone from the bag
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<Weapon>(e).unwrap().reload = true;
    for _ in 0..180 {
        s.step();
    }
    assert_eq!(weapon(&s).mag, 30, "{:?}", weapon(&s));
    assert_eq!(count(&s, AMMO556), 50 - fired, "the bag paid for the rounds that went into the magazine");
    assert_eq!(load(&s).took, fired);
    assert_eq!(weapon(&s).reserve, 50 - fired);
}

#[test]
fn each_gun_keeps_its_magazine_while_another_is_in_hand_and_ammunition_decides_what_a_reload_can_do() {
    let mut s = world();
    bag(&mut s, &[(0, PISTOL, 1), (1, RIFLE, 1), (2, AMMO9, 4)]); // (no rifle ammunition at all; only four pistol rounds)
    held(&mut s, 0);
    s.step();
    trigger(&mut s, true);
    s.step();
    trigger(&mut s, false);
    for _ in 0..30 {
        s.step();
    }
    assert_eq!(weapon(&s).mag, 9, "one round fired of ten");
    held(&mut s, 1);
    s.step();
    assert_eq!((weapon(&s).mag, weapon(&s).reserve), (30, 0), "the rifle starts with a full magazine and no ammunition behind it");
    trigger(&mut s, true);
    for _ in 0..20 {
        s.step();
    }
    trigger(&mut s, false);
    let rifle_mag = weapon(&s).mag;
    assert!(rifle_mag < 30);
    held(&mut s, 0);
    s.step();
    assert_eq!(weapon(&s).mag, 9, "the pistol's magazine is as it was left");
    held(&mut s, 1);
    s.step();
    assert_eq!(weapon(&s).mag, rifle_mag, "and the rifle's");
    // a reload with no rifle ammunition does nothing; the pistol's reload takes what there is (4 of the 9 it needs... a magazine of 10 with 9 in it takes 1)
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<Weapon>(e).unwrap().reload = true;
    for _ in 0..180 {
        s.step();
    }
    assert_eq!(weapon(&s).mag, rifle_mag, "no ammunition, no reload");
    held(&mut s, 0);
    s.step();
    s.world.get_mut::<Weapon>(e).unwrap().reload = true;
    for _ in 0..120 {
        s.step();
    }
    assert_eq!((weapon(&s).mag, count(&s, AMMO9)), (10, 3), "one round was needed, one left the bag");
}

#[test]
fn a_gun_with_no_ammunition_item_never_runs_out_and_a_gun_that_is_moved_away_is_not_in_hand() {
    let mut s = world();
    bag(&mut s, &[(0, FLARE, 1), (1, PISTOL, 1)]);
    held(&mut s, 0);
    s.step();
    let w = weapon(&s);
    assert!(w.active && w.infinite && w.mag_size == 2, "{w:?}");
    // the item is taken out of the slot (dropped, given, stolen): the next tick the hand is empty
    bag(&mut s, &[(1, PISTOL, 1)]);
    s.step();
    assert!(!weapon(&s).active);
    assert_eq!(load(&s).holding, 0);
}

#[test]
fn the_same_world_gives_the_same_state_hash() {
    let run = || {
        let mut s = world();
        bag(&mut s, &[(0, RIFLE, 1), (1, AMMO556, 60), (2, PISTOL, 1), (3, AMMO9, 20)]);
        held(&mut s, 0);
        s.step();
        trigger(&mut s, true);
        for _ in 0..40 {
            s.step();
        }
        held(&mut s, 2);
        for _ in 0..40 {
            s.step();
        }
        trigger(&mut s, false);
        s.state_hash()
    };
    assert_eq!(run(), run());
}

#[test]
fn the_text_of_a_loadout_is_read_and_its_mistakes_named() {
    use crate::loadout::parse_guns;
    let (g, p) = parse_guns("gun rifle ammo=ammo556 damage=35 rate=600 mag=30 reload=2 auto=1 pellets=1 # a comment\ngun shotgun pellets=8 damage=9 mag=6 kind=0\n");
    assert!(p.is_empty(), "{p:?}");
    assert_eq!((g[0].damage, g[0].mag, g[0].auto, g[0].ammo.as_str()), (35.0, 30, true, "ammo556"));
    assert_eq!((g[1].pellets, g[1].damage, g[1].mag), (8, 9.0, 6));
    let (g, p) = parse_guns("weapon x\ngun\ngun a damage=lots\ngun b nonsense\ngun c colour=red\ngun d\ngun d\n");
    assert_eq!(g.len(), 4, "a, b, c and the first d are guns: {g:?}");
    assert_eq!(p.len(), 6, "{p:#?}");
}

#[test]
fn the_guns_last_read_are_found_again_by_their_text_whatever_its_length() {
    // two worlds one after the other on one thread whose texts of guns are as long as each other and differ in one number: each has its own guns
    let guns_b = GUNS.replace("damage=20", "damage=25");
    assert_eq!(guns_b.len(), GUNS.len());
    let mut a = world();
    bag(&mut a, &[(0, PISTOL, 1), (1, AMMO9, 30)]);
    held(&mut a, 0);
    a.step();
    assert_eq!(weapon(&a).damage, 20.0);
    let mut b = Sim::new(1);
    spawn(&mut b, 50, &format!(r#"{{"ItemTable":{{"text":"{ITEMS}"}}}}"#));
    spawn(&mut b, 1, r#"{"Transform":{"pos":[0,0,0]},"Inventory":{"size":12}}"#);
    spawn(&mut b, 2, &format!(r#"{{"Parent":{{"target":1}},"Transform":{{"pos":[0,1.2,0]}},"LocalTransform":{{"pos":[0,1.2,0]}},"Weapon":{{"active":false,"spread":0,"recoil":[0,0]}},"Loadout":{{"guns":"{guns_b}"}}}}"#));
    bag(&mut b, &[(0, PISTOL, 1), (1, AMMO9, 30)]);
    held(&mut b, 0);
    b.step();
    assert_eq!(weapon(&b).damage, 25.0, "the guns of the second world, not the first's");
    a.step();
    held(&mut a, 1);
    held(&mut a, 0);
    a.step();
    assert_eq!(weapon(&a).damage, 20.0, "and the first world is read again with its own");
}

/// The same world as `world()` with other guns (the text as it goes into the JSON: newlines written `\n`).
fn world_with(guns: &str) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 50, &format!(r#"{{"ItemTable":{{"text":"{ITEMS}"}}}}"#));
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]},"Inventory":{"size":12}}"#);
    spawn(
        &mut s,
        2,
        &format!(r#"{{"Parent":{{"target":1}},"Transform":{{"pos":[0,1.2,0]}},"LocalTransform":{{"pos":[0,1.2,0]}},"Weapon":{{"active":false}},"Loadout":{{"guns":"{guns}"}}}}"#),
    );
    s
}

fn aim(s: &mut Sim, on: bool) {
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<Loadout>(e).unwrap().aiming = on;
}

#[test]
fn aiming_down_the_sights_narrows_the_scatter_and_the_kick_by_the_guns_own_numbers_and_back() {
    use crate::loadout::parse_guns;
    let (g, p) = parse_guns("gun pistol spread=2 bloom=1 recoil=1 recoil_side=0.5 aim=0.25 aim_recoil=0.5\ngun rifle\n");
    assert!(p.is_empty(), "{p:?}");
    assert_eq!((g[0].aim, g[0].aim_recoil), (0.25, 0.5));
    assert_eq!((g[1].aim, g[1].aim_recoil), (1.0, 1.0), "a gun that says nothing is not changed by aiming");
    let mut s = world_with("gun pistol ammo=ammo9 spread=2 bloom=1 recoil=1 recoil_side=0.5 aim=0.25 aim_recoil=0.5 mag=10\\ngun rifle ammo=ammo556 spread=0.5 recoil=0.6 recoil_side=0.2 mag=30\\n");
    bag(&mut s, &[(0, PISTOL, 1), (1, AMMO9, 30), (2, RIFLE, 1), (3, AMMO556, 60)]);
    held(&mut s, 0);
    s.step();
    let w = weapon(&s);
    assert_eq!((w.spread, w.bloom_per_shot, w.recoil), (2.0, 1.0, [1.0, 0.5]), "the gun as it is");
    aim(&mut s, true);
    s.step();
    let w = weapon(&s);
    assert_eq!((w.spread, w.bloom_per_shot, w.recoil), (0.5, 0.25, [0.5, 0.25]), "aiming: a quarter of the scatter, half the kick");
    s.step_n(5);
    assert_eq!(weapon(&s).spread, 0.5, "and it stays");
    aim(&mut s, false);
    s.step();
    let w = weapon(&s);
    assert_eq!((w.spread, w.bloom_per_shot, w.recoil), (2.0, 1.0, [1.0, 0.5]), "the button released: the gun's own numbers again, exactly");
    // the aim button held while the gun is changed: the new gun starts aimed
    aim(&mut s, true);
    held(&mut s, 2);
    s.step();
    let w = weapon(&s);
    assert_eq!((w.spread, w.recoil), (0.5, [0.6, 0.2]), "a gun with no `aim` is not changed by aiming: the rifle's own 0.5 degrees and kick");
    held(&mut s, 0);
    s.step();
    assert_eq!(weapon(&s).spread, 0.5, "back to the pistol, still aiming: 2 x 0.25");
}

#[test]
fn a_gun_that_is_aimed_scatters_its_shots_less() {
    // 20 shots at a wall 10 m away from a gun of 8 degrees of scatter: the spread of the places they land is smaller when aimed (8 x 0.1)
    let spread_of = |aiming: bool| {
        let mut s = world_with("gun pistol ammo=ammo9 damage=1 rate=600 mag=30 auto=1 spread=8 bloom=0 recoil=0 recoil_side=0 aim=0.1\\n");
        spawn(&mut s, 3, r#"{"Transform":{"pos":[0,1.2,-10]},"Collider":{"shape":1,"size":[40,40,0.2]}}"#);
        bag(&mut s, &[(0, PISTOL, 1), (1, AMMO9, 60)]);
        held(&mut s, 0);
        aim(&mut s, aiming);
        s.step();
        trigger(&mut s, true);
        let mut xs: Vec<f32> = Vec::new();
        for _ in 0..150 {
            s.step();
            let w = weapon(&s);
            if w.fired && w.hit_dist > 0.0 {
                xs.push(w.hit_point[0]);
            }
        }
        assert!(xs.len() >= 8, "enough shots landed: {}", xs.len());
        xs.iter().map(|x| x.abs()).fold(0.0f32, f32::max)
    };
    let (loose, tight) = (spread_of(false), spread_of(true));
    assert!(loose > 0.5, "8 degrees at 10 m scatters over more than half a metre: {loose}");
    assert!(tight < loose * 0.3, "aimed, the same gun lands within a tenth of that: {tight} against {loose}");
}
