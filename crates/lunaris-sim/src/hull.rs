//! Convex hulls (`ConvexCollider`, docs/PHYSICS.md): the smallest convex solid that contains all the vertices of a model. Rocks, crates, wedges,
//! gems and barrels collide as solids of that shape and turn about their own middle with the mass spread over their volume.
//!
//! **Building** (once for each model and scale, in `f64`, which on every machine gives the same bits for `+ - * /` and `sqrt`): the points are
//! put in a fixed order (sorted by x, y, z, exact duplicates dropped), then *quickhull* grows the hull from a first tetrahedron by always adding
//! the point that is farthest outside it, until no point is outside or the hull has [`MAX_HULL_VERTICES`] corners. Stopping at 64 is the
//! simplification: the points that were left out are the ones nearest the hull, and `dropped` says how far the farthest of them is from it.
//! Triangles that lie in one plane are joined into polygons, so a cube has six faces and not twelve.
//!
//! **Mass.** The volume, the centre of mass and the inertia tensor come from splitting the solid into tetrahedra (exact for a polyhedron). The
//! solver wants the inertia along the body's own axes, so the hull is stored in its **principal frame**: moved so that its centre of mass is
//! the origin and turned so that its inertia tensor is diagonal. `com` and `frame` say how to get back to the model's coordinates. A shape whose
//! tensor is already diagonal (a box, a symmetric rock) keeps the model's axes (`frame` is exactly the identity).
//!
//! Nothing here is read from a clock or a hash table: the same points give the same hull on every machine.
use crate::meshcol::V3;

/// The most corners a hull has. A model with more is simplified (see the module comment).
pub const MAX_HULL_VERTICES: usize = 64;

type D3 = [f64; 3];

fn dsub(a: D3, b: D3) -> D3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dadd(a: D3, b: D3) -> D3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn dmul(a: D3, s: f64) -> D3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn ddot(a: D3, b: D3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn dcross(a: D3, b: D3) -> D3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dlen(a: D3) -> f64 {
    ddot(a, a).sqrt()
}

/// One flat face of a hull: the outward unit normal, the plane (`dot(n, x) = d`) and the corners in counter-clockwise order seen from outside.
#[derive(Clone, Debug)]
pub struct HullFace {
    pub n: V3,
    pub d: f32,
    pub verts: Vec<u16>,
}

/// An edge where two faces meet: its two corners and the faces on either side (`fa` runs from `a` to `b` counter-clockwise, `fb` from `b` to `a`).
#[derive(Clone, Copy, Debug)]
pub struct HullEdge {
    pub a: u16,
    pub b: u16,
    pub fa: u16,
    pub fb: u16,
}

/// A convex solid in its principal frame (see the module comment).
#[derive(Clone, Debug)]
pub struct Hull {
    pub verts: Vec<V3>,
    pub faces: Vec<HullFace>,
    pub edges: Vec<HullEdge>,
    /// The volume, in cubic metres (of the scaled model).
    pub volume: f32,
    /// The principal moments of inertia of the solid for a mass of 1 (multiply by the mass), about the three axes of the principal frame.
    pub inertia: V3,
    /// The centre of mass, in the coordinates of the (scaled) model.
    pub com: V3,
    /// The orientation of the principal frame in the coordinates of the model (a unit quaternion x, y, z, w): a vector `v` of the hull is at
    /// `rotate(frame, v) + com` in the model.
    pub frame: [f32; 4],
    /// The distance of the farthest corner from the centre of mass.
    pub radius: f32,
    /// The distance from the centre of mass to the nearest face (the biggest ball about the centre that fits inside).
    pub inner: f32,
    /// FNV-1a of the numbers: the simulation hashes it, so a replay with another hull is noticed.
    pub hash: u64,
    /// How many different points the model had, and how many corners the hull has.
    pub points: usize,
    /// How far the farthest point that was left out is from the hull (0 when the hull has all the corners it needs): above 0 the hull was simplified.
    pub dropped: f32,
}

// ------------------------------------------------------------------------------------------------ quickhull

struct QFace {
    v: [u32; 3],
    n: D3,
    d: f64,
    outside: Vec<u32>,
    alive: bool,
}

fn make_face(p: &[D3], a: u32, b: u32, c: u32, inside: D3) -> Option<QFace> {
    let n = dcross(dsub(p[b as usize], p[a as usize]), dsub(p[c as usize], p[a as usize]));
    let l = dlen(n);
    if !(l > 0.0) || !l.is_finite() {
        return None;
    }
    let mut n = dmul(n, 1.0 / l);
    let mut v = [a, b, c];
    let mut d = ddot(n, p[a as usize]);
    // the inside point must be behind the face: otherwise the face is wound the wrong way
    if ddot(n, inside) - d > 0.0 {
        n = dmul(n, -1.0);
        d = -d;
        v = [a, c, b];
    }
    Some(QFace { v, n, d, outside: Vec::new(), alive: true })
}

/// The triangles of the hull of `pts` (indices into it), the corners that were used, and how far the farthest left-out point is from it.
fn quickhull(pts: &[D3], budget: usize, eps: f64) -> Result<(Vec<[u32; 3]>, Vec<u32>, f64), String> {
    let n = pts.len();
    // the first tetrahedron: the first point in the order, the farthest from it, the farthest from that line, the farthest from that plane
    let i0 = 0u32;
    let farthest = |from: &dyn Fn(D3) -> f64, skip: &[u32]| -> (u32, f64) {
        let mut best = (0u32, -1.0f64);
        for (i, p) in pts.iter().enumerate() {
            if skip.contains(&(i as u32)) {
                continue;
            }
            let d = from(*p);
            if d > best.1 {
                best = (i as u32, d);
            }
        }
        best
    };
    let p0 = pts[0];
    let (i1, d1) = farthest(&|p| ddot(dsub(p, p0), dsub(p, p0)), &[i0]);
    if !(d1 > eps * eps) {
        return Err("all the points are the same".to_string());
    }
    let e = dsub(pts[i1 as usize], p0);
    let (i2, d2) = farthest(&|p| {
        let c = dcross(e, dsub(p, p0));
        ddot(c, c)
    }, &[i0, i1]);
    if !(d2.sqrt() > eps * dlen(e)) {
        return Err("all the points lie on a line".to_string());
    }
    let pn = {
        let c = dcross(e, dsub(pts[i2 as usize], p0));
        dmul(c, 1.0 / dlen(c))
    };
    let (i3, d3) = farthest(&|p| ddot(pn, dsub(p, p0)).abs(), &[i0, i1, i2]);
    if !(d3 > eps) {
        return Err("all the points lie in one plane: a convex solid needs a volume".to_string());
    }
    let simplex = [i0, i1, i2, i3];
    let mut centroid = [0.0f64; 3];
    for &i in &simplex {
        centroid = dadd(centroid, pts[i as usize]);
    }
    let inside = dmul(centroid, 0.25);
    let mut faces: Vec<QFace> = Vec::new();
    for (a, b, c) in [(i0, i1, i2), (i0, i1, i3), (i0, i2, i3), (i1, i2, i3)] {
        faces.push(make_face(pts, a, b, c, inside).ok_or("the first tetrahedron has no area")?);
    }
    // every other point goes to the first face it is outside of
    for i in 0..n as u32 {
        if simplex.contains(&i) {
            continue;
        }
        for f in faces.iter_mut() {
            if ddot(f.n, pts[i as usize]) - f.d > eps {
                f.outside.push(i);
                break;
            }
        }
    }
    let mut used: Vec<u32> = simplex.to_vec();
    let mut dropped = 0.0f64;
    loop {
        // the point that is farthest outside, over all faces (ties: the lower face, then the lower point)
        let mut best: Option<(usize, u32, f64)> = None;
        for (fi, f) in faces.iter().enumerate() {
            if !f.alive {
                continue;
            }
            for &pi in &f.outside {
                let d = ddot(f.n, pts[pi as usize]) - f.d;
                if best.is_none_or(|b| d > b.2) {
                    best = Some((fi, pi, d));
                }
            }
        }
        let Some((_, p, dist)) = best else { break };
        if used.len() >= budget {
            dropped = dist;
            break;
        }
        let pp = pts[p as usize];
        // the faces it can see, and the edges between those and the rest (the horizon)
        let visible: Vec<usize> = (0..faces.len()).filter(|&fi| faces[fi].alive && ddot(faces[fi].n, pp) - faces[fi].d > eps).collect();
        let mut owner: std::collections::BTreeMap<(u32, u32), usize> = std::collections::BTreeMap::new();
        for (fi, f) in faces.iter().enumerate().filter(|(_, f)| f.alive) {
            for k in 0..3 {
                owner.insert((f.v[k], f.v[(k + 1) % 3]), fi);
            }
        }
        let mut horizon: Vec<(u32, u32)> = Vec::new();
        for &fi in &visible {
            let f = &faces[fi];
            for k in 0..3 {
                let (a, b) = (f.v[k], f.v[(k + 1) % 3]);
                match owner.get(&(b, a)) {
                    Some(&other) if visible.contains(&other) => {}
                    _ => horizon.push((a, b)),
                }
            }
        }
        if horizon.len() < 3 {
            return Err("the hull could not be closed (the points are too nearly flat)".to_string());
        }
        // the points that were outside the faces that go; the faces that go
        let mut loose: Vec<u32> = Vec::new();
        for &fi in &visible {
            loose.extend(faces[fi].outside.iter().copied().filter(|&q| q != p));
            faces[fi].alive = false;
            faces[fi].outside.clear();
        }
        loose.sort_unstable();
        loose.dedup();
        let first_new = faces.len();
        for &(a, b) in &horizon {
            match make_face(pts, a, b, p, inside) {
                Some(f) => faces.push(f),
                None => return Err("the hull has a face with no area".to_string()),
            }
        }
        for q in loose {
            for f in faces[first_new..].iter_mut() {
                if ddot(f.n, pts[q as usize]) - f.d > eps {
                    f.outside.push(q);
                    break;
                }
            }
        }
        used.push(p);
    }
    let tris: Vec<[u32; 3]> = faces.iter().filter(|f| f.alive).map(|f| f.v).collect();
    // it must be a closed surface: every directed edge once, and its reverse once
    let mut edges: std::collections::BTreeMap<(u32, u32), u32> = std::collections::BTreeMap::new();
    for t in &tris {
        for k in 0..3 {
            *edges.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
        }
    }
    if edges.iter().any(|(&(a, b), &c)| c != 1 || edges.get(&(b, a)) != Some(&1)) {
        return Err("the hull could not be closed (the points are too nearly flat)".to_string());
    }
    let mut corners: Vec<u32> = tris.iter().flatten().copied().collect();
    corners.sort_unstable();
    corners.dedup();
    Ok((tris, corners, dropped))
}

// ------------------------------------------------------------------------------------------------ mass

/// Eigenvalues and eigenvectors (as the columns of the returned matrix) of a symmetric 3 x 3 matrix, by Jacobi rotations in a fixed order. A
/// matrix that is diagonal already (to within 1e-12 of its entries) comes back unchanged with the identity for its eigenvectors.
fn jacobi3(mut a: [[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..60 {
        let mut rotated = false;
        for (p, q) in [(0usize, 1usize), (0, 2), (1, 2)] {
            let apq = a[p][q];
            if apq.abs() <= 1.0e-12 * (a[p][p].abs() + a[q][q].abs()) || apq == 0.0 {
                continue;
            }
            rotated = true;
            let theta = (a[q][q] - a[p][p]) / (2.0 * apq);
            let sign = if theta < 0.0 { -1.0 } else { 1.0 };
            let t = sign / (theta.abs() + (theta * theta + 1.0).sqrt());
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            let tau = s / (1.0 + c);
            a[p][p] -= t * apq;
            a[q][q] += t * apq;
            a[p][q] = 0.0;
            a[q][p] = 0.0;
            for r in 0..3 {
                if r != p && r != q {
                    let (g, h) = (a[r][p], a[r][q]);
                    a[r][p] = g - s * (h + g * tau);
                    a[r][q] = h + s * (g - h * tau);
                    a[p][r] = a[r][p];
                    a[q][r] = a[r][q];
                }
            }
            for row in v.iter_mut() {
                let (g, h) = (row[p], row[q]);
                row[p] = g - s * (h + g * tau);
                row[q] = h + s * (g - h * tau);
            }
        }
        if !rotated {
            break;
        }
    }
    ([a[0][0], a[1][1], a[2][2]], v)
}

/// The unit quaternion (x, y, z, w) of the rotation whose matrix has the given columns.
fn quat_of_columns(m: &[[f64; 3]; 3]) -> [f64; 4] {
    // m[row][col]
    let tr = m[0][0] + m[1][1] + m[2][2];
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s, 0.25 * s]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        [0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s, (m[2][1] - m[1][2]) / s]
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        [(m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s, (m[0][2] - m[2][0]) / s]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        [(m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s, (m[1][0] - m[0][1]) / s]
    };
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    [q[0] / l, q[1] / l, q[2] / l, q[3] / l]
}

fn fnv(h: &mut u64, x: u32) {
    for b in x.to_le_bytes() {
        *h = (*h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
}

impl Hull {
    /// The hull of some points (the vertices of a model) stretched by `scale` (a negative scale mirrors it). Fails, with a reason, when the
    /// points are not numbers or do not enclose any volume.
    pub fn build(points: &[V3], scale: V3) -> Result<Hull, String> {
        let mut pts: Vec<D3> = Vec::with_capacity(points.len());
        for p in points {
            let q = [p[0] as f64 * scale[0] as f64, p[1] as f64 * scale[1] as f64, p[2] as f64 * scale[2] as f64];
            if q.iter().any(|c| !c.is_finite()) {
                return Err("the model has vertices that are not numbers, or a scale that is not one".to_string());
            }
            pts.push(q);
        }
        pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])).then(a[2].total_cmp(&b[2])));
        pts.dedup();
        if pts.len() < 4 {
            return Err(format!("a convex solid needs at least four different points (the model has {})", pts.len()));
        }
        let extent = pts.iter().flat_map(|p| p.iter()).fold(0.0f64, |m, c| m.max(c.abs()));
        if !(extent > 0.0) || extent > 1.0e12 {
            return Err("the model is empty or far too big".to_string());
        }
        let eps = 1.0e-9 * extent;
        let (tris, corners, dropped) = quickhull(&pts, MAX_HULL_VERTICES, eps)?;
        // ---- mass: volume, centre and second moments from tetrahedra between a point inside and the triangles
        let mut o = [0.0f64; 3];
        for &c in &corners {
            o = dadd(o, pts[c as usize]);
        }
        o = dmul(o, 1.0 / corners.len() as f64);
        let mut volume = 0.0f64;
        let mut first = [0.0f64; 3]; // sum of volume * centroid
        let mut second = [[0.0f64; 3]; 3]; // integral of x x^T
        for t in &tris {
            let (a, b, c) = (dsub(pts[t[0] as usize], o), dsub(pts[t[1] as usize], o), dsub(pts[t[2] as usize], o));
            let v = ddot(a, dcross(b, c)) / 6.0;
            volume += v;
            let sum = dadd(dadd(a, b), c); // (the fourth corner is the origin)
            first = dadd(first, dmul(sum, v * 0.25));
            for r in 0..3 {
                for s in 0..3 {
                    let diag = a[r] * a[s] + b[r] * b[s] + c[r] * c[s];
                    second[r][s] += v / 20.0 * (diag + sum[r] * sum[s]);
                }
            }
        }
        if !(volume > 1.0e-12 * extent * extent * extent) {
            return Err("the model has no volume (it is flat or a line)".to_string());
        }
        let centre_rel = dmul(first, 1.0 / volume); // relative to `o`
        let com_d = dadd(o, centre_rel);
        // second moments about the centre of mass
        let mut sc = [[0.0f64; 3]; 3];
        for r in 0..3 {
            for s in 0..3 {
                sc[r][s] = second[r][s] - volume * centre_rel[r] * centre_rel[s];
            }
        }
        let trace = sc[0][0] + sc[1][1] + sc[2][2];
        let mut tensor = [[0.0f64; 3]; 3];
        for r in 0..3 {
            for s in 0..3 {
                tensor[r][s] = (if r == s { trace } else { 0.0 } - sc[r][s]) / volume; // per unit mass
            }
        }
        let (moments, axes) = jacobi3(tensor);
        // the principal axes are the columns; make a proper rotation of them (and give each a fixed sign)
        let mut cols = [[axes[0][0], axes[1][0], axes[2][0]], [axes[0][1], axes[1][1], axes[2][1]], [axes[0][2], axes[1][2], axes[2][2]]];
        for c in cols.iter_mut() {
            let mut big = 0usize;
            for k in 1..3 {
                if c[k].abs() > c[big].abs() + 1.0e-12 {
                    big = k;
                }
            }
            if c[big] < 0.0 {
                *c = dmul(*c, -1.0);
            }
        }
        if ddot(cols[0], dcross(cols[1], cols[2])) < 0.0 {
            cols[2] = dmul(cols[2], -1.0);
        }
        let to_frame = |p: D3| [ddot(cols[0], p), ddot(cols[1], p), ddot(cols[2], p)];
        let rot = [[cols[0][0], cols[1][0], cols[2][0]], [cols[0][1], cols[1][1], cols[2][1]], [cols[0][2], cols[1][2], cols[2][2]]];
        let mut frame = quat_of_columns(&rot);
        // (a frame that is the identity to within rounding is exactly the identity, and a centre of mass that is the origin to within a
        // millionth of the size is exactly the origin: a body that is turned and moved by them and back again then gets its numbers back)
        let snap = 1.0e-6;
        if frame[0].abs() < snap && frame[1].abs() < snap && frame[2].abs() < snap && frame[3] > 0.0 {
            frame = [0.0, 0.0, 0.0, 1.0];
        }
        let identity = frame == [0.0, 0.0, 0.0, 1.0];
        let com = [0usize, 1, 2].map(|k| if com_d[k].abs() <= snap * extent { 0.0 } else { com_d[k] });
        let local = |p: D3| {
            let rel = dsub(p, com);
            if identity {
                rel
            } else {
                to_frame(rel)
            }
        };
        // ---- the polygons: triangles in one plane are one face
        let verts_d: Vec<D3> = corners.iter().map(|&c| local(pts[c as usize])).collect();
        let index_of = |c: u32| corners.binary_search(&c).unwrap_or(0) as u16;
        let tri_planes: Vec<(D3, f64)> = tris
            .iter()
            .map(|t| {
                let (a, b, c) = (verts_d[index_of(t[0]) as usize], verts_d[index_of(t[1]) as usize], verts_d[index_of(t[2]) as usize]);
                let n = dcross(dsub(b, a), dsub(c, a));
                let n = dmul(n, 1.0 / dlen(n));
                (n, ddot(n, a))
            })
            .collect();
        let mut group: Vec<usize> = (0..tris.len()).collect();
        for i in 0..tris.len() {
            for j in 0..i {
                if group[j] == j && ddot(tri_planes[i].0, tri_planes[j].0) > 1.0 - 1.0e-9 && (tri_planes[i].1 - tri_planes[j].1).abs() < 1.0e-7 * extent {
                    group[i] = j;
                    break;
                }
            }
        }
        let mut faces: Vec<HullFace> = Vec::new();
        for lead in 0..tris.len() {
            if group[lead] != lead {
                continue;
            }
            let members: Vec<usize> = (0..tris.len()).filter(|&i| group[i] == lead).collect();
            // the outline: directed edges of the members whose reverse is not also a member's
            let mut next: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
            let mut all: std::collections::BTreeSet<(u32, u32)> = std::collections::BTreeSet::new();
            for &m in &members {
                for k in 0..3 {
                    all.insert((tris[m][k], tris[m][(k + 1) % 3]));
                }
            }
            for &(a, b) in &all {
                if !all.contains(&(b, a)) {
                    next.insert(a, b);
                }
            }
            let mut ring: Vec<u32> = Vec::new();
            if let Some(&start) = next.keys().next() {
                let mut at = start;
                loop {
                    ring.push(at);
                    at = next[&at];
                    if at == start || ring.len() > next.len() {
                        break;
                    }
                }
            }
            if ring.len() != next.len() || ring.len() < 3 {
                // not a single polygon (it should not happen): keep the triangles as they are
                for &m in &members {
                    faces.push(HullFace { n: [0.0; 3], d: 0.0, verts: tris[m].iter().map(|&c| index_of(c)).collect() });
                }
            } else {
                faces.push(HullFace { n: [0.0; 3], d: 0.0, verts: ring.iter().map(|&c| index_of(c)).collect() });
            }
        }
        // the plane of each face (from its corners, so that the face's own corners all lie on it as exactly as can be)
        let mut radius = 0.0f64;
        for v in &verts_d {
            radius = radius.max(dlen(*v));
        }
        let mut inner = f64::INFINITY;
        for f in faces.iter_mut() {
            // Newell's method: the normal of a polygon from all its corners
            let mut n = [0.0f64; 3];
            for k in 0..f.verts.len() {
                let (a, b) = (verts_d[f.verts[k] as usize], verts_d[f.verts[(k + 1) % f.verts.len()] as usize]);
                n[0] += (a[1] - b[1]) * (a[2] + b[2]);
                n[1] += (a[2] - b[2]) * (a[0] + b[0]);
                n[2] += (a[0] - b[0]) * (a[1] + b[1]);
            }
            let n = dmul(n, 1.0 / dlen(n));
            let mut d = 0.0f64;
            for &v in &f.verts {
                d += ddot(n, verts_d[v as usize]);
            }
            d /= f.verts.len() as f64;
            inner = inner.min(d);
            f.n = [n[0] as f32, n[1] as f32, n[2] as f32];
            f.d = d as f32;
        }
        // the edges, with the faces on either side
        let mut by_edge: std::collections::BTreeMap<(u16, u16), u16> = std::collections::BTreeMap::new();
        for (fi, f) in faces.iter().enumerate() {
            for k in 0..f.verts.len() {
                by_edge.insert((f.verts[k], f.verts[(k + 1) % f.verts.len()]), fi as u16);
            }
        }
        let mut edges: Vec<HullEdge> = Vec::new();
        for (&(a, b), &fa) in &by_edge {
            if a < b {
                if let Some(&fb) = by_edge.get(&(b, a)) {
                    edges.push(HullEdge { a, b, fa, fb });
                }
            }
        }
        let verts: Vec<V3> = verts_d.iter().map(|v| [v[0] as f32, v[1] as f32, v[2] as f32]).collect();
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for v in &verts {
            for c in v {
                fnv(&mut hash, c.to_bits());
            }
        }
        for f in &faces {
            fnv(&mut hash, f.verts.len() as u32);
            for &v in &f.verts {
                fnv(&mut hash, v as u32);
            }
        }
        for x in [moments[0], moments[1], moments[2], volume] {
            fnv(&mut hash, (x as f32).to_bits());
        }
        Ok(Hull {
            verts,
            faces,
            edges,
            volume: volume as f32,
            inertia: [moments[0] as f32, moments[1] as f32, moments[2] as f32],
            com: [com[0] as f32, com[1] as f32, com[2] as f32],
            frame: [frame[0] as f32, frame[1] as f32, frame[2] as f32, frame[3] as f32],
            radius: radius as f32,
            inner: inner as f32,
            hash,
            points: pts.len(),
            dropped: dropped as f32,
        })
    }

    /// Where the hull's principal frame stands in the world for an entity with this `Transform`: its centre of mass, and the orientation of
    /// its principal axes (a unit quaternion). The entity's origin is the model's origin; the body is the solid about its centre of mass.
    pub fn pose(&self, t: &crate::components::Transform) -> (V3, [f32; 4]) {
        use crate::physics_rot::{axes_of, normalize_q, quat_mul, rotate};
        let qt = normalize_q(t.rot);
        let q = if self.frame == [0.0, 0.0, 0.0, 1.0] { qt } else { normalize_q(quat_mul(qt, self.frame)) };
        if self.com == [0.0; 3] {
            return (t.pos, q);
        }
        let r = rotate(&axes_of(qt), self.com);
        ([t.pos[0] + r[0], t.pos[1] + r[1], t.pos[2] + r[2]], q)
    }

    /// The inverse of [`Hull::pose`]: the `Transform` position and rotation that put the principal frame at `pos` with orientation `q`.
    pub fn model_pose(&self, pos: V3, q: [f32; 4]) -> (V3, [f32; 4]) {
        use crate::physics_rot::{normalize_q, quat_conj, quat_mul};
        let rot = if self.frame == [0.0, 0.0, 0.0, 1.0] { q } else { normalize_q(quat_mul(q, quat_conj(self.frame))) };
        (self.model_pos(pos, rot), rot)
    }

    /// The `Transform` position of an entity whose model is turned by `rot` and whose centre of mass is at `pos`.
    pub fn model_pos(&self, pos: V3, rot: [f32; 4]) -> V3 {
        use crate::physics_rot::{axes_of, normalize_q, rotate};
        if self.com == [0.0; 3] {
            return pos;
        }
        let r = rotate(&axes_of(normalize_q(rot)), self.com);
        [pos[0] - r[0], pos[1] - r[1], pos[2] - r[2]]
    }

    /// The inverse of the principal moments of inertia for a body of mass `m` (what the solver wants).
    pub fn inv_inertia(&self, m: f32) -> V3 {
        let floor = 1.0e-6 * m;
        self.inertia.map(|i| 1.0 / (i * m).max(floor))
    }

    /// The box around the hull, as half sizes along the world axes when the hull's principal axes are `axes` (the columns of the rotation).
    pub fn world_extent(&self, axes: &[V3; 3]) -> V3 {
        let mut e = [0.0f32; 3];
        for v in &self.verts {
            for (a, ea) in e.iter_mut().enumerate() {
                let x = axes[0][a] * v[0] + axes[1][a] * v[1] + axes[2][a] * v[2];
                *ea = ea.max(x.abs());
            }
        }
        e
    }

    /// Where a ray in the world (`o` + `d` * t, `d` a unit vector) hits the solid when its centre of mass is at `pos` and its principal axes
    /// are turned by `q`: the distance and the normal of the face (facing the ray).
    pub fn ray_world(&self, pos: V3, q: [f32; 4], o: V3, d: V3, max: f32) -> Option<(f32, V3)> {
        use crate::physics_rot::{axes_of, rotate, unrotate};
        let axes = axes_of(q);
        let (lo, ld) = (unrotate(&axes, [o[0] - pos[0], o[1] - pos[1], o[2] - pos[2]]), unrotate(&axes, d));
        self.ray_local(lo, ld, max).map(|(t, n)| (t, rotate(&axes, n)))
    }

    /// Where a ray (`o` + `d` * t, in the hull's own principal frame) enters the solid, between `t` = 0 and `max`: the distance and the unit
    /// normal of the face it hits, or `None` if it misses. A ray that starts inside reports where it leaves.
    pub fn ray_local(&self, o: V3, d: V3, max: f32) -> Option<(f32, V3)> {
        if !(o.iter().chain(d.iter()).all(|v| v.is_finite()) && max >= 0.0) {
            return None;
        }
        let (mut t0, mut t1) = (0.0f32, max);
        let (mut n0, mut n1) = ([0.0f32; 3], [0.0f32; 3]);
        for f in &self.faces {
            let den = f.n[0] * d[0] + f.n[1] * d[1] + f.n[2] * d[2];
            let dist = f.n[0] * o[0] + f.n[1] * o[1] + f.n[2] * o[2] - f.d; // > 0: the start is outside this face
            if den.abs() < 1.0e-12 {
                if dist > 0.0 {
                    return None;
                }
                continue;
            }
            let t = -dist / den;
            if den < 0.0 {
                // entering through this face
                if t > t0 {
                    t0 = t;
                    n0 = f.n;
                }
            } else if t < t1 {
                t1 = t;
                n1 = f.n;
            }
            if t0 > t1 {
                return None;
            }
        }
        if t0 > 0.0 {
            Some((t0, n0))
        } else if n1 != [0.0; 3] {
            // the ray starts inside: it leaves through the face n1, and the normal reported faces the ray (like every other ray hit)
            Some((t1, [-n1[0], -n1[1], -n1[2]]))
        } else {
            None
        }
    }
}
