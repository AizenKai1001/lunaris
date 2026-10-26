//! `check` names the mistakes of a helicopter: no body to fly, a rotor or a pilot that does not exist, a lift that cannot leave the ground; a good one says nothing.
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

const BODY: &str = r#""Transform":{"pos":[0,10,0]},"Collider":{"shape":1,"size":[2,2,6]},"RigidBody":{"mass":2000},"Velocity":{},"AngularVelocity":{}"#;

#[test]
fn a_good_helicopter_says_nothing() {
    let mut e = engine();
    spawn(&mut e, 7, r#"{"Transform":{}}"#);
    spawn(&mut e, 5, r#"{"Transform":{},"Heat":{}}"#);
    spawn(&mut e, 1, &format!(r#"{{{BODY},"Helicopter":{{"rotor":7,"pilot":5}}}}"#));
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("Helicopter")), "{m:?}");
}

#[test]
fn the_mistakes_of_a_helicopter_are_named() {
    let mut e = engine();
    spawn(&mut e, 1, r#"{"Transform":{},"Helicopter":{"rotor":99,"pilot":98}}"#);
    spawn(&mut e, 2, &format!(r#"{{{BODY},"Helicopter":{{"lift":3}}}}"#));
    spawn(&mut e, 3, &format!(r#"{{{BODY},"Helicopter":{{"spool_up":0}}}}"#));
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("needs Collider, RigidBody with a mass, Velocity, AngularVelocity")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Helicopter.rotor names entity 99")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Helicopter.pilot names entity 98")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Helicopter.lift is 3")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("lift and spool times above 0")), "{m:?}");
}
