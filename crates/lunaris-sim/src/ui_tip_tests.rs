//! Tooltips: `UiTooltip` on the trigger, `UiTooltipBox` on the box that shows the text.
use super::*;
use components::{Ui, UiTooltipBox};
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

fn tip(s: &Sim) -> UiTooltipBox {
    *s.world.get::<UiTooltipBox>(s.world.entity(99).unwrap()).unwrap()
}

fn text(s: &Sim, uid: u64) -> String {
    s.world.strings().get(ui(s, uid).text.0).to_string()
}

fn pointer(s: &mut Sim, x: f64, y: f64, down: bool) {
    setr(s, "input.mouse_x", x);
    setr(s, "input.mouse_y", y);
    setr(s, "input.mouse_left", down as u8 as f64);
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

/// Two triggers, a sword (uid 1, 0.1..0.3 x 0.1..0.3, a half second delay) and a shield (uid 2, 0.6..0.8 x 0.1..0.3, no delay), and the box (uid 99, hidden until a tooltip is due).
fn scene() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.2,0.2]},"UiTooltip":{"text":"Iron sword\n+5 attack"}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.6,0.1],"size":[0.2,0.2],"button":true},"UiTooltip":{"text":"Shield {hp}","delay":0.0}}"#);
    spawn(&mut s, 99, r#"{"Ui":{"pos":[0.0,0.0],"size":[0.1,0.1],"visible":false,"text_size":0.04},"UiTooltipBox":{}}"#);
    s
}

#[test]
fn a_tooltip_appears_near_the_pointer_after_it_has_rested_for_the_delay() {
    let mut s = scene();
    s.step();
    assert!(!ui(&s, 99).visible, "hidden to start with");
    pointer(&mut s, 0.2, 0.2, false);
    s.step_n(20);
    assert!(!ui(&s, 99).visible && !tip(&s).shown, "a third of a second: not yet");
    s.step_n(5);
    assert!(!ui(&s, 99).visible, "0.4 s: not yet");
    s.step_n(15);
    assert!(ui(&s, 99).visible && tip(&s).shown, "after the delay");
    assert_eq!(text(&s, 99), "Iron sword\n+5 attack");
    assert_eq!(tip(&s).source.0, 1);
    let b = ui(&s, 99);
    assert!(close(b.pos[0], 0.2 + 0.012) && close(b.pos[1], 0.2 + 0.025), "near the pointer, offset down and right: {b:?}");
    // sized from the text: 10 letters of 0.75 * 0.04 over the aspect (1 here) plus the padding, and two lines
    assert!(close(b.size[0], 10.0 * 0.75 * 0.04 + 0.024) && close(b.size[1], 2.0 * 0.04 * 1.4 + 0.024), "{b:?}");
    // it stays where it came up while the pointer moves about the rectangle
    pointer(&mut s, 0.25, 0.25, false);
    s.step_n(3);
    assert!(ui(&s, 99).visible && close(ui(&s, 99).pos[0], 0.2 + 0.012));
}

#[test]
fn moving_the_pointer_about_starts_the_wait_again() {
    let mut s = scene();
    s.step();
    pointer(&mut s, 0.15, 0.15, false);
    s.step_n(25);
    pointer(&mut s, 0.2, 0.2, false); // moved: the wait starts again
    s.step_n(25);
    assert!(!tip(&s).shown, "25 + 25 ticks, but never 30 at rest");
    s.step_n(10);
    assert!(tip(&s).shown);
    // a nudge inside the resting radius does not count as moving
    let mut s = scene();
    s.step();
    pointer(&mut s, 0.2, 0.2, false);
    s.step_n(15);
    pointer(&mut s, 0.2 + 0.003, 0.2, false);
    s.step_n(20);
    assert!(tip(&s).shown);
}

#[test]
fn it_goes_when_the_pointer_leaves_or_a_button_is_pressed_and_comes_back_after_a_rest() {
    let mut s = scene();
    s.step();
    pointer(&mut s, 0.2, 0.2, false);
    s.step_n(40);
    assert!(tip(&s).shown);
    pointer(&mut s, 0.5, 0.5, false);
    s.step();
    assert!(!ui(&s, 99).visible && tip(&s).source.0 == 0, "left the rectangle: gone");
    // back, and a press while it is up takes it away until the pointer has left again
    pointer(&mut s, 0.2, 0.2, false);
    s.step_n(40);
    assert!(tip(&s).shown);
    pointer(&mut s, 0.2, 0.2, true);
    s.step();
    assert!(!ui(&s, 99).visible, "pressed: gone");
    pointer(&mut s, 0.2, 0.2, false);
    s.step_n(60);
    assert!(!tip(&s).shown, "and it does not come back while the pointer stays on the same thing");
    pointer(&mut s, 0.5, 0.5, false);
    s.step();
    pointer(&mut s, 0.2, 0.2, false);
    s.step_n(40);
    assert!(tip(&s).shown, "left and rested again");
}

#[test]
fn a_press_in_the_very_tick_the_pointer_arrives_keeps_the_tooltip_away_too() {
    let mut s = scene();
    s.step();
    pointer(&mut s, 0.2, 0.2, true);
    s.step();
    pointer(&mut s, 0.2, 0.2, false);
    s.step_n(60);
    assert!(!tip(&s).shown, "a click on the sword is not followed by a tooltip for it");
    pointer(&mut s, 0.5, 0.5, false);
    s.step();
    pointer(&mut s, 0.2, 0.2, false);
    s.step_n(40);
    assert!(tip(&s).shown, "after the pointer has been away and rested");
}

#[test]
fn another_trigger_has_its_own_text_and_delay() {
    let mut s = scene();
    s.step();
    pointer(&mut s, 0.2, 0.2, false);
    s.step_n(40);
    assert_eq!(tip(&s).source.0, 1);
    pointer(&mut s, 0.7, 0.2, false);
    s.step();
    assert!(!ui(&s, 99).visible, "the sword's tooltip went with the pointer");
    s.step_n(2);
    assert!(ui(&s, 99).visible, "the shield has no delay");
    assert_eq!(text(&s, 99), "Shield {hp}", "the template: the renderer fills the placeholder");
    assert_eq!(tip(&s).source.0, 2);
}

#[test]
fn near_the_edge_the_box_goes_to_the_other_side_of_the_pointer_and_stays_on_the_picture() {
    let mut s = scene();
    s.step();
    spawn(&mut s, 3, r#"{"Ui":{"pos":[0.8,0.7],"size":[0.2,0.3]},"UiTooltip":{"text":"Lower right corner","delay":0.0}}"#);
    pointer(&mut s, 0.95, 0.95, false);
    s.step_n(3);
    let b = ui(&s, 99);
    assert!(b.visible && b.pos[0] + b.size[0] <= 0.95 + 1e-4 && b.pos[1] + b.size[1] <= 0.95 + 1e-4, "above and to the left of the pointer: {b:?}");
    assert!(b.pos[0] >= 0.0 && b.pos[1] >= 0.0);
    // without `auto_size` the size is the box's own
    s.world.get_mut::<UiTooltipBox>(s.world.entity(99).unwrap()).unwrap().auto_size = false;
    s.world.get_mut::<Ui>(s.world.entity(99).unwrap()).unwrap().size = [0.3, 0.1];
    pointer(&mut s, 0.5, 0.5, false);
    s.step();
    pointer(&mut s, 0.9, 0.9, false);
    s.step_n(3);
    assert_eq!(ui(&s, 99).size, [0.3, 0.1]);
}

#[test]
fn a_hidden_or_scrolled_away_trigger_shows_nothing_and_the_clock_is_not_the_games() {
    let mut s = scene();
    spawn(&mut s, 5, r#"{"UiGroup":{"shown":false}}"#);
    spawn(&mut s, 6, r#"{"Ui":{"pos":[0.4,0.4],"size":[0.2,0.2]},"UiTooltip":{"text":"in a hidden panel","delay":0.0},"Parent":{"target":5}}"#);
    s.step();
    pointer(&mut s, 0.5, 0.5, false);
    s.step_n(10);
    assert!(!tip(&s).shown, "hidden by a group");
    // a trigger half out of a scroll view: only its visible half
    spawn(&mut s, 7, r#"{"Ui":{"pos":[0.0,0.6],"size":[0.3,0.2]},"UiScroll":{"content_size":[0.3,0.6]}}"#);
    spawn(&mut s, 8, r#"{"Ui":{"pos":[0.0,0.7],"size":[0.3,0.3]},"UiTooltip":{"text":"half","delay":0.0},"Parent":{"target":7}}"#);
    s.step();
    pointer(&mut s, 0.1, 0.9, false);
    s.step_n(5);
    assert!(!tip(&s).shown, "below the viewport: not under the pointer");
    pointer(&mut s, 0.1, 0.75, false);
    s.step_n(3);
    assert!(tip(&s).shown, "inside the viewport");
    // a game that is paused still has tooltips
    let mut s = scene();
    setr(&mut s, "time.scale", 0.0);
    s.step();
    pointer(&mut s, 0.2, 0.2, false);
    s.step_n(40);
    assert!(tip(&s).shown, "time.scale 0 is a pause menu, which is where tooltips are wanted");
}

#[test]
fn the_new_components_are_registered_with_their_documentation() {
    let s = Sim::new(1);
    for name in ["UiTooltip", "UiTooltipBox", "UiRadial"] {
        let id = s.world.component_id(name).unwrap_or_else(|| panic!("{name} is not registered"));
        assert!(s.world.info(id).doc.len() > 100, "{name} has a description for `schema`");
    }
}
