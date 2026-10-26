//! Stealth (docs/STEALTH.md): how loud a character is and how hard it is to see. `Stealth` measures how fast the character moves and keeps a `Noisy` on it (what an AI's hearing reacts to) and a `visibility`
//! (what a `Perception` multiplies its sight range by, for this target). Crouching quietens and hides; standing still hides a little; darkness hides. It draws and plays nothing.
//!
//! Deterministic: characters are visited in uid order and the smoothing goes through `detmath`.
use crate::components::{Noisy, Stealth, Transform};
use crate::detmath;
use lunaris_ecs::{Entity, World};

type V3 = [f32; 3];

/// A character that moves faster than this between two ticks was put somewhere (a respawn): it is not a speed.
const TELEPORT: f32 = 40.0;
/// How long the speed takes to follow what the character does (seconds).
const SMOOTH: f32 = 0.2;

fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

/// The noise radius for a speed: 0 when still, rising in straight lines through the walk, the run and the sprint.
fn noise_of(s: &Stealth, speed: f32) -> f32 {
    let (w, r, p) = (finite(s.walk_speed, 2.0).max(0.1), finite(s.run_speed, 5.0).max(0.2), finite(s.sprint_speed, 8.0).max(0.3));
    let (nw, nr, np) = (finite(s.walk_noise, 5.0).max(0.0), finite(s.run_noise, 14.0).max(0.0), finite(s.sprint_noise, 22.0).max(0.0));
    let (r, p) = (r.max(w + 0.1), p.max(r.max(w + 0.1) + 0.1));
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t.clamp(0.0, 1.0);
    if speed < 0.3 {
        0.0
    } else if speed < w {
        lerp(0.0, nw, (speed - 0.3) / (w - 0.3).max(0.01))
    } else if speed < r {
        lerp(nw, nr, (speed - w) / (r - w))
    } else {
        lerp(nr, np, (speed - r) / (p - r))
    }
}

pub fn update(world: &mut World, dt: f32) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let mut list: Vec<(u64, Entity, Stealth, V3)> = Vec::new();
    let _ = world.for_each_uid::<(&Stealth, &Transform), _>(|e, uid, (s, t)| {
        if t.pos.iter().all(|v| v.is_finite()) {
            list.push((uid, e, *s, t.pos));
        }
    });
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.0);
    let k = 1.0 - detmath::exp(-(dt / SMOOTH) as f64) as f32;
    for (_, e, mut s, pos) in list {
        if !s.seen {
            s.last = pos;
            s.seen = true;
            s.speed = 0.0;
        }
        let inst = ((pos[0] - s.last[0]).powi(2) + (pos[2] - s.last[2]).powi(2)).sqrt() / dt;
        s.last = pos;
        let inst = if inst > TELEPORT { 0.0 } else { inst };
        s.speed += (inst - s.speed) * k;
        if s.speed < 0.02 {
            s.speed = 0.0;
        }
        let mut noise = noise_of(&s, s.speed);
        if s.crouch {
            noise *= finite(s.crouch_mult, 0.3).clamp(0.0, 1.0);
        }
        s.noise = noise;
        let motion = (s.speed / finite(s.run_speed, 5.0).max(0.2)).clamp(0.0, 1.0);
        let still = finite(s.still_mult, 0.7).clamp(0.0, 1.0);
        let min_light = finite(s.min_light, 0.4).clamp(0.0, 1.0);
        let mut vis = if s.crouch { finite(s.vis_crouch, 0.6).clamp(0.0, 1.0) } else { 1.0 };
        vis *= still + (1.0 - still) * motion;
        vis *= min_light + (1.0 - min_light) * finite(s.light, 1.0).clamp(0.0, 1.0);
        s.visibility = vis;
        match world.get_mut::<Noisy>(e) {
            Some(n) => n.radius = noise,
            None => {
                let _ = world.insert(e, Noisy { radius: noise });
            }
        }
        if let Some(w) = world.get_mut::<Stealth>(e) {
            *w = s;
        }
    }
}
