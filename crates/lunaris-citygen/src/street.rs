//! Streets: asphalt, kerbs, paved sidewalks, lane paint and zebra crossings, laid at their real sizes on the real photographed materials.

use crate::materials::Lib;
use crate::mesh::*;
use crate::rng::Rng;

/// Heights and widths of a street, in metres.
#[derive(Clone, Copy, Debug)]
pub struct Section {
    /// The carriageway (two lanes and a parking lane each side).
    pub carriageway: f32,
    pub sidewalk: f32,
    pub kerb_h: f32,
}

impl Default for Section {
    fn default() -> Section {
        Section { carriageway: 11.0, sidewalk: 3.6, kerb_h: 0.15 }
    }
}

impl Section {
    pub fn total(&self) -> f32 {
        self.carriageway + 2.0 * self.sidewalk
    }
}

pub struct Ground {
    pub asphalt: usize,
    pub sidewalk: usize,
    pub kerb: usize,
    pub court: usize,
    pub white: usize,
    pub yellow: usize,
    /// A park's lawn and the paths over it.
    pub lawn: usize,
    pub path: usize,
}

impl Ground {
    pub fn new(lib: &Lib, rng: &mut Rng) -> Ground {
        let pick = |rng: &mut Rng, names: &[&str]| {
            let present: Vec<&str> = names.iter().copied().filter(|n| lib.has(n)).collect();
            if present.is_empty() { lib.id("equip_paint") } else { lib.id(rng.pick(&present)) }
        };
        Ground {
            asphalt: pick(rng, &["asphalt_04", "clean_asphalt"]),
            sidewalk: pick(rng, &["concrete_pavement_02", "concrete_tiles_02"]),
            kerb: pick(rng, &["concrete_wall_007", "rough_concrete"]),
            court: pick(rng, &["concrete_pavers", "pavement_03"]),
            white: lib.id("paint_white"),
            yellow: lib.id("paint_yellow"),
            lawn: pick(rng, &["grass_ground", "leafy_grass"]),
            path: pick(rng, &["park_dirt", "stony_dirt_path"]),
        }
    }
}

/// A horizontal rectangle on the plane `y`, laid with the material's picture at its real size and the world's x and z as metres.
#[allow(clippy::too_many_arguments)]
pub fn flat(mb: &mut MeshBuilder, lib: &Lib, mat: usize, x0: f32, x1: f32, z0: f32, z1: f32, y: f32, col: V3) {
    if x1 - x0 < 1e-4 || z1 - z0 < 1e-4 {
        return;
    }
    let tex = lib.tex_plain(mat);
    mb.quad_tex(mat, [[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]], [[x0, z0], [x1, z0], [x1, z1], [x0, z1]], &tex, col, [0.0, 1.0, 0.0]);
}

/// A horizontal convex quadrilateral on the plane `y` (the corners as x, z in order round it), laid with the material's picture at its real size and the world's x and z as metres.
pub fn flat_quad(mb: &mut MeshBuilder, lib: &Lib, mat: usize, p: [[f32; 2]; 4], y: f32, col: V3) {
    let tex = lib.tex_plain(mat);
    mb.quad_tex(mat, p.map(|q| [q[0], y, q[1]]), p, &tex, col, [0.0, 1.0, 0.0]);
}

/// Paint on the road: a rectangle centred at (cx, cz) with its length along `along` (unit, horizontal) and a width across, a hair above the asphalt.
pub fn paint(mb: &mut MeshBuilder, mat: usize, cx: f32, cz: f32, along: [f32; 2], length: f32, width: f32, y: f32) {
    let across = [-along[1], along[0]];
    let (l, w) = (length * 0.5, width * 0.5);
    let p = |a: f32, b: f32| [cx + along[0] * a + across[0] * b, y, cz + along[1] * a + across[1] * b];
    mb.quad(mat, [p(-l, -w), p(l, -w), p(l, w), p(-l, w)], [[0.0; 2]; 4], [[1.0; 3]; 4], [0.0, 1.0, 0.0]);
}

/// A vertical face of a kerb along x or z.
#[allow(clippy::too_many_arguments)]
pub(crate) fn kerb_face(mb: &mut MeshBuilder, lib: &Lib, mat: usize, a: [f32; 2], b: [f32; 2], y0: f32, y1: f32, out: V3) {
    let tex = lib.tex_plain(mat);
    let along = (b[0] - a[0]).abs().max((b[1] - a[1]).abs());
    mb.quad_tex(mat, [[a[0], y0, a[1]], [b[0], y0, b[1]], [b[0], y1, b[1]], [a[0], y1, a[1]]], [[0.0, y0], [along, y0], [along, y1], [0.0, y1]], &tex, [0.8; 3], out);
}

/// The sidewalk round a block (x0..x1, z0..z1 is the block itself): four strips at kerb height, a kerb along their outer edge. Corners are square.
#[allow(clippy::too_many_arguments)]
pub fn sidewalk_ring(mb: &mut MeshBuilder, lib: &Lib, g: &Ground, sec: &Section, x0: f32, x1: f32, z0: f32, z1: f32) {
    let (sw, kh) = (sec.sidewalk, sec.kerb_h);
    let (ox0, ox1, oz0, oz1) = (x0 - sw, x1 + sw, z0 - sw, z1 + sw);
    let c = [0.92; 3];
    // the paving: north, south, west, east strips
    flat(mb, lib, g.sidewalk, ox0, ox1, oz0, z0, kh, c);
    flat(mb, lib, g.sidewalk, ox0, ox1, z1, oz1, kh, c);
    flat(mb, lib, g.sidewalk, ox0, x0, z0, z1, kh, c);
    flat(mb, lib, g.sidewalk, x1, ox1, z0, z1, kh, c);
    // the kerb stone along the outer edge: a strip on top, a face down to the asphalt
    let kw = 0.18;
    flat(mb, lib, g.kerb, ox0, ox1, oz0, oz0 + kw, kh + 0.002, [0.95; 3]);
    flat(mb, lib, g.kerb, ox0, ox1, oz1 - kw, oz1, kh + 0.002, [0.95; 3]);
    flat(mb, lib, g.kerb, ox0, ox0 + kw, oz0 + kw, oz1 - kw, kh + 0.002, [0.95; 3]);
    flat(mb, lib, g.kerb, ox1 - kw, ox1, oz0 + kw, oz1 - kw, kh + 0.002, [0.95; 3]);
    kerb_face(mb, lib, g.kerb, [ox0, oz0], [ox1, oz0], 0.0, kh, [0.0, 0.0, -1.0]);
    kerb_face(mb, lib, g.kerb, [ox0, oz1], [ox1, oz1], 0.0, kh, [0.0, 0.0, 1.0]);
    kerb_face(mb, lib, g.kerb, [ox0, oz0], [ox0, oz1], 0.0, kh, [-1.0, 0.0, 0.0]);
    kerb_face(mb, lib, g.kerb, [ox1, oz0], [ox1, oz1], 0.0, kh, [1.0, 0.0, 0.0]);
}

/// Zebra stripes across a carriageway: bars along `along` (the direction of travel), spread across it, centred at (cx, cz).
pub fn zebra(mb: &mut MeshBuilder, g: &Ground, cx: f32, cz: f32, along: [f32; 2], span: f32, y: f32) {
    let across = [-along[1], along[0]];
    let n = (span / 1.0) as i32;
    for i in 0..n {
        let b = (i as f32 + 0.5) * (span / n as f32) - span * 0.5;
        paint(mb, g.white, cx + across[0] * b, cz + across[1] * b, along, 3.2, 0.5, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sidewalk_ring_is_made_of_flat_strips_and_kerb_faces_that_face_the_road() {
        let lib = Lib::from_json("");
        let mut rng = Rng::new(2);
        let g = Ground::new(&lib, &mut rng);
        let mut mb = MeshBuilder::new();
        sidewalk_ring(&mut mb, &lib, &g, &Section::default(), 0.0, 60.0, 0.0, 40.0);
        let (lo, hi) = mb.bounds().unwrap();
        assert!((lo[0] + 3.6).abs() < 1e-4 && (hi[0] - 63.6).abs() < 1e-4 && lo[1] == 0.0 && (hi[1] - 0.152).abs() < 1e-4);
        // the kerb faces point away from the block
        let part = &mb.parts[g.kerb];
        let mut away = 0;
        for q in 0..part.idx.len() / 6 {
            let i = part.idx[q * 6] as usize;
            let (p, n) = (part.pos[i], part.nrm[i]);
            if n[1].abs() < 0.5 && ((p[0] - 30.0) * n[0] + (p[2] - 20.0) * n[2]) > 0.0 {
                away += 1;
            }
        }
        assert_eq!(away, 4);
    }
}
