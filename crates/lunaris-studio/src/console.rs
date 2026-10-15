//! The Console panel: what the editor asked the engine and what came back, with the Unity toolbar (clear, search, counters of messages,
//! warnings and errors) and, underneath, a box to type any tool call.
use crate::icons::{self, Icon};
use crate::state::{LogKind, Studio};
use crate::theme;
use eframe::egui::{self, Align2, CornerRadius, FontId, RichText, Sense, Vec2};
use lunaris_json::{parse, Json};

pub struct ConsoleUi {
    show_calls: bool,
    show_info: bool,
    show_errors: bool,
    search: String,
    selected: Option<usize>,
}

impl Default for ConsoleUi {
    fn default() -> ConsoleUi {
        ConsoleUi { show_calls: true, show_info: true, show_errors: true, search: String::new(), selected: None }
    }
}

impl ConsoleUi {
    pub fn show(&mut self, ui: &mut egui::Ui, studio: &mut Studio) {
        let (infos, errors) = (studio.console.iter().filter(|l| l.kind != LogKind::Error).count(), studio.console.iter().filter(|l| l.kind == LogKind::Error).count());
        ui.horizontal(|ui| {
            if ui.button("Clear").clicked() {
                studio.console.clear();
                self.selected = None;
            }
            ui.toggle_value(&mut self.show_calls, "Tool calls").on_hover_text("Show every tool call the editor made (the same calls an AI agent makes)");
            if ui.button("Check scene").on_hover_text("Run the `check` tool: look for mistakes that would give a blank picture").clicked() {
                if let Some(r) = studio.try_call("check", Json::Null) {
                    studio.problems = r.get("problems").and_then(|p| p.as_array()).map(|a| a.iter().map(|p| (p.get("severity").and_then(|s| s.as_str()).unwrap_or("").to_string(), format!("{} {}", p.get("entity").map(|e| e.to_string_lossy()).unwrap_or_default(), p.get("message").and_then(|m| m.as_str()).unwrap_or("")))).collect()).unwrap_or_default();
                    let n = studio.problems.len();
                    studio.log(LogKind::Info, if n == 0 { "check: no problems".to_string() } else { format!("check: {n} problem(s)") }, String::new(), 0.0);
                    for (sev, msg) in studio.problems.clone() {
                        studio.log(if sev == "error" { LogKind::Error } else { LogKind::Info }, format!("[{sev}] {msg}"), String::new(), 0.0);
                    }
                }
            }
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(180.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for (icon, count, flag) in [(Icon::Error, errors, &mut self.show_errors), (Icon::Info, infos, &mut self.show_info)] {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(46.0, 20.0), Sense::click());
                    if *flag {
                        ui.painter().rect_filled(r, CornerRadius::same(2), theme::BUTTON_ACTIVE);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, CornerRadius::same(2), theme::BUTTON);
                    }
                    icons::draw(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.min.x + 12.0, r.center().y), Vec2::splat(15.0)), icon, icon.tint());
                    ui.painter().text(egui::pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, count.to_string(), FontId::proportional(12.0), theme::TEXT);
                    if resp.clicked() {
                        *flag = !*flag;
                    }
                }
            });
        });
        let input_h = 30.0;
        let detail_h = if self.selected.is_some() { 64.0 } else { 0.0 };
        let list_h = (ui.available_height() - input_h - detail_h).max(30.0);
        let search = self.search.to_lowercase();
        let rows: Vec<usize> = studio
            .console
            .iter()
            .enumerate()
            .filter(|(_, l)| match l.kind {
                LogKind::Call => self.show_calls && self.show_info,
                LogKind::Info => self.show_info,
                LogKind::Error => self.show_errors,
            })
            .filter(|(_, l)| search.is_empty() || l.text.to_lowercase().contains(&search))
            .map(|(i, _)| i)
            .collect();
        let list_rect = egui::Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), list_h));
        ui.painter().rect_filled(list_rect, CornerRadius::ZERO, theme::FIELD);
        egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).max_height(list_h).id_salt("console-scroll").show_rows(ui, 34.0, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for k in range {
                let i = rows[k];
                let l = &studio.console[i];
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
                let sel = self.selected == Some(i);
                ui.painter().rect_filled(rect, CornerRadius::ZERO, if sel { theme::SELECT } else if k % 2 == 0 { theme::ROW_ALT } else { theme::PANEL });
                let icon = match l.kind {
                    LogKind::Error => Icon::Error,
                    _ => Icon::Info,
                };
                icons::draw(ui.painter(), egui::Rect::from_center_size(egui::pos2(rect.min.x + 14.0, rect.center().y), Vec2::splat(18.0)), icon, icon.tint());
                let color = if l.kind == LogKind::Error { theme::ERR } else { theme::TEXT };
                let first: String = l.text.lines().next().unwrap_or("").chars().take(220).collect();
                ui.painter().text(egui::pos2(rect.min.x + 30.0, rect.min.y + 11.0), Align2::LEFT_CENTER, first, FontId::proportional(12.5), color);
                let sub = if l.kind == LogKind::Call { format!("{:.1} ms   {}", l.ms, l.detail.chars().take(160).collect::<String>()) } else { l.detail.chars().take(160).collect() };
                ui.painter().text(egui::pos2(rect.min.x + 30.0, rect.min.y + 25.0), Align2::LEFT_CENTER, sub, FontId::proportional(11.0), theme::TEXT_DIM);
                if resp.clicked() {
                    self.selected = Some(i);
                }
            }
        });
        if let Some(i) = self.selected {
            if let Some(l) = studio.console.get(i) {
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), detail_h), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::ZERO, theme::FIELD);
                let text = format!("{}\n{}", l.text, l.detail);
                ui.painter().with_clip_rect(r.shrink(3.0)).text(r.min + Vec2::new(6.0, 4.0), Align2::LEFT_TOP, text, FontId::monospace(11.5), theme::TEXT);
            } else {
                self.selected = None;
            }
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(">").monospace().color(theme::BLUE));
            let r = ui.add(egui::TextEdit::singleline(&mut studio.command_line).hint_text("tool_name {\"json\": \"arguments\"}   (any tool: the same calls an AI agent makes)").desired_width(f32::INFINITY).font(FontId::monospace(12.0)));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !studio.command_line.trim().is_empty() {
                let line = std::mem::take(&mut studio.command_line);
                let (tool, rest) = line.trim().split_once(char::is_whitespace).map(|(a, b)| (a.to_string(), b.trim().to_string())).unwrap_or((line.trim().to_string(), String::new()));
                let args = if rest.is_empty() { Ok(Json::Null) } else { parse(&rest) };
                match args {
                    Ok(a) => match studio.engine.call(&tool, &a) {
                        Ok(v) => {
                            let text = v.to_string_compact().unwrap_or_default();
                            studio.log(LogKind::Call, format!("{tool} {rest}"), text.chars().take(2000).collect(), 0.0);
                            studio.log(LogKind::Info, text.chars().take(300).collect(), String::new(), 0.0);
                            studio.rows_dirty = true;
                            studio.dirty = true;
                        }
                        Err(e) => studio.log(LogKind::Error, format!("{tool}: {e}"), String::new(), 0.0),
                    },
                    Err(e) => studio.log(LogKind::Error, format!("bad JSON arguments: {e}"), String::new(), 0.0),
                }
                r.request_focus();
            }
        });
    }
}
