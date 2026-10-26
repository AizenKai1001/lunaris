//! Tiers and what shrugs off bullets (docs/RAIDING.md): `bullet=`, `upgrade=` and `upgrade_cost=` in the building rules, a piece that becomes the next tier for the materials in the bag, a `Resist` that takes a share off a bullet's damage, and a stone wall that it takes
//! three charges to break where a wood one takes one. No art: boxes.
use super::*;
use crate::blast::Resist;
use crate::building::{parse_rules, Upgrader};
use crate::components::{BuildPiece, BuildUpkeep, HitPoints, NetEntity, Transform, Weapon};
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

/// The numbers of the pieces, from 1: foundation 1, wall 2, foundation_stone 3, wall_stone 4.
const RULES: &str = "rules snap=1.5 min=0.2 gap=0.05 slope=0.7 step=90 ground=0.6 audit=1\\n\
piece foundation size=4,0.5,4 hp=500 free=1 anchor=1 bullet=0.05 upgrade=foundation_stone upgrade_cost=scrap:40,tape:6\\n\
  socket n at=0,0.25,-2 yaw=0   type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
  socket s at=0,0.25,2  yaw=180 type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
  socket e at=2,0.25,0  yaw=-90 type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
  socket w at=-2,0.25,0 yaw=90  type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
piece wall size=4,3,0.2 hp=300 inset=0.12 bullet=0.05 upgrade=wall_stone upgrade_cost=scrap:25,tape:4\\n\
  socket base at=0,-1.5,0 yaw=0 type=wall_base mates=edge carry=0.9 turn=0\\n\
piece foundation_stone size=4,0.5,4 hp=1500 free=1 anchor=1 bullet=0.03\\n\
  socket n at=0,0.25,-2 yaw=0   type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
  socket s at=0,0.25,2  yaw=180 type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
  socket e at=2,0.25,0  yaw=-90 type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
  socket w at=-2,0.25,0 yaw=90  type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
piece wall_stone size=4,3,0.2 hp=1000 inset=0.12 bullet=0.03\\n\
  socket base at=0,-1.5,0 yaw=0 type=wall_base mates=edge carry=0.9 turn=0\\n";

const ITEMS: &str = "item scrap | Scrap | stack=50\\nitem tape | Tape | stack=10\\n";

/// A ground, the rules, the items, a foundation (uid 20) and a wall on its north edge (uid 21, owner 7), and a player (uid 1: 30 scrap and 5 tape, an `Upgrader`, slot 0) whose head (uid 2) looks down -z at the wall's south face from 3.5 m.
fn world() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 900, &format!(r#"{{"BuildRules":{{"text":"{RULES}"}}}}"#));
    spawn(&mut s, 901, &format!(r#"{{"ItemTable":{{"text":"{ITEMS}"}}}}"#));
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[80,1,80]}}"#);
    spawn(&mut s, 20, r#"{"Transform":{"pos":[0,0.25,0]},"Collider":{"shape":1,"size":[4,0.5,4]},"BuildPiece":{"kind":"foundation","anchored":true,"checked":true,"owner":7},"HitPoints":{"value":500,"max":500,"last":500},"Resist":{"bullet":0.05},"NetEntity":{"class":3,"kind":1}}"#);
    spawn(&mut s, 21, r#"{"Transform":{"pos":[0,2.0,-2]},"Collider":{"shape":1,"size":[4,3,0.2]},"BuildPiece":{"kind":"wall","anchored":false,"checked":true,"owner":7},"HitPoints":{"value":300,"max":300,"last":300},"Resist":{"bullet":0.05},"NetEntity":{"class":3,"kind":2}}"#);
    let mut id = vec!["0".to_string(); 48];
    let mut count = vec!["0".to_string(); 48];
    (id[0], count[0]) = ("1".into(), "30".into());
    (id[1], count[1]) = ("2".into(), "5".into());
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,0.5,1.5]}},"HitPoints":{{"value":100,"max":100}},"Inventory":{{"size":12,"id":[{}],"count":[{}]}},"NetAvatar":{{"slot":0,"head":2}},"Upgrader":{{}}}}"#, id.join(","), count.join(",")));
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,2.1,1.5]}}"#);
    step(&mut s, 3);
    s
}

fn ask(s: &mut Sim) {
    s.events.clear();
    s.world.get_mut::<Upgrader>(s.world.entity(1).unwrap()).unwrap().want = true;
    s.step();
}

fn kind_of(s: &Sim, uid: u64) -> String {
    let p = s.world.get::<BuildPiece>(s.world.entity(uid).unwrap()).unwrap();
    s.world.strings().get(p.kind.0).to_string()
}

fn hp(s: &Sim, uid: u64) -> (f32, f32) {
    let h = s.world.get::<HitPoints>(s.world.entity(uid).unwrap()).unwrap();
    (h.value, h.max)
}

fn bag(s: &Sim) -> (u32, u32) {
    let inv = s.world.get::<Inventory>(s.world.entity(1).unwrap()).unwrap();
    (inv.count[0], inv.count[1])
}

fn denied(s: &Sim) -> String {
    let u = s.world.get::<Upgrader>(s.world.entity(1).unwrap()).unwrap();
    s.world.strings().get(u.denied.0).to_string()
}

#[test]
fn the_rules_read_bullet_upgrade_and_cost_and_name_their_mistakes() {
    let text = RULES.replace("\\n", "\n");
    let r = parse_rules(&text, None);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let (wall, stone) = (&r.kinds[r.kind("wall").unwrap()], &r.kinds[r.kind("wall_stone").unwrap()]);
    assert_eq!((wall.bullet, wall.upgrade.as_str(), wall.upgrade_cost.clone()), (0.05, "wall_stone", vec![("scrap".to_string(), 25), ("tape".to_string(), 4)]));
    assert_eq!((stone.bullet, stone.upgrade.as_str(), stone.hp), (0.03, "", 1000.0));
    // what is wrong: a share out of range, a cost that is not ITEM:COUNT, an upgrade to a piece that does not exist, a cost with no upgrade
    let bad = parse_rules("piece a size=1,1,1 free=1 bullet=2 upgrade=nothing upgrade_cost=scrap:x,tape\npiece b size=1,1,1 free=1 upgrade_cost=scrap:5\npiece c size=1,1,1 free=1 bullet=oops", None);
    assert_eq!(bad.problems.len(), 6, "{:#?}", bad.problems);
    // a piece that is not told of any of it is as it was
    let plain = parse_rules("piece a size=1,1,1 free=1", None);
    assert_eq!((plain.kinds[0].bullet, plain.kinds[0].upgrade.as_str(), plain.kinds[0].upgrade_cost.len()), (1.0, "", 0));
}

#[test]
fn a_piece_becomes_the_next_tier_for_the_materials_in_the_bag() {
    let mut s = world();
    ask(&mut s);
    assert_eq!(denied(&s), "");
    assert_eq!(kind_of(&s, 21), "wall_stone", "the wall is a stone wall now");
    assert_eq!(hp(&s, 21), (1000.0, 1000.0), "with the new piece's hit points, full");
    assert_eq!(bag(&s), (5, 1), "25 scrap and 4 tape left the bag");
    assert_eq!(s.world.get::<Resist>(s.world.entity(21).unwrap()).unwrap().bullet, 0.03, "and what the new piece shrugs off");
    assert_eq!(s.world.get::<NetEntity>(s.world.entity(21).unwrap()).unwrap().kind, 4, "the clients are told it is piece 4: they show the stone wall");
    assert_eq!(s.world.get::<BuildPiece>(s.world.entity(21).unwrap()).unwrap().owner, 7, "the owner is the same");
    assert_eq!(events(&s, "build_upgraded").iter().map(|e| e.0).collect::<Vec<_>>(), vec![21]);
    assert_eq!(events(&s, "build_upgrade_ok").iter().map(|e| e.0).collect::<Vec<_>>(), vec![2], "and the player's head is told");
    assert_eq!(s.world.get::<Upgrader>(s.world.entity(1).unwrap()).unwrap().upgraded, 1);
    // the structure still stands (same size, same sockets): the audit sees a new kind and looks again
    step(&mut s, 130);
    assert!(s.world.entity(21).is_some() && s.world.entity(20).is_some());
    let st = s.world.get::<BuildPiece>(s.world.entity(21).unwrap()).unwrap().stability;
    assert!((st - 0.9).abs() < 1e-4, "held by the foundation as before: {st}");
    // the stone wall has no tier above it
    step(&mut s, 40);
    ask(&mut s);
    assert_eq!(denied(&s), "no_upgrade");
    assert_eq!(events(&s, "build_upgrade_refused_no_upgrade").len(), 1);
    assert_eq!(bag(&s), (5, 1));
}

#[test]
fn an_upgrade_is_refused_with_its_reason_and_costs_nothing_then() {
    let mut s = world();
    // not enough tape: nothing is taken (not the scrap either)
    s.world.get_mut::<Inventory>(s.world.entity(1).unwrap()).unwrap().count[1] = 3;
    ask(&mut s);
    assert_eq!((denied(&s), kind_of(&s, 21), bag(&s)), ("no_materials".to_string(), "wall".to_string(), (30, 3)));
    assert_eq!(events(&s, "build_upgrade_refused_no_materials").iter().map(|e| e.0).collect::<Vec<_>>(), vec![2]);
    // a request is not heard again at once
    s.world.get_mut::<Inventory>(s.world.entity(1).unwrap()).unwrap().count[1] = 5;
    ask(&mut s);
    assert_eq!(kind_of(&s, 21), "wall", "0.5 s has not passed");
    step(&mut s, 40);
    // looking away: no piece in reach
    s.world.get_mut::<Transform>(s.world.entity(2).unwrap()).unwrap().rot = [0.0, 1.0, 0.0, 0.0];
    ask(&mut s);
    assert_eq!(denied(&s), "no_aim");
    s.world.get_mut::<Transform>(s.world.entity(2).unwrap()).unwrap().rot = [0.0, 0.0, 0.0, 1.0];
    step(&mut s, 40);
    // a wall of the world in front hides the piece
    spawn(&mut s, 60, r#"{"Transform":{"pos":[0,2.5,0]},"Collider":{"shape":1,"size":[3,3,0.2]}}"#);
    ask(&mut s);
    assert_eq!(denied(&s), "no_aim");
    s.world.despawn(s.world.entity(60).unwrap());
    step(&mut s, 40);
    // on land that is another owner's (a cupboard of owner 9, which the player of slot 0, number 1, is not)
    spawn(&mut s, 61, r#"{"Transform":{"pos":[0,0,0]},"BuildUpkeep":{"owner":9,"radius":20,"exclusive":true}}"#);
    ask(&mut s);
    assert_eq!((denied(&s), kind_of(&s, 21)), ("not_yours".to_string(), "wall".to_string()));
    step(&mut s, 40);
    // the cupboard of the player's own number does not stop it
    s.world.get_mut::<BuildUpkeep>(s.world.entity(61).unwrap()).unwrap().owner = 1;
    ask(&mut s);
    assert_eq!(kind_of(&s, 21), "wall_stone");
    // a dead player upgrades nothing
    step(&mut s, 40);
    s.world.get_mut::<HitPoints>(s.world.entity(1).unwrap()).unwrap().dead = true;
    s.world.get_mut::<Inventory>(s.world.entity(1).unwrap()).unwrap().count[0] = 99;
    ask(&mut s);
    assert_eq!(denied(&s), "dead");
}

#[test]
fn a_stone_wall_takes_three_charges_where_a_wood_wall_takes_one() {
    let charge = |kind_up: bool| {
        let mut s = world();
        if kind_up {
            ask(&mut s);
            step(&mut s, 40);
        }
        // (the foundation is made to stand whatever happens to it: what is tried is the wall, which would fall with its foundation)
        let f = s.world.entity(20).unwrap();
        let h = s.world.get_mut::<HitPoints>(f).unwrap();
        (h.value, h.max, h.last) = (1.0e6, 1.0e6, 1.0e6);
        // a satchel's worth 20 cm from the wall's south face, inside the core: 120 x 3 = 360
        let boom = |s: &mut Sim, uid: u64| spawn(s, uid, r#"{"Transform":{"pos":[0,2.0,-1.7]},"Explosive":{"fuse":0,"radius":5,"damage":120,"core":0.6,"structure_mult":3,"cover":1}}"#);
        let mut used = 0;
        while s.world.entity(21).is_some() && used < 6 {
            boom(&mut s, 700 + used);
            step(&mut s, 4);
            used += 1;
        }
        used
    };
    assert_eq!(charge(false), 1, "300 hit points, 360 a charge");
    assert_eq!(charge(true), 3, "1000 hit points, 360 a charge: three");
}

#[test]
fn a_bullet_does_a_share_of_its_damage_to_what_resists_it() {
    let mut s = world();
    // a gun 6 m south of the wall's south face, looking down -z, 25 a round
    spawn(&mut s, 70, r#"{"Transform":{"pos":[0,2.0,4.5]},"Weapon":{"damage":25,"rate":600,"spread":0,"bloom_per_shot":0,"recoil":[0,0],"mag_size":30,"mag":30,"infinite":true,"fire":true}}"#);
    step(&mut s, 2);
    s.world.get_mut::<Weapon>(s.world.entity(70).unwrap()).unwrap().fire = false;
    let (value, _) = hp(&s, 21);
    assert!(value < 300.0 && (300.0 - value - 25.0 * 0.05).abs() < 1e-3, "a twentieth of 25: {value}");
    // the same round on a thing that shrugs off nothing does all of it
    spawn(&mut s, 71, r#"{"Transform":{"pos":[6,1.0,-3]},"Collider":{"shape":1,"size":[1,2,1]},"HitPoints":{"value":100,"max":100,"last":100}}"#);
    spawn(&mut s, 72, r#"{"Transform":{"pos":[6,1.0,4.5]},"Weapon":{"damage":25,"rate":600,"spread":0,"bloom_per_shot":0,"recoil":[0,0],"mag_size":30,"mag":30,"infinite":true,"fire":true}}"#);
    step(&mut s, 2);
    assert!((hp(&s, 71).0 - 75.0).abs() < 1e-3, "{}", hp(&s, 71).0);
}

#[test]
fn a_piece_built_from_rules_that_shrug_off_bullets_does() {
    let mut s = world();
    // a free foundation put on open ground by a tool (owner 7), looking at the ground from above
    spawn(&mut s, 80, r#"{"Transform":{"pos":[20,5,20]},"BuildTool":{"reach":12,"owner":7}}"#);
    let q = crate::perception::look_rot([0.0, -5.0, 0.01], true);
    s.world.get_mut::<Transform>(s.world.entity(80).unwrap()).unwrap().rot = q;
    let sym = s.world.strings_mut().intern("foundation_stone");
    s.world.get_mut::<crate::components::BuildTool>(s.world.entity(80).unwrap()).unwrap().kind = lunaris_ecs::Sym(sym);
    s.step();
    s.world.get_mut::<crate::components::BuildTool>(s.world.entity(80).unwrap()).unwrap().place = true;
    s.step();
    let placed = s.world.get::<crate::components::BuildTool>(s.world.entity(80).unwrap()).unwrap().placed.0;
    assert!(placed != 0, "the foundation was built");
    let e = s.world.entity(placed).unwrap();
    assert_eq!(s.world.get::<Resist>(e).map(|r| r.bullet), Some(0.03), "it shrugs off what its rules say");
    assert_eq!(s.world.get::<HitPoints>(e).unwrap().max, 1500.0);
}

/// What the tiers cost a tick when nobody asks, and what one upgrade costs (run by hand: `cargo test -p lunaris-sim --lib tier_costs --release -- --ignored --nocapture`).
#[test]
#[ignore]
fn tier_costs_what_the_docs_say() {
    let mut s = world();
    s.profile = Some(Default::default());
    step(&mut s, 600);
    let p = s.profile.take().unwrap();
    let st = p.stages.get("gameplay building::update").unwrap();
    let base = st.total_ms / st.calls as f64;
    println!("two pieces and one upgrader: building::update {:.4} ms a tick on average, {:.4} ms at most", base, st.max_ms);
    for k in 0..200u64 {
        spawn(&mut s, 5000 + k, &format!(r#"{{"Transform":{{"pos":[{},0,{}]}},"Upgrader":{{}}}}"#, (k % 20) as f32 * 3.0 + 100.0, (k / 20) as f32 * 3.0 + 100.0));
    }
    s.profile = Some(Default::default());
    step(&mut s, 600);
    let p = s.profile.take().unwrap();
    let st = p.stages.get("gameplay building::update").unwrap();
    let now = st.total_ms / st.calls as f64;
    println!("... and 200 more upgraders, none asking: {:.4} ms a tick on average, {:.4} ms at most (they cost {:.4} ms)", now, st.max_ms, now - base);
    let t = std::time::Instant::now();
    ask(&mut s);
    println!("one upgrade: the whole tick of the request took {:.3} ms", t.elapsed().as_secs_f64() * 1000.0);
}
