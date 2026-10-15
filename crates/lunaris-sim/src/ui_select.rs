//! Choosing one of several things: drop-downs (`UiDropdown` on a header button, `UiOption` on each option) and tabs (`UiTabs` and `UiTab`), built from the pieces of the kit (buttons, `UiLayout` columns,
//! `UiGroup`s that show and hide a subtree). See `docs/UI.md`, "Drop-downs and tabs".
use crate::components::{Ui, UiDropdown, UiGroup, UiNav, UiOption, UiTab, UiTabs};
use crate::ui_kit::{pointer, pressed};
use lunaris_ecs::{Entity, World};
use std::collections::BTreeSet;

fn rect_of(u: &Ui) -> [f32; 4] {
    [u.pos[0], u.pos[1], u.pos[0] + u.size[0], u.pos[1] + u.size[1]]
}

fn inside(r: [f32; 4], p: (f32, f32)) -> bool {
    p.0 >= r[0] && p.0 <= r[2] && p.1 >= r[1] && p.1 <= r[3]
}

fn union(a: Option<[f32; 4]>, b: [f32; 4]) -> [f32; 4] {
    match a {
        None => b,
        Some(a) => [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])],
    }
}

struct Dd {
    uid: u64,
    e: Entity,
    d: UiDropdown,
    header: Ui,
}

type Opt = (u64, Entity, UiOption, Ui);

fn dropdowns_of(world: &mut World) -> Vec<Dd> {
    let mut v = Vec::new();
    let _ = world.for_each_uid::<(&UiDropdown, &Ui), _>(|e, uid, (d, u)| v.push(Dd { uid, e, d: *d, header: *u }));
    v.sort_by_key(|d| d.uid);
    v
}

fn options_of(world: &mut World) -> Vec<Opt> {
    let mut v: Vec<Opt> = Vec::new();
    let _ = world.for_each_uid::<(&UiOption, &Ui), _>(|e, uid, (o, u)| v.push((uid, e, *o, *u)));
    v.sort_by_key(|o| o.0);
    v
}

/// The rectangle an open list covers: the list's own `Ui` and its options together.
fn area_of(world: &World, dd: &Dd, opts: &[Opt]) -> Option<[f32; 4]> {
    let mut area: Option<[f32; 4]> = None;
    if dd.d.list.0 != 0 {
        if let Some(u) = world.entity(dd.d.list.0).and_then(|e| world.get::<Ui>(e)) {
            area = Some(union(area, rect_of(u)));
        }
    }
    for o in opts.iter().filter(|o| o.2.dropdown.0 == dd.uid && o.3.visible) {
        area = Some(union(area, rect_of(&o.3)));
    }
    area.filter(|a| a.iter().all(|v| v.is_finite()))
}

/// The lowest-numbered `UiNav` and where its focus is.
fn nav_of(world: &mut World) -> Option<(Entity, u64)> {
    let mut best: Option<(u64, Entity, u64)> = None;
    let _ = world.for_each_uid::<(&UiNav,), _>(|e, uid, (n,)| {
        if best.is_none_or(|b| uid < b.0) {
            best = Some((uid, e, n.current.0));
        }
    });
    best.map(|b| (b.1, b.2))
}

/// Before anything reacts to the pointer: the part of an open drop-down that has to do with the pointer. A press outside the header and the list closes the list and goes no further (the buttons
/// under it neither hover nor press until the button is let go), and the buttons the open list covers do not react where it covers them.
pub(crate) fn popup_pointer(world: &mut World) {
    let mut dds = dropdowns_of(world);
    if dds.is_empty() {
        return;
    }
    let opts = options_of(world);
    let ptr = pointer(world);
    let down = world.resource("input.mouse_left").is_some_and(|v| v > 0.5);
    let parts: BTreeSet<u64> = dds.iter().map(|d| d.uid).chain(opts.iter().map(|o| o.0)).collect();
    let (mut swallow_all, mut swallow_release) = (false, false);
    let mut covers: Vec<[f32; 4]> = Vec::new();
    for dd in dds.iter_mut() {
        let press_edge = down && !dd.d.down;
        if dd.d.swallow {
            if down {
                swallow_all = true;
            } else {
                swallow_release = true;
                dd.d.swallow = false;
            }
        }
        if dd.d.open {
            let area = area_of(world, dd, &opts);
            let over_header = ptr.is_some_and(|p| inside(rect_of(&dd.header), p));
            let over_area = ptr.is_some_and(|p| area.is_some_and(|a| inside(a, p)));
            if press_edge && !over_header && !over_area {
                dd.d.open = false;
                dd.d.swallow = true;
                swallow_all = true;
            } else if let Some(a) = area {
                covers.push(a);
            }
        }
        dd.d.down = down;
        if let Some(w) = world.get_mut::<UiDropdown>(dd.e) {
            *w = dd.d;
        }
    }
    if !(swallow_all || swallow_release || !covers.is_empty()) {
        return;
    }
    let mut buttons: Vec<(u64, Entity, [f32; 4])> = Vec::new();
    let _ = world.for_each_uid::<(&Ui,), _>(|e, uid, (u,)| {
        if u.button && !parts.contains(&uid) {
            buttons.push((uid, e, rect_of(u)));
        }
    });
    for (_, e, r) in buttons {
        let covered = ptr.is_some_and(|p| inside(r, p) && covers.iter().any(|a| inside(*a, p)));
        if let Some(u) = world.get_mut::<Ui>(e) {
            if swallow_all || covered {
                u.hover = false;
                u.pressed = false;
                u.clicked = false;
            } else if swallow_release {
                u.clicked = false;
            }
        }
    }
}

/// After the buttons have been clicked (by the pointer or by `ui_accept`): header and option clicks, the list shown or hidden, the header's text, the resource, tabs.
pub(crate) fn update(world: &mut World) {
    dropdowns(world);
    tabs(world);
}

fn dropdowns(world: &mut World) {
    let dds = dropdowns_of(world);
    if dds.is_empty() {
        return;
    }
    let opts = options_of(world);
    let cancel = pressed(world, "ui_cancel");
    let nav = nav_of(world);
    for dd in dds {
        let mut d = dd.d;
        d.changed = false;
        let mine: Vec<&Opt> = opts.iter().filter(|o| o.2.dropdown.0 == dd.uid).collect();
        let was_open = d.open;
        if dd.header.clicked {
            d.open = !d.open;
        }
        // a pick
        if let Some(o) = mine.iter().find(|o| o.3.clicked) {
            if d.selected != o.2.index {
                d.selected = o.2.index;
                d.changed = true;
            }
            d.open = false;
        }
        if d.open && cancel {
            d.open = false;
        }
        // the focus went to something else: the list closes (a stale focus from before it opened does not count)
        if let Some((_, cur)) = nav {
            if d.open && was_open && cur != 0 && cur != d.focus.0 && cur != dd.uid && !mine.iter().any(|o| o.0 == cur) {
                d.open = false;
            }
            d.focus.0 = cur;
        }
        // opened from the keyboard or a pad (the focus is on the header): the focus goes to the picked option, so that up and down start from there
        if d.open && !was_open {
            if let Some((ne, cur)) = nav {
                if cur == dd.uid {
                    let target = mine.iter().find(|o| o.2.index == d.selected).or(mine.first()).map(|o| o.0);
                    if let Some(t) = target {
                        if let Some(n) = world.get_mut::<UiNav>(ne) {
                            n.current.0 = t;
                        }
                        d.focus.0 = t;
                    }
                }
            }
        }
        // picked or cancelled with the focus on one of the options (which is about to be hidden): the focus goes back to the header
        if was_open && !d.open {
            if let Some((ne, cur)) = nav {
                if mine.iter().any(|o| o.0 == cur) {
                    if let Some(n) = world.get_mut::<UiNav>(ne) {
                        n.current.0 = dd.uid;
                    }
                    d.focus.0 = dd.uid;
                }
            }
        }
        // the list is shown while it is open
        if d.list.0 != 0 {
            if let Some(le) = world.entity(d.list.0) {
                if let Some(g) = world.get_mut::<UiGroup>(le) {
                    g.shown = d.open;
                } else if let Some(u) = world.get_mut::<Ui>(le) {
                    u.visible = d.open;
                }
            }
        }
        // the header says what is picked
        if let Some(o) = mine.iter().find(|o| o.2.index == d.selected) {
            if let Some(u) = world.get_mut::<Ui>(dd.e) {
                u.text = o.3.text;
            }
        }
        let name = world.strings().get(d.resource.0).to_string();
        if !name.is_empty() {
            let _ = world.set_resource(&name, Some(d.selected as f64));
        }
        if let Some(w) = world.get_mut::<UiDropdown>(dd.e) {
            *w = d;
        }
    }
}

fn tabs(world: &mut World) {
    let mut sets: Vec<(u64, Entity, UiTabs)> = Vec::new();
    let _ = world.for_each_uid::<(&UiTabs,), _>(|e, uid, (t,)| sets.push((uid, e, *t)));
    if sets.is_empty() {
        return;
    }
    sets.sort_by_key(|s| s.0);
    let mut items: Vec<(u64, Entity, UiTab, bool)> = Vec::new();
    let _ = world.for_each_uid::<(&UiTab, &Ui), _>(|e, uid, (t, u)| items.push((uid, e, *t, u.clicked)));
    items.sort_by_key(|i| i.0);
    for s in sets.iter_mut() {
        s.2.changed = false;
    }
    for it in &items {
        if it.3 {
            if let Some(s) = sets.iter_mut().find(|s| s.0 == it.2.tabs.0) {
                if s.2.selected != it.2.index {
                    s.2.selected = it.2.index;
                    s.2.changed = true;
                }
            }
        }
    }
    for it in &items {
        let Some(selected) = sets.iter().find(|s| s.0 == it.2.tabs.0).map(|s| s.2.selected) else { continue };
        let shown = selected == it.2.index;
        if let Some(u) = world.get_mut::<Ui>(it.1) {
            u.color = if shown { it.2.on_color } else { it.2.off_color };
        }
        if it.2.panel.0 != 0 {
            if let Some(pe) = world.entity(it.2.panel.0) {
                if let Some(g) = world.get_mut::<UiGroup>(pe) {
                    g.shown = shown;
                } else if let Some(u) = world.get_mut::<Ui>(pe) {
                    u.visible = shown;
                }
            }
        }
    }
    for (_, e, t) in sets {
        let name = world.strings().get(t.resource.0).to_string();
        if !name.is_empty() {
            let _ = world.set_resource(&name, Some(t.selected as f64));
        }
        if let Some(w) = world.get_mut::<UiTabs>(e) {
            *w = t;
        }
    }
}
