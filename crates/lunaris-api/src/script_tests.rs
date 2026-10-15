//! The three script tools: what an agent does to write gameplay as text instead of JSON.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn text(source: &str) -> Json {
    Json::obj([("source", Json::str(source))])
}

const GRAVITY: &str = "/// Fall.\nsystem fall each Transform, Velocity {\n    Velocity.lin[1] -= 9.8 * dt;\n    if Transform.pos[1] < 0 {\n        Transform.pos[1] = 0;\n    }\n}\n";

#[test]
fn compile_script_checks_without_touching_the_world_and_reports_where() {
    let mut e = engine();
    let rev = e.call("revision", &Json::Null).unwrap();
    let ok = e.call("compile_script", &text(GRAVITY)).unwrap();
    assert_eq!(ok.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(ok.get("systems").unwrap().index(0).unwrap().get("name").unwrap().as_str(), Some("fall"));
    assert!(e.call("get_system", &j(r#"{"name":"fall"}"#)).is_err(), "compiling defines nothing");
    let bad = e.call("compile_script", &text("system s each Transform {\n    Transform.pos[0] = ;\n}")).unwrap();
    assert_eq!(bad.get("ok").and_then(|v| v.as_bool()), Some(false));
    let first = bad.get("errors").unwrap().index(0).unwrap();
    assert_eq!(first.get("line").unwrap().as_u64(), Some(2));
    assert!(first.get("message").unwrap().as_str().unwrap().contains("expected a value"));
    // reading changes nothing: the revision is the same
    assert_eq!(e.call("revision", &Json::Null).unwrap().get("revision"), rev.get("revision"));
}

#[test]
fn define_script_defines_every_system_as_one_undo_step_and_runs_them() {
    let mut e = engine();
    let src = format!("{GRAVITY}\nsystem drift each Transform {{\n    Transform.pos[0] += 1.0 * dt;\n}}\n");
    let r = e.call("define_script", &text(&src)).unwrap();
    assert_eq!(r.get("systems").unwrap().as_array().unwrap().len(), 2);
    let names: Vec<String> = e.call("systems", &Json::Null).unwrap().get("graph").unwrap().as_array().unwrap().iter().map(|s| s.get("name").unwrap().as_str().unwrap().to_string()).collect();
    assert!(names.contains(&"fall".to_string()) && names.contains(&"drift".to_string()), "{names:?}");
    // the JSON of the system is exactly what the script says
    let def = e.call("get_system", &j(r#"{"name":"fall"}"#)).unwrap();
    let def = def.get("def").unwrap();
    assert_eq!(def.get("doc").unwrap().as_str(), Some("Fall."));
    // one undo takes both away
    e.call("undo", &Json::Null).unwrap();
    assert!(e.call("get_system", &j(r#"{"name":"fall"}"#)).is_err() && e.call("get_system", &j(r#"{"name":"drift"}"#)).is_err());
}

#[test]
fn a_script_the_world_refuses_is_all_or_nothing_and_the_error_has_its_line() {
    let mut e = engine();
    let src = "system fine each Transform {\n    Transform.pos[0] = 1;\n}\nsystem wrong each Transform {\n    Transform.pos[0] = 1;\n    Transform.nope[0] = 2;\n}\n";
    let err = e.call("define_script", &text(src)).unwrap_err();
    assert!(err.message.starts_with("6:"), "{}", err.message);
    assert!(err.message.contains("nope"), "{}", err.message);
    assert!(e.call("get_system", &j(r#"{"name":"fine"}"#)).is_err(), "the first system was rolled back");
    // a script that does not even read
    let err = e.call("define_script", &text("system x each Transform { ) }")).unwrap_err();
    assert_eq!(err.code, "bad_script");
    assert!(err.hint.as_deref().unwrap_or("").contains("1:"), "{err:?}");
    // the redo stack is not polluted by the rollback
    assert!(e.call("redo", &Json::Null).is_err());
}

#[test]
fn get_script_gives_back_text_that_defines_the_same_system() {
    let mut e = engine();
    e.call("define_system", &j(r#"{"name":"spin","each":["Transform"],"do":[{"set":"Transform.rot[1]","to":{"add":[{"get":"Transform.rot[1]"},{"mul":[0.5,{"dt":true}]}]}}]}"#)).unwrap();
    let s = e.call("get_script", &j(r#"{"name":"spin"}"#)).unwrap();
    let source = s.get("source").unwrap().as_str().unwrap().to_string();
    assert!(source.contains("system spin each Transform"), "{source}");
    let before = e.call("get_system", &j(r#"{"name":"spin"}"#)).unwrap().get("def").unwrap().clone();
    e.call("remove_system", &j(r#"{"name":"spin"}"#)).unwrap();
    e.call("define_script", &text(&source)).unwrap();
    let after = e.call("get_system", &j(r#"{"name":"spin"}"#)).unwrap().get("def").unwrap().clone();
    assert_eq!(before.to_string_compact().unwrap().replace(",\"enabled\":true", ""), after.to_string_compact().unwrap().replace(",\"enabled\":true", ""));
    assert!(e.call("get_script", &j(r#"{"name":"nope"}"#)).unwrap_err().code == "no_such_system");
}
