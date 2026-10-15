//! The Systems panel: the systems that run every tick (native ones, and the graph systems of the scene, which can be edited as text).
use crate::graph::GraphView;
use crate::state::Studio;
use crate::theme;
use eframe::egui::{self, FontId, RichText, Vec2};
use lunaris_json::{parse, Json};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct SystemsUi {
    loaded_at: Option<Instant>,
    graph: GraphView,
    hide_json: bool,
    shown: Option<String>,
}

impl SystemsUi {
    pub fn show(&mut self, ui: &mut egui::Ui, studio: &mut Studio) {
        if self.loaded_at.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
            self.loaded_at = Some(Instant::now());
            if let Ok(s) = studio.engine.call("systems", &Json::Null) {
                let mut list: Vec<Json> = s.get("graph").and_then(|g| g.as_array()).map(|a| a.to_vec()).unwrap_or_default();
                list.extend(s.get("native").and_then(|g| g.as_array()).map(|a| a.to_vec()).unwrap_or_default());
                studio.systems = list;
            }
        }
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(Vec2::new(240.0, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(240.0);
                egui::ScrollArea::vertical().id_salt("systems-list").auto_shrink([false, false]).show(ui, |ui| {
                    for s in studio.systems.clone() {
                        let name = s.get("name").and_then(|n| n.as_str()).unwrap_or("?").to_string();
                        let native = s.get("kind").and_then(|k| k.as_str()) == Some("native");
                        let enabled = s.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true);
                        ui.horizontal(|ui| {
                            if !native {
                                let mut e = enabled;
                                if ui.checkbox(&mut e, "").changed() {
                                    let _ = studio.try_call("enable_system", Json::obj([("name", Json::str(name.clone())), ("enabled", Json::Bool(e))]));
                                    self.loaded_at = None;
                                }
                            } else {
                                ui.label(RichText::new("native").small().color(theme::TEXT_DIM));
                            }
                            let label = if let Some(err) = s.get("last_error").and_then(|e| e.as_str()) { RichText::new(format!("{name}  (error)")).color(theme::ERR).on_hover(err) } else { RichText::new(&name) };
                            if ui.selectable_label(studio.selected_system.as_deref() == Some(name.as_str()), label).clicked() && !native {
                                studio.open_system(&name);
                            }
                        });
                    }
                    if ui.button("+ New graph system").clicked() {
                        studio.selected_system = Some("new_system".to_string());
                        studio.system_text = "{\n  \"name\": \"new_system\",\n  \"each\": [\"Transform\"],\n  \"do\": []\n}".to_string();
                        studio.system_error = None;
                    }
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                if studio.selected_system.is_some() {
                    // the picture follows the text; a different system is fitted into the view
                    let refit = self.shown != studio.selected_system;
                    self.shown = studio.selected_system.clone();
                    self.graph.set_text(&studio.system_text, refit);
                    ui.horizontal(|ui| {
                        if ui.button("Fit").clicked() {
                            self.graph.set_text(&studio.system_text, true);
                        }
                        ui.toggle_value(&mut self.hide_json, "Hide JSON");
                        if ui.button("Apply").clicked() {
                            match parse(&studio.system_text) {
                                Ok(def) => match studio.call("define_system", def) {
                                    Ok(_) => {
                                        studio.system_error = None;
                                        studio.toast("System applied", false);
                                        self.loaded_at = None;
                                    }
                                    Err(e) => studio.system_error = Some(e),
                                },
                                Err(e) => studio.system_error = Some(format!("not valid JSON: {e}")),
                            }
                        }
                        if ui.button("Edit as nodes").on_hover_text("Open this system in the Blueprint tab").clicked() {
                            studio.open_blueprint = studio.selected_system.clone();
                        }
                        if ui.button("Edit as script").on_hover_text("Open this system as text in the Script tab").clicked() {
                            studio.open_script = studio.selected_system.clone();
                        }
                        if ui.button("Remove").clicked() {
                            if let Some(n) = studio.selected_system.clone() {
                                let _ = studio.try_call("remove_system", Json::obj([("name", Json::str(n))]));
                                studio.selected_system = None;
                                self.loaded_at = None;
                            }
                        }
                        if let Some(e) = &studio.system_error {
                            ui.label(RichText::new(e).color(theme::ERR).small());
                        }
                    });
                    if let Some(e) = &self.graph.error {
                        ui.label(RichText::new(e).small().color(theme::WARN));
                    }
                    ui.horizontal_top(|ui| {
                        let json_w = if self.hide_json { 0.0 } else { 330.0 };
                        let graph_w = (ui.available_width() - json_w - 8.0).max(120.0);
                        ui.allocate_ui_with_layout(Vec2::new(graph_w, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                            if let Some(text) = self.graph.show(ui) {
                                studio.system_text = text;
                                studio.system_error = None;
                            }
                        });
                        if !self.hide_json {
                            egui::ScrollArea::vertical().id_salt("system-text").auto_shrink([false, false]).show(ui, |ui| {
                                ui.add(egui::TextEdit::multiline(&mut studio.system_text).font(FontId::monospace(12.0)).desired_width(f32::INFINITY).desired_rows(14));
                            });
                        }
                    });
                } else {
                    ui.label(RichText::new("Select a graph system to see and edit it.").color(theme::TEXT_DIM));
                }
            });
        });
    }
}

trait OnHover {
    fn on_hover(self, text: &str) -> Self;
}

impl OnHover for RichText {
    fn on_hover(self, _text: &str) -> Self {
        self
    }
}
