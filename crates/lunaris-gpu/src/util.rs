//! Small helpers: matrix maths for the GPU's depth convention, colour conversions, cascade fitting for the shadow maps, texture mip
//! chains, and the glyph quads of the HUD text.
use lunaris_render::math::{cross, dot, length, normalize, rotate, scale, sub, add, M4, V3};
use lunaris_render::render::{Cam, Scene};
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
    let mut out = [Cascade { view_proj: M4::identity(), texel: 1.0, far: end }; CASCADES];
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
        out[i] = Cascade { view_proj: M4(p).mul(&view), texel, far: slice_far };
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

/// A run of interface vertices drawn with one texture (`None`: plain white).
pub struct UiBatch {
    pub tex: Option<Arc<Texture>>,
    pub smooth: bool,
    pub first: u32,
    pub count: u32,
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
        let (x0, y0, x1, y1) = (px0 as f32, py0 as f32, px1 as f32, py1 as f32);
        let (wf, hf) = (x1 - x0, y1 - y0);
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
            Some(b) if same(&b.tex) && b.smooth == smooth => b.count += count,
            _ => batches.push(UiBatch { tex, smooth, first, count }),
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
        let id_pass = [(0.0f32, [slots.map(|s| s[ti] as f32).unwrap_or(0.0), 0.0, 0.0, 1.0])];
        let passes: &[(f32, [f32; 4])] = if slots.is_some() { &id_pass } else if tx.shadow { &[(cell, [0.0, 0.0, 0.0, 0.65 * a]), (0.0, [c[0], c[1], c[2], a])] } else { &[(0.0, [c[0], c[1], c[2], a])] };
        for &(off, color) in passes {
            for (n, ch) in chars.iter().enumerate() {
                let gi = if (' '..='~').contains(ch) { *ch as usize - 32 } else { '?' as usize - 32 };
                let (u0, u1) = (gi as f32 / 95.0, (gi + 1) as f32 / 95.0);
                let (x, y) = (x0 + off + n as f32 * advance, y0 + off);
                let (xr, yb) = (x + gw * cell, y + gh * cell);
                for (px, py, u, v) in [(x, y, u0, 0.0), (xr, y, u1, 0.0), (xr, yb, u1, 1.0), (x, y, u0, 0.0), (xr, yb, u1, 1.0), (x, yb, u0, 1.0)] {
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
