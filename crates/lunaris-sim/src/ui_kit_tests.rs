//! Layout containers, sliders, toggles, text fields and focus navigation.
use super::*;
use components::{Ui, UiInput, UiSlider, UiToggle};
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

fn pointer(s: &mut Sim, x: f64, y: f64, down: bool) {
    setr(s, "input.mouse_x", x);
    setr(s, "input.mouse_y", y);
    setr(s, "input.mouse_left", down as u8 as f64);
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn layouts_put_children_in_a_column_a_row_or_a_grid_and_nest() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.4,0.6]},"UiLayout":{"kind":0,"padding":0.02,"spacing":0.02}}"#);
    for k in 0..3u64 {
        spawn(&mut s, 2 + k, r#"{"Ui":{},"Parent":{"target":1}}"#);
    }
    s.step();
    // a column: three equal cells in the padded rectangle
    let h = (0.6 - 0.04 - 0.04) / 3.0;
    for k in 0..3u64 {
        let u = ui(&s, 2 + k);
        assert!(close(u.pos[0], 0.12) && close(u.size[0], 0.36), "{u:?}");
        assert!(close(u.pos[1], 0.12 + k as f32 * (h + 0.02)) && close(u.size[1], h), "{u:?}");
    }
    // a hidden child takes no room
    s.world.get_mut::<Ui>(s.world.entity(3).unwrap()).unwrap().visible = false;
    s.step();
    assert!(close(ui(&s, 2).size[1], (0.6 - 0.04 - 0.02) / 2.0));
    // a row, with the width's padding corrected by the aspect ratio
    s.world.get_mut::<components::UiLayout>(s.world.entity(1).unwrap()).unwrap().kind = 1;
    setr(&mut s, "screen.aspect", 2.0);
    s.step();
    let (a, b) = (ui(&s, 2), ui(&s, 4));
    assert!(close(a.pos[0], 0.1 + 0.01) && a.pos[0] < b.pos[0] && close(a.size[1], 0.56), "{a:?} {b:?}");
    // a grid of two columns with four children, and a nested column inside the first cell
    s.world.get_mut::<Ui>(s.world.entity(3).unwrap()).unwrap().visible = true;
    spawn(&mut s, 5, r#"{"Ui":{},"Parent":{"target":1}}"#);
    s.world.get_mut::<components::UiLayout>(s.world.entity(1).unwrap()).unwrap().kind = 2;
    spawn(&mut s, 6, r#"{"Ui":{},"Parent":{"target":2}}"#);
    spawn(&mut s, 7, r#"{"Ui":{},"Parent":{"target":2}}"#);
    s.apply(&Command::Add { uid: 2, component: "UiLayout".into(), value: parse(r#"{"kind":0,"padding":0,"spacing":0}"#).unwrap() }).unwrap();
    s.step();
    let (c0, c3) = (ui(&s, 2), ui(&s, 5));
    assert!(c3.pos[0] > c0.pos[0] && c3.pos[1] > c0.pos[1], "second row, second column: {c0:?} {c3:?}");
    let (n1, n2) = (ui(&s, 6), ui(&s, 7));
    assert!(close(n1.pos[0], c0.pos[0]) && close(n1.size[1], c0.size[1] / 2.0) && close(n2.pos[1], n1.pos[1] + n1.size[1]), "nested: {n1:?} {n2:?}");
}

#[test]
fn a_slider_follows_the_pointer_even_outside_and_stops_on_release() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.2,0.2],"size":[0.4,0.1],"button":true},"UiSlider":{"min":0,"max":10,"step":1,"value":2}}"#);
    let slider = |s: &Sim| *s.world.get::<UiSlider>(s.world.entity(1).unwrap()).unwrap();
    s.step();
    assert_eq!(slider(&s).value, 2.0);
    assert!(close(ui(&s, 1).fill, 0.2));
    pointer(&mut s, 0.4, 0.25, true);
    s.step();
    s.step();
    assert_eq!(slider(&s).value, 5.0, "{:?}", slider(&s));
    assert!(slider(&s).dragging);
    pointer(&mut s, 0.95, 0.9, true); // far outside, still dragging
    s.step();
    assert_eq!(slider(&s).value, 10.0);
    assert!(close(ui(&s, 1).fill, 1.0));
    pointer(&mut s, 0.3, 0.25, false);
    s.step();
    assert!(!slider(&s).dragging);
    pointer(&mut s, 0.3, 0.25, false);
    s.step();
    assert_eq!(slider(&s).value, 10.0, "released: it stays");
    // snaps to the step
    pointer(&mut s, 0.31, 0.25, true);
    s.step();
    s.step();
    assert_eq!(slider(&s).value, 3.0);
}

#[test]
fn a_toggle_flips_on_every_click_and_changes_colour() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.2,0.1],"button":true},"UiToggle":{}}"#);
    let t = |s: &Sim| *s.world.get::<UiToggle>(s.world.entity(1).unwrap()).unwrap();
    s.step();
    let off = ui(&s, 1).color;
    assert!(!t(&s).on);
    for round in 1..=2 {
        pointer(&mut s, 0.2, 0.15, true);
        s.step();
        pointer(&mut s, 0.2, 0.15, false);
        s.step(); // the click
        assert_eq!(t(&s).on, round % 2 == 1, "round {round}");
        assert!(t(&s).changed);
        s.step();
        assert!(!t(&s).changed, "one tick only");
    }
    assert_eq!(ui(&s, 1).color, off, "back off: the off colour");
}

#[test]
fn a_text_field_takes_focus_types_deletes_and_submits() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.4,0.1],"button":true},"UiInput":{"placeholder":"name","max_len":4}}"#);
    let shown = |s: &Sim| s.world.strings().get(ui(s, 1).text.0).to_string();
    let field = |s: &Sim| *s.world.get::<UiInput>(s.world.entity(1).unwrap()).unwrap();
    s.step();
    assert_eq!(shown(&s), "name", "the placeholder");
    // typing without focus does nothing
    setr(&mut s, "input.char", 'x' as u32 as f64);
    s.step();
    assert_eq!(s.world.strings().get(field(&s).text.0), "");
    // click to focus
    pointer(&mut s, 0.3, 0.15, true);
    s.step();
    pointer(&mut s, 0.3, 0.15, false);
    s.step();
    assert!(field(&s).focused);
    for c in "Hello".chars() {
        setr(&mut s, "input.char", c as u32 as f64);
        s.step();
    }
    assert_eq!(s.world.strings().get(field(&s).text.0), "Hell", "at most max_len characters");
    assert_eq!(s.world.resource("input.char"), Some(0.0), "typed characters are used up");
    setr(&mut s, "input.backspace", 1.0);
    s.step();
    assert_eq!(s.world.strings().get(field(&s).text.0), "Hel");
    setr(&mut s, "input.enter", 1.0);
    s.step();
    assert!(field(&s).submitted);
    s.step();
    assert!(!field(&s).submitted, "one tick");
    assert!(shown(&s).starts_with("Hel"));
    // a click away takes the focus
    pointer(&mut s, 0.9, 0.9, true);
    s.step();
    assert!(!field(&s).focused);
    assert_eq!(shown(&s), "Hel");
}

#[test]
fn focus_moves_between_buttons_with_actions_and_accept_clicks() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"UiNav":{}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.3,0.1],"button":true}}"#);
    spawn(&mut s, 3, r#"{"Ui":{"pos":[0.1,0.3],"size":[0.3,0.1],"button":true}}"#);
    spawn(&mut s, 4, r#"{"Ui":{"pos":[0.6,0.3],"size":[0.3,0.1],"button":true}}"#);
    let cur = |s: &Sim| s.world.get::<components::UiNav>(s.world.entity(1).unwrap()).unwrap().current.0;
    let press = |s: &mut Sim, name: &str| {
        setr(s, &format!("action.{name}.pressed"), 1.0);
        s.step();
        setr(s, &format!("action.{name}.pressed"), 0.0);
    };
    s.step();
    assert_eq!(cur(&s), 0);
    press(&mut s, "ui_down");
    assert_eq!(cur(&s), 2, "the first press focuses the first button");
    assert!(ui(&s, 2).hover);
    press(&mut s, "ui_down");
    assert_eq!(cur(&s), 3);
    press(&mut s, "ui_right");
    assert_eq!(cur(&s), 4);
    press(&mut s, "ui_down");
    assert_eq!(cur(&s), 4, "nothing below: it stays");
    press(&mut s, "ui_left");
    assert_eq!(cur(&s), 3);
    press(&mut s, "ui_up");
    assert_eq!(cur(&s), 2);
    setr(&mut s, "action.ui_accept.pressed", 1.0);
    s.step();
    assert!(ui(&s, 2).clicked && !ui(&s, 3).clicked, "accept clicks the focused button");
}
