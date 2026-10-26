//! The Effects tab: the editor for `ParticleSystem` effects (docs/FX.md), the part of Unreal's Niagara editor that shows what an effect looks like while you change it. An effect is JSON, so the middle of the tab is its text with the
//! reader's own messages under it (the same ones `check` gives, with the place of the mistake); above it a **preview** plays the effect on a small private stage (a floor, a sun, a camera that looks at it) drawn by the reference renderer, so
//! that every keystroke that makes a good effect shows at once and one that does not shows the message. The left panel lists the effect files of the workspace; the right one shows the emitters of the effect with their curves drawn.
//! Saving writes the file (a running game reads it again within half a second), and "Use on the selected entity" puts the effect on the entity that is selected in the scene, through the same `set` tool everything else uses.
use crate::state::Studio;
use crate::theme;
use eframe::egui::{self, Align2, Color32, ColorImage, CornerRadius, FontId, RichText, Sense, Shape, Stroke, TextureOptions, Vec2};
use lunaris_json::Json;
use lunaris_render::{render, RenderOptions, TextureCache};
use lunaris_sim::fx::{parse, Effect};
use lunaris_sim::particle_modules::Gradient;
use lunaris_sim::{Command, Sim};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The effect a new file starts with.
pub const TEMPLATE: &str = r##"{
  "name": "new effect",
  "bounds": 4,
  "emitters": [
    {
      "name": "sparks",
      "spawn": { "rate": 40 },
      "init": { "life": [0.8, 1.4], "speed": [1.5, 3.5], "direction": [0, 1, 0], "spread": 30, "size": 0.14, "color": [1, 0.6, 0.2, 1] },
      "update": { "gravity": [0, -2, 0], "drag": 0.3 },
      "curves": { "size": [[0, 1], [1, 0.2]], "alpha": [[0, 0], [0.1, 1], [1, 0]] },
      "render": { "texture": "soft", "blend": "add" }
    }
  ]
}
"##;

/// The size of the preview picture.
const PREVIEW: (u32, u32) = (480, 270);
/// How often the preview is drawn at the most, in seconds.
const FRAME: f32 = 1.0 / 30.0;

/// Where the effect files of a workspace are: every `*.fx.json` under `root` (folders that are hidden, `target` and `node_modules` are not looked into), as paths relative to it with `/`, sorted.
pub fn scan(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, depth: usize, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if depth < 6 && !name.starts_with('.') && name != "target" && name != "node_modules" {
                    walk(&p, root, depth + 1, out);
                }
            } else if name.ends_with(".fx.json") {
                if let Ok(rel) = p.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
            if out.len() >= 1000 {
                return;
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, 0, &mut out);
    out.sort();
    out
}

/// How long to play an effect before starting it over: nothing for one that loops (it never ends), else a little longer than its last particle lives.
pub fn suggest_repeat(e: &Effect) -> f32 {
    if e.emitters.iter().any(|m| m.looping) {
        return 0.0;
    }
    e.emitters.iter().map(|m| m.delay + m.spawn.bursts.iter().map(|b| b.time).fold(0.0, f32::max) + m.duration + m.init.life[1]).fold(0.0, f32::max) + 0.5
}

/// The stage the effect is played on: a camera, a sun, a floor, an environment, and one entity with the effect (`ParticleSystem`) in the middle.
fn stage(text: &str, dist: f32, floor: bool) -> Json {
    let a = ((dist * 0.3) - dist * 0.08).atan2(dist);
    let n = |x: f32| Json::Float(x as f64);
    let arr = |v: &[f32]| Json::arr(v.iter().map(|x| n(*x)));
    let mut ents = vec![
        Json::obj([("uid", Json::from(1)), ("components", Json::obj([("Transform", Json::obj([("pos", arr(&[0.0, dist * 0.3, dist])), ("rot", arr(&[-(a / 2.0).sin(), 0.0, 0.0, (a / 2.0).cos()]))])), ("Camera", Json::obj([("fov_y", n(50.0)), ("clear", arr(&[0.03, 0.04, 0.08, 1.0]))]))]))]),
        Json::obj([("uid", Json::from(2)), ("components", Json::obj([("Transform", Json::obj([("pos", arr(&[0.0, 0.0, 0.0]))])), ("ParticleSystem", Json::obj([("effect", Json::str(text)), ("playing", Json::Bool(false))]))]))]),
        Json::obj([("uid", Json::from(3)), ("components", Json::obj([("Transform", Json::obj([("rot", arr(&[-0.55, 0.3, 0.1, 0.77]))])), ("Light", Json::obj([("kind", Json::from(0)), ("color", arr(&[0.8, 0.85, 1.0])), ("intensity", n(1.0))]))]))]),
        Json::obj([("uid", Json::from(4)), ("components", Json::obj([("Environment", Json::obj([("sky", Json::Bool(false)), ("ambient", n(0.45))]))]))]),
    ];
    if floor {
        ents.push(Json::obj([("uid", Json::from(5)), ("components", Json::obj([("Transform", Json::obj([("pos", arr(&[0.0, -0.01, 0.0])), ("scale", arr(&[80.0, 1.0, 80.0]))])), ("MeshRenderer", Json::obj([("mesh", Json::str("plane")), ("texture", Json::str("stone")), ("uv_scale", arr(&[40.0, 40.0])), ("color", arr(&[0.35, 0.37, 0.4, 1.0])), ("roughness", n(0.95))]))]))]));
    }
    Json::obj([("lunaris", Json::from(1)), ("entities", Json::Arr(ents))])
}

/// The effect playing on its stage.
pub struct Preview {
    sim: Sim,
    tex: TextureCache,
    /// What the stage was built from: the effect's text, the distance of the camera in centimetres and whether there is a floor.
    built: (String, i32, bool),
    handle: Option<egui::TextureHandle>,
    last_draw: Option<Instant>,
    /// What the last picture showed: the particles the processor has, the steps taken for it.
    pub live: usize,
}

impl Preview {
    fn new() -> Preview {
        Preview { sim: Sim::new(1), tex: TextureCache::new(None), built: (String::new(), 0, false), handle: None, last_draw: None, live: 0 }
    }

    /// Builds the stage again when the effect, the distance or the floor changed (the particles start over then).
    fn ensure(&mut self, root: &Path, text: &str, dist: f32, floor: bool) -> bool {
        let key = (text.to_string(), (dist * 100.0) as i32, floor);
        if self.built == key {
            return false;
        }
        let mut sim = Sim::new(1);
        if sim.load_scene(&stage(text, dist, floor)).is_err() {
            return false;
        }
        self.sim = sim;
        self.tex = TextureCache::new(Some(root.to_path_buf()));
        self.built = key;
        true
    }

    /// The picture of the effect at this clock (steps the particles up to it first).
    pub fn picture(&mut self, time: f32) -> Option<lunaris_render::Frame> {
        let set = Command::Set { uid: 2, component: "ParticleSystem".into(), value: Json::obj([("time", Json::Float(time as f64))]) };
        self.sim.apply(&set).ok()?;
        let f = render(&mut self.sim.world, &mut self.tex, &RenderOptions { width: PREVIEW.0, height: PREVIEW.1, threads: 2, ..RenderOptions::default() }).ok()?;
        self.live = self.tex.fx.stats.particles;
        Some(f)
    }

    /// What reading the effect or its files said that was worth saying (the warnings the cache collected).
    pub fn notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.tex.warnings)
    }
}

pub struct EffectsUi {
    files: Vec<String>,
    files_at: Option<Instant>,
    /// The file being edited (relative to the workspace), or none for a new effect that has no file yet.
    pub path: Option<String>,
    pub text: String,
    saved: String,
    parsed: Result<Effect, String>,
    parsed_text: String,
    pub emitter: usize,
    pub clock: f32,
    pub playing: bool,
    pub speed: f32,
    pub repeat: bool,
    pub floor: bool,
    preview: Preview,
    notes: Vec<String>,
}

impl Default for EffectsUi {
    fn default() -> EffectsUi {
        EffectsUi {
            files: Vec::new(),
            files_at: None,
            path: None,
            text: TEMPLATE.to_string(),
            saved: String::new(),
            parsed: parse(TEMPLATE),
            parsed_text: TEMPLATE.to_string(),
            emitter: 0,
            clock: 0.0,
            playing: true,
            speed: 1.0,
            repeat: true,
            floor: true,
            preview: Preview::new(),
            notes: Vec::new(),
        }
    }
}

fn root_of(studio: &Studio) -> PathBuf {
    studio.opts.root.clone()
}

impl EffectsUi {
    /// Whether the text differs from what is on disk (a new effect that was never saved always does).
    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }

    pub fn effect(&self) -> Option<&Effect> {
        self.parsed.as_ref().ok()
    }

    fn reparse(&mut self) {
        if self.parsed_text != self.text {
            self.parsed = parse(&self.text);
            self.parsed_text = self.text.clone();
            if let Ok(e) = &self.parsed {
                self.emitter = self.emitter.min(e.emitters.len().saturating_sub(1));
            }
        }
    }

    fn refresh(&mut self, studio: &Studio, force: bool) {
        if force || self.files_at.is_none_or(|t| t.elapsed().as_secs_f32() > 2.0) {
            self.files = scan(&root_of(studio));
            self.files_at = Some(Instant::now());
        }
    }

    /// Opens an effect file of the workspace (the text of it); `false` when it cannot be read.
    pub fn open(&mut self, studio: &mut Studio, rel: &str) -> bool {
        match std::fs::read_to_string(root_of(studio).join(rel)) {
            Ok(text) => {
                self.path = Some(rel.to_string());
                self.saved = text.clone();
                self.text = text;
                self.emitter = 0;
                self.clock = 0.0;
                true
            }
            Err(e) => {
                studio.toast(format!("cannot read {rel}: {e}"), true);
                false
            }
        }
    }

    /// A new effect, from the template, with no file yet.
    pub fn new_effect(&mut self) {
        self.path = None;
        self.text = TEMPLATE.to_string();
        self.saved = String::new();
        self.emitter = 0;
        self.clock = 0.0;
    }

    /// Writes the effect to its file (a new effect gets `fx/<its name>.fx.json`).
    pub fn save(&mut self, studio: &mut Studio) {
        let rel = match &self.path {
            Some(p) => p.clone(),
            None => {
                let name = self.effect().map(|e| e.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "new effect".to_string());
                let slug: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
                format!("fx/{slug}.fx.json")
            }
        };
        let full = root_of(studio).join(&rel);
        if let Some(dir) = full.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(&full, &self.text) {
            Ok(()) => {
                self.path = Some(rel.clone());
                self.saved = self.text.clone();
                self.files_at = None;
                studio.toast(format!("saved {rel}"), false);
            }
            Err(e) => studio.toast(format!("cannot save {rel}: {e}"), true),
        }
    }

    /// Puts the effect on the entity that is selected (as its file when it has one, else the JSON itself).
    pub fn use_on_selection(&mut self, studio: &mut Studio) {
        let Some(uid) = studio.selection else {
            studio.toast("select an entity in the scene first", true);
            return;
        };
        let what = match &self.path {
            Some(p) if !self.dirty() => p.clone(),
            _ => self.text.clone(),
        };
        let has = studio.engine.sim.world.entity(uid).is_some_and(|e| studio.engine.sim.world.get::<lunaris_sim::components::ParticleSystem>(e).is_some());
        let result = if has {
            studio.call("set", Json::obj([("uid", Json::from(uid as i64)), ("component", Json::str("ParticleSystem")), ("value", Json::obj([("effect", Json::str(what)), ("time", Json::Float(0.0))]))]))
        } else {
            studio.call("add_component", Json::obj([("uid", Json::from(uid as i64)), ("component", Json::str("ParticleSystem")), ("value", Json::obj([("effect", Json::str(what))]))]))
        };
        if let Err(e) = result {
            studio.toast(e, true);
        }
    }

    /// The effect the selected entity has, as a file or as text (what to open).
    fn selected_effect(&self, studio: &Studio) -> Option<String> {
        let uid = studio.selection?;
        let w = &studio.engine.sim.world;
        let e = w.entity(uid)?;
        let ps = w.get::<lunaris_sim::components::ParticleSystem>(e)?;
        let text = w.strings().get(ps.effect.0).to_string();
        (!text.trim().is_empty()).then_some(text)
    }

    fn open_text_or_file(&mut self, studio: &mut Studio, what: &str) {
        if what.trim_start().starts_with('{') {
            self.path = None;
            self.text = what.to_string();
            self.saved = what.to_string();
            self.emitter = 0;
            self.clock = 0.0;
        } else {
            self.open(studio, what);
        }
    }

    // ------------------------------------------------------------------------------------------------------------ the panels

    /// The left panel: the effect files of the workspace.
    pub fn left_panel(&mut self, ui: &mut egui::Ui, studio: &mut Studio) {
        self.refresh(studio, false);
        ui.horizontal(|ui| {
            if ui.button("New").on_hover_text("A new effect from a template").clicked() {
                if self.dirty() && self.path.is_some() {
                    studio.toast("save the changes first (Save), or Revert them", true);
                } else {
                    self.new_effect();
                }
            }
            if ui.button("Refresh").clicked() {
                self.refresh(studio, true);
            }
        });
        ui.add_space(4.0);
        if self.files.is_empty() {
            ui.label(RichText::new("No effect files in the workspace yet: press New, edit, Save.").small().color(theme::TEXT_DIM));
        }
        let files = self.files.clone();
        egui::ScrollArea::vertical().id_salt("fx-files").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (k, f) in files.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::click());
                let on = self.path.as_deref() == Some(f.as_str());
                let fill = if on { theme::SELECT } else if resp.hovered() { theme::HOVER_ROW } else if k % 2 == 0 { theme::LIST_A } else { theme::LIST_B };
                ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
                let name = f.trim_end_matches(".fx.json");
                ui.painter().with_clip_rect(rect).text(egui::pos2(rect.min.x + 8.0, rect.center().y), Align2::LEFT_CENTER, name, FontId::proportional(12.0), if on { theme::TEXT_BRIGHT } else { theme::TEXT });
                if resp.clicked() && !on {
                    if self.dirty() {
                        studio.toast("save the changes first (Save), or Revert them", true);
                    } else {
                        self.open(studio, f);
                    }
                }
            }
        });
    }

    /// The right panel: the entity's effect, then the emitters of this one, and the curves of the one chosen.
    pub fn details(&mut self, ui: &mut egui::Ui, studio: &mut Studio) {
        ui.label(RichText::new("Selected entity").small().color(theme::TEXT_DIM));
        ui.horizontal(|ui| {
            if ui.button("Use this effect on it").on_hover_text("Puts this effect on the entity that is selected in the scene (its file, once saved)").clicked() {
                self.use_on_selection(studio);
            }
        });
        if let Some(what) = self.selected_effect(studio) {
            let label = if what.trim_start().starts_with('{') { "its own JSON".to_string() } else { what.clone() };
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("has: {label}")).small().color(theme::TEXT_DIM));
                if ui.small_button("Open").clicked() {
                    if self.dirty() {
                        studio.toast("save the changes first (Save), or Revert them", true);
                    } else {
                        self.open_text_or_file(studio, &what);
                    }
                }
            });
        }
        ui.separator();
        let Some(e) = self.parsed.as_ref().ok() else {
            ui.label(RichText::new("The text does not read as an effect yet: see the message under it.").small().color(theme::WARN));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(if e.name.is_empty() { "(no name)" } else { e.name.as_str() }).strong());
            ui.label(RichText::new(format!("{} emitters, budget {}, about {:.0} m", e.emitters.len(), e.budget, e.radius())).small().color(theme::TEXT_DIM));
        });
        let names: Vec<String> = e.emitters.iter().map(|m| format!("{}{}", m.name, if m.gpu { "  [card]" } else { "" })).collect();
        for (i, n) in names.iter().enumerate() {
            if ui.selectable_label(self.emitter == i, n).clicked() {
                self.emitter = i;
            }
        }
        let Some(m) = e.emitters.get(self.emitter) else { return };
        ui.separator();
        let spawn = format!("{} a second{}", m.spawn.rate, if m.spawn.bursts.is_empty() { String::new() } else { format!(", {} burst(s)", m.spawn.bursts.len()) });
        let facts: Vec<String> = vec![
            format!("space: {}{}", if matches!(m.space, lunaris_sim::fx::Space::Local) { "the entity's" } else { "the world's" }, if m.child { " (a child: made by events)" } else { "" }),
            format!("makes: {spawn}; holds at most {}", m.max),
            format!("lives {:.2} to {:.2} s, speed {:.1} to {:.1}, size {:.2} to {:.2}", m.init.life[0], m.init.life[1], m.init.speed[0], m.init.speed[1], m.init.size[0], m.init.size[1]),
            format!("draws: {:?}, {}{}{}", m.render.kind, if m.render.additive { "added" } else { "blended" }, if m.render.nearest { ", hard texels" } else { "" }, if m.render.light.is_some() { ", gives light" } else { "" }),
            format!("events: {}", m.events.len()),
        ];
        for f in facts {
            ui.label(RichText::new(f).small().color(theme::TEXT));
        }
        ui.add_space(6.0);
        let c = &m.curves;
        for (label, g, top) in [("size", &c.size, 2.0f32), ("opacity", &c.alpha, 1.0), ("speed", &c.speed, 2.0), ("light", &c.light, 1.0)] {
            curve_plot(ui, label, g.as_ref(), top);
        }
        colour_strip(ui, "colour", c.color.as_ref(), m.init.color);
    }

    /// The middle of the tab: the controls, the preview, the text and what is wrong with it.
    pub fn show(&mut self, ui: &mut egui::Ui, studio: &mut Studio) {
        self.reparse();
        let root = root_of(studio);
        // the controls
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(self.dirty(), egui::Button::new("Save")).on_hover_text("Write the file (Ctrl+S); a running game reads it again by itself").clicked() {
                self.save(studio);
            }
            if ui.add_enabled(self.dirty() && self.path.is_some(), egui::Button::new("Revert")).clicked() {
                if let Some(p) = self.path.clone() {
                    self.open(studio, &p);
                }
            }
            ui.separator();
            if ui.button(if self.playing { "Pause" } else { "Play" }).clicked() {
                self.playing = !self.playing;
            }
            if ui.button("Restart").clicked() {
                self.clock = 0.0;
            }
            ui.label(RichText::new("speed").small().color(theme::TEXT_DIM));
            ui.add(egui::DragValue::new(&mut self.speed).range(0.05..=4.0).speed(0.02).fixed_decimals(2));
            ui.checkbox(&mut self.repeat, "repeat").on_hover_text("Start an effect that ends over again when its last particle is gone");
            ui.checkbox(&mut self.floor, "floor");
            ui.separator();
            let title = match (&self.path, self.dirty()) {
                (Some(p), true) => format!("{p} (changed)"),
                (Some(p), false) => p.clone(),
                (None, _) => "a new effect (not saved)".to_string(),
            };
            ui.label(RichText::new(title).small().color(if self.dirty() { theme::WARN } else { theme::TEXT_DIM }));
        });
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) && self.dirty() {
            self.save(studio);
        }
        // the clock
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        let effect_ok = self.parsed.as_ref().ok().cloned();
        if self.playing {
            self.clock += dt * self.speed;
        }
        if let (true, Some(e)) = (self.repeat, &effect_ok) {
            let len = suggest_repeat(e);
            if len > 0.0 && self.clock > len {
                self.clock = 0.0;
            }
        }
        // the preview
        let avail = ui.available_size();
        let height = (avail.y * 0.46).clamp(120.0, 420.0);
        let width = (height * PREVIEW.0 as f32 / PREVIEW.1 as f32).min(avail.x);
        if let Some(e) = &effect_ok {
            let dist = (e.radius().clamp(0.5, 60.0) * 1.7).max(2.5);
            self.preview.ensure(&root, &self.text, dist, self.floor);
            let due = self.last_draw_due();
            if due {
                if let Some(f) = self.preview.picture(self.clock) {
                    let img = ColorImage::from_rgba_unmultiplied([f.width as usize, f.height as usize], &f.rgba);
                    match &mut self.preview.handle {
                        Some(h) => h.set(img, TextureOptions::LINEAR),
                        None => self.preview.handle = Some(ui.ctx().load_texture("fx-preview", img, TextureOptions::LINEAR)),
                    }
                }
                self.preview.last_draw = Some(Instant::now());
                let notes = self.preview.notes();
                if !notes.is_empty() {
                    self.notes = notes;
                }
            }
        }
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::ZERO, theme::FIELD);
            match (&self.preview.handle, &effect_ok) {
                (Some(h), Some(_)) => {
                    ui.painter().image(h.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                }
                (Some(h), None) => {
                    // (the last picture of an effect that read, dimmed, while the text is being mended)
                    ui.painter().image(h.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE.gamma_multiply(0.3));
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, "the effect does not read yet", FontId::proportional(12.0), theme::WARN);
                }
                _ => {
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, "no picture: the effect does not read yet", FontId::proportional(12.0), theme::TEXT_DIM);
                }
            }
            ui.painter().text(rect.min + Vec2::new(6.0, 4.0), Align2::LEFT_TOP, format!("{:.2} s   {} particles on the processor", self.clock, self.preview.live), FontId::monospace(10.0), theme::TEXT_DIM);
            ui.vertical(|ui| {
                ui.label(RichText::new("The preview is drawn by the reference renderer on a stage of its own: no soft particles, bloom or card emitters (those are stepped on the processor here).").small().color(theme::TEXT_DIM));
            });
        });
        if self.playing {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
        }
        ui.add_space(4.0);
        // what is wrong with it
        match &self.parsed {
            Err(m) => {
                ui.label(RichText::new(m).color(theme::ERR).monospace().size(12.0));
            }
            Ok(e) => {
                for w in &e.warnings {
                    ui.label(RichText::new(format!("warning: {w}")).color(theme::WARN).small());
                }
                for n in &self.notes {
                    ui.label(RichText::new(n).color(theme::WARN).small());
                }
            }
        }
        // the text
        egui::ScrollArea::vertical().id_salt("fx-text").auto_shrink([false, false]).show(ui, |ui| {
            let edit = egui::TextEdit::multiline(&mut self.text).code_editor().desired_width(f32::INFINITY).desired_rows(14).lock_focus(true);
            ui.add(edit);
        });
    }

    #[cfg(test)]
    pub fn test_files(&self) -> Vec<String> {
        self.files.clone()
    }

    #[cfg(test)]
    pub fn test_has_picture(&self) -> bool {
        self.preview.handle.is_some()
    }

    fn last_draw_due(&self) -> bool {
        self.preview.last_draw.is_none_or(|t| t.elapsed().as_secs_f32() >= FRAME) || self.preview.handle.is_none()
    }
}

/// A curve over the life of a particle drawn small, with its keys; `top` is the value at the top of the box.
fn curve_plot(ui: &mut egui::Ui, label: &str, g: Option<&Gradient<1>>, top: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(300.0), 40.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, theme::FIELD);
    let Some(g) = g else {
        ui.painter().text(rect.min + Vec2::new(6.0, 4.0), Align2::LEFT_TOP, format!("{label}: not set (1 all the way)"), FontId::proportional(10.0), theme::TEXT_DIM);
        return;
    };
    let top = g.keys.iter().fold(top, |m, k| m.max(k.1[0])).max(1e-3);
    let inner = rect.shrink2(Vec2::new(4.0, 5.0));
    let at = |u: f32, v: f32| egui::pos2(inner.min.x + u * inner.width(), inner.max.y - (v / top).clamp(0.0, 1.0) * inner.height());
    let pts: Vec<egui::Pos2> = (0..=48).map(|i| {
        let u = i as f32 / 48.0;
        at(u, g.at(u)[0])
    }).collect();
    ui.painter().add(Shape::line(pts, Stroke::new(1.5, theme::BLUE)));
    for k in &g.keys {
        ui.painter().circle_filled(at(k.0, k.1[0]), 2.5, theme::TEXT_BRIGHT);
    }
    ui.painter().text(rect.min + Vec2::new(6.0, 2.0), Align2::LEFT_TOP, format!("{label} (to {top:.1})"), FontId::proportional(10.0), theme::TEXT_DIM);
}

/// The colour over a life as a strip (display colours, clamped), over a chequer of dark and light so that opacity shows.
fn colour_strip(ui: &mut egui::Ui, label: &str, g: Option<&Gradient<4>>, base: [f32; 4]) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(300.0), 26.0), Sense::hover());
    let n = 48;
    for i in 0..n {
        let u = i as f32 / (n - 1) as f32;
        let k = g.map_or([1.0; 4], |g| g.at(u));
        let c = [base[0] * k[0], base[1] * k[1], base[2] * k[2], base[3] * k[3]];
        let x0 = rect.min.x + rect.width() * i as f32 / n as f32;
        let x1 = rect.min.x + rect.width() * (i + 1) as f32 / n as f32;
        let r = egui::Rect::from_min_max(egui::pos2(x0, rect.min.y), egui::pos2(x1 + 0.5, rect.max.y));
        ui.painter().rect_filled(r, CornerRadius::ZERO, if i % 2 == 0 { Color32::from_gray(30) } else { Color32::from_gray(60) });
        let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
        ui.painter().rect_filled(r, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(to8(c[0]), to8(c[1]), to8(c[2]), to8(c[3])));
    }
    ui.painter().text(rect.min + Vec2::new(6.0, 2.0), Align2::LEFT_TOP, label, FontId::proportional(10.0), Color32::from_gray(200));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_is_an_effect_without_a_warning_and_a_finite_effect_has_a_length_to_play() {
        let e = parse(TEMPLATE).expect("the template reads");
        assert!(e.warnings.is_empty(), "{:?}", e.warnings);
        assert_eq!(suggest_repeat(&e), 0.0, "it loops: it never ends");
        let one_shot = parse(r#"{"emitters":[{"loop":false,"delay":0.2,"spawn":{"bursts":[{"time":0.1,"count":5}]},"init":{"life":[1,2]}}]}"#).unwrap();
        assert!((suggest_repeat(&one_shot) - (0.2 + 0.1 + 0.0 + 2.0 + 0.5)).abs() < 1e-5);
    }

    #[test]
    fn effect_files_are_found_under_the_workspace_but_not_in_hidden_or_build_folders() {
        let dir = std::env::temp_dir().join(format!("lunaris_fx_scan_{}", std::process::id()));
        for f in ["a.fx.json", "fx/b.fx.json", "game/fx/deep/c.fx.json", ".hidden/d.fx.json", "target/e.fx.json", "fx/not.json"] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "{}").unwrap();
        }
        assert_eq!(scan(&dir), ["a.fx.json", "fx/b.fx.json", "game/fx/deep/c.fx.json"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_preview_plays_the_effect_on_its_stage_and_a_changed_text_builds_the_stage_again() {
        let mut p = Preview::new();
        let root = std::env::temp_dir();
        assert!(p.ensure(&root, TEMPLATE, 6.0, true));
        assert!(!p.ensure(&root, TEMPLATE, 6.0, true), "the same stage is kept (the particles go on)");
        let early = p.picture(0.0).expect("a picture");
        // (half a second on: a loop that is looked at again after more than it takes to fill up starts over, which is the same picture)
        let later = p.picture(0.5).expect("a picture");
        assert!(p.notes().is_empty());
        assert_eq!((later.width, later.height), PREVIEW);
        assert_ne!(early.hash(), later.hash(), "the sparks fly");
        assert!(p.live > 20, "{} particles", p.live);
        assert!(p.ensure(&root, &TEMPLATE.replace("40", "80"), 6.0, true), "another text, another stage");
        assert!(p.ensure(&root, TEMPLATE, 6.0, false), "no floor, another stage");
    }
}
