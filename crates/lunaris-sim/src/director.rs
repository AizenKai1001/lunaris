//! The AI director (docs/AI.md): the part of a game that decides when the machines come. Players carry a `Heat`; a `Director` raises it when a `Searchlight` has them detected, when they fire a
//! gun that can be heard, and when a machine sees them, and when the heat reaches its threshold it sends a wave of hunters, made from the game's own prefab, on a ring round the player, alerted to
//! where the player is. It makes no model and plays nothing: what a hunter is (body, weapon, animation, sounds) is all in the prefab the game supplies.
//!
//! Deterministic: directors and players are visited in uid order and the places of a wave come from a hash of the tick, the director and the hunter's number.
use crate::components::{Director, Exposure, Heat, Parent, Perception, SpawnedBy, Transform, Weapon};
use crate::gameplay::despawn_trees;
use crate::graph::ScriptSet;
use crate::layers::Filter;
use crate::meshcol::MeshStore;
use crate::perception::look_rot;
use crate::rng::hash_rand;
use crate::terrain::Terrains;
use crate::vehicle_ray::RayWorld;
use lunaris_ecs::{Entity, Uid, World};
use lunaris_json::Json;
use std::collections::{BTreeMap, BTreeSet};

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

const SEED: u64 = 0xD12E_C70B_5EED;
/// Heat never goes beyond this (a wave takes the player a while to shake off, but not forever).
const MAX_HEAT: f32 = 10.0;
/// The most hunters in one wave.
const MAX_WAVE: u32 = 16;

fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

pub fn update(world: &mut World, scripts: &mut ScriptSet, tick: u64, dt: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let mut dirs: Vec<(Entity, u64, Director)> = Vec::new();
    let _ = world.for_each_uid::<(&Director,), _>(|e, uid, (d,)| dirs.push((e, uid, *d)));
    if dirs.is_empty() {
        return;
    }
    dirs.sort_by_key(|d| d.1);
    let Some(source) = dirs.iter().find(|d| d.2.active).map(|d| d.2) else { return };

    // ---- heat: what raises it this tick, and what lowers it
    let mut seen_by: BTreeMap<u64, u32> = BTreeMap::new();
    let _ = world.for_each::<(&Perception,), _>(|_, (p,)| {
        if p.active && p.state == 2 && p.visible && p.target.0 != 0 {
            *seen_by.entry(p.target.0).or_default() += 1;
        }
    });
    let mut shooters: BTreeSet<u64> = BTreeSet::new();
    // (the owner of a gun that can be heard: its `owner`, or without one the entity it is a child of, which is how the weapons system takes the owner of a player's tool, the gun on an avatar's head)
    let mut tools: Vec<Entity> = Vec::new();
    let _ = world.for_each::<(&Weapon,), _>(|e, (w,)| {
        if w.fired && w.noise > 0.0 {
            if w.owner.0 != 0 {
                shooters.insert(w.owner.0);
            } else {
                tools.push(e);
            }
        }
    });
    for e in tools {
        if let Some(p) = world.get::<Parent>(e) {
            shooters.insert(p.target.0);
        }
    }
    let mut heats: Vec<(Entity, u64, Heat, V3)> = Vec::new();
    let _ = world.for_each_uid::<(&Heat, &Transform), _>(|e, uid, (h, t)| heats.push((e, uid, *h, t.pos)));
    heats.sort_by_key(|h| h.1);
    // the Core's learning (docs/WALKER.md): nothing of it without an `Escalation` in the scene
    let esc = crate::escalation::config(world);
    let gains = match &esc {
        Some(c) => {
            let at: Vec<(u64, V3)> = heats.iter().map(|h| (h.1, h.3)).collect();
            crate::escalation::event_gains(world, events, &scripts.events, c, &at)
        }
        None => Vec::new(),
    };
    for (i, (e, uid, h, pos)) in heats.iter_mut().enumerate() {
        // (what other systems added to the heat since the director left it: a helicopter's noise, a hacking trace, a script)
        let ext = (finite(h.value, 0.0) - finite(h.last, 0.0)).max(0.0);
        let mut add = 0.0;
        if world.get::<Exposure>(*e).is_some_and(|x| x.detected) {
            add += finite(source.light_heat, 0.0) * dt;
        }
        if shooters.contains(&*uid) {
            add += finite(source.shot_heat, 0.0);
        }
        if let Some(n) = seen_by.get(&*uid) {
            add += finite(source.seen_heat, 0.0) * dt * (*n).min(3) as f32;
        }
        h.value = (finite(h.value, 0.0) + add - finite(h.decay, 0.0).max(0.0) * dt).clamp(0.0, MAX_HEAT);
        h.cooldown = (h.cooldown - dt).max(0.0);
        if let Some(c) = &esc {
            let before = h.tier;
            crate::escalation::track(h, ext + add, gains.get(i).copied().unwrap_or(0.0), dt, c);
            if h.tier != before {
                events.push(("tier".to_string(), *uid, Some(*pos)));
            }
        }
    }

    // ---- the hunters that are alive, and the idle ones to remove
    let mut alive: BTreeMap<u64, u32> = BTreeMap::new();
    let mut idle: Vec<(u64, u64, f32)> = Vec::new();
    let mut raw: Vec<(Entity, u64, u64)> = Vec::new();
    let _ = world.for_each_uid::<(&SpawnedBy,), _>(|e, uid, (b,)| raw.push((e, uid, b.spawner.0)));
    for (e, uid, spawner) in raw {
        *alive.entry(spawner).or_default() += 1;
        if let Some(p) = world.get::<Perception>(e) {
            if p.active && p.state == 0 {
                idle.push((spawner, uid, p.since_seen));
            }
        }
    }

    // ---- each director: clean up, then send waves
    let mut doomed: Vec<u64> = Vec::new();
    for (de, duid, mut d) in dirs {
        if !d.active {
            continue;
        }
        if d.cleanup > 0.0 {
            for (spawner, uid, since) in &idle {
                if *spawner == duid && *since >= d.cleanup {
                    doomed.push(*uid);
                    if let Some(a) = alive.get_mut(&duid) {
                        *a = a.saturating_sub(1);
                    }
                }
            }
        }
        let name = world.strings().get(d.prefab.0).to_string();
        let mut my_alive = alive.get(&duid).copied().unwrap_or(0);
        if !name.is_empty() {
            for (_, tuid, h, pos) in heats.iter_mut() {
                // (the player's tier scales the wave: docs/WALKER.md)
                let (hunters, slow, more, per) = match &esc {
                    Some(c) => crate::escalation::effect(c, h.tier),
                    None => (1.0, 1.0, 0, 1.0),
                };
                let cap = d.max_alive + more;
                if h.value < finite(d.threshold, 1.0).max(0.0) || h.cooldown > 0.0 || my_alive >= cap {
                    continue;
                }
                let room = cap - my_alive;
                let grow = ((h.value - d.threshold) * finite(d.per_heat, 0.0).max(0.0) * per).floor().max(0.0) as u32;
                let want = (((1 + grow) as f32) * hunters).round().max(1.0) as u32;
                let n = want.min(room).min(MAX_WAVE);
                let (lo, hi) = (finite(d.min_dist, 40.0).max(0.0), finite(d.max_dist, 70.0).max(finite(d.min_dist, 40.0).max(0.0)));
                let mut spawned = 0u32;
                // (`clear`: in a world of buildings a hunter that is dropped on the far side of a block cannot get round it and never sees the player, so a place with a clear line to the player's head is looked for, up to `clear` more draws: one world gathering for the wave)
                let tries = d.clear.min(32) as u64;
                let rw = (tries > 0).then(|| {
                    let r = hi + 6.0;
                    RayWorld::gather(world, fields, meshes, Some(([pos[0] - r, -50.0, pos[2] - r], [pos[0] + r, 600.0, pos[2] + r])))
                });
                let head = [pos[0], pos[1] + 1.4, pos[2]];
                for k in 0..n {
                    let salt0 = d.sent as u64 * 64 + k as u64;
                    let spot = |salt: u64| {
                        let angle = std::f64::consts::TAU * hash_rand(SEED, tick, duid, salt * 2) as f64;
                        let dist = lo + (hi - lo) * hash_rand(SEED, tick, duid, salt * 2 + 1);
                        [pos[0] + dist * crate::detmath::cos(angle) as f32, pos[1] + finite(d.height, 1.0), pos[2] + dist * crate::detmath::sin(angle) as f32]
                    };
                    let mut at = spot(salt0);
                    if let Some(rw) = &rw {
                        for attempt in 0..=tries {
                            let c = spot(salt0 + attempt * 1_000_003);
                            let (dx, dy, dz) = (head[0] - c[0], head[1] - c[1], head[2] - c[2]);
                            let len = (dx * dx + dy * dy + dz * dz).sqrt().max(0.01);
                            if rw.cast(c, [dx / len, dy / len, dz / len], len, Filter::DEFAULT, 0).is_none_or(|h| h.t >= len - 0.6) {
                                at = c;
                                break;
                            }
                        }
                    }
                    let face = [pos[0] - at[0], 0.0, pos[2] - at[2]];
                    let rot = look_rot(face, false);
                    let num = |v: f32| Json::Float(v as f64);
                    let mut ov: Vec<(String, Json)> = Vec::new();
                    crate::prefab::merge_component(&mut ov, "Transform", &Json::obj([("pos", Json::arr(at.map(num))), ("rot", Json::arr(rot.map(num)))]));
                    crate::prefab::merge_component(&mut ov, "SpawnedBy", &Json::obj([("spawner", Json::from(duid as i64))]));
                    let Ok(plan) = crate::prefab::plan(&scripts.prefabs, &name, &ov) else { break };
                    let base = world.next_uid();
                    let cmds = crate::prefab::spawn_commands(&plan, base);
                    if crate::command::apply_batch(world, scripts, &cmds).is_err() {
                        break;
                    }
                    // the hunter starts out knowing where the player is
                    if let Some(he) = world.entity(base) {
                        if let Some(p) = world.get_mut::<Perception>(he) {
                            p.awareness = 1.0;
                            p.state = 3;
                            p.target = Uid(*tuid);
                            p.seen_pos = [pos[0], pos[1], pos[2]];
                            p.since_seen = 0.0;
                        }
                    }
                    events.push(("hunter".to_string(), base, Some(at)));
                    spawned += 1;
                }
                if spawned > 0 {
                    my_alive += spawned;
                    h.value = (h.value - finite(d.wave_cost, 0.0).max(0.0)).max(0.0);
                    h.cooldown = finite(d.cooldown, 25.0).max(0.0) * slow;
                    h.waves = h.waves.wrapping_add(1);
                    d.waves = d.waves.wrapping_add(1);
                    d.sent = d.sent.wrapping_add(spawned);
                    events.push(("wave".to_string(), duid, Some(*pos)));
                }
            }
        }
        d.alive = my_alive;
        alive.insert(duid, my_alive);
        if let Some(w) = world.get_mut::<Director>(de) {
            *w = d;
        }
    }
    for (e, _, h, _) in heats.iter_mut() {
        h.last = h.value;
        if let Some(w) = world.get_mut::<Heat>(*e) {
            *w = *h;
        }
    }
    if let Some(c) = &esc {
        let top = heats.iter().fold((0u32, 0.0f32), |a, h| (a.0.max(h.2.tier), a.1.max(h.2.alert)));
        crate::escalation::publish(world, c, top.0, top.1);
    }
    if !doomed.is_empty() {
        despawn_trees(world, doomed);
    }
}
