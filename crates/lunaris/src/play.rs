//! `lunaris play`: run a scene in a native window. The keyboard and mouse become `input` resources (the same ones the editor and any
//! agent set), the simulation runs at its fixed rate, every frame is drawn by the GPU renderer (wgpu: Vulkan or DirectX 12) at the
//! window's own resolution in its high-quality mode (or the quality `--quality` names), and events play their sound effects. If the GPU
//! renderer cannot start, or with `--cpu`, the reference renderer draws and GDI blits the picture. Windows only for now; `lunaris edit`
//! works everywhere.
//!
//!   lunaris play examples/vista.lunaris.json [--size 1280x720] [--mute] [--quality high] [--cpu] [--novsync] [--perf]
//!
//! A window waits for the display's refresh (vsync), so on a 60 Hz screen the game never draws faster than 60 pictures a second; `--novsync` lets it run as fast as it can (and shows what the
//! machine could do on a faster screen), `--perf` prints once a second how long the pictures took (average, 95th percentile and slowest, and how that splits into simulation and drawing).
//!   lunaris play scene.json --hidden --frames 120 --hold d --press enter@30:4 --save-frame shot.png --report     (a self-test: no window at all, so no display is needed either; `--press KEY@FRAME[:FRAMES]` presses keys at set frames)
//!
//! Game controllers work too (Xbox-style pads: XInput on Windows, the joystick interface on Linux): `input.gamepad.a`, `input.gamepad.lx` and so on, which an
//! `InputMap` binds (`jump = space, gamepad.a`), and `rumble.low` / `rumble.high` resources shake the pad (Windows). `lunaris-pad` has the names.
//!
//! Looking around: the mouse's motion since the last tick is `input.mouse_dx` / `input.mouse_dy` (pixels, right and down positive), the wheel `input.mouse_wheel`
//! (notches), and a game that writes the resource `cursor.lock` above 0.5 gets the pointer hidden and held in the window (first-person games; Esc still quits). A
//! `MouseLook` component turns by them.
//!
//! Hands-off play: `--hands-off` makes the window ignore this machine's keyboard, mouse and pads (nothing the person at the computer does reaches the game: the pointer is never hidden or held, Esc does not quit,
//! the window does not take the focus when it opens) and `--control FILE` lets a program play instead: the player reads the text file every picture and plays what it says, so a person can play something else on the same
//! machine while the window shows a game that is being played for them (`--pos X,Y` puts the window where it should be, for example on the second monitor). One directive a line, `#` begins a comment, a file that
//! does not name a key means every key is up:
//!   keys w,shift,space       the keys and mouse buttons (`mouse_left`, `mouse_right`) held down
//!   look 120 -40             mouse motion in pixels a second (`input.mouse_dx`, `input.mouse_dy`), held until the file says otherwise
//!   mouse 0.5 0.5            the pointer's position (0..1 across the picture)
//!   tap space 7              a key pressed for four pictures each time the number after it changes (the first read of the file only notes the numbers)
//!   quit                     closes the window
//!
//! Live control by a program (`--api PORT`): the engine's whole tool set as newline-delimited JSON-RPC on 127.0.0.1:PORT, answered between two pictures (`liveapi.rs`): read anything (`get`, `query`, `raycast`,
//! `nearby`, `path`, `render`), play (`input`), change anything (`set`, `spawn`, a teleport), also with `--hands-off`, so that a program, an AI, plays the game while a window shows it and nobody's keyboard is involved.
//! `lunaris serve --connect 127.0.0.1:PORT` is the same port as a stdio MCP server. `--fps N` caps the pictures a second, `--fps unlimited` (or 0) lets the window draw as fast as the machine can (it implies `--novsync`).
//!
//! Esc quits (unless the game sets the resource `app.quit_on_escape` to 0: then Esc is the key `input.escape`, and the game closes itself with `app.quit` above 0), F11 toggles full screen. Mouse position is `input.mouse_x` / `input.mouse_y` (0..1 across the picture, y down; across the safe area of the interface when the game has a `UiScale`) and the
//! buttons are `input.mouse_left` / `input.mouse_right`. A menu can hide the system pointer to draw its own (`cursor.hide` above 0.5), switch full screen with `display.fullscreen` (1 borderless, 0 a window) and the GPU quality with
//! `display.quality` (0 classic, 1 low, 2 medium, 3 high, 4 ultra) and the display's refresh with `display.vsync` (1 wait for it, 0 show at once); these are applied when they change. A game that shows a frame counter sets `perf.want` and reads `perf.fps` and `perf.ms`. The same menu sets the master volume with `display.volume` (0 to 4, 1 as mixed) and the most pictures a
//! second with `display.fps` (0: no limit; `--fps` wins), and keeps its choices between runs: the resources whose names start with `opt.` or `display.` are saved to `settings.json` in the game's folder when the game
//! sets `settings.save` above 0.5 (the engine sets it back to 0), and read from there when the game starts (docs/UI.md, "Settings"). The keys a player rebound (`KeyBind` buttons) are kept in the same file as `bind.ACTION.N` (docs/KEYBINDS.md).
use lunaris_api::{Director, Engine};
use lunaris_audio::{Mixer, Output};
use lunaris_json::Json;
use lunaris_render::{Frame, TextureCache};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
#[cfg(windows)]
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{CursorGrabMode, Fullscreen, Window, WindowId};

/// Playing over the network: where to connect, how, and (when the network is simulated) the proxy that makes it worse.
pub struct Join {
    /// The host's address, or the address of the latency proxy in front of it.
    pub addr: String,
    pub options: lunaris_net::JoinOptions,
    /// Keeps the simulated network alive for as long as the game lasts.
    pub proxy: Option<lunaris_net::LatencyProxy>,
}

/// Hands-off play (`--hands-off`, `--control FILE`, `--pos X,Y`): see the top of this file. A program that embeds the player and wants it sets this once before `run`, so `Options` does not change.
pub struct Remote {
    /// Ignore this machine's keyboard, mouse and pads.
    pub hands_off: bool,
    /// The text file a program plays through.
    pub control: Option<PathBuf>,
    /// Where the window's top left corner goes, in pixels of the whole desktop.
    pub pos: Option<(i32, i32)>,
    /// The port of the live API (`--api`): the engine's tools as JSON-RPC on 127.0.0.1, see `liveapi.rs`.
    pub api: Option<u16>,
    /// The most pictures a second (`--fps`): 0 or less is no limit.
    pub fps: Option<f32>,
    /// A window that is part of the desktop (`--desktop`): no title bar, below every other window, never takes the focus, and on X11 a window of the desktop type (the task bar and the pager leave it out); the keyboard, mouse and pads are ignored like in
    /// hands-off play. For a display that is always on, such as a dashboard drawn by the engine, put at a place with `--pos` and `--size`.
    pub desktop: bool,
}

static REMOTE: std::sync::OnceLock<Remote> = std::sync::OnceLock::new();

pub fn set_remote(r: Remote) {
    let _ = REMOTE.set(r);
}

static TITLE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The name the window carries in front of the scene's path (`--title NAME`, "Lunaris" when none is set): a game that is not the engine's own puts its own name here, so that its window,
/// the task bar and the recordings say what it is. A program that embeds the player sets it once before `run`, so `Options` does not change.
pub fn set_title(name: String) {
    let _ = TITLE.set(name);
}

fn title() -> &'static str {
    TITLE.get().map_or("Lunaris", |t| t.as_str())
}

/// What a control file says (hands-off play): the keys held, the look rate, the pointer, the numbers of the taps, whether to quit.
#[derive(Default, Debug, PartialEq)]
pub struct Control {
    pub keys: BTreeSet<String>,
    pub look: (f32, f32),
    pub mouse: Option<(f32, f32)>,
    pub taps: Vec<(String, i64)>,
    pub quit: bool,
}

/// Read a control file: one directive a line (`keys`, `buttons`, `look`, `mouse`, `tap`, `quit`), `#` begins a comment, commas and spaces separate the words; what it does not know it skips.
pub fn parse_control(text: &str) -> Control {
    let mut c = Control::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut words = line.split(|ch: char| ch.is_whitespace() || ch == ',').filter(|w| !w.is_empty());
        let Some(cmd) = words.next() else { continue };
        let rest: Vec<&str> = words.collect();
        let num = |i: usize| rest.get(i).and_then(|w| w.parse::<f32>().ok()).filter(|v| v.is_finite());
        match cmd {
            "keys" | "buttons" => c.keys.extend(rest.iter().map(|k| k.to_lowercase())),
            "look" => c.look = (num(0).unwrap_or(0.0), num(1).unwrap_or(0.0)),
            "mouse" => {
                if let (Some(x), Some(y)) = (num(0), num(1)) {
                    c.mouse = Some((x, y));
                }
            }
            "tap" => {
                if let (Some(key), Some(n)) = (rest.first(), num(1)) {
                    c.taps.push((key.to_lowercase(), n as i64));
                }
            }
            "quit" => c.quit = true,
            _ => {}
        }
    }
    c
}

/// What the control file held when it was last read and what the window has done with it.
#[derive(Default)]
struct RemoteState {
    path: Option<PathBuf>,
    seen: Option<(Option<std::time::SystemTime>, u64)>,
    first: bool,
    keys: BTreeSet<String>,
    look: (f32, f32),
    taps: std::collections::BTreeMap<String, (i64, u32)>,
    quit: bool,
}

pub struct Options {
    pub scene: String,
    pub size: (u32, u32),
    pub hidden: bool,
    pub mute: bool,
    /// Master volume of the sound, 0 to 4 (1 = as mixed).
    pub volume: f32,
    pub frames: Option<u64>,
    pub hold: Vec<String>,
    /// Keys pressed by a self-test at a given frame: (key, first frame, frames held). `--press enter@30:4,s@50` presses Enter at frame 30 for 4 frames and S at frame 50 for 4.
    pub presses: Vec<(String, u64, u64)>,
    pub save_frame: Option<PathBuf>,
    pub report: bool,
    /// Wait for the display's refresh before showing a picture (on by default; `--novsync` turns it off).
    pub vsync: bool,
    /// Print the frame times once a second (`--perf`).
    pub perf: bool,
    /// Draw with the reference CPU renderer even when a GPU is available.
    pub cpu: bool,
    /// The GPU renderer's quality: classic, low, medium, high or ultra.
    pub quality: String,
    /// Play over the network: take a seat in a `lunaris host` game (lockstep or rollback: every player's copy plays the same inputs), or
    /// watch one.
    pub join: Option<Join>,
    /// A program that embeds the player can run its own code before every tick of the simulation (in the window and in a hidden self-test alike): it gets the
    /// simulation, so it can read the world and write to it, for example to drive creatures whose brains live outside the engine. `lunaris play` has none.
    pub driver: Option<Box<dyn FnMut(&mut lunaris_sim::Sim)>>,
}

/// `--press enter@30:4,s@50`: KEY@FIRST_FRAME[:FRAMES] (4 frames when not said): the keys a self-test presses and when.
pub fn parse_presses(text: &str) -> Vec<(String, u64, u64)> {
    text.split(',')
        .filter_map(|item| {
            let (key, when) = item.trim().split_once('@')?;
            let (first, len) = when.split_once(':').unwrap_or((when, "4"));
            Some((key.trim().to_string(), first.trim().parse::<u64>().ok()?, len.trim().parse::<u64>().ok()?.max(1))).filter(|(k, _, _)| !k.is_empty())
        })
        .collect()
}

/// Does Esc close the window? Yes, unless the game has set the resource `app.quit_on_escape` to 0: then Esc is the key `input.escape` (a pause menu, a back button) and
/// the game closes itself with the resource `app.quit` (a Quit button).
fn escape_quits(engine: &Engine) -> bool {
    engine.sim.world.resource("app.quit_on_escape").is_none_or(|v| v > 0.5)
}

/// The name of a key as the games read it: `input.<name>`.
pub fn key_name(code: KeyCode) -> Option<String> {
    use KeyCode::*;
    let s = match code {
        KeyA => "a", KeyB => "b", KeyC => "c", KeyD => "d", KeyE => "e", KeyF => "f", KeyG => "g", KeyH => "h", KeyI => "i", KeyJ => "j",
        KeyK => "k", KeyL => "l", KeyM => "m", KeyN => "n", KeyO => "o", KeyP => "p", KeyQ => "q", KeyR => "r", KeyS => "s", KeyT => "t",
        KeyU => "u", KeyV => "v", KeyW => "w", KeyX => "x", KeyY => "y", KeyZ => "z",
        Digit0 => "0", Digit1 => "1", Digit2 => "2", Digit3 => "3", Digit4 => "4", Digit5 => "5", Digit6 => "6", Digit7 => "7", Digit8 => "8", Digit9 => "9",
        Space => "space", Enter => "enter", Tab => "tab", Backspace => "backspace",
        ArrowLeft => "arrowleft", ArrowRight => "arrowright", ArrowUp => "arrowup", ArrowDown => "arrowdown",
        ShiftLeft | ShiftRight => "shift", ControlLeft | ControlRight => "control", AltLeft | AltRight => "alt",
        F1 => "f1", F2 => "f2", F3 => "f3", F4 => "f4", F5 => "f5", F6 => "f6", F7 => "f7", F8 => "f8", F9 => "f9", F10 => "f10", F12 => "f12",
        // (the rest of the keyboard, for games that let the player rebind: `lunaris_sim::rebind::BINDABLE` has every name)
        Minus => "minus", Equal => "equal", BracketLeft => "bracketleft", BracketRight => "bracketright", Semicolon => "semicolon", Quote => "quote", Backquote => "backquote", Backslash => "backslash",
        Comma => "comma", Period => "period", Slash => "slash", CapsLock => "capslock", Insert => "insert", Delete => "delete", Home => "home", End => "end", PageUp => "pageup", PageDown => "pagedown",
        Numpad0 => "numpad0", Numpad1 => "numpad1", Numpad2 => "numpad2", Numpad3 => "numpad3", Numpad4 => "numpad4", Numpad5 => "numpad5", Numpad6 => "numpad6", Numpad7 => "numpad7", Numpad8 => "numpad8", Numpad9 => "numpad9",
        NumpadAdd => "numpadadd", NumpadSubtract => "numpadsubtract", NumpadMultiply => "numpadmultiply", NumpadDivide => "numpaddivide", NumpadDecimal => "numpaddecimal", NumpadEnter => "numpadenter",
        _ => return None,
    };
    Some(s.to_string())
}

/// What a key press means to a text field that has the focus (docs/UI.md, "Typing").
#[derive(Debug, PartialEq)]
enum Typing {
    /// The characters the key makes (letters, digits, punctuation, space; what the keyboard layout says, not the key's name).
    Text(String),
    /// Ctrl+V.
    Paste,
    /// Ctrl+Backspace.
    Clear,
}

/// The typing in a key press, from the text the system gives it and the keys held. Ctrl with a letter is a shortcut, not text (Ctrl+V pastes, Ctrl+Backspace clears the field);
/// Ctrl with Alt is AltGr, which makes characters on many layouts (`@`, `{`, a backslash).
fn typing_of(text: Option<&str>, code: KeyCode, ctrl: bool, alt: bool) -> Option<Typing> {
    if ctrl && !alt {
        return match code {
            KeyCode::KeyV => Some(Typing::Paste),
            KeyCode::Backspace => Some(Typing::Clear),
            _ => None,
        };
    }
    let text: String = text?.chars().filter(|c| !c.is_control()).collect();
    (!text.is_empty()).then_some(Typing::Text(text))
}

#[cfg(windows)]
mod clip {
    //! The text on the clipboard (no extra crates): the Win32 calls a paste needs.
    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(hwnd: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> isize;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalLock(mem: isize) -> *const u16;
        fn GlobalUnlock(mem: isize) -> i32;
    }

    /// The clipboard's text, at most 64 K characters, or None when it holds none.
    pub fn text() -> Option<String> {
        const CF_UNICODETEXT: u32 = 13;
        // SAFETY: the clipboard is opened and closed in this call; the handle `GetClipboardData` returns is owned by the clipboard and is locked only while the text is copied out;
        // the copy stops at the terminating zero or after 65536 units, so it never reads past a well-formed clipboard string.
        unsafe {
            if OpenClipboard(0) == 0 {
                return None;
            }
            let mut out = None;
            let handle = GetClipboardData(CF_UNICODETEXT);
            if handle != 0 {
                let p = GlobalLock(handle);
                if !p.is_null() {
                    let mut n = 0usize;
                    while n < 65536 && *p.add(n) != 0 {
                        n += 1;
                    }
                    out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                    GlobalUnlock(handle);
                }
            }
            CloseClipboard();
            out
        }
    }
}

/// No clipboard off Windows (a hidden or headless run has none to paste).
#[cfg(not(windows))]
mod clip {
    pub fn text() -> Option<String> {
        None
    }
}

#[cfg(windows)]
mod gdi {
    //! Blit an RGBA picture to a window with GDI (no extra crates): StretchDIBits scales it to the window.
    #[repr(C)]
    struct BitmapInfoHeader {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bit_count: u16,
        compression: u32,
        size_image: u32,
        x_ppm: i32,
        y_ppm: i32,
        clr_used: u32,
        clr_important: u32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetDC(hwnd: isize) -> isize;
        fn ReleaseDC(hwnd: isize, hdc: isize) -> i32;
    }
    #[link(name = "gdi32")]
    extern "system" {
        fn SetStretchBltMode(hdc: isize, mode: i32) -> i32;
        fn StretchDIBits(hdc: isize, xd: i32, yd: i32, wd: i32, hd: i32, xs: i32, ys: i32, ws: i32, hs: i32, bits: *const u8, info: *const BitmapInfoHeader, usage: u32, rop: u32) -> i32;
    }

    /// `bgra` holds `w * h` pixels, top row first.
    pub fn blit(hwnd: isize, bgra: &[u8], w: u32, h: u32, to_w: u32, to_h: u32) {
        if bgra.len() < (w * h * 4) as usize || w == 0 || h == 0 || to_w == 0 || to_h == 0 {
            return;
        }
        let info = BitmapInfoHeader { size: std::mem::size_of::<BitmapInfoHeader>() as u32, width: w as i32, height: -(h as i32), planes: 1, bit_count: 32, compression: 0, size_image: 0, x_ppm: 0, y_ppm: 0, clr_used: 0, clr_important: 0 };
        // SAFETY: `hwnd` is the live window handle from winit; `bgra` is a full w x h 32-bit image and `info` describes exactly that;
        // the device context is released before returning.
        unsafe {
            let dc = GetDC(hwnd);
            if dc == 0 {
                return;
            }
            SetStretchBltMode(dc, 3); // COLORONCOLOR: nearest neighbour, crisp pixels
            StretchDIBits(dc, 0, 0, to_w as i32, to_h as i32, 0, 0, w as i32, h as i32, bgra.as_ptr(), &info, 0, 0x00CC_0020);
            ReleaseDC(hwnd, dc);
        }
    }
}

/// The settings menu's choices are the resources named `opt.*` (the game's own: field of view, sensitivity, ...) and `display.*` (the engine's: quality, full screen, volume, frame cap), kept in `settings.json`.
fn settings_keep(name: &str) -> bool {
    name.starts_with("opt.") || name.starts_with("display.")
}

/// Read `settings.json` into resources (a missing or unreadable file changes nothing).
fn load_settings(engine: &mut Engine, path: &std::path::Path) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    match lunaris_json::parse(&text) {
        Ok(json) => {
            for (name, value) in json.as_object().unwrap_or(&[]) {
                if let (true, Some(v)) = (settings_keep(name), value.as_f64().filter(|v| v.is_finite())) {
                    let _ = engine.sim.world.set_resource(name, Some(v));
                }
                // (the text of a field that remembers, `UiInput.remember`: kept as `text.NAME`)
                if let (Some(key), Some(text)) = (name.strip_prefix("text."), value.as_str()) {
                    lunaris_sim::ui_kit::restore_remembered(&mut engine.sim.world, key, text);
                }
                // (a key the player rebound, `KeyBind`: `bind.ACTION.N` is the Nth key of the action)
                if let (Some((action, n)), Some(key)) = (name.strip_prefix("bind.").and_then(|r| r.rsplit_once('.')), value.as_str()) {
                    if let Ok(n) = n.parse::<usize>() {
                        lunaris_sim::rebind::restore(&mut engine.sim.world, action, n, key);
                    }
                }
            }
        }
        Err(e) => eprintln!("note: {} is not read ({e:?})", path.display()),
    }
}

/// Write the settings resources to `settings.json` and put `settings.save` back to 0 (the game asked for it by setting it above 0.5).
fn save_settings(engine: &mut Engine, path: &std::path::Path) -> Result<(), String> {
    let mut pairs: Vec<(String, Json)> = engine.sim.world.resources().iter().filter(|(k, v)| settings_keep(k) && v.is_finite()).map(|(k, v)| (k.clone(), Json::Float(*v))).collect();
    // the texts of the fields that remember (an address, a name): the last field of a name wins
    for (name, _, text) in lunaris_sim::ui_kit::remembered_texts(&mut engine.sim.world) {
        pairs.retain(|(k, _)| *k != format!("text.{name}"));
        pairs.push((format!("text.{name}"), Json::str(&text)));
    }
    // the keys the player rebound (only what differs from the scene)
    for (action, n, key) in lunaris_sim::rebind::changed(&mut engine.sim.world) {
        pairs.push((format!("bind.{action}.{n}"), Json::str(&key)));
    }
    let text = Json::Obj(pairs).to_string_pretty().map_err(|e| format!("{e:?}"))?;
    let written = std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()));
    let _ = engine.sim.world.set_resource("settings.save", Some(0.0));
    written
}

/// The remembered text fields as (name, text): what is compared to see that one has changed since the settings were last written.
fn remembered_now(engine: &mut Engine) -> Vec<(String, String)> {
    lunaris_sim::ui_kit::remembered_texts(&mut engine.sim.world).into_iter().map(|(n, _, t)| (n, t)).collect()
}

/// Keep the text fields that remember between runs: when one has changed and none is being typed in any more, the settings are written (once, when the player is done with it: a click elsewhere, Enter).
fn keep_texts(engine: &mut Engine, path: &std::path::Path, last: &mut Vec<(String, String)>) {
    if lunaris_sim::ui_kit::has_focused_input(&mut engine.sim.world) {
        return;
    }
    let now = remembered_now(engine);
    if now != *last {
        if let Err(e) = save_settings(engine, path) {
            eprintln!("note: the settings were not saved: {e}");
        }
        *last = now;
    }
}

/// The sound director and the output it plays into (dropping it closes the device).
struct Sound {
    director: Director,
    _output: Output,
    /// The mixer the director plays into: the settings menu's `display.volume` sets its master volume.
    mixer: Arc<Mutex<Mixer>>,
}

/// The size a picture of the reference renderer is drawn at for a window of `size`: the window's own, but never more than 1280 x 720 (a window blits it up).
fn capped_size(size: (u32, u32)) -> (u32, u32) {
    let (w, h) = (size.0.max(2) as f32, size.1.max(2) as f32);
    let k = (1280.0 / w).min(720.0 / h).min(1.0);
    (((w * k) as u32).max(16), ((h * k) as u32).max(16))
}

/// The keys a self-test presses or lets go of at frame `at`: `--press KEY@FRAME[:FRAMES]` (the key goes down at its frame and up `FRAMES` frames later).
fn scripted_keys(presses: &[(String, u64, u64)], at: u64) -> Vec<(String, bool)> {
    presses.iter().flat_map(|(k, first, len)| [(at == *first).then(|| (k.clone(), true)), (at == first + len).then(|| (k.clone(), false))]).flatten().collect()
}

/// What `--report` prints at the end of a self-test.
fn print_report(engine: &mut Engine, frames: u64, presented: Option<u64>, sound: Option<&Sound>) {
    let resources = engine.call("resources", &Json::Null).ok().and_then(|r| r.get("resources").cloned()).unwrap_or(Json::Null);
    println!("frames {}, tick {}, state hash {:016x}", frames, engine.sim.tick, engine.sim.state_hash());
    if let Some(n) = presented {
        println!("presented to the window: {n} pictures");
    }
    println!("resources {}", resources.to_string_compact().unwrap_or_default());
    if let Some(s) = sound {
        let started: Vec<String> = s.director.stats.started.iter().map(|(k, v)| format!("{k} x{v}")).collect();
        println!("sound: {} voices started ({}), {} traces, {} room analyses, room rt60 {:.2} s{}", s.director.stats.started.values().sum::<u64>(), started.join(", "), s.director.stats.traces, s.director.stats.room_analyses, s.director.room.rt60, if s.director.warnings.is_empty() { String::new() } else { format!(", warnings: {}", s.director.warnings.join("; ")) });
    }
}

/// Save a picture for `--save-frame`.
fn save_png(path: &std::path::Path, png: Result<Vec<u8>, String>) {
    match png.and_then(|png| std::fs::write(path, png).map_err(|e| e.to_string())) {
        Ok(()) => println!("saved the last frame to {}", path.display()),
        Err(e) => eprintln!("could not save the frame: {e}"),
    }
}

/// A self-test without a window (`--hidden` with `--frames`, one player): the simulation steps one tick a frame exactly as it does in a window, the keys of `--hold` and `--press` arrive at
/// the same frames, the sound director hears the events, and the last picture (`--save-frame`) is drawn offscreen, by the GPU renderer's headless adapter or, with `--cpu` or where there is no
/// GPU, by the reference renderer. It opens no window and so needs no display server: it runs over ssh, in a container and in CI.
fn run_headless(mut opts: Options, mut engine: Engine, mut sound: Option<Sound>, quality: lunaris_gpu::Quality, root: PathBuf) -> ExitCode {
    let press = |engine: &mut Engine, values: Vec<(String, bool)>| -> Result<(), String> {
        if values.is_empty() {
            return Ok(());
        }
        let obj = Json::Obj(values.into_iter().map(|(k, v)| (k, Json::Bool(v))).collect());
        engine.call("input", &Json::obj([("values", obj)])).map(|_| ()).map_err(|e| e.to_string())
    };
    let frames = opts.frames.unwrap_or(0);
    let mut played = 0u64;
    let mut failed: Option<String> = press(&mut engine, opts.hold.iter().map(|k| (k.clone(), true)).collect()).err();
    // `--api PORT` in a self-test: the game is played in real time with no window (60 ticks a second) while a program plays it through the live API: a headless server for tests and for machines with no display
    let mut api = REMOTE.get().and_then(|r| r.api).and_then(|port| crate::liveapi::LiveApi::start(port).map_err(|e| eprintln!("the live API could not listen on port {port}: {e}")).ok());
    if let Some(a) = &api {
        println!("live API on {} (no window, real time)", a.addr);
    }
    let tick_len = Duration::from_secs_f32(engine.sim.dt);
    let mut texts_seen = remembered_now(&mut engine);
    while failed.is_none() && played < frames {
        let tick_start = Instant::now();
        if let Some(api) = api.as_mut() {
            api.pump(&mut engine, Duration::from_millis(4));
        }
        failed = press(&mut engine, scripted_keys(&opts.presses, played)).err();
        let first_tick = engine.sim.tick;
        if let Some(driver) = opts.driver.as_mut() {
            driver(&mut engine.sim);
        }
        engine.sim.step();
        if engine.sim.world.resource("settings.save").is_some_and(|v| v > 0.5) {
            if let Err(e) = save_settings(&mut engine, &root.join("settings.json")) {
                eprintln!("note: the settings were not saved: {e}");
            }
            texts_seen = remembered_now(&mut engine);
        }
        keep_texts(&mut engine, &root.join("settings.json"), &mut texts_seen);
        if let Some(sound) = &mut sound {
            let events: Vec<lunaris_sim::Event> = engine.sim.events.iter().filter(|e| e.tick >= first_tick).cloned().collect();
            sound.director.update(&mut engine, &events);
        }
        played += 1;
        if engine.sim.world.resource("app.quit").is_some_and(|v| v > 0.5) {
            break;
        }
        if api.is_some() {
            if let Some(rest) = tick_len.checked_sub(tick_start.elapsed()) {
                std::thread::sleep(rest);
            }
        }
    }
    if let Some(e) = failed {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    if let Some(path) = &opts.save_frame {
        let (w, h) = opts.size;
        let mut textures = TextureCache::new(Some(root));
        let mut image: Option<(u32, u32, Vec<u8>)> = None;
        if !opts.cpu {
            match lunaris_gpu::GpuRenderer::headless() {
                Ok(mut gpu) => {
                    println!("rendering on the GPU: {} ({} quality, no window)", gpu.adapter_name(), opts.quality);
                    textures.share_world(engine.sim.meshes.clone(), engine.sim.heightfields.clone());
                    match gpu.render_to_image(&mut engine.sim.world, &mut textures, &lunaris_render::RenderOptions { width: w.max(16), height: h.max(16), ..Default::default() }, &quality) {
                        Ok(f) => image = Some((f.width, f.height, f.rgba)),
                        Err(e) => eprintln!("the GPU renderer failed ({e}); using the CPU renderer"),
                    }
                }
                Err(e) => eprintln!("the GPU renderer is not available ({e}); using the CPU renderer"),
            }
        }
        match image {
            Some((w, h, rgba)) => save_png(path, lunaris_render::encode_png(w, h, &rgba)),
            None => {
                let (rw, rh) = capped_size((w, h));
                save_png(path, engine.render_rgba(rw, rh).map_err(|e| e.to_string()).and_then(|f| f.png()));
            }
        }
    }
    if opts.report {
        print_report(&mut engine, played, None, sound.as_ref());
    }
    ExitCode::SUCCESS
}

struct App {
    engine: Engine,
    opts: Options,
    window: Option<Arc<Window>>,
    held: BTreeSet<String>,
    /// Hands-off play: the keys come from a control file and not from this machine's devices.
    remote: Option<RemoteState>,
    /// The live API: a program that plays the game and reads it while it runs.
    api: Option<crate::liveapi::LiveApi>,
    mouse: (f32, f32),
    /// Mouse motion and wheel since the last frame, the pointer held in the window (`cursor.lock`), whether zeros still have to be sent, whether the window has focus.
    look: (f32, f32),
    wheel: f32,
    look_sent: bool,
    cursor_locked: bool,
    /// What the game asked for in the resources `cursor.hide`, `display.fullscreen` and `display.quality` when they were last applied (they are applied again only when they change).
    cursor_hidden: bool,
    display_fullscreen: Option<i64>,
    display_quality: Option<i64>,
    /// The `display.vsync` last applied to the window's surface.
    display_vsync: Option<bool>,
    /// The `display.volume` last applied to the mixer (in thousandths, so a float that did not change is not set again).
    display_volume: Option<i64>,
    focused: bool,
    /// Where the settings menu's choices are kept (`settings.json` in the game's folder).
    settings_path: PathBuf,
    /// The text of the fields that remember (`UiInput.remember`) as they were when the settings were last written or read.
    saved_texts: Vec<(String, String)>,
    /// Game controllers: their buttons and sticks become `input.gamepad*` resources, and `rumble*` resources shake them (a visible window only).
    pads: lunaris_pad::Pads,
    /// The sound: the director that turns events and `AudioSource` entities into voices, and the sound card it plays into (none with `--mute`
    /// or when there is no device).
    sound: Option<Sound>,
    last: Instant,
    next_frame: Instant,
    acc: f32,
    frames: u64,
    fps_start: Instant,
    fps_frames: u32,
    fps: f32,
    /// The work of every picture since the last `--perf` line: (whole, simulation) in milliseconds, and when that line was printed.
    perf_frames: Vec<(f32, f32)>,
    /// The passes of the card and the phases of the processor over the same pictures (name, total ms), when `LUNARIS_GPU_PROFILE=1`.
    perf_passes: Vec<(String, f32)>,
    perf_at: Instant,
    last_frame: Option<Frame>,
    #[cfg_attr(not(windows), allow(dead_code))]
    bgra: Vec<u8>,
    error: Option<String>,
    /// The GPU renderer, when it started (it draws into this window's surface).
    gpu: Option<lunaris_gpu::GpuRenderer>,
    quality: lunaris_gpu::Quality,
    textures: TextureCache,
    /// The last frame drawn by the GPU, kept as an image only when `--save-frame` asks for it.
    gpu_image: Option<(u32, u32, Vec<u8>)>,
    /// A networked game: the keys go to the host. In lockstep the simulation steps only the ticks everybody's input has arrived for; in a
    /// rollback game it steps at once and goes back when a guess about somebody's input was wrong; a spectator plays what the host sends.
    net: Option<lunaris_net::Seat>,
    /// Drawing between ticks (`interp.rs`): the state a tick ago, blended with the state now when the picture is drawn.
    interp: crate::interp::Interp,
    interpolate: bool,
}

impl App {
    fn press(&mut self, values: Vec<(String, bool)>) {
        self.set_input(values.into_iter().map(|(k, v)| (k, if v { 1.0 } else { 0.0 })).collect());
    }

    /// Keys, buttons and the mouse position become `input.<name>` resources: directly in a single-player game, through the host in a
    /// networked one (where they reach every copy of the game a few ticks later).
    fn set_input(&mut self, values: Vec<(String, f64)>) {
        if values.is_empty() {
            return;
        }
        if let Some(net) = self.net.as_mut() {
            for (k, v) in values {
                net.set_key(&k, v);
            }
            return;
        }
        let obj = Json::Obj(values.into_iter().map(|(k, v)| (k, if v == 0.0 || v == 1.0 { Json::Bool(v == 1.0) } else { Json::from(v) })).collect());
        if let Err(e) = self.engine.call("input", &Json::obj([("values", obj)])) {
            self.error = Some(e.to_string());
        }
    }

    /// Hands-off play: read the control file when it has changed, press and release what it says, release the taps and turn the look rate into this picture's motion. True when the file says `quit`.
    fn remote_frame(&mut self, real: f32) -> bool {
        let Some(mut st) = self.remote.take() else { return false };
        if let Some(path) = st.path.clone() {
            if let Ok(meta) = std::fs::metadata(&path) {
                let stamp = (meta.modified().ok(), meta.len());
                if st.seen != Some(stamp) {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        st.seen = Some(stamp);
                        self.remote_apply(&mut st, &text);
                    }
                }
            }
        }
        let mut released = Vec::new();
        for (key, (_, left)) in st.taps.iter_mut() {
            if *left > 0 {
                *left -= 1;
                if *left == 0 {
                    released.push((key.clone(), false));
                }
            }
        }
        self.press(released);
        self.look.0 += st.look.0 * real;
        self.look.1 += st.look.1 * real;
        let quit = st.quit;
        self.remote = Some(st);
        quit
    }

    fn remote_apply(&mut self, st: &mut RemoteState, text: &str) {
        let c = parse_control(text);
        let mut presses: Vec<(String, bool)> = Vec::new();
        // (the first read of a file only notes the numbers of its taps: a file left from an earlier run does not press anything)
        for (key, n) in &c.taps {
            match st.taps.get_mut(key) {
                Some(entry) if entry.0 == *n => {}
                Some(entry) => {
                    *entry = (*n, 4);
                    presses.push((key.clone(), true));
                }
                None => {
                    st.taps.insert(key.clone(), (*n, if st.first { 4 } else { 0 }));
                    if st.first {
                        presses.push((key.clone(), true));
                    }
                }
            }
        }
        st.first = true;
        for k in c.keys.difference(&st.keys) {
            presses.push((k.clone(), true));
        }
        for k in st.keys.difference(&c.keys) {
            presses.push((k.clone(), false));
        }
        st.keys = c.keys;
        st.look = c.look;
        st.quit |= c.quit;
        self.press(presses);
        self.set_input(c.mouse.map(|(x, y)| vec![("mouse_x".to_string(), x as f64), ("mouse_y".to_string(), y as f64)]).unwrap_or_default());
    }

    fn release_all(&mut self) {
        let v: Vec<(String, bool)> = std::mem::take(&mut self.held).into_iter().map(|k| (k, false)).collect();
        self.press(v);
    }

    fn render_size(&self) -> (u32, u32) {
        // draw at the window's size but never more than 1280x720: GDI scales the picture up to the window
        capped_size(self.window.as_ref().map(|w| (w.inner_size().width, w.inner_size().height)).unwrap_or(self.opts.size))
    }

    /// The `--perf` line: the pictures of the last second. The times are the work of a picture (simulation and drawing, which with vsync includes waiting for the display), not the gap between pictures.
    fn print_perf(&mut self, now: Instant) {
        let mut all: Vec<f32> = self.perf_frames.iter().map(|f| f.0).collect();
        if all.is_empty() {
            return;
        }
        all.sort_by(|a, b| a.total_cmp(b));
        let n = all.len();
        let avg = all.iter().sum::<f32>() / n as f32;
        let sim = self.perf_frames.iter().map(|f| f.1).sum::<f32>() / n as f32;
        let secs = now.duration_since(self.perf_at).as_secs_f32().max(1.0e-3);
        println!("perf: {:.0} fps, {n} pictures, work avg {avg:.1} ms (simulation {sim:.1}), p95 {:.1}, slowest {:.1}", n as f32 / secs, all[((n as f32 * 0.95) as usize).min(n - 1)], all[n - 1]);
        if !self.perf_passes.is_empty() {
            // the passes and phases that took longest on average (`pass.mark`: the parts of a pass)
            let mut top: Vec<&(String, f32)> = self.perf_passes.iter().collect();
            top.sort_by(|a, b| b.1.total_cmp(&a.1));
            if let Some(g) = &self.gpu {
                println!("      biggest: {}", g.last_top_meshes.iter().take(4).map(|(whole, n, lods, tris, verts)| format!("{whole} tris/{verts} verts x{n} (per level {}) = {:.1}M", lods.iter().map(|c| c.to_string()).collect::<Vec<_>>().join("/"), *tris as f32 / 1.0e6)).collect::<Vec<_>>().join("; "));
            }
            println!("      {}", top.iter().take(16).map(|e| format!("{} {:.2}", e.0, e.1 / n as f32)).collect::<Vec<_>>().join(", "));
        }
        self.perf_passes.clear();
        self.perf_frames.clear();
        self.perf_at = now;
    }

    /// One frame: advance the simulation, draw, show, play sounds.
    fn frame(&mut self, el: &ActiveEventLoop) {
        // `exit` takes effect after the events already queued: a self-test must not run one picture more than it was told to
        if self.opts.frames.is_some_and(|n| self.frames >= n) {
            el.exit();
            return;
        }
        let now = Instant::now();
        let real = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        let dt = self.engine.sim.dt;
        // a self-test steps exactly one tick per frame so that it does not depend on how fast the machine is
        let steps = if self.opts.frames.is_some() {
            1
        } else {
            self.acc += real;
            let n = (self.acc / dt).floor() as u32;
            self.acc -= n as f32 * dt;
            n.min(6)
        };
        if !self.opts.hidden && self.remote.is_none() {
            let moved = self.pads.poll();
            self.set_input(moved);
        }
        if self.remote.is_some() && self.remote_frame(real) {
            el.exit();
            return;
        }
        // the live API: what a program asked since the last picture is answered now, between two ticks
        if let Some(api) = self.api.as_mut() {
            api.pump(&mut self.engine, Duration::from_millis(4));
        }
        // the mouse's motion since the last frame goes into the first tick of this one, then back to zero
        // keys a self-test presses at set frames (one tick a frame, so the frame is the tick)
        let at = self.frames;
        let scripted = scripted_keys(&self.opts.presses, at);
        if !scripted.is_empty() {
            self.press(scripted);
        }
        let (look, wheel) = (std::mem::take(&mut self.look), std::mem::take(&mut self.wheel));
        let moving = look != (0.0, 0.0) || wheel != 0.0;
        if moving || self.look_sent {
            self.set_input(vec![("mouse_dx".to_string(), look.0 as f64), ("mouse_dy".to_string(), look.1 as f64), ("mouse_wheel".to_string(), wheel as f64)]);
            self.look_sent = moving;
        }
        let work_start = Instant::now();
        let first_tick = self.engine.sim.tick;
        if let Some(net) = self.net.as_mut() {
            // send our keys, then play what the network allows: lockstep steps only the ticks that everybody's input has arrived for,
            // rollback steps at once (and goes back when a guess was wrong); the time not played waits. A copy that is replaying the game
            // from the log (a spectator that came late, a player coming back) plays as many ticks as it can instead.
            let steps = if net.catching_up(&self.engine) { 4000 } else { steps };
            match net.advance(&mut self.engine, steps as usize) {
                Ok(done) => self.acc = (self.acc + steps.saturating_sub(done as u32) as f32 * dt).min(0.25),
                Err(e) => self.error = Some(e),
            }
            if let Some(d) = net.take_desync() {
                eprintln!("the copies of the game have drifted apart: {d}");
            }
            for news in net.take_events() {
                println!("{news}");
            }
            if let Some(why) = net.finished().map(str::to_string) {
                if net.ready_ticks() == 0 {
                    println!("the game ended: {why}");
                    el.exit();
                }
            }
        } else {
            for k in 0..steps {
                if let Some(driver) = self.opts.driver.as_mut() {
                    driver(&mut self.engine.sim);
                }
                if self.interpolate && k + 1 == steps {
                    self.interp.capture(&mut self.engine.sim.world);
                }
                self.engine.sim.step();
                if k == 0 && self.look_sent {
                    self.set_input(vec![("mouse_dx".to_string(), 0.0), ("mouse_dy".to_string(), 0.0), ("mouse_wheel".to_string(), 0.0)]);
                    self.look_sent = false;
                }
            }
        }
        if !self.opts.hidden && self.remote.is_none() && self.pads.any_connected() {
            // the game shakes a pad by writing the resources `rumble.low` and `rumble.high` (0 to 1), `rumble2.*` for the second pad, ...
            for pad in 0..lunaris_pad::MAX_PADS {
                let p = if pad == 0 { "rumble".to_string() } else { format!("rumble{}", pad + 1) };
                let low = self.engine.sim.world.resource(&format!("{p}.low")).unwrap_or(0.0) as f32;
                let high = self.engine.sim.world.resource(&format!("{p}.high")).unwrap_or(0.0) as f32;
                self.pads.set_rumble(pad, low, high);
            }
        }
        // a game that wants the pointer (first-person look) hides it and holds it in the window while the window has focus
        let want_lock = !self.opts.hidden && self.remote.is_none() && self.focused && self.engine.sim.world.resource("cursor.lock").is_some_and(|v| v > 0.5);
        if want_lock != self.cursor_locked {
            if let Some(w) = &self.window {
                if want_lock {
                    if w.set_cursor_grab(CursorGrabMode::Locked).is_err() {
                        let _ = w.set_cursor_grab(CursorGrabMode::Confined);
                    }
                } else {
                    let _ = w.set_cursor_grab(CursorGrabMode::None);
                }
                w.set_cursor_visible(!want_lock);
            }
            self.cursor_locked = want_lock;
        }
        // a menu that draws its own pointer hides the system one (`cursor.hide` above 0.5; `cursor.lock` hides it too, and holds it)
        let want_hide = !self.opts.hidden && self.remote.is_none() && !want_lock && self.engine.sim.world.resource("cursor.hide").is_some_and(|v| v > 0.5);
        if want_hide != self.cursor_hidden {
            if let Some(w) = &self.window {
                w.set_cursor_visible(!want_hide);
            }
            self.cursor_hidden = want_hide;
        }
        // `display.volume` (0 to 4) is the master volume of the mixer, `settings.save` writes the settings to `settings.json`
        let vol = self.engine.sim.world.resource("display.volume").filter(|v| v.is_finite()).map(|v| (v.clamp(0.0, 4.0) * 1000.0).round() as i64);
        if vol.is_some() && vol != self.display_volume {
            if let (Some(sound), Some(v)) = (&self.sound, vol) {
                sound.mixer.lock().unwrap_or_else(|p| p.into_inner()).master = v as f32 / 1000.0;
            }
            self.display_volume = vol;
        }
        if self.engine.sim.world.resource("settings.save").is_some_and(|v| v > 0.5) {
            if let Err(e) = save_settings(&mut self.engine, &self.settings_path) {
                eprintln!("note: the settings were not saved: {e}");
            }
            self.saved_texts = remembered_now(&mut self.engine);
        }
        keep_texts(&mut self.engine, &self.settings_path, &mut self.saved_texts);
        // a settings menu writes `display.fullscreen` (1 borderless full screen, 0 a window) and `display.quality` (0 classic ... 4 ultra); each is applied when it changes
        if !self.opts.hidden {
            let fs = self.engine.sim.world.resource("display.fullscreen").map(|v| v.round() as i64);
            if fs.is_some() && fs != self.display_fullscreen {
                if let Some(w) = &self.window {
                    w.set_fullscreen(if fs == Some(1) { Some(Fullscreen::Borderless(None)) } else { None });
                }
                self.display_fullscreen = fs;
            }
            let q = self.engine.sim.world.resource("display.quality").map(|v| v.round() as i64);
            if q.is_some() && q != self.display_quality {
                if let Some(name) = q.and_then(|n| ["classic", "low", "medium", "high", "ultra"].get(n.clamp(0, 4) as usize).copied()) {
                    if let Some(quality) = lunaris_gpu::Quality::from_name(name) {
                        self.quality = quality;
                        self.opts.quality = name.to_string();
                    }
                }
                self.display_quality = q;
            }
            // `display.vsync` (1 wait for the display's refresh, 0 show each picture at once) is applied when it changes, once the window has its renderer; `--novsync` and `--fps` on the command line win
            let vs = self.engine.sim.world.resource("display.vsync").map(|v| v > 0.5);
            if vs.is_some() && vs != self.display_vsync {
                if !self.opts.vsync {
                    self.display_vsync = vs;
                } else if let Some(gpu) = self.gpu.as_mut() {
                    gpu.set_vsync(vs == Some(true));
                    self.display_vsync = vs;
                }
            }
        }
        let sim_ms = work_start.elapsed().as_secs_f32() * 1000.0;
        // the picture is of a moment between the last two ticks (`interp.rs`)
        let undo = if self.interpolate { Some(self.interp.apply(&mut self.engine.sim.world, self.acc / dt, dt)) } else { None };
        let mut drew_on_gpu = false;
        if let (Some(gpu), Some(window)) = (self.gpu.as_mut(), self.window.as_ref()) {
            let size = window.inner_size();
            if size.width == 0 || size.height == 0 {
                drew_on_gpu = true; // minimised: nothing to draw
            } else if self.opts.save_frame.is_some() {
                // a self-test that wants a picture: draw into an image so it can be saved, and show it
                let opts = lunaris_render::RenderOptions { width: size.width, height: size.height, ..Default::default() };
                self.textures.share_world(self.engine.sim.meshes.clone(), self.engine.sim.heightfields.clone());
                match gpu.render_to_image(&mut self.engine.sim.world, &mut self.textures, &opts, &self.quality) {
                    Ok(f) => {
                        self.gpu_image = Some((f.width, f.height, f.rgba));
                        drew_on_gpu = true;
                    }
                    Err(e) => eprintln!("the GPU renderer failed ({e}); switching to the CPU renderer"),
                }
            } else {
                self.textures.share_world(self.engine.sim.meshes.clone(), self.engine.sim.heightfields.clone());
                match gpu.render_to_window(&mut self.engine.sim.world, &mut self.textures, size.width, size.height, &self.quality) {
                    Ok(_) => {
                        drew_on_gpu = true;
                    }
                    Err(e) => eprintln!("the GPU renderer failed ({e}); switching to the CPU renderer"),
                }
            }
            if !drew_on_gpu {
                self.gpu = None;
            }
        }
        let (rw, rh) = self.render_size();
        match if drew_on_gpu { Err(lunaris_ecs::EcsError::new("skipped", "drawn on the GPU")) } else { self.engine.render_rgba(rw, rh) } {
            Ok(frame) => {
                #[cfg(windows)]
                if let Some(window) = &self.window {
                    if let Ok(handle) = window.window_handle() {
                        if let RawWindowHandle::Win32(h) = handle.as_raw() {
                            self.bgra.resize((frame.width * frame.height * 4) as usize, 0);
                            for (dst, src) in self.bgra.chunks_exact_mut(4).zip(frame.rgba.chunks_exact(4)) {
                                dst.copy_from_slice(&[src[2], src[1], src[0], 255]);
                            }
                            let size = window.inner_size();
                            gdi::blit(h.hwnd.get(), &self.bgra, frame.width, frame.height, size.width, size.height);
                        }
                    }
                }
                self.last_frame = Some(frame);
            }
            Err(e) if drew_on_gpu && e.code == "skipped" => {}
            Err(e) => self.error = Some(e.to_string()),
        }
        if let Some(undo) = undo {
            self.interp.restore(&mut self.engine.sim.world, undo);
        }
        // sound for what happened in this frame
        if let Some(sound) = &mut self.sound {
            let events: Vec<lunaris_sim::Event> = self.engine.sim.events.iter().filter(|e| e.tick >= first_tick).cloned().collect();
            sound.director.update(&mut self.engine, &events);
        }
        self.frames += 1;
        self.fps_frames += 1;
        if self.opts.perf {
            self.perf_frames.push((work_start.elapsed().as_secs_f32() * 1000.0, sim_ms));
            if let Some(g) = &self.gpu {
                // what the picture drew: thousands of triangles in the camera's view, objects in the list, objects the view left out (as "passes" so that they are averaged like the times)
                for (k, v) in [("tris k", g.last_triangles as f32 / 1000.0), ("objects", g.last_cull.0 as f32), ("culled", g.last_cull.1 as f32), ("draws", g.last_draws as f32), ("draws shadow", g.last_draws_split[0] as f32), ("draws point", g.last_draws_split[1] as f32), ("occluded", g.last_occluded.0 as f32), ("prisms", g.last_occluded.1 as f32)] {
                    match self.perf_passes.iter_mut().find(|e| e.0 == k) {
                        Some(e) => e.1 += v,
                        None => self.perf_passes.push((k.to_string(), v)),
                    }
                }
                let total: f32 = g.last_gpu_ms.iter().filter(|e| !e.0.contains('.')).map(|e| e.1).sum();
                if total > 0.0 {
                    match self.perf_passes.iter_mut().find(|e| e.0 == "gpu TOTAL") {
                        Some(e) => e.1 += total,
                        None => self.perf_passes.push(("gpu TOTAL".to_string(), total)),
                    }
                }
                let this_frame = lunaris_render::timing::take_sums();
                let work = work_start.elapsed().as_secs_f32() * 1000.0;
                // `LUNARIS_SLOW_MS` (default 25) is how long a picture must take to be listed with what it was busy with; the steps shown are those over a twelfth of it (at least 0.7 ms)
                let slow_ms = std::env::var("LUNARIS_SLOW_MS").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(25.0);
                if work >= slow_ms && !this_frame.is_empty() {
                    let cpu: Vec<(String, f32)> = g.last_cpu_ms.iter().map(|(k, v)| (format!("cpu {k}"), *v)).collect();
                    let mut big: Vec<&(String, f32)> = this_frame.iter().chain(cpu.iter()).filter(|e| e.1 >= (slow_ms / 12.0).max(0.7)).collect();
                    big.sort_by(|a, b| b.1.total_cmp(&a.1));
                    println!("      slow picture {}: {work:.0} ms (simulation {sim_ms:.1}): {}", self.frames, big.iter().take(10).map(|e| format!("{} {:.1}", e.0, e.1)).collect::<Vec<_>>().join(", "));
                }
                for (k, v) in g.last_gpu_ms.iter().map(|(k, v)| (format!("gpu {k}"), *v)).chain(g.last_cpu_ms.iter().map(|(k, v)| (format!("cpu {k}"), *v))).chain(this_frame.into_iter().map(|(k, v)| (format!("[{k}]"), v))) {
                    match self.perf_passes.iter_mut().find(|e| e.0 == k) {
                        Some(e) => e.1 += v,
                        None => self.perf_passes.push((k, v)),
                    }
                }
            }
            if now.duration_since(self.perf_at).as_secs_f32() >= 1.0 {
                self.print_perf(now);
            }
        }
        let since = now.duration_since(self.fps_start).as_secs_f32();
        if since >= 0.5 {
            self.fps = self.fps_frames as f32 / since;
            self.fps_start = now;
            self.fps_frames = 0;
            // a game that shows a frame counter asks with `perf.want` above 0.5 and reads `perf.fps` (pictures a second) and `perf.ms` (milliseconds a picture), written twice a second; only when asked (a resource written in every game
            // would change every game's state), and not in a lockstep game (the copies must stay alike)
            if self.net.is_none() && self.engine.sim.world.resource("perf.want").is_some_and(|v| v > 0.5) {
                let _ = self.engine.sim.world.set_resource("perf.fps", Some(self.fps.round() as f64));
                let _ = self.engine.sim.world.set_resource("perf.ms", Some((1000.0 / self.fps.max(0.001) * 10.0).round() as f64 / 10.0));
            }
            if let Some(w) = &self.window {
                let seat = self.net.as_ref().map(|n| if n.welcome().watching { format!(" - {}", n.status(&self.engine)) } else { format!(" - player {} of {} - {}", n.welcome().player + 1, n.welcome().players, n.status(&self.engine)) }).unwrap_or_default();
                w.set_title(&format!("{} - {}{}{} - tick {} - {:.0} fps ({})", title(), self.opts.scene, seat, if self.remote.is_some() { " - hands-off" } else { "" }, self.engine.sim.tick, self.fps, if self.gpu.is_some() { format!("GPU {}", self.opts.quality) } else { "CPU".to_string() }));
            }
        }
        if self.error.is_some() || self.opts.frames.is_some_and(|n| self.frames >= n) || self.engine.sim.world.resource("app.quit").is_some_and(|v| v > 0.5) {
            el.exit();
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut attrs = Window::default_attributes().with_title(format!("{} - {}", title(), self.opts.scene)).with_inner_size(PhysicalSize::new(self.opts.size.0, self.opts.size.1)).with_visible(!self.opts.hidden);
        // (a window that is played for somebody who is busy with something else does not take the focus when it opens)
        if self.remote.is_some() {
            attrs = attrs.with_active(false);
        }
        // a window that is part of the desktop: bare, at the bottom, and of the desktop's own window type where the system has one
        if REMOTE.get().is_some_and(|r| r.desktop) {
            attrs = attrs.with_decorations(false).with_window_level(winit::window::WindowLevel::AlwaysOnBottom).with_active(false);
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                use winit::platform::x11::{WindowAttributesExtX11, WindowType};
                attrs = attrs.with_x11_window_type(vec![WindowType::Desktop]);
            }
        }
        if let Some((x, y)) = REMOTE.get().and_then(|r| r.pos) {
            attrs = attrs.with_position(winit::dpi::PhysicalPosition::new(x, y));
        }
        match el.create_window(attrs) {
            Ok(w) => self.window = Some(Arc::new(w)),
            Err(e) => {
                self.error = Some(format!("cannot open a window: {e}"));
                el.exit();
            }
        }
        // on systems where only the GPU renderer can put a picture in a window (everything but Windows, which has a software blit) the CPU renderer cannot be shown
        #[cfg(not(windows))]
        if self.opts.cpu {
            self.error = Some("--cpu is not available here: a window on this system is drawn by the GPU renderer (use `lunaris run` or the `render` tool for pictures of the CPU reference renderer)".to_string());
            el.exit();
            return;
        }
        // the GPU renderer, if wanted and available: a surface on this window
        if !self.opts.cpu {
            if let Some(window) = &self.window {
                let size = window.inner_size();
                match lunaris_gpu::GpuRenderer::for_window(window.clone(), size.width, size.height, self.opts.vsync) {
                    // where the engine's own CPU renderer can show a window (Windows), a machine whose only adapter is a software one (WARP) draws faster with it: on Mesa's llvmpipe a picture took 210 ms
                    // against 29 to 48 ms (docs/PERFORMANCE.md); WARP itself has not been measured. `LUNARIS_GPU=<name>` keeps the adapter.
                    Ok(r) if cfg!(windows) && r.adapter_is_software() && std::env::var_os("LUNARIS_GPU").is_none() => {
                        println!("the only graphics adapter is a software one ({}); using the CPU renderer (LUNARIS_GPU=<name> keeps the adapter)", r.adapter_name());
                    }
                    Ok(mut r) => {
                        println!("rendering on the GPU: {} ({} quality)", r.adapter_name(), self.opts.quality);
                        // a game that is played never waits for a cell of the world to be read, built and compressed: that happens on other threads (`LUNARIS_SYNC_UPLOAD=1` turns it off)
                        if self.opts.frames.is_none() && std::env::var_os("LUNARIS_SYNC_UPLOAD").is_none() {
                            r.async_prep = true;
                            self.textures.async_jobs = true;
                        }
                        self.gpu = Some(r);
                    }
                    Err(e) => {
                        eprintln!("the GPU renderer is not available ({e}); using the CPU renderer");
                        #[cfg(not(windows))]
                        {
                            self.error = Some(format!("a window on this system needs the GPU renderer, and it is not available ({e})"));
                            el.exit();
                            return;
                        }
                    }
                }
            }
        }
        let hold: Vec<(String, bool)> = self.opts.hold.iter().map(|k| (k.clone(), true)).collect();
        self.held.extend(self.opts.hold.iter().cloned());
        self.press(hold);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // hands-off play: nothing this machine's keyboard or mouse does reaches the game
        if self.remote.is_some() && matches!(event, WindowEvent::KeyboardInput { .. } | WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. } | WindowEvent::CursorMoved { .. }) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Focused(true) => self.focused = true,
            WindowEvent::Focused(false) => {
                self.focused = false;
                if self.remote.is_none() {
                    self.release_all();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.wheel += match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else { return };
                let down = event.state == ElementState::Pressed;
                // typing into a text field (`UiInput`): the characters, a paste, clearing the field; all of it at once, between two ticks (a lockstep game has no menu to type into)
                if down && self.net.is_none() && lunaris_sim::ui_kit::has_focused_input(&mut self.engine.sim.world) {
                    let (ctrl, alt) = (self.held.contains("control"), self.held.contains("alt"));
                    match typing_of(event.text.as_deref(), code, ctrl, alt) {
                        Some(Typing::Text(t)) => {
                            lunaris_sim::ui_kit::type_into_focused(&mut self.engine.sim.world, &t);
                        }
                        Some(Typing::Paste) if !event.repeat => {
                            if let Some(t) = clip::text() {
                                lunaris_sim::ui_kit::type_into_focused(&mut self.engine.sim.world, t.trim());
                            }
                        }
                        Some(Typing::Clear) if !event.repeat => {
                            lunaris_sim::ui_kit::clear_focused(&mut self.engine.sim.world);
                        }
                        _ => {}
                    }
                }
                if code == KeyCode::Escape && escape_quits(&self.engine) {
                    if down {
                        el.exit();
                    }
                } else if code == KeyCode::Escape {
                    // a game that took Esc for itself (a pause menu) reads it as the key `input.escape`
                    if !event.repeat && down != self.held.contains("escape") {
                        if down {
                            self.held.insert("escape".to_string());
                        } else {
                            self.held.remove("escape");
                        }
                        self.press(vec![("escape".to_string(), down)]);
                    }
                } else if code == KeyCode::F11 && down && !event.repeat {
                    if let Some(w) = &self.window {
                        w.set_fullscreen(if w.fullscreen().is_some() { None } else { Some(Fullscreen::Borderless(None)) });
                    }
                } else if let Some(name) = key_name(code) {
                    if event.repeat && down && name == "backspace" {
                        // holding Backspace deletes on (a field takes one `input.backspace` a tick)
                        self.press(vec![(name, true)]);
                    } else if !event.repeat && down != self.held.contains(&name) {
                        if down {
                            self.held.insert(name.clone());
                        } else {
                            self.held.remove(&name);
                        }
                        self.press(vec![(name, down)]);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(w) = &self.window {
                    let s = w.inner_size();
                    // the fractions of the window, or of the safe area of the interface when the game has a `UiScale` (docs/UI.md, "Other screens")
                    let aspect = lunaris_sim::ui_kit::ui_scale_aspect(&mut self.engine.sim.world);
                    let (x, y) = lunaris_sim::ui_kit::pointer_to_ui(aspect, position.x as f32, position.y as f32, s.width, s.height);
                    if (x - self.mouse.0).abs() > 0.001 || (y - self.mouse.1).abs() > 0.001 {
                        self.mouse = (x, y);
                        self.set_input(vec![("mouse_x".to_string(), x as f64), ("mouse_y".to_string(), y as f64)]);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let name = match button {
                    MouseButton::Left => "mouse_left",
                    MouseButton::Right => "mouse_right",
                    MouseButton::Middle => "mouse_middle",
                    MouseButton::Back => "mouse_back",
                    MouseButton::Forward => "mouse_forward",
                    _ => return,
                };
                self.press(vec![(name.to_string(), state == ElementState::Pressed)]);
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            if self.focused && !self.opts.hidden && self.remote.is_none() {
                self.look.0 += delta.0 as f32;
                self.look.1 += delta.1 as f32;
            }
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_frame {
            self.frame(el);
            if self.gpu.is_some() {
                // on the GPU, presenting waits for the display's refresh (vsync), so the picture is drawn once per refresh;
                // the simulation still advances in its own fixed steps. The small gap only guards against a driver that ignores vsync (`--fps` sets it: none at all for an unlimited rate).
                let cap = REMOTE.get().and_then(|r| r.fps).or_else(|| self.engine.sim.world.resource("display.fps").filter(|f| f.is_finite() && *f > 0.0).map(|f| f as f32)); // (the resource 0 is "no cap": the default pacing)
                let gap = match cap {
                    Some(f) if f <= 0.0 => Duration::ZERO,
                    Some(f) => Duration::from_secs_f32(1.0 / f.min(1000.0)),
                    None => Duration::from_millis(2),
                };
                self.next_frame = Instant::now() + gap;
            } else {
                self.next_frame += Duration::from_micros(16_667);
                if self.next_frame < now {
                    self.next_frame = now + Duration::from_micros(16_667);
                }
            }
        }
        el.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

pub fn run(mut opts: Options, root: PathBuf, seed: u64, threads: Option<usize>) -> ExitCode {
    let quality = match lunaris_gpu::Quality::from_name(&opts.quality) {
        Some(q) => q,
        None => {
            eprintln!("error: unknown quality `{}` (classic, low, medium, high or ultra)", opts.quality);
            return ExitCode::from(2);
        }
    };
    let root_for_textures = root.clone();
    // the simulated network (if any) lives until the window closes
    let mut _proxy = None;
    let (engine, net) = match opts.join.take() {
        Some(join) => {
            println!("joining {} as {} (waiting for the other players)...", join.addr, join.options.name);
            let scene = opts.scene.clone();
            _proxy = join.proxy;
            let joined = lunaris_net::connect(&join.addr, join.options, |w| {
                let mut engine = Engine::new(w.seed, root);
                if let Some(t) = threads {
                    engine.sim.threads = t.clamp(1, 64);
                }
                engine.call("load_scene", &Json::obj([("path", Json::str(scene))])).map_err(|e| e.to_string())?;
                Ok(engine)
            });
            match joined {
                Ok((net, engine)) => {
                    let w = net.welcome();
                    if w.watching {
                        println!("you are watching: the game is replayed from tick 0 and then followed live");
                    } else {
                        println!("you are player {} of {}; the game starts now{}", w.player + 1, w.players, if w.rejoin { " (taking your seat back: the game is replayed first)" } else { "" });
                    }
                    if let Some(t) = net.token() {
                        println!("seat token: {t}   (to come back after a drop: --join ... --rollback --token {t})");
                    }
                    (engine, Some(net))
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
        None => {
            let mut engine = Engine::new(seed, root);
            if let Some(t) = threads {
                engine.sim.threads = t.clamp(1, 64);
            }
            if let Err(e) = engine.call("load_scene", &Json::obj([("path", Json::str(opts.scene.clone()))])) {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
            (engine, None)
        }
    };
    let mut engine = engine;
    let settings_path = root_for_textures.join("settings.json");
    load_settings(&mut engine, &settings_path);
    let saved_texts = remembered_now(&mut engine);
    let sound = if opts.mute {
        None
    } else {
        let mixer = Arc::new(Mutex::new(Mixer::new()));
        mixer.lock().unwrap_or_else(|p| p.into_inner()).master = opts.volume;
        match Output::open(mixer.clone()) {
            Ok(output) => {
                let mut director = Director::new(mixer.clone());
                director.spawn_tracer();
                Some(Sound { director, _output: output, mixer })
            }
            Err(e) => {
                eprintln!("note: no sound ({e})");
                None
            }
        }
    };
    // a self-test (`--hidden` with `--frames`) needs no window at all: it is played and drawn offscreen, so it also runs where there is no display
    if opts.hidden && opts.frames.is_some() && net.is_none() {
        return run_headless(opts, engine, sound, quality, root_for_textures);
    }
    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: cannot start the window system: {e}");
            return ExitCode::FAILURE;
        }
    };
    let now = Instant::now();
    // a played game is drawn between its ticks (`LUNARIS_NO_INTERP=1` draws the last tick as it is); a self-test, a networked game and a hidden window are drawn as the world is
    let interpolate = !opts.hidden && opts.frames.is_none() && net.is_none() && std::env::var_os("LUNARIS_NO_INTERP").is_none();
    let mut app = App {
        engine,
        opts,
        window: None,
        held: BTreeSet::new(),
        remote: REMOTE.get().filter(|r| r.hands_off).map(|r| RemoteState { path: r.control.clone(), ..RemoteState::default() }),
        api: REMOTE.get().and_then(|r| r.api).and_then(|port| match crate::liveapi::LiveApi::start(port) {
            Ok(a) => {
                println!("live API on {} (JSON-RPC, one message a line; `lunaris tools` lists the tools)", a.addr);
                Some(a)
            }
            Err(e) => {
                eprintln!("the live API could not listen on port {port}: {e}");
                None
            }
        }),
        mouse: (-1.0, -1.0),
        look: (0.0, 0.0),
        wheel: 0.0,
        look_sent: false,
        cursor_locked: false,
        cursor_hidden: false,
        display_fullscreen: None,
        display_quality: None,
        display_vsync: None,
        display_volume: None,
        settings_path,
        saved_texts,
        focused: true,
        pads: lunaris_pad::Pads::new(),
        sound,
        last: now,
        next_frame: now,
        acc: 0.0,
        frames: 0,
        fps_start: now,
        fps_frames: 0,
        fps: 0.0,
        perf_frames: Vec::new(),
        perf_passes: Vec::new(),
        perf_at: now,
        last_frame: None,
        bgra: Vec::new(),
        error: None,
        gpu: None,
        quality,
        textures: TextureCache::new(Some(root_for_textures)),
        gpu_image: None,
        net,
        interp: crate::interp::Interp::default(),
        interpolate,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    if let Some(e) = &app.error {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    if let Some(path) = &app.opts.save_frame {
        let png = match (&app.gpu_image, &app.last_frame) {
            (Some((w, h, rgba)), _) => Some(lunaris_render::encode_png(*w, *h, rgba)),
            (None, Some(frame)) => Some(frame.png()),
            _ => None,
        };
        if let Some(png) = png {
            save_png(path, png);
        }
    }
    if app.opts.report {
        print_report(&mut app.engine, app.frames, app.gpu.as_ref().map(|g| g.frames_presented), app.sound.as_ref());
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_presses_say_which_key_when_and_for_how_long() {
        assert_eq!(parse_presses("enter@30:4, s@50 ,space@7:1"), vec![("enter".to_string(), 30, 4), ("s".to_string(), 50, 4), ("space".to_string(), 7, 1)]);
        assert_eq!(parse_presses("nonsense,@3,x@y,w@5:0"), vec![("w".to_string(), 5, 1)], "what cannot be read is skipped, a press lasts at least a frame");
        assert!(parse_presses("").is_empty());
    }

    #[test]
    fn keys_make_text_and_ctrl_makes_shortcuts() {
        let t = |text: Option<&str>, code, ctrl, alt| typing_of(text, code, ctrl, alt);
        assert_eq!(t(Some("a"), KeyCode::KeyA, false, false), Some(Typing::Text("a".into())));
        assert_eq!(t(Some("A"), KeyCode::KeyA, false, false), Some(Typing::Text("A".into())), "what the layout says, with Shift");
        assert_eq!(t(Some(" "), KeyCode::Space, false, false), Some(Typing::Text(" ".into())));
        assert_eq!(t(Some("@"), KeyCode::Digit2, true, true), Some(Typing::Text("@".into())), "AltGr is Ctrl and Alt: still text");
        assert_eq!(t(Some("\u{16}"), KeyCode::KeyV, true, false), Some(Typing::Paste), "Ctrl+V pastes");
        assert_eq!(t(Some("\u{7f}"), KeyCode::Backspace, true, false), Some(Typing::Clear), "Ctrl+Backspace clears");
        assert_eq!(t(Some("\u{1}"), KeyCode::KeyA, true, false), None, "Ctrl+A is no text");
        assert_eq!(t(Some("\r"), KeyCode::Enter, false, false), None, "Enter makes no text (it is the key `enter`)");
        assert_eq!(t(Some("\u{8}"), KeyCode::Backspace, false, false), None, "nor does Backspace");
        assert_eq!(t(None, KeyCode::ArrowLeft, false, false), None);
    }

    /// The control file of hands-off play: keys and buttons held (spaces or commas), the look rate, the pointer, taps by number, quit; comments and what it does not know are skipped.
    #[test]
    fn a_control_file_says_what_to_hold_and_tap() {
        let c = parse_control("# a demo\nkeys W, shift ,space   # run and jump\nbuttons mouse_left\nlook 120 -40\nmouse 0.25 0.75\ntap E 3\nwhatever 1\nquit\n");
        assert_eq!(c.keys.iter().cloned().collect::<Vec<_>>(), ["mouse_left", "shift", "space", "w"]);
        assert_eq!(c.look, (120.0, -40.0));
        assert_eq!(c.mouse, Some((0.25, 0.75)));
        assert_eq!(c.taps, vec![("e".to_string(), 3)]);
        assert!(c.quit);
        assert_eq!(parse_control(""), Control::default());
        assert_eq!(parse_control("keys\nlook x y\nmouse 1\n").look, (0.0, 0.0));
    }

    /// A hidden self-test with `--frames` opens no window, so it works where there is no display (it is the test that runs here, in CI and over ssh), steps one tick a frame like a window does,
    /// presses the keys when it is told to and draws the last picture offscreen.
    #[test]
    fn a_hidden_self_test_needs_no_window_and_plays_like_a_window() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let dir = std::env::temp_dir().join(format!("lunaris-headless-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let options = |frames: u64, presses: &str, save: Option<PathBuf>| Options {
            scene: "examples/orb_dash.lunaris.json".to_string(),
            size: (320, 180),
            hidden: true,
            mute: true,
            volume: 1.0,
            frames: Some(frames),
            hold: vec!["d".to_string()],
            presses: parse_presses(presses),
            save_frame: save,
            report: false,
            vsync: true,
            perf: false,
            cpu: true,
            quality: "classic".to_string(),
            join: None,
            driver: None,
        };
        let png = dir.join("last.png");
        let succeeded = |code: ExitCode| format!("{code:?}") == format!("{:?}", ExitCode::SUCCESS);
        assert!(succeeded(run(options(90, "space@30:4", Some(png.clone())), root.clone(), 1, Some(1))));
        let bytes = std::fs::read(&png).expect("the last picture is saved");
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']), "a PNG");
        // the same ticks by hand: one tick a frame, the held key down from the start, the other key pressed at frame 30 and let go at frame 34
        let mut by_hand = Engine::new(1, root.clone());
        by_hand.sim.threads = 1;
        by_hand.call("load_scene", &Json::obj([("path", Json::str("examples/orb_dash.lunaris.json"))])).unwrap();
        by_hand.call("input", &Json::obj([("values", Json::obj([("d", Json::Bool(true))]))])).unwrap();
        let keys = parse_presses("space@30:4");
        for at in 0..90 {
            let pressed = scripted_keys(&keys, at);
            if !pressed.is_empty() {
                by_hand.call("input", &Json::obj([("values", Json::Obj(pressed.into_iter().map(|(k, v)| (k, Json::Bool(v))).collect()))])).unwrap();
            }
            by_hand.sim.step();
        }
        // the picture is the one of the reference renderer at the state those ticks reach
        let frame = by_hand.render_rgba(320, 180).unwrap();
        assert_eq!(std::fs::read(&png).unwrap(), frame.png().unwrap(), "the saved picture is what the reference renderer draws after those 90 ticks");
        // a scene that does not exist is an error, not a hang, and there is still no window
        let mut missing = options(5, "", None);
        missing.scene = "no/such/scene.lunaris.json".to_string();
        assert!(!succeeded(run(missing, root.clone(), 1, Some(1))));
        // the report of a finished self-test (what `--report` prints) is part of the same path
        let mut reported = options(10, "", None);
        reported.report = true;
        assert!(succeeded(run(reported, root, 1, Some(1))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scripted_keys_go_down_at_their_frame_and_up_after_their_length() {
        let presses = parse_presses("enter@3:2,s@4");
        assert!(scripted_keys(&presses, 2).is_empty());
        assert_eq!(scripted_keys(&presses, 3), vec![("enter".to_string(), true)]);
        assert_eq!(scripted_keys(&presses, 4), vec![("s".to_string(), true)]);
        assert_eq!(scripted_keys(&presses, 5), vec![("enter".to_string(), false)]);
        assert_eq!(scripted_keys(&presses, 8), vec![("s".to_string(), false)]);
        assert_eq!(capped_size((1920, 1080)), (1280, 720));
        assert_eq!(capped_size((640, 360)), (640, 360));
        assert_eq!(capped_size((1, 1)), (16, 16));
    }

    #[test]
    fn escape_is_the_games_when_it_says_so() {
        let mut e = Engine::new(1, std::env::temp_dir());
        assert!(escape_quits(&e), "a game that says nothing keeps the old way: Esc closes the window");
        e.sim.world.set_resource("app.quit_on_escape", Some(0.0)).unwrap();
        assert!(!escape_quits(&e), "the resource at 0 hands Esc to the game as the key `input.escape`");
        e.sim.world.set_resource("app.quit_on_escape", Some(1.0)).unwrap();
        assert!(escape_quits(&e));
    }

    #[test]
    fn keys_get_the_names_games_read() {
        assert_eq!(key_name(KeyCode::KeyW).as_deref(), Some("w"));
        assert_eq!(key_name(KeyCode::Space).as_deref(), Some("space"));
        assert_eq!(key_name(KeyCode::ArrowLeft).as_deref(), Some("arrowleft"));
        assert_eq!(key_name(KeyCode::Digit7).as_deref(), Some("7"));
        assert_eq!(key_name(KeyCode::ShiftRight).as_deref(), Some("shift"));
        assert_eq!(key_name(KeyCode::MediaPlayPause), None);
        // every name is a valid resource part, so `{"res": "input.<name>"}` works
        for code in [KeyCode::KeyA, KeyCode::Enter, KeyCode::F12, KeyCode::ControlLeft] {
            let n = key_name(code).unwrap();
            assert!(lunaris_ecs::valid_resource_name(&format!("input.{n}")), "{n}");
        }
    }

    /// A settings menu's choices (`opt.*`, `display.*`) are kept in `settings.json` when the game sets `settings.save`, and come back at the next start; nothing else is kept.
    #[test]
    fn the_settings_of_a_menu_are_saved_and_come_back_and_nothing_else_does() {
        let dir = std::env::temp_dir().join(format!("lunaris-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let _ = std::fs::remove_file(&path);
        let mut a = Engine::new(1, dir.clone());
        load_settings(&mut a, &path); // (no file: nothing changes)
        assert!(a.sim.world.resources().is_empty());
        for (k, v) in [("opt.fov", 95.0), ("opt.sens", 1.5), ("display.volume", 0.4), ("display.quality", 2.0), ("score", 7.0), ("settings.save", 1.0)] {
            a.sim.world.set_resource(k, Some(v)).unwrap();
        }
        save_settings(&mut a, &path).unwrap();
        assert_eq!(a.sim.world.resource("settings.save"), Some(0.0), "the request is answered");
        let mut b = Engine::new(1, dir.clone());
        load_settings(&mut b, &path);
        assert_eq!((b.sim.world.resource("opt.fov"), b.sim.world.resource("opt.sens"), b.sim.world.resource("display.volume"), b.sim.world.resource("display.quality")), (Some(95.0), Some(1.5), Some(0.4), Some(2.0)));
        assert_eq!((b.sim.world.resource("score"), b.sim.world.resource("settings.save")), (None, None), "only opt.* and display.* are kept");
        // the text of the fields that remember is kept as text.NAME, and only theirs: a field with no name (a secret, a token) is not
        let fields = |e: &mut Engine, address: &str, name: &str, secret: &str| {
            for (uid, comps) in [(10, format!(r#"{{"Ui":{{"button":true}},"UiInput":{{"text":"{address}","remember":"address"}}}}"#)), (11, format!(r#"{{"Ui":{{"button":true}},"UiInput":{{"text":"{name}","remember":"name"}}}}"#)), (12, format!(r#"{{"Ui":{{"button":true}},"UiInput":{{"text":"{secret}"}}}}"#))] {
                let lunaris_json::Json::Obj(components) = lunaris_json::parse(&comps).unwrap() else { panic!() };
                e.sim.apply(&lunaris_sim::Command::Spawn { uid: Some(uid), components }).unwrap();
            }
        };
        let text_of = |e: &mut Engine, uid: u64| {
            let ent = e.sim.world.entity(uid).unwrap();
            let sym = e.sim.world.get::<lunaris_sim::components::UiInput>(ent).unwrap().text.0;
            e.sim.world.strings().get(sym).to_string()
        };
        let mut t1 = Engine::new(1, dir.clone());
        fields(&mut t1, "203.0.113.5:7777", "Ada", "0123456789abcdef");
        save_settings(&mut t1, &path).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("text.address") && saved.contains("203.0.113.5:7777") && saved.contains("text.name") && saved.contains("Ada"), "{saved}");
        assert!(!saved.contains("0123456789abcdef"), "a field that remembers nothing is never written: {saved}");
        let mut t2 = Engine::new(1, dir.clone());
        fields(&mut t2, "", "Survivor", "");
        load_settings(&mut t2, &path);
        assert_eq!((text_of(&mut t2, 10), text_of(&mut t2, 11), text_of(&mut t2, 12)), ("203.0.113.5:7777".to_string(), "Ada".to_string(), String::new()), "the address and the name come back, the secret field stays empty");
        // a change is noticed when the field has lost the focus, and not while it is typed in
        let mut last = remembered_now(&mut t2);
        keep_texts(&mut t2, &path, &mut last);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved, "nothing changed: nothing is written");
        lunaris_sim::ui_kit::restore_remembered(&mut t2.sim.world, "name", "Bo");
        keep_texts(&mut t2, &path, &mut last);
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("Bo") && !after.contains("Ada"), "{after}");
        let _ = std::fs::remove_file(&path);
        // a damaged file changes nothing
        std::fs::write(&path, "{ not json").unwrap();
        let mut c = Engine::new(1, dir.clone());
        load_settings(&mut c, &path);
        assert!(c.sim.world.resources().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A key the player rebound (`KeyBind`) is kept as `bind.ACTION.N` in `settings.json` and put back at the next start; a line that no longer fits the game is left alone.
    #[test]
    fn rebound_keys_are_kept_as_bind_lines_and_come_back() {
        let dir = std::env::temp_dir().join(format!("lunaris-binds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let scene = |e: &mut Engine| {
            for (uid, comps) in [
                (1u64, r#"{"InputMap":{"bindings":"jump = space, gamepad.a\nuse = e"}}"#),
                (10, r#"{"Ui":{"button":true},"KeyBind":{"action":"jump","index":0}}"#),
                (11, r#"{"Ui":{"button":true},"KeyBind":{"action":"use","index":0}}"#),
            ] {
                let lunaris_json::Json::Obj(components) = lunaris_json::parse(comps).unwrap() else { panic!() };
                e.sim.apply(&lunaris_sim::Command::Spawn { uid: Some(uid), components }).unwrap();
            }
        };
        let keys = |e: &mut Engine, action: &str| {
            let ent = e.sim.world.entity(1).unwrap();
            let sym = e.sim.world.get::<lunaris_sim::components::InputMap>(ent).unwrap().bindings.0;
            lunaris_sim::rebind::keys_of(e.sim.world.strings().get(sym), action)
        };
        std::fs::write(&path, r#"{"bind.jump.0": "x", "bind.use.0": "space", "bind.nothing.0": "q", "bind.jump.9": "q", "bind.use.x": "q", "bind.use.0.0": "q"}"#).unwrap();
        let mut a = Engine::new(1, dir.clone());
        scene(&mut a);
        load_settings(&mut a, &path);
        assert_eq!((keys(&mut a, "jump"), keys(&mut a, "use")), (vec!["x".to_string()], vec!["space".to_string()]), "the two lines that fit are back, the others (no such action, no such key, not a number) changed nothing");
        save_settings(&mut a, &path).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("bind.jump.0") && saved.contains("bind.use.0") && !saved.contains("nothing"), "{saved}");
        // and after a start from that file the same keys are kept again
        let mut b = Engine::new(1, dir.clone());
        scene(&mut b);
        load_settings(&mut b, &path);
        assert_eq!((keys(&mut b, "jump"), keys(&mut b, "use")), (vec!["x".to_string()], vec!["space".to_string()]));
        // the defaults again: the file keeps no line for a key that is the scene's own
        let mut c = Engine::new(1, dir.clone());
        scene(&mut c);
        save_settings(&mut c, &path).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("bind."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every key the window reports is a name a game can bind, and the extra keys have names.
    #[test]
    fn every_key_the_window_reports_can_be_bound() {
        for code in [KeyCode::KeyA, KeyCode::Digit0, KeyCode::Space, KeyCode::Enter, KeyCode::Tab, KeyCode::Backspace, KeyCode::ArrowUp, KeyCode::ShiftLeft, KeyCode::ControlRight, KeyCode::AltLeft, KeyCode::F1, KeyCode::F12, KeyCode::Minus, KeyCode::Comma, KeyCode::Slash, KeyCode::Backquote, KeyCode::CapsLock, KeyCode::PageDown, KeyCode::Numpad5, KeyCode::NumpadEnter] {
            let n = key_name(code).unwrap();
            assert!(lunaris_sim::rebind::is_bindable(&n), "{n}");
        }
        assert_eq!(key_name(KeyCode::Numpad5).as_deref(), Some("numpad5"));
        assert_eq!(key_name(KeyCode::BracketLeft).as_deref(), Some("bracketleft"));
    }
}
