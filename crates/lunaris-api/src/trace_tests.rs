//! `trace_system`: what a graph system's expressions came to for one entity, without touching the world (the editor shows these as live values on the wires).
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn spawn(e: &mut crate::Engine, components: &str) -> u64 {
    e.call("spawn", &j(&format!(r#"{{"components":{components}}}"#))).unwrap().get("uid").unwrap().as_u64().unwrap()
}

const FALL: &str = r#"{"name":"fall","each":["Transform","Velocity"],"do":[
    {"let":"g","be":{"mul":[9.8,{"dt":true}]}},
    {"set":"Velocity.lin[1]","to":{"sub":[{"get":"Velocity.lin[1]"},{"var":"g"}]}},
    {"if":{"lt":[{"get":"Transform.pos[1]"},0]},"then":[{"set":"Transform.pos[1]","to":0}]}]}"#;

fn value(t: &Json, path: &str) -> Option<f64> {
    t.get("values").and_then(|v| v.get(path)).and_then(|v| v.as_f64())
}

#[test]
fn a_trace_gives_the_value_of_every_expression_by_its_path_and_changes_nothing() {
    let mut e = engine();
    let uid = spawn(&mut e, r#"{"Transform":{"pos":[0,5,0]},"Velocity":{"lin":[1,2,3]}}"#);
    e.call("define_system", &j(FALL)).unwrap();
    let hash = e.call("hash", &Json::Null).unwrap();
    let rev = e.call("revision", &Json::Null).unwrap();
    let t = e.call("trace_system", &j(&format!(r#"{{"name":"fall","uid":{uid}}}"#))).unwrap();
    assert_eq!(t.get("matched").and_then(|v| v.as_bool()), Some(true));
    // dt is 1/60: g = 9.8/60
    // dt is an f32 in the engine: 1/60 to seven digits
    let g = 9.8 / 60.0;
    assert!((value(&t, "do[0].be").unwrap() - g).abs() < 1e-6, "{t:?}");
    assert!((value(&t, "do[0].be.mul[1]").unwrap() - 1.0 / 60.0).abs() < 1e-6, "dt, a part of the multiplication");
    assert_eq!(value(&t, "do[1].to.sub[0]"), Some(2.0), "Velocity.lin[1] before the statement runs");
    assert!((value(&t, "do[1].to").unwrap() - (2.0 - g)).abs() < 1e-6);
    assert!((value(&t, "do[1].to.sub[1]").unwrap() - g).abs() < 1e-6, "the variable");
    assert_eq!(t.get("values").unwrap().get("do[2].if").and_then(|v| v.as_bool()), Some(false), "5 is not below 0");
    assert_eq!(value(&t, "do[2].if.lt[0]"), Some(5.0));
    // a constant is not an expression worth showing
    assert!(t.get("values").unwrap().get("do[0].be.mul[0]").is_none());
    assert!(t.get("error").is_none());
    // the world is exactly as it was: same hash, same revision, and the velocity did not change
    assert_eq!(e.call("hash", &Json::Null).unwrap(), hash);
    assert_eq!(e.call("revision", &Json::Null).unwrap().get("revision"), rev.get("revision"));
    let v = e.call("get", &j(&format!(r#"{{"uid":{uid},"components":["Velocity"]}}"#))).unwrap();
    assert_eq!(v.get("components").unwrap().get("Velocity").unwrap().get("lin").unwrap().index(1).unwrap().as_f64(), Some(2.0));
    // the same trace again says the same (no hidden state)
    let again = e.call("trace_system", &j(&format!(r#"{{"name":"fall","uid":{uid}}}"#))).unwrap();
    assert_eq!(again.to_string_compact().unwrap(), t.to_string_compact().unwrap());
}

#[test]
fn an_entity_the_system_does_not_visit_has_nothing_to_show_and_a_definition_can_be_traced_before_it_is_defined() {
    let mut e = engine();
    let plain = spawn(&mut e, r#"{"Transform":{"pos":[0,5,0]}}"#);
    let moving = spawn(&mut e, r#"{"Transform":{"pos":[0,-1,0]},"Velocity":{"lin":[0,0,0]}}"#);
    e.call("define_system", &j(FALL)).unwrap();
    let t = e.call("trace_system", &j(&format!(r#"{{"name":"fall","uid":{plain}}}"#))).unwrap();
    assert_eq!(t.get("matched").and_then(|v| v.as_bool()), Some(false), "no Velocity: the system never visits it");
    assert!(t.get("values").unwrap().as_object().unwrap().is_empty());
    // below zero: the branch condition is true
    let t = e.call("trace_system", &j(&format!(r#"{{"name":"fall","uid":{moving}}}"#))).unwrap();
    assert_eq!(t.get("values").unwrap().get("do[2].if").and_then(|v| v.as_bool()), Some(true));
    // a definition that is not defined in the world
    let def = r#"{"name":"later","each":["Transform"],"do":[{"set":"Transform.pos[0]","to":{"add":[{"get":"Transform.pos[1]"},10]}}]}"#;
    let t = e.call("trace_system", &j(&format!(r#"{{"def":{def},"uid":{plain}}}"#))).unwrap();
    assert_eq!(value(&t, "do[0].to"), Some(15.0));
    assert!(e.call("get_system", &j(r#"{"name":"later"}"#)).is_err(), "tracing defines nothing");
    // errors of the request itself
    assert_eq!(e.call("trace_system", &j(r#"{"name":"nope","uid":1}"#)).unwrap_err().code, "no_such_system");
    assert_eq!(e.call("trace_system", &j(r#"{"name":"fall","uid":9999}"#)).unwrap_err().code, "no_such_entity");
}

#[test]
fn a_run_that_fails_reports_the_error_with_the_values_found_before_it() {
    let mut e = engine();
    let uid = spawn(&mut e, r#"{"Transform":{"pos":[0,5,0]},"Velocity":{"lin":[1,2,0]}}"#);
    e.call("define_system", &j(r#"{"name":"inverse","each":["Velocity"],"do":[{"let":"a","be":{"get":"Velocity.lin[0]"}},{"set":"Velocity.lin[1]","to":{"div":[1,{"get":"Velocity.lin[2]"}]}}]}"#)).unwrap();
    let t = e.call("trace_system", &j(&format!(r#"{{"name":"inverse","uid":{uid}}}"#))).unwrap();
    assert_eq!(t.get("matched").and_then(|v| v.as_bool()), Some(true));
    assert!(t.get("error").unwrap().as_str().unwrap().contains("division by zero"), "{t:?}");
    assert_eq!(value(&t, "do[0].be"), Some(1.0), "the statement before the failure");
    assert_eq!(value(&t, "do[1].to.div[1]"), Some(0.0), "the divisor that caused it");
    assert!(t.get("values").unwrap().get("do[1].to").is_none(), "the division itself never finished");
}

#[test]
fn timeline_seek_puts_a_timeline_at_a_time_and_says_how_long_it_is() {
    let mut e = engine();
    let uid = spawn(&mut e, r#"{"Transform":{"pos":[0,0,0]},"Timeline":{"script":"at 0 ramp Transform.pos[1] 0 -> 10 over 2\nat 3 end"}}"#);
    let r = e.call("timeline_seek", &j(&format!(r#"{{"uid":{uid},"time":1}}"#))).unwrap();
    assert_eq!(r.get("length").and_then(|v| v.as_f64()), Some(3.0));
    assert_eq!(r.get("time").and_then(|v| v.as_f64()), Some(1.0));
    assert_eq!(r.get("written").and_then(|v| v.as_u64()), Some(1));
    let t = e.call("get", &j(&format!(r#"{{"uid":{uid},"components":["Transform"]}}"#))).unwrap();
    assert_eq!(t.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().index(1).unwrap().as_f64(), Some(5.0), "half way up");
    // past the end the time is clamped and the ramp is at its last value
    let r = e.call("timeline_seek", &j(&format!(r#"{{"uid":{uid},"time":50}}"#))).unwrap();
    assert_eq!(r.get("time").and_then(|v| v.as_f64()), Some(3.0));
    // refusals: no such entity, no timeline, no time
    assert_eq!(e.call("timeline_seek", &j(r#"{"uid":9999,"time":1}"#)).unwrap_err().code, "no_such_entity");
    let bare = spawn(&mut e, r#"{"Transform":{}}"#);
    assert_eq!(e.call("timeline_seek", &j(&format!(r#"{{"uid":{bare},"time":1}}"#))).unwrap_err().code, "bad_timeline");
    assert_eq!(e.call("timeline_seek", &j(&format!(r#"{{"uid":{uid}}}"#))).unwrap_err().code, "missing_param");
}

#[test]
fn a_trace_says_which_statements_ran_so_the_way_of_an_if_is_known() {
    let mut e = engine();
    let low = spawn(&mut e, r#"{"Transform":{"pos":[0,-1,0]},"Velocity":{"lin":[0,0,0]}}"#);
    let high = spawn(&mut e, r#"{"Transform":{"pos":[0,5,0]},"Velocity":{"lin":[0,0,0]}}"#);
    e.call("define_system", &j(FALL)).unwrap();
    let ran = |e: &mut crate::Engine, uid: u64| -> Vec<String> {
        let t = e.call("trace_system", &j(&format!(r#"{{"name":"fall","uid":{uid}}}"#))).unwrap();
        t.get("ran").unwrap().as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect()
    };
    // below zero the `then` branch runs (do[2].then[0]); above it only the three top-level statements do
    assert_eq!(ran(&mut e, low), ["do[0]", "do[1]", "do[2]", "do[2].then[0]"]);
    assert_eq!(ran(&mut e, high), ["do[0]", "do[1]", "do[2]"]);
    // an entity that is not visited ran nothing
    let plain = spawn(&mut e, r#"{"Transform":{}}"#);
    assert!(ran(&mut e, plain).is_empty());
}
