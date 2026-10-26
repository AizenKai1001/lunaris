//! The Blueprint tab: a graph system drawn and edited the way Unreal Engine's Blueprint editor does it. Nodes with coloured headers sit
//! anywhere on a dark grid; white arrows are the execution pins, round pins carry values (green numbers, red true/false, pink text);
//! wires are splines; a pin that nothing is wired to holds a value you can type; right click opens the "All Actions" menu with a search box;
//! dragging a wire out of a pin and letting go on an empty place opens it with only the nodes that fit; C wraps the selection in a comment.
//! Compile (Ctrl+S) sends the graph to the world as the JSON of `define_system`, and the positions are kept in `blueprints/<system>.bp.json`.
use crate::blueprint::{self as bpm, Arity, Bp, Cat, Comment, Node, Pin, PinKind, Problem, Ty, EVENT, HEADER, ROW};
use crate::state::{LogKind, Mode, Studio};
use eframe::egui::{
    self, epaint::CubicBezierShape, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Id, Key, Modifiers, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
use lunaris_json::Json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

// ---------------------------------------------------------------------------------------------------------------- the look

const BG: Color32 = Color32::from_rgb(28, 28, 28);
const GRID_MINOR: Color32 = Color32::from_rgb(23, 23, 23);
const GRID_MAJOR: Color32 = Color32::from_rgb(14, 14, 14);
const BODY: Color32 = Color32::from_rgba_premultiplied(17, 17, 18, 238);
const SELECTED: Color32 = Color32::from_rgb(255, 188, 36);
const ERR: Color32 = Color32::from_rgb(235, 70, 60);
const TEXT: Color32 = Color32::from_rgb(235, 235, 235);
const TEXT_DIM: Color32 = Color32::from_rgb(160, 160, 160);
const WHITE_WIRE: Color32 = Color32::from_rgb(240, 240, 240);

fn header_color(cat: Cat) -> Color32 {
    match cat {
        Cat::Event => Color32::from_rgb(176, 32, 32),
        Cat::Flow => Color32::from_rgb(100, 100, 106),
        Cat::Variable => Color32::from_rgb(50, 118, 156),
        Cat::Resource => Color32::from_rgb(160, 106, 40),
        Cat::Entity => Color32::from_rgb(52, 98, 176),
        Cat::Math => Color32::from_rgb(64, 132, 74),
        Cat::Compare => Color32::from_rgb(64, 132, 74),
        Cat::Logic => Color32::from_rgb(150, 56, 56),
        Cat::Utility => Color32::from_rgb(86, 84, 168),
        Cat::Raw => Color32::from_rgb(96, 96, 96),
    }
}

fn ty_color(t: Ty) -> Color32 {
    match t {
        Ty::Num => Color32::from_rgb(112, 232, 88),
        Ty::Bool => Color32::from_rgb(214, 44, 44),
        Ty::Str => Color32::from_rgb(244, 56, 206),
        Ty::Any => Color32::from_rgb(150, 150, 150),
    }
}

const COMMENT_COLORS: [(u8, u8, u8); 5] = [(255, 255, 255), (255, 214, 90), (110, 220, 130), (100, 170, 255), (255, 110, 110)];

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("segoe-bold".into()))
}

fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

// ---------------------------------------------------------------------------------------------------------------- state

#[derive(Clone, Debug, PartialEq)]
enum Drag {
    Pan,
    Nodes { ids: Vec<u32>, comment_carry: Vec<usize>, start: [f32; 2], orig: Vec<(u32, [f32; 2])>, orig_comments: Vec<(usize, [f32; 2])>, moved: bool },
    Wire { from: Pin },
    Marquee { start: Pos2 },
    CommentResize { idx: usize, start: [f32; 2], orig: [f32; 2] },
    CommentMove { idx: usize, start: [f32; 2], orig: [f32; 2], carried: Vec<(u32, [f32; 2])> },
}

#[derive(Clone, Debug, PartialEq)]
enum EditTarget {
    Param(u32, usize),
    Default(u32, usize),
    PinName(u32, usize),
    Comment(usize),
    Raw(u32),
    EventName,
}

struct Editing {
    target: EditTarget,
    text: String,
    at: Pos2,
    width: f32,
    fresh: bool,
    /// Frames since it opened: the click that opened it must not close it again.
    age: u8,
}

struct Menu {
    screen: Pos2,
    graph: [f32; 2],
    search: String,
    from: Option<Pin>,
    selected: usize,
    fresh: bool,
    /// A menu for one node or pin instead of the list of actions.
    target: Option<MenuTarget>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MenuTarget {
    Node(u32),
    Pin(Pin),
    Comment(usize),
}

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Info(String),
    Warn(String),
    Error(String, Option<u32>),
}

struct Doc {
    bp: Bp,
    /// The JSON the world has for this system (what Compile last sent, or what was read).
    applied: Option<Json>,
    undo: Vec<Bp>,
    redo: Vec<Bp>,
    pan: Vec2,
    zoom: f32,
    fit: bool,
    fit_all: bool,
    selected: Vec<u32>,
    problems: Vec<Problem>,
    /// What a trace of the selected entity found (`trace_system`), shown as values at the output pins; `None` when nothing is selected or the graph cannot be traced.
    live: Option<Live>,
    live_at: Option<Instant>,
    /// The statements that have a breakpoint (a red dot on the node): while a game runs, it pauses when the selected entity is about to run one of them.
    breaks: Vec<u32>,
    /// The breakpoint the game stopped at, until it runs on or stops.
    hit: Option<u32>,
}

/// What `trace_system` said about the entity that is selected, by node: the value each data node had when the system ran for it.
#[derive(Clone, Debug, Default)]
struct Live {
    uid: u64,
    tick: i64,
    /// The entity has the components of the system's `each`: otherwise the system does not visit it.
    matched: bool,
    /// What stopped the run (a division by zero ...), with the values found before it.
    error: Option<String>,
    values: HashMap<u32, Json>,
    /// The statements that ran (or, with the game running, that run next) for the entity.
    ran: HashSet<u32>,
}

impl Live {
    /// The values of a trace, put on the nodes they belong to (a value of something that is not a data node, a statement's own field for instance, is left out).
    fn from_trace(bp: &Bp, uid: u64, t: &Json) -> Live {
        let mut values: HashMap<u32, Json> = HashMap::new();
        if let Some(o) = t.get("values").and_then(|v| v.as_object()) {
            for (path, v) in o {
                if let Some(id) = locate(bp, path) {
                    if bp.node(id).is_some_and(|n| !n.spec().exec_in && n.spec().out.is_some()) {
                        values.entry(id).or_insert_with(|| v.clone());
                    }
                }
            }
        }
        let ran: HashSet<u32> = t.get("ran").and_then(|r| r.as_array()).map(|a| a.iter().filter_map(|p| p.as_str().and_then(|p| locate(bp, p))).collect()).unwrap_or_default();
        Live { uid, tick: t.get("tick").and_then(|v| v.as_i64()).unwrap_or(0), matched: t.get("matched").and_then(|v| v.as_bool()).unwrap_or(false), error: t.get("error").and_then(|e| e.as_str()).map(str::to_string), values, ran }
    }
}

/// A value as it is written on a wire: numbers to three decimals (without trailing zeros), very big or small ones in exponent form, flags and text as they are.
fn live_text(v: &Json) -> String {
    match v {
        Json::Bool(b) => b.to_string(),
        Json::Int(i) => i.to_string(),
        Json::Float(f) => {
            if !f.is_finite() {
                return f.to_string();
            }
            if *f != 0.0 && (f.abs() >= 1e6 || f.abs() < 1e-3) {
                return format!("{f:.2e}");
            }
            let s = format!("{f:.3}");
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        }
        Json::Str(s) => format!("\"{}\"", s.chars().take(14).collect::<String>()),
        other => other.to_string_compact().unwrap_or_default().chars().take(16).collect(),
    }
}

#[derive(Default)]
pub struct BlueprintEditor {
    docs: HashMap<String, Doc>,
    current: Option<String>,
    drag: Option<Drag>,
    menu: Option<Menu>,
    editing: Option<Editing>,
    msgs: Vec<Msg>,
    systems: Vec<(String, bool)>,
    systems_at: Option<Instant>,
    resources: Vec<String>,
    show_log: bool,
    canvas: Option<Rect>,
    new_count: u32,
    selected_comment: Option<usize>,
    clipboard: Option<Bp>,
    request_compile: bool,
    last_error: Option<String>,
}

fn layout_file(studio: &Studio, name: &str) -> PathBuf {
    studio.opts.root.join("blueprints").join(format!("{name}.bp.json"))
}

impl BlueprintEditor {
    pub fn is_dirty(&self) -> bool {
        self.docs.values().any(|d| d.dirty())
    }

    /// Ask the tab to compile the system on show (the window's Ctrl+S while this tab is up).
    pub fn request_compile(&mut self) {
        self.request_compile = true;
    }

    /// Open a graph system of the world (or the one that is already open).
    pub fn open(&mut self, studio: &mut Studio, name: &str) {
        if self.docs.contains_key(name) {
            self.current = Some(name.to_string());
            return;
        }
        let def = match studio.engine.call("get_system", &Json::obj([("name", Json::str(name))])) {
            Ok(d) => d.get("def").cloned().unwrap_or(d),
            Err(e) => {
                studio.toast(format!("cannot read system {name}: {}", e.message), true);
                return;
            }
        };
        let from_file = std::fs::read_to_string(layout_file(studio, name)).ok().and_then(|t| lunaris_json::parse(&t).ok()).and_then(|l| Bp::from_layout(&l, &def));
        let bp = match from_file {
            Some(b) => b,
            None => match Bp::from_json(&def) {
                Ok(b) => b,
                Err(e) => {
                    studio.toast(format!("{name} has no nodes: {e}"), true);
                    return;
                }
            },
        };
        self.docs.insert(name.to_string(), Doc { bp, applied: Some(def), undo: vec![], redo: vec![], pan: Vec2::new(40.0, 40.0), zoom: 1.0, fit: true, fit_all: false, selected: vec![], problems: vec![], live: None, live_at: None, breaks: vec![], hit: None });
        self.current = Some(name.to_string());
        self.msgs.push(Msg::Info(format!("opened {name}")));
    }

    fn new_system(&mut self, studio: &mut Studio) {
        self.new_count += 1;
        let mut name = format!("new_system_{}", self.new_count);
        while self.systems.iter().any(|(n, _)| *n == name) || self.docs.contains_key(&name) {
            self.new_count += 1;
            name = format!("new_system_{}", self.new_count);
        }
        let mut bp = Bp::new(&name);
        bp.auto_layout();
        let _ = studio;
        self.docs.insert(name.clone(), Doc { bp, applied: None, undo: vec![], redo: vec![], pan: Vec2::new(40.0, 120.0), zoom: 1.0, fit: false, fit_all: false, selected: vec![], problems: vec![], live: None, live_at: None, breaks: vec![], hit: None });
        self.current = Some(name);
    }

    fn doc(&mut self) -> Option<&mut Doc> {
        let n = self.current.clone()?;
        self.docs.get_mut(&n)
    }

    fn refresh(&mut self, studio: &mut Studio) {
        if self.systems_at.is_none_or(|t| t.elapsed().as_secs_f32() > 2.0) {
            self.systems_at = Some(Instant::now());
            if let Ok(s) = studio.engine.call("systems", &Json::Null) {
                self.systems = s.get("graph").and_then(|g| g.as_array()).map(|a| a.iter().filter_map(|x| x.get("name").and_then(|n| n.as_str()).map(|n| (n.to_string(), x.get("last_error").and_then(|e| e.as_str()).is_some()))).collect()).unwrap_or_default();
            }
            if let Ok(r) = studio.engine.call("resources", &Json::Null) {
                self.resources = r.get("resources").and_then(|o| o.as_object()).map(|o| o.iter().map(|(k, _)| k.clone()).collect()).unwrap_or_default();
            }
        }
    }

    /// Are there breakpoints in the graph that is open?
    pub fn has_breakpoints(&self) -> bool {
        self.current.as_ref().and_then(|n| self.docs.get(n)).is_some_and(|d| !d.breaks.is_empty())
    }

    /// Called after every tick of a running game: is the selected entity about to run a statement that has a breakpoint? Asks `trace_system` what the next tick of the graph on the
    /// screen would do for it (the state after the tick is the state the statement will see: the game stops *before* it runs). Returns the title of the node when it is, and the node is
    /// marked until the game runs on. Nothing is asked in a scene of more than 20 000 entities (a trace copies the world), or for a system of touching pairs.
    pub fn check_breakpoints(&mut self, studio: &mut Studio) -> Option<String> {
        let name = self.current.clone()?;
        let doc = self.docs.get_mut(&name)?;
        doc.hit = None;
        if doc.breaks.is_empty() || doc.bp.meta.contact.is_some() || studio.total_entities > 20_000 {
            return None;
        }
        let uid = studio.selection?;
        let def = doc.bp.to_json().ok()?;
        let t = studio.engine.call("trace_system", &Json::obj([("uid", Json::from(uid as i64)), ("def", def)])).ok()?;
        let live = Live::from_trace(&doc.bp, uid, &t);
        let hit = doc.breaks.iter().copied().find(|b| live.ran.contains(b));
        doc.live = Some(live);
        doc.live_at = Some(Instant::now());
        doc.hit = hit;
        let node = hit?;
        let title = doc.bp.node(node).map(|n| n.title()).unwrap_or("A node").to_string();
        self.msgs.push(Msg::Warn(format!("Breakpoint: {title} is about to run for entity {uid} (tick {})", studio.engine.sim.tick)));
        Some(title)
    }

    /// Ask the engine what the open system does for the selected entity (`trace_system` with the graph as it is now, so that an edit that is not compiled yet shows its values
    /// too), a few times a second, and keep the answer by node. Nothing is asked for a system of touching pairs, for a graph that has problems, or in a scene too big to copy often.
    fn update_live(&mut self, studio: &mut Studio) {
        // the game that stopped at a breakpoint is gone once it is stopped
        if studio.mode == Mode::Edit {
            for d in self.docs.values_mut() {
                d.hit = None;
            }
        }
        let Some(name) = self.current.clone() else { return };
        let Some(doc) = self.docs.get_mut(&name) else { return };
        let Some(uid) = studio.selection else {
            doc.live = None;
            return;
        };
        let every = if studio.mode == Mode::Edit { 0.5 } else { 0.15 };
        if doc.live_at.is_some_and(|t| t.elapsed().as_secs_f32() < every) {
            return;
        }
        doc.live_at = Some(Instant::now());
        if doc.bp.meta.contact.is_some() || studio.total_entities > 100_000 {
            doc.live = None;
            return;
        }
        let Ok(def) = doc.bp.to_json() else {
            doc.live = None;
            return;
        };
        doc.live = studio.engine.call("trace_system", &Json::obj([("uid", Json::from(uid as i64)), ("def", def)])).ok().map(|t| Live::from_trace(&doc.bp, uid, &t));
    }

    /// Send the graph to the world.
    fn compile(&mut self, studio: &mut Studio) {
        let Some(name) = self.current.clone() else { return };
        let Some(doc) = self.docs.get_mut(&name) else { return };
        match doc.bp.to_json() {
            Err(problems) => {
                for p in &problems {
                    self.msgs.push(Msg::Error(p.message.clone(), p.node));
                }
                self.msgs.push(Msg::Error(format!("Compile of {name} failed: {} problem{}", problems.len(), if problems.len() == 1 { "" } else { "s" }), None));
                doc.problems = problems;
                self.show_log = true;
                self.last_error = Some("failed".to_string());
            }
            Ok(def) => {
                doc.problems.clear();
                let t0 = Instant::now();
                match studio.engine.call("define_system", &def) {
                    Ok(_) => {
                        studio.log(LogKind::Call, "define_system".to_string(), def.to_string_compact().unwrap_or_default().chars().take(400).collect(), t0.elapsed().as_secs_f32() * 1000.0);
                        studio.dirty = true;
                        doc.applied = Some(def);
                        let unreached = doc.bp.unreachable();
                        for id in &unreached {
                            self.msgs.push(Msg::Warn(format!("{} is not connected to the event: it does nothing", doc.bp.node(*id).map(|n| n.title()).unwrap_or("A node"))));
                        }
                        self.msgs.push(Msg::Info(format!("[{:.2}] Compile of {name} successful!", t0.elapsed().as_secs_f32())));
                        self.last_error = None;
                        // keep the layout next to the project
                        let file = layout_file(studio, &name);
                        if let Some(dir) = file.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        let _ = std::fs::write(&file, doc.bp.to_layout().to_string_pretty().unwrap_or_default());
                        studio.toast(format!("Compiled {name}"), false);
                    }
                    Err(e) => {
                        let node = e.path.as_deref().and_then(|p| locate(&doc.bp, p));
                        if let Some(n) = node {
                            doc.problems.push(Problem { node: Some(n), message: e.message.clone() });
                        }
                        studio.log(LogKind::Error, format!("define_system: {}", e.message), String::new(), 0.0);
                        self.msgs.push(Msg::Error(format!("{}{}", e.message, e.hint.as_ref().map(|h| format!(" ({h})")).unwrap_or_default()), node));
                        self.msgs.push(Msg::Error(format!("Compile of {name} failed."), None));
                        self.show_log = true;
                        self.last_error = Some(e.message);
                    }
                }
            }
        }
        if self.msgs.len() > 200 {
            self.msgs.drain(0..100);
        }
    }
}

impl Doc {
    fn dirty(&self) -> bool {
        match (&self.applied, self.bp.to_json()) {
            (Some(a), Ok(now)) => !bpm::lunaris_json_eq(a, &now),
            (None, _) => true,
            (Some(_), Err(_)) => true,
        }
    }

    fn snapshot(&mut self) {
        self.undo.push(self.bp.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.bp, prev));
            self.selected.clear();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.bp, next));
            self.selected.clear();
        }
    }
}

/// The node an engine error path (`do[1].then[0].to.add[1]`) belongs to.
fn locate(bp: &Bp, path: &str) -> Option<u32> {
    // split into keys and indexes
    let mut toks: Vec<String> = Vec::new();
    for part in path.split('.') {
        let mut rest = part;
        if let Some(i) = rest.find('[') {
            if i > 0 {
                toks.push(rest[..i].to_string());
            }
            rest = &rest[i..];
            while let Some(r) = rest.strip_prefix('[') {
                let close = r.find(']')?;
                toks.push(format!("[{}", &r[..close]));
                rest = &r[close + 1..];
            }
        } else {
            toks.push(rest.to_string());
        }
    }
    let idx = |t: &str| t.strip_prefix('[').and_then(|n| n.parse::<usize>().ok());
    let mut k = 0;
    let mut chain_pin = Pin { node: EVENT, kind: PinKind::ExecOut(0) };
    let mut at: Option<u32> = None;
    // statements
    while k < toks.len() {
        match toks[k].as_str() {
            "do" => {
                chain_pin = Pin { node: EVENT, kind: PinKind::ExecOut(0) };
                k += 1;
            }
            "then" | "else" => {
                let which = if toks[k] == "then" { 0 } else { 1 };
                chain_pin = Pin { node: at?, kind: PinKind::ExecOut(which) };
                k += 1;
            }
            t if idx(t).is_some() && (at.is_none() || bp.node(at?).map(|n| n.op == "if").unwrap_or(false)) && matches!(toks.get(k.wrapping_sub(1)).map(|s| s.as_str()), Some("do" | "then" | "else")) => {
                let mut n = bp.wires_from(chain_pin).next()?.to.node;
                for _ in 0..idx(t)? {
                    let node = bp.node(n)?;
                    let next = if node.op == "if" { 2 } else { 0 };
                    n = bp.wires_from(Pin { node: n, kind: PinKind::ExecOut(next) }).next()?.to.node;
                }
                at = Some(n);
                k += 1;
            }
            _ => break,
        }
    }
    let mut cur = at?;
    // a value of the statement, then the operations inside it
    let node = bp.node(cur)?;
    if let Some(key) = toks.get(k) {
        let first = match key.as_str() {
            "to" | "be" | "by" | "if" => Some(0),
            "values" | "spawn" | "with" | "at" | "velocity" => None,
            _ => None,
        };
        if let Some(i) = first {
            k += 1;
            let mut input = i;
            loop {
                let Some(w) = bp.wire_into(Pin { node: cur, kind: PinKind::DataIn(input) }) else { break };
                cur = w.from.node;
                // the operation name, then `[i]` for the argument
                if k < toks.len() && idx(&toks[k]).is_none() {
                    k += 1;
                }
                match toks.get(k).and_then(|t| idx(t)) {
                    Some(i) => {
                        input = i;
                        k += 1;
                    }
                    None => break,
                }
            }
        }
    }
    let _ = node;
    Some(cur)
}

#[cfg(test)]
impl BlueprintEditor {
    pub fn test_bp(&self) -> Option<&Bp> {
        self.current.as_ref().and_then(|n| self.docs.get(n)).map(|d| &d.bp)
    }

    /// Where a pin is on the screen.
    pub fn test_pin_screen(&self, pin: Pin) -> Option<Pos2> {
        let d = self.docs.get(self.current.as_ref()?)?;
        let v = View { origin: self.canvas?.min, pan: d.pan, zoom: d.zoom };
        pin_graph_pos(&d.bp, pin).map(|g| v.to_screen(g))
    }

    /// Where the value box of an unconnected input is on the screen.
    pub fn test_default_screen(&self, node: u32, i: usize) -> Option<Pos2> {
        let d = self.docs.get(self.current.as_ref()?)?;
        let v = View { origin: self.canvas?.min, pan: d.pan, zoom: d.zoom };
        let n = d.bp.node(node)?;
        let o = bpm::pin_offset(n, PinKind::DataIn(i));
        let bx = literal_box(n, i, o);
        Some(v.to_screen([n.pos[0] + bx[0] + bx[2] / 2.0, n.pos[1] + bx[1] + bx[3] / 2.0]))
    }

    /// The middle of a node's header on the screen.
    pub fn test_header_screen(&self, node: u32) -> Option<Pos2> {
        let d = self.docs.get(self.current.as_ref()?)?;
        let v = View { origin: self.canvas?.min, pan: d.pan, zoom: d.zoom };
        let (pos, size) = node_rect(&d.bp, node)?;
        Some(v.to_screen([pos[0] + size[0] / 2.0, pos[1] + HEADER / 2.0]))
    }

    pub fn test_menu_open(&self) -> bool {
        self.menu.is_some()
    }

    pub fn test_selected(&self) -> Vec<u32> {
        self.current.as_ref().and_then(|n| self.docs.get(n)).map(|d| d.selected.clone()).unwrap_or_default()
    }

    pub fn test_dirty(&self) -> bool {
        self.is_dirty()
    }

    /// Put a breakpoint on a node (or take it off).
    pub fn test_toggle_break(&mut self, node: u32) {
        if let Some(d) = self.doc() {
            toggle_breakpoint(d, node);
        }
    }

    pub fn test_has_breakpoints(&self) -> bool {
        self.has_breakpoints()
    }

    /// The node the game stopped at, if it did.
    pub fn test_hit(&self) -> Option<u32> {
        self.current.as_ref().and_then(|n| self.docs.get(n)).and_then(|d| d.hit)
    }

    /// Which nodes ran for the selected entity at the last trace.
    pub fn test_ran(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.current.as_ref().and_then(|n| self.docs.get(n)).and_then(|d| d.live.as_ref()).map(|l| l.ran.iter().copied().collect()).unwrap_or_default();
        v.sort();
        v
    }

    /// The live value of a node, as text (what is drawn at its output pin).
    pub fn test_live(&self, node: u32) -> Option<String> {
        self.current.as_ref().and_then(|n| self.docs.get(n)).and_then(|d| d.live.as_ref()).and_then(|l| l.values.get(&node)).map(live_text)
    }

    /// How many nodes have a live value, and whether the trace found the selected entity visited by the system.
    pub fn test_live_summary(&self) -> Option<(usize, bool)> {
        self.current.as_ref().and_then(|n| self.docs.get(n)).and_then(|d| d.live.as_ref()).map(|l| (l.values.len(), l.matched))
    }
}

// ---------------------------------------------------------------------------------------------------------------- geometry

struct View {
    origin: Pos2,
    pan: Vec2,
    zoom: f32,
}

impl View {
    fn to_screen(&self, g: [f32; 2]) -> Pos2 {
        self.origin + self.pan + Vec2::new(g[0], g[1]) * self.zoom
    }

    fn to_graph(&self, p: Pos2) -> [f32; 2] {
        let v = (p - self.origin - self.pan) / self.zoom;
        [v.x, v.y]
    }

    fn rect(&self, pos: [f32; 2], size: [f32; 2]) -> Rect {
        Rect::from_min_size(self.to_screen(pos), Vec2::new(size[0], size[1]) * self.zoom)
    }
}

fn node_rect(bp: &Bp, id: u32) -> Option<([f32; 2], [f32; 2])> {
    if id == EVENT {
        return Some((bp.meta.event_pos, bpm::event_size(&bp.meta)));
    }
    bp.node(id).map(|n| (n.pos, bpm::node_size(n)))
}

fn pin_graph_pos(bp: &Bp, pin: Pin) -> Option<[f32; 2]> {
    if pin.node == EVENT {
        let s = bpm::event_size(&bp.meta);
        return Some([bp.meta.event_pos[0] + s[0] - 10.0, bp.meta.event_pos[1] + HEADER + 6.0 + ROW / 2.0]);
    }
    let n = bp.node(pin.node)?;
    let o = bpm::pin_offset(n, pin.kind);
    Some([n.pos[0] + o[0], n.pos[1] + o[1]])
}

fn all_pins(bp: &Bp) -> Vec<Pin> {
    let mut v = vec![Pin { node: EVENT, kind: PinKind::ExecOut(0) }];
    for n in &bp.nodes {
        if n.spec().exec_in {
            v.push(Pin { node: n.id, kind: PinKind::ExecIn });
        }
        for k in 0..n.exec_outs() {
            v.push(Pin { node: n.id, kind: PinKind::ExecOut(k) });
        }
        for i in 0..n.inputs() {
            v.push(Pin { node: n.id, kind: PinKind::DataIn(i) });
        }
        if n.out_ty().is_some() {
            v.push(Pin { node: n.id, kind: PinKind::DataOut });
        }
    }
    v
}

fn pin_ty(bp: &Bp, pin: Pin) -> Option<Ty> {
    let n = bp.node(pin.node)?;
    match pin.kind {
        PinKind::DataOut => n.out_ty(),
        PinKind::DataIn(i) => Some(n.input_ty(i)),
        _ => None,
    }
}

/// What the pointer is over.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Pin(Pin),
    Default(u32, usize),
    Param(u32, usize),
    AddPin(u32),
    PinLabel(u32, usize),
    EventName,
    Node(u32),
    CommentTitle(usize),
    CommentCorner(usize),
    None,
}

fn hit_test(bp: &Bp, v: &View, p: Pos2) -> Hit {
    let z = v.zoom;
    // pins first: they stick out of the nodes
    let mut best: Option<(f32, Pin)> = None;
    for pin in all_pins(bp) {
        if let Some(g) = pin_graph_pos(bp, pin) {
            let d = v.to_screen(g).distance(p);
            if d <= (9.0 * z).max(7.0) && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, pin));
            }
        }
    }
    if let Some((_, pin)) = best {
        return Hit::Pin(pin);
    }
    for n in bp.nodes.iter().rev() {
        let size = bpm::node_size(n);
        let r = v.rect(n.pos, size);
        if !r.contains(p) {
            continue;
        }
        let local = (p - r.min) / z;
        let sp = n.spec();
        let rows_left = usize::from(sp.exec_in) + n.inputs();
        let rows_right = sp.exec_out.len() + usize::from(sp.out.is_some());
        let pin_rows = rows_left.max(rows_right).max(1);
        // value boxes of unconnected inputs
        for i in 0..n.inputs() {
            if bp.wire_into(Pin { node: n.id, kind: PinKind::DataIn(i) }).is_some() {
                continue;
            }
            let o = bpm::pin_offset(n, PinKind::DataIn(i));
            let bx = literal_box(n, i, o);
            if local.x >= bx[0] && local.x <= bx[0] + bx[2] && local.y >= bx[1] && local.y <= bx[1] + bx[3] {
                return Hit::Default(n.id, i);
            }
            if matches!(n.arity(), Arity::Named) && local.x >= o[0] + 8.0 && local.x < bx[0] && (local.y - o[1]).abs() < ROW / 2.0 {
                return Hit::PinLabel(n.id, i);
            }
        }
        let params_y = HEADER + 6.0 + pin_rows as f32 * ROW + 4.0;
        for (i, _) in sp.params.iter().enumerate() {
            let y = params_y + i as f32 * ROW;
            if local.y >= y && local.y < y + ROW && local.x > 6.0 {
                return Hit::Param(n.id, i);
            }
        }
        if matches!(n.arity(), Arity::Nary { .. } | Arity::Named) {
            let y = HEADER + 6.0 + pin_rows as f32 * ROW - ROW + 3.0;
            let _ = y;
            // the "Add pin" label sits at the right of the last input row
            let add = add_pin_box(n);
            if local.x >= add[0] && local.x <= add[0] + add[2] && local.y >= add[1] && local.y <= add[1] + add[3] {
                return Hit::AddPin(n.id);
            }
        }
        return Hit::Node(n.id);
    }
    // the event node
    let es = bpm::event_size(&bp.meta);
    let r = v.rect(bp.meta.event_pos, es);
    if r.contains(p) {
        let local = (p - r.min) / z;
        if local.y > HEADER && local.y < HEADER + 6.0 + ROW {
            return Hit::EventName;
        }
        return Hit::Node(EVENT);
    }
    for (i, c) in bp.comments.iter().enumerate().rev() {
        let r = v.rect(c.pos, c.size);
        let corner = Rect::from_min_size(r.right_bottom() - Vec2::splat(16.0 * z.max(0.6)), Vec2::splat(16.0 * z.max(0.6)));
        if corner.contains(p) {
            return Hit::CommentCorner(i);
        }
        let title = Rect::from_min_size(r.min, Vec2::new(r.width(), 26.0 * z));
        if title.contains(p) {
            return Hit::CommentTitle(i);
        }
    }
    Hit::None
}

/// The box (x, y, w, h in node space) that shows the value of an unconnected input, to the right of its label.
fn literal_box(n: &Node, i: usize, pin: [f32; 2]) -> [f32; 4] {
    let label_w = n.input_label(i).chars().count() as f32 * 7.2;
    let w = 46.0;
    [pin[0] + 16.0 + label_w + 6.0, pin[1] - 9.0, w, 18.0]
}

fn add_pin_box(n: &Node) -> [f32; 4] {
    let size = bpm::node_size(n);
    let sp = n.spec();
    let rows_left = usize::from(sp.exec_in) + n.inputs();
    let rows_right = sp.exec_out.len() + usize::from(sp.out.is_some());
    let pin_rows = rows_left.max(rows_right);
    // below the last row, left aligned, inside the body
    let y = HEADER + 6.0 + pin_rows as f32 * ROW - 2.0;
    let _ = size;
    [10.0, y, 60.0, 16.0]
}

fn literal_text(j: &Json) -> String {
    match j {
        Json::Int(i) => i.to_string(),
        Json::Float(f) => lunaris_script::float_text(*f),
        Json::Bool(b) => b.to_string(),
        Json::Str(s) => format!("\"{s}\""),
        Json::Null => "-".to_string(),
        other => other.to_string_compact().unwrap_or_default(),
    }
}

fn parse_literal(text: &str, ty: Ty, was: &Json) -> Json {
    let t = text.trim();
    match (ty, was) {
        (Ty::Bool, _) => Json::Bool(matches!(t, "true" | "1" | "yes")),
        (Ty::Str, _) => Json::str(t.trim_matches('"')),
        (_, Json::Str(_)) if t.starts_with('"') || t.parse::<f64>().is_err() && t != "true" && t != "false" => Json::str(t.trim_matches('"')),
        _ => {
            if t == "true" || t == "false" {
                Json::Bool(t == "true")
            } else if let Ok(i) = t.parse::<i64>() {
                Json::Int(i)
            } else if let Ok(f) = t.parse::<f64>() {
                if f.is_finite() {
                    Json::Float(f)
                } else {
                    Json::Int(0)
                }
            } else {
                Json::str(t.trim_matches('"'))
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------- drawing

fn bezier(painter: &egui::Painter, a: Pos2, b: Pos2, z: f32, color: Color32, width: f32) {
    let dx = ((b.x - a.x).abs() * 0.5).max(50.0 * z);
    let (c1, c2) = (a + Vec2::new(dx, 0.0), b - Vec2::new(dx, 0.0));
    painter.add(Shape::CubicBezier(CubicBezierShape::from_points_stroke([a, c1, c2, b], false, Color32::TRANSPARENT, Stroke::new(width, color))));
}

fn exec_pin(painter: &egui::Painter, c: Pos2, z: f32, connected: bool, hot: bool) {
    let s = 6.0 * z;
    let pts = vec![c + Vec2::new(-s * 0.8, -s), c + Vec2::new(s * 0.1, -s), c + Vec2::new(s * 1.0, 0.0), c + Vec2::new(s * 0.1, s), c + Vec2::new(-s * 0.8, s)];
    let col = if hot { SELECTED } else { WHITE_WIRE };
    painter.add(Shape::convex_polygon(pts, if connected { col } else { Color32::from_rgba_unmultiplied(0, 0, 0, 120) }, Stroke::new((1.6 * z).max(1.0), col)));
}

fn data_pin(painter: &egui::Painter, c: Pos2, z: f32, ty: Ty, connected: bool, hot: bool) {
    let col = if hot { SELECTED } else { ty_color(ty) };
    painter.circle_filled(c, 5.6 * z, if connected { col } else { Color32::from_rgb(20, 20, 20) });
    painter.circle_stroke(c, 5.6 * z, Stroke::new((1.8 * z).max(1.0), col));
}

/// Small drawn icons for the toolbar buttons.
fn toolbar_icon(p: &egui::Painter, id: &str, c: Pos2) {
    let s = Stroke::new(1.5, Color32::from_rgb(190, 190, 190));
    match id {
        "save" => {
            let r = Rect::from_center_size(c, Vec2::splat(13.0));
            p.rect_stroke(r, CornerRadius::same(2), s, StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_size(r.min + Vec2::new(3.0, 0.0), Vec2::new(7.0, 4.5)), CornerRadius::ZERO, s, StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_size(r.min + Vec2::new(3.0, 8.0), Vec2::new(7.0, 5.0)), CornerRadius::ZERO, s, StrokeKind::Middle);
        }
        "arrange" => {
            for (dx, dy, w) in [(-6.0, -5.0, 5.0), (0.0, 0.0, 6.0), (-6.0, 5.0, 5.0)] {
                p.rect_stroke(Rect::from_min_size(c + Vec2::new(dx, dy - 2.0), Vec2::new(w, 4.0)), CornerRadius::same(1), s, StrokeKind::Middle);
            }
            p.line_segment([c + Vec2::new(-1.0, -3.0), c + Vec2::new(0.0, 0.0)], s);
            p.line_segment([c + Vec2::new(-1.0, 5.0), c + Vec2::new(0.0, 0.0)], s);
        }
        "frame" => {
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let k = c + Vec2::new(6.5 * sx, 6.5 * sy);
                p.line_segment([k, k - Vec2::new(4.0 * sx, 0.0)], s);
                p.line_segment([k, k - Vec2::new(0.0, 4.0 * sy)], s);
            }
        }
        "script" => {
            p.text(c, Align2::CENTER_CENTER, "{ }", FontId::monospace(12.0), Color32::from_rgb(190, 190, 190));
        }
        _ => {
            for dy in [-4.0, 0.0, 4.0] {
                p.line_segment([c + Vec2::new(-6.0, dy), c + Vec2::new(6.0, dy)], s);
            }
        }
    }
}

impl BlueprintEditor {
    /// The middle of the window: the toolbar, the graph, the compiler results.
    pub fn show(&mut self, ui: &mut Ui, studio: &mut Studio) {
        self.refresh(studio);
        if self.current.is_none() {
            if let Some((n, _)) = self.systems.first().cloned() {
                self.open(studio, &n);
            }
        }
        let save = ui.ctx().input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S));
        if std::mem::take(&mut self.request_compile) || save {
            self.compile(studio);
        }
        self.update_live(studio);
        self.toolbar(ui, studio);
        let log_h = if self.show_log { 110.0 } else { 0.0 };
        let avail = ui.available_rect_before_wrap();
        let canvas = Rect::from_min_max(avail.min, avail.max - Vec2::new(0.0, log_h));
        self.canvas = Some(canvas);
        self.canvas_ui(ui, canvas, studio);
        if self.show_log {
            let log = Rect::from_min_max(Pos2::new(avail.min.x, canvas.max.y), avail.max);
            self.log_ui(ui, log);
        }
        ui.allocate_rect(avail, Sense::hover());
    }

    fn toolbar(&mut self, ui: &mut Ui, studio: &mut Studio) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(34, 34, 34));
        p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, Color32::from_rgb(15, 15, 15)));
        let status = match (&self.current.clone().and_then(|n| self.docs.get(&n).map(|d| d.dirty())), &self.last_error) {
            (_, Some(_)) => 2,
            (Some(true), _) => 1,
            _ => 0,
        };
        let mut x = rect.min.x + 8.0;
        let mut buttons: Vec<(&str, &str, bool)> = vec![("Compile", "compile", true), ("Save", "save", true), ("|", "", false), ("Arrange", "arrange", true), ("Frame All", "frame", true), ("Edit as Script", "script", true), ("|", "", false), ("Compiler Results", "log", true)];
        let mut action: Option<&str> = None;
        for (label, id, active) in buttons.drain(..) {
            if label == "|" {
                p.line_segment([Pos2::new(x + 4.0, rect.min.y + 6.0), Pos2::new(x + 4.0, rect.max.y - 6.0)], Stroke::new(1.0, Color32::from_rgb(70, 70, 70)));
                x += 10.0;
                continue;
            }
            let g = p.layout_no_wrap(label.to_string(), FontId::proportional(12.5), TEXT);
            let w = g.size().x + if id == "compile" { 52.0 } else { 36.0 };
            let r = Rect::from_min_size(Pos2::new(x, rect.min.y + 3.0), Vec2::new(w, 28.0));
            let resp = ui.interact(r, Id::new(("bp-tool", id)), Sense::click());
            if resp.hovered() && active {
                p.rect_filled(r, CornerRadius::same(3), Color32::from_rgb(58, 58, 58));
            }
            // an icon at the left
            let c = Pos2::new(r.min.x + 17.0, r.center().y);
            match id {
                "compile" => {
                    // the status: a green check, a yellow "?" when the graph changed, a red cross after a failure
                    let col = [Color32::from_rgb(90, 200, 90), Color32::from_rgb(240, 190, 60), ERR][status];
                    p.circle_filled(c, 8.0, Color32::from_rgb(25, 25, 25));
                    p.circle_stroke(c, 8.0, Stroke::new(1.6, col));
                    match status {
                        0 => {
                            p.line_segment([c + Vec2::new(-3.5, 0.0), c + Vec2::new(-1.0, 3.0)], Stroke::new(1.8, col));
                            p.line_segment([c + Vec2::new(-1.0, 3.0), c + Vec2::new(4.0, -3.5)], Stroke::new(1.8, col));
                        }
                        1 => {
                            p.text(c, Align2::CENTER_CENTER, "?", FontId::proportional(11.0), col);
                        }
                        _ => {
                            p.line_segment([c + Vec2::new(-3.0, -3.0), c + Vec2::new(3.0, 3.0)], Stroke::new(1.8, col));
                            p.line_segment([c + Vec2::new(-3.0, 3.0), c + Vec2::new(3.0, -3.0)], Stroke::new(1.8, col));
                        }
                    }
                }
                other => toolbar_icon(&p, other, c),
            }
            p.text(Pos2::new(r.min.x + if id == "compile" { 32.0 } else { 28.0 }, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(12.5), TEXT);
            if resp.clicked() {
                action = Some(id);
            }
            x += w + 4.0;
        }
        // where we are, at the right: "orb_dash > timer"
        if let Some(n) = &self.current {
            let scene = studio.scene.clone().unwrap_or_else(|| "Scene".to_string());
            let scene = scene.rsplit('/').next().unwrap_or(&scene).trim_end_matches(".lunaris.json").to_string();
            let crumb = p.text(Pos2::new(rect.max.x - 12.0, rect.center().y), Align2::RIGHT_CENTER, format!("{scene}  >  {n}"), FontId::proportional(12.5), TEXT_DIM);
            // what the values on the wires are the values of
            let stopped = self.docs.get(n).and_then(|d| d.hit.map(|_| d.live.as_ref().map(|l| l.uid).unwrap_or(0)));
            let status = match self.docs.get(n).and_then(|d| d.live.as_ref()) {
                _ if stopped.is_some() => Some((format!("stopped at a breakpoint (entity {}): Play or Step goes on", stopped.unwrap_or(0)), Color32::from_rgb(255, 120, 100))),
                Some(l) if !l.matched => Some((format!("entity {} is not visited by this system", l.uid), TEXT_DIM)),
                Some(l) => Some((
                    format!("live: entity {}, tick {}{}", l.uid, l.tick, l.error.as_ref().map(|e| format!("   stopped: {e}")).unwrap_or_default()),
                    if l.error.is_some() { ERR } else { Color32::from_rgb(110, 200, 130) },
                )),
                None if studio.selection.is_none() => Some(("select an entity to see the values on the wires".to_string(), TEXT_DIM)),
                None => None,
            };
            if let Some((text, color)) = status {
                // (a status that is longer than the room between the buttons and the crumb is cut short, not drawn over the buttons)
                let shown = fit_text(&p, &text, FontId::proportional(12.0), (crumb.min.x - 24.0) - (x + 8.0));
                if !shown.is_empty() {
                    p.text(Pos2::new(crumb.min.x - 24.0, rect.center().y), Align2::RIGHT_CENTER, shown, FontId::proportional(12.0), color);
                }
            }
        }
        match action {
            Some("compile") | Some("save") => self.compile(studio),
            Some("arrange") => {
                if let Some(d) = self.doc() {
                    d.snapshot();
                    d.bp.auto_layout();
                    d.fit = true;
                    d.fit_all = true;
                }
            }
            Some("frame") => {
                if let Some(d) = self.doc() {
                    d.fit = true;
                    d.fit_all = true;
                }
            }
            Some("script") => {
                studio.open_script = self.current.clone();
            }
            Some("log") => self.show_log = !self.show_log,
            _ => {}
        }
    }

    fn log_ui(&mut self, ui: &mut Ui, rect: Rect) {
        let p = ui.painter().clone();
        p.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(24, 24, 24));
        p.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, Color32::from_rgb(10, 10, 10)));
        p.text(rect.min + Vec2::new(10.0, 12.0), Align2::LEFT_CENTER, "Compiler Results", FontId::proportional(12.0), TEXT_DIM);
        let body = Rect::from_min_max(rect.min + Vec2::new(0.0, 24.0), rect.max);
        let mut jump: Option<u32> = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            egui::ScrollArea::vertical().id_salt("bp-log").stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                for m in &self.msgs {
                    let (color, text, node) = match m {
                        Msg::Info(t) => (Color32::from_rgb(120, 200, 120), t.clone(), None),
                        Msg::Warn(t) => (Color32::from_rgb(235, 190, 80), format!("Warning: {t}"), None),
                        Msg::Error(t, n) => (ERR, format!("Error: {t}"), *n),
                    };
                    let r = ui.add(egui::Label::new(RichText::new(text).size(12.0).color(color)).sense(if node.is_some() { Sense::click() } else { Sense::hover() }));
                    if r.clicked() {
                        jump = node;
                    }
                }
            });
        });
        if let Some(n) = jump {
            if let Some(d) = self.doc() {
                d.selected = vec![n];
            }
        }
    }

    fn canvas_ui(&mut self, ui: &mut Ui, rect: Rect, studio: &mut Studio) {
        let Some(name) = self.current.clone() else {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, BG);
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "Pick a graph system in My Blueprint, or create one with +", FontId::proportional(14.0), TEXT_DIM);
            return;
        };
        if !self.docs.contains_key(&name) {
            return;
        }
        let ctx = ui.ctx().clone();
        let resp = ui.interact(rect, Id::new("bp-canvas"), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, BG);
        let mut drag = self.drag.take();
        let mut menu = self.menu.take();
        let mut editing = self.editing.take();
        let mut selected_comment = self.selected_comment;
        let ptr = ctx.input(|i| i.pointer.hover_pos());
        let editing_open = editing.is_some() || menu.is_some();

        {
            let doc = self.docs.get_mut(&name).unwrap();
            if doc.fit {
                fit(doc, rect, doc.fit_all);
                doc.fit = false;
                doc.fit_all = false;
            }
            let view = View { origin: rect.min, pan: doc.pan, zoom: doc.zoom };
            // ---- the grid
            let step = 16.0 * doc.zoom;
            if step > 4.0 {
                let (ox, oy) = ((view.pan.x).rem_euclid(step), (view.pan.y).rem_euclid(step));
                let mut x = rect.min.x + ox;
                let mut n = ((x - rect.min.x - view.pan.x) / step).round() as i64;
                while x < rect.max.x {
                    let major = n.rem_euclid(8) == 0;
                    painter.line_segment([Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)], Stroke::new(if major { 1.4 } else { 1.0 }, if major { GRID_MAJOR } else { GRID_MINOR }));
                    x += step;
                    n += 1;
                }
                let mut y = rect.min.y + oy;
                let mut n = ((y - rect.min.y - view.pan.y) / step).round() as i64;
                while y < rect.max.y {
                    let major = n.rem_euclid(8) == 0;
                    painter.line_segment([Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)], Stroke::new(if major { 1.4 } else { 1.0 }, if major { GRID_MAJOR } else { GRID_MINOR }));
                    y += step;
                    n += 1;
                }
            }
            painter.text(rect.right_bottom() - Vec2::new(26.0, 20.0), Align2::RIGHT_BOTTOM, "BLUEPRINT", FontId::proportional(54.0), Color32::from_white_alpha(14));

            // ---- input
            let hit = ptr.filter(|p| rect.contains(*p) && resp.hovered() || matches!(drag, Some(Drag::Wire { .. }))).map(|p| hit_test(&doc.bp, &view, p)).unwrap_or(Hit::None);
            let (primary_pressed, primary_released, secondary_pressed, secondary_released, mods, delta, scroll, primary_down, double) = ctx.input(|i| {
                (
                    i.pointer.button_pressed(egui::PointerButton::Primary),
                    i.pointer.button_released(egui::PointerButton::Primary),
                    i.pointer.button_pressed(egui::PointerButton::Secondary),
                    i.pointer.button_released(egui::PointerButton::Secondary),
                    i.modifiers,
                    i.pointer.delta(),
                    i.smooth_scroll_delta.y,
                    i.pointer.primary_down(),
                    i.pointer.button_double_clicked(egui::PointerButton::Primary),
                )
            });
            let over_canvas = resp.hovered() && !editing_open;
            // zoom around the pointer
            if over_canvas && scroll != 0.0 {
                if let Some(p) = ptr {
                    let k = (doc.zoom * (1.0 + scroll * 0.0016)).clamp(0.25, 2.0);
                    let local = p - rect.min;
                    doc.pan = local - (local - doc.pan) * (k / doc.zoom);
                    doc.zoom = k;
                }
            }
            if over_canvas {
                if primary_pressed {
                    let p = ptr.unwrap_or(rect.center());
                    match hit {
                        Hit::Pin(pin) => {
                            if mods.alt {
                                doc.snapshot();
                                doc.bp.disconnect(pin);
                            } else {
                                // an input that has a wire: pulling it out moves that wire
                                let existing = match pin.kind {
                                    PinKind::DataIn(_) | PinKind::ExecIn => doc.bp.wire_into(pin).map(|w| w.from),
                                    _ => None,
                                };
                                drag = Some(Drag::Wire { from: existing.unwrap_or(pin) });
                                if existing.is_some() {
                                    doc.snapshot();
                                    doc.bp.wires.retain(|w| w.to != pin);
                                }
                            }
                        }
                        Hit::Default(id, i) => {
                            let node = doc.bp.node(id).cloned();
                            if let Some(n) = node {
                                let ty = n.input_ty(i);
                                if ty == Ty::Bool {
                                    doc.snapshot();
                                    let cur = matches!(n.defaults.get(i), Some(Json::Bool(true)));
                                    if let Some(m) = doc.bp.node_mut(id) {
                                        if i < m.defaults.len() {
                                            m.defaults[i] = Json::Bool(!cur);
                                        }
                                    }
                                } else {
                                    let o = bpm::pin_offset(&n, PinKind::DataIn(i));
                                    let bx = literal_box(&n, i, o);
                                    let at = view.to_screen([n.pos[0] + bx[0], n.pos[1] + bx[1]]);
                                    editing = Some(Editing { target: EditTarget::Default(id, i), text: n.defaults.get(i).map(|d| match d { Json::Str(s) => s.clone(), other => literal_text(other) }).unwrap_or_default(), at, width: 110.0, fresh: true, age: 0 });
                                }
                            }
                        }
                        Hit::Param(id, i) => {
                            if let Some(n) = doc.bp.node(id) {
                                let sp = n.spec();
                                let rows_left = usize::from(sp.exec_in) + n.inputs();
                                let rows_right = sp.exec_out.len() + usize::from(sp.out.is_some());
                                let pin_rows = rows_left.max(rows_right).max(1);
                                let y = HEADER + 6.0 + pin_rows as f32 * ROW + 4.0 + i as f32 * ROW;
                                let at = view.to_screen([n.pos[0] + 8.0, n.pos[1] + y]);
                                let w = bpm::node_size(n)[0] * doc.zoom;
                                let target = if n.op.starts_with("raw") { EditTarget::Raw(id) } else { EditTarget::Param(id, i) };
                                editing = Some(Editing { target, text: n.params.get(i).cloned().unwrap_or_default(), at, width: w.max(160.0), fresh: true, age: 0 });
                            }
                        }
                        Hit::PinLabel(id, i) => {
                            if double {
                                if let Some(n) = doc.bp.node(id) {
                                    let o = bpm::pin_offset(n, PinKind::DataIn(i));
                                    let at = view.to_screen([n.pos[0] + o[0] + 12.0, n.pos[1] + o[1] - 9.0]);
                                    editing = Some(Editing { target: EditTarget::PinName(id, i), text: n.named.get(i).cloned().unwrap_or_default(), at, width: 150.0, fresh: true, age: 0 });
                                }
                            } else {
                                select(doc, id, mods.shift || mods.command);
                                start_node_drag(doc, &view, p, &mut drag);
                            }
                        }
                        Hit::AddPin(id) => {
                            doc.snapshot();
                            let is_named = doc.bp.node(id).map(|n| matches!(n.arity(), Arity::Named)).unwrap_or(false);
                            if let Some(n) = doc.bp.node_mut(id) {
                                n.add_input(if n.op == "spawn_prefab" { "at.x" } else if n.op == "add_comp" { "field" } else { "Transform.pos" });
                            }
                            if is_named {
                                if let Some(n) = doc.bp.node(id) {
                                    let i = n.inputs() - 1;
                                    let o = bpm::pin_offset(n, PinKind::DataIn(i));
                                    let at = view.to_screen([n.pos[0] + o[0] + 12.0, n.pos[1] + o[1] - 9.0]);
                                    editing = Some(Editing { target: EditTarget::PinName(id, i), text: n.named.get(i).cloned().unwrap_or_default(), at, width: 150.0, fresh: true, age: 0 });
                                }
                            }
                        }
                        Hit::EventName => {
                            select(doc, EVENT, false);
                            if double {
                                let at = view.to_screen([doc.bp.meta.event_pos[0] + 8.0, doc.bp.meta.event_pos[1] + HEADER + 6.0]);
                                editing = Some(Editing { target: EditTarget::EventName, text: doc.bp.meta.doc.clone(), at, width: 220.0, fresh: true, age: 0 });
                            } else {
                                start_node_drag(doc, &view, p, &mut drag);
                            }
                        }
                        Hit::Node(id) => {
                            if !doc.selected.contains(&id) || mods.shift || mods.command {
                                select(doc, id, mods.shift || mods.command);
                            }
                            selected_comment = None;
                            start_node_drag(doc, &view, p, &mut drag);
                        }
                        Hit::CommentTitle(i) => {
                            selected_comment = Some(i);
                            doc.selected.clear();
                            if double {
                                let c = &doc.bp.comments[i];
                                let at = view.to_screen([c.pos[0] + 6.0, c.pos[1] + 2.0]);
                                editing = Some(Editing { target: EditTarget::Comment(i), text: c.title.clone(), at, width: (c.size[0] * doc.zoom).max(160.0), fresh: true, age: 0 });
                            } else {
                                let c = doc.bp.comments[i].clone();
                                let (cx0, cy0, cx1, cy1) = (c.pos[0], c.pos[1], c.pos[0] + c.size[0], c.pos[1] + c.size[1]);
                                let carried: Vec<(u32, [f32; 2])> = doc.bp.nodes.iter().filter(|n| {
                                    let s = bpm::node_size(n);
                                    n.pos[0] >= cx0 && n.pos[1] >= cy0 && n.pos[0] + s[0] <= cx1 && n.pos[1] + s[1] <= cy1
                                }).map(|n| (n.id, n.pos)).collect();
                                doc.snapshot();
                                drag = Some(Drag::CommentMove { idx: i, start: view.to_graph(p), orig: c.pos, carried });
                            }
                        }
                        Hit::CommentCorner(i) => {
                            selected_comment = Some(i);
                            doc.snapshot();
                            drag = Some(Drag::CommentResize { idx: i, start: view.to_graph(p), orig: doc.bp.comments[i].size });
                        }
                        Hit::None => {
                            if !(mods.shift || mods.command) {
                                doc.selected.clear();
                            }
                            selected_comment = None;
                            drag = Some(Drag::Marquee { start: p });
                        }
                    }
                }
                if secondary_pressed {
                    drag = Some(Drag::Pan);
                }
            }
            // ---- dragging
            match &mut drag {
                Some(Drag::Pan) => {
                    doc.pan += delta;
                    if secondary_released {
                        let moved = ctx.input(|i| i.pointer.press_origin()).zip(ptr).map(|(a, b)| a.distance(b)).unwrap_or(0.0);
                        if moved < 5.0 {
                            if let Some(p) = ptr {
                                // a click without a drag: the menu of what is under the pointer
                                let g = view.to_graph(p);
                                let target = match hit {
                                    Hit::Pin(pin) => Some(MenuTarget::Pin(pin)),
                                    Hit::Node(id) | Hit::Param(id, _) | Hit::Default(id, _) | Hit::PinLabel(id, _) | Hit::AddPin(id) if id != EVENT => Some(MenuTarget::Node(id)),
                                    Hit::CommentTitle(i) | Hit::CommentCorner(i) => Some(MenuTarget::Comment(i)),
                                    _ => None,
                                };
                                menu = Some(Menu { screen: p, graph: g, search: String::new(), from: None, selected: 0, fresh: true, target });
                            }
                        }
                        drag = None;
                    }
                }
                Some(Drag::Nodes { ids, comment_carry: _, start, orig, orig_comments: _, moved }) => {
                    if let Some(p) = ptr {
                        let g = view.to_graph(p);
                        let (dx, dy) = (g[0] - start[0], g[1] - start[1]);
                        if !*moved && (dx.abs() + dy.abs()) * doc.zoom > 4.0 {
                            *moved = true;
                            doc.snapshot();
                        }
                        if *moved {
                            for (id, o) in orig.iter() {
                                if *id == EVENT {
                                    doc.bp.meta.event_pos = [o[0] + dx, o[1] + dy];
                                } else if let Some(n) = doc.bp.node_mut(*id) {
                                    n.pos = [o[0] + dx, o[1] + dy];
                                }
                            }
                        }
                    }
                    let _ = ids;
                    if primary_released || !primary_down {
                        drag = None;
                    }
                }
                Some(Drag::CommentMove { idx, start, orig, carried }) => {
                    if let Some(p) = ptr {
                        let g = view.to_graph(p);
                        let (dx, dy) = (g[0] - start[0], g[1] - start[1]);
                        if let Some(c) = doc.bp.comments.get_mut(*idx) {
                            c.pos = [orig[0] + dx, orig[1] + dy];
                        }
                        for (id, o) in carried.iter() {
                            if let Some(n) = doc.bp.node_mut(*id) {
                                n.pos = [o[0] + dx, o[1] + dy];
                            }
                        }
                    }
                    if primary_released || !primary_down {
                        drag = None;
                    }
                }
                Some(Drag::CommentResize { idx, start, orig }) => {
                    if let Some(p) = ptr {
                        let g = view.to_graph(p);
                        if let Some(c) = doc.bp.comments.get_mut(*idx) {
                            c.size = [(orig[0] + g[0] - start[0]).max(80.0), (orig[1] + g[1] - start[1]).max(50.0)];
                        }
                    }
                    if primary_released || !primary_down {
                        drag = None;
                    }
                }
                Some(Drag::Marquee { start }) => {
                    if let Some(p) = ptr {
                        let sel = Rect::from_two_pos(*start, p);
                        painter.rect_filled(sel, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(255, 255, 255, 14));
                        painter.rect_stroke(sel, CornerRadius::ZERO, Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 120)), StrokeKind::Inside);
                        if primary_released || !primary_down {
                            let mut ids: Vec<u32> = Vec::new();
                            for n in &doc.bp.nodes {
                                if view.rect(n.pos, bpm::node_size(n)).intersects(sel) {
                                    ids.push(n.id);
                                }
                            }
                            if view.rect(doc.bp.meta.event_pos, bpm::event_size(&doc.bp.meta)).intersects(sel) {
                                ids.push(EVENT);
                            }
                            if mods.shift || mods.command {
                                for i in ids {
                                    if !doc.selected.contains(&i) {
                                        doc.selected.push(i);
                                    }
                                }
                            } else {
                                doc.selected = ids;
                            }
                            drag = None;
                        }
                    }
                }
                Some(Drag::Wire { from }) => {
                    let from = *from;
                    if let Some(p) = ptr {
                        if primary_released || !primary_down {
                            // let go: on a pin it connects, on nothing it opens the menu of nodes that fit
                            match hit_test(&doc.bp, &view, p) {
                                Hit::Pin(to) => {
                                    if doc.bp.can_connect(from, to).is_some() {
                                        doc.snapshot();
                                        doc.bp.connect(from, to);
                                    }
                                }
                                Hit::None if rect.contains(p) => {
                                    menu = Some(Menu { screen: p, graph: view.to_graph(p), search: String::new(), from: Some(from), selected: 0, fresh: true, target: None });
                                }
                                _ => {}
                            }
                            drag = None;
                        }
                    } else if primary_released {
                        drag = None;
                    }
                }
                None => {}
            }
            // a click that does not start a drag stays a click
            let _ = (primary_released, double);
            // ---- keys
            // keys belong to the graph only while nothing else has the keyboard (typing a name in Details must not delete nodes)
            if !editing_open && resp.hovered() && ctx.memory(|m| m.focused().is_none()) {
                let toggle_break = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F9));
                let (del, dup, all, comment, frame, undo, redo, copy, paste) = ctx.input_mut(|i| {
                    (
                        i.consume_key(Modifiers::NONE, Key::Delete) || i.consume_key(Modifiers::NONE, Key::Backspace),
                        i.consume_key(Modifiers::COMMAND, Key::D),
                        i.consume_key(Modifiers::COMMAND, Key::A),
                        i.consume_key(Modifiers::NONE, Key::C),
                        i.consume_key(Modifiers::NONE, Key::F) || i.consume_key(Modifiers::NONE, Key::Home),
                        i.consume_key(Modifiers::COMMAND, Key::Z),
                        i.consume_key(Modifiers::COMMAND, Key::Y) || i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
                        i.consume_key(Modifiers::COMMAND, Key::C),
                        i.consume_key(Modifiers::COMMAND, Key::V),
                    )
                });
                if toggle_break {
                    for id in doc.selected.clone() {
                        toggle_breakpoint(doc, id);
                    }
                }
                if del {
                    if let Some(i) = selected_comment.take() {
                        if i < doc.bp.comments.len() {
                            doc.snapshot();
                            doc.bp.comments.remove(i);
                        }
                    } else if !doc.selected.is_empty() {
                        doc.snapshot();
                        for id in doc.selected.clone() {
                            doc.bp.remove_node(id);
                        }
                        doc.selected.clear();
                    }
                }
                if dup && !doc.selected.is_empty() {
                    doc.snapshot();
                    let ids = doc.selected.clone();
                    doc.selected = doc.bp.duplicate(&ids, [30.0, 30.0]);
                }
                if all {
                    doc.selected = std::iter::once(EVENT).chain(doc.bp.nodes.iter().map(|n| n.id)).collect();
                }
                if comment && !doc.selected.is_empty() {
                    doc.snapshot();
                    add_comment_around(doc);
                }
                if frame {
                    if doc.selected.is_empty() {
                        doc.fit = true;
                        doc.fit_all = true;
                    } else {
                        focus_selection(doc, rect);
                    }
                }
                if undo {
                    doc.undo();
                }
                if redo {
                    doc.redo();
                }
                if copy && !doc.selected.is_empty() {
                    // copy: a graph of only the selected nodes (without the event)
                    let mut c = doc.bp.clone();
                    let keep = doc.selected.clone();
                    let drop_ids: Vec<u32> = c.nodes.iter().filter(|n| !keep.contains(&n.id)).map(|n| n.id).collect();
                    for id in drop_ids {
                        c.remove_node(id);
                    }
                    c.wires.retain(|w| w.from.node != EVENT);
                    self.clipboard = Some(c);
                }
                if paste {
                    if let Some(c) = self.clipboard.clone() {
                        doc.snapshot();
                        let ids: Vec<u32> = c.nodes.iter().map(|n| n.id).collect();
                        // add the copies to this graph with fresh ids and the wires between them
                        let mut mapped: Vec<(u32, u32)> = Vec::new();
                        let at = ptr.map(|p| view.to_graph(p)).unwrap_or([0.0, 0.0]);
                        let top_left = c.nodes.iter().fold([f32::MAX, f32::MAX], |m, n| [m[0].min(n.pos[0]), m[1].min(n.pos[1])]);
                        for id in &ids {
                            if let Some(n) = c.node(*id) {
                                let nid = doc.bp.add_node(&n.op, [at[0] + n.pos[0] - top_left[0], at[1] + n.pos[1] - top_left[1]]);
                                if let Some(m) = doc.bp.node_mut(nid) {
                                    let (p, nm, ni, d, r) = (n.params.clone(), n.named.clone(), n.n_in, n.defaults.clone(), n.raw.clone());
                                    m.params = p;
                                    m.named = nm;
                                    m.n_in = ni;
                                    m.defaults = d;
                                    m.raw = r;
                                }
                                mapped.push((*id, nid));
                            }
                        }
                        let find = |id: u32| mapped.iter().find(|(o, _)| *o == id).map(|(_, n)| *n);
                        for w in &c.wires {
                            if let (Some(f), Some(t)) = (find(w.from.node), find(w.to.node)) {
                                doc.bp.connect(Pin { node: f, kind: w.from.kind }, Pin { node: t, kind: w.to.kind });
                            }
                        }
                        doc.selected = mapped.iter().map(|(_, n)| *n).collect();
                    }
                }
            }

            // ---- comments (behind everything)
            for (i, c) in doc.bp.comments.iter().enumerate() {
                let r = view.rect(c.pos, c.size);
                if !r.intersects(rect) {
                    continue;
                }
                let (cr, cg, cb) = COMMENT_COLORS[c.color % COMMENT_COLORS.len()];
                let col = Color32::from_rgb(cr, cg, cb);
                let z = doc.zoom;
                painter.rect_filled(r, CornerRadius::same((4.0 * z) as u8), alpha(col, 34));
                painter.rect_stroke(r, CornerRadius::same((4.0 * z) as u8), Stroke::new(if selected_comment == Some(i) { 2.0 } else { 1.0 }, if selected_comment == Some(i) { SELECTED } else { alpha(col, 120) }), StrokeKind::Inside);
                let title = Rect::from_min_size(r.min, Vec2::new(r.width(), 26.0 * z));
                painter.rect_filled(title, CornerRadius { nw: (4.0 * z) as u8, ne: (4.0 * z) as u8, sw: 0, se: 0 }, alpha(col, 70));
                if z > 0.35 {
                    painter.text(title.min + Vec2::new(8.0 * z, 13.0 * z), Align2::LEFT_CENTER, &c.title, bold(14.0 * z), TEXT);
                }
                // the resize corner
                let corner = r.right_bottom();
                painter.add(Shape::convex_polygon(vec![corner, corner - Vec2::new(12.0 * z, 0.0), corner - Vec2::new(0.0, 12.0 * z)], alpha(col, 120), Stroke::NONE));
            }

            // ---- wires
            for w in &doc.bp.wires {
                let (Some(a), Some(b)) = (pin_graph_pos(&doc.bp, w.from), pin_graph_pos(&doc.bp, w.to)) else { continue };
                let (pa, pb) = (view.to_screen(a), view.to_screen(b));
                if !Rect::from_two_pos(pa, pb).expand(200.0).intersects(rect) {
                    continue;
                }
                let is_exec = matches!(w.from.kind, PinKind::ExecOut(_));
                let hot = doc.selected.contains(&w.from.node) && doc.selected.contains(&w.to.node);
                if is_exec {
                    bezier(&painter, pa, pb, doc.zoom, if hot { SELECTED } else { WHITE_WIRE }, (2.6 * doc.zoom).max(1.4));
                } else {
                    let ty = pin_ty(&doc.bp, w.from).unwrap_or(Ty::Any);
                    bezier(&painter, pa, pb, doc.zoom, if hot { SELECTED } else { ty_color(ty) }, (2.0 * doc.zoom).max(1.2));
                }
            }
            // the wire being pulled
            if let Some(Drag::Wire { from }) = &drag {
                if let (Some(g), Some(p)) = (pin_graph_pos(&doc.bp, *from), ptr) {
                    let a = view.to_screen(g);
                    let ty = pin_ty(&doc.bp, *from);
                    let col = ty.map(ty_color).unwrap_or(WHITE_WIRE);
                    let (s, e) = if matches!(from.kind, PinKind::ExecOut(_) | PinKind::DataOut) { (a, p) } else { (p, a) };
                    bezier(&painter, s, e, doc.zoom, col, (2.4 * doc.zoom).max(1.4));
                }
            }

            // ---- nodes
            let problem_nodes: Vec<u32> = doc.problems.iter().filter_map(|p| p.node).collect();
            let unreached = doc.bp.unreachable();
            let hover_pin = match hit {
                Hit::Pin(p) => Some(p),
                _ => None,
            };
            draw_event(&painter, &view, &doc.bp, doc.selected.contains(&EVENT), problem_nodes.contains(&EVENT));
            for n in &doc.bp.nodes {
                let r = view.rect(n.pos, bpm::node_size(n));
                if !r.intersects(rect) {
                    continue;
                }
                draw_node(&painter, &view, &doc.bp, n, doc.selected.contains(&n.id), problem_nodes.contains(&n.id), unreached.contains(&n.id), hover_pin, Marks { breakpoint: doc.breaks.contains(&n.id), hit: doc.hit == Some(n.id), ran: doc.live.as_ref().is_some_and(|l| l.ran.contains(&n.id)) });
            }
            // ---- live values: at the output pin of every data node the trace found a value for
            if let Some(live) = doc.live.as_ref().filter(|_| doc.zoom > 0.4) {
                for (id, v) in &live.values {
                    let Some(g) = pin_graph_pos(&doc.bp, Pin { node: *id, kind: PinKind::DataOut }) else { continue };
                    let at = view.to_screen(g) + Vec2::new(10.0 * doc.zoom, 0.0);
                    if !rect.contains(at) {
                        continue;
                    }
                    let text = live_text(v);
                    let galley = painter.layout_no_wrap(text, FontId::monospace(11.0 * doc.zoom), match v {
                        Json::Bool(true) => Color32::from_rgb(120, 220, 140),
                        Json::Bool(false) => Color32::from_rgb(235, 120, 110),
                        _ => Color32::from_rgb(250, 232, 150),
                    });
                    let pill = Rect::from_min_size(at + Vec2::new(0.0, -galley.size().y / 2.0 - 2.0), galley.size() + Vec2::new(10.0, 4.0));
                    painter.rect_filled(pill, CornerRadius::same(3), Color32::from_rgba_unmultiplied(12, 14, 18, 225));
                    painter.rect_stroke(pill, CornerRadius::same(3), Stroke::new(1.0, Color32::from_rgb(70, 110, 160)), StrokeKind::Inside);
                    painter.galley(pill.min + Vec2::new(5.0, 2.0), galley, TEXT);
                }
            }
            // hovering a node with a problem shows what it is
            if let (Some(p), Hit::Node(id) | Hit::Param(id, _) | Hit::Default(id, _)) = (ptr, hit) {
                let tips: Vec<&str> = doc.problems.iter().filter(|x| x.node == Some(id)).map(|x| x.message.as_str()).collect();
                if !tips.is_empty() && rect.contains(p) {
                    egui::Area::new(Id::new("bp-tip")).fixed_pos(p + Vec2::new(14.0, 16.0)).order(egui::Order::Tooltip).interactable(false).show(&ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            for t in tips {
                                ui.label(RichText::new(t).color(ERR).size(12.5));
                            }
                        });
                    });
                }
            }
        }

        self.drag = drag;
        self.selected_comment = selected_comment;
        // ---- popups: the menu of actions, the little editor for a value
        if let Some(m) = menu {
            menu = self.menu_ui(&ctx, studio, m);
        }
        self.menu = menu;
        if let Some(e) = editing {
            editing = self.edit_ui(&ctx, e);
        }
        self.editing = editing;
    }

    /// The right click menu. Returns it again while it stays open.
    fn menu_ui(&mut self, ctx: &egui::Context, studio: &mut Studio, mut m: Menu) -> Option<Menu> {
        let Some(name) = self.current.clone() else { return None };
        let mut close = false;
        let mut chosen: Option<&'static str> = None;
        let mut action: Option<String> = None;
        let ptr_pressed = ctx.input(|i| i.pointer.any_pressed());
        let ptr_pos = ctx.input(|i| i.pointer.interact_pos());
        let esc = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        let mut area_rect = Rect::NOTHING;
        let resources = self.resources.clone();
        let components = studio.components_available();
        let _ = &components;
        let target = m.target;
        let doc_has = self.docs.contains_key(&name);
        if !doc_has {
            return None;
        }
        let comp_filter = m.from;
        let (node_target_ok, filter_specs): (bool, Vec<&'static bpm::Spec>) = (true, filtered_specs(&self.docs[&name].bp, comp_filter));
        let _ = node_target_ok;
        let resp = egui::Area::new(Id::new("bp-menu")).fixed_pos(m.screen).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::NONE.fill(Color32::from_rgb(36, 36, 36)).stroke(Stroke::new(1.0, Color32::from_rgb(12, 12, 12))).inner_margin(egui::Margin::same(6)).shadow(egui::epaint::Shadow { offset: [2, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(140) }).show(ui, |ui| {
                ui.visuals_mut().override_text_color = Some(TEXT);
                match target {
                    Some(MenuTarget::Node(id)) => {
                        ui.set_width(190.0);
                        ui.label(RichText::new("Node Actions").size(11.0).color(TEXT_DIM));
                        let is_statement = self.docs[&name].bp.node(id).is_some_and(|n| n.spec().exec_in);
                        let has_break = self.docs[&name].breaks.contains(&id);
                        if is_statement && ui.selectable_label(false, if has_break { "Remove Breakpoint (F9)" } else { "Add Breakpoint (F9)" }).clicked() {
                            action = Some(format!("breakpoint:{id}"));
                            close = true;
                        }
                        for (label, id2) in [("Delete", "delete"), ("Duplicate", "duplicate"), ("Break All Pin Links", "break_all"), ("Create Comment from Selection", "comment")] {
                            if ui.selectable_label(false, label).clicked() {
                                action = Some(format!("{id2}:{id}"));
                                close = true;
                            }
                        }
                    }
                    Some(MenuTarget::Pin(pin)) => {
                        ui.set_width(190.0);
                        ui.label(RichText::new("Pin Actions").size(11.0).color(TEXT_DIM));
                        if ui.selectable_label(false, "Break Link(s)").clicked() {
                            action = Some(format!("break_pin:{}:{:?}", pin.node, pin.kind));
                            close = true;
                        }
                        if matches!(pin.kind, PinKind::DataIn(_)) && self.docs[&name].bp.node(pin.node).map(|n| !matches!(n.arity(), Arity::Fixed)).unwrap_or(false) && ui.selectable_label(false, "Remove Pin").clicked() {
                            action = Some(format!("remove_pin:{}:{:?}", pin.node, pin.kind));
                            close = true;
                        }
                    }
                    Some(MenuTarget::Comment(i)) => {
                        ui.set_width(190.0);
                        ui.label(RichText::new("Comment Actions").size(11.0).color(TEXT_DIM));
                        if ui.selectable_label(false, "Delete Comment").clicked() {
                            action = Some(format!("delete_comment:{i}"));
                            close = true;
                        }
                        ui.horizontal(|ui| {
                            for (k, (r, g, b)) in COMMENT_COLORS.iter().enumerate() {
                                let (rr, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                                ui.painter().rect_filled(rr, CornerRadius::same(3), Color32::from_rgb(*r, *g, *b));
                                if resp.clicked() {
                                    action = Some(format!("comment_color:{i}:{k}"));
                                    close = true;
                                }
                            }
                        });
                    }
                    None => {
                        ui.set_width(320.0);
                        ui.label(RichText::new(if m.from.is_some() { "Actions that fit this pin" } else { "All Actions for this Blueprint" }).size(12.0).color(TEXT_DIM));
                        let te = ui.add(TextEdit::singleline(&mut m.search).hint_text("Search").desired_width(f32::INFINITY));
                        if m.fresh {
                            te.request_focus();
                            m.fresh = false;
                        }
                        ui.add_space(4.0);
                        let q = m.search.to_lowercase();
                        let mut rows: Vec<(&'static bpm::Spec, String)> = Vec::new();
                        let mut extra: Vec<(String, String)> = Vec::new();
                        for sp in &filter_specs {
                            if q.is_empty() || sp.title.to_lowercase().contains(&q) || sp.keywords.contains(&q) || sp.cat.title().to_lowercase().contains(&q) {
                                rows.push((sp, sp.cat.title().to_string()));
                            }
                        }
                        if m.from.is_none() || matches!(m.from.map(|p| p.kind), Some(PinKind::DataIn(_))) {
                            for r in &resources {
                                if q.is_empty() || format!("get {r}").to_lowercase().contains(&q) || r.to_lowercase().contains(&q) {
                                    extra.push((format!("Get {r}"), format!("res:{r}")));
                                }
                            }
                        }
                        let n_rows = rows.len() + extra.len();
                        if m.selected >= n_rows {
                            m.selected = n_rows.saturating_sub(1);
                        }
                        ctx.input_mut(|i| {
                            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                                m.selected = (m.selected + 1) % n_rows.max(1);
                            }
                            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                                m.selected = (m.selected + n_rows.max(1) - 1) % n_rows.max(1);
                            }
                        });
                        let enter = te.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter));
                        egui::ScrollArea::vertical().max_height(340.0).id_salt("bp-menu-list").show(ui, |ui| {
                            let mut last_cat = "";
                            let mut idx = 0;
                            for (sp, cat) in &rows {
                                if q.is_empty() && last_cat != cat {
                                    last_cat = cat;
                                    ui.add_space(3.0);
                                    ui.label(RichText::new(cat.as_str()).size(11.5).color(header_color(sp.cat).gamma_multiply(1.5)));
                                }
                                let sel = idx == m.selected;
                                let r = ui.selectable_label(sel, RichText::new(format!("   {}", sp.title)).size(13.0));
                                if r.clicked() || (enter && sel) {
                                    chosen = Some(sp.op);
                                }
                                if sel && ctx.input(|i| i.key_pressed(Key::ArrowDown) || i.key_pressed(Key::ArrowUp)) {
                                    r.scroll_to_me(Some(Align::Center));
                                }
                                idx += 1;
                            }
                            for (title, id) in &extra {
                                let sel = idx == m.selected;
                                let r = ui.selectable_label(sel, RichText::new(format!("   {title}")).size(13.0));
                                if r.clicked() || (enter && sel) {
                                    action = Some(id.clone());
                                    close = true;
                                }
                                idx += 1;
                            }
                            if n_rows == 0 {
                                ui.label(RichText::new("No matching actions").color(TEXT_DIM));
                            }
                        });
                    }
                }
            });
        });
        area_rect = area_rect.union(resp.response.rect);
        if esc {
            close = true;
        }
        if ptr_pressed && !m.fresh {
            if let Some(p) = ptr_pos {
                if !area_rect.contains(p) {
                    close = true;
                }
            }
        }
        // act
        let graph = m.graph;
        let from = m.from;
        let pending_op = chosen.map(|op| (op, graph, from));
        let mut res_name: Option<String> = None;
        if let Some(a) = &action {
            if let Some(r) = a.strip_prefix("res:") {
                res_name = Some(r.to_string());
            }
        }
        if let Some(doc) = self.docs.get_mut(&name) {
            if let Some((op, g, from)) = pending_op {
                doc.snapshot();
                let id = doc.bp.add_node(op, g);
                connect_new(doc, id, from);
                doc.selected = vec![id];
                close = true;
            } else if let Some(r) = res_name {
                doc.snapshot();
                let id = doc.bp.add_node("res", graph);
                if let Some(n) = doc.bp.node_mut(id) {
                    n.params = vec![r];
                }
                connect_new(doc, id, from);
                doc.selected = vec![id];
            } else if let Some(a) = action {
                run_menu_action(doc, &a);
            }
        }
        if close {
            None
        } else {
            Some(m)
        }
    }

    /// The little text box that opens over a value or a property. Returns it while it stays open.
    fn edit_ui(&mut self, ctx: &egui::Context, mut e: Editing) -> Option<Editing> {
        let Some(name) = self.current.clone() else { return None };
        let mut done = false;
        let mut cancel = false;
        egui::Area::new(Id::new("bp-edit")).fixed_pos(e.at).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::NONE.fill(Color32::from_rgb(12, 12, 12)).stroke(Stroke::new(1.0, SELECTED)).inner_margin(egui::Margin::symmetric(4, 2)).show(ui, |ui| {
                let te = ui.add(TextEdit::singleline(&mut e.text).desired_width(e.width).font(FontId::proportional(13.0)).text_color(TEXT).frame(egui::Frame::NONE));
                e.age = e.age.saturating_add(1);
                if e.age <= 3 {
                    te.request_focus();
                }
                if e.fresh {
                    // select all, like the other editors
                    if let Some(mut s) = TextEdit::load_state(ui.ctx(), te.id) {
                        s.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(e.text.chars().count()))));
                        TextEdit::store_state(ui.ctx(), te.id, s);
                    }
                    e.fresh = false;
                }
                if te.lost_focus() && e.age > 3 {
                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        cancel = true;
                    } else {
                        done = true;
                    }
                }
            });
        });
        if cancel {
            return None;
        }
        if done {
            if let Some(doc) = self.docs.get_mut(&name) {
                doc.snapshot();
                apply_edit(doc, &e);
            }
            return None;
        }
        Some(e)
    }

    // ------------------------------------------------------------------------------------------------ side panels

    /// "My Blueprint": the graph systems of the world, and its resources.
    pub fn my_blueprint(&mut self, ui: &mut Ui, studio: &mut Studio) {
        self.refresh(studio);
        let mut open: Option<String> = None;
        let mut add_new = false;
        let mut add_res: Option<String> = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("GRAPHS").size(11.0).color(TEXT_DIM));
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("+ System").on_hover_text("A new graph system").clicked() {
                    add_new = true;
                }
            });
        });
        let mut names: Vec<(String, bool, bool)> = self.systems.iter().map(|(n, e)| (n.clone(), *e, false)).collect();
        for n in self.docs.keys() {
            if !names.iter().any(|(m, _, _)| m == n) {
                names.push((n.clone(), false, true));
            }
        }
        egui::ScrollArea::vertical().id_salt("bp-systems").max_height(ui.available_height() * 0.6).show(ui, |ui| {
            for (n, err, fresh) in names {
                let dirty = self.docs.get(&n).map(|d| d.dirty()).unwrap_or(false);
                let label = format!("{}{}{}", if fresh { "+ " } else { "" }, n, if dirty { " *" } else { "" });
                let r = ui.selectable_label(self.current.as_deref() == Some(n.as_str()), RichText::new(label).color(if err { ERR } else { TEXT }));
                if r.clicked() {
                    open = Some(n);
                }
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new("RESOURCES").size(11.0).color(TEXT_DIM));
        egui::ScrollArea::vertical().id_salt("bp-resources").show(ui, |ui| {
            for r in self.resources.clone() {
                let resp = ui.selectable_label(false, RichText::new(format!("   {r}")).color(ty_color(Ty::Num)));
                if resp.clicked() {
                    add_res = Some(r);
                }
                resp.on_hover_text("Click to add a Get Resource node");
            }
            if self.resources.is_empty() {
                ui.label(RichText::new("   (none yet)").color(TEXT_DIM));
            }
        });
        if add_new {
            self.new_system(studio);
        }
        if let Some(n) = open {
            self.open(studio, &n);
        }
        if let Some(r) = add_res {
            let canvas = self.canvas.unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)));
            if let Some(doc) = self.doc() {
                doc.snapshot();
                let view = View { origin: canvas.min, pan: doc.pan, zoom: doc.zoom };
                let g = view.to_graph(canvas.center());
                let id = doc.bp.add_node("res", g);
                if let Some(n) = doc.bp.node_mut(id) {
                    n.params = vec![r];
                }
                doc.selected = vec![id];
            }
        }
    }

    /// "Details": the selected node's properties.
    pub fn details(&mut self, ui: &mut Ui, studio: &mut Studio) {
        let comps = studio.components_available();
        let Some(name) = self.current.clone() else {
            ui.label(RichText::new("Nothing is open").color(TEXT_DIM));
            return;
        };
        let Some(doc) = self.docs.get_mut(&name) else { return };
        let sel = if doc.selected.len() == 1 { Some(doc.selected[0]) } else { None };
        let mut snapshot_needed = false;
        let before = doc.bp.clone();
        match sel {
            Some(EVENT) | None => {
                let meta = &mut doc.bp.meta;
                ui.label(RichText::new("System").size(12.0).color(TEXT_DIM));
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.add_enabled(false, TextEdit::singleline(&mut meta.name).desired_width(f32::INFINITY));
                });
                ui.label(RichText::new("Description").size(12.0).color(TEXT_DIM));
                ui.add(TextEdit::multiline(&mut meta.doc).desired_rows(2).desired_width(f32::INFINITY));
                ui.checkbox(&mut meta.enabled, "Enabled");
                let mut contact = meta.contact.is_some();
                if ui.checkbox(&mut contact, "Runs for touching pairs (on contact)").changed() {
                    meta.contact = if contact { Some((vec![], vec![], "all".to_string())) } else { None };
                }
                let list = |ui: &mut Ui, label: &str, items: &mut Vec<String>| {
                    let mut text = items.join(", ");
                    ui.label(RichText::new(label).size(12.0).color(TEXT_DIM));
                    if ui.add(TextEdit::singleline(&mut text).desired_width(f32::INFINITY).hint_text("Component, Component")).changed() {
                        *items = text.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                    }
                };
                match &mut meta.contact {
                    Some((a, b, t)) => {
                        list(ui, "First entity has (a)", a);
                        list(ui, "Second entity has (b)", b);
                        ui.horizontal(|ui| {
                            ui.label("Triggers");
                            for k in ["all", "only", "solid"] {
                                if ui.selectable_label(t == k, k).clicked() {
                                    *t = k.to_string();
                                }
                            }
                        });
                    }
                    None => list(ui, "For each entity that has", &mut meta.each),
                }
                list(ui, "and does not have", &mut meta.without);
                ui.horizontal(|ui| {
                    ui.label("Order");
                    let mut o = meta.order.unwrap_or(100);
                    if ui.add(egui::DragValue::new(&mut o)).changed() {
                        meta.order = Some(o);
                    }
                    if meta.order.is_some() && ui.small_button("default").clicked() {
                        meta.order = None;
                    }
                });
                if !comps.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(format!("Components in the world: {}", comps.join(", "))).size(11.0).color(TEXT_DIM));
                }
                if !doc.problems.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Problems").size(12.0).color(ERR));
                    for p in &doc.problems {
                        ui.label(RichText::new(&p.message).size(12.0).color(ERR));
                    }
                }
            }
            Some(id) => {
                let (title, sp_params, arity) = match doc.bp.node(id) {
                    Some(n) => (n.title(), n.spec().params.to_vec(), n.arity()),
                    None => return,
                };
                ui.label(RichText::new(title).size(15.0).strong());
                ui.add_space(4.0);
                for (i, (label, _)) in sp_params.iter().enumerate() {
                    ui.label(RichText::new(*label).size(12.0).color(TEXT_DIM));
                    let n = doc.bp.node_mut(id).unwrap();
                    if n.op.starts_with("raw") {
                        let mut text = n.raw.as_ref().and_then(|j| j.to_string_pretty().ok()).unwrap_or_default();
                        if ui.add(TextEdit::multiline(&mut text).code_editor().desired_rows(4).desired_width(f32::INFINITY)).changed() {
                            if let Ok(j) = lunaris_json::parse(&text) {
                                n.params = vec![j.to_string_compact().unwrap_or_default()];
                                n.raw = Some(j);
                            }
                        }
                    } else if let Some(p) = n.params.get_mut(i) {
                        ui.add(TextEdit::singleline(p).desired_width(f32::INFINITY));
                    }
                }
                if matches!(arity, Arity::Named | Arity::Nary { .. }) {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Pins").size(12.0).color(TEXT_DIM));
                    let count = doc.bp.node(id).map(|n| n.inputs()).unwrap_or(0);
                    let mut remove: Option<usize> = None;
                    for i in 0..count {
                        ui.horizontal(|ui| {
                            if matches!(arity, Arity::Named) {
                                if let Some(n) = doc.bp.node_mut(id) {
                                    if let Some(name) = n.named.get_mut(i) {
                                        ui.add(TextEdit::singleline(name).desired_width(160.0));
                                    }
                                }
                            } else {
                                ui.label(((b'A' + (i % 26) as u8) as char).to_string());
                            }
                            if ui.small_button("x").on_hover_text("Remove this pin").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        snapshot_needed = true;
                        doc.bp.remove_input(id, i);
                    }
                    if ui.button("+ Add pin").clicked() {
                        snapshot_needed = true;
                        if let Some(n) = doc.bp.node_mut(id) {
                            n.add_input(if n.op == "spawn_prefab" { "at.x" } else if n.op == "add_comp" { "field" } else { "Transform.pos" });
                        }
                    }
                }
                let ids = doc.bp.node(id).map(|n| (0..n.inputs()).map(|i| (i, n.input_label(i), n.input_ty(i), doc.bp.wire_into(Pin { node: id, kind: PinKind::DataIn(i) }).is_none())).collect::<Vec<_>>()).unwrap_or_default();
                if !ids.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Default values").size(12.0).color(TEXT_DIM));
                }
                for (i, label, ty, free) in ids {
                    if !free {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.label(label);
                        let Some(n) = doc.bp.node_mut(id) else { return };
                        if i >= n.defaults.len() {
                            return;
                        }
                        if ty == Ty::Bool {
                            let mut b = matches!(n.defaults[i], Json::Bool(true));
                            if ui.checkbox(&mut b, "").changed() {
                                n.defaults[i] = Json::Bool(b);
                            }
                        } else {
                            let mut t = match &n.defaults[i] {
                                Json::Str(s) => s.clone(),
                                other => literal_text(other),
                            };
                            if ui.add(TextEdit::singleline(&mut t).desired_width(120.0)).changed() {
                                n.defaults[i] = parse_literal(&t, ty, &n.defaults[i]);
                            }
                        }
                    });
                }
            }
        }
        if doc.bp != before {
            if snapshot_needed {
                // the change was already made: the snapshot is the state from before it
                doc.undo.push(before);
                doc.redo.clear();
            } else if doc.undo.last().is_none_or(|u| *u != before) {
                doc.undo.push(before);
                if doc.undo.len() > 100 {
                    doc.undo.remove(0);
                }
                doc.redo.clear();
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------- helpers

fn select(doc: &mut Doc, id: u32, add: bool) {
    if add {
        if let Some(p) = doc.selected.iter().position(|s| *s == id) {
            doc.selected.remove(p);
        } else {
            doc.selected.push(id);
        }
    } else {
        doc.selected = vec![id];
    }
}

fn start_node_drag(doc: &Doc, view: &View, p: Pos2, drag: &mut Option<Drag>) {
    let ids = doc.selected.clone();
    let orig: Vec<(u32, [f32; 2])> = ids.iter().filter_map(|id| if *id == EVENT { Some((EVENT, doc.bp.meta.event_pos)) } else { doc.bp.node(*id).map(|n| (n.id, n.pos)) }).collect();
    *drag = Some(Drag::Nodes { ids, comment_carry: vec![], start: view.to_graph(p), orig, orig_comments: vec![], moved: false });
}

/// Show everything (`all`), or what you would look at when opening it: the start, big enough to read.
fn fit(doc: &mut Doc, rect: Rect, all: bool) {
    let (lo, hi) = doc.bp.bounds();
    let (w, h) = ((hi[0] - lo[0]).max(1.0), (hi[1] - lo[1]).max(1.0));
    let k = ((rect.width() - 80.0) / w).min((rect.height() - 80.0) / h).clamp(if all { 0.2 } else { 0.7 }, 1.0);
    doc.zoom = k;
    let fits = w * k <= rect.width() - 80.0 && h * k <= rect.height() - 80.0;
    doc.pan = if fits { Vec2::new((rect.width() - w * k) / 2.0 - lo[0] * k, ((rect.height() - h * k) / 2.0).max(30.0) - lo[1] * k) } else { Vec2::new(40.0 - lo[0] * k, 50.0 - lo[1] * k) };
}

fn focus_selection(doc: &mut Doc, rect: Rect) {
    let mut lo = [f32::MAX, f32::MAX];
    let mut hi = [f32::MIN, f32::MIN];
    for id in &doc.selected {
        if let Some((p, s)) = node_rect(&doc.bp, *id) {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0] + s[0]), hi[1].max(p[1] + s[1])];
        }
    }
    if lo[0] > hi[0] {
        return;
    }
    let (w, h) = ((hi[0] - lo[0]).max(1.0), (hi[1] - lo[1]).max(1.0));
    let k = ((rect.width() - 160.0) / w).min((rect.height() - 160.0) / h).clamp(0.4, 1.2);
    doc.zoom = k;
    doc.pan = Vec2::new((rect.width() - w * k) / 2.0 - lo[0] * k, (rect.height() - h * k) / 2.0 - lo[1] * k);
}

fn add_comment_around(doc: &mut Doc) {
    let mut lo = [f32::MAX, f32::MAX];
    let mut hi = [f32::MIN, f32::MIN];
    for id in &doc.selected {
        if let Some((p, s)) = node_rect(&doc.bp, *id) {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0] + s[0]), hi[1].max(p[1] + s[1])];
        }
    }
    if lo[0] > hi[0] {
        return;
    }
    doc.bp.comments.push(Comment { pos: [lo[0] - 16.0, lo[1] - 44.0], size: [hi[0] - lo[0] + 32.0, hi[1] - lo[1] + 60.0], title: "Comment".to_string(), color: 1 });
}

/// The actions that make sense when a wire is dragged from `from` (all of them when there is none).
fn filtered_specs(bp: &Bp, from: Option<Pin>) -> Vec<&'static bpm::Spec> {
    let mut v: Vec<&'static bpm::Spec> = bpm::SPECS.iter().filter(|s| !s.op.starts_with("raw") || from.is_none()).collect();
    if let Some(p) = from {
        v.retain(|s| match p.kind {
            PinKind::ExecOut(_) => s.exec_in,
            PinKind::ExecIn => !s.exec_out.is_empty(),
            PinKind::DataOut => !s.inputs.is_empty() || !matches!(s.arity, Arity::Fixed),
            PinKind::DataIn(_) => s.out.is_some(),
        });
        let _ = bp;
    }
    v
}

fn connect_new(doc: &mut Doc, id: u32, from: Option<Pin>) {
    let Some(p) = from else { return };
    let pin = match p.kind {
        PinKind::ExecOut(_) => Pin { node: id, kind: PinKind::ExecIn },
        PinKind::ExecIn => Pin { node: id, kind: PinKind::ExecOut(doc.bp.node(id).map(|n| if n.op == "if" { 2 } else { 0 }).unwrap_or(0)) },
        PinKind::DataOut => Pin { node: id, kind: PinKind::DataIn(0) },
        PinKind::DataIn(_) => Pin { node: id, kind: PinKind::DataOut },
    };
    doc.bp.connect(p, pin);
}

/// Put a breakpoint on a statement node, or take it off (nodes that are not statements cannot have one).
fn toggle_breakpoint(doc: &mut Doc, id: u32) {
    if !doc.bp.node(id).is_some_and(|n| n.spec().exec_in) {
        return;
    }
    match doc.breaks.iter().position(|b| *b == id) {
        Some(i) => {
            doc.breaks.remove(i);
        }
        None => doc.breaks.push(id),
    }
}

fn run_menu_action(doc: &mut Doc, a: &str) {
    let mut it = a.split(':');
    let verb = it.next().unwrap_or("");
    if verb == "breakpoint" {
        // not an edit of the graph: no undo step
        let id: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        toggle_breakpoint(doc, id);
        return;
    }
    doc.snapshot();
    match verb {
        "delete" => {
            let id: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let ids = if doc.selected.contains(&id) { doc.selected.clone() } else { vec![id] };
            for i in ids {
                doc.bp.remove_node(i);
            }
            doc.selected.clear();
        }
        "duplicate" => {
            let id: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let ids = if doc.selected.contains(&id) { doc.selected.clone() } else { vec![id] };
            doc.selected = doc.bp.duplicate(&ids, [30.0, 30.0]);
        }
        "break_all" => {
            let id: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            doc.bp.wires.retain(|w| w.from.node != id && w.to.node != id);
        }
        "comment" => {
            if doc.selected.is_empty() {
                let id: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                doc.selected = vec![id];
            }
            add_comment_around(doc);
        }
        "break_pin" | "remove_pin" => {
            let id: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let kind = it.next().unwrap_or("");
            let pin = parse_pin(id, kind);
            if let Some(pin) = pin {
                if verb == "break_pin" {
                    doc.bp.disconnect(pin);
                } else if let PinKind::DataIn(i) = pin.kind {
                    doc.bp.remove_input(id, i);
                }
            }
        }
        "delete_comment" => {
            let i: usize = it.next().and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
            if i < doc.bp.comments.len() {
                doc.bp.comments.remove(i);
            }
        }
        "comment_color" => {
            let i: usize = it.next().and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
            let k: usize = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            if let Some(c) = doc.bp.comments.get_mut(i) {
                c.color = k;
            }
        }
        _ => {}
    }
}

fn parse_pin(node: u32, kind: &str) -> Option<Pin> {
    let inner = |s: &str| -> Option<usize> { s.split('(').nth(1)?.trim_end_matches(')').parse().ok() };
    let kind = if kind.starts_with("ExecIn") {
        PinKind::ExecIn
    } else if kind.starts_with("ExecOut") {
        PinKind::ExecOut(inner(kind)?)
    } else if kind.starts_with("DataIn") {
        PinKind::DataIn(inner(kind)?)
    } else if kind.starts_with("DataOut") {
        PinKind::DataOut
    } else {
        return None;
    };
    Some(Pin { node, kind })
}

fn apply_edit(doc: &mut Doc, e: &Editing) {
    match &e.target {
        EditTarget::Param(id, i) => {
            if let Some(n) = doc.bp.node_mut(*id) {
                if let Some(p) = n.params.get_mut(*i) {
                    *p = e.text.trim().to_string();
                }
            }
        }
        EditTarget::Raw(id) => {
            if let (Some(n), Ok(j)) = (doc.bp.node_mut(*id), lunaris_json::parse(&e.text)) {
                n.params = vec![j.to_string_compact().unwrap_or_default()];
                n.raw = Some(j);
            }
        }
        EditTarget::Default(id, i) => {
            if let Some(n) = doc.bp.node_mut(*id) {
                let ty = n.input_ty(*i);
                if let Some(d) = n.defaults.get_mut(*i) {
                    *d = parse_literal(&e.text, ty, d);
                }
            }
        }
        EditTarget::PinName(id, i) => {
            if let Some(n) = doc.bp.node_mut(*id) {
                if let Some(p) = n.named.get_mut(*i) {
                    *p = e.text.trim().to_string();
                }
            }
        }
        EditTarget::Comment(i) => {
            if let Some(c) = doc.bp.comments.get_mut(*i) {
                c.title = e.text.clone();
            }
        }
        EditTarget::EventName => doc.bp.meta.doc = e.text.clone(),
    }
}

fn draw_event(painter: &egui::Painter, v: &View, bp: &Bp, selected: bool, problem: bool) {
    let z = v.zoom;
    let size = bpm::event_size(&bp.meta);
    let r = v.rect(bp.meta.event_pos, size);
    let round = CornerRadius::same((6.0 * z).clamp(2.0, 10.0) as u8);
    painter.rect_filled(r.translate(Vec2::new(2.0, 3.0) * z), round, Color32::from_black_alpha(90));
    painter.rect_filled(r, round, BODY);
    let head = Rect::from_min_size(r.min, Vec2::new(r.width(), HEADER * z));
    painter.rect_filled(head, CornerRadius { nw: round.nw, ne: round.ne, sw: 0, se: 0 }, header_color(Cat::Event));
    painter.rect_filled(Rect::from_min_size(head.min, Vec2::new(head.width(), head.height() * 0.45)), CornerRadius { nw: round.nw, ne: round.ne, sw: 0, se: 0 }, Color32::from_white_alpha(22));
    painter.rect_stroke(r, round, Stroke::new(if selected || problem { 2.0 } else { 1.0 }, if problem { ERR } else if selected { SELECTED } else { Color32::from_black_alpha(220) }), StrokeKind::Inside);
    if z > 0.3 {
        let title = if bp.meta.contact.is_some() { "On Contact" } else { "Every Tick" };
        painter.text(r.min + Vec2::new(12.0 * z, HEADER * z / 2.0), Align2::LEFT_CENTER, title, bold(13.5 * z), Color32::WHITE);
        if z > 0.45 {
            for (i, line) in bpm::event_lines(&bp.meta).iter().enumerate() {
                painter.text(r.min + Vec2::new(12.0 * z, (HEADER + 12.0 + i as f32 * ROW) * z), Align2::LEFT_CENTER, line, FontId::proportional(12.0 * z), if i == 0 { TEXT } else { TEXT_DIM });
            }
        }
    }
    let connected = bp.wires.iter().any(|w| w.from.node == EVENT);
    if let Some(g) = pin_graph_pos(bp, Pin { node: EVENT, kind: PinKind::ExecOut(0) }) {
        exec_pin(painter, v.to_screen(g), z, connected, false);
    }
}

/// What a node shows about debugging: a breakpoint dot, that the game stopped at it, that it ran (or runs next) for the selected entity.
#[derive(Clone, Copy, Default)]
struct Marks {
    breakpoint: bool,
    hit: bool,
    ran: bool,
}

#[allow(clippy::too_many_arguments)]
fn draw_node(painter: &egui::Painter, v: &View, bp: &Bp, n: &Node, selected: bool, problem: bool, unreached: bool, hover: Option<Pin>, marks: Marks) {
    let z = v.zoom;
    let size = bpm::node_size(n);
    let r = v.rect(n.pos, size);
    let sp = n.spec();
    let round = CornerRadius::same((6.0 * z).clamp(2.0, 10.0) as u8);
    painter.rect_filled(r.translate(Vec2::new(2.0, 3.0) * z), round, Color32::from_black_alpha(90));
    painter.rect_filled(r, round, if unreached { Color32::from_rgba_premultiplied(14, 14, 15, 190) } else { BODY });
    let hc = header_color(sp.cat);
    let head = Rect::from_min_size(r.min, Vec2::new(r.width(), HEADER * z));
    let hround = CornerRadius { nw: round.nw, ne: round.ne, sw: 0, se: 0 };
    painter.rect_filled(head, hround, if unreached { alpha(hc, 150) } else { hc });
    painter.rect_filled(Rect::from_min_size(head.min, Vec2::new(head.width(), head.height() * 0.45)), hround, Color32::from_white_alpha(22));
    painter.rect_stroke(r, round, Stroke::new(if selected || problem { 2.0 } else { 1.0 }, if problem { ERR } else if selected { SELECTED } else if marks.ran { Color32::from_rgb(70, 160, 95) } else { Color32::from_black_alpha(220) }), StrokeKind::Inside);
    if marks.hit {
        // the game stopped here: the statement is about to run
        painter.rect_stroke(r.expand(3.0 * z), round, Stroke::new(2.5, Color32::from_rgb(255, 90, 70)), StrokeKind::Outside);
        if z > 0.4 {
            painter.text(r.right_top() + Vec2::new(-4.0 * z, -4.0 * z), Align2::RIGHT_BOTTOM, "STOPPED HERE", bold(10.5 * z), Color32::from_rgb(255, 120, 100));
        }
    }
    if marks.breakpoint {
        let c = r.min + Vec2::new(-9.0 * z, HEADER * z / 2.0);
        painter.circle_filled(c, 6.0 * z, Color32::from_rgb(220, 50, 40));
        painter.circle_stroke(c, 6.0 * z, Stroke::new(1.2, Color32::from_rgb(255, 170, 160)));
    }
    if z > 0.3 {
        painter.text(r.min + Vec2::new(12.0 * z, HEADER * z / 2.0), Align2::LEFT_CENTER, sp.title, bold(13.0 * z), Color32::WHITE);
    }
    let detail = z > 0.45;
    // pins on the left
    let mut left_row = 0usize;
    if sp.exec_in {
        let pin = Pin { node: n.id, kind: PinKind::ExecIn };
        let o = bpm::pin_offset(n, PinKind::ExecIn);
        exec_pin(painter, v.to_screen([n.pos[0] + o[0], n.pos[1] + o[1]]), z, bp.is_connected(pin), hover == Some(pin));
        left_row += 1;
    }
    let _ = left_row;
    for i in 0..n.inputs() {
        let pin = Pin { node: n.id, kind: PinKind::DataIn(i) };
        let o = bpm::pin_offset(n, PinKind::DataIn(i));
        let c = v.to_screen([n.pos[0] + o[0], n.pos[1] + o[1]]);
        let connected = bp.wire_into(pin).is_some();
        data_pin(painter, c, z, n.input_ty(i), connected, hover == Some(pin));
        if detail {
            painter.text(c + Vec2::new(13.0 * z, 0.0), Align2::LEFT_CENTER, n.input_label(i), FontId::proportional(12.0 * z), TEXT);
            if !connected {
                let bx = literal_box(n, i, o);
                let br = v.rect([n.pos[0] + bx[0], n.pos[1] + bx[1]], [bx[2], bx[3]]);
                if n.input_ty(i) == Ty::Bool {
                    let on = matches!(n.defaults.get(i), Some(Json::Bool(true)));
                    let cb = Rect::from_min_size(br.min, Vec2::splat(15.0 * z));
                    painter.rect_filled(cb, CornerRadius::same((2.0 * z) as u8), Color32::from_rgb(8, 8, 8));
                    painter.rect_stroke(cb, CornerRadius::same((2.0 * z) as u8), Stroke::new(1.0, Color32::from_rgb(70, 70, 70)), StrokeKind::Inside);
                    if on {
                        let m = cb.center();
                        painter.line_segment([m + Vec2::new(-4.0, 0.0) * z, m + Vec2::new(-1.0, 3.0) * z], Stroke::new(1.8, Color32::from_rgb(235, 235, 235)));
                        painter.line_segment([m + Vec2::new(-1.0, 3.0) * z, m + Vec2::new(4.5, -3.5) * z], Stroke::new(1.8, Color32::from_rgb(235, 235, 235)));
                    }
                } else {
                    painter.rect_filled(br, CornerRadius::same((3.0 * z) as u8), Color32::from_rgb(8, 8, 8));
                    painter.rect_stroke(br, CornerRadius::same((3.0 * z) as u8), Stroke::new(1.0, Color32::from_rgb(58, 58, 58)), StrokeKind::Inside);
                    let t = n.defaults.get(i).map(literal_text).unwrap_or_default();
                    let t: String = if t.chars().count() > 7 { format!("{}..", t.chars().take(6).collect::<String>()) } else { t };
                    painter.text(br.center(), Align2::CENTER_CENTER, t, FontId::proportional(11.5 * z), Color32::from_rgb(225, 225, 225));
                }
            }
        }
    }
    // pins on the right
    for (k, label) in sp.exec_out.iter().enumerate() {
        let pin = Pin { node: n.id, kind: PinKind::ExecOut(k) };
        let o = bpm::pin_offset(n, PinKind::ExecOut(k));
        let c = v.to_screen([n.pos[0] + o[0], n.pos[1] + o[1]]);
        exec_pin(painter, c, z, bp.is_connected(pin), hover == Some(pin));
        if detail && !label.is_empty() {
            painter.text(c - Vec2::new(13.0 * z, 0.0), Align2::RIGHT_CENTER, *label, FontId::proportional(12.0 * z), TEXT);
        }
    }
    if let Some(ty) = sp.out {
        let pin = Pin { node: n.id, kind: PinKind::DataOut };
        let o = bpm::pin_offset(n, PinKind::DataOut);
        let c = v.to_screen([n.pos[0] + o[0], n.pos[1] + o[1]]);
        data_pin(painter, c, z, ty, bp.is_connected(pin), hover == Some(pin));
        if detail {
            painter.text(c - Vec2::new(13.0 * z, 0.0), Align2::RIGHT_CENTER, "Value", FontId::proportional(12.0 * z), TEXT_DIM);
        }
    }
    // properties
    if detail && !sp.params.is_empty() {
        let rows_left = usize::from(sp.exec_in) + n.inputs();
        let rows_right = sp.exec_out.len() + usize::from(sp.out.is_some());
        let pin_rows = rows_left.max(rows_right).max(1);
        let y0 = HEADER + 6.0 + pin_rows as f32 * ROW + 4.0;
        painter.line_segment([v.to_screen([n.pos[0] + 6.0, n.pos[1] + y0 - 2.0]), v.to_screen([n.pos[0] + size[0] - 6.0, n.pos[1] + y0 - 2.0])], Stroke::new(1.0, Color32::from_white_alpha(18)));
        for (i, (label, _)) in sp.params.iter().enumerate() {
            let y = y0 + i as f32 * ROW;
            let rr = v.rect([n.pos[0] + 8.0, n.pos[1] + y + 1.0], [size[0] - 16.0, ROW - 3.0]);
            painter.rect_filled(rr, CornerRadius::same((3.0 * z) as u8), Color32::from_rgb(10, 10, 10));
            painter.text(rr.min + Vec2::new(6.0 * z, rr.height() / 2.0), Align2::LEFT_CENTER, *label, FontId::proportional(11.5 * z), TEXT_DIM);
            let val = n.params.get(i).cloned().unwrap_or_default();
            let val = if val.is_empty() { "(none)".to_string() } else { val };
            painter.text(rr.right_center() - Vec2::new(6.0 * z, 0.0), Align2::RIGHT_CENTER, val, FontId::proportional(12.0 * z), TEXT);
        }
    }
    // "Add pin" under the inputs of nodes that can grow
    if detail && matches!(n.arity(), Arity::Nary { .. } | Arity::Named) {
        let add = add_pin_box(n);
        let ar = v.rect([n.pos[0] + add[0], n.pos[1] + add[1]], [add[2], add[3]]);
        painter.text(ar.min + Vec2::new(0.0, ar.height() / 2.0), Align2::LEFT_CENTER, "+ Add pin", FontId::proportional(11.5 * z), Color32::from_rgb(120, 170, 235));
    }
    if unreached && detail {
        painter.text(r.right_top() + Vec2::new(-8.0 * z, -10.0 * z), Align2::RIGHT_BOTTOM, "not connected", FontId::proportional(10.5 * z), Color32::from_rgb(200, 160, 70));
    }
    if problem {
        let c = r.right_top() + Vec2::new(-12.0 * z, 12.0 * z);
        painter.circle_filled(c, 8.0 * z, ERR);
        painter.text(c, Align2::CENTER_CENTER, "!", bold(11.0 * z), Color32::WHITE);
    }
}

/// `text` if it is at most `max_w` wide in `font`, else the longest start of it that fits with three dots after it, else nothing (when there is almost no room).
fn fit_text(p: &egui::Painter, text: &str, font: FontId, max_w: f32) -> String {
    if max_w <= 24.0 {
        return String::new();
    }
    let width = |s: &str| p.layout_no_wrap(s.to_string(), font.clone(), TEXT).size().x;
    if width(text) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let s: String = chars[..mid].iter().collect::<String>() + "...";
        if width(&s) <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "..."
}
