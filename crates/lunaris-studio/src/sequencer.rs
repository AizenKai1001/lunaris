//! The Sequencer tab: a `Timeline` (docs/GAMEPLAY.md, "Timelines": a script of `at SECONDS ACTION` lines) drawn as tracks over a time ruler, the way Unreal's Sequencer shows a
//! cutscene. Each number the script moves has a lane; a ramp is a bar from its start to its end, a `set`, a resource step or an event a diamond, `end` a line across the lanes.
//! Drag a bar or a diamond to move it in time, drag the right edge of a bar to change how long it takes, double-click to edit its numbers, right-click for more, and drag
//! the playhead (or click the ruler) to scrub: in a **preview** (a play session that Stop undoes) the scene is put where the sequence would have it at that second
//! (`timeline_seek`). Everything is written back into the `Timeline`'s script text, so the text, this view and the engine are one thing.
use crate::state::{Mode, Studio};
use crate::theme;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Vec2};
use lunaris_json::Json;
use std::time::Instant;

/// Times are kept to this many seconds when dragging (hold Shift for no snapping).
pub const SNAP: f32 = 0.05;

// ---------------------------------------------------------------------------------------------------------------- the model

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Ramp { path: String, from: f32, to: f32, dur: f32, ease: u32 },
    Set { path: String, add: bool, value: f32 },
    Res { name: String, add: bool, value: f32 },
    Event(String),
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub at: f32,
    pub kind: Kind,
}

/// A line of the script: a step, or text the sequencer keeps as it is (comments, blank lines, anything it does not understand).
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    /// The step and the text it was read from (written back untouched while the step is unchanged, so a comment on the line survives).
    Step(Item, String),
    Other(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seq {
    pub lines: Vec<Line>,
}

/// One row of the sequencer: everything that moves the same thing.
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub label: String,
    /// Indexes into `Seq::lines`.
    pub lines: Vec<usize>,
}

fn num(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

/// The number as the script writes it: whole numbers without a point, others to three decimals without trailing zeros.
pub fn fmt_num(v: f32) -> String {
    if v == v.trunc() && v.abs() < 1e9 {
        return format!("{}", v as i64);
    }
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" || s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// One line of a timeline script as a step (the same language as `gameplay::parse_timeline`), or `None` for a comment, a blank line or a line that does not parse.
pub fn parse_line(raw: &str) -> Option<Item> {
    let line = raw.split('#').next().unwrap_or("").trim();
    let rest = line.strip_prefix("at ")?;
    let (when, what) = rest.trim().split_once(char::is_whitespace).unwrap_or((rest.trim(), ""));
    let at = num(when).filter(|v| *v >= 0.0)?;
    let what = what.trim();
    let kind = if what == "end" {
        Kind::End
    } else if let Some(r) = what.strip_prefix("ramp ") {
        let (path, tail) = r.trim().split_once(char::is_whitespace)?;
        let (range, after) = tail.split_once(" over ")?;
        let (a, b) = range.split_once("->")?;
        let mut words = after.split_whitespace();
        let dur = num(words.next()?)?.max(0.0);
        let mut ease = 0u32;
        while let Some(w) = words.next() {
            if w == "ease" {
                ease = words.next()?.parse().ok()?;
            } else {
                return None;
            }
        }
        Kind::Ramp { path: path.to_string(), from: num(a)?, to: num(b)?, dur, ease }
    } else if let Some(name) = what.strip_prefix("event ") {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        Kind::Event(name.to_string())
    } else {
        let (verb, rest) = what.split_once(' ')?;
        let (target, add, value) = if let Some((t, v)) = rest.split_once("+=") {
            (t.trim(), true, num(v)?)
        } else if let Some((t, v)) = rest.split_once('=') {
            (t.trim(), false, num(v)?)
        } else {
            return None;
        };
        match verb {
            "set" => Kind::Set { path: target.to_string(), add, value },
            "res" => Kind::Res { name: target.to_string(), add, value },
            _ => return None,
        }
    };
    Some(Item { at, kind })
}

/// A step as a line of script.
pub fn render(item: &Item) -> String {
    let at = fmt_num(item.at);
    match &item.kind {
        Kind::End => format!("at {at} end"),
        Kind::Event(n) => format!("at {at} event {n}"),
        Kind::Set { path, add, value } => format!("at {at} set {path} {} {}", if *add { "+=" } else { "=" }, fmt_num(*value)),
        Kind::Res { name, add, value } => format!("at {at} res {name} {} {}", if *add { "+=" } else { "=" }, fmt_num(*value)),
        Kind::Ramp { path, from, to, dur, ease } => format!("at {at} ramp {path} {} -> {} over {}{}", fmt_num(*from), fmt_num(*to), fmt_num(*dur), if *ease != 0 { format!(" ease {ease}") } else { String::new() }),
    }
}

impl Item {
    /// When the step is over: a ramp lasts `dur`, anything else is an instant.
    pub fn end(&self) -> f32 {
        match &self.kind {
            Kind::Ramp { dur, .. } => self.at + dur,
            _ => self.at,
        }
    }

    fn lane_label(&self) -> Option<String> {
        match &self.kind {
            Kind::Ramp { path, .. } | Kind::Set { path, .. } => Some(path.clone()),
            Kind::Res { name, .. } => Some(format!("res {name}")),
            Kind::Event(_) => Some("events".to_string()),
            Kind::End => None,
        }
    }
}

impl Seq {
    pub fn parse(text: &str) -> Seq {
        let mut lines: Vec<Line> = text.split(['\n', ';']).map(|raw| match parse_line(raw) {
            Some(item) => Line::Step(item, raw.trim_end().to_string()),
            None => Line::Other(raw.trim_end().to_string()),
        }).collect();
        // the empty line after a final newline is not a line of the script
        while matches!(lines.last(), Some(Line::Other(s)) if s.trim().is_empty()) {
            lines.pop();
        }
        Seq { lines }
    }

    /// The script: a step that was not changed since it was read keeps the text it had, so comments written on its line are not lost.
    pub fn to_text(&self) -> String {
        let mut out: Vec<String> = Vec::with_capacity(self.lines.len());
        for l in &self.lines {
            out.push(match l {
                Line::Other(s) => s.clone(),
                Line::Step(item, raw) => {
                    if parse_line(raw).as_ref() == Some(item) {
                        raw.clone()
                    } else {
                        render(item)
                    }
                }
            });
        }
        out.join("\n")
    }

    pub fn item(&self, line: usize) -> Option<&Item> {
        match self.lines.get(line) {
            Some(Line::Step(i, _)) => Some(i),
            _ => None,
        }
    }

    pub fn item_mut(&mut self, line: usize) -> Option<&mut Item> {
        match self.lines.get_mut(line) {
            Some(Line::Step(i, _)) => Some(i),
            _ => None,
        }
    }

    pub fn items(&self) -> impl Iterator<Item = (usize, &Item)> {
        self.lines.iter().enumerate().filter_map(|(i, l)| match l {
            Line::Step(item, _) => Some((i, item)),
            _ => None,
        })
    }

    /// How long the sequence is: the end of its last step.
    pub fn length(&self) -> f32 {
        self.items().map(|(_, i)| i.end()).fold(0.0, f32::max)
    }

    /// The lanes in the order their first step appears; `end` has no lane (it is a line across all of them).
    pub fn lanes(&self) -> Vec<Lane> {
        let mut lanes: Vec<Lane> = Vec::new();
        for (idx, item) in self.items() {
            let Some(label) = item.lane_label() else { continue };
            match lanes.iter_mut().find(|l| l.label == label) {
                Some(l) => l.lines.push(idx),
                None => lanes.push(Lane { label, lines: vec![idx] }),
            }
        }
        lanes
    }

    pub fn add(&mut self, item: Item) -> usize {
        let raw = render(&item);
        self.lines.push(Line::Step(item, raw));
        self.lines.len() - 1
    }

    pub fn remove(&mut self, line: usize) {
        if line < self.lines.len() {
            self.lines.remove(line);
        }
    }
}

/// `t` snapped to `SNAP` seconds (not below zero).
pub fn snapped(t: f32, snap: bool) -> f32 {
    let t = t.max(0.0);
    if snap {
        (t / SNAP).round() * SNAP
    } else {
        (t * 1000.0).round() / 1000.0
    }
}

// ---------------------------------------------------------------------------------------------------------------- the panel

#[derive(Clone, Debug)]
enum Drag {
    Playhead,
    Move { line: usize, grab: f32 },
    Resize { line: usize },
}

/// A step being edited or added: the numbers and names in boxes.
struct Editing {
    line: Option<usize>,
    item: Item,
}

/// What the entity's `Timeline` says right now.
#[derive(Clone, Debug, Default)]
struct Live {
    script: String,
    time: f32,
    looping: bool,
    speed: f32,
}

pub struct SequencerUi {
    uid: Option<u64>,
    live: Option<Live>,
    seq: Seq,
    /// The script `seq` was read from (the world's `Timeline.script` when it was last read): when the world's differs and nothing is being dragged, `seq` is read again.
    script: String,
    zoom: f32,
    scroll: f32,
    drag: Option<Drag>,
    editing: Option<Editing>,
    next_sync: Option<Instant>,
    /// The playhead while the timeline is not being run (edit mode, or before it has a time).
    playhead: f32,
    /// The step under the pointer when the context menu was asked for.
    menu_line: Option<usize>,
}

impl Default for SequencerUi {
    fn default() -> SequencerUi {
        SequencerUi { uid: None, live: None, seq: Seq::default(), script: String::new(), zoom: 120.0, scroll: -0.3, drag: None, editing: None, next_sync: None, playhead: 0.0, menu_line: None }
    }
}

const LABELS: f32 = 190.0;
const RULER: f32 = 24.0;
const LANE: f32 = 28.0;

fn lane_color(label: &str) -> Color32 {
    let h = label.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let palette = [(86, 156, 214), (120, 190, 120), (214, 160, 80), (190, 120, 190), (90, 190, 190), (220, 110, 110), (170, 170, 90)];
    let (r, g, b) = palette[(h % palette.len() as u32) as usize];
    Color32::from_rgb(r, g, b)
}

/// Ruler tick spacing in seconds for a zoom (pixels a second): the first of the round steps that keeps ticks at least 56 pixels apart.
pub fn tick_step(zoom: f32) -> f32 {
    [0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0].into_iter().find(|s| s * zoom >= 56.0).unwrap_or(60.0)
}

impl SequencerUi {
    /// Read the entity's `Timeline` (a few times a second, and at once when the selection changes) and, when its script is not what `seq` was made from and nothing is being
    /// dragged or edited, read the script again.
    fn sync(&mut self, studio: &mut Studio) {
        if studio.selection != self.uid {
            self.uid = studio.selection;
            self.live = None;
            self.seq = Seq::default();
            self.script.clear();
            self.next_sync = None;
            self.drag = None;
            self.editing = None;
        }
        let Some(uid) = self.uid else { return };
        if self.next_sync.is_some_and(|t| t.elapsed().as_secs_f32() < 0.12) {
            return;
        }
        self.next_sync = Some(Instant::now());
        let got = studio.engine.call("get", &Json::obj([("uid", Json::from(uid as i64)), ("components", Json::arr([Json::str("Timeline")]))])).ok();
        self.live = got.as_ref().and_then(|g| g.get("components")).and_then(|c| c.get("Timeline")).map(|t| Live {
            script: t.get("script").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            time: t.get("time").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            looping: t.get("looping").and_then(|v| v.as_bool()).unwrap_or(false),
            speed: t.get("speed").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        });
        if let Some(l) = &self.live {
            if l.script != self.script && self.drag.is_none() && self.editing.is_none() {
                self.seq = Seq::parse(&l.script);
                self.script = l.script.clone();
            }
        }
    }

    /// Write the model back into the entity's `Timeline`.
    fn commit(&mut self, studio: &mut Studio) {
        let Some(uid) = self.uid else { return };
        let text = self.seq.to_text();
        if text == self.script {
            return;
        }
        studio.set(uid, "Timeline", Json::obj([("script", Json::str(text.clone()))]), None);
        self.script = text;
        self.next_sync = None;
    }

    /// Put the timeline at `t` seconds: the scene shows what the sequence makes of it then. Only inside a preview (a play session, which Stop undoes): in edit mode the scene is
    /// the author's and scrubbing must not change it, so the playhead only moves.
    pub fn seek(&mut self, studio: &mut Studio, t: f32) {
        let t = t.max(0.0);
        self.playhead = t;
        let Some(uid) = self.uid else { return };
        if studio.mode == Mode::Edit {
            return;
        }
        if let Ok(r) = studio.engine.call("timeline_seek", &Json::obj([("uid", Json::from(uid as i64)), ("time", Json::Float(t as f64))])) {
            if let Some(clamped) = r.get("time").and_then(|v| v.as_f64()) {
                self.playhead = clamped as f32;
            }
            studio.rows_dirty = true;
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, studio: &mut Studio) {
        self.sync(studio);
        let Some(uid) = self.uid else {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("Select an entity that has a Timeline (or any entity, to give it one).").color(theme::TEXT_DIM)));
            return;
        };
        if self.live.is_none() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("This entity has no Timeline.").color(theme::TEXT_DIM));
                if ui.button("Add a Timeline").on_hover_text("A script of timed steps that moves numbers, fires events and sets resources").clicked() {
                    let script = "at 0 ramp Transform.pos[1] 0 -> 2 over 1 ease 3\nat 2 end";
                    let _ = studio.try_call("add_component", Json::obj([("uid", Json::from(uid as i64)), ("component", Json::str("Timeline")), ("value", Json::obj([("script", Json::str(script)), ("playing", Json::Bool(false))]))]));
                    self.next_sync = None;
                }
            });
            return;
        }
        self.toolbar(ui, studio);
        self.tracks(ui, studio);
        self.edit_window(ui.ctx(), studio);
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, studio: &mut Studio) {
        let length = self.seq.length();
        ui.horizontal(|ui| {
            if studio.mode == Mode::Edit {
                if ui.button("Preview").on_hover_text("Start a preview (like Play, paused): scrub to see the scene as the sequence makes it at any second; Stop puts everything back").clicked() {
                    studio.play();
                    studio.pause();
                    self.seek(studio, self.playhead);
                }
            } else if ui.button("Stop preview").clicked() {
                studio.stop();
            }
            let now = self.live.as_ref().map(|l| l.time).filter(|_| studio.mode != Mode::Edit).unwrap_or(self.playhead);
            ui.label(RichText::new(format!("{now:.2} / {length:.2} s")).monospace());
            ui.separator();
            ui.menu_button("+ Add step", |ui| {
                let at = snapped(self.playhead, true);
                for (label, kind) in [
                    ("Ramp: move a number smoothly", Kind::Ramp { path: "Transform.pos[1]".into(), from: 0.0, to: 1.0, dur: 1.0, ease: 3 }),
                    ("Set: put a number somewhere", Kind::Set { path: "Light.intensity".into(), add: false, value: 0.0 }),
                    ("Event: tell the game something happened", Kind::Event("boom".into())),
                    ("Resource: change a global number", Kind::Res { name: "score".into(), add: true, value: 1.0 }),
                    ("End: where the sequence stops", Kind::End),
                ] {
                    if ui.button(label).clicked() {
                        self.editing = Some(Editing { line: None, item: Item { at, kind } });
                        ui.close();
                    }
                }
            });
            if let Some(l) = self.live.clone() {
                let mut looping = l.looping;
                if ui.checkbox(&mut looping, "Loop").changed() {
                    if let Some(uid) = self.uid {
                        studio.set(uid, "Timeline", Json::obj([("looping", Json::Bool(looping))]), None);
                    }
                }
                let mut speed = l.speed;
                ui.label(RichText::new("speed").small().color(theme::TEXT_DIM));
                if ui.add(egui::DragValue::new(&mut speed).speed(0.05).range(0.0..=10.0)).changed() {
                    if let Some(uid) = self.uid {
                        studio.set(uid, "Timeline", Json::obj([("speed", Json::Float(speed as f64))]), None);
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::Slider::new(&mut self.zoom, 20.0..=600.0).logarithmic(true).show_value(false));
                ui.label(RichText::new("zoom").small().color(theme::TEXT_DIM));
            });
        });
        if studio.mode == Mode::Edit {
            ui.label(RichText::new("Edit mode: the playhead moves but the scene does not. Press Preview to scrub the scene.").small().color(theme::TEXT_DIM));
        }
    }

    fn tracks(&mut self, ui: &mut egui::Ui, studio: &mut Studio) {
        let lanes = self.seq.lanes();
        let rows = lanes.len().max(1);
        let avail = ui.available_size();
        let height = (RULER + rows as f32 * LANE + 12.0).max(avail.y);
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(avail.x, height), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, theme::FIELD);
        let x0 = rect.min.x + LABELS;
        let (zoom, scroll) = (self.zoom, self.scroll);
        let x_of = |t: f32| x0 + (t - scroll) * zoom;
        let t_of = |x: f32| (x - x0) / zoom + scroll;
        let lane_top = |i: usize| rect.min.y + RULER + i as f32 * LANE;

        // wheel: scroll in time, Ctrl zooms
        if resp.hovered() {
            let (sx, sy, ctrl) = ui.input(|i| (i.smooth_scroll_delta.x, i.smooth_scroll_delta.y, i.modifiers.command));
            if ctrl && sy != 0.0 {
                self.zoom = (self.zoom * (1.0 + sy * 0.002)).clamp(20.0, 600.0);
            } else if sx != 0.0 || sy != 0.0 {
                self.scroll = (self.scroll - (sx + sy) / zoom).max(-0.5);
            }
        }

        // lanes: names at the left, a band for each
        for (i, lane) in lanes.iter().enumerate() {
            let top = lane_top(i);
            let band = Rect::from_min_size(Pos2::new(rect.min.x, top), Vec2::new(rect.width(), LANE));
            painter.rect_filled(band, CornerRadius::ZERO, if i % 2 == 0 { theme::ROW_ALT } else { theme::PANEL });
            painter.with_clip_rect(Rect::from_min_size(band.min, Vec2::new(LABELS - 4.0, LANE))).text(Pos2::new(band.min.x + 8.0, band.center().y), Align2::LEFT_CENTER, &lane.label, FontId::proportional(12.5), theme::TEXT);
            painter.circle_filled(Pos2::new(band.min.x + LABELS - 10.0, band.center().y), 3.5, lane_color(&lane.label));
        }
        if lanes.is_empty() {
            painter.text(Pos2::new(x0 + 16.0, rect.min.y + RULER + LANE / 2.0), Align2::LEFT_CENTER, "No steps yet: + Add step", FontId::proportional(12.5), theme::TEXT_DIM);
        }
        painter.line_segment([Pos2::new(x0, rect.min.y), Pos2::new(x0, rect.max.y)], Stroke::new(1.0, theme::LINE));

        // the ruler
        let step = tick_step(self.zoom);
        painter.rect_filled(Rect::from_min_size(Pos2::new(x0, rect.min.y), Vec2::new(rect.width() - LABELS, RULER)), CornerRadius::ZERO, theme::TOOLBAR);
        let clip = Rect::from_min_max(Pos2::new(x0, rect.min.y), rect.max);
        let tp = painter.with_clip_rect(clip);
        let mut k = (self.scroll / step).floor() as i32;
        loop {
            let t = k as f32 * step;
            let x = x_of(t);
            if x > rect.max.x {
                break;
            }
            if t >= 0.0 {
                tp.line_segment([Pos2::new(x, rect.min.y + RULER - 8.0), Pos2::new(x, rect.max.y)], Stroke::new(1.0, Color32::from_white_alpha(if k % 5 == 0 { 26 } else { 12 })));
                tp.text(Pos2::new(x + 3.0, rect.min.y + 4.0), Align2::LEFT_TOP, format!("{}", fmt_num((t * 100.0).round() / 100.0)), FontId::proportional(10.5), theme::TEXT_DIM);
            }
            k += 1;
        }

        // the steps
        let mut hits: Vec<(usize, Rect, bool)> = Vec::new(); // (line, rect, can resize)
        for (i, lane) in lanes.iter().enumerate() {
            let top = lane_top(i);
            let mid = top + LANE / 2.0;
            let color = lane_color(&lane.label);
            for &line in &lane.lines {
                let Some(item) = self.seq.item(line) else { continue };
                let selected = matches!(&self.drag, Some(Drag::Move { line: l, .. } | Drag::Resize { line: l }) if *l == line);
                match &item.kind {
                    Kind::Ramp { from, to, ease, dur, .. } => {
                        let bar = Rect::from_min_max(Pos2::new(x_of(item.at), top + 4.0), Pos2::new(x_of(item.at + dur).max(x_of(item.at) + 6.0), top + LANE - 4.0));
                        tp.rect_filled(bar, CornerRadius::same(3), color.gamma_multiply(if selected { 1.0 } else { 0.72 }));
                        tp.rect_stroke(bar, CornerRadius::same(3), Stroke::new(1.0, Color32::from_black_alpha(120)), StrokeKind::Inside);
                        tp.with_clip_rect(bar).text(Pos2::new(bar.min.x + 5.0, mid), Align2::LEFT_CENTER, format!("{} \u{2192} {}{}", fmt_num(*from), fmt_num(*to), if *ease != 0 { format!("  ease {ease}") } else { String::new() }), FontId::proportional(11.0), Color32::from_black_alpha(220));
                        hits.push((line, bar, true));
                    }
                    kind => {
                        let c = Pos2::new(x_of(item.at), mid);
                        let r = 7.0;
                        let d = vec![c + Vec2::new(0.0, -r), c + Vec2::new(r, 0.0), c + Vec2::new(0.0, r), c + Vec2::new(-r, 0.0)];
                        tp.add(Shape::convex_polygon(d, color.gamma_multiply(if selected { 1.0 } else { 0.8 }), Stroke::new(1.0, Color32::from_black_alpha(160))));
                        let text = match kind {
                            Kind::Set { value, add, .. } => format!("{}{}", if *add { "+" } else { "" }, fmt_num(*value)),
                            Kind::Res { value, add, .. } => format!("{}{}", if *add { "+" } else { "" }, fmt_num(*value)),
                            Kind::Event(n) => n.clone(),
                            _ => String::new(),
                        };
                        tp.text(c + Vec2::new(r + 4.0, 0.0), Align2::LEFT_CENTER, text, FontId::proportional(11.0), theme::TEXT);
                        hits.push((line, Rect::from_center_size(c, Vec2::splat(r * 2.0 + 4.0)), false));
                    }
                }
            }
        }
        // `end`: a line across every lane
        for (line, item) in self.seq.items() {
            if item.kind == Kind::End {
                let x = x_of(item.at);
                // (what lies after the end is darker, as in Unreal's Sequencer; the playhead is the red thing)
                tp.rect_filled(egui::Rect::from_min_max(Pos2::new(x, rect.min.y + RULER), rect.max), CornerRadius::ZERO, Color32::from_black_alpha(80));
                tp.line_segment([Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)], Stroke::new(1.5, Color32::from_rgb(120, 120, 130)));
                tp.text(Pos2::new(x + 4.0, rect.max.y - 14.0), Align2::LEFT_CENTER, "end", FontId::proportional(11.0), Color32::from_rgb(150, 150, 160));
                hits.push((line, Rect::from_min_max(Pos2::new(x - 4.0, rect.min.y + RULER), Pos2::new(x + 4.0, rect.max.y)), false));
            }
        }

        // the playhead
        let head_t = if studio.mode == Mode::Edit { self.playhead } else { self.live.as_ref().map(|l| l.time).unwrap_or(self.playhead) };
        let hx = x_of(head_t);
        let red = Color32::from_rgb(214, 66, 44);
        tp.line_segment([Pos2::new(hx, rect.min.y), Pos2::new(hx, rect.max.y)], Stroke::new(1.5, red));
        tp.add(Shape::convex_polygon(vec![Pos2::new(hx - 6.0, rect.min.y), Pos2::new(hx + 6.0, rect.min.y), Pos2::new(hx + 6.0, rect.min.y + 8.0), Pos2::new(hx, rect.min.y + 14.0), Pos2::new(hx - 6.0, rect.min.y + 8.0)], red, Stroke::NONE));

        // pointer
        let ptr = resp.interact_pointer_pos().or_else(|| resp.hover_pos());
        let shift = ui.input(|i| i.modifiers.shift);
        let hit_at = |p: Pos2| hits.iter().rev().find(|(_, r, _)| r.contains(p)).map(|(l, r, rs)| (*l, *r, *rs));
        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                if p.x < x0 {
                    // the names are not for dragging
                } else if let Some((line, r, can_resize)) = hit_at(p).filter(|_| p.y > rect.min.y + RULER) {
                    if can_resize && p.x > r.max.x - 7.0 {
                        self.drag = Some(Drag::Resize { line });
                    } else {
                        let at = self.seq.item(line).map(|i| i.at).unwrap_or(0.0);
                        self.drag = Some(Drag::Move { line, grab: t_of(p.x) - at });
                    }
                } else {
                    self.drag = Some(Drag::Playhead);
                }
            }
        }
        if resp.dragged() {
            if let (Some(p), Some(d)) = (ptr, self.drag.clone()) {
                let t = t_of(p.x);
                match d {
                    Drag::Playhead => self.seek(studio, snapped(t, !shift)),
                    Drag::Move { line, grab } => {
                        if let Some(i) = self.seq.item_mut(line) {
                            i.at = snapped(t - grab, !shift);
                        }
                    }
                    Drag::Resize { line } => {
                        if let Some(i) = self.seq.item_mut(line) {
                            let at = i.at;
                            if let Kind::Ramp { dur, .. } = &mut i.kind {
                                *dur = (snapped(t, !shift) - at).max(0.0);
                            }
                        }
                    }
                }
            }
        }
        if resp.drag_stopped() {
            if matches!(self.drag, Some(Drag::Move { .. } | Drag::Resize { .. })) {
                self.commit(studio);
            }
            self.drag = None;
        }
        if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                if p.x >= x0 && hit_at(p).filter(|_| p.y > rect.min.y + RULER).is_none() {
                    self.seek(studio, snapped(t_of(p.x), !shift));
                }
            }
        }
        if resp.double_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                if let Some((line, _, _)) = hit_at(p).filter(|_| p.y > rect.min.y + RULER) {
                    if let Some(item) = self.seq.item(line) {
                        self.editing = Some(Editing { line: Some(line), item: item.clone() });
                    }
                }
            }
        }
        // right click: delete or copy a step
        if resp.secondary_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.menu_line = hit_at(p).filter(|_| p.y > rect.min.y + RULER).map(|h| h.0);
            }
        }
        resp.context_menu(|ui| {
            let Some(line) = self.menu_line.filter(|l| self.seq.item(*l).is_some()) else {
                ui.label(RichText::new("Right-click a step").small().color(theme::TEXT_DIM));
                return;
            };
            if ui.button("Edit...").clicked() {
                if let Some(item) = self.seq.item(line) {
                    self.editing = Some(Editing { line: Some(line), item: item.clone() });
                }
                ui.close();
            }
            if ui.button("Duplicate (0.5 s later)").clicked() {
                if let Some(mut item) = self.seq.item(line).cloned() {
                    item.at = snapped(item.at + 0.5, true);
                    self.seq.add(item);
                    self.commit(studio);
                }
                ui.close();
            }
            if ui.button("Delete").clicked() {
                self.seq.remove(line);
                self.commit(studio);
                ui.close();
            }
        });
    }

    /// The window for the numbers and names of one step (a new one, or one being edited).
    fn edit_window(&mut self, ctx: &egui::Context, studio: &mut Studio) {
        let Some(mut ed) = self.editing.take() else { return };
        let mut open = true;
        let mut done = false;
        let mut cancel = false;
        egui::Window::new(if ed.line.is_some() { "Edit step" } else { "New step" }).open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            egui::Grid::new("seq-edit").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("At (seconds)");
                ui.add(egui::DragValue::new(&mut ed.item.at).speed(0.05).range(0.0..=3600.0));
                ui.end_row();
                match &mut ed.item.kind {
                    Kind::Ramp { path, from, to, dur, ease } => {
                        ui.label("Moves");
                        ui.add(egui::TextEdit::singleline(path).hint_text("Transform.pos[1]").desired_width(200.0));
                        ui.end_row();
                        ui.label("From, to");
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(from).speed(0.05));
                            ui.label("\u{2192}");
                            ui.add(egui::DragValue::new(to).speed(0.05));
                        });
                        ui.end_row();
                        ui.label("Takes (seconds)");
                        ui.add(egui::DragValue::new(dur).speed(0.05).range(0.0..=3600.0));
                        ui.end_row();
                        ui.label("Easing");
                        ui.add(egui::DragValue::new(ease).range(0..=13)).on_hover_text("0 linear, 1 in-quad, 2 out-quad, 3 in-out-quad, 4 in-cubic, 5 out-cubic, 6 in-out-cubic, 7 in-sine, 8 out-sine, 9 in-out-sine, 10 out-back, 11 out-bounce, 12 out-elastic, 13 smoothstep");
                        ui.end_row();
                    }
                    Kind::Set { path, add, value } => {
                        ui.label("Number");
                        ui.add(egui::TextEdit::singleline(path).hint_text("Light.intensity").desired_width(200.0));
                        ui.end_row();
                        ui.label(if *add { "Add" } else { "Set to" });
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(value).speed(0.05));
                            ui.checkbox(add, "add to it instead");
                        });
                        ui.end_row();
                    }
                    Kind::Res { name, add, value } => {
                        ui.label("Resource");
                        ui.add(egui::TextEdit::singleline(name).hint_text("score").desired_width(200.0));
                        ui.end_row();
                        ui.label(if *add { "Add" } else { "Set to" });
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(value).speed(0.05));
                            ui.checkbox(add, "add to it instead");
                        });
                        ui.end_row();
                    }
                    Kind::Event(name) => {
                        ui.label("Event");
                        ui.add(egui::TextEdit::singleline(name).hint_text("boom").desired_width(200.0));
                        ui.end_row();
                    }
                    Kind::End => {
                        ui.label("");
                        ui.label(RichText::new("The sequence stops here (or starts again, with Loop).").small().color(theme::TEXT_DIM));
                        ui.end_row();
                    }
                }
            });
            ui.add_space(6.0);
            let valid = match &ed.item.kind {
                Kind::Ramp { path, .. } | Kind::Set { path, .. } => !path.trim().is_empty() && !path.contains(char::is_whitespace),
                Kind::Res { name, .. } => !name.trim().is_empty() && !name.contains(char::is_whitespace),
                Kind::Event(n) => !n.trim().is_empty(),
                Kind::End => true,
            };
            ui.horizontal(|ui| {
                if ui.add_enabled(valid, egui::Button::new("OK")).clicked() {
                    done = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                if !valid {
                    ui.label(RichText::new("needs a name without spaces").small().color(theme::WARN));
                }
            });
        });
        if done {
            ed.item.at = snapped(ed.item.at, false);
            match ed.line {
                Some(line) => {
                    if let Some(i) = self.seq.item_mut(line) {
                        *i = ed.item.clone();
                    }
                }
                None => {
                    self.seq.add(ed.item.clone());
                }
            }
            self.commit(studio);
        } else if open && !cancel {
            self.editing = Some(ed);
        }
    }

    #[cfg(test)]
    pub fn test_seq(&self) -> &Seq {
        &self.seq
    }

    #[cfg(test)]
    pub fn test_script(&self) -> &str {
        &self.script
    }

    #[cfg(test)]
    pub fn test_edit(&mut self, f: impl FnOnce(&mut Seq)) {
        f(&mut self.seq);
    }

    #[cfg(test)]
    pub fn test_commit(&mut self, studio: &mut Studio) {
        self.commit(studio);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = "# the intro\nat 0 ramp Transform.pos[1] 0 -> 5 over 2 ease 3\nat 0.5 set Light.intensity = 0.25  # dim\nat 1 event thunder\nat 1.5 res score += 10\nat 3 end\n";

    #[test]
    fn a_script_is_read_into_steps_lanes_and_a_length() {
        let s = Seq::parse(SCRIPT);
        assert_eq!(s.items().count(), 5);
        assert_eq!(s.length(), 3.0);
        let lanes = s.lanes();
        assert_eq!(lanes.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(), ["Transform.pos[1]", "Light.intensity", "events", "res score"], "`end` has no lane");
        match &s.item(1).unwrap().kind {
            Kind::Ramp { path, from, to, dur, ease } => assert_eq!((path.as_str(), *from, *to, *dur, *ease), ("Transform.pos[1]", 0.0, 5.0, 2.0, 3)),
            other => panic!("{other:?}"),
        }
        assert_eq!(s.item(2).unwrap().kind, Kind::Set { path: "Light.intensity".into(), add: false, value: 0.25 });
        assert_eq!(s.item(4).unwrap().kind, Kind::Res { name: "score".into(), add: true, value: 10.0 });
    }

    #[test]
    fn writing_it_back_changes_only_what_was_changed_and_keeps_comments() {
        let mut s = Seq::parse(SCRIPT);
        assert_eq!(s.to_text(), SCRIPT.trim_end(), "nothing changed: the same text (a trailing newline aside)");
        // move the `set` later: that line is written again (its comment goes with the old text), the others keep theirs
        s.item_mut(2).unwrap().at = 0.75;
        let text = s.to_text();
        assert!(text.contains("at 0.75 set Light.intensity = 0.25\n"), "{text}");
        assert!(text.starts_with("# the intro\nat 0 ramp"), "the comment line and the untouched lines are as they were");
        // the written script means the same to the engine
        let entries = lunaris_sim::gameplay::parse_timeline(&text).unwrap();
        assert_eq!(entries.len(), 5);
        assert_eq!(lunaris_sim::gameplay::timeline_length(&entries), 3.0);
    }

    #[test]
    fn what_is_not_a_step_stays_as_it_is_and_every_step_survives_a_round_trip() {
        let odd = "at 1 dance wildly\nnonsense\n\nat x event bad";
        let s = Seq::parse(odd);
        assert_eq!(s.items().count(), 0, "nothing here is a step");
        assert_eq!(s.to_text(), odd, "and nothing is lost");
        for line in ["at 0 end", "at 2.5 event a.b", "at 1 set A.b[2] += -3.5", "at 4 res x = 0.001", "at 0.25 ramp A.b 1 -> -1 over 0 ease 12", "at 0 ramp A.b 0 -> 1 over 3"] {
            let item = parse_line(line).unwrap_or_else(|| panic!("{line}"));
            assert_eq!(render(&item), line, "rendered the same");
            assert_eq!(parse_line(&render(&item)), Some(item));
        }
        assert_eq!(parse_line("at -1 end"), None, "a negative time is not a step");
        assert_eq!(parse_line("at 1 ramp A.b 0 -> 1"), None, "a ramp needs its length");
    }

    #[test]
    fn numbers_are_written_short_and_times_snap() {
        assert_eq!(fmt_num(2.0), "2");
        assert_eq!(fmt_num(0.5), "0.5");
        assert_eq!(fmt_num(0.1 + 0.2), "0.3");
        assert_eq!(fmt_num(-0.0001), "0");
        assert_eq!(fmt_num(1.2346), "1.235");
        assert!((snapped(0.12, true) - 0.1).abs() < 1e-6 && (snapped(0.13, true) - 0.15).abs() < 1e-6);
        assert_eq!(snapped(-1.0, true), 0.0);
        assert_eq!(snapped(0.1234, false), 0.123);
        assert_eq!(tick_step(120.0), 0.5);
        assert_eq!(tick_step(20.0), 5.0);
        assert!(tick_step(600.0) <= 0.1);
    }

    #[test]
    fn add_and_remove_keep_the_rest_of_the_script() {
        let mut s = Seq::parse("at 0 event a\nat 1 end");
        let n = s.add(Item { at: 0.5, kind: Kind::Event("b".into()) });
        assert_eq!(s.to_text(), "at 0 event a\nat 1 end\nat 0.5 event b");
        s.remove(n);
        s.remove(0);
        assert_eq!(s.to_text(), "at 1 end");
        assert_eq!(s.lanes().len(), 0);
    }
}
