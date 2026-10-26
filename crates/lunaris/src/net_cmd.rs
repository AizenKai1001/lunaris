//! `lunaris host`, `lunaris join` and `lunaris watch`: multiplayer (docs/NETWORK.md). The host is a relay and runs no game; `join` takes a
//! seat without a window (a bot, a test client, a server-side copy that watches the others); `watch` is a seatless copy that replays the game
//! from tick 0 and follows it live; `lunaris play <scene> --join HOST:PORT` takes a seat in a window.
//!
//! Lockstep is the default. `--rollback` plays with guessed inputs and goes back when a guess was wrong; `--lobby` waits for the players to be
//! ready; `--sim-latency MS --sim-jitter MS --sim-loss PCT` makes the client's network worse on purpose (see `lunaris_net::LinkSim`).
use lunaris_api::Engine;
use lunaris_json::Json;
use lunaris_net::{connect, Host, HostConfig, JoinOptions, LatencyProxy, LinkSim, LobbyCommand, Mode, Seat};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub(crate) fn number<T: std::str::FromStr>(kv: &HashMap<String, String>, key: &str, default: T) -> Result<T, String> {
    match kv.get(key) {
        None => Ok(default),
        Some(v) => v.parse().map_err(|_| format!("--{key} needs a number")),
    }
}

/// The network the client was asked to pretend to have, if any: `--sim-latency MS --sim-jitter MS --sim-loss PCT [--sim-dup PCT --sim-seed N]`.
pub fn link_sim(kv: &HashMap<String, String>) -> Result<Option<LinkSim>, String> {
    let sim = LinkSim {
        latency_ms: number(kv, "sim-latency", 0)?,
        jitter_ms: number(kv, "sim-jitter", 0)?,
        loss_pct: number(kv, "sim-loss", 0.0)?,
        dup_pct: number(kv, "sim-dup", 0.0)?,
        seed: number(kv, "sim-seed", 1)?,
    };
    if sim.loss_pct < 0.0 || sim.loss_pct > 100.0 || sim.dup_pct < 0.0 || sim.dup_pct > 100.0 {
        return Err("--sim-loss and --sim-dup are percentages, 0 to 100".to_string());
    }
    Ok(if sim.is_off() { None } else { Some(sim) })
}

/// Put a latency proxy in front of `addr` when a simulated network was asked for; returns the address to connect to and the proxy (which
/// must stay alive as long as the connection does).
pub fn through_proxy(addr: &str, kv: &HashMap<String, String>) -> Result<(String, Option<LatencyProxy>), String> {
    match link_sim(kv)? {
        None => Ok((addr.to_string(), None)),
        Some(sim) => {
            let proxy = LatencyProxy::start(addr, sim)?;
            println!("simulated network: each message is held back {} ms (+/- {} ms) in each direction, {}% sent again late, {}% twice (TCP never loses data: loss is simulated as lateness)", sim.latency_ms, sim.jitter_ms, sim.loss_pct, sim.dup_pct);
            Ok((proxy.addr().to_string(), Some(proxy)))
        }
    }
}

/// `lunaris host [--players N] [--port P] [--bind ADDR] [--delay D] [--ticks N] [--seed S] [--rollback] [--window W] [--lobby] [--grace S]
/// [--max-log-mb M] [--max-watchers N]`
pub fn host(kv: &HashMap<String, String>, seed: u64) -> ExitCode {
    let run = || -> Result<ExitCode, String> {
        let rollback = kv.contains_key("rollback");
        let lobby = kv.contains_key("lobby");
        let mode = if rollback { Mode::Rollback } else { Mode::Lockstep };
        let players: usize = number(kv, "players", 2)?;
        let port: u16 = number(kv, "port", 7000)?;
        // lockstep waits for the network, so it wants more delay; rollback hides it, so a little is enough
        let delay: u64 = number(kv, "delay", if rollback { 2 } else { 3 })?;
        let window: u64 = number(kv, "window", 8)?;
        let grace: f64 = number(kv, "grace", 3.0)?;
        let max_log_mb: usize = number(kv, "max-log-mb", 64)?;
        let max_watchers: usize = number(kv, "max-watchers", 16)?;
        let max_ticks: Option<u64> = match kv.get("ticks") {
            None => None,
            Some(t) => Some(t.parse().map_err(|_| "--ticks needs a number".to_string())?),
        };
        if !grace.is_finite() || !(0.0..=3600.0).contains(&grace) {
            return Err("--grace is a number of seconds, 0 to 3600".to_string());
        }
        if !rollback && (kv.contains_key("window") || kv.contains_key("grace") || kv.contains_key("max-log-mb") || kv.contains_key("max-watchers")) {
            return Err("--window, --grace, --max-log-mb and --max-watchers belong to rollback games: add --rollback".to_string());
        }
        // 127.0.0.1 unless asked: opening a game to other machines is a decision for the person running it
        let bind = kv.get("bind").cloned().unwrap_or_else(|| "127.0.0.1".to_string());
        let host = Host::bind((bind.as_str(), port)).map_err(|e| format!("cannot listen on {bind}:{port}: {e}"))?;
        let what = if rollback { format!("rollback, window {window} ticks, input delay {delay} ticks") } else { format!("lockstep, input delay {delay} ticks") };
        println!("Lunaris host: waiting for {players} player(s) on {bind}:{} ({what}{}, seed {seed})", host.port(), if lobby { ", with a lobby" } else { "" });
        if bind == "127.0.0.1" {
            println!("Listening on this machine only; use --bind 0.0.0.0 to let other computers join.");
        }
        let flags = format!("{}{}", if rollback { " --rollback" } else { "" }, if lobby { " --lobby" } else { "" });
        println!("Join with: lunaris play <scene> --join {}:{}{flags} --name <you>", if bind == "0.0.0.0" { "<this machine's address>" } else { &bind }, host.port());
        if rollback {
            println!("Watch with: lunaris watch {}:{} <scene>   (a late spectator replays the game from tick 0, then follows it)", if bind == "0.0.0.0" { "<this machine's address>" } else { &bind }, host.port());
        }
        let cfg = HostConfig { players, delay, seed, max_ticks, mode, window, lobby, grace: Duration::from_secs_f64(grace), max_log_bytes: max_log_mb << 20, max_watchers, ..HostConfig::default() };
        let report = host.run(&cfg)?;
        println!("game over after {} ticks: {}", report.ticks, report.ended);
        for e in &report.events {
            println!("  {e}");
        }
        if rollback {
            println!("{} spectator(s) watched, {} player(s) came back to a seat; the input log {}", report.watchers, report.rejoins, if report.log.complete { "was complete" } else { "was cut short to stay inside its memory limit" });
        }
        for d in &report.desyncs {
            println!("DESYNC {d}");
        }
        Ok(if report.desyncs.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
    };
    run().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        ExitCode::from(2)
    })
}

/// What a person types in a lobby: `ready`, `unready`, `start` (first seat only).
fn lobby_commands() -> mpsc::Receiver<LobbyCommand> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut line = String::new();
        while stdin.read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
            let c = match line.trim() {
                "ready" | "r" => Some(LobbyCommand::Ready(true)),
                "unready" | "u" => Some(LobbyCommand::Ready(false)),
                "start" | "s" => Some(LobbyCommand::Start),
                "" => None,
                other => {
                    println!("(in the lobby: type `ready`, `unready` or `start`; `{other}` means nothing here)");
                    None
                }
            };
            if let Some(c) = c {
                if tx.send(c).is_err() {
                    return;
                }
            }
            line.clear();
        }
    });
    rx
}

/// The options every command that joins a game shares: mode, name, input delay, lobby, token.
pub fn join_options(kv: &HashMap<String, String>, watch: bool) -> Result<JoinOptions, String> {
    let name = kv.get("name").cloned().unwrap_or_else(|| if watch { "watcher".to_string() } else { "bot".to_string() });
    let mode = if kv.contains_key("rollback") || watch { Mode::Rollback } else { Mode::Lockstep };
    let mut o = JoinOptions::new(&name, mode);
    o.watch = watch;
    o.token = kv.get("token").cloned();
    o.input_delay = match kv.get("input-delay") {
        None => None,
        Some(d) => Some(d.parse().map_err(|_| "--input-delay needs a number of ticks".to_string())?),
    };
    if o.input_delay.is_some() && mode == Mode::Lockstep {
        return Err("--input-delay belongs to rollback games (lockstep uses the host's --delay): add --rollback".to_string());
    }
    if kv.contains_key("token") && mode == Mode::Lockstep {
        return Err("--token takes a seat back in a rollback game: add --rollback".to_string());
    }
    o.lobby = kv.contains_key("lobby");
    if o.lobby && !watch {
        o.on_roster = Some(Box::new(|r| println!("{}", r.describe())));
        if !kv.contains_key("ready") {
            println!("In the lobby: type `ready` when you are ready (the first seat can type `start`); or join with --ready to be ready at once.");
            o.commands = Some(lobby_commands());
        }
    }
    Ok(o)
}

/// Print what the seats and the game have to say; returns false when the game is over.
fn report_news(seat: &mut Seat) {
    if let Some(d) = seat.take_desync() {
        println!("DESYNC {d}");
    }
    for e in seat.take_events() {
        println!("  {e}");
    }
}

fn finish_line(seat: &Seat, engine: &mut Engine) {
    println!("finished at tick {}: state hash {:016x} ({})", engine.sim.tick, engine.sim.state_hash(), seat.finished().unwrap_or_default());
    if let Ok(r) = engine.call("resources", &Json::Null) {
        println!("resources: {}", r.get("resources").map(|r| r.to_string_lossy()).unwrap_or_default());
    }
    if let Some(s) = seat.rollback_stats() {
        println!("rollback: {} rollbacks, {} ticks played again, deepest {} ticks, {} ticks played on a guess, {} waits for the window", s.rollbacks, s.resimulated_ticks, s.max_depth, s.guessed_ticks, s.stalls);
    }
}

/// `lunaris join HOST:PORT <scene> [--name NAME] [--hold d,space] [--ticks N] [--rollback [--input-delay D] [--token T]] [--lobby [--ready]]
/// [--sim-latency MS --sim-jitter MS --sim-loss PCT]`: a seat without a window. It holds the keys named in `--hold` for the whole game and
/// plays flat out (no waiting for the clock) until the host ends the game or it has played `--ticks`.
pub fn join(positional: &[String], kv: &HashMap<String, String>, root: PathBuf, threads: Option<usize>) -> ExitCode {
    let run = || -> Result<ExitCode, String> {
        let (Some(addr), Some(scene)) = (positional.first(), positional.get(1)) else {
            return Err("`join` needs the host's address and a scene: lunaris join 127.0.0.1:7000 examples/duel.lunaris.json".to_string());
        };
        let stop_at: Option<u64> = match kv.get("ticks") {
            None => None,
            Some(t) => Some(t.parse().map_err(|_| "--ticks needs a number".to_string())?),
        };
        let hold: Vec<String> = kv.get("hold").map(|h| h.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect()).unwrap_or_default();
        let opts = join_options(kv, false)?;
        let name = opts.name.clone();
        let (target, _proxy) = through_proxy(addr, kv)?;
        let (mut seat, mut engine) = connect(&target, opts, |w| {
            let mut engine = Engine::new(w.seed, root.clone());
            if let Some(t) = threads {
                engine.sim.threads = t.clamp(1, 64);
            }
            engine.call("load_scene", &Json::obj([("path", Json::str(scene.clone()))])).map_err(|e| e.to_string())?;
            Ok(engine)
        })?;
        let w = seat.welcome().clone();
        println!("player {} of {} ({name}): the game starts{}", w.player + 1, w.players, if w.rejoin { " (taking the seat back: replaying the game first)" } else { "" });
        if let Some(t) = seat.token() {
            println!("seat token: {t}   (to come back after a drop: --rollback --token {t})");
        }
        for k in &hold {
            seat.set_key(k, 1.0);
        }
        let mut last_progress = Instant::now();
        loop {
            let stepped = seat.advance(&mut engine, 16)?;
            report_news(&mut seat);
            if stop_at.is_some_and(|t| engine.sim.tick >= t) {
                seat.leave("this player reached its tick limit");
                break;
            }
            if stepped == 0 {
                if seat.finished().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            if last_progress.elapsed() > Duration::from_secs(5) {
                println!("  {}", seat.status(&engine));
                last_progress = Instant::now();
            }
        }
        finish_line(&seat, &mut engine);
        Ok(ExitCode::SUCCESS)
    };
    run().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        ExitCode::FAILURE
    })
}

/// `lunaris watch HOST:PORT <scene> [--name NAME] [--ticks N]`: a seatless copy of a rollback game. The host sends it the game's input log from
/// tick 0, it replays that as fast as it can, and then it follows the game live. It never sends input.
pub fn watch(positional: &[String], kv: &HashMap<String, String>, root: PathBuf, threads: Option<usize>) -> ExitCode {
    let run = || -> Result<ExitCode, String> {
        let (Some(addr), Some(scene)) = (positional.first(), positional.get(1)) else {
            return Err("`watch` needs the host's address and a scene: lunaris watch 127.0.0.1:7000 examples/duel.lunaris.json".to_string());
        };
        let stop_at: Option<u64> = match kv.get("ticks") {
            None => None,
            Some(t) => Some(t.parse().map_err(|_| "--ticks needs a number".to_string())?),
        };
        let opts = join_options(kv, true)?;
        let (target, _proxy) = through_proxy(addr, kv)?;
        let (mut seat, mut engine) = connect(&target, opts, |w| {
            let mut engine = Engine::new(w.seed, root.clone());
            if let Some(t) = threads {
                engine.sim.threads = t.clamp(1, 64);
            }
            engine.call("load_scene", &Json::obj([("path", Json::str(scene.clone()))])).map_err(|e| e.to_string())?;
            Ok(engine)
        })?;
        println!("watching: the game is replayed from tick 0, then followed live");
        let mut last_progress = Instant::now();
        loop {
            let stepped = seat.advance(&mut engine, 100_000)?;
            report_news(&mut seat);
            if stop_at.is_some_and(|t| engine.sim.tick >= t) {
                seat.leave("this spectator reached its tick limit");
                break;
            }
            if stepped == 0 {
                if seat.finished().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            if last_progress.elapsed() > Duration::from_secs(5) {
                println!("  {}", seat.status(&engine));
                last_progress = Instant::now();
            }
        }
        finish_line(&seat, &mut engine);
        Ok(ExitCode::SUCCESS)
    };
    run().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        ExitCode::FAILURE
    })
}
