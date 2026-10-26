//! `check` names the mistakes of a game server's scene: the avatar prefab, the forwarded events, the replicated things and the way the client shows them; a good scene says nothing.
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
fn a_good_game_server_scene_says_nothing() {
    let mut e = engine();
    call(&mut e, "define_prefab", r#"{"name":"Avatar","components":{"Transform":{},"CharacterBody":{},"HitPoints":{},"NetAvatar":{}}}"#);
    call(&mut e, "define_prefab", r#"{"name":"Mate","components":{"Transform":{},"NetPuppet":{}}}"#);
    call(&mut e, "define_prefab", r#"{"name":"Wall","components":{"Transform":{},"NetPuppet":{}}}"#);
    spawn(&mut e, 1, r#"{"NetHost":{"player":"Avatar","forward":"shot 150 # loud\nhit 60"}}"#);
    spawn(&mut e, 2, r#"{"Transform":{},"NetEntity":{"class":3,"kind":4}}"#);
    spawn(&mut e, 3, r#"{"Transform":{},"NetPlayer":{"puppet":"Mate","kinds":"3 4 Wall\n4 * Wall"}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("NetHost") || x.contains("NetPlayer") || x.contains("NetEntity") || x.contains("player prefab")), "{m:?}");
}

#[test]
fn the_mistakes_of_a_game_server_scene_are_named() {
    let mut e = engine();
    call(&mut e, "define_prefab", r#"{"name":"Blob","components":{"Transform":{}}}"#);
    spawn(&mut e, 1, r#"{"NetHost":{"player":"Blob","forward":"shot far"}}"#);
    spawn(&mut e, 2, r#"{"NetHost":{}}"#);
    spawn(&mut e, 3, r#"{"Transform":{},"NetEntity":{"class":200}}"#);
    spawn(&mut e, 4, r#"{"Transform":{},"NetPlayer":{"puppet":"Nobody","kinds":"x 4 Wall\n3 * Missing"}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("has no HitPoints")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("has no NetAvatar")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("no CharacterBody or Collider")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("the radius is not a number")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("more than one NetHost")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("NetEntity.class 200")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("NetPlayer.puppet `Nobody`")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("is not CLASS KIND PREFAB")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("the prefab `Missing`")), "{m:?}");
    spawn(&mut e, 6, r#"{"NetContainer":{}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("NetContainer needs an Inventory")), "{m:?}");
    // a host with no player prefab at all
    let mut e = engine();
    spawn(&mut e, 1, r#"{"NetHost":{}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("NetHost.player is empty")), "{m:?}");
}

#[test]
fn the_rules_of_using_items_are_checked_against_the_item_table() {
    let mut e = engine();
    spawn(&mut e, 9, r#"{"ItemTable":{"text":"item can | Food | stack=5\nitem bandage | Bandage | stack=5\n"}}"#);
    spawn(&mut e, 1, r#"{"NetHost":{"uses":"use can eat=30 # food\nuse bandage heal=5 bandage=1"}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("NetHost.uses")), "a good list says nothing: {m:?}");
    let mut e = engine();
    spawn(&mut e, 9, r#"{"ItemTable":{"text":"item can | Food | stack=5\n"}}"#);
    spawn(&mut e, 1, r#"{"NetHost":{"uses":"use pie eat=30\nuse can eat=lots speed=3\neat can\nuse"}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("`pie` is not an item")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("`eat=lots` is not eat")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("`speed=3` is not eat")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("does not start with `use`")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("names no item")), "{m:?}");
}

#[test]
fn the_pictures_of_items_on_the_ground_are_checked_against_the_item_table_and_the_clients_kinds() {
    let mut e = engine();
    call(&mut e, "define_prefab", r#"{"name":"Gun","components":{"Transform":{},"NetPuppet":{}}}"#);
    spawn(&mut e, 9, r#"{"ItemTable":{"text":"item pistol | Pistol | stack=1\nitem rifle | Rifle | stack=1\n"}}"#);
    spawn(&mut e, 1, r#"{"NetHost":{"item_kinds":"pistol 33 # small\nrifle 34"}}"#);
    spawn(&mut e, 3, r#"{"Transform":{},"NetPlayer":{"kinds":"1 33 Gun\n1 * Gun"}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("item_kinds")), "a good list says nothing: {m:?}");
    let mut e = engine();
    call(&mut e, "define_prefab", r#"{"name":"Gun","components":{"Transform":{},"NetPuppet":{}}}"#);
    spawn(&mut e, 9, r#"{"ItemTable":{"text":"item pistol | Pistol | stack=1\n"}}"#);
    spawn(&mut e, 1, r#"{"NetHost":{"item_kinds":"pistol 33\nsword 35\nrifle\npistol many"}}"#);
    spawn(&mut e, 3, r#"{"Transform":{},"NetPlayer":{"kinds":"3 33 Gun\n1 34 Gun"}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("`sword` is not an item")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("`rifle` is not KEY KIND")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("`pistol many` is not KEY KIND")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("gives the kind 35")), "a kind no client line shows: {m:?}");
    assert!(!m.iter().any(|x| x.contains("gives the kind 33")), "a kind of class 3 is shown too: {m:?}");
}

#[test]
fn a_connect_button_that_reads_no_text_field_is_named() {
    let mut e = engine();
    spawn(&mut e, 5, r#"{"Ui":{"button":true},"UiInput":{}}"#);
    spawn(&mut e, 6, r#"{"NetConnect":{"address_input":5}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("NetConnect")), "a good one says nothing: {m:?}");
    let mut e = engine();
    spawn(&mut e, 5, r#"{"Ui":{"button":true}}"#);
    spawn(&mut e, 6, r#"{"NetConnect":{"address_input":5,"name_input":77}}"#);
    spawn(&mut e, 7, r#"{"NetConnect":{}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("NetConnect.address_input 5 is not an entity with a UiInput")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("NetConnect.name_input 77 is not an entity with a UiInput")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("neither `address_input` nor an `address`")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("more than one NetConnect")), "{m:?}");
}

#[test]
fn what_nethost_sync_and_skip_name_is_checked() {
    let mut e = engine();
    call(&mut e, "define_prefab", r#"{"name":"Avatar","components":{"Transform":{},"CharacterBody":{},"HitPoints":{},"NetAvatar":{}}}"#);
    spawn(&mut e, 1, r#"{"NetHost":{"player":"Avatar","sync":"dn.t dn.rate","sync_every":0.5,"skip":"Ui Text"}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("NetHost.sync") || x.contains("NetHost.skip")), "a good host says nothing: {m:?}");
    spawn(&mut e, 2, r#"{"NetHost":{}}"#);
    call(&mut e, "set", r#"{"uid":1,"component":"NetHost","value":{"sync":"dn.t input.key.w bad-name dn.t","skip":"Ui Nonsense","sync_every":-1}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("NetHost.sync: `input.key.w`")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("NetHost.sync: `bad-name`")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("NetHost.sync: `dn.t` is not sent")), "the second copy of a name: {m:?}");
    assert!(m.iter().any(|x| x.contains("NetHost.skip: `Nonsense` is not a component")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("NetHost.sync_every must be")), "{m:?}");
}
