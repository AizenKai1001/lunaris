//! Helicopters (docs/HELICOPTER.md): a rigid body that the rotors lift along its own up axis, whose stick tilts it, whose pedals turn it, that is loud and draws the machines to its pilot, and that
//! gets hurt when it hits something fast. It is a flight model for a game, not a simulation of one: the stick asks for an *attitude* (how far the nose is down, how far the right side is down), a collective
//! for lift, a pedal for a turn rate, and the engine steers the body's spin towards them; the rest (gravity, bumping into things, landing) is the rigid-body physics. Nothing is drawn or played: the
//! rotors are entities the game supplies and this turns, the sound is the game's, and `Noisy` is what the AI hears.
//!
//! The step runs right after the physics of the tick, reads the `Velocity` the physics left (a sudden loss of speed is a crash), and writes the `Velocity` and `AngularVelocity` of the next one.
//! Helicopters are visited in uid order and everything is f32 with `+ - * /` and `sqrt` (the angles go through `detmath`), so it is deterministic. A world without a helicopter pays for one query.
use crate::components::{AngularVelocity, Collider, Heat, Helicopter, HitPoints, LocalTransform, Noisy, Parent, RigidBody, Transform, Velocity};
use crate::detmath;
use crate::layers::Layers;
use crate::meshcol::MeshStore;
use crate::physics_rot::{axes_of, normalize_q, quat_conj, quat_mul};
use crate::terrain::Terrains;
use crate::vehicle_ray::{segment_box, RayWorld};
use lunaris_ecs::{Entity, World};

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

const TAU: f32 = std::f32::consts::TAU;
const DEG: f32 = std::f32::consts::PI / 180.0;
/// How far down the altitude is looked for (metres).
const ALTITUDE_REACH: f32 = 300.0;
/// How fast the attitude closes the gap to what the stick asks for (1/s).
const ATTITUDE_GAIN: f32 = 6.0;
/// Below this rotor speed the body is not steered at all (it tumbles), above it the control fades in.
const CONTROL_FROM: f32 = 0.3;
/// The heat the engine never lets a pilot go above (the director's own cap).
const MAX_HEAT: f32 = 10.0;

#[inline]
fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

#[inline]
fn len(a: V3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// The angle (radians) whose sine is `x` (-1 to 1), by `detmath` so that it is the same on every machine.
fn asin(x: f32) -> f32 {
    detmath::atan2(x as f64, (1.0 - x as f64 * x as f64).max(0.0).sqrt()) as f32
}

/// A turn of `angle` radians about the vertical axis, about the local x axis, about the local z axis.
fn q_y(angle: f32) -> [f32; 4] {
    let h = angle as f64 * 0.5;
    [0.0, detmath::sin(h) as f32, 0.0, detmath::cos(h) as f32]
}
fn q_x(angle: f32) -> [f32; 4] {
    let h = angle as f64 * 0.5;
    [detmath::sin(h) as f32, 0.0, 0.0, detmath::cos(h) as f32]
}
fn q_z(angle: f32) -> [f32; 4] {
    let h = angle as f64 * 0.5;
    [0.0, 0.0, detmath::sin(h) as f32, detmath::cos(h) as f32]
}

/// The rotation vector (axis times angle, radians) of the shortest turn that `q` makes.
fn rotation_vector(q: [f32; 4]) -> V3 {
    let q = if q[3] < 0.0 { [-q[0], -q[1], -q[2], -q[3]] } else { q };
    let s = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
    if s < 1.0e-7 {
        return [0.0; 3];
    }
    let angle = 2.0 * detmath::atan2(s as f64, q[3] as f64) as f32;
    [q[0] / s * angle, q[1] / s * angle, q[2] / s * angle]
}

fn spin_part(world: &mut World, uid: u64, angle: f32, about_y: bool) {
    if uid == 0 {
        return;
    }
    let Some(e) = world.entity(uid) else { return };
    let q = if about_y { q_y(angle) } else { q_x(angle) };
    if world.get::<Parent>(e).is_some() && world.get::<LocalTransform>(e).is_some() {
        if let Some(l) = world.get_mut::<LocalTransform>(e) {
            l.rot = q;
        }
    } else if let Some(t) = world.get_mut::<Transform>(e) {
        t.rot = q;
    }
}

struct Rig {
    uid: u64,
    e: Entity,
    h: Helicopter,
    t: Transform,
    v: V3,
    w: V3,
    half_height: f32,
    gravity_scale: f32,
}

pub fn step(world: &mut World, gravity: V3, dt: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let mut list: Vec<(u64, Entity, Helicopter)> = Vec::new();
    let _ = world.for_each_uid::<(&Helicopter,), _>(|e, uid, (h,)| list.push((uid, e, *h)));
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.0);
    let mut rigs: Vec<Rig> = Vec::with_capacity(list.len());
    for (uid, e, h) in list {
        let (Some(t), Some(vel), Some(av), Some(rb)) = (world.get::<Transform>(e).copied(), world.get::<Velocity>(e).copied(), world.get::<AngularVelocity>(e).copied(), world.get::<RigidBody>(e).copied()) else { continue };
        if !(rb.mass.is_finite() && rb.mass > 0.0) || !t.pos.iter().chain(vel.lin.iter()).chain(av.w.iter()).chain(t.rot.iter()).all(|x| x.is_finite()) {
            continue;
        }
        let half_height = world.get::<Collider>(e).map_or(0.0, |c| 0.5 * c.size[1].abs() * t.scale[1].abs());
        rigs.push(Rig { uid, e, h, t, v: vel.lin, w: av.w, half_height, gravity_scale: finite(rb.gravity_scale, 1.0) });
    }
    if rigs.is_empty() {
        return;
    }
    // what is under each of them: one gathering of the world for all
    let mut reach: Option<(V3, V3)> = None;
    for r in &rigs {
        let (lo, hi) = segment_box(r.t.pos, [r.t.pos[0], r.t.pos[1] - ALTITUDE_REACH, r.t.pos[2]], 1.0);
        reach = Some(match reach {
            None => (lo, hi),
            Some((rl, rh)) => ([rl[0].min(lo[0]), rl[1].min(lo[1]), rl[2].min(lo[2])], [rh[0].max(hi[0]), rh[1].max(hi[1]), rh[2].max(hi[2])]),
        });
    }
    let ground = RayWorld::gather(world, fields, meshes, reach);
    let layers = Layers::gather(world);

    for r in rigs {
        let mut h = r.h;
        let dead = world.get::<HitPoints>(r.e).is_some_and(|x| x.dead);
        let (collective, pitch, roll, yaw) = (finite(h.collective, 0.0).clamp(-1.0, 1.0), finite(h.pitch, 0.0).clamp(-1.0, 1.0), finite(h.roll, 0.0).clamp(-1.0, 1.0), finite(h.yaw, 0.0).clamp(-1.0, 1.0));
        let mut v = r.v;

        // a crash: the speed that the contacts took away this tick (what is left after what the rotors and gravity gave it last tick)
        let crash_speed = finite(h.crash_speed, 8.0);
        if crash_speed > 0.0 {
            let g = [gravity[0] * r.gravity_scale * dt, gravity[1] * r.gravity_scale * dt, gravity[2] * r.gravity_scale * dt];
            let expected = [h.last_v[0] + g[0], h.last_v[1] + g[1], h.last_v[2] + g[2]];
            let lost = len([v[0] - expected[0], v[1] - expected[1], v[2] - expected[2]]);
            if lost > crash_speed && !dead {
                let damage = (lost - crash_speed) * finite(h.crash_damage, 12.0).max(0.0);
                if damage > 0.0 {
                    if let Some(hp) = world.get_mut::<HitPoints>(r.e) {
                        hp.value = (hp.value - damage).max(0.0);
                    }
                }
                events.push(("heli_crash".to_string(), r.uid, Some(r.t.pos)));
            }
        }

        // the rotors
        let engine = h.engine && !dead;
        let spool_up = finite(h.spool_up, 6.0).max(0.05);
        let spool_down = finite(h.spool_down, 10.0).max(0.05);
        h.spool = finite(h.spool, 0.0).clamp(0.0, 1.0);
        h.spool = if engine { (h.spool + dt / spool_up).min(1.0) } else { (h.spool - dt / spool_down).max(0.0) };

        // what is under it
        let axes = axes_of(normalize_q(r.t.rot));
        let up = axes[1];
        let fwd = [-axes[2][0], -axes[2][1], -axes[2][2]];
        let right = axes[0];
        h.altitude = match ground.cast(r.t.pos, [0.0, -1.0, 0.0], ALTITUDE_REACH, layers.of(r.uid), r.uid) {
            Some(hit) => (hit.t - r.half_height).max(0.0),
            None => -1.0,
        };

        // lift along the body's up axis
        let lift = finite(h.lift, 19.6).max(0.0);
        let mut thrust = h.spool * h.spool * lift * (collective + 1.0) * 0.5;
        if h.altitude >= 0.0 {
            let fade = (1.0 - h.altitude / finite(h.ground_height, 6.0).max(0.1)).clamp(0.0, 1.0);
            thrust *= 1.0 + finite(h.ground_effect, 0.0).max(0.0) * fade;
        }
        // a tipped helicopter loses lift: give back what the assist says
        let c = up[1].clamp(0.5, 1.0);
        thrust *= 1.0 + finite(h.assist, 0.0).clamp(0.0, 1.0) * (1.0 / c - 1.0);
        for a in 0..3 {
            v[a] += up[a] * thrust * dt;
        }
        // the air
        let side = detmath::exp((-finite(h.drag, 0.0).max(0.0) * dt) as f64) as f32;
        let climb = detmath::exp((-finite(h.climb_drag, 0.0).max(0.0) * (0.25 + 0.75 * h.spool) * dt) as f64) as f32;
        v = [v[0] * side, v[1] * climb, v[2] * side];

        // the attitude the stick asks for
        let heading = detmath::atan2(-(fwd[0] as f64), -(fwd[2] as f64)) as f32;
        let fy = (-fwd[1]).clamp(-1.0, 1.0);
        let cos_nose = (1.0 - fy * fy).sqrt().max(0.2);
        let nose_down = asin(fy);
        let right_down = -asin((right[1] / cos_nose).clamp(-1.0, 1.0));
        h.tilt = [nose_down / DEG, right_down / DEG];
        h.heading = heading / DEG;
        let mut w = r.w;
        if h.spool > CONTROL_FROM {
            let authority = ((h.spool - CONTROL_FROM) / (1.0 - CONTROL_FROM)).clamp(0.0, 1.0);
            let max_tilt = finite(h.max_tilt, 30.0).clamp(0.0, 80.0) * DEG;
            let want = quat_mul(quat_mul(q_y(heading), q_x(-pitch * max_tilt * authority)), q_z(-roll * max_tilt * authority));
            let have = normalize_q(r.t.rot);
            let mut turn = rotation_vector(quat_mul(want, quat_conj(have)));
            let cap = finite(h.tilt_rate, 70.0).max(0.0) * DEG;
            let n = len(turn) * ATTITUDE_GAIN;
            let k = if n > cap && n > 0.0 { cap / n } else { 1.0 };
            for a in 0..3 {
                turn[a] *= ATTITUDE_GAIN * k;
            }
            // the pedal turns it about the vertical axis (right is positive, a turn to the left is a positive rotation)
            turn[1] -= yaw * finite(h.yaw_rate, 80.0).max(0.0) * DEG * authority;
            w = turn;
        }

        // what it sounds like to the machines, and what that does to the pilot
        let noise = finite(h.noise, 0.0).max(0.0);
        let idle = finite(h.idle_noise, 0.35).clamp(0.0, 1.0);
        let radius = noise * h.spool * (idle + (1.0 - idle) * (collective + 1.0) * 0.5);
        match world.get_mut::<Noisy>(r.e) {
            Some(n) => n.radius = radius,
            None => {
                let _ = world.insert(r.e, Noisy { radius });
            }
        }
        let heat = finite(h.heat, 0.0).max(0.0);
        if heat > 0.0 && noise > 0.0 && h.pilot.0 != 0 {
            if let Some(pe) = world.entity(h.pilot.0) {
                if let Some(ht) = world.get_mut::<Heat>(pe) {
                    ht.value = (ht.value + heat * (radius / noise) * dt).min(MAX_HEAT);
                }
            }
        }

        // the rotors turn
        h.rotor_angle = (finite(h.rotor_angle, 0.0) + h.spool * finite(h.rotor_rps, 6.0).max(0.0) * TAU * dt) % TAU;
        spin_part(world, h.rotor.0, h.rotor_angle, true);
        spin_part(world, h.tail_rotor.0, (h.rotor_angle * 5.0) % TAU, false);

        h.speed = len(v);
        h.last_v = v;
        if let Some(vel) = world.get_mut::<Velocity>(r.e) {
            vel.lin = v;
        }
        if let Some(av) = world.get_mut::<AngularVelocity>(r.e) {
            av.w = w;
        }
        if let Some(x) = world.get_mut::<Helicopter>(r.e) {
            *x = h;
        }
    }
}
