//! Rebinding keys through the tool API (docs/KEYBINDS.md): `{key:ACTION}` in a text is drawn with the key the `InputMap` has now, and a key button is clicked and answered with the `input` tool the way a window's keys do it.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn draw(e: &mut crate::Engine) -> String {
    e.call("render", &j(r#"{"width":160,"height":90,"quality":"reference"}"#)).unwrap().get("hash").unwrap().as_str().unwrap().to_string()
}

fn resource(e: &mut crate::Engine, name: &str) -> f64 {
    e.call("resources", &Json::Null).unwrap().get("resources").and_then(|r| r.get(name)).and_then(|v| v.as_f64()).unwrap_or(0.0)
}

fn camera(e: &mut crate::Engine) {
    e.call("spawn", &j(r#"{"uid":1,"components":{"Camera":{},"Transform":{"pos":[0,0,3]}}}"#)).unwrap();
}

#[test]
fn a_text_that_names_a_key_is_drawn_with_the_key_the_map_has_now() {
    let mut e = engine();
    camera(&mut e);
    e.call("spawn", &j(r#"{"uid":2,"components":{"InputMap":{"bindings":"jump = space, gamepad.a\nuse = e"}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":3,"components":{"Text":{"value":"Press {key:jump} or {key:use}","pos":[0.1,0.4],"size":0.2}}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":1}"#)).unwrap();
    let before = draw(&mut e);
    // the picture of the same words written out, in a scene that has no map at all
    let mut plain = engine();
    camera(&mut plain);
    plain.call("spawn", &j(r#"{"uid":3,"components":{"Text":{"value":"Press SPACE or E","pos":[0.1,0.4],"size":0.2}}}"#)).unwrap();
    plain.call("step", &j(r#"{"ticks":1}"#)).unwrap();
    assert_eq!(before, draw(&mut plain), "the placeholders are the keys the map has: SPACE and E");
    // the player rebinds jump: the text says the new key at once
    e.call("set", &j(r#"{"uid":2,"component":"InputMap","value":{"bindings":"jump = x, gamepad.a\nuse = e"}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":1}"#)).unwrap();
    let after = draw(&mut e);
    assert_ne!(before, after);
    let mut plain = engine();
    camera(&mut plain);
    plain.call("spawn", &j(r#"{"uid":3,"components":{"Text":{"value":"Press X or E","pos":[0.1,0.4],"size":0.2}}}"#)).unwrap();
    plain.call("step", &j(r#"{"ticks":1}"#)).unwrap();
    assert_eq!(after, draw(&mut plain), "and it is drawn as the words with X");
    // a button's label is a text too, and a `{key:..}` that names nothing stays as it was written
    e.call("spawn", &j(r#"{"uid":4,"components":{"Ui":{"pos":[0.1,0.7],"size":[0.3,0.1],"text":"{key:use}"}}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":1}"#)).unwrap();
    let with_label = draw(&mut e);
    assert_ne!(with_label, after, "the label is drawn");
}

#[test]
fn a_key_button_is_clicked_and_answered_with_the_input_tool() {
    let mut e = engine();
    e.call("spawn", &j(r#"{"uid":2,"components":{"InputMap":{"bindings":"jump = space, gamepad.a\nuse = e"}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":10,"components":{"Ui":{"pos":[0.1,0.1],"size":[0.2,0.05],"button":true},"KeyBind":{"action":"jump","index":0}}}"#)).unwrap();
    e.call("spawn", &j(r#"{"uid":11,"components":{"Ui":{"pos":[0.1,0.2],"size":[0.2,0.05],"button":true},"KeyBind":{"action":"use","index":0}}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":2}"#)).unwrap();
    let text = |e: &mut crate::Engine, uid: u64| e.call("get", &j(&format!(r#"{{"uid":{uid},"component":"Ui"}}"#))).unwrap().get("components").and_then(|c| c.get("Ui")).and_then(|u| u.get("text")).and_then(|t| t.as_str()).unwrap().to_string();
    assert_eq!((text(&mut e, 10).as_str(), text(&mut e, 11).as_str()), ("SPACE", "E"), "the buttons say the keys");
    // the click, as a window makes it: the pointer over the button, the left button down, then up
    for (down, ticks) in [(true, 1), (false, 2)] {
        e.call("input", &j(&format!(r#"{{"values":{{"mouse_x":0.2,"mouse_y":0.125,"mouse_left":{down}}}}}"#))).unwrap();
        e.call("step", &j(&format!(r#"{{"ticks":{ticks}}}"#))).unwrap();
    }
    assert_eq!((text(&mut e, 10).as_str(), resource(&mut e, "rebind.state")), ("PRESS A KEY", 2.0));
    // a key, pressed and let go the way a window's key events do it
    e.call("input", &j(r#"{"values":{"x":true}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":2}"#)).unwrap();
    e.call("input", &j(r#"{"values":{"x":false}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":2}"#)).unwrap();
    assert_eq!(text(&mut e, 10), "X");
    let map = e.call("get", &j(r#"{"uid":2,"component":"InputMap"}"#)).unwrap().get("components").and_then(|c| c.get("InputMap")).and_then(|m| m.get("bindings")).and_then(|b| b.as_str()).unwrap().to_string();
    assert!(map.contains("jump = x, gamepad.a") && map.contains("use = e"), "{map}");
    assert_eq!((resource(&mut e, "rebind.state"), resource(&mut e, "settings.save")), (0.0, 1.0), "the wait is over and the player is asked to keep the key");
    // the action follows the key
    e.call("input", &j(r#"{"values":{"x":true}}"#)).unwrap();
    e.call("step", &j(r#"{"ticks":2}"#)).unwrap();
    assert_eq!(resource(&mut e, "action.jump"), 1.0);
}
