//! Drag and drop: inventory slots and equipment (`UiDrag`, `UiDrop`).
use super::*;
use components::{Ui, UiDrag, UiDrop};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn setr(s: &mut Sim, name: &str, v: f64) {
    s.world.set_resource(name, Some(v)).unwrap();
}

fn ui(s: &Sim, uid: u64) -> Ui {
    *s.world.get::<Ui>(s.world.entity(uid).unwrap()).unwrap()
}

fn drag(s: &Sim, uid: u64) -> UiDrag {
    *s.world.get::<UiDrag>(s.world.entity(uid).unwrap()).unwrap()
}

fn slot(s: &Sim, uid: u64) -> UiDrop {
    *s.world.get::<UiDrop>(s.world.entity(uid).unwrap()).unwrap()
}

fn pointer(s: &mut Sim, x: f64, y: f64, down: bool) {
    setr(s, "input.mouse_x", x);
    setr(s, "input.mouse_y", y);
    setr(s, "input.mouse_left", down as u8 as f64);
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn res(s: &Sim, name: &str) -> f64 {
    s.world.resource(name).unwrap_or(0.0)
}

/// Two slots, A (uid 1, 0.1..0.2 x 0.1..0.3) and B (uid 2, 0.4..0.5 x 0.1..0.3), and a sword (uid 10, 0.06 x 0.1, in A, item number 7).
fn inventory() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.1,0.2],"layer":1},"UiDrop":{"name":"A"}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.4,0.1],"size":[0.1,0.2],"layer":1},"UiDrop":{"name":"B"}}"#);
    spawn(&mut s, 10, r#"{"Ui":{"pos":[0.0,0.0],"size":[0.06,0.1],"layer":2,"button":true},"UiDrag":{"item":7,"slot":1}}"#);
    s
}

/// Pick the sword up at its middle, carry it to (x, y) and let go there (the pointer's steps are the ticks).
fn carry(s: &mut Sim, to: (f64, f64)) {
    let c = ui(s, 10);
    let (x, y) = ((c.pos[0] + c.size[0] * 0.5) as f64, (c.pos[1] + c.size[1] * 0.5) as f64);
    pointer(s, x, y, true);
    s.step();
    pointer(s, (x + to.0) * 0.5, (y + to.1) * 0.5, true);
    s.step();
    pointer(s, to.0, to.1, true);
    s.step();
    pointer(s, to.0, to.1, false);
    s.step();
}

#[test]
fn an_item_sits_centred_in_its_slot_and_follows_the_pointer_while_held() {
    let mut s = inventory();
    s.step();
    let (a, sword) = (ui(&s, 1), ui(&s, 10));
    assert!(close(sword.pos[0], a.pos[0] + (a.size[0] - sword.size[0]) * 0.5) && close(sword.pos[1], a.pos[1] + (a.size[1] - sword.size[1]) * 0.5), "{sword:?}");
    assert_eq!(slot(&s,1).holds.0, 10, "the slot knows what it holds, without being told");
    assert_eq!(slot(&s,2).holds.0, 0);
    // pick it up in the middle
    let (cx, cy) = ((sword.pos[0] + sword.size[0] * 0.5) as f64, (sword.pos[1] + sword.size[1] * 0.5) as f64);
    pointer(&mut s, cx, cy, true);
    s.step();
    assert!(drag(&s, 10).dragging);
    assert!(ui(&s, 10).layer > 100, "lifted above everything while it is held: {}", ui(&s, 10).layer);
    // carried: the same grip on it the whole way
    pointer(&mut s, 0.3, 0.5, true);
    s.step();
    let held = ui(&s, 10);
    assert!(close(held.pos[0] + held.size[0] * 0.5, 0.3) && close(held.pos[1] + held.size[1] * 0.5, 0.5), "{held:?}");
    pointer(&mut s, 0.31, 0.52, true);
    s.step_n(3);
    let held = ui(&s, 10);
    assert!(close(held.pos[0] + held.size[0] * 0.5, 0.31) && close(held.pos[1] + held.size[1] * 0.5, 0.52), "{held:?}");
    // let go over nothing: back in its slot, the layer put back
    pointer(&mut s, 0.31, 0.52, false);
    s.step();
    let back = ui(&s, 10);
    assert!(close(back.pos[0], sword.pos[0]) && close(back.pos[1], sword.pos[1]) && back.layer == 2, "{back:?}");
    assert!(!drag(&s, 10).dragging && drag(&s, 10).slot.0 == 1);
    assert_eq!(res(&s, "drop.A") + res(&s, "drop.B"), 0.0, "nothing was dropped on anything");
}

#[test]
fn dropping_on_another_slot_moves_the_item_and_reports_it() {
    let mut s = inventory();
    s.step();
    carry(&mut s, (0.45, 0.2));
    assert_eq!(drag(&s, 10).slot.0, 2, "now in slot B");
    let (b, sword) = (ui(&s, 2), ui(&s, 10));
    assert!(close(sword.pos[0], b.pos[0] + (b.size[0] - sword.size[0]) * 0.5) && close(sword.pos[1], b.pos[1] + (b.size[1] - sword.size[1]) * 0.5), "centred in B: {sword:?}");
    assert_eq!((slot(&s,1).holds.0, slot(&s,2).holds.0), (0, 10));
    // reported in the tick of the drop: the component and the resources
    assert!(slot(&s,2).dropped && slot(&s,2).from.0 == 10 && slot(&s,2).item == 7);
    assert_eq!((res(&s, "drop.B"), res(&s, "drop.B.item"), res(&s, "drop.B.count")), (10.0, 7.0, 1.0));
    assert!(!slot(&s,1).dropped);
    // and over again one tick later (the count stays)
    s.step();
    assert!(!slot(&s,2).dropped);
    assert_eq!((res(&s, "drop.B"), res(&s, "drop.B.item"), res(&s, "drop.B.count")), (0.0, 0.0, 1.0));
    // and back to A
    carry(&mut s, (0.15, 0.2));
    assert_eq!((drag(&s, 10).slot.0, slot(&s,1).holds.0, slot(&s,2).holds.0), (1, 10, 0));
    assert_eq!((res(&s, "drop.A"), res(&s, "drop.A.count")), (10.0, 1.0));
    // putting it back in its own slot is not a drop
    s.step();
    carry(&mut s, (0.15, 0.25));
    assert_eq!(res(&s, "drop.A.count"), 1.0);
    assert_eq!(drag(&s, 10).slot.0, 1);
}

#[test]
fn a_slot_with_something_in_it_swaps_places() {
    let mut s = inventory();
    spawn(&mut s, 11, r#"{"Ui":{"pos":[0.0,0.0],"size":[0.06,0.1],"layer":2,"button":true},"UiDrag":{"item":9,"slot":2}}"#);
    s.step();
    carry(&mut s, (0.45, 0.2));
    assert_eq!((drag(&s, 10).slot.0, drag(&s, 11).slot.0), (2, 1), "the two traded places");
    assert_eq!((slot(&s,1).holds.0, slot(&s,2).holds.0), (11, 10));
    let (a, b) = (ui(&s, 1), ui(&s, 2));
    let (sw, sh) = (ui(&s, 10), ui(&s, 11));
    assert!(close(sw.pos[0], b.pos[0] + (b.size[0] - sw.size[0]) * 0.5) && close(sh.pos[0], a.pos[0] + (a.size[0] - sh.size[0]) * 0.5), "{sw:?} {sh:?}");
    // a slot that does not swap sends it back
    s.world.get_mut::<UiDrop>(s.world.entity(1).unwrap()).unwrap().swap = false;
    s.step();
    carry(&mut s, (0.15, 0.2));
    assert_eq!((drag(&s, 10).slot.0, drag(&s, 11).slot.0), (2, 1), "A does not swap: nothing moved");
}

#[test]
fn a_slot_takes_only_the_kinds_it_accepts() {
    let mut s = inventory();
    // slot C takes kind 3 only
    spawn(&mut s, 3, r#"{"Ui":{"pos":[0.7,0.1],"size":[0.1,0.2],"layer":1},"UiDrop":{"name":"C","accept":8}}"#);
    s.step();
    carry(&mut s, (0.75, 0.2));
    assert_eq!(drag(&s, 10).slot.0, 1, "kind 0 is not welcome in C: back in A");
    assert_eq!(res(&s, "drop.C.count"), 0.0);
    s.world.get_mut::<UiDrag>(s.world.entity(10).unwrap()).unwrap().kind = 3;
    s.step();
    carry(&mut s, (0.75, 0.2));
    assert_eq!(drag(&s, 10).slot.0, 3, "kind 3 goes in");
    assert_eq!(res(&s, "drop.C"), 10.0);
}

#[test]
fn a_still_press_is_a_click_and_a_drag_is_not() {
    let mut s = inventory();
    s.step();
    let sword = ui(&s, 10);
    let (cx, cy) = ((sword.pos[0] + sword.size[0] * 0.5) as f64, (sword.pos[1] + sword.size[1] * 0.5) as f64);
    pointer(&mut s, cx, cy, true);
    s.step();
    pointer(&mut s, cx, cy, false);
    s.step();
    assert!(ui(&s, 10).clicked, "pressed and released without moving: a click, and it did not go anywhere");
    assert_eq!(drag(&s, 10).slot.0, 1);
    s.step();
    carry(&mut s, (0.45, 0.2));
    let _ = (cx, cy);
    assert!(!ui(&s, 10).clicked, "carried to another slot: not a click");
}

#[test]
fn a_press_that_starts_elsewhere_does_not_pick_the_item_up() {
    let mut s = inventory();
    s.step();
    pointer(&mut s, 0.7, 0.7, true);
    s.step();
    let sword = ui(&s, 10);
    // the button is still down and the pointer slides over the item
    pointer(&mut s, (sword.pos[0] + 0.02) as f64, (sword.pos[1] + 0.02) as f64, true);
    s.step_n(2);
    assert!(!drag(&s, 10).dragging, "it was not pressed on the item");
}

#[test]
fn a_slot_that_moves_takes_its_item_along_and_a_hidden_one_takes_nothing() {
    let mut s = inventory();
    s.step();
    s.world.get_mut::<Ui>(s.world.entity(1).unwrap()).unwrap().pos = [0.6, 0.5];
    s.step();
    let (a, sword) = (ui(&s, 1), ui(&s, 10));
    assert!(close(sword.pos[0], a.pos[0] + (a.size[0] - sword.size[0]) * 0.5) && close(sword.pos[1], a.pos[1] + (a.size[1] - sword.size[1]) * 0.5));
    // a slot in a hidden group is not there to drop on
    spawn(&mut s, 5, r#"{"UiGroup":{"shown":false}}"#);
    spawn(&mut s, 3, r#"{"Ui":{"pos":[0.7,0.1],"size":[0.1,0.2],"layer":1},"UiDrop":{"name":"C"},"Parent":{"target":5}}"#);
    s.step();
    carry(&mut s, (0.75, 0.2));
    assert_eq!(drag(&s, 10).slot.0, 1);
}

#[test]
fn a_free_thing_goes_home_and_into_a_slot_and_stays_there() {
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.4,0.1],"size":[0.1,0.2],"layer":1},"UiDrop":{"name":"B"}}"#);
    spawn(&mut s, 10, r#"{"Ui":{"pos":[0.7,0.6],"size":[0.06,0.1],"layer":2,"button":true},"UiDrag":{"item":4}}"#);
    s.step();
    carry(&mut s, (0.2, 0.9));
    let free = ui(&s, 10);
    assert!(close(free.pos[0], 0.7) && close(free.pos[1], 0.6), "dropped on nothing: home {free:?}");
    carry(&mut s, (0.45, 0.2));
    assert_eq!(drag(&s, 10).slot.0, 2);
    // out of a slot onto nothing: back to the slot, not to where it started
    s.step();
    carry(&mut s, (0.2, 0.9));
    assert_eq!(drag(&s, 10).slot.0, 2);
    let b = ui(&s, 2);
    assert!(close(ui(&s, 10).pos[0], b.pos[0] + (b.size[0] - 0.06) * 0.5));
}

#[test]
fn a_press_on_an_item_in_a_scroll_view_drags_the_item_not_the_content() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.4,0.4]},"UiScroll":{"content_size":[0.4,1.0]}}"#);
    spawn(&mut s, 10, r#"{"Ui":{"pos":[0.15,0.15],"size":[0.1,0.1],"button":true},"UiDrag":{"item":1},"Parent":{"target":1}}"#);
    s.step_n(2);
    pointer(&mut s, 0.2, 0.2, true);
    s.step();
    pointer(&mut s, 0.2, 0.35, true);
    s.step();
    let o = s.world.get::<components::UiScroll>(s.world.entity(1).unwrap()).unwrap().offset;
    assert_eq!(o, [0.0, 0.0], "the content stayed: the finger holds an item");
    assert!(drag(&s, 10).dragging);
}

#[test]
fn the_new_components_are_registered_with_their_documentation() {
    let s = Sim::new(1);
    for name in ["UiDrag", "UiDrop"] {
        let id = s.world.component_id(name).unwrap_or_else(|| panic!("{name} is not registered"));
        assert!(s.world.info(id).doc.len() > 100, "{name} has a description for `schema`");
    }
}
