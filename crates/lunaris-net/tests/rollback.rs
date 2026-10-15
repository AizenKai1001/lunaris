//! Rollback netcode over real TCP sockets on this machine, through proxies that delay the traffic: every copy of the game must end
//! bit-identical, and identical to a game worked out without any network.
mod common;

use common::*;
use lunaris_json::Json;
use lunaris_net::{connect, HostConfig, JoinOptions, LatencyProxy, LinkSim, Mode};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::mpsc;
use std::time::Duration;

/// Three players, each behind a link with a different delay and jitter (40 to 120 ms one way, so a round trip of up to a quarter of a
/// second), each pressing different keys at different ticks.
#[test]
fn three_players_through_delaying_proxies_end_identical_and_equal_to_the_offline_replay() {
    let ticks = 150;
    let (addr, host) = spawn_host(rb_cfg(3, ticks));
    let links = [LinkSim { latency_ms: 40, jitter_ms: 20, loss_pct: 5.0, seed: 11, ..LinkSim::default() }, LinkSim { latency_ms: 80, jitter_ms: 30, seed: 12, ..LinkSim::default() }, LinkSim { latency_ms: 120, jitter_ms: 40, dup_pct: 5.0, seed: 13, ..LinkSim::default() }];
    let proxies: Vec<LatencyProxy> = links.iter().map(|l| LatencyProxy::start(&addr, *l).expect("proxy")).collect();
    let bots: Vec<_> = proxies.iter().map(|p| spawn_bot(p.addr().to_string(), BotSpec::new("bot", Mode::Rollback, mixed))).collect();
    let outcomes: Vec<Outcome> = bots.into_iter().map(|b| b.join().expect("thread").expect("bot")).collect();
    let report = host.join().expect("host thread").expect("host");

    assert!(report.desyncs.is_empty(), "{:?}", report.desyncs);
    assert_eq!(report.ticks, ticks);
    for o in &outcomes {
        assert_eq!(o.tick, ticks, "every copy ends on the host's last tick");
        assert_eq!(o.hash, outcomes[0].hash, "all copies are identical");
        assert!(o.desync.is_none());
        let s = o.stats.expect("rollback stats");
        assert!(s.max_depth <= 8, "a rollback is never deeper than the window: {s:?}");
    }
    // the point of the exercise: guesses were wrong, and the copies still agree
    let rollbacks: u64 = outcomes.iter().map(|o| o.stats.map_or(0, |s| s.rollbacks)).sum();
    assert!(rollbacks >= 1, "with this much delay and these keys somebody must have rolled back: {outcomes:?}");
    let resimulated: u64 = outcomes.iter().map(|o| o.stats.map_or(0, |s| s.resimulated_ticks)).sum();
    println!("rollbacks {rollbacks}, ticks played again {resimulated}; proxies: {:?}", proxies.iter().map(|p| p.report()).collect::<Vec<_>>());
    // the same game, worked out twice without a network: from the keys the scripts chose, and from the log the host kept
    let expected = scripted_hash(&[mixed, mixed, mixed], ticks, 2, SCENE);
    assert_eq!(outcomes[0].hash, expected, "the network game and the offline game of the same keys agree");
    assert_eq!(outcomes[0].hash, log_hash(&report, ticks, SCENE), "...and so does a replay of the host's confirmed input log");
    // the keys did something: each hero moved, and the counters of the players who held space moved
    assert!(outcomes[0].resources["held1"] > 0.0 && outcomes[0].resources["held2"] > 0.0 && outcomes[0].resources["held3"] > 0.0, "{:?}", outcomes[0].resources);
}

/// With nothing to correct, nothing is corrected: no rollbacks, whatever the delay, and the copies still agree.
#[test]
fn a_game_where_nobody_changes_a_key_never_rolls_back() {
    let ticks = 120;
    let (addr, host) = spawn_host(rb_cfg(3, ticks));
    let proxies: Vec<LatencyProxy> = [30u64, 60, 90].iter().map(|l| LatencyProxy::start(&addr, LinkSim::new(*l, 10)).expect("proxy")).collect();
    let bots: Vec<_> = proxies.iter().map(|p| spawn_bot(p.addr().to_string(), BotSpec::new("bot", Mode::Rollback, calm))).collect();
    let outcomes: Vec<Outcome> = bots.into_iter().map(|b| b.join().expect("thread").expect("bot")).collect();
    let report = host.join().expect("host thread").expect("host");
    assert!(report.desyncs.is_empty());
    for o in &outcomes {
        let s = o.stats.expect("stats");
        assert_eq!(s.rollbacks, 0, "nothing changed, so no guess was wrong: {s:?}");
        assert_eq!(s.resimulated_ticks, 0);
        assert!(s.guessed_ticks > 0, "...although the copies did run ahead on guesses: {s:?}");
        assert_eq!(o.tick, ticks);
        assert_eq!(o.hash, outcomes[0].hash);
    }
    assert_eq!(outcomes[0].hash, scripted_hash(&[calm, calm, calm], ticks, 2, SCENE));
}

/// The same keys, the same input delay: a lockstep game and a rollback game over a bad network end in the same state.
#[test]
fn rollback_and_lockstep_play_the_same_game() {
    let ticks = 120;
    // lockstep, input delay 3
    let lock_cfg = HostConfig { players: 2, max_ticks: Some(ticks), join_timeout: Duration::from_secs(30), timeout: Duration::from_secs(30), ..HostConfig::default() };
    let (addr, host) = spawn_host(lock_cfg);
    let a = spawn_bot(addr.clone(), BotSpec::new("a", Mode::Lockstep, mixed));
    let b = spawn_bot(addr, BotSpec::new("b", Mode::Lockstep, mixed));
    let (la, lb) = (a.join().expect("a").expect("bot a"), b.join().expect("b").expect("bot b"));
    host.join().expect("host thread").expect("host");
    assert_eq!(la.hash, lb.hash);
    // rollback, input delay 3 as well, through proxies
    let mut cfg = rb_cfg(2, ticks);
    cfg.delay = 3;
    let (addr, host) = spawn_host(cfg);
    let proxies: Vec<LatencyProxy> = [50u64, 90].iter().map(|l| LatencyProxy::start(&addr, LinkSim::new(*l, 25)).expect("proxy")).collect();
    let bots: Vec<_> = proxies.iter().map(|p| spawn_bot(p.addr().to_string(), BotSpec::new("bot", Mode::Rollback, mixed))).collect();
    let outcomes: Vec<Outcome> = bots.into_iter().map(|b| b.join().expect("thread").expect("bot")).collect();
    let report = host.join().expect("host thread").expect("host");
    assert!(report.desyncs.is_empty());
    assert_eq!(outcomes[0].hash, outcomes[1].hash);
    assert_eq!(outcomes[0].hash, la.hash, "lockstep and rollback give the same game for the same keys");
    assert_eq!(la.hash, scripted_hash(&[mixed, mixed], ticks, 3, SCENE));
}

// ---------------------------------------------------------------------------------------------------------------- protocol by hand

/// A player written by hand in a few lines of JSON, to show that the wire is all there is to it, and to be a player that does exactly what the
/// test says (here: nothing, then a run of empty inputs).
struct Ghost {
    w: TcpStream,
    r: BufReader<TcpStream>,
    player: usize,
}

impl Ghost {
    fn say(&mut self, v: &str) {
        // (a write that fails because the game is over is not the ghost's problem)
        let _ = self.w.write_all(format!("{v}\n").as_bytes());
    }

    fn hear(&mut self) -> Json {
        let mut line = String::new();
        self.r.read_line(&mut line).expect("a line from the host");
        lunaris_json::parse(line.trim()).expect("json")
    }

    /// Hello, welcome, load the scene, ready, start.
    fn join(addr: &str) -> Ghost {
        let w = TcpStream::connect(addr).expect("connect");
        w.set_read_timeout(Some(Duration::from_secs(20))).expect("timeout");
        let r = BufReader::new(w.try_clone().expect("clone"));
        let mut g = Ghost { w, r, player: 0 };
        g.say(r#"{"hello": {"name": "ghost", "version": 2, "mode": "rollback", "role": "player"}}"#);
        let welcome = g.hear();
        let wv = welcome.get("welcome").unwrap_or_else(|| panic!("a welcome, got {welcome:?}"));
        g.player = wv.get("player").and_then(|v| v.as_u64()).expect("player") as usize;
        let seed = u64::from_str_radix(wv.get("seed").and_then(|v| v.as_str()).expect("seed"), 16).expect("hex");
        let e = engine_for(seed, SCENE).expect("scene");
        g.say(&format!(r#"{{"ready": {{"hash": "{}"}}}}"#, hash_of(&e)));
        loop {
            let m = g.hear();
            if m.get("start").is_some() {
                return g;
            }
            assert!(m.get("bye").is_none(), "refused: {m:?}");
        }
    }

    fn input(&mut self, tick: u64) {
        self.say(&format!(r#"{{"input": {{"tick": {tick}, "values": {{}}}}}}"#));
    }
}

/// On a network that reorders messages (or the latency simulator, which holds one message back longer than the next) another player's first
/// inputs can overtake the `start` that was sent before them. They must be kept, not thrown away with the other things that arrive while a
/// client waits for the start: without them the client would wait for ever for inputs that were never sent again.
#[test]
fn inputs_that_overtake_the_start_message_are_not_lost() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = format!("127.0.0.1:{}", listener.local_addr().expect("addr").port());
    let fake_host = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().expect("accept");
        let mut r = BufReader::new(s.try_clone().expect("clone"));
        let mut line = String::new();
        r.read_line(&mut line).expect("hello");
        writeln!(s, r#"{{"welcome": {{"player": 0, "players": 2, "seed": "0000000000000001", "delay": 2, "mode": "rollback", "window": 8, "token": "t", "max_ticks": 40}}}}"#).expect("welcome");
        line.clear();
        r.read_line(&mut line).expect("ready");
        // the other player's inputs for ticks 0 to 30 arrive BEFORE the start message
        for t in 0..=30 {
            writeln!(s, r#"{{"input": {{"player": 1, "tick": {t}, "values": {{}}}}}}"#).expect("input");
        }
        writeln!(s, r#"{{"start": {{"player": 0, "players": 2, "tick": 0}}}}"#).expect("start");
        // stay connected until the client hangs up
        loop {
            line.clear();
            if r.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
        }
    });
    let mut o = JoinOptions::new("client", Mode::Rollback);
    o.input_delay = Some(2);
    let (mut seat, mut engine) = connect(&addr, o, |w| engine_for(w.seed, SCENE)).expect("join");
    let t0 = std::time::Instant::now();
    while t0.elapsed() < Duration::from_millis(400) {
        seat.advance(&mut engine, 100).expect("advance");
        std::thread::sleep(Duration::from_millis(1));
    }
    // with the other player's inputs known for ticks 0 to 30 the first tick that is not settled is 31, and the client may play 8 ticks past
    // it (the tick limit, 40, is further); had the early inputs been thrown away it would have stopped at tick 8
    assert_eq!(engine.sim.tick, 39, "the early inputs were used");
    drop(seat);
    fake_host.join().expect("fake host");
}

/// The news that a frozen seat is back can arrive after the first input of the player who is back (a network that reorders messages). That
/// input must not be thrown away, and it must still correct the ticks that were played with the seat counted as empty.
#[test]
fn an_input_that_overtakes_the_news_that_its_seat_is_back_still_counts() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = format!("127.0.0.1:{}", listener.local_addr().expect("addr").port());
    let fake_host = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().expect("accept");
        let mut r = BufReader::new(s.try_clone().expect("clone"));
        let mut line = String::new();
        r.read_line(&mut line).expect("hello");
        writeln!(s, r#"{{"welcome": {{"player": 0, "players": 2, "seed": "0000000000000001", "delay": 2, "mode": "rollback", "window": 8, "token": "t", "max_ticks": 14}}}}"#).expect("welcome");
        line.clear();
        r.read_line(&mut line).expect("ready");
        writeln!(s, r#"{{"start": {{"player": 0, "players": 2, "tick": 0}}}}"#).expect("start");
        for t in 0..=4 {
            writeln!(s, r#"{{"input": {{"player": 1, "tick": {t}, "values": {{}}}}}}"#).expect("input");
        }
        // the other seat is frozen from tick 5: the client may play on without it (up to 8 ticks beyond what the host says is settled)
        writeln!(s, r#"{{"seat": {{"player": 1, "name": "ghost", "state": "frozen", "tick": 5, "reason": "test", "n": 0}}}}"#).expect("frozen");
        std::thread::sleep(Duration::from_millis(300));
        // the player is back from tick 7, and its first input (pressing D at tick 7) comes BEFORE the news
        writeln!(s, r#"{{"input": {{"player": 1, "tick": 7, "values": {{"d": true}}}}}}"#).expect("input");
        std::thread::sleep(Duration::from_millis(100));
        writeln!(s, r#"{{"seat": {{"player": 1, "name": "ghost", "state": "live", "tick": 7, "reason": "test", "n": 1}}}}"#).expect("live");
        for t in 5..=6 {
            let _ = t; // (ticks 5 and 6 are frozen: empty by the host's decision, nothing is sent for them)
        }
        for t in 8..=14 {
            writeln!(s, r#"{{"input": {{"player": 1, "tick": {t}, "values": {{}}}}}}"#).expect("input");
        }
        std::thread::sleep(Duration::from_millis(100));
        writeln!(s, r#"{{"bye": {{"reason": "test over", "tick": 14}}}}"#).expect("bye");
        loop {
            line.clear();
            if r.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
        }
    });
    let mut o = JoinOptions::new("client", Mode::Rollback);
    o.input_delay = Some(2);
    o.hash_every = 0;
    let (mut seat, mut engine) = connect(&addr, o, |w| engine_for(w.seed, SCENE)).expect("join");
    let t0 = std::time::Instant::now();
    loop {
        seat.advance(&mut engine, 100).expect("advance");
        if seat.finished().is_some() {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(20), "the game did not finish: tick {}", engine.sim.tick);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(engine.sim.tick, 14);
    // the game as it was: the second player held D from tick 7
    let mut expected = engine_for(1, SCENE).expect("scene");
    for tick in 0..14 {
        if tick == 7 {
            expected.call("input", &Json::obj([("values", Json::obj([("p2.d", Json::Bool(true))]))])).expect("input");
        }
        expected.sim.step();
    }
    assert_eq!(hash_of(&engine), hash_of(&expected), "the early input was kept and corrected tick 7 on");
    let stats = seat.rollback_stats().expect("stats");
    assert!(stats.rollbacks >= 1, "{stats:?}");
    drop(seat);
    fake_host.join().expect("fake host");
}

/// A client may play at most `window` ticks beyond the oldest tick somebody's input is still missing for. Here the missing input is a
/// ghost's, and the real client must stop exactly there, and go on when the ghost speaks.
#[test]
fn a_client_stalls_at_the_edge_of_its_window_until_confirmations_arrive() {
    let ticks = 60;
    let (addr, host) = spawn_host(rb_cfg(2, ticks));
    let (head_tx, head_rx) = mpsc::channel::<u64>();
    let real = {
        let addr = addr.clone();
        std::thread::spawn(move || {
            let mut o = JoinOptions::new("real", Mode::Rollback);
            o.input_delay = Some(2);
            o.hash_every = 10;
            let (mut seat, mut engine) = connect(&addr, o, |w| engine_for(w.seed, SCENE)).expect("join");
            loop {
                let stepped = seat.advance(&mut engine, 100).expect("advance");
                let _ = head_tx.send(engine.sim.tick);
                if stepped == 0 {
                    if seat.finished().is_some() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            (hash_of(&engine), engine.sim.tick, seat.rollback_stats().expect("stats"))
        })
    };
    let mut ghost = Ghost::join(&addr);
    // the ghost says nothing: the real client has no input of the ghost's for tick 0 and cannot settle anything, so it may play tick 0 to 7
    // (8 ticks ahead of the first tick that is not settled) and then it must wait
    wait_until("the client to reach the edge of its window", Duration::from_secs(10), || head_rx.try_iter().any(|h| h == 8));
    std::thread::sleep(Duration::from_millis(150));
    let highest = head_rx.try_iter().max().unwrap_or(8);
    assert_eq!(highest, 8, "stalled exactly one window (8 ticks) ahead of tick 0, the first one the ghost's input is missing for");
    // the ghost speaks up to tick 20: ticks 0 to 20 are settled now, so the first one that is not is 21, and the client may play 8 beyond it
    for t in 0..=20 {
        ghost.input(t);
    }
    wait_until("the client to move on", Duration::from_secs(10), || head_rx.try_iter().any(|h| h >= 29));
    std::thread::sleep(Duration::from_millis(150));
    let highest = head_rx.try_iter().max().unwrap_or(29);
    assert_eq!(highest, 21 + 8, "the edge moved to tick 21 and the window is 8");
    // the rest of the game
    for t in 21..(ticks + 10) {
        ghost.input(t);
    }
    let (hash, tick, stats) = real.join().expect("real client");
    let report = host.join().expect("host thread").expect("host");
    assert_eq!(tick, ticks);
    assert!(stats.stalls >= 1, "the client counted the times it had to wait: {stats:?}");
    assert_eq!(stats.rollbacks, 0, "the ghost never changed a key");
    assert_eq!(hash, scripted_hash(&[calm, calm], ticks, 2, SCENE));
    assert_eq!(report.ticks, ticks);
    drop(ghost);
}

/// A copy of the game that gets corrupted in the middle of a game is found at the next settled tick that is reported, and everybody is told.
#[test]
fn a_mid_game_divergence_is_reported_to_everybody() {
    let ticks = 150;
    let (addr, host) = spawn_host(rb_cfg(2, ticks));
    let proxies: Vec<LatencyProxy> = [20u64, 35].iter().map(|l| LatencyProxy::start(&addr, LinkSim::new(*l, 5)).expect("proxy")).collect();
    let a = spawn_bot(proxies[0].addr().to_string(), BotSpec::new("honest", Mode::Rollback, mixed));
    let mut cheat = BotSpec::new("cheat", Mode::Rollback, mixed);
    cheat.hooks.tamper_at = Some(60);
    let b = spawn_bot(proxies[1].addr().to_string(), cheat);
    let (a, b) = (a.join().expect("a").expect("bot a"), b.join().expect("b").expect("bot b"));
    let report = host.join().expect("host thread").expect("host");
    assert!(!report.desyncs.is_empty(), "the host must notice the corrupted copy");
    assert!(report.desyncs[0].contains("differ"), "{:?}", report.desyncs);
    assert_ne!(a.hash, b.hash);
    assert!(a.desync.is_some() && b.desync.is_some(), "both players are told");
    // it was found at a tick after the corruption and not before: before it, the copies agreed
    let tick: u64 = report.desyncs[0].split(|c: char| !c.is_ascii_digit()).find(|s| !s.is_empty()).and_then(|s| s.parse().ok()).expect("a tick in the report");
    assert!(tick >= 60, "{:?}", report.desyncs);
}

/// A player whose world is not the same as the first player's is turned away at the door, and the game starts without it as soon as somebody
/// with the right world takes the seat.
#[test]
fn a_client_with_a_tampered_scene_is_refused_at_hello_and_the_game_goes_on_without_it() {
    let ticks = 60;
    let (addr, host) = spawn_host(rb_cfg(2, ticks));
    let (welcomed_tx, welcomed_rx) = mpsc::channel::<()>();
    let a = {
        let addr = addr.clone();
        std::thread::spawn(move || {
            let mut o = JoinOptions::new("first", Mode::Rollback);
            o.hash_every = 10;
            let (mut seat, mut engine) = connect(&addr, o, |w| {
                let e = engine_for(w.seed, SCENE);
                let _ = welcomed_tx.send(());
                e
            })
            .expect("first player");
            drive(&mut seat, &mut engine, &Hooks::new(calm)).expect("drive")
        })
    };
    welcomed_rx.recv_timeout(Duration::from_secs(10)).expect("the first player loaded its world");
    std::thread::sleep(Duration::from_millis(200)); // its ready message is on its way to the host
    let other: &'static str = Box::leak(SCENE.replace("\"pos\": [0, 0, 5]", "\"pos\": [0, 0, 6]").into_boxed_str());
    let mut tampered = BotSpec::new("tampered", Mode::Rollback, calm);
    tampered.scene = other;
    let refused = spawn_bot(addr.clone(), tampered).join().expect("thread");
    let err = refused.expect_err("a different world must be refused");
    assert!(err.contains("not the same"), "{err}");
    // the seat is free again: a player with the right world takes it and the game starts
    let c = spawn_bot(addr, BotSpec::new("third", Mode::Rollback, calm)).join().expect("c").expect("bot c");
    let a = a.join().expect("a");
    let report = host.join().expect("host thread").expect("host");
    assert_eq!(a.hash, c.hash);
    assert_eq!(a.tick, ticks);
    assert!(report.events.iter().any(|e| e.contains("refused") && e.contains("tampered")), "{:?}", report.events);
}
