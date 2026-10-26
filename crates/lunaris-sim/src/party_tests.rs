//! Parties in the simulation (docs/PARTIES.md): a party's number is a team to the weapons, `same_party` for the rules that come after, land that is a party member's is every member's, and a place to get up at that is a
//! party mate's is the member's own once their own are gone. The membership itself (invitations, leaders, chat) is the server's, in `lunaris-net`. No art: boxes and numbers.
use super::*;
use crate::components::{BuildTool, HitPoints, Respawn, Transform};
use crate::party::{party_of, same_party, PARTY_TEAM_BASE};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn hp(s: &Sim, uid: u64) -> f32 {
    s.world.get::<HitPoints>(s.world.entity(uid).unwrap()).unwrap().value
}

#[test]
fn a_party_number_is_a_team_and_same_party_says_who_is_friends() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Team":{"id":100}}"#);
    spawn(&mut s, 2, r#"{"Team":{"id":100}}"#);
    spawn(&mut s, 3, r#"{"Team":{"id":101}}"#);
    spawn(&mut s, 4, r#"{"Team":{"id":2}}"#);
    spawn(&mut s, 5, r#"{"Team":{"id":2}}"#);
    spawn(&mut s, 6, r#"{"Team":{"id":0}}"#);
    spawn(&mut s, 7, r#"{"Transform":{}}"#);
    assert_eq!(PARTY_TEAM_BASE, 100, "the machines' team (2) and the numbers a game gives its own teams stay below the parties'");
    assert!(same_party(&s.world, 1, 2) && same_party(&s.world, 2, 1), "two members of party 100");
    assert!(same_party(&s.world, 1, 1), "a member is in its own party");
    assert!(!same_party(&s.world, 1, 3), "party 100 and party 101");
    assert!(!same_party(&s.world, 4, 5), "two machines of team 2 are not a party: it is a number from 100 up that makes one");
    assert!(!same_party(&s.world, 6, 6) && !same_party(&s.world, 6, 1), "no team is no party, even with itself");
    assert!(!same_party(&s.world, 7, 7) && !same_party(&s.world, 7, 1), "an entity with no `Team` is in none");
    assert!(!same_party(&s.world, 1, 999) && !same_party(&s.world, 999, 999), "an entity that is not there is in none");
    assert_eq!((party_of(&s.world, 1), party_of(&s.world, 3), party_of(&s.world, 4), party_of(&s.world, 7)), (100, 101, 0, 0));
}

#[test]
fn weapons_never_hurt_a_member_of_the_shooters_party_and_hurt_everybody_else() {
    // the shooter (uid 3) stands at the muzzle; one behind the other in the line: a party mate, a player of another party, a player of no party, a machine
    let setup = |shooter_party: u32| {
        let mut s = Sim::new(1);
        spawn(&mut s, 3, &format!(r#"{{"Transform":{{"pos":[0,0.9,0]}},"Collider":{{"shape":2,"size":[0.7,1.8,0.7]}},"HitPoints":{{"value":100,"max":100}},"Team":{{"id":{shooter_party}}}}}"#));
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0.9,0]},"Weapon":{"owner":3,"spread":0,"bloom_per_shot":0,"recoil":[0,0],"damage":10,"fire":true}}"#);
        let dummy = |s: &mut Sim, uid: u64, d: f32, team: &str| {
            spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[0,0.9,{}]}},"Collider":{{"shape":2,"size":[0.7,1.8,0.7]}},"HitPoints":{{"value":100,"max":100}}{team}}}"#, -d));
        };
        dummy(&mut s, 10, 4.0, r#","Team":{"id":100}"#);
        dummy(&mut s, 11, 8.0, r#","Team":{"id":101}"#);
        dummy(&mut s, 12, 12.0, "");
        dummy(&mut s, 13, 16.0, r#","Team":{"id":2}"#);
        s.step();
        s
    };
    let s = setup(100);
    assert_eq!(hp(&s, 3), 100.0, "the shooter is not hit by its own round");
    assert_eq!(hp(&s, 10), 100.0, "a party mate: not hurt, and does not stop the round (friendly fire is off inside a party)");
    assert!(hp(&s, 11) < 100.0, "the next party is hurt: {}", hp(&s, 11));
    let s = setup(100 + 5);
    assert!(hp(&s, 10) < 100.0, "a player of the party 100 is an enemy of party 105: friendly fire is only off inside one party");
    let s = setup(0);
    assert!(hp(&s, 10) < 100.0, "a shooter in no party hurts a member of any");
}

/// A foundation that stands on the ground.
const RULES: &str = "rules snap=1.5 min=0.2 gap=0.05 slope=0.7 step=90 ground=0.6 audit=1\npiece foundation size=4,0.5,4 hp=500 free=1 anchor=1\n";

/// A ground, a cupboard (land of owner 3, 10 m round the origin) and two tools: uid 10 of owner 7 (the builder) and uid 11 of owner 3 (the land's owner, far away), each with a party.
fn land(builder_party: u32, owner_party: u32) -> Sim {
    let mut s = Sim::new(1);
    let text = RULES.replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"BuildRules":{{"text":"{text}"}}}}"#));
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[80,1,80]}}"#);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,0,0]},"BuildUpkeep":{"owner":3,"radius":10}}"#);
    spawn(&mut s, 10, &format!(r#"{{"Transform":{{"pos":[0,5,0]}},"BuildTool":{{"reach":12,"owner":7,"party":{builder_party}}}}}"#));
    spawn(&mut s, 11, &format!(r#"{{"Transform":{{"pos":[30,5,30]}},"BuildTool":{{"reach":12,"owner":3,"party":{owner_party}}}}}"#));
    s
}

/// Tool 10 above `x, z` looking straight down, asking to build a foundation: (can it, why not).
fn can_build(s: &mut Sim, x: f32, z: f32) -> (bool, String) {
    let q = crate::perception::look_rot([0.0, -5.0, 0.01], true);
    let e = s.world.entity(10).unwrap();
    let t = s.world.get_mut::<Transform>(e).unwrap();
    t.pos = [x, 5.0, z - 0.01];
    t.rot = q;
    let sym = s.world.strings_mut().intern("foundation");
    s.world.get_mut::<BuildTool>(e).unwrap().kind = lunaris_ecs::Sym(sym);
    s.step();
    let tool = *s.world.get::<BuildTool>(e).unwrap();
    (tool.valid, s.world.strings().get(tool.why.0).to_string())
}

#[test]
fn land_of_a_party_member_is_every_members_and_nobody_elses() {
    // the builder (owner 7) and the land's owner (owner 3) are in the same party: not refused
    let mut s = land(100, 100);
    assert_eq!(can_build(&mut s, 3.0, 3.0), (true, String::new()), "land of a party mate is the member's too");
    // in different parties, or only one of them in a party, or neither: `not_yours`
    for (a, b) in [(100, 101), (100, 0), (0, 100), (0, 0)] {
        let mut s = land(a, b);
        assert_eq!(can_build(&mut s, 3.0, 3.0), (false, "not_yours".to_string()), "parties {a} and {b}");
    }
    // the land's own owner is never refused, party or not
    let mut s = land(0, 0);
    let e = s.world.entity(10).unwrap();
    s.world.get_mut::<BuildTool>(e).unwrap().owner = 3;
    assert!(can_build(&mut s, 3.0, 3.0).0);
    // while the owner of the land is away (no tool of theirs in the world) their party mates are refused: parties last as long as the players are in
    let mut s = land(100, 100);
    s.apply(&Command::Despawn { uid: 11 }).unwrap();
    assert_eq!(can_build(&mut s, 3.0, 3.0), (false, "not_yours".to_string()));
    // outside the land nobody is refused
    let mut s = land(0, 0);
    assert!(can_build(&mut s, 25.0, 25.0).0, "outside the 10 m of the cupboard");
}

const ITEMS: &str = "item bandage | Bandage | stack=10\n";

/// A player at (x, 0, 0): 100 hit points, a respawn with the group `group`, and the team `team`.
fn player(s: &mut Sim, uid: u64, x: f32, group: u32, team: u32) {
    spawn(
        s,
        uid,
        &format!(r#"{{"Transform":{{"pos":[{x},0,0]}},"HitPoints":{{"value":100,"max":100,"last":100}},"Respawn":{{"corpse":"corpse","delay":1,"protect":0,"group":{group}}},"Team":{{"id":{team}}}}}"#),
    );
}

fn point(s: &mut Sim, uid: u64, x: f32, z: f32, group: u32) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[{x},0,{z}]}},"SpawnPoint":{{"group":{group}}}}}"#));
}

fn kill(s: &mut Sim, uid: u64) {
    let e = s.world.entity(uid).unwrap();
    s.world.get_mut::<HitPoints>(e).unwrap().value = 0.0;
}

fn at(s: &Sim, uid: u64) -> [f32; 3] {
    s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
}

#[test]
fn a_party_member_without_a_place_of_their_own_gets_up_at_a_party_mates_and_a_stranger_does_not() {
    let mut s = Sim::new(1);
    let items = ITEMS.replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"ItemTable":{{"text":"{items}"}}}}"#));
    s.apply(&Command::DefinePrefab { name: "corpse".into(), def: parse(r#"{"components":{"Name":{"value":"Corpse"}}}"#).unwrap() }).unwrap();
    // Ada (group 1) and Bo (group 2) are in party 100, Cy (group 3) is in none, Di (group 4) is in party 101 with nobody else here
    player(&mut s, 10, 0.0, 1, 100);
    player(&mut s, 11, 3.0, 2, 100);
    player(&mut s, 12, 6.0, 3, 0);
    player(&mut s, 13, 9.0, 4, 101);
    // Ada's sleeping bag far away to the east, one shared place far away to the west, a place of party 100's other member's group is none yet
    point(&mut s, 20, 200.0, 0.0, 1);
    point(&mut s, 21, -200.0, 0.0, 0);
    s.step_n(2);
    // Bo dies: no place of their own, so Ada's (group 1) is theirs, not the shared one
    kill(&mut s, 11);
    s.step_n(80);
    let p = at(&s, 11);
    assert!((p[0] - 200.0).abs() < 1.0 && p[2].abs() < 1.0, "Bo got up at Ada's place: {p:?}");
    // Cy and Di are not in that party: they get up at the shared place, never at Ada's
    kill(&mut s, 12);
    kill(&mut s, 13);
    s.step_n(80);
    for uid in [12, 13] {
        let p = at(&s, uid);
        assert!((p[0] + 200.0).abs() < 1.0, "a player of another party gets up at the shared place, not at Ada's: {uid} at {p:?}");
    }
    // Bo's own place (group 2) comes first, wherever Ada's is
    point(&mut s, 900, 100.0, 50.0, 2);
    kill(&mut s, 11);
    s.step_n(80);
    let p = at(&s, 11);
    assert!((p[0] - 100.0).abs() < 1.0 && (p[2] - 50.0).abs() < 1.0, "Bo's own place before a mate's: {p:?}");
    // when Bo leaves the party (their team goes back to 0) Ada's place is no longer theirs: with their own one used up they get up at the shared place
    let e = s.world.entity(900).unwrap();
    s.world.get_mut::<crate::components::SpawnPoint>(e).unwrap().active = false;
    let b = s.world.entity(11).unwrap();
    s.world.get_mut::<crate::components::Team>(b).unwrap().id = 0;
    kill(&mut s, 11);
    s.step_n(80);
    let p = at(&s, 11);
    assert!((p[0] + 200.0).abs() < 1.0, "out of the party, Ada's place is Ada's: {p:?}");
    // a mate's place is a bag that is used up when somebody gets up on it (`once`): the next one goes elsewhere
    let mut s = Sim::new(1);
    s.apply(&Command::DefinePrefab { name: "corpse".into(), def: parse(r#"{"components":{"Name":{"value":"Corpse"}}}"#).unwrap() }).unwrap();
    player(&mut s, 10, 0.0, 1, 100);
    player(&mut s, 11, 3.0, 2, 100);
    spawn(&mut s, 20, r#"{"Transform":{"pos":[200,0,0]},"SpawnPoint":{"group":1,"once":true}}"#);
    point(&mut s, 21, -200.0, 0.0, 0);
    s.step_n(2);
    kill(&mut s, 11);
    s.step_n(80);
    assert!((at(&s, 11)[0] - 200.0).abs() < 1.0);
    kill(&mut s, 11);
    s.step_n(80);
    assert!((at(&s, 11)[0] + 200.0).abs() < 1.0, "the bag was used up");
    let r = *s.world.get::<Respawn>(s.world.entity(11).unwrap()).unwrap();
    assert_eq!(r.deaths, 2);
}
