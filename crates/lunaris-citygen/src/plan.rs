//! The plan of a city that has no edge: a grid of blocks (street `k` runs at `k` times the pitch, block `(i, j)` lies between streets `i` and `i + 1`), every sixth street an avenue, a land use for every block from smooth
//! noise and a few downtown centres, lots along the edges of every block, and a kind of building for each lot. Everything is a function of the seed and the block's own indices, so any place can be made without making
//! its neighbours, in any order, any number of times.

use crate::building::{Kind, Lot, Side};
use crate::mesh::*;
use crate::polar::Polar;
use crate::rng::Rng;
use crate::street::Section;

/// A hash of three integers to [0, 1).
pub(crate) fn hash01(seed: u64, a: i64, b: i64, c: u64) -> f32 {
    Rng::keyed(&[seed, a as u64, b as u64, c]).f32()
}

/// Smooth value noise in [0, 1): hashed values at the lattice points, smoothly blended.
fn value_noise(seed: u64, x: f32, z: f32, salt: u64) -> f32 {
    let (xi, zi) = (x.floor(), z.floor());
    let (fx, fz) = (x - xi, z - zi);
    let (sx, sz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let v = |dx: i64, dz: i64| hash01(seed, xi as i64 + dx, zi as i64 + dz, salt);
    let a = lerp(v(0, 0), v(1, 0), sx);
    let b = lerp(v(0, 1), v(1, 1), sx);
    lerp(a, b, sz)
}

fn fbm(seed: u64, x: f32, z: f32, salt: u64) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut f = 1.0;
    for o in 0..4 {
        sum += amp * value_noise(seed, x * f, z * f, salt + o);
        amp *= 0.5;
        f *= 2.0;
    }
    sum / 0.9375
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LandUse {
    Downtown,
    Midrise,
    Residential,
    Outskirts,
    Industrial,
    Park,
    /// The apron between a round city's last street and its wall: paving, no buildings.
    Rim,
}

/// One lot of a block, decided: where it is, what is built on it, and the key everything about the building is made from.
#[derive(Clone, Copy, Debug)]
pub struct LotPlan {
    pub lot: Lot,
    pub kind: Kind,
    pub key: u64,
    pub index: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Plan {
    pub seed: u64,
    pub section: Section,
    pub block: f32,
    /// Blocks (i0..=i1, j0..=j1) that are one big plaza (a game keeps the middle of its map for a landmark): no building, a paved park with a fountain.
    pub plaza: Option<(i32, i32, i32, i32)>,
    /// The city is round: only the blocks whose middle is within this many metres of the origin exist (a game keeps its map under a dome and a wall: `docs/42-the-dome-city.md` of the game).
    pub dome: Option<f32>,
    /// The city is round and its streets are rings and radii (`polar.rs`): blocks are named (band, sector).
    pub polar: Option<Polar>,
}

/// The `"ruins": [[band, sector], ...]` of a city's `city.json` (blocks of the round city given over to rubble); anything else in it is skipped.
pub fn ruins_from_json(doc: &lunaris_json::Json) -> Vec<(i32, i32)> {
    let Some(lunaris_json::Json::Arr(list)) = doc.get("ruins") else { return Vec::new() };
    list.iter()
        .filter_map(|e| match e {
            lunaris_json::Json::Arr(a) if a.len() == 2 => Some((a[0].as_i64()? as i32, a[1].as_i64()? as i32)),
            _ => None,
        })
        .collect()
}

/// How many blocks along a side make a streaming cell.
pub const CELL_BLOCKS: i32 = 4;

impl Plan {
    pub fn new(seed: u64) -> Plan {
        Plan { seed, section: Section::default(), block: 84.0, plaza: None, dome: None, polar: None }
    }

    /// The same plan with blocks `i0..=i1` by `j0..=j1` given over to a plaza.
    pub fn with_plaza(mut self, area: Option<(i32, i32, i32, i32)>) -> Plan {
        self.plaza = area;
        self
    }

    /// The same plan cut round at `radius` metres from the origin (`None`: no edge).
    pub fn with_dome(mut self, radius: Option<f32>) -> Plan {
        self.dome = radius;
        self
    }

    /// The same plan with its streets as rings and radii.
    pub fn with_polar(mut self, polar: Option<Polar>) -> Plan {
        self.polar = polar;
        self
    }

    /// The same plan with the polar blocks `(band, sector)` of the list (at most 24) given over to rubble: a destroyed district (`LandUse::Park` with `ParkKind::Ruin`: no lots, no trees, a dirt ground; the game's builder dresses it).
    /// A block that does not exist is ignored; a plan that is not polar keeps no ruins.
    pub fn with_ruins(mut self, list: &[(i32, i32)]) -> Plan {
        if let Some(p) = self.polar.as_mut() {
            p.ruins = [0; 24];
            let mut k = 0;
            for &(b, s) in list {
                if k < p.ruins.len() && (0..crate::polar::BANDS).contains(&b) && s >= 0 && (s as u32) < p.counts[b as usize] && s < 256 {
                    p.ruins[k] = crate::polar::Polar::ruin_code(b, s);
                    k += 1;
                }
            }
        }
        self
    }

    /// Whether block (i, j) is a ruin (see `with_ruins`).
    pub fn is_ruin(&self, i: i32, j: i32) -> bool {
        self.polar.is_some_and(|p| p.is_ruin(i, j))
    }

    /// Whether block (i, j) is part of the city: its middle is within the dome's radius (always, when there is no dome); a polar city has the blocks it names.
    pub fn inside(&self, i: i32, j: i32) -> bool {
        if let Some(p) = self.polar {
            return p.valid(i, j);
        }
        let Some(r) = self.dome else { return true };
        let (x0, x1, z0, z1) = self.block_rect(i, j);
        let (cx, cz) = ((x0 + x1) * 0.5, (z0 + z1) * 0.5);
        (cx * cx + cz * cz).sqrt() <= r
    }

    /// Whether any block of the far cell (ci, cj) is part of the city (a round city has no far cells where it has no blocks).
    pub fn cell_inside(&self, ci: i32, cj: i32) -> bool {
        if self.polar.is_some() {
            return !self.blocks_in_cell(self.cell_size(), ci, cj).is_empty();
        }
        self.dome.is_none() || (cj * CELL_BLOCKS..(cj + 1) * CELL_BLOCKS).any(|bj| (ci * CELL_BLOCKS..(ci + 1) * CELL_BLOCKS).any(|bi| self.inside(bi, bj)))
    }

    pub fn in_plaza(&self, i: i32, j: i32) -> bool {
        if self.polar.is_some() {
            return i < 0;
        }
        self.plaza.is_some_and(|(i0, i1, j0, j1)| (i0..=i1).contains(&i) && (j0..=j1).contains(&j))
    }

    /// The distance between two streets (the same both ways).
    pub fn pitch(&self) -> f32 {
        self.block + self.section.total()
    }

    pub fn cell_size(&self) -> f32 {
        self.pitch() * CELL_BLOCKS as f32
    }

    /// Every sixth street is an avenue: its carriageway is wider (and the blocks beside it are narrower by the same amount, the pitch staying what it is).
    pub fn extra(&self, k: i32) -> f32 {
        if k.rem_euclid(6) == 0 { 3.5 } else { 0.0 }
    }

    /// The carriageway of street `k` (either direction).
    pub fn carriageway(&self, k: i32) -> f32 {
        self.section.carriageway + 2.0 * self.extra(k)
    }

    /// Block (i, j) as x0, x1, z0, z1 in the world.
    pub fn block_rect(&self, i: i32, j: i32) -> (f32, f32, f32, f32) {
        if self.polar.is_some() {
            // (a polar block's bounding box, for what does not know about the rings)
            let q = self.block_poly(i, j);
            let f = |k: usize, m: fn(f32, f32) -> f32, s: f32| q.iter().map(|p| p[k]).fold(s, m);
            return (f(0, f32::min, f32::MAX), f(0, f32::max, f32::MIN), f(1, f32::min, f32::MAX), f(1, f32::max, f32::MIN));
        }
        let (p, st) = (self.pitch(), self.section.total());
        let x0 = i as f32 * p + st * 0.5 + self.extra(i);
        let x1 = (i + 1) as f32 * p - st * 0.5 - self.extra(i + 1);
        let z0 = j as f32 * p + st * 0.5 + self.extra(j);
        let z1 = (j + 1) as f32 * p - st * 0.5 - self.extra(j + 1);
        (x0, x1, z0, z1)
    }

    /// How built-up the land is at a block, 0 to 1: high near the downtown centres (one a jittered lattice cell of 6 km, strengths and sizes of their own), with noise on top.
    pub fn density(&self, i: i32, j: i32) -> f32 {
        if self.polar.is_some() {
            return (0.95 - 0.16 * i.max(0) as f32).clamp(0.05, 1.0);
        }
        let (x, z) = ((i as f32 + 0.5) * self.pitch(), (j as f32 + 0.5) * self.pitch());
        let lattice = 6000.0;
        let (cx, cz) = ((x / lattice).floor() as i64, (z / lattice).floor() as i64);
        let mut best = 0.0f32;
        for dz in -1..=1 {
            for dx in -1..=1 {
                let (gx, gz) = (cx + dx, cz + dz);
                let px = (gx as f32 + 0.2 + 0.6 * hash01(self.seed, gx, gz, 11)) * lattice;
                let pz = (gz as f32 + 0.2 + 0.6 * hash01(self.seed, gx, gz, 12)) * lattice;
                let strength = 0.55 + 0.45 * hash01(self.seed, gx, gz, 13);
                let sigma = 900.0 + 1700.0 * hash01(self.seed, gx, gz, 14);
                let d2 = ((x - px).powi(2) + (z - pz).powi(2)) / (sigma * sigma);
                best = best.max(strength * (-d2).exp());
            }
        }
        let n = fbm(self.seed, x / 1500.0, z / 1500.0, 21);
        (best * 0.9 + (n - 0.5) * 0.5 + 0.12).clamp(0.0, 1.0)
    }

    pub fn landuse(&self, i: i32, j: i32) -> LandUse {
        if self.polar.is_some() {
            return self.polar_landuse(i, j);
        }
        if self.in_plaza(i, j) {
            return LandUse::Park;
        }
        let d = self.density(i, j);
        let (x, z) = ((i as f32 + 0.5) * self.pitch(), (j as f32 + 0.5) * self.pitch());
        if hash01(self.seed, i as i64, j as i64, 31) < 0.09 && d < 0.88 {
            return LandUse::Park;
        }
        if d < 0.45 && fbm(self.seed, x / 1200.0, z / 1200.0, 41) > 0.62 {
            return LandUse::Industrial;
        }
        match d {
            d if d > 0.74 => LandUse::Downtown,
            d if d > 0.52 => LandUse::Midrise,
            d if d > 0.30 => LandUse::Residential,
            _ => LandUse::Outskirts,
        }
    }

    /// The mix of kinds for a land use: [tower, masonry, apartment, shophouse, house, warehouse, skyscraper].
    pub(crate) fn mix(&self, u: LandUse) -> [f32; 7] {
        match u {
            LandUse::Downtown => [0.12, 0.14, 0.06, 0.04, 0.0, 0.0, 0.64],
            LandUse::Midrise => [0.12, 0.32, 0.30, 0.18, 0.02, 0.02, 0.04],
            LandUse::Residential => [0.0, 0.10, 0.25, 0.15, 0.48, 0.02, 0.0],
            LandUse::Outskirts => [0.0, 0.03, 0.10, 0.10, 0.72, 0.05, 0.0],
            LandUse::Industrial => [0.0, 0.10, 0.0, 0.07, 0.0, 0.83, 0.0],
            LandUse::Park | LandUse::Rim => [0.0; 7],
        }
    }

    /// The lots of block (i, j), each with its kind and key.
    pub fn lots(&self, i: i32, j: i32) -> Vec<LotPlan> {
        if self.polar.is_some() {
            return self.polar_lots(i, j);
        }
        let use_ = self.landuse(i, j);
        if use_ == LandUse::Park {
            return Vec::new();
        }
        let (x0, x1, z0, z1) = self.block_rect(i, j);
        let y = self.section.kerb_h;
        let mix = self.mix(use_);
        let mut rng = Rng::keyed(&[self.seed, i as u64, j as u64, 1]);
        // in the heart of a downtown some blocks are given over to the big ones: their lots are deeper
        let core = use_ == LandUse::Downtown && self.density(i, j) > 0.82 && hash01(self.seed, i as i64, j as i64, 51) < 0.55;
        let (dn, ds) = if core { (rng.range(27.0, 34.0), rng.range(27.0, 34.0)) } else { (rng.range(19.0, 26.0), rng.range(19.0, 26.0)) };
        let (dw, de) = (rng.range(16.0, 22.0), rng.range(16.0, 22.0));
        let mut out: Vec<LotPlan> = Vec::new();
        let mut strip = |rng: &mut Rng, o: V3, ex: V3, ez: V3, length: f32, depth: f32, end_street: bool| {
            let mut x = 0.0;
            while length - x > 5.0 {
                let mut kind = [Kind::Tower, Kind::Masonry, Kind::Apartment, Kind::Shophouse, Kind::House, Kind::Warehouse, Kind::Skyscraper][rng.weighted(&mix)];
                let d = depth * rng.range(0.85, 1.0);
                // a house needs a lot deep enough for its yard in front and a floor plan behind
                if kind == Kind::House && d < 17.0 {
                    kind = Kind::Shophouse;
                }
                let mut w = match kind {
                    Kind::Tower => rng.range(26.0, 42.0),
                    Kind::Masonry => rng.range(12.0, 24.0),
                    Kind::Apartment => rng.range(16.0, 30.0),
                    Kind::Shophouse => rng.range(6.5, 11.5),
                    Kind::House => rng.range(13.0, 19.0),
                    Kind::Warehouse => rng.range(24.0, 44.0),
                    Kind::Skyscraper => rng.range(30.0, 46.0),
                };
                if length - x - w < 9.0 {
                    w = length - x;
                }
                if kind == Kind::Tower && w < 22.0 {
                    kind = Kind::Masonry;
                }
                // a skyscraper needs a lot it can stand on: wide enough, and deep enough for its podium and its tiers
                if kind == Kind::Skyscraper && (w < 24.0 || d < 22.0) {
                    kind = if w >= 22.0 { Kind::Tower } else { Kind::Masonry };
                }
                let first = x == 0.0;
                let last = (length - x - w) < 0.01;
                let left = if first && end_street { Side::Street } else { Side::Party };
                let right = if last && end_street { Side::Street } else { Side::Party };
                let index = out.len() as u32;
                let key = Rng::keyed(&[self.seed, i as u64, j as u64, index as u64, 0x4C4F54]).u64();
                out.push(LotPlan { lot: Lot { o: add(o, scale(ex, x)), ex, ez, w, d, sides: [Side::Street, right, Side::Open, left] }, kind, key, index });
                x += w;
            }
        };
        strip(&mut rng, [x0, y, z0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], x1 - x0, dn, true);
        strip(&mut rng, [x1, y, z1], [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0], x1 - x0, ds, true);
        strip(&mut rng, [x0, y, z0 + dn], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], (z1 - z0) - dn - ds, dw, false);
        strip(&mut rng, [x1, y, z1 - ds], [0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], (z1 - z0) - dn - ds, de, false);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_block_is_planned_the_same_way_every_time_and_blocks_fit_between_their_streets() {
        let p = Plan::new(5);
        for (i, j) in [(0, 0), (-7, 12), (123, -456)] {
            let a = p.lots(i, j);
            let b = p.lots(i, j);
            assert_eq!(a.len(), b.len());
            for (x, y) in a.iter().zip(&b) {
                assert_eq!(x.key, y.key);
                assert_eq!(x.kind, y.kind);
            }
            let (x0, x1, z0, z1) = p.block_rect(i, j);
            assert!(x1 > x0 + 60.0 && z1 > z0 + 60.0, "{x0} {x1} {z0} {z1}");
            for l in &a {
                for c in l.lot.corners() {
                    assert!(c[0] > x0 - 0.05 && c[0] < x1 + 0.05 && c[2] > z0 - 0.05 && c[2] < z1 + 0.05);
                }
            }
        }
        // neighbouring blocks never overlap: the gap between them is the street
        let (_, a1, _, _) = p.block_rect(3, 3);
        let (b0, _, _, _) = p.block_rect(4, 3);
        assert!((b0 - a1 - (p.section.total() + p.extra(3 + 1) * 2.0)).abs() < 0.01, "{}", b0 - a1);
    }

    #[test]
    fn a_city_has_every_kind_of_land_somewhere_and_its_downtowns_are_dense() {
        let p = Plan::new(7);
        let mut seen = std::collections::HashMap::new();
        let mut downtown_density = 0.0f32;
        let mut n = 0;
        for j in -40..40 {
            for i in -40..40 {
                *seen.entry(p.landuse(i, j)).or_insert(0) += 1;
                let d = p.density(i, j);
                assert!((0.0..=1.0).contains(&d));
                if p.landuse(i, j) == LandUse::Downtown {
                    downtown_density += d;
                    n += 1;
                }
            }
        }
        for u in [LandUse::Downtown, LandUse::Midrise, LandUse::Residential, LandUse::Outskirts, LandUse::Park] {
            assert!(seen.get(&u).copied().unwrap_or(0) > 20, "{u:?}: {seen:?}");
        }
        assert!(n > 0 && downtown_density / n as f32 > 0.74);
    }
}
