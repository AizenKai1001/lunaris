//! `check` names the mistakes of a flying machine: no body to move, a mode that is not one, a target or a patrol that is not there, a perception to follow that is missing; a good drone says nothing.
use crate::tests::engine;
use crate::Engine;
use lunaris_json::{parse, Json};

fn call(e: &mut Engine, tool: &str, args: &str) -> Json {
    e.call(tool, &parse(args).unwrap()).unwrap_or_else(|err| panic!("{tool} failed: {err}"))
}

fn spawn(e: &mut Engine, uid: u64, components: &str) {
    call(e, "spawn", &format!(r#"{{"uid":{uid},"components":{components}}}"#));
}

fn messages(e: &mut Engine) -> Vec<String> {
    let c = call(e, "check", "{}");
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| format!("{}: {}", p.get("severity").unwrap().as_str().unwrap(), p.get("message").unwrap().as_str().unwrap())).collect()
}

#[test]
fn a_good_drone_says_nothing() {
    let mut e = engine();
    spawn(&mut e, 5, r#"{"Transform":{}}"#);
    spawn(&mut e, 1, r#"{"Transform":{},"Velocity":{},"FlyAgent":{"mode":2,"target":5}}"#);
    spawn(&mut e, 2, r#"{"Transform":{},"Velocity":{},"Perception":{},"FlyAgent":{"perception":true,"patrol":"0 0 0\n5 0 5\n"}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("FlyAgent")), "{m:?}");
}

#[test]
fn the_mistakes_of_a_drone_are_named() {
    let mut e = engine();
    spawn(&mut e, 1, r#"{"FlyAgent":{"mode":9}}"#);
    spawn(&mut e, 2, r#"{"Transform":{},"Velocity":{},"FlyAgent":{"mode":3,"target":99}}"#);
    spawn(&mut e, 3, r#"{"Transform":{},"Velocity":{},"FlyAgent":{"mode":4,"patrol":"nowhere"}}"#);
    spawn(&mut e, 4, r#"{"Transform":{},"Velocity":{},"FlyAgent":{"perception":true,"speed":0}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("needs a Transform and a Velocity")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("FlyAgent.mode 9")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("FlyAgent.target 99")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("FlyAgent.patrol has no point")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("no Perception to follow")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("speed and an accel above 0")), "{m:?}");
}
