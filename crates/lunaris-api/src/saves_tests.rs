//! Saved games: the tools and the `game.save` / `game.load` resources.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn hash(e: &mut crate::Engine) -> String {
    e.call("hash", &Json::Null).unwrap().get("hash").unwrap().as_str().unwrap().to_string()
}

#[test]
fn a_saved_game_comes_back_exactly_and_the_slots_are_listed() {
    let mut e = engine();
    let _ = std::fs::remove_dir_all(e.root.join("saves"));
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{"pos":[0,5,0]},"Velocity":{},"Collider":{"shape":0},"RigidBody":{"mass":1}}}"#)).unwrap();
    e.call("set_resource", &j(r#"{"name":"score","value":7}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":10}"#)).unwrap();
    let before = hash(&mut e);
    let saved = e.call("save_game", &j(r#"{"slot":"first"}"#)).unwrap();
    assert_eq!(saved.get("tick").unwrap().as_u64(), Some(10));
    // life goes on and changes everything
    e.call("step", &j(r#"{"ticks":50}"#)).unwrap();
    e.call("set_resource", &j(r#"{"name":"score","value":99}"#)).unwrap();
    e.call("spawn", &j(r#"{"components":{"Transform":{}}}"#)).unwrap();
    assert_ne!(hash(&mut e), before);
    let loaded = e.call("load_game", &j(r#"{"slot":"first"}"#)).unwrap();
    assert_eq!(loaded.get("tick").unwrap().as_u64(), Some(10));
    assert_eq!(hash(&mut e), before, "the same world, tick and random numbers");
    // and playing on from the save gives what playing on from there gave
    e.call("step", &j(r#"{"ticks":50}"#)).unwrap();
    let again = hash(&mut e);
    e.call("load_game", &j(r#"{"slot":"first"}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":50}"#)).unwrap();
    assert_eq!(hash(&mut e), again);
    // a number is a slot too; the list shows both
    e.call("save_game", &j(r#"{"slot":3}"#)).unwrap();
    let list = e.call("saves", &Json::Null).unwrap();
    let names: Vec<String> = list.get("saves").unwrap().as_array().unwrap().iter().map(|s| s.get("slot").unwrap().as_str().unwrap().to_string()).collect();
    assert_eq!(names, vec!["first".to_string(), "slot3".to_string()]);
    // mistakes
    for bad in [r#"{"slot":"../x"}"#, r#"{"slot":""}"#, r#"{"slot":"a b"}"#, r#"{}"#, r#"{"slot":[1]}"#] {
        assert!(e.call("save_game", &j(bad)).is_err(), "{bad}");
    }
    let missing = e.call("load_game", &j(r#"{"slot":"nope"}"#)).unwrap_err();
    assert!(missing.hint.unwrap_or_default().contains("saves"));
}

#[test]
fn a_game_saves_and_loads_itself_through_resources() {
    let mut e = engine();
    let _ = std::fs::remove_dir_all(e.root.join("saves"));
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{"pos":[1,2,3]}}}"#)).unwrap();
    e.call("set_resource", &j(r#"{"name":"game.save","value":2}"#)).unwrap();
    e.call("step", &Json::Null).unwrap();
    assert!(e.root.join("saves").join("slot2.json").exists());
    let r = e.call("resources", &Json::Null).unwrap();
    let res = r.get("resources").unwrap();
    assert_eq!(res.get("game.save").unwrap().as_f64(), Some(0.0), "the request is answered");
    assert!(res.get("game.saved").is_some() && res.get("game.error").is_none(), "{res:?}");
    // change the world, then ask for the load
    e.call("despawn", &j(r#"{"uid":1}"#)).unwrap();
    e.call("spawn", &j(r#"{"components":{"Transform":{"pos":[9,9,9]}}}"#)).unwrap();
    e.call("set_resource", &j(r#"{"name":"game.load","value":2}"#)).unwrap();
    e.call("step", &Json::Null).unwrap();
    let p = e.call("get", &j(r#"{"uid":1,"components":["Transform"]}"#)).unwrap();
    assert_eq!(p.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().index(1).unwrap().as_f64(), Some(2.0));
    assert!(e.call("resources", &Json::Null).unwrap().get("resources").unwrap().get("game.loaded").is_some());
    // a slot that does not exist is reported, not fatal
    e.call("set_resource", &j(r#"{"name":"game.load","value":77}"#)).unwrap();
    e.call("step", &Json::Null).unwrap();
    assert_eq!(e.call("resources", &Json::Null).unwrap().get("resources").unwrap().get("game.error").unwrap().as_f64(), Some(1.0));
}

#[test]
fn type_text_types_into_a_focused_text_field_and_enter_submits_it() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Ui":{"pos":[0.1,0.1],"size":[0.4,0.1],"button":true},"UiInput":{}}}"#)).unwrap();
    for (k, v) in [("input.mouse_x", 0.3), ("input.mouse_y", 0.15), ("input.mouse_left", 1.0)] {
        e.call("set_resource", &j(&format!(r#"{{"name":"{k}","value":{v}}}"#))).unwrap();
    }
    e.call("step", &Json::Null).unwrap();
    e.call("set_resource", &j(r#"{"name":"input.mouse_left","value":0}"#)).unwrap();
    e.call("step", &Json::Null).unwrap();
    let r = e.call("type_text", &j(r#"{"text":"Lunaris","enter":true}"#)).unwrap();
    assert_eq!(r.get("typed").unwrap().as_u64(), Some(7));
    let f = e.call("get", &j(r#"{"uid":1,"components":["UiInput"]}"#)).unwrap();
    let field = f.get("components").unwrap().get("UiInput").unwrap();
    assert_eq!(field.get("text").unwrap().as_str(), Some("Lunaris"));
    assert_eq!(field.get("submitted").unwrap().as_bool(), Some(true));
    assert!(e.call("type_text", &j(&format!(r#"{{"text":"{}"}}"#, "x".repeat(201)))).is_err());
}
