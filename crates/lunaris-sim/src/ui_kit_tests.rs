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

#[test]
fn a_menu_with_auto_focus_takes_the_focus_by_itself_and_hands_it_on() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"UiNav":{},"UiNavAuto":{}}"#);
    // a pause menu: two buttons that are hidden until it is wanted
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.3,0.3],"size":[0.4,0.1],"button":true,"visible":false}}"#);
    spawn(&mut s, 3, r#"{"Ui":{"pos":[0.3,0.5],"size":[0.4,0.1],"button":true,"visible":false}}"#);
    let cur = |s: &Sim| s.world.get::<components::UiNav>(s.world.entity(1).unwrap()).unwrap().current.0;
    let show = |s: &mut Sim, uid: u64, visible: bool| s.world.get_mut::<Ui>(s.world.entity(uid).unwrap()).unwrap().visible = visible;
    s.step_n(2);
    assert_eq!(cur(&s), 0, "no visible button: nothing to focus");
    // the menu appears: the first button is focused in the same tick, and accept clicks it at once (no direction key first)
    show(&mut s, 2, true);
    show(&mut s, 3, true);
    setr(&mut s, "action.ui_accept.pressed", 1.0);
    s.step();
    setr(&mut s, "action.ui_accept.pressed", 0.0);
    assert_eq!(cur(&s), 2);
    assert!(ui(&s, 2).hover && ui(&s, 2).clicked && !ui(&s, 3).clicked, "{:?}", ui(&s, 2));
    // the focus moves on from there with the direction actions as ever
    setr(&mut s, "action.ui_down.pressed", 1.0);
    s.step();
    setr(&mut s, "action.ui_down.pressed", 0.0);
    assert_eq!(cur(&s), 3);
    // the focused button goes away: the focus is handed to the next visible one, and then to none
    show(&mut s, 3, false);
    s.step();
    assert_eq!(cur(&s), 2);
    show(&mut s, 2, false);
    s.step();
    assert_eq!(cur(&s), 0);
    // switched off, it is the old behaviour: the buttons come back and nothing is focused until a direction is pressed
    s.world.get_mut::<components::UiNavAuto>(s.world.entity(1).unwrap()).unwrap().enabled = false;
    show(&mut s, 2, true);
    s.step();
    assert_eq!(cur(&s), 0);
}

#[test]
fn without_the_component_nothing_changes_for_an_existing_menu() {
    // the same scene with and without a `UiNavAuto { enabled: false }` runs the same
    let run = |extra: &str| {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, &format!(r#"{{"UiNav":{{}}{extra}}}"#));
        spawn(&mut s, 2, r#"{"Ui":{"pos":[0.3,0.3],"size":[0.4,0.1],"button":true}}"#);
        s.step_n(5);
        s.world.get::<components::UiNav>(s.world.entity(1).unwrap()).unwrap().current.0
    };
    assert_eq!((run(""), run(r#","UiNavAuto":{"enabled":false}"#), run(r#","UiNavAuto":{}"#)), (0, 0, 2));
}

#[test]
fn the_safe_area_is_the_biggest_picture_of_the_made_for_shape_in_the_middle() {
    use crate::ui_kit::safe_area;
    let sixteen_nine = 16.0 / 9.0;
    assert_eq!(safe_area(sixteen_nine, 1920, 1080), (0, 0, 1920, 1080), "the shape it was made for: all of it");
    assert_eq!(safe_area(sixteen_nine, 640, 480), (0, 60, 640, 360), "4:3: bars above and below");
    assert_eq!(safe_area(sixteen_nine, 840, 360), (100, 0, 640, 360), "21:9: bars at the sides");
    assert_eq!(safe_area(sixteen_nine, 900, 1600), (0, 547, 900, 506), "a phone held upright: a thin strip");
    assert_eq!(safe_area(1.0, 1000, 600), (200, 0, 600, 600));
    // nonsense is the whole picture
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY, 1.0e9] {
        assert_eq!(safe_area(bad, 640, 480), (0, 0, 640, 480), "{bad}");
    }
    assert_eq!(safe_area(sixteen_nine, 0, 480), (0, 0, 0, 480));
}

#[test]
fn the_pointer_is_in_fractions_of_the_safe_area_and_outside_zero_to_one_over_the_margins() {
    use crate::ui_kit::pointer_to_ui;
    let close = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4;
    let a = Some(16.0 / 9.0);
    // no scale: the fractions of the window, kept inside
    assert!(close(pointer_to_ui(None, 420.0, 180.0, 840, 360), (0.5, 0.5)));
    assert!(close(pointer_to_ui(None, 900.0, -5.0, 840, 360), (1.0, 0.0)));
    // 21:9 window, 100 pixel margins: the middle is the middle, the left edge of the safe area is 0, a pixel in the margin is below 0
    assert!(close(pointer_to_ui(a, 420.0, 180.0, 840, 360), (0.5, 0.5)));
    assert!(close(pointer_to_ui(a, 100.0, 0.0, 840, 360), (0.0, 0.0)));
    assert!(close(pointer_to_ui(a, 740.0, 360.0, 840, 360), (1.0, 1.0)));
    assert!(pointer_to_ui(a, 50.0, 180.0, 840, 360).0 < 0.0 && pointer_to_ui(a, 790.0, 180.0, 840, 360).0 > 1.0);
    // 4:3 window: the margins are above and below
    assert!(close(pointer_to_ui(a, 320.0, 240.0, 640, 480), (0.5, 0.5)));
    assert!(pointer_to_ui(a, 320.0, 30.0, 640, 480).1 < 0.0);
}

#[test]
fn a_button_is_clicked_where_the_picture_shows_it_and_not_from_the_margin() {
    // a button at the left edge of the interface and the pointer positions a 21:9 window gives
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"UiScale":{"aspect":1.7777778}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.0,0.4],"size":[0.1,0.2],"button":true}}"#);
    let aspect = crate::ui_kit::ui_scale_aspect(&mut s.world);
    assert!(aspect.is_some_and(|a| (a - 16.0 / 9.0).abs() < 1e-4));
    for (px, py, hit) in [(110.0, 180.0, true), (60.0, 180.0, false), (0.0, 180.0, false), (200.0, 180.0, false)] {
        let (x, y) = crate::ui_kit::pointer_to_ui(aspect, px, py, 840, 360);
        pointer(&mut s, x as f64, y as f64, false);
        s.step();
        assert_eq!(ui(&s, 2).hover, hit, "pointer at pixel {px}: ({x}, {y})");
    }
}

#[test]
fn a_layout_container_pads_in_the_shape_of_the_scale_whatever_screen_aspect_says() {
    let place = |scale: bool| {
        let mut s = Sim::new(1);
        if scale {
            spawn(&mut s, 9, r#"{"UiScale":{"aspect":2.0}}"#);
        }
        setr(&mut s, "screen.aspect", 1.0);
        spawn(&mut s, 1, r#"{"Ui":{"pos":[0.0,0.0],"size":[1.0,1.0]},"UiLayout":{"kind":0,"padding":0.1,"spacing":0.0}}"#);
        spawn(&mut s, 2, r#"{"Ui":{},"Parent":{"target":1}}"#);
        s.step();
        ui(&s, 2)
    };
    // the padding is a tenth of the height: in width that is a tenth over the aspect, and the aspect is the scale's when there is one
    let (plain, scaled) = (place(false), place(true));
    assert!(close(plain.pos[0], 0.1) && close(scaled.pos[0], 0.05), "{plain:?} {scaled:?}");
    assert!(close(plain.pos[1], 0.1) && close(scaled.pos[1], 0.1));
}

#[test]
fn text_typed_by_a_window_goes_into_the_focused_field_at_once() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.4,0.1],"button":true},"UiInput":{"max_len":600,"show_last":6}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.1,0.3],"size":[0.4,0.1],"button":true},"UiInput":{"max_len":4}}"#);
    let text = |s: &mut Sim, uid: u64| {
        let e = s.world.entity(uid).unwrap();
        let t = s.world.get::<UiInput>(e).unwrap().text.0;
        s.world.strings().get(t).to_string()
    };
    let shown = |s: &Sim, uid: u64| s.world.strings().get(ui(s, uid).text.0).to_string();
    s.step();
    assert_eq!(super::ui_kit::type_into_focused(&mut s.world, "abc"), 0, "no field has the focus: nothing is taken");
    assert!(!super::ui_kit::has_focused_input(&mut s.world));
    pointer(&mut s, 0.3, 0.15, true);
    s.step();
    pointer(&mut s, 0.3, 0.15, false);
    s.step();
    assert!(super::ui_kit::has_focused_input(&mut s.world));
    let token: String = (0..400).map(|i| char::from(b"0123456789abcdef"[i % 16])).collect();
    assert_eq!(super::ui_kit::type_into_focused(&mut s.world, &token), 400, "a pasted token is taken whole, not a character a tick");
    assert_eq!(super::ui_kit::type_into_focused(&mut s.world, "x\ny\tz"), 3, "control characters are left out");
    s.step();
    assert_eq!(text(&mut s, 1).chars().count(), 403);
    assert!(text(&mut s, 1).ends_with("xyz"));
    let on_screen = shown(&s, 1);
    assert!(on_screen.starts_with("...") && on_screen.contains("def") && on_screen.trim_end_matches('|').chars().count() == 9, "only the last 6 are shown after the dots: {on_screen}");
    assert!(super::ui_kit::clear_focused(&mut s.world));
    s.step();
    assert_eq!(text(&mut s, 1), "");
    // the field's own limit holds for typed text too
    pointer(&mut s, 0.3, 0.35, true);
    s.step();
    pointer(&mut s, 0.3, 0.35, false);
    s.step();
    assert_eq!(super::ui_kit::type_into_focused(&mut s.world, "hello"), 4);
    assert_eq!(text(&mut s, 2), "hell");
    assert_eq!(text(&mut s, 1), "", "the other field got nothing");
}
