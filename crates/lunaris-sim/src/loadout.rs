//! Guns that are items (docs/LOADOUT.md): what a character holds in its bag decides what its `Weapon` is. A **`Loadout`** (on the character, or on the entity that carries its `Weapon`, a head) has a text that says
//! which items are guns and what each gun is (`gun rifle ammo=ammo556 damage=35 rate=600 mag=30 ...`) and the slot of the bag that is in hand; every tick, before the weapons fire, the `Weapon` is made the gun
//! that is in hand (or switched off if the hand holds no gun), its magazine is the one that gun had when it was last held and its reserve is the ammunition item the gun takes, counted in the bag; after the weapons
//! have fired and reloaded, the rounds that went from the reserve into the magazine are taken out of the bag. Nothing is drawn or played: the model of the gun in the hand is the game's.
//!
//! Deterministic: characters are visited in uid order and nothing is random. A world without a `Loadout` pays for one query.
use crate::components::{Loadout, Parent, Weapon};
use crate::inventory::{table_of, Bag, Inventory};
use lunaris_ecs::{Entity, World};
use std::cell::RefCell;
use std::rc::Rc;

/// What the text of a `Loadout` says one gun is.
#[derive(Clone, Debug, PartialEq)]
pub struct Gun {
    /// The key of the item that is this gun.
    pub item: String,
    /// The key of the ammunition item it takes (empty: it never runs out).
    pub ammo: String,
    pub damage: f32,
    pub rate: f32,
    pub mag: u32,
    pub reload: f32,
    pub spread: f32,
    pub range: f32,
    pub auto: bool,
    pub pellets: u32,
    pub noise: f32,
    pub head: f32,
    pub limb: f32,
    /// The share (0 to 1) of worn armor's stopping a hit of this gun ignores.
    pub pierce: f32,
    pub falloff_start: f32,
    pub falloff_min: f32,
    /// 0 an instant ray, 1 a round that flies.
    pub kind: u32,
    pub speed: f32,
    pub recoil: f32,
    pub recoil_side: f32,
    pub bloom: f32,
    /// What the spread and the bloom of a shot are multiplied by while the player aims down the sights (1: aiming changes nothing).
    pub aim: f32,
    /// The same for the kick of a shot.
    pub aim_recoil: f32,
}

impl Default for Gun {
    fn default() -> Gun {
        // (a rifle-like gun: what a line that says nothing gets)
        Gun {
            item: String::new(),
            ammo: String::new(),
            damage: 25.0,
            rate: 600.0,
            mag: 30,
            reload: 2.2,
            spread: 0.4,
            range: 200.0,
            auto: true,
            pellets: 1,
            noise: 120.0,
            head: 2.0,
            limb: 0.7,
            pierce: 0.0,
            falloff_start: 50.0,
            falloff_min: 0.35,
            kind: 0,
            speed: 400.0,
            recoil: 0.6,
            recoil_side: 0.25,
            bloom: 0.3,
            aim: 1.0,
            aim_recoil: 1.0,
        }
    }
}

/// Read the text of a `Loadout`: one gun a line, `gun ITEM key=value ...` (`#` starts a comment). Lines that are not understood are skipped and listed.
pub fn parse_guns(text: &str) -> (Vec<Gun>, Vec<String>) {
    let mut guns: Vec<Gun> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        if words.next() != Some("gun") {
            problems.push(format!("line {}: a line is `gun ITEM key=value ...`, not `{line}`", n + 1));
            continue;
        }
        let Some(item) = words.next() else {
            problems.push(format!("line {}: `gun` needs the key of an item", n + 1));
            continue;
        };
        let mut g = Gun { item: item.to_string(), ..Gun::default() };
        for w in words {
            let Some((k, v)) = w.split_once('=') else {
                problems.push(format!("line {}: `{w}` is not key=value", n + 1));
                continue;
            };
            let num = v.parse::<f32>().ok().filter(|x| x.is_finite());
            let mut set = |slot: &mut f32| match num {
                Some(x) => *slot = x,
                None => problems.push(format!("line {}: {k} needs a number, not `{v}`", n + 1)),
            };
            match k {
                "ammo" => g.ammo = v.to_string(),
                "damage" => set(&mut g.damage),
                "rate" => set(&mut g.rate),
                "reload" => set(&mut g.reload),
                "spread" => set(&mut g.spread),
                "range" => set(&mut g.range),
                "noise" => set(&mut g.noise),
                "head" => set(&mut g.head),
                "limb" => set(&mut g.limb),
                "pierce" => set(&mut g.pierce),
                "falloff" => set(&mut g.falloff_start),
                "falloff_min" => set(&mut g.falloff_min),
                "speed" => set(&mut g.speed),
                "recoil" => set(&mut g.recoil),
                "recoil_side" => set(&mut g.recoil_side),
                "bloom" => set(&mut g.bloom),
                "aim" => set(&mut g.aim),
                "aim_recoil" => set(&mut g.aim_recoil),
                "mag" => g.mag = num.map_or(g.mag, |x| x.max(1.0) as u32),
                "pellets" => g.pellets = num.map_or(g.pellets, |x| x.max(1.0) as u32),
                "kind" => g.kind = num.map_or(g.kind, |x| x.max(0.0) as u32),
                "auto" => g.auto = num.is_some_and(|x| x != 0.0),
                _ => problems.push(format!("line {}: unknown key `{k}`", n + 1)),
            }
        }
        if guns.iter().any(|x| x.item == g.item) {
            problems.push(format!("line {}: the gun `{}` is already defined", n + 1, g.item));
            continue;
        }
        guns.push(g);
    }
    (guns, problems)
}

thread_local! {
    /// The text of the guns last read and what it parsed to (found again by comparing the text: every character that holds a gun asks, on every tick).
    static CACHE: RefCell<Option<(String, Rc<Vec<Gun>>)>> = const { RefCell::new(None) };
}

fn guns_of(text: &str) -> Rc<Vec<Gun>> {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((k, g)) = c.as_ref() {
            if k == text {
                return g.clone();
            }
        }
        let g = Rc::new(parse_guns(text).0);
        *c = Some((text.to_string(), g.clone()));
        g
    })
}

/// The characters with a loadout: (loadout entity, uid, the loadout, the entity with the weapon, the entity with the bag).
fn loadouts(world: &mut World) -> Vec<(Entity, u64, Loadout, Entity, Entity)> {
    let mut found: Vec<(Entity, u64, Loadout)> = Vec::new();
    let _ = world.for_each_uid::<(&Loadout,), _>(|e, uid, (l,)| found.push((e, uid, *l)));
    found.sort_by_key(|f| f.1);
    let mut out = Vec::with_capacity(found.len());
    for (e, uid, l) in found {
        let we = if l.weapon.0 != 0 { world.entity(l.weapon.0) } else { Some(e) };
        // (whose bag: the one named, or this entity's own, or what this entity hangs on)
        let ie = if l.inventory.0 != 0 {
            world.entity(l.inventory.0)
        } else if world.get::<Inventory>(e).is_some() {
            Some(e)
        } else {
            world.get::<Parent>(e).and_then(|p| world.entity(p.target.0))
        };
        if let (Some(we), Some(ie)) = (we, ie) {
            out.push((e, uid, l, we, ie));
        }
    }
    out
}

/// Before the weapons fire: the `Weapon` becomes the gun in hand (or is switched off), with its magazine and the ammunition of the bag as its reserve.
pub fn before(world: &mut World) {
    let list = loadouts(world);
    if list.is_empty() {
        return;
    }
    let Some((table, _)) = table_of(world) else { return };
    for (e, _, mut l, we, ie) in list {
        let Some(inv) = world.get::<Inventory>(ie).copied() else { continue };
        let Some(mut w) = world.get::<Weapon>(we).copied() else { continue };
        let guns = guns_of(world.strings().get(l.guns.0));
        let size = (inv.size as usize).clamp(1, 48);
        let slot = (l.held as usize).min(size - 1);
        let item = if inv.count[slot] > 0 { inv.id[slot] } else { 0 };
        let key = table.item(item).map(|i| i.key.as_str()).unwrap_or("");
        let gun = (item != 0).then(|| guns.iter().find(|g| g.item == key)).flatten();
        // (leaving a gun: its magazine is kept for when it is held again)
        let changed = l.holding != item || l.slot as usize != slot;
        if changed && l.holding != 0 && (l.slot as usize) < 48 {
            l.mags[l.slot as usize] = w.mag;
        }
        match gun {
            Some(g) => {
                if changed {
                    w.damage = g.damage;
                    w.rate = g.rate;
                    w.mag_size = g.mag;
                    w.reload_time = g.reload;
                    w.spread = g.spread;
                    w.range = g.range;
                    w.auto = g.auto;
                    w.pellets = g.pellets;
                    w.noise = g.noise;
                    w.head_mult = g.head;
                    w.limb_mult = g.limb;
                    w.pierce = g.pierce;
                    w.falloff_start = g.falloff_start;
                    w.falloff_min = g.falloff_min;
                    w.kind = g.kind;
                    w.muzzle_speed = g.speed;
                    w.recoil = [g.recoil, g.recoil_side];
                    w.bloom_per_shot = g.bloom;
                    w.mag = if l.mags[slot] == u32::MAX { g.mag } else { l.mags[slot].min(g.mag) };
                    w.reload_left = 0.0;
                    w.cooldown = 0.0;
                    w.held = false;
                    l.holding = item;
                    l.slot = slot as u32;
                }
                // aiming down the sights: the scatter and the kick are the gun's own times its `aim` and `aim_recoil` while the aim button is held (set when the gun or the button changes, so a game that
                // changes the weapon's numbers in between keeps them until then)
                if changed || l.aiming != l.aim_applied {
                    let (scatter, kick) = if l.aiming { (g.aim.max(0.0), g.aim_recoil.max(0.0)) } else { (1.0, 1.0) };
                    w.spread = g.spread * scatter;
                    w.bloom_per_shot = g.bloom * scatter;
                    w.recoil = [g.recoil * kick, g.recoil_side * kick];
                    l.aim_applied = l.aiming;
                }
                w.active = true;
                if g.ammo.is_empty() {
                    w.infinite = true;
                    l.reserve = 0;
                } else {
                    w.infinite = false;
                    let have = Bag::of(&inv).total(table.id_of(&g.ammo));
                    w.reserve = have;
                    l.reserve = have;
                }
            }
            None => {
                if changed {
                    l.holding = 0;
                    l.slot = slot as u32;
                }
                w.active = false;
                w.fire = false;
                l.reserve = 0;
            }
        }
        if let Some(x) = world.get_mut::<Weapon>(we) {
            *x = w;
        }
        if let Some(x) = world.get_mut::<Loadout>(e) {
            *x = l;
        }
    }
}

/// After the weapons have fired and reloaded: the rounds that went from the reserve into the magazine come out of the bag.
pub fn after(world: &mut World) {
    let list = loadouts(world);
    if list.is_empty() {
        return;
    }
    let Some((table, _)) = table_of(world) else { return };
    for (e, _, mut l, we, ie) in list {
        if l.holding == 0 {
            continue;
        }
        let Some(w) = world.get::<Weapon>(we).copied() else { continue };
        if (l.slot as usize) < 48 {
            l.mags[l.slot as usize] = w.mag;
        }
        if !w.infinite && l.reserve > w.reserve {
            let guns = guns_of(world.strings().get(l.guns.0));
            let key = table.item(l.holding).map(|i| i.key.clone()).unwrap_or_default();
            let ammo = guns.iter().find(|g| g.item == key).map(|g| table.id_of(&g.ammo)).unwrap_or(0);
            let used = l.reserve - w.reserve;
            if ammo != 0 {
                if let Some(inv) = world.get_mut::<Inventory>(ie) {
                    let mut bag = Bag::of(inv);
                    let took = bag.remove(ammo, used);
                    bag.store(inv);
                    l.took = l.took.saturating_add(took);
                }
            }
            l.reserve = w.reserve;
        }
        if let Some(x) = world.get_mut::<Loadout>(e) {
            *x = l;
        }
    }
}
