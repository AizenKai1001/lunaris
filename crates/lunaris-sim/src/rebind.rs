//! Rebinding keys while the game runs (`docs/KEYBINDS.md`): a `KeyBind` button shows the key an action of the `InputMap` has, a click on it waits for the next key or mouse button and
//! makes that the action's key, the keys that are kept change in `settings.json`, and `{key:ACTION}` in any text says the key now. The `InputMap` text is rewritten, so nothing else in
//! the game has to know: the actions follow it the very next tick.
use crate::components::{InputMap, KeyBind, Ui};
use lunaris_ecs::{Entity, World};

/// Every key and mouse button a game can bind: the names `input.NAME` has in the native player (letters, digits, `space`, the arrows, `f1` to `f12`, `mouse_left` and so on).
/// `escape` is among them (a game may have it as a key) but a rebind never takes it: Esc cancels the wait.
pub const BINDABLE: &[&str] = &[
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "space", "enter", "tab", "backspace",
    "escape", "arrowleft", "arrowright", "arrowup", "arrowdown", "shift", "control", "alt", "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f12", "minus", "equal", "bracketleft", "bracketright", "semicolon",
    "quote", "backquote", "backslash", "comma", "period", "slash", "capslock", "insert", "delete", "home", "end", "pageup", "pagedown", "numpad0", "numpad1", "numpad2", "numpad3", "numpad4", "numpad5", "numpad6",
    "numpad7", "numpad8", "numpad9", "numpadadd", "numpadsubtract", "numpadmultiply", "numpaddivide", "numpaddecimal", "numpadenter", "mouse_left", "mouse_right", "mouse_middle", "mouse_back", "mouse_forward",
];

/// Shift, Ctrl and Alt: when one of them is down together with another key, the other one is the key that is taken.
const MODIFIERS: &[&str] = &["shift", "control", "alt"];

pub fn is_bindable(name: &str) -> bool {
    BINDABLE.contains(&name)
}

/// What a key is called on a screen: `e` is E, `space` SPACE, `mouse_left` LEFT CLICK, `control` CTRL.
pub fn key_label(name: &str) -> String {
    let fixed = match name {
        "mouse_left" => "LEFT CLICK",
        "mouse_right" => "RIGHT CLICK",
        "mouse_middle" => "MIDDLE CLICK",
        "mouse_back" => "MOUSE 4",
        "mouse_forward" => "MOUSE 5",
        "control" => "CTRL",
        "escape" => "ESC",
        "backspace" => "BACKSPACE",
        "arrowleft" => "ARROW LEFT",
        "arrowright" => "ARROW RIGHT",
        "arrowup" => "ARROW UP",
        "arrowdown" => "ARROW DOWN",
        "minus" => "-",
        "equal" => "=",
        "bracketleft" => "[",
        "bracketright" => "]",
        "semicolon" => ";",
        "quote" => "'",
        "backquote" => "`",
        "backslash" => "\\",
        "comma" => ",",
        "period" => ".",
        "slash" => "/",
        "capslock" => "CAPS",
        "insert" => "INS",
        "delete" => "DEL",
        "pageup" => "PG UP",
        "pagedown" => "PG DN",
        "numpadadd" => "NUM +",
        "numpadsubtract" => "NUM -",
        "numpadmultiply" => "NUM *",
        "numpaddivide" => "NUM /",
        "numpaddecimal" => "NUM .",
        "numpadenter" => "NUM ENTER",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    match name.strip_prefix("numpad") {
        Some(n) if !n.is_empty() => format!("NUM {n}"),
        _ => name.to_uppercase(),
    }
}

/// One line of an `InputMap` text: an action and its terms (key, scale as written), or any other line (`deadzone = 0.15`), kept as it was.
enum Item {
    Action { name: String, terms: Vec<(String, Option<String>)> },
    Other(String),
}

/// The lines of an `InputMap` text (the engine's own way of reading it: lines and `;`, `#` comments). None when a line is not `name = terms`.
fn parse(text: &str) -> Option<Vec<Item>> {
    let mut out = Vec::new();
    for raw in text.split(['\n', ';']) {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (name, rest) = line.split_once('=')?;
        let name = name.trim();
        if name == "deadzone" {
            out.push(Item::Other(line.to_string()));
            continue;
        }
        let mut terms = Vec::new();
        for t in rest.split(',') {
            let t = t.trim();
            if t.is_empty() {
                continue;
            }
            match t.rsplit_once(':') {
                Some((k, s)) => terms.push((k.trim().to_string(), Some(s.trim().to_string()))),
                None => terms.push((t.to_string(), None)),
            }
        }
        out.push(Item::Action { name: name.to_string(), terms });
    }
    Some(out)
}

fn write(items: &[Item]) -> String {
    let lines: Vec<String> = items
        .iter()
        .map(|i| match i {
            Item::Other(l) => l.clone(),
            Item::Action { name, terms } => {
                let t: Vec<String> = terms.iter().map(|(k, s)| s.as_ref().map_or_else(|| k.clone(), |s| format!("{k}:{s}"))).collect();
                format!("{name} = {}", t.join(", "))
            }
        })
        .collect();
    lines.join("\n")
}

/// Where the `index`th key of an action is: (line, term). Only the terms that are keys of this module (`BINDABLE`) count: a pad term (`gamepad.a`), a resource (`$x`) and a stick axis are not keys, so they never
/// shift the numbers.
fn slot(items: &[Item], action: &str, index: usize) -> Option<(usize, usize)> {
    for (i, item) in items.iter().enumerate() {
        if let Item::Action { name, terms } = item {
            if name == action {
                return terms.iter().enumerate().filter(|(_, (k, _))| is_bindable(k)).nth(index).map(|(t, _)| (i, t));
            }
        }
    }
    None
}

/// The keys of an action in parsed lines, in the order written (pad terms and resources left out).
fn keys_in(items: &[Item], action: &str) -> Vec<String> {
    for item in items {
        if let Item::Action { name, terms } = item {
            if name == action {
                return terms.iter().filter(|(k, _)| is_bindable(k)).map(|(k, _)| k.clone()).collect();
            }
        }
    }
    Vec::new()
}

/// The keys of an action in `text`, in the order written (pad terms and resources left out): `move_y = w, s:-1, gamepad.ly` gives `w`, `s`.
pub fn keys_of(text: &str, action: &str) -> Vec<String> {
    parse(text).map(|items| keys_in(&items, action)).unwrap_or_default()
}

/// Why a key was not taken.
#[derive(Debug, PartialEq)]
pub enum Refused {
    /// The action has no such key.
    NoSuchKey,
    /// Not a key a game can bind (`BINDABLE`).
    NotBindable,
    /// Another action has it and no button of the screen rebinds that one, so the two cannot trade places.
    Taken(String),
}

/// `text` with the `index`th key of `action` replaced by `key` (its scale stays: `s:-1` becomes `x:-1`). When another key of the map already is `key`, the two trade places, if `may_trade(action, index)` says the
/// other one can be changed (a button of the screen rebinds it); otherwise the key is refused.
pub fn rebind(text: &str, action: &str, index: usize, key: &str, may_trade: &dyn Fn(&str, usize) -> bool) -> Result<String, Refused> {
    if !is_bindable(key) {
        return Err(Refused::NotBindable);
    }
    let mut items = parse(text).ok_or(Refused::NoSuchKey)?;
    let (li, ti) = slot(&items, action, index).ok_or(Refused::NoSuchKey)?;
    let old = match &items[li] {
        Item::Action { terms, .. } => terms[ti].0.clone(),
        Item::Other(_) => return Err(Refused::NoSuchKey),
    };
    if old == key {
        return Ok(write(&items));
    }
    // who else has the key (the first, in the order written)
    let mut other: Option<(usize, usize, String, usize)> = None;
    'find: for (i, item) in items.iter().enumerate() {
        if let Item::Action { name, terms } = item {
            let mut n = 0;
            for (t, (k, _)) in terms.iter().enumerate() {
                if !is_bindable(k) {
                    continue;
                }
                if k == key && !(i == li && t == ti) {
                    other = Some((i, t, name.clone(), n));
                    break 'find;
                }
                n += 1;
            }
        }
    }
    if let Some((i, t, name, n)) = other {
        if !may_trade(&name, n) {
            return Err(Refused::Taken(name));
        }
        if let Item::Action { terms, .. } = &mut items[i] {
            terms[t].0 = old;
        }
    }
    if let Item::Action { terms, .. } = &mut items[li] {
        terms[ti].0 = key.to_string();
    }
    Ok(write(&items))
}

/// `{key:ACTION}` (its first key) and `{key:ACTION:N}` (the Nth, from 0) in a text become what the keys of `input_map` (an `InputMap` text) are called now: `Press {key:use} to open` says `Press E to open`. An action
/// or a number that does not exist is left as it was written.
pub fn expand_keys(input_map: &str, template: &str) -> String {
    if !template.contains("{key:") {
        return template.to_string();
    }
    let items = parse(input_map).unwrap_or_default();
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("{key:") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 5..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let spec = &after[..close];
        let (action, n) = match spec.split_once(':') {
            Some((a, n)) => (a, n.trim().parse::<usize>().ok()),
            None => (spec, Some(0)),
        };
        match n.and_then(|n| keys_in(&items, action.trim()).into_iter().nth(n)) {
            Some(k) => out.push_str(&key_label(&k)),
            None => out.push_str(&rest[open..open + 5 + close + 1]),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// The map the engine reads (the lowest uid that has an `InputMap`).
fn map_entity(world: &mut World) -> Option<Entity> {
    let mut best: Option<(u64, Entity)> = None;
    let _ = world.for_each_uid::<(&InputMap,), _>(|e, uid, _| {
        if best.is_none_or(|b| uid < b.0) {
            best = Some((uid, e));
        }
    });
    best.map(|b| b.1)
}

fn map_text(world: &mut World) -> Option<(Entity, String)> {
    let e = map_entity(world)?;
    let sym = world.get::<InputMap>(e)?.bindings.0;
    Some((e, world.strings().get(sym).to_string()))
}

fn set_map_text(world: &mut World, e: Entity, text: &str) {
    let sym = world.strings_mut().intern(text);
    if let Some(m) = world.get_mut::<InputMap>(e) {
        m.bindings.0 = sym;
    }
}

/// A button that rebinds a key: its entity, uid, what it says and which key of which action it is for.
struct Row {
    e: Entity,
    uid: u64,
    bind: KeyBind,
    action: String,
    index: usize,
}

/// The buttons as the world has them (no order, no strings: what the idle check of `update` needs).
fn raw_rows(world: &mut World) -> Vec<(Entity, u64, KeyBind)> {
    let mut found: Vec<(Entity, u64, KeyBind)> = Vec::new();
    let _ = world.for_each_uid::<(&KeyBind,), _>(|e, uid, (k,)| found.push((e, uid, *k)));
    found
}

/// The buttons in uid order with their action names.
fn named_rows(world: &World, mut found: Vec<(Entity, u64, KeyBind)>) -> Vec<Row> {
    found.sort_by_key(|r| r.1);
    found.into_iter().map(|(e, uid, bind)| Row { e, uid, bind, action: world.strings().get(bind.action.0).to_string(), index: bind.index as usize }).collect()
}

fn rows(world: &mut World) -> Vec<Row> {
    let found = raw_rows(world);
    named_rows(world, found)
}

/// A button that has no `default` takes the key the map has for it now: the key the scene was made with. (Done before anything is restored from a file, and on the first tick.)
fn ensure_defaults(world: &mut World, items: &[Item], rows: &mut [Row]) {
    for r in rows.iter_mut() {
        if r.bind.default.0 != 0 {
            continue;
        }
        let Some(k) = keys_in(items, &r.action).into_iter().nth(r.index) else { continue };
        let sym = world.strings_mut().intern(&k);
        if let Some(b) = world.get_mut::<KeyBind>(r.e) {
            b.default.0 = sym;
        }
        r.bind.default.0 = sym;
    }
}

/// The keys now down, as names of `BINDABLE` (what `input.NAME` says).
fn keys_down(world: &World) -> Vec<&'static str> {
    BINDABLE.iter().copied().filter(|k| world.resource(&format!("input.{k}")).is_some_and(|v| v > 0.5)).collect()
}

/// Is a key being taken (a button was clicked and the key has not come yet, or has just come and is still down)? While it is, every action reads 0: the key that is meant for the button must not also shoot, jump or open the menu.
pub fn blocking(world: &World) -> bool {
    world.resource("rebind.state").is_some_and(|s| s > 0.5)
}

fn set_state(world: &mut World, s: f64) {
    let _ = world.set_resource("rebind.state", Some(s));
}

/// The keys that differ from what the scene has, as (action, number of the key, key): what `settings.json` keeps (`bind.ACTION.N`).
pub fn changed(world: &mut World) -> Vec<(String, usize, String)> {
    let Some((_, text)) = map_text(world) else { return Vec::new() };
    let Some(items) = parse(&text) else { return Vec::new() };
    let mut rs = rows(world);
    ensure_defaults(world, &items, &mut rs);
    let mut out: Vec<(String, usize, String)> = Vec::new();
    for r in &rs {
        let default = world.strings().get(r.bind.default.0).to_string();
        let Some(now) = keys_in(&items, &r.action).into_iter().nth(r.index) else { continue };
        if !default.is_empty() && now != default && !out.iter().any(|(a, i, _)| *a == r.action && *i == r.index) {
            out.push((r.action.clone(), r.index, now));
        }
    }
    out
}

/// Put a kept key back (at the start of the game: before the first tick). False when it does not fit the scene (no button for that action and key, not a key a game can bind): such a line of the file is left alone.
pub fn restore(world: &mut World, action: &str, index: usize, key: &str) -> bool {
    let Some((e, text)) = map_text(world) else { return false };
    let Some(items) = parse(&text) else { return false };
    let mut rs = rows(world);
    ensure_defaults(world, &items, &mut rs);
    let known: Vec<(String, usize)> = rs.iter().map(|r| (r.action.clone(), r.index)).collect();
    if !known.iter().any(|(a, i)| a == action && *i == index) {
        return false;
    }
    match rebind(&text, action, index, key, &|a, i| known.iter().any(|(ka, ki)| ka == a && *ki == i)) {
        Ok(new) => {
            set_map_text(world, e, &new);
            true
        }
        Err(_) => false,
    }
}

/// At the start of a tick, before the actions are read: the buttons say their keys, a click (the one the interface widgets saw in the tick before: `Ui.clicked` lasts until they run again) starts the wait for a key, the key that
/// comes is taken. It must run before the widgets, which use up `input.escape`, `input.enter` and `input.backspace` every tick. Costs next to nothing when the map and the buttons have not changed and nothing is going on,
/// and nothing at all in a scene that has no `KeyBind`.
///
/// `rebind.state` (the engine's to write; a game may read it): 0 nothing, 1 a button was clicked and the keys are still down, 2 the next key will be taken, 3 taken (or cancelled) and not let go of yet.
/// The game may set `rebind.cancel` above 0 (the screen is closing) and `rebind.reset` above 0 (every button goes back to its default key). The engine sets `settings.save` after a change, counts the changes in `rebind.count`
/// and sets `rebind.denied` to 1 when the key could not be taken.
pub fn update(world: &mut World) {
    let found = raw_rows(world);
    let state = world.resource("rebind.state").unwrap_or(0.0);
    if found.is_empty() {
        if state != 0.0 {
            set_state(world, 0.0);
        }
        return;
    }
    let Some(map) = map_entity(world) else { return };
    let sym = world.get::<InputMap>(map).map_or(0, |m| m.bindings.0);
    // nothing to do: the map and the buttons are as they were when the labels were last written, no key is being taken, nobody clicked, nobody asked
    let signature = sym as f64 * 4096.0 + found.len() as f64;
    let asked = ["rebind.reset", "rebind.cancel"].iter().any(|n| world.resource(n).is_some_and(|v| v > 0.5));
    let clicked = state == 0.0 && found.iter().any(|r| world.get::<Ui>(r.0).is_some_and(|u| u.clicked));
    if state == 0.0 && !asked && !clicked && world.resource("rebind.sync") == Some(signature) {
        return;
    }
    let mut rs = named_rows(world, found);
    let mut text = world.strings().get(sym).to_string();
    let Some(mut items) = parse(&text) else { return };
    ensure_defaults(world, &items, &mut rs);
    let known: Vec<(String, usize)> = rs.iter().map(|r| (r.action.clone(), r.index)).collect();
    let row_has = |a: &str, i: usize| known.iter().any(|(ka, ki)| ka == a && *ki == i);

    // the game asks for the defaults
    if world.resource("rebind.reset").is_some_and(|v| v > 0.5) {
        let _ = world.set_resource("rebind.reset", Some(0.0));
        for r in &rs {
            let default = world.strings().get(r.bind.default.0).to_string();
            if default.is_empty() {
                continue;
            }
            if let Some((li, ti)) = slot(&items, &r.action, r.index) {
                if let Item::Action { terms, .. } = &mut items[li] {
                    terms[ti].0 = default;
                }
            }
        }
        let new = write(&items);
        if new != text {
            text = new;
            set_map_text(world, map, &text);
            let _ = world.set_resource("settings.save", Some(1.0));
            bump(world);
        }
    }
    // the game closes the screen
    let mut state = state;
    if world.resource("rebind.cancel").is_some_and(|v| v > 0.5) {
        let _ = world.set_resource("rebind.cancel", Some(0.0));
        if state > 0.5 {
            stop_listening(world, &rs);
            state = 3.0;
            set_state(world, state);
        }
    }

    // a key that never comes up (a lost key-up event) must not keep every action at 0 for ever: waiting for the keys to be let go of gives up after five seconds (300 ticks)
    let held = world.resource("rebind.hold").unwrap_or(0.0);
    if state == 1.0 || state == 3.0 {
        if held >= 300.0 {
            stop_listening(world, &rs);
            state = 0.0;
            set_state(world, state);
            let _ = world.set_resource("rebind.hold", Some(0.0));
        } else {
            let _ = world.set_resource("rebind.hold", Some(held + 1.0));
        }
    } else if held != 0.0 {
        let _ = world.set_resource("rebind.hold", Some(0.0));
    }

    let mut now = state as u32;
    match now {
        1 => {
            if keys_down(world).is_empty() {
                set_state(world, 2.0);
            }
        }
        2 => {
            let down = keys_down(world);
            if down.contains(&"escape") || (down.contains(&"mouse_left") && pointer_on_button(world)) {
                // Esc, or a click on a button of the screen (another tab, another key, BACK): not a key, it ends the wait (a click on an empty place is the left mouse button)
                stop_listening(world, &rs);
                set_state(world, 3.0);
            } else if let Some(key) = down.iter().copied().find(|k| !MODIFIERS.contains(k)).or_else(|| down.first().copied()) {
                if let Some(r) = rs.iter().find(|r| world.get::<KeyBind>(r.e).is_some_and(|b| b.listening)) {
                    match rebind(&text, &r.action, r.index, key, &row_has) {
                        Ok(new) => {
                            if new != text {
                                set_map_text(world, map, &new);
                                text = new;
                                let _ = world.set_resource("settings.save", Some(1.0));
                                bump(world);
                            }
                        }
                        Err(_) => {
                            let _ = world.set_resource("rebind.denied", Some(1.0));
                        }
                    }
                }
                stop_listening(world, &rs);
                set_state(world, 3.0);
            }
        }
        3 => {
            if keys_down(world).is_empty() {
                set_state(world, 0.0);
                now = 0;
            }
        }
        _ => {}
    }
    // a click on a button (the interface saw it in this tick, before this runs): wait for the keys to be let go of (that click is not the key), then for the next. A click that ended the wait of another button
    // (it came down on this one, the wait ended, and it is let go of now) is a click on this one: its wait begins in the very tick the other one's is over
    if now == 0 {
        if let Some(uid) = rs.iter().find(|r| world.get::<Ui>(r.e).is_some_and(|u| u.clicked)).map(|r| r.uid) {
            for r in &rs {
                if let Some(b) = world.get_mut::<KeyBind>(r.e) {
                    b.listening = r.uid == uid;
                }
            }
            let _ = world.set_resource("rebind.denied", Some(0.0));
            // (the click's own button is up by now, so the next key can come at once)
            set_state(world, if keys_down(world).is_empty() { 2.0 } else { 1.0 });
        }
    }

    // what the buttons say
    let items = parse(&text).unwrap_or_default();
    for r in &rs {
        let listening = world.get::<KeyBind>(r.e).is_some_and(|b| b.listening);
        let want = if listening { "PRESS A KEY".to_string() } else { keys_in(&items, &r.action).into_iter().nth(r.index).map_or_else(|| "-".to_string(), |k| key_label(&k)) };
        let Some(ui) = world.get::<Ui>(r.e) else { continue };
        if world.strings().get(ui.text.0) != want {
            let sym = world.strings_mut().intern(&want);
            if let Some(ui) = world.get_mut::<Ui>(r.e) {
                ui.text.0 = sym;
            }
        }
    }
    let sym = world.get::<InputMap>(map).map_or(0, |m| m.bindings.0);
    let _ = world.set_resource("rebind.sync", Some(sym as f64 * 4096.0 + rs.len() as f64));
}

/// Is the pointer over a button of the interface (`Ui.hover`, which the widgets of the tick before worked out)?
fn pointer_on_button(world: &mut World) -> bool {
    let mut over = false;
    let _ = world.for_each::<(&Ui,), _>(|_, (u,)| over |= u.button && u.hover);
    over
}

fn stop_listening(world: &mut World, rs: &[Row]) {
    for r in rs {
        if let Some(b) = world.get_mut::<KeyBind>(r.e) {
            b.listening = false;
        }
    }
}

fn bump(world: &mut World) {
    let n = world.resource("rebind.count").unwrap_or(0.0);
    let _ = world.set_resource("rebind.count", Some(n + 1.0));
}
