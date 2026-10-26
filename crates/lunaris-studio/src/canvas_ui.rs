//! The Canvas tab: a board of notes, groups and arrows for plans and maps (a game's systems, its milestones), inside the editor. The files are in the JSON Canvas
//! format (`*.canvas`: `nodes` of type `text`, `file`, `link` and `group`, and `edges` with a label and a colour), the same one Obsidian reads and writes, so a canvas
//! made there opens here and the other way round. Drag the background to move around, scroll to zoom, drag a note to move it (a group takes what is inside it), click a
//! note to edit its text and colour in the Details panel. The file is read again when another program (an agent updating a plan) saves it, as long as the board has no
//! unsaved changes of its own. Nothing here has to do with a browser: it is drawn by the editor like every other panel.
use crate::theme;
use eframe::egui::{self, epaint::CubicBezierShape, text::LayoutJob, text::TextFormat, Align2, Color32, CornerRadius, FontId, Key, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use lunaris_json::Json;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

#[derive(Clone, Debug)]
struct Node {
    id: String,
    /// `text`, `file`, `link` or `group`.
    kind: String,
    /// The note's text, the group's label, the file's path or the link's address.
    body: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Option<String>,
    /// Whatever else the file had on the node (kept as it was when saved again).
    extra: Vec<(String, Json)>,
}

#[derive(Clone, Debug)]
struct Edge {
    id: String,
    from: String,
    to: String,
    from_side: Option<String>,
    to_side: Option<String>,
    label: String,
    color: Option<String>,
    extra: Vec<(String, Json)>,
}

#[derive(Clone, Debug, Default)]
struct Doc {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    extra: Vec<(String, Json)>,
}

fn body_key(kind: &str) -> &'static str {
    match kind {
        "group" => "label",
        "file" => "file",
        "link" => "url",
        _ => "text",
    }
}

fn num(j: &Json, key: &str, default: f32) -> f32 {
    j.get(key).and_then(|v| v.as_f64()).map(|v| v as f32).filter(|v| v.is_finite()).unwrap_or(default)
}

fn text_of(j: &Json, key: &str) -> Option<String> {
    j.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

fn keep(j: &Json, known: &[&str]) -> Vec<(String, Json)> {
    j.as_object().map(|o| o.iter().filter(|(k, _)| !known.contains(&k.as_str())).cloned().collect()).unwrap_or_default()
}

fn parse(text: &str) -> Result<Doc, String> {
    let j = lunaris_json::parse(text).map_err(|e| format!("not valid JSON: {} (line {})", e.message, e.line))?;
    let mut doc = Doc { extra: keep(&j, &["nodes", "edges"]), ..Default::default() };
    for (i, n) in j.get("nodes").and_then(|n| n.as_array()).unwrap_or(&[]).iter().enumerate() {
        let kind = text_of(n, "type").unwrap_or_else(|| "text".to_string());
        let body = text_of(n, body_key(&kind)).unwrap_or_default();
        doc.nodes.push(Node {
            id: text_of(n, "id").unwrap_or_else(|| format!("n{i}")),
            kind: kind.clone(),
            body,
            x: num(n, "x", 0.0),
            y: num(n, "y", 0.0),
            w: num(n, "width", 250.0).max(20.0),
            h: num(n, "height", 60.0).max(20.0),
            color: n.get("color").and_then(|c| c.as_str().map(str::to_string).or_else(|| c.as_i64().map(|v| v.to_string()))),
            extra: keep(n, &["id", "type", "x", "y", "width", "height", "color", "text", "label", "file", "url"]),
        });
    }
    for (i, e) in j.get("edges").and_then(|n| n.as_array()).unwrap_or(&[]).iter().enumerate() {
        let (Some(from), Some(to)) = (text_of(e, "fromNode"), text_of(e, "toNode")) else { continue };
        doc.edges.push(Edge {
            id: text_of(e, "id").unwrap_or_else(|| format!("e{i}")),
            from,
            to,
            from_side: text_of(e, "fromSide"),
            to_side: text_of(e, "toSide"),
            label: text_of(e, "label").unwrap_or_default(),
            color: e.get("color").and_then(|c| c.as_str().map(str::to_string).or_else(|| c.as_i64().map(|v| v.to_string()))),
            extra: keep(e, &["id", "fromNode", "toNode", "fromSide", "toSide", "label", "color"]),
        });
    }
    Ok(doc)
}

fn to_json(doc: &Doc) -> Json {
    let mut nodes = Vec::new();
    for n in &doc.nodes {
        let mut o: Vec<(String, Json)> = vec![("id".into(), Json::str(n.id.clone())), ("type".into(), Json::str(n.kind.clone())), (body_key(&n.kind).into(), Json::str(n.body.clone()))];
        for (k, v) in [("x", n.x), ("y", n.y), ("width", n.w), ("height", n.h)] {
            o.push((k.into(), Json::from(v.round() as i64)));
        }
        if let Some(c) = &n.color {
            o.push(("color".into(), Json::str(c.clone())));
        }
        o.extend(n.extra.iter().cloned());
        nodes.push(Json::Obj(o));
    }
    let mut edges = Vec::new();
    for e in &doc.edges {
        let mut o: Vec<(String, Json)> = vec![("id".into(), Json::str(e.id.clone())), ("fromNode".into(), Json::str(e.from.clone())), ("toNode".into(), Json::str(e.to.clone()))];
        if let Some(s) = &e.from_side {
            o.push(("fromSide".into(), Json::str(s.clone())));
        }
        if let Some(s) = &e.to_side {
            o.push(("toSide".into(), Json::str(s.clone())));
        }
        if !e.label.is_empty() {
            o.push(("label".into(), Json::str(e.label.clone())));
        }
        if let Some(c) = &e.color {
            o.push(("color".into(), Json::str(c.clone())));
        }
        o.extend(e.extra.iter().cloned());
        edges.push(Json::Obj(o));
    }
    let mut root: Vec<(String, Json)> = vec![("nodes".into(), Json::Arr(nodes)), ("edges".into(), Json::Arr(edges))];
    root.extend(doc.extra.iter().cloned());
    Json::Obj(root)
}

/// The colour a canvas colour name stands for (Obsidian's six presets, or `#rrggbb`); grey for none.
fn tint(c: &Option<String>) -> Color32 {
    match c.as_deref() {
        Some("1") => Color32::from_rgb(232, 84, 84),
        Some("2") => Color32::from_rgb(236, 150, 62),
        Some("3") => Color32::from_rgb(224, 200, 76),
        Some("4") => Color32::from_rgb(88, 190, 112),
        Some("5") => Color32::from_rgb(82, 188, 212),
        Some("6") => Color32::from_rgb(160, 112, 220),
        Some(h) if h.starts_with('#') && h.len() == 7 => {
            let v = |a: usize| u8::from_str_radix(&h[a..a + 2], 16).unwrap_or(128);
            Color32::from_rgb(v(1), v(3), v(5))
        }
        _ => Color32::from_rgb(136, 140, 150),
    }
}

const PRESETS: [(&str, &str); 6] = [("1", "red"), ("2", "orange"), ("3", "yellow"), ("4", "green"), ("5", "cyan"), ("6", "purple")];

/// `[[note|shown]]` becomes `shown`, `[[note]]` becomes `note`.
fn unlink(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(a) = rest.find("[[") {
        let Some(b) = rest[a + 2..].find("]]") else { break };
        out.push_str(&rest[..a]);
        let inner = &rest[a + 2..a + 2 + b];
        out.push_str(inner.rsplit('|').next().unwrap_or(inner));
        rest = &rest[a + 2 + b + 2..];
    }
    out.push_str(rest);
    out
}

/// A note's text as drawn: `#` headings, `-` bullets, `**bold**` and `` `code` `` (the little of Markdown a plan uses).
fn md_job(text: &str, size: f32, width: f32, base: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width.max(8.0);
    let fmt = |size: f32, color: Color32, code: bool| TextFormat { font_id: if code { FontId::monospace(size * 0.92) } else { FontId::proportional(size) }, color, background: if code { Color32::from_rgb(30, 30, 34) } else { Color32::TRANSPARENT }, ..Default::default() };
    let mut first = true;
    for line in unlink(text).lines() {
        if !first {
            job.append("\n", 0.0, fmt(size, base, false));
        }
        first = false;
        let t = line.trim_start();
        let (mut s, mut sz, mut col) = (t, size, base);
        if let Some(r) = t.strip_prefix("### ") {
            (s, sz, col) = (r, size * 1.1, theme::TEXT_BRIGHT);
        } else if let Some(r) = t.strip_prefix("## ") {
            (s, sz, col) = (r, size * 1.25, theme::TEXT_BRIGHT);
        } else if let Some(r) = t.strip_prefix("# ") {
            (s, sz, col) = (r, size * 1.5, Color32::WHITE);
        } else if let Some(r) = t.strip_prefix("- ") {
            job.append("• ", 0.0, fmt(size, col, false));
            s = r;
        }
        let (mut bold, mut code) = (false, false);
        let mut rest = s;
        loop {
            let b = rest.find("**");
            let c = rest.find('`');
            let (at, len, is_code) = match (b, c) {
                (Some(b), Some(c)) if b < c => (b, 2, false),
                (Some(b), None) => (b, 2, false),
                (_, Some(c)) => (c, 1, true),
                (None, None) => break,
            };
            if at > 0 {
                job.append(&rest[..at], 0.0, fmt(sz, if bold { Color32::WHITE } else if code { Color32::from_rgb(150, 210, 230) } else { col }, code));
            }
            if is_code {
                code = !code;
            } else {
                bold = !bold;
            }
            rest = &rest[at + len..];
        }
        if !rest.is_empty() {
            job.append(rest, 0.0, fmt(sz, if bold { Color32::WHITE } else if code { Color32::from_rgb(150, 210, 230) } else { col }, code));
        }
    }
    job
}

/// Where an edge leaves or enters a node, and which way it points away from it.
fn anchor(n: &Node, side: &str) -> ([f32; 2], [f32; 2]) {
    match side {
        "top" => ([n.x + n.w / 2.0, n.y], [0.0, -1.0]),
        "bottom" => ([n.x + n.w / 2.0, n.y + n.h], [0.0, 1.0]),
        "left" => ([n.x, n.y + n.h / 2.0], [-1.0, 0.0]),
        _ => ([n.x + n.w, n.y + n.h / 2.0], [1.0, 0.0]),
    }
}

/// The side of `a` that faces `b`, when the file does not say.
fn facing(a: &Node, b: &Node) -> &'static str {
    let (dx, dy) = ((b.x + b.w / 2.0) - (a.x + a.w / 2.0), (b.y + b.h / 2.0) - (a.y + a.h / 2.0));
    if dx.abs() * a.h > dy.abs() * a.w {
        if dx > 0.0 {
            "right"
        } else {
            "left"
        }
    } else if dy > 0.0 {
        "bottom"
    } else {
        "top"
    }
}

fn contains(r: &Node, n: &Node) -> bool {
    n.x >= r.x && n.y >= r.y && n.x + n.w <= r.x + r.w && n.y + n.h <= r.y + r.h
}

#[derive(Clone, Debug)]
enum Drag {
    Pan,
    /// Moving these nodes (the grabbed one, and what a grabbed group holds).
    Nodes(Vec<usize>),
}

pub struct CanvasUi {
    /// The canvas file, relative to the workspace.
    pub path: Option<String>,
    root: PathBuf,
    doc: Doc,
    pan: Vec2,
    zoom: f32,
    fit_next: bool,
    selected: Option<usize>,
    drag: Option<Drag>,
    pub dirty: bool,
    stamp: Option<SystemTime>,
    next_check: Option<Instant>,
    message: Option<(String, bool)>,
    files: Vec<String>,
    files_at: Option<Instant>,
    outline_filter: String,
    /// The picture of the board's canvas area this frame (so that the Details panel can centre the view).
    area: Rect,
}

impl Default for CanvasUi {
    fn default() -> Self {
        CanvasUi {
            path: None,
            root: PathBuf::from("."),
            doc: Doc::default(),
            pan: Vec2::ZERO,
            zoom: 1.0,
            fit_next: true,
            selected: None,
            drag: None,
            dirty: false,
            stamp: None,
            next_check: None,
            message: None,
            files: Vec::new(),
            files_at: None,
            outline_filter: String::new(),
            area: Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 600.0)),
        }
    }
}

fn find_canvases(dir: &Path, depth: usize, root: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if depth < 4 && !matches!(name.as_str(), "target" | ".git" | "node_modules" | ".claude") {
                find_canvases(&p, depth + 1, root, out);
            }
        } else if name.ends_with(".canvas") {
            if let Ok(rel) = p.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        if out.len() > 200 {
            return;
        }
    }
}

impl CanvasUi {
    pub fn set_root(&mut self, root: &Path) {
        self.root = root.to_path_buf();
    }

    fn full(&self, rel: &str) -> PathBuf {
        let p = Path::new(rel);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }

    fn mtime(&self) -> Option<SystemTime> {
        let p = self.path.as_ref()?;
        std::fs::metadata(self.full(p)).and_then(|m| m.modified()).ok()
    }

    /// Read a canvas file (a path in the workspace, or a full one).
    pub fn open(&mut self, path: &str) {
        let rel = path.replace('\\', "/");
        let rel = {
            let root = self.root.to_string_lossy().replace('\\', "/");
            let root = root.trim_end_matches('/');
            if rel.to_lowercase().starts_with(&format!("{}/", root.to_lowercase())) {
                rel[root.len() + 1..].to_string()
            } else {
                rel
            }
        };
        match std::fs::read_to_string(self.full(&rel)).map_err(|e| e.to_string()).and_then(|t| parse(t.trim_start_matches('\u{feff}'))) {
            Ok(doc) => {
                let first = self.path.as_deref() != Some(rel.as_str());
                self.doc = doc;
                self.path = Some(rel);
                self.stamp = self.mtime();
                self.dirty = false;
                self.selected = None;
                self.drag = None;
                if first {
                    self.fit_next = true;
                }
                self.message = None;
            }
            Err(e) => self.message = Some((format!("Could not open {rel}: {e}"), true)),
        }
    }

    pub fn save(&mut self) {
        let Some(p) = self.path.clone() else {
            self.message = Some(("There is no file to save to: open or create one first".to_string(), true));
            return;
        };
        let text = to_json(&self.doc).to_string_pretty().unwrap_or_default();
        match std::fs::write(self.full(&p), text) {
            Ok(()) => {
                self.dirty = false;
                self.stamp = self.mtime();
                self.message = Some((format!("Saved {p}"), false));
            }
            Err(e) => self.message = Some((format!("Could not save {p}: {e}"), true)),
        }
    }

    /// Look at the file now and then: when another program saved it and the board has no unsaved changes, read it again (keeping the view and the selection).
    fn follow_disk(&mut self) {
        if self.dirty || !self.next_check.is_none_or(|t| Instant::now() >= t) {
            return;
        }
        self.next_check = Some(Instant::now() + std::time::Duration::from_millis(500));
        let (Some(was), Some(now)) = (self.stamp, self.mtime()) else { return };
        if was == now {
            return;
        }
        let (pan, zoom, sel) = (self.pan, self.zoom, self.selected.and_then(|i| self.doc.nodes.get(i).map(|n| n.id.clone())));
        let path = self.path.clone().unwrap_or_default();
        self.open(&path);
        if self.stamp != Some(was) {
            self.pan = pan;
            self.zoom = zoom;
            self.fit_next = false;
            self.selected = sel.and_then(|id| self.doc.nodes.iter().position(|n| n.id == id));
            self.message = Some(("The file changed on disk: read again".to_string(), false));
        }
    }

    fn fit(&mut self, area: Vec2) {
        if self.doc.nodes.is_empty() {
            self.pan = Vec2::new(20.0, 20.0);
            self.zoom = 1.0;
            return;
        }
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for n in &self.doc.nodes {
            lo = Vec2::new(lo.x.min(n.x), lo.y.min(n.y));
            hi = Vec2::new(hi.x.max(n.x + n.w), hi.y.max(n.y + n.h));
        }
        let size = (hi - lo).max(Vec2::splat(10.0));
        let k = (area.x / size.x).min(area.y / size.y) * 0.94;
        self.zoom = k.clamp(0.04, 1.5);
        self.pan = area / 2.0 - (lo + size / 2.0) * self.zoom;
    }

    fn center_on(&mut self, i: usize) {
        if let Some(n) = self.doc.nodes.get(i) {
            let c = Vec2::new(n.x + n.w / 2.0, n.y + n.h / 2.0);
            self.pan = self.area.size() / 2.0 - c * self.zoom;
        }
    }

    fn new_id(&self, base: &str) -> String {
        (1..).map(|i| format!("{base}{i}")).find(|id| !self.doc.nodes.iter().any(|n| &n.id == id) && !self.doc.edges.iter().any(|e| &e.id == id)).unwrap_or_default()
    }

    fn add_note(&mut self, at: [f32; 2], kind: &str) {
        let id = self.new_id(if kind == "group" { "group" } else { "note" });
        let (w, h) = if kind == "group" { (520.0, 320.0) } else { (320.0, 140.0) };
        let body = if kind == "group" { "Group".to_string() } else { "A new note".to_string() };
        self.doc.nodes.push(Node { id, kind: kind.to_string(), body, x: at[0].round(), y: at[1].round(), w, h, color: None, extra: Vec::new() });
        self.selected = Some(self.doc.nodes.len() - 1);
        self.dirty = true;
    }

    fn delete_selected(&mut self) {
        if let Some(i) = self.selected.take() {
            if i < self.doc.nodes.len() {
                let id = self.doc.nodes.remove(i).id;
                self.doc.edges.retain(|e| e.from != id && e.to != id);
                self.dirty = true;
            }
        }
    }

    /// What is under the pointer: the topmost note, or a group by its title strip.
    fn hit(&self, p: [f32; 2]) -> Option<usize> {
        let inside = |n: &Node, h: f32| p[0] >= n.x && p[0] <= n.x + n.w && p[1] >= n.y && p[1] <= n.y + h;
        for (i, n) in self.doc.nodes.iter().enumerate().rev() {
            if n.kind != "group" && inside(n, n.h) {
                return Some(i);
            }
        }
        for (i, n) in self.doc.nodes.iter().enumerate().rev() {
            if n.kind == "group" && inside(n, 34.0) {
                return Some(i);
            }
        }
        None
    }

    // ------------------------------------------------------------------------------------------------------- the panels

    /// The tool row over the board.
    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let label = match &self.path {
                Some(p) => format!("{}{}", p.rsplit('/').next().unwrap_or(p), if self.dirty { "*" } else { "" }),
                None => "no canvas open".to_string(),
            };
            let stale = self.files_at.is_none_or(|t| t.elapsed().as_secs() > 5);
            if stale {
                let mut found = Vec::new();
                find_canvases(&self.root.clone(), 0, &self.root.clone(), &mut found);
                self.files = found;
                self.files_at = Some(Instant::now());
            }
            let files = self.files.clone();
            let mut open: Option<String> = None;
            crate::chrome::drop_down(ui, &label, |ui| {
                if files.is_empty() {
                    ui.label("No .canvas files in the workspace");
                }
                for f in &files {
                    if ui.button(f).clicked() {
                        open = Some(f.clone());
                        ui.close();
                    }
                }
            });
            if let Some(f) = open {
                self.open(&f);
            }
            if ui.button("Fit").on_hover_text("Show the whole board (F)").clicked() {
                self.fit_next = true;
            }
            ui.label(egui::RichText::new(format!("{:.0}%", self.zoom * 100.0)).small().color(theme::TEXT_DIM));
            crate::chrome::rule(ui);
            if ui.button("+ Note").on_hover_text("Add a note in the middle of the view").clicked() {
                let c = (self.area.size() / 2.0 - self.pan) / self.zoom;
                self.add_note([c.x - 160.0, c.y - 70.0], "text");
            }
            if ui.button("+ Group").clicked() {
                let c = (self.area.size() / 2.0 - self.pan) / self.zoom;
                self.add_note([c.x - 260.0, c.y - 160.0], "group");
            }
            let save = ui.add_enabled(self.dirty && self.path.is_some(), egui::Button::new("Save"));
            if save.on_hover_text("Ctrl+S").clicked() {
                self.save();
            }
            if let Some((m, bad)) = &self.message {
                ui.label(egui::RichText::new(m.as_str()).small().color(if *bad { theme::ERR } else { theme::TEXT_DIM }));
            }
        });
    }

    /// The left panel: the notes of the board in order, to jump to one.
    pub fn outline(&mut self, ui: &mut Ui) {
        ui.add(egui::TextEdit::singleline(&mut self.outline_filter).hint_text("Search the notes").desired_width(f32::INFINITY));
        ui.add_space(4.0);
        let filter = self.outline_filter.to_lowercase();
        let mut go: Option<usize> = None;
        egui::ScrollArea::vertical().id_salt("canvas-outline").auto_shrink([false, false]).show(ui, |ui| {
            for (i, n) in self.doc.nodes.iter().enumerate() {
                let first = unlink(n.body.lines().find(|l| !l.trim().is_empty()).unwrap_or("")).replace(['#', '*', '`'], "").trim().to_string();
                if !filter.is_empty() && !n.body.to_lowercase().contains(&filter) {
                    continue;
                }
                let title = if first.is_empty() { format!("({})", n.kind) } else { first };
                let icon = if n.kind == "group" { "▣ " } else { "▪ " };
                let resp = ui.selectable_label(self.selected == Some(i), egui::RichText::new(format!("{icon}{title}")).color(tint(&n.color)));
                if resp.clicked() {
                    go = Some(i);
                }
            }
        });
        if let Some(i) = go {
            self.selected = Some(i);
            self.center_on(i);
        }
    }

    /// The right panel: the selected note's text, colour and size.
    pub fn details(&mut self, ui: &mut Ui) {
        let Some(i) = self.selected.filter(|i| *i < self.doc.nodes.len()) else {
            ui.label(egui::RichText::new("Click a note to edit it.\n\nDrag the background to move around, scroll to zoom, drag a note to move it.\nRight-click the board for a new note.").small().color(theme::TEXT_DIM));
            ui.add_space(8.0);
            ui.label(egui::RichText::new(format!("{} notes, {} arrows", self.doc.nodes.len(), self.doc.edges.len())).small().color(theme::TEXT_DIM));
            return;
        };
        let mut changed = false;
        let mut delete = false;
        {
            let n = &mut self.doc.nodes[i];
            ui.label(egui::RichText::new(format!("{} ({})", n.id, n.kind)).small().color(theme::TEXT_DIM));
            ui.add_space(4.0);
            let label = match n.kind.as_str() {
                "group" => "Label",
                "file" => "File",
                "link" => "Address",
                _ => "Text (Markdown: # heading, - bullet, **bold**, `code`)",
            };
            ui.label(label);
            if n.kind == "text" {
                changed |= ui.add(egui::TextEdit::multiline(&mut n.body).desired_width(f32::INFINITY).desired_rows(12)).changed();
            } else {
                changed |= ui.add(egui::TextEdit::singleline(&mut n.body).desired_width(f32::INFINITY)).changed();
            }
            ui.add_space(6.0);
            ui.label("Colour");
            ui.horizontal_wrapped(|ui| {
                let none = ui.selectable_label(n.color.is_none(), "none");
                if none.clicked() {
                    n.color = None;
                    changed = true;
                }
                for (code, name) in PRESETS {
                    let c = tint(&Some(code.to_string()));
                    let sel = n.color.as_deref() == Some(code);
                    let b = ui.add(egui::Button::new(egui::RichText::new(name).color(if sel { Color32::BLACK } else { c })).fill(if sel { c } else { theme::FIELD }));
                    if b.clicked() {
                        n.color = Some(code.to_string());
                        changed = true;
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("x");
                changed |= ui.add(egui::DragValue::new(&mut n.x).speed(1.0)).changed();
                ui.label("y");
                changed |= ui.add(egui::DragValue::new(&mut n.y).speed(1.0)).changed();
            });
            ui.horizontal(|ui| {
                ui.label("width");
                changed |= ui.add(egui::DragValue::new(&mut n.w).speed(1.0).range(40.0..=4000.0)).changed();
                ui.label("height");
                changed |= ui.add(egui::DragValue::new(&mut n.h).speed(1.0).range(40.0..=4000.0)).changed();
            });
            ui.add_space(8.0);
            if ui.button("Delete this note").clicked() {
                delete = true;
            }
        }
        // the arrows of this note
        let id = self.doc.nodes[i].id.clone();
        let labels: Vec<String> = self
            .doc
            .edges
            .iter()
            .filter(|e| e.from == id || e.to == id)
            .map(|e| {
                let other = if e.from == id { &e.to } else { &e.from };
                let name = self.doc.nodes.iter().find(|n| &n.id == other).map(|n| unlink(n.body.lines().next().unwrap_or("")).replace(['#', '*', '`'], "")).unwrap_or_default();
                format!("{} {}{}", if e.from == id { "→" } else { "←" }, name.trim(), if e.label.is_empty() { String::new() } else { format!("  ({})", e.label) })
            })
            .collect();
        if !labels.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Arrows").small().color(theme::TEXT_DIM));
            for l in labels {
                ui.label(egui::RichText::new(l).small());
            }
        }
        if changed {
            self.dirty = true;
        }
        if delete {
            self.delete_selected();
        }
    }

    // ------------------------------------------------------------------------------------------------------- the board

    pub fn show(&mut self, ui: &mut Ui) {
        self.follow_disk();
        self.toolbar(ui);
        ui.add_space(2.0);
        let (rect, resp) = ui.allocate_exact_size(ui.available_size().max(Vec2::splat(60.0)), Sense::click_and_drag());
        self.area = Rect::from_min_size(Pos2::ZERO, rect.size());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(30, 30, 32));
        if self.path.is_none() {
            painter.text(rect.center(), Align2::CENTER_CENTER, "Open a canvas from the list above, or start one with + Note.", FontId::proportional(14.0), theme::TEXT_DIM);
        }
        if self.fit_next {
            self.fit(rect.size());
            self.fit_next = false;
        }
        // the dotted grid
        let step = (40.0 * self.zoom).max(10.0);
        let mut x = rect.min.x + self.pan.x.rem_euclid(step);
        while x < rect.max.x {
            let mut y = rect.min.y + self.pan.y.rem_euclid(step);
            while y < rect.max.y {
                painter.circle_filled(Pos2::new(x, y), 0.9, Color32::from_rgb(50, 50, 54));
                y += step;
            }
            x += step;
        }
        // ---- the pointer: zoom, pan, move a note, select, add
        let to_canvas = |p: Pos2, pan: Vec2, zoom: f32| -> [f32; 2] {
            let v = (p - rect.min - pan) / zoom;
            [v.x, v.y]
        };
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                if let Some(p) = resp.hover_pos() {
                    let k = (self.zoom * (1.0 + scroll * 0.0016)).clamp(0.04, 2.5);
                    let local = p - rect.min;
                    self.pan = local - (local - self.pan) * (k / self.zoom);
                    self.zoom = k;
                }
            }
            let (f, del, save) = ui.input_mut(|i| (i.key_pressed(Key::F), i.key_pressed(Key::Delete), i.consume_key(egui::Modifiers::COMMAND, Key::S)));
            if f && !ui.ctx().egui_wants_keyboard_input() {
                self.fit_next = true;
            }
            if del && !ui.ctx().egui_wants_keyboard_input() {
                self.delete_selected();
            }
            if save {
                self.save();
            }
        }
        if resp.drag_started() {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(rect.center());
            let middle = ui.input(|i| i.pointer.middle_down());
            let at = to_canvas(origin, self.pan, self.zoom);
            self.drag = match (middle, self.hit(at)) {
                (false, Some(i)) => {
                    self.selected = Some(i);
                    let mut moving = vec![i];
                    if self.doc.nodes[i].kind == "group" {
                        let g = self.doc.nodes[i].clone();
                        moving.extend(self.doc.nodes.iter().enumerate().filter(|(j, n)| *j != i && contains(&g, n)).map(|(j, _)| j));
                    }
                    Some(Drag::Nodes(moving))
                }
                _ => Some(Drag::Pan),
            };
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            match self.drag.clone() {
                Some(Drag::Nodes(list)) => {
                    for i in list {
                        if let Some(n) = self.doc.nodes.get_mut(i) {
                            n.x += d.x / self.zoom;
                            n.y += d.y / self.zoom;
                        }
                    }
                    if d != Vec2::ZERO {
                        self.dirty = true;
                    }
                }
                _ => self.pan += d,
            }
        }
        if resp.drag_stopped() {
            if let Some(Drag::Nodes(list)) = self.drag.take() {
                for i in list {
                    if let Some(n) = self.doc.nodes.get_mut(i) {
                        n.x = n.x.round();
                        n.y = n.y.round();
                    }
                }
            }
            self.drag = None;
        }
        if resp.clicked() {
            let at = resp.interact_pointer_pos().map(|p| to_canvas(p, self.pan, self.zoom));
            self.selected = at.and_then(|a| self.hit(a));
        }
        let mut add_at: Option<[f32; 2]> = None;
        let menu_at = resp.interact_pointer_pos().map(|p| to_canvas(p, self.pan, self.zoom));
        resp.context_menu(|ui| {
            if let Some(a) = menu_at {
                if ui.button("New note here").clicked() {
                    add_at = Some(a);
                    ui.close();
                }
                if ui.button("Fit the whole board").clicked() {
                    self.fit_next = true;
                    ui.close();
                }
            }
        });
        if let Some(a) = add_at {
            self.add_note(a, "text");
        }

        // ---- drawing: groups, arrows, notes
        let (pan, zoom) = (self.pan, self.zoom);
        let origin = rect.min;
        let at = |x: f32, y: f32| origin + pan + Vec2::new(x, y) * zoom;
        let screen = |n: &Node| Rect::from_min_size(at(n.x, n.y), Vec2::new(n.w, n.h) * zoom);
        let on_screen = |r: Rect| r.intersects(rect);
        let selected_id = self.selected.and_then(|i| self.doc.nodes.get(i)).map(|n| n.id.clone());
        for n in self.doc.nodes.iter().filter(|n| n.kind == "group") {
            let r = screen(n);
            if !on_screen(r) {
                continue;
            }
            let c = tint(&n.color);
            painter.rect_filled(r, CornerRadius::same((10.0 * zoom) as u8), c.gamma_multiply(0.08));
            let selected = selected_id.as_deref() == Some(n.id.as_str());
            painter.rect_stroke(r, CornerRadius::same((10.0 * zoom) as u8), Stroke::new(if selected { 2.5f32 } else { 1.5f32 } * zoom.clamp(0.6, 1.4), if selected { Color32::WHITE } else { c.gamma_multiply(0.8) }), StrokeKind::Inside);
            // a group's name stays readable when the board is far away
            let sz = (15.0 * zoom).max(11.5);
            painter.with_clip_rect(r.intersect(rect)).text(r.min + Vec2::new(12.0 * zoom.max(0.5), 8.0 * zoom.max(0.5)), Align2::LEFT_TOP, unlink(&n.body), FontId::proportional(sz), c);
        }
        for e in &self.doc.edges {
            let (Some(a), Some(b)) = (self.doc.nodes.iter().find(|n| n.id == e.from), self.doc.nodes.iter().find(|n| n.id == e.to)) else { continue };
            let sa = e.from_side.clone().unwrap_or_else(|| facing(a, b).to_string());
            let sb = e.to_side.clone().unwrap_or_else(|| facing(b, a).to_string());
            let ((pa, da), (pb, db)) = (anchor(a, &sa), anchor(b, &sb));
            let (p1, p2) = (at(pa[0], pa[1]), at(pb[0], pb[1]));
            let reach = ((p2 - p1).length() * 0.4).clamp(30.0 * zoom, 400.0 * zoom);
            let (c1, c2) = (p1 + Vec2::new(da[0], da[1]) * reach, p2 + Vec2::new(db[0], db[1]) * reach);
            let hot = selected_id.as_deref().is_some_and(|id| id == e.from || id == e.to);
            let col = if e.color.is_some() { tint(&e.color) } else if hot { theme::BLUE } else { Color32::from_rgb(150, 150, 160) };
            let width = (if hot { 2.6 } else { 1.8 } * zoom).max(1.0);
            let bbox = Rect::from_points(&[p1, p2, c1, c2]);
            if !on_screen(bbox) {
                continue;
            }
            painter.add(Shape::CubicBezier(CubicBezierShape::from_points_stroke([p1, c1, c2, p2], false, Color32::TRANSPARENT, Stroke::new(width, col))));
            // the arrow head at the end, pointing into the note
            let dir = Vec2::new(-db[0], -db[1]);
            let tip = p2;
            let side = Vec2::new(-dir.y, dir.x);
            let s = (9.0 * zoom).max(3.0);
            painter.add(Shape::convex_polygon(vec![tip, tip - dir * s * 1.6 + side * s * 0.7, tip - dir * s * 1.6 - side * s * 0.7], col, Stroke::NONE));
            if !e.label.is_empty() && zoom > 0.3 {
                let mid = Pos2::new((p1.x + 3.0 * c1.x + 3.0 * c2.x + p2.x) / 8.0, (p1.y + 3.0 * c1.y + 3.0 * c2.y + p2.y) / 8.0);
                let g = painter.layout_no_wrap(e.label.clone(), FontId::proportional((11.5 * zoom).max(8.0)), theme::TEXT_BRIGHT);
                let r = Rect::from_center_size(mid, g.size() + Vec2::new(10.0, 4.0));
                painter.rect_filled(r, CornerRadius::same(4), Color32::from_rgb(30, 30, 32));
                painter.rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, col.gamma_multiply(0.7)), StrokeKind::Inside);
                painter.galley(r.center() - g.size() / 2.0, g, theme::TEXT_BRIGHT);
            }
        }
        for n in self.doc.nodes.iter().filter(|n| n.kind != "group") {
            let r = screen(n);
            if !on_screen(r) {
                continue;
            }
            let c = tint(&n.color);
            let round = CornerRadius::same((6.0 * zoom).clamp(2.0, 8.0) as u8);
            painter.rect_filled(r, round, Color32::from_rgb(40, 40, 44));
            painter.rect_filled(r, round, c.gamma_multiply(0.16));
            let selected = selected_id.as_deref() == Some(n.id.as_str());
            painter.rect_stroke(r, round, Stroke::new(if selected { 2.5 } else { 1.4 }, if selected { Color32::WHITE } else { c.gamma_multiply(0.9) }), StrokeKind::Inside);
            let size = 13.0 * zoom;
            if size >= 7.0 {
                let text = match n.kind.as_str() {
                    "file" => format!("**{}**\n{}", n.body.rsplit('/').next().unwrap_or(&n.body), n.body),
                    "link" => format!("`{}`", n.body),
                    _ => n.body.clone(),
                };
                let pad = 9.0 * zoom;
                let job = md_job(&text, size, r.width() - 2.0 * pad, theme::TEXT);
                let galley = painter.layout_job(job);
                painter.with_clip_rect(r.shrink(1.0).intersect(rect)).galley(r.min + Vec2::splat(pad), galley, theme::TEXT);
            } else {
                // far away: the first line of the note, as small as is still readable, so that the shape of the board still says what is where
                let title = unlink(n.body.lines().find(|l| !l.trim().is_empty()).unwrap_or("")).replace(['#', '*', '`'], "");
                let g = painter.layout(title.trim().to_string(), FontId::proportional(10.5), theme::TEXT, (r.width() - 8.0).max(8.0));
                painter.with_clip_rect(r.shrink(1.0).intersect(rect)).galley(r.min + Vec2::new(4.0, 3.0), g, theme::TEXT);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"{"nodes":[{"id":"a","type":"text","text":"# Title\n- one\n**bold** and `code` [[Note|shown]]","x":0,"y":0,"width":300,"height":120,"color":"4"},
        {"id":"g","type":"group","label":"Group","x":-40,"y":-40,"width":500,"height":300,"color":"#aa3366"},
        {"id":"b","type":"text","text":"B","x":400,"y":0,"width":100,"height":60,"zoom":3}],
        "edges":[{"id":"e","fromNode":"a","toNode":"b","label":"goes to","fromSide":"right","toSide":"left"}],"keep":true}"##;

    #[test]
    fn a_canvas_file_is_read_and_written_back_with_everything_it_had() {
        let doc = parse(SAMPLE).unwrap();
        assert_eq!((doc.nodes.len(), doc.edges.len()), (3, 1));
        assert_eq!(doc.nodes[0].color.as_deref(), Some("4"));
        assert_eq!(doc.nodes[1].kind, "group");
        assert_eq!(doc.nodes[1].body, "Group");
        let out = to_json(&doc);
        let again = parse(&out.to_string_compact().unwrap()).unwrap();
        assert_eq!(again.nodes.len(), 3);
        assert_eq!(again.edges[0].label, "goes to");
        assert_eq!(again.edges[0].from_side.as_deref(), Some("right"));
        // an unknown field of a node and of the file survive
        assert!(again.nodes[2].extra.iter().any(|(k, _)| k == "zoom"));
        assert!(again.extra.iter().any(|(k, _)| k == "keep"));
    }

    #[test]
    fn links_headings_and_bold_become_text_runs() {
        assert_eq!(unlink("see [[Note|shown]] and [[plain]] end"), "see shown and plain end");
        let job = md_job("# Title\n- one\n**bold** and `code`", 13.0, 300.0, Color32::GRAY);
        let text: String = job.sections.iter().map(|s| &job.text[s.byte_range.start.0..s.byte_range.end.0]).collect();
        assert_eq!(text, "Title\n• one\nbold and code");
        assert!(job.sections.iter().any(|s| s.format.font_id.size > 13.0), "the heading is bigger");
    }

    #[test]
    fn colours_are_the_six_presets_or_a_hex_value() {
        assert_eq!(tint(&Some("1".into())), Color32::from_rgb(232, 84, 84));
        assert_eq!(tint(&Some("#10ff20".into())), Color32::from_rgb(16, 255, 32));
        assert_eq!(tint(&None), Color32::from_rgb(136, 140, 150));
    }

    #[test]
    fn an_edge_leaves_the_side_that_faces_the_other_note() {
        let doc = parse(SAMPLE).unwrap();
        let (a, b) = (&doc.nodes[0], &doc.nodes[2]);
        assert_eq!(facing(a, b), "right");
        assert_eq!(facing(b, a), "left");
        assert_eq!(anchor(a, "right").0, [300.0, 60.0]);
    }

    #[test]
    fn opening_a_missing_file_says_so_and_keeps_the_board() {
        let mut c = CanvasUi::default();
        c.open("no/such/file.canvas");
        assert!(c.message.as_ref().is_some_and(|m| m.1), "{:?}", c.message);
        assert!(c.path.is_none());
    }
}
