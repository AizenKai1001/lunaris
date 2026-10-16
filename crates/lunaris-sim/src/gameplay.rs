//! Gameplay building blocks that are data: input actions, state machines (for animation and for AI), tweens, camera rigs and sensors.
//! Each is a component with its text or numbers on it, run in entity-uid order inside the fixed step, so they are deterministic, recorded,
//! hashed and replayed like everything else. See `docs/GAMEPLAY.md`.
use crate::components::{AnimEvents, CameraCollide, RootMotion, BehaviorTree, HitPoints, StreamZone, PathFollower, Spline, Timeline, Animator, CameraRig, Environment, InputMap, Lifetime, Light, Parent, Sensor, PrefabSpawner, StateMachine, TimeOfDay, Transform, Tween};
use crate::graph::ScriptSet;
use crate::detmath;
use crate::expr::Cond;
use crate::fieldio;
use crate::terrain::{grounds, Terrains};
use lunaris_ecs::{Entity, World};
use std::collections::BTreeMap;

pub type V3 = [f32; 3];

// ================================================================================================================ input actions

/// One term of an action: a resource to read and the amount it counts for.
struct Term {
    resource: String,
    scale: f64,
}

/// `jump = space, gamepad.a` / `move_x = d, a:-1, gamepad.lx` / `deadzone = 0.15`: the actions of an `InputMap` text.
pub fn parse_actions(text: &str) -> Result<(Vec<(String, Vec<(String, f64)>)>, f64), String> {
    let mut out: Vec<(String, Vec<(String, f64)>)> = Vec::new();
    let mut dead = 0.0;
    for (n, line) in text.split(['\n', ';']).enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (name, rest) = line.split_once('=').ok_or_else(|| format!("line {}: expected `action = key, key:scale`", n + 1))?;
        let name = name.trim();
        if name == "deadzone" {
            dead = rest.trim().parse::<f64>().ok().filter(|d| (0.0..1.0).contains(d)).ok_or_else(|| format!("line {}: deadzone must be a number from 0 to under 1", n + 1))?;
            continue;
        }
        if !lunaris_ecs::valid_resource_name(&format!("action.{name}")) || name.contains('.') {
            return Err(format!("line {}: `{name}` is not a valid action name (letters, digits and underscores)", n + 1));
        }
        let mut terms = Vec::new();
        for t in rest.split(',') {
            let t = t.trim();
            if t.is_empty() {
                continue;
            }
            let (key, scale) = match t.rsplit_once(':') {
                Some((k, s)) => (k.trim(), s.trim().parse::<f64>().ok().filter(|x| x.is_finite()).ok_or_else(|| format!("line {}: `{s}` is not a number", n + 1))?),
                None => (t, 1.0),
            };
            let resource = if let Some(r) = key.strip_prefix('$') { r.to_string() } else { format!("input.{key}") };
            if !lunaris_ecs::valid_resource_name(&resource) {
                return Err(format!("line {}: `{key}` is not a valid input name", n + 1));
            }
            terms.push((resource, scale));
        }
        if terms.is_empty() {
            return Err(format!("line {}: action `{name}` has no inputs", n + 1));
        }
        out.push((name.to_string(), terms));
    }
    Ok((out, dead))
}

/// Before the graph systems: turn the raw `input.*` resources into `action.NAME` (-1 to 1), `action.NAME.pressed` and `action.NAME.released`
/// (1 on the one tick when the action went over or under one half).
pub fn actions(world: &mut World) {
    let mut maps: Vec<(u64, InputMap)> = Vec::new();
    let _ = world.for_each_uid::<(&InputMap,), _>(|_, uid, (m,)| maps.push((uid, *m)));
    let Some((_, map)) = maps.into_iter().min_by_key(|m| m.0) else { return };
    let text = world.strings().get(map.bindings.0).to_string();
    let Ok((list, dead)) = parse_actions(&text) else { return };
    for (name, terms) in list {
        let terms: Vec<Term> = terms.into_iter().map(|(resource, scale)| Term { resource, scale }).collect();
        let mut v = 0.0;
        for t in &terms {
            let raw = world.resource(&t.resource).unwrap_or(0.0) * t.scale;
            if raw.abs() >= dead {
                v += raw;
            }
        }
        let v = v.clamp(-1.0, 1.0);
        let key = format!("action.{name}");
        let was = world.resource(&key).unwrap_or(0.0);
        let _ = world.set_resource(&key, Some(v));
        let _ = world.set_resource(&format!("{key}.pressed"), Some((v.abs() > 0.5 && was.abs() <= 0.5) as u8 as f64));
        let _ = world.set_resource(&format!("{key}.released"), Some((v.abs() <= 0.5 && was.abs() > 0.5) as u8 as f64));
    }
}

// ================================================================================================================ state machines

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// `set Component.field = n` / `+= n`
    Field { path: String, add: bool, value: f64 },
    /// `res name = n` / `+= n`
    Resource { name: String, add: bool, value: f64 },
    /// `event name`
    Event(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum When {
    Enter,
    Exit,
    Tick,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StateDef {
    pub name: String,
    pub clip: Option<String>,
    pub speed: f32,
    pub looping: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Trans {
    pub from: Option<String>,
    pub to: String,
    pub when: Option<Cond>,
    pub after: f32,
    pub fade: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hook {
    pub when: When,
    pub state: String,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    pub states: Vec<StateDef>,
    pub trans: Vec<Trans>,
    pub hooks: Vec<Hook>,
}

fn number(s: &str, line: usize, what: &str) -> Result<f64, String> {
    s.trim().parse::<f64>().ok().filter(|x| x.is_finite()).ok_or_else(|| format!("line {line}: {what} must be a number, not `{s}`"))
}

pub(crate) fn parse_action(src: &str, line: usize) -> Result<Action, String> {
    let src = src.trim();
    if let Some(name) = src.strip_prefix("event ") {
        let name = name.trim();
        return if name.is_empty() { Err(format!("line {line}: `event` needs a name")) } else { Ok(Action::Event(name.to_string())) };
    }
    let (kind, rest) = src.split_once(' ').ok_or_else(|| format!("line {line}: `{src}` is not an action (set Component.field = n, res name = n, event name)"))?;
    let (target, add, value) = if let Some((t, v)) = rest.split_once("+=") {
        (t.trim(), true, number(v, line, "the amount")?)
    } else if let Some((t, v)) = rest.split_once('=') {
        (t.trim(), false, number(v, line, "the value")?)
    } else {
        return Err(format!("line {line}: `{src}` needs `= value` or `+= value`"));
    };
    match kind {
        "set" => Ok(Action::Field { path: target.to_string(), add, value }),
        "res" => {
            if !lunaris_ecs::valid_resource_name(target) {
                return Err(format!("line {line}: `{target}` is not a valid resource name"));
            }
            Ok(Action::Resource { name: target.to_string(), add, value })
        }
        other => Err(format!("line {line}: unknown action `{other}` (set, res or event)")),
    }
}

/// Parse the text of a `StateMachine`. See `docs/GAMEPLAY.md` for the language.
pub fn parse_machine(text: &str) -> Result<Machine, String> {
    let mut m = Machine { states: Vec::new(), trans: Vec::new(), hooks: Vec::new() };
    for (n, line) in text.split(['\n', ';']).enumerate() {
        let n = n + 1;
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("state ") {
            let mut parts = rest.split_whitespace();
            let name = parts.next().ok_or_else(|| format!("line {n}: `state` needs a name"))?;
            let mut s = StateDef { name: name.to_string(), clip: None, speed: 1.0, looping: true };
            for p in parts {
                match p.split_once('=') {
                    Some(("clip", v)) => s.clip = Some(v.to_string()),
                    Some(("speed", v)) => s.speed = number(v, n, "speed")? as f32,
                    None if p == "loop" => s.looping = true,
                    None if p == "once" => s.looping = false,
                    _ => return Err(format!("line {n}: unknown state option `{p}` (clip=Name, speed=n, loop, once)")),
                }
            }
            if m.states.iter().any(|x| x.name == s.name) {
                return Err(format!("line {n}: state `{}` is declared twice", s.name));
            }
            m.states.push(s);
            continue;
        }
        let hook = ["enter ", "exit ", "tick "].iter().find_map(|k| line.strip_prefix(k).map(|r| (*k, r)));
        if let Some((kind, rest)) = hook {
            let (state, actions) = rest.split_once(':').ok_or_else(|| format!("line {n}: write `{}STATE: action, action`", kind))?;
            let when = match kind {
                "enter " => When::Enter,
                "exit " => When::Exit,
                _ => When::Tick,
            };
            let actions = actions.split(',').filter(|a| !a.trim().is_empty()).map(|a| parse_action(a, n)).collect::<Result<Vec<_>, _>>()?;
            if actions.is_empty() {
                return Err(format!("line {n}: no actions after the colon"));
            }
            m.hooks.push(Hook { when, state: state.trim().to_string(), actions });
            continue;
        }
        if let Some((from, rest)) = line.split_once("->") {
            let from = from.trim();
            let words: Vec<&str> = rest.split_whitespace().collect();
            let to = words.first().ok_or_else(|| format!("line {n}: `->` needs a target state"))?;
            let mut t = Trans { from: if from == "*" { None } else { Some(from.to_string()) }, to: to.to_string(), when: None, after: 0.0, fade: None };
            let mut i = 1;
            while i < words.len() {
                match words[i] {
                    "when" => {
                        let mut j = i + 1;
                        while j < words.len() && words[j] != "after" && words[j] != "fade" {
                            j += 1;
                        }
                        t.when = Some(Cond::parse(&words[i + 1..j].join(" ")).map_err(|e| format!("line {n}: {e}"))?);
                        i = j;
                    }
                    "after" => {
                        t.after = number(words.get(i + 1).copied().unwrap_or(""), n, "`after`")?.max(0.0) as f32;
                        i += 2;
                    }
                    "fade" => {
                        t.fade = Some(number(words.get(i + 1).copied().unwrap_or(""), n, "`fade`")?.max(0.0) as f32);
                        i += 2;
                    }
                    other => return Err(format!("line {n}: unexpected `{other}` (expected when, after or fade)")),
                }
            }
            m.trans.push(t);
            continue;
        }
        return Err(format!("line {n}: `{line}` is not a state, a transition (a -> b when ...) or an enter/exit/tick action"));
    }
    if m.states.is_empty() {
        return Err("the machine has no states (write `state idle clip=Idle`)".to_string());
    }
    let known = |s: &str| m.states.iter().any(|x| x.name == s);
    for t in &m.trans {
        for s in t.from.iter().chain(std::iter::once(&t.to)) {
            if !known(s) {
                return Err(format!("a transition mentions the state `{s}`, which is not declared"));
            }
        }
    }
    for h in &m.hooks {
        if !known(&h.state) {
            return Err(format!("an action mentions the state `{}`, which is not declared", h.state));
        }
    }
    Ok(m)
}

pub(crate) fn run_actions(world: &mut World, e: Entity, uid: u64, actions: &[Action], events: &mut Vec<(String, u64, Option<V3>)>) {
    for a in actions {
        match a {
            Action::Field { path, add, value } => {
                if let Ok(slot) = fieldio::slot(world, path) {
                    let old = fieldio::read(world, e, &slot).unwrap_or(0.0);
                    fieldio::write(world, e, &slot, if *add { old + value } else { *value });
                }
            }
            Action::Resource { name, add, value } => {
                let old = world.resource(name).unwrap_or(0.0);
                let _ = world.set_resource(name, Some(if *add { old + value } else { *value }));
            }
            Action::Event(name) => {
                let pos = world.get::<Transform>(e).map(|t| t.pos);
                events.push((name.clone(), uid, pos));
            }
        }
    }
}

fn enter_state(world: &mut World, e: Entity, uid: u64, m: &Machine, to: usize, fade: Option<f32>, events: &mut Vec<(String, u64, Option<V3>)>) {
    let st = &m.states[to];
    let clip_sym = st.clip.as_ref().map(|c| world.strings_mut().intern(c));
    let name_sym = world.strings_mut().intern(&st.name);
    if let Some(sm) = world.get_mut::<StateMachine>(e) {
        sm.state.0 = name_sym;
        sm.index = to as u32;
        sm.time = 0.0;
    }
    if let (Some(clip), Some(a)) = (clip_sym, world.get_mut::<Animator>(e)) {
        if let Some(f) = fade {
            a.fade = f;
        }
        a.speed = st.speed;
        a.looping = st.looping;
        if a.clip.0 != clip {
            a.clip.0 = clip;
        }
        a.playing = true;
    }
    for h in m.hooks.iter().filter(|h| h.when == When::Enter && h.state == st.name) {
        run_actions(world, e, uid, &h.actions, events);
    }
}

fn state_machines(world: &mut World, dt: f32, events: &mut Vec<(String, u64, Option<V3>)>) {
    let mut list: Vec<(u64, Entity, StateMachine)> = Vec::new();
    let _ = world.for_each_uid::<(&StateMachine,), _>(|e, uid, (s,)| list.push((uid, e, *s)));
    list.sort_by_key(|l| l.0);
    for (uid, e, sm) in list {
        if !sm.enabled {
            continue;
        }
        let text = world.strings().get(sm.machine.0).to_string();
        let Ok(m) = parse_machine(&text) else { continue };
        let current = world.strings().get(sm.state.0).to_string();
        if !m.states.iter().any(|s| s.name == current) {
            // the first tick (or the state was edited away): start in the first state
            enter_state(world, e, uid, &m, 0, None, events);
            continue;
        }
        if let Some(s) = world.get_mut::<StateMachine>(e) {
            s.time += dt;
        }
        let time = sm.time + dt;
        for h in m.hooks.iter().filter(|h| h.when == When::Tick && h.state == current) {
            run_actions(world, e, uid, &h.actions, events);
        }
        let mut next: Option<(usize, Option<f32>)> = None;
        for t in &m.trans {
            if t.from.as_deref().is_some_and(|f| f != current) || t.to == current || time < t.after {
                continue;
            }
            let ok = match &t.when {
                Some(c) => c.eval(world, e).unwrap_or(false),
                None => true,
            };
            if ok {
                next = m.states.iter().position(|s| s.name == t.to).map(|i| (i, t.fade));
                break;
            }
        }
        if let Some((to, fade)) = next {
            for h in m.hooks.iter().filter(|h| h.when == When::Exit && h.state == current) {
                run_actions(world, e, uid, &h.actions, events);
            }
            enter_state(world, e, uid, &m, to, fade, events);
        }
    }
}

// ================================================================================================================ behaviour trees

#[derive(Clone, Debug, PartialEq)]
pub enum BtKind {
    Selector,
    Sequence,
    Parallel,
    Cond(Cond),
    Act(Vec<Action>),
    Wait(f32),
    Repeat(u32),
    Invert,
    Succeed,
    Cooldown(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct BtNode {
    pub kind: BtKind,
    pub children: Vec<usize>,
}

/// Parse the text of a `BehaviorTree`: one node per line, children indented under their parent; node 0 is the root.
pub fn parse_tree(text: &str) -> Result<Vec<BtNode>, String> {
    let mut nodes: Vec<BtNode> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for (n, raw) in text.split('\n').enumerate() {
        let n = n + 1;
        let line = raw.split('#').next().unwrap_or("").trim_end();
        if line.trim().is_empty() {
            continue;
        }
        if line.contains('\t') {
            return Err(format!("line {n}: indent with spaces, not tabs"));
        }
        let indent = line.len() - line.trim_start().len();
        let body = line.trim_start();
        let (word, rest) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
        let rest = rest.trim();
        let kind = match word {
            "selector" => BtKind::Selector,
            "sequence" => BtKind::Sequence,
            "parallel" => BtKind::Parallel,
            "invert" => BtKind::Invert,
            "succeed" => BtKind::Succeed,
            "cond" => BtKind::Cond(Cond::parse(rest).map_err(|e| format!("line {n}: {e}"))?),
            "act" => {
                let acts = rest.split(',').filter(|a| !a.trim().is_empty()).map(|a| parse_action(a, n)).collect::<Result<Vec<_>, _>>()?;
                if acts.is_empty() {
                    return Err(format!("line {n}: `act` needs an action"));
                }
                BtKind::Act(acts)
            }
            "wait" => BtKind::Wait(number(rest, n, "the wait")?.max(0.0) as f32),
            "cooldown" => BtKind::Cooldown(number(rest, n, "the cooldown")?.max(0.0) as f32),
            "repeat" => BtKind::Repeat(number(rest, n, "the repeat count")?.max(0.0) as u32),
            other => return Err(format!("line {n}: `{other}` is not a node (selector, sequence, parallel, cond, act, wait, repeat, invert, succeed, cooldown)")),
        };
        while stack.last().is_some_and(|(ind, _)| *ind >= indent) {
            stack.pop();
        }
        let idx = nodes.len();
        match stack.last() {
            Some((_, parent)) => nodes[*parent].children.push(idx),
            None if idx > 0 => return Err(format!("line {n}: a tree has one root; indent this node under another")),
            None => {}
        }
        nodes.push(BtNode { kind, children: Vec::new() });
        stack.push((indent, idx));
    }
    if nodes.is_empty() {
        return Err("the tree is empty (start with `selector` or `sequence`)".to_string());
    }
    for (i, nd) in nodes.iter().enumerate() {
        let want = match nd.kind {
            BtKind::Selector | BtKind::Sequence | BtKind::Parallel => (1, usize::MAX),
            BtKind::Invert | BtKind::Succeed | BtKind::Repeat(_) | BtKind::Cooldown(_) => (1, 1),
            _ => (0, 0),
        };
        if nd.children.len() < want.0 || nd.children.len() > want.1 {
            return Err(format!("node {} (`{}`) has {} children, which is not allowed (composites need at least one, decorators exactly one, leaves none)", i + 1, bt_name(&nd.kind), nd.children.len()));
        }
    }
    Ok(nodes)
}

fn bt_name(k: &BtKind) -> &'static str {
    match k {
        BtKind::Selector => "selector",
        BtKind::Sequence => "sequence",
        BtKind::Parallel => "parallel",
        BtKind::Cond(_) => "cond",
        BtKind::Act(_) => "act",
        BtKind::Wait(_) => "wait",
        BtKind::Repeat(_) => "repeat",
        BtKind::Invert => "invert",
        BtKind::Succeed => "succeed",
        BtKind::Cooldown(_) => "cooldown",
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum BtStatus {
    Done,
    Failed,
    Running,
}

struct BtCx<'a> {
    world: &'a mut World,
    e: Entity,
    uid: u64,
    dt: f32,
    events: &'a mut Vec<(String, u64, Option<V3>)>,
    mem: &'a mut Vec<f32>,
    nodes: &'a [BtNode],
}

fn bt_run(cx: &mut BtCx, i: usize) -> BtStatus {
    let nodes: &[BtNode] = cx.nodes;
    let node = &nodes[i];
    match &node.kind {
        BtKind::Sequence | BtKind::Selector => {
            let seq = matches!(node.kind, BtKind::Sequence);
            let mut k = cx.mem[i] as usize;
            while k < node.children.len() {
                match bt_run(cx, node.children[k]) {
                    BtStatus::Running => {
                        cx.mem[i] = k as f32;
                        return BtStatus::Running;
                    }
                    BtStatus::Done if seq => k += 1,
                    BtStatus::Failed if !seq => k += 1,
                    BtStatus::Done => {
                        cx.mem[i] = 0.0;
                        return BtStatus::Done;
                    }
                    BtStatus::Failed => {
                        cx.mem[i] = 0.0;
                        return BtStatus::Failed;
                    }
                }
            }
            cx.mem[i] = 0.0;
            if seq { BtStatus::Done } else { BtStatus::Failed }
        }
        BtKind::Parallel => {
            let (mut failed, mut running) = (false, false);
            for &c in &node.children {
                match bt_run(cx, c) {
                    BtStatus::Failed => failed = true,
                    BtStatus::Running => running = true,
                    BtStatus::Done => {}
                }
            }
            if failed { BtStatus::Failed } else if running { BtStatus::Running } else { BtStatus::Done }
        }
        BtKind::Cond(c) => {
            if c.eval(cx.world, cx.e).unwrap_or(false) { BtStatus::Done } else { BtStatus::Failed }
        }
        BtKind::Act(a) => {
            run_actions(cx.world, cx.e, cx.uid, a, cx.events);
            BtStatus::Done
        }
        BtKind::Wait(s) => {
            cx.mem[i] += cx.dt;
            if cx.mem[i] >= *s {
                cx.mem[i] = 0.0;
                BtStatus::Done
            } else {
                BtStatus::Running
            }
        }
        BtKind::Repeat(n) => match bt_run(cx, node.children[0]) {
            BtStatus::Running => BtStatus::Running,
            BtStatus::Failed => {
                cx.mem[i] = 0.0;
                BtStatus::Failed
            }
            BtStatus::Done => {
                cx.mem[i] += 1.0;
                if *n > 0 && cx.mem[i] >= *n as f32 {
                    cx.mem[i] = 0.0;
                    BtStatus::Done
                } else {
                    BtStatus::Running
                }
            }
        },
        BtKind::Invert => match bt_run(cx, node.children[0]) {
            BtStatus::Done => BtStatus::Failed,
            BtStatus::Failed => BtStatus::Done,
            BtStatus::Running => BtStatus::Running,
        },
        BtKind::Succeed => match bt_run(cx, node.children[0]) {
            BtStatus::Running => BtStatus::Running,
            _ => BtStatus::Done,
        },
        BtKind::Cooldown(s) => {
            if cx.mem[i] > 0.0 {
                cx.mem[i] = (cx.mem[i] - cx.dt).max(0.0);
                return BtStatus::Failed;
            }
            let r = bt_run(cx, node.children[0]);
            if r == BtStatus::Done {
                cx.mem[i] = *s;
            }
            r
        }
    }
}

fn behavior_trees(world: &mut World, scripts: &mut ScriptSet, dt: f32, events: &mut Vec<(String, u64, Option<V3>)>) {
    let mut list: Vec<(u64, Entity, BehaviorTree)> = Vec::new();
    let _ = world.for_each_uid::<(&BehaviorTree,), _>(|e, uid, (t,)| list.push((uid, e, *t)));
    if list.is_empty() && scripts.bt.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.0);
    let alive: std::collections::BTreeSet<u64> = list.iter().map(|l| l.0).collect();
    scripts.bt.retain(|uid, _| alive.contains(uid));
    for (uid, e, bt) in list {
        if !bt.enabled {
            continue;
        }
        let text = world.strings().get(bt.script.0).to_string();
        let Ok(nodes) = parse_tree(&text) else { continue };
        // the tree's memory is thrown away when its text changes
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for b in text.bytes() {
            hash = (hash ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
        let entry = scripts.bt.entry(uid).or_insert_with(|| (hash, vec![0.0; nodes.len()]));
        if entry.0 != hash || entry.1.len() != nodes.len() {
            *entry = (hash, vec![0.0; nodes.len()]);
        }
        let mut mem = std::mem::take(&mut entry.1);
        let status = {
            let mut cx = BtCx { world, e, uid, dt, events, mem: &mut mem, nodes: &nodes };
            bt_run(&mut cx, 0)
        };
        if let Some(entry) = scripts.bt.get_mut(&uid) {
            entry.1 = mem;
        }
        if let Some(w) = world.get_mut::<BehaviorTree>(e) {
            w.status = match status {
                BtStatus::Running => 1,
                BtStatus::Done => 2,
                BtStatus::Failed => 3,
            };
        }
    }
}

// ================================================================================================================ splines and path followers

/// The points of a `Spline`'s text: `x y z` separated by `;` or new lines.
pub fn parse_points(text: &str) -> Result<Vec<V3>, String> {
    let mut out = Vec::new();
    for (n, part) in text.split([';', '\n']).enumerate() {
        let part = part.split('#').next().unwrap_or("").trim();
        if part.is_empty() {
            continue;
        }
        let nums: Vec<f32> = part.split_whitespace().map(|w| w.parse::<f32>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("point {}: `{w}` is not a number", n + 1))).collect::<Result<_, _>>()?;
        if nums.len() != 3 {
            return Err(format!("point {} needs three numbers (x y z), found {}", n + 1, nums.len()));
        }
        out.push([nums[0], nums[1], nums[2]]);
    }
    if out.len() < 2 {
        return Err("a path needs at least two points".to_string());
    }
    Ok(out)
}

/// A path sampled densely, with the distance along it at every sample.
pub struct PathTable {
    pub pts: Vec<V3>,
    pub cum: Vec<f32>,
}

impl PathTable {
    pub fn build(points: &[V3], smooth: bool, closed: bool) -> PathTable {
        let n = points.len();
        let segs = if closed { n } else { n - 1 };
        let at = |i: i64| -> V3 {
            if closed {
                points[i.rem_euclid(n as i64) as usize]
            } else {
                points[i.clamp(0, n as i64 - 1) as usize]
            }
        };
        let k = if smooth { 24 } else { 1 };
        let mut pts: Vec<V3> = Vec::with_capacity(segs * k + 1);
        for s in 0..segs as i64 {
            let (p0, p1, p2, p3) = (at(s - 1), at(s), at(s + 1), at(s + 2));
            for j in 0..k {
                let u = j as f32 / k as f32;
                if smooth {
                    let (u2, u3) = (u * u, u * u * u);
                    let mut p = [0.0; 3];
                    for a in 0..3 {
                        p[a] = 0.5 * (2.0 * p1[a] + (-p0[a] + p2[a]) * u + (2.0 * p0[a] - 5.0 * p1[a] + 4.0 * p2[a] - p3[a]) * u2 + (-p0[a] + 3.0 * p1[a] - 3.0 * p2[a] + p3[a]) * u3);
                    }
                    pts.push(p);
                } else {
                    pts.push(p1);
                }
            }
        }
        pts.push(at(segs as i64));
        let mut cum = vec![0.0f32];
        for w in pts.windows(2) {
            let d = sub(w[1], w[0]);
            cum.push(cum[cum.len() - 1] + (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt());
        }
        PathTable { pts, cum }
    }

    pub fn length(&self) -> f32 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    /// The point `s` metres along the path and the direction it runs there.
    pub fn at(&self, s: f32) -> (V3, V3) {
        let s = s.clamp(0.0, self.length());
        let i = self.cum.partition_point(|c| *c <= s).clamp(1, self.pts.len() - 1);
        let (a, b) = (self.pts[i - 1], self.pts[i]);
        let span = (self.cum[i] - self.cum[i - 1]).max(1.0e-9);
        let u = (s - self.cum[i - 1]) / span;
        let p = [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u, a[2] + (b[2] - a[2]) * u];
        (p, norm(sub(b, a)).unwrap_or([0.0, 0.0, -1.0]))
    }
}

fn followers(world: &mut World, dt: f32) {
    let mut list: Vec<(u64, Entity, PathFollower)> = Vec::new();
    let _ = world.for_each_uid::<(&PathFollower,), _>(|e, uid, (f,)| list.push((uid, e, *f)));
    list.sort_by_key(|l| l.0);
    for (_, e, f) in list {
        if f.done || !f.speed.is_finite() {
            continue;
        }
        let Some(pe) = world.entity(f.path.0) else { continue };
        let (Some(sp), Some(base)) = (world.get::<Spline>(pe).copied(), world.get::<Transform>(pe).map(|t| t.pos)) else { continue };
        let Ok(points) = parse_points(world.strings().get(sp.points.0)) else { continue };
        let table = PathTable::build(&points, sp.smooth, sp.closed);
        let len = table.length();
        if len <= 0.0 {
            continue;
        }
        let dir = if f.direction < 0.0 { -1.0 } else { 1.0 };
        let (mut d, mut dir, mut done) = (f.distance + dir * f.speed * dt, dir, false);
        match f.mode {
            1 => d = d.rem_euclid(len),
            2 => {
                if d > len {
                    d = 2.0 * len - d;
                    dir = -1.0;
                } else if d < 0.0 {
                    d = -d;
                    dir = 1.0;
                }
                d = d.clamp(0.0, len);
            }
            _ => {
                if d >= len {
                    d = len;
                    done = true;
                } else if d < 0.0 {
                    d = 0.0;
                    done = true;
                }
            }
        }
        let (p, tangent) = table.at(d);
        let travel = [tangent[0] * dir, tangent[1] * dir, tangent[2] * dir];
        let rot = if f.align { look_rotation(travel) } else { None };
        if let Some(t) = world.get_mut::<Transform>(e) {
            t.pos = [base[0] + p[0] + f.offset[0], base[1] + p[1] + f.offset[1], base[2] + p[2] + f.offset[2]];
            if let Some(q) = rot {
                t.rot = q;
            }
        }
        if let Some(w) = world.get_mut::<PathFollower>(e) {
            w.distance = d;
            w.direction = dir;
            w.done = done;
        }
    }
}

// ================================================================================================================ timelines

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Do(Action),
    Ramp { path: String, from: f32, to: f32, dur: f32, ease: u32 },
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub at: f32,
    pub step: Step,
}

/// Parse the text of a `Timeline`.
pub fn parse_timeline(text: &str) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    for (n, line) in text.split(['\n', ';']).enumerate() {
        let n = n + 1;
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let rest = line.strip_prefix("at ").ok_or_else(|| format!("line {n}: a timeline line starts with `at SECONDS`"))?;
        let (when, what) = rest.trim().split_once(char::is_whitespace).unwrap_or((rest.trim(), ""));
        let at = number(when, n, "the time after `at`")?;
        if at < 0.0 {
            return Err(format!("line {n}: a time cannot be negative"));
        }
        let what = what.trim();
        let step = if what == "end" {
            Step::End
        } else if let Some(r) = what.strip_prefix("ramp ") {
            let (path, tail) = r.trim().split_once(char::is_whitespace).ok_or_else(|| format!("line {n}: ramp needs `Component.field FROM -> TO over SECONDS`"))?;
            let (range, after) = tail.split_once(" over ").ok_or_else(|| format!("line {n}: ramp needs `over SECONDS`"))?;
            let (a, b) = range.split_once("->").ok_or_else(|| format!("line {n}: ramp needs `FROM -> TO`"))?;
            let mut words = after.split_whitespace();
            let dur = number(words.next().unwrap_or(""), n, "the ramp's length")?.max(0.0) as f32;
            let mut ease = 0;
            while let Some(w) = words.next() {
                match w {
                    "ease" => ease = number(words.next().unwrap_or(""), n, "the easing number")? as u32,
                    other => return Err(format!("line {n}: unexpected `{other}` after the ramp's length (expected `ease N`)")),
                }
            }
            Step::Ramp { path: path.to_string(), from: number(a, n, "the ramp's start")? as f32, to: number(b, n, "the ramp's end")? as f32, dur, ease }
        } else {
            Step::Do(parse_action(what, n)?)
        };
        out.push(Entry { at: at as f32, step });
    }
    if out.is_empty() {
        return Err("the timeline is empty (write `at 1 event boom`)".to_string());
    }
    Ok(out)
}

fn timelines(world: &mut World, dt: f32, events: &mut Vec<(String, u64, Option<V3>)>) {
    let mut list: Vec<(u64, Entity, Timeline)> = Vec::new();
    let _ = world.for_each_uid::<(&Timeline,), _>(|e, uid, (t,)| list.push((uid, e, *t)));
    list.sort_by_key(|l| l.0);
    for (uid, e, tl) in list {
        if !tl.playing || tl.done || !tl.speed.is_finite() {
            continue;
        }
        let text = world.strings().get(tl.script.0).to_string();
        let Ok(entries) = parse_timeline(&text) else { continue };
        let length = entries.iter().map(|x| match &x.step {
            Step::Ramp { dur, .. } => x.at + dur,
            _ => x.at,
        }).fold(0.0f32, f32::max);
        let prev = tl.time.max(0.0);
        let mut now = prev + dt * tl.speed.max(0.0);
        let mut done = false;
        // what happens between two times: instant steps fire once, ramps are held at their value while running and left at their end value
        let mut run = |world: &mut World, lo: f32, hi: f32, first: bool| {
            for x in &entries {
                match &x.step {
                    Step::Do(a) => {
                        if (x.at > lo || (first && x.at == lo)) && x.at <= hi {
                            run_actions(world, e, uid, std::slice::from_ref(a), events);
                        }
                    }
                    Step::Ramp { path, from, to, dur, ease: kind } => {
                        let running = hi >= x.at && (lo < x.at + dur || (first && lo == x.at));
                        if running {
                            let p = if *dur > 0.0 { ((hi - x.at) / dur).clamp(0.0, 1.0) } else { 1.0 };
                            if let Ok(slot) = fieldio::slot(world, path) {
                                fieldio::write(world, e, &slot, (from + (to - from) * ease(*kind, p)) as f64);
                            }
                        }
                    }
                    Step::End => {}
                }
            }
        };
        if now >= length {
            if tl.looping && length > 0.0 {
                run(world, prev, length, prev == 0.0);
                now = now.rem_euclid(length);
                run(world, 0.0, now, true);
            } else {
                run(world, prev, length, prev == 0.0);
                now = length;
                done = true;
            }
        } else {
            run(world, prev, now, prev == 0.0);
        }
        if let Some(w) = world.get_mut::<Timeline>(e) {
            w.time = now;
            if done {
                w.done = true;
                w.playing = false;
            }
        }
    }
}

// ================================================================================================================ tweens

/// The eased value of progress `p` (0 to 1) for each `Tween.ease` number.
pub fn ease(kind: u32, p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    let pi = std::f64::consts::PI;
    let q = p as f64;
    match kind {
        1 => p * p,
        2 => 1.0 - (1.0 - p) * (1.0 - p),
        3 => {
            if p < 0.5 {
                2.0 * p * p
            } else {
                1.0 - 2.0 * (1.0 - p) * (1.0 - p)
            }
        }
        4 => p * p * p,
        5 => 1.0 - (1.0 - p) * (1.0 - p) * (1.0 - p),
        6 => {
            if p < 0.5 {
                4.0 * p * p * p
            } else {
                1.0 - 4.0 * (1.0 - p) * (1.0 - p) * (1.0 - p)
            }
        }
        7 => (1.0 - detmath::cos(q * pi * 0.5)) as f32,
        8 => detmath::sin(q * pi * 0.5) as f32,
        9 => (-(detmath::cos(pi * q) - 1.0) * 0.5) as f32,
        10 => {
            let (c1, c3) = (1.701_58f32, 2.701_58f32);
            let u = p - 1.0;
            1.0 + c3 * u * u * u + c1 * u * u
        }
        11 => {
            let (n1, d1) = (7.5625f32, 2.75f32);
            if p < 1.0 / d1 {
                n1 * p * p
            } else if p < 2.0 / d1 {
                let u = p - 1.5 / d1;
                n1 * u * u + 0.75
            } else if p < 2.5 / d1 {
                let u = p - 2.25 / d1;
                n1 * u * u + 0.9375
            } else {
                let u = p - 2.625 / d1;
                n1 * u * u + 0.984_375
            }
        }
        12 => {
            if p <= 0.0 {
                0.0
            } else if p >= 1.0 {
                1.0
            } else {
                (detmath::exp(-10.0 * q * std::f64::consts::LN_2) * detmath::sin((q * 10.0 - 0.75) * (2.0 * pi / 3.0)) + 1.0) as f32
            }
        }
        13 => p * p * (3.0 - 2.0 * p),
        _ => p,
    }
}

fn tweens(world: &mut World, dt: f32) {
    let mut list: Vec<(u64, Entity, Tween)> = Vec::new();
    let _ = world.for_each_uid::<(&Tween,), _>(|e, uid, (t,)| list.push((uid, e, *t)));
    list.sort_by_key(|l| l.0);
    for (_, e, tw) in list {
        if !tw.active {
            continue;
        }
        let time = tw.time + dt;
        let local = time - tw.delay.max(0.0);
        let path = world.strings().get(tw.path.0).to_string();
        let target = if tw.entity.0 == 0 { Some(e) } else { world.entity(tw.entity.0) };
        let (mut done, mut value) = (false, None);
        if local >= 0.0 {
            let d = tw.duration.max(1.0e-6);
            let raw = local / d;
            let p = match tw.mode {
                1 => raw - raw.floor(),
                2 => {
                    let m = raw % 2.0;
                    if m <= 1.0 {
                        m
                    } else {
                        2.0 - m
                    }
                }
                _ => raw.min(1.0),
            };
            done = tw.mode == 0 && raw >= 1.0;
            value = Some(tw.from + (tw.to - tw.from) * ease(tw.ease, if done { 1.0 } else { p }));
        }
        if let (Some(v), Some(t)) = (value, target) {
            if let Ok(slot) = fieldio::slot(world, &path) {
                fieldio::write(world, t, &slot, v as f64);
            }
        }
        if let Some(w) = world.get_mut::<Tween>(e) {
            w.time = time;
            if done {
                w.active = false;
                w.done = true;
            }
        }
    }
}

// ================================================================================================================ camera rigs

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm(a: V3) -> Option<V3> {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    (l > 1.0e-6).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

/// The rotation (x, y, z, w) of a camera at rest that looks along `forward` (the camera looks down its own -z) with `up` above it.
pub fn look_rotation(forward: V3) -> Option<[f32; 4]> {
    let f = norm(forward)?;
    let right = norm(cross(f, [0.0, 1.0, 0.0])).unwrap_or([1.0, 0.0, 0.0]);
    let up = cross(right, f);
    // columns of the rotation matrix: right, up, -forward
    let (m00, m01, m02) = (right[0], up[0], -f[0]);
    let (m10, m11, m12) = (right[1], up[1], -f[1]);
    let (m20, m21, m22) = (right[2], up[2], -f[2]);
    let tr = m00 + m11 + m22;
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    };
    Some(q)
}

fn hash01(seed: u64, a: u64, b: u64) -> f32 {
    let mut x = seed ^ a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    x ^= x >> 33;
    (x >> 40) as f32 / 16_777_216.0
}

/// Smooth noise in -1..1 along time (25 changes a second), different for each camera and axis.
fn shake_noise(uid: u64, axis: u64, tick: u64, dt: f32) -> f32 {
    let t = tick as f32 * dt * 25.0;
    let i = t.floor();
    let f = t - i;
    let (a, b) = (hash01(uid, axis, i as u64), hash01(uid, axis, i as u64 + 1));
    let s = f * f * (3.0 - 2.0 * f);
    (a + (b - a) * s) * 2.0 - 1.0
}

fn camera_rigs(world: &mut World, tick: u64, dt: f32) {
    let mut list: Vec<(u64, Entity, CameraRig, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&CameraRig, &Transform), _>(|e, uid, (r, t)| list.push((uid, e, *r, *t)));
    list.sort_by_key(|l| l.0);
    for (uid, e, rig, t) in list {
        let Some(target) = world.entity(rig.target.0).and_then(|te| world.get::<Transform>(te)).map(|tt| tt.pos) else { continue };
        let focus = add(target, rig.look_offset);
        let desired = if rig.mode == 1 {
            let (yaw, pitch) = (rig.yaw as f64, rig.pitch as f64);
            let (cp, sp, cy, sy) = (detmath::cos(pitch) as f32, detmath::sin(pitch) as f32, detmath::cos(yaw) as f32, detmath::sin(yaw) as f32);
            add(focus, [rig.distance * cp * sy, rig.distance * sp, rig.distance * cp * cy])
        } else {
            add(target, rig.offset)
        };
        let base = if rig.follow_speed <= 0.0 || !t.pos.iter().all(|v| v.is_finite()) {
            desired
        } else {
            let k = 1.0 - detmath::exp(-(rig.follow_speed as f64) * dt as f64) as f32;
            add(t.pos, [(desired[0] - t.pos[0]) * k, (desired[1] - t.pos[1]) * k, (desired[2] - t.pos[2]) * k])
        };
        let trauma = (rig.shake - rig.shake_decay.max(0.0) * dt).max(0.0).min(1.0);
        let power = trauma * trauma * rig.shake_amount;
        let jitter = [shake_noise(uid, 0, tick, dt) * power, shake_noise(uid, 1, tick, dt) * power, shake_noise(uid, 2, tick, dt) * power];
        let rot = if rig.look_at { look_rotation(sub(focus, base)) } else { None };
        if let Some(tr) = world.get_mut::<Transform>(e) {
            tr.pos = add(base, jitter);
            if let Some(q) = rot {
                tr.rot = q;
            }
        }
        if let Some(r) = world.get_mut::<CameraRig>(e) {
            r.shake = trauma;
        }
    }
}

/// `CameraCollide`: a camera that a rig has just placed is pulled in when a wall, a tree, a rock or the ground is between it and what it looks at.
fn camera_collisions(world: &mut World, fields: &Terrains, meshes: &crate::meshcol::MeshStore) {
    let mut list: Vec<(u64, Entity, CameraCollide, CameraRig, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&CameraCollide, &CameraRig, &Transform), _>(|e, uid, (c, r, t)| list.push((uid, e, *c, *r, *t)));
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.0);
    let level = meshes.instances(world);
    let hulls = meshes.hull_instances(world);
    for (uid, e, cc, rig, t) in list {
        let Some(target) = world.entity(rig.target.0).and_then(|te| world.get::<Transform>(te)).map(|tt| tt.pos) else { continue };
        let focus = add(target, rig.look_offset);
        let d = sub(t.pos, focus);
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len < 1.0e-4 || !len.is_finite() {
            continue;
        }
        let skip = [rig.target.0, uid];
        if !blocked(world, fields, &level, &hulls, focus, t.pos, skip, None) {
            continue;
        }
        let dir = [d[0] / len, d[1] / len, d[2] / len];
        let (mut free, mut hit) = (0.0f32, len);
        for _ in 0..9 {
            let mid = 0.5 * (free + hit);
            if blocked(world, fields, &level, &hulls, focus, add(focus, [dir[0] * mid, dir[1] * mid, dir[2] * mid]), skip, None) {
                hit = mid;
            } else {
                free = mid;
            }
        }
        let keep = (free - cc.radius.max(0.0)).max(cc.min_distance.max(0.0)).min(len);
        if let Some(tr) = world.get_mut::<Transform>(e) {
            tr.pos = add(focus, [dir[0] * keep, dir[1] * keep, dir[2] * keep]);
        }
    }
}

// ================================================================================================================ sensors

/// Does anything solid stand between `a` and `b`? (spheres and boxes of colliders other than `skip`, and terrain). `eyes` is the sensor's collision layer
/// and the layers of the world: a blocker whose layer is not in the sensor's mask is looked through (`None`: nothing is).
#[allow(clippy::too_many_arguments)]
fn blocked(world: &mut World, fields: &Terrains, level: &[crate::meshcol::Instance], hulls: &[crate::meshcol::HullInstance], a: V3, b: V3, skip: [u64; 2], eyes: Option<(crate::layers::Filter, &crate::layers::Layers)>) -> bool {
    let counts = |uid: u64| eyes.is_none_or(|(w, layers)| w.sees(layers.of(uid)));
    let d = sub(b, a);
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1.0e-5 {
        return false;
    }
    let dir = [d[0] / len, d[1] / len, d[2] / len];
    let mut cols: Vec<(u64, Transform, crate::components::Collider)> = Vec::new();
    let _ = world.for_each_uid::<(&Transform, &crate::components::Collider), _>(|_, uid, (t, c)| {
        if !c.trigger && uid != skip[0] && uid != skip[1] {
            cols.push((uid, *t, *c));
        }
    });
    for (uid, t, c) in &cols {
        if !counts(*uid) {
            continue;
        }
        let s = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
        if c.shape == 0 {
            let r = 0.5 * c.size[0].abs() * s[0].max(s[1]).max(s[2]);
            let oc = sub(a, t.pos);
            let (bq, cq) = (oc[0] * dir[0] + oc[1] * dir[1] + oc[2] * dir[2], oc[0] * oc[0] + oc[1] * oc[1] + oc[2] * oc[2] - r * r);
            let disc = bq * bq - cq;
            if disc >= 0.0 {
                let th = -bq - disc.sqrt();
                if th > 0.0 && th < len {
                    return true;
                }
                if cq <= 0.0 {
                    return true; // the start is inside it
                }
            }
        } else {
            let h = [0.5 * c.size[0].abs() * s[0], 0.5 * c.size[1].abs() * s[1], 0.5 * c.size[2].abs() * s[2]];
            let (mut t0, mut t1) = (0.0f32, len);
            let mut hit = true;
            for ax in 0..3 {
                let (lo, hi) = (t.pos[ax] - h[ax], t.pos[ax] + h[ax]);
                if dir[ax].abs() < 1.0e-9 {
                    if a[ax] < lo || a[ax] > hi {
                        hit = false;
                        break;
                    }
                } else {
                    let (u0, u1) = ((lo - a[ax]) / dir[ax], (hi - a[ax]) / dir[ax]);
                    let (u0, u1) = if u0 < u1 { (u0, u1) } else { (u1, u0) };
                    t0 = t0.max(u0);
                    t1 = t1.min(u1);
                    if t0 > t1 {
                        hit = false;
                        break;
                    }
                }
            }
            if hit {
                return true;
            }
        }
    }
    for g in grounds(world, fields) {
        if counts(g.uid) && g.placed().ray(a, dir, len).is_some() {
            return true;
        }
    }
    // the triangles of mesh colliders (the same ones the model is drawn with)
    level.iter().any(|m| m.uid != skip[0] && m.uid != skip[1] && counts(m.uid) && m.placed.ray(a, dir, len).is_some())
        || hulls.iter().any(|h| !h.trigger && h.uid != skip[0] && h.uid != skip[1] && counts(h.uid) && h.ray(a, dir, len).is_some())
}

fn sensors(world: &mut World, fields: &Terrains, meshes: &crate::meshcol::MeshStore) {
    let mut list: Vec<(u64, Entity, Sensor, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&Sensor, &Transform), _>(|e, uid, (s, t)| list.push((uid, e, *s, *t)));
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.0);
    let level = meshes.instances(world); // (empty without mesh colliders)
    let hulls = meshes.hull_instances(world); // (and without convex hulls)
    let layers = crate::layers::Layers::gather(world); // (empty without any `CollisionLayer`)
    let mut cache: BTreeMap<u32, Vec<(u64, V3)>> = BTreeMap::new();
    for (uid, e, s, t) in list {
        let mut found: (f32, u64) = (f32::INFINITY, 0);
        let mut count = 0u32;
        let name = world.strings().get(s.filter.0).to_string();
        if let Some(cid) = world.component_id(&name).filter(|_| !name.is_empty()) {
            if !cache.contains_key(&cid.0) {
                let mut all: Vec<(Entity, u64, V3)> = Vec::new();
                let _ = world.for_each_uid::<(&Transform,), _>(|oe, ouid, (ot,)| all.push((oe, ouid, ot.pos)));
                let mut v: Vec<(u64, V3)> = all.into_iter().filter(|(oe, _, _)| world.has(*oe, cid)).map(|(_, u, p)| (u, p)).collect();
                v.sort_by_key(|x| x.0);
                cache.insert(cid.0, v);
            }
            let others = cache[&cid.0].clone();
            // the way the sensor looks: its local -z
            let q = t.rot;
            let fwd = {
                let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
                [-(2.0 * (x * z + w * y)), -(2.0 * (y * z - w * x)), -(1.0 - 2.0 * (x * x + y * y))]
            };
            let half_cos = if s.fov >= 360.0 { -2.0 } else { detmath::cos((s.fov.max(0.0) as f64 * 0.5).to_radians()) as f32 };
            for (ouid, opos) in others {
                if ouid == uid {
                    continue;
                }
                let d = sub(opos, t.pos);
                let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                if dist > s.radius || !dist.is_finite() {
                    continue;
                }
                if dist > 1.0e-5 && half_cos > -1.5 {
                    let dot = (d[0] * fwd[0] + d[1] * fwd[1] + d[2] * fwd[2]) / dist;
                    if dot < half_cos {
                        continue;
                    }
                }
                let eyes = (!layers.is_empty()).then(|| (layers.of(uid), &layers));
                if s.los && blocked(world, fields, &level, &hulls, add(t.pos,[0.0, s.eye, 0.0]), add(opos, [0.0, s.eye, 0.0]), [uid, ouid], eyes) {
                    continue;
                }
                count += 1;
                if dist < found.0 {
                    found = (dist, ouid);
                }
            }
        }
        if let Some(se) = world.get_mut::<Sensor>(e) {
            se.seen.0 = found.1;
            se.distance = if found.1 == 0 { 0.0 } else { found.0 };
            se.count = count;
        }
    }
}

// ================================================================================================================ lifetimes, spawners

/// Remove these entities and everything attached to them by `Parent`, children before parents, in uid order.
fn despawn_trees(world: &mut World, roots: Vec<u64>) {
    if roots.is_empty() {
        return;
    }
    let mut children: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let _ = world.for_each_uid::<(&Parent,), _>(|_, uid, (p,)| children.entry(p.target.0).or_default().push(uid));
    let mut doomed: Vec<u64> = Vec::new();
    let mut stack = roots;
    while let Some(u) = stack.pop() {
        if doomed.contains(&u) {
            continue;
        }
        doomed.push(u);
        if let Some(kids) = children.get(&u) {
            stack.extend(kids.iter().copied());
        }
    }
    doomed.sort_unstable();
    for u in doomed {
        if let Some(e) = world.entity(u) {
            world.despawn(e);
        }
    }
}

fn health(world: &mut World, dt: f32, events: &mut Vec<(String, u64, Option<V3>)>) {
    let mut list: Vec<(u64, Entity, HitPoints)> = Vec::new();
    let _ = world.for_each_uid::<(&HitPoints,), _>(|e, uid, (h,)| list.push((uid, e, *h)));
    list.sort_by_key(|l| l.0);
    for (uid, e, mut h) in list {
        let max = h.max.max(0.0);
        if !h.value.is_finite() {
            h.value = max;
        }
        // grace: damage while invulnerable is undone, when the grace was already running at the end of the last tick (so a hit and the grace it earns can be set together)
        if h.guard && h.value < h.last && h.last > 0.0 {
            h.value = h.last;
        }
        if h.invulnerable > 0.0 {
            h.invulnerable = (h.invulnerable - dt).max(0.0);
        }
        h.guard = h.invulnerable > 0.0;
        if h.value < h.last {
            h.hurt = 0.0;
        } else {
            h.hurt += dt;
        }
        if !h.dead && h.value > 0.0 && h.regen > 0.0 && h.hurt >= h.regen_delay {
            h.value += h.regen * dt;
        }
        h.value = h.value.clamp(0.0, max);
        let mut died = false;
        if !h.dead && h.value <= 0.0 {
            h.dead = true;
            died = true;
        }
        h.last = h.value;
        if let Some(w) = world.get_mut::<HitPoints>(e) {
            *w = h;
        }
        if died {
            let pos = world.get::<Transform>(e).map(|t| t.pos);
            events.push(("died".to_string(), uid, pos));
            if h.remove_after > 0.0 {
                let _ = world.insert(e, Lifetime { seconds: h.remove_after });
            }
        }
    }
}

/// Load and unload `StreamZone`s by the distance of their target.
fn streaming(world: &mut World, scripts: &mut ScriptSet) {
    let mut zones: Vec<(u64, Entity, StreamZone, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&StreamZone, &Transform), _>(|e, uid, (z, t)| zones.push((uid, e, *z, *t)));
    if zones.is_empty() {
        return;
    }
    zones.sort_by_key(|z| z.0);
    // the active camera (the lowest uid among the active ones) for zones that do not name a target
    let mut cam: Option<(u64, V3)> = None;
    let _ = world.for_each_uid::<(&crate::components::Camera, &Transform), _>(|_, uid, (c, t)| {
        if c.active && cam.is_none_or(|b| uid < b.0) {
            cam = Some((uid, t.pos));
        }
    });
    for (uid, e, z, t) in zones {
        let target = if z.target.0 != 0 { world.entity(z.target.0).and_then(|te| world.get::<Transform>(te)).map(|tt| tt.pos) } else { cam.map(|c| c.1) };
        let Some(tp) = target else { continue };
        let dist = ((tp[0] - t.pos[0]).powi(2) + (tp[2] - t.pos[2]).powi(2)).sqrt();
        if !z.loaded && dist <= z.radius {
            let name = world.strings().get(z.prefab.0).to_string();
            let num = |v: f32| lunaris_json::Json::Float(v as f64);
            let mut ov: Vec<(String, lunaris_json::Json)> = Vec::new();
            crate::prefab::merge_component(&mut ov, "Transform", &lunaris_json::Json::obj([("pos", lunaris_json::Json::arr(t.pos.map(num))), ("rot", lunaris_json::Json::arr(t.rot.map(num)))]));
            crate::prefab::merge_component(&mut ov, "SpawnedBy", &lunaris_json::Json::obj([("spawner", lunaris_json::Json::from(uid as i64))]));
            let Ok(plan) = crate::prefab::plan(&scripts.prefabs, &name, &ov) else { continue };
            let base = world.next_uid();
            let cmds = crate::prefab::spawn_commands(&plan, base);
            if crate::command::apply_batch(world, scripts, &cmds).is_ok() {
                if let Some(w) = world.get_mut::<StreamZone>(e) {
                    w.loaded = true;
                }
            }
        } else if z.loaded && dist > z.radius + z.hysteresis.max(0.0) {
            let mut roots: Vec<u64> = Vec::new();
            let _ = world.for_each_uid::<(&crate::components::SpawnedBy,), _>(|_, ruid, (b,)| {
                if b.spawner.0 == uid {
                    roots.push(ruid);
                }
            });
            despawn_trees(world, roots);
            if let Some(w) = world.get_mut::<StreamZone>(e) {
                w.loaded = false;
            }
        }
    }
}

fn lifetimes(world: &mut World, dt: f32) {
    let mut list: Vec<(u64, Entity, f32)> = Vec::new();
    let _ = world.for_each_uid::<(&Lifetime,), _>(|e, uid, (l,)| list.push((uid, e, l.seconds)));
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.0);
    let mut expired: Vec<u64> = Vec::new();
    for (uid, e, seconds) in list {
        let left = seconds - dt;
        if left <= 0.0 {
            expired.push(uid);
        } else if let Some(l) = world.get_mut::<Lifetime>(e) {
            l.seconds = left;
        }
    }
    if expired.is_empty() {
        return;
    }
    // everything attached to an expired entity goes with it
    despawn_trees(world, expired);
}

fn spawners(world: &mut World, scripts: &mut ScriptSet, dt: f32) {
    let mut list: Vec<(u64, Entity, PrefabSpawner, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&PrefabSpawner, &Transform), _>(|e, uid, (s, t)| list.push((uid, e, *s, *t)));
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.0);
    let mut alive: BTreeMap<u64, u32> = BTreeMap::new();
    let _ = world.for_each_uid::<(&crate::components::SpawnedBy,), _>(|_, _, (b,)| *alive.entry(b.spawner.0).or_default() += 1);
    for (uid, e, sp, t) in list {
        if !sp.active {
            continue;
        }
        let mut timer = sp.timer - dt;
        let mut spawned = sp.spawned;
        if timer <= 0.0 {
            timer = sp.interval.max(1.0e-3);
            let name = world.strings().get(sp.prefab.0).to_string();
            let q = t.rot;
            let fwd = [-(2.0 * (q[0] * q[2] + q[3] * q[1])), -(2.0 * (q[1] * q[2] - q[3] * q[0])), -(1.0 - 2.0 * (q[0] * q[0] + q[1] * q[1]))];
            for _ in 0..sp.count.min(64) {
                let mine = alive.get(&uid).copied().unwrap_or(0);
                if name.is_empty() || (sp.max_alive > 0 && mine >= sp.max_alive) || (sp.limit > 0 && spawned >= sp.limit) {
                    break;
                }
                let (a, r) = (hash01(uid, spawned as u64, 1) as f64 * std::f64::consts::TAU, sp.radius.max(0.0) * hash01(uid, spawned as u64, 2).sqrt());
                let at = [t.pos[0] + sp.offset[0] + r * detmath::cos(a) as f32, t.pos[1] + sp.offset[1], t.pos[2] + sp.offset[2] + r * detmath::sin(a) as f32];
                let num = |v: f32| lunaris_json::Json::Float(v as f64);
                let mut ov: Vec<(String, lunaris_json::Json)> = Vec::new();
                crate::prefab::merge_component(&mut ov, "Transform", &lunaris_json::Json::obj([("pos", lunaris_json::Json::arr(at.map(num))), ("rot", lunaris_json::Json::arr(t.rot.map(num)))]));
                crate::prefab::merge_component(&mut ov, "SpawnedBy", &lunaris_json::Json::obj([("spawner", lunaris_json::Json::from(uid as i64))]));
                if sp.speed != 0.0 {
                    crate::prefab::merge_component(&mut ov, "Velocity", &lunaris_json::Json::obj([("lin", lunaris_json::Json::arr(fwd.map(|c| num(c * sp.speed))))]));
                }
                let Ok(plan) = crate::prefab::plan(&scripts.prefabs, &name, &ov) else { break };
                let base = world.next_uid();
                let cmds = crate::prefab::spawn_commands(&plan, base);
                if crate::command::apply_batch(world, scripts, &cmds).is_err() {
                    break;
                }
                spawned += 1;
                *alive.entry(uid).or_default() += 1;
            }
        }
        if let Some(s) = world.get_mut::<PrefabSpawner>(e) {
            s.timer = timer;
            s.spawned = spawned;
        }
    }
}

// ================================================================================================================ time of day

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn time_of_day(world: &mut World, dt: f32) {
    let mut list: Vec<(u64, Entity, TimeOfDay)> = Vec::new();
    let _ = world.for_each_uid::<(&TimeOfDay,), _>(|e, uid, (t,)| list.push((uid, e, *t)));
    let Some((_, e, tod)) = list.into_iter().min_by_key(|l| l.0) else { return };
    if !tod.enabled || !tod.time.is_finite() {
        return;
    }
    let time = if tod.day_seconds > 0.0 { (tod.time + dt * 24.0 / tod.day_seconds).rem_euclid(24.0) } else { tod.time.rem_euclid(24.0) };
    if let Some(t) = world.get_mut::<TimeOfDay>(e) {
        t.time = time;
    }
    let elev = (time as f64 - 6.0) / 12.0 * std::f64::consts::PI;
    let (s, c, az) = (detmath::sin(elev) as f32, detmath::cos(elev) as f32, tod.azimuth as f64);
    let sun_dir = [c * detmath::sin(az) as f32, s, c * detmath::cos(az) as f32];
    // the sun: the first directional light
    let mut lights: Vec<(u64, Entity)> = Vec::new();
    let _ = world.for_each_uid::<(&Light,), _>(|e, uid, (l,)| {
        if l.kind == 0 {
            lights.push((uid, e));
        }
    });
    if let Some((_, le)) = lights.into_iter().min_by_key(|l| l.0) {
        let day = smooth(-0.05, 0.25, s);
        let warm = smooth(0.0, 0.45, s);
        if let Some(l) = world.get_mut::<Light>(le) {
            l.intensity = tod.night_intensity + (tod.sun_intensity - tod.night_intensity) * day;
            l.color = mix3([1.0, 0.55, 0.3], [1.0, 0.96, 0.88], warm);
        }
        if let (Some(q), Some(t)) = (look_rotation([-sun_dir[0], -sun_dir[1], -sun_dir[2]]), world.get_mut::<Transform>(le)) {
            t.rot = q;
        }
    }
    // the sky: the first environment
    let mut envs: Vec<(u64, Entity)> = Vec::new();
    let _ = world.for_each_uid::<(&Environment,), _>(|e, uid, _| envs.push((uid, e)));
    if let Some((_, ee)) = envs.into_iter().min_by_key(|l| l.0) {
        let (night_top, night_hor) = ([0.01, 0.015, 0.05], [0.03, 0.04, 0.09]);
        let (dusk_top, dusk_hor) = ([0.18, 0.2, 0.4], [0.95, 0.5, 0.28]);
        let (day_top, day_hor) = ([0.2, 0.42, 0.85], [0.72, 0.82, 0.95]);
        let (n2d, d2day) = (smooth(-0.15, 0.0, s), smooth(0.0, 0.45, s));
        let top = mix3(mix3(night_top, dusk_top, n2d), day_top, d2day);
        let hor = mix3(mix3(night_hor, dusk_hor, n2d), day_hor, d2day);
        if let Some(env) = world.get_mut::<Environment>(ee) {
            env.sky_top = top;
            env.sky_horizon = hor;
            env.fog_color = hor;
            env.ambient = 0.15 + 0.85 * smooth(-0.1, 0.3, s);
        }
    }
}

/// The clocks of the extra clips of `AnimLayers` (the base clip's clock is the `Animator`'s).
fn anim_layers(world: &mut World, dt: f32) {
    let _ = world.for_each::<(&mut crate::components::AnimLayers,), _>(|_, (l,)| {
        if l.speed_b.is_finite() {
            l.time_b += l.speed_b * dt;
        }
        if l.speed_c.is_finite() {
            l.time_c += l.speed_c * dt;
        }
        // keep the numbers small and exact: a day of play must not lose the fraction of a second
        if l.time_b > 1.0e5 {
            l.time_b = l.time_b.rem_euclid(1.0e3);
        }
        if l.time_c > 1.0e5 {
            l.time_c = l.time_c.rem_euclid(1.0e3);
        }
    });
}

/// The clock of the leaves of every `Sway`.
fn sways(world: &mut World, dt: f32) {
    let _ = world.for_each::<(&mut crate::components::Sway,), _>(|_, (w,)| {
        if w.speed.is_finite() {
            w.phase = (w.phase + w.speed * std::f32::consts::TAU * dt).rem_euclid(6283.0);
        }
    });
}

/// The clock of the waves of every `Water`.
fn waters(world: &mut World, dt: f32) {
    let _ = world.for_each::<(&mut crate::components::Water,), _>(|_, (w,)| {
        if w.speed.is_finite() {
            w.phase = (w.phase + w.speed * dt).rem_euclid(6283.0);
        }
    });
}

/// One clip's animation events: its length and the `(time, name)` moments.
#[derive(Clone, Debug, PartialEq)]
pub struct EventClip {
    pub clip: String,
    pub length: f32,
    pub keys: Vec<(f32, String)>,
}

fn clip_head(head: &str, line: usize) -> Result<(String, f32), String> {
    let mut words = head.split_whitespace();
    let clip = words.next().ok_or_else(|| format!("line {line}: a clip name comes first, as `walk length=0.8: ...`"))?.to_string();
    let mut length = 0.0;
    for w in words {
        match w.strip_prefix("length=") {
            Some(v) => length = v.parse::<f32>().ok().filter(|l| l.is_finite() && *l > 0.0).ok_or_else(|| format!("line {line}: length must be a number of seconds above 0, not `{v}`"))?,
            None => return Err(format!("line {line}: `{w}` is not `length=SECONDS`")),
        }
    }
    if length <= 0.0 {
        return Err(format!("line {line}: say how long the clip is: `{clip} length=0.8: ...` (`model_info` lists the lengths)"));
    }
    Ok((clip, length))
}

/// Animation events as text: `walk length=0.8: 0.1 step_left, 0.5 step_right`, one clip a line.
pub fn parse_anim_events(text: &str) -> Result<Vec<EventClip>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (head, body) = line.split_once(':').ok_or_else(|| format!("line {}: expected `clip length=SECONDS: TIME name, TIME name`", n + 1))?;
        let (clip, length) = clip_head(head, n + 1)?;
        let mut keys = Vec::new();
        for key in body.split(',').map(str::trim).filter(|k| !k.is_empty()) {
            let mut w = key.split_whitespace();
            let (Some(t), Some(name), None) = (w.next(), w.next(), w.next()) else { return Err(format!("line {}: `{key}` is not `TIME name`", n + 1)) };
            let t = t.parse::<f32>().ok().filter(|t| t.is_finite() && *t >= 0.0 && *t <= length).ok_or_else(|| format!("line {}: the time `{t}` must be from 0 to the clip's length {length}", n + 1))?;
            keys.push((t, name.to_string()));
        }
        out.push(EventClip { clip, length, keys });
    }
    Ok(out)
}

/// How many times the moment `at` of a clip of `length` seconds has been passed when the animation is `t` seconds in.
fn passes(t: f32, at: f32, length: f32, looping: bool) -> u32 {
    if t < at {
        0
    } else if looping {
        (((t - at) / length).floor() as u32).saturating_add(1)
    } else {
        1
    }
}

fn anim_events(world: &mut World, events: &mut Vec<(String, u64, Option<V3>)>) {
    let mut found: Vec<(Entity, u64, u32, u32, f32, u32, f32, bool)> = Vec::new();
    let _ = world.for_each_uid::<(&AnimEvents, &Animator), _>(|e, uid, (v, a)| found.push((e, uid, v.events.0, v.prev_clip.0, v.prev_time, a.clip.0, a.time, a.looping)));
    for (e, uid, text_sym, prev_clip, prev_time, clip, time, looping) in found {
        if let Some(v) = world.get_mut::<AnimEvents>(e) {
            v.prev_clip = lunaris_ecs::Sym(clip);
            v.prev_time = time;
        }
        // the same clip, moving forward: otherwise this is a restart, a scrub or a new clip, and nothing has been passed (the very first look is "from before the start")
        let first = prev_clip == 0 && clip != 0;
        let prev_time = if first { -1.0 } else { prev_time };
        if (clip != prev_clip && !first) || !(time >= prev_time) || !time.is_finite() {
            continue;
        }
        let text = world.strings().get(text_sym).to_string();
        let clip_name = world.strings().get(clip).to_string();
        let pos = world.get::<Transform>(e).map(|t| t.pos);
        for c in parse_anim_events(&text).unwrap_or_default().iter().filter(|c| c.clip == clip_name) {
            for (at, name) in &c.keys {
                let n = passes(time, *at, c.length, looping).saturating_sub(passes(prev_time, *at, c.length, looping));
                for _ in 0..n.min(4) {
                    events.push((name.clone(), uid, pos));
                }
            }
        }
    }
}

/// One clip's root motion: its length and the keys `(time, x, y, z)`.
#[derive(Clone, Debug, PartialEq)]
pub struct MotionClip {
    pub clip: String,
    pub length: f32,
    pub keys: Vec<(f32, V3)>,
}

/// Root motion as text: `walk length=0.8: 0 0 0 0, 0.4 0 0 0.5, 0.8 0 0 1.0` (each key is a time and x y z), one clip a line, times increasing.
pub fn parse_root_motion(text: &str) -> Result<Vec<MotionClip>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (head, body) = line.split_once(':').ok_or_else(|| format!("line {}: expected `clip length=SECONDS: TIME x y z, TIME x y z`", n + 1))?;
        let (clip, length) = clip_head(head, n + 1)?;
        let mut keys: Vec<(f32, V3)> = Vec::new();
        for key in body.split(',').map(str::trim).filter(|k| !k.is_empty()) {
            let nums: Vec<f32> = key.split_whitespace().map(|w| w.parse::<f32>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("line {}: `{w}` is not a number", n + 1))).collect::<Result<_, _>>()?;
            if nums.len() != 4 {
                return Err(format!("line {}: `{key}` is not `TIME x y z`", n + 1));
            }
            if keys.last().is_some_and(|k| nums[0] <= k.0) || nums[0] < 0.0 {
                return Err(format!("line {}: times must increase from 0 (at `{}`)", n + 1, nums[0]));
            }
            keys.push((nums[0], [nums[1], nums[2], nums[3]]));
        }
        if keys.len() < 2 {
            return Err(format!("line {}: a clip needs at least two keys", n + 1));
        }
        out.push(MotionClip { clip, length, keys });
    }
    Ok(out)
}

/// Where the root is at `t` seconds into the clip (held at the first and last key).
fn motion_at(c: &MotionClip, t: f32) -> V3 {
    let (first, last) = (&c.keys[0], &c.keys[c.keys.len() - 1]);
    if t <= first.0 {
        return first.1;
    }
    if t >= last.0 {
        return last.1;
    }
    let k = c.keys.iter().rposition(|k| k.0 <= t).unwrap_or(0).min(c.keys.len() - 2);
    let (a, b) = (&c.keys[k], &c.keys[k + 1]);
    let u = (t - a.0) / (b.0 - a.0);
    [a.1[0] + (b.1[0] - a.1[0]) * u, a.1[1] + (b.1[1] - a.1[1]) * u, a.1[2] + (b.1[2] - a.1[2]) * u]
}

/// Where the root is after `t` seconds of playing the clip: each finished loop adds the clip's whole travel.
fn motion_total(c: &MotionClip, t: f32, looping: bool) -> V3 {
    if !looping {
        return motion_at(c, t.min(c.length));
    }
    let loops = (t / c.length).floor();
    let (start, end) = (motion_at(c, 0.0), motion_at(c, c.length));
    let inside = motion_at(c, t - loops * c.length);
    [inside[0] + (end[0] - start[0]) * loops, inside[1] + (end[1] - start[1]) * loops, inside[2] + (end[2] - start[2]) * loops]
}

fn rotate_by(q: [f32; 4], v: V3) -> V3 {
    let u = [q[0], q[1], q[2]];
    let cross = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let t = cross(u, v);
    let t = [2.0 * t[0], 2.0 * t[1], 2.0 * t[2]];
    let c = cross(u, t);
    [v[0] + q[3] * t[0] + c[0], v[1] + q[3] * t[1] + c[1], v[2] + q[3] * t[2] + c[2]]
}

fn root_motion(world: &mut World) {
    let mut found: Vec<(Entity, u32, f32, u32, f32, u32, f32, bool)> = Vec::new();
    let _ = world.for_each_uid::<(&RootMotion, &Animator), _>(|e, _, (m, a)| found.push((e, m.curves.0, m.scale, m.prev_clip.0, m.prev_time, a.clip.0, a.time, a.looping)));
    for (e, text_sym, scale, prev_clip, prev_time, clip, time, looping) in found {
        if let Some(m) = world.get_mut::<RootMotion>(e) {
            m.prev_clip = lunaris_ecs::Sym(clip);
            m.prev_time = time;
        }
        let first = prev_clip == 0 && clip != 0;
        let prev_time = if first { 0.0 } else { prev_time };
        if (clip != prev_clip && !first) || !(time >= prev_time) || !time.is_finite() || !scale.is_finite() {
            continue;
        }
        let text = world.strings().get(text_sym).to_string();
        let clip_name = world.strings().get(clip).to_string();
        let Some(c) = parse_root_motion(&text).unwrap_or_default().into_iter().find(|c| c.clip == clip_name) else { continue };
        let (a, b) = (motion_total(&c, prev_time, looping), motion_total(&c, time, looping));
        let local = [(b[0] - a[0]) * scale, (b[1] - a[1]) * scale, (b[2] - a[2]) * scale];
        if let Some(t) = world.get_mut::<Transform>(e) {
            let d = rotate_by(t.rot, local);
            t.pos = [t.pos[0] + d[0], t.pos[1] + d[1], t.pos[2] + d[2]];
        }
    }
}

/// The events of the tick before, as resources, for the names an `EventWatch` asks for.
pub fn publish_events(world: &mut World, events: &[crate::sim::Event], tick: u64) {
    let mut syms: Vec<u32> = Vec::new();
    let _ = world.for_each::<(&crate::components::EventWatch,), _>(|_, (w,)| syms.push(w.names.0));
    if syms.is_empty() {
        return;
    }
    let mut names: Vec<String> = Vec::new();
    for s in syms {
        let text = world.strings().get(s).to_string();
        names.extend(text.split(|c: char| c.is_whitespace() || c == ',').filter(|n| !n.is_empty() && lunaris_ecs::valid_resource_name(&format!("event.{n}.uid"))).map(str::to_string));
    }
    names.sort();
    names.dedup();
    for name in names.into_iter().take(64) {
        let recent: Vec<&crate::sim::Event> = events.iter().filter(|e| e.name == name && e.tick + 1 == tick).collect();
        let _ = world.set_resource(&format!("event.{name}"), Some(recent.len() as f64));
        if let Some(first) = recent.first() {
            let p = first.pos.unwrap_or([0.0; 3]);
            for (k, v) in [("x", p[0] as f64), ("y", p[1] as f64), ("z", p[2] as f64), ("uid", first.uid as f64)] {
                let _ = world.set_resource(&format!("event.{name}.{k}"), Some(v));
            }
        }
    }
}

/// The clock of every `Decal`: those that have lived their `life` go.
fn decals(world: &mut World, dt: f32) {
    let mut list: Vec<(u64, Entity, f32, f32)> = Vec::new();
    let _ = world.for_each_uid::<(&crate::components::Decal,), _>(|e, uid, (d,)| list.push((uid, e, d.age, d.life)));
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.0);
    let mut expired: Vec<u64> = Vec::new();
    for (uid, e, age, life) in list {
        let age = age + dt;
        if life > 0.0 && age >= life {
            expired.push(uid);
        } else if let Some(d) = world.get_mut::<crate::components::Decal>(e) {
            d.age = age.min(1.0e6);
        }
    }
    if !expired.is_empty() {
        despawn_trees(world, expired);
    }
}

/// The clock of every `Particles` cloud.
fn particle_clocks(world: &mut World, dt: f32) {
    let _ = world.for_each::<(&mut crate::components::Particles,), _>(|_, (p,)| {
        if p.playing && p.speed.is_finite() && p.time.is_finite() {
            p.time = (p.time + p.speed * dt).min(1.0e6);
        }
    });
}

/// After the graph systems, navigation and characters: lifetimes run out, spawners spawn, the sun moves, sensors look, state machines react,
/// tweens move, cameras follow. Events from the machines go to the script set's events.
pub fn update(world: &mut World, scripts: &mut ScriptSet, tick: u64, dt: f32, fields: &Terrains, meshes: &crate::meshcol::MeshStore) {
    lifetimes(world, dt);
    waters(world, dt);
    decals(world, dt);
    particle_clocks(world, dt);
    root_motion(world);
    sways(world, dt);
    streaming(world, scripts);
    anim_layers(world, dt);
    spawners(world, scripts, dt);
    let mut flow_events: Vec<(String, u64, Option<V3>)> = Vec::new();
    health(world, dt, &mut flow_events);
    behavior_trees(world, scripts, dt, &mut flow_events);
    followers(world, dt);
    timelines(world, dt, &mut flow_events);
    anim_events(world, &mut flow_events);
    crate::spriteanim::update(world, &mut flow_events);
    crate::lookctl::update(world, dt);
    crate::steer::update(world, tick, dt);
    crate::dialogue::update(world, dt, &mut flow_events);
    scripts.events.extend(flow_events);
    time_of_day(world, dt);
    sensors(world, fields, meshes);
    let mut events: Vec<(String, u64, Option<V3>)> = Vec::new();
    state_machines(world, dt, &mut events);
    scripts.events.extend(events);
    tweens(world, dt);
    camera_rigs(world, tick, dt);
    camera_collisions(world, fields, meshes);
}
