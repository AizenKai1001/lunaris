//! The program a built game is: it opens one scene in a window and plays it. No console window, no command line needed.
//!
//! A game made with "Build Windows Player" is a folder with this program (renamed to the game's name), a `game.json` next to it
//! (`{"name": ..., "scene": "scenes/main.lunaris.json", "quality": "high", "size": [1280, 720]}`) and the files the scene uses. Double-click
//! the program and it plays. For testing from the repository it also takes a scene on the command line:
//!
//!     lunaris-player examples/orb_dash.lunaris.json [--size WxH] [--quality classic|low|medium|high|ultra] [--mute] [--volume 0..1]
//!     (self-test: --hidden --frames 90 --hold d --report prints the frame count and the state hash and quits; a console is attached when started from one)
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
#[path = "../interp.rs"]
mod interp;

#[cfg(windows)]
#[allow(dead_code)]
#[path = "../liveapi.rs"]
mod liveapi;

#[cfg(windows)]
#[path = "../play.rs"]
mod play;

// the same network options as `lunaris play` (`--join HOST:PORT [--name N] [--rollback] [--watch] [--lobby] [--sim-latency MS] ...`)
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../net_cmd.rs"]
mod net_cmd;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::path::PathBuf;
    attach_console();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exe_dir: PathBuf = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| PathBuf::from("."));
    let config = std::fs::read_to_string(exe_dir.join("game.json")).ok().and_then(|t| lunaris_json::parse(&t).ok());
    let get = |k: &str| config.as_ref().and_then(|c| c.get(k)).and_then(|v| v.as_str()).map(str::to_string);
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let has = |name: &str| args.iter().any(|a| a == name);
    // a scene on the command line (from the repository), or the one the game was built with (next to the program)
    let positional = args.first().filter(|a| !a.starts_with("--")).cloned();
    let (scene, root) = match (positional, get("scene")) {
        (Some(s), _) => (s, std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
        (None, Some(s)) => (s, exe_dir.clone()),
        (None, None) => {
            message("There is no game here: game.json (next to this program) names the scene to play, and it is missing.");
            return std::process::ExitCode::from(2);
        }
    };
    let size = flag("--size")
        .and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))))
        .or_else(|| {
            let a = config.as_ref()?.get("size")?.as_array()?;
            Some((a.first()?.as_u64()? as u32, a.get(1)?.as_u64()? as u32))
        })
        .filter(|(w, h)| (64..=4096).contains(w) && (64..=4096).contains(h))
        .unwrap_or((1280, 720));
    // `--join HOST:PORT` takes a seat in a game that `lunaris host` runs: every other `--key value` is an option of the seat
    let join = match flag("--join") {
        None => None,
        Some(addr) => {
            let mut kv: std::collections::HashMap<String, String> = std::collections::HashMap::new();
            let mut i = 0;
            while i < args.len() {
                match args[i].strip_prefix("--") {
                    Some(k) if ["rollback", "watch", "lobby", "ready"].contains(&k) => {
                        kv.insert(k.to_string(), "1".to_string());
                        i += 1;
                    }
                    Some(k) if args.get(i + 1).is_some_and(|v| !v.starts_with("--")) => {
                        kv.insert(k.to_string(), args[i + 1].clone());
                        i += 2;
                    }
                    _ => i += 1,
                }
            }
            kv.entry("name".to_string()).or_insert_with(|| get("name").unwrap_or_else(|| "player".to_string()));
            let built = net_cmd::join_options(&kv, kv.contains_key("watch")).and_then(|options| net_cmd::through_proxy(&addr, &kv).map(|(addr, proxy)| play::Join { addr, options, proxy }));
            match built {
                Ok(j) => Some(j),
                Err(e) => {
                    message(&e);
                    return std::process::ExitCode::from(2);
                }
            }
        }
    };
    let opts = play::Options {
        scene,
        size,
        hidden: has("--hidden"),
        mute: has("--mute"),
        volume: flag("--volume").and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 4.0)).unwrap_or(1.0),
        frames: flag("--frames").and_then(|f| f.parse().ok()),
        hold: flag("--hold").map(|h| h.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect()).unwrap_or_default(),
        presses: flag("--press").map(|p| play::parse_presses(&p)).unwrap_or_default(),
        save_frame: None,
        report: has("--report"),
        vsync: !has("--novsync"),
        perf: has("--perf"),
        cpu: has("--cpu"),
        quality: flag("--quality").or_else(|| get("quality")).unwrap_or_else(|| "high".to_string()),
        join,
        driver: None,
    };
    let seed = config.as_ref().and_then(|c| c.get("seed")).and_then(|s| s.as_u64()).unwrap_or(1);
    play::run(opts, root, seed, None)
}

/// A program of the Windows subsystem has no console of its own: when it is started from one, write to that one.
#[cfg(windows)]
fn attach_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(pid: u32) -> i32;
        fn GetStdHandle(n: u32) -> isize;
        fn SetStdHandle(n: u32, h: isize) -> i32;
        fn CreateFileW(name: *const u16, access: u32, share: u32, security: *const u8, disposition: u32, flags: u32, template: isize) -> isize;
    }
    // SAFETY: plain Win32 calls with NUL-terminated names and valid arguments; a failure just leaves the handles as they were.
    unsafe {
        if AttachConsole(0xFFFF_FFFF) != 0 {
            // standard output and error that were not redirected now go to the console
            let out: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
            let h = CreateFileW(out.as_ptr(), 0x4000_0000, 3, std::ptr::null(), 3, 0, 0);
            if h != -1 {
                for n in [0xFFFF_FFF5u32, 0xFFFF_FFF4u32] {
                    let cur = GetStdHandle(n);
                    if cur == 0 || cur == -1 {
                        SetStdHandle(n, h);
                    }
                }
            }
        }
    }
}

/// A message box: this program has no console.
#[cfg(windows)]
fn message(text: &str) {
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(0, wide(text).as_ptr(), wide("Lunaris").as_ptr(), 0x10);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("lunaris-player runs on Windows for now");
}
