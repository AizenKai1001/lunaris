//! Things the 3D game "Caldera" found in `check` and the tools (`docs/DOGFOOD_3D.md`).
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
fn an_orthographic_minimap_camera_does_not_make_a_3d_scene_2d() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Camera":{},"Transform":{"pos":[0,3,8]}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Camera":{"ortho":true,"active":false},"Transform":{"pos":[0,100,0]}}}"#)).unwrap();
    let body = |x: f32, z: f32| format!(r#"{{"components":{{"Transform":{{"pos":[{x},0,{z}]}},"Velocity":{{}},"Collider":{{"shape":0}},"RigidBody":{{"mass":1}}}}}}"#);
    e.call("spawn", &j(&body(0.0, 0.0))).unwrap();
    e.call("spawn", &j(&body(3.0, 9.0))).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("different z")), "bodies at different z in a 3D scene are normal: {:?}", problems(&mut e));
    // the picture is the active camera with the lowest uid: when that one is orthographic the warning is there again
    e.call("set", &j(r#"{"uid":1,"component":"Camera","value":{"active":false}}"#)).unwrap();
    e.call("set", &j(r#"{"uid":2,"component":"Camera","value":{"active":true}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("different z")), "{:?}", problems(&mut e));
}

#[test]
fn check_wants_a_camera_rig_beside_a_camera_collide() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":1,"components":{"Camera":{},"Transform":{},"CameraCollide":{}}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("CameraCollide but no CameraRig")), "{:?}", problems(&mut e));
    e.call("spawn", &j(r#"{"uid":2,"components":{"Transform":{}}}"#)).unwrap();
    e.call("add_component", &j(r#"{"uid":1,"component":"CameraRig","value":{"target":2}}"#)).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("CameraCollide but no CameraRig")), "{:?}", problems(&mut e));
}

#[test]
fn check_warns_about_turned_boxes_only_where_characters_or_agents_walk() {
    let mut e = engine();
    let wall = |yaw_deg: f32| {
        let h = yaw_deg.to_radians() / 2.0;
        format!(r#"{{"uid":1,"components":{{"Transform":{{"pos":[0,0,0],"rot":[0,{},0,{}]}},"Collider":{{"shape":1,"size":[4,2,0.5]}}}}}}"#, h.sin(), h.cos())
    };
    e.call("spawn", &j(&wall(30.0))).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("turned by an angle")), "nobody walks here: physics keeps the true box: {:?}", problems(&mut e));
    e.call("spawn", &j(r#"{"components":{"Transform":{"pos":[5,0,5]},"CharacterBody":{}}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|m| m.contains("1 long solid box Colliders") && m.contains("axis-aligned")), "{:?}", problems(&mut e));
    // a square trunk or rock turned any way is at most twice as big from above: not worth a warning (hundreds of scattered trees would drown the real ones)
    e.call("despawn", &j(r#"{"uid":1}"#)).unwrap();
    let h = 40.0_f32.to_radians() / 2.0;
    e.call("spawn", &j(&format!(r#"{{"components":{{"Transform":{{"rot":[0,{},0,{}]}},"Collider":{{"shape":1,"size":[0.7,3,0.7]}}}}}}"#, h.sin(), h.cos()))).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("turned by an angle")), "{:?}", problems(&mut e));
    // a quarter turn is its own axis-aligned box; so is a box that has no turn, and a trigger or a moving body is not a wall
    e.call("spawn", &j(&wall(90.0))).unwrap();
    e.call("spawn", &j(r#"{"components":{"Transform":{},"Collider":{"shape":1}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"components":{"Transform":{"rot":[0,0.26,0,0.97]},"Collider":{"shape":1,"trigger":true}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"components":{"Transform":{"rot":[0,0.26,0,0.97]},"Collider":{"shape":1},"Velocity":{},"RigidBody":{}}}"#)).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("turned by an angle")), "{:?}", problems(&mut e));
}

#[test]
fn check_knows_the_resources_the_engine_writes_for_languages_and_scenes() {
    let mut e = engine();
    e.call("define_script", &j(r#"{"source":"system next each Transform { $language.index = ($language.index + 1) % max($language.count, 1); $x = $scene.loaded; }"}"#)).unwrap();
    e.call("spawn", &j(r#"{"components":{"Transform":{}}}"#)).unwrap();
    assert!(problems(&mut e).iter().all(|m| !m.contains("language.") && !m.contains("scene.loaded")), "{:?}", problems(&mut e));
}

#[test]
fn profile_times_pictures_of_the_size_asked_for() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"components":{"Transform":{"pos":[0,0,5]},"Camera":{}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"components":{"Transform":{},"MeshRenderer":{"mesh":"cube"}}}"#)).unwrap();
    let p = e.call("profile", &j(r#"{"ticks":3,"render":true,"width":96,"height":54,"pictures":4}"#)).unwrap();
    assert!(p.get("render_first_ms").and_then(|v| v.as_f64()).is_some() && p.get("render_ms").and_then(|v| v.as_f64()).is_some(), "{p:?}");
    assert_eq!(p.get("render_size").and_then(|v| v.as_array()).map(|a| a.len()), Some(2));
    let one = e.call("profile", &j(r#"{"ticks":3,"render":true}"#)).unwrap();
    assert!(one.get("render_ms").is_some() && one.get("render_first_ms").is_none(), "{one:?}");
}
