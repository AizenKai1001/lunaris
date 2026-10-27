//! The Problems tab: what the `check` tool finds in the scene, as rows to click (VS Code's Problems panel, Unreal's Message Log): the severity, the entity it is about, the message and the hint. A click selects the entity,
//! a double click puts the scene view on it. Run by the button, or by itself while **Auto** is on (when the scene has changed, at most every second and a half).
use crate::icons::{self, Icon};
use crate::state::Studio;
use crate::theme;
use eframe::egui::{self, Align2, CornerRadius, FontId, RichText, Sense, Vec2};
use lunaris_json::Json;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    /// `error`, `warning` or `info`.
    pub severity: String,
    pub message: String,
    pub hint: String,
    pub entity: Option<u64>,
}

/// How much a severity matters when the rows are ordered: errors, then warnings, then the rest.
fn rank(severity: &str) -> u8 {
    match severity {
        "error" => 0,
        "warning" => 1,
        _ => 2,
    }
}

/// The problems in an answer of `check`: errors first, then warnings, then the rest, each group in the order `check` found them.
pub fn problems_of(answer: &Json) -> Vec<Problem> {
    let mut out: Vec<Problem> = answer
        .get("problems")
        .and_then(|p| p.as_array())
        .unwrap_or(&[])
        .iter()
        .map(|p| Problem {
            severity: p.get("severity").and_then(|s| s.as_str()).unwrap_or("info").to_string(),
            message: p.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string(),
            hint: p.get("hint").and_then(|h| h.as_str()).unwrap_or("").to_string(),
            entity: p.get("entity").and_then(|e| e.as_u64()),
        })
        .collect();
    out.sort_by_key(|p| rank(&p.severity));
    out
}

/// What a click on a row asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub uid: u64,
    /// A double click: put the scene view on it.
    pub focus: bool,
}

#[derive(Default)]
pub struct ProblemsUi {
    pub list: Vec<Problem>,
    /// Whether `check` has been run at all (an empty list means "no problems" only after it has).
    pub checked: bool,
    pub auto: bool,
    last_rev: u64,
    last_run: Option<Instant>,
}

impl ProblemsUi {
    /// Run `check` and keep what it says.
    pub fn run(&mut self, studio: &mut Studio) {
        if let Ok(answer) = studio.engine.call("check", &Json::Null) {
            self.list = problems_of(&answer);
            self.checked = true;
        }
        self.last_rev = studio.live_revision;
        self.last_run = Some(Instant::now());
    }

    /// With Auto on, check again when the scene has changed since the last time (at most every second and a half).
    pub fn keep_up(&mut self, studio: &mut Studio) {
        if self.auto && self.last_rev != studio.live_revision && self.last_run.is_none_or(|t| t.elapsed() > Duration::from_millis(1500)) {
            self.run(studio);
        }
    }

    pub fn counts(&self) -> (usize, usize) {
        (self.list.iter().filter(|p| p.severity == "error").count(), self.list.iter().filter(|p| p.severity == "warning").count())
    }

    pub fn show(&mut self, ui: &mut egui::Ui, studio: &mut Studio) -> Option<Pick> {
        self.keep_up(studio);
        ui.horizontal(|ui| {
            if ui.button("Check scene").on_hover_text("Run the `check` tool: mistakes that give a blank picture, a game that does nothing or a server that is slow").clicked() {
                self.run(studio);
            }
            ui.toggle_value(&mut self.auto, "Auto").on_hover_text("Check again by itself when the scene changes");
            let (errors, warnings) = self.counts();
            let text = if !self.checked {
                "Press Check scene".to_string()
            } else if self.list.is_empty() {
                "No problems".to_string()
            } else {
                format!("{errors} errors, {warnings} warnings, {} notes", self.list.len() - errors - warnings)
            };
            ui.label(RichText::new(text).small().color(if errors > 0 { theme::ERR } else if warnings > 0 { theme::WARN } else { theme::TEXT_DIM }));
        });
        if self.list.is_empty() {
            return None;
        }
        let mut pick: Option<Pick> = None;
        let list = self.list.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("problems-rows").show_rows(ui, 20.0, list.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for k in range {
                let p = &list[k];
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::click());
                let selected = p.entity.is_some_and(|u| studio.is_selected(u));
                ui.painter().rect_filled(rect, CornerRadius::ZERO, if selected { theme::SELECT } else if k % 2 == 0 { theme::ROW_ALT } else { theme::PANEL });
                let mid = rect.center().y;
                let icon = match p.severity.as_str() {
                    "error" => Icon::Error,
                    "warning" => Icon::Warn,
                    _ => Icon::Info,
                };
                icons::draw(ui.painter(), egui::Rect::from_center_size(egui::pos2(rect.min.x + 14.0, mid), Vec2::splat(15.0)), icon, icon.tint());
                let who = match p.entity {
                    Some(u) => studio.all.iter().find(|r| r.uid == u).map_or(format!("#{u}"), |r| format!("{}  #{u}", r.name)),
                    None => "scene".to_string(),
                };
                let name_w = (rect.width() * 0.2).clamp(90.0, 220.0);
                ui.painter().with_clip_rect(egui::Rect::from_min_size(rect.min + Vec2::new(26.0, 0.0), Vec2::new(name_w, 20.0))).text(egui::pos2(rect.min.x + 30.0, mid), Align2::LEFT_CENTER, who, FontId::proportional(12.0), theme::TEXT_DIM);
                let text_x = rect.min.x + 34.0 + name_w;
                let clip = egui::Rect::from_min_max(egui::pos2(text_x, rect.min.y), rect.max);
                let msg = ui.painter().with_clip_rect(clip).text(egui::pos2(text_x, mid), Align2::LEFT_CENTER, &p.message, FontId::proportional(12.5), theme::TEXT);
                if !p.hint.is_empty() {
                    ui.painter().with_clip_rect(clip).text(egui::pos2(msg.max.x + 12.0, mid), Align2::LEFT_CENTER, &p.hint, FontId::proportional(12.0), theme::TEXT_DIM);
                }
                if let Some(u) = p.entity {
                    if resp.clicked() || resp.double_clicked() {
                        studio.select(Some(u));
                        pick = Some(Pick { uid: u, focus: resp.double_clicked() });
                    }
                }
            }
        });
        pick
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunaris_json::parse;

    #[test]
    fn the_problems_of_an_answer_are_ordered_errors_first_and_keep_their_entity_and_hint() {
        let a = parse(
            r#"{"problems":[
                {"severity":"warning","message":"w1","entity":7},
                {"severity":"error","message":"e1","entity":3,"hint":"do this"},
                {"severity":"info","message":"i1"},
                {"severity":"error","message":"e2"},
                {"severity":"warning","message":"w2","entity":8}]}"#,
        )
        .unwrap();
        let p = problems_of(&a);
        assert_eq!(p.iter().map(|x| x.message.as_str()).collect::<Vec<_>>(), ["e1", "e2", "w1", "w2", "i1"], "errors, warnings, then the rest, each in the order found");
        assert_eq!((p[0].entity, p[0].hint.as_str()), (Some(3), "do this"));
        assert_eq!(p[1].entity, None);
        let ui = ProblemsUi { list: p, checked: true, ..Default::default() };
        assert_eq!(ui.counts(), (2, 2));
        assert!(problems_of(&parse("{}").unwrap()).is_empty());
    }
}
