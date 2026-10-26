//! `check` names the mistakes of a seat: no way in, a pilot's seat with no helicopter; a good one says nothing.
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
fn a_good_seat_says_nothing() {
    let mut e = engine();
    spawn(&mut e, 1, r#"{"Transform":{},"Collider":{"shape":1,"size":[1,1,1]},"Interactable":{"kind":0,"prompt":"Enter"},"Seat":{}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("Seat")), "{m:?}");
}

#[test]
fn the_mistakes_of_a_seat_are_named() {
    let mut e = engine();
    spawn(&mut e, 1, r#"{"Transform":{},"Seat":{}}"#);
    spawn(&mut e, 2, r#"{"Transform":{},"Interactable":{"kind":1},"Seat":{}}"#);
    spawn(&mut e, 3, r#"{"Transform":{},"Interactable":{"kind":0},"Seat":{"drive":1}}"#);
    spawn(&mut e, 4, r#"{"Transform":{},"Interactable":{"kind":0},"Seat":{"drive":7}}"#);
    spawn(&mut e, 5, r#"{"Transform":{},"Interactable":{"kind":0},"Seat":{"drive":2}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("a Seat has no way in")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Interactable is of kind 1")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Seat.drive 1 flies a Helicopter")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Seat.drive is 7")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Seat.drive 2 drives a Vehicle")), "{m:?}");
}
