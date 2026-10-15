//! A stereo mixer at 44.1 kHz.
//!
//! A voice plays a [`Clip`] once or in a loop at some volume and pitch. A voice may sit at a place in the world ([`Spatial`]): it then
//! follows the inverse-distance law (full loudness inside `near` metres, `near / distance` beyond, fading to nothing at `range`) and is
//! panned between the ears by the direction of the listener's right-hand side. A voice may also carry [`Acoustics`] (what a ray tracer
//! found out about the way from the source to the listener, see `trace`): a low-pass for the direct sound, a handful of echoes, and a
//! send into the room's reverberator.
//!
//! Gains move smoothly over a block of samples, so a source that moves, a door that opens or a listener that turns does not click.
//! [`Mixer::render`] is a plain function of what happened before it, which is how a test, or an agent without ears, listens.

use crate::reverb::Reverb;
use crate::trace::{V3, SPEED_OF_SOUND};
use crate::wav::Clip;
use std::sync::Arc;

pub const RATE: u32 = 44_100;
/// Voices that can play at once; a new voice replaces the quietest one that is not looping.
pub const MAX_VOICES: usize = 32;
/// The longest echo, seconds.
pub const MAX_ECHO: f32 = 0.26;
const HISTORY: usize = (RATE as f32 * MAX_ECHO) as usize + 4;
/// Echoes at most this many per voice.
pub const MAX_TAPS: usize = 16;

pub type VoiceId = u64;

fn finite(x: f32, default: f32) -> f32 {
    if x.is_finite() {
        x
    } else {
        default
    }
}

/// The listener: where the ears are and which way the right ear points (a unit vector).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listener {
    pub pos: V3,
    pub right: V3,
}

impl Default for Listener {
    fn default() -> Listener {
        Listener { pos: [0.0; 3], right: [1.0, 0.0, 0.0] }
    }
}

/// Where a voice is and how far it carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spatial {
    pub pos: V3,
    /// Beyond this distance (metres) it is silent; the last quarter fades out.
    pub range: f32,
    /// Inside this distance (metres) it is at full loudness.
    pub near: f32,
}

impl Spatial {
    pub fn at(pos: V3) -> Spatial {
        Spatial { pos, range: 40.0, near: 2.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub volume: f32,
    /// Playback speed: 1 = as recorded, 2 = an octave up and twice as fast.
    pub pitch: f32,
    pub looped: bool,
    pub spatial: Option<Spatial>,
}

impl Default for Params {
    fn default() -> Params {
        Params { volume: 1.0, pitch: 1.0, looped: false, spatial: None }
    }
}

/// One echo of a voice: it arrives `delay` seconds after the direct sound at `gain` times the loudness the direct sound has at a metre, from
/// direction `dir` (unit, or shorter when the echo is spread over several directions).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tap {
    pub delay: f32,
    pub gain: f32,
    pub dir: V3,
}

/// What the way from a source to the listener does to its sound.
#[derive(Clone, Debug, PartialEq)]
pub struct Acoustics {
    /// 0..1: amplitude of the direct sound that gets through.
    pub direct_gain: f32,
    /// Low-pass for the direct sound, Hz; 20000 or more = none.
    pub direct_cutoff: f32,
    /// How much of the voice goes into the room's reverberator (1 = as much as the room's own level says).
    pub send: f32,
    pub taps: Vec<Tap>,
}

impl Default for Acoustics {
    /// Nothing in the way, no echoes, no reverb.
    fn default() -> Acoustics {
        Acoustics { direct_gain: 1.0, direct_cutoff: 20_000.0, send: 0.0, taps: Vec::new() }
    }
}

/// The room around the listener, as the reverberator wants it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoomSettings {
    pub rt60: f32,
    pub damping: f32,
    /// Level of the reverberation (amplitude, relative to a source heard at a metre).
    pub wet: f32,
    pub predelay: f32,
}

impl RoomSettings {
    pub const DRY: RoomSettings = RoomSettings { rt60: 0.3, damping: 0.3, wet: 0.0, predelay: 0.0 };
}

struct TapState {
    delay_now: f32,
    gain_now: f32,
    delay: f32,
    gain: f32,
    pan_l: f32,
    pan_r: f32,
}

struct Voice {
    id: VoiceId,
    clip: Arc<Clip>,
    pos: f64,
    params: Params,
    gl: f32,
    gr: f32,
    started: bool,
    ended: bool,
    // acoustics
    acoustics: Option<Acoustics>,
    lp: f32,
    hist: Vec<f32>,
    hist_pos: usize,
    taps: Vec<TapState>,
    send_now: f32,
    tail: f32,
}

/// Gains for a source: (left, right, distance gain). The distance gain is the inverse-distance law with a fade at the end of the range.
pub fn spatial_gains(listener: &Listener, s: &Spatial) -> (f32, f32, f32) {
    let to = [s.pos[0] - listener.pos[0], s.pos[1] - listener.pos[1], s.pos[2] - listener.pos[2]];
    let d = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
    let near = finite(s.near, 2.0).max(0.01);
    let range = finite(s.range, 40.0).max(0.1);
    let mut g = if d <= near { 1.0 } else { near / d };
    let fade_start = range * 0.75;
    if d >= range {
        g = 0.0;
    } else if d > fade_start {
        let w = (range - d) / (range - fade_start);
        g *= w * w;
    }
    // sideways: the cosine between the direction to the source and the right ear; right in front of the face (or on top of it) is centred
    let pan = if d > 1e-4 { (to[0] * listener.right[0] + to[1] * listener.right[1] + to[2] * listener.right[2]) / d * (d / 1.0).min(1.0) } else { 0.0 };
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (angle.cos(), angle.sin(), g)
}

fn fetch(clip: &Clip, pos: f64, looped: bool) -> (f32, f32) {
    let n = clip.frames();
    let i0 = (pos as usize).min(n - 1);
    let frac = (pos - i0 as f64) as f32;
    let i1 = if i0 + 1 < n {
        i0 + 1
    } else if looped {
        0
    } else {
        i0
    };
    if clip.channels == 2 {
        let (l0, r0, l1, r1) = (clip.data[i0 * 2], clip.data[i0 * 2 + 1], clip.data[i1 * 2], clip.data[i1 * 2 + 1]);
        (l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac)
    } else {
        let s = clip.data[i0] + (clip.data[i1] - clip.data[i0]) * frac;
        (s, s)
    }
}

pub struct Mixer {
    voices: Vec<Voice>,
    pub listener: Listener,
    pub master: f32,
    next_id: VoiceId,
    reverb: Reverb,
    room_target: RoomSettings,
    room_now: RoomSettings,
    bus: Vec<f32>,
}

impl Default for Mixer {
    fn default() -> Mixer {
        Mixer::new()
    }
}

impl Mixer {
    pub fn new() -> Mixer {
        Mixer { voices: Vec::new(), listener: Listener::default(), master: 1.0, next_id: 1, reverb: Reverb::new(), room_target: RoomSettings::DRY, room_now: RoomSettings::DRY, bus: Vec::new() }
    }

    pub fn active(&self) -> usize {
        self.voices.len()
    }

    pub fn is_playing(&self, id: VoiceId) -> bool {
        self.voices.iter().any(|v| v.id == id)
    }

    fn sanitize(p: Params) -> Params {
        Params {
            volume: finite(p.volume, 0.0).clamp(0.0, 8.0),
            pitch: finite(p.pitch, 1.0).clamp(0.1, 8.0),
            looped: p.looped,
            spatial: p.spatial.map(|s| Spatial { pos: [finite(s.pos[0], 0.0), finite(s.pos[1], 0.0), finite(s.pos[2], 0.0)], range: finite(s.range, 40.0).max(0.1), near: finite(s.near, 2.0).max(0.01) }),
        }
    }

    /// Start a voice. `None` when the clip is empty, or when every voice is a loop.
    pub fn play(&mut self, clip: Arc<Clip>, params: Params) -> Option<VoiceId> {
        self.play_with(clip, params, None)
    }

    /// Start a voice that already knows what is in the way, so that not one block of it is heard unfiltered (a short sound behind a wall
    /// must be muffled from its first sample).
    pub fn play_with(&mut self, clip: Arc<Clip>, params: Params, acoustics: Option<Acoustics>) -> Option<VoiceId> {
        if clip.frames() == 0 || clip.channels == 0 || clip.channels > 2 || clip.rate == 0 {
            return None;
        }
        if self.voices.len() >= MAX_VOICES {
            // the quietest voice that is not a loop makes room (oldest first among equals)
            let loud = |v: &Voice| v.gl.max(v.gr);
            let victim = self.voices.iter().enumerate().filter(|(_, v)| !v.params.looped).min_by(|(_, a), (_, b)| loud(a).partial_cmp(&loud(b)).unwrap_or(std::cmp::Ordering::Equal).then(a.id.cmp(&b.id))).map(|(i, _)| i);
            match victim {
                Some(i) => {
                    self.voices.remove(i);
                }
                None => return None,
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        self.voices.push(Voice { id, clip, pos: 0.0, params: Mixer::sanitize(params), gl: 0.0, gr: 0.0, started: false, ended: false, acoustics: None, lp: 0.0, hist: Vec::new(), hist_pos: 0, taps: Vec::new(), send_now: 0.0, tail: 0.0 });
        if let Some(a) = acoustics {
            self.set_acoustics(id, a);
        }
        Some(id)
    }

    pub fn stop(&mut self, id: VoiceId) {
        self.voices.retain(|v| v.id != id);
    }

    pub fn stop_all(&mut self) {
        self.voices.clear();
    }

    fn voice_mut(&mut self, id: VoiceId) -> Option<&mut Voice> {
        self.voices.iter_mut().find(|v| v.id == id)
    }

    /// Move a spatial voice (nothing happens for a voice that is not spatial or has ended).
    pub fn set_position(&mut self, id: VoiceId, pos: V3) {
        if let Some(v) = self.voice_mut(id) {
            if let Some(s) = &mut v.params.spatial {
                s.pos = [finite(pos[0], s.pos[0]), finite(pos[1], s.pos[1]), finite(pos[2], s.pos[2])];
            }
        }
    }

    /// Change the volume and the pitch of a playing voice.
    pub fn set_volume_pitch(&mut self, id: VoiceId, volume: f32, pitch: f32) {
        if let Some(v) = self.voice_mut(id) {
            v.params.volume = finite(volume, v.params.volume).clamp(0.0, 8.0);
            v.params.pitch = finite(pitch, v.params.pitch).clamp(0.1, 8.0);
        }
    }

    pub fn set_listener(&mut self, l: Listener) {
        let ok = |v: V3| v.iter().all(|x| x.is_finite());
        if ok(l.pos) && ok(l.right) {
            self.listener = l;
        }
    }

    /// Give a voice the result of a trace; the direct low-pass, the echoes and the reverb send glide to their new values.
    pub fn set_acoustics(&mut self, id: VoiceId, a: Acoustics) {
        let right = self.listener.right;
        if let Some(v) = self.voice_mut(id) {
            if v.hist.is_empty() {
                v.hist = vec![0.0; HISTORY];
            }
            let mut taps: Vec<Tap> = a.taps.iter().copied().filter(|t| t.delay.is_finite() && t.gain.is_finite() && t.dir.iter().all(|x| x.is_finite())).collect();
            taps.truncate(MAX_TAPS);
            for (k, t) in taps.iter().enumerate() {
                let delay = t.delay.clamp(0.001, MAX_ECHO);
                let pan = (t.dir[0] * right[0] + t.dir[1] * right[1] + t.dir[2] * right[2]).clamp(-1.0, 1.0);
                let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                if let Some(s) = v.taps.get_mut(k) {
                    s.delay = delay;
                    s.gain = t.gain.max(0.0);
                    s.pan_l = angle.cos();
                    s.pan_r = angle.sin();
                } else {
                    v.taps.push(TapState { delay_now: delay, gain_now: 0.0, delay, gain: t.gain.max(0.0), pan_l: angle.cos(), pan_r: angle.sin() });
                }
            }
            for s in v.taps.iter_mut().skip(taps.len()) {
                s.gain = 0.0; // fades out, then is dropped at the end of the block
            }
            if !v.ended {
                // a voice that has already ended keeps the tail it had: asking again must not keep it alive for ever
                v.tail = taps.iter().map(|t| t.delay).fold(0.0, f32::max).min(MAX_ECHO) + 0.05;
            }
            v.acoustics = Some(Acoustics { direct_gain: finite(a.direct_gain, 1.0).clamp(0.0, 1.0), direct_cutoff: finite(a.direct_cutoff, 20_000.0).clamp(50.0, 40_000.0), send: finite(a.send, 0.0).clamp(0.0, 4.0), taps: Vec::new() });
        }
    }

    pub fn set_room(&mut self, r: RoomSettings) {
        self.room_target = RoomSettings { rt60: finite(r.rt60, 0.3).clamp(0.05, 10.0), damping: finite(r.damping, 0.3).clamp(0.0, 0.95), wet: finite(r.wet, 0.0).clamp(0.0, 4.0), predelay: finite(r.predelay, 0.0).clamp(0.0, 0.1) };
    }

    pub fn room(&self) -> RoomSettings {
        self.room_target
    }

    /// Mix the next `out.len() / 2` frames of interleaved stereo into `out` (which is overwritten).
    pub fn render(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        out.iter_mut().for_each(|x| *x = 0.0);
        if frames == 0 {
            return;
        }
        self.bus.clear();
        self.bus.resize(frames, 0.0);
        let listener = self.listener;
        let inv_frames = 1.0 / frames as f32;
        let master = finite(self.master, 1.0).clamp(0.0, 4.0);
        for v in self.voices.iter_mut() {
            let p = v.params;
            let acoustic = v.acoustics.is_some();
            let (direct, cutoff, send_t) = match &v.acoustics {
                Some(a) => (a.direct_gain, a.direct_cutoff, a.send),
                None => (1.0, 20_000.0, 0.0),
            };
            // target gains for this block
            let (tl, tr) = match &p.spatial {
                Some(s) => {
                    let (l, r, g) = spatial_gains(&listener, s);
                    (p.volume * g * l * direct, p.volume * g * r * direct)
                }
                None => (p.volume * direct, p.volume * direct),
            };
            // the reverberant field does not fall off with distance as the direct sound does, only with the end of the range
            let (near, window) = match &p.spatial {
                Some(s) => {
                    let to = [s.pos[0] - listener.pos[0], s.pos[1] - listener.pos[1], s.pos[2] - listener.pos[2]];
                    let d = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
                    let fade_start = s.range * 0.75;
                    let w = if d >= s.range { 0.0 } else if d > fade_start { ((s.range - d) / (s.range - fade_start)).powi(2) } else { 1.0 };
                    (s.near, w)
                }
                None => (1.0, 1.0),
            };
            let send_target = send_t * p.volume * near * window;
            if !v.started {
                v.gl = tl;
                v.gr = tr;
                v.send_now = send_target;
                v.started = true;
            }
            let (gl0, gr0, send0) = (v.gl, v.gr, v.send_now);
            let alpha = if cutoff >= 19_999.0 { 1.0 } else { 1.0 - (-std::f32::consts::TAU * cutoff / RATE as f32).exp() };
            let step = v.clip.rate as f64 * p.pitch as f64 / RATE as f64;
            let n = v.clip.frames() as f64;
            let spatial = p.spatial.is_some();
            let mut lp = v.lp;
            let mut hist_pos = v.hist_pos;
            let mut tail_left = v.tail;
            for i in 0..frames {
                if v.ended && !acoustic {
                    break;
                }
                let f = i as f32 * inv_frames;
                let (gl, gr) = (gl0 + (tl - gl0) * f, gr0 + (tr - gr0) * f);
                let (sl, sr) = if v.ended {
                    (0.0, 0.0)
                } else {
                    let s = fetch(&v.clip, v.pos, p.looped);
                    v.pos += step;
                    if v.pos >= n {
                        if p.looped {
                            v.pos %= n;
                        } else {
                            v.ended = true;
                        }
                    }
                    s
                };
                // a source in the world is a point: its two channels are mixed to one before it is placed
                let (dl, dr) = if spatial {
                    let m = 0.5 * (sl + sr);
                    (m, m)
                } else {
                    (sl, sr)
                };
                let (mut ol, mut or) = (dl, dr);
                if acoustic {
                    // direct sound through the low-pass of whatever is in the way
                    let m = 0.5 * (dl + dr);
                    lp += alpha * (m - lp);
                    let filtered = if alpha >= 1.0 { m } else { lp };
                    if spatial {
                        ol = filtered;
                        or = filtered;
                    } else {
                        ol = dl + (filtered - m);
                        or = dr + (filtered - m);
                    }
                    if !v.hist.is_empty() {
                        v.hist[hist_pos] = m * p.volume;
                    }
                }
                let mut l = ol * gl;
                let mut r = or * gr;
                if acoustic {
                    let len = v.hist.len();
                    let sf = f;
                    let send = send0 + (send_target - send0) * sf;
                    self.bus[i] += 0.5 * (dl + dr) * send;
                    for t in v.taps.iter_mut() {
                        // delay and gain glide: a jump in the traced path must not be a click
                        let dd = t.delay - t.delay_now;
                        t.delay_now += dd.clamp(-0.25 / RATE as f32, 0.25 / RATE as f32);
                        t.gain_now += (t.gain - t.gain_now) * 0.002;
                        let back = t.delay_now * RATE as f32;
                        let pos_f = hist_pos as f32 - back;
                        let base = pos_f.floor();
                        let fr = pos_f - base;
                        let a = (base as isize).rem_euclid(len as isize) as usize;
                        let b = (a + 1) % len;
                        let s = v.hist[a] + (v.hist[b] - v.hist[a]) * fr;
                        let g = s * t.gain_now * near;
                        l += g * t.pan_l;
                        r += g * t.pan_r;
                    }
                    hist_pos = (hist_pos + 1) % len.max(1);
                    if v.ended {
                        tail_left -= 1.0 / RATE as f32;
                    }
                }
                out[i * 2] += l;
                out[i * 2 + 1] += r;
            }
            v.gl = tl;
            v.gr = tr;
            v.send_now = send_target;
            v.lp = lp;
            v.hist_pos = hist_pos;
            v.tail = tail_left;
            v.taps.retain(|t| t.gain > 0.0 || t.gain_now > 1e-5);
        }
        self.voices.retain(|v| !(v.ended && (v.acoustics.is_none() || v.tail <= 0.0)));
        // the room: glide towards the latest settings, then the reverb bus
        let (a, b) = (self.room_now, self.room_target);
        self.room_now = RoomSettings { rt60: a.rt60 + (b.rt60 - a.rt60) * 0.2, damping: a.damping + (b.damping - a.damping) * 0.2, wet: a.wet + (b.wet - a.wet) * 0.2, predelay: b.predelay };
        self.reverb.set(self.room_now.rt60, self.room_now.damping, self.room_now.predelay);
        let wet = self.room_now.wet;
        if wet > 1e-4 || self.bus.iter().any(|x| *x != 0.0) {
            for i in 0..frames {
                let (l, r) = self.reverb.process(self.bus[i]);
                out[i * 2] += l * wet;
                out[i * 2 + 1] += r * wet;
            }
        }
        for x in out.iter_mut() {
            *x = (*x * master).clamp(-1.0, 1.0);
        }
    }

    /// Render `frames` frames of silence-or-sound and return them (convenient for tests and offline mixing).
    pub fn render_vec(&mut self, frames: usize) -> Vec<f32> {
        let mut v = vec![0.0; frames * 2];
        self.render(&mut v);
        v
    }
}

/// The delay in seconds for a path of `metres`.
pub fn travel_time(metres: f32) -> f32 {
    metres / SPEED_OF_SOUND
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, hz: f32, seconds: f32) -> Arc<Clip> {
        Arc::new(Clip::mono(rate, (0..(rate as f32 * seconds) as usize).map(|i| (i as f32 / rate as f32 * hz * std::f32::consts::TAU).sin() * 0.5).collect()))
    }

    fn dc(frames: usize) -> Arc<Clip> {
        Arc::new(Clip::mono(RATE, vec![1.0; frames]))
    }

    fn crossings(samples: &[f32], channel: usize) -> usize {
        let ch: Vec<f32> = samples.iter().skip(channel).step_by(2).copied().collect();
        ch.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count()
    }

    fn energy(samples: &[f32], channel: usize) -> f32 {
        samples.iter().skip(channel).step_by(2).map(|x| x * x).sum()
    }

    #[test]
    fn a_voice_plays_once_to_both_ears_and_is_gone_a_loop_is_not() {
        let mut m = Mixer::new();
        m.play(dc(1000), Params { volume: 0.5, ..Default::default() }).unwrap();
        let out = m.render_vec(600);
        assert!(out.iter().all(|x| (*x - 0.5).abs() < 1e-6));
        assert_eq!(m.active(), 1);
        let out = m.render_vec(600);
        assert!((out[2 * 399] - 0.5).abs() < 1e-6 && out[2 * 401] == 0.0, "stops at the end of the clip");
        assert_eq!(m.active(), 0);
        let id = m.play(dc(100), Params { looped: true, ..Default::default() }).unwrap();
        for _ in 0..20 {
            assert!(m.render_vec(441).iter().all(|x| (*x - 1.0).abs() < 1e-6));
        }
        assert!(m.is_playing(id));
        m.stop(id);
        assert_eq!(m.active(), 0);
        assert!(m.play(Arc::new(Clip::mono(RATE, Vec::new())), Params::default()).is_none(), "an empty clip is refused");
    }

    #[test]
    fn pitch_and_sample_rate_change_the_frequency_as_they_should() {
        let count = |rate: u32, pitch: f32| {
            let mut m = Mixer::new();
            m.play(tone(rate, 440.0, 1.0), Params { pitch, ..Default::default() }).unwrap();
            let out = m.render_vec(RATE as usize / 2);
            crossings(&out, 0)
        };
        let base = count(RATE, 1.0);
        assert!((base as i32 - 440).abs() <= 3, "{base} crossings in half a second = 440 Hz");
        let other_rate = count(22_050, 1.0);
        assert!((other_rate as i32 - base as i32).abs() <= 3, "a clip recorded at 22.05 kHz sounds the same: {other_rate}");
        let octave = count(RATE, 2.0);
        assert!((octave as i32 - 2 * base as i32).abs() <= 6, "{octave}");
    }

    #[test]
    fn a_source_on_the_right_is_heard_on_the_right_and_gets_quieter_with_distance() {
        let at = |pos: V3| {
            let mut m = Mixer::new();
            m.play(dc(RATE as usize), Params { spatial: Some(Spatial::at(pos)), ..Default::default() }).unwrap();
            let o = m.render_vec(200);
            (o[2 * 150], o[2 * 150 + 1])
        };
        let (l, r) = at([5.0, 0.0, 0.0]);
        assert!(r > l * 20.0, "{l} {r}");
        let (l, r) = at([-5.0, 0.0, 0.0]);
        assert!(l > r * 20.0);
        let (l, r) = at([0.0, 0.0, -5.0]);
        assert!((l - r).abs() < 1e-4, "straight ahead is centred: {l} {r}");
        // the power of the two ears together, by distance: inverse distance beyond `near`, nothing at the range
        let power = |d: f32| {
            let (l, r) = at([0.0, 0.0, -d]);
            (l * l + r * r).sqrt()
        };
        let (p2, p4, p8) = (power(2.0), power(4.0), power(8.0));
        assert!((p2 / p4 - 2.0).abs() < 0.01 && (p4 / p8 - 2.0).abs() < 0.01, "{p2} {p4} {p8}");
        assert!(power(1.0) <= p2 * 1.001 && (power(0.5) - power(1.0)).abs() < 1e-3, "full loudness inside `near`");
        assert!(power(35.0) < power(25.0) && power(40.0) == 0.0 && power(100.0) == 0.0);
        // the listener turned: right is now the old front
        let mut m = Mixer::new();
        m.set_listener(Listener { pos: [0.0; 3], right: [0.0, 0.0, -1.0] });
        m.play(dc(RATE as usize), Params { spatial: Some(Spatial::at([0.0, 0.0, -5.0])), ..Default::default() }).unwrap();
        let o = m.render_vec(200);
        assert!(o[2 * 150 + 1] > o[2 * 150] * 20.0);
    }

    #[test]
    fn gains_glide_instead_of_clicking_and_hostile_numbers_are_harmless() {
        let mut m = Mixer::new();
        let id = m.play(dc(RATE as usize * 4), Params { spatial: Some(Spatial::at([-5.0, 0.0, 0.0])), ..Default::default() }).unwrap();
        m.render_vec(256);
        m.set_position(id, [5.0, 0.0, 0.0]); // jumps from the far left to the far right
        let o = m.render_vec(441);
        let biggest = o.chunks(2).collect::<Vec<_>>().windows(2).map(|w| (w[1][1] - w[0][1]).abs().max((w[1][0] - w[0][0]).abs())).fold(0.0f32, f32::max);
        assert!(biggest < 0.01, "the step between two samples is {biggest}");
        // NaN everywhere
        let mut h = Mixer::new();
        h.set_listener(Listener { pos: [f32::NAN; 3], right: [f32::INFINITY; 3] });
        let id = h.play(dc(500), Params { volume: f32::NAN, pitch: f32::NAN, looped: false, spatial: Some(Spatial { pos: [f32::NAN; 3], range: f32::NAN, near: -1.0 }) }).unwrap();
        h.set_acoustics(id, Acoustics { direct_gain: f32::NAN, direct_cutoff: f32::NAN, send: f32::NAN, taps: vec![Tap { delay: f32::NAN, gain: 1.0, dir: [0.0; 3] }, Tap { delay: 9.0, gain: f32::INFINITY, dir: [1.0, 0.0, 0.0] }] });
        h.set_room(RoomSettings { rt60: f32::NAN, damping: f32::NAN, wet: f32::NAN, predelay: f32::NAN });
        h.master = f32::NAN;
        assert!(h.render_vec(600).iter().all(|x| x.is_finite() && x.abs() <= 1.0));
        // a loud mix is clipped, not wrapped
        let mut c = Mixer::new();
        for _ in 0..5 {
            c.play(dc(100), Params { volume: 3.0, ..Default::default() });
        }
        assert!(c.render_vec(50).iter().all(|x| *x == 1.0));
    }

    #[test]
    fn the_quietest_voice_makes_room_for_a_new_one_and_loops_are_never_cut() {
        let mut m = Mixer::new();
        let quiet = m.play(dc(RATE as usize), Params { volume: 0.01, ..Default::default() }).unwrap();
        for _ in 1..MAX_VOICES {
            m.play(dc(RATE as usize), Params::default()).unwrap();
        }
        m.render_vec(100);
        assert_eq!(m.active(), MAX_VOICES);
        m.play(dc(RATE as usize), Params::default()).unwrap();
        assert_eq!(m.active(), MAX_VOICES);
        assert!(!m.is_playing(quiet));
        let mut l = Mixer::new();
        for _ in 0..MAX_VOICES {
            l.play(dc(100), Params { looped: true, ..Default::default() }).unwrap();
        }
        assert!(l.play(dc(100), Params::default()).is_none());
    }

    #[test]
    fn what_is_in_the_way_muffles_the_direct_sound() {
        let noise: Vec<f32> = (0..RATE as usize).map(|i| (crate::trace::fibonacci_direction(i, 1_000_003)[0] * 997.0).fract() * 2.0 - 1.0).collect();
        let play = |a: Option<Acoustics>| {
            let mut m = Mixer::new();
            let id = m.play(Arc::new(Clip::mono(RATE, noise.clone())), Params { spatial: Some(Spatial::at([0.0, 0.0, -1.0])), ..Default::default() }).unwrap();
            if let Some(a) = a {
                m.set_acoustics(id, a);
            }
            m.render_vec(4410);
            m.render_vec(8820)
        };
        let rough = |o: &[f32]| o.chunks(2).collect::<Vec<_>>().windows(2).map(|w| (w[1][0] - w[0][0]).abs()).sum::<f32>();
        let open = play(None);
        let behind_wall = play(Some(Acoustics { direct_gain: 0.5, direct_cutoff: 400.0, send: 0.0, taps: Vec::new() }));
        assert!(rough(&behind_wall) < rough(&open) * 0.1, "{} vs {}", rough(&behind_wall), rough(&open));
        assert!(energy(&behind_wall, 0) < energy(&open, 0) * 0.3);
    }

    #[test]
    fn an_echo_arrives_later_from_its_own_side_and_the_reverb_send_rings() {
        let mut click = vec![0.0f32; 2000];
        click[0] = 1.0;
        let clip = Arc::new(Clip::mono(RATE, click));
        // a click straight ahead with one echo from the right 50 ms later at half loudness
        let mut m = Mixer::new();
        let id = m.play(clip.clone(), Params { spatial: Some(Spatial { pos: [0.0, 0.0, -1.0], range: 40.0, near: 1.0 }), ..Default::default() }).unwrap();
        m.set_acoustics(id, Acoustics { direct_gain: 1.0, direct_cutoff: 20_000.0, send: 0.0, taps: vec![Tap { delay: 0.05, gain: 0.5, dir: [1.0, 0.0, 0.0] }] });
        let mut o = Vec::new();
        for _ in 0..20 {
            o.extend(m.render_vec(441));
        }
        let at = |i: usize, ch: usize| o[i * 2 + ch].abs();
        let echo_at = (0.05 * RATE as f32) as usize;
        let window_peak = |ch: usize| (echo_at - 20..echo_at + 20).map(|i| at(i, ch)).fold(0.0f32, f32::max);
        assert!(window_peak(1) > 0.3, "{}", window_peak(1));
        assert!(window_peak(0) < window_peak(1) * 0.1, "the echo is on the right: {} {}", window_peak(0), window_peak(1));
        assert!(at(0, 0) > 0.3 && at(0, 1) > 0.3, "the direct click comes first, centred");
        let quiet_before = (400..echo_at - 100).map(|i| at(i, 1)).fold(0.0f32, f32::max);
        assert!(quiet_before < 0.02, "{quiet_before}");
        // the send: a click into a one second room keeps ringing a second later at about -60 dB and is loud at 100 ms
        let mut r = Mixer::new();
        r.set_room(RoomSettings { rt60: 1.0, damping: 0.0, wet: 1.0, predelay: 0.0 });
        for _ in 0..30 {
            r.render_vec(441); // let the room settings glide in
        }
        let id = r.play(clip, Params { spatial: Some(Spatial { pos: [0.0, 0.0, -1.0], range: 40.0, near: 1.0 }), ..Default::default() }).unwrap();
        r.set_acoustics(id, Acoustics { direct_gain: 1.0, direct_cutoff: 20_000.0, send: 1.0, taps: Vec::new() });
        let mut o = Vec::new();
        for _ in 0..200 {
            o.extend(r.render_vec(441));
        }
        let band = |a: f32, b: f32| energy(&o[(a * RATE as f32) as usize * 2..(b * RATE as f32) as usize * 2], 0);
        assert!(band(0.1, 0.2) > 1e-4, "{}", band(0.1, 0.2));
        assert!(band(0.1, 0.2) > band(0.5, 0.6) * 5.0 && band(0.5, 0.6) > band(1.5, 1.6) * 5.0, "it decays: {} {} {}", band(0.1, 0.2), band(0.5, 0.6), band(1.5, 1.6));
    }
}
