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
use std::sync::Arc;
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

// OCEAN BEGIN (the table is made by tools/gen_ocean.py; edit the script, not the numbers)
/// The waves that move the mesh and the bodies that float (`ocean_disp` in `scene.wgsl`, sampled from a JONSWAP spectrum): for each its direction as (cos, sin), the wave number as a multiple of the
/// peak's, its amplitude as a multiple of the significant height, and its phase.
const OCEAN_V: [[f32; 5]; 25] = [
    [0.80000067, 0.59999911, 0.434, 0.0098440602, 4.6819227],
    [0.86382524, -0.50379158, 0.532, 0.016177173, 1.3359719],
    [0.37230102, 0.92811204, 0.6156, 0.079437833, 5.3992607],
    [0.93620625, 0.35145107, 0.69255, 0.047628978, 0.93006218],
    [-0.3175663, 0.94823607, 0.765225, 0.069579758, 1.2793331],
    [0.70856986, 0.70564067, 0.83235, 0.089447417, 0.56733557],
    [0.98205244, 0.18860806, 0.89745, 0.11370389, 4.2140777],
    [0.42955694, 0.90303977, 0.960175, 0.12170799, 0.31715633],
    [0.83990278, 0.54273688, 1.019875, 0.13608808, 6.078139],
    [0.99041328, -0.13813595, 1.081375, 0.051608111, 1.6928003],
    [0.62616969, 0.77968681, 1.14995, 0.085739014, 6.2577439],
    [0.93953627, 0.3424494, 1.22625, 0.012438399, 1.296134],
    [0.13752948, 0.99049767, 1.310625, 0.043114563, 5.7680196],
    [0.76754506, 0.640995, 1.40965, 0.036587199, 6.2137731],
    [0.99999962, -0.00087576157, 1.5246, 0.029372964, 4.9825238],
    [0.42210992, 0.90654466, 1.6632, 0.095919031, 3.6480765],
    [0.90322141, 0.4291749, 1.8348, 0.10702956, 6.2732576],
    [0.46565571, -0.88496596, 2.0433, 0.0039202489, 1.3549507],
    [0.62208369, 0.78295075, 2.3079, 0.0127442, 1.0680043],
    [0.99923144, 0.03919851, 2.6533, 0.04217132, 5.9529851],
    [-0.078472099, 0.99691631, 3.0927, 0.079070814, 0.2895483],
    [0.83207906, 0.55465704, 3.66, 0.071612728, 1.204931],
    [0.71563773, -0.69847165, 4.4, 0.063075106, 4.3558],
    [0.22241715, 0.9749516, 5.39, 0.046099261, 3.4653057],
    [0.99444185, 0.10528726, 6.615, 0.026652314, 3.3905129],
];
/// The sum of the amplitudes (a multiple of the significant height): the surface never stands higher than this above its level.
const OCEAN_AMP_SUM: f32 = 1.49077;
/// The domain warp (`ocean_warp` in `scene.wgsl`): for each of two slow sines, its direction, its wave number (per peak length) and its phase; the waves are evaluated at a point bent by them, up to `OCEAN_WARP_AMP` peak lengths.
const OCEAN_WARP: [[f32; 4]; 2] = [[0.83, -0.557, 1.1855067, 1.3], [0.37, 0.929, 1.6110732, 4.1]];
const OCEAN_WARP_AMP: f32 = 0.2;
/// The sum of amplitude times wave number (`OCEAN_SK` in `scene.wgsl`): how steep the sea can get, which bounds the sideways pull of the waves.
const OCEAN_SK: f32 = 2.6168607;
// OCEAN END

/// The waves the renderers draw (`ocean_disp` in `scene.wgsl`): the waves of the spectrum with the speeds of deep water, each a Gerstner wave, so that the points of the surface also move sideways towards
/// the crests. Everything about a surface that does not depend on the place is worked out once, here.
#[derive(Clone, Copy)]
struct Wave {
    /// per wave: the wave vector (x, z), the phase at the origin (the wave's own phase minus its speed times the clock), the amplitude
    c: [[f32; 4]; OCEAN_V.len()],
    /// per wave: how far the sideways pull moves a point per unit of the sine
    pull: [[f32; 2]; OCEAN_V.len()],
    reach: f32,
    /// the domain warp: for each slow sine the wave vector (x, z) and the phase, and the size of the bend in metres
    warp: [[f32; 3]; 2],
    bend: f32,
    /// the significant height and the peak wave length (for the shoaling)
    hs: f32,
    len: f32,
}

impl Wave {
    fn new(height: f32, length: f32, phase: f32) -> Option<Wave> {
        if !(height > 0.0 && height.is_finite() && length.is_finite() && phase.is_finite()) {
            return None;
        }
        let height = height.min(10.0);
        let kp = 6.283_185_3 / length.max(0.2);
        // all of the sideways pull, unless the sea is so steep that it would fold everywhere (`ocean_chop`)
        let lam = (0.85 / (height * kp * OCEAN_SK).max(1.0e-4)).min(1.0);
        let mut c = [[0.0; 4]; OCEAN_V.len()];
        let mut pull = [[0.0; 2]; OCEAN_V.len()];
        for (i, w) in OCEAN_V.iter().enumerate() {
            let k = kp * w[2];
            let a = height * w[3];
            c[i] = [k * w[0], k * w[1], w[4] - (9.81 * k).sqrt() * phase, a];
            pull[i] = [lam * a * w[0], lam * a * w[1]];
        }
        let warp = [0, 1].map(|i| [OCEAN_WARP[i][0] * OCEAN_WARP[i][2] / length.max(0.2), OCEAN_WARP[i][1] * OCEAN_WARP[i][2] / length.max(0.2), OCEAN_WARP[i][3]]);
        Some(Wave { c, pull, reach: height * OCEAN_AMP_SUM, warp, bend: OCEAN_WARP_AMP * length.max(0.2), hs: height, len: length.max(0.2) })
    }

    /// The point the waves are evaluated at for the point that rests at (x, z): bent by two slow sines (`ocean_warp`).
    #[inline]
    fn bent(&self, x: f32, z: f32) -> (f32, f32) {
        let a = fast_sin(self.warp[0][0] * x + self.warp[0][1] * z + self.warp[0][2]);
        let b = fast_sin(self.warp[1][0] * x + self.warp[1][1] * z + self.warp[1][2]);
        (x + self.bend * a, z + self.bend * b)
    }

    /// How far the surface is above or below its level at (x, z): the sideways pull is undone first (the point that now stands at (x, z) rested at (x, z) plus the pull), then the heights are summed.
    #[inline]
    #[allow(dead_code)]
    fn at(&self, x: f32, z: f32) -> f32 {
        self.at_scaled(x, z, 1.0)
    }

    /// The same with every wave's height (and pull) scaled by `f`: the sea in shallow water (`shoal`).
    #[inline]
    fn at_scaled(&self, x: f32, z: f32, f: f32) -> f32 {
        const HALF_PI: f32 = 1.570_796_4;
        let (qx, qz) = self.bent(x, z);
        let (mut rx, mut rz) = (x, z);
        for (c, p) in self.c.iter().zip(self.pull.iter()) {
            let s = fast_sin(c[0] * qx + c[1] * qz + c[2]) * f;
            rx += p[0] * s;
            rz += p[1] * s;
        }
        let (qx, qz) = self.bent(rx, rz);
        let mut h = 0.0;
        for c in self.c.iter() {
            h += c[3] * fast_sin(c[0] * qx + c[1] * qz + c[2] + HALF_PI);
        }
        h * f
    }

    /// The highest the waves ever stand above the level.
    fn reach(&self) -> f32 {
        self.reach
    }
}


/// How much of its height the sea keeps over water `depth` metres deep (`ocean_shoal` in `scene.wgsl`): the height of a wave grows as the depth to the power minus one quarter once the depth is less than half the wave length
/// (Green's law), and the significant height cannot stay above half the depth (the surf zone saturates; the waves break): so 1 in deep water, a little more as the sea shoals, then falling to nothing at the waterline.
pub(crate) fn shoal(depth: f32, hs: f32, length: f32) -> f32 {
    let d = depth.max(0.0);
    if d >= 0.6 * length {
        return 1.0;
    }
    let grow = (0.5 * length / d.max(0.1)).clamp(1.0, 8.0).sqrt().sqrt();
    (hs * grow).min(0.5 * d) / hs.max(1.0e-4)
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
    /// The terrains under the sea, when its waves shoal over them (a sheet at the level of the sea that has a `WaterLook`, as in the renderer).
    floors: Option<Arc<Vec<crate::terrain::Ground>>>,
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
            Some(w) if waves > 0.0 => {
                let f = match &self.floors {
                    Some(fl) => shoal(fl.iter().find_map(|g| g.placed().surface(x, z)).map_or(1000.0, |y| self.level - y), w.hs, w.len),
                    None => 1.0,
                };
                base + w.at_scaled(x, z, f) * waves
            }
            _ => base,
        })
    }
}

/// The water surfaces of the world in uid order: the `Water` sheets and the `WaterBody` pictures, with their waves and their currents.
fn surfaces<'a>(world: &mut World, fields: &'a Terrains) -> Vec<Surface<'a>> {
    let mut found: Vec<(u64, Surface<'a>)> = Vec::new();
    let mut sheets: Vec<(u64, V3, Water)> = Vec::new();
    let _ = world.for_each_uid::<(&Water, &Transform), _>(|_, uid, (w, t)| sheets.push((uid, t.pos, *w)));
    // the sea: the lowest sheet that has a look, and the terrains under it (the waves of every sheet at its level shoal over them, as in the renderer)
    let sea_level = sheets.iter().filter(|(uid, _, _)| world.entity(*uid).is_some_and(|e| world.get::<crate::components::WaterLook>(e).is_some())).min_by_key(|(uid, _, _)| *uid).map(|(_, pos, _)| pos[1]);
    let grounds = if sea_level.is_some() { crate::terrain::grounds(world, fields) } else { Vec::new() };
    let floors = if grounds.is_empty() { None } else { Some(Arc::new(grounds)) };
    for (uid, pos, w) in sheets {
        if !(pos.iter().all(|v| v.is_finite()) && w.size[0].is_finite() && w.size[1].is_finite()) {
            continue;
        }
        let wave = Wave::new(w.wave_height, w.wave_length, w.phase);
        let floors_here = match (sea_level, &floors) {
            (Some(l), Some(f)) if wave.is_some() && (pos[1] - l).abs() < 0.05 => Some(f.clone()),
            _ => None,
        };
        // (over shallows the waves grow by up to 1.7 before they break)
        let top = pos[1] + wave.map_or(0.0, |w| w.reach()) * if floors_here.is_some() { 1.7 } else { 1.0 };
        found.push((uid, Surface { kind: Kind::Sheet, centre: [pos[0], pos[2]], half: [w.size[0].abs() * 0.5, w.size[1].abs() * 0.5], level: pos[1], top, flat: true, wave, flow: [0.0; 2], floors: floors_here }));
    }
    if !fields.is_empty() {
        for (uid, pos, w) in crate::water::bodies(world, fields) {
            let Some(field) = fields.get(world.strings().get(w.heightmap.0)) else { continue };
            let placed = PlacedWater::new(field, pos, &w);
            let wave = Wave::new(w.wave_height, w.wave_length, w.phase);
            let top = placed.base + placed.relief.max(0.0) + wave.map_or(0.0, |w| w.reach());
            found.push((uid, Surface { kind: Kind::Picture(placed), centre: placed.centre, half: [placed.size[0] * 0.5, placed.size[1] * 0.5], level: placed.base, top, flat: false, wave, flow: [0.0; 2], floors: None }));
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

#[cfg(test)]
mod ocean_tests {
    use super::*;

    /// The surface by the book, in double precision: the point that rests at u stands at u + D(u) (the Gerstner displacement), at the height h(u); to find the height at a place, find the u that stands there.
    fn exact(height: f64, length: f64, phase: f64, pull: f64, x: f64, z: f64) -> f64 {
        let kp = std::f64::consts::TAU / length;
        let lam = (0.85 / (height * kp * OCEAN_SK as f64)).min(1.0) * pull;
        let wave = |u: [f64; 2]| -> ([f64; 2], f64) {
            let bend = OCEAN_WARP_AMP as f64 * length;
            let ph = |i: usize| (OCEAN_WARP[i][0] as f64 * u[0] + OCEAN_WARP[i][1] as f64 * u[1]) * OCEAN_WARP[i][2] as f64 / length + OCEAN_WARP[i][3] as f64;
            let u = [u[0] + bend * ph(0).sin(), u[1] + bend * ph(1).sin()];
            let (mut d, mut h) = ([0.0, 0.0], 0.0);
            for w in OCEAN_V.iter() {
                let (dx, dz, m, a, ph) = (w[0] as f64, w[1] as f64, w[2] as f64, w[3] as f64 * height, w[4] as f64);
                let k = kp * m;
                let th = k * (dx * u[0] + dz * u[1]) - (9.81 * k).sqrt() * phase + ph;
                h += a * th.cos();
                d[0] -= lam * a * dx * th.sin();
                d[1] -= lam * a * dz * th.sin();
            }
            (d, h)
        };
        let mut u = [x, z];
        for _ in 0..40 {
            let (d, _) = wave(u);
            u = [x - d[0], z - d[1]];
        }
        wave(u).1
    }

    fn samples(w: &Wave, extent: f32, step: f32) -> Vec<f32> {
        let n = (extent / step) as i32;
        let mut out = Vec::new();
        for i in 0..n {
            for j in 0..n {
                out.push(w.at(i as f32 * step - extent * 0.5, j as f32 * step - extent * 0.5));
            }
        }
        out
    }

    #[test]
    fn the_sea_has_the_wave_height_its_spectrum_says() {
        // Hs = 4 sqrt(m0): the standard deviation of the surface is a quarter of the significant wave height
        for (hs, len) in [(0.5f32, 14.0f32), (2.0, 40.0), (4.0, 90.0)] {
            let w = Wave::new(hs, len, 3.0).unwrap();
            let h = samples(&w, 1200.0, 4.0);
            let mean = h.iter().sum::<f32>() / h.len() as f32;
            let var = h.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / h.len() as f32;
            let est = 4.0 * var.sqrt();
            assert!((est - hs).abs() < 0.1 * hs, "Hs {hs} length {len}: the surface says {est}");
            assert!(h.iter().all(|v| v.abs() <= w.reach() + 1e-3), "never above the reach");
        }
    }

    #[test]
    fn the_sea_runs_downwind() {
        // a little later the heights at a point further along the wind equal the heights now at the first point (the peak of the energy goes at c = sqrt(g / k))
        let early = Wave::new(2.0, 40.0, 0.0).unwrap();
        let late = Wave::new(2.0, 40.0, 0.5).unwrap();
        let c_peak = (9.81f32 * 40.0 / std::f32::consts::TAU).sqrt();
        let dir = [0.6435f32.cos(), 0.6435f32.sin()];
        let (mut moved, mut stayed) = (0.0f32, 0.0f32);
        for i in 0..400 {
            let (x, z) = (i as f32 * 3.1 - 600.0, i as f32 * 1.7 - 300.0);
            let now = early.at(x, z);
            moved += (late.at(x + dir[0] * c_peak * 0.5, z + dir[1] * c_peak * 0.5) - now).powi(2);
            stayed += (late.at(x, z) - now).powi(2);
        }
        assert!(moved < 0.6 * stayed, "the surface moves with the wind: moved {moved} stayed {stayed}");
    }

    #[test]
    fn the_copy_in_the_simulation_agrees_with_the_waves_by_the_book() {
        let (hs, len, phase) = (1.5f32, 30.0f32, 7.3f32);
        let w = Wave::new(hs, len, phase).unwrap();
        let (mut worst, mut err_pulled, mut err_straight) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..300 {
            let (x, z) = (i as f64 * 5.37 - 800.0, i as f64 * 3.11 - 400.0);
            let here = w.at(x as f32, z as f32) as f64;
            let e = exact(hs as f64, len as f64, phase as f64, 1.0, x, z);
            worst = worst.max((here - e).abs());
            err_pulled += (here - e).abs();
            // the same sea without the sideways pull: the copy must be nearer to the one that has it
            err_straight += (here - exact(hs as f64, len as f64, phase as f64, 0.0, x, z)).abs();
        }
        // fast_sin is within 0.0011 of the sine, and the sideways pull is undone with one step (not to convergence)
        assert!(worst < 0.05 * hs as f64, "worst difference {worst} for Hs {hs}");
        assert!(err_pulled < 0.5 * err_straight, "the copy follows the choppy sea: {err_pulled} against {err_straight} for the straight one");
    }
}
