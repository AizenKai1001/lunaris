//! `lunaris play`: run a scene in a native window. The keyboard and mouse become `input` resources (the same ones the editor and any
//! agent set), the simulation runs at its fixed rate, every frame is drawn by the GPU renderer (wgpu: Vulkan or DirectX 12) at the
//! window's own resolution in its high-quality mode (or the quality `--quality` names), and events play their sound effects. If the GPU
//! renderer cannot start, or with `--cpu`, the reference renderer draws and GDI blits the picture. Windows only for now; `lunaris edit`
//! works everywhere.
//!
//!   lunaris play examples/vista.lunaris.json [--size 1280x720] [--mute] [--quality high] [--cpu]
//!   lunaris play scene.json --hidden --frames 120 --hold d --press enter@30:4 --save-frame shot.png --report     (a self-test: no window shown; `--press KEY@FRAME[:FRAMES]` presses keys at set frames)
//!
//! Game controllers work too (Xbox-style pads: XInput on Windows, the joystick interface on Linux): `input.gamepad.a`, `input.gamepad.lx` and so on, which an
//! `InputMap` binds (`jump = space, gamepad.a`), and `rumble.low` / `rumble.high` resources shake the pad (Windows). `lunaris-pad` has the names.
//!
//! Looking around: the mouse's motion since the last tick is `input.mouse_dx` / `input.mouse_dy` (pixels, right and down positive), the wheel `input.mouse_wheel`
//! (notches), and a game that writes the resource `cursor.lock` above 0.5 gets the pointer hidden and held in the window (first-person games; Esc still quits). A
//! `MouseLook` component turns by them.
//!
//! Esc quits (unless the game sets the resource `app.quit_on_escape` to 0: then Esc is the key `input.escape`, and the game closes itself with `app.quit` above 0), F11 toggles full screen. Mouse position is `input.mouse_x` / `input.mouse_y` (0..1 across the picture, y down) and the
//! buttons are `input.mouse_left` / `input.mouse_right`.
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
    /// Draw with the reference CPU renderer even when a GPU is available.
    pub cpu: bool,
    /// The GPU renderer's quality: classic, low, medium, high or ultra.
    pub quality: String,
    /// Play over the network: take a seat in a `lunaris host` game (lockstep or rollback: every player's copy plays the same inputs), or
    /// watch one.
    pub join: Option<Join>,
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
        _ => return None,
    };
    Some(s.to_string())
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

/// The sound director and the output it plays into (dropping it closes the device).
struct Sound {
    director: Director,
    _output: Output,
}

struct App {
    engine: Engine,
    opts: Options,
    window: Option<Arc<Window>>,
    held: BTreeSet<String>,
    mouse: (f32, f32),
    /// Mouse motion and wheel since the last frame, the pointer held in the window (`cursor.lock`), whether zeros still have to be sent, whether the window has focus.
    look: (f32, f32),
    wheel: f32,
    look_sent: bool,
    cursor_locked: bool,
    focused: bool,
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

    fn release_all(&mut self) {
        let v: Vec<(String, bool)> = std::mem::take(&mut self.held).into_iter().map(|k| (k, false)).collect();
        self.press(v);
    }

    fn render_size(&self) -> (u32, u32) {
        let (w, h) = self.window.as_ref().map(|w| (w.inner_size().width, w.inner_size().height)).unwrap_or(self.opts.size);
        let (w, h) = (w.max(2) as f32, h.max(2) as f32);
        // draw at the window's size but never more than 1280x720: GDI scales the picture up to the window
        let k = (1280.0 / w).min(720.0 / h).min(1.0);
        (((w * k) as u32).max(16), ((h * k) as u32).max(16))
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
        if !self.opts.hidden {
            let moved = self.pads.poll();
            self.set_input(moved);
        }
        // the mouse's motion since the last frame goes into the first tick of this one, then back to zero
        // keys a self-test presses at set frames (one tick a frame, so the frame is the tick)
        let at = self.frames;
        let scripted: Vec<(String, bool)> = self.opts.presses.iter().flat_map(|(k, first, len)| [(at == *first).then(|| (k.clone(), true)), (at == first + len).then(|| (k.clone(), false))]).flatten().collect();
        if !scripted.is_empty() {
            self.press(scripted);
        }
        let (look, wheel) = (std::mem::take(&mut self.look), std::mem::take(&mut self.wheel));
        let moving = look != (0.0, 0.0) || wheel != 0.0;
        if moving || self.look_sent {
            self.set_input(vec![("mouse_dx".to_string(), look.0 as f64), ("mouse_dy".to_string(), look.1 as f64), ("mouse_wheel".to_string(), wheel as f64)]);
            self.look_sent = moving;
        }
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
                self.engine.sim.step();
                if k == 0 && self.look_sent {
                    self.set_input(vec![("mouse_dx".to_string(), 0.0), ("mouse_dy".to_string(), 0.0), ("mouse_wheel".to_string(), 0.0)]);
                    self.look_sent = false;
                }
            }
        }
        if !self.opts.hidden && self.pads.any_connected() {
            // the game shakes a pad by writing the resources `rumble.low` and `rumble.high` (0 to 1), `rumble2.*` for the second pad, ...
            for pad in 0..lunaris_pad::MAX_PADS {
                let p = if pad == 0 { "rumble".to_string() } else { format!("rumble{}", pad + 1) };
                let low = self.engine.sim.world.resource(&format!("{p}.low")).unwrap_or(0.0) as f32;
                let high = self.engine.sim.world.resource(&format!("{p}.high")).unwrap_or(0.0) as f32;
                self.pads.set_rumble(pad, low, high);
            }
        }
        // a game that wants the pointer (first-person look) hides it and holds it in the window while the window has focus
        let want_lock = !self.opts.hidden && self.focused && self.engine.sim.world.resource("cursor.lock").is_some_and(|v| v > 0.5);
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
        let mut drew_on_gpu = false;
        if let (Some(gpu), Some(window)) = (self.gpu.as_mut(), self.window.as_ref()) {
            let size = window.inner_size();
            if size.width == 0 || size.height == 0 {
                drew_on_gpu = true; // minimised: nothing to draw
            } else if self.opts.save_frame.is_some() {
                // a self-test that wants a picture: draw into an image so it can be saved, and show it
                let opts = lunaris_render::RenderOptions { width: size.width, height: size.height, ..Default::default() };
                match gpu.render_to_image(&mut self.engine.sim.world, &mut self.textures, &opts, &self.quality) {
                    Ok(f) => {
                        self.gpu_image = Some((f.width, f.height, f.rgba));
                        drew_on_gpu = true;
                    }
                    Err(e) => eprintln!("the GPU renderer failed ({e}); switching to the CPU renderer"),
                }
            } else {
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
        // sound for what happened in this frame
        if let Some(sound) = &mut self.sound {
            let events: Vec<lunaris_sim::Event> = self.engine.sim.events.iter().filter(|e| e.tick >= first_tick).cloned().collect();
            sound.director.update(&mut self.engine, &events);
        }
        self.frames += 1;
        self.fps_frames += 1;
        let since = now.duration_since(self.fps_start).as_secs_f32();
        if since >= 0.5 {
            self.fps = self.fps_frames as f32 / since;
            self.fps_start = now;
            self.fps_frames = 0;
            if let Some(w) = &self.window {
                let seat = self.net.as_ref().map(|n| if n.welcome().watching { format!(" - {}", n.status(&self.engine)) } else { format!(" - player {} of {} - {}", n.welcome().player + 1, n.welcome().players, n.status(&self.engine)) }).unwrap_or_default();
                w.set_title(&format!("Lunaris - {}{} - tick {} - {:.0} fps ({})", self.opts.scene, seat, self.engine.sim.tick, self.fps, if self.gpu.is_some() { format!("GPU {}", self.opts.quality) } else { "CPU".to_string() }));
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
        let attrs = Window::default_attributes().with_title(format!("Lunaris - {}", self.opts.scene)).with_inner_size(PhysicalSize::new(self.opts.size.0, self.opts.size.1)).with_visible(!self.opts.hidden);
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
                match lunaris_gpu::GpuRenderer::for_window(window.clone(), size.width, size.height, true) {
                    Ok(r) => {
                        println!("rendering on the GPU: {} ({} quality)", r.adapter_name(), self.opts.quality);
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
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Focused(true) => self.focused = true,
            WindowEvent::Focused(false) => {
                self.focused = false;
                self.release_all();
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
                    if !event.repeat && down != self.held.contains(&name) {
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
                    let (x, y) = ((position.x as f32 / s.width.max(1) as f32).clamp(0.0, 1.0), (position.y as f32 / s.height.max(1) as f32).clamp(0.0, 1.0));
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
                    _ => return,
                };
                self.press(vec![(name.to_string(), state == ElementState::Pressed)]);
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            if self.focused && !self.opts.hidden {
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
                // the simulation still advances in its own fixed steps. The small gap only guards against a driver that ignores vsync.
                self.next_frame = Instant::now() + Duration::from_millis(2);
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
    let sound = if opts.mute {
        None
    } else {
        let mixer = Arc::new(Mutex::new(Mixer::new()));
        mixer.lock().unwrap_or_else(|p| p.into_inner()).master = opts.volume;
        match Output::open(mixer.clone()) {
            Ok(output) => {
                let mut director = Director::new(mixer);
                director.spawn_tracer();
                Some(Sound { director, _output: output })
            }
            Err(e) => {
                eprintln!("note: no sound ({e})");
                None
            }
        }
    };
    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: cannot start the window system: {e}");
            return ExitCode::FAILURE;
        }
    };
    let now = Instant::now();
    let mut app = App {
        engine,
        opts,
        window: None,
        held: BTreeSet::new(),
        mouse: (-1.0, -1.0),
        look: (0.0, 0.0),
        wheel: 0.0,
        look_sent: false,
        cursor_locked: false,
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
        last_frame: None,
        bgra: Vec::new(),
        error: None,
        gpu: None,
        quality,
        textures: TextureCache::new(Some(root_for_textures)),
        gpu_image: None,
        net,
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
            match png.and_then(|png| std::fs::write(path, png).map_err(|e| e.to_string())) {
                Ok(()) => println!("saved the last frame to {}", path.display()),
                Err(e) => eprintln!("could not save the frame: {e}"),
            }
        }
    }
    if app.opts.report {
        let resources = app.engine.call("resources", &Json::Null).ok().and_then(|r| r.get("resources").cloned()).unwrap_or(Json::Null);
        println!("frames {}, tick {}, state hash {:016x}", app.frames, app.engine.sim.tick, app.engine.sim.state_hash());
        if let Some(g) = &app.gpu {
            println!("presented to the window: {} pictures", g.frames_presented);
        }
        println!("resources {}", resources.to_string_compact().unwrap_or_default());
        if let Some(s) = &app.sound {
            let started: Vec<String> = s.director.stats.started.iter().map(|(k, v)| format!("{k} x{v}")).collect();
            println!("sound: {} voices started ({}), {} traces, {} room analyses, room rt60 {:.2} s{}", s.director.stats.started.values().sum::<u64>(), started.join(", "), s.director.stats.traces, s.director.stats.room_analyses, s.director.room.rt60, if s.director.warnings.is_empty() { String::new() } else { format!(", warnings: {}", s.director.warnings.join("; ")) });
        }
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
}
