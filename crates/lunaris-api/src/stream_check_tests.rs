//! `check` says what a streamed world needs: a limit on the zones loaded in a tick, and a procedural grid that does not look at thousands of cells round every anchor. A small world, or one that has both, hears nothing.
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

fn zones(e: &mut Engine, from: u64, n: u64) {
    call(e, "define_prefab", r#"{"name":"block","components":{"Transform":{}}}"#);
    for i in from..from + n {
        call(e, "spawn", &format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{},0,0]}},"StreamZone":{{"prefab":"block","radius":50}}}}}}"#, 1000 + i, i * 100));
    }
}

#[test]
fn many_zones_or_a_grid_with_no_limit_on_the_loads_are_told_so_and_a_limit_quiets_it() {
    let mut e = engine();
    zones(&mut e, 0, 5);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("stream.loads_per_tick")), "a few zones are nobody's problem: {m:?}");
    zones(&mut e, 5, 30);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("StreamZones with no limit")), "{m:?}");
    call(&mut e, "set_resource", r#"{"name":"stream.loads_per_tick","value":3}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("stream.loads_per_tick")), "{m:?}");
    // a grid with no zones yet is told too, until the limit is there
    let mut g = engine();
    call(&mut g, "spawn", r#"{"uid":5,"components":{"ProcGrid":{"grid":"city","cell":100,"radius":300}}}"#);
    let m = messages(&mut g);
    assert!(m.iter().any(|x| x.contains("a ProcGrid with no limit")), "{m:?}");
}

#[test]
fn a_grid_that_looks_at_thousands_of_cells_round_an_anchor_is_told_so() {
    let mut e = engine();
    call(&mut e, "set_resource", r#"{"name":"stream.loads_per_tick","value":3}"#);
    call(&mut e, "spawn", r#"{"uid":5,"components":{"ProcGrid":{"grid":"far","cell":100,"radius":300}}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("cells round every anchor")), "{m:?}");
    call(&mut e, "spawn", r#"{"uid":6,"components":{"ProcGrid":{"grid":"huge","cell":10,"radius":3000}}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("cells round every anchor") && x.contains("radius 3000")), "{m:?}");
}

#[test]
fn meshes_with_no_light_and_no_environment_are_told_they_are_flat_and_either_one_quiets_it() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[0,1,5]},"Camera":{}}}"#);
    call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[0,1,0]},"MeshRenderer":{"mesh":"cube"}}}"#);
    assert!(!messages(&mut e).iter().any(|x| x.contains("no Light or Environment")), "a lone mesh is not asked to have a light");
    call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[3,1,0]},"MeshRenderer":{"mesh":"sphere"}}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("no Light or Environment")), "{m:?}");
    call(&mut e, "spawn", r#"{"components":{"Transform":{},"Light":{"kind":0}}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("no Light or Environment")), "a light quiets it: {m:?}");
    // an Environment alone does too (the sky lights the surfaces in the GPU qualities), and a 2D scene (it has a sprite) needs neither
    let mut sky = engine();
    call(&mut sky, "spawn", r#"{"components":{"Transform":{"pos":[0,1,0]},"MeshRenderer":{"mesh":"cube"}}}"#);
    call(&mut sky, "spawn", r#"{"components":{"Transform":{"pos":[3,1,0]},"MeshRenderer":{"mesh":"cube"}}}"#);
    call(&mut sky, "spawn", r#"{"components":{"Environment":{"sky":true}}}"#);
    assert!(!messages(&mut sky).iter().any(|x| x.contains("no Light or Environment")));
    let mut empty = engine();
    for x in [0, 3, 6] {
        call(&mut empty, "spawn", &format!(r#"{{"components":{{"Transform":{{"pos":[{x},0,0]}},"MeshRenderer":{{"mesh":"cube"}}}}}}"#));
    }
    call(&mut empty, "spawn", r#"{"components":{"Transform":{},"Sprite":{}}}"#);
    assert!(!messages(&mut empty).iter().any(|x| x.contains("no Light or Environment")));
}

#[test]
fn a_mesh_with_a_zero_in_its_scale_is_told_it_is_not_drawn() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"components":{"Transform":{"scale":[1,0,1]},"MeshRenderer":{"mesh":"cube"}}}"#);
    call(&mut e, "spawn", r#"{"components":{"Transform":{"scale":[2,2,2]},"MeshRenderer":{"mesh":"cube"}}}"#);
    call(&mut e, "spawn", r#"{"components":{"Transform":{"scale":[1,0,1]}}}"#);
    let m = messages(&mut e);
    let zero: Vec<&String> = m.iter().filter(|x| x.contains("has a 0")).collect();
    assert_eq!(zero.len(), 1, "only the mesh that has a zero: {m:?}");
    assert!(zero[0].contains("not drawn"));
}

#[test]
fn a_camera_that_looks_away_from_every_mesh_is_told_so_and_turning_it_quiets_it() {
    let mut e = engine();
    // the camera at z = 5 looks along -Z by default: a cube at the origin is in front of it
    call(&mut e, "spawn", r#"{"uid":1,"components":{"Transform":{"pos":[0,1,5]},"Camera":{}}}"#);
    call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[0,1,0]},"MeshRenderer":{"mesh":"cube"}}}"#);
    assert!(!messages(&mut e).iter().any(|x| x.contains("looks along its local -Z")));
    // a cube behind it: the camera at z = -5 still looks along -Z
    call(&mut e, "set", r#"{"uid":1,"component":"Transform","value":{"pos":[0,1,-5]}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("looks along its local -Z")), "{m:?}");
    // turned half a way round (a quarter turn each way of y is a half turn together: rot = (0, 1, 0, 0)) it sees the cube
    call(&mut e, "set", r#"{"uid":1,"component":"Transform","value":{"rot":[0,1,0,0]}}"#);
    assert!(!messages(&mut e).iter().any(|x| x.contains("looks along its local -Z")), "turned to face the cube");
}

#[test]
fn a_scene_of_hundreds_of_thousands_of_triangles_gets_advice_about_the_cpu_renderer_and_a_gpu_quiets_it() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"count":2000,"components":{"Transform":{},"MeshRenderer":{"mesh":"cube"}}}"#);
    assert!(!messages(&mut e).iter().any(|x| x.contains("triangles are drawn")), "24,000 triangles are nothing");
    call(&mut e, "spawn", r#"{"count":30000,"components":{"Transform":{},"MeshRenderer":{"mesh":"cube"}}}"#);
    let m = messages(&mut e);
    let advice: Vec<&String> = m.iter().filter(|x| x.contains("triangles are drawn")).collect();
    assert_eq!(advice.len(), 1, "{m:?}");
    assert!(advice[0].starts_with("info:") && advice[0].contains("384000") && advice[0].contains("no graphics card"), "{advice:?}");
    // meshes that are hidden are not drawn
    let mut hidden = engine();
    call(&mut hidden, "spawn", r#"{"count":30000,"components":{"Transform":{},"MeshRenderer":{"mesh":"cube","visible":false}}}"#);
    assert!(!messages(&mut hidden).iter().any(|x| x.contains("triangles are drawn")));
}
