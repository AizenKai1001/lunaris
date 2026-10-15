//! The lighting effects of the GPU renderer as an agent meets them: the components exist and carry their documentation, `check` finds the mistakes, and the reference renderer ignores them.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn problems(e: &mut crate::Engine) -> Vec<String> {
    let c = e.call("check", &Json::Null).unwrap();
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| p.get("message").unwrap().as_str().unwrap().to_string()).collect()
}

fn cube(e: &mut crate::Engine) {
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{"pos":[0,0,0]},"MeshRenderer":{"mesh":"cube"}}}"#)).unwrap();
}

#[test]
fn auto_exposure_is_a_component_with_documented_defaults_and_the_reference_renderer_ignores_it() {
    let mut e = engine();
    cube(&mut e);
    let picture = |e: &mut crate::Engine| e.call("render", &j(r#"{"width":64,"height":48,"image":false,"quality":"reference"}"#)).unwrap().get("hash").unwrap().as_str().unwrap().to_string();
    let before = picture(&mut e);
    e.call("spawn", &j(r#"{"uid":2,"components":{"AutoExposure":{}}}"#)).unwrap();
    assert_eq!(picture(&mut e), before, "the reference renderer draws the same picture with or without eye adaptation");
    let got = e.call("get", &j(r#"{"uid":2,"components":["AutoExposure"]}"#)).unwrap();
    let text = got.to_string_lossy();
    assert!(text.contains("0.18") && text.contains("16") && text.contains("1.5"), "{text}");
    let schema = e.call("schema", &j(r#"{"component":"AutoExposure"}"#)).unwrap().to_string_lossy();
    assert!(schema.contains("Eye adaptation"), "the schema tool shows the component's documentation: {schema}");
}

#[test]
fn check_finds_the_mistakes_of_auto_exposure() {
    let mut e = engine();
    // a flat scene has nothing to measure
    e.call("spawn", &j(r#"{"uid":2,"components":{"AutoExposure":{}}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|p| p.contains("no 3D meshes")), "{:?}", problems(&mut e));
    cube(&mut e);
    assert!(!problems(&mut e).iter().any(|p| p.contains("AutoExposure")), "{:?}", problems(&mut e));
    // numbers that mean nothing, and a second one
    e.call("set", &j(r#"{"uid":2,"component":"AutoExposure","value":{"min":20}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|p| p.contains("min 20") && p.contains("above max")), "{:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":2,"component":"AutoExposure","value":{"min":0.05}}"#)).unwrap();
    e.call("set", &j(r#"{"uid":2,"component":"AutoExposure","value":{"speed":0}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|p| p.contains("speed") && p.contains("never moves")), "{:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":2,"component":"AutoExposure","value":{"speed":2}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"AutoExposure":{}}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|p| p.contains("second AutoExposure")), "{:?}", problems(&mut e));
}

#[test]
fn check_finds_the_mistakes_of_ssr() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":2,"components":{"Ssr":{}}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|p| p.contains("Ssr has no 3D meshes")), "{:?}", problems(&mut e));
    // a dull ball: nothing in the world can reflect
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{"pos":[0,0,0]},"MeshRenderer":{"mesh":"sphere","roughness":0.95}}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|p| p.contains("no mesh is smooth enough")), "{:?}", problems(&mut e));
    // a polished one, or water, is enough
    e.call("set", &j(r#"{"uid":1,"component":"MeshRenderer","value":{"roughness":0.1,"metallic":1}}"#)).unwrap();
    assert!(!problems(&mut e).iter().any(|p| p.contains("Ssr")), "{:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":2,"component":"Ssr","value":{"strength":0}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|p| p.contains("strength") && p.contains("no reflections")), "{:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":2,"component":"Ssr","value":{"strength":1,"max_distance":-3}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|p| p.contains("max_distance")), "{:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":2,"component":"Ssr","value":{"max_distance":40}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Ssr":{}}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|p| p.contains("second Ssr")), "{:?}", problems(&mut e));
}

#[test]
fn check_finds_the_mistakes_of_lens_flare() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":2,"components":{"LensFlare":{}}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|p| p.contains("LensFlare has no 3D meshes")), "{:?}", problems(&mut e));
    cube(&mut e);
    assert!(problems(&mut e).iter().any(|p| p.contains("needs the sun")), "{:?}", problems(&mut e));
    e.call("spawn", &j(r#"{"uid":3,"components":{"Light":{"kind":0},"Transform":{}}}"#)).unwrap();
    assert!(!problems(&mut e).iter().any(|p| p.contains("LensFlare")), "{:?}", problems(&mut e));
    e.call("set", &j(r#"{"uid":2,"component":"LensFlare","value":{"ghosts":12,"intensity":0}}"#)).unwrap();
    let all = problems(&mut e);
    assert!(all.iter().any(|p| p.contains("ghosts 12")) && all.iter().any(|p| p.contains("intensity 0")), "{all:?}");
    e.call("spawn", &j(r#"{"uid":4,"components":{"LensFlare":{}}}"#)).unwrap();
    assert!(problems(&mut e).iter().any(|p| p.contains("second LensFlare")), "{:?}", problems(&mut e));
}

#[test]
fn check_finds_the_mistakes_of_reflection_probes_and_bake_probe_needs_a_probe_and_the_gpu() {
    let mut e = engine();
    assert_eq!(e.call("bake_probe", &j("{}")).unwrap_err().code, "no_probe");
    // a probe with no place, an empty box and a silly size
    e.call("spawn", &j(r#"{"uid":5,"components":{"ReflectionProbe":{"size":[0,4,4],"resolution":4}}}"#)).unwrap();
    let all = problems(&mut e);
    assert!(all.iter().any(|p| p.contains("no Transform")), "{all:?}");
    assert!(all.iter().any(|p| p.contains("not three numbers above 0")), "{all:?}");
    assert!(all.iter().any(|p| p.contains("resolution 4") && p.contains("counts as 16")), "{all:?}");
    assert!(all.iter().any(|p| p.contains("ReflectionProbe has no 3D meshes")), "{all:?}");
    e.call("add_component", &j(r#"{"uid":5,"component":"Transform"}"#)).unwrap();
    e.call("set", &j(r#"{"uid":5,"component":"ReflectionProbe","value":{"size":[10,5,10],"resolution":128}}"#)).unwrap();
    cube(&mut e);
    assert!(!problems(&mut e).iter().any(|p| p.contains("ReflectionProbe")), "{:?}", problems(&mut e));
    // the tool: a probe that does not exist, and no GPU renderer on this engine
    assert_eq!(e.call("bake_probe", &j(r#"{"uid":9}"#)).unwrap_err().code, "no_such_probe");
    assert_eq!(e.call("bake_probe", &j("{}")).unwrap_err().code, "no_gpu");
    // more than four: only the first four work
    for uid in 6..10 {
        e.call("spawn", &j(&format!(r#"{{"uid":{uid},"components":{{"Transform":{{}},"ReflectionProbe":{{}}}}}}"#))).unwrap();
    }
    assert!(problems(&mut e).iter().any(|p| p.contains("a fifth ReflectionProbe")), "{:?}", problems(&mut e));
}

#[test]
fn ssr_is_a_component_the_reference_renderer_ignores() {
    let mut e = engine();
    cube(&mut e);
    let picture = |e: &mut crate::Engine| e.call("render", &j(r#"{"width":64,"height":48,"image":false,"quality":"reference"}"#)).unwrap().get("hash").unwrap().as_str().unwrap().to_string();
    let before = picture(&mut e);
    e.call("spawn", &j(r#"{"uid":2,"components":{"Ssr":{}}}"#)).unwrap();
    assert_eq!(picture(&mut e), before);
    let schema = e.call("schema", &j(r#"{"component":"Ssr"}"#)).unwrap().to_string_lossy();
    assert!(schema.contains("Screen-space reflections") && schema.contains("max_distance"), "{schema}");
}
