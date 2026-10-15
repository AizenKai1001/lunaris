//! Game controllers, standard library only. [`Pads::poll`] says which `gamepad.*` inputs changed since the last call, as `(name, value)` pairs that the
//! player writes into the game's `input.*` resources, so an `InputMap` can bind `gamepad.a` or `gamepad.lx` like any key. [`Pads::set_rumble`] shakes a pad.
//!
//! * Windows: XInput (Xbox 360 / One / Series pads and everything that pretends to be one), looked up in `xinput1_4.dll` when the first pad is asked for, so
//!   nothing has to be installed to build and a machine without it has no pads. Up to four pads; rumble works.
//! * Linux: the kernel's joystick interface (`/dev/input/js0` to `js3`) with the Xbox-style numbering of the `xpad` driver (and what the `xpadneo` and Steam
//!   Input drivers present). No rumble yet.
//! * Anywhere else there are no pads and the calls do nothing.
//!
//! Names (pad 1 is `gamepad`, pad 2 `gamepad2`, up to `gamepad4`): the buttons `a b x y lb rb back start ls rs dpad_up dpad_down dpad_left dpad_right` are 1 while
//! held; the sticks `lx ly rx ry` run from -1 to 1 with **up and right positive**; the triggers `lt rt` from 0 to 1; `connected` is 1 while the pad is plugged in
//! (every name of a pad that goes away drops to 0). A stick inside a small dead zone (0.08) reads 0 and every value is rounded to 1/32, so an idle pad does not
//! send a stream of changes (which matters in a networked game, where every change is a message).

use std::io::Read;
#[allow(unused_imports)]
use std::time::{Duration, Instant};

/// The most pads at once.
pub const MAX_PADS: usize = 4;

/// The names of the buttons, in the order of [`PadState::buttons`].
pub const BUTTONS: [&str; 14] = ["a", "b", "x", "y", "lb", "rb", "back", "start", "ls", "rs", "dpad_up", "dpad_down", "dpad_left", "dpad_right"];
/// The names of the axes, in the order of [`PadState::axes`].
pub const AXES: [&str; 6] = ["lx", "ly", "rx", "ry", "lt", "rt"];

/// What a pad is doing, in the engine's terms.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PadState {
    pub buttons: [bool; 14],
    /// lx, ly, rx, ry (-1..1, up and right positive), lt, rt (0..1)
    pub axes: [f32; 6],
}

/// The resource-name prefix of pad `pad` (0 based): `gamepad`, `gamepad2`, ...
pub fn prefix(pad: usize) -> String {
    if pad == 0 {
        "gamepad".to_string()
    } else {
        format!("gamepad{}", pad + 1)
    }
}

fn round32(v: f32) -> f32 {
    (v * 32.0).round() / 32.0
}

/// A stick from raw readings (-32768..32767 each way, up positive): inside the dead zone it is the middle, and it is rounded.
fn stick(x: i32, y: i32) -> (f32, f32) {
    let (fx, fy) = ((x as f32 / 32767.0).clamp(-1.0, 1.0), (y as f32 / 32767.0).clamp(-1.0, 1.0));
    if (fx * fx + fy * fy).sqrt() < 0.08 {
        return (0.0, 0.0);
    }
    (round32(fx), round32(fy))
}

/// A trigger from 0 (let go) to `full`.
fn trigger(v: i32, full: i32) -> f32 {
    let t = (v as f32 / full as f32).clamp(0.0, 1.0);
    if t < 0.04 {
        0.0
    } else {
        round32(t)
    }
}

/// What XInput reports (`XINPUT_GAMEPAD`), kept as plain numbers so the mapping can be tested everywhere.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct RawXInput {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}

impl PadState {
    pub fn from_xinput(r: &RawXInput) -> PadState {
        let bit = |m: u16| r.buttons & m != 0;
        let (lx, ly) = stick(r.thumb_lx as i32, r.thumb_ly as i32);
        let (rx, ry) = stick(r.thumb_rx as i32, r.thumb_ry as i32);
        PadState {
            buttons: [
                bit(0x1000), // a
                bit(0x2000), // b
                bit(0x4000), // x
                bit(0x8000), // y
                bit(0x0100), // lb
                bit(0x0200), // rb
                bit(0x0020), // back
                bit(0x0010), // start
                bit(0x0040), // ls
                bit(0x0080), // rs
                bit(0x0001), // dpad up
                bit(0x0002), // dpad down
                bit(0x0004), // dpad left
                bit(0x0008), // dpad right
            ],
            axes: [lx, ly, rx, ry, trigger(r.left_trigger as i32, 255), trigger(r.right_trigger as i32, 255)],
        }
    }
}

/// What a pad's (name, value) pairs become when it goes from `before` to `now` (`None` is not plugged in): everything when it appears, only what changed
/// afterwards, and all zeros when it goes away.
pub fn diff(pad: usize, before: Option<&PadState>, now: Option<&PadState>) -> Vec<(String, f64)> {
    let p = prefix(pad);
    let mut out = Vec::new();
    match (before, now) {
        (None, None) => {}
        (None, Some(n)) => {
            out.push((format!("{p}.connected"), 1.0));
            for (i, name) in BUTTONS.iter().enumerate() {
                out.push((format!("{p}.{name}"), n.buttons[i] as u8 as f64));
            }
            for (i, name) in AXES.iter().enumerate() {
                out.push((format!("{p}.{name}"), n.axes[i] as f64));
            }
        }
        (Some(_), None) => {
            out.push((format!("{p}.connected"), 0.0));
            for name in BUTTONS.iter().chain(AXES.iter()) {
                out.push((format!("{p}.{name}"), 0.0));
            }
        }
        (Some(b), Some(n)) => {
            for (i, name) in BUTTONS.iter().enumerate() {
                if b.buttons[i] != n.buttons[i] {
                    out.push((format!("{p}.{name}"), n.buttons[i] as u8 as f64));
                }
            }
            for (i, name) in AXES.iter().enumerate() {
                if b.axes[i] != n.axes[i] {
                    out.push((format!("{p}.{name}"), n.axes[i] as f64));
                }
            }
        }
    }
    out
}

// ---- the Linux joystick interface ------------------------------------------------------------------------------------------------------------------

/// One `struct js_event` of `<linux/joystick.h>`: 8 bytes, little endian.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JsEvent {
    pub value: i16,
    /// 1 button, 2 axis (the "initial state" flag 0x80 is removed)
    pub kind: u8,
    pub number: u8,
}

/// The events at the front of `bytes` (whole 8-byte records; a partial one stays for the next read).
pub fn parse_js_events(bytes: &mut Vec<u8>) -> Vec<JsEvent> {
    let whole = bytes.len() / 8 * 8;
    let events = bytes[..whole].chunks_exact(8).map(|c| JsEvent { value: i16::from_le_bytes([c[4], c[5]]), kind: c[6] & !0x80, number: c[7] }).collect();
    bytes.drain(..whole);
    events
}

/// A joystick as the kernel numbers it for an Xbox-style pad: axes 0 lx, 1 ly (down positive), 2 lt, 3 rx, 4 ry, 5 rt, 6 hat x, 7 hat y (down positive);
/// buttons 0 a, 1 b, 2 x, 3 y, 4 lb, 5 rb, 6 back, 7 start, 8 guide, 9 ls, 10 rs.
#[derive(Clone, Debug)]
pub struct JsState {
    axes: [i16; 8],
    buttons: [bool; 16],
}

impl Default for JsState {
    /// Before the kernel has said anything: nothing held, the triggers let go (they rest at the bottom of their range).
    fn default() -> JsState {
        JsState { axes: [0, 0, -32767, 0, 0, -32767, 0, 0], buttons: [false; 16] }
    }
}

impl JsState {
    pub fn apply(&mut self, e: JsEvent) {
        match e.kind {
            1 if (e.number as usize) < self.buttons.len() => self.buttons[e.number as usize] = e.value != 0,
            2 if (e.number as usize) < self.axes.len() => self.axes[e.number as usize] = e.value,
            _ => {}
        }
    }

    pub fn to_pad(&self) -> PadState {
        let a = |i: usize| self.axes[i] as i32;
        let b = |i: usize| self.buttons[i];
        let (lx, ly) = stick(a(0), -a(1));
        let (rx, ry) = stick(a(3), -a(4));
        PadState {
            buttons: [b(0), b(1), b(2), b(3), b(4), b(5), b(6), b(7), b(9), b(10), a(7) < -16384, a(7) > 16384, a(6) < -16384, a(6) > 16384],
            // the triggers run from -32767 (let go) to 32767 (pulled)
            axes: [lx, ly, rx, ry, trigger(a(2) + 32767, 65534), trigger(a(5) + 32767, 65534)],
        }
    }
}

/// A joystick device that is read without waiting: `read` returning no bytes means nothing new, an error means the pad is gone.
pub struct Joystick {
    source: Box<dyn Read + Send>,
    state: JsState,
    pending: Vec<u8>,
}

impl Joystick {
    pub fn new(source: Box<dyn Read + Send>) -> Joystick {
        Joystick { source, state: JsState::default(), pending: Vec::new() }
    }

    /// Read what has arrived. `Err` when the device has gone (or cannot be read).
    pub fn poll(&mut self) -> Result<PadState, std::io::Error> {
        let mut buf = [0u8; 512];
        loop {
            match self.source.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    self.pending.extend_from_slice(&buf[..n]);
                    for e in parse_js_events(&mut self.pending) {
                        self.state.apply(e);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(self.state.to_pad())
    }
}

// ---- the backends --------------------------------------------------------------------------------------------------------------------------------

#[cfg(windows)]
mod backend {
    use super::*;
    use std::ffi::c_void;

    extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }

    #[repr(C)]
    struct State {
        packet: u32,
        pad: RawXInput,
    }
    #[repr(C)]
    struct Vibration {
        left: u16,
        right: u16,
    }
    type GetState = unsafe extern "system" fn(u32, *mut State) -> u32;
    type SetState = unsafe extern "system" fn(u32, *mut Vibration) -> u32;

    pub struct Backend {
        get: Option<GetState>,
        set: Option<SetState>,
        up: [bool; MAX_PADS],
        next_probe: [Instant; MAX_PADS],
    }

    impl Backend {
        pub fn new() -> Backend {
            let mut b = Backend { get: None, set: None, up: [false; MAX_PADS], next_probe: [Instant::now(); MAX_PADS] };
            for dll in ["xinput1_4.dll", "xinput1_3.dll", "xinput9_1_0.dll"] {
                let wide: Vec<u16> = dll.encode_utf16().chain(std::iter::once(0)).collect();
                // SAFETY: a valid, null-terminated UTF-16 name; a missing library gives null
                let module = unsafe { LoadLibraryW(wide.as_ptr()) };
                if module.is_null() {
                    continue;
                }
                // SAFETY: the library is loaded and the names are null-terminated; the signatures are XInput's documented ones
                unsafe {
                    let get = GetProcAddress(module, b"XInputGetState\0".as_ptr());
                    let set = GetProcAddress(module, b"XInputSetState\0".as_ptr());
                    if !get.is_null() {
                        b.get = Some(std::mem::transmute::<*mut c_void, GetState>(get));
                        if !set.is_null() {
                            b.set = Some(std::mem::transmute::<*mut c_void, SetState>(set));
                        }
                        break;
                    }
                }
            }
            b
        }

        pub fn read(&mut self, slot: usize) -> Option<PadState> {
            let get = self.get?;
            // asking for a pad that is not there is slow on some systems: look again once a second
            if !self.up[slot] && Instant::now() < self.next_probe[slot] {
                return None;
            }
            let mut st = State { packet: 0, pad: RawXInput::default() };
            // SAFETY: `st` is a valid XINPUT_STATE for the call
            let r = unsafe { get(slot as u32, &mut st) };
            if r == 0 {
                self.up[slot] = true;
                Some(PadState::from_xinput(&st.pad))
            } else {
                self.up[slot] = false;
                self.next_probe[slot] = Instant::now() + Duration::from_secs(1);
                None
            }
        }

        pub fn rumble(&mut self, slot: usize, low: u16, high: u16) {
            if let Some(set) = self.set {
                let mut v = Vibration { left: low, right: high };
                // SAFETY: `v` is a valid XINPUT_VIBRATION for the call
                unsafe {
                    set(slot as u32, &mut v);
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod backend {
    use super::*;
    use std::os::unix::fs::OpenOptionsExt;

    pub struct Backend {
        devices: [Option<Joystick>; MAX_PADS],
        next_scan: [Instant; MAX_PADS],
    }

    impl Backend {
        pub fn new() -> Backend {
            Backend { devices: [None, None, None, None], next_scan: [Instant::now(); MAX_PADS] }
        }

        pub fn read(&mut self, slot: usize) -> Option<PadState> {
            if self.devices[slot].is_none() {
                if Instant::now() < self.next_scan[slot] {
                    return None;
                }
                self.next_scan[slot] = Instant::now() + Duration::from_secs(2);
                // O_NONBLOCK: a read with nothing to say returns at once
                match std::fs::OpenOptions::new().read(true).custom_flags(0o4000).open(format!("/dev/input/js{slot}")) {
                    Ok(f) => self.devices[slot] = Some(Joystick::new(Box::new(f))),
                    Err(_) => return None,
                }
            }
            match self.devices[slot].as_mut()?.poll() {
                Ok(s) => Some(s),
                Err(_) => {
                    self.devices[slot] = None;
                    None
                }
            }
        }

        pub fn rumble(&mut self, _slot: usize, _low: u16, _high: u16) {}
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod backend {
    use super::*;

    pub struct Backend;

    impl Backend {
        pub fn new() -> Backend {
            Backend
        }
        pub fn read(&mut self, _slot: usize) -> Option<PadState> {
            None
        }
        pub fn rumble(&mut self, _slot: usize, _low: u16, _high: u16) {}
    }
}

/// The pads of this machine.
pub struct Pads {
    backend: backend::Backend,
    state: [Option<PadState>; MAX_PADS],
    rumble: [(u16, u16); MAX_PADS],
}

impl Default for Pads {
    fn default() -> Pads {
        Pads::new()
    }
}

impl Pads {
    pub fn new() -> Pads {
        Pads { backend: backend::Backend::new(), state: [None; MAX_PADS], rumble: [(0, 0); MAX_PADS] }
    }

    /// What changed since the last call, as `(name, value)` pairs (`gamepad.a` 1, `gamepad.lx` 0.5, ...). The first call after a pad appears has all of its names.
    pub fn poll(&mut self) -> Vec<(String, f64)> {
        let mut out = Vec::new();
        for slot in 0..MAX_PADS {
            let now = self.backend.read(slot);
            out.extend(diff(slot, self.state[slot].as_ref(), now.as_ref()));
            if now.is_none() {
                self.rumble[slot] = (0, 0);
            }
            self.state[slot] = now;
        }
        out
    }

    /// Whether any pad is plugged in (as of the last `poll`).
    pub fn any_connected(&self) -> bool {
        self.state.iter().any(|s| s.is_some())
    }

    /// Shake pad `pad` (0 based): `low` is the heavy motor and `high` the light one, each 0 to 1. Does nothing for a pad that is not there; the motors are
    /// only told when the strength changes.
    pub fn set_rumble(&mut self, pad: usize, low: f32, high: f32) {
        if pad >= MAX_PADS || self.state[pad].is_none() {
            return;
        }
        let to = |v: f32| (v.clamp(0.0, 1.0) * 65535.0) as u16;
        let want = (to(low), to(high));
        if self.rumble[pad] != want {
            self.rumble[pad] = want;
            self.backend.rumble(pad, want.0, want.1);
        }
    }
}

impl Drop for Pads {
    fn drop(&mut self) {
        for slot in 0..MAX_PADS {
            if self.rumble[slot] != (0, 0) {
                self.backend.rumble(slot, 0, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(d: &[(String, f64)], name: &str) -> Option<f64> {
        d.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }

    #[test]
    fn xinput_buttons_sticks_and_triggers_become_names() {
        let raw = RawXInput { buttons: 0x1000 | 0x0100 | 0x0008 | 0x0010, left_trigger: 255, right_trigger: 128, thumb_lx: 32767, thumb_ly: -32768, thumb_rx: 1000, thumb_ry: 0 };
        let p = PadState::from_xinput(&raw);
        let d = diff(0, None, Some(&p));
        assert_eq!(get(&d, "gamepad.connected"), Some(1.0));
        for held in ["gamepad.a", "gamepad.lb", "gamepad.dpad_right", "gamepad.start"] {
            assert_eq!(get(&d, held), Some(1.0), "{held}");
        }
        for free in ["gamepad.b", "gamepad.x", "gamepad.y", "gamepad.rb", "gamepad.back", "gamepad.dpad_up", "gamepad.ls"] {
            assert_eq!(get(&d, free), Some(0.0), "{free}");
        }
        assert_eq!(get(&d, "gamepad.lx"), Some(1.0));
        assert_eq!(get(&d, "gamepad.ly"), Some(-1.0), "down is negative: up is positive");
        assert_eq!(get(&d, "gamepad.rx"), Some(0.0), "inside the dead zone");
        assert_eq!(get(&d, "gamepad.lt"), Some(1.0));
        assert!((get(&d, "gamepad.rt").unwrap() - 0.5).abs() < 0.04);
    }

    #[test]
    fn only_changes_are_reported_and_a_pad_that_goes_away_lets_go_of_everything() {
        let mut p = PadState::default();
        let first = diff(1, None, Some(&p));
        assert_eq!(first.len(), 1 + 14 + 6);
        assert!(first.iter().all(|(n, _)| n.starts_with("gamepad2.")), "the second pad is gamepad2");
        assert!(diff(1, Some(&p), Some(&p)).is_empty(), "nothing changed, nothing sent");
        let before = p;
        p.buttons[0] = true;
        p.axes[0] = 0.5;
        let d = diff(1, Some(&before), Some(&p));
        assert_eq!(d, vec![("gamepad2.a".to_string(), 1.0), ("gamepad2.lx".to_string(), 0.5)]);
        let gone = diff(1, Some(&p), None);
        assert_eq!(get(&gone, "gamepad2.connected"), Some(0.0));
        assert!(gone.iter().all(|(_, v)| *v == 0.0) && gone.len() == 21);
        assert!(diff(1, None, None).is_empty());
        assert_eq!((prefix(0), prefix(3)), ("gamepad".to_string(), "gamepad4".to_string()));
    }

    #[test]
    fn a_resting_stick_is_quiet_and_values_are_rounded() {
        // a worn stick that wobbles by a few percent says nothing at all
        for wobble in [0, 300, -700, 1500, -2000] {
            let p = PadState::from_xinput(&RawXInput { thumb_lx: wobble, thumb_ly: wobble / 2, ..Default::default() });
            assert_eq!((p.axes[0], p.axes[1]), (0.0, 0.0), "{wobble}");
        }
        // past it, the stick moves in steps of 1/32
        let a = PadState::from_xinput(&RawXInput { thumb_lx: 16000, ..Default::default() });
        let b = PadState::from_xinput(&RawXInput { thumb_lx: 16100, ..Default::default() });
        assert_eq!(a.axes[0], b.axes[0]);
        assert_eq!(a.axes[0] * 32.0, (a.axes[0] * 32.0).round());
        assert_eq!(PadState::from_xinput(&RawXInput { left_trigger: 5, ..Default::default() }).axes[4], 0.0, "a trigger that is nearly let go is let go");
    }

    fn js(kind: u8, number: u8, value: i16) -> [u8; 8] {
        let v = value.to_le_bytes();
        [0, 0, 0, 0, v[0], v[1], kind, number]
    }

    #[test]
    fn linux_joystick_events_make_the_same_names() {
        let mut bytes: Vec<u8> = Vec::new();
        // the kernel starts with the whole state, flagged 0x80
        for e in [js(0x81, 0, 0), js(0x82, 2, -32767), js(0x82, 5, -32767), js(0x82, 0, 0)] {
            bytes.extend_from_slice(&e);
        }
        // then the player: A held, left stick pushed up and to the right, left trigger half way, d-pad left
        for e in [js(1, 0, 1), js(2, 0, 32767), js(2, 1, -32767), js(2, 2, 0), js(2, 6, -32767)] {
            bytes.extend_from_slice(&e);
        }
        bytes.extend_from_slice(&[9, 9, 9]); // half a record stays for the next read
        let mut pending = bytes.clone();
        let events = parse_js_events(&mut pending);
        assert_eq!(events.len(), 9);
        assert_eq!(pending, vec![9, 9, 9]);
        assert_eq!(events[0], JsEvent { value: 0, kind: 1, number: 0 }, "the initial-state flag is removed");

        let mut pad = Joystick::new(Box::new(std::io::Cursor::new(bytes)));
        let p = pad.poll().unwrap();
        assert!(p.buttons[0], "a");
        assert_eq!((p.axes[0], p.axes[1]), (1.0, 1.0), "right and up, both positive");
        assert!((p.axes[4] - 0.5).abs() < 0.04, "the trigger half way: {}", p.axes[4]);
        assert_eq!(p.axes[5], 0.0);
        assert!(p.buttons[12] && !p.buttons[13] && !p.buttons[10], "the d-pad comes from the hat axes");
    }

    #[test]
    fn a_joystick_that_errors_is_gone_and_one_with_nothing_to_say_is_not() {
        struct Gone;
        impl Read for Gone {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "no such device"))
            }
        }
        struct Quiet;
        impl Read for Quiet {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::WouldBlock.into())
            }
        }
        assert!(Joystick::new(Box::new(Gone)).poll().is_err());
        assert_eq!(Joystick::new(Box::new(Quiet)).poll().unwrap(), PadState::default());
    }

    #[test]
    fn this_machine_can_be_asked_and_a_missing_pad_is_not_an_error() {
        // on a build machine with no controller nothing is connected and nothing is reported; with one, the names are all `gamepad*`
        let mut pads = Pads::new();
        let d = pads.poll();
        assert!(d.iter().all(|(n, _)| n.starts_with("gamepad")));
        pads.set_rumble(0, 1.0, 1.0);
        pads.set_rumble(9, 1.0, 1.0);
        let _ = pads.any_connected();
    }
}
