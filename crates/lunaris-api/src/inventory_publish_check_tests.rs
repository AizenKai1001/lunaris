//! `check` knows the resources an `Inventory` with `publish` makes (`have.KEY` for every item, `have.can0`, ...): a page that reads them is not told "nothing sets it"; a name that is not an item still is.
use crate::tests::engine;
use crate::Engine;
use lunaris_json::{parse, Json};

fn call(e: &mut Engine, tool: &str, args: &str) -> Json {
    e.call(tool, &parse(args).unwrap()).unwrap_or_else(|err| panic!("{tool} failed: {err}"))
}

fn messages(e: &mut Engine) -> Vec<String> {
    let c = call(e, "check", "{}");
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| p.get("message").unwrap().as_str().unwrap().to_string()).collect()
}

#[test]
fn what_a_published_inventory_writes_is_known_to_check() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"uid":9,"components":{"ItemTable":{"text":"item wood | Wood | stack=50\nitem stone | Stone | stack=50\nrecipe stone 1 = wood 2\n"}}}"#);
    call(&mut e, "spawn", r#"{"uid":1,"components":{"Transform":{},"Inventory":{"size":8,"publish":"have"}}}"#);
    call(&mut e, "spawn", r#"{"uid":2,"components":{"Text":{"value":"wood {have.wood}, can {have.can0}, nails {have.nails}"}}}"#);
    call(&mut e, "define_system", r#"{"name":"reads","each":["Inventory"],"do":[{"res_set":"seen","to":{"add":[{"res":"have.stone"},{"res":"have.free"},{"res":"have.screws"}]}}]}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("have.wood") || x.contains("have.can0") || x.contains("have.stone") || x.contains("have.free")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("have.nails")), "a name that is not an item is still reported: {m:?}");
    assert!(m.iter().any(|x| x.contains("have.screws")), "{m:?}");
}

#[test]
fn a_loot_drop_that_cannot_work_is_named() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"uid":9,"components":{"ItemTable":{"text":"item scrap | Scrap | stack=30\n"}}}"#);
    call(&mut e, "spawn", r#"{"uid":8,"components":{"LootTable":{"text":"table heap rolls=1\n  scrap w=1 n=1-3\n"}}}"#);
    call(&mut e, "define_prefab", r#"{"name":"Pickup","def":{"components":{"Interactable":{"kind":3}}}}"#);
    call(&mut e, "spawn", r#"{"uid":1,"components":{"Transform":{},"HitPoints":{},"LootDrop":{"table":"heap","prefab":"Pickup"}}}"#);
    call(&mut e, "spawn", r#"{"uid":2,"components":{"LootDrop":{"table":"nope","prefab":"Gone"}}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("LootDrop") && x.contains("heap")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("needs HitPoints")) && m.iter().any(|x| x.contains("needs a Transform")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("`nope` is not a table")) && m.iter().any(|x| x.contains("`Gone` is not a prefab")), "{m:?}");
}


#[test]
fn a_station_that_cannot_work_is_named_and_the_resources_it_adds_are_known() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"uid":9,"components":{"ItemTable":{"text":"item wood | Wood | stack=50\nitem axe | Axe | stack=1\nrecipe axe 1 = wood 2 @1\n"}}}"#);
    call(&mut e, "spawn", r#"{"uid":1,"components":{"Transform":{},"Inventory":{"size":8,"publish":"have"}}}"#);
    call(&mut e, "spawn", r#"{"uid":2,"components":{"Transform":{},"Station":{"bit":1,"radius":3}}}"#);
    call(&mut e, "define_system", r#"{"name":"reads","each":["Inventory"],"do":[{"res_set":"seen","to":{"add":[{"res":"have.near"},{"res":"have.craft_wait"}]}}]}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("Station") || x.contains("have.near") || x.contains("have.craft_wait")), "a good station says nothing: {m:?}");
    call(&mut e, "spawn", r#"{"uid":3,"components":{"Station":{"bit":2,"radius":0}}}"#);
    call(&mut e, "spawn", r#"{"uid":4,"components":{"Transform":{},"Station":{"bit":40}}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("a Station needs a Transform")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Station.bit 2: no recipe")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Station.radius must be above 0")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("Station.bit 40 is not a station number")), "{m:?}");
}
