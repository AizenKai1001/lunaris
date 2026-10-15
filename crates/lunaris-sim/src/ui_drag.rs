//! Drag and drop: `UiDrag` on a `Ui` button follows the pointer while the left button is held on it, `UiDrop` marks the places it can be dropped on (inventory slots, equipment, a bin), and the item goes
//! into the slot it is dropped on (trading places with what is there) or snaps back. See `docs/UI.md`, "Drag and drop".
use crate::components::{Ui, UiDrag, UiDrop};
use crate::ui_kit::pointer;
use lunaris_ecs::{Entity, World};

/// How far (a fraction of the picture's height) a dragged thing has to be from where it was taken before the drag is a drag and not a click that wobbled.
const MOVED: f32 = 0.006;

fn rect_of(u: &Ui) -> [f32; 4] {
    [u.pos[0], u.pos[1], u.pos[0] + u.size[0], u.pos[1] + u.size[1]]
}

fn inside(r: [f32; 4], p: (f32, f32)) -> bool {
    p.0 >= r[0] && p.0 <= r[2] && p.1 >= r[1] && p.1 <= r[3]
}

fn takes(slot: &UiDrop, kind: u32) -> bool {
    slot.accept & (1u32 << (kind & 31)) != 0
}

/// The resource a slot's drops are reported in: `drop.NAME`, with `NAME` the slot's `name`, or its uid when it has none.
fn slot_name(world: &World, uid: u64, d: &UiDrop) -> String {
    let n = world.strings().get(d.name.0);
    if n.is_empty() {
        uid.to_string()
    } else {
        n.to_string()
    }
}

struct Item {
    uid: u64,
    e: Entity,
    d: UiDrag,
    u: Ui,
}

struct Slot {
    uid: u64,
    e: Entity,
    d: UiDrop,
    u: Ui,
}

/// The place the item sits in when it is not held: its slot's middle, or its home.
fn rest_pos(it: &Item, slots: &[Slot]) -> Option<[f32; 2]> {
    match slots.iter().find(|s| s.uid == it.d.slot.0) {
        Some(s) => Some([s.u.pos[0] + (s.u.size[0] - it.u.size[0]) * 0.5, s.u.pos[1] + (s.u.size[1] - it.u.size[1]) * 0.5]),
        None => it.d.home_set.then_some(it.d.home),
    }
}

pub(crate) fn update(world: &mut World) {
    let mut items: Vec<Item> = Vec::new();
    let _ = world.for_each_uid::<(&UiDrag, &Ui), _>(|e, uid, (d, u)| items.push(Item { uid, e, d: *d, u: *u }));
    let mut slots: Vec<Slot> = Vec::new();
    let _ = world.for_each_uid::<(&UiDrop, &Ui), _>(|e, uid, (d, u)| slots.push(Slot { uid, e, d: *d, u: *u }));
    if items.is_empty() && slots.is_empty() {
        return;
    }
    items.sort_by_key(|i| i.uid);
    slots.sort_by_key(|s| s.uid);
    let hidden = crate::ui_kit3::hidden_set(world);
    let ptr = pointer(world);
    let down = world.resource("input.mouse_left").is_some_and(|v| v > 0.5);
    // what a slot reported last tick is over: the resources go back to nothing in between two drops (the count stays)
    for s in slots.iter_mut() {
        if s.d.dropped {
            let name = slot_name(world, s.uid, &s.d);
            let _ = world.set_resource(&format!("drop.{name}"), Some(0.0));
            let _ = world.set_resource(&format!("drop.{name}.item"), Some(0.0));
            s.d.dropped = false;
        }
        s.d.over = false;
    }
    // the slot that is under the pointer and takes a thing of this kind: the topmost by layer, then by uid
    let target_of = |slots: &[Slot], kind: u32, p: (f32, f32)| -> Option<usize> {
        slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.u.visible && !hidden.contains(&s.uid) && inside(rect_of(&s.u), p) && takes(&s.d, kind))
            .max_by_key(|(_, s)| (s.u.layer, s.uid))
            .map(|(i, _)| i)
    };
    for k in 0..items.len() {
        let (uid, mut d, mut u) = (items[k].uid, items[k].d, items[k].u);
        let press_edge = u.pressed && !d.down;
        if !d.home_set && d.slot.0 == 0 {
            d.home = u.pos;
            d.home_set = true;
        }
        if !d.dragging && press_edge {
            if let Some(p) = ptr {
                d.dragging = true;
                d.moved = false;
                d.grab = [p.0 - u.pos[0], p.1 - u.pos[1]];
                d.layer0 = u.layer;
                u.layer = u.layer.saturating_add(d.lift);
            }
        }
        let mut dropped_on: Option<usize> = None;
        if d.dragging {
            if down {
                if let Some(p) = ptr {
                    u.pos = [p.0 - d.grab[0], p.1 - d.grab[1]];
                    // far from where it sat: a drag, not a click
                    let from = rest_pos(&Item { uid, e: items[k].e, d, u }, &slots).unwrap_or(d.home);
                    if (u.pos[0] - from[0]).abs() > MOVED || (u.pos[1] - from[1]).abs() > MOVED {
                        d.moved = true;
                    }
                    if let Some(i) = target_of(&slots, d.kind, p) {
                        slots[i].d.over = true;
                    }
                }
                if d.moved {
                    u.pressed = false;
                    u.clicked = false;
                }
            } else {
                // let go: where it is dropped, if it was carried anywhere
                d.dragging = false;
                u.layer = d.layer0;
                if d.moved {
                    u.clicked = false;
                    dropped_on = ptr.and_then(|p| target_of(&slots, d.kind, p));
                }
            }
        }
        if let Some(t) = dropped_on {
            let old = slots.iter().position(|s| s.uid == d.slot.0);
            let occupant = items.iter().enumerate().find(|(j, o)| *j != k && o.d.slot.0 == slots[t].uid).map(|(j, _)| j);
            // it may go into an empty slot; into an occupied one only if that thing can take the place this one comes from
            let ok = match (occupant, old) {
                (None, _) => true,
                (Some(_), None) => false,
                (Some(j), Some(o)) => o != t && slots[t].d.swap && takes(&slots[o].d, items[j].d.kind),
            };
            if ok && old != Some(t) {
                if let (Some(j), Some(o)) = (occupant, old) {
                    items[j].d.slot.0 = slots[o].uid;
                }
                d.slot.0 = slots[t].uid;
                slots[t].d.dropped = true;
                slots[t].d.from.0 = uid;
                slots[t].d.item = d.item;
                slots[t].d.count = slots[t].d.count.wrapping_add(1);
                let name = slot_name(world, slots[t].uid, &slots[t].d);
                let _ = world.set_resource(&format!("drop.{name}"), Some(uid as f64));
                let _ = world.set_resource(&format!("drop.{name}.item"), Some(d.item as f64));
                let _ = world.set_resource(&format!("drop.{name}.count"), Some(slots[t].d.count as f64));
            }
        }
        d.down = down;
        items[k].d = d;
        items[k].u = u;
    }
    // where each thing is when it is not held: in the middle of its slot, or at home
    for i in 0..items.len() {
        if items[i].d.dragging {
            continue;
        }
        if let Some(pos) = rest_pos(&items[i], &slots) {
            items[i].u.pos = pos;
        }
    }
    // each slot knows what it holds
    for s in slots.iter_mut() {
        s.d.holds.0 = items.iter().find(|i| i.d.slot.0 == s.uid).map_or(0, |i| i.uid);
    }
    for it in &items {
        if let Some(u) = world.get_mut::<Ui>(it.e) {
            u.pos = it.u.pos;
            u.layer = it.u.layer;
            u.pressed = it.u.pressed;
            u.clicked = it.u.clicked;
        }
        if let Some(d) = world.get_mut::<UiDrag>(it.e) {
            *d = it.d;
        }
    }
    for s in &slots {
        if let Some(d) = world.get_mut::<UiDrop>(s.e) {
            *d = s.d;
        }
    }
}
