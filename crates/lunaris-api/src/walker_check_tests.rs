//! `check` names the mistakes of a heavy machine (docs/WALKER.md): legs without an agent, a cannon without a shell, a stomp or a cannon without a side, a weak point that is a solid thing, and the Core's learning with nothing to learn from; a good one says nothing.
use crate::tests::engine;
use crate::Engine;
use lunaris_json::{parse, Json};

fn call(e: &mut Engine, tool: &str, args: &str) -> Json {
    e.call(tool, &parse(args).unwrap()).unwrap_or_else(|err| panic!("{tool} failed: {err}"))
}

fn spawn(e: &mut Engine, uid: u64, components: &str) {
    call(e, "spawn", &format!(r#"{{"uid":{uid},"components":{components}}}"#));
}

fn messages(e: &mut Engine) -> Vec<String> {
    let c = call(e, "check", "{}");
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| format!("{}: {}", p.get("severity").unwrap().as_str().unwrap(), p.get("message").unwrap().as_str().unwrap())).collect()
}

/// A machine: a body with legs, senses, a stomp and a cannon, a gun child and a core child.
fn machine(e: &mut Engine) {
    spawn(e, 1, r#"{"Transform":{},"Team":{"id":2},"HitPoints":{},"CharacterBody":{"radius":1.3,"height":5},"NavAgent":{},"Perception":{},"Pursuit":{},"Stomp":{},"Cannon":{}}"#);
    spawn(e, 2, r#"{"Parent":{"target":1},"LocalTransform":{},"Transform":{},"Weapon":{"kind":1}}"#);
    spawn(e, 3, r#"{"Parent":{"target":1},"LocalTransform":{},"Transform":{},"Collider":{"shape":0},"HitPoints":{},"Team":{"id":2},"CollisionLayer":{"layer":30,"mask":0},"Hitbox":{}}"#);
}

#[test]
fn a_good_machine_and_a_good_core_say_nothing() {
    let mut e = engine();
    machine(&mut e);
    call(&mut e, "define_prefab", r#"{"name":"Walker","components":{"Transform":{}}}"#);
    spawn(&mut e, 10, r#"{"Director":{"prefab":"Walker"}}"#);
    spawn(&mut e, 11, r#"{"Transform":{},"Heat":{}}"#);
    spawn(&mut e, 12, r#"{"Escalation":{"walker":"Walker","tiers":"tier 1 at=2\ntier 2 at=5"}}"#);
    let m = messages(&mut e);
    let ours: Vec<&String> = m.iter().filter(|x| ["Pursuit", "Cannon", "Stomp", "Hitbox", "Escalation"].iter().any(|w| x.contains(w))).collect();
    assert!(ours.is_empty(), "{ours:?}");
}

#[test]
fn the_mistakes_of_a_heavy_machine_are_named() {
    let mut e = engine();
    // legs with nothing to walk by and nothing to follow, and the autopilot as well
    spawn(&mut e, 1, r#"{"Transform":{},"Pursuit":{}}"#);
    spawn(&mut e, 2, r#"{"Transform":{},"NavAgent":{},"Perception":{"engage":true},"Pursuit":{}}"#);
    // a cannon with no gun, one whose gun is a hitscan, and one with no side; a stomp with no side and no senses
    spawn(&mut e, 3, r#"{"Transform":{},"Team":{"id":2},"Cannon":{}}"#);
    spawn(&mut e, 4, r#"{"Transform":{},"Team":{"id":2},"Cannon":{}}"#);
    spawn(&mut e, 5, r#"{"Parent":{"target":4},"LocalTransform":{},"Transform":{},"Weapon":{"kind":0}}"#);
    spawn(&mut e, 13, r#"{"Transform":{},"Cannon":{}}"#);
    spawn(&mut e, 6, r#"{"Transform":{},"Stomp":{}}"#);
    // a weak point with nobody to pass the damage to, a solid one, and one on the wrong side
    spawn(&mut e, 7, r#"{"Transform":{},"Collider":{"shape":0},"HitPoints":{},"Hitbox":{}}"#);
    spawn(&mut e, 8, r#"{"Transform":{},"Team":{"id":2},"HitPoints":{}}"#);
    spawn(&mut e, 14, r#"{"Parent":{"target":8},"LocalTransform":{},"Transform":{},"Hitbox":{}}"#);
    spawn(&mut e, 9, r#"{"Parent":{"target":8},"LocalTransform":{},"Transform":{},"Collider":{"shape":0},"HitPoints":{},"Hitbox":{}}"#);
    // the Core: no director, a prefab that is not there, a line of the tiers that is wrong, an event line that is wrong, a ring that is the wrong way round, and a second one
    spawn(&mut e, 20, r#"{"Escalation":{"walker":"Nobody","tiers":"tier 2 at=5","noise":"explosion","walker_min_dist":50,"walker_max_dist":10}}"#);
    spawn(&mut e, 21, r#"{"Escalation":{}}"#);
    let m = messages(&mut e);
    for want in [
        "Pursuit needs a NavAgent",
        "Pursuit with no Perception",
        "Pursuit and Perception.engage both walk and turn",
        "this Cannon has no gun",
        "is not a flying round (kind 1)",
        "a Cannon on a machine with no Team",
        "a Stomp on a machine with no Team",
        "a Stomp with no Perception never starts",
        "this Hitbox has no machine to pass the damage to",
        "needs HitPoints (a max too big to be used up) and a Collider that is not a trigger",
        "a Hitbox without a CollisionLayer mask of 0 is a solid thing",
        "this Hitbox has no Team while its machine has one",
        "an Escalation needs an active Director",
        "Escalation.walker `Nobody` is not a prefab",
        "Escalation.tiers: line 1: the tiers are numbered from 1 with no gap",
        "Escalation.noise: line 1",
        "walker_min_dist is greater than walker_max_dist",
        "more than one Escalation",
    ] {
        assert!(m.iter().any(|x| x.contains(want)), "`{want}` is missing from {m:#?}");
    }
}
