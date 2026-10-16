//! Floaters: rigid bodies that ride the water (`Floater` and `WaterFlow`; docs/TERRAIN.md, "Floaters and waves"). A body is cut into `columns` x `columns` upright columns of its own volume; each column is lifted by the
//! share of it under the surface *at its own place* (the surface of a `Water` sheet or a `WaterBody`, with the same waves the renderers draw), at the middle of the part that is wet. The sum of the lifts is the buoyancy,
//! the lifts' moments about the body's middle are what tips a boat with a wave and rights it when it lists, and the water's current and drag slow the body relative to the water. Everything is plain `+ - * /`
//! and `sqrt` in a fixed order (the sine of the waves is a polynomial), over the bodies in uid order, so every machine finds the same bits.
//!
//! The cost is kept small on purpose: a world without a `Floater` pays for one empty query; a world with some builds its few water surfaces once a tick (a rectangle and a ceiling each); a body that is out of every
//! surface's rectangle or above its highest wave is left alone after a few comparisons and writes nothing; a body deep under a flat surface takes one comparison instead of its columns; only the bodies at the
//! surface sample columns, and a column costs one surface lookup (four sines when there are waves). Nothing is allocated per body.
use crate::components::{AngularVelocity, Collider, Floater, Lifetime, Particles, PhysicsSettings, RigidBody, Transform, Velocity, Water, WaterFlow};
use crate::physics::Shape;
use crate::physics_rot as rot;
use crate::terrain::Terrains;
use crate::water::PlacedWater;
use lunaris_ecs::World;

type V3 = [f32; 3];

/// The most columns along an axis of a body.
const MAX_COLUMNS: usize = 6;
/// A body that hits the water faster than this (metres per second) makes a `splash` event.
const SPLASH_SPEED: f32 = 1.0;
/// The most splashes that throw up spray in one tick (a hundred crates dropped at once make the first few, not a hundred clouds).
const MAX_FX_PER_TICK: u32 = 6;
/// The most a single tick may add to a body's spin (radians per second), so that a stiff boat in a short wave never kicks itself over.
const MAX_SPIN_STEP: f32 = 12.0;

/// The sine of `x` (radians) by a parabola and a correction: within 0.0011 of the real one, and only `+ - *`, `abs` and `floor`, so it is the same everywhere and several times cheaper than a series.
#[inline]
pub fn fast_sin(x: f32) -> f32 {
    const TWO_PI: f32 = 6.283_185_5;
    const INV_TWO_PI: f32 = 0.159_154_94;
    let r = x - (x * INV_TWO_PI + 0.5).floor() * TWO_PI; // -pi .. pi
    let y = 1.273_239_5 * r - 0.405_284_73 * r * r.abs();
    0.225 * (y * y.abs() - y) + y
}

/// The waves the renderers draw (`wave_height` in `scene.wgsl`): four sines of different directions and lengths that travel with `phase`.
#[derive(Clone, Copy)]
struct Wave {
    height: f32,
    k: f32,
    phase: f32,
}

impl Wave {
    fn new(height: f32, length: f32, phase: f32) -> Option<Wave> {
        (height > 0.0 && height.is_finite() && length.is_finite() && phase.is_finite()).then(|| Wave { height: height.min(10.0), k: 6.283_185_3 / length.max(0.2), phase })
    }

    /// How far the surface is above or below its level at (x, z).
    #[inline]
    fn at(&self, x: f32, z: f32) -> f32 {
        let (k, p) = (self.k, self.phase);
        let mut s = fast_sin((x * 0.8 + z * 0.6) * k + p);
        s += 0.6 * fast_sin((x * -0.5 + z * 0.85) * k * 1.7 + p * 1.3 + 1.7);
        s += 0.35 * fast_sin((x * 0.95 + z * -0.3) * k * 2.9 + p * 1.9 + 4.1);
        s += 0.2 * fast_sin((x * -0.2 + z * -0.98) * k * 4.3 + p * 2.4 + 2.2);
        s * self.height * 0.4
    }

    /// The highest the waves ever stand above the level.
    fn reach(&self) -> f32 {
        self.height * 0.4 * 2.15
    }
}

enum Kind<'a> {
    /// A flat sheet: the rectangle `half` (x, z) about `centre`, at `level`.
    Sheet,
    /// The surface of a water picture.
    Picture(PlacedWater<'a>),
}

/// One water surface of the world, prepared once a tick.
struct Surface<'a> {
    kind: Kind<'a>,
    centre: [f32; 2],
    half: [f32; 2],
    level: f32,
    /// The highest the surface can be anywhere (a body above this is dry).
    top: f32,
    /// Whether the level is the same everywhere (a body that is deep enough is wholly under).
    flat: bool,
    wave: Option<Wave>,
    flow: [f32; 2],
}

impl Surface<'_> {
    /// Could a body of this radius at (x, y, z) touch this surface at all?
    #[inline]
    fn near(&self, p: V3, r: f32) -> bool {
        (p[0] - self.centre[0]).abs() <= self.half[0] + r && (p[2] - self.centre[1]).abs() <= self.half[1] + r && p[1] - r <= self.top
    }

    /// The height of the surface at (x, z), waves counting `waves` times; `None` where there is no water.
    #[inline]
    fn height(&self, x: f32, z: f32, waves: f32) -> Option<f32> {
        let base = match &self.kind {
            Kind::Sheet => {
                if (x - self.centre[0]).abs() > self.half[0] || (z - self.centre[1]).abs() > self.half[1] {
                    return None;
                }
                self.level
            }
            Kind::Picture(p) => p.surface(x, z)?,
        };
        Some(match self.wave {
            Some(w) if waves > 0.0 => base + w.at(x, z) * waves,
            _ => base,
        })
    }
}

/// The water surfaces of the world in uid order: the `Water` sheets and the `WaterBody` pictures, with their waves and their currents.
fn surfaces<'a>(world: &mut World, fields: &'a Terrains) -> Vec<Surface<'a>> {
    let mut found: Vec<(u64, Surface<'a>)> = Vec::new();
    let mut sheets: Vec<(u64, V3, Water)> = Vec::new();
    let _ = world.for_each_uid::<(&Water, &Transform), _>(|_, uid, (w, t)| sheets.push((uid, t.pos, *w)));
    for (uid, pos, w) in sheets {
        if !(pos.iter().all(|v| v.is_finite()) && w.size[0].is_finite() && w.size[1].is_finite()) {
            continue;
        }
        let wave = Wave::new(w.wave_height, w.wave_length, w.phase);
        let top = pos[1] + wave.map_or(0.0, |w| w.reach());
        found.push((uid, Surface { kind: Kind::Sheet, centre: [pos[0], pos[2]], half: [w.size[0].abs() * 0.5, w.size[1].abs() * 0.5], level: pos[1], top, flat: true, wave, flow: [0.0; 2] }));
    }
    if !fields.is_empty() {
        for (uid, pos, w) in crate::water::bodies(world, fields) {
            let Some(field) = fields.get(world.strings().get(w.heightmap.0)) else { continue };
            let placed = PlacedWater::new(field, pos, &w);
            let wave = Wave::new(w.wave_height, w.wave_length, w.phase);
            let top = placed.base + placed.relief.max(0.0) + wave.map_or(0.0, |w| w.reach());
            found.push((uid, Surface { kind: Kind::Picture(placed), centre: placed.centre, half: [placed.size[0] * 0.5, placed.size[1] * 0.5], level: placed.base, top, flat: false, wave, flow: [0.0; 2] }));
        }
    }
    found.sort_by_key(|f| f.0);
    for (uid, s) in found.iter_mut() {
        if let Some(e) = world.entity(*uid) {
            if let Some(f) = world.get::<WaterFlow>(e) {
                if f.velocity[0].is_finite() && f.velocity[1].is_finite() {
                    s.flow = f.velocity;
                }
            }
        }
    }
    found.into_iter().map(|f| f.1).collect()
}

/// What one tick found out about a body.
struct Verdict {
    /// The share of the body under the surface.
    share: f32,
    /// The lift of the columns, as a share of the full lift: the sum of (the wet share of a column / the number of columns).
    lift: f32,
    /// The moments of the columns' lifts about the body's middle, for a unit lift upward (x, z; the y moment of a lift upward is none).
    moment: [f32; 2],
    flow: [f32; 2],
    /// The surface above the body's middle, for a splash.
    surface: f32,
}

/// The columns of one body at (`pos`, `axes`) with `half` extents against the surfaces `near` it.
fn columns(surfaces: &[Surface], near: &[usize], pos: V3, axes: &rot::M3, half: V3, n: usize, waves: f32, sphere: Option<f32>) -> Verdict {
    // the surface at the middle decides the current the body is in (the highest one wins, the lowest uid among equals)
    let mut at_middle: Option<(f32, usize)> = None;
    for &i in near {
        if let Some(y) = surfaces[i].height(pos[0], pos[2], waves) {
            if at_middle.is_none_or(|b| y > b.0) {
                at_middle = Some((y, i));
            }
        }
    }
    let radius = (half[0] * half[0] + half[1] * half[1] + half[2] * half[2]).sqrt();
    let flow = at_middle.map_or([0.0; 2], |m| surfaces[m.1].flow);
    // a body that is deep under a flat, still surface is wholly in: no columns to sample
    if let Some((y, i)) = at_middle {
        let s = &surfaces[i];
        if s.flat && pos[1] + radius <= s.level - s.wave.map_or(0.0, |w| w.reach()) * waves {
            return Verdict { share: 1.0, lift: 1.0, moment: [0.0; 2], flow, surface: y };
        }
    }
    if let Some(r) = sphere {
        // a ball: one column at its middle, its volume under the surface by the cap
        let Some((y, _)) = at_middle else { return Verdict { share: 0.0, lift: 0.0, moment: [0.0; 2], flow, surface: pos[1] } };
        let d = (y - (pos[1] - r)).clamp(0.0, 2.0 * r);
        let share = (d * d * (3.0 * r - d) / (4.0 * r * r * r)).clamp(0.0, 1.0);
        return Verdict { share, lift: share, moment: [0.0; 2], flow, surface: y };
    }
    let inv_n = 1.0 / n as f32;
    let weight = inv_n * inv_n;
    // how far up and down a column reaches: its own height along the body's y axis and the width of its cross section along the other two
    let reach = axes[1][1].abs() * half[1] + axes[0][1].abs() * half[0] * inv_n + axes[2][1].abs() * half[2] * inv_n;
    let e = reach.max(1.0e-4);
    let (mut lift, mut mx, mut mz) = (0.0f32, 0.0f32, 0.0f32);
    for iz in 0..n {
        let lz = ((iz as f32 + 0.5) * inv_n * 2.0 - 1.0) * half[2];
        for ix in 0..n {
            let lx = ((ix as f32 + 0.5) * inv_n * 2.0 - 1.0) * half[0];
            let c = [pos[0] + axes[0][0] * lx + axes[2][0] * lz, pos[1] + axes[0][1] * lx + axes[2][1] * lz, pos[2] + axes[0][2] * lx + axes[2][2] * lz];
            let mut surface: Option<f32> = None;
            for &i in near {
                if let Some(y) = surfaces[i].height(c[0], c[2], waves) {
                    if surface.is_none_or(|b| y > b) {
                        surface = Some(y);
                    }
                }
            }
            let Some(y) = surface else { continue };
            let wet = ((y - (c[1] - e)) / (2.0 * e)).clamp(0.0, 1.0);
            if wet <= 0.0 {
                continue;
            }
            // the lift acts at the middle of the part of the column that is under: its lever about the body's middle is horizontal (x, z)
            let w = wet * weight;
            lift += w;
            mx += (c[0] - pos[0]) * w;
            mz += (c[2] - pos[2]) * w;
        }
    }
    Verdict { share: lift, lift, moment: [mx, mz], flow, surface: at_middle.map_or(pos[1], |m| m.0) }
}

/// The body's own axes in world coordinates turned by its `Transform.rot`.
fn axes_of(t: &Transform) -> rot::M3 {
    rot::axes_of(rot::normalize_q(t.rot))
}

struct Body {
    uid: u64,
    f: Floater,
    t: Transform,
    c: Collider,
    mass: f32,
    gravity_scale: f32,
    lin: V3,
}

/// Lift, tip, steer and slow every `Floater`; run before the physics step (which integrates the velocities it changes). Pushes a `splash` event for a body that hit the water fast.
pub fn step(world: &mut World, fields: &Terrains, dt: f32, events: &mut Vec<(String, u64, Option<[f32; 3]>)>) {
    let mut list: Vec<Body> = Vec::new();
    let _ = world.for_each_uid::<(&Floater, &Transform, &Collider, &RigidBody, &Velocity), _>(|_, uid, (f, t, c, rb, v)| {
        if rb.mass > 0.0 && rb.mass.is_finite() && !c.trigger && t.pos.iter().all(|p| p.is_finite()) {
            list.push(Body { uid, f: *f, t: *t, c: *c, mass: rb.mass, gravity_scale: rb.gravity_scale, lin: v.lin });
        }
    });
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|b| b.uid);
    let surfaces = surfaces(world, fields);
    let mut gravity: Option<V3> = None;
    let mut near: Vec<usize> = Vec::with_capacity(surfaces.len());
    let mut fx_left = MAX_FX_PER_TICK;
    for b in &list {
        let s = [b.t.scale[0].abs(), b.t.scale[1].abs(), b.t.scale[2].abs()];
        // the body as a box (a ball and a capsule included: a capsule is a box as tall as it is and as wide as it is round)
        let (half, ball): (V3, Option<f32>) = match b.c.shape {
            0 => {
                let r = (b.c.size[0] * 0.5 * s[0].max(s[1]).max(s[2])).max(1.0e-4);
                ([r; 3], Some(r))
            }
            1 => ([(b.c.size[0] * 0.5 * s[0]).max(1.0e-4), (b.c.size[1] * 0.5 * s[1]).max(1.0e-4), (b.c.size[2] * 0.5 * s[2]).max(1.0e-4)], None),
            _ => {
                let r = (b.c.size[0] * 0.5 * s[0].max(s[2])).max(1.0e-4);
                ([r, (b.c.size[1] * 0.5 * s[1]).max(r), r], None)
            }
        };
        let radius = (half[0] * half[0] + half[1] * half[1] + half[2] * half[2]).sqrt();
        let waves = if b.f.waves.is_finite() { b.f.waves.clamp(0.0, 1.0) } else { 1.0 };
        near.clear();
        for (i, s) in surfaces.iter().enumerate() {
            if s.near(b.t.pos, radius) {
                near.push(i);
            }
        }
        let touching = !near.is_empty();
        let v = if touching {
            let n = if ball.is_some() { 1 } else { (b.f.columns as usize).clamp(1, MAX_COLUMNS) };
            let axes = axes_of(&b.t);
            columns(&surfaces, &near, b.t.pos, &axes, half, n, waves, ball)
        } else {
            Verdict { share: 0.0, lift: 0.0, moment: [0.0; 2], flow: [0.0; 2], surface: b.t.pos[1] }
        };
        let share = if v.share.is_finite() { v.share.clamp(0.0, 1.0) } else { 0.0 };
        // nothing to do and nothing to clear: the common case for a body that is out of the water
        if share <= 0.0 && b.f.submerged == 0.0 && b.f.splash == 0.0 {
            continue;
        }
        let Some(e) = world.entity(b.uid) else { continue };
        let was_dry = b.f.submerged <= 0.0;
        let g = *gravity.get_or_insert_with(|| {
            let (mut g, mut lowest) = ([0.0f32, -9.8, 0.0], u64::MAX);
            let _ = world.for_each_uid::<(&PhysicsSettings,), _>(|_, uid, (p,)| {
                if uid < lowest {
                    lowest = uid;
                    g = p.gravity;
                }
            });
            g
        });
        let mut hit = 0.0f32;
        if share > 0.0 {
            let density = if b.f.density.is_finite() && b.f.density > 1.0 { b.f.density } else { 1.0 };
            // how many times its own weight the water lifts a body that is wholly under, with the gravity scale the body falls with
            let k = (1000.0 / density) * b.gravity_scale;
            let current = if b.f.current.is_finite() { b.f.current.clamp(0.0, 1.0) } else { 1.0 };
            let flow = [v.flow[0] * current, v.flow[1] * current];
            // water pushes back harder the faster a body goes through it: a linear part (`drag`) and one that grows with the speed through the water, which stops a body that dives in fast without a bounce
            let (rx, ry, rz) = (b.lin[0] - flow[0], b.lin[1], b.lin[2] - flow[1]);
            let speed = (rx * rx + ry * ry + rz * rz).sqrt();
            let size = (half[0] + half[1] + half[2]) * (1.0 / 3.0);
            let damp = 1.0 / (1.0 + (b.f.drag.max(0.0) + speed * (0.35 / size.max(0.05))) * share * dt);
            let lin = [
                (b.lin[0] - g[0] * k * v.lift * dt - flow[0]) * damp + flow[0],
                (b.lin[1] - g[1] * k * v.lift * dt) * damp,
                (b.lin[2] - g[2] * k * v.lift * dt - flow[1]) * damp + flow[1],
            ];
            if was_dry {
                // the speed into the water along gravity
                let glen = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt().max(1.0e-6);
                let down = (b.lin[0] * g[0] + b.lin[1] * g[1] + b.lin[2] * g[2]) / glen;
                if down > SPLASH_SPEED {
                    hit = down;
                }
            }
            if let Some(vel) = world.get_mut::<Velocity>(e) {
                vel.lin = lin;
            }
            {
                let angular = world.get::<AngularVelocity>(e).map(|a| a.w);
                if let Some(w) = angular {
                    let axes = axes_of(&b.t);
                    let shape = match b.c.shape {
                        0 => Shape::Sphere((b.c.size[0] * 0.5 * s_max(&b.t)).max(1.0e-4)),
                        1 => Shape::Obb(half),
                        _ => Shape::Capsule(half[0], (half[1] - half[0]).max(0.0)),
                    };
                    let inv_i = rot::inv_inertia(&shape, b.mass);
                    // the lift of the water at the columns, moments about the middle: r x F with F upward = (-r.z, 0, r.x) times the lift; scaled by the mass the inertia carries
                    let glen = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt();
                    let torque = [-v.moment[1] * b.mass * k * glen, 0.0, v.moment[0] * b.mass * k * glen];
                    let local = [rot::dot(axes[0], torque), rot::dot(axes[1], torque), rot::dot(axes[2], torque)];
                    let spin = [local[0] * inv_i[0], local[1] * inv_i[1], local[2] * inv_i[2]];
                    let alpha = rot::add(rot::add(rot::mul(axes[0], spin[0]), rot::mul(axes[1], spin[1])), rot::mul(axes[2], spin[2]));
                    let damp = 1.0 / (1.0 + b.f.angular_drag.max(0.0) * share * dt);
                    let mut next = [0.0f32; 3];
                    for a in 0..3 {
                        next[a] = (w[a] + (alpha[a] * dt).clamp(-MAX_SPIN_STEP, MAX_SPIN_STEP)) * damp;
                    }
                    if let Some(av) = world.get_mut::<AngularVelocity>(e) {
                        av.w = next;
                    }
                }
            }
        }
        if let Some(f) = world.get_mut::<Floater>(e) {
            f.submerged = share;
            f.splash = hit;
        }
        if hit > 0.0 {
            events.push(("splash".to_string(), b.uid, Some([b.t.pos[0], v.surface, b.t.pos[2]])));
            if b.f.splash_fx && fx_left > 0 {
                fx_left -= 1;
                spray(world, [b.t.pos[0], v.surface, b.t.pos[2]], hit, half[0] + half[2], b.uid);
            }
        }
    }
}

/// Droplets thrown up and a soft mist round a body that hit the water at `speed` (`width` metres wide): two bursts of particles that remove themselves.
fn spray(world: &mut World, at: V3, speed: f32, width: f32, uid: u64) {
    let seed = (uid as u32).wrapping_mul(2_654_435_761);
    let drops = ((20.0 + speed * width * 30.0) as u32).clamp(24, 240);
    let e = world.spawn();
    let _ = world.insert(e, Transform { pos: at, ..Transform::default() });
    let _ = world.insert(
        e,
        Particles {
            count: drops,
            lifetime: 1.1,
            looping: false,
            prewarm: false,
            spread_time: 0.08,
            shape: 3,
            extent: [width * 0.4, 0.0, width * 0.4],
            cone: 32.0,
            speed_range: [0.8 + speed * 0.12, 1.5 + speed * 0.38],
            gravity: [0.0, -9.8, 0.0],
            drag: 0.2,
            size: [0.09, 0.035],
            size_jitter: 0.4,
            color: [0.92, 0.97, 1.0, 0.9],
            color_end: [0.8, 0.9, 1.0, 0.0],
            fade_in: 0.02,
            additive: false,
            seed,
            ..Particles::default()
        },
    );
    let _ = world.insert(e, Lifetime { seconds: 1.6 });
    let m = world.spawn();
    let _ = world.insert(m, Transform { pos: at, ..Transform::default() });
    let _ = world.insert(
        m,
        Particles {
            count: (drops / 3).max(10),
            lifetime: 1.4,
            looping: false,
            prewarm: false,
            spread_time: 0.15,
            shape: 3,
            extent: [width * 0.3, 0.0, width * 0.3],
            cone: 70.0,
            speed_range: [0.3, 0.6 + speed * 0.06],
            gravity: [0.0, 0.15, 0.0],
            drag: 1.2,
            size: [0.15 + width.min(2.5) * 0.12, 0.35 + width.min(2.5) * 0.3],
            size_jitter: 0.3,
            color: [1.0, 1.0, 1.0, 0.1],
            color_end: [1.0, 1.0, 1.0, 0.0],
            fade_in: 0.1,
            additive: false,
            seed: seed ^ 0x9e37,
            ..Particles::default()
        },
    );
    let _ = world.insert(m, Lifetime { seconds: 1.8 });
}

fn s_max(t: &Transform) -> f32 {
    t.scale[0].abs().max(t.scale[1].abs()).max(t.scale[2].abs())
}
