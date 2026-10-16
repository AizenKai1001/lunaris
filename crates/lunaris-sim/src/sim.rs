//! The deterministic simulation: a world, a tick counter, a seed, an ordered list of systems, and a fixed time step.
use crate::command::{apply, Applied, Command};
use crate::graph::{self, ScriptSet};
use crate::physics::Contact;
use crate::components::*;
use crate::rng::{hash_rand, Rng};
use crate::systems;
use lunaris_ecs::{EcsError, Result, StateHasher, World};
use lunaris_json::Json;
use std::path::PathBuf;
use std::sync::Arc;

/// Something that happened that the outside world may want to react to (a sound to play, a message to show). Events are
/// output of the simulation, not state: they are not part of the state hash and a replay produces them again.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub tick: u64,
    pub name: String,
    /// The entity that emitted it.
    pub uid: u64,
    /// Where that entity was (its `Transform` position) when it emitted the event: sound is placed there. `None` when it has no `Transform`.
    pub pos: Option<[f32; 3]>,
}

/// How many recent events are kept.
pub const MAX_EVENTS: usize = 2048;

/// How often (in ticks) a model file that a `MeshCollider` uses is looked at to see whether it changed.
pub const MESH_RECHECK: u64 = 60;

/// What a system sees besides the world.
pub struct SimCtx {
    pub tick: u64,
    /// Worker threads a system may use (results never depend on this number).
    pub threads: usize,
    /// Fixed time step in seconds (the same every tick).
    pub dt: f32,
    pub seed: u64,
    /// Order-dependent stream: only draw from it in code that visits entities in a canonical order.
    pub rng: Rng,
    /// Gravity from the world's `PhysicsSettings` (or the default).
    pub gravity: [f32; 3],
    /// Filled by the physics system with this tick's contacts.
    pub contacts: Vec<Contact>,
    /// The heightmaps of the terrain in the world, by path.
    pub terrains: Arc<crate::terrain::Terrains>,
    /// The triangle meshes of the `MeshCollider`s in the world that have been read (empty, and costing nothing, without any).
    pub meshes: Arc<crate::meshcol::MeshStore>,
}

impl SimCtx {
    /// Per-entity random number in [0, 1): depends only on (seed, tick, uid, salt), never on storage order.
    pub fn entity_rand(&self, uid: u64, salt: u64) -> f32 {
        hash_rand(self.seed, self.tick, uid, salt)
    }
}

pub type SystemFn = dyn Fn(&mut World, &mut SimCtx) + Send + Sync;

#[derive(Clone)]
pub struct SystemEntry {
    pub name: String,
    pub run: Arc<SystemFn>,
}

#[derive(Clone, Debug)]
pub struct Recording {
    pub seed: u64,
    pub dt: f32,
    pub start_tick: u64,
    pub start_scene: Json,
    pub start_rng: [u64; 4],
    pub entries: Vec<(u64, Command)>,
    pub final_tick: u64,
    pub final_hash: u64,
    /// How many ticks were stepped while recording. Usually `final_tick - start_tick`, but a game that loads a saved game from inside (`game.load`) goes back in time:
    /// the replay must step as often as the game did, not up to a tick number.
    pub steps: u64,
    /// What the game asked the files for while it was recorded (`game.probe`, `game.load`), in order: the replay is given the same answers, whatever the files say by then.
    pub reads: Vec<RecordedRead>,
}

/// One answer the outside world gave to a running game (see `Recording::reads`).
#[derive(Clone, Debug, PartialEq)]
pub enum RecordedRead {
    /// `game.probe`: did slot `slot` have a saved game?
    Probe { slot: u32, exists: bool },
    /// `game.load`: the saved game that slot `slot` held (`None`: nothing, or something unreadable).
    Load { slot: u32, save: Option<Json> },
}

/// Time spent in one part of a tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct StageTime {
    pub total_ms: f64,
    pub max_ms: f64,
    pub calls: u64,
}

/// Where the time of each tick goes, collected while `Sim::profile` is set (the `profile` tool does that). Wall-clock measurements: not part of the
/// state, and never read by the simulation.
#[derive(Clone, Debug, Default)]
pub struct StepProfile {
    pub ticks: u64,
    pub total_ms: f64,
    pub max_ms: f64,
    pub stages: std::collections::BTreeMap<String, StageTime>,
}

impl StepProfile {
    fn add(&mut self, name: &str, ms: f64) {
        let s = self.stages.entry(name.to_string()).or_default();
        s.total_ms += ms;
        s.max_ms = s.max_ms.max(ms);
        s.calls += 1;
    }
}

/// What `Sim::checkpoint` keeps (see there). Opaque: take one, rewind to it.
#[derive(Clone)]
pub struct Checkpoint {
    tick: u64,
    world: World,
    rng: Rng,
    nav: crate::nav::NavState,
    bt: std::collections::BTreeMap<u64, (u64, Vec<f32>)>,
    stamp: u64,
}

impl Checkpoint {
    /// The tick the simulation was at: the next `step` after rewinding to this checkpoint runs this tick.
    pub fn tick(&self) -> u64 {
        self.tick
    }
}

#[derive(Clone)]
pub struct Sim {
    pub world: World,
    pub tick: u64,
    pub dt: f32,
    pub seed: u64,
    pub rng: Rng,
    /// Worker threads for parallel systems. Not part of the state: any value gives identical results.
    pub threads: usize,
    /// Graph systems (gameplay logic as data); they run after the native systems.
    pub scripts: ScriptSet,
    /// The paths navigation agents are following (see `nav`).
    pub nav: crate::nav::NavState,
    /// Contacts found by the physics system in the last tick (derived from the state, so not hashed).
    pub contacts: Vec<Contact>,
    /// What the characters with a `CharacterMotion` touched at the end of the last tick, waiting to be handed to the next one's graph systems (derived, not hashed).
    pub(crate) char_contacts: Vec<Contact>,
    /// The most recent events emitted by graph systems, oldest first.
    pub events: Vec<Event>,
    /// The workspace folder that the heightmaps of `Terrain` entities are read from (none: terrain has no ground).
    pub asset_root: Option<PathBuf>,
    /// The heightmaps that have been read (a replay or a snapshot shares them).
    pub heightfields: Arc<crate::terrain::Terrains>,
    /// Why a heightmap could not be read, by path (tried again now and then).
    pub terrain_errors: std::collections::BTreeMap<String, String>,
    /// The models that `MeshCollider` entities name and that have been read (a replay or a snapshot shares them).
    pub meshes: Arc<crate::meshcol::MeshStore>,
    /// Why a model could not be read, by path (tried again now and then; a model that was read and then broke on disk keeps its last good copy).
    pub mesh_errors: std::collections::BTreeMap<String, String>,
    /// What each model file looked like when it was read (modified time, length), to notice that it changed.
    mesh_stamps: std::collections::BTreeMap<String, (Option<std::time::SystemTime>, u64)>,
    /// Set it to `Some(Default::default())` and every tick adds the time of its stages (not part of the state).
    pub profile: Option<StepProfile>,
    systems: Vec<SystemEntry>,
    recording: Option<Recording>,
    /// While a recording is replayed: the answers the files gave the game the first time (`Recording::reads`), handed out in order.
    replay_reads: Option<std::collections::VecDeque<RecordedRead>>,
}

impl Sim {
    /// A simulation with the standard components and systems, running at 60 ticks per second.
    pub fn new(seed: u64) -> Sim {
        let mut world = World::new();
        register_standard_components(&mut world);
        let mut sim = Sim { world, tick: 0, dt: 1.0 / 60.0, seed, rng: Rng::new(seed), threads: default_threads(), scripts: ScriptSet::default(), nav: Default::default(), contacts: Vec::new(), char_contacts: Vec::new(), events: Vec::new(), asset_root: None, heightfields: Arc::default(), terrain_errors: Default::default(), meshes: Arc::default(), mesh_errors: Default::default(), mesh_stamps: Default::default(), profile: None, systems: Vec::new(), recording: None, replay_reads: None };
        sim.add_system("ui", systems::ui);
        sim.add_system("wander", systems::wander);
        sim.add_system("particles", systems::particles);
        sim.add_system("animate", systems::animate);
        sim.add_system("movement", systems::movement);
        sim.add_system("physics", systems::physics);
        sim
    }

    /// The whole simulation state as a text scene: the world plus the graph systems.
    pub fn scene_json(&self) -> Json {
        let mut j = lunaris_ecs::to_scene_json(&self.world);
        j.set("systems", self.scripts.defs_json());
        if !self.scripts.prefabs.is_empty() {
            j.set("prefabs", Json::Obj(self.scripts.prefabs.iter().map(|(n, d)| (n.clone(), d.clone())).collect()));
        }
        j
    }

    /// A saved game: the scene (entities, resources, graph systems, prefabs) plus the tick and the random generator, so that playing on after
    /// `load_save` is the same as never having stopped (apart from navigation paths in progress, which are planned again).
    pub fn save_json(&self) -> Json {
        let mut j = self.scene_json();
        let rng: Vec<Json> = self.rng.state().iter().map(|v| Json::str(format!("{v:016x}"))).collect();
        j.set("save", Json::obj([("tick", Json::from(self.tick as i64)), ("seed", Json::from(self.seed as i64)), ("rng", Json::Arr(rng))]));
        j
    }

    /// Replace the world with a saved game (`save_json`). An error leaves everything as it was.
    pub fn load_save(&mut self, save: &Json) -> Result<Vec<u64>> {
        // a recording in progress goes on across the load (a game that loads a saved game from inside, `game.load`, is recorded and replayed as it happened: the replay
        // makes the same load at the same step)
        let (recording, reads) = (self.recording.take(), self.replay_reads.take());
        let result = self.load_save_inner(save);
        self.recording = recording;
        self.replay_reads = reads;
        result
    }

    fn load_save_inner(&mut self, save: &Json) -> Result<Vec<u64>> {
        let mut next = self.clone();
        next.clear();
        let created = next.load_scene(save)?;
        if let Some(s) = save.get("save") {
            next.tick = s.get("tick").and_then(|t| t.as_u64()).unwrap_or(0);
            if let Some(list) = s.get("rng").and_then(|r| r.as_array()).filter(|l| l.len() == 4) {
                let mut state = [0u64; 4];
                for (slot, v) in state.iter_mut().zip(list) {
                    *slot = v.as_str().and_then(|h| u64::from_str_radix(h, 16).ok()).ok_or_else(|| EcsError::new("bad_save", "`save.rng` must be four hexadecimal strings").at("save.rng"))?;
                }
                if state != [0; 4] {
                    next.rng = Rng::from_state(state);
                }
            }
        }
        *self = next;
        Ok(created)
    }

    /// The resources `game.load` and `game.save` (a slot number, 1 to 99) ask for a saved game from inside the game: a graph system sets one
    /// of them, and at the end of the step the engine reads or writes `saves/slotN.json` of the workspace and sets the resource back to 0
    /// (`game.saved` / `game.loaded` get the tick, `game.error` 1 when it did not work).
    fn game_requests(&mut self) {
        let want = |s: &Sim, name: &str| s.world.resource(name).filter(|v| v.is_finite() && *v >= 1.0).map(|v| (v as u32).min(99));
        // `game.probe` = a slot number asks whether that slot has a saved game: the answer is the resource `game.has.<slot>` (1 or 0) at the end of the step
        // (a menu shows or greys its Continue button from it; it costs nothing until a game asks)
        if let Some(n) = want(self, "game.probe") {
            let exists = self.slot_exists(n);
            let _ = self.world.set_resource("game.probe", Some(0.0));
            let _ = self.world.set_resource(&format!("game.has.{n}"), Some(if exists { 1.0 } else { 0.0 }));
        }
        let (load, save) = (want(self, "game.load"), want(self, "game.save"));
        if load.is_none() && save.is_none() {
            return;
        }
        let _ = self.world.set_resource("game.load", Some(0.0));
        let _ = self.world.set_resource("game.save", Some(0.0));
        let Some(root) = self.asset_root.clone() else {
            let _ = self.world.set_resource("game.error", Some(1.0));
            return;
        };
        let mut failed = false;
        if let Some(n) = load {
            let loaded = self.slot_save(n, &root).map(|j| self.load_save(&j));
            match loaded {
                Some(Ok(_)) => {
                    let _ = self.world.set_resource("game.loaded", Some(self.tick as f64));
                }
                _ => failed = true,
            }
        }
        if let Some(n) = save {
            let dir = root.join("saves");
            let text = self.save_json().to_string_pretty().ok();
            // a replay does not write the files again (they are output: what the first run wrote is there already, and a replay of an old recording must not overwrite a newer save)
            let replaying = self.replay_reads.is_some();
            let ok = text.is_some_and(|t| replaying || (std::fs::create_dir_all(&dir).is_ok() && std::fs::write(dir.join(format!("slot{n}.json.tmp")), t).is_ok() && std::fs::rename(dir.join(format!("slot{n}.json.tmp")), dir.join(format!("slot{n}.json"))).is_ok()));
            if ok {
                let _ = self.world.set_resource("game.saved", Some(self.tick as f64));
            } else {
                failed = true;
            }
        }
        if failed {
            let _ = self.world.set_resource("game.error", Some(1.0));
        }
    }

    /// Does slot `n` hold a saved game? A recording keeps the answer, and a replay is given it again: what the files say is not part of the game's history.
    fn slot_exists(&mut self, n: u32) -> bool {
        if let Some(queue) = &mut self.replay_reads {
            if let Some(RecordedRead::Probe { slot, exists }) = queue.front() {
                if *slot == n {
                    let answer = *exists;
                    queue.pop_front();
                    return answer;
                }
            }
        }
        let exists = self.asset_root.as_ref().is_some_and(|r| r.join("saves").join(format!("slot{n}.json")).is_file());
        if let Some(rec) = &mut self.recording {
            rec.reads.push(RecordedRead::Probe { slot: n, exists });
        }
        exists
    }

    /// The saved game of slot `n`, read from `saves/` (or, in a replay, given back as it was the first time).
    fn slot_save(&mut self, n: u32, root: &std::path::Path) -> Option<Json> {
        if let Some(queue) = &mut self.replay_reads {
            if let Some(RecordedRead::Load { slot, save }) = queue.front() {
                if *slot == n {
                    let answer = save.clone();
                    queue.pop_front();
                    return answer;
                }
            }
        }
        let save = std::fs::read_to_string(root.join("saves").join(format!("slot{n}.json"))).ok().and_then(|t| lunaris_json::parse(&t).ok());
        if let Some(rec) = &mut self.recording {
            rec.reads.push(RecordedRead::Load { slot: n, save: save.clone() });
        }
        save
    }

    /// Load a scene produced by `scene_json` (entities, runtime components and graph systems).
    pub fn load_scene(&mut self, scene: &Json) -> Result<Vec<u64>> {
        let created = lunaris_ecs::load_scene_json(&mut self.world, scene)?;
        if let Some(defs) = scene.get("systems").and_then(|v| v.as_array()) {
            for (i, d) in defs.iter().enumerate() {
                let sys = graph::compile_with(&mut self.world, d, self.scripts.plugin_root.as_deref(), self.scripts.wasm.as_ref()).map_err(|e| e.at(format!("systems[{i}]")))?;
                self.scripts.put(sys);
            }
        }
        // prefabs, then the instances the scene places (`"instances": [{"prefab": "Tree", "at": [x, y, z], "overrides": {...}}]`)
        let mut created = created;
        if let Some(defs) = scene.get("prefabs") {
            let Json::Obj(defs) = defs else { return Err(EcsError::new("bad_scene", "`prefabs` must be an object of prefab definitions").at("prefabs")) };
            for (name, def) in defs {
                self.scripts.prefabs.insert(name.clone(), def.clone());
            }
            for (name, _) in defs {
                crate::prefab::validate(&self.world, &self.scripts.prefabs, name).map_err(|e| e.at(format!("prefabs.{name}")))?;
            }
        }
        if let Some(list) = scene.get("instances") {
            let Json::Arr(list) = list else { return Err(EcsError::new("bad_scene", "`instances` must be an array").at("instances")) };
            for (i, inst) in list.iter().enumerate() {
                let at = format!("instances[{i}]");
                let name = inst.get("prefab").and_then(|p| p.as_str()).ok_or_else(|| EcsError::new("bad_scene", "an instance needs the name of a `prefab`").at(at.clone()))?;
                let mut overrides: Vec<(String, Json)> = Vec::new();
                match inst.get("overrides") {
                    None | Some(Json::Null) => {}
                    Some(Json::Obj(o)) => {
                        for (c, v) in o {
                            crate::prefab::merge_component(&mut overrides, c, v);
                        }
                    }
                    Some(_) => return Err(EcsError::new("bad_scene", "`overrides` must be an object keyed by component name").at(at)),
                }
                if let Some(pos) = inst.get("at") {
                    crate::prefab::merge_component(&mut overrides, "Transform", &Json::obj([("pos", pos.clone())]));
                }
                let plan = crate::prefab::plan(&self.scripts.prefabs, name, &overrides).map_err(|e| e.at(at.clone()))?;
                let cmds = crate::prefab::spawn_commands(&plan, self.world.next_uid());
                for c in &cmds {
                    let applied = apply(&mut self.world, &mut self.scripts, c).map_err(|e| e.at(at.clone()))?;
                    created.extend(applied.uid);
                }
            }
        }
        systems::hierarchy(&mut self.world);
        Ok(created)
    }

    /// Empty the world and rewind to tick 0 with a freshly seeded RNG (systems and settings are kept).
    pub fn clear(&mut self) {
        self.world = World::new();
        register_standard_components(&mut self.world);
        self.tick = 0;
        self.rng = Rng::new(self.seed);
        self.scripts = ScriptSet { plugin_root: self.scripts.plugin_root.clone(), wasm: self.scripts.wasm.clone(), ..ScriptSet::default() };
        self.nav = Default::default();
        self.contacts.clear();
        self.char_contacts.clear();
        self.events.clear();
        self.recording = None;
    }

    pub fn add_system(&mut self, name: &str, f: impl Fn(&mut World, &mut SimCtx) + Send + Sync + 'static) {
        self.systems.push(SystemEntry { name: name.to_string(), run: Arc::new(f) });
    }

    pub fn system_names(&self) -> Vec<&str> {
        self.systems.iter().map(|s| s.name.as_str()).collect()
    }

    /// Advance one tick: run every system in order, then increment the tick counter.
    pub fn step(&mut self) {
        if let Some(rec) = &mut self.recording {
            rec.steps += 1;
        }
        let mut prof = self.profile.take();
        let tick_started = prof.is_some().then(std::time::Instant::now);
        // `stage!("name", work)` runs the work and, while profiling, adds its time under that name
        macro_rules! stage {
            ($name:expr, $body:expr) => {{
                match prof.as_mut() {
                    Some(p) => {
                        let t = std::time::Instant::now();
                        let r = $body;
                        p.add($name, t.elapsed().as_secs_f64() * 1e3);
                        r
                    }
                    None => $body,
                }
            }};
        }
        stage!("terrain loading", self.load_terrains());
        stage!("mesh loading", self.load_meshes());
        // slow motion, pause and fast forward: the resource `time.scale` (default 1, 0 to 8) multiplies the length of every step; at 0 nothing moves
        // but graph systems and the interface still run, so a pause menu can unpause
        let dt = self.dt * self.world.resource("time.scale").filter(|s| s.is_finite()).unwrap_or(1.0).clamp(0.0, 8.0) as f32;
        // bodies that float are pushed up by the water before the physics step moves them (a world without a `Buoyant` pays for one query)
        stage!("buoyancy", crate::water::Water::buoyancy(&mut self.world, &self.heightfields, dt));
        let mut ctx = SimCtx { tick: self.tick, threads: self.threads, gravity: [0.0, -9.8, 0.0], contacts: Vec::new(), dt, seed: self.seed, rng: self.rng.clone(), terrains: self.heightfields.clone(), meshes: self.meshes.clone() };
        for s in &self.systems {
            match prof.as_mut() {
                Some(p) => {
                    let t = std::time::Instant::now();
                    (s.run)(&mut self.world, &mut ctx);
                    p.add(&format!("system {}", s.name), t.elapsed().as_secs_f64() * 1e3);
                }
                None => (s.run)(&mut self.world, &mut ctx),
            }
        }
        self.rng = ctx.rng;
        self.contacts = std::mem::take(&mut ctx.contacts);
        // what the characters touched at the end of the last tick (`CharacterMotion.report_contacts`) is handed to this tick's graph systems like any contact
        self.contacts.append(&mut self.char_contacts);
        stage!("event resources", crate::gameplay::publish_events(&mut self.world, &self.events, self.tick));
        stage!("input actions", crate::gameplay::actions(&mut self.world));
        stage!("interface widgets", crate::ui_kit::update(&mut self.world, self.tick, self.dt));
        stage!("graph systems (all)", graph::run_all(&mut self.world, &mut self.scripts, &self.contacts, self.tick, dt, self.seed, self.threads));
        stage!("language", crate::language::update(&mut self.world));
        if let Some(p) = prof.as_mut() {
            for s in self.scripts.systems.iter().filter(|s| s.enabled) {
                p.add(&format!("graph system {}", s.name), s.last_ms);
            }
        }
        // navigation agents choose their walk (after the graph systems, which may give them targets), then characters walk with this
        // tick's input, then attached entities follow their parents after everything else has moved them
        stage!("navigation", crate::nav::step(&mut self.world, &mut self.nav, dt, &self.heightfields, &self.meshes));
        stage!("characters", {
            self.char_contacts = crate::character::step(&mut self.world, dt, &self.heightfields, &self.meshes);
        });
        // the waves' clock, and `InWater` on the characters that now stand in a water body
        stage!("water", {
            crate::water::Water::waves(&mut self.world, dt);
            crate::water::Water::swimmers(&mut self.world, &self.heightfields);
        });
        stage!("gameplay blocks", crate::gameplay::update(&mut self.world, &mut self.scripts, self.tick, dt, &self.heightfields, &self.meshes));
        stage!("hierarchy", systems::hierarchy(&mut self.world));
        let tick = self.tick;
        self.events.extend(self.scripts.events.drain(..).map(|(name, uid, pos)| Event { tick, name, uid, pos }));
        if self.events.len() > MAX_EVENTS {
            self.events.drain(..self.events.len() - MAX_EVENTS);
        }
        self.tick += 1;
        stage!("game requests", self.game_requests());
        stage!("scene requests", self.scene_requests());
        if let (Some(mut p), Some(t0)) = (prof, tick_started) {
            let ms = t0.elapsed().as_secs_f64() * 1e3;
            p.ticks += 1;
            p.total_ms += ms;
            p.max_ms = p.max_ms.max(ms);
            self.profile = Some(p);
        }
    }

    pub fn step_n(&mut self, n: u64) {
        for _ in 0..n {
            self.step();
        }
    }

    /// The pictures that the `Terrain` and `WaterBody` entities of the world name (sorted, each once): heightmaps, and the heights of the water.
    pub fn used_terrains(&self) -> Vec<String> {
        let mut syms: Vec<u32> = self.world.entities_by_uid().iter().filter_map(|e| self.world.get::<Terrain>(*e)).map(|t| t.heightmap.0).collect();
        syms.extend(self.world.entities_by_uid().iter().filter_map(|e| self.world.get::<WaterBody>(*e)).map(|w| w.heightmap.0));
        self.terrain_names(syms)
    }

    fn terrain_names(&self, syms: Vec<u32>) -> Vec<String> {
        let mut names: Vec<String> = syms.into_iter().filter(|s| *s != 0).map(|s| self.world.strings().get(s).to_string()).filter(|n| !n.is_empty()).collect();
        names.sort();
        names.dedup();
        names
    }

    /// Read the heightmaps that terrain names and that are not read yet. A heightmap that fails is tried again every 300 ticks.
    pub fn load_terrains(&mut self) {
        let Some(root) = self.asset_root.clone() else { return };
        // (a query over the archetypes that have a Terrain: free when there is none, however many entities there are)
        let mut syms: Vec<u32> = Vec::new();
        let _ = self.world.for_each_uid::<(&Terrain,), _>(|_, _, (t,)| syms.push(t.heightmap.0));
        let _ = self.world.for_each_uid::<(&WaterBody,), _>(|_, _, (w,)| syms.push(w.heightmap.0));
        for name in self.terrain_names(syms) {
            if self.heightfields.contains_key(&name) || (self.terrain_errors.contains_key(&name) && self.tick % 300 != 0) {
                continue;
            }
            match crate::terrain::Heightfield::read(&root, &name) {
                Ok(h) => {
                    Arc::make_mut(&mut self.heightfields).insert(name.clone(), Arc::new(h));
                    self.terrain_errors.remove(&name);
                }
                Err(e) => {
                    self.terrain_errors.insert(name, e);
                }
            }
        }
    }

    /// The models that the `MeshCollider` and `ConvexCollider` entities of the world name (sorted, each once).
    pub fn used_meshes(&self) -> Vec<String> {
        let mut syms: Vec<u32> = Vec::new();
        for e in self.world.entities_by_uid() {
            syms.extend(self.world.get::<MeshCollider>(e).map(|m| m.mesh.0));
            syms.extend(self.world.get::<ConvexCollider>(e).map(|m| m.mesh.0));
        }
        self.terrain_names(syms)
    }

    /// Read the models that `MeshCollider`s name and that are not read yet, and read again the ones whose file changed (looked at every
    /// [`MESH_RECHECK`] ticks). A model that fails is tried again every 300 ticks; one that was fine and breaks keeps its last good copy
    /// (it may be half written: the next change reads it again). Free when the world has no `MeshCollider`.
    pub fn load_meshes(&mut self) {
        let Some(root) = self.asset_root.clone() else { return };
        let mut syms: Vec<u32> = Vec::new();
        let _ = self.world.for_each_uid::<(&MeshCollider,), _>(|_, _, (m,)| syms.push(m.mesh.0));
        let _ = self.world.for_each_uid::<(&ConvexCollider,), _>(|_, _, (m,)| syms.push(m.mesh.0));
        if syms.is_empty() {
            return;
        }
        for name in self.terrain_names(syms) {
            let loaded = self.meshes.by_name.contains_key(&name);
            if loaded && self.tick % MESH_RECHECK != 0 {
                continue;
            }
            if !loaded && self.mesh_errors.contains_key(&name) && self.tick % 300 != 0 {
                continue;
            }
            let stamp = (|| {
                let rel = std::path::Path::new(&name);
                if rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
                    return None;
                }
                let meta = std::fs::metadata(root.join(rel)).ok()?;
                Some((meta.modified().ok(), meta.len()))
            })();
            if loaded && stamp.is_some_and(|s| self.mesh_stamps.get(&name) == Some(&s)) {
                continue; // unchanged
            }
            if let Some(s) = stamp {
                self.mesh_stamps.insert(name.clone(), s);
            }
            match crate::meshcol::CollisionMesh::read(&root, &name) {
                Ok(m) => {
                    Arc::make_mut(&mut self.meshes).by_name.insert(name.clone(), Arc::new(m));
                    self.mesh_errors.remove(&name);
                }
                Err(e) => {
                    let why = if loaded { format!("it changed on disk but cannot be read now ({e}); the old copy stays") } else { e };
                    self.mesh_errors.insert(name, why);
                }
            }
        }
    }

    /// Hash of everything that decides the future: the world, the tick and the RNG state.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::default();
        h.u64(self.world.state_hash());
        // the pictures that the terrain stands on: a replay with a different heightmap must not look the same
        for name in self.used_terrains() {
            h.bytes(name.as_bytes());
            h.u64(self.heightfields.get(&name).map(|f| f.hash).unwrap_or(0));
        }
        // and the models that mesh colliders stand on (nothing is added to the hash of a world without any)
        for name in self.used_meshes() {
            h.bytes(name.as_bytes());
            h.u64(self.meshes.by_name.get(&name).map(|m| m.hash).unwrap_or(0));
        }
        h.u64(self.tick);
        h.u64(self.seed);
        h.u64(self.dt.to_bits() as u64);
        for w in self.rng.state() {
            h.u64(w);
        }
        self.scripts.hash_into(&mut h);
        self.nav.hash_into(&mut h);
        h.finish()
    }

    // ------------------------------------------------------------------------------------------ checkpoints (rollback netcode)
    /// A number that changes when the *set* of graph systems or prefabs changes (a system added, removed, switched on or off, re-ordered).
    /// A `Checkpoint` does not copy the systems (they never change while a game runs, and copying their definitions is most of what a
    /// whole-`Sim` clone costs), so this stamp is how `rewind_to` notices the one case it cannot undo.
    fn script_stamp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: u64| h = (h ^ v).wrapping_mul(0x0000_0100_0000_01b3);
        mix(self.scripts.systems.len() as u64);
        for s in &self.scripts.systems {
            mix(s.name.len() as u64);
            mix(s.enabled as u64);
            mix(s.order as u64);
            mix(s.identity() as u64); // a system defined again has a new compiled form, even if its text is the same
        }
        mix(self.scripts.prefabs.len() as u64);
        h
    }

    /// A cheap copy of what decides the game's future, to come back to with `rewind_to`: the world (entities, resources, strings), the tick,
    /// the random generator, navigation paths and behaviour-tree memory. It leaves out what a running game never changes (the systems, the
    /// prefabs, the settings), what is derived (contacts, heightmaps read from files) and what only leaves the game (events, the recording),
    /// so it costs about half of `clone()` and, unlike it, can be taken on every tick. Rollback netcode keeps one per tick.
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint { tick: self.tick, world: self.world.clone(), rng: self.rng.clone(), nav: self.nav.clone(), bt: self.scripts.bt.clone(), stamp: self.script_stamp() }
    }

    /// Put the simulation back exactly as it was when `c` was taken: the state hash is the same as it was then, and stepping on gives the
    /// same history. Events of the ticks that are taken back are forgotten (the replay emits them again). An error leaves the simulation
    /// untouched; it means the systems or prefabs changed since the checkpoint, which a checkpoint cannot undo.
    pub fn rewind_to(&mut self, c: &Checkpoint) -> Result<()> {
        if self.script_stamp() != c.stamp {
            return Err(EcsError::new("rewind_unsupported", "the game's systems or prefabs changed since the checkpoint was taken").hint("a checkpoint does not copy systems; use snapshot/restore (a whole clone) when systems are edited while the game runs"));
        }
        self.world = c.world.clone();
        self.tick = c.tick;
        self.rng = c.rng.clone();
        self.nav = c.nav.clone();
        self.scripts.bt = c.bt.clone();
        self.scripts.events.clear();
        self.contacts.clear();
        self.char_contacts.clear();
        self.events.retain(|e| e.tick < c.tick);
        Ok(())
    }

    // ------------------------------------------------------------------------------------------ commands
    pub fn apply(&mut self, cmd: &Command) -> Result<Applied> {
        let applied = apply(&mut self.world, &mut self.scripts, cmd)?;
        if let Some(rec) = &mut self.recording {
            rec.entries.push((self.tick, applied.executed.clone()));
        }
        Ok(applied)
    }

    /// Apply several commands atomically (all or nothing); on success they are logged to the recording as a group.
    pub fn apply_batch(&mut self, cmds: &[Command]) -> Result<(Vec<Applied>, Option<Vec<Command>>)> {
        let (done, inverse) = crate::command::apply_batch(&mut self.world, &mut self.scripts, cmds)?;
        if let Some(rec) = &mut self.recording {
            for a in &done {
                rec.entries.push((self.tick, a.executed.clone()));
            }
        }
        Ok((done, inverse))
    }

    // ------------------------------------------------------------------------------------------ recording and replay
    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    pub fn start_recording(&mut self) -> Result<()> {
        if self.recording.is_some() {
            return Err(EcsError::new("already_recording", "a recording is already in progress"));
        }
        self.recording = Some(Recording {
            seed: self.seed,
            dt: self.dt,
            start_tick: self.tick,
            start_scene: self.scene_json(),
            start_rng: self.rng.state(),
            entries: Vec::new(),
            final_tick: self.tick,
            final_hash: 0,
            steps: 0,
            reads: Vec::new(),
        });
        Ok(())
    }

    pub fn stop_recording(&mut self) -> Result<Recording> {
        let mut rec = self.recording.take().ok_or_else(|| EcsError::new("not_recording", "no recording is in progress").hint("call start_recording first"))?;
        rec.final_tick = self.tick;
        rec.final_hash = self.state_hash();
        Ok(rec)
    }

    /// Rebuild a simulation from a recording and play it forward; the result must have `rec.final_hash`.
    /// `template` supplies the systems (code is not stored in recordings).
    pub fn replay(template: &Sim, rec: &Recording) -> Result<Sim> {
        let mut sim = template.clone();
        sim.recording = None;
        sim.events.clear(); // the replay makes its own
        sim.world = World::new();
        sim.scripts = ScriptSet { plugin_root: template.scripts.plugin_root.clone(), wasm: template.scripts.wasm.clone(), ..ScriptSet::default() };
        sim.nav = Default::default();
        register_standard_components(&mut sim.world);
        // native components the template registered beyond the standard set must exist before the scene loads
        for info in template.world.infos().iter().filter(|i| i.native) {
            if sim.world.component_id(&info.name).is_none() {
                return Err(EcsError::new("replay_unsupported", format!("template has native component {} that the replay world cannot create", info.name)));
            }
        }
        sim.load_scene(&rec.start_scene)?;
        sim.tick = rec.start_tick;
        sim.seed = rec.seed;
        sim.dt = rec.dt;
        sim.rng = Rng::from_state(rec.start_rng);
        sim.replay_reads = Some(rec.reads.iter().cloned().collect());
        let mut next = 0usize;
        for _ in 0..rec.steps {
            while next < rec.entries.len() && rec.entries[next].0 == sim.tick {
                apply(&mut sim.world, &mut sim.scripts, &rec.entries[next].1)?;
                next += 1;
            }
            sim.step();
        }
        while next < rec.entries.len() {
            apply(&mut sim.world, &mut sim.scripts, &rec.entries[next].1)?;
            next += 1;
        }
        sim.replay_reads = None;
        Ok(sim)
    }
}

/// Default worker thread count: half the logical cores (at most 8) so a development machine stays responsive;
/// `LUNARIS_THREADS` overrides it.
pub fn default_threads() -> usize {
    if let Some(n) = std::env::var("LUNARIS_THREADS").ok().and_then(|v| v.parse::<usize>().ok()) {
        return n.clamp(1, 64);
    }
    (std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2) / 2).clamp(1, 8)
}

pub fn register_standard_components(world: &mut World) {
    world.register_native::<Name>().expect("standard component");
    world.register_native::<Transform>().expect("standard component");
    world.register_native::<Parent>().expect("standard component");
    world.register_native::<Emitter>().expect("standard component");
    world.register_native::<Environment>().expect("standard component");
    world.register_native::<Animator>().expect("standard component");
    world.register_native::<CharacterBody>().expect("standard component");
    world.register_native::<NavAgent>().expect("standard component");
    world.register_native::<Ui>().expect("standard component");
    world.register_native::<Particle>().expect("standard component");
    world.register_native::<LocalTransform>().expect("standard component");
    world.register_native::<Velocity>().expect("standard component");
    world.register_native::<Wander>().expect("standard component");
    world.register_native::<Camera>().expect("standard component");
    world.register_native::<MeshRenderer>().expect("standard component");
    world.register_native::<Sprite>().expect("standard component");
    world.register_native::<Light>().expect("standard component");
    world.register_native::<Text>().expect("standard component");
    world.register_native::<Sound>().expect("standard component");
    world.register_native::<SoundFile>().expect("standard component");
    world.register_native::<AudioSource>().expect("standard component");
    world.register_native::<AcousticMaterial>().expect("standard component");
    world.register_native::<Tilemap>().expect("standard component");
    world.register_native::<Collider>().expect("standard component");
    world.register_native::<Terrain>().expect("standard component");
    world.register_native::<crate::components::Joint>().expect("standard component");
    world.register_native::<crate::components::InputMap>().expect("standard component");
    world.register_native::<crate::components::StateMachine>().expect("standard component");
    world.register_native::<crate::components::Tween>().expect("standard component");
    world.register_native::<crate::components::CameraRig>().expect("standard component");
    world.register_native::<crate::components::Sensor>().expect("standard component");
    world.register_native::<crate::components::Lifetime>().expect("standard component");
    world.register_native::<crate::components::AudioMixer>().expect("standard component");
    world.register_native::<crate::components::UiStyle>().expect("standard component");
    world.register_native::<crate::components::MusicPlayer>().expect("standard component");
    world.register_native::<crate::components::Ik>().expect("standard component");
    world.register_native::<crate::components::Particles>().expect("standard component");
    world.register_native::<crate::components::AnimEvents>().expect("standard component");
    world.register_native::<crate::components::SpriteAnim>().expect("standard component");
    world.register_native::<crate::components::MouseLook>().expect("standard component");
    world.register_native::<crate::components::Steer>().expect("standard component");
    world.register_native::<crate::components::ColorGrade>().expect("standard component");
    world.register_native::<crate::components::SceneSwitch>().expect("standard component");
    world.register_native::<crate::components::Persist>().expect("standard component");
    world.register_native::<crate::components::HeightFog>().expect("standard component");
    world.register_native::<crate::components::ParticleModules>().expect("standard component");
    world.register_native::<crate::components::CameraView>().expect("standard component");
    world.register_native::<crate::components::Dialogue>().expect("standard component");
    world.register_native::<crate::components::Parallax>().expect("standard component");
    world.register_native::<crate::components::NavObstacle>().expect("standard component");
    world.register_native::<crate::components::Decal>().expect("standard component");
    world.register_native::<crate::components::EventWatch>().expect("standard component");
    world.register_native::<crate::components::RootMotion>().expect("standard component");
    world.register_native::<crate::components::TextFont>().expect("standard component");
    world.register_native::<crate::components::UiAnchor>().expect("standard component");
    world.register_native::<crate::components::Lod>().expect("standard component");
    world.register_native::<crate::components::Sway>().expect("standard component");
    world.register_native::<crate::components::Water>().expect("standard component");
    world.register_native::<crate::components::PostFx>().expect("standard component");
    world.register_native::<crate::components::BehaviorTree>().expect("standard component");
    world.register_native::<crate::components::HitPoints>().expect("standard component");
    world.register_native::<crate::components::StreamZone>().expect("standard component");
    world.register_native::<crate::components::EmitterFx>().expect("standard component");
    world.register_native::<crate::components::ParticleFx>().expect("standard component");
    world.register_native::<crate::components::Spline>().expect("standard component");
    world.register_native::<crate::components::PathFollower>().expect("standard component");
    world.register_native::<crate::components::Timeline>().expect("standard component");
    world.register_native::<crate::components::UiSlider>().expect("standard component");
    world.register_native::<crate::components::UiToggle>().expect("standard component");
    world.register_native::<crate::components::UiInput>().expect("standard component");
    world.register_native::<crate::components::UiLayout>().expect("standard component");
    world.register_native::<crate::components::UiNav>().expect("standard component");
    world.register_native::<crate::components::AnimLayers>().expect("standard component");
    world.register_native::<crate::components::NavAvoid>().expect("standard component");
    world.register_native::<crate::components::NavLink>().expect("standard component");
    world.register_native::<crate::components::Localization>().expect("standard component");
    world.register_native::<crate::components::SpawnedBy>().expect("standard component");
    world.register_native::<crate::components::PrefabSpawner>().expect("standard component");
    world.register_native::<crate::components::TimeOfDay>().expect("standard component");
    world.register_native::<RigidBody>().expect("standard component");
    world.register_native::<PhysicsSettings>().expect("standard component");
    world.register_native::<AngularVelocity>().expect("standard component");
    world.register_native::<MeshCollider>().expect("standard component");
    world.register_native::<ConvexCollider>().expect("standard component");
    world.register_native::<CollisionLayer>().expect("standard component");
    world.register_native::<Vehicle>().expect("standard component");
    world.register_native::<crate::components::DrawLayer>().expect("standard component");
    world.register_native::<crate::components::LanguageList>().expect("standard component");
    world.register_native::<crate::components::UiNavAuto>().expect("standard component");
    world.register_native::<crate::components::SceneReset>().expect("standard component");
    world.register_native::<crate::components::UiScale>().expect("standard component");
    world.register_native::<crate::components::UiEdge>().expect("standard component");
    world.register_native::<crate::components::UiScroll>().expect("standard component");
    world.register_native::<crate::components::UiScrollBar>().expect("standard component");
    world.register_native::<crate::components::UiGroup>().expect("standard component");
    world.register_native::<crate::components::UiDropdown>().expect("standard component");
    world.register_native::<crate::components::UiOption>().expect("standard component");
    world.register_native::<crate::components::UiTabs>().expect("standard component");
    world.register_native::<crate::components::UiTab>().expect("standard component");
    world.register_native::<crate::components::UiDrag>().expect("standard component");
    world.register_native::<crate::components::UiDrop>().expect("standard component");
    world.register_native::<crate::components::UiTooltip>().expect("standard component");
    world.register_native::<crate::components::UiTooltipBox>().expect("standard component");
    world.register_native::<crate::components::UiRadial>().expect("standard component");
    world.register_native::<crate::components::TerrainLayers>().expect("standard component");
    world.register_native::<crate::components::WaterBody>().expect("standard component");
    world.register_native::<crate::components::InWater>().expect("standard component");
    world.register_native::<crate::components::Buoyant>().expect("standard component");
    world.register_native::<crate::components::AutoExposure>().expect("standard component");
    world.register_native::<crate::components::Ssr>().expect("standard component");
    world.register_native::<crate::components::LensFlare>().expect("standard component");
    world.register_native::<crate::components::ReflectionProbe>().expect("standard component");
    world.register_native::<crate::components::CameraCollide>().expect("standard component");
    world.register_native::<crate::components::CharacterMotion>().expect("standard component");
    world.register_native::<crate::components::Climbable>().expect("standard component");
    world.register_native::<crate::components::Probe>().expect("standard component");
    world.register_native::<crate::components::BoneAttach>().expect("standard component");
    world.register_native::<crate::components::PbrMaps>().expect("standard component");
    world.register_native::<crate::components::Lens>().expect("standard component");
}

impl RecordedRead {
    fn to_json(&self) -> Json {
        match self {
            RecordedRead::Probe { slot, exists } => Json::obj([("probe", Json::from(*slot as i64)), ("exists", Json::Bool(*exists))]),
            RecordedRead::Load { slot, save } => Json::obj([("load", Json::from(*slot as i64)), ("save", save.clone().unwrap_or(Json::Null))]),
        }
    }

    fn from_json(j: &Json) -> Option<RecordedRead> {
        if let Some(slot) = j.get("probe").and_then(|v| v.as_u64()) {
            return Some(RecordedRead::Probe { slot: slot as u32, exists: j.get("exists").and_then(|v| v.as_bool()).unwrap_or(false) });
        }
        let slot = j.get("load").and_then(|v| v.as_u64())?;
        Some(RecordedRead::Load { slot: slot as u32, save: j.get("save").filter(|s| !matches!(s, Json::Null)).cloned() })
    }
}

impl Recording {
    pub fn to_json(&self) -> Json {
        let mut out = self.to_json_base();
        if !self.reads.is_empty() {
            out.set("reads", Json::arr(self.reads.iter().map(RecordedRead::to_json)));
        }
        out
    }

    fn to_json_base(&self) -> Json {
        Json::obj([
            ("lunaris_recording", Json::from(1)),
            ("seed", Json::str(format!("{:016x}", self.seed))),
            ("dt", Json::from(self.dt as f64)),
            ("start_tick", Json::from(self.start_tick as i64)),
            ("start_rng", Json::arr(self.start_rng.iter().map(|w| Json::str(format!("{w:016x}"))))),
            ("start_scene", self.start_scene.clone()),
            ("entries", Json::arr(self.entries.iter().map(|(t, c)| Json::obj([("tick", Json::from(*t as i64)), ("command", c.to_json())])))),
            ("final_tick", Json::from(self.final_tick as i64)),
            ("final_hash", Json::str(format!("{:016x}", self.final_hash))),
            ("steps", Json::from(self.steps as i64)),
        ])
    }

    pub fn from_json(j: &Json) -> Result<Recording> {
        let bad = |m: &str| EcsError::new("bad_recording", m.to_string());
        let hex = |v: Option<&Json>| -> Result<u64> {
            v.and_then(|x| x.as_str()).and_then(|s| u64::from_str_radix(s, 16).ok()).ok_or_else(|| bad("expected a 16-digit hex string"))
        };
        if j.get("lunaris_recording").and_then(|v| v.as_i64()) != Some(1) {
            return Err(bad("not a Lunaris recording (version 1)"));
        }
        let rng = j.get("start_rng").and_then(|v| v.as_array()).ok_or_else(|| bad("missing start_rng"))?;
        if rng.len() != 4 {
            return Err(bad("start_rng needs 4 words"));
        }
        let mut start_rng = [0u64; 4];
        for (i, w) in rng.iter().enumerate() {
            start_rng[i] = hex(Some(w))?;
        }
        let mut entries = Vec::new();
        for e in j.get("entries").and_then(|v| v.as_array()).ok_or_else(|| bad("missing entries"))? {
            let tick = e.get("tick").and_then(|v| v.as_u64()).ok_or_else(|| bad("entry without tick"))?;
            entries.push((tick, Command::from_json(e.get("command").ok_or_else(|| bad("entry without command"))?)?));
        }
        let start_tick = j.get("start_tick").and_then(|v| v.as_u64()).ok_or_else(|| bad("missing start_tick"))?;
        let final_tick = j.get("final_tick").and_then(|v| v.as_u64()).ok_or_else(|| bad("missing final_tick"))?;
        Ok(Recording {
            seed: hex(j.get("seed"))?,
            dt: j.get("dt").and_then(|v| v.as_f64()).ok_or_else(|| bad("missing dt"))? as f32,
            start_tick,
            start_scene: j.get("start_scene").cloned().ok_or_else(|| bad("missing start_scene"))?,
            start_rng,
            entries,
            final_tick,
            final_hash: hex(j.get("final_hash"))?,
            // recordings written before `steps` existed never went back in time
            steps: j.get("steps").and_then(|v| v.as_u64()).unwrap_or(final_tick.saturating_sub(start_tick)),
            reads: j.get("reads").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(RecordedRead::from_json).collect()).unwrap_or_default(),
        })
    }
}
