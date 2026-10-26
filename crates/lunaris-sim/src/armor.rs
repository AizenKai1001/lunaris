//! Worn gear (docs/EQUIPMENT.md): a character's bag has places that take only one kind of item and what is in them protects the character where it is hit.
//!
//! An `Equipment` on a character with an `Inventory` says which slots of the bag are worn places: `slot INDEX accepts=CLASS zones=body,head,legs` (one line a place; `#` starts a comment). The server
//! refuses to put an item whose `class` is not CLASS into the slot (the game's interface also filters what is dragged there, `UiDrop.accept`), and the item in it protects the zones: its `power` is the
//! share of a hit that it stops, in percent (0 to 95), its `dur` how much damage it can stop before it is worn out. A hit of the weapons system (`weapons.rs`) goes through [`absorb`], which takes
//! off what the worn pieces of the zone that was hit stop (several pieces on one zone multiply: two of 50 percent stop 75), wears them by what they stopped (`Equipment.wear` a point of durability
//! for each point stopped) and breaks a piece that is used up (the event `item_break`). Nothing else changes: a character with no `Equipment` takes its hits whole, and hunger, falls and fire are not hits.
//!
//! The zones are the weapons system's: 1 the body, 2 the head, 3 the legs and arms. `Equipment.cover` (kept by [`update`] every tick, on a client as well, so that an interface can show it) is the share
//! of a hit that is stopped in each zone now.
//!
//! Deterministic: nothing here is random and the pieces are visited in the order of the text.
use crate::components::Equipment;
use crate::inventory::{table_of, Bag, Inventory, Table};
use lunaris_ecs::{Entity, World};
use std::cell::RefCell;
use std::rc::Rc;

type Events = Vec<(String, u64, Option<[f32; 3]>)>;

/// The zones a hit can land in (the weapons system's numbers): 0 none, 1 body, 2 head, 3 legs and arms.
pub const ZONES: usize = 4;

/// The most that worn gear can stop of one hit, as a share (a piece of 100 percent counts as 95).
const MOST: f32 = 0.95;

/// One worn place of the bag.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// The slot of the bag.
    pub slot: usize,
    /// The `class` of the items it takes (0: any item).
    pub accepts: u32,
    /// Bit `z` is set when what is worn here protects zone `z`.
    pub zones: u8,
}

/// Read the text of an `Equipment`: one place a line, `slot INDEX accepts=CLASS zones=body,head,legs`. Lines that are not understood are skipped and listed.
pub fn parse_places(text: &str) -> (Vec<Place>, Vec<String>) {
    let mut places: Vec<Place> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        if words.next() != Some("slot") {
            problems.push(format!("line {}: a line is `slot INDEX accepts=CLASS zones=body,head,legs`, not `{line}`", n + 1));
            continue;
        }
        let Some(slot) = words.next().and_then(|w| w.parse::<usize>().ok()).filter(|s| *s < crate::inventory::MAX_SLOTS) else {
            problems.push(format!("line {}: `slot` needs the number of a slot of the bag (0 to {})", n + 1, crate::inventory::MAX_SLOTS - 1));
            continue;
        };
        let mut p = Place { slot, accepts: 0, zones: 0 };
        for w in words {
            let Some((k, v)) = w.split_once('=') else {
                problems.push(format!("line {}: `{w}` is not key=value", n + 1));
                continue;
            };
            match k {
                "accepts" => match v.parse::<u32>() {
                    Ok(c) => p.accepts = c,
                    Err(_) => problems.push(format!("line {}: accepts needs a number, not `{v}`", n + 1)),
                },
                "zones" => {
                    for z in v.split(',') {
                        match z.trim() {
                            "body" => p.zones |= 1 << 1,
                            "head" => p.zones |= 1 << 2,
                            "legs" | "limbs" => p.zones |= 1 << 3,
                            "" => {}
                            other => problems.push(format!("line {}: unknown zone `{other}` (body, head, legs)", n + 1)),
                        }
                    }
                }
                _ => problems.push(format!("line {}: unknown key `{k}`", n + 1)),
            }
        }
        if places.iter().any(|x| x.slot == slot) {
            problems.push(format!("line {}: slot {slot} is already a worn place", n + 1));
            continue;
        }
        places.push(p);
    }
    (places, problems)
}

thread_local! {
    /// The text of the places last read and what it parsed to (every character with gear asks on every tick, and every hit does).
    static CACHE: RefCell<Option<(String, Rc<Vec<Place>>)>> = const { RefCell::new(None) };
}

fn places_of(text: &str) -> Rc<Vec<Place>> {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((k, p)) = c.as_ref() {
            if k == text {
                return p.clone();
            }
        }
        let p = Rc::new(parse_places(text).0);
        *c = Some((text.to_string(), p.clone()));
        p
    })
}

/// The worn places of an entity (none when it has no `Equipment`).
pub fn places(world: &World, e: Entity) -> Rc<Vec<Place>> {
    match world.get::<Equipment>(e) {
        Some(q) => places_of(world.strings().get(q.slots.0)),
        None => Rc::new(Vec::new()),
    }
}

/// May the item `item` (a number of the table) go into slot `slot` of the bag of `e`? Yes for every slot that is not a worn place; a worn place takes the class it accepts (and always takes nothing:
/// emptying a place is a move too).
pub fn allows(world: &World, e: Entity, table: &Table, slot: usize, item: u32) -> bool {
    if item == 0 {
        return true;
    }
    let all = places(world, e);
    match all.iter().find(|p| p.slot == slot) {
        None => true,
        Some(p) => p.accepts == 0 || table.item(item).is_some_and(|i| i.class == p.accepts),
    }
}

/// The share of a hit that the worn pieces stop, by zone.
fn cover_of(table: &Table, inv: &Inventory, places: &[Place]) -> [f32; ZONES] {
    let mut keep = [1.0f32; ZONES];
    for p in places {
        if p.slot >= inv.size as usize || inv.id[p.slot] == 0 || inv.count[p.slot] == 0 {
            continue;
        }
        let Some(item) = table.item(inv.id[p.slot]) else { continue };
        let stops = (item.power / 100.0).clamp(0.0, MOST);
        for (z, k) in keep.iter_mut().enumerate().skip(1) {
            if p.zones & (1 << z) != 0 {
                *k *= 1.0 - stops;
            }
        }
    }
    [0.0, 1.0 - keep[1], 1.0 - keep[2], 1.0 - keep[3]]
}

/// Keep `Equipment.cover` up to date (every tick; a client does it too, from the bag the server told it).
pub fn update(world: &mut World) {
    let mut found: Vec<(Entity, Equipment)> = Vec::new();
    let _ = world.for_each::<(&Equipment,), _>(|e, (q,)| found.push((e, *q)));
    if found.is_empty() {
        return;
    }
    let Some((table, _)) = table_of(world) else { return };
    for (e, q) in found {
        let Some(inv) = world.get::<Inventory>(e).copied() else { continue };
        let p = places_of(world.strings().get(q.slots.0));
        let cover = cover_of(&table, &inv, &p);
        if cover != q.cover {
            if let Some(w) = world.get_mut::<Equipment>(e) {
                w.cover = cover;
            }
        }
    }
}

/// A hit of `damage` on the character `victim`, in `zone`: what is left of it after the worn gear has stopped its share. The pieces that stopped something are worn by it and a piece that is used
/// up is broken (the event `item_break` on the victim). Returns `damage` as it was when the victim wears nothing for that zone.
pub fn absorb(world: &mut World, victim: u64, damage: f32, zone: u32, events: &mut Events) -> f32 {
    absorb_piercing(world, victim, damage, zone, 0.0, events)
}

/// `absorb` for a hit that ignores part of the armor: `pierce` (0 to 1) is the share of each piece's stopping that does not count (a round with 1 goes through every piece; a piece that stops nothing wears nothing).
pub fn absorb_piercing(world: &mut World, victim: u64, damage: f32, zone: u32, pierce: f32, events: &mut Events) -> f32 {
    let pierce = if pierce.is_finite() { pierce.clamp(0.0, 1.0) } else { 0.0 };
    if !(damage > 0.0) || !(1..ZONES as u32).contains(&zone) {
        return damage;
    }
    let Some(e) = world.entity(victim) else { return damage };
    let Some(q) = world.get::<Equipment>(e).copied() else { return damage };
    let Some(inv) = world.get::<Inventory>(e).copied() else { return damage };
    let Some((table, _)) = table_of(world) else { return damage };
    let places = places_of(world.strings().get(q.slots.0));
    // the pieces that cover this zone, and what each stops
    let mut pieces: Vec<(usize, f32)> = Vec::new();
    let mut keep = 1.0f32;
    for p in places.iter() {
        if p.zones & (1 << zone) == 0 || p.slot >= inv.size as usize || inv.id[p.slot] == 0 || inv.count[p.slot] == 0 {
            continue;
        }
        let Some(item) = table.item(inv.id[p.slot]) else { continue };
        let stops = (item.power / 100.0).clamp(0.0, MOST) * (1.0 - pierce);
        if stops > 0.0 {
            pieces.push((p.slot, stops));
            keep *= 1.0 - stops;
        }
    }
    if pieces.is_empty() {
        return damage;
    }
    let stopped = damage * (1.0 - keep);
    // wear: each piece loses durability by its share of what was stopped (a piece with no durability, `dur` 0 in the table, never wears)
    let total: f32 = pieces.iter().map(|p| p.1).sum();
    let mut bag = Bag::of(&inv);
    let mut broke: Vec<usize> = Vec::new();
    if q.wear > 0.0 {
        for (slot, stops) in &pieces {
            if bag.dur[*slot] > 0.0 {
                bag.dur[*slot] -= stopped * (stops / total) * q.wear;
                if bag.dur[*slot] <= 0.0 {
                    broke.push(*slot);
                }
            }
        }
    }
    for slot in &broke {
        bag.id[*slot] = 0;
        bag.count[*slot] = 0;
        bag.dur[*slot] = 0.0;
        events.push(("item_break".to_string(), victim, None));
    }
    if let Some(w) = world.get_mut::<Inventory>(e) {
        bag.store(w);
    }
    if let Some(w) = world.get_mut::<Equipment>(e) {
        w.absorbed += stopped;
    }
    damage - stopped
}
