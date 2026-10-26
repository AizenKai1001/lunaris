//! The city as a procedural provider (`lunaris_sim::procgen`): an endless city that exists only where somebody is near. Nothing is stored; every file, prefab and zone below is a function of the seed and the numbers in its name.
//!
//! Four kinds of file, each a model written in the frame of its own place (so that far from the origin the numbers stay small):
//!
//! | file                                  | what                                                                  | where it is loaded                     |
//! |---------------------------------------|-----------------------------------------------------------------------|----------------------------------------|
//! | `city/gen_b0_<i>_<j>.glb`             | block (i, j): its ground and the full outside of every building       | within ~200 m                          |
//! | `city/gen_b1_<i>_<j>.glb`             | the same, much coarser (windows as bands)                             | 150 m to 900 m                         |
//! | `city/gen_c2_<ci>_<cj>.glb`           | a cell of 4 x 4 blocks as coloured boxes                              | 340 m to 3.8 km                        |
//! | `city/gen_in_<i>_<j>_<lot>_<g>.glb`   | the inside of two floors (group g) of one building: rooms, stairs, furniture | near the building, on its floors |
//!
//! `gen:` prefabs (`gen:b0_3_-2`, ...) put such a file in the world with its collider (and, for an inside, its lamps), and the grids named in `grids()` make the zones that load them as the camera moves.
//! Files are made by a few worker threads and kept (bounded) in memory; a file asked for before it is ready is made by the asker.

use crate::building::{self, exterior, inside_floor, spec_of, Spec};
use crate::gltf;
use crate::lod;
use crate::materials::Lib;
use crate::mesh::*;
use crate::plan::{Plan, CELL_BLOCKS};
use crate::props::Props;
use crate::rng::Rng;
use crate::street::Ground;
use crate::streetscape;
use lunaris_json::Json;
use lunaris_sim::procgen::{self, Provider, ZoneDef};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};

/// Where each level of detail is loaded: the zone's own radius (and the nearest it is wanted, so that nobody stands inside a building drawn at a coarser level), the margin it is dropped at and how early it is made.
pub const L0_RADIUS: f32 = 125.0;
pub const L0_PREFETCH: f32 = 90.0;
pub const L1_MIN: f32 = 95.0;
/// The things in the streets (trees, lamps, cars, signs: entities of their own) are loaded this near.
pub const THINGS_RADIUS: f32 = 95.0;
pub const THINGS_PREFETCH: f32 = 50.0;
pub const L1_RADIUS: f32 = 380.0;
pub const L1_PREFETCH: f32 = 80.0;
pub const L2_MIN: f32 = 340.0;
pub const L2_RADIUS: f32 = 3800.0;
pub const L2_PREFETCH: f32 = 400.0;
/// How far past a building the inside of it is loaded, how far in height (from the middle of the two floors) and how early it is made.
pub const IN_PAD: f32 = 11.0;
pub const IN_PREFETCH: f32 = 14.0;
pub const IN_VERTICAL: f32 = 7.5;

/// How far from a player the collision shells of a server's blocks are kept (a bullet and a line of sight are stopped by buildings this far off).
pub const SHELL_RADIUS: f32 = 260.0;

/// What a city is for: `Visual` makes everything a player sees (the files and prefabs of the grids in `grids`), `Server` only what a game server needs (the collision shells of `shell`, in the grids of `server_grids`): no
/// pictures, no furniture, no windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Role {
    /// The city of its own single-player game (`citygen city`): the pictures, and the doors, containers and things to harvest as entities of the scene's own components.
    #[default]
    Visual,
    /// The city of a player of the multiplayer game: the pictures and the colliders, but no door, container or harvest entity: a server owns those (milestone M4 of docs/41-the-map.md in the game).
    Client,
    /// The city of a game server: collision shells and nothing else.
    Server,
}

/// A `ProcGrid` the scene needs: the name the provider knows it by and its numbers.
#[derive(Clone, Copy, Debug)]
pub struct GridDef {
    pub name: &'static str,
    pub cell: f32,
    pub radius: f32,
    pub hysteresis: f32,
    pub per_tick: u32,
}

/// The grids of a server's city (`Role::Server`): the blocks' collision shells, and the buildings that somebody is near (the partitions, the street doors and the doors between rooms, the things to search).
pub fn server_grids(plan: &Plan) -> [GridDef; 2] {
    let p = plan.pitch();
    [
        GridDef { name: "s", cell: p, radius: SHELL_RADIUS + p, hysteresis: 60.0, per_tick: 6 },
        GridDef { name: "r", cell: p, radius: 60.0 + IN_PAD + p * 1.1, hysteresis: 40.0, per_tick: 2 },
    ]
}

/// The grids of a city (the `ProcGrid` entities of its scene).
pub fn grids(plan: &Plan) -> [GridDef; 5] {
    let p = plan.pitch();
    let cs = plan.cell_size();
    [
        GridDef { name: "b0", cell: p, radius: L0_RADIUS + L0_PREFETCH + p, hysteresis: 60.0, per_tick: 3 },
        GridDef { name: "b1", cell: p, radius: L1_RADIUS + L1_PREFETCH + p, hysteresis: 100.0, per_tick: 4 },
        GridDef { name: "c2", cell: cs, radius: L2_RADIUS + L2_PREFETCH + cs * 0.75, hysteresis: 600.0, per_tick: 4 },
        GridDef { name: "in", cell: p, radius: 60.0 + IN_PAD + IN_PREFETCH + p * 1.1, hysteresis: 40.0, per_tick: 2 },
        GridDef { name: "t", cell: p, radius: THINGS_RADIUS + THINGS_PREFETCH + p, hysteresis: 40.0, per_tick: 3 },
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Near(i32, i32),
    /// The collision shell of block (i, j), for a server (`shell`).
    Shell(i32, i32),
    /// The things in the streets of block (i, j): no file of its own, a prefab of entities that share the models of `Prop`.
    Things(i32, i32),
    Mid(i32, i32),
    /// The trees of block (i, j) at level 1 as crossed cards (what `streetscape::block_l1` makes): a file of its own when the block's level 1 is baked (a modelling program made the rest).
    Cards(i32, i32),
    Far(i32, i32),
    Inside(i32, i32, u32, u32),
    /// The inside of floors 2g and 2g+1 of a building, for a server: the partitions as a collision model, and the doors and containers as entities (`serverside`).
    Rooms(i32, i32, u32, u32),
    /// The model `n` of `streetscape::PROPS`, at the origin of its own frame, shared by every copy.
    Prop(u32),
}

impl Item {
    fn parse(path: &str) -> Option<Item> {
        let name = path.strip_prefix("city/gen_")?.strip_suffix(".glb")?;
        Item::from_tail(name)
    }

    /// `b0_3_-2`, `in_1_2_4_0`, ...
    fn from_tail(tail: &str) -> Option<Item> {
        let mut it = tail.split('_');
        let kind = it.next()?;
        let n: Vec<i64> = it.map(|s| s.parse().ok()).collect::<Option<_>>()?;
        match (kind, n.as_slice()) {
            ("b0", &[i, j]) => Some(Item::Near(i as i32, j as i32)),
            ("s", &[i, j]) => Some(Item::Shell(i as i32, j as i32)),
            ("b1", &[i, j]) => Some(Item::Mid(i as i32, j as i32)),
            ("k", &[i, j]) => Some(Item::Cards(i as i32, j as i32)),
            ("t", &[i, j]) => Some(Item::Things(i as i32, j as i32)),
            ("c2", &[i, j]) => Some(Item::Far(i as i32, j as i32)),
            ("in", &[i, j, l, g]) if l >= 0 && g >= 0 => Some(Item::Inside(i as i32, j as i32, l as u32, g as u32)),
            ("r", &[i, j, l, g]) if l >= 0 && g >= 0 => Some(Item::Rooms(i as i32, j as i32, l as u32, g as u32)),
            ("p", &[k]) if k >= 0 => Some(Item::Prop(k as u32)),
            _ => None,
        }
    }

    fn tail(&self) -> String {
        match *self {
            Item::Near(i, j) => format!("b0_{i}_{j}"),
            Item::Shell(i, j) => format!("s_{i}_{j}"),
            Item::Mid(i, j) => format!("b1_{i}_{j}"),
            Item::Cards(i, j) => format!("k_{i}_{j}"),
            Item::Things(i, j) => format!("t_{i}_{j}"),
            Item::Far(i, j) => format!("c2_{i}_{j}"),
            Item::Inside(i, j, l, g) => format!("in_{i}_{j}_{l}_{g}"),
            Item::Rooms(i, j, l, g) => format!("r_{i}_{j}_{l}_{g}"),
            Item::Prop(k) => format!("p_{k}"),
        }
    }

    pub fn path(&self) -> String {
        format!("city/gen_{}.glb", self.tail())
    }
}

/// The door of a building, as the game puts it in the doorway: where the leaf hangs (the hinge, on the wall plane), the way along the closed leaf and out of the building (unit, horizontal), the leaf's size, glass or wood.
#[derive(Clone, Copy, Debug)]
pub struct DoorEnt {
    pub hinge: V3,
    pub along: V3,
    pub out: V3,
    pub width: f32,
    pub height: f32,
    pub glass: bool,
    /// How open it starts, 0 (shut) to 1 (wide).
    pub open: f32,
    /// A street door (the way into the building from outside), not a door between rooms: the game's `door_socket` goes on these only (`serverside::socket_entity`).
    pub street: bool,
}

/// The model numbers of the two door leaves (`city/gen_p_300.glb`: a wooden one, 301: a glazed one), each a unit leaf centred on its origin, which a `Door` entity scales to the doorway.
pub const DOOR_WOOD: u32 = 300;
pub const DOOR_GLASS: u32 = 301;

/// A file made: the model, and (for the inside of a building) where its lamps are, relative to the file's origin.
pub struct Made {
    pub glb: Option<Arc<Vec<u8>>>,
    pub lamps: Vec<(V3, f32, f32)>,
    pub triangles: usize,
    /// For a block: its street doors (the middle of the opening on the wall plane, the way out, the width), so that nothing is put in front of one.
    pub doors: Vec<(V3, V3, f32)>,
    /// For a block: its doors as entities (the model has no leaves).
    pub door_ents: Vec<DoorEnt>,
    /// For the inside of a building: the furniture that can be searched, relative to the file's origin (see `MeshBuilder::containers`).
    pub containers: Vec<[f32; 8]>,
    /// For a block: the walls and roofs of its buildings as boxes for the sound (a client has no collision shell; the engine's sound is traced through `Collider` boxes), in the world.
    pub boxes: Vec<crate::shell::AcousticBox>,
}

struct Cell {
    done: Mutex<Option<Result<Arc<Made>, String>>>,
    cv: Condvar,
}

enum Entry {
    Making(Arc<Cell>),
    Ready { made: Arc<Made>, used: u64, bytes: usize },
}

#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
    queue: VecDeque<String>,
    /// The prefab texts to make when no file is waiting.
    prefab_queue: VecDeque<String>,
    bytes: usize,
    clock: u64,
}

/// What the city has done, for logs and tests.
#[derive(Default, Debug, Clone, Copy)]
pub struct Stats {
    pub files: u64,
    pub triangles: u64,
    pub micros: u64,
}

pub struct City {
    pub plan: Plan,
    lib: Lib,
    /// The furniture of the rooms, and the things of the streets and parks.
    props: Props,
    street: Props,
    ground: Ground,
    state: Mutex<State>,
    wake: Condvar,
    cache_bytes: usize,
    log: bool,
    stats: Mutex<Stats>,
    role: Role,
    /// What a game adds to the entities of a server's city (`serverside`).
    parts: crate::serverside::Parts,
    /// The files a modelling program made (`city/baked/b0_I_J.glb`, `b1_I_J.glb`, `c2_CI_CJ.glb`): the folder and the names in it. A block that has its file is served from it; its collision is the shell the server uses.
    baked: Option<(std::path::PathBuf, std::collections::HashSet<String>)>,
    /// The texts of block prefabs the workers have made ahead (`prefetch_prefab`): the tick that loads a block's zone finds its things already described, and spends its time spawning them.
    prefabs: Mutex<HashMap<String, Arc<String>>>,
    /// How many worker threads `install` started (0: nobody makes files in the background, so a caller that needs one makes it itself: the unit tests, the command-line tools).
    workers: std::sync::atomic::AtomicUsize,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl City {
    pub fn new(lib: Lib, props: Props, street: Props, seed: u64) -> City {
        let ground = Ground::new(&lib, &mut Rng::new(seed ^ 0x6772_6F75_6E64));
        let mb: usize = std::env::var("LUNARIS_CITY_CACHE_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(256);
        City { plan: Plan::new(seed), lib, props, street, ground, state: Mutex::new(State::default()), wake: Condvar::new(), cache_bytes: mb << 20, log: std::env::var("LUNARIS_CITY_LOG").is_ok(), role: Role::Visual, parts: Default::default(), stats: Mutex::new(Stats::default()), baked: None, prefabs: Mutex::new(HashMap::new()), workers: std::sync::atomic::AtomicUsize::new(0) }
    }

    /// Serve the files of `dir` (`b0_I_J.glb` for the level-0 model of block (I, J), `b1_I_J.glb` level 1, `c2_CI_CJ.glb` a far cell) instead of making those models: a modelling program made them to stand on the same
    /// footprints and open at the same doors as the buildings this city decides on (`export::polar_plan` is what it was given).
    pub fn with_baked(mut self, dir: &std::path::Path) -> City {
        let names: std::collections::HashSet<String> = std::fs::read_dir(dir).map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.ends_with(".glb")).collect()).unwrap_or_default();
        if !names.is_empty() {
            self.baked = Some((dir.to_path_buf(), names));
        }
        self
    }

    /// The name of the baked file of an item, when it has one.
    fn baked_name(&self, item: Item) -> Option<String> {
        let (_, names) = self.baked.as_ref()?;
        let n = match item {
            Item::Near(..) | Item::Mid(..) | Item::Far(..) => format!("{}.glb", item.tail()),
            _ => return None,
        };
        names.contains(&n).then_some(n)
    }

    fn is_baked(&self, item: Item) -> bool {
        self.baked_name(item).is_some()
    }

    /// Blocks `i0..=i1` by `j0..=j1` given over to a plaza (`Plan::plaza`): the same on a server and on every player's machine, or the walls are not where the picture shows them.
    /// The city cut round at `radius` metres (`Plan::dome`): the same on a server and on every player's machine.
    pub fn with_dome(mut self, radius: Option<f32>) -> City {
        self.plan = self.plan.with_dome(radius);
        self
    }

    /// The city with its streets as rings and radii (`polar.rs`): the same on a server and on every player's machine.
    pub fn with_polar(mut self, on: bool) -> City {
        self.plan = self.plan.with_polar(on.then(crate::polar::Polar::default));
        self
    }

    pub fn with_plaza(mut self, area: Option<(i32, i32, i32, i32)>) -> City {
        self.plan = self.plan.with_plaza(area);
        self
    }

    /// The same city for another purpose (`Role::Server`: nothing but collision).
    pub fn with_role(mut self, role: Role) -> City {
        self.role = role;
        self
    }

    /// The polar blocks `(band, sector)` given over to rubble (`Plan::with_ruins`; `"ruins": [[3, 6], ...]` in `city/city.json`).
    pub fn with_ruins(mut self, list: &[(i32, i32)]) -> City {
        self.plan = self.plan.with_ruins(list);
        self
    }

    /// What the game makes of the doors and containers a server owns (`serverside::Parts`).
    pub fn with_parts(mut self, parts: crate::serverside::Parts) -> City {
        self.parts = parts;
        self
    }

    /// The city of a workspace for a `role`. A server needs the materials' list (the buildings are chosen from what it has: the server and its players must have the same `city/textures/materials.json`) and nothing else:
    /// no furniture, no pictures.
    pub fn open_as(root: &std::path::Path, seed: u64, role: Role) -> std::io::Result<City> {
        if role != Role::Server {
            return Ok(City::open(root, seed)?.with_role(role));
        }
        let text = std::fs::read_to_string(root.join("city").join("textures").join("materials.json")).unwrap_or_default();
        let lib = Lib::from_json(&text);
        Ok(City::new(lib, Props { map: Default::default() }, Props { map: Default::default() }, seed).with_role(role))
    }

    /// The city of a workspace: its materials (`city/textures/materials.json`) and furniture (`city/props`), and the plain roughness pictures its models need written under `city/textures/_flat`.
    pub fn open(root: &std::path::Path, seed: u64) -> std::io::Result<City> {
        let dir = root.join("city");
        let text = std::fs::read_to_string(dir.join("textures").join("materials.json")).unwrap_or_default();
        let mut lib = Lib::from_json(&text);
        let props = Props::load_dir(&mut lib, &dir.join("props"), &crate::props::indoor_budget);
        let street = Props::load_dir_with(&mut lib, &dir.join("props"), &crate::props::outdoor_budget, &crate::props::outdoor_cards);
        gltf::ensure_flat_arms(&dir, &lib)?;
        gltf::ensure_far_textures(&dir)?;
        let city = City::new(lib, props, street, seed);
        let baked = dir.join("baked");
        Ok(if baked.is_dir() && std::env::var("LUNARIS_CITY_BAKED").map_or(true, |v| v != "0") { city.with_baked(&baked) } else { city })
    }

    /// Install this city as the program's procedural provider and start its worker threads.
    pub fn install(self: Arc<Self>) {
        let n = std::env::var("LUNARIS_CITY_WORKERS").ok().and_then(|v| v.parse().ok()).unwrap_or_else(|| (std::thread::available_parallelism().map_or(2, |n| n.get()) / 4).clamp(1, 3));
        self.workers.store(n, std::sync::atomic::Ordering::Relaxed);
        for k in 0..n {
            let me = self.clone();
            std::thread::Builder::new().name(format!("city-worker-{k}")).spawn(move || {
                background_priority();
                me.work()
            }).expect("spawn city worker");
        }
        procgen::set_provider(Some(self));
    }

    pub fn stats(&self) -> Stats {
        *lock(&self.stats)
    }

    // ------------------------------------------------------------------------------------------------ making files

    fn block_centre(&self, i: i32, j: i32) -> V3 {
        if self.plan.polar.is_some() {
            let m = self.plan.block_mid(i, j);
            return [m[0], 0.0, m[1]];
        }
        let (x0, x1, z0, z1) = self.plan.block_rect(i, j);
        [(x0 + x1) * 0.5, 0.0, (z0 + z1) * 0.5]
    }

    fn cell_centre(&self, ci: i32, cj: i32) -> V3 {
        let c = self.plan.cell_size();
        [(ci as f32 + 0.5) * c, 0.0, (cj as f32 + 0.5) * c]
    }

    /// Where the file of the inside of floors 2g and 2g+1 of a building has its origin: the middle of the lot, at the middle height of the two floors.
    fn inside_origin(spec: &Spec, g: u32) -> V3 {
        let c = spec.lot.at(spec.lot.w * 0.5, spec.lot.d * 0.5);
        let n = spec.floor_count();
        let (a, b) = (2 * g, (2 * g + 1).min(n.saturating_sub(1)));
        [c[0], 0.5 * (spec.mid(a) + spec.mid(b)), c[2]]
    }

    fn groups(spec: &Spec) -> u32 {
        spec.floor_count().div_ceil(2)
    }

    /// The origin of the file and the model of an item.
    fn build(&self, item: Item) -> Result<Made, String> {
        let plan = &self.plan;
        let lib = &self.lib;
        let mut doors: Vec<(V3, V3, f32)> = Vec::new();
        let mut door_ents: Vec<DoorEnt> = Vec::new();
        let mut room_containers: Vec<[f32; 8]> = Vec::new();
        let mut boxes: Vec<crate::shell::AcousticBox> = Vec::new();
        if let Some(name) = self.baked_name(item) {
            let (dir, _) = self.baked.as_ref().expect("baked");
            let bytes = std::fs::read(dir.join(&name)).map_err(|e| format!("{name}: {e}"))?;
            // (the street doors are the engine's: asked of the outside it would have made, a millisecond a building)
            if let Item::Near(i, j) = item {
                for lp in plan.lots(i, j) {
                    let spec = spec_of(lib, &lp.lot, lp.kind, lp.key);
                    let (found, glass) = building::exterior_with(&mut MeshBuilder::new(), lib, &spec, false);
                    for (d, g) in found.iter().zip(glass) {
                        doors.push((add(d.hinge, scale(d.along, d.width * 0.5)), d.out, d.width));
                        door_ents.push(DoorEnt { hinge: d.hinge, along: d.along, out: d.out, width: d.width, height: d.height, glass: g, open: 0.0, street: true });
                    }
                    boxes.extend(crate::shell::acoustic_boxes(&spec));
                }
            }
            let triangles = (bytes.len() / 40).max(1);
            return Ok(Made { glb: Some(Arc::new(bytes)), lamps: Vec::new(), triangles, doors, door_ents, containers: Vec::new(), boxes });
        }
        let (mut mb, origin, mut lamps) = match item {
            Item::Near(i, j) => {
                let mut mb = MeshBuilder::new();
                lod::ground_l0(&mut mb, lib, plan, &self.ground, i, j);
                for lp in plan.lots(i, j) {
                    let spec = spec_of(lib, &lp.lot, lp.kind, lp.key);
                    let (found, glass) = building::exterior_with(&mut mb, lib, &spec, false);
                    for (d, g) in found.iter().zip(glass) {
                        doors.push((add(d.hinge, scale(d.along, d.width * 0.5)), d.out, d.width));
                        door_ents.push(DoorEnt { hinge: d.hinge, along: d.along, out: d.out, width: d.width, height: d.height, glass: g, open: 0.0, street: true });
                    }
                    lod::occluders(&mut mb, &spec);
                    boxes.extend(crate::shell::acoustic_boxes(&spec));
                }
                (mb, self.block_centre(i, j), Vec::new())
            }
            Item::Shell(i, j) => {
                let mut mb = MeshBuilder::new();
                let lots = plan.lots(i, j);
                let c = self.block_centre(i, j);
                // the ground: one slab under the block and half the street round it
                let (mat, p) = (lib.id("far_flat"), plan.pitch());
                let ground = lots.first().map_or(0.0, |l| l.lot.o[1]);
                if plan.polar.is_some() {
                    // (the largest rectangle of the block's own frame that the block holds)
                    let (_, ux, uz) = plan.block_frame(i, j);
                    let (hw, hd) = plan.local_rect(i, j);
                    let fr = Frame { o: c, a: [ux[0], 0.0, ux[1]], b: [0.0, 1.0, 0.0], c: [uz[0], 0.0, uz[1]] };
                    mb.cuboid(mat, &fr, [-hw - 3.0, ground - 1.0, -hd - 3.0], [hw + 3.0, ground, hd + 3.0], 63, &lib.tex_plain(mat), [0.5; 3]);
                } else {
                    let fr = Frame { o: [0.0; 3], a: [1.0, 0.0, 0.0], b: [0.0, 1.0, 0.0], c: [0.0, 0.0, 1.0] };
                    mb.cuboid(mat, &fr, [c[0] - p * 0.5, ground - 1.0, c[2] - p * 0.5], [c[0] + p * 0.5, ground, c[2] + p * 0.5], 63, &lib.tex_plain(mat), [0.5; 3]);
                }
                for lp in lots {
                    let spec = spec_of(lib, &lp.lot, lp.kind, lp.key);
                    // (the doors are asked of the outside: it is cheap, a millisecond a building)
                    let (found, _) = building::exterior_with(&mut MeshBuilder::new(), lib, &spec, false);
                    crate::shell::shell(&mut mb, lib, &spec, &found);
                }
                (mb, c, Vec::new())
            }
            Item::Mid(i, j) => {
                let mut mb = MeshBuilder::new();
                lod::ground_l1(&mut mb, lib, plan, &self.ground, i, j);
                for lp in plan.lots(i, j) {
                    let spec = spec_of(lib, &lp.lot, lp.kind, lp.key);
                    lod::building_l1(&mut mb, lib, &spec);
                    lod::occluders(&mut mb, &spec);
                }
                streetscape::block_l1(&mut mb, &self.street, &streetscape::spots(plan, i, j));
                (mb, self.block_centre(i, j), Vec::new())
            }
            Item::Far(ci, cj) => {
                let mut mb = MeshBuilder::new();
                lod::ground_l2(&mut mb, lib, plan, ci, cj);
                let blocks: Vec<(i32, i32)> = if plan.polar.is_some() {
                    plan.blocks_in_cell(plan.cell_size(), ci, cj)
                } else {
                    (cj * CELL_BLOCKS..(cj + 1) * CELL_BLOCKS).flat_map(|bj| (ci * CELL_BLOCKS..(ci + 1) * CELL_BLOCKS).map(move |bi| (bi, bj))).filter(|&(bi, bj)| plan.inside(bi, bj)).collect()
                };
                for (bi, bj) in blocks {
                    for lp in plan.lots(bi, bj) {
                        lod::building_l2(&mut mb, lib, &spec_of(lib, &lp.lot, lp.kind, lp.key));
                    }
                }
                (mb, self.cell_centre(ci, cj), Vec::new())
            }
            Item::Cards(i, j) => {
                let mut mb = MeshBuilder::new();
                streetscape::block_l1(&mut mb, &self.street, &streetscape::spots(plan, i, j));
                (mb, self.block_centre(i, j), Vec::new())
            }
            Item::Things(..) => (MeshBuilder::new(), [0.0; 3], Vec::new()),
            Item::Prop(k) => {
                let mut mb = MeshBuilder::new();
                if let Some(p) = streetscape::PROPS.get(k as usize).and_then(|s| self.street.get(s)) {
                    p.place(&mut mb, [0.0; 3], 0.0, 1.0, [1.0; 3]);
                } else if k == DOOR_WOOD || k == DOOR_GLASS {
                    // a door leaf of unit size, centred: it is scaled to the doorway by the entity (the thickness stays 4.5 cm)
                    let fr = Frame { o: [0.0; 3], a: [1.0, 0.0, 0.0], b: [0.0, 1.0, 0.0], c: [0.0, 0.0, 1.0] };
                    let steel = lib.id("steel");
                    let st = lib.tex_plain(steel);
                    if k == DOOR_WOOD {
                        let wood = lib.id("door_wood");
                        mb.cuboid(wood, &fr, [-0.5, -0.5, -0.0225], [0.5, 0.5, 0.0225], 63, &lib.tex_plain(wood), [1.0; 3]);
                    } else {
                        let glass = lib.id("door_glass");
                        let frame = lib.id("frame_dark");
                        let ft = lib.tex_plain(frame);
                        mb.cuboid(glass, &fr, [-0.5, -0.5, -0.0225], [0.5, 0.5, 0.0225], 63, &lib.tex_plain(glass), [1.0; 3]);
                        for (a0, a1, b0, b1) in [(-0.5, -0.43, -0.5, 0.5), (0.43, 0.5, -0.5, 0.5), (-0.5, 0.5, 0.43, 0.5), (-0.5, 0.5, -0.5, -0.4)] {
                            mb.cuboid(frame, &fr, [a0, b0, -0.03], [a1, b1, 0.03], 63, &ft, [1.0; 3]);
                        }
                    }
                    // a handle on the free edge, at about a metre above the floor
                    mb.cuboid(steel, &fr, [0.40, -0.015, 0.0225], [0.44, 0.025, 0.11], 63, &st, [1.0; 3]);
                } else if k == 100 {
                    // a street name post: a steel pole 3.3 m tall
                    let steel = lib.id("frame_dark");
                    let fr = Frame { o: [0.0; 3], a: [1.0, 0.0, 0.0], b: [0.0, 1.0, 0.0], c: [0.0, 0.0, 1.0] };
                    mb.cuboid(steel, &fr, [-0.04, 0.0, -0.04], [0.04, 3.3, 0.04], 63, &lib.tex_plain(steel), [1.0; 3]);
                } else if (200..200 + crate::materials::STREET_PLATES as u32).contains(&k) {
                    // a street name plate, 1.1 m by 0.27 m, its base on the origin, readable from both sides (the back is a second face turned round, so that its letters are not mirrored)
                    let mat = lib.id(&format!("streetname_{}", k - 200));
                    mb.quad(mat, [[-0.55, 0.0, 0.004], [0.55, 0.0, 0.004], [0.55, 0.27, 0.004], [-0.55, 0.27, 0.004]], [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]], [[1.0; 3]; 4], [0.0, 0.0, 1.0]);
                    mb.quad(mat, [[-0.55, 0.0, -0.004], [0.55, 0.0, -0.004], [0.55, 0.27, -0.004], [-0.55, 0.27, -0.004]], [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]], [[1.0; 3]; 4], [0.0, 0.0, -1.0]);
                }
                (mb, [0.0; 3], Vec::new())
            }
            Item::Rooms(i, j, l, g) => {
                let lots = plan.lots(i, j);
                let lp = lots.get(l as usize).ok_or_else(|| format!("block {i} {j} has no lot {l}"))?;
                let spec = spec_of(lib, &lp.lot, lp.kind, lp.key);
                if g >= Self::groups(&spec) {
                    return Err(format!("building has no floors {}", 2 * g));
                }
                let origin = Self::inside_origin(&spec, g);
                let mut mb = MeshBuilder::new();
                let mat = lib.id("far_flat");
                let tex = lib.tex_plain(mat);
                // the street doors of a building are with its ground floors: they are entities only while somebody is near the building (they are relative to the file's origin, like everything of it)
                if g == 0 {
                    let (found, glass) = building::exterior_with(&mut MeshBuilder::new(), lib, &spec, false);
                    for (d, gl) in found.iter().zip(glass) {
                        door_ents.push(DoorEnt { hinge: sub(d.hinge, origin), along: d.along, out: d.out, width: d.width, height: d.height, glass: gl, open: 0.0, street: true });
                    }
                }
                for k in [2 * g, 2 * g + 1] {
                    if k >= spec.floor_count() {
                        break;
                    }
                    let Some((f, foot)) = building::server_floor(&spec, k) else { continue };
                    let fr = foot.frame();
                    for (lo, hi) in &f.boxes {
                        mb.cuboid(mat, &fr, *lo, *hi, 63, &tex, [0.5; 3]);
                    }
                    // (the doors and containers are relative to the file's origin, like everything of it)
                    for d in &f.doors {
                        door_ents.push(DoorEnt { hinge: sub([d[0], d[1], d[2]], origin), along: [d[3], 0.0, d[4]], out: [d[5], 0.0, d[6]], width: d[7], height: d[8], glass: false, open: d[9], street: false });
                    }
                    for &(x, z, yaw, name) in &f.spots {
                        let p = foot.at(x, z, f.y);
                        let room = crate::interior::SEARCH_ROOMS.iter().position(|r| *r == name).unwrap_or(0);
                        room_containers.push([p[0] - origin[0], p[1] - origin[1], p[2] - origin[2], yaw, 1.0, 1.0, 1.0, room as f32]);
                    }
                }
                (mb, origin, Vec::new())
            }
            Item::Inside(i, j, l, g) => {
                let lots = plan.lots(i, j);
                let lp = lots.get(l as usize).ok_or_else(|| format!("block {i} {j} has no lot {l}"))?;
                let spec = spec_of(lib, &lp.lot, lp.kind, lp.key);
                if g >= Self::groups(&spec) {
                    return Err(format!("building has no floors {}", 2 * g));
                }
                let origin = Self::inside_origin(&spec, g);
                let mut mb = MeshBuilder::new();
                let mut lamps: Vec<(V3, f32, f32)> = Vec::new();
                for k in [2 * g, 2 * g + 1] {
                    if k >= spec.floor_count() {
                        break;
                    }
                    if let Some((f, lights, _)) = inside_floor(lib, &self.props, &spec, k) {
                        mb.append(&f);
                        lamps.extend(lights.iter().map(|l| (sub(l.pos, origin), l.range, l.intensity)));
                    }
                }
                (mb, origin, lamps)
            }
        };
        let triangles = mb.triangles();
        if triangles == 0 {
            // a building with no inside (too small for rooms): a 1 cm quad far below the ground stands in for the file (the zone asks for it ahead of need), and the prefab makes nothing of it
            lamps.clear();
            let far = lib.id("far_flat");
            let mut dot = MeshBuilder::new();
            dot.quad(far, [[0.0, -900.0, 0.0], [0.01, -900.0, 0.0], [0.01, -900.0, 0.01], [0.0, -900.0, 0.01]], [[0.0; 2]; 4], [[0.0; 3]; 4], [0.0, 1.0, 0.0]);
            let glb = gltf::to_glb(&dot, lib).map_err(|e| e.to_string())?;
            return Ok(Made { glb: Some(Arc::new(glb)), lamps, triangles: 0, doors, door_ents, containers: room_containers, boxes: Vec::new() });
        }
        mb.translate(scale(origin, -1.0));
        let glb = gltf::to_glb(&mb, lib).map_err(|e| e.to_string())?;
        let mut containers = std::mem::take(&mut mb.containers);
        containers.extend(room_containers);
        // (the doors inside a building: relative to the file's origin like everything else of it)
        if matches!(item, Item::Inside(..)) {
            door_ents = mb.doors.iter().map(|d| DoorEnt { hinge: [d[0], d[1], d[2]], along: [d[3], 0.0, d[4]], out: [d[5], 0.0, d[6]], width: d[7], height: d[8], glass: false, open: d[9], street: false }).collect();
        }
        Ok(Made { glb: Some(Arc::new(glb)), lamps, triangles, doors, door_ents, containers, boxes })
    }

    /// Make the item of a path, keeping the books: the time, the triangles.
    fn make(&self, path: &str) -> Result<Arc<Made>, String> {
        let item = Item::parse(path).ok_or_else(|| format!("not a city file: {path}"))?;
        let t = std::time::Instant::now();
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.build(item))).map_err(|_| format!("the city panicked making {path}"))??;
        let us = t.elapsed().as_micros() as u64;
        {
            let mut s = lock(&self.stats);
            s.files += 1;
            s.triangles += made.triangles as u64;
            s.micros += us;
        }
        if self.log {
            eprintln!("city: {path}: {} triangles, {} KiB, {:.1} ms", made.triangles, made.glb.as_ref().map_or(0, |g| g.len()) >> 10, us as f64 / 1000.0);
        }
        Ok(Arc::new(made))
    }

    // ------------------------------------------------------------------------------------------------ the cache

    fn publish(&self, path: &str, cell: &Cell, result: Result<Arc<Made>, String>) {
        {
            let mut st = lock(&self.state);
            st.clock += 1;
            let clock = st.clock;
            match &result {
                Ok(made) => {
                    let bytes = made.glb.as_ref().map_or(0, |g| g.len()) + made.lamps.len() * 24;
                    st.bytes += bytes;
                    st.entries.insert(path.to_string(), Entry::Ready { made: made.clone(), used: clock, bytes });
                    // keep the books under the limit: the least recently used go first
                    while st.bytes > self.cache_bytes {
                        let oldest = st.entries.iter().filter_map(|(k, e)| match e {
                            Entry::Ready { used, .. } if k != path => Some((*used, k.clone())),
                            _ => None,
                        }).min();
                        let Some((_, k)) = oldest else { break };
                        if let Some(Entry::Ready { bytes, .. }) = st.entries.remove(&k) {
                            st.bytes -= bytes;
                        }
                    }
                }
                Err(_) => {
                    st.entries.remove(path);
                }
            }
        }
        *lock(&cell.done) = Some(result);
        cell.cv.notify_all();
    }

    /// The made item of a path when it is ready; otherwise it is asked of the workers, at the front of their queue, and the answer is `None`: the tick that loads a zone never waits for a file (an interior is 30 to 80 ms of work).
    fn made_if_ready(&self, path: &str) -> Option<Arc<Made>> {
        {
            let mut guard = lock(&self.state);
            let st = &mut *guard;
            st.clock += 1;
            let clock = st.clock;
            if let Some(Entry::Ready { made, used, .. }) = st.entries.get_mut(path) {
                *used = clock;
                return Some(made.clone());
            }
        }
        self.prefetch_file(path);
        let mut st = lock(&self.state);
        if let Some(at) = st.queue.iter().position(|p| p == path) {
            if at > 0 {
                let p = st.queue.remove(at).expect("queued");
                st.queue.push_front(p);
            }
        }
        None
    }

    /// The made item of a path: from the cache, or made now (by this thread, or waited for when a worker is at it; one that waits in the queue is taken out of it and made by the asker, who is not to wait behind the others).
    fn made(&self, path: &str) -> Result<Arc<Made>, String> {
        enum Act {
            Have(Arc<Made>),
            Wait(Arc<Cell>),
            Make(Arc<Cell>),
        }
        let act = {
            let mut guard = lock(&self.state);
            let st = &mut *guard;
            st.clock += 1;
            let clock = st.clock;
            match st.entries.get_mut(path) {
                Some(Entry::Ready { made, used, .. }) => {
                    *used = clock;
                    Act::Have(made.clone())
                }
                Some(Entry::Making(c)) => {
                    let c = c.clone();
                    match st.queue.iter().position(|p| p == path) {
                        Some(at) => {
                            st.queue.remove(at);
                            Act::Make(c)
                        }
                        None => Act::Wait(c),
                    }
                }
                None => {
                    let c = Arc::new(Cell { done: Mutex::new(None), cv: Condvar::new() });
                    st.entries.insert(path.to_string(), Entry::Making(c.clone()));
                    Act::Make(c)
                }
            }
        };
        match act {
            Act::Have(m) => Ok(m),
            Act::Wait(c) => {
                let mut done = lock(&c.done);
                while done.is_none() {
                    done = c.cv.wait(done).unwrap_or_else(|e| e.into_inner());
                }
                done.clone().expect("the item is done")
            }
            Act::Make(c) => {
                let result = self.make(path);
                self.publish(path, &c, result.clone());
                result
            }
        }
    }

    fn work(self: Arc<Self>) {
        loop {
            let path = {
                let mut st = lock(&self.state);
                loop {
                    if let Some(p) = st.queue.pop_front() {
                        break p;
                    }
                    if let Some(name) = st.prefab_queue.pop_front() {
                        drop(st);
                        // (a prefab text made ahead, for the tick that will load the zone)
                        if !lock(&self.prefabs).contains_key(&name) {
                            if let Some(text) = self.make_prefab(&name, true) {
                                lock(&self.prefabs).insert(name, Arc::new(text));
                            }
                        }
                        st = lock(&self.state);
                        continue;
                    }
                    st = self.wake.wait(st).unwrap_or_else(|e| e.into_inner());
                }
            };
            let cell = match lock(&self.state).entries.get(&path) {
                Some(Entry::Making(c)) => c.clone(),
                _ => continue,
            };
            let result = self.make(&path);
            self.publish(&path, &cell, result);
        }
    }

    // ------------------------------------------------------------------------------------------------ prefabs

    /// A door in a doorway, as an entity: the leaf (a shared unit model scaled to the doorway) hung on the hinge, a box collider the size of the leaf, and the `Door` the scene's systems swing (`want` 1 opens it, `open` follows
    /// it): the leaf turns about the hinge into the building, up to `maxang`. Everything it needs to place itself at any angle is in the component (the hinge and the two directions, relative to the block).
    /// A wall or a slab of a building for the sound: the engine's `Collider` that no body touches (its own layer, an empty mask) with the `AcousticMaterial` of a facade seen from outside and a plastered wall seen from inside (absorption 0.2: a street still rings for about two seconds, a room of 3000 cubic metres for under two; the echoes between the buildings of a street are what a city sounds like)
    /// that a wall's worth of sound hardly passes (the doors and windows that are not cut out are what lets some through).
    fn acoustic_entity(b: &crate::shell::AcousticBox, origin: V3) -> Json {
        let n = |x: f32| Json::Float(x as f64);
        let v3 = |a: [f32; 3]| Json::arr(a.map(n));
        let rot = [0.0, (b.yaw * 0.5).sin(), 0.0, (b.yaw * 0.5).cos()];
        Json::obj([
            ("name", Json::str("Wall")),
            ("components", Json::obj([
                ("LocalTransform", Json::obj([("pos", v3(sub(b.center, origin))), ("rot", Json::arr(rot.map(n)))])),
                ("Collider", Json::obj([("shape", Json::from(1)), ("size", v3([b.half[0] * 2.0, b.half[1] * 2.0, b.half[2] * 2.0])), ("trigger", Json::Bool(false))])),
                ("CollisionLayer", Json::obj([("layer", Json::from(31)), ("mask", Json::from(0))])),
                ("AcousticMaterial", Json::obj([("absorption", n(0.20)), ("transmission", n(0.06))])),
            ])),
        ])
    }

    fn door_entity(d: &DoorEnt, origin: V3, locked: bool) -> Json {
        let n = |x: f32| Json::Float(x as f64);
        let v3 = |a: [f32; 3]| Json::arr(a.map(n));
        let crate::serverside::Leaf { hx, hz, cx, cy: centre_y, cz, ax, az, ox, oz, yaw0, dir, w, h, half } = crate::serverside::leaf(d, origin);
        let rot = [0.0, (yaw0 * 0.5).sin(), 0.0, (yaw0 * 0.5).cos()];
        let model = if d.glass { DOOR_GLASS } else { DOOR_WOOD };
        Json::obj([
            ("name", Json::str("Door")),
            ("components", Json::obj([
                ("LocalTransform", Json::obj([("pos", v3([cx, centre_y, cz])), ("rot", Json::arr(rot.map(n))), ("scale", v3([w, h, 1.0]))])),
                ("MeshRenderer", Json::obj([("mesh", Json::str(format!("city/gen_p_{model}.glb"))), ("color", Json::arr([1.0, 1.0, 1.0, 1.0].map(n))), ("cast_shadow", Json::Bool(false))])),
                ("Collider", Json::obj([("shape", Json::from(1)), ("size", v3([1.0, 1.0, 0.09])), ("trigger", Json::Bool(false))])),
                ("Door", Json::obj([
                    ("hx", n(hx)), ("hy", n(centre_y)), ("hz", n(hz)),
                    ("ax", n(ax)), ("az", n(az)), ("ox", n(ox)), ("oz", n(oz)),
                    ("yaw0", n(yaw0)), ("dir", n(dir)), ("half", n(half)),
                    ("open", n(d.open)), ("want", n(d.open)), ("maxang", n(1.75)), ("locked", n(if locked { 1.0 } else { 0.0 })),
                ])),
            ])),
        ])
    }

    /// A piece of furniture that can be searched, as an entity: an invisible box where it stands (the engine's eye ray finds it) and the `Container` the scene's systems open: its `kind` says what is likely inside and
    /// its `seed` is the building's own number for it; `state` 0 means nobody has looked inside yet.
    fn container_entity(&self, c: &[f32; 8], seed: u64) -> Json {
        let n = |x: f32| Json::Float(x as f64);
        let v3 = |a: [f32; 3]| Json::arr(a.map(n));
        let half = c[3].to_radians() * 0.5;
        let rot = [0.0, half.sin(), 0.0, half.cos()];
        Json::obj([
            ("name", Json::str("Container")),
            ("components", Json::obj([
                ("LocalTransform", Json::obj([("pos", v3([c[0], c[1] + c[5] * 0.5, c[2]])), ("rot", Json::arr(rot.map(n)))])),
                ("Collider", Json::obj([("shape", Json::from(1)), ("size", v3([c[4].max(0.2), c[5].max(0.2), c[6].max(0.2)])), ("trigger", Json::Bool(false))])),
                ("Container", Json::obj([("kind", n(c[7])), ("seed", n(seed as f32)), ("state", n(0.0))])),
            ])),
        ])
    }

    /// A prefab of nothing but the entities given (no model of its own).
    fn model_children_only(children: Vec<Json>) -> String {
        let n = |x: f32| Json::Float(x as f64);
        Json::obj([("components", Json::obj([("Transform", Json::obj([("pos", Json::arr([0.0f32; 3].map(n)))]))])), ("children", Json::Arr(children))]).to_string_compact().unwrap_or_default()
    }

    /// `vary`: the model gets `MacroVary` (the component's own defaults: patches of lighter and darker, warmer and cooler colour over 24 m, 8 m and 1 m, so that a wall, a floor or a road that repeats one small texture does not show it from afar); `LUNARIS_CITY_VARY=0` leaves it off.
    fn model_children(path: &str, collider: bool, shadow: bool, glow: bool, vary: bool, lamps: &[(V3, f32, f32)], more: Vec<Json>) -> String {
        let n = |x: f32| Json::Float(x as f64);
        let v3 = |a: [f32; 3]| Json::arr(a.map(n));
        let mut shape: Vec<(&str, Json)> = vec![
            ("LocalTransform", Json::obj([("pos", v3([0.0; 3]))])),
            ("MeshRenderer", Json::obj([("mesh", Json::str(path)), ("color", Json::arr([1.0, 1.0, 1.0, 1.0].map(n))), ("cast_shadow", Json::Bool(shadow))])),
        ];
        // the lit windows of a block glow at night (`NightGlow`, the scene's `windows_glow`; the far models have windows that the shader lights)
        if glow {
            shape.push(("NightGlow", Json::Obj(Vec::new())));
        }
        if vary && std::env::var("LUNARIS_CITY_VARY").map_or(true, |v| v != "0") {
            shape.push(("MacroVary", Json::Obj(Vec::new())));
        }
        if collider {
            shape.push(("MeshCollider", Json::obj([("mesh", Json::str(path))])));
        }
        let mut children = vec![Json::obj([("name", Json::str("Model")), ("components", Json::obj(shape))])];
        children.extend(more);
        for (pos, range, intensity) in lamps {
            children.push(Json::obj([
                ("name", Json::str("Lamp")),
                ("components", Json::obj([
                    ("LocalTransform", Json::obj([("pos", v3(*pos))])),
                    ("Light", Json::obj([("kind", Json::from(1)), ("color", v3([1.0, 0.86, 0.68])), ("intensity", n(*intensity)), ("range", n(*range))])),
                ])),
            ]));
        }
        Json::obj([("components", Json::obj([("Transform", Json::obj([("pos", v3([0.0; 3]))]))])), ("children", Json::Arr(children))]).to_string_compact().unwrap_or_default()
    }

    // ------------------------------------------------------------------------------------------------ zones

    fn zone(&self, item: Item, pos: V3, radius: f32, min_radius: f32, hysteresis: f32, prefetch: f32, vertical: f32) -> ZoneDef {
        let path = item.path();
        // a block asks ahead for the models of the things in it too (each is one file for the whole city)
        let mut assets = format!("model {path}");
        if matches!(item, Item::Near(..) | Item::Inside(..)) {
            // the leaves of its doors
            assets.push_str(&format!("
model city/gen_p_{DOOR_WOOD}.glb
model city/gen_p_{DOOR_GLASS}.glb"));
        }
        if let Item::Near(i, j) = item {
            if self.is_baked(item) && self.plan.polar.is_some_and(|_| (0..crate::polar::BANDS).contains(&i)) {
                assets.push_str(&format!("\nmodel {}", Item::Shell(i, j).path()));
            }
        }
        if let Item::Mid(i, j) = item {
            if self.is_baked(item) {
                assets.push_str(&format!("\nmodel {}", Item::Cards(i, j).path()));
            }
        }
        if let Item::Things(i, j) = item {
            assets.clear();
            let mut used: Vec<usize> = streetscape::spots(&self.plan, i, j).iter().filter_map(|s| streetscape::prop_index(s.slug)).collect();
            used.sort_unstable();
            used.dedup();
            for k in used {
                assets.push_str(&format!("\nmodel city/gen_p_{k}.glb"));
            }
        }
        if matches!(item, Item::Near(..) | Item::Things(..)) {
            self.prefetch_prefab(&format!("gen:{}", item.tail()));
        }
        ZoneDef { name: item.tail(), pos, prefab: format!("gen:{}", item.tail()), radius, min_radius, hysteresis, prefetch, vertical, assets }
    }
}

impl Provider for City {
    fn claims(&self, path: &str) -> bool {
        Item::parse(path).is_some()
    }

    fn file(&self, path: &str) -> Option<Result<Vec<u8>, String>> {
        Item::parse(path)?;
        Some(self.made(path).and_then(|m| m.glb.as_ref().map(|g| g.as_ref().clone()).ok_or_else(|| format!("{path} is empty"))))
    }

    fn file_ready(&self, path: &str) -> bool {
        matches!(lock(&self.state).entries.get(path), Some(Entry::Ready { .. }))
    }

    fn prefetch_file(&self, path: &str) {
        if Item::parse(path).is_none() {
            return;
        }
        let mut st = lock(&self.state);
        if st.entries.contains_key(path) {
            return;
        }
        let cell = Arc::new(Cell { done: Mutex::new(None), cv: Condvar::new() });
        st.entries.insert(path.to_string(), Entry::Making(cell));
        st.queue.push_back(path.to_string());
        drop(st);
        self.wake.notify_one();
    }

    fn prefab(&self, name: &str) -> Option<String> {
        // made ahead by a worker when there was time (it is used once: a block that unloads and comes back is made again)
        if let Some(t) = lock(&self.prefabs).remove(name) {
            return Some(t.as_ref().clone());
        }
        // (the tick never waits for a file when workers make them; with no workers it makes them itself)
        self.make_prefab(name, self.workers.load(std::sync::atomic::Ordering::Relaxed) == 0)
    }

    fn zones(&self, grid: &str, i: i32, j: i32) -> Vec<ZoneDef> {
        let plan = &self.plan;
        // a server has the collision shells and the insides of the buildings (as its own entities) and nothing else; a player's city has everything but them
        if (self.role == Role::Server) != matches!(grid, "s" | "r") {
            return Vec::new();
        }
        // (a round city: the blocks beyond the dome's radius are not made; the far cells are, and make nothing there)
        if grid == "c2" {
            return if plan.cell_inside(i, j) { vec![self.zone(Item::Far(i, j), self.cell_centre(i, j), L2_RADIUS, L2_MIN, 150.0, L2_PREFETCH, 0.0)] } else { Vec::new() };
        }
        if plan.polar.is_some() {
            // a polar city's blocks are named (band, sector): a streaming cell has the blocks whose middle is in it
            return plan.blocks_in_cell(plan.pitch(), i, j).into_iter().flat_map(|(bi, bj)| self.block_zones(grid, bi, bj)).collect();
        }
        if !plan.inside(i, j) {
            return Vec::new();
        }
        self.block_zones(grid, i, j)
    }
}

impl City {
    /// The zones of block (i, j) in a grid.
    fn block_zones(&self, grid: &str, i: i32, j: i32) -> Vec<ZoneDef> {
        let plan = &self.plan;
        let polar = plan.polar.is_some();
        match grid {
            // (the plaza of the Core is a disc 190 m across the middle: it is loaded from afar)
            "b0" => vec![self.zone(Item::Near(i, j), self.block_centre(i, j), if polar && i < 0 { 330.0 } else { L0_RADIUS }, 0.0, 40.0, L0_PREFETCH, 0.0)],
            "s" if polar && (i < 0 || i >= crate::polar::BANDS) => Vec::new(),
            "s" => vec![self.zone(Item::Shell(i, j), self.block_centre(i, j), SHELL_RADIUS, 0.0, 40.0, 0.0, 0.0)],
            "t" if polar && (i < 0 || i >= crate::polar::BANDS) => Vec::new(),
            "t" => vec![self.zone(Item::Things(i, j), self.block_centre(i, j), THINGS_RADIUS, 0.0, 30.0, THINGS_PREFETCH, 0.0)],
            "b1" => vec![self.zone(Item::Mid(i, j), self.block_centre(i, j), L1_RADIUS, L1_MIN, 60.0, L1_PREFETCH, 0.0)],
            "r" => {
                let mut out = Vec::new();
                for lp in plan.lots(i, j) {
                    let spec = spec_of(&self.lib, &lp.lot, lp.kind, lp.key);
                    let half = 0.5 * (lp.lot.w * lp.lot.w + lp.lot.d * lp.lot.d).sqrt();
                    for g in 0..Self::groups(&spec) {
                        out.push(self.zone(Item::Rooms(i, j, lp.index, g), Self::inside_origin(&spec, g), half + IN_PAD, 0.0, 12.0, 0.0, IN_VERTICAL));
                    }
                }
                out
            }
            "in" => {
                let mut out = Vec::new();
                for lp in plan.lots(i, j) {
                    let spec = spec_of(&self.lib, &lp.lot, lp.kind, lp.key);
                    let half = 0.5 * (lp.lot.w * lp.lot.w + lp.lot.d * lp.lot.d).sqrt();
                    for g in 0..Self::groups(&spec) {
                        out.push(self.zone(Item::Inside(i, j, lp.index, g), Self::inside_origin(&spec, g), half + IN_PAD, 0.0, 12.0, IN_PREFETCH, IN_VERTICAL));
                    }
                }
                out
            }
            _ => Vec::new(),
        }
    }
}

impl City {
    /// Ask a worker to describe the prefab `name` (`gen:b0_3_4`) ahead of its zone being loaded.
    fn prefetch_prefab(&self, name: &str) {
        if lock(&self.prefabs).contains_key(name) {
            return;
        }
        let mut st = lock(&self.state);
        if st.prefab_queue.iter().any(|p| p == name) {
            return;
        }
        st.prefab_queue.push_back(name.to_string());
        drop(st);
        self.wake.notify_one();
    }

    fn make_prefab(&self, name: &str, block: bool) -> Option<String> {
        // (a worker may wait for the files it needs; the tick may not)
        let get = |p: &str| -> Option<Arc<Made>> { if block { self.made(p).ok() } else { self.made_if_ready(p) } };
        let item = Item::from_tail(name.strip_prefix("gen:")?)?;
        let path = item.path();
        match item {
            Item::Near(i, j) => {
                let made = get(&path)?;
                let origin = self.block_centre(i, j);
                // a street door is locked now and then (the scene's `door_use`: a hatchet or a crowbar breaks the lock); the rooms inside are never locked
                let doors: Vec<Json> = made.door_ents.iter().filter(|_| self.role == Role::Visual).enumerate().map(|(k, d)| Self::door_entity(d, origin, Rng::keyed(&[self.plan.seed, i as u64, j as u64, k as u64, 0x4C4F_434B]).u64() % 100 < 28)).collect();
                // a baked block is collided with by the shell the server uses (its own file is a picture only)
                let mut doors = doors;
                // (a client's sound is traced through boxes where the walls and roofs are: it has no shell, and meshes stop no sound)
                if self.role != Role::Server {
                    doors.extend(made.boxes.iter().map(|b| Self::acoustic_entity(b, origin)));
                }
                let baked = self.is_baked(item) && self.plan.polar.is_some_and(|_| (0..crate::polar::BANDS).contains(&i));
                if baked {
                    let n = |x: f32| Json::Float(x as f64);
                    doors.push(Json::obj([("name", Json::str("Shell")), ("components", Json::obj([("LocalTransform", Json::obj([("pos", Json::arr([0.0f32; 3].map(n)))])), ("MeshCollider", Json::obj([("mesh", Json::str(Item::Shell(i, j).path()))]))]))]));
                }
                Some(Self::model_children(&path, !self.is_baked(item), true, true, true, &[], doors))
            }
            Item::Cards(..) => Some(Self::model_children(&path, false, true, false, false, &[], Vec::new())),
            Item::Shell(..) => {
                let n = |x: f32| Json::Float(x as f64);
                let shape = Json::obj([("name", Json::str("Shell")), ("components", Json::obj([("LocalTransform", Json::obj([("pos", Json::arr([0.0f32; 3].map(n)))])), ("MeshCollider", Json::obj([("mesh", Json::str(path))]))]))]);
                Some(Json::obj([("components", Json::obj([("Transform", Json::obj([("pos", Json::arr([0.0f32; 3].map(n)))]))])), ("children", Json::Arr(vec![shape]))]).to_string_compact().unwrap_or_default())
            }
            Item::Things(i, j) => {
                // the things of the block, but none in front of a door (the block's file knows where its doors are)
                let made = get(&Item::Near(i, j).path())?;
                let spots = streetscape::clear_of_doors(streetscape::spots(&self.plan, i, j), &made.doors);
                let mut things = streetscape::entities(&self.street, &spots, self.block_centre(i, j), self.role == Role::Visual, self.plan.polar.is_some());
                things.extend(streetscape::sign_posts(&self.plan, i, j, self.block_centre(i, j)));
                Some(Self::model_children_only(things))
            }
            Item::Mid(i, j) => {
                let mut more = Vec::new();
                if self.is_baked(item) {
                    // (the baked level 1 has no trees: their cards are the engine's, at the places of the trees of level 0)
                    let n = |x: f32| Json::Float(x as f64);
                    more.push(Json::obj([("name", Json::str("Cards")), ("components", Json::obj([("LocalTransform", Json::obj([("pos", Json::arr([0.0f32; 3].map(n)))])), ("MeshRenderer", Json::obj([("mesh", Json::str(Item::Cards(i, j).path())), ("color", Json::arr([1.0, 1.0, 1.0, 1.0].map(n))), ("cast_shadow", Json::Bool(true))]))]))]));
                }
                Some(Self::model_children(&path, false, true, false, true, &[], more))
            }
            Item::Far(..) => Some(Self::model_children(&path, false, false, true, true, &[], Vec::new())),
            Item::Prop(_) => None,
            Item::Rooms(i, j, l, g) => {
                let made = get(&path)?;
                let n = |x: f32| Json::Float(x as f64);
                let mut children: Vec<Json> = Vec::new();
                if made.triangles > 0 {
                    children.push(Json::obj([("name", Json::str("Rooms")), ("components", Json::obj([("LocalTransform", Json::obj([("pos", Json::arr([0.0f32; 3].map(n)))])), ("MeshCollider", Json::obj([("mesh", Json::str(path.clone()))]))]))]));
                }
                children.extend(made.door_ents.iter().map(|d| crate::serverside::door_entity(d, [0.0; 3], &self.parts)));
                children.extend(made.door_ents.iter().filter_map(|d| crate::serverside::socket_entity(d, [0.0; 3], &self.parts)));
                children.extend(made.containers.iter().enumerate().filter_map(|(k, c)| {
                    let key = Rng::keyed(&[self.plan.seed, i as u64, j as u64, l as u64, g as u64, k as u64, 0x434F4E54]).u64() % 16_000_000;
                    crate::serverside::container_entity(c, key as u32, &self.parts)
                }));
                Some(Json::obj([("components", Json::obj([("Transform", Json::obj([("pos", Json::arr([0.0f32; 3].map(n)))]))])), ("children", Json::Arr(children))]).to_string_compact().unwrap_or_default())
            }
            Item::Inside(..) => {
                let made = get(&path)?;
                if made.triangles == 0 {
                    return Some(Json::obj([("components", Json::obj([("Transform", Json::obj([("pos", Json::arr([0.0; 3].map(|x| Json::Float(x))))]))]))]).to_string_compact().unwrap_or_default());
                }
                let Item::Inside(i, j, l, g) = item else { return None };
                let boxes: Vec<Json> = made.containers.iter().filter(|_| self.role == Role::Visual).enumerate().map(|(k, c)| self.container_entity(c, Rng::keyed(&[self.plan.seed, i as u64, j as u64, l as u64, g as u64, k as u64, 0x434F4E54]).u64() % 16_000_000)).collect();
                let mut boxes = boxes;
                // (its doors are relative to the file's origin already)
                boxes.extend(made.door_ents.iter().filter(|_| self.role == Role::Visual).map(|d| Self::door_entity(d, [0.0; 3], false)));
                Some(Self::model_children(&path, true, false, false, true, &made.lamps, boxes))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn city() -> City {
        City::new(Lib::from_json(""), Props { map: Default::default() }, Props { map: Default::default() }, 9)
    }

    #[test]
    fn paths_and_names_round_trip_with_negative_numbers() {
        for item in [Item::Near(3, -2), Item::Shell(-4, 9), Item::Mid(-17, 40), Item::Cards(3, -2), Item::Far(-1, -1), Item::Inside(5, -6, 12, 3), Item::Rooms(5, -6, 12, 3), Item::Prop(7)] {
            assert_eq!(Item::parse(&item.path()), Some(item));
            assert_eq!(Item::from_tail(&item.tail()), Some(item));
        }
        assert_eq!(Item::parse("city/district.gltf"), None);
        assert_eq!(Item::parse("city/gen_b0_1.glb"), None);
        assert_eq!(Item::parse("city/gen_zz_1_2.glb"), None);
        assert_eq!(Item::parse("models/gen_b0_1_2.glb"), None);
    }

    #[test]
    fn every_level_makes_a_model_in_its_own_frame_and_the_provider_claims_it() {
        let c = city();
        for item in [Item::Near(2, 3), Item::Mid(2, 3), Item::Far(0, 0)] {
            let made = c.make(&item.path()).unwrap();
            let glb = made.glb.as_ref().unwrap();
            assert_eq!(&glb[..4], b"glTF");
            assert!(made.triangles > 10, "{item:?}");
            assert!(Provider::claims(&c, &item.path()));
        }
        let near = c.make(&Item::Near(2, 3).path()).unwrap().triangles;
        let mid = c.make(&Item::Mid(2, 3).path()).unwrap().triangles;
        assert!(near > 4 * mid, "{near} against {mid}");
    }

    #[test]
    fn the_models_of_blocks_carry_macro_variation_and_the_trees_do_not() {
        let c = city();
        for item in [Item::Near(2, 3), Item::Mid(2, 3), Item::Far(0, 0)] {
            let name = format!("gen:{}", item.tail());
            let text = c.prefab(&name).unwrap_or_else(|| panic!("{name}"));
            assert!(text.contains("\"MacroVary\""), "{name}: {text}");
        }
        // (the cards of a block's trees are cut-outs: leaves are not patched)
        let name = format!("gen:{}", Item::Cards(2, 3).tail());
        let cards = c.prefab(&name).unwrap_or_else(|| panic!("{name}"));
        assert!(!cards.contains("MacroVary") && cards.contains("MeshRenderer"), "{cards}");
    }

    #[test]
    fn a_round_city_has_no_blocks_and_no_far_cells_beyond_its_radius() {
        let c = city().with_dome(Some(300.0));
        let pitch = c.plan.pitch();
        let (near, far) = ((0, 0), ((600.0 / pitch) as i32 + 2, (600.0 / pitch) as i32 + 2));
        assert!(c.plan.inside(near.0, near.1) && !c.plan.inside(far.0, far.1));
        for grid in ["b0", "b1", "t"] {
            assert!(!c.zones(grid, near.0, near.1).is_empty() || grid == "t", "{grid}: a block inside has zones");
            assert!(c.zones(grid, far.0, far.1).is_empty(), "{grid}: a block outside has none");
        }
        assert!(!c.zones("c2", 0, 0).is_empty());
        assert!(c.zones("c2", far.0, far.1).is_empty());
        // and the same plan without a dome has them all
        let open = city();
        assert!(!open.zones("b0", far.0, far.1).is_empty() && !open.zones("c2", far.0, far.1).is_empty());
        // the far cell that has some blocks inside still makes a model
        let edge = (300.0 / c.plan.cell_size()) as i32;
        assert!(c.plan.cell_inside(edge, 0));
        let made = c.make(&Item::Far(edge, 0).path()).unwrap();
        assert!(made.triangles > 0);
    }

    #[test]
    fn a_baked_block_is_served_from_its_file_and_collided_with_by_its_shell_and_the_others_are_made_as_usual() {
        let dir = std::env::temp_dir().join(format!("lunaris_baked_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lib = Lib::from_json("");
        let mut mb = MeshBuilder::new();
        mb.quad(lib.id("far_flat"), [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]], [[0.0; 2]; 4], [[1.0; 3]; 4], [0.0, 1.0, 0.0]);
        let glb = gltf::to_glb(&mb, &lib).unwrap();
        let probe = city().with_polar(true);
        // a block of the second band that has buildings (and so doors), and one that has too
        let with_lots: Vec<i32> = (0..32).filter(|s| !probe.plan.lots(2, *s).is_empty()).collect();
        let (baked, other) = (with_lots[0], with_lots[1]);
        std::fs::write(dir.join(format!("b0_2_{baked}.glb")), &glb).unwrap();
        std::fs::write(dir.join("b1_2_0.glb"), &glb).unwrap();
        let c = city().with_polar(true).with_baked(&dir);
        let item = Item::Near(2, baked);
        let made = c.make(&item.path()).unwrap();
        assert_eq!(made.glb.as_ref().unwrap().as_ref(), &glb, "the file's bytes are what is served");
        assert!(!made.doors.is_empty() && !made.door_ents.is_empty(), "the street doors are still the engine's");
        // its prefab: the model without a collider of its own, and the shell as the collider; its zone asks for the shell's file too
        let prefab = c.prefab(&format!("gen:{}", item.tail())).unwrap();
        assert!(prefab.contains("\"Shell\"") && prefab.contains(&format!("gen_s_2_{baked}")), "{prefab}");
        assert!(!prefab.contains(&format!("\"MeshCollider\":{{\"mesh\":\"{}\"", item.path())), "the baked model is a picture only");
        let zone = c.zone(item, c.block_centre(2, baked), 100.0, 0.0, 10.0, 0.0, 0.0);
        assert!(zone.assets.contains(&Item::Shell(2, baked).path()));
        // level 1 of block (2, 0) is baked: its prefab has the engine's tree cards beside it
        let mid = c.prefab("gen:b1_2_0").unwrap();
        assert!(mid.contains("gen_k_2_0"), "{mid}");
        assert!(c.make(&Item::Cards(2, 0).path()).is_ok());
        // a block with no file is made as before (with its collider in its own model)
        let made2 = c.make(&Item::Near(2, other).path()).unwrap();
        assert!(made2.triangles > 100);
        let prefab2 = c.prefab(&format!("gen:b0_2_{other}")).unwrap();
        assert!(prefab2.contains("MeshCollider") && !prefab2.contains("\"Shell\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_servers_shell_of_a_block_is_a_small_file_of_walls() {
        let c = city();
        let shell = c.make(&Item::Shell(2, 3).path()).unwrap();
        let near = c.make(&Item::Near(2, 3).path()).unwrap();
        assert!(shell.triangles > 30, "{} triangles", shell.triangles);
        assert!(near.triangles > 5 * shell.triangles, "the shell {} against the block {}", shell.triangles, near.triangles);
        assert!(shell.glb.as_ref().unwrap().len() < near.glb.as_ref().unwrap().len() / 4);
        // the grid of a server, its zone and its prefab (a visual city has no such zone: the grids of a role are its own)
        assert!(c.zones("s", 2, 3).is_empty());
        let c = city().with_role(Role::Server).with_parts(crate::serverside::Parts { door_kind: 35, ..Default::default() });
        let g = server_grids(&c.plan);
        assert_eq!(g[0].name, "s");
        let z = c.zones("s", 2, 3);
        assert_eq!(z.len(), 1);
        assert_eq!(z[0].prefab, "gen:s_2_3");
        let text = c.prefab(&z[0].prefab).expect("a prefab");
        assert!(text.contains("MeshCollider") && !text.contains("MeshRenderer"), "{text}");
        // the shell has no doors: they are with the buildings, which are nearer
        assert!(!text.contains("\"Door\""), "the shell has no door entities");
        // the street doors of the block, which the server owns, are in the zones of its buildings' ground floors (so that they are entities only while somebody is near the building): the number the outsides make, standard
        // components, told to the players as picture 35, none of them a model
        let lib = Lib::from_json("");
        let wanted: usize = c.plan.lots(2, 3).iter().map(|lp| building::exterior_with(&mut MeshBuilder::new(), &lib, &spec_of(&lib, &lp.lot, lp.kind, lp.key), false).0.len()).sum();
        let mut found = 0;
        for zn in c.zones("r", 2, 3) {
            let doc = lunaris_json::parse(&c.prefab(&zn.prefab).expect("a prefab")).expect("json");
            let Some(lunaris_json::Json::Arr(kids)) = doc.get("children") else { panic!("children") };
            for k in kids {
                let comps = k.get("components").unwrap();
                if comps.get("Door").is_some() && zn.prefab.ends_with("_0") {
                    found += 1;
                    assert_eq!(comps.get("NetEntity").and_then(|n| n.get("kind")).and_then(|k| k.as_i64()), Some(35));
                    assert!(comps.get("Interactable").is_some() && comps.get("Collider").is_some() && comps.get("MeshRenderer").is_none());
                }
            }
        }
        assert!(wanted > 0 && found >= wanted, "the block has {wanted} street doors and its ground floors' zones carry {found} doors");
        // a player's city has none of them
        let player = city().with_role(Role::Client);
        let near = player.prefab("gen:b0_2_3").expect("a prefab");
        assert!(!near.contains("\"Door\"") && !near.contains("NetEntity"), "a client's city has no door entities");
    }

    #[test]
    fn a_servers_street_doorways_carry_build_sockets_only_when_the_game_asks_for_them() {
        let lib = Lib::from_json("");
        let sockets_of = |c: &City| -> (usize, usize) {
            let (mut sockets, mut doors) = (0, 0);
            for zn in c.zones("r", 2, 3) {
                let doc = lunaris_json::parse(&c.prefab(&zn.prefab).expect("a prefab")).expect("json");
                let Some(Json::Arr(kids)) = doc.get("children") else { panic!("children") };
                for k in kids {
                    let comps = k.get("components").unwrap();
                    if let Some(b) = comps.get("BuildSocket") {
                        sockets += 1;
                        assert!(zn.prefab.ends_with("_0"), "street doorways are with the ground floors: {}", zn.prefab);
                        assert_eq!(b.get("kind").and_then(|k| k.as_str()), Some("edge"));
                        assert_eq!(b.get("anchor"), Some(&Json::Bool(true)));
                        assert!(comps.get("LocalTransform").is_some());
                    }
                    if comps.get("Door").is_some() && zn.prefab.ends_with("_0") {
                        doors += 1;
                    }
                }
            }
            (sockets, doors)
        };
        let with = city().with_role(Role::Server).with_parts(crate::serverside::Parts { door_kind: 35, door_socket: "edge".into(), ..Default::default() });
        let wanted: usize = with.plan.lots(2, 3).iter().map(|lp| building::exterior_with(&mut MeshBuilder::new(), &lib, &spec_of(&lib, &lp.lot, lp.kind, lp.key), false).0.len()).sum();
        let (sockets, doors) = sockets_of(&with);
        assert!(wanted > 0 && sockets == wanted, "the block has {wanted} street doors and {sockets} sockets ({doors} doors on its ground floors)");
        let without = city().with_role(Role::Server).with_parts(crate::serverside::Parts { door_kind: 35, ..Default::default() });
        assert_eq!(sockets_of(&without).0, 0, "a game that names no door_socket gets none");
    }

    #[test]
    fn a_servers_rooms_are_partitions_with_doors_and_things_to_search_loaded_by_building() {
        let parts = crate::serverside::Parts { door_kind: 35, container_kind: 31, tables: crate::interior::SEARCH_ROOMS.iter().map(|r| (r.to_string(), "crate".to_string())).collect(), refill: 900.0, ..Default::default() };
        let c = city().with_role(Role::Server).with_parts(parts);
        let (mut zones, mut doors, mut boxes, mut walls) = (0, 0, 0, 0);
        for (i, j) in [(2, 3), (0, 0), (1, 1), (-2, 4)] {
            for z in c.zones("r", i, j) {
                let text = c.prefab(&z.prefab).expect("a prefab");
                let doc = lunaris_json::parse(&text).expect("json");
                let Some(Json::Arr(kids)) = doc.get("children") else { panic!("children") };
                for k in kids {
                    let comps = k.get("components").unwrap();
                    if comps.get("Door").is_some() {
                        doors += 1;
                        assert!(comps.get("NetEntity").is_some() && comps.get("MeshRenderer").is_none());
                    }
                    if let Some(l) = comps.get("LootContainer") {
                        boxes += 1;
                        assert_eq!(l.get("table").and_then(|t| t.as_str()), Some("crate"));
                        assert!(comps.get("Inventory").is_some() && comps.get("Interactable").is_some());
                    }
                    if comps.get("MeshCollider").is_some() {
                        walls += 1;
                    }
                }
                zones += 1;
            }
        }
        assert!(zones > 10, "{zones} zones");
        assert!(doors > 0 && boxes > 0 && walls > 0, "{doors} doors, {boxes} containers, {walls} partition models");
        // only a server has them
        assert!(city().with_role(Role::Client).zones("r", 2, 3).is_empty());
        assert!(city().zones("r", 2, 3).is_empty());
    }

    #[test]
    fn the_zones_of_a_block_name_files_the_provider_can_make_and_prefabs_that_use_them() {
        let c = city();
        let mut seen = 0;
        for grid in ["b0", "b1", "c2", "in", "t"] {
            for z in c.zones(grid, 4, -3) {
                let item = Item::from_tail(z.prefab.strip_prefix("gen:").unwrap()).unwrap();
                assert!(z.assets.starts_with(&format!("model {}", item.path())) || matches!(item, Item::Things(..)));
                let text = c.prefab(&z.prefab).expect("a prefab");
                let doc = lunaris_json::parse(&text).expect("json");
                assert!(doc.get("children").is_some() || matches!(item, Item::Inside(..)));
                assert!(z.radius > 0.0);
                seen += 1;
            }
        }
        assert!(seen > 10, "{seen} zones");
        assert!(c.zones("nothing", 0, 0).is_empty());
    }

    #[test]
    fn the_inside_of_a_building_is_made_around_its_zone_and_comes_from_the_cache_the_second_time() {
        let c = city();
        let (i, j) = (3, 5);
        let zones = c.zones("in", i, j);
        assert!(zones.len() >= 8, "{} zones", zones.len());
        let z = zones.iter().find(|z| z.pos[1] < 8.0).unwrap();
        let item = Item::from_tail(z.prefab.strip_prefix("gen:").unwrap()).unwrap();
        let first = c.made(&item.path()).unwrap();
        assert!(first.triangles > 100, "{} triangles", first.triangles);
        let before = c.stats().files;
        let second = c.made(&item.path()).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(c.stats().files, before);
        assert!(Provider::file_ready(&c, &item.path()));
        // the lamps are inside the zone's radius
        for (p, range, _) in &first.lamps {
            assert!(p[0].abs() < z.radius && p[2].abs() < z.radius && *range > 1.0);
        }
        // the same name is the same bytes in a second city of the same seed
        let other = city().file(&item.path()).unwrap().unwrap();
        assert_eq!(&other, first.glb.as_ref().unwrap().as_ref());
    }

    #[test]
    fn a_background_prefetch_is_waited_for_not_made_twice() {
        let c = Arc::new(city());
        let path = Item::Mid(1, 1).path();
        c.prefetch_file(&path);
        assert!(!Provider::file_ready(&*c, &path));
        // no worker takes it: the asker takes it out of the queue and makes it itself
        let a = c.file(&path).unwrap().unwrap();
        assert!(Provider::file_ready(&*c, &path));
        assert_eq!(c.stats().files, 1);
        assert!(!a.is_empty());
    }
}

/// Let the calling thread give way to the one that draws the pictures: a worker making a block of the city must not take the processor from the next picture (measured: with the workers at normal priority and
/// half the cores of the machine busy, every picture that a streamed block arrived in took twice as long). Linux: a nice value of 10 for this thread; Windows: below normal.
fn background_priority() {
    #[cfg(target_os = "linux")]
    {
        extern "C" {
            fn setpriority(which: i32, who: u32, prio: i32) -> i32;
        }
        // SAFETY: a plain system call with integers; `who` 0 is the calling thread.
        let _ = unsafe { setpriority(0, 0, 10) };
    }
    #[cfg(windows)]
    {
        extern "system" {
            fn GetCurrentThread() -> isize;
            fn SetThreadPriority(thread: isize, priority: i32) -> i32;
        }
        // SAFETY: the pseudo handle of the calling thread and THREAD_PRIORITY_BELOW_NORMAL.
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), -1) };
    }
}
