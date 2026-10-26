//! Rebinding keys: the text rewriting, the buttons that wait for a key, and what the actions do meanwhile (docs/KEYBINDS.md).
use super::*;
use crate::rebind::{changed, expand_keys, is_bindable, key_label, keys_of, rebind, restore, Refused};
use components::{InputMap, KeyBind, Ui};
use lunaris_json::{parse, Json};

const MAP: &str = "jump = space, gamepad.a\nmove_y = w, s:-1, gamepad.ly\nuse = e\nreload = r\npause = escape, gamepad.start\ndeadzone = 0.15";

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn setr(s: &mut Sim, name: &str, v: f64) {
    s.world.set_resource(name, Some(v)).unwrap();
}

fn res(s: &Sim, name: &str) -> f64 {
    s.world.resource(name).unwrap_or(0.0)
}

fn map(s: &Sim) -> String {
    let e = s.world.entity(1).unwrap();
    s.world.strings().get(s.world.get::<InputMap>(e).unwrap().bindings.0).to_string()
}

fn label(s: &Sim, uid: u64) -> String {
    let ui = s.world.get::<Ui>(s.world.entity(uid).unwrap()).unwrap();
    s.world.strings().get(ui.text.0).to_string()
}

fn listening(s: &Sim, uid: u64) -> bool {
    s.world.get::<KeyBind>(s.world.entity(uid).unwrap()).unwrap().listening
}

/// The map of a game and four buttons: the jump key (uid 10), the key of move_y that goes back (11, the second one), the use key (12), the key of move_y that goes forward (13). Nothing rebinds `reload` or `pause`.
fn scene() -> Sim {
    let mut s = Sim::new(1);
    let text = MAP.replace('\n', "\\n");
    spawn(&mut s, 1, &format!(r#"{{"InputMap":{{"bindings":"{text}"}}}}"#));
    spawn(&mut s, 10, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.2,0.05],"button":true},"KeyBind":{"action":"jump","index":0}}"#);
    spawn(&mut s, 11, r#"{"Ui":{"pos":[0.1,0.2],"size":[0.2,0.05],"button":true},"KeyBind":{"action":"move_y","index":1}}"#);
    spawn(&mut s, 12, r#"{"Ui":{"pos":[0.1,0.3],"size":[0.2,0.05],"button":true},"KeyBind":{"action":"use","index":0}}"#);
    spawn(&mut s, 13, r#"{"Ui":{"pos":[0.1,0.4],"size":[0.2,0.05],"button":true},"KeyBind":{"action":"move_y","index":0}}"#);
    s.step();
    s
}

/// A click on a button of the screen, as the player's mouse makes it: down over it, then up (the interface sees the click in that tick, the key button answers in the next).
fn click(s: &mut Sim, x: f64, y: f64) {
    setr(s, "input.mouse_x", x);
    setr(s, "input.mouse_y", y);
    setr(s, "input.mouse_left", 1.0);
    s.step();
    setr(s, "input.mouse_left", 0.0);
    s.step();
    s.step();
}

fn tap(s: &mut Sim, key: &str) {
    setr(s, &format!("input.{key}"), 1.0);
    s.step();
    setr(s, &format!("input.{key}"), 0.0);
    s.step();
}

const NONE: &dyn Fn(&str, usize) -> bool = &|_, _| false;
const ANY: &dyn Fn(&str, usize) -> bool = &|_, _| true;

#[test]
fn the_keys_of_an_action_are_the_terms_that_are_keys_and_a_rebind_keeps_the_rest_as_it_was() {
    assert_eq!(keys_of(MAP, "move_y"), ["w", "s"], "the pad's stick is not a key");
    assert_eq!(keys_of(MAP, "jump"), ["space"]);
    assert!(keys_of(MAP, "nothing").is_empty());
    // the second key of move_y is the one with the minus: its scale stays
    let text = rebind(MAP, "move_y", 1, "x", NONE).unwrap();
    assert_eq!(keys_of(&text, "move_y"), ["w", "x"]);
    assert!(text.contains("move_y = w, x:-1, gamepad.ly") && text.contains("jump = space, gamepad.a") && text.contains("deadzone = 0.15"), "{text}");
    // the actions still read the same way
    let (list, dead) = crate::gameplay::parse_actions(&text).unwrap();
    assert_eq!(dead, 0.15);
    assert!(list.iter().any(|(n, t)| n == "move_y" && t[1] == ("input.x".to_string(), -1.0)));
    // not a key, or a key that is not there
    assert_eq!(rebind(MAP, "jump", 0, "gamepad.a", NONE), Err(Refused::NotBindable));
    assert_eq!(rebind(MAP, "jump", 1, "x", NONE), Err(Refused::NoSuchKey));
    assert_eq!(rebind(MAP, "nothing", 0, "x", NONE), Err(Refused::NoSuchKey));
    assert_eq!(rebind("jump = ???\nthis is not a line", "jump", 0, "x", NONE), Err(Refused::NoSuchKey));
    // the same key again changes nothing
    assert_eq!(keys_of(&rebind(MAP, "jump", 0, "space", NONE).unwrap(), "jump"), ["space"]);
}

#[test]
fn a_key_somebody_else_has_trades_places_when_that_one_can_be_changed_and_is_refused_when_it_cannot() {
    // `use` is on e, `reload` on r: taking r for use gives r to use and e to reload when a button has reload
    let text = rebind(MAP, "use", 0, "r", ANY).unwrap();
    assert_eq!((keys_of(&text, "use"), keys_of(&text, "reload")), (vec!["r".to_string()], vec!["e".to_string()]));
    assert_eq!(rebind(MAP, "use", 0, "r", NONE), Err(Refused::Taken("reload".into())), "nothing rebinds reload: its key stays its own");
    // a trade with a key of the same action: forward and back change places
    let text = rebind(MAP, "move_y", 0, "s", ANY).unwrap();
    assert_eq!(keys_of(&text, "move_y"), ["s", "w"]);
    assert!(text.contains("move_y = s, w:-1"), "{text}");
    // the question the engine asks is which action and which number of its keys
    let asked = std::cell::RefCell::new(Vec::new());
    let _ = rebind(MAP, "jump", 0, "s", &|a, i| {
        asked.borrow_mut().push((a.to_string(), i));
        true
    });
    assert_eq!(asked.into_inner(), [("move_y".to_string(), 1)]);
}

#[test]
fn keys_are_called_what_a_player_expects_and_texts_say_the_key_the_map_has_now() {
    for (name, label) in [("e", "E"), ("space", "SPACE"), ("mouse_left", "LEFT CLICK"), ("mouse_right", "RIGHT CLICK"), ("control", "CTRL"), ("f1", "F1"), ("numpad3", "NUM 3"), ("comma", ","), ("arrowleft", "ARROW LEFT"), ("9", "9")] {
        assert_eq!(key_label(name), label, "{name}");
        assert!(is_bindable(name), "{name}");
    }
    assert!(!is_bindable("gamepad.a") && !is_bindable("mouse_x") && !is_bindable("") && !is_bindable("f11"));
    assert_eq!(expand_keys(MAP, "Press {key:use} then {key:move_y:1} or {key:move_y}"), "Press E then S or W");
    assert_eq!(expand_keys(MAP, "{key:nothing} {key:use:3} {key:use"), "{key:nothing} {key:use:3} {key:use", "what is not there is left as it was written");
    assert_eq!(expand_keys(MAP, "no keys here {score}"), "no keys here {score}");
    let text = rebind(MAP, "use", 0, "mouse_left", NONE).unwrap();
    assert_eq!(expand_keys(&text, "{key:use} opens"), "LEFT CLICK opens");
}

#[test]
fn a_click_waits_for_the_next_key_and_the_action_follows_it() {
    let mut s = scene();
    assert_eq!((label(&s, 10).as_str(), label(&s, 11).as_str(), label(&s, 12).as_str()), ("SPACE", "S", "E"), "the buttons say their keys");
    // click on the jump button: it waits, and says so
    click(&mut s, 0.15, 0.125);
    assert!(listening(&s, 10) && !listening(&s, 11), "only the clicked one waits");
    assert_eq!((label(&s, 10).as_str(), res(&s, "rebind.state")), ("PRESS A KEY", 2.0));
    // the key that comes is taken: x, and the map is rewritten
    setr(&mut s, "input.x", 1.0);
    s.step();
    assert_eq!(keys_of(&map(&s), "jump"), ["x"]);
    assert_eq!((label(&s, 10).as_str(), listening(&s, 10), res(&s, "rebind.state"), res(&s, "settings.save"), res(&s, "rebind.count")), ("X", false, 3.0, 1.0, 1.0));
    // x is held and is jump now: it does nothing until it is let go of
    s.step();
    assert_eq!(res(&s, "action.jump"), 0.0, "the key that was taken does not also jump");
    setr(&mut s, "input.x", 0.0);
    s.step();
    assert_eq!(res(&s, "rebind.state"), 0.0);
    setr(&mut s, "input.x", 1.0);
    s.step();
    assert_eq!(res(&s, "action.jump"), 1.0, "the new key jumps");
    setr(&mut s, "input.x", 0.0);
    setr(&mut s, "input.space", 1.0);
    s.step();
    assert_eq!(res(&s, "action.jump"), 0.0, "and the old one does not");
}

#[test]
fn every_action_reads_zero_while_a_key_is_waited_for() {
    let mut s = scene();
    setr(&mut s, "input.e", 1.0);
    s.step();
    assert_eq!(res(&s, "action.use"), 1.0, "before the click E is the use key");
    setr(&mut s, "input.e", 0.0);
    s.step();
    click(&mut s, 0.15, 0.125);
    setr(&mut s, "input.e", 1.0);
    setr(&mut s, "input.w", 1.0);
    s.step();
    // (the key taken is w or e: not a modifier: the first of them; whichever, no action has fired for the keys of this press)
    assert_eq!((res(&s, "action.use"), res(&s, "action.move_y"), res(&s, "action.use.pressed")), (0.0, 0.0, 0.0));
    // Esc does not open the pause menu on the press that ends the wait either
    let mut s = scene();
    click(&mut s, 0.15, 0.125);
    setr(&mut s, "input.escape", 1.0);
    s.step();
    assert_eq!((res(&s, "action.pause"), res(&s, "action.pause.pressed")), (0.0, 0.0), "Esc cancelled the wait, it is not the pause key now");
    assert!(!listening(&s, 10) && keys_of(&map(&s), "jump") == ["space"], "nothing was rebound");
    s.step();
    assert_eq!(res(&s, "action.pause"), 0.0, "still held: still nothing");
    setr(&mut s, "input.escape", 0.0);
    s.step();
    setr(&mut s, "input.escape", 1.0);
    s.step();
    assert_eq!(res(&s, "action.pause.pressed"), 1.0, "after it was let go of Esc is the pause key again");
}

#[test]
fn the_click_that_starts_the_wait_is_not_the_key_and_a_key_held_then_must_be_let_go_of_first() {
    let mut s = scene();
    // W is held while the button is clicked: that W does not count, a new press of it does
    setr(&mut s, "input.w", 1.0);
    click(&mut s, 0.15, 0.125);
    assert_eq!(res(&s, "rebind.state"), 1.0, "a key is still down: not ready yet");
    s.step();
    assert_eq!((res(&s, "rebind.state"), keys_of(&map(&s), "jump")), (1.0, vec!["space".to_string()]));
    setr(&mut s, "input.w", 0.0);
    s.step();
    assert_eq!(res(&s, "rebind.state"), 2.0);
    // the click itself (the left button) did not become the key
    assert_eq!(keys_of(&map(&s), "jump"), ["space"]);
    tap(&mut s, "w");
    assert_eq!(keys_of(&map(&s), "jump"), ["w"], "the next press is taken");
    assert_eq!(keys_of(&map(&s), "move_y"), ["space", "s"], "and w and space traded places: forward is the key jump had");
    assert_eq!((label(&s, 10).as_str(), label(&s, 11).as_str()), ("W", "S"));
}

#[test]
fn a_click_on_a_button_ends_the_wait_and_is_a_click_on_it_and_a_click_on_an_empty_place_is_the_left_mouse_button() {
    let mut s = scene();
    click(&mut s, 0.15, 0.125);
    assert!(listening(&s, 10));
    // the pointer goes over another key button and the left button comes down: that is a click on a button, not a key
    setr(&mut s, "input.mouse_x", 0.15);
    setr(&mut s, "input.mouse_y", 0.225);
    s.step();
    setr(&mut s, "input.mouse_left", 1.0);
    s.step();
    assert!(!listening(&s, 10), "the wait is over");
    assert_eq!(keys_of(&map(&s), "jump"), ["space"], "the left mouse button was not taken");
    setr(&mut s, "input.mouse_left", 0.0);
    s.step();
    s.step();
    assert!(listening(&s, 11) && !listening(&s, 10) && res(&s, "rebind.state") == 2.0, "and the click itself went through: the button that was clicked waits now");
    // on an empty place the left mouse button is the key
    let mut s = scene();
    click(&mut s, 0.15, 0.125);
    setr(&mut s, "input.mouse_x", 0.9);
    setr(&mut s, "input.mouse_y", 0.9);
    s.step();
    setr(&mut s, "input.mouse_left", 1.0);
    s.step();
    assert_eq!(keys_of(&map(&s), "jump"), ["mouse_left"]);
    assert_eq!(label(&s, 10), "LEFT CLICK");
}

#[test]
fn a_key_that_never_comes_up_does_not_keep_the_actions_at_zero_for_ever() {
    let mut s = scene();
    // W is held and its key-up is lost: the click waits for it to be let go of
    setr(&mut s, "input.w", 1.0);
    click(&mut s, 0.15, 0.125);
    assert_eq!((res(&s, "rebind.state"), res(&s, "action.move_y")), (1.0, 0.0), "waiting, and every action reads 0");
    for _ in 0..200 {
        s.step();
    }
    assert_eq!(res(&s, "rebind.state"), 1.0, "still waiting after a few seconds");
    for _ in 0..120 {
        s.step();
    }
    assert!(res(&s, "rebind.state") == 0.0 && !listening(&s, 10), "it gives up after five seconds");
    s.step();
    assert_eq!(res(&s, "action.move_y"), 1.0, "and W is the held key it always was");
}

#[test]
fn a_modifier_is_taken_alone_and_gives_way_to_the_key_pressed_with_it() {
    let mut s = scene();
    click(&mut s, 0.15, 0.125);
    setr(&mut s, "input.shift", 1.0);
    setr(&mut s, "input.x", 1.0);
    s.step();
    assert_eq!(keys_of(&map(&s), "jump"), ["x"], "shift and x together: x");
    let mut s = scene();
    click(&mut s, 0.15, 0.125);
    tap(&mut s, "shift");
    assert_eq!(keys_of(&map(&s), "jump"), ["shift"], "a modifier alone is a key too");
    assert_eq!(label(&s, 10), "SHIFT");
}

#[test]
fn a_key_that_belongs_to_a_control_without_a_button_is_refused_and_one_with_a_button_trades() {
    let mut s = scene();
    // the use button wants r: reload has no button, so r stays reload's
    click(&mut s, 0.15, 0.325);
    tap(&mut s, "r");
    assert_eq!((keys_of(&map(&s), "use"), res(&s, "rebind.denied"), res(&s, "rebind.count")), (vec!["e".to_string()], 1.0, 0.0));
    assert_eq!(label(&s, 12), "E");
    // the use button wants space: the jump button has it, so the two trade
    click(&mut s, 0.15, 0.325);
    assert_eq!(res(&s, "rebind.denied"), 0.0, "a new try starts clean");
    tap(&mut s, "space");
    assert_eq!((keys_of(&map(&s), "use"), keys_of(&map(&s), "jump")), (vec!["space".to_string()], vec!["e".to_string()]));
    assert_eq!((label(&s, 10).as_str(), label(&s, 12).as_str()), ("E", "SPACE"), "both buttons say what they are now");
}

#[test]
fn closing_the_screen_cancels_the_wait_and_the_defaults_come_back_when_asked() {
    let mut s = scene();
    click(&mut s, 0.15, 0.125);
    assert_eq!(res(&s, "rebind.state"), 2.0);
    setr(&mut s, "rebind.cancel", 1.0);
    s.step();
    assert!(!listening(&s, 10), "the wait is over");
    assert_eq!((res(&s, "rebind.state"), res(&s, "rebind.cancel")), (0.0, 0.0), "and nothing is held: no quiet time");
    // two changes, then the defaults
    click(&mut s, 0.15, 0.125);
    tap(&mut s, "x");
    click(&mut s, 0.15, 0.225);
    tap(&mut s, "y");
    assert_eq!((keys_of(&map(&s), "jump"), keys_of(&map(&s), "move_y")), (vec!["x".to_string()], vec!["w".to_string(), "y".to_string()]));
    setr(&mut s, "settings.save", 0.0);
    setr(&mut s, "rebind.reset", 1.0);
    s.step();
    assert_eq!((keys_of(&map(&s), "jump"), keys_of(&map(&s), "move_y"), keys_of(&map(&s), "use")), (vec!["space".to_string()], vec!["w".to_string(), "s".to_string()], vec!["e".to_string()]));
    assert_eq!((label(&s, 10).as_str(), label(&s, 11).as_str(), res(&s, "rebind.reset"), res(&s, "settings.save")), ("SPACE", "S", 0.0, 1.0));
    assert!(changed(&mut s.world).is_empty(), "nothing differs from the scene any more");
}

#[test]
fn what_differs_from_the_scene_is_listed_and_goes_back_into_a_fresh_one() {
    let mut s = scene();
    assert!(changed(&mut s.world).is_empty());
    click(&mut s, 0.15, 0.125);
    tap(&mut s, "x");
    click(&mut s, 0.15, 0.325);
    tap(&mut s, "x");
    let mut kept = changed(&mut s.world);
    kept.sort();
    assert_eq!(kept, [("jump".to_string(), 0, "e".to_string()), ("use".to_string(), 0, "x".to_string())], "x went to jump, then use took it and jump got e, the key use had");
    // the next start: the scene is as it was made, the file puts the keys back (the first line trades, the second does not)
    let mut t = scene();
    for (action, index, key) in &kept {
        assert!(restore(&mut t.world, action, *index, key), "{action} {index} {key}");
    }
    assert_eq!((keys_of(&map(&t), "jump"), keys_of(&map(&t), "use")), (vec!["e".to_string()], vec!["x".to_string()]));
    t.step();
    assert_eq!((label(&t, 10).as_str(), label(&t, 12).as_str()), ("E", "X"), "the buttons say it from the first tick");
    let mut again = changed(&mut t.world);
    again.sort();
    assert_eq!(again, kept, "and what is kept is the same");
    // lines of an old file that no longer fit the scene are left alone
    assert!(!restore(&mut t.world, "nothing", 0, "x") && !restore(&mut t.world, "jump", 5, "x") && !restore(&mut t.world, "jump", 0, "gamepad.a") && !restore(&mut t.world, "reload", 0, "x"), "no button for reload: its key is not the player's to change");
    assert_eq!(keys_of(&map(&t), "reload"), ["r"]);
}

#[test]
fn a_scene_without_buttons_pays_nothing_and_a_button_idle_costs_one_look() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"InputMap":{"bindings":"jump = space"}}"#);
    for _ in 0..5 {
        s.step();
    }
    assert!(s.world.resource("rebind.state").is_none() && s.world.resource("rebind.sync").is_none(), "no resource is made for a game that has no key buttons");
    let mut s = scene();
    let before = s.world.strings().len();
    for _ in 0..30 {
        s.step();
    }
    assert_eq!(s.world.strings().len(), before, "an idle button interns nothing");
}
