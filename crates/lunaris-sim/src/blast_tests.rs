//! Explosives: the rules text, the fall of damage with distance, cover, structures (pieces and doors), the work a blast may do, the fuse, touch, sticking and the trigger, a throw and a charge put on a wall (the bag, the cooldown,
//! the caps, the dead), and that the same world gives the same state. No art: boxes, spheres and capsules.
use super::*;
use crate::blast::{falloff, parse_rules, Explosive, Thrower, MAX_VICTIMS};
use crate::components::{HitPoints, Transform, Velocity};
use crate::inventory::Inventory;
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn prefab(s: &mut Sim, name: &str, def: &str) {
    s.scripts.prefabs.insert(name.to_string(), parse(def).unwrap());
}

fn hp(s: &Sim, uid: u64) -> f32 {
    s.world.get::<HitPoints>(s.world.entity(uid).unwrap()).unwrap().value
}

fn alive(s: &Sim, uid: u64) -> bool {
    s.world.entity(uid).is_some()
}

fn events(s: &Sim, name: &str) -> Vec<(u64, Option<[f32; 3]>)> {
    s.events.iter().filter(|e| e.name == name).map(|e| (e.uid, e.pos)).collect()
}

fn step(s: &mut Sim, n: usize) {
    for _ in 0..n {
        s.step();
    }
}

/// A world with a ground (a box whose top is y = 0) and nothing else.
fn world() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[200,1,200]}}"#);
    s
}

/// A standing character (capsule of 1.8 m, feet on the ground) at (x, 0, z) with plenty of hit points; nothing moves it.
fn person(s: &mut Sim, uid: u64, x: f32, z: f32, extra: &str) {
    let sep = if extra.is_empty() { "" } else { "," };
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{x},0,{z}]}},"CharacterBody":{{"radius":0.35,"height":1.8,"gravity_scale":0}},"HitPoints":{{"value":1000,"max":1000,"last":1000}}{sep}{extra}}}"#));
}

/// A bomb at (0, 0.9, 0) (chest height) that goes off at once unless `extra` says otherwise.
fn bomb(s: &mut Sim, uid: u64, extra: &str) {
    let mut e = String::from(r#""fuse":0.0,"radius":6,"damage":100,"core":0,"edge":0,"cover":0.25,"structure_mult":1"#);
    for part in extra.split(',').filter(|p| !p.is_empty()) {
        let key = part.split(':').next().unwrap();
        e = e.split(',').filter(|p| p.split(':').next().unwrap() != key).collect::<Vec<_>>().join(",");
        e.push(',');
        e.push_str(part);
    }
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[0,0.9,0]}},"Explosive":{{{e}}}}}"#));
}

const RULES: &str = "rules snap=1.5 min=0.2 gap=0.05 slope=0.7 step=90 ground=0.6 audit=1\\n\
piece foundation size=4,0.5,4 hp=500 free=1 anchor=1\\n\
  socket n at=0,0.25,-2 yaw=0   type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
  socket s at=0,0.25,2  yaw=180 type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
  socket e at=2,0.25,0  yaw=-90 type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
  socket w at=-2,0.25,0 yaw=90  type=edge mates=edge,wall_base carry=0.8,wall_base:0\\n\
piece wall size=4,3,0.2 hp=300 inset=0.12\\n\
  socket base at=0,-1.5,0 yaw=0 type=wall_base mates=edge carry=0.9 turn=0\\n";

#[test]
fn the_rules_text_is_read_and_its_mistakes_named() {
    let r = parse_rules("# what can be thrown\nthrow grenade prefab=Grenade speed=14 lift=0.1 cooldown=1.5\nplace satchel prefab=Satchel reach=2.5 cooldown=2 gap=0.1 # on a wall\ncap player=3 server=20\ntrigger range=40");
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.acts.len(), 2);
    assert_eq!((r.acts[0].place, r.acts[0].key.as_str(), r.acts[0].prefab.as_str(), r.acts[0].speed, r.acts[0].lift, r.acts[0].cooldown), (false, "grenade", "Grenade", 14.0, 0.1, 1.5));
    assert_eq!((r.acts[1].place, r.acts[1].key.as_str(), r.acts[1].reach, r.acts[1].cooldown, r.acts[1].gap), (true, "satchel", 2.5, 2.0, 0.1));
    assert_eq!((r.cap_player, r.cap_server, r.trigger_range), (3, 20, 40.0));
    // defaults, and what is wrong
    let d = parse_rules("throw grenade prefab=G");
    assert_eq!((d.cap_player, d.cap_server, d.acts[0].speed), (4, 64, 15.0));
    let bad = parse_rules("throw\nthrow rock\nplace brick prefab=B reach=far\nthrow x prefab=P wobble=3\nfling x prefab=P\ncap player=x\ntrigger far");
    assert_eq!(bad.problems.len(), 7, "{:#?}", bad.problems);
}

#[test]
fn damage_falls_in_a_straight_line_from_the_core_to_the_edge() {
    assert_eq!(falloff(100.0, 0.0, 6.0, 1.0, 0.2), 100.0);
    assert_eq!(falloff(100.0, 1.0, 6.0, 1.0, 0.2), 100.0);
    assert!((falloff(100.0, 3.5, 6.0, 1.0, 0.2) - 60.0).abs() < 1e-4);
    assert!((falloff(100.0, 6.0, 6.0, 1.0, 0.2) - 20.0).abs() < 1e-4);
    assert_eq!(falloff(100.0, 6.01, 6.0, 1.0, 0.2), 0.0);
    // no core: from the middle; an edge of 0 falls to nothing; no radius: nothing
    assert!((falloff(100.0, 3.0, 6.0, 0.0, 0.0) - 50.0).abs() < 1e-4);
    assert_eq!(falloff(100.0, 1.0, 0.0, 0.0, 0.0), 0.0);
    assert_eq!(falloff(100.0, f32::NAN, 6.0, 0.0, 0.0), 0.0);
}

#[test]
fn a_blast_hurts_what_is_near_by_distance_and_nothing_beyond_its_radius() {
    let mut s = world();
    // three people on the ground in a row, then one past the edge (the distance is to the nearest point of the capsule: the axis less 0.35)
    for (uid, x) in [(10u64, 1.35f32), (11, 3.35), (12, 5.35), (13, 6.5)] {
        person(&mut s, uid, x, 0.0, "");
    }
    bomb(&mut s, 1, r#""core":1,"edge":0.2"#);
    s.step();
    let took = |uid| 1000.0 - hp(&s, uid);
    assert!((took(10) - 100.0).abs() < 0.01, "inside the core: all of it, {}", took(10));
    assert!((took(11) - falloff(100.0, 3.0, 6.0, 1.0, 0.2)).abs() < 0.01, "{}", took(11));
    assert!((took(12) - falloff(100.0, 5.0, 6.0, 1.0, 0.2)).abs() < 0.01, "{}", took(12));
    assert_eq!(took(13), 0.0, "past the radius");
    assert_eq!(events(&s, "blast").len(), 1);
    assert_eq!(events(&s, "blast")[0].1, Some([0.0, 0.9, 0.0]));
    assert!(!alive(&s, 1), "the explosive is gone");
    // a `hit` for each person hurt, none for the one beyond
    let hit: Vec<u64> = events(&s, "hit").iter().map(|e| e.0).collect();
    assert_eq!(hit, vec![10, 11, 12]);
}

#[test]
fn the_distance_is_to_the_nearest_point_of_a_thing_not_to_its_middle() {
    let mut s = world();
    // a long wall whose middle is 10 m away and whose end is 1 m from the bomb
    spawn(&mut s, 20, r#"{"Transform":{"pos":[0,1.5,-11]},"Collider":{"shape":1,"size":[0.2,3,20]},"HitPoints":{"value":1000,"max":1000,"last":1000}}"#);
    bomb(&mut s, 1, r#""core":0,"edge":0"#);
    s.step();
    // (the bomb is at (0, 0.9, 0); the wall's end face is at z = -1: 1 m)
    let want = 100.0 * (1.0 - 1.0 / 6.0);
    assert!((1000.0 - hp(&s, 20) - want).abs() < 0.01, "{}", 1000.0 - hp(&s, 20));
}

#[test]
fn solid_cover_lets_only_its_share_through() {
    let mut s = world();
    person(&mut s, 10, 3.35, 0.0, ""); // behind the wall
    person(&mut s, 11, 0.0, 3.35, ""); // the same distance, in the open
    // a wall (no hit points of its own) between the bomb and the first one
    spawn(&mut s, 30, r#"{"Transform":{"pos":[1.5,1.5,0]},"Collider":{"shape":1,"size":[0.2,3,6]}}"#);
    bomb(&mut s, 1, r#""cover":0.25"#);
    s.step();
    let (behind, open) = (1000.0 - hp(&s, 10), 1000.0 - hp(&s, 11));
    assert!((open - falloff(100.0, 3.0, 6.0, 0.0, 0.0)).abs() < 0.01, "in the open: {open}");
    assert!((behind - open * 0.25).abs() < 0.01, "behind the wall a quarter: {behind} of {open}");
}

#[test]
fn a_wall_with_hit_points_is_cover_for_what_stands_behind_it_and_takes_the_blast_itself() {
    let mut s = world();
    spawn(&mut s, 30, r#"{"Transform":{"pos":[1.5,1.5,0]},"Collider":{"shape":1,"size":[0.2,3,6]},"HitPoints":{"value":1000,"max":1000,"last":1000}}"#);
    person(&mut s, 10, 3.35, 0.0, "");
    person(&mut s, 11, 0.0, 3.35, "");
    bomb(&mut s, 1, "");
    s.step();
    let (behind, open) = (1000.0 - hp(&s, 10), 1000.0 - hp(&s, 11));
    assert!(behind < open * 0.3 && behind > 0.0, "{behind} {open}");
    // the wall is 1.4 m from the bomb (its face at x = 1.4 less nothing) and takes the damage of that distance
    assert!((1000.0 - hp(&s, 30) - falloff(100.0, 1.4, 6.0, 0.0, 0.0)).abs() < 0.05, "{}", 1000.0 - hp(&s, 30));
}

#[test]
fn a_piece_or_a_door_takes_the_structure_multiple_and_other_things_do_not() {
    let mut s = world();
    spawn(&mut s, 1, &format!(r#"{{"BuildRules":{{"text":"{RULES}"}}}}"#));
    // a piece (a wall, anchored), a door, a crate with hit points, all 2 m from the bomb in different directions
    spawn(&mut s, 20, r#"{"Transform":{"pos":[-2.1,1.5,0]},"Collider":{"shape":1,"size":[0.2,3,4]},"BuildPiece":{"kind":"foundation","anchored":true,"checked":true},"HitPoints":{"value":1000,"max":1000,"last":1000}}"#);
    spawn(&mut s, 21, r#"{"Transform":{"pos":[2.1,1.5,0]},"Collider":{"shape":1,"size":[0.2,3,2]},"Interactable":{"kind":1},"Door":{},"HitPoints":{"value":1000,"max":1000,"last":1000}}"#);
    spawn(&mut s, 22, r#"{"Transform":{"pos":[0,1.5,2.1]},"Collider":{"shape":1,"size":[2,3,0.2]},"HitPoints":{"value":1000,"max":1000,"last":1000}}"#);
    bomb(&mut s, 5, r#""structure_mult":4,"cover":1"#);
    s.step();
    let (piece, door, crate_) = (1000.0 - hp(&s, 20), 1000.0 - hp(&s, 21), 1000.0 - hp(&s, 22));
    assert!((piece - 4.0 * falloff(100.0, 2.0, 6.0, 0.0, 0.0)).abs() < 0.05, "a piece takes four times: {piece}");
    assert!((door - 4.0 * falloff(100.0, 2.0, 6.0, 0.0, 0.0)).abs() < 0.05, "a door too: {door}");
    assert!((crate_ - falloff(100.0, 2.0, 6.0, 0.0, 0.0)).abs() < 0.05, "a crate once: {crate_}");
    // only what is not a structure is told with a `hit`
    assert_eq!(events(&s, "hit").iter().map(|e| e.0).collect::<Vec<_>>(), vec![22]);
}

#[test]
fn a_piece_that_is_brought_to_nothing_is_destroyed_and_what_hung_on_it_collapses_the_same_tick() {
    let mut s = world();
    spawn(&mut s, 1, &format!(r#"{{"BuildRules":{{"text":"{RULES}"}}}}"#));
    // a foundation on the ground and one joined to its east edge, over nothing (the ground there is not there: a hole in the ground box is not needed, the second is not an anchor because it is not marked)
    spawn(&mut s, 20, r#"{"Transform":{"pos":[0,0.25,0]},"Collider":{"shape":1,"size":[4,0.5,4]},"BuildPiece":{"kind":"foundation","anchored":true,"checked":true},"HitPoints":{"value":500,"max":500,"last":500}}"#);
    spawn(&mut s, 21, r#"{"Transform":{"pos":[4,0.25,0]},"Collider":{"shape":1,"size":[4,0.5,4]},"BuildPiece":{"kind":"foundation","anchored":false,"checked":true},"HitPoints":{"value":500,"max":500,"last":500}}"#);
    step(&mut s, 3);
    let st = |s: &Sim, uid: u64| s.world.get::<crate::components::BuildPiece>(s.world.entity(uid).unwrap()).unwrap().stability;
    assert!((st(&s, 21) - 0.8).abs() < 1e-5, "joined to the first, 0.8: {}", st(&s, 21));
    // the bomb west of the first: 1 m from it, 5 m from the second
    spawn(&mut s, 5, r#"{"Transform":{"pos":[-3,0.9,0]},"Explosive":{"fuse":0,"radius":3.5,"damage":100,"structure_mult":10,"cover":1}}"#);
    s.step();
    assert_eq!(events(&s, "build_destroyed").iter().map(|e| e.0).collect::<Vec<_>>(), vec![20], "the first piece is destroyed by the blast");
    assert_eq!(events(&s, "build_collapse").iter().map(|e| e.0).collect::<Vec<_>>(), vec![21], "and the one it held up collapses");
    assert!(!alive(&s, 20) && !alive(&s, 21));
}

#[test]
fn a_door_that_is_destroyed_goes_away_after_its_time() {
    let mut s = world();
    spawn(&mut s, 21, r#"{"Transform":{"pos":[0,1.05,-1.5]},"Collider":{"shape":1,"size":[1,2.1,0.12]},"Interactable":{"kind":1},"Door":{},"HitPoints":{"value":150,"max":150,"last":150,"remove_after":0.05}}"#);
    bomb(&mut s, 5, r#""structure_mult":3,"cover":1"#);
    s.step();
    assert!(s.world.get::<HitPoints>(s.world.entity(21).unwrap()).unwrap().dead, "150 hit points, 100 x 3 x 0.75 hurts it past that");
    assert!(events(&s, "died").iter().any(|e| e.0 == 21));
    assert!(events(&s, "kill").iter().any(|e| e.0 == 21));
    step(&mut s, 6);
    assert!(!alive(&s, 21), "removed by its Lifetime");
}

#[test]
fn explosion_damage_ignores_teams() {
    let mut s = world();
    person(&mut s, 10, 2.35, 0.0, r#""Team":{"id":3}"#);
    person(&mut s, 11, -2.35, 0.0, r#""Team":{"id":5}"#);
    person(&mut s, 12, 0.0, 2.35, r#""Team":{"id":0}"#);
    bomb(&mut s, 1, r#""cover":1"#);
    s.step();
    let a = 1000.0 - hp(&s, 10);
    assert!(a > 50.0 && (a - (1000.0 - hp(&s, 11))).abs() < 0.01 && (a - (1000.0 - hp(&s, 12))).abs() < 0.01, "everyone, whatever the side: {a}");
}

#[test]
fn a_blast_does_a_bounded_amount_of_work_the_nearest_things_only() {
    let mut s = world();
    // seventy crates in a row along x, 1 m apart: only the nearest MAX_VICTIMS are hurt
    for k in 0..70u64 {
        spawn(&mut s, 100 + k, &format!(r#"{{"Transform":{{"pos":[{},0.5,0]}},"Collider":{{"shape":1,"size":[0.5,1,0.5]}},"HitPoints":{{"value":1000,"max":1000,"last":1000}}}}"#, 0.7 + k as f32 * 0.07));
    }
    bomb(&mut s, 1, r#""radius":50,"cover":1"#);
    s.step();
    let hurt: usize = (0..70u64).filter(|k| hp(&s, 100 + k) < 1000.0).count();
    assert_eq!(hurt, MAX_VICTIMS);
    assert!(hp(&s, 100) < 1000.0 && hp(&s, 100 + 63) < 1000.0 && hp(&s, 100 + 64) == 1000.0, "the nearest 64");
    // and at most 24 `hit` events however many are hurt
    assert!(events(&s, "hit").len() <= 24, "{}", events(&s, "hit").len());
}

#[test]
fn at_most_eight_explosions_go_off_in_a_tick_and_the_rest_in_the_next() {
    let mut s = world();
    for k in 0..12u64 {
        spawn(&mut s, 40 + k, &format!(r#"{{"Transform":{{"pos":[{},0.9,20]}},"Explosive":{{"fuse":0,"radius":1,"damage":1}}}}"#, k as f32 * 3.0));
    }
    s.step();
    assert_eq!(events(&s, "blast").len(), crate::blast::MAX_BLASTS);
    s.step();
    assert_eq!(events(&s, "blast").len(), 12, "all twelve by the second tick");
}

#[test]
fn the_fuse_counts_down_and_the_tag_says_which_kind_it_was() {
    let mut s = world();
    bomb(&mut s, 1, r#""fuse":1.0,"tag":"frag""#);
    step(&mut s, 50);
    assert!(alive(&s, 1) && events(&s, "blast").is_empty(), "not yet at 0.83 s");
    let x = *s.world.get::<Explosive>(s.world.entity(1).unwrap()).unwrap();
    assert!((x.age - 50.0 / 60.0).abs() < 1e-3, "{}", x.age);
    step(&mut s, 12);
    assert!(!alive(&s, 1));
    assert_eq!(events(&s, "blast").len(), 1);
    assert_eq!(events(&s, "blast_frag").len(), 1, "the kind has its own event too");
}

#[test]
fn the_seconds_left_of_the_fuse_are_told_to_the_clients_through_the_net_entity() {
    let mut s = world();
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0.9,0]},"Explosive":{"fuse":2.0},"NetEntity":{"class":2,"kind":80}}"#);
    step(&mut s, 60);
    let n = *s.world.get::<crate::components::NetEntity>(s.world.entity(1).unwrap()).unwrap();
    assert!((n.anim_t - 1.0).abs() < 0.03, "{}", n.anim_t);
}

#[test]
fn an_explosive_with_a_life_goes_away_unexploded_and_one_with_no_fuse_waits() {
    let mut s = world();
    bomb(&mut s, 1, r#""fuse":-1,"life":0.5"#);
    bomb(&mut s, 3, r#""fuse":-1"#);
    step(&mut s, 20);
    assert!(alive(&s, 1) && alive(&s, 3));
    step(&mut s, 20);
    assert!(!alive(&s, 1), "its life ran out");
    assert!(events(&s, "blast").is_empty(), "and it did not explode");
    assert!(alive(&s, 3));
}

#[test]
fn a_body_that_touches_something_goes_off_at_the_impact_not_before() {
    let mut s = world();
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,3,0]},"Collider":{"shape":0,"size":[0.14,0.14,0.14]},"RigidBody":{"mass":0.4,"restitution":0.3,"friction":0.6},"Velocity":{"lin":[0,-2,0]},"Explosive":{"fuse":-1,"contact":true,"arm":0.1,"radius":2,"damage":10}}"#);
    step(&mut s, 20);
    assert!(events(&s, "blast").is_empty(), "still falling after a third of a second");
    step(&mut s, 60);
    let b = events(&s, "blast");
    assert_eq!(b.len(), 1, "the impact set it off");
    assert!(b[0].1.unwrap()[1] < 0.5, "at the ground: {:?}", b[0].1);
}

#[test]
fn a_sticky_body_stays_where_it_first_hit_and_goes_off_by_its_fuse() {
    let mut s = world();
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,3,0]},"Collider":{"shape":0,"size":[0.14,0.14,0.14]},"RigidBody":{"mass":0.4,"restitution":0.6,"friction":0.6},"Velocity":{"lin":[3,-2,0]},"Explosive":{"fuse":2.0,"sticky":true,"radius":2,"damage":10}}"#);
    step(&mut s, 70);
    let e = s.world.entity(10).unwrap();
    let x = *s.world.get::<Explosive>(e).unwrap();
    assert!(x.stuck, "it hit the ground");
    assert!(s.world.get::<Velocity>(e).is_none(), "and became a still thing");
    let p = s.world.get::<Transform>(e).unwrap().pos;
    step(&mut s, 20);
    let q = s.world.get::<Transform>(s.world.entity(10).unwrap()).unwrap().pos;
    assert_eq!(p, q, "it did not move any more");
    assert!(events(&s, "blast").is_empty());
    step(&mut s, 40);
    assert_eq!(events(&s, "blast").len(), 1, "after its two seconds");
}

#[test]
fn a_trigger_sets_off_the_owners_remote_charges_in_reach_and_nobody_elses() {
    let mut s = world();
    // two players with a thrower each (rules: nothing to throw), a charge each and a far one of the first
    for (uid, x) in [(1u64, -5.0f32), (11, 5.0)] {
        spawn(&mut s, uid, &format!(r#"{{"Transform":{{"pos":[{x},0,30]}},"Thrower":{{"rules":"trigger range=40"}},"HitPoints":{{"value":100,"max":100}}}}"#));
    }
    for (uid, owner, x) in [(50u64, 1u64, -20.0f32), (51, 11, 20.0), (52, 1, 200.0)] {
        spawn(&mut s, uid, &format!(r#"{{"Transform":{{"pos":[{x},0.9,30]}},"Explosive":{{"fuse":-1,"remote":true,"radius":1,"damage":1,"owner":{owner}}}}}"#));
    }
    // a charge that is not remote does not answer to the trigger
    spawn(&mut s, 53, r#"{"Transform":{"pos":[-6,0.9,30]},"Explosive":{"fuse":-1,"radius":1,"damage":1,"owner":1}}"#);
    step(&mut s, 3);
    assert!(events(&s, "blast").is_empty());
    s.world.get_mut::<Thrower>(s.world.entity(1).unwrap()).unwrap().trigger = true;
    s.step();
    let gone: Vec<u64> = events(&s, "blast").iter().map(|e| e.0).collect();
    assert_eq!(gone, vec![50], "the owner's charge in reach (15 m) and no other: {gone:?}");
    assert!(alive(&s, 51) && alive(&s, 52) && alive(&s, 53));
    assert!(!s.world.get::<Thrower>(s.world.entity(1).unwrap()).unwrap().trigger, "the request is answered");
    // a dead owner cannot press it
    s.world.get_mut::<HitPoints>(s.world.entity(11).unwrap()).unwrap().dead = true;
    s.world.get_mut::<Thrower>(s.world.entity(11).unwrap()).unwrap().trigger = true;
    s.step();
    assert!(alive(&s, 51));
}

#[test]
fn a_blast_sets_off_another_explosive_that_has_hit_points() {
    let mut s = world();
    spawn(&mut s, 9, r#"{"Transform":{"pos":[1,0.9,0]},"Collider":{"shape":1,"size":[0.4,0.4,0.4]},"HitPoints":{"value":20,"max":20,"last":20},"Explosive":{"fuse":-1,"radius":1,"damage":1}}"#);
    bomb(&mut s, 1, r#""cover":1"#);
    s.step();
    assert_eq!(hp(&s, 9), 0.0, "the first one hurt the second");
    assert!(events(&s, "blast").iter().all(|e| e.0 == 1));
    s.step();
    assert!(events(&s, "blast").iter().any(|e| e.0 == 9), "and it went off the tick after");
}

// ---------------------------------------------------------------- the players' side

const ITEMS: &str = "item grenade | Grenade | stack=5\\nitem satchel | Satchel | stack=3\\nitem pistol | Pistol | stack=1\\n";
const THROW_RULES: &str = "throw grenade prefab=Grenade speed=15 lift=0.15 cooldown=1.2\\nplace satchel prefab=Satchel reach=3 cooldown=2 gap=0.05\\ncap player=3 server=5\\ntrigger range=100";

/// A player (uid 1: an avatar with a bag of 3 grenades and 2 satchels, a thrower, 100 hit points) at the origin, and its head (uid 3, eye height 1.6 m) looking down -z.
fn player(s: &mut Sim) {
    spawn(s, 50, &format!(r#"{{"ItemTable":{{"text":"{ITEMS}"}}}}"#));
    prefab(s, "Grenade", r#"{"components":{"Name":{"value":"Grenade"},"Transform":{},"Collider":{"shape":0,"size":[0.14,0.14,0.14]},"RigidBody":{"mass":0.4,"restitution":0.35,"friction":0.6},"Velocity":{},"Explosive":{"fuse":3.5,"radius":6,"damage":140,"structure_mult":0.8,"edge":0.15,"cover":0.25},"NetEntity":{"class":2,"kind":80}}}"#);
    prefab(s, "Satchel", r#"{"components":{"Name":{"value":"Satchel"},"Transform":{},"Explosive":{"fuse":10,"radius":5,"damage":120,"structure_mult":3,"remote":true,"arm":0.2},"NetEntity":{"class":3,"kind":81}}}"#);
    let mut id = vec!["0".to_string(); 48];
    let mut count = vec!["0".to_string(); 48];
    (id[0], count[0]) = ("1".into(), "3".into());
    (id[1], count[1]) = ("2".into(), "2".into());
    spawn(s, 1, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"HitPoints":{{"value":100,"max":100,"last":100}},"Inventory":{{"size":12,"id":[{}],"count":[{}]}},"NetAvatar":{{"head":3}},"Thrower":{{"rules":"{THROW_RULES}"}}}}"#, id.join(","), count.join(",")));
    spawn(s, 3, r#"{"Transform":{"pos":[0,1.6,0]}}"#);
}

fn bag(s: &Sim, uid: u64) -> (u32, u32) {
    let inv = s.world.get::<Inventory>(s.world.entity(uid).unwrap()).unwrap();
    ((0..12).filter(|k| inv.id[*k] == 1).map(|k| inv.count[k]).sum(), (0..12).filter(|k| inv.id[*k] == 2).map(|k| inv.count[k]).sum())
}

fn ask(s: &mut Sim, uid: u64, want: u32) {
    s.world.get_mut::<Thrower>(s.world.entity(uid).unwrap()).unwrap().want = want;
}

fn thrower(s: &Sim, uid: u64) -> Thrower {
    *s.world.get::<Thrower>(s.world.entity(uid).unwrap()).unwrap()
}

fn denied(s: &Sim, uid: u64) -> String {
    s.world.strings().get(thrower(s, uid).denied.0).to_string()
}

fn explosives(s: &mut Sim) -> Vec<(u64, Explosive, [f32; 3])> {
    let mut out = Vec::new();
    let _ = s.world.for_each_uid::<(&Explosive, &Transform), _>(|_, uid, (x, t)| out.push((uid, *x, t.pos)));
    out.sort_by_key(|o| o.0);
    out
}

#[test]
fn a_throw_takes_the_item_out_of_the_bag_and_sends_a_body_along_the_look() {
    let mut s = world();
    player(&mut s);
    ask(&mut s, 1, 1);
    s.step();
    assert_eq!(bag(&s, 1), (2, 2), "one grenade left the bag");
    let list = explosives(&mut s);
    assert_eq!(list.len(), 1);
    let (uid, x, pos) = list[0].clone();
    assert_eq!(x.owner.0, 1, "it is the player's");
    // it left a little in front of the eyes (0.55 m ahead along -z)
    assert!((pos[0]).abs() < 0.05 && (pos[1] - 1.6).abs() < 0.25 && (pos[2] + 0.55).abs() < 0.3, "{pos:?}");
    let v = s.world.get::<Velocity>(s.world.entity(uid).unwrap()).unwrap().lin;
    let want = 15.0 / (1.0f32 + 0.15 * 0.15).sqrt();
    assert!((v[2] + want).abs() < 0.5 && v[1] > 1.5 && v[0].abs() < 0.01, "along -z, a little up: {v:?}");
    assert_eq!(events(&s, "throw").len(), 1);
    assert_eq!(events(&s, "throw")[0].0, 3, "the player's aim entity is the one the event is about");
    let t = thrower(&s, 1);
    assert_eq!((t.thrown, t.want, t.placed), (1, 0, 0));
    assert_eq!(denied(&s, 1), "");
}

#[test]
fn a_thrown_body_flies_falls_and_goes_off_where_it_lands() {
    let mut s = world();
    player(&mut s);
    ask(&mut s, 1, 1);
    step(&mut s, 215);
    let b = events(&s, "blast");
    assert_eq!(b.len(), 1, "after its 3.5 s");
    let at = b[0].1.unwrap();
    assert!(at[2] < -8.0 && at[2] > -40.0 && at[1] < 0.5, "it flew some metres along -z, bounced and lay on the ground: {at:?}");
    assert!(explosives(&mut s).is_empty());
}

#[test]
fn what_the_thrower_does_not_have_cannot_be_thrown_and_a_refusal_says_why() {
    let mut s = world();
    player(&mut s);
    // no more grenades
    {
        let e = s.world.entity(1).unwrap();
        let inv = s.world.get_mut::<Inventory>(e).unwrap();
        inv.count[0] = 0;
        inv.id[0] = 0;
    }
    ask(&mut s, 1, 1);
    s.step();
    assert_eq!(denied(&s, 1), "no_item");
    assert!(explosives(&mut s).is_empty());
    assert_eq!(events(&s, "throw_refused").len(), 1);
    assert_eq!(events(&s, "throw_refused_no_item").len(), 1, "and the reason is in the name of a second event");
    // the reason is there for one tick
    s.step();
    assert_eq!(denied(&s, 1), "");
    // a number that is no line of the rules
    step(&mut s, 20);
    ask(&mut s, 1, 9);
    s.step();
    assert_eq!(denied(&s, 1), "unknown");
}

#[test]
fn a_dead_player_throws_nothing() {
    let mut s = world();
    player(&mut s);
    s.world.get_mut::<HitPoints>(s.world.entity(1).unwrap()).unwrap().dead = true;
    ask(&mut s, 1, 1);
    s.step();
    assert_eq!(denied(&s, 1), "dead");
    assert_eq!(bag(&s, 1), (3, 2));
}

#[test]
fn a_throw_is_rate_limited_per_player_and_a_refusal_pauses_the_player_too() {
    let mut s = world();
    player(&mut s);
    ask(&mut s, 1, 1);
    s.step();
    assert_eq!(bag(&s, 1).0, 2);
    // asked again at once and every tick for a second: the cooldown is 1.2 s, nothing is heard, nothing is told
    for _ in 0..60 {
        ask(&mut s, 1, 1);
        s.step();
    }
    assert_eq!(bag(&s, 1).0, 2, "still one thrown");
    assert_eq!(events(&s, "throw").len(), 1);
    assert!(events(&s, "throw_refused").is_empty(), "a request in a cooldown is not even answered");
    step(&mut s, 20);
    ask(&mut s, 1, 1);
    s.step();
    assert_eq!(bag(&s, 1).0, 1, "after the cooldown the next goes");
    // another player is not held by this one's cooldown
}

#[test]
fn a_player_may_have_only_so_many_live() {
    let mut s = world();
    player(&mut s);
    // (a long fuse, so that the first is still in the air when the third is thrown; the bag holds more)
    prefab(&mut s, "Grenade", r#"{"components":{"Name":{"value":"Grenade"},"Transform":{},"Collider":{"shape":0,"size":[0.14,0.14,0.14]},"RigidBody":{"mass":0.4},"Velocity":{},"Explosive":{"fuse":30,"radius":6,"damage":140}}}"#);
    s.world.get_mut::<Inventory>(s.world.entity(1).unwrap()).unwrap().count[0] = 5;
    for k in 0..3 {
        ask(&mut s, 1, 1);
        step(&mut s, 80);
        assert_eq!(explosives(&mut s).len(), k + 1, "thrown {k}");
    }
    ask(&mut s, 1, 1);
    s.step();
    assert_eq!(denied(&s, 1), "cap", "the cap of the player is 3");
    assert_eq!(explosives(&mut s).len(), 3);
    assert_eq!(bag(&s, 1).0, 2, "nothing was taken");
    assert_eq!(events(&s, "throw_refused").len(), 1);
}

#[test]
fn the_server_may_have_only_so_many_in_all() {
    let mut s = world();
    player(&mut s);
    // the server's cap is 5: five of another player's (not this one's) are alive
    for k in 0..5u64 {
        spawn(&mut s, 80 + k, r#"{"Transform":{"pos":[60,0.9,50]},"Explosive":{"fuse":-1,"owner":999}}"#);
    }
    ask(&mut s, 1, 1);
    s.step();
    assert_eq!(denied(&s, 1), "cap_server");
    assert_eq!(bag(&s, 1).0, 3, "nothing was taken");
    // when one of them goes the player may throw
    s.world.despawn(s.world.entity(80).unwrap());
    step(&mut s, 20);
    ask(&mut s, 1, 1);
    s.step();
    assert_eq!(bag(&s, 1).0, 2);
}

#[test]
fn a_charge_is_put_on_the_surface_in_front_of_the_player_turned_to_it_and_not_beyond_its_reach() {
    let mut s = world();
    player(&mut s);
    // a wall 1.9 m in front of the eyes: its face is at z = -1.9
    spawn(&mut s, 30, r#"{"Transform":{"pos":[0,1.5,-2]},"Collider":{"shape":1,"size":[6,3,0.2]}}"#);
    ask(&mut s, 1, 2);
    s.step();
    assert_eq!(bag(&s, 1), (3, 1), "a satchel left the bag");
    let list = explosives(&mut s);
    assert_eq!(list.len(), 1);
    let (uid, x, pos) = list[0].clone();
    assert!(x.remote && x.owner.0 == 1);
    assert!((pos[0]).abs() < 0.01 && (pos[1] - 1.6).abs() < 0.01 && (pos[2] + 1.85).abs() < 0.01, "on the face of the wall, 5 cm out: {pos:?}");
    let q = s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().rot;
    let (s2, c2) = (std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    assert!((q[0] - s2).abs() < 1e-3 && q[1].abs() < 1e-3 && q[2].abs() < 1e-3 && (q[3] - c2).abs() < 1e-3, "its up is the wall's normal (+z): {q:?}");
    assert_eq!(events(&s, "charge_placed").len(), 1);
    assert_eq!(thrower(&s, 1).placed, 1);
    // it is still: no velocity
    assert!(s.world.get::<Velocity>(s.world.entity(uid).unwrap()).is_none());
    // a wall that is too far, and the sky
    s.world.get_mut::<Transform>(s.world.entity(30).unwrap()).unwrap().pos = [0.0, 1.5, -9.0];
    step(&mut s, 130);
    ask(&mut s, 1, 2);
    s.step();
    assert_eq!(denied(&s, 1), "no_surface");
    assert_eq!(bag(&s, 1), (3, 1), "nothing taken");
}

#[test]
fn a_charge_goes_off_by_its_fuse_and_breaks_the_wall_it_is_on() {
    let mut s = world();
    player(&mut s);
    spawn(&mut s, 1000, &format!(r#"{{"BuildRules":{{"text":"{RULES}"}}}}"#));
    // a foundation under the player's feet with a wall on its north edge: the wall's face is 1.9 m in front of the eyes
    spawn(&mut s, 31, r#"{"Transform":{"pos":[0,0.25,0]},"Collider":{"shape":1,"size":[4,0.5,4]},"BuildPiece":{"kind":"foundation","anchored":true,"checked":true},"HitPoints":{"value":500,"max":500,"last":500}}"#);
    spawn(&mut s, 30, r#"{"Transform":{"pos":[0,2,-2]},"Collider":{"shape":1,"size":[4,3,0.2]},"BuildPiece":{"kind":"wall","anchored":false,"checked":true},"HitPoints":{"value":300,"max":300,"last":300}}"#);
    step(&mut s, 3);
    ask(&mut s, 1, 2);
    s.step();
    step(&mut s, 60 * 10 + 5);
    // 120 damage x 3 for structures at 5 cm: 120 * (1 - 0.05/5) * 3 = 356 > 300; the foundation, 1.1 m away, takes 281 of its 500
    assert!(!alive(&s, 30), "the wall is gone");
    assert!(alive(&s, 31), "the foundation stands");
    assert!((500.0 - hp(&s, 31) - 3.0 * falloff(120.0, 1.1, 5.0, 0.0, 0.0)).abs() < 1.0, "{}", 500.0 - hp(&s, 31));
    assert_eq!(events(&s, "build_destroyed").iter().map(|e| e.0).collect::<Vec<_>>(), vec![30]);
    assert!(events(&s, "blast").len() == 1);
}

#[test]
fn the_player_presses_the_trigger_and_its_charge_goes_off_early() {
    let mut s = world();
    player(&mut s);
    spawn(&mut s, 30, r#"{"Transform":{"pos":[0,1.5,-2]},"Collider":{"shape":1,"size":[4,3,0.2]}}"#);
    ask(&mut s, 1, 2);
    s.step();
    step(&mut s, 30);
    assert!(events(&s, "blast").is_empty());
    s.world.get_mut::<Thrower>(s.world.entity(1).unwrap()).unwrap().trigger = true;
    s.step();
    assert_eq!(events(&s, "blast").len(), 1, "after half a second of its ten");
}

#[test]
fn the_same_world_gives_the_same_state_every_time() {
    let run = || {
        let mut s = world();
        player(&mut s);
        spawn(&mut s, 1000, &format!(r#"{{"BuildRules":{{"text":"{RULES}"}}}}"#));
        spawn(&mut s, 30, r#"{"Transform":{"pos":[0,1.5,-6]},"Collider":{"shape":1,"size":[4,3,0.2]},"BuildPiece":{"kind":"wall","anchored":true,"checked":true},"HitPoints":{"value":300,"max":300,"last":300}}"#);
        person(&mut s, 12, 1.0, -5.0, "");
        ask(&mut s, 1, 1);
        step(&mut s, 30);
        ask(&mut s, 1, 1);
        step(&mut s, 250);
        (s.world.state_hash(), s.events.iter().map(|e| (e.tick, e.name.clone(), e.uid)).collect::<Vec<_>>())
    };
    let (a, b) = (run(), run());
    assert_eq!(a, b);
    assert!(a.1.iter().any(|e| e.1 == "blast"), "something went off");
}

/// What idle throwers and a live explosive cost a tick (run by hand: `cargo test -p lunaris-sim --lib blast_costs --release -- --ignored --nocapture`).
#[test]
#[ignore]
fn blast_costs_what_the_docs_say() {
    let mut s = world();
    player(&mut s);
    for k in 0..200u64 {
        spawn(&mut s, 5000 + k, &format!(r#"{{"Transform":{{"pos":[{},0,{}]}},"Thrower":{{"rules":"{THROW_RULES}"}},"HitPoints":{{"value":100,"max":100}},"Inventory":{{"size":12}}}}"#, (k % 20) as f32 * 3.0, (k / 20) as f32 * 3.0 + 60.0));
    }
    s.profile = Some(Default::default());
    step(&mut s, 600);
    let p = s.profile.take().unwrap();
    let st = p.stages.get("gameplay blast::update").unwrap();
    println!("201 throwers, no explosives: blast::update {:.4} ms a tick on average, {:.4} ms at most", st.total_ms / st.calls as f64, st.max_ms);
    // one explosive alive
    spawn(&mut s, 9000, r#"{"Transform":{"pos":[0,0.9,500]},"Explosive":{"fuse":-1,"radius":6,"damage":1}}"#);
    s.profile = Some(Default::default());
    step(&mut s, 600);
    let p = s.profile.take().unwrap();
    let st = p.stages.get("gameplay blast::update").unwrap();
    println!("201 throwers, one live explosive: {:.4} ms a tick on average, {:.4} ms at most", st.total_ms / st.calls as f64, st.max_ms);
    // a blast in a crowd of 200 people and 400 pieces
    for k in 0..200u64 {
        person(&mut s, 7000 + k, (k % 20) as f32 * 0.8 - 8.0, (k / 20) as f32 * 0.8 - 4.0 + 200.0, "");
    }
    spawn(&mut s, 9001, r#"{"Transform":{"pos":[0,0.9,200]},"Explosive":{"fuse":0,"radius":8,"damage":50,"cover":0.25}}"#);
    let t = std::time::Instant::now();
    s.step();
    println!("one blast in a crowd of 200 (64 hurt): the tick took {:.3} ms", t.elapsed().as_secs_f64() * 1000.0);
    // the most a big server lets live: 800 charges that sit and wait (no fuse, a remote trigger), 4 each of 200 players
    for k in 0..800u64 {
        spawn(&mut s, 20000 + k, &format!(r#"{{"Transform":{{"pos":[{},0.1,{}]}},"Explosive":{{"fuse":-1,"remote":true,"radius":5,"damage":120,"owner":{}}}}}"#, (k % 40) as f32 * 3.0 - 60.0, (k / 40) as f32 * 3.0 + 700.0, 5000 + k / 4));
    }
    s.profile = Some(Default::default());
    step(&mut s, 600);
    let p = s.profile.take().unwrap();
    let st = p.stages.get("gameplay blast::update").unwrap();
    println!("201 throwers, 801 live explosives waiting: {:.4} ms a tick on average, {:.4} ms at most", st.total_ms / st.calls as f64, st.max_ms);
}

/// The machines hear a blast like a shot when the explosive says how far it carries (`noise`): they go to where it went off. A quiet one, the thrower's own side and a machine out of earshot hear nothing.
#[test]
fn a_loud_blast_is_heard_by_the_machines_of_the_other_side_within_its_noise() {
    let hear = |noise: f32, thrower_team: u32, machine_at: f32| -> ([f32; 3], f32) {
        let mut s = world();
        spawn(&mut s, 5, &format!(r#"{{"Transform":{{"pos":[0,0,3]}},"Team":{{"id":{thrower_team}}}}}"#));
        spawn(&mut s, 6, &format!(r#"{{"Transform":{{"pos":[{machine_at},0,0]}},"Team":{{"id":2}},"Perception":{{"engage":false,"sight_range":2,"hearing":1}}}}"#));
        bomb(&mut s, 7, &format!(r#""owner":5,"noise":{noise}"#));
        step(&mut s, 4);
        let p = s.world.get::<crate::components::Perception>(s.world.entity(6).unwrap()).unwrap();
        (p.noise_pos, p.awareness)
    };
    let (at, awareness) = hear(80.0, 1, 50.0);
    assert!(at[0].abs() < 0.01 && (at[1] - 0.9).abs() < 0.01 && at[2].abs() < 0.01 && awareness >= 0.5, "the machine 50 m away heard it and knows where: {at:?} {awareness}");
    let (at, awareness) = hear(0.0, 1, 50.0);
    assert!(at == [0.0; 3] && awareness == 0.0, "a quiet blast is not heard: {at:?} {awareness}");
    let (_, awareness) = hear(80.0, 2, 50.0);
    assert_eq!(awareness, 0.0, "the machines do not hear their own side's blast");
    let (_, awareness) = hear(80.0, 1, 90.0);
    assert_eq!(awareness, 0.0, "out of earshot");
}
