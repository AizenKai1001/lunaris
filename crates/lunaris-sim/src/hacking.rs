//! Hacking and taking control of machines (docs/AI.md): a `Hacker` (the goggles of a player) hacks a `Hackable` machine that it aims at, within range and in line of sight, leaves a trace (heat) while it
//! does, and at 100 percent takes the machine: the machine joins its team, its own AI goes quiet, the hacker's `move_dir` and `fire` drive it, a camera follows it, and when the control ends (released,
//! timed out, destroyed) everything is put back. It makes no picture of the hack and plays no sound: the goggles' interface, the drone's body and the hacking effects are the game's.
//!
//! A machine whose `Hackable.mode` is 1 (a turret) is not taken but **turned**: it joins the hacker's team for `duration` seconds with its own AI running, and the hacker stays free; [`run_down_turned`] ends it.
//!
//! Deterministic: hackers are visited in uid order and nothing is random.
use crate::components::{CameraRig, CharacterBody, FlyAgent, Hackable, Hacker, Heat, HitPoints, NavAgent, Perception, Team, Transform, Velocity, Weapon};
use crate::layers::Layers;
use crate::meshcol::MeshStore;
use crate::perception::clear;
use crate::terrain::Terrains;
use crate::vehicle_ray::RayWorld;
use lunaris_ecs::{Entity, Uid, World};
use std::collections::BTreeSet;

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

/// How high above its feet a hacker's goggles are (the line of sight starts there).
const EYE: f32 = 1.6;

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(a: V3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

fn dead(world: &World, e: Entity) -> bool {
    world.get::<HitPoints>(e).is_some_and(|h| h.dead)
}

/// Give a machine back: its team and its AI as they were (a hack that `keep`s it leaves it on the hacker's side), nothing held, the camera home.
fn end_control(world: &mut World, machine: u64, h: &mut Hacker, name: &str, events: &mut Events) {
    let mut at = None;
    if let Some(de) = world.entity(machine) {
        let k = world.get::<Hackable>(de).copied();
        at = world.get::<Transform>(de).map(|t| t.pos);
        if let Some(k) = k {
            if let Some(p) = world.get_mut::<Perception>(de) {
                p.engage = k.saved_engage;
                p.active = k.saved_perception;
            }
            if let Some(f) = world.get_mut::<FlyAgent>(de) {
                f.active = k.saved_fly;
            }
            if !k.keep {
                if let Some(t) = world.get_mut::<Team>(de) {
                    t.id = k.saved_team;
                }
            }
        }
        if let Some(kw) = world.get_mut::<Hackable>(de) {
            kw.hacked = false;
            kw.controller = Uid(0);
            kw.progress = 0.0;
            kw.time_left = 0.0;
        }
        if let Some(w) = world.get_mut::<Weapon>(de) {
            w.fire = false;
        }
        if let Some(cb) = world.get_mut::<CharacterBody>(de) {
            cb.walk = [0.0, 0.0];
        }
    }
    if h.camera.0 != 0 {
        if let Some(ce) = world.entity(h.camera.0) {
            if let Some(rig) = world.get_mut::<CameraRig>(ce) {
                rig.target = h.saved_view;
            }
        }
    }
    h.state = 0;
    h.controlled = Uid(0);
    h.progress = 0.0;
    h.saved_view = Uid(0);
    // (a hack key that is still held does not start another hack at once)
    h.blocked = true;
    events.push((name.to_string(), machine, at));
}

/// A machine forgets what it was after (its target, its awareness) and lets go of its trigger.
fn forget(world: &mut World, machine: Entity) {
    if let Some(p) = world.get_mut::<Perception>(machine) {
        p.target = Uid(0);
        p.awareness = 0.0;
        p.state = 0;
        p.visible = false;
        p.seen_count = 0;
    }
    if let Some(w) = world.get_mut::<Weapon>(machine) {
        w.fire = false;
    }
}

/// A turned machine goes back to its own side (a hack that `keep`s it leaves it on the hacker's), and forgets whom it was shooting at.
fn end_turn(world: &mut World, e: Entity, uid: u64, events: &mut Events) {
    let k = world.get::<Hackable>(e).copied();
    let at = world.get::<Transform>(e).map(|t| t.pos);
    if let Some(k) = k {
        if !k.keep {
            if let Some(t) = world.get_mut::<Team>(e) {
                t.id = k.saved_team;
            }
        }
    }
    if let Some(kw) = world.get_mut::<Hackable>(e) {
        kw.hacked = false;
        kw.controller = Uid(0);
        kw.progress = 0.0;
        kw.time_left = 0.0;
    }
    forget(world, e);
    events.push(("link_lost".to_string(), uid, at));
}

/// The time of every turned machine (`Hackable.mode` 1) runs down; at 0, or when it is destroyed, it goes back to its own side. A hacker who dies or leaves does not end it (a defence that goes on).
fn run_down_turned(world: &mut World, dt: f32, events: &mut Events) {
    let mut turned: Vec<(Entity, u64)> = Vec::new();
    let _ = world.for_each_uid::<(&Hackable,), _>(|e, uid, (k,)| {
        if k.hacked && k.mode == 1 {
            turned.push((e, uid));
        }
    });
    turned.sort_by_key(|t| t.1);
    for (e, uid) in turned {
        let mut over = dead(world, e);
        if let Some(k) = world.get_mut::<Hackable>(e) {
            if k.duration > 0.0 {
                k.time_left -= dt;
                over |= k.time_left <= 0.0;
            }
        }
        if over {
            end_turn(world, e, uid, events);
        }
    }
}

/// The machine is taken: it joins the hacker's team, its AI stops, the camera turns to it.
fn take(world: &mut World, machine: Entity, machine_uid: u64, hacker_uid: u64, hacker_team: u32, h: &mut Hacker, events: &mut Events) {
    let team = world.get::<Team>(machine).map_or(0, |t| t.id);
    let (engage, active) = world.get::<Perception>(machine).map_or((false, true), |p| (p.engage, p.active));
    let fly = world.get::<FlyAgent>(machine).is_none_or(|f| f.active);
    let turn = world.get::<Hackable>(machine).is_some_and(|k| k.mode == 1);
    if let Some(k) = world.get_mut::<Hackable>(machine) {
        k.hacked = true;
        k.controller = Uid(hacker_uid);
        k.time_left = k.duration;
        k.progress = 1.0;
        k.saved_team = team;
        k.saved_engage = engage;
        k.saved_perception = active;
        k.saved_fly = fly;
    }
    if turn {
        // a turret is turned, not taken: its senses and its gun go on, for the hacker's side now; what it was shooting at is forgotten (it may have been the hacker's own side)
        forget(world, machine);
        if world.get::<Team>(machine).is_some() {
            if let Some(t) = world.get_mut::<Team>(machine) {
                t.id = hacker_team;
            }
        } else {
            let _ = world.insert(machine, Team { id: hacker_team });
        }
        // (the hacker is free: no control, no camera; the hack key that is still held does not start another hack)
        h.state = 0;
        h.progress = 0.0;
        h.blocked = true;
        let at = world.get::<Transform>(machine).map(|t| t.pos);
        events.push(("hacked".to_string(), machine_uid, at));
        return;
    }
    // (a flying machine that is taken stops flying itself: the hacker flies it)
    if let Some(f) = world.get_mut::<FlyAgent>(machine) {
        f.active = false;
    }
    if let Some(p) = world.get_mut::<Perception>(machine) {
        p.engage = false;
        p.active = false;
        p.state = 0;
        p.awareness = 0.0;
    }
    if let Some(n) = world.get_mut::<NavAgent>(machine) {
        n.active = false;
    }
    if let Some(w) = world.get_mut::<Weapon>(machine) {
        w.fire = false;
    }
    if world.get::<Team>(machine).is_some() {
        if let Some(t) = world.get_mut::<Team>(machine) {
            t.id = hacker_team;
        }
    } else {
        let _ = world.insert(machine, Team { id: hacker_team });
    }
    if h.camera.0 != 0 {
        if let Some(ce) = world.entity(h.camera.0) {
            if let Some(rig) = world.get_mut::<CameraRig>(ce) {
                h.saved_view = rig.target;
                rig.target = Uid(machine_uid);
            }
        }
    }
    h.state = 2;
    h.controlled = Uid(machine_uid);
    h.progress = 1.0;
    let at = world.get::<Transform>(machine).map(|t| t.pos);
    events.push(("hacked".to_string(), machine_uid, at));
}

pub fn update(world: &mut World, dt: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let mut hackers: Vec<(Entity, u64, Hacker, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&Hacker, &Transform), _>(|e, uid, (h, t)| hackers.push((e, uid, *h, *t)));
    hackers.sort_by_key(|h| h.1);
    // a hacker that aims chooses its own target: the machine nearest to where it looks (the machines are looked at once, and only if one of them aims)
    if hackers.iter().any(|h| h.2.aim && h.2.active && h.2.hack && h.2.state != 2) {
        let mut found: Vec<(Entity, u64, f32, V3)> = Vec::new();
        let _ = world.for_each_uid::<(&Hackable, &Transform), _>(|e, uid, (k, t)| {
            if k.active && !k.hacked {
                found.push((e, uid, finite(k.range, 25.0), t.pos));
            }
        });
        found.sort_by_key(|f| f.1);
        let machines: Vec<(u64, f32, V3)> = found.into_iter().filter(|f| !dead(world, f.0)).map(|f| (f.1, f.2, f.3)).collect();
        for (_, _, h, t) in hackers.iter_mut() {
            if !(h.aim && h.active && h.hack && h.state != 2) {
                continue;
            }
            let look = h.look.map(|v| finite(v, 0.0));
            let l = len(look);
            if l < 1.0e-6 {
                continue;
            }
            let look = [look[0] / l, look[1] / l, look[2] / l];
            let cos_min = crate::detmath::cos((finite(h.aim_cone, 4.0).clamp(0.0, 89.0) as f64).to_radians()) as f32;
            let eye = [t.pos[0], t.pos[1] + EYE, t.pos[2]];
            let reach = finite(h.range, 30.0);
            let mut best: Option<(f32, u64)> = None;
            for (uid, range, pos) in &machines {
                let d = sub(*pos, eye);
                let dist = len(d);
                if dist < 1.0e-4 || dist > reach.min(*range) {
                    continue;
                }
                let c = (d[0] * look[0] + d[1] * look[1] + d[2] * look[2]) / dist;
                if c >= cos_min && best.is_none_or(|b| c > b.0) {
                    best = Some((c, *uid));
                }
            }
            h.target = Uid(best.map_or(0, |b| b.1));
        }
    }
    let mut being_hacked: BTreeSet<u64> = BTreeSet::new();
    if !hackers.is_empty() {
        // the static world, if a line of sight has to be checked this tick
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for (_, _, h, t) in &hackers {
            if !(h.active && h.state != 2 && h.hack && h.target.0 != 0) {
                continue;
            }
            let Some(te) = world.entity(h.target.0) else { continue };
            if world.get::<Hackable>(te).is_some_and(|k| k.los) {
                if let Some(tt) = world.get::<Transform>(te) {
                    for k in 0..3 {
                        lo[k] = lo[k].min(t.pos[k].min(tt.pos[k]) - 1.0);
                        hi[k] = hi[k].max(t.pos[k].max(tt.pos[k]) + 2.0);
                    }
                }
            }
        }
        let rw = lo[0].is_finite().then(|| RayWorld::gather_except(world, fields, meshes, Some((lo, hi)), &BTreeSet::new()));
        let layers = Layers::gather(world);

        for (e, uid, mut h, t) in hackers {
            h.in_range = false;
            if !h.hack {
                h.blocked = false;
            }
            let team = world.get::<Team>(e).map_or(0, |x| x.id);
            if !h.active {
                if h.state == 2 {
                    let m = h.controlled.0;
                    end_control(world, m, &mut h, "link_lost", events);
                }
            } else if h.state == 2 {
                let machine = h.controlled.0;
                let me = world.entity(machine).filter(|me| world.get::<Hackable>(*me).is_some_and(|k| k.hacked && k.controller.0 == uid) && !dead(world, *me));
                match me {
                    None => end_control(world, machine, &mut h, "link_lost", events),
                    Some(_) if dead(world, e) => end_control(world, machine, &mut h, "link_lost", events),
                    Some(_) if h.release => end_control(world, machine, &mut h, "released", events),
                    Some(me) => {
                        let mut lost = false;
                        let (mut speed, mut accel) = (8.0, 20.0);
                        if let Some(k) = world.get_mut::<Hackable>(me) {
                            speed = finite(k.speed, 8.0).max(0.0);
                            accel = finite(k.accel, 20.0).max(0.0);
                            if k.duration > 0.0 {
                                k.time_left -= dt;
                                lost = k.time_left <= 0.0;
                            }
                        }
                        if lost {
                            end_control(world, machine, &mut h, "link_lost", events);
                        } else {
                            // drive it: the direction the hacker asks for, at most the machine's speed
                            let want = [finite(h.move_dir[0], 0.0), finite(h.move_dir[1], 0.0), finite(h.move_dir[2], 0.0)];
                            let l = len(want);
                            let k = if l > 1.0 { 1.0 / l } else { 1.0 };
                            let desired = [want[0] * k * speed, want[1] * k * speed, want[2] * k * speed];
                            if let Some(cb) = world.get_mut::<CharacterBody>(me) {
                                cb.walk = [desired[0], desired[2]];
                            } else if let Some(v) = world.get_mut::<Velocity>(me) {
                                let step = accel * dt;
                                for a in 0..3 {
                                    v.lin[a] += (desired[a] - v.lin[a]).clamp(-step, step);
                                }
                            }
                            if let Some(w) = world.get_mut::<Weapon>(me) {
                                w.fire = h.fire;
                            }
                        }
                    }
                }
            } else if h.hack && !h.blocked && h.target.0 != 0 {
                let started = h.state == 0;
                h.state = 0;
                if let Some(me) = world.entity(h.target.0) {
                    let k = world.get::<Hackable>(me).copied();
                    if let (Some(k), Some(tt)) = (k, world.get::<Transform>(me).map(|x| *x)) {
                        if k.active && !k.hacked && !dead(world, me) {
                            let dist = len(sub(tt.pos, t.pos));
                            let mut ok = dist <= finite(h.range, 30.0).min(finite(k.range, 25.0));
                            if ok && k.los {
                                let eye = [t.pos[0], t.pos[1] + EYE, t.pos[2]];
                                ok = clear(&rw, layers.of(uid), h.target.0, eye, tt.pos);
                            }
                            h.in_range = ok;
                            if ok {
                                h.state = 1;
                                being_hacked.insert(h.target.0);
                                let progress = (k.progress + dt * finite(h.speed, 1.0).max(0.0) / finite(k.difficulty, 4.0).max(0.05)).min(1.0);
                                if let Some(kw) = world.get_mut::<Hackable>(me) {
                                    kw.progress = progress;
                                }
                                h.progress = progress;
                                if let Some(heat) = world.get_mut::<Heat>(e) {
                                    heat.value = (heat.value + finite(h.trace_heat, 0.0).max(0.0) * dt).min(10.0);
                                }
                                if started {
                                    events.push(("hack_start".to_string(), uid, Some(tt.pos)));
                                }
                                if progress >= 1.0 {
                                    let target = h.target.0;
                                    take(world, me, target, uid, team, &mut h, events);
                                }
                            }
                        }
                    }
                }
            } else {
                h.state = 0;
            }
            if h.state == 0 {
                h.progress = world.entity(h.target.0).and_then(|me| world.get::<Hackable>(me)).map_or(0.0, |k| if k.hacked { 0.0 } else { k.progress });
            }
            h.release = false;
            if let Some(w) = world.get_mut::<Hacker>(e) {
                *w = h;
            }
        }
    }
    run_down_turned(world, dt, events);
    // a machine nobody is hacking recovers
    let mut idle: Vec<(Entity, u64, f32, f32)> = Vec::new();
    let _ = world.for_each_uid::<(&Hackable,), _>(|e, uid, (k,)| {
        if !k.hacked && k.progress > 0.0 {
            idle.push((e, uid, k.progress, k.difficulty));
        }
    });
    for (e, uid, progress, difficulty) in idle {
        if being_hacked.contains(&uid) {
            continue;
        }
        if let Some(k) = world.get_mut::<Hackable>(e) {
            k.progress = (progress - 0.5 * dt / finite(difficulty, 4.0).max(0.05)).max(0.0);
        }
    }
}
