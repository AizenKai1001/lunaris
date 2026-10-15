//! `check` rules for the conveniences of menus and scene changes: `UiNavAuto` (the focus a menu takes by itself), `SceneReset` (the resources a scene switch resets) and `UiScale` and
//! `UiEdge` (the interface on screens of other shapes).
use super::Problems;
use lunaris_ecs::{valid_resource_name, Result, World};
use lunaris_sim::components::{SceneReset, SceneSwitch, Text, Ui, UiEdge, UiNav, UiNavAuto, UiScale};

pub(super) fn run(world: &mut World, problems: &mut Problems) -> Result<()> {
    let mut autos: Vec<u64> = Vec::new();
    world.for_each_uid::<(&UiNavAuto,), _>(|_, uid, _| autos.push(uid))?;
    autos.sort_unstable();
    for uid in autos {
        if !world.entity(uid).is_some_and(|e| world.get::<UiNav>(e).is_some()) {
            problems.add("warning", Some(uid), "has a UiNavAuto but no UiNav on the same entity, so no menu is focused".into(), "put `UiNav` and `UiNavAuto` on one entity");
        }
    }
    let mut resets: Vec<(u64, u32)> = Vec::new();
    world.for_each_uid::<(&SceneReset,), _>(|_, uid, (r,)| resets.push((uid, r.resources.0)))?;
    resets.sort_by_key(|r| r.0);
    for (uid, sym) in resets {
        if !world.entity(uid).is_some_and(|e| world.get::<SceneSwitch>(e).is_some()) {
            problems.add("warning", Some(uid), "has a SceneReset but no SceneSwitch on the same entity, so it resets nothing".into(), "put `SceneReset` on the entity that has the `SceneSwitch` (the switch that fires decides what is reset)");
        }
        let names = lunaris_sim::scenes::reset_names(world.strings().get(sym));
        if names.is_empty() {
            problems.add("warning", Some(uid), "SceneReset.resources names nothing, so it resets nothing".into(), "list the resources, as `paused leave time.scale` (a name ending in `*` is a prefix: `fx.*`)");
        }
        for n in names {
            let plain = n.strip_suffix('*').unwrap_or(&n).trim_end_matches('.');
            let everything = n == "*";
            if !everything && !valid_resource_name(plain) {
                problems.add("warning", Some(uid), format!("SceneReset: `{n}` is not a resource name"), "resource names are letters, digits, `_` and dots, like `time.scale`; a trailing `*` makes a prefix");
            }
        }
    }
    // the interface on screens of other shapes
    let mut scales: Vec<(u64, f32)> = Vec::new();
    world.for_each_uid::<(&UiScale,), _>(|_, uid, (s,)| scales.push((uid, s.aspect)))?;
    scales.sort_by_key(|s| s.0);
    for (n, (uid, aspect)) in scales.iter().enumerate() {
        if !(aspect.is_finite() && *aspect > 0.05 && *aspect < 20.0) {
            problems.add("warning", Some(*uid), format!("UiScale.aspect {aspect} is not a shape a picture can have, so it is ignored"), "the width over the height the interface was made for: 1.7778 for 16:9, 1.3333 for 4:3");
        } else if n > 0 {
            problems.add("warning", Some(*uid), format!("a second UiScale: only the one with the lowest uid ({}) is used", scales[0].0), "keep one entity with a UiScale");
        }
    }
    let mut edges: Vec<(u64, UiEdge)> = Vec::new();
    world.for_each_uid::<(&UiEdge,), _>(|_, uid, (e,)| edges.push((uid, *e)))?;
    edges.sort_by_key(|e| e.0);
    for (uid, e) in edges {
        if scales.is_empty() {
            problems.add("warning", Some(uid), "has a UiEdge but the world has no UiScale, so it does nothing".into(), "add a `UiScale` entity: the interface is then laid out in the safe area, and a `UiEdge` moves single elements to the edges of the picture");
        }
        let (has_ui, button) = world.entity(uid).map_or((false, false), |ent| (world.get::<Ui>(ent).is_some() || world.get::<Text>(ent).is_some(), world.get::<Ui>(ent).is_some_and(|u| u.button)));
        if !has_ui {
            problems.add("warning", Some(uid), "has a UiEdge but no Ui or Text on the same entity".into(), "put `UiEdge` on the element it moves");
        }
        if !(e.x.is_finite() && e.y.is_finite() && (0.0..=1.0).contains(&e.x) && (0.0..=1.0).contains(&e.y)) {
            problems.add("warning", Some(uid), format!("UiEdge x {} and y {} must be between 0 and 1 (0 the left or top edge, 1 the right or bottom edge)", e.x, e.y), "0.5 is the middle, which is no pin at all");
        }
        if button && (e.screen || e.x != 0.5 || e.y != 0.5) {
            problems.add("warning", Some(uid), "a button with a UiEdge is drawn at the edge, but the mouse rectangle that decides a click stays where the safe area puts it".into(), "use UiEdge for bars, backdrops and fades; keep buttons inside the safe area");
        }
    }
    Ok(())
}
