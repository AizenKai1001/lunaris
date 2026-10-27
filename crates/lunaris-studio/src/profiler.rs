//! The Profiler tab: where the time of each tick goes while the game plays, as Unreal's `stat` views and Insights show it for the game thread. The simulation times its own
//! stages (`Sim::profile`, the same numbers the `profile` tool reports); this panel switches that on only while the tab is open, takes the numbers every second of game time,
//! and draws them: the average and slowest tick against the time a tick may take, one bar per stage, and the history of the last two minutes.
use crate::state::{Mode, Studio};
use crate::theme;
use eframe::egui::{self, Align2, CornerRadius, FontId, RichText, Sense, Shape, Stroke, Vec2};
use lunaris_sim::sim::StepProfile;
use lunaris_sim::Sim;

/// Ticks in one window of measurement (one second of game time at 60 ticks a second).
pub const WINDOW_TICKS: u64 = 60;
/// How many windows of history the graph keeps.
const HISTORY: usize = 120;

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub avg_ms: f64,
    pub max_ms: f64,
    /// Share of the tick's time, in percent.
    pub share: f64,
}

/// One window of measurement, as the panel shows it.
#[derive(Clone, Debug, Default)]
pub struct Window {
    pub ticks: u64,
    pub avg_ms: f64,
    pub max_ms: f64,
    pub entities: usize,
    /// Slowest stage first.
    pub rows: Vec<Row>,
}

impl Window {
    pub fn from_profile(p: &StepProfile, entities: usize) -> Window {
        let n = p.ticks.max(1) as f64;
        let mut rows: Vec<Row> =
            p.stages.iter().map(|(name, s)| Row { name: name.clone(), avg_ms: s.total_ms / n, max_ms: s.max_ms, share: s.total_ms / p.total_ms.max(1e-9) * 100.0 }).collect();
        rows.sort_by(|a, b| b.avg_ms.total_cmp(&a.avg_ms).then(a.name.cmp(&b.name)));
        Window { ticks: p.ticks, avg_ms: p.total_ms / n, max_ms: p.max_ms, entities, rows }
    }

    /// What a person should look at first (the same rules as the `profile` tool's advice), given the time one tick may take.
    pub fn advice(&self, budget_ms: f64) -> Vec<String> {
        let mut out = Vec::new();
        if self.avg_ms > budget_ms {
            out.push(format!("a tick takes {:.1} ms on average: the game cannot keep up with {:.0} ticks a second", self.avg_ms, 1000.0 / budget_ms));
        }
        if let Some(r) = self.rows.iter().find(|r| !r.name.contains("(all)")) {
            if r.share > 40.0 && self.avg_ms > 0.2 {
                out.push(format!("`{}` is {:.0}% of the time: start there", r.name, r.share));
            }
        }
        if self.max_ms > self.avg_ms * 4.0 && self.max_ms > 5.0 {
            out.push(format!("the slowest tick took {:.1} ms against {:.2} ms on average: something spikes (a spawn burst, a path search, a save)", self.max_ms, self.avg_ms));
        }
        out
    }

    /// How many ticks like these fit in the time one tick may take (the headroom; below 1 the game falls behind).
    pub fn headroom(&self, budget_ms: f64) -> f64 {
        budget_ms / self.avg_ms.max(1e-6)
    }
}

#[derive(Default)]
pub struct ProfilerUi {
    pub last: Option<Window>,
    /// Average tick time of each past window, oldest first.
    pub history: Vec<f32>,
    pub frozen: bool,
    filter: String,
}

impl ProfilerUi {
    /// Called after the editor stepped the game. While the tab is open (`on`) the simulation is asked to time its stages and every `WINDOW_TICKS` ticks the numbers are
    /// taken; otherwise the timing is switched off, so a closed tab costs nothing.
    pub fn collect(&mut self, sim: &mut Sim, on: bool) {
        if !on {
            sim.profile = None;
            return;
        }
        match &sim.profile {
            None => sim.profile = Some(StepProfile::default()),
            Some(p) if p.ticks >= WINDOW_TICKS => {
                let p = sim.profile.replace(StepProfile::default()).expect("a profile");
                if !self.frozen {
                    self.history.push(p.total_ms as f32 / p.ticks.max(1) as f32);
                    if self.history.len() > HISTORY {
                        self.history.remove(0);
                    }
                    self.last = Some(Window::from_profile(&p, sim.world.len()));
                }
            }
            Some(_) => {}
        }
    }

    pub fn clear(&mut self) {
        self.last = None;
        self.history.clear();
    }

    pub fn show(&mut self, ui: &mut egui::Ui, studio: &mut Studio, gpu: &crate::viewport::GpuStats) {
        let budget_ms = studio.engine.sim.dt as f64 * 1000.0;
        ui.horizontal(|ui| {
            if ui.button("Clear").clicked() {
                self.clear();
            }
            ui.toggle_value(&mut self.frozen, "Freeze").on_hover_text("Stop taking new measurements, to read the ones on screen");
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Filter stages").desired_width(160.0));
            match studio.mode {
                Mode::Edit => ui.label(RichText::new("Press Play: the game is timed while it runs").small().color(theme::TEXT_DIM)),
                Mode::Paused => ui.label(RichText::new("Paused").small().color(theme::TEXT_DIM)),
                Mode::Play => ui.label(RichText::new("Measuring").small().color(theme::OK)),
            };
        });
        let Some(w) = self.last.clone() else {
            ui.label(RichText::new("No measurement yet: a window is one second of game time.").color(theme::TEXT_DIM));
            return;
        };
        // the headline: average and slowest tick, headroom
        ui.horizontal(|ui| {
            let head = |ui: &mut egui::Ui, label: &str, value: String, color| {
                ui.label(RichText::new(label).small().color(theme::TEXT_DIM));
                ui.label(RichText::new(value).strong().color(color));
                ui.add_space(14.0);
            };
            let load = w.avg_ms / budget_ms;
            head(ui, "tick", format!("{:.3} ms", w.avg_ms), if load > 1.0 { theme::ERR } else if load > 0.6 { theme::WARN } else { theme::OK });
            head(ui, "slowest", format!("{:.2} ms", w.max_ms), if w.max_ms > budget_ms { theme::WARN } else { theme::TEXT });
            head(ui, "of a 60 Hz tick", format!("{:.1}%", load * 100.0), theme::TEXT);
            head(ui, "headroom", format!("{:.0}x", w.headroom(budget_ms)), theme::TEXT);
            head(ui, "entities", w.entities.to_string(), theme::TEXT);
        });
        // history
        let graph_h = 46.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), graph_h), Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::ZERO, theme::FIELD);
        if self.history.len() >= 2 {
            let top = (self.history.iter().cloned().fold(0.0f32, f32::max)).max(budget_ms as f32 * 1.2);
            let y_of = |ms: f32| rect.max.y - 2.0 - (ms / top).clamp(0.0, 1.0) * (graph_h - 4.0);
            let by = y_of(budget_ms as f32);
            ui.painter().line_segment([egui::pos2(rect.min.x, by), egui::pos2(rect.max.x, by)], Stroke::new(1.0, theme::WARN.gamma_multiply(0.6)));
            let n = self.history.len();
            let pts: Vec<egui::Pos2> = self.history.iter().enumerate().map(|(i, &v)| egui::pos2(rect.max.x - (n - 1 - i) as f32 * ((rect.width() - 4.0) / HISTORY as f32) - 2.0, y_of(v))).collect();
            ui.painter().add(Shape::line(pts, Stroke::new(1.5, theme::BLUE)));
            ui.painter().text(rect.min + Vec2::new(4.0, 2.0), Align2::LEFT_TOP, format!("{top:.1} ms"), FontId::proportional(10.0), theme::TEXT_DIM);
            ui.painter().text(egui::pos2(rect.max.x - 4.0, by - 1.0), Align2::RIGHT_BOTTOM, "60 Hz", FontId::proportional(10.0), theme::WARN);
        }
        for line in w.advice(budget_ms) {
            ui.label(RichText::new(line).small().color(theme::WARN));
        }
        // the card: what the scene view's last picture cost, pass by pass (the card's own clock; the scene view has to be drawn by a GPU quality, not the CPU reference)
        egui::CollapsingHeader::new(RichText::new(match gpu.total_ms() {
            Some(ms) => format!("Card: {ms:.2} ms a picture on {}   {} draws, {} triangles", gpu.adapter, gpu.draws, gpu.triangles),
            None if gpu.draws > 0 => format!("Card: {} draws, {} triangles (no pass times: this card has no timestamp queries)", gpu.draws, gpu.triangles),
            None => "Card: no times (the scene view is drawn by the CPU reference renderer: pick a GPU quality in the bar above it)".to_string(),
        }))
        .id_salt("profiler-card")
        .default_open(gpu.total_ms().is_some())
        .show(ui, |ui| {
            let rows = gpu.rows();
            let widest = rows.iter().filter(|r| !r.part).map(|r| r.ms).fold(1e-6f32, f32::max);
            for r in &rows {
                ui.horizontal(|ui| {
                    ui.add_space(if r.part { 24.0 } else { 6.0 });
                    let name_w = if r.part { 200.0 } else { 218.0 };
                    ui.add_sized([name_w, 16.0], egui::Label::new(RichText::new(&r.name).small().color(if r.part { theme::TEXT_DIM } else { theme::TEXT })));
                    let (bar, _) = ui.allocate_exact_size(Vec2::new(160.0, 12.0), Sense::hover());
                    let w = (bar.width() * (r.ms / widest).clamp(0.0, 1.0)).max(1.0);
                    ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, Vec2::new(w, 12.0)), CornerRadius::ZERO, if r.part { theme::TEXT_DIM } else { theme::BLUE }.gamma_multiply(0.85));
                    ui.label(RichText::new(format!("{:.3} ms", r.ms)).small().monospace());
                });
            }
            if !gpu.cpu.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new("The processor, by phase").small().color(theme::TEXT_DIM));
                for (name, ms) in &gpu.cpu {
                    ui.label(RichText::new(format!("   {name:<28} {ms:.3} ms")).small().monospace());
                }
            }
        });
        // one bar per stage
        let filter = self.filter.to_lowercase();
        let rows: Vec<&Row> = w.rows.iter().filter(|r| filter.is_empty() || r.name.to_lowercase().contains(&filter)).collect();
        let widest = rows.iter().map(|r| r.avg_ms).fold(1e-9f64, f64::max);
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("profiler-rows").show_rows(ui, 18.0, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for k in range {
                let r = rows[k];
                let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
                ui.painter().rect_filled(rect, CornerRadius::ZERO, if k % 2 == 0 { theme::ROW_ALT } else { theme::PANEL });
                let group = r.name.contains("(all)");
                let name_w = 260.0f32.min(rect.width() * 0.4);
                let bar_x0 = rect.min.x + name_w + 6.0;
                let bar_w = (rect.width() - name_w - 180.0).max(40.0);
                let full = (r.avg_ms / widest) as f32;
                let color = if group { theme::TEXT_DIM } else if r.share > 40.0 { theme::WARN } else { theme::BLUE };
                ui.painter().rect_filled(egui::Rect::from_min_size(egui::pos2(bar_x0, rect.min.y + 3.0), Vec2::new((bar_w * full).max(1.0), 12.0)), CornerRadius::ZERO, color.gamma_multiply(0.85));
                ui.painter().with_clip_rect(egui::Rect::from_min_size(rect.min, Vec2::new(name_w, 18.0))).text(
                    egui::pos2(rect.min.x + 6.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &r.name,
                    FontId::proportional(12.0),
                    if group { theme::TEXT_DIM } else { theme::TEXT },
                );
                ui.painter().text(egui::pos2(bar_x0 + bar_w + 8.0, rect.center().y), Align2::LEFT_CENTER, format!("{:.4} ms   {:.1}%   max {:.3}", r.avg_ms, r.share, r.max_ms), FontId::monospace(11.0), theme::TEXT);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunaris_sim::sim::StageTime;

    fn profile(stages: &[(&str, f64, f64)], ticks: u64, total_ms: f64, max_ms: f64) -> StepProfile {
        let mut p = StepProfile { ticks, total_ms, max_ms, ..Default::default() };
        for (name, total, max) in stages {
            p.stages.insert(name.to_string(), StageTime { total_ms: *total, max_ms: *max, calls: ticks });
        }
        p
    }

    #[test]
    fn a_window_ranks_the_stages_by_time_and_shares_add_up() {
        let p = profile(&[("physics", 30.0, 1.0), ("movement", 60.0, 2.0), ("ui", 10.0, 0.2)], 60, 100.0, 3.0);
        let w = Window::from_profile(&p, 500);
        assert_eq!(w.rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["movement", "physics", "ui"]);
        assert!((w.avg_ms - 100.0 / 60.0).abs() < 1e-9);
        assert!((w.rows[0].avg_ms - 1.0).abs() < 1e-9);
        assert!((w.rows.iter().map(|r| r.share).sum::<f64>() - 100.0).abs() < 1e-6);
        assert_eq!(w.entities, 500);
        // 1.67 ms of a 16.67 ms tick: ten times the headroom
        assert!((w.headroom(1000.0 / 60.0) - 10.0).abs() < 1e-6);
    }

    #[test]
    fn advice_says_what_the_profile_tool_says() {
        let budget = 1000.0 / 60.0;
        // too slow, one stage most of it, and a spike
        let slow = Window::from_profile(&profile(&[("movement", 1500.0, 150.0), ("ui", 100.0, 1.0)], 60, 1600.0, 150.0), 10);
        let text = slow.advice(budget).join(" | ");
        assert!(text.contains("cannot keep up"), "{text}");
        assert!(text.contains("`movement` is 94%"), "{text}");
        assert!(text.contains("spikes"), "{text}");
        // a quiet game has nothing to say
        let quiet = Window::from_profile(&profile(&[("movement", 6.0, 0.2), ("ui", 4.0, 0.2)], 60, 10.0, 0.3), 10);
        assert!(quiet.advice(budget).is_empty());
        // a group row ("(all)") is never named as the place to start
        let group = Window::from_profile(&profile(&[("graph systems (all)", 90.0, 2.0), ("a", 5.0, 0.1)], 60, 100.0, 2.0), 10);
        assert!(group.advice(budget).iter().all(|l| !l.contains("(all)")));
    }

    #[test]
    fn timing_is_off_unless_the_tab_is_open_and_windows_close_every_second_of_game_time() {
        let mut sim = Sim::new(1);
        let mut ui = ProfilerUi::default();
        ui.collect(&mut sim, false);
        assert!(sim.profile.is_none());
        ui.collect(&mut sim, true);
        assert!(sim.profile.is_some(), "the tab asks the simulation to time itself");
        for _ in 0..WINDOW_TICKS - 1 {
            sim.step();
        }
        ui.collect(&mut sim, true);
        assert!(ui.last.is_none(), "a window is not closed before {WINDOW_TICKS} ticks");
        sim.step();
        ui.collect(&mut sim, true);
        let w = ui.last.clone().expect("the window closed");
        assert_eq!(w.ticks, WINDOW_TICKS);
        assert!(!w.rows.is_empty(), "the stages of the tick were timed");
        assert_eq!(ui.history.len(), 1);
        // frozen: new windows are not taken
        ui.frozen = true;
        for _ in 0..WINDOW_TICKS {
            sim.step();
        }
        ui.collect(&mut sim, true);
        assert_eq!(ui.history.len(), 1);
        ui.collect(&mut sim, false);
        assert!(sim.profile.is_none());
    }
}
