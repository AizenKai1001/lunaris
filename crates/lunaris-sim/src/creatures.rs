//! Creatures in the world (docs/CREATURES.md): an entity with a `Creature` is an evolved animal, a spider, whose body and brain were trained together (the Evolve-0 project's `spidergpu`) and are run here by the
//! `lunaris-creature` crate exactly as they were trained. Every tick it senses the ground and the place it is to walk to (an entity or a point), thinks, and moves: 50 `CreaturePart` entities (the segments of its
//! body, drawn as built-in shapes, and a few drawn-on details) follow its body, and the `Creature` entity itself rides on its prosoma, so that game code can read where the animal is.
//!
//! Its physics is its own (a skeleton of 50 rigid bodies with joints, muscles, hydraulics and grip, solved against the ground), not the engine's rigid bodies: it walks on `Terrain`s (and on the flat ground at the
//! height where it was placed), and other things do not push it (a game shoves it with `Sim::creatures.shove`). Like every part of the state it is cloned by `clone`, copied by `checkpoint`, hashed, recorded and
//! emptied by `clear`; a world without creatures pays one query.
use crate::components::{Collider, Creature, CreaturePart, Food, FoodSpot, MeshRenderer, RigidBody, Transform, Velocity};
use crate::terrain::{grounds, Placed, Terrains};
use lunaris_creature as lc;
use lunaris_ecs::{Sym, Uid, World};
use std::collections::BTreeMap;
use std::path::Path;

/// One creature that is alive: its simulation and the entities that draw it.
#[derive(Clone)]
struct Live {
    c: lc::Creature,
    /// The uids of the part entities (50) and of the details.
    parts: Vec<u64>,
    decor: Vec<u64>,
    /// The entity that draws the silk thread (0: none).
    thread: u64,
    /// The height of the flat ground where there is no terrain.
    floor: f32,
    /// What it was made from, to notice a change (brain file, model file, which program, body seed).
    key: (String, String, u32, u32, u32),
    visible: bool,
    /// How big it is against the animal it was trained as (1: the trained size; 0.25: a quarter, with every length, mass and time scaled to match), and the ticks it owes.
    size: f32,
    acc: f32,
    /// How many meals it had when we last looked (a new one starts a rest).
    eaten_seen: u32,
}

/// The creatures of a simulation (state: see the module's note).
#[derive(Clone, Default)]
pub struct Creatures {
    live: BTreeMap<u64, Live>,
    /// Fractions of ticks owed while time runs at a speed other than 1 (`time.scale`).
    acc: f32,
    /// Why a brain or a model could not be read, by path.
    pub errors: BTreeMap<String, String>,
    /// The generator that decides where eaten food appears again.
    food_rng: u64,
}

impl std::fmt::Debug for Creatures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Creatures({} alive)", self.live.len())
    }
}

impl Creatures {
    pub fn is_empty(&self) -> bool {
        self.live.is_empty() && self.acc == 0.0
    }
    pub fn clear(&mut self) {
        self.live.clear();
        self.acc = 0.0;
        self.errors.clear();
        self.food_rng = 0;
    }
    /// Feeds the state to `f`, in a fixed order (the simulation's state hash).
    pub fn fold(&self, f: &mut dyn FnMut(u64)) {
        f(self.acc.to_bits() as u64);
        f(self.food_rng);
        for (uid, l) in &self.live {
            f(*uid);
            f(l.acc.to_bits() as u64);
            l.c.fold_state(f);
        }
    }
    /// A sudden change of speed of the creature of entity `uid` (a blow, a gust); false if it is not alive.
    pub fn shove(&mut self, uid: u64, dv: [f32; 3]) -> bool {
        match self.live.get_mut(&uid) {
            Some(l) => {
                // a speed of the world is (size's square root) times as big in the creature's own units
                let k = 1.0 / l.size.sqrt();
                l.c.shove(lc::vecmath::v3(dv[0] * k, dv[1] * k, dv[2] * k));
                true
            }
            None => false,
        }
    }
    /// How many creatures are alive.
    pub fn len(&self) -> usize {
        self.live.len()
    }
}

/// The ground as the creature wants it: the terrains of the world (the first that covers the place), the flat ground at `floor` elsewhere.
struct WorldGround<'a> {
    placed: Vec<Placed<'a>>,
    floor: f32,
    /// The creature's size: its own units are the world's divided by this.
    s: f32,
    /// The static box colliders of the world (walls, ramps, ceilings, platforms): the creature walks and climbs on them.
    boxes: &'a [lc::SolidBox],
}

impl lc::Ground for WorldGround<'_> {
    fn height(&self, x: f32, z: f32) -> f32 {
        for p in &self.placed {
            if let Some(y) = p.surface(x * self.s, z * self.s) {
                return y / self.s;
            }
        }
        self.floor / self.s
    }
    fn solid(&self, p: lc::vecmath::V3) -> Option<(f32, lc::vecmath::V3)> {
        let pw = lc::vecmath::v3(p.x * self.s, p.y * self.s, p.z * self.s);
        let mut best: Option<(f32, lc::vecmath::V3)> = None;
        for b in self.boxes {
            let (d, n) = b.sdf(pw);
            match best {
                Some((bd, _)) if bd <= d => {}
                _ => best = Some((d / self.s, n)),
            }
        }
        best
    }
}

/// The static box colliders of the world as solids for the creatures (a box with a `RigidBody` that has mass and a `Velocity` is dynamic and is left out).
/// The farthest a target is shown to a creature's brain (in the creature's own units): the trainer's targets are up to 6.5 away, mostly 1 to 2.5.
const FAR_TARGET: f32 = 3.0;

fn static_boxes(world: &mut World) -> Vec<lc::SolidBox> {
    let mut raw: Vec<(lunaris_ecs::Entity, Transform, Collider)> = Vec::new();
    let _ = world.for_each_uid::<(&Transform, &Collider), _>(|e, _, (t, c)| {
        if c.shape == 1 && !c.trigger {
            raw.push((e, *t, *c));
        }
    });
    let mut out = Vec::new();
    for (e, t, c) in raw {
        if let (Some(rb), Some(_)) = (world.get::<RigidBody>(e), world.get::<Velocity>(e)) {
            if rb.mass > 0.0 {
                continue;
            }
        }
        if world.get::<CreaturePart>(e).is_some() {
            continue;
        }
        let half = lc::vecmath::v3(0.5 * c.size[0].abs() * t.scale[0].abs(), 0.5 * c.size[1].abs() * t.scale[1].abs(), 0.5 * c.size[2].abs() * t.scale[2].abs());
        let n = (t.rot[0] * t.rot[0] + t.rot[1] * t.rot[1] + t.rot[2] * t.rot[2] + t.rot[3] * t.rot[3]).sqrt();
        let q = if n > 1e-9 { [t.rot[0] / n, t.rot[1] / n, t.rot[2] / n, t.rot[3] / n] } else { [0.0, 0.0, 0.0, 1.0] };
        out.push(lc::SolidBox { c: lc::vecmath::v3(t.pos[0], t.pos[1], t.pos[2]), r: lc::vecmath::from_quat(&q), half });
    }
    out
}

fn yaw_of(rot: [f32; 4]) -> f32 {
    // the way the entity's x axis points in the ground plane; a rotation about +y by theta turns +x to (cos theta, -sin theta)
    let (x, y, z, w) = (rot[0], rot[1], rot[2], rot[3]);
    let fx = 1.0 - 2.0 * (y * y + z * z);
    let fz = 2.0 * (x * z - w * y);
    lc::mathx::datan2f(-fz, fx)
}

fn read_brain(root: &Path, name: &str, which: usize) -> Result<lc::Brain, String> {
    let text = std::fs::read_to_string(root.join(name)).map_err(|e| format!("cannot read {name}: {e}"))?;
    let doc = lunaris_json::parse(&text).map_err(|e| format!("{name} is not JSON: {e:?}"))?;
    match doc.get("genomes").and_then(|g| g.as_array()) {
        Some(list) if !list.is_empty() => Ok(lc::Brain::from_genome_json(&list[which.min(list.len() - 1)])),
        Some(_) => Err(format!("{name} has no genomes")),
        None => Ok(lc::Brain::from_genome_json(&doc)),
    }
}

fn read_model(root: &Path, name: &str) -> Result<lc::Model, String> {
    if name.is_empty() {
        return Ok(lc::Model::spider());
    }
    let bytes = std::fs::read(root.join(name)).map_err(|e| format!("cannot read {name}: {e}"))?;
    lc::Model::from_bytes(&bytes)
}

fn spawn_visuals(world: &mut World, owner: u64, model: &lc::Model, c: &lc::Creature, visible: bool, s: f32) -> (Vec<u64>, Vec<u64>, u64) {
    use lc::visuals::{decorations, part_look, part_size};
    let mut parts = Vec::with_capacity(lc::NB);
    for i in 0..lc::NB {
        let look = part_look(i);
        let (p, q) = c.pose(i);
        let size = part_size(model, i).map(|v| v * s);
        let mesh = Sym(world.strings_mut().intern(look.mesh));
        let t = Transform { pos: [p.x * s, p.y * s, p.z * s], rot: q, scale: size };
        let m = MeshRenderer { mesh, color: look.colour, roughness: 0.5, visible, emissive: look.emissive, ..Default::default() };
        if let Ok(e) = world.spawn_with3(t, m, CreaturePart { owner: Uid(owner), index: i as u32 }) {
            if let Some(uid) = world.uid_of(e) {
                parts.push(uid);
            }
        }
    }
    let mut decor = Vec::new();
    for (k, d) in decorations().iter().enumerate() {
        let mesh = Sym(world.strings_mut().intern(d.mesh));
        let t = Transform { pos: [0.0; 3], rot: d.rot, scale: d.size.map(|v| v * s) };
        let m = MeshRenderer { mesh, color: d.colour, roughness: 0.3, visible, emissive: d.emissive, ..Default::default() };
        if let Ok(e) = world.spawn_with3(t, m, CreaturePart { owner: Uid(owner), index: (lc::NB + k) as u32 }) {
            if let Some(uid) = world.uid_of(e) {
                decor.push(uid);
            }
        }
    }
    // the silk thread: a thin white cylinder from the spider to its anchor, drawn only while a thread is held
    let mut thread = 0u64;
    {
        let mesh = Sym(world.strings_mut().intern("cylinder"));
        let t = Transform { pos: [0.0; 3], rot: [0.0, 0.0, 0.0, 1.0], scale: [0.012 * s, 0.01 * s, 0.012 * s] };
        let m = MeshRenderer { mesh, color: [0.92, 0.94, 1.0, 1.0], roughness: 0.4, visible: false, emissive: 0.25, ..Default::default() };
        if let Ok(e) = world.spawn_with3(t, m, CreaturePart { owner: Uid(owner), index: 200 }) {
            if let Some(uid) = world.uid_of(e) {
                thread = uid;
            }
        }
    }
    (parts, decor, thread)
}

fn despawn_visuals(world: &mut World, l: &Live) {
    for uid in l.parts.iter().chain(l.decor.iter()).chain(std::iter::once(&l.thread)) {
        if let Some(e) = world.entity(*uid) {
            world.despawn(e);
        }
    }
}

/// One tick of every creature of the world. `scale` is the world's time scale (1 normally, 0 paused).
pub fn step(world: &mut World, st: &mut Creatures, fields: &Terrains, root: Option<&Path>, scale: f32) {
    // who is there (a world without a creature pays for this one query)
    let mut want: Vec<(u64, Creature, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&Creature, &Transform), _>(|_, uid, (c, t)| want.push((uid, *c, *t)));
    // the creatures that are gone take their parts with them
    if !st.live.is_empty() {
        let gone: Vec<u64> = st.live.keys().copied().filter(|u| !want.iter().any(|w| w.0 == *u)).collect();
        for u in gone {
            if let Some(l) = st.live.remove(&u) {
                despawn_visuals(world, &l);
            }
        }
    }
    if want.is_empty() {
        st.acc = 0.0;
        return;
    }
    want.sort_by_key(|w| w.0);
    let Some(root) = root else { return };
    // time can be slowed, paused and sped up; a creature of another size lives on a clock of its own (a quarter-size animal has a second half as long)
    let rate = scale.clamp(0.0, 8.0);
    let grounds_list = grounds(world, fields);
    let boxes = static_boxes(world);
    // the food and the places it can be (only looked up when somebody forages)
    let mut foods: Vec<(u64, lunaris_ecs::Entity, [f32; 3])> = Vec::new();
    let mut spots: Vec<[f32; 3]> = Vec::new();
    if want.iter().any(|w| w.1.forage) {
        let _ = world.for_each_uid::<(&Food, &Transform), _>(|e, uid, (_, t)| foods.push((uid, e, t.pos)));
        foods.sort_by_key(|f| f.0);
        let mut raw: Vec<(u64, [f32; 3])> = Vec::new();
        let _ = world.for_each_uid::<(&FoodSpot, &Transform), _>(|_, uid, (_, t)| raw.push((uid, t.pos)));
        raw.sort_by_key(|r| r.0);
        spots = raw.into_iter().map(|r| r.1).collect();
    }
    for (uid, mut cr, t) in want {
        let s = cr.size.clamp(0.02, 8.0);
        let brain_name = world.strings().get(cr.brain.0).to_string();
        let model_name = world.strings().get(cr.model.0).to_string();
        let key = (brain_name.clone(), model_name.clone(), cr.genome, cr.body_seed, s.to_bits());
        let stale = st.live.get(&uid).map(|l| l.key != key).unwrap_or(false);
        if stale {
            if let Some(l) = st.live.remove(&uid) {
                despawn_visuals(world, &l);
            }
        }
        if !st.live.contains_key(&uid) {
            if brain_name.is_empty() || !cr.enabled {
                continue;
            }
            let made = read_brain(root, &brain_name, cr.genome as usize).and_then(|b| read_model(root, &model_name).map(|m| (b, m)));
            match made {
                Err(e) => {
                    st.errors.insert(brain_name.clone(), e);
                    continue;
                }
                Ok((brain, model)) => {
                    st.errors.remove(&brain_name);
                    let ground = WorldGround { placed: grounds_list.iter().map(|g| g.placed()).collect(), floor: t.pos[1], s, boxes: &boxes };
                    let mut c = lc::Creature::new(&model, brain, cr.body_seed as u64, &ground);
                    c.place(t.pos[0] / s, t.pos[2] / s, yaw_of(t.rot), &ground);
                    let (parts, decor, thread) = spawn_visuals(world, uid, &model, &c, cr.visible, s);
                    st.live.insert(uid, Live { c, parts, decor, thread, floor: t.pos[1], key, visible: cr.visible, size: s, acc: 0.0, eaten_seen: cr.eaten });
                }
            }
        }
        let Some(l) = st.live.get_mut(&uid) else { continue };
        l.acc += rate / s.sqrt();
        let n_steps = l.acc.floor() as u32;
        l.acc -= n_steps as f32;
        // a foraging creature goes to the nearest food; when it is there it eats it, the food appears at a place of its own choosing elsewhere, and the creature goes for the nearest again
        if cr.forage {
            let hc = l.c.pose(0).0;
            let head = [hc.x * s, hc.y * s, hc.z * s];
            let d2 = |p: [f32; 3]| (p[0] - head[0]) * (p[0] - head[0]) + (p[1] - head[1]) * (p[1] - head[1]) + (p[2] - head[2]) * (p[2] - head[2]);
            // a food it has given up on is not looked at for a while
            let ban = if cr.give_up_left > 0.0 { cr.given_up.0 } else { 0 };
            let nearest = |foods: &Vec<(u64, lunaris_ecs::Entity, [f32; 3])>, skip: u64| -> Option<usize> {
                let mut best: Option<usize> = None;
                for (i, f) in foods.iter().enumerate() {
                    if f.0 == skip && foods.len() > 1 {
                        continue;
                    }
                    if best.map_or(true, |b| d2(f.2) < d2(foods[b].2)) {
                        best = Some(i);
                    }
                }
                best
            };
            let mut pick = foods.iter().position(|f| f.0 == cr.target.0 && f.0 != ban).or_else(|| nearest(&foods, ban));
            if let Some(i) = pick {
                if d2(foods[i].2).sqrt() < 0.32 * s && !spots.is_empty() {
                    cr.eaten += 1;
                    cr.best = 0.0;
                    cr.stuck = 0.0;
                    let here = foods[i].2;
                    let mut sp = spots[0];
                    for _ in 0..8 {
                        let k = (lc::creature::splitmix(&mut st.food_rng) % spots.len() as u64) as usize;
                        sp = spots[k];
                        let far = ((sp[0] - here[0]).powi(2) + (sp[1] - here[1]).powi(2) + (sp[2] - here[2]).powi(2)).sqrt() > 0.5;
                        if d2(sp).sqrt() > 1.5 && far {
                            break;
                        }
                    }
                    foods[i].2 = sp;
                    if let Some(tr) = world.get_mut::<Transform>(foods[i].1) {
                        tr.pos = sp;
                    }
                    if let Some(f) = world.get_mut::<Food>(foods[i].1) {
                        f.eaten += 1;
                    }
                    pick = nearest(&foods, ban);
                }
            }
            // it gives up on a food it cannot get closer to (one up on a table it cannot climb) and tries another
            if cr.patience > 0.0 {
                let dt = rate / 60.0;
                cr.give_up_left = (cr.give_up_left - dt).max(0.0);
                if let Some(i) = pick {
                    let d = d2(foods[i].2).sqrt();
                    if cr.target.0 != foods[i].0 || !(cr.best > 0.0) || d < cr.best - 0.05 * s {
                        cr.best = d;
                        cr.stuck = 0.0;
                    } else {
                        cr.stuck += dt;
                    }
                    if cr.stuck > cr.patience && foods.len() > 1 {
                        cr.given_up = Uid(foods[i].0);
                        cr.give_up_left = cr.patience * 3.0;
                        cr.stuck = 0.0;
                        cr.best = 0.0;
                        let skip = foods[i].0;
                        pick = nearest(&foods, skip);
                    }
                }
            }
            cr.target = pick.map(|i| Uid(foods[i].0)).unwrap_or(Uid(0));
        }
        // where it walks to: an entity, or the point `goal` (in the world; the creature gets it in its own units)
        let target_w = if cr.target.0 != 0 {
            match world.entity(cr.target.0).and_then(|e| world.get::<Transform>(e).map(|t| [t.pos[0], t.pos[1], t.pos[2]])) {
                Some(p) => p,
                None => cr.goal,
            }
        } else {
            cr.goal
        };
        let target = [target_w[0] / s, target_w[1] / s, target_w[2] / s];
        // a far target is seen as a nearer one in the same direction: the brains were trained on targets a few metres of their own size away (a far one would be beyond what they ever sensed), so it walks towards it
        // in stretches and the place it is heading for is always one it understands
        let target = {
            let hc = l.c.pose(0).0;
            let rel = [target[0] - hc.x, target[1] - hc.y, target[2] - hc.z];
            let d = (rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2]).sqrt();
            if d > FAR_TARGET {
                let k = FAR_TARGET / d;
                [hc.x + rel[0] * k, hc.y + rel[1] * k, hc.z + rel[2] * k]
            } else {
                target
            }
        };
        // moving in bursts, as a spider does: a meal is followed by a rest, a walk is broken by pauses to scan, an alarm freezes it (it stands, its brain waits)
        {
            let dt = rate / 60.0;
            let rnd = |r: &mut u64| (lc::creature::splitmix(r) >> 40) as f32 / 16_777_216.0;
            if cr.eaten > l.eaten_seen {
                l.eaten_seen = cr.eaten;
                if cr.rest > 0.0 {
                    cr.pause_left = cr.rest * (0.5 + rnd(&mut st.food_rng));
                    cr.walked = 0.0;
                }
            }
            if cr.pause_left > 0.0 {
                cr.pause_left = (cr.pause_left - dt).max(0.0);
            } else if cr.alarm <= 0.0 {
                cr.walked += dt;
                if cr.scan_every > 0.0 && cr.walked > cr.scan_every * (0.5 + rnd(&mut st.food_rng)) {
                    cr.pause_left = cr.scan_for * (0.5 + rnd(&mut st.food_rng));
                    cr.walked = 0.0;
                }
            }
            if cr.alarm > 0.0 {
                cr.alarm = (cr.alarm - dt).max(0.0);
            }
        }
        let idle = cr.pause_left > 0.0 || cr.alarm > 0.0;
        if cr.enabled {
            let ground = WorldGround { placed: grounds_list.iter().map(|g| g.placed()).collect(), floor: l.floor, s, boxes: &boxes };
            for _ in 0..n_steps {
                if idle {
                    l.c.stand(&ground);
                } else {
                    l.c.step(target, &ground);
                }
            }
        }
        // the parts follow the body (in the world's units)
        let (head_p, head_q) = l.c.pose(0);
        let (abd_p, abd_q) = l.c.pose(1);
        for (i, puid) in l.parts.iter().enumerate() {
            if let Some(e) = world.entity(*puid) {
                let (p, q) = l.c.pose(i);
                if let Some(tr) = world.get_mut::<Transform>(e) {
                    tr.pos = [p.x * s, p.y * s, p.z * s];
                    tr.rot = q;
                }
                if l.visible != cr.visible {
                    if let Some(m) = world.get_mut::<MeshRenderer>(e) {
                        m.visible = cr.visible;
                    }
                }
            }
        }
        {
            use lc::visuals::decor_world;
            let decs = lc::visuals::decorations();
            for (k, duid) in l.decor.iter().enumerate() {
                if let Some(e) = world.entity(*duid) {
                    let d = &decs[k];
                    let (bp, bq) = if d.parent == 0 { (head_p, head_q) } else { (abd_p, abd_q) };
                    let (pos, rot) = decor_world(bp, bq, d.offset, d.rot);
                    if let Some(tr) = world.get_mut::<Transform>(e) {
                        tr.pos = [pos[0] * s, pos[1] * s, pos[2] * s];
                        tr.rot = rot;
                    }
                    if l.visible != cr.visible {
                        if let Some(m) = world.get_mut::<MeshRenderer>(e) {
                            m.visible = cr.visible;
                        }
                    }
                }
            }
        }
        // the thread follows the spider (and is hidden when none is held)
        if let Some(e) = world.entity(l.thread) {
            let ends = l.c.thread_ends();
            if let Some((a, b)) = ends {
                let d = b - a;
                let len = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
                if len > 1.0e-4 {
                    let dir = [d.x / len, d.y / len, d.z / len];
                    if let Some(tr) = world.get_mut::<Transform>(e) {
                        tr.pos = [(a.x + b.x) * 0.5 * s, (a.y + b.y) * 0.5 * s, (a.z + b.z) * 0.5 * s];
                        tr.rot = lc::visuals::q_y_to(dir);
                        tr.scale = [0.012 * s, len * s, 0.012 * s];
                    }
                }
            }
            if let Some(m) = world.get_mut::<MeshRenderer>(e) {
                m.visible = ends.is_some() && cr.visible;
            }
        }
        l.visible = cr.visible;
        // what the game may read, in the world's units: the creature rides on its prosoma, and its state
        if let Some(e) = world.entity(uid) {
            let (dx, dy, dz) = (target_w[0] - head_p.x * s, target_w[1] - head_p.y * s, target_w[2] - head_p.z * s);
            let vel = l.c.velocity(0);
            let feet = l.c.foot.iter().filter(|f| **f).count() as u32;
            let up = l.c.up();
            let ground = WorldGround { placed: grounds_list.iter().map(|g| g.placed()).collect(), floor: l.floor, s, boxes: &boxes };
            let gh = lc::Ground::height(&ground, head_p.x, head_p.z);
            if let Some(tr) = world.get_mut::<Transform>(e) {
                tr.pos = [head_p.x * s, head_p.y * s, head_p.z * s];
                tr.rot = head_q;
            }
            if let Some(c) = world.get_mut::<Creature>(e) {
                c.distance = (dx * dx + dy * dy + dz * dz).sqrt();
                c.speed = (vel.x * vel.x + vel.z * vel.z).sqrt() * s.sqrt();
                c.height = (head_p.y - gh) * s;
                c.upright = up.y;
                c.feet = feet;
                c.web = l.c.anchored;
                c.eaten = cr.eaten;
                c.pause_left = cr.pause_left;
                c.walked = cr.walked;
                c.alarm = cr.alarm;
                if cr.forage {
                    c.target = cr.target;
                    c.stuck = cr.stuck;
                    c.best = cr.best;
                    c.given_up = cr.given_up;
                    c.give_up_left = cr.give_up_left;
                }
                c.tension = l.c.silk_tension * s * s * s;
            }
        }
    }
}
