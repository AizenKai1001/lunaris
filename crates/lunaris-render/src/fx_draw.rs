//! The drawing of particle systems (docs/FX.md): one mesh a **layer** (an emitter that has particles), in world space, new for every picture, in the vertex layout the cloud renderers already draw (`particles.rs`: colour in the vertex colour, opacity in the texture weight).
//! Sprites face the camera, follow the particle's speed (sparks, rain) or lie in a plane (a ripple); ribbons run through the places a particle has been; solids are small tumbling shapes. Flipbook pictures play over the particle's life, at random or at a rate. Every
//! particle's size, colour, opacity and light follow the emitter's curves at its age. Pictures that are alpha-blended are sorted far to near (a stable radix sort: the same for the same view); added ones are drawn in the order of the list, which is the same light
//! whichever way it is added up. Looking at the particles and filling the mesh are done in slices by `threads` workers and joined in order, so the mesh is the same whatever the number.
use crate::fx_sim::*;
use crate::math::*;
use crate::mesh::Mesh;
use crate::particles::{quad_uv, rotate, solid_faces, CameraBasis};
use crate::render::LightData;
use lunaris_sim::fx::*;
use lunaris_sim::particle_modules::Solid as ModSolid;

/// The most solids and ribbon points one layer draws.
const MAX_SOLIDS: usize = 20_000;
const MAX_RIBBON_VERTS: usize = 400_000;
/// Particles a worker is given at the least: below this a layer is done on the calling thread.
const SLICE: usize = 4096;

/// One emitter's picture.
pub struct FxLayer {
    pub emitter: usize,
    pub mesh: Mesh,
    /// The texture's name (`soft` when the emitter names none).
    pub texture: String,
    pub additive: bool,
    /// Sampled without smoothing.
    pub nearest: bool,
    /// The soft-particle distance in metres (0: none).
    pub soft: f32,
}

/// What a particle looks like at its age, in the world.
#[derive(Clone, Copy)]
struct Look {
    pos: V3,
    vel: V3,
    size: f32,
    /// The cosine and sine of the angle it is turned by.
    cs: [f32; 2],
    /// Linear colour (the display colour squared) and opacity.
    rgb: V3,
    alpha: f32,
    frame: u32,
    depth: f32,
    id: u32,
    index: usize,
}

fn gradient1(g: &Option<lunaris_sim::particle_modules::Gradient<1>>, u: f32) -> f32 {
    g.as_ref().map_or(1.0, |g| g.at(u)[0])
}

/// The particles of `parts` (the ones from number `first` on) that can be seen: where they are, how big, what colour, in front of the camera.
fn look_range(def: &Emitter, sys: &FxSystem, cam: &CameraBasis, parts: &[P], first: usize) -> Vec<Look> {
    let local = def.space == Space::Local;
    let c = &def.curves;
    let mut out = Vec::with_capacity(parts.len());
    let st = &sys.tint;
    let size_k = if local { sys.scale } else { 1.0 };
    let frames = def.render.frames.max(1);
    for (k, p) in parts.iter().enumerate() {
        let u = (p.age / p.life).clamp(0.0, 1.0);
        let size = p.size * gradient1(&c.size, u) * size_k;
        let mut col = p.color;
        if let Some(g) = &c.color {
            let k = g.at(u);
            for n in 0..4 {
                col[n] *= k[n];
            }
        }
        let alpha = (col[3] * st[3] * gradient1(&c.alpha, u)).clamp(0.0, 1.0);
        if alpha <= 0.002 || size.abs() <= 1e-6 {
            continue;
        }
        let (pos, vel) = if local { (sys.model.transform_point(p.pos), sys.model.transform_dir(p.vel)) } else { (p.pos, p.vel) };
        let depth = dot(sub(pos, cam.pos), cam.forward);
        // (written so that a position that is not a number is left out too)
        if !(depth > 0.01) || !vel.iter().all(|v| v.is_finite()) {
            continue;
        }
        let r = [(col[0] * st[0]).max(0.0), (col[1] * st[1]).max(0.0), (col[2] * st[2]).max(0.0)];
        let frame = match def.render.frame_mode {
            FrameMode::OverLife => ((u * frames as f32) as u32).min(frames - 1),
            FrameMode::Random => p.frame % frames,
            FrameMode::Fps(f) => ((p.age * f) as u32) % frames,
        };
        out.push(Look { pos, vel, size, cs: p.cs, rgb: [r[0] * r[0], r[1] * r[1], r[2] * r[2]], alpha, frame, depth, id: p.id, index: first + k });
    }
    out
}

/// How many slices of at least `SLICE` items a job of `n` items is cut in for `threads` workers.
fn workers(n: usize, threads: usize) -> usize {
    threads.clamp(1, 16).min((n / SLICE).max(1))
}

fn looks_of(def: &Emitter, ei: usize, sys: &FxSystem, cam: &CameraBasis, threads: usize) -> Vec<Look> {
    let parts = &sys.em[ei].parts;
    let w = workers(parts.len(), threads);
    if w <= 1 {
        return look_range(def, sys, cam, parts, 0);
    }
    let per = parts.len().div_ceil(w);
    let pieces: Vec<Vec<Look>> = std::thread::scope(|s| {
        let handles: Vec<_> = parts.chunks(per).enumerate().map(|(k, chunk)| s.spawn(move || look_range(def, sys, cam, chunk, k * per))).collect();
        handles.into_iter().map(|h| h.join().expect("look worker")).collect()
    });
    let mut out = Vec::with_capacity(pieces.iter().map(|p| p.len()).sum());
    for p in pieces {
        out.extend(p);
    }
    out
}

/// The positions of `keys` in ascending key order, equal keys in the order they came: a stable sort, so the same for the same input. Short lists go to the library's; long ones to three passes of an 11-bit radix sort (a pass touches each item once, in order).
pub(crate) fn order(keys: &[u32]) -> Vec<u32> {
    let n = keys.len();
    if n < 1024 {
        let mut idx: Vec<u32> = (0..n as u32).collect();
        idx.sort_by_key(|&i| keys[i as usize]);
        return idx;
    }
    let mut a: Vec<(u32, u32)> = keys.iter().enumerate().map(|(i, &k)| (k, i as u32)).collect();
    let mut b: Vec<(u32, u32)> = vec![(0, 0); n];
    for pass in 0..3 {
        let shift = pass * 11;
        let mut count = vec![0u32; 2049];
        for &(k, _) in &a {
            count[((k >> shift) & 2047) as usize + 1] += 1;
        }
        for i in 0..2048 {
            count[i + 1] += count[i];
        }
        for &(k, i) in &a {
            let bucket = ((k >> shift) & 2047) as usize;
            b[count[bucket] as usize] = (k, i);
            count[bucket] += 1;
        }
        std::mem::swap(&mut a, &mut b);
    }
    a.into_iter().map(|(_, i)| i).collect()
}

/// A flat picture's four corners (order (-1,-1), (1,-1), (1,1), (-1,1)) for a particle, and the way its face looks.
fn corners(def: &Emitter, l: &Look, cam: &CameraBasis, axis_world: Option<V3>) -> ([V3; 4], V3) {
    let (hx, hy) = (l.size.abs() * 0.5, l.size.abs() * 0.5 * def.init.aspect);
    let (right, up, hx, hy, normal) = match def.render.facing {
        Facing::Camera => {
            let (c, s) = (l.cs[0], l.cs[1]);
            if s == 0.0 && c == 1.0 {
                (cam.right, cam.up, hx, hy, scale(cam.forward, -1.0))
            } else {
                (add(scale(cam.right, c), scale(cam.up, s)), sub(scale(cam.up, c), scale(cam.right, s)), hx, hy, scale(cam.forward, -1.0))
            }
        }
        Facing::Velocity => {
            let speed = length(l.vel);
            if speed > 1e-4 {
                let along = scale(l.vel, 1.0 / speed);
                let across = cross(along, cam.forward);
                let across = if length(across) < 1e-5 { cam.right } else { normalize(across) };
                // the picture's height runs along the way it flies
                (across, along, hx, hy + speed * def.render.stretch * 0.5, scale(cam.forward, -1.0))
            } else {
                (cam.right, cam.up, hx, hy, scale(cam.forward, -1.0))
            }
        }
        Facing::Axis(_) => {
            let axis = axis_world.unwrap_or([0.0, 1.0, 0.0]);
            let h = if axis[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
            let t1 = normalize(cross(axis, h));
            let t2 = cross(axis, t1);
            let (c, s) = (l.cs[0], l.cs[1]);
            // it shows its face to whichever side the camera is on
            let n = if dot(axis, sub(cam.pos, l.pos)) >= 0.0 { axis } else { scale(axis, -1.0) };
            (add(scale(t1, c), scale(t2, s)), sub(scale(t2, c), scale(t1, s)), hx, hy, n)
        }
    };
    ([(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(sx, sy)| add(l.pos, add(scale(right, sx * hx), scale(up, sy * hy)))), normal)
}

fn tumble_axis(id: u32) -> V3 {
    let h = |k: u32| {
        let mut x = id.wrapping_mul(0x9E37_79B1) ^ k.wrapping_mul(0x85EB_CA6B);
        x ^= x >> 15;
        x = x.wrapping_mul(0x2C1B_3C6D);
        x ^= x >> 13;
        (x >> 8) as f32 / 16_777_216.0
    };
    normalize([h(1) - 0.5 + 1e-3, h(2) - 0.5, h(3) - 0.5])
}

/// The mesh of sprites for `looks`, in this order: four vertices and six indices each, at the place only their number decides, filled by slices.
fn sprites(def: &Emitter, sys: &FxSystem, looks: &[Look], cam: &CameraBasis, threads: usize) -> Mesh {
    let r = &def.render;
    let (cols, rows) = (r.sheet[0].max(1), r.sheet[1].max(1));
    let (du, dv) = (1.0 / cols as f32, 1.0 / rows as f32);
    let axis_world = match r.facing {
        Facing::Axis(a) => Some(if def.space == Space::Local { normalize(sys.model.transform_dir(a)) } else { a }),
        _ => None,
    };
    let n = looks.len();
    let mut mesh = Mesh::new();
    mesh.pos = vec![[0.0; 3]; n * 4];
    mesh.nrm = vec![[0.0; 3]; n * 4];
    mesh.uv = vec![[0.0; 2]; n * 4];
    mesh.col = vec![[0.0; 3]; n * 4];
    mesh.texw = vec![0.0; n * 4];
    mesh.idx = vec![0; n * 6];
    let fill = |first: usize, items: &[Look], pos: &mut [V3], nrm: &mut [V3], uv: &mut [[f32; 2]], col: &mut [V3], texw: &mut [f32], idx: &mut [u32]| {
        for (k, l) in items.iter().enumerate() {
            let (q, normal) = corners(def, l, cam, axis_world);
            let (fx, fy) = (l.frame % cols, l.frame / cols);
            let (u0, v1) = (fx as f32 / cols as f32, 1.0 - fy as f32 / rows as f32);
            for (j, (c, (u, v))) in q.iter().zip(quad_uv(u0, v1, du, dv)).enumerate() {
                pos[k * 4 + j] = *c;
                nrm[k * 4 + j] = normal;
                uv[k * 4 + j] = [u, v];
                col[k * 4 + j] = l.rgb;
                texw[k * 4 + j] = l.alpha;
            }
            let base = ((first + k) * 4) as u32;
            idx[k * 6..k * 6 + 6].copy_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    };
    let w = workers(n, threads);
    let Mesh { pos, nrm, uv, col, texw, idx, .. } = &mut mesh;
    if w <= 1 {
        fill(0, looks, pos, nrm, uv, col, texw, idx);
    } else {
        let per = n.div_ceil(w);
        let fill = &fill;
        std::thread::scope(|s| {
            let parts = looks.chunks(per).zip(pos.chunks_mut(per * 4)).zip(nrm.chunks_mut(per * 4)).zip(uv.chunks_mut(per * 4)).zip(col.chunks_mut(per * 4)).zip(texw.chunks_mut(per * 4)).zip(idx.chunks_mut(per * 6));
            for (k, ((((((items, pos), nrm), uv), col), texw), idx)) in parts.enumerate() {
                s.spawn(move || fill(k * per, items, pos, nrm, uv, col, texw, idx));
            }
        });
    }
    mesh
}

/// The layers of a system for this camera: one for every emitter that has something to show, in the emitters' order.
pub fn build(sys: &FxSystem, cam: &CameraBasis, threads: usize) -> Vec<FxLayer> {
    let mut layers = Vec::new();
    for (ei, def) in sys.effect.emitters.iter().enumerate() {
        if sys.em[ei].parts.is_empty() {
            continue;
        }
        let t_look = crate::Timed::new(|| format!("fx: looking at the {} particles of `{}`", sys.em[ei].parts.len(), def.name));
        let mut looks = looks_of(def, ei, sys, cam, threads);
        drop(t_look);
        if looks.is_empty() {
            continue;
        }
        let r = &def.render;
        if !r.additive && r.sort {
            // far to near: the larger depth first (a positive depth's bits go up with it); equal depths stay in the order of the list
            let _t = crate::Timed::new(|| format!("fx: sorting {} pictures of `{}`", looks.len(), def.name));
            let keys: Vec<u32> = looks.iter().map(|l| !l.depth.to_bits()).collect();
            let sorted: Vec<Look> = order(&keys).into_iter().map(|i| looks[i as usize]).collect();
            looks = sorted;
        }
        let _t_mesh = crate::Timed::new(|| format!("fx: the mesh of `{}`", def.name));
        let (cols, rows) = (r.sheet[0].max(1), r.sheet[1].max(1));
        let (du, dv) = (1.0 / cols as f32, 1.0 / rows as f32);
        let frame_uv = |f: u32| {
            let (fx, fy) = (f % cols, f / cols);
            (fx as f32 / cols as f32, 1.0 - fy as f32 / rows as f32)
        };
        let mut mesh = Mesh::new();
        match r.kind {
            Kind::Sprite => mesh = sprites(def, sys, &looks, cam, threads),
            Kind::Ribbon => {
                let npts = r.ribbon_points as usize;
                let em = &sys.em[ei];
                let budget = (MAX_RIBBON_VERTS / (npts + 1) / 2).max(1);
                for l in looks.iter().take(budget) {
                    let m = em.tmeta[l.index];
                    if m.count == 0 {
                        continue;
                    }
                    // the head, then the places it has been, newest first
                    let mut pts: Vec<V3> = Vec::with_capacity(m.count as usize + 1);
                    pts.push(l.pos);
                    for j in 0..m.count as usize {
                        let slot = (m.head as usize + npts - j) % npts;
                        let p = em.trail[l.index * npts + slot];
                        pts.push(if def.space == Space::Local { sys.model.transform_point(p) } else { p });
                    }
                    let (u0, v1) = frame_uv(l.frame);
                    let n = pts.len();
                    let base = mesh.pos.len() as u32;
                    for (j, q) in pts.iter().enumerate() {
                        let way = if j + 1 < n { sub(*q, pts[j + 1]) } else { sub(pts[j - 1], *q) };
                        let side = cross(way, cam.forward);
                        let side = if length(side) < 1e-7 { cam.right } else { normalize(side) };
                        let along = j as f32 / (n - 1) as f32;
                        let half = l.size.abs() * 0.5 * r.ribbon_width * (1.0 - along);
                        let fade = (1.0 - along) * (1.0 - along);
                        for (sx, u) in [(-1.0f32, u0), (1.0, u0 + du)] {
                            mesh.pos.push(add(*q, scale(side, sx * half)));
                            mesh.nrm.push(scale(cam.forward, -1.0));
                            mesh.uv.push([u, v1 - dv * along]);
                            mesh.col.push(l.rgb);
                            mesh.texw.push(l.alpha * fade);
                        }
                    }
                    for j in 0..(n as u32 - 1) {
                        let b = base + j * 2;
                        mesh.idx.extend_from_slice(&[b, b + 1, b + 3, b, b + 3, b + 2]);
                    }
                }
            }
            Kind::Solid => {
                let faces = solid_faces(match r.solid {
                    Solid::Cube => ModSolid::Cube,
                    Solid::Diamond => ModSolid::Diamond,
                    Solid::Tetra => ModSolid::Tetra,
                });
                let light = normalize([0.35, 0.8, 0.5]);
                for l in looks.iter().take(MAX_SOLIDS) {
                    let axis = tumble_axis(l.id);
                    let (c, sn) = (l.cs[0], l.cs[1]);
                    let half = l.size.abs() * 0.5;
                    let (u0, v1) = frame_uv(l.frame);
                    let uv = [u0 + du * 0.5, v1 - dv * 0.5];
                    for (cs, normal) in &faces {
                        let nw = rotate(*normal, axis, c, sn);
                        let shade = 0.45 + 0.55 * dot(nw, light).max(0.0);
                        let base = mesh.pos.len() as u32;
                        for q in cs {
                            mesh.pos.push(add(l.pos, scale(rotate(*q, axis, c, sn), half)));
                            mesh.nrm.push(nw);
                            mesh.uv.push(uv);
                            mesh.col.push([l.rgb[0] * shade, l.rgb[1] * shade, l.rgb[2] * shade]);
                            mesh.texw.push(l.alpha);
                        }
                        mesh.idx.extend_from_slice(&[base, base + 1, base + 2]);
                    }
                }
            }
        }
        if mesh.pos.is_empty() {
            continue;
        }
        layers.push(FxLayer { emitter: ei, mesh, texture: if r.texture.is_empty() { "soft".to_string() } else { r.texture.clone() }, additive: r.additive, nearest: r.nearest, soft: r.soft });
    }
    layers
}

/// The lights the particles of a system give: for every emitter with a `light`, the `max` brightest of its particles (brightest first, the earlier in the list first when two are as bright).
pub fn lights(sys: &FxSystem) -> Vec<LightData> {
    let mut out = Vec::new();
    for (ei, def) in sys.effect.emitters.iter().enumerate() {
        let Some(l) = def.render.light else { continue };
        let local = def.space == Space::Local;
        let keep = l.max as usize;
        // the few brightest, found in one pass (a list of at most 16, kept in order)
        let mut best: Vec<(f32, u32, V3, V3)> = Vec::with_capacity(keep + 1);
        for p in &sys.em[ei].parts {
            let u = (p.age / p.life).clamp(0.0, 1.0);
            let mut col = p.color;
            if let Some(g) = &def.curves.color {
                let k = g.at(u);
                for n in 0..4 {
                    col[n] *= k[n];
                }
            }
            let value = (col[3] * sys.tint[3] * gradient1(&def.curves.alpha, u) * gradient1(&def.curves.light, u)).clamp(0.0, 1000.0);
            if value <= 0.002 || (best.len() == keep && best[keep - 1].0 >= value) {
                continue;
            }
            let rgb = match l.color {
                Some(c) => c,
                None => {
                    let c = [(col[0] * sys.tint[0]).max(0.0), (col[1] * sys.tint[1]).max(0.0), (col[2] * sys.tint[2]).max(0.0)];
                    let c = [c[0] * c[0], c[1] * c[1], c[2] * c[2]];
                    let m = c[0].max(c[1]).max(c[2]).max(1e-3);
                    [c[0] / m, c[1] / m, c[2] / m]
                }
            };
            let pos = if local { sys.model.transform_point(p.pos) } else { p.pos };
            // (a particle as bright as one that is there already goes after it)
            let at = best.partition_point(|b| b.0 >= value);
            best.insert(at, (value, p.id, pos, rgb));
            best.truncate(keep);
        }
        for (value, _, pos, rgb) in best {
            let k = l.intensity * value;
            out.push(LightData { vec: pos, point: true, color: scale(rgb, k), range: l.range * if local { sys.scale } else { 1.0 }, spot_dir: [0.0, -1.0, 0.0], cos_outer: -2.0, cos_inner: -2.0 });
        }
    }
    out
}
