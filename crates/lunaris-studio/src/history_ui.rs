//! The History panel: the edits that undo and redo hold, as a list to jump through (Unreal's undo history and Photoshop's History): the oldest edit at the top, the newest above the line that says where the scene is now, the
//! edits that were undone dimmed below it. Clicking an edit takes the scene to the state just after it by calling `undo` or `redo` as many times as it needs: the panel only ever calls the same tools an agent does.
use crate::state::Studio;
use crate::theme;
use eframe::egui::{self, CornerRadius, FontId, RichText, Sense, Vec2};
use lunaris_json::Json;
use std::time::{Duration, Instant};

/// One row of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub label: String,
    /// How many `undo` calls (negative: `redo` calls) take the scene to the state just after this edit; 0 is where it is now.
    pub steps: i64,
}

/// The rows of the list, oldest first, from the two stacks as `history` gives them (newest first): the start of the scene, the edits that are done, then the edits that were undone, the next redo first.
pub fn rows_of(history: &Json) -> (Vec<Row>, usize) {
    let labels = |key: &str| -> Vec<String> { history.get(key).and_then(|a| a.as_array()).map(|a| a.iter().map(|r| r.get("label").and_then(|l| l.as_str()).unwrap_or("?").to_string()).collect()).unwrap_or_default() };
    let (undo, redo) = (labels("undo"), labels("redo"));
    let mut rows = vec![Row { label: "Start".to_string(), steps: undo.len() as i64 }];
    for (i, l) in undo.iter().enumerate().rev() {
        rows.push(Row { label: l.clone(), steps: i as i64 });
    }
    let now = rows.len() - 1;
    for (j, l) in redo.iter().enumerate() {
        rows.push(Row { label: l.clone(), steps: -(j as i64 + 1) });
    }
    (rows, now)
}

#[derive(Default)]
pub struct HistoryUi {
    loaded_at: Option<Instant>,
    rows: Vec<Row>,
    now: usize,
}

impl HistoryUi {
    pub fn show(&mut self, ui: &mut egui::Ui, studio: &mut Studio) {
        if self.loaded_at.is_none_or(|t| t.elapsed() > Duration::from_millis(250)) {
            self.loaded_at = Some(Instant::now());
            if let Ok(h) = studio.engine.call("history", &Json::Null) {
                (self.rows, self.now) = rows_of(&h);
            }
        }
        ui.horizontal(|ui| {
            let done = self.now;
            ui.label(RichText::new(format!("{done} {}, {} undone", if done == 1 { "edit" } else { "edits" }, self.rows.len().saturating_sub(self.now + 1))).small().color(theme::TEXT_DIM));
        });
        let mut jump: Option<i64> = None;
        egui::ScrollArea::vertical().id_salt("history-scroll").auto_shrink([false, false]).stick_to_bottom(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (k, row) in self.rows.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::click());
                let current = k == self.now;
                if current {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, theme::SELECT);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, theme::HOVER_ROW);
                }
                let color = if k > self.now { theme::TEXT_DIM } else { theme::TEXT };
                ui.painter().text(rect.min + Vec2::new(10.0, 10.0), egui::Align2::LEFT_CENTER, &row.label, FontId::proportional(12.5), color);
                if current {
                    ui.painter().text(egui::pos2(rect.max.x - 10.0, rect.center().y), egui::Align2::RIGHT_CENTER, "now", FontId::proportional(11.0), theme::TEXT_DIM);
                }
                if resp.clicked() && !current {
                    jump = Some(row.steps);
                }
            }
        });
        if let Some(steps) = jump {
            studio.undo_by(steps);
            self.loaded_at = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunaris_json::parse;

    #[test]
    fn the_rows_run_from_the_start_to_the_last_redo_with_the_steps_to_each() {
        let h = parse(r#"{"undo":[{"label":"Edit B"},{"label":"Edit A"}],"redo":[{"label":"Edit C"},{"label":"Edit D"}]}"#).unwrap();
        let (rows, now) = rows_of(&h);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["Start", "Edit A", "Edit B", "Edit C", "Edit D"]);
        assert_eq!(now, 2, "the scene is after the newest edit that is done");
        let steps: Vec<i64> = rows.iter().map(|r| r.steps).collect();
        assert_eq!(steps, [2, 1, 0, -1, -2], "undo twice for the start, redo twice for the last");
    }

    #[test]
    fn an_empty_history_is_the_start_alone() {
        let (rows, now) = rows_of(&parse(r#"{"undo":[],"redo":[]}"#).unwrap());
        assert_eq!((rows.len(), now, rows[0].steps), (1, 0, 0));
        let (rows, now) = rows_of(&Json::Null);
        assert_eq!((rows.len(), now), (1, 0));
    }
}
