//! What the city looks like at each distance. Level 0 is everything (`building::exterior`, the street laid with paint and zebra crossings); the coarser levels are made here, each a little *inside* the finer one (its
//! walls and roof drawn a hand's width in), so that where two levels are loaded at once, in the overlap that keeps a level from vanishing before the next one appears, the finer one is the one in front.
//!
//! - level 1 (a few hundred metres): textured walls, a dark band of glass for the windows of every floor, the roof; asphalt and paving as flat strips;
//! - level 2 (kilometres): a box wearing a picture of a bay of windows (tinted with the average colour of the wall's material, darker at the foot) and its roof; one dark ground per cell.
//!
//! The coarser levels are only loaded where nobody can be inside a building (`StreamZone.min_radius`), because from inside a room a wall that stands a hand's width in would be seen in front of the real one.

use crate::building::{Spec, VolSpec};
use crate::facade::{col, wall_frame};
use crate::materials::Lib;
use crate::mesh::*;
use crate::plan::{LandUse, Plan, CELL_BLOCKS};
use crate::rng::Rng;
use crate::street::{self, Ground};

/// How far inside the fine building the walls of level 1 and level 2 stand.
pub const INSET1: f32 = 0.45;
pub const INSET2: f32 = 1.2;

/// The corners of a volume's footprint pulled in by `by` metres, at height `y`.
fn corners_in(v: &VolSpec, by: f32, y: f32) -> [V3; 4] {
    let f = &v.foot;
    [f.at(f.x0 + by, f.z0 + by, y), f.at(f.x1 - by, f.z0 + by, y), f.at(f.x1 - by, f.z1 - by, y), f.at(f.x0 + by, f.z1 - by, y)]
}

/// The solid body of each volume of a building as an occluder (see `MeshBuilder::occluders`): the footprint a little inside the walls, from a little above the floor to a little under the roof. A volume under five metres high is left
/// out (a porch, a shed): it hides little.
pub fn occluders(mb: &mut MeshBuilder, spec: &Spec) {
    for v in &spec.vols {
        if v.top() - v.y0 < 5.0 {
            continue;
        }
        let c = corners_in(v, 0.6, 0.0);
        mb.occluders.push([c[0][0], c[0][2], c[1][0], c[1][2], c[2][0], c[2][2], c[3][0], c[3][2], v.y0 + 0.6, v.top() - 0.6]);
    }
}

fn centroid(c: &[V3; 4]) -> V3 {
    scale(add(add(c[0], c[1]), add(c[2], c[3])), 0.25)
}

/// Level 1 of a building: the same boxes as level 2 (a bay of windows on a wall tinted with the colour of its material, a roof), but a little nearer the real walls. Few materials a block, so that a few hundred of them
/// are drawn in a few hundred draws: what is seen from further than a hundred metres does not need its own bricks.
pub fn building_l1(mb: &mut MeshBuilder, lib: &Lib, spec: &Spec) {
    boxes(mb, lib, spec, INSET1);
}

/// Level 2 of a building: one box for every volume, a bay of windows on a plain wall repeated down it (`far_win`; a curtain wall wears `far_glass`), painted with vertex colours: the average colour of the wall's
/// material, a little different from one building to the next, darker at the foot.
pub fn building_l2(mb: &mut MeshBuilder, lib: &Lib, spec: &Spec) {
    boxes(mb, lib, spec, INSET2);
}

fn boxes(mb: &mut MeshBuilder, lib: &Lib, spec: &Spec, inset: f32) {
    let far = lib.id("far_flat");
    let mut rng = Rng::keyed(&[spec.key, 4]);
    for v in &spec.vols {
        let corners = corners_in(v, inset, v.y0);
        let mid = centroid(&corners);
        let h = v.top() - v.y0;
        let (mat, base) = if v.is_curtain() {
            let g = lib.mats[v.pal.glass[0]].avg;
            (lib.id("far_glass"), [g[0] * 4.0 + 0.03, g[1] * 4.0 + 0.045, g[2] * 4.0 + 0.07])
        } else {
            let a = lib.mats[v.pal.wall].avg;
            (lib.id("far_win"), [a[0] * v.pal.wall_tint[0], a[1] * v.pal.wall_tint[1], a[2] * v.pal.wall_tint[2]])
        };
        let base = col(base, rng.range(0.85, 1.12));
        let (dark, light) = (col(base, 0.6), col(base, 1.0));
        for e in 0..4 {
            let (fr, length) = wall_frame(corners[e], corners[(e + 1) % 4], mid);
            if length < 0.5 {
                continue;
            }
            // whole bays across the wall, one floor high
            let (bay, rows) = if v.is_curtain() { (1.8, v.floor_h) } else { (3.6, v.floor_h) };
            let across = (length / bay).round().max(1.0);
            let tex = Tex { size: [length / across, rows], off: [0.0, 0.0] };
            let p = [fr.at(0.0, 0.0, 0.0), fr.at(length, 0.0, 0.0), fr.at(length, h, 0.0), fr.at(0.0, h, 0.0)];
            let uv = [tex.uv(0.0, 0.0), tex.uv(length, 0.0), tex.uv(length, h), tex.uv(0.0, h)];
            mb.quad(mat, p, uv, [dark, dark, light, light], fr.c);
        }
        let rp = corners_in(v, inset, v.top());
        mb.quad(far, rp, [[0.0; 2]; 4], [[0.07, 0.07, 0.075]; 4], [0.0, 1.0, 0.0]);
    }
    // a spire as one thin tapering shaft (it is what makes a skyline known from far off)
    for (base, w, h) in &spec.spires {
        let (half, tip) = ((w * 0.30).max(0.5), 0.10);
        let c = [[1.0, 1.0], [1.0, -1.0], [-1.0, -1.0], [-1.0, 1.0]];
        let grey = [0.42, 0.43, 0.46];
        for e in 0..4 {
            let (a, b) = (c[e], c[(e + 1) % 4]);
            let p = [
                [base[0] + a[0] * half, base[1], base[2] + a[1] * half],
                [base[0] + b[0] * half, base[1], base[2] + b[1] * half],
                [base[0] + b[0] * tip, base[1] + h * 1.18, base[2] + b[1] * tip],
                [base[0] + a[0] * tip, base[1] + h * 1.18, base[2] + a[1] * tip],
            ];
            let n = [(a[0] + b[0]) * 0.5, 0.0, (a[1] + b[1]) * 0.5];
            mb.quad(far, p, [[0.0; 2]; 4], [grey; 4], n);
        }
    }
}

/// The ground a block owns at level 0: the paving round it and its court, and, starting at its north-west corner, the crossing and the two street segments that run from it (south along street `i`, east along street `j`),
/// asphalt, lane paint and zebra crossings. Every piece of street belongs to exactly one block.
pub fn ground_l0(mb: &mut MeshBuilder, lib: &Lib, plan: &Plan, g: &Ground, i: i32, j: i32) {
    if plan.polar.is_some() {
        return crate::polar::ground_l0(mb, lib, plan, g, i, j);
    }
    let sec = plan.section;
    let (x0, x1, z0, z1) = plan.block_rect(i, j);
    street::sidewalk_ring(mb, lib, g, &sec, x0, x1, z0, z1);
    if plan.landuse(i, j) == LandUse::Park {
        crate::streetscape::park_ground(mb, lib, g, plan, i, j);
    } else {
        street::flat(mb, lib, g.court, x0, x1, z0, z1, sec.kerb_h, [0.8; 3]);
    }
    let p = plan.pitch();
    let (cwi, cwj) = (plan.carriageway(i), plan.carriageway(j));
    let (xi, zj) = (i as f32 * p, j as f32 * p);
    let (cwi1, cwj1) = (plan.carriageway(i + 1), plan.carriageway(j + 1));
    let c = [0.9; 3];
    street::flat(mb, lib, g.asphalt, xi - cwi * 0.5, xi + cwi * 0.5, zj - cwj * 0.5, zj + cwj * 0.5, 0.0, c);
    let (za, zb) = (zj + cwj * 0.5, zj + p - cwj1 * 0.5);
    street::flat(mb, lib, g.asphalt, xi - cwi * 0.5, xi + cwi * 0.5, za, zb, 0.0, c);
    let (xa, xb) = (xi + cwi * 0.5, xi + p - cwi1 * 0.5);
    street::flat(mb, lib, g.asphalt, xa, xb, zj - cwj * 0.5, zj + cwj * 0.5, 0.0, c);
    let py = 0.012;
    // street i, going south
    {
        let (a, b) = (za + 8.5, zb - 8.5);
        let mid = (a + b) * 0.5;
        for dx in [-0.13, 0.13] {
            street::paint(mb, g.yellow, xi + dx, mid, [0.0, 1.0], b - a, 0.1, py);
        }
        for dx in [-(cwi * 0.5 - 2.3), cwi * 0.5 - 2.3] {
            street::paint(mb, g.white, xi + dx, mid, [0.0, 1.0], b - a, 0.12, py);
        }
        street::zebra(mb, g, xi, za + 2.4, [0.0, 1.0], cwi - 4.0, py);
        street::zebra(mb, g, xi, zb - 2.4, [0.0, 1.0], cwi - 4.0, py);
    }
    // street j, going east
    {
        let (a, b) = (xa + 8.5, xb - 8.5);
        let mid = (a + b) * 0.5;
        for dz in [-0.13, 0.13] {
            street::paint(mb, g.yellow, mid, zj + dz, [1.0, 0.0], b - a, 0.1, py);
        }
        for dz in [-(cwj * 0.5 - 2.3), cwj * 0.5 - 2.3] {
            street::paint(mb, g.white, mid, zj + dz, [1.0, 0.0], b - a, 0.12, py);
        }
        street::zebra(mb, g, xa + 2.4, zj, [1.0, 0.0], cwj - 4.0, py);
        street::zebra(mb, g, xb - 2.4, zj, [1.0, 0.0], cwj - 4.0, py);
    }
}

/// The ground of a block at level 1: the same pieces as level 0 as plain strips, a little below it.
pub fn ground_l1(mb: &mut MeshBuilder, lib: &Lib, plan: &Plan, g: &Ground, i: i32, j: i32) {
    if plan.polar.is_some() {
        return crate::polar::ground_l1(mb, lib, plan, g, i, j);
    }
    let sec = plan.section;
    let (x0, x1, z0, z1) = plan.block_rect(i, j);
    let (sw, kh) = (sec.sidewalk, sec.kerb_h);
    let drop = 0.12;
    street::flat(mb, lib, g.sidewalk, x0 - sw, x1 + sw, z0 - sw, z1 + sw, kh - drop, [0.9; 3]);
    if plan.landuse(i, j) == LandUse::Park {
        street::flat(mb, lib, g.lawn, x0, x1, z0, z1, kh - drop + 0.01, crate::streetscape::LAWN_TINT);
    } else {
        street::flat(mb, lib, g.court, x0, x1, z0, z1, kh - drop + 0.01, [0.8; 3]);
    }
    let p = plan.pitch();
    let (cwi, cwj) = (plan.carriageway(i), plan.carriageway(j));
    let (cwi1, cwj1) = (plan.carriageway(i + 1), plan.carriageway(j + 1));
    let (xi, zj) = (i as f32 * p, j as f32 * p);
    let y = -drop;
    let c = [0.9; 3];
    street::flat(mb, lib, g.asphalt, xi - cwi * 0.5, xi + cwi * 0.5, zj - cwj * 0.5, zj + cwj * 0.5, y, c);
    street::flat(mb, lib, g.asphalt, xi - cwi * 0.5, xi + cwi * 0.5, zj + cwj * 0.5, zj + p - cwj1 * 0.5, y, c);
    street::flat(mb, lib, g.asphalt, xi + cwi * 0.5, xi + p - cwi1 * 0.5, zj - cwj * 0.5, zj + cwj * 0.5, y, c);
}

/// The ground of a cell at level 2: one dark quad under the buildings, and a green one for every park.
pub fn ground_l2(mb: &mut MeshBuilder, lib: &Lib, plan: &Plan, ci: i32, cj: i32) {
    let far = lib.id("far_flat");
    let cs = plan.cell_size();
    if plan.polar.is_some() {
        return crate::polar::ground_l2(mb, lib, plan, cs, ci, cj);
    }
    // (a round city has no ground where it has no block)
    if !plan.cell_inside(ci, cj) {
        return;
    }
    let (x0, x1, z0, z1) = (ci as f32 * cs, (ci + 1) as f32 * cs, cj as f32 * cs, (cj + 1) as f32 * cs);
    let c = [0.05, 0.05, 0.052];
    mb.quad(far, [[x0, -0.5, z0], [x1, -0.5, z0], [x1, -0.5, z1], [x0, -0.5, z1]], [[0.0; 2]; 4], [c; 4], [0.0, 1.0, 0.0]);
    let a = lib.mats[lib.id("grass_ground")].avg;
    let green = [a[0] * crate::streetscape::LAWN_TINT[0], a[1] * crate::streetscape::LAWN_TINT[1], a[2] * crate::streetscape::LAWN_TINT[2]];
    for bj in cj * CELL_BLOCKS..(cj + 1) * CELL_BLOCKS {
        for bi in ci * CELL_BLOCKS..(ci + 1) * CELL_BLOCKS {
            if plan.landuse(bi, bj) == LandUse::Park {
                let (px0, px1, pz0, pz1) = plan.block_rect(bi, bj);
                mb.quad(far, [[px0, 0.1, pz0], [px1, 0.1, pz0], [px1, 0.1, pz1], [px0, 0.1, pz1]], [[0.0; 2]; 4], [green; 4], [0.0, 1.0, 0.0]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::building::{exterior, spec_of, Kind, Lot, Side};

    #[test]
    fn the_coarser_levels_are_far_smaller_than_the_fine_one_and_stand_inside_its_walls() {
        let lib = Lib::from_json("");
        let lot = Lot { o: [0.0, 0.15, 0.0], ex: [1.0, 0.0, 0.0], ez: [0.0, 0.0, 1.0], w: 26.0, d: 22.0, sides: [Side::Street, Side::Party, Side::Open, Side::Party] };
        for kind in [Kind::Masonry, Kind::Apartment, Kind::Tower, Kind::Shophouse] {
            let spec = spec_of(&lib, &lot, kind, 3);
            let mut fine = MeshBuilder::new();
            exterior(&mut fine, &lib, &spec);
            let (mut l1, mut l2) = (MeshBuilder::new(), MeshBuilder::new());
            building_l1(&mut l1, &lib, &spec);
            building_l2(&mut l2, &lib, &spec);
            assert!(l1.triangles() * 3 < fine.triangles(), "{kind:?}: level 1 has {} triangles, the building {}", l1.triangles(), fine.triangles());
            assert!(l2.triangles() <= 10 * spec.vols.len(), "{kind:?} level 2: {}", l2.triangles());
            let (lo, hi) = l1.bounds().unwrap();
            assert!(lo[0] > 0.0 && hi[0] < 26.0 && lo[2] > 0.0 && hi[2] < 22.0, "{kind:?} {lo:?} {hi:?}");
            let (lo2, hi2) = l2.bounds().unwrap();
            assert!(lo2[0] > lo[0] && hi2[0] < hi[0] + 0.01 && lo2[2] > lo[2], "{kind:?} level 2 outside level 1");
        }
    }

    #[test]
    fn every_piece_of_street_has_an_owner_and_the_blocks_ground_stays_in_its_corner() {
        let lib = Lib::from_json("");
        let plan = Plan::new(4);
        let g = Ground::new(&lib, &mut Rng::new(1));
        let (mut a, mut b) = (MeshBuilder::new(), MeshBuilder::new());
        ground_l0(&mut a, &lib, &plan, &g, 6, -3);
        ground_l1(&mut b, &lib, &plan, &g, 6, -3);
        assert!(a.triangles() > 3 * b.triangles() && b.triangles() >= 10, "{} {}", a.triangles(), b.triangles());
        let p = plan.pitch();
        for m in [&a, &b] {
            let (lo, hi) = m.bounds().unwrap();
            // from the north-west crossing's asphalt to the next crossing's, with the sidewalk ring round the block inside that
            assert!(lo[0] > 6.0 * p - 12.0 && hi[0] < 7.0 * p + 12.0 && lo[2] > -3.0 * p - 12.0 && hi[2] < -2.0 * p + 12.0, "{lo:?} {hi:?}");
        }
    }
}
