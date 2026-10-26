//! A software picture of what egui draws, for tests that want to be looked at (the real window needs a graphics card and a screen): the textures egui hands over (the glyph atlas, the
//! scene view's picture, the thumbnails) are kept, the shapes of a frame are cut into triangles by egui itself and filled here, one pixel at a time, with the same blending as the
//! window's (premultiplied colours, mixed in the colours' own space). Not part of the editor: only the tests use it.
use eframe::egui::{self, epaint::Primitive, ImageData, TextureId};
use std::collections::HashMap;

struct Tex {
    w: usize,
    h: usize,
    /// Premultiplied RGBA, 0 to 255.
    px: Vec<[u8; 4]>,
    linear: bool,
}

#[derive(Default)]
pub struct Screen {
    textures: HashMap<TextureId, Tex>,
    to_free: Vec<TextureId>,
    shapes: Vec<egui::epaint::ClippedShape>,
    ppp: f32,
    /// Keep the shapes of each frame (only a frame that is going to be photographed needs them).
    pub keep_shapes: bool,
}

impl Screen {
    /// Take in one frame's output: the texture changes always, the shapes when `keep_shapes` is set.
    pub fn apply(&mut self, out: &egui::FullOutput) {
        for id in self.to_free.drain(..) {
            self.textures.remove(&id);
        }
        for (id, deltas) in &out.textures_delta.set {
          for delta in deltas {
            let ImageData::Color(img) = &delta.image;
            let (w, h) = (img.size[0], img.size[1]);
            let linear = delta.options.magnification == egui::TextureFilter::Linear;
            let to_px = |c: &egui::Color32| c.to_array();
            match delta.pos {
                None => {
                    self.textures.insert(*id, Tex { w, h, px: img.pixels.iter().map(to_px).collect(), linear });
                }
                Some([x0, y0]) => {
                    if let Some(t) = self.textures.get_mut(id) {
                        for y in 0..h {
                            for x in 0..w {
                                if x0 + x < t.w && y0 + y < t.h {
                                    t.px[(y0 + y) * t.w + x0 + x] = img.pixels[y * w + x].to_array();
                                }
                            }
                        }
                    }
                }
            }
          }
        }
        self.to_free.extend(out.textures_delta.free.iter().copied());
        self.ppp = out.pixels_per_point;
        if self.keep_shapes {
            self.shapes = out.shapes.clone();
        }
    }

    /// The last kept frame as `width` x `height` pixels, RGBA, on a dark ground.
    pub fn picture(&self, ctx: &egui::Context, width: usize, height: usize) -> Vec<u8> {
        let mut fb: Vec<[f32; 4]> = vec![[0.07, 0.07, 0.08, 1.0]; width * height];
        let prims = ctx.tessellate(self.shapes.clone(), self.ppp);
        for p in &prims {
            let Primitive::Mesh(mesh) = &p.primitive else { continue };
            let Some(tex) = self.textures.get(&mesh.texture_id) else { continue };
            let clip = [(p.clip_rect.min.x * self.ppp).max(0.0), (p.clip_rect.min.y * self.ppp).max(0.0), (p.clip_rect.max.x * self.ppp).min(width as f32), (p.clip_rect.max.y * self.ppp).min(height as f32)];
            for t in mesh.indices.chunks_exact(3) {
                let v = [&mesh.vertices[t[0] as usize], &mesh.vertices[t[1] as usize], &mesh.vertices[t[2] as usize]];
                let pt = |i: usize| (v[i].pos.x * self.ppp, v[i].pos.y * self.ppp);
                let (a, b, c) = (pt(0), pt(1), pt(2));
                let area = (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1);
                if area.abs() < 1e-9 {
                    continue;
                }
                let x0 = a.0.min(b.0).min(c.0).floor().max(clip[0]).max(0.0) as usize;
                let y0 = a.1.min(b.1).min(c.1).floor().max(clip[1]).max(0.0) as usize;
                let x1 = (a.0.max(b.0).max(c.0).ceil().min(clip[2])).max(0.0) as usize;
                let y1 = (a.1.max(b.1).max(c.1).ceil().min(clip[3])).max(0.0) as usize;
                let inv = 1.0 / area;
                let col = |i: usize| v[i].color.to_array().map(|x| x as f32 / 255.0);
                let (ca, cb, cc) = (col(0), col(1), col(2));
                for y in y0..y1.min(height) {
                    for x in x0..x1.min(width) {
                        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                        let w0 = ((b.0 - px) * (c.1 - py) - (c.0 - px) * (b.1 - py)) * inv;
                        let w1 = ((c.0 - px) * (a.1 - py) - (a.0 - px) * (c.1 - py)) * inv;
                        let w2 = 1.0 - w0 - w1;
                        if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                            continue;
                        }
                        let u = w0 * v[0].uv.x + w1 * v[1].uv.x + w2 * v[2].uv.x;
                        let vv = w0 * v[0].uv.y + w1 * v[1].uv.y + w2 * v[2].uv.y;
                        let texel = sample(tex, u, vv);
                        let mut src = [0.0f32; 4];
                        for k in 0..4 {
                            src[k] = (w0 * ca[k] + w1 * cb[k] + w2 * cc[k]) * texel[k];
                        }
                        let dst = &mut fb[y * width + x];
                        for k in 0..4 {
                            dst[k] = src[k] + dst[k] * (1.0 - src[3]);
                        }
                    }
                }
            }
        }
        fb.iter().flat_map(|p| [(p[0] * 255.0 + 0.5) as u8, (p[1] * 255.0 + 0.5) as u8, (p[2] * 255.0 + 0.5) as u8, 255]).collect()
    }
}

/// A texel (premultiplied, 0 to 1) at the texture coordinates `u`, `v` (0 to 1): bilinear for a texture that asked for it, the nearest texel otherwise.
fn sample(t: &Tex, u: f32, v: f32) -> [f32; 4] {
    let at = |x: i64, y: i64| -> [f32; 4] {
        let (x, y) = (x.clamp(0, t.w as i64 - 1) as usize, y.clamp(0, t.h as i64 - 1) as usize);
        t.px[y * t.w + x].map(|c| c as f32 / 255.0)
    };
    if !t.linear {
        return at((u * t.w as f32).floor() as i64, (v * t.h as f32).floor() as i64);
    }
    let (fx, fy) = (u * t.w as f32 - 0.5, v * t.h as f32 - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let (p00, p10, p01, p11) = (at(x0 as i64, y0 as i64), at(x0 as i64 + 1, y0 as i64), at(x0 as i64, y0 as i64 + 1), at(x0 as i64 + 1, y0 as i64 + 1));
    let mut out = [0.0; 4];
    for k in 0..4 {
        let top = p00[k] + (p10[k] - p00[k]) * tx;
        let bottom = p01[k] + (p11[k] - p01[k]) * tx;
        out[k] = top + (bottom - top) * ty;
    }
    out
}

/// Write RGBA pixels as a PNG.
pub fn save_png(path: &std::path::Path, width: usize, height: usize, rgba: &[u8]) {
    std::fs::write(path, lunaris_render::encode_png(width as u32, height as u32, rgba).expect("png")).expect("write the picture");
}
