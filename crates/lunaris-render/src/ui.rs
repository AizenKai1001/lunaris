//! The on-screen interface as both renderers draw it: every visible `Ui` and `Text` becomes flat shapes (`SceneRect`: rectangles, rounded ones,
//! gradients, pictures and the letters of a TrueType font, in drawing order) and bitmap-font texts (`SceneText`). Here the layout decisions are made
//! once, in whole picture pixels, so the reference renderer and the GPU renderer cannot disagree about where anything is.
use crate::fonts::FontCache;
use crate::math::*;
use crate::render::{SceneRect, SceneText, UiImage};
use crate::texture::TextureCache;
use lunaris_ecs::{Sym, World};
use lunaris_sim::components::{Text, Transform, Ui, UiAnchor, UiStyle};
use std::collections::BTreeMap;

/// The `TextFont` of an entity.
pub(crate) struct FontSpec {
    pub file: String,
    pub wrap: f32,
    pub spacing: f32,
}

/// What `gather` reads: the interface components of the world, sorted the way they are drawn.
pub(crate) struct UiData<'a> {
    pub uis: &'a [(u64, Ui)],
    pub texts: &'a [(u64, Text)],
    pub styles: BTreeMap<u64, UiStyle>,
    pub fonts: BTreeMap<u64, FontSpec>,
    pub anchors: BTreeMap<u64, UiAnchor>,
    /// Entities that have a `CameraView`: a `Ui` among them shows its own view.
    pub views: std::collections::BTreeSet<u64>,
}

/// Where a point of the world is in the picture, as fractions from the top left; `None` behind the camera.
fn project(view_proj: &M4, p: [f32; 3]) -> Option<(f32, f32)> {
    let c = view_proj.mul_v4([p[0], p[1], p[2], 1.0]);
    if !(c[3] > 1e-4) || !c[0].is_finite() || !c[1].is_finite() {
        return None;
    }
    Some((c[0] / c[3] * 0.5 + 0.5, 0.5 - c[1] / c[3] * 0.5))
}

/// Where an anchor points on the screen.
fn anchor_screen(world: &World, view_proj: &M4, a: &UiAnchor) -> Option<(f32, f32)> {
    let mut p = a.world;
    if a.entity.0 != 0 {
        let t = world.entity(a.entity.0).and_then(|e| world.get::<Transform>(e))?;
        p = [p[0] + t.pos[0], p[1] + t.pos[1], p[2] + t.pos[2]];
    }
    if !p.iter().all(|v| v.is_finite()) || !a.offset.iter().all(|v| v.is_finite()) {
        return None;
    }
    let (x, y) = project(view_proj, p)?;
    Some((x + a.offset[0], y + a.offset[1]))
}

fn finite4(c: [f32; 4]) -> [f32; 4] {
    let f = |v: f32| if v.is_finite() { v } else { 0.0 };
    [f(c[0]), f(c[1]), f(c[2]), f(c[3])]
}

/// A text laid out in lines.
struct Lines {
    runs: Vec<crate::fonts::Run>,
    /// How far apart the baselines are, and how tall the whole block is, in pixels.
    step: i64,
    height: i64,
    px: u32,
}

/// Lay `text` out in the font of `spec` at `size` (a fraction of the picture's height): a line for each newline, and with `wrap` more at the last space that
/// fits. A word wider than the line stays whole.
fn lay_out(fonts: &mut FontCache, spec: &FontSpec, text: &str, size: f32, pw: f32, ph: f32) -> Option<Lines> {
    let px = (size.clamp(0.005, 0.5) * ph).round().max(4.0) as u32;
    let max_w = if spec.wrap.is_finite() && spec.wrap > 0.0 { Some((spec.wrap * pw).round().max(1.0) as i32) } else { None };
    let mut runs = Vec::new();
    for paragraph in text.split('\n').take(64) {
        let mut current = String::new();
        for word in paragraph.split(' ') {
            let candidate = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
            match max_w {
                Some(m) if !current.is_empty() && fonts.layout(&spec.file, &candidate, px).ok()?.width > m => {
                    runs.push(fonts.layout(&spec.file, &current, px).ok()?);
                    current = word.to_string();
                }
                _ => current = candidate,
            }
        }
        runs.push(fonts.layout(&spec.file, &current, px).ok()?);
    }
    let gap = if spec.spacing.is_finite() { (spec.spacing.clamp(-0.5, 4.0) * px as f32).round() as i64 } else { 0 };
    let line = runs[0].height as i64;
    let step = line + gap;
    Some(Lines { height: step * (runs.len() as i64 - 1) + line, step, runs, px })
}

/// The letters of laid-out text as picture-pixel quads. `ax` is the picture column the lines are aligned to and `top` the row of the top of the first line.
#[allow(clippy::too_many_arguments)]
fn letters(lines: &Lines, uid: u64, color: [f32; 4], ax: i64, align: u32, top: i64, shadow: bool, pw: f32, ph: f32) -> Vec<SceneRect> {
    let off = ((lines.px as f32 / 14.0).round() as i64).max(1);
    let c = finite4(color);
    let (a, shadow_color) = (c[3].clamp(0.0, 1.0), [0.0, 0.0, 0.0, 0.65 * c[3].clamp(0.0, 1.0)]);
    let passes: &[(i64, [f32; 4])] = if shadow { &[(off, shadow_color), (0, c)] } else { &[(0, c)] };
    let mut out = Vec::new();
    if a <= 0.0 {
        return out;
    }
    for &(o, col) in passes {
        for (n, run) in lines.runs.iter().enumerate() {
            let x0 = match align {
                1 => ax - (run.width / 2) as i64,
                2 => ax - run.width as i64,
                _ => ax,
            };
            let baseline = top + n as i64 * lines.step + run.ascent as i64;
            for g in &run.glyphs {
                let (qx, qy) = (x0 + o + g.x as i64, baseline + o + g.y as i64);
                let (qx1, qy1) = (qx + g.w as i64, qy + g.h as i64);
                if qx1 <= 0 || qy1 <= 0 || qx >= pw as i64 || qy >= ph as i64 {
                    continue;
                }
                out.push(SceneRect {
                    uid,
                    x0: qx as f32 / pw,
                    y0: qy as f32 / ph,
                    x1: qx1 as f32 / pw,
                    y1: qy1 as f32 / ph,
                    color: col,
                    color2: [0.0; 4],
                    radius: 0.0,
                    image: Some(UiImage { tex: g.tex.clone(), uv: g.uv, smooth: false }),
                });
            }
        }
    }
    out
}

pub(crate) fn gather(world: &World, textures: &mut TextureCache, view_proj: &M4, pw: f32, ph: f32, data: &UiData<'_>, say: &dyn Fn(String) -> String) -> (Vec<SceneRect>, Vec<SceneText>) {
    let mut rects: Vec<SceneRect> = Vec::new();
    let mut texts: Vec<SceneText> = Vec::new();
    let sym = |s: Sym| world.strings().get(s.0).to_string();
    // texts first (the order they have always been drawn in), their letters go over every rectangle at the end
    let mut text_quads: Vec<SceneRect> = Vec::new();
    for (uid, tx0) in data.texts {
        let mut tx = *tx0;
        if !tx.visible {
            continue;
        }
        let template = sym(tx.value);
        if template.is_empty() {
            continue;
        }
        if let Some(a) = data.anchors.get(uid) {
            let Some((sx, sy)) = anchor_screen(world, view_proj, a) else { continue };
            tx.pos = [sx, sy];
        }
        let text = say(crate::font::expand_template(&template, |n| world.resource(n)));
        if let Some(spec) = data.fonts.get(uid) {
            if tx.pos[0].is_finite() && tx.pos[1].is_finite() {
                let ax = (tx.pos[0] * pw).round().clamp(-1.0e9, 1.0e9) as i64;
                let top = (tx.pos[1] * ph).round().clamp(-1.0e9, 1.0e9) as i64;
                if let Some(lines) = lay_out(&mut textures.fonts, spec, &text, tx.size, pw, ph) {
                    text_quads.extend(letters(&lines, *uid, tx.color, ax, tx.align, top, tx.shadow, pw, ph));
                    continue;
                }
            }
        }
        texts.push(SceneText { uid: *uid, text, tx });
    }
    for (uid, u0) in data.uis {
        let mut u = *u0;
        if !u.visible || !u.size[0].is_finite() || !u.size[1].is_finite() || !u.pos[0].is_finite() || !u.pos[1].is_finite() {
            continue;
        }
        if let Some(a) = data.anchors.get(uid) {
            let Some((sx, sy)) = anchor_screen(world, view_proj, a) else { continue };
            u.pos = [sx - u.size[0] * 0.5, sy - u.size[1] * 0.5];
        }
        let style = data.styles.get(uid).copied().or_else(|| data.views.contains(uid).then(UiStyle::default));
        let (x0, y0, x1, y1) = (u.pos[0], u.pos[1], u.pos[0] + u.size[0], u.pos[1] + u.size[1]);
        let (wpx, hpx) = ((x1 - x0) * pw, (y1 - y0) * ph);
        let radius = style.map_or(0.0, |s| if s.radius.is_finite() { (s.radius * ph).clamp(0.0, wpx.min(hpx) * 0.5) } else { 0.0 });
        let flat = |x0: f32, y0: f32, x1: f32, y1: f32, color: [f32; 4], color2: [f32; 4], radius_px: f32| SceneRect { uid: *uid, x0, y0, x1, y1, color, color2, radius: radius_px / ph, image: None };
        if let Some(s) = style.filter(|s| s.shadow_color[3] > 0.0 && s.shadow_color.iter().all(|v| v.is_finite()) && s.shadow_offset.iter().all(|v| v.is_finite())) {
            let (dx, dy) = (s.shadow_offset[0] * ph / pw, s.shadow_offset[1]);
            rects.push(flat(x0 + dx, y0 + dy, x1 + dx, y1 + dy, s.shadow_color, [0.0; 4], radius));
        }
        let mut inner = (x0, y0, x1, y1);
        let mut inner_radius = radius;
        if u.border > 0.0 && u.border_color[3] > 0.0 {
            let px = (u.border * ph).round().max(1.0);
            rects.push(flat(x0, y0, x1, y1, u.border_color, [0.0; 4], radius));
            inner = (x0 + px / pw, y0 + px / ph, x1 - px / pw, y1 - px / ph);
            inner_radius = (radius - px).max(0.0);
        }
        if inner.2 <= inner.0 || inner.3 <= inner.1 {
            continue;
        }
        let fill = u.fill.clamp(0.0, 1.0);
        let k = if u.button {
            // a button answers the pointer: lighter under it, darker while pressed
            if u.pressed { 0.7 } else if u.hover { 1.25 } else { 1.0 }
        } else {
            1.0
        };
        let lit = |c: [f32; 4]| [(c[0] * k).min(1.0), (c[1] * k).min(1.0), (c[2] * k).min(1.0), c[3]];
        let color = lit(u.color);
        let color2 = style.map_or([0.0; 4], |s| if s.color2[3] > 0.0 && s.color2.iter().all(|v| v.is_finite()) { lit(s.color2) } else { [0.0; 4] });
        if fill < 1.0 && u.back_color[3] > 0.0 {
            rects.push(flat(inner.0, inner.1, inner.2, inner.3, u.back_color, [0.0; 4], inner_radius));
        }
        let fill_x1 = inner.0 + (inner.2 - inner.0) * fill;
        if fill > 0.0 && color[3] > 0.0 {
            rects.push(flat(inner.0, inner.1, fill_x1, inner.3, color, color2, inner_radius));
        }
        if let Some(s) = style {
            let mut name = sym(s.image);
            if name.is_empty() && data.views.contains(uid) {
                name = crate::views::view_name(*uid);
            }
            if !name.is_empty() && fill > 0.0 && s.tint[3] > 0.0 && s.tint.iter().all(|v| v.is_finite()) && s.image_uv.iter().all(|v| v.is_finite()) {
                let tex = textures.get(&name);
                let mut uv = s.image_uv;
                let (mut qx0, mut qy0, mut qx1, mut qy1) = (inner.0, inner.1, fill_x1, inner.3);
                let mut radius_px = inner_radius;
                // a bar that is part full shows that part of the picture
                uv[2] = uv[0] + (uv[2] - uv[0]) * fill;
                let (iw, ih) = (tex.width as f32 * (uv[2] - uv[0]).abs(), tex.height as f32 * (uv[3] - uv[1]).abs());
                let (bw, bh) = ((qx1 - qx0) * pw, (qy1 - qy0) * ph);
                if iw > 0.0 && ih > 0.0 && bw > 0.0 && bh > 0.0 {
                    match s.fit {
                        1 => {
                            // the whole picture, in proportion, centred
                            let sc = (bw / iw).min(bh / ih);
                            let (dw, dh) = (iw * sc / pw, ih * sc / ph);
                            let (cx, cy) = ((qx0 + qx1) * 0.5, (qy0 + qy1) * 0.5);
                            (qx0, qy0, qx1, qy1) = (cx - dw * 0.5, cy - dh * 0.5, cx + dw * 0.5, cy + dh * 0.5);
                            radius_px = 0.0;
                        }
                        2 => {
                            // fills the box; what does not fit is cropped evenly
                            let sc = (bw / iw).max(bh / ih);
                            let (vis_w, vis_h) = (bw / (iw * sc), bh / (ih * sc));
                            let (cu, cv) = ((uv[0] + uv[2]) * 0.5, (uv[1] + uv[3]) * 0.5);
                            let (hu, hv) = ((uv[2] - uv[0]) * vis_w * 0.5, (uv[3] - uv[1]) * vis_h * 0.5);
                            uv = [cu - hu, cv - hv, cu + hu, cv + hv];
                        }
                        _ => {}
                    }
                }
                rects.push(SceneRect { uid: *uid, x0: qx0, y0: qy0, x1: qx1, y1: qy1, color: s.tint, color2: [0.0; 4], radius: radius_px / ph, image: Some(UiImage { tex, uv, smooth: s.smooth }) });
            }
        }
        let template = sym(u.text);
        if !template.is_empty() {
            let label = say(crate::font::expand_template(&template, |n| world.resource(n)));
            let size = u.text_size.clamp(0.005, 0.5);
            if let Some(spec) = data.fonts.get(uid) {
                if let Some(lines) = lay_out(&mut textures.fonts, spec, &label, size, pw, ph) {
                    // the block of lines is centred in the box
                    let top = (y0 * ph + (u.size[1] * ph - lines.height as f32) * 0.5).round() as i64;
                    let ax = (((x0 + x1) * 0.5) * pw).round() as i64;
                    rects.extend(letters(&lines, *uid, u.text_color, ax, 1, top, false, pw, ph));
                    continue;
                }
            }
            let tx = Text { value: Sym(0), pos: [(x0 + x1) * 0.5, y0 + (u.size[1] - size) * 0.5], size, color: u.text_color, align: 1, shadow: false, visible: true };
            texts.push(SceneText { uid: *uid, text: label, tx });
        }
    }
    rects.extend(text_quads);
    (rects, texts)
}
