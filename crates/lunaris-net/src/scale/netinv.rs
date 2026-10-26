//! Bags and containers over the network (docs/GAME_SERVER.md, "Bags and containers"): the owner of an inventory is told what is in it, whenever it changes, and asks for things to be moved; nobody else is
//! told. The server keeps the truth (the avatar's `Inventory`, the container's); a client keeps a copy of its own bag in its player's `Inventory` and of the container it has open in the `Inventory` of its
//! `NetContainer`, so the interface kit's `InvSlot`s are bound to them as in a game for one player.
//!
//! The messages are reliable and small: a whole inventory is `size` slots, each an item number (a variable-length integer, 0 for empty) and, when there is one, its count and its durability (a tenth of
//! a point in two bytes): at most about 250 bytes for 48 slots, sent when something has changed.
use super::bytes::{R, W};
use lunaris_sim::inventory::{Inventory, Table};

/// The kinds of message of this layer (the first byte of a reliable message).
pub mod msg {
    /// The owner's bag: this byte and the slots.
    pub const INV: u8 = 0x41;
    /// The open container: this byte, the server's number for it, and the slots.
    pub const CONT: u8 = 0x42;
    /// The container has been shut: this byte alone.
    pub const CONT_END: u8 = 0x43;
}

const MAX_SLOTS: usize = 48;

fn write_slots(w: &mut W, inv: &Inventory) {
    let size = (inv.size as usize).clamp(1, MAX_SLOTS);
    w.u8(size as u8);
    for k in 0..size {
        let (id, count) = (inv.id[k], inv.count[k]);
        if id == 0 || count == 0 {
            w.varu(0);
        } else {
            w.varu(id as u64);
            w.varu(count as u64);
            w.u16((inv.dur[k] * 10.0).clamp(0.0, 65535.0) as u16);
        }
    }
}

fn read_slots(r: &mut R, into: &mut Inventory) -> Option<()> {
    let size = r.u8()? as usize;
    if size == 0 || size > MAX_SLOTS {
        return None;
    }
    let mut id = [0u32; MAX_SLOTS];
    let mut count = [0u32; MAX_SLOTS];
    let mut dur = [0.0f32; MAX_SLOTS];
    for k in 0..size {
        let i = u32::try_from(r.varu()?).ok()?;
        if i != 0 {
            id[k] = i;
            count[k] = u32::try_from(r.varu()?).ok()?;
            dur[k] = r.u16()? as f32 / 10.0;
        }
    }
    into.size = size as u32;
    into.bag = into.bag.min(size as u32);
    into.id = id;
    into.count = count;
    into.dur = dur;
    Some(())
}

/// The timed craft in hand (docs/GAMEPLAY.md, "Inventories"): which recipe, the seconds left and in all, how many crafts are still to make. The seconds left are told once, when the craft starts or changes, and counted down
/// by the client (its copy of the bag only counts: the server makes the item).
fn write_craft(w: &mut W, inv: &Inventory) {
    w.varu(inv.crafting as u64);
    if inv.crafting > 0 {
        w.u16((inv.craft_left * 10.0).clamp(0.0, 65535.0) as u16);
        w.u16((inv.craft_total * 10.0).clamp(0.0, 65535.0) as u16);
        w.varu(inv.craft_todo as u64);
    }
}

fn read_craft(r: &mut R, into: &mut Inventory) -> Option<()> {
    let crafting = if r.left() == 0 { 0 } else { u32::try_from(r.varu()?).ok()? };
    into.crafting = crafting;
    if crafting > 0 {
        into.craft_left = r.u16()? as f32 / 10.0;
        into.craft_total = r.u16()? as f32 / 10.0;
        into.craft_todo = u32::try_from(r.varu()?).ok()?;
    } else {
        into.craft_left = 0.0;
        into.craft_total = 0.0;
        into.craft_todo = 0;
    }
    Some(())
}

/// What is in an inventory, as one number that changes when the slots or the craft in hand do (the seconds left only every two seconds: they change every tick and the client counts them).
pub fn hash(inv: &Inventory) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ inv.size as u64 ^ (inv.crafting as u64).wrapping_mul(0x9E37_79B9) ^ ((inv.craft_todo as u64) << 20) ^ (((inv.craft_total * 10.0) as u64) << 40) ^ (((inv.craft_left / 2.0) as u64) << 52);
    for k in 0..(inv.size as usize).clamp(1, MAX_SLOTS) {
        for v in [inv.id[k] as u64, inv.count[k] as u64, (inv.dur[k] * 10.0) as u64] {
            h ^= v;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    h
}

/// The message that tells the owner what is in its bag.
pub fn inv_message(inv: &Inventory) -> Vec<u8> {
    let mut w = W::new();
    w.u8(msg::INV);
    write_slots(&mut w, inv);
    write_craft(&mut w, inv);
    w.buf
}

/// The message that tells the owner what is in the container it has open (`source` is the server's number for it).
pub fn cont_message(source: u32, inv: &Inventory) -> Vec<u8> {
    let mut w = W::new();
    w.u8(msg::CONT);
    w.varu(source as u64);
    write_slots(&mut w, inv);
    w.buf
}

pub fn cont_end() -> Vec<u8> {
    vec![msg::CONT_END]
}

/// Put what an [`inv_message`] says into `inv` (its requests and its other fields are left alone).
pub fn apply_inv(data: &[u8], inv: &mut Inventory) -> Option<()> {
    let mut r = R::new(data);
    if r.u8()? != msg::INV {
        return None;
    }
    read_slots(&mut r, inv)?;
    read_craft(&mut r, inv)
}

/// Put what a [`cont_message`] says into `inv`; returns the server's number for the container.
pub fn apply_cont(data: &[u8], inv: &mut Inventory) -> Option<u32> {
    let mut r = R::new(data);
    if r.u8()? != msg::CONT {
        return None;
    }
    let source = u32::try_from(r.varu()?).ok()?;
    read_slots(&mut r, inv)?;
    Some(source)
}

/// Move `count` (0: all) of what is in slot `from` of `a` to slot `to` of `b` (the same inventory when `b` is `None`): into an empty slot, onto a stack of the same item up to its stack size (what does not fit
/// stays), or, for a whole stack, a swap with a different item. Returns whether anything moved.
pub fn move_items(a: &mut Inventory, b: Option<&mut Inventory>, table: &Table, from: usize, to: usize, count: u32) -> bool {
    let stack = |id: u32| table.item(id).map_or(1, |i| i.stack.max(1));
    match b {
        None => {
            let size = (a.size as usize).clamp(1, MAX_SLOTS);
            if from >= size || to >= size || from == to {
                return false;
            }
            let (id, cnt, dur) = (a.id[from], a.count[from], a.dur[from]);
            if id == 0 || cnt == 0 {
                return false;
            }
            let n = if count == 0 || count >= cnt { cnt } else { count };
            let (tid, tcnt) = (a.id[to], a.count[to]);
            if tid == 0 || tcnt == 0 {
                a.id[to] = id;
                a.count[to] = n;
                a.dur[to] = dur;
                take(a, from, n);
                true
            } else if tid == id {
                let put = n.min(stack(id).saturating_sub(tcnt));
                if put == 0 {
                    return false;
                }
                a.count[to] += put;
                take(a, from, put);
                true
            } else if n == cnt {
                let (tdur, tid, tcnt) = (a.dur[to], a.id[to], a.count[to]);
                a.id[to] = id;
                a.count[to] = cnt;
                a.dur[to] = dur;
                a.id[from] = tid;
                a.count[from] = tcnt;
                a.dur[from] = tdur;
                true
            } else {
                false
            }
        }
        Some(b) => {
            let (sa, sb) = ((a.size as usize).clamp(1, MAX_SLOTS), (b.size as usize).clamp(1, MAX_SLOTS));
            if from >= sa || to >= sb {
                return false;
            }
            let (id, cnt, dur) = (a.id[from], a.count[from], a.dur[from]);
            if id == 0 || cnt == 0 {
                return false;
            }
            let n = if count == 0 || count >= cnt { cnt } else { count };
            let (tid, tcnt) = (b.id[to], b.count[to]);
            if tid == 0 || tcnt == 0 {
                b.id[to] = id;
                b.count[to] = n;
                b.dur[to] = dur;
                take(a, from, n);
                true
            } else if tid == id {
                let put = n.min(stack(id).saturating_sub(tcnt));
                if put == 0 {
                    return false;
                }
                b.count[to] += put;
                take(a, from, put);
                true
            } else if n == cnt {
                let (tdur, tid, tcnt) = (b.dur[to], b.id[to], b.count[to]);
                b.id[to] = id;
                b.count[to] = cnt;
                b.dur[to] = dur;
                a.id[from] = tid;
                a.count[from] = tcnt;
                a.dur[from] = tdur;
                true
            } else {
                false
            }
        }
    }
}

fn take(inv: &mut Inventory, k: usize, n: u32) {
    inv.count[k] = inv.count[k].saturating_sub(n);
    if inv.count[k] == 0 {
        inv.id[k] = 0;
        inv.dur[k] = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunaris_sim::inventory::parse_table;

    fn table() -> Table {
        parse_table("item bandage | Bandage | stack=10\nitem axe | Axe | stack=1\n")
    }

    fn bag(items: &[(usize, u32, u32)], size: u32) -> Inventory {
        let mut i = Inventory { size, bag: size, ..Default::default() };
        for &(k, id, n) in items {
            i.id[k] = id;
            i.count[k] = n;
            i.dur[k] = if id == 2 { 12.5 } else { 0.0 };
        }
        i
    }

    #[test]
    fn a_bag_round_trips_and_changes_change_its_hash() {
        let a = bag(&[(0, 1, 7), (3, 2, 1)], 12);
        let mut b = bag(&[], 5);
        let m = inv_message(&a);
        assert!(m.len() < 40, "{}", m.len());
        apply_inv(&m, &mut b).unwrap();
        assert_eq!((b.size, b.id[0], b.count[0], b.id[3], b.count[3]), (12, 1, 7, 2, 1));
        assert!((b.dur[3] - 12.5).abs() < 0.11);
        assert_eq!(hash(&a), hash(&b));
        let mut c = a;
        c.count[0] = 6;
        assert_ne!(hash(&a), hash(&c));
        // a container message names its source, a bad one is refused and never panics
        let m = cont_message(77, &a);
        assert_eq!(apply_cont(&m, &mut b), Some(77));
        assert!(apply_inv(&m, &mut b).is_none() && apply_cont(&[msg::CONT, 1, 0], &mut b).is_none() && apply_inv(&[], &mut b).is_none());
        // a full bag of 48 slots is a few hundred bytes at most
        let mut full = Inventory { size: 48, bag: 48, ..Default::default() };
        for k in 0..48 {
            full.id[k] = 1 + k as u32;
            full.count[k] = 100;
            full.dur[k] = 99.0;
        }
        assert!(inv_message(&full).len() < 400, "{}", inv_message(&full).len());
    }

    #[test]
    fn the_craft_in_hand_is_told_with_the_bag_and_changes_the_hash_but_not_every_tick() {
        let mut a = bag(&[(0, 1, 2)], 12);
        let idle = inv_message(&a);
        a.crafting = 3;
        a.craft_left = 7.5;
        a.craft_total = 12.0;
        a.craft_todo = 4;
        let m = inv_message(&a);
        assert_eq!(m.len(), idle.len() + 2 + 2 + 1, "a craft costs five bytes more (the recipe number takes the byte the idle message has for 0)");
        let mut b = bag(&[], 5);
        apply_inv(&m, &mut b).unwrap();
        assert_eq!((b.crafting, b.craft_todo), (3, 4));
        assert!((b.craft_left - 7.5).abs() < 0.11 && (b.craft_total - 12.0).abs() < 0.11, "{b:?}");
        let h = hash(&a);
        a.craft_left = 6.1; // (the seconds left count down on every tick: not a change)
        assert_eq!(hash(&a), h);
        a.craft_todo = 3;
        assert_ne!(hash(&a), h);
        // a message without a craft clears it, and an old message that ends after the slots is read as no craft
        apply_inv(&idle, &mut b).unwrap();
        assert_eq!((b.crafting, b.craft_todo, b.craft_left), (0, 0, 0.0));
        let mut old = idle.clone();
        old.truncate(idle.len() - 1);
        b.crafting = 9;
        apply_inv(&old, &mut b).unwrap();
        assert_eq!(b.crafting, 0);
    }

    #[test]
    fn items_move_stack_split_and_swap() {
        let t = table();
        // into an empty slot, whole and part
        let mut a = bag(&[(0, 1, 8)], 8);
        assert!(move_items(&mut a, None, &t, 0, 3, 0));
        assert_eq!((a.id[0], a.id[3], a.count[3]), (0, 1, 8));
        assert!(move_items(&mut a, None, &t, 3, 5, 3));
        assert_eq!((a.count[3], a.count[5]), (5, 3));
        // onto a stack: up to its size, the rest stays
        assert!(move_items(&mut a, None, &t, 3, 5, 0));
        assert_eq!((a.count[3], a.count[5]), (0, 8));
        let mut a = bag(&[(0, 1, 8), (1, 1, 7)], 8);
        assert!(move_items(&mut a, None, &t, 0, 1, 0));
        assert_eq!((a.count[0], a.count[1]), (5, 10), "the stack is 10: 3 went, 5 stayed");
        assert!(!move_items(&mut a, None, &t, 0, 1, 0), "the other stack is full");
        // a whole stack of one thing swaps with another thing, a part does not
        let mut a = bag(&[(0, 1, 8), (1, 2, 1)], 8);
        assert!(!move_items(&mut a, None, &t, 0, 1, 3));
        assert!(move_items(&mut a, None, &t, 0, 1, 0));
        assert_eq!((a.id[0], a.id[1], a.count[1]), (2, 1, 8));
        // between two inventories
        let mut a = bag(&[(0, 1, 8)], 8);
        let mut b = bag(&[], 4);
        assert!(move_items(&mut a, Some(&mut b), &t, 0, 2, 5));
        assert_eq!((a.count[0], b.count[2]), (3, 5));
        assert!(!move_items(&mut a, Some(&mut b), &t, 0, 9, 1), "out of range");
        assert!(!move_items(&mut a, Some(&mut b), &t, 4, 0, 1), "nothing there");
    }
}
