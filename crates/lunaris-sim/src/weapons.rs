//! Weapons and bullets (docs/COMBAT.md): the shooting half of a game, as data. A `Weapon` fires along its entity's local -z when `fire` is true, as an instant ray (a hitscan) or as a
//! `Bullet` that flies and falls; the first wall or target in the way takes the round; damage falls off with distance and differs for the head and the legs; the cone of fire opens with
//! every shot (bloom) and the recoil piles up in `kick` for the camera or the aim to use. Targets are the entities that have `HitPoints` (and a `CharacterBody`, or a `Collider` for a
//! crate, a drone or a turret): they are hurt by lowering `HitPoints.value`, so the usual `died` event, regeneration and `invulnerable` grace of `HitPoints` apply, and nobody is hurt
//! by their own side (`Team`). It draws and plays nothing: a game answers the events `shot`, `hit`, `impact`, `kill`, `reload_done` and `dry` (the muzzle flash and the sound at the
//! muzzle, the spark on the wall, the hit marker) with its own assets.
//!
//! Everything is deterministic: the spread comes from a hash of the tick, the weapon and the shot, never from a generator with state; targets and weapons are visited in uid order; a hit
//! at the same distance goes to the lowest uid.
use crate::components::{Bullet, CharacterBody, Collider, HitPoints, Parent, Team, Transform, Weapon};
use crate::detmath;
use crate::layers::{Filter, Layers};
use crate::meshcol::MeshStore;
use crate::physics_rot::{axes_of, normalize_q, M3};
use crate::rng::hash_rand;
use crate::terrain::Terrains;
use crate::vehicle_ray::{ray_capsule, ray_obb, ray_sphere, RayWorld};
use lunaris_ecs::{Entity, Uid, World};

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

const SEED: u64 = 0x57E4_9D05_C0DE;
/// The most rounds one weapon fires in a tick (a very high rate of fire at a low tick rate).
const MAX_PER_TICK: u32 = 4;
/// The most pellets one round has.
const MAX_PELLETS: u32 = 32;

#[inline]
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l > 1.0e-9 {
        scale(a, 1.0 / l)
    } else {
        [0.0, 0.0, -1.0]
    }
}

fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Shape {
    Capsule(V3, V3, f32),
    Sphere(V3, f32),
    Obb(V3, M3, V3),
}

/// Something that can be shot: an entity with `HitPoints`.
pub(crate) struct Target {
    pub(crate) uid: u64,
    /// Where its entity is (the feet of a character, the middle of anything else).
    pub(crate) pos: V3,
    pub(crate) shape: Shape,
    pub(crate) team: u32,
    /// For a character: where its feet are and how tall it is, to tell the head from the legs (a crate has one zone).
    pub(crate) feet: f32,
    pub(crate) height: f32,
    pub(crate) zones: bool,
}

impl Target {
    /// The same target with its entity at `p` instead (a shooter's view of the past: [`Rewind`]).
    fn moved_to(&self, p: V3) -> Target {
        let d = [p[0] - self.pos[0], p[1] - self.pos[1], p[2] - self.pos[2]];
        let shape = match self.shape {
            Shape::Capsule(a, b, r) => Shape::Capsule(add(a, d), add(b, d), r),
            Shape::Sphere(c, r) => Shape::Sphere(add(c, d), r),
            Shape::Obb(c, ax, h) => Shape::Obb(add(c, d), ax, h),
        };
        Target { uid: self.uid, pos: p, shape, team: self.team, feet: self.feet + d[1], height: self.height, zones: self.zones }
    }

    /// The middle of it: what an AI aims at and looks at.
    pub(crate) fn center(&self) -> V3 {
        match self.shape {
            Shape::Capsule(a, b, _) => scale(add(a, b), 0.5),
            Shape::Sphere(c, _) => c,
            Shape::Obb(c, _, _) => c,
        }
    }
}

/// What each shooter saw (lag compensation, docs/LAG_COMPENSATION.md): a game server that hosts the simulation tells, before a step, where the shooters' targets were when the shooters pulled the trigger on their own
/// screens. For a shooter (the uid of the weapon's owner, or of the weapon without one) the list holds `(target uid, where its entity was)`, **sorted by target uid**; a hitscan round of that shooter is tested against
/// the targets named there as if they stood in those places, and against everything else where it is. Rounds that fly (`Weapon.kind` 1) are not rewound. Host policy, not state: it is not saved, not in the state
/// hash and `Sim::step` never changes it.
#[derive(Clone, Debug, Default)]
pub struct Rewind {
    pub shooters: std::collections::BTreeMap<u64, Vec<(u64, V3)>>,
}

impl Rewind {
    pub fn clear(&mut self) {
        self.shooters.clear();
    }
}

pub(crate) fn targets(world: &mut World) -> Vec<Target> {
    let mut raw: Vec<(Entity, u64, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&HitPoints, &Transform), _>(|e, uid, (h, t)| {
        if !h.dead && t.pos.iter().all(|v| v.is_finite()) {
            raw.push((e, uid, *t));
        }
    });
    raw.sort_by_key(|r| r.1);
    let mut out = Vec::with_capacity(raw.len());
    for (e, uid, t) in raw {
        let team = world.get::<Team>(e).map_or(0, |x| x.id);
        if let Some(cb) = world.get::<CharacterBody>(e) {
            let r = finite(cb.radius, 0.35).max(0.05);
            let h = finite(cb.height, 1.8).max(2.0 * r);
            let (a, b) = ([t.pos[0], t.pos[1] + r, t.pos[2]], [t.pos[0], t.pos[1] + h - r, t.pos[2]]);
            out.push(Target { uid, pos: t.pos, shape: Shape::Capsule(a, b, r), team, feet: t.pos[1], height: h, zones: true });
        } else if let Some(c) = world.get::<Collider>(e) {
            if c.trigger {
                continue;
            }
            let s = [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()];
            let axes = axes_of(normalize_q(t.rot));
            let shape = if c.shape == 0 {
                Shape::Sphere(t.pos, 0.5 * c.size[0].abs() * s[0].max(s[1]).max(s[2]))
            } else if c.shape == 2 {
                let r = 0.5 * c.size[0].abs() * s[0].max(s[2]);
                let half = (0.5 * c.size[1].abs() * s[1] - r).max(0.0);
                Shape::Capsule(add(t.pos, scale(axes[1], -half)), add(t.pos, scale(axes[1], half)), r)
            } else {
                Shape::Obb(t.pos, axes, [0.5 * c.size[0].abs() * s[0], 0.5 * c.size[1].abs() * s[1], 0.5 * c.size[2].abs() * s[2]])
            };
            let (feet, height) = match shape {
                Shape::Capsule(a, b, r) => (a[1].min(b[1]) - r, (a[1] - b[1]).abs() + 2.0 * r),
                Shape::Sphere(c, r) => (c[1] - r, 2.0 * r),
                Shape::Obb(c, _, h) => (c[1] - h[1], 2.0 * h[1]),
            };
            out.push(Target { uid, pos: t.pos, shape, team, feet, height: height.max(0.01), zones: false });
        }
    }
    out
}

/// What a shot hit: the distance along the ray, the point, the surface normal, the victim (0 for a wall) and the zone (1 body, 2 head, 3 legs; 0 for a wall).
struct Shot {
    t: f32,
    point: V3,
    normal: V3,
    victim: u64,
    zone: u32,
}

fn zone_of(tg: &Target, y: f32) -> u32 {
    if !tg.zones {
        return 1;
    }
    let f = (y - tg.feet) / tg.height.max(0.01);
    if f >= 0.8 {
        2
    } else if f < 0.45 {
        3
    } else {
        1
    }
}

/// The nearest thing the ray `o + t d` (`d` a unit vector), `0 <= t <= max`, meets: the static world, or a target that is not the shooter's own side.
#[allow(clippy::too_many_arguments)]
fn resolve(rw: &RayWorld, targets: &[Target], over: Option<&[(u64, V3)]>, o: V3, d: V3, max: f32, who: Filter, owner: u64, weapon: u64, team: u32) -> Option<Shot> {
    let mut best: Option<Shot> = None;
    if let Some(h) = rw.cast(o, d, max, who, owner) {
        best = Some(Shot { t: h.t, point: add(o, scale(d, h.t)), normal: h.normal, victim: 0, zone: 0 });
    }
    for tg in targets {
        if tg.uid == owner || tg.uid == weapon || (team != 0 && tg.team == team) {
            continue;
        }
        // (a shooter's view of the past: this target where the shooter saw it)
        let moved;
        let tg = match over.and_then(|l| l.binary_search_by_key(&tg.uid, |e| e.0).ok().map(|i| l[i].1)) {
            Some(p) => {
                moved = tg.moved_to(p);
                &moved
            }
            None => tg,
        };
        let hit = match tg.shape {
            Shape::Capsule(a, b, r) => ray_capsule(o, d, a, b, r),
            Shape::Sphere(c, r) => ray_sphere(o, d, c, r),
            Shape::Obb(c, ax, h) => ray_obb(o, d, c, &ax, h),
        };
        if let Some((t, n)) = hit {
            if t <= max && best.as_ref().is_none_or(|b| t < b.t) {
                let point = add(o, scale(d, t));
                best = Some(Shot { t, point, normal: n, victim: tg.uid, zone: zone_of(tg, point[1]) });
            }
        }
    }
    best
}

/// Damage at a distance: full up to `falloff_start`, falling in a straight line to `falloff_min` times at `range`, nothing beyond.
fn falloff(base: f32, dist: f32, range: f32, start: f32, min: f32) -> f32 {
    if dist > range {
        return 0.0;
    }
    let f = if dist <= start || range <= start { 1.0 } else { 1.0 - (1.0 - min.clamp(0.0, 1.0)) * ((dist - start) / (range - start)).clamp(0.0, 1.0) };
    base * f
}

fn zone_mult(zone: u32, head: f32, limb: f32) -> f32 {
    match zone {
        2 => head,
        3 => limb,
        _ => 1.0,
    }
}

/// Take `amount` off a victim's `HitPoints`; true when this hit is the one that brought it to 0 (a victim in its grace period, or already dead, takes nothing).
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

/// A bullet's hit, to be written into the gun that fired it once the guns of this tick are written back.
struct Credit {
    weapon: u64,
    victim: u64,
    point: V3,
    normal: V3,
    zone: u32,
    dist: f32,
    damage: f32,
    killed: bool,
}

struct Pending {
    widx: usize,
    rays: Vec<(V3, V3)>,
}

struct Held {
    e: Entity,
    uid: u64,
    w: Weapon,
    t: Transform,
    owner: u64,
    team: u32,
}

fn start_reload(w: &mut Weapon, events: &mut Events, uid: u64) {
    if w.reload_time <= 0.0 {
        finish_reload(w, events, uid);
    } else {
        w.reload_left = w.reload_time;
    }
}

fn finish_reload(w: &mut Weapon, events: &mut Events, uid: u64) {
    let need = w.mag_size.saturating_sub(w.mag);
    let take = if w.infinite { need } else { need.min(w.reserve) };
    w.mag += take;
    if !w.infinite {
        w.reserve -= take;
    }
    w.reload_left = 0.0;
    events.push(("reload_done".to_string(), uid, None));
}

pub fn update(world: &mut World, tick: u64, dt: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events, rewind: &Rewind) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let mut guns: Vec<Held> = Vec::new();
    let mut raw: Vec<(Entity, u64, Weapon, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&Weapon, &Transform), _>(|e, uid, (w, t)| raw.push((e, uid, *w, *t)));
    raw.sort_by_key(|r| r.1);
    for (e, uid, w, t) in raw {
        // the holder: named, or what the weapon is attached to
        let owner = if w.owner.0 != 0 { w.owner.0 } else { world.get::<Parent>(e).map_or(0, |p| p.target.0) };
        let team = world.entity(owner).and_then(|oe| world.get::<Team>(oe)).or_else(|| world.get::<Team>(e)).map_or(0, |x| x.id);
        guns.push(Held { e, uid, w, t, owner, team });
    }
    let mut bullets: Vec<(Entity, u64, Bullet, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&Bullet, &Transform), _>(|e, uid, (b, t)| bullets.push((e, uid, *b, *t)));
    bullets.sort_by_key(|r| r.1);
    if guns.is_empty() && bullets.is_empty() {
        return;
    }

    // ---- the triggers: timers, ammo, recoil, and the rays and bullets that leave this tick
    let mut pending: Vec<Pending> = Vec::new();
    let mut new_bullets: Vec<(V3, [f32; 4], Bullet)> = Vec::new();
    let mut credits: Vec<Credit> = Vec::new();
    for (gi, g) in guns.iter_mut().enumerate() {
        let (uid, t) = (g.uid, g.t);
        let w = &mut g.w;
        w.fired = false;
        w.dry = false;
        w.hit = Uid(0);
        w.hit_zone = 0;
        w.hit_damage = 0.0;
        w.hit_dist = 0.0;
        w.hit_point = [0.0; 3];
        w.hit_normal = [0.0; 3];
        if !w.active {
            w.held = false;
            w.reload = false;
            continue;
        }
        w.bloom = (finite(w.bloom, 0.0) - finite(w.bloom_recover, 0.0).max(0.0) * dt).max(0.0);
        let step = finite(w.recoil_recover, 0.0).max(0.0) * dt;
        for k in 0..2 {
            let a = finite(w.kick[k], 0.0);
            w.kick[k] = if a.abs() <= step { 0.0 } else { a - step * a.signum() };
        }
        if w.reload_left > 0.0 {
            w.reload_left -= dt;
            if w.reload_left <= 0.0 {
                finish_reload(w, events, uid);
            }
        }
        let can_reload = w.reload_left <= 0.0 && w.mag < w.mag_size && (w.infinite || w.reserve > 0);
        if w.reload && can_reload {
            start_reload(w, events, uid);
        }
        w.reload = false;
        let pulled = w.fire && (w.auto || !w.held);
        w.held = w.fire;
        w.cooldown -= dt;
        let interval = 60.0 / finite(w.rate, 600.0).max(1.0);
        if w.reload_left > 0.0 {
            w.cooldown = w.cooldown.max(0.0);
            continue;
        }
        if !pulled {
            w.cooldown = w.cooldown.max(0.0);
            continue;
        }
        if w.mag == 0 {
            if w.infinite || w.reserve > 0 {
                start_reload(w, events, uid);
            } else {
                w.dry = true;
                events.push(("dry".to_string(), uid, None));
            }
            w.cooldown = w.cooldown.max(0.0);
            continue;
        }
        // the transform of the gun: its own axes, forward is local -z
        let ax = axes_of(normalize_q(t.rot));
        let (right, up, back) = (ax[0], ax[1], ax[2]);
        let fwd = scale(back, -1.0);
        let origin = add(t.pos, add(add(scale(right, w.muzzle[0]), scale(up, w.muzzle[1])), scale(back, w.muzzle[2])));
        let mut rounds = 0;
        // (a semi-automatic weapon fires one round for each pull of the trigger, however high its rate)
        let most = if w.auto { MAX_PER_TICK } else { 1 };
        while w.cooldown <= 0.0 && w.mag > 0 && rounds < most {
            let n = w.pellets.clamp(1, MAX_PELLETS);
            let tan_cone = detmath::tan((finite(w.spread, 0.0).max(0.0) + w.bloom).min(80.0).to_radians() as f64) as f32;
            let lean_y = detmath::tan((finite(w.kick[1], 0.0) as f64).to_radians().clamp(-1.2, 1.2)) as f32;
            let lean_p = detmath::tan((finite(w.kick[0], 0.0) as f64).to_radians().clamp(-1.2, 1.2)) as f32;
            let mut rays: Vec<(V3, V3)> = Vec::with_capacity(n as usize);
            for p in 0..n {
                let salt = (w.shots as u64) * 64 + p as u64;
                let u1 = hash_rand(SEED, tick, uid, salt * 4);
                let u2 = hash_rand(SEED, tick, uid, salt * 4 + 1);
                let r = tan_cone * u1.sqrt();
                let ang = std::f64::consts::TAU * u2 as f64;
                let (ox, oy) = (r * detmath::cos(ang) as f32, r * detmath::sin(ang) as f32);
                let d = norm(add(fwd, add(scale(right, ox + lean_y), scale(up, oy + lean_p))));
                rays.push((origin, d));
            }
            if w.kind == 1 {
                for (o, d) in &rays {
                    let b = Bullet {
                        owner: Uid(g.owner),
                        weapon: Uid(uid),
                        team: g.team,
                        damage: w.damage,
                        range: w.range,
                        falloff_start: w.falloff_start,
                        falloff_min: w.falloff_min,
                        head_mult: w.head_mult,
                        limb_mult: w.limb_mult,
                        pierce: w.pierce,
                        vel: scale(*d, finite(w.muzzle_speed, 400.0).max(1.0)),
                        gravity: finite(w.gravity, 9.81),
                        life: finite(w.bullet_life, 4.0).max(0.05),
                        age: 0.0,
                        travelled: 0.0,
                    };
                    new_bullets.push((*o, t.rot, b));
                }
            } else {
                pending.push(Pending { widx: gi, rays });
            }
            w.mag -= 1;
            w.shots = w.shots.wrapping_add(1);
            w.fired = true;
            w.bloom = (w.bloom + finite(w.bloom_per_shot, 0.0).max(0.0)).min(finite(w.bloom_max, 0.0).max(0.0));
            let side = hash_rand(SEED, tick, uid, (w.shots as u64) * 64 + 63) * 2.0 - 1.0;
            w.kick[0] += finite(w.recoil[0], 0.0);
            w.kick[1] += side * finite(w.recoil[1], 0.0);
            w.cooldown += interval;
            rounds += 1;
            events.push(("shot".to_string(), uid, Some(origin)));
        }
    }

    // ---- what the rays and the bullets meet
    let need_world = !pending.is_empty() || !bullets.is_empty();
    if need_world {
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        let mut grow = |a: V3, b: V3| {
            for k in 0..3 {
                lo[k] = lo[k].min(a[k].min(b[k]));
                hi[k] = hi[k].max(a[k].max(b[k]));
            }
        };
        for p in &pending {
            let range = guns[p.widx].w.range.max(0.0);
            for (o, d) in &p.rays {
                grow(*o, add(*o, scale(*d, range)));
            }
        }
        for (_, _, b, t) in &bullets {
            grow(t.pos, add(t.pos, scale(b.vel, dt * 1.5 + 0.01)));
        }
        let pad = 1.0;
        let reach = (lo.map(|v| v - pad), hi.map(|v| v + pad));
        // (what can be shot is tested as a target below, not as a wall: a crate that has `HitPoints` must not also stop the round)
        let tgs = targets(world);
        let except: std::collections::BTreeSet<u64> = tgs.iter().map(|t| t.uid).collect();
        let rw = RayWorld::gather_except(world, fields, meshes, Some(reach), &except);
        let layers = Layers::gather(world);

        for p in &pending {
            let (uid, owner, team, w) = {
                let g = &guns[p.widx];
                (g.uid, g.owner, g.team, g.w)
            };
            let who = layers.of(if owner != 0 { owner } else { uid });
            let over = rewind.shooters.get(&if owner != 0 { owner } else { uid }).map(|v| v.as_slice());
            let mut total = 0.0f32;
            let mut kills = 0u32;
            let mut mine = guns[p.widx].w;
            let mut first = mine.hit_dist == 0.0 && mine.hit.0 == 0 && mine.hit_zone == 0;
            for (ray_n, (o, d)) in p.rays.iter().enumerate() {
                let Some(shot) = resolve(&rw, &tgs, over, *o, *d, finite(w.range, 200.0).max(0.0), who, owner, uid, team) else { continue };
                let dmg = if shot.victim != 0 { falloff(w.damage, shot.t, w.range, w.falloff_start, w.falloff_min) * zone_mult(shot.zone, w.head_mult, w.limb_mult) } else { 0.0 };
                if shot.victim != 0 {
                    // (worn gear stops its share of the hit where it landed: docs/EQUIPMENT.md)
                    let dmg = crate::blast::resist(world, shot.victim, dmg);
                    let dmg = crate::armor::absorb_piercing(world, shot.victim, dmg, shot.zone, w.pierce, events);
                    crate::survival::wound(world, shot.victim, dmg, tick, uid.wrapping_mul(64).wrapping_add(ray_n as u64));
                    let killed = hurt(world, shot.victim, dmg);
                    events.push(("hit".to_string(), shot.victim, Some(shot.point)));
                    if killed {
                        kills += 1;
                        events.push(("kill".to_string(), shot.victim, Some(shot.point)));
                    }
                    total += dmg;
                } else {
                    events.push(("impact".to_string(), uid, Some(shot.point)));
                }
                if first {
                    first = false;
                    mine.hit = Uid(shot.victim);
                    mine.hit_point = shot.point;
                    mine.hit_normal = shot.normal;
                    mine.hit_zone = shot.zone;
                    mine.hit_dist = shot.t;
                }
            }
            mine.hit_damage += total;
            mine.kills = mine.kills.wrapping_add(kills);
            guns[p.widx].w = mine;
        }

        // ---- bullets in flight: each moves, and the stretch it flew is tested
        let mut gone: Vec<Entity> = Vec::new();
        for (e, uid, b, t) in bullets {
            let mut b = b;
            let owner = b.owner.0;
            let who = layers.of(if owner != 0 { owner } else { b.weapon.0 });
            b.age += dt;
            b.vel[1] -= finite(b.gravity, 9.81) * dt;
            let step = scale(b.vel, dt);
            let seg = dot(step, step).sqrt();
            let mut done = b.age >= b.life;
            if seg > 1.0e-9 {
                let d = scale(step, 1.0 / seg);
                if let Some(shot) = resolve(&rw, &tgs, None, t.pos, d, seg, who, owner, uid, b.team) {
                    let dist = b.travelled + shot.t;
                    let mut killed = false;
                    let mut dmg = 0.0;
                    if shot.victim != 0 {
                        dmg = falloff(b.damage, dist, b.range, b.falloff_start, b.falloff_min) * zone_mult(shot.zone, b.head_mult, b.limb_mult);
                        dmg = crate::blast::resist(world, shot.victim, dmg);
                        dmg = crate::armor::absorb_piercing(world, shot.victim, dmg, shot.zone, b.pierce, events);
                        crate::survival::wound(world, shot.victim, dmg, tick, uid);
                        killed = hurt(world, shot.victim, dmg);
                        events.push(("hit".to_string(), shot.victim, Some(shot.point)));
                        if killed {
                            events.push(("kill".to_string(), shot.victim, Some(shot.point)));
                        }
                    } else {
                        events.push(("impact".to_string(), b.weapon.0, Some(shot.point)));
                    }
                    credits.push(Credit { weapon: b.weapon.0, victim: shot.victim, point: shot.point, normal: shot.normal, zone: shot.zone, dist, damage: dmg, killed });
                    done = true;
                }
                b.travelled += seg;
                if b.travelled > b.range {
                    done = true;
                }
            }
            if done {
                gone.push(e);
            } else {
                if let Some(tr) = world.get_mut::<Transform>(e) {
                    tr.pos = add(t.pos, step);
                }
                if let Some(bb) = world.get_mut::<Bullet>(e) {
                    *bb = b;
                }
            }
        }
        for e in gone {
            world.despawn(e);
        }
    }

    // ---- write the guns back, then what the bullets of this tick hit (the first hit of a tick is the one described), and make the bullets that left this tick
    for g in &guns {
        if let Some(w) = world.get_mut::<Weapon>(g.e) {
            *w = g.w;
        }
    }
    for c in credits {
        let Some(we) = world.entity(c.weapon) else { continue };
        let Some(gw) = world.get_mut::<Weapon>(we) else { continue };
        if gw.hit_dist == 0.0 && gw.hit.0 == 0 && gw.hit_zone == 0 {
            gw.hit = Uid(c.victim);
            gw.hit_point = c.point;
            gw.hit_normal = c.normal;
            gw.hit_zone = c.zone;
            gw.hit_dist = c.dist;
        }
        gw.hit_damage += c.damage;
        if c.killed {
            gw.kills = gw.kills.wrapping_add(1);
        }
    }
    for (pos, rot, b) in new_bullets {
        let _ = world.spawn_with2(Transform { pos, rot, ..Default::default() }, b);
    }
}
