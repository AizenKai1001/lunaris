//! Particle clouds: tens of thousands of billboards for fire, smoke, sparks, rain, snow, dust and magic, with no entity for any of them. Every particle is a
//! **closed-form function of the cloud's clock**: where it starts, how fast, how it falls and turns, how big and what colour it is at its age all come from a hash
//! of its number, so nothing is stored, nothing is integrated, and the same clock gives the same cloud on every machine and in both renderers (they draw
//! the one mesh of quads built here). The clock (`Particles.time`) is part of the game state and moves with the simulation; set it to scrub or restart.
//!
//! The modules of a `ParticleModules` (gradients over the life, winds, attractors, vortices, colliders, trails, solids) add to that. Forces and colliders make a
//! particle depend on where it has been, so it is stepped from its birth with a fixed step instead: still a pure function of the clock, just dearer.
use crate::math::*;
use crate::mesh::Mesh;
use lunaris_sim::components::{Particles, Transform};
use lunaris_sim::detmath;
use lunaris_sim::noise::noise2;
use lunaris_sim::particle_modules::{Collide, Hit, Modules, Shape, Solid};

/// The most particles in one cloud.
pub const MAX_PARTICLES: u32 = 60_000;
/// The most particles drawn as solids (36 vertices each) in one cloud.
pub const MAX_SOLIDS: usize = 20_000;
/// The most ribbon points in one cloud.
pub const MAX_TRAIL_POINTS: usize = 400_000;

/// A pseudo-random number in [0, 1) for particle `i` of life number `cycle`, the `k`-th thing that is random about it.
fn rnd(seed: u32, i: u32, cycle: u32, k: u32) -> f32 {
    let mut h = (seed as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (i as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ (cycle as u64).wrapping_mul(0x1656_67B1_9E37_79F9) ^ (k as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93);
    h ^= h >> 32;
    h = h.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    h ^= h >> 29;
    h = h.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= h >> 32;
    ((h >> 40) as f32) / 16_777_216.0
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn cos32(x: f32) -> f32 {
    detmath::cos(x as f64) as f32
}

fn sin32(x: f32) -> f32 {
    detmath::sin(x as f64) as f32
}

/// The camera as the particles need it: where it is, where it looks, and its right and up.
pub struct CameraBasis {
    pub pos: V3,
    pub forward: V3,
    pub right: V3,
    pub up: V3,
}

/// One particle at one moment, in the cloud's local space.
struct State {
    pos: V3,
    vel: V3,
    size: f32,
    angle: f32,
    color: [f32; 4],
    frame: u32,
    /// the last positions along the path, newest first (empty without a `trail` module): the cloud's space, then the world's
    trail: Vec<V3>,
    /// the axis a solid particle tumbles about
    axis: V3,
}

fn finite(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

/// The position and velocity at `age` of a particle that starts at `start` with velocity `v0`, by the closed form (gravity `g`, drag `k`).
fn closed_form(start: V3, v0: V3, g: V3, k: f32, age: f32) -> (V3, V3) {
    if k > 1e-4 {
        let e = detmath::exp(-(k * age) as f64) as f32;
        let a = (1.0 - e) / k;
        (add(add(start, scale(v0, a)), scale(g, (age - a) / k)), add(scale(v0, e), scale(g, (1.0 - e) / k)))
    } else {
        (add(add(start, scale(v0, age)), scale(g, 0.5 * age * age)), add(v0, scale(g, age)))
    }
}

/// What a collider did to a particle that may have reached it.
enum Outcome {
    Free,
    Moved,
    Stuck,
    Dead,
}

fn collide(c: &Collide, pos: &mut V3, vel: &mut V3) -> Outcome {
    // the surface's normal pointing out to where the particle may be, and how far into the solid side it has got (positive = inside)
    let (normal, depth) = match &c.shape {
        Shape::Plane { normal, d } => (*normal, d - dot(*normal, *pos)),
        Shape::Sphere { pos: centre, r, inside } => {
            let off = sub(*pos, *centre);
            let len = length(off);
            if len < 1e-6 {
                return Outcome::Free;
            }
            let out = scale(off, 1.0 / len);
            if *inside {
                (scale(out, -1.0), len - r)
            } else {
                (out, r - len)
            }
        }
    };
    if depth <= 0.0 {
        return Outcome::Free;
    }
    match c.hit {
        Hit::Kill => return Outcome::Dead,
        Hit::Stick => {
            *pos = add(*pos, scale(normal, depth));
            *vel = [0.0; 3];
            return Outcome::Stuck;
        }
        Hit::Bounce => {}
    }
    *pos = add(*pos, scale(normal, depth));
    let vn = dot(*vel, normal);
    if vn < 0.0 {
        let along = scale(normal, vn);
        let tangent = sub(*vel, along);
        *vel = sub(scale(tangent, 1.0 - c.friction), scale(along, c.bounce));
    }
    Outcome::Moved
}

/// Where a particle is now and at each of `samples` (ages, newest first), found by stepping it from its birth with a fixed step; `None` when a collider killed it.
#[allow(clippy::too_many_arguments)]
fn integrate(m: &Modules, start: V3, v0: V3, g: V3, k: f32, clock: f32, age: f32, life: f32, samples: &[f32], seed: u32) -> Option<(V3, V3, Vec<V3>)> {
    let h = (1.0f32 / 30.0).max(life / 48.0);
    let full = (age / h).floor() as usize;
    let (mut pos, mut vel) = (start, v0);
    let mut path: Vec<(f32, V3)> = Vec::with_capacity(full + 2);
    path.push((0.0, pos));
    let mut t = 0.0f32;
    let decay_full = detmath::exp(-(k * h) as f64) as f32;
    'steps: for s in 0..=full {
        let dt = if s < full { h } else { age - full as f32 * h };
        if dt <= 1e-7 {
            continue;
        }
        let acc = add(g, m.acceleration(pos, clock + t, seed));
        let before = vel;
        vel = add(vel, scale(acc, dt));
        if k > 1e-4 {
            vel = scale(vel, if s < full { decay_full } else { detmath::exp(-(k * dt) as f64) as f32 });
        }
        // (the trapezoid rule: a steady push is followed exactly)
        pos = add(pos, scale(add(before, vel), 0.5 * dt));
        t += dt;
        for c in &m.colliders {
            match collide(c, &mut pos, &mut vel) {
                Outcome::Dead => return None,
                Outcome::Stuck => {
                    path.push((age, pos));
                    break 'steps;
                }
                Outcome::Free | Outcome::Moved => {}
            }
        }
        path.push((t, pos));
    }
    // the trail: the recorded path read at the sample ages (straight between two steps)
    let at = |a: f32| -> V3 {
        if a <= 0.0 {
            return path[0].1;
        }
        match path.iter().position(|(x, _)| *x >= a) {
            Some(0) | None => path[path.len() - 1].1,
            Some(j) => {
                let ((a0, p0), (a1, p1)) = (path[j - 1], path[j]);
                let w = if a1 > a0 { (a - a0) / (a1 - a0) } else { 1.0 };
                add(p0, scale(sub(p1, p0), w))
            }
        }
    };
    Some((pos, vel, samples.iter().map(|a| at(*a)).collect()))
}

fn particle(p: &Particles, m: &Modules, i: u32, count: u32, time: f32) -> Option<State> {
    let life = finite(p.lifetime, 2.0).clamp(0.05, 600.0);
    let phase = i as f32 / count as f32;
    let (age, cycle) = if p.looping {
        let c = time / life - phase;
        if !p.prewarm && c < 0.0 {
            return None;
        }
        let whole = c.floor();
        ((c - whole) * life, whole as i64 as u32)
    } else {
        (time - phase * finite(p.spread_time, 0.0).max(0.0), 0)
    };
    if !(0.0..life).contains(&age) {
        return None;
    }
    let u = age / life;
    let r = |k: u32| rnd(p.seed, i, cycle, k);
    // where it starts
    let ext = [finite(p.extent[0], 0.5), finite(p.extent[1], 0.5), finite(p.extent[2], 0.5)];
    let start = match p.shape {
        1 => {
            let (c, a) = (1.0 - 2.0 * r(1), r(2) * std::f32::consts::TAU);
            let s = (1.0 - c * c).max(0.0).sqrt();
            let radius = ext[0] * r(3).sqrt();
            [radius * s * cos32(a), radius * c, radius * s * sin32(a)]
        }
        2 => [(r(1) * 2.0 - 1.0) * ext[0], (r(2) * 2.0 - 1.0) * ext[1], (r(3) * 2.0 - 1.0) * ext[2]],
        3 => {
            let (a, radius) = (r(1) * std::f32::consts::TAU, ext[0] * r(2).sqrt());
            [radius * cos32(a), 0.0, radius * sin32(a)]
        }
        4 => {
            let a = r(1) * std::f32::consts::TAU;
            [ext[0] * cos32(a), 0.0, ext[0] * sin32(a)]
        }
        _ => [0.0; 3],
    };
    // which way and how fast: a direction inside a cone round `direction`
    let axis = {
        let d = [finite(p.direction[0], 0.0), finite(p.direction[1], 1.0), finite(p.direction[2], 0.0)];
        if length(d) < 1e-6 { [0.0, 1.0, 0.0] } else { normalize(d) }
    };
    let cone = (finite(p.cone, 25.0).clamp(0.0, 180.0) as f64).to_radians();
    let cos_cone = detmath::cos(cone) as f32;
    let c = 1.0 - r(4) * (1.0 - cos_cone);
    let s = (1.0 - c * c).max(0.0).sqrt();
    let phi = r(5) * std::f32::consts::TAU;
    let t1 = {
        let h = if axis[0].abs() < 0.6 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        normalize(cross(axis, h))
    };
    let t2 = cross(axis, t1);
    let dir = add(scale(axis, c), scale(add(scale(t1, cos32(phi)), scale(t2, sin32(phi))), s));
    let speed = lerp(finite(p.speed_range[0], 1.0), finite(p.speed_range[1], 3.0), r(6));
    let v0 = scale(dir, speed);
    let g = [finite(p.gravity[0], 0.0), finite(p.gravity[1], 0.0), finite(p.gravity[2], 0.0)];
    let k = finite(p.drag, 0.0).max(0.0);
    // the ages of the trail's points, newest first
    let samples: Vec<f32> = match m.trail {
        Some(t) => (0..=t.segments).map(|j| (age - t.time * j as f32 / t.segments as f32).max(0.0)).collect(),
        None => Vec::new(),
    };
    let (mut pos, vel, mut trail) = if m.dynamic() {
        integrate(m, start, v0, g, k, time - age, age, life, &samples, p.seed)?
    } else {
        let (pos, vel) = closed_form(start, v0, g, k, age);
        (pos, vel, samples.iter().map(|a| closed_form(start, v0, g, k, *a).0).collect())
    };
    let amp = finite(p.turbulence, 0.0);
    if amp != 0.0 {
        let sc = finite(p.turbulence_scale, 1.0) as f64;
        let n = i as f64 * 7.13 + cycle as f64 * 13.7;
        let seed = p.seed as f64 * 3.0;
        let offset = |age: f32| {
            let (a, grow) = (age as f64 * sc, ((age / life) * 2.0).min(1.0));
            scale([noise2(a, n, seed) as f32, noise2(a, n, seed + 1.0) as f32, noise2(a, n, seed + 2.0) as f32], amp * grow)
        };
        pos = add(pos, offset(age));
        for (q, a) in trail.iter_mut().zip(&samples) {
            *q = add(*q, offset(*a));
        }
    }
    let jitter = finite(p.size_jitter, 0.0).clamp(0.0, 1.0);
    let base = match &m.size {
        Some(g) => g.at(u)[0],
        None => lerp(finite(p.size[0], 0.2), finite(p.size[1], 0.05), u),
    };
    let size = base * (1.0 + (r(7) * 2.0 - 1.0) * jitter);
    let fade = if p.fade_in > 0.0 { (u / p.fade_in).min(1.0) } else { 1.0 };
    let mut color = [0.0; 4];
    match &m.color {
        Some(g) => color = g.at(u),
        None => {
            for (n, out) in color.iter_mut().enumerate() {
                *out = lerp(finite(p.color[n], 1.0), finite(p.color_end[n], 1.0), u);
            }
        }
    }
    color[3] *= fade;
    let spin = finite(p.spin, 0.0);
    // (a solid always has some turn, so that no two lie alike)
    let angle = if spin == 0.0 && m.solid.is_none() { 0.0 } else { r(8) * std::f32::consts::TAU + spin * age * if r(9) < 0.5 { -1.0 } else { 1.0 } };
    let frames = (p.sheet[0].clamp(1, 64) * p.sheet[1].clamp(1, 64)).max(1);
    let frame = ((u * frames as f32) as u32).min(frames - 1);
    let tumble = if m.solid.is_some() { normalize([r(10) - 0.5 + 1e-3, r(11) - 0.5, r(12) - 0.5]) } else { [0.0, 1.0, 0.0] };
    Some(State { pos, vel, size, angle, color, frame, trail, axis: tumble })
}

/// The faces of a solid of half size 1: triangles with their corners and the face's normal.
fn solid_faces(kind: Solid) -> Vec<([V3; 3], V3)> {
    let tri = |a: V3, b: V3, c: V3| -> ([V3; 3], V3) { ([a, b, c], normalize(cross(sub(b, a), sub(c, a)))) };
    match kind {
        Solid::Cube => {
            let v = |x: f32, y: f32, z: f32| [x, y, z];
            let quads: [[V3; 4]; 6] = [
                [v(1.0, -1.0, -1.0), v(1.0, 1.0, -1.0), v(1.0, 1.0, 1.0), v(1.0, -1.0, 1.0)],
                [v(-1.0, -1.0, 1.0), v(-1.0, 1.0, 1.0), v(-1.0, 1.0, -1.0), v(-1.0, -1.0, -1.0)],
                [v(-1.0, 1.0, -1.0), v(-1.0, 1.0, 1.0), v(1.0, 1.0, 1.0), v(1.0, 1.0, -1.0)],
                [v(-1.0, -1.0, 1.0), v(-1.0, -1.0, -1.0), v(1.0, -1.0, -1.0), v(1.0, -1.0, 1.0)],
                [v(-1.0, -1.0, 1.0), v(1.0, -1.0, 1.0), v(1.0, 1.0, 1.0), v(-1.0, 1.0, 1.0)],
                [v(1.0, -1.0, -1.0), v(-1.0, -1.0, -1.0), v(-1.0, 1.0, -1.0), v(1.0, 1.0, -1.0)],
            ];
            quads.iter().flat_map(|q| [tri(q[0], q[1], q[2]), tri(q[0], q[2], q[3])]).collect()
        }
        Solid::Diamond => {
            let (px, nx, py, ny, pz, nz) = ([1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]);
            vec![tri(py, pz, px), tri(py, nx, pz), tri(py, nz, nx), tri(py, px, nz), tri(ny, px, pz), tri(ny, pz, nx), tri(ny, nx, nz), tri(ny, nz, px)]
        }
        Solid::Tetra => {
            let (a, b, c, d) = ([1.0, 1.0, 1.0], [1.0, -1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, -1.0, 1.0]);
            vec![tri(a, c, b), tri(a, b, d), tri(a, d, c), tri(b, c, d)]
        }
    }
}

/// `v` turned by `angle` about the unit axis `k` (Rodrigues' formula).
fn rotate(v: V3, k: V3, cos: f32, sin: f32) -> V3 {
    add(add(scale(v, cos), scale(cross(k, v), sin)), scale(k, dot(k, v) * (1.0 - cos)))
}

/// The quads of a cloud as one mesh in world space: four vertices a particle, its colour in the vertex colour and its opacity in the texture weight (see
/// `Shade::Particle` and the GPU's `fs_particle`). Alpha-blended clouds are sorted far to near (a stable sort: the same for the same view); additive ones are
/// drawn in particle order, which is the same light however it is added up. A `trail` module adds a ribbon behind each particle and a `mesh` module draws solids
/// (36 vertices or fewer a particle, at most `MAX_SOLIDS`) in place of the pictures.
///
/// With `threads` above 1 the particles are worked out in that many slices at once and joined in order, so the mesh is the same whatever the number.
pub fn build_with(p: &Particles, m: &Modules, tr: &Transform, cam: &CameraBasis, threads: usize) -> Mesh {
    let count = p.count.clamp(1, MAX_PARTICLES);
    let model = M4::from_trs(tr.pos, tr.rot, tr.scale);
    let time = finite(p.time, 0.0);
    let slice = |range: std::ops::Range<u32>| -> Vec<(f32, State)> {
        let mut out = Vec::new();
        for i in range {
            if let Some(s) = particle(p, m, i, count, time) {
                let world = model.transform_point(s.pos);
                let depth = dot(sub(world, cam.pos), cam.forward);
                if depth > 0.01 && s.color[3] > 0.0 && s.size.abs() > 1e-6 {
                    let trail = s.trail.iter().map(|q| model.transform_point(*q)).collect();
                    out.push((depth, State { pos: world, vel: model.transform_dir(s.vel), trail, ..s }));
                }
            }
        }
        out
    };
    let parts = threads.clamp(1, 16).min((count / 4096).max(1) as usize) as u32;
    let mut alive: Vec<(f32, State)> = if parts <= 1 {
        slice(0..count)
    } else {
        let per = count.div_ceil(parts);
        let slice = &slice;
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..parts).map(|k| s.spawn(move || slice(k * per..((k + 1) * per).min(count)))).collect();
            handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        })
    };
    if !p.additive {
        alive.sort_by(|a, b| b.0.total_cmp(&a.0));
    }
    let (cols, rows) = (p.sheet[0].clamp(1, 64), p.sheet[1].clamp(1, 64));
    let stretch = finite(p.stretch, 0.0).max(0.0);
    if m.solid.is_none() && m.trail.is_none() {
        // the common case: plain pictures, filled in parallel
        return quads(&alive, cam, cols, rows, stretch, threads);
    }
    let mut mesh = Mesh { pos: Vec::with_capacity(alive.len() * 4), nrm: Vec::new(), uv: Vec::new(), col: Vec::new(), texw: Vec::new(), idx: Vec::with_capacity(alive.len() * 6), tex: None, rig: None };
    let faces = m.solid.map(solid_faces);
    if faces.is_some() {
        alive.truncate(MAX_SOLIDS);
    }
    let ribbon_points = m.trail.map_or(0, |t| t.segments + 1);
    if ribbon_points > 0 {
        alive.truncate((MAX_TRAIL_POINTS / ribbon_points).max(1));
    }
    let light = normalize([0.35, 0.8, 0.5]);
    for (_, s) in &alive {
        let (fx, fy) = (s.frame % cols, s.frame / cols);
        let (u0, v1) = (fx as f32 / cols as f32, 1.0 - fy as f32 / rows as f32);
        let (du, dv) = (1.0 / cols as f32, 1.0 / rows as f32);
        let rgb = [s.color[0].max(0.0) * s.color[0].max(0.0), s.color[1].max(0.0) * s.color[1].max(0.0), s.color[2].max(0.0) * s.color[2].max(0.0)];
        let alpha = s.color[3].clamp(0.0, 1.0);
        // the ribbon behind it: each point is two vertices across the way it has been going, thinning and fading towards the tail
        if let (Some(t), true) = (m.trail, s.trail.len() >= 2) {
            let n = s.trail.len();
            let base = mesh.pos.len() as u32;
            for (j, q) in s.trail.iter().enumerate() {
                let way = if j + 1 < n { sub(*q, s.trail[j + 1]) } else { sub(s.trail[j - 1], *q) };
                let side = cross(way, cam.forward);
                let side = if length(side) < 1e-7 { cam.right } else { normalize(side) };
                let along = j as f32 / (n - 1) as f32;
                let half = s.size.abs() * 0.5 * t.width * (1.0 - along);
                let fade = (1.0 - along) * (1.0 - along);
                for (sx, u) in [(-1.0f32, u0), (1.0, u0 + du)] {
                    mesh.pos.push(add(*q, scale(side, sx * half)));
                    mesh.nrm.push(scale(cam.forward, -1.0));
                    mesh.uv.push([u, v1 - dv * 0.5]);
                    mesh.col.push(rgb);
                    mesh.texw.push(alpha * fade);
                }
            }
            for j in 0..(n as u32 - 1) {
                let b = base + j * 2;
                mesh.idx.extend_from_slice(&[b, b + 1, b + 3, b, b + 3, b + 2]);
            }
        }
        // the head: a solid tumbling about its own axis, or a picture facing the camera
        if let Some(faces) = &faces {
            let (c, sn) = (cos32(s.angle), sin32(s.angle));
            let half = s.size.abs() * 0.5;
            let uv = [u0 + du * 0.5, v1 - dv * 0.5];
            for (corners, normal) in faces {
                let nw = rotate(*normal, s.axis, c, sn);
                let shade = 0.45 + 0.55 * dot(nw, light).max(0.0);
                let base = mesh.pos.len() as u32;
                for q in corners {
                    mesh.pos.push(add(s.pos, scale(rotate(*q, s.axis, c, sn), half)));
                    mesh.nrm.push(nw);
                    mesh.uv.push(uv);
                    mesh.col.push([rgb[0] * shade, rgb[1] * shade, rgb[2] * shade]);
                    mesh.texw.push(alpha);
                }
                mesh.idx.extend_from_slice(&[base, base + 1, base + 2]);
            }
            continue;
        }
        let base = mesh.pos.len() as u32;
        let corners = billboard(s, cam, stretch);
        for (q, (u, v)) in corners.iter().zip(quad_uv(u0, v1, du, dv)) {
            mesh.pos.push(*q);
            mesh.nrm.push(scale(cam.forward, -1.0));
            mesh.uv.push([u, v]);
            mesh.col.push(rgb);
            mesh.texw.push(alpha);
        }
        mesh.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    mesh
}

/// The corners of a picture's quad in the order (-1,-1), (1,-1), (1,1), (-1,1): the camera's right and up turned by the particle's angle, or along its velocity
/// when it is stretched.
fn billboard(s: &State, cam: &CameraBasis, stretch: f32) -> [V3; 4] {
    let half = s.size.abs() * 0.5;
    let (rx, ry, hx, hy) = {
        let speed = length(s.vel);
        if stretch > 0.0 && speed > 1e-4 {
            let along = scale(s.vel, 1.0 / speed);
            let across = cross(along, cam.forward);
            let across = if length(across) < 1e-5 { cam.right } else { normalize(across) };
            (along, across, half + speed * stretch * 0.5, half)
        } else {
            // (no spin: the angle is exactly 0, whose cosine and sine are exactly 1 and 0)
            let (c, sn) = if s.angle == 0.0 { (1.0, 0.0) } else { (cos32(s.angle), sin32(s.angle)) };
            (add(scale(cam.right, c), scale(cam.up, sn)), sub(scale(cam.up, c), scale(cam.right, sn)), half, half)
        }
    };
    [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(sx, sy)| add(s.pos, add(scale(rx, sx * hx), scale(ry, sy * hy))))
}

/// The texture coordinates of the four corners of the frame whose bottom left is `(u0, v1 - dv)`.
fn quad_uv(u0: f32, v1: f32, du: f32, dv: f32) -> [(f32, f32); 4] {
    [(u0, v1 - dv), (u0 + du, v1 - dv), (u0 + du, v1), (u0, v1)]
}

/// The mesh of a cloud of plain pictures (no ribbons, no solids), filled by `threads` slices at once: every particle owns four vertices and six indices at a
/// place that only its position in the list decides, so the result is the same whatever the number of threads.
fn quads(alive: &[(f32, State)], cam: &CameraBasis, cols: u32, rows: u32, stretch: f32, threads: usize) -> Mesh {
    let n = alive.len();
    let mut mesh = Mesh { pos: vec![[0.0; 3]; n * 4], nrm: vec![scale(cam.forward, -1.0); n * 4], uv: vec![[0.0; 2]; n * 4], col: vec![[0.0; 3]; n * 4], texw: vec![0.0; n * 4], idx: vec![0; n * 6], tex: None, rig: None };
    let per = n.div_ceil(threads.clamp(1, 16).min((n / 4096).max(1))).max(1);
    let (du, dv) = (1.0 / cols as f32, 1.0 / rows as f32);
    let fill = |first: usize, items: &[(f32, State)], pos: &mut [V3], uv: &mut [[f32; 2]], col: &mut [V3], texw: &mut [f32], idx: &mut [u32]| {
        for (k, (_, s)) in items.iter().enumerate() {
            let (fx, fy) = (s.frame % cols, s.frame / cols);
            let (u0, v1) = (fx as f32 / cols as f32, 1.0 - fy as f32 / rows as f32);
            let rgb = [s.color[0].max(0.0) * s.color[0].max(0.0), s.color[1].max(0.0) * s.color[1].max(0.0), s.color[2].max(0.0) * s.color[2].max(0.0)];
            let alpha = s.color[3].clamp(0.0, 1.0);
            let corners = billboard(s, cam, stretch);
            let coords = quad_uv(u0, v1, du, dv);
            for j in 0..4 {
                pos[k * 4 + j] = corners[j];
                uv[k * 4 + j] = [coords[j].0, coords[j].1];
                col[k * 4 + j] = rgb;
                texw[k * 4 + j] = alpha;
            }
            let base = ((first + k) * 4) as u32;
            idx[k * 6..k * 6 + 6].copy_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    };
    let Mesh { pos, uv, col, texw, idx, .. } = &mut mesh;
    if per >= n {
        fill(0, alive, pos, uv, col, texw, idx);
    } else {
        let fill = &fill;
        std::thread::scope(|sc| {
            let parts = alive.chunks(per).zip(pos.chunks_mut(per * 4)).zip(uv.chunks_mut(per * 4)).zip(col.chunks_mut(per * 4)).zip(texw.chunks_mut(per * 4)).zip(idx.chunks_mut(per * 6));
            for (k, (((((items, pos), uv), col), texw), idx)) in parts.enumerate() {
                sc.spawn(move || fill(k * per, items, pos, uv, col, texw, idx));
            }
        });
    }
    mesh
}

/// A cloud without modules: the same as `build_with` and the default (empty) `Modules`.
pub fn build(p: &Particles, tr: &Transform, cam: &CameraBasis, threads: usize) -> Mesh {
    build_with(p, &Modules::default(), tr, cam, threads)
}
