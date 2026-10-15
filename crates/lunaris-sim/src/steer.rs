//! `Steer`: steering behaviours and flocking for crowds, herds, schools and swarms (the steering library of Unity's Mass and Unreal's Mass Entity crowds, in one
//! component). An entity with a `Steer` and a `Velocity` turns its velocity every tick (after the graph systems) by the sum of the behaviours that are switched on, each
//! a weighted acceleration, capped at `max_accel`, and the speed is capped at `max_speed`; the usual movement then carries it.
//!
//! * `seek` (an entity) with `arrive`: run at it and slow down inside the arrive distance; `flee` (an entity) inside `flee_radius`; `wander`: drift in a random direction
//!   that changes smoothly;
//! * flocking, with the other `Steer` entities of the same `group` within `radius`: `separation` (stay `personal` metres apart), `alignment` (head the same way),
//!   `cohesion` (stay together);
//! * `plane` 0 moves on the ground (x and z), 1 in the picture plane (x and y); `face` turns the entity to face its way.
//!
//! The neighbours are found with a grid of cells the size of the radius, so a thousand agents cost about a millisecond. Everything is worked out from the velocities
//! and positions of the start of the tick, in uid order, so the result does not depend on the order in which agents are stored.
use crate::components::{Steer, Transform, Velocity};
use crate::detmath;
use crate::noise::noise2;
use lunaris_ecs::{Entity, World};
use std::collections::HashMap;

type V3 = [f32; 3];

fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

struct Agent {
    e: Entity,
    uid: u64,
    pos: V3,
    vel: V3,
    s: Steer,
}

/// The two axes of the plane an agent moves in.
fn axes(plane: u32) -> (usize, usize) {
    if plane == 1 {
        (0, 1)
    } else {
        (0, 2)
    }
}

fn clamp_len(v: (f32, f32), max: f32) -> (f32, f32) {
    let l = (v.0 * v.0 + v.1 * v.1).sqrt();
    if l > max && l > 1e-9 {
        (v.0 * max / l, v.1 * max / l)
    } else {
        v
    }
}

pub fn update(world: &mut World, tick: u64, dt: f32) {
    let mut agents: Vec<Agent> = Vec::new();
    let _ = world.for_each_uid::<(&Steer, &Transform, &Velocity), _>(|e, uid, (s, t, v)| {
        if s.active {
            agents.push(Agent { e, uid, pos: t.pos, vel: v.lin, s: *s });
        }
    });
    if agents.is_empty() {
        return;
    }
    agents.sort_by_key(|a| a.uid);
    // where the targets are (seek and flee name entities)
    let target = |world: &World, uid: u64| -> Option<V3> { world.entity(uid).and_then(|e| world.get::<Transform>(e)).map(|t| t.pos) };
    // a grid of the flocks: cell size is the largest radius of a flocking agent
    let cell = agents.iter().filter(|a| a.s.group.0 != 0).map(|a| finite(a.s.radius, 4.0)).fold(0.5f32, f32::max);
    let key = |a: &Agent| -> (i32, i32) {
        let (i, j) = axes(a.s.plane);
        ((a.pos[i] / cell).floor() as i32, (a.pos[j] / cell).floor() as i32)
    };
    let mut grid: HashMap<(i32, i32, u32), Vec<usize>> = HashMap::new();
    for (n, a) in agents.iter().enumerate() {
        if a.s.group.0 != 0 && a.s.plane <= 1 {
            let k = key(a);
            grid.entry((k.0, k.1, a.s.group.0)).or_default().push(n);
        }
    }
    let mut out: Vec<(V3, Option<f32>)> = Vec::with_capacity(agents.len());
    for (n, a) in agents.iter().enumerate() {
        let s = &a.s;
        let (i, j) = axes(s.plane);
        let max_speed = finite(s.max_speed, 4.0).max(0.0);
        let max_accel = finite(s.max_accel, 10.0).max(0.0);
        let (px, py) = (a.pos[i], a.pos[j]);
        let (vx, vy) = (a.vel[i], a.vel[j]);
        let mut acc = (0.0f32, 0.0f32);
        let mut add = |w: f32, v: (f32, f32)| {
            acc.0 += w * v.0;
            acc.1 += w * v.1;
        };
        // towards a velocity: the acceleration that would reach it in about a quarter of a second
        let towards = |dvx: f32, dvy: f32| ((dvx - vx) * 4.0, (dvy - vy) * 4.0);
        if s.seek.0 != 0 {
            if let Some(t) = target(world, s.seek.0) {
                let (dx, dy) = (t[i] - px, t[j] - py);
                let d = (dx * dx + dy * dy).sqrt();
                if d > 1e-4 {
                    let arrive = finite(s.arrive, 1.5).max(1e-3);
                    let want = max_speed * (d / arrive).min(1.0);
                    add(finite(s.seek_weight, 1.0), clamp_len(towards(dx / d * want, dy / d * want), max_accel));
                } else {
                    add(finite(s.seek_weight, 1.0), clamp_len(towards(0.0, 0.0), max_accel));
                }
            }
        }
        if s.flee.0 != 0 {
            if let Some(t) = target(world, s.flee.0) {
                let (dx, dy) = (px - t[i], py - t[j]);
                let d = (dx * dx + dy * dy).sqrt();
                let radius = finite(s.flee_radius, 6.0).max(1e-3);
                if d < radius {
                    let (ux, uy) = if d > 1e-4 { (dx / d, dy / d) } else { (1.0, 0.0) };
                    let urgency = 1.0 - d / radius;
                    add(finite(s.flee_weight, 1.0) * (0.3 + 0.7 * urgency), clamp_len(towards(ux * max_speed, uy * max_speed), max_accel));
                }
            }
        }
        let mut new_heading = None;
        if s.wander > 0.0 {
            // the heading drifts smoothly: noise of the clock and of the agent, turning at up to about 2.5 radians a second
            let turn = noise2(tick as f64 * dt as f64 * 0.8, a.uid as f64 * 3.7, 41.0) as f32;
            let h = finite(s.heading, 0.0) + turn * 2.5 * dt * 2.0;
            new_heading = Some(h);
            let (c, sn) = (detmath::cos(h as f64) as f32, detmath::sin(h as f64) as f32);
            add(finite(s.wander, 0.0), clamp_len(towards(c * max_speed * 0.5, sn * max_speed * 0.5), max_accel));
        }
        if s.group.0 != 0 && s.plane <= 1 {
            let radius = finite(s.radius, 4.0).max(0.01);
            let personal = finite(s.personal, 0.8).max(0.0);
            let (cx, cy) = key(a);
            let (mut sep, mut avg_v, mut centre, mut count) = ((0.0f32, 0.0f32), (0.0f32, 0.0f32), (0.0f32, 0.0f32), 0u32);
            for gx in cx - 1..=cx + 1 {
                for gy in cy - 1..=cy + 1 {
                    let Some(list) = grid.get(&(gx, gy, s.group.0)) else { continue };
                    for &m in list {
                        if m == n {
                            continue;
                        }
                        let o = &agents[m];
                        let (dx, dy) = (px - o.pos[i], py - o.pos[j]);
                        let d2 = dx * dx + dy * dy;
                        if d2 > radius * radius {
                            continue;
                        }
                        let d = d2.sqrt();
                        count += 1;
                        avg_v.0 += o.vel[i];
                        avg_v.1 += o.vel[j];
                        centre.0 += o.pos[i];
                        centre.1 += o.pos[j];
                        if d < personal {
                            // pushed away, harder the closer (two on the same spot part along the x axis, by uid, so that they do separate)
                            let (ux, uy) = if d > 1e-4 { (dx / d, dy / d) } else if a.uid < o.uid { (1.0, 0.0) } else { (-1.0, 0.0) };
                            let w = 1.0 - d / personal.max(1e-6);
                            sep.0 += ux * w;
                            sep.1 += uy * w;
                        }
                    }
                }
            }
            if count > 0 {
                let inv = 1.0 / count as f32;
                add(finite(s.separation, 1.5), clamp_len((sep.0 * max_accel, sep.1 * max_accel), max_accel));
                add(finite(s.alignment, 1.0), clamp_len(((avg_v.0 * inv - vx) * 2.0, (avg_v.1 * inv - vy) * 2.0), max_accel));
                let (tx, ty) = (centre.0 * inv - px, centre.1 * inv - py);
                let d = (tx * tx + ty * ty).sqrt();
                if d > 1e-4 {
                    add(finite(s.cohesion, 1.0), clamp_len(towards(tx / d * max_speed * 0.5, ty / d * max_speed * 0.5), max_accel));
                }
            }
        }
        // a rectangle to stay in: pushed back from the last `edge` metres, up to the full acceleration at the side
        let b = s.bounds;
        if b != [0.0; 4] {
            let edge = finite(s.edge, 2.0).max(1e-3);
            let push = |p: f32, lo: f32, hi: f32| -> f32 { (((lo + edge - p) / edge).clamp(0.0, 1.5) - ((p - (hi - edge)) / edge).clamp(0.0, 1.5)) * max_accel };
            acc.0 += push(px, b[0], b[2]);
            acc.1 += push(py, b[1], b[3]);
        }
        let acc = clamp_len(acc, max_accel);
        let (nvx, nvy) = clamp_len((vx + acc.0 * dt, vy + acc.1 * dt), max_speed);
        let mut vel = a.vel;
        vel[i] = nvx;
        vel[j] = nvy;
        out.push((vel, new_heading));
    }
    for (a, (vel, heading)) in agents.iter().zip(out) {
        let (i, j) = axes(a.s.plane);
        let speed = (vel[i] * vel[i] + vel[j] * vel[j]).sqrt();
        if let Some(v) = world.get_mut::<Velocity>(a.e) {
            v.lin = vel;
        }
        if let Some(s) = world.get_mut::<Steer>(a.e) {
            s.speed = speed;
            if let Some(h) = heading {
                s.heading = if h.abs() > 1000.0 { h % std::f32::consts::TAU } else { h };
            }
        }
        if a.s.face && speed > 0.05 {
            let q = if a.s.plane == 1 {
                // the picture plane: a turn about z, 0 pointing along +x
                let h = detmath::atan2(vel[1] as f64, vel[0] as f64) * 0.5;
                [0.0, 0.0, detmath::sin(h) as f32, detmath::cos(h) as f32]
            } else {
                // the ground: a turn about y so that -z points along the velocity
                let h = detmath::atan2(-vel[0] as f64, -vel[2] as f64) * 0.5;
                [0.0, detmath::sin(h) as f32, 0.0, detmath::cos(h) as f32]
            };
            if let Some(t) = world.get_mut::<Transform>(a.e) {
                t.rot = q;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::Sim;
    use lunaris_json::parse as json;

    fn spawn(s: &mut Sim, uid: u64, comps: &str) {
        s.apply(&Command::Spawn { uid: Some(uid), components: json(comps).unwrap().as_object().unwrap().to_vec() }).unwrap();
    }

    fn pos(s: &Sim, uid: u64) -> V3 {
        s.world.get::<Transform>(s.world.entity(uid).unwrap()).unwrap().pos
    }

    fn vel(s: &Sim, uid: u64) -> V3 {
        s.world.get::<Velocity>(s.world.entity(uid).unwrap()).unwrap().lin
    }

    fn dist(a: V3, b: V3) -> f32 {
        ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }

    #[test]
    fn seek_arrives_and_stops_at_the_target() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[10,0,0]}}"#);
        spawn(&mut s, 2, r#"{"Transform":{},"Velocity":{},"Steer":{"seek":1,"max_speed":5,"arrive":2}}"#);
        s.step_n(60);
        let v = (vel(&s, 2)[0].powi(2) + vel(&s, 2)[2].powi(2)).sqrt();
        assert!(v > 3.0 && v <= 5.001, "full speed on the way: {v}");
        s.step_n(400);
        assert!(dist(pos(&s, 2), pos(&s, 1)) < 0.35, "{:?}", pos(&s, 2));
        assert!((vel(&s, 2)[0].powi(2) + vel(&s, 2)[2].powi(2)).sqrt() < 0.5, "it has slowed to a stop");
        // it never left the ground plane
        assert_eq!(pos(&s, 2)[1], 0.0);
    }

    #[test]
    fn flee_runs_away_inside_its_radius_only() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,0]}}"#);
        spawn(&mut s, 2, r#"{"Transform":{"pos":[2,0,0]},"Velocity":{},"Steer":{"flee":1,"flee_radius":6,"max_speed":4}}"#);
        spawn(&mut s, 3, r#"{"Transform":{"pos":[20,0,0]},"Velocity":{},"Steer":{"flee":1,"flee_radius":6,"max_speed":4}}"#);
        s.step_n(120);
        assert!(pos(&s, 2)[0] > 6.0, "{:?}", pos(&s, 2));
        assert_eq!(pos(&s, 3), [20.0, 0.0, 0.0], "too far to be afraid");
    }

    fn flock(n: u32, seed_x: f32) -> Sim {
        flock_with(n, seed_x, 1.0)
    }

    fn flock_with(n: u32, seed_x: f32, alignment: f32) -> Sim {
        let mut s = Sim::new(1);
        for k in 0..n {
            // a jittered ring-ish scatter, deterministic and without a random generator
            let (a, r) = (k as f32 * 2.399963, 0.5 + (k as f32 * 0.37) % 3.0);
            let (x, z) = (seed_x + r * a.cos(), r * a.sin());
            spawn(&mut s, 100 + k as u64, &format!(r#"{{"Transform":{{"pos":[{x},0,{z}]}},"Velocity":{{"lin":[{},0,{}]}},"Steer":{{"group":"birds","radius":5,"personal":1.0,"max_speed":4,"wander":0.3,"alignment":{alignment}}}}}"#, (k % 3) as f32 - 1.0, (k % 5) as f32 * 0.3 - 0.6));
        }
        s
    }

    /// How alike the velocities are: the length of the mean velocity over the mean speed (1 is everybody the same way, 0 is no common direction).
    fn order(s: &Sim) -> f32 {
        let mean = (100..160u64).fold([0.0f32; 3], |m, u| [m[0] + vel(s, u)[0] / 60.0, 0.0, m[2] + vel(s, u)[2] / 60.0]);
        let mean_speed = (100..160u64).map(|u| (vel(s, u)[0].powi(2) + vel(s, u)[2].powi(2)).sqrt()).sum::<f32>() / 60.0;
        (mean[0] * mean[0] + mean[2] * mean[2]).sqrt() / mean_speed.max(1e-3)
    }

    #[test]
    fn a_flock_keeps_apart_and_together_and_heads_the_same_way() {
        let mut s = flock(60, 0.0);
        s.step_n(600);
        let ids: Vec<u64> = (100..160).collect();
        // nobody sits on top of anybody
        let mut closest = f32::MAX;
        for (n, a) in ids.iter().enumerate() {
            for b in &ids[n + 1..] {
                closest = closest.min(dist(pos(&s, *a), pos(&s, *b)));
            }
        }
        assert!(closest > 0.1, "closest pair {closest}");
        // and on the whole each keeps about half of `personal` or more from its nearest mate
        let nearest: f32 = ids.iter().map(|a| ids.iter().filter(|b| *b != a).map(|b| dist(pos(&s, *a), pos(&s, *b))).fold(f32::MAX, f32::min)).sum::<f32>() / 60.0;
        assert!(nearest > 0.4, "mean nearest-neighbour distance {nearest}");
        // they stayed one flock: nobody wandered far from the centre of the rest
        let c = ids.iter().fold([0.0f32; 3], |m, u| [m[0] + pos(&s, *u)[0] / 60.0, 0.0, m[2] + pos(&s, *u)[2] / 60.0]);
        let far = ids.iter().map(|u| dist(pos(&s, *u), c)).fold(0.0f32, f32::max);
        assert!(far < 25.0, "the flock fell apart: {far}");
        // and they head more alike than a flock that does not try to (the same start, the alignment weight 0, then 4)
        let mut lazy = flock_with(60, 0.0, 0.0);
        let mut keen = flock_with(60, 0.0, 4.0);
        lazy.step_n(600);
        keen.step_n(600);
        let (lo, hi) = (order(&lazy), order(&keen));
        assert!(hi > lo + 0.1 && hi > 0.35, "alignment 4 gives order {hi}, alignment 0 gives {lo}");
    }

    #[test]
    fn the_same_flock_twice_is_the_same_and_other_groups_do_not_mix() {
        let (mut a, mut b) = (flock(40, 0.0), flock(40, 0.0));
        a.step_n(200);
        b.step_n(200);
        assert_eq!(a.world.state_hash(), b.world.state_hash());
        // a flock of another group in the same place does not push on the first
        let mut c = flock(20, 0.0);
        let mut d = flock(20, 0.0);
        spawn(&mut d, 500, r#"{"Transform":{"pos":[0,0,0]},"Velocity":{},"Steer":{"group":"fish","radius":5,"personal":3,"max_speed":4}}"#);
        c.step_n(100);
        d.step_n(100);
        assert_eq!(pos(&c, 105), pos(&d, 105), "a fish does not disturb the birds");
    }

    #[test]
    fn the_picture_plane_and_facing() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,5,0]}}"#);
        spawn(&mut s, 2, r#"{"Transform":{},"Velocity":{},"Steer":{"seek":1,"plane":1,"max_speed":3}}"#);
        s.step_n(90);
        let p = pos(&s, 2);
        assert!(p[1] > 1.0 && p[0].abs() < 1e-3 && p[2] == 0.0, "{p:?}");
        // facing: moving up the picture plane turns it a quarter turn about z (atan2(1, 0) = 90 degrees: z = sin 45, w = cos 45)
        let q = s.world.get::<Transform>(s.world.entity(2).unwrap()).unwrap().rot;
        assert!((q[2] - 0.7071).abs() < 0.02 && (q[3] - 0.7071).abs() < 0.02, "{q:?}");
        // on the ground, running along -z is no turn at all
        let mut g = Sim::new(1);
        spawn(&mut g, 1, r#"{"Transform":{"pos":[0,0,-8]}}"#);
        spawn(&mut g, 2, r#"{"Transform":{},"Velocity":{},"Steer":{"seek":1,"max_speed":3}}"#);
        g.step_n(60);
        let q = g.world.get::<Transform>(g.world.entity(2).unwrap()).unwrap().rot;
        assert!(q[1].abs() < 0.02 && (q[3] - 1.0).abs() < 0.02, "{q:?}");
    }

    #[test]
    fn bounds_keep_a_wanderer_in_its_field() {
        let mut s = Sim::new(1);
        for k in 0..12u64 {
            spawn(&mut s, 10 + k, &format!(r#"{{"Transform":{{"pos":[{},0,{}]}},"Velocity":{{}},"Steer":{{"wander":1,"max_speed":3,"bounds":[-6,-4,6,4],"edge":2}}}}"#, k as f32 - 6.0, (k % 3) as f32 - 1.0));
        }
        let mut worst = 0.0f32;
        for _ in 0..1200 {
            s.step();
            for k in 0..12u64 {
                let p = pos(&s, 10 + k);
                worst = worst.max(p[0].abs() - 6.0).max(p[2].abs() - 4.0);
            }
        }
        assert!(worst < 0.8, "the furthest any got beyond the fence: {worst}");
        // and they do use the field
        let spread = (0..12u64).map(|k| pos(&s, 10 + k)[0].abs()).fold(0.0f32, f32::max);
        assert!(spread > 1.0, "{spread}");
    }

    #[test]
    fn wander_moves_without_a_goal_and_inactive_agents_are_left_alone() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{},"Velocity":{},"Steer":{"wander":1,"max_speed":2}}"#);
        spawn(&mut s, 2, r#"{"Transform":{},"Velocity":{},"Steer":{"wander":1,"max_speed":2,"active":false}}"#);
        s.step_n(300);
        assert!(dist(pos(&s, 1), [0.0; 3]) > 1.0);
        assert!((vel(&s, 1)[0].powi(2) + vel(&s, 1)[2].powi(2)).sqrt() <= 2.001);
        assert_eq!(pos(&s, 2), [0.0; 3]);
    }

    #[test]
    fn a_thousand_agents_are_cheap() {
        let mut s = flock(1000, 0.0);
        let t = std::time::Instant::now();
        s.step_n(30);
        let ms = t.elapsed().as_secs_f64() * 1000.0 / 30.0;
        println!("1000 flocking agents: {ms:.2} ms a tick");
        assert!(ms < 40.0, "{ms} ms a tick");
    }
}
