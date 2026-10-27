//! The lots of a city on a grid, for many seeds and blocks: each lot is inside its block, has the size its kind needs and no two lots of a block overlap. A city is made from hashes of a seed, so a mistake that
//! shows only for some seeds and some blocks (two buildings standing in one another) is found by trying many.
use crate::building::{Kind, Lot};
use crate::plan::Plan;

/// The rectangle (x0, x1, z0, z1) a lot covers: the lots of this plan run along the axes.
fn rect_of(l: &Lot) -> (f32, f32, f32, f32) {
    let corners = [l.at(0.0, 0.0), l.at(l.w, 0.0), l.at(0.0, l.d), l.at(l.w, l.d)];
    let xs = corners.iter().map(|c| c[0]);
    let zs = corners.iter().map(|c| c[2]);
    (xs.clone().fold(f32::MAX, f32::min), xs.fold(f32::MIN, f32::max), zs.clone().fold(f32::MAX, f32::min), zs.fold(f32::MIN, f32::max))
}

/// How far a building whose box is `lo`..`hi` reaches over each edge of its lot: [front (z = 0), right (x = w), back (z = d), left (x = 0)], in metres, 0 when it stays inside.
fn overhang(l: &Lot, lo: [f32; 3], hi: [f32; 3]) -> [f32; 4] {
    let local = |c: [f32; 3]| {
        let d = [c[0] - l.o[0], c[1] - l.o[1], c[2] - l.o[2]];
        (d[0] * l.ex[0] + d[1] * l.ex[1] + d[2] * l.ex[2], d[0] * l.ez[0] + d[1] * l.ez[1] + d[2] * l.ez[2])
    };
    let corners = [local([lo[0], 0.0, lo[2]]), local([hi[0], 0.0, lo[2]]), local([lo[0], 0.0, hi[2]]), local([hi[0], 0.0, hi[2]])];
    let (x0, x1) = corners.iter().fold((f32::MAX, f32::MIN), |a, c| (a.0.min(c.0), a.1.max(c.0)));
    let (z0, z1) = corners.iter().fold((f32::MAX, f32::MIN), |a, c| (a.0.min(c.1), a.1.max(c.1)));
    [(-z0).max(0.0), (x1 - l.w).max(0.0), (z1 - l.d).max(0.0), (-x0).max(0.0)]
}

/// The most a building may reach over an edge of its lot, by what lies beyond it (measured over 3 seeds and 49 blocks, `overhang_of_buildings_over_their_lot_edges`): balconies, awnings and cornices
/// reach over a street (1.3 m) or an open back (2.0 m); a shared wall is not crossed by more than a few centimetres of trim.
fn most_over(beyond: crate::building::Side) -> f32 {
    use crate::building::Side;
    match beyond {
        Side::Party => 0.45,
        Side::Street => 1.4,
        Side::Open => 2.1,
    }
}

#[test]
fn lots_of_many_seeds_and_blocks_are_inside_their_block_and_do_not_overlap() {
    let (mut blocks, mut lots) = (0usize, 0usize);
    let mut bad: Vec<String> = Vec::new();
    for seed in 1..=40u64 {
        let plan = Plan::new(seed);
        for i in -8..=8 {
            for j in -8..=8 {
                let (bx0, bx1, bz0, bz1) = plan.block_rect(i, j);
                let planned = plan.lots(i, j);
                blocks += 1;
                lots += planned.len();
                let rects: Vec<(f32, f32, f32, f32)> = planned.iter().map(|p| rect_of(&p.lot)).collect();
                for (k, p) in planned.iter().enumerate() {
                    let at = format!("seed {seed} block ({i}, {j}) lot {k} ({:?} {:.1} x {:.1})", p.kind, p.lot.w, p.lot.d);
                    if !(p.lot.w.is_finite() && p.lot.d.is_finite() && p.lot.w > 0.0 && p.lot.d > 0.0) {
                        bad.push(format!("{at}: no size"));
                        continue;
                    }
                    let r = rects[k];
                    if r.0 < bx0 - 0.02 || r.1 > bx1 + 0.02 || r.2 < bz0 - 0.02 || r.3 > bz1 + 0.02 {
                        bad.push(format!("{at}: sticks out of its block ({:.1}..{:.1}, {:.1}..{:.1} against {bx0:.1}..{bx1:.1}, {bz0:.1}..{bz1:.1})", r.0, r.1, r.2, r.3));
                    }
                    for (m, q) in rects.iter().enumerate().skip(k + 1) {
                        let (ox, oz) = ((r.1.min(q.1) - r.0.max(q.0)), (r.3.min(q.3) - r.2.max(q.2)));
                        if ox > 0.05 && oz > 0.05 {
                            bad.push(format!("{at}: overlaps lot {m} ({:?}) by {ox:.1} x {oz:.1} m", planned[m].kind));
                        }
                    }
                }
            }
        }
    }
    assert!(blocks > 10_000 && lots > 20_000, "{blocks} blocks, {lots} lots");
    assert!(bad.is_empty(), "{} problems in {blocks} blocks ({lots} lots); the first ones:\n{}", bad.len(), bad.iter().take(12).cloned().collect::<Vec<_>>().join("\n"));
}

/// A building on each of the lots a plan makes (the shapes and kinds a city really has, not one lot for each kind): it has triangles, a door and a furnished inside, and it stays over its lot
/// (a balcony or an awning may reach over the street in front, a cornice a little over the sides).
#[test]
fn a_building_on_every_lot_of_some_blocks_has_doors_stays_over_its_lot_and_is_finite() {
    use crate::building::generate;
    use crate::materials::Lib;
    use crate::mesh::MeshBuilder;
    use crate::props::Props;
    use crate::rng::Rng;
    let lib = Lib::from_json("");
    let props = Props { map: Default::default() };
    let (mut made, mut bad) = (0usize, Vec::<String>::new());
    let t = std::time::Instant::now();
    for seed in 1..=3u64 {
        let plan = Plan::new(seed);
        for i in -2..=2 {
            for j in -2..=2 {
                for p in plan.lots(i, j) {
                    let l = p.lot;
                    let mut mb = MeshBuilder::new();
                    let mut rng = Rng::keyed(&[seed, i as u64, j as u64, p.index as u64]);
                    let built = generate(&mut mb, &lib, &props, &l, p.kind, &mut rng);
                    made += 1;
                    let at = format!("seed {seed} block ({i}, {j}) lot {} ({:?} {:.1} x {:.1})", p.index, p.kind, l.w, l.d);
                    let Some((lo, hi)) = mb.bounds() else {
                        bad.push(format!("{at}: nothing was built"));
                        continue;
                    };
                    if !lo.iter().chain(hi.iter()).all(|v| v.is_finite()) || !built.height.is_finite() {
                        bad.push(format!("{at}: not finite ({lo:?} {hi:?} height {})", built.height));
                        continue;
                    }
                    for (k, over) in overhang(&l, lo, hi).iter().enumerate() {
                        if *over > most_over(l.sides[k]) {
                            bad.push(format!("{at}: reaches {over:.2} m over its {} edge, which has {:?} beyond it (at most {:.2})", ["front", "right", "back", "left"][k], l.sides[k], most_over(l.sides[k])));
                        }
                    }
                    if mb.triangles() < 100 {
                        bad.push(format!("{at}: only {} triangles", mb.triangles()));
                    }
                    if built.doors.is_empty() {
                        bad.push(format!("{at}: no door"));
                    }
                    if built.interior.is_empty() {
                        bad.push(format!("{at}: no inside"));
                    }
                }
            }
        }
    }
    println!("{made} buildings in {:.1} s", t.elapsed().as_secs_f64());
    assert!(made > 300, "{made} buildings");
    assert!(bad.is_empty(), "{} of {made} buildings; the first ones:\n{}", bad.len(), bad.iter().take(12).cloned().collect::<Vec<_>>().join("\n"));
}

/// How far each kind of building reaches over each kind of lot edge (a probe that prints a table): `cargo test --release -p lunaris-citygen overhang -- --ignored --nocapture`.
#[test]
#[ignore]
fn overhang_of_buildings_over_their_lot_edges() {
    use crate::building::generate;
    use crate::materials::Lib;
    use crate::mesh::MeshBuilder;
    use crate::props::Props;
    use crate::rng::Rng;
    use std::collections::BTreeMap;
    let lib = Lib::from_json("");
    let props = Props { map: Default::default() };
    // (edge, what lies beyond it, building kind) -> the most it reached over, and how many buildings reached over by more than 10 cm
    let mut worst: BTreeMap<(String, String, String), (f32, usize, usize)> = BTreeMap::new();
    for seed in 1..=3u64 {
        let plan = Plan::new(seed);
        for i in -3..=3 {
            for j in -3..=3 {
                for p in plan.lots(i, j) {
                    let l = p.lot;
                    let mut mb = MeshBuilder::new();
                    let mut rng = Rng::keyed(&[seed, i as u64, j as u64, p.index as u64]);
                    let _ = generate(&mut mb, &lib, &props, &l, p.kind, &mut rng);
                    let Some((lo, hi)) = mb.bounds() else { continue };
                    let over = overhang(&l, lo, hi);
                    for (k, name) in ["front", "right", "back", "left"].iter().enumerate() {
                        let e = worst.entry((name.to_string(), format!("{:?}", l.sides[k]), format!("{:?}", p.kind))).or_insert((0.0, 0, 0));
                        e.0 = e.0.max(over[k]);
                        e.2 += 1;
                        if over[k] > 0.1 {
                            e.1 += 1;
                        }
                    }
                }
            }
        }
    }
    for ((edge, beyond, kind), (most, n, of)) in &worst {
        if *most > 0.1 {
            println!("{edge:>5} edge, {beyond:>6} beyond, {kind:<10}: up to {most:.2} m over, {n} of {of} reach over by more than 10 cm");
        }
    }
}

/// The same for the round city (the dome city's plan): its lots are turned and cut to the shape of a block, so only what does not depend on the turn is asked: a house is as wide as a house needs,
/// every building has a door and a furnished inside, and nothing is made of numbers that are not numbers.
#[test]
fn a_building_on_every_lot_of_a_round_city_has_doors_and_an_inside_and_a_house_is_wide_enough() {
    use crate::building::generate;
    use crate::materials::Lib;
    use crate::mesh::MeshBuilder;
    use crate::polar::{Polar, BANDS};
    use crate::props::Props;
    use crate::rng::Rng;
    let lib = Lib::from_json("");
    let props = Props { map: Default::default() };
    let (mut made, mut houses, mut bad) = (0usize, 0usize, Vec::<String>::new());
    for seed in 1..=4u64 {
        let plan = Plan::new(seed).with_polar(Some(Polar::default()));
        let counts = plan.polar.unwrap().counts;
        for band in 0..BANDS {
            for sector in (0..counts[band as usize] as i32).step_by(3) {
                for p in plan.lots(band, sector) {
                    let l = p.lot;
                    let at = format!("seed {seed} block ({band}, {sector}) lot {} ({:?} {:.1} x {:.1})", p.index, p.kind, l.w, l.d);
                    if p.kind == Kind::House {
                        houses += 1;
                        if l.w < crate::plan::HOUSE_MIN_WIDTH {
                            bad.push(format!("{at}: a house on a lot narrower than {} m", crate::plan::HOUSE_MIN_WIDTH));
                        }
                    }
                    let mut mb = MeshBuilder::new();
                    let mut rng = Rng::keyed(&[seed, band as u64, sector as u64, p.index as u64]);
                    let built = generate(&mut mb, &lib, &props, &l, p.kind, &mut rng);
                    made += 1;
                    match mb.bounds() {
                        Some((lo, hi)) if lo.iter().chain(hi.iter()).all(|v| v.is_finite()) => {}
                        _ => bad.push(format!("{at}: nothing or not finite was built")),
                    }
                    if built.doors.is_empty() {
                        bad.push(format!("{at}: no door"));
                    }
                    // (a tower or a skyscraper may have no rooms: measured on 4 seeds, 36 of 207 towers and 7 of 79 skyscrapers of the round city have none, every other kind has them all)
                    if built.interior.is_empty() && !matches!(p.kind, Kind::Tower | Kind::Skyscraper) {
                        bad.push(format!("{at}: no inside"));
                    }
                }
            }
        }
    }
    println!("{made} buildings, {houses} houses");
    assert!(made > 300 && houses > 5, "{made} buildings, {houses} houses");
    assert!(bad.is_empty(), "{} of {made} buildings; the first ones:\n{}", bad.len(), bad.iter().take(12).cloned().collect::<Vec<_>>().join("\n"));
}
