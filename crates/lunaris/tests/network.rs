//! `lunaris host` and `lunaris join` as real processes: a host relays a two-player duel, two bots hold different keys, and both must end
//! on the same tick with the same state hash (docs/NETWORK.md).
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn lunaris() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_lunaris"));
    c.current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    c
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("addr").port()
}

fn wait(mut child: Child, what: &str) -> String {
    let t0 = Instant::now();
    loop {
        if child.try_wait().expect("wait").is_some() {
            let out = child.wait_with_output().expect("output");
            assert!(out.status.success(), "{what} failed: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            return String::from_utf8_lossy(&out.stdout).into_owned();
        }
        if t0.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            panic!("{what} did not finish");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn line_with<'a>(out: &'a str, key: &str) -> &'a str {
    out.lines().find(|l| l.contains(key)).unwrap_or_else(|| panic!("no `{key}` line in:\n{out}"))
}

#[test]
fn a_host_and_two_bots_play_the_duel_and_agree() {
    let port = free_port().to_string();
    let addr = format!("127.0.0.1:{port}");
    let host = lunaris().args(["host", "--players", "2", "--port", &port, "--ticks", "500"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("host");
    std::thread::sleep(Duration::from_millis(300));
    let a = lunaris().args(["join", &addr, "examples/duel.lunaris.json", "--name", "Ada", "--hold", "d"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("bot a");
    let b = lunaris().args(["join", &addr, "examples/duel.lunaris.json", "--name", "Bo", "--hold", "a"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("bot b");
    let (a, b, host) = (wait(a, "bot a"), wait(b, "bot b"), wait(host, "host"));
    let (fa, fb) = (line_with(&a, "finished at tick"), line_with(&b, "finished at tick"));
    assert!(fa.contains("tick 500"), "{fa}");
    // same tick, same state hash: the two copies are bit-identical
    let hash = |l: &str| l.split("state hash ").nth(1).and_then(|r| r.split_whitespace().next()).expect("hash").to_string();
    assert_eq!(hash(fa), hash(fb), "{fa} / {fb}");
    assert!(host.contains("game over after 500 ticks"), "{host}");
    assert!(!host.contains("DESYNC") && !a.contains("DESYNC") && !b.contains("DESYNC"));
    // each bot saw its own and the other player's keys: player 1 pressed one key, player 2 the other
    assert!(line_with(&a, "resources:").contains("\"input.p2."), "{a}");
}

#[test]
fn a_bot_with_another_scene_is_refused() {
    let port = free_port().to_string();
    let addr = format!("127.0.0.1:{port}");
    let host = lunaris().args(["host", "--players", "2", "--port", &port, "--ticks", "100"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("host");
    std::thread::sleep(Duration::from_millis(300));
    let a = lunaris().args(["join", &addr, "examples/duel.lunaris.json", "--name", "Ada"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("bot a");
    let b = lunaris().args(["join", &addr, "examples/walker.lunaris.json", "--name", "Bo"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("bot b");
    for (child, who) in [(a, "bot a"), (b, "bot b")] {
        let out = child.wait_with_output().expect(who);
        assert!(!out.status.success(), "{who} should have been refused");
        assert!(String::from_utf8_lossy(&out.stderr).contains("not the same"), "{who}: {}", String::from_utf8_lossy(&out.stderr));
    }
    let out = host.wait_with_output().expect("host");
    assert!(String::from_utf8_lossy(&out.stdout).contains("not the same"));
}

fn final_state(out: &str) -> (String, String) {
    let line = line_with(out, "finished at tick");
    let tick = line.split("tick ").nth(1).and_then(|r| r.split(':').next()).expect("tick").to_string();
    let hash = line.split("state hash ").nth(1).and_then(|r| r.split_whitespace().next()).expect("hash").to_string();
    (tick, hash)
}

/// A rollback host and two bots, each on a slow simulated network (`--sim-latency`, `--sim-jitter`, `--sim-loss`): both end on the same tick
/// in the same state, and somebody went back at least once.
#[test]
fn a_rollback_host_and_two_bots_on_a_bad_network_play_the_duel_and_agree() {
    let port = free_port().to_string();
    let addr = format!("127.0.0.1:{port}");
    let host = lunaris().args(["host", "--rollback", "--players", "2", "--port", &port, "--ticks", "400"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("host");
    std::thread::sleep(Duration::from_millis(300));
    let a = lunaris().args(["join", &addr, "examples/duel.lunaris.json", "--name", "Ada", "--hold", "d", "--rollback", "--sim-latency", "20", "--sim-jitter", "8"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("bot a");
    let b = lunaris().args(["join", &addr, "examples/duel.lunaris.json", "--name", "Bo", "--hold", "a", "--rollback", "--input-delay", "3", "--sim-latency", "45", "--sim-jitter", "15", "--sim-loss", "5"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("bot b");
    let (a, b, host) = (wait(a, "bot a"), wait(b, "bot b"), wait(host, "host"));
    let (sa, sb) = (final_state(&a), final_state(&b));
    assert_eq!(sa.0, "400", "{a}");
    assert_eq!(sa, sb, "the two copies end on the same tick in the same state");
    assert!(host.contains("game over after 400 ticks"), "{host}");
    assert!(!host.contains("DESYNC") && !a.contains("DESYNC") && !b.contains("DESYNC"));
    assert!(line_with(&a, "seat token:").contains("--token"), "a rollback player is told how to come back: {a}");
    let rolled: u64 = [&a, &b].iter().map(|o| line_with(o, "rollback:").split_whitespace().nth(1).and_then(|n| n.parse::<u64>().ok()).expect("rollback count")).sum();
    assert!(rolled >= 1, "one of them held a key the other did not know about yet: {a}\n{b}");
}

/// A spectator that arrives in the middle of a rollback game replays it from tick 0 and ends in the state the players end in.
#[test]
fn a_watcher_replays_a_rollback_game_and_ends_in_the_same_state() {
    let port = free_port().to_string();
    let addr = format!("127.0.0.1:{port}");
    let host = lunaris().args(["host", "--rollback", "--players", "2", "--port", &port, "--ticks", "600"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("host");
    std::thread::sleep(Duration::from_millis(300));
    // a slow network, so that the game lasts a few seconds
    let a = lunaris().args(["join", &addr, "examples/duel.lunaris.json", "--name", "Ada", "--hold", "d", "--rollback", "--sim-latency", "60"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("bot a");
    let b = lunaris().args(["join", &addr, "examples/duel.lunaris.json", "--name", "Bo", "--hold", "w", "--rollback", "--sim-latency", "60"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("bot b");
    std::thread::sleep(Duration::from_millis(1500));
    let w = lunaris().args(["watch", &addr, "examples/duel.lunaris.json", "--name", "Eve"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("watcher");
    let (a, b, w, host) = (wait(a, "bot a"), wait(b, "bot b"), wait(w, "watcher"), wait(host, "host"));
    assert_eq!(final_state(&w), final_state(&a), "{w}\n{a}");
    assert_eq!(final_state(&a), final_state(&b));
    assert!(host.contains("Eve is watching") && host.contains("1 spectator(s) watched"), "{host}");
    assert!(!host.contains("DESYNC") && !w.contains("DESYNC"), "{host}\n{w}");
}

/// A rollback host with a lobby: the bots mark themselves ready (`--ready`) and the game starts; a lockstep bot is told what to do instead.
#[test]
fn a_lobby_starts_when_the_bots_are_ready_and_a_lockstep_bot_is_refused() {
    let port = free_port().to_string();
    let addr = format!("127.0.0.1:{port}");
    let host = lunaris().args(["host", "--rollback", "--lobby", "--players", "2", "--port", &port, "--ticks", "200"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("host");
    std::thread::sleep(Duration::from_millis(300));
    let wrong = lunaris().args(["join", &addr, "examples/duel.lunaris.json", "--name", "Old"]).stdout(Stdio::piped()).stderr(Stdio::piped()).output().expect("lockstep bot");
    assert!(!wrong.status.success());
    let why = String::from_utf8_lossy(&wrong.stderr);
    assert!(why.contains("rollback"), "{why}");
    let a = lunaris().args(["join", &addr, "examples/duel.lunaris.json", "--name", "Ada", "--hold", "d", "--rollback", "--lobby", "--ready"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("bot a");
    let b = lunaris().args(["join", &addr, "examples/duel.lunaris.json", "--name", "Bo", "--hold", "a", "--rollback", "--lobby", "--ready"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("bot b");
    let (a, b, host) = (wait(a, "bot a"), wait(b, "bot b"), wait(host, "host"));
    assert_eq!(final_state(&a), final_state(&b));
    assert!(a.contains("lobby (") && host.contains("Ada") && host.contains("Bo"), "{a}\n{host}");
}
