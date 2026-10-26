//! Named resources the server tells every client (`NetHost.sync`, docs/GAME_SERVER.md, "Resources every client is told"): the world clock, the weather, the zone's size: numbers that only the server decides and
//! every client only shows. The server reads the resources the scene names from its own world and sends the ones that changed (at most once per `NetHost.sync_every` seconds), all of them every few seconds as a
//! refresh, and all of them at once to a client that has just joined; the client writes them into its own simulation as resources of the same names.
//!
//! The message is reliable and lazy (it never makes a quiet client's packet come sooner): `[SYNC][count u8]` and for each resource `[name length u8][name][tag u8][value]`, the tag saying how the value is
//! written: 0 a whole number from 0 to 255 (one byte), 1 an `f32` that is exactly the value (four bytes), 2 an `f64` (eight). A clock that changes every tick is one entry of about 14 bytes once a second.
use super::bytes::{R, W};
use lunaris_ecs::World;

/// The kinds of message of this layer (the first byte of a reliable message).
pub mod msg {
    /// The server's resources: this byte, the number of them, and each as a name and a value.
    pub const SYNC: u8 = 0x48;
}

/// How many names `NetHost.sync` can list (the rest are ignored), and the longest name.
pub const MAX_NAMES: usize = 32;
const MAX_NAME: usize = 60;
/// Every how many seconds all the resources are sent again whether they changed or not.
pub const REFRESH_SECONDS: f32 = 5.0;

/// The names a `NetHost.sync` text lists: separated by spaces, commas or lines, valid resource names only, each once, at most [`MAX_NAMES`]. Names of the engine's own resources (`input.`, `action.`,
/// `game.`, `scene.`, `time.`) are left out: they are not the game's to replicate.
pub fn parse_names(text: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for n in text.split(|c: char| c.is_whitespace() || c == ',').filter(|n| !n.is_empty()) {
        if n.len() <= MAX_NAME && allowed(n) && !v.iter().any(|x| x == n) && v.len() < MAX_NAMES {
            v.push(n.to_string());
        }
    }
    v
}

/// May the server set a resource of this name on a client? A valid name that is not one of the engine's own.
pub fn allowed(name: &str) -> bool {
    lunaris_ecs::valid_resource_name(name) && !["input.", "action.", "game.", "scene.", "time."].iter().any(|p| name.starts_with(p))
}

/// The message with these resources.
pub fn encode(entries: &[(&str, f64)]) -> Vec<u8> {
    let mut w = W::new();
    w.u8(msg::SYNC);
    let n = entries.len().min(MAX_NAMES);
    w.u8(n as u8);
    for (name, v) in entries.iter().take(n) {
        let b = name.as_bytes();
        let b = &b[..b.len().min(MAX_NAME)];
        w.u8(b.len() as u8);
        w.raw(b);
        if v.fract() == 0.0 && (0.0..=255.0).contains(v) && !(*v == 0.0 && v.is_sign_negative()) {
            w.u8(0);
            w.u8(*v as u8);
        } else if (*v as f32) as f64 == *v {
            w.u8(1);
            w.f32(*v as f32);
        } else {
            w.u8(2);
            w.raw(&v.to_le_bytes());
        }
    }
    w.buf
}

/// Read a message of [`msg::SYNC`]: the resources it carries (a name that is not allowed, or a value that is not finite, is dropped).
pub fn parse(data: &[u8]) -> Option<Vec<(String, f64)>> {
    let mut r = R::new(data);
    if r.u8()? != msg::SYNC {
        return None;
    }
    let n = r.u8()? as usize;
    let mut out = Vec::new();
    for _ in 0..n.min(MAX_NAMES) {
        let len = r.u8()? as usize;
        let name = String::from_utf8_lossy(r.raw(len)?).into_owned();
        let v = match r.u8()? {
            0 => r.u8()? as f64,
            1 => r.f32()? as f64,
            2 => {
                let b = r.raw(8)?;
                f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
            }
            _ => return None,
        };
        if v.is_finite() && allowed(&name) {
            out.push((name, v));
        }
    }
    Some(out)
}

/// A client: write what the server said into this simulation as resources. Returns how many were written.
pub fn apply(world: &mut World, entries: &[(String, f64)]) -> usize {
    let mut n = 0;
    for (name, v) in entries {
        if world.resource(name) != Some(*v) && world.set_resource(name, Some(*v)).is_ok() {
            n += 1;
        }
    }
    n
}

/// The server's side: what the resources were when they were last sent, and when.
#[derive(Default)]
pub struct SyncOut {
    told: Vec<Option<f64>>,
    last: Option<u32>,
    refreshed: Option<u32>,
}

impl SyncOut {
    /// The message to send everybody at this server tick, if one is due: the resources that changed since the last one (all of them when a refresh is due), but not more often than every `every` seconds.
    pub fn due(&mut self, world: &World, names: &[String], tick: u32, hz: u32, every: f32) -> Option<Vec<u8>> {
        if names.is_empty() {
            return None;
        }
        let hz = hz.max(1);
        if self.told.len() != names.len() {
            self.told = vec![None; names.len()];
        }
        let gap = ((if every.is_finite() { every.max(0.0) } else { 1.0 }) * hz as f32) as u32;
        if self.last.is_some_and(|l| tick.wrapping_sub(l) < gap.max(1)) {
            return None;
        }
        let refresh = self.refreshed.is_none_or(|r| tick.wrapping_sub(r) as f32 >= REFRESH_SECONDS * hz as f32);
        let mut entries: Vec<(&str, f64)> = Vec::new();
        for (i, name) in names.iter().enumerate() {
            let Some(v) = world.resource(name).filter(|v| v.is_finite()) else { continue };
            if refresh || self.told[i] != Some(v) {
                entries.push((name.as_str(), v));
                self.told[i] = Some(v);
            }
        }
        if entries.is_empty() {
            return None;
        }
        self.last = Some(tick);
        if refresh {
            self.refreshed = Some(tick);
        }
        Some(encode(&entries))
    }

    /// The message with every resource as it is now, for a client that has just joined.
    pub fn full(&self, world: &World, names: &[String]) -> Option<Vec<u8>> {
        let entries: Vec<(&str, f64)> = names.iter().filter_map(|n| world.resource(n).filter(|v| v.is_finite()).map(|v| (n.as_str(), v))).collect();
        (!entries.is_empty()).then(|| encode(&entries))
    }
}
