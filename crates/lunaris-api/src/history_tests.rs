//! `history` lists the edits that `undo` and `redo` would undo and redo, newest first, with a label for each.
use crate::tests::engine;
use crate::Engine;
use lunaris_json::{parse, Json};

fn call(e: &mut Engine, tool: &str, args: &str) -> Json {
    e.call(tool, &parse(args).unwrap()).unwrap_or_else(|err| panic!("{tool} failed: {err}"))
}

fn labels(h: &Json, stack: &str) -> Vec<String> {
    h.get(stack).unwrap().as_array().unwrap().iter().map(|r| r.get("label").unwrap().as_str().unwrap().to_string()).collect()
}

#[test]
fn history_names_each_edit_newest_first_and_follows_undo_and_redo() {
    let mut e = engine();
    let h = call(&mut e, "history", "{}");
    assert!(labels(&h, "undo").is_empty() && labels(&h, "redo").is_empty());
    call(&mut e, "spawn", r#"{"uid":5,"components":{"Transform":{}}}"#);
    call(&mut e, "set", r#"{"uid":5,"component":"Transform","value":{"pos":[1,2,3]}}"#);
    call(&mut e, "add_component", r#"{"uid":5,"component":"Velocity","value":{}}"#);
    call(&mut e, "remove_component", r#"{"uid":5,"component":"Velocity"}"#);
    let h = call(&mut e, "history", "{}");
    assert_eq!(labels(&h, "undo"), ["Remove Velocity from 5", "Add Velocity to 5", "Edit Transform of 5", "Create entity 5"]);
    assert_eq!(h.get("undo_depth").unwrap().as_u64(), Some(4));
    // two undos: the two newest edits are now on the redo stack, the next redo being the one that was undone last
    call(&mut e, "undo", "{}");
    call(&mut e, "undo", "{}");
    let h = call(&mut e, "history", "{}");
    assert_eq!(labels(&h, "undo"), ["Edit Transform of 5", "Create entity 5"]);
    assert_eq!(labels(&h, "redo"), ["Add Velocity to 5", "Remove Velocity from 5"]);
    // a new edit empties the redo stack
    call(&mut e, "set", r#"{"uid":5,"component":"Transform","value":{"pos":[9,9,9]}}"#);
    let h = call(&mut e, "history", "{}");
    assert!(labels(&h, "redo").is_empty());
    assert_eq!(labels(&h, "undo")[0], "Edit Transform of 5");
    // despawn is an edit with a label too, and it is read-only: asking changes nothing
    call(&mut e, "despawn", r#"{"uid":5}"#);
    let rev = call(&mut e, "revision", "{}");
    let h = call(&mut e, "history", "{}");
    assert_eq!(labels(&h, "undo")[0], "Delete entity 5");
    assert_eq!(call(&mut e, "revision", "{}").get("revision"), rev.get("revision"), "history does not change the world");
}

#[test]
fn a_batch_is_one_edit_named_by_its_first_command() {
    let mut e = engine();
    call(&mut e, "batch", r#"{"commands":[{"op":"spawn","uid":1,"components":{"Transform":{}}},{"op":"spawn","uid":2,"components":{"Transform":{}}}]}"#);
    let h = call(&mut e, "history", "{}");
    let rows = h.get("undo").unwrap().as_array().unwrap();
    assert_eq!(rows.len(), 1, "one batch is one undo step");
    assert!(rows[0].get("label").unwrap().as_str().unwrap().contains("(and 1 more)"), "{rows:?}");
}
