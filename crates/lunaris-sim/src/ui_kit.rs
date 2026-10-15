//! The behaviour of interface widgets, as data on top of `Ui` rectangles: layout containers, sliders, toggles, text fields and focus
//! navigation with keys or a gamepad. They run every tick after the mouse has been read (`systems::ui`) and before the graph systems, so a graph system
//! reacts to `UiSlider.value`, `UiToggle.on`, `UiInput.submitted` or `Ui.clicked` in the tick it happens. See `docs/UI.md`.
use crate::components::{Parent, Ui, UiInput, UiLayout, UiNav, UiNavAuto, UiScale, UiSlider, UiToggle};
use lunaris_ecs::{Entity, World};
use std::collections::BTreeMap;

/// The shape the interface was made for, `aspect` of the lowest-numbered `UiScale` (width over height), when the world has one that is a usable number.
pub fn ui_scale_aspect(world: &mut World) -> Option<f32> {
    let mut found: Option<(u64, f32)> = None;
    let _ = world.for_each_uid::<(&UiScale,), _>(|_, uid, (s,)| {
        if found.is_none_or(|f| uid < f.0) {
            found = Some((uid, s.aspect));
        }
    });
    found.map(|f| f.1).filter(|a| a.is_finite() && *a > 0.05 && *a < 20.0)
}

/// The part of a picture of `w` x `h` pixels that the interface is laid out in when a `UiScale` says it was made for `aspect` (width over height): the biggest rectangle of that shape that fits,
/// in whole pixels, in the middle: `(x, y, width, height)`. A picture of that very shape is all of it.
pub fn safe_area(aspect: f32, w: u32, h: u32) -> (u32, u32, u32, u32) {
    if w == 0 || h == 0 || !(aspect.is_finite() && aspect > 0.05 && aspect < 20.0) {
        return (0, 0, w, h);
    }
    let (wf, hf) = (w as f32, h as f32);
    let (sw, sh) = if wf / hf > aspect { ((hf * aspect).round().clamp(1.0, wf) as u32, h) } else { (w, (wf / aspect).round().clamp(1.0, hf) as u32) };
    ((w - sw) / 2, (h - sh) / 2, sw, sh)
}

/// Where the pointer is for the simulation: `x` and `y` in pixels of a window of `w` x `h`. Without a `UiScale` the fractions of the window (0 to 1); with one (its `aspect`) the fractions of the safe area,
/// which are below 0 or above 1 over the margins (so that nothing at the edge of the interface is hit from there).
pub fn pointer_to_ui(aspect: Option<f32>, x: f32, y: f32, w: u32, h: u32) -> (f32, f32) {
    let (wf, hf) = (w.max(1) as f32, h.max(1) as f32);
    match aspect {
        None => ((x / wf).clamp(0.0, 1.0), (y / hf).clamp(0.0, 1.0)),
        Some(a) => {
            let (ax, ay, aw, ah) = safe_area(a, w, h);
            (((x - ax as f32) / aw.max(1) as f32).clamp(-4.0, 5.0), ((y - ay as f32) / ah.max(1) as f32).clamp(-4.0, 5.0))
        }
    }
}

pub(crate) fn pointer(world: &World) -> Option<(f32, f32)> {
    let (x, y) = (world.resource("input.mouse_x")?, world.resource("input.mouse_y")?);
    (x.is_finite() && y.is_finite()).then_some((x as f32, y as f32))
}

pub(crate) fn pressed(world: &World, action: &str) -> bool {
    world.resource(&format!("action.{action}.pressed")).is_some_and(|v| v > 0.5)
}

/// Place the children of every `UiLayout` container inside it, in uid order. A child is any `Ui` entity whose `Parent` is the container; a hidden one takes no room.
fn layout(world: &mut World) {
    let mut containers: Vec<(u64, UiLayout)> = Vec::new();
    let _ = world.for_each_uid::<(&UiLayout,), _>(|_, uid, (l,)| containers.push((uid, *l)));
    if containers.is_empty() {
        return;
    }
    let mut parent_of: BTreeMap<u64, u64> = BTreeMap::new();
    let mut kids: BTreeMap<u64, Vec<(u64, Entity)>> = BTreeMap::new();
    let mut ui: BTreeMap<u64, Ui> = BTreeMap::new();
    let _ = world.for_each_uid::<(&Ui,), _>(|_, uid, (u,)| {
        ui.insert(uid, *u);
    });
    // what a `UiGroup` hides takes no room either
    let hidden = crate::ui_kit3::hidden_set(world);
    let _ = world.for_each_uid::<(&Parent,), _>(|e, uid, (p,)| {
        parent_of.insert(uid, p.target.0);
        if ui.get(&uid).is_some_and(|u| u.visible) && !hidden.contains(&uid) {
            kids.entry(p.target.0).or_default().push((uid, e));
        }
    });
    let depth = |mut uid: u64| -> usize {
        let mut d = 0;
        while let Some(p) = parent_of.get(&uid) {
            d += 1;
            uid = *p;
            if d > 32 {
                break;
            }
        }
        d
    };
    containers.sort_by_key(|(uid, _)| (depth(*uid), *uid));
    // with a `UiScale` the interface is laid out in a picture of the shape it names; otherwise the resource `screen.aspect` says what shape the picture is
    let aspect = ui_scale_aspect(world).map(|a| a as f64).or_else(|| world.resource("screen.aspect").filter(|a| a.is_finite() && *a > 0.1)).unwrap_or(1.0) as f32;
    for (uid, l) in containers {
        let Some(mut list) = kids.remove(&uid) else { continue };
        let Some(c) = world.entity(uid).and_then(|e| world.get::<Ui>(e)).map(|u| (u.pos, u.size)) else { continue };
        list.sort_by_key(|k| k.0);
        let n = list.len();
        // padding and spacing are fractions of the picture's height; the width's share is made equal in pixels by the aspect ratio
        let (pad, gap) = (l.padding.max(0.0), l.spacing.max(0.0));
        let (px, py) = (pad / aspect, pad);
        let (gx, gy) = (gap / aspect, gap);
        let (x0, y0, w, h) = (c.0[0] + px, c.0[1] + py, (c.1[0] - 2.0 * px).max(0.0), (c.1[1] - 2.0 * py).max(0.0));
        let (cols, rows) = match l.kind {
            1 => (n, 1),
            2 => (l.columns.max(1) as usize, n.div_ceil(l.columns.max(1) as usize)),
            _ => (1, n),
        };
        let cw = ((w - gx * (cols as f32 - 1.0)) / cols as f32).max(0.0);
        let ch = ((h - gy * (rows as f32 - 1.0)) / rows as f32).max(0.0);
        for (i, (_, e)) in list.into_iter().enumerate() {
            let (col, row) = match l.kind {
                1 => (i, 0),
                2 => (i % cols, i / cols),
                _ => (0, i),
            };
            if let Some(u) = world.get_mut::<Ui>(e) {
                u.pos = [x0 + col as f32 * (cw + gx), y0 + row as f32 * (ch + gy)];
                u.size = [cw, ch];
            }
        }
    }
}

/// Keyboard and gamepad focus: `action.ui_up/down/left/right.pressed` move it to the nearest button in that direction, `action.ui_accept.pressed` clicks the
/// focused one (see `InputMap`: bind them to keys and sticks). The focused button shows as hovered.
fn nav(world: &mut World) {
    let mut navs: Vec<(u64, Entity, UiNav)> = Vec::new();
    let _ = world.for_each_uid::<(&UiNav,), _>(|e, uid, (n,)| navs.push((uid, e, *n)));
    let Some((_, ne, nav)) = navs.into_iter().min_by_key(|n| n.0) else { return };
    let mut cands: Vec<(u64, Entity, [f32; 2])> = Vec::new();
    let hidden = crate::ui_kit3::hidden_set(world);
    let _ = world.for_each_uid::<(&Ui,), _>(|e, uid, (u,)| {
        if u.button && u.visible && !hidden.contains(&uid) {
            cands.push((uid, e, [u.pos[0] + u.size[0] * 0.5, u.pos[1] + u.size[1] * 0.5]));
        }
    });
    cands.sort_by_key(|c| c.0);
    let mut cur = nav.current.0;
    if cur != 0 && !cands.iter().any(|c| c.0 == cur) {
        cur = 0;
    }
    // a menu that takes the focus by itself (`UiNavAuto`): when nothing is focused, the first visible button is
    if cur == 0 && world.get::<UiNavAuto>(ne).is_some_and(|a| a.enabled) {
        cur = cands.first().map_or(0, |c| c.0);
    }
    let dirs: [(&str, [f32; 2]); 4] = [("ui_up", [0.0, -1.0]), ("ui_down", [0.0, 1.0]), ("ui_left", [-1.0, 0.0]), ("ui_right", [1.0, 0.0])];
    let want = dirs.iter().find(|(a, _)| pressed(world, a)).map(|(_, d)| *d);
    if let Some(d) = want {
        if let Some(from) = cands.iter().find(|c| c.0 == cur).map(|c| c.2) {
            let best = cands
                .iter()
                .filter(|c| c.0 != cur)
                .filter_map(|c| {
                    let (dx, dy) = (c.2[0] - from[0], c.2[1] - from[1]);
                    let (along, across) = (dx * d[0] + dy * d[1], (dx * d[1] - dy * d[0]).abs());
                    (along > 1.0e-4).then_some((along + 2.0 * across, c.0))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            if let Some((_, uid)) = best {
                cur = uid;
            }
        } else if let Some(first) = cands.first() {
            cur = first.0;
        }
    }
    if cur != nav.current.0 {
        if let Some(n) = world.get_mut::<UiNav>(ne) {
            n.current.0 = cur;
        }
    }
    if cur != 0 {
        if let Some(e) = world.entity(cur) {
            let accept = pressed(world, "ui_accept");
            if let Some(u) = world.get_mut::<Ui>(e) {
                u.hover = true;
                if accept {
                    u.clicked = true;
                }
            }
        }
    }
}

fn sliders(world: &mut World) {
    let down = world.resource("input.mouse_left").is_some_and(|v| v > 0.5);
    let mouse = pointer(world);
    let (left, right) = (pressed(world, "ui_left"), pressed(world, "ui_right"));
    let focus = {
        let mut n: Vec<(u64, u64)> = Vec::new();
        let _ = world.for_each_uid::<(&UiNav,), _>(|_, uid, (v,)| n.push((uid, v.current.0)));
        n.into_iter().min_by_key(|x| x.0).map_or(0, |x| x.1)
    };
    let mut list: Vec<(u64, Entity, UiSlider, Ui)> = Vec::new();
    let _ = world.for_each_uid::<(&UiSlider, &Ui), _>(|e, uid, (s, u)| list.push((uid, e, *s, *u)));
    list.sort_by_key(|l| l.0);
    for (uid, e, mut s, u) in list {
        let (lo, hi) = (s.min.min(s.max), s.min.max(s.max));
        let span = hi - lo;
        let before = s.value;
        if !down {
            s.dragging = false;
        } else if u.pressed {
            s.dragging = true;
        }
        if s.dragging {
            if let Some((mx, _)) = mouse {
                if u.size[0] > 0.0 {
                    s.value = lo + span * ((mx - u.pos[0]) / u.size[0]).clamp(0.0, 1.0);
                }
            }
        } else if focus == uid && (left || right) {
            let step = if s.step > 0.0 { s.step } else { span * 0.05 };
            s.value += if right { step } else { -step };
        }
        if s.step > 0.0 && span > 0.0 {
            s.value = lo + ((s.value - lo) / s.step).round() * s.step;
        }
        s.value = s.value.clamp(lo, hi);
        s.changed = s.value != before;
        let fill = if span > 0.0 { (s.value - lo) / span } else { 0.0 };
        if let Some(w) = world.get_mut::<UiSlider>(e) {
            *w = s;
        }
        if let Some(w) = world.get_mut::<Ui>(e) {
            w.fill = fill;
        }
    }
}

fn toggles(world: &mut World) {
    let mut list: Vec<(Entity, UiToggle, bool)> = Vec::new();
    let mut seen: Vec<(u64, Entity, UiToggle, bool)> = Vec::new();
    let _ = world.for_each_uid::<(&UiToggle, &Ui), _>(|e, uid, (t, u)| seen.push((uid, e, *t, u.clicked)));
    seen.sort_by_key(|s| s.0);
    list.extend(seen.into_iter().map(|(_, e, t, c)| (e, t, c)));
    for (e, mut t, clicked) in list {
        t.changed = clicked;
        if clicked {
            t.on = !t.on;
        }
        let color = if t.on { t.on_color } else { t.off_color };
        if let Some(w) = world.get_mut::<UiToggle>(e) {
            *w = t;
        }
        if let Some(u) = world.get_mut::<Ui>(e) {
            u.color = color;
        }
    }
}

fn inputs(world: &mut World, tick: u64) {
    let mut list: Vec<(u64, Entity, UiInput, Ui)> = Vec::new();
    let _ = world.for_each_uid::<(&UiInput, &Ui), _>(|e, uid, (t, u)| list.push((uid, e, *t, *u)));
    if list.is_empty() {
        return;
    }
    list.sort_by_key(|l| l.0);
    let down = world.resource("input.mouse_left").is_some_and(|v| v > 0.5);
    let clicked: Option<u64> = list.iter().find(|l| l.3.clicked).map(|l| l.0);
    let over_any = list.iter().any(|l| l.3.hover);
    let (ch, backspace, enter, escape) = (world.resource("input.char").unwrap_or(0.0), world.resource("input.backspace").unwrap_or(0.0) > 0.5, world.resource("input.enter").unwrap_or(0.0) > 0.5, world.resource("input.escape").unwrap_or(0.0) > 0.5);
    // typed characters are used up whether or not a field has the focus, so that a key pressed with nothing to type into is not typed later
    let consumed = true;
    for (uid, e, mut t, u) in list {
        t.submitted = false;
        if let Some(c) = clicked {
            t.focused = c == uid;
        } else if down && !over_any {
            t.focused = false;
        }
        let mut text = world.strings().get(t.text.0).to_string();
        if t.focused {
            if ch >= 32.0 {
                if let Some(c) = char::from_u32(ch as u32).filter(|c| !c.is_control()) {
                    if text.chars().count() < t.max_len as usize {
                        text.push(c);
                    }
                }
            }
            if backspace {
                text.pop();
            }
            if enter {
                t.submitted = true;
            }
            if escape {
                t.focused = false;
            }
        }
        let value_sym = world.strings_mut().intern(&text);
        t.text.0 = value_sym;
        let shown = if text.is_empty() && !t.focused { world.strings().get(t.placeholder.0).to_string() } else if t.focused && (tick / 30) % 2 == 0 { format!("{text}|") } else { text };
        let shown_sym = world.strings_mut().intern(&shown);
        if let Some(w) = world.get_mut::<UiInput>(e) {
            *w = t;
        }
        if let Some(w) = world.get_mut::<Ui>(e) {
            w.text.0 = shown_sym;
        }
        let _ = u;
    }
    if consumed {
        for name in ["input.char", "input.backspace", "input.enter", "input.escape"] {
            if world.resource(name).is_some() {
                let _ = world.set_resource(name, Some(0.0));
            }
        }
    }
}

/// Run the whole toolkit for one tick.
pub fn update(world: &mut World, tick: u64, dt: f32) {
    layout(world);
    // UI kit v3 (`ui_kit3.rs`, `ui_select.rs`): what is hidden, scrolled away or covered by an open list does not see the pointer, before anything reacts to it; the widgets that
    // answer clicks (drop-downs, tabs) run after the buttons have been clicked, and scroll views move their content after the focus has moved
    crate::ui_kit3::hide_pointer(world);
    crate::ui_kit3::scroll_pointer(world);
    crate::ui_select::popup_pointer(world);
    nav(world);
    sliders(world);
    toggles(world);
    inputs(world, tick);
    crate::ui_select::update(world);
    crate::ui_kit3::scroll_apply(world);
    // things in slots are placed last, where the slots (and the scroll views they are in) have ended up this tick
    crate::ui_drag::update(world);
    // and the tooltip last, so that it sees where everything is: `dt` is the unscaled step, so a tooltip is shown while the game is paused too
    crate::ui_tip::update(world, dt);
}
