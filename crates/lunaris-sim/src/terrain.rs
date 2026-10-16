//! Terrain: ground made from a heightmap. A `Terrain` entity names a greyscale PNG of the workspace (16 bits is best: Gaea, World Machine
//! and the like write it); black is the lowest ground, white `height` metres above it, the picture covers `size` metres, centred on the
//! entity. The simulation reads the file once ([`Heightfield::read`]) and uses it for collisions (spheres, boxes and characters stand on it,
//! rays hit it); the renderers draw a mesh made from the same numbers.
//!
//! Everything here is f32 arithmetic with `+ - * /` and `sqrt` only, in a fixed order, so every machine finds the same contacts.
//! The ground is made of two triangles per cell with the diagonal always from the cell's (+x, -z) corner to its (-x, +z) corner; the
//! renderers use the same triangles, so what is drawn is what is stood on.
use crate::components::{Terrain, Transform};
use lunaris_ecs::World;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

pub type V3 = [f32; 3];

/// The heightmaps in use, by their path in the workspace.
pub type Terrains = BTreeMap<String, Arc<Heightfield>>;

#[derive(Clone, Debug)]
pub struct Heightfield {
    pub cols: usize,
    pub rows: usize,
    samples: Vec<u16>,
    /// FNV-1a of the numbers: the simulation hashes it, so a replay with a different picture is noticed.
    pub hash: u64,
}

impl Heightfield {
    pub fn new(cols: usize, rows: usize, samples: Vec<u16>) -> Result<Heightfield, String> {
        if cols < 2 || rows < 2 {
            return Err(format!("a heightmap needs at least 2 x 2 samples (this one has {cols} x {rows})"));
        }
        if samples.len() != cols * rows {
            return Err("the number of samples does not match the size".to_string());
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |b: u8| h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        for b in (cols as u64).to_le_bytes().into_iter().chain((rows as u64).to_le_bytes()) {
            feed(b);
        }
        for s in &samples {
            feed(*s as u8);
            feed((*s >> 8) as u8);
        }
        Ok(Heightfield { cols, rows, samples, hash: h })
    }

    pub fn from_png(bytes: &[u8]) -> Result<Heightfield, String> {
        let d = crate::png::decode_png(bytes)?;
        Heightfield::new(d.cols, d.rows, d.samples)
    }

    /// Read a heightmap of the workspace (a relative path without `..`).
    pub fn read(root: &Path, name: &str) -> Result<Heightfield, String> {
        let rel = Path::new(name);
        if name.is_empty() || rel.is_absolute() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(format!("`{name}` is not a path inside the workspace"));
        }
        let path = root.join(rel);
        let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Heightfield::from_png(&bytes).map_err(|e| format!("{name}: {e}"))
    }

    /// The raw samples, row by row (row 0 is the top of the picture, which is -z), 0 to 65535: for a renderer that keeps the picture on the card.
    pub fn samples(&self) -> &[u16] {
        &self.samples
    }

    /// The sample at a column and row, 0 to 1.
    pub fn sample(&self, c: usize, r: usize) -> f32 {
        self.samples[r.min(self.rows - 1) * self.cols + c.min(self.cols - 1)] as f32 * (1.0 / 65535.0)
    }
}

/// A solid terrain of the world with its picture, for code that wants the ground as a list (navigation).
#[derive(Clone)]
pub struct Ground {
    pub uid: u64,
    pub field: Arc<Heightfield>,
    pub pos: V3,
    pub terrain: Terrain,
}

impl Ground {
    pub fn placed(&self) -> Placed<'_> {
        Placed::new(&self.field, self.pos, &self.terrain)
    }
}

/// Every solid terrain whose picture has been read, in uid order.
pub fn grounds(world: &mut World, fields: &Terrains) -> Vec<Ground> {
    if fields.is_empty() {
        return Vec::new();
    }
    let mut raw: Vec<(u64, V3, Terrain)> = Vec::new();
    let _ = world.for_each_uid::<(&Transform, &Terrain), _>(|_, uid, (t, g)| raw.push((uid, t.pos, *g)));
    raw.sort_by_key(|r| r.0);
    raw.into_iter().filter(|r| r.2.solid).filter_map(|(uid, pos, terrain)| fields.get(world.strings().get(terrain.heightmap.0)).map(|f| Ground { uid, field: f.clone(), pos, terrain })).collect()
}

/// One terrain placed in the world: where it is and how its heightmap is used.
#[derive(Clone, Copy)]
pub struct Placed<'a> {
    pub field: &'a Heightfield,
    /// The middle of the ground (x, z).
    pub centre: [f32; 2],
    /// The height of black.
    pub base: f32,
    pub size: [f32; 2],
    pub height: f32,
    /// Cells across and down (every `step`-th sample is a corner).
    pub nx: usize,
    pub nz: usize,
    step: usize,
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l > 1e-12 {
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        [0.0, 1.0, 0.0]
    }
}

/// The point of triangle (a, b, c) that is nearest to p (Ericson, Real-Time Collision Detection).
fn closest_on_triangle(p: V3, a: V3, b: V3, c: V3) -> V3 {
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return [a[0] + ab[0] * v, a[1] + ab[1] * v, a[2] + ab[2] * v];
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return [a[0] + ac[0] * w, a[1] + ac[1] * w, a[2] + ac[2] * w];
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return [b[0] + (c[0] - b[0]) * w, b[1] + (c[1] - b[1]) * w, b[2] + (c[2] - b[2]) * w];
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    [a[0] + ab[0] * v + ac[0] * w, a[1] + ab[1] * v + ac[1] * w, a[2] + ab[2] * v + ac[2] * w]
}

impl<'a> Placed<'a> {
    /// The terrain of an entity at `pos`. The rotation and scale of the entity are not used: the ground lies flat, `size` says how big it is.
    pub fn new(field: &'a Heightfield, pos: V3, t: &Terrain) -> Placed<'a> {
        // at most 2048 cells across, however big the picture (a bigger mesh would not fit in memory)
        let too_big = (field.cols.max(field.rows) - 1).div_ceil(2048).max(1);
        let step = (t.step.max(1) as usize).max(too_big).min(field.cols.max(field.rows));
        let cells = |n: usize| ((n - 1).div_ceil(step)).max(1);
        let size = [if t.size[0].is_finite() { t.size[0].abs().max(0.01) } else { 1.0 }, if t.size[1].is_finite() { t.size[1].abs().max(0.01) } else { 1.0 }];
        Placed { field, centre: [pos[0], pos[2]], base: pos[1], size, height: if t.height.is_finite() { t.height } else { 0.0 }, nx: cells(field.cols), nz: cells(field.rows), step }
    }

    fn cell(&self) -> [f32; 2] {
        [self.size[0] / self.nx as f32, self.size[1] / self.nz as f32]
    }

    fn corner(&self) -> [f32; 2] {
        [self.centre[0] - self.size[0] * 0.5, self.centre[1] - self.size[1] * 0.5]
    }

    /// The ground's lowest point in world y, and its highest.
    pub fn y_range(&self) -> (f32, f32) {
        if self.height >= 0.0 {
            (self.base, self.base + self.height)
        } else {
            (self.base + self.height, self.base)
        }
    }

    /// World position of the corner (i, j), 0 <= i <= nx, 0 <= j <= nz (j counts from the top row of the picture, which is -z).
    pub fn vertex(&self, i: usize, j: usize) -> V3 {
        let (cell, c0) = (self.cell(), self.corner());
        let h = self.field.sample(i * self.step, j * self.step);
        [c0[0] + i as f32 * cell[0], self.base + h * self.height, c0[1] + j as f32 * cell[1]]
    }

    /// The two triangles of cell (i, j), each as three vertices, counter-clockwise seen from above.
    fn triangles(&self, i: usize, j: usize) -> [[V3; 3]; 2] {
        let (p00, p10, p01, p11) = (self.vertex(i, j), self.vertex(i + 1, j), self.vertex(i, j + 1), self.vertex(i + 1, j + 1));
        [[p00, p01, p10], [p10, p01, p11]]
    }

    /// The cell under (x, z) and where in it (0 to 1 each way); `None` outside the ground.
    fn locate(&self, x: f32, z: f32) -> Option<(usize, usize, f32, f32)> {
        let (cell, c0) = (self.cell(), self.corner());
        let (lx, lz) = ((x - c0[0]) / cell[0], (z - c0[1]) / cell[1]);
        if !(lx >= 0.0 && lz >= 0.0 && lx <= self.nx as f32 && lz <= self.nz as f32) {
            return None;
        }
        let i = (lx.floor() as usize).min(self.nx - 1);
        let j = (lz.floor() as usize).min(self.nz - 1);
        Some((i, j, lx - i as f32, lz - j as f32))
    }

    /// The height of the ground at (x, z) in world y, `None` outside it.
    pub fn surface(&self, x: f32, z: f32) -> Option<f32> {
        let (i, j, fx, fz) = self.locate(x, z)?;
        let (y00, y10, y01, y11) = (self.vertex(i, j)[1], self.vertex(i + 1, j)[1], self.vertex(i, j + 1)[1], self.vertex(i + 1, j + 1)[1]);
        Some(if fx + fz <= 1.0 { y00 + fx * (y10 - y00) + fz * (y01 - y00) } else { y11 + (1.0 - fx) * (y01 - y11) + (1.0 - fz) * (y10 - y11) })
    }

    /// The up-pointing normal of the ground at (x, z).
    pub fn normal(&self, x: f32, z: f32) -> V3 {
        match self.locate(x, z) {
            Some((i, j, fx, fz)) => {
                let t = self.triangles(i, j)[usize::from(fx + fz > 1.0)];
                unit(cross(sub(t[1], t[0]), sub(t[2], t[0])))
            }
            None => [0.0, 1.0, 0.0],
        }
    }

    /// A ball of radius `r` at `c` against the ground: the direction that pushes it out (away from the ground) and how deep it is in.
    pub fn sphere(&self, c: V3, r: f32) -> Option<(V3, f32)> {
        if !(c[0].is_finite() && c[1].is_finite() && c[2].is_finite() && r.is_finite() && r > 0.0) {
            return None;
        }
        let r = r.min(1.0e6);
        let (cell, c0) = (self.cell(), self.corner());
        let to_cell = |v: f32, o: f32, size: f32, n: usize| -> i64 { (((v - o) / size).floor() as i64).clamp(-1, n as i64) };
        let (i0, i1) = (to_cell(c[0] - r, c0[0], cell[0], self.nx), to_cell(c[0] + r, c0[0], cell[0], self.nx));
        let (j0, j1) = (to_cell(c[2] - r, c0[1], cell[1], self.nz), to_cell(c[2] + r, c0[1], cell[1], self.nz));
        if i1 < 0 || j1 < 0 || i0 >= self.nx as i64 || j0 >= self.nz as i64 {
            return None;
        }
        let (lo, hi) = self.y_range();
        if c[1] - r > hi || c[1] + r < lo {
            return None;
        }
        let mut best: Option<(f32, V3)> = None;
        for j in j0.max(0)..=j1.min(self.nz as i64 - 1) {
            for i in i0.max(0)..=i1.min(self.nx as i64 - 1) {
                for t in self.triangles(i as usize, j as usize) {
                    let n = unit(cross(sub(t[1], t[0]), sub(t[2], t[0])));
                    let signed = dot(sub(c, t[0]), n);
                    let q = closest_on_triangle(c, t[0], t[1], t[2]);
                    let d = sub(c, q);
                    let dist = dot(d, d).sqrt();
                    let cand = if signed >= 0.0 {
                        if dist >= r {
                            continue;
                        }
                        let dir = if dist > 1e-6 { [d[0] / dist, d[1] / dist, d[2] / dist] } else { n };
                        (r - dist, dir)
                    } else if dist < 1e-4 || (q[0] - (c[0] - n[0] * signed)).abs() + (q[2] - (c[2] - n[2] * signed)).abs() < 1e-4 {
                        // the centre is under the ground, right under this triangle: out through the top
                        (r - signed, n)
                    } else {
                        continue;
                    };
                    if best.is_none_or(|b| cand.0 > b.0) {
                        best = Some(cand);
                    }
                }
            }
        }
        best.map(|(depth, n)| (n, depth))
    }

    /// An axis-aligned box centred at `c` with half extents `h` against the ground: the push-out direction and the depth. The box's
    /// underside is sampled on a grid as fine as the ground's cells; the deepest point decides.
    pub fn aabb(&self, c: V3, h: V3) -> Option<(V3, f32)> {
        if !(c.iter().chain(h.iter()).all(|v| v.is_finite())) {
            return None;
        }
        let (lo, hi) = self.y_range();
        let bottom = c[1] - h[1];
        if bottom > hi || c[1] + h[1] < lo {
            return None;
        }
        let cell = self.cell();
        let nx = ((2.0 * h[0] / cell[0]).ceil() as usize + 1).clamp(3, 9) | 1;
        let nz = ((2.0 * h[2] / cell[1]).ceil() as usize + 1).clamp(3, 9) | 1;
        let mut deepest: Option<(f32, f32, f32)> = None;
        for a in 0..nx {
            for b in 0..nz {
                let x = c[0] - h[0] + 2.0 * h[0] * a as f32 / (nx - 1) as f32;
                let z = c[2] - h[2] + 2.0 * h[2] * b as f32 / (nz - 1) as f32;
                if let Some(s) = self.surface(x, z) {
                    let pen = s - bottom;
                    if pen > 0.0 && deepest.is_none_or(|d| pen > d.0) {
                        deepest = Some((pen, x, z));
                    }
                }
            }
        }
        let (pen, x, z) = deepest?;
        let n = self.normal(x, z);
        Some((n, pen * n[1].max(0.2)))
    }

    /// A ray from `o` along the unit vector `d`, at most `max` long: the distance to the ground and its normal.
    pub fn ray(&self, o: V3, d: V3, max: f32) -> Option<(f32, V3)> {
        if !(o.iter().chain(d.iter()).all(|v| v.is_finite()) && max > 0.0) {
            return None;
        }
        let max = max.min(1.0e9);
        let (c0, y) = (self.corner(), self.y_range());
        let (lo, hi) = ([c0[0], y.0, c0[1]], [c0[0] + self.size[0], y.1, c0[1] + self.size[1]]);
        let (mut t0, mut t1) = (0.0f32, max);
        for a in 0..3 {
            if d[a].abs() < 1e-12 {
                if o[a] < lo[a] || o[a] > hi[a] {
                    return None;
                }
            } else {
                let (mut ta, mut tb) = ((lo[a] - o[a]) / d[a], (hi[a] - o[a]) / d[a]);
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                }
                t0 = t0.max(ta);
                t1 = t1.min(tb);
                if t0 > t1 {
                    return None;
                }
            }
        }
        let f = |t: f32| -> f32 {
            let p = [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t];
            match self.surface(p[0], p[2]) {
                Some(s) => p[1] - s,
                None => 1.0,
            }
        };
        let cell = self.cell();
        let step = (0.5 * cell[0].min(cell[1])).max(1e-3);
        let mut prev_t = t0;
        if f(t0) <= 0.0 {
            let p = [o[0] + d[0] * t0, 0.0, o[2] + d[2] * t0];
            return Some((t0, self.normal(p[0], p[2])));
        }
        let mut t = t0;
        let mut steps = 0u32;
        // a ray as long as the ground is wide needs about 2 x the number of cells; the cap only matters for absurd numbers
        while t < t1 && steps < 200_000 {
            steps += 1;
            t = (t + step).min(t1);
            let ft = f(t);
            if ft <= 0.0 {
                let (mut a, mut b) = (prev_t, t);
                for _ in 0..24 {
                    let m = (a + b) * 0.5;
                    if f(m) <= 0.0 {
                        b = m;
                    } else {
                        a = m;
                    }
                }
                let p = [o[0] + d[0] * b, 0.0, o[2] + d[2] * b];
                return Some((b, self.normal(p[0], p[2])));
            }
            prev_t = t;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 5 x 5 field: flat at 0.5 with a ridge (1.0) along the middle column.
    fn ridge() -> Heightfield {
        let mut s = vec![32768u16; 25];
        for r in 0..5 {
            s[r * 5 + 2] = 65535;
        }
        Heightfield::new(5, 5, s).unwrap()
    }

    fn placed(f: &Heightfield) -> Placed<'_> {
        // 40 x 40 metres, 20 metres of relief, ground level 0, centred on the origin: cells of 10 m
        let t = Terrain { size: [40.0, 40.0], height: 20.0, step: 1, ..Terrain::default() };
        Placed::new(f, [0.0, 0.0, 0.0], &t)
    }

    #[test]
    fn the_surface_follows_the_samples_and_is_none_outside_the_ground() {
        let f = ridge();
        let p = placed(&f);
        let near = |a: Option<f32>, b: f32| a.is_some_and(|a| (a - b).abs() < 0.01);
        assert!(near(p.surface(-20.0, 0.0), 10.0));
        assert!(near(p.surface(0.0, 7.0), 20.0));
        assert!(near(p.surface(-10.0, 3.0), 10.0));
        // half way up the slope between the flat part and the ridge
        assert!((p.surface(-5.0, -5.0).unwrap() - 15.0).abs() < 1e-3);
        assert_eq!(p.surface(20.5, 0.0), None);
        assert_eq!(p.surface(f32::NAN, 0.0), None);
        let n = p.normal(-15.0, 0.0);
        assert!(n[1] > 0.99, "{n:?}");
        let slope = p.normal(-5.0, 0.0);
        assert!(slope[0] < -0.3 && slope[1] > 0.5, "the slope faces away from the ridge: {slope:?}");
    }

    #[test]
    fn a_ball_rests_on_flat_ground_slides_off_a_slope_and_is_lifted_from_under_it() {
        let f = ridge();
        let p = placed(&f);
        // above the flat ground (y = 10): a ball of radius 1 whose centre is 0.5 above the ground is 0.5 deep
        let (n, depth) = p.sphere([-15.0, 10.5, 0.0], 1.0).expect("touching");
        assert!(n[1] > 0.99 && (depth - 0.5).abs() < 0.05, "{n:?} {depth}");
        // clear of the ground
        assert!(p.sphere([-15.0, 12.5, 0.0], 1.0).is_none());
        // on the slope the push is tilted away from the ridge
        let (n, _) = p.sphere([-5.0, 15.2, 0.0], 1.0).expect("touching the slope");
        assert!(n[0] < -0.2 && n[1] > 0.5, "{n:?}");
        // a centre under the ground comes out through the top
        let (n, depth) = p.sphere([-15.0, 9.0, 0.0], 1.0).expect("under it");
        assert!(n[1] > 0.99 && depth > 1.9, "{n:?} {depth}");
        // off the edge of the ground: nothing
        assert!(p.sphere([30.0, 10.0, 0.0], 1.0).is_none());
    }

    #[test]
    fn a_box_is_pushed_up_by_the_highest_point_under_it() {
        let f = ridge();
        let p = placed(&f);
        // 1 x 1 x 1 box straddling the ridge crest (y = 20): its bottom is 0.3 below the crest
        let (n, depth) = p.aabb([0.0, 20.2, 0.0], [0.5, 0.5, 0.5]).expect("on the crest");
        assert!(n[1] > 0.5 && depth > 0.0, "{n:?} {depth}");
        assert!(p.aabb([-15.0, 14.0, 0.0], [0.5, 0.5, 0.5]).is_none(), "well above the flat ground");
        assert!(p.aabb([100.0, 10.0, 0.0], [0.5, 0.5, 0.5]).is_none(), "off the ground");
    }

    #[test]
    fn rays_hit_the_ground_from_above_the_side_and_inside_the_box_and_miss_beside_it() {
        let f = ridge();
        let p = placed(&f);
        let (t, n) = p.ray([-15.0, 50.0, 0.0], [0.0, -1.0, 0.0], 100.0).expect("down onto the flat part");
        assert!((t - 40.0).abs() < 1e-2 && n[1] > 0.99, "{t} {n:?}");
        // a shallow ray from the side hits the ridge
        let d = unit([1.0, -0.05, 0.0]);
        let (t, _) = p.ray([-30.0, 18.0, 0.0], d, 200.0).expect("into the ridge");
        let hit_x = -30.0 + d[0] * t;
        assert!(hit_x > -10.0 && hit_x < 0.5, "{hit_x}");
        // too short, beside the ground, pointing away
        assert!(p.ray([-15.0, 50.0, 0.0], [0.0, -1.0, 0.0], 30.0).is_none());
        assert!(p.ray([60.0, 50.0, 0.0], [0.0, -1.0, 0.0], 100.0).is_none());
        assert!(p.ray([-15.0, 50.0, 0.0], [0.0, 1.0, 0.0], 100.0).is_none());
    }

    #[test]
    fn coarser_steps_use_fewer_cells_and_the_same_picture_gives_the_same_hash() {
        let f = ridge();
        let t = Terrain { size: [40.0, 40.0], height: 20.0, step: 2, ..Terrain::default() };
        let p = Placed::new(&f, [0.0, 0.0, 0.0], &t);
        assert_eq!((p.nx, p.nz), (2, 2));
        assert!((p.surface(0.0, 0.0).unwrap() - 20.0).abs() < 0.01);
        let again = ridge();
        assert_eq!(f.hash, again.hash);
        let mut s = vec![32768u16; 25];
        s[0] = 1;
        assert_ne!(Heightfield::new(5, 5, s).unwrap().hash, f.hash);
        assert!(Heightfield::new(1, 5, vec![0; 5]).is_err() && Heightfield::new(2, 2, vec![0; 3]).is_err());
    }

    #[test]
    fn odd_numbers_never_panic_or_loop() {
        let f = ridge();
        for size in [[0.0f32, 0.0], [f32::NAN, 5.0], [1e30, 1e30], [1e-20, 1e-20]] {
            for height in [0.0f32, -50.0, f32::INFINITY, f32::NAN, 1e30] {
                let t = Terrain { size, height, step: 3, ..Terrain::default() };
                let p = Placed::new(&f, [1.0, 2.0, 3.0], &t);
                let _ = p.surface(0.0, 0.0);
                let _ = p.sphere([0.0, 0.0, 0.0], 1.0);
                let _ = p.sphere([f32::NAN, 0.0, 0.0], f32::INFINITY);
                let _ = p.aabb([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
                let _ = p.ray([0.0, 100.0, 0.0], [0.0, -1.0, 0.0], 1e9);
                let _ = p.ray([f32::NAN, 100.0, 0.0], [0.0, -1.0, 0.0], f32::INFINITY);
            }
        }
    }
}
