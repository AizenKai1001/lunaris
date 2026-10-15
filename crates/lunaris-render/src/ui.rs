//! The on-screen interface as both renderers draw it: every visible `Ui` and `Text` becomes flat shapes (`SceneRect`: rectangles, rounded ones,
//! gradients, pictures and the letters of a TrueType font, in drawing order) and bitmap-font texts (`SceneText`). Here the layout decisions are made
//! once, in whole picture pixels, so the reference renderer and the GPU renderer cannot disagree about where anything is.
use crate::fonts::FontCache;
use crate::math::*;
use crate::render::{SceneRect, SceneText, UiImage};
use crate::texture::TextureCache;
use lunaris_ecs::{Sym, World};
use lunaris_sim::components::{Text, Transform, Ui, UiAnchor, UiEdge, UiStyle};
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
    /// The shape the interface was made for (`UiScale.aspect`), when the world has one: the elements are laid out in the safe area of that shape.
    pub scale: Option<f32>,
    /// `UiEdge`: elements pinned to an edge of the picture, or laid out over the whole of it.
    pub edges: BTreeMap<u64, UiEdge>,
}

/// The rectangle of the picture, in pixels, that an element's own fractions (0..1) span.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Frame {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
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

/// A text's `{resource}` placeholders and `{#key}` translations. A translated text may bring placeholders of its own (`"Score {score}"` in the language file): they are filled too.
fn fill_text(world: &World, say: &dyn Fn(String) -> String, template: &str) -> String {
    let filled = say(crate::font::expand_template(template, |n| world.resource(n)));
    if template.contains("{#") && filled.contains('{') {
        crate::font::expand_template(&filled, |n| world.resource(n))
    } else {
        filled
    }
}

pub(crate) fn gather(world: &World, textures: &mut TextureCache, view_proj: &M4, pw: f32, ph: f32, data: &UiData<'_>, say: &dyn Fn(String) -> String) -> (Vec<SceneRect>, Vec<SceneText>) {
    let mut rects: Vec<SceneRect> = Vec::new();
    let mut texts: Vec<SceneText> = Vec::new();
    // with a `UiScale` an element is laid out in the safe area of the shape the interface was made for (or at an edge of the picture, or over all of it): `frame_of` says which rectangle of
    // the picture its fractions span, the layout below works in that rectangle as if it were the whole picture, and the result is mapped back at the end
    let (full_w, full_h) = (pw, ph);
    let full = Frame { x: 0.0, y: 0.0, w: full_w, h: full_h };
    let area = data.scale.map(|a| lunaris_sim::ui_kit::safe_area(a, pw as u32, ph as u32)).filter(|a| *a != (0, 0, pw as u32, ph as u32));
    let frame_of = |uid: u64| -> Frame {
        let Some((ax, ay, aw, ah)) = area else { return full };
        let edge = data.edges.get(&uid);
        if edge.is_some_and(|e| e.screen) {
            return full;
        }
        let (left, top) = (ax as f32, ay as f32);
        let (right, bottom) = (full_w - (ax + aw) as f32, full_h - (ay + ah) as f32);
        // pinned to an edge: the margin on that side is taken away from the front (x or y of 0), or all of it is put in front (1)
        let place = |pin: f32, before: f32, after: f32| if pin.is_finite() && pin != 0.5 { pin.clamp(0.0, 1.0) * (before + after) } else { before };
        let (x, y) = match edge {
            Some(e) => (place(e.x, left, right), place(e.y, top, bottom)),
            None => (left, top),
        };
        Frame { x, y, w: aw as f32, h: ah as f32 }
    };
    let mut frames: BTreeMap<u64, Frame> = BTreeMap::new();
    // a point of the whole picture (fractions) as fractions of a frame
    let in_frame = |f: &Frame, sx: f32, sy: f32| ((sx * full_w - f.x) / f.w, (sy * full_h - f.y) / f.h);
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
        let f = frame_of(*uid);
        let (pw, ph) = (f.w, f.h);
        if f != full {
            frames.insert(*uid, f);
        }
        if let Some(a) = data.anchors.get(uid) {
            let Some((sx, sy)) = anchor_screen(world, view_proj, a) else { continue };
            let (fx, fy) = in_frame(&f, sx, sy);
            tx.pos = [fx, fy];
        }
        let text = fill_text(world, say, &template);
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
        let f = frame_of(*uid);
        let (pw, ph) = (f.w, f.h);
        if f != full {
            frames.insert(*uid, f);
        }
        if let Some(a) = data.anchors.get(uid) {
            let Some((sx, sy)) = anchor_screen(world, view_proj, a) else { continue };
            let (fx, fy) = in_frame(&f, sx, sy);
            u.pos = [fx - u.size[0] * 0.5, fy - u.size[1] * 0.5];
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
            let label = fill_text(world, say, &template);
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
    // back from the frames the elements were laid out in to fractions of the whole picture (the sizes that are fractions of a height follow the frame's height)
    if !frames.is_empty() {
        let (mx, my) = (|f: &Frame, x: f32| (f.x + x * f.w) / full_w, |f: &Frame, y: f32| (f.y + y * f.h) / full_h);
        for r in rects.iter_mut() {
            if let Some(f) = frames.get(&r.uid) {
                (r.x0, r.x1, r.y0, r.y1) = (mx(f, r.x0), mx(f, r.x1), my(f, r.y0), my(f, r.y1));
                r.radius *= f.h / full_h;
            }
        }
        for t in texts.iter_mut() {
            if let Some(f) = frames.get(&t.uid) {
                t.tx.pos = [mx(f, t.tx.pos[0]), my(f, t.tx.pos[1])];
                t.tx.size *= f.h / full_h;
            }
        }
    }
    (rects, texts)
}
