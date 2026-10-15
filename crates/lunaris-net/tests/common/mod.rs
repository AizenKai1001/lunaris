//! What the multiplayer tests share: a three-player scene, bots that play a script of keys, hosts on free ports, and the two ways to know what
//! a game should end as: replaying the scripted keys offline, and replaying the host's log.
#![allow(dead_code)]
use lunaris_api::Engine;
use lunaris_json::{parse, Json};
use lunaris_net::{connect, Host, HostConfig, HostReport, JoinOptions, Mode, RollbackStats, Seat};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Three heroes, each driven by its own player's keys (`input.<key>`, `input.p2.<key>`, `input.p3.<key>`): d/a move in x, s/w in z, and
/// every tick the space key is held adds one to the player's own counter, so the state depends on *when* keys go down and up, not only on
/// where the heroes end.
pub const SCENE: &str = r#"{
 "lunaris": 1,
 "components": [{"name": "P1", "fields": []}, {"name": "P2", "fields": []}, {"name": "P3", "fields": []}],
 "resources": {"held1": 0, "held2": 0, "held3": 0},
 "entities": [
  {"uid": 1, "components": {"Name": {"value": "One"}, "Transform": {"pos": [0, 0, 0]}, "Velocity": {"lin": [0, 0, 0]}, "P1": {}}},
  {"uid": 2, "components": {"Name": {"value": "Two"}, "Transform": {"pos": [0, 0, 5]}, "Velocity": {"lin": [0, 0, 0]}, "P2": {}}},
  {"uid": 3, "components": {"Name": {"value": "Three"}, "Transform": {"pos": [0, 0, 10]}, "Velocity": {"lin": [0, 0, 0]}, "P3": {}}}
 ],
 "systems": [
  {"name": "move_one", "each": ["P1", "Velocity"], "do": [
    {"set": "Velocity.lin[0]", "to": {"mul": [6, {"sub": [{"res": "input.d"}, {"res": "input.a"}]}]}},
    {"set": "Velocity.lin[2]", "to": {"mul": [6, {"sub": [{"res": "input.s"}, {"res": "input.w"}]}]}},
    {"if": {"gt": [{"res": "input.space"}, 0.5]}, "then": [{"res_add": "held1", "by": 1}]}]},
  {"name": "move_two", "each": ["P2", "Velocity"], "do": [
    {"set": "Velocity.lin[0]", "to": {"mul": [6, {"sub": [{"res": "input.p2.d"}, {"res": "input.p2.a"}]}]}},
    {"set": "Velocity.lin[2]", "to": {"mul": [6, {"sub": [{"res": "input.p2.s"}, {"res": "input.p2.w"}]}]}},
    {"if": {"gt": [{"res": "input.p2.space"}, 0.5]}, "then": [{"res_add": "held2", "by": 1}]}]},
  {"name": "move_three", "each": ["P3", "Velocity"], "do": [
    {"set": "Velocity.lin[0]", "to": {"mul": [6, {"sub": [{"res": "input.p3.d"}, {"res": "input.p3.a"}]}]}},
    {"set": "Velocity.lin[2]", "to": {"mul": [6, {"sub": [{"res": "input.p3.s"}, {"res": "input.p3.w"}]}]}},
    {"if": {"gt": [{"res": "input.p3.space"}, 0.5]}, "then": [{"res_add": "held3", "by": 1}]}]}
 ]
}"#;

pub fn engine_for(seed: u64, scene: &str) -> Result<Engine, String> {
    let mut e = Engine::new(seed, ".");
    e.call("load_scene", &Json::obj([("scene", parse(scene).map_err(|e| e.to_string())?)])).map_err(|e| e.to_string())?;
    Ok(e)
}

pub fn hash_of(e: &Engine) -> String {
    format!("{:016x}", e.sim.state_hash())
}

/// What a player does: the keys to set when its own simulation is at this tick (setting a key to the value it has is free).
pub type Script = fn(usize, u64) -> Vec<(&'static str, f64)>;

/// Nobody presses anything.
pub fn calm(_player: usize, _tick: u64) -> Vec<(&'static str, f64)> {
    Vec::new()
}

/// Different keys at different ticks for each player, so that every player's changes arrive at everybody else's copy in the past.
pub fn mixed(player: usize, tick: u64) -> Vec<(&'static str, f64)> {
    let on = |from: u64, to: u64| if tick >= to { Some(0.0) } else if tick >= from { Some(1.0) } else { None };
    let mut out = Vec::new();
    let mut put = |k: &'static str, v: Option<f64>| {
        if let Some(v) = v {
            out.push((k, v));
        }
    };
    match player {
        0 => {
            put("d", on(10, 70));
            put("space", on(25, 40));
            put("s", on(90, 110));
        }
        1 => {
            put("a", on(20, 60));
            put("w", on(35, 80));
            put("space", on(50, 95));
        }
        _ => {
            put("s", on(5, 30));
            put("d", on(40, 100));
            put("space", on(33, 36));
            put("a", on(105, 125));
        }
    }
    out
}

/// A longer game for the tests that need time to pass: every player changes a key every few ticks, at a different rhythm.
pub fn busy(player: usize, tick: u64) -> Vec<(&'static str, f64)> {
    let key = ["a", "d", "w", "s"][(tick as usize / (7 + player * 3)) % 4];
    ["a", "d", "w", "s"].iter().map(|k| (*k, if *k == key { 1.0 } else { 0.0 })).collect()
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub hash: String,
    pub tick: u64,
    pub player: usize,
    pub stats: Option<RollbackStats>,
    pub desync: Option<String>,
    pub finished: Option<String>,
    pub events: Vec<String>,
    pub killed: bool,
    pub resources: BTreeMap<String, f64>,
    /// How many players the game had, as the host told this copy when it started.
    pub players: usize,
}

/// How a bot plays.
#[derive(Clone)]
pub struct Hooks {
    pub script: Script,
    /// Hang up (no goodbye, like a crash) when the simulation reaches this tick.
    pub kill_at: Option<u64>,
    /// Corrupt this copy of the game when it reaches this tick (the seed changes: nothing a rollback can undo).
    pub tamper_at: Option<u64>,
    /// Where to publish the tick this bot has reached.
    pub progress: Option<Arc<AtomicU64>>,
    /// Sleep this long after every tick that was played (to make a game last long enough for something to happen to it).
    pub pace: Duration,
    pub max_wall: Duration,
    /// The most ticks per call of `advance`; 1 makes a key chosen at tick t reach the game at exactly tick t + delay.
    pub steps: usize,
}

impl Hooks {
    pub fn new(script: Script) -> Hooks {
        Hooks { script, kill_at: None, tamper_at: None, progress: None, pace: Duration::ZERO, max_wall: Duration::from_secs(90), steps: 1 }
    }
}

pub fn drive(seat: &mut Seat, engine: &mut Engine, h: &Hooks) -> Result<Outcome, String> {
    let me = seat.welcome().player;
    let started = Instant::now();
    let mut events = Vec::new();
    let mut desync: Option<String> = None;
    let mut tampered = false;
    let result = |seat: &mut Seat, engine: &mut Engine, events: Vec<String>, desync: Option<String>, killed: bool| {
        let resources = engine.call("resources", &Json::Null).ok().and_then(|r| r.get("resources").and_then(|r| r.as_object().map(|o| o.iter().filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f))).collect()))).unwrap_or_default();
        Outcome { hash: hash_of(engine), tick: engine.sim.tick, player: me, stats: seat.rollback_stats(), desync, finished: seat.finished().map(str::to_string), events, killed, resources, players: seat.welcome().players }
    };
    loop {
        if h.kill_at.is_some_and(|k| engine.sim.tick >= k) {
            return Ok(result(seat, engine, events, desync, true));
        }
        for (k, v) in (h.script)(me, engine.sim.tick) {
            seat.set_key(k, v);
        }
        if !tampered && h.tamper_at.is_some_and(|t| engine.sim.tick >= t) {
            tampered = true;
            engine.sim.seed ^= 0x5555;
        }
        let stepped = seat.advance(engine, h.steps)?;
        events.extend(seat.take_events());
        if let Some(d) = seat.take_desync() {
            desync.get_or_insert(d);
        }
        if let Some(p) = &h.progress {
            p.store(engine.sim.tick, Ordering::SeqCst);
        }
        if stepped == 0 {
            if seat.finished().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        } else if !h.pace.is_zero() {
            std::thread::sleep(h.pace);
        }
        if started.elapsed() > h.max_wall {
            return Err(format!("the game did not finish within {:?} (tick {})", h.max_wall, engine.sim.tick));
        }
    }
    if let Some(p) = &h.progress {
        p.store(engine.sim.tick, Ordering::SeqCst);
    }
    Ok(result(seat, engine, events, desync, false))
}

/// A player: joins, plays its script until the game is over (or hangs up when told to).
#[derive(Clone)]
pub struct BotSpec {
    pub name: &'static str,
    pub mode: Mode,
    pub delay: Option<u64>,
    pub watch: bool,
    pub token: Option<String>,
    pub scene: &'static str,
    pub hooks: Hooks,
    /// Where to send the seat token as soon as there is one.
    pub token_tx: Option<mpsc::Sender<String>>,
}

impl BotSpec {
    pub fn new(name: &'static str, mode: Mode, script: Script) -> BotSpec {
        BotSpec { name, mode, delay: None, watch: false, token: None, scene: SCENE, hooks: Hooks::new(script), token_tx: None }
    }
}

pub fn spawn_bot(addr: String, spec: BotSpec) -> JoinHandle<Result<Outcome, String>> {
    std::thread::spawn(move || {
        let mut o = JoinOptions::new(spec.name, spec.mode);
        o.input_delay = spec.delay;
        o.watch = spec.watch;
        o.token = spec.token.clone();
        o.hash_every = 10;
        let scene = spec.scene;
        let (mut seat, mut engine) = connect(&addr, o, |w| engine_for(w.seed, scene))?;
        if let (Some(tx), Some(t)) = (&spec.token_tx, seat.token()) {
            let _ = tx.send(t.to_string());
        }
        drive(&mut seat, &mut engine, &spec.hooks)
    })
}

/// A host on a free port of this machine; the report comes back through the handle.
pub fn spawn_host(cfg: HostConfig) -> (String, JoinHandle<Result<HostReport, String>>) {
    let h = Host::bind("127.0.0.1:0").expect("bind");
    let addr = format!("127.0.0.1:{}", h.port());
    (addr, std::thread::spawn(move || h.run(&cfg)))
}

/// A rollback host for `players` players that ends the game at tick `ticks`.
pub fn rb_cfg(players: usize, ticks: u64) -> HostConfig {
    HostConfig {
        players,
        mode: Mode::Rollback,
        delay: 2,
        window: 8,
        max_ticks: Some(ticks),
        join_timeout: Duration::from_secs(30),
        timeout: Duration::from_secs(30),
        grace: Duration::from_millis(300),
        ..HostConfig::default()
    }
}

/// What the game must end as, worked out without any network: the scripts' keys, applied at the tick they take effect (`delay` ticks after
/// they were chosen), every player's changes in player order.
pub fn scripted_hash(scripts: &[Script], ticks: u64, delay: u64, scene: &str) -> String {
    let mut e = engine_for(1, scene).expect("scene");
    let mut held: Vec<BTreeMap<&str, f64>> = vec![BTreeMap::new(); scripts.len()];
    for tick in 0..ticks {
        for (player, h) in held.iter_mut().enumerate() {
            if tick >= delay {
                let mut changes: Vec<(String, Json)> = Vec::new();
                for (k, v) in scripts[player](player, tick - delay) {
                    if h.get(k) != Some(&v) {
                        h.insert(k, v);
                        changes.push((lunaris_net::key_name(player, k), Json::from(v)));
                    }
                }
                if !changes.is_empty() {
                    e.call("input", &Json::obj([("values", Json::Obj(changes))])).expect("input");
                }
            }
        }
        e.sim.step();
    }
    hash_of(&e)
}

/// What the game must end as according to the host's confirmed log of key changes.
pub fn log_hash(report: &HostReport, ticks: u64, scene: &str) -> String {
    let mut e = engine_for(1, scene).expect("scene");
    report.log.replay(&mut e, ticks).expect("replay");
    hash_of(&e)
}

/// Wait until `cond` holds; panics after `limit`.
pub fn wait_until(what: &str, limit: Duration, mut cond: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !cond() {
        assert!(t0.elapsed() < limit, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}
