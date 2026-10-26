//! Inventories: bags of item stacks on any entity (a hero, a chest, a corpse), an item and recipe table, and the binding of slots and cards of the interface kit to them.
//!
//! * `ItemTable` (one per world; the lowest uid counts) holds the items and recipes as text, one per line:
//!
//! ```text
//! # item KEY | Display name | property=value ...   (stack weight class power dur icon kind hp sp secs)
//! item wood | Wood | stack=50 weight=0.5
//! item iron_axe | Iron axe | stack=1 weight=2.5 class=1 power=9 dur=120 icon=14 kind=0
//! item helm | Iron helm | stack=1 weight=3 class=8 kind=1 icon=20
//! # recipe RESULT COUNT = INGREDIENT N, INGREDIENT N @ STATION   (@0 or no station: by hand; @n: station number n, 1 to 31)
//! recipe iron_axe 1 = wood 4, stone 6 @1
//! ```
//!
//! Items are numbered 1, 2, 3 ... in the order of their lines (0 is "nothing"); the numbering is what `Inventory`, `UiDrag.item` and a graph system see.
//! * `Inventory` keeps up to 48 slots (`id`, `count`, `dur`). Graph systems ask for things by setting request fields (`give_id`/`give_n`, `take_id`/`take_n`, `craft`, `wear`, `fill`);
//!   the engine does them at the end of the tick, in the order fill, wear, take, give, craft, writes the answers (`gave`, `left`, `took`, `crafted`), clears the requests, and keeps
//!   `weight`, the recipes that can be made now (`can`) and, with `publish`, the resources `PREFIX.KEY` (how many of each item are held) up to date.
//! * `InvSlot` (on a `Ui` that is also a `UiDrop`) says which slot of which inventory a rectangle shows; `InvCard` (on a `Ui` button that is also a `UiDrag`) is the thing that sits in
//!   that slot and can be dragged: the engine shows the item's picture on it, hides it when the slot is empty and turns a drop on another slot into a move (merge, swap or put down).
//!   `InvSlot.label` is a `Ui` that shows the stack count.
use crate::components::{NetContainer, NetPlayer, Transform, Ui, UiDrag, UiStyle};
use lunaris_ecs::{component, Entity, Sym, Uid, World};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// The most slots an `Inventory` can have.
pub const MAX_SLOTS: usize = 48;

type Events = Vec<(String, u64, Option<[f32; 3]>)>;

component! {
    /// The items and recipes of a game, as text (docs/GAMEPLAY.md, "Inventories"). One entity of the world has it (the lowest uid wins). `text` has one line per item,
    /// `item KEY | Display name | stack=50 weight=0.5 class=0 power=0 dur=0 icon=0 kind=0 hp=0 sp=0 secs=0`, and one per recipe, `recipe RESULT COUNT = INGREDIENT N, INGREDIENT N @STATION`;
    /// `#` starts a comment. Items are numbered from 1 in the order of their lines. `class`, `power`, `dur` (durability of one item), `hp`, `sp` and `secs` are numbers the game gives meaning to;
    /// `kind` (0 to 31) is the `UiDrag.kind` of the item's card, so that equipment slots (`UiDrop.accept`) take only what fits. `icons` is a picture of the workspace holding the item pictures in
    /// a grid of `icon_cols` by `icon_rows` cells; item number `icon` is a cell counted from the top left, row by row.
    pub struct ItemTable {
        pub text: Sym = Sym(0),
        pub icons: Sym = Sym(0),
        pub icon_cols: u32 = 8,
        pub icon_rows: u32 = 8,
    }
}

component! {
    /// A bag of item stacks: a hero's backpack, a chest, a corpse. `size` slots (1 to 48) hold `id` (item number, 0 = empty), `count` and `dur` (durability left of the one item in a stack of 1).
    /// Ask by setting a request, answered at the end of the tick and then cleared: `give_id`/`give_n` puts items in (onto stacks of the same thing first, then into empty slots among the first
    /// `bag` slots; `gave` is how many went in, `left` how many did not fit), `take_id`/`take_n` takes out (`took`), `craft` (recipe number + 1; needs the ingredients and the stations in the
    /// bit mask `station`; `crafted` says which was made), `wear` (slot number + 1: one use of a tool; it breaks at 0 and the event `item_break` is emitted), `fill` (text `wood 5, stone 3`: put in
    /// once, for starting loot). The answers hold for the next tick. `weight` is the total weight; with `strict_weight` and `max_weight` above 0, items that would take it over are refused
    /// (`left`). `can` is a bit mask of the recipes that can be made now (bit n of `can[n / 32]`). With `publish` set (a prefix such as `have`) the resources `PREFIX.KEY` hold how many of every item are
    /// held, `PREFIX.weight` the weight, `PREFIX.free` the empty slots of the bag, `PREFIX.can0` and `.can1` the `can` words, `PREFIX.sel` the selected slot (`sel`, set by the game: the hotbar) and `PREFIX.held.id`, `.class`, `.power`, `.dur`, `.kind`, `.hp`, `.sp`, `.secs` what that slot holds
    /// (0 when empty). Events: `item_given`, `item_taken`, `craft`, `item_break`, `inv_full`, `inv_move`, and `craft_denied` (a craft that was asked for and refused) together with the one that says why: `craft_denied_station` (the recipe's station is not at hand), `craft_denied_ingredients`, `craft_denied_room` (the result would not fit) or `craft_denied_busy` (a craft is already in hand); also when the rest of a quantity cannot go on.
    /// The stations at hand are `station` (set by the game) and the `Station` entities within reach of this entity (added by the engine, never written back).
    /// **Timed crafting:** a recipe with `~ SECONDS` takes that long. `craft_start` (recipe number + 1, with `craft_qty` crafts, 1 to 99) starts it: the ingredients are taken at once (a craft that cannot be made is refused),
    /// `crafting` (recipe number + 1, 0 when nothing is being made), `craft_left`, `craft_total` (seconds) and `craft_todo` (crafts left, this one included) say how it goes, and when `craft_left` reaches 0 the result is
    /// put in the bag (a bag with no room for it waits) and the next of the quantity starts if its ingredients are there. `craft_cancel` gives the ingredients of the craft in hand back (only if they fit: a full bag keeps the craft and says `inv_full`). A craft is refused (with `inv_full`) when its result would not fit once the ingredients are out; a craft whose result no longer fits when it is done waits with `craft_wait` set and says `inv_full` once. The older request `craft` on a timed recipe is a start of one craft: no request makes a timed recipe at once. A recipe without a time is made at once, as many
    /// times as `craft_qty` says. The timer runs only where the truth is: on a client, an inventory of a `NetPlayer` (the player's copy, told by the server) only counts `craft_left` down for the display.
    pub struct Inventory {
        pub size: u32 = 24,
        pub bag: u32 = 48,
        pub id: [u32; 48] = [0; 48],
        pub count: [u32; 48] = [0; 48],
        pub dur: [f32; 48] = [0.0; 48],
        pub give_id: u32 = 0,
        pub give_n: u32 = 0,
        pub take_id: u32 = 0,
        pub take_n: u32 = 0,
        pub craft: u32 = 0,
        pub wear: u32 = 0,
        pub fill: Sym = Sym(0),
        pub station: u32 = 0,
        pub gave: u32 = 0,
        pub left: u32 = 0,
        pub took: u32 = 0,
        pub crafted: u32 = 0,
        pub weight: f32 = 0.0,
        pub max_weight: f32 = 0.0,
        pub strict_weight: bool = false,
        pub can: [u32; 2] = [0, 0],
        pub publish: Sym = Sym(0),
        pub sel: u32 = 0,
        pub craft_start: u32 = 0,
        pub craft_qty: u32 = 1,
        pub craft_cancel: bool = false,
        pub crafting: u32 = 0,
        pub craft_left: f32 = 0.0,
        pub craft_total: f32 = 0.0,
        pub craft_todo: u32 = 0,
        pub craft_wait: bool = false,
    }
}

component! {
    /// A crafting station (a workbench, a forge, an electronics bench): an entity with a `Transform` that unlocks the recipes of station number `bit` (`recipe ... @BIT`, 1 to 31) for every `Inventory`
    /// whose own entity (its `Transform`) is within `radius` metres of it along the ground (x and z) and within `height` metres above or below it (`height` 0 = no limit). The engine works this out every tick
    /// in the inventory step, on a game server and on every client alike, and adds the bits to the ones the game sets in `Inventory.station` (which stays the game's own: the sum is used and never written back),
    /// so a bench is one entity that anyone can place, move, build or destroy, and the client's `have.can0` / `have.can1` account for it with no system of the game. With `publish` set and at least one
    /// `Station` in the world, `PREFIX.near` is the bit mask of the stations in reach (bit n set = station n at hand), for a hint over the hotbar. A world without a `Station` is bit for bit what it was. A
    /// `bit` outside 1 to 31, a `radius` of 0 or less, or a `Station` without a `Transform` reaches nobody (`check` says so).
    pub struct Station {
        pub bit: u32 = 1,
        pub radius: f32 = 3.0,
        pub height: f32 = 2.5,
    }
}

component! {
    /// On a `Ui` that is also a `UiDrop`: this rectangle is slot `index` of the `Inventory` of entity `inv` (a place to drop things on). `label` is a `Ui` (child of the same panel) the engine uses to
    /// write the stack count into (visible only above 1). See `InvCard`.
    pub struct InvSlot {
        pub inv: Uid = Uid(0),
        pub index: u32 = 0,
        pub label: Uid = Uid(0),
    }
}

component! {
    /// On a `Ui` button that is also a `UiDrag`: the thing that sits in the slot `slot` (an entity with an `InvSlot`) and shows what that slot holds: the engine hides it when the slot is empty,
    /// puts the item's picture on it (`UiStyle.image` and `image_uv`, from the `ItemTable`), sets `UiDrag.item` and `UiDrag.kind` from the item, and, when it has been dropped on another slot, moves the
    /// stack in the inventories (onto a stack of the same item, swapping, or into an empty slot) and sets it back in its own slot. Dropped on a `UiDrop` that is not an inventory slot (a bin, the
    /// ground) it goes back and the resources `inv.out.uid` (that slot), `inv.out.id` and `inv.out.n` say so for a tick, for a graph system to `take` the stack and drop it in the world. `Ui.visible` of a card is the
    /// engine's: hide a whole panel with a `UiGroup`.
    pub struct InvCard {
        pub slot: Uid = Uid(0),
    }
}

// ------------------------------------------------------------------------------------------------ the table
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub key: String,
    pub name: String,
    pub stack: u32,
    pub weight: f32,
    pub class: u32,
    pub power: f32,
    pub dur: f32,
    pub icon: u32,
    pub kind: u32,
    pub hp: f32,
    pub sp: f32,
    pub secs: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recipe {
    pub result: u32,
    pub count: u32,
    pub ingredients: Vec<(u32, u32)>,
    pub station: u32,
    /// Seconds a craft takes (`~ SECONDS` at the end of the line): 0 is at once, above 0 a timed craft (`Inventory.craft_start`).
    pub time: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub items: Vec<Item>,
    pub recipes: Vec<Recipe>,
    pub by_key: BTreeMap<String, u32>,
    pub problems: Vec<String>,
}

impl Table {
    /// The item with number `id` (1-based).
    pub fn item(&self, id: u32) -> Option<&Item> {
        (id >= 1).then(|| self.items.get(id as usize - 1)).flatten()
    }
    pub fn id_of(&self, key: &str) -> u32 {
        self.by_key.get(key).copied().unwrap_or(0)
    }
    fn stack_of(&self, id: u32) -> u32 {
        self.item(id).map_or(1, |i| i.stack.max(1))
    }
}

fn valid_key(k: &str) -> bool {
    !k.is_empty() && k.len() <= 40 && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Read the text of an `ItemTable`. Lines that are not understood are skipped and listed in `problems` (for `check`).
pub fn parse_table(text: &str) -> Table {
    let mut t = Table::default();
    let mut recipes: Vec<(usize, String)> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("item ") {
            let mut parts = rest.splitn(3, '|');
            let key = parts.next().unwrap_or("").trim().to_string();
            let name = parts.next().unwrap_or("").trim().to_string();
            let props = parts.next().unwrap_or("");
            if !valid_key(&key) {
                t.problems.push(format!("line {}: `{}` is not an item key (letters, digits and underscores)", n + 1, key));
                continue;
            }
            if t.by_key.contains_key(&key) {
                t.problems.push(format!("line {}: item `{}` is already defined", n + 1, key));
                continue;
            }
            let mut it = Item { key: key.clone(), name: if name.is_empty() { key.clone() } else { name }, stack: 50, ..Item::default() };
            for kv in props.split_whitespace() {
                let Some((k, v)) = kv.split_once('=') else {
                    t.problems.push(format!("line {}: `{}` is not property=value", n + 1, kv));
                    continue;
                };
                let Ok(x) = v.parse::<f32>() else {
                    t.problems.push(format!("line {}: `{}` is not a number", n + 1, kv));
                    continue;
                };
                if !x.is_finite() {
                    continue;
                }
                match k {
                    "stack" => it.stack = x.clamp(1.0, 9999.0) as u32,
                    "weight" => it.weight = x.max(0.0),
                    "class" => it.class = x.max(0.0) as u32,
                    "power" => it.power = x,
                    "dur" => it.dur = x.max(0.0),
                    "icon" => it.icon = x.max(0.0) as u32,
                    "kind" => it.kind = (x.max(0.0) as u32).min(31),
                    "hp" => it.hp = x,
                    "sp" => it.sp = x,
                    "secs" => it.secs = x,
                    _ => t.problems.push(format!("line {}: unknown property `{}`", n + 1, k)),
                }
            }
            t.items.push(it);
            t.by_key.insert(key, t.items.len() as u32);
        } else if let Some(rest) = line.strip_prefix("recipe ") {
            recipes.push((n + 1, rest.to_string()));
        } else {
            t.problems.push(format!("line {}: expected `item ...` or `recipe ...`", n + 1));
        }
    }
    // recipes after every item is known, so they may name items defined below them
    for (n, rest) in recipes {
        let (head, tail) = match rest.split_once('=') {
            Some(x) => x,
            None => {
                t.problems.push(format!("line {n}: a recipe needs `=`"));
                continue;
            }
        };
        let mut h = head.split_whitespace();
        let (Some(res), Some(cnt)) = (h.next(), h.next()) else {
            t.problems.push(format!("line {n}: a recipe starts `recipe RESULT COUNT =`"));
            continue;
        };
        let result = t.id_of(res);
        let Ok(count) = cnt.parse::<u32>() else {
            t.problems.push(format!("line {n}: `{cnt}` is not a count"));
            continue;
        };
        if result == 0 || count == 0 {
            t.problems.push(format!("line {n}: recipe result `{res}` is not an item (or the count is 0)"));
            continue;
        }
        // after the ingredients come `@ STATION` and `~ SECONDS`, in either order; a time that is no number makes the line wrong (it must not become an instant recipe)
        let (ing, mut opts) = tail.split_at(tail.find(['~', '@']).unwrap_or(tail.len()));
        let (mut time, mut station) = (0.0f32, 0u32);
        let mut ok = true;
        while let Some(kind) = opts.chars().next() {
            let body = &opts[1..];
            let end = body.find(['~', '@']).unwrap_or(body.len());
            let value = body[..end].trim();
            if kind == '~' {
                match value.parse::<f32>() {
                    Ok(v) if v.is_finite() && v >= 0.0 => time = v,
                    _ => {
                        t.problems.push(format!("line {n}: `~ {value}` is not a number of seconds"));
                        ok = false;
                    }
                }
            } else {
                station = value.parse::<u32>().unwrap_or(0).min(31);
            }
            opts = &body[end..];
        }
        let mut ingredients = Vec::new();
        for part in ing.split(',') {
            let mut w = part.split_whitespace();
            let (Some(k), Some(c)) = (w.next(), w.next()) else {
                if !part.trim().is_empty() {
                    ok = false;
                }
                continue;
            };
            let id = t.id_of(k);
            let c = c.parse::<u32>().unwrap_or(0);
            if id == 0 || c == 0 {
                t.problems.push(format!("line {n}: ingredient `{}` is not an item (or has no count)", part.trim()));
                ok = false;
            } else {
                ingredients.push((id, c));
            }
        }
        if ok {
            t.recipes.push(Recipe { result, count, ingredients, station, time });
        }
    }
    t
}

thread_local! {
    /// The text of the item table last read and what it parsed to. (Found again by comparing the text, which costs a copy's worth of reading, where hashing it was five times that: this is asked for by every system that
    /// touches a bag, on every tick.)
    static CACHE: RefCell<Option<(String, Rc<Table>)>> = const { RefCell::new(None) };
}

/// The parsed table of the world and its `ItemTable` (picture and grid), or None when the world has none.
pub fn table_of(world: &mut World) -> Option<(Rc<Table>, ItemTable)> {
    let mut best: Option<(u64, ItemTable)> = None;
    let _ = world.for_each_uid::<(&ItemTable,), _>(|_, uid, (t,)| {
        if best.as_ref().is_none_or(|b| uid < b.0) {
            best = Some((uid, *t));
        }
    });
    let (_, it) = best?;
    let text = world.strings().get(it.text.0);
    let table = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((k, t)) = c.as_ref() {
            if k == text {
                return t.clone();
            }
        }
        let t = Rc::new(parse_table(text));
        *c = Some((text.to_string(), t.clone()));
        t
    });
    Some((table, it))
}

// ------------------------------------------------------------------------------------------------ the bag
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bag {
    pub size: usize,
    pub bag: usize,
    pub id: [u32; MAX_SLOTS],
    pub count: [u32; MAX_SLOTS],
    pub dur: [f32; MAX_SLOTS],
}

impl Bag {
    pub(crate) fn of(i: &Inventory) -> Bag {
        let size = (i.size as usize).clamp(1, MAX_SLOTS);
        Bag { size, bag: (i.bag as usize).min(size), id: i.id, count: i.count, dur: i.dur }
    }

    pub(crate) fn store(&self, i: &mut Inventory) {
        i.id = self.id;
        i.count = self.count;
        i.dur = self.dur;
    }

    pub fn total(&self, id: u32) -> u32 {
        (0..self.size).filter(|&k| self.id[k] == id).map(|k| self.count[k]).fold(0u32, |a, b| a.saturating_add(b))
    }

    fn clear(&mut self, k: usize) {
        self.id[k] = 0;
        self.count[k] = 0;
        self.dur[k] = 0.0;
    }

    /// Put `n` of item `id` in: onto stacks of it first, then into empty slots among the first `bag`. Returns how many did not fit.
    pub fn add(&mut self, table: &Table, id: u32, n: u32) -> u32 {
        if id == 0 || n == 0 {
            return 0;
        }
        let stack = table.stack_of(id);
        let dur = table.item(id).map_or(0.0, |i| i.dur);
        let mut left = n;
        for k in 0..self.bag {
            if left == 0 {
                break;
            }
            if self.id[k] == id && self.count[k] < stack {
                let put = left.min(stack - self.count[k]);
                self.count[k] += put;
                left -= put;
            }
        }
        for k in 0..self.bag {
            if left == 0 {
                break;
            }
            if self.id[k] == 0 {
                let put = left.min(stack);
                self.id[k] = id;
                self.count[k] = put;
                self.dur[k] = dur;
                left -= put;
            }
        }
        left
    }

    /// Take up to `n` of item `id`, from the first slots that hold it. Returns how many were taken.
    pub fn remove(&mut self, id: u32, n: u32) -> u32 {
        let mut need = n;
        for k in 0..self.size {
            if need == 0 {
                break;
            }
            if self.id[k] == id && id != 0 {
                let t = need.min(self.count[k]);
                self.count[k] -= t;
                need -= t;
                if self.count[k] == 0 {
                    self.clear(k);
                }
            }
        }
        n - need
    }

    pub fn weight(&self, table: &Table) -> f32 {
        let mut w = 0.0f32;
        for k in 0..self.size {
            if self.id[k] != 0 {
                w += self.count[k] as f32 * table.item(self.id[k]).map_or(0.0, |i| i.weight);
            }
        }
        w
    }

    /// Is recipe `r` possible with what is held and the stations at hand?
    pub fn can_make(&self, r: &Recipe, station: u32) -> bool {
        (r.station == 0 || station & (1u32 << r.station) != 0) && r.ingredients.iter().all(|&(id, n)| self.total(id) >= n)
    }
}

/// How many of `n` items of `id` the weight limit lets in.
fn allowed_by_weight(inv: &Inventory, bag: &Bag, table: &Table, id: u32, n: u32) -> u32 {
    if !inv.strict_weight || inv.max_weight <= 0.0 {
        return n;
    }
    let w = table.item(id).map_or(0.0, |i| i.weight);
    if w <= 0.0 {
        return n;
    }
    let room = (inv.max_weight - bag.weight(table)).max(0.0);
    ((room / w).floor() as u32).min(n)
}

// ------------------------------------------------------------------------------------------------ the tick
/// One craft at once: the ingredients out, the result in; `Err(why)` (and the bag as it was) when it cannot be made (`why_not`) or does not fit (`"room"`).
fn craft_now(bag: &mut Bag, table: &Table, inv: &Inventory, station: u32, rec: &Recipe) -> Result<(), &'static str> {
    if !bag.can_make(rec, station) {
        return Err(why_not(rec, station));
    }
    let mut trial = *bag;
    for &(id, n) in &rec.ingredients {
        trial.remove(id, n);
    }
    let okn = allowed_by_weight(inv, &trial, table, rec.result, rec.count);
    if okn == rec.count && trial.add(table, rec.result, rec.count) == 0 {
        *bag = trial;
        Ok(())
    } else {
        Err("room")
    }
}

/// Why a recipe cannot be made now: its station is not at hand (named first: it is what the player has to walk to) or the ingredients are not all there.
fn why_not(rec: &Recipe, station: u32) -> &'static str {
    if rec.station != 0 && station & (1u32 << rec.station) == 0 {
        "station"
    } else {
        "ingredients"
    }
}

/// A craft that was asked for and refused: the event `craft_denied` and the one that says why, `craft_denied_station`, `craft_denied_ingredients`, `craft_denied_room` (the result would not fit) or
/// `craft_denied_busy` (a craft is already in hand).
fn denied(events: &mut Events, uid: u64, why: &str) {
    events.push(("craft_denied".to_string(), uid, None));
    events.push((format!("craft_denied_{why}"), uid, None));
}

/// A station of the world, ready to measure: where it is, which bit it sets, how far it reaches along the ground and above or below it.
#[derive(Clone, Copy)]
struct Reach {
    x: f32,
    y: f32,
    z: f32,
    bit: u32,
    r2: f32,
    height: f32,
}

/// Every `Station` with a `Transform` that can reach anybody (a bit from 1 to 31 and a radius above 0).
fn stations_of(world: &mut World) -> Vec<Reach> {
    let mut v = Vec::new();
    let _ = world.for_each::<(&Station, &Transform), _>(|_, (s, t)| {
        if (1..=31).contains(&s.bit) && s.radius > 0.0 && s.radius.is_finite() {
            v.push(Reach { x: t.pos[0], y: t.pos[1], z: t.pos[2], bit: s.bit, r2: s.radius * s.radius, height: if s.height.is_finite() { s.height } else { 0.0 } });
        }
    });
    v
}

/// The stations within reach of an entity (0 when it has no `Transform`).
fn near_of(world: &World, e: Entity, stations: &[Reach]) -> u32 {
    if stations.is_empty() {
        return 0;
    }
    world.get::<Transform>(e).map_or(0, |t| stations_near(stations, t.pos))
}

/// The bit mask of the stations within reach of a place (only `+ - *` and comparisons, so every machine agrees).
fn stations_near(stations: &[Reach], p: [f32; 3]) -> u32 {
    let mut mask = 0u32;
    for s in stations {
        let (dx, dz) = (p[0] - s.x, p[2] - s.z);
        if dx * dx + dz * dz <= s.r2 && (s.height <= 0.0 || (p[1] - s.y).abs() <= s.height) {
            mask |= 1u32 << s.bit;
        }
    }
    mask
}

fn parse_fill(table: &Table, text: &str) -> Vec<(u32, u32)> {
    let mut v = Vec::new();
    for part in text.split(',') {
        let mut w = part.split_whitespace();
        if let (Some(k), Some(n)) = (w.next(), w.next()) {
            let id = table.id_of(k);
            if let (true, Ok(n)) = (id != 0, n.parse::<u32>()) {
                v.push((id, n));
            }
        }
    }
    v
}

/// Do the requests of every inventory, keep their totals, and bind the slots and cards of the interface to them. A world without an `ItemTable` or an `Inventory` pays for two queries.
pub fn update(world: &mut World, dt: f32, events: &mut Events) {
    let Some((table, itab)) = table_of(world) else { return };
    let mut invs: Vec<(u64, Entity, Inventory)> = Vec::new();
    let _ = world.for_each_uid::<(&Inventory,), _>(|e, uid, (i,)| invs.push((uid, e, *i)));
    invs.sort_by_key(|x| x.0);
    let stations = stations_of(world);
    for (uid, e, mut inv) in invs {
        let mut bag = Bag::of(&inv);
        // the stations in reach (a bench near the entity) count with the ones the game sets in `inv.station`
        let near = near_of(world, e, &stations);
        let station = inv.station | near;
        inv.gave = 0;
        inv.left = 0;
        inv.took = 0;
        inv.crafted = 0;
        let mut changed = false;
        // fill: starting loot
        if inv.fill.0 != 0 {
            let text = world.strings().get(inv.fill.0).to_string();
            for (id, n) in parse_fill(&table, &text) {
                bag.add(&table, id, n);
            }
            inv.fill = Sym(0);
            changed = true;
        }
        // wear: one use of the tool in a slot
        if inv.wear > 0 {
            let k = inv.wear as usize - 1;
            inv.wear = 0;
            if k < bag.size && bag.id[k] != 0 && bag.dur[k] > 0.0 {
                bag.dur[k] -= 1.0;
                if bag.dur[k] <= 0.0 {
                    bag.clear(k);
                    events.push(("item_break".to_string(), uid, None));
                }
                changed = true;
            }
        }
        // take
        if inv.take_id > 0 && inv.take_n > 0 {
            inv.took = bag.remove(inv.take_id, inv.take_n);
            if inv.took > 0 {
                events.push(("item_taken".to_string(), uid, None));
                changed = true;
            }
        }
        inv.take_id = 0;
        inv.take_n = 0;
        // give
        if inv.give_id > 0 && inv.give_n > 0 {
            let okn = allowed_by_weight(&inv, &bag, &table, inv.give_id, inv.give_n);
            let left_slots = bag.add(&table, inv.give_id, okn);
            inv.gave = okn - left_slots;
            inv.left = inv.give_n - inv.gave;
            if inv.gave > 0 {
                events.push(("item_given".to_string(), uid, None));
                changed = true;
            }
            if inv.left > 0 {
                events.push(("inv_full".to_string(), uid, None));
            }
        }
        inv.give_id = 0;
        inv.give_n = 0;
        // craft
        if inv.craft > 0 {
            let r = inv.craft as usize - 1;
            inv.craft = 0;
            if let Some(rec) = table.recipes.get(r) {
                if rec.time > 0.0 {
                    // (a timed recipe is never made at once, whichever request asks: this one is a start of one craft)
                    if inv.craft_start == 0 {
                        inv.craft_start = r as u32 + 1;
                        inv.craft_qty = 1;
                    }
                } else if bag.can_make(rec, station) {
                    let mut trial = bag;
                    for &(id, n) in &rec.ingredients {
                        trial.remove(id, n);
                    }
                    let okn = allowed_by_weight(&inv, &trial, &table, rec.result, rec.count);
                    if okn == rec.count && trial.add(&table, rec.result, rec.count) == 0 {
                        bag = trial;
                        inv.crafted = r as u32 + 1;
                        events.push(("craft".to_string(), uid, None));
                        changed = true;
                    } else {
                        denied(events, uid, "room");
                    }
                } else {
                    denied(events, uid, why_not(rec, station));
                }
            }
        }
        // timed crafting (on a client the player's copy only counts down: the server has the truth; a player's copy that is not connected to a server is its own truth)
        if world.get::<crate::components::NetPlayer>(e).is_some_and(|p| p.connected) {
            if inv.crafting > 0 {
                inv.craft_left = (inv.craft_left - dt).max(0.0);
            }
            inv.craft_start = 0;
            inv.craft_cancel = false;
        } else {
            if inv.craft_start > 0 {
                let r = inv.craft_start as usize - 1;
                let qty = inv.craft_qty.clamp(1, 99);
                inv.craft_start = 0;
                inv.craft_qty = 1;
                if inv.crafting == 0 {
                    if let Some(rec) = table.recipes.get(r) {
                        if rec.time <= 0.0 {
                            for k in 0..qty {
                                if let Err(why) = craft_now(&mut bag, &table, &inv, station, rec) {
                                    // (refused only when not even the first could be made: a quantity that runs out of ingredients just ends)
                                    if k == 0 {
                                        denied(events, uid, why);
                                    }
                                    break;
                                }
                                inv.crafted = r as u32 + 1;
                                events.push(("craft".to_string(), uid, None));
                                changed = true;
                            }
                        } else if bag.can_make(rec, station) {
                            // (the result must have room once the ingredients are out: otherwise the craft would end waiting with its ingredients gone)
                            let mut trial = bag;
                            for &(id, n) in &rec.ingredients {
                                trial.remove(id, n);
                            }
                            let okn = allowed_by_weight(&inv, &trial, &table, rec.result, rec.count);
                            if okn == rec.count && trial.add(&table, rec.result, rec.count) == 0 {
                                for &(id, n) in &rec.ingredients {
                                    bag.remove(id, n);
                                }
                                inv.crafting = r as u32 + 1;
                                inv.craft_left = rec.time;
                                inv.craft_total = rec.time;
                                inv.craft_todo = qty;
                                inv.craft_wait = false;
                                changed = true;
                            } else {
                                events.push(("inv_full".to_string(), uid, None));
                                denied(events, uid, "room");
                            }
                        } else {
                            denied(events, uid, why_not(rec, station));
                        }
                    }
                } else if table.recipes.get(r).is_some() {
                    // (one craft in hand at a time)
                    denied(events, uid, "busy");
                }
            }
            // (after the start: a start and a cancel in one tick cancel the craft that was just started)
            if inv.craft_cancel {
                inv.craft_cancel = false;
                if inv.crafting > 0 {
                    match table.recipes.get(inv.crafting as usize - 1) {
                        Some(rec) => {
                            // the ingredients go back only if they fit (a full bag keeps the craft, and says so)
                            let mut trial = bag;
                            let fits = rec.ingredients.iter().all(|&(id, n)| allowed_by_weight(&inv, &trial, &table, id, n) == n && trial.add(&table, id, n) == 0);
                            if fits {
                                bag = trial;
                                inv.crafting = 0;
                                inv.craft_todo = 0;
                                inv.craft_left = 0.0;
                                inv.craft_total = 0.0;
                                inv.craft_wait = false;
                                changed = true;
                            } else {
                                events.push(("inv_full".to_string(), uid, None));
                            }
                        }
                        None => {
                            inv.crafting = 0;
                            inv.craft_todo = 0;
                            inv.craft_left = 0.0;
                            inv.craft_total = 0.0;
                            inv.craft_wait = false;
                        }
                    }
                }
            }
            if inv.crafting > 0 {
                inv.craft_left = (inv.craft_left - dt).max(0.0);
                if inv.craft_left <= 0.0 {
                    let r = inv.crafting as usize - 1;
                    match table.recipes.get(r) {
                        Some(rec) => {
                            // (the ingredients are already taken: only the result is put in; a bag with no room for it waits, and says so once)
                            let mut trial = bag;
                            let okn = allowed_by_weight(&inv, &trial, &table, rec.result, rec.count);
                            if okn == rec.count && trial.add(&table, rec.result, rec.count) == 0 {
                                bag = trial;
                                inv.crafted = r as u32 + 1;
                                events.push(("craft".to_string(), uid, None));
                                changed = true;
                                inv.craft_wait = false;
                                inv.craft_todo = inv.craft_todo.saturating_sub(1);
                                if inv.craft_todo > 0 && bag.can_make(rec, station) {
                                    for &(id, n) in &rec.ingredients {
                                        bag.remove(id, n);
                                    }
                                    inv.craft_left = rec.time;
                                } else {
                                    if inv.craft_todo > 0 {
                                        // (the rest of a quantity cannot go on: the station is out of reach or the ingredients are gone)
                                        denied(events, uid, why_not(rec, station));
                                    }
                                    inv.crafting = 0;
                                    inv.craft_todo = 0;
                                    inv.craft_total = 0.0;
                                }
                            } else if !inv.craft_wait {
                                inv.craft_wait = true;
                                events.push(("inv_full".to_string(), uid, None));
                            }
                        }
                        None => {
                            inv.crafting = 0;
                            inv.craft_todo = 0;
                            inv.craft_left = 0.0;
                            inv.craft_wait = false;
                        }
                    }
                }
            }
        }
        let _ = changed;
        finish(world, &table, e, uid, &mut inv, &bag, station, near, !stations.is_empty());
    }
    ui_moves(world, &table, events);
    ui_cards(world, &table, &itab);
}

/// Totals after the requests: weight, what can be made, the published resources; and the write-back.
fn finish(world: &mut World, table: &Table, e: Entity, _uid: u64, inv: &mut Inventory, bag: &Bag, station: u32, near: u32, with_stations: bool) {
    bag.store(inv);
    inv.weight = bag.weight(table);
    inv.can = [0, 0];
    for (r, rec) in table.recipes.iter().take(64).enumerate() {
        if bag.can_make(rec, station) {
            inv.can[r / 32] |= 1u32 << (r % 32);
        }
    }
    let prefix = world.strings().get(inv.publish.0).to_string();
    let wanted = !prefix.is_empty();
    if let Some(w) = world.get_mut::<Inventory>(e) {
        *w = *inv;
    }
    if wanted {
        for (k, it) in table.items.iter().enumerate() {
            let name = format!("{prefix}.{}", it.key);
            let v = bag.total(k as u32 + 1) as f64;
            if world.resource(&name) != Some(v) {
                let _ = world.set_resource(&name, Some(v));
            }
        }
        let wname = format!("{prefix}.weight");
        let wv = inv.weight as f64;
        if world.resource(&wname) != Some(wv) {
            let _ = world.set_resource(&wname, Some(wv));
        }
        // what can be made (the mask, 32 recipes a word) and how many slots of the bag are free
        let free = (0..bag.bag).filter(|&k| bag.id[k] == 0).count();
        for (n, v) in [("can0", inv.can[0] as f64), ("can1", inv.can[1] as f64), ("free", free as f64), ("crafting", inv.crafting as f64), ("craft_left", inv.craft_left as f64), ("craft_total", inv.craft_total as f64), ("craft_todo", inv.craft_todo as f64), ("craft_wait", if inv.craft_wait { 1.0 } else { 0.0 })] {
            let name = format!("{prefix}.{n}");
            if world.resource(&name) != Some(v) {
                let _ = world.set_resource(&name, Some(v));
            }
        }
        // the stations in reach: only in a world that has stations (or has had them: the resource goes back to 0 when the last one is gone)
        let nname = format!("{prefix}.near");
        if with_stations || world.resource(&nname).is_some() {
            let nv = near as f64;
            if world.resource(&nname) != Some(nv) {
                let _ = world.set_resource(&nname, Some(nv));
            }
        }
        // the selected slot and what it holds
        let k = (inv.sel as usize).min(bag.size - 1);
        let hid = bag.id[k];
        let it = table.item(hid);
        let held: [(&str, f64); 10] = [
            ("sel", k as f64),
            ("held.id", hid as f64),
            ("held.class", it.map_or(0.0, |i| i.class as f64)),
            ("held.power", it.map_or(0.0, |i| i.power as f64)),
            ("held.dur", if hid != 0 { bag.dur[k] as f64 } else { 0.0 }),
            ("held.kind", it.map_or(0.0, |i| i.kind as f64)),
            ("held.hp", it.map_or(0.0, |i| i.hp as f64)),
            ("held.sp", it.map_or(0.0, |i| i.sp as f64)),
            ("held.secs", it.map_or(0.0, |i| i.secs as f64)),
            ("held.n", if hid != 0 { bag.count[k] as f64 } else { 0.0 }),
        ];
        for (n, v) in held {
            let name = format!("{prefix}.{n}");
            if world.resource(&name) != Some(v) {
                let _ = world.set_resource(&name, Some(v));
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------ the interface
struct Where {
    inv: u64,
    index: usize,
    label: u64,
}

fn slots_of(world: &mut World) -> BTreeMap<u64, Where> {
    let mut m = BTreeMap::new();
    let _ = world.for_each_uid::<(&InvSlot,), _>(|_, uid, (s,)| {
        m.insert(uid, Where { inv: s.inv.0, index: s.index as usize, label: s.label.0 });
    });
    m
}

/// Cards that were dropped on another slot become moves in the inventories; every card goes back to its own slot.
fn ui_moves(world: &mut World, table: &Table, events: &mut Events) {
    let mut cards: Vec<(u64, Entity, u64, u64)> = Vec::new();
    let _ = world.for_each_uid::<(&InvCard, &UiDrag), _>(|e, uid, (c, d)| cards.push((uid, e, c.slot.0, d.slot.0)));
    if cards.is_empty() {
        return;
    }
    cards.sort_by_key(|c| c.0);
    // the report of a drop on something that is not an inventory slot lasts a tick
    for n in ["inv.out.uid", "inv.out.id", "inv.out.n"] {
        if world.resource(n).is_some_and(|v| v != 0.0) {
            let _ = world.set_resource(n, Some(0.0));
        }
    }
    let moved: Vec<&(u64, Entity, u64, u64)> = cards.iter().filter(|c| c.3 != 0 && c.3 != c.2).collect();
    if moved.is_empty() {
        return;
    }
    let slots = slots_of(world);
    let mut done: BTreeSet<u64> = BTreeSet::new();
    let mut bags: BTreeMap<u64, Bag> = BTreeMap::new();
    let mut changed: BTreeSet<u64> = BTreeSet::new();
    for c in &moved {
        if done.contains(&c.0) {
            continue;
        }
        done.insert(c.0);
        // the card that was swapped out of the target slot, if any, was sent here
        for o in &moved {
            if o.2 == c.3 && o.3 == c.2 {
                done.insert(o.0);
            }
        }
        let (Some(from), to) = (slots.get(&c.2), slots.get(&c.3)) else { continue };
        let Some(to) = to else {
            // dropped on something that is not an inventory slot: report it (and, on a game server, ask for it to be dropped)
            ask_server(world, "inv_drop", (from.inv, from.index), None);
            if let Some(ent) = world.entity(from.inv) {
                if let Some(i) = world.get::<Inventory>(ent) {
                    let k = from.index;
                    if k < MAX_SLOTS && i.id[k] != 0 {
                        let (id, n) = (i.id[k], i.count[k]);
                        let _ = world.set_resource("inv.out.uid", Some(c.3 as f64));
                        let _ = world.set_resource("inv.out.id", Some(id as f64));
                        let _ = world.set_resource("inv.out.n", Some(n as f64));
                    }
                }
            }
            continue;
        };
        for u in [from.inv, to.inv] {
            if !bags.contains_key(&u) {
                if let Some(ent) = world.entity(u) {
                    if let Some(i) = world.get::<Inventory>(ent) {
                        bags.insert(u, Bag::of(i));
                    }
                }
            }
        }
        if from.index >= MAX_SLOTS || to.index >= MAX_SLOTS || !bags.contains_key(&from.inv) || !bags.contains_key(&to.inv) {
            continue;
        }
        if bags[&from.inv].size <= from.index || bags[&to.inv].size <= to.index {
            continue;
        }
        // the interface swaps the cards of the two slots when a card is dropped on a slot that has a card of its own (an empty slot's card is hidden, but it is swapped too), so the move comes twice, once from each card:
        // when the one in hand is the card of the empty slot (it has the lower number), the move is the other way round
        let mut did = transfer(&mut bags, table, from.inv, from.index, to.inv, to.index);
        let mut way = ((from.inv, from.index), (to.inv, to.index));
        let paired = moved.iter().any(|o| o.2 == c.3 && o.3 == c.2);
        if !did && paired {
            did = transfer(&mut bags, table, to.inv, to.index, from.inv, from.index);
            way = (way.1, way.0);
        }
        if did {
            changed.insert(from.inv);
            changed.insert(to.inv);
            events.push(("inv_move".to_string(), from.inv, None));
            ask_server(world, "inv_move", way.0, Some(way.1));
        }
    }
    let stations = if changed.is_empty() { Vec::new() } else { stations_of(world) };
    for u in changed {
        if let (Some(ent), Some(bag)) = (world.entity(u), bags.get(&u).copied()) {
            if let Some(i) = world.get_mut::<Inventory>(ent) {
                bag.store(i);
            }
            // totals follow at once (the next tick's update would otherwise show them a tick late)
            if let Some(mut inv) = world.get::<Inventory>(ent).copied() {
                let near = near_of(world, ent, &stations);
                let station = inv.station | near;
                finish(world, table, ent, u, &mut inv, &bag, station, near, !stations.is_empty());
            }
        }
    }
    for c in moved {
        if let Some(d) = world.get_mut::<UiDrag>(c.1) {
            d.slot = Uid(c.2);
        }
    }
}

/// On a game server (docs/GAME_SERVER.md, "Bags and containers") a move the player made in the interface is also asked of the server, as the command `inv_move` (or `inv_drop` for a card let go over the ground): the local
/// move shows at once, and the server's next message about the bag or the container makes it true, or puts things back when it refused. A place is 100 times the bag (0: the player's own, the inventory of the entity
/// with the `NetPlayer`; 1: the open container, the inventory of the entity with the `NetContainer`) plus the slot; a slot of any other inventory is no concern of the server's. Offline nothing is sent.
fn ask_server(world: &mut World, command: &str, from: (u64, usize), to: Option<(u64, usize)>) {
    let mut player: Option<(u64, Entity)> = None;
    let _ = world.for_each_uid::<(&NetPlayer,), _>(|e, uid, (p,)| {
        if player.is_none() && p.connected {
            player = Some((uid, e));
        }
    });
    let Some((player_uid, pe)) = player else { return };
    let place = |world: &World, inv: u64, index: usize| -> Option<f32> {
        let bag = if inv == player_uid {
            0
        } else if world.entity(inv).is_some_and(|e| world.get::<NetContainer>(e).is_some()) {
            1
        } else {
            return None;
        };
        Some((bag * 100 + index) as f32)
    };
    let args = match (command, to) {
        ("inv_move", Some(to)) => match (place(world, from.0, from.1), place(world, to.0, to.1)) {
            (Some(a), Some(b)) => [a, b, 0.0, 0.0],
            _ => return,
        },
        // (only the player's own things can be dropped: a slot of the container is not its to throw away)
        ("inv_drop", None) => match place(world, from.0, from.1) {
            Some(a) if a < 100.0 => [a, 0.0, 0.0, 0.0],
            _ => return,
        },
        _ => return,
    };
    let name = Sym(world.strings_mut().intern(command));
    if let Some(p) = world.get_mut::<NetPlayer>(pe) {
        p.cmd = name;
        p.cmd_args = args;
    }
}

/// Move the stack at (inventory a, slot i) to (b, j): into an empty slot, onto a stack of the same item, or swapping places.
fn transfer(bags: &mut BTreeMap<u64, Bag>, table: &Table, a: u64, i: usize, b: u64, j: usize) -> bool {
    if a == b && i == j {
        return false;
    }
    let src = {
        let ba = &bags[&a];
        (ba.id[i], ba.count[i], ba.dur[i])
    };
    if src.0 == 0 {
        return false;
    }
    let dst = {
        let bb = &bags[&b];
        (bb.id[j], bb.count[j], bb.dur[j])
    };
    let stack = table.stack_of(src.0);
    if dst.0 == 0 {
        let ba = bags.get_mut(&a).unwrap();
        ba.clear(i);
        let bb = bags.get_mut(&b).unwrap();
        bb.id[j] = src.0;
        bb.count[j] = src.1;
        bb.dur[j] = src.2;
        return true;
    }
    if dst.0 == src.0 && stack > 1 && dst.1 < stack {
        let put = src.1.min(stack - dst.1);
        let ba = bags.get_mut(&a).unwrap();
        ba.count[i] -= put;
        if ba.count[i] == 0 {
            ba.clear(i);
        }
        bags.get_mut(&b).unwrap().count[j] += put;
        return true;
    }
    // swap
    let ba = bags.get_mut(&a).unwrap();
    ba.id[i] = dst.0;
    ba.count[i] = dst.1;
    ba.dur[i] = dst.2;
    let bb = bags.get_mut(&b).unwrap();
    bb.id[j] = src.0;
    bb.count[j] = src.1;
    bb.dur[j] = src.2;
    true
}

/// What every card shows: the item's picture, its kind for the slots' `accept`, hidden when the slot is empty; and the count labels.
fn ui_cards(world: &mut World, table: &Table, itab: &ItemTable) {
    let mut cards: Vec<(u64, Entity, u64)> = Vec::new();
    let _ = world.for_each_uid::<(&InvCard, &UiDrag, &Ui), _>(|e, uid, (c, _, _)| cards.push((uid, e, c.slot.0)));
    if cards.is_empty() {
        return;
    }
    let slots = slots_of(world);
    let cols = itab.icon_cols.max(1);
    let rows = itab.icon_rows.max(1);
    for (_, e, slot) in cards {
        let Some(w) = slots.get(&slot) else { continue };
        let (id, count) = match world.entity(w.inv).and_then(|ie| world.get::<Inventory>(ie)) {
            Some(i) if w.index < (i.size as usize).min(MAX_SLOTS) => (i.id[w.index], i.count[w.index]),
            _ => (0, 0),
        };
        let item = table.item(id);
        if let Some(u) = world.get_mut::<Ui>(e) {
            u.visible = id != 0;
        }
        if let Some(d) = world.get_mut::<UiDrag>(e) {
            d.item = id;
            if let Some(it) = item {
                d.kind = it.kind;
            }
        }
        if let (Some(it), true) = (item, itab.icons.0 != 0) {
            let (c, r) = (it.icon % cols, (it.icon / cols).min(rows - 1));
            if let Some(st) = world.get_mut::<UiStyle>(e) {
                st.image = itab.icons;
                st.image_uv = [c as f32 / cols as f32, r as f32 / rows as f32, (c + 1) as f32 / cols as f32, (r + 1) as f32 / rows as f32];
            }
        }
        if w.label != 0 {
            if let Some(le) = world.entity(w.label) {
                let show = count > 1;
                let sym = if show { world.strings_mut().intern(&count.to_string()) } else { 0 };
                if let Some(u) = world.get_mut::<Ui>(le) {
                    u.visible = show;
                    if u.text.0 != sym {
                        u.text = Sym(sym);
                    }
                }
            }
        }
    }
}
