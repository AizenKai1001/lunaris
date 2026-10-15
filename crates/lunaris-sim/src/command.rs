//! Commands: every change an editor, a script or an AI makes to the world goes through here. Each command is
//! validated, applied atomically, and returns the commands that undo it. Commands round-trip through JSON so
//! sessions can be recorded and replayed.
use crate::graph::ScriptSet;
use lunaris_ecs::{parse_component_def, EcsError, Result, World};
use lunaris_json::Json;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Create an entity (with `uid` if given) holding the listed components (partial values use the component defaults).
    Spawn { uid: Option<u64>, components: Vec<(String, Json)> },
    Despawn { uid: u64 },
    /// Add a component, or merge fields into it when the entity already has it.
    Add { uid: u64, component: String, value: Json },
    /// Merge fields into a component the entity already has.
    Set { uid: u64, component: String, value: Json },
    Remove { uid: u64, component: String },
    /// Define a component at runtime. Not undoable.
    Define { def: Json },
    /// Add or replace a graph system (see `graph`). Undoable.
    DefineSystem { def: Json },
    RemoveSystem { name: String },
    SetSystemEnabled { name: String, enabled: bool },
    /// Add or replace a prefab (an entity template, see `prefab`). Undoable.
    DefinePrefab { name: String, def: Json },
    RemovePrefab { name: String },
    /// Set (`Some`) or remove (`None`) a world resource such as the score or a key's state. Undoable.
    SetResource { name: String, value: Option<f64> },
}

#[derive(Clone, Debug)]
pub struct Applied {
    /// The command as it actually ran (uids filled in), which is what a recording stores.
    pub executed: Command,
    /// Commands that undo this one, in the order to run them. `None` when the change cannot be undone.
    pub inverse: Option<Vec<Command>>,
    /// The entity this command created or touched, when there is exactly one.
    pub uid: Option<u64>,
}

/// The most entities a world may hold: a safety valve against runaway spawners, far above anything a game needs (the benchmark uses one million).
pub const MAX_ENTITIES: usize = 10_000_000;

fn no_system(scripts: &ScriptSet, name: &str) -> EcsError {
    let all: Vec<&str> = scripts.systems.iter().map(|s| s.name.as_str()).collect();
    EcsError::new("no_such_system", format!("no graph system named `{name}`")).hint(format!("graph systems: {}", if all.is_empty() { "(none)".to_string() } else { all.join(", ") }))
}

fn snapshot_entity(world: &World, uid: u64) -> Result<Vec<(String, Json)>> {
    let e = world.require_entity(uid)?;
    Ok(world.components_of(e).into_iter().map(|c| (world.info(c).name.clone(), world.get_json(e, c).expect("listed component exists"))).collect())
}

pub fn apply(world: &mut World, scripts: &mut ScriptSet, cmd: &Command) -> Result<Applied> {
    let r = apply_one(world, scripts, cmd);
    if r.is_ok() {
        crate::systems::hierarchy(world); // attached entities follow their parents right away, so nothing ever looks stale
    }
    r
}

fn apply_one(world: &mut World, scripts: &mut ScriptSet, cmd: &Command) -> Result<Applied> {
    match cmd {
        Command::Spawn { uid, components } => {
            // a spawner that feeds on its own spawns would otherwise eat all the memory of the machine
            if world.len() >= MAX_ENTITIES {
                return Err(EcsError::new("too_many_entities", format!("the world already holds {MAX_ENTITIES} entities, the most Lunaris will create")).hint("a graph system that spawns entities needs a way to stop (despawn what is old, or count what it made)"));
            }
            // resolve names first; spawn_json validates every value before creating anything
            let mut resolved = Vec::with_capacity(components.len());
            for (name, value) in components {
                resolved.push((world.require_component(name).map_err(|e| e.at(format!("components.{name}")))?, value));
            }
            let e = world.spawn_json(*uid, &resolved)?;
            let uid = world.uid_of(e).expect("just spawned");
            Ok(Applied { executed: Command::Spawn { uid: Some(uid), components: components.clone() }, inverse: Some(vec![Command::Despawn { uid }]), uid: Some(uid) })
        }
        Command::Despawn { uid } => {
            let before = snapshot_entity(world, *uid)?;
            let e = world.require_entity(*uid)?;
            world.despawn(e);
            Ok(Applied { executed: cmd.clone(), inverse: Some(vec![Command::Spawn { uid: Some(*uid), components: before }]), uid: Some(*uid) })
        }
        Command::Add { uid, component, value } => {
            let e = world.require_entity(*uid)?;
            let cid = world.require_component(component)?;
            let prev = world.get_json(e, cid);
            world.insert_json(e, cid, value)?;
            let inverse = match prev {
                Some(p) => Command::Set { uid: *uid, component: component.clone(), value: p },
                None => Command::Remove { uid: *uid, component: component.clone() },
            };
            Ok(Applied { executed: cmd.clone(), inverse: Some(vec![inverse]), uid: Some(*uid) })
        }
        Command::Set { uid, component, value } => {
            let e = world.require_entity(*uid)?;
            let cid = world.require_component(component)?;
            let prev = world.get_json(e, cid);
            world.set_json(e, cid, value)?;
            let inverse = Command::Set { uid: *uid, component: component.clone(), value: prev.expect("set_json succeeded so the component exists") };
            Ok(Applied { executed: cmd.clone(), inverse: Some(vec![inverse]), uid: Some(*uid) })
        }
        Command::Remove { uid, component } => {
            let e = world.require_entity(*uid)?;
            let cid = world.require_component(component)?;
            let prev = world.get_json(e, cid).ok_or_else(|| EcsError::new("missing_component", format!("entity {uid} has no {component} component")).at(component.clone()))?;
            world.remove(e, cid)?;
            Ok(Applied { executed: cmd.clone(), inverse: Some(vec![Command::Add { uid: *uid, component: component.clone(), value: prev }]), uid: Some(*uid) })
        }
        Command::DefineSystem { def } => {
            let sys = crate::graph::compile_with(world, def, scripts.plugin_root.as_deref(), scripts.wasm.as_ref())?;
            let name = sys.name.clone();
            let inverse = match scripts.put(sys) {
                Some(prev) => {
                    let mut d = prev.def.clone();
                    d.set("enabled", Json::Bool(prev.enabled));
                    Command::DefineSystem { def: d }
                }
                None => Command::RemoveSystem { name },
            };
            Ok(Applied { executed: cmd.clone(), inverse: Some(vec![inverse]), uid: None })
        }
        Command::RemoveSystem { name } => {
            let prev = scripts.remove(name).ok_or_else(|| no_system(scripts, name))?;
            let mut d = prev.def.clone();
            d.set("enabled", Json::Bool(prev.enabled));
            Ok(Applied { executed: cmd.clone(), inverse: Some(vec![Command::DefineSystem { def: d }]), uid: None })
        }
        Command::SetSystemEnabled { name, enabled } => {
            let present = scripts.get(name).map(|s| s.enabled).ok_or_else(|| no_system(scripts, name))?;
            scripts.systems.iter_mut().find(|s| s.name == *name).expect("checked").enabled = *enabled;
            Ok(Applied { executed: cmd.clone(), inverse: Some(vec![Command::SetSystemEnabled { name: name.clone(), enabled: present }]), uid: None })
        }
        Command::DefinePrefab { name, def } => {
            let mut next = scripts.prefabs.clone();
            let prev = next.insert(name.clone(), def.clone());
            crate::prefab::validate(world, &next, name)?;
            scripts.prefabs = next;
            let inverse = match prev {
                Some(p) => Command::DefinePrefab { name: name.clone(), def: p },
                None => Command::RemovePrefab { name: name.clone() },
            };
            Ok(Applied { executed: cmd.clone(), inverse: Some(vec![inverse]), uid: None })
        }
        Command::RemovePrefab { name } => {
            let prev = scripts.prefabs.remove(name).ok_or_else(|| {
                let all: Vec<&str> = scripts.prefabs.keys().map(String::as_str).collect();
                EcsError::new("no_such_prefab", format!("no prefab named `{name}`")).hint(format!("prefabs: {}", if all.is_empty() { "(none)".to_string() } else { all.join(", ") }))
            })?;
            Ok(Applied { executed: cmd.clone(), inverse: Some(vec![Command::DefinePrefab { name: name.clone(), def: prev }]), uid: None })
        }
        Command::SetResource { name, value } => {
            let prev = world.set_resource(name, *value)?;
            Ok(Applied { executed: cmd.clone(), inverse: Some(vec![Command::SetResource { name: name.clone(), value: prev }]), uid: None })
        }
        Command::Define { def } => {
            let (name, doc, specs) = parse_component_def(def)?;
            world.register_dynamic(&name, &doc, &specs)?;
            Ok(Applied { executed: cmd.clone(), inverse: None, uid: None })
        }
    }
}

/// Apply several commands as one step: either all succeed or the world is left as it was.
/// Returns the executed commands and one inverse group that undoes them all (in the right order).
pub fn apply_batch(world: &mut World, scripts: &mut ScriptSet, cmds: &[Command]) -> Result<(Vec<Applied>, Option<Vec<Command>>)> {
    let mut done: Vec<Applied> = Vec::new();
    for (i, c) in cmds.iter().enumerate() {
        match apply_one(world, scripts, c) {
            Ok(a) => done.push(a),
            Err(err) => {
                // roll back what already ran, newest first
                for a in done.iter().rev() {
                    if let Some(inv) = &a.inverse {
                        for ic in inv {
                            let _ = apply_one(world, scripts, ic);
                        }
                    }
                }
                crate::systems::hierarchy(world);
                return Err(EcsError { message: format!("command {i} failed: {}", err.message), ..err });
            }
        }
    }
    crate::systems::hierarchy(world);
    let inverse = if done.iter().all(|a| a.inverse.is_some()) { Some(done.iter().rev().flat_map(|a| a.inverse.clone().unwrap()).collect()) } else { None };
    Ok((done, inverse))
}

impl Command {
    pub fn to_json(&self) -> Json {
        match self {
            Command::Spawn { uid, components } => {
                let mut o = Json::obj([("op", Json::str("spawn"))]);
                if let Some(u) = uid {
                    o.set("uid", Json::from(*u as i64));
                }
                o.set("components", Json::Obj(components.clone()));
                o
            }
            Command::Despawn { uid } => Json::obj([("op", Json::str("despawn")), ("uid", Json::from(*uid as i64))]),
            Command::Add { uid, component, value } => Json::obj([("op", Json::str("add")), ("uid", Json::from(*uid as i64)), ("component", Json::str(component.clone())), ("value", value.clone())]),
            Command::Set { uid, component, value } => Json::obj([("op", Json::str("set")), ("uid", Json::from(*uid as i64)), ("component", Json::str(component.clone())), ("value", value.clone())]),
            Command::Remove { uid, component } => Json::obj([("op", Json::str("remove")), ("uid", Json::from(*uid as i64)), ("component", Json::str(component.clone()))]),
            Command::Define { def } => Json::obj([("op", Json::str("define")), ("def", def.clone())]),
            Command::DefineSystem { def } => Json::obj([("op", Json::str("define_system")), ("def", def.clone())]),
            Command::RemoveSystem { name } => Json::obj([("op", Json::str("remove_system")), ("name", Json::str(name.clone()))]),
            Command::SetSystemEnabled { name, enabled } => Json::obj([("op", Json::str("enable_system")), ("name", Json::str(name.clone())), ("enabled", Json::Bool(*enabled))]),
            Command::DefinePrefab { name, def } => Json::obj([("op", Json::str("define_prefab")), ("name", Json::str(name.clone())), ("def", def.clone())]),
            Command::RemovePrefab { name } => Json::obj([("op", Json::str("remove_prefab")), ("name", Json::str(name.clone()))]),
            Command::SetResource { name, value } => Json::obj([("op", Json::str("set_resource")), ("name", Json::str(name.clone())), ("value", value.map(Json::Float).unwrap_or(Json::Null))]),
        }
    }

    pub fn from_json(j: &Json) -> Result<Command> {
        let op = j.get("op").and_then(|v| v.as_str()).ok_or_else(|| EcsError::new("bad_command", "a command needs a string `op`").hint("ops: spawn, despawn, add, set, remove, define, define_system, remove_system, enable_system, define_prefab, remove_prefab, set_resource"))?;
        let uid = || j.get("uid").and_then(|v| v.as_u64()).ok_or_else(|| EcsError::new("bad_command", format!("`{op}` needs a positive integer `uid`")));
        let comp = || j.get("component").and_then(|v| v.as_str()).map(str::to_string).ok_or_else(|| EcsError::new("bad_command", format!("`{op}` needs a string `component`")));
        let value = || j.get("value").cloned().ok_or_else(|| EcsError::new("bad_command", format!("`{op}` needs a `value` object")));
        Ok(match op {
            "spawn" => {
                let components = match j.get("components") {
                    None => Vec::new(),
                    Some(Json::Obj(p)) => p.clone(),
                    Some(_) => return Err(EcsError::new("bad_command", "`components` must be an object keyed by component name")),
                };
                Command::Spawn { uid: j.get("uid").and_then(|v| v.as_u64()), components }
            }
            "despawn" => Command::Despawn { uid: uid()? },
            "add" => Command::Add { uid: uid()?, component: comp()?, value: value()? },
            "set" => Command::Set { uid: uid()?, component: comp()?, value: value()? },
            "remove" => Command::Remove { uid: uid()?, component: comp()? },
            "define" => Command::Define { def: j.get("def").cloned().ok_or_else(|| EcsError::new("bad_command", "`define` needs a `def` object"))? },
            "define_system" => Command::DefineSystem { def: j.get("def").cloned().ok_or_else(|| EcsError::new("bad_command", "`define_system` needs a `def` object"))? },
            "remove_system" => Command::RemoveSystem { name: j.get("name").and_then(|v| v.as_str()).map(str::to_string).ok_or_else(|| EcsError::new("bad_command", "`remove_system` needs a string `name`"))? },
            "enable_system" => Command::SetSystemEnabled { name: j.get("name").and_then(|v| v.as_str()).map(str::to_string).ok_or_else(|| EcsError::new("bad_command", "`enable_system` needs a string `name`"))?, enabled: j.get("enabled").and_then(|v| v.as_bool()).ok_or_else(|| EcsError::new("bad_command", "`enable_system` needs a boolean `enabled`"))? },
            "define_prefab" => Command::DefinePrefab {
                name: j.get("name").and_then(|v| v.as_str()).map(str::to_string).ok_or_else(|| EcsError::new("bad_command", "`define_prefab` needs a string `name`"))?,
                def: j.get("def").cloned().ok_or_else(|| EcsError::new("bad_command", "`define_prefab` needs a `def` object"))?,
            },
            "remove_prefab" => Command::RemovePrefab { name: j.get("name").and_then(|v| v.as_str()).map(str::to_string).ok_or_else(|| EcsError::new("bad_command", "`remove_prefab` needs a string `name`"))? },
            "set_resource" => Command::SetResource {
                name: j.get("name").and_then(|v| v.as_str()).map(str::to_string).ok_or_else(|| EcsError::new("bad_command", "`set_resource` needs a string `name`"))?,
                value: match j.get("value") {
                    Some(Json::Null) | None => None,
                    Some(v) => Some(v.as_f64().ok_or_else(|| EcsError::new("bad_command", "`set_resource` needs a number `value` (or null to remove it)"))?),
                },
            },
            other => return Err(EcsError::new("bad_command", format!("unknown op `{other}`")).hint("ops: spawn, despawn, add, set, remove, define, define_system, remove_system, enable_system, define_prefab, remove_prefab, set_resource")),
        })
    }
}
