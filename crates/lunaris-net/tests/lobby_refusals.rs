//! The lobby (players join with a name, mark themselves ready, the game starts when all are, or when the first seat says so) and the
//! refusals: every pairing of a client and a host that cannot play together gets a message that says why, promptly, never a hang.
mod common;

use common::*;
use lunaris_net::{connect, HostConfig, JoinOptions, LobbyCommand, Lockstep, Mode};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

struct LobbyBot {
    handle: JoinHandle<Result<Outcome, String>>,
    commands: mpsc::Sender<LobbyCommand>,
    /// Every roster the host sent this player, as a line of text.
    rosters: Arc<Mutex<Vec<String>>>,
    /// Gets a message when the game starts for this player.
    started: mpsc::Receiver<()>,
}

impl LobbyBot {
    fn saw(&self, text: &str) -> bool {
        self.rosters.lock().expect("rosters").iter().any(|l| l.contains(text))
    }
}

/// A player that waits in the lobby: `steered` ones do what the test tells them (marking ready, starting), the others are bots that mark
/// themselves ready at once.
fn lobby_bot(addr: &str, name: &'static str, mode: Mode, steered: bool) -> LobbyBot {
    let (commands, rx) = mpsc::channel();
    let rosters = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started) = mpsc::channel();
    let addr = addr.to_string();
    let r2 = rosters.clone();
    let handle = std::thread::spawn(move || {
        let mut o = JoinOptions::new(name, mode);
        o.lobby = true;
        o.hash_every = 10;
        if steered {
            o.commands = Some(rx);
        }
        o.on_roster = Some(Box::new(move |r| r2.lock().expect("rosters").push(r.describe())));
        let (mut seat, mut engine) = connect(&addr, o, |w| engine_for(w.seed, SCENE))?;
        let _ = started_tx.send(());
        drive(&mut seat, &mut engine, &Hooks::new(mixed))
    });
    LobbyBot { handle, commands, rosters, started }
}

fn lobby_cfg(mode: Mode, players: usize, ticks: u64) -> HostConfig {
    let mut cfg = if mode == Mode::Rollback { rb_cfg(players, ticks) } else { HostConfig { players, max_ticks: Some(ticks), join_timeout: Duration::from_secs(30), timeout: Duration::from_secs(30), ..HostConfig::default() } };
    cfg.lobby = true;
    cfg
}

/// Three seats, two people: nothing starts until the first seat says `start`, whoever is ready or not.
fn the_first_seat_starts_the_game(mode: Mode) {
    let ticks = 90;
    let cfg = lobby_cfg(mode, 3, ticks);
    let delay = cfg.delay;
    let (addr, host) = spawn_host(cfg);
    let ada = lobby_bot(&addr, "Ada", mode, true);
    wait_until("Ada to be in the lobby", Duration::from_secs(10), || ada.saw("Ada [not ready]"));
    assert!(ada.saw("lobby (1 of 3)"), "{:?}", ada.rosters);
    let bo = lobby_bot(&addr, "Bo", mode, true);
    wait_until("both to be listed for both", Duration::from_secs(10), || ada.saw("Ada [not ready], Bo [not ready]") && bo.saw("Ada [not ready], Bo [not ready]"));
    // the second seat cannot start the game, and nothing starts by itself with a seat empty
    bo.commands.send(LobbyCommand::Ready(true)).expect("command");
    bo.commands.send(LobbyCommand::Start).expect("command");
    wait_until("Bo to be marked ready", Duration::from_secs(10), || ada.saw("Bo [ready]"));
    assert!(bo.started.recv_timeout(Duration::from_millis(400)).is_err() && ada.started.try_recv().is_err(), "only the first seat can start the game");
    ada.commands.send(LobbyCommand::Ready(true)).expect("command");
    wait_until("Ada to be marked ready", Duration::from_secs(10), || bo.saw("Ada [ready], Bo [ready]"));
    assert!(ada.started.recv_timeout(Duration::from_millis(400)).is_err(), "two of three seats are ready: that is not everybody");
    ada.commands.send(LobbyCommand::Start).expect("command");
    ada.started.recv_timeout(Duration::from_secs(10)).expect("the game started for Ada");
    bo.started.recv_timeout(Duration::from_secs(10)).expect("the game started for Bo");
    let (a, b) = (ada.handle.join().expect("ada").expect("ada's game"), bo.handle.join().expect("bo").expect("bo's game"));
    let report = host.join().expect("host thread").expect("host");
    assert_eq!((a.players, b.players), (2, 2), "the game has the two players that were there, not three");
    assert_eq!((a.player, b.player), (0, 1), "seats in the order of arrival");
    assert_eq!(a.hash, b.hash);
    assert_eq!(report.players, 2);
    assert!(report.desyncs.is_empty());
    assert_eq!(a.hash, scripted_hash(&[mixed, mixed], ticks, delay, SCENE), "{}", mode.name());
}

/// Everybody ready starts the game without anybody saying `start`.
fn everybody_ready_starts_the_game(mode: Mode) {
    let ticks = 90;
    let cfg = lobby_cfg(mode, 2, ticks);
    let delay = cfg.delay;
    let (addr, host) = spawn_host(cfg);
    let ada = lobby_bot(&addr, "Ada", mode, false);
    let bo = lobby_bot(&addr, "Bo", mode, false);
    let (rosters_a, rosters_b) = (ada.rosters.clone(), bo.rosters.clone());
    let (a, b) = (ada.handle.join().expect("ada").expect("ada's game"), bo.handle.join().expect("bo").expect("bo's game"));
    let report = host.join().expect("host thread").expect("host");
    assert_eq!(a.hash, b.hash);
    assert_eq!(a.tick, ticks);
    let said = |r: &Arc<Mutex<Vec<String>>>, text: &str| r.lock().expect("rosters").iter().any(|l| l.contains(text));
    assert!(said(&rosters_a, "Ada [ready]") || said(&rosters_b, "Bo [ready]"), "the roster told everybody who was ready: {rosters_a:?}");
    assert!(said(&rosters_a, "Bo"), "...and who was there: {rosters_a:?}");
    assert!(report.desyncs.is_empty());
    assert_eq!(a.hash, scripted_hash(&[mixed, mixed], ticks, delay, SCENE));
}

#[test]
fn rollback_lobby_the_first_seat_starts_the_game() {
    the_first_seat_starts_the_game(Mode::Rollback);
}

#[test]
fn lockstep_lobby_the_first_seat_starts_the_game() {
    the_first_seat_starts_the_game(Mode::Lockstep);
}

#[test]
fn rollback_lobby_everybody_ready_starts_the_game() {
    everybody_ready_starts_the_game(Mode::Rollback);
}

#[test]
fn lockstep_lobby_everybody_ready_starts_the_game() {
    everybody_ready_starts_the_game(Mode::Lockstep);
}

// ---------------------------------------------------------------------------------------------------------------- refusals

/// Run a join on another thread and return the reason the host gave for refusing it; fails if there is no answer (a hang) or it was accepted.
fn refusal(addr: &str, join: impl FnOnce(&str) -> Result<(), String> + Send + 'static) -> String {
    let (tx, rx) = mpsc::channel();
    let addr = addr.to_string();
    std::thread::spawn(move || {
        let _ = tx.send(join(&addr));
    });
    rx.recv_timeout(Duration::from_secs(10)).expect("a clear refusal within seconds, not a hang").expect_err("the host should have refused")
}

fn quick_host(mode: Mode, lobby: bool, players: usize) -> String {
    let mut cfg = if mode == Mode::Rollback { rb_cfg(players, 50) } else { HostConfig { players, max_ticks: Some(50), ..HostConfig::default() } };
    cfg.lobby = lobby;
    cfg.join_timeout = Duration::from_secs(8); // the host gives up by itself when the test is over
    spawn_host(cfg).0
}

fn lockstep_join(addr: &str) -> Result<(), String> {
    Lockstep::join(addr, "old-client", |w| engine_for(w.seed, SCENE)).map(|_| ())
}

fn v2_join(addr: &str, mode: Mode, watch: bool, token: Option<&str>) -> Result<(), String> {
    let mut o = JoinOptions::new("new-client", mode);
    o.lobby = true; // speak protocol 2 even when asking for lockstep (`lockstep_join` is the protocol-1 client)
    o.watch = watch;
    o.token = token.map(str::to_string);
    connect(addr, o, |w| engine_for(w.seed, SCENE)).map(|_| ())
}

#[test]
fn a_lockstep_client_at_a_rollback_host_is_told_to_use_rollback() {
    let addr = quick_host(Mode::Rollback, false, 2);
    let why = refusal(&addr, lockstep_join);
    assert!(why.contains("rollback game") && why.contains("--rollback"), "{why}");
}

#[test]
fn a_rollback_client_at_a_lockstep_host_is_told_the_host_runs_lockstep() {
    let addr = quick_host(Mode::Lockstep, false, 2);
    let why = refusal(&addr, |a| v2_join(a, Mode::Rollback, false, None));
    assert!(why.contains("lockstep") && why.contains("--rollback"), "{why}");
    // watching and taking a seat back are rollback things too
    let why = refusal(&addr, |a| v2_join(a, Mode::Rollback, true, None));
    assert!(why.contains("lockstep"), "{why}");
}

#[test]
fn a_plain_lockstep_client_at_a_lobby_host_is_told_about_the_lobby() {
    let addr = quick_host(Mode::Lockstep, true, 2);
    let why = refusal(&addr, lockstep_join);
    assert!(why.contains("lobby") && why.contains("--lobby"), "{why}");
}

#[test]
fn a_client_for_one_mode_at_a_host_of_the_other_is_refused_in_both_directions_of_the_lobby_too() {
    let addr = quick_host(Mode::Rollback, true, 2);
    let why = refusal(&addr, |a| v2_join(a, Mode::Lockstep, false, None));
    assert!(why.contains("rollback") && why.contains("lockstep"), "{why}");
    let addr = quick_host(Mode::Lockstep, true, 2);
    let why = refusal(&addr, |a| v2_join(a, Mode::Rollback, false, None));
    assert!(why.contains("lockstep") && why.contains("rollback"), "{why}");
    let why = refusal(&addr, |a| v2_join(a, Mode::Rollback, true, None));
    assert!(why.contains("no spectators"), "{why}");
    let why = refusal(&addr, |a| v2_join(a, Mode::Lockstep, false, Some("not-a-seat")));
    assert!(why.contains("not started"), "{why}");
}

#[test]
fn somebody_who_arrives_after_a_lockstep_game_has_started_is_told_so_instead_of_waiting() {
    let cfg = HostConfig { players: 1, max_ticks: Some(300), join_timeout: Duration::from_secs(20), timeout: Duration::from_secs(20), ..HostConfig::default() };
    let (addr, host) = spawn_host(cfg);
    let progress = Arc::new(AtomicU64::new(0));
    let mut a = BotSpec::new("alone", Mode::Lockstep, calm);
    a.hooks.progress = Some(progress.clone());
    a.hooks.pace = Duration::from_millis(3);
    let a = spawn_bot(addr.clone(), a);
    wait_until("the game to start", Duration::from_secs(10), || progress.load(Ordering::SeqCst) >= 30);
    let why = refusal(&addr, lockstep_join);
    assert!(why.contains("already started"), "{why}");
    let why = refusal(&addr, |a| v2_join(a, Mode::Rollback, true, None));
    assert!(why.contains("already started"), "{why}");
    let a = a.join().expect("a").expect("the game that was running is not disturbed");
    assert_eq!(a.tick, 300);
    host.join().expect("host thread").expect("host");
}

#[test]
fn at_a_running_rollback_game_only_watching_and_a_known_seat_token_get_in() {
    let (addr, host) = spawn_host(rb_cfg(2, 300));
    let a = spawn_bot(addr.clone(), {
        let mut s = BotSpec::new("a", Mode::Rollback, calm);
        s.hooks.pace = Duration::from_millis(3);
        s
    });
    let progress = Arc::new(AtomicU64::new(0));
    let b = spawn_bot(addr.clone(), {
        let mut s = BotSpec::new("b", Mode::Rollback, calm);
        s.hooks.pace = Duration::from_millis(3);
        s.hooks.progress = Some(progress.clone());
        s
    });
    wait_until("the game to start", Duration::from_secs(10), || progress.load(Ordering::SeqCst) >= 20);
    let why = refusal(&addr, |a| v2_join(a, Mode::Rollback, false, None));
    assert!(why.contains("already started") && why.contains("watch"), "{why}");
    let why = refusal(&addr, |a| v2_join(a, Mode::Rollback, false, Some("0123456789abcdef0123456789abcdef")));
    assert!(why.contains("unknown seat token"), "{why}");
    let (a, b) = (a.join().expect("a").expect("bot a"), b.join().expect("b").expect("bot b"));
    assert_eq!(a.hash, b.hash);
    host.join().expect("host thread").expect("host");
}
