//! `check` names the mistakes of the raiding components (docs/RAIDING.md): a thrower's rules that do not read or name an item or a prefab that is not there, an explosive that hurts nothing or that nothing sets off, a code lock on a thing that is not used,
//! a keypad whose item does not exist; a good setup says nothing.
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

const ITEMS: &str = r#"{"ItemTable":{"text":"item grenade | Grenade | stack=5\nitem lock | Code lock | stack=5\n"}}"#;

fn raid(m: &[String]) -> Vec<&String> {
    m.iter().filter(|x| x.contains("Thrower") || x.contains("Explosive") || x.contains("CodeLock") || x.contains("Keypad")).collect()
}

#[test]
fn a_good_raiding_setup_says_nothing() {
    let mut e = engine();
    spawn(&mut e, 1, ITEMS);
    call(&mut e, "define_prefab", r#"{"name":"Grenade","components":{"Name":{"value":"Grenade"},"Transform":{},"Collider":{"shape":0,"size":[0.1,0.1,0.1]},"RigidBody":{"mass":0.4},"Velocity":{},"Explosive":{"fuse":3,"radius":6,"damage":100}}}"#);
    spawn(&mut e, 2, r#"{"Transform":{},"Inventory":{"size":4},"Thrower":{"rules":"throw grenade prefab=Grenade speed=15\ncap player=3"},"Keypad":{"item":"lock"}}"#);
    spawn(&mut e, 3, r#"{"Transform":{},"Interactable":{"kind":1},"Door":{},"CodeLock":{"code":1234}}"#);
    spawn(&mut e, 4, r#"{"Transform":{},"Explosive":{"fuse":-1,"remote":true,"radius":5,"damage":50}}"#);
    let m = messages(&mut e);
    assert!(raid(&m).is_empty(), "{m:?}");
}

#[test]
fn the_mistakes_of_a_thrower_are_named() {
    let mut e = engine();
    spawn(&mut e, 1, ITEMS);
    call(&mut e, "define_prefab", r#"{"name":"Dud","components":{"Name":{"value":"Dud"},"Transform":{}}}"#);
    // an unknown item, a prefab that does not exist, a prefab with no Explosive, a thrown prefab with no body, a line that is not a line
    spawn(&mut e, 2, r#"{"Transform":{},"Thrower":{"rules":"throw rock prefab=Nothing\nthrow grenade prefab=Dud\nplace lock prefab=Dud\nfling grenade"}}"#);
    // no rules at all
    spawn(&mut e, 3, r#"{"Transform":{},"Thrower":{}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("`rock` is not an item of the ItemTable")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("prefab=Nothing is not a prefab of this scene")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("the prefab `Dud` has no Explosive")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("the prefab `Dud` is thrown and has no RigidBody and Collider")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("`fling` is not throw, place, cap or trigger")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("has no `throw` or `place` line")), "{m:?}");
}

#[test]
fn the_mistakes_of_explosives_locks_and_keypads_are_named() {
    let mut e = engine();
    spawn(&mut e, 1, ITEMS);
    spawn(&mut e, 10, r#"{"Explosive":{"radius":0,"damage":10}}"#);
    spawn(&mut e, 11, r#"{"Transform":{},"Explosive":{"fuse":-1,"radius":5,"damage":50}}"#);
    spawn(&mut e, 12, r#"{"Transform":{},"Explosive":{"fuse":2,"radius":5,"damage":50,"cover":3}}"#);
    spawn(&mut e, 20, r#"{"Transform":{},"CodeLock":{"code":12345}}"#);
    spawn(&mut e, 21, r#"{"Transform":{},"Interactable":{"kind":3},"CodeLock":{"code":5}}"#);
    spawn(&mut e, 30, r#"{"Transform":{},"Keypad":{"item":"nothing_like_it"}}"#);
    spawn(&mut e, 31, r#"{"Transform":{},"Keypad":{}}"#);
    let m = messages(&mut e);
    for want in ["an Explosive needs a Transform", "an Explosive with no radius or no damage hurts nothing", "is never set off", "Explosive.cover 3", "CodeLock.code 12345", "a CodeLock needs an Interactable", "only doors (1) and containers (2) are opened by code", "Keypad.item `nothing_like_it` is not an item", "a Keypad with no `item` cannot fit a lock"] {
        assert!(m.iter().any(|x| x.contains(want)), "{want}: {m:?}");
    }
}

#[test]
fn the_mistakes_of_tier_rules_are_named_and_a_good_pair_of_tiers_says_nothing() {
    let mut e = engine();
    spawn(&mut e, 1, ITEMS);
    spawn(&mut e, 2, r#"{"BuildRules":{"text":"piece a size=1,1,1 free=1 bullet=0.05 upgrade=b upgrade_cost=lock:2\npiece b size=1,1,1 free=1 bullet=0.02"}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("BuildRules")), "{m:?}");
    let mut e = engine();
    spawn(&mut e, 1, ITEMS);
    spawn(&mut e, 2, r#"{"BuildRules":{"text":"piece a size=1,1,1 free=1 bullet=2 upgrade=nothing upgrade_cost=scrap:x\npiece b size=1,1,1 free=1 upgrade_cost=scrap:5\npiece c size=1,1,1 free=1 upgrade=a upgrade_cost=rubble:3"}}"#);
    let m = messages(&mut e);
    for want in ["bullet=2 is not a share from 0 to 1", "upgrade_cost `scrap:x` is not ITEM:COUNT", "has upgrade=nothing, which is not a piece of these rules", "has an upgrade_cost but no upgrade=PIECE", "upgrade_cost `rubble`, which is not an item of the ItemTable"] {
        assert!(m.iter().any(|x| x.contains(want)), "{want}: {m:?}");
    }
}

#[test]
fn a_repair_cost_in_an_item_that_does_not_exist_is_named() {
    // (base depth, docs/DEPTH.md: what repairing a piece costs is items of the table, like an upgrade's cost)
    let mut e = engine();
    spawn(&mut e, 1, ITEMS);
    spawn(&mut e, 2, r#"{"BuildRules":{"text":"piece a size=1,1,1 free=1 repair_cost=lock:1\npiece b size=1,1,1 free=1 repair_cost=rubble:3,scrap:x"}}"#);
    let m = messages(&mut e);
    for want in ["repair_cost `rubble`, which is not an item of the ItemTable", "repair_cost `scrap:x` is not ITEM:COUNT"] {
        assert!(m.iter().any(|x| x.contains(want)), "{want}: {m:?}");
    }
    assert!(!m.iter().any(|x| x.contains("repair_cost `lock`")), "{m:?}");
}

#[test]
fn a_private_forward_line_is_not_a_mistake_but_a_bad_radius_still_is() {
    // (`raid_alarm own`: told only to the player it is about, docs/DEPTH.md)
    let mut e = engine();
    call(&mut e, "define_prefab", r#"{"name":"Blob","components":{"Transform":{}}}"#);
    spawn(&mut e, 1, r#"{"NetHost":{"player":"Blob","forward":"raid_alarm own\nshot far"}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("`raid_alarm own`")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("`shot far`: the radius is not a number")), "{m:?}");
}
