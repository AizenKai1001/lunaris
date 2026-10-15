//! `profile`: run some ticks and say where the time goes, by stage of the tick and by graph system, so that an agent can find out why a game is slow and
//! what to change. The numbers are wall-clock times of this machine (never part of the state), so they vary a little from run to run.
use crate::engine::{Args, Engine};
use lunaris_ecs::{EcsError, Result};
use lunaris_json::Json;

fn round(x: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (x * k).round() / k
}

impl Engine {
    pub(crate) fn profile_tool(&mut self, a: &Args) -> Result<Json> {
        let ticks = a.opt_u64("ticks")?.unwrap_or(120);
        if ticks == 0 || ticks > 3600 {
            return Err(EcsError::new("out_of_range", "`ticks` must be 1 to 3600").at("ticks").hint("60 ticks are one second of game time"));
        }
        let want_render = matches!(a.0.get("render"), Some(Json::Bool(true)));
        if self.sim.is_recording() {
            // timing a recorded game would still be exact (it measures, it does not change anything), but a profile of a long run belongs to a copy
            return Err(EcsError::new("recording_active", "stop the recording first: profiling steps the game").hint("call record_stop, or profile a loaded copy"));
        }
        self.sim.profile = Some(Default::default());
        let args = Json::obj([("ticks", Json::from(ticks as i64)), ("budget_ms", Json::from(60_000))]);
        let stepped = self.step(&Args(&args));
        let prof = self.sim.profile.take().unwrap_or_default();
        stepped?;
        if prof.ticks == 0 {
            return Err(EcsError::new("no_ticks", "no tick was timed"));
        }
        let n = prof.ticks as f64;
        let avg = prof.total_ms / n;
        let mut stages: Vec<(&String, &lunaris_sim::sim::StageTime)> = prof.stages.iter().collect();
        stages.sort_by(|x, y| y.1.total_ms.total_cmp(&x.1.total_ms).then(x.0.cmp(y.0)));
        let rows: Vec<Json> = stages
            .iter()
            .take(40)
            .map(|(name, s)| {
                Json::obj([
                    ("stage", Json::str((*name).clone())),
                    ("avg_ms", Json::from(round(s.total_ms / n, 4))),
                    ("max_ms", Json::from(round(s.max_ms, 4))),
                    // "graph systems (all)" holds the single graph systems, which are listed again under their own names
                    ("share_percent", Json::from(round(s.total_ms / prof.total_ms.max(1e-9) * 100.0, 1))),
                ])
            })
            .collect();
        let mut advice: Vec<String> = Vec::new();
        if avg > 1000.0 / 60.0 {
            advice.push(format!("a tick takes {avg:.1} ms on average: the game cannot keep up with 60 ticks a second"));
        }
        if let Some((name, s)) = stages.iter().find(|(n, _)| !n.contains("(all)")) {
            if s.total_ms / prof.total_ms.max(1e-9) > 0.4 && prof.total_ms > 0.2 {
                advice.push(format!("`{name}` is {:.0}% of the time: start there", s.total_ms / prof.total_ms * 100.0));
            }
        }
        if prof.max_ms > avg * 4.0 && prof.max_ms > 5.0 {
            advice.push(format!("the slowest tick took {:.1} ms against {avg:.2} ms on average: something spikes (a spawn burst, a path search, a save)", prof.max_ms));
        }
        let mut out = Json::obj([
            ("ticks", Json::from(prof.ticks as i64)),
            ("entities", Json::from(self.sim.world.len())),
            ("avg_tick_ms", Json::from(round(avg, 4))),
            ("max_tick_ms", Json::from(round(prof.max_ms, 4))),
            ("ticks_per_second_possible", Json::from(round(1000.0 / avg.max(1e-6), 0))),
            ("stages", Json::Arr(rows)),
        ]);
        if want_render {
            let t = std::time::Instant::now();
            let r = self.call("render", &Json::obj([("width", Json::from(640)), ("height", Json::from(360)), ("image", Json::Bool(false))]));
            match r {
                Ok(_) => out.set("render_ms", Json::from(round(t.elapsed().as_secs_f64() * 1e3, 2))),
                Err(e) => out.set("render_error", Json::str(e.to_string())),
            }
        }
        if !advice.is_empty() {
            out.set("advice", Json::arr(advice.into_iter().map(Json::str)));
        }
        Ok(out)
    }
}
