//! `lunaris mind`: let a language model (or a scripted stand-in) play one body in a scene, record the session, and print what happened.
//!
//!   lunaris mind <scene.json> --body UID --goal "text" --scripted replies.json [--turns N] [--think-ticks N] [--record FILE] [--transcript FILE]
//!   options for games: --tools get,nearby,input  --radius 40  --nearby-limit 40  --max-actions 6  --minimap 21  --hearing 5
//!   --success FILE   a goal as data: one condition or a list, like an `expect` in a playtest ({"resource": "score", "ge": 12}); the
//!                    run stops when all hold, prints how many turns it took, and the exit code says whether it was reached
//!   lunaris mind <scene.json> --body UID --goal "text" --endpoint http://host:port/v1 --model NAME [--key-env VAR] ...
//!   options for a real model server: --json-schema (the reply must follow the reply schema: constrained decoding, always parses),
//!   --temperature 0.3  --max-tokens 300  --model-seed N  --no-thinking  --history 16 (messages kept; the oldest half goes at once)
//!   --context-tokens 6144 (the server's context window: the history is cut before the prompt would use three quarters of it)
//!   --timeout 120 (seconds per call)
//!   --memory-endpoint http://host:port/v1 [--memory-model NAME] [--memory-k 3]   long-term memory: each turn is embedded and the notes most like the
//!                    present come back in the observation (any OpenAI-style /v1/embeddings server)
//!
//! There is no default endpoint: a real model is used only when `--endpoint` is given.
use lunaris_api::Engine;
use lunaris_json::{parse, Json};
use lunaris_mind::{reply_schema, Memory, Mind, MindConfig, Model, OpenAiEmbedder, OpenAiModel, ScriptedModel};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

pub fn run(positional: &[String], kv: &HashMap<String, String>, root: PathBuf, seed: u64) -> ExitCode {
    let Some(scene) = positional.first() else {
        eprintln!("error: `mind` needs a scene file (see examples/arena.lunaris.json)");
        return ExitCode::from(2);
    };
    let get = |k: &str| kv.get(k).map(|s| s.as_str());
    let Some(body) = get("body").and_then(|b| b.parse::<u64>().ok()) else {
        eprintln!("error: `mind` needs --body <uid> (the entity the mind controls)");
        return ExitCode::from(2);
    };
    let goal = get("goal").unwrap_or("explore and stay safe");
    let turns: usize = get("turns").and_then(|t| t.parse().ok()).unwrap_or(20);

    // the mind's settings first: a model that is asked for constrained replies needs to know the tools it may name
    let mut cfg = MindConfig::new(body, goal);
    if let Some(n) = get("history").and_then(|t| t.parse().ok()) {
        cfg.history = n;
    }
    if let Some(n) = get("context-tokens").and_then(|t| t.parse().ok()) {
        cfg.context_tokens = n;
    }
    if let Some(t) = get("think-ticks").and_then(|t| t.parse().ok()) {
        cfg.think_ticks = t;
    }
    if let Some(r) = get("radius").and_then(|t| t.parse().ok()) {
        cfg.radius = r;
    }
    if let Some(n) = get("nearby-limit").and_then(|t| t.parse().ok()) {
        cfg.nearby_limit = n;
    }
    if let Some(n) = get("memory-k").and_then(|t| t.parse().ok()) {
        cfg.memory_k = n;
    }
    if let Some(n) = get("nearby-repeat").and_then(|t| t.parse().ok()) {
        cfg.nearby_repeat = n;
    }
    if let Some(n) = get("max-actions").and_then(|t| t.parse().ok()) {
        cfg.max_actions = n;
    }
    if let Some(n) = get("hearing").and_then(|t| t.parse().ok()) {
        cfg.hearing = n;
    }
    if let Some(n) = get("minimap").and_then(|t| t.parse().ok()) {
        cfg.minimap = n;
    }
    if let Some(list) = get("tools") {
        // the engine's tools, and the two verbs the mind loop runs itself (`go_to`, `wait`)
        let mut known = lunaris_api::tool_names();
        known.extend(["go_to", "wait"]);
        let wanted: Vec<String> = list.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
        if let Some(bad) = wanted.iter().find(|t| !known.contains(&t.as_str())) {
            eprintln!("error: --tools names `{bad}`, which is not a tool (see `lunaris tools`)");
            return ExitCode::from(2);
        }
        cfg.allowed_tools = wanted;
    }

    let model: Box<dyn Model> = match (get("scripted"), get("endpoint")) {
        (Some(file), None) => {
            let text = match std::fs::read_to_string(root.join(file)).or_else(|_| std::fs::read_to_string(file)) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("error: cannot read {file}: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let replies: Vec<String> = match parse(&text) {
                Ok(Json::Arr(items)) => items.iter().map(|i| i.as_str().map(str::to_string).unwrap_or_else(|| i.to_string_lossy())).collect(),
                Ok(_) => {
                    eprintln!("error: the scripted file must be a JSON array of replies (strings or objects)");
                    return ExitCode::FAILURE;
                }
                Err(e) => {
                    eprintln!("error: {file} is not valid JSON: {e}");
                    return ExitCode::FAILURE;
                }
            };
            Box::new(ScriptedModel::new(replies))
        }
        (None, Some(url)) => {
            let mut m = OpenAiModel::new(url, get("model").unwrap_or("local"));
            if let Some(var) = get("key-env") {
                m.api_key = std::env::var(var).ok();
            }
            if kv.contains_key("json-schema") {
                m.schema = Some(reply_schema(&cfg));
            }
            if let Some(t) = get("temperature").and_then(|t| t.parse().ok()) {
                m.temperature = t;
            }
            if let Some(n) = get("max-tokens").and_then(|t| t.parse().ok()) {
                m.max_tokens = n;
            }
            m.seed = get("model-seed").and_then(|t| t.parse().ok());
            m.no_thinking = kv.contains_key("no-thinking");
            if let Some(secs) = get("timeout").and_then(|t| t.parse().ok()) {
                m.timeout = std::time::Duration::from_secs(secs);
            }
            eprintln!("using the model server at {url}");
            Box::new(m)
        }
        _ => {
            eprintln!("error: give exactly one of --scripted <file> (offline stand-in) or --endpoint <http://host:port/v1> (a real model server)");
            return ExitCode::from(2);
        }
    };

    let mut engine = Engine::new(seed, root.clone());
    if let Err(e) = engine.call("load_scene", &Json::obj([("path", Json::str(scene.clone()))])) {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    if let Err(e) = engine.call("record_start", &Json::Null) {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    // the success condition, if any: every condition in the file must hold
    let success: Option<Vec<Json>> = match get("success") {
        None => None,
        Some(file) => {
            let text = match std::fs::read_to_string(root.join(file)).or_else(|_| std::fs::read_to_string(file)) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("error: cannot read {file}: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match parse(&text) {
                Ok(Json::Arr(items)) if !items.is_empty() => Some(items),
                Ok(obj @ Json::Obj(_)) => Some(vec![obj]),
                Ok(_) => {
                    eprintln!("error: {file} must hold a condition object or a non-empty array of them");
                    return ExitCode::FAILURE;
                }
                Err(e) => {
                    eprintln!("error: {file} is not valid JSON: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
    };
    let mut mind = Mind::new(cfg, model);
    if let Some(url) = get("memory-endpoint") {
        let mut emb = OpenAiEmbedder::new(url, get("memory-model").unwrap_or("nomic-embed-text-v1.5"));
        if let Some(var) = get("key-env") {
            emb.api_key = std::env::var(var).ok();
        }
        mind.memory = Some(Memory::new(Box::new(emb)));
        eprintln!("long-term memory through the embedding server at {url}");
    }
    println!("mind: {} controlling entity {body}; goal: {goal}", mind.model.name());
    let mut reached: Option<Option<usize>> = None;
    let outcome = match &success {
        None => mind.run(&mut engine, turns),
        Some(conds) => {
            let mut last_seen = String::new();
            let r = mind.run_until(&mut engine, turns, |e| {
                let mut all = true;
                last_seen.clear();
                for c in conds {
                    match lunaris_api::evaluate(e, c) {
                        Ok((ok, actual, wanted)) => {
                            all &= ok;
                            last_seen.push_str(&format!("{} {} (wanted {wanted}); ", if ok { "ok:" } else { "NOT YET:" }, actual.to_string_lossy()));
                        }
                        Err(m) => {
                            all = false;
                            last_seen.push_str(&format!("cannot read the condition: {m}; "));
                        }
                    }
                }
                all
            });
            if let Ok(turns_used) = &r {
                match turns_used {
                    Some(n) => println!("success: the goal was reached after {n} turns (tick {})", engine.sim.tick),
                    None => println!("success: NOT reached in {turns} turns. Last look: {last_seen}"),
                }
                reached = Some(*turns_used);
            }
            r.map(|_| ())
        }
    };
    for t in &mind.transcript {
        let ok = t.actions.iter().filter(|a| a.ok).count();
        let stats = t.model_stats.as_ref().map(|m| {
            let usage = m.get("usage");
            let n = |k: &str| usage.and_then(|u| u.get(k)).and_then(|v| v.as_i64());
            let cached = usage.and_then(|u| u.get("prompt_tokens_details")).and_then(|d| d.get("cached_tokens")).and_then(|v| v.as_i64());
            format!("  (prompt {} tokens{}, reply {}, {} ms)", n("prompt_tokens").unwrap_or(0), cached.map(|c| format!(", {c} cached")).unwrap_or_default(), n("completion_tokens").unwrap_or(0), m.get("elapsed_ms").and_then(|v| v.as_i64()).unwrap_or(0))
        });
        println!("tick {:>5}  {}  [{} of {} actions ok]{}{}", t.tick, if t.thought.is_empty() { "(no thought)" } else { &t.thought }, ok, t.actions.len(), t.problem.as_ref().map(|p| format!("  PROBLEM: {p}")).unwrap_or_default(), stats.unwrap_or_default());
    }
    if let Err(e) = &outcome {
        eprintln!("stopped: {e}");
    }
    let record = get("record");
    let stop = engine.call("record_stop", &match record {
        Some(p) => Json::obj([("path", Json::str(p))]),
        None => Json::Null,
    });
    match stop {
        Ok(v) => println!("recorded {} commands up to tick {} (state hash {})", v.get("entries").and_then(|e| e.as_i64()).unwrap_or(0), v.get("final_tick").and_then(|e| e.as_i64()).unwrap_or(0), v.get("final_hash").and_then(|e| e.as_str()).unwrap_or("?")),
        Err(e) => eprintln!("could not stop the recording: {e}"),
    }
    if let Ok(r) = engine.call("replay", &Json::Null) {
        println!("replay without the model: {}", if r.get("matches") == Some(&Json::Bool(true)) { "identical" } else { "DIFFERENT" });
    }
    if let Some(path) = get("transcript") {
        let doc = Json::obj([("model", Json::str(mind.model.name())), ("goal", Json::str(goal)), ("turns", Json::arr(mind.transcript.iter().map(|t| t.to_json())))]);
        let text = doc.to_string_pretty().unwrap_or_default();
        match std::fs::write(PathBuf::from(path), text) {
            Ok(()) => println!("transcript written to {path}"),
            Err(e) => eprintln!("could not write the transcript: {e}"),
        }
    }
    let goal_missed = matches!(reached, Some(None));
    if outcome.is_ok() && !goal_missed { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
