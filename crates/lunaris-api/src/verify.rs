//! `verify`: a playtest written as data. A spec names a scene and lists steps (press keys, advance time, call any tool) and
//! expectations about the result (a resource, a field of an entity, how many entities have some components, events, the state hash,
//! the `check` report). It runs on a scratch engine, so the live world is never touched, and returns a report an agent can read:
//! which expectation failed, what the value was, and what was wanted. The same runner is `lunaris verify spec.json` for people
//! and CI. Because the simulation is deterministic, a spec that passes once passes every time.
//!
//! ```json
//! { "name": "holding d moves the ball", "scene": "examples/orb_dash.lunaris.json", "seed": 1, "replay_check": true,
//!   "steps": [
//!     { "expect": { "count": { "with": ["Orb"] }, "eq": 12 } },
//!     { "input": { "d": true } }, { "step": 60 },
//!     { "expect": { "resource": "player.x", "gt": 0.5 } },
//!     { "until": { "resource": "time_left", "le": 40, "max_ticks": 600 } } ] }
//! ```
use crate::engine::Engine;
use lunaris_ecs::{EcsError, Result};
use lunaris_json::Json;
use std::path::Path;

const MAX_STEPS: usize = 5_000;
const MAX_TOTAL_TICKS: u64 = 5_000_000;
const TARGET_KEYS: &[&str] = &["resource", "entity", "name", "field", "exists", "count", "tick", "hash", "check", "events"];
const COMPARATORS: &[&str] = &["eq", "ne", "gt", "ge", "lt", "le", "approx", "between"];

struct Runner {
    eng: Engine,
    results: Vec<Json>,
    passed: usize,
    failed: usize,
    ticks: u64,
}

fn msg(e: EcsError) -> String {
    match &e.hint {
        Some(h) => format!("{}: {} (hint: {h})", e.code, e.message),
        None => format!("{}: {}", e.code, e.message),
    }
}

fn shown(v: &Json) -> String {
    match v {
        Json::Float(f) => format!("{}", (*f * 1e6).round() / 1e6),
        other => other.to_string_lossy(),
    }
}

fn call(eng: &mut Engine, tool: &str, args: Json) -> std::result::Result<Json, String> {
    eng.call(tool, &args).map_err(msg)
}

/// The value a target names, or why it cannot be read.
fn value_of(eng: &mut Engine, t: &Json) -> std::result::Result<Json, String> {
        let get = |k: &str| t.get(k);
    if let Some(name) = get("resource").and_then(|v| v.as_str()) {
        let r = call(eng, "resources", Json::Obj(Vec::new()))?;
        return Ok(r.get("resources").and_then(|m| m.get(name)).cloned().unwrap_or(Json::Float(0.0)));
    }
    if get("tick").is_some() {
        return Ok(Json::from(eng.sim.tick as i64));
    }
    if get("hash").is_some() {
        let r = call(eng, "hash", Json::Obj(Vec::new()))?;
        return Ok(r.get("hash").cloned().unwrap_or(Json::Null));
    }
    if let Some(which) = get("check").and_then(|v| v.as_str()) {
        let key = match which {
            "errors" => "errors",
            "warnings" => "warnings",
            _ => return Err("`check` is \"errors\" or \"warnings\" (the number of each in the check report)".into()),
        };
        let r = call(eng, "check", Json::Obj(Vec::new()))?;
        return Ok(r.get(key).cloned().unwrap_or(Json::Null));
    }
    if let Some(q) = get("count") {
        let mut args = Json::Obj(Vec::new());
        for k in ["with", "without"] {
            if let Some(v) = q.get(k) {
                args.set(k, v.clone());
            }
        }
        args.set("limit", Json::from(1));
        let r = call(eng, "query", args)?;
        return Ok(r.get("total").cloned().unwrap_or(Json::from(0)));
    }
    if let Some(name) = get("events").and_then(|v| v.as_str()) {
        let r = call(eng, "events", Json::obj([("limit", Json::from(2000))]))?;
        let n = r.get("events").and_then(|e| e.as_array()).map(|a| a.iter().filter(|e| e.get("name").and_then(|n| n.as_str()) == Some(name)).count()).unwrap_or(0);
        return Ok(Json::from(n));
    }
    if get("entity").is_some() || get("name").is_some() || get("exists").is_some() {
        let want_exists = get("exists").is_some();
        let who = get("entity").or_else(|| get("name")).or_else(|| get("exists")).cloned().unwrap_or(Json::Null);
        let uid = match &who {
            Json::Str(name) => {
                let r = call(eng, "query", Json::obj([("with", Json::arr([Json::str("Name")])), ("select", Json::arr([Json::str("Name")])), ("limit", Json::from(10_000))]))?;
                r.get("entities")
                    .and_then(|e| e.as_array())
                    .and_then(|a| a.iter().find(|e| e.get("components").and_then(|c| c.get("Name")).and_then(|n| n.get("value")).and_then(|v| v.as_str()) == Some(name.as_str())))
                    .and_then(|e| e.get("uid").and_then(|u| u.as_u64()))
            }
            other => other.as_u64(),
        };
        let Some(uid) = uid else {
            return if want_exists { Ok(Json::Bool(false)) } else { Err(format!("no entity {}", shown(&who))) };
        };
        if want_exists {
            let alive = eng.call("get", &Json::obj([("uid", Json::from(uid as i64)), ("components", Json::arr([]))])).is_ok();
            return Ok(Json::Bool(alive));
        }
        let path = get("field").and_then(|f| f.as_str()).ok_or("an entity target needs a `field` such as \"Transform.pos[1]\"")?;
        let (comp, rest) = path.split_once('.').ok_or_else(|| format!("`field` is `Component.field` or `Component.field[i]`, got `{path}`"))?;
        let r = call(eng, "get", Json::obj([("uid", Json::from(uid as i64)), ("components", Json::arr([Json::str(comp)]))]))?;
        let mut cur = r.get("components").and_then(|c| c.get(comp)).cloned().ok_or_else(|| format!("entity {uid} has no {comp}"))?;
        // `field`, `field[2]`, `a.b[1]`
        for part in rest.split('.') {
            let (name, index) = match part.split_once('[') {
                Some((n, i)) => (n, Some(i.trim_end_matches(']').parse::<usize>().map_err(|_| format!("bad index in `{part}`"))?)),
                None => (part, None),
            };
            cur = cur.get(name).cloned().ok_or_else(|| format!("{comp} has no field `{name}`"))?;
            if let Some(i) = index {
                cur = cur.index(i).cloned().ok_or_else(|| format!("`{name}` has no element {i}"))?;
            }
        }
        return Ok(cur);
    }
    Err(format!("say what to look at: one of {}", TARGET_KEYS.join(", ")))
}


/// Is the condition `cond` (a target plus comparators, as in an `expect` step) true in `engine` right now? Returns whether it holds,
/// the value seen, and a text of what was wanted. Used by playtests and by `lunaris mind --success`.
pub fn evaluate(engine: &mut Engine, cond: &Json) -> std::result::Result<(bool, Json, String), String> {
    let actual = value_of(engine, cond)?;
    let (ok, wanted) = Runner::compare(&actual, cond)?;
    Ok((ok, actual, wanted))
}

impl Runner {
    fn call(&mut self, tool: &str, args: Json) -> std::result::Result<Json, String> {
        self.eng.call(tool, &args).map_err(msg)
    }

    fn record(&mut self, step: usize, what: String, outcome: std::result::Result<Option<Json>, String>) {
        let mut o = Json::obj([("step", Json::from(step)), ("what", Json::Str(what))]);
        match outcome {
            Ok(actual) => {
                self.passed += 1;
                o.set("ok", Json::Bool(true));
                if let Some(a) = actual {
                    o.set("actual", a);
                }
            }
            Err(m) => {
                self.failed += 1;
                o.set("ok", Json::Bool(false));
                o.set("message", Json::Str(m));
            }
        }
        self.results.push(o);
    }

    /// The value a target names, or why it cannot be read.
    fn value(&mut self, t: &Json) -> std::result::Result<Json, String> {
        value_of(&mut self.eng, t)
    }

    /// Does `actual` satisfy every comparator in `cond`? `Ok(true/false)` plus a text of what was wanted.
    fn compare(actual: &Json, cond: &Json) -> std::result::Result<(bool, String), String> {
        let Json::Obj(pairs) = cond else { return Err("an expectation is an object".into()) };
        let mut wanted: Vec<String> = Vec::new();
        let mut ok = true;
        let mut any = false;
        for (k, v) in pairs {
            if !COMPARATORS.contains(&k.as_str()) {
                continue;
            }
            any = true;
            let num = |x: &Json| x.as_f64();
            let (pass, text) = match k.as_str() {
                "eq" => (
                    match (num(actual), num(v)) {
                        (Some(a), Some(b)) => a == b,
                        _ => actual == v,
                    },
                    format!("== {}", shown(v)),
                ),
                "ne" => (
                    match (num(actual), num(v)) {
                        (Some(a), Some(b)) => a != b,
                        _ => actual != v,
                    },
                    format!("!= {}", shown(v)),
                ),
                "gt" | "ge" | "lt" | "le" => {
                    let (a, b) = (num(actual).ok_or_else(|| format!("{} is not a number", shown(actual)))?, num(v).ok_or_else(|| format!("`{k}` needs a number"))?);
                    let (pass, sym) = match k.as_str() {
                        "gt" => (a > b, ">"),
                        "ge" => (a >= b, ">="),
                        "lt" => (a < b, "<"),
                        _ => (a <= b, "<="),
                    };
                    (pass, format!("{sym} {}", shown(v)))
                }
                "approx" => {
                    let arr = v.as_array().filter(|a| a.len() == 2).ok_or("`approx` is [value, tolerance]")?;
                    let (target, tol) = (arr[0].as_f64().ok_or("`approx` value must be a number")?, arr[1].as_f64().ok_or("`approx` tolerance must be a number")?);
                    let a = num(actual).ok_or_else(|| format!("{} is not a number", shown(actual)))?;
                    ((a - target).abs() <= tol, format!("within {tol} of {target}"))
                }
                _ => {
                    let arr = v.as_array().filter(|a| a.len() == 2).ok_or("`between` is [low, high]")?;
                    let (lo, hi) = (arr[0].as_f64().ok_or("`between` needs numbers")?, arr[1].as_f64().ok_or("`between` needs numbers")?);
                    let a = num(actual).ok_or_else(|| format!("{} is not a number", shown(actual)))?;
                    ((lo..=hi).contains(&a), format!("between {lo} and {hi}"))
                }
            };
            ok &= pass;
            wanted.push(text);
        }
        if !any {
            return Err(format!("say what is expected: one of {}", COMPARATORS.join(", ")));
        }
        Ok((ok, wanted.join(" and ")))
    }

    fn describe_target(t: &Json) -> String {
        for k in TARGET_KEYS {
            if let Some(v) = t.get(k) {
                return match *k {
                    "field" => format!("{} {}", t.get("entity").or_else(|| t.get("name")).map(shown).unwrap_or_default(), shown(v)),
                    "entity" | "name" | "exists" if t.get("field").is_some() => continue,
                    "count" => format!("number of entities {}", shown(v)),
                    "events" => format!("events named {}", shown(v)),
                    "tick" | "hash" => (*k).to_string(),
                    "check" => format!("check {}", shown(v)),
                    other => format!("{other} {}", shown(v)),
                };
            }
        }
        "?".into()
    }

    fn expect(&mut self, step: usize, cond: &Json) {
        let what = format!("expect {}", Self::describe_target(cond));
        let outcome: std::result::Result<Option<Json>, String> = (|| {
            let actual = self.value(cond)?;
            let (ok, wanted) = Self::compare(&actual, cond)?;
            if ok {
                Ok(Some(actual))
            } else {
                Err(cond.get("message").and_then(|m| m.as_str()).map(|m| format!("{m} (got {}, wanted {wanted})", shown(&actual))).unwrap_or_else(|| format!("got {}, wanted {wanted}", shown(&actual))))
            }
        })();
        self.record(step, what, outcome);
    }

    fn until(&mut self, step: usize, cond: &Json) {
        let what = format!("until {}", Self::describe_target(cond));
        let max = cond.get("max_ticks").and_then(|m| m.as_u64()).unwrap_or(600);
        let poll = cond.get("poll").and_then(|m| m.as_u64()).unwrap_or(1).clamp(1, 600);
        let outcome: std::result::Result<Option<Json>, String> = (|| {
            let mut waited = 0u64;
            loop {
                let actual = self.value(cond)?;
                let (ok, wanted) = Self::compare(&actual, cond)?;
                if ok {
                    return Ok(Some(Json::obj([("value", actual), ("after_ticks", Json::from(waited as i64))])));
                }
                if waited >= max {
                    return Err(format!("still {} after {waited} ticks, wanted {wanted}", shown(&actual)));
                }
                let n = poll.min(max - waited).max(1);
                self.advance(n)?;
                waited += n;
            }
        })();
        self.record(step, what, outcome);
    }

    fn advance(&mut self, ticks: u64) -> std::result::Result<(), String> {
        if self.ticks + ticks > MAX_TOTAL_TICKS {
            return Err(format!("the spec would run more than {MAX_TOTAL_TICKS} ticks in total"));
        }
        self.call("step", Json::obj([("ticks", Json::from(ticks as i64))]))?;
        self.ticks += ticks;
        Ok(())
    }

    fn run_step(&mut self, i: usize, step: &Json) {
        let Json::Obj(pairs) = step else { return self.record(i, "step".into(), Err("a step is an object such as {\"step\": 60}".into())) };
        let Some((kind, arg)) = pairs.iter().find(|(k, _)| k != "message") else { return self.record(i, "step".into(), Err("an empty step".into())) };
        match kind.as_str() {
            "note" => {}
            "step" => {
                let n = arg.as_u64();
                let r = n.ok_or_else(|| "`step` is a number of ticks".to_string()).and_then(|n| self.advance(n));
                if let Err(m) = r {
                    self.record(i, "step".into(), Err(m));
                }
            }
            "input" => {
                if let Err(m) = self.call("input", Json::obj([("values", arg.clone())])) {
                    self.record(i, "input".into(), Err(m));
                }
            }
            "call" => {
                let tool = arg.get("tool").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let args = arg.get("args").cloned().unwrap_or(Json::Obj(Vec::new()));
                let want_error = arg.get("error").and_then(|e| e.as_str()).map(str::to_string);
                let result = self.eng.call(&tool, &args);
                let outcome = match (result, want_error) {
                    (Ok(_), None) => Ok(None),
                    (Ok(_), Some(code)) => Err(format!("expected the error `{code}` but the call worked")),
                    (Err(e), None) => Err(msg(e)),
                    (Err(e), Some(code)) => {
                        if e.code == code {
                            Ok(None)
                        } else {
                            Err(format!("expected the error `{code}`, got {}", msg(e)))
                        }
                    }
                };
                self.record(i, format!("call {tool}"), outcome);
            }
            "expect" => self.expect(i, arg),
            "until" => self.until(i, arg),
            other => self.record(i, other.to_string(), Err(format!("unknown step `{other}`; steps are step, input, call, expect, until, note"))),
        }
    }
}

impl Engine {
    /// The `verify` tool: a spec inline (`spec`) or in a workspace file (`path`).
    pub(crate) fn verify_tool(&mut self, a: &crate::engine::Args) -> Result<Json> {
        let spec = match (a.0.get("spec"), a.opt_str("path")?) {
            (Some(s), None) => s.clone(),
            (None, Some(p)) => lunaris_json::parse(&self.read_text(p)?).map_err(|e| EcsError::new("bad_spec", e.to_string()).at("path"))?,
            _ => return Err(EcsError::new("bad_arguments", "give exactly one of `spec` or `path`")),
        };
        run_spec(&spec, &self.root)
    }
}

/// Run a spec on a scratch engine rooted at `root` and return the report.
pub fn run_spec(spec: &Json, root: &Path) -> Result<Json> {
    let Json::Obj(_) = spec else { return Err(EcsError::new("bad_spec", "a spec is an object with `steps`").hint("{\"scene\": \"examples/orb_dash.lunaris.json\", \"steps\": [{\"step\": 60}, {\"expect\": {\"resource\": \"score\", \"ge\": 0}}]}")) };
    let steps = spec.get("steps").and_then(|s| s.as_array()).ok_or_else(|| EcsError::new("bad_spec", "a spec needs a `steps` array").at("steps"))?;
    if steps.len() > MAX_STEPS {
        return Err(EcsError::new("bad_spec", format!("a spec may have at most {MAX_STEPS} steps")).at("steps"));
    }
    let seed = spec.get("seed").and_then(|s| s.as_u64()).unwrap_or(1);
    let mut r = Runner { eng: Engine::new(seed, root.to_path_buf()), results: Vec::new(), passed: 0, failed: 0, ticks: 0 };
    if let Some(scene) = spec.get("scene").and_then(|s| s.as_str()) {
        r.eng.call("load_scene", &Json::obj([("path", Json::str(scene))])).map_err(|e| e.at("scene"))?;
    }
    if let Some(threads) = spec.get("threads").and_then(|t| t.as_u64()) {
        r.eng.sim.threads = (threads as usize).clamp(1, 64);
    }
    let replay_check = spec.get("replay_check").and_then(|b| b.as_bool()).unwrap_or(false);
    if replay_check {
        r.eng.call("record_start", &Json::Obj(Vec::new()))?;
    }
    for (i, step) in steps.iter().enumerate() {
        r.run_step(i, step);
    }
    let mut report = Json::obj([("name", Json::str(spec.get("name").and_then(|n| n.as_str()).unwrap_or("(unnamed)")))]);
    if replay_check {
        match r.eng.call("record_stop", &Json::Obj(Vec::new())).and_then(|_| r.eng.call("replay", &Json::Obj(Vec::new()))) {
            Ok(rep) => {
                let matches = rep.get("matches").and_then(|m| m.as_bool()).unwrap_or(false);
                let what = "the recording replays to the same state hash".to_string();
                let outcome = if matches { Ok(rep.get("got").cloned()) } else { Err(format!("replay gave {}, the live run {}", shown(rep.get("got").unwrap_or(&Json::Null)), shown(rep.get("expected").unwrap_or(&Json::Null)))) };
                r.record(steps.len(), what, outcome);
            }
            Err(e) => r.record(steps.len(), "replay".into(), Err(msg(e))),
        }
    }
    let hash = r.eng.call("hash", &Json::Obj(Vec::new()))?;
    report.set("ok", Json::Bool(r.failed == 0));
    report.set("passed", Json::from(r.passed));
    report.set("failed", Json::from(r.failed));
    report.set("ticks", Json::from(r.eng.sim.tick as i64));
    report.set("hash", hash.get("hash").cloned().unwrap_or(Json::Null));
    report.set("results", Json::Arr(r.results));
    Ok(report)
}
