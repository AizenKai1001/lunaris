//! The ground a creature walks on: a height and a slope at every place. The trainer's terrains are heightfields of `TERRAIN_N` x `TERRAIN_N` heights `TERRAIN_CELL` metres apart, centred on the start
//! (bilinear, flat outside by clamping to the edge); a game gives its own ground by implementing `Ground` (a Lunaris terrain, a floor, a mesh).
use crate::vecmath::{norm3, v3, V3};

pub const TERRAIN_N: usize = 49;
pub const TERRAIN_CELL: f32 = 0.5;

/// What the creature needs to know about the ground.
pub trait Ground {
    /// The height of the ground at (x, z).
    fn height(&self, x: f32, z: f32) -> f32;
    /// The unit normal of the ground at (x, z) (pointing up). The default is the one the trainer used: the slope from the heights a little to each side, so that it changes smoothly.
    fn normal(&self, x: f32, z: f32) -> V3 {
        let e = 0.15f32;
        let dx = (self.height(x + e, z) - self.height(x - e, z)) / (2.0 * e);
        let dz = (self.height(x, z + e) - self.height(x, z - e)) / (2.0 * e);
        norm3(v3(-dx, 1.0, -dz))
    }
    /// The nearest solid that is not the heightfield (a wall, a ramp, a platform): its signed distance (negative inside) and outward unit normal at `p`; `None` where the world has none.
    fn solid(&self, _p: V3) -> Option<(f32, V3)> {
        None
    }
}

/// A ground with solids standing on it (walls, ramps, ceilings: `Solids`).
pub struct WithSolids<'a> {
    pub base: &'a dyn Ground,
    pub solids: crate::solid::Solids,
}

impl Ground for WithSolids<'_> {
    fn height(&self, x: f32, z: f32) -> f32 {
        self.base.height(x, z)
    }
    fn normal(&self, x: f32, z: f32) -> V3 {
        self.base.normal(x, z)
    }
    fn solid(&self, p: V3) -> Option<(f32, V3)> {
        self.solids.nearest(p)
    }
}

/// The flat ground at height 0.
pub struct Flat;

impl Ground for Flat {
    fn height(&self, _x: f32, _z: f32) -> f32 {
        0.0
    }
    fn normal(&self, _x: f32, _z: f32) -> V3 {
        v3(0.0, 1.0, 0.0)
    }
}

/// A heightfield of the trainer's kind (`TERRAIN_N` x `TERRAIN_N` heights, row z, column x), as in `ground_height` and `ground_normal` of `creature_core.h`.
pub struct Grid<'a> {
    pub h: &'a [f32],
}

impl Grid<'_> {
    fn at(&self, i: i32, j: i32) -> f32 {
        let n = TERRAIN_N as i32 - 1;
        let i = i.clamp(0, n) as usize;
        let j = j.clamp(0, n) as usize;
        self.h[j * TERRAIN_N + i]
    }
}

impl Ground for Grid<'_> {
    fn height(&self, x: f32, z: f32) -> f32 {
        let half = (TERRAIN_N as f32 - 1.0) * 0.5;
        let fx = x / TERRAIN_CELL + half;
        let fz = z / TERRAIN_CELL + half;
        let flx = fx.floor();
        let flz = fz.floor();
        let (i, j) = (flx as i32, flz as i32);
        let (tx, tz) = (fx - flx, fz - flz);
        let (h00, h10, h01, h11) = (self.at(i, j), self.at(i + 1, j), self.at(i, j + 1), self.at(i + 1, j + 1));
        (h00 * (1.0 - tx) + h10 * tx) * (1.0 - tz) + (h01 * (1.0 - tx) + h11 * tx) * tz
    }
    fn normal(&self, x: f32, z: f32) -> V3 {
        let e = 0.15f32;
        let dx = (self.height(x + e, z) - self.height(x - e, z)) / (2.0 * e);
        let dz = (self.height(x, z + e) - self.height(x, z - e)) / (2.0 * e);
        norm3(v3(-dx, 1.0, -dz))
    }
}
