//! The senses of an AI and the searching beam (docs/AI.md): `Perception` (sight with line of sight, hearing, awareness that fills and drains, memory of where the target was, an alert shouted to
//! the squad, and an optional autopilot that chases with a `NavAgent`, turns and holds a `Weapon`'s trigger), `Noisy`, and `Searchlight` with the `Exposure` it puts on what it lights. It
//! decides nothing about what a game's machines are or do: it gives a behaviour tree, a state machine or a graph system the facts (`Perception.state`, `target`, `seen_pos`, `Exposure.detected`)
//! and, for the simple case, does the chasing and shooting itself. It draws and plays nothing.
//!
//! Deterministic: AIs, lights, targets and noises are visited in uid order and nothing is random.
use crate::components::{CharacterBody, Exposure, Hackable, NavAgent, Noisy, Perception, Searchlight, Team, Transform, Weapon};
use crate::detmath;
use crate::layers::{Filter, Layers};
use crate::meshcol::MeshStore;
use crate::physics_rot::{axes_of, normalize_q};
use crate::terrain::Terrains;
use crate::vehicle_ray::RayWorld;
use crate::weapons::targets;
use lunaris_ecs::{Entity, Uid, World};
use std::collections::{BTreeMap, BTreeSet};

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

#[inline]
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}

/// `a` and `b` are on opposing sides: other teams, or any when `mine` is 0 (no team is hostile to everybody, itself included).
fn hostile(mine: u32, theirs: u32) -> bool {
    theirs != mine || mine == 0
}

fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// A turn about the vertical axis (a quarter turn to the left is positive).
fn yaw_quat(yaw: f32) -> [f32; 4] {
    let h = yaw as f64 * 0.5;
    [0.0, detmath::sin(h) as f32, 0.0, detmath::cos(h) as f32]
}

/// The rotation that makes local -z point along `dir`, level (yaw only) or tilted up and down as well.
pub(crate) fn look_rot(dir: V3, tilt: bool) -> [f32; 4] {
    let h = (dir[0] * dir[0] + dir[2] * dir[2]).sqrt();
    let yaw = detmath::atan2(-(dir[0] as f64), -(dir[2] as f64)) as f32;
    let q = yaw_quat(yaw);
    if !tilt {
        return q;
    }
    let pitch = detmath::atan2(dir[1] as f64, h as f64) * 0.5;
    quat_mul(q, [detmath::sin(pitch) as f32, 0.0, 0.0, detmath::cos(pitch) as f32])
}

/// Can a ray from `from` to `to` pass? (`rw` is the static world without the things that can be shot.)
pub(crate) fn clear(rw: &Option<RayWorld>, who: Filter, skip: u64, from: V3, to: V3) -> bool {
    let Some(rw) = rw else { return true };
    let d = sub(to, from);
    let dist = len(d);
    if dist < 1.0e-4 {
        return true;
    }
    let dir = [d[0] / dist, d[1] / dist, d[2] / dist];
    rw.cast(from, dir, dist, who, skip).is_none_or(|h| h.t >= dist - 0.25)
}

fn team_of(world: &World, e: Entity) -> u32 {
    world.get::<Team>(e).map_or(0, |x| x.id)
}

struct Ai {
    e: Entity,
    uid: u64,
    p: Perception,
    t: Transform,
    team: u32,
    /// The hacker a turned machine (`Hackable.mode` 1) serves: never a target, whatever its team (a hacker with no party has team 0, which is hostile to everybody).
    friend: u64,
}

struct Light {
    e: Entity,
    uid: u64,
    s: Searchlight,
    t: Transform,
    team: u32,
}

struct Noise {
    source: u64,
    pos: V3,
    radius: f32,
    team: u32,
}

pub fn update(world: &mut World, dt: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let mut ais: Vec<Ai> = Vec::new();
    let mut raw: Vec<(Entity, u64, Perception, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&Perception, &Transform), _>(|e, uid, (p, t)| raw.push((e, uid, *p, *t)));
    raw.sort_by_key(|r| r.1);
    for (e, uid, p, t) in raw {
        let team = team_of(world, e);
        let friend = world.get::<Hackable>(e).filter(|k| k.hacked && k.mode == 1).map_or(0, |k| k.controller.0);
        ais.push(Ai { e, uid, p, t, team, friend });
    }
    let mut lights: Vec<Light> = Vec::new();
    let mut raw: Vec<(Entity, u64, Searchlight, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&Searchlight, &Transform), _>(|e, uid, (s, t)| raw.push((e, uid, *s, *t)));
    raw.sort_by_key(|r| r.1);
    for (e, uid, s, t) in raw {
        let team = team_of(world, e);
        lights.push(Light { e, uid, s, t, team });
    }
    if ais.is_empty() && lights.is_empty() {
        return;
    }

    let tgs = targets(world);
    let except: BTreeSet<u64> = tgs.iter().map(|t| t.uid).collect();
    let layers = Layers::gather(world);

    // what can be heard: things that make noise now, and the rounds fired this tick
    let mut noises: Vec<Noise> = Vec::new();
    if ais.iter().any(|a| a.p.active && a.p.hearing > 0.0) {
        let mut found: Vec<(Entity, u64, f32, V3)> = Vec::new();
        let _ = world.for_each_uid::<(&Noisy, &Transform), _>(|e, uid, (n, t)| {
            if n.radius > 0.0 && n.radius.is_finite() {
                found.push((e, uid, n.radius, t.pos));
            }
        });
        let mut shots: Vec<(Entity, u64, f32, V3, u64)> = Vec::new();
        let _ = world.for_each_uid::<(&Weapon, &Transform), _>(|e, uid, (w, t)| {
            if w.fired && w.noise > 0.0 && w.noise.is_finite() {
                shots.push((e, uid, w.noise, t.pos, w.owner.0));
            }
        });
        found.sort_by_key(|f| f.1);
        shots.sort_by_key(|f| f.1);
        for (e, uid, radius, pos) in found {
            noises.push(Noise { source: uid, pos, radius, team: team_of(world, e) });
        }
        for (e, uid, radius, pos, owner) in shots {
            let team = world.entity(owner).map_or_else(|| team_of(world, e), |oe| team_of(world, oe));
            noises.push(Noise { source: if owner != 0 { owner } else { uid }, pos, radius, team });
        }
    }

    // how much of an AI's sight a target has to be seen by (a crouched or still or hidden character: `Stealth.visibility`)
    let mut vis: Vec<(u64, f32)> = Vec::new();
    let _ = world.for_each_uid::<(&crate::components::Stealth,), _>(|_, uid, (s,)| vis.push((uid, if s.visibility.is_finite() { s.visibility.clamp(0.0, 1.0) } else { 1.0 })));
    vis.sort_by_key(|v| v.0);

    // the static world the sight lines pass through: only the part the senses can reach
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    let mut reach = |c: V3, r: f32| {
        for k in 0..3 {
            lo[k] = lo[k].min(c[k] - r);
            hi[k] = hi[k].max(c[k] + r);
        }
    };
    for a in &ais {
        if a.p.active && a.p.los && a.p.sight_range > 0.0 && a.p.sight_range.is_finite() {
            reach(a.t.pos, a.p.sight_range + a.p.sight_eye.max(0.0));
        }
    }
    for l in &lights {
        if l.s.active && l.s.los && l.s.range > 0.0 && l.s.range.is_finite() {
            reach(l.t.pos, l.s.range);
        }
    }
    let rw = (lo[0].is_finite() && !tgs.is_empty()).then(|| RayWorld::gather_except(world, fields, meshes, Some((lo, hi)), &except));

    // ---- the senses of every AI
    // (what an AI of a team may see: the targets of the other teams, or all of them for team 0, as lists of places in `tgs` in the order of `tgs`: the same ones in the same order as looking at all and skipping its own side, which in a crowd of machines is most of them)
    let mut sides: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for a in ais.iter().filter(|a| a.p.active) {
        sides.entry(a.team).or_insert_with(|| tgs.iter().enumerate().filter(|(_, tg)| hostile(a.team, tg.team)).map(|(n, _)| n as u32).collect());
    }
    let mut shouts: Vec<(usize, V3)> = Vec::new();
    for (i, a) in ais.iter_mut().enumerate() {
        let (uid, t) = (a.uid, a.t);
        let p = &mut a.p;
        let prev = p.state;
        p.heard = false;
        if !p.active {
            p.visible = false;
            p.seen_count = 0;
            continue;
        }
        let range = if p.sight_range.is_finite() { p.sight_range.max(0.0) } else { 0.0 };
        let eye = [t.pos[0], t.pos[1] + p.sight_eye.max(0.0), t.pos[2]];
        let ax = axes_of(normalize_q(t.rot));
        let fwd = [-ax[2][0], -ax[2][1], -ax[2][2]];
        let half_cos = if p.sight_fov >= 360.0 { -2.0 } else { detmath::cos((p.sight_fov.max(0.0) as f64 * 0.5).to_radians()) as f32 };
        let who = layers.of(uid);
        let mut best: Option<(f32, usize)> = None;
        let mut count = 0u32;
        let side: &[u32] = sides.get(&a.team).map_or(&[], |v| v.as_slice());
        for &n in side {
            let (n, tg) = (n as usize, &tgs[n as usize]);
            if tg.uid == uid || tg.uid == a.friend {
                continue;
            }
            let c = tg.center();
            let d = sub(c, eye);
            let dist = len(d);
            if dist > range || !dist.is_finite() {
                continue;
            }
            // (a thing that is hard to see is seen only from nearer; what is within 1.5 m is always noticed)
            if dist > 1.5 && !vis.is_empty() {
                let v = vis.binary_search_by_key(&tg.uid, |x| x.0).map_or(1.0, |i| vis[i].1);
                if dist > range * v {
                    continue;
                }
            }
            if dist > 1.5 && half_cos > -1.5 && dot(d, fwd) / dist < half_cos {
                continue;
            }
            if p.los && dist > 1.5 && !clear(&rw, who, uid, eye, c) {
                continue;
            }
            count += 1;
            if best.is_none_or(|b| dist < b.0) {
                best = Some((dist, n));
            }
        }
        p.seen_count = count;
        p.visible = best.is_some();
        if let Some((dist, n)) = best {
            let near = 1.0 - (dist / range.max(0.1)).clamp(0.0, 1.0);
            let rate = (1.0 + 2.0 * near) / p.notice_time.max(0.05);
            p.awareness = (p.awareness + rate * dt).min(1.0);
            let tg = &tgs[n];
            p.target = Uid(tg.uid);
            p.distance = dist;
            p.seen_pos = [tg.center()[0], tg.feet, tg.center()[2]];
            p.since_seen = 0.0;
        } else {
            p.since_seen = (p.since_seen + dt).min(1.0e6);
            if p.awareness > 0.0 {
                p.awareness = (p.awareness - dt / p.lose_time.max(0.05)).max(0.0);
            }
        }
        // hearing: the nearest noise of the other side that carries this far
        if p.hearing > 0.0 {
            let mut heard: Option<(f32, &Noise)> = None;
            for nz in &noises {
                if nz.source == uid || nz.source == a.friend || !hostile(a.team, nz.team) {
                    continue;
                }
                let dist = len(sub(nz.pos, t.pos));
                if dist <= nz.radius * p.hearing && heard.is_none_or(|h| dist < h.0) {
                    heard = Some((dist, nz));
                }
            }
            if let Some((_, nz)) = heard {
                p.heard = true;
                p.noise_pos = nz.pos;
                events.push(("heard".to_string(), uid, Some(nz.pos)));
                if !p.visible {
                    p.awareness = p.awareness.max(0.55);
                    p.seen_pos = [nz.pos[0], nz.pos[1], nz.pos[2]];
                    p.since_seen = 0.0;
                    p.target = Uid(nz.source);
                }
            }
        }
        p.state = if p.visible && p.awareness >= 1.0 {
            2
        } else if p.awareness > 0.0 {
            if p.visible {
                1
            } else {
                3
            }
        } else {
            0
        };
        if p.state == 2 && prev != 2 {
            events.push(("spotted".to_string(), uid, Some(p.seen_pos)));
            if p.shout > 0.0 {
                shouts.push((i, p.seen_pos));
            }
        } else if prev == 2 && p.state == 3 {
            events.push(("lost".to_string(), uid, Some(p.seen_pos)));
        } else if prev != 0 && p.state == 0 {
            events.push(("forgot".to_string(), uid, None));
        }
    }
    // an alert is shouted to the squad: the ones near enough start searching where the target is
    for (i, at) in shouts {
        let (team, pos, radius, target) = (ais[i].team, ais[i].t.pos, ais[i].p.shout, ais[i].p.target);
        if team == 0 {
            continue;
        }
        for (j, b) in ais.iter_mut().enumerate() {
            if j == i || !b.p.active || b.team != team || len(sub(b.t.pos, pos)) > radius || b.p.state == 2 {
                continue;
            }
            b.p.awareness = b.p.awareness.max(0.6);
            b.p.seen_pos = at;
            b.p.since_seen = 0.0;
            b.p.target = target;
            b.p.state = if b.p.visible { 1 } else { 3 };
        }
    }
    // ---- write them back, and act for the ones that engage
    for a in &ais {
        let (e, uid, t, p) = (a.e, a.uid, a.t, a.p);
        if let Some(w) = world.get_mut::<Perception>(e) {
            *w = p;
        }
        if !(p.engage && p.active) {
            continue;
        }
        if p.chase && (p.state == 2 || p.state == 3) {
            if let Some(na) = world.get_mut::<NavAgent>(e) {
                // a target it sees is approached to its attack range (not walked into: a body is solid); a place it only remembers is walked to
                let goal = if p.state == 2 && p.visible {
                    let (dx, dz) = (t.pos[0] - p.seen_pos[0], t.pos[2] - p.seen_pos[2]);
                    let flat = (dx * dx + dz * dz).sqrt();
                    if p.distance <= p.attack_range * 0.85 || flat < 1.0e-3 {
                        t.pos
                    } else {
                        // (the agent stops within `arrive` of its goal, so the ring is that much nearer: it ends inside its own range, as close as 0.45 of it)
                        let arrive = if na.arrive.is_finite() { na.arrive.max(0.0) } else { 0.0 };
                        let keep = (p.attack_range * 0.9 - arrive).max(p.attack_range * 0.45).min(flat);
                        [p.seen_pos[0] + dx / flat * keep, p.seen_pos[1], p.seen_pos[2] + dz / flat * keep]
                    }
                } else {
                    p.seen_pos
                };
                if len(sub(na.target, goal)) > 1.5 {
                    na.target = goal;
                    na.active = true;
                }
            }
        }
        if p.turn && p.visible && (p.state == 1 || p.state == 2) {
            let eye = [t.pos[0], t.pos[1] + p.sight_eye.max(0.0), t.pos[2]];
            let aim = tgs.binary_search_by_key(&p.target.0, |tg| tg.uid).ok().map(|k| tgs[k].center());
            if let Some(c) = aim {
                let d = sub(c, eye);
                if len(d) > 1.0e-4 {
                    let tilt = world.get::<CharacterBody>(e).is_none();
                    if let Some(tr) = world.get_mut::<Transform>(e) {
                        tr.rot = look_rot(d, tilt);
                    }
                }
            }
        }
        let wuid = if p.weapon.0 != 0 { p.weapon.0 } else { uid };
        if let Some(we) = world.entity(wuid) {
            if let Some(w) = world.get_mut::<Weapon>(we) {
                w.fire = p.state == 2 && p.visible && p.distance <= p.attack_range;
            }
        }
    }

    // ---- the beams
    if lights.is_empty() {
        return;
    }
    let mut held: BTreeMap<u64, u64> = BTreeMap::new();
    let mut counts: BTreeMap<u64, u32> = BTreeMap::new();
    let mut holds: BTreeMap<u64, f32> = BTreeMap::new();
    for l in lights.iter_mut() {
        if !l.s.active {
            l.s.lit = 0;
            l.s.target = Uid(0);
            l.s.exposure = 0.0;
            l.s.detected = false;
            continue;
        }
        if l.s.sweep != 0.0 && l.s.sweep.is_finite() {
            l.t.rot = normalize_q(quat_mul(yaw_quat((l.s.sweep as f64).to_radians() as f32 * dt), l.t.rot));
            if let Some(tr) = world.get_mut::<Transform>(l.e) {
                tr.rot = l.t.rot;
            }
        }
        holds.insert(l.uid, l.s.hold.max(0.05));
        let ax = axes_of(normalize_q(l.t.rot));
        let fwd = [-ax[2][0], -ax[2][1], -ax[2][2]];
        let cos = detmath::cos((l.s.angle.clamp(0.0, 89.0) as f64).to_radians()) as f32;
        let who = layers.of(l.uid);
        let mut n = 0u32;
        for tg in &tgs {
            if tg.uid == l.uid || !hostile(l.team, tg.team) {
                continue;
            }
            let c = tg.center();
            let d = sub(c, l.t.pos);
            let dist = len(d);
            if dist > l.s.range || dist < 1.0e-4 || dot(d, fwd) / dist < cos {
                continue;
            }
            if l.s.los && !clear(&rw, who, l.uid, l.t.pos, c) {
                continue;
            }
            n += 1;
            held.entry(tg.uid).or_insert(l.uid);
        }
        counts.insert(l.uid, n);
    }
    // the exposure of whatever is in a beam, or was
    for tg in &tgs {
        let Some(e) = world.entity(tg.uid) else { continue };
        let by = held.get(&tg.uid).copied();
        if by.is_none() && world.get::<Exposure>(e).is_none() {
            continue;
        }
        if world.get::<Exposure>(e).is_none() {
            let _ = world.insert(e, Exposure::default());
        }
        let Some(ex) = world.get_mut::<Exposure>(e) else { continue };
        let (was_lit, was_detected) = (ex.lit, ex.detected);
        match by {
            Some(light) => {
                ex.value += dt / holds.get(&light).copied().unwrap_or(1.5);
                ex.by = Uid(light);
            }
            None => ex.value -= ex.decay.max(0.0) * dt,
        }
        ex.value = ex.value.clamp(0.0, 1.0);
        ex.lit = by.is_some();
        ex.detected = ex.value >= 1.0;
        let pos = Some(tg.center());
        if ex.lit && !was_lit {
            events.push(("lit".to_string(), tg.uid, pos));
        } else if !ex.lit && was_lit {
            events.push(("unlit".to_string(), tg.uid, pos));
        }
        if ex.detected && !was_detected {
            events.push(("detected".to_string(), tg.uid, pos));
        }
    }
    // what each light reports
    for l in lights.iter_mut() {
        if l.s.active {
            l.s.lit = counts.get(&l.uid).copied().unwrap_or(0);
            let mut best = (0.0f32, 0u64, false);
            for (tuid, light) in &held {
                if *light != l.uid {
                    continue;
                }
                let v = world.entity(*tuid).and_then(|e| world.get::<Exposure>(e)).map_or(0.0, |x| x.value);
                if v > best.0 || best.1 == 0 {
                    best = (v, *tuid, v >= 1.0);
                }
            }
            l.s.target = Uid(best.1);
            l.s.exposure = best.0;
            l.s.detected = best.2;
        }
        if let Some(w) = world.get_mut::<Searchlight>(l.e) {
            *w = l.s;
        }
    }
}
