//! Flying machines (docs/DRONES.md): the mind of a drone, a gunship's scout or anything else that floats. A `FlyAgent` moves its entity by setting its `Velocity` (so it needs no physics body: like a `Steer`
//! it is kinematic), holds a height over whatever is under it, goes to a place, follows a target or circles it at a distance, patrols a list of points, steers round obstacles it sees ahead and away from the other
//! agents, and, with `perception`, lets its own `Perception` decide among those (alert: circle the target; searching: go where it was; calm: patrol). The `Perception`'s autopilot (`engage`) turns it to the target
//! and pulls its `Weapon`'s trigger. It is a classical controller: nothing is learned or random.
//!
//! Deterministic: agents are visited in uid order, the rays come from one gathering of the static world, and the angles go through `detmath`.
use crate::components::{FlyAgent, Perception, Transform, Velocity};
use crate::detmath;
use crate::layers::Layers;
use crate::meshcol::MeshStore;
use crate::terrain::Terrains;
use crate::vehicle_ray::{segment_box, RayWorld};
use lunaris_ecs::{Entity, World};
use std::collections::HashMap;

type V3 = [f32; 3];

/// How far down the ground is looked for (metres).
const GROUND_REACH: f32 = 500.0;
/// A point is reached when the agent is this near it on the ground plane (metres).
const ARRIVE: f32 = 2.0;
/// The most points of a patrol that are read.
const MAX_POINTS: usize = 64;

fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len(a: V3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn len_xz(a: V3) -> f32 {
    (a[0] * a[0] + a[2] * a[2]).sqrt()
}

/// The points of a patrol text: one `x y z` a line (`#` starts a comment).
fn points_of(text: &str) -> Vec<V3> {
    let mut out = Vec::new();
    for line in text.lines() {
        let nums: Vec<f32> = line.split('#').next().unwrap_or("").split_whitespace().filter_map(|w| w.parse::<f32>().ok()).filter(|v| v.is_finite()).collect();
        if nums.len() >= 3 {
            out.push([nums[0], nums[1], nums[2]]);
            if out.len() >= MAX_POINTS {
                break;
            }
        }
    }
    out
}

/// `v` turned about the vertical axis by `deg` degrees (left is positive, as everywhere).
fn turn(v: V3, deg: f32) -> V3 {
    let a = deg as f64 * (std::f64::consts::PI / 180.0);
    let (s, c) = (detmath::sin(a) as f32, detmath::cos(a) as f32);
    [v[0] * c + v[2] * s, v[1], -v[0] * s + v[2] * c]
}

struct Rig {
    e: Entity,
    uid: u64,
    a: FlyAgent,
    pos: V3,
    vel: V3,
    rot: [f32; 4],
}

/// What the agent wants to do this tick: where to go on the ground plane, how it should approach, and whether it is engaged with a target.
struct Plan {
    /// A point to go to (x, z), and how close to stop (0: go right there).
    goal: Option<([f32; 2], f32)>,
    /// A target to circle: its place and the tangential speed.
    orbit: Option<(V3, f32)>,
    engaged: bool,
    /// Waiting at a point of a patrol (which counts as having arrived).
    waiting: bool,
}

pub fn update(world: &mut World, dt: f32, fields: &Terrains, meshes: &MeshStore) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let mut list: Vec<(u64, Entity, FlyAgent)> = Vec::new();
    let _ = world.for_each_uid::<(&FlyAgent,), _>(|e, uid, (a,)| {
        if a.active {
            list.push((uid, e, *a));
        }
    });
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.0);
    let mut rigs: Vec<Rig> = Vec::with_capacity(list.len());
    for (uid, e, a) in list {
        let (Some(t), Some(v)) = (world.get::<Transform>(e).copied(), world.get::<Velocity>(e).copied()) else { continue };
        if !t.pos.iter().chain(v.lin.iter()).chain(t.rot.iter()).all(|x| x.is_finite()) {
            continue;
        }
        rigs.push(Rig { e, uid, a, pos: t.pos, vel: v.lin, rot: t.rot });
    }
    if rigs.is_empty() {
        return;
    }
    // the world the rays meet (all the agents together)
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for r in &rigs {
        let reach = finite(r.a.avoid, 8.0).max(0.0) + 4.0;
        let (a, b) = segment_box(r.pos, [r.pos[0], r.pos[1] - GROUND_REACH, r.pos[2]], reach);
        for k in 0..3 {
            lo[k] = lo[k].min(a[k]);
            hi[k] = hi[k].max(b[k]).max(r.pos[k] + reach);
        }
    }
    let rw = RayWorld::gather(world, fields, meshes, Some((lo, hi)));
    let layers = Layers::gather(world);
    let others: Vec<(u64, V3, f32)> = rigs.iter().map(|r| (r.uid, r.pos, finite(r.a.separation, 5.0).max(0.0))).collect();
    // (the agents on a grid as wide as the widest separation, so that each looks only at its own and the eight cells round it: the same agents in the same order as looking at all of them, which is O(n^2))
    let cell = others.iter().map(|o| o.2).fold(1.0f32, f32::max);
    let cell_of = |p: V3| ((p[0] / cell).floor() as i32, (p[2] / cell).floor() as i32);
    let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::with_capacity(others.len());
    for (i, o) in others.iter().enumerate() {
        grid.entry(cell_of(o.1)).or_default().push(i as u32);
    }
    let mut near: Vec<u32> = Vec::new();

    for r in rigs {
        let mut a = r.a;
        let who = layers.of(r.uid);
        let speed = finite(a.speed, 12.0).max(0.0);
        let accel = finite(a.accel, 10.0).max(0.1);
        let climb = finite(a.climb, 6.0).max(0.0);

        // what it means to do
        let plan = decide(world, &mut a, &r, dt);

        // desired velocity on the ground plane
        let mut want = [0.0f32; 3];
        let mut arrived = false;
        if let Some((g, stop)) = plan.goal {
            let d = [g[0] - r.pos[0], 0.0, g[1] - r.pos[2]];
            let dist = len_xz(d);
            let along = dist - stop;
            if along > 0.0 && dist > 1.0e-4 {
                // (it slows as it comes in: a speed that it can stop from)
                let s = speed.min((2.0 * accel * along).sqrt());
                want = [d[0] / dist * s, 0.0, d[2] / dist * s];
            }
            arrived = dist <= stop.max(ARRIVE);
        }
        if let Some((c, tangent)) = plan.orbit {
            let d = [c[0] - r.pos[0], 0.0, c[2] - r.pos[2]];
            let dist = len_xz(d).max(1.0e-4);
            let range = finite(a.range, 12.0).max(1.0);
            let dir = [d[0] / dist, 0.0, d[2] / dist];
            // towards the ring at `range`, out of it if inside, and round it
            let radial = ((dist - range) * 1.5).clamp(-speed * 0.5, speed);
            let side = [-dir[2], 0.0, dir[0]];
            want = [dir[0] * radial + side[0] * tangent, 0.0, dir[2] * radial + side[2] * tangent];
            arrived = (dist - range).abs() <= ARRIVE;
        }
        // away from the others
        let sep = finite(a.separation, 5.0).max(0.0);
        if sep > 0.0 {
            near.clear();
            let (cx, cz) = cell_of(r.pos);
            for dx in -1..=1 {
                for dz in -1..=1 {
                    if let Some(c) = grid.get(&(cx.wrapping_add(dx), cz.wrapping_add(dz))) {
                        near.extend_from_slice(c);
                    }
                }
            }
            // (in the order of the agents, which is that of their uids, as the sum was always made)
            near.sort_unstable();
            for &oi in &near {
                let (ouid, opos, _) = &others[oi as usize];
                if *ouid == r.uid {
                    continue;
                }
                let d = sub(r.pos, *opos);
                let dist = len(d);
                if dist < sep {
                    // (agents on the same spot are pushed apart by their uids)
                    let away = if dist > 1.0e-3 { [d[0] / dist, d[1] / dist, d[2] / dist] } else if r.uid < *ouid { [1.0, 0.0, 0.0] } else { [-1.0, 0.0, 0.0] };
                    let push = (1.0 - dist / sep) * speed;
                    want[0] += away[0] * push;
                    want[2] += away[2] * push;
                }
            }
        }
        // round what is ahead: three rays, and over it if it is all in the way
        a.blocked = false;
        let avoid = finite(a.avoid, 8.0).max(0.0);
        let mut over = false;
        let heading = len_xz(want);
        if avoid > 0.0 && heading > 0.1 {
            let dir = [want[0] / heading, 0.0, want[2] / heading];
            let look = avoid + r.vel[0].hypot(r.vel[2]) * 0.4;
            let ray = |d: V3| rw.cast(r.pos, d, look, who, r.uid).map(|h| (h.t, h.normal));
            let (c, l, rr) = (ray(dir), ray(turn(dir, 35.0)), ray(turn(dir, -35.0)));
            if let Some((t, n)) = c {
                a.blocked = true;
                let free_l = l.map_or(look, |h| h.0);
                let free_r = rr.map_or(look, |h| h.0);
                let urgency = 1.0 - (t / look).clamp(0.0, 1.0);
                if free_l.max(free_r) < look * 0.6 {
                    // nowhere to go round: up
                    over = true;
                    want = [want[0] * 0.3, 0.0, want[2] * 0.3];
                } else {
                    let side = if free_l >= free_r { turn(dir, 90.0) } else { turn(dir, -90.0) };
                    let k = speed * (0.4 + 0.9 * urgency);
                    want = [want[0] * (1.0 - 0.7 * urgency) + side[0] * k + n[0] * k * 0.3, 0.0, want[2] * (1.0 - 0.7 * urgency) + side[2] * k + n[2] * k * 0.3];
                }
            } else {
                // a wall at the side pushes it off a little
                if let Some((t, n)) = l {
                    let k = speed * 0.4 * (1.0 - (t / look).clamp(0.0, 1.0));
                    want[0] += n[0] * k;
                    want[2] += n[2] * k;
                }
                if let Some((t, n)) = rr {
                    let k = speed * 0.4 * (1.0 - (t / look).clamp(0.0, 1.0));
                    want[0] += n[0] * k;
                    want[2] += n[2] * k;
                }
            }
        }
        let l = len_xz(want);
        if l > speed && l > 0.0 {
            want = [want[0] / l * speed, 0.0, want[2] / l * speed];
        }

        // the height: what is under it, and the hover above that
        let down = rw.cast([r.pos[0], r.pos[1] + 0.5, r.pos[2]], [0.0, -1.0, 0.0], GROUND_REACH, who, r.uid);
        a.altitude = down.map_or(-1.0, |h| (h.t - 0.5).max(0.0));
        if let Some(h) = down {
            let ground = r.pos[1] + 0.5 - h.t;
            let target_y = ground + finite(a.hover, 6.0).max(0.0);
            want[1] = ((target_y - r.pos[1]) * 2.5).clamp(-climb, climb);
        } else {
            want[1] = 0.0;
        }
        if over {
            want[1] = climb;
        }
        // (a ceiling or a roof just above it: the rays look up as well)
        if want[1] > 0.0 {
            if let Some(h) = rw.cast(r.pos, [0.0, 1.0, 0.0], 3.0, who, r.uid) {
                want[1] = want[1].min(((h.t - 1.0) * 2.0).max(0.0));
            }
        }

        // the velocity follows, no faster than the machine can change it
        let step = accel * dt;
        let mut v = r.vel;
        let mut dv = sub(want, v);
        let d = len(dv);
        if d > step && d > 0.0 {
            let k = step / d;
            dv = [dv[0] * k, dv[1] * k, dv[2] * k];
        }
        for k in 0..3 {
            v[k] += dv[k];
        }
        a.speed_now = len(v);
        a.arrived = arrived || plan.waiting;
        if let Some(x) = world.get_mut::<Velocity>(r.e) {
            x.lin = v;
        }
        // facing: the way it goes, at its turn rate, when nothing else is turning it
        if a.face && !plan.engaged && len_xz(v) > 0.8 {
            let want_yaw = detmath::atan2(-(v[0] as f64), -(v[2] as f64)) as f32 * (180.0 / std::f32::consts::PI);
            let have = crate::building::yaw_of(r.rot);
            let mut diff = want_yaw - have;
            while diff > 180.0 {
                diff -= 360.0;
            }
            while diff <= -180.0 {
                diff += 360.0;
            }
            let max = finite(a.turn_rate, 180.0).max(0.0) * dt;
            let yaw = have + diff.clamp(-max, max);
            if let Some(t) = world.get_mut::<Transform>(r.e) {
                t.rot = crate::building::yaw_quat(yaw);
            }
        }
        if let Some(w) = world.get_mut::<FlyAgent>(r.e) {
            *w = a;
        }
    }
}

/// Choose what to do from the mode (or the `Perception`), moving the patrol on when a point is reached.
fn decide(world: &mut World, a: &mut FlyAgent, r: &Rig, dt: f32) -> Plan {
    let mut mode = a.mode;
    let mut goal: Option<V3> = None;
    let mut target: Option<V3> = None;
    let mut engaged = false;
    if a.perception {
        if let Some(p) = world.get::<Perception>(r.e).copied().filter(|p| p.active) {
            match p.state {
                2 => {
                    mode = 3;
                    target = world.entity(p.target.0).and_then(|e| world.get::<Transform>(e)).map(|t| t.pos).or(Some(p.seen_pos));
                    engaged = true;
                }
                3 => {
                    mode = 1;
                    goal = Some(p.seen_pos);
                    engaged = p.engage;
                }
                _ => {
                    mode = if a.patrol.0 != 0 { 4 } else { 0 };
                }
            }
        }
    }
    let range = finite(a.range, 12.0).max(1.0);
    match mode {
        1 => {
            let g = goal.unwrap_or(a.goal);
            Plan { goal: Some(([g[0], g[2]], 0.0)), orbit: None, engaged, waiting: false }
        }
        2 | 3 => {
            let t = target.or_else(|| world.entity(a.target.0).and_then(|e| world.get::<Transform>(e)).map(|t| t.pos));
            match t {
                Some(t) if mode == 2 => Plan { goal: Some(([t[0], t[2]], range)), orbit: None, engaged, waiting: false },
                Some(t) => Plan { goal: None, orbit: Some((t, finite(a.orbit, 0.0))), engaged, waiting: false },
                None => Plan { goal: None, orbit: None, engaged, waiting: false },
            }
        }
        4 => {
            let text = world.strings().get(a.patrol.0).to_string();
            let pts = points_of(&text);
            if pts.is_empty() {
                return Plan { goal: None, orbit: None, engaged, waiting: false };
            }
            if a.patrol_at as usize >= pts.len() {
                a.patrol_at = 0;
            }
            let p = pts[a.patrol_at as usize];
            let dist = len_xz(sub(p, r.pos));
            if a.wait_left > 0.0 {
                a.wait_left = (a.wait_left - dt).max(0.0);
                if a.wait_left <= 0.0 {
                    a.patrol_at = (a.patrol_at + 1) % pts.len() as u32;
                }
                return Plan { goal: None, orbit: None, engaged, waiting: true };
            }
            if dist <= ARRIVE {
                a.wait_left = finite(a.wait, 1.0).max(0.0).max(dt);
                return Plan { goal: None, orbit: None, engaged, waiting: true };
            }
            Plan { goal: Some(([p[0], p[2]], 0.0)), orbit: None, engaged, waiting: false }
        }
        _ => Plan { goal: None, orbit: None, engaged, waiting: false },
    }
}
