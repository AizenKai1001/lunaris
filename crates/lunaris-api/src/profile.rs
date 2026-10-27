//! `profile`: run some ticks and say where the time goes, by stage of the tick and by graph system, so that an agent can find out why a game is slow and
//! what to change. The numbers are wall-clock times of this machine (never part of the state), so they vary a little from run to run.
use crate::engine::{Args, Engine};
use lunaris_ecs::{EcsError, Result};
use lunaris_json::Json;

fn round(x: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (x * k).round() / k
}

/// What to try when a stage has a spike far above its average: the fix the engine has for the stages that are known to make one.
fn spike_advice(stage: &str) -> Option<&'static str> {
    match stage {
        "navigation" => Some("a path search spikes (a target that cannot be reached is looked for everywhere up to the limit): put a `NavLimits` entity in the world with `max_searched` and `max_distance`, and set the resource `nav.cells_per_step` so that a search goes on over several steps (docs/NAVIGATION.md)"),
        "gameplay procgrid" => Some("a procedural grid makes many cells in one step: lower `ProcGrid.per_tick`, or set the resource `procgrid.every` so that it looks at the cells every few steps (docs/GAMEPLAY.md)"),
        "gameplay streaming" => Some("streaming loads many zones in one step: set the resource `stream.loads_per_tick` (docs/GAMEPLAY.md)"),
        _ => None,
    }
}

/// What to try when a stage is a steady cost (not a spike): the resource that sets how often the toolkit updates, and what to say. Said only while that resource is not set.
fn cost_advice(stage: &str) -> Option<(&'static str, &'static str)> {
    match stage {
        "gameplay perception::update" => Some(("gameplay.every.perception", "the AI looks at the world every step: set the resource `gameplay.every.perception` to 2 so that it looks every second step with the time of both (docs/AI.md)")),
        "gameplay flyagent::update" => Some(("gameplay.every.flyagent", "the flying machines decide every step: set the resource `gameplay.every.flyagent` to 2 so that they decide every second step with the time of both (docs/AI.md)")),
        _ => None,
    }
}

/// A stage that costs at least this many milliseconds a step on average gets the advice of `cost_advice`.
const COST_ADVICE_MS: f64 = 0.4;

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
            // every stage that ran (about sixty with the gameplay blocks): cutting the list at 40 dropped the quiet stages by the luck of the timings, and the quiet ones are what a person is looking for when something is not running
            .take(120)
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
        // a stage whose slowest step is far above its average and is a stage with a known remedy
        for (name, s) in &stages {
            let per_step = s.total_ms / n;
            if s.max_ms > 8.0 && s.max_ms > 6.0 * per_step.max(0.05) {
                if let Some(text) = spike_advice(name) {
                    advice.push(format!("`{name}` took {:.1} ms in its slowest step against {per_step:.2} ms on average: {text}", s.max_ms));
                }
            }
        }
        // a stage that is a steady cost and has a knob for how often it runs
        for (name, s) in &stages {
            let per_step = s.total_ms / n;
            if per_step >= COST_ADVICE_MS {
                if let Some((resource, text)) = cost_advice(name) {
                    if self.sim.world.resource(resource).is_none() {
                        advice.push(format!("`{name}` costs {per_step:.2} ms a step: {text}"));
                    }
                }
            }
        }
        let mut out = Json::obj([
            ("ticks", Json::from(prof.ticks as i64)),
            ("entities", Json::from(self.sim.world.len())),
            ("avg_tick_ms", Json::from(round(avg, 4))),
            ("max_tick_ms", Json::from(round(prof.max_ms, 4))),
            ("ticks_per_second_possible", Json::from(round(1000.0 / avg.max(1e-6), 0))),
            ("stages", Json::Arr(rows)),
        ]);
        // how many steps took longer than 10, 15, 20, 30, 50 and 100 ms: the shape of the tail, which the average and the slowest hide
        out.set("steps_over_ms", Json::obj(lunaris_sim::sim::STEP_LIMITS_MS.iter().zip(prof.over.iter()).map(|(l, n)| (format!("{l}"), Json::from(*n as i64)))));
        // the slowest steps that took longer than 30 ms (`LUNARIS_SLOW_STEP_MS`), with their biggest stages
        if !prof.slow.is_empty() {
            let rows: Vec<Json> = prof
                .slow
                .iter()
                .take(5)
                .map(|(ms, st)| Json::obj([("ms", Json::from(round(*ms, 2))), ("stages", Json::arr(st.iter().map(|(n, t)| Json::obj([("stage", Json::str(n.clone())), ("ms", Json::from(round(*t, 3)))]))))]))
                .collect();
            out.set("slowest_steps", Json::Arr(rows));
        }
        if want_render {
            // the pictures of a game are the size of its window: `width` and `height` (default 640 x 360); the first picture builds the meshes and textures, so with `pictures` above 1 the
            // first is reported on its own and `render_ms` is the median of the rest
            let (w, h) = (a.opt_u64("width")?.unwrap_or(640).clamp(16, 4096), a.opt_u64("height")?.unwrap_or(360).clamp(16, 4096));
            let pictures = a.opt_u64("pictures")?.unwrap_or(1).clamp(1, 100) as usize;
            let mut times: Vec<f64> = Vec::new();
            for _ in 0..pictures {
                let t = std::time::Instant::now();
                match self.call("render", &Json::obj([("width", Json::from(w as i64)), ("height", Json::from(h as i64)), ("image", Json::Bool(false))])) {
                    Ok(_) => times.push(t.elapsed().as_secs_f64() * 1e3),
                    Err(e) => {
                        out.set("render_error", Json::str(e.to_string()));
                        break;
                    }
                }
            }
            if let Some(first) = times.first().copied() {
                if times.len() > 1 {
                    let mut rest: Vec<f64> = times[1..].to_vec();
                    rest.sort_by(|x, y| x.total_cmp(y));
                    out.set("render_first_ms", Json::from(round(first, 2)));
                    out.set("render_ms", Json::from(round(rest[rest.len() / 2], 2)));
                } else {
                    out.set("render_ms", Json::from(round(first, 2)));
                }
                out.set("render_size", Json::arr([Json::from(w as i64), Json::from(h as i64)]));
            }
        }
        if !advice.is_empty() {
            out.set("advice", Json::arr(advice.into_iter().map(Json::str)));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{cost_advice, spike_advice};

    #[test]
    fn stages_with_a_known_remedy_get_advice_and_the_others_none() {
        for stage in ["navigation", "gameplay procgrid", "gameplay streaming"] {
            assert!(spike_advice(stage).is_some_and(|t| t.contains("docs/")), "{stage}");
        }
        assert!(spike_advice("system physics").is_none() && spike_advice("characters").is_none());
    }

    #[test]
    fn the_ai_toolkits_name_the_resource_that_sets_how_often_they_run_and_other_stages_have_none() {
        for (stage, resource) in [("gameplay perception::update", "gameplay.every.perception"), ("gameplay flyagent::update", "gameplay.every.flyagent")] {
            let (r, text) = cost_advice(stage).unwrap();
            assert_eq!(r, resource);
            assert!(text.contains(resource) && text.contains("docs/AI.md"), "{text}");
        }
        assert!(cost_advice("gameplay procgrid").is_none() && cost_advice("system physics").is_none());
    }
}
