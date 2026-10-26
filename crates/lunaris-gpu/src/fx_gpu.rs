//! Particle emitters simulated on the card (`"simulate": "gpu"`, docs/FX.md; the shader is `shaders/fx.wgsl`). The processor decides every birth (`lunaris_render::fx_sim`: rate, bursts, delay, in steps of 1/60 s, each with the first slot of the emitter's ring it fills and
//! the serial of its first particle); the card keeps the particles (a position, a speed, an age, a life and a serial each: 48 bytes), steps them and writes the four vertices of each into the buffer the particle cloud pipelines draw, so that the picture is drawn like any other cloud:
//! soft particles, the blend, the texture and the order are all the ones every cloud has. What a picture needs of the processor is a few hundred bytes of numbers a step, whatever the number of particles.
//!
//! The steps are dispatched ahead of the pictures that draw them, oldest first, and the card remembers the clock of the last it took (`applied`): a picture that is given steps it has taken (two gatherings of one moment, a reflection probe, the picture itself) skips them.
use lunaris_render::fx_sim::{GpuEmitterJob, GpuStep, DT};
use lunaris_render::math::{M4, V3};
use lunaris_sim::fx::*;
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// The numbers of one dispatch: 40 vec4s (`Fx` of `fx.wgsl`), set by word so that the layout cannot drift from the shader's unseen.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FxU {
    w: [u32; 160],
}

impl FxU {
    fn new() -> FxU {
        FxU { w: [0; 160] }
    }

    fn f(&mut self, vec4: usize, v: [f32; 4]) {
        for (i, x) in v.iter().enumerate() {
            self.w[vec4 * 4 + i] = x.to_bits();
        }
    }

    fn u(&mut self, vec4: usize, v: [u32; 4]) {
        self.w[vec4 * 4..vec4 * 4 + 4].copy_from_slice(&v);
    }
}

/// The camera the picture is drawn from.
#[derive(Clone, Copy)]
pub struct Cam {
    pub pos: V3,
    pub forward: V3,
    pub right: V3,
    pub up: V3,
}

fn norm(v: V3, d: V3) -> V3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-6 {
        d
    } else {
        [v[0] / l, v[1] / l, v[2] / l]
    }
}

fn dir_of(m: &M4, v: V3) -> V3 {
    m.transform_dir(v)
}

/// The numbers of one dispatch of an emitter: a step (`Some`) or the picture's last dispatch, which draws.
fn uniform(job: &GpuEmitterJob, step: Option<&GpuStep>, clock: f32, cam: &Cam) -> FxU {
    let e = &job.effect.emitters[job.emitter];
    let local = e.space == Space::Local;
    let m = &job.model;
    let mut u = FxU::new();
    for (i, x) in m.0.iter().enumerate() {
        u.w[i] = x.to_bits();
    }
    u.f(4, [cam.right[0], cam.right[1], cam.right[2], 0.0]);
    u.f(5, [cam.up[0], cam.up[1], cam.up[2], 0.0]);
    u.f(6, [cam.forward[0], cam.forward[1], cam.forward[2], 0.0]);
    u.f(7, [cam.pos[0], cam.pos[1], cam.pos[2], 0.0]);
    let (first, count, serial) = step.map_or((0, 0, 0), |s| (s.first, s.spawn, s.serial));
    u.u(8, [job.capacity, first, count, serial]);
    let facing = match e.render.facing {
        Facing::Camera => 0u32,
        Facing::Velocity => 1,
        Facing::Axis(_) => 2,
    };
    u.u(9, [job.seed, u32::from(local) | facing << 1, e.render.sheet[0], e.render.sheet[1]]);
    let (mode, fps) = match e.render.frame_mode {
        FrameMode::OverLife => (0u32, 0.0f32),
        FrameMode::Random => (1, 0.0),
        FrameMode::Fps(f) => (2, f),
    };
    let planes: Vec<&Plane> = e.update.collide.as_ref().map_or(Vec::new(), |c| c.planes.iter().take(2).collect());
    let hit = match e.update.collide.as_ref().map(|c| c.hit) {
        Some(Hit::Kill) => 1u32,
        Some(Hit::Stick) => 2,
        _ => 0,
    };
    u.u(10, [e.render.frames.max(1), mode, e.update.vortices.len().min(2) as u32, e.update.attractors.len().min(2) as u32]);
    u.u(11, [planes.len() as u32, hit, 0, 0]);
    u.f(12, [DT, clock, e.init.life[0], e.init.life[1]]);
    let cos_spread = if e.init.spread >= 180.0 { -1.0 } else if e.init.spread <= 0.0 { 1.0 } else { e.init.spread.to_radians().cos() };
    u.f(13, [e.init.speed[0], e.init.speed[1], cos_spread, e.init.radial]);
    let axis = norm(e.init.direction, [0.0, 1.0, 0.0]);
    let (kind, a, b) = match &e.spawn.shape {
        Shape::Point => (0.0, [0.0; 4], [0.0; 4]),
        Shape::Sphere { radius, shell } => (1.0, [*radius, 0.0, 0.0, f32::from(*shell)], [0.0; 4]),
        Shape::Box { half } => (2.0, [half[0], half[1], half[2], 0.0], [0.0; 4]),
        Shape::Disc { radius, ring } => (3.0, [*radius, 0.0, 0.0, f32::from(*ring)], [0.0; 4]),
        Shape::Line { to } => (4.0, [0.0; 4], [to[0], to[1], to[2], 0.0]),
    };
    u.f(14, [axis[0], axis[1], axis[2], kind]);
    u.f(15, a);
    u.f(16, b);
    u.f(17, [e.init.size[0], e.init.size[1], e.init.aspect, job.k]);
    u.f(18, e.init.color);
    u.f(19, job.tint);
    u.f(20, [e.init.color_jitter, e.render.stretch, fps, 0.0]);
    u.f(21, [e.init.rotation[0].to_radians(), e.init.rotation[1].to_radians(), e.init.spin[0].to_radians(), e.init.spin[1].to_radians()]);
    u.f(22, [e.update.gravity[0], e.update.gravity[1], e.update.gravity[2], e.update.drag]);
    u.f(23, [e.update.wind[0], e.update.wind[1], e.update.wind[2], e.update.speed_limit]);
    let n = e.update.noise.unwrap_or(Noise { strength: 0.0, scale: 1.0, speed: 0.0 });
    u.f(24, [n.strength, n.scale, n.speed, f32::from(e.update.noise.is_some())]);
    if let Facing::Axis(a) = e.render.facing {
        let w = if local { norm(dir_of(m, a), a) } else { a };
        u.f(25, [w[0], w[1], w[2], 0.0]);
    }
    // places and axes written in the entity's space are moved to where the particles are (the world's, for a particle of the world)
    let place = |p: V3| if local { p } else { m.transform_point(p) };
    let axis_of = |a: V3| if local { a } else { norm(dir_of(m, a), a) };
    for (i, v) in e.update.vortices.iter().take(2).enumerate() {
        let (c, ax) = (place(v.pos), axis_of(v.axis));
        u.f(26 + i * 3, [c[0], c[1], c[2], v.strength]);
        u.f(27 + i * 3, [ax[0], ax[1], ax[2], v.pull]);
        u.f(28 + i * 3, [v.radius, 0.0, 0.0, 0.0]);
    }
    for (i, a) in e.update.attractors.iter().take(2).enumerate() {
        let c = place(a.pos);
        u.f(32 + i * 2, [c[0], c[1], c[2], a.strength]);
        u.f(33 + i * 2, [a.radius, a.falloff, a.kill, 0.0]);
    }
    let pos = m.transform_point([0.0; 3]);
    for (i, p) in planes.iter().enumerate() {
        let (nrm, d) = if !local && p.relative {
            let n = norm(dir_of(m, p.normal), p.normal);
            (n, p.d + n[0] * pos[0] + n[1] * pos[1] + n[2] * pos[2])
        } else {
            (p.normal, p.d)
        };
        u.f(36 + i, [nrm[0], nrm[1], nrm[2], d]);
    }
    let c = e.update.collide.as_ref();
    u.f(38, [c.map_or(0.0, |c| c.bounce), c.map_or(0.0, |c| c.friction), c.map_or(0.0, |c| c.life_loss), 0.0]);
    u
}

/// The tables the card looks the curves up in: 32 samples of (size, opacity, speed, light) over the life, then 32 of the colour (all ones where the effect has no curve).
fn curve_table(e: &Emitter) -> Vec<f32> {
    let mut t = Vec::with_capacity(64 * 4);
    for i in 0..32 {
        let u = i as f32 / 31.0;
        let at = |g: &Option<lunaris_sim::particle_modules::Gradient<1>>| g.as_ref().map_or(1.0, |g| g.at(u)[0]);
        t.extend_from_slice(&[at(&e.curves.size), at(&e.curves.alpha), at(&e.curves.speed), at(&e.curves.light)]);
    }
    for i in 0..32 {
        let u = i as f32 / 31.0;
        t.extend_from_slice(&e.curves.color.as_ref().map_or([1.0; 4], |g| g.at(u)));
    }
    t
}

fn buffer(device: &wgpu::Device, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(16), usage, mapped_at_creation: false })
}

/// What the card keeps of one emitter.
struct FxEmitter {
    state: wgpu::Buffer,
    lut: wgpu::Buffer,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    capacity: u32,
    epoch: u64,
    /// The clock of the last step taken.
    applied: f32,
    /// The effect's address and the emitter's number: when either changes (an effect file was read again) the curves and the shape are new.
    effect: usize,
    emitter: usize,
    bg: Option<(wgpu::BindGroup, u64)>,
    last_used: u64,
}

pub struct FxTask {
    pub key: u64,
    pub job: Arc<GpuEmitterJob>,
}

pub struct FxState {
    step: wgpu::ComputePipeline,
    emit: wgpu::ComputePipeline,
    emitters: HashMap<u64, FxEmitter>,
    params: Option<wgpu::Buffer>,
    /// Counts up whenever the numbers' buffer is made again: the bind groups that name it are then made again too.
    gen: u64,
    pub tasks: Vec<FxTask>,
    /// What the pictures took, for `FxWorld::gpu_applied`: the system, the emitter, the system's number and the clock of the last step taken.
    pub acks: Vec<(u64, usize, u64, f32)>,
}

impl FxState {
    pub fn new(step: wgpu::ComputePipeline, emit: wgpu::ComputePipeline) -> FxState {
        FxState { step, emit, emitters: HashMap::new(), params: None, gen: 0, tasks: Vec::new(), acks: Vec::new() }
    }

    /// Forget the emitters whose mesh is gone.
    pub fn retain(&mut self, live: impl Fn(u64) -> bool) {
        self.emitters.retain(|k, _| live(*k));
    }

    /// The emitter's buffers on the card (made when it is new or has changed); `(vertices, indices, whether they are new)`.
    pub fn ensure(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, job: &Arc<GpuEmitterJob>, frame: u64) -> (wgpu::Buffer, wgpu::Buffer, bool) {
        let cap = job.capacity.max(1);
        let eff = Arc::as_ptr(&job.effect) as usize;
        let stale = self.emitters.get(&job.key).is_none_or(|s| s.capacity != cap || s.effect != eff || s.emitter != job.emitter);
        if stale {
            let e = &job.effect.emitters[job.emitter];
            let vertices = buffer(device, "fx-vertices", cap as u64 * 4 * 48, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE);
            let mut idx: Vec<u32> = Vec::with_capacity(cap as usize * 6);
            for s in 0..cap {
                let b = s * 4;
                idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
            }
            let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("fx-indices"), contents: bytemuck::cast_slice(&idx), usage: wgpu::BufferUsages::INDEX });
            let lut = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("fx-curves"), contents: bytemuck::cast_slice(&curve_table(e)), usage: wgpu::BufferUsages::STORAGE });
            let state = buffer(device, "fx-particles", cap as u64 * 48, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
            self.emitters.insert(job.key, FxEmitter { state, lut, vertices, indices, capacity: cap, epoch: job.epoch, applied: f32::NEG_INFINITY, effect: eff, emitter: job.emitter, bg: None, last_used: frame });
        }
        let s = self.emitters.get_mut(&job.key).expect("just made");
        s.last_used = frame;
        if s.epoch != job.epoch {
            // the system started over: no particles
            s.epoch = job.epoch;
            s.applied = f32::NEG_INFINITY;
            s.state = buffer(device, "fx-particles", cap as u64 * 48, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
            s.bg = None;
        }
        let _ = queue;
        (s.vertices.clone(), s.indices.clone(), stale)
    }

    /// Steps the emitters staged for this picture, and draws them into their vertex buffers, in the commands `enc`. Nothing is recorded when there are none.
    pub fn encode(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, enc: &mut wgpu::CommandEncoder, cam: &Cam) {
        if self.tasks.is_empty() {
            return;
        }
        // (a dynamic offset must be a multiple of the card's alignment: the table's rows are the numbers' size rounded up to it)
        let stride = std::mem::size_of::<FxU>().next_multiple_of((device.limits().min_uniform_buffer_offset_alignment as usize).max(1));
        // the steps each emitter takes in this picture: those it has not taken (at most the last 240: a first picture is the prewarm)
        let mut plan: Vec<(usize, Vec<GpuStep>)> = Vec::new();
        let mut slots = 0usize;
        for (i, t) in self.tasks.iter().enumerate() {
            let Some(s) = self.emitters.get(&t.key) else { continue };
            let todo: Vec<GpuStep> = t.job.steps.iter().filter(|st| st.t > s.applied).copied().collect();
            let todo = todo[todo.len().saturating_sub(240)..].to_vec();
            slots += todo.len() + 1;
            plan.push((i, todo));
        }
        let table = slots * stride;
        if self.params.as_ref().is_none_or(|b| (b.size() as usize) < table) {
            let size = (table.next_power_of_two().max(4096)) as u64;
            self.params = Some(device.create_buffer(&wgpu::BufferDescriptor { label: Some("fx-numbers"), size, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }));
            self.gen += 1;
        }
        let params = self.params.as_ref().expect("made");
        let mut numbers = vec![0u8; table];
        let mut at = 0usize;
        let mut offsets: Vec<Vec<usize>> = Vec::new();
        for (i, todo) in &plan {
            let t = &self.tasks[*i];
            let mut mine = Vec::new();
            for st in todo {
                let u = uniform(&t.job, Some(st), st.t, cam);
                numbers[at * stride..at * stride + std::mem::size_of::<FxU>()].copy_from_slice(bytemuck::bytes_of(&u));
                mine.push(at * stride);
                at += 1;
            }
            // the last: it draws
            let u = uniform(&t.job, None, t.job.steps.last().map_or(0.0, |s| s.t), cam);
            numbers[at * stride..at * stride + std::mem::size_of::<FxU>()].copy_from_slice(bytemuck::bytes_of(&u));
            mine.push(at * stride);
            at += 1;
            offsets.push(mine);
        }
        queue.write_buffer(params, 0, &numbers);
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("fx"), timestamp_writes: None });
        for ((i, todo), mine) in plan.iter().zip(&offsets) {
            let t = &self.tasks[*i];
            let gen = self.gen;
            let Some(s) = self.emitters.get_mut(&t.key) else { continue };
            let fresh = s.bg.as_ref().is_some_and(|(_, g)| *g == gen);
            if !fresh {
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("fx"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: params, offset: 0, size: std::num::NonZeroU64::new(std::mem::size_of::<FxU>() as u64) }) },
                        wgpu::BindGroupEntry { binding: 1, resource: s.state.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: s.lut.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: s.vertices.as_entire_binding() },
                    ],
                });
                s.bg = Some((bg, gen));
            }
            let bg = &s.bg.as_ref().expect("made").0;
            let groups = s.capacity.div_ceil(64);
            pass.set_pipeline(&self.step);
            for (k, _) in todo.iter().enumerate() {
                pass.set_bind_group(0, bg, &[mine[k] as u32]);
                pass.dispatch_workgroups(groups, 1, 1);
            }
            pass.set_pipeline(&self.emit);
            pass.set_bind_group(0, bg, &[mine[todo.len()] as u32]);
            pass.dispatch_workgroups(groups, 1, 1);
            if let Some(last) = todo.last() {
                s.applied = last.t;
                self.acks.push((t.job.uid, t.job.emitter, t.job.epoch, last.t));
            }
        }
        drop(pass);
        self.tasks.clear();
    }
}
