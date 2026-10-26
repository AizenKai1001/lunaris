//! `ui_audit`: the listing of the interface for quality checks (docs/UI_AUDIT.md) says what the pictures show.
use crate::ui_audit::audit;
use crate::TextureCache;
use lunaris_json::{parse, Json};
use lunaris_sim::{Command, Sim};

fn spawn(sim: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    sim.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn tex() -> TextureCache {
    TextureCache::new(Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")))
}

fn base() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,3]},"Camera":{"clear":[0.1,0.1,0.2,1]}}"#);
    s
}

#[test]
fn a_label_wider_than_its_button_is_wider_in_the_listing_too() {
    let mut s = base();
    // a button 0.1 of the width wide (192 px at 1920) with 24 letters of the built-in font at 0.04 of the height (5 px per font pixel: 30 px a letter)
    spawn(&mut s, 10, r#"{"Name":{"value":"Long"},"Ui":{"pos":[0.4,0.4],"size":[0.1,0.08],"button":true,"text":"ABCDEFGHIJKLMNOPQRSTUVWX","text_size":0.04}}"#);
    spawn(&mut s, 11, r#"{"Name":{"value":"Short"},"Ui":{"pos":[0.1,0.1],"size":[0.2,0.08],"text":"OK","text_size":0.04}}"#);
    let list = audit(&mut s.world, &mut tex(), 1920, 1080).unwrap();
    let long = list.iter().find(|e| e.name == "Long").unwrap();
    let (r, ink) = (long.rect.unwrap(), long.ink.unwrap());
    assert_eq!(r, [768.0, 432.0, 960.0, 518.4]);
    assert!(ink[2] - ink[0] > (r[2] - r[0]) * 2.0, "ink {ink:?} rect {r:?}");
    assert_eq!(long.label, "ABCDEFGHIJKLMNOPQRSTUVWX");
    assert!(long.button && long.font.is_empty());
    let short = list.iter().find(|e| e.name == "Short").unwrap();
    let (r, ink) = (short.rect.unwrap(), short.ink.unwrap());
    assert!(ink[0] >= r[0] && ink[2] <= r[2] && ink[1] >= r[1] && ink[3] <= r[3], "ink {ink:?} rect {r:?}");
}

#[test]
fn truetype_letters_are_measured_with_the_real_font_and_wrap_makes_lines() {
    let mut s = base();
    spawn(&mut s, 10, r#"{"Name":{"value":"One"},"Text":{"value":"Hello there world","pos":[0.1,0.1],"size":0.08,"shadow":false},"TextFont":{"file":"pixel.ttf"}}"#);
    spawn(&mut s, 11, r#"{"Name":{"value":"Wrapped"},"Text":{"value":"Hello there world","pos":[0.1,0.5],"size":0.08,"shadow":false},"TextFont":{"file":"pixel.ttf","wrap":0.15}}"#);
    let list = audit(&mut s.world, &mut tex(), 1280, 720).unwrap();
    let one = list.iter().find(|e| e.name == "One").unwrap();
    let wrapped = list.iter().find(|e| e.name == "Wrapped").unwrap();
    let (a, b) = (one.ink.unwrap(), wrapped.ink.unwrap());
    assert!(one.is_text && one.rect.is_none() && one.font == "pixel.ttf");
    // the wrapped one is narrower and taller than the one line
    assert!(b[2] - b[0] < a[2] - a[0], "{a:?} {b:?}");
    assert!(b[3] - b[1] > (a[3] - a[1]) * 1.5, "{a:?} {b:?}");
    assert!(a[0] >= 128.0 - 2.0, "starts at 10% of 1280: {a:?}");
}

#[test]
fn the_safe_area_and_edge_pins_move_the_boxes_and_a_hidden_group_leaves_the_list() {
    let mut s = base();
    spawn(&mut s, 2, r#"{"UiScale":{"aspect":1.7777778}}"#);
    spawn(&mut s, 10, r#"{"Name":{"value":"Middle"},"Ui":{"pos":[0.0,0.0],"size":[0.5,0.5]}}"#);
    spawn(&mut s, 11, r#"{"Name":{"value":"Pinned"},"Ui":{"pos":[0.0,0.0],"size":[0.1,0.1]},"UiEdge":{"x":0,"y":0}}"#);
    spawn(&mut s, 12, r#"{"Name":{"value":"Backdrop"},"Ui":{"pos":[0,0],"size":[1,1]},"UiEdge":{"screen":true}}"#);
    spawn(&mut s, 13, r#"{"Name":{"value":"Panel"},"UiGroup":{"shown":false}}"#);
    spawn(&mut s, 14, r#"{"Name":{"value":"Inside"},"Ui":{"pos":[0.2,0.2],"size":[0.1,0.1]},"Parent":{"target":13}}"#);
    // 3440 x 1440: the 16:9 safe area is 2560 x 1440 at x 440
    let list = audit(&mut s.world, &mut tex(), 3440, 1440).unwrap();
    let find = |n: &str| list.iter().find(|e| e.name == n).map(|e| e.rect.unwrap());
    assert_eq!(find("Middle"), Some([440.0, 0.0, 1720.0, 720.0]));
    assert_eq!(find("Pinned"), Some([0.0, 0.0, 256.0, 144.0]));
    assert_eq!(find("Backdrop"), Some([0.0, 0.0, 3440.0, 1440.0]));
    assert_eq!(find("Inside"), None, "a hidden group takes its children out");
    // shown again: it is in the list
    let e = s.world.entity(13).unwrap();
    s.world.get_mut::<lunaris_sim::components::UiGroup>(e).unwrap().shown = true;
    let list = audit(&mut s.world, &mut tex(), 3440, 1440).unwrap();
    assert!(list.iter().any(|e| e.name == "Inside"));
}

#[test]
fn placeholders_are_filled_and_an_unknown_key_action_stays_visible() {
    let mut s = base();
    s.world.set_resource("score", Some(7.0)).unwrap();
    spawn(&mut s, 10, r#"{"Name":{"value":"Score"},"Text":{"value":"Score {score} / {nothing.here}","pos":[0.1,0.1]}}"#);
    spawn(&mut s, 11, r#"{"Name":{"value":"Key"},"Text":{"value":"Press {key:NOPE}","pos":[0.1,0.3]}}"#);
    let list = audit(&mut s.world, &mut tex(), 640, 360).unwrap();
    let score = list.iter().find(|e| e.name == "Score").unwrap();
    assert_eq!(score.template, "Score {score} / {nothing.here}");
    assert_eq!(score.label, "Score 7 / 0");
    let key = list.iter().find(|e| e.name == "Key").unwrap();
    assert_eq!(key.label, "Press {key:NOPE}");
}
