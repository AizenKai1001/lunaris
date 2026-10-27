//! The Hierarchy's rows, read straight from the world. The editor and the engine are one process, so asking the `query` tool for every entity with fourteen components and reading its JSON back
//! cost 35 ms for 10,000 entities, four times a second while the game plays; typed reads of the same things are about a twentieth of that. The rows are the same ones, in the same order (by uid, the first
//! `limit` of them), which a test checks against the `query` way.
use crate::state::{Kind, Row};
use lunaris_ecs::{ComponentId, World};
use lunaris_sim::components::{MeshRenderer, Name, Parent, Transform};
use std::collections::HashMap;

/// The kind of thing an archetype holds, for the little icon (the first of Camera, Light, Environment, MeshRenderer, Sprite, Tilemap, Ui, Text, Emitter, AudioSource, Collider), and whether it has a `MeshRenderer`.
fn kind_of(world: &World, ids: &KindIds, archetype: usize) -> (Kind, bool) {
    let has: Vec<ComponentId> = world.archetypes()[archetype].component_ids().collect();
    let kind = ids.order.iter().find(|(_, id)| id.is_some_and(|id| has.contains(&id))).map_or(Kind::Empty, |(k, _)| *k);
    (kind, ids.mesh.is_some_and(|id| has.contains(&id)))
}

/// The ids of the components the kinds look for, in the order they are tried.
struct KindIds {
    order: Vec<(Kind, Option<ComponentId>)>,
    mesh: Option<ComponentId>,
}

impl KindIds {
    fn of(world: &World) -> KindIds {
        let id = |n: &str| world.component_id(n);
        KindIds {
            order: vec![
                (Kind::Camera, id("Camera")),
                (Kind::Light, id("Light")),
                (Kind::Environment, id("Environment")),
                (Kind::Mesh, id("MeshRenderer")),
                (Kind::Sprite, id("Sprite")),
                (Kind::Tilemap, id("Tilemap")),
                (Kind::Ui, id("Ui")),
                (Kind::Text, id("Text")),
                (Kind::Emitter, id("Emitter")),
                (Kind::Audio, id("AudioSource")),
                (Kind::Collider, id("Collider")),
            ],
            mesh: id("MeshRenderer"),
        }
    }
}

/// How many entities there are, and the rows of the first `limit` of them by uid, with the meshes counted.
pub fn read(world: &World, limit: usize) -> (usize, Vec<Row>) {
    let order = world.entities_by_uid();
    let total = order.len();
    let ids = KindIds::of(world);
    let mut kinds: HashMap<usize, (Kind, bool)> = HashMap::new();
    let mut rows: Vec<Row> = Vec::with_capacity(total.min(limit));
    let mut mesh_rows: Vec<(usize, bool)> = Vec::new();
    for &e in order.iter().take(limit) {
        let (Some(uid), Some((archetype, _))) = (world.uid_of(e), world.locate(e)) else { continue };
        let (kind, own_mesh) = *kinds.entry(archetype).or_insert_with(|| kind_of(world, &ids, archetype));
        let name = world.get::<Name>(e).map(|n| world.strings().get(n.value.0)).filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| format!("Entity {uid}"));
        let parent = world.get::<Parent>(e).map(|p| p.target.0).filter(|p| *p != 0);
        let pos = world.get::<Transform>(e).map(|t| t.pos);
        if own_mesh {
            mesh_rows.push((rows.len(), world.get::<MeshRenderer>(e).is_none_or(|m| m.visible)));
        }
        rows.push(Row { uid, name, parent, depth: 0, children: 0, kind, pos, own_mesh, meshes: 0, shown: 0 });
    }
    count_meshes(&mut rows, &mesh_rows);
    (total, rows)
}

/// Each row counts the meshes in it and under it, and how many of them are shown (the Outliner's eye); `mesh_rows` is the rows that have a mesh of their own and whether it is visible.
pub fn count_meshes(rows: &mut [Row], mesh_rows: &[(usize, bool)]) {
    let by_uid: HashMap<u64, usize> = rows.iter().enumerate().map(|(i, r)| (r.uid, i)).collect();
    for &(i, visible) in mesh_rows {
        let (mut at, mut hops) = (Some(i), 0);
        while let Some(k) = at {
            rows[k].meshes += 1;
            rows[k].shown += visible as u32;
            hops += 1;
            let up = rows[k].parent.and_then(|p| by_uid.get(&p).copied());
            at = if hops > 256 { None } else { up.filter(|p| *p != k) };
        }
    }
}
