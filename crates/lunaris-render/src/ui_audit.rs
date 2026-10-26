//! A listing of the on-screen interface for quality checks (`docs/UI_AUDIT.md`): every visible `Ui` and `Text` of the world with the box it covers and the
//! box its letters cover, in pixels of a picture of the size asked for. Nothing here lays anything out itself: it asks `gather_scene` (the code both renderers
//! draw from) and sorts what comes back, so a text that the audit says runs over its button does so in the picture too.
use crate::fonts::PAGE;
use crate::render::{gather_scene, RenderOptions, Scene};
use crate::texture::TextureCache;
use lunaris_ecs::{Result, World};
use lunaris_sim::components::{InputMap, Localization, Name, Parent, Text, TextFont, Ui, UiAnchor, UiEdge, UiStyle};
use std::collections::BTreeMap;

/// One visible interface element.
#[derive(Clone, Debug)]
pub struct AuditEntry {
    pub uid: u64,
    pub name: String,
    /// The `Parent` entity (0 for none).
    pub parent: u64,
    /// `Ui.layer`; 0 for a `Text`.
    pub layer: u32,
    /// A `Text` (the HUD text, drawn over every rectangle) and not a `Ui`.
    pub is_text: bool,
    /// The element's rectangle in pixels (x0, y0, x1, y1): a `Ui`'s own box (with a `UiScale` after the safe area and `UiEdge` placement). `None` for a `Text`.
    pub rect: Option<[f32; 4]>,
    /// The box the letters cover (x0, y0, x1, y1), `None` when nothing is drawn.
    pub ink: Option<[f32; 4]>,
    /// The words drawn, `{resource}`, `{#key}` and `{key:ACTION}` filled in (a `{key:...}` left in it is an action the `InputMap` does not know).
    pub label: String,
    /// The text as written in the scene, placeholders and all.
    pub template: String,
    /// The `TextFont` file; empty for the built-in 5x8 font.
    pub font: String,
    /// How tall a letter's em is in pixels (the built-in font: the height of a glyph row of 8).
    pub em_px: f32,
    pub button: bool,
    /// `Ui.color`, `Ui.back_color`, `Ui.text_color` (a `Text`: its own colour), RGBA display colours.
    pub color: [f32; 4],
    pub back_color: [f32; 4],
    pub text_color: [f32; 4],
    /// Draws a picture (`UiStyle.image`).
    pub image: bool,
    pub shadow: bool,
    /// A `Text` pinned with a `UiAnchor` or a `Ui` with one: the rectangle is then the one the pin gave.
    pub anchored: bool,
    pub clip: Option<[f32; 4]>,
}

fn union(a: &mut Option<[f32; 4]>, b: [f32; 4]) {
    *a = Some(match *a {
        None => b,
        Some(c) => [c[0].min(b[0]), c[1].min(b[1]), c[2].max(b[2]), c[3].max(b[3])],
    });
}

/// Every visible `Ui` and `Text` of the world as a picture of `width` x `height` pixels would show it. Does not change the world (the textures and fonts of `textures` are loaded as a picture would).
pub fn audit(world: &mut World, textures: &mut TextureCache, width: u32, height: u32) -> Result<Vec<AuditEntry>> {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let scene: Scene = gather_scene(world, textures, &RenderOptions { width: width.max(1), height: height.max(1), ..RenderOptions::default() })?;
    let hidden = lunaris_sim::ui_kit3::hidden_set(world);
    let scale = lunaris_sim::ui_kit::ui_scale_aspect(world);
    let area = scale.map(|a| lunaris_sim::ui_kit::safe_area(a, width.max(1), height.max(1))).filter(|a| *a != (0, 0, width.max(1), height.max(1)));
    // the rectangles the scene holds for each element, split into the letters of TrueType text and the rest
    let mut letters: BTreeMap<u64, [f32; 4]> = BTreeMap::new();
    let mut shapes: BTreeMap<u64, [f32; 4]> = BTreeMap::new();
    for r in &scene.rects {
        let b = [r.x0 * w, r.y0 * h, r.x1 * w, r.y1 * h];
        let is_letter = r.radius == 0.0 && r.sweep.is_none() && r.image.as_ref().is_some_and(|i| !i.smooth && i.tex.width == PAGE && i.tex.height == PAGE);
        let mut slot = if is_letter { letters.get(&r.uid).copied() } else { shapes.get(&r.uid).copied() };
        union(&mut slot, b);
        if is_letter {
            letters.insert(r.uid, slot.unwrap_or(b));
        } else {
            shapes.insert(r.uid, slot.unwrap_or(b));
        }
    }
    // the built-in font's texts: the box is worked out as the reference renderer draws them
    let mut bitmap: BTreeMap<u64, ([f32; 4], f32, ())> = BTreeMap::new();
    for t in &scene.texts {
        let cell = ((t.tx.size.clamp(0.005, 0.5) * h / 8.0).round().max(1.0)) as f32;
        let n = t.text.chars().take(512).count() as f32;
        let width_px = (n * 6.0 * cell - cell).max(0.0);
        let ax = (t.tx.pos[0] * w).round();
        let x0 = match t.tx.align {
            1 => ax - (width_px / 2.0).floor(),
            2 => ax - width_px,
            _ => ax,
        };
        let y0 = (t.tx.pos[1] * h).round();
        bitmap.insert(t.uid, ([x0, y0, x0 + width_px, y0 + 8.0 * cell], cell * 8.0, ()));
    }
    let mut uis: Vec<(u64, Ui)> = Vec::new();
    world.for_each_uid::<(&Ui,), _>(|_, uid, (u,)| uis.push((uid, *u)))?;
    uis.sort_by_key(|c| c.0);
    let mut texts: Vec<(u64, Text)> = Vec::new();
    world.for_each_uid::<(&Text,), _>(|_, uid, (t,)| texts.push((uid, *t)))?;
    texts.sort_by_key(|c| c.0);
    // the words as `gather` fills them in: `{resource}`, then `{key:ACTION}` and `{#key}`
    let mut languages: Vec<(u64, u32)> = Vec::new();
    world.for_each_uid::<(&Localization,), _>(|_, uid, (l,)| languages.push((uid, l.language.0)))?;
    let table = match languages.iter().min_by_key(|l| l.0) {
        Some((_, sym)) => {
            let name = world.strings().get(*sym).to_string();
            textures.translations(&name)
        }
        None => Default::default(),
    };
    let mut maps: Vec<(u64, u32)> = Vec::new();
    world.for_each_uid::<(&InputMap,), _>(|_, uid, (m,)| maps.push((uid, m.bindings.0)))?;
    let input_map = maps.iter().min_by_key(|m| m.0).map(|m| world.strings().get(m.1).to_string()).unwrap_or_default();
    let say = |label: String| {
        let label = if label.contains("{key:") { lunaris_sim::rebind::expand_keys(&input_map, &label) } else { label };
        if label.contains("{#") {
            crate::font::translate(&label, |k| table.get(k).cloned())
        } else {
            label
        }
    };
    let fill = |world: &World, template: &str| -> String {
        let filled = say(crate::font::expand_template(template, |n| world.resource(n)));
        if template.contains("{#") && filled.contains('{') {
            crate::font::expand_template(&filled, |n| world.resource(n))
        } else {
            filled
        }
    };
    let mut out = Vec::new();
    let strings = |world: &World, s: u32| world.strings().get(s).to_string();
    let frame_of = |world: &World, uid: u64| -> (f32, f32, f32, f32) {
        let Some((ax, ay, aw, ah)) = area else { return (0.0, 0.0, w, h) };
        let edge = world.entity(uid).and_then(|e| world.get::<UiEdge>(e)).copied();
        if edge.is_some_and(|e| e.screen) {
            return (0.0, 0.0, w, h);
        }
        let (left, top) = (ax as f32, ay as f32);
        let (right, bottom) = (w - (ax + aw) as f32, h - (ay + ah) as f32);
        let place = |pin: f32, before: f32, after: f32| if pin.is_finite() && pin != 0.5 { pin.clamp(0.0, 1.0) * (before + after) } else { before };
        let (x, y) = match edge {
            Some(e) => (place(e.x, left, right), place(e.y, top, bottom)),
            None => (left, top),
        };
        (x, y, aw as f32, ah as f32)
    };
    for (uid, u) in &uis {
        if !u.visible || hidden.contains(uid) || !u.size[0].is_finite() || !u.size[1].is_finite() || !u.pos[0].is_finite() || !u.pos[1].is_finite() {
            continue;
        }
        let e = world.entity(*uid);
        let anchored = e.and_then(|e| world.get::<UiAnchor>(e)).is_some();
        let (fx, fy, fw, fh) = frame_of(world, *uid);
        let rect = if anchored {
            match shapes.get(uid) {
                Some(b) => *b,
                None => continue,
            }
        } else {
            [fx + u.pos[0] * fw, fy + u.pos[1] * fh, fx + (u.pos[0] + u.size[0]) * fw, fy + (u.pos[1] + u.size[1]) * fh]
        };
        let font = e.and_then(|e| world.get::<TextFont>(e)).map(|f| strings(world, f.file.0)).unwrap_or_default();
        let template = strings(world, u.text.0);
        let (ink, em_px) = match (letters.get(uid), bitmap.get(uid)) {
            (Some(b), _) => (Some(*b), (u.text_size.clamp(0.005, 0.5) * fh).round().max(4.0)),
            (None, Some((b, em, _))) => (Some(*b), *em),
            _ => (None, 0.0),
        };
        let label = if template.is_empty() { String::new() } else { fill(world, &template) };
        let style = e.and_then(|e| world.get::<UiStyle>(e)).copied();
        let name = e.and_then(|e| world.get::<Name>(e)).map(|n| strings(world, n.value.0)).unwrap_or_default();
        let parent = e.and_then(|e| world.get::<Parent>(e)).map(|p| p.target.0).unwrap_or(0);
        out.push(AuditEntry {
            uid: *uid,
            name,
            parent,
            layer: u.layer,
            is_text: false,
            rect: Some(rect),
            ink,
            label,
            template,
            font,
            em_px,
            button: u.button,
            color: u.color,
            back_color: u.back_color,
            text_color: u.text_color,
            image: style.is_some_and(|s| s.image.0 != 0),
            shadow: false,
            anchored,
            clip: scene.rects.iter().find(|r| r.uid == *uid).and_then(|r| r.clip),
        });
    }
    for (uid, t) in &texts {
        if !t.visible || hidden.contains(uid) {
            continue;
        }
        let e = world.entity(*uid);
        let font = e.and_then(|e| world.get::<TextFont>(e)).map(|f| strings(world, f.file.0)).unwrap_or_default();
        let (_, fh) = {
            let (_, _, _, fh) = frame_of(world, *uid);
            (0.0, fh)
        };
        let template = strings(world, t.value.0);
        let (ink, em_px) = match (letters.get(uid), bitmap.get(uid)) {
            (Some(b), _) => (Some(*b), (t.size.clamp(0.005, 0.5) * fh).round().max(4.0)),
            (None, Some((b, em, _))) => (Some(*b), *em),
            _ => (None, 0.0),
        };
        let label = if template.is_empty() { String::new() } else { fill(world, &template) };
        if ink.is_none() {
            continue;
        }
        let name = e.and_then(|e| world.get::<Name>(e)).map(|n| strings(world, n.value.0)).unwrap_or_default();
        let parent = e.and_then(|e| world.get::<Parent>(e)).map(|p| p.target.0).unwrap_or(0);
        out.push(AuditEntry {
            uid: *uid,
            name,
            parent,
            layer: 0,
            is_text: true,
            rect: None,
            ink,
            label,
            template,
            font,
            em_px,
            button: false,
            color: [0.0; 4],
            back_color: [0.0; 4],
            text_color: t.color,
            image: false,
            shadow: t.shadow,
            anchored: e.and_then(|e| world.get::<UiAnchor>(e)).is_some(),
            clip: None,
        });
    }
    Ok(out)
}
