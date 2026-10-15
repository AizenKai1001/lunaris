//! Music: a `MusicPlayer` entity plays its playlist one track after another, the next one fading in while the last fades out (an equal-power crossfade, so the
//! loudness holds), and can shuffle, repeat, pause and skip. Like everything in the director it only *reads* the simulation (`playing`, `skip`, `volume` are
//! set by graph systems; nothing is written back), so music never changes a game's state.
use super::*;
use lunaris_sim::components::MusicPlayer;

/// One voice of the player: the track that is meant to be heard, or one that is fading out.
struct Track {
    id: VoiceId,
    label: String,
    /// The track's own volume from the playlist.
    volume: f32,
    started: u64,
    length: f32,
    fade_in: f32,
    /// When it began to fade out (and over how long); only for tracks that are leaving.
    fade_started: u64,
    fade_out: f32,
}

pub(super) struct MusicState {
    text: String,
    list: Vec<(String, f32)>,
    order: Vec<usize>,
    /// How many of `order` have been started in this pass.
    pos: usize,
    current: Option<Track>,
    fading: Vec<Track>,
    seen_skip: u32,
    rng: u64,
    /// A list that does not repeat has started its last track.
    ended: bool,
    /// Tracks in a row that could not be started (a missing file): after a full list of them the player gives up until the playlist changes.
    failures: usize,
}

impl MusicState {
    fn new(uid: u64) -> MusicState {
        MusicState { text: String::new(), list: Vec::new(), order: Vec::new(), pos: 0, current: None, fading: Vec::new(), seen_skip: 0, rng: uid.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1, ended: false, failures: 0 }
    }

    fn next_random(&mut self) -> u64 {
        // xorshift64: the shuffle only has to be fair, and the same for the same entity
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn reshuffle(&mut self, shuffle: bool) {
        self.order = (0..self.list.len()).collect();
        if shuffle {
            for i in (1..self.order.len()).rev() {
                let j = (self.next_random() % (i as u64 + 1)) as usize;
                self.order.swap(i, j);
            }
            // never the same track twice in a row across the seam of two passes
            if let (Some(last), true) = (self.current.as_ref().map(|t| t.label.clone()), self.order.len() > 1) {
                if self.list[self.order[0]].0 == last {
                    self.order.swap(0, 1);
                }
            }
        }
        self.pos = 0;
    }
}

/// One entry per line: a file of the workspace or `voice:NAME`, then optionally `volume=0.8`. `#` starts a comment.
fn parse_playlist(text: &str) -> Vec<(String, f32)> {
    let mut out = Vec::new();
    for line in text.split(['\n', ';']) {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        let label = words.next().unwrap_or("").to_string();
        let volume = words.find_map(|w| w.strip_prefix("volume=").and_then(|v| v.parse::<f32>().ok())).filter(|v| v.is_finite()).unwrap_or(1.0).clamp(0.0, 4.0);
        out.push((label, volume));
    }
    out.truncate(256);
    out
}

fn seconds_since(now: u64, from: u64, dt: f32) -> f32 {
    now.saturating_sub(from) as f32 * dt
}

impl Director {
    /// Keep the voices of every `MusicPlayer` right: start the next track, fade, stop.
    pub(super) fn sync_music(&mut self, engine: &mut Engine) {
        let mut found: Vec<(u64, MusicPlayer)> = Vec::new();
        let _ = engine.sim.world.for_each_uid::<(&MusicPlayer,), _>(|_, uid, (m,)| found.push((uid, *m)));
        found.sort_by_key(|f| f.0);
        let (now, dt, root) = (engine.sim.tick, engine.sim.dt.max(1e-4), engine.root.clone());
        // players that are gone: their music stops
        let alive: Vec<u64> = found.iter().map(|f| f.0).collect();
        let gone: Vec<u64> = self.music.keys().copied().filter(|u| !alive.contains(u)).collect();
        for u in gone {
            if let Some(st) = self.music.remove(&u) {
                for t in st.current.into_iter().chain(st.fading) {
                    self.lock().stop(t.id);
                    self.live.retain(|l| l.id != t.id);
                }
            }
        }
        for (uid, m) in found {
            let text = engine.sim.world.strings().get(m.playlist.0).to_string();
            let mut st = self.music.remove(&uid).unwrap_or_else(|| MusicState::new(uid));
            let finite = |v: f32, d: f32| if v.is_finite() { v } else { d };
            let (volume, cross) = (finite(m.volume, 1.0).clamp(0.0, 4.0), finite(m.crossfade, 2.0).clamp(0.0, 60.0));
            if text != st.text {
                st.list = parse_playlist(&text);
                st.text = text;
                st.reshuffle(m.shuffle);
                st.ended = false;
                st.failures = 0;
            }
            // voices that ended by themselves
            let playing = |s: &Director, id: VoiceId| s.mixer.lock().unwrap_or_else(|p| p.into_inner()).is_playing(id);
            if st.current.as_ref().is_some_and(|t| !playing(self, t.id)) {
                st.current = None;
            }
            st.fading.retain(|t| playing(self, t.id));
            // paused: the current track fades out, nothing new starts
            if !m.playing {
                if let Some(mut t) = st.current.take() {
                    t.fade_started = now;
                    t.fade_out = cross.max(dt);
                    st.fading.push(t);
                }
            }
            let skip = m.skip != st.seen_skip;
            st.seen_skip = m.skip;
            let mut start_next = false;
            if m.playing && !st.list.is_empty() && st.failures <= st.list.len() {
                match &st.current {
                    None => start_next = !st.ended,
                    Some(c) => {
                        let cf = cross.min(c.length * 0.5);
                        let more = st.pos < st.order.len() || m.repeat;
                        start_next = more && (skip || seconds_since(now, c.started, dt) >= c.length - cf);
                    }
                }
            }
            if start_next {
                if st.pos >= st.order.len() {
                    if m.repeat {
                        st.reshuffle(m.shuffle);
                    } else {
                        st.ended = true;
                    }
                }
                if let (false, Some(&index)) = (st.ended, st.order.get(st.pos)) {
                    st.pos += 1;
                    let (label, track_volume) = st.list[index].clone();
                    let clip = match label.strip_prefix("voice:") {
                        Some(v) => Some(self.voice_clip(v)),
                        None => self.file_clip(&root, &label),
                    };
                    match clip {
                        None => st.failures += 1,
                        Some(clip) => {
                            st.failures = 0;
                            let (bus, gain) = self.bus_gain(&engine.sim.world, &label);
                            let fade_in = if cross > 0.0 { cross } else { 0.0 };
                            let start_volume = if fade_in > 0.0 { 0.0 } else { volume * track_volume * gain };
                            let started = self.lock().play_with(clip.clone(), Params { volume: start_volume, pitch: 1.0, looped: false, spatial: None }, None);
                            if let Some(id) = started {
                                if let Some(mut old) = st.current.take() {
                                    old.fade_started = now;
                                    old.fade_out = cross.max(dt);
                                    st.fading.push(old);
                                }
                                *self.stats.started.entry(format!("music:{label}")).or_insert(0) += 1;
                                self.live.push(Live { id, pos: [0.0; 3], spatial: false, bus });
                                st.current = Some(Track { id, label, volume: track_volume, started: now, length: clip.seconds().max(0.05), fade_in, fade_started: 0, fade_out: 0.0 });
                            }
                        }
                    }
                }
            }
            // volumes: equal power, so two different tracks keep the loudness through a crossfade
            let quarter = std::f32::consts::FRAC_PI_2;
            if let Some(t) = &st.current {
                let f = if t.fade_in > 0.0 { (seconds_since(now, t.started, dt) / t.fade_in).min(1.0) } else { 1.0 };
                let (_, gain) = self.bus_gain(&engine.sim.world, &t.label);
                self.lock().set_volume_pitch(t.id, volume * t.volume * gain * (f * quarter).sin(), 1.0);
            }
            let mut done: Vec<VoiceId> = Vec::new();
            for t in &st.fading {
                let f = seconds_since(now, t.fade_started, dt) / t.fade_out;
                if f >= 1.0 {
                    done.push(t.id);
                    continue;
                }
                let fin = if t.fade_in > 0.0 { (seconds_since(now, t.started, dt) / t.fade_in).min(1.0) } else { 1.0 };
                let (_, gain) = self.bus_gain(&engine.sim.world, &t.label);
                self.lock().set_volume_pitch(t.id, volume * t.volume * gain * (f * quarter).cos() * (fin * quarter).sin(), 1.0);
            }
            for id in done {
                self.lock().stop(id);
                self.live.retain(|l| l.id != id);
                st.fading.retain(|t| t.id != id);
            }
            self.music.insert(uid, st);
        }
    }
}
