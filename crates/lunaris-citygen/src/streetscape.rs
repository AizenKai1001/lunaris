//! What stands in a street and what a park is: trees along the kerb, street lamps, benches, hydrants, cabinets; a lawn with paths, trees, shrubs, benches and lamps. Everything is decided from the seed and the block's
//! indices (`spots`), and shown at the level of detail asked for: the real models (level 0) or, for the trees, a few crossed cards of the crown (level 1).

use crate::materials::Lib;
use crate::mesh::*;
use crate::plan::{LandUse, Plan};
use crate::props::Props;
use crate::rng::Rng;
use crate::street::Ground;

/// Every model the streets and parks use, by number: `city/gen_p_<n>.glb` is model n, one file for the whole city, which every copy shares.
pub const PROPS: &[&str] = &["tree_small_02", "island_tree_01", "island_tree_02", "island_tree_03", "jacaranda_tree", "pine_tree_01", "fir_tree_01", "shrub_01", "shrub_02", "shrub_03", "shrub_04", "street_lamp_01", "fire_hydrant", "modular_street_seating", "utility_box_01", "utility_box_02", "covered_car", "painted_wooden_bench", "wooden_picnic_table", "trashbag", "standing_chalkboard_01", "planter_box_01", "planter_box_02", "WetFloorSign_01", "wreck_car_a", "wreck_car_b", "wreck_van", "kit_oak_g_1", "kit_oak_y_1", "kit_oak_o_1", "kit_oak_d_1", "kit_acacia_g_1", "kit_acacia_y_1", "kit_acacia_d_1", "kit_pine_g_1", "kit_pine_d_1", "kit_dead_1", "kit_shrub_g_1", "kit_shrub_y_1", "kit_shrub_d_1", "kit_fern_g_1", "kit_fern_y_1", "kit_oak_g_2", "kit_oak_y_2", "kit_oak_o_2", "kit_oak_d_2", "kit_acacia_g_2", "kit_acacia_y_2", "kit_acacia_d_2", "kit_pine_g_2", "kit_pine_d_2", "kit_dead_2", "kit_shrub_g_2", "kit_shrub_y_2", "kit_shrub_d_2", "kit_fern_g_2", "kit_fern_y_2"];

/// The car a derelict city leaves at a kerb (`spots` of a polar city): mostly wrecks, some still under their covers.
pub(crate) fn derelict_car(rng: &mut Rng) -> &'static str {
    ["covered_car", "wreck_car_a", "wreck_car_b", "wreck_van"][rng.weighted(&[0.34, 0.30, 0.22, 0.14])]
}

pub fn prop_index(slug: &str) -> Option<usize> {
    PROPS.iter().position(|p| *p == slug)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Thing {
    Tree,
    Shrub,
    Lamp,
    Bench,
    Hydrant,
    Cabinet,
    /// A car left at the kerb under its cover.
    Car,
    Table,
    Litter,
    /// Planters and sandwich boards along the shop fronts.
    Planter,
    Board,
}

/// One thing standing somewhere: the model (a slug of the props folder), where, turned how much and how big.
#[derive(Clone, Copy, Debug)]
pub struct Spot {
    pub thing: Thing,
    pub slug: &'static str,
    pub pos: V3,
    pub yaw: f32,
    pub scale: f32,
}

/// (slug, scale a model is put at in a street, scale in a park, weight in a street, weight in a park): the sizes make a tree of 5 to 10 metres.
const TREES: &[(&str, f32, f32, f32, f32)] = &[
    // the dome city's own trees (art/city/blender/make_trees.py), built in metres: g green, y yellowing, o orange, d dry brown; a derelict street has all of them, and some dead ones
    ("kit_oak_g_1", 0.95, 1.1, 3.0, 2.5),
    ("kit_oak_g_2", 0.95, 1.1, 3.0, 2.5),
    ("kit_oak_y_1", 0.95, 1.1, 0.7, 0.5),
    ("kit_oak_y_2", 0.95, 1.1, 0.7, 0.5),
    ("kit_oak_o_1", 0.95, 1.1, 0.4, 0.3),
    ("kit_oak_o_2", 0.95, 1.1, 0.4, 0.3),
    ("kit_oak_d_1", 0.95, 1.1, 0.45, 0.3),
    ("kit_oak_d_2", 0.95, 1.1, 0.45, 0.3),
    ("kit_acacia_g_1", 0.95, 1.1, 2.0, 2.0),
    ("kit_acacia_g_2", 0.95, 1.1, 2.0, 2.0),
    ("kit_acacia_y_1", 0.95, 1.1, 0.5, 0.4),
    ("kit_acacia_y_2", 0.95, 1.1, 0.5, 0.4),
    ("kit_acacia_d_1", 0.95, 1.1, 0.35, 0.25),
    ("kit_acacia_d_2", 0.95, 1.1, 0.35, 0.25),
    ("kit_pine_g_1", 0.0, 1.0, 0.0, 2.0),
    ("kit_pine_g_2", 0.0, 1.0, 0.0, 2.0),
    ("kit_pine_d_1", 0.0, 1.0, 0.0, 0.6),
    ("kit_dead_1", 0.95, 1.05, 0.7, 0.5),
    ("kit_dead_2", 0.95, 1.05, 0.7, 0.5),
];
const SHRUBS: &[&str] = &["kit_shrub_g_1", "kit_shrub_g_2", "kit_shrub_g_1", "kit_shrub_y_1", "kit_shrub_y_2", "kit_shrub_d_1", "kit_shrub_d_2", "kit_fern_g_1", "kit_fern_g_2", "kit_fern_y_1", "kit_fern_y_2"];

pub(crate) fn pick_tree(rng: &mut Rng, park: bool) -> (&'static str, f32) {
    let w: Vec<f32> = TREES.iter().map(|t| if park { t.4 } else { t.3 }).collect();
    let t = TREES[rng.weighted(&w)];
    (t.0, if park { t.2 } else { t.1 } * rng.range(0.85, 1.18))
}

/// The yaw (degrees) that turns a model's +x axis along the horizontal direction (dx, dz).
pub(crate) fn yaw_along(dx: f32, dz: f32) -> f32 {
    (-dz).atan2(dx).to_degrees()
}

/// The things of block (i, j): along the four sidewalks for a block of buildings, over the lawn for a park.
pub fn spots(plan: &Plan, i: i32, j: i32) -> Vec<Spot> {
    if plan.polar.is_some() {
        return crate::polar::spots(plan, i, j);
    }
    let mut rng = Rng::keyed(&[plan.seed, i as u64, j as u64, 0x5352_4545]);
    let (x0, x1, z0, z1) = plan.block_rect(i, j);
    let sw = plan.section.sidewalk;
    let y = plan.section.kerb_h;
    let park = plan.landuse(i, j) == LandUse::Park;
    let mut out: Vec<Spot> = Vec::new();
    // the four sidewalks: where one starts, which way it runs, which way is into the block, and how long it is
    let sides = [
        ([x0, z0 - sw], [1.0, 0.0], [0.0, 1.0], x1 - x0),
        ([x1, z1 + sw], [-1.0, 0.0], [0.0, -1.0], x1 - x0),
        ([x0 - sw, z0], [0.0, 1.0], [1.0, 0.0], z1 - z0),
        ([x1 + sw, z1], [0.0, -1.0], [-1.0, 0.0], z1 - z0),
    ];
    for (p0, t, n, len) in sides {
        let at = |s: f32, off: f32| [p0[0] + t[0] * s + n[0] * off, y, p0[1] + t[1] * s + n[1] * off];
        // trees at the kerb every 13 m or so, lamps between them
        let mut s = rng.range(5.0, 9.0);
        while s < len - 5.0 {
            let (slug, scale) = pick_tree(&mut rng, false);
            if !park || rng.chance(0.55) {
                out.push(Spot { thing: Thing::Tree, slug, pos: at(s, rng.range(0.95, 1.2)), yaw: rng.range(0.0, 360.0), scale });
            }
            let lamp_s = s + rng.range(5.5, 7.5);
            if lamp_s < len - 4.0 && rng.chance(0.85) {
                out.push(Spot { thing: Thing::Lamp, slug: "street_lamp_01", pos: at(lamp_s, 0.55), yaw: yaw_along(-n[0], -n[1]), scale: 1.0 });
            }
            s += rng.range(12.0, 15.5);
        }
        if rng.chance(0.4) {
            out.push(Spot { thing: Thing::Hydrant, slug: "fire_hydrant", pos: at(rng.range(8.0, len - 8.0), 0.7), yaw: rng.range(0.0, 360.0), scale: 1.0 });
        }
        // a bench and a cabinet by the kerb (not by the buildings, where a door may be), clear of the trees and lamps there
        let clear = |out: &[Spot], p: V3, r: f32| out.iter().all(|o| (o.pos[0] - p[0]).powi(2) + (o.pos[2] - p[2]).powi(2) > r * r);
        if rng.chance(0.35) {
            let p = at(rng.range(10.0, len - 10.0), 1.25);
            if clear(&out, p, 4.2) {
                out.push(Spot { thing: Thing::Bench, slug: "modular_street_seating", pos: p, yaw: yaw_along(t[0], t[1]), scale: 1.0 });
            }
        }
        if rng.chance(0.2) {
            let slug = if rng.chance(0.5) { "utility_box_01" } else { "utility_box_02" };
            let p = at(rng.range(8.0, len - 8.0), 1.0);
            if clear(&out, p, 2.5) {
                out.push(Spot { thing: Thing::Cabinet, slug, pos: p, yaw: yaw_along(t[0], t[1]), scale: 1.0 });
            }
        }
    }
    // cars left in the parking lane of the two street segments that start at this block's north-west crossing (one going south, one going east), clear of the crossings at their ends
    {
        let p = plan.pitch();
        let (xi, zj) = (i as f32 * p, j as f32 * p);
        let (cwi, cwj) = (plan.carriageway(i), plan.carriageway(j));
        let (cwi1, cwj1) = (plan.carriageway(i + 1), plan.carriageway(j + 1));
        let mut crng = Rng::keyed(&[plan.seed, i as u64, j as u64, 0x434152]);
        // (start, end, fixed coordinate, half carriageway, vertical?)
        let segs = [(zj + cwj * 0.5, zj + p - cwj1 * 0.5, xi, cwi * 0.5, true), (xi + cwi * 0.5, xi + p - cwi1 * 0.5, zj, cwj * 0.5, false)];
        for (a, b, fixed, half, vertical) in segs {
            let mut s = a + crng.range(14.0, 30.0);
            while s < b - 14.0 {
                if crng.chance(0.55) {
                    let side = if crng.chance(0.5) { 1.0 } else { -1.0 };
                    let off = side * (half - 1.3);
                    let (pos, along) = if vertical { ([fixed + off, y - plan.section.kerb_h, s], [0.0, 1.0]) } else { ([s, y - plan.section.kerb_h, fixed + off], [1.0, 0.0]) };
                    out.push(Spot { thing: Thing::Car, slug: "covered_car", pos, yaw: yaw_along(along[0], along[1]) + crng.range(-3.0, 3.0), scale: 1.0 });
                }
                s += crng.range(14.0, 42.0);
            }
        }
    }
    // rubbish bags by the walls now and then
    for (p0, t, n, len) in sides {
        let mut r = Rng::keyed(&[plan.seed, i as u64, j as u64, 0x4C49545445, p0[0].to_bits() as u64 ^ p0[1].to_bits() as u64]);
        for _ in 0..r.int(0, 3) {
            let (s, off) = (r.range(6.0, len - 6.0), r.range(sw - 0.7, sw - 0.3));
            out.push(Spot { thing: Thing::Litter, slug: "trashbag", pos: [p0[0] + t[0] * s + n[0] * off, y, p0[1] + t[1] * s + n[1] * off], yaw: r.range(0.0, 360.0), scale: r.range(0.9, 1.2) });
        }
    }
    // planters and boards standing along the walls
    for (p0, t, n, len) in sides {
        let mut r = Rng::keyed(&[plan.seed, i as u64, j as u64, 0x504C41, p0[0].to_bits() as u64 ^ p0[1].to_bits() as u64]);
        let mut s = r.range(5.0, 12.0);
        while s < len - 5.0 {
            let off = sw - r.range(0.45, 0.7);
            let pos = [p0[0] + t[0] * s + n[0] * off, y, p0[1] + t[1] * s + n[1] * off];
            let (thing, slug, scale) = match r.weighted(&[0.4, 0.35, 0.25]) {
                0 => (Thing::Planter, if r.chance(0.5) { "planter_box_01" } else { "planter_box_02" }, 1.0),
                1 => (Thing::Board, "standing_chalkboard_01", 1.0),
                _ => (Thing::Board, "WetFloorSign_01", 1.2),
            };
            if r.chance(0.7) {
                out.push(Spot { thing, slug, pos, yaw: yaw_along(t[0], t[1]) + r.range(-15.0, 15.0), scale });
            }
            s += r.range(8.0, 18.0);
        }
    }
    // the yards of houses: a tree in a front corner of the lot, and one at the back now and then
    for lp in plan.lots(i, j) {
        if lp.kind != crate::building::Kind::House {
            continue;
        }
        let lot = &lp.lot;
        let mut r = Rng::keyed(&[plan.seed, i as u64, j as u64, lp.index as u64, 0x59415244]);
        let left = r.chance(0.5);
        let (slug, scale) = pick_tree(&mut r, false);
        let x = if left { 1.4 } else { lot.w - 1.4 };
        let p = lot.at(x, r.range(2.0, 3.0));
        out.push(Spot { thing: Thing::Tree, slug, pos: [p[0], y, p[2]], yaw: r.range(0.0, 360.0), scale });
        if r.chance(0.5) {
            let (slug, scale) = pick_tree(&mut r, false);
            let p = lot.at(r.range(2.0, lot.w - 2.0), lot.d - 1.6);
            out.push(Spot { thing: Thing::Tree, slug, pos: [p[0], y, p[2]], yaw: r.range(0.0, 360.0), scale });
        }
    }
    if park {
        let lay = park_layout(plan, i, j);
        park_spots(&mut out, &lay, (x0, x1, z0, z1), &mut rng, y);
    }
    out
}

/// The things of a park laid out in `rect` (x0, x1, z0, z1): trees, shrubs, benches, lamps, and what its kind has besides.
pub(crate) fn park_spots(out: &mut Vec<Spot>, lay: &ParkLayout, rect: (f32, f32, f32, f32), rng: &mut Rng, y: f32) {
    if lay.kind == ParkKind::Ruin {
        return;
    }
    let (x0, x1, z0, z1) = rect;
    let (cx, cz) = ((x0 + x1) * 0.5, (z0 + z1) * 0.5);
    let on_path = |x: f32, z: f32| lay.occupied(x, z);
    // what each kind of park has besides trees: benches facing its middle feature
    match lay.kind {
        ParkKind::Plaza => {
            for (fx, fz) in [(1.0f32, 0.0f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                out.push(Spot { thing: Thing::Bench, slug: "painted_wooden_bench", pos: [cx + fx * 11.5, y, cz + fz * 11.5], yaw: yaw_along(fz, -fx), scale: 1.5 });
            }
            // trees in the paving: a ring of twelve round the fountain, and eight more further out
            for k in 0..12 {
                let a = (k as f32 + 0.5) / 12.0 * std::f32::consts::TAU;
                let (slug, scale) = pick_tree(rng, true);
                out.push(Spot { thing: Thing::Tree, slug, pos: [cx + a.cos() * 20.0, y, cz + a.sin() * 20.0], yaw: rng.range(0.0, 360.0), scale });
            }
            for k in 0..8 {
                let a = k as f32 / 8.0 * std::f32::consts::TAU + 0.39;
                let (slug, scale) = pick_tree(rng, true);
                out.push(Spot { thing: Thing::Tree, slug, pos: [cx + a.cos() * 30.0, y, cz + a.sin() * 30.0], yaw: rng.range(0.0, 360.0), scale });
            }
        }
        ParkKind::Pond => {
            for k in 0..4 {
                let a = (k as f32 * 90.0 + 45.0 + rng.range(-15.0, 15.0)).to_radians();
                let (px, pz) = (lay.fx + a.cos() * (lay.hx + 3.2), lay.fz + a.sin() * (lay.hz + 3.2));
                out.push(Spot { thing: Thing::Bench, slug: "painted_wooden_bench", pos: [px, y, pz], yaw: yaw_along(a.sin(), -a.cos()), scale: 1.5 });
            }
        }
        ParkKind::Sports => {
            for s in [-1.0f32, 1.0] {
                for k in 0..2 {
                    let along = (k as f32 - 0.5) * lay.hx * 1.1;
                    let (px, pz) = if lay.along_x { (lay.fx + along, lay.fz + s * (lay.hz + 3.0)) } else { (lay.fx + s * (lay.hx + 3.0), lay.fz + along) };
                    let (fx, fz) = if lay.along_x { (0.0, -s) } else { (-s, 0.0) };
                    out.push(Spot { thing: Thing::Bench, slug: "painted_wooden_bench", pos: [px, y, pz], yaw: yaw_along(-fz, fx), scale: 1.5 });
                }
            }
            // floodlights at the corners of the pitch (the lamps at night)
            for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                out.push(Spot { thing: Thing::Lamp, slug: "street_lamp_01", pos: [lay.fx + sx * (lay.hx + 2.5), y, lay.fz + sz * (lay.hz + 2.5)], yaw: 0.0, scale: 1.3 });
            }
        }
        ParkKind::Playground => {
            for (k, (sx, sz)) in [(-1.0f32, 0.0f32), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)].into_iter().enumerate() {
                if k < 3 {
                    let (px, pz) = (lay.fx + sx * (lay.hx + 2.6), lay.fz + sz * (lay.hz + 2.6));
                    out.push(Spot { thing: Thing::Bench, slug: "painted_wooden_bench", pos: [px, y, pz], yaw: yaw_along(sz, -sx), scale: 1.5 });
                }
            }
        }
        _ => {}
    }
    let (attempts, spacing) = match lay.kind {
        ParkKind::Meadow => (320, 5.5f32),
        ParkKind::Plaza => (150, 6.0),
        ParkKind::Sports => (150, 7.5),
        ParkKind::Playground => (140, 8.0),
        _ => (160, 7.0),
    };
    let mut placed: Vec<[f32; 2]> = Vec::new();
    for _ in 0..attempts {
        let (x, z) = (rng.range(x0 + 3.0, x1 - 3.0), rng.range(z0 + 3.0, z1 - 3.0));
        if on_path(x, z) || placed.iter().any(|p| (p[0] - x).powi(2) + (p[1] - z).powi(2) < spacing * spacing) {
            continue;
        }
        placed.push([x, z]);
        let (slug, scale) = pick_tree(rng, true);
        out.push(Spot { thing: Thing::Tree, slug, pos: [x, y, z], yaw: rng.range(0.0, 360.0), scale });
    }
    for _ in 0..if matches!(lay.kind, ParkKind::Green | ParkKind::Meadow) { 26 } else { 8 } {
        let along_x = rng.chance(0.5);
        let (x, z) = if along_x { (rng.range(x0 + 3.0, x1 - 3.0), cz + if rng.chance(0.5) { 4.2 } else { -4.2 } + rng.range(-0.5, 0.5)) } else { (cx + if rng.chance(0.5) { 4.2 } else { -4.2 } + rng.range(-0.5, 0.5), rng.range(z0 + 3.0, z1 - 3.0)) };
        if lay.kind != ParkKind::Green && lay.occupied(x, z) {
            continue;
        }
        out.push(Spot { thing: Thing::Shrub, slug: rng.pick(SHRUBS), pos: [x, y, z], yaw: rng.range(0.0, 360.0), scale: rng.range(0.9, 1.5) });
    }
    if !matches!(lay.kind, ParkKind::Green | ParkKind::Meadow | ParkKind::Playground) {
        // (no picnic tables on a plaza, a pitch or by a pond; the benches and lamps above are theirs)
        for s in 0..6 {
            let side = if s % 2 == 0 { 1.0 } else { -1.0 };
            let (px, pz) = (cx + (s as f32 - 2.5) * 13.0, cz + side * 33.0);
            if px > x0 + 3.0 && px < x1 - 3.0 && pz > z0 + 3.0 && pz < z1 - 3.0 && !lay.occupied(px, pz) {
                out.push(Spot { thing: Thing::Lamp, slug: "street_lamp_01", pos: [px, y, pz], yaw: yaw_along(0.0, -side), scale: 1.0 });
            }
        }
        return;
    }
    for _ in 0..3 {
        let p = [rng.range(x0 + 6.0, x1 - 6.0), rng.range(z0 + 6.0, z1 - 6.0)];
        if !on_path(p[0], p[1]) && placed.iter().all(|q| (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) > 36.0) {
            placed.push(p);
            out.push(Spot { thing: Thing::Table, slug: "wooden_picnic_table", pos: [p[0], y, p[1]], yaw: rng.range(0.0, 360.0), scale: 1.0 });
        }
    }
    for k in 0..6 {
        // benches beside the paths, facing them; lamps along them (a meadow has diagonal paths: the benches stand in its clearings)
        let side = if k % 2 == 0 { 1.0 } else { -1.0 };
        if lay.kind == ParkKind::Meadow {
            let p = [cx + (k as f32 - 2.5) * 12.0, cz + side * 24.0];
            out.push(Spot { thing: Thing::Bench, slug: "painted_wooden_bench", pos: [p[0], y, p[1]], yaw: yaw_along(0.0, -side), scale: 1.5 });
            out.push(Spot { thing: Thing::Lamp, slug: "street_lamp_01", pos: [p[0] + 3.0, y, p[1]], yaw: yaw_along(0.0, -side), scale: 1.0 });
            continue;
        }
        if k < 4 && lay.kind != ParkKind::Playground {
            let s = rng.range(8.0, 30.0) * if k < 2 { 1.0 } else { -1.0 };
            out.push(Spot { thing: Thing::Bench, slug: "painted_wooden_bench", pos: [cx + s, y, cz + side * 3.7], yaw: if side > 0.0 { 0.0 } else { 180.0 }, scale: 1.5 });
        }
        let s = (k as f32 - 2.5) * 13.0;
        out.push(Spot { thing: Thing::Lamp, slug: "street_lamp_01", pos: [cx + s, y, cz + side * 3.0], yaw: yaw_along(0.0, -side), scale: 1.0 });
    }
}

/// Which plate (of `materials::STREET_PLATES`) names street `k` (a vertical street, running along z, or a horizontal one): 64 names and every sixth street an avenue.
pub fn street_plate(plan: &Plan, k: i32, vertical: bool) -> usize {
    let h = Rng::keyed(&[plan.seed, k as u64, vertical as u64, 0x5354_524E]).u64();
    (h % 64) as usize * 2 + usize::from(k.rem_euclid(6) == 0)
}

/// The street name posts at the crossing at the north-west corner of block (i, j): two posts on opposite corners, each with the plate of the vertical street (readable along x) over the plate of the horizontal one (along z).
/// Model 100 is the post, 200 + m the plate m (see `City::build`).
pub fn sign_posts(plan: &Plan, i: i32, j: i32, origin: V3) -> Vec<lunaris_json::Json> {
    if plan.polar.is_some() {
        return crate::polar::sign_posts(plan, i, j, origin);
    }
    use lunaris_json::Json;
    let n = |x: f32| Json::Float(x as f64);
    let v3 = |a: [f32; 3]| Json::arr(a.map(n));
    let p = plan.pitch();
    let (xi, zj) = (i as f32 * p, j as f32 * p);
    let (hw_i, hw_j) = (plan.carriageway(i) * 0.5 + 0.6, plan.carriageway(j) * 0.5 + 0.6);
    let y = plan.section.kerb_h;
    let (m_v, m_h) = (street_plate(plan, i, true), street_plate(plan, j, false));
    let model = |k: usize| Json::obj([("mesh", Json::str(format!("city/gen_p_{k}.glb"))), ("color", Json::arr([1.0, 1.0, 1.0, 1.0].map(n)))]);
    let mut out = Vec::new();
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, 1.0)] {
        let pos = [xi + sx * hw_i - origin[0], y - origin[1], zj + sz * hw_j - origin[2]];
        let at = |h: f32| v3([pos[0], pos[1] + h, pos[2]]);
        out.push(Json::obj([("name", Json::str("Sign post")), ("components", Json::obj([("LocalTransform", Json::obj([("pos", v3(pos))])), ("MeshRenderer", model(100))]))]));
        out.push(Json::obj([("name", Json::str("Collider")), ("components", Json::obj([("LocalTransform", Json::obj([("pos", at(1.6))])), ("Collider", Json::obj([("shape", Json::from(2)), ("size", v3([0.2, 3.2, 0.2])), ("trigger", Json::Bool(false))]))]))]));
        // readable along x: the plate's face looks along x (a quarter turn), the other along z
        let quarter = [0.0, (std::f32::consts::FRAC_PI_4).sin(), 0.0, (std::f32::consts::FRAC_PI_4).cos()];
        out.push(Json::obj([("name", Json::str("Plate")), ("components", Json::obj([("LocalTransform", Json::obj([("pos", at(3.08)), ("rot", Json::arr(quarter.map(n)))])), ("MeshRenderer", model(200 + m_v))]))]));
        out.push(Json::obj([("name", Json::str("Plate")), ("components", Json::obj([("LocalTransform", Json::obj([("pos", at(2.76))])), ("MeshRenderer", model(200 + m_h))]))]));
    }
    out
}

/// The things that are not in front of a door: a door is (the middle of its opening, the way out, its width); nothing stands within a metre or two to either side of it, from the wall out to the kerb.
pub fn clear_of_doors(spots: Vec<Spot>, doors: &[(V3, V3, f32)]) -> Vec<Spot> {
    spots
        .into_iter()
        .filter(|s| {
            let margin = if s.thing == Thing::Tree { 1.9 } else { 1.0 };
            doors.iter().all(|(mid, out, width)| {
                let v = [s.pos[0] - mid[0], s.pos[2] - mid[2]];
                let ahead = v[0] * out[0] + v[1] * out[2];
                let across = (v[0] * out[2] - v[1] * out[0]).abs();
                !(ahead > -1.0 && ahead < 8.0 && across < width * 0.5 + margin)
            })
        })
        .collect()
}

/// Level 0: the things of a block as entities (children of the block's prefab, whose origin is `origin`): each wears the shared model of its kind, turned and scaled, and the ones that can be bumped into have a simple
/// collider (a capsule for a trunk or a post, a box for a bench or a cabinet), so that nobody is stopped by a leaf. A shrub can be walked through.
pub fn entities(props: &Props, spots: &[Spot], origin: V3, harvest: bool, derelict: bool) -> Vec<lunaris_json::Json> {
    use lunaris_json::Json;
    let n = |x: f32| Json::Float(x as f64);
    let v3 = |a: [f32; 3]| Json::arr(a.map(n));
    let mut out = Vec::new();
    for s in spots {
        let (Some(p), Some(k)) = (props.get(s.slug), prop_index(s.slug)) else { continue };
        let local = [s.pos[0] - origin[0], s.pos[1] - origin[1], s.pos[2] - origin[2]];
        let size = p.size();
        // (a derelict city: the leaves are duller, the cars rustier)
        // (and no two trees alike: the same six models come back all down a street, so each gets its own tint from where it stands: most a dull green, some yellowing, a few dead and brown)
        let jit = |k: f32| ((s.pos[0] * 12.9898 + s.pos[2] * 78.233 + k * 37.719).sin() * 43758.547).fract().abs();
        let tint: [f32; 4] = match (derelict, s.thing) {
            (true, Thing::Tree | Thing::Shrub) if s.slug.starts_with("kit_") => [1.0, 1.0, 1.0, 1.0],
            (true, Thing::Tree | Thing::Shrub) => {
                // (a tint multiplies the leaf picture, which is green: only a tint with little green and blue turns it, so the dying ones are orange and brown and the yellowing ones olive)
                let (a, b, c) = (jit(1.0), jit(2.0), jit(3.0));
                if a < 0.16 {
                    [0.70 + 0.12 * b, 0.20 + 0.08 * c, 0.08, 1.0]
                } else if a < 0.28 {
                    [1.0, 0.30 + 0.12 * b, 0.07 + 0.04 * c, 1.0]
                } else if a < 0.48 {
                    [0.95, 0.58 + 0.14 * b, 0.12 + 0.08 * c, 1.0]
                } else {
                    [0.40 + 0.18 * b, 0.58 + 0.14 * c, 0.36 + 0.12 * b, 1.0]
                }
            }
            (true, Thing::Car) => {
                let b = jit(4.0);
                [0.52 + 0.20 * b, 0.48 + 0.16 * b, 0.44 + 0.10 * b, 1.0]
            }
            _ => [1.0, 1.0, 1.0, 1.0],
        };
        // a car is longer in z than in x: its long side goes along the street
        let yaw = s.yaw + if s.thing == Thing::Car && size[0] < size[2] { 90.0 } else { 0.0 };
        let half = (yaw.to_radians() * 0.5, 0.0);
        let rot = [0.0, half.0.sin(), 0.0, half.0.cos()];
        out.push(Json::obj([
            ("name", Json::str(s.slug)),
            ("components", Json::obj([
                ("LocalTransform", Json::obj([("pos", v3(local)), ("rot", Json::arr(rot.map(n))), ("scale", v3([s.scale; 3]))])),
                ("MeshRenderer", Json::obj([("mesh", Json::str(format!("city/gen_p_{k}.glb"))), ("color", Json::arr(tint.map(n))), ("cast_shadow", Json::Bool(matches!(s.thing, Thing::Tree | Thing::Car)))])),
            ])),
        ]));
        if s.thing == Thing::Lamp {
            // the lamp's light, at the head of the lamp; the scene's `lamps_on` system lights it at night (its intensity is 0 until then)
            out.push(Json::obj([
                ("name", Json::str("Lamp light")),
                ("components", Json::obj([
                    ("StreetLamp", Json::Obj(Vec::new())),
                    ("LocalTransform", Json::obj([("pos", v3([local[0], local[1] + 3.35 * s.scale, local[2]]))])),
                    ("Light", Json::obj([("kind", Json::from(1)), ("color", v3([1.0, 0.80, 0.50])), ("intensity", n(0.0)), ("range", n(24.0 * s.scale.min(1.4)))])),
                ])),
            ]));
        }
        let sc = s.scale;
        let collider = match s.thing {
            Thing::Tree => {
                let d = (0.045 * size[0].max(size[2]) * sc).clamp(0.22, 0.8);
                Some((2, [d, 3.2, d], 1.6))
            }
            Thing::Lamp => Some((2, [0.3, 3.4, 0.3], 1.7)),
            Thing::Hydrant => Some((2, [0.5, 0.8, 0.5], 0.4)),
            Thing::Bench | Thing::Cabinet | Thing::Car | Thing::Table | Thing::Planter => Some((1, [size[0] * sc, size[1] * sc, size[2] * sc], size[1] * sc * 0.5)),
            Thing::Shrub | Thing::Litter | Thing::Board => None,
        };
        if let Some((shape, dims, up)) = collider {
            let mut comps = vec![
                ("LocalTransform", Json::obj([("pos", v3([local[0], local[1] + up, local[2]])), ("rot", Json::arr(if shape == 1 { rot.map(n) } else { [0.0, 0.0, 0.0, 1.0].map(n) }))])),
                ("Collider", Json::obj([("shape", Json::from(shape)), ("size", v3(dims)), ("trigger", Json::Bool(false))])),
            ];
            // what the game can take from it (the scene's `harvest_use`): kind 1 wood from a tree (a hatchet), kind 2 scrap from a car (a crowbar), kind 3 water from a hydrant (nothing needed); `left` is how many times
            match s.thing {
                _ if !harvest => {}
                Thing::Tree => comps.push(("Harvest", Json::obj([("kind", n(1.0)), ("left", n(4.0))]))),
                Thing::Car => comps.push(("Harvest", Json::obj([("kind", n(2.0)), ("left", n(3.0))]))),
                Thing::Hydrant => comps.push(("Harvest", Json::obj([("kind", n(3.0)), ("left", n(3.0))]))),
                _ => {}
            }
            out.push(Json::obj([("name", Json::str("Collider")), ("components", Json::obj(comps))]));
        }
    }
    out
}

/// Level 1: a tree is three crossed cards wearing a picture of a clump of its leaves (the crown), on a thin trunk of two crossed cards of its bark... the trunk is left out: the crown reaches down far enough.
pub fn block_l1(mb: &mut MeshBuilder, props: &Props, spots: &[Spot]) {
    for s in spots {
        if !matches!(s.thing, Thing::Tree) {
            continue;
        }
        let Some(p) = props.get(s.slug) else { continue };
        let Some(mat) = p.crown else { continue };
        let size = p.size();
        let (w, h) = (size[0].max(size[2]) * s.scale * 1.0, size[1] * s.scale);
        let (cy, ch) = (s.pos[1] + h * 0.6, h * 0.62);
        let (sn, cs) = (s.yaw.to_radians().sin(), s.yaw.to_radians().cos());
        let col = [0.95; 3];
        let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        for k in 0..2 {
            let a = s.yaw.to_radians() + k as f32 * std::f32::consts::FRAC_PI_2;
            let d = [a.cos() * w * 0.5, a.sin() * w * 0.5];
            mb.quad(mat, [[s.pos[0] - d[0], cy - ch * 0.5, s.pos[2] - d[1]], [s.pos[0] + d[0], cy - ch * 0.5, s.pos[2] + d[1]], [s.pos[0] + d[0], cy + ch * 0.5, s.pos[2] + d[1]], [s.pos[0] - d[0], cy + ch * 0.5, s.pos[2] - d[1]]], uv, [col; 4], [-a.sin(), 0.0, a.cos()]);
        }
        let (dx, dz) = (cs * w * 0.5, sn * w * 0.5);
        mb.quad(mat, [[s.pos[0] - dx, cy + ch * 0.1, s.pos[2] - dz], [s.pos[0] + dx, cy + ch * 0.1, s.pos[2] - dz], [s.pos[0] + dx, cy + ch * 0.1, s.pos[2] + dz], [s.pos[0] - dx, cy + ch * 0.1, s.pos[2] + dz]], uv, [col; 4], [0.0, 1.0, 0.0]);
    }
}

/// What a park is: a lawn with a cross of paths, a meadow crossed by diagonal paths and thick with trees, a paved plaza with a fountain, a pair of sports pitches, a lawn with a pond, a playground.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParkKind {
    Green,
    Meadow,
    Plaza,
    Sports,
    Pond,
    Playground,
    /// Rubble where buildings stood (`Plan::with_ruins`): bare ground, nothing planted, nothing set up; the game's own builder dresses it.
    Ruin,
}

/// A park decided: its kind, and where its feature is (the pond's middle and radii, the pitch's, the sand's: `fx, fz` the middle, `hx, hz` the half sizes).
#[derive(Clone, Copy, Debug)]
pub struct ParkLayout {
    pub kind: ParkKind,
    pub cx: f32,
    pub cz: f32,
    pub fx: f32,
    pub fz: f32,
    pub hx: f32,
    pub hz: f32,
    /// A pitch lies along x (else along z).
    pub along_x: bool,
}

pub fn park_layout(plan: &Plan, i: i32, j: i32) -> ParkLayout {
    park_layout_rect(plan, i, j, plan.block_rect(i, j))
}

/// The layout of the park of block (i, j) in the rectangle `rect` (x0, x1, z0, z1) of some frame.
pub fn park_layout_rect(plan: &Plan, i: i32, j: i32, rect: (f32, f32, f32, f32)) -> ParkLayout {
    let (x0, x1, z0, z1) = rect;
    let (cx, cz) = ((x0 + x1) * 0.5, (z0 + z1) * 0.5);
    let mut r = Rng::keyed(&[plan.seed, i as u64, j as u64, 0x5041_524B]);
    let kind = match r.weighted(&[0.26, 0.14, 0.18, 0.16, 0.16, 0.10]) {
        0 => ParkKind::Green,
        1 => ParkKind::Meadow,
        2 => ParkKind::Plaza,
        3 => ParkKind::Sports,
        4 => ParkKind::Pond,
        _ => ParkKind::Playground,
    };
    let kind = if plan.in_plaza(i, j) { ParkKind::Plaza } else { kind };
    let kind = if plan.is_ruin(i, j) { ParkKind::Ruin } else { kind };
    let along_x = r.chance(0.5);
    let (fx, fz, hx, hz) = match kind {
        ParkKind::Sports => if along_x { (cx, cz - 6.0, 31.0, 19.0) } else { (cx - 6.0, cz, 19.0, 31.0) },
        ParkKind::Pond => (cx + r.range(-12.0, 12.0), cz + r.range(-12.0, 12.0), r.range(13.0, 20.0), r.range(10.0, 15.0)),
        ParkKind::Playground => (cx + r.range(-8.0, 8.0), cz + r.range(-8.0, 8.0), 11.0, 9.0),
        _ => (cx, cz, 0.0, 0.0),
    };
    ParkLayout { kind, cx, cz, fx, fz, hx, hz, along_x }
}

impl ParkLayout {
    /// Whether the point is where a path, paving, a pitch, the water or the sand is (no tree stands there).
    pub fn occupied(&self, x: f32, z: f32) -> bool {
        let (dx, dz) = (x - self.cx, z - self.cz);
        match self.kind {
            ParkKind::Green => dx.abs() < 3.4 || dz.abs() < 3.4,
            ParkKind::Meadow => (dx - dz).abs() < 4.8 || (dx + dz).abs() < 4.8,
            ParkKind::Plaza => dx.abs() < 36.0 && dz.abs() < 36.0,
            ParkKind::Sports => {
                let pitch = (x - self.fx).abs() < self.hx + 3.5 && (z - self.fz).abs() < self.hz + 3.5;
                let (bx, bz, bw, bd) = if self.along_x { (self.cx, self.cz + 33.0, 14.0, 7.5) } else { (self.cx + 33.0, self.cz, 7.5, 14.0) };
                pitch || ((x - bx).abs() < bw + 2.5 && (z - bz).abs() < bd + 2.5)
            }
            ParkKind::Pond => ((x - self.fx) / (self.hx + 3.5)).powi(2) + ((z - self.fz) / (self.hz + 3.5)).powi(2) < 1.0 || dx.abs() < 1.8 || dz.abs() < 1.8,
            ParkKind::Playground => (x - self.fx).abs() < self.hx + 2.5 && (z - self.fz).abs() < self.hz + 2.5 || dx.abs() < 1.8 || dz.abs() < 1.8,
            ParkKind::Ruin => false,
        }
    }
}

/// A strip of ground: a rectangle `length` long along the unit direction `along` and `width` across, centred on (cx, cz), laid with the material's picture at its real size.
#[allow(clippy::too_many_arguments)]
fn strip(mb: &mut MeshBuilder, lib: &Lib, mat: usize, cx: f32, cz: f32, along: [f32; 2], length: f32, width: f32, y: f32, col: V3) {
    let across = [-along[1], along[0]];
    let (l, w) = (length * 0.5, width * 0.5);
    let p = |a: f32, b: f32| [cx + along[0] * a + across[0] * b, y, cz + along[1] * a + across[1] * b];
    let q = [p(-l, -w), p(l, -w), p(l, w), p(-l, w)];
    let tex = lib.tex_plain(mat);
    mb.quad_tex(mat, q, [[q[0][0], q[0][2]], [q[1][0], q[1][2]], [q[2][0], q[2][2]], [q[3][0], q[3][2]]], &tex, col, [0.0, 1.0, 0.0]);
}

/// A ring (a thin band) round (cx, cz), `n` strips of it.
#[allow(clippy::too_many_arguments)]
fn ring(mb: &mut MeshBuilder, lib: &Lib, mat: usize, cx: f32, cz: f32, rx: f32, rz: f32, width: f32, y: f32, n: usize, col: V3) {
    for k in 0..n {
        let (a0, a1) = (k as f32 / n as f32 * std::f32::consts::TAU, (k + 1) as f32 / n as f32 * std::f32::consts::TAU);
        let (p0, p1) = ([cx + a0.cos() * rx, cz + a0.sin() * rz], [cx + a1.cos() * rx, cz + a1.sin() * rz]);
        let mid = [(p0[0] + p1[0]) * 0.5, (p0[1] + p1[1]) * 0.5];
        let d = [p1[0] - p0[0], p1[1] - p0[1]];
        let l = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-4);
        strip(mb, lib, mat, mid[0], mid[1], [d[0] / l, d[1] / l], l * 1.04, width, y, col);
    }
}

/// A filled ellipse (a pond), as a fan of quads each running from the middle to two points on the rim.
#[allow(clippy::too_many_arguments)]
fn disc(mb: &mut MeshBuilder, mat: usize, cx: f32, cz: f32, rx: f32, rz: f32, y: f32, n: usize, col: V3) {
    for k in 0..n {
        let (a0, a1) = (k as f32 / n as f32 * std::f32::consts::TAU, (k + 1) as f32 / n as f32 * std::f32::consts::TAU);
        let p0 = [cx + a0.cos() * rx, y, cz + a0.sin() * rz];
        let p1 = [cx + a1.cos() * rx, y, cz + a1.sin() * rz];
        mb.quad(mat, [[cx, y, cz], p0, p1, p1], [[0.0; 2]; 4], [col; 4], [0.0, 1.0, 0.0]);
    }
}

/// The ground of a park at level 0, by kind (the pavement round the block and the streets are `lod::ground_l0`'s).
pub fn park_ground(mb: &mut MeshBuilder, lib: &Lib, g: &Ground, plan: &Plan, i: i32, j: i32) {
    let lay = park_layout(plan, i, j);
    park_ground_rect(mb, lib, g, plan.section.kerb_h, &lay, plan.block_rect(i, j), true);
}

/// The ground of a park laid out in `rect` (x0, x1, z0, z1) of some frame, at height `y`; `with_lawn` is false where the caller has laid the lawn itself.
pub(crate) fn park_ground_rect(mb: &mut MeshBuilder, lib: &Lib, g: &Ground, y: f32, lay: &ParkLayout, rect: (f32, f32, f32, f32), with_lawn: bool) {
    use crate::street::flat;
    let (x0, x1, z0, z1) = rect;
    let (cx, cz) = (lay.cx, lay.cz);
    let id = |mb: &mut MeshBuilder, mat: usize, x: f32, z: f32, a: [f32; 2], l: f32, w: f32, h: f32| strip(mb, lib, mat, x, z, a, l, w, y + h, [1.0; 3]);
    let steel = lib.id("steel");
    let st = lib.tex_plain(steel);
    let fr = Frame { o: [0.0; 3], a: [1.0, 0.0, 0.0], b: [0.0, 1.0, 0.0], c: [0.0, 0.0, 1.0] };
    match lay.kind {
        ParkKind::Ruin => {
            if with_lawn {
                flat(mb, lib, g.path, x0, x1, z0, z1, y, [0.7; 3]);
            }
        }
        ParkKind::Green => {
            if with_lawn {
                flat(mb, lib, g.lawn, x0, x1, z0, z1, y, LAWN_TINT);
            }
            flat(mb, lib, g.path, x0, x1, cz - 1.6, cz + 1.6, y + 0.012, [1.0; 3]);
            flat(mb, lib, g.path, cx - 1.6, cx + 1.6, z0, cz - 1.6, y + 0.012, [1.0; 3]);
            flat(mb, lib, g.path, cx - 1.6, cx + 1.6, cz + 1.6, z1, y + 0.012, [1.0; 3]);
        }
        ParkKind::Meadow => {
            if with_lawn {
                flat(mb, lib, g.lawn, x0, x1, z0, z1, y, LAWN_TINT);
            }
            let d = std::f32::consts::FRAC_1_SQRT_2;
            let reach = ((x1 - x0) * 0.5).min((z1 - z0) * 0.5) * 1.35;
            id(mb, g.path, cx, cz, [d, d], reach * 2.0, 3.0, 0.012);
            id(mb, g.path, cx, cz, [d, -d], reach * 2.0, 3.0, 0.014);
        }
        ParkKind::Plaza => {
            // a lawn border, a paved square, a fountain in the middle
            if with_lawn {
                flat(mb, lib, g.lawn, x0, x1, z0, z1, y, LAWN_TINT);
            }
            let h = 36.0f32.min((x1 - x0) * 0.5 - 3.0).min((z1 - z0) * 0.5 - 3.0);
            flat(mb, lib, g.court, cx - h, cx + h, cz - h, cz + h, y + 0.012, [0.95; 3]);
            let (wall, t) = (lib.id("concrete_wall_007"), 0.55);
            let wt = lib.tex_plain(wall);
            let r = 6.5;
            for (a, b) in [([-r, -r], [r, -r + t]), ([-r, r - t], [r, r]), ([-r, -r], [-r + t, r]), ([r - t, -r], [r, r])] {
                mb.cuboid(wall, &fr, [cx + a[0], y, cz + a[1]], [cx + b[0], y + 0.62, cz + b[1]], 63, &wt, [0.95; 3]);
            }
            flat(mb, lib, lib.id("glass_teal"), cx - r + t, cx + r - t, cz - r + t, cz + r - t, y + 0.46, [1.0; 3]);
            // a column with two bowls
            mb.cuboid(wall, &fr, [cx - 0.35, y, cz - 0.35], [cx + 0.35, y + 2.6, cz + 0.35], 63, &wt, [0.95; 3]);
            mb.cuboid(wall, &fr, [cx - 1.6, y + 1.5, cz - 1.6], [cx + 1.6, y + 1.8, cz + 1.6], 63, &wt, [0.95; 3]);
            mb.cuboid(wall, &fr, [cx - 0.9, y + 2.4, cz - 0.9], [cx + 0.9, y + 2.65, cz + 0.9], 63, &wt, [0.95; 3]);
        }
        ParkKind::Sports => {
            if with_lawn {
                flat(mb, lib, g.lawn, x0, x1, z0, z1, y, LAWN_TINT);
            }
            let white = g.white;
            let line = |mb: &mut MeshBuilder, ax: f32, az: f32, bx: f32, bz: f32| {
                let (d0, d1) = (bx - ax, bz - az);
                let l = (d0 * d0 + d1 * d1).sqrt().max(1e-3);
                strip(mb, lib, white, (ax + bx) * 0.5, (az + bz) * 0.5, [d0 / l, d1 / l], l, 0.14, y + 0.02, [1.0; 3]);
            };
            // the pitch: darker mown stripes, the outline, the halfway line, the centre circle, the boxes, the goals
            let (px, pz, hx, hz) = (lay.fx, lay.fz, lay.hx, lay.hz);
            let n = 8;
            for k in 0..n {
                if k % 2 == 0 {
                    let (a, b) = if lay.along_x { (px - hx + 2.0 * hx * k as f32 / n as f32, px - hx + 2.0 * hx * (k + 1) as f32 / n as f32) } else { (pz - hz + 2.0 * hz * k as f32 / n as f32, pz - hz + 2.0 * hz * (k + 1) as f32 / n as f32) };
                    if lay.along_x {
                        flat(mb, lib, g.lawn, a, b, pz - hz, pz + hz, y + 0.008, [0.70, 1.0, 0.54]);
                    } else {
                        flat(mb, lib, g.lawn, px - hx, px + hx, a, b, y + 0.008, [0.70, 1.0, 0.54]);
                    }
                }
            }
            line(mb, px - hx, pz - hz, px + hx, pz - hz);
            line(mb, px - hx, pz + hz, px + hx, pz + hz);
            line(mb, px - hx, pz - hz, px - hx, pz + hz);
            line(mb, px + hx, pz - hz, px + hx, pz + hz);
            if lay.along_x {
                line(mb, px, pz - hz, px, pz + hz);
                for s in [-1.0f32, 1.0] {
                    let gx = px + s * hx;
                    line(mb, gx, pz - 8.5, gx - s * 11.0, pz - 8.5);
                    line(mb, gx, pz + 8.5, gx - s * 11.0, pz + 8.5);
                    line(mb, gx - s * 11.0, pz - 8.5, gx - s * 11.0, pz + 8.5);
                    // a goal: two posts and a bar, 7.3 m wide
                    for dz in [-3.65f32, 3.65] {
                        mb.cuboid(steel, &fr, [gx - 0.06, y, pz + dz - 0.06], [gx + 0.06, y + 2.44, pz + dz + 0.06], 63, &st, [0.95; 3]);
                    }
                    mb.cuboid(steel, &fr, [gx - 0.06, y + 2.38, pz - 3.65], [gx + 0.06, y + 2.5, pz + 3.65], 63, &st, [0.95; 3]);
                }
            } else {
                line(mb, px - hx, pz, px + hx, pz);
                for s in [-1.0f32, 1.0] {
                    let gz = pz + s * hz;
                    line(mb, px - 8.5, gz, px - 8.5, gz - s * 11.0);
                    line(mb, px + 8.5, gz, px + 8.5, gz - s * 11.0);
                    line(mb, px - 8.5, gz - s * 11.0, px + 8.5, gz - s * 11.0);
                    for dx in [-3.65f32, 3.65] {
                        mb.cuboid(steel, &fr, [px + dx - 0.06, y, gz - 0.06], [px + dx + 0.06, y + 2.44, gz + 0.06], 63, &st, [0.95; 3]);
                    }
                    mb.cuboid(steel, &fr, [px - 3.65, y + 2.38, gz - 0.06], [px + 3.65, y + 2.5, gz + 0.06], 63, &st, [0.95; 3]);
                }
            }
            ring(mb, lib, white, px, pz, 9.15, 9.15, 0.14, y + 0.02, 28, [1.0; 3]);
            // a basketball court in the corner that the pitch leaves
            let (bx, bz) = if lay.along_x { (cx, cz + 33.0) } else { (cx + 33.0, cz) };
            let (bw, bd) = if lay.along_x { (14.0, 7.5) } else { (7.5, 14.0) };
            flat(mb, lib, g.asphalt, bx - bw, bx + bw, bz - bd, bz + bd, y + 0.012, [1.0; 3]);
            line(mb, bx - bw, bz - bd, bx + bw, bz - bd);
            line(mb, bx - bw, bz + bd, bx + bw, bz + bd);
            line(mb, bx - bw, bz - bd, bx - bw, bz + bd);
            line(mb, bx + bw, bz - bd, bx + bw, bz + bd);
            if lay.along_x {
                line(mb, bx, bz - bd, bx, bz + bd);
                for s in [-1.0f32, 1.0] {
                    mb.cuboid(steel, &fr, [bx + s * (bw - 1.0) - 0.07, y, bz - 0.07], [bx + s * (bw - 1.0) + 0.07, y + 3.05, bz + 0.07], 63, &st, [0.9; 3]);
                    mb.cuboid(steel, &fr, [bx + s * (bw - 1.0) - 0.04, y + 2.7, bz - 0.9], [bx + s * (bw - 1.0) + 0.04, y + 3.6, bz + 0.9], 63, &st, [1.0; 3]);
                }
            } else {
                line(mb, bx - bw, bz, bx + bw, bz);
                for s in [-1.0f32, 1.0] {
                    mb.cuboid(steel, &fr, [bx - 0.07, y, bz + s * (bd - 1.0) - 0.07], [bx + 0.07, y + 3.05, bz + s * (bd - 1.0) + 0.07], 63, &st, [0.9; 3]);
                    mb.cuboid(steel, &fr, [bx - 0.9, y + 2.7, bz + s * (bd - 1.0) - 0.04], [bx + 0.9, y + 3.6, bz + s * (bd - 1.0) + 0.04], 63, &st, [1.0; 3]);
                }
            }
        }
        ParkKind::Pond => {
            if with_lawn {
                flat(mb, lib, g.lawn, x0, x1, z0, z1, y, LAWN_TINT);
            }
            // a path ringing the pond and a cross of paths to the street, the water a hair above the lawn
            ring(mb, lib, g.path, lay.fx, lay.fz, lay.hx + 2.2, lay.hz + 2.2, 2.4, y + 0.012, 28, [1.0; 3]);
            flat(mb, lib, g.path, x0, lay.fx - lay.hx - 3.4, cz - 1.0, cz + 1.0, y + 0.014, [1.0; 3]);
            flat(mb, lib, g.path, lay.fx + lay.hx + 3.4, x1, cz - 1.0, cz + 1.0, y + 0.014, [1.0; 3]);
            flat(mb, lib, g.path, cx - 1.0, cx + 1.0, z0, lay.fz - lay.hz - 3.4, y + 0.014, [1.0; 3]);
            flat(mb, lib, g.path, cx - 1.0, cx + 1.0, lay.fz + lay.hz + 3.4, z1, y + 0.014, [1.0; 3]);
            ring(mb, lib, lib.id("concrete_wall_007"), lay.fx, lay.fz, lay.hx + 0.35, lay.hz + 0.35, 0.7, y + 0.1, 28, [0.8; 3]);
            disc(mb, lib.id("glass_teal"), lay.fx, lay.fz, lay.hx, lay.hz, y + 0.06, 28, [1.0; 3]);
        }
        ParkKind::Playground => {
            if with_lawn {
                flat(mb, lib, g.lawn, x0, x1, z0, z1, y, LAWN_TINT);
            }
            flat(mb, lib, g.path, x0, x1, cz - 1.0, cz + 1.0, y + 0.012, [1.0; 3]);
            flat(mb, lib, g.path, cx - 1.0, cx + 1.0, z0, z1, y + 0.012, [1.0; 3]);
            let (sx, sz, hx, hz) = (lay.fx, lay.fz, lay.hx, lay.hz);
            flat(mb, lib, g.path, sx - hx, sx + hx, sz - hz, sz + hz, y + 0.02, [1.0, 0.88, 0.62]);
            let frame = lib.id("frame_dark");
            let ft = lib.tex_plain(frame);
            let seat = lib.id("paint_yellow");
            let sett = lib.tex_plain(seat);
            // a swing set: two A-frames and a bar, three swings
            let (wx, wz) = (sx - hx * 0.45, sz - hz * 0.5);
            for e in [-3.2f32, 3.2] {
                mb.cuboid(frame, &fr, [wx + e - 0.05, y, wz - 0.9], [wx + e + 0.05, y + 2.5, wz - 0.8], 63, &ft, [1.0; 3]);
                mb.cuboid(frame, &fr, [wx + e - 0.05, y, wz + 0.8], [wx + e + 0.05, y + 2.5, wz + 0.9], 63, &ft, [1.0; 3]);
            }
            mb.cuboid(frame, &fr, [wx - 3.3, y + 2.45, wz - 0.04], [wx + 3.3, y + 2.55, wz + 0.04], 63, &ft, [1.0; 3]);
            for e in [-1.8f32, 0.0, 1.8] {
                mb.cuboid(steel, &fr, [wx + e - 0.45, y + 0.5, wz - 0.14], [wx + e + 0.45, y + 0.56, wz + 0.14], 63, &sett, [1.0; 3]);
                for c in [-0.4f32, 0.4] {
                    mb.cuboid(steel, &fr, [wx + e + c - 0.012, y + 0.56, wz - 0.012], [wx + e + c + 0.012, y + 2.5, wz + 0.012], 63, &st, [1.0; 3]);
                }
            }
            // a slide: a platform on legs, a ramp down and a ladder up
            let (qx, qz) = (sx + hx * 0.4, sz + hz * 0.35);
            mb.cuboid(seat, &fr, [qx - 0.8, y + 1.7, qz - 0.8], [qx + 0.8, y + 1.78, qz + 0.8], 63, &sett, [1.0; 3]);
            for (a, b) in [(-0.75f32, -0.75f32), (0.7, -0.75), (-0.75, 0.7), (0.7, 0.7)] {
                mb.cuboid(frame, &fr, [qx + a - 0.04, y, qz + b - 0.04], [qx + a + 0.04, y + 1.7, qz + b + 0.04], 63, &ft, [1.0; 3]);
            }
            mb.quad(lib.id("steel"), [[qx - 0.45, y + 1.78, qz + 0.8], [qx + 0.45, y + 1.78, qz + 0.8], [qx + 0.45, y + 0.12, qz + 3.6], [qx - 0.45, y + 0.12, qz + 3.6]], [[0.0; 2]; 4], [[0.9; 3]; 4], [0.0, 1.0, 0.0]);
            for rungs in 0..5 {
                let h = 0.3 + rungs as f32 * 0.32;
                mb.cuboid(steel, &fr, [qx - 0.35, y + h, qz - 0.86], [qx + 0.35, y + h + 0.04, qz - 0.8], 63, &st, [1.0; 3]);
            }
        }
    }
}

/// Grass in the picture is a dry, tan lawn; this makes it a green one.
pub const LAWN_TINT: [f32; 3] = [0.80, 1.14, 0.62];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_street_block_has_trees_lamps_and_a_park_has_many_more_trees_and_every_spot_is_near_its_block() {
        let plan = Plan::new(3);
        let (mut streets, mut parks) = (0, 0);
        for j in -30..30 {
            for i in -30..30 {
                let sp = spots(&plan, i, j);
                let (x0, x1, z0, z1) = plan.block_rect(i, j);
                let trees = sp.iter().filter(|s| s.thing == Thing::Tree).count();
                // (cars stand in the streets, outside the block)
                for s in sp.iter().filter(|s| s.thing != Thing::Car) {
                    assert!(s.pos[0] > x0 - 4.5 && s.pos[0] < x1 + 4.5 && s.pos[2] > z0 - 4.5 && s.pos[2] < z1 + 4.5, "{s:?} outside block {i} {j}");
                    assert!(s.scale > 0.0);
                }
                if plan.landuse(i, j) == LandUse::Park {
                    assert!(trees >= 14, "a park of {trees} trees");
                    assert!(sp.iter().any(|s| s.thing == Thing::Bench));
                    parks += 1;
                } else {
                    assert!(trees >= 8, "a street of {trees} trees");
                    assert!(sp.iter().any(|s| s.thing == Thing::Lamp));
                    streets += 1;
                }
                // the same every time
                assert_eq!(sp.len(), spots(&plan, i, j).len());
            }
        }
        assert!(streets > 500 && parks > 20, "{streets} blocks of buildings, {parks} parks");
    }
}
