//! `lunaris server` and `lunaris swarm`: the netcode for big matches (docs/NETWORK_SCALE.md) over real UDP. The server runs a demo match for up to `--players` clients; the swarm
//! is a crowd of bots that play it, and both report what the network and the server's tick cost.
use crate::net_cmd::number;
use lunaris_api::Engine;
use lunaris_json::Json;
use lunaris_net::scale::client::NetClient;
use lunaris_net::scale::play::NetDriver;
use lunaris_net::scale::secure::{self, ConnectToken};
use lunaris_net::scale::server::{phase, Auth, NetServer, ServerConfig};
use lunaris_net::scale::simgame::SimGame;
use lunaris_net::scale::swarm::{Bot, DemoGame};
use lunaris_net::scale::transport::UdpTransport;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

/// Windows sleeps in steps of 15.6 ms unless a program asks for finer ones: a server that looks at its socket every 15 ms adds that to every round trip, so both commands ask for 1 ms.
#[cfg(windows)]
fn fine_timer() {
    #[link(name = "winmm")]
    extern "system" {
        fn timeBeginPeriod(period: u32) -> u32;
    }
    // SAFETY: a plain call into the Windows multimedia timer with a period in milliseconds; it takes and gives nothing else.
    unsafe {
        timeBeginPeriod(1);
    }
}

#[cfg(not(windows))]
fn fine_timer() {}

/// Set when the operating system asks the server to stop (Ctrl+C, a terminate signal, closing the console): the main loop then ends the way `--seconds` does, with the world saved.
static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Set by the main loop when it has saved and is leaving (a console that is closing waits for it a few seconds).
static STOPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(unix)]
fn install_stop_handler() {
    extern "C" fn on_signal(_sig: i32) {
        STOP.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
    }
    // SAFETY: `signal` with the number of SIGINT (2) or SIGTERM (15) and a handler that only stores to an atomic, which is allowed in a signal handler.
    unsafe {
        signal(2, on_signal as usize);
        signal(15, on_signal as usize);
    }
}

#[cfg(windows)]
fn install_stop_handler() {
    unsafe extern "system" fn on_ctrl(kind: u32) -> i32 {
        STOP.store(true, std::sync::atomic::Ordering::SeqCst);
        // (closing the console, logging off and shutting down end the process when this returns: give the loop a few seconds to save)
        if kind >= 2 {
            for _ in 0..90 {
                if STOPPED.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        1
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
    }
    // SAFETY: a plain call into the console API with a handler that only stores to atomics and sleeps.
    unsafe {
        SetConsoleCtrlHandler(Some(on_ctrl), 1);
    }
}

/// The in-game client of a big-match server: `lunaris play SCENE --connect HOST:PORT [--name NAME]` runs this inside the player (docs/NETWORK_SCALE.md, "In a game").
pub fn connect_driver(addr_text: &str, name: &str, token: Option<ConnectToken>) -> Result<NetDriver<UdpTransport>, String> {
    use std::net::ToSocketAddrs;
    fine_timer();
    let addr = addr_text.to_socket_addrs().map_err(|e| format!("bad address {addr_text}: {e}"))?.next().ok_or_else(|| format!("no address for {addr_text}"))?;
    let local = if addr.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
    let transport = UdpTransport::bind(local).map_err(|e| format!("cannot open a socket: {e}"))?;
    let salt = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos() ^ (d.as_secs() as u32).wrapping_mul(2654435761)) | 1;
    Ok(NetDriver::new(match token {
        Some(t) => NetClient::with_token(transport, addr, t, salt),
        None => NetClient::new(transport, addr, name, salt),
    }))
}

/// The driver of `lunaris play` when the game was not started with `--connect`: online play begun from the game's own menu. The entity with a `NetConnect` (docs/UI.md, "Connecting from a menu") asks for
/// a connection (`go`, the address and name typed in two `UiInput`s) or to leave (`leave`), and is told how it goes (`status`, `message`); the connection itself is the one `--connect` makes.
///
/// `launch_token` is the `--connect-token` text the game was started with (a connect token for a secure server, docs/SECURE.md): it is used when nothing is typed in the `NetConnect`'s `token_input` / `token`.
pub fn menu_driver(launch_token: Option<String>) -> Box<dyn FnMut(&mut lunaris_sim::Sim)> {
    use lunaris_net::scale::play::LinkState;
    use lunaris_sim::components::{NetConnect, UiInput};
    fn say(sim: &mut lunaris_sim::Sim, uid: u64, status: u32, message: &str) {
        let sym = sim.world.strings_mut().intern(message);
        if let Some(e) = sim.world.entity(uid) {
            if let Some(nc) = sim.world.get_mut::<NetConnect>(e) {
                if nc.status != status {
                    nc.status = status;
                }
                if nc.message.0 != sym {
                    nc.message.0 = sym;
                }
            }
        }
    }
    fn text_of(sim: &lunaris_sim::Sim, input: u32, fallback: u32) -> String {
        let typed = (input != 0).then(|| sim.world.entity(input as u64)).flatten().and_then(|e| sim.world.get::<UiInput>(e)).map(|i| i.text.0).filter(|s| *s != 0);
        sim.world.strings().get(typed.unwrap_or(fallback)).trim().to_string()
    }
    let mut driver: Option<NetDriver<UdpTransport>> = None;
    Box::new(move |sim: &mut lunaris_sim::Sim| {
        let mut asked: Option<(u64, NetConnect)> = None;
        let _ = sim.world.for_each_uid::<(&NetConnect,), _>(|_, uid, (nc,)| {
            if asked.as_ref().is_none_or(|(u, _)| uid < *u) {
                asked = Some((uid, *nc));
            }
        });
        let Some((uid, nc)) = asked else {
            if let Some(d) = driver.as_mut() {
                d.tick(sim);
            }
            return;
        };
        if nc.go || nc.leave {
            if let Some(e) = sim.world.entity(uid) {
                if let Some(c) = sim.world.get_mut::<NetConnect>(e) {
                    c.go = false;
                    c.leave = false;
                }
            }
            if let Some(mut d) = driver.take() {
                d.leave(sim);
                d.tick(sim); // (once more, so that the player's `NetPlayer.connected` goes off)
            }
            if nc.leave {
                say(sim, uid, 0, "");
            } else {
                let address = text_of(sim, nc.address_input, nc.address.0);
                let mut name = text_of(sim, nc.name_input, nc.name.0);
                if name.is_empty() {
                    name = "player".to_string();
                }
                // a connect token (typed, or the one the game was started with): the player is whoever it was made for
                let typed = text_of(sim, nc.token_input, nc.token.0);
                let token_text = if typed.is_empty() { launch_token.clone().unwrap_or_default().trim().to_string() } else { typed };
                let token = if token_text.is_empty() { Ok(None) } else { read_token(&token_text).map(Some) };
                if address.is_empty() {
                    say(sim, uid, 5, "Type the server's address (host:port)");
                } else if let Err(e) = &token {
                    say(sim, uid, 5, e);
                } else {
                    let token = token.ok().flatten();
                    if let Some(t) = &token {
                        name = t.name.clone();
                    }
                    match connect_driver(&address, &name, token) {
                        Ok(mut d) => {
                            d.timeout_ms = (nc.timeout.clamp(1.0, 120.0) * 1000.0) as u64;
                            driver = Some(d);
                            say(sim, uid, 1, LinkState::Connecting.message());
                        }
                        Err(e) => say(sim, uid, 5, &e),
                    }
                }
            }
        }
        let Some(d) = driver.as_mut() else { return };
        d.tick(sim);
        let link = d.link;
        match link {
            LinkState::Connecting => say(sim, uid, 1, link.message()),
            LinkState::Connected => say(sim, uid, 2, link.message()),
            LinkState::Refused(_) => say(sim, uid, 3, link.message()),
            LinkState::Lost => say(sim, uid, 4, link.message()),
            LinkState::TimedOut => say(sim, uid, 5, link.message()),
        }
        if !matches!(link, LinkState::Connecting | LinkState::Connected) {
            driver = None;
        }
    })
}

/// The key a server and the tokens it accepts share: a file with 64 hexadecimal digits (`lunaris token --gen-key FILE` makes one).
fn read_key(path: &str) -> Result<[u8; 32], String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read the key file {path}: {e}"))?;
    secure::unhex(text.trim()).and_then(|b| <[u8; 32]>::try_from(b).ok()).ok_or_else(|| format!("{path} is not a key file: it should hold 64 hexadecimal digits (lunaris token --gen-key {path} makes one)"))
}

/// `--secure KEYFILE [--protocol-id N]`: the server lets in only the players who have a connect token made with that key (docs/SECURE.md).
fn auth_from(kv: &HashMap<String, String>) -> Result<Option<Auth>, String> {
    let Some(path) = kv.get("secure") else { return Ok(None) };
    Ok(Some(Auth { key: read_key(path)?, protocol_id: number(kv, "protocol-id", 1)?, unix_now: None }))
}

/// The token of `--connect-token HEX` or `--connect-token @FILE`.
pub fn read_token(text: &str) -> Result<ConnectToken, String> {
    let body = match text.strip_prefix('@') {
        Some(path) => std::fs::read_to_string(path).map_err(|e| format!("cannot read the token file {path}: {e}"))?,
        None => text.to_string(),
    };
    ConnectToken::from_hex(&body).ok_or_else(|| "that is not a connect token (lunaris token --key KEYFILE --name NAME makes one)".to_string())
}

/// `lunaris token --gen-key FILE` makes a key for a server (`lunaris server --secure FILE`); `lunaris token --key FILE --name NAME [--id N] [--ttl SECONDS] [--protocol-id N]` makes a connect
/// token for a player (the text to hand them: `lunaris play SCENE --connect HOST:PORT --connect-token TOKEN`). A login service does the same with its own copy of the key (docs/SECURE.md).
pub fn token(kv: &HashMap<String, String>) -> ExitCode {
    let run = || -> Result<(), String> {
        if let Some(path) = kv.get("gen-key") {
            if std::path::Path::new(path).exists() {
                return Err(format!("{path} exists already: a key is not overwritten"));
            }
            let mut key = [0u8; 32];
            secure::random_bytes(&mut key);
            std::fs::write(path, secure::hex(&key) + "\n").map_err(|e| format!("cannot write {path}: {e}"))?;
            println!("wrote a new key to {path}: keep it secret, give it to the server (--secure {path}) and to whatever makes tokens");
            return Ok(());
        }
        let key = read_key(kv.get("key").ok_or_else(|| "lunaris token needs --key KEYFILE (or --gen-key FILE to make one)".to_string())?)?;
        let name = kv.get("name").ok_or_else(|| "lunaris token needs --name NAME".to_string())?;
        let protocol_id: u64 = number(kv, "protocol-id", 1)?;
        let ttl: u64 = number(kv, "ttl", 300)?;
        // (a player id: 0 means the name's own number, so the same name is the same player)
        let mut id: u64 = number(kv, "id", 0)?;
        if id == 0 {
            id = name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)) | 1;
        }
        let (mut nonce, mut c2s, mut s2c) = ([0u8; 24], [0u8; 32], [0u8; 32]);
        secure::random_bytes(&mut nonce);
        secure::random_bytes(&mut c2s);
        secure::random_bytes(&mut s2c);
        let t = ConnectToken::issue(&key, protocol_id, id, name, secure::unix_now(), ttl, nonce, c2s, s2c);
        eprintln!("a connect token for {name} (player {id}), good for {ttl} s, for one connection:");
        println!("{}", t.to_hex());
        Ok(())
    };
    run().map(|_| ExitCode::SUCCESS).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        ExitCode::FAILURE
    })
}

/// The resources of `--resources NAME=VALUE,NAME=VALUE`, set on the world in one step; how many were set.
fn set_resources(engine: &mut Engine, list: &str) -> Result<usize, String> {
    let mut values: Vec<(String, Json)> = Vec::new();
    for item in list.split(',').map(str::trim).filter(|i| !i.is_empty()) {
        let (name, value) = item.split_once('=').ok_or_else(|| format!("--resources: `{item}` is not NAME=VALUE"))?;
        let number: f64 = value.trim().parse().map_err(|_| format!("--resources: the value of {} must be a number, not `{}`", name.trim(), value.trim()))?;
        values.push((name.trim().to_string(), Json::from(number)));
    }
    if values.is_empty() {
        return Ok(0);
    }
    let n = values.len();
    engine.call("set_resource", &Json::obj([("values", Json::obj(values))])).map_err(|e| format!("--resources: {e}"))?;
    Ok(n)
}

/// `lunaris server --scene SCENE [--players N] [--port P] [--bind ADDR] [--seconds S]`: the scene is the game, run here (docs/GAME_SERVER.md): its own systems, an avatar for every player that
/// connects, what it replicates (`NetEntity`) told to the players, saved as its `NetHost` says. Without `--scene` it is the demo match of [`server`].
fn game_server(kv: &HashMap<String, String>, seed: u64, root: PathBuf, scene: &str) -> Result<ExitCode, String> {
    fine_timer();
    let players: usize = number(kv, "players", 100)?;
    let port: u16 = number(kv, "port", 7777)?;
    let seconds: Option<f64> = match kv.get("seconds") {
        None => None,
        Some(v) => Some(v.parse().map_err(|_| "--seconds needs a number".to_string())?),
    };
    if players == 0 || players > 4000 {
        return Err("--players is 1 to 4000".to_string());
    }
    let bind = kv.get("bind").cloned().unwrap_or_else(|| "127.0.0.1".to_string());
    let mut engine = Engine::new(seed.max(1), root);
    engine.call("load_scene", &Json::obj([("path", Json::str(scene))])).map_err(|e| format!("cannot load {scene}: {e}"))?;
    // (`--resources NAME=VALUE,NAME=VALUE`: resources set once the scene is loaded, for what depends on the machine and the load and not on the game: `nav.cells_per_step=2000`, `stream.loads_per_tick=3`, `procgrid.every=4`)
    if let Some(list) = kv.get("resources") {
        let n = set_resources(&mut engine, list)?;
        println!("{n} resources set from --resources");
    }
    let entities = engine.sim.world.len();
    let transport = UdpTransport::bind(&format!("{bind}:{port}")).map_err(|e| format!("cannot listen on {bind}:{port}: {e}"))?;
    let profile = kv.contains_key("profile");
    // (`--spin`: the server never sleeps between its ticks, it waits for the next one on the processor, which then stays at its full clock: a core that is idle for 20 ms of every 33 is clocked down by the operating
    // system and the tick that follows starts at a third of the speed (the canaries of `--profile` show it). It uses one whole core; for a machine that is the game server and has cores to spare)
    let spin = kv.contains_key("spin");
    let auth = auth_from(kv)?;
    let secure_note = auth.as_ref().map(|a| format!(", players need a connect token (protocol id {})", a.protocol_id)).unwrap_or_default();
    let mut cfg = ServerConfig { max_clients: players, profile, auth, ..ServerConfig::default() };
    // (the rays of the wallhack defence are cut when the ticks take longer than this many milliseconds on average: 24 leaves a tick of 33 ms room to spike in; `--governor 0` never cuts them)
    cfg.occlusion.governor_ms = number(kv, "governor", 24)?;
    // (the packets of the clients are made on several threads at once: `--packet-threads 1` makes them one after the other, 0 (the default) uses the machine's processors up to 8; with fewer than 24 players it is always one)
    cfg.threads = number(kv, "packet-threads", 0)?;
    // (`--max-per-ip N`: at most N players from one IP address (0, the default: no limit); a server on the internet sets a few, see docs/SERVER_ROBUSTNESS.md)
    cfg.max_per_ip = number(kv, "max-per-ip", 0)?;
    let tick_hz = cfg.tick_hz;
    let mut server = NetServer::new(cfg, transport, SimGame::new(engine.sim));
    // (`--profile-sim`: the simulation times its stages and graph systems too, and the slowest and the average of them are printed when the server stops)
    if kv.contains_key("profile-sim") {
        server.game.sim.profile = Some(Default::default());
    }
    println!("Lunaris game server: {scene} ({entities} entities), up to {players} players on UDP {bind}:{port}, {tick_hz} ticks a second{secure_note}");
    if bind == "127.0.0.1" {
        println!("Listening on this machine only; use --bind 0.0.0.0 to let other computers join.");
    }
    if secure_note.is_empty() {
        println!("Join it with: lunaris play {scene} --connect {bind}:{port} --name YOU");
    } else {
        println!("Make a token with: lunaris token --key KEYFILE --name YOU --protocol-id N, then: lunaris play {scene} --connect {bind}:{port} --connect-token TOKEN");
    }
    let start = Instant::now();
    let mut last = Instant::now();
    let mut prev = server.stats.clone();
    let mut prev_game = server.game.stats.clone();
    install_stop_handler();
    // (`--admin DIR`: the admin console, docs/ADMIN.md: the lines appended to DIR/commands.txt are run while the server runs, the answers go to DIR/output.log, the bans are DIR/bans.txt)
    let mut admin = match kv.get("admin") {
        Some(dir) => match lunaris_net::scale::admin::Admin::open(std::path::Path::new(dir), &mut server) {
            Ok(a) => {
                println!("Admin console: append commands to {dir}/commands.txt, read the answers in {dir}/output.log (docs/ADMIN.md)");
                Some(a)
            }
            Err(e) => return Err(format!("cannot open the admin folder {dir}: {e}")),
        },
        None => None,
    };
    loop {
        let now = start.elapsed().as_millis() as u64 + 1;
        server.update(now);
        if let Some(a) = admin.as_mut() {
            a.poll(&mut server, now);
            if a.stop {
                println!("asked to stop by the admin console: saving the world");
                break;
            }
        }
        if STOP.load(std::sync::atomic::Ordering::SeqCst) {
            println!("asked to stop: saving the world");
            break;
        }
        if last.elapsed() >= Duration::from_secs(5) {
            let dt = last.elapsed().as_secs_f64();
            let (log, worst, canaries) = if profile { (server.take_tick_log(), server.take_worst(), server.take_canary_log()) } else { Default::default() };
            if profile && std::env::var("LUNARIS_CENSUS").is_ok() {
                println!("        census: {}", server.game.census());
            }
            let (s, g) = (&server.stats, &server.game.stats);
            let ticks = (s.ticks - prev.ticks).max(1);
            let per = |a: u64, b: u64| (a - b) as f64 / ticks as f64;
            println!(
                "  {:4.0}s  players {:3}  ticks {:.1}/s  tick {:.0} us avg {} us max  out {:.0} KB/s  in {:.0} KB/s  corrections {}  | game: input {:.0} us, simulation {:.0} us, replication {:.0} us a tick, {} things replicated, {} events sent | world: {} entities, {} strings, {} half-open handshakes, {} saves",
                start.elapsed().as_secs_f64(),
                server.client_count(),
                ticks as f64 / dt,
                (s.tick_us_total - prev.tick_us_total) as f64 / ticks as f64,
                s.tick_us_max,
                (s.bytes_out - prev.bytes_out) as f64 / dt / 1000.0,
                (s.bytes_in - prev.bytes_in) as f64 / dt / 1000.0,
                s.corrections - prev.corrections,
                per(g.input_us, prev_game.input_us),
                per(g.step_us, prev_game.step_us),
                per(g.mirror_us, prev_game.mirror_us),
                g.mirrored,
                g.events_sent - prev_game.events_sent,
                server.game.sim.world.len(),
                server.game.sim.world.strings().len(),
                server.pending_len(),
                g.saves,
            );
            if profile {
                let steps: Vec<String> = phase::NAMES.iter().enumerate().map(|(i, n)| format!("{n} {:.0}", (s.phase_ns[i] - prev.phase_ns[i]) as f64 / ticks as f64 / 1000.0)).collect();
                println!("        us a tick: {}", steps.join("  "));
                // (how long the ticks took by the clock and by the processor time of this thread, and what the slowest was made of)
                if !log.is_empty() {
                    let (mut wall, mut cpu): (Vec<u32>, Vec<u32>) = log.iter().copied().unzip();
                    wall.sort_unstable();
                    cpu.sort_unstable();
                    let q = |v: &[u32], f: f64| v[((v.len() - 1) as f64 * f).round() as usize];
                    let over = wall.iter().filter(|&&u| u > 33_333).count();
                    println!(
                        "        tick us by the clock: p50 {} p90 {} p99 {} max {}   by the processor: p50 {} p99 {} max {}   ticks over 33 ms: {} of {}",
                        q(&wall, 0.5),
                        q(&wall, 0.9),
                        q(&wall, 0.99),
                        wall[wall.len() - 1],
                        q(&cpu, 0.5),
                        q(&cpu, 0.99),
                        cpu[cpu.len() - 1],
                        over,
                        wall.len()
                    );
                    let names = phase::NAMES.iter().zip(worst.phase_ns.iter()).map(|(n, ns)| format!("{n} {}", ns / 1000)).collect::<Vec<_>>().join(" ");
                    let parts = worst.parts.iter().map(|(n, us)| format!("{n} {us}")).collect::<Vec<_>>().join(" ");
                    let mut alu: Vec<u32> = canaries.iter().map(|c| c[0]).collect();
                    let mut mem: Vec<u32> = canaries.iter().map(|c| c[1]).collect();
                    alu.sort_unstable();
                    mem.sort_unstable();
                    let med = |v: &[u32]| v.get(v.len() / 2).copied().unwrap_or(0);
                    println!("        slowest tick #{}: {} us ({} us on the processor), {} players: {} | game: {}", worst.tick, worst.us, worst.cpu_us, worst.players, names, parts);
                    if g.saves > 0 || g.save_copy_us_max > 0 {
                        println!("        saves so far {}, the longest the tick was held up by starting one: {} us (the copy of the world; writing it is on a thread of its own)", g.saves, g.save_copy_us_max);
                    }
                    println!("        the defence in the slowest tick: pairs tested {} rays {} kept {} hidden {} no rays {}; the level changed in it: {}", worst.occ[0], worst.occ[1], worst.occ[2], worst.occ[3], worst.occ[4], worst.level_changed);
                    println!(
                        "        canaries (us): core clock p50 {} max {}, memory p50 {} max {}; at the slowest tick: {} and {}",
                        med(&alu) / 1000,
                        alu.last().copied().unwrap_or(0) / 1000,
                        med(&mem) / 1000,
                        mem.last().copied().unwrap_or(0) / 1000,
                        worst.canary_ns[0] / 1000,
                        worst.canary_ns[1] / 1000
                    );
                }
                let (n, o) = (&server.stats, &prev);
                if n.occ_pairs + n.occ_hidden + n.occ_still + n.occ_unaffordable > o.occ_pairs + o.occ_hidden + o.occ_still + o.occ_unaffordable {
                    let a = |x: u64, y: u64| (x - y) as f64 / ticks as f64;
                    println!(
                        "        occlusion a tick: pairs {:.0} rays {:.0} still {:.0} hidden {:.0} unaffordable {:.1}   (the static world gathered {} times, {} of them found it changed, {:.1} ms in all, {} shapes; rays cut for {} packets in all, the share now {:.0}%)",
                        a(n.occ_pairs, o.occ_pairs),
                        a(n.occ_rays, o.occ_rays),
                        a(n.occ_still, o.occ_still),
                        a(n.occ_hidden, o.occ_hidden),
                        a(n.occ_unaffordable, o.occ_unaffordable),
                        g.sight_builds,
                        g.sight_changes,
                        g.sight_us as f64 / 1000.0,
                        g.sight_shapes,
                        n.occ_throttled,
                        server.occlusion_scale() * 100.0
                    );
                }
            }
            prev = server.stats.clone();
            prev_game = server.game.stats.clone();
            server.stats.tick_us_max = 0;
            prev.tick_us_max = 0;
            last = Instant::now();
        }
        if seconds.is_some_and(|t| start.elapsed().as_secs_f64() >= t) {
            break;
        }
        if spin {
            std::hint::spin_loop();
        } else {
            std::thread::sleep(Duration::from_micros(500));
        }
    }
    if let Some(p) = &server.game.sim.profile {
        // (the simulation's own stages, by the time they took in all: the average of a tick of the simulation, and the slowest)
        let mut rows: Vec<_> = p.stages.iter().filter(|(n, _)| !n.contains("(all)")).collect();
        rows.sort_by(|a, b| b.1.total_ms.total_cmp(&a.1.total_ms));
        println!("simulation: {} steps, {:.3} ms a step on average, {:.2} ms the slowest; the stages (ms a step: average, slowest):", p.ticks, p.total_ms / p.ticks.max(1) as f64, p.max_ms);
        for (n, t) in rows.iter().take(45) {
            println!("    {:7.4} {:7.3}  {}", t.total_ms / p.ticks.max(1) as f64, t.max_ms, n);
        }
        println!("steps over {} ms: {} of {}", lunaris_sim::sim::STEP_LIMITS_MS.iter().map(|l| format!("{l}")).collect::<Vec<_>>().join(" / "), p.over.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(" / "), p.ticks);
        if !p.slow.is_empty() {
            println!("the slowest simulation steps over the limit (`LUNARIS_SLOW_STEP_MS`, 30 ms) and what each was made of (ms):");
            for (ms, stages) in &p.slow {
                println!("    {:7.2} ms: {}", ms, stages.iter().map(|(n, t)| format!("{n} {t:.2}")).collect::<Vec<_>>().join(", "));
            }
        }
    }
    if server.game.save_now() {
        println!("world saved");
    }
    STOPPED.store(true, std::sync::atomic::Ordering::SeqCst);
    let s = &server.stats;
    println!("server stopped: {} ticks, {} joins, {} leaves, {} refused, {} saves", s.ticks, s.joins, s.leaves, s.refused, server.game.stats.saves);
    Ok(ExitCode::SUCCESS)
}

/// `lunaris server [--players N] [--port P] [--bind ADDR] [--map HALF_WIDTH_M] [--items N] [--seconds S]`: up to `--players` clients (200 by default) over UDP, a demo match (players
/// spread over the map, loot, a zone that shrinks, shots decided here). With `--scene SCENE`: the game server of docs/GAME_SERVER.md.
pub fn server(kv: &HashMap<String, String>, seed: u64, root: PathBuf, scene: Option<&str>) -> ExitCode {
    if let Some(scene) = scene {
        return game_server(kv, seed, root, scene).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        });
    }
    let run = || -> Result<ExitCode, String> {
        fine_timer();
        let players: usize = number(kv, "players", 200)?;
        let port: u16 = number(kv, "port", 7777)?;
        let map: f32 = number(kv, "map", 1500.0)?;
        let items: usize = number(kv, "items", 1500)?;
        let seconds: Option<f64> = match kv.get("seconds") {
            None => None,
            Some(v) => Some(v.parse().map_err(|_| "--seconds needs a number".to_string())?),
        };
        if players == 0 || players > 4000 {
            return Err("--players is 1 to 4000".to_string());
        }
        // 127.0.0.1 unless asked: opening a game to other machines is for the person running it to decide
        let bind = kv.get("bind").cloned().unwrap_or_else(|| "127.0.0.1".to_string());
        let transport = UdpTransport::bind(&format!("{bind}:{port}")).map_err(|e| format!("cannot listen on {bind}:{port}: {e}"))?;
        let profile = kv.contains_key("profile");
        let cfg = ServerConfig { max_clients: players, profile, auth: auth_from(kv)?, ..ServerConfig::default() };
        let tick_hz = cfg.tick_hz;
        let mut server = NetServer::new(cfg, transport, DemoGame::new(map, items, seed.max(1)));
        let mut loot = lunaris_net::scale::server::World::default();
        server.game.scatter(&mut loot);
        for (_, e) in loot.entities.drain() {
            server.world.spawn_state(e.state);
        }
        println!("Lunaris server: up to {players} players on UDP {bind}:{port}, {tick_hz} ticks a second, a {} m square map with {items} items", map * 2.0);
        if bind == "127.0.0.1" {
            println!("Listening on this machine only; use --bind 0.0.0.0 to let other computers join.");
        }
        println!("Try it with: lunaris swarm {bind}:{port} --count 100 --seconds 30");
        let start = Instant::now();
        let mut last = Instant::now();
        let mut prev = server.stats.clone();
        let (mut prev_shots, mut prev_hits) = (0u64, 0u64);
        loop {
            let now = start.elapsed().as_millis() as u64 + 1;
            server.update(now);
            if last.elapsed() >= Duration::from_secs(5) {
                let dt = last.elapsed().as_secs_f64();
                let s = &server.stats;
                let ticks = (s.ticks - prev.ticks).max(1);
                let clients = server.client_count().max(1) as f64;
                println!(
                    "  {:4.0}s  players {:3}  ticks {:.1}/s  tick {:.0} us avg {} us max  out {:.0} KB/s ({:.1} each)  in {:.0} KB/s  records out {:.0}/s  corrections {}  shots {} hits {}  refused {}",
                    start.elapsed().as_secs_f64(),
                    server.client_count(),
                    ticks as f64 / dt,
                    (s.tick_us_total - prev.tick_us_total) as f64 / ticks as f64,
                    s.tick_us_max,
                    (s.bytes_out - prev.bytes_out) as f64 / dt / 1000.0,
                    (s.bytes_out - prev.bytes_out) as f64 / dt / 1000.0 / clients,
                    (s.bytes_in - prev.bytes_in) as f64 / dt / 1000.0,
                    (s.records_out - prev.records_out) as f64 / dt,
                    s.corrections - prev.corrections,
                    server.game.shots - prev_shots,
                    server.game.hits - prev_hits,
                    s.refused - prev.refused,
                );
                if profile {
                    // where the tick went, microseconds a tick: reading what arrived, the game, the grids, then per client: candidates, ranks, packet, network
                    let steps: Vec<String> = phase::NAMES.iter().enumerate().map(|(i, n)| format!("{n} {:.0}", (s.phase_ns[i] - prev.phase_ns[i]) as f64 / ticks as f64 / 1000.0)).collect();
                    println!("        us a tick: {}", steps.join("  "));
                }
                prev = server.stats.clone();
                prev_shots = server.game.shots;
                prev_hits = server.game.hits;
                server.stats.tick_us_max = 0;
                prev.tick_us_max = 0;
                last = Instant::now();
            }
            if seconds.is_some_and(|t| start.elapsed().as_secs_f64() >= t) {
                break;
            }
            std::thread::sleep(Duration::from_micros(500));
        }
        let s = &server.stats;
        println!("server stopped: {} ticks, {} joins, {} leaves, {} shots, {} hits, {} refused", s.ticks, s.joins, s.leaves, server.game.shots, server.game.hits, s.refused);
        Ok(ExitCode::SUCCESS)
    };
    run().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        ExitCode::FAILURE
    })
}

/// `lunaris swarm HOST:PORT [--count N] [--seconds S] [--map HALF_WIDTH_M] [--fire 0|1] [--near M]`: `--count` bots (100 by default) that join a `lunaris server` over real UDP, walk about,
/// shoot at whoever is near and report what they see and how the network treats them.
pub fn swarm(positional: &[String], kv: &HashMap<String, String>) -> ExitCode {
    let run = || -> Result<ExitCode, String> {
        use std::net::ToSocketAddrs;
        fine_timer();
        let addr_text = positional.first().ok_or_else(|| "swarm needs the server's address: lunaris swarm HOST:PORT".to_string())?;
        let addr = addr_text.to_socket_addrs().map_err(|e| format!("bad address {addr_text}: {e}"))?.next().ok_or_else(|| format!("no address for {addr_text}"))?;
        let count: usize = number(kv, "count", 100)?;
        let seconds: f64 = number(kv, "seconds", 30.0)?;
        let map: f32 = number(kv, "map", 1500.0)?;
        let fire: u32 = number(kv, "fire", 1)?;
        // (how near a player is to count in the gap figure: in a crowd only the nearest twenty are sent at full rate, so use a small number there)
        let near: f32 = number(kv, "near", 30.0)?;
        // (`--first N`: the bots are called bot N, bot N+1, ...: several swarms at once, or one after the other with bots that come and go, need names that do not meet)
        let first: usize = number(kv, "first", 0)?;
        let local = if addr.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
        let mut bots: Vec<Bot<UdpTransport>> = Vec::with_capacity(count);
        // (`--secure KEYFILE [--protocol-id N]`: the server wants connect tokens: each bot makes its own with the key)
        let auth = auth_from(kv)?;
        for i in first..first + count {
            let t = UdpTransport::bind(local).map_err(|e| format!("cannot open a socket: {e}"))?;
            let mut bot = match &auth {
                Some(a) => {
                    let (mut nonce, mut c2s, mut s2c) = ([0u8; 24], [0u8; 32], [0u8; 32]);
                    secure::random_bytes(&mut nonce);
                    secure::random_bytes(&mut c2s);
                    secure::random_bytes(&mut s2c);
                    let token = ConnectToken::issue(&a.key, a.protocol_id, 5000 + i as u64, &format!("bot{i}"), secure::unix_now(), 600, nonce, c2s, s2c);
                    Bot::with_token(t, addr, token, 1000 + i as u64, map, fire != 0)
                }
                None => Bot::new(t, addr, &format!("bot{i}"), 1000 + i as u64, map, fire != 0),
            };
            bot.client.near_m = near;
            bots.push(bot);
        }
        println!("Lunaris swarm: {count} bots joining {addr} for {seconds} s");
        let start = Instant::now();
        let mut last = Instant::now();
        let (mut prev_in, mut prev_out, mut prev_rec) = (0u64, 0u64, 0u64);
        loop {
            let now = start.elapsed().as_millis() as u64 + 1;
            for b in bots.iter_mut() {
                b.step(now);
            }
            if last.elapsed() >= Duration::from_secs(5) {
                let dt = last.elapsed().as_secs_f64();
                let connected = bots.iter().filter(|b| b.client.connected()).count();
                let each = connected.max(1) as f64;
                let bytes_in: u64 = bots.iter().map(|b| b.client.bytes_in).sum();
                let bytes_out: u64 = bots.iter().map(|b| b.client.bytes_out).sum();
                let records: u64 = bots.iter().map(|b| b.client.records_in).sum();
                let rtt: f64 = bots.iter().filter(|b| b.client.connected()).map(|b| b.client.rtt_ms()).sum::<f64>() / each;
                let known: f64 = bots.iter().map(|b| b.client.remotes().len() as f64).sum::<f64>() / each;
                let gap = bots.iter().map(|b| b.stalest_ms).fold(0.0, f64::max);
                println!(
                    "  {:4.0}s  connected {connected:3}/{count}  rtt {rtt:.1} ms  each: in {:.1} KB/s out {:.1} KB/s, {:.0} records/s, knows {known:.0} entities  longest gap of a player within {near:.0} m {gap:.0} ms  corrections {}  hits taken {}",
                    start.elapsed().as_secs_f64(),
                    (bytes_in - prev_in) as f64 / dt / 1000.0 / each,
                    (bytes_out - prev_out) as f64 / dt / 1000.0 / each,
                    (records - prev_rec) as f64 / dt / each,
                    bots.iter().map(|b| b.corrections).sum::<u64>(),
                    bots.iter().map(|b| b.hits_taken).sum::<u64>(),
                );
                (prev_in, prev_out, prev_rec) = (bytes_in, bytes_out, records);
                for b in bots.iter_mut() {
                    b.reset_staleness();
                }
                last = Instant::now();
            }
            if start.elapsed().as_secs_f64() >= seconds {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let connected = bots.iter().filter(|b| b.client.connected()).count();
        let fired: u64 = bots.iter().map(|b| b.fired).sum();
        for b in bots.iter_mut() {
            b.client.disconnect();
            b.client.update(start.elapsed().as_millis() as u64 + 1);
        }
        println!("swarm done: {connected} of {count} bots were connected at the end, {fired} shots fired");
        Ok(ExitCode::SUCCESS)
    };
    run().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunaris_sim::components::NetConnect;

    /// `--resources` sets every named resource in one step, and says what is wrong with a list that is not NAME=VALUE pairs of numbers.
    #[test]
    fn the_resources_flag_sets_each_named_resource_and_refuses_a_bad_list() {
        let mut engine = Engine::new(1, std::env::temp_dir());
        assert_eq!(set_resources(&mut engine, "nav.cells_per_step=2000, stream.loads_per_tick = 3,,").unwrap(), 2);
        assert_eq!(engine.sim.world.resource("nav.cells_per_step"), Some(2000.0));
        assert_eq!(engine.sim.world.resource("stream.loads_per_tick"), Some(3.0));
        assert_eq!(set_resources(&mut engine, "").unwrap(), 0);
        assert!(set_resources(&mut engine, "nav.cells_per_step").unwrap_err().contains("NAME=VALUE"));
        assert!(set_resources(&mut engine, "procgrid.every=often").unwrap_err().contains("must be a number"));
        assert_eq!(engine.sim.world.resource("procgrid.every"), None, "a bad list sets nothing");
    }

    fn status(engine: &mut Engine) -> (u32, String) {
        let mut found: Option<NetConnect> = None;
        let _ = engine.sim.world.for_each_uid::<(&NetConnect,), _>(|_, _, (nc,)| found = Some(*nc));
        let nc = found.expect("the entity with a NetConnect");
        (nc.status, engine.sim.world.strings().get(nc.message.0).to_string())
    }

    fn set_go(engine: &mut Engine, leave: bool) {
        let e = engine.sim.world.entity(6).unwrap();
        let nc = engine.sim.world.get_mut::<NetConnect>(e).unwrap();
        if leave {
            nc.leave = true;
        } else {
            nc.go = true;
        }
    }

    fn set_address(engine: &mut Engine, text: &str) {
        let sym = engine.sim.world.strings_mut().intern(text);
        let e = engine.sim.world.entity(5).unwrap();
        engine.sim.world.get_mut::<lunaris_sim::components::UiInput>(e).unwrap().text.0 = sym;
    }

    /// A Connect button sets `NetConnect.go`; the address is what is typed in the `UiInput`; the entity is told how it goes and a spent attempt can be tried again.
    #[test]
    fn the_menu_driver_connects_when_asked_says_how_it_goes_and_gives_up_after_the_timeout() {
        let mut engine = Engine::new(1, std::env::temp_dir());
        let spawn = |engine: &mut Engine, args: &str| engine.call("spawn", &lunaris_json::parse(args).unwrap()).unwrap();
        spawn(&mut engine, r#"{"uid":5,"components":{"Name":{"value":"Address"},"Ui":{"button":true},"UiInput":{}}}"#);
        spawn(&mut engine, r#"{"uid":6,"components":{"Name":{"value":"Connect"},"NetConnect":{"address_input":5,"timeout":1.0}}}"#);
        let mut driver = menu_driver(None);
        // nothing typed: it says so
        set_go(&mut engine, false);
        driver(&mut engine.sim);
        assert_eq!(status(&mut engine), (5, "Type the server's address (host:port)".to_string()));
        // an address that is none
        set_address(&mut engine, "not an address");
        set_go(&mut engine, false);
        driver(&mut engine.sim);
        let (code, message) = status(&mut engine);
        assert_eq!(code, 5);
        assert!(message.starts_with("bad address"), "{message}");
        // an address where nobody answers: connecting, then no answer after the timeout (one second here)
        set_address(&mut engine, "127.0.0.1:9");
        set_go(&mut engine, false);
        driver(&mut engine.sim);
        assert_eq!(status(&mut engine), (1, "Connecting...".to_string()));
        let t0 = Instant::now();
        while status(&mut engine).0 == 1 && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
            driver(&mut engine.sim);
        }
        assert_eq!(status(&mut engine), (5, "No answer from the server".to_string()));
        assert!(t0.elapsed() >= Duration::from_millis(900), "it asked for about the timeout, not less");
        // leave: back to nothing going on
        set_go(&mut engine, true);
        driver(&mut engine.sim);
        assert_eq!(status(&mut engine), (0, String::new()));
    }

    /// A connect token (pasted in `token_input`, or given at start) is read before connecting: a text that is no token is said so and nothing is started; a real one starts a connection (as the player it names).
    #[test]
    fn the_menu_driver_reads_a_connect_token_typed_or_given_at_start() {
        let mut engine = Engine::new(1, std::env::temp_dir());
        let spawn = |engine: &mut Engine, args: &str| engine.call("spawn", &lunaris_json::parse(args).unwrap()).unwrap();
        spawn(&mut engine, r#"{"uid":5,"components":{"Name":{"value":"Address"},"Ui":{"button":true},"UiInput":{}}}"#);
        spawn(&mut engine, r#"{"uid":7,"components":{"Name":{"value":"Token"},"Ui":{"button":true},"UiInput":{}}}"#);
        spawn(&mut engine, r#"{"uid":6,"components":{"Name":{"value":"Connect"},"NetConnect":{"address_input":5,"token_input":7,"timeout":1.0}}}"#);
        set_address(&mut engine, "127.0.0.1:9");
        let set_token = |engine: &mut Engine, text: &str| {
            let sym = engine.sim.world.strings_mut().intern(text);
            let e = engine.sim.world.entity(7).unwrap();
            engine.sim.world.get_mut::<lunaris_sim::components::UiInput>(e).unwrap().text.0 = sym;
        };
        let key = [7u8; 32];
        let real = ConnectToken::issue(&key, 1, 11, "Ada", secure::unix_now(), 300, [1u8; 24], [2u8; 32], [3u8; 32]).to_hex();
        // a text that is no token: refused with the reason, nothing started
        let mut driver = menu_driver(None);
        set_token(&mut engine, "not a token");
        set_go(&mut engine, false);
        driver(&mut engine.sim);
        let (code, message) = status(&mut engine);
        assert_eq!(code, 5);
        assert!(message.starts_with("that is not a connect token"), "{message}");
        // a real one typed: the connection starts
        set_token(&mut engine, &real);
        set_go(&mut engine, false);
        driver(&mut engine.sim);
        assert_eq!(status(&mut engine), (1, "Connecting...".to_string()));
        set_go(&mut engine, true);
        driver(&mut engine.sim);
        // nothing typed but the game was started with a token: that one is used; a bad one given at start is said so
        set_token(&mut engine, "");
        let mut with_launch = menu_driver(Some(real.clone()));
        set_go(&mut engine, false);
        with_launch(&mut engine.sim);
        assert_eq!(status(&mut engine), (1, "Connecting...".to_string()));
        let mut with_bad_launch = menu_driver(Some("zz".to_string()));
        set_go(&mut engine, false);
        with_bad_launch(&mut engine.sim);
        assert_eq!(status(&mut engine).0, 5);
    }
}
