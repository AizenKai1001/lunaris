//! `check` names the mistakes of the world events (docs/EVENTS.md): a table line that does not read, a prefab it names that is not in the scene, a second scheduler, a supply drop with no `WorldEvent`; a good setup says nothing.
use crate::tests::engine;
use crate::Engine;
use lunaris_json::{parse, Json};

fn call(e: &mut Engine, tool: &str, args: &str) -> Json {
    e.call(tool, &parse(args).unwrap()).unwrap_or_else(|err| panic!("{tool} failed: {err}"))
}

fn messages(e: &mut Engine) -> Vec<String> {
    let c = call(e, "check", "{}");
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| format!("{}: {}", p.get("severity").unwrap().as_str().unwrap(), p.get("message").unwrap().as_str().unwrap())).collect()
}

#[test]
fn a_good_table_says_nothing_and_the_mistakes_of_events_are_named() {
    let mut e = engine();
    call(&mut e, "define_prefab", r#"{"name":"Drop","def":{"components":{"Transform":{}}}}"#);
    call(&mut e, "spawn", r#"{"uid":1,"components":{"WorldEvents":{"table":"event drop prefab=Drop every=60..90 first=10"}}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("WorldEvents") || x.contains("AirDrop")), "{m:?}");
    call(&mut e, "spawn", r#"{"uid":2,"components":{"WorldEvents":{"table":"event ghost prefab=Nothing marker=AlsoNothing\nevent x prefab=Drop speed=3\nthing\nevent d prefab=Drop every=9..3"}}}"#);
    call(&mut e, "spawn", r#"{"uid":3,"components":{"Transform":{},"AirDrop":{}}}"#);
    let m = messages(&mut e);
    let has = |s: &str| m.iter().any(|x| x.contains(s));
    assert!(has("more than one WorldEvents"), "{m:?}");
    assert!(has("unknown key `speed`") && has("a line is `event NAME key=value ...`, not `thing`") && has("every needs a number"), "{m:?}");
    assert!(has("is not a prefab of this scene"), "{m:?}");
    assert!(has("an AirDrop needs a WorldEvent beside it"), "{m:?}");
}
