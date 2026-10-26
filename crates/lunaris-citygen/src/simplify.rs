//! Mesh simplification by quadric error metrics (Garland and Heckbert, 1997): edges are collapsed cheapest first, the cost of a collapse being how far the new vertex is from the planes of the triangles that met at the
//! two old ones. Edges on the border of the surface (which is where the seams of a model's texture are, since vertices there are split) carry a heavy extra plane that keeps them in place, and a collapse that would
//! turn a triangle over is refused. The picture coordinates and normals of the new vertex are taken from the old ones in proportion to where along the edge it falls.

use crate::mesh::*;
use crate::props::PropPart;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// A symmetric 4x4 quadric (10 numbers): the sum of squared distances to a set of planes.
#[derive(Clone, Copy, Default)]
struct Q([f64; 10]);

impl Q {
    fn plane(a: f64, b: f64, c: f64, d: f64, w: f64) -> Q {
        Q([w * a * a, w * a * b, w * a * c, w * a * d, w * b * b, w * b * c, w * b * d, w * c * c, w * c * d, w * d * d])
    }
    fn add(&self, o: &Q) -> Q {
        let mut r = [0.0; 10];
        for i in 0..10 {
            r[i] = self.0[i] + o.0[i];
        }
        Q(r)
    }
    fn error(&self, p: [f64; 3]) -> f64 {
        let q = &self.0;
        let (x, y, z) = (p[0], p[1], p[2]);
        q[0] * x * x + 2.0 * q[1] * x * y + 2.0 * q[2] * x * z + 2.0 * q[3] * x + q[4] * y * y + 2.0 * q[5] * y * z + 2.0 * q[6] * y + q[7] * z * z + 2.0 * q[8] * z + q[9]
    }
    /// The point that minimises the error, if the 3x3 system is well conditioned.
    fn optimum(&self) -> Option<[f64; 3]> {
        let q = &self.0;
        let m = [[q[0], q[1], q[2]], [q[1], q[4], q[5]], [q[2], q[5], q[7]]];
        let b = [-q[3], -q[6], -q[8]];
        let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = |r: usize, c: usize| -> f64 {
            let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
            let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
            // cofactor of (c, r): the adjugate is the transposed cofactor matrix
            (m[c1][r1] * m[c2][r2] - m[c1][r2] * m[c2][r1]) / det
        };
        let mut p = [0.0; 3];
        for (r, pr) in p.iter_mut().enumerate() {
            *pr = (0..3).map(|c| inv(r, c) * b[c]).sum();
        }
        Some(p)
    }
}

#[derive(PartialEq)]
struct Cand {
    cost: f64,
    a: u32,
    b: u32,
    ver: u32,
}
impl Eq for Cand {}
impl PartialOrd for Cand {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Cand {
    fn cmp(&self, o: &Self) -> Ordering {
        // a min-heap on cost
        o.cost.total_cmp(&self.cost).then(o.a.cmp(&self.a)).then(o.b.cmp(&self.b))
    }
}

fn d3(p: V3) -> [f64; 3] {
    [p[0] as f64, p[1] as f64, p[2] as f64]
}

/// A simpler copy of a part with at most `target` triangles (or the part itself when it already is).
pub fn simplify(part: &PropPart, target: usize) -> PropPart {
    let tris0 = part.idx.len() / 3;
    if tris0 <= target || target == 0 {
        return part.clone();
    }
    let n = part.pos.len();
    let mut pos: Vec<[f64; 3]> = part.pos.iter().map(|p| d3(*p)).collect();
    let mut nrm: Vec<V3> = part.nrm.clone();
    let mut uv: Vec<[f32; 2]> = part.uv.clone();
    let mut tri: Vec<[u32; 3]> = part.idx.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
    let mut alive: Vec<bool> = vec![true; tri.len()];
    let mut q: Vec<Q> = vec![Q::default(); n];
    let mut ver: Vec<u32> = vec![0; n];
    let mut dead: Vec<bool> = vec![false; n];
    let mut vt: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    let plane_of = |p: &[[f64; 3]], t: [u32; 3]| -> Option<(f64, f64, f64, f64, f64)> {
        let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let nn = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = (nn[0] * nn[0] + nn[1] * nn[1] + nn[2] * nn[2]).sqrt();
        if l < 1e-14 {
            return None;
        }
        let (x, y, z) = (nn[0] / l, nn[1] / l, nn[2] / l);
        Some((x, y, z, -(x * a[0] + y * a[1] + z * a[2]), l * 0.5))
    };
    let mut edge_count: HashMap<(u32, u32), u32> = HashMap::new();
    for (ti, t) in tri.iter().enumerate() {
        for k in 0..3 {
            vt[t[k] as usize].insert(ti as u32);
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *edge_count.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
        if let Some((a, b, c, d, area)) = plane_of(&pos, *t) {
            let pq = Q::plane(a, b, c, d, area);
            for k in 0..3 {
                q[t[k] as usize] = q[t[k] as usize].add(&pq);
            }
        }
    }
    // the border: a plane through each border edge, square to its triangle, heavily weighted
    for (ti, t) in tri.iter().enumerate() {
        let Some((fa, fb, fc, _, _)) = plane_of(&pos, *t) else { continue };
        let _ = ti;
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if edge_count[&(a.min(b), a.max(b))] == 1 {
                let (pa, pb) = (pos[a as usize], pos[b as usize]);
                let e = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
                let nn = [e[1] * fc - e[2] * fb, e[2] * fa - e[0] * fc, e[0] * fb - e[1] * fa];
                let l = (nn[0] * nn[0] + nn[1] * nn[1] + nn[2] * nn[2]).sqrt();
                if l > 1e-14 {
                    let (x, y, z) = (nn[0] / l, nn[1] / l, nn[2] / l);
                    let pq = Q::plane(x, y, z, -(x * pa[0] + y * pa[1] + z * pa[2]), 100.0 * (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt());
                    q[a as usize] = q[a as usize].add(&pq);
                    q[b as usize] = q[b as usize].add(&pq);
                }
            }
        }
    }
    let place = |q: &Q, pa: [f64; 3], pb: [f64; 3]| -> ([f64; 3], f64) {
        let mid = [(pa[0] + pb[0]) * 0.5, (pa[1] + pb[1]) * 0.5, (pa[2] + pb[2]) * 0.5];
        let mut best = (pa, q.error(pa));
        for c in [pb, mid] {
            let e = q.error(c);
            if e < best.1 {
                best = (c, e);
            }
        }
        if let Some(o) = q.optimum() {
            // an optimum far from the edge is not to be trusted
            let span = ((pa[0] - pb[0]).powi(2) + (pa[1] - pb[1]).powi(2) + (pa[2] - pb[2]).powi(2)).sqrt();
            let off = ((o[0] - mid[0]).powi(2) + (o[1] - mid[1]).powi(2) + (o[2] - mid[2]).powi(2)).sqrt();
            let e = q.error(o);
            if off < span * 2.0 && e < best.1 {
                best = (o, e);
            }
        }
        best
    };
    let mut heap: BinaryHeap<Cand> = BinaryHeap::new();
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    for t in &tri {
        for k in 0..3 {
            let (a, b) = (t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3]));
            if seen.insert((a, b)) {
                let qq = q[a as usize].add(&q[b as usize]);
                heap.push(Cand { cost: place(&qq, pos[a as usize], pos[b as usize]).1, a, b, ver: 0 });
            }
        }
    }
    let mut live = tri.len();
    while live > target {
        let Some(c) = heap.pop() else { break };
        let (a, b) = (c.a as usize, c.b as usize);
        if dead[a] || dead[b] || c.ver != ver[a].wrapping_add(ver[b]) {
            continue;
        }
        let qq = q[a].add(&q[b]);
        let (np, _) = place(&qq, pos[a], pos[b]);
        // refuse a collapse that turns a triangle over or makes one a sliver
        let around: Vec<u32> = vt[a].union(&vt[b]).copied().filter(|t| alive[*t as usize]).collect();
        let mut ok = true;
        for &ti in &around {
            let t = tri[ti as usize];
            if t.contains(&(a as u32)) && t.contains(&(b as u32)) {
                continue;
            }
            let moved = |v: u32| -> [f64; 3] { if v as usize == a || v as usize == b { np } else { pos[v as usize] } };
            let old = plane_of(&pos, t);
            let new = {
                let nt = [if t[0] as usize == b { a as u32 } else { t[0] }, if t[1] as usize == b { a as u32 } else { t[1] }, if t[2] as usize == b { a as u32 } else { t[2] }];
                let pp = [moved(nt[0]), moved(nt[1]), moved(nt[2])];
                let tmp = [pp[0], pp[1], pp[2]];
                plane_of(&tmp, [0, 1, 2])
            };
            match (old, new) {
                (Some(o), Some(nw)) => {
                    if o.0 * nw.0 + o.1 * nw.1 + o.2 * nw.2 < 0.2 {
                        ok = false;
                        break;
                    }
                }
                (_, None) => {
                    ok = false;
                    break;
                }
                _ => {}
            }
        }
        if !ok {
            continue;
        }
        // collapse b into a: the new place, picture coordinates and normal from where along the edge it falls
        let (pa, pb) = (pos[a], pos[b]);
        let e = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
        let l2 = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).max(1e-18);
        let t = (((np[0] - pa[0]) * e[0] + (np[1] - pa[1]) * e[1] + (np[2] - pa[2]) * e[2]) / l2).clamp(0.0, 1.0) as f32;
        uv[a] = [uv[a][0] + (uv[b][0] - uv[a][0]) * t, uv[a][1] + (uv[b][1] - uv[a][1]) * t];
        nrm[a] = norm([nrm[a][0] * (1.0 - t) + nrm[b][0] * t, nrm[a][1] * (1.0 - t) + nrm[b][1] * t, nrm[a][2] * (1.0 - t) + nrm[b][2] * t]);
        pos[a] = np;
        q[a] = qq;
        dead[b] = true;
        ver[a] = ver[a].wrapping_add(1);
        let tb: Vec<u32> = vt[b].iter().copied().collect();
        for ti in tb {
            if !alive[ti as usize] {
                continue;
            }
            let tt = &mut tri[ti as usize];
            for v in tt.iter_mut() {
                if *v as usize == b {
                    *v = a as u32;
                }
            }
            if tt[0] == tt[1] || tt[1] == tt[2] || tt[0] == tt[2] {
                alive[ti as usize] = false;
                live -= 1;
                for v in *tt {
                    vt[v as usize].remove(&ti);
                }
            } else {
                vt[a].insert(ti);
            }
        }
        vt[b].clear();
        // new costs for the edges round a
        let mut nbr: HashSet<u32> = HashSet::new();
        for &ti in &vt[a] {
            if alive[ti as usize] {
                for v in tri[ti as usize] {
                    if v as usize != a {
                        nbr.insert(v);
                    }
                }
            }
        }
        for v in nbr {
            let (x, y) = (a.min(v as usize) as u32, a.max(v as usize) as u32);
            let qq = q[x as usize].add(&q[y as usize]);
            heap.push(Cand { cost: place(&qq, pos[x as usize], pos[y as usize]).1, a: x, b: y, ver: ver[x as usize].wrapping_add(ver[y as usize]) });
        }
    }
    // the survivors
    let mut remap: Vec<u32> = vec![u32::MAX; n];
    let mut out = PropPart { mat: part.mat, cut: part.cut, pos: Vec::new(), nrm: Vec::new(), uv: Vec::new(), idx: Vec::new() };
    for (ti, t) in tri.iter().enumerate() {
        if !alive[ti] {
            continue;
        }
        for &v in t {
            if remap[v as usize] == u32::MAX {
                remap[v as usize] = out.pos.len() as u32;
                let p = pos[v as usize];
                out.pos.push([p[0] as f32, p[1] as f32, p[2] as f32]);
                out.nrm.push(nrm[v as usize]);
                out.uv.push(uv[v as usize]);
            }
            out.idx.push(remap[v as usize]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat grid of n x n quads in the xz plane with a bump in the middle.
    fn grid(n: usize) -> PropPart {
        let mut p = PropPart { mat: 0, cut: false, pos: Vec::new(), nrm: Vec::new(), uv: Vec::new(), idx: Vec::new() };
        for j in 0..=n {
            for i in 0..=n {
                let (x, z) = (i as f32 / n as f32, j as f32 / n as f32);
                let y = if (x - 0.5).abs() < 0.2 && (z - 0.5).abs() < 0.2 { 0.3 } else { 0.0 };
                p.pos.push([x, y, z]);
                p.nrm.push([0.0, 1.0, 0.0]);
                p.uv.push([x, z]);
            }
        }
        for j in 0..n {
            for i in 0..n {
                let a = (j * (n + 1) + i) as u32;
                let (b, c, d) = (a + 1, a + (n + 1) as u32 + 1, a + (n + 1) as u32);
                p.idx.extend_from_slice(&[a, b, c, a, c, d]);
            }
        }
        p
    }

    #[test]
    fn a_grid_loses_its_flat_triangles_first_and_keeps_its_bump_and_its_border() {
        let g = grid(20);
        assert_eq!(g.idx.len() / 3, 800);
        let s = simplify(&g, 100);
        assert!(s.idx.len() / 3 <= 100 && s.idx.len() / 3 >= 60, "{} triangles", s.idx.len() / 3);
        let (mut ymax, mut x0, mut x1, mut z0, mut z1) = (0.0f32, f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in &s.pos {
            ymax = ymax.max(p[1]);
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            z0 = z0.min(p[2]);
            z1 = z1.max(p[2]);
        }
        assert!(ymax > 0.25, "the bump went flat: {ymax}");
        assert!(x0 < 0.001 && x1 > 0.999 && z0 < 0.001 && z1 > 0.999, "the border moved in: {x0} {x1} {z0} {z1}");
        // no triangle is turned over (the grid is wound so that its triangles face down)
        for t in s.idx.chunks_exact(3) {
            let (a, b, c) = (s.pos[t[0] as usize], s.pos[t[1] as usize], s.pos[t[2] as usize]);
            let n = cross(sub(b, a), sub(c, a));
            assert!(n[1] < 1e-6, "a triangle turned over");
        }
    }

    #[test]
    fn a_part_within_the_target_is_returned_as_it_is() {
        let g = grid(3);
        assert_eq!(simplify(&g, 100).idx.len(), g.idx.len());
    }
}
