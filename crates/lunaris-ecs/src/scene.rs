//! Text scenes: the whole world as a JSON document that people and AI can read, diff and edit with ordinary tools.
//!
//! ```json
//! { "lunaris": 1,
//!   "components": [ { "name": "Health", "fields": [ { "name": "hp", "type": "f32", "default": 100 } ] } ],
//!   "next_uid": 3,
//!   "entities": [ { "uid": 1, "components": { "Health": { "hp": 80 } } } ] }
//! ```
use crate::error::{EcsError, Result};
use crate::schema::*;
use crate::world::World;
use lunaris_json::Json;

pub const SCENE_VERSION: i64 = 1;

/// Parse `{ "name", "doc"?, "fields": [ { "name", "type", "count"?, "default"? } ] }`.
pub fn parse_component_def(def: &Json) -> Result<(String, String, Vec<FieldSpec>)> {
    let name = def.get("name").and_then(|v| v.as_str()).ok_or_else(|| EcsError::new("bad_definition", "a component definition needs a string `name`"))?.to_string();
    let doc = def.get("doc").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let fields = def.get("fields").and_then(|v| v.as_array()).ok_or_else(|| EcsError::new("bad_definition", "a component definition needs a `fields` array").at(name.clone()))?;
    let mut specs = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        let path = format!("{name}.fields[{i}]");
        let fname = f.get("name").and_then(|v| v.as_str()).ok_or_else(|| EcsError::new("bad_definition", "each field needs a string `name`").at(path.clone()))?;
        let tname = f.get("type").and_then(|v| v.as_str()).ok_or_else(|| EcsError::new("bad_definition", "each field needs a string `type`").at(path.clone()))?;
        let ty = FieldType::parse(tname).ok_or_else(|| EcsError::new("bad_type", format!("unknown field type `{tname}`")).hint(format!("valid types: {}", FieldType::ALL.join(", "))).at(path.clone()))?;
        let count = match f.get("count") {
            None => 1,
            Some(c) => c.as_u64().filter(|&n| (1..=4096).contains(&n)).ok_or_else(|| EcsError::new("bad_count", "`count` must be an integer from 1 to 4096").at(path.clone()))? as u32,
        };
        specs.push(FieldSpec { name: fname.to_string(), ty, count, default: f.get("default").cloned() });
    }
    Ok((name, doc, specs))
}

fn definition_json(world: &World, info: &ComponentInfo) -> Json {
    let mut defaults = Vec::new();
    if info.size > 0 {
        let mut buf = vec![0u64; info.size.div_ceil(8)];
        // SAFETY: buf is 8-aligned with at least `size` bytes; default_bytes has exactly `size` bytes.
        let obj = unsafe {
            std::ptr::copy_nonoverlapping(info.default_bytes.as_ptr(), buf.as_mut_ptr().cast::<u8>(), info.size);
            info.read_json(buf.as_ptr().cast::<u8>(), world.strings())
        };
        if let Json::Obj(p) = obj {
            defaults = p;
        }
    }
    let fields = info.fields.iter().map(|f| {
        let mut o = Json::obj([("name", Json::str(f.name.clone())), ("type", Json::str(f.ty.name()))]);
        if f.count > 1 {
            o.set("count", Json::from(f.count as i64));
        }
        if let Some((_, d)) = defaults.iter().find(|(k, _)| *k == f.name) {
            o.set("default", d.clone());
        }
        o
    });
    Json::obj([("name", Json::str(info.name.clone())), ("doc", Json::str(info.doc.clone())), ("fields", Json::arr(fields))])
}

pub fn to_scene_json(world: &World) -> Json {
    let defs: Vec<Json> = world.infos().iter().filter(|i| !i.native).map(|i| definition_json(world, i)).collect();
    let mut entities = Vec::with_capacity(world.len());
    for e in world.entities_by_uid() {
        let mut comps = Vec::new();
        for cid in world.components_of(e) {
            comps.push((world.info(cid).name.clone(), world.get_json(e, cid).expect("component listed on the entity")));
        }
        entities.push(Json::obj([("uid", Json::from(world.uid_of(e).unwrap() as i64)), ("components", Json::Obj(comps))]));
    }
    let mut scene = Json::obj([
        ("lunaris", Json::from(SCENE_VERSION)),
        ("components", Json::Arr(defs)),
        ("next_uid", Json::from(world.next_uid() as i64)),
        ("entities", Json::Arr(entities)),
    ]);
    // globals such as the score are part of a saved game; live input state (`input.*`) is not
    let saved: Vec<(String, Json)> = world.resources().iter().filter(|(k, _)| !k.starts_with("input.")).map(|(k, v)| (k.clone(), Json::Float(*v))).collect();
    if !saved.is_empty() {
        scene.set("resources", Json::Obj(saved));
    }
    scene
}

/// Load a scene into `world` (which should normally be empty). Native components must already be registered.
/// Returns the uids of the entities created. On error nothing partial is guaranteed, so load into a scratch world first
/// when atomicity matters (the API layer does).
pub fn load_scene_json(world: &mut World, scene: &Json) -> Result<Vec<u64>> {
    let version = scene.get("lunaris").and_then(|v| v.as_i64()).ok_or_else(|| EcsError::new("bad_scene", "not a Lunaris scene: missing integer `lunaris` version").hint("a scene looks like {\"lunaris\":1,\"entities\":[...]}"))?;
    if version != SCENE_VERSION {
        return Err(EcsError::new("bad_scene_version", format!("scene version {version} is not supported (this build reads version {SCENE_VERSION})")));
    }
    if let Some(defs) = scene.get("components").and_then(|v| v.as_array()) {
        for d in defs {
            let (name, doc, specs) = parse_component_def(d)?;
            world.register_dynamic(&name, &doc, &specs)?;
        }
    }
    let entities = scene.get("entities").and_then(|v| v.as_array()).ok_or_else(|| EcsError::new("bad_scene", "scene needs an `entities` array"))?;
    let mut created = Vec::with_capacity(entities.len());
    let mut items: Vec<(ComponentId, &Json)> = Vec::new();
    for (i, ent) in entities.iter().enumerate() {
        let at = || format!("entities[{i}]");
        let uid = ent.get("uid").and_then(|v| v.as_u64()).ok_or_else(|| EcsError::new("bad_scene", "each entity needs a positive integer `uid`").at(at()))?;
        items.clear();
        match ent.get("components") {
            Some(Json::Obj(comps)) => {
                for (cname, value) in comps {
                    let cid = world.require_component(cname).map_err(|err| err.at(format!("{}.components.{cname}", at())))?;
                    items.push((cid, value));
                }
            }
            Some(_) => return Err(EcsError::new("bad_scene", "`components` must be an object keyed by component name").at(at())),
            None => {}
        }
        // the entity is created directly in its final archetype with every component at once (no hops through smaller archetypes)
        world.spawn_json(Some(uid), &items).map_err(|err| match err.path.clone() {
            Some(p) => err.at(format!("{}.components.{p}", at())),
            None => err.at(at()),
        })?;
        created.push(uid);
    }
    if let Some(n) = scene.get("next_uid").and_then(|v| v.as_u64()) {
        world.set_next_uid(n);
    }
    match scene.get("resources") {
        None => {}
        Some(Json::Obj(res)) => {
            for (name, v) in res {
                let x = v.as_f64().ok_or_else(|| EcsError::new("bad_scene", format!("resource `{name}` must be a number")).at(format!("resources.{name}")))?;
                world.set_resource(name, Some(x)).map_err(|e| e.at(format!("resources.{name}")))?;
            }
        }
        Some(_) => return Err(EcsError::new("bad_scene", "`resources` must be an object of numbers").at("resources")),
    }
    Ok(created)
}
