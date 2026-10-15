//! The reference renderer: a deterministic CPU rasteriser. It draws 3D meshes with lighting and 2D sprites through the same
//! triangle path, tracks which entity covers each pixel, and produces bit-identical images on every machine (only IEEE
//! `+ - * / sqrt` and `lunaris_sim::detmath` are used). A GPU renderer can be validated against it later.
//!
//! Colour handling: colours given to the engine are display colours (0..1). Inside the renderer they are squared into a
//! gamma-2.0 linear space, lit and blended there, and square-rooted back on output.
use crate::frame::Frame;
use crate::math::*;
use crate::mesh::{builtin, Mesh};
use crate::texture::{Texture, TextureCache};
use lunaris_ecs::{EcsError, Result, World};
use lunaris_sim::components::{Water, AnimLayers, Animator, Camera, Environment, Joint, Light, Localization, MeshRenderer, Sprite, Terrain, CameraView, Decal, Ik, Parallax, Particles, Text, TextFont, Tilemap, Transform, Ui, UiAnchor, UiStyle};
use lunaris_sim::detmath;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ExplicitCamera {
    pub eye: V3,
    pub target: V3,
    pub up: V3,
    pub fov_y: f32,
    pub ortho_height: Option<f32>,
    pub near: f32,
    pub far: f32,
    pub clear: [f32; 4],
}

impl Default for ExplicitCamera {
    fn default() -> Self {
        ExplicitCamera { eye: [0.0, 3.0, 8.0], target: [0.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], fov_y: 60.0, ortho_height: None, near: 0.1, far: 1000.0, clear: [0.05, 0.05, 0.08, 1.0] }
    }
}

#[derive(Clone, Debug)]
pub enum View {
    /// Use the Camera entity with this uid, or the active one with the lowest uid, or a default view when there is none.
    World(Option<u64>),
    Explicit(ExplicitCamera),
}

#[derive(Clone, Debug)]
pub struct RenderOptions {
    pub width: u32,
    pub height: u32,
    pub view: View,
    /// 1 = none, 2 = 2x2 supersampling (smoother edges, four times the work).
    pub supersample: u32,
    /// Worker threads. The picture is split into horizontal bands drawn in parallel; every pixel is computed exactly as it would be on
    /// one thread, so the image (and its hash) never depends on this number.
    pub threads: usize,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions { width: 640, height: 360, view: View::World(None), supersample: 1, threads: 1 }
    }
}

/// The camera a picture is taken with.
pub struct Cam {
    pub pos: V3,
    pub rot: [f32; 4],
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
    /// Height of the view in world units for an orthographic camera.
    pub ortho: Option<f32>,
    /// Background colour (display colour).
    pub clear: [f32; 4],
    pub desc: String,
}

#[derive(Clone, Copy)]
struct Vtx {
    clip: [f32; 4],
    world: V3,
    nrm: V3,
    uv: [f32; 2],
    col: V3,
    tw: f32,
}

fn lerp_v(a: &Vtx, b: &Vtx, t: f32) -> Vtx {
    let l = |x: f32, y: f32| x + (y - x) * t;
    Vtx {
        clip: [l(a.clip[0], b.clip[0]), l(a.clip[1], b.clip[1]), l(a.clip[2], b.clip[2]), l(a.clip[3], b.clip[3])],
        world: [l(a.world[0], b.world[0]), l(a.world[1], b.world[1]), l(a.world[2], b.world[2])],
        nrm: [l(a.nrm[0], b.nrm[0]), l(a.nrm[1], b.nrm[1]), l(a.nrm[2], b.nrm[2])],
        uv: [l(a.uv[0], b.uv[0]), l(a.uv[1], b.uv[1])],
        col: [l(a.col[0], b.col[0]), l(a.col[1], b.col[1]), l(a.col[2], b.col[2])],
        tw: l(a.tw, b.tw),
    }
}

/// Clip a triangle against the near plane (z >= -w). Returns up to four vertices.
fn clip_near(tri: [Vtx; 3]) -> ([Vtx; 4], usize) {
    let d = |v: &Vtx| v.clip[2] + v.clip[3];
    let mut out = [tri[0]; 4];
    let mut n = 0;
    for i in 0..3 {
        let (a, b) = (&tri[i], &tri[(i + 1) % 3]);
        let (da, db) = (d(a), d(b));
        if da >= 0.0 {
            out[n] = *a;
            n += 1;
        }
        if (da >= 0.0) != (db >= 0.0) {
            out[n] = lerp_v(a, b, da / (da - db));
            n += 1;
        }
    }
    (out, n)
}

#[derive(Clone, Copy, Debug)]
pub struct LightData {
    /// Directional: unit vector towards the light. Point: world position.
    pub vec: V3,
    pub point: bool,
    /// Colour times intensity, in the linear working space.
    pub color: V3,
    pub range: f32,
    /// A spot light's cone (the high-quality GPU mode only): the direction it shines in and the cosines of the outer and inner angles;
    /// `cos_outer` is -2 for lights that are not spots. The reference renderer treats a spot as a point light.
    pub spot_dir: V3,
    pub cos_outer: f32,
    pub cos_inner: f32,
}

enum Shade<'a> {
    Lit { color: V3, shininess: u32, emissive: f32, tex: Option<&'a Texture>, vcol: bool, uv_scale: [f32; 2] },
    /// `uv` is the part of the texture to show: (u0, v0, du, dv), v counted from the bottom.
    Sprite { color: [f32; 4], tex: Option<&'a Texture>, uv: [f32; 4] },
    /// A particle cloud: colour from the vertices, opacity from the vertex texture weight, the picture (bilinear) on top.
    Particle { tex: Option<&'a Texture>, additive: bool },
}

/// A band of rows `y0..y1` of the picture (the buffers hold only those rows). `h` is the height of the whole picture.
struct Target<'a> {
    w: usize,
    h: usize,
    y0: usize,
    y1: usize,
    color: &'a mut [V3],
    depth: &'a mut [f32],
    ids: &'a mut [u64],
}

const AMBIENT: f32 = 0.16;

fn shade_lit(base: V3, shininess: u32, p: V3, n: V3, eye: V3, lights: &[LightData]) -> V3 {
    let mut col = scale(base, AMBIENT);
    let view = normalize(sub(eye, p));
    for l in lights {
        let (ldir, atten) = if l.point {
            let to = sub(l.vec, p);
            let dist = length(to);
            let k = (dist / l.range).min(1.0);
            let a = 1.0 - k * k;
            (if dist > 0.0 { scale(to, 1.0 / dist) } else { [0.0, 1.0, 0.0] }, a * a)
        } else {
            (l.vec, 1.0)
        };
        let ndl = dot(n, ldir);
        if ndl <= 0.0 {
            continue;
        }
        let lc = scale(l.color, atten);
        col = add(col, [base[0] * lc[0] * ndl, base[1] * lc[1] * ndl, base[2] * lc[2] * ndl]);
        let h = normalize(add(ldir, view));
        let nh = dot(n, h).max(0.0);
        let spec = detmath::powi(nh, shininess.max(1)) * 0.25;
        col = add(col, scale(lc, spec));
    }
    col
}

/// Screen-space edge function. Done in f64: triangles clipped at the near plane can span thousands of pixels, and f32
/// loses the sub-pixel accuracy the inside test needs there.
#[inline]
fn edge(ax: f64, ay: f64, bx: f64, by: f64, px: f64, py: f64) -> f64 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

/// Does an edge from a to b include pixels exactly on it? Exactly one of two triangles sharing an edge says yes.
fn owns_edge(ax: f64, ay: f64, bx: f64, by: f64) -> bool {
    let (dx, dy) = (bx - ax, by - ay);
    dy < 0.0 || (dy == 0.0 && dx > 0.0)
}

#[allow(clippy::too_many_arguments)]
fn raster_triangle(t: &mut Target<'_>, v: [Vtx; 3], shade: &Shade, lights: &[LightData], eye: V3, id: u64, cull_back: bool, write_depth: bool) {
    let (w, h) = (t.w as f64, t.h as f64);
    let mut s = [[0.0f64; 2]; 3];
    let mut z = [0.0f64; 3];
    let mut iw = [0.0f64; 3];
    for i in 0..3 {
        let c = v[i].clip;
        let inv = 1.0 / c[3] as f64;
        iw[i] = inv;
        s[i] = [(c[0] as f64 * inv * 0.5 + 0.5) * w, (1.0 - (c[1] as f64 * inv * 0.5 + 0.5)) * h];
        z[i] = c[2] as f64 * inv;
    }
    let mut area = edge(s[0][0], s[0][1], s[1][0], s[1][1], s[2][0], s[2][1]);
    if area == 0.0 || !area.is_finite() {
        return;
    }
    // With `edge` as defined above, a triangle that is counter-clockwise in NDC (y up) has a positive area on screen (y down):
    // that is a front face. Back faces are dropped, or flipped when the caller wants double-sided drawing.
    let mut order = [0usize, 1, 2];
    if area < 0.0 {
        if cull_back {
            return;
        }
        order = [0, 2, 1];
        area = -area;
    }
    let (i0, i1, i2) = (order[0], order[1], order[2]);
    let (x0, y0, x1, y1, x2, y2) = (s[i0][0], s[i0][1], s[i1][0], s[i1][1], s[i2][0], s[i2][1]);
    let min_x = x0.min(x1).min(x2).floor().max(0.0) as usize;
    let max_x = (x0.max(x1).max(x2).ceil().min(w) as usize).min(t.w);
    let min_y = (y0.min(y1).min(y2).floor().max(0.0) as usize).max(t.y0);
    let max_y = (y0.max(y1).max(y2).ceil().min(h) as usize).min(t.y1);
    let own = [owns_edge(x1, y1, x2, y2), owns_edge(x2, y2, x0, y0), owns_edge(x0, y0, x1, y1)];
    let inv_area = 1.0 / area;
    for py in min_y..max_y {
        for px in min_x..max_x {
            let (cx, cy) = (px as f64 + 0.5, py as f64 + 0.5);
            let e0 = edge(x1, y1, x2, y2, cx, cy);
            let e1 = edge(x2, y2, x0, y0, cx, cy);
            let e2 = edge(x0, y0, x1, y1, cx, cy);
            // with positive area the inside has all edge values >= 0; pixels exactly on an edge go to one side only
            let inside = (e0 > 0.0 || (e0 == 0.0 && own[0])) && (e1 > 0.0 || (e1 == 0.0 && own[1])) && (e2 > 0.0 || (e2 == 0.0 && own[2]));
            if !inside {
                continue;
            }
            let (b0, b1, b2) = (e0 * inv_area, e1 * inv_area, e2 * inv_area);
            let depth = (b0 * z[i0] + b1 * z[i1] + b2 * z[i2]) as f32;
            if !(-1.0..=1.0).contains(&depth) {
                continue;
            }
            let idx = (py - t.y0) * t.w + px;
            if depth >= t.depth[idx] {
                continue;
            }
            // perspective-correct weights
            let (p0, p1, p2) = (b0 * iw[i0], b1 * iw[i1], b2 * iw[i2]);
            let norm = 1.0 / (p0 + p1 + p2);
            let (q0, q1, q2) = ((p0 * norm) as f32, (p1 * norm) as f32, (p2 * norm) as f32);
            let (va, vb, vc) = (&v[i0], &v[i1], &v[i2]);
            match shade {
                Shade::Lit { color, shininess, emissive, tex, vcol, uv_scale } => {
                    let world = [q0 * va.world[0] + q1 * vb.world[0] + q2 * vc.world[0], q0 * va.world[1] + q1 * vb.world[1] + q2 * vc.world[1], q0 * va.world[2] + q1 * vb.world[2] + q2 * vc.world[2]];
                    let n = normalize([q0 * va.nrm[0] + q1 * vb.nrm[0] + q2 * vc.nrm[0], q0 * va.nrm[1] + q1 * vb.nrm[1] + q2 * vc.nrm[1], q0 * va.nrm[2] + q1 * vb.nrm[2] + q2 * vc.nrm[2]]);
                    // vertex colours only for imported models: multiplying by an interpolated white would change the last bit of every pixel
                    let mut base = if *vcol { [color[0] * (q0 * va.col[0] + q1 * vb.col[0] + q2 * vc.col[0]), color[1] * (q0 * va.col[1] + q1 * vb.col[1] + q2 * vc.col[1]), color[2] * (q0 * va.col[2] + q1 * vb.col[2] + q2 * vc.col[2])] } else { *color };
                    if let Some(tx) = tex {
                        let w = q0 * va.tw + q1 * vb.tw + q2 * vc.tw;
                        if w > 0.001 {
                            let uv = [q0 * va.uv[0] + q1 * vb.uv[0] + q2 * vc.uv[0], q0 * va.uv[1] + q1 * vb.uv[1] + q2 * vc.uv[1]];
                            let texel = tx.sample((uv[0] * uv_scale[0]).rem_euclid(1.0), (uv[1] * uv_scale[1]).rem_euclid(1.0)); // textures repeat
                            if w > 0.999 && texel[3] < 0.5 {
                                continue; // cut-out
                            }
                            base = [base[0] * (1.0 + w * (texel[0] - 1.0)), base[1] * (1.0 + w * (texel[1] - 1.0)), base[2] * (1.0 + w * (texel[2] - 1.0))];
                        }
                    }
                    let mut lit = shade_lit(base, *shininess, world, n, eye, lights);
                    if *emissive > 0.0 {
                        lit = add(lit, scale(base, *emissive));
                    }
                    t.color[idx] = lit;
                    t.depth[idx] = depth;
                    t.ids[idx] = id;
                }
                Shade::Sprite { color, tex, uv: rect } => {
                    let uv = [q0 * va.uv[0] + q1 * vb.uv[0] + q2 * vc.uv[0], q0 * va.uv[1] + q1 * vb.uv[1] + q2 * vc.uv[1]];
                    let texel = tex.map(|t| t.sample(rect[0] + uv[0] * rect[2], rect[1] + uv[1] * rect[3])).unwrap_or([1.0, 1.0, 1.0, 1.0]);
                    let a = color[3] * texel[3];
                    if a <= 0.0 {
                        continue;
                    }
                    let src = [color[0] * texel[0], color[1] * texel[1], color[2] * texel[2]];
                    let dst = t.color[idx];
                    t.color[idx] = [src[0] * a + dst[0] * (1.0 - a), src[1] * a + dst[1] * (1.0 - a), src[2] * a + dst[2] * (1.0 - a)];
                    if a >= 0.5 {
                        t.ids[idx] = id;
                    }
                    if write_depth {
                        t.depth[idx] = depth;
                    }
                }
                Shade::Particle { tex, additive } => {
                    let uv = [q0 * va.uv[0] + q1 * vb.uv[0] + q2 * vc.uv[0], q0 * va.uv[1] + q1 * vb.uv[1] + q2 * vc.uv[1]];
                    let texel = tex.map(|t| t.sample_ui(uv[0], 1.0 - uv[1], true)).unwrap_or([1.0, 1.0, 1.0, 1.0]);
                    let a = (q0 * va.tw + q1 * vb.tw + q2 * vc.tw) * texel[3];
                    if a <= 0.0 {
                        continue;
                    }
                    let col = [q0 * va.col[0] + q1 * vb.col[0] + q2 * vc.col[0], q0 * va.col[1] + q1 * vb.col[1] + q2 * vc.col[1], q0 * va.col[2] + q1 * vb.col[2] + q2 * vc.col[2]];
                    let src = [col[0] * texel[0], col[1] * texel[1], col[2] * texel[2]];
                    let dst = t.color[idx];
                    t.color[idx] = if *additive { [dst[0] + src[0] * a, dst[1] + src[1] * a, dst[2] + src[2] * a] } else { [src[0] * a + dst[0] * (1.0 - a), src[1] * a + dst[1] * (1.0 - a), src[2] * a + dst[2] * (1.0 - a)] };
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_mesh(t: &mut Target<'_>, mesh: &Mesh, model: &M4, view_proj: &M4, shade: &Shade, lights: &[LightData], eye: V3, id: u64, cull_back: bool) {
    let mvp = view_proj.mul(model);
    let nm = model.normal_matrix();
    let verts: Vec<Vtx> = (0..mesh.pos.len())
        .map(|i| {
            let p = mesh.pos[i];
            let n = mesh.nrm[i];
            let wn = normalize([nm[0][0] * n[0] + nm[0][1] * n[1] + nm[0][2] * n[2], nm[1][0] * n[0] + nm[1][1] * n[1] + nm[1][2] * n[2], nm[2][0] * n[0] + nm[2][1] * n[1] + nm[2][2] * n[2]]);
            Vtx { clip: mvp.mul_v4([p[0], p[1], p[2], 1.0]), world: model.transform_point(p), nrm: wn, uv: mesh.uv[i], col: mesh.col.get(i).copied().unwrap_or([1.0, 1.0, 1.0]), tw: mesh.texw.get(i).copied().unwrap_or(1.0) }
        })
        .collect();
    for tri in mesh.idx.chunks(3) {
        let v = [verts[tri[0] as usize], verts[tri[1] as usize], verts[tri[2] as usize]];
        // quick reject: all three vertices outside the same side of the view volume
        let outside = |f: &dyn Fn(&Vtx) -> bool| v.iter().all(f);
        if outside(&|a| a.clip[0] > a.clip[3]) || outside(&|a| a.clip[0] < -a.clip[3]) || outside(&|a| a.clip[1] > a.clip[3]) || outside(&|a| a.clip[1] < -a.clip[3]) || outside(&|a| a.clip[2] > a.clip[3]) {
            continue;
        }
        let (poly, n) = clip_near(v);
        for k in 1..n.saturating_sub(1) {
            raster_triangle(t, [poly[0], poly[k], poly[k + 1]], shade, lights, eye, id, cull_back, matches!(shade, Shade::Lit { .. }));
        }
    }
}

/// Whether a mesh name refers to a model file rather than a built-in shape.
pub fn is_model_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".glb") || l.ends_with(".gltf")
}

fn display_to_linear(c: [f32; 4]) -> [f32; 4] {
    [c[0] * c[0], c[1] * c[1], c[2] * c[2], c[3]]
}

/// Fill a rectangle of the render target with `color` at `alpha`, optionally claiming the pixels for an entity.
fn fill_rect(t: &mut Target<'_>, x: i64, y: i64, w: usize, h: usize, color: V3, alpha: f32, claim: Option<u64>) {
    for yy in y.max(t.y0 as i64)..(y + h as i64).min(t.y1 as i64) {
        for xx in x.max(0)..(x + w as i64).min(t.w as i64) {
            let i = (yy as usize - t.y0) * t.w + xx as usize;
            let bg = t.color[i];
            t.color[i] = [bg[0] + (color[0] - bg[0]) * alpha, bg[1] + (color[1] - bg[1]) * alpha, bg[2] + (color[2] - bg[2]) * alpha];
            if let Some(id) = claim {
                t.ids[i] = id;
                t.depth[i] = -1.0;
            }
        }
    }
}

/// How far a point is inside (negative) or outside (positive) of a rounded box of half size `(hx, hy)` and corner radius `r`, all in pixels from its centre.
fn rounded_distance(px: f32, py: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let (qx, qy) = (px.abs() - (hx - r), py.abs() - (hy - r));
    let outside = (qx.max(0.0) * qx.max(0.0) + qy.max(0.0) * qy.max(0.0)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

/// A shape of the interface: a (rounded, gradient, picture) rectangle on a render target. `target_height` is the height of the target in pixels, which a radius
/// (a fraction of the picture's height) is counted in. Edges are anti-aliased by their coverage, so a rounded corner is smooth at any supersampling.
fn draw_shape(t: &mut Target<'_>, r: &SceneRect, target_height: usize) {
    let (px0, py0, px1, py1) = r.pixels(t.w, t.h);
    if px1 <= px0 || py1 <= py0 {
        return;
    }
    let (wf, hf) = ((px1 - px0) as f32, (py1 - py0) as f32);
    let radius = if r.radius.is_finite() { (r.radius * target_height as f32).clamp(0.0, wf.min(hf) * 0.5) } else { 0.0 };
    let c1 = display_to_linear(r.color);
    let c2 = if r.color2[3] > 0.0 { display_to_linear(r.color2) } else { c1 };
    let styled = r.is_styled();
    for yy in py0.max(t.y0 as i64)..py1.min(t.y1 as i64) {
        let fy = (yy - py0) as f32 + 0.5;
        for xx in px0.max(0)..px1.min(t.w as i64) {
            let fx = (xx - px0) as f32 + 0.5;
            let mut cover = 1.0f32;
            if radius > 0.0 {
                cover = (0.5 - rounded_distance(fx - wf * 0.5, fy - hf * 0.5, wf * 0.5, hf * 0.5, radius)).clamp(0.0, 1.0);
                if cover <= 0.0 {
                    continue;
                }
            }
            let ty = fy / hf;
            let mut c = if styled { [c1[0] + (c2[0] - c1[0]) * ty, c1[1] + (c2[1] - c1[1]) * ty, c1[2] + (c2[2] - c1[2]) * ty, c1[3] + (c2[3] - c1[3]) * ty] } else { c1 };
            if let Some(img) = &r.image {
                let (u, v) = (img.uv[0] + (img.uv[2] - img.uv[0]) * (fx / wf), img.uv[1] + (img.uv[3] - img.uv[1]) * ty);
                let s = img.tex.sample_ui(u, v, img.smooth);
                c = [c[0] * s[0], c[1] * s[1], c[2] * s[2], c[3] * s[3]];
            }
            let alpha = c[3].clamp(0.0, 1.0) * cover;
            if alpha <= 0.0 {
                continue;
            }
            let i = (yy as usize - t.y0) * t.w + xx as usize;
            let bg = t.color[i];
            t.color[i] = [bg[0] + (c[0] - bg[0]) * alpha, bg[1] + (c[1] - bg[1]) * alpha, bg[2] + (c[2] - bg[2]) * alpha];
        }
    }
}

fn draw_text(t: &mut Target<'_>, uid: u64, text: &str, tx: &Text, ss: usize, final_height: usize) {
    use crate::font::{glyph, lit, GLYPH_H, GLYPH_W};
    // whole font pixels (a multiple of the supersampling factor) keep the edges crisp
    let cell = ((tx.size.clamp(0.005, 0.5) * final_height as f32 / GLYPH_H as f32).round().max(1.0) as usize) * ss;
    let advance = (GLYPH_W + 1) * cell;
    let chars: Vec<char> = text.chars().take(512).collect();
    let width = (chars.len() * advance).saturating_sub(cell);
    let ax = (tx.pos[0] * t.w as f32).round().clamp(-1.0e9, 1.0e9) as i64; // (a huge position is off the picture, not an overflow)
    let x0 = match tx.align {
        1 => ax - (width / 2) as i64,
        2 => ax - width as i64,
        _ => ax,
    };
    let y0 = (tx.pos[1] * t.h as f32).round().clamp(-1.0e9, 1.0e9) as i64;
    let c = display_to_linear(tx.color);
    let passes: &[(i64, V3, f32, bool)] = if tx.shadow { &[(cell as i64, [0.0, 0.0, 0.0], 0.65, false), (0, [c[0], c[1], c[2]], 1.0, true)] } else { &[(0, [c[0], c[1], c[2]], 1.0, true)] };
    for &(off, color, a, claim) in passes {
        for (n, ch) in chars.iter().enumerate() {
            let g = glyph(*ch);
            for col in 0..GLYPH_W {
                for row in 0..GLYPH_H {
                    if lit(g, col, row) {
                        let x = x0 + off + (n * advance + col * cell) as i64;
                        let y = y0 + off + (row * cell) as i64;
                        fill_rect(t, x, y, cell, cell, color, a * c[3].clamp(0.0, 1.0), if claim { Some(uid) } else { None });
                    }
                }
            }
        }
    }
}

/// The part of a sprite sheet to show for `frame`: (u0, v0, du, dv), with v counted from the bottom like the quad's uv.
fn sheet_rect(sheet: [u32; 2], frame: u32) -> [f32; 4] {
    let (cols, rows) = (sheet[0].max(1), sheet[1].max(1));
    if cols == 1 && rows == 1 {
        return [0.0, 0.0, 1.0, 1.0];
    }
    let f = frame % (cols * rows);
    let (c, r) = (f % cols, f / cols); // row 0 is the top of the picture
    [c as f32 / cols as f32, 1.0 - (r + 1) as f32 / rows as f32, 1.0 / cols as f32, 1.0 / rows as f32]
}

/// A hash of everything `tilemap_mesh` depends on (never 0).
fn tile_key(m: &Tilemap, data: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |b: u8| {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for b in data.bytes() {
        eat(b);
    }
    for v in [m.size[0], m.size[1], m.sheet[0], m.sheet[1], m.tile.to_bits()] {
        for b in v.to_le_bytes() {
            eat(b);
        }
    }
    h | 1
}

/// One quad per non-empty cell, in the map's local XY plane, centred on the origin, with the uv of its tile baked in.
fn tilemap_mesh(m: &Tilemap, data: &str) -> Mesh {
    let mut mesh = Mesh { pos: Vec::new(), nrm: Vec::new(), uv: Vec::new(), col: Vec::new(), texw: Vec::new(), idx: Vec::new(), tex: None, rig: None };
    let (w, h) = (m.size[0].min(1024) as usize, m.size[1].min(1024) as usize);
    let tile = m.tile;
    if w == 0 || h == 0 || !tile.is_finite() || w * h > 262_144 {
        return mesh;
    }
    let numbers = data.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).map(|t| t.parse::<i64>().unwrap_or(-1));
    for (cell, n) in numbers.take(w * h).enumerate() {
        if n < 0 {
            continue;
        }
        let (c, r) = ((cell % w) as f32, (cell / w) as f32);
        let uv = sheet_rect(m.sheet, n.min(u32::MAX as i64) as u32);
        let (x0, y1) = ((c - w as f32 / 2.0) * tile, (h as f32 / 2.0 - r) * tile);
        let (x1, y0) = (x0 + tile, y1 - tile);
        let base = mesh.pos.len() as u32;
        for (x, y, u, v) in [(x0, y0, uv[0], uv[1]), (x1, y0, uv[0] + uv[2], uv[1]), (x1, y1, uv[0] + uv[2], uv[1] + uv[3]), (x0, y1, uv[0], uv[1] + uv[3])] {
            mesh.pos.push([x, y, 0.0]);
            mesh.nrm.push([0.0, 0.0, 1.0]);
            mesh.uv.push([u, v]);
        }
        mesh.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    mesh
}

/// One thing to draw. `rows` is the range of picture rows it can touch (None = unknown, treat as all).
struct Draw<'a> {
    uid: u64,
    rows: Option<(i64, i64)>,
    kind: DrawKind<'a>,
}

enum DrawKind<'a> {
    Mesh { mesh: &'a Mesh, model: M4, color: V3, shininess: u32, emissive: f32, tex: Option<&'a Texture>, uv_scale: [f32; 2] },
    Sprite { mesh: &'a Mesh, model: M4, color: [f32; 4], tex: Option<&'a Texture>, uv: [f32; 4] },
    Particles { mesh: &'a Mesh, tex: Option<&'a Texture>, additive: bool },
    Text { text: String, tx: Text },
    Rect(&'a SceneRect),
}

struct FullTarget {
    color: Vec<V3>,
    depth: Vec<f32>,
    ids: Vec<u64>,
}

/// The rows of the picture a mesh can cover, from its bounding box (conservative; None when any corner is not in front of the camera).
fn screen_rows(mesh: &Mesh, mvp: &M4, height: usize) -> Option<(i64, i64)> {
    if mesh.pos.is_empty() {
        return Some((0, 0));
    }
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for p in &mesh.pos {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let (mut y_min, mut y_max) = (f64::INFINITY, f64::NEG_INFINITY);
    for corner in 0..8 {
        let p = [if corner & 1 == 0 { lo[0] } else { hi[0] }, if corner & 2 == 0 { lo[1] } else { hi[1] }, if corner & 4 == 0 { lo[2] } else { hi[2] }];
        let c = mvp.mul_v4([p[0], p[1], p[2], 1.0]);
        if !(c[3] > 1e-4) || !c[1].is_finite() {
            return None;
        }
        let sy = (1.0 - (c[1] as f64 / c[3] as f64 * 0.5 + 0.5)) * height as f64;
        y_min = y_min.min(sy);
        y_max = y_max.max(sy);
    }
    if !y_min.is_finite() || !y_max.is_finite() {
        return None;
    }
    // clamped before the cast: a mesh a few astronomical units away must not overflow the arithmetic
    Some((y_min.floor().clamp(-1e9, 1e9) as i64 - 2, y_max.ceil().clamp(-1e9, 1e9) as i64 + 2))
}

/// Draw everything that touches the rows of this band, in order.
#[allow(clippy::too_many_arguments)]
fn draw_band(t: &mut Target<'_>, draws: &[Draw<'_>], view_proj: &M4, lights: &[LightData], eye: V3, ss: usize, final_height: usize) {
    for d in draws {
        if let Some((a, b)) = d.rows {
            if b <= t.y0 as i64 || a >= t.y1 as i64 {
                continue;
            }
        }
        match &d.kind {
            DrawKind::Mesh { mesh, model, color, shininess, emissive, tex, uv_scale } => {
                let shade = Shade::Lit { color: *color, shininess: *shininess, emissive: *emissive, tex: tex.or(mesh.tex.as_deref()), vcol: !mesh.col.is_empty(), uv_scale: *uv_scale };
                draw_mesh(t, mesh, model, view_proj, &shade, lights, eye, d.uid, true);
            }
            DrawKind::Sprite { mesh, model, color, tex, uv } => {
                let shade = Shade::Sprite { color: *color, tex: *tex, uv: *uv };
                draw_mesh(t, mesh, model, view_proj, &shade, lights, eye, d.uid, false);
            }
            DrawKind::Particles { mesh, tex, additive } => {
                let shade = Shade::Particle { tex: *tex, additive: *additive };
                draw_mesh(t, mesh, &M4::identity(), view_proj, &shade, lights, eye, d.uid, false);
            }
            DrawKind::Text { text, tx } => draw_text(t, d.uid, text, tx, ss, final_height),
            DrawKind::Rect(r) => {
                let th = t.h;
                draw_shape(t, r, th);
            }
        }
    }
}

/// A mesh a scene refers to: one of the built-in shapes (shared for ever), or a loaded model or generated tile map.
#[derive(Clone)]
pub enum MeshRef {
    Builtin(&'static Mesh),
    Shared(Arc<Mesh>),
    /// A model posed for this picture (an animated, skinned model): new every frame, so a renderer must not cache it by address.
    /// `key` is the entity's uid, which a renderer can use to reuse the buffers it filled for the same entity last time.
    Dynamic { key: u64, mesh: Arc<Mesh> },
}

impl MeshRef {
    pub fn get(&self) -> &Mesh {
        match self {
            MeshRef::Builtin(m) => m,
            MeshRef::Shared(m) => m,
            MeshRef::Dynamic { mesh, .. } => mesh,
        }
    }
}

/// A lit 3D mesh to draw.
pub struct SceneMesh {
    /// Water (GPU high qualities): wave height, wave length, phase.
    pub water: Option<[f32; 3]>,
    /// Leaves in the wind (GPU high qualities): amount, extent, phase.
    pub sway: Option<[f32; 3]>,
    pub uid: u64,
    pub mesh: MeshRef,
    pub model: M4,
    /// Linear colour.
    pub color: V3,
    pub shininess: u32,
    pub emissive: f32,
    /// Physically based material (the GPU renderer's high-quality mode): 0..1 metallic, and 0..1 roughness (already worked out from
    /// `shininess` when the component leaves it negative).
    pub metallic: f32,
    pub roughness: f32,
    pub cast_shadow: bool,
    /// Opacity (the colour's alpha): the GPU renderer's high-quality mode blends meshes with alpha below 1 with what is behind them;
    /// the reference renderer and the classic mode draw every mesh opaque.
    pub alpha: f32,
    /// Depth of the relief of a texture that has a height map, in world units.
    pub bump: f32,
    /// A base colour texture chosen on the component (it replaces the model's own), repeated `uv_scale` times.
    pub tex: Option<Arc<Texture>>,
    pub uv_scale: [f32; 2],
}

/// A sprite or a tile map: a flat textured mesh, alpha blended, drawn after the 3D meshes, far to near.
pub struct SceneLayer {
    pub uid: u64,
    pub mesh: MeshRef,
    pub model: M4,
    /// Linear colour and alpha.
    pub color: [f32; 4],
    pub tex: Option<Arc<Texture>>,
    /// The part of the texture to show: (u0, v0, du, dv), v counted from the bottom.
    pub uv: [f32; 4],
    /// 0 for the built-in quad; for a tile map, a hash of everything its mesh is made from, so a renderer can keep the mesh it
    /// uploaded while the map does not change.
    pub mesh_key: u64,
}

/// A cloud of particles (see `particles`): one mesh of camera-facing quads in world space, new for every picture.
pub struct SceneParticles {
    pub uid: u64,
    pub mesh: MeshRef,
    pub tex: Option<Arc<Texture>>,
    pub additive: bool,
}

/// A shape of the on-screen interface (`Ui`, and the letters of TrueType text), in fractions of the picture, drawn over the 3D picture and under the
/// bitmap-font texts, in the order of the list.
#[derive(Clone, Debug)]
pub struct SceneRect {
    pub uid: u64,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// Display colour and alpha (with a picture: what its texels are multiplied by).
    pub color: [f32; 4],
    /// With an alpha above 0 the colour at the bottom edge: a gradient from `color` at the top.
    pub color2: [f32; 4],
    /// Corner radius as a fraction of the picture's height.
    pub radius: f32,
    pub image: Option<UiImage>,
}

/// A picture on a `SceneRect`: the part `uv` (u0, v0, u1, v1, from the top left) of `tex`.
#[derive(Clone, Debug)]
pub struct UiImage {
    pub tex: Arc<Texture>,
    pub uv: [f32; 4],
    /// Bilinear filtering (otherwise the nearest texel: the letters of a font are always drawn that way).
    pub smooth: bool,
}

impl SceneRect {
    /// The whole-pixel box the rectangle covers on a picture of `w` x `h` pixels: (x0, y0, x1, y1), rounded the way both renderers round it.
    pub fn pixels(&self, w: usize, h: usize) -> (i64, i64, i64, i64) {
        let to_px = |v: f32, n: usize| (v * n as f32).round().clamp(-1.0e9, 1.0e9) as i64;
        (to_px(self.x0, w), to_px(self.y0, h), to_px(self.x1, w), to_px(self.y1, h))
    }

    /// Does it need more than a flat fill (rounded corners, a gradient, a picture)?
    pub fn is_styled(&self) -> bool {
        self.radius > 0.0 || self.color2[3] > 0.0 || self.image.is_some()
    }
}

/// HUD text, with its `{resource}` placeholders already filled in.
pub struct SceneText {
    pub uid: u64,
    pub text: String,
    pub tx: Text,
}

/// How the whole picture should look in the GPU renderer's high-quality mode (the reference CPU renderer ignores all of it). Colours are
/// display colours (0..1), like everything the user writes.
#[derive(Clone, Debug)]
pub struct EnvData {
    /// An `Environment` entity gave these values (otherwise they are worked out from the camera's clear colour).
    pub explicit: bool,
    pub sky: bool,
    pub sky_top: V3,
    pub sky_horizon: V3,
    pub ground: V3,
    pub ambient: f32,
    pub exposure: f32,
    pub clouds: f32,
    pub fog: f32,
    pub fog_color: V3,
    pub shadows: bool,
    pub shadow_distance: f32,
    pub ssao: f32,
    pub bloom: f32,
    pub bloom_threshold: f32,
    pub vignette: f32,
    pub saturation: f32,
    pub tonemap: u32,
    /// Depth of field: strength 0..1, the distance that is in focus and the depth around it that stays sharp.
    pub dof: f32,
    pub focus: f32,
    pub focus_range: f32,
    /// `PostFx`: light shafts, motion blur, film grain, contrast and tint.
    pub shafts: f32,
    pub motion_blur: f32,
    pub grain: f32,
    pub contrast: f32,
    pub tint: V3,
    /// `ColorGrade`: lift rgb + temperature, gamma rgb + tint shift, gain rgb, and the fade colour + amount (all identity by default).
    pub grade_a: [f32; 4],
    pub grade_b: [f32; 4],
    pub grade_c: [f32; 4],
    pub fade: [f32; 4],
    /// `HeightFog`: density, falloff, height (all 0 when there is none).
    pub hfog: [f32; 3],
}

impl EnvData {
    /// The look of a scene without an `Environment`: the flat clear colour behind, a soft ambient of about the brightness the reference
    /// renderer uses, a little bloom and ambient occlusion. A flat 2D scene (no 3D meshes) gets none of the effects, so that its colours
    /// come out exactly as authored.
    pub fn derived(clear: [f32; 4], flat_2d: bool) -> EnvData {
        let c = [clear[0], clear[1], clear[2]];
        let mix = |a: V3, b: V3, t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
        let sky = mix(c, [0.52, 0.55, 0.62], 0.7);
        EnvData { explicit: false, sky: false, sky_top: sky, sky_horizon: scale(sky, 0.95), ground: scale(sky, 0.55), ambient: 1.0, exposure: 1.0, clouds: 0.0, fog: 0.0, fog_color: c, shadows: true, shadow_distance: 80.0, ssao: if flat_2d { 0.0 } else { 0.7 }, bloom: if flat_2d { 0.0 } else { 0.2 }, bloom_threshold: 1.0, vignette: if flat_2d { 0.0 } else { 0.15 }, saturation: 1.0, tonemap: 0, dof: 0.0, focus: 10.0, focus_range: 4.0, shafts: 0.0, motion_blur: 0.0, grain: 0.0, contrast: 1.0, tint: [1.0; 3], grade_a: [0.0; 4], grade_b: [1.0, 1.0, 1.0, 0.0], grade_c: [1.0, 1.0, 1.0, 0.0], fade: [0.0; 4], hfog: [0.0; 3] }
    }

    fn from_component(e: &Environment) -> EnvData {
        let nz = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        let c3 = |v: [f32; 3]| [nz(v[0], 0.0, 1.0), nz(v[1], 0.0, 1.0), nz(v[2], 0.0, 1.0)];
        EnvData {
            explicit: true,
            sky: e.sky,
            sky_top: c3(e.sky_top),
            sky_horizon: c3(e.sky_horizon),
            ground: c3(e.ground),
            ambient: nz(e.ambient, 0.0, 16.0),
            exposure: nz(e.exposure, 0.0, 64.0),
            clouds: nz(e.clouds, 0.0, 1.0),
            fog: nz(e.fog, 0.0, 10.0),
            fog_color: c3(e.fog_color),
            shadows: e.shadows,
            shadow_distance: nz(e.shadow_distance, 1.0, 2000.0),
            ssao: nz(e.ssao, 0.0, 2.0),
            bloom: nz(e.bloom, 0.0, 2.0),
            bloom_threshold: nz(e.bloom_threshold, 0.0, 64.0),
            vignette: nz(e.vignette, 0.0, 1.0),
            saturation: nz(e.saturation, 0.0, 4.0),
            tonemap: e.tonemap.min(3),
            dof: nz(e.dof, 0.0, 1.0),
            focus: nz(e.focus, 0.1, 100_000.0),
            focus_range: nz(e.focus_range, 0.0, 100_000.0),
            shafts: 0.0,
            motion_blur: 0.0,
            grain: 0.0,
            contrast: 1.0,
            tint: [1.0; 3],
            grade_a: [0.0; 4],
            grade_b: [1.0, 1.0, 1.0, 0.0],
            grade_c: [1.0, 1.0, 1.0, 0.0],
            fade: [0.0; 4],
            hfog: [0.0; 3],
        }
    }
}

/// Everything needed to draw one picture, gathered once and sorted so the result never depends on how the world is stored. Both the
/// reference CPU renderer and the GPU renderer draw from this, so they always agree about what is in the picture.
pub struct Scene {
    pub cam: Cam,
    pub view: M4,
    pub proj: M4,
    pub view_proj: M4,
    pub lights: Vec<LightData>,
    pub meshes: Vec<SceneMesh>,
    pub layers: Vec<SceneLayer>,
    pub particles: Vec<SceneParticles>,
    pub texts: Vec<SceneText>,
    pub rects: Vec<SceneRect>,
    pub env: EnvData,
    pub warnings: Vec<String>,
}

/// Collect the camera, lights, meshes, layers and texts for a picture of `opts.width` x `opts.height`. Models and textures are loaded
/// here (they need the caches); problems that do not stop the picture (an unknown mesh name) become warnings.
pub fn gather_scene(world: &mut World, textures: &mut TextureCache, opts: &RenderOptions) -> Result<Scene> {
    // the pictures of `CameraView`s first (not while drawing one)
    if !textures.nested {
        crate::views::update(world, textures, opts.threads)?;
    }
    // ---- gather (sorted by uid so the picture never depends on storage order)
    let mut cams: Vec<(u64, Camera, Transform)> = Vec::new();
    world.for_each_uid::<(&Camera, &Transform), _>(|_, uid, (c, t)| cams.push((uid, *c, *t)))?;
    cams.sort_by_key(|c| c.0);
    let mut lights_raw: Vec<(u64, Light, Transform)> = Vec::new();
    world.for_each_uid::<(&Light, &Transform), _>(|_, uid, (l, t)| lights_raw.push((uid, *l, *t)))?;
    lights_raw.sort_by_key(|c| c.0);
    let mut meshes_raw: Vec<(u64, MeshRenderer, Transform)> = Vec::new();
    world.for_each_uid::<(&MeshRenderer, &Transform), _>(|_, uid, (m, t)| meshes_raw.push((uid, *m, *t)))?;
    meshes_raw.sort_by_key(|c| c.0);
    let mut sprites_raw: Vec<(u64, Sprite, Transform)> = Vec::new();
    world.for_each_uid::<(&Sprite, &Transform), _>(|_, uid, (s, t)| sprites_raw.push((uid, *s, *t)))?;
    sprites_raw.sort_by_key(|c| c.0);

    // joints that have a width: a cord from the body to the other end
    let mut cords: Vec<(u64, Joint, [f32; 3])> = Vec::new();
    world.for_each_uid::<(&Joint, &Transform), _>(|_, uid, (j, t)| {
        if j.width > 0.0 && j.width.is_finite() {
            cords.push((uid, *j, t.pos));
        }
    })?;
    cords.sort_by_key(|c| c.0);
    let mut terrains_raw: Vec<(u64, Terrain, Transform)> = Vec::new();
    world.for_each_uid::<(&Terrain, &Transform), _>(|_, uid, (g, t)| terrains_raw.push((uid, *g, *t)))?;
    terrains_raw.sort_by_key(|c| c.0);

    let mut tilemaps_raw: Vec<(u64, Tilemap, Transform)> = Vec::new();
    world.for_each_uid::<(&Tilemap, &Transform), _>(|_, uid, (m, t)| tilemaps_raw.push((uid, *m, *t)))?;
    tilemaps_raw.sort_by_key(|c| c.0);
    // inverse kinematics goals by the entity of the model they bend: (goal entity, goal, where it is)
    let mut ik_by_skin: std::collections::BTreeMap<u64, Vec<(u64, Ik, V3)>> = std::collections::BTreeMap::new();
    world.for_each_uid::<(&Ik, &Transform), _>(|_, uid, (g, t)| {
        if g.active && g.weight > 0.0 && g.skin.0 != 0 {
            ik_by_skin.entry(g.skin.0).or_default().push((uid, *g, t.pos));
        }
    })?;
    for goals in ik_by_skin.values_mut() {
        goals.sort_by_key(|g| g.0);
    }
    let mut anim_layers_raw: std::collections::BTreeMap<u64, AnimLayers> = std::collections::BTreeMap::new();
    world.for_each_uid::<(&AnimLayers,), _>(|_, uid, (l,)| {
        if (l.weight_b > 0.0 && l.weight_b.is_finite()) || (l.weight_c > 0.0 && l.weight_c.is_finite()) {
            anim_layers_raw.insert(uid, *l);
        }
    })?;
    let mut sways_raw: std::collections::BTreeMap<u64, lunaris_sim::components::Sway> = std::collections::BTreeMap::new();
    world.for_each_uid::<(&lunaris_sim::components::Sway,), _>(|_, uid, (w,)| {
        if w.amount.is_finite() && w.amount != 0.0 {
            sways_raw.insert(uid, *w);
        }
    })?;
    let mut animators_raw: std::collections::BTreeMap<u64, Animator> = std::collections::BTreeMap::new();
    world.for_each_uid::<(&Animator,), _>(|_, uid, (a,)| {
        animators_raw.insert(uid, *a);
    })?;
    let mut texts_raw: Vec<(u64, Text)> = Vec::new();
    world.for_each_uid::<(&Text,), _>(|_, uid, (t,)| texts_raw.push((uid, *t)))?;
    texts_raw.sort_by_key(|c| c.0);
    let mut uis_raw: Vec<(u64, Ui)> = Vec::new();
    world.for_each_uid::<(&Ui,), _>(|_, uid, (u,)| uis_raw.push((uid, *u)))?;
    uis_raw.sort_by_key(|c| (c.1.layer, c.0));
    let mut envs_raw: Vec<(u64, Environment)> = Vec::new();
    world.for_each_uid::<(&Environment,), _>(|_, uid, (e,)| envs_raw.push((uid, *e)))?;
    envs_raw.sort_by_key(|c| c.0);

    // ---- camera
    let mut warnings: Vec<String> = Vec::new();
    let cam = match &opts.view {
        View::Explicit(e) => Cam {
            pos: e.eye,
            rot: quat_look_at(e.eye, e.target, e.up),
            fov_y: e.fov_y,
            near: e.near,
            far: e.far,
            ortho: e.ortho_height,
            clear: e.clear,
            desc: format!("explicit view from {:?} towards {:?}", e.eye, e.target),
        },
        View::World(wanted) => {
            let chosen = match wanted {
                Some(uid) => Some(cams.iter().find(|c| c.0 == *uid).ok_or_else(|| EcsError::new("no_such_camera", format!("entity {uid} has no Camera and Transform")).hint("add a Camera component, or omit `camera` to use the default"))?),
                None => cams.iter().find(|c| c.1.active),
            };
            match chosen {
                Some((uid, c, t)) => Cam {
                    pos: t.pos,
                    rot: t.rot,
                    fov_y: c.fov_y,
                    near: c.near,
                    far: c.far,
                    ortho: if c.ortho { Some(c.ortho_height) } else { None },
                    clear: c.clear,
                    desc: format!("camera entity {uid}"),
                },
                None => {
                    let d = ExplicitCamera::default();
                    warnings.push("no active Camera entity: showing a default view (add a Camera, or pass eye/target)".to_string());
                    Cam { pos: d.eye, rot: quat_look_at(d.eye, d.target, d.up), fov_y: d.fov_y, near: d.near, far: d.far, ortho: None, clear: d.clear, desc: "default view".to_string() }
                }
            }
        }
    };
    if !(cam.near > 0.0 && cam.far > cam.near) && cam.ortho.is_none() {
        return Err(EcsError::new("bad_camera", "a perspective camera needs 0 < near < far").at("camera"));
    }
    let mut env = match envs_raw.first() {
        Some((_, e)) => EnvData::from_component(e),
        None => EnvData::derived(cam.clear, meshes_raw.iter().all(|(_, m, _)| !m.visible)),
    };
    {
        let mut fx: Vec<(u64, lunaris_sim::components::PostFx)> = Vec::new();
        world.for_each_uid::<(&lunaris_sim::components::PostFx,), _>(|_, uid, (f,)| fx.push((uid, *f)))?;
        if let Some((_, f)) = fx.into_iter().min_by_key(|x| x.0) {
            let nz = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
            env.shafts = nz(f.shafts, 0.0, 2.0);
            env.motion_blur = nz(f.motion_blur, 0.0, 1.0);
            env.grain = nz(f.grain, 0.0, 1.0);
            env.contrast = nz(f.contrast, 0.0, 4.0);
            env.tint = [nz(f.tint[0], 0.0, 4.0), nz(f.tint[1], 0.0, 4.0), nz(f.tint[2], 0.0, 4.0)];
        }
    }
    {
        let mut grades: Vec<(u64, lunaris_sim::components::ColorGrade)> = Vec::new();
        world.for_each_uid::<(&lunaris_sim::components::ColorGrade,), _>(|_, uid, (g,)| grades.push((uid, *g)))?;
        if let Some((_, g)) = grades.into_iter().min_by_key(|x| x.0) {
            let nz = |v: f32, lo: f32, hi: f32, d: f32| if v.is_finite() { v.clamp(lo, hi) } else { d };
            env.grade_a = [nz(g.lift[0], -1.0, 1.0, 0.0), nz(g.lift[1], -1.0, 1.0, 0.0), nz(g.lift[2], -1.0, 1.0, 0.0), nz(g.temperature, -1.0, 1.0, 0.0)];
            env.grade_b = [nz(g.gamma[0], 0.1, 4.0, 1.0), nz(g.gamma[1], 0.1, 4.0, 1.0), nz(g.gamma[2], 0.1, 4.0, 1.0), nz(g.tint_shift, -1.0, 1.0, 0.0)];
            env.grade_c = [nz(g.gain[0], 0.0, 8.0, 1.0), nz(g.gain[1], 0.0, 8.0, 1.0), nz(g.gain[2], 0.0, 8.0, 1.0), 0.0];
            env.fade = [nz(g.fade[0], 0.0, 1.0, 0.0), nz(g.fade[1], 0.0, 1.0, 0.0), nz(g.fade[2], 0.0, 1.0, 0.0), nz(g.fade[3], 0.0, 1.0, 0.0)];
        }
    }
    {
        let mut fogs: Vec<(u64, lunaris_sim::components::HeightFog)> = Vec::new();
        world.for_each_uid::<(&lunaris_sim::components::HeightFog,), _>(|_, uid, (f,)| fogs.push((uid, *f)))?;
        if let Some((_, f)) = fogs.into_iter().min_by_key(|x| x.0) {
            let nz = |v: f32, lo: f32, hi: f32, d: f32| if v.is_finite() { v.clamp(lo, hi) } else { d };
            env.hfog = [nz(f.density, 0.0, 10.0, 0.0), nz(f.falloff, 0.0001, 10.0, 0.15), nz(f.height, -100_000.0, 100_000.0, 0.0)];
            env.fog_color = [nz(f.color[0], 0.0, 1.0, 0.72), nz(f.color[1], 0.0, 1.0, 0.82), nz(f.color[2], 0.0, 1.0, 0.95)];
        }
    }
    let aspect = opts.width as f32 / opts.height as f32;
    let proj = match cam.ortho {
        Some(h) => M4::ortho(h.max(1e-6), aspect, cam.near, cam.far),
        None => M4::perspective(cam.fov_y.clamp(1.0, 170.0), aspect, cam.near, cam.far),
    };
    let view = M4::view(cam.pos, cam.rot);
    let view_proj = proj.mul(&view);

    // ---- lights
    let mut lights: Vec<LightData> = lights_raw
        .iter()
        .take(16)
        .map(|(_, l, t)| {
            let color = scale(l.color, l.intensity);
            if l.kind >= 1 {
                let spot = l.kind == 2;
                let outer = l.angle.clamp(1.0, 89.0).to_radians();
                let inner = outer * (1.0 - l.softness.clamp(0.01, 1.0));
                LightData { vec: t.pos, point: true, color, range: l.range.max(1e-3), spot_dir: normalize(rotate(t.rot, [0.0, 0.0, -1.0])), cos_outer: if spot { outer.cos() } else { -2.0 }, cos_inner: inner.cos() }
            } else {
                LightData { vec: scale(rotate(t.rot, [0.0, 0.0, -1.0]), -1.0), point: false, color, range: 1.0, spot_dir: [0.0, -1.0, 0.0], cos_outer: -2.0, cos_inner: -2.0 }
            }
        })
        .collect();
    if lights.is_empty() {
        lights.push(LightData { vec: normalize([-0.4, 0.8, 0.6]), point: false, color: [1.0, 1.0, 1.0], range: 1.0, spot_dir: [0.0, -1.0, 0.0], cos_outer: -2.0, cos_inner: -2.0 });
        lights.push(LightData { vec: normalize([0.7, 0.2, 0.3]), point: false, color: [0.3, 0.3, 0.35], range: 1.0, spot_dir: [0.0, -1.0, 0.0], cos_outer: -2.0, cos_inner: -2.0 });
    }

    // ---- what to draw, in drawing order: meshes by uid, sprites and tile maps far to near, then text
    // models and textures are loaded here (they need the caches)
    let mut model_cache: std::collections::HashMap<String, Option<Arc<Mesh>>> = std::collections::HashMap::new();
    let mut lods: std::collections::BTreeMap<u64, lunaris_sim::components::Lod> = std::collections::BTreeMap::new();
    world.for_each_uid::<(&lunaris_sim::components::Lod,), _>(|_, uid, (l,)| {
        lods.insert(uid, *l);
    })?;
    let mut hidden: Vec<bool> = Vec::with_capacity(meshes_raw.len());
    let mut mesh_names: Vec<String> = Vec::with_capacity(meshes_raw.len());
    for (uid, m, tr) in &meshes_raw {
        let mut name = world.strings().get(m.mesh.0).to_string();
        let mut hide = false;
        if let Some(l) = lods.get(uid) {
            // level of detail: by the distance from the camera to the entity
            let d = ((tr.pos[0] - cam.pos[0]).powi(2) + (tr.pos[1] - cam.pos[1]).powi(2) + (tr.pos[2] - cam.pos[2]).powi(2)).sqrt();
            if l.cull > 0.0 && d > l.cull {
                hide = true;
            } else {
                let pick = |sym: lunaris_ecs::Sym| world.strings().get(sym.0).to_string();
                if d >= l.distance2 && l.mesh2.0 != 0 {
                    name = pick(l.mesh2);
                } else if d >= l.distance1 && l.mesh1.0 != 0 {
                    name = pick(l.mesh1);
                }
            }
        }
        hidden.push(hide);
        if m.visible && builtin(&name).is_none() && is_model_name(&name) && !model_cache.contains_key(&name) {
            let loaded = textures.model(&name);
            model_cache.insert(name.clone(), loaded);
        }
        mesh_names.push(name);
    }
    let quad = builtin("quad").expect("quad exists");
    // sprites and tile maps are layers drawn far to near on top of the 3D scene
    enum Layer {
        Sprite(Sprite, Transform),
        Tiles(Tilemap, Transform, Arc<Mesh>, u64),
    }
    let mut layers: Vec<(f32, u64, Layer)> = Vec::new();
    // parallax: a layer is drawn shifted against the camera's movement, as if it lay at another depth
    let mut parallax: std::collections::BTreeMap<u64, Parallax> = std::collections::BTreeMap::new();
    world.for_each_uid::<(&Parallax,), _>(|_, uid, (p,)| {
        parallax.insert(uid, *p);
    })?;
    // decals lie on surfaces as sprites turned to face their normal
    let mut decals: Vec<(u64, Decal, Transform)> = Vec::new();
    world.for_each_uid::<(&Decal, &Transform), _>(|_, uid, (d, t)| {
        if d.visible && d.size.iter().all(|v| v.is_finite()) && d.normal.iter().all(|v| v.is_finite()) && d.color.iter().all(|v| v.is_finite()) && d.roll.is_finite() {
            decals.push((uid, *d, *t));
        }
    })?;
    decals.sort_by_key(|d| d.0);
    for (uid, d, t) in decals {
        let n = normalize(d.normal);
        let n = if length(n) < 0.5 { [0.0, 1.0, 0.0] } else { n };
        // the rotation that turns the sprite's +z to the normal, then the roll about the normal
        let (c, a) = (dot([0.0, 0.0, 1.0], n), cross([0.0, 0.0, 1.0], n));
        let swing = if c < -0.999_999 { [1.0, 0.0, 0.0, 0.0] } else { quat_normalize([a[0], a[1], a[2], 1.0 + c]) };
        let rot = quat_mul(quat_axis_angle(n, d.roll), swing);
        let mut alpha = d.color[3].clamp(0.0, 1.0);
        if d.life > 0.0 && d.fade > 0.0 {
            alpha *= ((d.life - d.age) / d.fade).clamp(0.0, 1.0);
        }
        let sprite = Sprite { size: d.size, color: [d.color[0], d.color[1], d.color[2], alpha], texture: if d.texture.0 == 0 { lunaris_ecs::Sym(world.strings_mut().intern("soft")) } else { d.texture }, visible: true, sheet: [1, 1], frame: 0 };
        let placed = Transform { pos: add(t.pos, scale(n, 0.01)), rot, scale: [1.0, 1.0, 1.0] };
        if alpha > 0.0 {
            sprites_raw.push((uid, sprite, placed));
        }
    }
    let shifted = |uid: u64, mut t: Transform| {
        if let Some(p) = parallax.get(&uid).filter(|p| p.factor.iter().chain(&p.origin).all(|v| v.is_finite())) {
            t.pos[0] += (cam.pos[0] - p.origin[0]) * p.factor[0];
            t.pos[1] += (cam.pos[1] - p.origin[1]) * p.factor[1];
        }
        t
    };
    for (uid, s, t) in sprites_raw.into_iter().filter(|(_, s, _)| s.visible) {
        let t = shifted(uid, t);
        layers.push((view.transform_point(t.pos)[2], uid, Layer::Sprite(s, t))); // z is negative in front of the camera
    }
    for (uid, m, t) in tilemaps_raw.into_iter().filter(|(_, m, _)| m.visible) {
        let t = shifted(uid, t);
        let data = world.strings().get(m.data.0).to_string();
        let mesh = tilemap_mesh(&m, &data);
        if mesh.idx.is_empty() {
            continue;
        }
        let key = tile_key(&m, &data);
        layers.push((view.transform_point(t.pos)[2], uid, Layer::Tiles(m, t, Arc::new(mesh), key)));
    }
    layers.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    let mut layer_tex: Vec<Option<Arc<Texture>>> = Vec::with_capacity(layers.len());
    for (_, _, l) in &layers {
        let sym = match l {
            Layer::Sprite(s, _) => s.texture.0,
            Layer::Tiles(m, _, _, _) => m.texture.0,
        };
        let name = world.strings().get(sym).to_string();
        layer_tex.push(if name.is_empty() { None } else { Some(textures.get(&name)) });
    }

    let mut scene_meshes: Vec<SceneMesh> = Vec::new();
    for (((uid, m, tr), name), hide) in meshes_raw.iter().zip(&mesh_names).zip(&hidden) {
        if !m.visible || *hide {
            continue;
        }
        let mesh = match builtin(name) {
            Some(b) => MeshRef::Builtin(b),
            None => match model_cache.get(name).and_then(|m| m.clone()) {
                // an animated model: pose its skeleton for this picture
                Some(shared) if shared.rig.is_some() && animators_raw.contains_key(uid) => {
                    let rig = shared.rig.as_ref().expect("checked");
                    let a = animators_raw[uid];
                    let clip_name = world.strings().get(a.clip.0).to_string();
                    let clip = if clip_name.is_empty() { (!rig.clips.is_empty()).then_some(0) } else { rig.clip_index(&clip_name) };
                    if clip.is_none() && !clip_name.is_empty() {
                        let msg = format!("entity {uid}: the model `{name}` has no animation called `{clip_name}` (it has: {})", rig.clips.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "));
                        if !warnings.contains(&msg) {
                            warnings.push(msg);
                        }
                    }
                    // inverse kinematics goals of this model, in the model's own space
                    let mut goals: Vec<crate::rig::IkGoal> = Vec::new();
                    if let Some(list) = ik_by_skin.get(uid) {
                        use crate::rig::{IkGoal, IkKind};
                        let inv = M4::from_trs(tr.pos, tr.rot, tr.scale).inverse_affine();
                        for (gid, ik, gpos) in list {
                            let joint_name = world.strings().get(ik.joint.0).to_string();
                            let Some(joint) = rig.joint_index(&joint_name) else {
                                let msg = format!("entity {gid}: the model `{name}` has no joint called `{joint_name}` for Ik (it has: {})", rig.joints.iter().map(|j| j.name.as_str()).collect::<Vec<_>>().join(", "));
                                if !warnings.contains(&msg) {
                                    warnings.push(msg);
                                }
                                continue;
                            };
                            let pole = (ik.pole.0 != 0).then(|| world.entity(ik.pole.0).and_then(|e| world.get::<Transform>(e)).map(|t| inv.transform_point(t.pos))).flatten();
                            let kind = if ik.kind == 1 { IkKind::Look { axis: ik.axis, max_angle: if ik.max_angle.is_finite() { ik.max_angle } else { 80.0 } } } else { IkKind::Reach };
                            goals.push(IkGoal { joint, kind, target: inv.transform_point(*gpos), pole, weight: ik.weight.min(1.0) });
                        }
                    }
                    // extra clips laid over the base (blend trees and layers): the base (a crossfade, if one is running) first, then each extra clip
                    let mesh = if anim_layers_raw.contains_key(uid) || !goals.is_empty() {
                        use crate::rig::PoseLayer;
                        let mut stack: Vec<PoseLayer> = Vec::new();
                        if a.blend < 1.0 {
                            let prev_name = world.strings().get(a.prev_clip.0).to_string();
                            let prev = if prev_name.is_empty() { (!rig.clips.is_empty()).then_some(0) } else { rig.clip_index(&prev_name) };
                            stack.push(PoseLayer { clip: prev, time: a.prev_time, looping: a.looping, weight: 1.0, root: None });
                            stack.push(PoseLayer { clip, time: a.time, looping: a.looping, weight: a.blend, root: None });
                        } else {
                            stack.push(PoseLayer { clip, time: a.time, looping: a.looping, weight: 1.0, root: None });
                        }
                        let base_len = clip.map_or(0.0, |c| rig.clips[c].duration);
                        let extras: Vec<(u32, f32, u32, f32, bool)> = anim_layers_raw.get(uid).map(|l| vec![(l.clip_b.0, l.weight_b, l.root_b.0, l.time_b, l.sync_b), (l.clip_c.0, l.weight_c, l.root_c.0, l.time_c, l.sync_c)]).unwrap_or_default();
                        for (sym, weight, root_sym, time, sync) in extras {
                            let extra = world.strings().get(sym).to_string();
                            if !(weight > 0.0) || extra.is_empty() {
                                continue;
                            }
                            let Some(ci) = rig.clip_index(&extra) else {
                                let msg = format!("entity {uid}: the model `{name}` has no animation called `{extra}` for AnimLayers (it has: {})", rig.clips.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "));
                                if !warnings.contains(&msg) {
                                    warnings.push(msg);
                                }
                                continue;
                            };
                            let root_name = world.strings().get(root_sym).to_string();
                            let root = if root_name.is_empty() { None } else { rig.joint_index(&root_name) };
                            if !root_name.is_empty() && root.is_none() {
                                let msg = format!("entity {uid}: the model `{name}` has no joint called `{root_name}` for AnimLayers");
                                if !warnings.contains(&msg) {
                                    warnings.push(msg);
                                }
                                continue;
                            }
                            let t = if sync && base_len > 0.0 { a.time / base_len * rig.clips[ci].duration } else { time };
                            stack.push(PoseLayer { clip: Some(ci), time: t, looping: a.looping, weight, root });
                        }
                        if goals.is_empty() {
                            rig.pose_layers(&shared, &stack)
                        } else {
                            rig.pose_layers_ik(&shared, &stack, &goals)
                        }
                    } else if a.blend < 1.0 {
                        let prev_name = world.strings().get(a.prev_clip.0).to_string();
                        let prev = if prev_name.is_empty() { (!rig.clips.is_empty()).then_some(0) } else { rig.clip_index(&prev_name) };
                        rig.pose_blend(&shared, (prev, a.prev_time), (clip, a.time), a.looping, a.blend)
                    } else {
                        rig.pose(&shared, clip, a.time, a.looping)
                    };
                    MeshRef::Dynamic { key: *uid, mesh: Arc::new(mesh) }
                }
                Some(shared) => MeshRef::Shared(shared),
                None => {
                    let msg = if name.is_empty() {
                        format!("entity {uid}: MeshRenderer.mesh is empty (try cube, sphere, plane, cylinder or quad, or a .glb file)")
                    } else if is_model_name(name) {
                        format!("entity {uid}: the model `{name}` could not be loaded (see the model warning)")
                    } else {
                        format!("entity {uid}: unknown mesh `{name}` (built-ins: cube, sphere, plane, cylinder, quad; or a .glb / .gltf path)")
                    };
                    if !warnings.contains(&msg) {
                        warnings.push(msg);
                    }
                    continue;
                }
            },
        };
        let c = display_to_linear(m.color);
        let shininess = m.shininess.max(1.0);
        // a negative roughness means "work it out from the shininess": a Blinn-Phong exponent n looks like a GGX distribution of width
        // alpha = sqrt(2 / (n + 2)), and the roughness is the square root of alpha (alpha = roughness squared)
        let roughness = if m.roughness >= 0.0 && m.roughness.is_finite() { m.roughness.min(1.0) } else { (2.0 / (shininess + 2.0)).sqrt().sqrt().clamp(0.05, 1.0) };
        let tex_name = world.strings().get(m.texture.0).to_string();
        let tex = if tex_name.is_empty() { None } else { Some(textures.get(&tex_name)) };
        let uv_scale = if m.uv_scale[0].is_finite() && m.uv_scale[1].is_finite() { m.uv_scale } else { [1.0, 1.0] };
        scene_meshes.push(SceneMesh {
            water: None,
            sway: sways_raw.get(uid).map(|w| [w.amount.clamp(-5.0, 5.0), if w.extent.is_finite() { w.extent.max(0.1) } else { 3.0 }, w.phase + w.offset]),
            uid: *uid,
            mesh,
            model: M4::from_trs(tr.pos, tr.rot, tr.scale),
            color: [c[0], c[1], c[2]],
            shininess: shininess as u32,
            emissive: m.emissive.clamp(0.0, 16.0),
            metallic: if m.metallic.is_finite() { m.metallic.clamp(0.0, 1.0) } else { 0.0 },
            roughness,
            cast_shadow: m.cast_shadow,
            alpha: if m.color[3].is_finite() { m.color[3].clamp(0.0, 1.0) } else { 1.0 },
            bump: if m.bump.is_finite() { m.bump.clamp(0.0, 1.0) } else { 0.0 },
            tex,
            uv_scale,
        });
    }
    // water: a flat grid scaled to the sheet, see-through, with the wave numbers for the GPU renderer
    let mut waters: Vec<(u64, Water, Transform)> = Vec::new();
    world.for_each_uid::<(&Water, &Transform), _>(|_, uid, (w, t)| waters.push((uid, *w, *t)))?;
    waters.sort_by_key(|w| w.0);
    for (uid, w, tr) in &waters {
        if !w.size.iter().all(|v| v.is_finite() && *v > 0.0) {
            continue;
        }
        let n = ((w.size[0].max(w.size[1]) / (w.wave_length.max(0.5) / 6.0)) as usize).clamp(16, 400);
        let mesh = textures.water_grid(n);
        let c = display_to_linear(w.color);
        scene_meshes.push(SceneMesh {
            water: if w.wave_height > 0.0 && w.wave_height.is_finite() { Some([w.wave_height.min(10.0), w.wave_length.max(0.2), w.phase]) } else { None },
            sway: None,
            uid: *uid,
            mesh: MeshRef::Shared(mesh),
            model: M4::from_trs(tr.pos, [0.0, 0.0, 0.0, 1.0], [w.size[0], 1.0, w.size[1]]),
            color: [c[0], c[1], c[2]],
            shininess: 90,
            emissive: 0.0,
            metallic: 0.0,
            roughness: 0.04,
            cast_shadow: false,
            alpha: if w.color[3].is_finite() { w.color[3].clamp(0.0, 1.0) } else { 0.8 },
            bump: 0.0,
            tex: None,
            uv_scale: [1.0, 1.0],
        });
    }
    // terrain: the mesh of each heightmap, drawn like any model (the ground lies flat: the entity's rotation and scale are not used)
    for (uid, g, tr) in &terrains_raw {
        if !g.visible {
            continue;
        }
        let name = world.strings().get(g.heightmap.0).to_string();
        if name.is_empty() {
            continue;
        }
        let Some(mesh) = textures.terrain_mesh(&name, g) else { continue };
        let c = display_to_linear(g.color);
        let tex_name = world.strings().get(g.texture.0).to_string();
        let tex = if tex_name.is_empty() { None } else { Some(textures.get(&tex_name)) };
        scene_meshes.push(SceneMesh {
            water: None,
            sway: None,
            uid: *uid,
            mesh: MeshRef::Shared(mesh),
            model: M4::from_trs(tr.pos, [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0]),
            color: [c[0], c[1], c[2]],
            shininess: 6,
            emissive: 0.0,
            metallic: 0.0,
            roughness: 0.92,
            cast_shadow: true,
            alpha: if g.color[3].is_finite() { g.color[3].clamp(0.0, 1.0) } else { 1.0 },
            bump: 0.0,
            tex,
            uv_scale: if g.uv_scale[0].is_finite() && g.uv_scale[1].is_finite() { g.uv_scale } else { [1.0, 1.0] },
        });
    }
    for (uid, j, from) in &cords {
        let to = if j.target.0 != 0 { world.entity(j.target.0).and_then(|e| world.get::<Transform>(e)).map(|t| t.pos) } else { Some(j.anchor) };
        let Some(to) = to.filter(|p| p.iter().all(|v| v.is_finite())) else { continue };
        let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len < 1.0e-4 {
            continue;
        }
        let n = [d[0] / len, d[1] / len, d[2] / len];
        // the cylinder's axis is y: turn it to run along n
        let rot = if n[1] < -0.9999 { [1.0, 0.0, 0.0, 0.0] } else { [n[2], 0.0, -n[0], 1.0 + n[1]] };
        let c = display_to_linear(j.color);
        scene_meshes.push(SceneMesh {
            water: None,
            sway: None,
            uid: *uid,
            mesh: MeshRef::Builtin(builtin("cylinder").expect("built-in")),
            model: M4::from_trs([(from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5, (from[2] + to[2]) * 0.5], rot, [j.width, len, j.width]),
            color: [c[0], c[1], c[2]],
            shininess: 8,
            emissive: 0.0,
            metallic: 0.0,
            roughness: 0.85,
            cast_shadow: true,
            alpha: if j.color[3].is_finite() { j.color[3].clamp(0.0, 1.0) } else { 1.0 },
            bump: 0.0,
            tex: None,
            uv_scale: [1.0, 1.0],
        });
    }
    let mut scene_layers: Vec<SceneLayer> = Vec::with_capacity(layers.len());
    for ((_, uid, layer), tex) in layers.into_iter().zip(layer_tex) {
        match layer {
            Layer::Sprite(s, tr) => scene_layers.push(SceneLayer {
                uid,
                mesh: MeshRef::Builtin(quad),
                model: M4::from_trs(tr.pos, tr.rot, [tr.scale[0] * s.size[0], tr.scale[1] * s.size[1], tr.scale[2]]),
                color: display_to_linear(s.color),
                tex,
                uv: sheet_rect(s.sheet, s.frame),
                mesh_key: 0,
            }),
            Layer::Tiles(m, tr, mesh, key) => scene_layers.push(SceneLayer { uid, mesh: MeshRef::Shared(mesh), model: M4::from_trs(tr.pos, tr.rot, tr.scale), color: display_to_linear(m.color), tex, uv: [0.0, 0.0, 1.0, 1.0], mesh_key: key }),
        }
    }
    // `{#key}` in a text is the entry of the language file of the first `Localization`
    let mut languages: Vec<(u64, u32)> = Vec::new();
    world.for_each_uid::<(&Localization,), _>(|_, uid, (l,)| languages.push((uid, l.language.0)))?;
    let table = match languages.iter().min_by_key(|l| l.0) {
        Some((_, sym)) => {
            let name = world.strings().get(*sym).to_string();
            textures.translations(&name)
        }
        None => Default::default(),
    };
    let say = |label: String| if label.contains("{#") { crate::font::translate(&label, |k| table.get(k).cloned()) } else { label };
    // the on-screen interface: shapes (rectangles, rounded ones, gradients, pictures, the letters of TrueType text) and bitmap-font texts, in drawing order
    let (pw, ph) = (opts.width.max(1) as f32, opts.height.max(1) as f32);
    let mut styles: std::collections::BTreeMap<u64, UiStyle> = Default::default();
    world.for_each_uid::<(&UiStyle,), _>(|_, uid, (st,)| {
        styles.insert(uid, *st);
    })?;
    let mut anchors: std::collections::BTreeMap<u64, UiAnchor> = Default::default();
    world.for_each_uid::<(&UiAnchor,), _>(|_, uid, (a,)| {
        anchors.insert(uid, *a);
    })?;
    let mut font_syms: Vec<(u64, u32, f32, f32)> = Vec::new();
    world.for_each_uid::<(&TextFont,), _>(|_, uid, (f,)| font_syms.push((uid, f.file.0, f.wrap, f.line_spacing)))?;
    let fonts: std::collections::BTreeMap<u64, crate::ui::FontSpec> = font_syms
        .into_iter()
        .map(|(uid, sym, wrap, spacing)| (uid, crate::ui::FontSpec { file: world.strings().get(sym).to_string(), wrap, spacing }))
        .filter(|(_, f)| !f.file.is_empty())
        .collect();
    let mut views = std::collections::BTreeSet::new();
    world.for_each_uid::<(&CameraView,), _>(|_, uid, _| {
        views.insert(uid);
    })?;
    let data = crate::ui::UiData { uis: &uis_raw, texts: &texts_raw, styles, fonts, anchors, views };
    // a picture drawn for a `CameraView` has no interface
    let (scene_rects, scene_texts) = if textures.nested { (Vec::new(), Vec::new()) } else { crate::ui::gather(world, textures, &view_proj, pw, ph, &data, &say) };
    let mut font_warnings = std::mem::take(&mut textures.fonts.warnings);
    textures.warnings.append(&mut font_warnings);
    // particle clouds: built for this camera
    let mut clouds: Vec<(u64, Particles, Transform, lunaris_ecs::Entity)> = Vec::new();
    world.for_each_uid::<(&Particles, &Transform), _>(|e, uid, (p, t)| {
        if p.visible {
            clouds.push((uid, *p, *t, e));
        }
    })?;
    clouds.sort_by_key(|c| c.0);
    let mut scene_particles: Vec<SceneParticles> = Vec::new();
    if !clouds.is_empty() {
        let inv = view.inverse_affine();
        let basis = crate::particles::CameraBasis { pos: cam.pos, forward: normalize(inv.transform_dir([0.0, 0.0, -1.0])), right: normalize(inv.transform_dir([1.0, 0.0, 0.0])), up: normalize(inv.transform_dir([0.0, 1.0, 0.0])) };
        for (uid, p, tr, e) in &clouds {
            let name = world.strings().get(p.texture.0).to_string();
            let tex = textures.get(if name.is_empty() { "soft" } else { &name });
            let modules = match world.get::<lunaris_sim::components::ParticleModules>(*e) {
                Some(m) => {
                    let text = world.strings().get(m.modules.0).to_string();
                    lunaris_sim::particle_modules::parse(&text).unwrap_or_else(|err| {
                        warnings.push(format!("ParticleModules of entity {uid}: {err} (the cloud is drawn without them)"));
                        Default::default()
                    })
                }
                None => Default::default(),
            };
            let mesh = crate::particles::build_with(p, &modules, tr, &basis, opts.threads);
            if mesh.pos.is_empty() {
                continue;
            }
            scene_particles.push(SceneParticles { uid: *uid, mesh: MeshRef::Dynamic { key: *uid, mesh: Arc::new(mesh) }, tex: Some(tex), additive: p.additive });
        }
    }
    Ok(Scene { cam, view, proj, view_proj, lights, meshes: scene_meshes, layers: scene_layers, particles: scene_particles, texts: scene_texts, rects: scene_rects, env, warnings })
}

pub fn render(world: &mut World, textures: &mut TextureCache, opts: &RenderOptions) -> Result<Frame> {
    if opts.width == 0 || opts.height == 0 || opts.width > 4096 || opts.height > 4096 {
        return Err(EcsError::new("bad_size", "image size must be between 1x1 and 4096x4096").at("width"));
    }
    let ss = opts.supersample.clamp(1, 3) as usize;
    let (rw, rh) = (opts.width as usize * ss, opts.height as usize * ss);
    if rw * rh > 40_000_000 {
        return Err(EcsError::new("bad_size", "image too large for the reference renderer").hint("lower width/height or supersample"));
    }

    // ---- gather (the same scene the GPU renderer draws)
    let scene = gather_scene(world, textures, opts)?;
    let cam = &scene.cam;
    let view_proj = scene.view_proj;
    let lights = &scene.lights[..];
    let mut warnings = scene.warnings.clone();
    let clear = display_to_linear(cam.clear);
    let clear = [clear[0], clear[1], clear[2]];
    let mut draws: Vec<Draw<'_>> = Vec::new();
    let mvp_of = |model: &M4| view_proj.mul(model);
    for m in &scene.meshes {
        let mesh = m.mesh.get();
        let rows = screen_rows(mesh, &mvp_of(&m.model), rh);
        draws.push(Draw { uid: m.uid, rows, kind: DrawKind::Mesh { mesh, model: m.model, color: m.color, shininess: m.shininess, emissive: m.emissive, tex: m.tex.as_deref(), uv_scale: m.uv_scale } });
    }
    for l in &scene.layers {
        let mesh = l.mesh.get();
        let rows = screen_rows(mesh, &mvp_of(&l.model), rh);
        draws.push(Draw { uid: l.uid, rows, kind: DrawKind::Sprite { mesh, model: l.model, color: l.color, tex: l.tex.as_deref(), uv: l.uv } });
    }
    for p in &scene.particles {
        let mesh = p.mesh.get();
        let rows = screen_rows(mesh, &view_proj, rh);
        draws.push(Draw { uid: p.uid, rows, kind: DrawKind::Particles { mesh, tex: p.tex.as_deref(), additive: p.additive } });
    }
    for r in &scene.rects {
        draws.push(Draw { uid: r.uid, rows: None, kind: DrawKind::Rect(r) });
    }
    for t in &scene.texts {
        draws.push(Draw { uid: t.uid, rows: None, kind: DrawKind::Text { text: t.text.clone(), tx: t.tx } });
    }

    // ---- draw: the picture is cut into bands of rows, each drawn independently (in parallel when threads > 1)
    let threads = opts.threads.clamp(1, 32);
    let bands = if threads > 1 && rh >= 96 { (threads * 2).min(rh / 16).max(1) } else { 1 };
    let rows_per = rh.div_ceil(bands);
    let mut color = vec![[0.0f32; 3]; rw * rh]; // zeroed memory costs nothing here; every band fills its own rows below
    let mut depth_buf = vec![0.0f32; rw * rh];
    let mut id_buf = vec![0u64; rw * rh];
    {
        let band_slices: Vec<std::sync::Mutex<Option<(usize, &mut [V3], &mut [f32], &mut [u64])>>> = color
            .chunks_mut(rows_per * rw)
            .zip(depth_buf.chunks_mut(rows_per * rw))
            .zip(id_buf.chunks_mut(rows_per * rw))
            .enumerate()
            .map(|(i, ((c, d), n))| std::sync::Mutex::new(Some((i * rows_per, c, d, n))))
            .collect();
        let run_band = |slot: &std::sync::Mutex<Option<(usize, &mut [V3], &mut [f32], &mut [u64])>>| {
            let (y0, c, d, n) = slot.lock().unwrap().take().expect("each band is drawn once");
            c.fill(clear);
            d.fill(f32::INFINITY);
            let mut t = Target { w: rw, h: rh, y0, y1: (y0 + rows_per).min(rh), color: c, depth: d, ids: n };
            draw_band(&mut t, &draws, &view_proj, lights, cam.pos, ss, opts.height as usize);
        };
        if band_slices.len() == 1 {
            run_band(&band_slices[0]);
        } else {
            let next = std::sync::atomic::AtomicUsize::new(0);
            std::thread::scope(|scope| {
                for _ in 0..threads.min(band_slices.len()) {
                    scope.spawn(|| loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(slot) = band_slices.get(i) else { break };
                        run_band(slot);
                    });
                }
            });
        }
    }
    let target = FullTarget { color, depth: depth_buf, ids: id_buf };
    warnings.append(&mut textures.warnings);

    // ---- resolve (supersample, gamma, 8-bit), in parallel over bands of output rows
    let (w, h) = (opts.width as usize, opts.height as usize);
    let mut rgba = vec![0u8; w * h * 4];
    let mut ids = vec![0u64; w * h];
    let mut depth = vec![0.0f32; w * h];
    let inv = 1.0 / (ss * ss) as f32;
    let out_bands = if threads > 1 && h >= 64 { (threads * 2).min(h / 8).max(1) } else { 1 };
    let out_rows = h.div_ceil(out_bands);
    let resolve = |first_row: usize, rgba: &mut [u8], ids: &mut [u64], depth: &mut [f32]| {
        for ry in 0..rgba.len() / (w * 4) {
            let y = first_row + ry;
            for x in 0..w {
                let mut acc = [0.0f32; 3];
                for sy in 0..ss {
                    for sx in 0..ss {
                        let c = target.color[(y * ss + sy) * rw + x * ss + sx];
                        acc = [acc[0] + c[0].clamp(0.0, 1.0), acc[1] + c[1].clamp(0.0, 1.0), acc[2] + c[2].clamp(0.0, 1.0)];
                    }
                }
                let o = (ry * w + x) * 4;
                for k in 0..3 {
                    rgba[o + k] = ((acc[k] * inv).sqrt() * 255.0 + 0.5) as u8;
                }
                rgba[o + 3] = 255;
                // the sample with the nearest surface decides the entity id
                let mut best = f32::INFINITY;
                let cell = ry * w + x;
                for sy in 0..ss {
                    for sx in 0..ss {
                        let i = (y * ss + sy) * rw + x * ss + sx;
                        if target.depth[i] < best || (target.ids[i] != 0 && ids[cell] == 0) {
                            best = best.min(target.depth[i]);
                            if target.ids[i] != 0 {
                                ids[cell] = target.ids[i];
                            }
                        }
                    }
                }
                depth[cell] = best;
            }
        }
    };
    if out_bands == 1 {
        resolve(0, &mut rgba, &mut ids, &mut depth);
    } else {
        let jobs: Vec<std::sync::Mutex<Option<(usize, &mut [u8], &mut [u64], &mut [f32])>>> = rgba
            .chunks_mut(out_rows * w * 4)
            .zip(ids.chunks_mut(out_rows * w))
            .zip(depth.chunks_mut(out_rows * w))
            .enumerate()
            .map(|(i, ((c, n), d))| std::sync::Mutex::new(Some((i * out_rows, c, n, d))))
            .collect();
        let next = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..threads.min(jobs.len()) {
                scope.spawn(|| loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    let (first, c, n, d) = job.lock().unwrap().take().expect("each band is resolved once");
                    resolve(first, c, n, d);
                });
            }
        });
    }
    Ok(Frame { width: opts.width, height: opts.height, rgba, ids, depth, view_proj, camera: cam.desc.clone(), warnings })
}
