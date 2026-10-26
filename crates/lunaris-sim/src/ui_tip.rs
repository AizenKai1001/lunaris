//! Tooltips: a `Ui` with a `UiTooltip` shows its text in the tooltip box (a `Ui` with a `UiTooltipBox`) near the pointer once the pointer has rested on it for a delay. See `docs/UI.md`, "Tooltips".
use crate::components::{Ui, UiTooltip, UiTooltipBox};
use crate::ui_kit::pointer;
use lunaris_ecs::{Entity, Sym, World};

/// How far (a fraction of the picture's height) the pointer may wander and still be resting.
const REST: f32 = 0.006;

fn rect_of(u: &Ui) -> [f32; 4] {
    [u.pos[0], u.pos[1], u.pos[0] + u.size[0], u.pos[1] + u.size[1]]
}

fn inside(r: [f32; 4], p: (f32, f32)) -> bool {
    p.0 >= r[0] && p.0 <= r[2] && p.1 >= r[1] && p.1 <= r[3]
}

/// What a letter of a label takes up, as a share of the text size (the built-in font's advance is 6 of its 8 pixels).
const LETTER: f32 = 0.75;

/// Where and how big the box is: near the pointer, on the other side of it where it would run off the picture, and never off the top or the left. `size` comes from the text when `auto`.
fn place(b: &UiTooltipBox, ui: &Ui, text: &str, p: (f32, f32), aspect: f32) -> ([f32; 2], [f32; 2]) {
    let size = if b.auto_size {
        let lines: Vec<&str> = text.split('\n').collect();
        let widest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
        let ts = ui.text_size.clamp(0.005, 0.5);
        [widest * LETTER * ts / aspect + 2.0 * b.padding / aspect, lines.len() as f32 * ts * 1.4 + 2.0 * b.padding]
    } else {
        ui.size
    };
    let (ox, oy) = (b.offset[0], b.offset[1]);
    let x = if p.0 + ox + size[0] > 1.0 { p.0 - ox - size[0] } else { p.0 + ox };
    let y = if p.1 + oy + size[1] > 1.0 { p.1 - oy - size[1] } else { p.1 + oy };
    ([x.max(0.0), y.max(0.0)], size)
}

pub(crate) fn update(world: &mut World, dt: f32) {
    let mut boxes: Vec<(u64, Entity, UiTooltipBox, Ui)> = Vec::new();
    let _ = world.for_each_uid::<(&UiTooltipBox, &Ui), _>(|e, uid, (b, u)| boxes.push((uid, e, *b, *u)));
    let Some((box_uid, be, mut b, bui)) = boxes.into_iter().min_by_key(|b| b.0) else { return };
    let mut triggers: Vec<(u64, Sym, f32, Ui)> = Vec::new();
    let _ = world.for_each_uid::<(&UiTooltip, &Ui), _>(|_, uid, (t, u)| triggers.push((uid, t.text, t.delay, *u)));
    let hidden = crate::ui_kit3::hidden_set_shared(world);
    let clips = crate::ui_kit3::clip_rects(world);
    let aspect = crate::ui_kit3::aspect_of(world);
    let ptr = pointer(world);
    let down = world.resource("input.mouse_left").is_some_and(|v| v > 0.5);
    // the trigger under the pointer: visible, not hidden by a group, in its scroll view's viewport; the topmost by layer, then by uid
    let target = ptr.and_then(|p| {
        triggers
            .iter()
            .filter(|t| t.0 != box_uid && t.3.visible && !hidden.contains(&t.0) && inside(rect_of(&t.3), p) && clips.get(&t.0).is_none_or(|c| inside(*c, p)))
            .max_by_key(|t| (t.3.layer, t.0))
    });
    let target_uid = target.map_or(0, |t| t.0);
    let mut placed: Option<([f32; 2], [f32; 2], Sym)> = None;
    // a press is an answer to the question: the tooltip goes away, and stays away until the pointer has left and rested again
    let press_edge = down && !b.down;
    if target_uid != b.source.0 {
        // a different trigger (or none): start over
        b.source.0 = target_uid;
        b.timer = 0.0;
        b.shown = false;
        b.suppress = press_edge && target_uid != 0;
        if let Some(p) = ptr {
            b.anchor = [p.0, p.1];
        }
    } else if let (Some(t), Some(p)) = (target, ptr) {
        if press_edge {
            b.suppress = true;
            b.shown = false;
        }
        if !b.suppress && !b.shown {
            let moved = ((p.0 - b.anchor[0]) * aspect).abs().max((p.1 - b.anchor[1]).abs());
            if moved > REST {
                b.anchor = [p.0, p.1];
                b.timer = 0.0;
            } else {
                b.timer += dt;
            }
            if b.timer >= t.2.max(0.0) {
                b.shown = true;
                let text = world.strings().get(t.1 .0).to_string();
                let (pos, size) = place(&b, &bui, &text, p, aspect);
                placed = Some((pos, size, t.1));
            }
        }
    }
    b.down = down;
    if let Some(w) = world.get_mut::<UiTooltipBox>(be) {
        *w = b;
    }
    if let Some(u) = world.get_mut::<Ui>(be) {
        u.visible = b.shown;
        if let Some((pos, size, text)) = placed {
            u.pos = pos;
            u.size = size;
            u.text = text;
        }
    }
}
