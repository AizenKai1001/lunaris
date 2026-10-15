//! The node view of a graph system (`docs/GRAPH.md`): a JSON document drawn the Unreal way. A red event node at the top says which entities the
//! system visits every tick, white execution wires run down through the statements, and coloured data wires (green numbers, red booleans,
//! magenta text) run from expression nodes into the pins of the statement that uses them. The JSON is the source of truth: the picture is
//! made from it after every change, and clicking a constant edits the number or text in the document.
use crate::theme;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2};
use lunaris_json::Json;
use std::collections::HashSet;

const GAP: f32 = 34.0;
const HDR: f32 = 22.0;
const ROW: f32 = 18.0;
const CW: f32 = 6.9;
const PADX: f32 = 12.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Key(String),
    Idx(usize),
}

pub type Path = Vec<Seg>;

fn key(p: &Path, k: &str) -> Path {
    let mut q = p.clone();
    q.push(Seg::Key(k.to_string()));
    q
}

fn idx(p: &Path, i: usize) -> Path {
    let mut q = p.clone();
    q.push(Seg::Idx(i));
    q
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Event,
    Stmt,
    Ctrl,
    Const,
    Get,
    Op,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Num,
    Bool,
    Str,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: Kind,
    pub ty: Ty,
    pub title: String,
    pub rows: Vec<String>,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub path: Path,
    pub ins: Vec<usize>,
    pub is_if: bool,
    pub has_in: bool,
    pub has_out: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WireKind {
    Exec,
    /// From a statement's `then` (0) or `else` (1) pin to the first statement of the branch.
    Branch(usize),
    /// An expression into pin `n` of a node.
    Data(usize, Ty),
}

#[derive(Clone, Copy, Debug)]
pub struct Wire {
    pub from: usize,
    pub to: usize,
    pub kind: WireKind,
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub wires: Vec<Wire>,
}

fn node_width(title: &str, rows: &[String]) -> f32 {
    let mut w = title.chars().count() as f32 * CW + PADX * 2.0;
    for r in rows {
        w = w.max(r.chars().count() as f32 * CW + PADX * 2.0 + 6.0);
    }
    w.max(92.0).ceil()
}

fn row_names(op: &str) -> Option<&'static [&'static str]> {
    Some(match op {
        "sub" | "div" | "mod" | "lt" | "le" | "gt" | "ge" | "eq" | "ne" => &["a", "b"],
        "clamp" => &["x", "lo", "hi"],
        "lerp" => &["a", "b", "t"],
        "select" => &["if", "then", "else"],
        _ => return None,
    })
}

fn is_bool_op(op: &str) -> bool {
    matches!(op, "lt" | "le" | "gt" | "ge" | "eq" | "ne" | "and" | "or" | "not")
}

struct Built {
    node: usize,
    w: f32,
    h: f32,
}

struct Layout {
    g: Graph,
}

impl Layout {
    fn shift(&mut self, from: usize, dx: f32, dy: f32) {
        for n in self.g.nodes.iter_mut().skip(from) {
            n.x += dx;
            n.y += dy;
        }
    }

    /// Move a node and everything that feeds it.
    fn shift_tree(&mut self, root: usize, dx: f32, dy: f32) {
        let mut seen = HashSet::new();
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            if !seen.insert(n) {
                continue;
            }
            self.g.nodes[n].x += dx;
            self.g.nodes[n].y += dy;
            stack.extend(self.g.nodes[n].ins.clone());
        }
    }

    /// Lay out an expression tree with its root's right edge at the returned width and its top at 0; its inputs are children on the left.
    fn expr(&mut self, e: &Json, path: &Path) -> Built {
        let leaf = |this: &mut Layout, text: String, ty: Ty| {
            let w = (text.chars().count() as f32 * CW + PADX * 2.0).max(46.0);
            this.g.nodes.push(Node { kind: Kind::Const, ty, title: text, rows: vec![], x: 0.0, y: 0.0, w, h: HDR + 2.0, path: path.clone(), ins: vec![], is_if: false, has_in: false, has_out: false });
            Built { node: this.g.nodes.len() - 1, w, h: HDR + 2.0 }
        };
        let obj = match e.as_object() {
            Some(o) if !o.is_empty() => o,
            _ => {
                let ty = match e {
                    Json::Bool(_) => Ty::Bool,
                    Json::Str(_) => Ty::Str,
                    _ => Ty::Num,
                };
                return leaf(self, e.to_string_compact().unwrap_or_else(|_| "?".into()), ty);
            }
        };
        let (op, arg) = (&obj[0].0, &obj[0].1);
        let arg_path = key(path, op);
        let mut title = op.clone();
        let mut kids: Vec<(&Json, Path)> = Vec::new();
        let mut labels: Vec<String> = Vec::new();
        let text = |v: &Json| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string_compact().unwrap_or_default());
        match op.as_str() {
            "get" => title = format!("Get {}", text(arg)),
            "res" => title = format!("Resource {}", text(arg)),
            "var" => title = format!("Var {}", text(arg)),
            "dt" => title = "Delta time".to_string(),
            "tick" => title = "Tick".to_string(),
            "uid" => title = if arg.as_str().is_some() { format!("Uid of {}", text(arg)) } else { "This entity uid".to_string() },
            "contact" => title = format!("Contact {}", text(arg)),
            "rand" => {
                title = "Random".to_string();
                kids.push((arg, key(path, op)));
                labels.push("salt".to_string());
            }
            _ => {
                if let Some(a) = arg.as_array() {
                    for (i, x) in a.iter().enumerate() {
                        kids.push((x, idx(&arg_path, i)));
                    }
                    labels = match row_names(op) {
                        Some(names) => names.iter().map(|s| s.to_string()).collect(),
                        None => (0..a.len()).map(|i| ((b'a' + (i % 26) as u8) as char).to_string()).collect(),
                    };
                } else {
                    kids.push((arg, arg_path.clone()));
                    labels.push("x".to_string());
                }
            }
        }
        let start = self.g.nodes.len();
        let mut built: Vec<Built> = Vec::new();
        let (mut cy, mut max_w) = (0.0f32, 0.0f32);
        for (ke, kp) in &kids {
            let s0 = self.g.nodes.len();
            let k = self.expr(ke, kp);
            self.shift(s0, 0.0, cy);
            cy += k.h + 8.0;
            max_w = max_w.max(k.w);
            built.push(k);
        }
        let kids_h = if kids.is_empty() { 0.0 } else { cy - 8.0 };
        let h = HDR + ROW * labels.len() as f32;
        let w = node_width(&title, &labels);
        let total = kids_h.max(h);
        // children right-aligned against the node
        for k in &built {
            self.shift_tree(k.node, max_w - k.w, 0.0);
        }
        let dy_kids = if kids_h < h { (h - kids_h) / 2.0 } else { 0.0 };
        if dy_kids != 0.0 {
            self.shift(start, 0.0, dy_kids);
        }
        let kind = if matches!(op.as_str(), "get" | "var" | "res") { Kind::Get } else { Kind::Op };
        let ty = if is_bool_op(op) { Ty::Bool } else { Ty::Num };
        let x = max_w + if kids.is_empty() { 0.0 } else { GAP };
        let y = if kids_h > h { (kids_h - h) / 2.0 } else { 0.0 };
        self.g.nodes.push(Node { kind, ty, title, rows: labels, x, y, w, h, path: path.clone(), ins: built.iter().map(|b| b.node).collect(), is_if: false, has_in: false, has_out: false });
        let me = self.g.nodes.len() - 1;
        for (i, k) in built.iter().enumerate() {
            let ty = self.g.nodes[k.node].ty;
            self.g.wires.push(Wire { from: k.node, to: me, kind: WireKind::Data(i, ty) });
        }
        Built { node: me, w: x + w, h: total }
    }

    fn stmt_node(&mut self, s: &Json, path: &Path) -> (usize, Vec<(Json, Path)>, &'static str) {
        let kinds = ["set", "let", "if", "despawn", "spawn_prefab", "spawn", "add", "remove", "res_set", "res_add", "emit"];
        let kind = kinds.iter().copied().find(|k| s.get(k).is_some()).unwrap_or("unknown");
        let text = |k: &str| s.get(k).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string_compact().unwrap_or_default())).unwrap_or_default();
        let (title, rows, inputs): (String, Vec<String>, Vec<(Json, Path)>) = match kind {
            "set" => (format!("Set {}", text("set")), vec!["value".into()], vec![(s.get("to").cloned().unwrap_or(Json::Null), key(path, "to"))]),
            "let" => (format!("Let {}", text("let")), vec!["value".into()], vec![(s.get("be").cloned().unwrap_or(Json::Null), key(path, "be"))]),
            "res_set" => (format!("Set resource {}", text("res_set")), vec!["value".into()], vec![(s.get("to").cloned().unwrap_or(Json::Null), key(path, "to"))]),
            "res_add" => (format!("Add to resource {}", text("res_add")), vec!["by".into()], vec![(s.get("by").cloned().unwrap_or(Json::Null), key(path, "by"))]),
            "emit" => (format!("Emit event {}", text("emit")), vec![], vec![]),
            "if" => ("If".to_string(), vec!["condition".into(), String::new()], vec![(s.get("if").cloned().unwrap_or(Json::Null), key(path, "if"))]),
            "despawn" => (if s.get("despawn").and_then(|v| v.as_str()).is_some() { format!("Despawn {}", text("despawn")) } else { "Despawn this entity".to_string() }, vec![], vec![]),
            "remove" => (format!("Remove {}{}", text("remove"), if s.get("on").is_some() { format!(" from {}", text("on")) } else { String::new() }), vec![], vec![]),
            "add" => {
                let mut rows = Vec::new();
                let mut inputs = Vec::new();
                if let Some(vals) = s.get("values").and_then(|v| v.as_object()) {
                    for (f, ex) in vals {
                        rows.push(f.clone());
                        inputs.push((ex.clone(), key(&key(path, "values"), f)));
                    }
                }
                (format!("Add {}{}", text("add"), if s.get("on").is_some() { format!(" to {}", text("on")) } else { String::new() }), rows, inputs)
            }
            "spawn" => {
                let mut rows = Vec::new();
                let mut inputs = Vec::new();
                if let Some(cs) = s.get("spawn").and_then(|v| v.as_object()) {
                    for (c, fields) in cs {
                        for (f, ex) in fields.as_object().unwrap_or(&[]) {
                            rows.push(format!("{c}.{f}"));
                            inputs.push((ex.clone(), key(&key(&key(path, "spawn"), c), f)));
                        }
                    }
                }
                ("Spawn entity".to_string(), rows, inputs)
            }
            "spawn_prefab" => {
                let mut rows = Vec::new();
                let mut inputs = Vec::new();
                for (name, label) in [("at", "at"), ("velocity", "velocity")] {
                    for (i, ex) in s.get(name).and_then(|v| v.as_array()).unwrap_or(&[]).iter().enumerate() {
                        rows.push(format!("{label}.{}", ["x", "y", "z"].get(i).unwrap_or(&"?")));
                        inputs.push((ex.clone(), idx(&key(path, name), i)));
                    }
                }
                if let Some(cs) = s.get("with").and_then(|v| v.as_object()) {
                    for (c, fields) in cs {
                        for (f, ex) in fields.as_object().unwrap_or(&[]) {
                            rows.push(format!("{c}.{f}"));
                            inputs.push((ex.clone(), key(&key(&key(path, "with"), c), f)));
                        }
                    }
                }
                (format!("Spawn prefab {}", text("spawn_prefab")), rows, inputs)
            }
            _ => ("Unknown statement".to_string(), vec![s.to_string_compact().unwrap_or_default().chars().take(40).collect()], vec![]),
        };
        let widest: Vec<String> = if kind == "if" { vec!["condition then >".into()] } else { rows.clone() };
        let w = node_width(&title, &widest);
        let h = HDR + ROW * rows.len() as f32 + 4.0;
        self.g.nodes.push(Node { kind: if kind == "if" { Kind::Ctrl } else { Kind::Stmt }, ty: Ty::Num, title, rows, x: 0.0, y: 0.0, w, h, path: path.clone(), ins: vec![], is_if: kind == "if", has_in: true, has_out: true });
        (self.g.nodes.len() - 1, inputs, kind)
    }

    /// Lay out a list of statements starting at (x, y); returns (first node, last node, height, x of the statement column).
    fn block(&mut self, stmts: &[Json], path: &Path, x: f32, y: f32) -> (Option<usize>, f32, f32) {
        struct Item {
            node: usize,
            trees: Vec<(Built, usize)>,
            trees_h: f32,
            trees_w: f32,
            stmt: Json,
            kind: &'static str,
        }
        let mut items: Vec<Item> = Vec::new();
        // statements first (their nodes), then the input trees, so that node indices stay in step with the JS version's draw order
        for (i, s) in stmts.iter().enumerate() {
            let p = idx(path, i);
            let (node, inputs, kind) = self.stmt_node(s, &p);
            let (mut trees, mut th, mut tw) = (Vec::new(), 0.0f32, 0.0f32);
            for (row, (ex, ep)) in inputs.iter().enumerate() {
                let t = self.expr(ex, ep);
                th += t.h + 8.0;
                tw = tw.max(t.w);
                trees.push((t, row));
            }
            items.push(Item { node, trees, trees_h: (th - 8.0).max(0.0), trees_w: tw, stmt: s.clone(), kind });
        }
        let tree_w = items.iter().fold(0.0f32, |m, it| m.max(it.trees_w));
        let node_x = x + tree_w + if tree_w > 0.0 { GAP } else { 0.0 };
        let (mut cy, mut prev, mut first): (f32, Option<usize>, Option<usize>) = (y, None, None);
        for it in items {
            let n = it.node;
            let mut ty = cy;
            for (t, row) in &it.trees {
                let root = t.node;
                let target = node_x - GAP - self.g.nodes[root].w;
                let (dx, dy) = (target - self.g.nodes[root].x, ty);
                self.shift_tree(root, dx, dy);
                ty += t.h + 8.0;
                let ty_out = self.g.nodes[root].ty;
                self.g.wires.push(Wire { from: root, to: n, kind: WireKind::Data(*row, ty_out) });
            }
            self.g.nodes[n].x = node_x;
            self.g.nodes[n].y = cy;
            if let Some(p) = prev {
                self.g.wires.push(Wire { from: p, to: n, kind: WireKind::Exec });
            }
            let mut band = self.g.nodes[n].h.max(it.trees_h);
            if it.kind == "if" {
                let nx = self.g.nodes[n].x + self.g.nodes[n].w + 56.0;
                let mut ny = cy;
                let then_stmts = it.stmt.get("then").and_then(|v| v.as_array()).map(|a| a.to_vec()).unwrap_or_default();
                let (then_first, then_h, _) = self.block(&then_stmts, &key(&self.g.nodes[n].path.clone(), "then"), nx, ny);
                if let Some(f) = then_first {
                    self.g.wires.push(Wire { from: n, to: f, kind: WireKind::Branch(0) });
                }
                ny += then_h.max(0.0) + if then_h > 0.0 { 16.0 } else { 0.0 };
                let else_stmts = it.stmt.get("else").and_then(|v| v.as_array()).map(|a| a.to_vec()).unwrap_or_default();
                if !else_stmts.is_empty() {
                    let (else_first, else_h, _) = self.block(&else_stmts, &key(&self.g.nodes[n].path.clone(), "else"), nx, ny);
                    if let Some(f) = else_first {
                        self.g.wires.push(Wire { from: n, to: f, kind: WireKind::Branch(1) });
                    }
                    ny += else_h;
                }
                band = band.max(ny - cy);
            }
            if first.is_none() {
                first = Some(n);
            }
            prev = Some(n);
            cy += band + 22.0;
        }
        (first, if prev.is_some() { cy - y - 22.0 } else { 0.0 }, node_x)
    }
}

/// The graph of a system definition.
pub fn build(def: &Json) -> Graph {
    let mut lay = Layout { g: Graph::default() };
    let names = |v: Option<&Json>| -> Vec<String> { v.and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default() };
    let each = names(def.get("each"));
    let without = names(def.get("without"));
    let name = def.get("name").and_then(|n| n.as_str()).unwrap_or("(unnamed)");
    let (title, mut rows) = match def.get("on_contact") {
        Some(oc) if oc.as_object().is_some() => {
            let (a, b) = (names(oc.get("a")), names(oc.get("b")));
            let mut r = vec![format!("a: {}", if a.is_empty() { "(none)".to_string() } else { a.join(", ") }), format!("b: {}", if b.is_empty() { "(none)".to_string() } else { b.join(", ") })];
            if let Some(t) = oc.get("triggers").and_then(|t| t.as_str()).filter(|t| *t != "all") {
                r.push(format!("triggers: {t}"));
            }
            (format!("On contact: {name}"), r)
        }
        _ => (format!("Every tick: {name}"), vec![format!("for each: {}", if each.is_empty() { "(none)".to_string() } else { each.join(", ") })]),
    };
    if !without.is_empty() {
        rows.push(format!("without: {}", without.join(", ")));
    }
    if let Some(o) = def.get("order") {
        rows.push(format!("order: {}", o.to_string_compact().unwrap_or_default()));
    }
    let (w, h) = (node_width(&title, &rows), HDR + ROW * rows.len() as f32 + 4.0);
    let stmts = def.get("do").and_then(|d| d.as_array()).map(|a| a.to_vec()).unwrap_or_default();
    let block_y = h + 26.0;
    let (first, _, node_x) = lay.block(&stmts, &vec![Seg::Key("do".to_string())], 0.0, block_y);
    let ev = Node { kind: Kind::Event, ty: Ty::Num, title, rows, x: node_x, y: 0.0, w, h, path: vec![], ins: vec![], is_if: false, has_in: false, has_out: true };
    // the event node goes first, as in the browser version: every index is one more
    let mut g = lay.g;
    g.nodes.insert(0, ev);
    for wire in g.wires.iter_mut() {
        wire.from += 1;
        wire.to += 1;
    }
    for n in g.nodes.iter_mut().skip(1) {
        for i in n.ins.iter_mut() {
            *i += 1;
        }
    }
    if let Some(f) = first {
        g.wires.push(Wire { from: 0, to: f + 1, kind: WireKind::Exec });
    }
    g
}

pub fn bounds(g: &Graph) -> Rect {
    let mut r = Rect::NOTHING;
    for n in &g.nodes {
        r = r.union(Rect::from_min_size(Pos2::new(n.x, n.y - 6.0), Vec2::new(n.w, n.h + 12.0)));
    }
    r
}

/// The value at a path of a document, and a way to change it.
pub fn get_at<'a>(doc: &'a Json, path: &[Seg]) -> Option<&'a Json> {
    let mut cur = doc;
    for s in path {
        cur = match s {
            Seg::Key(k) => cur.get(k)?,
            Seg::Idx(i) => cur.as_array()?.get(*i)?,
        };
    }
    Some(cur)
}

pub fn set_at(doc: &mut Json, path: &[Seg], value: Json) -> bool {
    let Some((last, parents)) = path.split_last() else {
        *doc = value;
        return true;
    };
    let mut cur = doc;
    for s in parents {
        cur = match s {
            Seg::Key(k) => match cur {
                Json::Obj(o) => match o.iter_mut().find(|(n, _)| n == k) {
                    Some((_, v)) => v,
                    None => return false,
                },
                _ => return false,
            },
            Seg::Idx(i) => match cur {
                Json::Arr(a) => match a.get_mut(*i) {
                    Some(v) => v,
                    None => return false,
                },
                _ => return false,
            },
        };
    }
    match (last, cur) {
        (Seg::Key(k), Json::Obj(o)) => match o.iter_mut().find(|(n, _)| n == k) {
            Some((_, v)) => {
                *v = value;
                true
            }
            None => false,
        },
        (Seg::Idx(i), Json::Arr(a)) if *i < a.len() => {
            a[*i] = value;
            true
        }
        _ => false,
    }
}

/// What a typed text means as a constant: a number, `true`/`false`, or text.
pub fn parse_const(text: &str, was: &Json) -> Json {
    let t = text.trim();
    match was {
        Json::Str(_) => Json::str(t.trim_matches('"')),
        Json::Bool(_) => Json::Bool(t == "true" || t == "1"),
        _ => t.parse::<f64>().map(Json::from).unwrap_or_else(|_| Json::str(t)),
    }
}

// ----------------------------------------------------------------------------------------------------------------- drawing

pub struct GraphView {
    pub zoom: f32,
    pub pan: Vec2,
    pub graph: Graph,
    pub def: Option<Json>,
    stamp: u64,
    pub error: Option<String>,
    editing: Option<(Path, String)>,
    fit_next: bool,
}

impl Default for GraphView {
    fn default() -> GraphView {
        GraphView { zoom: 1.0, pan: Vec2::new(24.0, 24.0), graph: Graph::default(), def: None, stamp: 0, error: None, editing: None, fit_next: true }
    }
}

fn hash(s: &str) -> u64 {
    s.bytes().fold(1469598103934665603u64, |h, b| (h ^ b as u64).wrapping_mul(1099511628211))
}

fn ty_color(t: Ty) -> Color32 {
    match t {
        Ty::Num => Color32::from_rgb(120, 200, 130),
        Ty::Bool => Color32::from_rgb(235, 100, 100),
        Ty::Str => Color32::from_rgb(220, 120, 220),
    }
}

impl GraphView {
    /// Make the picture from the document text (when it has changed).
    pub fn set_text(&mut self, text: &str, refit: bool) {
        let h = hash(text);
        if h == self.stamp && !refit {
            return;
        }
        self.stamp = h;
        if text.trim().is_empty() {
            self.graph = Graph::default();
            self.def = None;
            self.error = None;
            return;
        }
        match lunaris_json::parse(text) {
            Ok(def) => {
                self.graph = build(&def);
                self.def = Some(def);
                self.error = None;
                if refit {
                    self.fit_next = true;
                }
            }
            Err(e) => self.error = Some(format!("Not valid JSON yet: {e}")),
        }
    }

    /// Show the graph; returns the new document text when the person edited a constant.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size().max(Vec2::splat(40.0)), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(30, 30, 32));
        // a dotted grid, like the other editors' graph views
        let step = (24.0 * self.zoom).max(8.0);
        let mut x = rect.min.x + self.pan.x.rem_euclid(step);
        while x < rect.max.x {
            let mut y = rect.min.y + self.pan.y.rem_euclid(step);
            while y < rect.max.y {
                painter.circle_filled(Pos2::new(x, y), 0.9, Color32::from_rgb(52, 52, 56));
                y += step;
            }
            x += step;
        }
        if self.graph.nodes.is_empty() {
            painter.text(rect.center(), Align2::CENTER_CENTER, if self.error.is_some() { self.error.clone().unwrap_or_default() } else { "Select a graph system to see its nodes.".to_string() }, FontId::proportional(13.0), theme::TEXT_DIM);
            return None;
        }
        if self.fit_next {
            self.fit(rect.size());
            self.fit_next = false;
        }
        if resp.dragged() {
            self.pan += resp.drag_delta();
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                if let Some(p) = resp.hover_pos() {
                    let k = (self.zoom * (1.0 + scroll * 0.0015)).clamp(0.15, 3.0);
                    let local = p - rect.min;
                    self.pan = local - (local - self.pan) * (k / self.zoom);
                    self.zoom = k;
                }
            }
        }
        let k = self.zoom;
        let at = |x: f32, y: f32| rect.min + self.pan + Vec2::new(x, y) * k;
        // wires
        for w in &self.graph.wires {
            let (f, t) = (&self.graph.nodes[w.from], &self.graph.nodes[w.to]);
            match w.kind {
                WireKind::Exec => {
                    painter.line_segment([at(f.x + 10.0, f.y + f.h), at(t.x + 10.0, t.y)], Stroke::new(2.0 * k.max(0.6), Color32::from_rgb(235, 235, 235)));
                }
                WireKind::Branch(b) => {
                    let (p1, p2) = (at(f.x + f.w, f.y + HDR + ROW * b as f32 + ROW / 2.0), at(t.x + 10.0, t.y));
                    painter.add(Shape::CubicBezier(egui::epaint::CubicBezierShape::from_points_stroke([p1, p1 + Vec2::new(40.0 * k, 0.0), p2 - Vec2::new(0.0, 40.0 * k), p2], false, Color32::TRANSPARENT, Stroke::new(2.0 * k.max(0.6), Color32::from_rgb(235, 235, 235)))));
                }
                WireKind::Data(pin, ty) => {
                    let (p1, p2) = (at(f.x + f.w, f.y + HDR / 2.0), at(t.x, t.y + HDR + ROW * pin as f32 + ROW / 2.0));
                    let dx = ((p2.x - p1.x).abs() * 0.5).max(30.0 * k);
                    painter.add(Shape::CubicBezier(egui::epaint::CubicBezierShape::from_points_stroke([p1, p1 + Vec2::new(dx, 0.0), p2 - Vec2::new(dx, 0.0), p2], false, Color32::TRANSPARENT, Stroke::new(1.8 * k.max(0.6), ty_color(ty)))));
                }
            }
        }
        // nodes
        let mut clicked: Option<Path> = None;
        for n in &self.graph.nodes {
            let r = Rect::from_min_size(at(n.x, n.y), Vec2::new(n.w, n.h) * k);
            if !r.intersects(rect) {
                continue;
            }
            let (head, body) = match n.kind {
                Kind::Event => (Color32::from_rgb(150, 50, 50), Color32::from_rgb(52, 40, 40)),
                Kind::Stmt => (Color32::from_rgb(60, 90, 140), Color32::from_rgb(40, 44, 54)),
                Kind::Ctrl => (Color32::from_rgb(110, 70, 150), Color32::from_rgb(46, 40, 56)),
                Kind::Get => (Color32::from_rgb(60, 120, 80), Color32::from_rgb(38, 48, 42)),
                Kind::Op => (Color32::from_rgb(78, 78, 84), Color32::from_rgb(42, 42, 46)),
                Kind::Const => (ty_color(n.ty).gamma_multiply(0.45), Color32::from_rgb(40, 40, 44)),
            };
            painter.rect_filled(r, CornerRadius::same((6.0 * k) as u8), body);
            painter.rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width(), HDR * k)), CornerRadius { nw: (6.0 * k) as u8, ne: (6.0 * k) as u8, sw: 0, se: 0 }, head);
            painter.rect_stroke(r, CornerRadius::same((6.0 * k) as u8), Stroke::new(1.0, Color32::from_rgb(20, 20, 20)), StrokeKind::Inside);
            if k > 0.35 {
                painter.text(r.min + Vec2::new(PADX * k, HDR * k / 2.0), Align2::LEFT_CENTER, &n.title, FontId::proportional(12.0 * k), Color32::WHITE);
            }
            for (i, row) in n.rows.iter().enumerate() {
                let y = (HDR + ROW * i as f32 + ROW / 2.0) * k;
                if k > 0.45 && !row.is_empty() {
                    painter.text(r.min + Vec2::new(10.0 * k, y), Align2::LEFT_CENTER, row, FontId::proportional(11.0 * k), theme::TEXT_DIM);
                }
                if matches!(n.kind, Kind::Op | Kind::Stmt | Kind::Ctrl) && !(n.is_if && i == 1) && !matches!(n.kind, Kind::Event) {
                    painter.circle_filled(r.min + Vec2::new(0.0, y), 3.6 * k, Color32::from_rgb(120, 200, 130));
                }
                if n.is_if {
                    painter.text(r.min + Vec2::new(n.w * k - 8.0 * k, y), Align2::RIGHT_CENTER, if i == 0 { "then >" } else { "else >" }, FontId::proportional(11.0 * k), theme::TEXT_DIM);
                }
            }
            if matches!(n.kind, Kind::Const | Kind::Get | Kind::Op) {
                painter.circle_filled(r.min + Vec2::new(n.w * k, HDR * k / 2.0), 3.8 * k, ty_color(n.ty));
            }
            if n.has_in {
                painter.add(Shape::convex_polygon(vec![r.min + Vec2::new(5.0 * k, -6.0 * k), r.min + Vec2::new(15.0 * k, -6.0 * k), r.min + Vec2::new(10.0 * k, 0.0)], Color32::WHITE, Stroke::NONE));
            }
            if n.has_out {
                painter.add(Shape::convex_polygon(vec![r.min + Vec2::new(5.0 * k, n.h * k), r.min + Vec2::new(15.0 * k, n.h * k), r.min + Vec2::new(10.0 * k, n.h * k + 6.0 * k)], Color32::WHITE, Stroke::NONE));
            }
            if n.kind == Kind::Const && resp.clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    if r.contains(p) {
                        clicked = Some(n.path.clone());
                    }
                }
            }
        }
        if let (Some(p), Some(def)) = (clicked, &self.def) {
            let text = get_at(def, &p).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string_compact().unwrap_or_default())).unwrap_or_default();
            self.editing = Some((p, text));
        }
        // editing a constant: a small box at the top left of the view
        let mut result = None;
        let mut done = false;
        if let Some((path, text)) = &mut self.editing {
            egui::Area::new(egui::Id::new("graph-edit")).fixed_pos(rect.min + Vec2::new(12.0, 12.0)).show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("value");
                        let r = ui.add(egui::TextEdit::singleline(text).desired_width(140.0));
                        r.request_focus();
                        if ui.button("Set").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                            if let Some(def) = &mut self.def {
                                let was = get_at(def, path).cloned().unwrap_or(Json::Null);
                                let v = parse_const(text, &was);
                                if set_at(def, path, v) {
                                    result = def.to_string_pretty().ok();
                                }
                            }
                            done = true;
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            done = true;
                        }
                    });
                });
            });
        }
        if done {
            self.editing = None;
        }
        result
    }

    pub fn fit(&mut self, size: Vec2) {
        let b = bounds(&self.graph);
        if !b.is_positive() {
            return;
        }
        let k = ((size.x - 40.0) / b.width()).min((size.y - 40.0) / b.height()).clamp(0.5, 1.3);
        self.zoom = k;
        self.pan = Vec2::new((size.x - b.width() * k) / 2.0 - b.min.x * k, ((size.y - b.height() * k) / 2.0).max(20.0) - b.min.y * k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunaris_json::parse;

    const SYSTEM: &str = r#"{"name":"fall","each":["Transform","Velocity"],"do":[
        {"set":"Transform.pos[1]","to":{"add":[{"get":"Transform.pos[1]"},{"mul":[0.5,{"dt":true}]}]}},
        {"if":{"lt":[{"get":"Transform.pos[1]"},0]},"then":[{"set":"Transform.pos[1]","to":0},{"emit":"thud"}],"else":[{"res_add":"air","by":1}]}]}"#;

    #[test]
    fn a_system_becomes_an_event_node_statements_expressions_and_the_wires_between_them() {
        let g = build(&parse(SYSTEM).unwrap());
        assert_eq!(g.nodes[0].kind, Kind::Event);
        assert!(g.nodes[0].title.contains("fall") && g.nodes[0].rows[0].contains("Transform, Velocity"));
        let count = |k: Kind| g.nodes.iter().filter(|n| n.kind == k).count();
        assert_eq!(count(Kind::Stmt), 4, "set, set, emit, res_add");
        assert_eq!(count(Kind::Ctrl), 1, "the if");
        assert!(count(Kind::Op) >= 3 && count(Kind::Get) >= 2 && count(Kind::Const) >= 2, "{} ops, {} gets, {} constants", count(Kind::Op), count(Kind::Get), count(Kind::Const));
        let exec = g.wires.iter().filter(|w| w.kind == WireKind::Exec).count();
        let branches = g.wires.iter().filter(|w| matches!(w.kind, WireKind::Branch(_))).count();
        assert_eq!(branches, 2, "then and else");
        assert!(exec >= 3, "event to the first statement, statement to statement, inside the branches: {exec}");
        // every data wire goes from a node on the left to a pin of a node on the right
        for w in &g.wires {
            if let WireKind::Data(pin, _) = w.kind {
                let (f, t) = (&g.nodes[w.from], &g.nodes[w.to]);
                assert!(f.x + f.w <= t.x + 0.01, "{} -> {}", f.title, t.title);
                assert!(pin < t.rows.len(), "pin {pin} of {}", t.title);
            }
        }
        // statements of one block are in one column, top to bottom, and nothing overlaps
        for (i, a) in g.nodes.iter().enumerate() {
            for b in g.nodes.iter().skip(i + 1) {
                let (ra, rb) = (Rect::from_min_size(Pos2::new(a.x, a.y), Vec2::new(a.w, a.h)), Rect::from_min_size(Pos2::new(b.x, b.y), Vec2::new(b.w, b.h)));
                assert!(!ra.shrink(0.5).intersects(rb.shrink(0.5)), "{} overlaps {}", a.title, b.title);
            }
        }
    }

    #[test]
    fn constants_are_found_by_path_and_changed_in_the_document() {
        let mut def = parse(SYSTEM).unwrap();
        let g = build(&def);
        let half = g.nodes.iter().find(|n| n.kind == Kind::Const && n.title == "0.5").expect("the 0.5");
        assert_eq!(get_at(&def, &half.path).and_then(|v| v.as_f64()), Some(0.5));
        assert!(set_at(&mut def, &half.path, parse_const("2.5", &Json::from(0.5))));
        let g2 = build(&def);
        assert!(g2.nodes.iter().any(|n| n.kind == Kind::Const && n.title == "2.5") && !g2.nodes.iter().any(|n| n.title == "0.5"));
        assert!(!set_at(&mut def, &[Seg::Key("nope".into())], Json::Null), "a path that is not there is refused");
        assert_eq!(parse_const("true", &Json::Bool(false)), Json::Bool(true));
        assert_eq!(parse_const("\"hi\"", &Json::str("x")), Json::str("hi"));
    }

    #[test]
    fn odd_documents_draw_something_and_never_panic() {
        for text in [r#"{}"#, r#"{"name":"x"}"#, r#"{"name":"x","each":[],"do":[{"nope":1}]}"#, r#"{"name":"c","on_contact":{"a":["A"],"b":["B"],"triggers":"only"},"do":[{"emit":"hit"}]}"#, r#"{"name":"d","do":[{"set":"a","to":{"mul":[]}},{"add":"C","values":{"x":1,"y":{"neg":2}}},{"spawn":{"Transform":{"pos":0}}},{"spawn_prefab":"P","at":[1,2,3]}]}"#] {
            let g = build(&parse(text).unwrap());
            assert!(!g.nodes.is_empty(), "{text}");
        }
    }
}
