//! Spectators and players who come back: both are sent the host's input log from tick 0, play it at full speed to catch up, and end the
//! game in exactly the state everybody else has. And a seat that nobody sits in does not hold the others up for ever.
mod common;

use common::*;
use lunaris_net::{LatencyProxy, LinkSim, Mode};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// The player these tests kill: holds `d` from tick 20 to 300 (so it is holding a key when it dies) and taps space late in the game.
fn keeper(_player: usize, tick: u64) -> Vec<(&'static str, f64)> {
    let mut out = Vec::new();
    if tick >= 300 {
        out.push(("d", 0.0));
    } else if tick >= 20 {
        out.push(("d", 1.0));
    }
    if tick >= 450 {
        out.push(("space", if tick < 470 { 1.0 } else { 0.0 }));
    }
    out
}

fn paced(mut spec: BotSpec, ms: u64) -> BotSpec {
    spec.hooks.pace = Duration::from_millis(ms);
    spec
}

#[test]
fn a_spectator_who_joins_late_catches_up_and_ends_with_the_same_hash() {
    let ticks = 600;
    let (addr, host) = spawn_host(rb_cfg(2, ticks));
    let progress = Arc::new(AtomicU64::new(0));
    let proxies: Vec<LatencyProxy> = [12u64, 20].iter().map(|l| LatencyProxy::start(&addr, LinkSim::new(*l, 4)).expect("proxy")).collect();
    let mut a = paced(BotSpec::new("a", Mode::Rollback, mixed), 2);
    a.hooks.progress = Some(progress.clone());
    let a = spawn_bot(proxies[0].addr().to_string(), a);
    let b = spawn_bot(proxies[1].addr().to_string(), paced(BotSpec::new("b", Mode::Rollback, mixed), 2));
    wait_until("the game to be well under way", Duration::from_secs(20), || progress.load(Ordering::SeqCst) >= 150);
    // a spectator whose scene is not the game's is turned away...
    let other: &'static str = Box::leak(SCENE.replace("\"pos\": [0, 0, 5]", "\"pos\": [0, 0, 6]").into_boxed_str());
    let mut wrong = BotSpec::new("wrong", Mode::Rollback, calm);
    wrong.watch = true;
    wrong.scene = other;
    let err = spawn_bot(addr.clone(), wrong).join().expect("thread").expect_err("a spectator with another world is refused");
    assert!(err.contains("not the same"), "{err}");
    // ...and a good one is sent the 150 ticks it missed, plays them in one go, and follows the game to its end
    let mut w = BotSpec::new("watcher", Mode::Rollback, calm);
    w.watch = true;
    w.hooks.steps = 100_000;
    let w = spawn_bot(addr, w).join().expect("watcher thread");
    let (a, b) = (a.join().expect("a").expect("bot a"), b.join().expect("b").expect("bot b"));
    let report = host.join().expect("host thread").expect("host");
    let w = w.unwrap_or_else(|e| panic!("watcher: {e}; host events {:?}; ended {}", report.events, report.ended));
    assert_eq!(a.hash, b.hash);
    assert_eq!(w.tick, ticks, "the spectator ends on the same tick");
    assert_eq!(w.hash, a.hash, "...in the same state as the players, which it never played a single key of");
    assert_eq!(w.hash, log_hash(&report, ticks, SCENE));
    assert!(w.stats.is_none(), "a spectator has nothing to roll back");
    assert_eq!(report.watchers, 1);
    assert!(report.desyncs.is_empty(), "the spectator's own hashes were checked against the players': {:?}", report.desyncs);
    assert!(report.events.iter().any(|e| e.contains("watcher is watching")), "{:?}", report.events);
    assert!(report.log.entries.iter().all(|e| e.player < 2));
}

#[test]
fn a_player_who_was_killed_comes_back_with_its_token_replays_the_game_and_plays_on() {
    let ticks = 700;
    let mut cfg = rb_cfg(2, ticks);
    cfg.grace = Duration::from_millis(250);
    let (addr, host) = spawn_host(cfg);
    let progress = Arc::new(AtomicU64::new(0));
    let proxies: Vec<LatencyProxy> = [10u64, 15].iter().map(|l| LatencyProxy::start(&addr, LinkSim::new(*l, 3)).expect("proxy")).collect();
    let mut a = paced(BotSpec::new("a", Mode::Rollback, busy), 2);
    a.hooks.progress = Some(progress.clone());
    let a = spawn_bot(proxies[0].addr().to_string(), a);
    let (token_tx, token_rx) = mpsc::channel();
    let mut b = BotSpec::new("b", Mode::Rollback, keeper);
    b.hooks.kill_at = Some(100); // hangs up with `d` held down, without a goodbye
    b.token_tx = Some(token_tx);
    let b = spawn_bot(proxies[1].addr().to_string(), b);
    let token = token_rx.recv_timeout(Duration::from_secs(20)).expect("the seat token");
    let dead = b.join().expect("b thread").expect("bot b");
    assert!(dead.killed && dead.tick >= 100 && dead.tick < 110, "{dead:?}");
    // the others do not wait for ever: after the grace period the seat repeats its last keys (`d` stays down) and the game goes on
    wait_until("the game to go on without the dead player", Duration::from_secs(20), || progress.load(Ordering::SeqCst) >= 250);
    // the player comes back: same token, same scene, and the whole game is replayed from tick 0 before the seat is given back
    let mut b2 = BotSpec::new("b-again", Mode::Rollback, keeper);
    b2.token = Some(token);
    let b2 = spawn_bot(proxies[1].addr().to_string(), b2).join().expect("b2 thread").expect("bot b2");
    let a = a.join().expect("a").expect("bot a");
    let report = host.join().expect("host thread").expect("host");
    assert_eq!(a.tick, ticks);
    assert_eq!(b2.tick, ticks);
    let seat = dead.player;
    assert_eq!(b2.player, seat, "it got its old seat");
    assert_eq!(a.hash, b2.hash, "the player who was away ends in the same state as the one who stayed");
    assert_eq!(a.hash, log_hash(&report, ticks, SCENE), "...which is the state of the host's confirmed log");
    assert!(report.desyncs.is_empty(), "{:?}", report.desyncs);
    assert_eq!(report.rejoins, 1);
    // what the host and the other player were told, in order
    let said = |events: &[String], what: &str| events.iter().position(|e| e.contains(what));
    let (left, frozen, back) = (said(&report.events, &format!("left seat {}", seat + 1)), said(&report.events, "is frozen from tick"), said(&report.events, &format!("is back in seat {}", seat + 1)));
    assert!(left.is_some() && frozen.is_some() && back.is_some(), "{:?}", report.events);
    assert!(left < frozen && frozen < back, "{:?}", report.events);
    assert!(a.events.iter().any(|e| e.contains("frozen")) && a.events.iter().any(|e| e.contains(&format!("is back in seat {}", seat + 1))), "the other player was told too: {:?}", a.events);
    // the seat plays again after it came back: the space key it taps late in the game is in the log (and the counter of that hero moved)
    assert!(report.log.entries.iter().any(|e| e.player == seat && e.tick >= 450), "the returned player's late key presses reached the log");
    assert!(a.resources[&format!("held{}", seat + 1)] > 0.0, "{:?}", a.resources);
    // while it was away its seat repeated the last keys: nothing for player 2 appears in the log between the drop and the return
    let drop_tick = 100;
    let back_tick = report.events.iter().find_map(|e| e.split("from tick ").nth(1).filter(|_| e.contains("is back")).and_then(|r| r.split_whitespace().next()).and_then(|n| n.parse::<u64>().ok())).expect("the tick it came back at");
    assert!(back_tick > 250 - 20, "it came back after the game had gone on without it: {back_tick}");
    assert!(report.log.entries.iter().filter(|e| e.player == seat).all(|e| e.tick < drop_tick + 10 || e.tick >= back_tick), "no input for the empty seat in between: {:?}", report.log.entries.iter().filter(|e| e.player == seat).map(|e| e.tick).collect::<Vec<_>>());
}

/// The player who dies is the one that was ahead (its network is the fast one, the other's is slow), and it comes back at once, before the
/// grace period is over and while the others have not settled what it had already sent: its old inputs are known to everybody but not
/// settled, so the seat must be given back from after them, with the inputs of its earlier life, not from the settled tick.
#[test]
fn a_player_who_comes_back_at_once_while_the_others_lag_ends_in_the_same_state() {
    let ticks = 400;
    let mut cfg = rb_cfg(2, ticks);
    cfg.grace = Duration::from_secs(10); // nothing is frozen by the clock: the return itself freezes the seat
    let (addr, host) = spawn_host(cfg);
    let slow = LatencyProxy::start(&addr, LinkSim::new(80, 10)).expect("proxy");
    let a = spawn_bot(slow.addr().to_string(), BotSpec::new("slow", Mode::Rollback, busy));
    let (token_tx, token_rx) = mpsc::channel();
    let mut b = BotSpec::new("fast", Mode::Rollback, keeper);
    b.hooks.kill_at = Some(60);
    b.token_tx = Some(token_tx);
    let b = spawn_bot(addr.clone(), b);
    let token = token_rx.recv_timeout(Duration::from_secs(20)).expect("the seat token");
    let dead = b.join().expect("b thread").expect("bot b");
    assert!(dead.killed);
    let mut again = BotSpec::new("fast-again", Mode::Rollback, keeper);
    again.token = Some(token);
    let again = spawn_bot(addr, again).join().expect("again thread").expect("bot again");
    let a = a.join().expect("a").expect("bot a");
    let report = host.join().expect("host thread").expect("host");
    assert_eq!((a.tick, again.tick), (ticks, ticks), "the stayer: {a:?}; the returner: {again:?}; the host's story: {:?}", report.events);
    let from_log = log_hash(&report, ticks, SCENE);
    assert_eq!(a.hash, again.hash, "the player who came back and the one who stayed: log {from_log}; the host's story: {:?}; the stayer's: {:?}; the returner's: {:?}; desyncs {:?}", report.events, a.events, again.events, report.desyncs);
    assert_eq!(a.hash, from_log);
    assert!(report.desyncs.is_empty(), "{:?}", report.desyncs);
    assert_eq!(report.rejoins, 1, "{:?}", report.events);
}

#[test]
fn an_empty_seat_does_not_hold_the_game_up_for_ever() {
    let ticks = 400;
    let mut cfg = rb_cfg(2, ticks);
    cfg.grace = Duration::from_millis(800);
    let (addr, host) = spawn_host(cfg);
    let progress = Arc::new(AtomicU64::new(0));
    let mut a = paced(BotSpec::new("stays", Mode::Rollback, mixed), 1);
    a.hooks.progress = Some(progress.clone());
    let a = spawn_bot(addr.clone(), a);
    let mut b = BotSpec::new("leaves", Mode::Rollback, mixed);
    b.hooks.kill_at = Some(50);
    let dead = spawn_bot(addr, b).join().expect("b thread").expect("bot b");
    assert!(dead.killed);
    // during the grace period everybody waits for the seat: the player who stayed can go no further than a window past the last input the
    // dead one sent (it sent through tick 51 or 52)
    std::thread::sleep(Duration::from_millis(150));
    let during = progress.load(Ordering::SeqCst);
    assert!(during <= 53 + 8, "the game waits for the empty seat for the grace period, then not: {during}");
    // after it, the seat is frozen (it repeats its last input) and the game goes on to the end alone
    wait_until("the game to go on", Duration::from_secs(20), || progress.load(Ordering::SeqCst) > 120);
    let a = a.join().expect("a").expect("bot a");
    let report = host.join().expect("host thread").expect("host");
    assert_eq!(a.tick, ticks);
    assert_eq!(report.ticks, ticks);
    assert!(report.events.iter().any(|e| e.contains("leaves left seat")), "{:?}", report.events);
    assert!(report.events.iter().any(|e| e.contains("is frozen from tick")), "{:?}", report.events);
    assert!(a.events.iter().any(|e| e.contains("frozen")), "the player who stayed was told: {:?}", a.events);
    assert_eq!(a.hash, log_hash(&report, ticks, SCENE));
    assert_eq!(report.rejoins, 0);
}

#[test]
fn the_input_log_has_a_memory_limit_and_a_game_too_old_cannot_be_joined_late() {
    let ticks = 500;
    let mut cfg = rb_cfg(2, ticks);
    cfg.max_log_bytes = 3000; // about forty key changes
    let (addr, host) = spawn_host(cfg);
    let progress = Arc::new(AtomicU64::new(0));
    let mut a = paced(BotSpec::new("a", Mode::Rollback, busy), 2);
    a.hooks.progress = Some(progress.clone());
    let a = spawn_bot(addr.clone(), a);
    let b = spawn_bot(addr.clone(), paced(BotSpec::new("b", Mode::Rollback, busy), 2));
    wait_until("the game to produce more key changes than the log keeps", Duration::from_secs(20), || progress.load(Ordering::SeqCst) >= 300);
    let mut w = BotSpec::new("late", Mode::Rollback, calm);
    w.watch = true;
    let err = spawn_bot(addr, w).join().expect("thread").expect_err("the log no longer starts at tick 0");
    assert!(err.contains("too old"), "{err}");
    let (a, b) = (a.join().expect("a").expect("bot a"), b.join().expect("b").expect("bot b"));
    let report = host.join().expect("host thread").expect("host");
    assert_eq!(a.hash, b.hash, "the game itself is not affected by the host's memory limit");
    assert!(!report.log.complete);
    assert!(report.events.iter().any(|e| e.contains("reached its limit")), "{:?}", report.events);
    assert!(report.log.entries.len() < 100, "bounded: {} entries kept", report.log.entries.len());
}
