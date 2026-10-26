//! Base depth (docs/DEPTH.md): the third tier (a stone wall that becomes a metal one), repairing a piece with a hammer for materials, and the cupboard's raid alarm. No art: boxes.
use super::*;
use crate::blast::Resist;
use crate::building::{parse_rules, Upgrader};
use crate::components::{BuildPiece, BuildUpkeep, HitPoints, NetEntity, Team, Transform};
use crate::depth::{RaidAlarm, Repairer, Striker};
use crate::inventory::Inventory;
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn step(s: &mut Sim, n: usize) {
    for _ in 0..n {
        s.step();
    }
}

fn events(s: &Sim, name: &str) -> Vec<(u64, Option<[f32; 3]>)> {
    s.events.iter().filter(|e| e.name == name).map(|e| (e.uid, e.pos)).collect()
}

/// The numbers of the pieces, from 1: foundation 1, wall 2, wall_stone 3, wall_metal 4. Only the wall has a decay (so a test can tell decay from a hit).
const RULES: &str = "rules snap=1.5 min=0.2 gap=0.05 slope=0.7 step=90 ground=0.6 audit=1\\n\
piece foundation size=4,0.5,4 hp=500 free=1 anchor=1 bullet=0.05 repair_cost=scrap:8,tape:2\\n\
  socket n at=0,0.25,-2 yaw=0   type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
  socket s at=0,0.25,2  yaw=180 type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
  socket e at=2,0.25,0  yaw=-90 type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
  socket w at=-2,0.25,0 yaw=90  type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
piece wall size=4,3,0.2 hp=300 inset=0.12 bullet=0.05 upgrade=wall_stone upgrade_cost=scrap:25,tape:4 repair_cost=scrap:12,tape:2\\n\
  socket base at=0,-1.5,0 yaw=0 type=wall_base mates=edge carry=0.9 turn=0\\n\
piece wall_stone size=4,3,0.2 hp=900 inset=0.12 bullet=0.02 melee=0.25 upgrade=wall_metal upgrade_cost=scrap:40,tape:8 repair_cost=scrap:20,tape:4\\n\
  socket base at=0,-1.5,0 yaw=0 type=wall_base mates=edge carry=0.9 turn=0\\n\
piece wall_metal size=4,3,0.2 hp=2400 inset=0.12 bullet=0.01 melee=0.1 repair_cost=scrap:30,tape:6\\n\
  socket base at=0,-1.5,0 yaw=0 type=wall_base mates=edge carry=0.9 turn=0\\n";

const ITEMS: &str = "item scrap | Scrap | stack=50\\nitem tape | Tape | stack=10\\nitem hammer | Hammer | stack=1\\n";

/// A ground, the rules, the items, a foundation (uid 20) and a wall on its north edge (uid 21, owner 1: the number of the player of slot 0), and a player (uid 1: 200 scrap, 30 tape, a hammer, an `Upgrader` and a `Repairer`, slot 0) whose head (uid 2) looks down -z at the wall's south face from 3.5 m.
fn world() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 900, &format!(r#"{{"BuildRules":{{"text":"{RULES}"}}}}"#));
    spawn(&mut s, 901, &format!(r#"{{"ItemTable":{{"text":"{ITEMS}"}}}}"#));
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[80,1,80]}}"#);
    spawn(&mut s, 20, r#"{"Transform":{"pos":[0,0.25,0]},"Collider":{"shape":1,"size":[4,0.5,4]},"BuildPiece":{"kind":"foundation","anchored":true,"checked":true,"owner":1},"HitPoints":{"value":500,"max":500,"last":500},"Resist":{"bullet":0.05},"NetEntity":{"class":3,"kind":1}}"#);
    spawn(&mut s, 21, r#"{"Transform":{"pos":[0,2.0,-2]},"Collider":{"shape":1,"size":[4,3,0.2]},"BuildPiece":{"kind":"wall","anchored":false,"checked":true,"owner":1},"HitPoints":{"value":300,"max":300,"last":300},"Resist":{"bullet":0.05},"NetEntity":{"class":3,"kind":2}}"#);
    spawn(&mut s, 1, &player_json(0, 200, 30, 1));
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,2.1,1.5]}}"#);
    step(&mut s, 3);
    s
}

fn player_json(slot: u32, scrap: u32, tape: u32, hammer: u32) -> String {
    let mut id = vec!["0".to_string(); 48];
    let mut count = vec!["0".to_string(); 48];
    (id[0], count[0]) = ("1".into(), scrap.to_string());
    (id[1], count[1]) = ("2".into(), tape.to_string());
    (id[2], count[2]) = ("3".into(), hammer.to_string());
    format!(r#"{{"Transform":{{"pos":[0,0.5,1.5]}},"HitPoints":{{"value":100,"max":100}},"Inventory":{{"size":12,"id":[{}],"count":[{}]}},"NetAvatar":{{"slot":{slot},"head":2}},"Upgrader":{{}},"Repairer":{{"item":"hammer"}},"Striker":{{"item":"hammer","damage":34,"reach":5}}}}"#, id.join(","), count.join(","))
}

fn upgrade(s: &mut Sim) {
    s.events.clear();
    s.world.get_mut::<Upgrader>(s.world.entity(1).unwrap()).unwrap().want = true;
    s.step();
}

fn repair(s: &mut Sim) {
    s.events.clear();
    s.world.get_mut::<Repairer>(s.world.entity(1).unwrap()).unwrap().want = true;
    s.step();
}

fn strike(s: &mut Sim) {
    s.events.clear();
    s.world.get_mut::<Striker>(s.world.entity(1).unwrap()).unwrap().want = true;
    s.step();
}

fn strike_denied(s: &Sim) -> String {
    let r = s.world.get::<Striker>(s.world.entity(1).unwrap()).unwrap();
    s.world.strings().get(r.denied.0).to_string()
}

fn kind_of(s: &Sim, uid: u64) -> String {
    let p = s.world.get::<BuildPiece>(s.world.entity(uid).unwrap()).unwrap();
    s.world.strings().get(p.kind.0).to_string()
}

fn hp(s: &Sim, uid: u64) -> (f32, f32) {
    let h = s.world.get::<HitPoints>(s.world.entity(uid).unwrap()).unwrap();
    (h.value, h.max)
}

/// (scrap, tape, hammer) in the player's bag.
fn bag(s: &Sim) -> (u32, u32, u32) {
    let inv = s.world.get::<Inventory>(s.world.entity(1).unwrap()).unwrap();
    (inv.count[0], inv.count[1], inv.count[2])
}

fn repair_denied(s: &Sim) -> String {
    let r = s.world.get::<Repairer>(s.world.entity(1).unwrap()).unwrap();
    s.world.strings().get(r.denied.0).to_string()
}

/// Wound the wall to `value` hit points as a blast or a bullet would (the health system sees it and starts the clock of `hurt`), then let the 10 seconds go that a repair waits for.
fn wound(s: &mut Sim, value: f32) {
    s.world.get_mut::<HitPoints>(s.world.entity(21).unwrap()).unwrap().value = value;
    step(s, 1);
}

fn wait_after_hit(s: &mut Sim) {
    step(s, 620);
}

#[test]
fn the_rules_read_the_repair_cost_and_name_its_mistakes() {
    let r = parse_rules(&RULES.replace("\\n", "\n"), None);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let metal = &r.kinds[r.kind("wall_metal").unwrap()];
    assert_eq!((metal.hp, metal.bullet, metal.repair_cost.clone()), (2400.0, 0.01, vec![("scrap".to_string(), 30), ("tape".to_string(), 6)]));
    assert!(r.kinds[r.kind("wall_stone").unwrap()].upgrade == "wall_metal", "the stone tier has a tier above it now");
    let bad = parse_rules("piece a size=1,1,1 free=1 repair_cost=scrap:x,tape\npiece b size=1,1,1 free=1", None);
    assert_eq!(bad.problems.len(), 2, "{:#?}", bad.problems);
    assert!(bad.kinds[1].repair_cost.is_empty(), "a piece that says nothing cannot be repaired");
}

#[test]
fn a_stone_wall_becomes_a_metal_one_and_the_metal_one_has_nothing_above_it() {
    let mut s = world();
    upgrade(&mut s);
    assert_eq!((kind_of(&s, 21), hp(&s, 21), bag(&s).0), ("wall_stone".to_string(), (900.0, 900.0), 175));
    step(&mut s, 40);
    upgrade(&mut s);
    assert_eq!(kind_of(&s, 21), "wall_metal");
    assert_eq!(hp(&s, 21), (2400.0, 2400.0), "the new piece's hit points, full");
    assert_eq!((bag(&s).0, bag(&s).1), (135, 18), "25 + 40 scrap and 4 + 8 tape left the bag in all");
    assert_eq!(s.world.get::<Resist>(s.world.entity(21).unwrap()).unwrap().bullet, 0.01);
    assert_eq!(s.world.get::<NetEntity>(s.world.entity(21).unwrap()).unwrap().kind, 4, "the clients show piece 4: the metal wall");
    assert_eq!(s.world.get::<BuildPiece>(s.world.entity(21).unwrap()).unwrap().owner, 1, "the owner is the same");
    step(&mut s, 130);
    assert!(s.world.entity(21).is_some() && s.world.entity(20).is_some(), "the structure still stands on its foundation");
    step(&mut s, 40);
    upgrade(&mut s);
    assert_eq!(events(&s, "build_upgrade_refused_no_upgrade").len(), 1);
    assert_eq!(bag(&s).0, 135, "nothing was taken");
    // not enough for the metal tier: nothing is taken
    let mut t = world();
    upgrade(&mut t);
    step(&mut t, 40);
    t.world.get_mut::<Inventory>(t.world.entity(1).unwrap()).unwrap().count[1] = 7;
    upgrade(&mut t);
    assert_eq!((kind_of(&t, 21), bag(&t).0, bag(&t).1), ("wall_stone".to_string(), 175, 7));
    assert_eq!(events(&t, "build_upgrade_refused_no_materials").len(), 1);
}

#[test]
fn a_metal_wall_takes_eight_satchels_where_a_stone_wall_takes_three_and_a_wood_wall_one() {
    let charges = |tier: usize| {
        let mut s = world();
        for _ in 0..tier {
            upgrade(&mut s);
            step(&mut s, 40);
        }
        // (the foundation is made to stand whatever happens to it: what is tried is the wall, which would fall with its foundation)
        let h = s.world.get_mut::<HitPoints>(s.world.entity(20).unwrap()).unwrap();
        (h.value, h.max, h.last) = (1.0e6, 1.0e6, 1.0e6);
        // the game's satchel (120, 2.8 times that to a piece) 20 cm from the wall's south face, inside the core: 336 a charge
        let mut used = 0;
        while s.world.entity(21).is_some() && used < 12 {
            spawn(&mut s, 700 + used, r#"{"Transform":{"pos":[0,2.0,-1.7]},"Explosive":{"fuse":0,"radius":5,"damage":120,"core":0.6,"structure_mult":2.8,"cover":1}}"#);
            step(&mut s, 4);
            used += 1;
        }
        used
    };
    assert_eq!(charges(0), 1, "300 hit points: one charge");
    assert_eq!(charges(1), 3, "900: three");
    assert_eq!(charges(2), 8, "2400: eight (7 charges do 2352)");
}

#[test]
fn a_repair_costs_a_share_of_the_cost_rounded_up_and_stops_at_the_full_hit_points() {
    let mut s = world();
    wound(&mut s, 40.0);
    wait_after_hit(&mut s);
    // a quarter of 300 is 75: a quarter of 12 scrap (3) and of 2 tape (0.5: one)
    repair(&mut s);
    assert_eq!(repair_denied(&s), "");
    assert_eq!(hp(&s, 21), (115.0, 300.0));
    assert_eq!(bag(&s), (197, 29, 1), "3 scrap and 1 tape; the hammer is kept");
    assert_eq!(events(&s, "build_repaired").iter().map(|e| e.0).collect::<Vec<_>>(), vec![21]);
    assert_eq!(events(&s, "build_repair_ok").iter().map(|e| e.0).collect::<Vec<_>>(), vec![2], "the player's head is told");
    assert_eq!(s.world.get::<Repairer>(s.world.entity(1).unwrap()).unwrap().repaired, 1);
    // a request at once is not heard (the pause), the next one after it is
    repair(&mut s);
    assert_eq!(hp(&s, 21).0, 115.0);
    step(&mut s, 60);
    repair(&mut s);
    assert_eq!(hp(&s, 21).0, 190.0);
    step(&mut s, 60);
    repair(&mut s);
    assert_eq!(hp(&s, 21).0, 265.0);
    assert_eq!(bag(&s), (191, 27, 1));
    // the last bit is only what is missing: 35 hit points, a share of 0.1167 of the cost: 2 scrap (1.4 rounded up) and 1 tape
    step(&mut s, 60);
    repair(&mut s);
    assert_eq!(hp(&s, 21), (300.0, 300.0), "it does not go over the full hit points");
    assert_eq!(bag(&s), (189, 26, 1));
    step(&mut s, 60);
    repair(&mut s);
    assert_eq!(repair_denied(&s), "full");
    assert_eq!(bag(&s), (189, 26, 1), "nothing is taken for a piece that is whole");
    assert_eq!(events(&s, "build_repair_refused_full").len(), 1);
}

#[test]
fn a_repair_is_refused_for_every_reason_and_costs_nothing_then() {
    let mut s = world();
    wound(&mut s, 100.0);
    // not at once after the hit (nine seconds are not ten), then yes
    repair(&mut s);
    assert_eq!((repair_denied(&s), hp(&s, 21).0, bag(&s)), ("recently_hit".to_string(), 100.0, (200, 30, 1)));
    step(&mut s, 540);
    repair(&mut s);
    assert_eq!(repair_denied(&s), "recently_hit", "9 seconds");
    step(&mut s, 80);
    repair(&mut s);
    assert_eq!(repair_denied(&s), "");
    assert_eq!(hp(&s, 21).0, 175.0);
    // a hit stops it again: the clock starts from the new hit
    step(&mut s, 60);
    wound(&mut s, 50.0);
    repair(&mut s);
    assert_eq!((repair_denied(&s), hp(&s, 21).0), ("recently_hit".to_string(), 50.0));
    wait_after_hit(&mut s);
    // not the player's piece: another owner's (number 5)
    s.world.get_mut::<BuildPiece>(s.world.entity(21).unwrap()).unwrap().owner = 5;
    let before = bag(&s);
    repair(&mut s);
    assert_eq!((repair_denied(&s), hp(&s, 21).0, bag(&s)), ("not_yours".to_string(), 50.0, before));
    assert_eq!(events(&s, "build_repair_refused_not_yours").len(), 1);
    // ... unless that owner is in the player's party (Team 100 and up: docs/PARTIES.md)
    spawn(&mut s, 30, r#"{"Transform":{"pos":[50,0.5,50]},"NetAvatar":{"slot":4},"Team":{"id":100}}"#);
    s.world.insert(s.world.entity(1).unwrap(), Team { id: 100 }).unwrap();
    step(&mut s, 60);
    repair(&mut s);
    assert_eq!(repair_denied(&s), "", "a party mate's piece");
    assert_eq!(hp(&s, 21).0, 125.0);
    // a stranger of another party is not
    s.world.get_mut::<Team>(s.world.entity(30).unwrap()).unwrap().id = 101;
    step(&mut s, 60);
    repair(&mut s);
    assert_eq!(repair_denied(&s), "not_yours");
    s.world.get_mut::<BuildPiece>(s.world.entity(21).unwrap()).unwrap().owner = 1;
    // no tool: the hammer is not in the bag
    step(&mut s, 60);
    s.world.get_mut::<Inventory>(s.world.entity(1).unwrap()).unwrap().count[2] = 0;
    repair(&mut s);
    assert_eq!(repair_denied(&s), "no_tool");
    s.world.get_mut::<Inventory>(s.world.entity(1).unwrap()).unwrap().count[2] = 1;
    // no materials: nothing is taken, not the scrap that is there
    step(&mut s, 60);
    s.world.get_mut::<Inventory>(s.world.entity(1).unwrap()).unwrap().count[1] = 0;
    let (scrap, _, _) = bag(&s);
    repair(&mut s);
    assert_eq!((repair_denied(&s), bag(&s).0, hp(&s, 21).0), ("no_materials".to_string(), scrap, 125.0));
    s.world.get_mut::<Inventory>(s.world.entity(1).unwrap()).unwrap().count[1] = 9;
    // looking away: no piece in reach
    step(&mut s, 60);
    s.world.get_mut::<Transform>(s.world.entity(2).unwrap()).unwrap().rot = [0.0, 1.0, 0.0, 0.0];
    repair(&mut s);
    assert_eq!(repair_denied(&s), "no_aim");
    s.world.get_mut::<Transform>(s.world.entity(2).unwrap()).unwrap().rot = [0.0, 0.0, 0.0, 1.0];
    // a wall of the world in front hides the piece
    step(&mut s, 60);
    spawn(&mut s, 60, r#"{"Transform":{"pos":[0,2.5,0]},"Collider":{"shape":1,"size":[3,3,0.2]}}"#);
    repair(&mut s);
    assert_eq!(repair_denied(&s), "no_aim");
    s.world.despawn(s.world.entity(60).unwrap());
    // a piece whose rules line has no repair_cost cannot be repaired
    step(&mut s, 60);
    let text = RULES.replace("\\n", "\n").replace(" repair_cost=scrap:12,tape:2", "");
    let sym = s.world.strings_mut().intern(&text);
    s.world.get_mut::<crate::components::BuildRules>(s.world.entity(900).unwrap()).unwrap().text = lunaris_ecs::Sym(sym);
    repair(&mut s);
    assert_eq!(repair_denied(&s), "no_repair");
    // a dead player repairs nothing
    step(&mut s, 60);
    s.world.get_mut::<HitPoints>(s.world.entity(1).unwrap()).unwrap().dead = true;
    repair(&mut s);
    assert_eq!(repair_denied(&s), "dead");
}

#[test]
fn the_rules_read_the_melee_share_and_name_a_bad_one() {
    let r = parse_rules(&RULES.replace("\\n", "\n"), None);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let shares: Vec<f32> = ["wall", "wall_stone", "wall_metal"].iter().map(|n| r.kinds[r.kind(n).unwrap()].melee).collect();
    assert_eq!(shares, vec![1.0, 0.25, 0.1]);
    let bad = parse_rules("piece a size=1,1,1 free=1 melee=2\npiece b size=1,1,1 free=1 melee=oops", None);
    assert_eq!(bad.problems.len(), 2, "{:#?}", bad.problems);
}

#[test]
fn a_blow_takes_a_share_of_a_piece_by_its_tier_and_costs_nothing_and_is_not_heard_twice_at_once() {
    let mut s = world();
    // wood: all of a blow (34)
    strike(&mut s);
    assert_eq!(strike_denied(&s), "");
    assert_eq!(hp(&s, 21), (266.0, 300.0));
    assert_eq!(events(&s, "build_struck").iter().map(|e| e.0).collect::<Vec<_>>(), vec![21]);
    assert_eq!(events(&s, "build_damaged").iter().map(|e| e.0).collect::<Vec<_>>(), vec![21], "the alarm's event too");
    assert_eq!(events(&s, "build_strike_ok").iter().map(|e| e.0).collect::<Vec<_>>(), vec![2], "the player's head is told");
    assert_eq!(bag(&s), (200, 30, 1), "a blow costs nothing and the tool is kept");
    assert_eq!(s.world.get::<Striker>(s.world.entity(1).unwrap()).unwrap().struck, 1);
    strike(&mut s);
    assert_eq!(hp(&s, 21).0, 266.0, "the pause: not heard");
    step(&mut s, 60);
    strike(&mut s);
    assert_eq!(hp(&s, 21).0, 232.0);
    // stone takes a quarter of a blow (8.5), metal a tenth (3.4)
    upgrade(&mut s);
    step(&mut s, 60);
    strike(&mut s);
    assert!((hp(&s, 21).0 - 891.5).abs() < 1e-3, "{:?}", hp(&s, 21));
    upgrade(&mut s);
    step(&mut s, 60);
    strike(&mut s);
    assert_eq!(kind_of(&s, 21), "wall_metal");
    assert!((hp(&s, 21).0 - 2396.6).abs() < 1e-2, "{:?}", hp(&s, 21));
}

#[test]
fn a_blow_is_refused_for_every_reason_and_a_piece_taken_to_nothing_is_destroyed() {
    let mut s = world();
    // no tool
    s.world.get_mut::<Inventory>(s.world.entity(1).unwrap()).unwrap().count[2] = 0;
    strike(&mut s);
    assert_eq!((strike_denied(&s), hp(&s, 21).0), ("no_tool".to_string(), 300.0));
    assert_eq!(events(&s, "build_strike_refused_no_tool").len(), 1);
    s.world.get_mut::<Inventory>(s.world.entity(1).unwrap()).unwrap().count[2] = 1;
    // looking away
    step(&mut s, 60);
    s.world.get_mut::<Transform>(s.world.entity(2).unwrap()).unwrap().rot = [0.0, 1.0, 0.0, 0.0];
    strike(&mut s);
    assert_eq!(strike_denied(&s), "no_aim");
    s.world.get_mut::<Transform>(s.world.entity(2).unwrap()).unwrap().rot = [0.0, 0.0, 0.0, 1.0];
    // a piece that hand cannot hurt (`melee=0`)
    step(&mut s, 60);
    let text = RULES.replace("\\n", "\n").replace("piece wall size=4,3,0.2 hp=300", "piece wall size=4,3,0.2 melee=0 hp=300");
    let sym = s.world.strings_mut().intern(&text);
    s.world.get_mut::<crate::components::BuildRules>(s.world.entity(900).unwrap()).unwrap().text = lunaris_ecs::Sym(sym);
    strike(&mut s);
    assert_eq!((strike_denied(&s), hp(&s, 21).0), ("immune".to_string(), 300.0));
    // a dead player strikes nothing
    step(&mut s, 60);
    s.world.get_mut::<HitPoints>(s.world.entity(1).unwrap()).unwrap().dead = true;
    strike(&mut s);
    assert_eq!(strike_denied(&s), "dead");
    // a blow that takes a piece to nothing: it is destroyed (the foundation is made to stand: the wall would fall with it)
    let mut t = world();
    let f = t.world.entity(20).unwrap();
    let h = t.world.get_mut::<HitPoints>(f).unwrap();
    (h.value, h.max, h.last) = (1.0e6, 1.0e6, 1.0e6);
    t.world.get_mut::<HitPoints>(t.world.entity(21).unwrap()).unwrap().value = 20.0;
    step(&mut t, 1);
    strike(&mut t);
    step(&mut t, 4);
    assert!(t.world.entity(21).is_none(), "the wall is gone");
    assert!(events(&t, "build_destroyed").len() <= 1);
}

#[test]
fn a_blow_on_a_wall_rings_the_cupboard_like_a_blast() {
    let mut s = cupboard_world();
    strike(&mut s);
    assert_eq!(alarms(&s), vec![1], "the owner of the cupboard, once");
    let a = *s.world.get::<RaidAlarm>(s.world.entity(70).unwrap()).unwrap();
    assert_eq!((a.rung, a.told), (1, 1));
    // the next blow, a second later: the cupboard is quiet
    step(&mut s, 60);
    strike(&mut s);
    assert!(alarms(&s).is_empty());
}

#[test]
fn decay_is_not_a_hit_but_a_blast_is() {
    let mut s = world();
    // a wall that decays half a hit point a second
    let text = RULES.replace("\\n", "\n").replace("piece wall size=4,3,0.2 hp=300", "piece wall size=4,3,0.2 decay=0.5 hp=300");
    let sym = s.world.strings_mut().intern(&text);
    s.world.get_mut::<crate::components::BuildRules>(s.world.entity(900).unwrap()).unwrap().text = lunaris_ecs::Sym(sym);
    step(&mut s, 400);
    let h = *s.world.get::<HitPoints>(s.world.entity(21).unwrap()).unwrap();
    assert!(h.value < 299.0 && h.value > 290.0, "it decayed a little: {}", h.value);
    assert!(h.hurt > 6.0, "decay does not start the clock of a hit: {}", h.hurt);
    // so a decayed piece can be repaired after ten seconds without a hit, and a repair of such a little costs one of each
    repair(&mut s);
    assert_eq!(repair_denied(&s), "");
    assert_eq!(bag(&s), (199, 29, 1));
    // a blast does start it
    step(&mut s, 60);
    spawn(&mut s, 700, r#"{"Transform":{"pos":[0,2.0,-1.7]},"Explosive":{"fuse":0,"radius":5,"damage":10,"core":0.6,"structure_mult":1,"cover":1}}"#);
    step(&mut s, 3);
    assert!(s.world.get::<HitPoints>(s.world.entity(21).unwrap()).unwrap().hurt < 0.2);
    step(&mut s, 60);
    repair(&mut s);
    assert_eq!(repair_denied(&s), "recently_hit");
}

/// The cupboard (uid 70) at the base, owner number 1 (the player of slot 0, uid 1), a stranger (uid 31, slot 5, number 6), and a satchel's worth on the wall.
fn cupboard_world() -> Sim {
    let mut s = world();
    spawn(&mut s, 70, r#"{"Transform":{"pos":[3,0.6,3]},"BuildUpkeep":{"owner":1,"radius":15},"RaidAlarm":{"throttle":30}}"#);
    spawn(&mut s, 31, r#"{"Transform":{"pos":[10,0.5,10]},"NetAvatar":{"slot":5}}"#);
    // (the foundation and the wall are made to stand: what is tried is the alarm)
    for u in [20, 21] {
        let h = s.world.get_mut::<HitPoints>(s.world.entity(u).unwrap()).unwrap();
        (h.value, h.max, h.last) = (1.0e6, 1.0e6, 1.0e6);
    }
    step(&mut s, 2);
    s
}

fn blast(s: &mut Sim, uid: u64, at: &str) {
    s.events.clear();
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":{at}}},"Explosive":{{"fuse":0,"radius":5,"damage":120,"core":0.6,"structure_mult":2.8,"cover":1}}}}"#));
    step(s, 4);
}

fn alarms(s: &Sim) -> Vec<u64> {
    let mut v: Vec<u64> = events(s, "raid_alarm").iter().map(|e| e.0).collect();
    v.sort();
    v
}

#[test]
fn a_blast_on_a_piece_in_range_rings_the_cupboard_for_its_owner_and_nobody_else() {
    let mut s = cupboard_world();
    blast(&mut s, 700, "[0,2.0,-1.7]");
    // the wall and the foundation were both hurt (two build_damaged events), the owner is told once, at the place of the damage, the stranger not at all
    assert!(events(&s, "build_damaged").len() >= 2, "{:?}", events(&s, "build_damaged"));
    assert_eq!(alarms(&s), vec![1], "only the owner's avatar");
    let at = events(&s, "raid_alarm")[0].1.unwrap();
    assert!((at[2] + 1.9).abs() < 0.5 && at[0].abs() < 1.0, "the place is where the wall was hurt, not where the cupboard is: {at:?}");
    let a = *s.world.get::<RaidAlarm>(s.world.entity(70).unwrap()).unwrap();
    assert_eq!((a.rung, a.told), (1, 1));
    // a second blast 2 seconds later: the cupboard is quiet (30 s between two alarms)
    step(&mut s, 110);
    blast(&mut s, 701, "[0,2.0,-1.7]");
    assert!(alarms(&s).is_empty(), "throttled");
    assert_eq!(s.world.get::<RaidAlarm>(s.world.entity(70).unwrap()).unwrap().rung, 1);
    // after the 30 seconds it rings again
    step(&mut s, 1800);
    blast(&mut s, 702, "[0,2.0,-1.7]");
    assert_eq!(alarms(&s), vec![1]);
    assert_eq!(s.world.get::<RaidAlarm>(s.world.entity(70).unwrap()).unwrap().rung, 2);
}

#[test]
fn only_a_piece_in_range_and_only_a_blast_rings_it_and_a_party_is_told_too() {
    // a cupboard 100 m away does not hear
    let mut s = cupboard_world();
    s.world.get_mut::<Transform>(s.world.entity(70).unwrap()).unwrap().pos = [100.0, 0.6, 100.0];
    blast(&mut s, 700, "[0,2.0,-1.7]");
    assert!(alarms(&s).is_empty(), "out of the cupboard's range");
    // a blast that hurts nothing of a base (in the open, far from every piece) rings nothing
    let mut s = cupboard_world();
    blast(&mut s, 700, "[30,2.0,30]");
    assert!(alarms(&s).is_empty() && events(&s, "build_damaged").is_empty());
    // a bullet or decay is not a blast: no alarm (the alarm answers `build_damaged`, which the blast tells)
    let mut s = cupboard_world();
    s.world.get_mut::<HitPoints>(s.world.entity(21).unwrap()).unwrap().value = 5.0e5;
    step(&mut s, 5);
    assert!(alarms(&s).is_empty());
    // a party mate of the owner (both in party 100) is told too, a stranger of party 101 is not
    let mut s = cupboard_world();
    s.world.insert(s.world.entity(1).unwrap(), Team { id: 100 }).unwrap();
    spawn(&mut s, 32, r#"{"Transform":{"pos":[40,0.5,40]},"NetAvatar":{"slot":6},"Team":{"id":100}}"#);
    s.world.insert(s.world.entity(31).unwrap(), Team { id: 101 }).unwrap();
    blast(&mut s, 700, "[0,2.0,-1.7]");
    assert_eq!(alarms(&s), vec![1, 32]);
    assert_eq!(s.world.get::<RaidAlarm>(s.world.entity(70).unwrap()).unwrap().told, 2);
}

#[test]
fn a_cupboard_that_is_a_piece_belongs_to_its_builder() {
    let mut s = cupboard_world();
    // the cupboard as the build tool makes it: a piece (a kind the rules do not need to know for this) with its builder's number, and a `BuildUpkeep` with no owner yet
    spawn(&mut s, 71, r#"{"Transform":{"pos":[-3,0.6,3]},"BuildPiece":{"kind":"cupboard","owner":1,"checked":true,"anchored":true},"BuildUpkeep":{"owner":0,"radius":15},"RaidAlarm":{}}"#);
    s.world.despawn(s.world.entity(70).unwrap());
    step(&mut s, 2);
    assert_eq!(s.world.get::<BuildUpkeep>(s.world.entity(71).unwrap()).unwrap().owner, 1, "the land is its builder's");
    blast(&mut s, 700, "[0,2.0,-1.7]");
    assert_eq!(alarms(&s), vec![1]);
}

#[test]
fn what_depth_keeps_survives_a_save_and_a_load() {
    let mut s = cupboard_world();
    upgrade(&mut s);
    step(&mut s, 40);
    upgrade(&mut s);
    blast(&mut s, 700, "[0,2.0,-1.7]");
    let save = s.save_json();
    let mut t = Sim::new(1);
    t.load_save(&save).unwrap();
    assert_eq!(kind_of(&t, 21), "wall_metal", "the metal tier is in the save");
    let a = *t.world.get::<RaidAlarm>(t.world.entity(70).unwrap()).unwrap();
    assert_eq!((a.rung, a.throttle), (1, 30.0));
    let r = *t.world.get::<Repairer>(t.world.entity(1).unwrap()).unwrap();
    assert_eq!(t.world.strings().get(r.item.0), "hammer");
}
