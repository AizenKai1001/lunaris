//! Conversations: the script language, the choices, the conditions and the screens they write to.
use super::*;
use crate::dialogue::parse_dialogue;
use components::{Dialogue, Text, Ui};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

const SCRIPT: &str = "# the gate\n== start\nGuard: Halt! Who goes there?\nGuard: State your business.\n* [A friend] -> friend\n* [Pay the toll] if $gold >= 10 -> paid\n* [Nothing] -> end\n\n== friend\nres met_guard = 1\nGuard: Then you may pass.\nevent gate_opens\n-> end\n\n== paid\nres gold += -10\nThe guard pockets the coins.\nif $met_guard == 0 -> friend\n-> end";

fn world(speed: f32) -> Sim {
    let mut s = Sim::new(1);
    let script = Json::Str(SCRIPT.to_string()).to_string_lossy();
    spawn(&mut s, 1, &format!(r#"{{"Dialogue":{{"script":{script},"text_ui":2,"speaker_ui":3,"choice_1":4,"choice_2":5,"choice_3":6,"speed":{speed}}}}}"#));
    spawn(&mut s, 2, r#"{"Text":{"value":"","visible":false}}"#);
    spawn(&mut s, 3, r#"{"Text":{"value":"","visible":false}}"#);
    for (uid, y) in [(4u64, 0.5), (5, 0.6), (6, 0.7)] {
        spawn(&mut s, uid, &format!(r#"{{"Ui":{{"pos":[0.3,{y}],"size":[0.4,0.08],"button":true,"visible":false}}}}"#));
    }
    s
}

fn shown(s: &Sim, uid: u64) -> Option<String> {
    let e = s.world.entity(uid).unwrap();
    if let Some(t) = s.world.get::<Text>(e) {
        return t.visible.then(|| s.world.strings().get(t.value.0).to_string());
    }
    let u = s.world.get::<Ui>(e).unwrap();
    u.visible.then(|| s.world.strings().get(u.text.0).to_string())
}

fn dialogue(s: &Sim) -> Dialogue {
    *s.world.get::<Dialogue>(s.world.entity(1).unwrap()).unwrap()
}

fn set(s: &mut Sim, comp: &str, json: &str) {
    s.apply(&Command::Set { uid: 1, component: comp.into(), value: parse(json).unwrap() }).unwrap();
}

fn names(s: &Sim) -> Vec<String> {
    s.events.iter().map(|e| e.name.clone()).collect()
}

#[test]
fn a_conversation_shows_lines_offers_the_choices_whose_condition_holds_and_follows_them() {
    let mut s = world(0.0);
    s.step_n(3);
    assert_eq!((shown(&s, 2), shown(&s, 3), shown(&s, 4)), (None, None, None), "nothing is shown before the talk");
    set(&mut s, "Dialogue", r#"{"active":true}"#);
    s.step();
    assert_eq!((shown(&s, 2).as_deref(), shown(&s, 3).as_deref()), (Some("Halt! Who goes there?"), Some("Guard")));
    assert!(names(&s).contains(&"dialogue_start".to_string()));
    let d = dialogue(&s);
    assert_eq!((s.world.strings().get(d.speaker.0), s.world.strings().get(d.text.0), s.world.strings().get(d.node.0)), ("Guard", "Halt! Who goes there?", "start"));
    // continuing: add one to `advance`
    set(&mut s, "Dialogue", r#"{"advance":1}"#);
    s.step();
    assert_eq!(shown(&s, 2).as_deref(), Some("State your business."));
    set(&mut s, "Dialogue", r#"{"advance":2}"#);
    s.step();
    // the menu: the toll needs 10 gold, so there are two choices
    assert_eq!((shown(&s, 2).as_deref(), shown(&s, 4).as_deref(), shown(&s, 5).as_deref(), shown(&s, 6)), (Some("State your business."), Some("A friend"), Some("Nothing"), None), "the question stays up while the choices show");
    // with the gold the three are there
    s.world.set_resource("gold", Some(20.0)).unwrap();
    s.step();
    assert_eq!((shown(&s, 4).as_deref(), shown(&s, 5).as_deref(), shown(&s, 6).as_deref()), (Some("A friend"), Some("Pay the toll"), Some("Nothing")));
    // picking the second: the action takes the coins, a narration line has no speaker, the conditional jump is not taken
    set(&mut s, "Dialogue", r#"{"choose":2}"#);
    s.step();
    assert_eq!(s.world.resource("gold"), Some(10.0));
    assert_eq!((shown(&s, 2).as_deref(), shown(&s, 3)), (Some("The guard pockets the coins."), None), "narration: no speaker");
    assert_eq!(dialogue(&s).choose, 0, "the pick is used up");
    // continuing: `if $met_guard == 0 -> friend` jumps to the friend node (the guard has not met them): its action and its line
    set(&mut s, "Dialogue", r#"{"advance":3}"#);
    s.step();
    assert_eq!((s.world.resource("met_guard"), shown(&s, 2).as_deref()), (Some(1.0), Some("Then you may pass.")));
    // the last line: continuing runs `event gate_opens` and `-> end`: the talk is over, everything is hidden, and `active` is false again
    set(&mut s, "Dialogue", r#"{"advance":4}"#);
    s.step();
    let n = names(&s);
    assert!(n.contains(&"gate_opens".to_string()) && n.contains(&"dialogue_end".to_string()), "{n:?}");
    assert_eq!((shown(&s, 2), shown(&s, 3), shown(&s, 4), dialogue(&s).active), (None, None, None, false));
    // and it can be started again from another node
    set(&mut s, "Dialogue", r#"{"active":true,"start":"friend"}"#);
    s.step();
    assert_eq!(shown(&s, 2).as_deref(), Some("Then you may pass."));
}

#[test]
fn clicks_and_actions_drive_a_conversation_and_a_line_can_be_typed_out() {
    let mut s = world(0.0);
    set(&mut s, "Dialogue", r#"{"active":true}"#);
    s.step();
    let click = |s: &mut Sim, x: f64, y: f64| {
        for left in [1.0, 0.0] {
            s.world.set_resource("input.mouse_x", Some(x)).unwrap();
            s.world.set_resource("input.mouse_y", Some(y)).unwrap();
            s.world.set_resource("input.mouse_left", Some(left)).unwrap();
            s.step();
        }
    };
    // the action `dialogue_next` (as a keypress or a pad button would set it) continues too
    s.world.set_resource("action.dialogue_next.pressed", Some(1.0)).unwrap();
    s.step();
    s.world.set_resource("action.dialogue_next.pressed", Some(0.0)).unwrap();
    assert_eq!(shown(&s, 2).as_deref(), Some("State your business."));
    s.step();
    s.step();
    assert_eq!(shown(&s, 2).as_deref(), Some("State your business."), "nothing happens by itself");
    // choice buttons are clicked: the first button (at y 0.5..0.58) picks "A friend" once the menu is up
    s.world.set_resource("action.dialogue_next.pressed", Some(1.0)).unwrap();
    s.step();
    s.world.set_resource("action.dialogue_next.pressed", Some(0.0)).unwrap();
    assert_eq!(shown(&s, 4).as_deref(), Some("A friend"));
    click(&mut s, 0.5, 0.54);
    assert_eq!(shown(&s, 2).as_deref(), Some("Then you may pass."), "clicking the button picked it");
    // typing a line out: ten characters a second
    let mut t = world(10.0);
    set(&mut t, "Dialogue", r#"{"active":true}"#);
    t.step();
    let at = |t: &Sim| shown(t, 2).unwrap_or_default();
    t.step_n(30); // half a second more: five characters in all
    assert!(at(&t).len() > 3 && at(&t).len() < 12 && "Halt! Who goes there?".starts_with(&at(&t)), "{:?}", at(&t));
    // a press while it types finishes the line; the next press moves on
    set(&mut t, "Dialogue", r#"{"advance":1}"#);
    t.step();
    assert_eq!(at(&t), "Halt! Who goes there?");
    set(&mut t, "Dialogue", r#"{"advance":2}"#);
    t.step();
    assert!(at(&t).len() < 4, "the next line starts typing: {:?}", at(&t));
}

#[test]
fn the_script_language_reports_what_is_wrong() {
    assert_eq!(parse_dialogue(SCRIPT).unwrap().len(), 3);
    for (bad, word) in [
        ("Guard: hi", "before any node"),
        ("== a\n== a", "twice"),
        ("== a\n-> nowhere", "does not exist"),
        ("== a\n* Yes -> a", "square brackets"),
        ("== a\n* [Yes]", "->"),
        ("== a\n* [Yes] maybe -> a", "if CONDITION"),
        ("== a\nif -> a", "condition"),
        ("== a\nres x", "needs"),
        ("# nothing here", "no node"),
        ("== end\nhi", "not `end`"),
    ] {
        let e = parse_dialogue(bad).unwrap_err();
        assert!(e.to_lowercase().contains(&word.to_lowercase()), "{bad:?}: {e}");
    }
}

#[test]
fn a_speaker_may_be_a_translation_key_and_the_screen_gets_the_key_as_written() {
    use crate::dialogue::Item;
    let nodes = parse_dialogue("== start
{#npc.guard}: Halt!
{score}: five
Short: ok
A name that is far too long to be one: no
{#bad key}: x
{#npc.guard}:no space
-> end").unwrap();
    let says: Vec<(String, String)> = nodes[0].items.iter().filter_map(|i| if let Item::Say { speaker, text } = i { Some((speaker.clone(), text.clone())) } else { None }).collect();
    let want = [("{#npc.guard}", "Halt!"), ("", "{score}: five"), ("Short", "ok"), ("", "A name that is far too long to be one: no"), ("", "{#bad key}: x"), ("", "{#npc.guard}:no space")];
    assert_eq!(says, want.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>());
    // on the screen it is the text as written: the renderer makes it the name of the language that is on
    let mut s = Sim::new(1);
    let script = Json::Str("== start
{#npc.guard}: {#npc.hello}
-> end".to_string()).to_string_lossy();
    spawn(&mut s, 1, &format!(r#"{{"Dialogue":{{"script":{script},"text_ui":2,"speaker_ui":3}}}}"#));
    spawn(&mut s, 2, r#"{"Text":{"value":"","visible":false}}"#);
    spawn(&mut s, 3, r#"{"Text":{"value":"","visible":false}}"#);
    set(&mut s, "Dialogue", r#"{"active":true}"#);
    s.step();
    assert_eq!((shown(&s, 3).as_deref(), shown(&s, 2).as_deref()), (Some("{#npc.guard}"), Some("{#npc.hello}")));
}
