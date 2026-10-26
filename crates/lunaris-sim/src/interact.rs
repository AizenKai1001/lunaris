//! Interaction (docs/INTERACT.md): the use button. An `Interactor` (a character's eyes and its press of the use key) looks along its entity at an `Interactable` (a door, a chest, an item on the ground, a
//! switch) and uses it: a `Door` swings, a container opens for its user, an item goes into the user's bag, a locked thing wants its key. It makes no screen: the prompt text, the container's window and the sounds are
//! the game's, driven by the outputs and the events.
//!
//! Deterministic: interactors are visited in uid order, what is aimed at is the nearest by distance then uid, the doors turn by `detmath` angles.
use crate::components::{Collider, Door, Interactable, Interactor, LocalTransform, Parent, Transform};
use crate::detmath;
use crate::gameplay::despawn_trees;
use crate::inventory::{table_of, Bag, Inventory};
use crate::layers::Layers;
use crate::meshcol::MeshStore;
use crate::physics_rot::{axes_of, normalize_q, quat_mul, rotate};
use crate::terrain::Terrains;
use crate::vehicle_ray::{ray_capsule, ray_obb, ray_sphere, segment_box, RayWorld};
use lunaris_ecs::{Entity, Sym, Uid, World};

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len(a: V3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

// ------------------------------------------------------------------------------------------------ doors

/// Turn every `Door` towards open or shut.
fn doors(world: &mut World, dt: f32) {
    let mut list: Vec<(u64, Entity, Door)> = Vec::new();
    let _ = world.for_each_uid::<(&Door,), _>(|e, uid, (d,)| list.push((uid, e, *d)));
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.0);
    for (_, e, mut d) in list {
        let local = world.get::<Parent>(e).is_some() && world.get::<LocalTransform>(e).is_some();
        let (pos, rot) = if local {
            world.get::<LocalTransform>(e).map(|l| (l.pos, l.rot)).unwrap_or(([0.0; 3], [0.0, 0.0, 0.0, 1.0]))
        } else {
            match world.get::<Transform>(e) {
                Some(t) => (t.pos, t.rot),
                None => continue,
            }
        };
        if !d.based {
            // the pose it starts in is its shut pose
            d.base_pos = pos;
            d.base_rot = normalize_q(rot);
            d.based = true;
            d.angle = 0.0;
        }
        let open = finite(d.angle_open, 100.0);
        let target = if d.open { open } else { 0.0 };
        let step = finite(d.speed, 160.0).max(0.0) * dt;
        let before = d.angle;
        d.angle = finite(d.angle, 0.0);
        d.angle += (target - d.angle).clamp(-step, step);
        if d.angle != before || (d.angle == 0.0 && (pos != d.base_pos || rot != d.base_rot) && !d.open) {
            let axes = axes_of(d.base_rot);
            let hinge = add(d.base_pos, rotate(&axes, d.hinge));
            let mut axis = rotate(&axes, d.axis);
            let l = len(axis);
            if l < 1.0e-6 {
                axis = [0.0, 1.0, 0.0];
            } else {
                axis = [axis[0] / l, axis[1] / l, axis[2] / l];
            }
            let half = d.angle as f64 * (std::f64::consts::PI / 360.0);
            let (s, c) = (detmath::sin(half) as f32, detmath::cos(half) as f32);
            let q = [axis[0] * s, axis[1] * s, axis[2] * s, c];
            let new_rot = quat_mul(q, d.base_rot);
            let new_pos = add(hinge, rotate(&axes_of(q), sub(d.base_pos, hinge)));
            if local {
                if let Some(l) = world.get_mut::<LocalTransform>(e) {
                    l.pos = new_pos;
                    l.rot = new_rot;
                }
            } else if let Some(t) = world.get_mut::<Transform>(e) {
                t.pos = new_pos;
                t.rot = new_rot;
            }
        }
        if let Some(w) = world.get_mut::<Door>(e) {
            *w = d;
        }
    }
}

// ------------------------------------------------------------------------------------------------ the use button

struct Cand {
    uid: u64,
    e: Entity,
    i: Interactable,
    t: Transform,
    col: Option<Collider>,
}

/// Where the ray `o + t d` meets the thing, if it does.
fn shape_hit(c: &Cand, o: V3, d: V3) -> Option<f32> {
    let s = [c.t.scale[0].abs(), c.t.scale[1].abs(), c.t.scale[2].abs()];
    let axes = axes_of(normalize_q(c.t.rot));
    match c.col {
        Some(col) => match col.shape {
            0 => ray_sphere(o, d, c.t.pos, 0.5 * col.size[0].abs() * s[0].max(s[1]).max(s[2])),
            2 => {
                let r = 0.5 * col.size[0].abs() * s[0].max(s[2]);
                let half = (0.5 * col.size[1].abs() * s[1] - r).max(0.0);
                let a = [c.t.pos[0] - axes[1][0] * half, c.t.pos[1] - axes[1][1] * half, c.t.pos[2] - axes[1][2] * half];
                let b = [c.t.pos[0] + axes[1][0] * half, c.t.pos[1] + axes[1][1] * half, c.t.pos[2] + axes[1][2] * half];
                ray_capsule(o, d, a, b, r)
            }
            _ => ray_obb(o, d, c.t.pos, &axes, [0.5 * col.size[0].abs() * s[0], 0.5 * col.size[1].abs() * s[1], 0.5 * col.size[2].abs() * s[2]]),
        },
        None => ray_sphere(o, d, c.t.pos, finite(c.i.radius, 0.6).max(0.05)),
    }
    .map(|(t, _)| t)
}

/// Whose `Inventory` an interactor uses: the one it names, its own, or its parent's.
fn inventory_of(world: &World, ie: Entity, named: Uid) -> Option<Entity> {
    if named.0 != 0 {
        return world.entity(named.0).filter(|e| world.get::<Inventory>(*e).is_some());
    }
    if world.get::<Inventory>(ie).is_some() {
        return Some(ie);
    }
    let parent = world.get::<Parent>(ie)?.target.0;
    world.entity(parent).filter(|e| world.get::<Inventory>(*e).is_some())
}

fn shut_container(world: &mut World, container: u64, actor: u64, events: &mut Events) {
    let Some(ce) = world.entity(container) else { return };
    let pos = world.get::<Transform>(ce).map(|t| t.pos);
    if let Some(i) = world.get_mut::<Interactable>(ce) {
        if i.open && i.user.0 == actor {
            i.open = false;
            i.user = Uid(0);
            events.push(("container_close".to_string(), container, pos));
        }
    }
}

/// The interactor `iuid` uses `target`.
#[allow(clippy::too_many_arguments)]
fn use_it(world: &mut World, tick: u64, dt: f32, iuid: u64, ie: Entity, it: &mut Interactor, target: &Cand, events: &mut Events) {
    let mut i = target.i;
    let pos = Some(target.t.pos);
    if (tick as u32) < i.ready {
        return;
    }
    let inv = inventory_of(world, ie, it.inventory);
    // a code lock: only the players who know the code (docs/RAIDING.md)
    if let Some(lock) = world.get::<crate::lock::CodeLock>(target.e).copied() {
        let who = crate::lock::identity(world, inv.unwrap_or(ie));
        if !crate::lock::may_use(&lock, who) {
            it.denied = Sym(world.strings_mut().intern("locked"));
            events.push(("locked".to_string(), target.uid, pos));
            events.push(("lock_prompt".to_string(), iuid, pos));
            if let Some(w) = world.get_mut::<Interactable>(target.e) {
                w.ready = (tick as u32).saturating_add(((finite(i.cooldown, 0.3).max(0.0)) / dt).ceil() as u32);
            }
            return;
        }
    }
    // locked: a key, or no
    if i.locked {
        let key = world.strings().get(i.key.0).to_string();
        let mut opened = false;
        if !key.is_empty() {
            if let (Some((table, _)), Some(inv_e)) = (table_of(world), inv) {
                let id = table.id_of(&key);
                if id != 0 {
                    if let Some(mut inventory) = world.get::<Inventory>(inv_e).copied() {
                        let mut bag = Bag::of(&inventory);
                        if bag.total(id) > 0 {
                            if i.key_consume {
                                bag.remove(id, 1);
                                bag.store(&mut inventory);
                                if let Some(w) = world.get_mut::<Inventory>(inv_e) {
                                    *w = inventory;
                                }
                            }
                            opened = true;
                        }
                    }
                }
            }
        }
        if opened {
            i.locked = false;
            events.push(("unlocked".to_string(), target.uid, pos));
        } else {
            it.denied = Sym(world.strings_mut().intern("locked"));
            events.push(("locked".to_string(), target.uid, pos));
            if let Some(w) = world.get_mut::<Interactable>(target.e) {
                w.ready = (tick as u32).saturating_add(((finite(i.cooldown, 0.3).max(0.0)) / dt).ceil() as u32);
            }
            return;
        }
    }
    match i.kind {
        1 if world.get::<Door>(target.e).is_some() => {
            if let Some(d) = world.get_mut::<Door>(target.e) {
                d.open = !d.open;
                let name = if d.open { "door_open" } else { "door_close" };
                events.push((name.to_string(), target.uid, pos));
            }
        }
        2 => {
            if i.open && i.user.0 == iuid {
                i.open = false;
                i.user = Uid(0);
                it.open = Uid(0);
                events.push(("container_close".to_string(), target.uid, pos));
            } else {
                // (what the interactor had open before is shut)
                if it.open.0 != 0 && it.open.0 != target.uid {
                    shut_container(world, it.open.0, iuid, events);
                }
                i.open = true;
                i.user = Uid(iuid);
                it.open = Uid(target.uid);
                events.push(("container_open".to_string(), target.uid, pos));
            }
        }
        3 => {
            let text = world.strings().get(i.item.0).to_string();
            let mut words = text.split_whitespace();
            let key = words.next().unwrap_or("").to_string();
            let n: u32 = words.next().and_then(|w| w.parse().ok()).unwrap_or(1).max(1);
            let table = table_of(world).map(|(t, _)| t);
            let id = table.as_ref().map_or(0, |t| t.id_of(&key));
            match (table, inv, id) {
                (Some(table), Some(inv_e), id) if id != 0 => {
                    let mut left = n;
                    if let Some(mut inventory) = world.get::<Inventory>(inv_e).copied() {
                        let mut bag = Bag::of(&inventory);
                        left = bag.add(&table, id, n);
                        bag.store(&mut inventory);
                        if let Some(w) = world.get_mut::<Inventory>(inv_e) {
                            *w = inventory;
                        }
                    }
                    if left == 0 {
                        events.push(("picked".to_string(), target.uid, pos));
                        despawn_trees(world, vec![target.uid]);
                        it.used = Uid(target.uid);
                        events.push(("interact".to_string(), target.uid, pos));
                        return;
                    }
                    // some of it did not fit: what is left stays
                    let rest = format!("{key} {left}");
                    i.item = Sym(world.strings_mut().intern(&rest));
                    it.denied = Sym(world.strings_mut().intern("full"));
                    events.push(("inventory_full".to_string(), target.uid, pos));
                }
                _ => events.push(("used".to_string(), target.uid, pos)),
            }
        }
        _ => events.push(("used".to_string(), target.uid, pos)),
    }
    i.uses = i.uses.wrapping_add(1);
    i.ready = (tick as u32).saturating_add(((finite(i.cooldown, 0.3).max(0.0)) / dt).ceil() as u32);
    if i.kind != 2 {
        i.user = Uid(iuid);
    }
    it.used = Uid(target.uid);
    events.push(("interact".to_string(), target.uid, pos));
    if let Some(w) = world.get_mut::<Interactable>(target.e) {
        *w = i;
    }
}

pub fn update(world: &mut World, tick: u64, dt: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    doors(world, dt);
    let mut list: Vec<(Entity, u64, Interactor, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&Interactor, &Transform), _>(|e, uid, (i, t)| {
        if t.pos.iter().chain(t.rot.iter()).all(|v| v.is_finite()) {
            list.push((e, uid, *i, *t));
        }
    });
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.1);
    // who looks this tick (to show a prompt) or acts (a press)
    let wants: Vec<usize> = (0..list.len()).filter(|&k| list[k].2.look || (list[k].2.act && !list[k].2.held)).collect();
    // open containers: shut when the user has walked off or the thing is gone
    for (e, uid, it, t) in list.iter_mut() {
        it.used = Uid(0);
        it.denied = Sym(0);
        if it.open.0 != 0 {
            let far = match world.entity(it.open.0) {
                None => true,
                Some(ce) => {
                    let (range, pos) = (world.get::<Interactable>(ce).map_or(3.0, |i| finite(i.range, 3.0)), world.get::<Transform>(ce).map_or(t.pos, |x| x.pos));
                    len(sub(pos, t.pos)) > range + 1.0
                }
            };
            if far {
                shut_container(world, it.open.0, *uid, events);
                it.open = Uid(0);
            }
        }
        let _ = e;
    }
    if !wants.is_empty() {
        // the things that can be used, near enough to any of the rays
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for &k in &wants {
            let (_, _, it, t) = &list[k];
            let reach = finite(it.reach, 3.0).clamp(0.1, 50.0);
            let (a, b) = segment_box(t.pos, add(t.pos, [0.0, 0.0, 0.0]), reach + 1.0);
            for q in 0..3 {
                lo[q] = lo[q].min(a[q]);
                hi[q] = hi[q].max(b[q]);
            }
        }
        let mut cands: Vec<Cand> = Vec::new();
        let _ = world.for_each_uid::<(&Interactable, &Transform), _>(|e, uid, (i, t)| {
            if i.enabled && (0..3).all(|q| t.pos[q] >= lo[q] - 2.0 && t.pos[q] <= hi[q] + 2.0) {
                cands.push(Cand { uid, e, i: *i, t: *t, col: None });
            }
        });
        for c in cands.iter_mut() {
            c.col = world.get::<Collider>(c.e).copied();
        }
        cands.sort_by_key(|c| c.uid);
        let rw = RayWorld::gather(world, fields, meshes, Some((lo, hi)));
        let layers = Layers::gather(world);
        for &k in &wants {
            let (ie, iuid, mut it, t) = list[k];
            let axes = axes_of(normalize_q(t.rot));
            let o = add(t.pos, rotate(&axes, it.eye));
            let d = [-axes[2][0], -axes[2][1], -axes[2][2]];
            let reach = finite(it.reach, 3.0).clamp(0.1, 50.0);
            let first = rw.cast(o, d, reach, layers.of(iuid), iuid);
            let mut best: Option<(f32, usize)> = None;
            for (ci, c) in cands.iter().enumerate() {
                if c.uid == iuid {
                    continue;
                }
                let Some(ts) = shape_hit(c, o, d) else { continue };
                if ts > reach.min(finite(c.i.range, 3.0)) {
                    continue;
                }
                // (a wall nearer than the thing hides it; the thing's own collider is no wall)
                if !first.is_none_or(|h| h.uid == c.uid || ts <= h.t + 0.05) {
                    continue;
                }
                if best.is_none_or(|b| ts < b.0) {
                    best = Some((ts, ci));
                }
            }
            it.target = Uid(best.map_or(0, |b| cands[b.1].uid));
            it.kind = best.map_or(0, |b| cands[b.1].i.kind);
            it.prompt = best.map_or(Sym(0), |b| cands[b.1].i.prompt);
            it.distance = best.map_or(0.0, |b| b.0);
            let press = it.act && !it.held;
            if press {
                if let Some((_, ci)) = best {
                    let target = Cand { uid: cands[ci].uid, e: cands[ci].e, i: world.get::<Interactable>(cands[ci].e).copied().unwrap_or(cands[ci].i), t: cands[ci].t, col: cands[ci].col };
                    use_it(world, tick, dt, iuid, ie, &mut it, &target, events);
                }
            }
            list[k].2 = it;
        }
    }
    for (e, _, it, _) in list.iter_mut() {
        it.held = it.act;
        if let Some(w) = world.get_mut::<Interactor>(*e) {
            // (what was found or used is the engine's; the buttons and numbers the game sets stay as they are now)
            w.target = it.target;
            w.prompt = it.prompt;
            w.kind = it.kind;
            w.distance = it.distance;
            w.used = it.used;
            w.denied = it.denied;
            w.held = it.held;
            w.open = it.open;
        }
    }
}

/// The nearest thing that an interactor looking from `o` along `d` (a unit vector) could use within `reach` metres, as the use button picks it: the distance, its uid and its entity. What it looks at is the thing's shape (its `Collider`, or a sphere of `radius`) within
/// the thing's `range`, and a wall nearer than the thing hides it (the thing's own collider is no wall). `who` is the looker's uid (its layers, and it never finds itself). For the code locks' keypad (`lock.rs`), which asks the same question the use button does.
pub(crate) fn look_at(world: &mut World, fields: &Terrains, meshes: &MeshStore, o: V3, d: V3, reach: f32, who: u64) -> Option<(f32, u64, Entity)> {
    let end = add(o, [d[0] * reach, d[1] * reach, d[2] * reach]);
    let (lo, hi) = segment_box(o, end, 2.0);
    let mut cands: Vec<Cand> = Vec::new();
    let _ = world.for_each_uid::<(&Interactable, &Transform), _>(|e, uid, (i, t)| {
        if i.enabled && uid != who && (0..3).all(|q| t.pos[q] >= lo[q] && t.pos[q] <= hi[q]) {
            cands.push(Cand { uid, e, i: *i, t: *t, col: None });
        }
    });
    if cands.is_empty() {
        return None;
    }
    for c in cands.iter_mut() {
        c.col = world.get::<Collider>(c.e).copied();
    }
    cands.sort_by_key(|c| c.uid);
    let rw = RayWorld::gather(world, fields, meshes, Some((lo, hi)));
    let layers = Layers::gather(world);
    let first = rw.cast(o, d, reach, layers.of(who), who);
    let mut best: Option<(f32, usize)> = None;
    for (ci, c) in cands.iter().enumerate() {
        let Some(ts) = shape_hit(c, o, d) else { continue };
        if ts > reach.min(finite(c.i.range, 3.0)) {
            continue;
        }
        if !first.is_none_or(|h| h.uid == c.uid || ts <= h.t + 0.05) {
            continue;
        }
        if best.is_none_or(|b| ts < b.0) {
            best = Some((ts, ci));
        }
    }
    best.map(|(t, ci)| (t, cands[ci].uid, cands[ci].e))
}
