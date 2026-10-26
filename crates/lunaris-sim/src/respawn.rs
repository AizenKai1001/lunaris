//! Death and respawn (docs/RESPAWN.md): what happens to a player whose `HitPoints` reach 0 in a survival game. A corpse is made where they fell (the game's prefab) with a copy of what they carried, their
//! own bag is emptied, and after a delay they get up again at a `SpawnPoint`: their own (a sleeping bag) or a shared one that is far from the other living players. It makes no body and shows no death
//! screen: the corpse prefab, the screen and the sounds are the game's.
//!
//! Deterministic: players are visited in uid order and the choice among spawn points and the scatter come from a hash of the tick, the player and the point, never a generator with state.
use crate::building::spawn_prefab_rot;
use crate::components::{Exposure, Heat, HitPoints, Respawn, SpawnPoint, Transform, Velocity, Vitals};
use crate::graph::ScriptSet;
use crate::inventory::{table_of, Bag, Inventory};
use crate::rng::hash_rand;
use lunaris_ecs::{Entity, Sym, Uid, World};
use lunaris_json::Json;

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

const SEED: u64 = 0xDEAD_5EED_0001;

#[inline]
fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

fn dist_xz(a: V3, b: V3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

struct Point {
    uid: u64,
    e: Entity,
    p: SpawnPoint,
    t: Transform,
}

pub fn update(world: &mut World, scripts: &mut ScriptSet, tick: u64, dt: f32, events: &mut Events) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let mut players: Vec<(Entity, u64, Respawn, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&Respawn, &Transform, &HitPoints), _>(|e, uid, (r, t, _)| players.push((e, uid, *r, *t)));
    if players.is_empty() {
        return;
    }
    players.sort_by_key(|p| p.1);

    for i in 0..players.len() {
        let (e, uid, mut r, t) = (players[i].0, players[i].1, players[i].2, players[i].3);
        let dead = world.get::<HitPoints>(e).is_some_and(|h| h.dead);
        if r.state == 0 {
            if !dead {
                continue;
            }
            // they have just died: the corpse, the empty bag, the wait
            r.state = 1;
            r.timer = finite(r.delay, 5.0).max(0.0);
            r.deaths = r.deaths.wrapping_add(1);
            r.died_at = t.pos;
            r.request = false;
            let mut carried: Option<Inventory> = None;
            // (a craft in hand ends with the life, and its ingredients, which were taken when it started, go where the rest of the bag goes: into the corpse, or back into the bag that is kept)
            let in_hand = world.get::<Inventory>(e).map_or(0, |i| i.crafting);
            let (table, refund) = match (in_hand > 0).then(|| table_of(world)).flatten() {
                Some((t, _)) => {
                    let ingredients = t.recipes.get(in_hand as usize - 1).map(|r| r.ingredients.clone()).unwrap_or_default();
                    (Some(t), ingredients)
                }
                None => (None, Vec::new()),
            };
            if let Some(inv) = world.get_mut::<Inventory>(e) {
                let has_items = (0..48).any(|k| inv.id[k] != 0 && inv.count[k] > 0) || !refund.is_empty();
                if has_items {
                    carried = Some(*inv);
                }
                if let Some(table) = &table {
                    let target = if r.keep_items { Some(&mut *inv) } else { carried.as_mut() };
                    if let Some(target) = target {
                        let mut bag = Bag::of(target);
                        for &(id, n) in &refund {
                            bag.add(table, id, n);
                        }
                        bag.store(target);
                    }
                }
                inv.crafting = 0;
                inv.craft_left = 0.0;
                inv.craft_total = 0.0;
                inv.craft_todo = 0;
                inv.craft_start = 0;
                inv.craft_cancel = false;
                if !r.keep_items {
                    inv.id = [0; 48];
                    inv.count = [0; 48];
                    inv.dur = [0.0; 48];
                    inv.weight = 0.0;
                    inv.can = [0, 0];
                }
            }
            let name = world.strings().get(r.corpse.0).to_string();
            let mut more: Vec<(&str, Json)> = Vec::new();
            if finite(r.corpse_time, 0.0) > 0.0 {
                more.push(("Lifetime", Json::obj([("seconds", Json::Float(r.corpse_time as f64))])));
            }
            r.corpse_uid = Uid(0);
            if let Some(c) = spawn_prefab_rot(world, scripts, &name, t.pos, t.rot, &more) {
                r.corpse_uid = Uid(c);
                if let (Some(mut inv), Some(ce)) = (carried, world.entity(c)) {
                    // (what the corpse holds is a copy of the bag, not a second bag that asks for things or publishes the same numbers)
                    inv.give_id = 0;
                    inv.give_n = 0;
                    inv.take_id = 0;
                    inv.take_n = 0;
                    inv.craft = 0;
                    inv.craft_start = 0;
                    inv.craft_cancel = false;
                    inv.crafting = 0;
                    inv.craft_todo = 0;
                    inv.craft_left = 0.0;
                    inv.craft_total = 0.0;
                    inv.wear = 0;
                    inv.fill = Sym(0);
                    inv.publish = Sym(0);
                    inv.sel = 0;
                    let _ = world.insert(ce, inv);
                }
                events.push(("corpse".to_string(), c, Some(t.pos)));
            }
        } else {
            if !dead {
                // (the game brought them back by itself)
                r.state = 0;
            } else {
                r.timer = (r.timer - dt).max(0.0);
                let ready = r.timer <= 0.0 && (!r.hold || r.request);
                if ready {
                    let (at, face) = choose(world, &players, i, &r, uid, tick);
                    let at = at.map_or(r.died_at, |a| a.0);
                    let rot = face.unwrap_or(t.rot);
                    revive(world, e, &r, at, rot);
                    r.state = 0;
                    r.request = false;
                    events.push(("respawned".to_string(), uid, Some(at)));
                }
            }
        }
        if let Some(w) = world.get_mut::<Respawn>(e) {
            *w = r;
        }
    }
}

/// Where the player `i` starts again: the spawn point and the way it faces (None for no point at all).
fn choose(world: &mut World, players: &[(Entity, u64, Respawn, Transform)], i: usize, r: &Respawn, uid: u64, tick: u64) -> (Option<(V3, u64)>, Option<[f32; 4]>) {
    // the others who are alive now
    let others: Vec<V3> = players
        .iter()
        .enumerate()
        .filter(|(k, p)| *k != i && world.get::<Respawn>(p.0).is_some_and(|x| x.state == 0) && !world.get::<HitPoints>(p.0).is_some_and(|h| h.dead))
        .map(|(_, p)| world.get::<Transform>(p.0).map_or(p.3.pos, |t| t.pos))
        .collect();
    // (the members of a party share their places: a player with none of their own may get up at one of a party mate's, docs/PARTIES.md)
    let party = crate::party::party_of(world, uid);
    let mates: Vec<u32> = if party == 0 {
        Vec::new()
    } else {
        players.iter().enumerate().filter(|(k, p)| *k != i && p.2.group != 0 && p.2.group != r.group && crate::party::party_of(world, p.1) == party).map(|(_, p)| p.2.group).collect()
    };
    choose_place(world, &others, r.group, &mates, finite(r.avoid, 0.0).max(0.0), uid, tick)
}

/// Where a player who joins a game server starts (docs/GAME_SERVER.md): the same choice a respawn makes, for a player of `group` among the `others` (their places), by a hash of `salt` and `tick`. None with no spawn point.
pub fn spawn_place(world: &mut World, others: &[V3], group: u32, avoid: f32, salt: u64, tick: u64) -> Option<(V3, [f32; 4])> {
    let (at, rot) = choose_place(world, others, group, &[], avoid.max(0.0), salt, tick);
    at.map(|a| a.0).zip(rot)
}

/// `mates` are the groups of the player's party mates: a place of one of them is the player's own after the places of their own group.
fn choose_place(world: &mut World, others: &[V3], group: u32, mates: &[u32], avoid: f32, uid: u64, tick: u64) -> (Option<(V3, u64)>, Option<[f32; 4]>) {
    let mut points: Vec<Point> = Vec::new();
    let _ = world.for_each_uid::<(&SpawnPoint, &Transform), _>(|e, puid, (p, t)| {
        if p.active && t.pos.iter().all(|v| v.is_finite()) {
            points.push(Point { uid: puid, e, p: *p, t: *t });
        }
    });
    if points.is_empty() {
        return (None, None);
    }
    points.sort_by_key(|p| p.uid);
    let personal: Vec<usize> = (0..points.len()).filter(|&k| group != 0 && points[k].p.group == group).collect();
    let pick = if !personal.is_empty() {
        // their own place: the first one (the lowest uid), whoever is near it
        personal[0]
    } else if let Some(k) = (0..points.len()).find(|&k| points[k].p.group != 0 && mates.contains(&points[k].p.group)) {
        // a party mate's place: the first (the lowest uid) of the places of the party's other members
        k
    } else {
        let shared: Vec<usize> = (0..points.len()).filter(|&k| points[k].p.group == 0).collect();
        if shared.is_empty() {
            return (None, None);
        }
        let near = |k: usize| others.iter().map(|o| dist_xz(*o, points[k].t.pos)).fold(f32::INFINITY, f32::min);
        let ok: Vec<usize> = shared.iter().copied().filter(|&k| near(k) >= avoid).collect();
        if ok.is_empty() {
            // nowhere is far enough: the farthest of them (the lowest uid among equals)
            let mut best = shared[0];
            for &k in &shared {
                if near(k) > near(best) {
                    best = k;
                }
            }
            best
        } else {
            ok[((hash_rand(SEED, tick, uid, 0) * ok.len() as f32) as usize).min(ok.len() - 1)]
        }
    };
    let p = &points[pick];
    let radius = finite(p.p.radius, 0.0).max(0.0);
    let mut at = p.t.pos;
    if radius > 0.0 {
        let a = std::f64::consts::TAU * hash_rand(SEED, tick, uid, 1 + p.uid * 2) as f64;
        let d = radius * hash_rand(SEED, tick, uid, 2 + p.uid * 2).sqrt();
        at[0] += d * crate::detmath::cos(a) as f32;
        at[2] += d * crate::detmath::sin(a) as f32;
    }
    let (e, once) = (p.e, p.p.once);
    let (puid, rot) = (p.uid, p.t.rot);
    if let Some(w) = world.get_mut::<SpawnPoint>(e) {
        w.uses = w.uses.wrapping_add(1);
        if once {
            w.active = false;
        }
    }
    (Some((at, puid)), Some(rot))
}

/// Get the player up: full hit points with a grace, fed, rested, at `at`, facing `rot`.
fn revive(world: &mut World, e: Entity, r: &Respawn, at: V3, rot: [f32; 4]) {
    if let Some(h) = world.get_mut::<HitPoints>(e) {
        let max = h.max.max(1.0);
        h.value = max;
        h.last = max;
        h.dead = false;
        h.hurt = 100.0;
        h.guard = false;
        h.invulnerable = finite(r.protect, 0.0).max(0.0);
    }
    if let Some(t) = world.get_mut::<Transform>(e) {
        t.pos = at;
        t.rot = rot;
    }
    if let Some(v) = world.get_mut::<Velocity>(e) {
        v.lin = [0.0; 3];
        v.ang = [0.0; 3];
    }
    if let Some(v) = world.get_mut::<Vitals>(e) {
        let food = finite(r.food, 80.0).clamp(0.0, 100.0);
        v.hunger = food;
        v.thirst = food;
        v.stamina = 100.0;
        v.bleeding = 0.0;
        v.exhausted = false;
        v.since_sprint = 0.0;
    }
    if let Some(h) = world.get_mut::<Heat>(e) {
        h.value = 0.0;
        h.cooldown = 0.0;
        // (a new life: the Core has not learned of this one yet, docs/WALKER.md)
        h.alert = 0.0;
        h.tier = 0;
        h.last = 0.0;
    }
    if let Some(x) = world.get_mut::<Exposure>(e) {
        x.value = 0.0;
        x.detected = false;
        x.lit = false;
    }
}
