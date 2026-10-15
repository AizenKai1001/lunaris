//! The numbers of the boxes `tilemap_colliders` makes: where they start (`first_uid`) and what the error says when a builder clashes with them.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn map(e: &mut crate::Engine) {
    // 4 x 2 tiles, the bottom row solid: one box
    e.call("spawn", &j(r#"{"uid":1,"components":{"Transform":{},"Tilemap":{"size":[4,2],"tile":1.0,"data":"0 0 0 0, 1 1 1 1"}}}"#)).unwrap();
}

/// The uids of every entity with a Collider (the boxes), in order.
fn boxes(e: &mut crate::Engine) -> Vec<u64> {
    let found = e.call("query", &j(r#"{"with":["Collider"]}"#)).unwrap();
    let mut v: Vec<u64> = found.get("entities").unwrap().as_array().unwrap().iter().map(|x| x.get("uid").unwrap().as_u64().unwrap()).collect();
    v.sort_unstable();
    v
}

fn hash(e: &mut crate::Engine) -> String {
    e.call("hash", &Json::Null).unwrap().get("hash").unwrap().as_str().unwrap().to_string()
}

#[test]
fn the_boxes_start_where_first_uid_says_and_a_builder_can_keep_clear_of_them() {
    let mut e = engine();
    map(&mut e);
    // two boxes: a map with a gap
    e.call("set", &j(r#"{"uid":1,"component":"Tilemap","value":{"data":"0 0 0 0, 1 -1 1 1"}}"#)).unwrap();
    let r = e.call("tilemap_colliders", &j(r#"{"uid":1,"first_uid":1000}"#)).unwrap();
    assert_eq!((r.get("boxes").and_then(|b| b.as_u64()), r.get("first_uid").and_then(|b| b.as_u64()), r.get("last_uid").and_then(|b| b.as_u64())), (Some(2), Some(1000), Some(1001)));
    assert_eq!(boxes(&mut e), vec![1000, 1001]);
    // the builder then spawns entities with uids of its own, below, with no clash
    for uid in [20, 21, 30] {
        e.call("spawn", &j(&format!(r#"{{"uid":{uid},"components":{{"Transform":{{}}}}}}"#))).unwrap();
    }
    // again: the old boxes go first, so the same numbers are free for the new ones
    let again = e.call("tilemap_colliders", &j(r#"{"uid":1,"first_uid":1000}"#)).unwrap();
    assert_eq!((again.get("removed").and_then(|b| b.as_u64()), again.get("first_uid").and_then(|b| b.as_u64())), (Some(2), Some(1000)));
    assert_eq!(boxes(&mut e), vec![1000, 1001]);
    // a number that is taken refuses the whole call, and nothing is changed
    let before = hash(&mut e);
    let refused = e.call("tilemap_colliders", &j(r#"{"uid":1,"first_uid":30}"#)).unwrap_err();
    assert_eq!(refused.code, "duplicate_uid", "{refused:?}");
    assert!(refused.message.contains("30") && refused.message.contains("2 boxes"), "{refused:?}");
    assert!(refused.hint.as_deref().is_some_and(|h| h.contains("first_uid")), "{refused:?}");
    assert_eq!(hash(&mut e), before, "the old boxes were not removed either: all or nothing");
    // the old boxes' own numbers are free to be used again, but with `clear` false they are taken
    let kept = e.call("tilemap_colliders", &j(r#"{"uid":1,"first_uid":1000,"clear":false}"#)).unwrap_err();
    assert_eq!(kept.code, "duplicate_uid", "{kept:?}");
    // not a number
    for bad in ["0", "-3", "\"x\"", "1.5"] {
        let e2 = e.call("tilemap_colliders", &j(&format!(r#"{{"uid":1,"first_uid":{bad}}}"#))).unwrap_err();
        assert_eq!(e2.code, "bad_value", "{bad}: {e2:?}");
    }
}

#[test]
fn a_clash_with_the_boxes_says_who_made_them_and_what_to_do() {
    let mut e = engine();
    map(&mut e);
    e.call("set", &j(r#"{"uid":1,"component":"Tilemap","value":{"data":"0 0 0 0, 1 -1 1 1"}}"#)).unwrap();
    let r = e.call("tilemap_colliders", &j(r#"{"uid":1}"#)).unwrap();
    let first = r.get("first_uid").unwrap().as_u64().unwrap();
    // an entity made by number that is one of the boxes' numbers
    let err = e.call("spawn", &j(&format!(r#"{{"uid":{first},"components":{{"Transform":{{}}}}}}"#))).unwrap_err();
    assert_eq!(err.code, "duplicate_uid");
    assert!(err.message.contains(&format!("uid {first} already exists")) && err.message.contains("tile collider 1") && err.message.contains("tilemap_colliders"), "{err:?}");
    let hint = err.hint.clone().unwrap();
    assert!(hint.contains("first_uid") && hint.contains("above"), "{hint}");
    // a clash with any other named entity says its name; an unnamed one still gets a hint
    e.call("spawn", &j(r#"{"uid":500,"components":{"Name":{"value":"hero"}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":501,"components":{"Transform":{}}}"#)).unwrap();
    let err = e.call("spawn", &j(r#"{"uid":500,"components":{}}"#)).unwrap_err();
    assert!(err.message.contains("`hero`") && err.hint.as_deref().is_some_and(|h| h.contains("leave `uid` out")), "{err:?}");
    let err = e.call("spawn", &j(r#"{"uid":501,"components":{}}"#)).unwrap_err();
    assert!(err.hint.as_deref().is_some_and(|h| h.contains("leave `uid` out")), "{err:?}");
    // other errors are left as they are
    assert_eq!(e.call("despawn", &j(r#"{"uid":99999}"#)).unwrap_err().code, "no_such_entity");
}
