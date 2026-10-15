//! Lockstep over real TCP sockets on this machine: every player's copy of the game must stay bit-identical, a copy that drifts must be
//! found, and a host must refuse players whose worlds differ.
use lunaris_api::Engine;
use lunaris_json::{parse, Json};
use lunaris_net::{Host, HostConfig, HostReport, Lockstep};
use std::thread::JoinHandle;
use std::time::Duration;

/// Two heroes, each driven by its own player's keys: the first by `input.a`/`input.d`, the second by `input.p2.a`/`input.p2.d`.
const SCENE: &str = r#"{
 "lunaris": 1,
 "components": [{"name": "P1", "fields": []}, {"name": "P2", "fields": []}],
 "entities": [
  {"uid": 1, "components": {"Name": {"value": "One"}, "Transform": {"pos": [0, 0, 0]}, "Velocity": {"lin": [0, 0, 0]}, "P1": {}}},
  {"uid": 2, "components": {"Name": {"value": "Two"}, "Transform": {"pos": [0, 0, 5]}, "Velocity": {"lin": [0, 0, 0]}, "P2": {}}}
 ],
 "systems": [
  {"name": "move_one", "each": ["P1", "Velocity"], "do": [{"set": "Velocity.lin[0]", "to": {"mul": [6, {"sub": [{"res": "input.d"}, {"res": "input.a"}]}]}}]},
  {"name": "move_two", "each": ["P2", "Velocity"], "do": [{"set": "Velocity.lin[0]", "to": {"mul": [6, {"sub": [{"res": "input.p2.d"}, {"res": "input.p2.a"}]}]}}]}
 ]
}"#;

fn engine_for(seed: u64, scene: &str) -> Result<Engine, String> {
    let mut e = Engine::new(seed, ".");
    e.call("load_scene", &Json::obj([("scene", parse(scene).map_err(|e| e.to_string())?)])).map_err(|e| e.to_string())?;
    Ok(e)
}

struct Outcome {
    hash: String,
    tick: u64,
    xs: [f64; 2],
    desync: Option<String>,
    finished: Option<String>,
}

fn x_of(e: &mut Engine, uid: u64) -> f64 {
    let v = e.call("get", &Json::obj([("uid", Json::from(uid as i64)), ("components", Json::arr([Json::str("Transform")]))])).expect("get");
    v.get("components").and_then(|c| c.get("Transform")).and_then(|t| t.get("pos")).and_then(|p| p.index(0)).and_then(|x| x.as_f64()).expect("x")
}

/// Play one seat: hold the keys `script` says for every tick, until the host ends the game.
fn play(addr: String, scene: &'static str, script: fn(usize, u64) -> Vec<(&'static str, f64)>, meddle_at: Option<u64>) -> JoinHandle<Outcome> {
    std::thread::spawn(move || {
        let (mut ls, mut engine) = Lockstep::join(&addr, "bot", |w| engine_for(w.seed, scene)).expect("join");
        ls.hash_every = 10;
        let me = ls.welcome.player;
        let started = std::time::Instant::now();
        loop {
            for (k, v) in script(me, engine.sim.tick) {
                ls.set_key(k, v);
            }
            if meddle_at.is_some_and(|t| engine.sim.tick == t) {
                // a "cheat": this copy of the world no longer matches the others
                engine.call("set_resource", &Json::obj([("name", Json::str("cheat")), ("value", Json::from(1))])).expect("meddle");
            }
            // one tick per call, so that a key chosen at tick t reaches the game at exactly tick t + delay
            let stepped = ls.advance(&mut engine, 1).expect("advance");
            if stepped == 0 {
                if ls.finished.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(started.elapsed() < Duration::from_secs(60), "the game did not finish");
        }
        Outcome { hash: format!("{:016x}", engine.sim.state_hash()), tick: engine.sim.tick, xs: [x_of(&mut engine, 1), x_of(&mut engine, 2)], desync: ls.desync.clone(), finished: ls.finished.clone() }
    })
}

/// The first player holds D from tick 10 on; the second holds A from tick 20 on and lets go at 60.
fn keys(player: usize, tick: u64) -> Vec<(&'static str, f64)> {
    match player {
        0 if tick >= 10 => vec![("d", 1.0)],
        1 if tick >= 60 => vec![("a", 0.0)],
        1 if tick >= 20 => vec![("a", 1.0)],
        _ => vec![],
    }
}

fn host(players: usize, ticks: u64) -> (String, JoinHandle<Result<HostReport, String>>) {
    let h = Host::bind("127.0.0.1:0").expect("bind");
    let addr = format!("127.0.0.1:{}", h.port());
    let cfg = HostConfig { players, max_ticks: Some(ticks), join_timeout: Duration::from_secs(20), timeout: Duration::from_secs(20), ..HostConfig::default() };
    (addr, std::thread::spawn(move || h.run(&cfg)))
}

#[test]
fn two_players_stay_bit_identical_and_the_inputs_arrive() {
    let (addr, host) = host(2, 120);
    let a = play(addr.clone(), SCENE, keys, None);
    let b = play(addr, SCENE, keys, None);
    let (a, b) = (a.join().expect("player 0"), b.join().expect("player 1"));
    let report = host.join().expect("host thread").expect("host");
    assert!(report.desyncs.is_empty(), "{:?}", report.desyncs);
    assert_eq!(report.ticks, 120);
    assert_eq!(a.tick, 120);
    assert_eq!(b.tick, 120);
    assert_eq!(a.hash, b.hash, "the two copies of the game must be identical");
    assert!(a.desync.is_none() && b.desync.is_none());
    // the first player's hero walked right, the second's walked left for a while: each hero answers only to its own player
    assert!(a.xs[0] > 3.0, "player 0's hero moved right ({})", a.xs[0]);
    assert!(a.xs[1] < -1.0, "player 1's hero moved left ({})", a.xs[1]);
    assert_eq!(a.xs, b.xs);
    assert!(a.finished.as_deref().is_some_and(|r| r.contains("tick limit")), "{:?}", a.finished);
}

#[test]
fn lockstep_equals_the_offline_replay() {
    // the same inputs, applied by hand at the tick they take effect, give the same final hash: the network adds nothing
    let (addr, host) = host(2, 90);
    let a = play(addr.clone(), SCENE, keys, None);
    let b = play(addr, SCENE, keys, None);
    let (a, _b) = (a.join().expect("player 0"), b.join().expect("player 1"));
    host.join().expect("host thread").expect("host");
    let delay = HostConfig::default().delay;
    let mut offline = engine_for(1, SCENE).expect("scene");
    let mut held: [std::collections::BTreeMap<&str, f64>; 2] = Default::default();
    for tick in 0..90u64 {
        // a key chosen when the player's simulation was at tick `t` reaches the game at tick `t + delay`
        for (player, h) in held.iter_mut().enumerate() {
            if tick >= delay {
                for (k, v) in keys(player, tick - delay) {
                    if h.get(k) != Some(&v) {
                        h.insert(k, v);
                        let name = lunaris_net::key_name(player, k);
                        offline.call("input", &Json::obj([("values", Json::obj([(name, Json::from(v))]))])).expect("input");
                    }
                }
            }
        }
        offline.sim.step();
    }
    assert_eq!(format!("{:016x}", offline.sim.state_hash()), a.hash, "the lockstep game and the offline replay must agree");
}

#[test]
fn a_copy_that_drifts_is_found() {
    let (addr, host) = host(2, 150);
    let a = play(addr.clone(), SCENE, keys, None);
    let b = play(addr, SCENE, keys, Some(40));
    let (a, b) = (a.join().expect("player 0"), b.join().expect("player 1"));
    let report = host.join().expect("host thread").expect("host");
    assert!(!report.desyncs.is_empty(), "the host must notice the drift");
    assert!(report.desyncs[0].contains("differ"), "{:?}", report.desyncs);
    assert_ne!(a.hash, b.hash);
    assert!(a.desync.is_some() && b.desync.is_some(), "both players are told");
}

#[test]
fn players_with_different_worlds_are_refused() {
    let (addr, host) = host(2, 50);
    let a = std::thread::spawn({
        let addr = addr.clone();
        move || Lockstep::join(&addr, "a", |w| engine_for(w.seed, SCENE)).map(|_| ()).err()
    });
    let other: &'static str = Box::leak(SCENE.replace("\"pos\": [0, 0, 5]", "\"pos\": [0, 0, 6]").into_boxed_str());
    let b = std::thread::spawn(move || Lockstep::join(&addr, "b", |w| engine_for(w.seed, other)).map(|_| ()).err());
    let (a, b) = (a.join().expect("a"), b.join().expect("b"));
    let report = host.join().expect("host thread").expect("host");
    assert!(report.ended.contains("not the same"), "{}", report.ended);
    assert!(a.is_some_and(|e| e.contains("not the same")) && b.is_some_and(|e| e.contains("not the same")));
}

#[test]
fn a_player_who_leaves_ends_the_game_for_the_others() {
    let h = Host::bind("127.0.0.1:0").expect("bind");
    let addr = format!("127.0.0.1:{}", h.port());
    let cfg = HostConfig { players: 2, join_timeout: Duration::from_secs(20), timeout: Duration::from_secs(20), ..HostConfig::default() };
    let host = std::thread::spawn(move || h.run(&cfg));
    let stayer = play(addr.clone(), SCENE, keys, None);
    let leaver = std::thread::spawn(move || {
        let (mut ls, mut engine) = Lockstep::join(&addr, "leaver", |w| engine_for(w.seed, SCENE)).expect("join");
        let mut n = 0;
        while n < 25 {
            n += ls.advance(&mut engine, 4).expect("advance");
            std::thread::sleep(Duration::from_millis(1));
        }
        ls.leave("test over");
    });
    leaver.join().expect("leaver");
    let stayer = stayer.join().expect("stayer");
    let report = host.join().expect("host thread").expect("host");
    assert!(report.ended.contains("left"), "{}", report.ended);
    assert!(stayer.finished.is_some_and(|r| r.contains("left")));
}

/// Four players, each holding different keys on and off at different ticks, 300 ticks: everybody's copy must end identical.
#[test]
fn four_players_with_busy_fingers_stay_in_sync() {
    fn busy(player: usize, tick: u64) -> Vec<(&'static str, f64)> {
        let key = ["a", "d", "w", "s"][(tick as usize / (7 + player * 3)) % 4];
        // press one key and release the others, changing every few ticks at a different rhythm for each player
        ["a", "d", "w", "s"].iter().map(|k| (*k, if *k == key { 1.0 } else { 0.0 })).collect()
    }
    let (addr, host) = host(4, 300);
    let seats: Vec<_> = (0..4).map(|_| play(addr.clone(), SCENE, busy, None)).collect();
    let outcomes: Vec<Outcome> = seats.into_iter().map(|s| s.join().expect("a player")).collect();
    let report = host.join().expect("host thread").expect("host");
    assert!(report.desyncs.is_empty(), "{:?}", report.desyncs);
    assert_eq!(report.ticks, 300);
    for o in &outcomes {
        assert_eq!(o.tick, 300);
        assert_eq!(o.hash, outcomes[0].hash, "all four copies are identical");
        assert!(o.desync.is_none());
    }
}
