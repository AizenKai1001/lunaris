//! The behaviour of interface widgets, as data on top of `Ui` rectangles: layout containers, sliders, toggles, text fields and focus
//! navigation with keys or a gamepad. They run every tick after the mouse has been read (`systems::ui`) and before the graph systems, so a graph system
//! reacts to `UiSlider.value`, `UiToggle.on`, `UiInput.submitted` or `Ui.clicked` in the tick it happens. See `docs/UI.md`.
use crate::components::{Parent, Ui, UiInput, UiLayout, UiNav, UiSlider, UiToggle};
use lunaris_ecs::{Entity, World};
use std::collections::BTreeMap;

fn pointer(world: &World) -> Option<(f32, f32)> {
    let (x, y) = (world.resource("input.mouse_x")?, world.resource("input.mouse_y")?);
    (x.is_finite() && y.is_finite()).then_some((x as f32, y as f32))
}

fn pressed(world: &World, action: &str) -> bool {
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
    let _ = world.for_each_uid::<(&Parent,), _>(|e, uid, (p,)| {
        parent_of.insert(uid, p.target.0);
        if ui.get(&uid).is_some_and(|u| u.visible) {
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
    let aspect = world.resource("screen.aspect").filter(|a| a.is_finite() && *a > 0.1).unwrap_or(1.0) as f32;
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
    let _ = world.for_each_uid::<(&Ui,), _>(|e, uid, (u,)| {
        if u.button && u.visible {
            cands.push((uid, e, [u.pos[0] + u.size[0] * 0.5, u.pos[1] + u.size[1] * 0.5]));
        }
    });
    cands.sort_by_key(|c| c.0);
    let mut cur = nav.current.0;
    if cur != 0 && !cands.iter().any(|c| c.0 == cur) {
        cur = 0;
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
pub fn update(world: &mut World, tick: u64) {
    layout(world);
    nav(world);
    sliders(world);
    toggles(world);
    inputs(world, tick);
}
