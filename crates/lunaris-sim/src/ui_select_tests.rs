//! Groups, drop-downs and tabs.
use super::*;
use components::{Ui, UiDropdown, UiGroup, UiNav, UiTabs};
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

fn dd(s: &Sim, uid: u64) -> UiDropdown {
    *s.world.get::<UiDropdown>(s.world.entity(uid).unwrap()).unwrap()
}

fn pointer(s: &mut Sim, x: f64, y: f64, down: bool) {
    setr(s, "input.mouse_x", x);
    setr(s, "input.mouse_y", y);
    setr(s, "input.mouse_left", down as u8 as f64);
}

fn text(s: &Sim, uid: u64) -> String {
    s.world.strings().get(ui(s, uid).text.0).to_string()
}

fn click(s: &mut Sim, x: f64, y: f64) {
    pointer(s, x, y, true);
    s.step();
    pointer(s, x, y, false);
    s.step();
}

fn shown(s: &Sim, uid: u64) -> bool {
    s.world.get::<UiGroup>(s.world.entity(uid).unwrap()).unwrap().shown
}

/// A drop-down: header 1 (0.1..0.4 x 0.1..0.18), its list 2 under it (0.1..0.4 x 0.18..0.48, three rows laid out in a column), options 3, 4, 5, and optionally a button 9 that the open list covers and a button 10 away from it.
fn dropdown_scene(bystanders: bool) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.3,0.08],"button":true,"text":"Quality"},"UiDropdown":{"list":2,"resource":"ui.quality","selected":1}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.1,0.18],"size":[0.3,0.3]},"UiLayout":{"kind":0,"padding":0.01,"spacing":0.01},"UiGroup":{"shown":true}}"#);
    for (i, name) in ["Low", "Medium", "High"].iter().enumerate() {
        spawn(&mut s, 3 + i as u64, &format!(r#"{{"Ui":{{"button":true,"text":"{name}"}},"UiOption":{{"dropdown":1,"index":{i}}},"Parent":{{"target":2}}}}"#));
    }
    if bystanders {
        spawn(&mut s, 9, r#"{"Ui":{"pos":[0.1,0.3],"size":[0.3,0.1],"button":true}}"#);
        spawn(&mut s, 10, r#"{"Ui":{"pos":[0.6,0.1],"size":[0.2,0.1],"button":true}}"#);
    }
    s
}

#[test]
fn a_group_hides_everything_below_it_from_the_pointer_the_focus_and_the_layout() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Ui":{"pos":[0.0,0.0],"size":[1.0,1.0],"color":[0,0,0,0]},"UiGroup":{}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.3,0.1],"button":true},"Parent":{"target":1}}"#);
    spawn(&mut s, 3, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.1,0.1],"button":true},"Parent":{"target":2}}"#);
    spawn(&mut s, 4, r#"{"UiNav":{},"UiNavAuto":{}}"#);
    pointer(&mut s, 0.15, 0.15, false);
    s.step();
    assert!(ui(&s, 2).hover && ui(&s, 3).hover, "shown: the pointer is over both");
    assert_eq!(s.world.get::<UiNav>(s.world.entity(4).unwrap()).unwrap().current.0, 2, "and the focus took the first button");
    s.world.get_mut::<UiGroup>(s.world.entity(1).unwrap()).unwrap().shown = false;
    pointer(&mut s, 0.15, 0.15, true);
    s.step();
    assert!(!ui(&s, 2).hover && !ui(&s, 3).hover && !ui(&s, 2).pressed && !ui(&s, 3).pressed, "hidden: nobody is under the pointer");
    pointer(&mut s, 0.15, 0.15, false);
    s.step();
    assert!(!ui(&s, 2).clicked && !ui(&s, 3).clicked);
    assert_eq!(s.world.get::<UiNav>(s.world.entity(4).unwrap()).unwrap().current.0, 0, "and nothing is focusable");
    assert!(ui(&s, 2).visible && ui(&s, 3).visible, "their own visible flags are not touched");
    // a layout gives no room to a hidden child
    spawn(&mut s, 5, r#"{"Ui":{"pos":[0.0,0.5],"size":[0.4,0.4]},"UiLayout":{"kind":0,"padding":0.0,"spacing":0.0}}"#);
    spawn(&mut s, 6, r#"{"Ui":{},"Parent":{"target":5}}"#);
    spawn(&mut s, 7, r#"{"Ui":{},"UiGroup":{"shown":false},"Parent":{"target":5}}"#);
    s.step();
    assert!((ui(&s, 6).size[1] - 0.4).abs() < 1e-4, "the one that shows has the whole column: {:?}", ui(&s, 6));
    // shown again: everything is back, as it was
    s.world.get_mut::<UiGroup>(s.world.entity(1).unwrap()).unwrap().shown = true;
    s.step();
    assert!(ui(&s, 2).hover && ui(&s, 3).hover);
}

#[test]
fn a_click_on_the_header_opens_the_list_and_a_click_on_an_option_picks_it() {
    let mut s = dropdown_scene(false);
    s.step();
    assert_eq!(text(&s, 1), "Medium", "the header says what is picked (index 1)");
    assert_eq!(s.world.resource("ui.quality"), Some(1.0), "and the resource has it from the first tick");
    assert!(!dd(&s, 1).open && !shown(&s, 2), "closed: the list is hidden");
    click(&mut s, 0.2, 0.14);
    assert!(dd(&s, 1).open && shown(&s, 2), "a click on the header opens the list");
    s.step();
    // the options sit in the list, in order
    let (a, b, c) = (ui(&s, 3), ui(&s, 4), ui(&s, 5));
    assert!(a.pos[1] < b.pos[1] && b.pos[1] < c.pos[1] && a.pos[1] >= 0.18, "{a:?} {b:?} {c:?}");
    let y = (c.pos[1] + c.size[1] * 0.5) as f64;
    pointer(&mut s, 0.2, y, true);
    s.step();
    pointer(&mut s, 0.2, y, false);
    s.step();
    assert_eq!(dd(&s, 1).selected, 2);
    assert!(dd(&s, 1).changed, "changed for the tick of the pick");
    assert!(!dd(&s, 1).open && !shown(&s, 2), "picking closes the list");
    assert_eq!(text(&s, 1), "High");
    assert_eq!(s.world.resource("ui.quality"), Some(2.0));
    s.step();
    assert!(!dd(&s, 1).changed, "one tick only");
    // picking what is picked already changes nothing, but closes
    click(&mut s, 0.2, 0.14);
    s.step();
    let y = (ui(&s, 5).pos[1] + 0.02) as f64;
    click(&mut s, 0.2, y);
    assert!(!dd(&s, 1).open && !dd(&s, 1).changed && dd(&s, 1).selected == 2);
    // a click on the header while it is open closes it
    click(&mut s, 0.2, 0.14);
    assert!(dd(&s, 1).open);
    click(&mut s, 0.2, 0.14);
    assert!(!dd(&s, 1).open && dd(&s, 1).selected == 2);
    // the outside can preselect
    s.world.get_mut::<UiDropdown>(s.world.entity(1).unwrap()).unwrap().selected = 0;
    s.step();
    assert_eq!((text(&s, 1).as_str(), s.world.resource("ui.quality")), ("Low", Some(0.0)));
}

#[test]
fn a_press_outside_closes_the_list_and_goes_no_further_and_the_list_covers_what_is_under_it() {
    let mut s = dropdown_scene(true);
    s.step();
    click(&mut s, 0.2, 0.14);
    s.step();
    assert!(dd(&s, 1).open);
    // button 9 is under the list: the pointer over an option that covers it does not reach it (and the option is hovered)
    let y = (ui(&s, 4).pos[1] + 0.02) as f64;
    pointer(&mut s, 0.2, y, false);
    s.step();
    assert!(ui(&s, 4).hover && !ui(&s, 9).hover, "the list covers button 9 where it is");
    pointer(&mut s, 0.2, y, true);
    s.step();
    pointer(&mut s, 0.2, y, false);
    s.step();
    assert!(!ui(&s, 9).clicked && dd(&s, 1).selected == 1, "{:?}", ui(&s, 4));
    // open again; a press on button 10 (away from the list) closes the list and is not a click on it
    click(&mut s, 0.2, 0.14);
    s.step();
    assert!(dd(&s, 1).open);
    pointer(&mut s, 0.7, 0.15, true);
    s.step();
    assert!(!dd(&s, 1).open && !shown(&s, 2), "pressed outside: closed");
    assert!(!ui(&s, 10).pressed && !ui(&s, 10).hover, "and button 10 does not see the press: {:?}", ui(&s, 10));
    s.step();
    assert!(!ui(&s, 10).pressed, "still held: still not seen");
    pointer(&mut s, 0.7, 0.15, false);
    s.step();
    assert!(!ui(&s, 10).clicked, "letting go is not a click on it either");
    s.step();
    // the next click works as ever
    click(&mut s, 0.7, 0.15);
    assert!(ui(&s, 10).clicked, "{:?}", ui(&s, 10));
    // and with the list closed button 9 answers again
    pointer(&mut s, 0.2, 0.35, false);
    s.step();
    assert!(ui(&s, 9).hover);
}

#[test]
fn the_keyboard_opens_picks_cancels_and_leaves_the_focus_on_the_header() {
    let mut s = dropdown_scene(false);
    spawn(&mut s, 20, r#"{"UiNav":{},"UiNavAuto":{}}"#);
    spawn(&mut s, 21, r#"{"Ui":{"pos":[0.6,0.1],"size":[0.2,0.1],"button":true}}"#);
    let focus = |s: &Sim| s.world.get::<UiNav>(s.world.entity(20).unwrap()).unwrap().current.0;
    let press = |s: &mut Sim, name: &str| {
        setr(s, &format!("action.{name}.pressed"), 1.0);
        s.step();
        setr(s, &format!("action.{name}.pressed"), 0.0);
        s.step();
    };
    s.step_n(2);
    assert_eq!(focus(&s), 1, "the focus starts on the header (the options are hidden)");
    press(&mut s, "ui_accept");
    assert!(dd(&s, 1).open);
    assert_eq!(focus(&s), 4, "opened from the keyboard: the focus is on the picked option (index 1 is uid 4)");
    press(&mut s, "ui_down");
    assert_eq!(focus(&s), 5);
    press(&mut s, "ui_accept");
    assert_eq!((dd(&s, 1).selected, dd(&s, 1).open), (2, false));
    assert_eq!(focus(&s), 1, "back on the header, not lost");
    // cancel: closes without picking
    press(&mut s, "ui_accept");
    assert!(dd(&s, 1).open);
    press(&mut s, "ui_up");
    press(&mut s, "ui_cancel");
    assert!(!dd(&s, 1).open && dd(&s, 1).selected == 2);
    assert_eq!(focus(&s), 1);
    // the focus moved to something else: closed
    press(&mut s, "ui_accept");
    assert!(dd(&s, 1).open);
    press(&mut s, "ui_right");
    assert_eq!(focus(&s), 21);
    assert!(!dd(&s, 1).open, "focus went elsewhere: the list closes");
}

#[test]
fn a_stale_focus_does_not_close_a_list_the_mouse_opened() {
    let mut s = dropdown_scene(true);
    spawn(&mut s, 20, r#"{"UiNav":{"current":10}}"#);
    s.step_n(2);
    click(&mut s, 0.2, 0.14);
    s.step_n(5);
    assert!(dd(&s, 1).open, "the focus was on button 10 before and still is: that is not a move");
}

fn tabs_scene() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 30, r#"{"UiTabs":{"resource":"ui.tab"}}"#);
    for i in 0..3u64 {
        spawn(&mut s, 31 + i, &format!(r#"{{"Ui":{{"pos":[{},0.05],"size":[0.1,0.06],"button":true}},"UiTab":{{"tabs":30,"index":{i},"panel":{}}}}}"#, 0.1 + i as f32 * 0.11, 34 + i));
        spawn(&mut s, 34 + i, r#"{"Ui":{"pos":[0.1,0.15],"size":[0.4,0.4]},"UiGroup":{}}"#);
        // the same button in the same place in each panel
        spawn(&mut s, 37 + i, &format!(r#"{{"Ui":{{"pos":[0.2,0.2],"size":[0.1,0.1],"button":true}},"Parent":{{"target":{}}}}}"#, 34 + i));
    }
    s
}

#[test]
fn a_tab_shows_its_panel_and_hides_the_others_with_everything_in_them() {
    let mut s = tabs_scene();
    s.step();
    let sel = |s: &Sim| s.world.get::<UiTabs>(s.world.entity(30).unwrap()).unwrap().selected;
    assert_eq!(sel(&s), 0);
    assert!(shown(&s, 34) && !shown(&s, 35) && !shown(&s, 36), "the first panel is shown at the start");
    let (on, off) = (ui(&s, 31).color, ui(&s, 32).color);
    assert_ne!(on, off, "the selected tab has its own colour");
    assert_eq!(s.world.resource("ui.tab"), Some(0.0));
    // the button inside the shown panel is under the pointer; the same place in a hidden panel is not
    pointer(&mut s, 0.25, 0.25, false);
    s.step();
    assert!(ui(&s, 37).hover && !ui(&s, 38).hover && !ui(&s, 39).hover);
    // click the second tab (x 0.21 to 0.31)
    click(&mut s, 0.26, 0.08);
    assert_eq!(sel(&s), 1);
    assert!(s.world.get::<UiTabs>(s.world.entity(30).unwrap()).unwrap().changed, "changed for that tick");
    assert!(!shown(&s, 34) && shown(&s, 35) && !shown(&s, 36));
    assert_eq!(s.world.resource("ui.tab"), Some(1.0));
    assert_eq!((ui(&s, 32).color, ui(&s, 31).color), (on, off), "the colours moved with the selection");
    pointer(&mut s, 0.25, 0.25, false);
    s.step();
    assert!(!ui(&s, 37).hover && ui(&s, 38).hover, "now the second panel's button answers");
    assert!(!s.world.get::<UiTabs>(s.world.entity(30).unwrap()).unwrap().changed);
    // clicking the selected tab again changes nothing
    click(&mut s, 0.26, 0.08);
    assert_eq!(sel(&s), 1);
    assert!(!s.world.get::<UiTabs>(s.world.entity(30).unwrap()).unwrap().changed);
    // a graph system can switch tabs by setting `selected`
    s.world.get_mut::<UiTabs>(s.world.entity(30).unwrap()).unwrap().selected = 2;
    s.step();
    assert!(shown(&s, 36) && !shown(&s, 35));
    assert_eq!(s.world.resource("ui.tab"), Some(2.0));
}

#[test]
fn a_panel_without_a_group_is_shown_and_hidden_by_its_own_visible_flag() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"UiTabs":{}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"button":true},"UiTab":{"tabs":1,"index":0,"panel":4}}"#);
    spawn(&mut s, 3, r#"{"Ui":{"button":true},"UiTab":{"tabs":1,"index":1,"panel":5}}"#);
    spawn(&mut s, 4, r#"{"Ui":{}}"#);
    spawn(&mut s, 5, r#"{"Ui":{}}"#);
    s.step();
    assert!(ui(&s, 4).visible && !ui(&s, 5).visible);
    s.world.get_mut::<UiTabs>(s.world.entity(1).unwrap()).unwrap().selected = 1;
    s.step();
    assert!(!ui(&s, 4).visible && ui(&s, 5).visible);
}

#[test]
fn the_new_components_are_registered_with_their_documentation() {
    let s = Sim::new(1);
    for name in ["UiGroup", "UiDropdown", "UiOption", "UiTabs", "UiTab"] {
        let id = s.world.component_id(name).unwrap_or_else(|| panic!("{name} is not registered"));
        assert!(s.world.info(id).doc.len() > 100, "{name} has a description for `schema`");
    }
}
