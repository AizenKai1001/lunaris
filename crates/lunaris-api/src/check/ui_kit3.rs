//! `check` rules for UI kit v3 (docs/UI.md): scroll views and their bars, and (in later commits) dropdowns, tabs, drag and drop, tooltips and the radial fill. Each rule says what the mistake does:
//! these components point at each other by uid, so a wrong number is a widget that quietly does nothing.
use super::Problems;
use lunaris_ecs::{valid_resource_name, Result, World};
use lunaris_sim::components::{Parent, Ui, UiDrag, UiDrop, UiDropdown, UiGroup, UiLayout, UiOption, UiRadial, UiScroll, UiScrollBar, UiStyle, UiTab, UiTabs, UiTooltip, UiTooltipBox};
use std::collections::{BTreeMap, BTreeSet};

fn finite(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite())
}

pub(super) fn run(world: &mut World, problems: &mut Problems) -> Result<()> {
    scroll(world, problems)?;
    select(world, problems)?;
    drag(world, problems)?;
    tooltips(world, problems)?;
    radial(world, problems)
}

/// Tooltips and their box.
fn tooltips(world: &mut World, problems: &mut Problems) -> Result<()> {
    let mut tips: Vec<(u64, UiTooltip)> = Vec::new();
    world.for_each_uid::<(&UiTooltip,), _>(|_, uid, (t,)| tips.push((uid, *t)))?;
    let mut boxes: Vec<(u64, UiTooltipBox)> = Vec::new();
    world.for_each_uid::<(&UiTooltipBox,), _>(|_, uid, (b,)| boxes.push((uid, *b)))?;
    tips.sort_by_key(|t| t.0);
    boxes.sort_by_key(|b| b.0);
    let ui_of = |world: &World, uid: u64| world.entity(uid).and_then(|e| world.get::<Ui>(e)).copied();
    for (uid, t) in &tips {
        if ui_of(world, *uid).is_none() {
            problems.add("warning", Some(*uid), "has a UiTooltip but no Ui on the same entity, so there is no rectangle for the pointer to rest on".into(), "put `UiTooltip` on the `Ui` it explains");
        }
        if world.strings().get(t.text.0).is_empty() {
            problems.add("warning", Some(*uid), "the UiTooltip has no text, so it would show an empty box".into(), "set `text` (it may hold {resource} placeholders and \\n for a new line)");
        }
        if !(t.delay.is_finite() && t.delay >= 0.0) {
            problems.add("warning", Some(*uid), format!("UiTooltip.delay {} is not a number of seconds from 0 up, so it counts as 0", t.delay), "0.5 is usual");
        }
        if boxes.is_empty() {
            problems.add("warning", Some(*uid), "the world has no UiTooltipBox, so no tooltip is ever shown".into(), "add a `Ui` with a `UiTooltipBox` (a dark rounded rectangle with a high `layer`): the engine puts the text in it");
        }
    }
    for (n, (uid, b)) in boxes.iter().enumerate() {
        match ui_of(world, *uid) {
            None => problems.add("warning", Some(*uid), "has a UiTooltipBox but no Ui on the same entity, so there is nothing to show the text in".into(), "put `UiTooltipBox` on the `Ui` that is the box"),
            Some(u) if u.button => problems.add("warning", Some(*uid), "the UiTooltipBox is a button, so it would take clicks and hover like one".into(), "leave `button` false"),
            Some(_) => {}
        }
        if n > 0 {
            problems.add("warning", Some(*uid), format!("a second UiTooltipBox: only the one with the lowest uid ({}) is used", boxes[0].0), "keep one box");
        }
        if !(b.padding.is_finite() && b.padding >= 0.0) || !b.offset.iter().all(|v| v.is_finite()) {
            problems.add("warning", Some(*uid), "UiTooltipBox.padding or offset is not a number, so the box is placed at 0".into(), "padding 0.012, offset [0.012, 0.025] are the defaults");
        }
    }
    Ok(())
}

/// Radial fills.
fn radial(world: &mut World, problems: &mut Problems) -> Result<()> {
    let mut dials: Vec<(u64, UiRadial)> = Vec::new();
    world.for_each_uid::<(&UiRadial,), _>(|_, uid, (r,)| dials.push((uid, *r)))?;
    dials.sort_by_key(|d| d.0);
    for (uid, r) in &dials {
        let ent = world.entity(*uid);
        let Some(ui) = ent.and_then(|e| world.get::<Ui>(e)).copied() else {
            problems.add("warning", Some(*uid), "has a UiRadial but no Ui on the same entity, so there is no rectangle to fill".into(), "put `UiRadial` on the `Ui` that is the dial");
            continue;
        };
        if !r.value.is_finite() || !r.start.is_finite() {
            problems.add("warning", Some(*uid), format!("UiRadial.value {} or start {} is not a number, so nothing is drawn", r.value, r.start), "value is 0 to 1 of a turn, start is degrees clockwise from twelve o'clock");
        }
        if r.shape > 1 {
            problems.add("warning", Some(*uid), format!("UiRadial.shape {} is not 0 (the circle) or 1 (the whole rectangle), so it counts as the rectangle", r.shape), "");
        }
        if !(r.inner.is_finite() && (0.0..=1.0).contains(&r.inner)) {
            problems.add("warning", Some(*uid), format!("UiRadial.inner {} is not between 0 and 1, so it is cut to that", r.inner), "the share of the circle's radius that is a hole");
        }
        if r.shape == 1 && r.inner > 0.0 {
            problems.add("warning", Some(*uid), "UiRadial.inner only makes a hole in the circle (shape 0), so it is ignored for the whole-rectangle shape".into(), "");
        }
        let styled = ent.and_then(|e| world.get::<UiStyle>(e)).is_some_and(|s| s.shadow_color[3] > 0.0 || s.color2[3] > 0.0 || s.image.0 != 0);
        if ui.border > 0.0 || ui.fill < 1.0 || styled {
            problems.add("warning", Some(*uid), "a UiRadial replaces the rectangle's bar, border, shadow, gradient and picture, so those are not drawn".into(), "draw the dial's frame or picture with a second `Ui` behind it");
        }
    }
    Ok(())
}

/// Things that are dragged and the places they are dropped on.
fn drag(world: &mut World, problems: &mut Problems) -> Result<()> {
    let mut items: Vec<(u64, UiDrag)> = Vec::new();
    world.for_each_uid::<(&UiDrag,), _>(|_, uid, (d,)| items.push((uid, *d)))?;
    let mut slots: Vec<(u64, UiDrop)> = Vec::new();
    world.for_each_uid::<(&UiDrop,), _>(|_, uid, (d,)| slots.push((uid, *d)))?;
    items.sort_by_key(|i| i.0);
    slots.sort_by_key(|s| s.0);
    let ui_of = |world: &World, uid: u64| world.entity(uid).and_then(|e| world.get::<Ui>(e)).copied();
    for (uid, d) in &items {
        match ui_of(world, *uid) {
            None => problems.add("warning", Some(*uid), "has a UiDrag but no Ui on the same entity, so there is nothing to pick up".into(), "put `UiDrag` on the `Ui` that is the thing to drag"),
            Some(u) if !u.button => problems.add("warning", Some(*uid), "a UiDrag is not a button, so a press does not pick it up".into(), "set `button` true on its `Ui`"),
            Some(_) => {}
        }
        if d.slot.0 != 0 && !slots.iter().any(|s| s.0 == d.slot.0) {
            problems.add("warning", Some(*uid), format!("the UiDrag's slot {} is not an entity with a UiDrop, so it is treated as free and goes home when dropped", d.slot.0), "set `slot` to the uid of the `UiDrop` it sits in, or 0");
        } else if d.slot.0 != 0 && items.iter().any(|o| o.0 < *uid && o.1.slot.0 == d.slot.0) {
            problems.add("warning", Some(*uid), format!("another UiDrag sits in the same slot {}, so the two are drawn on top of each other", d.slot.0), "give each thing its own slot (a drop on an occupied slot trades places)");
        }
        if d.kind > 31 {
            problems.add("warning", Some(*uid), format!("UiDrag.kind {} is above 31, so it counts as {}", d.kind, d.kind & 31), "kinds are 0 to 31, the bits of a slot's `accept`");
        }
    }
    let mut names: BTreeSet<String> = BTreeSet::new();
    for (uid, s) in &slots {
        if ui_of(world, *uid).is_none() {
            problems.add("warning", Some(*uid), "has a UiDrop but no Ui on the same entity, so there is nowhere to drop".into(), "put `UiDrop` on the `Ui` rectangle of the slot");
        }
        if s.accept == 0 {
            problems.add("warning", Some(*uid), "UiDrop.accept is 0, so nothing is ever dropped here".into(), "bit n takes a UiDrag of kind n; the default takes all");
        }
        let name = world.strings().get(s.name.0).to_string();
        if !name.is_empty() {
            if !valid_resource_name(&format!("drop.{name}")) {
                problems.add("warning", Some(*uid), format!("UiDrop.name `{name}` does not make a resource name (drop.{name}), so drops here are not reported in resources"), "letters, digits, `_` and dots");
            }
            if !names.insert(name.clone()) {
                problems.add("warning", Some(*uid), format!("another UiDrop has the name `{name}` too, so they report in the same resources"), "give each slot its own name (or none: its uid is used)");
            }
        }
    }
    Ok(())
}

/// The resource name a drop-down or a set of tabs writes to, when it is not a name a resource can have.
fn resource_problem(world: &World, uid: u64, sym: u32, what: &str, problems: &mut Problems) {
    let name = world.strings().get(sym);
    if !name.is_empty() && !valid_resource_name(name) {
        problems.add("warning", Some(uid), format!("{what}'s resource `{name}` is not a resource name, so nothing is written"), "resource names are letters, digits, `_` and dots, like `ui.quality`");
    }
}

/// Drop-downs and tabs: the numbers that point at other entities, and what each leaves undone when it points at nothing.
fn select(world: &mut World, problems: &mut Problems) -> Result<()> {
    let mut dds: Vec<(u64, UiDropdown)> = Vec::new();
    world.for_each_uid::<(&UiDropdown,), _>(|_, uid, (d,)| dds.push((uid, *d)))?;
    let mut opts: Vec<(u64, UiOption)> = Vec::new();
    world.for_each_uid::<(&UiOption,), _>(|_, uid, (o,)| opts.push((uid, *o)))?;
    let mut sets: Vec<(u64, UiTabs)> = Vec::new();
    world.for_each_uid::<(&UiTabs,), _>(|_, uid, (t,)| sets.push((uid, *t)))?;
    let mut tabs: Vec<(u64, UiTab)> = Vec::new();
    world.for_each_uid::<(&UiTab,), _>(|_, uid, (t,)| tabs.push((uid, *t)))?;
    dds.sort_by_key(|d| d.0);
    opts.sort_by_key(|o| o.0);
    sets.sort_by_key(|s| s.0);
    tabs.sort_by_key(|t| t.0);
    let button = |world: &World, uid: u64| world.entity(uid).and_then(|e| world.get::<Ui>(e)).map(|u| u.button);
    let has = |world: &World, uid: u64| world.entity(uid).is_some();
    let has_group = |world: &World, uid: u64| world.entity(uid).is_some_and(|e| world.get::<UiGroup>(e).is_some());
    let has_ui = |world: &World, uid: u64| world.entity(uid).is_some_and(|e| world.get::<Ui>(e).is_some());
    for (uid, d) in &dds {
        match button(world, *uid) {
            None => problems.add("warning", Some(*uid), "has a UiDropdown but no Ui on the same entity: the header has to be a `Ui`".into(), "put `UiDropdown` on the `Ui` button that opens the list"),
            Some(false) => problems.add("warning", Some(*uid), "the header of a UiDropdown is not a button, so a click cannot open the list".into(), "set `button` true on the header's `Ui`"),
            Some(true) => {}
        }
        resource_problem(world, *uid, d.resource.0, "the UiDropdown", problems);
        let mine: Vec<&(u64, UiOption)> = opts.iter().filter(|o| o.1.dropdown.0 == *uid).collect();
        if mine.is_empty() {
            problems.add("warning", Some(*uid), "no UiOption names this UiDropdown, so there is nothing to pick".into(), &format!("put a `UiOption {{dropdown: {uid}, index: 0}}` on each option button"));
        } else {
            let indexes: BTreeSet<u32> = mine.iter().map(|o| o.1.index).collect();
            if indexes.len() < mine.len() {
                problems.add("warning", Some(*uid), "two options of this UiDropdown have the same `index`, so they pick the same value".into(), "give each `UiOption` its own number");
            }
            if !indexes.contains(&d.selected) {
                problems.add("warning", Some(*uid), format!("UiDropdown.selected {} is the index of no option, so the header keeps its own text until one is picked", d.selected), "set `selected` to the `index` of one of the options");
            }
        }
        if d.list.0 == 0 {
            problems.add("warning", Some(*uid), "this UiDropdown has no `list`, so nothing hides the options while it is closed".into(), "set `list` to the entity that holds the options (a `Ui` with a `UiGroup`)");
        } else if !has(world, d.list.0) {
            problems.add("warning", Some(*uid), format!("the UiDropdown's list {} does not exist, so nothing hides the options while it is closed", d.list.0), "set `list` to the uid of the entity that holds the options, or 0");
        } else if !has_group(world, d.list.0) {
            problems.add("warning", Some(*uid), "the list of this UiDropdown has no UiGroup, so only the list's own rectangle is hidden while it is closed, not the options in it".into(), "add a `UiGroup` to the list entity");
        }
    }
    for (uid, o) in &opts {
        let target_ok = has(world, o.dropdown.0) && world.entity(o.dropdown.0).is_some_and(|e| world.get::<UiDropdown>(e).is_some());
        if !target_ok {
            problems.add("warning", Some(*uid), format!("this UiOption's dropdown {} is not an entity with a UiDropdown, so picking it does nothing", o.dropdown.0), "set `dropdown` to the uid of the header button");
        }
        match button(world, *uid) {
            None => problems.add("warning", Some(*uid), "has a UiOption but no Ui on the same entity".into(), "put `UiOption` on the `Ui` button of the option"),
            Some(false) => problems.add("warning", Some(*uid), "this UiOption is not a button, so it cannot be clicked".into(), "set `button` true on the option's `Ui`"),
            Some(true) => {}
        }
    }
    for (uid, t) in &tabs {
        match button(world, *uid) {
            None => problems.add("warning", Some(*uid), "has a UiTab but no Ui on the same entity: a tab has to be a `Ui`".into(), "put `UiTab` on the `Ui` button of the tab"),
            Some(false) => problems.add("warning", Some(*uid), "this UiTab is not a button, so a click cannot select it".into(), "set `button` true on the tab's `Ui`"),
            Some(true) => {}
        }
        if !sets.iter().any(|s| s.0 == t.tabs.0) {
            problems.add("warning", Some(*uid), format!("this UiTab's tabs {} is not an entity with a UiTabs, so selecting it does nothing", t.tabs.0), "set `tabs` to the uid of the entity that has the `UiTabs`");
        }
        if t.panel.0 == 0 || !has(world, t.panel.0) {
            problems.add("warning", Some(*uid), format!("this UiTab's panel {} does not exist, so it shows nothing", t.panel.0), "set `panel` to the uid of the panel this tab shows");
        } else if !has_group(world, t.panel.0) {
            if has_ui(world, t.panel.0) {
                problems.add("warning", Some(*uid), "the panel of this UiTab has no UiGroup, so only the panel's own rectangle is hidden when another tab is selected, not what is in it".into(), "add a `UiGroup` to the panel");
            } else {
                problems.add("warning", Some(*uid), "the panel of this UiTab has neither a UiGroup nor a Ui, so it cannot be shown or hidden".into(), "add a `UiGroup` to the panel");
            }
        }
        if tabs.iter().any(|o| o.0 < *uid && o.1.tabs.0 == t.tabs.0 && o.1.index == t.index) {
            problems.add("warning", Some(*uid), format!("another UiTab of the same set has the index {}, so they are selected together", t.index), "give each `UiTab` its own `index`");
        }
    }
    for (uid, s) in &sets {
        resource_problem(world, *uid, s.resource.0, "the UiTabs", problems);
        let mine: Vec<&(u64, UiTab)> = tabs.iter().filter(|t| t.1.tabs.0 == *uid).collect();
        if mine.is_empty() {
            problems.add("warning", Some(*uid), "no UiTab names this UiTabs, so there are no tabs".into(), &format!("put a `UiTab {{tabs: {uid}, index: 0, panel: ...}}` on each tab button"));
        } else if !mine.iter().any(|t| t.1.index == s.selected) {
            problems.add("warning", Some(*uid), format!("UiTabs.selected {} is the index of no tab, so every panel is hidden", s.selected), "set `selected` to the `index` of one of the tabs");
        }
    }
    Ok(())
}

fn scroll(world: &mut World, problems: &mut Problems) -> Result<()> {
    let mut views: Vec<(u64, UiScroll)> = Vec::new();
    world.for_each_uid::<(&UiScroll,), _>(|_, uid, (s,)| views.push((uid, *s)))?;
    let mut bars: Vec<(u64, UiScrollBar)> = Vec::new();
    world.for_each_uid::<(&UiScrollBar,), _>(|_, uid, (b,)| bars.push((uid, *b)))?;
    if views.is_empty() && bars.is_empty() {
        return Ok(());
    }
    views.sort_by_key(|v| v.0);
    bars.sort_by_key(|b| b.0);
    let mut parents: BTreeMap<u64, u64> = BTreeMap::new();
    world.for_each_uid::<(&Parent,), _>(|_, uid, (p,)| {
        if p.target.0 != 0 {
            parents.insert(uid, p.target.0);
        }
    })?;
    let inside = |uid: u64, view: u64| {
        let (mut cur, mut n) = (uid, 0);
        while let Some(p) = parents.get(&cur) {
            if *p == view {
                return true;
            }
            cur = *p;
            n += 1;
            if n > 32 {
                break;
            }
        }
        false
    };
    for (uid, s) in &views {
        let ui = world.entity(*uid).and_then(|e| world.get::<Ui>(e)).copied();
        let Some(ui) = ui else {
            problems.add("warning", Some(*uid), "has a UiScroll but no Ui on the same entity, so there is no viewport".into(), "put `UiScroll` on the `Ui` rectangle that should show part of the content");
            continue;
        };
        if !finite(&ui.pos) || !finite(&ui.size) || ui.size[0] <= 0.0 || ui.size[1] <= 0.0 {
            problems.add("warning", Some(*uid), format!("the viewport of this UiScroll has no size ({} x {}), so nothing is shown", ui.size[0], ui.size[1]), "give the `Ui` a `size` above 0");
        }
        if world.entity(*uid).and_then(|e| world.get::<UiLayout>(e)).is_some() {
            problems.add("warning", Some(*uid), "a UiScroll and a UiLayout on one entity: the layout puts the children in the viewport every tick, so they cannot scroll".into(), "put the `UiLayout` on a child of the view that is taller than the viewport, and the rows under that");
        }
        if !(s.speed.is_finite() && s.speed > 0.0) {
            problems.add("warning", Some(*uid), format!("UiScroll.speed {} is not above 0, so the mouse wheel does not scroll it", s.speed), "a notch of the wheel moves the content by `speed`, a fraction of the picture's height (0.06 by default)");
        }
        if !finite(&s.content_size) || s.content_size.iter().any(|c| *c < 0.0) {
            problems.add("warning", Some(*uid), format!("UiScroll.content_size {:?} is not a size, so it is measured from the children instead", s.content_size), "0 measures the visible children; otherwise give the content's width and height as fractions of the picture");
        }
        if !parents.keys().any(|k| inside(*k, *uid)) {
            problems.add("warning", Some(*uid), "nothing is inside this UiScroll: no entity has it as its `Parent`, so there is nothing to scroll or clip".into(), "give the rows, panels and texts `Parent {target: <this uid>}` (or a `Parent` chain that reaches it)");
        }
    }
    for (uid, b) in &bars {
        let thumb = world.entity(*uid).and_then(|e| world.get::<Ui>(e)).copied();
        match thumb {
            None => problems.add("warning", Some(*uid), "has a UiScrollBar but no Ui on the same entity: the thumb has to be a `Ui`".into(), "put `UiScrollBar` on the `Ui` rectangle that is the thumb"),
            Some(t) if !t.button => problems.add("warning", Some(*uid), "the thumb of a UiScrollBar is not a button, so it cannot be dragged".into(), "set `button` true on the thumb's `Ui` (and on its track, to click it)"),
            Some(_) => {}
        }
        if world.entity(b.view.0).and_then(|e| world.get::<UiScroll>(e)).is_none() {
            problems.add("warning", Some(*uid), format!("the UiScrollBar's view {} is not an entity with a UiScroll, so the bar scrolls nothing and stays where it is", b.view.0), "set `view` to the uid of the scroll view");
        }
        if b.track.0 != 0 {
            match world.entity(b.track.0).and_then(|e| world.get::<Ui>(e)).copied() {
                None => problems.add("warning", Some(*uid), format!("the UiScrollBar's track {} does not exist or has no Ui", b.track.0), "set `track` to a `Ui` entity drawn behind the thumb, or 0 for none"),
                Some(t) if !t.button => problems.add("warning", Some(*uid), "the track of a UiScrollBar is not a button, so a click on it does not move the thumb".into(), "set `button` true on the track's `Ui`"),
                Some(_) => {}
            }
        }
        if !(b.thickness.is_finite() && b.thickness > 0.0) {
            problems.add("warning", Some(*uid), format!("UiScrollBar.thickness {} is not above 0, so the bar has no width", b.thickness), "a fraction of the picture's height, 0.014 by default");
        }
    }
    Ok(())
}
