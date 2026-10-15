//! Raycast vehicles (docs/PHYSICS.md): a car, a truck or a buggy that is one rigid body (the chassis: `RigidBody`, `Collider`, `Velocity`,
//! `AngularVelocity`) and a few rays. Each wheel is a ray down the chassis's own -y from its mount point; where the ray meets the ground the wheel
//! stands, and the engine pushes the chassis there with
//!
//! * a **suspension**: a spring and a damper along the ground's normal (explicit forces, so a car at rest sits where `spring * compression = its
//!   share of the weight`; the damper is limited so that it can never reverse the speed it damps, and the suspension bottoms out against a stop),
//!   with an anti-roll bar between the two wheels of an axle;
//! * **tyre friction** in the plane of the ground: along the wheel (the engine's drive, the brakes, a little rolling resistance) and across it (the
//!   grip that makes the car follow its wheels), solved as sequential impulses at the contact points with the friction circle `mu * load`: ask for
//!   more than that and the wheel slides (power oversteer, locked brakes, handbrake drifts all fall out of that one rule);
//! * a little air: drag, and a downforce that grows with the square of the speed.
//!
//! The rest is the rigid-body physics of the chassis (it falls, flips, lands on its roof, bumps into walls). The step runs right after the rigid
//! bodies of the tick, reads and writes only the vehicle's own `Velocity` and `AngularVelocity`, and visits vehicles in uid order, so it is
//! deterministic for any thread count. Everything is f32 with `+ - * / sqrt` (the steering angle goes through `detmath`). A world without a
//! `Vehicle` pays for one query that finds nothing.
//!
//! Directions: the chassis's local **-z is forward** (like a camera and a sensor), +x right, +y up. A wheel table is text, one wheel a line:
//!
//! ```text
//! # name: x y z  key=value ...        (x y z: the mount point, in the chassis's own coordinates; the ray starts there and goes down)
//! defaults: radius=0.35 rest=0.3 travel=0.2 grip=1.2
//! fl: -0.8 -0.15 -1.3 steer=1 brake=1
//! rr: 0.8 -0.15 1.3 drive=1 brake=0.7 handbrake=1
//! ```
use crate::components::{AngularVelocity, Collider, LocalTransform, Parent, RigidBody, Transform, Vehicle, Velocity};
use crate::detmath;
use crate::layers::{Filter, Layers};
use crate::meshcol::MeshStore;
use crate::physics::Shape;
use crate::physics_rot::{add, axes_of, cross, dot, inv_inertia, mul, neg, normalize_q, quat_mul, rotate, sub, M3};
use crate::terrain::Terrains;
use crate::vehicle_ray::{segment_box, RayWorld};
use lunaris_ecs::{Entity, World};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

type V3 = [f32; 3];

/// A vehicle has at most this many wheels (the size of the output arrays of `Vehicle`).
pub const MAX_WHEELS: usize = 8;

/// The friction solver goes over the wheels this many times.
const FRICTION_ROUNDS: usize = 8;

// ------------------------------------------------------------------------------------------------ the wheel table

/// One wheel of a vehicle, as the text of `Vehicle.wheels` describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Wheel {
    pub name: String,
    /// Where the strut is fixed to the chassis (its own coordinates, relative to the chassis's `Transform`); the ray starts here and goes down.
    pub pos: V3,
    pub radius: f32,
    /// The length of the suspension at which the spring pushes with no force, which is also its longest (the wheel hangs there in the air): the
    /// distance from the mount point to the wheel's centre.
    pub rest: f32,
    /// How far the suspension can be pressed in from `rest` before it bottoms out.
    pub travel: f32,
    /// Newtons per metre; `None` (omitted or `auto`): the car's share of the weight over a quarter of the travel.
    pub spring: Option<f32>,
    /// Newton-seconds per metre; `None`: a damping ratio of 0.6.
    pub damper: Option<f32>,
    /// How much of the steering angle this wheel takes (1 front, 0 fixed, negative turns the other way: rear-wheel steering).
    pub steer: f32,
    /// The share of the engine this wheel gets (the shares of all wheels are scaled to add up to 1; the wheels that are off the ground lose theirs).
    pub drive: f32,
    /// The share of the brakes.
    pub brake: f32,
    /// The share of the handbrake (locks the wheel and lets go of its grip).
    pub handbrake: f32,
    /// The friction coefficient of the tyre: the wheel can push sideways and along the ground with at most `grip` times the force that presses it down.
    pub grip: f32,
    /// An entity whose pose the engine sets (the wheel's model: a child of the vehicle's entity, or a free entity), 0 for none.
    pub entity: u64,
}

impl Default for Wheel {
    fn default() -> Wheel {
        Wheel { name: String::new(), pos: [0.0; 3], radius: 0.35, rest: 0.3, travel: 0.2, spring: None, damper: None, steer: 0.0, drive: 0.0, brake: 1.0, handbrake: 0.0, grip: 1.2, entity: 0 }
    }
}

impl Wheel {
    /// The wheel with its numbers brought into a range the solver can work with (`check` warns about the ones that were outside it).
    fn sane(&self) -> Wheel {
        let mut w = self.clone();
        let fin = |v: f32, d: f32| if v.is_finite() { v } else { d };
        w.pos = [fin(w.pos[0], 0.0), fin(w.pos[1], 0.0), fin(w.pos[2], 0.0)];
        w.radius = fin(w.radius, 0.35).clamp(0.02, 5.0);
        w.rest = fin(w.rest, 0.3).clamp(0.01, 5.0);
        w.travel = fin(w.travel, 0.2).clamp(0.0, w.rest);
        w.spring = w.spring.filter(|s| s.is_finite() && *s > 0.0).map(|s| s.min(1.0e8));
        w.damper = w.damper.filter(|s| s.is_finite() && *s >= 0.0).map(|s| s.min(1.0e7));
        w.steer = fin(w.steer, 0.0).clamp(-4.0, 4.0);
        w.drive = fin(w.drive, 0.0).clamp(0.0, 1.0e3);
        w.brake = fin(w.brake, 0.0).clamp(0.0, 1.0e3);
        w.handbrake = fin(w.handbrake, 0.0).clamp(0.0, 1.0e3);
        w.grip = fin(w.grip, 1.0).clamp(0.0, 10.0);
        w
    }
}

fn set_key(w: &mut Wheel, key: &str, value: &str) -> Result<(), String> {
    let num = || value.parse::<f32>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("`{key}={value}`: the value must be a number"));
    match key {
        "radius" | "r" => w.radius = num()?,
        "rest" => w.rest = num()?,
        "travel" => w.travel = num()?,
        "spring" | "k" => w.spring = if value == "auto" { None } else { Some(num()?) },
        "damper" | "c" => w.damper = if value == "auto" { None } else { Some(num()?) },
        "steer" => w.steer = num()?,
        "drive" => w.drive = num()?,
        "brake" => w.brake = num()?,
        "handbrake" | "hb" => w.handbrake = num()?,
        "grip" | "mu" => w.grip = num()?,
        "entity" | "model" => {
            let v = num()?;
            if v < 0.0 || v.fract() != 0.0 {
                return Err(format!("`{key}={value}`: the value must be the uid of an entity (a whole number)"));
            }
            w.entity = v as u64;
        }
        other => return Err(format!("unknown key `{other}` (radius, rest, travel, spring, damper, steer, drive, brake, handbrake, grip, entity)")),
    }
    Ok(())
}

/// The wheels of a `Vehicle.wheels` text: one a line (`;` also ends a line, `#` starts a comment), `name: x y z key=value ...`, and a line
/// `defaults: key=value ...` that sets the values of the wheels after it. At most [`MAX_WHEELS`]. The error says which line and what is wrong.
pub fn parse_wheels(text: &str) -> Result<Vec<Wheel>, String> {
    let mut defaults = Wheel::default();
    let mut out: Vec<Wheel> = Vec::new();
    for (n, raw) in text.split(['\n', ';']).enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let at = |m: String| format!("line {}: {m}", n + 1);
        let (name, rest) = line.split_once(':').ok_or_else(|| at("expected `name: x y z key=value ...`".into()))?;
        let name = name.trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(at(format!("`{name}` is not a name (one word before the colon)")));
        }
        let mut tokens = rest.split_whitespace().peekable();
        if name == "defaults" {
            for t in tokens {
                let (k, v) = t.split_once('=').ok_or_else(|| at(format!("`{t}`: defaults take `key=value` pairs only")))?;
                set_key(&mut defaults, k, v).map_err(at)?;
            }
            continue;
        }
        if out.len() >= MAX_WHEELS {
            return Err(at(format!("a vehicle has at most {MAX_WHEELS} wheels")));
        }
        if out.iter().any(|w| w.name == name) {
            return Err(at(format!("there is already a wheel called `{name}`")));
        }
        let mut w = Wheel { name: name.to_string(), ..defaults.clone() };
        for axis in 0..3 {
            let t = tokens.next().ok_or_else(|| at(format!("`{name}` needs its mount point: three numbers x y z in the chassis's own coordinates")))?;
            w.pos[axis] = t.parse::<f32>().ok().filter(|v| v.is_finite()).ok_or_else(|| at(format!("`{t}` is not a number (the mount point of `{name}` is three numbers x y z)")))?;
        }
        for t in tokens {
            let (k, v) = t.split_once('=').ok_or_else(|| at(format!("`{t}`: expected `key=value` after the mount point")))?;
            set_key(&mut w, k, v).map_err(at)?;
        }
        out.push(w);
    }
    Ok(out)
}

thread_local! {
    /// The tables that have been parsed, by their text: a wheel table is read once, not every tick (a pure function of the text, so nothing here is state).
    static TABLES: RefCell<HashMap<String, Result<Arc<Vec<Wheel>>, String>>> = RefCell::new(HashMap::new());
    /// How many times the vehicle step has found a vehicle on this thread (tests read it to see that a world without one never gets here).
    static WORK: Cell<u64> = const { Cell::new(0) };
}

/// The parsed wheels of a text (cached), with their numbers brought into the range the solver works in (`check` reads the raw ones with [`parse_wheels`]).
pub fn wheels_of(text: &str) -> Result<Arc<Vec<Wheel>>, String> {
    TABLES.with(|t| {
        let mut t = t.borrow_mut();
        if let Some(r) = t.get(text) {
            return r.clone();
        }
        if t.len() >= 256 {
            t.clear();
        }
        let r = parse_wheels(text).map(|v| Arc::new(v.iter().map(Wheel::sane).collect::<Vec<Wheel>>()));
        t.insert(text.to_string(), r.clone());
        r
    })
}

/// How many ticks (on this thread) have run the vehicle code with at least one vehicle in the world.
pub fn work_done() -> u64 {
    WORK.with(|w| w.get())
}

// ------------------------------------------------------------------------------------------------ the chassis as a rigid body

struct Chassis {
    pos: V3,
    axes: M3,
    v: V3,
    w: V3,
    inv_mass: f32,
    inv_i: V3,
}

impl Chassis {
    /// The inverse inertia tensor in the world, applied to `x`.
    fn inv_i_apply(&self, x: V3) -> V3 {
        let (s0, s1, s2) = (self.inv_i[0] * dot(self.axes[0], x), self.inv_i[1] * dot(self.axes[1], x), self.inv_i[2] * dot(self.axes[2], x));
        add(add(mul(self.axes[0], s0), mul(self.axes[1], s1)), mul(self.axes[2], s2))
    }

    /// The velocity of the point `r` from the centre of mass.
    fn at(&self, r: V3) -> V3 {
        add(self.v, cross(self.w, r))
    }

    /// How much a unit impulse along `n` at `r` changes the speed of that point along `n`.
    fn inv_eff(&self, r: V3, n: V3) -> f32 {
        let rn = cross(r, n);
        self.inv_mass + dot(rn, self.inv_i_apply(rn))
    }

    /// An impulse `j` at `r`.
    fn push(&mut self, j: V3, r: V3) {
        self.v = add(self.v, mul(j, self.inv_mass));
        self.w = add(self.w, self.inv_i_apply(cross(r, j)));
    }
}

/// One wheel while the tick is worked out.
#[derive(Clone, Copy, Default)]
struct Grip {
    grounded: bool,
    /// Where the wheel touches (from the centre of mass), the ground's normal, the compression of the suspension (0 to the travel), and how far past
    /// the stop it was pushed.
    r: V3,
    n: V3,
    x: f32,
    pen: f32,
    /// The force of the suspension at the end of its work (what presses the tyre down).
    load: f32,
    /// The steering angle of this wheel (radians) and the directions along and across it in the ground's plane.
    angle: f32,
    f: V3,
    l: V3,
    /// The lever of the friction impulses (the contact point with its height scaled by `roll_influence`).
    rf: V3,
    kf: f32,
    kl: f32,
    /// The impulses along and across the wheel so far this tick, and the ones the drive and the brakes ask for.
    jf: f32,
    jl: f32,
    jb: f32,
    jd: f32,
    brake_budget: f32,
    limit: f32,
}

/// Everything the tick found out, to be written back.
struct Outcome {
    v: V3,
    w: V3,
    speed: f32,
    steering: f32,
    contact: u32,
    on_ground: u32,
    compression: [f32; MAX_WHEELS],
    spin: [f32; MAX_WHEELS],
    slip: [f32; MAX_WHEELS],
    /// For the wheel models: the length of the suspension and the steering angle of each wheel.
    length: [f32; MAX_WHEELS],
    angle: [f32; MAX_WHEELS],
}

const DEG: f32 = std::f32::consts::PI / 180.0;

fn finite_or(x: f32, d: f32) -> f32 {
    if x.is_finite() {
        x
    } else {
        d
    }
}

fn unit_or(a: V3, fallback: V3) -> V3 {
    let l2 = dot(a, a);
    if l2 > 1.0e-18 {
        let l = l2.sqrt();
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        fallback
    }
}

/// The engine's push at this forward speed: full up to where the power runs out, then falling as `power / speed`, and fading to nothing at the top speed.
fn engine_push(v: &Vehicle, speed: f32, reverse: bool) -> f32 {
    let (force, top) = if reverse { (0.6 * v.engine_force, v.reverse_speed) } else { (v.engine_force, v.top_speed) };
    let mut f = finite_or(force, 0.0).max(0.0);
    if !reverse && v.power > 0.0 && speed > 1.0e-3 {
        f = f.min(v.power / speed);
    }
    if top > 0.0 {
        let fade = if reverse { 0.25 } else { 0.1 };
        f *= ((top - speed) / (fade * top)).clamp(0.0, 1.0);
    }
    f
}

struct Rig {
    uid: u64,
    entity: Entity,
    v: Vehicle,
    t: Transform,
    vel: V3,
    w: V3,
    mass: f32,
    gravity_scale: f32,
    shape: Shape,
    wheels: Arc<Vec<Wheel>>,
}

/// One tick of one vehicle.
fn drive(rig: &Rig, ray: &RayWorld, who: Filter, gravity: V3, dt: f32) -> Outcome {
    let v = &rig.v;
    let axes = axes_of(normalize_q(rig.t.rot));
    let (right, up, fwd) = (axes[0], axes[1], neg(axes[2]));
    let mut ch = Chassis { pos: rig.t.pos, axes, v: rig.vel, w: rig.w, inv_mass: 1.0 / rig.mass, inv_i: inv_inertia(&rig.shape, rig.mass) };
    let wheels: &[Wheel] = &rig.wheels;
    let n = wheels.len();
    let mut out = Outcome { v: ch.v, w: ch.w, speed: 0.0, steering: v.steering, contact: 0, on_ground: 0, compression: [0.0; MAX_WHEELS], spin: v.spin, slip: [0.0; MAX_WHEELS], length: [0.0; MAX_WHEELS], angle: [0.0; MAX_WHEELS] };
    for (i, w) in wheels.iter().enumerate() {
        out.length[i] = w.rest;
    }

    // ---- what the driver asked for (made safe)
    let clean = |x: f32, lo: f32, hi: f32| if x.is_finite() { x.clamp(lo, hi) } else { 0.0 };
    let (throttle, steer_in, brake_in, hb_in) = (clean(v.throttle, -1.0, 1.0), clean(v.steer, -1.0, 1.0), clean(v.brake, 0.0, 1.0), clean(v.handbrake, 0.0, 1.0));
    let speed0 = dot(ch.v, fwd);

    // ---- the steering: toward what was asked at the rate of the wheel, asking for less the faster the car goes
    let fade = finite_or(v.steer_fade, 1.0).clamp(0.0, 1.0);
    let fade_at = finite_or(v.steer_fade_speed, 40.0).max(0.1);
    let limit = finite_or(v.steer_max, 32.0).clamp(0.0, 89.0) * (1.0 + (fade - 1.0) * (speed0.abs() / fade_at).min(1.0));
    let target = steer_in * limit;
    let rate = finite_or(v.steer_rate, 180.0);
    let steering = if rate > 0.0 {
        let step = rate * dt;
        finite_or(v.steering, 0.0) + (target - finite_or(v.steering, 0.0)).clamp(-step, step)
    } else {
        target
    };
    out.steering = steering;

    // ---- the air: drag, and downforce pushing the chassis down
    let speed_abs = dot(ch.v, ch.v).sqrt();
    let drag = finite_or(v.air_drag, 0.0).max(0.0) * speed_abs * dt * ch.inv_mass;
    if drag > 0.0 {
        ch.v = mul(ch.v, 1.0 - drag.min(0.5));
    }
    let down = finite_or(v.downforce, 0.0).max(0.0) * speed_abs * speed_abs * dt;
    if down > 0.0 {
        ch.v = add(ch.v, mul(up, -down * ch.inv_mass));
    }

    // ---- the rays: where each wheel stands
    // gravity has already changed the velocity this tick (the rigid bodies ran first): the damper must not see that as the wheel pressing in, or a car at
    // rest would be held up by its damper as well as its springs and sit lower than the springs say
    let gdt = if gravity.iter().all(|x| x.is_finite()) { mul(gravity, rig.gravity_scale * dt) } else { [0.0; 3] };
    let g = dot(gravity, gravity).sqrt();
    let g = if g.is_finite() { g } else { 9.8 };
    let mut gs = [Grip::default(); MAX_WHEELS];
    for (i, w) in wheels.iter().enumerate() {
        out.angle[i] = steering * DEG * w.steer; // (for the model of the wheel, in the air too)
        let mount = add(ch.pos, rotate(&ch.axes, w.pos));
        let reach = w.rest + w.radius;
        let hit = ray.cast(mount, neg(up), reach, who, rig.uid).filter(|h| dot(h.normal, up) > 0.25);
        if let Some(h) = hit {
            let stand = h.t - w.radius; // the length of the suspension
            let squeeze = (w.rest - stand).max(0.0);
            let p = add(mount, mul(neg(up), h.t));
            gs[i] = Grip { grounded: true, r: sub(p, ch.pos), n: h.normal, x: squeeze.min(w.travel), pen: (squeeze - w.travel).max(0.0), ..Grip::default() };
            out.length[i] = stand.clamp(w.rest - w.travel, w.rest);
            out.compression[i] = gs[i].x;
            out.contact |= 1 << i;
            out.on_ground += 1;
        }
    }

    // ---- the suspension: a spring and a damper along the normal, an anti-roll bar between the wheels of an axle
    let weight_share = rig.mass * g / n.max(1) as f32;
    let mut force = [0.0f32; MAX_WHEELS];
    for (i, w) in wheels.iter().enumerate() {
        let s = gs[i];
        if !s.grounded {
            continue;
        }
        let k = w.spring.unwrap_or_else(|| weight_share / (0.25 * w.travel.max(0.02)));
        let c = w.damper.unwrap_or_else(|| 2.0 * 0.6 * (k * rig.mass / n.max(1) as f32).sqrt());
        let approach = -dot(sub(ch.at(s.r), gdt), s.n); // how fast the spring is being pressed in
        let m_eff = 1.0 / ch.inv_eff(s.r, s.n).max(1.0e-9);
        let cap = 0.8 * m_eff * approach.abs() / dt;
        force[i] = (k * s.x + (c * approach).clamp(-cap, cap)).max(0.0);
    }
    let arb = finite_or(v.anti_roll, 0.0).max(0.0);
    if arb > 0.0 {
        let mut extra = [0.0f32; MAX_WHEELS];
        for (i, w) in wheels.iter().enumerate() {
            if !gs[i].grounded {
                continue;
            }
            // the other wheel of the axle: the one at the same distance along the car and on the other side
            let partner = wheels.iter().enumerate().find(|(j, o)| *j != i && (o.pos[2] - w.pos[2]).abs() < 0.05 && o.pos[0] * w.pos[0] < 0.0);
            if let Some((j, _)) = partner {
                extra[i] = arb * (gs[i].x - gs[j].x);
            }
        }
        for i in 0..n {
            force[i] = (force[i] + extra[i]).max(0.0);
        }
    }
    let n_max = rig.mass * 60.0;
    for i in 0..n {
        if gs[i].grounded {
            force[i] = force[i].min(n_max);
            let j = mul(gs[i].n, force[i] * dt);
            ch.push(j, gs[i].r);
            gs[i].load = force[i];
        }
    }
    // the stops: a wheel pressed past its travel is held back at once
    for i in 0..n {
        let s = gs[i];
        if s.grounded && s.pen > 0.0 {
            let need = (-dot(ch.at(s.r), s.n)).max(0.0) + 0.2 * s.pen / dt;
            let jn = need / ch.inv_eff(s.r, s.n).max(1.0e-9);
            ch.push(mul(s.n, jn), s.r);
            gs[i].load += jn / dt;
        }
    }

    // ---- what the engine and the brakes want of each wheel
    let sum_of = |f: &dyn Fn(&Wheel) -> f32| wheels.iter().map(f).sum::<f32>();
    let (sum_drive, sum_brake, sum_hb) = (sum_of(&|w| w.drive), sum_of(&|w| w.brake), sum_of(&|w| w.handbrake));
    let mut extra_brake = 0.0f32;
    let drive_total = if throttle > 0.0 {
        if speed0 < -1.0 {
            extra_brake = throttle; // pressing forward while rolling backwards: that is braking first
            0.0
        } else {
            throttle * engine_push(v, speed0.max(0.0), false)
        }
    } else if throttle < 0.0 {
        if speed0 > 1.0 {
            extra_brake = -throttle;
            0.0
        } else {
            throttle * engine_push(v, (-speed0).max(0.0), true)
        }
    } else {
        0.0
    };
    // with no foot on anything a light brake holds the car below walking pace (a car on a slope stays where it is; a coasting car comes to rest)
    let idle = if throttle.abs() < 1.0e-3 && brake_in == 0.0 && hb_in == 0.0 { 0.25 * (1.0 - speed_abs / 2.0).clamp(0.0, 1.0) } else { 0.0 };
    let brake_in = brake_in.max(extra_brake).max(idle);
    let brake_force = finite_or(v.brake_force, 0.0).max(0.0);
    let hb_force = finite_or(v.handbrake_force, 0.0).max(0.0);
    let roll_infl = finite_or(v.roll_influence, 0.5).clamp(0.0, 1.0);
    let rolling = finite_or(v.rolling, 0.0).max(0.0);
    let hb_grip = finite_or(v.handbrake_grip, 0.5).clamp(0.0, 1.0);
    let mut locked = [false; MAX_WHEELS];
    for (i, w) in wheels.iter().enumerate() {
        let s = &mut gs[i];
        if !s.grounded || s.load <= 0.0 {
            continue;
        }
        s.angle = out.angle[i];
        let (sa, ca) = (detmath::sin(s.angle as f64) as f32, detmath::cos(s.angle as f64) as f32);
        let along = add(mul(fwd, ca), mul(right, sa));
        s.f = unit_or(sub(along, mul(s.n, dot(along, s.n))), fwd);
        s.l = cross(s.f, s.n);
        s.rf = sub(s.r, mul(up, dot(s.r, up) * (1.0 - roll_infl)));
        s.kf = ch.inv_eff(s.rf, s.f).max(1.0e-9);
        s.kl = ch.inv_eff(s.rf, s.l).max(1.0e-9);
        let hb_here = if sum_hb > 0.0 { (hb_in * w.handbrake).clamp(0.0, 1.0) } else { 0.0 };
        locked[i] = hb_here > 0.5;
        s.limit = w.grip * (1.0 + (hb_grip - 1.0) * hb_here) * s.load * dt;
        s.jd = if sum_drive > 0.0 { drive_total * (w.drive / sum_drive) * dt } else { 0.0 };
        let b = if sum_brake > 0.0 { brake_in * brake_force * (w.brake / sum_brake) } else { 0.0 };
        let h = if sum_hb > 0.0 { hb_in * hb_force * (w.handbrake / sum_hb) } else { 0.0 };
        s.brake_budget = (b + h) * dt;
        // the tyre rolls with a little resistance, which goes with the drive
        let vf = dot(ch.at(s.rf), s.f);
        s.jd += -(vf / s.kf).clamp(-rolling * s.load * dt, rolling * s.load * dt);
    }

    // ---- tyre friction: sequential impulses, the friction circle on the impulse of each wheel
    let apply = |ch: &mut Chassis, s: &mut Grip, jf: f32, jl: f32| {
        let (df, dl) = (jf - s.jf, jl - s.jl);
        if df != 0.0 || dl != 0.0 {
            ch.push(add(mul(s.f, df), mul(s.l, dl)), s.rf);
        }
        s.jf = jf;
        s.jl = jl;
    };
    let clamp_circle = |jf: f32, jl: f32, limit: f32| {
        let m2 = jf * jf + jl * jl;
        if m2 > limit * limit && m2 > 0.0 {
            let k = limit / m2.sqrt();
            (jf * k, jl * k)
        } else {
            (jf, jl)
        }
    };
    for i in 0..n {
        if gs[i].grounded && gs[i].load > 0.0 {
            let s = &mut gs[i];
            let (jf, jl) = clamp_circle(s.jd, 0.0, s.limit);
            let mut sc = *s;
            apply(&mut ch, &mut sc, jf, jl);
            *s = sc;
        }
    }
    for _ in 0..FRICTION_ROUNDS {
        for i in 0..n {
            if !(gs[i].grounded && gs[i].load > 0.0) {
                continue;
            }
            let mut s = gs[i];
            let vp = ch.at(s.rf);
            let (vf, vl) = (dot(vp, s.f), dot(vp, s.l));
            // the brakes stop the wheel from moving along the ground, within what they can do
            if s.brake_budget > 0.0 {
                s.jb = (s.jb - vf / s.kf).clamp(-s.brake_budget, s.brake_budget);
            }
            let want_f = s.jd + s.jb;
            let want_l = s.jl - vl / s.kl;
            let (jf, jl) = clamp_circle(want_f, want_l, s.limit);
            apply(&mut ch, &mut s, jf, jl);
            gs[i] = s;
        }
    }

    // ---- what the wheels did: how fast they turn, how much they slid
    for (i, w) in wheels.iter().enumerate() {
        let s = gs[i];
        let roll = if s.grounded && s.load > 0.0 {
            let vp = ch.at(s.rf);
            out.slip[i] = dot(vp, s.l);
            dot(vp, s.f)
        } else {
            dot(ch.v, fwd)
        };
        if !locked[i] {
            let mut a = out.spin[i] + roll / w.radius * dt;
            if !a.is_finite() {
                a = 0.0;
            }
            // keep the angle small: a wheel that has turned for an hour must still be a precise number
            a -= (a / (2.0 * std::f32::consts::PI)).trunc() * 2.0 * std::f32::consts::PI;
            out.spin[i] = a;
        }
    }
    out.v = ch.v;
    out.w = ch.w;
    out.speed = dot(ch.v, fwd);
    out
}

// ------------------------------------------------------------------------------------------------ the step

/// The shape of the chassis's collider, as the rigid-body physics has it (for its inertia).
fn shape_of(c: &Collider, t: &Transform) -> Shape {
    let scale = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
    match c.shape {
        0 => Shape::Sphere(0.5 * c.size[0].abs() * scale[0].max(scale[1]).max(scale[2])),
        2 => {
            let r = 0.5 * c.size[0].abs() * scale[0].max(scale[2]);
            Shape::Capsule(r, (0.5 * c.size[1].abs() * scale[1] - r).max(0.0))
        }
        _ => Shape::Box([0.5 * c.size[0].abs() * scale[0], 0.5 * c.size[1].abs() * scale[1], 0.5 * c.size[2].abs() * scale[2]]),
    }
}

/// A quaternion for a turn of `angle` radians about the unit axis `a`.
fn turn(a: V3, angle: f32) -> [f32; 4] {
    let (s, c) = (detmath::sin(0.5 * angle as f64) as f32, detmath::cos(0.5 * angle as f64) as f32);
    [a[0] * s, a[1] * s, a[2] * s, c]
}

/// Move the models of the wheels: each takes its place on the strut, its steering and its spin.
fn place_wheels(world: &mut World, rig: &Rig, out: &Outcome) {
    let q = normalize_q(rig.t.rot);
    let axes = axes_of(q);
    for (i, w) in rig.wheels.iter().enumerate() {
        if w.entity == 0 {
            continue;
        }
        let Some(e) = world.entity(w.entity) else { continue };
        let local = [w.pos[0], w.pos[1] - out.length[i], w.pos[2]];
        // the wheel turns about its axle (x) as it rolls forward (-z), and about the vertical as it steers to the right
        let turned = quat_mul(turn([0.0, 1.0, 0.0], -out.angle[i]), turn([1.0, 0.0, 0.0], -out.spin[i]));
        if let Some(p) = world.get::<Parent>(e) {
            if p.target.0 == rig.uid {
                if let Some(lt) = world.get_mut::<LocalTransform>(e) {
                    lt.pos = local;
                    lt.rot = turned;
                }
            }
        } else if let Some(t) = world.get_mut::<Transform>(e) {
            t.pos = add(rig.t.pos, rotate(&axes, local));
            t.rot = quat_mul(q, turned);
        }
    }
}

/// Run every `Vehicle` of the world for one tick (after the rigid bodies of the tick: see the module).
pub fn step(world: &mut World, gravity: V3, dt: f32, fields: &Terrains, meshes: &MeshStore) {
    let mut list: Vec<(u64, Entity, Vehicle)> = Vec::new();
    let _ = world.for_each_uid::<(&Vehicle,), _>(|e, uid, (v,)| list.push((uid, e, *v)));
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.0);
    WORK.with(|w| w.set(w.get() + 1));
    let mut rigs: Vec<Rig> = Vec::with_capacity(list.len());
    for (uid, entity, v) in list {
        let text = world.strings().get(v.wheels.0).to_string();
        let Ok(wheels) = wheels_of(&text) else { continue };
        let (Some(t), Some(vel), Some(av), Some(rb), Some(c)) = (world.get::<Transform>(entity).copied(), world.get::<Velocity>(entity).copied(), world.get::<AngularVelocity>(entity).copied(), world.get::<RigidBody>(entity).copied(), world.get::<Collider>(entity).copied()) else { continue };
        if !(rb.mass.is_finite() && rb.mass > 0.0) || wheels.is_empty() || !t.pos.iter().chain(vel.lin.iter()).chain(av.w.iter()).all(|x| x.is_finite()) {
            continue;
        }
        rigs.push(Rig { uid, entity, v, t, vel: vel.lin, w: av.w, mass: rb.mass, gravity_scale: rb.gravity_scale, shape: shape_of(&c, &t), wheels });
    }
    if rigs.is_empty() {
        return;
    }
    // the part of the world the rays can reach (all the vehicles together), and what is in it
    let mut reach: Option<(V3, V3)> = None;
    for rig in &rigs {
        let axes = axes_of(normalize_q(rig.t.rot));
        for w in rig.wheels.iter() {
            let a = add(rig.t.pos, rotate(&axes, w.pos));
            let b = add(a, mul(axes[1], -(w.rest + w.radius)));
            let (lo, hi) = segment_box(a, b, 0.5);
            reach = Some(match reach {
                None => (lo, hi),
                Some((rl, rh)) => ([rl[0].min(lo[0]), rl[1].min(lo[1]), rl[2].min(lo[2])], [rh[0].max(hi[0]), rh[1].max(hi[1]), rh[2].max(hi[2])]),
            });
        }
    }
    let ray = RayWorld::gather(world, fields, meshes, reach);
    let layers = Layers::gather(world);
    for rig in &rigs {
        let out = drive(rig, &ray, layers.of(rig.uid), gravity, dt);
        if let Some(vel) = world.get_mut::<Velocity>(rig.entity) {
            vel.lin = out.v;
        }
        if let Some(av) = world.get_mut::<AngularVelocity>(rig.entity) {
            av.w = out.w;
        }
        if let Some(v) = world.get_mut::<Vehicle>(rig.entity) {
            v.speed = out.speed;
            v.steering = out.steering;
            v.contact = out.contact;
            v.on_ground = out.on_ground;
            v.compression = out.compression;
            v.spin = out.spin;
            v.slip = out.slip;
        }
        place_wheels(world, rig, &out);
    }
}

// ------------------------------------------------------------------------------------------------ for pictures

/// One wheel as the picture of the debug overlay draws it.
#[derive(Clone, Debug)]
pub struct WheelView {
    pub vehicle: u64,
    /// Where the strut is fixed, where the wheel's centre is (on the strut, `rest - compression` below the mount), and the wheel's radius.
    pub mount: V3,
    pub centre: V3,
    pub radius: f32,
    /// The far end of the ray the wheel casts (a wheel in the air hangs at `rest` on it).
    pub ray_end: V3,
    /// The wheel's axle (the way the wheel's axis points: across the car, turned by the steering), its forward direction and the chassis's up.
    pub axle: V3,
    pub forward: V3,
    pub up: V3,
    pub grounded: bool,
}

/// Every wheel of every vehicle, from the state the last tick left (the pose of the chassis and the outputs of the `Vehicle`).
pub fn wheel_views(world: &mut World) -> Vec<WheelView> {
    let mut list: Vec<(u64, Entity, Vehicle)> = Vec::new();
    let _ = world.for_each_uid::<(&Vehicle,), _>(|e, uid, (v,)| list.push((uid, e, *v)));
    list.sort_by_key(|l| l.0);
    let mut out = Vec::new();
    for (uid, e, v) in list {
        let Some(t) = world.get::<Transform>(e).copied() else { continue };
        let Ok(wheels) = wheels_of(world.strings().get(v.wheels.0)) else { continue };
        let axes = axes_of(normalize_q(t.rot));
        for (i, w) in wheels.iter().enumerate() {
            let mount = add(t.pos, rotate(&axes, w.pos));
            let stand = w.rest - v.compression[i].clamp(0.0, w.travel);
            let angle = v.steering * DEG * w.steer;
            let (sa, ca) = (detmath::sin(angle as f64) as f32, detmath::cos(angle as f64) as f32);
            out.push(WheelView {
                vehicle: uid,
                mount,
                centre: add(mount, mul(axes[1], -stand)),
                radius: w.radius,
                ray_end: add(mount, mul(axes[1], -(w.rest + w.radius))),
                axle: add(mul(axes[0], ca), mul(axes[2], sa)),
                forward: add(mul(neg(axes[2]), ca), mul(axes[0], sa)),
                up: axes[1],
                grounded: v.contact >> i & 1 != 0,
            });
        }
    }
    out
}
