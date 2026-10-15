//! Prefabs: named entity templates. A prefab is a JSON object that says what an entity (and its children) looks like; spawning it makes
//! real entities with the next free uids. Games use them for everything that appears more than once (trees, bullets, enemies, pickups,
//! whole rooms), and an AI agent defines one instead of repeating a long `spawn` call.
//!
//! ```json
//! { "components": { "Name": {"value": "Tree"}, "Transform": {"pos": [0, 0, 0]}, "Collider": {"shape": 0} },
//!   "children": [ { "name": "Crown", "components": { "LocalTransform": {"pos": [0, 2, 0]}, "MeshRenderer": {"mesh": "sphere"} } },
//!                 { "prefab": "Branch", "components": { "LocalTransform": {"pos": [0.5, 1, 0]} } } ] }
//! ```
//!
//! - `components` are the root entity's components (partial values use the component defaults).
//! - `children` are spawned with a `Parent` that points at the entity above them. A child that has a `LocalTransform` and no `Transform`
//!   gets a `Transform` of the same value, so it already stands in the right place before the first tick.
//! - `prefab` inherits another prefab's components and children: this node's own `components` replace the inherited ones field by field,
//!   its own `children` come after the inherited ones.
//! - `name` is a shorthand for `Name.value`.
//!
//! Spawning with `overrides` (`{"Transform": {"pos": [3, 0, 1]}}`) replaces fields of the root entity's components. A prefab is a template,
//! not a link: changing a prefab later does not change entities already spawned from it, and a saved scene holds the plain entities.
//! Prefabs live next to the graph systems (`ScriptSet::prefabs`), are part of the state hash, of scenes and of recordings.
use crate::command::Command;
use lunaris_ecs::{EcsError, Result, World};
use lunaris_json::Json;
use std::collections::BTreeMap;

pub type Prefabs = BTreeMap<String, Json>;

/// How deep prefabs may nest (a child of a child of ...); also stops a prefab that contains itself.
pub const MAX_DEPTH: usize = 8;
/// The most entities one spawn may create.
pub const MAX_ENTITIES: usize = 4096;

/// One entity of an expanded prefab: where it hangs and what it has.
#[derive(Clone, Debug)]
pub struct Planned {
    /// Index (in the plan) of the parent; `None` for the root.
    pub parent: Option<usize>,
    pub components: Vec<(String, Json)>,
}

fn no_prefab(prefabs: &Prefabs, name: &str) -> EcsError {
    let all: Vec<&str> = prefabs.keys().map(String::as_str).collect();
    EcsError::new("no_such_prefab", format!("no prefab named `{name}`")).hint(format!("prefabs: {}", if all.is_empty() { "(none; define one with define_prefab)".to_string() } else { all.join(", ") }))
}

/// Put `value` into the component list: fields of an object replace the same-named fields of what is there, anything else replaces it.
pub fn merge_component(comps: &mut Vec<(String, Json)>, name: &str, value: &Json) {
    match comps.iter_mut().find(|(n, _)| n == name) {
        Some((_, existing)) => {
            let both_objects = matches!(existing, Json::Obj(_)) && matches!(value, Json::Obj(_));
            match value {
                Json::Obj(fields) if both_objects => {
                    for (k, v) in fields {
                        existing.set(k, v.clone());
                    }
                }
                _ => *existing = value.clone(),
            }
        }
        None => comps.push((name.to_string(), value.clone())),
    }
}

/// A node's components and children with its `prefab` inheritance resolved.
fn resolve(prefabs: &Prefabs, def: &Json, depth: usize) -> Result<(Vec<(String, Json)>, Vec<Json>)> {
    if !matches!(def, Json::Obj(_)) {
        return Err(EcsError::new("bad_prefab", "a prefab (or a child) must be an object with `components`, `children` and/or `prefab`"));
    }
    let (mut comps, mut kids) = (Vec::new(), Vec::new());
    if let Some(base_name) = def.get("prefab") {
        let base_name = base_name.as_str().ok_or_else(|| EcsError::new("bad_prefab", "`prefab` must be the name of another prefab").at("prefab"))?;
        if depth >= MAX_DEPTH {
            return Err(EcsError::new("prefab_too_deep", format!("prefabs inherit or nest more than {MAX_DEPTH} levels deep (does `{base_name}` contain itself?)")));
        }
        let base = prefabs.get(base_name).ok_or_else(|| no_prefab(prefabs, base_name))?;
        let (c, k) = resolve(prefabs, base, depth + 1)?;
        comps = c;
        kids = k;
    }
    match def.get("components") {
        None | Some(Json::Null) => {}
        Some(Json::Obj(own)) => {
            for (name, value) in own {
                merge_component(&mut comps, name, value);
            }
        }
        Some(_) => return Err(EcsError::new("bad_prefab", "`components` must be an object keyed by component name").at("components")),
    }
    match def.get("children") {
        None | Some(Json::Null) => {}
        Some(Json::Arr(own)) => kids.extend(own.iter().cloned()),
        Some(_) => return Err(EcsError::new("bad_prefab", "`children` must be an array of prefab nodes").at("children")),
    }
    if let Some(n) = def.get("name").and_then(|n| n.as_str()) {
        merge_component(&mut comps, "Name", &Json::obj([("value", Json::str(n))]));
    }
    Ok((comps, kids))
}

fn expand_node(prefabs: &Prefabs, def: &Json, parent: Option<usize>, depth: usize, out: &mut Vec<Planned>) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(EcsError::new("prefab_too_deep", format!("prefabs nest more than {MAX_DEPTH} levels deep (does a prefab contain itself?)")));
    }
    if out.len() >= MAX_ENTITIES {
        return Err(EcsError::new("prefab_too_big", format!("a prefab spawn is limited to {MAX_ENTITIES} entities")));
    }
    let (components, kids) = resolve(prefabs, def, 0)?;
    let me = out.len();
    out.push(Planned { parent, components });
    for kid in &kids {
        expand_node(prefabs, kid, Some(me), depth + 1, out)?;
    }
    Ok(())
}

/// Expand prefab `name` into the entities it makes, parents before children; `overrides` replace fields of the root's components.
pub fn plan(prefabs: &Prefabs, name: &str, overrides: &[(String, Json)]) -> Result<Vec<Planned>> {
    let def = prefabs.get(name).ok_or_else(|| no_prefab(prefabs, name))?;
    let mut out = Vec::new();
    expand_node(prefabs, def, None, 0, &mut out)?;
    for (c, v) in overrides {
        merge_component(&mut out[0].components, c, v);
    }
    Ok(out)
}

/// Check that prefab `name` can be spawned: it expands (nested prefabs exist, depth and size are in bounds) and every component it names
/// exists. Values are checked when it is spawned.
pub fn validate(world: &World, prefabs: &Prefabs, name: &str) -> Result<()> {
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(|c| c.is_control()) {
        return Err(EcsError::new("bad_prefab", "a prefab name is 1 to 64 characters without control characters").at("name"));
    }
    for (i, p) in plan(prefabs, name, &[])?.iter().enumerate() {
        for (c, _) in &p.components {
            world.require_component(c).map_err(|e| e.at(format!("{}components.{c}", if i == 0 { String::new() } else { format!("children[{}].", i - 1) })))?;
        }
    }
    Ok(())
}

/// The spawn commands for one copy of a plan; the entities get the uids `base`, `base + 1`, ... in plan order.
pub fn spawn_commands(plan: &[Planned], base: u64) -> Vec<Command> {
    plan.iter()
        .enumerate()
        .map(|(i, p)| {
            let mut comps = p.components.clone();
            if let Some(parent) = p.parent {
                merge_component(&mut comps, "Parent", &Json::obj([("target", Json::from((base + parent as u64) as i64))]));
                let local = comps.iter().find(|(n, _)| n == "LocalTransform").map(|(_, v)| v.clone());
                if let (Some(local), false) = (local, comps.iter().any(|(n, _)| n == "Transform")) {
                    comps.push(("Transform".to_string(), local));
                }
            }
            Command::Spawn { uid: Some(base + i as u64), components: comps }
        })
        .collect()
}
