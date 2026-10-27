//! The editor's model: the engine it edits, what is selected, what the hierarchy looks like, the console, and play mode.
//!
//! The editor never touches the world except through the engine's tools (`spawn`, `set`, `despawn`, `snapshot`, ...), exactly as an AI agent
//! or the command line does. That is what makes undo, recording and the MCP server see everything the editor does.
use crate::mathx::{q_conj, q_rotate, sub, V3};
use lunaris_api::Engine;
use lunaris_json::Json;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

/// What was asked on the command line.
#[derive(Clone, Debug)]
pub struct Options {
    pub scene: Option<String>,
    pub root: PathBuf,
    pub seed: u64,
    pub size: (f32, f32),
    /// `--size` was given: do not open maximised.
    pub size_given: bool,
    pub quality: String,
    /// Save a picture of the window to this file and quit (for tests and documentation).
    pub shot: Option<PathBuf>,
    pub shot_delay: f32,
    /// Select the entity with this name (or uid) at start.
    pub select: Option<String>,
    /// Start playing at once.
    pub play: bool,
    pub mute: bool,
    /// Which bottom tab and which view to start with (for pictures): `project`, `console`, `systems`; `scene`, `2d`, `game`.
    pub tab: Option<String>,
    pub view: Option<String>,
    /// Open this canvas file (`*.canvas`) in the Canvas tab at start.
    pub canvas: Option<String>,
    /// Open this graph system in the Systems tab at start.
    pub system: Option<String>,
    /// Build the scene as a game (the program's path) without opening a window.
    pub build: Option<PathBuf>,
    /// Open this script file in the Script tab at start.
    pub script: Option<String>,
}

impl Default for Options {
    fn default() -> Options {
        Options { scene: None, root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")), seed: 1, size: (1600.0, 900.0), size_given: false, quality: "high".to_string(), shot: None, shot_delay: 2.0, select: None, play: false, mute: false, tab: None, view: None, system: None, build: None, script: None, canvas: None }
    }
}

/// What the middle of the window shows: the scene and game views, or one of the editors.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Center {
    View,
    Ui,
    Blueprint,
    Script,
    Canvas,
    Sequencer,
    Effects,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Edit,
    Play,
    Paused,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogKind {
    Call,
    Error,
    Warn,
    Info,
}

pub struct LogLine {
    pub kind: LogKind,
    pub text: String,
    pub detail: String,
    pub ms: f32,
}

/// One line of the hierarchy.
#[derive(Clone, Debug)]
pub struct Row {
    pub uid: u64,
    pub name: String,
    pub parent: Option<u64>,
    pub depth: usize,
    pub children: usize,
    /// What it is, for the little icon: the first of Camera, Light, MeshRenderer, Sprite, ...
    pub kind: Kind,
    pub pos: Option<V3>,
    /// Does the entity itself have a MeshRenderer?
    pub own_mesh: bool,
    /// How many MeshRenderers there are in this row and everything attached under it, and how many of those are shown (the Outliner's eye).
    pub meshes: u32,
    pub shown: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Empty,
    Camera,
    Light,
    Mesh,
    Sprite,
    Text,
    Ui,
    Emitter,
    Audio,
    Collider,
    Tilemap,
    Environment,
}

impl Kind {
    pub fn letter(self) -> &'static str {
        match self {
            Kind::Empty => "o",
            Kind::Camera => "C",
            Kind::Light => "L",
            Kind::Mesh => "M",
            Kind::Sprite => "S",
            Kind::Text => "T",
            Kind::Ui => "U",
            Kind::Emitter => "P",
            Kind::Audio => "A",
            Kind::Collider => "B",
            Kind::Tilemap => "G",
            Kind::Environment => "E",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FieldDef {
    pub name: String,
    pub ty: String,
    pub count: usize,
}

#[derive(Clone, Debug, Default)]
pub struct CompDef {
    /// The values a new component of this kind starts with, by field (`schema`'s `defaults`).
    pub defaults: Option<Json>,
    pub name: String,
    pub doc: String,
    pub fields: Vec<FieldDef>,
}

/// The selected entity as the engine last reported it.
#[derive(Clone, Debug)]
pub struct Entity {
    pub uid: u64,
    pub comps: Vec<(String, Json)>,
}

impl Entity {
    pub fn comp(&self, name: &str) -> Option<&Json> {
        self.comps.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
    pub fn vec3(&self, comp: &str, field: &str) -> Option<V3> {
        let a = self.comp(comp)?.get(field)?.as_array()?;
        (a.len() >= 3).then(|| [a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32, a[2].as_f64().unwrap_or(0.0) as f32])
    }
    pub fn quat(&self, comp: &str, field: &str) -> Option<[f32; 4]> {
        let a = self.comp(comp)?.get(field)?.as_array()?;
        (a.len() == 4).then(|| [a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32, a[2].as_f64().unwrap_or(0.0) as f32, a[3].as_f64().unwrap_or(1.0) as f32])
    }
    pub fn name(&self) -> String {
        self.comp("Name").and_then(|n| n.get("value")).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| format!("Entity {}", self.uid))
    }
    /// The parent when the entity is attached to one (it has `Parent` and `LocalTransform`).
    pub fn parent(&self) -> Option<u64> {
        let p = self.comp("Parent")?.get("target")?.as_u64()?;
        (p != 0 && self.comp("LocalTransform").is_some()).then_some(p)
    }
}

/// The tabs of the panel on the left of the scene view.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeftTab {
    Hierarchy,
    Place,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BottomTab {
    Project,
    Console,
    Systems,
    Profiler,
    History,
}

const READ_ONLY: &[&str] = &[
    "describe", "schema", "get", "query", "hash", "revision", "history", "snapshots", "events", "resources", "systems", "get_system", "contacts", "raycast", "nearby", "render", "pick", "minimap", "check", "guide", "project", "files", "model_info",
    "prefabs", "path", "acoustics", "compile_script", "get_script", "terrain_height", "terrain_layer", "water_depth", "saves", "bake_probe",
];

/// Calls that change the play session, not the scene: they never make the scene "modified".
const SESSION_ONLY: &[&str] = &["step", "input", "snapshot", "restore", "delete_snapshot", "record_start", "record_stop", "replay", "record_audio", "load_scene", "reset"];

const MAX_LOG: usize = 400;
const MAX_ROWS: usize = 10_000;
const EDIT_SNAPSHOT: &str = "__editor_before_play__";

pub struct Studio {
    pub engine: Engine,
    pub opts: Options,
    pub scene: Option<String>,
    pub dirty: bool,
    /// The main selected entity (the Inspector shows it, the handles are on it): the one selected last.
    pub selection: Option<u64>,
    /// Every selected entity, the main one last (empty when nothing is selected); Ctrl+click adds and removes, Shift+click in the Hierarchy takes a range.
    pub selected: Vec<u64>,
    pub entity: Option<Entity>,
    pub mode: Mode,
    pub console: Vec<LogLine>,
    pub rows: Vec<Row>,
    pub total_entities: usize,
    pub all: Vec<Row>,
    pub expanded: HashSet<u64>,
    pub filter: String,
    pub comp_names: Vec<String>,
    comps: BTreeMap<String, CompDef>,
    pub revision: u64,
    /// The engine's revision as of the last frame (the one the scene view redraws on); `revision` is the one the hierarchy was built at.
    pub live_revision: u64,
    pub tick: u64,
    pub toast: Option<(String, Instant, bool)>,
    pub tab: BottomTab,
    /// The tab of the left panel that shows while the scene view is in the middle.
    pub left_tab: LeftTab,
    pub center: Center,
    /// A panel asks the window to open this graph system in the Script tab.
    pub open_script: Option<String>,
    /// A panel asks the window to open this graph system in the Blueprint tab.
    pub open_blueprint: Option<String>,
    pub undo_group: u64,
    pub command_line: String,
    pub files: Vec<(String, String, u64)>,
    pub files_dir: String,
    pub files_stamp: Option<Instant>,
    pub problems: Vec<(String, String)>,
    pub systems: Vec<Json>,
    pub selected_system: Option<String>,
    pub system_text: String,
    pub system_error: Option<String>,
    pub prefabs: Vec<String>,
    /// The brush of Paint mode while one is armed (the scene view sprinkles copies of it where the button is dragged).
    pub brush: Option<crate::paint::Brush>,
    /// The values of a component that the Inspector copied (its name and its fields as the engine gave them), for Paste.
    pub component_clipboard: Option<(String, Json)>,
    pub quit: bool,
    next_refresh: Option<Instant>,
    /// When the scene file had the modification time we know of (what we loaded or saved), and when to look again.
    scene_stamp: Option<std::time::SystemTime>,
    next_disk_check: Option<Instant>,
    pub rows_dirty: bool,
}

impl Studio {
    pub fn new(opts: Options) -> Studio {
        let engine = Engine::new(opts.seed, opts.root.clone());
        let mut s = Studio {
            engine,
            opts,
            scene: None,
            dirty: false,
            selection: None,
            selected: Vec::new(),
            entity: None,
            mode: Mode::Edit,
            console: Vec::new(),
            rows: Vec::new(),
            total_entities: 0,
            all: Vec::new(),
            expanded: HashSet::new(),
            filter: String::new(),
            comp_names: Vec::new(),
            comps: BTreeMap::new(),
            revision: u64::MAX,
            live_revision: 0,
            tick: 0,
            toast: None,
            tab: BottomTab::Project,
            left_tab: LeftTab::Hierarchy,
            center: Center::View,
            open_script: None,
            open_blueprint: None,
            undo_group: 0,
            command_line: String::new(),
            files: Vec::new(),
            files_dir: String::new(),
            files_stamp: None,
            problems: Vec::new(),
            systems: Vec::new(),
            selected_system: None,
            system_text: String::new(),
            system_error: None,
            prefabs: Vec::new(),
            brush: None,
            component_clipboard: None,
            quit: false,
            next_refresh: None,
            scene_stamp: None,
            next_disk_check: None,
            rows_dirty: true,
        };
        if let Some(p) = s.opts.scene.clone() {
            s.open_scene(&p);
        }
        s.log(LogKind::Info, format!("Lunaris Editor, workspace {}", s.opts.root.display()), String::new(), 0.0);
        s
    }

    // ------------------------------------------------------------------------------------------------------------- calls

    pub fn log(&mut self, kind: LogKind, text: String, detail: String, ms: f32) {
        self.console.push(LogLine { kind, text, detail, ms });
        if self.console.len() > MAX_LOG {
            self.console.drain(..self.console.len() - MAX_LOG);
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toast = Some((text.into(), Instant::now(), error));
    }

    /// Call a tool. Calls that change the world are written to the console; failures always are.
    pub fn call(&mut self, tool: &str, args: Json) -> Result<Json, String> {
        let t0 = Instant::now();
        let r = self.engine.call(tool, &args);
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        match r {
            Ok(v) => {
                if !READ_ONLY.contains(&tool) {
                    let detail = format!("{} -> {}", args.to_string_compact().unwrap_or_default(), v.to_string_compact().unwrap_or_default());
                    self.log(LogKind::Call, tool.to_string(), detail.chars().take(600).collect(), ms);
                    if self.mode == Mode::Edit && !SESSION_ONLY.contains(&tool) {
                        self.dirty = true;
                    }
                    // a `set` changes values, not what exists
                    if tool != "set" {
                        self.rows_dirty = true;
                    }
                }
                Ok(v)
            }
            Err(e) => {
                let text = e.to_string();
                self.log(LogKind::Error, format!("{tool}: {text}"), args.to_string_compact().unwrap_or_default(), ms);
                Err(text)
            }
        }
    }

    /// Call a tool and show the error (if any) as a toast.
    pub fn try_call(&mut self, tool: &str, args: Json) -> Option<Json> {
        match self.call(tool, args) {
            Ok(v) => Some(v),
            Err(e) => {
                self.toast(e, true);
                None
            }
        }
    }

    fn obj(pairs: Vec<(&str, Json)>) -> Json {
        Json::obj(pairs)
    }

    // ------------------------------------------------------------------------------------------------------------- refresh

    /// Bring the hierarchy and the selected entity up to date with the engine. Cheap when nothing changed.
    pub fn sync(&mut self) {
        self.follow_disk();
        let rev = self.engine.call("revision", &Json::Null).ok().and_then(|r| r.get("revision").and_then(|v| v.as_u64())).unwrap_or(0);
        let tick = self.engine.sim.tick;
        let playing = self.mode != Mode::Edit;
        let due = self.next_refresh.is_none_or(|t| Instant::now() >= t);
        self.live_revision = rev;
        // the list is rebuilt at once after a change the editor made that adds or removes things, and otherwise at most four times a second
        // (a drag of a handle changes the revision with every step)
        if self.rows_dirty || (rev != self.revision && due) || (playing && due && tick != self.tick) {
            self.refresh_rows();
            self.revision = rev;
            self.rows_dirty = false;
            self.next_refresh = Some(Instant::now() + std::time::Duration::from_millis(250));
        }
        self.tick = tick;
        self.refresh_entity();
    }

    /// The modification time of the scene file on disk, when there is one.
    fn scene_mtime(&self) -> Option<std::time::SystemTime> {
        let p = self.scene.as_ref()?;
        std::fs::metadata(self.opts.root.join(p)).and_then(|m| m.modified()).ok()
    }

    /// The scene file was changed by another program (an agent saving the world it works on, a script that builds the scene): load it again, so that what the person
    /// watches follows the work. Only while editing and only when the editor has no unsaved changes of its own (those are never thrown away), and at most twice a second.
    fn follow_disk(&mut self) {
        if self.mode != Mode::Edit || self.dirty {
            return;
        }
        if !self.next_disk_check.is_none_or(|t| Instant::now() >= t) {
            return;
        }
        self.next_disk_check = Some(Instant::now() + std::time::Duration::from_millis(500));
        let (Some(stamp), Some(now)) = (self.scene_stamp, self.scene_mtime()) else { return };
        if now == stamp {
            return;
        }
        let (selected, path) = (self.selection, self.scene.clone());
        if let Some(p) = path {
            self.open_scene(&p);
            if self.scene_stamp != Some(stamp) {
                self.select(selected);
                self.toast("Scene reloaded: the file changed on disk", false);
            }
        }
    }

    pub fn refresh_rows(&mut self) {
        let select = Json::arr(["Name", "Parent", "Transform", "Camera", "Light", "MeshRenderer", "Sprite", "Text", "Ui", "Emitter", "AudioSource", "Collider", "Tilemap", "Environment"].iter().map(|s| Json::str(*s)));
        let q = self.engine.call("query", &Self::obj(vec![("select", select), ("limit", Json::from(MAX_ROWS as i64))]));
        let Ok(q) = q else { return };
        self.total_entities = q.get("total").and_then(|t| t.as_u64()).unwrap_or(0) as usize;
        let mut rows: Vec<Row> = Vec::new();
        let mut mesh_rows: Vec<(usize, bool)> = Vec::new();
        for e in q.get("entities").and_then(|e| e.as_array()).unwrap_or(&[]) {
            let uid = e.get("uid").and_then(|u| u.as_u64()).unwrap_or(0);
            let comps = e.get("components");
            let has = |n: &str| comps.and_then(|c| c.get(n)).is_some();
            let name = comps.and_then(|c| c.get("Name")).and_then(|n| n.get("value")).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| format!("Entity {uid}"));
            let parent = comps.and_then(|c| c.get("Parent")).and_then(|p| p.get("target")).and_then(|t| t.as_u64()).filter(|p| *p != 0);
            let kind = if has("Camera") {
                Kind::Camera
            } else if has("Light") {
                Kind::Light
            } else if has("Environment") {
                Kind::Environment
            } else if has("MeshRenderer") {
                Kind::Mesh
            } else if has("Sprite") {
                Kind::Sprite
            } else if has("Tilemap") {
                Kind::Tilemap
            } else if has("Ui") {
                Kind::Ui
            } else if has("Text") {
                Kind::Text
            } else if has("Emitter") {
                Kind::Emitter
            } else if has("AudioSource") {
                Kind::Audio
            } else if has("Collider") {
                Kind::Collider
            } else {
                Kind::Empty
            };
            let pos = comps.and_then(|c| c.get("Transform")).and_then(|t| t.get("pos")).and_then(|p| p.as_array()).filter(|a| a.len() >= 3).map(|a| [a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32, a[2].as_f64().unwrap_or(0.0) as f32]);
            let own_mesh = has("MeshRenderer");
            let visible = comps.and_then(|c| c.get("MeshRenderer")).and_then(|m| m.get("visible")).and_then(|v| v.as_bool()).unwrap_or(true);
            if own_mesh {
                mesh_rows.push((rows.len(), visible));
            }
            rows.push(Row { uid, name, parent, depth: 0, children: 0, kind, pos, own_mesh, meshes: 0, shown: 0 });
        }
        // each row counts the meshes in it and under it, and how many of them are shown
        let by_uid: std::collections::HashMap<u64, usize> = rows.iter().enumerate().map(|(i, r)| (r.uid, i)).collect();
        for (i, visible) in mesh_rows {
            let (mut at, mut hops) = (Some(i), 0);
            while let Some(k) = at {
                rows[k].meshes += 1;
                rows[k].shown += visible as u32;
                hops += 1;
                let up = rows[k].parent.and_then(|p| by_uid.get(&p).copied());
                at = if hops > 256 { None } else { up.filter(|p| *p != k) };
            }
        }
        self.all = rows;
        self.rebuild_tree();
        // a selected entity that is gone leaves the selection (only judged against a list that was just read)
        let alive: std::collections::HashSet<u64> = self.all.iter().map(|r| r.uid).collect();
        if self.total_entities <= self.all.len() && self.selected.iter().any(|u| !alive.contains(u)) {
            self.selected.retain(|u| alive.contains(u));
            let main = self.selected.last().copied();
            if self.selection != main {
                self.selection = main;
                self.refresh_entity();
            }
        }
    }

    /// Lay the entities out as a list: parents before their children, children indented, hidden when their parent is collapsed.
    pub fn rebuild_tree(&mut self) {
        let by_uid: std::collections::HashMap<u64, usize> = self.all.iter().enumerate().map(|(i, r)| (r.uid, i)).collect();
        let mut kids: std::collections::HashMap<u64, Vec<usize>> = std::collections::HashMap::new();
        let mut roots: Vec<usize> = Vec::new();
        for (i, r) in self.all.iter().enumerate() {
            match r.parent {
                Some(p) if by_uid.contains_key(&p) && p != r.uid => kids.entry(p).or_default().push(i),
                _ => roots.push(i),
            }
        }
        for r in self.all.iter_mut() {
            r.children = kids.get(&r.uid).map_or(0, |k| k.len());
        }
        let filter = self.filter.to_lowercase();
        let mut out: Vec<Row> = Vec::new();
        if filter.is_empty() {
            let mut stack: Vec<(usize, usize)> = roots.iter().rev().map(|i| (*i, 0)).collect();
            let mut seen = 0usize;
            while let Some((i, depth)) = stack.pop() {
                seen += 1;
                if seen > self.all.len() + 1 {
                    break; // a cycle: stop rather than loop
                }
                let mut r = self.all[i].clone();
                r.depth = depth;
                let expanded = self.expanded.contains(&r.uid);
                let uid = r.uid;
                out.push(r);
                if expanded {
                    if let Some(k) = kids.get(&uid) {
                        for c in k.iter().rev() {
                            stack.push((*c, depth + 1));
                        }
                    }
                }
            }
        } else {
            for r in &self.all {
                if r.name.to_lowercase().contains(&filter) || r.uid.to_string() == filter {
                    let mut r = r.clone();
                    r.depth = 0;
                    out.push(r);
                }
            }
        }
        self.rows = out;
    }

    pub fn refresh_entity(&mut self) {
        let Some(uid) = self.selection else {
            self.entity = None;
            return;
        };
        match self.engine.call("get", &Self::obj(vec![("uid", Json::from(uid as i64))])) {
            Ok(v) => {
                let mut comps: Vec<(String, Json)> = v.get("components").and_then(|c| c.as_object().map(|o| o.to_vec())).unwrap_or_default();
                let rank = |n: &str| match n {
                    "Name" => 0,
                    "Transform" => 1,
                    "Parent" => 2,
                    "LocalTransform" => 3,
                    _ => 10,
                };
                comps.sort_by(|a, b| rank(&a.0).cmp(&rank(&b.0)).then(a.0.cmp(&b.0)));
                self.entity = Some(Entity { uid, comps });
            }
            Err(_) => {
                self.selection = None;
                self.selected.clear();
                self.entity = None;
            }
        }
    }

    /// Is this entity one of the selected ones?
    pub fn is_selected(&self, uid: u64) -> bool {
        self.selected.contains(&uid)
    }

    /// Ctrl+click: add the entity to the selection (and make it the main one), or take it out if it was in.
    pub fn toggle_select(&mut self, uid: u64) {
        if let Some(i) = self.selected.iter().position(|u| *u == uid) {
            self.selected.remove(i);
            let main = self.selected.last().copied();
            if self.selection != main {
                self.selection = main;
                self.refresh_entity();
            }
        } else {
            if let Some(main) = self.selection.filter(|m| !self.selected.contains(m)) {
                self.selected.push(main);
            }
            self.selected.push(uid);
            self.selection = Some(uid);
            self.refresh_entity();
        }
    }

    /// Select several entities at once (a range of the Hierarchy); `main` is the one the Inspector shows.
    pub fn select_many(&mut self, uids: Vec<u64>, main: Option<u64>) {
        let mut list: Vec<u64> = Vec::new();
        for u in uids {
            if !list.contains(&u) {
                list.push(u);
            }
        }
        if let Some(m) = main.filter(|m| list.contains(m)) {
            list.retain(|u| *u != m);
            list.push(m);
        }
        let main = list.last().copied();
        self.selected = list;
        if self.selection != main {
            self.selection = main;
            self.refresh_entity();
        }
    }

    /// The selected entities that are not inside another selected one (acting on a parent acts on what hangs on it).
    fn selection_roots(&self) -> Vec<u64> {
        let parent_of = |u: u64| self.all.iter().find(|r| r.uid == u).and_then(|r| r.parent);
        self.selected
            .iter()
            .copied()
            .filter(|&u| {
                let (mut cur, mut guard) = (parent_of(u), 0);
                while let Some(p) = cur {
                    if self.selected.contains(&p) {
                        return false;
                    }
                    cur = parent_of(p);
                    guard += 1;
                    if guard > 64 {
                        break;
                    }
                }
                true
            })
            .collect()
    }

    /// Delete every selected entity (one inside another selected one goes with it).
    pub fn delete_selected(&mut self) {
        for u in self.selection_roots() {
            self.delete(u);
        }
    }

    /// Duplicate every selected entity; the copies become the selection.
    pub fn duplicate_selected(&mut self) {
        let mut copies: Vec<u64> = Vec::new();
        for u in self.selection_roots() {
            self.duplicate(u);
            if let Some(c) = self.selection {
                copies.push(c);
            }
        }
        if copies.len() > 1 {
            self.select_many(copies, None);
        }
    }

    pub fn select(&mut self, uid: Option<u64>) {
        // (a single click collapses a selection of several, even onto the entity that was the main one)
        self.selected = uid.into_iter().collect();
        if self.selection != uid {
            self.selection = uid;
            self.refresh_entity();
            // show the selection in the hierarchy: open its parents
            if let Some(u) = uid {
                let mut cur = u;
                let mut guard = 0;
                while let Some(p) = self.all.iter().find(|r| r.uid == cur).and_then(|r| r.parent) {
                    self.expanded.insert(p);
                    cur = p;
                    guard += 1;
                    if guard > 64 {
                        break;
                    }
                }
                self.rebuild_tree();
            }
        }
    }

    pub fn components_available(&mut self) -> Vec<String> {
        if self.comp_names.is_empty() {
            if let Ok(d) = self.engine.call("describe", &Json::Null) {
                self.comp_names = d.get("components").and_then(|c| c.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                self.comp_names.sort();
                self.prefabs = d.get("prefabs").and_then(|c| c.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
            }
        }
        self.comp_names.clone()
    }

    /// The fields of a component (asked once, kept).
    pub fn def(&mut self, name: &str) -> CompDef {
        if let Some(d) = self.comps.get(name) {
            return d.clone();
        }
        let mut def = CompDef { name: name.to_string(), ..Default::default() };
        if let Ok(s) = self.engine.call("schema", &Self::obj(vec![("component", Json::str(name))])) {
            def.doc = s.get("doc").and_then(|d| d.as_str()).unwrap_or("").to_string();
            def.defaults = s.get("defaults").cloned();
            for f in s.get("fields").and_then(|f| f.as_array()).unwrap_or(&[]) {
                def.fields.push(FieldDef { name: f.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(), ty: f.get("type").and_then(|n| n.as_str()).unwrap_or("f32").to_string(), count: f.get("count").and_then(|n| n.as_u64()).unwrap_or(1) as usize });
            }
        }
        self.comps.insert(name.to_string(), def.clone());
        def
    }

    /// Show a graph system in the Systems tab: its definition as text (the node view is made from it).
    pub fn open_system(&mut self, name: &str) {
        if let Ok(d) = self.engine.call("get_system", &Self::obj(vec![("name", Json::str(name))])) {
            self.system_text = d.get("def").unwrap_or(&d).to_string_pretty().unwrap_or_default();
            self.selected_system = Some(name.to_string());
            self.system_error = None;
        }
    }

    pub fn forget_schema(&mut self) {
        self.comps.clear();
        self.comp_names.clear();
    }

    // ------------------------------------------------------------------------------------------------------------- scenes

    pub fn title(&self) -> String {
        let name = self.scene.as_deref().unwrap_or("Untitled");
        format!("{}{} - Lunaris Editor", name, if self.dirty { "*" } else { "" })
    }

    pub fn open_scene(&mut self, path: &str) {
        let norm = path.replace('\\', "/");
        let rel = self.relative(&norm);
        match self.call("load_scene", Self::obj(vec![("path", Json::str(rel.clone()))])) {
            Ok(_) => {
                self.scene = Some(rel);
                self.scene_stamp = self.scene_mtime();
                self.dirty = false;
                self.selection = None;
                self.selected.clear();
                self.entity = None;
                self.expanded.clear();
                self.forget_schema();
                self.rows_dirty = true;
                self.revision = u64::MAX;
                self.toast("Scene loaded", false);
            }
            Err(e) => self.toast(e, true),
        }
    }

    /// A path inside the workspace as the tools want it: relative, with forward slashes.
    pub fn relative(&self, path: &str) -> String {
        let p = path.replace('\\', "/");
        let root = self.opts.root.to_string_lossy().replace('\\', "/");
        let root = root.trim_end_matches('/').to_string();
        let lower = p.to_lowercase();
        if lower.starts_with(&format!("{}/", root.to_lowercase())) {
            return p[root.len() + 1..].to_string();
        }
        p
    }

    pub fn new_scene(&mut self) {
        if self.try_call("reset", Json::Null).is_some() {
            self.scene = None;
            self.dirty = false;
            self.selection = None;
            self.selected.clear();
            self.entity = None;
            self.rows_dirty = true;
            // a new scene starts with a camera and a light, so that there is something to see
            let _ = self.spawn_template(Template::Camera, [0.0, 2.0, 6.0]);
            let _ = self.spawn_template(Template::Sun, [0.0, 4.0, 0.0]);
            self.dirty = false;
        }
    }

    pub fn save(&mut self) -> bool {
        match self.scene.clone() {
            Some(p) => self.save_as(&p),
            None => false,
        }
    }

    pub fn save_as(&mut self, path: &str) -> bool {
        let rel = self.relative(path);
        match self.call("save_scene", Self::obj(vec![("path", Json::str(rel.clone()))])) {
            Ok(_) => {
                self.scene = Some(rel);
                self.scene_stamp = self.scene_mtime();
                self.dirty = false;
                self.toast("Scene saved", false);
                true
            }
            Err(e) => {
                self.toast(e, true);
                false
            }
        }
    }

    // ------------------------------------------------------------------------------------------------------------- editing

    pub fn unique_name(&self, base: &str) -> String {
        let taken: HashSet<&str> = self.all.iter().map(|r| r.name.as_str()).collect();
        if !taken.contains(base) {
            return base.to_string();
        }
        (2..).map(|i| format!("{base} {i}")).find(|n| !taken.contains(n.as_str())).unwrap_or_else(|| base.to_string())
    }

    pub fn spawn_template(&mut self, t: Template, at: V3) -> Option<u64> {
        let name = self.unique_name(t.label());
        let comps = t.components(&name, at);
        self.spawn_components(comps)
    }

    /// Spawn an entity from a `components` object (as the `spawn` tool takes it) and select it.
    fn spawn_components(&mut self, comps: Json) -> Option<u64> {
        let r = self.try_call("spawn", Self::obj(vec![("components", comps)]))?;
        let uid = r.get("uid").and_then(|u| u.as_u64())?;
        self.rows_dirty = true;
        self.refresh_rows();
        self.select(Some(uid));
        Some(uid)
    }

    /// A new entity that shows the model file at `path`, named after the file and standing with its lowest point on `at`: where a model's own origin lies is up to its author,
    /// so the bounds of the model (`model_info`) say how far to lift it.
    pub fn add_model(&mut self, path: &str, at: V3) -> Option<u64> {
        let file = path.rsplit('/').next().unwrap_or(path);
        let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
        let lowest = self
            .try_call("model_info", Self::obj(vec![("path", Json::str(path))]))
            .and_then(|i| i.get("bounds")?.get("min")?.as_array()?.get(1)?.as_f64())
            .unwrap_or(0.0) as f32;
        let name = self.unique_name(if stem.is_empty() { "Model" } else { stem });
        let mut comps = Template::Empty.components(&name, [at[0], at[1] - lowest, at[2]]);
        comps.set("MeshRenderer", Json::obj([("mesh", Json::str(path))]));
        self.spawn_components(comps)
    }

    /// A new Audio Source at `at` that plays the sound file at `path`.
    pub fn add_sound(&mut self, path: &str, at: V3) -> Option<u64> {
        let uid = self.spawn_template(Template::AudioSource, at)?;
        self.set(uid, "AudioSource", Json::obj([("file", Json::str(path)), ("voice", Json::str(""))]), None);
        Some(uid)
    }

    pub fn spawn_prefab(&mut self, name: &str, at: V3) {
        let args = Self::obj(vec![("prefab", Json::str(name)), ("at", Json::arr(at.iter().map(|x| Json::from(*x))))]);
        if let Some(r) = self.try_call("spawn_prefab", args) {
            self.rows_dirty = true;
            self.refresh_rows();
            // (the tool answers with the roots of the copies it made, `uids`)
            if let Some(uid) = r.get("uids").and_then(|a| a.as_array()).and_then(|a| a.first()).or_else(|| r.get("uid")).and_then(|u| u.as_u64()) {
                self.select(Some(uid));
            }
        }
    }

    pub fn delete(&mut self, uid: u64) {
        if self.try_call("despawn", Self::obj(vec![("uid", Json::from(uid as i64))])).is_some() {
            if self.selection == Some(uid) {
                self.select(None);
            }
            self.selected.retain(|u| *u != uid);
            self.rows_dirty = true;
        }
    }

    pub fn duplicate(&mut self, uid: u64) {
        let Some(got) = self.try_call("get", Self::obj(vec![("uid", Json::from(uid as i64))])) else { return };
        let Some(Json::Obj(mut comps)) = got.get("components").cloned() else { return };
        // a copy gets a new name and sits a little to the side
        let base = comps.iter().find(|(k, _)| k == "Name").and_then(|(_, v)| v.get("value")).and_then(|v| v.as_str()).unwrap_or("Entity").to_string();
        let new_name = self.unique_name(&base);
        for (k, v) in comps.iter_mut() {
            if k == "Name" {
                *v = Json::obj([("value", Json::str(new_name.clone()))]);
            } else if k == "Transform" {
                if let Some(p) = v.get("pos").and_then(|p| p.as_array()).filter(|a| a.len() == 3) {
                    let moved = Json::arr([Json::from(p[0].as_f64().unwrap_or(0.0) + 1.0), p[1].clone(), p[2].clone()]);
                    v.set("pos", moved);
                }
            }
        }
        if let Some(r) = self.try_call("spawn", Self::obj(vec![("components", Json::Obj(comps))])) {
            if let Some(u) = r.get("uid").and_then(|u| u.as_u64()) {
                self.rows_dirty = true;
                self.refresh_rows();
                self.select(Some(u));
            }
        }
    }

    pub fn rename(&mut self, uid: u64, name: &str) {
        let args = Self::obj(vec![("uid", Json::from(uid as i64)), ("component", Json::str("Name")), ("value", Json::obj([("value", Json::str(name))]))]);
        let _ = self.try_call("set", args);
    }

    /// A new undo group: the edits made with it until the next call to this are one undo step (a drag made of many small `set`s).
    pub fn next_group(&mut self) -> String {
        self.undo_group += 1;
        format!("editor-{}", self.undo_group)
    }

    /// Set some fields of a component of an entity (`value` is an object of fields).
    pub fn set(&mut self, uid: u64, comp: &str, value: Json, group: Option<&str>) {
        let mut args = vec![("uid", Json::from(uid as i64)), ("component", Json::str(comp)), ("value", value)];
        if let Some(g) = group {
            args.push(("undo_group", Json::str(g)));
        }
        let _ = self.try_call("set", Self::obj(args));
    }

    /// Like `set`, and when `uid` is the main one of several selected entities the same fields go to every other selected entity that has the component (what is typed in the Inspector goes to
    /// everything that is selected, as in Unreal's Details panel). The edits share one undo group, so one Undo takes them all back; a name is never copied.
    pub fn set_selected(&mut self, uid: u64, comp: &str, value: Json, group: Option<&str>) {
        let mut targets = vec![uid];
        if comp != "Name" && self.selected.len() > 1 && self.selection == Some(uid) {
            let others: Vec<u64> = self.selected.iter().copied().filter(|u| *u != uid).collect();
            for o in others {
                // (asked of the engine directly: an entity without the component is an answer here, not an error for the Console)
                let has = self
                    .engine
                    .call("get", &Self::obj(vec![("uid", Json::from(o as i64)), ("components", Json::arr([Json::str(comp)]))]))
                    .ok()
                    .is_some_and(|g| g.get("components").and_then(|c| c.get(comp)).is_some_and(|v| !v.is_null()));
                if has {
                    targets.push(o);
                }
            }
        }
        if targets.len() == 1 {
            self.set(uid, comp, value, group);
            return;
        }
        // one `batch` call: one undo step for all of them (a group of single `set` calls would keep only the first one's inverse: consecutive edits of one group are taken to be one drag of one thing)
        let commands = targets.iter().map(|u| Json::obj([("op", Json::str("set")), ("uid", Json::from(*u as i64)), ("component", Json::str(comp)), ("value", value.clone())]));
        let mut args = vec![("commands", Json::arr(commands))];
        if let Some(g) = group {
            args.push(("undo_group", Json::str(g)));
        }
        let _ = self.try_call("batch", Self::obj(args));
    }

    /// Show or hide an entity and everything attached under it (the Outliner's eye): `MeshRenderer.visible` of every mesh in the subtree, as one `batch` call (one Undo).
    pub fn set_shown(&mut self, uid: u64, show: bool) {
        let mut kids: std::collections::HashMap<u64, Vec<u64>> = std::collections::HashMap::new();
        for r in &self.all {
            if let Some(p) = r.parent {
                kids.entry(p).or_default().push(r.uid);
            }
        }
        let own: std::collections::HashSet<u64> = self.all.iter().filter(|r| r.own_mesh).map(|r| r.uid).collect();
        let (mut stack, mut seen, mut targets) = (vec![uid], std::collections::HashSet::new(), Vec::new());
        while let Some(u) = stack.pop() {
            if !seen.insert(u) {
                continue;
            }
            if own.contains(&u) {
                targets.push(u);
            }
            if let Some(k) = kids.get(&u) {
                stack.extend(k.iter().copied());
            }
        }
        if targets.is_empty() {
            return;
        }
        let commands = targets.iter().map(|u| Json::obj([("op", Json::str("set")), ("uid", Json::from(*u as i64)), ("component", Json::str("MeshRenderer")), ("value", Json::obj([("visible", Json::Bool(show))]))]));
        if self.try_call("batch", Self::obj(vec![("commands", Json::arr(commands))])).is_some() {
            self.rows_dirty = true;
            self.refresh_rows();
            if self.selection.is_some_and(|s| targets.contains(&s)) {
                self.refresh_entity();
            }
        }
    }

    /// Read the prefabs of the scene again (they are defined while the editor runs, by a tool call or by loading a scene).
    pub fn refresh_prefabs(&mut self) {
        if let Ok(p) = self.engine.call("prefabs", &Self::obj(vec![])) {
            self.prefabs = p.get("prefabs").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.get("name").and_then(|n| n.as_str()).map(str::to_string)).collect()).unwrap_or_default();
        }
    }

    /// The world transform of a parent: (position, rotation, scale).
    fn parent_tf(&mut self, parent: u64) -> (V3, [f32; 4], V3) {
        let pt = self.engine.call("get", &Self::obj(vec![("uid", Json::from(parent as i64)), ("components", Json::arr([Json::str("Transform")]))])).ok();
        let t = pt.as_ref().and_then(|g| g.get("components")).and_then(|c| c.get("Transform"));
        let vec3 = |n: &str, d: f32| t.and_then(|t| t.get(n)).and_then(|a| a.as_array()).filter(|a| a.len() >= 3).map(|a| [a[0].as_f64().unwrap_or(d as f64) as f32, a[1].as_f64().unwrap_or(d as f64) as f32, a[2].as_f64().unwrap_or(d as f64) as f32]).unwrap_or([d; 3]);
        let rot = t.and_then(|t| t.get("rot")).and_then(|a| a.as_array()).filter(|a| a.len() == 4).map(|a| [a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32, a[2].as_f64().unwrap_or(0.0) as f32, a[3].as_f64().unwrap_or(1.0) as f32]).unwrap_or([0.0, 0.0, 0.0, 1.0]);
        (vec3("pos", 0.0), rot, vec3("scale", 1.0))
    }

    fn attached_to(&self, uid: u64) -> Option<u64> {
        self.entity.as_ref().filter(|e| e.uid == uid).and_then(|e| e.parent())
    }

    /// Move an entity to a world position; an entity attached to a parent moves by changing its `LocalTransform`.
    pub fn move_to(&mut self, uid: u64, world: V3, group: &str) {
        let jv = |p: V3| Json::arr(p.iter().map(|x| Json::from(*x)));
        match self.attached_to(uid) {
            Some(parent) => {
                let (ppos, prot, pscale) = self.parent_tf(parent);
                let d = q_rotate(q_conj(prot), sub(world, ppos));
                let local = [d[0] / pscale[0].max(1e-6), d[1] / pscale[1].max(1e-6), d[2] / pscale[2].max(1e-6)];
                self.set(uid, "LocalTransform", Json::obj([("pos", jv(local))]), Some(group));
            }
            None => self.set(uid, "Transform", Json::obj([("pos", jv(world))]), Some(group)),
        }
    }

    /// Turn an entity to a world rotation (a quaternion); an attached entity changes its `LocalTransform`.
    pub fn rotate_to(&mut self, uid: u64, world: [f32; 4], group: &str) {
        let jq = |q: [f32; 4]| Json::arr(q.iter().map(|x| Json::from(*x)));
        match self.attached_to(uid) {
            Some(parent) => {
                let (_, prot, _) = self.parent_tf(parent);
                let local = crate::mathx::q_norm(crate::mathx::q_mul(q_conj(prot), world));
                self.set(uid, "LocalTransform", Json::obj([("rot", jq(local))]), Some(group));
            }
            None => self.set(uid, "Transform", Json::obj([("rot", jq(world))]), Some(group)),
        }
    }

    /// Give an entity a world scale; an attached entity changes its `LocalTransform`.
    pub fn scale_to(&mut self, uid: u64, world: V3, group: &str) {
        let jv = |p: V3| Json::arr(p.iter().map(|x| Json::from(*x)));
        match self.attached_to(uid) {
            Some(parent) => {
                let (_, _, ps) = self.parent_tf(parent);
                let local = [world[0] / ps[0].max(1e-6), world[1] / ps[1].max(1e-6), world[2] / ps[2].max(1e-6)];
                self.set(uid, "LocalTransform", Json::obj([("scale", jv(local))]), Some(group));
            }
            None => self.set(uid, "Transform", Json::obj([("scale", jv(world))]), Some(group)),
        }
    }

    pub fn undo(&mut self) {
        let _ = self.try_call("undo", Json::Null);
        self.rows_dirty = true;
    }

    pub fn redo(&mut self) {
        let _ = self.try_call("redo", Json::Null);
        self.rows_dirty = true;
    }

    /// Copy the values of component `name` of entity `uid` for `paste_component`. False when the entity has no such component.
    pub fn copy_component(&mut self, uid: u64, name: &str) -> bool {
        let got = self.engine.call("get", &Self::obj(vec![("uid", Json::from(uid as i64)), ("components", Json::arr([Json::str(name)]))])).ok();
        match got.and_then(|g| g.get("components").and_then(|c| c.get(name)).cloned()).filter(|v| !v.is_null()) {
            Some(v) => {
                self.component_clipboard = Some((name.to_string(), v));
                true
            }
            None => false,
        }
    }

    /// Put the copied values on the selected entities (the one shown when none is selected): the component is added where it is missing and its fields are set where it is there, in one undo step.
    pub fn paste_component(&mut self) {
        let Some((name, value)) = self.component_clipboard.clone() else { return };
        let mut targets: Vec<u64> = self.selected.clone();
        if targets.is_empty() {
            targets.extend(self.entity.as_ref().map(|e| e.uid));
        }
        if targets.is_empty() {
            return;
        }
        // (`add_component` merges into a component that is there, so one command does both)
        let commands = targets.iter().map(|u| Json::obj([("op", Json::str("add")), ("uid", Json::from(*u as i64)), ("component", Json::str(name.clone())), ("value", value.clone())]));
        let _ = self.try_call("batch", Self::obj(vec![("commands", Json::arr(commands))]));
        self.rows_dirty = true;
    }

    /// Take the scene back (`steps` above 0) or forward (below 0) that many edits, as that many calls of `undo` or `redo`; it stops at the first one that fails (there is nothing more to undo or redo).
    pub fn undo_by(&mut self, steps: i64) {
        for _ in 0..steps.unsigned_abs() {
            let tool = if steps > 0 { "undo" } else { "redo" };
            if self.try_call(tool, Json::Null).is_none() {
                break;
            }
        }
        self.rows_dirty = true;
    }

    // ------------------------------------------------------------------------------------------------------------- play

    /// Start playing: the scene is saved in a snapshot and comes back when playing stops.
    pub fn play(&mut self) {
        match self.mode {
            Mode::Edit => {
                if self.try_call("snapshot", Self::obj(vec![("name", Json::str(EDIT_SNAPSHOT))])).is_some() {
                    self.mode = Mode::Play;
                    self.log(LogKind::Info, "Play".to_string(), String::new(), 0.0);
                }
            }
            Mode::Paused => self.mode = Mode::Play,
            Mode::Play => {}
        }
    }

    pub fn pause(&mut self) {
        match self.mode {
            Mode::Play => self.mode = Mode::Paused,
            Mode::Paused => self.mode = Mode::Play,
            Mode::Edit => {}
        }
    }

    pub fn stop(&mut self) {
        if self.mode == Mode::Edit {
            return;
        }
        self.mode = Mode::Edit;
        let _ = self.try_call("restore", Self::obj(vec![("name", Json::str(EDIT_SNAPSHOT))]));
        let _ = self.engine.call("delete_snapshot", &Self::obj(vec![("name", Json::str(EDIT_SNAPSHOT))]));
        self.log(LogKind::Info, "Stopped: the scene is back as it was".to_string(), String::new(), 0.0);
        self.rows_dirty = true;
        self.revision = u64::MAX;
    }

    /// The components of an entity as the engine has them now (None when there is no such entity).
    fn components_now(&mut self, uid: u64) -> Option<Vec<(String, Json)>> {
        match self.engine.call("get", &Self::obj(vec![("uid", Json::from(uid as i64))])).ok()?.get("components") {
            Some(Json::Obj(c)) => Some(c.clone()),
            _ => None,
        }
    }

    /// Stop playing but keep what the selected entities were doing: what they have now that differs from the scene that comes back (their places, the values the game changed) is written into
    /// that scene in one undo step (Unreal's "Keep Simulation Changes"). Entities made while playing are not in the scene and are left; a component the game added or removed is added or left as it was.
    /// Returns how many entities had something kept.
    pub fn stop_keeping_selected(&mut self) -> usize {
        if self.mode == Mode::Edit {
            return 0;
        }
        let uids: Vec<u64> = if self.selected.is_empty() { self.selection.into_iter().collect() } else { self.selected.clone() };
        let playing: Vec<(u64, Vec<(String, Json)>)> = uids.iter().filter_map(|u| self.components_now(*u).map(|c| (*u, c))).collect();
        self.stop();
        let mut commands: Vec<Json> = Vec::new();
        let mut kept = 0usize;
        for (uid, comps) in playing {
            let Some(before) = self.components_now(uid) else { continue };
            let mut any = false;
            for (name, value) in comps {
                let same = before.iter().find(|(n, _)| *n == name).is_some_and(|(_, v)| v.to_string_compact().ok() == value.to_string_compact().ok());
                if !same {
                    any = true;
                    commands.push(Json::obj([("op", Json::str("add")), ("uid", Json::from(uid as i64)), ("component", Json::str(name)), ("value", value)]));
                }
            }
            kept += any as usize;
        }
        if !commands.is_empty() {
            let _ = self.try_call("batch", Self::obj(vec![("commands", Json::Arr(commands))]));
            self.log(LogKind::Info, format!("Kept the changes of {kept} entit{} from the play session", if kept == 1 { "y" } else { "ies" }), String::new(), 0.0);
            self.rows_dirty = true;
        }
        kept
    }

    pub fn is_playing(&self) -> bool {
        self.mode == Mode::Play
    }

    pub fn entity_world_pos(&self) -> Option<V3> {
        self.entity.as_ref().and_then(|e| e.vec3("Transform", "pos"))
    }

    pub fn world_of(&self, uid: u64) -> Option<V3> {
        self.all.iter().find(|r| r.uid == uid).and_then(|r| r.pos)
    }
}

/// Things the Entity menu can make.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Template {
    Empty,
    Cube,
    Sphere,
    Cylinder,
    Plane,
    Camera,
    Sun,
    PointLight,
    SpotLight,
    Environment,
    Sprite,
    Text,
    UiPanel,
    UiButton,
    AudioSource,
    Emitter,
}

impl Template {
    pub const ALL: [Template; 16] = [
        Template::Empty,
        Template::Cube,
        Template::Sphere,
        Template::Cylinder,
        Template::Plane,
        Template::Camera,
        Template::Sun,
        Template::PointLight,
        Template::SpotLight,
        Template::Environment,
        Template::Sprite,
        Template::Text,
        Template::UiPanel,
        Template::UiButton,
        Template::AudioSource,
        Template::Emitter,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Template::Empty => "Empty",
            Template::Cube => "Cube",
            Template::Sphere => "Sphere",
            Template::Cylinder => "Cylinder",
            Template::Plane => "Plane",
            Template::Camera => "Camera",
            Template::Sun => "Sun",
            Template::PointLight => "Point Light",
            Template::SpotLight => "Spot Light",
            Template::Environment => "Environment",
            Template::Sprite => "Sprite",
            Template::Text => "Text",
            Template::UiPanel => "UI Panel",
            Template::UiButton => "UI Button",
            Template::AudioSource => "Audio Source",
            Template::Emitter => "Particle Emitter",
        }
    }

    pub fn group(self) -> &'static str {
        match self {
            Template::Empty => "",
            Template::Cube | Template::Sphere | Template::Cylinder | Template::Plane => "3D Object",
            Template::Sun | Template::PointLight | Template::SpotLight | Template::Environment => "Light",
            Template::Camera => "",
            Template::Sprite => "2D",
            Template::Text | Template::UiPanel | Template::UiButton => "UI",
            Template::AudioSource => "Audio",
            Template::Emitter => "Effects",
        }
    }

    /// The components of a new entity of this kind, as the `spawn` tool takes them.
    pub fn components(self, name: &str, at: V3) -> Json {
        let at = [(at[0] * 100.0).round() / 100.0, (at[1] * 100.0).round() / 100.0, (at[2] * 100.0).round() / 100.0];
        let pos = Json::arr(at.iter().map(|x| Json::from(*x)));
        let tr = |extra: Vec<(&str, Json)>| {
            let mut t = vec![("pos", pos.clone())];
            t.extend(extra);
            Json::obj(t)
        };
        let nm = Json::obj([("value", Json::str(name))]);
        let mesh = |m: &str, color: [f64; 4]| Json::obj([("mesh", Json::str(m)), ("color", Json::arr(color.iter().map(|x| Json::from(*x))))]);
        let mut c: Vec<(&str, Json)> = vec![("Name", nm)];
        match self {
            Template::Empty => c.push(("Transform", tr(vec![]))),
            Template::Cube => {
                c.push(("Transform", tr(vec![])));
                c.push(("MeshRenderer", mesh("cube", [0.8, 0.8, 0.85, 1.0])));
            }
            Template::Sphere => {
                c.push(("Transform", tr(vec![])));
                c.push(("MeshRenderer", mesh("sphere", [0.85, 0.7, 0.55, 1.0])));
            }
            Template::Cylinder => {
                c.push(("Transform", tr(vec![])));
                c.push(("MeshRenderer", mesh("cylinder", [0.6, 0.75, 0.9, 1.0])));
            }
            Template::Plane => {
                c.push(("Transform", tr(vec![("scale", Json::arr([Json::from(10.0), Json::from(1.0), Json::from(10.0)]))])));
                c.push(("MeshRenderer", Json::obj([("mesh", Json::str("plane")), ("texture", Json::str("tiles")), ("uv_scale", Json::arr([Json::from(5.0), Json::from(5.0)])), ("color", Json::arr([Json::from(0.8), Json::from(0.82), Json::from(0.9), Json::from(1.0)]))])));
            }
            Template::Camera => {
                c.push(("Transform", tr(vec![])));
                c.push(("Camera", Json::obj([("fov_y", Json::from(60.0))])));
            }
            Template::Sun => {
                c.push(("Transform", tr(vec![("rot", Json::arr([Json::from(-0.5), Json::from(0.3), Json::from(0.1), Json::from(0.8)]))])));
                c.push(("Light", Json::obj([("kind", Json::from(0)), ("intensity", Json::from(2.0))])));
            }
            Template::PointLight => {
                c.push(("Transform", tr(vec![])));
                c.push(("Light", Json::obj([("kind", Json::from(1)), ("intensity", Json::from(5.0)), ("range", Json::from(12.0))])));
            }
            Template::SpotLight => {
                c.push(("Transform", tr(vec![])));
                c.push(("Light", Json::obj([("kind", Json::from(2)), ("intensity", Json::from(8.0)), ("range", Json::from(16.0)), ("angle", Json::from(35.0))])));
            }
            Template::Environment => {
                c.push(("Environment", Json::obj([("sky", Json::Bool(true)), ("bloom", Json::from(0.3))])));
            }
            Template::Sprite => {
                c.push(("Transform", tr(vec![])));
                c.push(("Sprite", Json::obj([("size", Json::arr([Json::from(1.0), Json::from(1.0)]))])));
            }
            Template::Text => {
                c.push(("Text", Json::obj([("value", Json::str("Text")), ("pos", Json::arr([Json::from(0.5), Json::from(0.5)])), ("size", Json::from(0.05))])));
            }
            Template::UiPanel => {
                c.push(("Ui", Json::obj([("pos", Json::arr([Json::from(0.3), Json::from(0.3)])), ("size", Json::arr([Json::from(0.4), Json::from(0.3)])), ("color", Json::arr([Json::from(0.1), Json::from(0.12), Json::from(0.2), Json::from(0.9)]))])));
            }
            Template::UiButton => {
                c.push(("Ui", Json::obj([("pos", Json::arr([Json::from(0.4), Json::from(0.45)])), ("size", Json::arr([Json::from(0.2), Json::from(0.08)])), ("text", Json::str("Button")), ("button", Json::Bool(true)), ("color", Json::arr([Json::from(0.2), Json::from(0.3), Json::from(0.55), Json::from(0.95)]))])));
            }
            Template::AudioSource => {
                c.push(("Transform", tr(vec![])));
                c.push(("AudioSource", Json::obj([("voice", Json::str("coin")), ("looped", Json::Bool(true))])));
            }
            Template::Emitter => {
                c.push(("Transform", tr(vec![])));
                c.push(("Emitter", Json::Obj(Vec::new())));
            }
        }
        Json::obj(c)
    }
}

