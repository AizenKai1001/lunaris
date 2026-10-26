//! Code locks on doors and boxes (docs/RAIDING.md): a thing with an `Interactable` and a `CodeLock` opens only for the players who know its code. The server enforces it; nothing the client says is believed but the code itself.
//!
//! * `CodeLock` (on the door or the container) keeps the code (0 to 9999, `-1` while none is chosen), the names of the players who entered it (`auth`, eight, the oldest goes when a ninth comes) and who has been wrong lately (`who`, `strikes`, `since`: three wrong codes
//!   lock that player out of that lock for 30 seconds, and nobody else is held by it). The use button (`interact.rs`) refuses a player who is not in `auth` (`locked`, and the event `lock_prompt` about the player: its screen asks for the code).
//! * `Keypad` (on the avatar, the player's side) carries the requests the game sets from the player's commands: `act` 1 enter the code `code` at the lock the player looks at (the right code puts the player in `auth` and opens the thing at once, as if the use button had been
//!   pressed), 2 set a new code (a player in `auth`: everybody else's authorisation goes, `auth` is only the one who changed it), 3 fit a lock (the item `item` leaves the bag and a `CodeLock` with no code is put on the door or container, which must be able to be destroyed: it has `HitPoints`; the fitter is its keeper) or, at a lock the player is in
//!   `auth` of, ask for the page that changes the code (`lock_setup`). The aim is the same as the use button's (`reach` metres along the aim entity, a wall in front hides things), so a code can only be tried at a lock the player stands at and looks at.
//!
//! The player is known by `NetAvatar.name` (the name it connected with; a secure server: the name in its token): a lock remembers names, so what a player entered survives a server restart (the lock is in the world's save) and a player who leaves and comes back is still in. A scene with no `NetAvatar`
//! knows a player by its uid. Deterministic: nothing is random; the times are ticks, and a lockout that began in a tick that has not come yet (a world loaded into a younger clock) is over.
use crate::components::{HitPoints, Interactable, Interactor, Lifetime, NetAvatar, NetEntity, Parent, Transform};
use crate::interact::look_at;
use crate::inventory::{table_of, Bag, Inventory};
use crate::meshcol::MeshStore;
use crate::physics_rot::{axes_of, normalize_q};
use crate::terrain::Terrains;
use lunaris_ecs::{component, Entity, Sym, Uid, World};

type V3 = [f32; 3];
type Events = Vec<(String, u64, Option<V3>)>;

/// The most a code can be (four digits).
pub const MAX_CODE: i32 = 9999;
/// Bit 2 of `NetEntity.flags` is set on a thing with a code lock (bits 0 and 1 are the engine's: a door is open, a container is open), so that the clients can show a lock.
pub const FLAG_LOCKED: u32 = 4;
/// The shortest time between two requests of one player (seconds): a player cannot ask a thousand times a second, whatever the lock's own limits.
const PAUSE: f32 = 0.4;

component! {
    /// A code lock on a door or a container (an entity with an `Interactable`): only the players in `auth` can use it; the others are refused (`locked`) and told to enter the code (the event `lock_prompt`, about the player). `code` is 0 to 9999, -1 while none is chosen (nobody but the keeper can use a lock with no
    /// code). `auth` holds the names of the players who entered the code, eight (the oldest goes when a ninth comes), `next` is where the next goes. `who`, `strikes` and `since` are the players who were wrong lately: after `tries` wrong codes (3) a player cannot try again for `lockout` seconds (30)
    /// at this lock (`since` is the tick of the lockout, plus one); another player is not held by it. `owner` is the name of the player who fitted it. Setting a new code takes everybody's authorisation away (`Keypad`). The engine writes the flag 4 of `NetEntity.flags` of a thing it fits a lock to.
    pub struct CodeLock {
        pub code: i32 = -1,
        pub auth: [Sym; 8] = [Sym(0); 8],
        pub next: u32 = 0,
        pub who: [Sym; 4] = [Sym(0); 4],
        pub strikes: [u32; 4] = [0; 4],
        pub since: [u32; 4] = [0; 4],
        pub tries: u32 = 3,
        pub lockout: f32 = 30.0,
        pub owner: Sym = Sym(0),
    }
}

component! {
    /// The player's side of code locks (docs/RAIDING.md), on the avatar: the game sets `act` for one tick: 1 enter `code` at the lock the player looks at, 2 set `code` as the lock's new code, 3 fit a lock (the item `item`, one is taken from the bag) to the door or container
    /// aimed at, or, at a lock the player is a keeper of, ask for the page that changes the code. The engine answers at once and clears the request; a refusal is in `denied` for a tick (`no_target`, `no_lock`, `no_code`, `not_yours`, `bad_code`, `not_lockable`, `no_item`, `dead`, and `wrong_code` and `locked_out` for a code that was not accepted) and the player is told
    /// by the events `lock_refused` and `lock_refused_REASON` (a wrong code: `lock_bad`, `lock_out`). `reach` is how far it looks along the aim entity (`aim`, or the `NetAvatar`'s `head`, or this entity), `inventory` whose bag it is (0: this entity's, else its parent's), `kinds` which kinds of `Interactable` can be given a lock
    /// (a bit mask: bit 1 doors, bit 2 containers; 6), `tries` and `lockout` what a new lock allows, `ready` the tick of the next request that is heard. Events, all about the player's aim entity at the lock's place: `lock_ok` (the code was right), `lock_bad`, `lock_out`, `lock_new` (a code was set),
    /// `lock_placed` (a lock was fitted: the player now chooses its code), `lock_setup` (a keeper may change the code).
    pub struct Keypad {
        pub act: u32 = 0,
        pub code: i32 = -1,
        pub item: Sym = Sym(0),
        pub reach: f32 = 3.0,
        pub aim: Uid = Uid(0),
        pub inventory: Uid = Uid(0),
        pub kinds: u32 = 6,
        pub tries: u32 = 3,
        pub lockout: f32 = 30.0,
        pub ready: u32 = 0,
        pub denied: Sym = Sym(0),
    }
}

/// The name a player is known by on locks: its `NetAvatar.name` (on the entity or its parent), else its uid.
pub fn identity(world: &mut World, e: Entity) -> Sym {
    let named = |w: &World, x: Entity| w.get::<NetAvatar>(x).map(|n| n.name).filter(|n| n.0 != 0);
    if let Some(n) = named(world, e) {
        return n;
    }
    let parent = world.get::<Parent>(e).and_then(|p| world.entity(p.target.0));
    if let Some(n) = parent.and_then(|p| named(world, p)) {
        return n;
    }
    let root = parent.unwrap_or(e);
    let uid = world.uid_of(root).unwrap_or(0);
    Sym(world.strings_mut().intern(&format!("#{uid}")))
}

/// Is `who` in the lock's `auth`?
pub fn authorized(lock: &CodeLock, who: Sym) -> bool {
    who.0 != 0 && lock.auth.contains(&who)
}

fn authorize(lock: &mut CodeLock, who: Sym) {
    if authorized(lock, who) {
        return;
    }
    // (the first free place; when all eight are taken the oldest goes: `next` walks round them)
    if let Some(i) = lock.auth.iter().position(|a| a.0 == 0) {
        lock.auth[i] = who;
        return;
    }
    let k = lock.next as usize % lock.auth.len();
    lock.auth[k] = who;
    lock.next = lock.next.wrapping_add(1);
}

/// The use button's question (`interact.rs`): may the player who is `who` use this thing? A lock with no code, or one the player is not in, refuses.
pub fn may_use(lock: &CodeLock, who: Sym) -> bool {
    authorized(lock, who)
}

/// The slot of `who` among the players who were wrong lately (a free one, or the one with the fewest strikes that is not locked out, when there is none).
fn slot_of(lock: &CodeLock, who: Sym, now: u32, len: u32) -> usize {
    if let Some(i) = lock.who.iter().position(|w| *w == who) {
        return i;
    }
    if let Some(i) = lock.who.iter().position(|w| w.0 == 0) {
        return i;
    }
    (0..lock.who.len()).filter(|&i| !locked_out(lock.since[i], now, len)).min_by_key(|&i| (lock.strikes[i], i)).unwrap_or(0)
}

/// Is a lockout that began at `since` (a tick plus one, 0 for none) still going at `now`, when it lasts `len` ticks? One that began in a tick that has not come is over.
fn locked_out(since: u32, now: u32, len: u32) -> bool {
    since != 0 && now.wrapping_add(1) >= since && now.wrapping_add(1) - since < len
}

fn ticks(secs: f32, dt: f32) -> u32 {
    if !(secs > 0.0) {
        return 0;
    }
    ((secs / dt).ceil() as u32).max(1)
}

/// Whose bag a keypad uses: the one it names, its own, or its parent's.
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

/// The entity that looks: the one named, the `head` of the `NetAvatar`, or the keypad's own.
fn aim_of(world: &World, e: Entity, k: &Keypad) -> Entity {
    if k.aim.0 != 0 {
        if let Some(a) = world.entity(k.aim.0) {
            return a;
        }
    }
    world.get::<NetAvatar>(e).filter(|n| n.head.0 != 0).and_then(|n| world.entity(n.head.0)).unwrap_or(e)
}

/// What a request did.
struct Answer {
    /// The event that tells the player (None: nothing to tell).
    event: Option<&'static str>,
    /// The reason it was refused.
    denied: &'static str,
    /// The place of the lock.
    at: V3,
    /// Press the use button for the player now (the code was right).
    press: bool,
}

fn refuse(why: &'static str, at: V3) -> Answer {
    Answer { event: Some("lock_refused"), denied: why, at, press: false }
}

#[allow(clippy::too_many_arguments)]
fn handle(world: &mut World, fields: &Terrains, meshes: &MeshStore, tick: u64, dt: f32, e: Entity, uid: u64, k: &Keypad) -> Answer {
    let now = tick as u32;
    let origin = world.get::<Transform>(e).map_or([0.0; 3], |t| t.pos);
    // the bag and who it is
    let bag_e = bag_of(world, e, k.inventory);
    let body = bag_e.unwrap_or(e);
    if world.get::<HitPoints>(body).is_some_and(|h| h.dead || h.value <= 0.0) {
        return refuse("dead", origin);
    }
    let who = identity(world, body);
    // what the player looks at
    let aim_e = aim_of(world, e, k);
    let Some(xf) = world.get::<Transform>(aim_e).copied() else { return refuse("no_target", origin) };
    if !xf.pos.iter().chain(xf.rot.iter()).all(|v| v.is_finite()) {
        return refuse("no_target", origin);
    }
    let z = axes_of(normalize_q(xf.rot))[2];
    let reach = if k.reach.is_finite() { k.reach.clamp(0.1, 20.0) } else { 3.0 };
    let aim_uid = world.uid_of(aim_e).unwrap_or(uid);
    let Some((_, _, te)) = look_at(world, fields, meshes, xf.pos, [-z[0], -z[1], -z[2]], reach, aim_uid) else { return refuse("no_target", origin) };
    let at = world.get::<Transform>(te).map_or(origin, |t| t.pos);
    let lock = world.get::<CodeLock>(te).copied();
    let kind = world.get::<Interactable>(te).map_or(0, |i| i.kind);
    match k.act {
        1 => {
            let Some(mut lock) = lock else { return refuse("no_lock", at) };
            if authorized(&lock, who) {
                return Answer { event: Some("lock_ok"), denied: "", at, press: true };
            }
            if lock.code < 0 {
                return refuse("no_code", at);
            }
            let len = ticks(lock.lockout, dt);
            let i = slot_of(&lock, who, now, len);
            if lock.who[i] != who {
                (lock.who[i], lock.strikes[i], lock.since[i]) = (who, 0, 0);
            }
            if lock.since[i] != 0 && !locked_out(lock.since[i], now, len) {
                (lock.since[i], lock.strikes[i]) = (0, 0); // (the lockout is over: three new tries)
            }
            let answer = if locked_out(lock.since[i], now, len) {
                Answer { event: Some("lock_out"), denied: "locked_out", at, press: false }
            } else if k.code == lock.code {
                authorize(&mut lock, who);
                (lock.who[i], lock.strikes[i], lock.since[i]) = (Sym(0), 0, 0);
                Answer { event: Some("lock_ok"), denied: "", at, press: true }
            } else {
                lock.strikes[i] += 1;
                if lock.strikes[i] >= lock.tries.max(1) {
                    (lock.since[i], lock.strikes[i]) = (now.wrapping_add(1), 0);
                    Answer { event: Some("lock_out"), denied: "locked_out", at, press: false }
                } else {
                    Answer { event: Some("lock_bad"), denied: "wrong_code", at, press: false }
                }
            };
            if let Some(w) = world.get_mut::<CodeLock>(te) {
                *w = lock;
            }
            answer
        }
        2 => {
            let Some(mut lock) = lock else { return refuse("no_lock", at) };
            if !authorized(&lock, who) {
                return refuse("not_yours", at);
            }
            if !(0..=MAX_CODE).contains(&k.code) {
                return refuse("bad_code", at);
            }
            // (a new code: nobody who knew the old one is in any more, but the one who set it)
            lock.code = k.code;
            lock.auth = [Sym(0); 8];
            lock.next = 0;
            lock.who = [Sym(0); 4];
            lock.strikes = [0; 4];
            lock.since = [0; 4];
            authorize(&mut lock, who);
            if let Some(w) = world.get_mut::<CodeLock>(te) {
                *w = lock;
            }
            Answer { event: Some("lock_new"), denied: "", at, press: false }
        }
        3 => {
            if let Some(lock) = lock {
                // (at a lock of its own: the page that changes the code, for a keeper)
                return if authorized(&lock, who) { Answer { event: Some("lock_setup"), denied: "", at, press: false } } else { refuse("not_yours", at) };
            }
            // (a door or a box of the kinds the keypad allows, that stays (no `Lifetime`: a corpse) and that can be destroyed (`HitPoints`): a lock can only go where it can be raided, so nobody can lock the world's loot crates away from everybody)
            if kind == 0 || kind > 31 || k.kinds & (1u32 << kind) == 0 || world.get::<Interactable>(te).is_none() || world.get::<Lifetime>(te).is_some() || world.get::<HitPoints>(te).is_none() {
                return refuse("not_lockable", at);
            }
            let Some(bag_e) = bag_e else { return refuse("no_item", at) };
            let Some((table, _)) = table_of(world) else { return refuse("no_item", at) };
            let key = world.strings().get(k.item.0).to_string();
            let id = table.id_of(&key);
            let Some(mut inv) = world.get::<Inventory>(bag_e).copied() else { return refuse("no_item", at) };
            let mut bag = Bag::of(&inv);
            if id == 0 || bag.total(id) == 0 {
                return refuse("no_item", at);
            }
            bag.remove(id, 1);
            bag.store(&mut inv);
            if let Some(w) = world.get_mut::<Inventory>(bag_e) {
                *w = inv;
            }
            let mut fresh = CodeLock { tries: k.tries.max(1), lockout: if k.lockout.is_finite() { k.lockout.max(0.0) } else { 30.0 }, owner: who, ..CodeLock::default() };
            authorize(&mut fresh, who);
            let _ = world.insert(te, fresh);
            if let Some(n) = world.get_mut::<NetEntity>(te) {
                n.flags |= FLAG_LOCKED;
            }
            Answer { event: Some("lock_placed"), denied: "", at, press: false }
        }
        _ => refuse("no_target", at),
    }
}

/// The players' requests, before the use buttons are looked at (a right code presses the use button for the player in the same step).
pub fn update(world: &mut World, tick: u64, dt: f32, fields: &Terrains, meshes: &MeshStore, events: &mut Events) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let mut asks: Vec<(Entity, u64, Keypad)> = Vec::new();
    let _ = world.for_each_uid::<(&Keypad,), _>(|e, uid, (k,)| {
        if k.act != 0 || k.denied.0 != 0 {
            asks.push((e, uid, *k));
        }
    });
    asks.sort_by_key(|a| a.1);
    for (e, uid, mut k) in asks {
        let before = k;
        k.denied = Sym(0);
        let now = tick as u32;
        if k.act != 0 && now >= k.ready {
            let aim = aim_of(world, e, &k);
            let aim_uid = world.uid_of(aim).unwrap_or(uid);
            let ans = handle(world, fields, meshes, tick, dt, e, uid, &k);
            if let Some(name) = ans.event {
                events.push((name.to_string(), aim_uid, Some(ans.at)));
                if name == "lock_refused" && !ans.denied.is_empty() {
                    events.push((format!("lock_refused_{}", ans.denied), aim_uid, Some(ans.at)));
                }
            }
            if !ans.denied.is_empty() {
                k.denied = Sym(world.strings_mut().intern(ans.denied));
            }
            if ans.press {
                // the player uses what it looks at, as if it had pressed the button: the lock lets it through now
                if let Some(i) = world.get_mut::<Interactor>(aim) {
                    i.act = true;
                    i.held = false;
                }
            }
            k.ready = now.saturating_add(ticks(PAUSE, dt));
        }
        k.act = 0;
        if k != before {
            if let Some(w) = world.get_mut::<Keypad>(e) {
                *w = k;
            }
        }
    }
}
