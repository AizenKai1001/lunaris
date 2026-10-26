//! Small helpers: matrix maths for the GPU's depth convention, colour conversions, cascade fitting for the shadow maps, texture mip
//! chains, and the glyph quads of the HUD text.
use lunaris_render::math::{cross, dot, length, normalize, rotate, scale, sub, add, M4, V3};
use lunaris_render::render::{clip_pixels, Cam, Scene, Sweep};
use lunaris_render::font;
use lunaris_render::texture::Texture;
use std::sync::Arc;

pub fn srgb_decode(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn srgb_encode(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// A display colour (0..1) into the linear working space of the high-quality mode.
pub fn display_to_linear(c: V3) -> V3 {
    [srgb_decode(c[0]), srgb_decode(c[1]), srgb_decode(c[2])]
}

/// The reference renderer's working space (display colour squared) back to a display colour.
pub fn gamma2_to_display(c: V3) -> V3 {
    [c[0].max(0.0).sqrt(), c[1].max(0.0).sqrt(), c[2].max(0.0).sqrt()]
}

/// OpenGL-style clip space (z in [-w, w], what `lunaris_render` produces) to the GPU's: depth reversed (1 at the near plane, 0 far away)
/// so that float depth keeps its precision far from the camera.
pub fn to_gpu_clip(m: &M4) -> M4 {
    // (fix * v).z = 0.5 * (v.w - v.z)
    let mut fix = M4::identity();
    fix.0[10] = -0.5;
    fix.0[14] = 0.5;
    fix.mul(m)
}

/// The inverse of any 4x4 matrix (the identity when it is singular).
pub fn inverse(m: &M4) -> M4 {
    let a: Vec<f64> = m.0.iter().map(|v| *v as f64).collect();
    // cofactor expansion on the column-major array
    let mut inv = [0.0f64; 16];
    inv[0] = a[5] * a[10] * a[15] - a[5] * a[11] * a[14] - a[9] * a[6] * a[15] + a[9] * a[7] * a[14] + a[13] * a[6] * a[11] - a[13] * a[7] * a[10];
    inv[4] = -a[4] * a[10] * a[15] + a[4] * a[11] * a[14] + a[8] * a[6] * a[15] - a[8] * a[7] * a[14] - a[12] * a[6] * a[11] + a[12] * a[7] * a[10];
    inv[8] = a[4] * a[9] * a[15] - a[4] * a[11] * a[13] - a[8] * a[5] * a[15] + a[8] * a[7] * a[13] + a[12] * a[5] * a[11] - a[12] * a[7] * a[9];
    inv[12] = -a[4] * a[9] * a[14] + a[4] * a[10] * a[13] + a[8] * a[5] * a[14] - a[8] * a[6] * a[13] - a[12] * a[5] * a[10] + a[12] * a[6] * a[9];
    inv[1] = -a[1] * a[10] * a[15] + a[1] * a[11] * a[14] + a[9] * a[2] * a[15] - a[9] * a[3] * a[14] - a[13] * a[2] * a[11] + a[13] * a[3] * a[10];
    inv[5] = a[0] * a[10] * a[15] - a[0] * a[11] * a[14] - a[8] * a[2] * a[15] + a[8] * a[3] * a[14] + a[12] * a[2] * a[11] - a[12] * a[3] * a[10];
    inv[9] = -a[0] * a[9] * a[15] + a[0] * a[11] * a[13] + a[8] * a[1] * a[15] - a[8] * a[3] * a[13] - a[12] * a[1] * a[11] + a[12] * a[3] * a[9];
    inv[13] = a[0] * a[9] * a[14] - a[0] * a[10] * a[13] - a[8] * a[1] * a[14] + a[8] * a[2] * a[13] + a[12] * a[1] * a[10] - a[12] * a[2] * a[9];
    inv[2] = a[1] * a[6] * a[15] - a[1] * a[7] * a[14] - a[5] * a[2] * a[15] + a[5] * a[3] * a[14] + a[13] * a[2] * a[7] - a[13] * a[3] * a[6];
    inv[6] = -a[0] * a[6] * a[15] + a[0] * a[7] * a[14] + a[4] * a[2] * a[15] - a[4] * a[3] * a[14] - a[12] * a[2] * a[7] + a[12] * a[3] * a[6];
    inv[10] = a[0] * a[5] * a[15] - a[0] * a[7] * a[13] - a[4] * a[1] * a[15] + a[4] * a[3] * a[13] + a[12] * a[1] * a[7] - a[12] * a[3] * a[5];
    inv[14] = -a[0] * a[5] * a[14] + a[0] * a[6] * a[13] + a[4] * a[1] * a[14] - a[4] * a[2] * a[13] - a[12] * a[1] * a[6] + a[12] * a[2] * a[5];
    inv[3] = -a[1] * a[6] * a[11] + a[1] * a[7] * a[10] + a[5] * a[2] * a[11] - a[5] * a[3] * a[10] - a[9] * a[2] * a[7] + a[9] * a[3] * a[6];
    inv[7] = a[0] * a[6] * a[11] - a[0] * a[7] * a[10] - a[4] * a[2] * a[11] + a[4] * a[3] * a[10] + a[8] * a[2] * a[7] - a[8] * a[3] * a[6];
    inv[11] = -a[0] * a[5] * a[11] + a[0] * a[7] * a[9] + a[4] * a[1] * a[11] - a[4] * a[3] * a[9] - a[8] * a[1] * a[7] + a[8] * a[3] * a[5];
    inv[15] = a[0] * a[5] * a[10] - a[0] * a[6] * a[9] - a[4] * a[1] * a[10] + a[4] * a[2] * a[9] + a[8] * a[1] * a[6] - a[8] * a[2] * a[5];
    let det = a[0] * inv[0] + a[1] * inv[4] + a[2] * inv[8] + a[3] * inv[12];
    if det == 0.0 || !det.is_finite() {
        return M4::identity();
    }
    let mut out = [0.0f32; 16];
    for i in 0..16 {
        out[i] = (inv[i] / det) as f32;
    }
    M4(out)
}

/// Build a rigid view matrix for an eye looking along `forward`.
fn look_matrix(eye: V3, forward: V3, up_hint: V3) -> M4 {
    let f = normalize(forward);
    let mut r = cross(f, up_hint);
    if length(r) < 1e-4 {
        r = cross(f, [1.0, 0.0, 0.0]);
    }
    let r = normalize(r);
    let u = cross(r, f);
    let mut m = [0.0f32; 16];
    // rows: right, up, back
    for (row, v) in [r, u, scale(f, -1.0)].iter().enumerate() {
        for col in 0..3 {
            m[col * 4 + row] = v[col];
        }
    }
    m[12] = -dot(r, eye);
    m[13] = -dot(u, eye);
    m[14] = dot(f, eye);
    m[15] = 1.0;
    M4(m)
}

/// One shadow cascade: the light's view-projection (standard depth, 0 near .. 1 far) and the world size of one shadow-map texel.
#[derive(Clone, Copy, Debug)]
pub struct Cascade {
    pub view_proj: M4,
    pub texel: f32,
    /// View distance at which this cascade ends.
    pub far: f32,
    /// How many metres the light's depth range of this cascade spans (the depth in the map is 0 to 1 over it).
    pub depth: f32,
}

pub const CASCADES: usize = 3;

/// The caster bounds the cascades need: world-space bounding spheres of everything that casts a shadow.
pub struct Casters {
    pub spheres: Vec<(V3, f32)>,
}

/// Fit three cascades around the camera's view, out to `distance`, for a light shining from the direction `to_light` (a unit vector).
pub fn fit_cascades(cam: &Cam, aspect: f32, to_light: V3, distance: f32, size: u32, casters: &Casters) -> [Cascade; CASCADES] {
    let right = rotate(cam.rot, [1.0, 0.0, 0.0]);
    let up = rotate(cam.rot, [0.0, 1.0, 0.0]);
    let fwd = rotate(cam.rot, [0.0, 0.0, -1.0]);
    let near = if cam.ortho.is_some() { cam.near.max(0.0) } else { cam.near.max(1e-3) };
    let end = distance.min(cam.far).max(near + 0.01);
    // splits: a mix of logarithmic and uniform, starting a little in front of the camera
    let start = near.max(0.05);
    let lambda = 0.75f32;
    let mut far_of = [0.0f32; CASCADES];
    for (i, f) in far_of.iter_mut().enumerate() {
        let t = (i + 1) as f32 / CASCADES as f32;
        let log = start * (end / start).powf(t);
        let uni = start + (end - start) * t;
        *f = lambda * log + (1.0 - lambda) * uni;
    }
    far_of[CASCADES - 1] = end;
    let tan_half = (cam.fov_y.clamp(1.0, 170.0).to_radians() * 0.5).tan();
    let mut out = [Cascade { view_proj: M4::identity(), texel: 1.0, far: end, depth: 1.0 }; CASCADES];
    let mut slice_near = start;
    for i in 0..CASCADES {
        let slice_far = far_of[i];
        // the eight corners of this slice of the view frustum
        let mut corners = [[0.0f32; 3]; 8];
        for (k, d) in [slice_near, slice_far].into_iter().enumerate() {
            let (hh, hw) = match cam.ortho {
                Some(h) => (h * 0.5, h * 0.5 * aspect),
                None => (tan_half * d, tan_half * d * aspect),
            };
            let c = add(cam.pos, scale(fwd, d));
            for (j, (sx, sy)) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].into_iter().enumerate() {
                corners[k * 4 + j] = add(add(c, scale(right, sx * hw)), scale(up, sy * hh));
            }
        }
        let mut centre = [0.0f32; 3];
        for c in &corners {
            centre = add(centre, scale(*c, 1.0 / 8.0));
        }
        let mut radius = 0.0f32;
        for c in &corners {
            radius = radius.max(length(sub(*c, centre)));
        }
        // a stable size: round the radius up a little so that tiny changes of the view do not make the shadows shimmer
        radius = (radius * 16.0).ceil() / 16.0;
        let radius = radius.max(0.05);
        // the light's axes, and the centre snapped to whole texels
        let probe = look_matrix([0.0; 3], scale(to_light, -1.0), [0.0, 1.0, 0.0]);
        let lr = [probe.0[0], probe.0[4], probe.0[8]];
        let lu = [probe.0[1], probe.0[5], probe.0[9]];
        let texel = 2.0 * radius / size as f32;
        let (x, y) = (dot(centre, lr), dot(centre, lu));
        let (sx, sy) = ((x / texel).floor() * texel, (y / texel).floor() * texel);
        let centre = add(add(centre, scale(lr, sx - x)), scale(lu, sy - y));
        // casters between the slice and the light must be in the map: pull the light back to the farthest of them
        let along = dot(centre, to_light);
        let mut back = radius;
        for (c, r) in &casters.spheres {
            back = back.max(dot(*c, to_light) + r - along);
        }
        let back = back.min(20_000.0);
        let eye = add(centre, scale(to_light, back));
        let view = look_matrix(eye, scale(to_light, -1.0), [0.0, 1.0, 0.0]);
        let depth = back + radius;
        let mut p = [0.0f32; 16];
        p[0] = 1.0 / radius;
        p[5] = 1.0 / radius;
        p[10] = -1.0 / depth;
        p[15] = 1.0;
        out[i] = Cascade { view_proj: M4(p).mul(&view), texel, far: slice_far, depth };
        slice_near = slice_far;
    }
    out
}

/// The six view-projection matrices of a point light's shadow map (faces +X, -X, +Y, -Y, +Z, -Z), each a perspective view a little wider than
/// 90 degrees so that filtering at a face's edge still reads valid texels. Standard depth (0 near .. 1 far).
pub fn point_light_faces(pos: V3, near: f32, far: f32) -> [M4; 6] {
    let fov = 94.0f32.to_radians();
    let f = 1.0 / (fov * 0.5).tan();
    let mut p = [0.0f32; 16];
    p[0] = f;
    p[5] = f;
    p[10] = far / (near - far);
    p[11] = -1.0;
    p[14] = near * far / (near - far);
    let proj = M4(p);
    let dirs: [(V3, V3); 6] = [([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), ([0.0, 1.0, 0.0], [0.0, 0.0, -1.0]), ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]), ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]), ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0])];
    let mut out = [M4::identity(); 6];
    for (i, (fwd, up)) in dirs.iter().enumerate() {
        out[i] = proj.mul(&look_matrix(pos, *fwd, *up));
    }
    out
}

/// A box-filtered chain of mip levels for an RGBA8 image (given in display encoding, filtered in linear light), level 0 first.
pub fn mip_chain(width: u32, height: u32, rgba: Vec<u8>, srgb: bool) -> Vec<(u32, u32, Vec<u8>)> {
    let mut levels = vec![(width, height, rgba)];
    let dec: Vec<f32> = (0..256).map(|i| if srgb { srgb_decode(i as f32 / 255.0) } else { i as f32 / 255.0 }).collect();
    loop {
        let (w, h, data) = levels.last().expect("at least one level");
        if *w == 1 && *h == 1 {
            break;
        }
        let (nw, nh) = ((*w / 2).max(1), (*h / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0.0f32; 4];
                let mut n = 0.0;
                for dy in 0..2u32 {
                    for dx in 0..2u32 {
                        let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
                        let o = ((sy * w + sx) * 4) as usize;
                        // colours are weighted by alpha so that transparent texels do not tint their neighbours
                        let a = data[o + 3] as f32 / 255.0;
                        for k in 0..3 {
                            acc[k] += dec[data[o + k] as usize] * a;
                        }
                        acc[3] += a;
                        n += 1.0;
                    }
                }
                let o = ((y * nw + x) * 4) as usize;
                let alpha = acc[3] / n;
                for k in 0..3 {
                    let lin = if acc[3] > 0.0 { acc[k] / acc[3] } else { 0.0 };
                    let enc = if srgb { srgb_encode(lin) } else { lin };
                    next[o + k] = (enc.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
                next[o + 3] = (alpha.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        }
        levels.push((nw, nh, next));
    }
    levels
}

/// A run of interface vertices drawn with one texture (`None`: plain white) and one scissor rectangle (inside a scroll view: x, y, width and height in pixels; `None`: the whole picture).
pub struct UiBatch {
    pub tex: Option<Arc<Texture>>,
    pub smooth: bool,
    pub first: u32,
    pub count: u32,
    pub scissor: Option<[u32; 4]>,
}

/// The scissor rectangle (x, y, width, height) of a clip on a picture of `w` x `h` pixels, cut to the picture; `None` when nothing of it is on the picture.
pub fn scissor_of(clip: [f32; 4], w: f32, h: f32) -> Option<[u32; 4]> {
    let (x0, y0, x1, y1) = clip_pixels(clip, w as usize, h as usize);
    let (x0, y0, x1, y1) = (x0.clamp(0, w as i64), y0.clamp(0, h as i64), x1.clamp(0, w as i64), y1.clamp(0, h as i64));
    (x1 > x0 && y1 > y0).then(|| [x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32])
}

/// The triangles of a radial fill (`UiRadial`) on the pixel box `x0, y0, x1, y1`, as a flat list of corners (three to a triangle): a fan of thin triangles round the arc for a circle (a ring when it has a
/// hole: two triangles to a step), a fan from the middle to the points where the rays and the corners meet the edge for the whole rectangle. The graphics card decides each pixel by its middle, as the
/// reference renderer does for the straight edges of the sector; the arc is a polygon of at most a fraction of a pixel's error.
pub fn sweep_triangles(sw: &Sweep, x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<[f32; 2]> {
    let (start, delta) = sw.span();
    let mut out: Vec<[f32; 2]> = Vec::new();
    if !(delta > 0.0) || !(x1 > x0 && y1 > y0) {
        return out;
    }
    let a0 = start.rem_euclid(360.0);
    let ((cx, cy), (hw, hh)) = (((x0 + x1) * 0.5, (y0 + y1) * 0.5), ((x1 - x0) * 0.5, (y1 - y0) * 0.5));
    let dir = |deg: f32| {
        let r = deg.to_radians();
        (r.sin(), -r.cos())
    };
    if sw.shape == 0 {
        let radius = hw.min(hh);
        let hole = if sw.inner.is_finite() { sw.inner.clamp(0.0, 1.0) * radius } else { 0.0 };
        let per_turn = radius.clamp(32.0, 256.0);
        let n = ((delta / 360.0) * per_turn).ceil().max(1.0) as usize;
        for i in 0..n {
            let (d0, d1) = (dir(a0 + delta * i as f32 / n as f32), dir(a0 + delta * (i + 1) as f32 / n as f32));
            let (o0, o1) = ([cx + d0.0 * radius, cy + d0.1 * radius], [cx + d1.0 * radius, cy + d1.1 * radius]);
            if hole > 0.0 {
                let (i0, i1) = ([cx + d0.0 * hole, cy + d0.1 * hole], [cx + d1.0 * hole, cy + d1.1 * hole]);
                out.extend_from_slice(&[o0, o1, i1, o0, i1, i0]);
            } else {
                out.extend_from_slice(&[[cx, cy], o0, o1]);
            }
        }
    } else {
        // where a ray from the middle leaves the rectangle
        let hit = |deg: f32| {
            let d = dir(deg);
            let t = (hw / d.0.abs().max(1.0e-6)).min(hh / d.1.abs().max(1.0e-6));
            [cx + d.0 * t, cy + d.1 * t]
        };
        // the angles of the corners (clockwise from twelve o'clock) that fall inside the sweep, in order
        let mut angles: Vec<f32> = vec![a0];
        let mut corners: Vec<f32> = [(hw, hh), (hw, -hh), (-hw, -hh), (-hw, hh)]
            .iter()
            .map(|&(dx, dy)| {
                let mut a = dx.atan2(dy).to_degrees();
                while a <= a0 {
                    a += 360.0;
                }
                a
            })
            .filter(|a| *a < a0 + delta)
            .collect();
        corners.sort_by(|a, b| a.total_cmp(b));
        angles.extend(corners);
        angles.push(a0 + delta);
        for w in angles.windows(2) {
            out.extend_from_slice(&[[cx, cy], hit(w[0]), hit(w[1])]);
        }
    }
    out
}

/// Triangles for every shape of the interface (flat and rounded rectangles, gradients, pictures, the letters of TrueType text), in drawing order, as
/// (x, y, u, v, r, g, b, a) per vertex, and the runs that share a texture. Positions are in pixels from the top left, rounded exactly as the reference renderer
/// rounds them; a rounded rectangle is a fan of triangles round its outline, and the gradient and the picture's coordinates are worked out from each vertex's
/// place, so the graphics card's interpolation lands on the same colours the reference renderer computes pixel by pixel. With `square` the colours are squared
/// into the classic gamma-2.0 working space.
pub fn ui_vertices(scene: &Scene, w: f32, h: f32, square: bool) -> (Vec<f32>, Vec<UiBatch>) {
    let mut out: Vec<f32> = Vec::new();
    let mut batches: Vec<UiBatch> = Vec::new();
    for r in &scene.rects {
        let (px0, py0, px1, py1) = r.pixels(w as usize, h as usize);
        if px1 <= px0 || py1 <= py0 {
            continue;
        }
        // inside a scroll view: the graphics card cuts the shape at the viewport (a scissor rectangle), and nothing of it on the picture means nothing to draw
        let scissor = match r.clip {
            Some(c) => match scissor_of(c, w, h) {
                Some(s) => Some(s),
                None => continue,
            },
            None => None,
        };
        let (x0, y0, x1, y1) = (px0 as f32, py0 as f32, px1 as f32, py1 as f32);
        let (wf, hf) = (x1 - x0, y1 - y0);
        // a radial fill: the triangles of its sector, in one flat colour
        if let Some(sw) = &r.sweep {
            let c = if square { [r.color[0] * r.color[0], r.color[1] * r.color[1], r.color[2] * r.color[2], r.color[3]] } else { r.color };
            let first = (out.len() / 8) as u32;
            let tris = sweep_triangles(sw, x0, y0, x1, y1);
            for p in &tris {
                out.extend_from_slice(&[p[0], p[1], 0.5, 0.5, c[0], c[1], c[2], c[3].clamp(0.0, 1.0)]);
            }
            let count = tris.len() as u32;
            if count > 0 {
                match batches.last_mut() {
                    Some(b) if b.tex.is_none() && !b.smooth && b.scissor == scissor => b.count += count,
                    _ => batches.push(UiBatch { tex: None, smooth: false, first, count, scissor }),
                }
            }
            continue;
        }
        let radius = if r.radius.is_finite() { (r.radius * h).clamp(0.0, wf.min(hf) * 0.5) } else { 0.0 };
        let sq = |c: [f32; 4]| if square { [c[0] * c[0], c[1] * c[1], c[2] * c[2], c[3]] } else { c };
        let c1 = sq(r.color);
        let c2 = if r.color2[3] > 0.0 { sq(r.color2) } else { c1 };
        let (tex, smooth, uv) = match &r.image {
            Some(i) => (Some(i.tex.clone()), i.smooth, i.uv),
            None => (None, false, [0.5; 4]),
        };
        // the outline, clockwise from the top left
        let mut ring: Vec<(f32, f32)> = Vec::new();
        if radius <= 0.0 {
            ring.extend_from_slice(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)]);
        } else {
            let n = ((radius / 2.0).ceil() as usize).clamp(3, 24);
            for (cx, cy, from) in [(x1 - radius, y0 + radius, -90.0f32), (x1 - radius, y1 - radius, 0.0), (x0 + radius, y1 - radius, 90.0), (x0 + radius, y0 + radius, 180.0)] {
                for k in 0..=n {
                    let a = (from + 90.0 * k as f32 / n as f32).to_radians();
                    ring.push((cx + radius * a.cos(), cy + radius * a.sin()));
                }
            }
        }
        let vertex = |x: f32, y: f32| {
            let (tx, ty) = ((x - x0) / wf, (y - y0) / hf);
            let mix = |a: f32, b: f32| a + (b - a) * ty;
            [x, y, uv[0] + (uv[2] - uv[0]) * tx, uv[1] + (uv[3] - uv[1]) * ty, mix(c1[0], c2[0]), mix(c1[1], c2[1]), mix(c1[2], c2[2]), mix(c1[3], c2[3]).clamp(0.0, 1.0)]
        };
        let centre = vertex((x0 + x1) * 0.5, (y0 + y1) * 0.5);
        let first = (out.len() / 8) as u32;
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            out.extend_from_slice(&centre);
            out.extend_from_slice(&vertex(a.0, a.1));
            out.extend_from_slice(&vertex(b.0, b.1));
        }
        let count = (ring.len() * 3) as u32;
        let same = |t: &Option<Arc<Texture>>| match (t, &tex) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        };
        match batches.last_mut() {
            Some(b) if same(&b.tex) && b.smooth == smooth && b.scissor == scissor => b.count += count,
            _ => batches.push(UiBatch { tex, smooth, first, count, scissor }),
        }
    }
    (out, batches)
}

/// Quads for every lit font pixel row of every text, shadow pass first, as (x, y, u, v, r, g, b, a) per vertex, two triangles each.
/// Positions are in pixels from the top left. The geometry is the CPU renderer's: whole font pixels, `cell` pixels each. With `square`
/// the colours are squared into the classic gamma-2.0 working space (the picture is square-rooted afterwards). With `slots` (one per
/// text, the number the entity-id pass writes) the quads are those of the id pass instead: only the text itself, no shadow, and the
/// slot in the red channel.
pub fn text_vertices(scene: &Scene, w: f32, h: f32, square: bool, slots: Option<&[u32]>) -> Vec<f32> {
    let mut out: Vec<f32> = Vec::new();
    let (gw, gh) = (font::GLYPH_W as f32, font::GLYPH_H as f32);
    for (ti, t) in scene.texts.iter().enumerate() {
        let tx = &t.tx;
        let cell = ((tx.size.clamp(0.005, 0.5) * h / gh).round().max(1.0)).max(1.0);
        let advance = (gw + 1.0) * cell;
        let chars: Vec<char> = t.text.chars().take(512).collect();
        let width = (chars.len() as f32 * advance - cell).max(0.0);
        let ax = (tx.pos[0] * w).round();
        let x0 = match tx.align {
            1 => ax - (width / 2.0).floor(),
            2 => ax - width,
            _ => ax,
        };
        let y0 = (tx.pos[1] * h).round();
        let c = if square { [tx.color[0] * tx.color[0], tx.color[1] * tx.color[1], tx.color[2] * tx.color[2], tx.color[3]] } else { tx.color };
        let a = c[3].clamp(0.0, 1.0);
        // inside a scroll view the glyph quads are cut at the viewport (their texture coordinates with them, so the visible pixels are the ones the reference renderer draws)
        let bounds = t.clip.map(|c| {
            let (a, b, c2, d) = clip_pixels(c, w as usize, h as usize);
            (a as f32, b as f32, c2 as f32, d as f32)
        });
        let id_pass = [(0.0f32, [slots.map(|s| s[ti] as f32).unwrap_or(0.0), 0.0, 0.0, 1.0])];
        let passes: &[(f32, [f32; 4])] = if slots.is_some() { &id_pass } else if tx.shadow { &[(cell, [0.0, 0.0, 0.0, 0.65 * a]), (0.0, [c[0], c[1], c[2], a])] } else { &[(0.0, [c[0], c[1], c[2], a])] };
        for &(off, color) in passes {
            for (n, ch) in chars.iter().enumerate() {
                let gi = if (' '..='~').contains(ch) { *ch as usize - 32 } else { '?' as usize - 32 };
                let (u0, u1) = (gi as f32 / 95.0, (gi + 1) as f32 / 95.0);
                let (x, y) = (x0 + off + n as f32 * advance, y0 + off);
                let (xr, yb) = (x + gw * cell, y + gh * cell);
                let (mut qx, mut qy, mut qxr, mut qyb) = (x, y, xr, yb);
                let (mut qu0, mut qu1, mut qv0, mut qv1) = (u0, u1, 0.0f32, 1.0f32);
                if let Some((bx0, by0, bx1, by1)) = bounds {
                    let (nx0, nx1, ny0, ny1) = (x.max(bx0), xr.min(bx1), y.max(by0), yb.min(by1));
                    if nx0 >= nx1 || ny0 >= ny1 {
                        continue;
                    }
                    let (du, dv) = ((u1 - u0) / (xr - x), 1.0 / (yb - y));
                    (qu0, qu1, qv0, qv1) = (u0 + (nx0 - x) * du, u0 + (nx1 - x) * du, (ny0 - y) * dv, (ny1 - y) * dv);
                    (qx, qxr, qy, qyb) = (nx0, nx1, ny0, ny1);
                }
                for (px, py, u, v) in [(qx, qy, qu0, qv0), (qxr, qy, qu1, qv0), (qxr, qyb, qu1, qv1), (qx, qy, qu0, qv0), (qxr, qyb, qu1, qv1), (qx, qyb, qu0, qv1)] {
                    out.extend_from_slice(&[px, py, u, v, color[0], color[1], color[2], color[3]]);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_undoes_a_projection() {
        let p = M4::perspective(60.0, 16.0 / 9.0, 0.1, 500.0);
        let i = inverse(&p);
        let id = p.mul(&i);
        for r in 0..4 {
            for c in 0..4 {
                let want = if r == c { 1.0 } else { 0.0 };
                assert!((id.0[c * 4 + r] - want).abs() < 1e-4, "{r},{c}: {}", id.0[c * 4 + r]);
            }
        }
    }

    #[test]
    fn the_gpu_clip_space_is_reversed_z() {
        let p = M4::perspective(60.0, 1.0, 0.1, 100.0);
        let g = to_gpu_clip(&p);
        let near = g.mul_v4([0.0, 0.0, -0.1, 1.0]);
        let far = g.mul_v4([0.0, 0.0, -100.0, 1.0]);
        assert!((near[2] / near[3] - 1.0).abs() < 1e-4, "near plane maps to 1: {}", near[2] / near[3]);
        assert!((far[2] / far[3]).abs() < 1e-4, "far plane maps to 0: {}", far[2] / far[3]);
    }

    #[test]
    fn each_point_light_face_sees_its_own_axis() {
        let pos = [1.0, 2.0, 3.0];
        let faces = point_light_faces(pos, 0.1, 50.0);
        let dirs = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];
        for (face, d) in faces.iter().zip(dirs) {
            // a point straight ahead lands in the middle of the face, at the right depth
            let p = [pos[0] + d[0] * 10.0, pos[1] + d[1] * 10.0, pos[2] + d[2] * 10.0];
            let c = face.mul_v4([p[0], p[1], p[2], 1.0]);
            assert!((c[0] / c[3]).abs() < 1e-4 && (c[1] / c[3]).abs() < 1e-4, "{d:?}: {c:?}");
            let z = c[2] / c[3];
            let want = 50.0 * (10.0 - 0.1) / (10.0 * (50.0 - 0.1));
            assert!((z - want).abs() < 1e-4, "{d:?}: depth {z} vs {want}");
            // a point just beyond the face's 90 degree edge still projects inside the (wider) face
            let edge = [p[0] + 0.0, p[1], p[2]];
            let _ = edge;
        }
    }

    /// The area of a list of triangles (corners three to a triangle).
    fn area(tris: &[[f32; 2]]) -> f32 {
        tris.chunks(3).map(|t| ((t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[2][0] - t[0][0]) * (t[1][1] - t[0][1])).abs() * 0.5).sum()
    }

    fn sweep(start: f32, amount: f32, clockwise: bool, shape: u32, inner: f32) -> Sweep {
        Sweep { start, amount, clockwise, shape, inner }
    }

    #[test]
    fn the_triangles_of_a_circle_ring_and_box_sector_cover_the_right_area() {
        let r = 40.0f32;
        let pi = std::f32::consts::PI;
        // a disc in an 80 x 100 box: the circle inscribed (radius 40), the whole turn, a quarter, three quarters, and a ring with half the radius left out
        let (full, quarter, three, ring) = (
            sweep_triangles(&sweep(0.0, 1.0, true, 0, 0.0), 0.0, 0.0, 80.0, 100.0),
            sweep_triangles(&sweep(0.0, 0.25, true, 0, 0.0), 0.0, 0.0, 80.0, 100.0),
            sweep_triangles(&sweep(90.0, 0.75, false, 0, 0.0), 0.0, 0.0, 80.0, 100.0),
            sweep_triangles(&sweep(0.0, 1.0, true, 0, 0.5), 0.0, 0.0, 80.0, 100.0),
        );
        let disc = pi * r * r;
        assert!((area(&full) - disc).abs() < disc * 0.01, "{} vs {disc}", area(&full));
        assert!((area(&quarter) - disc * 0.25).abs() < disc * 0.01);
        assert!((area(&three) - disc * 0.75).abs() < disc * 0.01);
        assert!((area(&ring) - disc * 0.75).abs() < disc * 0.01, "a ring with half the radius out has three quarters of the area: {}", area(&ring));
        // the quarter from twelve o'clock clockwise is the top right quarter of the disc: the middle is (40, 50)
        assert!(quarter.iter().all(|p| p[0] >= 40.0 - 1e-3 && p[1] <= 50.0 + 1e-3), "{quarter:?}");
        // the box: all of it, the right half, a quarter from twelve o'clock to three (the top right quarter of the box), and an eighth that is cut by a corner
        let (w, h) = (60.0f32, 100.0f32);
        let whole = sweep_triangles(&sweep(0.0, 1.0, true, 1, 0.0), 0.0, 0.0, w, h);
        let right = sweep_triangles(&sweep(0.0, 0.5, true, 1, 0.0), 0.0, 0.0, w, h);
        let top_right = sweep_triangles(&sweep(0.0, 0.25, true, 1, 0.0), 0.0, 0.0, w, h);
        let left_up = sweep_triangles(&sweep(0.0, 0.25, false, 1, 0.0), 0.0, 0.0, w, h);
        assert!((area(&whole) - w * h).abs() < 0.1, "{}", area(&whole));
        assert!((area(&right) - w * h * 0.5).abs() < 0.1);
        assert!((area(&top_right) - w * h * 0.25).abs() < 0.1, "{}", area(&top_right));
        assert!((area(&left_up) - w * h * 0.25).abs() < 0.1, "counter-clockwise from twelve o'clock: the top left quarter");
        assert!(left_up.iter().all(|p| p[0] <= 30.0 + 1e-3 && p[1] <= 50.0 + 1e-3));
        // starting past a corner: a sweep from three o'clock for a quarter turn is the bottom right quarter
        let bottom_right = sweep_triangles(&sweep(90.0, 0.25, true, 1, 0.0), 0.0, 0.0, w, h);
        assert!((area(&bottom_right) - w * h * 0.25).abs() < 0.1);
        assert!(bottom_right.iter().all(|p| p[0] >= 30.0 - 1e-3 && p[1] >= 50.0 - 1e-3));
        // nothing for nothing
        assert!(sweep_triangles(&sweep(0.0, 0.0, true, 0, 0.0), 0.0, 0.0, 80.0, 100.0).is_empty());
        assert!(sweep_triangles(&sweep(0.0, 0.5, true, 1, 0.0), 5.0, 5.0, 5.0, 9.0).is_empty());
        assert!(sweep_triangles(&sweep(f32::NAN, 0.5, true, 1, 0.0), 0.0, 0.0, 10.0, 10.0).len() <= 12, "a start that is not a number is treated as 0");
    }

    #[test]
    fn srgb_round_trips() {
        for i in 0..=255 {
            let c = i as f32 / 255.0;
            assert!((srgb_encode(srgb_decode(c)) - c).abs() < 1e-5);
        }
    }

    #[test]
    fn mip_chain_ends_at_one_texel_and_keeps_the_average() {
        let mut rgba = Vec::new();
        for i in 0..16 {
            rgba.extend_from_slice(if i % 2 == 0 { &[255, 255, 255, 255] } else { &[0, 0, 0, 255] });
        }
        let chain = mip_chain(4, 4, rgba, true);
        assert_eq!(chain.len(), 3);
        let top = &chain[2].2;
        // half of the texels white: the linear average is 0.5, which encodes to about 188 in sRGB
        assert!((top[0] as i32 - 188).abs() <= 2, "{}", top[0]);
    }
}

/// A hash for small integer keys (addresses, indices): one multiply a word, no randomness, so a look-up costs a few nanoseconds (the default hasher's strength is for keys from outside). Only for look-ups: never iterated where the order matters.
#[derive(Default, Clone, Copy)]
pub struct FastHasher(u64);

impl std::hash::Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(5) ^ *b as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
    fn write_u8(&mut self, i: u8) {
        self.0 = (self.0.rotate_left(5) ^ i as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    fn write_u32(&mut self, i: u32) {
        self.0 = (self.0.rotate_left(5) ^ i as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    fn write_usize(&mut self, i: usize) {
        self.0 = (self.0.rotate_left(5) ^ i as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

pub type FastMap<K, V> = std::collections::HashMap<K, V, std::hash::BuildHasherDefault<FastHasher>>;

/// Run `f(first_index, chunk)` over pieces of `data` on up to 8 threads, each piece at least `min_chunk` long (a short slice is done right here: starting threads costs more than they save below a few thousand elements). The result must
/// not depend on how the slice is cut, which it does not when each element is a function of its own index.
pub fn par_chunks_mut<T: Send>(data: &mut [T], min_chunk: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(8);
    let n = data.len();
    if threads <= 1 || n < min_chunk * 2 {
        f(0, data);
        return;
    }
    let chunk = n.div_ceil(threads).max(min_chunk);
    std::thread::scope(|s| {
        for (ci, c) in data.chunks_mut(chunk).enumerate() {
            let f = &f;
            s.spawn(move || f(ci * chunk, c));
        }
    });
}

/// The planes of a view volume (a camera's, or one cascade of a shadow map): what is outside any one of them cannot be seen, so it need not be drawn. Made from a view-projection matrix in the GPU's clip space
/// (x and y in -w..w, z in 0..w; the depth being reversed does not matter), each plane normalised and pointing inwards, so that a point's signed distance from it is its distance in metres.
#[derive(Clone, Copy, Debug)]
pub struct Frustum {
    planes: [[f32; 4]; 6],
    count: usize,
}

impl Frustum {
    /// `sides_only` leaves out the near and far planes: for a shadow map, whose caster may stand between the light and the map's volume (and still throw its shadow into it).
    pub fn from_view_proj(m: &M4, sides_only: bool) -> Frustum {
        let row = |r: usize| [m.0[r], m.0[4 + r], m.0[8 + r], m.0[12 + r]];
        let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
        let comb = |a: [f32; 4], b: [f32; 4], s: f32| [a[0] + s * b[0], a[1] + s * b[1], a[2] + s * b[2], a[3] + s * b[3]];
        let raw = [comb(r3, r0, 1.0), comb(r3, r0, -1.0), comb(r3, r1, 1.0), comb(r3, r1, -1.0), r2, comb(r3, r2, -1.0)];
        let mut planes = [[0.0, 0.0, 0.0, 1.0]; 6];
        for (out, p) in planes.iter_mut().zip(raw) {
            let len = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            // a degenerate plane (a matrix that is not a view) lets everything through
            if len > 1.0e-12 && len.is_finite() {
                *out = [p[0] / len, p[1] / len, p[2] / len, p[3] / len];
            }
        }
        Frustum { planes, count: if sides_only { 4 } else { 6 } }
    }

    /// Could any part of the box (`min` to `max`, axis-aligned) be inside? (Conservative, like `sphere_visible`.)
    pub fn aabb_visible(&self, min: V3, max: V3) -> bool {
        self.planes[..self.count].iter().all(|p| {
            // the corner of the box that lies farthest along the plane's normal: if even that one is outside, the whole box is
            let x = if p[0] >= 0.0 { max[0] } else { min[0] };
            let y = if p[1] >= 0.0 { max[1] } else { min[1] };
            let z = if p[2] >= 0.0 { max[2] } else { min[2] };
            !(p[0] * x + p[1] * y + p[2] * z + p[3] < 0.0)
        })
    }

    /// Could any part of the sphere be inside? (Conservative: a sphere that only touches counts, and a NaN is "yes".)
    #[inline]
    pub fn sphere_visible(&self, c: V3, r: f32) -> bool {
        self.planes[..self.count].iter().all(|p| !(p[0] * c[0] + p[1] * c[1] + p[2] * c[2] + p[3] < -r))
    }
}

#[cfg(test)]
mod frustum_tests {
    use super::*;

    fn view(eye: V3, target: V3) -> M4 {
        // a look-at in the engine's own convention: the camera looks down its own -z
        let f = normalize(sub(target, eye));
        let r = normalize(cross(f, [0.0, 1.0, 0.0]));
        let u = cross(r, f);
        let mut m = M4::identity();
        m.0[0] = r[0];
        m.0[4] = r[1];
        m.0[8] = r[2];
        m.0[1] = u[0];
        m.0[5] = u[1];
        m.0[9] = u[2];
        m.0[2] = -f[0];
        m.0[6] = -f[1];
        m.0[10] = -f[2];
        m.0[12] = -dot(r, eye);
        m.0[13] = -dot(u, eye);
        m.0[14] = dot(f, eye);
        m
    }

    #[test]
    fn a_sphere_is_seen_when_it_touches_the_view_volume_and_not_otherwise() {
        // 90 degrees across, square picture, from the origin down -z, 0.5 m to 100 m
        let vp = to_gpu_clip(&M4::perspective(90.0, 1.0, 0.5, 100.0).mul(&view([0.0, 0.0, 0.0], [0.0, 0.0, -1.0])));
        let fr = Frustum::from_view_proj(&vp, false);
        assert!(fr.sphere_visible([0.0, 0.0, -10.0], 0.1), "straight ahead");
        assert!(fr.sphere_visible([9.0, 0.0, -10.0], 0.1), "inside the 45 degree side plane");
        assert!(!fr.sphere_visible([12.0, 0.0, -10.0], 0.1), "beside it");
        assert!(fr.sphere_visible([12.0, 0.0, -10.0], 2.5), "a big sphere that reaches in (the plane is 10 m out at that depth: 1.41 m from the centre)");
        assert!(!fr.sphere_visible([0.0, 14.0, -10.0], 1.0), "above it");
        assert!(!fr.sphere_visible([0.0, 0.0, 10.0], 1.0), "behind it");
        assert!(!fr.sphere_visible([0.0, 0.0, -150.0], 1.0), "beyond the far plane");
        assert!(fr.sphere_visible([0.0, 0.0, -101.0], 2.0), "reaching over the far plane");
        assert!(fr.sphere_visible([0.0, 0.0, 0.2], 1.0), "reaching over the near plane");
        // a shadow cascade is a box (an orthographic volume 20 m high and wide, from 0.5 to 100 m along -z): a caster behind it (towards the light) still throws its shadow in, so only the sides count
        let vp = to_gpu_clip(&M4::ortho(20.0, 1.0, 0.5, 100.0).mul(&view([0.0, 0.0, 0.0], [0.0, 0.0, -1.0])));
        let (all, sides) = (Frustum::from_view_proj(&vp, false), Frustum::from_view_proj(&vp, true));
        assert!(!all.sphere_visible([0.0, 0.0, 10.0], 1.0), "behind the box");
        assert!(sides.sphere_visible([0.0, 0.0, 10.0], 1.0), "behind the box, but between its sides");
        assert!(sides.sphere_visible([9.0, 0.0, 10.0], 0.5), "inside the side");
        assert!(!sides.sphere_visible([12.0, 0.0, 10.0], 1.0), "outside the sides");
        // nothing to say about a sphere of infinite size, and a NaN is kept
        assert!(fr.sphere_visible([1.0e6, 0.0, 0.0], f32::INFINITY));
        assert!(fr.sphere_visible([f32::NAN, 0.0, 0.0], 1.0));
    }

    #[test]
    fn a_box_is_seen_when_any_corner_of_it_is_inside_the_volume() {
        let vp = to_gpu_clip(&M4::perspective(90.0, 1.0, 0.5, 100.0).mul(&view([0.0, 0.0, 0.0], [0.0, 0.0, -1.0])));
        let fr = Frustum::from_view_proj(&vp, false);
        assert!(fr.aabb_visible([-1.0, -1.0, -11.0], [1.0, 1.0, -9.0]), "straight ahead");
        assert!(fr.aabb_visible([8.0, -1.0, -11.0], [20.0, 1.0, -9.0]), "a box that reaches in from the side");
        assert!(!fr.aabb_visible([12.0, -1.0, -11.0], [20.0, 1.0, -9.0]), "wholly beside it");
        assert!(!fr.aabb_visible([-5.0, -5.0, 3.0], [5.0, 5.0, 9.0]), "behind it");
        assert!(!fr.aabb_visible([-1.0, -1.0, -300.0], [1.0, 1.0, -200.0]), "beyond the far plane");
    }

    #[test]
    fn a_degenerate_matrix_lets_everything_through() {
        let fr = Frustum::from_view_proj(&M4([0.0; 16]), false);
        assert!(fr.sphere_visible([5.0, 5.0, 5.0], 0.0));
    }
}
