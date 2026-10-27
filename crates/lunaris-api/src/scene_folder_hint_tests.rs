//! `check` on a scene whose files are named from the scene's own folder (a world a generator wrote into a folder of its own): the error stays an error, and says that the file is there and how to open the scene.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn problems(e: &mut crate::Engine) -> Vec<Json> {
    e.call("check", &Json::Null).unwrap().get("problems").unwrap().as_array().unwrap().to_vec()
}

fn texture_error(e: &mut crate::Engine) -> Json {
    problems(e).into_iter().find(|p| p.get("message").and_then(|m| m.as_str()).is_some_and(|m| m.contains("`tile.png` was not found"))).expect("the texture is reported")
}

#[test]
fn a_file_that_is_next_to_the_scene_but_named_from_its_folder_gets_the_way_to_open_it() {
    let mut e = engine();
    let world = e.root.join("gen_world");
    std::fs::create_dir_all(&world).unwrap();
    std::fs::write(world.join("tile.png"), b"not a picture, only a file that is there").unwrap();
    let scene = r#"{"lunaris":1,"entities":[{"uid":1,"components":{"Transform":{},"Sprite":{"texture":"tile.png"}}}]}"#;
    std::fs::write(world.join("world.lunaris.json"), scene).unwrap();
    e.call("load_scene", &parse(r#"{"path":"gen_world/world.lunaris.json"}"#).unwrap()).unwrap();
    let p = texture_error(&mut e);
    assert_eq!(p.get("severity").and_then(|s| s.as_str()), Some("error"), "it is still an error: {p:?}");
    let hint = p.get("hint").and_then(|h| h.as_str()).unwrap_or("");
    assert!(hint.contains("--root gen_world") && hint.contains("gen_world/tile.png"), "{hint}");
}

#[test]
fn a_file_that_is_nowhere_gets_no_such_advice_and_neither_does_a_scene_that_was_not_loaded_from_a_file() {
    let mut e = engine();
    let world = e.root.join("gen_world");
    std::fs::create_dir_all(&world).unwrap();
    let scene = r#"{"lunaris":1,"entities":[{"uid":1,"components":{"Transform":{},"Sprite":{"texture":"tile.png"}}}]}"#;
    std::fs::write(world.join("world.lunaris.json"), scene).unwrap();
    e.call("load_scene", &parse(r#"{"path":"gen_world/world.lunaris.json"}"#).unwrap()).unwrap();
    let hint = texture_error(&mut e).get("hint").and_then(|h| h.as_str()).unwrap_or("").to_string();
    assert!(!hint.contains("--root"), "{hint}");
    // the same file next to the scene, and the scene given as text instead of a path: nothing says where it came from
    std::fs::write(world.join("tile.png"), b"x").unwrap();
    e.call("load_scene", &parse(&format!(r#"{{"scene":{scene}}}"#)).unwrap()).unwrap();
    let hint = texture_error(&mut e).get("hint").and_then(|h| h.as_str()).unwrap_or("").to_string();
    assert!(!hint.contains("--root"), "{hint}");
}
