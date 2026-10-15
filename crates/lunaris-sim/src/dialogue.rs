//! Conversations: a `Dialogue` entity runs a branching script (lines, choices, jumps, conditions and actions) and writes the current line and choices into the
//! `Text` and `Ui` entities that show them. The script is text:
//!
//! ```text
//! # a comment
//! == start
//! Guard: Halt! Who goes there?
//! Guard: State your business.
//! * [A friend] -> friend
//! * [Pay the toll] if $gold >= 10 -> paid
//! * [Nothing] -> end
//!
//! == friend
//! res met_guard = 1
//! Guard: Then you may pass.
//! event gate_opens
//! -> end
//! ```
//!
//! `== name` starts a node; `Speaker: words` is a line (a line without a speaker is narration); `* [words] -> node` is a choice (`if CONDITION` before the arrow hides it
//! unless the condition holds); `-> node` jumps (`-> end` ends the talk) and `if CONDITION -> node` jumps only when the condition holds; `set Comp.field = n`,
//! `res name = n` and `event name` are the same actions state machines use. Conditions are those of `StateMachine` (`$gold >= 10`, `Health.hp < 3 or $key`).
use crate::components::{Dialogue, Text, Ui};
use crate::expr::Cond;
use crate::gameplay::{parse_action, run_actions, Action};
use lunaris_ecs::{Entity, Sym, World};

type Events = Vec<(String, u64, Option<[f32; 3]>)>;

#[derive(Clone, Debug)]
pub enum Item {
    Say { speaker: String, text: String },
    Choice { text: String, cond: Option<Cond>, target: String },
    Jump { cond: Option<Cond>, target: String },
    Do(Action),
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub items: Vec<Item>,
}

/// Is `name` a translation key written as a speaker, `{#npc.guard}` (letters, digits, dots, underscores and hyphens between the braces)? The renderer turns it into
/// the name in the language that is on, like any `{#key}` in a text.
fn is_key_name(name: &str) -> bool {
    name.strip_prefix("{#").and_then(|k| k.strip_suffix('}')).is_some_and(|k| !k.is_empty() && k.chars().count() <= 48 && k.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-')))
}

/// Is `s` the start of a line of speech, `Name: words`? A name is up to 24 letters, digits, spaces, dots, apostrophes or underscores, or a `{#key}`.
fn speech(line: &str) -> Option<(&str, &str)> {
    let (name, words) = line.split_once(':')?;
    let plain = !name.is_empty() && name.chars().count() <= 24 && name.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '.' | '\'' | '_')) && name.trim() == name;
    let ok = (plain || is_key_name(name)) && words.starts_with(' ');
    ok.then_some((name, words.trim()))
}

fn split_arrow(s: &str, line: usize) -> Result<(&str, String), String> {
    let (head, target) = s.rsplit_once("->").ok_or_else(|| format!("line {line}: expected `-> node` (or `-> end`)"))?;
    let target = target.trim();
    if target.is_empty() || target.contains(char::is_whitespace) {
        return Err(format!("line {line}: the node after `->` must be one word, not `{target}`"));
    }
    Ok((head.trim(), target.to_string()))
}

fn condition(src: &str, line: usize) -> Result<Cond, String> {
    Cond::parse(src.trim()).map_err(|e| format!("line {line}: condition `{}`: {e}", src.trim()))
}

pub fn parse_dialogue(text: &str) -> Result<Vec<Node>, String> {
    let mut nodes: Vec<Node> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix("==") {
            let name = name.trim().trim_end_matches('=').trim();
            if name.is_empty() || name.contains(char::is_whitespace) || name == "end" {
                return Err(format!("line {n}: a node name is one word (and not `end`), as `== start`"));
            }
            if nodes.iter().any(|d| d.name == name) {
                return Err(format!("line {n}: the node `{name}` is defined twice"));
            }
            nodes.push(Node { name: name.to_string(), items: Vec::new() });
            continue;
        }
        let node = nodes.last_mut().ok_or_else(|| format!("line {n}: a line before any node: start with `== start`"))?;
        let item = if let Some(rest) = line.strip_prefix('*') {
            let rest = rest.trim();
            let (open, close) = (rest.find('['), rest.find(']'));
            let (Some(open), Some(close)) = (open, close) else { return Err(format!("line {n}: a choice is `* [words] -> node` (the words go in square brackets first)")) };
            if open != 0 || close < open {
                return Err(format!("line {n}: a choice is `* [words] -> node` (the words go in square brackets first)"));
            }
            let text = rest[1..close].trim().to_string();
            let (middle, target) = split_arrow(rest[close + 1..].trim(), n)?;
            let cond = match middle.strip_prefix("if ") {
                Some(c) => Some(condition(c, n)?),
                None if middle.is_empty() => None,
                None => return Err(format!("line {n}: after the `]` comes `if CONDITION` or `->`, not `{middle}`")),
            };
            Item::Choice { text, cond, target }
        } else if let Some(rest) = line.strip_prefix("->") {
            let target = rest.trim();
            if target.is_empty() || target.contains(char::is_whitespace) {
                return Err(format!("line {n}: `->` needs a node name (or `end`)"));
            }
            Item::Jump { cond: None, target: target.to_string() }
        } else if let Some(rest) = line.strip_prefix("if ") {
            let (cond, target) = split_arrow(rest, n)?;
            Item::Jump { cond: Some(condition(cond, n)?), target }
        } else if line.starts_with("set ") || line.starts_with("res ") || line.starts_with("event ") {
            Item::Do(parse_action(line, n)?)
        } else if let Some((speaker, words)) = speech(line) {
            Item::Say { speaker: speaker.to_string(), text: words.to_string() }
        } else {
            Item::Say { speaker: String::new(), text: line.to_string() }
        };
        node.items.push(item);
    }
    if nodes.is_empty() {
        return Err("no node: start with `== start`".to_string());
    }
    for node in &nodes {
        for item in &node.items {
            let target = match item {
                Item::Choice { target, .. } | Item::Jump { target, .. } => target,
                _ => continue,
            };
            if target != "end" && !nodes.iter().any(|d| &d.name == target) {
                return Err(format!("the node `{}` jumps to `{target}`, which does not exist", node.name));
            }
        }
    }
    Ok(nodes)
}

/// Show `text` (or hide) on the `Text` or `Ui` entity `uid`.
fn show(world: &mut World, uid: u64, text: Option<&str>) {
    if uid == 0 {
        return;
    }
    let Some(e) = world.entity(uid) else { return };
    let sym = text.map(|t| world.strings_mut().intern(t));
    if let Some(t) = world.get_mut::<Text>(e) {
        match sym {
            Some(s) => {
                t.value = Sym(s);
                t.visible = true;
            }
            None => t.visible = false,
        }
    }
    if let Some(u) = world.get_mut::<Ui>(e) {
        match sym {
            Some(s) => {
                u.text = Sym(s);
                u.visible = true;
            }
            None => u.visible = false,
        }
    }
}

fn clicked(world: &World, uid: u64) -> bool {
    uid != 0 && world.entity(uid).and_then(|e| world.get::<Ui>(e)).is_some_and(|u| u.clicked)
}

/// What the player did this tick: continue, or pick choice `k` (1 to 4).
struct Input {
    advance: bool,
    pick: Option<usize>,
}

fn read_input(world: &World, uid: u64, d: &Dialogue) -> Input {
    let pressed = |name: &str| world.resource(&format!("action.{name}.pressed")).is_some_and(|v| v > 0.5);
    let advance = d.advance != d.seen_advance || clicked(world, d.text_ui.0) || pressed("dialogue_next");
    let ids = [d.choice_1.0, d.choice_2.0, d.choice_3.0, d.choice_4.0];
    let mut pick = (1..=4usize).find(|&k| clicked(world, ids[k - 1]) || pressed(&format!("dialogue_choice_{k}")));
    if pick.is_none() && (1..=4).contains(&d.choose) {
        pick = Some(d.choose as usize);
    }
    let _ = uid;
    Input { advance, pick }
}

/// The choices on offer at item `at` of a node: the run of choice items that start there, those whose condition holds. `(index in the node, text, target)`.
fn offered(world: &World, e: Entity, node: &Node, at: usize) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    for (i, item) in node.items.iter().enumerate().skip(at) {
        match item {
            Item::Choice { text, cond, target } => {
                if cond.as_ref().is_none_or(|c| c.eval(world, e).unwrap_or(false)) {
                    out.push((i, text.clone(), target.clone()));
                }
            }
            _ => break,
        }
    }
    out
}

/// How many items from `at` are choices (shown or hidden).
fn choice_run(node: &Node, at: usize) -> usize {
    node.items.iter().skip(at).take_while(|i| matches!(i, Item::Choice { .. })).count()
}

enum Stop {
    Waiting,
    Ended,
}

/// Run from `item` of node `node`: actions and jumps happen at once; a line or a menu of choices waits for the player.
#[allow(clippy::too_many_arguments)]
fn run(world: &mut World, e: Entity, uid: u64, nodes: &[Node], node: &mut usize, item: &mut usize, events: &mut Events) -> Stop {
    for _ in 0..1000 {
        let Some(it) = nodes[*node].items.get(*item).cloned() else { return Stop::Ended };
        match it {
            Item::Do(a) => {
                run_actions(world, e, uid, std::slice::from_ref(&a), events);
                *item += 1;
            }
            Item::Jump { cond, target } => {
                if cond.as_ref().is_none_or(|c| c.eval(world, e).unwrap_or(false)) {
                    match nodes.iter().position(|n| n.name == target) {
                        Some(k) => {
                            *node = k;
                            *item = 0;
                        }
                        None => return Stop::Ended,
                    }
                } else {
                    *item += 1;
                }
            }
            Item::Say { .. } => return Stop::Waiting,
            Item::Choice { .. } => {
                if offered(world, e, &nodes[*node], *item).is_empty() {
                    // every choice is hidden: carry on after them
                    *item += choice_run(&nodes[*node], *item);
                } else {
                    return Stop::Waiting;
                }
            }
        }
    }
    Stop::Ended
}

/// What is shown for the item the talk is waiting at.
fn present(world: &mut World, e: Entity, d: &Dialogue, nodes: &[Node], node: usize, item: usize, revealed: usize) {
    let ids = [d.choice_1.0, d.choice_2.0, d.choice_3.0, d.choice_4.0];
    match &nodes[node].items[item] {
        Item::Say { speaker, text } => {
            let shown: String = text.chars().take(revealed).collect();
            show(world, d.text_ui.0, Some(&shown));
            show(world, d.speaker_ui.0, (!speaker.is_empty()).then_some(speaker.as_str()));
            let (s, t) = (world.strings_mut().intern(speaker), world.strings_mut().intern(text));
            if let Some(dd) = world.get_mut::<Dialogue>(e) {
                dd.speaker = Sym(s);
                dd.text = Sym(t);
            }
            for id in ids {
                show(world, id, None);
            }
        }
        Item::Choice { .. } => {
            let menu = offered(world, e, &nodes[node], item);
            for (k, id) in ids.iter().enumerate() {
                show(world, *id, menu.get(k).map(|m| m.1.as_str()));
            }
        }
        _ => {}
    }
}

fn clear(world: &mut World, e: Entity, d: &Dialogue) {
    for id in [d.text_ui.0, d.speaker_ui.0, d.choice_1.0, d.choice_2.0, d.choice_3.0, d.choice_4.0] {
        show(world, id, None);
    }
    if let Some(dd) = world.get_mut::<Dialogue>(e) {
        dd.node = Sym(0);
        dd.item = 0;
        dd.text = Sym(0);
        dd.speaker = Sym(0);
        dd.elapsed = 0.0;
    }
}

pub fn update(world: &mut World, dt: f32, events: &mut Events) {
    let mut list: Vec<(u64, Entity, Dialogue)> = Vec::new();
    let _ = world.for_each_uid::<(&Dialogue,), _>(|e, uid, (d,)| list.push((uid, e, *d)));
    list.sort_by_key(|l| l.0);
    for (uid, e, d) in list {
        let script = world.strings().get(d.script.0).to_string();
        let started = d.node.0 != 0;
        if !d.active {
            if started {
                // switched off in the middle of a talk
                clear(world, e, &d);
                events.push(("dialogue_end".to_string(), uid, None));
            }
            if let Some(dd) = world.get_mut::<Dialogue>(e) {
                dd.seen_advance = dd.advance;
                dd.choose = 0;
            }
            continue;
        }
        let nodes = match parse_dialogue(&script) {
            Ok(n) => n,
            Err(_) => {
                // a script that does not parse ends the talk at once (`check` says why)
                if let Some(dd) = world.get_mut::<Dialogue>(e) {
                    dd.active = false;
                }
                if started {
                    clear(world, e, &d);
                }
                continue;
            }
        };
        let (mut node, mut item);
        let mut elapsed = d.elapsed;
        let mut changed = false;
        let mut ended = false;
        if !started {
            let start = world.strings().get(d.start.0).to_string();
            node = nodes.iter().position(|n| n.name == start).unwrap_or(0);
            item = 0;
            events.push(("dialogue_start".to_string(), uid, None));
            changed = true;
        } else {
            let name = world.strings().get(d.node.0).to_string();
            let Some(k) = nodes.iter().position(|n| n.name == name) else {
                // the script changed under the talk
                clear(world, e, &d);
                if let Some(dd) = world.get_mut::<Dialogue>(e) {
                    dd.active = false;
                }
                events.push(("dialogue_end".to_string(), uid, None));
                continue;
            };
            node = k;
            item = (d.item as usize).min(nodes[node].items.len());
            let input = read_input(world, uid, &d);
            elapsed += dt;
            match nodes[node].items.get(item) {
                Some(Item::Say { text, .. }) => {
                    let total = text.chars().count();
                    let revealed = if d.speed > 0.0 { (elapsed * d.speed) as usize } else { usize::MAX };
                    if input.advance {
                        if revealed < total {
                            // the first press finishes the line, the next moves on
                            elapsed = if d.speed > 0.0 { (total as f32 + 1.0) / d.speed } else { 0.0 };
                        } else {
                            item += 1;
                            elapsed = 0.0;
                            changed = true;
                        }
                    }
                }
                Some(Item::Choice { .. }) => {
                    let menu = offered(world, e, &nodes[node], item);
                    if let Some(k) = input.pick.filter(|k| *k >= 1 && *k <= menu.len() && *k <= 4) {
                        let target = menu[k - 1].2.clone();
                        match nodes.iter().position(|n| n.name == target) {
                            Some(t) => {
                                node = t;
                                item = 0;
                            }
                            None => ended = true,
                        }
                        elapsed = 0.0;
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        if changed && !ended {
            match run(world, e, uid, &nodes, &mut node, &mut item, events) {
                Stop::Ended => ended = true,
                Stop::Waiting => {}
            }
        }
        if ended {
            clear(world, e, &d);
            if let Some(dd) = world.get_mut::<Dialogue>(e) {
                dd.active = false;
                dd.seen_advance = dd.advance;
                dd.choose = 0;
            }
            events.push(("dialogue_end".to_string(), uid, None));
            continue;
        }
        // remember where we are and show it
        let node_sym = world.strings_mut().intern(&nodes[node].name);
        let mut d2 = d;
        d2.node = Sym(node_sym);
        d2.item = item as u32;
        d2.elapsed = elapsed;
        d2.seen_advance = d.advance;
        let revealed = if d.speed > 0.0 { (elapsed * d.speed) as usize } else { usize::MAX };
        if let Some(dd) = world.get_mut::<Dialogue>(e) {
            dd.node = d2.node;
            dd.item = d2.item;
            dd.elapsed = d2.elapsed;
            dd.seen_advance = d2.seen_advance;
            dd.choose = 0;
        }
        present(world, e, &d2, &nodes, node, item, revealed);
    }
}
