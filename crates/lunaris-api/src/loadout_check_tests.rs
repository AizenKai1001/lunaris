//! `check` names the mistakes of a loadout: guns or ammunition that are not items, lines it cannot read, no weapon or bag; a good one says nothing.
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

const ITEMS: &str = r#"{"ItemTable":{"text":"item pistol | Pistol | stack=1\nitem ammo9 | 9 mm | stack=60\n"}}"#;

#[test]
fn a_good_loadout_says_nothing() {
    let mut e = engine();
    spawn(&mut e, 9, ITEMS);
    spawn(&mut e, 1, r#"{"Transform":{},"Inventory":{"size":8},"Weapon":{},"Loadout":{"guns":"gun pistol ammo=ammo9 damage=20"}}"#);
    let m = messages(&mut e);
    assert!(!m.iter().any(|x| x.contains("Loadout")), "{m:?}");
}

#[test]
fn the_mistakes_of_a_loadout_are_named() {
    let mut e = engine();
    spawn(&mut e, 9, ITEMS);
    spawn(&mut e, 1, r#"{"Transform":{},"Loadout":{"guns":"gun rifle ammo=ammo556\ngun pistol ammo=ammo9 damage=lots\nnonsense"}}"#);
    spawn(&mut e, 2, r#"{"Transform":{},"Inventory":{"size":8},"Weapon":{},"Loadout":{"guns":""}}"#);
    let m = messages(&mut e);
    assert!(m.iter().any(|x| x.contains("gun `rifle` is not an item")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("takes ammunition `ammo556`")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("damage needs a number")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("a line is `gun ITEM")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("no Weapon to configure")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("no Inventory")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("names no gun")), "{m:?}");
}
