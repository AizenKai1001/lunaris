//! The window: menu bar, toolbar, hierarchy on the left, the scene in the middle, the inspector on the right, project, console and systems
//! below, a status bar. Play mode runs the simulation in the editor.
use crate::chrome;
use crate::dialog;
use crate::icons::{self, Icon};
use crate::hierarchy::{create_menu, HierarchyUi};
use crate::input;
use crate::inspector::InspectorUi;
use crate::console::ConsoleUi;
use crate::history_ui::HistoryUi;
use crate::streaming_ui::StreamingUi;
use crate::panels::SystemsUi;
use crate::profiler::ProfilerUi;
use crate::sequencer::SequencerUi;
use crate::project_settings::{ProjectSettings, QUALITIES, SIZE_RANGE};
use crate::blueprint_ui::BlueprintEditor;
use crate::canvas_ui::CanvasUi;
use crate::script_ui::ScriptEditor;
use crate::ui_designer::UiDesigner;
use crate::project::ProjectUi;
use crate::state::{BottomTab, Center, LeftTab, Mode, Options, Studio};
use crate::theme;
use crate::viewport::{Tool, Viewport};
use eframe::egui::{self, Align, Color32, Frame, Key, Layout, Margin, Modifiers, Panel, Rect, RichText, Stroke, UiBuilder, Vec2};
use lunaris_api::Director;
use lunaris_audio::{Mixer, Output};
use lunaris_json::Json;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The sound while playing: the director that turns events into voices and the sound card it plays into.
struct Sound {
    director: Director,
    _output: Output,
}

#[derive(Clone, Debug, PartialEq)]
enum Pending {
    Quit,
    NewScene,
    OpenDialog,
    Open(String),
}

struct Shot {
    path: std::path::PathBuf,
    at: Instant,
    requested: bool,
    frames: u32,
}

pub struct App {
    pub(crate) studio: Studio,
    pub(crate) viewport: Viewport,
    pub(crate) hierarchy: HierarchyUi,
    pub(crate) place: crate::place::PlaceUi,
    pub(crate) inspector: InspectorUi,
    pub(crate) project: ProjectUi,
    pub(crate) console: ConsoleUi,
    pub(crate) history: HistoryUi,
    pub(crate) streaming: StreamingUi,
    pub(crate) systems: SystemsUi,
    pub(crate) profiler: ProfilerUi,
    pub(crate) sequencer: SequencerUi,
    pub(crate) effects: crate::effects::EffectsUi,
    /// The project's settings (`project.lunaris.json`), the Project Settings window, and what was last saved (to save when the window closes after a change).
    pub(crate) settings: ProjectSettings,
    pub(crate) settings_saved: ProjectSettings,
    pub(crate) settings_open: bool,
    settings_scenes: Vec<String>,
    pub(crate) script: ScriptEditor,
    pub(crate) blueprint: BlueprintEditor,
    pub(crate) designer: UiDesigner,
    pub(crate) canvas: CanvasUi,
    pub(crate) show_hierarchy: bool,
    pub(crate) show_inspector: bool,
    pub(crate) show_bottom: bool,
    pub(crate) sound: Option<Sound>,
    pub(crate) last_title: String,
    pub(crate) acc: f32,
    /// The state hash the status bar shows, with what it was taken at (revision, tick) and when: hashing a big scene on every frame is not free, so it is redone only after a change and, while the game runs, four times a second.
    pub(crate) status_hash: (String, (u64, u64), Option<Instant>),
    pub(crate) held: BTreeSet<String>,
    pub(crate) confirm: Option<Pending>,
    pub(crate) shot: Option<Shot>,
    pub(crate) first_frame: bool,
    pub(crate) allow_close: bool,
    pub(crate) last_selection: Option<u64>,
}

impl App {
    pub fn new(ctx: &egui::Context, opts: Options) -> App {
        theme::apply(ctx);
        let shot = opts.shot.clone().map(|path| Shot { path, at: Instant::now() + Duration::from_secs_f32(opts.shot_delay.max(0.5)), requested: false, frames: 0 });
        let start_play = opts.play;
        let opts_root = opts.root.clone();
        let select = opts.select.clone();
        let mut studio = Studio::new(opts.clone());
        studio.refresh_rows();
        if let Some(s) = select {
            let uid = s.parse::<u64>().ok().or_else(|| studio.all.iter().find(|r| r.name == s).map(|r| r.uid));
            studio.select(uid);
        }
        let mut viewport = Viewport::new(opts.root.clone(), &opts.quality);
        match opts.view.as_deref() {
            Some("2d") => viewport.kind = crate::viewport::ViewKind::Scene2D,
            Some("game") => viewport.kind = crate::viewport::ViewKind::Game,
            _ => {}
        }
        if let Some(name) = &opts.system {
            studio.tab = BottomTab::Systems;
            studio.open_system(name);
        }
        match opts.tab.as_deref() {
            Some("console") => studio.tab = BottomTab::Console,
            Some("systems") => studio.tab = BottomTab::Systems,
            Some("profiler") => studio.tab = BottomTab::Profiler,
            Some("history") => studio.tab = BottomTab::History,
            Some("streaming") => studio.tab = BottomTab::Streaming,
            Some("script") => studio.center = Center::Script,
            Some("blueprint") => studio.center = Center::Blueprint,
            Some("ui") => studio.center = Center::Ui,
            Some("canvas") => studio.center = Center::Canvas,
            Some("sequencer") => studio.center = Center::Sequencer,
            Some("effects") => studio.center = Center::Effects,
            _ => {}
        }
        let mut script = ScriptEditor::default();
        let mut blueprint = BlueprintEditor::default();
        if studio.center == Center::Blueprint {
            if let Some(name) = &opts.system {
                blueprint.open(&mut studio, name);
            }
        }
        if let Some(file) = &opts.script {
            studio.center = Center::Script;
            script.open_path(&mut studio, std::path::Path::new(file));
        }
        let mut canvas = CanvasUi::default();
        canvas.set_root(&opts.root);
        if let Some(file) = &opts.canvas {
            studio.center = Center::Canvas;
            canvas.open(file);
        }
        let mut app = App {
            studio,
            viewport,
            hierarchy: HierarchyUi::default(),
            place: crate::place::PlaceUi::default(),
            inspector: InspectorUi::default(),
            project: ProjectUi::default(),
            console: ConsoleUi::default(),
            history: HistoryUi::default(),
            streaming: StreamingUi::default(),
            systems: SystemsUi::default(),
            profiler: ProfilerUi::default(),
            sequencer: SequencerUi::default(),
            effects: crate::effects::EffectsUi::default(),
            settings_saved: ProjectSettings::load(&opts_root),
            settings: ProjectSettings::load(&opts_root),
            settings_open: false,
            settings_scenes: Vec::new(),
            script,
            blueprint,
            designer: UiDesigner::default(),
            canvas,
            show_hierarchy: true,
            show_inspector: true,
            show_bottom: true,
            sound: None,
            last_title: String::new(),
            acc: 0.0,
            status_hash: (String::new(), (u64::MAX, u64::MAX), None),
            held: BTreeSet::new(),
            confirm: None,
            shot,
            first_frame: true,
            allow_close: false,
            last_selection: None,
        };
        app.frame_all();
        if start_play {
            app.play();
        }
        app
    }

    /// Show the whole scene: the view looks at the middle of what is there.
    fn frame_all(&mut self) {
        let ps: Vec<[f32; 3]> = self.studio.all.iter().filter_map(|r| r.pos).collect();
        if ps.is_empty() {
            return;
        }
        let n = ps.len() as f32;
        let c = [ps.iter().map(|p| p[0]).sum::<f32>() / n, ps.iter().map(|p| p[1]).sum::<f32>() / n, ps.iter().map(|p| p[2]).sum::<f32>() / n];
        let r = ps.iter().map(|p| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt()).fold(2.0f32, f32::max);
        self.viewport.focus(c, (r * 0.45).clamp(2.0, 80.0));
    }

    pub(crate) fn play(&mut self) {
        // the game runs in the Game view: leave the editors
        self.studio.center = Center::View;
        let was_edit = self.studio.mode == Mode::Edit;
        self.studio.play();
        if was_edit && self.studio.mode == Mode::Play {
            self.acc = 0.0;
            self.held.clear();
            if !self.studio.opts.mute {
                let mixer = Arc::new(Mutex::new(Mixer::new()));
                match Output::open(mixer.clone()) {
                    Ok(output) => {
                        let mut director = Director::new(mixer);
                        director.spawn_tracer();
                        self.sound = Some(Sound { director, _output: output });
                    }
                    Err(e) => {
                        self.sound = None;
                        self.studio.log(crate::state::LogKind::Info, format!("no sound: {e}"), String::new(), 0.0);
                    }
                }
            }
        }
    }

    pub(crate) fn stop(&mut self) {
        self.release_input();
        self.sound = None;
        self.studio.stop();
        self.viewport.refresh();
    }

    /// Stop playing and keep what the selected entities were doing (`Studio::stop_keeping_selected`).
    pub(crate) fn stop_keeping(&mut self) -> usize {
        self.release_input();
        self.sound = None;
        let kept = self.studio.stop_keeping_selected();
        self.viewport.refresh();
        kept
    }

    fn release_input(&mut self) {
        if self.held.is_empty() {
            return;
        }
        let values: Vec<(String, Json)> = std::mem::take(&mut self.held).into_iter().map(|k| (k, Json::Bool(false))).collect();
        let _ = self.studio.engine.call("input", &Json::obj([("values", Json::Obj(values))]));
    }

    /// Advance the simulation by the time that has passed (fixed steps), play the sounds of what happened, and pass the game its input.
    pub(crate) fn run_simulation(&mut self, ctx: &egui::Context) {
        if self.studio.mode != Mode::Play {
            return;
        }
        // input first, so that this frame's steps see it
        if self.viewport.game_input {
            let now = input::held(ctx);
            let rect = self.viewport.last_rect;
            let mut values: Vec<(String, Json)> = Vec::new();
            for k in now.difference(&self.held) {
                values.push((k.clone(), Json::Bool(true)));
            }
            for k in self.held.difference(&now) {
                values.push((k.clone(), Json::Bool(false)));
            }
            if let Some(p) = ctx.input(|i| i.pointer.hover_pos()) {
                if rect.width() > 1.0 {
                    values.push(("mouse_x".to_string(), Json::from(((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0) as f64)));
                    values.push(("mouse_y".to_string(), Json::from(((p.y - rect.min.y) / rect.height()).clamp(0.0, 1.0) as f64)));
                }
            }
            if !values.is_empty() {
                let _ = self.studio.engine.call("input", &Json::obj([("values", Json::Obj(values))]));
            }
            self.held = now;
        } else {
            self.release_input();
        }
        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        let step = self.studio.engine.sim.dt.max(1e-4);
        self.acc += dt;
        let n = ((self.acc / step) as u32).min(8);
        self.acc -= n as f32 * step;
        if self.acc > step * 8.0 {
            self.acc = 0.0;
        }
        let first_tick = self.studio.engine.sim.tick;
        // with a breakpoint in the open Blueprint the game steps a tick at a time and stops when the selected entity is about to run the statement that has it
        let breaks = self.blueprint.has_breakpoints();
        for _ in 0..n {
            self.studio.engine.sim.step();
            if breaks && self.blueprint.check_breakpoints(&mut self.studio).is_some() {
                self.studio.pause();
                break;
            }
        }
        // the stages of the tick are timed only while the Profiler tab is showing
        let profiling = self.show_bottom && self.studio.tab == BottomTab::Profiler;
        self.profiler.collect(&mut self.studio.engine.sim, profiling);
        if let Some(s) = &mut self.sound {
            let events: Vec<lunaris_sim::Event> = self.studio.engine.sim.events.iter().filter(|e| e.tick >= first_tick).cloned().collect();
            s.director.update(&mut self.studio.engine, &events);
        }
        ctx.request_repaint();
    }

    fn step_once(&mut self) {
        if self.studio.mode == Mode::Paused {
            self.studio.engine.sim.step();
            if self.blueprint.has_breakpoints() {
                self.blueprint.check_breakpoints(&mut self.studio);
            }
            self.studio.rows_dirty = true;
            self.viewport.refresh();
        }
    }

    // ------------------------------------------------------------------------------------------------------------- actions

    fn do_open_dialog(&mut self) {
        if dialog::available() {
            if let Some(p) = dialog::open_scene(&self.studio.opts.root) {
                self.studio.open_scene(&p.to_string_lossy());
                self.after_load();
            }
        } else {
            self.studio.tab = BottomTab::Project;
            self.studio.toast("Double-click a scene in the Project panel", false);
        }
    }

    fn after_load(&mut self) {
        self.studio.refresh_rows();
        self.frame_all();
        self.viewport.refresh();
    }

    fn save_as_dialog(&mut self) {
        let default = self.studio.scene.as_deref().and_then(|s| s.rsplit('/').next()).unwrap_or("scene.lunaris.json").to_string();
        if let Some(p) = dialog::save_scene(&self.studio.opts.root, &default) {
            self.studio.save_as(&p.to_string_lossy());
        }
    }

    /// Build the game: the program, the scene as it is now and the files it uses, into a folder of the user's choosing.
    pub(crate) fn build_player(&mut self, run_after: bool) {
        if self.studio.mode != Mode::Edit {
            self.studio.toast("Stop playing first: the game is built from the scene as you edit it", true);
            return;
        }
        let settings = self.settings.clone();
        let stem = Some(settings.name.trim().to_string()).filter(|s| !s.is_empty()).or_else(|| self.studio.scene.as_deref().and_then(|s| s.rsplit('/').next()).map(|s| s.trim_end_matches(".lunaris.json").trim_end_matches(".json").to_string()).filter(|s| !s.is_empty())).unwrap_or_else(|| "Game".to_string());
        let dir = self.studio.opts.root.join("Builds").join(&stem);
        let _ = std::fs::create_dir_all(&dir);
        let Some(exe_path) = dialog::save_program(&dir, &format!("{stem}.exe")) else { return };
        let player = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("lunaris-player.exe"))).filter(|p| p.is_file()).unwrap_or_else(|| self.studio.opts.root.join("target").join("release").join("lunaris-player.exe"));
        // the game starts in the scene the project settings name; with none named, in the one that is open (as it is now, saved or not)
        let (json, scene_rel) = if !settings.scene.is_empty() && self.studio.scene.as_deref() != Some(settings.scene.as_str()) {
            match std::fs::read_to_string(self.studio.opts.root.join(&settings.scene)) {
                Ok(text) => (text, settings.scene.clone()),
                Err(e) => {
                    self.studio.toast(format!("the start scene {} (Project Settings) cannot be read: {e}", settings.scene), true);
                    return;
                }
            }
        } else {
            let Some(doc) = self.studio.try_call("save_scene", Json::Null) else { return };
            (doc.to_string_pretty().unwrap_or_default(), self.studio.scene.clone().unwrap_or_else(|| format!("scenes/{stem}.lunaris.json")))
        };
        match crate::package::build(&self.studio.opts.root, &json, &scene_rel, &player, &exe_path, &settings) {
            Ok(b) => {
                let note = format!("Built {} ({} files, {:.1} MB){}", b.exe.display(), b.files, b.bytes as f64 / 1e6, if b.missing.is_empty() { String::new() } else { format!("; could not copy: {}", b.missing.join(", ")) });
                self.studio.log(crate::state::LogKind::Info, note.clone(), String::new(), 0.0);
                self.studio.toast(note, false);
                if run_after {
                    if let Err(e) = std::process::Command::new(&b.exe).current_dir(&b.folder).spawn() {
                        self.studio.toast(format!("cannot start the game: {e}"), true);
                    }
                } else {
                    let _ = std::process::Command::new("explorer").arg(&b.folder).spawn();
                }
            }
            Err(e) => self.studio.toast(e, true),
        }
    }

    fn save(&mut self) {
        if self.studio.scene.is_none() {
            self.save_as_dialog();
        } else {
            self.studio.save();
        }
    }

    /// Run something that would lose unsaved changes: ask first when there are some.
    fn guarded(&mut self, what: Pending) {
        if self.studio.dirty && self.studio.mode == Mode::Edit {
            self.confirm = Some(what);
        } else {
            self.perform(what);
        }
    }

    fn perform(&mut self, what: Pending) {
        match what {
            Pending::Quit => {
                self.allow_close = true;
                self.studio.quit = true;
            }
            Pending::NewScene => {
                self.studio.new_scene();
                self.frame_all();
                self.viewport.refresh();
            }
            Pending::OpenDialog => self.do_open_dialog(),
            Pending::Open(p) => {
                self.studio.open_scene(&p);
                self.after_load();
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        // the Script tab has its own keys (Ctrl+S applies the script, Ctrl+P opens files, Ctrl+Z undoes typing)
        if self.studio.center != Center::View {
            return;
        }
        let cmd = Modifiers::COMMAND;
        let rmb = ctx.input(|i| i.pointer.secondary_down());
        let (tool_q, tool_w, tool_e, tool_r) = ctx.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::Q), i.consume_key(Modifiers::NONE, Key::W), i.consume_key(Modifiers::NONE, Key::E), i.consume_key(Modifiers::NONE, Key::R)));
        if !rmb && !(self.studio.mode == Mode::Play && self.viewport.game_input) {
            for (k, tool) in [(tool_q, Tool::Hand), (tool_w, Tool::Move), (tool_e, Tool::Rotate), (tool_r, Tool::Scale)] {
                if k {
                    self.viewport.tool = tool;
                }
            }
        }
        let (undo, redo, save, open, new, dup, play, del, focus, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(cmd, Key::Z),
                i.consume_key(cmd, Key::Y) || i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
                i.consume_key(cmd, Key::S),
                i.consume_key(cmd, Key::O),
                i.consume_key(cmd, Key::N),
                i.consume_key(cmd, Key::D),
                i.consume_key(cmd, Key::P),
                i.consume_key(Modifiers::NONE, Key::Delete),
                i.consume_key(Modifiers::NONE, Key::F),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        let playing_game = self.studio.mode == Mode::Play && self.viewport.game_input;
        if play {
            if self.studio.mode == Mode::Edit {
                self.play();
            } else {
                self.stop();
            }
        }
        if playing_game {
            return;
        }
        // camera bookmarks: Ctrl+1 to Ctrl+9 save the view, 1 to 9 return to it (not while the right button flies the camera)
        if !rmb {
            const NUMS: [Key; crate::viewport::BOOKMARKS] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
            for (n, key) in NUMS.into_iter().enumerate() {
                if ctx.input_mut(|i| i.consume_key(cmd, key)) {
                    self.viewport.set_bookmark(n);
                } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                    self.viewport.go_bookmark(n);
                }
            }
        }
        if undo {
            self.studio.undo();
        }
        if redo {
            self.studio.redo();
        }
        if save {
            self.save();
        }
        if open {
            self.guarded(Pending::OpenDialog);
        }
        if new {
            self.guarded(Pending::NewScene);
        }
        if dup {
            self.studio.duplicate_selected();
        }
        if del {
            self.studio.delete_selected();
        }
        if focus {
            self.viewport.focus_selection(&self.studio);
        }
        if esc && self.studio.brush.is_some() {
            // Esc puts the paint brush down (a stroke in the making is dropped, not spawned)
            self.studio.brush = None;
            self.viewport.cancel_stroke();
        } else if esc && self.studio.mode != Mode::Edit {
            self.stop();
        }
    }

    // ------------------------------------------------------------------------------------------------------------- panels

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let spawn_at = self.viewport.target;
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New Scene          Ctrl+N").clicked() {
                    self.guarded(Pending::NewScene);
                    ui.close();
                }
                if ui.button("Open Scene...      Ctrl+O").clicked() {
                    self.guarded(Pending::OpenDialog);
                    ui.close();
                }
                ui.separator();
                if ui.button("Save                   Ctrl+S").clicked() {
                    self.save();
                    ui.close();
                }
                if ui.button("Save As...").clicked() {
                    self.save_as_dialog();
                    ui.close();
                }
                ui.separator();
                if ui.button("Build Windows Player...").on_hover_text("Make a folder with the game as a program: double-click it to play").clicked() {
                    self.build_player(false);
                    ui.close();
                }
                if ui.button("Build and Run...").clicked() {
                    self.build_player(true);
                    ui.close();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    self.guarded(Pending::Quit);
                    ui.close();
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.button("Undo                Ctrl+Z").clicked() {
                    self.studio.undo();
                    ui.close();
                }
                if ui.button("Redo                Ctrl+Y").clicked() {
                    self.studio.redo();
                    ui.close();
                }
                ui.separator();
                let has = self.studio.selection.is_some();
                if ui.add_enabled(has, egui::Button::new("Duplicate        Ctrl+D")).clicked() {
                    self.studio.duplicate_selected();
                    ui.close();
                }
                if ui.add_enabled(has, egui::Button::new("Delete               Del")).clicked() {
                    self.studio.delete_selected();
                    ui.close();
                }
                if ui.button("Select None").clicked() {
                    self.studio.select(None);
                    ui.close();
                }
                ui.separator();
                if ui.button("Project Settings...").on_hover_text("The game's name, start scene, quality, window size and seed").clicked() {
                    self.open_settings();
                    ui.close();
                }
            });
            ui.menu_button("Entity", |ui| create_menu(ui, &mut self.studio, spawn_at));
            ui.menu_button("View", |ui| {
                ui.checkbox(&mut self.show_hierarchy, "Hierarchy");
                ui.checkbox(&mut self.show_inspector, "Inspector");
                ui.checkbox(&mut self.show_bottom, "Project / Console / Systems / Profiler");
                ui.separator();
                if ui.button("Frame selection    F").clicked() {
                    self.viewport.focus_selection(&self.studio);
                    ui.close();
                }
                if ui.button("Frame the scene").clicked() {
                    self.frame_all();
                    ui.close();
                }
            });
            ui.menu_button("Play", |ui| {
                let mode = self.studio.mode;
                if ui.add_enabled(mode != Mode::Play, egui::Button::new("Play                 Ctrl+P")).clicked() {
                    if mode == Mode::Paused {
                        self.studio.play();
                    } else {
                        self.play();
                    }
                    ui.close();
                }
                if ui.add_enabled(mode != Mode::Edit, egui::Button::new("Pause")).clicked() {
                    self.studio.pause();
                    ui.close();
                }
                if ui.add_enabled(mode == Mode::Paused, egui::Button::new("Step")).clicked() {
                    self.step_once();
                    ui.close();
                }
                if ui.add_enabled(mode != Mode::Edit, egui::Button::new("Stop                 Esc")).clicked() {
                    self.stop();
                    ui.close();
                }
                if ui.add_enabled(mode != Mode::Edit && !self.studio.selected.is_empty(), egui::Button::new("Stop and keep the selected entities' changes")).on_hover_text("Select entities in the Hierarchy while the game runs or is paused (move them, let the game change them); this stops and writes what they have now into the scene, in one undo step").clicked() {
                    self.stop_keeping();
                    ui.close();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Documentation folder").clicked() {
                    let docs = self.studio.opts.root.join("docs");
                    let _ = std::process::Command::new("explorer").arg(docs).spawn();
                    ui.close();
                }
                ui.label(RichText::new(format!("Lunaris Editor, engine {}", lunaris_api::VERSION)).small().color(theme::TEXT_DIM));
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let mode = self.studio.mode;
        // left: the tools of the scene view
        let left = Rect::from_min_size(rect.min, Vec2::new(rect.width() * 0.4, rect.height()));
        ui.scope_builder(UiBuilder::new().max_rect(left).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for (tool, icon, tip) in [(Tool::Hand, Icon::Hand, "Hand tool (Q): drag to move the view"), (Tool::Move, Icon::Move, "Move (W)"), (Tool::Rotate, Icon::Rotate, "Rotate (E)"), (Tool::Scale, Icon::Scale, "Scale (R)")] {
                if icons::button(ui, icon, self.viewport.tool == tool, tip).clicked() {
                    self.viewport.tool = tool;
                }
            }
            chrome::rule(ui);
            ui.add_sized([56.0, 22.0], egui::Button::new("Pivot")).on_hover_text("Tools work around the object's own origin");
            ui.add_sized([56.0, 22.0], egui::Button::new("Global")).on_hover_text("Handles are aligned to the world");
        });
        // middle: play, pause, step
        let mid = Rect::from_center_size(rect.center(), Vec2::new(120.0, rect.height()));
        ui.scope_builder(UiBuilder::new().max_rect(mid).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let play_fill = if mode != Mode::Edit { theme::PLAY_BLUE } else { theme::BUTTON };
            if icons::colored_button(ui, Icon::Play, play_fill, true, if mode == Mode::Edit { "Play (Ctrl+P): run the simulation here; the scene comes back when you stop" } else { "Stop (Esc)" }).clicked() {
                if mode == Mode::Edit {
                    self.play();
                } else {
                    self.stop();
                }
            }
            let pause_fill = if mode == Mode::Paused { theme::PLAY_BLUE } else { theme::BUTTON };
            if icons::colored_button(ui, Icon::Pause, pause_fill, mode != Mode::Edit, "Pause").clicked() {
                self.studio.pause();
            }
            if icons::colored_button(ui, Icon::Step, theme::BUTTON, mode == Mode::Paused, "Step one tick (1/60 s)").clicked() {
                self.step_once();
            }
        });
        // right: what is going on
        let right = Rect::from_min_max(egui::pos2(rect.max.x - rect.width() * 0.35, rect.min.y), rect.max);
        ui.scope_builder(UiBuilder::new().max_rect(right).layout(Layout::right_to_left(Align::Center)), |ui| {
            ui.add_space(6.0);
            if ui.add_sized([46.0, 22.0], egui::Button::new("Redo")).clicked() {
                self.studio.redo();
            }
            if ui.add_sized([46.0, 22.0], egui::Button::new("Undo")).clicked() {
                self.studio.undo();
            }
            if mode != Mode::Edit {
                ui.label(RichText::new(format!("tick {}", self.studio.engine.sim.tick)).small().color(theme::TEXT_DIM));
                if let Some(s) = &self.sound {
                    ui.label(RichText::new(format!("sound: {} voices", s.director.mixer.lock().map(|m| m.active()).unwrap_or(0))).small().color(theme::TEXT_DIM));
                }
                ui.label(RichText::new(if mode == Mode::Play { "Playing" } else { "Paused" }).color(theme::BLUE));
            }
        });
        ui.advance_cursor_after_rect(rect);
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            // Unreal's status bar starts with two buttons that open the Content Drawer and the Output Log: here they open (and close) the bottom panel on the Project tab and on the Console
            for (label, tab) in [("Content Drawer", BottomTab::Project), ("Output Log", BottomTab::Console)] {
                let on = self.show_bottom && self.studio.tab == tab;
                let text = RichText::new(label).size(12.0).color(if on { theme::TEXT_BRIGHT } else { theme::TEXT });
                let button = egui::Button::new(text).fill(if on { Color32::from_rgb(38, 38, 38) } else { Color32::TRANSPARENT }).stroke(Stroke::NONE).min_size(Vec2::new(0.0, 20.0));
                if ui.add(button).on_hover_text(if on { "Close the bottom panel" } else { "Open it here" }).clicked() {
                    self.show_bottom = !on;
                    if !on {
                        self.studio.tab = tab;
                    }
                }
            }
            chrome::rule(ui);
            if let Some((t, at, err)) = &self.studio.toast {
                if at.elapsed() < Duration::from_secs(6) {
                    ui.label(RichText::new(t).color(if *err { theme::ERR } else { theme::OK }));
                } else {
                    self.studio.toast = None;
                }
            } else {
                ui.label(RichText::new(self.studio.scene.as_deref().unwrap_or("Untitled scene")).color(theme::TEXT_DIM));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let key = (self.studio.live_revision, self.studio.engine.sim.tick);
                let stale = self.status_hash.1 != key;
                let due = self.status_hash.2.is_none_or(|at| self.studio.mode == Mode::Edit || at.elapsed() >= Duration::from_millis(250));
                if stale && due {
                    self.status_hash = (format!("{:016x}", self.studio.engine.sim.state_hash())[..8].to_string(), key, Some(Instant::now()));
                }
                ui.label(RichText::new(format!("{} entities  |  tick {}  |  hash {}", self.studio.total_entities, self.studio.engine.sim.tick, self.status_hash.0)).small().color(theme::TEXT_DIM));
            });
        });
    }

    /// Show the Project Settings window (the scenes of the workspace are listed again each time it opens).
    pub(crate) fn open_settings(&mut self) {
        self.settings_open = true;
        let listing = self.studio.engine.call("files", &Json::obj([("recursive", Json::Bool(true)), ("limit", Json::from(2000))])).ok();
        let mut scenes: Vec<String> = listing
            .as_ref()
            .and_then(|f| f.get("files").and_then(|x| x.as_array()))
            .unwrap_or(&[])
            .iter()
            .filter(|e| e.get("kind").and_then(|k| k.as_str()) == Some("scene"))
            .filter_map(|e| e.get("path").and_then(|p| p.as_str()).map(str::to_string))
            .collect();
        scenes.sort();
        self.settings_scenes = scenes;
    }

    /// Write the settings to `project.lunaris.json` when they have changed since they were last written.
    pub(crate) fn save_settings(&mut self) {
        if self.settings == self.settings_saved {
            return;
        }
        match self.settings.save(&self.studio.opts.root) {
            Ok(()) => {
                self.settings_saved = self.settings.clone();
                self.studio.toast(format!("Project settings saved ({})", crate::project_settings::FILE), false);
            }
            Err(e) => self.studio.toast(e, true),
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = true;
        let mut close = false;
        egui::Window::new("Project Settings").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            egui::Grid::new("project-settings").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("Game name");
                ui.add(egui::TextEdit::singleline(&mut self.settings.name).hint_text("the name of the scene").desired_width(260.0));
                ui.end_row();
                ui.label("Start scene");
                let shown = if self.settings.scene.is_empty() { "the open scene".to_string() } else { self.settings.scene.clone() };
                egui::ComboBox::from_id_salt("settings-scene").selected_text(shown).width(260.0).show_ui(ui, |ui| {
                    if ui.selectable_label(self.settings.scene.is_empty(), "the open scene").clicked() {
                        self.settings.scene.clear();
                    }
                    for s in self.settings_scenes.clone() {
                        if ui.selectable_label(self.settings.scene == s, &s).clicked() {
                            self.settings.scene = s;
                        }
                    }
                });
                ui.end_row();
                ui.label("Quality");
                egui::ComboBox::from_id_salt("settings-quality").selected_text(self.settings.quality.clone()).show_ui(ui, |ui| {
                    for q in QUALITIES {
                        if ui.selectable_label(self.settings.quality == q, q).clicked() {
                            self.settings.quality = q.to_string();
                        }
                    }
                });
                ui.end_row();
                ui.label("Window size");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.settings.size.0).range(SIZE_RANGE).suffix(" px"));
                    ui.label("x");
                    ui.add(egui::DragValue::new(&mut self.settings.size.1).range(SIZE_RANGE).suffix(" px"));
                });
                ui.end_row();
                ui.label("Seed");
                ui.add(egui::DragValue::new(&mut self.settings.seed).speed(1.0));
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.label(RichText::new(format!("Saved in {} in the workspace and used by File > Build Windows Player.", crate::project_settings::FILE)).small().color(theme::TEXT_DIM));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    self.save_settings();
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
                if self.settings != self.settings_saved {
                    ui.label(RichText::new("changes not saved yet").small().color(theme::WARN));
                }
            });
        });
        if !open || close {
            // closing the window keeps what was changed
            self.save_settings();
            self.settings_open = false;
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(p) = self.confirm.clone() else { return };
        let mut choice: Option<u8> = None;
        egui::Window::new("Unsaved changes").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("The scene has changes that are not saved.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(0);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(1);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(2);
                }
            });
        });
        match choice {
            Some(0) => {
                self.save();
                if !self.studio.dirty {
                    self.confirm = None;
                    self.perform(p);
                }
            }
            Some(1) => {
                self.confirm = None;
                self.studio.dirty = false;
                self.perform(p);
            }
            Some(_) => self.confirm = None,
            None => {}
        }
    }

    fn handle_shot(&mut self, ctx: &egui::Context) {
        let Some(shot) = self.shot.as_mut() else { return };
        shot.frames += 1;
        if !shot.requested && Instant::now() >= shot.at && shot.frames > 10 {
            shot.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        } else if !shot.requested {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let image = ctx.input(|i| i.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }));
        if let Some(img) = image {
            let [w, h] = img.size;
            let path = shot.path.clone();
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            match lunaris_render::encode_png(w as u32, h as u32, img.as_raw()).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
                Ok(()) => println!("saved a picture of the window to {}", path.display()),
                Err(e) => eprintln!("could not save the picture: {e}"),
            }
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl App {
    /// Everything the window shows, once per frame (what `eframe` calls, and what the tests call without a window).
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.first_frame {
            self.first_frame = false;
            ctx.request_repaint();
        }
        // closing the window with unsaved changes asks first
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if (self.studio.dirty || self.script.is_dirty() || self.blueprint.is_dirty()) && self.studio.mode == Mode::Edit {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm = Some(Pending::Quit);
            } else {
                self.stop();
            }
        }
        if self.studio.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.run_simulation(&ctx);
        self.studio.sync();
        if let Some(name) = self.studio.open_script.take() {
            self.studio.center = Center::Script;
            self.script.open_system(&mut self.studio, &name);
        }
        if let Some(name) = self.studio.open_blueprint.take() {
            self.studio.center = Center::Blueprint;
            self.blueprint.open(&mut self.studio, &name);
        }
        self.shortcuts(&ctx);
        if let Some(p) = self.project.open_scene.take() {
            self.guarded(Pending::Open(p));
        }
        let title = self.studio.title();
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }

        let bar = |fill: Color32, h: f32| Frame::new().fill(fill).stroke(Stroke::new(1.0, theme::LINE)).inner_margin(Margin::symmetric(6, ((h - 24.0) / 2.0).max(0.0) as i8));
        Panel::top("menu").exact_size(26.0).resizable(false).frame(bar(theme::TOOLBAR, 26.0)).show_separator_line(false).show(ui, |ui| self.menu_bar(ui));
        Panel::top("toolbar").exact_size(36.0).resizable(false).frame(bar(theme::TOOLBAR, 36.0)).show(ui, |ui| self.toolbar(ui));
        Panel::bottom("status").exact_size(22.0).resizable(false).frame(bar(theme::TOOLBAR, 22.0)).show(ui, |ui| self.status_bar(ui));
        if self.show_bottom {
            Panel::bottom("bottom").default_size(240.0).size_range(if self.studio.tab == BottomTab::Systems { 340.0..=900.0 } else { 90.0..=600.0 }).resizable(true).frame(chrome::dock(theme::PANEL)).show(ui, |ui| {
                let tabs = [(BottomTab::Project, "Project"), (BottomTab::Console, "Console"), (BottomTab::Systems, "Systems"), (BottomTab::Profiler, "Profiler"), (BottomTab::History, "History"), (BottomTab::Streaming, "Streaming")];
                let errors = self.studio.console.iter().rev().take(40).filter(|l| l.kind == crate::state::LogKind::Error).count();
                let tab = chrome::tab_strip(ui, &tabs, self.studio.tab, |ui| {
                    if errors > 0 {
                        ui.label(RichText::new(format!("{errors} errors")).small().color(theme::ERR));
                    }
                });
                if let Some(t) = tab {
                    self.studio.tab = t;
                }
                chrome::body().show(ui, |ui| match self.studio.tab {
                    BottomTab::Project => self.project.show(ui, &mut self.studio),
                    BottomTab::Console => self.console.show(ui, &mut self.studio),
                    BottomTab::Systems => self.systems.show(ui, &mut self.studio),
                    BottomTab::Profiler => self.profiler.show(ui, &mut self.studio, &self.viewport.gpu),
                    BottomTab::History => self.history.show(ui, &mut self.studio),
                    BottomTab::Streaming => {
                        if let Some(p) = self.streaming.show(ui, &mut self.studio, &mut self.viewport.zones, self.viewport.target) {
                            if p.focus {
                                self.viewport.focus(p.pos, (p.radius * 0.5).clamp(5.0, 200.0));
                            }
                        }
                    }
                });
            });
        }
        if self.show_hierarchy {
            let at = self.viewport.target;
            Panel::left("hierarchy").default_size(250.0).size_range(160.0..=480.0).resizable(true).frame(chrome::dock(theme::PANEL)).show(ui, |ui| {
                if self.studio.center == Center::Blueprint {
                    chrome::tab_strip(ui, &[(0, "My Blueprint")], 0, |_| {});
                    chrome::body().show(ui, |ui| self.blueprint.my_blueprint(ui, &mut self.studio));
                } else if self.studio.center == Center::Ui {
                    chrome::tab_strip(ui, &[(0, "Palette  /  Hierarchy")], 0, |_| {});
                    chrome::body().show(ui, |ui| self.designer.left_panel(ui, &mut self.studio));
                } else if self.studio.center == Center::Canvas {
                    chrome::tab_strip(ui, &[(0, "Notes")], 0, |_| {});
                    chrome::body().show(ui, |ui| self.canvas.outline(ui));
                } else if self.studio.center == Center::Effects {
                    chrome::tab_strip(ui, &[(0, "Effects")], 0, |_| {});
                    chrome::body().show(ui, |ui| self.effects.left_panel(ui, &mut self.studio));
                } else {
                    if let Some(t) = chrome::tab_strip(ui, &[(LeftTab::Hierarchy, "Hierarchy"), (LeftTab::Place, "Place")], self.studio.left_tab, |_| {}) {
                        self.studio.left_tab = t;
                    }
                    chrome::body().show(ui, |ui| match self.studio.left_tab {
                        LeftTab::Hierarchy => self.hierarchy.show(ui, &mut self.studio, at),
                        LeftTab::Place => self.place.show(ui, &mut self.studio, at),
                    });
                }
            });
        }
        if self.show_inspector {
            Panel::right("inspector").default_size(330.0).size_range(240.0..=640.0).resizable(true).frame(chrome::dock(theme::PANEL)).show(ui, |ui| {
                if self.studio.center == Center::Blueprint {
                    chrome::tab_strip(ui, &[(0, "Details")], 0, |_| {});
                    chrome::body().show(ui, |ui| egui::ScrollArea::vertical().id_salt("bp-details").auto_shrink([false, false]).show(ui, |ui| self.blueprint.details(ui, &mut self.studio)));
                } else if self.studio.center == Center::Ui {
                    chrome::tab_strip(ui, &[(0, "Details")], 0, |_| {});
                    chrome::body().show(ui, |ui| egui::ScrollArea::vertical().id_salt("ui-details").auto_shrink([false, false]).show(ui, |ui| self.designer.details(ui, &mut self.studio)));
                } else if self.studio.center == Center::Canvas {
                    chrome::tab_strip(ui, &[(0, "Details")], 0, |_| {});
                    chrome::body().show(ui, |ui| egui::ScrollArea::vertical().id_salt("canvas-details").auto_shrink([false, false]).show(ui, |ui| self.canvas.details(ui)));
                } else if self.studio.center == Center::Effects {
                    chrome::tab_strip(ui, &[(0, "Details")], 0, |_| {});
                    chrome::body().show(ui, |ui| egui::ScrollArea::vertical().id_salt("effects-details").auto_shrink([false, false]).show(ui, |ui| self.effects.details(ui, &mut self.studio)));
                } else {
                    chrome::tab_strip(ui, &[(0, "Inspector")], 0, |_| {});
                    chrome::body().show(ui, |ui| self.inspector.show(ui, &mut self.studio));
                }
            });
        }
        egui::CentralPanel::default().frame(chrome::dock(theme::PANEL)).show(ui, |ui| match self.studio.center {
            Center::View => {
                // (the card's passes are timed while the Profiler tab or the Stats overlay shows them: timestamp queries are not free)
                self.viewport.profile_gpu = self.viewport.stats || (self.show_bottom && self.studio.tab == BottomTab::Profiler);
                self.viewport.show(ui, &mut self.studio);
                if let Some(c) = self.viewport.goto.take() {
                    self.studio.center = c;
                }
            }
            Center::Ui => {
                let playing = self.studio.mode != Mode::Edit;
                let strip = crate::center::strip(ui, crate::center::CenterTab::Ui, |ui| {
                    if playing {
                        ui.label(RichText::new("playing").small().color(theme::BLUE));
                    }
                });
                if let Some(t) = strip {
                    match t {
                        crate::center::CenterTab::Game => self.viewport.set_kind(crate::viewport::ViewKind::Game),
                        crate::center::CenterTab::Scene => self.viewport.set_kind(crate::viewport::ViewKind::Scene),
                        crate::center::CenterTab::Script => self.studio.center = Center::Script,
                        crate::center::CenterTab::Blueprint => self.studio.center = Center::Blueprint,
                        crate::center::CenterTab::Ui => {}
                        crate::center::CenterTab::Canvas => self.studio.center = Center::Canvas,
                        crate::center::CenterTab::Sequencer => self.studio.center = Center::Sequencer,
                        crate::center::CenterTab::Effects => self.studio.center = Center::Effects,
                    }
                    if matches!(t, crate::center::CenterTab::Game | crate::center::CenterTab::Scene) {
                        self.studio.center = Center::View;
                    }
                }
                if self.studio.center == Center::Ui {
                    if let Some(go) = self.designer.show(ui, &mut self.studio) {
                        if go == crate::center::CenterTab::Blueprint {
                            self.studio.center = Center::Blueprint;
                        }
                    }
                }
            }
            Center::Blueprint => {
                let playing = self.studio.mode != Mode::Edit;
                let strip = crate::center::strip(ui, crate::center::CenterTab::Blueprint, |ui| {
                    if playing {
                        ui.label(RichText::new("playing").small().color(theme::BLUE));
                    }
                });
                if let Some(t) = strip {
                    match t {
                        crate::center::CenterTab::Game => self.viewport.set_kind(crate::viewport::ViewKind::Game),
                        crate::center::CenterTab::Scene => self.viewport.set_kind(crate::viewport::ViewKind::Scene),
                        crate::center::CenterTab::Script => self.studio.center = Center::Script,
                        crate::center::CenterTab::Ui => self.studio.center = Center::Ui,
                        crate::center::CenterTab::Blueprint => {}
                        crate::center::CenterTab::Canvas => self.studio.center = Center::Canvas,
                        crate::center::CenterTab::Sequencer => self.studio.center = Center::Sequencer,
                        crate::center::CenterTab::Effects => self.studio.center = Center::Effects,
                    }
                    if matches!(t, crate::center::CenterTab::Game | crate::center::CenterTab::Scene) {
                        self.studio.center = Center::View;
                    }
                }
                if self.studio.center == Center::Blueprint {
                    self.blueprint.show(ui, &mut self.studio);
                }
            }
            Center::Canvas => {
                let strip = crate::center::strip(ui, crate::center::CenterTab::Canvas, |_| {});
                if let Some(t) = strip {
                    match t {
                        crate::center::CenterTab::Game => self.viewport.set_kind(crate::viewport::ViewKind::Game),
                        crate::center::CenterTab::Scene => self.viewport.set_kind(crate::viewport::ViewKind::Scene),
                        crate::center::CenterTab::Script => self.studio.center = Center::Script,
                        crate::center::CenterTab::Blueprint => self.studio.center = Center::Blueprint,
                        crate::center::CenterTab::Ui => self.studio.center = Center::Ui,
                        crate::center::CenterTab::Canvas => {}
                        crate::center::CenterTab::Sequencer => self.studio.center = Center::Sequencer,
                        crate::center::CenterTab::Effects => self.studio.center = Center::Effects,
                    }
                    if matches!(t, crate::center::CenterTab::Game | crate::center::CenterTab::Scene) {
                        self.studio.center = Center::View;
                    }
                }
                if self.studio.center == Center::Canvas {
                    self.canvas.show(ui);
                }
            }
            Center::Sequencer => {
                let playing = self.studio.mode != Mode::Edit;
                let strip = crate::center::strip(ui, crate::center::CenterTab::Sequencer, |ui| {
                    if playing {
                        ui.label(RichText::new("playing").small().color(theme::BLUE));
                    }
                });
                if let Some(t) = strip {
                    match t {
                        crate::center::CenterTab::Game => self.viewport.set_kind(crate::viewport::ViewKind::Game),
                        crate::center::CenterTab::Scene => self.viewport.set_kind(crate::viewport::ViewKind::Scene),
                        crate::center::CenterTab::Script => self.studio.center = Center::Script,
                        crate::center::CenterTab::Blueprint => self.studio.center = Center::Blueprint,
                        crate::center::CenterTab::Ui => self.studio.center = Center::Ui,
                        crate::center::CenterTab::Canvas => self.studio.center = Center::Canvas,
                        crate::center::CenterTab::Sequencer => {}
                        crate::center::CenterTab::Effects => self.studio.center = Center::Effects,
                    }
                    if matches!(t, crate::center::CenterTab::Game | crate::center::CenterTab::Scene) {
                        self.studio.center = Center::View;
                    }
                }
                if self.studio.center == Center::Sequencer {
                    self.sequencer.show(ui, &mut self.studio);
                }
            }
            Center::Effects => {
                let strip = crate::center::strip(ui, crate::center::CenterTab::Effects, |_| {});
                if let Some(t) = strip {
                    match t {
                        crate::center::CenterTab::Game => self.viewport.set_kind(crate::viewport::ViewKind::Game),
                        crate::center::CenterTab::Scene => self.viewport.set_kind(crate::viewport::ViewKind::Scene),
                        crate::center::CenterTab::Script => self.studio.center = Center::Script,
                        crate::center::CenterTab::Blueprint => self.studio.center = Center::Blueprint,
                        crate::center::CenterTab::Ui => self.studio.center = Center::Ui,
                        crate::center::CenterTab::Canvas => self.studio.center = Center::Canvas,
                        crate::center::CenterTab::Sequencer => self.studio.center = Center::Sequencer,
                        crate::center::CenterTab::Effects => {}
                    }
                    if matches!(t, crate::center::CenterTab::Game | crate::center::CenterTab::Scene) {
                        self.studio.center = Center::View;
                    }
                }
                if self.studio.center == Center::Effects {
                    self.effects.show(ui, &mut self.studio);
                }
            }
            Center::Script => {
                let playing = self.studio.mode != Mode::Edit;
                let strip = crate::center::strip(ui, crate::center::CenterTab::Script, |ui| {
                    if playing {
                        ui.label(RichText::new("playing").small().color(theme::BLUE));
                    }
                });
                if let Some(t) = strip {
                    match t {
                        crate::center::CenterTab::Game => self.viewport.set_kind(crate::viewport::ViewKind::Game),
                        crate::center::CenterTab::Scene => self.viewport.set_kind(crate::viewport::ViewKind::Scene),
                        crate::center::CenterTab::Script => {}
                        crate::center::CenterTab::Canvas => self.studio.center = Center::Canvas,
                        crate::center::CenterTab::Sequencer => self.studio.center = Center::Sequencer,
                        crate::center::CenterTab::Effects => self.studio.center = Center::Effects,
                        crate::center::CenterTab::Blueprint => self.studio.center = Center::Blueprint,
                        crate::center::CenterTab::Ui => self.studio.center = Center::Ui,
                    }
                    if matches!(t, crate::center::CenterTab::Game | crate::center::CenterTab::Scene) {
                        self.studio.center = Center::View;
                    }
                }
                self.script.show(ui, &mut self.studio);
            }
        });
        if self.studio.selection != self.last_selection {
            self.last_selection = self.studio.selection;
            self.hierarchy.want_focus = true;
        }
        self.confirm_dialog(&ctx);
        self.settings_window(&ctx);
        self.handle_shot(&ctx);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::BG.to_normalized_gamma_f32()
    }
}
