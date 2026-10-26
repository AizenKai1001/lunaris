//! What a character wears, seen by the other players (docs/GEAR_VISIBLE.md). `Equipment` (docs/EQUIPMENT.md) says which places of a bag are worn places and what is in them protects the character; this is the picture of it.
//!
//! **The server** keeps a mask on each character that has a [`WornTell`]: bit `k` is set while the `k`-th worn place of its `Equipment` (the order of the text) has an item in it. The net host (`SimGame`) tells the
//! mask to the other players in the low bits of the character's `anim` (7 bits at most, so a record that carried `anim` 0 is exactly as long as one that carries a mask: nothing is added to the wire), and it
//! replaces whatever `anim` the player itself reported: the player says what it holds, the server says what it wears. A piece that breaks, is taken off or dies with its owner leaves the bag, so its bit goes at
//! the next step; the mask is worked out from the bag in the step, not by the clients' word.
//!
//! **The client** dresses the mannequin of another player with [`WornPart`]s: entities that have a `MeshRenderer` (the piece's model, hidden to begin with) and are children of the mannequin, fixed to one of its
//! joints with `BoneAttach` (no `target`: the model they are a child of). `place` is the bit of the mask that shows the part. A part is drawn while that bit is set in the `NetPuppet.anim` of its mannequin AND the
//! mannequin itself is drawn (`MeshRenderer.visible`, which a game's level-of-detail rules switch: a far puppet is a box, a hidden mannequin has no gear and costs nothing). It only ever turns a part on that
//! the mannequin shows, so the game's own rules over the mannequin stay in charge.
//!
//! Cost: the server reads one component of each character a step and writes the mask when it changed; a client looks at each part once a step, and works the mannequin out once for the parts that follow each
//! other (a puppet's parts are made together), writing only when a part changes. Nothing here is random.
use crate::armor::places;
use crate::components::{MeshRenderer, NetPuppet, Parent};
use crate::inventory::Inventory;
use lunaris_ecs::{component, Entity, World};
use std::cell::RefCell;

/// The most worn places a mask can tell: the low 7 bits of `anim`, which a record writes in one byte.
pub const MAX_PLACES: usize = 7;
/// The bits of `anim` a mask uses.
pub const MASK: u32 = (1 << MAX_PLACES) - 1;

component! {
    /// On a character that has an `Equipment` and an `Inventory` (the avatar of a game server): the server tells the other players what it wears. `mask` is the engine's: bit `k` is set while the `k`-th worn place of
    /// the character's `Equipment` has an item in it (docs/GEAR_VISIBLE.md); the net host puts it in the low 7 bits of the `anim` the other players are told about this character (what the character's own client
    /// said in `anim` is replaced), and a client's `NetPuppet.anim & 127` is that mask for another player. A character with no `WornTell` is told as before.
    pub struct WornTell {
        pub mask: u32 = 0,
    }
}

component! {
    /// A piece of gear on the mannequin of another player (docs/GEAR_VISIBLE.md), a child of the mannequin with a `MeshRenderer` and a `BoneAttach` (no `target`: the model it is a child of). It is drawn
    /// while bit `place` of the mask in the mannequin's `NetPuppet.anim` is set and the mannequin itself is drawn; the engine writes its `MeshRenderer.visible` every step (author it hidden).
    pub struct WornPart {
        pub place: u32 = 0,
    }
}

/// The mask of the character `e`: bit `k` for each of the first [`MAX_PLACES`] worn places of its `Equipment` that holds an item. 0 for a character with no bag or no places.
pub fn mask_of(world: &World, e: Entity) -> u32 {
    let Some(inv) = world.get::<Inventory>(e) else { return 0 };
    let all = places(world, e);
    let mut mask = 0;
    for (k, p) in all.iter().enumerate().take(MAX_PLACES) {
        if p.slot < inv.size as usize && inv.id[p.slot] != 0 && inv.count[p.slot] != 0 {
            mask |= 1 << k;
        }
    }
    mask
}

thread_local! {
    /// The parts seen this step (kept between steps so that a crowd does not allocate each one).
    static PARTS: RefCell<Vec<(Entity, u64, u32, bool)>> = const { RefCell::new(Vec::new()) };
}

/// Once a step, after the game's systems: the server's masks, and the gear of the puppets on a client.
pub fn update(world: &mut World) {
    tell(world);
    show(world);
}

/// Keep `WornTell.mask` up to date (only a write when it changed).
fn tell(world: &mut World) {
    let mut found: Vec<(Entity, u32)> = Vec::new();
    let _ = world.for_each::<(&WornTell,), _>(|e, (t,)| found.push((e, t.mask)));
    for (e, was) in found {
        let now = mask_of(world, e);
        if now != was {
            if let Some(t) = world.get_mut::<WornTell>(e) {
                t.mask = now;
            }
        }
    }
}

/// Draw the parts whose bit is set on a mannequin that is drawn, hide the others.
fn show(world: &mut World) {
    let mut parts = PARTS.with(|p| std::mem::take(&mut *p.borrow_mut()));
    parts.clear();
    let _ = world.for_each::<(&WornPart, &Parent, &MeshRenderer), _>(|e, (w, p, m)| parts.push((e, p.target.0, w.place, m.visible)));
    // (the parts of one mannequin are made together, so they come one after the other: its state is looked up once for the run)
    let mut run: (u64, bool, u32) = (0, false, 0);
    for &(e, parent, place, was) in parts.iter() {
        if run.0 != parent || run.0 == 0 {
            run = match world.entity(parent) {
                Some(pe) => (parent, world.get::<MeshRenderer>(pe).is_some_and(|m| m.visible), world.get::<NetPuppet>(pe).map_or(0, |n| n.anim & MASK)),
                None => (parent, false, 0),
            };
        }
        let want = run.1 && (place as usize) < MAX_PLACES && run.2 >> place & 1 == 1;
        if want != was {
            if let Some(m) = world.get_mut::<MeshRenderer>(e) {
                m.visible = want;
            }
        }
    }
    parts.clear();
    PARTS.with(|p| *p.borrow_mut() = parts);
}
