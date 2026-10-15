//! The scene view: the engine's own renderer draws the world into a picture, and the editor lays a grid, icons, the selection outline and
//! the tool handles over it. Looking around, picking and dragging are plain arithmetic on the camera the editor chose, so they agree with
//! what is on the screen.
use crate::chrome;
use crate::gizmo::{self, Handles};
pub use crate::gizmo::{Axis, Tool};
use crate::icons::{self, Icon};
use crate::mathx::*;
use crate::state::{Kind, Mode, Studio};
use crate::theme;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Key, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, TextureOptions, Vec2};
use lunaris_gpu::{GpuRenderer, Quality};
use lunaris_render::{ExplicitCamera, Frame, RenderOptions, TextureCache, View};

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

pub struct Viewport {
    pub kind: ViewKind,
    pub tool: Tool,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub target: V3,
    pub ortho_h: f32,
    pub center2: [f32; 2],
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
    hover_axis: Option<Axis>,
    pub grid: bool,
    pub gizmos: bool,
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
            hover_axis: None,
            grid: true,
            gizmos: true,
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

    pub fn focus_selection(&mut self, studio: &Studio) {
        if let Some(p) = studio.entity_world_pos() {
            let size = studio.entity.as_ref().and_then(|e| e.vec3("Transform", "scale")).map(|s| s[0].abs().max(s[1].abs()).max(s[2].abs())).unwrap_or(1.0).clamp(0.5, 50.0);
            self.focus(p, size);
        }
    }

    fn basis(&self, aspect: f32) -> Cam {
        match self.kind {
            ViewKind::Scene2D => Cam { eye: [self.center2[0], self.center2[1], 100.0], fwd: [0.0, 0.0, -1.0], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], tan: 0.5, aspect, ortho: Some(self.ortho_h) },
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
                        r.render_frame(&mut studio.engine.sim.world, &mut self.textures, &opts, &q, want_ids)
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
                self.info = format!("{} - {}x{} - {:.1} ms{}", used, f.width, f.height, started.elapsed().as_secs_f32() * 1000.0, if f.warnings.is_empty() { String::new() } else { format!(" - {}", f.warnings.join("; ")) });
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
                _ => {
                    self.kind = if t == crate::center::CenterTab::Game { ViewKind::Game } else if self.kind == ViewKind::Scene2D { ViewKind::Scene2D } else { ViewKind::Scene };
                    self.forced = true;
                }
            }
        }
        // the scene toolbar
        let bar = Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), 24.0));
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
            chrome::rule(ui);
            let two_d = self.kind == ViewKind::Scene2D;
            if ui.selectable_label(two_d, "2D").clicked() && self.kind != ViewKind::Game {
                self.kind = if two_d { ViewKind::Scene } else { ViewKind::Scene2D };
                self.forced = true;
            }
            ui.toggle_value(&mut self.grid, "Grid");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.toggle_value(&mut self.gizmos, "Gizmos");
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
        if !game_play && self.kind != ViewKind::Game {
            self.interact(ui, &resp, rect, studio, ppp, &cam);
        } else if resp.clicked() && !playing {
            self.pick(studio, &resp, rect, ppp);
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

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(48, 48, 50));
        if let Some(t) = &self.tex {
            painter.image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        if playing {
            // like the other editors: a tint over the view while the game runs
            painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(40, 70, 130, 22));
        }
        if self.kind != ViewKind::Game {
            self.overlays(&painter, rect, studio, &cam);
        } else {
            self.selection_outline(&painter, rect, studio);
        }
        if self.kind == ViewKind::Game && playing && self.game_input {
            painter.text(rect.right_top() + Vec2::new(-8.0, 6.0), Align2::RIGHT_TOP, "keyboard and mouse go to the game", FontId::proportional(11.0), theme::TEXT_DIM);
        }
        if let Some(e) = &self.error {
            painter.text(rect.center(), Align2::CENTER_CENTER, e, FontId::proportional(14.0), theme::ERR);
        }
        painter.text(rect.left_bottom() + Vec2::new(8.0, -6.0), Align2::LEFT_BOTTOM, &self.info, FontId::proportional(11.0), Color32::from_rgba_unmultiplied(200, 205, 215, 150));
        if studio.total_entities == 0 && self.error.is_none() {
            painter.text(rect.center(), Align2::CENTER_CENTER, "An empty scene. Use Entity > Create, or File > Open Scene.", FontId::proportional(14.0), theme::TEXT_DIM);
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
            if orbit {
                self.yaw -= delta.x * 0.008;
                self.pitch = (self.pitch + delta.y * 0.008).clamp(-1.5, 1.5);
            }
            if pan {
                let per_px = 2.0 * self.dist * cam.tan / rect.height().max(1.0);
                self.target = add(self.target, add(mul(cam.right, -delta.x * per_px), mul(cam.up, delta.y * per_px)));
            }
            // fly: with the right button held, the keys move the view
            if resp.dragged_by(PointerButton::Secondary) {
                let (fwd, right, up) = ui.input(|i| {
                    let k = |key: Key| if i.key_down(key) { 1.0 } else { 0.0 };
                    (k(Key::W) - k(Key::S), k(Key::D) - k(Key::A), k(Key::E) - k(Key::Q))
                });
                if fwd != 0.0 || right != 0.0 || up != 0.0 {
                    let dt = ui.input(|i| i.stable_dt).min(0.1);
                    let speed = self.dist.max(2.0) * 1.2 * if shift { 3.0 } else { 1.0 };
                    let flat = norm([cam.fwd[0], 0.0, cam.fwd[2]]);
                    let step = add(add(mul(flat, fwd * speed * dt), mul(cam.right, right * speed * dt)), mul([0.0, 1.0, 0.0], up * speed * dt));
                    self.target = add(self.target, step);
                    ui.ctx().request_repaint();
                }
            }
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
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

    fn pick(&mut self, studio: &mut Studio, resp: &egui::Response, rect: Rect, ppp: f32) {
        let Some((x, y)) = self.pixel(resp, rect, ppp) else { return };
        let uid = self.ids.get(y * self.img.0 as usize + x).copied().unwrap_or(0);
        studio.select((uid != 0).then_some(uid));
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
        Some(Handles { origin, len: self.gizmo_len(cam, origin), rot })
    }

    /// The point at `t` (0 to 1) along an axis handle of the move tool, on the screen (for tests and hints).
    pub fn handle_point(&self, studio: &Studio, axis: Axis, t: f32) -> Option<Pos2> {
        let cam = self.basis(self.last_rect.width() / self.last_rect.height().max(1.0));
        let h = self.handles(studio, &cam)?;
        self.project(add(h.origin, mul(axis.dir(), h.len * t)))
    }

    /// A point on the rotate tool's ring about `axis`, `degrees` round it, on the screen (for tests).
    pub fn ring_point(&self, studio: &Studio, axis: Axis, degrees: f32) -> Option<Pos2> {
        let cam = self.basis(self.last_rect.width() / self.last_rect.height().max(1.0));
        let h = self.handles(studio, &cam)?;
        let (u, v) = axis.plane();
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
                    self.drag = Some(Drag { tool: self.tool, axis, grab, handles: hd, start_scale: ent.vec3("Transform", "scale").unwrap_or([1.0; 3]), start_rot: ent.quat("Transform", "rot").unwrap_or([0.0, 0.0, 0.0, 1.0]), group });
                }
            }
        }
        if let Some(d) = &self.drag {
            if resp.dragged_by(PointerButton::Primary) {
                if let (Some(p), Some(sel)) = (resp.interact_pointer_pos(), studio.selection) {
                    let (ro, rd) = self.ray_at(rect, p, cam);
                    let (ctrl, shift) = ui.input(|i| (i.modifiers.ctrl, i.modifiers.shift));
                    let group = d.group.clone();
                    match d.tool {
                        Tool::Move => {
                            if let Some(mut np) = gizmo::moved(&d.handles, d.axis, d.grab, ro, rd) {
                                if ctrl {
                                    np = [(np[0] * 2.0).round() / 2.0, (np[1] * 2.0).round() / 2.0, (np[2] * 2.0).round() / 2.0];
                                }
                                studio.move_to(sel, np, &group);
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
                                studio.scale_to(sel, s, &group);
                                self.forced = true;
                            }
                        }
                        Tool::Rotate => {
                            if let Some(q) = gizmo::turned(&d.handles, d.axis, d.grab, ro, rd, ctrl.then_some(15.0)) {
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
                if let Some(uid) = self.icon_at(studio, rect, p) {
                    studio.select(Some(uid));
                } else {
                    self.pick(studio, resp, rect, ppp);
                }
            }
        }
        if resp.double_clicked() {
            self.focus_selection(studio);
        }
    }

    // ------------------------------------------------------------------------------------------------------------- overlays

    fn selection_outline(&self, painter: &egui::Painter, rect: Rect, studio: &Studio) {
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
        if self.grid && self.kind == ViewKind::Scene {
            let mut step = 1.0f32;
            while self.dist / step > 14.0 {
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
        // icons for what has no picture
        for r in &studio.all {
            if !matches!(r.kind, Kind::Camera | Kind::Light) {
                continue;
            }
            let Some(p) = r.pos else { continue };
            let Some(s) = to_screen(&self.view_proj, p, w, h) else { continue };
            let c = at(s);
            let selected = studio.selection == Some(r.uid);
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
            painter.text(origin + Vec2::new(0.0, 46.0), Align2::CENTER_CENTER, "< Persp", FontId::proportional(11.0), Color32::from_rgba_unmultiplied(220, 220, 225, 170));
        }
    }
}
