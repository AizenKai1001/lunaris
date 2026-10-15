//! The engine facade: one object that owns a simulation and exposes every capability as a named tool taking and
//! returning JSON. Editors, AI agents (over MCP), the CLI and tests all call exactly this.
use lunaris_ecs::{parse_component_def, ComponentId, EcsError, Entity, Result};
use lunaris_json::{parse, Json};
use lunaris_render::TextureCache;
use lunaris_sim::{Command, Recording, Sim};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_SNAPSHOTS: usize = 32;
const MAX_QUERY_LIMIT: u64 = 10_000;
const MAX_STEP_TICKS: u64 = 10_000_000;
const DEFAULT_STEP_BUDGET_MS: u64 = 20_000;
const MAX_STEP_BUDGET_MS: u64 = 600_000;
pub(crate) const MAX_SPAWN_COUNT: u64 = 1_000_000;

/// Tools that never change the world (they may write a file or draw a picture). Every other tool advances the revision counter.
const READ_ONLY_TOOLS: &[&str] = &[
    "describe", "schema", "get", "query", "hash", "revision", "snapshots", "events", "resources", "systems", "get_system", "contacts", "raycast", "nearby", "render", "pick", "minimap", "check", "guide", "project",
    "save_scene", "export_game", "verify", "files", "saves", "model_info", "prefabs", "path", "acoustics", "compile_script", "get_script", "terrain_height", "terrain_layer", "water_depth",
];

fn script_diag_json(d: &lunaris_script::Diagnostic) -> Json {
    let mut o = Json::obj([("line", Json::from(d.line)), ("col", Json::from(d.col)), ("end_line", Json::from(d.end_line)), ("end_col", Json::from(d.end_col)), ("message", Json::str(d.message.clone()))]);
    if let Some(h) = &d.hint {
        o.set("hint", Json::str(h.clone()));
    }
    o
}

pub struct Engine {
    pub sim: Sim,
    undo: Vec<Vec<Command>>,
    redo: Vec<Vec<Command>>,
    /// `undo_group` of the call being handled, and of the last edit that went on the undo stack: consecutive edits with the same
    /// group (a drag in the editor) are one undo step.
    current_group: Option<String>,
    last_group: Option<String>,
    snapshots: BTreeMap<String, Sim>,
    last_recording: Option<Recording>,
    /// Goes up with every tool call that is not read-only (see `READ_ONLY_TOOLS`); not part of the simulation state.
    revision: u64,
    pub(crate) root: PathBuf,
    pub(crate) textures: TextureCache,
    pub(crate) gpu: crate::gpu::GpuState,
    /// The quality `render` uses when the call names none: `reference` (the CPU renderer) unless a GPU renderer was offered.
    pub(crate) default_quality: String,
}

pub(crate) struct Args<'a>(pub(crate) &'a Json);

impl<'a> Args<'a> {
    pub(crate) fn missing(&self, key: &str, what: &str) -> EcsError {
        EcsError::new("missing_param", format!("missing required parameter `{key}` ({what})")).hint("call tools/list (or the `schema` tool) to see each tool's parameters")
    }
    pub(crate) fn str(&self, key: &str) -> Result<&'a str> {
        match self.0.get(key) {
            Some(Json::Str(s)) => Ok(s),
            Some(other) => Err(EcsError::new("type_mismatch", format!("`{key}` must be a string, got {}", other.type_name())).at(key)),
            None => Err(self.missing(key, "string")),
        }
    }
    pub(crate) fn opt_str(&self, key: &str) -> Result<Option<&'a str>> {
        match self.0.get(key) {
            None | Some(Json::Null) => Ok(None),
            Some(Json::Str(s)) => Ok(Some(s)),
            Some(other) => Err(EcsError::new("type_mismatch", format!("`{key}` must be a string, got {}", other.type_name())).at(key)),
        }
    }
    pub(crate) fn u64(&self, key: &str) -> Result<u64> {
        match self.0.get(key) {
            Some(v) => v.as_u64().ok_or_else(|| EcsError::new("type_mismatch", format!("`{key}` must be a non-negative integer")).at(key)),
            None => Err(self.missing(key, "integer")),
        }
    }
    pub(crate) fn opt_u64(&self, key: &str) -> Result<Option<u64>> {
        match self.0.get(key) {
            None | Some(Json::Null) => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or_else(|| EcsError::new("type_mismatch", format!("`{key}` must be a non-negative integer")).at(key)),
        }
    }
    pub(crate) fn names(&self, key: &str) -> Result<Option<Vec<&'a str>>> {
        match self.0.get(key) {
            None | Some(Json::Null) => Ok(None),
            Some(Json::Arr(a)) => a.iter().enumerate().map(|(i, v)| v.as_str().ok_or_else(|| EcsError::new("type_mismatch", format!("`{key}[{i}]` must be a string")).at(key))).collect::<Result<Vec<_>>>().map(Some),
            Some(_) => Err(EcsError::new("type_mismatch", format!("`{key}` must be an array of component names")).at(key)),
        }
    }
    pub(crate) fn value(&self, key: &str) -> Result<&'a Json> {
        self.0.get(key).ok_or_else(|| self.missing(key, "object"))
    }
}

pub(crate) fn hex(h: u64) -> Json {
    Json::str(format!("{h:016x}"))
}

pub(crate) fn uid_json(u: u64) -> Json {
    Json::Int(u as i64)
}

impl Engine {
    pub fn new(seed: u64, root: impl Into<PathBuf>) -> Engine {
        let root = root.into();
        let mut engine = Engine { sim: Sim::new(seed), undo: Vec::new(), redo: Vec::new(), current_group: None, last_group: None, snapshots: BTreeMap::new(), last_recording: None, revision: 0, textures: TextureCache::new(Some(root.clone())), root, gpu: crate::gpu::GpuState::Off, default_quality: "reference".to_string() };
        // heightmaps of `Terrain` entities are read from the workspace
        engine.sim.asset_root = Some(engine.root.clone());
        // sandboxed WebAssembly systems need no permission: a module can touch nothing but the batch of fields it is given
        engine.sim.scripts.wasm = Some(lunaris_sim::plugin::WasmHost { root: engine.root.clone(), loader: std::sync::Arc::new(lunaris_wasm::Loader) });
        // native plugins are the host's decision, never a client's: `lunaris --allow-plugins` sets this variable for the whole process
        if std::env::var_os("LUNARIS_ALLOW_PLUGINS").is_some_and(|v| v == "1") {
            engine.allow_plugins(true);
        }
        engine
    }

    /// Let graph systems load native plugins (`{"plugin": "plugins/x.dll", ...}`) from the workspace folder, or stop allowing it.
    /// Plugins are native code: only turn this on for libraries you trust. No tool can change it.
    pub fn allow_plugins(&mut self, on: bool) {
        self.sim.scripts.plugin_root = on.then(|| self.root.clone());
    }

    pub(crate) fn resolve(&self, rel: &str) -> Result<PathBuf> {
        // the same answer on every operating system (`C:/x` is a relative path to Linux, a drive to Windows)
        let norm = lunaris_json::safe_relative(rel).map_err(|why| {
            if rel.is_empty() {
                EcsError::new("bad_path", "path is empty")
            } else {
                EcsError::new("bad_path", format!("`{rel}` leaves the workspace ({why})")).hint(format!("use a relative path without `..` or a drive letter; the workspace root is {}", self.root.display())).at("path")
            }
        })?;
        Ok(self.root.join(norm))
    }

    pub(crate) fn read_text(&self, rel: &str) -> Result<String> {
        let path = self.resolve(rel)?;
        std::fs::read_to_string(&path).map_err(|e| EcsError::new("io_error", format!("cannot read {}: {e}", path.display())).at("path"))
    }

    pub(crate) fn write_bytes(&self, rel: &str, bytes: &[u8]) -> Result<PathBuf> {
        let path = self.resolve(rel)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| EcsError::new("io_error", format!("cannot create {}: {e}", dir.display())))?;
        }
        std::fs::write(&path, bytes).map_err(|e| EcsError::new("io_error", format!("cannot write {}: {e}", path.display())).at("path"))?;
        Ok(path)
    }

    pub(crate) fn write_text(&self, rel: &str, text: &str) -> Result<PathBuf> {
        let path = self.resolve(rel)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| EcsError::new("io_error", format!("cannot create {}: {e}", dir.display())))?;
        }
        std::fs::write(&path, text).map_err(|e| EcsError::new("io_error", format!("cannot write {}: {e}", path.display())).at("path"))?;
        Ok(path)
    }

    // ------------------------------------------------------------------------------------------ dispatch
    /// A counter that goes up whenever something may have changed the world. A client that sees a different number than last time
    /// knows another client (a person in the editor, another agent) did something and can query again.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Run one tool. Unknown tools and bad arguments return an `EcsError` with a hint. Every tool that is not read-only advances
    /// the revision counter, also when it fails part way.
    pub fn call(&mut self, tool: &str, args: &Json) -> Result<Json> {
        let out = self.dispatch(tool, args).map_err(|e| self.explain_duplicate_uid(e));
        if !READ_ONLY_TOOLS.contains(&tool) {
            self.revision += 1;
        }
        out
    }

    /// A `duplicate_uid` error that says who holds the number: the entity's name and, for the boxes of `tilemap_colliders` (they take the numbers after the last entity, so entities
    /// that a builder spawns afterwards with numbers of its own can clash with them), what to do about it.
    fn explain_duplicate_uid(&self, mut e: EcsError) -> EcsError {
        if e.code != "duplicate_uid" {
            return e;
        }
        let Some(uid) = e.message.split_whitespace().find_map(|w| w.parse::<u64>().ok()) else { return e };
        let w = &self.sim.world;
        let Some(holder) = w.entity(uid) else { return e };
        let name_of = |ent| w.get::<lunaris_sim::components::Name>(ent).map(|n| w.strings().get(n.value.0).to_string()).unwrap_or_default();
        let name = name_of(holder);
        if let Some(map) = name.strip_prefix("tile collider ") {
            let last = w.entities_by_uid().into_iter().filter(|ent| name_of(*ent).starts_with("tile collider ")).filter_map(|ent| w.uid_of(ent)).max().unwrap_or(uid);
            e.message = format!("{} (it is a box named `{name}`, made by the `tilemap_colliders` tool)", e.message);
            e.hint.get_or_insert_with(|| format!("`tilemap_colliders` gave the boxes of the Tilemap {map} the numbers after the last entity (up to {last}), and an entity made by number clashes with them: use numbers above {last}, or make the boxes with `first_uid` set above everything you will spawn (or after your entities)"));
        } else if !name.is_empty() {
            e.message = format!("{} (it is `{name}`)", e.message);
            e.hint.get_or_insert_with(|| "choose another uid, or leave `uid` out and the engine picks a free one".to_string());
        } else {
            e.hint.get_or_insert_with(|| "choose another uid, or leave `uid` out and the engine picks a free one".to_string());
        }
        e
    }

    fn dispatch(&mut self, tool: &str, args: &Json) -> Result<Json> {
        let empty = Json::Obj(Vec::new());
        let args = if args.is_null() { &empty } else { args };
        if !matches!(args, Json::Obj(_)) {
            return Err(EcsError::new("bad_arguments", "tool arguments must be a JSON object"));
        }
        let a = Args(args);
        self.current_group = args.get("undo_group").and_then(|g| g.as_str()).map(str::to_string);
        match tool {
            "describe" => Ok(self.describe()),
            "schema" => self.schema(&a),
            "define_component" => self.define_component(args),
            "spawn" => self.spawn(&a),
            "despawn" => self.single(Command::Despawn { uid: a.u64("uid")? }),
            "get" => self.get(&a),
            "set" => self.single(Command::Set { uid: a.u64("uid")?, component: a.str("component")?.to_string(), value: a.value("value")?.clone() }),
            "add_component" => {
                let value = a.0.get("value").cloned().unwrap_or(Json::Obj(Vec::new()));
                self.single(Command::Add { uid: a.u64("uid")?, component: a.str("component")?.to_string(), value })
            }
            "remove_component" => self.single(Command::Remove { uid: a.u64("uid")?, component: a.str("component")?.to_string() }),
            "query" => self.query(&a),
            "batch" => self.batch(&a),
            "step" => self.step(&a),
            "hash" => Ok(Json::obj([("tick", Json::from(self.sim.tick as i64)), ("hash", hex(self.sim.state_hash()))])),
            "revision" => Ok(Json::obj([("revision", Json::from(self.revision as i64)), ("tick", Json::from(self.sim.tick as i64)), ("entities", Json::from(self.sim.world.len()))])),
            "snapshot" => self.snapshot(&a),
            "restore" => self.restore(&a),
            "snapshots" => Ok(Json::obj([("snapshots", Json::arr(self.snapshots.iter().map(|(n, s)| Json::obj([("name", Json::str(n.clone())), ("tick", Json::from(s.tick as i64)), ("entities", Json::from(s.world.len())), ("hash", hex(s.state_hash()))]))))])),
            "delete_snapshot" => {
                let name = a.str("name")?;
                self.snapshots.remove(name).map(|_| Json::obj([("deleted", Json::str(name))])).ok_or_else(|| self.no_snapshot(name))
            }
            "undo" => self.undo_redo(true),
            "redo" => self.undo_redo(false),
            "record_start" => {
                self.sim.start_recording()?;
                Ok(Json::obj([("recording", Json::Bool(true)), ("start_tick", Json::from(self.sim.tick as i64))]))
            }
            "record_stop" => self.record_stop(&a),
            "replay" => self.replay(&a),
            "load_scene" => self.load_scene(&a),
            "save_scene" => self.save_scene(&a),
            "files" => self.files_tool(&a),
            "model_info" => self.model_info_tool(&a),
            "define_system" => self.single(Command::DefineSystem { def: args.clone() }),
            "terrain_height" => self.terrain_height_tool(&a),
            "terrain_layer" => self.terrain_layer_tool(&a),
            "water_depth" => self.water_depth_tool(&a),
            "scatter" => self.scatter_tool(&a),
            "type_text" => self.type_text_tool(&a),
            "save_game" => self.save_game_tool(&a),
            "export_game" => self.export_tool(&a),
            "profile" => self.profile_tool(&a),
            "autotile" => self.autotile_tool(&a),
            "generate_map" => self.generate_map_tool(&a),
            "tilemap_colliders" => self.tilemap_colliders_tool(&a),
            "import_tiled" => self.import_tiled_tool(&a),
            "load_game" => self.load_game_tool(&a),
            "saves" => self.saves_tool(&a),
            "reload_terrain" => {
                // read every heightmap again (after the files changed): the simulation's copies and the renderer's
                self.sim.heightfields = Default::default();
                self.sim.terrain_errors.clear();
                self.textures.forget_heightmaps();
                self.sim.load_terrains();
                Ok(Json::obj([("loaded", Json::arr(self.sim.heightfields.keys().map(|k| Json::str(k.clone())))), ("errors", Json::Obj(self.sim.terrain_errors.iter().map(|(k, v)| (k.clone(), Json::str(v.clone()))).collect()))]))
            }
            "compile_script" => self.compile_script_tool(&a),
            "define_script" => self.define_script_tool(&a),
            "get_script" => self.get_script_tool(&a),
            "remove_system" => self.single(Command::RemoveSystem { name: a.str("name")?.to_string() }),
            "define_prefab" => {
                let name = a.str("name")?.to_string();
                let def = Json::Obj(args.as_object().unwrap_or(&[]).iter().filter(|(k, _)| k != "name" && k != "undo_group").cloned().collect());
                self.single(Command::DefinePrefab { name, def })
            }
            "remove_prefab" => self.single(Command::RemovePrefab { name: a.str("name")?.to_string() }),
            "prefabs" => self.prefabs_tool(&a),
            "path" => self.path_tool(&a),
            "spawn_prefab" => self.spawn_prefab_tool(&a),
            "enable_system" => {
                let enabled = match a.0.get("enabled") {
                    Some(Json::Bool(b)) => *b,
                    _ => return Err(EcsError::new("missing_param", "missing required parameter `enabled` (boolean)")),
                };
                self.single(Command::SetSystemEnabled { name: a.str("name")?.to_string(), enabled })
            }
            "set_resource" => {
                let name = a.str("name")?.to_string();
                let value = match a.0.get("value") {
                    None | Some(Json::Null) => None,
                    Some(v) => Some(resource_number(v, "value")?),
                };
                self.single(Command::SetResource { name, value })
            }
            "events" => {
                let since = a.opt_u64("since_tick")?.unwrap_or(0);
                let limit = a.opt_u64("limit")?.unwrap_or(200).min(2000) as usize;
                let list: Vec<&lunaris_sim::Event> = self.sim.events.iter().filter(|e| e.tick >= since).collect();
                let total = list.len();
                // the newest `limit`, oldest first
                let shown = list[total.saturating_sub(limit)..].to_vec();
                Ok(Json::obj([("total", Json::from(total)), ("tick", Json::from(self.sim.tick as i64)), ("events", Json::arr(shown.iter().map(|e| event_json(e))))]))
            }
            "resources" => Ok(Json::obj([("resources", Json::Obj(self.sim.world.resources().iter().map(|(k, v)| (k.clone(), Json::Float(*v))).collect()))])),
            "input" => self.input(&a),
            "systems" => Ok(self.systems_json()),
            "get_system" => {
                let name = a.str("name")?;
                let sys = self.sim.scripts.get(name).ok_or_else(|| {
                    let all: Vec<&str> = self.sim.scripts.systems.iter().map(|s| s.name.as_str()).collect();
                    EcsError::new("no_such_system", format!("no graph system named `{name}`")).hint(format!("graph systems: {}", if all.is_empty() { "(none)".to_string() } else { all.join(", ") }))
                })?;
                let mut o = lunaris_sim::graph::describe(sys);
                o.set("def", sys.def.clone());
                Ok(o)
            }
            "contacts" => self.contacts_tool(&a),
            "raycast" => self.raycast_tool(&a),
            "acoustics" => self.acoustics_tool(&a),
            "record_audio" => self.record_audio_tool(&a),
            "nearby" => self.nearby_tool(&a),
            "render" => self.render_tool(&a),
            "pick" => self.pick_tool(&a),
            "minimap" => self.minimap_tool(&a),
            "check" => self.check_tool(&a),
            "verify" => self.verify_tool(&a),
            "guide" => guide_tool(&a),
            "project" => self.project_tool(&a),
            "reset" => {
                let seed = a.opt_u64("seed")?.unwrap_or(self.sim.seed);
                let mut fresh = Sim::new(seed);
                fresh.asset_root = self.sim.asset_root.clone();
                fresh.scripts.plugin_root = self.sim.scripts.plugin_root.clone();
                fresh.scripts.wasm = self.sim.scripts.wasm.clone();
                fresh.dt = self.sim.dt;
                fresh.threads = match a.opt_u64("threads")? {
                    Some(t) if (1..=64).contains(&t) => t as usize,
                    Some(_) => return Err(EcsError::new("out_of_range", "`threads` must be between 1 and 64").at("threads")),
                    None => self.sim.threads,
                };
                self.sim = fresh;
                self.undo.clear();
                self.redo.clear();
                Ok(Json::obj([("seed", hex(seed)), ("threads", Json::from(self.sim.threads)), ("tick", Json::from(0)), ("entities", Json::from(0))]))
            }
            other => Err(EcsError::new("unknown_tool", format!("no tool named `{other}`")).hint(format!("tools: {}", crate::tools::tool_names().join(", ")))),
        }
    }

    // ------------------------------------------------------------------------------------------ introspection
    fn systems_json(&self) -> Json {
        Json::obj([
            ("native", Json::arr(self.sim.system_names().into_iter().map(|n| Json::obj([("name", Json::str(n)), ("kind", Json::str("native"))])))),
            ("graph", Json::arr(self.sim.scripts.systems.iter().map(lunaris_sim::graph::describe))),
            ("order", Json::str("native systems run first, then graph systems by (order, definition)")),
        ])
    }

    pub fn describe(&self) -> Json {
        let w = &self.sim.world;
        Json::obj([
            ("engine", Json::str("lunaris")),
            ("version", Json::str(VERSION)),
            ("tick", Json::from(self.sim.tick as i64)),
            ("dt", Json::from(self.sim.dt as f64)),
            ("seed", hex(self.sim.seed)),
            ("threads", Json::from(self.sim.threads)),
            ("entities", Json::from(w.len())),
            ("archetypes", Json::from(w.archetype_count())),
            ("components", Json::arr(w.infos().iter().map(|i| Json::str(i.name.clone())))),
            ("systems", Json::arr(self.sim.system_names().into_iter().map(Json::str).chain(self.sim.scripts.systems.iter().map(|s| Json::str(s.name.clone()))))),
            ("prefabs", Json::arr(self.sim.scripts.prefabs.keys().map(|k| Json::str(k.clone())))),
            ("snapshots", Json::arr(self.snapshots.keys().map(|k| Json::str(k.clone())))),
            ("recording", Json::Bool(self.sim.is_recording())),
            ("undo_depth", Json::from(self.undo.len())),
            ("redo_depth", Json::from(self.redo.len())),
            ("revision", Json::from(self.revision as i64)),
            ("workspace", Json::str(self.root.display().to_string())),
            ("hash", hex(self.sim.state_hash())),
        ])
    }

    fn schema(&self, a: &Args) -> Result<Json> {
        let w = &self.sim.world;
        if let Some(name) = a.opt_str("component")? {
            let id = w.require_component(name)?;
            return Ok(w.info(id).schema_json());
        }
        Ok(Json::obj([
            ("components", Json::arr(w.infos().iter().map(|i| i.schema_json()))),
            ("field_types", Json::arr(lunaris_ecs::FieldType::ALL.iter().map(|s| Json::str(*s)))),
            ("notes", Json::str("Entities are addressed by uid. A field with `count` n is a fixed array of n values. `str` fields hold text, `uid` fields reference another entity (0 = none).")),
        ]))
    }

    fn define_component(&mut self, args: &Json) -> Result<Json> {
        // validate the definition first so the error points at the problem
        parse_component_def(args)?;
        self.single(Command::Define { def: args.clone() })
    }

    // ------------------------------------------------------------------------------------------ mutation
    /// Forget undo and redo (after the whole world was replaced).
    pub(crate) fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub(crate) fn push_history(&mut self, inverse: Option<Vec<Command>>) -> bool {
        match inverse {
            Some(_) if self.current_group.is_some() && self.current_group == self.last_group && !self.undo.is_empty() => {
                // the same drag again: the first step's inverse already restores the state before the whole drag
                self.redo.clear();
                true
            }
            Some(group) => {
                self.last_group = self.current_group.clone();
                self.undo.push(group);
                self.redo.clear();
                if self.undo.len() > 1000 {
                    self.undo.remove(0);
                }
                true
            }
            None => false,
        }
    }

    /// Set controller state: each `values` entry becomes the resource `input.<name>` (true/false become 1/0). This is not an
    /// edit, so it does not enter the undo history, but it is recorded (a recording replays the same input).
    fn input(&mut self, a: &Args) -> Result<Json> {
        let values = match a.0.get("values") {
            Some(Json::Obj(p)) => p,
            Some(other) => return Err(EcsError::new("type_mismatch", format!("`values` must be an object, got {}", other.type_name())).at("values")),
            None => return Err(a.missing("values", "object").hint("e.g. {\"values\": {\"w\": true, \"move_x\": 0.5}}")),
        };
        let mut cmds = Vec::with_capacity(values.len());
        for (k, v) in values {
            cmds.push(Command::SetResource { name: format!("input.{k}"), value: Some(resource_number(v, &format!("values.{k}"))?) });
        }
        self.sim.apply_batch(&cmds)?;
        Ok(Json::obj([("ok", Json::Bool(true)), ("set", Json::from(cmds.len()))]))
    }

    /// A point given as [x, y, z] or as the uid of an entity (its `Transform.pos`, less the NavAgent's `feet` when it has one).
    fn point_arg(&mut self, a: &Args, key: &str) -> Result<([f32; 3], Option<lunaris_sim::components::NavAgent>)> {
        let v = a.value(key)?;
        if let Some(p) = v.as_array() {
            if p.len() == 3 && p.iter().all(|x| x.as_f64().is_some()) {
                return Ok(([p[0].as_f64().unwrap_or(0.0) as f32, p[1].as_f64().unwrap_or(0.0) as f32, p[2].as_f64().unwrap_or(0.0) as f32], None));
            }
            return Err(EcsError::new("type_mismatch", format!("`{key}` must be [x, y, z] (three numbers) or the uid of an entity")).at(key));
        }
        let uid = v.as_u64().ok_or_else(|| EcsError::new("type_mismatch", format!("`{key}` must be [x, y, z] (three numbers) or the uid of an entity")).at(key))?;
        let e = self.sim.world.require_entity(uid)?;
        let t = self.sim.world.get::<lunaris_sim::components::Transform>(e).map(|t| t.pos).ok_or_else(|| EcsError::new("missing_component", format!("entity {uid} has no Transform")).at(key))?;
        let agent = self.sim.world.get::<lunaris_sim::components::NavAgent>(e).copied();
        Ok(([t[0], t[1] - agent.map(|n| n.feet).unwrap_or(0.0), t[2]], agent))
    }

    /// `path`: the route between two points, without walking it.
    fn path_tool(&mut self, a: &Args) -> Result<Json> {
        let (from, agent) = self.point_arg(a, "from")?;
        let (to, _) = self.point_arg(a, "to")?;
        let mut params = match agent {
            Some(n) => lunaris_sim::nav::NavParams { radius: n.radius, height: n.height, step_height: n.step_height, max_drop: n.max_drop, ..Default::default() },
            None => lunaris_sim::nav::NavParams::default(),
        };
        for (key, slot) in [("radius", &mut params.radius), ("height", &mut params.height), ("step_height", &mut params.step_height), ("max_drop", &mut params.max_drop)] {
            match a.0.get(key) {
                None | Some(Json::Null) => {}
                Some(v) => *slot = v.as_f64().ok_or_else(|| EcsError::new("type_mismatch", format!("`{key}` must be a number")).at(key))? as f32,
            }
        }
        self.sim.load_terrains();
        self.sim.load_meshes();
        // `from` an entity: it plans on that entity's collision layer (what it does not collide with is no obstacle for it)
        let who = a.0.get("from").and_then(|v| v.as_u64()).and_then(|uid| {
            let layers = lunaris_sim::layers::Layers::gather(&mut self.sim.world);
            (!layers.is_empty()).then(|| layers.of(uid))
        });
        let r = lunaris_sim::nav::find_path_as(&mut self.sim.world, from, to, params, &self.sim.heightfields, &self.sim.meshes, who).map_err(|e| EcsError::new(e.code(), e.message().to_string()).hint("the `path` tool needs walkable ground (a box Collider, a Terrain or a MeshCollider whose triangles face up) under both points; check agent size, `step_height` and `max_drop`"))?;
        let round = |v: f32| Json::Float((v as f64 * 1000.0).round() / 1000.0);
        Ok(Json::obj([
            ("points", Json::arr(r.points.iter().map(|p| Json::arr([round(p[0]), round(p[1]), round(p[2])])))),
            ("jumps", Json::arr(r.jumps.iter().enumerate().filter(|(_, j)| j.is_some()).map(|(i, _)| Json::from(i)))),
            ("length", round(r.length)),
            ("exact", Json::Bool(r.exact)),
            ("searched", Json::from(r.searched)),
            ("cell", round(r.cell)),
        ]))
    }

    /// `prefabs`: the list, or one prefab with the entities it expands to.
    fn prefabs_tool(&self, a: &Args) -> Result<Json> {
        let prefabs = &self.sim.scripts.prefabs;
        if let Some(name) = a.opt_str("name")? {
            let def = prefabs.get(name).ok_or_else(|| EcsError::new("no_such_prefab", format!("no prefab named `{name}`")).hint(format!("prefabs: {}", prefabs.keys().cloned().collect::<Vec<_>>().join(", "))))?;
            let plan = lunaris_sim::prefab::plan(prefabs, name, &[])?;
            let entities = plan.iter().map(|p| Json::obj([("parent", p.parent.map(Json::from).unwrap_or(Json::Null)), ("components", Json::Obj(p.components.clone()))]));
            return Ok(Json::obj([("name", Json::str(name)), ("def", def.clone()), ("entities", Json::arr(entities))]));
        }
        let list = prefabs.keys().map(|n| Json::obj([("name", Json::str(n.clone())), ("entities", Json::from(lunaris_sim::prefab::plan(prefabs, n, &[]).map(|p| p.len()).unwrap_or(0)))]));
        Ok(Json::obj([("prefabs", Json::arr(list))]))
    }

    /// `spawn_prefab`: the root and its children in one all-or-nothing, undoable step.
    fn spawn_prefab_tool(&mut self, a: &Args) -> Result<Json> {
        let name = a.str("prefab")?;
        let count = a.opt_u64("count")?.unwrap_or(1);
        if count == 0 || count > MAX_SPAWN_COUNT {
            return Err(EcsError::new("out_of_range", format!("`count` must be between 1 and {MAX_SPAWN_COUNT}")).at("count"));
        }
        let mut overrides: Vec<(String, Json)> = Vec::new();
        match a.0.get("overrides") {
            None | Some(Json::Null) => {}
            Some(Json::Obj(o)) => {
                for (c, v) in o {
                    lunaris_sim::prefab::merge_component(&mut overrides, c, v);
                }
            }
            Some(other) => return Err(EcsError::new("type_mismatch", format!("`overrides` must be an object keyed by component name, got {}", other.type_name())).at("overrides")),
        }
        if let Some(at) = a.0.get("at") {
            let ok = at.as_array().is_some_and(|p| p.len() == 3 && p.iter().all(|x| x.as_f64().is_some()));
            if !ok {
                return Err(EcsError::new("type_mismatch", "`at` must be [x, y, z] (three numbers)").at("at"));
            }
            lunaris_sim::prefab::merge_component(&mut overrides, "Transform", &Json::obj([("pos", at.clone())]));
        }
        let plan = lunaris_sim::prefab::plan(&self.sim.scripts.prefabs, name, &overrides)?;
        if plan.len() as u64 * count > MAX_SPAWN_COUNT {
            return Err(EcsError::new("out_of_range", format!("{count} copies of {} entities is more than {MAX_SPAWN_COUNT}", plan.len())).at("count"));
        }
        let mut cmds = Vec::with_capacity(plan.len() * count as usize);
        let mut base = self.sim.world.next_uid();
        for _ in 0..count {
            cmds.extend(lunaris_sim::prefab::spawn_commands(&plan, base));
            base += plan.len() as u64;
        }
        let (done, inverse) = self.sim.apply_batch(&cmds)?;
        let undoable = self.push_history(inverse);
        let roots: Vec<Json> = done.iter().step_by(plan.len()).filter_map(|d| d.uid).map(uid_json).collect();
        Ok(Json::obj([("ok", Json::Bool(true)), ("uids", Json::Arr(roots)), ("entities", Json::from(done.len())), ("undoable", Json::Bool(undoable))]))
    }

    // ------------------------------------------------------------------------------------------------------ terrain

    /// The height and slope of the ground at a point of the world: the highest terrain under (x, z).
    fn terrain_height_tool(&mut self, a: &Args) -> Result<Json> {
        use lunaris_sim::components::{Terrain, Transform};
        let num = |k: &str| a.0.get(k).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).ok_or_else(|| a.missing(k, "number"));
        let (x, z) = (num("x")? as f32, num("z")? as f32);
        self.sim.load_terrains();
        let mut list: Vec<(u64, Transform, Terrain)> = Vec::new();
        self.sim.world.for_each_uid::<(&Transform, &Terrain), _>(|_, uid, (t, g)| list.push((uid, *t, *g)))?;
        list.sort_by_key(|l| l.0);
        let mut best: Option<(f32, [f32; 3], u64, String)> = None;
        for (uid, t, g) in &list {
            let name = self.sim.world.strings().get(g.heightmap.0).to_string();
            let Some(field) = self.sim.heightfields.get(&name) else { continue };
            let p = lunaris_sim::terrain::Placed::new(field, t.pos, g);
            if let Some(h) = p.surface(x, z) {
                if best.as_ref().is_none_or(|b| h > b.0) {
                    best = Some((h, p.normal(x, z), *uid, name));
                }
            }
        }
        match best {
            Some((h, n, uid, name)) => Ok(Json::obj([("height", Json::Float(h as f64)), ("normal", Json::arr(n.iter().map(|v| Json::Float(*v as f64)))), ("uid", uid_json(uid)), ("heightmap", Json::str(name))])),
            None => Err(EcsError::new("no_terrain", format!("there is no terrain under x = {x}, z = {z}")).hint("terrain is an entity with a Terrain component whose heightmap could be read: see `check`")),
        }
    }

    // ------------------------------------------------------------------------------------------------------ Lunaris Script

    fn compile_script_tool(&mut self, a: &Args) -> Result<Json> {
        let src = a.str("source")?;
        Ok(match lunaris_script::compile(src) {
            Ok(list) => Json::obj([("ok", Json::Bool(true)), ("systems", Json::arr(list.iter().map(|c| Json::obj([("name", Json::str(c.name.clone())), ("def", c.def.clone())]))))]),
            Err(d) => Json::obj([("ok", Json::Bool(false)), ("errors", Json::arr(d.iter().map(script_diag_json)))]),
        })
    }

    /// Define every system of a script, all or nothing, as one undo step. A mistake comes back with its line and column.
    fn define_script_tool(&mut self, a: &Args) -> Result<Json> {
        let src = a.str("source")?;
        let list = lunaris_script::compile(src).map_err(|d| {
            let lines: Vec<String> = d.iter().take(10).map(|x| x.to_text()).collect();
            EcsError::new("bad_script", format!("{} problem{} in the script, the first at {}", d.len(), if d.len() == 1 { "" } else { "s" }, lines[0])).hint(lines.join("\n"))
        })?;
        // all or nothing, one undo step
        let cmds: Vec<Command> = list.iter().map(|c| Command::DefineSystem { def: c.def.clone() }).collect();
        match self.sim.apply_batch(&cmds) {
            Ok((_, inverse)) => {
                self.push_history(inverse);
            }
            Err(batch_err) => {
                // which system did the world refuse? Try them one at a time on a copy
                let mut scratch = self.sim.clone();
                for (c, cmd) in list.iter().zip(&cmds) {
                    if let Err(e) = scratch.apply(cmd) {
                        let range = e.path.as_deref().and_then(|p| c.locate(p)).unwrap_or_else(|| c.range.clone());
                        let d = lunaris_script::diagnostic_at(src, range, e.message.clone(), e.hint.clone());
                        let mut out = EcsError::new(e.code, format!("{}:{}: {} (system `{}`)", d.line, d.col, e.message, c.name));
                        out.path = e.path;
                        out.hint = e.hint;
                        return Err(out);
                    }
                }
                return Err(batch_err);
            }
        }
        Ok(Json::obj([("ok", Json::Bool(true)), ("systems", Json::arr(list.iter().map(|c| Json::str(c.name.clone()))))]))
    }

    /// A graph system as script text.
    fn get_script_tool(&mut self, a: &Args) -> Result<Json> {
        let name = a.str("name")?;
        let sys = self.sim.scripts.get(name).ok_or_else(|| {
            let all: Vec<&str> = self.sim.scripts.systems.iter().map(|s| s.name.as_str()).collect();
            EcsError::new("no_such_system", format!("no graph system named `{name}`")).hint(format!("graph systems: {}", if all.is_empty() { "(none)".to_string() } else { all.join(", ") }))
        })?;
        let source = lunaris_script::decompile(&sys.def).map_err(|e| EcsError::new("no_script_form", e))?;
        Ok(Json::obj([("name", Json::str(name)), ("source", Json::str(source))]))
    }

    pub(crate) fn single(&mut self, cmd: Command) -> Result<Json> {
        let applied = self.sim.apply(&cmd)?;
        let undoable = self.push_history(applied.inverse);
        let mut out = Json::obj([("ok", Json::Bool(true)), ("undoable", Json::Bool(undoable))]);
        if let Some(uid) = applied.uid {
            out.set("uid", uid_json(uid));
        }
        Ok(out)
    }

    fn spawn(&mut self, a: &Args) -> Result<Json> {
        let components = match a.0.get("components") {
            None | Some(Json::Null) => Vec::new(),
            Some(Json::Obj(p)) => p.clone(),
            Some(_) => return Err(EcsError::new("type_mismatch", "`components` must be an object keyed by component name").at("components").hint("e.g. {\"Transform\": {\"pos\": [0,1,0]}, \"Name\": {\"value\": \"Hero\"}}")),
        };
        let count = a.opt_u64("count")?.unwrap_or(1);
        if count == 0 || count > MAX_SPAWN_COUNT {
            return Err(EcsError::new("out_of_range", format!("`count` must be between 1 and {MAX_SPAWN_COUNT}")).at("count"));
        }
        let uid = a.opt_u64("uid")?;
        if count > 1 && uid.is_some() {
            return Err(EcsError::new("bad_arguments", "`uid` cannot be combined with `count` > 1"));
        }
        if count == 1 {
            return self.single(Command::Spawn { uid, components });
        }
        let cmds: Vec<Command> = (0..count).map(|_| Command::Spawn { uid: None, components: components.clone() }).collect();
        let (done, inverse) = self.sim.apply_batch(&cmds)?;
        let undoable = self.push_history(inverse);
        let uids: Vec<u64> = done.iter().filter_map(|d| d.uid).collect();
        Ok(Json::obj([
            ("ok", Json::Bool(true)),
            ("undoable", Json::Bool(undoable)),
            ("count", Json::from(uids.len())),
            ("first_uid", uid_json(uids[0])),
            ("last_uid", uid_json(*uids.last().unwrap())),
        ]))
    }

    fn batch(&mut self, a: &Args) -> Result<Json> {
        let list = a.value("commands")?.as_array().ok_or_else(|| EcsError::new("type_mismatch", "`commands` must be an array of {op, ...} objects").at("commands"))?;
        if list.is_empty() {
            return Err(EcsError::new("bad_arguments", "`commands` is empty"));
        }
        let mut cmds = Vec::with_capacity(list.len());
        for (i, c) in list.iter().enumerate() {
            cmds.push(Command::from_json(c).map_err(|e| e.at(format!("commands[{i}]")))?);
        }
        let (done, inverse) = self.sim.apply_batch(&cmds)?;
        let undoable = self.push_history(inverse);
        Ok(Json::obj([
            ("ok", Json::Bool(true)),
            ("applied", Json::from(done.len())),
            ("undoable", Json::Bool(undoable)),
            ("uids", Json::arr(done.iter().filter_map(|d| d.uid).map(uid_json))),
        ]))
    }

    fn undo_redo(&mut self, undo: bool) -> Result<Json> {
        let group = if undo { self.undo.pop() } else { self.redo.pop() };
        let group = group.ok_or_else(|| EcsError::new(if undo { "nothing_to_undo" } else { "nothing_to_redo" }, if undo { "there is nothing to undo" } else { "there is nothing to redo" }))?;
        let result = self.sim.apply_batch(&group);
        match result {
            Ok((_, inverse)) => {
                if let Some(inv) = inverse {
                    if undo {
                        self.redo.push(inv);
                    } else {
                        self.undo.push(inv);
                    }
                }
                Ok(Json::obj([("ok", Json::Bool(true)), ("commands", Json::arr(group.iter().map(|c| c.to_json()))), ("undo_depth", Json::from(self.undo.len())), ("redo_depth", Json::from(self.redo.len()))]))
            }
            Err(e) => {
                // the world moved on (for example an entity was deleted by other means): keep the group so nothing is lost
                if undo {
                    self.undo.push(group);
                } else {
                    self.redo.push(group);
                }
                Err(e.hint_if_empty("the world changed since this edit; check state with `get` or `query` first"))
            }
        }
    }

    // ------------------------------------------------------------------------------------------ reading
    fn get(&self, a: &Args) -> Result<Json> {
        let w = &self.sim.world;
        let uid = a.u64("uid")?;
        let e = w.require_entity(uid)?;
        let select = a.names("components")?;
        self.entity_json(e, select.as_deref())
    }

    fn entity_json(&self, e: Entity, select: Option<&[&str]>) -> Result<Json> {
        let w = &self.sim.world;
        let ids: Vec<ComponentId> = match select {
            None => w.components_of(e),
            Some(names) => names.iter().map(|n| w.require_component(n)).collect::<Result<Vec<_>>>()?,
        };
        let mut comps = Vec::new();
        for id in ids {
            if let Some(v) = w.get_json(e, id) {
                comps.push((w.info(id).name.clone(), v));
            }
        }
        Ok(Json::obj([("uid", uid_json(w.uid_of(e).unwrap())), ("components", Json::Obj(comps))]))
    }

    fn query(&self, a: &Args) -> Result<Json> {
        let w = &self.sim.world;
        let with_names = a.names("with")?.unwrap_or_default();
        let without_names = a.names("without")?.unwrap_or_default();
        let select = a.names("select")?;
        let limit = a.opt_u64("limit")?.unwrap_or(100).min(MAX_QUERY_LIMIT);
        let offset = a.opt_u64("offset")? .unwrap_or(0) as usize;
        let count_only = matches!(a.0.get("count_only"), Some(Json::Bool(true)));

        let mut with: Vec<ComponentId> = with_names.iter().map(|n| w.require_component(n)).collect::<Result<_>>()?;
        let without: Vec<ComponentId> = without_names.iter().map(|n| w.require_component(n)).collect::<Result<_>>()?;
        if let Some(names) = &select {
            for n in names {
                let id = w.require_component(n)?; // fail early on typos
                let _ = id;
            }
        }
        // where clauses
        struct Clause {
            cid: ComponentId,
            field: String,
            index: Option<usize>,
            op: String,
            value: Json,
        }
        let mut clauses: Vec<Clause> = Vec::new();
        if let Some(Json::Arr(list)) = a.0.get("where") {
            for (i, c) in list.iter().enumerate() {
                let at = format!("where[{i}]");
                let cname = c.get("component").and_then(|v| v.as_str()).ok_or_else(|| EcsError::new("bad_arguments", "each where clause needs a string `component`").at(at.clone()))?;
                let field = c.get("field").and_then(|v| v.as_str()).ok_or_else(|| EcsError::new("bad_arguments", "each where clause needs a string `field`").at(at.clone()))?;
                let op = c.get("op").and_then(|v| v.as_str()).unwrap_or("==");
                if !["==", "!=", "<", "<=", ">", ">="].contains(&op) {
                    return Err(EcsError::new("bad_arguments", format!("unknown operator `{op}`")).hint("operators: == != < <= > >=").at(at));
                }
                let value = c.get("value").cloned().ok_or_else(|| EcsError::new("bad_arguments", "each where clause needs a `value`").at(at.clone()))?;
                let cid = w.require_component(cname)?;
                let info = w.info(cid);
                let f = info.field(field).ok_or_else(|| EcsError::new("unknown_field", format!("{cname} has no field `{field}`")).hint(format!("fields: {}", info.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", "))).at(at.clone()))?;
                let index = c.get("index").and_then(|v| v.as_u64()).map(|i| i as usize);
                if f.count > 1 && index.is_none() {
                    return Err(EcsError::new("bad_arguments", format!("{cname}.{field} is an array of {}; give an `index`", f.count)).at(at));
                }
                if !with.contains(&cid) {
                    with.push(cid);
                }
                clauses.push(Clause { cid, field: field.to_string(), index, op: op.to_string(), value });
            }
        }
        let matches = |e: Entity| -> bool {
            clauses.iter().all(|c| {
                let Some(obj) = w.get_json(e, c.cid) else { return false };
                let Some(mut v) = obj.get(&c.field).cloned() else { return false };
                if let Some(i) = c.index {
                    v = match v.index(i) {
                        Some(x) => x.clone(),
                        None => return false,
                    };
                }
                match (v.as_f64(), c.value.as_f64()) {
                    (Some(x), Some(y)) => match c.op.as_str() {
                        "==" => x == y,
                        "!=" => x != y,
                        "<" => x < y,
                        "<=" => x <= y,
                        ">" => x > y,
                        _ => x >= y,
                    },
                    _ => match c.op.as_str() {
                        "==" => v == c.value,
                        "!=" => v != c.value,
                        _ => false,
                    },
                }
            })
        };
        let mut hits: Vec<(u64, Entity)> = Vec::new();
        for ai in w.matching_archetypes(&with, &without) {
            for &e in w.archetypes()[ai].entities() {
                if clauses.is_empty() || matches(e) {
                    hits.push((w.uid_of(e).unwrap(), e));
                }
            }
        }
        hits.sort_unstable_by_key(|h| h.0);
        let total = hits.len();
        if count_only {
            return Ok(Json::obj([("total", Json::from(total))]));
        }
        let mut out = Vec::new();
        for (_, e) in hits.into_iter().skip(offset).take(limit as usize) {
            out.push(self.entity_json(e, select.as_deref())?);
        }
        let returned = out.len();
        Ok(Json::obj([("total", Json::from(total)), ("offset", Json::from(offset)), ("returned", Json::from(returned)), ("entities", Json::Arr(out))]))
    }

    // ------------------------------------------------------------------------------------------ simulation
    pub(crate) fn step(&mut self, a: &Args) -> Result<Json> {
        let ticks = a.opt_u64("ticks")?.unwrap_or(1);
        if ticks > MAX_STEP_TICKS {
            return Err(EcsError::new("out_of_range", format!("`ticks` is at most {MAX_STEP_TICKS} per call")).hint("call step several times").at("ticks"));
        }
        // a wall-clock budget, so one call can never keep the engine busy for minutes (an agent cannot interrupt a running call)
        let budget_ms = a.opt_u64("budget_ms")?.unwrap_or(DEFAULT_STEP_BUDGET_MS).clamp(1, MAX_STEP_BUDGET_MS);
        let t0 = std::time::Instant::now();
        let first_tick = self.sim.tick;
        let mut run = 0u64;
        while run < ticks {
            self.sim.step();
            run += 1;
            if run % 4 == 0 && t0.elapsed().as_millis() as u64 >= budget_ms {
                break;
            }
        }
        let secs = t0.elapsed().as_secs_f64();
        let mut out = Json::obj([
            ("tick", Json::from(self.sim.tick as i64)),
            ("ticks_run", Json::from(run as i64)),
            ("entities", Json::from(self.sim.world.len())),
            ("hash", hex(self.sim.state_hash())),
            ("elapsed_ms", Json::from((secs * 1000.0 * 1000.0).round() / 1000.0)),
        ]);
        let fresh: Vec<&lunaris_sim::Event> = self.sim.events.iter().filter(|e| e.tick >= first_tick).collect();
        if !fresh.is_empty() {
            out.set("events", Json::arr(fresh.iter().rev().take(100).rev().map(|e| event_json(e))));
        }
        if run < ticks {
            out.set("stopped_early", Json::Bool(true));
            out.set("note", Json::str(format!("stopped after {budget_ms} ms with {} of {ticks} ticks done; call step again to continue (or raise budget_ms, at most {MAX_STEP_BUDGET_MS})", run)));
        }
        Ok(out)
    }

    fn no_snapshot(&self, name: &str) -> EcsError {
        EcsError::new("no_such_snapshot", format!("no snapshot named `{name}`")).hint(format!("snapshots: {}", if self.snapshots.is_empty() { "(none)".to_string() } else { self.snapshots.keys().cloned().collect::<Vec<_>>().join(", ") }))
    }

    fn snapshot(&mut self, a: &Args) -> Result<Json> {
        let name = a.str("name")?;
        if name.is_empty() || name.len() > 64 {
            return Err(EcsError::new("bad_name", "snapshot names are 1 to 64 characters").at("name"));
        }
        if !self.snapshots.contains_key(name) && self.snapshots.len() >= MAX_SNAPSHOTS {
            return Err(EcsError::new("too_many_snapshots", format!("at most {MAX_SNAPSHOTS} snapshots are kept")).hint("delete one with delete_snapshot"));
        }
        let mut snap = self.sim.clone();
        snap.stop_recording().ok();
        self.snapshots.insert(name.to_string(), snap);
        Ok(Json::obj([("name", Json::str(name)), ("tick", Json::from(self.sim.tick as i64)), ("hash", hex(self.sim.state_hash()))]))
    }

    fn restore(&mut self, a: &Args) -> Result<Json> {
        let name = a.str("name")?;
        if self.sim.is_recording() {
            return Err(EcsError::new("recording_active", "stop the recording before restoring a snapshot").hint("call record_stop"));
        }
        let snap = self.snapshots.get(name).ok_or_else(|| self.no_snapshot(name))?.clone();
        self.sim = snap;
        self.undo.clear();
        self.redo.clear();
        Ok(Json::obj([("restored", Json::str(name)), ("tick", Json::from(self.sim.tick as i64)), ("entities", Json::from(self.sim.world.len())), ("hash", hex(self.sim.state_hash()))]))
    }

    // ------------------------------------------------------------------------------------------ recording and scenes
    fn record_stop(&mut self, a: &Args) -> Result<Json> {
        let rec = self.sim.stop_recording()?;
        let mut out = Json::obj([
            ("entries", Json::from(rec.entries.len())),
            ("start_tick", Json::from(rec.start_tick as i64)),
            ("final_tick", Json::from(rec.final_tick as i64)),
            ("final_hash", hex(rec.final_hash)),
        ]);
        if let Some(path) = a.opt_str("path")? {
            let text = rec.to_json().to_string_pretty().map_err(|e| EcsError::new("write_error", e.to_string()))?;
            let p = self.write_text(path, &text)?;
            out.set("saved_to", Json::str(p.display().to_string()));
        }
        self.last_recording = Some(rec);
        Ok(out)
    }

    fn replay(&mut self, a: &Args) -> Result<Json> {
        let rec = match a.opt_str("path")? {
            Some(path) => {
                let text = self.read_text(path)?;
                let j = parse(&text).map_err(|e| EcsError::new("bad_recording", e.to_string()).at("path"))?;
                Recording::from_json(&j)?
            }
            None => self.last_recording.clone().ok_or_else(|| EcsError::new("no_recording", "no recording to replay").hint("call record_start, do some work, then record_stop (or pass `path`)"))?,
        };
        let replayed = Sim::replay(&self.sim, &rec)?;
        let got = replayed.state_hash();
        Ok(Json::obj([("matches", Json::Bool(got == rec.final_hash)), ("expected", hex(rec.final_hash)), ("got", hex(got)), ("final_tick", Json::from(replayed.tick as i64)), ("entries", Json::from(rec.entries.len()))]))
    }

    fn load_scene(&mut self, a: &Args) -> Result<Json> {
        if self.sim.is_recording() {
            return Err(EcsError::new("recording_active", "stop the recording before loading a scene").hint("call record_stop"));
        }
        let scene = match (a.opt_str("path")?, a.0.get("scene")) {
            (Some(path), None) => parse(&self.read_text(path)?).map_err(|e| EcsError::new("bad_scene", e.to_string()).at("path"))?,
            (None, Some(s)) => s.clone(),
            _ => return Err(EcsError::new("bad_arguments", "give exactly one of `path` or `scene`")),
        };
        let mut next = self.sim.clone();
        next.clear();
        let created = next.load_scene(&scene)?; // errors leave the live simulation untouched
        self.sim = next;
        self.undo.clear();
        self.redo.clear();
        Ok(Json::obj([("entities", Json::from(created.len())), ("tick", Json::from(0)), ("hash", hex(self.sim.state_hash()))]))
    }

    /// `model_info`: what is in a model file (size, bounds, texture, skeleton and animation clips), so that it can be placed and played
    /// without guessing.
    fn model_info_tool(&mut self, a: &Args) -> Result<Json> {
        let path = a.str("path")?;
        let root_joint = a.opt_str("root")?;
        let before = self.textures.warnings.len();
        let loaded = self.textures.model(path);
        let warnings: Vec<Json> = self.textures.warnings.drain(before..).map(Json::Str).collect();
        let Some(mesh) = loaded else {
            let why = warnings.first().and_then(|w| w.as_str()).unwrap_or("it could not be read").to_string();
            return Err(EcsError::new("model_load_failed", format!("cannot load the model `{path}`: {why}")).hint("paths are relative to the workspace folder; `files` with kind \"model\" lists the models").at("path"));
        };
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in &mesh.pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let v3 = |v: [f32; 3]| Json::arr(v.iter().map(|&x| Json::from(((x as f64) * 1e4).round() / 1e4)));
        // root motion as text for a `RootMotion` component: how far the root joint (the first joint, or the one named by `root`) travels in each clip
        let root_motion = mesh.rig.as_ref().map(|r| {
            let joint = root_joint.and_then(|n| r.joint_index(n)).or_else(|| r.order.first().copied()).unwrap_or(0);
            let lines: Vec<String> = r
                .clips
                .iter()
                .enumerate()
                .filter(|(_, c)| c.duration > 0.0 && !c.name.is_empty() && !c.name.contains(char::is_whitespace))
                .map(|(i, c)| {
                    let samples = ((c.duration * 10.0).ceil() as usize).clamp(2, 64);
                    let path = r.joint_path(i, joint, samples);
                    let first = path.first().copied().unwrap_or([0.0; 3]);
                    let keys: Vec<String> = path.iter().enumerate().map(|(k, p)| format!("{:.3} {:.4} {:.4} {:.4}", c.duration * k as f32 / samples as f32, p[0] - first[0], p[1] - first[1], p[2] - first[2])).collect();
                    format!("{} length={:.4}: {}", c.name, c.duration, keys.join(", "))
                })
                .collect();
            (r.joints.get(joint).map(|j| j.name.clone()).unwrap_or_default(), lines.join("\n"))
        });
        let rig = match &mesh.rig {
            Some(r) => Json::obj([
                ("root_motion_joint", Json::str(root_motion.as_ref().map(|m| m.0.clone()).unwrap_or_default())),
                ("root_motion", Json::str(root_motion.as_ref().map(|m| m.1.clone()).unwrap_or_default())),
                ("joints", Json::from(r.joints.len())),
                ("joint_names", Json::arr(r.joints.iter().take(128).map(|j| Json::str(j.name.clone())))),
                ("clips", Json::arr(r.clips.iter().map(|c| Json::obj([("name", Json::str(c.name.clone())), ("duration", Json::from(((c.duration as f64) * 1e4).round() / 1e4)), ("tracks", Json::from(c.tracks.len()))])))),
            ]),
            None => Json::Null,
        };
        Ok(Json::obj([
            ("path", Json::str(path)),
            ("vertices", Json::from(mesh.pos.len())),
            ("triangles", Json::from(mesh.idx.len() / 3)),
            ("bounds", Json::obj([("min", v3(lo)), ("max", v3(hi)), ("size", v3([hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]])), ("center", v3([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0]))])),
            ("textured", Json::Bool(mesh.tex.is_some())),
            ("vertex_colors", Json::Bool(!mesh.col.is_empty())),
            ("rig", rig),
            ("warnings", Json::Arr(warnings)),
        ]))
    }

    /// `files`: what is in the workspace folder, by kind. Paths come back relative to the workspace root, with `/` separators.
    fn files_tool(&mut self, a: &Args) -> Result<Json> {
        let dir_rel = a.opt_str("path")?.unwrap_or(".");
        let dir = if dir_rel == "." || dir_rel.is_empty() { self.root.clone() } else { self.resolve(dir_rel)? };
        let recursive = matches!(a.0.get("recursive"), Some(Json::Bool(true)));
        let want = a.opt_str("kind")?;
        if let Some(k) = want {
            if !["scene", "model", "texture", "sound", "font", "other"].contains(&k) {
                return Err(EcsError::new("bad_kind", format!("unknown kind `{k}`")).hint("kinds: scene, model, texture, sound, font, other").at("kind"));
            }
        }
        let limit = a.opt_u64("limit")?.unwrap_or(200).clamp(1, 2000) as usize;
        let meta = std::fs::metadata(&dir).map_err(|e| EcsError::new("io_error", format!("cannot read {}: {e}", dir.display())).at("path"))?;
        if !meta.is_dir() {
            return Err(EcsError::new("not_a_folder", format!("`{dir_rel}` is a file, not a folder")).at("path"));
        }
        let mut out: Vec<(String, &'static str, u64)> = Vec::new();
        let mut truncated = false;
        let mut stack = vec![dir];
        'walk: while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            let mut entries: Vec<std::fs::DirEntry> = rd.filter_map(|e| e.ok()).collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let name = e.file_name().to_string_lossy().to_string();
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_dir() {
                    // the build folder, version control and hidden folders are noise
                    if recursive && name != "target" && !name.starts_with('.') && name != "node_modules" {
                        stack.push(e.path());
                    }
                    continue;
                }
                let lower = name.to_ascii_lowercase();
                let kind = if lower.ends_with(".lunaris.json") {
                    "scene"
                } else if lower.ends_with(".glb") || lower.ends_with(".gltf") {
                    "model"
                } else if lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
                    "texture"
                } else if lower.ends_with(".wav") || lower.ends_with(".ogg") || lower.ends_with(".mp3") {
                    "sound"
                } else if lower.ends_with(".ttf") || lower.ends_with(".otf") {
                    "font"
                } else {
                    "other"
                };
                if want.is_some_and(|k| k != kind) {
                    continue;
                }
                let rel = e.path().strip_prefix(&self.root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or(name);
                out.push((rel, kind, e.metadata().map(|m| m.len()).unwrap_or(0)));
                if out.len() >= limit {
                    truncated = true;
                    break 'walk;
                }
            }
        }
        out.sort();
        Ok(Json::obj([
            ("folder", Json::str(dir_rel.to_string())),
            ("total", Json::from(out.len())),
            ("truncated", Json::Bool(truncated)),
            ("files", Json::arr(out.iter().map(|(p, k, n)| Json::obj([("path", Json::str(p.clone())), ("kind", Json::str(*k)), ("bytes", Json::from(*n as i64))])))),
        ]))
    }

    fn save_scene(&mut self, a: &Args) -> Result<Json> {
        let scene = self.sim.scene_json();
        match a.opt_str("path")? {
            Some(path) => {
                let text = scene.to_string_pretty().map_err(|e| EcsError::new("write_error", e.to_string()))?;
                let p = self.write_text(path, &text)?;
                Ok(Json::obj([("saved_to", Json::str(p.display().to_string())), ("entities", Json::from(self.sim.world.len())), ("bytes", Json::from(text.len()))]))
            }
            None => Ok(scene),
        }
    }
}

trait HintIfEmpty {
    fn hint_if_empty(self, hint: &str) -> Self;
}

impl HintIfEmpty for EcsError {
    fn hint_if_empty(mut self, hint: &str) -> Self {
        if self.hint.is_none() {
            self.hint = Some(hint.to_string());
        }
        self
    }
}

/// A resource value from JSON: a finite number, or a boolean (true is 1, false is 0).
fn resource_number(v: &Json, at: &str) -> Result<f64> {
    match v {
        Json::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        other => other.as_f64().filter(|x| x.is_finite()).ok_or_else(|| EcsError::new("type_mismatch", format!("`{at}` must be a number or a boolean, got {}", other.type_name())).at(at)),
    }
}

fn event_json(e: &lunaris_sim::Event) -> Json {
    let mut o = Json::obj([("tick", Json::from(e.tick as i64)), ("name", Json::str(e.name.clone())), ("uid", Json::from(e.uid as i64))]);
    if let Some(p) = e.pos {
        o.set("pos", Json::arr(p.iter().map(|x| Json::from(*x as f64))));
    }
    o
}

/// The agent guide (`docs/AI_GUIDE.md`), whole or one section.
fn guide_tool(a: &Args) -> Result<Json> {
    const GUIDE: &str = include_str!("../../../docs/AI_GUIDE.md");
    let text = GUIDE.replace("\r\n", "\n");
    // sections start at "## " headings; the text before the first one is the introduction
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut intro = String::new();
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            sections.push((h.trim().to_string(), format!("## {}\n", h.trim())));
        } else if let Some(last) = sections.last_mut() {
            last.1.push_str(line);
            last.1.push('\n');
        } else {
            intro.push_str(line);
            intro.push('\n');
        }
    }
    let topics: Vec<String> = sections.iter().map(|s| s.0.to_lowercase()).collect();
    match a.opt_str("topic")? {
        None => Ok(Json::obj([("topics", Json::arr(topics.iter().map(|t| Json::str(t.clone())))), ("text", Json::Str(text))])),
        Some(want) => {
            let w = want.to_lowercase();
            let hit = sections.iter().zip(&topics).find(|(_, t)| **t == w || t.contains(&w) || w.contains(t.as_str()));
            match hit {
                Some(((_, body), t)) => Ok(Json::obj([("topic", Json::str(t.clone())), ("topics", Json::arr(topics.iter().map(|t| Json::str(t.clone())))), ("text", Json::Str(body.clone()))])),
                None => Err(EcsError::new("no_such_topic", format!("no guide topic `{want}`")).hint(format!("topics: {}", topics.join(", "))).at("topic")),
            }
        }
    }
}
