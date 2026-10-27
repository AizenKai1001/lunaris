//! The round city of a game that lives under a dome (`Plan::polar`; the game's docs/42-the-dome-city.md): the Core in the middle, a paved plaza round it, ring roads at 200 to 750 m and eight avenues that run out to the wall, the blocks between two rings and two
//! radial streets (blocks are quadrilaterals), lots along the sides of a block, the streets laid between them. A block is named (band, sector): `i` is the band (0 is the innermost; band 5 is the apron between the boulevard and the wall, which has no buildings), `j`
//! the sector; (-1, 0) is the plaza of the Core. Everything is a function of the seed and the block's own two numbers, like the rectilinear plan.
//!
//! A tile is the quadrilateral between the centre lines of the four streets round a block; the block is the tile less half a street on each side. The streets are laid by the tiles that own them: the ring road on a tile's inner side (from its first corner to its second) and the
//! radial street on its first side, with the crossing at the first corner, exactly as the rectilinear ground does.

use crate::building::{Kind, Lot, Side};
use crate::materials::Lib;
use crate::mesh::*;
use crate::plan::{hash01, LandUse, LotPlan, Plan};
use crate::rng::Rng;
use crate::street::{self, Ground, Section};
use crate::streetscape::{self, Spot, Thing};
use std::f32::consts::TAU;

pub type P2 = [f32; 2];

fn add2(a: P2, b: P2) -> P2 {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub2(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}
fn mul2(a: P2, s: f32) -> P2 {
    [a[0] * s, a[1] * s]
}
fn dot2(a: P2, b: P2) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross2(a: P2, b: P2) -> f32 {
    a[0] * b[1] - a[1] * b[0]
}
fn len2(a: P2) -> f32 {
    dot2(a, a).sqrt()
}
fn norm2(a: P2) -> P2 {
    let l = len2(a).max(1e-9);
    [a[0] / l, a[1] / l]
}
fn perp2(a: P2) -> P2 {
    [-a[1], a[0]]
}
fn pt(r: f32, th: f32) -> P2 {
    [r * th.cos(), r * th.sin()]
}
fn centroid(p: &[P2; 4]) -> P2 {
    [(p[0][0] + p[1][0] + p[2][0] + p[3][0]) * 0.25, (p[0][1] + p[1][1] + p[2][1] + p[3][1]) * 0.25]
}
fn v3(p: P2, y: f32) -> V3 {
    [p[0], y, p[1]]
}

/// How much wider (on each side) an avenue's carriageway, the Core's ring road and the boulevard along the wall are than an ordinary street's.
pub const EXTRA: f32 = 3.5;
/// The bands of buildings (the apron beyond them is band `BANDS`).
pub const BANDS: i32 = 5;
pub const AVENUES: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Polar {
    /// The centre lines of the ring roads, from the road round the Core out to the boulevard along the wall and, last, the radius where the ground ends under the wall: band b lies between `rings[b]` and `rings[b + 1]`.
    pub rings: [f32; 7],
    /// How many blocks each band has: a multiple of eight, so that the eight avenues run out between blocks.
    pub counts: [u32; 6],
    /// Blocks given over to rubble (a destroyed district: `Plan::with_ruins`): `((band + 1) << 8) | sector`, 0 for a place not used.
    pub ruins: [u16; 24],
}

impl Default for Polar {
    fn default() -> Polar {
        Polar { rings: [200.0, 310.0, 420.0, 530.0, 640.0, 750.0, 802.0], counts: [16, 24, 32, 40, 48, 48], ruins: [0; 24] }
    }
}

/// The lines `p + t d` (a point and a direction each) meet at.
fn meet(p0: P2, d0: P2, p1: P2, d1: P2) -> P2 {
    let den = cross2(d0, d1);
    let t = if den.abs() < 1e-6 { 0.0 } else { cross2(sub2(p1, p0), d1) / den };
    add2(p0, mul2(d0, t))
}

/// The quadrilateral `p` (its corners in order round it) with every side moved `e[k]` metres inward (outward when negative).
pub fn inset(p: [P2; 4], e: [f32; 4]) -> [P2; 4] {
    let c = centroid(&p);
    let mut line = [([0.0f32; 2], [0.0f32; 2]); 4];
    for k in 0..4 {
        let (a, b) = (p[k], p[(k + 1) % 4]);
        let d = norm2(sub2(b, a));
        let mut n = perp2(d);
        if dot2(n, sub2(c, a)) < 0.0 {
            n = [-n[0], -n[1]];
        }
        line[k] = (add2(a, mul2(n, e[k])), d);
    }
    let mut out = [[0.0f32; 2]; 4];
    for k in 0..4 {
        let (l0, l1) = (line[(k + 3) % 4], line[k]);
        out[k] = meet(l0.0, l0.1, l1.0, l1.1);
    }
    out
}

/// The inward unit normals of the sides of a convex polygon (side k runs from corner k to corner k + 1).
fn normals(p: &[P2; 4]) -> [P2; 4] {
    let c = centroid(p);
    let mut out = [[0.0f32; 2]; 4];
    for k in 0..4 {
        let (a, b) = (p[k], p[(k + 1) % 4]);
        let mut n = perp2(norm2(sub2(b, a)));
        if dot2(n, sub2(c, a)) < 0.0 {
            n = [-n[0], -n[1]];
        }
        out[k] = n;
    }
    out
}

/// Whether two convex polygons overlap by more than `margin` metres.
fn overlap(a: &[P2; 4], b: &[P2; 4], margin: f32) -> bool {
    for poly in [a, b] {
        for k in 0..4 {
            let axis = perp2(norm2(sub2(poly[(k + 1) % 4], poly[k])));
            let proj = |q: &[P2; 4]| {
                let d: Vec<f32> = q.iter().map(|p| dot2(*p, axis)).collect();
                (d.iter().cloned().fold(f32::MAX, f32::min), d.iter().cloned().fold(f32::MIN, f32::max))
            };
            let (a0, a1) = proj(a);
            let (b0, b1) = proj(b);
            if a1.min(b1) - a0.max(b0) <= margin {
                return false;
            }
        }
    }
    true
}

impl Polar {
    /// The number a ruin block is kept under in `ruins`.
    pub fn ruin_code(b: i32, s: i32) -> u16 {
        (((b + 1) as u16) << 8) | (s as u16 & 0xFF)
    }

    /// Whether block (b, s) is a ruin: an empty block of rubble where buildings stood.
    pub fn is_ruin(&self, b: i32, s: i32) -> bool {
        (0..BANDS).contains(&b) && (0..256).contains(&s) && self.ruins.contains(&Polar::ruin_code(b, s))
    }

    pub fn valid(&self, b: i32, s: i32) -> bool {
        (b == -1 && s == 0) || ((0..=BANDS).contains(&b) && s >= 0 && (s as u32) < self.counts[b as usize])
    }

    fn n(&self, b: i32) -> u32 {
        self.counts[b.clamp(0, BANDS) as usize]
    }

    /// The corners of the tile of block (b, s), in order round it: the inner ring's two ends, then the outer ring's, at the centre lines of the streets.
    pub fn tile(&self, b: i32, s: i32) -> [P2; 4] {
        let n = self.n(b) as f32;
        let (ra, rb) = (self.rings[b as usize], self.rings[b as usize + 1]);
        let (t0, t1) = (TAU * s as f32 / n, TAU * (s + 1) as f32 / n);
        [pt(ra, t0), pt(ra, t1), pt(rb, t1), pt(rb, t0)]
    }

    /// Whether the radial street on the first side of block (b, s) is one of the eight avenues.
    pub fn avenue(&self, b: i32, s: i32) -> bool {
        let n = self.n(b) as i32;
        s.rem_euclid(n) % (n / AVENUES as i32) == 0
    }

    /// How much wider than an ordinary street the ring road `k` is (on each side).
    pub fn ring_extra(&self, k: i32) -> f32 {
        if k == 0 || k == BANDS { EXTRA } else { 0.0 }
    }

    /// The tile's middle (the middle of its middle radius), which says what streaming cell a block is in.
    fn tile_mid(&self, b: i32, s: i32) -> P2 {
        if b < 0 {
            return [0.0, 0.0];
        }
        let n = self.n(b) as f32;
        pt((self.rings[b as usize] + self.rings[b as usize + 1]) * 0.5, TAU * (s as f32 + 0.5) / n)
    }
}

/// How far from a corner of angle `alpha` a strip of lots of the given depth has to be shortened that its end does not stick out through the next side (an acute corner only).
fn acute(alpha: f32, depth: f32) -> f32 {
    if alpha < std::f32::consts::FRAC_PI_2 - 0.01 { depth / alpha.tan() + 0.2 } else { 0.0 }
}

/// The interior angle of polygon `q` at corner `k`.
fn angle_at(q: &[P2; 4], k: usize) -> f32 {
    let (a, b, c) = (q[(k + 3) % 4], q[k], q[(k + 1) % 4]);
    let (u, w) = (norm2(sub2(a, b)), norm2(sub2(c, b)));
    dot2(u, w).clamp(-1.0, 1.0).acos()
}

/// A kind of building, its depth and its width for the next lot of a strip with `left` metres still to cover (the same choice as the rectilinear plan's).
fn pick_lot(rng: &mut Rng, mix: &[f32; 7], depth: f32, left: f32) -> (Kind, f32, f32) {
    let mut kind = [Kind::Tower, Kind::Masonry, Kind::Apartment, Kind::Shophouse, Kind::House, Kind::Warehouse, Kind::Skyscraper][rng.weighted(mix)];
    let d = depth * rng.range(0.85, 1.0);
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
    if left - w < 9.0 {
        w = left;
    }
    if kind == Kind::Tower && w < 22.0 {
        kind = Kind::Masonry;
    }
    if kind == Kind::Skyscraper && (w < 24.0 || d < 22.0) {
        kind = if w >= 22.0 { Kind::Tower } else { Kind::Masonry };
    }
    // the last lot of a strip is what is left of it: a house on a narrower one has no room for its floor plan
    if kind == Kind::House && w < crate::plan::HOUSE_MIN_WIDTH {
        kind = Kind::Shophouse;
    }
    (kind, d, w)
}

impl Plan {
    fn pol(&self) -> Polar {
        self.polar.expect("a polar plan")
    }

    /// The centre-line quadrilateral of block (i, j) (for the plaza of the Core, the square that holds it).
    pub fn tile_poly(&self, i: i32, j: i32) -> [P2; 4] {
        let pol = self.pol();
        if i < 0 {
            let r = pol.rings[0];
            return [[-r, -r], [r, -r], [r, r], [-r, r]];
        }
        pol.tile(i, j)
    }

    /// The block itself: its tile less half a street along each side (an avenue is wider, and so are the Core's ring road and the boulevard).
    pub fn block_poly(&self, i: i32, j: i32) -> [P2; 4] {
        let pol = self.pol();
        if i < 0 {
            return self.tile_poly(i, j);
        }
        let half = self.section.total() * 0.5;
        let n = pol.n(i) as i32;
        let e = [
            half + pol.ring_extra(i),
            half + if pol.avenue(i, (j + 1) % n) { EXTRA } else { 0.0 },
            half + pol.ring_extra(i + 1),
            half + if pol.avenue(i, j) { EXTRA } else { 0.0 },
        ];
        inset(pol.tile(i, j), if i >= BANDS { [0.0; 4] } else { e })
    }

    /// The middle of the block.
    pub fn block_mid(&self, i: i32, j: i32) -> P2 {
        if i < 0 {
            return [0.0, 0.0];
        }
        centroid(&self.block_poly(i, j))
    }

    /// The block's own frame: its middle, where the local x axis (along the arc) and the local z axis (outward along the radius) point in the world. A turn: nothing in it is mirrored.
    pub fn block_frame(&self, i: i32, j: i32) -> (P2, P2, P2) {
        let c = self.block_mid(i, j);
        let ur = if len2(c) < 1e-3 { [0.0, 1.0] } else { norm2(c) };
        (c, [ur[1], -ur[0]], ur)
    }

    /// Half the width and half the depth of the largest rectangle in the block's own frame, round its middle, that lies inside the block with a margin: where a park's lawn, paths and pitches go.
    pub fn local_rect(&self, i: i32, j: i32) -> (f32, f32) {
        let q = self.block_poly(i, j);
        let (c, ux, uz) = self.block_frame(i, j);
        let nrm = normals(&q);
        let (mut best, mut hw0, mut hd0) = (0.0f32, 8.0f32, 8.0f32);
        let mut hw = 6.0;
        while hw < 80.0 {
            let mut hd = 80.0f32;
            for k in 0..4 {
                let slack = dot2(sub2(c, q[k]), nrm[k]) - 3.0;
                let (a, b) = (dot2(nrm[k], ux).abs(), dot2(nrm[k], uz).abs());
                if b > 1e-3 {
                    hd = hd.min((slack - hw * a) / b);
                } else if hw * a > slack {
                    hd = 0.0;
                }
            }
            if hd >= 6.0 && hw * hd > best {
                (best, hw0, hd0) = (hw * hd, hw, hd);
            }
            hw += 1.0;
        }
        (hw0, hd0)
    }

    /// The blocks whose middle is in the square cell (ci, cj) of the given size (a streaming grid's cell).
    pub fn blocks_in_cell(&self, cell: f32, ci: i32, cj: i32) -> Vec<(i32, i32)> {
        let Some(pol) = self.polar else { return vec![(ci, cj)] };
        let (x0, z0) = (ci as f32 * cell, cj as f32 * cell);
        let mut out = Vec::new();
        let (cx, cz) = (x0 + cell * 0.5, z0 + cell * 0.5);
        if cx.hypot(cz) > pol.rings[6] + cell * 0.71 + 120.0 {
            return out;
        }
        let inside = |m: P2| m[0] >= x0 && m[0] < x0 + cell && m[1] >= z0 && m[1] < z0 + cell;
        if inside([0.0, 0.0]) {
            out.push((-1, 0));
        }
        for b in 0..=BANDS {
            for s in 0..pol.counts[b as usize] as i32 {
                if inside(pol.tile_mid(b, s)) {
                    out.push((b, s));
                }
            }
        }
        out
    }

    /// What block (i, j) is: the plaza of the Core, five bands from a downtown of towers out to houses, a factory district, parks, and the apron by the wall.
    pub(crate) fn polar_landuse(&self, b: i32, s: i32) -> LandUse {
        let pol = self.pol();
        if b < 0 {
            return LandUse::Park;
        }
        if b >= BANDS {
            return LandUse::Rim;
        }
        if pol.is_ruin(b, s) {
            return LandUse::Park;
        }
        let n = pol.counts[b as usize];
        let turn = (s as f32 + 0.5) / n as f32;
        let (h, h2) = (hash01(self.seed, b as i64, s as i64, 31), hash01(self.seed, b as i64, s as i64, 32));
        if b >= 1 && h < 0.06 {
            return LandUse::Park;
        }
        // an arc of the outer city is the factory district
        if (b == 3 || b == 4) && (0.55..0.80).contains(&turn) {
            return LandUse::Industrial;
        }
        match b {
            0 => LandUse::Downtown,
            1 => if h2 < 0.6 { LandUse::Downtown } else { LandUse::Midrise },
            2 => if h2 < 0.15 { LandUse::Downtown } else { LandUse::Midrise },
            3 => if h2 < 0.45 { LandUse::Midrise } else { LandUse::Residential },
            _ => if h2 < 0.65 { LandUse::Residential } else { LandUse::Outskirts },
        }
    }

    /// The lots of a polar block: a strip along each of its four sides (the two along the rings deep, the two along the radii shallower), each lot a rectangle that lies inside the block and clear of the others.
    pub(crate) fn polar_lots(&self, i: i32, j: i32) -> Vec<LotPlan> {
        let use_ = self.landuse(i, j);
        if matches!(use_, LandUse::Park | LandUse::Rim) {
            return Vec::new();
        }
        let q = self.block_poly(i, j);
        let nrm = normals(&q);
        let c = centroid(&q);
        let y = self.section.kerb_h;
        let mix = self.mix(use_);
        let mut rng = Rng::keyed(&[self.seed, i as u64, j as u64, 1]);
        // the blocks round the Core are given to the big ones: their lots are deeper
        let core = use_ == LandUse::Downtown && i <= 1 && hash01(self.seed, i as i64, j as i64, 51) < 0.55;
        let (dn, ds) = if core { (rng.range(27.0, 34.0), rng.range(27.0, 34.0)) } else { (rng.range(19.0, 26.0), rng.range(19.0, 26.0)) };
        let (dw, de) = (rng.range(16.0, 22.0), rng.range(16.0, 22.0));
        let al: Vec<f32> = (0..4).map(|k| angle_at(&q, k)).collect();
        // (from corner, to corner, depth, whether the end lots show a street, how far the strip is kept clear at its start and at its end)
        let strips = [
            (0usize, 1usize, dn, true, acute(al[0], dn), acute(al[1], dn)),
            (2, 3, ds, true, acute(al[2], ds), acute(al[3], ds)),
            (0, 3, dw, false, dn / al[0].sin().max(0.3) + 0.2, ds / al[3].sin().max(0.3) + 0.2),
            (2, 1, de, false, ds / al[2].sin().max(0.3) + 0.2, dn / al[1].sin().max(0.3) + 0.2),
        ];
        let mut out: Vec<LotPlan> = Vec::new();
        let mut placed: Vec<[P2; 4]> = Vec::new();
        for &(from, to, depth, end_street, start_clear, end_clear) in &strips {
            let (p, qv) = (q[from], q[to]);
            let ex = norm2(sub2(qv, p));
            let mut ez = perp2(ex);
            if dot2(ez, sub2(c, p)) < 0.0 {
                ez = [-ez[0], -ez[1]];
            }
            let length = len2(sub2(qv, p)) - end_clear;
            let mut x = start_clear;
            let mut on_edge = 0;
            while length - x > 5.0 {
                let (kind, d, w) = pick_lot(&mut rng, &mix, depth, length - x);
                let corners = [add2(p, mul2(ex, x)), add2(p, mul2(ex, x + w)), add2(add2(p, mul2(ex, x + w)), mul2(ez, d)), add2(add2(p, mul2(ex, x)), mul2(ez, d))];
                let in_block = corners.iter().all(|k2| (0..4).all(|k| dot2(sub2(*k2, q[k]), nrm[k]) > -0.05));
                if !in_block || placed.iter().any(|r| overlap(r, &corners, 0.3)) {
                    x += 1.5;
                    continue;
                }
                let last = length - x - w < 0.01;
                let left = if on_edge == 0 && end_street { Side::Street } else { Side::Party };
                let right = if last && end_street { Side::Street } else { Side::Party };
                let index = out.len() as u32;
                let key = Rng::keyed(&[self.seed, i as u64, j as u64, index as u64, 0x4C4F54]).u64();
                out.push(LotPlan { lot: Lot { o: v3(add2(p, mul2(ex, x)), y), ex: v3(ex, 0.0), ez: v3(ez, 0.0), w, d, sides: [Side::Street, right, Side::Open, left] }, kind, key, index });
                placed.push(corners);
                x += w;
                on_edge += 1;
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------------- ground

/// An asphalt (or any) strip along the line a to b, `width` across.
#[allow(clippy::too_many_arguments)]
fn strip_flat(mb: &mut MeshBuilder, lib: &Lib, mat: usize, a: P2, b: P2, width: f32, y: f32, col: V3) {
    let d = norm2(sub2(b, a));
    let n = mul2(perp2(d), width * 0.5);
    street::flat_quad(mb, lib, mat, [sub2(a, n), sub2(b, n), add2(b, n), add2(a, n)], y, col);
}

/// A flat disc round `c` (a crossing, a plaza) as a fan.
#[allow(clippy::too_many_arguments)]
fn disc_flat(mb: &mut MeshBuilder, lib: &Lib, mat: usize, c: P2, r: f32, y: f32, n: usize, col: V3) {
    for k in 0..n {
        let (a0, a1) = (TAU * k as f32 / n as f32, TAU * (k + 1) as f32 / n as f32);
        let (p0, p1) = (add2(c, pt(r, a0)), add2(c, pt(r, a1)));
        street::flat_quad(mb, lib, mat, [c, p0, p1, p1], y, col);
    }
}

/// The plaza of the Core: a paved disc, the rings of a darker paving and the lines of the eight avenues in it.
fn core_plaza(mb: &mut MeshBuilder, lib: &Lib, g: &Ground, plan: &Plan) {
    let pol = plan.pol();
    let kh = plan.section.kerb_h;
    let r = pol.rings[0] - plan.section.total() * 0.5 - EXTRA;
    disc_flat(mb, lib, g.court, [0.0, 0.0], r, kh, 48, [0.8; 3]);
    // a ring of dark paving, and the eight ways in
    for k in 0..48 {
        let (a0, a1) = (TAU * k as f32 / 48.0, TAU * (k + 1) as f32 / 48.0);
        for (r0, r1) in [(r * 0.62, r * 0.66), (r * 0.93, r * 0.97)] {
            street::flat_quad(mb, lib, g.asphalt, [pt(r0, a0), pt(r0, a1), pt(r1, a1), pt(r1, a0)], kh + 0.01, [0.9; 3]);
        }
    }
    for k in 0..8 {
        let a = TAU * k as f32 / 8.0;
        strip_flat(mb, lib, g.asphalt, pt(r * 0.2, a), pt(r, a), 3.0, kh + 0.01, [0.9; 3]);
    }
}

/// The sidewalk round a block (`q`): four strips at kerb height, a kerb along their outer edge.
fn sidewalk_ring_poly(mb: &mut MeshBuilder, lib: &Lib, g: &Ground, sec: &Section, q: [P2; 4]) {
    let (sw, kh) = (sec.sidewalk, sec.kerb_h);
    let o = inset(q, [-sw; 4]);
    let kw = 0.18;
    let o2 = inset(o, [kw; 4]);
    let nrm = normals(&o);
    for k in 0..4 {
        let k1 = (k + 1) % 4;
        street::flat_quad(mb, lib, g.sidewalk, [q[k], q[k1], o[k1], o[k]], kh, [0.92; 3]);
        street::flat_quad(mb, lib, g.kerb, [o[k], o[k1], o2[k1], o2[k]], kh + 0.002, [0.95; 3]);
        street::kerb_face(mb, lib, g.kerb, o[k], o[k1], 0.0, kh, [-nrm[k][0], 0.0, -nrm[k][1]]);
    }
}

/// The ground a polar block owns at level 0 (the rectilinear `lod::ground_l0`'s counterpart): the paving round it and its court (a park's lawn and what is on it), the crossing at its first corner and the two street pieces that start there.
pub fn ground_l0(mb: &mut MeshBuilder, lib: &Lib, plan: &Plan, g: &Ground, i: i32, j: i32) {
    let sec = plan.section;
    let pol = plan.pol();
    if i < 0 {
        core_plaza(mb, lib, g, plan);
        return;
    }
    let (kh, py) = (sec.kerb_h, 0.012);
    let t = pol.tile(i, j);
    let q = plan.block_poly(i, j);
    if i < BANDS {
        sidewalk_ring_poly(mb, lib, g, &sec, q);
        if plan.landuse(i, j) == LandUse::Park {
            park_ground(mb, lib, g, plan, i, j);
        } else {
            street::flat_quad(mb, lib, g.court, q, kh, [0.8; 3]);
        }
    } else {
        // the apron between the boulevard and the wall: paving
        street::flat_quad(mb, lib, g.court, t, -0.01, [0.7; 3]);
    }
    let cw_ring = sec.carriageway + 2.0 * pol.ring_extra(i);
    let av = pol.avenue(i, j);
    let cw_rad = sec.carriageway + if av { 2.0 * EXTRA } else { 0.0 };
    let (a, b, d) = (t[0], t[1], t[3]);
    let c = [0.9; 3];
    // the crossing and the two streets (the ring road a good deal wider than the carriageway: where the next band's chord lies a little off this one the asphalt is under the sidewalks)
    disc_flat(mb, lib, g.asphalt, a, cw_ring.max(cw_rad) * 0.5 + 4.0, 0.0, 12, c);
    strip_flat(mb, lib, g.asphalt, a, b, cw_ring + 7.0, 0.0, c);
    if i < BANDS || av {
        strip_flat(mb, lib, g.asphalt, a, d, cw_rad, 0.0, c);
    }
    // paint
    let ring_dir = norm2(sub2(b, a));
    let l = len2(sub2(b, a));
    let mid = add2(a, mul2(ring_dir, l * 0.5));
    let across = perp2(ring_dir);
    let run = (l - 2.0 * (cw_rad * 0.5 + 3.0)).max(2.0);
    for off in [-0.13, 0.13] {
        let p = add2(mid, mul2(across, off));
        street::paint(mb, g.yellow, p[0], p[1], ring_dir, run, 0.1, py);
    }
    for off in [-(cw_ring * 0.5 - 2.3), cw_ring * 0.5 - 2.3] {
        let p = add2(mid, mul2(across, off));
        street::paint(mb, g.white, p[0], p[1], ring_dir, run, 0.12, py);
    }
    for at in [cw_rad * 0.5 + 2.4, l - cw_rad * 0.5 - 2.4] {
        let p = add2(a, mul2(ring_dir, at));
        street::zebra(mb, g, p[0], p[1], ring_dir, cw_ring - 4.0, py);
    }
    if i < BANDS {
        let rad_dir = norm2(sub2(d, a));
        let l = len2(sub2(d, a));
        let mid = add2(a, mul2(rad_dir, l * 0.5));
        let across = perp2(rad_dir);
        let run = (l - 2.0 * (cw_ring * 0.5 + 3.0)).max(2.0);
        for off in [-0.13, 0.13] {
            let p = add2(mid, mul2(across, off));
            street::paint(mb, g.yellow, p[0], p[1], rad_dir, run, 0.1, py);
        }
        for off in [-(cw_rad * 0.5 - 2.3), cw_rad * 0.5 - 2.3] {
            let p = add2(mid, mul2(across, off));
            street::paint(mb, g.white, p[0], p[1], rad_dir, run, 0.12, py);
        }
        for at in [cw_ring * 0.5 + 2.4, l - cw_ring * 0.5 - 2.4] {
            let p = add2(a, mul2(rad_dir, at));
            street::zebra(mb, g, p[0], p[1], rad_dir, cw_rad - 4.0, py);
        }
    }
}

/// The ground of a polar block at level 1: the same pieces as plain strips, a little below.
pub fn ground_l1(mb: &mut MeshBuilder, lib: &Lib, plan: &Plan, g: &Ground, i: i32, j: i32) {
    let sec = plan.section;
    let pol = plan.pol();
    if i < 0 {
        disc_flat(mb, lib, g.court, [0.0, 0.0], pol.rings[0] - sec.total() * 0.5 - EXTRA, sec.kerb_h - 0.12, 32, [0.8; 3]);
        return;
    }
    let (kh, drop) = (sec.kerb_h, 0.12);
    let t = pol.tile(i, j);
    let q = plan.block_poly(i, j);
    if i < BANDS {
        street::flat_quad(mb, lib, g.sidewalk, inset(q, [-sec.sidewalk; 4]), kh - drop, [0.9; 3]);
        if plan.is_ruin(i, j) {
            street::flat_quad(mb, lib, g.path, q, kh - drop + 0.01, [0.7; 3]);
        } else if plan.landuse(i, j) == LandUse::Park {
            street::flat_quad(mb, lib, g.lawn, q, kh - drop + 0.01, streetscape::LAWN_TINT);
        } else {
            street::flat_quad(mb, lib, g.court, q, kh - drop + 0.01, [0.8; 3]);
        }
    } else {
        street::flat_quad(mb, lib, g.court, t, -drop - 0.01, [0.7; 3]);
    }
    let cw_ring = sec.carriageway + 2.0 * pol.ring_extra(i);
    let av = pol.avenue(i, j);
    let cw_rad = sec.carriageway + if av { 2.0 * EXTRA } else { 0.0 };
    let c = [0.9; 3];
    disc_flat(mb, lib, g.asphalt, t[0], cw_ring.max(cw_rad) * 0.5 + 4.0, -drop, 8, c);
    strip_flat(mb, lib, g.asphalt, t[0], t[1], cw_ring + 7.0, -drop, c);
    if i < BANDS || av {
        strip_flat(mb, lib, g.asphalt, t[0], t[3], cw_rad, -drop, c);
    }
}

/// The ground of a far cell at level 2: a dark quad under every tile of the cell, and a green one for every park.
pub fn ground_l2(mb: &mut MeshBuilder, lib: &Lib, plan: &Plan, cell: f32, ci: i32, cj: i32) {
    let far = lib.id("far_flat");
    let a = lib.mats[lib.id("grass_ground")].avg;
    let green = [a[0] * streetscape::LAWN_TINT[0], a[1] * streetscape::LAWN_TINT[1], a[2] * streetscape::LAWN_TINT[2]];
    let dark = [0.05, 0.05, 0.052];
    for (i, j) in plan.blocks_in_cell(cell, ci, cj) {
        if i < 0 {
            let r = plan.pol().rings[0];
            mb.quad(far, [[-r, -0.5, -r], [r, -0.5, -r], [r, -0.5, r], [-r, -0.5, r]], [[0.0; 2]; 4], [dark; 4], [0.0, 1.0, 0.0]);
            continue;
        }
        let t = plan.tile_poly(i, j);
        mb.quad(far, t.map(|p| v3(p, -0.5)), [[0.0; 2]; 4], [dark; 4], [0.0, 1.0, 0.0]);
        if plan.landuse(i, j) == LandUse::Park {
            let q = plan.block_poly(i, j);
            let ground = if plan.is_ruin(i, j) { [0.30, 0.26, 0.21] } else { green };
            mb.quad(far, q.map(|p| v3(p, 0.1)), [[0.0; 2]; 4], [ground; 4], [0.0, 1.0, 0.0]);
        }
    }
}

/// The ground of a park on a polar block: a lawn over the whole block, and in the rectangle of its own frame that the block leaves, whatever the park has (paths, paving, a pond, pitches, a playground).
fn park_ground(mb: &mut MeshBuilder, lib: &Lib, g: &Ground, plan: &Plan, i: i32, j: i32) {
    let kh = plan.section.kerb_h;
    let q = plan.block_poly(i, j);
    if plan.is_ruin(i, j) {
        street::flat_quad(mb, lib, g.path, q, kh, [0.7; 3]);
    } else {
        street::flat_quad(mb, lib, g.lawn, q, kh, streetscape::LAWN_TINT);
    }
    let (c, ux, uz) = plan.block_frame(i, j);
    let (hw, hd) = plan.local_rect(i, j);
    let rect = (-hw, hw, -hd, hd);
    let lay = streetscape::park_layout_rect(plan, i, j, rect);
    let mut local = MeshBuilder::new();
    streetscape::park_ground_rect(&mut local, lib, g, kh, &lay, rect, false);
    local.turn_and_move(ux, uz, v3(c, 0.0));
    mb.append(&local);
}

// ---------------------------------------------------------------------------------------------------- things in the streets

/// The things of a polar block: along the four sidewalks for a block of buildings, in the rectangle of its own frame for a park, cars in the two street pieces the block owns.
pub fn spots(plan: &Plan, i: i32, j: i32) -> Vec<Spot> {
    let pol = plan.pol();
    if i < 0 || i >= BANDS {
        return Vec::new();
    }
    let mut rng = Rng::keyed(&[plan.seed, i as u64, j as u64, 0x5352_4545]);
    let q = plan.block_poly(i, j);
    let nrm = normals(&q);
    let sw = plan.section.sidewalk;
    let y = plan.section.kerb_h;
    let park = plan.landuse(i, j) == LandUse::Park;
    let mut out: Vec<Spot> = Vec::new();
    // the four sidewalks: where one starts (on the outer edge of the paving), which way it runs, which way is into the block, and how long it is
    let sides: Vec<(P2, P2, P2, f32)> = [(0usize, 1usize), (2, 3), (0, 3), (2, 1)]
        .iter()
        .map(|&(f, t)| {
            let (a, b) = (q[f], q[t]);
            let k = if (f, t) == (0, 1) { 0 } else if (f, t) == (2, 3) { 2 } else if (f, t) == (0, 3) { 3 } else { 1 };
            (sub2(a, mul2(nrm[k], sw)), norm2(sub2(b, a)), nrm[k], len2(sub2(b, a)))
        })
        .collect();
    for &(p0, t, n, len) in &sides {
        let at = |s: f32, off: f32| [p0[0] + t[0] * s + n[0] * off, y, p0[1] + t[1] * s + n[1] * off];
        let mut s = rng.range(5.0, 9.0);
        while s < len - 5.0 {
            let (slug, scale) = streetscape::pick_tree(&mut rng, false);
            if !park || rng.chance(0.55) {
                out.push(Spot { thing: Thing::Tree, slug, pos: at(s, rng.range(0.95, 1.2)), yaw: rng.range(0.0, 360.0), scale });
            }
            let lamp_s = s + rng.range(5.5, 7.5);
            if lamp_s < len - 4.0 && rng.chance(0.85) {
                out.push(Spot { thing: Thing::Lamp, slug: "street_lamp_01", pos: at(lamp_s, 0.55), yaw: streetscape::yaw_along(-n[0], -n[1]), scale: 1.0 });
            }
            s += rng.range(12.0, 15.5);
        }
        if rng.chance(0.4) {
            out.push(Spot { thing: Thing::Hydrant, slug: "fire_hydrant", pos: at(rng.range(8.0, len - 8.0), 0.7), yaw: rng.range(0.0, 360.0), scale: 1.0 });
        }
        let clear = |out: &[Spot], p: V3, r: f32| out.iter().all(|o| (o.pos[0] - p[0]).powi(2) + (o.pos[2] - p[2]).powi(2) > r * r);
        if rng.chance(0.35) {
            let p = at(rng.range(10.0, len - 10.0), 1.25);
            if clear(&out, p, 4.2) {
                out.push(Spot { thing: Thing::Bench, slug: "modular_street_seating", pos: p, yaw: streetscape::yaw_along(t[0], t[1]), scale: 1.0 });
            }
        }
        if rng.chance(0.2) {
            let slug = if rng.chance(0.5) { "utility_box_01" } else { "utility_box_02" };
            let p = at(rng.range(8.0, len - 8.0), 1.0);
            if clear(&out, p, 2.5) {
                out.push(Spot { thing: Thing::Cabinet, slug, pos: p, yaw: streetscape::yaw_along(t[0], t[1]), scale: 1.0 });
            }
        }
    }
    // cars left in the parking lane of the two street pieces this block owns, clear of the crossings at their ends
    {
        let t = pol.tile(i, j);
        let cw_ring = plan.section.carriageway + 2.0 * pol.ring_extra(i);
        let cw_rad = plan.section.carriageway + if pol.avenue(i, j) { 2.0 * EXTRA } else { 0.0 };
        let mut crng = Rng::keyed(&[plan.seed, i as u64, j as u64, 0x434152]);
        for (a, b, half) in [(t[0], t[1], cw_ring * 0.5), (t[0], t[3], cw_rad * 0.5)] {
            let dir = norm2(sub2(b, a));
            let across = perp2(dir);
            let l = len2(sub2(b, a));
            let mut s = 14.0 + crng.range(14.0, 30.0);
            while s < l - 14.0 {
                if crng.chance(0.55) {
                    let side = if crng.chance(0.5) { 1.0 } else { -1.0 };
                    let p = add2(add2(a, mul2(dir, s)), mul2(across, side * (half - 1.3)));
                    out.push(Spot { thing: Thing::Car, slug: streetscape::derelict_car(&mut crng), pos: [p[0], 0.0, p[1]], yaw: streetscape::yaw_along(dir[0], dir[1]) + crng.range(-3.0, 3.0), scale: 1.0 });
                }
                s += crng.range(14.0, 42.0);
            }
        }
    }
    // rubbish bags by the walls now and then
    for (k, &(p0, t, n, len)) in sides.iter().enumerate() {
        let mut r = Rng::keyed(&[plan.seed, i as u64, j as u64, 0x4C49545445, k as u64]);
        for _ in 0..r.int(0, 3) {
            let (s, off) = (r.range(6.0, (len - 6.0).max(6.5)), r.range(sw - 0.7, sw - 0.3));
            out.push(Spot { thing: Thing::Litter, slug: "trashbag", pos: [p0[0] + t[0] * s + n[0] * off, y, p0[1] + t[1] * s + n[1] * off], yaw: r.range(0.0, 360.0), scale: r.range(0.9, 1.2) });
        }
    }
    // planters and boards standing along the walls
    for (k, &(p0, t, n, len)) in sides.iter().enumerate() {
        let mut r = Rng::keyed(&[plan.seed, i as u64, j as u64, 0x504C41, k as u64]);
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
                out.push(Spot { thing, slug, pos, yaw: streetscape::yaw_along(t[0], t[1]) + r.range(-15.0, 15.0), scale });
            }
            s += r.range(8.0, 18.0);
        }
    }
    // the yards of houses
    for lp in plan.lots(i, j) {
        if lp.kind != Kind::House {
            continue;
        }
        let lot = &lp.lot;
        let mut r = Rng::keyed(&[plan.seed, i as u64, j as u64, lp.index as u64, 0x59415244]);
        let left = r.chance(0.5);
        let (slug, scale) = streetscape::pick_tree(&mut r, false);
        let x = if left { 1.4 } else { lot.w - 1.4 };
        let p = lot.at(x, r.range(2.0, 3.0));
        out.push(Spot { thing: Thing::Tree, slug, pos: [p[0], y, p[2]], yaw: r.range(0.0, 360.0), scale });
        if r.chance(0.5) {
            let (slug, scale) = streetscape::pick_tree(&mut r, false);
            let p = lot.at(r.range(2.0, lot.w - 2.0), lot.d - 1.6);
            out.push(Spot { thing: Thing::Tree, slug, pos: [p[0], y, p[2]], yaw: r.range(0.0, 360.0), scale });
        }
    }
    if park && plan.is_ruin(i, j) {
        // a destroyed block: nothing planted or set up along its pavements; the litter, the hydrants and the cars the streets have stay
        out.retain(|s| matches!(s.thing, Thing::Car | Thing::Litter | Thing::Hydrant));
    }
    if park {
        let (c, ux, uz) = plan.block_frame(i, j);
        let (hw, hd) = plan.local_rect(i, j);
        let rect = (-hw, hw, -hd, hd);
        let lay = streetscape::park_layout_rect(plan, i, j, rect);
        let mut local: Vec<Spot> = Vec::new();
        streetscape::park_spots(&mut local, &lay, rect, &mut rng, y);
        let turn = streetscape::yaw_along(ux[0], ux[1]);
        for s in local {
            let w = add2(add2(c, mul2(ux, s.pos[0])), mul2(uz, s.pos[2]));
            out.push(Spot { pos: [w[0], s.pos[1], w[1]], yaw: s.yaw + turn, ..s });
        }
    }
    out
}

/// The name posts at the crossing at a polar block's first corner: two posts on opposite corners, each with the plate of the ring road and the plate of the radial street (the plates are the rectilinear city's: model 100 is the post, 200 + m the plate m).
pub fn sign_posts(plan: &Plan, i: i32, j: i32, origin: V3) -> Vec<lunaris_json::Json> {
    use lunaris_json::Json;
    let pol = plan.pol();
    if i < 0 {
        return Vec::new();
    }
    let n = |x: f32| Json::Float(x as f64);
    let v = |a: [f32; 3]| Json::arr(a.map(n));
    let t = pol.tile(i, j);
    let a = t[0];
    let ur = norm2(a);
    let ut = perp2(ur);
    let cw_ring = plan.section.carriageway + 2.0 * pol.ring_extra(i);
    let cw_rad = plan.section.carriageway + if pol.avenue(i, j) { 2.0 * EXTRA } else { 0.0 };
    let y = plan.section.kerb_h;
    let plate_of = |salt: u64, a: i32, b: i32, big: bool| -> usize {
        let h = Rng::keyed(&[plan.seed, a as u64, b as u64, salt]).u64();
        (h % 64) as usize * 2 + usize::from(big)
    };
    let (m_ring, m_rad) = (plate_of(0x5249_4E47, i, 0, pol.ring_extra(i) > 0.0), plate_of(0x5241_4449, i, j, pol.avenue(i, j)));
    let model = |k: usize| Json::obj([("mesh", Json::str(format!("city/gen_p_{k}.glb"))), ("color", Json::arr([1.0, 1.0, 1.0, 1.0].map(n)))]);
    let quat = |dir: P2| {
        let phi = dir[0].atan2(dir[1]);
        [0.0, (phi * 0.5).sin(), 0.0, (phi * 0.5).cos()]
    };
    let mut out = Vec::new();
    for (sr, st) in [(-1.0f32, -1.0f32), (1.0, 1.0)] {
        let p = add2(add2(a, mul2(ur, sr * (cw_rad * 0.5 + 0.6))), mul2(ut, st * (cw_ring * 0.5 + 0.6)));
        let pos = [p[0] - origin[0], y - origin[1], p[1] - origin[2]];
        let at = |h: f32| v([pos[0], pos[1] + h, pos[2]]);
        out.push(Json::obj([("name", Json::str("Sign post")), ("components", Json::obj([("LocalTransform", Json::obj([("pos", v(pos))])), ("MeshRenderer", model(100))]))]));
        out.push(Json::obj([("name", Json::str("Collider")), ("components", Json::obj([("LocalTransform", Json::obj([("pos", at(1.6))])), ("Collider", Json::obj([("shape", Json::from(2)), ("size", v([0.2, 3.2, 0.2])), ("trigger", Json::Bool(false))]))]))]));
        // the ring road's plate faces along the radius, the radial street's along the arc
        out.push(Json::obj([("name", Json::str("Plate")), ("components", Json::obj([("LocalTransform", Json::obj([("pos", at(3.08)), ("rot", Json::arr(quat(ur).map(n)))])), ("MeshRenderer", model(200 + m_ring))]))]));
        out.push(Json::obj([("name", Json::str("Plate")), ("components", Json::obj([("LocalTransform", Json::obj([("pos", at(2.76)), ("rot", Json::arr(quat(ut).map(n)))])), ("MeshRenderer", model(200 + m_rad))]))]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        Plan::new(5).with_polar(Some(Polar::default()))
    }

    #[test]
    fn a_ruin_is_an_empty_park_of_its_own_kind_with_nothing_planted() {
        let list = [(3, 6), (3, 7), (4, 8)];
        let (p, plain) = (plan().with_ruins(&list), plan());
        for &(b, s) in &list {
            assert!(p.is_ruin(b, s) && p.landuse(b, s) == LandUse::Park, "block {b} {s}");
            assert!(p.lots(b, s).is_empty(), "a ruin has no lot");
            let (hw, hd) = p.local_rect(b, s);
            assert_eq!(streetscape::park_layout_rect(&p, b, s, (-hw, hw, -hd, hd)).kind, streetscape::ParkKind::Ruin);
            let things = spots(&p, b, s);
            assert!(things.iter().all(|t| matches!(t.thing, Thing::Car | Thing::Litter | Thing::Hydrant)), "block {b} {s}: {:?}", things.iter().map(|t| t.thing).collect::<Vec<_>>());
            assert!(!plain.is_ruin(b, s));
        }
        // the others are what they were, and a block that is not in the city is ignored
        for b in 0..BANDS {
            for s in 0..p.pol().counts[b as usize] as i32 {
                if !list.contains(&(b, s)) {
                    assert_eq!(p.landuse(b, s), plain.landuse(b, s), "block {b} {s}");
                }
            }
        }
        let none = plan().with_ruins(&[(9, 9), (3, 99), (-1, 0), (5, 2)]);
        assert!(none.pol().ruins.iter().all(|&r| r == 0), "{:?}", none.pol().ruins);
        // city.json: `"ruins": [[band, sector], ...]`
        let doc = lunaris_json::parse(r#"{"polar": true, "ruins": [[3, 6], [4, 8], [1]]}"#).unwrap();
        assert_eq!(crate::plan::ruins_from_json(&doc), vec![(3, 6), (4, 8)]);
        assert!(crate::plan::ruins_from_json(&lunaris_json::parse("{}").unwrap()).is_empty());
    }

    #[test]
    fn a_polar_block_lies_inside_its_tile_and_the_tiles_cover_the_ring_between_the_rings() {
        let p = plan();
        let pol = p.pol();
        let mut area = 0.0f32;
        for b in 0..=BANDS {
            for s in 0..pol.counts[b as usize] as i32 {
                let t = p.tile_poly(b, s);
                let q = p.block_poly(b, s);
                // a tile is a quadrilateral whose area is half the cross product of its diagonals
                let ar = |q: &[P2; 4]| 0.5 * cross2(sub2(q[2], q[0]), sub2(q[3], q[1])).abs();
                area += ar(&t);
                assert!(ar(&q) < ar(&t) + 1.0 && (b == BANDS || ar(&q) > 0.3 * ar(&t)), "block {b} {s}: {} of {}", ar(&q), ar(&t));
                let nt = normals(&t);
                for c in q {
                    assert!((0..4).all(|k| dot2(sub2(c, t[k]), nt[k]) > -0.1), "block {b} {s} sticks out of its tile");
                }
            }
        }
        // the tiles are polygons inside the rings, so a little less than the annulus between the first and the last
        let annulus = std::f32::consts::PI * (pol.rings[6].powi(2) - pol.rings[0].powi(2));
        assert!(area > 0.97 * annulus && area < annulus, "{area} against {annulus}");
    }

    #[test]
    fn lots_of_a_polar_block_stay_in_the_block_and_clear_of_each_other_and_are_the_same_every_time() {
        let p = plan();
        let pol = p.pol();
        let (mut total, mut blocks) = (0, 0);
        for b in 0..BANDS {
            for s in 0..pol.counts[b as usize] as i32 {
                let lots = p.lots(b, s);
                let again = p.lots(b, s);
                assert_eq!(lots.len(), again.len());
                let q = p.block_poly(b, s);
                let nrm = normals(&q);
                let rects: Vec<[P2; 4]> = lots.iter().map(|l| l.lot.corners().map(|c| [c[0], c[2]])).collect();
                for (k, r) in rects.iter().enumerate() {
                    for c in r {
                        assert!((0..4).all(|e| dot2(sub2(*c, q[e]), nrm[e]) > -0.06), "lot {k} of block {b} {s} sticks out");
                    }
                    for r2 in &rects[..k] {
                        assert!(!overlap(r, r2, 0.31), "two lots of block {b} {s} overlap");
                    }
                }
                if !lots.is_empty() {
                    blocks += 1;
                    total += lots.len();
                }
            }
        }
        assert!(blocks > 100 && total > 8 * blocks, "{total} lots in {blocks} blocks");
    }

    #[test]
    fn every_block_is_in_exactly_one_streaming_cell_and_the_cells_find_it() {
        let p = plan();
        let pol = p.pol();
        let cell = p.pitch();
        let mut seen = std::collections::HashSet::new();
        let reach = (pol.rings[6] / cell) as i32 + 2;
        for cj in -reach..=reach {
            for ci in -reach..=reach {
                for blk in p.blocks_in_cell(cell, ci, cj) {
                    assert!(seen.insert(blk), "block {blk:?} is in two cells");
                }
            }
        }
        let want: usize = 1 + pol.counts.iter().sum::<u32>() as usize;
        assert_eq!(seen.len(), want);
        // the same for the big cells of the far level
        let big = p.cell_size();
        let mut n = 0;
        for cj in -5..=5 {
            for ci in -5..=5 {
                n += p.blocks_in_cell(big, ci, cj).len();
            }
        }
        assert_eq!(n, want);
    }

    #[test]
    fn the_land_has_a_downtown_round_the_core_and_every_kind_of_land_and_the_streets_are_those_of_the_plan() {
        let p = plan();
        let pol = p.pol();
        let mut seen = std::collections::HashMap::new();
        for b in 0..=BANDS {
            for s in 0..pol.counts[b as usize] as i32 {
                *seen.entry(p.landuse(b, s)).or_insert(0) += 1;
            }
        }
        for u in [LandUse::Downtown, LandUse::Midrise, LandUse::Residential, LandUse::Outskirts, LandUse::Industrial, LandUse::Park, LandUse::Rim] {
            assert!(seen.get(&u).copied().unwrap_or(0) > 0, "{u:?}: {seen:?}");
        }
        assert!((0..16).all(|s| p.landuse(0, s) == LandUse::Downtown));
        // eight avenues in every band
        for b in 0..=BANDS {
            assert_eq!((0..pol.counts[b as usize] as i32).filter(|s| pol.avenue(b, *s)).count(), 8);
        }
        assert!(p.inside(-1, 0) && p.inside(3, 7) && !p.inside(3, 40) && !p.inside(7, 0));
    }

    #[test]
    fn a_polar_blocks_ground_and_things_stay_near_its_tile() {
        let lib = Lib::from_json("");
        let p = plan();
        let g = Ground::new(&lib, &mut Rng::new(1));
        let pol = p.pol();
        for (b, s) in [(0, 3), (2, 17), (3, 30), (4, 9), (5, 4), (-1, 0)] {
            let (mut a, mut c) = (MeshBuilder::new(), MeshBuilder::new());
            ground_l0(&mut a, &lib, &p, &g, b, s);
            ground_l1(&mut c, &lib, &p, &g, b, s);
            assert!(a.triangles() > 0 && c.triangles() > 0 && a.triangles() >= c.triangles(), "block {b} {s}");
            let t = p.tile_poly(b, s);
            let (lo, hi) = a.bounds().unwrap();
            let (mn, mx) = (t.iter().map(|q| q[0]).fold(f32::MAX, f32::min), t.iter().map(|q| q[0]).fold(f32::MIN, f32::max));
            let (nz, xz) = (t.iter().map(|q| q[1]).fold(f32::MAX, f32::min), t.iter().map(|q| q[1]).fold(f32::MIN, f32::max));
            assert!(lo[0] > mn - 20.0 && hi[0] < mx + 20.0 && lo[2] > nz - 20.0 && hi[2] < xz + 20.0, "block {b} {s} ground strays: {lo:?} {hi:?} against {mn} {mx} {nz} {xz}");
            if b >= 0 {
                let sp = spots(&p, b, s);
                if b < BANDS {
                    assert!(sp.iter().any(|x| x.thing == Thing::Lamp), "block {b} {s} has no lamp");
                }
                let q = p.block_poly(b, s);
                for x in sp.iter().filter(|x| x.thing != Thing::Car) {
                    assert!(x.scale > 0.0);
                    let c = [x.pos[0], x.pos[2]];
                    assert!(len2(sub2(c, centroid(&q))) < 150.0, "a thing of block {b} {s} is {} m from its middle", len2(sub2(c, centroid(&q))));
                }
            }
        }
        let far = ground_l2_for_test(&lib, &p);
        assert!(far > 0);
        let _ = pol;
    }

    fn ground_l2_for_test(lib: &Lib, p: &Plan) -> usize {
        let mut mb = MeshBuilder::new();
        ground_l2(&mut mb, lib, p, p.cell_size(), 0, 0);
        mb.triangles()
    }
}
