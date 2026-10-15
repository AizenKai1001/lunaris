//! A mind drives one body: each turn it is shown an observation, answers with JSON actions, the actions run through the same
//! tool API every other client uses (checked against permissions first), and the world advances. Because actions are ordinary
//! commands, a session recorded with `record_start`/`record_stop` replays bit for bit without the model.
use crate::memory::Memory;
use crate::model::{Message, Model};
use lunaris_api::{tool_defs, Engine};
use lunaris_json::{parse, Json};

#[derive(Clone, Debug)]
pub struct MindConfig {
    /// uid of the entity this mind controls.
    pub body: u64,
    pub goal: String,
    /// Simulation ticks that pass after each decision.
    pub think_ticks: u64,
    /// Conversation messages kept. The history only ever grows by appending until it passes this size; then the oldest half is
    /// dropped in one step. (Dropping one turn at a time would change the start of every prompt, and a model server could never reuse
    /// what it already read of the previous one.)
    pub history: usize,
    /// The model's context window in tokens (the server's `--ctx`); 0 asks the server (`GET /v1/models`). With a known window the
    /// message count in `history` no longer matters: the prompt grows until it would pass three quarters of the window, then it is cut
    /// once, deep (a summary and the last two turns), which keeps the server's prompt cache useful for as long as possible.
    pub context_tokens: usize,
    pub max_actions: usize,
    /// How far the mind can see other entities ("nearby" radius, metres).
    pub radius: f32,
    pub nearby_limit: usize,
    /// After the first observation (and the first after every cut) only this many of the nearest things are repeated each turn.
    pub nearby_repeat: usize,
    /// The longest one `go_to` may take, in simulation ticks (900 = 15 s at 60 Hz).
    pub go_to_ticks: u64,
    /// With a `Mind::memory`: how many older notes to bring back each turn, and how similar (cosine) a note must be to be worth it.
    pub memory_k: usize,
    pub memory_min_score: f32,
    /// Add a text map of the surroundings (this many cells per side) to every observation; 0 = none.
    pub minimap: usize,
    /// Sounds the mind hears: at most this many events since its last turn are listed in `heard` (with where they came from and whether a
    /// wall muffles them); 0 = deaf. Sounds its own body makes are never listed.
    pub hearing: usize,
    pub allowed_tools: Vec<String>,
}

const WRITE_TOOLS: [&str; 3] = ["set", "add_component", "remove_component"];

/// Components that only say how something is drawn or heard; they are left out of what a mind sees of its own body.
const VISUAL_ONLY: [&str; 11] = ["MeshRenderer", "Sprite", "Text", "Light", "Camera", "Sound", "SoundFile", "AudioSource", "AcousticMaterial", "Emitter", "Environment"];

/// Sounds farther away than this (metres) are not heard.
const HEARING_RANGE: f32 = 40.0;

impl MindConfig {
    pub fn new(body: u64, goal: &str) -> MindConfig {
        MindConfig {
            body,
            goal: goal.to_string(),
            think_ticks: 12,
            history: 16,
            context_tokens: 0,
            max_actions: 6,
            radius: 12.0,
            nearby_limit: 12,
            nearby_repeat: 5,
            go_to_ticks: 900,
            memory_k: 3,
            memory_min_score: 0.4,
            minimap: 0,
            hearing: 5,
            allowed_tools: ["get", "query", "nearby", "raycast", "contacts", "set", "add_component", "remove_component"].iter().map(|s| s.to_string()).collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ActionResult {
    pub tool: String,
    pub args: Json,
    pub ok: bool,
    pub result: Json,
}

#[derive(Clone, Debug)]
pub struct Turn {
    pub tick: u64,
    pub observation: Json,
    pub reply: String,
    pub thought: String,
    pub actions: Vec<ActionResult>,
    /// Set when the reply could not be used (the model is told on the next turn).
    pub problem: Option<String>,
    /// What the model server said about this call (tokens, cached tokens, timings), when it says anything.
    pub model_stats: Option<Json>,
    /// The older notes the memory put in front of the model this turn.
    pub memories: Vec<String>,
}

impl Turn {
    pub fn to_json(&self) -> Json {
        let mut o = Json::obj([
            ("tick", Json::from(self.tick as i64)),
            ("observation", self.observation.clone()),
            ("reply", Json::str(self.reply.clone())),
            ("thought", Json::str(self.thought.clone())),
            (
                "actions",
                Json::arr(self.actions.iter().map(|a| Json::obj([("tool", Json::str(a.tool.clone())), ("args", a.args.clone()), ("ok", Json::Bool(a.ok)), ("result", a.result.clone())]))),
            ),
        ]);
        if let Some(p) = &self.problem {
            o.set("problem", Json::str(p.clone()));
        }
        if let Some(m) = &self.model_stats {
            o.set("model", m.clone());
        }
        if !self.memories.is_empty() {
            o.set("memories", Json::arr(self.memories.iter().map(|m| Json::str(m.clone()))));
        }
        o
    }
}

/// Round numbers to two decimals (whole numbers stay whole): metres need no more, and every digit costs the model tokens.
fn compact(v: &Json) -> Json {
    match v {
        Json::Float(f) => {
            let r = (f * 100.0).round() / 100.0;
            if r.is_finite() && r == r.trunc() && r.abs() < 1e15 { Json::Int(r as i64) } else { Json::Float(r) }
        }
        Json::Arr(a) => Json::Arr(a.iter().map(compact).collect()),
        Json::Obj(o) => Json::Obj(o.iter().map(|(k, v)| (k.clone(), compact(v))).collect()),
        other => other.clone(),
    }
}

/// The observation as the model reads it: compact JSON on one line, with the lists that are empty left out.
fn observation_text(obs: &Json) -> String {
    let mut shown = compact(obs);
    if let Json::Obj(o) = &mut shown {
        o.retain(|(k, v)| !(matches!(k.as_str(), "nearby" | "touching") && matches!(v, Json::Arr(a) if a.is_empty())));
    }
    shown.to_string_compact().unwrap_or_else(|_| shown.to_string_lossy())
}

/// The JSON schema of a reply: `{"thought": "...", "actions": [{"tool": <one of the allowed tools>, "args": {...}}]}`. Pass it to
/// `OpenAiModel::schema` and a server that supports constrained decoding can only produce a reply the mind can run.
pub fn reply_schema(cfg: &MindConfig) -> Json {
    Json::obj([
        ("type", Json::str("object")),
        (
            "properties",
            Json::obj([
                ("thought", Json::obj([("type", Json::str("string"))])),
                (
                    "actions",
                    Json::obj([
                        ("type", Json::str("array")),
                        ("maxItems", Json::from(cfg.max_actions)),
                        (
                            "items",
                            Json::obj([
                                ("type", Json::str("object")),
                                ("properties", Json::obj([("tool", Json::obj([("type", Json::str("string")), ("enum", Json::arr(cfg.allowed_tools.iter().map(|t| Json::str(t.clone()))))])), ("args", Json::obj([("type", Json::str("object"))]))])),
                                ("required", Json::arr([Json::str("tool"), Json::str("args")])),
                            ]),
                        ),
                    ]),
                ),
            ]),
        ),
        ("required", Json::arr([Json::str("thought"), Json::str("actions")])),
    ])
}

pub struct Mind {
    pub cfg: MindConfig,
    pub model: Box<dyn Model>,
    history: Vec<Message>,
    /// Things to tell the model on the next turn (unusable reply, failed actions).
    notes: Vec<String>,
    last_results: Vec<Json>,
    pub transcript: Vec<Turn>,
    /// The next observation is shown in full (the first one, and the first after a cut).
    fresh: bool,
    /// What was `nearby` in the last observation, to tell the model what has gone.
    known: Vec<(u64, String)>,
    /// Events from this tick on have not been heard yet.
    heard_from: u64,
    /// The model's context window as the server reported it (outer `None`: not asked yet).
    window: Option<Option<usize>>,
    /// What the server counted divided by what we estimated, kept up to date (1.0 until the first answer).
    calibration: f64,
    /// How often the history was cut deep (see `deep_cut`).
    pub cuts: usize,
    /// Long-term memory (see `memory`): set it and every turn is written down and the notes most like the present come back.
    pub memory: Option<Memory>,
}

/// Tokens kept free for the reply when deciding whether the prompt still fits.
const REPLY_RESERVE: usize = 200;

/// Pull the first complete top-level JSON object out of free text (handles code fences and chatter around it).
pub fn extract_json_object(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let start = text.find('{')?;
    let (mut depth, mut in_str, mut esc) = (0usize, false, false);
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

impl Mind {
    pub fn new(cfg: MindConfig, model: Box<dyn Model>) -> Mind {
        Mind { cfg, model, history: Vec::new(), notes: Vec::new(), last_results: Vec::new(), transcript: Vec::new(), fresh: true, known: Vec::new(), heard_from: 0, window: None, calibration: 1.0, cuts: 0, memory: None }
    }

    fn system_prompt(&self) -> String {
        let defs = tool_defs();
        let mut tools = String::new();
        for name in &self.cfg.allowed_tools {
            if let Some(d) = defs.iter().find(|d| d.name == name) {
                let props: Vec<String> = d.input_schema.get("properties").and_then(|p| p.as_object()).map(|p| p.iter().map(|(k, _)| k.clone()).collect()).unwrap_or_default();
                let required: Vec<String> = d.input_schema.get("required").and_then(|r| r.as_array()).map(|r| r.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                tools.push_str(&format!("- {}: {} (arguments: {}; required: {})\n", d.name, d.description, props.join(", "), if required.is_empty() { "none".into() } else { required.join(", ") }));
            }
        }
        for name in &self.cfg.allowed_tools {
            match name.as_str() {
                "go_to" => tools.push_str("- go_to: walk to a place; the engine finds the way around obstacles and time passes until you arrive, are blocked or 15 s are up (arguments: target; required: target). `target` is the name of something (\"Orb 7\"), its uid, or [x, y, z]. The answer says arrived, blocked (with the reason) or still walking. Prefer this to steering by hand.\n"),
                "wait" => tools.push_str("- wait: let time pass (arguments: ticks, 1 to 600; required: ticks).\n"),
                _ => {}
            }
        }
        // Everything that is the same for every mind comes first and the mind's own facts (body, goal) last, so that a server that
        // caches the start of a prompt reads the shared part once.
        format!(
            "You are a mind controlling ONE body in a simulated world.\n\n\
             Each turn you receive an observation as compact JSON (a missing `nearby`, `touching` or `heard` means nothing is there; `heard` lists sounds since your last turn, each with the `offset` from you to where it came from, and `muffled` when a wall is in the way). Reply with ONLY one JSON object, no other text:\n\
             {{\"thought\": \"<one short sentence>\", \"actions\": [ {{\"tool\": \"<name>\", \"args\": {{...}}}} ]}}\n\
             Use at most {max} actions per turn. After your actions the world advances {ticks} ticks ({secs:.2} s) and you see the result.\n\n\
             Tools you may use:\n{tools}\n\
             Rules: tools that change things ({writes}) only work on your own body; you may leave out `uid` and it is filled in. \
             Positions are [x, y, z] in metres with y up. Every thing in `nearby` has an `offset` [dx, dy, dz]: the vector from your body to it. \
             Your first observation lists everything near you; the later ones repeat only the nearest few and name what is `gone` (remember the rest). \
             To walk toward it, point Velocity.lin along the offset's x and z (keep y 0) at 1 to 3 m/s: an offset of [9, 0, 0] means lin [2, 0, 0], an offset of [-3, 0, 4] means lin [-1.2, 0, 1.6]. \
             Velocity.lin is metres per second: to move, set it on your body, e.g. \
             {{\"tool\": \"set\", \"args\": {{\"component\": \"Velocity\", \"value\": {{\"lin\": [1, 0, 0]}}}}}}. \
             {controls}If an action fails the error says why; read it and fix it on your next turn. Keep replies short.\n\n\
             YOUR BODY: the entity with uid {body}.\nYOUR GOAL: {goal}",
            body = self.cfg.body,
            goal = self.cfg.goal,
            max = self.cfg.max_actions,
            ticks = self.cfg.think_ticks,
            secs = self.cfg.think_ticks as f64 / 60.0,
            tools = tools,
            writes = WRITE_TOOLS.join(", "),
            controls = if self.cfg.allowed_tools.iter().any(|t| t == "input") {
                "To play a game, press keys with the `input` tool: {\"tool\": \"input\", \"args\": {\"values\": {\"a\": true, \"d\": false}}}. A key stays pressed until you set it to false. "
            } else {
                ""
            },
        )
    }

    /// What the body heard since the last turn: events other things emitted (newest first), each with the vector from the body to where
    /// it happened, the distance, how long ago, and `muffled` when a wall stops most of the direct sound (traced through the colliders).
    fn hear(&mut self, engine: &mut Engine, pos: &Json) -> Vec<Json> {
        let since = self.heard_from;
        self.heard_from = engine.sim.tick;
        if self.cfg.hearing == 0 {
            return Vec::new();
        }
        let me: Vec<f32> = pos.as_array().map(|p| p.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default();
        if me.len() != 3 {
            return Vec::new();
        }
        let body = self.cfg.body;
        let now = engine.sim.tick;
        let events: Vec<(String, [f32; 3], u64)> = engine.sim.events.iter().rev().filter(|e| e.tick >= since && e.uid != body).filter_map(|e| Some((e.name.clone(), e.pos?, e.tick))).collect();
        let mut out: Vec<Json> = Vec::new();
        let mut said: Vec<(String, [f32; 3])> = Vec::new();
        for (name, p, tick) in events {
            if out.len() >= self.cfg.hearing {
                break;
            }
            let offset = [p[0] - me[0], p[1] - me[1], p[2] - me[2]];
            let distance = (offset[0] * offset[0] + offset[1] * offset[1] + offset[2] * offset[2]).sqrt();
            if distance > HEARING_RANGE {
                continue;
            }
            // the same sound from the same place is one sound
            if said.iter().any(|(n, q)| *n == name && (q[0] - p[0]).abs() + (q[1] - p[1]).abs() + (q[2] - p[2]).abs() < 1.0) {
                continue;
            }
            said.push((name.clone(), p));
            let args = Json::obj([("listener", Json::arr(me.iter().map(|x| Json::from(*x as f64)))), ("source", Json::arr(p.iter().map(|x| Json::from(*x as f64)))), ("rays", Json::from(16)), ("source_rays", Json::from(8))]);
            let muffled = engine.call("acoustics", &args).ok().and_then(|r| r.get("source").and_then(|s| s.get("direct_transmittance")).and_then(|t| t.as_f64())).is_some_and(|t| t < 0.3);
            out.push(Json::obj([
                ("sound", Json::str(name)),
                ("offset", Json::arr(offset.iter().map(|x| Json::from((*x as f64 * 10.0).round() / 10.0)))),
                ("distance", Json::from((distance as f64 * 10.0).round() / 10.0)),
                ("ticks_ago", Json::from(now.saturating_sub(tick) as i64)),
                ("muffled", Json::Bool(muffled)),
            ]));
        }
        out
    }

    pub fn observe(&mut self, engine: &mut Engine) -> Result<Json, String> {
        let body = self.cfg.body;
        let me = engine.call("get", &Json::obj([("uid", Json::from(body as i64))])).map_err(|e| format!("cannot read the body (uid {body}): {e}"))?;
        let pos = me.get("components").and_then(|c| c.get("Transform")).and_then(|t| t.get("pos")).cloned().unwrap_or_else(|| Json::arr([Json::from(0.0), Json::from(0.0), Json::from(0.0)]));
        let near = engine
            .call("nearby", &Json::obj([("point", pos.clone()), ("radius", Json::from(self.cfg.radius as f64)), ("limit", Json::from(self.cfg.nearby_limit + 1))]))
            .map_err(|e| e.to_string())?;
        // each neighbour also gets `offset`: the vector from the body to it. Small models do arithmetic on two absolute positions badly
        // and walk off in the wrong direction; "the Goal is [9, 0, 0] away" they can follow.
        let me_pos: Vec<f64> = pos.as_array().map(|p| p.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect()).unwrap_or_default();
        let others: Vec<Json> = near
            .get("entities")
            .and_then(|e| e.as_array())
            .unwrap_or(&[])
            .iter()
            .filter(|e| e.get("uid").and_then(|u| u.as_u64()) != Some(body))
            .take(self.cfg.nearby_limit)
            .cloned()
            .map(|mut e| {
                let offset = match e.get("pos").and_then(|p| p.as_array()) {
                    Some(p) if me_pos.len() == 3 && p.len() == 3 => Some(Json::Arr((0..3).map(|i| Json::from(p[i].as_f64().unwrap_or(0.0) - me_pos[i])).collect())),
                    _ => None,
                };
                if let Some(o) = offset {
                    e.set("offset", o);
                }
                e
            })
            .collect();
        let heard = self.hear(engine, &pos);
        let contacts = engine.call("contacts", &Json::obj([("limit", Json::from(500))])).map_err(|e| e.to_string())?;
        let mut touching = Vec::new();
        for c in contacts.get("contacts").and_then(|c| c.as_array()).unwrap_or(&[]) {
            let (a, b) = (c.get("a").and_then(|v| v.as_u64()), c.get("b").and_then(|v| v.as_u64()));
            let (other, name, sign) = if a == Some(body) {
                (b, c.get("b_name"), 1.0)
            } else if b == Some(body) {
                (a, c.get("a_name"), -1.0)
            } else {
                continue;
            };
            let n = c.get("normal").and_then(|n| n.as_array()).map(|n| n.iter().map(|x| Json::from(x.as_f64().unwrap_or(0.0) * sign)).collect::<Vec<_>>()).unwrap_or_default();
            let mut o = Json::obj([("uid", Json::from(other.unwrap_or(0) as i64)), ("direction", Json::Arr(n))]);
            if let Some(nm) = name {
                o.set("name", nm.clone());
            }
            touching.push(o);
        }
        // game state a mind should know about (score, timers...), without the raw input state
        let world_state: Vec<(String, Json)> = engine
            .call("resources", &Json::Null)
            .ok()
            .and_then(|r| r.get("resources").and_then(|o| o.as_object().map(|p| p.to_vec())))
            .unwrap_or_default()
            .into_iter()
            .filter(|(k, _)| !k.starts_with("input."))
            .take(24)
            .collect();
        let tick = engine.sim.tick;
        let mut obs = Json::obj([
            ("tick", Json::from(tick as i64)),
            ("seconds", Json::from(tick as f64 * engine.sim.dt as f64)),
            // what the body is made of, without the components that only decide how it is drawn (they cost tokens and mean nothing to a mind)
            ("you", match me.get("components") {
                Some(Json::Obj(c)) => Json::Obj(c.iter().filter(|(k, _)| !VISUAL_ONLY.contains(&k.as_str())).cloned().collect()),
                _ => Json::Null,
            }),
            ("nearby", Json::Arr(others)),
            ("touching", Json::Arr(touching)),
        ]);
        if !heard.is_empty() {
            obs.set("heard", Json::Arr(heard));
        }
        if !world_state.is_empty() {
            obs.set("world", Json::Obj(world_state));
        }
        if self.cfg.minimap > 0 {
            let args = Json::obj([("around", Json::from(body as i64)), ("radius", Json::from(self.cfg.radius as f64)), ("cells", Json::from(self.cfg.minimap as i64))]);
            if let Ok(m) = engine.call("minimap", &args) {
                // only the picture and its key: the entity list repeats `nearby`
                let mut small = Json::obj([("rows", m.get("rows").cloned().unwrap_or(Json::Null)), ("legend", m.get("legend").cloned().unwrap_or(Json::Null))]);
                small.set("how_to_read", Json::str("top view: right is +x, up is -z, `@` is you, one character per cell"));
                obs.set("map", small);
            }
        }
        if !self.last_results.is_empty() {
            obs.set("results_of_your_last_actions", Json::Arr(std::mem::take(&mut self.last_results)));
        }
        if !self.notes.is_empty() {
            obs.set("notes", Json::arr(self.notes.drain(..).map(Json::Str)));
        }
        Ok(obs)
    }

    /// Check an action against the permissions; fills in the body uid for write tools.
    fn permit(&self, tool: &str, args: &mut Json) -> Result<(), String> {
        if !self.cfg.allowed_tools.iter().any(|t| t == tool) {
            return Err(format!("the tool `{tool}` is not allowed for this mind; allowed tools: {}", self.cfg.allowed_tools.join(", ")));
        }
        if WRITE_TOOLS.contains(&tool) {
            match args.get("uid") {
                None => args.set("uid", Json::from(self.cfg.body as i64)),
                Some(u) if u.as_u64() == Some(self.cfg.body) => {}
                Some(_) => return Err(format!("this mind may only change its own body (uid {}), not other entities", self.cfg.body)),
            }
        }
        Ok(())
    }

    /// Our estimate of the prompt (system prompt and history) in tokens: JSON costs about three characters per token, and `calibration`
    /// corrects it with what the server counted for the last call.
    fn estimate_tokens(&self) -> usize {
        (((self.system_prompt().len() + self.history.iter().map(|m| m.content.len()).sum::<usize>()) / 3) as f64 * self.calibration) as usize
    }

    /// Make room before the call, never after a refusal. With a known context window the prompt is allowed to grow until it would pass
    /// three quarters of it (and the reply), and then it is cut once, deep: a summary of what happened, and the last two turns. That
    /// costs the server one cold read of about a thousand tokens, then it only reads what is new for a good while. Without a known window
    /// the history is limited to `cfg.history` messages and the oldest half goes in one step when it is passed.
    fn trim_history(&mut self, context: Option<usize>, incoming_chars: usize) {
        if let Some(ctx) = context {
            let incoming = (incoming_chars as f64 / 3.0 * self.calibration) as usize;
            if self.estimate_tokens() + incoming + REPLY_RESERVE > ctx * 3 / 4 && self.history.len() > 4 {
                self.deep_cut();
            }
            return;
        }
        let cap = self.cfg.history.max(4);
        if self.history.len() >= cap {
            let excess = self.history.len() + 1 - cap / 2;
            self.drop_oldest(excess);
            self.fresh = true;
        }
    }

    /// Keep the last two turns and put a summary of everything older in front of them; the observation that follows is a full one.
    fn deep_cut(&mut self) {
        let summary = self.summary();
        let start = self.history.len().saturating_sub(4);
        self.history.drain(..start);
        while self.history.first().is_some_and(|m| m.role != "user") {
            self.history.remove(0);
        }
        if let Some(first) = self.history.first_mut() {
            first.content = format!("{summary}\n{}", first.content);
        }
        self.fresh = true;
        self.cuts += 1;
    }

    /// What to look the memory up with: the latest thought and the nearest things (or the goal before anything has happened).
    fn situation(&self, obs: &Json) -> String {
        let thought = self.transcript.last().map(|t| t.thought.clone()).unwrap_or_default();
        let near: Vec<String> = Self::names_of(obs).into_iter().take(4).map(|n| n.1).collect();
        if thought.is_empty() && near.is_empty() {
            return self.cfg.goal.clone();
        }
        format!("{thought} near: {}", near.join(", "))
    }

    /// uid and name of everything `nearby` in an observation.
    fn names_of(obs: &Json) -> Vec<(u64, String)> {
        obs.get("nearby").and_then(|n| n.as_array()).unwrap_or(&[]).iter().filter_map(|e| Some((e.get("uid")?.as_u64()?, e.get("name").and_then(|n| n.as_str()).unwrap_or("?").to_string()))).collect()
    }

    /// The short form of an observation: `nearby` cut to the nearest `nearby_repeat` entries, plus `gone`: the names of what was
    /// nearby the last time and is not any more (collected, destroyed or out of view).
    fn delta_observation(&self, obs: &Json) -> Json {
        let mut d = obs.clone();
        let now = Self::names_of(obs);
        let gone: Vec<Json> = self.known.iter().filter(|(u, _)| !now.iter().any(|(n, _)| n == u)).map(|(_, name)| Json::str(name.clone())).collect();
        if let Some(Json::Arr(list)) = d.get("nearby").cloned() {
            if list.len() > self.cfg.nearby_repeat {
                d.set("nearby", Json::Arr(list.into_iter().take(self.cfg.nearby_repeat).collect()));
                d.set("nearby_note", Json::str(format!("only the nearest {} of {} things are repeated", self.cfg.nearby_repeat, now.len())));
            }
        }
        if !gone.is_empty() {
            d.set("gone", Json::Arr(gone));
        }
        d
    }

    /// What to remember of the turns that are about to be forgotten: where the body started and is, the game state, the latest thoughts.
    fn summary(&self) -> String {
        let at = |t: &Turn| t.observation.get("you").and_then(|y| y.get("Transform")).and_then(|t| t.get("pos")).map(observation_text).unwrap_or_else(|| "?".to_string());
        let mut s = format!("[Summary of the {} turns before these. Your goal is still: {}.", self.transcript.len(), self.cfg.goal);
        if let (Some(first), Some(last)) = (self.transcript.first(), self.transcript.last()) {
            s.push_str(&format!(" You started at {} (tick {}) and were at {} at tick {}.", at(first), first.tick, at(last), last.tick));
            if let Some(w) = last.observation.get("world") {
                s.push_str(&format!(" The game then: {}.", observation_text(w)));
            }
            let thoughts: Vec<&str> = self.transcript.iter().rev().take(3).map(|t| t.thought.as_str()).filter(|t| !t.is_empty()).collect();
            if !thoughts.is_empty() {
                s.push_str(&format!(" Your latest thoughts: {}.", thoughts.join(" | ")));
            }
        }
        s.push(']');
        s
    }

    fn drop_oldest(&mut self, n: usize) {
        let n = (n & !1).min(self.history.len().saturating_sub(1));
        self.history.drain(..n);
        while self.history.first().is_some_and(|m| m.role != "user") {
            self.history.remove(0);
        }
    }

    /// One decision: observe, ask the model, run its actions, advance the world. A model that cannot be reached is an `Err`;
    /// an unusable reply is not (it is reported in `Turn::problem` and the model is told next turn).
    pub fn think(&mut self, engine: &mut Engine) -> Result<Turn, String> {
        let observation = self.observe(engine)?;
        let tick = engine.sim.tick;
        // older notes like the present situation (the most recent turns are in the conversation already, so they are left out)
        let mut recalled: Vec<String> = Vec::new();
        if self.memory.is_some() {
            let query = self.situation(&observation);
            let before = self.transcript.len().checked_sub(3).map(|i| self.transcript[i].tick).unwrap_or(0);
            let (k, min) = (self.cfg.memory_k, self.cfg.memory_min_score);
            match self.memory.as_mut().expect("checked").recall(&query, k, before, min) {
                Ok(found) => recalled = found.into_iter().map(|(_, t, text)| format!("tick {t}: {text}")).collect(),
                Err(e) => {
                    self.notes.push(format!("(your memory is unavailable: {e})"));
                    self.memory = None;
                }
            }
        }
        let mut shown = observation.clone();
        if !recalled.is_empty() {
            shown.set("memories", Json::arr(recalled.iter().map(|m| Json::str(m.clone()))));
        }
        // the whole observation, and the short form for turns that follow one the model has seen: only the nearest few things (the
        // rest it remembers) and what has disappeared since
        let full_text = observation_text(&shown);
        let delta = self.delta_observation(&shown);
        let delta_text = observation_text(&delta);
        // the context window: given, or asked of the server once
        let context = if self.cfg.context_tokens > 0 {
            Some(self.cfg.context_tokens)
        } else {
            if self.window.is_none() {
                self.window = Some(self.model.context_tokens());
            }
            self.window.flatten()
        };
        self.trim_history(context, full_text.len());
        let text = if std::mem::take(&mut self.fresh) { full_text } else { delta_text };
        self.known = Self::names_of(&observation);
        self.history.push(Message::user(format!("Observation: {text}\nReply with ONLY the JSON object.")));
        let estimate = self.estimate_tokens();
        let system = Message::system(self.system_prompt());
        let ask = |mind: &mut Mind| {
            let mut messages = vec![system.clone()];
            messages.extend(mind.history.iter().cloned());
            mind.model.complete(&messages)
        };
        let reply = match ask(self) {
            Ok(r) => r,
            // the server says the conversation is too long for its context window: forget the older half and ask again, once
            Err(e) if e.contains("does not fit") && self.history.len() > 2 => {
                let half = self.history.len() / 2;
                self.drop_oldest(half);
                ask(self).map_err(|e2| format!("{e2} (after dropping the older half of the history because: {e})"))?
            }
            Err(e) => return Err(e),
        };
        self.history.push(Message::assistant(reply.clone()));
        // learn how our estimate compares with what the server counted
        if let Some(p) = self.model.last_stats().and_then(|s| s.get("usage").and_then(|u| u.get("prompt_tokens")).and_then(|p| p.as_u64())) {
            if estimate > 0 {
                self.calibration = (self.calibration * p as f64 / estimate as f64).clamp(0.3, 3.0);
            }
        }

        let mut advanced = false;
        let mut turn = Turn { tick, observation, reply: reply.clone(), thought: String::new(), actions: Vec::new(), problem: None, model_stats: self.model.last_stats(), memories: recalled };
        let parsed = extract_json_object(&reply).ok_or_else(|| "no JSON object found in the reply".to_string()).and_then(|t| parse(t).map_err(|e| format!("the JSON is invalid: {e}")));
        match parsed {
            Err(e) => {
                let msg = format!("Your last reply could not be used ({e}). Reply with ONLY one JSON object like {{\"thought\": \"...\", \"actions\": [...]}}.");
                turn.problem = Some(e);
                self.notes.push(msg);
            }
            Ok(j) => {
                turn.thought = j.get("thought").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let list: Vec<Json> = match j.get("actions") {
                    Some(Json::Arr(a)) => a.clone(),
                    _ => Vec::new(),
                };
                for (i, a) in list.iter().enumerate() {
                    if i >= self.cfg.max_actions {
                        self.notes.push(format!("You sent {} actions but at most {} are run per turn; the rest were ignored.", list.len(), self.cfg.max_actions));
                        break;
                    }
                    let Some(tool) = a.get("tool").and_then(|t| t.as_str()) else {
                        let r = Json::str("an action needs a string `tool` and an object `args`");
                        self.last_results.push(Json::obj([("action", Json::from(i)), ("ok", Json::Bool(false)), ("error", r.clone())]));
                        turn.actions.push(ActionResult { tool: String::new(), args: a.clone(), ok: false, result: r });
                        continue;
                    };
                    let mut args = a.get("args").cloned().unwrap_or_else(|| Json::Obj(Vec::new()));
                    let outcome = self.permit(tool, &mut args).and_then(|_| match tool {
                        // verbs the loop runs itself: they take game time, so this turn needs no extra waiting afterwards
                        "go_to" => {
                            advanced = true;
                            self.go_to(engine, &args)
                        }
                        "wait" => {
                            advanced = true;
                            self.wait(engine, &args)
                        }
                        _ => engine.call(tool, &args).map_err(|e| e.to_json().to_string_lossy()),
                    });
                    let (ok, result) = match outcome {
                        Ok(v) => (true, v),
                        Err(msg) => (false, Json::str(msg)),
                    };
                    // a change that worked needs no echo (`{"ok": true, "undoable": true, "uid": 1}` costs the model tokens every turn);
                    // reads show what they found and failures show why
                    let mut note = Json::obj([("tool", Json::str(tool)), ("ok", Json::Bool(ok))]);
                    if !ok {
                        note.set("error", result.clone());
                    } else if !WRITE_TOOLS.contains(&tool) && tool != "input" {
                        note.set("result", result.clone());
                    }
                    self.last_results.push(note);
                    turn.actions.push(ActionResult { tool: tool.to_string(), args, ok, result });
                }
            }
        }
        if let Some(mem) = self.memory.as_mut() {
            // write this turn down
            let actions: Vec<String> = turn
                .actions
                .iter()
                .map(|a| {
                    let args: String = a.args.to_string_lossy().chars().take(60).collect();
                    let outcome = a.result.get("result").and_then(|r| r.as_str()).map(str::to_string).unwrap_or_else(|| if a.ok { "ok".to_string() } else { "failed".to_string() });
                    format!("{}({}) {}", a.tool, args, outcome)
                })
                .collect();
            let note = format!("{} {}", if turn.thought.is_empty() { "(no thought)" } else { &turn.thought }, actions.join("; "));
            if let Err(e) = mem.remember(tick, &note.chars().take(300).collect::<String>()) {
                self.notes.push(format!("(your memory is unavailable: {e})"));
                self.memory = None;
            }
        }
        if !advanced {
            engine.call("step", &Json::obj([("ticks", Json::from(self.cfg.think_ticks as i64))])).map_err(|e| e.to_string())?;
        }
        self.transcript.push(turn.clone());
        Ok(turn)
    }

    fn call_json(engine: &mut Engine, tool: &str, args: Json) -> Result<Json, String> {
        engine.call(tool, &args).map_err(|e| e.to_json().to_string_lossy())
    }

    /// Where a `go_to` target is: [x, y, z], the uid of an entity, or the name of one (the nearest with that name), and what to call it.
    fn resolve_target(&self, engine: &mut Engine, spec: &Json) -> Result<([f64; 3], String), String> {
        let vec3 = |p: &Json| -> Option<[f64; 3]> {
            let a = p.as_array()?;
            (a.len() == 3).then(|| [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0), a[2].as_f64().unwrap_or(0.0)])
        };
        let pos_of = |engine: &mut Engine, uid: u64| -> Option<[f64; 3]> {
            let r = engine.call("get", &Json::obj([("uid", Json::from(uid as i64)), ("components", Json::arr([Json::str("Transform")]))])).ok()?;
            vec3(r.get("components")?.get("Transform")?.get("pos")?)
        };
        if let Some(p) = vec3(spec) {
            return Ok((p, format!("[{}, {}, {}]", p[0], p[1], p[2])));
        }
        let uid = spec.as_u64().or_else(|| spec.as_str().and_then(|s| s.trim().parse().ok()));
        if let Some(uid) = uid {
            return pos_of(engine, uid).map(|p| (p, format!("entity {uid}"))).ok_or_else(|| format!("there is no entity {uid} with a position"));
        }
        let Some(name) = spec.as_str() else { return Err("`target` is the name of something nearby, a uid or [x, y, z]".to_string()) };
        let me = pos_of(engine, self.cfg.body).unwrap_or([0.0; 3]);
        let all = Self::call_json(engine, "query", Json::obj([("with", Json::arr([Json::str("Name"), Json::str("Transform")])), ("select", Json::arr([Json::str("Name"), Json::str("Transform")])), ("limit", Json::from(10_000))]))?;
        let mut best: Option<(f64, [f64; 3], u64)> = None;
        let mut names: Vec<String> = Vec::new();
        for e in all.get("entities").and_then(|e| e.as_array()).unwrap_or(&[]) {
            let uid = e.get("uid").and_then(|u| u.as_u64()).unwrap_or(0);
            let c = e.get("components");
            let (Some(n), Some(p)) = (c.and_then(|c| c.get("Name")).and_then(|n| n.get("value")).and_then(|v| v.as_str()), c.and_then(|c| c.get("Transform")).and_then(|t| t.get("pos")).and_then(vec3)) else { continue };
            if uid == self.cfg.body {
                continue;
            }
            if n.eq_ignore_ascii_case(name.trim()) {
                let d = (p[0] - me[0]).powi(2) + (p[1] - me[1]).powi(2) + (p[2] - me[2]).powi(2);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, p, uid));
                }
            } else if names.len() < 8 {
                names.push(n.to_string());
            }
        }
        best.map(|(_, p, uid)| (p, format!("{name} (entity {uid})"))).ok_or_else(|| format!("nothing is named `{name}`; some names: {}", names.join(", ")))
    }

    /// The `go_to` verb: give the body's `NavAgent` a target and let the world run until it has arrived, cannot get there, or
    /// `go_to_ticks` have passed. One decision per trip instead of one per few steps.
    fn go_to(&mut self, engine: &mut Engine, args: &Json) -> Result<Json, String> {
        let spec = args.get("target").ok_or_else(|| "go_to needs a `target`: the name of something nearby (\"Orb 7\"), its uid, or [x, y, z]".to_string())?;
        let (pos, label) = self.resolve_target(engine, spec)?;
        let body = Json::from(self.cfg.body as i64);
        let nav = |engine: &mut Engine| Self::call_json(engine, "get", Json::obj([("uid", body.clone()), ("components", Json::arr([Json::str("NavAgent")]))]));
        if nav(engine)?.get("components").and_then(|c| c.get("NavAgent")).is_none() {
            Self::call_json(engine, "add_component", Json::obj([("uid", body.clone()), ("component", Json::str("NavAgent")), ("value", Json::Obj(Vec::new()))]))?;
        }
        let target = Json::arr(pos.iter().map(|v| Json::from(*v)));
        Self::call_json(engine, "set", Json::obj([("uid", body.clone()), ("component", Json::str("NavAgent")), ("value", Json::obj([("target", target.clone()), ("active", Json::Bool(true))]))]))?;
        let max = self.cfg.go_to_ticks.max(1);
        let (mut used, mut status, mut left) = (0u64, 1u64, 0.0f64);
        while used < max {
            let chunk = 10u64.min(max - used);
            Self::call_json(engine, "step", Json::obj([("ticks", Json::from(chunk as i64))]))?;
            used += chunk;
            let na = nav(engine)?;
            let n = na.get("components").and_then(|c| c.get("NavAgent"));
            status = n.and_then(|n| n.get("status")).and_then(|s| s.as_u64()).unwrap_or(0);
            left = n.and_then(|n| n.get("remaining")).and_then(|s| s.as_f64()).unwrap_or(0.0);
            if status == 2 || status == 3 {
                break;
            }
        }
        Ok(match status {
            2 => Json::obj([("result", Json::str("arrived")), ("target", Json::str(label)), ("ticks", Json::from(used as i64))]),
            3 => {
                let why = engine.call("path", &Json::obj([("from", body), ("to", target)])).err().map(|e| e.message).unwrap_or_else(|| "something blocked the way".to_string());
                Json::obj([("result", Json::str("blocked")), ("target", Json::str(label)), ("reason", Json::str(why))])
            }
            _ => Json::obj([("result", Json::str("still walking")), ("target", Json::str(label)), ("distance_left", Json::from((left * 10.0).round() / 10.0)), ("hint", Json::str("call go_to again to keep going"))]),
        })
    }

    /// The `wait` verb: let `ticks` ticks pass (1 to 600).
    fn wait(&mut self, engine: &mut Engine, args: &Json) -> Result<Json, String> {
        let ticks = args.get("ticks").and_then(|t| t.as_u64()).ok_or_else(|| "wait needs `ticks`, a whole number from 1 to 600".to_string())?.clamp(1, 600);
        Self::call_json(engine, "step", Json::obj([("ticks", Json::from(ticks as i64))]))?;
        Ok(Json::obj([("waited", Json::from(ticks as i64))]))
    }

    /// Run up to `turns` decisions. Stops at the first model/transport failure and reports it.
    pub fn run(&mut self, engine: &mut Engine, turns: usize) -> Result<(), String> {
        for _ in 0..turns {
            self.think(engine)?;
        }
        Ok(())
    }

    /// Like `run`, but stops as soon as `done` says the goal has been reached. Returns the number of turns used, or `None` if the
    /// goal was not reached in `turns` turns. (`done` is asked before the first turn too.)
    pub fn run_until(&mut self, engine: &mut Engine, turns: usize, mut done: impl FnMut(&mut Engine) -> bool) -> Result<Option<usize>, String> {
        if done(engine) {
            return Ok(Some(0));
        }
        for i in 0..turns {
            self.think(engine)?;
            if done(engine) {
                return Ok(Some(i + 1));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ScriptedModel;

    fn world() -> Engine {
        let dir = std::env::temp_dir().join(format!("lunaris-mind-test-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')', ' '], ""));
        std::fs::create_dir_all(&dir).unwrap();
        let mut e = Engine::new(7, dir);
        let j = |s: &str| parse(s).unwrap();
        e.call("spawn", &j(r#"{"uid":1,"components":{"Name":{"value":"Walker"},"Transform":{"pos":[0,0,0]},"Velocity":{}}}"#)).unwrap();
        e.call("spawn", &j(r#"{"uid":2,"components":{"Name":{"value":"Goal"},"Transform":{"pos":[9,0,0]}}}"#)).unwrap();
        e
    }

    fn mind(replies: &[&str]) -> Mind {
        Mind::new(MindConfig::new(1, "walk to the Goal"), Box::new(ScriptedModel::new(replies.iter().map(|s| s.to_string()).collect())))
    }

    const WALK: &str = r#"{"thought":"go east","actions":[{"tool":"set","args":{"component":"Velocity","value":{"lin":[3,0,0]}}}]}"#;

    fn x(e: &mut Engine, uid: u64) -> f64 {
        e.call("get", &Json::obj([("uid", Json::from(uid as i64)), ("components", Json::arr([Json::str("Transform")]))])).unwrap().get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().index(0).unwrap().as_f64().unwrap()
    }

    #[test]
    fn a_scripted_mind_walks_to_the_goal_and_the_session_replays_without_it() {
        let mut e = world();
        e.call("record_start", &Json::Null).unwrap();
        let mut m = mind(&[WALK]);
        m.cfg.think_ticks = 30;
        m.run(&mut e, 6).unwrap();
        let end = x(&mut e, 1);
        assert!((end - 9.0).abs() < 0.2, "walked to x = {end}");
        assert_eq!(m.transcript.len(), 6);
        // the last observation saw the goal close by
        let last = &m.transcript[5].observation;
        let near = last.get("nearby").unwrap().as_array().unwrap();
        assert_eq!(near[0].get("name").unwrap().as_str(), Some("Goal"));
        assert!(near[0].get("distance").unwrap().as_f64().unwrap() < 3.0);
        e.call("record_stop", &Json::Null).unwrap();
        let r = e.call("replay", &Json::Null).unwrap();
        assert_eq!(r.get("matches"), Some(&Json::Bool(true)), "the mind's decisions are in the recording: {}", r.to_string_lossy());
    }

    #[test]
    fn permissions_stop_a_mind_from_touching_anything_but_its_body() {
        let mut e = world();
        let before = e.call("hash", &Json::Null).unwrap();
        let bad = r#"{"actions":[
            {"tool":"reset","args":{}},
            {"tool":"set","args":{"uid":2,"component":"Transform","value":{"pos":[0,50,0]}}},
            {"tool":"despawn","args":{"uid":2}},
            {"tool":"nope"},
            {"args":{}}
        ]}"#;
        let mut m = mind(&[bad]);
        m.cfg.think_ticks = 0;
        let t = m.think(&mut e).unwrap();
        assert_eq!(t.actions.len(), 5);
        assert!(t.actions.iter().all(|a| !a.ok), "{:?}", t.actions);
        assert!(t.actions[0].result.as_str().unwrap().contains("not allowed"));
        assert!(t.actions[1].result.as_str().unwrap().contains("own body"));
        assert_eq!(e.call("hash", &Json::Null).unwrap(), before, "nothing in the world changed");
        // the failures are reported back on the next turn
        m.think(&mut e).unwrap();
        let seen = m.transcript[1].observation.get("results_of_your_last_actions").unwrap().as_array().unwrap();
        assert_eq!(seen.len(), 5);
    }

    #[test]
    fn an_unusable_reply_is_explained_to_the_model_and_the_mind_recovers() {
        let mut e = world();
        let mut m = Mind::new(MindConfig::new(1, "walk"), Box::new(ScriptedModel::new(vec!["I think I will head east!".into(), WALK.into()])));
        m.cfg.think_ticks = 20;
        let t1 = m.think(&mut e).unwrap();
        assert!(t1.problem.is_some() && t1.actions.is_empty());
        let t2 = m.think(&mut e).unwrap();
        assert!(t2.problem.is_none() && t2.actions[0].ok);
        let notes = t2.observation.get("notes").unwrap().as_array().unwrap();
        assert!(notes[0].as_str().unwrap().contains("could not be used"), "{notes:?}");
        assert!(x(&mut e, 1) > 0.5);
    }

    #[test]
    fn the_prompt_describes_only_the_allowed_tools_and_the_body() {
        let mut e = world();
        let mut cfg = MindConfig::new(1, "survive");
        cfg.allowed_tools = vec!["set".into(), "nearby".into()];
        let mut m = Mind::new(cfg, Box::new(ScriptedModel::new(vec![WALK.into()])));
        m.think(&mut e).unwrap();
        let sys = m.system_prompt();
        assert!(sys.contains("uid 1") && sys.contains("survive") && sys.contains("- set:") && sys.contains("- nearby:"));
        assert!(!sys.contains("- render:") && !sys.contains("- reset:") && !sys.contains("- despawn:"));
        // a tool that exists but was not granted is refused
        let mut m2 = Mind::new(MindConfig { allowed_tools: vec!["nearby".into()], ..MindConfig::new(1, "x") }, Box::new(ScriptedModel::new(vec![WALK.into()])));
        let t = m2.think(&mut e).unwrap();
        assert!(!t.actions[0].ok);
    }

    #[test]
    fn touching_and_action_limits_show_up_in_the_observation() {
        let mut e = world();
        let j = |s: &str| parse(s).unwrap();
        e.call("set", &j(r#"{"uid":2,"component":"Transform","value":{"pos":[0.6,0,0]}}"#)).unwrap();
        e.call("add_component", &j(r#"{"uid":1,"component":"Collider","value":{"shape":0}}"#)).unwrap();
        e.call("add_component", &j(r#"{"uid":2,"component":"Collider","value":{"shape":0}}"#)).unwrap();
        e.call("add_component", &j(r#"{"uid":1,"component":"RigidBody"}"#)).unwrap();
        e.call("spawn", &j(r#"{"components":{"PhysicsSettings":{"gravity":[0,0,0]}}}"#)).unwrap();
        e.call("step", &j(r#"{"ticks":2}"#)).unwrap();
        let mut m = mind(&[r#"{"actions":[{"tool":"get","args":{"uid":2}},{"tool":"get","args":{"uid":2}},{"tool":"get","args":{"uid":2}}]}"#]);
        m.cfg.max_actions = 2;
        let t = m.think(&mut e).unwrap();
        let touching = t.observation.get("touching").unwrap().as_array().unwrap();
        assert_eq!(touching[0].get("name").unwrap().as_str(), Some("Goal"));
        assert_eq!(t.actions.len(), 2, "only max_actions run");
        let t2 = m.think(&mut e).unwrap();
        assert!(t2.observation.get("notes").unwrap().to_string_lossy().contains("at most 2"));
    }

    #[test]
    fn json_is_found_inside_chatter_and_code_fences() {
        assert_eq!(extract_json_object("Sure!\n```json\n{\"a\": {\"b\": \"}\"}}\n```\nbye"), Some("{\"a\": {\"b\": \"}\"}}"));
        assert_eq!(extract_json_object("no braces"), None);
        assert_eq!(extract_json_object("{\"a\": \"unterminated"), None);
        assert_eq!(extract_json_object("{\"s\": \"quote \\\" and { brace\"}"), Some("{\"s\": \"quote \\\" and { brace\"}"));
    }

    #[test]
    fn history_is_bounded() {
        let mut e = world();
        let mut m = mind(&[WALK]);
        m.cfg.history = 16;
        m.cfg.think_ticks = 1;
        let mut starts = Vec::new();
        for _ in 0..24 {
            m.think(&mut e).unwrap();
            assert!(m.history.len() <= 17, "history {}", m.history.len());
            assert_eq!(m.history[0].role, "user");
            starts.push(m.history[0].content.clone());
        }
        // the start of the conversation changes only now and then (half the history at a time), never every turn: that is what lets a
        // model server reuse the part of the prompt it has already read
        starts.dedup();
        assert!(starts.len() <= 9, "the history was cut from the front on {} of 24 turns", starts.len() - 1);
    }

    #[test]
    fn go_to_walks_around_a_wall_in_one_decision_and_reports_arrival_blocks_and_unknown_names() {
        let mut e = world();
        let j = |s: &str| parse(s).unwrap();
        e.call("spawn", &j(r#"{"uid":50,"components":{"Name":{"value":"Floor"},"Transform":{"pos":[0,-0.5,0],"scale":[100,1,100]},"Collider":{"shape":1}}}"#)).unwrap();
        e.call("spawn", &j(r#"{"uid":51,"components":{"Name":{"value":"Wall"},"Transform":{"pos":[5,1.5,0],"scale":[1,3,10]},"Collider":{"shape":1}}}"#)).unwrap();
        e.call("spawn", &j(r#"{"uid":52,"components":{"Name":{"value":"Island"},"Transform":{"pos":[0,3,20]}}}"#)).unwrap();
        e.call("spawn", &j(r#"{"uid":53,"components":{"Name":{"value":"Tower"},"Transform":{"pos":[0,6,-8],"scale":[4,12,4]},"Collider":{"shape":1}}}"#)).unwrap();
        let reply = |args: &str| format!(r#"{{"thought":"go","actions":[{{"tool":"go_to","args":{args}}}]}}"#);
        let replies = [reply(r#"{"target":"goal"}"#), reply(r#"{"target":"Nowhere"}"#), reply(r#"{"target":[0,12,-8]}"#), r#"{"thought":"rest","actions":[{"tool":"wait","args":{"ticks":30}}]}"#.to_string()];
        let mut m = Mind::new(MindConfig::new(1, "go places"), Box::new(ScriptedModel::new(replies.to_vec())));
        m.cfg.allowed_tools = ["go_to", "wait", "get"].iter().map(|s| s.to_string()).collect();
        m.cfg.think_ticks = 60;
        // 1: around the wall to the Goal (nine metres away behind it), by name, case-insensitive
        let t = m.think(&mut e).unwrap();
        assert!(t.actions[0].ok, "{:?}", t.actions[0]);
        assert_eq!(t.actions[0].result.get("result").and_then(|r| r.as_str()), Some("arrived"), "{}", t.actions[0].result.to_string_lossy());
        assert!((x(&mut e, 1) - 9.0).abs() < 0.5, "at x = {}", x(&mut e, 1));
        assert!(e.sim.tick > 100 && e.sim.tick < 900, "the trip took {} ticks, not a fixed number", e.sim.tick);
        // 2: a name that is not in the world is an error that lists some that are
        let t = m.think(&mut e).unwrap();
        assert!(!t.actions[0].ok && t.actions[0].result.to_string_lossy().contains("nothing is named `Nowhere`"), "{:?}", t.actions[0]);
        // 3: a point on top of a 12 m tower cannot be reached: blocked, with the reason
        let t = m.think(&mut e).unwrap();
        assert_eq!(t.actions[0].result.get("result").and_then(|r| r.as_str()), Some("blocked"), "{}", t.actions[0].result.to_string_lossy());
        assert!(t.actions[0].result.get("reason").and_then(|r| r.as_str()).is_some_and(|r| r.contains("no walkable route") || r.contains("not near")), "{}", t.actions[0].result.to_string_lossy());
        // 4: waiting
        let before = e.sim.tick;
        let t = m.think(&mut e).unwrap();
        assert!(t.actions[0].ok && e.sim.tick == before + 30, "waited exactly 30 ticks (and no think_ticks on top)");
        // a mind that was not given the verbs cannot use them
        let mut denied = Mind::new(MindConfig { allowed_tools: vec!["get".into()], ..MindConfig::new(1, "x") }, Box::new(ScriptedModel::new(vec![reply(r#"{"target":"Goal"}"#)])));
        assert!(!denied.think(&mut e).unwrap().actions[0].ok);
    }

    #[test]
    fn a_memory_brings_back_older_notes_that_match_the_situation_and_survives_a_dead_server() {
        use crate::memory::tests::WordsEmbedder;
        let mut e = world();
        let thoughts = ["the red key is hidden behind the waterfall", "walking along the river", "resting by the fire", "counting my supplies", "looking for the red key near the waterfall", "nothing new"];
        let replies: Vec<String> = thoughts.iter().map(|t| format!(r#"{{"thought":"{t}","actions":[]}}"#)).collect();
        let mut m = Mind::new(MindConfig::new(1, "find things"), Box::new(ScriptedModel::new(replies)));
        m.cfg.think_ticks = 1;
        m.cfg.memory_min_score = 0.2;
        m.memory = Some(Memory::new(Box::new(WordsEmbedder)));
        for _ in 0..6 {
            m.think(&mut e).unwrap();
        }
        let last = m.transcript.last().unwrap();
        assert!(last.memories.iter().any(|t| t.contains("hidden behind the waterfall")), "the older note about the key came back: {:?}", last.memories);
        assert!(last.memories.iter().all(|t| !t.contains("nothing new")), "the newest turns are not repeated");
        assert!(m.history[m.history.len() - 2].content.contains("memories"), "and the model was shown it");
        assert!(last.to_json().get("memories").is_some());
        // an embedding server that fails switches the memory off with a note instead of stopping the run
        struct Dead;
        impl crate::memory::Embedder for Dead {
            fn embed(&mut self, _: &[String], _: bool) -> Result<Vec<Vec<f32>>, String> {
                Err("connection refused".into())
            }
        }
        let mut m2 = mind(&[WALK]);
        m2.cfg.think_ticks = 1;
        m2.memory = Some(Memory::new(Box::new(Dead)));
        for _ in 0..3 {
            m2.think(&mut e).unwrap();
        }
        assert!(m2.memory.is_none(), "switched off");
        assert!(m2.transcript.iter().any(|t| t.observation.get("notes").is_some_and(|n| n.to_string_lossy().contains("memory is unavailable"))), "and the model was told once");
    }

    #[test]
    fn a_known_context_window_means_rare_deep_cuts_with_a_summary() {
        let mut e = world();
        let mut m = mind(&[WALK]);
        m.cfg.think_ticks = 1;
        m.cfg.context_tokens = 3000; // the system prompt alone is about 900 tokens
        let mut fronts: Vec<String> = Vec::new();
        for _ in 0..40 {
            m.think(&mut e).unwrap();
            assert!(m.estimate_tokens() <= 3000, "{} tokens in a window of 3000 ({} messages)", m.estimate_tokens(), m.history.len());
            fronts.push(m.history[0].content.clone());
        }
        // the start of the conversation changes only at a cut, never in between, and a cut leaves a summary there
        let changes = fronts.windows(2).filter(|w| w[0] != w[1]).count();
        assert_eq!(changes, m.cuts, "the front moved {changes} times and the history was cut {} times", m.cuts);
        assert!(m.cuts >= 3 && m.cuts < 20, "{} cuts in 40 turns", m.cuts);
        assert!(m.history[0].content.starts_with("[Summary of the"), "{}", m.history[0].content);
        assert!(m.history[0].content.contains("walk to the Goal") || m.history[0].content.contains("Your goal is still"), "{}", m.history[0].content);
        assert_eq!(m.history[0].role, "user");
    }

    #[test]
    fn after_the_first_observation_only_the_nearest_things_are_repeated_and_what_vanished_is_named() {
        let mut e = world();
        for i in 0..8 {
            e.call("spawn", &parse(&format!(r#"{{"uid":{},"components":{{"Name":{{"value":"Orb {}"}},"Transform":{{"pos":[{},0,0]}}}}}}"#, 10 + i, i, 1 + i)).unwrap()).unwrap();
        }
        let mut m = mind(&[WALK]);
        m.cfg.think_ticks = 1;
        m.cfg.nearby_limit = 20;
        m.cfg.nearby_repeat = 3;
        m.think(&mut e).unwrap();
        e.call("despawn", &parse(r#"{"uid":10}"#).unwrap()).unwrap();
        m.think(&mut e).unwrap();
        let (first, second) = (&m.history[0].content, &m.history[2].content);
        assert!(first.contains("Orb 7") && first.contains("Goal"), "the first observation lists everything: {first}");
        assert!(!second.contains("Orb 7") && second.contains("only the nearest 3 of"), "the second repeats only the nearest few: {second}");
        assert!(second.contains(r#""gone":["Orb 0"]"#), "and names what disappeared: {second}");
        // the transcript keeps the full observation either way
        assert!(m.transcript[1].observation.get("nearby").unwrap().as_array().unwrap().len() >= 8);
    }

    #[test]
    fn the_prompt_starts_the_same_for_every_mind_and_the_observation_is_compact() {
        let mut e = world();
        let a = Mind::new(MindConfig::new(1, "walk to the Goal"), Box::new(ScriptedModel::new(vec![WALK.into()])));
        let b = Mind::new(MindConfig::new(2, "stay home"), Box::new(ScriptedModel::new(vec![WALK.into()])));
        let (pa, pb) = (a.system_prompt(), b.system_prompt());
        let common = pa.chars().zip(pb.chars()).take_while(|(x, y)| x == y).count();
        assert!(common * 10 >= pa.len() * 9, "the shared start is {common} of {} characters", pa.len());
        assert!(pa.ends_with("walk to the Goal") && pb.ends_with("stay home"));
        // one line, two decimals, no empty lists
        let mut m = mind(&[WALK]);
        let obs = m.observe(&mut e).unwrap();
        let text = observation_text(&obs);
        assert!(!text.contains('\n') && !text.contains("\"touching\":[]"), "{text}");
        assert_eq!(observation_text(&parse(r#"{"v":[1.23456, 2.0, 0.004],"nearby":[],"you":{"A":{}}}"#).unwrap()), r#"{"v":[1.23,2,0],"you":{"A":{}}}"#);
        // the reply schema names exactly the allowed tools
        let schema = reply_schema(&m.cfg);
        let tools = schema.get("properties").and_then(|p| p.get("actions")).and_then(|a| a.get("items")).and_then(|i| i.get("properties")).and_then(|p| p.get("tool")).and_then(|t| t.get("enum")).and_then(|e| e.as_array()).unwrap();
        assert_eq!(tools.len(), m.cfg.allowed_tools.len());
    }

    struct Overflow {
        calls: usize,
        sizes: Vec<usize>,
    }
    impl Model for Overflow {
        fn name(&self) -> String {
            "overflow".into()
        }
        fn complete(&mut self, messages: &[Message]) -> Result<String, String> {
            self.calls += 1;
            self.sizes.push(messages.len());
            if self.calls == 5 { Err("the model server answered HTTP 400: n_ctx exceeded (the conversation does not fit the model's context window)".into()) } else { Ok(WALK.into()) }
        }
    }

    #[test]
    fn a_context_overflow_drops_the_older_half_and_asks_again() {
        let mut e = world();
        let mut m = Mind::new(MindConfig::new(1, "walk"), Box::new(Overflow { calls: 0, sizes: Vec::new() }));
        m.cfg.think_ticks = 1;
        m.cfg.history = 40;
        for _ in 0..6 {
            m.think(&mut e).unwrap();
        }
        // turn 5 hit the overflow: the retry was made with fewer messages than the failed call
        assert_eq!(m.transcript.len(), 6);
    }

    #[test]
    fn a_mind_hears_what_others_emit_through_walls_but_not_its_own_sounds() {
        let mut e = world();
        let j = |s: &str| parse(s).unwrap();
        // a thin wall at x = 3 between the walker and a bell at x = 6 (and the Goal at x = 9), a gong on the open side at x = -4
        e.call("spawn", &j(r#"{"components":{"Transform":{"pos":[3,0,0]},"Collider":{"shape":1,"size":[0.2,4,10]}}}"#)).unwrap();
        e.call("spawn", &j(r#"{"uid":10,"components":{"Name":{"value":"Bell"},"Transform":{"pos":[6,0,0]}}}"#)).unwrap();
        e.call("spawn", &j(r#"{"uid":11,"components":{"Name":{"value":"Gong"},"Transform":{"pos":[-4,0,0]}}}"#)).unwrap();
        // everything with a Name and a Transform rings once, at tick 2: that includes the walker itself
        e.call("define_system", &j(r#"{"name":"ring","each":["Name","Transform"],"do":[{"if":{"eq":[{"tick":true},2]},"then":[{"emit":"ding"}]}]}"#)).unwrap();
        let mut m = mind(&[WALK]);
        m.cfg.think_ticks = 6;
        m.think(&mut e).unwrap();
        assert!(m.transcript[0].observation.get("heard").is_none(), "nothing had happened before the first turn");
        m.think(&mut e).unwrap();
        let heard: Vec<Json> = m.transcript[1].observation.get("heard").expect("sounds since the last turn").as_array().unwrap().to_vec();
        assert_eq!(heard.len(), 3, "the bell, the gong and the Goal, not the walker's own ding: {heard:?}");
        let dx = |h: &Json| h.get("offset").unwrap().index(0).unwrap().as_f64().unwrap();
        let at = |want: f64| heard.iter().find(|h| (dx(h) - want).abs() < 1.5).unwrap_or_else(|| panic!("nothing heard near dx {want}: {heard:?}"));
        assert_eq!(at(-4.3).get("muffled"), Some(&Json::Bool(false)), "the gong has a clear way");
        assert_eq!(at(5.7).get("muffled"), Some(&Json::Bool(true)), "the bell is behind the wall");
        assert_eq!(at(8.7).get("muffled"), Some(&Json::Bool(true)), "so is the Goal");
        for h in &heard {
            assert_eq!(h.get("sound").unwrap().as_str(), Some("ding"));
            assert!(h.get("distance").unwrap().as_f64().unwrap() > 3.0 && h.get("ticks_ago").unwrap().as_u64().unwrap() <= 6);
        }
        // the sound is heard once, not again on the next turn
        m.think(&mut e).unwrap();
        assert!(m.transcript[2].observation.get("heard").is_none());
        // a deaf mind hears nothing
        let mut e2 = world();
        e2.call("define_system", &j(r#"{"name":"ring","each":["Name","Transform"],"do":[{"if":{"eq":[{"tick":true},2]},"then":[{"emit":"ding"}]}]}"#)).unwrap();
        let mut deaf = mind(&[WALK]);
        deaf.cfg.hearing = 0;
        deaf.cfg.think_ticks = 6;
        deaf.think(&mut e2).unwrap();
        deaf.think(&mut e2).unwrap();
        assert!(deaf.transcript[1].observation.get("heard").is_none());
    }
}
