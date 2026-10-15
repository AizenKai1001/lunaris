//! A language-model client plays Orb Dash through the real OpenAI-style HTTP path. The "model" here is a small rule-based server
//! (no network beyond localhost, no real model): it reads the observation JSON in the last user message like a model would and
//! answers with `input` actions. The point is the whole loop: observation -> HTTP -> reply -> permission check -> tool -> step,
//! then the recorded session replays without the model.
use lunaris_api::Engine;
use lunaris_json::{parse, Json};
use lunaris_mind::{Mind, MindConfig, OpenAiModel};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

fn policy(observation: &Json) -> String {
    let pos = observation.get("you").and_then(|y| y.get("Transform")).and_then(|t| t.get("pos")).and_then(|p| p.as_array()).map(|p| [p[0].as_f64().unwrap(), p[2].as_f64().unwrap()]).unwrap_or([0.0, 0.0]);
    let orbs: Vec<(f64, f64, String)> = observation
        .get("nearby")
        .and_then(|n| n.as_array())
        .unwrap_or(&[])
        .iter()
        .filter(|e| e.get("name").and_then(|n| n.as_str()).is_some_and(|n| n.starts_with("Orb ")))
        .map(|e| {
            let p = e.get("pos").and_then(|p| p.as_array()).unwrap();
            (p[0].as_f64().unwrap(), p[2].as_f64().unwrap(), e.get("name").and_then(|n| n.as_str()).unwrap().to_string())
        })
        .collect();
    let target = orbs.iter().min_by(|a, b| ((a.0 - pos[0]).powi(2) + (a.1 - pos[1]).powi(2)).total_cmp(&((b.0 - pos[0]).powi(2) + (b.1 - pos[1]).powi(2))));
    let (thought, keys) = match target {
        Some((x, z, name)) => {
            let (dx, dz) = (x - pos[0], z - pos[1]);
            (format!("heading for {name}"), [("d", dx > 0.2), ("a", dx < -0.2), ("s", dz > 0.2), ("w", dz < -0.2)])
        }
        None => ("nothing left to collect".to_string(), [("d", false), ("a", false), ("s", false), ("w", false)]),
    };
    let values: Vec<String> = keys.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
    // wrap the JSON the way chatty models do: some prose and a code fence around it
    format!("Sure! Here is my move.\n```json\n{{\"thought\":\"{thought}\",\"actions\":[{{\"tool\":\"input\",\"args\":{{\"values\":{{{}}}}}}}]}}\n```", values.join(","))
}

fn serve(listener: TcpListener, first_system_prompt: Arc<Mutex<Option<String>>>) {
    for stream in listener.incoming() {
        let Ok(mut s) = stream else { break };
        let mut buf = vec![0u8; 1 << 20];
        let mut n = 0;
        loop {
            let Ok(k) = s.read(&mut buf[n..]) else { return };
            if k == 0 {
                return;
            }
            n += k;
            let text = String::from_utf8_lossy(&buf[..n]).to_string();
            if let Some(he) = text.find("\r\n\r\n") {
                let len: usize = text[..he].to_lowercase().lines().find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                if n >= he + 4 + len {
                    break;
                }
            }
        }
        let text = String::from_utf8_lossy(&buf[..n]).to_string();
        if text.starts_with("GET ") {
            // the client asking `/v1/models` for the context window: this server does not know it
            let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            continue;
        }
        let body = parse(&text[text.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        let messages = body.get("messages").and_then(|m| m.as_array()).unwrap();
        if let Some(sys) = messages.first().and_then(|m| m.get("content")).and_then(|c| c.as_str()) {
            first_system_prompt.lock().unwrap().get_or_insert_with(|| sys.to_string());
        }
        let last = messages.last().and_then(|m| m.get("content")).and_then(|c| c.as_str()).unwrap();
        let obs_text = last.strip_prefix("Observation: ").and_then(|t| t.split("\nReply with ONLY").next()).unwrap();
        let reply = policy(&parse(obs_text).unwrap());
        let json = Json::obj([("choices", Json::arr([Json::obj([("message", Json::obj([("role", Json::str("assistant")), ("content", Json::Str(reply))]))])]))]).to_string_compact().unwrap();
        let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}", json.len()).as_bytes());
    }
}

#[test]
fn a_model_server_plays_orb_dash_and_the_game_replays_without_it() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let prompt = Arc::new(Mutex::new(None));
    let p2 = Arc::clone(&prompt);
    std::thread::spawn(move || serve(listener, p2));

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let mut e = Engine::new(1, root);
    e.call("load_scene", &parse(r#"{"path":"examples/orb_dash.lunaris.json"}"#).unwrap()).unwrap();
    let player = e.call("query", &parse(r#"{"with":["Player"],"select":[]}"#).unwrap()).unwrap().get("entities").unwrap().index(0).unwrap().get("uid").unwrap().as_u64().unwrap();
    e.call("record_start", &Json::Null).unwrap();

    let mut cfg = MindConfig::new(player, "collect all 12 gold orbs before the clock runs out");
    cfg.allowed_tools = ["get", "nearby", "input"].iter().map(|s| s.to_string()).collect();
    cfg.radius = 80.0;
    cfg.nearby_limit = 60;
    cfg.think_ticks = 5;
    cfg.history = 2;
    let mut mind = Mind::new(cfg, Box::new(OpenAiModel::new(&base, "mock")));
    let mut turns = 0;
    while turns < 400 {
        mind.think(&mut e).unwrap();
        turns += 1;
        let res = e.call("resources", &Json::Null).unwrap();
        if res.get("resources").and_then(|r| r.get("score")).and_then(|s| s.as_f64()) == Some(12.0) {
            break;
        }
    }
    let res = e.call("resources", &Json::Null).unwrap().get("resources").unwrap().clone();
    assert_eq!(res.get("score").and_then(|s| s.as_f64()), Some(12.0), "the model collected every orb in {turns} decisions");
    assert!(res.get("time_left").and_then(|t| t.as_f64()).unwrap() > 0.0, "and beat the clock");
    assert!(mind.transcript.iter().all(|t| t.problem.is_none()), "every reply (even in a code fence with prose around it) was usable");
    // the system prompt told the model about the input tool and the observation carried the game state
    let sys = prompt.lock().unwrap().clone().unwrap();
    assert!(sys.contains("press keys with the `input` tool"), "{sys}");
    assert!(mind.transcript[0].observation.get("world").and_then(|w| w.get("score")).is_some(), "the score is part of what the model sees");
    // permissions: the model cannot do anything outside its allow-list (here: no `set`)
    let mut denied = Mind::new(MindConfig { allowed_tools: vec!["nearby".into()], ..MindConfig::new(player, "x") }, Box::new(OpenAiModel::new(&base, "mock")));
    let t = denied.think(&mut e).unwrap();
    assert!(!t.actions[0].ok && t.actions[0].result.as_str().unwrap().contains("not allowed"));
    // the whole session is in the recording: replaying it needs no model server
    e.call("input", &parse(r#"{"values":{"d":false,"a":false,"s":false,"w":false}}"#).unwrap()).unwrap();
    e.call("record_stop", &Json::Null).unwrap();
    let replay = e.call("replay", &Json::Null).unwrap();
    assert_eq!(replay.get("matches"), Some(&Json::Bool(true)));
}
