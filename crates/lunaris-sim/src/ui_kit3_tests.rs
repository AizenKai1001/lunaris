//! UI kit v3: scroll views and scroll bars (and, in the next commits, dropdowns, tabs, drag and drop, tooltips and the radial fill).
use super::*;
use components::{Text, Ui, UiScroll, UiScrollBar};
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

fn scroll(s: &Sim, uid: u64) -> UiScroll {
    *s.world.get::<UiScroll>(s.world.entity(uid).unwrap()).unwrap()
}

fn pointer(s: &mut Sim, x: f64, y: f64, down: bool) {
    setr(s, "input.mouse_x", x);
    setr(s, "input.mouse_y", y);
    setr(s, "input.mouse_left", down as u8 as f64);
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

/// A viewport (uid 1, x 0.1..0.5, y 0.1..0.5) over `n` buttons (uids 10...) each 0.1 high and 0.12 apart, the first at the viewport's top.
fn list(n: u64, extra: &str) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, &format!(r#"{{"Ui":{{"pos":[0.1,0.1],"size":[0.4,0.4]}},"UiScroll":{{{extra}}}}}"#));
    for i in 0..n {
        let y = 0.1 + i as f32 * 0.12;
        spawn(&mut s, 10 + i, &format!(r#"{{"Ui":{{"pos":[0.12,{y}],"size":[0.3,0.1],"button":true}},"Parent":{{"target":1}}}}"#));
    }
    s
}

fn wheel(s: &mut Sim, notches: f64, ticks: usize) {
    setr(s, "input.mouse_wheel", notches);
    s.step_n(ticks as u64);
    setr(s, "input.mouse_wheel", 0.0);
}

#[test]
fn the_wheel_moves_the_content_and_stops_at_both_ends() {
    let mut s = list(8, "");
    pointer(&mut s, 0.3, 0.3, false);
    s.step();
    // the content is measured from the children: from the viewport's top to the last button's bottom
    assert!(close(scroll(&s, 1).content[1], 0.1 + 7.0 * 0.12 + 0.1 - 0.1), "{:?}", scroll(&s, 1));
    assert!(close(ui(&s, 10).pos[1], 0.1) && scroll(&s, 1).offset == [0.0, 0.0]);
    // one notch down (the wheel turned towards you is negative) moves the content up by `speed`
    wheel(&mut s, -1.0, 1);
    assert!(close(scroll(&s, 1).offset[1], 0.06), "{:?}", scroll(&s, 1));
    assert!(close(ui(&s, 10).pos[1], 0.1 - 0.06) && close(ui(&s, 11).pos[1], 0.22 - 0.06), "the children moved with it");
    assert!(scroll(&s, 1).changed);
    s.step();
    assert!(!scroll(&s, 1).changed, "changed lasts the tick it moved");
    // as far as it goes: the last button's bottom edge meets the viewport's
    wheel(&mut s, -1.0, 30);
    let range = scroll(&s, 1).content[1] - 0.4;
    assert!(close(scroll(&s, 1).offset[1], range), "{:?}", scroll(&s, 1));
    assert!(close(ui(&s, 17).pos[1] + 0.1, 0.5), "the last one ends where the viewport ends: {:?}", ui(&s, 17));
    // and back: everything is where it was authored
    wheel(&mut s, 1.0, 40);
    assert_eq!(scroll(&s, 1).offset, [0.0, 0.0]);
    for i in 0..8u64 {
        assert!(close(ui(&s, 10 + i).pos[1], 0.1 + i as f32 * 0.12), "button {i}: {:?}", ui(&s, 10 + i));
    }
    // a pointer that is not over the view does not scroll it
    pointer(&mut s, 0.8, 0.8, false);
    wheel(&mut s, -1.0, 3);
    assert_eq!(scroll(&s, 1).offset, [0.0, 0.0]);
}

#[test]
fn the_innermost_scrollable_view_under_the_pointer_takes_the_wheel() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.0,0.0],"size":[1.0,1.0]},"UiScroll":{"content_size":[1.0,3.0]}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.4,0.3]},"UiScroll":{"content_size":[0.4,0.9]},"Parent":{"target":1}}"#);
    spawn(&mut s, 3, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.3,0.3]},"Parent":{"target":2}}"#);
    s.step();
    pointer(&mut s, 0.2, 0.2, false);
    wheel(&mut s, -1.0, 2);
    assert!(close(scroll(&s, 2).offset[1], 0.12) && scroll(&s, 1).offset == [0.0, 0.0], "{:?} {:?}", scroll(&s, 2), scroll(&s, 1));
    // outside the inner view the outer one scrolls
    pointer(&mut s, 0.8, 0.8, false);
    wheel(&mut s, -1.0, 1);
    assert!(close(scroll(&s, 1).offset[1], 0.06) && close(scroll(&s, 2).offset[1], 0.12), "{:?} {:?}", scroll(&s, 1), scroll(&s, 2));
    // the inner view's children moved with both: the inner offset and the outer one
    assert!(close(ui(&s, 3).pos[1], 0.1 - 0.12 - 0.06) || close(ui(&s, 3).pos[1], 0.1 - 0.06 - 0.12), "{:?}", ui(&s, 3));
}

#[test]
fn a_button_scrolled_out_of_the_viewport_does_not_react_from_there() {
    let mut s = list(6, "");
    s.step();
    // button 13 (y 0.46 to 0.56) pokes out of the bottom of the viewport (which ends at 0.5)
    pointer(&mut s, 0.3, 0.48, false);
    s.step();
    assert!(ui(&s, 13).hover, "the visible part answers the pointer");
    pointer(&mut s, 0.3, 0.52, false);
    s.step();
    assert!(!ui(&s, 13).hover, "the part outside the viewport does not");
    // a button that is wholly outside never reacts, pressed or released
    pointer(&mut s, 0.3, 0.75, true);
    s.step();
    pointer(&mut s, 0.3, 0.75, false);
    s.step();
    assert!(!ui(&s, 15).hover && !ui(&s, 15).clicked && !ui(&s, 15).pressed);
    // a click inside still works
    pointer(&mut s, 0.3, 0.15, true);
    s.step();
    pointer(&mut s, 0.3, 0.15, false);
    s.step();
    assert!(ui(&s, 10).clicked, "{:?}", ui(&s, 10));
}

#[test]
fn dragging_the_content_scrolls_it_and_is_not_a_click_but_a_still_press_is() {
    let mut s = list(8, "");
    s.step();
    s.world.get_mut::<UiScroll>(s.world.entity(1).unwrap()).unwrap().offset = [0.0, 0.3];
    s.step();
    // press on a button, pull it down 0.1: the content follows the pointer
    pointer(&mut s, 0.3, 0.3, true);
    s.step();
    assert_eq!(scroll(&s, 1).dragging, 1, "a press is only a possible drag until the pointer has moved");
    pointer(&mut s, 0.3, 0.4, true);
    s.step();
    assert_eq!(scroll(&s, 1).dragging, 2);
    assert!(close(scroll(&s, 1).offset[1], 0.2), "{:?}", scroll(&s, 1));
    for uid in 10..18 {
        assert!(!ui(&s, uid).pressed && !ui(&s, uid).clicked, "button {uid} is not being pressed while the content is dragged");
    }
    pointer(&mut s, 0.3, 0.4, false);
    s.step();
    for uid in 10..18 {
        assert!(!ui(&s, uid).clicked, "letting go after a drag is not a click on button {uid}");
    }
    assert_eq!(scroll(&s, 1).dragging, 0);
    assert!(close(scroll(&s, 1).offset[1], 0.2));
    // a press that does not move is a click, as ever
    pointer(&mut s, 0.3, 0.3, true);
    s.step();
    pointer(&mut s, 0.3, 0.3, false);
    s.step();
    let clicked: Vec<u64> = (10..18).filter(|u| ui(&s, *u).clicked).collect();
    assert_eq!(clicked.len(), 1, "{clicked:?}");
    assert!(close(scroll(&s, 1).offset[1], 0.2), "it did not scroll");
    // with `drag` off a drag does not scroll
    s.world.get_mut::<UiScroll>(s.world.entity(1).unwrap()).unwrap().drag = false;
    pointer(&mut s, 0.3, 0.3, true);
    s.step();
    pointer(&mut s, 0.3, 0.45, true);
    s.step();
    assert!(close(scroll(&s, 1).offset[1], 0.2));
}

#[test]
fn moving_the_focus_scrolls_the_focused_button_into_view() {
    let mut s = list(8, "");
    spawn(&mut s, 2, r#"{"UiNav":{},"UiNavAuto":{}}"#);
    s.step();
    let press = |s: &mut Sim, name: &str| {
        setr(s, &format!("action.{name}.pressed"), 1.0);
        s.step();
        setr(s, &format!("action.{name}.pressed"), 0.0);
        s.step();
    };
    assert_eq!(s.world.get::<components::UiNav>(s.world.entity(2).unwrap()).unwrap().current.0, 10);
    for _ in 0..6 {
        press(&mut s, "ui_down");
    }
    let focused = s.world.get::<components::UiNav>(s.world.entity(2).unwrap()).unwrap().current.0;
    assert_eq!(focused, 16);
    let f = ui(&s, focused);
    assert!(f.pos[1] >= 0.1 - 1e-4 && f.pos[1] + f.size[1] <= 0.5 + 1e-4, "the focused button is inside the viewport: {f:?} offset {:?}", scroll(&s, 1).offset);
    assert!(scroll(&s, 1).offset[1] > 0.0);
    // back up: the view follows the focus upwards too
    for _ in 0..6 {
        press(&mut s, "ui_up");
    }
    assert_eq!(s.world.get::<components::UiNav>(s.world.entity(2).unwrap()).unwrap().current.0, 10);
    assert!(close(scroll(&s, 1).offset[1], 0.0), "{:?}", scroll(&s, 1));
    // a wheel scroll away from the focus is left alone (the focus did not change)
    pointer(&mut s, 0.3, 0.3, false);
    wheel(&mut s, -1.0, 2);
    let o = scroll(&s, 1).offset[1];
    s.step_n(3);
    assert!(o > 0.0 && close(scroll(&s, 1).offset[1], o));
}

#[test]
fn the_right_stick_scrolls_the_view_that_holds_the_focus() {
    let mut s = list(8, "");
    spawn(&mut s, 2, r#"{"UiNav":{},"UiNavAuto":{}}"#);
    s.step_n(2);
    setr(&mut s, "input.gamepad.ry", -1.0); // pushed down
    s.step_n(4);
    assert!(scroll(&s, 1).offset[1] > 0.0, "{:?}", scroll(&s, 1));
    setr(&mut s, "input.gamepad.ry", 0.1); // inside the dead zone
    let before = scroll(&s, 1).offset[1];
    s.step_n(4);
    assert!(close(scroll(&s, 1).offset[1], before));
}

#[test]
fn a_scroll_bar_shows_where_the_view_is_and_can_be_dragged() {
    let mut s = list(8, "");
    spawn(&mut s, 2, r#"{"Ui":{"button":true},"UiScrollBar":{"view":1,"track":3,"thickness":0.02,"inset":0.0,"min_length":0.02}}"#);
    spawn(&mut s, 3, r#"{"Ui":{"button":true}}"#);
    s.step();
    // track: the right edge of the viewport; thumb: the share of the content that is in view, at the top
    let (track, thumb) = (ui(&s, 3), ui(&s, 2));
    assert!(close(track.pos[0] + track.size[0], 0.5) && close(track.pos[1], 0.1) && close(track.size[1], 0.4), "{track:?}");
    let content = scroll(&s, 1).content[1];
    assert!(close(thumb.size[1], 0.4 * 0.4 / content) && close(thumb.pos[1], 0.1) && thumb.visible && track.visible, "{thumb:?}");
    // scrolling to the end puts the thumb at the end of the track
    pointer(&mut s, 0.3, 0.3, false);
    wheel(&mut s, -1.0, 40);
    let thumb = ui(&s, 2);
    assert!(close(thumb.pos[1] + thumb.size[1], 0.5), "{thumb:?}");
    // press the thumb in the middle and pull it up by a quarter of the track: the offset follows in proportion
    let range = content - 0.4;
    let mid = (thumb.pos[1] + thumb.size[1] * 0.5) as f64;
    let x = (thumb.pos[0] + thumb.size[0] * 0.5) as f64;
    pointer(&mut s, x, mid, true);
    s.step();
    assert_eq!(scroll(&s, 1).dragging, 3, "the thumb of the vertical bar is held");
    pointer(&mut s, x - 0.3, mid - 0.1, true); // also off to the side: a thumb is held until the button is let go
    s.step();
    let travel = 0.4 - thumb.size[1];
    assert!(close(scroll(&s, 1).offset[1], range - 0.1 / travel * range), "{:?} range {range}", scroll(&s, 1));
    // the thumb stays under the pointer, the content did not get a drag of its own
    assert!(close(ui(&s, 2).pos[1] + ui(&s, 2).size[1] * 0.5, (mid - 0.1) as f32), "{:?}", ui(&s, 2));
    pointer(&mut s, x, mid - 0.1, false);
    s.step();
    assert_eq!(scroll(&s, 1).dragging, 0);
    // pressing the track away from the thumb puts the thumb's middle under the pointer
    pointer(&mut s, x, 0.45, true);
    s.step();
    let t = ui(&s, 2);
    assert!((t.pos[1] + t.size[1] * 0.5 - 0.45).abs() < 0.01 || t.pos[1] + t.size[1] >= 0.5 - 1e-4, "{t:?}");
    pointer(&mut s, x, 0.45, false);
    s.step();
    // content that fits hides the bar
    for uid in 11..18 {
        s.world.get_mut::<Ui>(s.world.entity(uid).unwrap()).unwrap().visible = false;
    }
    s.step_n(2);
    assert!(!ui(&s, 2).visible && !ui(&s, 3).visible, "nothing to scroll: no bar");
    assert_eq!(scroll(&s, 1).offset, [0.0, 0.0]);
}

#[test]
fn horizontal_scrolling_with_a_given_content_size_and_text_children() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.3,0.2]},"UiScroll":{"content_size":[0.9,0.0]}}"#);
    spawn(&mut s, 2, r#"{"Text":{"value":"hello","pos":[0.12,0.15],"size":0.04},"Parent":{"target":1}}"#);
    spawn(&mut s, 3, r#"{"Ui":{"pos":[0.6,0.12],"size":[0.2,0.1]},"Parent":{"target":1}}"#);
    s.step();
    // the content is 0.9 wide as said, and as high as the children measure (they all fit, so there is no vertical range)
    assert!(close(scroll(&s, 1).content[0], 0.9) && scroll(&s, 1).content[1] <= 0.2 + 1e-4, "{:?}", scroll(&s, 1));
    pointer(&mut s, 0.2, 0.2, false);
    // with no vertical range the wheel scrolls sideways (a notch is `speed` over the aspect, and the aspect here is 1)
    wheel(&mut s, -1.0, 2);
    assert!(close(scroll(&s, 1).offset[0], 0.12) && scroll(&s, 1).offset[1] == 0.0, "{:?}", scroll(&s, 1));
    let t = *s.world.get::<Text>(s.world.entity(2).unwrap()).unwrap();
    assert!(close(t.pos[0], 0.12 - 0.12) && close(t.pos[1], 0.15), "the text moved too: {t:?}");
    assert!(close(ui(&s, 3).pos[0], 0.6 - 0.12));
    // the end of the range is the content's width minus the viewport's
    wheel(&mut s, -1.0, 50);
    assert!(close(scroll(&s, 1).offset[0], 0.9 - 0.3));
}

#[test]
fn clip_rectangles_are_the_viewports_of_every_view_above_and_leave_scroll_bars_alone() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.6,0.6]},"UiScroll":{}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.3,0.2],"size":[0.6,0.3]},"UiScroll":{},"Parent":{"target":1}}"#);
    spawn(&mut s, 3, r#"{"Ui":{"pos":[0.2,0.2],"size":[0.1,0.1]},"Parent":{"target":2}}"#);
    spawn(&mut s, 4, r#"{"Ui":{"pos":[0.2,0.2],"size":[0.1,0.1]},"Parent":{"target":1}}"#);
    spawn(&mut s, 5, r#"{"Ui":{"pos":[0.2,0.2],"size":[0.1,0.1]},"UiScrollBar":{"view":1},"Parent":{"target":1}}"#);
    spawn(&mut s, 6, r#"{"Ui":{"pos":[0.9,0.9],"size":[0.1,0.1]}}"#);
    let clips = ui_kit3::clip_rects(&mut s.world);
    let c = |k: u64| clips.get(&k).copied();
    let near = |got: Option<[f32; 4]>, want: [f32; 4]| got.is_some_and(|v| v.iter().zip(want).all(|(a, b)| close(*a, b)));
    // the inner view is clipped by the outer one; what is in the inner one by both (the intersection)
    assert!(near(c(2), [0.1, 0.1, 0.7, 0.7]), "{:?}", c(2));
    assert!(near(c(3), [0.3, 0.2, 0.7, 0.5]), "{:?}", c(3));
    assert!(near(c(4), [0.1, 0.1, 0.7, 0.7]), "{:?}", c(4));
    assert_eq!(c(5), None, "a scroll bar is not clipped by its own view");
    assert_eq!(c(1), None);
    assert_eq!(c(6), None);
    assert!(ui_kit3::clip_rects(&mut Sim::new(1).world).is_empty());
}

#[test]
fn scrolling_is_deterministic_and_a_world_without_it_is_untouched() {
    let run = || {
        let mut s = list(8, "");
        spawn(&mut s, 2, r#"{"Ui":{"button":true},"UiScrollBar":{"view":1}}"#);
        pointer(&mut s, 0.3, 0.3, false);
        for t in 0..60u64 {
            setr(&mut s, "input.mouse_wheel", if t % 7 < 3 { -1.0 } else { 0.5 });
            s.step();
        }
        (s.world.state_hash(), scroll(&s, 1).offset)
    };
    assert_eq!(run(), run());
    // a world of plain buttons: stepping with the kit in place changes nothing about them
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.3,0.1],"button":true}}"#);
    let before = ui(&s, 1);
    setr(&mut s, "input.mouse_wheel", -1.0);
    s.step_n(5);
    assert_eq!(ui(&s, 1), before);
}

#[test]
fn a_layout_column_inside_a_view_is_laid_out_and_scrolled_together() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.4,0.3]},"UiScroll":{}}"#);
    // the content: a column twice as high as the viewport, four rows
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.4,0.8]},"UiLayout":{"kind":0,"padding":0.0,"spacing":0.0},"Parent":{"target":1}}"#);
    for i in 0..4u64 {
        spawn(&mut s, 3 + i, r#"{"Ui":{"button":true},"Parent":{"target":2}}"#);
    }
    s.step_n(2);
    assert!(close(ui(&s, 3).pos[1], 0.1) && close(ui(&s, 3).size[1], 0.2) && close(ui(&s, 6).pos[1], 0.7), "{:?} {:?}", ui(&s, 3), ui(&s, 6));
    pointer(&mut s, 0.3, 0.2, false);
    wheel(&mut s, -1.0, 3);
    let o = scroll(&s, 1).offset[1];
    assert!(close(o, 0.18), "{:?}", scroll(&s, 1));
    // the layout keeps placing the rows in the container, which moved: they all moved by the offset, after a few ticks too
    s.step_n(3);
    for i in 0..4u64 {
        assert!(close(ui(&s, 3 + i).pos[1], 0.1 + i as f32 * 0.2 - o), "row {i}: {:?}", ui(&s, 3 + i));
        assert!(close(ui(&s, 3 + i).size[1], 0.2));
    }
    assert!(close(scroll(&s, 1).content[1], 0.8), "the container is the content: {:?}", scroll(&s, 1));
}

#[test]
fn a_press_on_a_slider_in_a_view_moves_the_slider_not_the_content() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.4,0.4]},"UiScroll":{"content_size":[0.4,1.0]}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.15,0.2],"size":[0.3,0.06],"button":true},"UiSlider":{"min":0,"max":10,"value":0},"Parent":{"target":1}}"#);
    s.step_n(2);
    pointer(&mut s, 0.3, 0.23, true);
    s.step();
    pointer(&mut s, 0.4, 0.4, true); // down and to the right: the slider follows x, and the content must not follow y
    s.step_n(2);
    let v = s.world.get::<components::UiSlider>(s.world.entity(2).unwrap()).unwrap().value;
    assert!(v > 5.0, "the slider moved: {v}");
    assert_eq!(scroll(&s, 1).offset, [0.0, 0.0], "the content did not");
    assert_eq!(scroll(&s, 1).dragging, 0);
}

#[test]
fn what_a_group_hides_is_not_scroll_content() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.4,0.4]},"UiScroll":{}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.12,0.1],"size":[0.3,0.1]},"Parent":{"target":1}}"#);
    spawn(&mut s, 3, r#"{"UiGroup":{"shown":false},"Parent":{"target":1}}"#);
    spawn(&mut s, 4, r#"{"Ui":{"pos":[0.12,0.9],"size":[0.3,0.1]},"Parent":{"target":3}}"#);
    s.step();
    assert!(close(scroll(&s, 1).content[1], 0.1), "only the shown row counts: {:?}", scroll(&s, 1));
    s.world.get_mut::<components::UiGroup>(s.world.entity(3).unwrap()).unwrap().shown = true;
    s.step();
    assert!(close(scroll(&s, 1).content[1], 0.9), "{:?}", scroll(&s, 1));
}

#[test]
fn the_bar_components_are_registered_with_their_documentation() {
    let s = Sim::new(1);
    for name in ["UiScroll", "UiScrollBar"] {
        let id = s.world.component_id(name).unwrap_or_else(|| panic!("{name} is not registered"));
        assert!(s.world.info(id).doc.len() > 100, "{name} has a description for `schema`");
    }
    let _ = UiScrollBar::default();
}

#[test]
fn the_shared_hidden_set_is_always_the_one_made_from_scratch_whatever_changes() {
    use crate::ui_kit3::{hidden_compute, hidden_set_shared};
    use std::collections::BTreeSet;
    use std::sync::Arc;
    let mut s = Sim::new(1);
    // two groups, ten children, a grandchild each, and an element with no parent
    spawn(&mut s, 1, r#"{"Ui":{},"UiGroup":{"shown":true}}"#);
    spawn(&mut s, 2, r#"{"Ui":{},"UiGroup":{"shown":true}}"#);
    for k in 0..10u64 {
        spawn(&mut s, 10 + k, &format!(r#"{{"Ui":{{}},"Parent":{{"target":{}}}}}"#, 1 + k % 2));
        spawn(&mut s, 30 + k, &format!(r#"{{"Ui":{{}},"Parent":{{"target":{}}}}}"#, 10 + k));
    }
    spawn(&mut s, 50, r#"{"Ui":{}}"#);
    let check = |s: &mut Sim, what: &str| {
        let shared = hidden_set_shared(&mut s.world);
        let mut off = BTreeSet::new();
        let _ = s.world.for_each_uid::<(&components::UiGroup,), _>(|_, uid, (g,)| {
            if !g.shown {
                off.insert(uid);
            }
        });
        assert_eq!(*shared, hidden_compute(&mut s.world, off), "{what}");
        shared
    };
    let set = |s: &mut Sim, uid: u64, component: &str, json: &str| s.apply(&Command::Set { uid, component: component.into(), value: parse(json).unwrap() }).unwrap();
    assert!(check(&mut s, "all shown").is_empty());
    set(&mut s, 1, "UiGroup", r#"{"shown":false}"#);
    let a = check(&mut s, "group 1 off");
    assert!(a.contains(&1) && a.contains(&10) && a.contains(&30) && !a.contains(&11) && !a.contains(&50));
    // nothing changed: the same set, not a new one
    assert!(Arc::ptr_eq(&a, &check(&mut s, "again")));
    set(&mut s, 2, "UiGroup", r#"{"shown":false}"#);
    check(&mut s, "both off");
    // an element moves under the other group, one under the loose element, a parent goes away, another comes
    set(&mut s, 12, "Parent", r#"{"target":2}"#);
    check(&mut s, "12 under group 2");
    set(&mut s, 13, "Parent", r#"{"target":50}"#);
    check(&mut s, "13 under the loose element");
    set(&mut s, 1, "UiGroup", r#"{"shown":true}"#);
    let b = check(&mut s, "group 1 back");
    assert!(!b.contains(&10) && b.contains(&11) && b.contains(&31) && !b.contains(&33), "{b:?}");
    s.apply(&Command::Despawn { uid: 11 }).unwrap();
    check(&mut s, "a child despawned");
    set(&mut s, 50, "Ui", r#"{}"#);
    spawn(&mut s, 60, r#"{"Ui":{},"Parent":{"target":50}}"#);
    check(&mut s, "a child of the loose element");
    s.apply(&Command::Add { uid: 50, component: "Parent".into(), value: parse(r#"{"target":2}"#).unwrap() }).unwrap();
    let c = check(&mut s, "the loose element under group 2");
    assert!(c.contains(&50) && c.contains(&60) && c.contains(&13) && c.contains(&33), "{c:?}");
    set(&mut s, 2, "UiGroup", r#"{"shown":true}"#);
    assert!(check(&mut s, "everything shown again").is_empty());
}
