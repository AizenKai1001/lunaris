//! The city generator's command line.
//!
//!     citygen swatch ROOT         a row of the photographed materials on a wall, to see how the engine draws them (ROOT/city/swatch.gltf, ROOT/swatch.lunaris.json)
//!     citygen props ROOT          every prop model of ROOT/city/props in a grid with its size and triangle count (ROOT/city/showroom.gltf, ROOT/showroom.lunaris.json)
//!     citygen district ROOT [NX NZ SEED]    a grid of NX x NZ blocks with streets and buildings (ROOT/city/district.gltf, ROOT/district.lunaris.json)
//!     citygen city ROOT [SEED [EXTENT [HOUR [DAY_SECONDS]]]]      the scenes of an endless city streamed from the procedural provider (ROOT/city.lunaris.json, ROOT/city_view.lunaris.json, ROOT/city/city.json)
//!     citygen cityinfo ROOT [SEED]           time the making of some blocks, cells and insides
//!
//! ROOT is the workspace folder the engine is run on (`lunaris serve --root ROOT`); ROOT/city/textures/materials.json comes from tools/citygen/fetch_textures.py.

use lunaris_citygen::materials::Lib;
use lunaris_citygen::mesh::MeshBuilder;
use lunaris_citygen::rng::Rng;
use lunaris_citygen::{gltf, scene};
use std::path::{Path, PathBuf};

/// The plan of the workspace's city: the seed, and (when `city/city.json` says so, as a game's does) the round plan.
fn plan_of(root: &Path, seed: u64) -> lunaris_citygen::plan::Plan {
    let mut plan = lunaris_citygen::plan::Plan::new(seed);
    if let Ok(text) = std::fs::read_to_string(root.join("city").join("city.json")) {
        if let Ok(doc) = lunaris_json::parse(&text) {
            if matches!(doc.get("polar"), Some(lunaris_json::Json::Bool(true))) {
                plan = plan.with_polar(Some(lunaris_citygen::polar::Polar::default())).with_ruins(&lunaris_citygen::plan::ruins_from_json(&doc));
            }
        }
    }
    plan
}

fn library(root: &Path) -> Lib {
    let text = std::fs::read_to_string(root.join("city").join("textures").join("materials.json")).unwrap_or_default();
    Lib::from_json(&text)
}

fn write_scene(root: &Path, name: &str, look: &scene::Look, model: &str, cam: Json2) {
    let mut ents = scene::base_entities(look);
    ents.push(scene::camera(cam.pos, cam.yaw, cam.pitch, 4000.0));
    ents.push(scene::model(model));
    let doc = scene::document(ents);
    std::fs::write(root.join(format!("{name}.lunaris.json")), doc.to_string_pretty().expect("scene json")).expect("write scene");
}

struct Json2 {
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
}

fn swatch(root: &Path) {
    let lib = library(root);
    let mut rng = Rng::new(1);
    let mut m = MeshBuilder::new();
    let names = ["brick_wall_001", "brick_wall_003", "concrete_wall_007", "beige_wall_001", "concrete_slab_wall_02", "concrete_tile_facade", "grey_roof_01", "asphalt_04", "glass_dark", "glass_blue"];
    for (i, name) in names.iter().enumerate() {
        let id = lib.id_or(name, &[]);
        let tex = lib.tex(id, &mut rng);
        let x0 = i as f32 * 3.2;
        let f = lunaris_citygen::mesh::Frame { o: [x0, 0.0, 0.0], a: [1.0, 0.0, 0.0], b: [0.0, 1.0, 0.0], c: [0.0, 0.0, 1.0] };
        m.wall_rect(id, &f, 0.0, 3.0, 0.0, 3.0, 0.0, &tex, [1.0; 3], [1.0; 3]);
    }
    let ground = lib.id("asphalt_04");
    let gt = lib.tex_plain(ground);
    m.quad_tex(ground, [[-6.0, 0.0, -2.0], [40.0, 0.0, -2.0], [40.0, 0.0, 20.0], [-6.0, 0.0, 20.0]], [[-6.0, -2.0], [40.0, -2.0], [40.0, 20.0], [-6.0, 20.0]], &gt, [1.0; 3], [0.0, 1.0, 0.0]);
    let tris = gltf::write(&root.join("city"), "swatch", &m, &lib).expect("write model");
    write_scene(root, "swatch", &scene::Look::default(), "city/swatch.gltf", Json2 { pos: [16.0, 1.7, 9.0], yaw: 0.0, pitch: -2.0 });
    println!("swatch: {} materials, {tris} triangles", names.len());
}

fn props_showroom(root: &Path) {
    let mut lib = library(root);
    let props = lunaris_citygen::props::Props::load_dir(&mut lib, &root.join("city").join("props"), &|_| 20_000);
    let mut names: Vec<&String> = props.map.keys().collect();
    names.sort();
    let mut mb = MeshBuilder::new();
    let cols = 10usize;
    let mut total = 0usize;
    for (i, n) in names.iter().enumerate() {
        let p = &props.map[*n];
        let sz = p.size();
        let (cx, cz) = ((i % cols) as f32 * 6.0, (i / cols) as f32 * 6.0);
        // very large models (trees, poles) are shown at a tenth of their size
        let k = if sz[1] > 8.0 || sz[0] > 8.0 { 0.2 } else { 1.0 };
        p.place(&mut mb, [cx, 0.0, cz], 0.0, k, [1.0; 3]);
        total += p.triangles();
        println!("{:34} {:6.2} x {:5.2} x {:6.2} m  {:7} triangles", n, sz[0], sz[1], sz[2], p.triangles());
    }
    let ground = lib.id("concrete_tiles_02");
    let gt = lib.tex_plain(ground);
    let rows = names.len().div_ceil(cols) as f32;
    mb.quad_tex(ground, [[-4.0, 0.0, -4.0], [cols as f32 * 6.0, 0.0, -4.0], [cols as f32 * 6.0, 0.0, rows * 6.0], [-4.0, 0.0, rows * 6.0]], [[-4.0, -4.0], [cols as f32 * 6.0, -4.0], [cols as f32 * 6.0, rows * 6.0], [-4.0, rows * 6.0]], &gt, [1.0; 3], [0.0, 1.0, 0.0]);
    let tris = gltf::write(&root.join("city"), "showroom", &mb, &lib).expect("write model");
    write_scene(root, "showroom", &scene::Look::default(), "city/showroom.gltf", Json2 { pos: [27.0, 8.0, -14.0], yaw: 0.0, pitch: -20.0 });
    println!("showroom: {} props, {tris} triangles ({total} before scaling)", names.len());
}

fn district(root: &Path, nx: u32, nz: u32, seed: u64) {
    use lunaris_json::Json;
    let mut lib = library(root);
    let props = lunaris_citygen::props::Props::load_dir(&mut lib, &root.join("city").join("props"), &lunaris_citygen::props::indoor_budget);
    let g = lunaris_citygen::city::Grid { nx, nz, block_w: 84.0, block_d: 66.0, section: lunaris_citygen::street::Section::default(), seed, origin: [0.0, 0.0] };
    let t = std::time::Instant::now();
    let d = lunaris_citygen::city::district(&lib, &props, &g);
    let made = t.elapsed();
    let dir = root.join("city");
    // the outside of everything is one model; the inside of each building another, loaded when somebody comes near
    let tris = gltf::write(&dir, "district", &d.mesh, &lib).expect("write model");
    let n = |x: f32| Json::Float(x as f64);
    let v3 = |a: [f32; 3]| Json::arr(a.map(n));
    let mut prefabs: Vec<(String, Json)> = Vec::new();
    let mut ents = scene::base_entities(&scene::Look::default());
    ents.push(Json::obj([
        ("Transform", Json::obj([("pos", v3([0.0, 0.0, 0.0]))])),
        ("MeshRenderer", Json::obj([("mesh", Json::str("city/district.gltf")), ("color", Json::arr([1.0, 1.0, 1.0, 1.0].map(n)))])),
        ("MeshCollider", Json::obj([("mesh", Json::str("city/district.gltf"))])),
    ]));
    let (mut inside_tris, mut lights, mut doors, mut floor_files) = (0usize, 0usize, 0usize, 0usize);
    for (k, b) in d.buildings.iter().enumerate() {
        lights += b.lights.len();
        doors += b.doors.len();
        let (cx, cz) = (b.centre[0], b.centre[2]);
        for (j, (fm, mid)) in b.floors.iter().enumerate() {
            if fm.triangles() == 0 {
                continue;
            }
            let name = format!("b{k}_f{j}");
            inside_tris += gltf::write(&dir, &name, fm, &lib).expect("write floor");
            floor_files += 1;
            let span = if j + 1 < b.floors.len() { b.floors[j + 1].1 - mid } else { mid - b.floors[j.saturating_sub(1)].1 }.abs().max(2.8);
            let mut children = vec![Json::obj([
                ("name", Json::str("Floor")),
                ("components", Json::obj([
                    ("LocalTransform", Json::obj([("pos", v3([-cx, -mid, -cz]))])),
                    ("MeshRenderer", Json::obj([("mesh", Json::str(format!("city/{name}.gltf"))), ("color", Json::arr([1.0, 1.0, 1.0, 1.0].map(n)))])),
                    ("MeshCollider", Json::obj([("mesh", Json::str(format!("city/{name}.gltf")))])),
                ])),
            ])];
            for l in b.lights.iter().filter(|l| l.floor as usize == j) {
                children.push(Json::obj([
                    ("name", Json::str("Lamp")),
                    ("components", Json::obj([
                        ("LocalTransform", Json::obj([("pos", v3([l.pos[0] - cx, l.pos[1] - mid, l.pos[2] - cz]))])),
                        ("Light", Json::obj([("kind", Json::from(1)), ("color", v3([1.0, 0.86, 0.68])), ("intensity", n(l.intensity)), ("range", n(l.range))])),
                    ])),
                ]));
            }
            let pname = format!("Inside_{k}_{j}");
            prefabs.push((pname.clone(), Json::obj([("components", Json::obj([("Transform", Json::obj([("pos", v3([0.0, 0.0, 0.0]))]))])), ("children", Json::Arr(children))])));
            ents.push(Json::obj([
                ("Name", Json::obj([("value", Json::str(format!("inside {name}")))])),
                ("Transform", Json::obj([("pos", v3([cx, *mid, cz]))])),
                ("StreamZone", Json::obj([("prefab", Json::str(pname)), ("radius", n(b.radius + 28.0)), ("hysteresis", n(12.0)), ("prefetch", n(30.0)), ("vertical", n(span * 1.7)), ("assets", Json::str(format!("model city/{name}.gltf")))])),
            ]));
        }
    }
    // two scenes: one with a fixed camera (for pictures: tools/citygen/hl_views.sh moves it), one with a first-person walker standing before the first door, looking in (for `lunaris play`)
    let first_door = d.buildings.iter().find_map(|b| b.doors.first().copied());
    let numbered = |list: Vec<Json>| -> Vec<Json> { list.into_iter().enumerate().map(|(i, c)| Json::obj([("uid", Json::from(i + 1)), ("components", c)])).collect() };
    let view_doc = {
        let mut e = ents.clone();
        e.push(scene::camera([30.0, 1.7, 8.0], 0.0, 0.0, 4000.0));
        Json::obj([("lunaris", Json::from(1)), ("prefabs", Json::Obj(prefabs.clone())), ("entities", Json::Arr(numbered(e)))])
    };
    std::fs::write(root.join("district.lunaris.json"), view_doc.to_string_compact().expect("scene json")).expect("write scene");
    // the walker: the engine's first-person example supplies the graph systems that turn the keys and the mouse into walking
    let fp = lunaris_json::parse(include_str!("../../../../examples/first_person.lunaris.json")).expect("first person example");
    let mut e = ents.clone();
    let (spawn, yaw) = match first_door {
        Some(dr) => {
            let mid = [dr.hinge[0] + dr.along[0] * dr.width * 0.5, dr.hinge[1], dr.hinge[2] + dr.along[2] * dr.width * 0.5];
            ([mid[0] + dr.out[0] * 5.0, mid[1] + 0.05, mid[2] + dr.out[2] * 5.0], dr.out[0].atan2(dr.out[2]))
        }
        None => ([30.0, 0.2, 5.0], 0.0),
    };
    let base = e.len() as i64;
    e.push(Json::obj([("Name", Json::obj([("value", Json::str("Player"))])), ("Hero", Json::Obj(Vec::new())), ("Transform", Json::obj([("pos", v3(spawn))])), ("CharacterBody", Json::obj([("radius", n(0.35)), ("height", n(1.8)), ("step_height", n(0.4))]))]));
    e.push(Json::obj([
        ("Name", Json::obj([("value", Json::str("Eyes"))])),
        ("Parent", Json::obj([("target", Json::from(base + 1))])),
        ("LocalTransform", Json::obj([("pos", v3([0.0, 1.62, 0.0]))])),
        ("Transform", Json::obj([("pos", v3([spawn[0], spawn[1] + 1.62, spawn[2]]))])),
        ("Camera", Json::obj([("fov_y", n(72.0)), ("far", n(4000.0))])),
        ("MouseLook", Json::obj([("sensitivity", n(0.0022)), ("yaw", n(yaw))])),
    ]));
    e.push(Json::obj([("Name", Json::obj([("value", Json::str("Input"))])), ("InputMap", Json::obj([("bindings", Json::str("move_x = d, a:-1, gamepad.lx\nmove_y = w, s:-1, gamepad.ly\njump = space, gamepad.a\ndeadzone = 0.15"))]))]));
    let mut walk: Vec<(String, Json)> = vec![("lunaris".to_string(), Json::from(1)), ("prefabs".to_string(), Json::Obj(prefabs.clone())), ("entities".to_string(), Json::Arr(numbered(e)))];
    for k in ["components", "resources", "systems"] {
        if let Some(v) = fp.get(k) {
            walk.push((k.to_string(), v.clone()));
        }
    }
    std::fs::write(root.join("district_walk.lunaris.json"), Json::Obj(walk).to_string_compact().expect("scene json")).expect("write scene");
    let (lo, hi) = d.mesh.bounds().unwrap();
    for (k, b) in d.buildings.iter().enumerate().take(6) {
        if let Some(dr) = b.doors.first() {
            let mid = [dr.hinge[0] + dr.along[0] * dr.width * 0.5, dr.hinge[1], dr.hinge[2] + dr.along[2] * dr.width * 0.5];
            println!("building b{k}: door at ({:.1}, {:.2}, {:.1}) out ({:.2}, {:.2}), centre ({:.0}, {:.0}), {:.0} m tall, {} floors, {} triangles inside", mid[0], mid[1], mid[2], dr.out[0], dr.out[2], b.centre[0], b.centre[2], b.height, b.floors.len(), b.floors.iter().map(|f| f.0.triangles()).sum::<usize>());
        }
    }
    println!(
        "district {nx}x{nz} blocks, seed {seed}: outside {tris} triangles in {} materials; {} buildings, {floor_files} floors with {inside_tris} triangles inside, {doors} doors, {lights} lights; made in {:.2} s; extent x {:.0}..{:.0} z {:.0}..{:.0} up to {:.0} m",
        d.mesh.parts.iter().filter(|p| !p.idx.is_empty()).count(),
        d.buildings.len(),
        made.as_secs_f32(),
        lo[0],
        hi[0],
        lo[2],
        hi[2],
        hi[1]
    );
}


/// The scenes of the endless city: `ROOT/city/city.json` (the seed; the `lunaris` program installs the city as the procedural provider when it finds it), `ROOT/city.lunaris.json` (a first-person walker on the street
/// before a door) and `ROOT/city_view.lunaris.json` (the same with a fixed camera, for pictures).
fn city_scenes(root: &Path, seed: u64, extent: f32, hour: f32, day_seconds: f32) {
    use lunaris_citygen::{building, plan::Plan, runtime};
    use lunaris_json::Json;
    let lib = {
        let text = std::fs::read_to_string(root.join("city").join("textures").join("materials.json")).unwrap_or_default();
        Lib::from_json(&text)
    };
    let plan = Plan::new(seed);
    std::fs::create_dir_all(root.join("city")).expect("create city folder");
    std::fs::write(root.join("city").join("city.json"), format!("{{\"seed\":{seed}}}\n")).expect("write city.json");
    // somewhere to stand: the nearest downtown to the origin, and in it a building (not a warehouse) that has a door on the street, 6 m in front of it, looking at it
    let mut centre = (0, 0);
    let mut best = f32::MAX;
    for j in -60..=60 {
        for i in -60..=60 {
            let d = ((i * i + j * j) as f32).sqrt();
            if d < best && plan.landuse(i, j) == lunaris_citygen::plan::LandUse::Downtown {
                best = d;
                centre = (i, j);
            }
        }
    }
    let mut spawn = None;
    'find: for ring in 0..8i32 {
        for j in centre.1 - ring..=centre.1 + ring {
            for i in centre.0 - ring..=centre.0 + ring {
                if (i - centre.0).abs().max((j - centre.1).abs()) != ring || plan.landuse(i, j) == lunaris_citygen::plan::LandUse::Park {
                    continue;
                }
                for lp in plan.lots(i, j) {
                    if matches!(lp.kind, building::Kind::Warehouse | building::Kind::House) {
                        continue;
                    }
                    let spec = building::spec_of(&lib, &lp.lot, lp.kind, lp.key);
                    let doors = building::exterior(&mut MeshBuilder::new(), &lib, &spec);
                    if let Some(dr) = doors.first() {
                        let mid = [dr.hinge[0] + dr.along[0] * dr.width * 0.5, dr.hinge[1], dr.hinge[2] + dr.along[2] * dr.width * 0.5];
                        spawn = Some(([mid[0] + dr.out[0] * 6.0, mid[1] + 0.05, mid[2] + dr.out[2] * 6.0], dr.out[0].atan2(dr.out[2]), (i, j, lp.kind)));
                        break 'find;
                    }
                }
            }
        }
    }
    let (spawn, yaw, (bi, bj, kind)) = spawn.expect("a door somewhere near the middle");
    let n = |x: f32| Json::Float(x as f64);
    let v3 = |a: [f32; 3]| Json::arr(a.map(n));
    // a light haze that takes the city's far edge (4 km) out of sight, and a ground beyond it of the haze's colour
    let look = scene::Look { fog: 0.0006, fog_color: [0.70, 0.78, 0.88], ground: [0.60, 0.66, 0.72], shadow_distance: 150.0, ..scene::Look::default() };
    let mut ents = scene::base_entities(&look);
    for g in runtime::grids(&plan) {
        ents.push(Json::obj([
            ("Name", Json::obj([("value", Json::str(format!("grid {}", g.name)))])),
            ("ProcGrid", Json::obj([("grid", Json::str(g.name)), ("cell", n(g.cell)), ("radius", n(g.radius)), ("hysteresis", n(g.hysteresis)), ("extent", n(extent)), ("per_tick", Json::from(g.per_tick as i64))])),
        ]));
    }
    // the hour of the day (the sun, the sky and the haze follow it; with day_seconds above 0 the day goes round)
    ents.push(Json::obj([("Name", Json::obj([("value", Json::str("Time of day"))])), ("TimeOfDay", Json::obj([("time", n(hour)), ("day_seconds", n(day_seconds)), ("azimuth", n(0.6)), ("sun_intensity", n(3.0)), ("night_intensity", n(0.0))]))]));
    // a safety floor: a box under the whole city, so that nobody falls through while a block's own ground is still being made
    ents.push(Json::obj([
        ("Name", Json::obj([("value", Json::str("Bedrock"))])),
        ("Transform", Json::obj([("pos", v3([0.0, -1.5, 0.0]))])),
        ("Collider", Json::obj([("shape", Json::from(1)), ("size", v3([extent * 2.0 + 4000.0, 3.0, extent * 2.0 + 4000.0]))])),
    ]));
    let numbered = |list: Vec<Json>| -> Vec<Json> { list.into_iter().enumerate().map(|(i, c)| Json::obj([("uid", Json::from(i + 1)), ("components", c)])).collect() };
    let view = {
        let mut e = ents.clone();
        e.push(scene::camera([spawn[0], spawn[1] + 1.7, spawn[2]], yaw.to_degrees(), 0.0, 5000.0));
        let (night_components, night_systems) = night_parts();
        let doc = vec![("lunaris".to_string(), Json::from(1)), ("components".to_string(), night_components), ("entities".to_string(), Json::Arr(numbered(e))), ("resources".to_string(), Json::obj([("stream.loads_per_tick", Json::from(3)), ("night", n(0.0))])), ("systems".to_string(), Json::Arr(night_systems))];
        Json::Obj(doc)
    };
    std::fs::write(root.join("city_view.lunaris.json"), view.to_string_compact().expect("scene json")).expect("write scene");
    let fp = lunaris_json::parse(include_str!("../../../../examples/first_person.lunaris.json")).expect("first person example");
    let mut e = ents;
    let base = e.len() as i64;
    e.push(Json::obj([("Name", Json::obj([("value", Json::str("Player"))])), ("Hero", Json::Obj(Vec::new())), ("Transform", Json::obj([("pos", v3(spawn))])), ("CharacterBody", Json::obj([("radius", n(0.35)), ("height", n(1.8)), ("step_height", n(0.4))]))]));
    e.push(Json::obj([
        ("Name", Json::obj([("value", Json::str("Eyes"))])),
        ("Parent", Json::obj([("target", Json::from(base + 1))])),
        ("LocalTransform", Json::obj([("pos", v3([0.0, 1.62, 0.0]))])),
        ("Transform", Json::obj([("pos", v3([spawn[0], spawn[1] + 1.62, spawn[2]]))])),
        ("Camera", Json::obj([("fov_y", n(72.0)), ("far", n(5000.0))])),
        ("MouseLook", Json::obj([("sensitivity", n(0.0022)), ("yaw", n(yaw))])),
        // the eyes' reach: a ray 7 m ahead names what the player looks at (`interact_look` takes what is within 3.2 m; building aims with all of it)
        ("Probe", Json::obj([("up", n(0.1)), ("depth", n(1.0)), ("ray_from", v3([0.0, 0.0, 0.0])), ("ray_dir", v3([0.0, 0.0, -1.0])), ("ray_max", n(7.0)), ("ray_local", Json::Bool(true))])),
    ]));
    e.push(Json::obj([("Name", Json::obj([("value", Json::str("Input"))])), ("InputMap", Json::obj([("bindings", Json::str("move_x = d, a:-1, gamepad.lx\nmove_y = w, s:-1, gamepad.ly\njump = space, gamepad.a\ninteract = e, gamepad.x\ninventory = tab\nclose = escape\nsave = f5\nload = f9\nweather = f6\nflashlight = f\ncraft = c\nbuild = b\npiece1 = 1\npiece2 = 2\npiece3 = 3\npiece4 = 4\npiece5 = 5\npiece6 = 6\ndemolish = x\ndeadzone = 0.15"))]))]));
    // the HUD: the controls at the top, the position at the bottom (the engine's pixel font)
    let hud = |value: &str, pos: [f32; 2], size: f32| Json::obj([("Name", Json::obj([("value", Json::str("HUD"))])), ("Text", Json::obj([("value", Json::str(value)), ("pos", Json::arr(pos.map(n))), ("size", n(size)), ("color", Json::arr([1.0, 1.0, 1.0, 0.85].map(n)))]))]);
    e.push(hud("WASD walk   mouse look   Space jump", [0.015, 0.02], 0.028));
    e.push(hud("X {pos_x}   Z {pos_z}   BLOCK {block_i}, {block_j}", [0.015, 0.95], 0.028));
    e.push(hud("{clock_h1}{clock_h2}:{clock_m1}{clock_m2}", [0.92, 0.02], 0.028));
    // the line under the crosshair when the player looks at a door
    e.push(Json::obj([
        ("Name", Json::obj([("value", Json::str("Prompt door"))])),
        ("Text", Json::obj([("value", Json::str("E   open / close")), ("pos", Json::arr([0.43, 0.58].map(n))), ("size", n(0.032)), ("color", Json::arr([1.0, 1.0, 1.0, 0.0].map(n)))])),
        ("Prompt", Json::obj([("kind", n(1.0))])),
    ]));
    // the scene is assembled twice: once with placeholder save slots, to get a number from its text, and then with the slots that number makes (a quick slot 10 to 49 and an auto slot 50 to 98), so that a save of an
    // older scene, with its older systems and entities in it, is never loaded over a newer one (the engine saves the whole world, systems included)
    let eyes_uid = base as u64 + 2;
    let assemble = |slots: [f64; 2]| -> String {
        // the backpack, the containers to search and the loot (`survival.rs`): its entities come last, so their uids are known
        let mut e = e.clone();
        let surv = lunaris_citygen::survival::build(e.len() as u64 + 1, spawn, slots, eyes_uid);
        e.extend(surv.entities.iter().cloned());
        let weather = lunaris_citygen::weather::build();
        e.extend(weather.entities.iter().cloned());
        let mut walk: Vec<(String, Json)> = vec![("lunaris".to_string(), Json::from(1)), ("entities".to_string(), Json::Arr(numbered(e)))];
        let (night_components, night_systems) = night_parts();
        let mut comps: Vec<Json> = fp.get("components").and_then(|v| v.as_array().map(|a| a.to_vec())).unwrap_or_default();
        comps.extend(night_components.as_array().map(|a| a.to_vec()).unwrap_or_default());
        comps.extend(surv.components.iter().cloned());
        comps.extend(weather.components.iter().cloned());
        walk.push(("components".to_string(), Json::Arr(comps)));
        // the first-person example's systems, and one that writes where the player is into resources for the HUD text (the block the player is in counts from the world's origin: 102.2 m a block)
        let mut systems: Vec<Json> = fp.get("systems").and_then(|v| v.as_array().map(|a| a.to_vec())).unwrap_or_default();
        let pitch = Plan::new(seed).pitch();
        let get = |k: &str| Json::obj([("get", Json::str(k))]);
        systems.push(Json::obj([
            ("name", Json::str("hud_position")),
            ("doc", Json::str("Where the player is, for the text in the corner.")),
            ("each", Json::arr([Json::str("Hero"), Json::str("Transform")])),
            ("order", Json::from(20)),
            ("do", Json::arr([
                Json::obj([("res_set", Json::str("pos_x")), ("to", Json::obj([("round", get("Transform.pos[0]"))]))]),
                Json::obj([("res_set", Json::str("pos_z")), ("to", Json::obj([("round", get("Transform.pos[2]"))]))]),
                Json::obj([("res_set", Json::str("block_i")), ("to", Json::obj([("floor", Json::obj([("div", Json::arr([get("Transform.pos[0]"), n(pitch)]))]))]))]),
                Json::obj([("res_set", Json::str("block_j")), ("to", Json::obj([("floor", Json::obj([("div", Json::arr([get("Transform.pos[2]"), n(pitch)]))]))]))]),
            ])),
            ("enabled", Json::Bool(true)),
        ]));
        systems.extend(night_systems);
        systems.extend(surv.systems.iter().cloned());
        systems.extend(weather.systems.iter().cloned());
        walk.push(("systems".to_string(), Json::Arr(systems)));
        let mut res: Vec<(String, Json)> = fp.get("resources").and_then(|r| r.as_object().map(|o| o.to_vec())).unwrap_or_default();
        // one zone's prefab a tick at most: three at once made a picture that arrived in a block 10 ms slower than the others (a block's things alone are 200 entities)
        res.push(("stream.loads_per_tick".to_string(), Json::from(1)));
        res.push(("night".to_string(), n(0.0)));
        res.extend(surv.resources.iter().cloned());
        res.extend(weather.resources.iter().cloned());
        for k in ["interact_uid", "interact_kind", "interact_press", "interact_prev", "eye_x", "eye_z"] {
            res.push((k.to_string(), n(0.0)));
        }
        walk.push(("prefabs".to_string(), Json::Obj(surv.prefabs.iter().cloned().collect())));
        walk.push(("resources".to_string(), Json::Obj(res)));
        Json::Obj(walk).to_string_compact().expect("scene json")
    };
    let draft = assemble([1.0, 9.0]);
    let hash = draft.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    let slots = [10.0 + (hash % 40) as f64, 50.0 + ((hash >> 24) % 49) as f64];
    std::fs::write(root.join("city.lunaris.json"), assemble(slots)).expect("write scene");
    println!("city seed {seed}: you start at ({:.1}, {:.2}, {:.1}) before a {kind:?} in block ({bi}, {bj}) looking along ({:.3}, {:.3}); extent {extent} m; save slots {} (quick) and {} (auto)", spawn[0], spawn[1], spawn[2], -yaw.sin(), -yaw.cos(), slots[0], slots[1]);
}

/// What the day going round needs of a scene: the component that marks a street lamp's light, and the systems that work out how dark it is (a resource `night`, 0 by day to 1 at night, and the clock for the
/// text in the corner), turn the lamps on at night and dim the ground beyond the end of the city with the light.
fn night_parts() -> (lunaris_json::Json, Vec<lunaris_json::Json>) {
    use lunaris_json::Json;
    let n = |x: f64| Json::Float(x);
    let get = |k: &str| Json::obj([("get", Json::str(k))]);
    let res = |k: &str| Json::obj([("res", Json::str(k))]);
    let op3 = |op: &str, a: Json, b: Json, c: Json| Json::obj([(op, Json::arr([a, b, c]))]);
    let field = |name: &str, default: f64| Json::obj([("name", Json::str(name)), ("type", Json::str("f32")), ("default", Json::Float(default))]);
    let components = Json::arr([
        Json::obj([("name", Json::str("StreetLamp")), ("doc", Json::str("The light of a street lamp: on at night.")), ("fields", Json::arr([]))]),
        Json::obj([("name", Json::str("RainCloud")), ("doc", Json::str("The entity that follows the player's head and looks straight up for a roof (the weather's `rain_cloud`); the systems that read the eyes' probe leave it out (defined here so that every scene that has those systems has it).")), ("fields", Json::arr([]))]),
        Json::obj([("name", Json::str("NightGlow")), ("doc", Json::str("A model whose lit windows glow at night (its glass pieces have an emissive factor; the entity's `emissive` says how much).")), ("fields", Json::arr([]))]),
        Json::obj([
            ("name", Json::str("Door")),
            ("doc", Json::str("A door leaf in a doorway: hinge (hx, hy, hz: the middle of the leaf at the hinge's height, relative to the block), the directions along the closed leaf (ax, az) and out of the building (ox, oz), yaw0 (the closed leaf's yaw), dir (the sense it turns), half (half its width), maxang (how far it opens, radians); open (0 to 1) follows want (0 closed, 1 open).")),
            ("fields", Json::arr([field("hx", 0.0), field("hy", 0.0), field("hz", 0.0), field("ax", 1.0), field("az", 0.0), field("ox", 0.0), field("oz", 1.0), field("yaw0", 0.0), field("dir", 1.0), field("half", 0.45), field("open", 0.0), field("want", 0.0), field("maxang", 1.75), field("locked", 0.0)])),
        ]),
        Json::obj([("name", Json::str("Prompt")), ("doc", Json::str("A line of the HUD that shows while the player looks at something of this `kind` (1 a door).")), ("fields", Json::arr([field("kind", 1.0)]))]),
    ]);
    let mut systems = Vec::new();
    let op1 = |op: &str, a: Json| Json::obj([(op, a)]);
    let op2 = |op: &str, a: Json, b: Json| Json::obj([(op, Json::arr([a, b]))]);
    let gt = |a: &str| get(&format!("Door.{a}"));
    // the leaf's distance (squared) from the eyes on the ground plane, and how far it lies ahead of where the player looks
    let (dx, dz) = (op2("sub", get("Transform.pos[0]"), res("eye_x")), op2("sub", get("Transform.pos[2]"), res("eye_z")));
    let dist2 = op2("add", op2("mul", dx.clone(), dx.clone()), op2("mul", dz.clone(), dz.clone()));
    let ahead = op2("add", op2("mul", dx, res("look.fx")), op2("mul", dz, res("look.fz")));
    // what the player is looking at: the eyes' probe ray (3.2 m) names the entity it hits
    systems.push(Json::obj([
        ("name", Json::str("interact_look")),
        ("doc", Json::str("What the player looks at, within reach: its uid in `interact_uid` (0 for nothing); and whether the interact key was pressed this tick (`interact_press`).")),
        ("each", Json::arr([Json::str("Probe"), Json::str("Transform")])),
        ("without", Json::arr([Json::str("RainCloud")])),
        ("order", Json::from(8)),
        ("do", Json::arr([
            Json::obj([("res_set", Json::str("eye_x")), ("to", get("Transform.pos[0]"))]),
            Json::obj([("res_set", Json::str("eye_z")), ("to", get("Transform.pos[2]"))]),
            Json::obj([("res_set", Json::str("eye_y")), ("to", get("Transform.pos[1]"))]),
            Json::obj([("res_set", Json::str("interact_uid")), ("to", Json::obj([("select", Json::arr([op2("and", get("Probe.hit"), op2("lt", get("Probe.hit_dist"), n(3.2))), get("Probe.hit_uid"), n(0.0)]))]))]),
            Json::obj([("res_set", Json::str("interact_kind")), ("to", n(0.0))]),
            Json::obj([("res_set", Json::str("interact_press")), ("to", Json::obj([("select", Json::arr([op2("and", op2("gt", res("action.interact"), n(0.5)), op2("lt", res("interact_prev"), n(0.5))), n(1.0), n(0.0)]))]))]),
            Json::obj([("res_set", Json::str("interact_prev")), ("to", res("action.interact"))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    // a door: it opens or shuts when it is looked at and the key is pressed, swings towards its target, and tells the HUD that it is the target
    systems.push(Json::obj([
        ("name", Json::str("door_use")),
        ("doc", Json::str("The door the player looks at: names itself the target (kind 1) and toggles when the interact key is pressed.")),
        ("each", Json::arr([Json::str("Door"), Json::str("Transform")])),
        ("order", Json::from(9)),
        ("do", Json::arr([
            Json::obj([
                ("if", op2("eq", Json::obj([("uid", Json::Bool(true))]), res("interact_uid"))),
                ("then", Json::arr([
                    // a locked door: the prompt says so (kind 8), or says that a hatchet or a crowbar in the backpack will break the lock (kind 9); E then breaks it and the door swings open
                    Json::obj([("res_set", Json::str("interact_kind")), ("to", Json::obj([("select", Json::arr([op2("gt", gt("locked"), n(0.5)), Json::obj([("select", Json::arr([op2("or", op2("gt", res("has_hatchet"), n(0.5)), op2("gt", res("has_crowbar"), n(0.5))), n(9.0), n(8.0)]))]), n(1.0)]))]))]),
                    Json::obj([("if", op2("gt", res("interact_press"), n(0.5))), ("then", Json::arr([Json::obj([
                        ("if", op2("gt", gt("locked"), n(0.5))),
                        ("then", Json::arr([Json::obj([
                            ("if", op2("or", op2("gt", res("has_hatchet"), n(0.5)), op2("gt", res("has_crowbar"), n(0.5)))),
                            ("then", Json::arr([Json::obj([("set", Json::str("Door.locked")), ("to", n(0.0))]), Json::obj([("set", Json::str("Door.want")), ("to", n(1.0))]), Json::obj([("emit", Json::str("strip"))])])),
                            ("else", Json::arr([Json::obj([("emit", Json::str("door_locked"))])])),
                        ])])),
                        ("else", Json::arr([Json::obj([
                            ("if", op2("gt", gt("want"), n(0.5))),
                            ("then", Json::arr([Json::obj([("set", Json::str("Door.want")), ("to", n(0.0))]), Json::obj([("emit", Json::str("door_close"))])])),
                            ("else", Json::arr([Json::obj([("set", Json::str("Door.want")), ("to", n(1.0))]), Json::obj([("emit", Json::str("door_open"))])])),
                        ])])),
                    ])]))]),
                ])),
            ]),
            // an open door can be shut from the doorway, looking at it: the leaf lies against the wall, the ray goes through the opening, so the open door within 4 m ahead takes the key when nothing else is the target
            Json::obj([
                ("if", op2("and", op2("and", op2("and", op2("ne", Json::obj([("uid", Json::Bool(true))]), res("interact_uid")), op2("eq", res("interact_kind"), n(0.0))), op2("gt", gt("want"), n(0.5))), op2("gt", res("interact_press"), n(0.5)))),
                ("then", Json::arr([Json::obj([
                    ("if", op2("and", op2("lt", dist2.clone(), n(18.0)), op2("gt", ahead.clone(), op2("mul", n(0.2), op1("sqrt", dist2.clone()))))),
                    ("then", Json::arr([Json::obj([("set", Json::str("Door.want")), ("to", n(0.0))]), Json::obj([("emit", Json::str("door_close"))])])),
                ])])),
            ]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    let angle = op2("mul", gt("open"), gt("maxang"));
    let rot_half = op2("mul", op2("add", gt("yaw0"), op2("mul", gt("dir"), angle.clone())), n(0.5));
    let along = |i: &str| gt(i);
    systems.push(Json::obj([
        ("name", Json::str("door_swing")),
        ("doc", Json::str("A door's leaf turns about its hinge towards `want`: its place and its yaw follow `open`.")),
        ("each", Json::arr([Json::str("Door"), Json::str("LocalTransform")])),
        ("order", Json::from(10)),
        ("do", Json::arr([
            Json::obj([("set", Json::str("Door.open")), ("to", op2("add", gt("open"), op3("clamp", op2("sub", gt("want"), gt("open")), op1("neg", op2("mul", Json::obj([("dt", Json::Bool(true))]), n(3.2))), op2("mul", Json::obj([("dt", Json::Bool(true))]), n(3.2)))))]),
            Json::obj([("set", Json::str("LocalTransform.pos[0]")), ("to", op2("add", gt("hx"), op2("mul", op2("sub", op2("mul", op1("cos", angle.clone()), along("ax")), op2("mul", op1("sin", angle.clone()), gt("ox"))), gt("half"))))]),
            Json::obj([("set", Json::str("LocalTransform.pos[2]")), ("to", op2("add", gt("hz"), op2("mul", op2("sub", op2("mul", op1("cos", angle.clone()), along("az")), op2("mul", op1("sin", angle.clone()), gt("oz"))), gt("half"))))]),
            Json::obj([("set", Json::str("LocalTransform.rot[0]")), ("to", n(0.0))]),
            Json::obj([("set", Json::str("LocalTransform.rot[1]")), ("to", op1("sin", rot_half.clone()))]),
            Json::obj([("set", Json::str("LocalTransform.rot[2]")), ("to", n(0.0))]),
            Json::obj([("set", Json::str("LocalTransform.rot[3]")), ("to", op1("cos", rot_half))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    systems.push(Json::obj([
        ("name", Json::str("prompt_show")),
        ("doc", Json::str("A prompt line shows while the player looks at something of its kind.")),
        ("each", Json::arr([Json::str("Prompt"), Json::str("Text")])),
        ("order", Json::from(11)),
        ("do", Json::arr([Json::obj([("set", Json::str("Text.color[3]")), ("to", Json::obj([("select", Json::arr([op2("eq", res("interact_kind"), get("Prompt.kind")), n(0.95), n(0.0)]))]))])])),
        ("enabled", Json::Bool(true)),
    ]));
    systems.push(Json::obj([
        ("name", Json::str("night_amount")),
        ("doc", Json::str("How dark it is (the lamps and the ground follow it) and the time as digits for the clock in the corner.")),
        ("each", Json::arr([Json::str("TimeOfDay")])),
        ("order", Json::from(5)),
        ("do", Json::arr([
            Json::obj([("res_set", Json::str("night")), ("to", Json::obj([("max", Json::arr([
                Json::obj([("smoothstep", Json::arr([n(16.6), n(19.2), get("TimeOfDay.time")]))]),
                Json::obj([("sub", Json::arr([n(1.0), Json::obj([("smoothstep", Json::arr([n(4.8), n(7.4), get("TimeOfDay.time")]))])]))]),
            ]))]))]),
            Json::obj([("res_set", Json::str("clock_h")), ("to", Json::obj([("floor", get("TimeOfDay.time"))]))]),
            Json::obj([("res_set", Json::str("clock_m")), ("to", Json::obj([("floor", Json::obj([("mul", Json::arr([Json::obj([("fract", get("TimeOfDay.time"))]), n(60.0)]))]))]))]),
            Json::obj([("res_set", Json::str("clock_h1")), ("to", Json::obj([("floor", Json::obj([("div", Json::arr([res("clock_h"), n(10.0)]))]))]))]),
            Json::obj([("res_set", Json::str("clock_h2")), ("to", Json::obj([("mod", Json::arr([res("clock_h"), n(10.0)]))]))]),
            Json::obj([("res_set", Json::str("clock_m1")), ("to", Json::obj([("floor", Json::obj([("div", Json::arr([res("clock_m"), n(10.0)]))]))]))]),
            Json::obj([("res_set", Json::str("clock_m2")), ("to", Json::obj([("mod", Json::arr([res("clock_m"), n(10.0)]))]))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    systems.push(Json::obj([
        ("name", Json::str("lamps_on")),
        ("doc", Json::str("Street lamps shine at night.")),
        ("each", Json::arr([Json::str("StreetLamp"), Json::str("Light")])),
        ("order", Json::from(6)),
        ("do", Json::arr([Json::obj([("set", Json::str("Light.intensity")), ("to", Json::obj([("mul", Json::arr([res("night"), n(2.6)]))]))])])),
        ("enabled", Json::Bool(true)),
    ]));
    systems.push(Json::obj([
        ("name", Json::str("windows_glow")),
        ("doc", Json::str("The lit windows of the blocks glow when it is dark.")),
        ("each", Json::arr([Json::str("NightGlow"), Json::str("MeshRenderer")])),
        ("order", Json::from(6)),
        ("do", Json::arr([Json::obj([("set", Json::str("MeshRenderer.emissive")), ("to", Json::obj([("mul", Json::arr([res("night"), n(1.3)]))]))])])),
        ("enabled", Json::Bool(true)),
    ]));
    // the dark is not black: the ambient light keeps a floor at night (the sky of the air is the sun's and has no moon)
    systems.push(Json::obj([
        ("name", Json::str("night_ambient")),
        ("doc", Json::str("At night the ambient light does not fall below a dim blue.")),
        ("each", Json::arr([Json::str("Environment")])),
        ("order", Json::from(7)),
        ("do", Json::arr([Json::obj([("set", Json::str("Environment.ambient")), ("to", Json::obj([("max", Json::arr([get("Environment.ambient"), op2("mul", res("night"), n(0.9))]))]))])])),
        ("enabled", Json::Bool(true)),
    ]));
    let ground = [0.60, 0.66, 0.72];
    systems.push(Json::obj([
        ("name", Json::str("ground_at_night")),
        ("doc", Json::str("The colour of the ground beyond the end of the city darkens with the light.")),
        ("each", Json::arr([Json::str("Environment")])),
        ("order", Json::from(7)),
        ("do", Json::arr((0..3).map(|k| Json::obj([("set", Json::str(format!("Environment.ground[{k}]"))), ("to", Json::obj([("mul", Json::arr([n(ground[k]), Json::obj([("sub", Json::arr([n(1.0), Json::obj([("mul", Json::arr([res("night"), n(0.94)]))])]))])]))]))])))),
        ("enabled", Json::Bool(true)),
    ]));
    (components, systems)
}

/// How long the city takes to make a few things, and how big they are.
fn city_info(root: &Path, seed: u64) {
    use lunaris_citygen::{plan::Plan, runtime::City};
    use lunaris_sim::procgen::Provider;
    let t = std::time::Instant::now();
    let city = City::open(root, seed).expect("open the city");
    println!("opened in {:.1} s (furniture and materials loaded)", t.elapsed().as_secs_f32());
    let plan = Plan::new(seed);
    let probe = |what: &str, path: String| {
        let t = std::time::Instant::now();
        let bytes = city.file(&path).and_then(|r| r.ok()).map_or(0, |b| b.len());
        println!("{what:<34} {path:<32} {:>8.1} ms {:>9} KiB", t.elapsed().as_secs_f64() * 1000.0, bytes >> 10);
    };
    for (i, j) in [(0, 0), (12, 7), (-40, 25), (900, -30)] {
        probe(&format!("block {i} {j} ({:?})", plan.landuse(i, j)), format!("city/gen_b0_{i}_{j}.glb"));
        probe("  coarse", format!("city/gen_b1_{i}_{j}.glb"));
    }
    probe("cell 0 0", "city/gen_c2_0_0.glb".to_string());
    probe("cell 25 -9", "city/gen_c2_25_-9.glb".to_string());
    for l in [0, 1, 2, 3, 5] {
        probe(&format!("inside lot {l}, floors 0-1"), format!("city/gen_in_1_1_{l}_0.glb"));
    }
    for (i, j) in [(0, 0), (3, 2)] {
        let text = city.prefab(&format!("gen:b0_{i}_{j}")).unwrap_or_default();
        let count = |what: &str| text.matches(what).count();
        for sp in lunaris_citygen::streetscape::spots(&plan, i, j).iter().filter(|s| s.thing == lunaris_citygen::streetscape::Thing::Car) {
            println!("  car at ({:.1}, {:.2}, {:.1}) yaw {:.0}", sp.pos[0], sp.pos[1], sp.pos[2], sp.yaw);
        }
        println!("prefab of block {i} {j}: {} bytes; {} trees, {} lamps, {} cars, {} litter, {} benches", text.len(), count("tree") + count("jacaranda") + count("pine_"), count("street_lamp"), count("covered_car"), count("trashbag"), count("bench") + count("seating"));
    }
    println!("{:?}", city.stats());
    for g in lunaris_citygen::runtime::grids(&plan) {
        let n: usize = (0..4).map(|k| city.zones(g.name, k, k).len()).sum();
        println!("grid {:<3} cell {:>6.1} m, radius {:>6.0} m: {} zones in 4 sample cells", g.name, g.cell, g.radius, n);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(args.get(2).cloned().unwrap_or_else(|| ".".to_string()));
    match args.get(1).map(|s| s.as_str()) {
        Some("swatch") => swatch(&root),
        Some("props") => props_showroom(&root),
        Some("district") => {
            let n = |k: usize, d: u32| args.get(k).and_then(|v| v.parse().ok()).unwrap_or(d);
            district(&root, n(3, 2), n(4, 2), args.get(5).and_then(|v| v.parse().ok()).unwrap_or(1));
        }
        Some("city") => city_scenes(&root, args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1), args.get(4).and_then(|v| v.parse().ok()).unwrap_or(60_000.0), args.get(5).and_then(|v| v.parse().ok()).unwrap_or(15.5), args.get(6).and_then(|v| v.parse().ok()).unwrap_or(0.0)),
        Some("trees") => {
            // citygen trees ROOT: every outdoor model of the props folder in a row as the city makes them (trunks simplified, crowns as clump cards), to look at them (ROOT/city/trees.gltf, ROOT/trees.lunaris.json)
            let mut lib = library(&root);
            let props = lunaris_citygen::props::Props::load_dir_with(&mut lib, &root.join("city").join("props"), &lunaris_citygen::props::outdoor_budget, &lunaris_citygen::props::outdoor_cards);
            let mut names: Vec<&String> = props.map.keys().collect();
            names.sort();
            let mut mb = MeshBuilder::new();
            let mut x = 0.0;
            for n in &names {
                let p = &props.map[*n];
                let sz = p.size();
                p.place(&mut mb, [x + sz[0] * 0.5, 0.0, 0.0], 0.0, 1.0, [1.0; 3]);
                println!("{:24} {:6.2} x {:5.2} x {:6.2} m  {:7} triangles", n, sz[0], sz[1], sz[2], p.triangles());
                x += sz[0].max(sz[2]) + 3.0;
            }
            let ground = lib.id("leafy_grass");
            let gt = lib.tex_plain(ground);
            mb.quad_tex(ground, [[-60.0, 0.0, -200.0], [x + 60.0, 0.0, -200.0], [x + 60.0, 0.0, 40.0], [-60.0, 0.0, 40.0]], [[-60.0, -200.0], [x + 60.0, -200.0], [x + 60.0, 40.0], [-60.0, 40.0]], &gt, [1.0; 3], [0.0, 1.0, 0.0]);
            let tris = gltf::write(&root.join("city"), "trees", &mb, &lib).expect("write model");
            write_scene(&root, "trees", &scene::Look::default(), "city/trees.gltf", Json2 { pos: [x * 0.5, 9.0, -60.0], yaw: 0.0, pitch: -6.0 });
            println!("trees: {} models, {tris} triangles, {x:.0} m long", names.len());
        }
        Some("dump") => {
            // citygen dump ROOT city/gen_in_1_1_1_0.glb [SEED]: write a file of the city to disk (to look at it, or to measure what the renderer makes of it)
            use lunaris_sim::procgen::Provider;
            let name = args.get(3).cloned().expect("a file name");
            let city = lunaris_citygen::runtime::City::open(&root, args.get(4).and_then(|v| v.parse().ok()).unwrap_or(1)).expect("open the city");
            let bytes = city.file(&name).expect("a city file").expect("made");
            let out = root.join(format!("dump_{}", name.rsplit('/').next().unwrap_or("x")));
            std::fs::write(&out, &bytes).expect("write");
            println!("{} bytes -> {}", bytes.len(), out.display());
        }
        Some("propsize") => {
            // citygen propsize ROOT SLUG...: the size in metres of models of the outdoor props, after the loader's scaling
            use lunaris_citygen::props::Props;
            let mut lib = Lib::from_json(&std::fs::read_to_string(root.join("city").join("textures").join("materials.json")).unwrap_or_default());
            let street = Props::load_dir_with(&mut lib, &root.join("city").join("props"), &lunaris_citygen::props::outdoor_budget, &lunaris_citygen::props::outdoor_cards);
            for slug in &args[3..] {
                match street.get(slug) {
                    Some(p) => println!("{slug}: {:.2} x {:.2} x {:.2} m, {} triangles", p.size()[0], p.size()[1], p.size()[2], p.triangles()),
                    None => println!("{slug}: not loaded"),
                }
            }
        }
        Some("parks") => {
            // citygen parks ROOT [SEED [I J [RADIUS_BLOCKS]]]: the parks round block (I, J), with their kind and the middle of the block
            use lunaris_citygen::plan::{LandUse, Plan};
            let plan = Plan::new(args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1));
            let (ci, cj): (i32, i32) = (args.get(4).and_then(|v| v.parse().ok()).unwrap_or(14), args.get(5).and_then(|v| v.parse().ok()).unwrap_or(-29));
            let r: i32 = args.get(6).and_then(|v| v.parse().ok()).unwrap_or(12);
            for j in cj - r..=cj + r {
                for i in ci - r..=ci + r {
                    if plan.landuse(i, j) == LandUse::Park {
                        let l = lunaris_citygen::streetscape::park_layout(&plan, i, j);
                        println!("block ({i}, {j}) {:?} centre ({:.0}, {:.0})", l.kind, l.cx, l.cz);
                    }
                }
            }
        }
        Some("map") => {
            // citygen map ROOT [SEED [RADIUS_BLOCKS]]: the land use round the origin as letters (D downtown, M midrise, R residential, o outskirts, I industrial, P park), one for every block
            use lunaris_citygen::plan::{LandUse, Plan};
            let plan = Plan::new(args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1));
            let r: i32 = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(30);
            let mut best: Option<(f32, i32, i32)> = None;
            for j in -r..=r {
                let line: String = (-r..=r).map(|i| {
                    let u = plan.landuse(i, j);
                    if u == LandUse::Downtown {
                        let d = ((i * i + j * j) as f32).sqrt();
                        if best.is_none_or(|b| d < b.0) {
                            best = Some((d, i, j));
                        }
                    }
                    match u { LandUse::Downtown => 'D', LandUse::Midrise => 'M', LandUse::Residential => 'R', LandUse::Outskirts => 'o', LandUse::Industrial => 'I', LandUse::Park => 'P', LandUse::Rim => 'W' }
                }).collect();
                println!("{line}");
            }
            if let Some((_, i, j)) = best {
                let (x0, x1, z0, z1) = plan.block_rect(i, j);
                println!("nearest downtown block ({i}, {j}), middle at ({:.0}, {:.0})", (x0 + x1) * 0.5, (z0 + z1) * 0.5);
            }
        }
        Some("doors") => {
            // citygen doors ROOT [SEED [COUNT [SPREAD]]]: one front door of COUNT buildings spread over blocks within SPREAD blocks of the origin, as `kind block_i block_j x y z out_x out_z width`, for tools/citygen/enter.py
            use lunaris_citygen::{building, plan::Plan};
            let lib = library(&root);
            let plan = Plan::new(args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1));
            let count: usize = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(40);
            let spread: i32 = args.get(5).and_then(|v| v.parse().ok()).unwrap_or(60);
            let mut rng = Rng::new(99);
            let mut seen: std::collections::HashMap<String, usize> = Default::default();
            let mut tries = 0;
            while seen.values().sum::<usize>() < count && tries < count * 60 {
                tries += 1;
                let (i, j) = (rng.int(-spread, spread), rng.int(-spread, spread));
                let lots = plan.lots(i, j);
                if lots.is_empty() {
                    continue;
                }
                let lp = lots[rng.int(0, lots.len() as i32 - 1) as usize];
                let kind = format!("{:?}", lp.kind);
                if *seen.get(&kind).unwrap_or(&0) >= count.div_ceil(6) {
                    continue;
                }
                let spec = building::spec_of(&lib, &lp.lot, lp.kind, lp.key);
                let doors = building::exterior(&mut MeshBuilder::new(), &lib, &spec);
                if let Some(dr) = doors.first() {
                    let mid = [dr.hinge[0] + dr.along[0] * dr.width * 0.5, dr.hinge[1], dr.hinge[2] + dr.along[2] * dr.width * 0.5];
                    println!("{kind} {i} {j} {:.2} {:.2} {:.2} {:.3} {:.3} {:.2}", mid[0], mid[1], mid[2], dr.out[0], dr.out[2], dr.width);
                    *seen.entry(kind).or_insert(0) += 1;
                }
            }
        }
        Some("polar-export") => {
            // citygen polar-export ROOT [SEED]: the round city's plan (rings, blocks, lots, the buildings the engine decided on them, their doors) as JSON on the standard output, for the game's Blender builder
            use lunaris_citygen::{export, polar::Polar};
            let lib = library(&root);
            let plan = plan_of(&root, args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1));
            let plan = if plan.polar.is_some() { plan } else { plan.with_polar(Some(Polar::default())) };
            println!("{}", export::polar_plan(&lib, &plan).to_string_compact().expect("json"));
        }
        Some("grids") => {
            // citygen grids [ROOT [SEED]]: the `ProcGrid` numbers of the city (a player's grids and a server's), as JSON, for a game's scene builder (game/city/grids.json)
            use lunaris_citygen::{plan::Plan, runtime};
            use lunaris_json::Json;
            let plan = Plan::new(args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1));
            let one = |g: &runtime::GridDef| Json::obj([("grid", Json::str(g.name)), ("cell", Json::Float(g.cell as f64)), ("radius", Json::Float(g.radius as f64)), ("hysteresis", Json::Float(g.hysteresis as f64)), ("per_tick", Json::from(g.per_tick as i64))]);
            let doc = Json::obj([("pitch", Json::Float(plan.pitch() as f64)), ("client", Json::Arr(runtime::grids(&plan).iter().map(one).collect())), ("server", Json::Arr(runtime::server_grids(&plan).iter().map(one).collect()))]);
            println!("{}", doc.to_string_pretty().expect("json"));
        }
        Some("shellprobe") => {
            // citygen shellprobe ROOT [SEED [I J]]: a street door of the first building of block (I, J) (default the first block round the origin that has one) with room behind it, as JSON for the live test of a server's collision
            // (tools/city_smoke.py): the middle of the doorway at floor level, the way out and along the wall (unit, horizontal), the width, a point 3 m inside, and how many metres of wall there are beside the door on each side
            use lunaris_citygen::building;
            let lib = library(&root);
            let plan = plan_of(&root, args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1));
            let want: Option<(i32, i32)> = args.get(4).and_then(|v| v.parse().ok()).zip(args.get(5).and_then(|v| v.parse().ok()));
            let all: Vec<(i32, i32)> = match plan.polar {
                // (a round city: the blocks of the second band first, the nearest to the Core after it)
                Some(pol) => (1..=4).chain(std::iter::once(0)).flat_map(|b| (0..pol.counts[b as usize] as i32).map(move |s| (b, s))).collect(),
                None => (-4..=4).flat_map(|j| (-4..=4).map(move |i| (i, j))).collect(),
            };
            let blocks: Vec<(i32, i32)> = want.map_or(all, |b| vec![b]);
            for (i, j) in blocks {
                for lp in plan.lots(i, j) {
                    let spec = building::spec_of(&lib, &lp.lot, lp.kind, lp.key);
                    let (doors, _) = building::exterior_with(&mut MeshBuilder::new(), &lib, &spec, false);
                    let foot = spec.vols[0].foot;
                    for d in &doors {
                        let mid = [d.hinge[0] + d.along[0] * d.width * 0.5, d.hinge[1], d.hinge[2] + d.along[2] * d.width * 0.5];
                        let inside = [mid[0] - d.out[0] * 3.0, mid[1], mid[2] - d.out[2] * 3.0];
                        let rel = [inside[0] - foot.o[0], inside[2] - foot.o[2]];
                        let (x, z) = (rel[0] * foot.ex[0] + rel[1] * foot.ex[2], rel[0] * foot.ez[0] + rel[1] * foot.ez[2]);
                        let wall = 1.5 + d.width * 0.5;
                        let clear = doors.iter().filter(|o| !std::ptr::eq(*o, d)).all(|o| ((o.hinge[0] - mid[0]).powi(2) + (o.hinge[2] - mid[2]).powi(2)).sqrt() > wall + 2.0);
                        if d.width >= 0.9 && clear && x > foot.x0 + 1.0 && x < foot.x1 - 1.0 && z > foot.z0 + 1.0 && z < foot.z1 - 1.0 && d.hinge[1] <= spec.vols[0].y0 + 0.6 {
                            println!("{{\"block\":[{i},{j}],\"kind\":\"{:?}\",\"door\":[{:.3},{:.3},{:.3}],\"out\":[{:.4},{:.4}],\"along\":[{:.4},{:.4}],\"width\":{:.3},\"inside\":[{:.3},{:.3},{:.3}],\"floor_y\":{:.3}}}", lp.kind, mid[0], mid[1], mid[2], d.out[0], d.out[2], d.along[0], d.along[2], d.width, inside[0], inside[1], inside[2], spec.vols[0].y0);
                            return;
                        }
                    }
                }
            }
            eprintln!("no building with a door and room behind it");
            std::process::exit(1);
        }
        Some("stairs") => {
            // citygen stairs ROOT [SEED [COUNT [SPREAD [FLOOR]]]]: the way up from floor FLOOR (default the ground floor) of COUNT buildings, as `kind block_i block_j` and four points (x y z) of the walk (see interior::stairs_path), for tools/citygen/climb.py
            use lunaris_citygen::{building, plan::Plan};
            let lib = library(&root);
            let plan = Plan::new(args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1));
            let count: usize = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(30);
            let spread: i32 = args.get(5).and_then(|v| v.parse().ok()).unwrap_or(60);
            let from_floor: u32 = args.get(6).and_then(|v| v.parse().ok()).unwrap_or(0);
            let mut rng = Rng::new(77);
            let mut seen: std::collections::HashMap<String, usize> = Default::default();
            let mut tries = 0;
            while seen.values().sum::<usize>() < count && tries < count * 80 {
                tries += 1;
                let (i, j) = (rng.int(-spread, spread), rng.int(-spread, spread));
                let lots = plan.lots(i, j);
                if lots.is_empty() {
                    continue;
                }
                let lp = lots[rng.int(0, lots.len() as i32 - 1) as usize];
                let kind = format!("{:?}", lp.kind);
                if *seen.get(&kind).unwrap_or(&0) >= count.div_ceil(5) {
                    continue;
                }
                let spec = building::spec_of(&lib, &lp.lot, lp.kind, lp.key);
                if let Some(p) = building::stairs_path(&spec, from_floor) {
                    let pts: Vec<String> = p.iter().map(|q| format!("{:.2} {:.2} {:.2}", q[0], q[1], q[2])).collect();
                    println!("{kind} {i} {j} {}", pts.join(" "));
                    *seen.entry(kind).or_insert(0) += 1;
                }
            }
        }
        Some("rooms") => {
            // citygen rooms ROOT [SEED [COUNT [SPREAD [FLOOR]]]]: the rooms of floor FLOOR of COUNT buildings, as `building_kind block_i block_j room_kind x y z width depth` (the middle of the room), for tools/citygen/rooms.py
            use lunaris_citygen::{building, plan::Plan};
            let lib = library(&root);
            let plan = Plan::new(args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1));
            let count: usize = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(12);
            let spread: i32 = args.get(5).and_then(|v| v.parse().ok()).unwrap_or(40);
            let floor: u32 = args.get(6).and_then(|v| v.parse().ok()).unwrap_or(0);
            let mut rng = Rng::new(55);
            let mut tries = 0;
            let mut done = 0;
            while done < count && tries < count * 100 {
                tries += 1;
                let (i, j) = (rng.int(-spread, spread), rng.int(-spread, spread));
                let lots = plan.lots(i, j);
                if lots.is_empty() {
                    continue;
                }
                let lp = lots[rng.int(0, lots.len() as i32 - 1) as usize];
                let spec = building::spec_of(&lib, &lp.lot, lp.kind, lp.key);
                if spec.floor_count() <= floor {
                    continue;
                }
                for (name, c, w, d) in building::rooms(&spec, floor) {
                    println!("{:?} {i} {j} {name} {:.2} {:.2} {:.2} {:.2} {:.2}", lp.kind, c[0], c[1], c[2], w, d);
                }
                done += 1;
            }
        }
        Some("cityinfo") => city_info(&root, args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1)),
        _ => {
            eprintln!("usage: citygen swatch ROOT | citygen district ROOT [NX NZ SEED] | citygen city ROOT [SEED [EXTENT [HOUR [DAY_SECONDS]]]] | citygen cityinfo ROOT [SEED]");
            std::process::exit(2);
        }
    }
}
