//! The `lunaris` command line.
//!
//!   lunaris serve [--seed N] [--root DIR] [--connect HOST:PORT]        MCP / JSON-RPC server on stdin/stdout (for AI agents and editors)
//!   lunaris call <tool> [JSON] [--scene FILE]    run one tool and print the result
//!   lunaris run <scene> [--steps N] [--seed N] [--twice]   load a scene, step it, print the state hash
//!   lunaris tools | schema                        list the tools / the built-in component schemas
//!   lunaris bench [--entities N] [--ticks N] [--json] [--gpu]      measure the core (with --gpu: the graphics card)
//!   lunaris mind <scene> --body UID --scripted FILE|--endpoint URL   let a model play one body (see mind_cmd.rs)
//!   lunaris verify <spec.json>... [--json]       run playtests written as data (see docs/GAMES.md); exit 1 if any expectation fails
//!   lunaris host [--players N] [--port P] [--bind ADDR] [--delay D] [--ticks N] [--rollback] [--lobby]   relay a multiplayer game (see docs/NETWORK.md)
//!   lunaris join HOST:PORT <scene> [--name NAME] [--hold KEYS] [--ticks N] [--rollback]   play it headless (a bot); `play <scene> --join HOST:PORT` in a window
//!   lunaris watch HOST:PORT <scene>                 watch a rollback game from tick 0 (a spectator), then follow it live
//!
//! Every command accepts `--threads N` (worker threads for parallel systems; results never depend on it).
mod bench;
mod city_cmd;
mod gpu_backend;
mod interp;
mod liveapi;
mod mind_cmd;
mod net_cmd;
mod net_scale_cmd;
#[cfg(any(windows, target_os = "linux"))]
mod play;
mod posetrace;
mod sampler;

use lunaris_api::{serve, tool_defs, Engine};
use lunaris_json::{parse, Json};
use std::io::{stdin, stdout, BufReader};
use std::path::PathBuf;
use std::process::ExitCode;

fn usage() -> &'static str {
    "lunaris - AI-first game engine core\n\nUSAGE:\n  lunaris serve [--seed N] [--root DIR] [--connect HOST:PORT]\n  lunaris call <tool> [JSON-ARGS] [--scene FILE] [--seed N] [--root DIR]\n  lunaris run <scene.json> [--steps N] [--seed N] [--twice]\n  lunaris tools\n  lunaris schema\n  lunaris bench [--entities N] [--ticks N] [--json] [--gpu]\n  lunaris mind <scene.json> --body UID --goal TEXT (--scripted FILE | --endpoint http://host:port/v1 [--model NAME]) [--turns N] [--think-ticks N] [--record FILE] [--transcript FILE]\n  lunaris edit [scene.json]   (the Lunaris Editor, a Windows application)
  lunaris export <scene.json> [--out DIR] [--name NAME] [--no-player] [--overwrite] [--force]   package the game: the scene and every file it uses in a folder that plays by itself (docs/EXPORT.md)\n  lunaris verify <spec.json>... [--json] [--root DIR]   playtests as data: steps, inputs and expectations
  lunaris play <scene.json> [--size WxH] [--mute] [--volume 0..1] [--quality classic|low|medium|high|ultra] [--cpu] [--hands-off] [--desktop] [--control FILE] [--pos X,Y] [--api PORT] [--fps N|unlimited] [--title NAME] [--connect HOST:PORT --name NAME] [--join HOST:PORT --name NAME [--rollback] [--input-delay D] [--lobby] [--token T] [--watch]]   (a native window drawn by the GPU; --cpu uses the reference renderer; Windows)\n  lunaris host [--players N] [--port P] [--bind ADDR] [--delay D] [--ticks N] [--rollback [--window W] [--grace S]] [--lobby]   relay a multiplayer game: lockstep (default) or rollback (docs/NETWORK.md)\n  lunaris join HOST:PORT <scene.json> [--name NAME] [--hold KEYS] [--ticks N] [--rollback [--input-delay D] [--token T]] [--lobby [--ready]]   take a seat in it without a window (a bot)\n  lunaris watch HOST:PORT <scene.json> [--name NAME] [--ticks N]   watch a rollback game: replay it from tick 0 at full speed, then follow it live\n  lunaris server [--players N] [--port P] [--bind ADDR] [--map HALF_WIDTH_M] [--items N] [--seconds S] [--profile]   the server of a big match: 200 players by default, over UDP (docs/NETWORK_SCALE.md)\n  lunaris server --scene SCENE [--players N] [--port P] [--bind ADDR] [--seconds S] [--profile [--profile-sim]] [--governor MS] [--packet-threads N] [--admin DIR] [--spin]   the game server: the scene's own simulation hosted for 100+ players, an avatar each, saved as its NetHost says (docs/GAME_SERVER.md); --profile prints where the tick goes (and --profile-sim the simulation's stages), --governor MS cuts the wallhack defence's rays when the ticks average more than MS (24; 0 never), --packet-threads N makes the clients' packets on N threads at once (0 = the machine's processors up to 8, the default; 1 = one after the other), --spin keeps the server's core busy between ticks (a processor that idles 20 ms in every 33 clocks down, and the next tick starts slow)\n  lunaris swarm HOST:PORT [--count N] [--seconds S] [--map HALF_WIDTH_M] [--fire 0|1] [--near M]   N bots (100 by default) that join such a server and play it, with a report of what the network does\n  lunaris token --gen-key FILE | --key FILE --name NAME [--id N] [--ttl SECONDS] [--protocol-id N]   make a key for a server, or a connect token for a player (docs/SECURE.md); (server, swarm) --secure KEYFILE [--protocol-id N]: only players with a token get in; (play) --connect-token TOKEN|@FILE\n  (join, watch, play --join) --sim-latency MS --sim-jitter MS --sim-loss PCT   make this client's network worse on purpose (docs/NETWORK.md)\n  (serve, call, edit) --no-gpu | --quality NAME    the render tool draws with the CPU reference renderer only / with this GPU quality by default (high)\n  (any command) --threads N    worker threads for parallel systems; results never depend on it\n  lunaris --version\n"
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
            "--open" | "--hidden" | "--mute" | "--report" | "--cpu" | "--no-gpu" | "--gpu" | "--json-schema" | "--no-thinking" | "--play" | "--force" | "--overwrite" | "--no-player" | "--rollback" | "--lobby" | "--ready" | "--watch" | "--novsync" | "--perf" | "--hands-off" | "--desktop" | "--profile" | "--profile-sim" | "--spin" => {
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
    // `lunaris schema | head` closes the pipe early: that is not an error worth a panic
    use std::io::Write;
    let _ = writeln!(stdout(), "{}", v.to_string_pretty().unwrap_or_else(|_| v.to_string_lossy()));
}

fn load_into(engine: &mut Engine, scene: &str) -> Result<(), String> {
    engine.call("load_scene", &Json::obj([("path", Json::str(scene))])).map(|_| ()).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    // (`LUNARIS_SAMPLE=file`: a sampling profiler for the whole command, see sampler.rs; nothing happens without it)
    sampler::begin();
    let code = real_main();
    sampler::end();
    code
}

fn real_main() -> ExitCode {
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
    if !matches!(cmd.as_str(), "tools" | "schema" | "bench" | "host" | "swarm" | "token") && !(cmd == "server" && opts.scene.is_none()) {
        city_cmd::install_if_present(&opts.root, cmd == "server", if matches!(cmd.as_str(), "play" | "server") { opts.scene.as_deref().or(opts.positional.first().map(|s| s.as_str())) } else { None });
    }
    match cmd.as_str() {
        "serve" if opts.kv.contains_key("connect") => {
            // the live game's API (`play --api PORT`) as a stdio server
            let addr = opts.kv["connect"].clone();
            eprintln!("lunaris {} MCP server on stdin/stdout, relaying to the game at {addr}", lunaris_api::VERSION);
            match liveapi::relay(&addr) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
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
            use std::io::Write;
            for t in tool_defs() {
                if writeln!(stdout(), "{:<18} {}", t.name, t.description).is_err() {
                    break; // the reader went away (`| head`)
                }
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
        "bench" => bench::run(opts.entities, opts.ticks, opts.json, opts.kv.contains_key("gpu")),
        "edit" => edit_cmd(&opts),
        "verify" => verify_cmd(&opts),
        "mmbuild" => mmbuild_cmd(&opts),
        "footpaths" => footpaths_cmd(&opts),
        "posetrace" => posetrace::run(&opts.positional, &opts.kv, opts.root.clone(), opts.seed),
        "play" => play_cmd(&opts),
        "mind" => mind_cmd::run(&opts.positional, &opts.kv, opts.root.clone(), opts.seed),
        "host" => net_cmd::host(&opts.kv, opts.seed),
        "server" => net_scale_cmd::server(&opts.kv, opts.seed, opts.root.clone(), opts.scene.as_deref()),
        "swarm" => net_scale_cmd::swarm(&opts.positional, &opts.kv),
        "token" => net_scale_cmd::token(&opts.kv),
        "join" => net_cmd::join(&opts.positional, &opts.kv, opts.root.clone(), opts.threads),
        "watch" => net_cmd::watch(&opts.positional, &opts.kv, opts.root.clone(), opts.threads),
        other => {
            eprintln!("error: unknown command `{other}`\n\n{}", usage());
            ExitCode::from(2)
        }
    }
}

/// `lunaris verify spec.json...`: run playtests written as data. Prints one line per expectation and exits 1 if any failed.
/// `lunaris mmbuild MODEL.glb OUT.mmdb [--prefix NAME] [--rate N]`: the motion matching database of a model's clips (docs/GAMEPLAY.md, "Motion matching").
fn mmbuild_cmd(opts: &Opts) -> ExitCode {
    let (Some(model), Some(out)) = (opts.positional.first(), opts.positional.get(1)) else {
        eprintln!("error: `mmbuild` needs a model and the file to write: lunaris mmbuild MODEL.glb OUT.mmdb [--prefix NAME] [--rate 30]");
        return ExitCode::from(2);
    };
    let bytes = match std::fs::read(model) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: cannot read {model}: {e}");
            return ExitCode::from(2);
        }
    };
    let dir = std::path::Path::new(model).parent().map(std::path::Path::to_path_buf);
    let imported = match lunaris_render::gltf::import(&bytes, dir.as_deref()) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error: {model}: {e}");
            return ExitCode::from(2);
        }
    };
    let Some(rig) = imported.mesh.rig.as_ref() else {
        eprintln!("error: {model} has no skeleton");
        return ExitCode::from(2);
    };
    let mut o = lunaris_render::motion_db::BuildOptions::default();
    if let Some(p) = opts.kv.get("prefix") {
        o.prefix = p.clone();
    }
    if let Some(r) = opts.kv.get("rate").and_then(|r| r.parse::<f32>().ok()).filter(|r| *r >= 5.0 && *r <= 120.0) {
        o.rate = r;
    }
    let t0 = std::time::Instant::now();
    let db = match lunaris_render::motion_db::build(rig, &o) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = std::fs::write(out, db.to_bytes()) {
        eprintln!("error: cannot write {out}: {e}");
        return ExitCode::from(2);
    }
    println!("{out}: {} clips, {} frames at {} a second, {} values a frame, {} KB ({:.1} s)", db.clips.len(), db.frames(), db.rate, lunaris_sim::motionmatch::DIM, db.to_bytes().len() / 1024, t0.elapsed().as_secs_f32());
    let scales: Vec<String> = db.groups.iter().map(|g| format!("{:.3}", g.scale)).collect();
    println!("group deviations: {}", scales.join(" "));
    ExitCode::SUCCESS
}

/// `lunaris footpaths MODEL.glb OUT.json [--prefix NAME] [--rate N] [--left JOINT] [--right JOINT] [--heel M] [--toe M]`: where the feet are in every clip of a model (the `FootIk` component's table, docs/GAMEPLAY.md, "Feet on the ground"): per clip, every 1/rate
/// seconds, the model-space position (x left, y up, z forward, metres) of each foot's joint and whether the foot is planted (within 4 cm of the lowest it gets in the clip).
fn footpaths_cmd(opts: &Opts) -> ExitCode {
    let (Some(model), Some(out)) = (opts.positional.first(), opts.positional.get(1)) else {
        eprintln!("error: `footpaths` needs a model and the file to write: lunaris footpaths MODEL.glb OUT.json [--prefix NAME] [--rate 30]");
        return ExitCode::from(2);
    };
    let bytes = match std::fs::read(model) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: cannot read {model}: {e}");
            return ExitCode::from(2);
        }
    };
    let dir = std::path::Path::new(model).parent().map(std::path::Path::to_path_buf);
    let imported = match lunaris_render::gltf::import(&bytes, dir.as_deref()) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error: {model}: {e}");
            return ExitCode::from(2);
        }
    };
    let Some(rig) = imported.mesh.rig.as_ref() else {
        eprintln!("error: {model} has no skeleton");
        return ExitCode::from(2);
    };
    let prefix = opts.kv.get("prefix").cloned().unwrap_or_default();
    let rate = opts.kv.get("rate").and_then(|r| r.parse::<f32>().ok()).filter(|r| *r >= 5.0 && *r <= 120.0).unwrap_or(30.0);
    let (lname, rname) = (opts.kv.get("left").cloned().unwrap_or_else(|| "foot_l".into()), opts.kv.get("right").cloned().unwrap_or_else(|| "foot_r".into()));
    let (Some(jl), Some(jr)) = (rig.joint_index(&lname), rig.joint_index(&rname)) else {
        eprintln!("error: {model} has no joints called {lname} and {rname}");
        return ExitCode::from(2);
    };
    // the sole in the frame of the foot joint (the rest pose has the foot flat on the ground facing forward: the sole is the plane y = 0 under the joint, from `heel` behind it to `toe` in front of it); `dl` and `dr` say, for
    // every frame, how far below the joint the lowest of heel and toe is (the ankle's height when the foot lies flat, less when the heel or toe is raised)
    let (heel, toe) = (opts.kv.get("heel").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.075), opts.kv.get("toe").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.21));
    let rest = rig.joint_worlds(&[lunaris_render::rig::PoseLayer { clip: None, time: 0.0, looping: false, weight: 1.0, root: None, inertia: None }], &[]);
    let sole_points = |j: usize| {
        let inv = rest[j].inverse_affine();
        let (x, z) = (rest[j].0[12], rest[j].0[14]);
        (inv.transform_point([x, 0.0, z - heel]), inv.transform_point([x, 0.0, z + toe]))
    };
    let (soles_l, soles_r) = (sole_points(jl), sole_points(jr));
    let mut json = format!("{{\"rate\":{rate},\"clips\":{{");
    let mut n = 0;
    for (ci, clip) in rig.clips.iter().enumerate() {
        if !clip.name.starts_with(&prefix) || clip.duration <= 0.0 {
            continue;
        }
        let frames = ((clip.duration * rate).round() as usize).max(1);
        let l = rig.joint_path(ci, jl, frames);
        let r = rig.joint_path(ci, jr, frames);
        let drops = |j: usize, soles: &([f32; 3], [f32; 3])| -> String {
            (0..=frames)
                .map(|k| {
                    let layer = lunaris_render::rig::PoseLayer { clip: Some(ci), time: clip.duration * k as f32 / frames as f32, looping: false, weight: 1.0, root: None, inertia: None };
                    let m = rig.joint_worlds(&[layer], &[])[j];
                    let (h, t) = (m.transform_point(soles.0), m.transform_point(soles.1));
                    format!("{:.3}", m.0[13] - h[1].min(t[1]))
                })
                .collect::<Vec<_>>()
                .join(",")
        };
        let (dl, dr) = (drops(jl, &soles_l), drops(jr, &soles_r));
        let low = |p: &[[f32; 3]]| p.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
        let (ml, mr) = (low(&l), low(&r));
        let pts = |p: &[[f32; 3]]| p.iter().map(|v| format!("[{:.3},{:.3},{:.3}]", v[0], v[1], v[2])).collect::<Vec<_>>().join(",");
        let flags = |p: &[[f32; 3]], m: f32| p.iter().map(|v| if v[1] - m < 0.04 { "1" } else { "0" }).collect::<Vec<_>>().join(",");
        if n > 0 {
            json.push(',');
        }
        json.push_str(&format!("\"{}\":{{\"dur\":{:.4},\"l\":[{}],\"r\":[{}],\"cl\":[{}],\"cr\":[{}],\"dl\":[{}],\"dr\":[{}]}}", clip.name, clip.duration, pts(&l), pts(&r), flags(&l, ml), flags(&r, mr), dl, dr));
        n += 1;
    }
    json.push_str("}}");
    if let Err(e) = std::fs::write(out, json.as_bytes()) {
        eprintln!("error: cannot write {out}: {e}");
        return ExitCode::from(2);
    }
    println!("{out}: {n} clips, {} KB", json.len() / 1024);
    ExitCode::SUCCESS
}

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
    // `--hands-off` (ignore this machine's keyboard, mouse and pads), `--control FILE` (a program plays through a text file) and `--pos X,Y` (where the window goes): see the top of `play.rs`
    if ["hands-off", "desktop", "control", "pos", "api", "fps"].iter().any(|k| opts.kv.contains_key(*k)) {
        let pos = opts.kv.get("pos").and_then(|p| p.split_once(',')).and_then(|(x, y)| Some((x.trim().parse::<i32>().ok()?, y.trim().parse::<i32>().ok()?)));
        let control = opts.kv.get("control").map(PathBuf::from);
        let api = opts.kv.get("api").and_then(|p| p.parse::<u16>().ok());
        // `--fps 144`, `--fps 0` and `--fps unlimited`: the most pictures a second (0: no limit)
        let fps = opts.kv.get("fps").map(|f| if f.eq_ignore_ascii_case("unlimited") { 0.0 } else { f.parse::<f32>().unwrap_or(0.0) });
        let desktop = opts.kv.contains_key("desktop");
        play::set_remote(play::Remote { hands_off: opts.kv.contains_key("hands-off") || control.is_some() || desktop, control, pos, api, fps, desktop });
    }
    // `--connect HOST:PORT [--name NAME]`: play online on a big-match server (`lunaris server`): the local player (a `NetPlayer`) is reported, the others appear as `NetPuppet`s
    // (without `--connect` the game may still go online from its own menu: an entity with a `NetConnect` asks for it, `docs/UI.md`)
    let driver: Option<Box<dyn FnMut(&mut lunaris_sim::Sim)>> = match opts.kv.get("connect") {
        None if join.is_some() => None,
        None => Some(net_scale_cmd::menu_driver(opts.kv.get("connect-token").cloned())),
        Some(addr) => {
            if join.is_some() {
                eprintln!("error: --connect (a big-match server) and --join (a lockstep or rollback host) are two kinds of multiplayer: use one");
                return ExitCode::from(2);
            }
            let name = opts.kv.get("name").cloned().unwrap_or_else(|| "player".to_string());
            // `--connect-token TOKEN` (or `@FILE`): a server that wants connect tokens (`lunaris server --secure KEYFILE`): the player is whoever the token was made for
            let token = match opts.kv.get("connect-token").map(|t| net_scale_cmd::read_token(t)).transpose() {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(2);
                }
            };
            let name = token.as_ref().map_or(name, |t| t.name.clone());
            match net_scale_cmd::connect_driver(addr, &name, token) {
                Ok(mut d) => {
                    println!("connecting to {addr} as {name}");
                    Some(Box::new(move |sim: &mut lunaris_sim::Sim| d.tick(sim)))
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(2);
                }
            }
        }
    };
    // `--title NAME`: the name the window carries (a game that is not the engine's own puts its own here)
    if let Some(name) = opts.kv.get("title").filter(|t| !t.trim().is_empty()) {
        play::set_title(name.trim().to_string());
    }
    let play_opts = play::Options {
        scene,
        size,
        hidden: opts.kv.contains_key("hidden"),
        mute: opts.kv.contains_key("mute"),
        volume: opts.kv.get("volume").and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 4.0)).unwrap_or(1.0),
        frames: opts.kv.get("frames").and_then(|f| f.parse().ok()),
        hold: opts.kv.get("hold").map(|h| h.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect()).unwrap_or_default(),
        // `--press enter@30:4,s@50`: KEY@FIRST_FRAME[:FRAMES] (4 frames when not said)
        presses: opts.kv.get("press").map(|p| play::parse_presses(p)).unwrap_or_default(),
        save_frame: opts.kv.get("save-frame").map(PathBuf::from),
        report: opts.kv.contains_key("report"),
        vsync: !opts.kv.contains_key("novsync") && !opts.kv.contains_key("fps"),
        perf: opts.kv.contains_key("perf"),
        cpu: opts.kv.contains_key("cpu"),
        quality: opts.kv.get("quality").cloned().unwrap_or_else(|| "high".to_string()),
        join,
        driver,
    };
    play::run(play_opts, opts.root.clone(), opts.seed, opts.threads)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn play_cmd(_opts: &Opts) -> ExitCode {
    eprintln!("error: the native window runs on Windows and Linux only for now");
    ExitCode::from(2)
}
