//! `DrawLayer` through the tool API: `check` says what a layer on the wrong entity does not do, and a picture drawn through the `render` tool is ordered by it.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn problems(e: &mut crate::Engine) -> Vec<(String, String)> {
    let c = e.call("check", &Json::Null).unwrap();
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| (p.get("message").unwrap().as_str().unwrap().to_string(), p.get("hint").and_then(|h| h.as_str()).unwrap_or("").to_string())).collect()
}

fn mentions(e: &mut crate::Engine, text: &str) -> bool {
    problems(e).iter().any(|(m, _)| m.contains(text))
}

#[test]
fn check_is_quiet_about_a_layer_on_what_it_orders() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{},"Sprite":{},"DrawLayer":{"layer":3}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Transform":{},"Tilemap":{"size":[1,1],"data":"0"},"DrawLayer":{"layer":-1}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Transform":{},"Particles":{},"DrawLayer":{"layer":1}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":4,"components":{"Transform":{},"Decal":{},"DrawLayer":{"layer":1}}}"#)).unwrap();
    assert!(!mentions(&mut e, "DrawLayer"), "{:?}", problems(&mut e));
}

#[test]
fn check_says_what_a_layer_on_the_wrong_entity_does_not_do() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Ui":{},"DrawLayer":{"layer":3}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Text":{"value":"x"},"DrawLayer":{"layer":3}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Transform":{},"MeshRenderer":{},"DrawLayer":{"layer":3}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":4,"components":{"DrawLayer":{"layer":3}}}"#)).unwrap();
    let p = problems(&mut e);
    assert_eq!(p.iter().filter(|(m, _)| m.contains("has a DrawLayer")).count(), 4, "{p:?}");
    assert_eq!(p.iter().filter(|(m, h)| m.contains("screen's overlay") && h.contains("Ui.layer")).count(), 2, "{p:?}");
    assert!(p.iter().any(|(m, _)| m.contains("3D mesh is drawn by depth")), "{p:?}");
    assert!(p.iter().any(|(m, _)| m.contains("nothing it orders")), "{p:?}");
}

#[test]
fn check_warns_about_a_layer_that_is_not_a_number() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{},"Sprite":{},"DrawLayer":{}}}"#)).unwrap();
    assert!(!mentions(&mut e, "DrawLayer"));
    let entity = e.sim.world.entity(1).unwrap();
    e.sim.world.get_mut::<lunaris_sim::components::DrawLayer>(entity).unwrap().layer = f32::NAN;
    assert!(mentions(&mut e, "not a number the order can use"), "{:?}", problems(&mut e));
}

#[test]
fn the_z_warning_now_points_at_the_layer() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Camera":{"ortho":true},"Transform":{}}}"#)).unwrap();
    let body = |z: f32| format!(r#"{{"components":{{"Transform":{{"pos":[0,0,{z}]}},"Velocity":{{}},"Collider":{{"shape":0}},"RigidBody":{{"mass":1}}}}}}"#);
    e.call("spawn", &j(&body(0.0))).unwrap();
    e.call("spawn", &j(&body(0.1))).unwrap();
    assert!(problems(&mut e).iter().any(|(m, h)| m.contains("different z") && h.contains("DrawLayer")), "{:?}", problems(&mut e));
}

#[test]
fn the_render_tool_draws_by_layer_and_a_scene_file_carries_it() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{"pos":[0,0,5]},"Camera":{"ortho":true,"ortho_height":4}}}"#)).unwrap();
    // the hero is the lower uid and at the same z as the guard: only the layer puts it on top (and physics sees the same z)
    e.call("spawn", &j(r#"{"uid":2,"components":{"Name":{"value":"hero"},"Transform":{},"Sprite":{"size":[2,2],"color":[1,0,0,1]},"DrawLayer":{"layer":10}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Name":{"value":"guard"},"Transform":{},"Sprite":{"size":[2,2],"color":[0,1,0,1]},"DrawLayer":{"layer":5}}}"#)).unwrap();
    let r = e.call("render", &j(r#"{"width":64,"height":64,"quality":"reference"}"#)).unwrap();
    let visible = r.get("visible").unwrap().as_array().unwrap();
    let hero = visible.iter().find(|v| v.get("uid").and_then(|u| u.as_u64()) == Some(2)).expect("the hero is visible");
    assert!(hero.get("pixels").unwrap().as_u64().unwrap() > 1000, "the hero covers the middle of the picture: {hero:?}");
    assert!(visible.iter().all(|v| v.get("uid").and_then(|u| u.as_u64()) != Some(3)), "the guard is hidden behind the hero: {visible:?}");
    // without the layers the higher uid wins; the picture changes with them
    let layered = r.get("hash").unwrap().as_str().unwrap().to_string();
    e.call("remove_component", &j(r#"{"uid":2,"component":"DrawLayer"}"#)).unwrap();
    e.call("remove_component", &j(r#"{"uid":3,"component":"DrawLayer"}"#)).unwrap();
    let plain = e.call("render", &j(r#"{"width":64,"height":64,"quality":"reference"}"#)).unwrap();
    assert_ne!(plain.get("hash").unwrap().as_str().unwrap(), layered);
}

#[test]
fn a_graph_system_can_sort_by_y_for_a_top_down_game() {
    // the pattern of the docs: the layer is minus the y, so what stands lower on the screen is in front
    let mut e = engine();
    e.call("define_script", &j(r#"{"source":"system ysort each Transform, DrawLayer { DrawLayer.layer = 0 - Transform.pos[1]; }"}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{"pos":[0,2,0]},"Sprite":{},"DrawLayer":{}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Transform":{"pos":[0,-3,0]},"Sprite":{},"DrawLayer":{}}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":2}"#)).unwrap();
    let layer = |e: &mut crate::Engine, uid: u64| e.call("get", &j(&format!(r#"{{"uid":{uid},"component":"DrawLayer"}}"#))).unwrap().get("components").and_then(|c| c.get("DrawLayer")).and_then(|d| d.get("layer")).and_then(|l| l.as_f64()).unwrap();
    assert_eq!((layer(&mut e, 1), layer(&mut e, 2)), (-2.0, 3.0), "the one lower on the screen has the higher layer, so it is in front");
}

#[test]
fn a_saved_scene_keeps_its_layers() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Transform":{},"Sprite":{},"DrawLayer":{"layer":-2.5}}}"#)).unwrap();
    e.call("save_scene", &j(r#"{"path":"layers.lunaris.json"}"#)).unwrap();
    let before = e.call("hash", &Json::Null).unwrap();
    e.call("despawn", &j(r#"{"uid":2}"#)).unwrap();
    e.call("load_scene", &j(r#"{"path":"layers.lunaris.json"}"#)).unwrap();
    assert_eq!(e.call("hash", &Json::Null).unwrap().get("hash"), before.get("hash"));
    let got = e.call("get", &j(r#"{"uid":2,"component":"DrawLayer"}"#)).unwrap();
    assert_eq!(got.get("components").and_then(|c| c.get("DrawLayer")).and_then(|d| d.get("layer")).and_then(|l| l.as_f64()), Some(-2.5), "{got:?}");
}
