//! `UiNavAuto` and `SceneReset`, and a scene that is switched back to, through the tool API.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn problems(e: &mut crate::Engine) -> Vec<String> {
    let c = e.call("check", &Json::Null).unwrap();
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| p.get("message").unwrap().as_str().unwrap().to_string()).collect()
}

fn about(e: &mut crate::Engine) -> Vec<String> {
    problems(e).into_iter().filter(|m| m.contains("UiNavAuto") || m.contains("SceneReset")).collect()
}

fn resource(e: &mut crate::Engine, name: &str) -> Option<f64> {
    e.call("resources", &Json::Null).unwrap().get("resources").and_then(|r| r.get(name)).and_then(|v| v.as_f64())
}

#[test]
fn check_is_quiet_when_the_conveniences_are_where_they_belong() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"UiNav":{},"UiNavAuto":{}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"SceneSwitch":{"path":"x.lunaris.json"},"SceneReset":{"resources":"paused leave time.scale fx.* *"}}}"#)).unwrap();
    assert!(about(&mut e).is_empty(), "{:?}", about(&mut e));
}

#[test]
fn check_says_what_a_misplaced_convenience_does_not_do() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"UiNavAuto":{}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"SceneReset":{"resources":"paused"}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"SceneSwitch":{"path":"x.lunaris.json"},"SceneReset":{}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":4,"components":{"SceneSwitch":{"path":"x.lunaris.json"},"SceneReset":{"resources":"paused bad-name! ok.name"}}}"#)).unwrap();
    let p = about(&mut e);
    assert!(p.iter().any(|m| m.contains("UiNavAuto but no UiNav")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("SceneReset but no SceneSwitch")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("names nothing")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("`bad-name!` is not a resource name")) && !p.iter().any(|m| m.contains("`ok.name`") || m.contains("`paused` is not")), "{p:?}");
}

#[test]
fn a_game_can_go_back_to_the_scene_that_holds_its_music_and_reset_what_the_new_scene_declares() {
    let mut e = engine();
    let scene = |extra: &str| format!(r#"{{"lunaris":1,"entities":[{{"uid":1,"components":{{"Name":{{"value":"music"}},"Persist":{{}}}}}},{{"uid":2,"components":{{"Name":{{"value":"door"}},"SceneSwitch":{{"path":"b.lunaris.json"}},"SceneReset":{{"resources":"paused time.scale"}}}}}}{extra}],"resources":{{"paused":0}}}}"#);
    std::fs::write(e.root.join("a.lunaris.json"), scene("")).unwrap();
    std::fs::write(e.root.join("b.lunaris.json"), scene("").replace("b.lunaris.json", "a.lunaris.json")).unwrap();
    e.call("load_scene", &j(r#"{"path":"a.lunaris.json"}"#)).unwrap();
    for (n, from) in [(1, "a"), (2, "b"), (3, "a")] {
        e.call("set_resource", &j(r#"{"name":"paused","value":1}"#)).unwrap();
        e.call("set_resource", &j(r#"{"name":"time.scale","value":0.5}"#)).unwrap();
        e.call("set_resource", &j(r#"{"name":"score","value":9}"#)).unwrap();
        e.call("set", &j(r#"{"uid":2,"component":"SceneSwitch","value":{"go":true}}"#)).unwrap();
        e.call("step", &j(r#"{"ticks":1}"#)).unwrap();
        assert_eq!(resource(&mut e, "scene.error"), Some(0.0), "switch {n} from {from}");
        assert_eq!(resource(&mut e, "scene.skipped"), Some(1.0), "the music of the new scene is the persistent one");
        assert_eq!(resource(&mut e, "paused"), Some(0.0), "reset: what the new scene declares");
        assert_eq!(resource(&mut e, "time.scale"), None, "reset: the new scene says nothing, so the engine's default again");
        assert_eq!(resource(&mut e, "score"), Some(9.0), "not named: carried over");
    }
    let found = e.call("query", &j(r#"{"with":["Persist"]}"#)).unwrap();
    assert_eq!(found.get("total").and_then(|c| c.as_u64()), Some(1), "{found:?}");
}

fn about_scale(e: &mut crate::Engine) -> Vec<String> {
    problems(e).into_iter().filter(|m| m.contains("UiScale") || m.contains("UiEdge")).collect()
}

#[test]
fn check_is_quiet_about_a_scale_and_edges_that_are_used_properly() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"UiScale":{"aspect":1.7778}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Ui":{"pos":[0.02,0.02],"size":[0.2,0.05]},"UiEdge":{"x":0,"y":0}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Text":{"value":"x"},"UiEdge":{"x":1}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":4,"components":{"Ui":{"size":[1,1]},"UiEdge":{"screen":true}}}"#)).unwrap();
    assert!(about_scale(&mut e).is_empty(), "{:?}", about_scale(&mut e));
}

#[test]
fn check_says_what_is_wrong_with_a_scale_and_an_edge() {
    let mut e = engine();
    // edges with no scale, on nothing, out of range, on a button
    e.call("spawn", &j(r#"{"uid":2,"components":{"Ui":{},"UiEdge":{}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"UiEdge":{}}}"#)).unwrap();
    let p = about_scale(&mut e);
    assert!(p.iter().any(|m| m.contains("no UiScale")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("no Ui or Text")), "{p:?}");
    // a nonsense shape is ignored and says so; a second good one says which is used
    e.call("spawn", &j(r#"{"uid":1,"components":{"UiScale":{"aspect":0}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":5,"components":{"UiScale":{"aspect":1.7778}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":6,"components":{"UiScale":{"aspect":1.3333}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":7,"components":{"Ui":{"button":true},"UiEdge":{"x":1.5}}}"#)).unwrap();
    let p = about_scale(&mut e);
    assert!(p.iter().any(|m| m.contains("UiScale.aspect 0 is not a shape")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("a second UiScale") && m.contains("(1)")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("UiEdge x 1.5")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("button with a UiEdge")), "{p:?}");
    assert!(p.iter().all(|m| !m.contains("no UiScale")), "{p:?}");
}
