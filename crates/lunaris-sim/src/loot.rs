//! Loot tables (docs/LOOT.md): what is in a chest, a cabinet or a corpse, rolled from a text table, the same every time for the same container, nested (a rare find inside a common table), refilled
//! after it has been looted. It fills the engine's own `Inventory` through its `fill` request, so everything the inventory does (stacks, weight, publishing) applies. It makes no item and no picture:
//! the items are the game's `ItemTable`, their models and icons the game's.
//!
//! Deterministic: every roll is a hash of (container key, number of fills, roll number), never a generator with state.
use crate::components::{HitPoints, LootContainer, LootDrop, LootTable, Transform};
use crate::inventory::{table_of, Inventory};
use crate::rng::hash_rand;
use lunaris_ecs::{Entity, Sym, World};
use lunaris_json::Json;
use crate::detmath;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

type Events = Vec<(String, u64, Option<[f32; 3]>)>;

const SEED: u64 = 0x100_7_AB1E_C0FFEE;
/// How deep a table may name another table (and the longest chain of `@` the roller follows).
const MAX_DEPTH: u32 = 6;
/// The most items a single fill may put in a container (a mistaken table must not make a million).
const MAX_ITEMS: u32 = 500;

#[derive(Clone, Debug, PartialEq)]
pub enum What {
    Item(String),
    Table(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub what: What,
    pub weight: f32,
    pub min: u32,
    pub max: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub rolls: (u32, u32),
    pub empty: f32,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loot {
    pub tables: BTreeMap<String, Table>,
    /// What was wrong with the text (for `check`): unknown lines, unknown items, unknown tables, loops.
    pub problems: Vec<String>,
}

fn range(text: &str) -> Option<(u32, u32)> {
    let (a, b) = text.split_once('-').unwrap_or((text, text));
    let (a, b) = (a.trim().parse::<u32>().ok()?, b.trim().parse::<u32>().ok()?);
    Some((a.min(b), a.max(b)))
}

/// Read the text of a `LootTable`. `items` is the item table of the world (to say which keys do not exist), when there is one.
pub fn parse_loot(text: &str, items: Option<&crate::inventory::Table>) -> Loot {
    let mut loot = Loot::default();
    let mut current: Option<String> = None;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        let first = words.next().unwrap_or("");
        if first == "table" {
            let Some(name) = words.next() else {
                loot.problems.push(format!("line {}: `table` needs a name", n + 1));
                current = None;
                continue;
            };
            let mut t = Table { rolls: (1, 1), empty: 0.0, entries: Vec::new() };
            for w in words {
                match w.split_once('=') {
                    Some(("rolls", v)) => match range(v) {
                        Some(r) => t.rolls = r,
                        None => loot.problems.push(format!("line {}: rolls={v} is not a number or a range like 2-4", n + 1)),
                    },
                    Some(("empty", v)) => match v.parse::<f32>() {
                        Ok(p) if (0.0..=1.0).contains(&p) => t.empty = p,
                        _ => loot.problems.push(format!("line {}: empty={v} is not a chance from 0 to 1", n + 1)),
                    },
                    _ => loot.problems.push(format!("line {}: `{w}` is not rolls=A-B or empty=P", n + 1)),
                }
            }
            if loot.tables.insert(name.to_string(), t).is_some() {
                loot.problems.push(format!("line {}: the table `{name}` is defined twice (the later one counts)", n + 1));
            }
            current = Some(name.to_string());
            continue;
        }
        let Some(table) = current.as_ref().and_then(|c| loot.tables.get_mut(c)) else {
            loot.problems.push(format!("line {}: `{first}` is an entry outside a table (start one with `table NAME`)", n + 1));
            continue;
        };
        let what = match first.strip_prefix('@') {
            Some(other) => What::Table(other.to_string()),
            None => What::Item(first.to_string()),
        };
        let (mut weight, mut min, mut max) = (1.0f32, 1u32, 1u32);
        for w in words {
            match w.split_once('=') {
                Some(("w", v)) => match v.parse::<f32>() {
                    Ok(x) if x >= 0.0 && x.is_finite() => weight = x,
                    _ => loot.problems.push(format!("line {}: w={v} is not a weight (0 or more)", n + 1)),
                },
                Some(("n", v)) => match range(v) {
                    Some((a, b)) => {
                        min = a;
                        max = b;
                    }
                    None => loot.problems.push(format!("line {}: n={v} is not a number or a range like 1-3", n + 1)),
                },
                _ => loot.problems.push(format!("line {}: `{w}` is not w=WEIGHT or n=A-B", n + 1)),
            }
        }
        table.entries.push(Entry { what, weight, min, max });
    }
    // what refers to what does not exist
    let names: Vec<String> = loot.tables.keys().cloned().collect();
    for (name, t) in &loot.tables {
        if t.entries.is_empty() {
            loot.problems.push(format!("the table `{name}` has no entries"));
        }
        for e in &t.entries {
            match &e.what {
                What::Item(k) => {
                    if let Some(items) = items {
                        if items.id_of(k) == 0 {
                            loot.problems.push(format!("the table `{name}` has the item `{k}`, which is not in the ItemTable"));
                        }
                    }
                }
                What::Table(o) => {
                    if !names.contains(o) {
                        loot.problems.push(format!("the table `{name}` rolls `@{o}`, which is not defined"));
                    }
                }
            }
        }
    }
    loot
}

thread_local! {
    /// The text of the loot tables last read and what it parsed to (found again by comparing the text, which is cheaper than hashing it).
    static CACHE: RefCell<Option<(String, Rc<Loot>)>> = const { RefCell::new(None) };
}

/// The parsed loot tables of the world, or None when it has no `LootTable`.
pub fn loot_of(world: &mut World) -> Option<Rc<Loot>> {
    let mut best: Option<(u64, LootTable)> = None;
    let _ = world.for_each_uid::<(&LootTable,), _>(|_, uid, (t,)| {
        if best.as_ref().is_none_or(|b| uid < b.0) {
            best = Some((uid, *t));
        }
    });
    let (_, lt) = best?;
    let items = table_of(world).map(|(t, _)| t);
    let text = world.strings().get(lt.text.0);
    Some(CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((k, l)) = c.as_ref() {
            if k == text {
                return l.clone();
            }
        }
        let l = Rc::new(parse_loot(text, items.as_deref()));
        *c = Some((text.to_string(), l.clone()));
        l
    }))
}

struct Roller<'a> {
    loot: &'a Loot,
    key: u64,
    fill: u64,
    counter: u64,
    total: u32,
    /// Calls and rolls left to make: a table that rolls itself (or one that rolls many tables that roll many) ends when this is spent, not when the server's tick is long over.
    budget: u32,
    out: BTreeMap<String, u32>,
}

/// Calls of `Roller::roll` and rolls a single filling may make.
const MAX_BUDGET: u32 = 20_000;

impl Roller<'_> {
    fn next(&mut self) -> f32 {
        self.counter += 1;
        hash_rand(SEED, self.fill, self.key, self.counter)
    }

    fn between(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + ((self.next() * (hi as u64 - lo as u64 + 1) as f32) as u32).min(hi - lo)
    }

    fn roll(&mut self, name: &str, depth: u32) {
        let Some(t) = self.loot.tables.get(name) else { return };
        if depth > MAX_DEPTH || self.budget == 0 {
            return;
        }
        self.budget -= 1;
        let rolls = self.between(t.rolls.0, t.rolls.1);
        let weight_sum: f32 = t.entries.iter().map(|e| e.weight).sum();
        for _ in 0..rolls {
            if self.total >= MAX_ITEMS || weight_sum <= 0.0 || self.budget == 0 {
                return;
            }
            self.budget -= 1;
            if self.next() < t.empty {
                continue;
            }
            let mut pick = self.next() * weight_sum;
            for e in &t.entries {
                if pick < e.weight || std::ptr::eq(e, t.entries.last().unwrap()) {
                    match &e.what {
                        What::Item(k) => {
                            let n = self.between(e.min, e.max).min(MAX_ITEMS - self.total);
                            if n > 0 {
                                *self.out.entry(k.clone()).or_default() += n;
                                self.total += n;
                            }
                        }
                        What::Table(o) => {
                            let o = o.clone();
                            // (a table entry's count is how many times that table is rolled)
                            let times = self.between(e.min, e.max);
                            for _ in 0..times {
                                if self.budget == 0 {
                                    break;
                                }
                                self.roll(&o, depth + 1);
                            }
                        }
                    }
                    break;
                }
                pick -= e.weight;
            }
        }
    }
}

/// What the table `name` gives for the container `key` on its `fill`th filling, as (item key, count) in key order.
pub fn roll(loot: &Loot, name: &str, key: u64, fill: u64) -> Vec<(String, u32)> {
    let mut r = Roller { loot, key, fill, counter: 0, total: 0, budget: MAX_BUDGET, out: BTreeMap::new() };
    r.roll(name, 0);
    r.out.into_iter().collect()
}

pub fn update(world: &mut World, dt: f32, events: &mut Events) {
    let mut list: Vec<(Entity, u64, LootContainer)> = Vec::new();
    let _ = world.for_each_uid::<(&LootContainer, &Inventory), _>(|e, uid, (c, _)| list.push((e, uid, *c)));
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.1);
    let Some(loot) = loot_of(world) else { return };
    for (e, uid, mut c) in list {
        let mut fill_now = !c.filled;
        if c.filled && c.refill > 0.0 {
            c.timer = (c.timer - dt).max(0.0);
            if c.timer <= 0.0 {
                // (a container is filled again once it has been emptied)
                let empty = world.get::<Inventory>(e).is_some_and(|i| (0..(i.size as usize).min(48)).all(|k| i.id[k] == 0 || i.count[k] == 0));
                fill_now = empty;
            }
        }
        if fill_now {
            let name = world.strings().get(c.table.0).to_string();
            let key = if c.key != 0 { c.key as u64 } else { uid };
            let items = roll(&loot, &name, key, c.fills as u64);
            if !items.is_empty() {
                let text = items.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ");
                let sym = world.strings_mut().intern(&text);
                if let Some(inv) = world.get_mut::<Inventory>(e) {
                    inv.fill = Sym(sym);
                }
            }
            c.filled = true;
            c.fills = c.fills.wrapping_add(1);
            c.timer = c.refill.max(0.0);
            events.push(("loot_filled".to_string(), uid, None));
        }
        if let Some(w) = world.get_mut::<LootContainer>(e) {
            *w = c;
        }
    }
}

/// Things with a `LootDrop` whose `HitPoints` have run out drop their loot, once (docs/LOOT.md, "Drops").
pub fn drops(world: &mut World, scripts: &mut crate::ScriptSet, fields: &crate::terrain::Terrains, meshes: &crate::meshcol::MeshStore, events: &mut Events) {
    let mut list: Vec<(Entity, u64, LootDrop, [f32; 3])> = Vec::new();
    let _ = world.for_each_uid::<(&LootDrop, &HitPoints, &Transform), _>(|e, uid, (d, h, t)| {
        if h.dead && !d.done {
            list.push((e, uid, *d, t.pos));
        }
    });
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.1);
    let loot = loot_of(world);
    for (e, uid, d, pos) in list {
        // (once, whatever comes of it)
        if let Some(w) = world.get_mut::<LootDrop>(e) {
            w.done = true;
        }
        let Some(loot) = &loot else { continue };
        let (table, prefab) = (world.strings().get(d.table.0).to_string(), world.strings().get(d.prefab.0).to_string());
        let key = if d.key != 0 { d.key as u64 } else { uid };
        let items = roll(loot, &table, key, 0);
        let n = items.len().max(1);
        // (the floor under the ring: a machine that dies in the air leaves its loot on the ground)
        let floor = if d.ground && !items.is_empty() {
            let reach = d.spread.abs() + 2.0;
            let rw = crate::vehicle_ray::RayWorld::gather(world, fields, meshes, Some(([pos[0] - reach, pos[1] - 500.0, pos[2] - reach], [pos[0] + reach, pos[1] + 1.0, pos[2] + reach])));
            let who = crate::layers::Layers::gather(world).of(uid);
            Some((rw, who))
        } else {
            None
        };
        for (k, (item, count)) in items.iter().enumerate() {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            let dir = [detmath::cos(a) as f32, 0.0, detmath::sin(a) as f32];
            // (a wall between the machine and its ring: the pickup lies in front of the wall, not in it)
            let out = floor.as_ref().and_then(|(rw, who)| rw.cast([pos[0], pos[1] + 0.5, pos[2]], dir, d.spread.abs(), *who, uid)).map_or(d.spread.abs(), |h| (h.t - 0.3).clamp(0.0, d.spread.abs()));
            let (x, z) = (pos[0] + dir[0] * out, pos[2] + dir[2] * out);
            let ground_y = floor.as_ref().and_then(|(rw, who)| rw.cast([x, pos[1] + 0.5, z], [0.0, -1.0, 0.0], 500.0, *who, uid)).map_or(pos[1], |h| pos[1] + 0.5 - h.t);
            let at = [x, ground_y + d.lift, z];
            let more = [("Interactable", Json::obj([("item", Json::str(&format!("{item} {count}")))]))];
            crate::building::spawn_prefab_rot(world, scripts, &prefab, at, [0.0, 0.0, 0.0, 1.0], &more);
        }
        events.push(("loot_dropped".to_string(), uid, Some(pos)));
    }
}
