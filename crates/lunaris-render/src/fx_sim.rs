//! The stepping of particle systems (docs/FX.md). A `ParticleSystem`'s particles are **stepped** in fixed steps of 1/60 s up to the component's clock, so they can collide, spawn each other and leave ribbons, which a closed form (`particles.rs`) cannot. It happens here, in
//! the picture, for the systems that are in view (the simulation never does it, and the state hash never sees it), and it is deterministic: every random number comes from a generator seeded by the system's `seed` and the emitter's number, every sine and exponential
//! from `detmath`, so the same effect, seed and clock give the same particles on every machine and whatever the number of threads (the systems are independent: they are stepped in parallel, each by one thread).
//!
//! A system that is seen for the first time, or again after more than half a second out of view, or whose clock went back, is stepped from the start of its history: from the effect's `prewarm` before time 0 (so a looping fire is full in the first picture) up to the clock, but at most the
//! last 30 seconds of it (a clock that is far on only shows what a long-running effect looks like: the same steady state).
use crate::math::*;
use lunaris_sim::components::Transform;
use lunaris_sim::detmath;
use lunaris_sim::fx::*;
use lunaris_sim::vehicle_ray::RayWorld;
use std::collections::BTreeMap;
use std::sync::Arc;

/// The step, the most steps a system is stepped in one go (30 s), and the gap in steps after which a system that was out of view starts again (half a second).
pub const DT: f32 = 1.0 / 60.0;
pub const MAX_CATCHUP: i64 = 1800;
const GAP: i64 = 30;
/// A particle hits a surface hard enough to cause an event above this speed (m/s): one that rolls along the floor does not make sparks at every step.
const COLLIDE_EVENT_SPEED: f32 = 1.0;
/// The most births the events of one step may ask for in all.
const MAX_BIRTHS: usize = 8192;

/// A small deterministic generator (SplitMix64).
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD6E8_FEB8_6659_FD93);
        r.next();
        r
    }

    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 32) as u32
    }

    /// In [0, 1).
    pub fn f(&mut self) -> f32 {
        (self.next() >> 8) as f32 / 16_777_216.0
    }

    pub fn range(&mut self, r: [f32; 2]) -> f32 {
        r[0] + (r[1] - r[0]) * self.f()
    }

    /// `n` rounded up or down at random so that its average is `n`.
    fn stochastic(&mut self, n: f32) -> u32 {
        let whole = n.floor();
        whole as u32 + u32::from(self.f() < n - whole)
    }

    /// A point of the unit ball (by throwing darts: nothing but additions and products, the same everywhere).
    fn in_ball(&mut self) -> V3 {
        loop {
            let p = [self.f() * 2.0 - 1.0, self.f() * 2.0 - 1.0, self.f() * 2.0 - 1.0];
            if dot(p, p) <= 1.0 {
                return p;
            }
        }
    }

    /// A direction, evenly spread over the sphere.
    fn unit(&mut self) -> V3 {
        loop {
            let p = self.in_ball();
            let l = length(p);
            if l > 1e-3 {
                return scale(p, 1.0 / l);
            }
        }
    }
}

fn sin32(x: f32) -> f32 {
    detmath::sin(x as f64) as f32
}

fn cos32(x: f32) -> f32 {
    detmath::cos(x as f64) as f32
}

/// The three random numbers of a corner of the noise lattice, from one hash (ten bits each: a thousand levels is plenty for a force).
fn corner(x: i32, y: i32, z: i32) -> [f32; 3] {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ (z as u32).wrapping_mul(0xcb1a_b31f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    [(h & 1023) as f32 / 1023.0, ((h >> 10) & 1023) as f32 / 1023.0, ((h >> 20) & 1023) as f32 / 1023.0]
}

/// A divergence-free swirl: the curl of three smooth noise potentials (it moves particles round without piling them up or emptying a place). The three potentials are the three numbers of each corner of the lattice, blended together by a quintic
/// over the same eight corners, so the lattice is hashed eight times and not twenty-four.
pub(crate) fn curl(p: V3) -> V3 {
    let (ix, iy, iz) = (p[0].floor(), p[1].floor(), p[2].floor());
    let (fx, fy, fz) = (p[0] - ix, p[1] - iy, p[2] - iz);
    let q = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let dq = |t: f32| 30.0 * t * t * (t * (t - 2.0) + 1.0);
    let (ux, uy, uz) = (q(fx), q(fy), q(fz));
    let (dx, dy, dz) = (dq(fx), dq(fy), dq(fz));
    let (xi, yi, zi) = (ix as i32, iy as i32, iz as i32);
    let mut c = [[0.0f32; 3]; 8];
    for (i, v) in c.iter_mut().enumerate() {
        *v = corner(xi + (i & 1) as i32, yi + ((i >> 1) & 1) as i32, zi + ((i >> 2) & 1) as i32);
    }
    // the slope of each potential along x, y and z: g[k] = (d/dx, d/dy, d/dz) of potential k
    let mut g = [[0.0f32; 3]; 3];
    for (k, gk) in g.iter_mut().enumerate() {
        let (a, b, cc, d, e, f, gg, h) = (c[0][k], c[1][k], c[2][k], c[3][k], c[4][k], c[5][k], c[6][k], c[7][k]);
        let (k1, k2, k3) = (b - a, cc - a, e - a);
        let (k4, k5, k6) = (a - b - cc + d, a - cc - e + gg, a - b - e + f);
        let k7 = -a + b + cc - d + e - f - gg + h;
        *gk = [dx * (k1 + k4 * uy + k6 * uz + k7 * uy * uz), dy * (k2 + k5 * uz + k4 * ux + k7 * uz * ux), dz * (k3 + k6 * ux + k5 * uy + k7 * ux * uy)];
    }
    [g[2][1] - g[1][2], g[0][2] - g[2][0], g[1][0] - g[0][1]]
}

/// One particle. Positions and speeds are in the emitter's space (the entity's for `Local`, the world's for `World`).
#[derive(Clone, Copy, Debug)]
pub struct P {
    pub pos: V3,
    pub vel: V3,
    pub age: f32,
    pub life: f32,
    pub size: f32,
    /// The cosine and sine of the angle the picture is turned by, and of what it turns by in a step ((1, 0) for a particle that does not spin): worked out once at birth, then only multiplied, so a spinning particle costs no sines a step and none a picture.
    pub cs: [f32; 2],
    pub dcs: [f32; 2],
    pub color: [f32; 4],
    pub frame: u32,
    pub id: u32,
    pub stuck: bool,
}

/// What an event asks for: `n` particles of emitter `target` at a place in the world, which the particle that caused it was moving as `vel` and coloured `color`.
#[derive(Clone, Copy, Debug)]
struct Birth {
    target: usize,
    pos: V3,
    vel: V3,
    color: [f32; 4],
    n: u32,
    inherit_velocity: f32,
    inherit_color: f32,
}

/// The state of one emitter.
#[derive(Clone, Debug)]
pub struct Em {
    pub parts: Vec<P>,
    /// A ribbon emitter's memory of where each particle has been: `trail_points` places a particle, newest at `head`, `count` of them are real.
    pub trail: Vec<V3>,
    pub tmeta: Vec<TrailMeta>,
    rng: Rng,
    acc: f32,
    next_id: u32,
    /// Which round of a looping emitter it is in, how many of that round's bursts have gone off, and whether it has taken its first active step (one that starts in the middle of a round does not fire the bursts that came before it).
    round: i64,
    burst_next: usize,
    started: bool,
    /// For an emitter the card simulates: what the picture still has to tell it (see `GpuEm`).
    pub gpu: GpuEm,
}

/// One step of an emitter that is simulated on the card: the clock of the step, how many particles are born in it, into which slots of the ring (`first` and on) and which numbers they have (`serial` and on: the card's random numbers).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuStep {
    pub t: f32,
    pub spawn: u32,
    pub first: u32,
    pub serial: u32,
}

/// What an emitter that the card simulates needs from the processor: the steps to take, with the births of each (the schedule is the same as for the processor's own particles: rate, bursts, curves, delay), the ring of slots they go into and how many slots there are.
/// The picture that draws it takes the steps (the card remembers the clock of the last it took and skips what it has, so that two pictures of one moment do not step twice) and `FxWorld::gpu_applied` lets the processor forget them.
#[derive(Clone, Debug, Default)]
pub struct GpuEm {
    pub capacity: u32,
    next_slot: u32,
    serial: u32,
    pub pending: Vec<GpuStep>,
}

/// The most steps kept for a picture that has not taken them (the oldest go: the card's particles are only for show).
const MAX_PENDING: usize = 4096;

impl GpuEm {
    fn push_step(&mut self, t: f32, spawn: u32) {
        let spawn = spawn.min(self.capacity);
        self.pending.push(GpuStep { t, spawn, first: self.next_slot, serial: self.serial });
        if self.capacity > 0 {
            self.next_slot = (self.next_slot + spawn) % self.capacity;
        }
        self.serial = self.serial.wrapping_add(spawn);
        if self.pending.len() > MAX_PENDING {
            let excess = self.pending.len() - MAX_PENDING;
            self.pending.drain(..excess);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TrailMeta {
    pub since: f32,
    pub count: u8,
    pub head: u8,
}

/// A system: the effect, the state of each emitter, and what the last picture needs to draw it.
#[derive(Clone, Debug)]
pub struct FxSystem {
    /// A number of its own, never used by another system or by this one after it starts over: what the card tells its particles of this system from those of the one before.
    pub epoch: u64,
    /// Whether the emitters that ask for the card are stepped there (a system the GPU renderer drew) or here (one the reference renderer drew): a system whose renderer changes starts over.
    pub gpu_mode: bool,
    pub seed: u32,
    pub effect: Arc<Effect>,
    pub key: u64,
    /// Steps taken: the system is at time `step` times `DT`.
    pub step: i64,
    pub em: Vec<Em>,
    inbox: Vec<Birth>,
    next: Vec<Birth>,
    last_pos: Option<V3>,
    pub model: M4,
    pub scale: f32,
    pub tint: [f32; 4],
    pub last_frame: u64,
    /// Steps taken by the last `advance` and the particles alive after it.
    pub steps_taken: u32,
    /// A ball (centre in the world, radius) that holds every particle alive after the last `advance` (`None`: there are none): a system whose entity is out of the picture but whose smoke still hangs in it is stepped and drawn.
    pub bounds: Option<(V3, f32)>,
}

impl FxSystem {
    pub fn live(&self) -> usize {
        self.em.iter().map(|e| e.parts.len()).sum()
    }

    /// What the card needs to simulate and draw each of the emitters it simulates (`simulate: gpu`) in this picture.
    pub fn gpu_jobs(&self, uid: u64) -> Vec<GpuEmitterJob> {
        if !self.gpu_mode {
            return Vec::new();
        }
        let radius = self.effect.radius() * self.scale;
        self.effect
            .emitters
            .iter()
            .enumerate()
            .filter(|(i, e)| e.gpu && self.em[*i].gpu.capacity > 0)
            .map(|(i, _)| GpuEmitterJob {
                uid,
                emitter: i,
                // (the top bit keeps the card's copy of the mesh apart from the entities' own; the emitter's number tells a system's emitters apart)
                key: (1u64 << 63) | (mix(uid, i as u64 + 0x6770) >> 1),
                epoch: self.epoch,
                effect: self.effect.clone(),
                capacity: self.em[i].gpu.capacity,
                seed: self.seed ^ (i as u32).wrapping_mul(0x9E37_79B1),
                model: self.model,
                k: self.scale,
                tint: self.tint,
                steps: self.em[i].gpu.pending.clone(),
                radius,
            })
            .collect()
    }
}

/// What the card needs to simulate and draw one emitter in this picture (`fx_gpu` of the GPU renderer).
#[derive(Clone, Debug)]
pub struct GpuEmitterJob {
    pub uid: u64,
    pub emitter: usize,
    /// The key of the emitter's mesh on the card.
    pub key: u64,
    /// The system's start-over number: when it changes, the card empties the emitter's particles.
    pub epoch: u64,
    pub effect: Arc<Effect>,
    /// How many slots the emitter has on the card.
    pub capacity: u32,
    pub seed: u32,
    /// Where the entity is (and how it is turned and scaled) now, `k` how big the effect is (`ParticleSystem.scale` times the entity's own), and the system's colour.
    pub model: M4,
    pub k: f32,
    pub tint: [f32; 4],
    /// The steps to take before drawing, oldest first; the card skips those it has taken.
    pub steps: Vec<GpuStep>,
    /// A ball round the entity that holds the particles (metres).
    pub radius: f32,
}

/// What the stepping of a system is asked: the entity and what it was told.
#[derive(Clone)]
pub struct FxJob {
    pub uid: u64,
    pub effect: Arc<Effect>,
    pub key: u64,
    pub time: f32,
    pub seed: u32,
    pub scale: f32,
    pub intensity: f32,
    pub quality: f32,
    pub tint: [f32; 4],
    pub tr: Transform,
    pub camera: V3,
    pub frame: u64,
    /// The renderer simulates the emitters that ask for it (`simulate: gpu`) on the card: this picture only schedules their births.
    pub gpu: bool,
    /// The world's colliders round the system, for the emitters that collide with `world` (gathered for this picture; `None` when no emitter does).
    pub world: Option<Arc<RayWorld>>,
}

/// All the systems of a picture's world.
#[derive(Default)]
pub struct FxWorld {
    systems: BTreeMap<u64, FxSystem>,
    /// What the last `advance_many` did: systems stepped, steps taken and particles alive in all.
    pub stats: FxStats,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FxStats {
    pub systems: usize,
    pub steps: u64,
    pub particles: usize,
}

fn mix(a: u64, b: u64) -> u64 {
    let mut h = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 29;
    h = h.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    h ^ (h >> 32)
}

impl FxWorld {
    pub fn get(&self, uid: u64) -> Option<&FxSystem> {
        self.systems.get(&uid)
    }

    /// The card has taken the steps of this emitter up to the clock `t`: they are not kept any longer.
    pub fn gpu_applied(&mut self, uid: u64, emitter: usize, epoch: u64, t: f32) {
        if let Some(s) = self.systems.get_mut(&uid) {
            if s.epoch == epoch {
                s.em[emitter].gpu.pending.retain(|st| st.t > t);
            }
        }
    }

    /// Lets go of the systems no picture has asked for in the last `idle` frames.
    pub fn trim(&mut self, frame: u64, idle: u64) {
        self.systems.retain(|_, s| frame.saturating_sub(s.last_frame) <= idle);
    }

    pub fn len(&self) -> usize {
        self.systems.len()
    }

    pub fn is_empty(&self) -> bool {
        self.systems.is_empty()
    }

    /// Brings every system of `jobs` up to its clock, on `threads` workers (a system is stepped by one thread: the result does not depend on the number).
    pub fn advance_many(&mut self, jobs: &[FxJob], threads: usize) {
        let mut work: Vec<(FxJob, Option<FxSystem>)> = jobs.iter().map(|j| (j.clone(), self.systems.remove(&j.uid))).collect();
        let workers = threads.clamp(1, 16).min(work.len().max(1));
        if workers <= 1 {
            for (job, slot) in work.iter_mut() {
                advance(slot, job);
            }
        } else {
            let next = std::sync::atomic::AtomicUsize::new(0);
            let slots: Vec<std::sync::Mutex<&mut (FxJob, Option<FxSystem>)>> = work.iter_mut().map(std::sync::Mutex::new).collect();
            let run = || loop {
                let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if k >= slots.len() {
                    break;
                }
                let mut g = slots[k].lock().unwrap_or_else(|e| e.into_inner());
                let (job, slot) = &mut **g;
                advance(slot, job);
            };
            std::thread::scope(|s| {
                for _ in 1..workers {
                    s.spawn(run);
                }
                run();
            });
        }
        let mut stats = FxStats::default();
        for (job, slot) in work {
            if let Some(sys) = slot {
                stats.systems += 1;
                stats.steps += sys.steps_taken as u64;
                stats.particles += sys.live();
                self.systems.insert(job.uid, sys);
            }
        }
        self.stats = stats;
    }
}

/// How many steps a system has taken at this clock: every step up to and including the one at the clock itself (so a burst that is born at time 0 is in the picture of time 0). The clock is a sum of small numbers: a little room, so that a step is not lost to rounding.
fn target_step(time: f32) -> i64 {
    ((time.max(0.0) as f64) * 60.0 + 1e-3).floor() as i64 + 1
}

fn new_system(job: &FxJob) -> FxSystem {
    let seed = if job.seed == 0 { job.uid } else { job.seed as u64 };
    let em = job
        .effect
        .emitters
        .iter()
        .enumerate()
        .map(|(i, e)| Em { parts: Vec::new(), trail: Vec::new(), tmeta: Vec::new(), rng: Rng::new(mix(seed, i as u64 + 1)), acc: 0.0, next_id: 0, round: 0, burst_next: 0, started: false, gpu: GpuEm { capacity: if e.gpu && job.gpu { e.max.min(job.effect.budget).max(1) } else { 0 }, ..Default::default() } })
        .collect();
    static EPOCHS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    FxSystem { epoch: EPOCHS.fetch_add(1, std::sync::atomic::Ordering::Relaxed), gpu_mode: job.gpu, seed: (seed ^ (seed >> 32)) as u32, effect: job.effect.clone(), key: job.key, step: 0, em, inbox: Vec::new(), next: Vec::new(), last_pos: None, model: M4::identity(), scale: 1.0, tint: [1.0; 4], last_frame: job.frame, steps_taken: 0, bounds: None }
}

/// How full the effect is at this distance from the camera: 1 up to `lod_near`, down to 0 at `lod_far`.
pub fn lod_factor(e: &Effect, distance: f32) -> f32 {
    if e.lod_far <= 0.0 {
        return 1.0;
    }
    ((e.lod_far - distance) / (e.lod_far - e.lod_near).max(1e-3)).clamp(0.0, 1.0)
}

fn advance(slot: &mut Option<FxSystem>, job: &FxJob) {
    let effect = &job.effect;
    let target = target_step(job.time);
    let prewarm = if effect.prewarm > 0.0 { effect.prewarm } else { effect.max_life() };
    let prewarm_steps = (prewarm * 60.0).ceil() as i64;
    let finite = effect.emitters.iter().any(|e| !e.looping);
    // (only an emitter that loops without a delay makes particles before time 0: nothing else needs the steps before it)
    let floor_step = if effect.emitters.iter().any(|e| e.looping && e.delay == 0.0) { -prewarm_steps } else { 0 };
    // when the system has to start again: it is new, its effect changed, its clock went back, it was out of view for too long to step through (more than 30 s, or, for an effect that only loops, more than it takes to fill up again)
    let restart = match slot.as_ref() {
        None => true,
        Some(s) => {
            let gap = target - s.step;
            s.key != job.key || s.gpu_mode != job.gpu || gap < 0 || gap > MAX_CATCHUP || (!finite && gap > prewarm_steps.max(GAP))
        }
    };
    if restart {
        let mut s = new_system(job);
        // (a looping effect starts as far back as it takes to be full; one that ends, from its beginning when that is not too far back)
        s.step = if finite { (target - MAX_CATCHUP).max(floor_step) } else { target - prewarm_steps };
        s.step = s.step.min(target);
        *slot = Some(s);
    }
    let sys = slot.as_mut().expect("just made");
    sys.last_frame = job.frame;
    sys.steps_taken = 0;
    let k = job.scale.max(1e-4) * job.tr.scale.iter().fold(0.0f32, |a, s| a.max(s.abs())).max(1e-4);
    let scale3 = [job.tr.scale[0] * job.scale, job.tr.scale[1] * job.scale, job.tr.scale[2] * job.scale];
    let dist = length(sub(job.tr.pos, job.camera));
    let mult = job.intensity.max(0.0) * job.quality.clamp(0.0, 1.0) * lod_factor(effect, dist);
    let budget = ((effect.budget as f32) * job.quality.clamp(0.1, 1.0)).max(1.0) as usize;
    sys.scale = k;
    sys.tint = job.tint;
    let steps = (target - sys.step).max(0);
    let from = sys.last_pos.unwrap_or(job.tr.pos);
    for j in 1..=steps {
        // the entity moved smoothly through the steps of one picture
        let f = j as f32 / steps as f32;
        let pos = add(from, scale(sub(job.tr.pos, from), f));
        let model = M4::from_trs(pos, job.tr.rot, scale3);
        let vel = if sys.last_pos.is_some() { scale(sub(job.tr.pos, from), 1.0 / (steps as f32 * DT)) } else { [0.0; 3] };
        step(sys, &model, pos, vel, k, mult, budget, job.world.as_deref());
        sys.step += 1;
        sys.steps_taken += 1;
    }
    sys.last_pos = Some(job.tr.pos);
    sys.model = M4::from_trs(job.tr.pos, job.tr.rot, scale3);
    sys.bounds = bounds_of(sys);
}

/// The box round every live particle (and, for ribbons, the places they have been), made a ball, with room for the pictures.
fn bounds_of(sys: &FxSystem) -> Option<(V3, f32)> {
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    let mut margin = 0.0f32;
    for (def, em) in sys.effect.emitters.iter().zip(&sys.em) {
        let local = def.space == Space::Local;
        let ribbon = def.render.kind == Kind::Ribbon;
        let npts = def.render.ribbon_points as usize;
        let grow = |lo: &mut V3, hi: &mut V3, q: V3| {
            let w = if local { sys.model.transform_point(q) } else { q };
            for a in 0..3 {
                lo[a] = lo[a].min(w[a]);
                hi[a] = hi[a].max(w[a]);
            }
        };
        let size_k = if local { sys.scale } else { 1.0 };
        let stretch = if def.render.facing == Facing::Velocity { def.render.stretch } else { 0.0 };
        let grows = def.curves.size.as_ref().map_or(1.0, |c| c.keys.iter().fold(1.0f32, |m, k| m.max(k.1[0])));
        for (i, p) in em.parts.iter().enumerate() {
            grow(&mut lo, &mut hi, p.pos);
            if ribbon {
                let m = em.tmeta[i];
                for j in 0..m.count as usize {
                    grow(&mut lo, &mut hi, em.trail[i * npts + (m.head as usize + npts - j) % npts]);
                }
            }
            margin = margin.max(p.size.abs() * size_k * grows * (1.0 + def.init.aspect.max(1.0)) + length(p.vel) * stretch * size_k);
        }
    }
    if sys.gpu_mode && sys.effect.emitters.iter().any(|e| e.gpu) {
        // (the particles of the card are not known here: the ball that holds the effect round where the entity is stands for them)
        let r = sys.effect.radius() * sys.scale;
        let c = sys.model.transform_point([0.0; 3]);
        let (nc, nr) = if lo[0] > hi[0] {
            (c, r)
        } else {
            let m = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
            let mr = length([(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5, (hi[2] - lo[2]) * 0.5]) + margin;
            // the ball that holds both balls
            let d = length(sub(m, c));
            if d + mr <= r { (c, r) } else if d + r <= mr { (m, mr) } else { let rr = (d + r + mr) * 0.5; (add(c, scale(sub(m, c), (rr - r) / d.max(1e-6))), rr) }
        };
        return Some((nc, nr + 0.05));
    }
    if lo[0] > hi[0] {
        return None;
    }
    let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
    let r = length([(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5, (hi[2] - lo[2]) * 0.5]) + margin + 0.05;
    (r.is_finite() && c.iter().all(|v| v.is_finite())).then_some((c, r))
}

struct Ctx<'a> {
    world: Option<&'a RayWorld>,
    t: f32,
    model: &'a M4,
    inv: &'a M4,
    pos: V3,
    vel: V3,
    k: f32,
    mult: f32,
}

fn to_space(space: Space, ctx: &Ctx, p: V3) -> V3 {
    match space {
        Space::World => p,
        Space::Local => ctx.inv.transform_point(p),
    }
}

fn dir_to_space(space: Space, ctx: &Ctx, v: V3) -> V3 {
    match space {
        Space::World => v,
        Space::Local => ctx.inv.transform_dir(v),
    }
}

fn from_space(space: Space, ctx: &Ctx, p: V3) -> V3 {
    match space {
        Space::World => p,
        Space::Local => ctx.model.transform_point(p),
    }
}

fn dir_from_space(space: Space, ctx: &Ctx, v: V3) -> V3 {
    match space {
        Space::World => v,
        Space::Local => ctx.model.transform_dir(v),
    }
}

#[allow(clippy::too_many_arguments)]
fn step(sys: &mut FxSystem, model: &M4, pos: V3, vel: V3, k: f32, mult: f32, budget: usize, world: Option<&RayWorld>) {
    let inv = model.inverse_affine();
    let t = sys.step as f32 * DT;
    let ctx = Ctx { world, t, model, inv: &inv, pos, vel, k, mult };
    let effect = sys.effect.clone();
    let mut inbox = std::mem::take(&mut sys.inbox);
    let mut live: usize = sys.em.iter().map(|e| e.parts.len()).sum();
    let mut births: Vec<Birth> = std::mem::take(&mut sys.next);
    births.clear();
    let on_card = sys.gpu_mode;
    for (i, def) in effect.emitters.iter().enumerate() {
        let st = &mut sys.em[i];
        if def.gpu && on_card {
            // the card moves the particles; here only the births are decided
            let mut n = Some(0u32);
            spawn_emitter(def, st, &ctx, &[], budget, &mut live, &mut n);
            st.gpu.push_step(t, n.unwrap_or(0));
            continue;
        }
        update_emitter(def, st, &ctx, &mut births, &mut live);
        let mine: Vec<Birth> = inbox.iter().copied().filter(|b| b.target == i).collect();
        spawn_emitter(def, st, &ctx, &mine, budget, &mut live, &mut None);
    }
    births.truncate(MAX_BIRTHS);
    // what this step's events asked for is born in the next one
    sys.inbox = births;
    inbox.clear();
    sys.next = inbox;
}

/// Ages, moves and retires the particles of an emitter; events that come of it go to `births` (in the world's space).
fn update_emitter(def: &Emitter, st: &mut Em, ctx: &Ctx, births: &mut Vec<Birth>, live: &mut usize) {
    let u = &def.update;
    let dt = DT;
    let drag = if u.drag > 0.0 { detmath::exp(-(u.drag * dt) as f64) as f32 } else { 1.0 };
    let space = def.space;
    let ribbon = def.render.kind == Kind::Ribbon;
    let npts = def.render.ribbon_points as usize;
    // forces that are written in the entity's space are moved to where the particles are
    let vortices: Vec<(V3, V3, &Vortex)> = u.vortices.iter().map(|v| (from_space_entity(space, ctx, v.pos), dir_entity(space, ctx, v.axis), v)).collect();
    let attractors: Vec<(V3, &Attractor)> = u.attractors.iter().map(|a| (from_space_entity(space, ctx, a.pos), a)).collect();
    let collide = def.update.collide.as_ref();
    let local_colliders = collide.is_some_and(|c| !c.balls.is_empty() || !c.boxes.is_empty());
    let mut i = 0;
    while i < st.parts.len() {
        let mut p = st.parts[i];
        p.age += dt;
        let mut dead = p.age >= p.life;
        let mut hit_event = false;
        // a particle that is not a number any more (a zero scale turns the entity's space into nothing) is gone
        if !(p.pos.iter().chain(p.vel.iter()).all(|v| v.is_finite())) {
            dead = true;
        }
        if !dead && !p.stuck {
            let uu = (p.age / p.life).clamp(0.0, 1.0);
            let mut acc = add(u.gravity, u.wind);
            if let Some(n) = &u.noise {
                let q = add(scale(p.pos, 1.0 / n.scale), scale([0.31, 1.0, 0.17], ctx.t * n.speed));
                acc = add(acc, scale(curl(q), n.strength));
            }
            for (c, axis, v) in &vortices {
                let r = sub(p.pos, *c);
                let rp = sub(r, scale(*axis, dot(r, *axis)));
                let dist = length(rp);
                if dist > 1e-4 && dist < v.radius {
                    let w = 1.0 - dist / v.radius;
                    acc = add(acc, add(scale(cross(*axis, rp), v.strength * w / dist), scale(rp, -v.pull * w / dist)));
                }
            }
            for (c, a) in &attractors {
                let d = sub(*c, p.pos);
                let dist = length(d);
                if a.kill > 0.0 && dist < a.kill {
                    dead = true;
                    break;
                }
                if dist > 1e-4 && dist < a.radius {
                    let w = if a.falloff <= 0.0 { 1.0 } else { powf_det(1.0 - dist / a.radius, a.falloff) };
                    acc = add(acc, scale(d, a.strength * w / dist));
                }
            }
            if !dead {
                p.vel = add(p.vel, scale(acc, dt));
                if drag != 1.0 {
                    p.vel = scale(p.vel, drag);
                }
                if u.speed_limit > 0.0 {
                    let s = length(p.vel);
                    if s > u.speed_limit {
                        p.vel = scale(p.vel, u.speed_limit / s);
                    }
                }
                let sm = def.curves.speed.as_ref().map_or(1.0, |g| g.at(uu)[0]);
                let before = p.pos;
                p.pos = add(p.pos, scale(p.vel, sm * dt));
                if p.dcs[1] != 0.0 {
                    let (c, s) = (p.cs[0] * p.dcs[0] - p.cs[1] * p.dcs[1], p.cs[1] * p.dcs[0] + p.cs[0] * p.dcs[1]);
                    // (one step of Newton's method keeps (c, s) a unit vector however long it turns)
                    let k = 0.5 * (3.0 - (c * c + s * s));
                    p.cs = [c * k, s * k];
                }
                if let Some(c) = collide {
                    let r = collide_all(c, def, ctx, &mut p, before, local_colliders);
                    match r {
                        Hitting::None => {}
                        Hitting::Moved(speed) => {
                            hit_event = speed > COLLIDE_EVENT_SPEED;
                            p.age += p.life * c.life_loss;
                        }
                        Hitting::Stuck => {
                            hit_event = true;
                            p.age += p.life * c.life_loss;
                        }
                        Hitting::Killed => {
                            hit_event = true;
                            dead = true;
                        }
                    }
                }
            }
        }
        if hit_event && births.len() < MAX_BIRTHS {
            emit(def, On::Collide, &p, space, ctx, births, &mut st.rng);
        }
        if dead || p.age >= p.life {
            if births.len() < MAX_BIRTHS {
                emit(def, On::Death, &p, space, ctx, births, &mut st.rng);
            }
            st.parts.swap_remove(i);
            if ribbon {
                swap_remove_trail(st, i, npts);
            }
            *live = live.saturating_sub(1);
            continue;
        }
        st.parts[i] = p;
        if ribbon {
            let m = &mut st.tmeta[i];
            m.since += dt;
            if m.since >= def.render.ribbon_interval {
                m.since -= def.render.ribbon_interval;
                m.head = ((m.head as usize + 1) % npts) as u8;
                m.count = (m.count + 1).min(npts as u8);
                st.trail[i * npts + m.head as usize] = p.pos;
            }
        }
        i += 1;
    }
}

fn swap_remove_trail(st: &mut Em, i: usize, npts: usize) {
    let last = st.tmeta.len() - 1;
    if i != last {
        for j in 0..npts {
            st.trail[i * npts + j] = st.trail[last * npts + j];
        }
    }
    st.trail.truncate(last * npts);
    st.tmeta.swap_remove(i);
}

/// `powf` for a number in 0..1 and an exponent of 0 to 8, made of the deterministic exponential and logarithm.
fn powf_det(x: f32, e: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if e == 1.0 {
        return x;
    }
    detmath::exp(e as f64 * detmath::ln(x as f64)) as f32
}

/// A place written in the entity's space, in the space the particles of an emitter are in.
fn from_space_entity(space: Space, ctx: &Ctx, p: V3) -> V3 {
    match space {
        Space::Local => p,
        Space::World => ctx.model.transform_point(p),
    }
}

fn dir_entity(space: Space, ctx: &Ctx, v: V3) -> V3 {
    match space {
        Space::Local => v,
        Space::World => normalize(ctx.model.transform_dir(v)),
    }
}

/// An event of an emitter fires for a particle: what it asks for goes to `births`.
fn emit(def: &Emitter, on: On, p: &P, space: Space, ctx: &Ctx, births: &mut Vec<Birth>, rng: &mut Rng) {
    for e in def.events.iter().filter(|e| e.on == on) {
        if e.chance < 1.0 && rng.f() >= e.chance {
            continue;
        }
        let n = if e.count[0] == e.count[1] { e.count[0] } else { e.count[0] + rng.next() % (e.count[1] - e.count[0] + 1) };
        if n == 0 {
            continue;
        }
        births.push(Birth { target: e.spawn, pos: from_space(space, ctx, p.pos), vel: dir_from_space(space, ctx, p.vel), color: p.color, n, inherit_velocity: e.inherit_velocity, inherit_color: e.inherit_color });
    }
}

enum Hitting {
    None,
    /// Hit and bounced, at this speed along the normal.
    Moved(f32),
    Stuck,
    Killed,
}

fn bounce(c: &Collide, normal: V3, depth: f32, p: &mut P) -> f32 {
    p.pos = add(p.pos, scale(normal, depth));
    let vn = dot(p.vel, normal);
    if vn < 0.0 {
        let along = scale(normal, vn);
        let tangent = sub(p.vel, along);
        p.vel = sub(scale(tangent, 1.0 - c.friction), scale(along, c.bounce));
    }
    -vn.min(0.0)
}

fn collide_all(c: &Collide, def: &Emitter, ctx: &Ctx, p: &mut P, before: V3, local_colliders: bool) -> Hitting {
    let mut hit: Option<f32> = None;
    let settle = |hit: &mut Option<f32>, speed: f32| *hit = Some(hit.map_or(speed, |h: f32| h.max(speed)));
    macro_rules! react {
        ($normal:expr, $depth:expr) => {{
            match c.hit {
                Hit::Kill => return Hitting::Killed,
                Hit::Stick => {
                    p.pos = add(p.pos, scale($normal, $depth));
                    p.vel = [0.0; 3];
                    p.stuck = true;
                    return Hitting::Stuck;
                }
                Hit::Bounce => {
                    let s = bounce(c, $normal, $depth, p);
                    settle(&mut hit, s);
                }
            }
        }};
    }
    for pl in &c.planes {
        // a plane of the world is a plane of the emitter's space for a particle in the world; one of the entity's moves with it
        let (n, d) = if def.space == Space::World && pl.relative {
            let n = normalize(ctx.model.transform_dir(pl.normal));
            (n, pl.d + dot(n, ctx.pos))
        } else {
            (pl.normal, pl.d)
        };
        let depth = d - dot(n, p.pos);
        if depth > 0.0 {
            react!(n, depth);
        }
    }
    if local_colliders {
        // balls and boxes are in the entity's space: a particle of the world's is looked at there and moved back
        let world = def.space == Space::World;
        let enter = |p: &P| if world { (ctx.inv.transform_point(p.pos), ctx.inv.transform_dir(p.vel)) } else { (p.pos, p.vel) };
        let leave = |pos: V3, vel: V3| if world { (ctx.model.transform_point(pos), ctx.model.transform_dir(vel)) } else { (pos, vel) };
        let (mut lp, mut lv) = enter(p);
        let mut q = *p;
        q.pos = lp;
        q.vel = lv;
        let mut changed = false;
        for b in &c.balls {
            let off = sub(q.pos, b.pos);
            let len = length(off);
            if len < 1e-6 {
                continue;
            }
            let out = scale(off, 1.0 / len);
            let (n, depth) = if b.inside { (scale(out, -1.0), len - b.radius) } else { (out, b.radius - len) };
            if depth > 0.0 {
                match c.hit {
                    Hit::Kill => return Hitting::Killed,
                    Hit::Stick => {
                        q.pos = add(q.pos, scale(n, depth));
                        q.vel = [0.0; 3];
                        let (pp, vv) = leave(q.pos, q.vel);
                        p.pos = pp;
                        p.vel = vv;
                        p.stuck = true;
                        return Hitting::Stuck;
                    }
                    Hit::Bounce => {
                        let s = bounce(c, n, depth, &mut q);
                        settle(&mut hit, s);
                        changed = true;
                    }
                }
            }
        }
        for b in &c.boxes {
            let off = sub(q.pos, b.pos);
            let pen = [b.half[0] - off[0].abs(), b.half[1] - off[1].abs(), b.half[2] - off[2].abs()];
            if pen[0] > 0.0 && pen[1] > 0.0 && pen[2] > 0.0 {
                let axis = if pen[0] <= pen[1] && pen[0] <= pen[2] { 0 } else if pen[1] <= pen[2] { 1 } else { 2 };
                let mut n = [0.0; 3];
                n[axis] = if off[axis] >= 0.0 { 1.0 } else { -1.0 };
                match c.hit {
                    Hit::Kill => return Hitting::Killed,
                    Hit::Stick => {
                        q.pos = add(q.pos, scale(n, pen[axis]));
                        q.vel = [0.0; 3];
                        let (pp, vv) = leave(q.pos, q.vel);
                        p.pos = pp;
                        p.vel = vv;
                        p.stuck = true;
                        return Hitting::Stuck;
                    }
                    Hit::Bounce => {
                        let s = bounce(c, n, pen[axis], &mut q);
                        settle(&mut hit, s);
                        changed = true;
                    }
                }
            }
        }
        if changed {
            lp = q.pos;
            lv = q.vel;
            let (pp, vv) = leave(lp, lv);
            p.pos = pp;
            p.vel = vv;
        }
    }
    // the colliders of the world (boxes, balls, capsules, mesh colliders, terrain): the step the particle just took is a ray, whose first hit is where it stops
    if let (true, Some(rw)) = (c.world, ctx.world) {
        let local = def.space == Space::Local;
        let (a, b) = if local { (ctx.model.transform_point(before), ctx.model.transform_point(p.pos)) } else { (before, p.pos) };
        let seg = sub(b, a);
        let len = length(seg);
        if len > 1e-5 {
            let d = scale(seg, 1.0 / len);
            if let Some(h) = rw.cast(a, d, len, lunaris_sim::layers::Filter::DEFAULT, 0) {
                let at = add(add(a, scale(d, h.t)), scale(h.normal, 0.002));
                let mut vel = if local { ctx.model.transform_dir(p.vel) } else { p.vel };
                let back = |pos: V3, vel: V3| if local { (ctx.inv.transform_point(pos), ctx.inv.transform_dir(vel)) } else { (pos, vel) };
                match c.hit {
                    Hit::Kill => return Hitting::Killed,
                    Hit::Stick => {
                        (p.pos, p.vel) = back(at, [0.0; 3]);
                        p.stuck = true;
                        return Hitting::Stuck;
                    }
                    Hit::Bounce => {
                        let vn = dot(vel, h.normal);
                        if vn < 0.0 {
                            let along = scale(h.normal, vn);
                            vel = sub(scale(sub(vel, along), 1.0 - c.friction), scale(along, c.bounce));
                        }
                        settle(&mut hit, -vn.min(0.0));
                        (p.pos, p.vel) = back(at, vel);
                    }
                }
            }
        }
    }
    match hit {
        Some(s) => Hitting::Moved(s),
        None => Hitting::None,
    }
}

/// Makes the particles an emitter's rate, bursts and the events that came to it ask for.
/// One particle is born: made at once, or (when the card will make it: `gpu` is `Some`) only counted.
fn born(def: &Emitter, st: &mut Em, ctx: &Ctx, age0: f32, gpu: &mut Option<u32>, live: &mut usize) {
    if let Some(n) = gpu {
        *n += 1;
        return;
    }
    let p = make(def, st, ctx, [0.0; 3], None, age0);
    push(def, st, p);
    *live += 1;
}

fn spawn_emitter(def: &Emitter, st: &mut Em, ctx: &Ctx, inbox: &[Birth], budget: usize, live: &mut usize, gpu: &mut Option<u32>) {
    let cap = def.max as usize;
    let room = |st: &Em, live: &usize| (cap.saturating_sub(st.parts.len())).min(budget.saturating_sub(*live));
    // events first
    for b in inbox {
        for _ in 0..b.n {
            if room(st, live) == 0 {
                break;
            }
            let at = to_space(def.space, ctx, b.pos);
            let v_in = dir_to_space(def.space, ctx, b.vel);
            let p = make(def, st, ctx, at, Some((v_in, b.inherit_velocity, b.color, b.inherit_color)), 0.0);
            push(def, st, p);
            *live += 1;
        }
    }
    if def.child {
        return;
    }
    let et = ctx.t - def.delay;
    // (a looping emitter that has no delay was running before time 0, which is what the prewarm steps through)
    if et < 0.0 && !(def.looping && def.delay == 0.0) {
        return;
    }
    let sp = &def.spawn;
    let round_len = def.duration;
    let (round, phase) = if def.looping && round_len > 0.0 {
        let r = (et / round_len).floor();
        (r as i64, et - r * round_len)
    } else {
        (0, et)
    };
    let active = def.looping || et < def.duration;
    if !st.started {
        // the first step it is active in: the bursts of this round that came more than a step ago are not made late
        st.started = true;
        st.round = round;
        st.burst_next = sp.bursts.partition_point(|b| b.time < phase - DT);
    } else if round != st.round {
        st.round = round;
        st.burst_next = 0;
    }
    while st.burst_next < sp.bursts.len() && sp.bursts[st.burst_next].time <= phase {
        let b = sp.bursts[st.burst_next];
        st.burst_next += 1;
        if b.chance < 1.0 && st.rng.f() >= b.chance {
            continue;
        }
        let n = if b.count[0] == b.count[1] { b.count[0] } else { b.count[0] + st.rng.next() % (b.count[1] - b.count[0] + 1) };
        let n = st.rng.stochastic(n as f32 * ctx.mult);
        for _ in 0..n {
            if room(st, live) == 0 {
                break;
            }
            born(def, st, ctx, 0.0, gpu, live);
        }
    }
    if active && sp.rate > 0.0 {
        let x = if round_len > 0.0 { phase / round_len } else { phase.min(1.0) };
        let rate = sp.rate * sp.rate_curve.as_ref().map_or(1.0, |c| c.at(x.clamp(0.0, 1.0))[0]) * ctx.mult;
        st.acc += rate * DT;
        let n = st.acc as u32;
        st.acc -= n as f32;
        for j in 0..n {
            if room(st, live) == 0 {
                st.acc = 0.0;
                break;
            }
            // born somewhere inside the step: its age says where, so that a stream is smooth and not a row of clumps
            let age0 = DT * (1.0 - (j as f32 + 0.5) / n as f32);
            born(def, st, ctx, age0, gpu, live);
        }
    }
}

fn push(def: &Emitter, st: &mut Em, p: P) {
    st.parts.push(p);
    if def.render.kind == Kind::Ribbon {
        let npts = def.render.ribbon_points as usize;
        st.trail.resize(st.trail.len() + npts, p.pos);
        st.tmeta.push(TrailMeta { since: 0.0, count: 0, head: 0 });
    }
}

/// A new particle of `def`. `at`: where in the emitter's space the shape is centred; `from`: for one born of an event, the speed (in the emitter's space), how much of it it takes along, the colour and how much of it it takes.
fn make(def: &Emitter, st: &mut Em, ctx: &Ctx, at: V3, from: Option<(V3, f32, [f32; 4], f32)>, age0: f32) -> P {
    let init = &def.init;
    let r = &mut st.rng;
    // where it starts, and the way out of the middle of the shape
    let (local, outward) = match &def.spawn.shape {
        Shape::Point => ([0.0; 3], [0.0, 1.0, 0.0]),
        Shape::Sphere { radius, shell } => {
            if *shell {
                let d = r.unit();
                (scale(d, *radius), d)
            } else {
                let b = r.in_ball();
                let l = length(b);
                (scale(b, *radius), if l > 1e-4 { scale(b, 1.0 / l) } else { [0.0, 1.0, 0.0] })
            }
        }
        Shape::Box { half } => {
            let q = [r.f() * 2.0 - 1.0, r.f() * 2.0 - 1.0, r.f() * 2.0 - 1.0];
            let l = length(q);
            ([q[0] * half[0], q[1] * half[1], q[2] * half[2]], if l > 1e-4 { scale(q, 1.0 / l) } else { [0.0, 1.0, 0.0] })
        }
        Shape::Disc { radius, ring } => {
            let a = r.f() * std::f32::consts::TAU;
            let rr = if *ring { *radius } else { *radius * r.f().sqrt() };
            let d = [cos32(a), 0.0, sin32(a)];
            (scale(d, rr), d)
        }
        Shape::Line { to } => {
            let t = r.f();
            (scale(*to, t), normalize(*to))
        }
    };
    // the way it flies: `direction`, spread by `spread` degrees, mixed with the way out of the shape
    let axis = {
        let d = init.direction;
        if length(d) < 1e-6 { [0.0, 1.0, 0.0] } else { normalize(d) }
    };
    let dir = if init.spread >= 180.0 {
        r.unit()
    } else if init.spread > 0.0 {
        let cos_max = cos32(init.spread.to_radians());
        let cz = 1.0 - r.f() * (1.0 - cos_max);
        let sz = (1.0 - cz * cz).max(0.0).sqrt();
        let a = r.f() * std::f32::consts::TAU;
        let t1 = {
            let h = if axis[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
            normalize(cross(axis, h))
        };
        let t2 = cross(axis, t1);
        add(add(scale(axis, cz), scale(t1, sz * cos32(a))), scale(t2, sz * sin32(a)))
    } else {
        axis
    };
    let dir = if init.radial > 0.0 {
        let m = add(scale(dir, 1.0 - init.radial), scale(outward, init.radial));
        if length(m) > 1e-5 { normalize(m) } else { dir }
    } else {
        dir
    };
    let speed = r.range(init.speed);
    let jitter = if init.color_jitter > 0.0 { 1.0 + (r.f() * 2.0 - 1.0) * init.color_jitter } else { 1.0 };
    let mut color = [init.color[0] * jitter, init.color[1] * jitter, init.color[2] * jitter, init.color[3]];
    let mut size = r.range(init.size);
    // a particle of the entity's own space is placed and flies in that space; one of the world's is born where the entity is, at its scale and turned as it is
    let (mut pos, mut vel) = match def.space {
        Space::Local => (add(at, local), scale(dir, speed)),
        Space::World => {
            size *= ctx.k;
            // (one made by an event is born where the event was, not where the entity is)
            let base = if from.is_some() { at } else { ctx.pos };
            (add(base, ctx.model.transform_dir(local)), add(ctx.model.transform_dir(scale(dir, speed)), scale(ctx.vel, init.inherit_velocity)))
        }
    };
    if let Some((v_in, iv, c_in, ic)) = from {
        vel = add(vel, scale(v_in, iv));
        for (o, i) in color.iter_mut().zip(c_in) {
            *o += (i - *o) * ic;
        }
    }
    let life = r.range(init.life);
    let rot = r.range(init.rotation).to_radians();
    let spin = r.range(init.spin).to_radians();
    let cs = if rot == 0.0 { [1.0, 0.0] } else { [cos32(rot), sin32(rot)] };
    let dcs = if spin == 0.0 { [1.0, 0.0] } else { [cos32(spin * DT), sin32(spin * DT)] };
    let frame = if def.render.frames > 1 { r.next() % def.render.frames } else { 0 };
    let id = st.next_id;
    st.next_id = st.next_id.wrapping_add(1);
    // a particle born part-way through a step has already flown for that long
    pos = add(pos, scale(vel, age0));
    P { pos, vel, age: age0, life, size, cs, dcs, color, frame, id, stuck: false }
}
