//! The Core learns (docs/WALKER.md): what the machines do against a player grows with what that player has done, and fades slowly. Every player with a `Heat` carries an `alert` meter beside the heat: it takes a share of every bit of
//! heat the player gains (from the director's own sources and from everything else that adds to `Heat.value`: a helicopter's noise, a hacking trace, a script), a gain for each machine destroyed near the player and for the loud events a game
//! names (explosions), and falls by `decay` a second. The meter is cut into **tiers** by thresholds (with a margin for going down, so that the tier does not flicker): tier 0 calm, 1 the machines are looking, 2 and 3 they come in greater
//! numbers and sooner (the director's waves for that player are scaled by the tier's row), and at the tier named `walker_tier` the Core sends a **Walker** (a heavy machine, the prefab `walker`) to the place where the player was last known, at most
//! `walker_max` alive at a time on the whole server and one at a time for a player, with `walker_cooldown` seconds between two sent for the same player. A Walker that has been idle for `walker_cleanup` seconds goes away.
//!
//! The tier of each player is `Heat.tier` (and `NetPlayer.tier` on that player's client: the interface shows it), the highest of them and the highest alert are the resources `ai.tier` and `ai.alert` (a lighthouse that sweeps faster, a siren, a
//! sky that darkens answer them in the game's own systems), and the number of Walkers alive is `ai.walkers`.
//!
//! Nothing here is random (a Walker's place on its ring comes from a hash of the tick and the player) and nothing scans the players more than the director already does: the director passes its `Heat` list through [`track`] in its own loop.
use crate::components::{Heat, HitPoints, NetPlayer, Perception, SpawnedBy, Transform};
use crate::layers::Filter;
use crate::meshcol::MeshStore;
use crate::rng::hash_rand;
use crate::stomp::Stomp;
use crate::terrain::Terrains;
use crate::vehicle_ray::RayWorld;
use lunaris_ecs::{component, Entity, Sym, World};
use lunaris_json::Json;
use std::cell::RefCell;
use std::rc::Rc;

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

const SEED: u64 = 0xC02E_1EA2_9E57_0001;
/// The most tiers.
pub const MAX_TIERS: usize = 7;
/// How often (in ticks) the Walkers are counted, cleaned up and sent.
const EVERY: u64 = 6;

/// What a game gets if it leaves `Escalation.tiers` empty.
pub const DEFAULT_TIERS: &str = "tier 1 at=2\ntier 2 at=5 hunters=1.5 cooldown=0.7 alive=2 per_heat=1.5\ntier 3 at=9 hunters=2 cooldown=0.5 alive=3 per_heat=2\n";

component! {
    /// The Core's learning (docs/WALKER.md), on one entity of the scene (the lowest uid that has one counts). **The meter**: every player with a `Heat` has an `alert` that rises by `heat_gain` times every bit of heat the player gains, by
    /// `kill_gain` for each machine destroyed within `share` metres (`heavy_gain` for a machine with a `Stomp`: a Walker), and by the gain of each loud event named in `noise` (text: one `EVENT GAIN` a line, such as `explosion 1.5`; the
    /// event needs a place, and counts within `share` metres of it), the nearer the more (in a straight line to 0 at `share`), and falls by `decay` a second; `max` is the most it holds. **The tiers**: `tiers` is text, one tier a line,
    /// `tier N at=ALERT hunters=MULT cooldown=MULT alive=ADD per_heat=MULT` (N from 1: the tier a player is in is the highest whose `at` the alert has reached, and it drops when the alert is `hyst` below it): the director's waves for a
    /// player in that tier are `hunters` times as many (after `per_heat` times its usual growth with the heat), the wait after one is `cooldown` times as long and up to `alive` more of its hunters may be alive (empty text: `DEFAULT_TIERS`).
    /// **The Walker**: at tier `walker_tier` (0 never) a player who has no Walker within `walker_near` metres and has not been sent one for `walker_cooldown` seconds is sent the prefab `walker`, on the ground `walker_min_dist` to
    /// `walker_max_dist` metres away at a place where `clearance` metres round it are free of walls and the ground is level, looking at the player, already searching the place where the player is; at most `walker_max` of them are alive
    /// (counted over the whole world: the cap of the server) and one that is calm for `walker_cleanup` seconds is removed. A player's own tier, alert and the wait before the next Walker are in its `Heat` (`tier`, `alert`, `walker_cd`).
    /// Outputs: `tier` and `alert` (the highest of any player), `walkers` (alive now), `sent` (all the Walkers sent), `kills` (machines destroyed that counted). Events: `walker` (the new Walker, at its place), `tier` (a player, at its place: its tier changed).
    pub struct Escalation {
        pub active: bool = true,
        pub tiers: Sym = Sym(0),
        pub heat_gain: f32 = 0.5,
        pub kill_gain: f32 = 0.8,
        pub heavy_gain: f32 = 3.0,
        pub share: f32 = 120.0,
        pub noise: Sym = Sym(0),
        pub decay: f32 = 0.004,
        pub hyst: f32 = 0.5,
        pub max: f32 = 20.0,
        pub walker: Sym = Sym(0),
        pub walker_tier: u32 = 3,
        pub walker_max: u32 = 2,
        pub walker_cooldown: f32 = 240.0,
        pub walker_min_dist: f32 = 60.0,
        pub walker_max_dist: f32 = 100.0,
        pub walker_near: f32 = 150.0,
        pub walker_cleanup: f32 = 120.0,
        pub walker_height: f32 = 0.3,
        pub clearance: f32 = 2.0,
        pub tier: u32 = 0,
        pub alert: f32 = 0.0,
        pub walkers: u32 = 0,
        pub sent: u32 = 0,
        pub kills: u32 = 0,
    }
}

fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

/// One tier of the text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    /// The alert at which the tier starts.
    pub at: f32,
    /// A wave of the director is this many times as big.
    pub hunters: f32,
    /// The wait after a wave is this many times as long.
    pub cooldown: f32,
    /// This many more hunters may be alive.
    pub alive: u32,
    /// The director's growth of a wave with the heat is this many times as fast.
    pub per_heat: f32,
}

/// Read the text of the tiers: `tier N at=ALERT hunters=MULT cooldown=MULT alive=ADD per_heat=MULT`. The tiers must be numbered from 1 with no gap, and their thresholds must rise. Lines that are not understood are skipped and listed.
pub fn parse_tiers(text: &str) -> (Vec<Row>, Vec<String>) {
    let mut rows: Vec<Row> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        if words.next() != Some("tier") {
            problems.push(format!("line {}: a line is `tier N at=ALERT hunters=MULT cooldown=MULT alive=ADD per_heat=MULT`, not `{line}`", n + 1));
            continue;
        }
        let Some(number) = words.next().and_then(|w| w.parse::<usize>().ok()).filter(|k| (1..=MAX_TIERS).contains(k)) else {
            problems.push(format!("line {}: `tier` needs a number from 1 to {MAX_TIERS}", n + 1));
            continue;
        };
        if number != rows.len() + 1 {
            problems.push(format!("line {}: the tiers are numbered from 1 with no gap: this one should be {}", n + 1, rows.len() + 1));
            continue;
        }
        let mut r = Row { at: f32::NAN, hunters: 1.0, cooldown: 1.0, alive: 0, per_heat: 1.0 };
        for w in words {
            let Some((k, v)) = w.split_once('=') else {
                problems.push(format!("line {}: `{w}` is not key=value", n + 1));
                continue;
            };
            let Ok(x) = v.parse::<f32>() else {
                problems.push(format!("line {}: {k} needs a number, not `{v}`", n + 1));
                continue;
            };
            match k {
                "at" => r.at = x,
                "hunters" => r.hunters = x.max(0.0),
                "cooldown" => r.cooldown = x.max(0.0),
                "alive" => r.alive = x.max(0.0) as u32,
                "per_heat" => r.per_heat = x.max(0.0),
                _ => problems.push(format!("line {}: unknown key `{k}` (at, hunters, cooldown, alive, per_heat)", n + 1)),
            }
        }
        if !r.at.is_finite() {
            problems.push(format!("line {}: a tier needs `at=ALERT`", n + 1));
            continue;
        }
        if rows.last().is_some_and(|p| r.at <= p.at) {
            problems.push(format!("line {}: the threshold must be above the one before it", n + 1));
            continue;
        }
        rows.push(r);
    }
    (rows, problems)
}

/// Read the loud events: `EVENT GAIN` a line.
pub fn parse_noises(text: &str) -> (Vec<(String, f32)>, Vec<String>) {
    let mut out: Vec<(String, f32)> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut w = line.split_whitespace();
        match (w.next(), w.next().and_then(|g| g.parse::<f32>().ok())) {
            (Some(name), Some(g)) if g.is_finite() && g > 0.0 => {
                out.retain(|(k, _)| k != name);
                out.push((name.to_string(), g));
            }
            _ => problems.push(format!("line {}: a line is `EVENT GAIN` (a name and a number above 0), not `{line}`", n + 1)),
        }
    }
    (out, problems)
}

thread_local! {
    /// The texts last read and what they parsed to (the director asks on every tick).
    static TIERS: RefCell<Option<(String, Rc<Vec<Row>>)>> = const { RefCell::new(None) };
    static NOISES: RefCell<Option<(String, Rc<Vec<(String, f32)>>)>> = const { RefCell::new(None) };
}

fn tiers_of(text: &str) -> Rc<Vec<Row>> {
    TIERS.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((k, p)) = c.as_ref() {
            if k == text {
                return p.clone();
            }
        }
        let p = Rc::new(parse_tiers(if text.trim().is_empty() { DEFAULT_TIERS } else { text }).0);
        *c = Some((text.to_string(), p.clone()));
        p
    })
}

fn noises_of(text: &str) -> Rc<Vec<(String, f32)>> {
    NOISES.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((k, p)) = c.as_ref() {
            if k == text {
                return p.clone();
            }
        }
        let p = Rc::new(parse_noises(text).0);
        *c = Some((text.to_string(), p.clone()));
        p
    })
}

/// The Core's learning as the director needs it: the component, its tiers and its loud events.
pub struct Config {
    pub e: Entity,
    pub uid: u64,
    pub c: Escalation,
    pub rows: Rc<Vec<Row>>,
    pub noises: Rc<Vec<(String, f32)>>,
}

/// The `Escalation` of the scene: the active one with the lowest uid, if any.
pub fn config(world: &mut World) -> Option<Config> {
    let mut best: Option<(u64, Entity, Escalation)> = None;
    let _ = world.for_each_uid::<(&Escalation,), _>(|e, uid, (c,)| {
        if c.active && best.as_ref().is_none_or(|b| uid < b.0) {
            best = Some((uid, e, *c));
        }
    });
    let (uid, e, c) = best?;
    let rows = tiers_of(world.strings().get(c.tiers.0));
    let noises = noises_of(world.strings().get(c.noise.0));
    Some(Config { e, uid, c, rows, noises })
}

/// The tier an alert is in: the highest threshold it has reached, but one that it is leaving holds until the alert is `hyst` below its threshold.
pub fn tier_for(rows: &[Row], alert: f32, current: u32, hyst: f32) -> u32 {
    let up = rows.iter().rposition(|r| alert >= r.at).map_or(0, |i| i as u32 + 1);
    if up >= current {
        return up;
    }
    let mut t = current.min(rows.len() as u32);
    while t > up && alert < rows[t as usize - 1].at - hyst.max(0.0) {
        t -= 1;
    }
    t.max(up)
}

/// What a tier does to the director's waves: (hunters times, cooldown times, alive more, per_heat times). Tier 0 does nothing.
pub fn effect(cfg: &Config, tier: u32) -> (f32, f32, u32, f32) {
    match tier.checked_sub(1).and_then(|k| cfg.rows.get(k as usize)) {
        Some(r) => (r.hunters, r.cooldown, r.alive, r.per_heat),
        None => (1.0, 1.0, 0, 1.0),
    }
}

/// One player's meter for one tick (the director calls it in its own loop over the `Heat`s): `gained` is the heat the player gained this tick from every source, `extra` what the events of the tick are worth to it.
pub fn track(h: &mut Heat, gained: f32, extra: f32, dt: f32, cfg: &Config) {
    let c = &cfg.c;
    let a = finite(h.alert, 0.0) + finite(gained, 0.0).max(0.0) * finite(c.heat_gain, 0.0).max(0.0) + extra.max(0.0) - finite(c.decay, 0.0).max(0.0) * dt;
    h.alert = a.clamp(0.0, finite(c.max, 20.0).max(0.0));
    h.tier = tier_for(&cfg.rows, h.alert, h.tier, finite(c.hyst, 0.5));
    h.walker_cd = (h.walker_cd - dt).max(0.0);
}

/// What this tick's events are worth to each player (in the order of `players`: their uids and places): a machine destroyed (an entity with a `Perception`) and the loud events the game named (the engine's own events of the tick or
/// what a graph system emitted), to the players within `share` metres of them, more the nearer. Also counts the machines in `Escalation.kills`.
pub fn event_gains(world: &mut World, events: &Events, emitted: &Events, cfg: &Config, players: &[(u64, V3)]) -> Vec<f32> {
    let mut gains = vec![0.0f32; players.len()];
    let share = finite(cfg.c.share, 0.0).max(0.0);
    let mut killed = 0u32;
    // (the engine's own events of this tick, and what the game's graph systems have emitted: an explosion is one of those)
    for (name, uid, pos) in events.iter().chain(emitted.iter()) {
        let Some(at) = *pos else { continue };
        let worth = if name == "kill" {
            let Some(e) = world.entity(*uid) else { continue };
            if world.get::<Perception>(e).is_none() {
                continue;
            }
            killed += 1;
            if world.get::<Stomp>(e).is_some() {
                finite(cfg.c.heavy_gain, 0.0)
            } else {
                finite(cfg.c.kill_gain, 0.0)
            }
        } else if let Some((_, g)) = cfg.noises.iter().find(|(k, _)| k == name) {
            *g
        } else {
            continue;
        };
        if worth <= 0.0 || share <= 0.0 {
            continue;
        }
        for (i, (_, p)) in players.iter().enumerate() {
            let d = ((p[0] - at[0]).powi(2) + (p[2] - at[2]).powi(2)).sqrt();
            if d < share {
                gains[i] += worth * (1.0 - d / share);
            }
        }
    }
    if killed > 0 {
        if let Some(c) = world.get_mut::<Escalation>(cfg.e) {
            c.kills = c.kills.wrapping_add(killed);
        }
    }
    gains
}

/// Whether this world mirrors a server (it is a connected client: its local player's `NetPlayer.connected`). The Core's state is the server's then, told to the clients by `NetHost.sync`; what a client would work out for itself from its own
/// `Heat` (which nothing raises there) would only overwrite what the server said, every tick.
fn replica(world: &mut World) -> bool {
    let mut on = false;
    let _ = world.for_each::<(&NetPlayer,), _>(|_, (p,)| on |= p.connected);
    on
}

/// Tell the game the Core's state: the highest tier and alert of any player, in the component and as the resources `ai.tier`, `ai.alert` (not on a connected client: the server's, told by `NetHost.sync`, stand).
pub fn publish(world: &mut World, cfg: &Config, top_tier: u32, top_alert: f32) {
    if let Some(c) = world.get_mut::<Escalation>(cfg.e) {
        c.tier = top_tier;
        c.alert = top_alert;
    }
    if replica(world) {
        return;
    }
    for (name, v) in [("ai.tier", top_tier as f64), ("ai.alert", ((top_alert * 100.0).round() / 100.0) as f64)] {
        if world.resource(name) != Some(v) {
            let _ = world.set_resource(name, Some(v));
        }
    }
}

/// A place on the ground `min` to `max` metres from `player` where a Walker fits: level ground with nothing over it (no roof, no overhang, not inside a solid mass) and the `clearance` round it free of walls up to 3.5 m high. Tries eight places on the
/// ring, in an order from a hash. (The ground is found by a ray down from 8 m above the player's own level: a flat roof lower than that counts as ground.)
#[allow(clippy::too_many_arguments)]
fn place(world: &mut World, fields: &Terrains, meshes: &MeshStore, player: V3, uid: u64, tick: u64, salt: u64, cfg: &Escalation) -> Option<V3> {
    let (lo, hi) = (finite(cfg.walker_min_dist, 60.0).max(1.0), finite(cfg.walker_max_dist, 100.0).max(finite(cfg.walker_min_dist, 60.0).max(1.0)));
    let reach_r = hi + finite(cfg.clearance, 2.0) + 5.0;
    let reach = ([player[0] - reach_r, player[1] - 60.0, player[2] - reach_r], [player[0] + reach_r, player[1] + 80.0, player[2] + reach_r]);
    let rw = RayWorld::gather(world, fields, meshes, Some(reach));
    let who = Filter::DEFAULT;
    let clear = finite(cfg.clearance, 2.0).max(0.5);
    for k in 0..8u64 {
        let angle = std::f64::consts::TAU * hash_rand(SEED, tick, uid, salt * 32 + k * 2) as f64;
        let dist = lo + (hi - lo) * hash_rand(SEED, tick, uid, salt * 32 + k * 2 + 1);
        let (x, z) = (player[0] + dist * crate::detmath::cos(angle) as f32, player[2] + dist * crate::detmath::sin(angle) as f32);
        // the ground: a ray down from a little above the player's own level
        let top = player[1] + 8.0;
        let Some(g) = rw.cast([x, top, z], [0.0, -1.0, 0.0], 40.0, who, 0) else { continue };
        if g.normal[1] < 0.85 {
            continue;
        }
        let gy = top - g.t;
        // nothing over it: a ray down from 60 m above finds the ground itself first (a roof, an overhang or the inside of a solid mass would be found before it)
        let mut free = rw.cast([x, gy + 60.0, z], [0.0, -1.0, 0.0], 61.0, who, 0).is_some_and(|h| h.t >= 59.5);
        for i in 0..8 {
            let a = std::f64::consts::TAU * i as f64 / 8.0;
            let d = [crate::detmath::cos(a) as f32, 0.0, crate::detmath::sin(a) as f32];
            for h in [1.0f32, 3.5] {
                free &= rw.cast([x, gy + h, z], d, clear, who, 0).is_none();
            }
        }
        if free {
            return Some([x, gy + finite(cfg.walker_height, 0.3).max(0.0), z]);
        }
    }
    None
}

/// Count the Walkers, remove the ones that have been calm too long, and send one to each player whose turn it is (every few ticks).
#[allow(clippy::too_many_arguments)]
pub fn walkers(world: &mut World, scripts: &mut crate::ScriptSet, tick: u64, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    if tick % EVERY != 0 {
        return;
    }
    let Some(cfg) = config(world) else { return };
    // (a connected client sends no Walkers and counts none: the server's `ai.walkers` stands)
    if replica(world) {
        return;
    }
    let name = world.strings().get(cfg.c.walker.0).to_string();
    // the Walkers that are alive now: where they are, and which have been calm for too long
    let mut alive: Vec<V3> = Vec::new();
    let mut doomed: Vec<u64> = Vec::new();
    let mut mine: Vec<(Entity, u64, V3)> = Vec::new();
    let _ = world.for_each_uid::<(&SpawnedBy, &Stomp, &Transform), _>(|e, uid, (b, _, t)| {
        if b.spawner.0 == cfg.uid {
            mine.push((e, uid, t.pos));
        }
    });
    for (e, uid, pos) in mine {
        // (a Walker that has been destroyed is not counted: its wreck stays a while for the picture)
        let calm = world.get::<Perception>(e).is_some_and(|p| p.active && p.state == 0 && p.since_seen >= finite(cfg.c.walker_cleanup, 120.0));
        let dead = world.get::<HitPoints>(e).is_some_and(|h| h.dead);
        if calm && !dead {
            doomed.push(uid);
        } else if !dead {
            alive.push(pos);
        }
    }
    if !doomed.is_empty() {
        crate::gameplay::despawn_trees(world, doomed);
    }
    let mut count = alive.len() as u32;
    // the players whose turn it is
    let tier = finite(cfg.c.walker_tier as f32, 0.0) as u32;
    let mut due: Vec<(u64, V3)> = Vec::new();
    if tier > 0 && !name.is_empty() {
        let mut ready: Vec<(Entity, u64, V3)> = Vec::new();
        let _ = world.for_each_uid::<(&Heat, &Transform), _>(|e, uid, (h, t)| {
            if h.tier >= tier && h.walker_cd <= 0.0 {
                ready.push((e, uid, t.pos));
            }
        });
        for (e, uid, pos) in ready {
            if !world.get::<HitPoints>(e).is_some_and(|hp| hp.dead) {
                due.push((uid, pos));
            }
        }
    }
    due.sort_by_key(|d| d.0);
    let mut sent = 0u32;
    if !due.is_empty() && scripts.prefabs.contains_key(&name) {
        for (puid, ppos) in due {
            if count >= cfg.c.walker_max {
                break;
            }
            let near = finite(cfg.c.walker_near, 150.0);
            if alive.iter().any(|w| ((w[0] - ppos[0]).powi(2) + (w[2] - ppos[2]).powi(2)).sqrt() < near) {
                continue;
            }
            let salt = cfg.c.sent as u64 + sent as u64;
            let Some(at) = place(world, fields, meshes, ppos, puid, tick, salt, &cfg.c) else {
                // (no room on the ring now: try again in a few seconds)
                if let Some(pe) = world.entity(puid) {
                    if let Some(h) = world.get_mut::<Heat>(pe) {
                        h.walker_cd = 4.0;
                    }
                }
                continue;
            };
            let face = [ppos[0] - at[0], 0.0, ppos[2] - at[2]];
            let rot = crate::perception::look_rot(face, false);
            let Some(wuid) = crate::building::spawn_prefab_rot(world, scripts, &name, at, rot, &[("SpawnedBy", Json::obj([("spawner", Json::from(cfg.uid as i64))]))]) else { continue };
            // it starts out knowing where the player is
            if let Some(we) = world.entity(wuid) {
                if let Some(p) = world.get_mut::<Perception>(we) {
                    p.awareness = 1.0;
                    p.state = 3;
                    p.target = lunaris_ecs::Uid(puid);
                    p.seen_pos = ppos;
                    p.since_seen = 0.0;
                }
            }
            if let Some(pe) = world.entity(puid) {
                if let Some(h) = world.get_mut::<Heat>(pe) {
                    h.walker_cd = finite(cfg.c.walker_cooldown, 240.0).max(0.0);
                }
            }
            alive.push(at);
            count += 1;
            sent += 1;
            events.push(("walker".to_string(), wuid, Some(at)));
        }
    }
    if let Some(c) = world.get_mut::<Escalation>(cfg.e) {
        c.walkers = count;
        c.sent = c.sent.wrapping_add(sent);
    }
    if world.resource("ai.walkers") != Some(count as f64) {
        let _ = world.set_resource("ai.walkers", Some(count as f64));
    }
}
