//! `history`: what `undo` and `redo` would do, one line for each edit, so that an editor can show a list to jump back through and an agent can read what it has changed. Every edit is a group of commands on the undo stack
//! (as the commands that put things back); the label of an edit is made from them.
use crate::engine::Engine;
use lunaris_json::Json;
use lunaris_sim::Command;

/// The name of an edit from its group of commands. `inverse`: the group is on the undo stack, where it is the commands that undo the edit (a `despawn` there is an edit that created an entity); on the redo stack it is the edit itself.
pub(crate) fn edit_label(group: &[Command], inverse: bool) -> String {
    let Some(first) = group.first() else { return "(nothing)".to_string() };
    let name_of = |def: &Json| def.get("name").and_then(|n| n.as_str()).unwrap_or("?").to_string();
    let one = match first {
        Command::Despawn { uid } if inverse => format!("Create entity {uid}"),
        Command::Despawn { uid } => format!("Delete entity {uid}"),
        Command::Spawn { uid, .. } => match (inverse, uid) {
            (true, Some(uid)) => format!("Delete entity {uid}"),
            (true, None) => "Delete entity".to_string(),
            (false, Some(uid)) => format!("Create entity {uid}"),
            (false, None) => "Create entity".to_string(),
        },
        Command::Set { uid, component, .. } => format!("Edit {component} of {uid}"),
        Command::Add { uid, component, .. } if inverse => format!("Remove {component} from {uid}"),
        Command::Add { uid, component, .. } => format!("Add {component} to {uid}"),
        Command::Remove { uid, component } if inverse => format!("Add {component} to {uid}"),
        Command::Remove { uid, component } => format!("Remove {component} from {uid}"),
        Command::Define { def } => format!("Define component {}", name_of(def)),
        Command::DefineSystem { def } => format!("Edit system {}", name_of(def)),
        Command::RemoveSystem { name } if inverse => format!("Add system {name}"),
        Command::RemoveSystem { name } => format!("Remove system {name}"),
        Command::SetSystemEnabled { name, .. } => format!("Switch system {name}"),
        Command::DefinePrefab { name, .. } => format!("Edit prefab {name}"),
        Command::RemovePrefab { name } if inverse => format!("Add prefab {name}"),
        Command::RemovePrefab { name } => format!("Remove prefab {name}"),
        Command::SetResource { name, .. } => format!("Set resource {name}"),
    };
    if group.len() > 1 {
        format!("{one} (and {} more)", group.len() - 1)
    } else {
        one
    }
}

impl Engine {
    pub(crate) fn history_tool(&self) -> Json {
        let (undo, redo) = self.undo_redo_stacks();
        // newest first: the first row of `undo` is what `undo` does next, the first of `redo` what `redo` does next
        let rows = |stack: &[Vec<Command>], inverse: bool| Json::arr(stack.iter().rev().map(|g| Json::obj([("label", Json::str(edit_label(g, inverse))), ("commands", Json::from(g.len()))])));
        Json::obj([("undo", rows(undo, true)), ("redo", rows(redo, false)), ("undo_depth", Json::from(undo.len())), ("redo_depth", Json::from(redo.len()))])
    }
}
