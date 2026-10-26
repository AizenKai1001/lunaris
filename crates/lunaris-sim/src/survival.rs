//! Survival needs (docs/SURVIVAL.md): `Vitals` keeps a character's hunger, thirst, stamina and bleeding, turns an empty meter into damage and a fed body into slow healing, and tells a
//! movement script how fast the character may go (`speed_scale`, `can_sprint`). It is plain numbers on a component, so it is saved with the world, replayed, and replicated by the netcode like any
//! other state. It draws and plays nothing: the bars, the coughing and the heartbeat are the game's.
use crate::components::{HitPoints, Vitals};
use crate::rng::hash_rand;
use lunaris_ecs::{Entity, World};

type Events = Vec<(String, u64, Option<[f32; 3]>)>;

const WOUND_SEED: u64 = 0x5B1E_ED00_D5A1_0003;
const FULL: f32 = 100.0;
/// Below this a meter is "low" (hungry, thirsty).
const LOW: f32 = 25.0;
/// An exhausted character may sprint again when its stamina is back to this.
const RECOVERED: f32 = 25.0;

fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

/// A hit of `damage` (what got through the worn armor) on the character `victim`: a body whose `Vitals.wound_chance` is above 0 bleeds from a hit of at least `wound_min` with that chance (a hash of the tick, the victim and `shot`, never a
/// generator with state), by `wound_rate` hit points a second more, up to `bleed_max`. Anything that is not a character with `Vitals` is not touched.
pub fn wound(world: &mut World, victim: u64, damage: f32, tick: u64, shot: u64) {
    if !(damage.is_finite() && damage > 0.0) {
        return;
    }
    let Some(e) = world.entity(victim) else { return };
    let Some(v) = world.get::<Vitals>(e).copied() else { return };
    let chance = finite(v.wound_chance, 0.0).clamp(0.0, 1.0);
    if !v.active || chance <= 0.0 || damage < finite(v.wound_min, 6.0) || hash_rand(WOUND_SEED, tick, victim, shot) >= chance {
        return;
    }
    if let Some(w) = world.get_mut::<Vitals>(e) {
        w.bleeding = (finite(w.bleeding, 0.0).max(0.0) + finite(v.wound_rate, 0.5).max(0.0)).min(finite(v.bleed_max, 2.0).max(0.0));
    }
}

pub fn update(world: &mut World, dt: f32, events: &mut Events) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let mut list: Vec<(Entity, u64, Vitals)> = Vec::new();
    let _ = world.for_each_uid::<(&Vitals,), _>(|e, uid, (v,)| list.push((e, uid, *v)));
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.1);
    for (e, uid, mut v) in list {
        if !v.active {
            continue;
        }
        // what an item did this tick
        v.hunger += finite(v.eat, 0.0);
        v.thirst += finite(v.drink, 0.0);
        v.eat = 0.0;
        v.drink = 0.0;
        if v.bandage {
            v.bleeding = 0.0;
            v.bandage = false;
        }
        let instant_heal = finite(v.heal, 0.0);
        v.heal = 0.0;
        // sprinting: only while it can
        let sprinting = v.sprint && v.can_sprint && !v.exhausted && v.stamina > 0.0;
        let exertion = if sprinting { finite(v.exertion, 2.0).max(1.0) } else { 1.0 };
        let rest = if v.resting { 0.5 } else { 1.0 };
        v.hunger -= finite(v.hunger_rate, 0.0).max(0.0) * exertion * rest * dt;
        v.thirst -= finite(v.thirst_rate, 0.0).max(0.0) * exertion * rest * dt;
        if sprinting {
            v.stamina -= finite(v.sprint_cost, 0.0).max(0.0) * dt;
            v.since_sprint = 0.0;
        } else {
            v.since_sprint = (v.since_sprint + dt).min(1.0e6);
            if v.since_sprint >= finite(v.stamina_delay, 0.0).max(0.0) {
                v.stamina += finite(v.stamina_regen, 0.0).max(0.0) * dt;
            }
        }
        v.hunger = v.hunger.clamp(0.0, FULL);
        v.thirst = v.thirst.clamp(0.0, FULL);
        v.stamina = v.stamina.clamp(0.0, FULL);
        v.bleeding = finite(v.bleeding, 0.0).max(0.0);
        if v.stamina <= 0.0 {
            v.exhausted = true;
        } else if v.exhausted && v.stamina >= RECOVERED {
            v.exhausted = false;
        }
        v.can_sprint = !v.exhausted;
        // what the body does about it
        let mut harm = v.bleeding;
        if v.hunger <= 0.0 {
            harm += finite(v.starve_damage, 0.0).max(0.0);
        }
        if v.thirst <= 0.0 {
            harm += finite(v.dehydrate_damage, 0.0).max(0.0);
        }
        let healing = harm == 0.0 && v.hunger >= finite(v.heal_threshold, 70.0) && v.thirst >= finite(v.heal_threshold, 70.0);
        let net = instant_heal - harm * dt + if healing { finite(v.heal_rate, 0.0).max(0.0) * dt } else { 0.0 };
        if net != 0.0 {
            if let Some(h) = world.get_mut::<HitPoints>(e) {
                // (a character at 0 is dead even on the tick before the health system says so: nothing heals it)
                if !h.dead && h.value > 0.0 {
                    h.value = (h.value + net).clamp(0.0, h.max.max(0.0));
                }
            }
        }
        // what it tells the game
        let before = v.status;
        let mut status = 0u32;
        if v.hunger < LOW {
            status |= 1;
        }
        if v.thirst < LOW {
            status |= 2;
        }
        if v.bleeding > 0.0 {
            status |= 4;
        }
        if v.exhausted {
            status |= 8;
        }
        if v.hunger <= 0.0 || v.thirst <= 0.0 {
            status |= 16;
        }
        v.status = status;
        for (bit, name) in [(1u32, "hungry"), (2, "thirsty"), (4, "bleeding"), (8, "exhausted"), (16, "starving")] {
            if status & bit != 0 && before & bit == 0 {
                events.push((name.to_string(), uid, None));
            }
        }
        let mut scale = 1.0f32;
        if v.exhausted {
            scale *= 0.6;
        }
        if v.hunger < 10.0 || v.thirst < 10.0 {
            scale *= 0.85;
        }
        if v.bleeding > 0.0 {
            scale *= 0.9;
        }
        v.speed_scale = scale;
        if let Some(w) = world.get_mut::<Vitals>(e) {
            *w = v;
        }
    }
}
