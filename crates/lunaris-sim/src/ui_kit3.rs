//! UI kit v3: the widgets menus and inventories need beyond `ui_kit.rs`, as data on top of `Ui` rectangles: scroll views with scroll bars (this first part), and after them dropdowns, tabs, drag
//! and drop, tooltips and the radial fill. They run inside `ui_kit::update`, every tick, before the graph systems, so a graph system reacts in the tick it happens. See `docs/UI.md`.
//!
//! Everything here is new components (nothing was added to an existing one, so no existing scene's state hash changes), and every function returns at once for a world that has none of them.
use crate::components::{Parent, Text, Ui, UiDrag, UiGroup, UiNav, UiScroll, UiScrollBar, UiSlider};
use crate::ui_kit::{pointer, ui_scale_aspect};
use lunaris_ecs::{Entity, World};
use std::collections::{BTreeMap, BTreeSet};

/// How far (a fraction of the picture's height) the pointer must move with the button held before a press on a button becomes a drag of the content.
const DRAG_THRESHOLD: f32 = 0.012;
/// The right stick does nothing below this (a pad rests a little off centre).
const STICK_DEADZONE: f32 = 0.25;
/// `UiScroll.dragging`: nothing, a press that may still turn into a drag, a content drag, the thumb of the vertical bar, the thumb of the horizontal bar.
const IDLE: u32 = 0;
const PENDING: u32 = 1;
const CONTENT: u32 = 2;
const THUMB_Y: u32 = 3;
const THUMB_X: u32 = 4;

/// The shape of the picture for padding and thicknesses that are fractions of the height: a `UiScale`'s, else the resource `screen.aspect`, else 1 (the same rule as `UiLayout`).
pub(crate) fn aspect_of(world: &mut World) -> f32 {
    ui_scale_aspect(world).map(|a| a as f64).or_else(|| world.resource("screen.aspect").filter(|a| a.is_finite() && *a > 0.1)).unwrap_or(1.0) as f32
}

/// Who each entity is attached to (`Parent.target`, 0 being nobody).
pub(crate) fn parent_map(world: &mut World) -> BTreeMap<u64, u64> {
    let mut m = BTreeMap::new();
    let _ = world.for_each_uid::<(&Parent,), _>(|_, uid, (p,)| {
        if p.target.0 != 0 {
            m.insert(uid, p.target.0);
        }
    });
    m
}

/// The chain of parents of `uid`, nearest first (at most 32 deep, so a cycle ends).
pub(crate) fn ancestors(parents: &BTreeMap<u64, u64>, uid: u64) -> impl Iterator<Item = u64> + '_ {
    let (mut cur, mut n) = (uid, 0u32);
    std::iter::from_fn(move || {
        if n >= 32 {
            return None;
        }
        let p = *parents.get(&cur)?;
        n += 1;
        cur = p;
        Some(p)
    })
}

pub(crate) fn has_ancestor(parents: &BTreeMap<u64, u64>, uid: u64, ancestor: u64) -> bool {
    ancestors(parents, uid).any(|a| a == ancestor)
}

fn rect_of(u: &Ui) -> [f32; 4] {
    [u.pos[0], u.pos[1], u.pos[0] + u.size[0], u.pos[1] + u.size[1]]
}

fn inside(r: [f32; 4], p: (f32, f32)) -> bool {
    p.0 >= r[0] && p.0 <= r[2] && p.1 >= r[1] && p.1 <= r[3]
}

fn finite4(r: [f32; 4]) -> bool {
    r.iter().all(|v| v.is_finite())
}

/// The lowest-numbered `UiNav`'s focused button (0: none).
fn nav_current(world: &mut World) -> u64 {
    let mut best: Option<(u64, u64)> = None;
    let _ = world.for_each_uid::<(&UiNav,), _>(|_, uid, (n,)| {
        if best.is_none_or(|b| uid < b.0) {
            best = Some((uid, n.current.0));
        }
    });
    best.map_or(0, |b| b.1)
}

/// The thumbs and tracks of the scroll bars: the engine places them, so a scroll view neither moves nor clips them.
fn bar_parts(world: &mut World) -> BTreeSet<u64> {
    let mut s = BTreeSet::new();
    let _ = world.for_each_uid::<(&UiScrollBar,), _>(|_, uid, (b,)| {
        s.insert(uid);
        if b.track.0 != 0 {
            s.insert(b.track.0);
        }
    });
    s
}

/// What a `UiGroup` with `shown` false hides: the entity it is on and everything below it (every entity whose `Parent` chain reaches it).
pub fn hidden_set(world: &mut World) -> BTreeSet<u64> {
    hidden_set_shared(world).as_ref().clone()
}

/// A number that is the same for the same groups that are off and the same `Parent` relation of the world (the two things the hidden set is made of), added up over the entities so that their order does not matter.
fn hidden_key(world: &mut World, off: &BTreeSet<u64>) -> u64 {
    fn mix(a: u64, b: u64) -> u64 {
        let mut z = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_add(0xD6E8_FEB8_6659_FD93).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        z ^= z >> 32;
        z = z.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        z ^ (z >> 29)
    }
    let (mut sum, mut n) = (0u64, 0u64);
    let _ = world.for_each_uid::<(&Parent,), _>(|_, uid, (p,)| {
        if p.target.0 != 0 {
            sum = sum.wrapping_add(mix(uid, p.target.0));
            n += 1;
        }
    });
    let mut off_sum = 0u64;
    for &u in off {
        off_sum = off_sum.wrapping_add(mix(u, 0x5851_F42D_4C95_7F2D));
    }
    mix(sum, n) ^ mix(off_sum, off.len() as u64 + 1)
}

thread_local! {
    /// The last hidden set of this thread and what it was made from.
    static HIDDEN: std::cell::RefCell<Option<(u64, std::sync::Arc<BTreeSet<u64>>)>> = const { std::cell::RefCell::new(None) };
}

/// [`hidden_set`], shared: a world with a group switched off pays one pass over the world's `Parent`s to find out that the set is the one of the last call (the widgets ask for it half a dozen times a tick, and the
/// renderers once a picture; building it means mapping every parent of the world, which in a city with thousands of attached parts cost 0.3 to 0.4 ms each time). The set is a function of the groups that are off and
/// of the `Parent` relation, and the key covers both.
pub fn hidden_set_shared(world: &mut World) -> std::sync::Arc<BTreeSet<u64>> {
    let mut off: BTreeSet<u64> = BTreeSet::new();
    let _ = world.for_each_uid::<(&UiGroup,), _>(|_, uid, (g,)| {
        if !g.shown {
            off.insert(uid);
        }
    });
    if off.is_empty() {
        return std::sync::Arc::new(BTreeSet::new());
    }
    let key = hidden_key(world, &off);
    if let Some(hit) = HIDDEN.with(|c| c.borrow().as_ref().filter(|h| h.0 == key).map(|h| h.1.clone())) {
        return hit;
    }
    let set = std::sync::Arc::new(hidden_compute(world, off));
    HIDDEN.with(|c| *c.borrow_mut() = Some((key, set.clone())));
    set
}

/// The hidden set made from scratch: the groups that are off and everything whose `Parent` chain reaches one.
pub(crate) fn hidden_compute(world: &mut World, off: BTreeSet<u64>) -> BTreeSet<u64> {
    if off.is_empty() {
        return off;
    }
    let parents = parent_map(world);
    let mut hidden = off.clone();
    for &uid in parents.keys() {
        if ancestors(&parents, uid).any(|a| off.contains(&a)) {
            hidden.insert(uid);
        }
    }
    hidden
}

/// A button that a group has hidden is not under the pointer, whatever the rectangle says.
pub(crate) fn hide_pointer(world: &mut World) {
    for &uid in hidden_set_shared(world).iter() {
        if let Some(u) = world.entity(uid).and_then(|e| world.get_mut::<Ui>(e)) {
            if u.button {
                u.hover = false;
                u.pressed = false;
                u.clicked = false;
            }
        }
    }
}

/// For every entity that sits inside a scroll view (its `Parent` chain reaches one), the rectangle it is clipped to: `[x0, y0, x1, y1]`, fractions of the picture, the viewports of all the views
/// above it intersected (an empty rectangle has `x1 <= x0` or `y1 <= y0`). The renderers read this map to cut every rectangle and text of those entities at the viewport.
pub fn clip_rects(world: &mut World) -> BTreeMap<u64, [f32; 4]> {
    let mut views: BTreeMap<u64, [f32; 4]> = BTreeMap::new();
    let _ = world.for_each_uid::<(&UiScroll, &Ui), _>(|_, uid, (_, u)| {
        let r = rect_of(u);
        if finite4(r) {
            views.insert(uid, r);
        }
    });
    if views.is_empty() {
        return BTreeMap::new();
    }
    let parents = parent_map(world);
    let parts = bar_parts(world);
    clip_rects_with(&views, &parents, &parts)
}

fn clip_rects_with(views: &BTreeMap<u64, [f32; 4]>, parents: &BTreeMap<u64, u64>, parts: &BTreeSet<u64>) -> BTreeMap<u64, [f32; 4]> {
    let mut out = BTreeMap::new();
    for &uid in parents.keys() {
        if parts.contains(&uid) {
            continue;
        }
        let mut clip: Option<[f32; 4]> = None;
        for a in ancestors(parents, uid) {
            if let Some(v) = views.get(&a) {
                clip = Some(match clip {
                    None => *v,
                    Some(c) => [c[0].max(v[0]), c[1].max(v[1]), c[2].min(v[2]), c[3].min(v[3])],
                });
            }
        }
        if let Some(c) = clip {
            out.insert(uid, c);
        }
    }
    out
}

#[derive(Clone, Copy)]
struct View {
    uid: u64,
    e: Entity,
    s: UiScroll,
    rect: [f32; 4],
}

fn views(world: &mut World) -> Vec<View> {
    let mut v = Vec::new();
    let _ = world.for_each_uid::<(&UiScroll, &Ui), _>(|e, uid, (s, u)| v.push(View { uid, e, s: *s, rect: rect_of(u) }));
    v.sort_by_key(|x| x.uid);
    v
}

/// How far the content can move on each axis: its size minus the viewport's.
fn range_of(s: &UiScroll, rect: [f32; 4]) -> [f32; 2] {
    let f = |content: f32, view: f32| if content.is_finite() && view.is_finite() && content - view > 1.0e-5 { content - view } else { 0.0 };
    [f(s.content[0], rect[2] - rect[0]), f(s.content[1], rect[3] - rect[1])]
}

#[derive(Clone, Copy)]
struct BarInfo {
    uid: u64,
    e: Entity,
    b: UiScrollBar,
    thumb_pressed: bool,
    track_pressed: bool,
}

fn bars(world: &mut World) -> Vec<BarInfo> {
    let mut list: Vec<(u64, Entity, UiScrollBar, bool)> = Vec::new();
    let _ = world.for_each_uid::<(&UiScrollBar, &Ui), _>(|e, uid, (b, u)| list.push((uid, e, *b, u.pressed)));
    list.sort_by_key(|l| l.0);
    list.into_iter()
        .map(|(uid, e, b, thumb_pressed)| {
            let track_pressed = b.track.0 != 0 && world.entity(b.track.0).and_then(|t| world.get::<Ui>(t)).is_some_and(|u| u.pressed);
            BarInfo { uid, e, b, thumb_pressed, track_pressed }
        })
        .collect()
}

/// Where a scroll bar's track and thumb are: `track` and `thumb` rectangles, the axis it scrolls (0 x, 1 y), the track's start and length along it, the thumb's length and how far the content can move.
struct Geo {
    track: [f32; 4],
    thumb: [f32; 4],
    axis: usize,
    t0: f32,
    tlen: f32,
    thumb_len: f32,
    range: f32,
}

fn bar_geometry(view: [f32; 4], content: [f32; 2], offset: [f32; 2], b: &UiScrollBar, aspect: f32) -> Option<Geo> {
    let aspect = if aspect.is_finite() && aspect > 0.05 { aspect } else { 1.0 };
    let (t, inset, min_length) = (b.thickness.max(0.0), b.inset.max(0.0), b.min_length.max(0.0));
    let axis = if b.horizontal { 0 } else { 1 };
    // thicknesses are fractions of the picture's height: across the width they are that over the aspect
    let (tw, th, iw, ih) = (t / aspect, t, inset / aspect, inset);
    let track = if b.horizontal { [view[0] + iw, view[3] - ih - th, view[2] - iw, view[3] - ih] } else { [view[2] - iw - tw, view[1] + ih, view[2] - iw, view[3] - ih] };
    if !finite4(track) {
        return None;
    }
    let (t0, t1) = if b.horizontal { (track[0], track[2]) } else { (track[1], track[3]) };
    let tlen = t1 - t0;
    if !(tlen > 0.0) {
        return None;
    }
    let view_len = view[axis + 2] - view[axis];
    let clen = content[axis];
    let range = if clen.is_finite() && clen - view_len > 1.0e-5 { clen - view_len } else { 0.0 };
    let ratio = if clen.is_finite() && clen > 0.0 { (view_len / clen).clamp(0.0, 1.0) } else { 1.0 };
    let min_len = if b.horizontal { min_length / aspect } else { min_length };
    let thumb_len = (tlen * ratio).max(min_len).min(tlen);
    let frac = if range > 0.0 { (offset[axis] / range).clamp(0.0, 1.0) } else { 0.0 };
    let start = t0 + (tlen - thumb_len) * frac;
    let thumb = if b.horizontal { [start, track[1], start + thumb_len, track[3]] } else { [track[0], start, track[2], start + thumb_len] };
    Some(Geo { track, thumb, axis, t0, tlen, thumb_len, range })
}

/// The first half of the scroll views' tick, right after the mouse has been read: buttons that are scrolled out of their viewport stop reacting, the wheel, the stick, a drag of the content and a drag
/// of a scroll bar's thumb move `offset`, and a drag takes the click away from the buttons under it. Nothing is moved on the screen yet (`scroll_apply` does that, after the focus has moved).
pub(crate) fn scroll_pointer(world: &mut World) {
    let mut views = views(world);
    if views.is_empty() {
        return;
    }
    let parents = parent_map(world);
    let parts = bar_parts(world);
    let bars = bars(world);
    let aspect = aspect_of(world);
    let ptr = pointer(world);
    let down = world.resource("input.mouse_left").is_some_and(|v| v > 0.5);
    let number = |w: &World, name: &str| w.resource(name).filter(|v| v.is_finite()).unwrap_or(0.0) as f32;
    let (wheel, stick) = (number(world, "input.mouse_wheel"), number(world, "input.gamepad.ry"));
    let nav = nav_current(world);
    let rects: BTreeMap<u64, [f32; 4]> = views.iter().filter(|v| finite4(v.rect)).map(|v| (v.uid, v.rect)).collect();
    let clips = clip_rects_with(&rects, &parents, &parts);

    // a button scrolled out of its viewport is not under a pointer that is outside the viewport
    for (uid, c) in &clips {
        let Some(e) = world.entity(*uid) else { continue };
        let outside = ptr.is_none_or(|p| !inside(*c, p));
        if outside {
            if let Some(u) = world.get_mut::<Ui>(e) {
                if u.button {
                    u.hover = false;
                    u.pressed = false;
                    u.clicked = false;
                }
            }
        }
    }

    // the view the wheel and a drag belong to: the innermost scrollable one under the pointer
    let depth = |uid: u64| ancestors(&parents, uid).count();
    let scrollable = |v: &View| {
        let r = range_of(&v.s, v.rect);
        r[0] > 0.0 || r[1] > 0.0
    };
    let target: Option<u64> = ptr.and_then(|p| {
        views
            .iter()
            .filter(|v| finite4(v.rect) && inside(v.rect, p) && clips.get(&v.uid).is_none_or(|c| inside(*c, p)) && scrollable(v))
            .max_by_key(|v| (depth(v.uid), v.uid))
            .map(|v| v.uid)
    });
    // the view that holds the focused button: what the right stick scrolls
    let focus_view: Option<u64> = (nav != 0)
        .then(|| views.iter().filter(|v| has_ancestor(&parents, nav, v.uid) && scrollable(v)).max_by_key(|v| (depth(v.uid), v.uid)).map(|v| v.uid))
        .flatten();

    for v in views.iter_mut() {
        let range = range_of(&v.s, v.rect);
        let was_content_drag = v.s.dragging == CONTENT;
        let press_edge = down && !v.s.down;
        if !down {
            v.s.dragging = IDLE;
        }
        if press_edge && v.s.dragging == IDLE {
            // the thumb of a bar, or its track (which puts the thumb under the pointer and carries on as a drag of it)
            for b in bars.iter().filter(|b| b.b.view.0 == v.uid && (b.thumb_pressed || b.track_pressed)) {
                if let (Some(g), Some(p)) = (bar_geometry(v.rect, v.s.content, v.s.offset, &b.b, aspect), ptr) {
                    if g.range > 0.0 {
                        let along = if g.axis == 0 { p.0 } else { p.1 };
                        let thumb_start = if g.axis == 0 { g.thumb[0] } else { g.thumb[1] };
                        v.s.grab_bar = if b.thumb_pressed { along - thumb_start } else { g.thumb_len * 0.5 };
                        v.s.dragging = if g.axis == 0 { THUMB_X } else { THUMB_Y };
                        break;
                    }
                }
            }
            // the content itself, unless the press is on a slider (which wants the pointer's movement for itself)
            if v.s.dragging == IDLE && v.s.drag && target == Some(v.uid) {
                let on_slider = parents.keys().any(|k| {
                    has_ancestor(&parents, *k, v.uid)
                        && world.entity(*k).is_some_and(|e| (world.get::<UiSlider>(e).is_some() || world.get::<UiDrag>(e).is_some()) && world.get::<Ui>(e).is_some_and(|u| u.pressed))
                });
                if !on_slider {
                    if let Some(p) = ptr {
                        v.s.dragging = PENDING;
                        v.s.grab = [p.0, p.1];
                        v.s.grab_offset = v.s.offset;
                    }
                }
            }
        }
        if let Some(p) = ptr {
            match v.s.dragging {
                PENDING | CONTENT => {
                    let d = [p.0 - v.s.grab[0], p.1 - v.s.grab[1]];
                    if v.s.dragging == PENDING && (d[1].abs() > DRAG_THRESHOLD || d[0].abs() * aspect > DRAG_THRESHOLD) {
                        v.s.dragging = CONTENT;
                    }
                    if v.s.dragging == CONTENT {
                        for a in 0..2 {
                            if range[a] > 0.0 {
                                v.s.offset[a] = v.s.grab_offset[a] - d[a];
                            }
                        }
                    }
                }
                THUMB_X | THUMB_Y => {
                    let horizontal = v.s.dragging == THUMB_X;
                    if let Some(b) = bars.iter().find(|b| b.b.view.0 == v.uid && b.b.horizontal == horizontal) {
                        if let Some(g) = bar_geometry(v.rect, v.s.content, v.s.offset, &b.b, aspect) {
                            let travel = g.tlen - g.thumb_len;
                            if g.range > 0.0 && travel > 0.0 {
                                let along = if horizontal { p.0 } else { p.1 };
                                v.s.offset[g.axis] = ((along - v.s.grab_bar - g.t0) / travel).clamp(0.0, 1.0) * g.range;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // the wheel: up (positive) moves the content down, so the offset gets smaller
        if target == Some(v.uid) && wheel != 0.0 {
            if range[1] > 0.0 {
                v.s.offset[1] -= wheel * v.s.speed;
            } else if range[0] > 0.0 {
                v.s.offset[0] -= wheel * v.s.speed / aspect;
            }
        }
        if focus_view == Some(v.uid) && stick.abs() > STICK_DEADZONE {
            v.s.offset[1] -= stick * v.s.speed * 0.5;
        }
        // a drag is not a click: the buttons under it neither stay pressed nor are clicked when it ends
        if v.s.dragging == CONTENT || was_content_drag {
            for k in parents.keys() {
                if has_ancestor(&parents, *k, v.uid) {
                    if let Some(u) = world.entity(*k).and_then(|e| world.get_mut::<Ui>(e)) {
                        u.pressed = false;
                        u.clicked = false;
                    }
                }
            }
        }
        v.s.down = down;
        if let Some(w) = world.get_mut::<UiScroll>(v.e) {
            *w = v.s;
        }
    }
}

/// The second half of the scroll views' tick, after `UiNav` has moved the focus: measure the content, scroll the focused button into view, keep the offset in range, shift everything inside the view to
/// match, and place the scroll bars. Outer views go first, so that an inner one is measured where the outer one has put it.
pub(crate) fn scroll_apply(world: &mut World) {
    let first = views(world);
    if first.is_empty() {
        return;
    }
    let parents = parent_map(world);
    let parts = bar_parts(world);
    let bars = bars(world);
    let aspect = aspect_of(world);
    let nav = nav_current(world);
    let hidden = hidden_set_shared(world);
    let mut order: Vec<(usize, u64)> = first.iter().map(|v| (ancestors(&parents, v.uid).count(), v.uid)).collect();
    order.sort_unstable();
    for (_, uid) in order {
        let Some(e) = world.entity(uid) else { continue };
        let (Some(mut s), Some(u)) = (world.get::<UiScroll>(e).copied(), world.get::<Ui>(e).copied()) else { continue };
        let rect = rect_of(&u);
        if !finite4(rect) {
            continue;
        }
        // everything inside the view, bar parts aside
        let kid_uids: Vec<u64> = parents.keys().filter(|k| !parts.contains(k) && has_ancestor(&parents, **k, uid)).copied().collect();
        let kids: Vec<Entity> = kid_uids.iter().filter_map(|k| world.entity(*k)).collect();
        // the content: as big as the author says, or as big as the visible things in it (what a `UiGroup` hides is not content)
        let mut measured = [0.0f32; 2];
        for (k, ku) in kids.iter().zip(&kid_uids) {
            if hidden.contains(ku) {
                continue;
            }
            if let Some(c) = world.get::<Ui>(*k) {
                if c.visible && finite4(rect_of(c)) {
                    for a in 0..2 {
                        measured[a] = measured[a].max(c.pos[a] + c.size[a] + s.applied[a] - rect[a]);
                    }
                }
            } else if let Some(t) = world.get::<Text>(*k) {
                if t.visible && t.pos[0].is_finite() && t.pos[1].is_finite() && t.size.is_finite() {
                    measured[0] = measured[0].max(t.pos[0] + s.applied[0] - rect[0]);
                    measured[1] = measured[1].max(t.pos[1] + t.size + s.applied[1] - rect[1]);
                }
            }
        }
        for a in 0..2 {
            s.content[a] = if s.content_size[a].is_finite() && s.content_size[a] > 0.0 { s.content_size[a] } else { measured[a] };
        }
        let range = range_of(&s, rect);
        // the focus moved to a button of this view: scroll just far enough to show it
        if nav != 0 && s.focus.0 != nav {
            s.focus.0 = if has_ancestor(&parents, nav, uid) { nav } else { 0 };
            if s.focus.0 != 0 {
                if let Some(r) = world.entity(nav).and_then(|f| world.get::<Ui>(f)).map(rect_of).filter(|r| finite4(*r)) {
                    for a in 0..2 {
                        if range[a] > 0.0 {
                            let (lo, hi, rlo, rhi) = (rect[a], rect[a + 2], r[a], r[a + 2]);
                            if rhi - rlo >= hi - lo || rlo < lo {
                                s.offset[a] += rlo - lo;
                            } else if rhi > hi {
                                s.offset[a] += rhi - hi;
                            }
                        }
                    }
                }
            }
        } else if nav == 0 {
            s.focus.0 = 0;
        }
        for a in 0..2 {
            s.offset[a] = if s.offset[a].is_finite() { s.offset[a].clamp(0.0, range[a]) } else { 0.0 };
        }
        let delta = [s.applied[0] - s.offset[0], s.applied[1] - s.offset[1]];
        if delta != [0.0, 0.0] {
            for k in &kids {
                if let Some(c) = world.get_mut::<Ui>(*k) {
                    c.pos[0] += delta[0];
                    c.pos[1] += delta[1];
                } else if let Some(t) = world.get_mut::<Text>(*k) {
                    t.pos[0] += delta[0];
                    t.pos[1] += delta[1];
                }
            }
        }
        s.changed = delta != [0.0, 0.0];
        s.applied = s.offset;
        if let Some(w) = world.get_mut::<UiScroll>(e) {
            *w = s;
        }
        // the scroll bars
        for b in bars.iter().filter(|b| b.b.view.0 == uid) {
            let geo = bar_geometry(rect, s.content, s.offset, &b.b, aspect);
            let show = geo.as_ref().is_some_and(|g| g.range > 0.0 || !b.b.hide);
            let place = |world: &mut World, ent: Entity, r: Option<[f32; 4]>, visible: bool| {
                if let Some(ui) = world.get_mut::<Ui>(ent) {
                    if let Some(r) = r {
                        ui.pos = [r[0], r[1]];
                        ui.size = [r[2] - r[0], r[3] - r[1]];
                    }
                    ui.visible = visible;
                }
            };
            place(world, b.e, geo.as_ref().map(|g| g.thumb), show);
            if b.b.track.0 != 0 {
                if let Some(t) = world.entity(b.b.track.0) {
                    place(world, t, geo.as_ref().map(|g| g.track), show);
                }
            }
            let _ = b.uid;
        }
    }
}
