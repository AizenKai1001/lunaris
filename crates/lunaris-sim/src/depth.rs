//! Base depth (docs/DEPTH.md): what a base needs after it can be raided (docs/RAIDING.md): to be **repaired**, and to **raise the alarm** when it is hit. The third tier (metal) needs nothing new: it is a `piece` line with `upgrade=` in the building rules.
//!
//! * `Repairer` (on the avatar, the player's side, like `Upgrader`): the game sets `want` for one tick and the engine looks at the building piece aimed at. If the piece's rules line has a `repair_cost=ITEM:COUNT,...` (what repairing it from nothing to full takes), the piece is the player's own (or a party
//!   mate's), the tool `item` is in the bag (a hammer: it is not used up), the piece took no damage for `delay` seconds (`HitPoints.hurt`: a hit of a blast, a bullet or anything else, not the slow decay) and the bag holds the materials, it is healed by `step` of its full hit points (or what is missing)
//!   for that share of the cost, **rounded up** (so a small repair costs at least one of each material). Everything is the server's: the player says only that it pressed the button and where it looks. A pause (`pause`) follows every request that is heard.
//! * `RaidAlarm` (on a cupboard, an entity with a `BuildUpkeep` and a `Transform`): when a blast damages a piece within the cupboard's `BuildUpkeep.radius` (the event `build_damaged` that `blast.rs` tells, about the piece, at the place that was hurt) the cupboard rings: the event `raid_alarm`, about each player who owns
//!   the cupboard (`BuildUpkeep.owner`, or the owner of the piece the cupboard is, when that is 0) or is in the same party as the owner, at the place of the damage. At most once every `throttle` seconds a cupboard. The game forwards it to the player alone (`NetHost.forward` `raid_alarm own`): strangers are not told, and the place
//!   that the alarm names is where the damage was, not where the cupboard is.
//!
//! * `Striker` (on the avatar, like `Repairer`): a blow with a tool in the hand (a crowbar), the cheap way into a base that needs no charges: the game sets `want` for one tick and the engine looks at the building piece aimed at (3 m): if the tool `item` is in the bag (it is not used up) the piece
//!   loses `damage` hit points times its rules line's `melee=` share (wood 1, stone a quarter, metal a tenth: a satchel does 336 to a piece, a blow 34 to wood: a tenth of it), a pause follows. The blow is told as `build_damaged` too, so the cupboard's alarm rings for it as for a blast.
//!
//! Deterministic (uid order, ticks, nothing random) and cheap: nothing is done unless somebody asks (`Repairer.want`) or a piece was damaged by a blast (a scan of the few cupboards).
use crate::building::{aimed_piece, rules_of};
use crate::components::{BuildPiece, BuildUpkeep, HitPoints, NetAvatar, Parent, Transform};
use crate::inventory::{table_of, Bag, Inventory};
use crate::meshcol::MeshStore;
use crate::terrain::Terrains;
use lunaris_ecs::{component, Entity, Sym, Uid, World};

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

/// The most players one alarm tells (a party and its owner).
const MAX_TOLD: usize = 16;
/// The most damaged pieces of one tick that are looked at (a blast tells of at most 16, and at most 8 blasts go off in a tick).
const MAX_DAMAGED: usize = 128;
/// A piece with less than this many hit points missing is not worth a repair (decay takes a little off a piece every second).
const MIN_MISSING: f32 = 1.0;

component! {
    /// The player's side of repairing a base (docs/DEPTH.md), on the avatar: the game sets `want` for one tick and the engine looks at the piece aimed at (`reach` metres along the aim entity, `aim`, or the `NetAvatar`'s `head`, or this entity; a wall of the world in front hides it). It is repaired by `step` of its
    /// full hit points (or what is missing) if its rules line has `repair_cost=`, the piece is the player's (`owner` is the player's number, `NetAvatar.slot` + 1 when 0: the number of the piece's `BuildPiece.owner`; a party mate's piece counts), the tool `item` (an item key, empty for none) is in the bag
    /// (`inventory`, 0: this entity's or its parent's; it is not used up), the piece took no damage for `delay` seconds and the bag holds that share of the cost (rounded up, so at least one of each material); otherwise nothing is taken. The answer is the event `build_repaired` (the piece) and `build_repair_ok`
    /// (the player's aim entity), or `build_repair_refused` and `build_repair_refused_WHY` with `denied` for a tick: `dead`, `no_aim`, `no_repair` (a piece with no `repair_cost`), `not_yours`, `no_tool`, `full`, `recently_hit`, `no_materials`. `ready` is the tick of the next request that is heard (`pause` seconds
    /// after the last), `repaired` counts the repairs it has made.
    pub struct Repairer {
        pub want: bool = false,
        pub reach: f32 = 5.0,
        pub owner: u32 = 0,
        pub aim: Uid = Uid(0),
        pub inventory: Uid = Uid(0),
        pub item: Sym = Sym(0),
        pub step: f32 = 0.25,
        pub delay: f32 = 10.0,
        pub pause: f32 = 0.8,
        pub ready: u32 = 0,
        pub repaired: u32 = 0,
        pub denied: Sym = Sym(0),
    }
}

component! {
    /// A cupboard that raises the alarm (docs/DEPTH.md), on the entity with the `BuildUpkeep`: when a blast damages a piece within `radius` metres (0: the `BuildUpkeep`'s radius) the event `raid_alarm` is told, about each player who owns the cupboard or is in its owner's party, at the place of the damage; a cupboard
    /// rings at most once every `throttle` seconds. The engine fills `BuildUpkeep.owner` from the cupboard's own `BuildPiece.owner` while it is 0. `last` is the tick of the last alarm plus one (0: never), `rung` counts the alarms, `told` is how many players the last one reached.
    pub struct RaidAlarm {
        pub radius: f32 = 0.0,
        pub throttle: f32 = 30.0,
        pub last: u32 = 0,
        pub rung: u32 = 0,
        pub told: u32 = 0,
    }
}

component! {
    /// The player's side of hitting a base with a tool (docs/DEPTH.md), on the avatar: the game sets `want` for one tick and the engine looks at the piece aimed at (`reach` metres along the aim entity, `aim`, or the `NetAvatar`'s `head`, or this entity; a wall of the world in front hides it). If the tool `item`
    /// (an item key, empty for none) is in the bag (`inventory`, 0: this entity's or its parent's; it is not used up) the piece loses `damage` times its `melee` share (a rules key, default 1: `melee=0` cannot be hurt by hand) of its `HitPoints`; a piece taken to 0 is destroyed by the building system in the next tick. The answer is the events
    /// `build_struck` (the piece, at its place), `build_damaged` (the same, for what watches the base: the raid alarm) and `build_strike_ok` (the player's aim entity), or `build_strike_refused` and `build_strike_refused_WHY` with `denied` for a tick: `dead`, `no_aim`, `no_tool`, `immune`. `pause` seconds after a blow
    /// the next one is not heard (`ready` is its tick); `struck` counts the blows.
    pub struct Striker {
        pub want: bool = false,
        pub reach: f32 = 3.0,
        pub aim: Uid = Uid(0),
        pub inventory: Uid = Uid(0),
        pub item: Sym = Sym(0),
        pub damage: f32 = 30.0,
        pub pause: f32 = 0.7,
        pub ready: u32 = 0,
        pub struck: u32 = 0,
        pub denied: Sym = Sym(0),
    }
}

fn ticks(secs: f32, dt: f32) -> u32 {
    if secs.is_finite() && secs > 0.0 {
        ((secs / dt).ceil() as u32).max(1)
    } else {
        0
    }
}

fn dist(a: V3, b: V3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Whose bag a repairer uses: the one it names, its own, or its parent's.
fn bag_of(world: &World, e: Entity, named: Uid) -> Option<Entity> {
    let has = |b: Entity| world.get::<Inventory>(b).is_some();
    if named.0 != 0 {
        return world.entity(named.0).filter(|b| has(*b));
    }
    if has(e) {
        return Some(e);
    }
    world.get::<Parent>(e).and_then(|p| world.entity(p.target.0)).filter(|b| has(*b))
}

/// The uid of the avatar whose number (`NetAvatar.slot` + 1) is `owner`, if it is in the game.
fn avatar_of(world: &mut World, owner: u32) -> Option<u64> {
    let mut found: Option<u64> = None;
    let _ = world.for_each_uid::<(&NetAvatar,), _>(|_, uid, (n,)| {
        if n.slot + 1 == owner && found.is_none_or(|f| uid < f) {
            found = Some(uid);
        }
    });
    found
}

/// Is the piece owned by `piece_owner` the player's own piece (the player's number is `owner`, its avatar's uid `me`)? Its own, or a party mate's (the owner's avatar is in the game and in the same party).
fn is_mine(world: &mut World, owner: u32, me: u64, piece_owner: u32) -> bool {
    if piece_owner == 0 || owner == 0 {
        return false;
    }
    if piece_owner == owner {
        return true;
    }
    avatar_of(world, piece_owner).is_some_and(|theirs| crate::party::same_party(world, me, theirs))
}

/// What one repair did: the reason it was refused, or the piece and its place.
#[allow(clippy::too_many_arguments)]
fn repair_one(world: &mut World, rules: &crate::building::Rules, fields: &Terrains, meshes: &MeshStore, e: Entity, uid: u64, r: &Repairer) -> Result<(u64, V3), &'static str> {
    let bag_e = bag_of(world, e, r.inventory);
    let body = bag_e.unwrap_or(e);
    if world.get::<HitPoints>(body).is_some_and(|h| h.dead || h.value <= 0.0) {
        return Err("dead");
    }
    let aim_e = if r.aim.0 != 0 { world.entity(r.aim.0) } else { None }.or_else(|| world.get::<NetAvatar>(e).filter(|n| n.head.0 != 0).and_then(|n| world.entity(n.head.0))).unwrap_or(e);
    let (piece_uid, piece_e, piece_pos, kind_i) = aimed_piece(world, rules, fields, meshes, aim_e, uid, r.reach)?;
    let kind = &rules.kinds[kind_i];
    if kind.repair_cost.is_empty() {
        return Err("no_repair");
    }
    let owner = if r.owner != 0 { r.owner } else { world.get::<NetAvatar>(e).map_or(0, |n| n.slot + 1) };
    let piece_owner = world.get::<BuildPiece>(piece_e).map_or(0, |p| p.owner);
    if !is_mine(world, owner, uid, piece_owner) {
        return Err("not_yours");
    }
    // the tool: in the bag, and kept
    let Some((table, _)) = table_of(world) else { return Err("no_tool") };
    let Some(bag_e) = bag_e else { return Err("no_tool") };
    let Some(mut inv) = world.get::<Inventory>(bag_e).copied() else { return Err("no_tool") };
    let mut bag = Bag::of(&inv);
    let tool = world.strings().get(r.item.0).to_string();
    if !tool.is_empty() {
        let id = table.id_of(&tool);
        if id == 0 || bag.total(id) == 0 {
            return Err("no_tool");
        }
    }
    // the piece: damaged, and not hit lately (decay is not a hit: the building system keeps `last` level)
    let Some(h) = world.get::<HitPoints>(piece_e).copied() else { return Err("no_repair") };
    if h.dead || h.value <= 0.0 {
        return Err("no_aim");
    }
    let max = h.max.max(1.0);
    let missing = max - h.value;
    if !(missing >= MIN_MISSING.min(max)) {
        return Err("full");
    }
    if h.hurt < r.delay.max(0.0) {
        return Err("recently_hit");
    }
    // what it costs: that share of the full cost, rounded up (the whole of it is in the bag, or none is taken)
    let heal = missing.min(max * r.step.clamp(0.01, 1.0));
    let share = (heal / max) as f64;
    let ids: Vec<(u32, u32)> = kind.repair_cost.iter().map(|(name, n)| (table.id_of(name), ((*n as f64 * share) - 1.0e-6).ceil().max(1.0) as u32)).collect();
    if ids.iter().any(|(id, n)| *id == 0 || bag.total(*id) < *n) {
        return Err("no_materials");
    }
    for (id, n) in ids {
        bag.remove(id, n);
    }
    bag.store(&mut inv);
    if let Some(w) = world.get_mut::<Inventory>(bag_e) {
        *w = inv;
    }
    if let Some(w) = world.get_mut::<HitPoints>(piece_e) {
        w.value = (w.value + heal).min(w.max);
    }
    Ok((piece_uid, piece_pos))
}

/// The players' requests to repair a piece (`Repairer`): cheap when nobody asks.
fn run_repairs(world: &mut World, tick: u64, dt: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    let mut asks: Vec<(Entity, u64, Repairer)> = Vec::new();
    let _ = world.for_each_uid::<(&Repairer,), _>(|e, uid, (r,)| {
        if r.want || r.denied.0 != 0 {
            asks.push((e, uid, *r));
        }
    });
    if asks.is_empty() {
        return;
    }
    asks.sort_by_key(|a| a.1);
    let Some((_, rules)) = rules_of(world) else { return };
    for (e, uid, mut r) in asks {
        let before = r;
        r.denied = Sym(0);
        let now = tick as u32;
        if r.want && now >= r.ready {
            let aim = if r.aim.0 != 0 { world.entity(r.aim.0) } else { None }.or_else(|| world.get::<NetAvatar>(e).filter(|n| n.head.0 != 0).and_then(|n| world.entity(n.head.0))).unwrap_or(e);
            let aim_uid = world.uid_of(aim).unwrap_or(uid);
            match repair_one(world, &rules, fields, meshes, e, uid, &r) {
                Ok((piece, at)) => {
                    r.repaired = r.repaired.wrapping_add(1);
                    events.push(("build_repaired".to_string(), piece, Some(at)));
                    events.push(("build_repair_ok".to_string(), aim_uid, Some(at)));
                }
                Err(why) => {
                    r.denied = Sym(world.strings_mut().intern(why));
                    let at = world.get::<Transform>(e).map(|t| t.pos);
                    events.push(("build_repair_refused".to_string(), aim_uid, at));
                    events.push((format!("build_repair_refused_{why}"), aim_uid, at));
                }
            }
            // (a refused request pauses the player a little too: a client that asks in a loop is not answered in a loop)
            r.ready = now.saturating_add(ticks(r.pause, dt).max(ticks(0.25, dt)));
        }
        r.want = false;
        if r != before {
            if let Some(w) = world.get_mut::<Repairer>(e) {
                *w = r;
            }
        }
    }
}

/// What one blow did: the reason it was refused, or the piece and its place.
fn strike_one(world: &mut World, rules: &crate::building::Rules, fields: &Terrains, meshes: &MeshStore, e: Entity, uid: u64, r: &Striker) -> Result<(u64, V3), &'static str> {
    let bag_e = bag_of(world, e, r.inventory);
    let body = bag_e.unwrap_or(e);
    if world.get::<HitPoints>(body).is_some_and(|h| h.dead || h.value <= 0.0) {
        return Err("dead");
    }
    let aim_e = if r.aim.0 != 0 { world.entity(r.aim.0) } else { None }.or_else(|| world.get::<NetAvatar>(e).filter(|n| n.head.0 != 0).and_then(|n| world.entity(n.head.0))).unwrap_or(e);
    let (piece_uid, piece_e, piece_pos, kind_i) = aimed_piece(world, rules, fields, meshes, aim_e, uid, r.reach)?;
    let tool = world.strings().get(r.item.0).to_string();
    if !tool.is_empty() {
        let Some((table, _)) = table_of(world) else { return Err("no_tool") };
        let Some(inv) = bag_e.and_then(|b| world.get::<Inventory>(b).copied()) else { return Err("no_tool") };
        let id = table.id_of(&tool);
        if id == 0 || Bag::of(&inv).total(id) == 0 {
            return Err("no_tool");
        }
    }
    let share = rules.kinds[kind_i].melee;
    if !(share > 0.0) {
        return Err("immune");
    }
    let dmg = if r.damage.is_finite() { r.damage.max(0.0) } else { 0.0 } * share;
    let Some(h) = world.get_mut::<HitPoints>(piece_e) else { return Err("no_aim") };
    if h.dead || h.value <= 0.0 {
        return Err("no_aim");
    }
    h.value = (h.value - dmg).max(0.0);
    Ok((piece_uid, piece_pos))
}

/// The players' blows (`Striker`): cheap when nobody asks.
fn run_strikes(world: &mut World, tick: u64, dt: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    let mut asks: Vec<(Entity, u64, Striker)> = Vec::new();
    let _ = world.for_each_uid::<(&Striker,), _>(|e, uid, (r,)| {
        if r.want || r.denied.0 != 0 {
            asks.push((e, uid, *r));
        }
    });
    if asks.is_empty() {
        return;
    }
    asks.sort_by_key(|a| a.1);
    let Some((_, rules)) = rules_of(world) else { return };
    for (e, uid, mut r) in asks {
        let before = r;
        r.denied = Sym(0);
        let now = tick as u32;
        if r.want && now >= r.ready {
            let aim = if r.aim.0 != 0 { world.entity(r.aim.0) } else { None }.or_else(|| world.get::<NetAvatar>(e).filter(|n| n.head.0 != 0).and_then(|n| world.entity(n.head.0))).unwrap_or(e);
            let aim_uid = world.uid_of(aim).unwrap_or(uid);
            match strike_one(world, &rules, fields, meshes, e, uid, &r) {
                Ok((piece, at)) => {
                    r.struck = r.struck.wrapping_add(1);
                    events.push(("build_struck".to_string(), piece, Some(at)));
                    events.push(("build_damaged".to_string(), piece, Some(at)));
                    events.push(("build_strike_ok".to_string(), aim_uid, Some(at)));
                }
                Err(why) => {
                    r.denied = Sym(world.strings_mut().intern(why));
                    let at = world.get::<Transform>(e).map(|t| t.pos);
                    events.push(("build_strike_refused".to_string(), aim_uid, at));
                    events.push((format!("build_strike_refused_{why}"), aim_uid, at));
                }
            }
            r.ready = now.saturating_add(ticks(r.pause, dt).max(ticks(0.25, dt)));
        }
        r.want = false;
        if r != before {
            if let Some(w) = world.get_mut::<Striker>(e) {
                *w = r;
            }
        }
    }
}

/// The cupboards of the world that can ring: (uid, entity, place, radius, the alarm).
fn cupboards(world: &mut World) -> Vec<(u64, Entity, V3, f32, RaidAlarm)> {
    let mut out: Vec<(u64, Entity, V3, f32, RaidAlarm)> = Vec::new();
    let _ = world.for_each_uid::<(&RaidAlarm, &BuildUpkeep, &Transform), _>(|e, uid, (a, u, t)| {
        if t.pos.iter().all(|v| v.is_finite()) {
            let radius = if a.radius.is_finite() && a.radius > 0.0 { a.radius } else { u.radius.max(0.0) };
            out.push((uid, e, t.pos, radius, *a));
        }
    });
    out.sort_by_key(|c| c.0);
    out
}

/// Tell the players of a cupboard that it was hit: the owner and the owner's party, if the owner is in the game (the party is found through the owner's avatar). Returns how many were told.
fn ring(world: &mut World, owner: u32, at: V3, events: &mut Events) -> u32 {
    let owner_av = avatar_of(world, owner);
    let mut told: Vec<u64> = owner_av.into_iter().collect();
    let party = owner_av.map_or(0, |o| crate::party::party_of(world, o));
    if party != 0 {
        let mut all: Vec<u64> = Vec::new();
        let _ = world.for_each_uid::<(&NetAvatar,), _>(|_, uid, (_,)| all.push(uid));
        all.sort_unstable();
        for uid in all {
            if !told.contains(&uid) && crate::party::party_of(world, uid) == party {
                told.push(uid);
            }
        }
    }
    told.truncate(MAX_TOLD);
    for uid in &told {
        events.push(("raid_alarm".to_string(), *uid, Some(at)));
    }
    told.len() as u32
}

/// The cupboards' alarms: a piece within a cupboard's range was damaged by a blast this tick.
fn run_alarms(world: &mut World, tick: u64, dt: f32, events: &mut Events) {
    let damaged: Vec<V3> = events.iter().filter(|e| e.0 == "build_damaged").filter_map(|e| e.2).filter(|p| p.iter().all(|v| v.is_finite())).take(MAX_DAMAGED).collect();
    if damaged.is_empty() {
        return;
    }
    let now = tick as u32;
    for (_, e, pos, radius, mut a) in cupboards(world) {
        let quiet = ticks(a.throttle, dt);
        // (a stamp from a clock that is older than this one, a world loaded into a younger clock, is over)
        if a.last != 0 && a.last <= now.wrapping_add(1) && now.wrapping_add(1) - a.last < quiet {
            continue;
        }
        let Some(at) = damaged.iter().copied().find(|p| dist(*p, pos) <= radius) else { continue };
        let owner = world.get::<BuildUpkeep>(e).map_or(0, |u| u.owner);
        let owner = if owner != 0 { owner } else { world.get::<BuildPiece>(e).map_or(0, |p| p.owner) };
        a.told = ring(world, owner, at, events);
        a.rung = a.rung.wrapping_add(1);
        a.last = now.wrapping_add(1);
        if let Some(w) = world.get_mut::<RaidAlarm>(e) {
            *w = a;
        }
    }
}

/// A cupboard that is a building piece is its builder's: its `BuildUpkeep.owner` is the piece's owner while that is 0 (the land is then the builder's, as the engine's own cupboards say).
fn adopt_owners(world: &mut World) {
    let mut todo: Vec<(Entity, u32)> = Vec::new();
    let _ = world.for_each_uid::<(&RaidAlarm, &BuildUpkeep, &BuildPiece), _>(|e, _, (_, u, p)| {
        if u.owner == 0 && p.owner != 0 {
            todo.push((e, p.owner));
        }
    });
    for (e, owner) in todo {
        if let Some(u) = world.get_mut::<BuildUpkeep>(e) {
            u.owner = owner;
        }
    }
}

/// Base depth, once a tick, after the blasts and the building system: repairs, then the alarms.
pub fn update(world: &mut World, tick: u64, dt: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    adopt_owners(world);
    run_repairs(world, tick, dt, fields, meshes, events);
    run_strikes(world, tick, dt, fields, meshes, events);
    run_alarms(world, tick, dt, events);
}
