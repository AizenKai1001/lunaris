//! The UI tab: a designer for on-screen interfaces (`docs/UI.md`) in the way of Unreal's UMG Widget Blueprint designer. A **Palette** of
//! widgets to drag onto the screen, the **Hierarchy** of what is on it, the **Designer** (the screen as a frame, widgets you move and
//! resize by their handles) and **Details** (position and size in pixels, colours, text, behaviour). The widgets are the `Ui` entities of
//! the scene, so every change is a tool call (`spawn`, `set`, `despawn`): undo, recordings and an AI agent in the same project all see it.
use crate::state::Studio;
use eframe::egui::{self, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Id, Key, Modifiers, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui, Vec2};
use lunaris_json::Json;

const BG: Color32 = Color32::from_rgb(21, 21, 22);
const FRAME_BG: Color32 = Color32::from_rgb(11, 11, 12);
const SELECT: Color32 = Color32::from_rgb(255, 168, 28);
const TEXT: Color32 = Color32::from_rgb(222, 222, 222);
const DIM: Color32 = Color32::from_rgb(150, 150, 150);
const PANEL_HEAD: Color32 = Color32::from_rgb(48, 48, 50);

/// The screens a designer can show: (name, width, height).
const SCREENS: &[(&str, f32, f32)] = &[("1920 x 1080 (16:9)", 1920.0, 1080.0), ("1280 x 720 (16:9)", 1280.0, 720.0), ("2560 x 1440 (16:9)", 2560.0, 1440.0), ("1280 x 800 (16:10)", 1280.0, 800.0), ("1024 x 768 (4:3)", 1024.0, 768.0), ("1080 x 1920 (9:16, phone)", 1080.0, 1920.0)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Panel,
    Text,
    Button,
    Bar,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Panel => "Border",
            Kind::Text => "Text Block",
            Kind::Button => "Button",
            Kind::Bar => "Progress Bar",
        }
    }
}

/// What the palette offers: (category, kind, hint).
const PALETTE: &[(&str, Kind, &str)] = &[
    ("Common", Kind::Button, "A button: it reacts to the mouse and sets Ui.clicked for a tick"),
    ("Common", Kind::Text, "A line of text"),
    ("Common", Kind::Bar, "A bar that is filled from the left: set Ui.fill from a graph system"),
    ("Common", Kind::Panel, "A filled rectangle with an edge: a panel or a background"),
];

#[derive(Clone, Debug)]
struct Widget {
    uid: u64,
    name: String,
    pos: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
    back: [f32; 4],
    fill: f32,
    border: f32,
    border_color: [f32; 4],
    text: String,
    text_size: f32,
    text_color: [f32; 4],
    layer: u32,
    visible: bool,
    button: bool,
}

impl Widget {
    fn kind(&self) -> Kind {
        if self.button {
            Kind::Button
        } else if self.fill < 1.0 {
            Kind::Bar
        } else if !self.text.is_empty() && self.color[3] < 0.02 && self.border <= 0.0 {
            Kind::Text
        } else {
            Kind::Panel
        }
    }
}

fn arr<const N: usize>(j: Option<&Json>, d: [f32; N]) -> [f32; N] {
    let mut out = d;
    if let Some(a) = j.and_then(|a| a.as_array()) {
        for (i, v) in a.iter().take(N).enumerate() {
            out[i] = v.as_f64().unwrap_or(d[i] as f64) as f32;
        }
    }
    out
}

fn jarr(v: &[f32]) -> Json {
    Json::arr(v.iter().map(|x| Json::Float(*x as f64)))
}

#[derive(Clone, Debug, PartialEq)]
enum Drag {
    Move { uid: u64, start: [f32; 2], orig: [f32; 2] },
    Resize { uid: u64, handle: usize, start: [f32; 2], orig_pos: [f32; 2], orig_size: [f32; 2] },
    Pan,
}

#[derive(Default)]
pub struct UiDesigner {
    widgets: Vec<Widget>,
    stamp: Option<u64>,
    screen: usize,
    zoom: f32,
    pan: Vec2,
    fit: bool,
    grid: bool,
    snap: bool,
    drag: Option<Drag>,
    group: Option<String>,
    rename: Option<(u64, String, bool)>,
    palette_drag: Option<Kind>,
    canvas: Option<Rect>,
    frame: Option<Rect>,
    inited: bool,
    ctx_target: Option<u64>,
    search: String,
    detail_group: Option<String>,
}

impl UiDesigner {
    fn init(&mut self) {
        if !self.inited {
            self.inited = true;
            self.zoom = 1.0;
            self.fit = true;
            self.grid = true;
            self.snap = true;
        }
    }

    fn size(&self) -> [f32; 2] {
        let s = SCREENS[self.screen.min(SCREENS.len() - 1)];
        [s.1, s.2]
    }

    /// Read the widgets of the world (again, when something changed).
    fn load(&mut self, studio: &mut Studio) {
        let stamp = studio.live_revision;
        if self.stamp == Some(stamp) {
            return;
        }
        self.stamp = Some(stamp);
        let Ok(r) = studio.engine.call("query", &Json::obj([("with", Json::arr([Json::str("Ui")])), ("components", Json::arr([Json::str("Name"), Json::str("Ui")])), ("limit", Json::Int(500))])) else { return };
        let mut out = Vec::new();
        for e in r.get("entities").and_then(|e| e.as_array()).unwrap_or(&[]) {
            let (Some(uid), Some(c)) = (e.get("uid").and_then(|u| u.as_u64()), e.get("components")) else { continue };
            let Some(u) = c.get("Ui") else { continue };
            out.push(Widget {
                uid,
                name: c.get("Name").and_then(|n| n.get("value")).and_then(|v| v.as_str()).unwrap_or("Widget").to_string(),
                pos: arr(u.get("pos"), [0.4, 0.45]),
                size: arr(u.get("size"), [0.2, 0.1]),
                color: arr(u.get("color"), [0.16, 0.2, 0.32, 0.92]),
                back: arr(u.get("back_color"), [0.0; 4]),
                fill: u.get("fill").and_then(|f| f.as_f64()).unwrap_or(1.0) as f32,
                border: u.get("border").and_then(|f| f.as_f64()).unwrap_or(0.0) as f32,
                border_color: arr(u.get("border_color"), [1.0; 4]),
                text: u.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                text_size: u.get("text_size").and_then(|f| f.as_f64()).unwrap_or(0.04) as f32,
                text_color: arr(u.get("text_color"), [1.0; 4]),
                layer: u.get("layer").and_then(|l| l.as_u64()).unwrap_or(0) as u32,
                visible: u.get("visible").and_then(|b| b.as_bool()).unwrap_or(true),
                button: u.get("button").and_then(|b| b.as_bool()).unwrap_or(false),
            });
        }
        out.sort_by_key(|w| (w.layer, w.uid));
        self.widgets = out;
    }

    fn set(&mut self, studio: &mut Studio, uid: u64, fields: Vec<(&str, Json)>, group: Option<&str>) {
        studio.set(uid, "Ui", Json::obj(fields.into_iter().map(|(k, v)| (k.to_string(), v))), group);
        // the picture follows at once, not at the next frame
        self.stamp = None;
    }

    fn add(&mut self, studio: &mut Studio, kind: Kind, at: Option<[f32; 2]>) {
        let (name, ui): (&str, Json) = match kind {
            Kind::Button => ("Button", Json::obj([("pos", jarr(&[0.4, 0.45])), ("size", jarr(&[0.18, 0.08])), ("color", jarr(&[0.2, 0.3, 0.55, 0.95])), ("border", Json::Float(0.003)), ("border_color", jarr(&[0.8, 0.9, 1.0, 1.0])), ("text", Json::str("Button")), ("text_size", Json::Float(0.04)), ("button", Json::Bool(true)), ("layer", Json::Int(2))])),
            Kind::Text => ("Text Block", Json::obj([("pos", jarr(&[0.4, 0.45])), ("size", jarr(&[0.2, 0.06])), ("color", jarr(&[0.0, 0.0, 0.0, 0.0])), ("text", Json::str("Text Block")), ("text_size", Json::Float(0.04)), ("layer", Json::Int(2))])),
            Kind::Bar => ("Progress Bar", Json::obj([("pos", jarr(&[0.35, 0.45])), ("size", jarr(&[0.3, 0.04])), ("color", jarr(&[0.2, 0.8, 0.3, 1.0])), ("back_color", jarr(&[0.05, 0.05, 0.08, 0.8])), ("fill", Json::Float(0.6)), ("border", Json::Float(0.003)), ("border_color", jarr(&[1.0, 1.0, 1.0, 0.9])), ("layer", Json::Int(1))])),
            Kind::Panel => ("Border", Json::obj([("pos", jarr(&[0.35, 0.35])), ("size", jarr(&[0.3, 0.3])), ("color", jarr(&[0.08, 0.1, 0.2, 0.85])), ("border", Json::Float(0.004)), ("border_color", jarr(&[0.6, 0.75, 1.0, 1.0])), ("layer", Json::Int(0))])),
        };
        let mut ui = ui;
        if let Some(c) = at {
            let size = ui.get("size").and_then(|s| s.as_array()).map(|a| [a[0].as_f64().unwrap_or(0.2) as f32, a[1].as_f64().unwrap_or(0.1) as f32]).unwrap_or([0.2, 0.1]);
            let p = [(c[0] - size[0] / 2.0).clamp(0.0, 1.0 - size[0]), (c[1] - size[1] / 2.0).clamp(0.0, 1.0 - size[1])];
            ui.set("pos", jarr(&p));
        }
        let name = studio.unique_name(name);
        let comps = Json::obj([("Name", Json::obj([("value", Json::str(name))])), ("Ui", ui)]);
        if let Some(r) = studio.try_call("spawn", Json::obj([("components", comps)])) {
            if let Some(uid) = r.get("uid").and_then(|u| u.as_u64()) {
                studio.rows_dirty = true;
                studio.refresh_rows();
                studio.select(Some(uid));
            }
        }
        self.stamp = None;
    }

    // ------------------------------------------------------------------------------------------------ the middle

    pub fn show(&mut self, ui: &mut Ui, studio: &mut Studio) -> Option<crate::center::CenterTab> {
        self.init();
        self.load(studio);
        let mut go: Option<crate::center::CenterTab> = None;
        self.toolbar(ui, studio, &mut go);
        let rect = ui.available_rect_before_wrap();
        self.canvas = Some(rect);
        self.designer(ui, rect, studio);
        ui.allocate_rect(rect, Sense::hover());
        go
    }

    fn toolbar(&mut self, ui: &mut Ui, studio: &mut Studio, go: &mut Option<crate::center::CenterTab>) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(36, 36, 37));
        p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, Color32::from_rgb(14, 14, 14)));
        // the two faces of a widget blueprint: Designer and Graph
        let mut x = rect.max.x - 8.0;
        for (label, active) in [("Graph", false), ("Designer", true)] {
            let g = p.layout_no_wrap(label.to_string(), FontId::proportional(13.0), TEXT);
            let w = g.size().x + 26.0;
            let r = Rect::from_min_size(Pos2::new(x - w, rect.min.y + 5.0), Vec2::new(w, 24.0));
            let resp = ui.interact(r, Id::new(("ui-face", label)), Sense::click());
            p.rect_filled(r, CornerRadius::same(12), if active { Color32::from_rgb(70, 70, 72) } else if resp.hovered() { Color32::from_rgb(52, 52, 54) } else { Color32::TRANSPARENT });
            p.text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(13.0), if active { Color32::WHITE } else { DIM });
            if resp.clicked() && !active {
                *go = Some(crate::center::CenterTab::Blueprint);
            }
            x -= w + 4.0;
        }
        // tools at the left
        let mut at = rect.min.x + 10.0;
        let btn = |ui: &mut Ui, label: &str, on: bool, at: &mut f32, id: &str| -> bool {
            let g = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(12.5), TEXT);
            let w = g.size().x + 20.0;
            let r = Rect::from_min_size(Pos2::new(*at, rect.min.y + 4.0), Vec2::new(w, 26.0));
            let resp = ui.interact(r, Id::new(("ui-tool", id)), Sense::click());
            if on || resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(3), if on { Color32::from_rgb(66, 66, 70) } else { Color32::from_rgb(52, 52, 54) });
            }
            ui.painter().text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(12.5), TEXT);
            *at += w + 4.0;
            resp.clicked()
        };
        if btn(ui, "Save", false, &mut at, "save") {
            let _ = studio.try_call("save_scene", Json::obj([("path", Json::str(studio.scene.clone().unwrap_or_else(|| "scenes/scene.lunaris.json".to_string())))]));
            studio.toast("Scene saved", false);
        }
        at += 6.0;
        if btn(ui, "Zoom to Fit", false, &mut at, "fit") {
            self.fit = true;
        }
        if btn(ui, "Grid", self.grid, &mut at, "grid") {
            self.grid = !self.grid;
        }
        if btn(ui, "Snap", self.snap, &mut at, "snap") {
            self.snap = !self.snap;
        }
        at += 6.0;
        // the screen size menu
        let name = SCREENS[self.screen.min(SCREENS.len() - 1)].0;
        let r = Rect::from_min_size(Pos2::new(at, rect.min.y + 4.0), Vec2::new(210.0, 26.0));
        let resp = ui.interact(r, Id::new("ui-screen"), Sense::click());
        ui.painter().rect_filled(r, CornerRadius::same(3), if resp.hovered() { Color32::from_rgb(52, 52, 54) } else { Color32::from_rgb(44, 44, 46) });
        ui.painter().text(r.min + Vec2::new(10.0, 13.0), Align2::LEFT_CENTER, format!("Screen Size: {name}"), FontId::proportional(12.0), TEXT);
        egui::Popup::menu(&resp).id(Id::new("ui-screen-menu")).show(|ui| {
            for (i, s) in SCREENS.iter().enumerate() {
                if ui.selectable_label(self.screen == i, s.0).clicked() {
                    self.screen = i;
                    self.fit = true;
                    ui.close();
                }
            }
        });
    }

    fn frame_rect(&self, canvas: Rect) -> Rect {
        let [w, h] = self.size();
        let fit_k = ((canvas.width() - 80.0) / w).min((canvas.height() - 80.0) / h).max(0.05);
        let k = fit_k * self.zoom;
        let size = Vec2::new(w, h) * k;
        Rect::from_center_size(canvas.center() + self.pan, size)
    }

    fn designer(&mut self, ui: &mut Ui, canvas: Rect, studio: &mut Studio) {
        let ctx = ui.ctx().clone();
        let resp = ui.interact(canvas, Id::new("ui-canvas"), Sense::click_and_drag());
        let painter = ui.painter_at(canvas);
        painter.rect_filled(canvas, CornerRadius::ZERO, BG);
        if self.fit {
            self.zoom = 1.0;
            self.pan = Vec2::ZERO;
            self.fit = false;
        }
        let ptr = ctx.input(|i| i.pointer.hover_pos());
        let (scroll, mods, delta, primary_pressed, primary_down, primary_released, secondary_down) = ctx.input(|i| (i.smooth_scroll_delta.y, i.modifiers, i.pointer.delta(), i.pointer.button_pressed(egui::PointerButton::Primary), i.pointer.primary_down(), i.pointer.button_released(egui::PointerButton::Primary), i.pointer.secondary_down()));
        let hovered = resp.hovered() && self.rename.is_none();
        // zoom around the pointer
        if hovered && scroll != 0.0 {
            if let Some(p) = ptr {
                let before = self.frame_rect(canvas);
                let k = (self.zoom * (1.0 + scroll * 0.0016)).clamp(0.25, 6.0);
                let rel = (p - before.min) / before.size();
                self.zoom = k;
                let after = self.frame_rect(canvas);
                // keep the point under the pointer where it was
                let want = p - Vec2::new(rel.x * after.width(), rel.y * after.height());
                self.pan += want - after.min;
            }
        }
        let frame = self.frame_rect(canvas);
        self.frame = Some(frame);
        let [rw, rh] = self.size();
        let k = frame.width() / rw;
        // ---- the screen
        painter.rect_filled(frame.expand(1.0), CornerRadius::ZERO, Color32::from_rgb(70, 70, 74));
        painter.rect_filled(frame, CornerRadius::ZERO, FRAME_BG);
        if self.grid {
            let step = 40.0 * k;
            if step > 6.0 {
                let mut x = frame.min.x;
                while x <= frame.max.x {
                    painter.line_segment([Pos2::new(x, frame.min.y), Pos2::new(x, frame.max.y)], Stroke::new(1.0, Color32::from_rgb(20, 20, 22)));
                    x += step;
                }
                let mut y = frame.min.y;
                while y <= frame.max.y {
                    painter.line_segment([Pos2::new(frame.min.x, y), Pos2::new(frame.max.x, y)], Stroke::new(1.0, Color32::from_rgb(20, 20, 22)));
                    y += step;
                }
            }
        }
        painter.text(frame.right_top() + Vec2::new(0.0, -8.0), Align2::RIGHT_BOTTOM, SCREENS[self.screen.min(SCREENS.len() - 1)].0, FontId::proportional(12.0), DIM);
        painter.text(frame.left_bottom() + Vec2::new(0.0, 8.0), Align2::LEFT_TOP, format!("Zoom {:.0}%", k * 100.0), FontId::proportional(12.0), DIM);
        // ---- the widgets, back to front
        let clip = painter.with_clip_rect(canvas.intersect(frame.expand(2.0)));
        let wr = |w: &Widget| Rect::from_min_size(frame.min + Vec2::new(w.pos[0] * frame.width(), w.pos[1] * frame.height()), Vec2::new(w.size[0] * frame.width(), w.size[1] * frame.height()));
        for w in &self.widgets {
            draw_widget(&clip, w, wr(w), frame, k);
        }
        // ---- selection
        let sel = studio.selection.filter(|u| self.widgets.iter().any(|w| w.uid == *u));
        let mut handles: Vec<(usize, Pos2)> = Vec::new();
        if let Some(uid) = sel {
            if let Some(w) = self.widgets.iter().find(|w| w.uid == uid) {
                let r = wr(w);
                painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(2.0, SELECT), StrokeKind::Outside);
                let pts = [r.left_top(), r.center_top(), r.right_top(), r.right_center(), r.right_bottom(), r.center_bottom(), r.left_bottom(), r.left_center()];
                for (i, p) in pts.iter().enumerate() {
                    handles.push((i, *p));
                    let hr = Rect::from_center_size(*p, Vec2::splat(9.0));
                    painter.rect_filled(hr, CornerRadius::same(1), Color32::WHITE);
                    painter.rect_stroke(hr, CornerRadius::same(1), Stroke::new(1.0, Color32::from_rgb(30, 30, 30)), StrokeKind::Outside);
                }
                // the name over the box, like the designer shows it
                painter.text(r.left_top() + Vec2::new(0.0, -6.0), Align2::LEFT_BOTTOM, format!("{}  [{}]", w.name, w.kind().label()), FontId::proportional(11.5), SELECT);
            }
        }
        // ---- input
        let grab_handle = |p: Pos2| handles.iter().find(|(_, h)| h.distance(p) < 8.0).map(|(i, _)| *i);
        let topmost = |p: Pos2, ws: &[Widget]| ws.iter().rev().find(|w| w.visible && wr(w).contains(p)).map(|w| w.uid);
        if hovered && secondary_down && self.drag.is_none() {
            self.drag = Some(Drag::Pan);
        }
        if hovered && primary_pressed && self.drag.is_none() {
            if let Some(p) = ptr {
                if let (Some(h), Some(uid)) = (grab_handle(p), sel) {
                    if let Some(w) = self.widgets.iter().find(|w| w.uid == uid) {
                        self.group = Some(studio.next_group());
                        self.drag = Some(Drag::Resize { uid, handle: h, start: [p.x, p.y], orig_pos: w.pos, orig_size: w.size });
                    }
                } else if let Some(uid) = topmost(p, &self.widgets) {
                    studio.select(Some(uid));
                    if let Some(w) = self.widgets.iter().find(|w| w.uid == uid) {
                        self.group = Some(studio.next_group());
                        self.drag = Some(Drag::Move { uid, start: [p.x, p.y], orig: w.pos });
                    }
                } else if frame.contains(p) || canvas.contains(p) {
                    studio.select(None);
                }
            }
        }
        match self.drag.clone() {
            Some(Drag::Pan) => {
                self.pan += delta;
                if !secondary_down {
                    self.drag = None;
                }
            }
            Some(Drag::Move { uid, start, orig }) => {
                if let Some(p) = ptr {
                    let (dx, dy) = ((p.x - start[0]) / frame.width(), (p.y - start[1]) / frame.height());
                    let mut np = [orig[0] + dx, orig[1] + dy];
                    if self.snap && !mods.alt {
                        np = [snap(np[0], rw), snap(np[1], rh)];
                    }
                    let size = self.widgets.iter().find(|w| w.uid == uid).map(|w| w.size).unwrap_or([0.1, 0.1]);
                    np = [np[0].clamp(-size[0] + 0.02, 1.0 - 0.02), np[1].clamp(-size[1] + 0.02, 1.0 - 0.02)];
                    if (np[0] - orig[0]).abs() + (np[1] - orig[1]).abs() > 1e-5 {
                        let g = self.group.clone();
                        self.set(studio, uid, vec![("pos", jarr(&np))], g.as_deref());
                    }
                }
                if primary_released || !primary_down {
                    self.drag = None;
                    self.group = None;
                }
            }
            Some(Drag::Resize { uid, handle, start, orig_pos, orig_size }) => {
                if let Some(p) = ptr {
                    let (dx, dy) = ((p.x - start[0]) / frame.width(), (p.y - start[1]) / frame.height());
                    let (mut x0, mut y0, mut x1, mut y1) = (orig_pos[0], orig_pos[1], orig_pos[0] + orig_size[0], orig_pos[1] + orig_size[1]);
                    // handles: 0 top-left, 1 top, 2 top-right, 3 right, 4 bottom-right, 5 bottom, 6 bottom-left, 7 left
                    if matches!(handle, 0 | 6 | 7) {
                        x0 += dx;
                    }
                    if matches!(handle, 2 | 3 | 4) {
                        x1 += dx;
                    }
                    if matches!(handle, 0 | 1 | 2) {
                        y0 += dy;
                    }
                    if matches!(handle, 4 | 5 | 6) {
                        y1 += dy;
                    }
                    if self.snap && !mods.alt {
                        x0 = snap(x0, rw);
                        x1 = snap(x1, rw);
                        y0 = snap(y0, rh);
                        y1 = snap(y1, rh);
                    }
                    let (lo, hi) = (4.0 / rw, 4.0 / rh);
                    if x1 - x0 < lo {
                        if matches!(handle, 0 | 6 | 7) { x0 = x1 - lo } else { x1 = x0 + lo }
                    }
                    if y1 - y0 < hi {
                        if matches!(handle, 0 | 1 | 2) { y0 = y1 - hi } else { y1 = y0 + hi }
                    }
                    let g = self.group.clone();
                    self.set(studio, uid, vec![("pos", jarr(&[x0, y0])), ("size", jarr(&[x1 - x0, y1 - y0]))], g.as_deref());
                }
                if primary_released || !primary_down {
                    self.drag = None;
                    self.group = None;
                }
            }
            None => {}
        }
        // a widget being dragged from the palette
        if let Some(kind) = self.palette_drag {
            if let Some(p) = ptr {
                let ghost = Rect::from_center_size(p, Vec2::new(0.18 * frame.width(), 0.07 * frame.height()));
                let fp = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::new("ui-ghost")));
                fp.rect_filled(ghost, CornerRadius::same(3), Color32::from_rgba_unmultiplied(255, 168, 28, 60));
                fp.rect_stroke(ghost, CornerRadius::same(3), Stroke::new(1.5, SELECT), StrokeKind::Inside);
                fp.text(ghost.center(), Align2::CENTER_CENTER, kind.label(), FontId::proportional(12.0), Color32::WHITE);
                if !primary_down {
                    if frame.contains(p) {
                        let at = [(p.x - frame.min.x) / frame.width(), (p.y - frame.min.y) / frame.height()];
                        self.add(studio, kind, Some(at));
                    }
                    self.palette_drag = None;
                }
            } else if !primary_down {
                self.palette_drag = None;
            }
        }
        // ---- keys
        // keys belong to the designer only while nothing else has the keyboard (typing in Details must not delete the widget)
        if hovered && ctx.memory(|m| m.focused().is_none()) {
            let step_px = if mods.shift { 8.0 } else { 1.0 };
            let (l, r, u, d, del, dup, undo, redo, esc) = ctx.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::ArrowLeft) || i.consume_key(Modifiers::SHIFT, Key::ArrowLeft),
                    i.consume_key(Modifiers::NONE, Key::ArrowRight) || i.consume_key(Modifiers::SHIFT, Key::ArrowRight),
                    i.consume_key(Modifiers::NONE, Key::ArrowUp) || i.consume_key(Modifiers::SHIFT, Key::ArrowUp),
                    i.consume_key(Modifiers::NONE, Key::ArrowDown) || i.consume_key(Modifiers::SHIFT, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::Delete),
                    i.consume_key(Modifiers::COMMAND, Key::D),
                    i.consume_key(Modifiers::COMMAND, Key::Z),
                    i.consume_key(Modifiers::COMMAND, Key::Y) || i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
                    i.consume_key(Modifiers::NONE, Key::Escape),
                )
            });
            if let Some(uid) = sel {
                let (dx, dy) = ((r as i32 - l as i32) as f32 * step_px / rw, (d as i32 - u as i32) as f32 * step_px / rh);
                if dx != 0.0 || dy != 0.0 {
                    if let Some(w) = self.widgets.iter().find(|w| w.uid == uid) {
                        let np = [w.pos[0] + dx, w.pos[1] + dy];
                        self.set(studio, uid, vec![("pos", jarr(&np))], None);
                    }
                }
                if del {
                    studio.delete(uid);
                    self.stamp = None;
                }
                if dup {
                    studio.duplicate(uid);
                    // the copy sits a bit to the side and below
                    if let Some(new) = studio.selection {
                        if let Some(w) = self.widgets.iter().find(|w| w.uid == uid).cloned() {
                            self.set(studio, new, vec![("pos", jarr(&[(w.pos[0] + 12.0 / rw).min(0.95), (w.pos[1] + 12.0 / rh).min(0.95)]))], None);
                        }
                    }
                    self.stamp = None;
                }
            }
            if undo {
                studio.undo();
                self.stamp = None;
            }
            if redo {
                studio.redo();
                self.stamp = None;
            }
            if esc {
                studio.select(None);
            }
        }
        // ---- the menu of a right click (without a drag)
        let secondary_clicked = resp.secondary_clicked();
        if secondary_clicked {
            self.ctx_target = ptr.and_then(|p| topmost(p, &self.widgets));
            if let Some(u) = self.ctx_target {
                studio.select(Some(u));
            }
        }
        let target = self.ctx_target;
        let mut act: Option<&str> = None;
        resp.context_menu(|ui| {
            ui.set_min_width(180.0);
            if let Some(uid) = target {
                let name = self.widgets.iter().find(|w| w.uid == uid).map(|w| w.name.clone()).unwrap_or_default();
                ui.label(RichText::new(name).size(11.5).color(DIM));
                for (label, id) in [("Duplicate", "dup"), ("Delete", "del"), ("Bring to Front", "front"), ("Send to Back", "back")] {
                    if ui.button(label).clicked() {
                        act = Some(id);
                        ui.close();
                    }
                }
            } else {
                ui.label(RichText::new("Add Widget").size(11.5).color(DIM));
                for (_, kind, _) in PALETTE {
                    if ui.button(kind.label()).clicked() {
                        act = Some(match kind {
                            Kind::Button => "add_button",
                            Kind::Text => "add_text",
                            Kind::Bar => "add_bar",
                            Kind::Panel => "add_panel",
                        });
                        ui.close();
                    }
                }
            }
        });
        if let Some(a) = act {
            let at = ptr.map(|p| [(p.x - frame.min.x) / frame.width(), (p.y - frame.min.y) / frame.height()]);
            match (a, target) {
                ("dup", Some(u)) => {
                    studio.duplicate(u);
                    self.stamp = None;
                }
                ("del", Some(u)) => {
                    studio.delete(u);
                    self.stamp = None;
                }
                ("front", Some(u)) => {
                    let top = self.widgets.iter().map(|w| w.layer).max().unwrap_or(0) + 1;
                    self.set(studio, u, vec![("layer", Json::Int(top as i64))], None);
                }
                ("back", Some(u)) => {
                    let low = self.widgets.iter().map(|w| w.layer).min().unwrap_or(0);
                    if low > 0 {
                        self.set(studio, u, vec![("layer", Json::Int(low as i64 - 1))], None);
                    } else {
                        // everything else moves up one
                        for w in self.widgets.clone() {
                            if w.uid != u {
                                self.set(studio, w.uid, vec![("layer", Json::Int(w.layer as i64 + 1))], None);
                            }
                        }
                    }
                }
                ("add_button", _) => self.add(studio, Kind::Button, at),
                ("add_text", _) => self.add(studio, Kind::Text, at),
                ("add_bar", _) => self.add(studio, Kind::Bar, at),
                ("add_panel", _) => self.add(studio, Kind::Panel, at),
                _ => {}
            }
        }
        if self.widgets.is_empty() {
            painter.text(frame.center(), Align2::CENTER_CENTER, "Drag a widget from the Palette onto the screen,\nor right click to add one", FontId::proportional(14.0), DIM);
        }
    }

    // ------------------------------------------------------------------------------------------------ the side panels

    /// "Palette" over "Hierarchy" (the left panel while this tab is up).
    pub fn left_panel(&mut self, ui: &mut Ui, studio: &mut Studio) {
        self.init();
        self.load(studio);
        ui.add_space(2.0);
        section(ui, "Palette", |ui| {
            ui.add(TextEdit::singleline(&mut self.search).hint_text("Search Palette").desired_width(f32::INFINITY));
            ui.add_space(4.0);
            ui.label(RichText::new("Common").size(11.5).color(DIM));
            let q = self.search.to_lowercase();
            for (_, kind, hint) in PALETTE {
                if !q.is_empty() && !kind.label().to_lowercase().contains(&q) {
                    continue;
                }
                let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click_and_drag());
                if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::same(3), Color32::from_rgb(58, 58, 60));
                }
                widget_icon(ui.painter(), Pos2::new(r.min.x + 16.0, r.center().y), *kind);
                ui.painter().text(r.min + Vec2::new(32.0, 12.0), Align2::LEFT_CENTER, kind.label(), FontId::proportional(13.0), TEXT);
                if resp.drag_started() {
                    self.palette_drag = Some(*kind);
                }
                if resp.clicked() {
                    self.add(studio, *kind, Some([0.5, 0.5]));
                }
                resp.on_hover_text(*hint);
            }
        });
        ui.add_space(6.0);
        section(ui, "Hierarchy", |ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
            ui.painter().text(r.min + Vec2::new(4.0, 11.0), Align2::LEFT_CENTER, "[Canvas Panel]", FontId::proportional(13.0), DIM);
            egui::ScrollArea::vertical().id_salt("ui-hier").auto_shrink([false, false]).show(ui, |ui| {
                let mut toggle: Option<(u64, bool)> = None;
                let mut select: Option<u64> = None;
                let mut start_rename: Option<(u64, String)> = None;
                let mut delete: Option<u64> = None;
                // front to back, like the hierarchy of a widget blueprint
                for w in self.widgets.iter().rev() {
                    let selected = studio.selection == Some(w.uid);
                    if let Some((uid, text, fresh)) = &mut self.rename {
                        if *uid == w.uid {
                            let te = ui.add(TextEdit::singleline(text).desired_width(f32::INFINITY));
                            if *fresh {
                                te.request_focus();
                                *fresh = false;
                            }
                            if te.lost_focus() {
                                let (u, t) = (*uid, text.clone());
                                studio.rename(u, &t);
                                self.stamp = None;
                                self.rename = None;
                            }
                            // (the loop continues next frame)
                            break;
                        }
                    }
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
                    if selected {
                        ui.painter().rect_filled(r, CornerRadius::ZERO, Color32::from_rgb(52, 74, 104));
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, CornerRadius::ZERO, Color32::from_rgb(50, 50, 52));
                    }
                    widget_icon(ui.painter(), Pos2::new(r.min.x + 22.0, r.center().y), w.kind());
                    ui.painter().text(r.min + Vec2::new(38.0, 11.0), Align2::LEFT_CENTER, &w.name, FontId::proportional(13.0), if w.visible { TEXT } else { DIM });
                    ui.painter().text(r.right_center() - Vec2::new(30.0, 0.0), Align2::RIGHT_CENTER, w.kind().label(), FontId::proportional(11.0), DIM);
                    // the eye
                    let eye = Rect::from_center_size(r.right_center() - Vec2::new(12.0, 0.0), Vec2::splat(16.0));
                    let eye_resp = ui.interact(eye, resp.id.with("eye"), Sense::click());
                    let ec = if w.visible { TEXT } else { Color32::from_rgb(90, 90, 90) };
                    ui.painter().add(Shape::line(vec![eye.center() + Vec2::new(-6.0, 0.0), eye.center() + Vec2::new(0.0, -3.5), eye.center() + Vec2::new(6.0, 0.0), eye.center() + Vec2::new(0.0, 3.5), eye.center() + Vec2::new(-6.0, 0.0)], Stroke::new(1.2, ec)));
                    ui.painter().circle_filled(eye.center(), 1.8, ec);
                    if eye_resp.clicked() {
                        toggle = Some((w.uid, !w.visible));
                    } else if resp.clicked() {
                        select = Some(w.uid);
                    }
                    if resp.double_clicked() {
                        start_rename = Some((w.uid, w.name.clone()));
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Rename").clicked() {
                            start_rename = Some((w.uid, w.name.clone()));
                            ui.close();
                        }
                        if ui.button("Delete").clicked() {
                            delete = Some(w.uid);
                            ui.close();
                        }
                    });
                }
                if let Some(u) = select {
                    studio.select(Some(u));
                }
                if let Some((u, v)) = toggle {
                    self.set(studio, u, vec![("visible", Json::Bool(v))], None);
                }
                if let Some((u, n)) = start_rename {
                    self.rename = Some((u, n, true));
                }
                if let Some(u) = delete {
                    studio.delete(u);
                    self.stamp = None;
                }
                if self.widgets.is_empty() {
                    ui.label(RichText::new("The scene has no widgets yet.").color(DIM).size(12.0));
                }
            });
        });
    }

    /// "Details" of the selected widget (the right panel while this tab is up).
    pub fn details(&mut self, ui: &mut Ui, studio: &mut Studio) {
        self.init();
        self.load(studio);
        let Some(uid) = studio.selection else {
            ui.add_space(20.0);
            ui.label(RichText::new("Select a widget to see its details").color(DIM));
            return;
        };
        let Some(w) = self.widgets.iter().find(|w| w.uid == uid).cloned() else {
            ui.add_space(20.0);
            ui.label(RichText::new("The selected entity is not a widget").color(DIM));
            return;
        };
        let [rw, rh] = self.size();
        let mut changes: Vec<(&str, Json)> = Vec::new();
        let mut started = false;
        let mut stopped = false;
        ui.horizontal(|ui| {
            widget_icon(ui.painter(), ui.cursor().min + Vec2::new(10.0, 12.0), w.kind());
            ui.add_space(24.0);
            let mut name = w.name.clone();
            let te = ui.add(TextEdit::singleline(&mut name).desired_width(f32::INFINITY).font(FontId::new(15.0, FontFamily::Proportional)));
            if te.changed() {
                studio.rename(uid, &name);
                self.stamp = None;
            }
        });
        ui.label(RichText::new(w.kind().label()).size(11.5).color(DIM));
        ui.add_space(4.0);
        let drag = |ui: &mut Ui, label: &str, v: f32, speed: f32, lo: f32, hi: f32, suffix: &str, started: &mut bool, stopped: &mut bool| -> Option<f32> {
            let mut x = v;
            ui.label(RichText::new(label).color(DIM).size(12.0));
            let r = ui.add(egui::DragValue::new(&mut x).speed(speed).range(lo..=hi).suffix(suffix).max_decimals(1));
            if r.drag_started() {
                *started = true;
            }
            if r.drag_stopped() {
                *stopped = true;
            }
            (r.changed() && (x - v).abs() > 1e-6).then_some(x)
        };
        section(ui, "Slot (Canvas Panel Slot)", |ui| {
            egui::Grid::new("ui-slot").num_columns(4).spacing([8.0, 5.0]).show(ui, |ui| {
                let px = w.pos[0] * rw;
                let py = w.pos[1] * rh;
                let sx = w.size[0] * rw;
                let sy = w.size[1] * rh;
                ui.label(RichText::new("Position").color(DIM).size(12.0));
                if let Some(v) = drag(ui, "X", px, 1.0, -4000.0, 8000.0, "", &mut started, &mut stopped) {
                    changes.push(("pos", jarr(&[v / rw, w.pos[1]])));
                }
                if let Some(v) = drag(ui, "Y", py, 1.0, -4000.0, 8000.0, "", &mut started, &mut stopped) {
                    changes.push(("pos", jarr(&[w.pos[0], v / rh])));
                }
                ui.end_row();
                ui.label(RichText::new("Size").color(DIM).size(12.0));
                if let Some(v) = drag(ui, "X", sx, 1.0, 1.0, 8000.0, "", &mut started, &mut stopped) {
                    changes.push(("size", jarr(&[v / rw, w.size[1]])));
                }
                if let Some(v) = drag(ui, "Y", sy, 1.0, 1.0, 8000.0, "", &mut started, &mut stopped) {
                    changes.push(("size", jarr(&[w.size[0], v / rh])));
                }
                ui.end_row();
                ui.label(RichText::new("ZOrder").color(DIM).size(12.0));
                let mut z = w.layer as i64;
                if ui.add(egui::DragValue::new(&mut z).range(0..=1000)).changed() {
                    changes.push(("layer", Json::Int(z)));
                }
                ui.end_row();
            });
        });
        section(ui, "Appearance", |ui| {
            egui::Grid::new("ui-appearance").num_columns(2).spacing([8.0, 5.0]).show(ui, |ui| {
                color_row(ui, "Color and Opacity", w.color, "color", &mut changes);
                color_row(ui, "Background Color", w.back, "back_color", &mut changes);
                ui.label(RichText::new("Fill Percent").color(DIM).size(12.0));
                let mut f = w.fill;
                let r = ui.add(egui::Slider::new(&mut f, 0.0..=1.0).show_value(true));
                if r.drag_started() {
                    started = true;
                }
                if r.drag_stopped() {
                    stopped = true;
                }
                if r.changed() {
                    changes.push(("fill", Json::Float(f as f64)));
                }
                ui.end_row();
                ui.label(RichText::new("Border Width").color(DIM).size(12.0));
                let mut bw = w.border * rh;
                let r = ui.add(egui::DragValue::new(&mut bw).speed(0.1).range(0.0..=100.0).suffix(" px").max_decimals(1));
                if r.drag_started() {
                    started = true;
                }
                if r.drag_stopped() {
                    stopped = true;
                }
                if r.changed() {
                    changes.push(("border", Json::Float((bw / rh) as f64)));
                }
                ui.end_row();
                color_row(ui, "Border Color", w.border_color, "border_color", &mut changes);
            });
        });
        section(ui, "Content", |ui| {
            egui::Grid::new("ui-content").num_columns(2).spacing([8.0, 5.0]).show(ui, |ui| {
                ui.label(RichText::new("Text").color(DIM).size(12.0));
                let mut t = w.text.clone();
                if ui.add(TextEdit::singleline(&mut t).desired_width(150.0)).changed() {
                    changes.push(("text", Json::str(t)));
                }
                ui.end_row();
                ui.label(RichText::new("Font Size").color(DIM).size(12.0));
                let mut s = w.text_size * rh;
                let r = ui.add(egui::DragValue::new(&mut s).speed(0.5).range(1.0..=400.0).suffix(" px").max_decimals(0));
                if r.drag_started() {
                    started = true;
                }
                if r.drag_stopped() {
                    stopped = true;
                }
                if r.changed() {
                    changes.push(("text_size", Json::Float((s / rh) as f64)));
                }
                ui.end_row();
                color_row(ui, "Color", w.text_color, "text_color", &mut changes);
            });
            ui.label(RichText::new("{resource} in the text shows the value of a resource.").size(11.0).color(DIM));
        });
        section(ui, "Behavior", |ui| {
            let mut vis = w.visible;
            if ui.checkbox(&mut vis, "Is Visible").changed() {
                changes.push(("visible", Json::Bool(vis)));
            }
            let mut btn = w.button;
            if ui.checkbox(&mut btn, "Is Button (reacts to the mouse, sets Ui.clicked)").changed() {
                changes.push(("button", Json::Bool(btn)));
            }
        });
        if started {
            self.detail_group = Some(studio.next_group());
        }
        if !changes.is_empty() {
            let g = self.detail_group.clone();
            // later changes of one field replace earlier ones of the same frame
            let mut seen: Vec<&str> = Vec::new();
            let mut fields: Vec<(&str, Json)> = Vec::new();
            for (k, v) in changes.into_iter().rev() {
                if !seen.contains(&k) {
                    seen.push(k);
                    fields.push((k, v));
                }
            }
            self.set(studio, uid, fields, g.as_deref());
        }
        if stopped {
            self.detail_group = None;
        }
    }
}

#[cfg(test)]
impl UiDesigner {
    pub fn test_widgets(&self) -> Vec<(u64, String, [f32; 2], [f32; 2])> {
        self.widgets.iter().map(|w| (w.uid, w.name.clone(), w.pos, w.size)).collect()
    }

    /// Where a widget is on the screen.
    pub fn test_widget_screen(&self, uid: u64) -> Option<Rect> {
        let frame = self.frame?;
        let w = self.widgets.iter().find(|w| w.uid == uid)?;
        Some(Rect::from_min_size(frame.min + Vec2::new(w.pos[0] * frame.width(), w.pos[1] * frame.height()), Vec2::new(w.size[0] * frame.width(), w.size[1] * frame.height())))
    }

    pub fn test_add(&mut self, studio: &mut Studio, kind: &str) {
        let k = match kind {
            "button" => Kind::Button,
            "text" => Kind::Text,
            "bar" => Kind::Bar,
            _ => Kind::Panel,
        };
        self.add(studio, k, Some([0.5, 0.5]));
    }

    pub fn test_kinds(&self) -> Vec<String> {
        self.widgets.iter().map(|w| w.kind().label().to_string()).collect()
    }
}

fn snap(v: f32, reference: f32) -> f32 {
    let g = 8.0 / reference;
    (v / g).round() * g
}

fn color_row(ui: &mut Ui, label: &str, c: [f32; 4], key: &'static str, changes: &mut Vec<(&'static str, Json)>) {
    ui.label(RichText::new(label).color(DIM).size(12.0));
    let mut col = c;
    if ui.color_edit_button_rgba_unmultiplied(&mut col).changed() {
        changes.push((key, jarr(&col)));
    }
    ui.end_row();
}

/// A panel section in the manner of the editor's details panels: a bar with the title, the content under it.
fn section(ui: &mut Ui, title: &str, content: impl FnOnce(&mut Ui)) {
    let id = ui.make_persistent_id(("ui-section", title));
    let mut open = ui.data_mut(|d| d.get_persisted::<bool>(id)).unwrap_or(true);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
    ui.painter().rect_filled(r, CornerRadius::same(2), PANEL_HEAD);
    let c = Pos2::new(r.min.x + 12.0, r.center().y);
    let pts = if open { vec![c + Vec2::new(-4.0, -2.5), c + Vec2::new(4.0, -2.5), c + Vec2::new(0.0, 3.0)] } else { vec![c + Vec2::new(-2.5, -4.0), c + Vec2::new(-2.5, 4.0), c + Vec2::new(3.0, 0.0)] };
    ui.painter().add(Shape::convex_polygon(pts, TEXT, Stroke::NONE));
    ui.painter().text(r.min + Vec2::new(24.0, 12.0), Align2::LEFT_CENTER, title, FontId::new(13.0, FontFamily::Name("segoe-bold".into())), Color32::WHITE);
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_persisted(id, open));
    }
    if open {
        ui.add_space(4.0);
        egui::Frame::NONE.inner_margin(egui::Margin { left: 6, right: 2, top: 0, bottom: 0 }).show(ui, |ui| content(ui));
        ui.add_space(6.0);
    }
}

fn widget_icon(p: &egui::Painter, c: Pos2, kind: Kind) {
    let s = Stroke::new(1.4, Color32::from_rgb(160, 200, 240));
    match kind {
        Kind::Button => {
            p.rect_stroke(Rect::from_center_size(c, Vec2::new(14.0, 9.0)), CornerRadius::same(2), s, StrokeKind::Middle);
            p.line_segment([c + Vec2::new(-3.0, 0.0), c + Vec2::new(3.0, 0.0)], s);
        }
        Kind::Text => {
            p.text(c, Align2::CENTER_CENTER, "T", FontId::new(14.0, FontFamily::Name("segoe-bold".into())), Color32::from_rgb(160, 200, 240));
        }
        Kind::Bar => {
            p.rect_stroke(Rect::from_center_size(c, Vec2::new(14.0, 6.0)), CornerRadius::same(1), s, StrokeKind::Middle);
            p.rect_filled(Rect::from_min_size(c - Vec2::new(6.0, 2.0), Vec2::new(7.0, 4.0)), CornerRadius::ZERO, Color32::from_rgb(160, 200, 240));
        }
        Kind::Panel => {
            p.rect_stroke(Rect::from_center_size(c, Vec2::new(13.0, 11.0)), CornerRadius::same(1), s, StrokeKind::Middle);
            p.rect_stroke(Rect::from_center_size(c, Vec2::new(7.0, 5.0)), CornerRadius::ZERO, Stroke::new(1.0, Color32::from_rgb(110, 140, 170)), StrokeKind::Middle);
        }
    }
}

fn display(c: [f32; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied((c[0].clamp(0.0, 1.0) * 255.0) as u8, (c[1].clamp(0.0, 1.0) * 255.0) as u8, (c[2].clamp(0.0, 1.0) * 255.0) as u8, (c[3].clamp(0.0, 1.0) * 255.0) as u8)
}

/// Draw one widget the way the game's renderers do: back, fill, edge, a centred label.
fn draw_widget(p: &egui::Painter, w: &Widget, r: Rect, frame: Rect, k: f32) {
    let _ = k;
    let alpha_mul = if w.visible { 1.0 } else { 0.35 };
    let dim = |c: Color32| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (c.a() as f32 * alpha_mul) as u8);
    if w.fill < 1.0 {
        p.rect_filled(r, CornerRadius::ZERO, dim(display(w.back)));
        let f = Rect::from_min_size(r.min, Vec2::new(r.width() * w.fill.clamp(0.0, 1.0), r.height()));
        p.rect_filled(f, CornerRadius::ZERO, dim(display(w.color)));
    } else {
        p.rect_filled(r, CornerRadius::ZERO, dim(display(w.color)));
    }
    if w.border > 0.0 {
        let bw = (w.border * frame.height()).max(1.0);
        p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(bw, dim(display(w.border_color))), StrokeKind::Inside);
    }
    if !w.text.is_empty() {
        let size = (w.text_size * frame.height() * 0.8).clamp(4.0, 400.0);
        let p2 = p.with_clip_rect(r.intersect(p.clip_rect()));
        p2.text(r.center(), Align2::CENTER_CENTER, &w.text, FontId::proportional(size), dim(display(w.text_color)));
    }
    if !w.visible {
        p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, Color32::from_rgba_unmultiplied(200, 200, 200, 80)), StrokeKind::Inside);
    }
    let _ = Align::Min;
}
