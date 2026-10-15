//! The Project panel, in Unity's two-column layout: the folders of the workspace on the left, what is in the chosen folder as tiles on the right.
use crate::icons::{self, Icon};
use crate::state::{Studio, Template};
use crate::theme;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Sense, Stroke, StrokeKind, Vec2};
use lunaris_json::Json;
use std::collections::HashSet;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct ProjectUi {
    folder: String,
    filter: String,
    loaded: Option<Instant>,
    files: Vec<(String, String, u64)>,
    pub selected: Option<String>,
    pub open_scene: Option<String>,
    closed: HashSet<String>,
}

fn icon_of(kind: &str) -> Icon {
    match kind {
        "scene" => Icon::Scene,
        "model" => Icon::Model,
        "texture" => Icon::Image,
        "sound" => Icon::Sound,
        _ => Icon::File,
    }
}

fn human(b: u64) -> String {
    if b >= 1_000_000 {
        format!("{:.1} MB", b as f64 / 1e6)
    } else if b >= 1000 {
        format!("{:.0} kB", b as f64 / 1e3)
    } else {
        format!("{b} B")
    }
}

impl ProjectUi {
    pub fn reload(&mut self) {
        self.loaded = None;
    }

    fn load(&mut self, studio: &mut Studio) {
        if self.loaded.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) {
            return;
        }
        self.loaded = Some(Instant::now());
        self.files.clear();
        if let Ok(f) = studio.engine.call("files", &Json::obj([("recursive", Json::Bool(true)), ("limit", Json::from(2000))])) {
            for e in f.get("files").and_then(|x| x.as_array()).unwrap_or(&[]) {
                if let (Some(p), Some(k)) = (e.get("path").and_then(|p| p.as_str()), e.get("kind").and_then(|k| k.as_str())) {
                    if p.split('/').any(|part| part == "__pycache__" || part == "node_modules" || part == ".venv") {
                        continue;
                    }
                    self.files.push((p.to_string(), k.to_string(), e.get("bytes").and_then(|b| b.as_u64()).unwrap_or(0)));
                }
            }
        }
    }

    /// Every folder that holds a file, sorted so that a folder comes before its children.
    fn folders(&self) -> Vec<String> {
        let mut folders: Vec<String> = vec![String::new()];
        for (p, _, _) in &self.files {
            let parts: Vec<&str> = p.split('/').collect();
            let mut dir = String::new();
            for part in &parts[..parts.len().saturating_sub(1)] {
                if !dir.is_empty() {
                    dir.push('/');
                }
                dir.push_str(part);
                if !folders.contains(&dir) {
                    folders.push(dir.clone());
                }
            }
        }
        folders.sort();
        folders
    }

    pub fn show(&mut self, ui: &mut egui::Ui, studio: &mut Studio) {
        self.load(studio);
        let folders = self.folders();
        let tree_w = 190.0;
        ui.horizontal_top(|ui| {
            // ---- the folders
            ui.allocate_ui_with_layout(Vec2::new(tree_w, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(tree_w);
                let rect = ui.max_rect();
                ui.painter().rect_filled(rect, CornerRadius::ZERO, theme::FIELD);
                egui::ScrollArea::vertical().id_salt("project-folders").auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for f in &folders {
                        // hidden when an ancestor is folded
                        if f.split('/').scan(String::new(), |acc, part| {
                            if !acc.is_empty() {
                                acc.push('/');
                            }
                            acc.push_str(part);
                            Some(acc.clone())
                        }).any(|anc| anc != *f && self.closed.contains(&anc)) {
                            continue;
                        }
                        let depth = if f.is_empty() { 0 } else { f.matches('/').count() + 1 };
                        let has_kids = folders.iter().any(|g| g.len() > f.len() && g.starts_with(f.as_str()) && (f.is_empty() || g.as_bytes().get(f.len()) == Some(&b'/')) && g != f);
                        let name = if f.is_empty() { "Assets".to_string() } else { f.rsplit('/').next().unwrap_or(f).to_string() };
                        let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 19.0), Sense::click());
                        if self.folder == *f {
                            ui.painter().rect_filled(row, CornerRadius::ZERO, theme::SELECT);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(row, CornerRadius::ZERO, theme::HOVER_ROW);
                        }
                        let x = row.min.x + 6.0 + depth as f32 * 13.0;
                        if has_kids {
                            let c = egui::pos2(x + 4.0, row.center().y);
                            let open = !self.closed.contains(f);
                            let pts = if open { vec![c + Vec2::new(-3.5, -2.0), c + Vec2::new(3.5, -2.0), c + Vec2::new(0.0, 2.5)] } else { vec![c + Vec2::new(-2.0, -3.5), c + Vec2::new(-2.0, 3.5), c + Vec2::new(2.5, 0.0)] };
                            ui.painter().add(egui::Shape::convex_polygon(pts, theme::TEXT_DIM, Stroke::NONE));
                            if resp.clicked() && resp.interact_pointer_pos().is_some_and(|p| p.x < x + 10.0) {
                                if !self.closed.remove(f) {
                                    self.closed.insert(f.clone());
                                }
                                continue;
                            }
                        }
                        icons::draw(ui.painter(), egui::Rect::from_center_size(egui::pos2(x + 20.0, row.center().y), Vec2::splat(15.0)), Icon::Folder, Icon::Folder.tint());
                        ui.painter().text(egui::pos2(x + 32.0, row.center().y), Align2::LEFT_CENTER, name, FontId::proportional(13.0), theme::TEXT);
                        if resp.clicked() {
                            self.folder = f.clone();
                        }
                    }
                });
            });
            // ---- the divider
            let (rect, _) = ui.allocate_exact_size(Vec2::new(2.0, ui.available_height()), Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::ZERO, theme::LINE);
            // ---- the folder's contents
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    // the path as clickable pieces: Assets > examples > sounds
                    if ui.link("Assets").clicked() {
                        self.folder.clear();
                    }
                    let mut acc = String::new();
                    for part in self.folder.clone().split('/').filter(|p| !p.is_empty()) {
                        ui.label(egui::RichText::new(">").color(theme::TEXT_DIM));
                        if !acc.is_empty() {
                            acc.push('/');
                        }
                        acc.push_str(part);
                        if ui.link(part).clicked() {
                            self.folder = acc.clone();
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Refresh").clicked() {
                            self.loaded = None;
                        }
                        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Search").desired_width(160.0));
                    });
                });
                ui.add_space(2.0);
                let filter = self.filter.to_lowercase();
                let mut open: Option<String> = None;
                let mut go: Option<String> = None;
                egui::ScrollArea::vertical().id_salt("project-files").auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                    ui.horizontal_wrapped(|ui| {
                        // sub-folders first
                        let mut items: Vec<(String, String, String, u64)> = Vec::new(); // (display, path, kind, bytes)
                        if filter.is_empty() {
                            for f in &folders {
                                let parent = f.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
                                if !f.is_empty() && parent == self.folder {
                                    items.push((f.rsplit('/').next().unwrap_or(f).to_string(), f.clone(), "folder".to_string(), 0));
                                }
                            }
                        }
                        for (p, kind, bytes) in &self.files {
                            let dir = p.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                            let name = p.rsplit('/').next().unwrap_or(p);
                            if filter.is_empty() {
                                if dir != self.folder {
                                    continue;
                                }
                            } else if !p.to_lowercase().contains(&filter) {
                                continue;
                            }
                            items.push((name.to_string(), p.clone(), kind.clone(), *bytes));
                        }
                        for (name, path, kind, bytes) in items {
                            let (rect, resp) = ui.allocate_exact_size(Vec2::new(78.0, 86.0), Sense::click());
                            let sel = self.selected.as_deref() == Some(path.as_str());
                            if sel {
                                ui.painter().rect_filled(rect, CornerRadius::same(2), theme::SELECT);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(rect, CornerRadius::same(2), theme::HOVER_ROW);
                            }
                            let tile = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.min.y + 32.0), Vec2::splat(54.0));
                            ui.painter().rect_filled(tile, CornerRadius::same(2), if sel { Color32::from_rgb(34, 70, 104) } else { theme::FIELD });
                            let icon = if kind == "folder" { Icon::Folder } else { icon_of(&kind) };
                            icons::draw(ui.painter(), tile.shrink(10.0), icon, icon.tint());
                            let galley = ui.painter().layout(name.clone(), FontId::proportional(12.0), if sel { theme::TEXT_BRIGHT } else { theme::TEXT }, 74.0);
                            let h = galley.size().y.min(30.0);
                            let at = egui::pos2(rect.center().x - galley.size().x.min(74.0) / 2.0, rect.min.y + 62.0);
                            ui.painter().with_clip_rect(egui::Rect::from_min_size(egui::pos2(rect.min.x, rect.min.y + 60.0), Vec2::new(78.0, h + 2.0))).galley(at, galley, theme::TEXT);
                            if !filter.is_empty() {
                                resp.clone().on_hover_text(&path);
                            } else if bytes > 0 {
                                resp.clone().on_hover_text(format!("{path}\n{}", human(bytes)));
                            }
                            if resp.clicked() {
                                self.selected = Some(path.clone());
                            }
                            if resp.double_clicked() {
                                if kind == "folder" {
                                    go = Some(path.clone());
                                } else if kind == "scene" {
                                    open = Some(path.clone());
                                }
                            }
                            resp.context_menu(|ui| {
                                if kind == "scene" && ui.button("Open scene").clicked() {
                                    open = Some(path.clone());
                                    ui.close();
                                }
                                if kind == "model" && ui.button("Add to the scene").clicked() {
                                    if let Some(uid) = studio.spawn_template(Template::Empty, [0.0, 0.0, 0.0]) {
                                        let _ = studio.try_call("add_component", Json::obj([("uid", Json::from(uid as i64)), ("component", Json::str("MeshRenderer")), ("value", Json::obj([("mesh", Json::str(path.clone()))]))]));
                                    }
                                    ui.close();
                                }
                                if kind == "sound" && ui.button("Add as an Audio Source").clicked() {
                                    if let Some(uid) = studio.spawn_template(Template::AudioSource, [0.0, 0.0, 0.0]) {
                                        studio.set(uid, "AudioSource", Json::obj([("file", Json::str(path.clone())), ("voice", Json::str(""))]), None);
                                    }
                                    ui.close();
                                }
                                if kind == "texture" && studio.selection.is_some() && ui.button("Use on the selected mesh").clicked() {
                                    if let Some(uid) = studio.selection {
                                        studio.set(uid, "MeshRenderer", Json::obj([("texture", Json::str(path.clone()))]), None);
                                    }
                                    ui.close();
                                }
                            });
                        }
                    });
                });
                if let Some(p) = go {
                    self.folder = p;
                }
                if let Some(p) = open {
                    self.open_scene = Some(p);
                }
            });
        });
        let _ = StrokeKind::Inside;
    }
}
