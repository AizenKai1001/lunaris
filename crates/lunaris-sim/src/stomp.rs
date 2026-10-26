//! The stomp (docs/WALKER.md): an area melee hit with a wind-up, for a machine too big to bite. A `Stomp` on a machine that has a `Perception` waits until the target it is alert to is within `trigger` metres, plants its feet (`phase` 1, the
//! wind-up: the telegraph that gives a player time to run) and, when the wind-up is over, hits everything hostile within `radius` metres of the spot in front of its feet (`reach`), once, at full damage in the middle and less toward the
//! edge. A character takes it through the worn gear of the zone it is hit in (docs/EQUIPMENT.md: the same `armor::absorb` a bullet goes through); anything that is not a character (a wall or a floor of the building system, a car) takes `structure`
//! times as much. Then the machine stands `recover` seconds, waits `cooldown`, and may do it again.
//!
//! This file also holds [`area_damage`], the one deterministic function that hurts what is within a radius of a point: the stomp uses it, and so does the splash of a cannon shell (`walker.rs`). It is not an explosion system (a blast with
//! knock-back, fire and line of sight is its own feature): it is the minimal "everything within R of P takes N" that a heavy machine needs.
//!
//! Deterministic: machines and victims are visited in uid order and nothing is random.
use crate::components::{HitPoints, Perception, Team, Transform};
use crate::physics_rot::{axes_of, normalize_q};
use crate::weapons::{targets, Shape};
use lunaris_ecs::{component, Entity, World};

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

component! {
    /// A machine's stomp (docs/WALKER.md), next to its `Perception` and `Team` (a machine with no team would hurt itself and its friends: give it one). While calm (`phase` 0) it counts `wait` down and, once it has run out and the
    /// target the `Perception` is alert to (state 2) stands within `trigger` metres (on the ground plane), the wind-up starts (`phase` 1, event `stomp_windup`) and lasts `windup` seconds: a game plays the raised foot and a ring on the
    /// ground meanwhile, and a player who runs out of the `radius` in that time is not hurt. At the end of it the machine hits every hostile thing with `HitPoints` whose shape is within `radius` metres of the point `reach` metres in front of
    /// its feet and not more than `height` metres above them: `damage` at the middle, falling in a straight line to `edge` times at the radius; a thing that is not a character takes `structure` times that. The hit goes through a character's worn gear in the
    /// zone `zone` (1 body, 3 legs) and the event `hit` (and `kill`) tells who was hurt, as for a bullet, then `stomp` (the machine, where it landed). `recover` seconds of standing still follow (`phase` 2), then `cooldown`. The
    /// machine's `Pursuit` holds still for phases 1 and 2 and its `Cannon` does not fire. A dead machine stomps no more. `timer` is the time left in the phase, `hits` the number hurt by the last stomp, `strikes` how many stomps in all,
    /// `dealt` the damage done in all (after the gear).
    pub struct Stomp {
        pub active: bool = true,
        pub radius: f32 = 4.0,
        pub reach: f32 = 1.0,
        pub trigger: f32 = 3.2,
        pub windup: f32 = 1.1,
        pub recover: f32 = 1.4,
        pub cooldown: f32 = 3.0,
        pub damage: f32 = 90.0,
        pub edge: f32 = 0.35,
        pub structure: f32 = 3.0,
        pub zone: u32 = 1,
        pub height: f32 = 3.0,
        pub phase: u32 = 0,
        pub timer: f32 = 0.0,
        pub wait: f32 = 0.0,
        pub hits: u32 = 0,
        pub strikes: u32 = 0,
        pub dealt: f32 = 0.0,
    }
}

fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

#[inline]
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}

/// The distance from the point `p` to the segment `a`-`b`.
fn dist_to_segment(p: V3, a: V3, b: V3) -> f32 {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let t = if l2 > 1.0e-12 { (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    len(sub(p, [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t]))
}

/// How far the point `p` is from the surface of a shape that can be shot (0 inside it).
pub(crate) fn dist_to_shape(shape: &Shape, p: V3) -> f32 {
    match *shape {
        Shape::Capsule(a, b, r) => (dist_to_segment(p, a, b) - r).max(0.0),
        Shape::Sphere(c, r) => (len(sub(p, c)) - r).max(0.0),
        Shape::Obb(c, ax, h) => {
            let d = sub(p, c);
            let q = [(dot(d, ax[0]).abs() - h[0]).max(0.0), (dot(d, ax[1]).abs() - h[1]).max(0.0), (dot(d, ax[2]).abs() - h[2]).max(0.0)];
            len(q)
        }
    }
}

/// The lowest and the highest point of a shape (to tell whether it is at the height of a stomp or a blast).
fn span_of(shape: &Shape) -> (f32, f32) {
    match *shape {
        Shape::Capsule(a, b, r) => (a[1].min(b[1]) - r, a[1].max(b[1]) + r),
        Shape::Sphere(c, r) => (c[1] - r, c[1] + r),
        Shape::Obb(c, ax, h) => {
            // (the box's reach up and down, whatever way it is turned)
            let e = (ax[0][1] * h[0]).abs() + (ax[1][1] * h[1]).abs() + (ax[2][1] * h[2]).abs();
            (c[1] - e, c[1] + e)
        }
    }
}

/// What [`area_damage`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Outcome {
    /// How many things were hurt.
    pub hits: u32,
    /// How many of them that hit brought to 0.
    pub kills: u32,
    /// The damage done in all, after worn gear.
    pub dealt: f32,
}

/// Take `amount` off `HitPoints` of the thing `victim`: true when this is what brought it to 0 (a thing in its grace period, or already dead, takes nothing). The same rule as a bullet's.
fn hurt(world: &mut World, victim: u64, amount: f32) -> bool {
    let Some(e) = world.entity(victim) else { return false };
    let Some(h) = world.get_mut::<HitPoints>(e) else { return false };
    if h.dead || h.guard || h.invulnerable > 0.0 || !(amount > 0.0) {
        return false;
    }
    let before = h.value;
    h.value = (h.value - amount).max(0.0);
    before > 0.0 && h.value <= 0.0
}

/// Hurt everything hostile to `team` (the things of another team, or of any team when `team` is 0) with `HitPoints` and a shape (a capsule of a `CharacterBody`, a `Collider`) that is within `radius` metres of `center` on its surface and
/// reaches into the height band `[center.y - below, center.y + above]`: `damage` at distance 0, falling in a straight line to `edge` times `damage` at `radius`. A character (a `CharacterBody`) takes it in `zone` (1 body, 3 legs) through its worn
/// gear; a thing that is not a character takes `structure` times as much and no gear. `skip` are uids that are never hurt (the thing that did it, the victim of a shell's direct hit). Emits `hit` for each thing hurt (at its nearest point to
/// `center`) and `kill` for each that died of it. Targets are visited in uid order. A negative or non-finite amount hurts nothing.
#[allow(clippy::too_many_arguments)]
pub fn area_damage(world: &mut World, events: &mut Events, center: V3, radius: f32, damage: f32, edge: f32, structure: f32, zone: u32, team: u32, skip: &[u64], below: f32, above: f32) -> Outcome {
    let mut out = Outcome::default();
    let (radius, damage) = (finite(radius, 0.0), finite(damage, 0.0));
    if !(radius > 0.0) || !(damage > 0.0) || !center.iter().all(|v| v.is_finite()) {
        return out;
    }
    let edge = finite(edge, 0.0).clamp(0.0, 1.0);
    let structure = finite(structure, 1.0).max(0.0);
    let (lo, hi) = (center[1] - below.max(0.0), center[1] + above.max(0.0));
    let found = targets(world);
    for tg in found.iter() {
        if skip.contains(&tg.uid) || (team != 0 && tg.team == team) {
            continue;
        }
        let (bottom, top) = span_of(&tg.shape);
        if top < lo || bottom > hi {
            continue;
        }
        let d = dist_to_shape(&tg.shape, [center[0], center[1].clamp(bottom, top), center[2]]);
        if d > radius {
            continue;
        }
        let k = 1.0 - (1.0 - edge) * (d / radius).clamp(0.0, 1.0);
        let mut dmg = damage * k * if tg.zones { 1.0 } else { structure };
        if tg.zones {
            dmg = crate::armor::absorb(world, tg.uid, dmg, zone, events);
        }
        let killed = hurt(world, tg.uid, dmg);
        // (where it was hit: the nearest point of the thing to the middle of the blow, taken as the middle of it for a character: what a hit marker or a splash of blood is put at)
        let at = tg.center();
        events.push(("hit".to_string(), tg.uid, Some(at)));
        out.hits += 1;
        out.dealt += dmg;
        if killed {
            out.kills += 1;
            events.push(("kill".to_string(), tg.uid, Some(at)));
        }
    }
    out
}

/// The point on the ground `reach` metres in front of an entity (in the way it faces).
fn spot(t: &Transform, reach: f32) -> V3 {
    let ax = axes_of(normalize_q(t.rot));
    let (fx, fz) = (-ax[2][0], -ax[2][2]);
    let l = (fx * fx + fz * fz).sqrt();
    if l < 1.0e-6 {
        return t.pos;
    }
    [t.pos[0] + fx / l * reach, t.pos[1], t.pos[2] + fz / l * reach]
}

pub fn update(world: &mut World, dt: f32, events: &mut Events) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let mut found: Vec<(Entity, u64, Stomp, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&Stomp, &Transform), _>(|e, uid, (s, t)| found.push((e, uid, *s, *t)));
    if found.is_empty() {
        return;
    }
    found.sort_by_key(|f| f.1);
    for (e, uid, mut s, t) in found {
        if !s.active || world.get::<HitPoints>(e).is_some_and(|h| h.dead) {
            if s.phase != 0 {
                s.phase = 0;
                s.timer = 0.0;
                if let Some(w) = world.get_mut::<Stomp>(e) {
                    *w = s;
                }
            }
            continue;
        }
        let team = world.get::<Team>(e).map_or(0, |x| x.id);
        match s.phase {
            1 => {
                s.timer -= dt;
                if s.timer <= 0.0 {
                    // the foot comes down: everything near it is hurt (the machine's own side and the machine itself are not)
                    let at = spot(&t, finite(s.reach, 0.0));
                    let o = area_damage(world, events, at, s.radius, s.damage, s.edge, s.structure, s.zone, team, &[uid], 1.0, finite(s.height, 3.0));
                    s.hits = o.hits;
                    s.dealt += o.dealt;
                    s.strikes = s.strikes.wrapping_add(1);
                    s.phase = 2;
                    s.timer = finite(s.recover, 0.0).max(0.0);
                    events.push(("stomp".to_string(), uid, Some(at)));
                }
            }
            2 => {
                s.timer -= dt;
                if s.timer <= 0.0 {
                    s.phase = 0;
                    s.timer = 0.0;
                    s.wait = finite(s.cooldown, 0.0).max(0.0);
                }
            }
            _ => {
                s.wait = (s.wait - dt).max(0.0);
                if s.wait <= 0.0 {
                    // the target it is alert to is near: it plants its feet
                    let near = world.get::<Perception>(e).is_some_and(|p| {
                        let (dx, dz) = (p.seen_pos[0] - t.pos[0], p.seen_pos[2] - t.pos[2]);
                        p.active && p.state == 2 && p.visible && (dx * dx + dz * dz).sqrt() <= finite(s.trigger, 0.0)
                    });
                    if near {
                        s.phase = 1;
                        s.timer = finite(s.windup, 0.0).max(0.0);
                        events.push(("stomp_windup".to_string(), uid, Some(t.pos)));
                    }
                }
            }
        }
        if let Some(w) = world.get_mut::<Stomp>(e) {
            *w = s;
        }
    }
}
