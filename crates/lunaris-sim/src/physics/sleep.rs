//! Sleeping: a group of rigid bodies that has lain still for a while stops being solved, and costs next to nothing until something disturbs it.
//!
//! The rules, as they are in both physics paths (the reference and the fast one give the same bits, sleeping included):
//!
//! * The world has a `PhysicsSleep` (the lowest uid wins) or nothing sleeps. A body tied by a joint, a convex hull, a trigger and a body that moves
//!   faster than the limits never counts as still.
//! * Every tick each awake dynamic body that is not moving fast keeps a timer and an anchor (where it was when the timer started): the timer grows by
//!   the step while the body has not strayed farther than `linear * time` from its anchor and its spin is under `angular`, and starts again from the
//!   body's place otherwise. (A body is therefore still when its average speed over the window is under `linear`; a stack of plain boxes that buzzes
//!   at a few centimetres a second without going anywhere is still, which a limit on the instant speed would not say.) An **island** is a group of awake dynamic bodies that touch each other (through this tick's contacts; scenery and sleepers do not join
//!   islands). When the timer of every body of an island has reached `time`, the island goes to sleep: its bodies get zero velocity and from the next tick
//!   are built as bodies that never move (`Body::asleep`, `inv_mass` 0), so no forces, no contact rows and no pair tests between sleepers are made.
//! * A sleeping body wakes, together with its whole island, when
//!   - an awake dynamic body's box overlaps it (the step is then made again with the island awake: `SleepState::restart`),
//!   - its position, velocity or spin is not what it was when it fell asleep (someone moved it, pushed it, buoyancy or a script touched it),
//!   - the colliders around it changed: while the signature of all the scenery is the same as last tick nothing is checked, and when it is not, each
//!     sleeper compares the scenery that overlaps its box with what it was when it fell asleep (`touch`), so a floor taken away or a wall that
//!     moves in wakes what is near it and nothing else,
//!   - the entity is gone, or no longer has what a body needs.
//! * The sleep state is **state**, like `ContactMemory`: cloned with a `Sim`, kept in a checkpoint and a recording, hashed when there is any, emptied
//!   by `Sim::clear`. The contact impulses of a sleeping island are kept (`held`) and given back to the solver when it wakes.
use super::{dot, sub, Body, Shape};
use crate::components::{Collider, PhysicsSleep, Transform};
use crate::contact_memory::ContactMemory;
use crate::physics_rot::V3;
use lunaris_ecs::{Entity, StateHasher, World};
use lunaris_json::Json;

/// How many bodies have gone to sleep, and how many sleepers have been woken, in all the tests so far (to see that a test reaches them).
#[cfg(test)]
pub(crate) static SLEPT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(test)]
pub(crate) static WOKE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A body that moves faster than this (metres per second) cannot be about to sleep, and keeps no timer.
const MAYBE_STILL: f32 = 1.0;

/// How far (metres) a collider may be from a sleeper's box and still count as touching it for the `touch` signature.
pub(super) const TOUCH_MARGIN: f32 = 0.02;

/// The limits of a world (`PhysicsSleep`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Config {
    pub time: f32,
    /// The square of how far a body may stray from its anchor in `time` seconds and still be still.
    pub reach2: f32,
    pub ang2: f32,
}

impl Config {
    pub(super) fn of(world: &mut World) -> Option<Config> {
        let mut best: Option<(u64, PhysicsSleep)> = None;
        world
            .for_each_uid::<(&PhysicsSleep,), _>(|_, uid, (s,)| {
                if best.as_ref().is_none_or(|(u, _)| uid < *u) {
                    best = Some((uid, *s));
                }
            })
            .ok()?;
        let (_, s) = best?;
        (s.time.is_finite() && s.time > 0.0 && s.linear.is_finite() && s.angular.is_finite()).then(|| Config { time: s.time, reach2: (s.linear.max(0.0) * s.time).powi(2), ang2: s.angular.max(0.0).powi(2) })
    }
}

/// A body that is asleep, and what it was like when it lay down: it wakes if its `Transform` or `Collider` is not exactly this any more (or its speed not zero).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Sleeper {
    pub uid: u64,
    /// The smallest uid of its island: the name of the island.
    pub island: u64,
    /// `Transform`: position, rotation, scale.
    pub tf: [f32; 10],
    /// `Collider`.
    pub shape: u32,
    pub size: V3,
    pub trigger: bool,
    /// Half the size of the box around it (as a `Body` has it).
    pub ext: V3,
    /// What the scenery around it looked like (`touch_in`).
    pub touch: u64,
}

impl Sleeper {
    pub(super) fn pos(&self) -> V3 {
        [self.tf[0], self.tf[1], self.tf[2]]
    }
    /// The `Transform` it lay with.
    pub(super) fn transform(&self) -> Transform {
        Transform { pos: self.pos(), rot: [self.tf[3], self.tf[4], self.tf[5], self.tf[6]], scale: [self.tf[7], self.tf[8], self.tf[9]] }
    }
    /// The `Collider` it lay with.
    pub(super) fn collider(&self) -> Collider {
        Collider { shape: self.shape, size: self.size, trigger: self.trigger }
    }
}

fn tf_of(t: &Transform) -> [f32; 10] {
    [t.pos[0], t.pos[1], t.pos[2], t.rot[0], t.rot[1], t.rot[2], t.rot[3], t.scale[0], t.scale[1], t.scale[2]]
}

/// A number for each change of a list of sleepers, so that a cache built from one can tell it from another of the same length.
static GENERATIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn next_generation() -> u64 {
    GENERATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

/// What the physics step remembers of sleeping (see the top of the file). `SleepState::default()` is a world where nothing sleeps.
#[derive(Clone, Debug, Default)]
pub struct SleepState {
    /// For each awake dynamic body that is not moving fast: how long it has stayed near its anchor (seconds) and the anchor, by uid.
    timers: Vec<(u64, f32, V3)>,
    /// The bodies that sleep, by uid.
    sleepers: Vec<Sleeper>,
    /// The contact impulses of the sleeping islands, kept until they wake.
    held: ContactMemory,
    /// The signature of all the scenery as of the last step.
    statics: u64,
    /// Not state (empty between ticks): the uids of the bodies that woke in this step, whose contact impulses are still to be given back.
    pending: Vec<u64>,
    /// Not state (empty between ticks): the bodies that fell asleep in this step, whose `Transform` and `Collider` are read after the step has written them.
    fresh: Vec<(u64, Entity)>,
    /// Not state: a number that is new whenever the list of sleepers changes (and equal for a copy), for the cache that the fast path makes of it.
    generation: u64,
    /// Not state (false between ticks): something woke, so the step in progress must be made again.
    pub(super) restart: bool,
}

impl PartialEq for SleepState {
    fn eq(&self, o: &SleepState) -> bool {
        self.timers == o.timers && self.sleepers == o.sleepers && self.held == o.held && self.statics == o.statics
    }
}

#[inline]
pub(super) fn mix(uid: u64, pos: V3, ext: V3) -> u64 {
    let mut h = uid.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD6E8_FEB8_6659_FD93;
    for v in pos.iter().chain(ext.iter()) {
        h = (h ^ v.to_bits() as u64).wrapping_mul(0x0000_0100_0000_01B3);
        h ^= h >> 29;
    }
    h
}

/// A body that counts as scenery for the signatures: it never moves, is not a trigger and is not a sleeper.
#[inline]
pub(super) fn is_scenery(b: &Body) -> bool {
    b.inv_mass == 0.0 && !b.trigger && !b.asleep
}

/// The signature of the scenery among `bodies`: the sum (so the order does not matter) of a mix of each one's uid, place and size.
pub(super) fn scenery_sig(bodies: &[Body]) -> u64 {
    bodies.iter().filter(|b| is_scenery(b)).fold(0u64, |s, b| s.wrapping_add(mix(b.uid, b.pos, b.ext)))
}

/// Does the box of something at `pos` with half sizes `ext` come within `TOUCH_MARGIN` of the box of `b`?
#[inline]
pub(super) fn near(pos: V3, ext: V3, b: &Body) -> bool {
    (0..3).all(|k| (pos[k] - b.pos[k]).abs() <= ext[k] + b.ext[k] + TOUCH_MARGIN)
}

/// The indices of the bodies among `bodies` that are scenery (`is_scenery`).
pub(super) fn scenery_idx(bodies: &[Body]) -> Vec<usize> {
    (0..bodies.len()).filter(|&i| is_scenery(&bodies[i])).collect()
}

/// The signature of the scenery among `bodies` (`scenery`: their indices) that touches a box at `pos` with half sizes `ext`.
pub(super) fn touch_in(bodies: &[Body], scenery: &[usize], pos: V3, ext: V3) -> u64 {
    scenery.iter().map(|&j| &bodies[j]).filter(|b| near(pos, ext, b)).fold(0u64, |s, b| s.wrapping_add(mix(b.uid, b.pos, b.ext)))
}

/// How the signature of the scenery around a box is found: `(all the bodies, the indices of those that are scenery, the box's place, its half sizes)`.
pub(super) type Touch<'a> = &'a dyn Fn(&[Body], &[usize], V3, V3) -> u64;

impl SleepState {
    pub fn is_empty(&self) -> bool {
        self.timers.is_empty() && self.sleepers.is_empty() && self.held.is_empty() && self.statics == 0
    }

    pub fn clear(&mut self) {
        *self = SleepState::default();
    }

    /// How many bodies are asleep.
    pub fn sleeper_count(&self) -> usize {
        self.sleepers.len()
    }

    pub fn is_asleep(&self, uid: u64) -> bool {
        self.find(uid).is_some()
    }

    pub(super) fn has_sleepers(&self) -> bool {
        !self.sleepers.is_empty()
    }

    pub(super) fn statics(&self) -> u64 {
        self.statics
    }

    fn find(&self, uid: u64) -> Option<usize> {
        self.sleepers.binary_search_by_key(&uid, |s| s.uid).ok()
    }

    /// Is a gathered body a sleeper that still lies as it did? Returns true for one. A sleeper whose `Transform` or `Collider` has changed, or that has been
    /// pushed or spun, is not (the island is named in `bad`, to be woken by `finish_gather`); `seen` marks the sleepers that were found.
    pub(super) fn gather(&self, uid: u64, t: &Transform, c: &Collider, vel: V3, spin: V3, seen: &mut [bool], bad: &mut Vec<u64>) -> bool {
        let Some(k) = self.find(uid) else { return false };
        seen[k] = true;
        let s = &self.sleepers[k];
        if s.tf == tf_of(t) && s.shape == c.shape && s.size == c.size && s.trigger == c.trigger && vel == [0.0; 3] && spin == [0.0; 3] {
            true
        } else {
            bad.push(s.island);
            false
        }
    }

    /// After all the bodies were gathered: wake the islands of the sleepers that were not found (their entity is gone) and those named in `bad`.
    pub(super) fn finish_gather(&mut self, seen: &[bool], bad: &mut Vec<u64>) {
        for (k, s) in self.sleepers.iter().enumerate() {
            if !seen[k] {
                bad.push(s.island);
            }
        }
        self.wake_islands(bad);
    }

    /// Wake these islands (by name).
    pub(super) fn wake_islands(&mut self, islands: &mut Vec<u64>) {
        if islands.is_empty() {
            return;
        }
        islands.sort_unstable();
        islands.dedup();
        let pending = &mut self.pending;
        self.sleepers.retain(|s| {
            if islands.binary_search(&s.island).is_ok() {
                pending.push(s.uid);
                #[cfg(test)]
                WOKE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                false
            } else {
                true
            }
        });
        self.generation = next_generation();
        self.restart = true;
    }

    /// The sleepers, by uid.
    pub(super) fn records(&self) -> &[Sleeper] {
        &self.sleepers
    }

    /// Changes whenever the list of sleepers does (not a state: a cache of the list uses it to know when to be made again).
    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    /// After a step has written its bodies: the bodies that fell asleep in it are recorded as they now are in the world.
    pub(super) fn finish_new(&mut self, world: &World) {
        if self.fresh.is_empty() {
            return;
        }
        for (uid, entity) in std::mem::take(&mut self.fresh) {
            let (Some(t), Some(c)) = (world.get::<Transform>(entity), world.get::<Collider>(entity)) else { continue };
            if let Some(k) = self.find(uid) {
                let s = &mut self.sleepers[k];
                s.tf = tf_of(t);
                s.shape = c.shape;
                s.size = c.size;
                s.trigger = c.trigger;
            }
        }
        self.generation = next_generation();
    }

    /// The island that a sleeper is in.
    pub(super) fn island_of(&self, uid: u64) -> Option<u64> {
        self.find(uid).map(|k| self.sleepers[k].island)
    }

    /// Give the contact impulses of the bodies that woke back to `memory`.
    pub(super) fn take_pending(&mut self, memory: &mut ContactMemory) {
        if self.pending.is_empty() {
            return;
        }
        self.pending.sort_unstable();
        let woke = std::mem::take(&mut self.pending);
        let (rest, back) = self.held.partition(|a, b| woke.binary_search(&a).is_ok() || woke.binary_search(&b).is_ok());
        self.held = rest;
        *memory = memory.merged(&back);
    }

    /// When the scenery changed since the last step: wake the islands whose sleepers see other scenery around them than when they lay down (`touch` gives
    /// the signature of the scenery around a box at a place with half sizes).
    pub(super) fn check_scenery(&mut self, touch: &dyn Fn(V3, V3) -> u64) {
        let mut bad: Vec<u64> = Vec::new();
        for s in &self.sleepers {
            if s.touch != touch(s.pos(), s.ext) {
                bad.push(s.island);
            }
        }
        self.wake_islands(&mut bad);
    }

    /// Adds the state to a state hash.
    pub fn hash_into(&self, h: &mut StateHasher) {
        h.u64(self.statics);
        h.u64(self.timers.len() as u64);
        for (uid, t, anchor) in &self.timers {
            h.u64(*uid);
            h.u64(t.to_bits() as u64);
            for c in anchor {
                h.u64(c.to_bits() as u64);
            }
        }
        h.u64(self.sleepers.len() as u64);
        for s in &self.sleepers {
            h.u64(s.uid);
            h.u64(s.island);
            for c in s.tf.iter().chain(s.size.iter()).chain(s.ext.iter()) {
                h.u64(c.to_bits() as u64);
            }
            h.u64(s.shape as u64);
            h.u64(s.trigger as u64);
            h.u64(s.touch);
        }
        self.held.hash_into(h);
    }

    /// As JSON for a recording (hex bit patterns, so nothing is rounded).
    pub fn to_json(&self) -> Json {
        let hex = |v: u64| Json::str(format!("{v:016x}"));
        let bits = |x: f32| Json::str(format!("{:08x}", x.to_bits()));
        Json::obj([
            ("statics", hex(self.statics)),
            ("timers", Json::arr(self.timers.iter().map(|(u, t, a)| Json::arr([hex(*u), bits(*t), bits(a[0]), bits(a[1]), bits(a[2])])))),
            (
                "sleepers",
                Json::arr(self.sleepers.iter().map(|s| {
                    let mut v = vec![hex(s.uid), hex(s.island), hex(s.touch), hex(s.shape as u64), hex(s.trigger as u64)];
                    v.extend(s.tf.iter().chain(s.size.iter()).chain(s.ext.iter()).map(|x| bits(*x)));
                    Json::arr(v)
                })),
            ),
            ("held", self.held.to_json()),
        ])
    }

    /// The inverse of `to_json`; `None` for anything that is not what it writes.
    pub fn from_json(j: &Json) -> Option<SleepState> {
        let hex64 = |v: &Json| v.as_str().and_then(|s| u64::from_str_radix(s, 16).ok());
        let hex32 = |v: &Json| v.as_str().and_then(|s| u32::from_str_radix(s, 16).ok()).map(f32::from_bits);
        let mut st = SleepState { statics: hex64(j.get("statics")?)?, ..SleepState::default() };
        for t in j.get("timers")?.as_array()? {
            let t = t.as_array()?;
            if t.len() != 5 {
                return None;
            }
            st.timers.push((hex64(&t[0])?, hex32(&t[1])?, [hex32(&t[2])?, hex32(&t[3])?, hex32(&t[4])?]));
        }
        for s in j.get("sleepers")?.as_array()? {
            let s = s.as_array()?;
            if s.len() != 5 + 10 + 3 + 3 {
                return None;
            }
            let f = |i: usize| hex32(&s[5 + i]);
            let mut tf = [0.0f32; 10];
            for (i, x) in tf.iter_mut().enumerate() {
                *x = f(i)?;
            }
            st.sleepers.push(Sleeper {
                uid: hex64(&s[0])?,
                island: hex64(&s[1])?,
                touch: hex64(&s[2])?,
                shape: u32::try_from(hex64(&s[3])?).ok()?,
                trigger: hex64(&s[4])? != 0,
                tf,
                size: [f(10)?, f(11)?, f(12)?],
                ext: [f(13)?, f(14)?, f(15)?],
            });
        }
        st.held = ContactMemory::from_json(j.get("held")?)?;
        // (both lists are searched by uid)
        if st.timers.windows(2).any(|w| w[0].0 >= w[1].0) || st.sleepers.windows(2).any(|w| w[0].uid >= w[1].uid) {
            return None;
        }
        st.generation = next_generation();
        Some(st)
    }
}

/// After the solve, before anything is written back: advance the timers, find the islands that have been still for long enough and put them to sleep (their
/// velocities are zeroed here, so that zero is what is written back).
///
/// `pairs` are this step's solid contacts (indices into `bodies`), `never` flags the bodies that must stay awake (tied by a joint; empty for none) and
/// `touch` gives the signature of the scenery around a body. `statics` is the signature of the scenery as of this step, and `memory` the contact impulses just
/// collected: those of the islands that go to sleep are moved to `held`.
#[allow(clippy::too_many_arguments)]
pub(super) fn update(state: &mut SleepState, cfg: &Config, dt: f32, bodies: &mut [Body], pairs: &[(usize, usize)], never: &[bool], touch: Touch, statics: u64, memory: &mut ContactMemory) {
    state.statics = statics;
    // (nobody awake: nothing to time, nothing to put to sleep)
    if !bodies.iter().any(|b| b.inv_mass > 0.0) {
        state.timers.clear();
        return;
    }
    let n = bodies.len();
    // the timers: a body that can sleep and is not moving fast keeps one
    let mut t = vec![0.0f32; n];
    let mut anchors = vec![[0.0f32; 3]; n];
    let mut awake = vec![false; n];
    for (i, b) in bodies.iter().enumerate() {
        if b.inv_mass <= 0.0 {
            continue;
        }
        awake[i] = true;
        let w = b.w();
        let can = !b.trigger
            && !matches!(b.shape, Shape::Hull(_) | Shape::Mesh(_) | Shape::Terrain(..))
            && !never.get(i).copied().unwrap_or(false)
            && dot(b.vel, b.vel) < MAYBE_STILL * MAYBE_STILL
            && dot(w, w) < cfg.ang2;
        if !can {
            continue;
        }
        match state.timers.binary_search_by_key(&b.uid, |x| x.0) {
            Ok(k) if { let d = sub(b.pos, state.timers[k].2); dot(d, d) <= cfg.reach2 } => {
                t[i] = state.timers[k].1 + dt;
                anchors[i] = state.timers[k].2;
            }
            // (a new window: from here)
            _ => {
                t[i] = dt;
                anchors[i] = b.pos;
            }
        }
    }
    // the islands of the awake bodies
    let mut parent: Vec<u32> = (0..n as u32).collect();
    fn root(p: &mut [u32], mut x: u32) -> u32 {
        while p[x as usize] != x {
            p[x as usize] = p[p[x as usize] as usize];
            x = p[x as usize];
        }
        x
    }
    for &(a, b) in pairs {
        if awake[a] && awake[b] {
            let (ra, rb) = (root(&mut parent, a as u32), root(&mut parent, b as u32));
            if ra != rb {
                parent[ra.max(rb) as usize] = ra.min(rb);
            }
        }
    }
    let mut min_t = vec![f32::INFINITY; n];
    let mut min_uid = vec![u64::MAX; n];
    let mut roots = vec![0u32; n];
    for i in 0..n {
        if awake[i] {
            let r = root(&mut parent, i as u32);
            roots[i] = r;
            min_t[r as usize] = min_t[r as usize].min(t[i]);
            min_uid[r as usize] = min_uid[r as usize].min(bodies[i].uid);
        }
    }
    // who goes to sleep, and the new timers of the rest
    let mut asleep_now: Vec<usize> = Vec::new();
    let mut timers: Vec<(u64, f32, V3)> = Vec::new();
    for i in 0..n {
        if !awake[i] {
            continue;
        }
        let r = roots[i] as usize;
        if min_t[r] >= cfg.time {
            asleep_now.push(i);
        } else if t[i] > 0.0 {
            timers.push((bodies[i].uid, t[i], anchors[i]));
        }
    }
    timers.sort_unstable_by_key(|x| x.0);
    state.timers = timers;
    if asleep_now.is_empty() {
        return;
    }
    #[cfg(test)]
    SLEPT.fetch_add(asleep_now.len() as u64, std::sync::atomic::Ordering::Relaxed);
    let scenery = scenery_idx(bodies);
    // (the `Transform` and `Collider` they lie with are read by `finish_new`, after the step has written its bodies)
    let mut fresh: Vec<Sleeper> = asleep_now
        .iter()
        .map(|&i| Sleeper { uid: bodies[i].uid, island: min_uid[roots[i] as usize], tf: [0.0; 10], shape: 0, size: [0.0; 3], trigger: false, ext: bodies[i].ext, touch: touch(bodies, &scenery, bodies[i].pos, bodies[i].ext) })
        .collect();
    state.fresh.extend(asleep_now.iter().map(|&i| (bodies[i].uid, bodies[i].entity)));
    for &i in &asleep_now {
        let b = &mut bodies[i];
        b.vel = [0.0; 3];
        if let Some(r) = b.rot.as_deref_mut() {
            r.w = [0.0; 3];
            r.stash = [0.0; 4];
        }
    }
    let mut uids: Vec<u64> = fresh.iter().map(|s| s.uid).collect();
    uids.sort_unstable();
    let (rest, moved) = memory.partition(|a, b| uids.binary_search(&a).is_ok() || uids.binary_search(&b).is_ok());
    *memory = rest;
    state.held = state.held.merged(&moved);
    state.sleepers.append(&mut fresh);
    state.sleepers.sort_unstable_by_key(|s| s.uid);
}
