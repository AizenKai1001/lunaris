//! `check` and the picture a `CameraView` draws: `view:UID` is a texture (the monitor of docs/GAMEPLAY.md), and one that names nothing that draws a view is told.
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
fn a_view_texture_is_not_a_missing_file_and_one_that_no_camera_view_draws_is_named() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"uid":1,"components":{"Transform":{},"Camera":{"active":true}}}"#);
    call(&mut e, "spawn", r#"{"uid":9,"components":{"Transform":{"pos":[0,5,0]},"Camera":{"active":false},"CameraView":{"camera":9}}}"#);
    call(&mut e, "spawn", r#"{"uid":10,"components":{"Transform":{"pos":[3,0,-5]},"MeshRenderer":{"mesh":"quad","texture":"view:9","emissive":0.9}}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("view:9")), "the documented monitor is fine: {m:?}");
    call(&mut e, "spawn", r#"{"uid":11,"components":{"Transform":{"pos":[6,0,-5]},"MeshRenderer":{"mesh":"quad","texture":"view:77","emissive":0.9}}}"#);
    let m = messages(&mut e);
    assert_eq!(m.iter().filter(|x| x.contains("view:77")).count(), 1, "{m:?}");
    assert!(m.iter().any(|x| x.starts_with("warning") && x.contains("view:77") && x.contains("CameraView")), "{m:?}");
    assert!(!m.iter().any(|x| x.starts_with("error") && x.contains("view:")), "a view texture is never reported as a missing file: {m:?}");
}
