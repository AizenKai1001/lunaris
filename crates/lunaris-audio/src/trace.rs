//! Sound travelling through a scene, found by tracing rays: how much of a sound gets through a wall, what the room does to it (how long
//! it rings, where the first echoes come from) and so on, from the same shapes the physics engine collides with.
//!
//! This is geometric acoustics, a model and not a wave solver, and each piece says what it assumes:
//!
//! * **Occluders** are boxes and spheres, each with an `absorption` (the share of the energy a reflection loses) and a `transmission` (the
//!   amplitude that survives a wall 0.2 m thick; thicker walls pass less, thinner ones count as 0.2 m).
//! * [`Scene::nearest`] and [`Scene::transmittance`] ask a bounding-volume hierarchy, so a scene of thousands of occluders costs a few
//!   microseconds a ray.
//! * [`analyse_room`] shoots rays from the listener into every direction, lets them bounce (mostly mirror-like, partly scattered) and loses
//!   the energy that leaves through an opening. The decay curve of what is left gives the reverberation time ([`Room::rt60`]) the way a
//!   room-acoustics program measures it, and the share of rays that never come back says how open the place is ([`Room::closedness`]).
//! * [`trace_source`] answers, for one sound source: how much of the direct sound arrives (several rays towards a small sphere around the
//!   source, so a corner gives a gradual change and not a click), and the first echoes: from the listener, rays go out, hit a surface, and
//!   from that point the source is looked at again. A surface scatters like a matte one, so an echo's loudness follows from the geometry
//!   (the patch a ray stands for, the angle, the distance to the source and what the surface absorbs) and the echoes are merged into a
//!   handful of taps with a delay and a direction each.
//!
//! Everything is deterministic: fixed ray directions and a fixed hash for the scatter, so the same scene gives the same numbers.

/// A point or direction.
pub type V3 = [f32; 3];

/// Speed of sound in air, metres per second.
pub const SPEED_OF_SOUND: f32 = 343.0;
/// A ray that finds nothing within this many metres has left the scene (an opening, the sky).
pub const OPEN_DISTANCE: f32 = 60.0;
/// The wall thickness (metres) at which `transmission` applies.
pub const REFERENCE_THICKNESS: f32 = 0.2;

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> V3 {
    let l = length(a);
    if l > 1e-12 {
        mul(a, 1.0 / l)
    } else {
        [0.0, 0.0, 0.0]
    }
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// An axis-aligned box.
    Box { center: V3, half: V3 },
    Sphere { center: V3, radius: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Occluder {
    pub shape: Shape,
    /// 0..1: the share of the energy a reflection off this surface loses.
    pub absorption: f32,
    /// 0..1: the amplitude that gets through a wall of the reference thickness.
    pub transmission: f32,
}

impl Shape {
    fn bounds(&self) -> (V3, V3) {
        match *self {
            Shape::Box { center, half } => (sub(center, half), add(center, half)),
            Shape::Sphere { center, radius } => (sub(center, [radius; 3]), add(center, [radius; 3])),
        }
    }

    /// Where a ray (unit `d`) is inside the shape: (enter, leave, outward normal where it enters). `enter` is negative when the ray starts inside.
    fn interval(&self, o: V3, d: V3) -> Option<(f32, f32, V3)> {
        match *self {
            Shape::Box { center, half } => {
                let (mut t0, mut t1) = (f32::NEG_INFINITY, f32::INFINITY);
                let mut n0 = [0.0; 3];
                for a in 0..3 {
                    let rel = o[a] - center[a];
                    if d[a].abs() < 1e-9 {
                        if rel.abs() > half[a] {
                            return None;
                        }
                        continue;
                    }
                    let (mut ta, mut tb) = ((-half[a] - rel) / d[a], (half[a] - rel) / d[a]);
                    let mut na = [0.0; 3];
                    na[a] = -1.0;
                    if ta > tb {
                        std::mem::swap(&mut ta, &mut tb);
                        na[a] = 1.0;
                    }
                    if ta > t0 {
                        t0 = ta;
                        n0 = na;
                    }
                    t1 = t1.min(tb);
                    if t0 > t1 {
                        return None;
                    }
                }
                if t1 < 0.0 || !t0.is_finite() {
                    return None;
                }
                Some((t0, t1, n0))
            }
            Shape::Sphere { center, radius } => {
                let m = sub(o, center);
                let b = dot(m, d);
                let c = dot(m, m) - radius * radius;
                let disc = b * b - c;
                if disc < 0.0 {
                    return None;
                }
                let s = disc.sqrt();
                let (t0, t1) = (-b - s, -b + s);
                if t1 < 0.0 {
                    return None;
                }
                Some((t0, t1, unit(add(m, mul(d, t0)))))
            }
        }
    }
}

struct Node {
    min: V3,
    max: V3,
    /// Inner node: children `left` and `left + 1`. Leaf (`count` > 0): `order[left..left + count]`.
    left: u32,
    count: u32,
}

/// A nearest-surface answer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// Index into the occluder list the scene was built from.
    pub index: usize,
    pub t: f32,
    /// Unit normal on the side the ray came from.
    pub normal: V3,
}

pub struct Scene {
    occluders: Vec<Occluder>,
    order: Vec<u32>,
    nodes: Vec<Node>,
}

const LEAF_SIZE: usize = 4;

impl Scene {
    pub fn new(occluders: Vec<Occluder>) -> Scene {
        let mut scene = Scene { order: (0..occluders.len() as u32).collect(), nodes: Vec::new(), occluders };
        if !scene.occluders.is_empty() {
            let n = scene.occluders.len();
            scene.nodes.push(Node { min: [0.0; 3], max: [0.0; 3], left: 0, count: 0 });
            scene.build_into(0, 0, n);
        }
        scene
    }

    pub fn len(&self) -> usize {
        self.occluders.len()
    }

    pub fn is_empty(&self) -> bool {
        self.occluders.is_empty()
    }

    pub fn occluder(&self, index: usize) -> &Occluder {
        &self.occluders[index]
    }

    fn range_bounds(&self, from: usize, to: usize) -> (V3, V3) {
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for &i in &self.order[from..to] {
            let (a, b) = self.occluders[i as usize].shape.bounds();
            for k in 0..3 {
                lo[k] = lo[k].min(a[k]);
                hi[k] = hi[k].max(b[k]);
            }
        }
        (lo, hi)
    }

    /// Build the node for `order[from..to]` into the reserved slot `slot` of `nodes`; the children of an inner node sit next to each other.
    fn build_into(&mut self, slot: usize, from: usize, to: usize) -> usize {
        let (min, max) = self.range_bounds(from, to);
        if to - from <= LEAF_SIZE {
            self.nodes[slot] = Node { min, max, left: from as u32, count: (to - from) as u32 };
            return slot;
        }
        let (mut clo, mut chi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for &i in &self.order[from..to] {
            let (a, b) = self.occluders[i as usize].shape.bounds();
            for k in 0..3 {
                let c = 0.5 * (a[k] + b[k]);
                clo[k] = clo[k].min(c);
                chi[k] = chi[k].max(c);
            }
        }
        let axis = (0..3).max_by(|&a, &b| (chi[a] - clo[a]).partial_cmp(&(chi[b] - clo[b])).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(0);
        let occ = &self.occluders;
        self.order[from..to].sort_by(|&a, &b| {
            let ca = occ[a as usize].shape.bounds();
            let cb = occ[b as usize].shape.bounds();
            (ca.0[axis] + ca.1[axis]).partial_cmp(&(cb.0[axis] + cb.1[axis])).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b))
        });
        let mid = from + (to - from) / 2;
        let left = self.nodes.len();
        self.nodes.push(Node { min, max, left: 0, count: 0 });
        self.nodes.push(Node { min, max, left: 0, count: 0 });
        self.build_into(left, from, mid);
        self.build_into(left + 1, mid, to);
        self.nodes[slot] = Node { min, max, left: left as u32, count: 0 };
        slot
    }

    /// Visit every occluder whose bounding box the segment `o + t d`, 0 <= t <= `max`, touches, nearest nodes first when `sorted`.
    fn visit(&self, o: V3, d: V3, max: f32, mut f: impl FnMut(usize) -> f32) {
        if self.nodes.is_empty() {
            return;
        }
        let inv = [1.0 / if d[0].abs() < 1e-20 { 1e-20 } else { d[0] }, 1.0 / if d[1].abs() < 1e-20 { 1e-20 } else { d[1] }, 1.0 / if d[2].abs() < 1e-20 { 1e-20 } else { d[2] }];
        let mut max = max;
        let mut stack = [0u32; 96];
        let mut sp = 1;
        stack[0] = 0;
        let enter = |n: &Node, max: f32| -> Option<f32> {
            let (mut t0, mut t1) = (0.0f32, max);
            for a in 0..3 {
                let (mut ta, mut tb) = ((n.min[a] - o[a]) * inv[a], (n.max[a] - o[a]) * inv[a]);
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                }
                t0 = t0.max(ta);
                t1 = t1.min(tb);
                if t0 > t1 {
                    return None;
                }
            }
            Some(t0)
        };
        while sp > 0 {
            sp -= 1;
            let n = &self.nodes[stack[sp] as usize];
            if enter(n, max).is_none() {
                continue;
            }
            if n.count > 0 {
                for k in 0..n.count as usize {
                    let idx = self.order[n.left as usize + k] as usize;
                    max = max.min(f(idx));
                }
            } else if sp + 2 <= stack.len() {
                stack[sp] = n.left;
                stack[sp + 1] = n.left + 1;
                sp += 2;
            }
        }
    }

    /// The first surface a ray enters, within `max` metres. `d` must be a unit vector. A ray that starts inside an occluder ignores it.
    pub fn nearest(&self, o: V3, d: V3, max: f32) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        let mut limit = max;
        self.visit(o, d, max, |i| {
            if let Some((t0, _t1, n)) = self.occluders[i].shape.interval(o, d) {
                if t0 > 1e-4 && t0 <= limit && best.is_none_or(|b| t0 < b.t || (t0 == b.t && i < b.index)) {
                    best = Some(Hit { index: i, t: t0, normal: n });
                    limit = t0;
                }
            }
            limit
        });
        best
    }

    /// The share of the amplitude that survives the straight way from `a` to `b`: every occluder it passes through multiplies it by
    /// its `transmission`, to the power of the chord's length over [`REFERENCE_THICKNESS`] (at least one).
    pub fn transmittance(&self, a: V3, b: V3) -> f32 {
        let seg = sub(b, a);
        let len = length(seg);
        if len < 1e-5 {
            return 1.0;
        }
        let d = mul(seg, 1.0 / len);
        let mut t = 1.0f32;
        self.visit(a, d, len, |i| {
            if let Some((t0, t1, _)) = self.occluders[i].shape.interval(a, d) {
                let (enter, leave) = (t0.max(0.0), t1.min(len));
                // grazing contact and a source lying on a surface do not count as passing through
                if leave - enter >= 0.0 && enter < len - 0.02 && leave > 0.02 && t1 >= t0 {
                    let chord = (leave - enter).max(REFERENCE_THICKNESS);
                    t *= self.occluders[i].transmission.clamp(0.0, 1.0).powf(chord / REFERENCE_THICKNESS);
                }
            }
            len
        });
        t
    }
}

/// `n` directions spread evenly over the sphere (a Fibonacci spiral); direction `i` is the same on every machine.
pub fn fibonacci_direction(i: usize, n: usize) -> V3 {
    let y = 1.0 - 2.0 * (i as f32 + 0.5) / n.max(1) as f32;
    let r = (1.0 - y * y).max(0.0).sqrt();
    let phi = i as f32 * 2.399_963_2;
    [r * phi.cos(), y, r * phi.sin()]
}

/// A small deterministic generator for the scatter of a bounce.
fn hash(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

fn unit_random(seed: u64) -> f32 {
    (hash(seed) >> 40) as f32 / (1u64 << 24) as f32
}

/// A direction on the hemisphere around `n` with a cosine weighting (a matte surface).
fn cosine_direction(n: V3, seed: u64) -> V3 {
    let (u1, u2) = (unit_random(seed), unit_random(seed ^ 0x9e37_79b9));
    let r = u1.sqrt();
    let phi = std::f32::consts::TAU * u2;
    let (x, y, z) = (r * phi.cos(), r * phi.sin(), (1.0 - u1).max(0.0).sqrt());
    let helper = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let t = unit(cross(helper, n));
    let b = cross(n, t);
    unit(add(add(mul(t, x), mul(b, y)), mul(n, z)))
}

/// What a listener's surroundings do to sound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    /// Seconds a sound takes to fall by 60 dB (0 when there is nothing around to reflect it).
    pub rt60: f32,
    /// 0..1: the share of the directions in which a wall, floor or object is within [`OPEN_DISTANCE`]. 1 = enclosed, about 0.5 = a floor under open sky.
    pub closedness: f32,
    /// Average distance, metres, between one bounce and the next.
    pub mean_free_path: f32,
    /// Average absorption of the surfaces the rays hit.
    pub mean_absorption: f32,
    /// Average distance to the first thing in each direction that has one.
    pub first_hit: f32,
}

impl Room {
    pub const NONE: Room = Room { rt60: 0.0, closedness: 0.0, mean_free_path: 0.0, mean_absorption: 0.0, first_hit: 0.0 };
}

/// Measure the room around `listener` with `rays` rays of up to `bounces` bounces each.
pub fn analyse_room(scene: &Scene, listener: V3, rays: usize, bounces: usize) -> Room {
    const BIN: f32 = 0.005;
    const BINS: usize = 1200; // up to 6 s
    if scene.is_empty() || rays == 0 {
        return Room::NONE;
    }
    let mut energy = vec![0.0f64; BINS];
    let (mut sum_free, mut n_free, mut sum_abs, mut n_abs) = (0.0f64, 0u64, 0.0f64, 0u64);
    let (mut closed, mut first_sum) = (0u64, 0.0f64);
    for i in 0..rays {
        let mut d = fibonacci_direction(i, rays);
        let mut o = listener;
        let (mut e, mut travelled) = (1.0f64, 0.0f32);
        for b in 0..bounces {
            let Some(h) = scene.nearest(o, d, OPEN_DISTANCE) else { break };
            travelled += h.t;
            sum_free += h.t as f64;
            n_free += 1;
            if b == 0 {
                closed += 1;
                first_sum += h.t as f64;
            }
            let bin = ((travelled / SPEED_OF_SOUND) / BIN) as usize;
            if bin >= BINS {
                break;
            }
            energy[bin] += e;
            let occ = scene.occluder(h.index);
            let alpha = occ.absorption.clamp(0.0, 1.0);
            sum_abs += alpha as f64;
            n_abs += 1;
            e *= 1.0 - alpha as f64;
            // a ray stops when it has lost 17 dB: the fit below only needs the curve down to -10 dB, and a room that absorbs little
            // (few dB a bounce) is followed for as many bounces as it takes, up to the caller's limit
            if e < 0.02 {
                break;
            }
            let p = add(o, mul(d, h.t));
            let n = if dot(h.normal, d) > 0.0 { mul(h.normal, -1.0) } else { h.normal };
            o = add(p, mul(n, 0.01));
            // a bounce is mostly a mirror, partly a matte scatter (a third of the rays scatter)
            let roll = hash((i as u64) << 8 | b as u64);
            d = if roll % 3 == 0 { cosine_direction(n, roll) } else { unit(sub(d, mul(n, 2.0 * dot(d, n)))) };
        }
    }
    let hits = n_free.max(1) as f64;
    let mut room = Room {
        rt60: 0.0,
        closedness: closed as f32 / rays as f32,
        mean_free_path: (sum_free / hits) as f32,
        mean_absorption: (sum_abs / n_abs.max(1) as f64) as f32,
        first_hit: (first_sum / closed.max(1) as f64) as f32,
    };
    // Schroeder backward integration of the energy-time curve; the slope of the first 5 dB (the early decay time, which is what is heard
    // when rooms are joined by doorways and the curve has two slopes: the fast one counts) scaled to 60 dB
    let mut edc = vec![0.0f64; BINS + 1];
    for k in (0..BINS).rev() {
        edc[k] = edc[k + 1] + energy[k];
    }
    if edc[0] > 0.0 {
        let db = |k: usize| 10.0 * (edc[k] / edc[0]).max(1e-12).log10();
        // the moment the curve crosses `level`, interpolated between two bins
        let cross = |level: f64| -> Option<f64> {
            (1..BINS).find(|&k| db(k) <= level).map(|k| {
                let (a, b) = (db(k - 1), db(k));
                ((k - 1) as f64 + if a > b { (a - level) / (a - b) } else { 0.0 }) * BIN as f64
            })
        };
        if let (Some(a), Some(b)) = (cross(-1.0), cross(-5.0)) {
            room.rt60 = ((b - a).max(0.0) * 60.0 / 4.0) as f32;
        }
    }
    room.rt60 = room.rt60.clamp(0.0, 6.0);
    room
}

/// One echo: when it arrives (seconds after the direct sound), how loud it is (1 = as loud as the direct sound heard at a metre) and from
/// which direction (unit vector from the listener) it seems to come.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reflection {
    pub delay: f32,
    pub gain: f32,
    pub dir: V3,
}

/// What reaches the listener from one source.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceTrace {
    /// 0..1: the amplitude of the direct sound that survives, averaged over rays to a small sphere around the source.
    pub transmittance: f32,
    /// 0..1: the share of those rays that met nothing at all.
    pub visible: f32,
    pub distance: f32,
    pub reflections: Vec<Reflection>,
}

/// The delays (milliseconds) at which echoes are merged into one tap each.
const TAP_EDGES_MS: [f32; 12] = [0.0, 5.0, 10.0, 15.0, 20.0, 30.0, 40.0, 60.0, 80.0, 120.0, 180.0, 250.0];

/// Trace the sound of a source at `source` to a listener at `listener`: the direct path (`source_radius` is the size of the sphere that
/// the occlusion rays aim at) and the echoes from `rays` rays shot out of the listener, each bouncing up to twice.
pub fn trace_source(scene: &Scene, listener: V3, source: V3, rays: usize, source_radius: f32) -> SourceTrace {
    let to_source = sub(source, listener);
    let distance = length(to_source);
    let r = source_radius.max(0.0);
    let aim = [source, add(source, [r, 0.0, 0.0]), add(source, [-r, 0.0, 0.0]), add(source, [0.0, r, 0.0]), add(source, [0.0, -r, 0.0]), add(source, [0.0, 0.0, r]), add(source, [0.0, 0.0, -r])];
    let samples = if r > 0.0 { &aim[..] } else { &aim[..1] };
    let (mut t_sum, mut clear) = (0.0f32, 0usize);
    for p in samples {
        let t = scene.transmittance(listener, *p);
        t_sum += t;
        if t > 0.999 {
            clear += 1;
        }
    }
    let mut out = SourceTrace { transmittance: t_sum / samples.len() as f32, visible: clear as f32 / samples.len() as f32, distance, reflections: Vec::new() };
    if scene.is_empty() || rays == 0 {
        return out;
    }
    // buckets of (power, delay-weighted sum, direction-weighted sum)
    let mut buckets = [(0.0f32, 0.0f32, [0.0f32; 3]); TAP_EDGES_MS.len()];
    let direct_delay = distance / SPEED_OF_SOUND;
    for i in 0..rays {
        let mut d = fibonacci_direction(i, rays);
        let first = d;
        let mut o = listener;
        let (mut len, mut rho) = (0.0f32, 1.0f32);
        for _bounce in 0..2 {
            let Some(h) = scene.nearest(o, d, OPEN_DISTANCE) else { break };
            len += h.t;
            let p = add(o, mul(d, h.t));
            let n = if dot(h.normal, d) > 0.0 { mul(h.normal, -1.0) } else { h.normal };
            rho *= 1.0 - scene.occluder(h.index).absorption.clamp(0.0, 1.0);
            let q = add(p, mul(n, 0.02));
            let to_s = sub(source, q);
            let ds = length(to_s);
            if ds > 1e-3 {
                let cos_s = dot(n, mul(to_s, 1.0 / ds));
                if cos_s > 0.02 {
                    let t = scene.transmittance(q, source);
                    if t > 0.01 {
                        // the loudness of a matte patch seen under the solid angle 4 pi / rays (see the module comment); `t` is an amplitude
                        let gain = 2.0 * (rho * cos_s / rays as f32).sqrt() / ds.max(0.5) * t;
                        let delay = ((len + ds) / SPEED_OF_SOUND - direct_delay).max(0.001);
                        let ms = delay * 1000.0;
                        if ms < TAP_EDGES_MS[TAP_EDGES_MS.len() - 1] {
                            let k = TAP_EDGES_MS.iter().rposition(|e| ms >= *e).unwrap_or(0);
                            let pw = gain * gain;
                            let b = &mut buckets[k];
                            b.0 += pw;
                            b.1 += pw * delay;
                            b.2 = add(b.2, mul(first, pw));
                        }
                    }
                }
            }
            o = q;
            d = unit(sub(d, mul(n, 2.0 * dot(d, n))));
        }
    }
    for (power, delay_sum, dir_sum) in buckets {
        if power > 1e-12 {
            // the direction of the mean arrival; a spread of directions averages to nearly nothing, which is centred and diffuse
            out.reflections.push(Reflection { delay: delay_sum / power, gain: power.sqrt(), dir: mul(dir_sum, 1.0 / power) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(center: V3, half: V3, absorption: f32, transmission: f32) -> Occluder {
        Occluder { shape: Shape::Box { center, half }, absorption, transmission }
    }

    /// A closed room `w` x `h` x `l` metres centred on the origin, made of six 0.2 m slabs outside it.
    fn room(w: f32, h: f32, l: f32, absorption: f32) -> Vec<Occluder> {
        let (hw, hh, hl, t) = (w / 2.0, h / 2.0, l / 2.0, 0.1);
        vec![
            wall([0.0, -hh - t, 0.0], [hw + 0.2, t, hl + 0.2], absorption, 0.02),
            wall([0.0, hh + t, 0.0], [hw + 0.2, t, hl + 0.2], absorption, 0.02),
            wall([-hw - t, 0.0, 0.0], [t, hh, hl], absorption, 0.02),
            wall([hw + t, 0.0, 0.0], [t, hh, hl], absorption, 0.02),
            wall([0.0, 0.0, -hl - t], [hw, hh, t], absorption, 0.02),
            wall([0.0, 0.0, hl + t], [hw, hh, t], absorption, 0.02),
        ]
    }

    /// A deterministic scatter of boxes and spheres.
    fn clutter(n: usize) -> Vec<Occluder> {
        let f = |k: u64| unit_random(k) * 2.0 - 1.0;
        (0..n as u64)
            .map(|i| {
                let c = [f(i * 7) * 40.0, f(i * 7 + 1) * 10.0, f(i * 7 + 2) * 40.0];
                if i % 3 == 0 {
                    Occluder { shape: Shape::Sphere { center: c, radius: 0.3 + unit_random(i * 11) * 2.0 }, absorption: 0.2, transmission: 0.1 }
                } else {
                    wall(c, [0.2 + unit_random(i * 13) * 2.0, 0.2 + unit_random(i * 17) * 2.0, 0.2 + unit_random(i * 19) * 2.0], 0.2, 0.1)
                }
            })
            .collect()
    }

    #[test]
    fn the_hierarchy_finds_exactly_what_a_linear_scan_finds() {
        let occ = clutter(400);
        let scene = Scene::new(occ.clone());
        for k in 0..600u64 {
            let o = [unit_random(k * 3) * 60.0 - 30.0, unit_random(k * 3 + 1) * 14.0 - 7.0, unit_random(k * 3 + 2) * 60.0 - 30.0];
            let d = fibonacci_direction(k as usize, 600);
            let mut want: Option<(usize, f32)> = None;
            for (i, oc) in occ.iter().enumerate() {
                if let Some((t0, _, _)) = oc.shape.interval(o, d) {
                    if t0 > 1e-4 && t0 <= OPEN_DISTANCE && want.is_none_or(|(_, bt)| t0 < bt) {
                        want = Some((i, t0));
                    }
                }
            }
            let got = scene.nearest(o, d, OPEN_DISTANCE).map(|h| (h.index, h.t));
            assert_eq!(got.map(|g| g.0), want.map(|w| w.0), "ray {k}");
            if let (Some(g), Some(w)) = (got, want) {
                assert!((g.1 - w.1).abs() < 1e-3);
            }
            // the same for the product along a segment
            let b = add(o, mul(d, 25.0));
            let mut prod = 1.0f32;
            let len = 25.0;
            for oc in &occ {
                if let Some((t0, t1, _)) = oc.shape.interval(o, d) {
                    let (enter, leave) = (t0.max(0.0), t1.min(len));
                    if leave - enter >= 0.0 && enter < len - 0.02 && leave > 0.02 && t1 >= t0 {
                        prod *= oc.transmission.powf((leave - enter).max(REFERENCE_THICKNESS) / REFERENCE_THICKNESS);
                    }
                }
            }
            let t = scene.transmittance(o, b);
            assert!((t - prod).abs() <= 1e-5 + prod * 1e-4, "ray {k}: {t} vs {prod}");
        }
    }

    #[test]
    fn rays_and_walls_behave() {
        let s = Scene::new(vec![wall([0.0, 0.0, -5.0], [3.0, 3.0, 0.1], 0.1, 0.05), Occluder { shape: Shape::Sphere { center: [0.0, 0.0, 5.0], radius: 1.0 }, absorption: 0.3, transmission: 0.5 }]);
        let h = s.nearest([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], 100.0).unwrap();
        assert!((h.t - 4.9).abs() < 1e-4 && h.normal == [0.0, 0.0, 1.0], "{h:?}");
        let h = s.nearest([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 100.0).unwrap();
        assert!((h.t - 4.0).abs() < 1e-4 && (h.normal[2] + 1.0).abs() < 1e-4);
        assert!(s.nearest([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], 4.0).is_none(), "too far");
        assert!(s.nearest([0.0, 0.0, -5.0], [0.0, 0.0, -1.0], 100.0).is_none(), "a ray that starts inside a wall ignores it");
        // a 0.2 m wall passes its transmission, a thicker one less, an open line everything
        assert!((s.transmittance([0.0, 0.0, 0.0], [0.0, 0.0, -9.0]) - 0.05).abs() < 1e-3);
        assert_eq!(s.transmittance([0.0, 0.0, 0.0], [5.0, 0.0, 0.0]), 1.0);
        let thick = Scene::new(vec![wall([0.0, 0.0, 0.0], [0.4, 3.0, 3.0], 0.1, 0.1)]);
        assert!((thick.transmittance([-2.0, 0.0, 0.0], [2.0, 0.0, 0.0]) - 0.1f32.powf(4.0)).abs() < 1e-6, "0.8 m = four reference thicknesses");
        // a source lying on a surface is not behind it; the sphere is half-transparent
        let floor = Scene::new(vec![wall([0.0, -0.5, 0.0], [10.0, 0.5, 10.0], 0.1, 0.0)]);
        assert_eq!(floor.transmittance([0.0, 1.7, 0.0], [3.0, 0.0, 0.0]), 1.0);
        assert!((s.transmittance([0.0, 0.0, 0.0], [0.0, 0.0, 9.0]) - 0.5f32.powf(10.0)).abs() < 1e-3, "2 m of sphere is ten reference thicknesses");
        assert!(Scene::new(Vec::new()).nearest([0.0; 3], [1.0, 0.0, 0.0], 10.0).is_none());
        assert_eq!(Scene::new(Vec::new()).transmittance([0.0; 3], [9.0, 0.0, 0.0]), 1.0);
    }

    #[test]
    fn a_closed_room_rings_about_as_long_as_the_textbook_says_and_the_open_air_does_not() {
        // Eyring: 0.1611 V / (-S ln(1 - a)); 10 x 4 x 10 m, a = 0.1 gives 1.70 s
        let closed = analyse_room(&Scene::new(room(10.0, 4.0, 10.0, 0.1)), [0.0, 0.0, 0.0], 256, 64);
        assert!((1.2..2.4).contains(&closed.rt60), "{closed:?}");
        assert!(closed.closedness > 0.99, "{closed:?}");
        assert!((closed.mean_absorption - 0.1).abs() < 0.01);
        let floor_only = analyse_room(&Scene::new(vec![wall([0.0, -0.5, 0.0], [200.0, 0.5, 200.0], 0.1, 0.0)]), [0.0, 1.7, 0.0], 256, 64);
        assert!(floor_only.rt60 < 0.3, "{floor_only:?}");
        assert!((0.35..0.65).contains(&floor_only.closedness), "{floor_only:?}");
        assert_eq!(analyse_room(&Scene::new(Vec::new()), [0.0; 3], 100, 10), Room::NONE);
        // softer walls ring shorter, a bigger room rings longer, a doorway lets some of it out
        let soft = analyse_room(&Scene::new(room(10.0, 4.0, 10.0, 0.4)), [0.0; 3], 256, 64);
        assert!(soft.rt60 < closed.rt60 * 0.6, "{} vs {}", soft.rt60, closed.rt60);
        let big = analyse_room(&Scene::new(room(30.0, 12.0, 30.0, 0.1)), [0.0; 3], 256, 64);
        assert!(big.rt60 > closed.rt60 * 1.5, "{} vs {}", big.rt60, closed.rt60);
        let mut with_hole = room(10.0, 4.0, 10.0, 0.1);
        with_hole.remove(3); // no wall on +x
        let holey = analyse_room(&Scene::new(with_hole), [0.0; 3], 256, 64);
        assert!(holey.rt60 < closed.rt60 * 0.85 && holey.closedness < 0.95, "{holey:?}");
        // the measurement is repeatable
        assert_eq!(closed, analyse_room(&Scene::new(room(10.0, 4.0, 10.0, 0.1)), [0.0, 0.0, 0.0], 256, 64));
    }

    #[test]
    fn a_wall_between_listener_and_source_muffles_it_and_a_room_makes_echoes() {
        let mut occ = room(10.0, 4.0, 10.0, 0.1);
        let open = trace_source(&Scene::new(occ.clone()), [-3.0, 0.0, 0.0], [3.0, 0.0, 0.0], 96, 0.3);
        assert_eq!((open.transmittance, open.visible), (1.0, 1.0));
        assert!((open.distance - 6.0).abs() < 1e-4);
        assert!(!open.reflections.is_empty() && open.reflections.iter().all(|r| r.delay > 0.0 && r.delay < 0.26 && r.gain > 0.0), "{:?}", open.reflections);
        // the echoes are louder in a room that reflects more
        let lively = trace_source(&Scene::new(room(10.0, 4.0, 10.0, 0.02)), [-3.0, 0.0, 0.0], [3.0, 0.0, 0.0], 96, 0.3);
        let dull = trace_source(&Scene::new(room(10.0, 4.0, 10.0, 0.7)), [-3.0, 0.0, 0.0], [3.0, 0.0, 0.0], 96, 0.3);
        let power = |t: &SourceTrace| t.reflections.iter().map(|r| r.gain * r.gain).sum::<f32>();
        assert!(power(&lively) > power(&dull) * 2.0, "{} vs {}", power(&lively), power(&dull));
        // a partition across the room
        occ.push(wall([0.0, 0.0, 0.0], [0.1, 2.0, 5.0], 0.1, 0.03));
        let blocked = trace_source(&Scene::new(occ), [-3.0, 0.0, 0.0], [3.0, 0.0, 0.0], 96, 0.3);
        assert!(blocked.transmittance < 0.05 && blocked.visible == 0.0, "{blocked:?}");
        // an edge: only some of the rays to the small sphere are blocked, so the change is gradual
        let post = Scene::new(vec![wall([0.0, 0.0, 0.0], [0.1, 2.0, 0.05], 0.1, 0.0)]);
        let edge = trace_source(&post, [-3.0, 0.0, 0.0], [3.0, 0.0, 0.05], 0, 0.3);
        assert!(edge.transmittance > 0.1 && edge.transmittance < 0.95, "{edge:?}");
    }

    #[test]
    fn echoes_come_from_the_side_of_the_wall_they_bounce_off() {
        // a long room: listener and source both near the -z end, so the echo off the -z wall is close behind them and from -z
        let scene = Scene::new(room(8.0, 4.0, 40.0, 0.05));
        let t = trace_source(&scene, [0.0, 0.0, -15.0], [0.0, 0.0, -10.0], 192, 0.3);
        let first = t.reflections.first().expect("an echo");
        assert!(first.delay < 0.05, "{first:?}");
        let total: f32 = t.reflections.iter().map(|r| r.gain * r.gain).sum();
        assert!(total > 0.0);
    }
}
