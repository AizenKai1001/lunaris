//! `check` names the mistakes of interaction: a door with no Door, a container with no Inventory, a pickup of an item nobody knows, a key that is not an item, a hinge that cannot turn; good ones say nothing.
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

const ITEMS: &str = "item bandage | Bandage | stack=10\\nitem crowbar | Crowbar | stack=1\\n";

#[test]
fn good_doors_chests_and_pickups_say_nothing() {
    let mut e = engine();
    spawn(&mut e, 1, &format!(r#"{{"ItemTable":{{"text":"{ITEMS}"}}}}"#));
    spawn(&mut e, 2, r#"{"Transform":{},"Collider":{"shape":1},"Interactable":{"kind":1,"locked":true,"key":"crowbar"},"Door":{}}"#);
    spawn(&mut e, 3, r#"{"Transform":{},"Collider":{"shape":1},"Interactable":{"kind":2},"Inventory":{}}"#);
    spawn(&mut e, 4, r#"{"Transform":{},"Interactable":{"kind":3,"item":"bandage 2"}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("Interactable") || x.contains("Door")), "{m:?}");
}

#[test]
fn the_mistakes_of_interaction_are_named() {
    let mut e = engine();
    spawn(&mut e, 1, &format!(r#"{{"ItemTable":{{"text":"{ITEMS}"}}}}"#));
    spawn(&mut e, 2, r#"{"Transform":{},"Interactable":{"kind":1}}"#);
    spawn(&mut e, 3, r#"{"Transform":{},"Interactable":{"kind":2}}"#);
    spawn(&mut e, 4, r#"{"Transform":{},"Interactable":{"kind":3,"item":"nothing 2"}}"#);
    spawn(&mut e, 5, r#"{"Transform":{},"Collider":{},"Interactable":{"kind":1,"locked":true,"key":"skeleton"},"Door":{"axis":[0,0,0],"speed":0}}"#);
    spawn(&mut e, 6, r#"{"Interactable":{"kind":7}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("kind 1 (a door) has no Door")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("kind 2 (a container) has no Inventory")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Interactable.item `nothing 2`")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Interactable.key `skeleton`")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Door.axis is the zero vector")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Door.speed is 0")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Interactable.kind 7")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("needs a Transform")), "{m:?}");
    // a pickup with no item table at all
    let mut e = engine();
    spawn(&mut e, 4, r#"{"Transform":{},"Interactable":{"kind":3,"item":"bandage 2"}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("needs an ItemTable")), "{m:?}");
}
