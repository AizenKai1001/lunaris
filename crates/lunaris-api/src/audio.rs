//! The sound director: turns what the simulation says into sound.
//!
//! * Events (`{"emit": "door"}`) play the `SoundFile` of that name (a WAV file), or the built-in synthesised voice (a `Sound` entity, or
//!   the default for common names), at the place where the emitting entity was.
//! * `AudioSource` entities play by themselves (a loop on a campfire, music), following their entity.
//! * The listener is the active camera.
//! * With acoustics on, every positioned sound is traced through the colliders of the scene (`lunaris_audio::trace`): the share that
//!   gets through walls, a low-pass for what does, the first echoes with their delays and directions, and the room's reverberation
//!   (measured around the listener) are handed to the mixer.
//!
//! The same director drives the native player (live, into the sound card) and the `record_audio` tool (offline, into a WAV file whose
//! levels an agent can read), so what the player hears is what the tool measures. Nothing here touches the simulation.
mod music;

use crate::engine::{Args, Engine};
use lunaris_audio::mixer::{Acoustics, Listener, Mixer, Params, RoomSettings, Spatial, Tap, VoiceId};
use lunaris_audio::synth::{default_voice, synth, Voice};
use lunaris_audio::trace::{analyse_room, trace_source, Occluder, Room, Scene, Shape, SourceTrace, V3, SPEED_OF_SOUND};
use lunaris_audio::codec::decode_any as decode;
use lunaris_audio::wav::Clip;
use lunaris_audio::RATE;
use lunaris_ecs::{EcsError, Entity, Result, World};
use lunaris_json::Json;
use lunaris_sim::components::{AcousticMaterial, AudioSource, Camera, Collider, Sound, SoundFile, Transform};
use lunaris_sim::Event;
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// A sound file bigger than this is refused (the decoder expands it to 32-bit floats).
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// At most this many new voices from events per update (a burst of identical events makes one).
const MAX_EVENT_VOICES: usize = 8;
const MAX_WARNINGS: usize = 24;

/// How the director models the room. The defaults suit most games; they are knobs for taste, not for correctness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AcousticsConfig {
    /// Trace sound through the scene at all (off: positions and panning only).
    pub enabled: bool,
    /// Rays shot around the listener to measure the room (how long it rings, how open it is).
    pub room_rays: usize,
    /// Rays shot for the echoes of one source.
    pub source_rays: usize,
    /// Most bounces of a room ray.
    pub bounces: usize,
    /// Loudness of the traced echoes (1 = as computed).
    pub reflection_gain: f32,
    /// Loudness of the reverberation (1 = as computed from the room's size and absorption).
    pub reverb_gain: f32,
    /// Trace again every this many updates.
    pub interval: u64,
}

impl Default for AcousticsConfig {
    fn default() -> AcousticsConfig {
        AcousticsConfig { enabled: true, room_rays: 192, source_rays: 96, bounces: 256, reflection_gain: 0.7, reverb_gain: 0.5, interval: 6 }
    }
}

/// What the director did, for reports.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    /// Voices started, by event name (or `source:<uid>`).
    pub started: BTreeMap<String, u64>,
    pub traces: u64,
    pub room_analyses: u64,
}

struct FileDef {
    file: String,
    volume: f32,
    pitch: f32,
    spatial: bool,
    range: f32,
    near: f32,
}

struct Live {
    id: VoiceId,
    pos: V3,
    spatial: bool,
    /// The mixer bus (see `buses`) the voice was routed to.
    bus: Option<usize>,
}

struct SourceState {
    voice: Option<VoiceId>,
    finished: bool,
}

/// What the tracing thread is asked: the scene as it was, where the ears are, whether to measure the room, and which voices to trace.
struct Job {
    scene: Arc<Scene>,
    listener: V3,
    room: bool,
    sources: Vec<(VoiceId, V3)>,
    cfg: AcousticsConfig,
}

/// What it answers.
struct Done {
    room: Option<Room>,
    traces: Vec<(VoiceId, SourceTrace)>,
}

fn run_job(job: &Job) -> Done {
    let room = job.room.then(|| analyse_room(&job.scene, job.listener, job.cfg.room_rays, job.cfg.bounces));
    let traces = job.sources.iter().map(|(id, pos)| (*id, trace_source(&job.scene, job.listener, *pos, job.cfg.source_rays, 0.3))).collect();
    Done { room, traces }
}

/// A thread that traces in the background, one job at a time, so that the frame that asked never waits for the rays.
struct Worker {
    jobs: Sender<Job>,
    done: Receiver<Done>,
    busy: bool,
}

pub struct Director {
    pub mixer: Arc<Mutex<Mixer>>,
    pub config: AcousticsConfig,
    pub stats: Stats,
    /// Problems that did not stop anything (a missing sound file, a file that is not a WAV), each once.
    pub warnings: Vec<String>,
    voices: BTreeMap<String, Voice>,
    files: BTreeMap<String, FileDef>,
    clips: BTreeMap<String, Option<Arc<Clip>>>,
    sources: BTreeMap<u64, SourceState>,
    /// What each `MusicPlayer` is playing.
    music: BTreeMap<u64, music::MusicState>,
    live: Vec<Live>,
    /// The mixer buses of the world's `AudioMixer`, and how far each is turned down by ducking right now.
    buses: crate::buses::BusSet,
    bus_duck: BTreeMap<usize, f32>,
    duck_level: BTreeMap<usize, f32>,
    scene: Option<Arc<Scene>>,
    scene_key: u64,
    frame: u64,
    room_frame: Option<u64>,
    room_at: V3,
    /// The last room measured.
    pub room: Room,
    listener: Listener,
    forward: V3,
    dirty: bool,
    worker: Option<Worker>,
}

fn rotate(q: [f32; 4], v: V3) -> V3 {
    // v + 2 w (u x v) + 2 u x (u x v) with u = (x, y, z)
    let u = [q[0], q[1], q[2]];
    let cross = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let t = cross(u, v);
    let t = [2.0 * t[0], 2.0 * t[1], 2.0 * t[2]];
    let ut = cross(u, t);
    [v[0] + q[3] * t[0] + ut[0], v[1] + q[3] * t[1] + ut[1], v[2] + q[3] * t[2] + ut[2]]
}

fn unit(v: V3) -> V3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-9 && l.is_finite() {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        [0.0; 3]
    }
}

fn dist(a: V3, b: V3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The reverb the mixer should use for a measured room.
pub fn room_settings(room: &Room, cfg: &AcousticsConfig) -> RoomSettings {
    if room.rt60 <= 0.0 || room.mean_free_path <= 0.0 || !cfg.enabled {
        return RoomSettings::DRY;
    }
    // the distance at which a source's direct sound and the room's reverberation are equally loud (Sabine), with the volume of the room
    // guessed from the mean free path of a cube (4 V / S = 0.67 a)
    let volume = 3.4 * room.mean_free_path.powi(3);
    let critical = 0.057 * (volume / room.rt60.max(0.05)).sqrt();
    let wet = (1.0 / critical.max(0.2)).min(1.5) * room.closedness.powi(4) * cfg.reverb_gain;
    RoomSettings { rt60: room.rt60, damping: (0.15 + room.mean_absorption * 1.5).clamp(0.15, 0.85), wet, predelay: (room.first_hit * 2.0 / SPEED_OF_SOUND).min(0.05) }
}

/// What the mixer should do to a source, from its trace: how much of the direct sound gets through and how muffled it is, and the echoes.
pub fn acoustics_for(trace: &SourceTrace, cfg: &AcousticsConfig) -> Acoustics {
    let t = trace.transmittance.clamp(0.0, 1.0);
    // what is in the way takes the high pitches first; the air takes some too, the further the sound goes
    let muffled = 20_000.0 * (0.04 + 0.96 * t.sqrt());
    let air = 20_000.0 / (1.0 + trace.distance / 80.0);
    Acoustics {
        direct_gain: t,
        direct_cutoff: muffled.min(air).max(200.0),
        send: 1.0,
        taps: trace.reflections.iter().map(|r| Tap { delay: r.delay, gain: r.gain * cfg.reflection_gain, dir: r.dir }).collect(),
    }
}

impl Director {
    pub fn new(mixer: Arc<Mutex<Mixer>>) -> Director {
        Director {
            mixer,
            config: AcousticsConfig::default(),
            stats: Stats::default(),
            warnings: Vec::new(),
            voices: BTreeMap::new(),
            files: BTreeMap::new(),
            clips: BTreeMap::new(),
            sources: BTreeMap::new(),
            music: BTreeMap::new(),
            live: Vec::new(),
            buses: Default::default(),
            bus_duck: BTreeMap::new(),
            duck_level: BTreeMap::new(),
            scene: None,
            scene_key: 0,
            frame: 0,
            room_frame: None,
            room_at: [0.0; 3],
            room: Room::NONE,
            listener: Listener::default(),
            forward: [0.0, 0.0, -1.0],
            dirty: false,
            worker: None,
        }
    }

    /// Trace in a background thread from now on (what the live player does); the first moments of a new sound are still traced at once,
    /// in the calling thread, so that a short sound behind a wall is never heard unmuffled. Offline recording keeps everything inline.
    pub fn spawn_tracer(&mut self) {
        if self.worker.is_some() {
            return;
        }
        let (jobs, job_rx) = channel::<Job>();
        let (done_tx, done) = channel::<Done>();
        let spawned = std::thread::Builder::new().name("lunaris-acoustics".to_string()).spawn(move || {
            while let Ok(job) = job_rx.recv() {
                if done_tx.send(run_job(&job)).is_err() {
                    break;
                }
            }
        });
        if spawned.is_ok() {
            self.worker = Some(Worker { jobs, done, busy: false });
        }
    }

    fn warn(&mut self, text: String) {
        if self.warnings.len() < MAX_WARNINGS && !self.warnings.contains(&text) {
            self.warnings.push(text);
        }
    }

    /// Read the `Sound` and `SoundFile` entities (what each event name sounds like). Call it again after they change.
    pub fn refresh_definitions(&mut self, world: &mut World) {
        let mut sounds: Vec<Sound> = Vec::new();
        let mut files: Vec<SoundFile> = Vec::new();
        let _ = world.for_each_uid::<(&Sound,), _>(|_, _, (s,)| sounds.push(*s));
        let _ = world.for_each_uid::<(&SoundFile,), _>(|_, _, (s,)| files.push(*s));
        self.voices.clear();
        for s in sounds {
            let name = world.strings().get(s.name.0).to_string();
            if !name.is_empty() {
                self.voices.insert(name, Voice { wave: s.wave, freq: s.freq, freq_end: s.freq_end, duration: s.duration, volume: s.volume });
            }
        }
        self.files.clear();
        for f in files {
            let name = world.strings().get(f.name.0).to_string();
            if !name.is_empty() {
                let file = world.strings().get(f.file.0).to_string();
                self.files.insert(name, FileDef { file, volume: f.volume, pitch: f.pitch, spatial: f.spatial, range: f.range, near: f.near });
            }
        }
        // a voice that was synthesised from an older definition must be made again
        self.clips.retain(|k, _| !k.starts_with("voice:"));
        // the mixer buses
        let mut mixers: Vec<(u64, u32)> = Vec::new();
        let _ = world.for_each_uid::<(&lunaris_sim::components::AudioMixer,), _>(|_, uid, (m,)| mixers.push((uid, m.config.0)));
        match mixers.into_iter().min_by_key(|m| m.0) {
            Some((_, sym)) => {
                let text = world.strings().get(sym).to_string();
                match crate::buses::BusSet::parse(&text) {
                    Ok(b) => self.buses = b,
                    Err(e) => {
                        self.buses = Default::default();
                        self.warn(format!("AudioMixer: {e}"));
                    }
                }
            }
            None => self.buses = Default::default(),
        }
    }

    fn file_clip(&mut self, root: &std::path::Path, rel: &str) -> Option<Arc<Clip>> {
        if let Some(c) = self.clips.get(rel) {
            return c.clone();
        }
        let loaded = (|| -> std::result::Result<Arc<Clip>, String> {
            let norm = lunaris_json::safe_relative(rel).map_err(|why| format!("`{rel}` leaves the workspace ({why})"))?;
            let path = root.join(norm);
            let meta = std::fs::metadata(&path).map_err(|e| format!("cannot read `{rel}`: {e}"))?;
            if meta.len() > MAX_FILE_BYTES {
                return Err(format!("`{rel}` is {} MB; the limit is {} MB", meta.len() / 1_000_000, MAX_FILE_BYTES / 1_000_000));
            }
            let bytes = std::fs::read(&path).map_err(|e| format!("cannot read `{rel}`: {e}"))?;
            decode(&bytes).map(Arc::new).map_err(|e| format!("`{rel}`: {e}"))
        })();
        match loaded {
            Ok(c) => {
                self.clips.insert(rel.to_string(), Some(c.clone()));
                Some(c)
            }
            Err(e) => {
                self.warn(e);
                self.clips.insert(rel.to_string(), None);
                None
            }
        }
    }

    fn voice_clip(&mut self, name: &str) -> Arc<Clip> {
        let key = format!("voice:{name}");
        if let Some(Some(c)) = self.clips.get(&key) {
            return c.clone();
        }
        let voice = self.voices.get(name).copied().unwrap_or_else(|| default_voice(name));
        let clip = Arc::new(Clip::mono(RATE, synth(&voice, RATE)));
        self.clips.insert(key, Some(clip.clone()));
        clip
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Mixer> {
        self.mixer.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The listener is the active camera (the lowest uid among the active ones), or the origin looking along -Z when there is none.
    fn find_listener(world: &mut World) -> (Listener, V3) {
        let mut best: Option<(u64, V3, [f32; 4])> = None;
        let _ = world.for_each_uid::<(&Camera, &Transform), _>(|_, uid, (c, t)| {
            if c.active && best.is_none_or(|b| uid < b.0) {
                best = Some((uid, t.pos, t.rot));
            }
        });
        match best {
            Some((_, pos, rot)) => (Listener { pos, right: unit(rotate(rot, [1.0, 0.0, 0.0])) }, unit(rotate(rot, [0.0, 0.0, -1.0]))),
            None => (Listener::default(), [0.0, 0.0, -1.0]),
        }
    }

    /// The colliders as sound sees them, and a number that changes when any of them does.
    pub fn gather_occluders(world: &mut World) -> (Vec<Occluder>, u64) {
        let mut cols: Vec<(Entity, Collider, Transform)> = Vec::new();
        let _ = world.for_each_uid::<(&Collider, &Transform), _>(|e, _, (c, t)| {
            if !c.trigger {
                cols.push((e, *c, *t));
            }
        });
        let mut out = Vec::with_capacity(cols.len());
        let mut key: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |x: f32| {
            key ^= x.to_bits() as u64;
            key = key.wrapping_mul(0x0000_0100_0000_01b3);
        };
        let fin = |x: f32| if x.is_finite() { x } else { 0.0 };
        for (e, c, t) in cols {
            let m = world.get::<AcousticMaterial>(e).copied();
            let (absorption, transmission) = m.map(|m| (m.absorption, m.transmission)).unwrap_or((0.12, 0.03));
            let (absorption, transmission) = (fin(absorption).clamp(0.0, 1.0), fin(transmission).clamp(0.0, 1.0));
            let sc = [fin(t.scale[0]).abs(), fin(t.scale[1]).abs(), fin(t.scale[2]).abs()];
            let center = [fin(t.pos[0]), fin(t.pos[1]), fin(t.pos[2])];
            let shape = if c.shape == 0 {
                Shape::Sphere { center, radius: 0.5 * fin(c.size[0]).abs() * sc[0].max(sc[1]).max(sc[2]) }
            } else {
                Shape::Box { center, half: [0.5 * fin(c.size[0]).abs() * sc[0], 0.5 * fin(c.size[1]).abs() * sc[1], 0.5 * fin(c.size[2]).abs() * sc[2]] }
            };
            match shape {
                Shape::Box { center, half } => {
                    (0..3).for_each(|i| {
                        mix(center[i]);
                        mix(half[i]);
                    });
                }
                Shape::Sphere { center, radius } => {
                    (0..3).for_each(|i| mix(center[i]));
                    mix(radius);
                }
            }
            mix(absorption);
            mix(transmission);
            out.push(Occluder { shape, absorption, transmission });
        }
        (out, key)
    }

    fn ensure_scene(&mut self, world: &mut World) -> Arc<Scene> {
        let (occ, key) = Director::gather_occluders(world);
        if self.scene.is_none() || key != self.scene_key {
            self.scene = Some(Arc::new(Scene::new(occ)));
            self.scene_key = key;
        }
        self.scene.clone().expect("just built")
    }

    /// The bus a sound goes to and the volume that bus (with its parents, the settings and ducking) gives it; (None, 1) without buses.
    fn bus_gain(&self, world: &World, label: &str) -> (Option<usize>, f32) {
        match self.buses.route(label) {
            Some(b) => (Some(b), self.buses.gain(b, &|n| world.resource(n), &self.bus_duck)),
            None => (None, 1.0),
        }
    }

    /// Turn the ducked buses down while the buses that duck them are heard, with a quick attack and a slow release.
    fn update_ducking(&mut self) {
        if self.buses.buses.is_empty() {
            self.bus_duck.clear();
            self.duck_level.clear();
            return;
        }
        let playing: Vec<Option<usize>> = {
            let m = self.lock();
            self.live.iter().filter(|l| m.is_playing(l.id)).map(|l| l.bus).collect()
        };
        let mut next: BTreeMap<usize, f32> = BTreeMap::new();
        for (i, b) in self.buses.buses.iter().enumerate() {
            if b.ducks.is_empty() {
                continue;
            }
            let heard = playing.iter().any(|p| *p == Some(i));
            let level = self.duck_level.entry(i).or_insert(0.0);
            let target = if heard { 1.0 } else { 0.0 };
            *level += (target - *level) * if target > *level { 0.3 } else { 0.04 };
            for &(victim, amount) in &b.ducks {
                let f = next.entry(victim).or_insert(0.0);
                *f = f.max(amount * *level);
            }
        }
        self.bus_duck = next;
    }

    fn params_for(def: &FileDef, pos: Option<V3>) -> Params {
        Params { volume: def.volume, pitch: def.pitch, looped: false, spatial: if def.spatial { pos.map(|p| Spatial { pos: p, range: def.range, near: def.near }) } else { None } }
    }

    fn start_event_voice(&mut self, world: &mut World, root: &std::path::Path, e: &Event) {
        let label = match self.files.get(&e.name) {
            Some(d) if !d.file.is_empty() => d.file.clone(),
            _ => format!("voice:{}", e.name),
        };
        // what the event sounds like: a recorded file, a synthesised voice made spatial by a SoundFile without a file, or the plain voice
        let (clip, params) = match self.files.get(&e.name) {
            Some(def) if !def.file.is_empty() => {
                let (file, params) = (def.file.clone(), Director::params_for(def, e.pos));
                match self.file_clip(root, &file) {
                    Some(c) => (c, params),
                    None => return,
                }
            }
            Some(def) => {
                let params = Director::params_for(def, e.pos);
                (self.voice_clip(&e.name), params)
            }
            None => (self.voice_clip(&e.name), Params::default()),
        };
        let (voice_bus, gain) = self.bus_gain(world, &label);
        let mut params = params;
        params.volume *= gain;
        let spatial = params.spatial;
        let first = spatial.and_then(|s| self.initial_acoustics(world, s.pos));
        let started = self.lock().play_with(clip, params, first);
        if let Some(id) = started {
            *self.stats.started.entry(e.name.clone()).or_insert(0) += 1;
            self.live.push(Live { id, pos: spatial.map(|s| s.pos).unwrap_or([0.0; 3]), spatial: spatial.is_some(), bus: voice_bus });
        }
    }

    fn sync_sources(&mut self, engine: &mut Engine) {
        let mut found: Vec<(u64, AudioSource, Option<Transform>)> = Vec::new();
        {
            let world = &mut engine.sim.world;
            let mut raw: Vec<(u64, Entity, AudioSource)> = Vec::new();
            let _ = world.for_each_uid::<(&AudioSource,), _>(|e, uid, (a,)| raw.push((uid, e, *a)));
            for (uid, e, a) in raw {
                found.push((uid, a, world.get::<Transform>(e).copied()));
            }
        }
        let root = engine.root.clone();
        // sources that are gone: their voices stop
        let alive: Vec<u64> = found.iter().map(|f| f.0).collect();
        let gone: Vec<u64> = self.sources.keys().copied().filter(|u| !alive.contains(u)).collect();
        for u in gone {
            if let Some(s) = self.sources.remove(&u) {
                if let Some(id) = s.voice {
                    self.lock().stop(id);
                    self.live.retain(|l| l.id != id);
                }
            }
        }
        for (uid, a, t) in found {
            let (file, voice) = (engine.sim.world.strings().get(a.file.0).to_string(), engine.sim.world.strings().get(a.voice.0).to_string());
            let state = self.sources.entry(uid).or_insert(SourceState { voice: None, finished: false });
            let playing_voice = state.voice.filter(|id| self.mixer.lock().unwrap_or_else(|p| p.into_inner()).is_playing(*id));
            if !a.playing {
                if let Some(id) = playing_voice {
                    self.lock().stop(id);
                    self.live.retain(|l| l.id != id);
                }
                let s = self.sources.get_mut(&uid).expect("just inserted");
                s.voice = None;
                s.finished = false;
                continue;
            }
            let pos = t.map(|t| t.pos);
            let label = if !file.is_empty() { file.clone() } else { format!("voice:{voice}") };
            let (voice_bus, gain) = self.bus_gain(&engine.sim.world, &label);
            match playing_voice {
                Some(id) => {
                    let mut m = self.lock();
                    m.set_volume_pitch(id, a.volume * gain, a.pitch);
                    if let Some(p) = pos {
                        m.set_position(id, p);
                    }
                    drop(m);
                    if let (Some(p), Some(l)) = (pos, self.live.iter_mut().find(|l| l.id == id)) {
                        l.pos = p;
                    }
                }
                None => {
                    let state = self.sources.get_mut(&uid).expect("just inserted");
                    if state.voice.is_some() {
                        // it was playing and has ended: a one-shot source plays once
                        state.finished = true;
                        state.voice = None;
                    }
                    if self.sources[&uid].finished && !a.looped {
                        continue;
                    }
                    let clip = if !file.is_empty() {
                        self.file_clip(&root, &file)
                    } else if !voice.is_empty() {
                        Some(self.voice_clip(&voice))
                    } else {
                        self.warn(format!("AudioSource {uid} has neither `file` nor `voice`"));
                        None
                    };
                    let Some(clip) = clip else { continue };
                    let spatial = if a.spatial { pos.map(|p| Spatial { pos: p, range: a.range, near: a.near }) } else { None };
                    let params = Params { volume: a.volume * gain, pitch: a.pitch, looped: a.looped, spatial };
                    let first = spatial.and_then(|s| self.initial_acoustics(&mut engine.sim.world, s.pos));
                    let started = self.lock().play_with(clip, params, first);
                    if let Some(id) = started {
                        *self.stats.started.entry(format!("source:{uid}")).or_insert(0) += 1;
                        self.sources.get_mut(&uid).expect("just inserted").voice = Some(id);
                        self.live.push(Live { id, pos: spatial.map(|s| s.pos).unwrap_or([0.0; 3]), spatial: spatial.is_some(), bus: voice_bus });
                    }
                }
            }
        }
    }

    /// Call once per frame (or per tick, offline) with the events the simulation made since the last call.
    pub fn update(&mut self, engine: &mut Engine, events: &[Event]) {
        self.frame += 1;
        // what each event name sounds like can change while the game runs (an agent adds a `SoundFile`): look again every two seconds
        if self.frame == 1 || self.frame % 120 == 0 {
            self.refresh_definitions(&mut engine.sim.world);
        }
        let root = engine.root.clone();
        let (listener, forward) = Director::find_listener(&mut engine.sim.world);
        self.listener = listener;
        self.forward = forward;
        self.lock().set_listener(listener);
        self.collect_results();
        self.update_ducking();
        self.sync_sources(engine);
        self.sync_music(engine);
        let mut started = 0;
        let mut seen: Vec<(&str, Option<V3>)> = Vec::new();
        for e in events {
            if started >= MAX_EVENT_VOICES {
                break;
            }
            // one voice for events of the same name from (nearly) the same place in one update
            let same = seen.iter().any(|(n, p)| *n == e.name && match (p, e.pos) {
                (Some(a), Some(b)) => dist(*a, b) < 1.0,
                (None, None) => true,
                _ => false,
            });
            if same {
                continue;
            }
            seen.push((&e.name, e.pos));
            self.start_event_voice(&mut engine.sim.world, &root, e);
            started += 1;
        }
        self.live.retain(|l| self.mixer.lock().unwrap_or_else(|p| p.into_inner()).is_playing(l.id));
        if self.config.enabled && (self.dirty || self.frame % self.config.interval.max(1) == 0) {
            self.trace_all(&mut engine.sim.world);
        }
    }

    /// What a sound that starts now should already know: the way from `pos` to the listener, and the room (measured on the spot the first time).
    fn initial_acoustics(&mut self, world: &mut World, pos: V3) -> Option<Acoustics> {
        if !self.config.enabled {
            return None;
        }
        let scene = self.ensure_scene(world);
        if self.room_frame.is_none() {
            self.apply_room(analyse_room(&scene, self.listener.pos, self.config.room_rays, self.config.bounces));
            self.room_frame = Some(self.frame);
            self.room_at = self.listener.pos;
        }
        let trace = trace_source(&scene, self.listener.pos, pos, self.config.source_rays, 0.3);
        self.stats.traces += 1;
        Some(acoustics_for(&trace, &self.config))
    }

    fn apply_room(&mut self, room: Room) {
        self.room = room;
        self.stats.room_analyses += 1;
        let settings = room_settings(&room, &self.config);
        self.lock().set_room(settings);
    }

    /// Take what the tracing thread has finished (if there is one).
    fn collect_results(&mut self) {
        let Some(w) = &mut self.worker else { return };
        let mut got: Vec<Done> = Vec::new();
        while let Ok(d) = w.done.try_recv() {
            w.busy = false;
            got.push(d);
        }
        for d in got {
            if let Some(room) = d.room {
                self.apply_room(room);
            }
            for (id, trace) in d.traces {
                let a = acoustics_for(&trace, &self.config);
                self.lock().set_acoustics(id, a);
                self.stats.traces += 1;
            }
        }
    }

    /// Measure the room if it is due and trace every positioned voice (inline, or by the tracing thread when there is one).
    fn trace_all(&mut self, world: &mut World) {
        if !self.live.iter().any(|l| l.spatial) {
            self.dirty = false;
            return;
        }
        if self.worker.as_ref().is_some_and(|w| w.busy) {
            return; // still working on the last job; try again next update (`dirty` stays as it was)
        }
        self.dirty = false;
        let scene = self.ensure_scene(world);
        let listener = self.listener.pos;
        let moved = dist(listener, self.room_at);
        let room = self.room_frame.is_none_or(|f| self.frame - f >= 30 || moved > 2.0);
        if room {
            self.room_frame = Some(self.frame);
            self.room_at = listener;
        }
        let sources: Vec<(VoiceId, V3)> = self.live.iter().filter(|l| l.spatial).take(24).map(|l| (l.id, l.pos)).collect();
        let job = Job { scene, listener, room, sources, cfg: self.config };
        match &mut self.worker {
            Some(w) => {
                if w.jobs.send(job).is_ok() {
                    w.busy = true;
                } else {
                    self.worker = None; // the thread is gone: trace inline from now on
                }
            }
            None => {
                let done = run_job(&job);
                if let Some(r) = done.room {
                    self.apply_room(r);
                }
                for (id, trace) in done.traces {
                    let a = acoustics_for(&trace, &self.config);
                    self.lock().set_acoustics(id, a);
                    self.stats.traces += 1;
                }
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------ tools

fn v3_json(v: V3) -> Json {
    Json::arr(v.iter().map(|x| Json::from(*x as f64)))
}

fn round(x: f32, places: i32) -> f64 {
    let m = 10f64.powi(places);
    (x as f64 * m).round() / m
}

fn db(x: f32) -> f64 {
    if x <= 1e-9 {
        -120.0
    } else {
        round(20.0 * x.log10(), 1)
    }
}

fn point(v: &Json, key: &str) -> Result<V3> {
    let bad = || EcsError::new("type_mismatch", format!("`{key}` must be an array of three numbers, e.g. [0, 1.7, 0]")).at(key);
    let a = v.as_array().ok_or_else(bad)?;
    if a.len() != 3 {
        return Err(bad());
    }
    let mut out = [0.0f32; 3];
    for (i, x) in a.iter().enumerate() {
        out[i] = x.as_f64().filter(|f| f.is_finite()).ok_or_else(bad)? as f32;
    }
    Ok(out)
}

impl Engine {
    /// `acoustics`: what the way from a source to the listener does to sound, and what the room around the listener does.
    pub(crate) fn acoustics_tool(&mut self, a: &Args) -> Result<Json> {
        let cfg = AcousticsConfig::default();
        let (cam, forward) = Director::find_listener(&mut self.sim.world);
        let (listener_pos, right, forward) = match a.0.get("listener") {
            Some(v) if !matches!(v, Json::Null) => (point(v, "listener")?, cam.right, forward),
            _ => (cam.pos, cam.right, forward),
        };
        let (occ, _) = Director::gather_occluders(&mut self.sim.world);
        let scene = Scene::new(occ);
        let rays = a.opt_u64("rays")?.unwrap_or(cfg.room_rays as u64).clamp(16, 4096) as usize;
        let room = analyse_room(&scene, listener_pos, rays, cfg.bounces);
        let settings = room_settings(&room, &cfg);
        let volume = 3.4 * room.mean_free_path.powi(3);
        let mut out = Json::obj([
            ("listener", v3_json(listener_pos)),
            ("occluders", Json::from(scene.len())),
            (
                "room",
                Json::obj([
                    ("rt60_seconds", Json::from(round(room.rt60, 2))),
                    ("closedness", Json::from(round(room.closedness, 2))),
                    ("mean_free_path_m", Json::from(round(room.mean_free_path, 2))),
                    ("mean_absorption", Json::from(round(room.mean_absorption, 3))),
                    ("first_hit_m", Json::from(round(room.first_hit, 2))),
                    ("volume_estimate_m3", Json::from(round(volume, 0))),
                    ("reverb_level_db", Json::from(db(settings.wet))),
                    ("verdict", Json::str(if room.closedness < 0.3 { "open air: no reverberation" } else if room.closedness < 0.75 { "partly open (a floor, a few walls): little reverberation" } else if room.rt60 < 0.25 { "dry: a damped room" } else if room.rt60 < 0.8 { "normal room" } else if room.rt60 < 2.0 { "lively: hard walls" } else { "hall or cave: long reverberation" })),
                ]),
            ),
        ]);
        let source = match a.0.get("source") {
            None | Some(Json::Null) => None,
            Some(v) if v.as_f64().is_some() => {
                let uid = v.as_u64().ok_or_else(|| EcsError::new("type_mismatch", "`source` must be an entity uid or [x, y, z]").at("source"))?;
                let e = self.sim.world.entity(uid).ok_or_else(|| EcsError::new("no_such_entity", format!("no entity {uid}")).at("source"))?;
                let t = self.sim.world.get::<Transform>(e).ok_or_else(|| EcsError::new("missing_component", format!("entity {uid} has no Transform")).at("source"))?;
                Some(t.pos)
            }
            Some(v) => Some(point(v, "source")?),
        };
        if let Some(src) = source {
            let rays = a.opt_u64("source_rays")?.unwrap_or(cfg.source_rays as u64).clamp(8, 4096) as usize;
            let trace = trace_source(&scene, listener_pos, src, rays, 0.3);
            let ac = acoustics_for(&trace, &cfg);
            let reflections: Vec<Json> = trace
                .reflections
                .iter()
                .map(|r| {
                    let (side, ahead, up) = (r.dir[0] * right[0] + r.dir[1] * right[1] + r.dir[2] * right[2], r.dir[0] * forward[0] + r.dir[1] * forward[1] + r.dir[2] * forward[2], r.dir[1]);
                    Json::obj([
                        ("delay_ms", Json::from(round(r.delay * 1000.0, 1))),
                        ("level_db", Json::from(db(r.gain / trace.distance.max(1.0)))),
                        ("azimuth_deg", Json::from(round(side.atan2(ahead).to_degrees(), 0))),
                        ("elevation_deg", Json::from(round(up.clamp(-1.0, 1.0).asin().to_degrees(), 0))),
                    ])
                })
                .collect();
            out.set(
                "source",
                Json::obj([
                    ("position", v3_json(src)),
                    ("distance_m", Json::from(round(trace.distance, 2))),
                    ("direct_transmittance", Json::from(round(trace.transmittance, 3))),
                    ("direct_level_db", Json::from(db(trace.transmittance / trace.distance.max(1.0)))),
                    ("clear_line_of_sight", Json::Bool(trace.visible >= 0.999)),
                    ("direct_lowpass_hz", Json::from(round(ac.direct_cutoff, 0))),
                    ("echoes", Json::Arr(reflections)),
                ]),
            );
        }
        Ok(out)
    }

    /// `record_audio`: run the simulation for some ticks and mix what the game would have sounded like into a WAV file, reporting levels.
    pub(crate) fn record_audio_tool(&mut self, a: &Args) -> Result<Json> {
        let ticks = a.opt_u64("ticks")?.unwrap_or(300);
        if ticks == 0 || ticks > 3600 {
            return Err(EcsError::new("out_of_range", "`ticks` must be 1 to 3600 (one minute at 60 Hz) per call").at("ticks").hint("call record_audio several times for a longer game"));
        }
        let path = a.opt_str("path")?.map(|p| p.to_string());
        let acoustics = !matches!(a.0.get("acoustics"), Some(Json::Bool(false)));
        let target = match &path {
            Some(p) => Some((self.resolve(p)?, p.clone())),
            None => None,
        };
        let mixer = Arc::new(Mutex::new(Mixer::new()));
        let mut director = Director::new(mixer.clone());
        director.config.enabled = acoustics;
        let per_tick = (RATE as u64 / 60) as usize; // 735 frames: the simulation runs at 60 ticks a second
        let mut all: Vec<f32> = Vec::with_capacity(ticks as usize * per_tick * 2);
        let mut timeline: Vec<Json> = Vec::new();
        let mut event_count = 0usize;
        let mut slice_start = 0usize;
        for k in 0..ticks {
            let before = self.sim.tick;
            self.sim.step();
            let events: Vec<Event> = self.sim.events.iter().filter(|e| e.tick == before).cloned().collect();
            event_count += events.len();
            director.update(self, &events);
            let block = mixer.lock().unwrap_or_else(|p| p.into_inner()).render_vec(per_tick);
            all.extend_from_slice(&block);
            // a level every quarter of a second
            if (k + 1) % 15 == 0 || k + 1 == ticks {
                let l = lunaris_audio::levels(&all[slice_start..]);
                timeline.push(Json::obj([
                    ("t", Json::from(round((k + 1) as f32 / 60.0, 2))),
                    ("peak_db", Json::from(db(l.peak))),
                    ("left_db", Json::from(db(l.rms_left))),
                    ("right_db", Json::from(db(l.rms_right))),
                ]));
                slice_start = all.len();
            }
        }
        let l = lunaris_audio::levels(&all);
        let mut out = Json::obj([
            ("tick", Json::from(self.sim.tick as i64)),
            ("ticks_run", Json::from(ticks as i64)),
            ("seconds", Json::from(round(ticks as f32 / 60.0, 2))),
            ("sample_rate", Json::from(RATE as i64)),
            ("events", Json::from(event_count)),
            ("peak_db", Json::from(db(l.peak))),
            ("left_rms_db", Json::from(db(l.rms_left))),
            ("right_rms_db", Json::from(db(l.rms_right))),
            ("clipped_samples", Json::from(l.clipped as i64)),
            ("silent", Json::Bool(l.peak < 1e-4)),
            ("voices_started", Json::Obj(director.stats.started.iter().map(|(k, v)| (k.clone(), Json::from(*v as i64))).collect())),
            ("traces", Json::from(director.stats.traces as i64)),
            ("room_rt60_seconds", Json::from(round(director.room.rt60, 2))),
            ("timeline", Json::Arr(timeline)),
        ]);
        if !director.warnings.is_empty() {
            out.set("warnings", Json::arr(director.warnings.iter().map(Json::str)));
        }
        if let Some((full, shown)) = target {
            if let Some(dir) = full.parent() {
                std::fs::create_dir_all(dir).map_err(|e| EcsError::new("io_error", format!("cannot create {}: {e}", dir.display())).at("path"))?;
            }
            let bytes = lunaris_audio::wav::encode(RATE, 2, &all);
            std::fs::write(&full, &bytes).map_err(|e| EcsError::new("io_error", format!("cannot write {shown}: {e}")).at("path"))?;
            out.set("path", Json::str(shown));
            out.set("bytes", Json::from(bytes.len()));
        }
        Ok(out)
    }
}
