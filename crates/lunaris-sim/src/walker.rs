//! The parts of a heavy ground machine (docs/WALKER.md), each a component of its own that a designer puts on a body that already has a `Perception` (docs/AI.md), a `NavAgent` (docs/NAVIGATION.md) and a `CharacterBody`:
//!
//! * `Pursuit` is its legs: it patrols a list of points while calm, walks to the target it sees (stopping `close` metres from it), searches the place it last saw or heard something, turns its body at a rate (a big machine does not
//!   snap round), and holds still while its `Stomp` is winding up or recovering. The `Perception` autopilot (`engage`) is left off: that one snaps to face its target and walks to 85 percent of the attack range.
//! * `Cannon` is its gun: a `Weapon` of `kind` 1 on a child that it aims (the barrel's pitch, solved for the shell's speed and gravity, with a lead on a moving target) and fires while the body faces the target; a shell that lands also
//!   hurts what is within a few metres of the spot ([`crate::stomp::area_damage`]). Its shells are told to the players of a game server when `net_kind` says so.
//! * `Hitbox` is a part of a machine that is shot as a thing of its own (a rear power core, a rotor, a sensor): a small ghost collider with `HitPoints` that nothing walks into; what a shot takes off it is taken off the machine instead, times
//!   `mult`. It needs no change of the weapons system: the machine's plating (`Equipment`) covers the body, and a part that is out in the open takes the shot as it comes.
//!
//! Deterministic: machines are visited in uid order, the aim is arithmetic and the engine's own sin, cos and atan, nothing is random.
use crate::components::{Bullet, CharacterBody, HitPoints, LocalTransform, NavAgent, NetEntity, Parent, Perception, Team, Transform, Weapon};
use crate::detmath;
use crate::physics_rot::{axes_of, normalize_q};
use lunaris_ecs::{component, Entity, Sym, Uid, World};
use std::cell::RefCell;
use std::rc::Rc;

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

component! {
    /// The legs of a heavy machine (docs/WALKER.md), beside its `Perception`, `NavAgent` and `CharacterBody` (leave the perception's `engage` off: this does what `engage` and `chase` would, and turns the body at `turn_rate`). Calm (no
    /// awareness), it walks the points of `patrol` (text: one `x z` a line, `#` starts a comment, at most 32; the height is wherever the ground is) at `calm_speed` metres a second and waits `wait` seconds at each (`patrol_at` is the one it
    /// is going to, `wait_left` what is left of the wait; no points: it holds). Alert to a target it sees (state 2) it walks to it at `alert_speed` and stops `close` metres from it; searching (state 3: it lost the target or heard a
    /// noise) it walks to the place (`Perception.seen_pos`); suspicious (state 1: it has seen something it is not sure of) it stops and faces it. It asks its `NavAgent` for a path when the goal has moved `moved` metres and `replan` seconds have
    /// passed, and after a plan that found no way waits `retry` seconds. The body turns toward the target (or the way it walks) at `turn_rate` degrees a second, and a machine that is facing more than 75 degrees from the way it
    /// must go walks at a quarter of its speed until it has turned. It stands still while its `Stomp` is not ready to be left (`Stomp.phase` is not 0) and when it is dead. `mode` is what it does now (0 hold, 1 patrol, 2 chase,
    /// 3 search); `arrived` says it is standing at a patrol point (waiting there), `goal` is the point it last planned to and `plans` how many plans it has asked for.
    pub struct Pursuit {
        pub active: bool = true,
        pub patrol: Sym = Sym(0),
        pub wait: f32 = 4.0,
        pub calm_speed: f32 = 1.8,
        pub alert_speed: f32 = 3.4,
        pub turn_rate: f32 = 55.0,
        pub close: f32 = 2.6,
        pub replan: f32 = 0.7,
        pub moved: f32 = 2.0,
        pub retry: f32 = 2.5,
        pub mode: u32 = 0,
        pub patrol_at: u32 = 0,
        pub arrived: bool = false,
        pub wait_left: f32 = 0.0,
        pub goal: [f32; 3] = [0.0, 0.0, 0.0],
        pub since_plan: f32 = 99.0,
        pub plans: u32 = 0,
    }
}

component! {
    /// The gun of a heavy machine (docs/WALKER.md), on the machine, whose child (`weapon`, or the first child that has a `Weapon` when 0) is the gun: a `Weapon` of `kind` 1 with a `muzzle` at the end of its barrel. Every tick it aims the
    /// gun (the child's `LocalTransform`: a turn about the vertical, `yaw`, then a pitch) at the target the machine's `Perception` is alert to: the pitch that makes a shell of the weapon's `muzzle_speed` and `gravity` land on it, led by `lead` (0 to 1) of the
    /// way the target is moving, at `aim_height` of the target's height (a character: its `CharacterBody.height`), within `pitch_min` to `pitch_max` degrees, and the turn that points it there, within `arc` degrees either side of the way the body faces (a body
    /// that turns slowly is followed by a barrel that turns faster), both at most `slew` degrees a second. It holds the gun's trigger while the target is seen (state 2) between `min_range` and `range` metres (on the ground plane), is within the reach
    /// of the barrel (`arc` and the pitch limits) and the barrel is within 0.6 degrees of the aim, and not while its `Stomp` is at work or it is dead. A shell that lands
    /// (a hit of the weapon this tick) also hurts what is within `blast_radius` metres of the place: `blast` at the middle, `blast_edge` times that at the radius, `structure` times as much for things that are not characters (it spares a character that
    /// the shell hit directly, which took the shell's own damage; a wall or a vehicle that the shell hit takes the splash on top of the shell's damage); the event `cannon_blast` says where. With `net_kind` above 0 each shell is made a `NetEntity` of class 2 and that kind, so that the players of a game server see it fly. Outputs:
    /// `pitch` and `yaw` (degrees), `aimed`, `firing`, `shots` (rounds fired), `blasts`, `since_shot` (seconds) and what it tracks of the target (`tracked`, `track`, `vel`).
    pub struct Cannon {
        pub active: bool = true,
        pub weapon: Uid = Uid(0),
        pub range: f32 = 80.0,
        pub min_range: f32 = 8.0,
        pub arc: f32 = 25.0,
        pub lead: f32 = 0.6,
        pub aim_height: f32 = 0.55,
        pub pitch_min: f32 = -30.0,
        pub pitch_max: f32 = 35.0,
        pub slew: f32 = 70.0,
        pub blast: f32 = 50.0,
        pub blast_radius: f32 = 3.0,
        pub blast_edge: f32 = 0.3,
        pub structure: f32 = 2.5,
        pub net_kind: u32 = 0,
        pub pitch: f32 = 0.0,
        pub yaw: f32 = 0.0,
        pub aimed: bool = false,
        pub firing: bool = false,
        pub shots: u32 = 0,
        pub blasts: u32 = 0,
        pub since_shot: f32 = 99.0,
        pub tracked: Uid = Uid(0),
        pub track: [f32; 3] = [0.0, 0.0, 0.0],
        pub vel: [f32; 3] = [0.0, 0.0, 0.0],
    }
}

component! {
    /// A part of a machine that is shot as a thing of its own (docs/WALKER.md): put it on a child of the machine that has a small `Collider` (not a trigger; a `CollisionLayer` with `mask` 0 makes it a ghost that nothing walks into, plans round
    /// or is stopped by) and `HitPoints` with a `max` too big to be used up (the engine puts what a shot took off back every tick). What a shot takes off it is taken off the machine `owner` (the parent when 0) times `mult`, with no worn gear in
    /// between: a weak point behind the plating (a power core on the back) is a `Hitbox` that sticks out of the body on that side, so that shots from there meet it first and shots from elsewhere meet the body. `taken` is the damage the part has
    /// taken in all (before `mult`) and `hits` the ticks it was hit in. The part must have the machine's `Team` (the machine's own shells pass through it).
    pub struct Hitbox {
        pub owner: Uid = Uid(0),
        pub mult: f32 = 2.0,
        pub taken: f32 = 0.0,
        pub hits: u32 = 0,
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
fn flat(a: V3, b: V3) -> f32 {
    let (dx, dz) = (a[0] - b[0], a[2] - b[2]);
    (dx * dx + dz * dz).sqrt()
}

/// The turn about the vertical axis that makes local -z point along a heading (the way a `look_rot` does).
fn yaw_quat(yaw: f32) -> [f32; 4] {
    let h = yaw as f64 * 0.5;
    [0.0, detmath::sin(h) as f32, 0.0, detmath::cos(h) as f32]
}

fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// The heading an entity faces (a rotation about the vertical: radians, 0 looking down -z, a quarter turn to the left positive).
fn yaw_of(rot: [f32; 4]) -> f32 {
    let ax = axes_of(normalize_q(rot));
    detmath::atan2(ax[2][0] as f64, ax[2][2] as f64) as f32
}

/// The heading that looks from `from` toward `to` on the ground plane.
fn heading_to(from: V3, to: V3) -> f32 {
    detmath::atan2(-((to[0] - from[0]) as f64), -((to[2] - from[2]) as f64)) as f32
}

/// An angle as a turn of at most half a circle.
fn wrap(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut a = a % tau;
    if a > std::f32::consts::PI {
        a -= tau;
    } else if a < -std::f32::consts::PI {
        a += tau;
    }
    a
}

thread_local! {
    /// The patrol text last read and its points (every machine asks on every tick).
    static PATROLS: RefCell<Option<(String, Rc<Vec<[f32; 2]>>)>> = const { RefCell::new(None) };
}

/// The points of a patrol text, one `x z` a line; lines that are not two numbers are skipped.
pub fn parse_patrol(text: &str) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = Vec::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("");
        let mut w = line.split_whitespace();
        if let (Some(Ok(x)), Some(Ok(z))) = (w.next().map(str::parse::<f32>), w.next().map(str::parse::<f32>)) {
            if x.is_finite() && z.is_finite() && out.len() < 32 {
                out.push([x, z]);
            }
        }
    }
    out
}

fn patrol_of(text: &str) -> Rc<Vec<[f32; 2]>> {
    PATROLS.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((k, p)) = c.as_ref() {
            if k == text {
                return p.clone();
            }
        }
        let p = Rc::new(parse_patrol(text));
        *c = Some((text.to_string(), p.clone()));
        p
    })
}

/// The launch angle (radians above level) that makes a shell of speed `v` under gravity `g` land `x` metres away (level) and `y` metres above the muzzle: the flatter of the two ways. `None` when it cannot reach.
pub fn launch_angle(v: f32, g: f32, x: f32, y: f32) -> Option<f32> {
    let x = x.max(0.25);
    if !(v > 0.0) {
        return None;
    }
    if g.abs() < 1.0e-6 {
        return Some(detmath::atan2(y as f64, x as f64) as f32);
    }
    let v2 = v * v;
    let disc = v2 * v2 - g * (g * x * x + 2.0 * y * v2);
    if disc < 0.0 {
        return None;
    }
    Some(detmath::atan(((v2 - disc.sqrt()) / (g * x)) as f64) as f32)
}

/// The first child of `parent` that has a `Weapon`.
fn child_weapon(world: &mut World, parent: u64) -> Option<u64> {
    let mut best: Option<u64> = None;
    let _ = world.for_each_uid::<(&Parent, &Weapon), _>(|_, uid, (p, _)| {
        if p.target.0 == parent && best.is_none_or(|b| uid < b) {
            best = Some(uid);
        }
    });
    best
}

/// Take `amount` off the `HitPoints` of `victim`; true when that brought it to 0 (a thing in its grace period, or dead, takes nothing): the rule of a bullet's.
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

/// The parts that are shot as things of their own pass the damage on to their machine (after the weapons of this tick, before the health of everything is looked at).
pub fn relay(world: &mut World, events: &mut Events) {
    let mut found: Vec<(Entity, u64, Hitbox, HitPoints)> = Vec::new();
    let _ = world.for_each_uid::<(&Hitbox, &HitPoints), _>(|e, uid, (b, h)| found.push((e, uid, *b, *h)));
    if found.is_empty() {
        return;
    }
    found.sort_by_key(|f| f.1);
    for (e, _uid, mut b, mut h) in found {
        let owner = if b.owner.0 != 0 { b.owner.0 } else { world.get::<Parent>(e).map_or(0, |p| p.target.0) };
        let max = finite(h.max, 0.0).max(1.0);
        let lost = (max - finite(h.value, max)).max(0.0);
        if lost <= 0.0 && !h.dead {
            continue;
        }
        h.value = max;
        h.dead = false;
        h.last = max;
        if let Some(w) = world.get_mut::<HitPoints>(e) {
            *w = h;
        }
        if lost > 0.0 && owner != 0 {
            b.taken += lost;
            b.hits = b.hits.wrapping_add(1);
            let at = world.get::<Transform>(e).map(|t| t.pos);
            if hurt(world, owner, lost * finite(b.mult, 1.0).max(0.0)) {
                events.push(("kill".to_string(), owner, at));
            }
        }
        if let Some(w) = world.get_mut::<Hitbox>(e) {
            *w = b;
        }
    }
}

/// Everything about one machine that the legs and the gun need, read once a tick.
struct Body {
    per: Option<Perception>,
    stomping: bool,
    dead: bool,
}

fn body_of(world: &World, e: Entity) -> Body {
    Body {
        per: world.get::<Perception>(e).copied().filter(|p| p.active),
        stomping: world.get::<crate::stomp::Stomp>(e).is_some_and(|s| s.active && s.phase != 0),
        dead: world.get::<HitPoints>(e).is_some_and(|h| h.dead),
    }
}

pub fn update(world: &mut World, dt: f32, events: &mut Events) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    legs(world, dt);
    guns(world, dt, events);
}

fn legs(world: &mut World, dt: f32) {
    let mut found: Vec<(Entity, u64, Pursuit, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&Pursuit, &Transform, &NavAgent), _>(|e, uid, (p, t, _)| found.push((e, uid, *p, *t)));
    if found.is_empty() {
        return;
    }
    found.sort_by_key(|f| f.1);
    for (e, _uid, mut p, t) in found {
        if !p.active {
            continue;
        }
        let b = body_of(world, e);
        let Some(mut na) = world.get::<NavAgent>(e).copied() else { continue };
        if b.dead {
            // a wreck stands still
            if na.speed != 0.0 || na.active {
                na.speed = 0.0;
                na.active = false;
                if let Some(w) = world.get_mut::<NavAgent>(e) {
                    *w = na;
                }
            }
            if let Some(c) = world.get_mut::<CharacterBody>(e) {
                c.walk = [0.0, 0.0];
            }
            p.mode = 0;
            if let Some(w) = world.get_mut::<Pursuit>(e) {
                *w = p;
            }
            continue;
        }
        let pos = t.pos;
        let points = patrol_of(world.strings().get(p.patrol.0));
        let busy = b.stomping;
        let reach = na.arrive.max(0.3);
        // what it wants: a mode, a goal on the ground, and something to look at
        let mut mode = 0u32;
        let mut goal = p.goal;
        let mut look: Option<V3> = None;
        match b.per.map_or(0, |q| q.state) {
            2 => {
                let q = b.per.unwrap_or_default();
                mode = 2;
                goal = q.seen_pos;
                look = Some(q.seen_pos);
            }
            3 => {
                let q = b.per.unwrap_or_default();
                mode = 3;
                goal = q.seen_pos;
            }
            1 => {
                look = b.per.map(|q| q.seen_pos);
            }
            _ => {
                if !points.is_empty() {
                    mode = 1;
                }
            }
        }
        // a patrol: stand at a point for `wait` seconds, then go on to the next
        if mode == 1 {
            let n = points.len() as u32;
            p.patrol_at %= n;
            let at = |k: u32| [points[k as usize][0], pos[1], points[k as usize][1]];
            if !busy && flat(pos, at(p.patrol_at)) <= reach {
                if !p.arrived {
                    p.arrived = true;
                    p.wait_left = finite(p.wait, 0.0).max(0.0);
                }
                p.wait_left = (p.wait_left - dt).max(0.0);
                if p.wait_left <= 0.0 {
                    p.arrived = false;
                    p.patrol_at = (p.patrol_at + 1) % n;
                }
            } else if !p.arrived {
                p.wait_left = 0.0;
            }
            goal = at(p.patrol_at);
        } else {
            p.arrived = false;
            p.wait_left = 0.0;
        }
        let waiting = mode == 1 && p.arrived;
        let at_goal = flat(pos, goal) <= reach;
        let close = mode == 2 && flat(pos, goal) <= finite(p.close, 0.0).max(0.0);
        let moving = matches!(mode, 1 | 2 | 3) && !busy && !waiting && !close && !at_goal;
        // (holding: the walk it was on is given up, not left to look like a walk that makes no progress: the agent would plan again and again, and declare the way blocked)
        if !moving {
            if na.status == 1 {
                na.status = 2;
                na.active = false;
            }
            // (the agent only writes the body's walk while it controls it: a body that was left walking would walk on)
            if let Some(c) = world.get_mut::<CharacterBody>(e) {
                if c.walk != [0.0, 0.0] {
                    c.walk = [0.0, 0.0];
                }
            }
        }
        // the path: when the goal has moved enough, or the agent has stopped short of it
        p.since_plan += dt;
        if moving {
            let drifted = flat(goal, p.goal) > finite(p.moved, 0.0).max(0.0);
            let switched = p.mode != mode;
            let stalled = na.status != 1;
            let wait_for = if na.status == 3 { finite(p.retry, 0.0) } else { finite(p.replan, 0.0) };
            if (switched || drifted || stalled) && p.since_plan >= wait_for {
                na.target = goal;
                na.active = true;
                p.goal = goal;
                p.since_plan = 0.0;
                p.plans = p.plans.wrapping_add(1);
            }
        }
        // the body: turn toward what it looks at, or the way it walks, at the turn rate
        let walk = world.get::<CharacterBody>(e).map_or([0.0, 0.0], |c| c.walk);
        let walking = (walk[0] * walk[0] + walk[1] * walk[1]).sqrt() > 0.05;
        let want = if let Some(l) = look.filter(|l| flat(*l, pos) > 0.1) {
            Some(heading_to(pos, l))
        } else if walking {
            Some(detmath::atan2(-(walk[0] as f64), -(walk[1] as f64)) as f32)
        } else if moving && flat(goal, pos) > 0.1 {
            Some(heading_to(pos, goal))
        } else {
            None
        };
        let yaw = yaw_of(t.rot);
        let mut err = 0.0;
        if let Some(w) = want {
            err = wrap(w - yaw);
            let step = finite(p.turn_rate, 55.0).max(0.0).to_radians() * dt;
            let turn = err.clamp(-step, step);
            if turn != 0.0 {
                if let Some(tr) = world.get_mut::<Transform>(e) {
                    tr.rot = yaw_quat(wrap(yaw + turn));
                }
            }
        }
        // the speed of the legs: the mode's, a quarter of it while the body is still turned away from the way it has to go
        let mut speed = if moving {
            match mode {
                1 => finite(p.calm_speed, 0.0),
                _ => finite(p.alert_speed, 0.0),
            }
        } else {
            0.0
        };
        if moving && want.is_some() && err.abs() > 75.0f32.to_radians() {
            speed *= 0.25;
        }
        if (na.speed - speed).abs() > 1.0e-4 {
            na.speed = speed;
        }
        p.mode = mode;
        if let Some(w) = world.get_mut::<NavAgent>(e) {
            *w = na;
        }
        if let Some(w) = world.get_mut::<Pursuit>(e) {
            *w = p;
        }
    }
}

fn guns(world: &mut World, dt: f32, events: &mut Events) {
    let mut found: Vec<(Entity, u64, Cannon, Transform)> = Vec::new();
    let _ = world.for_each_uid::<(&Cannon, &Transform), _>(|e, uid, (c, t)| found.push((e, uid, *c, *t)));
    if found.is_empty() {
        return;
    }
    found.sort_by_key(|f| f.1);
    for (e, uid, mut c, t) in found {
        if !c.active {
            continue;
        }
        let b = body_of(world, e);
        c.since_shot = (c.since_shot + dt).min(1.0e6);
        c.firing = false;
        c.aimed = false;
        // the gun: a child with a weapon (looked for until found)
        let wuid = if c.weapon.0 != 0 { c.weapon.0 } else { child_weapon(world, uid).unwrap_or(0) };
        let Some(we) = (wuid != 0).then(|| world.entity(wuid)).flatten() else {
            if let Some(w) = world.get_mut::<Cannon>(e) {
                *w = c;
            }
            continue;
        };
        c.weapon = Uid(wuid);
        let Some(w) = world.get::<Weapon>(we).copied() else { continue };
        let wt = world.get::<Transform>(we).copied().unwrap_or(t);
        let ax = axes_of(normalize_q(wt.rot));
        let muzzle = [
            wt.pos[0] + ax[0][0] * w.muzzle[0] + ax[1][0] * w.muzzle[1] + ax[2][0] * w.muzzle[2],
            wt.pos[1] + ax[0][1] * w.muzzle[0] + ax[1][1] * w.muzzle[1] + ax[2][1] * w.muzzle[2],
            wt.pos[2] + ax[0][2] * w.muzzle[0] + ax[1][2] * w.muzzle[1] + ax[2][2] * w.muzzle[2],
        ];
        // a shell that landed this tick: the splash, and the shells in the air are shown to the players
        if w.fired {
            c.shots = c.shots.wrapping_add(1);
            c.since_shot = 0.0;
            if c.net_kind > 0 {
                let mut shells: Vec<Entity> = Vec::new();
                let _ = world.for_each_uid::<(&Bullet,), _>(|se, _, (bl,)| {
                    if bl.weapon.0 == wuid {
                        shells.push(se);
                    }
                });
                for se in shells {
                    if world.get::<NetEntity>(se).is_none() {
                        let _ = world.insert(se, NetEntity { class: 2, kind: c.net_kind, ..Default::default() });
                    }
                }
            }
        }
        if w.hit_dist > 0.0 && c.blast > 0.0 && !b.dead {
            let team = world.get::<Team>(e).map_or(0, |x| x.id);
            let at = [w.hit_point[0] + w.hit_normal[0] * 0.15, w.hit_point[1] + w.hit_normal[1] * 0.15, w.hit_point[2] + w.hit_normal[2] * 0.15];
            // (a character that the shell hit took the shell's own damage and is spared the splash; a wall or a vehicle that it hit takes the splash as well: the shell bursts on it)
            let hit_character = w.hit.0 != 0 && world.entity(w.hit.0).is_some_and(|he| world.get::<CharacterBody>(he).is_some());
            let skip = [uid, if hit_character { w.hit.0 } else { 0 }];
            crate::stomp::area_damage(world, events, at, c.blast_radius, c.blast, c.blast_edge, c.structure, 1, team, &skip, c.blast_radius, c.blast_radius);
            c.blasts = c.blasts.wrapping_add(1);
            events.push(("cannon_blast".to_string(), uid, Some(at)));
        }
        let mut fire = false;
        let target = b.per.filter(|q| q.state == 2 && q.visible && q.target.0 != 0).and_then(|q| world.entity(q.target.0).map(|te| (q, te)));
        if let (Some((q, te)), false) = (target, b.dead) {
            let tt = world.get::<Transform>(te).copied().unwrap_or_default();
            let height = world.get::<CharacterBody>(te).map_or(1.0, |cb| finite(cb.height, 1.8));
            let aim = [tt.pos[0], tt.pos[1] + height * finite(c.aim_height, 0.5).clamp(0.0, 1.0), tt.pos[2]];
            // how it moves: the change of its place since the last tick, smoothed
            if c.tracked.0 == q.target.0 {
                let k = 0.25;
                for i in 0..3 {
                    let v = (aim[i] - c.track[i]) / dt;
                    c.vel[i] += (finite(v, 0.0) - c.vel[i]) * k;
                }
            } else {
                c.vel = [0.0; 3];
            }
            c.tracked = q.target;
            c.track = aim;
            // the shell's flight to where the target will be
            let speed = finite(w.muzzle_speed, 400.0).max(1.0);
            let g = finite(w.gravity, 9.81);
            let mut lead = aim;
            let mut pitch = None;
            for _ in 0..2 {
                let x = flat(lead, muzzle).max(0.25);
                let y = lead[1] - muzzle[1];
                pitch = launch_angle(speed, g, x, y);
                let flight = pitch.map_or(0.0, |a| x / (speed * detmath::cos(a as f64) as f32).max(1.0));
                let k = finite(c.lead, 0.0).clamp(0.0, 1.0) * flight;
                lead = [aim[0] + c.vel[0] * k, aim[1], aim[2] + c.vel[2] * k];
            }
            let (lo, hi) = (finite(c.pitch_min, -30.0), finite(c.pitch_max, 35.0));
            let ideal = pitch.map(f32::to_degrees);
            let want = ideal.unwrap_or(hi).clamp(lo.min(hi), hi.max(lo));
            // the barrel's turn on the body: the heading from the muzzle to the aim, as far from the way the body faces as the arc lets it
            let arc = finite(c.arc, 25.0).clamp(0.0, 180.0);
            let rel = wrap(heading_to(muzzle, lead) - yaw_of(t.rot)).to_degrees();
            let want_yaw = rel.clamp(-arc, arc);
            let step = finite(c.slew, 70.0).max(0.0) * dt;
            c.pitch += (want - c.pitch).clamp(-step, step);
            c.yaw += (want_yaw - c.yaw).clamp(-step, step);
            let h = flat(lead, t.pos);
            let reach = rel.abs() <= arc && ideal.is_some_and(|a| a >= lo.min(hi) && a <= hi.max(lo));
            c.aimed = reach && (want - c.pitch).abs() <= 0.6 && (want_yaw - c.yaw).abs() <= 0.6;
            fire = c.active && !b.stomping && c.aimed && h >= finite(c.min_range, 0.0) && h <= finite(c.range, 80.0);
        } else {
            c.tracked = Uid(0);
        }
        // the barrel's turn and pitch (the gun is a child: its `LocalTransform` is its turn on the body)
        if let Some(l) = world.get_mut::<LocalTransform>(we) {
            let (hp, hy) = (c.pitch.to_radians() as f64 * 0.5, c.yaw.to_radians() as f64 * 0.5);
            let pitch_q = [detmath::sin(hp) as f32, 0.0, 0.0, detmath::cos(hp) as f32];
            let yaw_q = [0.0, detmath::sin(hy) as f32, 0.0, detmath::cos(hy) as f32];
            l.rot = quat_mul(yaw_q, pitch_q);
        }
        if let Some(wm) = world.get_mut::<Weapon>(we) {
            wm.fire = fire;
        }
        c.firing = fire;
        if let Some(wc) = world.get_mut::<Cannon>(e) {
            *wc = c;
        }
    }
}
