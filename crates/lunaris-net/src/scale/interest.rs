//! Who needs to know about what: a grid of the world's entities, and the rate at which each one is told about each.
//!
//! A client is sent the entities within `radius` of its own character (a grid of square cells finds them without looking at the rest of the match), and each one at a rate that falls with
//! distance and depends on what it is: a player next to you every tick, one across the street ten times a second, one at the edge three times a second. Every tick each candidate earns
//! its rate (as a share of a tick) in an account; the ones whose account has reached a whole send are put in the packet, the fullest first (an account counts for more the nearer the
//! entity is), until the packet's budget is used; a send takes one off the account. So a crowded place never makes a packet bigger than its budget, the nearest go first and the stalest of
//! each distance after them.
//!
//! Things that do not move (loot on the ground, a door) need no accounts: they are cut into one grid for each tick of their period (30 grids for once a second at 30 ticks a second), and a
//! client is offered the grid of the tick it is on, so each one is offered exactly once a period and a client looks at a thirtieth of them each tick ([`Statics`]).

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use super::state::class;

/// A hasher for keys that are small integers (entity ids, cells): one multiplication a word, where the standard library's pays for a defence against attackers that these tables do not
/// need (the keys are the server's own).
#[derive(Default, Clone, Copy)]
pub struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u32(b as u32);
        }
    }

    fn write_u32(&mut self, v: u32) {
        self.0 = (self.0.rotate_left(5) ^ v as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

pub type IdMap<K, V> = HashMap<K, V, BuildHasherDefault<IdHasher>>;

/// Whether an entity of this class stays where it is and is sent on a slow, fixed schedule.
pub fn is_static(class_id: u8) -> bool {
    class_id == class::ITEM || class_id == class::WORLD
}

/// A grid of cells of `cell` metres; entities are found by the cell they are in (the ground plane: x and z).
pub struct Grid {
    cell: f32,
    cells: IdMap<(i32, i32), Vec<u32>>,
}

impl Grid {
    pub fn new(cell: f32) -> Grid {
        Grid { cell: cell.max(1.0), cells: IdMap::default() }
    }

    pub fn clear(&mut self) {
        for v in self.cells.values_mut() {
            v.clear();
        }
    }

    fn key(&self, x: f32, z: f32) -> (i32, i32) {
        ((x / self.cell).floor() as i32, (z / self.cell).floor() as i32)
    }

    pub fn insert(&mut self, id: u32, x: f32, z: f32) {
        let k = self.key(x, z);
        self.cells.entry(k).or_default().push(id);
    }

    /// Drops the cells with nothing in them (a match whose players have moved on leaves a long tail of empty cells).
    pub fn prune(&mut self) {
        self.cells.retain(|_, v| !v.is_empty());
    }

    /// Calls `f` with every entity in the cells that touch the circle of `radius` round (x, z): a superset of the ones inside it.
    pub fn query(&self, x: f32, z: f32, radius: f32, mut f: impl FnMut(u32)) {
        let (x0, z0) = self.key(x - radius, z - radius);
        let (x1, z1) = self.key(x + radius, z + radius);
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                if let Some(v) = self.cells.get(&(cx, cz)) {
                    for &id in v {
                        f(id);
                    }
                }
            }
        }
    }
}

/// Things that do not move, in as many grids as ticks of their period: the one `id % period` holds this one, and a client is offered the grid of the tick it is on.
pub struct Statics {
    grids: Vec<Grid>,
}

impl Statics {
    pub fn new(cell: f32, period: usize) -> Statics {
        Statics { grids: (0..period.max(1)).map(|_| Grid::new(cell)).collect() }
    }

    pub fn clear(&mut self) {
        for g in self.grids.iter_mut() {
            g.clear();
        }
    }

    pub fn prune(&mut self) {
        for g in self.grids.iter_mut() {
            g.prune();
        }
    }

    /// Puts the thing with entity `id`, kept by `value`, at (x, z).
    pub fn insert(&mut self, id: u32, value: u32, x: f32, z: f32) {
        let n = self.grids.len();
        self.grids[id as usize % n].insert(value, x, z);
    }

    /// The things to offer on `tick` within `radius` of (x, z).
    pub fn query(&self, tick: u32, x: f32, z: f32, radius: f32, f: impl FnMut(u32)) {
        self.grids[tick as usize % self.grids.len()].query(x, z, radius, f);
    }
}

/// How often an entity of a class at a distance is sent, in sends a second.
#[derive(Clone, Copy, Debug)]
pub struct Rates {
    pub near: f32,
    pub mid: f32,
    pub far: f32,
    /// A thing on the ground or in the world.
    pub still: f32,
    /// The distances (metres) where near ends and mid ends.
    pub near_m: f32,
    pub mid_m: f32,
    /// However close they are, only this many of the nearest are sent at the near rate, and this many at most at the near or mid rate: what one client is sent a second is bounded
    /// (20 x 30 + 36 x 10 + the rest x 3 sends is about what a packet holds), so a hundred players in one square get the ones that matter the most at full rate and the rest less often,
    /// where every one of them would otherwise get a seventh of what it needs.
    pub near_n: usize,
    pub mid_n: usize,
}

impl Default for Rates {
    fn default() -> Rates {
        Rates { near: 30.0, mid: 10.0, far: 3.0, still: 1.0, near_m: 35.0, mid_m: 80.0, near_n: 20, mid_n: 56 }
    }
}

impl Rates {
    /// The rate (sends a second) for an entity of `class` at `dist` metres.
    pub fn of(&self, class_id: u8, dist: f32) -> f32 {
        match class_id {
            class::ITEM | class::WORLD => self.still,
            class::PROJECTILE => {
                if dist <= self.mid_m {
                    self.near
                } else {
                    self.mid
                }
            }
            _ => {
                if dist <= self.near_m {
                    self.near
                } else if dist <= self.mid_m {
                    self.mid
                } else {
                    self.far
                }
            }
        }
    }

    /// The distances within which at most `near_n` and at most `mid_n` of the `dists` are (the distances of the candidates that count for the limits; reordered): a candidate farther than
    /// the first is not sent faster than the mid rate, farther than the second not faster than the far rate.
    pub fn cuts(&self, dists: &mut [f32]) -> (f32, f32) {
        let n = dists.len();
        let mid_n = self.mid_n.max(1);
        let near_n = self.near_n.clamp(1, mid_n);
        let mid_cut = if n > mid_n { *dists.select_nth_unstable_by(mid_n - 1, f32::total_cmp).1 } else { f32::INFINITY };
        let near_cut = if n > near_n {
            let part = &mut dists[..n.min(mid_n)];
            *part.select_nth_unstable_by(near_n - 1, f32::total_cmp).1
        } else {
            f32::INFINITY
        };
        (near_cut, mid_cut)
    }

    /// The rate of an entity of `class` at `dist`, with the limits on how many are sent fast ([`Rates::cuts`]).
    pub fn limited(&self, class_id: u8, dist: f32, cuts: (f32, f32)) -> f32 {
        let rate = self.of(class_id, dist);
        if class_id == class::PROJECTILE || is_static(class_id) {
            rate
        } else if dist > cuts.1 {
            rate.min(self.far)
        } else if dist > cuts.0 {
            rate.min(self.mid)
        } else {
            rate
        }
    }

    /// How much an account counts for when a packet cannot hold everything that is due: the faster an entity is sent, the more it matters that it is not late.
    fn weight(&self, rate: f32) -> f32 {
        if rate >= self.near {
            3.0
        } else if rate >= self.mid {
            2.0
        } else {
            1.0
        }
    }

    /// Whether an entity at `dist` is sent with all its fields (near and mid) or only the basic ones.
    pub fn full(&self, class_id: u8, dist: f32) -> bool {
        class_id != class::ITEM && class_id != class::WORLD && dist <= self.mid_m
    }

    /// The ticks between two offers of a thing that does not move.
    pub fn static_period(&self, tick_hz: u32) -> usize {
        ((tick_hz as f32 / self.still.max(0.05)).round() as usize).max(1)
    }
}

/// One candidate for a packet.
pub struct Candidate {
    /// Where the server keeps the entity's state this tick.
    pub idx: u32,
    pub id: u32,
    pub dist: f32,
    pub class: u8,
}

/// The accounts of one client: what each entity has earned and not yet been sent for.
#[derive(Default)]
pub struct Accounts {
    // (earned, the last tick it was looked at)
    earned: IdMap<u32, (f32, u32)>,
    calls: u32,
}

impl Accounts {
    /// Gives each candidate its earnings for the ticks since the last time this was done (`ticks` is that tick, exclusive, and now, inclusive; `tick_hz` ticks a second) and puts the ones due
    /// in `due` as (priority, index in `cands`), the first to send first (a candidate never sent before has the biggest account: it is new). An entity that has not been looked at for
    /// a while loses its account, so that one that comes back is new again. Returns the fastest rate any candidate has (a client with none near needs a packet less often).
    ///
    /// The slowest tier (the far players) is not looked at on every tick: its account is only touched on its own tick of every period (`id` decides which), when it earns a whole send. Most of a
    /// crowd is that tier, so most of the candidates cost one addition and a comparison.
    pub fn earn(&mut self, cands: &[Candidate], rates: &Rates, tick_hz: f32, cuts: (f32, f32), ticks: (u32, u32), due: &mut Vec<(f32, usize)>) -> f32 {
        due.clear();
        self.calls += 1;
        let (from, to) = ticks;
        let dt = to.saturating_sub(from).clamp(1, 4) as f32;
        let far_period = ((tick_hz / rates.far.max(0.1)).round() as u32).max(1);
        let mut fastest = 0.0f32;
        for (i, c) in cands.iter().enumerate() {
            let rate = rates.limited(c.class, c.dist, cuts);
            fastest = fastest.max(rate);
            let slow = rate <= rates.far;
            let earns = if slow {
                // (this entity's tick of the period came in the ticks since the last look)
                if (to.wrapping_add(c.id)) / far_period == (from.wrapping_add(c.id)) / far_period {
                    continue;
                }
                1.0
            } else {
                rate / tick_hz * dt
            };
            let acc = match self.earned.get_mut(&c.id) {
                Some(e) => {
                    e.0 += earns;
                    e.1 = to;
                    e.0
                }
                None => {
                    self.earned.insert(c.id, (10.0, to));
                    10.0
                }
            };
            if acc >= 1.0 {
                // (nearer ones first among equals: they are the ones whose staleness shows)
                due.push((acc * rates.weight(rate) - c.dist * 0.001, i));
            }
        }
        due.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
        if self.calls % 16 == 0 {
            // (the slow tier is looked at once in a period of about ten ticks: forty is long enough to keep it)
            self.earned.retain(|_, e| to.wrapping_sub(e.1) <= 40);
        }
        fastest
    }

    /// Notes that `id` has been sent: its account pays for it (and keeps a little if it was long overdue, but never more than one send's worth).
    pub fn paid(&mut self, id: u32) {
        if let Some(e) = self.earned.get_mut(&id) {
            e.0 = (e.0 - 1.0).clamp(0.0, 0.5);
        }
    }
}
