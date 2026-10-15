//! A `SceneSwitch.path` that is not a file yet: a warning for a scene that is merely not built yet, an error for a name no scene has, and an export that refuses both.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn problems(e: &mut crate::Engine) -> Vec<String> {
    let c = e.call("check", &Json::Null).unwrap();
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| p.get("message").unwrap().as_str().unwrap().to_string()).collect()
}

#[test]
fn a_missing_scene_is_a_warning_only_when_it_is_named_like_a_scene() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"SceneSwitch":{"path":"levels/two.lunaris.json"}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"SceneSwitch":{"path":"levels/three.jsn"}}}"#)).unwrap();
    let c = e.call("check", &Json::Null).unwrap();
    let list = c.get("problems").unwrap().as_array().unwrap();
    let severity = |needle: &str| list.iter().find(|p| p.get("message").and_then(|m| m.as_str()).is_some_and(|m| m.contains(needle))).and_then(|p| p.get("severity")).and_then(|s| s.as_str()).map(str::to_string);
    assert_eq!(severity("levels/two.lunaris.json").as_deref(), Some("warning"), "a scene that is not built yet: {list:?}");
    assert_eq!(severity("levels/three.jsn").as_deref(), Some("error"), "a name no scene has: {list:?}");
    // once it is saved the warning goes
    std::fs::create_dir_all(e.root.join("levels")).unwrap();
    std::fs::write(e.root.join("levels/two.lunaris.json"), r#"{"lunaris":1,"entities":[]}"#).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("levels/two.lunaris.json")), "{:?}", problems(&mut e));
}

#[test]
fn scenes_that_name_each_other_in_a_circle_can_be_built_one_after_the_other() {
    // game over -> level -> game over: the first one saved cannot pass `check` without a warning, and after both are saved nothing is left
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"SceneSwitch":{"path":"over.lunaris.json"}}}"#)).unwrap();
    e.call("save_scene", &j(r#"{"path":"level.lunaris.json"}"#)).unwrap();
    let c = e.call("check", &Json::Null).unwrap();
    assert!(c.get("problems").unwrap().as_array().unwrap().iter().all(|p| p.get("severity").and_then(|s| s.as_str()) != Some("error")), "no error while the other scene is still to come: {c:?}");
    e.call("remove_component", &j(r#"{"uid":1,"component":"SceneSwitch"}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"SceneSwitch":{"path":"level.lunaris.json"}}}"#)).unwrap();
    e.call("save_scene", &j(r#"{"path":"over.lunaris.json"}"#)).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("SceneSwitch")), "{:?}", problems(&mut e));
}

#[test]
fn an_export_refuses_a_game_that_switches_to_a_scene_that_does_not_exist() {
    let mut e = engine();
    let root = e.root.clone();
    let _ = std::fs::remove_dir_all(root.join("dist_missing"));
    e.call("spawn", &j(r#"{"uid":1,"components":{"SceneSwitch":{"path":"next.lunaris.json"}}}"#)).unwrap();
    let refused = e.call("export_game", &j(r#"{"out":"dist_missing","name":"missing"}"#)).unwrap_err();
    assert_eq!(refused.code, "scene_missing", "{refused:?}");
    assert!(refused.message.contains("next.lunaris.json"), "{refused:?}");
    // when it exists, and names another that does not, the chain is followed
    std::fs::write(root.join("next.lunaris.json"), r#"{"lunaris":1,"entities":[{"uid":1,"components":{"SceneSwitch":{"path":"after.lunaris.json"}}}]}"#).unwrap();
    let refused = e.call("export_game", &j(r#"{"out":"dist_missing","name":"missing"}"#)).unwrap_err();
    assert_eq!(refused.code, "scene_missing");
    assert!(refused.message.contains("after.lunaris.json"), "{refused:?}");
    // `force` exports it anyway
    e.call("export_game", &j(r#"{"out":"dist_missing","name":"missing","force":true}"#)).unwrap();
    let _ = std::fs::remove_dir_all(root.join("dist_missing"));
    std::fs::write(root.join("after.lunaris.json"), r#"{"lunaris":1,"entities":[]}"#).unwrap();
    e.call("export_game", &j(r#"{"out":"dist_missing","name":"missing"}"#)).unwrap();
    assert!(root.join("dist_missing/after.lunaris.json").is_file());
}
