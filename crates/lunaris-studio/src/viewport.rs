//! The scene view: the engine's own renderer draws the world into a picture, and the editor lays a grid, icons, the selection outline and
//! the tool handles over it. Looking around, picking and dragging are plain arithmetic on the camera the editor chose, so they agree with
//! what is on the screen.
use crate::chrome;
use crate::gizmo::{self, Handles};
pub use crate::gizmo::{Axis, Tool};
use crate::icons::{self, Icon};
use crate::mathx::*;
use crate::place::PlaceDrag;
use crate::project::AssetDrag;
use crate::state::{Kind, Mode, Studio};
use crate::theme;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Key, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, TextureOptions, Vec2};
use lunaris_gpu::{GpuRenderer, Quality};
use lunaris_json::Json;
use lunaris_render::{ExplicitCamera, Frame, RenderOptions, TextureCache, View};

/// What letting go of a file dragged out of the Project panel would do where the pointer is.
enum DropPlan {
    /// Add a new entity (a model, an Audio Source) at this point of the world.
    Place(V3),
    /// Use the picture as the texture of this entity.
    Texture(u64),
    Nothing,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewKind {
    /// A free camera looking at the world in 3D.
    Scene,
    /// A flat view from the front, for 2D games.
    Scene2D,
    /// What the scene's own camera sees (what the game will look like).
    Game,
}

struct Drag {
    tool: Tool,
    axis: Axis,
    grab: f32,
    handles: Handles,
    start_scale: V3,
    start_rot: Q,
    group: String,
    /// Where every selected entity was when the drag began (world position): a move takes all of them along.
    starts: Vec<(u64, V3)>,
}

/// The camera the picture was drawn with (what the overlays and the picking use).
#[derive(Clone, Copy, Debug)]
struct Cam {
    eye: V3,
    fwd: V3,
    right: V3,
    up: V3,
    tan: f32,
    aspect: f32,
    ortho: Option<f32>,
}

impl Cam {
    /// The ray through a point of the picture (`nx`, `ny` in -1..1, y up).
    fn ray(&self, nx: f32, ny: f32) -> (V3, V3) {
        match self.ortho {
            Some(h) => (add(add(self.eye, mul(self.right, nx * h * 0.5 * self.aspect)), mul(self.up, ny * h * 0.5)), self.fwd),
            None => (self.eye, norm(add(add(self.fwd, mul(self.right, nx * self.tan * self.aspect)), mul(self.up, ny * self.tan)))),
        }
    }
}

/// What the last picture the card drew measured (`GpuRenderer::last_gpu_ms` and friends; empty for the CPU reference renderer, and the times empty on a card without timestamp queries or while nothing asks for them).
#[derive(Clone, Debug, Default)]
pub struct GpuStats {
    pub adapter: String,
    /// The milliseconds of each pass; a name with a dot is a part of the pass before the dot.
    pub passes: Vec<(String, f32)>,
    /// The milliseconds the processor spent on the picture, by phase.
    pub cpu: Vec<(String, f32)>,
    /// Draw calls of all the passes, and the triangles the main view drew.
    pub draws: u32,
    pub triangles: u64,
}

/// One row of the card's times as the Profiler shows them.
#[derive(Clone, Debug, PartialEq)]
pub struct GpuRow {
    pub name: String,
    pub ms: f32,
    /// A part of the pass above it.
    pub part: bool,
}

impl GpuStats {
    /// The time of the picture on the card: the passes without a dot added up (the parts are inside them), or none when nothing was timed.
    pub fn total_ms(&self) -> Option<f32> {
        let sum: f32 = self.passes.iter().filter(|(n, _)| !n.contains('.')).map(|(_, ms)| *ms).sum();
        (!self.passes.is_empty()).then_some(sum)
    }

    /// The passes, the slowest first, each followed by its parts (the slowest first); a part whose pass was not timed comes after all of them.
    pub fn rows(&self) -> Vec<GpuRow> {
        let by_time = |a: &&(String, f32), b: &&(String, f32)| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0));
        let mut tops: Vec<&(String, f32)> = self.passes.iter().filter(|(n, _)| !n.contains('.')).collect();
        tops.sort_by(by_time);
        let mut out = Vec::new();
        let mut used = vec![false; self.passes.len()];
        for t in tops {
            out.push(GpuRow { name: t.0.clone(), ms: t.1, part: false });
            let prefix = format!("{}.", t.0);
            let mut parts: Vec<(usize, &(String, f32))> = self.passes.iter().enumerate().filter(|(_, (n, _))| n.starts_with(&prefix)).collect();
            parts.sort_by(|a, b| by_time(&a.1, &b.1));
            for (i, p) in parts {
                used[i] = true;
                out.push(GpuRow { name: p.0.clone(), ms: p.1, part: true });
            }
        }
        let mut rest: Vec<&(String, f32)> = self.passes.iter().enumerate().filter(|(i, (n, _))| !used[*i] && n.contains('.')).map(|(_, p)| p).collect();
        rest.sort_by(by_time);
        out.extend(rest.into_iter().map(|p| GpuRow { name: p.0.clone(), ms: p.1, part: true }));
        out
    }
}

/// How the scene view looks at the world: through a perspective lens, or straight along an axis with no perspective (Unreal's Top, Front and Side views, for placing things exactly).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Projection {
    Perspective,
    Top,
    Bottom,
    Front,
    Back,
    Left,
    Right,
}

impl Projection {
    pub const ALL: [Projection; 7] = [Projection::Perspective, Projection::Top, Projection::Bottom, Projection::Front, Projection::Back, Projection::Left, Projection::Right];

    pub fn label(self) -> &'static str {
        match self {
            Projection::Perspective => "Perspective",
            Projection::Top => "Top",
            Projection::Bottom => "Bottom",
            Projection::Front => "Front",
            Projection::Back => "Back",
            Projection::Left => "Left",
            Projection::Right => "Right",
        }
    }

    /// The direction an orthographic view looks and the direction that is up on the screen; None for the perspective view. Top looks down with -z up the screen, Front looks along -z (from the +z side, x to the right, as the 2D view does),
    /// Right looks along -x (from the +x side), and Bottom, Back and Left are the same seen from the other side.
    pub fn axes(self) -> Option<(V3, V3)> {
        match self {
            Projection::Perspective => None,
            Projection::Top => Some(([0.0, -1.0, 0.0], [0.0, 0.0, -1.0])),
            Projection::Bottom => Some(([0.0, 1.0, 0.0], [0.0, 0.0, 1.0])),
            Projection::Front => Some(([0.0, 0.0, -1.0], [0.0, 1.0, 0.0])),
            Projection::Back => Some(([0.0, 0.0, 1.0], [0.0, 1.0, 0.0])),
            Projection::Right => Some(([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0])),
            Projection::Left => Some(([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])),
        }
    }
}

/// A saved place of the scene camera (Unreal's camera bookmarks): the orbit view and the 2D view both, so a bookmark returns to whichever one is showing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bookmark {
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub target: V3,
    pub ortho_h: f32,
    pub center2: [f32; 2],
    pub projection: Projection,
}

/// How many bookmarks the keys 1 to 9 reach.
pub const BOOKMARKS: usize = 9;

pub struct Viewport {
    pub kind: ViewKind,
    pub tool: Tool,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub target: V3,
    pub ortho_h: f32,
    pub center2: [f32; 2],
    /// Perspective, or one of the orthographic views of the scene (they use `target` and `ortho_h`; `yaw` and `pitch` are what the perspective view comes back to).
    pub projection: Projection,
    /// How many pictures have been drawn (for tests: an idle editor draws none).
    pub renders: u64,
    /// Ctrl+1 to Ctrl+9 save the view of the scene camera, 1 to 9 return to it (kept while the editor is open).
    pub bookmarks: [Option<Bookmark>; BOOKMARKS],
    renderer: Option<Result<GpuRenderer, String>>,
    textures: TextureCache,
    pub quality: String,
    tex: Option<egui::TextureHandle>,
    ids: Vec<u64>,
    img: (u32, u32),
    view_proj: Mat,
    cam: Option<Cam>,
    stamp: u64,
    pub info: String,
    drag: Option<Drag>,
    /// The points of the stroke being painted (Paint mode), in the world.
    stroke: Vec<V3>,
    hover_axis: Option<Axis>,
    pub grid: bool,
    pub gizmos: bool,
    /// The steps the handles snap to (0 = off): positions in metres, turns in degrees, scale factors. Ctrl snaps to the step, or to 0.5, 15 and 0.1 when there is none.
    pub snap_move: f32,
    pub snap_rot: f32,
    pub snap_scale: f32,
    /// The Move and Rotate handles follow the selected object's turn (Local) instead of the world's axes (World); scale always follows the object, and the 2D view keeps screen-aligned handles.
    pub local_space: bool,
    /// Draw every `StreamZone` as a ring on the ground (green while loaded), and the zones as last read.
    pub streaming: bool,
    pub zones: crate::streaming_ui::ZoneCache,
    /// The statistics overlay (frame time, picture time, entities, tick, fly speed).
    pub stats: bool,
    /// Time every pass on the card (the Stats overlay or the Profiler tab wants it: timestamp queries are not free), and what the last GPU picture measured.
    pub profile_gpu: bool,
    pub gpu: GpuStats,
    /// Tests: keep `gpu` as it was set (a CPU picture would clear it).
    pub gpu_fixed: bool,
    /// How fast the fly camera moves, as a multiple of its usual speed (right button held and the wheel changes it).
    pub fly_speed: f32,
    /// Time between editor frames (smoothed) and the last picture's drawing time, in milliseconds.
    pub frame_ms: f32,
    pub picture_ms: f32,
    pub error: Option<String>,
    forced: bool,
    pub game_input: bool,
    /// Set when a tab of another editor was clicked: the window switches the middle to it.
    pub goto: Option<crate::state::Center>,
    pub last_rect: Rect,
}

impl Viewport {
    pub fn new(root: std::path::PathBuf, quality: &str) -> Viewport {
        Viewport {
            kind: ViewKind::Scene,
            tool: Tool::Move,
            yaw: 0.6,
            pitch: 0.45,
            dist: 12.0,
            target: [0.0, 1.0, 0.0],
            ortho_h: 12.0,
            center2: [0.0, 0.0],
            projection: Projection::Perspective,
            renders: 0,
            bookmarks: [None; BOOKMARKS],
            renderer: None,
            textures: TextureCache::new(Some(root)),
            quality: quality.to_string(),
            tex: None,
            ids: Vec::new(),
            img: (0, 0),
            view_proj: Mat::IDENTITY,
            cam: None,
            stamp: 0,
            info: String::new(),
            drag: None,
            stroke: Vec::new(),
            snap_move: 0.0,
            snap_rot: 0.0,
            snap_scale: 0.0,
            local_space: false,
            streaming: false,
            zones: Default::default(),
            hover_axis: None,
            grid: true,
            gizmos: true,
            stats: false,
            fly_speed: 1.0,
            frame_ms: 0.0,
            picture_ms: 0.0,
            profile_gpu: false,
            gpu: GpuStats::default(),
            gpu_fixed: false,
            error: None,
            forced: true,
            game_input: false,
            goto: None,
            last_rect: Rect::NOTHING,
        }
    }

    pub fn refresh(&mut self) {
        self.forced = true;
    }

    /// Switch between the scene and the game view (from another editor's tab strip).
    pub fn set_kind(&mut self, kind: ViewKind) {
        let two_d = self.kind == ViewKind::Scene2D;
        self.kind = if kind == ViewKind::Scene && two_d { ViewKind::Scene2D } else { kind };
        self.forced = true;
    }

    /// Where a world point is on the screen, in the window's coordinates (for the last picture).
    pub fn project(&self, p: V3) -> Option<Pos2> {
        let r = self.last_rect;
        to_screen(&self.view_proj, p, r.width(), r.height()).map(|s| Pos2::new(r.min.x + s[0], r.min.y + s[1]))
    }

    /// Put the selection in the middle of the view.
    pub fn focus(&mut self, pos: V3, size: f32) {
        self.target = pos;
        self.center2 = [pos[0], pos[1]];
        self.dist = (size * 2.5).clamp(1.5, 400.0);
        self.ortho_h = (size * 3.0).clamp(2.0, 400.0);
        self.forced = true;
    }

    /// Remember where the scene camera is as bookmark `n` (0 to 8; the key is `n` + 1). The game view has the game's camera: nothing to save there.
    pub fn set_bookmark(&mut self, n: usize) -> bool {
        if n >= BOOKMARKS || self.kind == ViewKind::Game {
            return false;
        }
        self.bookmarks[n] = Some(Bookmark { yaw: self.yaw, pitch: self.pitch, dist: self.dist, target: self.target, ortho_h: self.ortho_h, center2: self.center2, projection: self.projection });
        true
    }

    /// Put the scene camera where bookmark `n` was saved; false when there is none (or the game view is showing).
    pub fn go_bookmark(&mut self, n: usize) -> bool {
        let Some(Some(b)) = self.bookmarks.get(n).copied() else { return false };
        if self.kind == ViewKind::Game {
            return false;
        }
        (self.yaw, self.pitch, self.dist, self.target, self.ortho_h, self.center2, self.projection) = (b.yaw, b.pitch, b.dist, b.target, b.ortho_h, b.center2, b.projection);
        self.forced = true;
        true
    }

    pub fn focus_selection(&mut self, studio: &Studio) {
        if studio.selected.len() > 1 {
            // several: their middle, far enough back to see them all
            let ps: Vec<V3> = studio.selected.iter().filter_map(|u| studio.world_of(*u)).collect();
            if !ps.is_empty() {
                let n = ps.len() as f32;
                let mid = [ps.iter().map(|p| p[0]).sum::<f32>() / n, ps.iter().map(|p| p[1]).sum::<f32>() / n, ps.iter().map(|p| p[2]).sum::<f32>() / n];
                let reach = ps.iter().map(|p| ((p[0] - mid[0]).powi(2) + (p[1] - mid[1]).powi(2) + (p[2] - mid[2]).powi(2)).sqrt()).fold(0.0f32, f32::max);
                self.focus(mid, (reach + 1.0).clamp(0.5, 100.0));
                return;
            }
        }
        if let Some(p) = studio.entity_world_pos() {
            let size = studio.entity.as_ref().and_then(|e| e.vec3("Transform", "scale")).map(|s| s[0].abs().max(s[1].abs()).max(s[2].abs())).unwrap_or(1.0).clamp(0.5, 50.0);
            self.focus(p, size);
        }
    }

    fn basis(&self, aspect: f32) -> Cam {
        match self.kind {
            ViewKind::Scene2D => Cam { eye: [self.center2[0], self.center2[1], 100.0], fwd: [0.0, 0.0, -1.0], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], tan: 0.5, aspect, ortho: Some(self.ortho_h) },
            ViewKind::Scene if self.projection != Projection::Perspective => {
                let (fwd, up) = self.projection.axes().expect("an orthographic view has axes");
                // (far enough back to see everything of a big world along the axis: the picture's far plane is 4,000 m)
                Cam { eye: sub(self.target, mul(fwd, ORTHO_EYE_DISTANCE)), fwd, right: cross(fwd, up), up, tan: 0.5, aspect, ortho: Some(self.ortho_h) }
            }
            _ => {
                let (sy, cy) = self.yaw.sin_cos();
                let (sp, cp) = self.pitch.sin_cos();
                let dir = [cp * sy, sp, cp * cy];
                let eye = add(self.target, mul(dir, self.dist));
                let fwd = mul(dir, -1.0);
                let right = norm(cross(fwd, [0.0, 1.0, 0.0]));
                let up = cross(right, fwd);
                Cam { eye, fwd, right, up, tan: (60.0f32.to_radians() * 0.5).tan(), aspect, ortho: None }
            }
        }
    }

    fn quality_value(&self) -> Option<Quality> {
        Quality::from_name(&self.quality)
    }

    fn render(&mut self, studio: &mut Studio, ctx: &egui::Context, w: u32, h: u32, want_ids: bool) {
        self.renders += 1;
        let cam = self.basis(w as f32 / h as f32);
        let view = match self.kind {
            ViewKind::Game => View::World(None),
            _ => View::Explicit(ExplicitCamera {
                eye: cam.eye,
                target: add(cam.eye, cam.fwd),
                up: cam.up,
                fov_y: 60.0,
                ortho_height: cam.ortho,
                near: if cam.ortho.is_some() { 0.1 } else { 0.05 },
                far: 4000.0,
                clear: [0.19, 0.19, 0.2, 1.0],
            }),
        };
        let opts = RenderOptions { width: w, height: h, view, supersample: 1, threads: 4 };
        let started = std::time::Instant::now();
        let used;
        let frame: Result<Frame, String> = match self.quality_value() {
            Some(q) => {
                if self.renderer.is_none() {
                    self.renderer = Some(GpuRenderer::headless());
                }
                match self.renderer.as_mut() {
                    Some(Ok(r)) => {
                        used = r.adapter_name();
                        r.set_profile(self.profile_gpu);
                        let frame = r.render_frame(&mut studio.engine.sim.world, &mut self.textures, &opts, &q, want_ids);
                        self.gpu = GpuStats { adapter: used.clone(), passes: r.last_gpu_ms.clone(), cpu: r.last_cpu_ms.clone(), draws: r.last_draws, triangles: r.last_triangles };
                        frame
                    }
                    Some(Err(e)) => {
                        used = String::new();
                        Err(format!("no GPU renderer ({e}); choose Reference (CPU)"))
                    }
                    None => {
                        used = String::new();
                        Err("no renderer".to_string())
                    }
                }
            }
            None => {
                used = "CPU reference renderer".to_string();
                if !self.gpu_fixed {
                    self.gpu = GpuStats::default();
                }
                lunaris_render::render(&mut studio.engine.sim.world, &mut self.textures, &opts).map_err(|e| e.to_string())
            }
        };
        match frame {
            Ok(f) => {
                let image = egui::ColorImage::from_rgba_unmultiplied([f.width as usize, f.height as usize], &f.rgba);
                match &mut self.tex {
                    Some(t) => t.set(image, TextureOptions::LINEAR),
                    None => self.tex = Some(ctx.load_texture("lunaris-viewport", image, TextureOptions::LINEAR)),
                }
                self.img = (f.width, f.height);
                self.ids = f.ids;
                self.view_proj = Mat(f.view_proj.0);
                self.cam = if self.kind == ViewKind::Game { None } else { Some(cam) };
                self.picture_ms = started.elapsed().as_secs_f32() * 1000.0;
                self.info = format!("{} - {}x{} - {:.1} ms{}", used, f.width, f.height, self.picture_ms, if f.warnings.is_empty() { String::new() } else { format!(" - {}", f.warnings.join("; ")) });
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    // ------------------------------------------------------------------------------------------------------------- the bars above the view

    fn bars(&mut self, ui: &mut egui::Ui, studio: &Studio) {
        let active = if self.kind == ViewKind::Game { crate::center::CenterTab::Game } else { crate::center::CenterTab::Scene };
        let playing = studio.mode != Mode::Edit;
        if let Some(t) = crate::center::strip(ui, active, |ui| {
            if playing {
                ui.label(egui::RichText::new("playing").small().color(theme::BLUE));
            }
        }) {
            match t {
                crate::center::CenterTab::Script => self.goto = Some(crate::state::Center::Script),
                crate::center::CenterTab::Blueprint => self.goto = Some(crate::state::Center::Blueprint),
                crate::center::CenterTab::Ui => self.goto = Some(crate::state::Center::Ui),
                crate::center::CenterTab::Canvas => self.goto = Some(crate::state::Center::Canvas),
                crate::center::CenterTab::Sequencer => self.goto = Some(crate::state::Center::Sequencer),
                crate::center::CenterTab::Effects => self.goto = Some(crate::state::Center::Effects),
                _ => {
                    self.kind = if t == crate::center::CenterTab::Game { ViewKind::Game } else if self.kind == ViewKind::Scene2D { ViewKind::Scene2D } else { ViewKind::Scene };
                    self.forced = true;
                }
            }
        }
        // the scene toolbar
        let bar = Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), 24.0));
        let narrow = bar.width() < NARROW_TOOLBAR;
        ui.painter().rect_filled(bar, CornerRadius::ZERO, theme::PANEL);
        ui.painter().line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, theme::LINE));
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(4.0, 1.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let label = match self.quality.as_str() {
                "reference" => "Reference (CPU)".to_string(),
                q => format!("{}{}", &q[..1].to_uppercase(), &q[1..]),
            };
            chrome::drop_down(ui, &label, |ui| {
                for q in ["classic", "low", "medium", "high", "ultra", "reference"] {
                    let text = if q == "reference" { "Reference (CPU)".to_string() } else { format!("{}{}", &q[..1].to_uppercase(), &q[1..]) };
                    if ui.selectable_label(self.quality == q, text).clicked() {
                        self.quality = q.to_string();
                        self.forced = true;
                        ui.close();
                    }
                }
            });
            chrome::drop_down(ui, "Views", |ui| {
                ui.label(egui::RichText::new("Ctrl+1 to Ctrl+9 save the view, 1 to 9 return to it").small().color(theme::TEXT_DIM));
                for n in 0..BOOKMARKS {
                    ui.horizontal(|ui| {
                        let saved = self.bookmarks[n].is_some();
                        if ui.add_enabled(saved, egui::Button::new(format!("Go to view {}", n + 1)).min_size(Vec2::new(100.0, 0.0))).clicked() {
                            self.go_bookmark(n);
                            ui.close();
                        }
                        if ui.button(if saved { "Replace" } else { "Save here" }).clicked() {
                            self.set_bookmark(n);
                        }
                    });
                }
            });
            if self.kind == ViewKind::Scene {
                let before = self.projection;
                chrome::drop_down(ui, self.projection.label(), |ui| {
                    for p in Projection::ALL {
                        if ui.selectable_label(self.projection == p, p.label()).clicked() {
                            self.projection = p;
                            ui.close();
                        }
                    }
                });
                if before == Projection::Perspective && self.projection != Projection::Perspective {
                    // (the same picture to begin with: the height the lens saw at the distance of the target)
                    self.ortho_h = (2.0 * self.dist * (60.0f32.to_radians() * 0.5).tan()).clamp(0.5, 4000.0);
                    self.forced = true;
                } else if before != self.projection {
                    self.forced = true;
                }
            }
            chrome::rule(ui);
            let two_d = self.kind == ViewKind::Scene2D;
            if chrome::pill(ui, "2D", two_d, false).clicked() && self.kind != ViewKind::Game {
                self.kind = if two_d { ViewKind::Scene } else { ViewKind::Scene2D };
                self.forced = true;
            }
            const STATS_TIP: &str = "Frame time, picture time, entities and tick; with the right button held the wheel sets the fly speed";
            const STREAMING_TIP: &str = "A ring on the ground for every StreamZone: green while it is loaded, grey while it is not";
            if narrow {
                // a narrow view (a small window, a wide Inspector): the switches go under one menu and the three snap steps under another, so that nothing is drawn over anything
                chrome::drop_down(ui, "Show", |ui| {
                    ui.checkbox(&mut self.grid, "Grid");
                    if ui.checkbox(&mut self.streaming, "Streaming").on_hover_text(STREAMING_TIP).changed() {
                        self.zones.invalidate();
                    }
                    ui.checkbox(&mut self.stats, "Stats").on_hover_text(STATS_TIP);
                    ui.checkbox(&mut self.gizmos, "Gizmos");
                });
            } else {
                if chrome::pill(ui, "Grid", self.grid, false).clicked() {
                    self.grid = !self.grid;
                }
                if chrome::pill(ui, "Streaming", self.streaming, false).on_hover_text(STREAMING_TIP).clicked() {
                    self.streaming = !self.streaming;
                    self.zones.invalidate();
                }
                if chrome::pill(ui, "Stats", self.stats, false).on_hover_text(STATS_TIP).clicked() {
                    self.stats = !self.stats;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !narrow && chrome::pill(ui, "Gizmos", self.gizmos, false).clicked() {
                    self.gizmos = !self.gizmos;
                }
                if chrome::pill(ui, if self.local_space { "Local" } else { "World" }, self.local_space, false).on_hover_text("The Move and Rotate handles along the object's own axes (Local) or the world's (World); Scale always follows the object").clicked() {
                    self.local_space = !self.local_space;
                }
                // (Unreal's snap settings: a step for each of the three handles, or off)
                let (scale_steps, rot_steps, move_steps): (&[f32], &[f32], &[f32]) = (&[0.05, 0.1, 0.25, 0.5, 1.0], &[5.0, 10.0, 15.0, 30.0, 45.0, 90.0], &[0.1, 0.25, 0.5, 1.0, 5.0, 10.0]);
                if narrow {
                    chrome::drop_down(ui, "Snap", |ui| {
                        for (title, unit, value, steps) in [("Move", " m", &mut self.snap_move, move_steps), ("Rotate", "\u{b0}", &mut self.snap_rot, rot_steps), ("Scale", "", &mut self.snap_scale, scale_steps)] {
                            ui.menu_button(snap_label(title, unit, *value), |ui| snap_items(ui, unit, value, steps));
                        }
                    });
                } else {
                    snap_menu(ui, "Scale", "", &mut self.snap_scale, scale_steps);
                    snap_menu(ui, "Rotate", "\u{b0}", &mut self.snap_rot, rot_steps);
                    snap_menu(ui, "Move", " m", &mut self.snap_move, move_steps);
                }
                if let Some(e) = &self.error {
                    ui.label(egui::RichText::new(e).small().color(theme::ERR));
                }
            });
        });
        ui.advance_cursor_after_rect(bar);
    }

    pub fn show(&mut self, ui: &mut egui::Ui, studio: &mut Studio) {
        self.bars(ui, studio);
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(avail.max(Vec2::splat(32.0)), Sense::click_and_drag());
        self.last_rect = rect;
        let ppp = ui.ctx().pixels_per_point();
        let w = ((rect.width() * ppp) as u32).clamp(32, 3840);
        let h = ((rect.height() * ppp) as u32).clamp(32, 2160);
        let cam_now = self.basis(w as f32 / h as f32);

        let playing = studio.mode != Mode::Edit;
        let game_play = playing && self.kind == ViewKind::Game;
        self.game_input = game_play && resp.hovered();

        if !game_play {
            self.navigate(ui, &resp, rect, &cam_now);
        }
        // picking and the handles work on the camera of the last picture
        let cam = self.basis(w as f32 / h as f32);
        // Paint mode: the button paints instead of picking and dragging handles
        let painting = studio.brush.is_some() && !playing && self.kind == ViewKind::Scene;
        if !game_play && self.kind != ViewKind::Game {
            if !painting {
                self.interact(ui, &resp, rect, studio, ppp, &cam);
            }
        } else if resp.clicked() && !playing {
            self.pick(studio, &resp, rect, ppp, false);
        }

        // draw again only when something that shows has changed
        let cam_bits = [cam.eye[0], cam.eye[1], cam.eye[2], cam.fwd[0], cam.fwd[1], cam.fwd[2], cam.ortho.unwrap_or(0.0)].iter().fold(1469598103934665603u64, |h2, f| (h2 ^ f.to_bits() as u64).wrapping_mul(1099511628211));
        let stamp = [studio.live_revision, if playing { studio.tick } else { 0 }, cam_bits, ((w as u64) << 16) | h as u64, self.kind as u64, self.quality.bytes().map(|b| b as u64).sum::<u64>()]
            .iter()
            .fold(1469598103934665603u64, |h2, v| (h2 ^ v).wrapping_mul(1099511628211));
        if self.forced || stamp != self.stamp || self.tex.is_none() {
            self.render(studio, ui.ctx(), w, h, !game_play);
            self.stamp = stamp;
            self.forced = false;
        }
        if playing {
            ui.ctx().request_repaint();
        }

        let dt_ms = ui.input(|i| i.unstable_dt) * 1000.0;
        self.frame_ms = if self.frame_ms == 0.0 { dt_ms } else { self.frame_ms * 0.9 + dt_ms * 0.1 };
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(48, 48, 50));
        if let Some(t) = &self.tex {
            painter.image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        if playing {
            // like the other editors: a tint over the view while the game runs
            painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(40, 70, 130, 22));
        }
        if self.streaming {
            self.zones.refresh(studio);
        }
        if self.kind != ViewKind::Game {
            self.overlays(&painter, rect, studio, &cam);
        } else {
            self.selection_outline(&painter, rect, studio);
        }
        if painting {
            self.paint_stroke(ui, &resp, rect, studio, &cam, &painter);
        } else {
            self.stroke.clear();
        }
        if !playing && self.kind != ViewKind::Game {
            self.asset_drop(ui, &resp, rect, studio, &cam, ppp, &painter);
            self.template_drop(ui, &resp, rect, studio, &cam, &painter);
        }
        if self.kind == ViewKind::Game && playing && self.game_input {
            painter.text(rect.right_top() + Vec2::new(-8.0, 6.0), Align2::RIGHT_TOP, "keyboard and mouse go to the game", FontId::proportional(11.0), theme::TEXT_DIM);
        }
        if let Some(e) = &self.error {
            painter.text(rect.center(), Align2::CENTER_CENTER, e, FontId::proportional(14.0), theme::ERR);
        }
        painter.text(rect.left_bottom() + Vec2::new(8.0, -6.0), Align2::LEFT_BOTTOM, &self.info, FontId::proportional(11.0), Color32::from_rgba_unmultiplied(200, 205, 215, 150));
        if self.stats {
            self.stats_overlay(&painter, rect, studio);
        }
        if studio.total_entities == 0 && self.error.is_none() {
            painter.text(rect.center(), Align2::CENTER_CENTER, "An empty scene. Use Entity > Create, or File > Open Scene.", FontId::proportional(14.0), theme::TEXT_DIM);
        }
    }

    /// Drop the stroke being painted without spawning it (the brush was put down).
    pub fn cancel_stroke(&mut self) {
        self.stroke.clear();
    }

    /// Paint mode: a ring of the brush's size at the pointer (on what the pointer is over); a stroke with the left button collects points `spacing` apart and, when the button is let go,
    /// everything of it is spawned at once (one Undo). (Esc puts the brush down: the window handles that key.)
    fn paint_stroke(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, studio: &mut Studio, cam: &Cam, painter: &egui::Painter) {
        let Some(brush) = studio.brush.clone() else { return };
        let ring = Stroke::new(2.0, theme::OUTLINE);
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let inside = pointer.filter(|p| rect.contains(*p));
        let (down, alt) = ui.input(|i| (i.pointer.primary_down(), i.modifiers.alt));
        let active = down && !alt && resp.is_pointer_button_down_on();
        let mut here = None;
        if let Some(p) = inside {
            if !ui.input(|i| i.pointer.secondary_down() || i.pointer.middle_down()) {
                here = Some(self.drop_point(studio, rect, p, cam));
            }
        }
        if active {
            if let Some(at) = here {
                let far = match self.stroke.last() {
                    Some(l) => (0..3).map(|k| (l[k] - at[k]).powi(2)).sum::<f32>().sqrt() >= brush.spacing,
                    None => true,
                };
                if far && self.stroke.len() < 400 {
                    self.stroke.push(at);
                }
            }
        } else if !self.stroke.is_empty() {
            // the button went up: the stroke is spawned
            let points = std::mem::take(&mut self.stroke);
            let n = crate::paint::stamp(studio, &brush, &points);
            if n > 0 {
                studio.toast(format!("Painted {n} x {}", brush.label()), false);
            }
            self.forced = true;
        }
        // the dots of the stroke, the ring at the pointer
        for pt in &self.stroke {
            if let Some(s) = self.project(*pt) {
                painter.circle_filled(s, 3.0, theme::OUTLINE);
            }
        }
        if let Some(at) = here {
            let pts: Vec<Pos2> = (0..=48)
                .filter_map(|k| {
                    let a = k as f32 / 48.0 * std::f32::consts::TAU;
                    self.project([at[0] + a.cos() * brush.radius, at[1], at[2] + a.sin() * brush.radius])
                })
                .collect();
            painter.add(egui::Shape::line(pts, ring));
            if let Some(s) = self.project(at) {
                painter.circle_filled(s, 2.5, theme::OUTLINE);
            }
        }
        // the banner at the top of the view
        let text = format!("Painting {}: drag over the scene, Esc to stop  ({:.1} m brush)", brush.label(), brush.radius);
        let galley = painter.layout_no_wrap(text, FontId::proportional(12.0), theme::TEXT_BRIGHT);
        let at = Pos2::new(rect.center().x - galley.size().x * 0.5 - 8.0, rect.min.y + 8.0);
        painter.rect_filled(Rect::from_min_size(at, galley.size() + Vec2::new(16.0, 8.0)), CornerRadius::same(4), Color32::from_rgba_unmultiplied(20, 22, 26, 230));
        painter.galley(at + Vec2::new(8.0, 4.0), galley, theme::TEXT_BRIGHT);
        ui.ctx().request_repaint();
    }

    /// A model, a picture or a sound dragged out of the Project panel over the view: a ring and a label show what letting go would do and where; letting go does it.
    #[allow(clippy::too_many_arguments)]
    fn asset_drop(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, studio: &mut Studio, cam: &Cam, ppp: f32, painter: &egui::Painter) {
        // (only when it is a file that is carried: taking a payload of another type would throw it away)
        if !egui::DragAndDrop::has_payload_of_type::<AssetDrag>(ui.ctx()) {
            return;
        }
        let released = resp.dnd_release_payload::<AssetDrag>();
        let Some(a) = released.clone().or_else(|| resp.dnd_hover_payload::<AssetDrag>()) else { return };
        let Some(p) = ui.input(|i| i.pointer.latest_pos()) else { return };
        let plan = self.drop_plan(studio, &a, rect, p, cam, ppp);
        if released.is_some() {
            match plan {
                DropPlan::Place(at) => {
                    if a.kind == "model" {
                        studio.add_model(&a.path, at);
                    } else {
                        studio.add_sound(&a.path, at);
                    }
                }
                DropPlan::Texture(uid) => studio.set(uid, "MeshRenderer", Json::obj([("texture", Json::str(a.path.clone()))]), None),
                DropPlan::Nothing => {}
            }
            self.forced = true;
            return;
        }
        // still carrying it: say what letting go would do
        let ring = Stroke::new(2.0, theme::OUTLINE);
        painter.rect_stroke(rect.shrink(1.0), CornerRadius::ZERO, ring, StrokeKind::Inside);
        let (at, text) = match plan {
            DropPlan::Place(at) => {
                if let Some(s) = self.project(at) {
                    painter.circle_stroke(s, 9.0, ring);
                    painter.circle_filled(s, 2.5, theme::OUTLINE);
                }
                (p, format!("add {}", a.name()))
            }
            DropPlan::Texture(uid) => (p, format!("use {} on {}", a.name(), studio.all.iter().find(|r| r.uid == uid).map_or("", |r| r.name.as_str()))),
            DropPlan::Nothing => (p, "drop it on a mesh".to_string()),
        };
        let galley = painter.layout_no_wrap(text, FontId::proportional(12.0), theme::TEXT_BRIGHT);
        let at = at + Vec2::new(14.0, 30.0);
        painter.rect_filled(Rect::from_min_size(at, galley.size() + Vec2::new(12.0, 6.0)), CornerRadius::same(3), Color32::from_rgba_unmultiplied(20, 22, 26, 230));
        painter.galley(at + Vec2::new(6.0, 3.0), galley, theme::TEXT_BRIGHT);
        ui.ctx().request_repaint();
    }

    /// A thing dragged out of the Place panel over the view: the ring and the label say where it will land; letting go adds it there (standing on what is under the pointer).
    fn template_drop(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, studio: &mut Studio, cam: &Cam, painter: &egui::Painter) {
        if !egui::DragAndDrop::has_payload_of_type::<PlaceDrag>(ui.ctx()) {
            return;
        }
        let released = resp.dnd_release_payload::<PlaceDrag>();
        let Some(t) = released.clone().or_else(|| resp.dnd_hover_payload::<PlaceDrag>()) else { return };
        let Some(p) = ui.input(|i| i.pointer.latest_pos()) else { return };
        let at = self.drop_point(studio, rect, p, cam);
        if released.is_some() {
            match &*t {
                PlaceDrag::Template(k) => {
                    studio.spawn_template(*k, [at[0], at[1] + crate::place::lift(*k), at[2]]);
                }
                PlaceDrag::Prefab(name) => studio.spawn_prefab(name, at),
            }
            self.forced = true;
            return;
        }
        let ring = Stroke::new(2.0, theme::OUTLINE);
        painter.rect_stroke(rect.shrink(1.0), CornerRadius::ZERO, ring, StrokeKind::Inside);
        if let Some(s) = self.project(at) {
            painter.circle_stroke(s, 9.0, ring);
            painter.circle_filled(s, 2.5, theme::OUTLINE);
        }
        let galley = painter.layout_no_wrap(format!("add {}", t.label()), FontId::proportional(12.0), theme::TEXT_BRIGHT);
        let at = p + Vec2::new(14.0, 30.0);
        painter.rect_filled(Rect::from_min_size(at, galley.size() + Vec2::new(12.0, 6.0)), CornerRadius::same(3), Color32::from_rgba_unmultiplied(20, 22, 26, 230));
        painter.galley(at + Vec2::new(6.0, 3.0), galley, theme::TEXT_BRIGHT);
        ui.ctx().request_repaint();
    }

    fn drop_plan(&self, studio: &mut Studio, a: &AssetDrag, rect: Rect, p: Pos2, cam: &Cam, ppp: f32) -> DropPlan {
        match a.kind.as_str() {
            "model" | "sound" => DropPlan::Place(self.drop_point(studio, rect, p, cam)),
            "texture" => {
                let (x, y) = (((p.x - rect.min.x) * ppp) as i64, ((p.y - rect.min.y) * ppp) as i64);
                let uid = if x >= 0 && y >= 0 && (x as u32) < self.img.0 && (y as u32) < self.img.1 { self.ids.get(y as usize * self.img.0 as usize + x as usize).copied().unwrap_or(0) } else { 0 };
                if studio.all.iter().any(|r| r.uid == uid && r.kind == Kind::Mesh) {
                    DropPlan::Texture(uid)
                } else {
                    DropPlan::Nothing
                }
            }
            _ => DropPlan::Nothing,
        }
    }

    /// Where something dropped at the pointer lands: on the first thing the ray through the pointer hits, else on the ground (the plane y = 0; z = 0 in the 2D view), else in front of the camera.
    fn drop_point(&self, studio: &mut Studio, rect: Rect, p: Pos2, cam: &Cam) -> V3 {
        let (o, dir) = self.ray_at(rect, p, cam);
        let arr = |v: V3| Json::arr(v.iter().map(|x| Json::from(*x)));
        if self.kind == ViewKind::Scene2D {
            return if dir[2].abs() > 1e-6 { add(o, mul(dir, -o[2] / dir[2])) } else { [o[0], o[1], 0.0] };
        }
        if let Some(r) = studio.try_call("raycast", Json::obj([("origin", arr(o)), ("direction", arr(dir)), ("max_distance", Json::from(1000.0))])) {
            if r.get("hit").and_then(|h| h.as_bool()) == Some(true) {
                if let Some(pt) = r.get("point").and_then(|a| a.as_array()).filter(|a| a.len() == 3) {
                    return [pt[0].as_f64().unwrap_or(0.0) as f32, pt[1].as_f64().unwrap_or(0.0) as f32, pt[2].as_f64().unwrap_or(0.0) as f32];
                }
            }
        }
        if dir[1].abs() > 1e-5 {
            let t = -o[1] / dir[1];
            if t > 0.0 && t < 2000.0 {
                return add(o, mul(dir, t));
            }
        }
        add(o, mul(dir, 8.0))
    }

    /// The statistics in the corner of the view: how fast the editor itself runs, what a picture costs, how much is in the scene.
    fn stats_overlay(&self, painter: &egui::Painter, rect: Rect, studio: &Studio) {
        let fps = if self.frame_ms > 0.0 { 1000.0 / self.frame_ms } else { 0.0 };
        let mut lines = vec![
            format!("frame   {:5.1} ms  {:4.0} fps", self.frame_ms, fps),
            format!("picture {:5.1} ms  {}x{}", self.picture_ms, self.img.0, self.img.1),
            format!("entities {}", studio.total_entities),
            format!("tick     {}", studio.tick),
        ];
        if self.gpu.draws > 0 {
            lines.push(format!("draws    {}  tris {}", self.gpu.draws, self.gpu.triangles));
        }
        if let Some(ms) = self.gpu.total_ms() {
            lines.push(format!("card    {ms:5.2} ms"));
        }
        if self.streaming {
            lines.push(self.zones.summary());
        }
        if self.kind == ViewKind::Scene {
            lines.push(format!("fly speed x{:.2}", self.fly_speed));
        }
        let (w, h) = (if self.streaming { 244.0 } else { 172.0 }, 8.0 + lines.len() as f32 * 14.0);
        let r = Rect::from_min_size(rect.min + Vec2::new(8.0, 8.0), Vec2::new(w, h));
        painter.rect_filled(r, CornerRadius::same(3), Color32::from_rgba_unmultiplied(20, 22, 26, 190));
        for (i, l) in lines.iter().enumerate() {
            painter.text(r.min + Vec2::new(8.0, 5.0 + i as f32 * 14.0), Align2::LEFT_TOP, l, FontId::monospace(11.0), theme::TEXT);
        }
    }

    // ------------------------------------------------------------------------------------------------------------- camera

    fn navigate(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, cam: &Cam) {
        let (alt, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift));
        let delta = resp.drag_delta();
        let hand = self.tool == Tool::Hand && resp.dragged_by(PointerButton::Primary) && !alt;
        let orbit = resp.dragged_by(PointerButton::Secondary) && !shift || (resp.dragged_by(PointerButton::Primary) && alt);
        let pan = resp.dragged_by(PointerButton::Middle) || (resp.dragged_by(PointerButton::Secondary) && shift) || hand;
        if self.kind == ViewKind::Scene2D {
            if pan || orbit {
                let per_px = self.ortho_h / rect.height().max(1.0);
                self.center2[0] -= delta.x * per_px;
                self.center2[1] += delta.y * per_px;
            }
        } else if self.kind == ViewKind::Scene {
            let flat = self.projection != Projection::Perspective;
            // (an orthographic view has nothing to orbit: the button that orbits pans it)
            if orbit && !flat {
                self.yaw -= delta.x * 0.008;
                self.pitch = (self.pitch + delta.y * 0.008).clamp(-1.5, 1.5);
            }
            if pan || (orbit && flat) {
                let per_px = if flat { self.ortho_h } else { 2.0 * self.dist * cam.tan } / rect.height().max(1.0);
                self.target = add(self.target, add(mul(cam.right, -delta.x * per_px), mul(cam.up, delta.y * per_px)));
            }
            // fly: with the right button held, the keys move the view
            if resp.dragged_by(PointerButton::Secondary) && !flat {
                let (fwd, right, up) = ui.input(|i| {
                    let k = |key: Key| if i.key_down(key) { 1.0 } else { 0.0 };
                    (k(Key::W) - k(Key::S), k(Key::D) - k(Key::A), k(Key::E) - k(Key::Q))
                });
                if fwd != 0.0 || right != 0.0 || up != 0.0 {
                    let dt = ui.input(|i| i.stable_dt).min(0.1);
                    let speed = self.dist.max(2.0) * 1.2 * self.fly_speed * if shift { 3.0 } else { 1.0 };
                    let flat = norm([cam.fwd[0], 0.0, cam.fwd[2]]);
                    let step = add(add(mul(flat, fwd * speed * dt), mul(cam.right, right * speed * dt)), mul([0.0, 1.0, 0.0], up * speed * dt));
                    self.target = add(self.target, step);
                    ui.ctx().request_repaint();
                }
            }
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 && resp.dragged_by(PointerButton::Secondary) && self.kind == ViewKind::Scene && self.projection == Projection::Perspective {
                // flying: the wheel sets the speed, as in the other editors
                self.fly_speed = fly_speed_after_scroll(self.fly_speed, scroll);
            } else if scroll != 0.0 {
                let k = (1.0 - scroll * 0.0015).clamp(0.5, 2.0);
                self.dist = (self.dist * k).clamp(0.3, 2000.0);
                self.ortho_h = (self.ortho_h * k).clamp(0.5, 4000.0);
            }
        }
    }

    // ------------------------------------------------------------------------------------------------------------- picking and the handles

    fn pixel(&self, resp: &egui::Response, rect: Rect, ppp: f32) -> Option<(usize, usize)> {
        let p = resp.interact_pointer_pos().or_else(|| resp.hover_pos())?;
        let (x, y) = (((p.x - rect.min.x) * ppp) as i64, ((p.y - rect.min.y) * ppp) as i64);
        (x >= 0 && y >= 0 && (x as u32) < self.img.0 && (y as u32) < self.img.1).then_some((x as usize, y as usize))
    }

    fn pick(&mut self, studio: &mut Studio, resp: &egui::Response, rect: Rect, ppp: f32, additive: bool) {
        let Some((x, y)) = self.pixel(resp, rect, ppp) else { return };
        let uid = self.ids.get(y * self.img.0 as usize + x).copied().unwrap_or(0);
        if additive {
            // Ctrl+click on something adds it (or takes it out); on nothing it changes nothing
            if uid != 0 {
                studio.toggle_select(uid);
            }
        } else {
            studio.select((uid != 0).then_some(uid));
        }
    }

    /// The icon of something that has no picture (a camera, a light) that is near the pointer.
    fn icon_at(&self, studio: &Studio, rect: Rect, pointer: Pos2) -> Option<u64> {
        self.cam?;
        let (w, h) = (rect.width(), rect.height());
        let mut best: Option<(f32, u64)> = None;
        for r in &studio.all {
            if !matches!(r.kind, Kind::Camera | Kind::Light) {
                continue;
            }
            let Some(p) = r.pos else { continue };
            let Some(s) = to_screen(&self.view_proj, p, w, h) else { continue };
            let d = ((rect.min.x + s[0] - pointer.x).powi(2) + (rect.min.y + s[1] - pointer.y).powi(2)).sqrt();
            if d < 14.0 && best.is_none_or(|b| d < b.0) {
                best = Some((d, r.uid));
            }
        }
        best.map(|b| b.1)
    }

    fn gizmo_len(&self, cam: &Cam, at: V3) -> f32 {
        match cam.ortho {
            Some(h) => h * 0.14,
            None => len(sub(at, cam.eye)) * 0.15 * (cam.tan / 0.577),
        }
    }

    /// The handles of the selection for the current tool.
    fn handles(&self, studio: &Studio, cam: &Cam) -> Option<Handles> {
        if !self.gizmos || self.tool == Tool::Hand {
            return None;
        }
        let origin = studio.entity_world_pos()?;
        let rot = studio.entity.as_ref().and_then(|e| e.quat("Transform", "rot")).unwrap_or([0.0, 0.0, 0.0, 1.0]);
        Some(Handles { origin, len: self.gizmo_len(cam, origin), rot, local: self.local_space && self.kind != ViewKind::Scene2D })
    }

    /// The point at `t` (0 to 1) along an axis handle of the move tool, on the screen (for tests and hints).
    pub fn handle_point(&self, studio: &Studio, axis: Axis, t: f32) -> Option<Pos2> {
        let cam = self.basis(self.last_rect.width() / self.last_rect.height().max(1.0));
        let h = self.handles(studio, &cam)?;
        self.project(add(h.origin, mul(h.axis_dir(Tool::Move, axis), h.len * t)))
    }

    /// A point on the rotate tool's ring about `axis`, `degrees` round it, on the screen (for tests).
    pub fn ring_point(&self, studio: &Studio, axis: Axis, degrees: f32) -> Option<Pos2> {
        let cam = self.basis(self.last_rect.width() / self.last_rect.height().max(1.0));
        let h = self.handles(studio, &cam)?;
        let (u, v) = h.plane_of(axis);
        let a = degrees.to_radians();
        self.project(add(h.origin, mul(add(mul(u, a.cos()), mul(v, a.sin())), h.len * 0.85)))
    }

    fn ray_at(&self, rect: Rect, p: Pos2, cam: &Cam) -> (V3, V3) {
        let (w, h) = (rect.width().max(1.0), rect.height().max(1.0));
        cam.ray(((p.x - rect.min.x) / w) * 2.0 - 1.0, 1.0 - ((p.y - rect.min.y) / h) * 2.0)
    }

    fn interact(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, studio: &mut Studio, ppp: f32, cam: &Cam) {
        let (w, h) = (rect.width(), rect.height());
        let handles = self.handles(studio, cam);
        let two_d = self.kind == ViewKind::Scene2D;
        // which handle is under the pointer
        self.hover_axis = None;
        if let (Some(p), Some(hd)) = (resp.hover_pos(), handles.as_ref()) {
            if self.drag.is_none() {
                self.hover_axis = gizmo::hit(self.tool, hd, &self.view_proj, w, h, [p.x - rect.min.x, p.y - rect.min.y], two_d);
            }
        }
        // start a drag of the selection by a handle
        if resp.drag_started_by(PointerButton::Primary) && !ui.input(|i| i.modifiers.alt) {
            // the drag is recognised a few pixels after the button went down: the handle and the grab point are those of the press
            let press = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
            let axis = match (press, handles.as_ref()) {
                (Some(p), Some(hd)) => gizmo::hit(self.tool, hd, &self.view_proj, w, h, [p.x - rect.min.x, p.y - rect.min.y], two_d).or(self.hover_axis),
                _ => self.hover_axis,
            };
            if let (Some(axis), Some(hd), Some(p), Some(ent)) = (axis, handles, press, studio.entity.clone()) {
                let (ro, rd) = self.ray_at(rect, p, cam);
                if let Some(grab) = gizmo::grab_value(self.tool, &hd, axis, ro, rd) {
                    let group = studio.next_group();
                    let starts: Vec<(u64, V3)> = studio.selected.iter().filter_map(|u| studio.world_of(*u).map(|p| (*u, p))).collect();
                    self.drag = Some(Drag { tool: self.tool, axis, grab, handles: hd, start_scale: ent.vec3("Transform", "scale").unwrap_or([1.0; 3]), start_rot: ent.quat("Transform", "rot").unwrap_or([0.0, 0.0, 0.0, 1.0]), group, starts });
                }
            }
        }
        if let Some(d) = &self.drag {
            if resp.dragged_by(PointerButton::Primary) {
                if let (Some(p), Some(sel)) = (resp.interact_pointer_pos(), studio.selection) {
                    let (ro, rd) = self.ray_at(rect, p, cam);
                    let (ctrl, shift) = ui.input(|i| (i.modifiers.ctrl, i.modifiers.shift));
                    let group = d.group.clone();
                    // the step in force: the one chosen in the toolbar, or with Ctrl held the usual one
                    let step = |set: f32, usual: f32| if set > 0.0 { set } else if ctrl { usual } else { 0.0 };
                    let (move_step, rot_step, scale_step) = (step(self.snap_move, 0.5), step(self.snap_rot, 15.0), step(self.snap_scale, 0.1));
                    match d.tool {
                        Tool::Move => {
                            if let Some(mut np) = gizmo::moved(&d.handles, d.axis, d.grab, ro, rd) {
                                if move_step > 0.0 {
                                    np = gizmo::snapped_move(&d.handles, d.axis, np, move_step);
                                }
                                // the others move by the same amount as the main one
                                let delta = [np[0] - d.handles.origin[0], np[1] - d.handles.origin[1], np[2] - d.handles.origin[2]];
                                let starts = d.starts.clone();
                                studio.move_to(sel, np, &group);
                                for (u, p0) in starts {
                                    if u != sel {
                                        studio.move_to(u, [p0[0] + delta[0], p0[1] + delta[1], p0[2] + delta[2]], &group);
                                    }
                                }
                                self.forced = true;
                            }
                        }
                        Tool::Scale => {
                            if let Some(f) = gizmo::scaled(&d.handles, d.axis, d.grab, ro, rd) {
                                let mut s = d.start_scale;
                                if shift {
                                    s = [s[0] * f, s[1] * f, s[2] * f];
                                } else {
                                    s[d.axis.index()] *= f;
                                }
                                if scale_step > 0.0 {
                                    s = [(s[0] / scale_step).round() * scale_step, (s[1] / scale_step).round() * scale_step, (s[2] / scale_step).round() * scale_step];
                                    s = [s[0].max(scale_step), s[1].max(scale_step), s[2].max(scale_step)];
                                }
                                studio.scale_to(sel, s, &group);
                                self.forced = true;
                            }
                        }
                        Tool::Rotate => {
                            if let Some(q) = gizmo::turned(&d.handles, d.axis, d.grab, ro, rd, (rot_step > 0.0).then_some(rot_step)) {
                                studio.rotate_to(sel, q_norm(q_mul(q, d.start_rot)), &group);
                                self.forced = true;
                            }
                        }
                        Tool::Hand => {}
                    }
                }
            }
            if resp.drag_stopped() {
                self.drag = None;
            }
        }
        if self.hover_axis.is_some() || self.drag.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        // a click selects: the entity drawn under the pointer, or a camera or light icon
        if resp.clicked_by(PointerButton::Primary) && self.drag.is_none() && self.tool != Tool::Hand {
            if let Some(p) = resp.interact_pointer_pos() {
                let additive = ui.input(|i| i.modifiers.command);
                if let Some(uid) = self.icon_at(studio, rect, p) {
                    if additive {
                        studio.toggle_select(uid);
                    } else {
                        studio.select(Some(uid));
                    }
                } else {
                    self.pick(studio, resp, rect, ppp, additive);
                }
            }
        }
        if resp.double_clicked() {
            self.focus_selection(studio);
        }
    }

    // ------------------------------------------------------------------------------------------------------------- overlays

    /// The outlines of several selected entities: the main one in the usual colour, the others dimmer.
    fn multi_outline(&self, painter: &egui::Painter, rect: Rect, studio: &Studio) {
        if self.img.0 == 0 || self.ids.len() != (self.img.0 * self.img.1) as usize {
            return;
        }
        let (w, h) = (self.img.0 as usize, self.img.1 as usize);
        let mut boxes: std::collections::HashMap<u64, (usize, usize, usize, usize)> = std::collections::HashMap::new();
        let (mut last, mut last_wanted) = (0u64, false);
        for y in 0..h {
            for x in 0..w {
                let id = self.ids[y * w + x];
                if id != last {
                    last = id;
                    last_wanted = id != 0 && studio.is_selected(id);
                }
                if last_wanted {
                    let b = boxes.entry(id).or_insert((x, y, x, y));
                    b.0 = b.0.min(x);
                    b.1 = b.1.min(y);
                    b.2 = b.2.max(x);
                    b.3 = b.3.max(y);
                }
            }
        }
        let (sx, sy) = (rect.width() / w as f32, rect.height() / h as f32);
        for (uid, (x0, y0, x1, y1)) in boxes {
            let r = Rect::from_min_max(Pos2::new(rect.min.x + x0 as f32 * sx - 1.0, rect.min.y + y0 as f32 * sy - 1.0), Pos2::new(rect.min.x + (x1 + 1) as f32 * sx + 1.0, rect.min.y + (y1 + 1) as f32 * sy + 1.0));
            let main = studio.selection == Some(uid);
            painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(if main { 1.5 } else { 1.0 }, if main { theme::OUTLINE } else { theme::OUTLINE.gamma_multiply(0.6) }), StrokeKind::Outside);
        }
    }

    fn selection_outline(&self, painter: &egui::Painter, rect: Rect, studio: &Studio) {
        if studio.selected.len() > 1 {
            self.multi_outline(painter, rect, studio);
            return;
        }
        let Some(sel) = studio.selection else { return };
        if self.img.0 == 0 || self.ids.len() != (self.img.0 * self.img.1) as usize {
            return;
        }
        let (w, h) = (self.img.0 as usize, self.img.1 as usize);
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
        for y in 0..h {
            let row = &self.ids[y * w..(y + 1) * w];
            if let (Some(a), Some(b)) = (row.iter().position(|v| *v == sel), row.iter().rposition(|v| *v == sel)) {
                x0 = x0.min(a);
                x1 = x1.max(b);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
        if x0 == usize::MAX {
            return;
        }
        let (sx, sy) = (rect.width() / w as f32, rect.height() / h as f32);
        let r = Rect::from_min_max(Pos2::new(rect.min.x + x0 as f32 * sx - 1.0, rect.min.y + y0 as f32 * sy - 1.0), Pos2::new(rect.min.x + (x1 + 1) as f32 * sx + 1.0, rect.min.y + (y1 + 1) as f32 * sy + 1.0));
        painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.5, theme::OUTLINE), StrokeKind::Outside);
    }

    fn overlays(&self, painter: &egui::Painter, rect: Rect, studio: &Studio, cam: &Cam) {
        let (w, h) = (rect.width(), rect.height());
        let at = |p: [f32; 2]| Pos2::new(rect.min.x + p[0], rect.min.y + p[1]);
        // the ground grid
        // (the ground grid is edge-on in the views from the side: only the perspective and the top and bottom views have it)
        if self.grid && self.kind == ViewKind::Scene && matches!(self.projection, Projection::Perspective | Projection::Top | Projection::Bottom) {
            let mut step = 1.0f32;
            let extent = if self.projection == Projection::Perspective { self.dist } else { self.ortho_h };
            while extent / step > 14.0 {
                step *= 10.0;
            }
            let n = 20i32;
            let (cx, cz) = ((self.target[0] / step).round() * step, (self.target[2] / step).round() * step);
            for i in -n..=n {
                let k = i as f32 * step;
                let fade = 1.0 - (i.abs() as f32 / n as f32).powi(2);
                let alpha = (34.0 * fade) as u8;
                let major = ((cx / step).round() as i32 + i) % 10 == 0;
                for (a, b, axis_line, along_z) in [([cx + k, 0.0, cz - n as f32 * step], [cx + k, 0.0, cz + n as f32 * step], (cx + k).abs() < 1e-3, true), ([cx - n as f32 * step, 0.0, cz + k], [cx + n as f32 * step, 0.0, cz + k], (cz + k).abs() < 1e-3, false)] {
                    if let Some((p, q)) = segment_to_screen(&self.view_proj, a, b, w, h) {
                        let color = if axis_line {
                            if along_z { Color32::from_rgba_unmultiplied(59, 128, 222, 150) } else { Color32::from_rgba_unmultiplied(219, 64, 52, 150) }
                        } else {
                            Color32::from_rgba_unmultiplied(255, 255, 255, if major { alpha.saturating_add(14) } else { alpha })
                        };
                        painter.line_segment([at(p), at(q)], Stroke::new(1.0, color));
                    }
                }
            }
        }
        if self.streaming && self.kind == ViewKind::Scene {
            crate::streaming_ui::draw(painter, rect, &self.view_proj, cam.eye, &self.zones.zones);
        }
        // icons for what has no picture
        for r in &studio.all {
            if !matches!(r.kind, Kind::Camera | Kind::Light) {
                continue;
            }
            let Some(p) = r.pos else { continue };
            let Some(s) = to_screen(&self.view_proj, p, w, h) else { continue };
            let c = at(s);
            let selected = studio.is_selected(r.uid);
            let icon = Icon::of_kind(r.kind);
            icons::draw(painter, Rect::from_center_size(c, Vec2::splat(22.0)), icon, if selected { theme::OUTLINE } else { icon.tint() });
        }
        self.selection_outline(painter, rect, studio);
        // the handles of the tool
        if let Some(hd) = self.handles(studio, cam) {
            let active = self.drag.as_ref().map(|d| d.axis);
            gizmo::draw(painter, &at, self.tool, &hd, &self.view_proj, w, h, self.hover_axis, active, self.kind == ViewKind::Scene2D);
        }
        // the orientation gizmo in the corner: which way is which
        if self.kind == ViewKind::Scene {
            let origin = Pos2::new(rect.max.x - 50.0, rect.min.y + 52.0);
            let mut axes: Vec<(f32, Axis, f32, f32)> = Axis::ALL.iter().map(|a| (dot(a.dir(), cam.fwd), *a, dot(a.dir(), cam.right), -dot(a.dir(), cam.up))).collect();
            axes.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal));
            for (_, axis, sx, sy) in axes {
                let tip = origin + Vec2::new(sx, sy) * 28.0;
                painter.line_segment([origin, tip], Stroke::new(2.0, axis.color()));
                painter.circle_filled(tip, 8.0, axis.color());
                painter.text(tip, Align2::CENTER_CENTER, match axis { Axis::X => "X", Axis::Y => "Y", Axis::Z => "Z" }, FontId::proportional(10.0), Color32::from_rgb(20, 20, 20));
            }
            painter.text(origin + Vec2::new(0.0, 46.0), Align2::CENTER_CENTER, format!("< {}", if self.projection == Projection::Perspective { "Persp" } else { self.projection.label() }), FontId::proportional(11.0), Color32::from_rgba_unmultiplied(220, 220, 225, 170));
        }
    }
}

/// The fly speed after the wheel turned by `scroll` points while flying: 15% a notch (a notch is about 50 points), kept between a tenth and ten times the usual speed.
pub fn fly_speed_after_scroll(speed: f32, scroll: f32) -> f32 {
    (speed * (1.0 + scroll * 0.003).clamp(0.5, 2.0)).clamp(0.1, 10.0)
}

/// The scene toolbar is drawn in its short form (the switches under "Show", the snap steps under "Snap") when the view is narrower than this many points: the long form needs about 870.
const NARROW_TOOLBAR: f32 = 940.0;

/// How far an orthographic view's eye is from its target, along the way it looks.
const ORTHO_EYE_DISTANCE: f32 = 1000.0;

fn snap_label(title: &str, unit: &str, value: f32) -> String {
    if value > 0.0 { format!("{title} {value}{unit}") } else { format!("{title} off") }
}

/// The rows of a snap menu: "Off" and the usual steps.
fn snap_items(ui: &mut egui::Ui, unit: &str, value: &mut f32, steps: &[f32]) {
    for s in std::iter::once(0.0).chain(steps.iter().copied()) {
        let text = if s == 0.0 { "Off".to_string() } else { format!("{s}{unit}") };
        if ui.selectable_label(*value == s, text).clicked() {
            *value = s;
            ui.close();
        }
    }
}

/// A drop-down pill for a snap step: "Off" and the usual steps.
fn snap_menu(ui: &mut egui::Ui, title: &str, unit: &str, value: &mut f32, steps: &[f32]) {
    let label = snap_label(title, unit, *value);
    chrome::drop_down(ui, &label, |ui| snap_items(ui, unit, value, steps));
}

#[cfg(test)]
mod gpu_tests {
    use super::*;

    fn stats(passes: &[(&str, f32)]) -> GpuStats {
        GpuStats { passes: passes.iter().map(|(n, ms)| (n.to_string(), *ms)).collect(), ..GpuStats::default() }
    }

    #[test]
    fn the_card_total_is_the_passes_without_a_dot_and_none_when_nothing_was_timed() {
        assert_eq!(GpuStats::default().total_ms(), None);
        let s = stats(&[("shadows", 1.0), ("main", 3.0), ("main.meshes", 2.0), ("main.sky", 0.5), ("post", 0.5)]);
        assert_eq!(s.total_ms(), Some(4.5), "the parts are inside their pass");
    }

    #[test]
    fn rows_put_the_slowest_pass_first_each_followed_by_its_parts_and_orphans_last() {
        let s = stats(&[("shadows", 1.0), ("main.sky", 0.5), ("main", 3.0), ("main.meshes", 2.0), ("post", 0.25), ("bloom.blur", 0.1)]);
        let names: Vec<(String, bool)> = s.rows().into_iter().map(|r| (r.name, r.part)).collect();
        let want = [("main", false), ("main.meshes", true), ("main.sky", true), ("shadows", false), ("post", false), ("bloom.blur", true)];
        assert_eq!(names, want.map(|(n, p)| (n.to_string(), p)));
    }
}

