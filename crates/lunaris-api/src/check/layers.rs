//! `check` rules for collision layers (docs/PHYSICS.md): a layer the engine cannot hold, a mask that collides with nothing, two entities that only
//! one of them wants to meet (they never do: both masks must agree), and bodies whose masks let them fall through everything.
use super::Problems;
use lunaris_ecs::{Result, World};
use lunaris_sim::components::{CharacterBody, CollisionLayer, Collider, ConvexCollider, MeshCollider, MeshRenderer, NavAgent, RigidBody, Sensor, Sprite, Terrain, Velocity};
use lunaris_sim::layers::Filter;
use std::collections::BTreeMap;

/// The most one-sided or falling-through reports: a scene with a wrong mask has the same mistake on a hundred entities.
const MAX_PAIR_REPORTS: usize = 8;

fn describe(f: Filter) -> String {
    format!("layer {}, mask {:#x}", f.layer, f.mask)
}

pub(super) fn run(world: &mut World, problems: &mut Problems) -> Result<()> {
    let mut layered: Vec<(u64, CollisionLayer)> = Vec::new();
    world.for_each_uid::<(&CollisionLayer,), _>(|_, uid, (c,)| layered.push((uid, *c)))?;
    if layered.is_empty() {
        return Ok(());
    }
    layered.sort_by_key(|l| l.0);
    let filters: BTreeMap<u64, Filter> = layered.iter().map(|(u, c)| (*u, Filter::of(c))).collect();
    let of = |uid: u64| filters.get(&uid).copied().unwrap_or(Filter::DEFAULT);

    // ---- what each entity has that a collision layer can act on
    let mut solid_static: Vec<u64> = Vec::new(); // things bodies stand on and bump into
    let mut solid_dynamic: Vec<u64> = Vec::new(); // bodies that fall and collide
    let mut shapes: Vec<u64> = Vec::new(); // anything that is part of a collision, triggers and characters too
    let mut movers: std::collections::BTreeSet<u64> = Default::default(); // the ones that move or only report: dynamic bodies, triggers, characters
    let mut acts: BTreeMap<u64, bool> = BTreeMap::new(); // entities for which a layer means something
    let mut dynamic_set: std::collections::BTreeSet<u64> = Default::default();
    world.for_each_uid::<(&RigidBody, &Velocity), _>(|_, uid, (rb, _)| {
        if rb.mass > 0.0 {
            dynamic_set.insert(uid);
        }
    })?;
    world.for_each_uid::<(&Collider,), _>(|_, uid, (c,)| {
        shapes.push(uid);
        acts.insert(uid, true);
        if c.trigger || dynamic_set.contains(&uid) {
            movers.insert(uid);
        }
        if !c.trigger {
            if dynamic_set.contains(&uid) {
                solid_dynamic.push(uid);
            } else {
                solid_static.push(uid);
            }
        }
    })?;
    world.for_each_uid::<(&ConvexCollider,), _>(|_, uid, (c,)| {
        shapes.push(uid);
        acts.insert(uid, true);
        if c.trigger || dynamic_set.contains(&uid) {
            movers.insert(uid);
        }
        if !c.trigger {
            if dynamic_set.contains(&uid) {
                solid_dynamic.push(uid);
            } else {
                solid_static.push(uid);
            }
        }
    })?;
    world.for_each_uid::<(&MeshCollider,), _>(|_, uid, (m,)| {
        acts.insert(uid, true);
        if m.solid {
            shapes.push(uid);
            solid_static.push(uid);
        }
    })?;
    world.for_each_uid::<(&Terrain,), _>(|_, uid, (g,)| {
        acts.insert(uid, true);
        if g.solid {
            shapes.push(uid);
            solid_static.push(uid);
        }
    })?;
    world.for_each_uid::<(&CharacterBody,), _>(|_, uid, _| {
        shapes.push(uid);
        movers.insert(uid);
        acts.insert(uid, true);
    })?;
    world.for_each_uid::<(&NavAgent,), _>(|_, uid, _| {
        acts.insert(uid, true);
    })?;
    world.for_each_uid::<(&Sensor,), _>(|_, uid, _| {
        acts.insert(uid, true);
    })?;
    // (rays hit what is drawn, too)
    world.for_each_uid::<(&MeshRenderer,), _>(|_, uid, _| {
        acts.insert(uid, true);
    })?;
    world.for_each_uid::<(&Sprite,), _>(|_, uid, _| {
        acts.insert(uid, true);
    })?;
    shapes.sort_unstable();
    solid_static.sort_unstable();
    solid_dynamic.sort_unstable();

    // ---- one entity at a time
    for (uid, c) in &layered {
        if c.layer > 31 {
            problems.add("warning", Some(*uid), format!("CollisionLayer.layer {} is above 31, which is the last layer: it counts as layer 31", c.layer), "the layers are numbered 0 to 31; pick one and put `1 << layer` in the masks that should collide with it");
        }
        if c.mask == 0 {
            problems.add("warning", Some(*uid), "CollisionLayer.mask is 0, so this entity collides with nothing (bodies pass through it, its triggers never fire, its rays and sight see through everything)".into(), "the mask is the layers it collides with, bit n = layer n: 4294967295 is all of them; remove the component for the default");
        }
        if !acts.contains_key(uid) {
            problems.add("warning", Some(*uid), "has a CollisionLayer but nothing it applies to (no Collider, ConvexCollider, MeshCollider, Terrain, CharacterBody, NavAgent, Sensor or MeshRenderer)".into(), "put it on the entity that has the collision shape");
        }
    }

    // ---- two sides that do not agree: one wants the other and is not wanted back. Compared by distinct (layer, mask), so a hundred crates are one report.
    // (two static things never meet, so only pairs with a mover in them matter)
    let mut kinds: BTreeMap<(u8, u32), (u64, bool)> = BTreeMap::new(); // (layer, mask) -> the first entity that has it, and whether one of them moves
    for uid in &shapes {
        let e = kinds.entry((of(*uid).layer, of(*uid).mask)).or_insert((*uid, false));
        e.1 |= movers.contains(uid);
    }
    let list: Vec<(Filter, u64, bool)> = kinds.iter().map(|(k, (u, m))| (Filter { layer: k.0, mask: k.1 }, *u, *m)).collect();
    let mut reports = 0usize;
    'pairs: for i in 0..list.len() {
        for j in i + 1..list.len() {
            let ((fa, ua, ma), (fb, ub, mb)) = (list[i], list[j]);
            if (ma || mb) && fa.sees(fb) != fb.sees(fa) && (filters.contains_key(&ua) || filters.contains_key(&ub)) {
                if reports >= MAX_PAIR_REPORTS {
                    problems.add("warning", None, "more pairs of collision layers that only one side wants to meet are not listed".into(), "fix the ones above and run `check` again");
                    break 'pairs;
                }
                let (wants, wanted) = if fa.sees(fb) { (ua, ub) } else { (ub, ua) };
                problems.add(
                    "warning",
                    Some(wants),
                    format!("({}) wants to collide with entity {wanted} ({}) but that one leaves its layer out of its mask, so they pass through each other: both masks must agree", describe(of(wants)), describe(of(wanted))),
                    "add the bit (1 << layer) to the mask that lacks it, or remove it from the one that has it if they should not meet",
                );
                reports += 1;
            }
        }
    }

    // ---- a body that collides with no solid at all, in a world that has solids: it falls through the world
    if !solid_static.is_empty() {
        let mut statics: Vec<Filter> = solid_static.iter().map(|u| of(*u)).collect();
        statics.sort_by_key(|f| (f.layer, f.mask));
        statics.dedup();
        let mut seen: Vec<Filter> = Vec::new();
        for uid in &solid_dynamic {
            let f = of(*uid);
            if seen.contains(&f) {
                continue;
            }
            seen.push(f);
            if !statics.iter().any(|s| f.collides(*s)) {
                if reports >= MAX_PAIR_REPORTS {
                    break;
                }
                reports += 1;
                problems.add("warning", Some(*uid), format!("this body ({}) collides with no static solid of the world (no ground, wall or level agrees with its mask and layer), so it falls through everything", describe(f)), "check its mask and the layer and mask of the ground: both must have the other's bit");
            }
        }
    }
    Ok(())
}
