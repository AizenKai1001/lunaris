//! `set` on the Transform of an attached entity says that the change will not stay (it is worked out from the LocalTransform every tick).
use crate::tests::engine;
use crate::Engine;
use lunaris_json::{parse, Json};

fn call(e: &mut Engine, tool: &str, args: &str) -> Json {
    e.call(tool, &parse(args).unwrap()).unwrap_or_else(|err| panic!("{tool} failed: {err}"))
}

fn pos_of(e: &mut Engine, uid: u64) -> Vec<f64> {
    let g = call(e, "get", &format!(r#"{{"uid":{uid},"components":["Transform"]}}"#));
    g.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

#[test]
fn a_transform_written_to_an_attached_entity_is_told_it_will_not_stay_and_is_in_fact_worked_out_again() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"uid":1,"components":{"Transform":{"pos":[10,0,0]}}}"#);
    call(&mut e, "spawn", r#"{"uid":2,"components":{"Transform":{},"Parent":{"target":1},"LocalTransform":{"pos":[1,0,0]}}}"#);
    call(&mut e, "spawn", r#"{"uid":3,"components":{"Transform":{}}}"#);
    call(&mut e, "step", r#"{"ticks":1}"#);
    assert_eq!(pos_of(&mut e, 2), [11.0, 0.0, 0.0]);
    let r = call(&mut e, "set", r#"{"uid":2,"component":"Transform","value":{"pos":[5,0,0]}}"#);
    let note = r.get("note").and_then(|n| n.as_str()).expect("a note");
    assert!(note.contains("LocalTransform"), "{note}");
    call(&mut e, "step", r#"{"ticks":1}"#);
    assert_eq!(pos_of(&mut e, 2), [11.0, 0.0, 0.0], "what the note says: the write did not stay");
    // the right way, and an entity that is not attached, say nothing
    let r = call(&mut e, "set", r#"{"uid":2,"component":"LocalTransform","value":{"pos":[2,0,0]}}"#);
    assert!(r.get("note").is_none());
    let r = call(&mut e, "set", r#"{"uid":3,"component":"Transform","value":{"pos":[5,0,0]}}"#);
    assert!(r.get("note").is_none(), "a free entity's Transform is the thing to set");
    call(&mut e, "step", r#"{"ticks":1}"#);
    assert_eq!(pos_of(&mut e, 2), [12.0, 0.0, 0.0]);
}
