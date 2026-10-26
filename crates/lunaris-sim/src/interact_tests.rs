//! Interaction: what is aimed at, doors that swing about their hinge, locks and keys, containers that open and shut, items that are picked up, the press that counts once. No art: boxes and numbers.
use super::*;
use crate::components::{Door, Interactable, Interactor, Transform};
use crate::inventory::Inventory;
use lunaris_json::{parse, Json};

const ITEMS: &str = "item bandage | Bandage | stack=10\nitem crowbar | Crowbar | stack=1\n";

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// A world with an item table and a character (uid 1) at (0, 1.6, 0) looking down -z with a bag of 12 slots.
fn world(interactor: &str) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 50, &format!(r#"{{"ItemTable":{{"text":"{}"}}}}"#, ITEMS.replace('\n', "\\n")));
    let sep = if interactor.is_empty() { "" } else { "," };
    spawn(&mut s, 1, &format!(r#"{{"Transform":{{"pos":[0,1.6,0]}},"Inventory":{{"size":12}},"Interactor":{{"reach":3{sep}{interactor}}}}}"#));
    s
}

fn chest(s: &mut Sim, uid: u64, z: f32, extra: &str) {
    let sep = if extra.is_empty() { "" } else { "," };
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[0,1.6,{z}]}},"Collider":{{"shape":1,"size":[1,1,1]}},"Interactable":{{"kind":2,"prompt":"Search"{sep}{extra}}},"Inventory":{{"size":8}}}}"#));
}

fn door(s: &mut Sim, uid: u64, extra: &str) {
    let sep = if extra.is_empty() { "" } else { "," };
    spawn(
        s,
        uid,
        &format!(r#"{{"Transform":{{"pos":[0,1.6,-2]}},"Collider":{{"shape":1,"size":[1,2,0.1]}},"Interactable":{{"kind":1,"prompt":"Open"{sep}{extra}}},"Door":{{"hinge":[-0.5,0,0],"angle_open":100,"speed":200}}}}"#),
    );
}

fn set_interactor(s: &mut Sim, f: impl FnOnce(&mut Interactor)) {
    let e = s.world.entity(1).unwrap();
    f(s.world.get_mut::<Interactor>(e).unwrap());
}

fn it(s: &Sim) -> Interactor {
    *s.world.get::<Interactor>(s.world.entity(1).unwrap()).unwrap()
}

fn press(s: &mut Sim) {
    set_interactor(s, |i| i.act = true);
    s.step();
    set_interactor(s, |i| i.act = false);
    s.step();
}

fn events(s: &Sim, name: &str) -> Vec<u64> {
    s.events.iter().filter(|e| e.name == name).map(|e| e.uid).collect()
}

fn text(s: &Sim, sym: lunaris_ecs::Sym) -> String {
    s.world.strings().get(sym.0).to_string()
}

fn count(s: &Sim, uid: u64, key: &str) -> u32 {
    let (table, _) = crate::inventory::table_of(&mut s.clone().world).unwrap();
    let id = table.id_of(key);
    let inv = s.world.get::<Inventory>(s.world.entity(uid).unwrap()).unwrap();
    (0..48).filter(|&k| inv.id[k] == id).map(|k| inv.count[k]).sum()
}

#[test]
fn the_character_knows_what_it_looks_at_and_the_prompt_of_it() {
    let mut s = world("");
    chest(&mut s, 10, -2.0, "");
    s.step();
    let i = it(&s);
    assert_eq!((i.target.0, i.kind, text(&s, i.prompt)), (10, 2, "Search".to_string()));
    assert!((i.distance - 1.5).abs() < 0.05, "the front face of the chest: {}", i.distance);
    // too far (the range of the thing is 3), and a wall in front of it hides it
    let mut s = world("");
    chest(&mut s, 10, -5.0, "");
    s.step();
    assert_eq!(it(&s).target.0, 0);
    let mut s = world("");
    chest(&mut s, 10, -2.5, "");
    spawn(&mut s, 11, r#"{"Transform":{"pos":[0,1.6,-1]},"Collider":{"shape":1,"size":[4,4,0.2]}}"#);
    s.step();
    assert_eq!(it(&s).target.0, 0, "the wall hides it");
    // looking away
    let mut s = world("");
    chest(&mut s, 10, 2.0, "");
    s.step();
    assert_eq!(it(&s).target.0, 0, "it is behind");
    // a thing that is disabled is not there
    let mut s = world("");
    chest(&mut s, 10, -2.0, r#""enabled":false"#);
    s.step();
    assert_eq!(it(&s).target.0, 0);
}

#[test]
fn a_door_swings_about_its_hinge_and_back_and_a_held_button_counts_once() {
    let mut s = world("");
    door(&mut s, 10, "");
    s.step();
    assert_eq!(it(&s).target.0, 10);
    let hinge = [-0.5, 1.6, -2.0];
    let pos_of = |s: &Sim| s.world.get::<Transform>(s.world.entity(10).unwrap()).unwrap().pos;
    let rot_of = |s: &Sim| s.world.get::<Transform>(s.world.entity(10).unwrap()).unwrap().rot;
    // hold the button for many ticks: the door opens once
    set_interactor(&mut s, |i| i.act = true);
    s.step_n(120);
    set_interactor(&mut s, |i| i.act = false);
    s.step_n(10);
    assert_eq!(events(&s, "door_open").len(), 1, "one press: {:?}", s.events.iter().map(|e| &e.name).collect::<Vec<_>>());
    let d = *s.world.get::<Door>(s.world.entity(10).unwrap()).unwrap();
    assert!(d.open && (d.angle - 100.0).abs() < 0.01, "{d:?}");
    let p = pos_of(&s);
    let r = ((p[0] - hinge[0]).powi(2) + (p[2] - hinge[2]).powi(2)).sqrt();
    assert!((r - 0.5).abs() < 1.0e-3, "the hinge stays where it is: {r} from it at {p:?}");
    assert!((crate::building::yaw_of(rot_of(&s)) - 100.0).abs() < 0.1, "{}", crate::building::yaw_of(rot_of(&s)));
    // another press shuts it, into exactly where it was (the open leaf lies along the way; the character steps to its side and looks at it, along -x)
    let e = s.world.entity(1).unwrap();
    {
        let t = s.world.get_mut::<Transform>(e).unwrap();
        t.pos = [0.5, 1.6, -2.5];
        t.rot = [0.0, std::f32::consts::FRAC_1_SQRT_2, 0.0, std::f32::consts::FRAC_1_SQRT_2];
    }
    s.step();
    assert_eq!(it(&s).target.0, 10, "the open door can be aimed at from the side");
    press(&mut s);
    s.step_n(120);
    assert_eq!(events(&s, "door_close").len(), 1);
    let p = pos_of(&s);
    assert!((p[0]).abs() < 1.0e-3 && (p[2] + 2.0).abs() < 1.0e-3, "back where it was: {p:?}");
    // it takes time to swing: 100 degrees at 200 a second is half a second
    let mut s = world("");
    door(&mut s, 10, "");
    press(&mut s);
    let a = s.world.get::<Door>(s.world.entity(10).unwrap()).unwrap().angle;
    assert!(a > 0.0 && a < 100.0, "it is on its way: {a}");
}

#[test]
fn a_locked_door_wants_its_key_and_a_key_that_is_not_used_up_stays() {
    let mut s = world("");
    door(&mut s, 10, r#""locked":true,"key":"crowbar""#);
    s.step();
    press(&mut s);
    let i = it(&s);
    assert_eq!(events(&s, "locked"), vec![10], "without the key it is told");
    assert!(!s.world.get::<Door>(s.world.entity(10).unwrap()).unwrap().open);
    // with the crowbar in the bag it opens
    spawn(&mut s, 60, "{}");
    let e = s.world.entity(1).unwrap();
    s.world.get_mut::<Inventory>(e).unwrap().fill = lunaris_ecs::Sym(s.world.strings_mut().intern("crowbar 1"));
    s.step();
    assert_eq!(count(&s, 1, "crowbar"), 1);
    s.step_n(30);
    press(&mut s);
    assert_eq!(events(&s, "unlocked"), vec![10]);
    assert!(s.world.get::<Door>(s.world.entity(10).unwrap()).unwrap().open, "and it swings");
    assert_eq!(count(&s, 1, "crowbar"), 1, "the crowbar is not used up");
    let _ = i;
    // a key that is used up
    let mut s = world("");
    door(&mut s, 10, r#""locked":true,"key":"crowbar","key_consume":true"#);
    let e = s.world.entity(1).unwrap();
    s.world.get_mut::<Inventory>(e).unwrap().fill = lunaris_ecs::Sym(s.world.strings_mut().intern("crowbar 1"));
    s.step_n(3);
    press(&mut s);
    assert_eq!(count(&s, 1, "crowbar"), 0);
    assert!(!s.world.get::<Interactable>(s.world.entity(10).unwrap()).unwrap().locked);
}

#[test]
fn a_container_opens_for_its_user_shuts_again_and_shuts_itself_when_they_walk_away() {
    let mut s = world("");
    chest(&mut s, 10, -2.0, "");
    s.step();
    press(&mut s);
    let c = *s.world.get::<Interactable>(s.world.entity(10).unwrap()).unwrap();
    assert!(c.open && c.user.0 == 1, "{c:?}");
    assert_eq!(it(&s).open.0, 10);
    assert_eq!(events(&s, "container_open"), vec![10]);
    // again: shut
    s.step_n(30);
    press(&mut s);
    assert!(!s.world.get::<Interactable>(s.world.entity(10).unwrap()).unwrap().open);
    assert_eq!(events(&s, "container_close"), vec![10]);
    // open it, then walk 10 metres away
    s.step_n(30);
    press(&mut s);
    assert!(s.world.get::<Interactable>(s.world.entity(10).unwrap()).unwrap().open);
    let e = s.world.entity(1).unwrap();
    s.world.get_mut::<Transform>(e).unwrap().pos[0] = 10.0;
    s.step_n(2);
    assert!(!s.world.get::<Interactable>(s.world.entity(10).unwrap()).unwrap().open, "it shut itself");
    assert_eq!(events(&s, "container_close").len(), 2);
    assert_eq!(it(&s).open.0, 0);
}

#[test]
fn an_item_on_the_ground_is_picked_up_and_what_does_not_fit_stays() {
    let mut s = world("");
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,1.6,-2]},"Interactable":{"kind":3,"prompt":"Take","item":"bandage 3","radius":0.5}}"#);
    s.step();
    assert_eq!(it(&s).target.0, 10, "a thing with no collider is aimed at as a sphere");
    press(&mut s);
    assert_eq!(count(&s, 1, "bandage"), 3);
    assert!(s.world.entity(10).is_none(), "it is gone from the ground");
    assert_eq!(events(&s, "picked"), vec![10]);
    // a bag with room for 10 only: 25 bandages in stacks of 10 (a bag of 1 slot)
    let mut s = Sim::new(1);
    spawn(&mut s, 50, &format!(r#"{{"ItemTable":{{"text":"{}"}}}}"#, ITEMS.replace('\n', "\\n")));
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.6,0]},"Inventory":{"size":1,"bag":1},"Interactor":{"reach":3}}"#);
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,1.6,-2]},"Interactable":{"kind":3,"item":"bandage 25"}}"#);
    s.step();
    press(&mut s);
    assert_eq!(count(&s, 1, "bandage"), 10);
    assert!(s.world.entity(10).is_some(), "15 are left on the ground");
    assert_eq!(text(&s, s.world.get::<Interactable>(s.world.entity(10).unwrap()).unwrap().item), "bandage 15");
    assert_eq!(events(&s, "inventory_full"), vec![10]);
}

#[test]
fn a_plain_thing_gives_the_event_and_the_outputs_last_one_tick() {
    let mut s = world("");
    spawn(&mut s, 10, r#"{"Transform":{"pos":[0,1.6,-2]},"Interactable":{"kind":0,"prompt":"Pull"}}"#);
    s.step();
    set_interactor(&mut s, |i| i.act = true);
    s.step();
    assert_eq!(it(&s).used.0, 10);
    assert_eq!(events(&s, "used"), vec![10]);
    assert_eq!(events(&s, "interact"), vec![10]);
    set_interactor(&mut s, |i| i.act = false);
    s.step();
    assert_eq!(it(&s).used.0, 0, "the outputs are for one tick");
    assert_eq!(s.world.get::<Interactable>(s.world.entity(10).unwrap()).unwrap().uses, 1);
}

#[test]
fn a_character_that_does_not_look_only_looks_when_it_acts() {
    let mut s = world(r#""look":false"#);
    chest(&mut s, 10, -2.0, "");
    s.step_n(3);
    assert_eq!(it(&s).target.0, 0, "no prompt without looking");
    set_interactor(&mut s, |i| i.act = true);
    s.step();
    assert!(s.world.get::<Interactable>(s.world.entity(10).unwrap()).unwrap().open, "but a press works");
}

#[test]
fn a_door_that_is_a_child_swings_on_its_local_pose() {
    let mut s = world("");
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,0,0]}}"#);
    spawn(
        &mut s,
        10,
        r#"{"Parent":{"target":5},"LocalTransform":{"pos":[0,1.6,-2]},"Transform":{"pos":[0,1.6,-2]},"Collider":{"shape":1,"size":[1,2,0.1]},"Interactable":{"kind":1},"Door":{"hinge":[-0.5,0,0],"angle_open":-90,"speed":300}}"#,
    );
    s.step();
    press(&mut s);
    s.step_n(60);
    let l = s.world.get::<crate::components::LocalTransform>(s.world.entity(10).unwrap()).unwrap();
    assert!((crate::building::yaw_of(l.rot) + 90.0).abs() < 0.1, "swung the other way: {}", crate::building::yaw_of(l.rot));
    let p = l.pos;
    assert!(((p[0] + 0.5).powi(2) + (p[2] + 2.0).powi(2)).sqrt() - 0.5 < 1.0e-3);
}

#[test]
fn using_things_is_deterministic() {
    let run = || {
        let mut s = world("");
        door(&mut s, 10, r#""locked":true,"key":"crowbar""#);
        chest(&mut s, 11, -2.0, "");
        spawn(&mut s, 12, r#"{"Transform":{"pos":[0,1.6,-2.8]},"Interactable":{"kind":3,"item":"bandage 2"}}"#);
        s.step_n(3);
        press(&mut s);
        s.step_n(30);
        press(&mut s);
        s.world.state_hash()
    };
    assert_eq!(run(), run());
}
