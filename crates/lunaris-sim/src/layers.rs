//! Collision layers (docs/PHYSICS.md): who collides with whom. A [`CollisionLayer`] gives an entity a `layer` (0 to 31) and a `mask` (the layers it collides with);
//! two entities touch only when **both** masks have the other's layer. An entity without the component is layer 0 with a full mask, so a world that never
//! uses layers takes exactly the old paths (no pair is ever skipped, and nothing here costs more than one query that finds nothing).
//!
//! Every system that asks "does this touch that?" goes through [`Filter::collides`] (contacts, triggers, characters, navigation, wheels) or, for a question
//! that only one side asks (a ray, a sensor's line of sight), through [`Filter::sees`], which looks at the asker's mask alone.
use crate::components::CollisionLayer;
use lunaris_ecs::World;

/// The mask that every layer is in.
pub const ALL: u32 = u32::MAX;

/// What an entity says about itself: its layer and the layers it collides with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Filter {
    pub layer: u8,
    pub mask: u32,
}

impl Default for Filter {
    fn default() -> Filter {
        Filter::DEFAULT
    }
}

impl Filter {
    /// Layer 0, collides with everything: what an entity without a `CollisionLayer` is.
    pub const DEFAULT: Filter = Filter { layer: 0, mask: ALL };

    /// A filter from the numbers of a scene (a `layer` above 31 counts as 31).
    pub fn new(layer: u32, mask: u32) -> Filter {
        Filter { layer: layer.min(31) as u8, mask }
    }

    pub fn of(c: &CollisionLayer) -> Filter {
        Filter::new(c.layer, c.mask)
    }

    /// A number for the layer and the mask.
    #[inline]
    pub(crate) fn digest(self) -> u64 {
        ((self.layer as u64) << 32) | self.mask as u64
    }

    /// The bit of this filter's layer in a mask.
    #[inline]
    pub fn bit(self) -> u32 {
        1u32 << self.layer
    }

    /// Do the two collide? Both masks must have the other's layer.
    #[inline]
    pub fn collides(self, other: Filter) -> bool {
        (self.mask >> other.layer) & 1 != 0 && (other.mask >> self.layer) & 1 != 0
    }

    /// Does this filter's mask have the other's layer? (One side only: a ray or a line of sight asks this, the thing it looks at is not asked.)
    #[inline]
    pub fn sees(self, other: Filter) -> bool {
        (self.mask >> other.layer) & 1 != 0
    }
}

/// The layers of a world: one `(uid, filter)` per entity that has a `CollisionLayer`, in uid order. Empty (and free) for a world that has none.
#[derive(Clone, Default)]
pub struct Layers {
    list: Vec<(u64, Filter)>,
}

impl Layers {
    pub fn gather(world: &mut World) -> Layers {
        let mut list: Vec<(u64, Filter)> = Vec::new();
        let _ = world.for_each_uid::<(&CollisionLayer,), _>(|_, uid, (c,)| list.push((uid, Filter::of(c))));
        list.sort_by_key(|l| l.0);
        Layers { list }
    }

    /// A number that is the same for two worlds with the same layers and different when one entity's layers differ (a gathering of the static world that is kept between steps asks whether it is still the same).
    pub(crate) fn digest(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for (uid, f) in &self.list {
            h = (h ^ uid).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ f.digest()).wrapping_mul(0x0000_0100_0000_01b3);
            h ^= h >> 29;
        }
        h
    }

    /// No entity has a `CollisionLayer`: nothing is filtered anywhere.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The filter of entity `uid` (the default for one without a `CollisionLayer`).
    #[inline]
    pub fn of(&self, uid: u64) -> Filter {
        if self.list.is_empty() {
            return Filter::DEFAULT;
        }
        match self.list.binary_search_by_key(&uid, |l| l.0) {
            Ok(i) => self.list[i].1,
            Err(_) => Filter::DEFAULT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_masks_must_agree() {
        let a = Filter::new(1, 1 << 2);
        let b = Filter::new(2, ALL);
        assert!(a.collides(b) && b.collides(a), "a wants b's layer and b takes everything");
        let c = Filter::new(2, 1 << 5);
        assert!(!a.collides(c) && !c.collides(a), "c's mask leaves a's layer out: nobody collides");
        assert!(a.sees(c) && !c.sees(a), "a ray asked by a sees c; asked by c it does not see a");
        assert!(Filter::DEFAULT.collides(Filter::DEFAULT));
        assert!(Filter::DEFAULT.collides(Filter::new(31, ALL)));
    }

    #[test]
    fn a_layer_above_31_counts_as_31() {
        assert_eq!(Filter::new(99, ALL).layer, 31);
        assert_eq!(Filter::new(31, ALL).bit(), 1 << 31);
        assert!(!Filter::new(0, 0).collides(Filter::DEFAULT), "mask 0 collides with nothing");
    }
}
