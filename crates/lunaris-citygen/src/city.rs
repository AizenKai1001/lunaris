//! A district: a grid of blocks with streets between them, each block ringed by a sidewalk and lined with lots that hold buildings of the district's mix of kinds.

use crate::building::{self, Kind, Lot, Side};
use crate::facade::DoorSpec;
use crate::interior::InLight;
use crate::props::Props;
use crate::materials::Lib;
use crate::mesh::*;
use crate::rng::Rng;
use crate::street::{self, Ground, Section};

#[derive(Clone, Debug)]
pub struct Grid {
    pub nx: u32,
    pub nz: u32,
    pub block_w: f32,
    pub block_d: f32,
    pub section: Section,
    pub seed: u64,
    /// The world position of the first street crossing (the district's north-west corner).
    pub origin: [f32; 2],
}

impl Grid {
    pub fn pitch(&self) -> (f32, f32) {
        let st = self.section.total();
        (self.block_w + st, self.block_d + st)
    }

    /// The block (i, j) as x0, x1, z0, z1 in the world.
    pub fn block(&self, i: u32, j: u32) -> (f32, f32, f32, f32) {
        let (px, pz) = self.pitch();
        let st = self.section.total();
        let x0 = self.origin[0] + i as f32 * px + st * 0.5;
        let z0 = self.origin[1] + j as f32 * pz + st * 0.5;
        (x0, x0 + self.block_w, z0, z0 + self.block_d)
    }
}

/// How dense a block is: 1 at the middle of the district, towards 0 at its edge, with some noise: the mix of building kinds follows it.
fn density(g: &Grid, i: u32, j: u32) -> f32 {
    let cx = (g.nx as f32 - 1.0) * 0.5;
    let cz = (g.nz as f32 - 1.0) * 0.5;
    let d = (((i as f32 - cx) / (cx + 1.0)).powi(2) + ((j as f32 - cz) / (cz + 1.0)).powi(2)).sqrt();
    let noise = Rng::keyed(&[g.seed, i as u64, j as u64, 77]).range(-0.25, 0.25);
    (1.0 - d * 0.8 + noise).clamp(0.0, 1.0)
}

fn pick_kind(rng: &mut Rng, dens: f32) -> Kind {
    // [tower, masonry, apartment, shophouse]
    let w = [0.05 + dens * 0.45, 0.25 + dens * 0.1, 0.30 - dens * 0.1, 0.40 - dens * 0.35];
    [Kind::Tower, Kind::Masonry, Kind::Apartment, Kind::Shophouse][rng.weighted(&w)]
}

fn width_of(rng: &mut Rng, kind: Kind) -> f32 {
    match kind {
        Kind::Tower => rng.range(26.0, 42.0),
        Kind::Skyscraper => rng.range(30.0, 46.0),
        Kind::Masonry => rng.range(12.0, 24.0),
        Kind::Apartment => rng.range(16.0, 30.0),
        Kind::Shophouse => rng.range(6.5, 11.5),
        Kind::House => rng.range(13.0, 19.0),
        Kind::Warehouse => rng.range(24.0, 44.0),
    }
}

/// The lots along one strip of a block. `o` is where the strip starts on the street edge, `ex` runs along it, `ez` goes into the block, `length` is the strip's length, `depth` how deep the lots are;
/// `end_street` says whether the strip's two ends face a street (the first and last lot get a facade there).
#[allow(clippy::too_many_arguments)]
fn strip(lots: &mut Vec<(Lot, Kind, u64)>, rng: &mut Rng, dens: f32, o: V3, ex: V3, ez: V3, length: f32, depth: f32, end_street: bool) {
    let mut x = 0.0;
    let mut k = 0u64;
    while length - x > 5.0 {
        let mut kind = pick_kind(rng, dens);
        let mut w = width_of(rng, kind);
        if length - x - w < 9.0 {
            w = length - x;
        }
        if kind == Kind::Tower && w < 22.0 {
            kind = Kind::Masonry;
        }
        let d = depth * rng.range(0.85, 1.0);
        let first = x == 0.0;
        let last = (length - x - w) < 0.01;
        let left = if first && end_street { Side::Street } else { Side::Party };
        let right = if last && end_street { Side::Street } else { Side::Party };
        let start = add(o, scale(ex, x));
        lots.push((Lot { o: start, ex, ez, w, d, sides: [Side::Street, right, Side::Open, left] }, kind, k));
        x += w;
        k += 1;
    }
}

/// The lots of block (i, j).
pub fn block_lots(g: &Grid, i: u32, j: u32) -> Vec<(Lot, Kind, u64)> {
    let (x0, x1, z0, z1) = g.block(i, j);
    let y = g.section.kerb_h;
    let dens = density(g, i, j);
    let mut rng = Rng::keyed(&[g.seed, i as u64, j as u64, 1]);
    let (dn, ds) = (rng.range(19.0, 26.0), rng.range(19.0, 26.0));
    let (dw, de) = (rng.range(16.0, 22.0), rng.range(16.0, 22.0));
    let mut lots = Vec::new();
    // north: runs east along the north edge, faces north
    strip(&mut lots, &mut rng, dens, [x0, y, z0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], x1 - x0, dn, true);
    // south: runs west along the south edge, faces south
    strip(&mut lots, &mut rng, dens, [x1, y, z1], [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0], x1 - x0, ds, true);
    // west: between the two, faces west
    strip(&mut lots, &mut rng, dens, [x0, y, z0 + dn], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], (z1 - z0) - dn - ds, dw, false);
    // east: faces east
    strip(&mut lots, &mut rng, dens, [x1, y, z1 - ds], [0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], (z1 - z0) - dn - ds, de, false);
    lots
}

/// One building's inside, to be loaded only when somebody is near: all its floors, the lights, the doors, and where it is.
pub struct Inside {
    pub centre: V3,
    pub radius: f32,
    pub height: f32,
    /// Each floor's inside, bottom to top, with the height of the middle of the floor.
    pub floors: Vec<(MeshBuilder, f32)>,
    pub lights: Vec<InLight>,
    pub doors: Vec<DoorSpec>,
}

/// What a district makes: the mesh (streets, sidewalks and the outsides of the buildings) and the inside of each building.
pub struct District {
    pub mesh: MeshBuilder,
    pub buildings: Vec<Inside>,
}

/// The whole district: streets, sidewalks, paint and every building, inside and out.
pub fn district(lib: &Lib, props: &Props, g: &Grid) -> District {
    let mut mb = MeshBuilder::new();
    let mut buildings: Vec<Inside> = Vec::new();
    let mut rng = Rng::keyed(&[g.seed, 0xC17]);
    let ground = Ground::new(lib, &mut rng);
    let sec = g.section;
    let (px, pz) = g.pitch();
    let (cw, st) = (sec.carriageway, sec.total());
    let (ox, oz) = (g.origin[0], g.origin[1]);
    let (west, north) = (ox - cw * 0.5, oz - cw * 0.5);
    let (east, south) = (ox + g.nx as f32 * px + cw * 0.5, oz + g.nz as f32 * pz + cw * 0.5);
    // the asphalt: vertical streets whole, horizontal streets between them
    for i in 0..=g.nx {
        let xc = ox + i as f32 * px;
        street::flat(&mut mb, lib, ground.asphalt, xc - cw * 0.5, xc + cw * 0.5, north, south, 0.0, [0.9; 3]);
    }
    for j in 0..=g.nz {
        let zc = oz + j as f32 * pz;
        for i in 0..g.nx {
            let x_a = ox + i as f32 * px + cw * 0.5;
            let x_b = ox + (i + 1) as f32 * px - cw * 0.5;
            street::flat(&mut mb, lib, ground.asphalt, x_a, x_b, zc - cw * 0.5, zc + cw * 0.5, 0.0, [0.9; 3]);
        }
    }
    let _ = (west, east);
    // paint: a double yellow line, the parking lines, and zebra crossings on every arm between two crossings
    let py = 0.012;
    for i in 0..=g.nx {
        let xc = ox + i as f32 * px;
        for j in 0..g.nz {
            let (za, zb) = (oz + j as f32 * pz + cw * 0.5, oz + (j + 1) as f32 * pz - cw * 0.5);
            let (a, b) = (za + 8.5, zb - 8.5);
            let mid = (a + b) * 0.5;
            for dx in [-0.13, 0.13] {
                street::paint(&mut mb, ground.yellow, xc + dx, mid, [0.0, 1.0], b - a, 0.1, py);
            }
            for dx in [-(cw * 0.5 - 2.3), cw * 0.5 - 2.3] {
                street::paint(&mut mb, ground.white, xc + dx, mid, [0.0, 1.0], b - a, 0.12, py);
            }
            street::zebra(&mut mb, &ground, xc, za + 2.4, [0.0, 1.0], cw - 4.0, py);
            street::zebra(&mut mb, &ground, xc, zb - 2.4, [0.0, 1.0], cw - 4.0, py);
        }
    }
    for j in 0..=g.nz {
        let zc = oz + j as f32 * pz;
        for i in 0..g.nx {
            let (xa, xb) = (ox + i as f32 * px + cw * 0.5, ox + (i + 1) as f32 * px - cw * 0.5);
            let (a, b) = (xa + 8.5, xb - 8.5);
            let mid = (a + b) * 0.5;
            for dz in [-0.13, 0.13] {
                street::paint(&mut mb, ground.yellow, mid, zc + dz, [1.0, 0.0], b - a, 0.1, py);
            }
            for dz in [-(cw * 0.5 - 2.3), cw * 0.5 - 2.3] {
                street::paint(&mut mb, ground.white, mid, zc + dz, [1.0, 0.0], b - a, 0.12, py);
            }
            street::zebra(&mut mb, &ground, xa + 2.4, zc, [1.0, 0.0], cw - 4.0, py);
            street::zebra(&mut mb, &ground, xb - 2.4, zc, [1.0, 0.0], cw - 4.0, py);
        }
    }
    let _ = st;
    // blocks: sidewalk ring, courtyard floor, lots
    for j in 0..g.nz {
        for i in 0..g.nx {
            let (x0, x1, z0, z1) = g.block(i, j);
            street::sidewalk_ring(&mut mb, lib, &ground, &sec, x0, x1, z0, z1);
            street::flat(&mut mb, lib, ground.court, x0, x1, z0, z1, sec.kerb_h, [0.8; 3]);
            for (lot, kind, k) in block_lots(g, i, j) {
                let mut r = Rng::keyed(&[g.seed, i as u64, j as u64, k, lot.o[0].to_bits() as u64]);
                let b = building::generate(&mut mb, lib, props, &lot, kind, &mut r);
                let floors: Vec<(MeshBuilder, f32)> = b.interior.into_iter().zip(b.floor_mid.iter().copied()).collect();
                let c = lot.corners();
                let centre = scale(add(add(c[0], c[1]), add(c[2], c[3])), 0.25);
                let radius = len(sub(lot.at(lot.w, lot.d), lot.at(0.0, 0.0))) * 0.5;
                buildings.push(Inside { centre, radius, height: b.height, floors, lights: b.lights, doors: b.doors });
            }
        }
    }
    District { mesh: mb, buildings }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Grid {
        Grid { nx: 2, nz: 2, block_w: 80.0, block_d: 64.0, section: Section::default(), seed: 7, origin: [0.0, 0.0] }
    }

    #[test]
    fn the_lots_of_a_block_fill_its_edges_without_overlapping_each_other() {
        let g = grid();
        let (x0, x1, z0, z1) = g.block(0, 0);
        let lots = block_lots(&g, 0, 0);
        assert!(lots.len() >= 8, "{} lots", lots.len());
        for (a, _, _) in &lots {
            for c in a.corners() {
                assert!(c[0] > x0 - 0.01 && c[0] < x1 + 0.01 && c[2] > z0 - 0.01 && c[2] < z1 + 0.01, "{c:?} outside the block");
            }
        }
        for (n, (a, _, _)) in lots.iter().enumerate() {
            for (b, _, _) in lots.iter().skip(n + 1) {
                let bbox = |l: &Lot| {
                    let c = l.corners();
                    (c.iter().map(|p| p[0]).fold(f32::MAX, f32::min), c.iter().map(|p| p[0]).fold(f32::MIN, f32::max), c.iter().map(|p| p[2]).fold(f32::MAX, f32::min), c.iter().map(|p| p[2]).fold(f32::MIN, f32::max))
                };
                let (a0, a1, a2, a3) = bbox(a);
                let (b0, b1, b2, b3) = bbox(b);
                let overlap = a0.max(b0) < a1.min(b1) - 0.05 && a2.max(b2) < a3.min(b3) - 0.05;
                assert!(!overlap, "two lots overlap: {a0},{a1},{a2},{a3} and {b0},{b1},{b2},{b3}");
            }
        }
    }

    #[test]
    fn a_district_is_the_same_every_time_and_has_streets_and_buildings() {
        let lib = Lib::from_json("");
        let g = grid();
        let props = Props { map: Default::default() };
        let a = district(&lib, &props, &g).mesh;
        let b = district(&lib, &props, &g).mesh;
        assert_eq!(a.triangles(), b.triangles());
        assert!(a.triangles() > 20_000, "{}", a.triangles());
        let (lo, hi) = a.bounds().unwrap();
        assert!(hi[1] > 20.0 && lo[1] >= -0.01);
    }
}
