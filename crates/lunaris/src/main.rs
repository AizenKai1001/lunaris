//! The `lunaris` command line.
//!
//!   lunaris serve [--seed N] [--root DIR]        MCP / JSON-RPC server on stdin/stdout (for AI agents and editors)
//!   lunaris call <tool> [JSON] [--scene FILE]    run one tool and print the result
//!   lunaris run <scene> [--steps N] [--seed N] [--twice]   load a scene, step it, print the state hash
//!   lunaris tools | schema                        list the tools / the built-in component schemas
//!   lunaris bench [--entities N] [--ticks N] [--json]      measure the core
//!   lunaris mind <scene> --body UID --scripted FILE|--endpoint URL   let a model play one body (see mind_cmd.rs)
//!   lunaris verify <spec.json>... [--json]       run playtests written as data (see docs/GAMES.md); exit 1 if any expectation fails
//!   lunaris host [--players N] [--port P] [--bind ADDR] [--delay D] [--ticks N] [--rollback] [--lobby]   relay a multiplayer game (see docs/NETWORK.md)
//!   lunaris join HOST:PORT <scene> [--name NAME] [--hold KEYS] [--ticks N] [--rollback]   play it headless (a bot); `play <scene> --join HOST:PORT` in a window
//!   lunaris watch HOST:PORT <scene>                 watch a rollback game from tick 0 (a spectator), then follow it live
//!
//! Every command accepts `--threads N` (worker threads for parallel systems; results never depend on it).
mod bench;
mod gpu_backend;
mod mind_cmd;
mod net_cmd;
#[cfg(any(windows, target_os = "linux"))]
mod play;

use lunaris_api::{serve, tool_defs, Engine};
use lunaris_json::{parse, Json};
use std::io::{stdin, stdout, BufReader};
use std::path::PathBuf;
use std::process::ExitCode;

fn usage() -> &'static str {
    "lunaris - AI-first game engine core\n\nUSAGE:\n  lunaris serve [--seed N] [--root DIR]\n  lunaris call <tool> [JSON-ARGS] [--scene FILE] [--seed N] [--root DIR]\n  lunaris run <scene.json> [--steps N] [--seed N] [--twice]\n  lunaris tools\n  lunaris schema\n  lunaris bench [--entities N] [--ticks N] [--json]\n  lunaris mind <scene.json> --body UID --goal TEXT (--scripted FILE | --endpoint http://host:port/v1 [--model NAME]) [--turns N] [--think-ticks N] [--record FILE] [--transcript FILE]\n  lunaris edit [scene.json]   (the Lunaris Editor, a Windows application)
  lunaris export <scene.json> [--out DIR] [--name NAME] [--no-player] [--overwrite] [--force]   package the game: the scene and every file it uses in a folder that plays by itself (docs/EXPORT.md)\n  lunaris verify <spec.json>... [--json] [--root DIR]   playtests as data: steps, inputs and expectations
  lunaris play <scene.json> [--size WxH] [--mute] [--volume 0..1] [--quality classic|low|medium|high|ultra] [--cpu] [--join HOST:PORT --name NAME [--rollback] [--input-delay D] [--lobby] [--token T] [--watch]]   (a native window drawn by the GPU; --cpu uses the reference renderer; Windows)\n  lunaris host [--players N] [--port P] [--bind ADDR] [--delay D] [--ticks N] [--rollback [--window W] [--grace S]] [--lobby]   relay a multiplayer game: lockstep (default) or rollback (docs/NETWORK.md)\n  lunaris join HOST:PORT <scene.json> [--name NAME] [--hold KEYS] [--ticks N] [--rollback [--input-delay D] [--token T]] [--lobby [--ready]]   take a seat in it without a window (a bot)\n  lunaris watch HOST:PORT <scene.json> [--name NAME] [--ticks N]   watch a rollback game: replay it from tick 0 at full speed, then follow it live\n  (join, watch, play --join) --sim-latency MS --sim-jitter MS --sim-loss PCT   make this client's network worse on purpose (docs/NETWORK.md)\n  (serve, call, edit) --no-gpu | --quality NAME    the render tool draws with the CPU reference renderer only / with this GPU quality by default (high)\n  (any command) --threads N    worker threads for parallel systems; results never depend on it\n  lunaris --version\n"
}

struct Opts {
    positional: Vec<String>,
    seed: u64,
    root: PathBuf,
    scene: Option<String>,
    steps: u64,
    twice: bool,
    entities: usize,
    ticks: u64,
    json: bool,
    threads: Option<usize>,
    kv: std::collections::HashMap<String, String>,
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts { positional: Vec::new(), seed: 1, root: std::env::current_dir().map_err(|e| e.to_string())?, scene: None, steps: 0, twice: false, entities: 1_000_000, ticks: 60, json: false, threads: None, kv: Default::default() };
    let mut i = 0;
    let need = |i: usize, flag: &str| args.get(i + 1).cloned().ok_or_else(|| format!("{flag} needs a value"));
    while i < args.len() {
        match args[i].as_str() {
            "--seed" => { o.seed = need(i, "--seed")?.parse().map_err(|_| "--seed needs an integer".to_string())?; i += 1; }
            "--root" => { o.root = PathBuf::from(need(i, "--root")?); i += 1; }
            "--scene" => { o.scene = Some(need(i, "--scene")?); i += 1; }
            "--steps" => { o.steps = need(i, "--steps")?.parse().map_err(|_| "--steps needs an integer".to_string())?; i += 1; }
            "--entities" => { o.entities = need(i, "--entities")?.parse().map_err(|_| "--entities needs an integer".to_string())?; i += 1; }
            "--ticks" => {
                o.ticks = need(i, "--ticks")?.parse().map_err(|_| "--ticks needs an integer".to_string())?;
                o.kv.insert("ticks".to_string(), args[i + 1].clone()); // `host` and `join` read it from here
                i += 1;
            }
            "--twice" => o.twice = true,
            "--json" => o.json = true,
            "--allow-plugins" => std::env::set_var("LUNARIS_ALLOW_PLUGINS", "1"),
            "--open" | "--hidden" | "--mute" | "--report" | "--cpu" | "--no-gpu" | "--json-schema" | "--no-thinking" | "--play" | "--force" | "--overwrite" | "--no-player" | "--rollback" | "--lobby" | "--ready" | "--watch" => {
                o.kv.insert(args[i].trim_start_matches("--").to_string(), "1".to_string());
            }
            "--threads" => { o.threads = Some(need(i, "--threads")?.parse().map_err(|_| "--threads needs an integer".to_string())?); i += 1; }
            other if other.starts_with("--") => {
                // options only some commands know (for example `mind --body 1`) are kept as key/value pairs
                let key = other.trim_start_matches("--").to_string();
                o.kv.insert(key, need(i, other)?);
                i += 1;
            }
            other => o.positional.push(other.to_string()),
        }
        i += 1;
    }
    Ok(o)
}

fn print_json(v: &Json) {
    println!("{}", v.to_string_pretty().unwrap_or_else(|_| v.to_string_lossy()));
}

fn load_into(engine: &mut Engine, scene: &str) -> Result<(), String> {
    engine.call("load_scene", &Json::obj([("path", Json::str(scene))])).map(|_| ()).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().cloned() else {
        eprint!("{}", usage());
        return ExitCode::from(2);
    };
    if matches!(cmd.as_str(), "--version" | "-V" | "version") {
        println!("lunaris {}", lunaris_api::VERSION);
        return ExitCode::SUCCESS;
    }
    if matches!(cmd.as_str(), "--help" | "-h" | "help") {
        print!("{}", usage());
        return ExitCode::SUCCESS;
    }
    let opts = match parse_opts(&args[1..]) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}\n\n{}", usage());
            return ExitCode::from(2);
        }
    };
    if let Some(t) = opts.threads {
        std::env::set_var("LUNARIS_THREADS", t.to_string());
    }
    match cmd.as_str() {
        "serve" => {
            let mut engine = Engine::new(opts.seed, opts.root.clone());
            gpu_backend::attach(&mut engine, opts.kv.contains_key("no-gpu"), opts.kv.get("quality").map(String::as_str));
            eprintln!("lunaris {} MCP server ready on stdin/stdout (workspace: {})", lunaris_api::VERSION, opts.root.display());
            match serve(&mut engine, BufReader::new(stdin().lock()), stdout().lock()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("io error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        "tools" => {
            for t in tool_defs() {
                println!("{:<18} {}", t.name, t.description);
            }
            ExitCode::SUCCESS
        }
        "schema" => {
            let mut engine = Engine::new(opts.seed, opts.root);
            print_json(&engine.call("schema", &Json::Null).expect("schema"));
            ExitCode::SUCCESS
        }
        "call" => {
            let Some(tool) = opts.positional.first() else {
                eprintln!("error: `call` needs a tool name (see `lunaris tools`)");
                return ExitCode::from(2);
            };
            let args_json = match opts.positional.get(1) {
                Some(text) => match parse(text) {
                    Ok(j) => j,
                    Err(e) => {
                        eprintln!("error: arguments are not valid JSON: {e}");
                        return ExitCode::from(2);
                    }
                },
                None => Json::Obj(Vec::new()),
            };
            let mut engine = Engine::new(opts.seed, opts.root.clone());
            gpu_backend::attach(&mut engine, opts.kv.contains_key("no-gpu"), opts.kv.get("quality").map(String::as_str));
            if let Some(scene) = &opts.scene {
                if let Err(e) = load_into(&mut engine, scene) {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                }
            }
            match engine.call(tool, &args_json) {
                Ok(v) => {
                    print_json(&v);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}", e.to_json().to_string_pretty().unwrap_or_default());
                    ExitCode::FAILURE
                }
            }
        }
        "run" => {
            let Some(scene) = opts.positional.first() else {
                eprintln!("error: `run` needs a scene file");
                return ExitCode::from(2);
            };
            let run_once = || -> Result<(Json, String), String> {
                let mut engine = Engine::new(opts.seed, opts.root.clone());
                load_into(&mut engine, scene)?;
                let stepped = engine.call("step", &Json::obj([("ticks", Json::from(opts.steps as i64))])).map_err(|e| e.to_string())?;
                let hash = stepped.get("hash").and_then(|h| h.as_str()).unwrap_or("").to_string();
                Ok((stepped, hash))
            };
            match run_once() {
                Ok((summary, hash)) => {
                    print_json(&summary);
                    if opts.twice {
                        match run_once() {
                            Ok((_, again)) if again == hash => println!("deterministic: second run produced the same hash {hash}"),
                            Ok((_, again)) => {
                                eprintln!("NOT deterministic: {hash} vs {again}");
                                return ExitCode::FAILURE;
                            }
                            Err(e) => {
                                eprintln!("error: {e}");
                                return ExitCode::FAILURE;
                            }
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        "export" => {
            let Some(scene) = opts.positional.first() else {
                eprintln!("error: `export` needs a scene file");
                return ExitCode::from(2);
            };
            let mut engine = Engine::new(opts.seed, opts.root.clone());
            if let Err(e) = load_into(&mut engine, scene) {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
            let mut args = Json::Obj(Vec::new());
            for key in ["out", "name"] {
                if let Some(v) = opts.kv.get(key) {
                    args.set(key, Json::str(v.clone()));
                }
            }
            for (flag, key, value) in [("overwrite", "overwrite", true), ("force", "force", true), ("no-player", "player", false)] {
                if opts.kv.contains_key(flag) {
                    args.set(key, Json::Bool(value));
                }
            }
            match engine.call("export_game", &args) {
                Ok(v) => {
                    print_json(&v);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}", e.to_json().to_string_pretty().unwrap_or_default());
                    ExitCode::FAILURE
                }
            }
        }
        "bench" => bench::run(opts.entities, opts.ticks, opts.json),
        "edit" => edit_cmd(&opts),
        "verify" => verify_cmd(&opts),
        "play" => play_cmd(&opts),
        "mind" => mind_cmd::run(&opts.positional, &opts.kv, opts.root.clone(), opts.seed),
        "host" => net_cmd::host(&opts.kv, opts.seed),
        "join" => net_cmd::join(&opts.positional, &opts.kv, opts.root.clone(), opts.threads),
        "watch" => net_cmd::watch(&opts.positional, &opts.kv, opts.root.clone(), opts.threads),
        other => {
            eprintln!("error: unknown command `{other}`\n\n{}", usage());
            ExitCode::from(2)
        }
    }
}

/// `lunaris verify spec.json...`: run playtests written as data. Prints one line per expectation and exits 1 if any failed.
fn verify_cmd(opts: &Opts) -> ExitCode {
    if opts.positional.is_empty() {
        eprintln!("error: `verify` needs at least one spec file (for example examples/tests/orb_dash.test.json)");
        return ExitCode::from(2);
    }
    let mut all_ok = true;
    for file in &opts.positional {
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("error: cannot read {file}: {e}");
                return ExitCode::from(2);
            }
        };
        let spec = match parse(&text) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {file} is not valid JSON: {e}");
                return ExitCode::from(2);
            }
        };
        let report = match lunaris_api::run_spec(&spec, &opts.root) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("error: {file}: {e}");
                return ExitCode::from(2);
            }
        };
        let ok = report.get("ok").and_then(|o| o.as_bool()).unwrap_or(false);
        all_ok &= ok;
        if opts.json {
            print_json(&report);
            continue;
        }
        println!("{} ({file})", report.get("name").and_then(|n| n.as_str()).unwrap_or("(unnamed)"));
        for r in report.get("results").and_then(|r| r.as_array()).unwrap_or(&[]) {
            let pass = r.get("ok").and_then(|o| o.as_bool()).unwrap_or(false);
            let what = r.get("what").and_then(|w| w.as_str()).unwrap_or("?");
            let detail = if pass {
                r.get("actual").map(|a| format!("  ({})", a.to_string_lossy())).unwrap_or_default()
            } else {
                format!("  <- {}", r.get("message").and_then(|m| m.as_str()).unwrap_or(""))
            };
            println!("  {} step {:>3}  {}{}", if pass { "PASS" } else { "FAIL" }, r.get("step").and_then(|s| s.as_u64()).unwrap_or(0), what, detail);
        }
        println!("  {} passed, {} failed; {} ticks, final state hash {}", report.get("passed").and_then(|p| p.as_u64()).unwrap_or(0), report.get("failed").and_then(|p| p.as_u64()).unwrap_or(0), report.get("ticks").and_then(|t| t.as_u64()).unwrap_or(0), report.get("hash").and_then(|h| h.as_str()).unwrap_or("?"));
    }
    if all_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `lunaris edit` opens the Lunaris Editor, the native desktop application that sits next to this program.
fn edit_cmd(opts: &Opts) -> ExitCode {
    native_editor(opts)
}

fn native_editor(opts: &Opts) -> ExitCode {
    let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(if cfg!(windows) { "lunaris-editor.exe" } else { "lunaris-editor" })));
    let Some(exe) = exe.filter(|p| p.exists()) else {
        eprintln!("error: the editor program `lunaris-editor` is not next to `lunaris`.
Build it with: cargo build --release -p lunaris-studio");
        return ExitCode::FAILURE;
    };
    let mut cmd = std::process::Command::new(exe);
    if let Some(scene) = opts.positional.first() {
        cmd.arg(scene);
    }
    cmd.arg("--root").arg(&opts.root).arg("--seed").arg(opts.seed.to_string());
    if let Some(q) = opts.kv.get("quality") {
        cmd.arg("--quality").arg(q);
    }
    if opts.kv.contains_key("mute") {
        cmd.arg("--mute");
    }
    match cmd.status() {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: cannot start the editor: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `lunaris play <scene>`: a native window (Windows, and Linux with X11 and a GPU).
#[cfg(any(windows, target_os = "linux"))]
fn play_cmd(opts: &Opts) -> ExitCode {
    let Some(scene) = opts.positional.first().cloned() else {
        eprintln!("error: `play` needs a scene file (for example examples/orb_dash.lunaris.json)");
        return ExitCode::from(2);
    };
    let size = opts
        .kv
        .get("size")
        .and_then(|s| s.split_once('x'))
        .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
        .filter(|(w, h)| (64..=4096).contains(w) && (64..=4096).contains(h))
        .unwrap_or((1280, 720));
    // `--join HOST:PORT` takes a seat (or, with `--watch`, a place among the spectators) in a game that `lunaris host` runs
    let join = match opts.kv.get("join") {
        None => None,
        Some(addr) => {
            let mut kv = opts.kv.clone();
            kv.entry("name".to_string()).or_insert_with(|| "player".to_string());
            let built = net_cmd::join_options(&kv, kv.contains_key("watch")).and_then(|options| net_cmd::through_proxy(addr, &kv).map(|(addr, proxy)| play::Join { addr, options, proxy }));
            match built {
                Ok(j) => Some(j),
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(2);
                }
            }
        }
    };
    let play_opts = play::Options {
        scene,
        size,
        hidden: opts.kv.contains_key("hidden"),
        mute: opts.kv.contains_key("mute"),
        volume: opts.kv.get("volume").and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 4.0)).unwrap_or(1.0),
        frames: opts.kv.get("frames").and_then(|f| f.parse().ok()),
        hold: opts.kv.get("hold").map(|h| h.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect()).unwrap_or_default(),
        save_frame: opts.kv.get("save-frame").map(PathBuf::from),
        report: opts.kv.contains_key("report"),
        cpu: opts.kv.contains_key("cpu"),
        quality: opts.kv.get("quality").cloned().unwrap_or_else(|| "high".to_string()),
        join,
    };
    play::run(play_opts, opts.root.clone(), opts.seed, opts.threads)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn play_cmd(_opts: &Opts) -> ExitCode {
    eprintln!("error: the native window runs on Windows and Linux only for now");
    ExitCode::from(2)
}
