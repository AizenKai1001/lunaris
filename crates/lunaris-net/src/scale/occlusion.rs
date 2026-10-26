//! Wallhacks: the server does not tell a client where a player is that the client's own character could not see (docs/NETWORK_SCALE.md, "Wallhacks").
//!
//! Interest management alone sends every client the place of everybody within 150 m, through any wall: a modified client only has to draw what it is told (an ESP). With occlusion on, a
//! game says (through [`Game::blocked`], for [`SimGame`](super::simgame::SimGame) the line-of-sight rays of the simulation against the static world) whether the level is in the way of a
//! straight line, and the server leaves a player out of a client's packets while no line from the client's character's eye to the player's head, chest, feet or shoulders is clear, unless
//!
//! * the player is within the **safe radius** (it is always told: a player round a corner next to you, or in the room above, is never late, and nothing has to be exact there),
//! * the player was in sight within the **hold** time (0.6 s after the pair was last tested and found in sight: ducking behind a wall for a moment, or a flickering line past a lamp post, loses nothing), or
//! * the line is clear **a short time ahead** (0.45 s) at the speeds the two are moving now: a client shows others 100 ms in the past, the server hears about the client's own move after the trip
//!   of its packet, finds out when the pair is next tested (up to 0.2 s) and sends the answer back along the same road, so a player must be told about before the line opens or it appears late, which at a run is a body length.
//!
//! The cost is what matters at 200 players and 30 ticks a second, so nothing is tested twice that need not be: the answer for a pair of an observer and a target is kept (in the observer's
//! [`Sightings`]) and asked again only after 3 to 6 ticks (the farther the pair, the longer: it cannot be sent faster than its rate anyway), a pair that has not moved a third of a metre keeps its answer
//! for 18, the first ray of a pair that is in sight is nearly always the last, a pair that is not tested costs one table lookup, and each client has a fixed number of rays to spend a tick
//! ([`Occlusion::rays_per_client`]): beyond that a known pair keeps its old answer, a pair never tested that is within 80 m is told (a short leak is better than a near player who is not shown) and one
//! farther away is not told until it can be tested (the nearest pairs are tested first, so a crowd delays the far end of the street and nothing else). A client can only use up its own rays: a crowd of
//! clients made to make a server run out of rays gets no wallhack out of it for anybody else. The numbers of the pairs and rays are in [`super::server::Stats`].
use super::interest::{Candidate, IdMap};
use super::server::Game;
use super::state::{class, EntityState};

type V3 = [f32; 3];

/// What the wallhack defence asks of the game: which kinds of entity hide behind walls, and whether the level is in the way of a line. Every [`Game`] is one and is asked directly; the server, when it makes the
/// packets of its clients on several threads at once, asks a [`SharedEyes`] (which a game gives from [`Game::blocker`]) because a game as a whole cannot be shared between threads.
pub trait Eyes {
    fn occludes(&self, class_id: u8) -> bool;
    fn blocked(&self, from: V3, to: V3) -> bool;
}

impl<G: Game + ?Sized> Eyes for G {
    #[inline]
    fn occludes(&self, class_id: u8) -> bool {
        Game::occludes(self, class_id)
    }
    #[inline]
    fn blocked(&self, from: V3, to: V3) -> bool {
        Game::blocked(self, from, to)
    }
}

/// The level's lines of sight, as something that can be asked from several threads at once (see [`Game::blocker`]).
pub trait Blocker: Sync {
    /// Is the level in the way of the straight line from `from` to `to`? (The same question as [`Game::blocked`].)
    fn blocked(&self, from: V3, to: V3) -> bool;
}

/// The eyes the server shares between its threads for one tick: the game's level of sight and which classes hide, worked out once.
pub struct SharedEyes<'a> {
    pub blocker: &'a dyn Blocker,
    pub occludes: [bool; 256],
}

impl Eyes for SharedEyes<'_> {
    #[inline]
    fn occludes(&self, class_id: u8) -> bool {
        self.occludes[class_id as usize]
    }
    #[inline]
    fn blocked(&self, from: V3, to: V3) -> bool {
        self.blocker.blocked(from, to)
    }
}

/// A standing character as the tests see it, in metres above its feet.
const EYE: f32 = 1.6;
const HEAD: f32 = 1.7;
const CHEST: f32 = 1.1;
const FEET: f32 = 0.25;
const SHOULDERS: f32 = 1.4;
/// How far the shoulder points are to each side of the line of sight.
const SIDE: f32 = 0.3;
/// A character with a `low` flag (crouched) is this much of its standing height.
const LOW: f32 = 0.6;
/// A pair where both have moved less than this since its last test keeps the answer.
pub(super) const STILL_M: f32 = 0.3;
/// A pair where either has moved this much since its last test is tested again, however recently that was: the answer was for places they have left (it is what makes a player who has stood still behind a wall
/// for a while and then starts to run be seen to in time).
pub(super) const MOVED_M: f32 = 1.0;
/// A pair where either has changed its velocity by this much (metres a second) in any direction since its last test is tested again too: the answer, and how far ahead it looked, were for another way of moving.
pub(super) const CALM_MS: f32 = 2.0;
/// The same three limits in the integers that places and speeds are kept in (`|difference| < limit`; a test makes sure they are the same): [`STILL_M`] and [`MOVED_M`] in 1/64 m, [`CALM_MS`] in 1/32 m/s.
pub(super) const STILL_Q: u32 = 20;
pub(super) const MOVED_Q: u32 = 64;
pub(super) const CALM_Q: u32 = 64;

/// What the server is told to do about wallhacks (the safe radius, which is also the switch, is the game's: [`Game::occlusion`]).
#[derive(Clone, Debug)]
pub struct Occlusion {
    /// How far ahead (milliseconds) to look: the two are moved on by their own velocity this long (never faster than `max_speed`) and a line that is clear at either place counts.
    pub look_ms: u32,
    /// How long (milliseconds) a player stays told about after it was last in sight.
    pub hold_ms: u32,
    /// The milliseconds between two tests of a pair for the near, mid and far players (the distances of [`Rates`](super::interest::Rates)).
    pub retest_ms: [u32; 3],
    /// The milliseconds between two looks at a pair that has not moved.
    pub still_ms: u32,
    /// The rays a client's packets may cost on average each tick (a client that has not used them can use up to four times as many at once, when it joins or a crowd comes round a corner). The whole server pays
    /// at most this many times its clients, and one client that is among many cannot use up what another needs.
    pub rays_per_client: u32,
    /// The time (milliseconds, averaged over half a second) the server's ticks may take before the rays are cut: above it the rays each client may spend are made fewer in proportion (never below a quarter of
    /// [`Occlusion::rays_per_client`]), and given back a little at a time once the ticks are quick again (see [`Governor`]). 0 (the default): the rays are never cut. A tick is 33 ms at 30 ticks a second, so 24 or 25
    /// leaves the rest of the tick room to spike in.
    pub governor_ms: u32,
}

impl Default for Occlusion {
    fn default() -> Occlusion {
        Occlusion { look_ms: 450, hold_ms: 600, retest_ms: [100, 133, 200], still_ms: 600, rays_per_client: 150, governor_ms: 0 }
    }
}

/// Keeps the cost of the wallhack defence from making a tick late: when the ticks are taking longer than [`Occlusion::governor_ms`] it cuts the rays every client may spend ([`Governor::scale`] is the share left),
/// and when they are quick again it gives them back, slowly. What a client with fewer rays gets is what a client with no rays left gets already (a pair never tested that is near is told, a far one waits, a pair
/// that is known keeps its answer: a short leak of the near ones and a short delay of the far ones, never a player that is not shown), so it is a way of paying for a crowd that is bigger than the server's time with
/// precision, in place of with a tick that is late for everybody. The same inputs always give the same scale: it is a function of the tick times only.
#[derive(Clone, Copy, Debug)]
pub struct Governor {
    /// The average time of a tick (microseconds, over about 15 ticks).
    load_us: f32,
    scale: f32,
}

impl Default for Governor {
    fn default() -> Governor {
        Governor { load_us: 0.0, scale: 1.0 }
    }
}

impl Governor {
    /// The smallest share of the rays a client is left with.
    pub const LEAST: f32 = 0.25;

    /// Takes in the time the tick that has just ended took, and gives the share of its rays a client may spend on the next one (1: all of them). With `limit_ms` 0 it is always 1.
    pub fn update(&mut self, tick_us: u64, limit_ms: u32) -> f32 {
        if limit_ms == 0 {
            *self = Governor::default();
            return 1.0;
        }
        let us = tick_us as f32;
        self.load_us = if self.load_us == 0.0 { us } else { self.load_us + (us - self.load_us) * (1.0 / 15.0) };
        let limit = limit_ms as f32 * 1000.0;
        if self.load_us > limit {
            // (over: cut the share in proportion to how far over, at most a tenth a tick)
            self.scale = (self.scale * (limit / self.load_us).max(0.9)).max(Governor::LEAST);
        } else if self.load_us < limit * 0.85 {
            // (well under: give back three in a hundred of what is left to give each tick, so that it takes about a second to be whole again)
            self.scale = (self.scale * 1.03).min(1.0);
        }
        self.scale
    }

    /// The share of its rays a client may spend now.
    pub fn scale(&self) -> f32 {
        self.scale
    }
}

/// The numbers of one tick, worked out once for all the clients.
#[derive(Clone, Debug, Default)]
pub struct Params {
    pub tick: u32,
    /// The safe radius (metres).
    pub safe: f32,
    pub look_s: f32,
    /// The hold, the three retest periods and the still period, in ticks.
    pub hold: u32,
    pub retest: [u32; 3],
    pub still: u32,
    /// The distances where the near and mid tiers end.
    pub near_m: f32,
    pub mid_m: f32,
    pub max_speed: f32,
    /// The game's `level_version`: an answer kept for another one is asked again.
    pub epoch: u32,
    /// The bits of a player's `flags` that mean it is low (crouched).
    pub low_flags: u16,
    /// The version the level had before its last change and the boxes that change was in ([`Game::level_change`](super::server::Game::level_change)): an answer found at that version stands if the lines of sight of its pair do not go through any of them.
    pub prev_epoch: u32,
    pub regions: Vec<([f32; 3], [f32; 3])>,
}

impl Params {
    #[allow(clippy::too_many_arguments)]
    pub fn new(cfg: &Occlusion, tick: u32, tick_hz: u32, safe: f32, near_m: f32, mid_m: f32, max_speed: f32, epoch: u32, low_flags: u16) -> Params {
        let ticks = |ms: u32| (ms as u64 * tick_hz as u64 + 500) as u32 / 1000;
        Params {
            tick,
            safe,
            look_s: cfg.look_ms as f32 / 1000.0,
            hold: ticks(cfg.hold_ms),
            retest: cfg.retest_ms.map(|ms| ticks(ms).max(1)),
            still: ticks(cfg.still_ms).max(1),
            near_m,
            mid_m,
            max_speed,
            epoch,
            low_flags,
            prev_epoch: 0,
            regions: Vec::new(),
        }
    }
}

/// What a tick of the server's time did for occlusion, added to [`super::server::Stats`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Counters {
    /// Pairs whose lines were tested, and rays cast for them.
    pub pairs: u64,
    pub rays: u64,
    /// Pairs that kept their answer because neither had moved.
    pub still: u64,
    /// Records of players left out of a packet's candidates because they were out of sight.
    pub hidden: u64,
    /// Tests that did not happen because the tick had no rays left.
    pub unaffordable: u64,
}

/// What one observer knows about one target. (Places and speeds are kept as the integers they are told in, 64 to the metre and 32 to the metre a second: a comparison of two of them is exact, and an entry is
/// 56 bytes instead of 68, which matters when 200 observers each keep 200 of them and every one is looked at every tick.)
#[derive(Clone, Copy)]
struct Seen {
    /// The tick up to which the target counts as told about (its last sighting and the hold).
    until: u32,
    /// The tick the pair is looked at again.
    due: u32,
    /// The last tick this entry was used (it is dropped when it has not been for a long time).
    touched: u32,
    epoch: u32,
    /// Where the observer and the target were at the last test, and how fast they were going.
    at: [i32; 6],
    v: [i16; 6],
    /// Whether the last test found a clear line, and whether the target was low (crouched) then: a pair that has not moved but has stood up or crouched is tested again.
    last: bool,
    low: bool,
    /// Whether `at` and `v` are known (an entry made for a pair that is close has not been tested: it has none).
    known: bool,
}

/// `a <= b` for ticks that wrap.
#[inline]
fn le(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) <= 0
}

#[inline]
fn lt(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}

/// What one client has been told about who is in its sight: the answer for each player near it, kept between ticks.
#[derive(Default)]
pub struct Sightings {
    map: IdMap<u32, Seen>,
    calls: u32,
}

impl Sightings {
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Forgets the targets that have not been asked about for four seconds (the player left, or went far away).
    pub fn prune(&mut self, tick: u32) {
        self.calls += 1;
        if self.calls % 32 == 0 {
            self.map.retain(|_, e| tick.wrapping_sub(e.touched) < 120);
        }
    }

    /// Takes out of `cands` (the candidates of the packet of one client, `own` being its character, `flat` where the entities are) the players the client is not to be told about this tick: the near ones are decided first,
    /// then the far ones (so that when the client's rays run out it is the far end that waits). `rays` is what the client may still cast, `n` counts what is done, and `n.hidden` is set to how many went.
    pub fn filter<G: Eyes + ?Sized>(&mut self, game: &G, p: &Params, own: &EntityState, flat: &[EntityState], cands: &mut Vec<Candidate>, rays: &mut u32, n: &mut Counters) {
        self.filter_except(game, p, own, flat, cands, rays, n, &[]);
    }

    /// [`Sightings::filter`] with players that are always told about, wherever they are (the members of the observer's party, docs/PARTIES.md): `except` holds their ids. With none the work is exactly what `filter` does: a client
    /// with no party pays one comparison for the whole list.
    #[allow(clippy::too_many_arguments)]
    pub fn filter_except<G: Eyes + ?Sized>(&mut self, game: &G, p: &Params, own: &EntityState, flat: &[EntityState], cands: &mut Vec<Candidate>, rays: &mut u32, n: &mut Counters, except: &[u32]) {
        let before = cands.len();
        let mid = p.mid_m;
        if except.is_empty() {
            cands.retain(|cand| cand.dist > mid || !game.occludes(cand.class) || self.visible(game, p, own, &flat[cand.idx as usize], cand.dist, rays, n));
            cands.retain(|cand| cand.dist <= mid || !game.occludes(cand.class) || self.visible(game, p, own, &flat[cand.idx as usize], cand.dist, rays, n));
        } else {
            cands.retain(|cand| cand.dist > mid || !game.occludes(cand.class) || except.contains(&cand.id) || self.visible(game, p, own, &flat[cand.idx as usize], cand.dist, rays, n));
            cands.retain(|cand| cand.dist <= mid || !game.occludes(cand.class) || except.contains(&cand.id) || self.visible(game, p, own, &flat[cand.idx as usize], cand.dist, rays, n));
        }
        self.prune(p.tick);
        n.hidden = (before - cands.len()) as u64;
    }

    /// Should the observer be told about `tgt` this tick? `dist` is the planar distance. `budget` is the rays this tick may still cast; `n` counts what is done.
    ///
    /// Most pairs of most ticks are answered here, from what was kept, with no more than a look at the table and some comparisons of whole numbers (all of them made, none ending the others early: a branch that
    /// goes either way for one pair in three costs more than the comparison it saves). What has to be tested, with rays, is [`Sightings::tested`].
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn visible<G: Eyes + ?Sized>(&mut self, game: &G, p: &Params, obs: &EntityState, tgt: &EntityState, dist: f32, budget: &mut u32, n: &mut Counters) -> bool {
        let tick = p.tick;
        if dist <= p.safe {
            // (close: told whatever is in the way; it counts as in sight, so that one who walks away behind a wall is kept for the hold, and the pair is tested the moment it is no longer close)
            let seen = Seen { until: tick.wrapping_add(p.hold), due: tick, touched: tick, epoch: p.epoch, at: [0; 6], v: [0; 6], last: true, low: false, known: false };
            self.map.insert(tgt.id, seen);
            return true;
        }
        if let Some(e) = self.map.get_mut(&tgt.id) {
            e.touched = tick;
            // (the level changed since this answer was found: it stands if the change was somewhere the lines of the pair do not go; the first look at each pair after a change is where that is found out, and the pair then has the answer of the new level)
            if e.epoch != p.epoch && e.epoch == p.prev_epoch && e.known && !p.regions.is_empty() && !lines_meet(p, obs, tgt) {
                e.epoch = p.epoch;
            }
            let low = tgt.class == class::PLAYER && tgt.flags & p.low_flags != 0;
            let (at, v) = (at_of(obs, tgt), v_of(obs, tgt));
            let (mut calm, mut moved) = (0u32, 0u32);
            for a in 0..6 {
                calm = calm.max((v[a] as i32).abs_diff(e.v[a] as i32));
                moved = moved.max(at[a].abs_diff(e.at[a]));
            }
            // (a pair that has not been tested has no places and speeds: it has to be)
            let known = (e.epoch == p.epoch) & e.known & (calm < CALM_Q);
            if known & (moved < MOVED_Q) & lt(tick, e.due) {
                return le(tick, e.until);
            }
            // (neither has moved since the last test: the answer stands without a ray)
            if known & (moved < STILL_Q) & (e.low == low) {
                e.due = tick.wrapping_add(p.still);
                if e.last {
                    e.until = e.due.wrapping_add(p.hold);
                }
                n.still += 1;
                return le(tick, e.until);
            }
        }
        self.tested(game, p, obs, tgt, dist, budget, n)
    }

    /// The pair has to be tested (or it would be, if the client had rays to spend): see if it can be afforded, cast the rays, and keep the answer and where the two were and how fast they went.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn tested<G: Eyes + ?Sized>(&mut self, game: &G, p: &Params, obs: &EntityState, tgt: &EntityState, dist: f32, budget: &mut u32, n: &mut Counters) -> bool {
        let tick = p.tick;
        let id = tgt.id;
        let low = tgt.class == class::PLAYER && tgt.flags & p.low_flags != 0;
        let (at, v) = (at_of(obs, tgt), v_of(obs, tgt));
        let slot = self.map.get_mut(&id);
        // a test costs rays; with none left, the old answer stands, and a pair never tested is told if it is near (a short leak) and not if it is far (a short delay)
        let open = dist <= p.mid_m;
        if *budget == 0 {
            return afford(slot, tick, open, n);
        }
        n.pairs += 1;
        let Some(vis) = sees(game, p, obs, tgt, low, budget, &mut n.rays) else {
            return afford(slot, tick, open, n);
        };
        let tier = if dist <= p.near_m {
            0
        } else if dist <= p.mid_m {
            1
        } else {
            2
        };
        let period = p.retest[tier];
        // (the pairs made on the same tick are put on one of two ticks, so that they are not all tested together for ever)
        let stagger = id.wrapping_mul(0x9E37_79B1) >> 31;
        let due = tick.wrapping_add(period + stagger);
        match slot {
            Some(e) => {
                if vis {
                    // (in sight now, so told until the pair is looked at again, and then for the hold)
                    e.until = due.wrapping_add(p.hold);
                }
                e.last = vis;
                e.low = low;
                e.due = due;
                e.touched = tick;
                e.epoch = p.epoch;
                e.at = at;
                e.v = v;
                e.known = true;
                le(tick, e.until)
            }
            None => {
                let until = if vis { due.wrapping_add(p.hold) } else { tick.wrapping_sub(1) };
                self.map.insert(id, Seen { until, due, touched: tick, epoch: p.epoch, at, v, last: vis, low, known: true });
                vis
            }
        }
    }
}

/// Could a line of sight of this pair go through one of the boxes in `p.regions`? Every one of them runs from the observer's eye, where it is or is going to be, to a point of the target's body, where it is or is going to be
/// ([`sees`]). Such a line is never more than the distance either of them is going to move (the look-ahead) and the width of the shoulders from the line between the middle of the observer and the middle of the target, and
/// never more than 0.85 m above or below it (the eye is 0.6 above the middle line, the feet of a crouched player 0.85 below it): so the pair's lines are in a tube round that line, and what has to be asked is whether the tube meets the box.
pub(super) fn lines_meet(p: &Params, obs: &EntityState, tgt: &EntityState) -> bool {
    let (o, t) = (obs.pos_m(), tgt.pos_m());
    let norm = |v: V3| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let ahead = norm(limited(obs.vel_ms(), p.max_speed).map(|v| v * p.look_s)).max(norm(limited(tgt.vel_ms(), p.max_speed).map(|v| v * p.look_s)));
    let (rh, rv) = (ahead + SIDE + 0.1, ahead + 0.85 + 0.1);
    // (the middle line: from the middle of the observer to the middle of the target, a metre up)
    let (a, d) = ([o[0], o[1] + 1.0, o[2]], [t[0] - o[0], t[1] - o[1], t[2] - o[2]]);
    p.regions.iter().any(|(lo, hi)| {
        // the segment against the box made bigger by the tube's radius (the slab test, the segment being the points a + s d, 0 <= s <= 1)
        let (mut s0, mut s1) = (0.0f32, 1.0f32);
        for k in 0..3 {
            let r = if k == 1 { rv } else { rh };
            let (l, h) = (lo[k] - r, hi[k] + r);
            if d[k].abs() < 1.0e-9 {
                if a[k] < l || a[k] > h {
                    return false;
                }
            } else {
                let (mut u, mut v) = ((l - a[k]) / d[k], (h - a[k]) / d[k]);
                if u > v {
                    std::mem::swap(&mut u, &mut v);
                }
                s0 = s0.max(u);
                s1 = s1.min(v);
                if s0 > s1 {
                    return false;
                }
            }
        }
        true
    })
}

/// Where the observer and the target are, in the integers they are told in.
#[inline(always)]
fn at_of(obs: &EntityState, tgt: &EntityState) -> [i32; 6] {
    [obs.pos[0], obs.pos[1], obs.pos[2], tgt.pos[0], tgt.pos[1], tgt.pos[2]]
}

/// How fast the observer and the target are going, in the integers they are told in.
#[inline(always)]
fn v_of(obs: &EntityState, tgt: &EntityState) -> [i16; 6] {
    [obs.vel[0], obs.vel[1], obs.vel[2], tgt.vel[0], tgt.vel[1], tgt.vel[2]]
}

/// No rays are left (or the test ran out of them): the answer that was found stands, and the pair is tried again on the next tick; a pair never tested is told if it is near and not if it is far.
fn afford(slot: Option<&mut Seen>, tick: u32, open: bool, n: &mut Counters) -> bool {
    n.unaffordable += 1;
    match slot {
        Some(e) => {
            e.due = tick.wrapping_add(1);
            le(tick, e.until)
        }
        None => open,
    }
}

fn limited(v: V3, max: f32) -> V3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l.is_finite() && l <= max {
        v
    } else if l.is_finite() && l > 0.0 {
        [v[0] * max / l, v[1] * max / l, v[2] * max / l]
    } else {
        [0.0; 3]
    }
}

fn moves(d: V3) -> bool {
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > 0.09
}

/// Is there a clear line from the observer's eye to any of the target's points, now or `look` ahead? `None` when the rays ran out first.
pub(super) fn sees<G: Eyes + ?Sized>(game: &G, p: &Params, obs: &EntityState, tgt: &EntityState, low: bool, budget: &mut u32, rays: &mut u64) -> Option<bool> {
    let (o, t) = (obs.pos_m(), tgt.pos_m());
    let od = limited(obs.vel_ms(), p.max_speed).map(|v| v * p.look_s);
    let td = limited(tgt.vel_ms(), p.max_speed).map(|v| v * p.look_s);
    let eye = [o[0], o[1] + EYE, o[2]];
    let eyes = [Some(eye), moves(od).then(|| [eye[0] + od[0], eye[1] + od[1], eye[2] + od[2]])];
    let places = [Some(t), moves(td).then(|| [t[0] + td[0], t[1] + td[1], t[2] + td[2]])];
    let k = if low { LOW } else { 1.0 };
    for (i, place) in places.iter().flatten().enumerate() {
        // (the shoulders are to either side of the line from the observer: a head and a shoulder past the edge of a wall are seen where the middle of the body is not)
        let (dx, dz) = (place[0] - o[0], place[2] - o[2]);
        let l = (dx * dx + dz * dz).sqrt().max(1.0e-3);
        let side = [-dz / l * SIDE, dx / l * SIDE];
        let at = |h: f32, s: f32| [place[0] + side[0] * s, place[1] + h * k, place[2] + side[1] * s];
        let points = [at(CHEST, 0.0), at(HEAD, 0.0), at(FEET, 0.0), at(SHOULDERS, 1.0), at(SHOULDERS, -1.0)];
        // (a place the target is going to be at is looked at with fewer points: it is a guess)
        let count = if i == 0 { 5 } else { 3 };
        for point in points.iter().take(count) {
            for from in eyes.iter().flatten() {
                if *budget == 0 {
                    return None;
                }
                *budget -= 1;
                *rays += 1;
                if !game.blocked(*from, *point) {
                    return Some(true);
                }
            }
        }
    }
    Some(false)
}
