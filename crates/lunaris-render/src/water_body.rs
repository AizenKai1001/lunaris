//! The mesh of a `WaterBody`: the surface the water's picture describes (`lunaris_sim::water`), as triangles in the entity's own space (x and z from -size/2 to size/2, y the surface's height
//! above the entity). Only the cells that have water in them are made, so a river on a big map is a thin ribbon of triangles, not a sheet the size of the map. The GPU renderer rolls waves over
//! it like over a `Water` sheet; the reference renderer draws it as it is.
use crate::math::{cross, normalize, sub};
use crate::mesh::Mesh;
use lunaris_sim::components::WaterBody;
use lunaris_sim::terrain::Heightfield;
use lunaris_sim::water::PlacedWater;

/// The surface of the water, or `None` when the picture has no water in it at all.
pub fn build_mesh(field: &Heightfield, w: &WaterBody) -> Option<Mesh> {
    let p = PlacedWater::new(field, [0.0, 0.0, 0.0], w);
    let (nx, nz) = (p.nx, p.nz);
    let stride = nx + 1;
    // which cells have water, and which vertices they use
    let mut used = vec![false; stride * (nz + 1)];
    let mut cells: Vec<(usize, usize)> = Vec::new();
    for j in 0..nz {
        for i in 0..nx {
            if p.cell_wet(i, j) {
                cells.push((i, j));
                for (a, b) in [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)] {
                    used[b * stride + a] = true;
                }
            }
        }
    }
    if cells.is_empty() {
        return None;
    }
    let mut index = vec![u32::MAX; used.len()];
    let (mut pos, mut nrm, mut uv) = (Vec::new(), Vec::new(), Vec::new());
    for j in 0..=nz {
        for i in 0..=nx {
            if !used[j * stride + i] {
                continue;
            }
            // every vertex of a wet cell has a height (a dry one takes the average of the wet ones around it)
            let Some(v) = p.vertex(i, j) else { continue };
            index[j * stride + i] = pos.len() as u32;
            pos.push(v);
            let at = |a: usize, b: usize| p.vertex(a, b).unwrap_or(v);
            let (l, r) = (at(i.saturating_sub(1), j), at((i + 1).min(nx), j));
            let (u, d) = (at(i, j.saturating_sub(1)), at(i, (j + 1).min(nz)));
            let n = normalize(cross(sub(d, u), sub(r, l)));
            nrm.push(if n[1].is_finite() && n[1] > 0.0 { n } else { [0.0, 1.0, 0.0] });
            uv.push([i as f32 / nx as f32, j as f32 / nz as f32]);
        }
    }
    let mut idx = Vec::with_capacity(cells.len() * 6);
    for (i, j) in cells {
        let v00 = index[j * stride + i];
        let (v10, v01, v11) = (index[j * stride + i + 1], index[(j + 1) * stride + i], index[(j + 1) * stride + i + 1]);
        if [v00, v10, v01, v11].contains(&u32::MAX) {
            continue;
        }
        // the terrain's triangles, counter-clockwise seen from above
        idx.extend_from_slice(&[v00, v01, v10, v10, v01, v11]);
    }
    if idx.is_empty() {
        return None;
    }
    Some(Mesh { pos, nrm, uv, col: Vec::new(), texw: Vec::new(), idx, tex: None, rig: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5 x 5: a lake of three samples at level 0.5 in the middle row, a stream of two at 0.25 in the corner.
    fn field() -> Heightfield {
        let mut s = vec![0u16; 25];
        for i in 1..4 {
            s[2 * 5 + i] = 32768;
        }
        s[4 * 5 + 3] = 16384;
        s[4 * 5 + 4] = 16384;
        Heightfield::new(5, 5, s).unwrap()
    }

    #[test]
    fn only_the_cells_with_water_are_made_and_they_face_up_at_the_height_of_the_picture() {
        let f = field();
        let w = WaterBody { size: [40.0, 40.0], relief: 20.0, ..WaterBody::default() };
        let m = build_mesh(&f, &w).expect("there is water");
        // fewer triangles than a full sheet of 4 x 4 cells (32): the dry corners are left out
        assert!(m.idx.len() % 3 == 0 && m.idx.len() < 4 * 4 * 6 && m.idx.len() >= 6 * 6, "{} indices", m.idx.len());
        assert_eq!(m.pos.len(), m.nrm.len());
        for t in m.idx.chunks(3) {
            let (a, b, c) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
            assert!(cross(sub(b, a), sub(c, a))[1] > 0.0, "{a:?} {b:?} {c:?}");
        }
        // the lake is at 0.5 of the relief (10 m), the stream lower (5 m); both are where the picture says
        let (lo, hi) = m.pos.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
        assert!((hi - 10.0).abs() < 0.01 && lo > 4.9 && lo < 5.1, "{lo} {hi}");
        let at_lake = m.pos.iter().find(|p| p[0] == 0.0 && p[2] == 0.0).expect("the lake's middle");
        assert!((at_lake[1] - 10.0).abs() < 0.01);
        // no water: no mesh
        assert!(build_mesh(&Heightfield::new(3, 3, vec![0; 9]).unwrap(), &w).is_none());
    }

    #[test]
    fn a_stream_that_steps_down_a_valley_slopes_its_normals() {
        // a row of wet samples falling from 0.8 to 0.2 along x
        let mut s = vec![0u16; 25];
        for (i, v) in [0.8f32, 0.65, 0.5, 0.35, 0.2].iter().enumerate() {
            s[2 * 5 + i] = (v * 65535.0) as u16;
        }
        let f = Heightfield::new(5, 5, s).unwrap();
        let w = WaterBody { size: [40.0, 40.0], relief: 20.0, ..WaterBody::default() };
        let m = build_mesh(&f, &w).unwrap();
        let k = m.pos.iter().position(|p| p[0] == 0.0 && p[2] == 0.0).unwrap();
        assert!(m.nrm[k][0] > 0.1 && m.nrm[k][1] > 0.8, "leans down the valley: {:?}", m.nrm[k]);
    }
}
