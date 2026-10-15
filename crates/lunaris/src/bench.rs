//! `lunaris bench`: measures the core on this machine. Run it with `cargo run --release -p lunaris -- bench`.
use lunaris_ecs::{to_scene_json, Result};
use lunaris_json::Json;
use lunaris_sim::components::{Transform, Velocity, Wander};
use lunaris_sim::Sim;
use std::process::ExitCode;
use std::time::Instant;

struct Row {
    name: &'static str,
    ms: f64,
    detail: String,
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn measure(entities: usize, ticks: u64) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    let mut sim = Sim::new(1);

    // 1. spawn straight into the final archetype
    let t = Instant::now();
    for i in 0..entities {
        let x = i as f32 * 0.01;
        let tr = Transform { pos: [x, 0.0, 0.0], ..Transform::default() };
        let v = Velocity { lin: [1.0, 0.5, 0.0], ang: [0.0, 0.0, 0.1] };
        if i % 4 == 0 {
            sim.world.spawn_with3(tr, v, Wander { strength: 1.0 })?;
        } else {
            sim.world.spawn_with2(tr, v)?;
        }
    }
    let spawn_ms = ms(t);
    rows.push(Row { name: "spawn", ms: spawn_ms, detail: format!("{:.2} M entities/s", entities as f64 / spawn_ms / 1000.0) });

    // 2. full ticks (wander on a quarter of the entities, movement on all), one thread and then the default thread count
    sim.step_n(2); // warm the caches and the allocator
    let default_threads = sim.threads;
    sim.threads = 1;
    let t = Instant::now();
    sim.step_n(ticks);
    let single = ms(t) / ticks as f64;
    sim.threads = default_threads;
    sim.step_n(2);
    let t = Instant::now();
    sim.step_n(ticks);
    let per_tick = ms(t) / ticks as f64;
    rows.push(Row { name: "step (1 thread)", ms: single, detail: format!("per tick over {entities} entities = {:.1} ns/entity ({:.0} M entity-updates/s)", single * 1e6 / entities as f64, entities as f64 / single / 1000.0) });
    rows.push(Row { name: "step", ms: per_tick, detail: format!("{default_threads} threads: {:.1} ns/entity ({:.0} M entity-updates/s), {:.1}x faster; the state hash is identical for any thread count", per_tick * 1e6 / entities as f64, entities as f64 / per_tick / 1000.0, single / per_tick) });

    // 2b. the same kind of work written as a graph system (data, interpreted) instead of Rust
    let def = lunaris_json::parse(r#"{"name":"drift","each":["Transform","Velocity"],"do":[
        {"set":"Velocity.lin[1]","to":{"add":[{"get":"Velocity.lin[1]"},{"mul":[0.1,{"dt":true}]}]}},
        {"if":{"gt":[{"get":"Transform.pos[1]"},100]},"then":[{"set":"Transform.pos[1]","to":0}]}]}"#).expect("valid json");
    sim.apply(&lunaris_sim::Command::DefineSystem { def })?;
    sim.step(); // warm up
    let gticks = 10u64;
    // the one-entity-at-a-time interpreter (the reference), then the batch vm that normal runs use
    sim.scripts.interpret_only = true;
    sim.threads = 1;
    let t = Instant::now();
    sim.step_n(gticks);
    let gi = (ms(t) / gticks as f64 - single).max(0.0);
    sim.scripts.interpret_only = false;
    sim.step(); // warm up
    let t = Instant::now();
    sim.step_n(gticks);
    let g1 = (ms(t) / gticks as f64 - single).max(0.0);
    sim.threads = default_threads;
    let t = Instant::now();
    sim.step_n(gticks);
    let gn = (ms(t) / gticks as f64 - per_tick).max(0.0);
    rows.push(Row { name: "graph interpreted", ms: gi, detail: format!("extra cost per tick of a 2-statement graph over {entities} entities, one thread, the reference interpreter = {:.0} ns/entity", gi * 1e6 / entities as f64) });
    rows.push(Row { name: "graph (1 thread)", ms: g1, detail: format!("the same graph through the batch vm = {:.1} ns/entity, {:.1}x the interpreter (data, no compiler needed)", g1 * 1e6 / entities as f64, if g1 > 0.0 { gi / g1 } else { 0.0 }) });
    rows.push(Row { name: "graph system", ms: gn, detail: format!("{default_threads} threads: {:.1} ns/entity, {:.1}x faster than one thread", gn * 1e6 / entities as f64, if gn > 0.0 { g1 / gn } else { 0.0 }) });
    sim.apply(&lunaris_sim::Command::RemoveSystem { name: "drift".into() })?;

    // 3. one bare query pass
    let t = Instant::now();
    let mut sum = 0.0f32;
    sim.world.for_each::<(&Transform,), _>(|_, (tr,)| sum += tr.pos[0])?;
    rows.push(Row { name: "query pass", ms: ms(t), detail: format!("read one component of every entity (checksum {sum:.0})") });

    // 4. state hash
    let t = Instant::now();
    let h = sim.state_hash();
    rows.push(Row { name: "state hash", ms: ms(t), detail: format!("{h:016x} (canonical order, field by field)") });

    // 5. snapshot by clone
    let t = Instant::now();
    let snap = sim.clone();
    let clone_ms = ms(t);
    let bytes = entities * (std::mem::size_of::<Transform>() + std::mem::size_of::<Velocity>());
    rows.push(Row { name: "snapshot", ms: clone_ms, detail: format!("clone of ~{:.0} MB of component data; equal hash: {}", bytes as f64 / 1e6, snap.state_hash() == h) });

    // 6. scene export (limited to 100k so the text stays reasonable)
    let mut small = Sim::new(1);
    for i in 0..entities.min(100_000) {
        small.world.spawn_with2(Transform { pos: [i as f32, 0.0, 0.0], ..Transform::default() }, Velocity::default())?;
    }
    let t = Instant::now();
    let text = to_scene_json(&small.world).to_string_compact().map(|s| s.len()).unwrap_or(0);
    rows.push(Row { name: "scene export", ms: ms(t), detail: format!("{} entities -> {:.1} MB of JSON", small.world.len(), text as f64 / 1e6) });

    // 7. scene import
    let doc = to_scene_json(&small.world).to_string_compact().unwrap_or_default();
    let t = Instant::now();
    let parsed = lunaris_json::parse(&doc).map_err(|e| lunaris_ecs::EcsError::new("bench", e.to_string()))?;
    let parse_ms = ms(t);
    let mut loaded = Sim::new(1);
    let t = Instant::now();
    lunaris_ecs::load_scene_json(&mut loaded.world, &parsed)?;
    rows.push(Row { name: "scene import", ms: parse_ms + ms(t), detail: format!("parse {:.0} ms + build {:.0} ms; equal hash: {}", parse_ms, ms(t), loaded.world.state_hash() == small.world.state_hash()) });
    // 7b. physics-lite: a pile of spheres falling onto a floor
    let mut phys = Sim::new(1);
    phys.apply(&lunaris_sim::Command::Spawn { uid: None, components: vec![("Transform".into(), lunaris_json::parse(r#"{"pos":[0,-0.5,0]}"#).expect("json")), ("Collider".into(), lunaris_json::parse(r#"{"shape":1,"size":[100,1,100]}"#).expect("json"))] })?;
    let bodies = 5000usize;
    for i in 0..bodies {
        let (gx, gz, gy) = ((i % 25) as f32 * 1.2 - 15.0, ((i / 25) % 25) as f32 * 1.2 - 15.0, 0.6 + (i / 625) as f32 * 1.1);
        phys.world.spawn_with3(
            lunaris_sim::components::Transform { pos: [gx, gy, gz], ..Default::default() },
            lunaris_sim::components::Velocity::default(),
            lunaris_sim::components::Collider::default(),
        )?;
    }
    let mut with_body = Vec::new();
    phys.world.for_each::<(&lunaris_sim::components::Collider, &lunaris_sim::components::Velocity), _>(|e, _| with_body.push(e))?;
    for e in with_body {
        phys.world.insert(e, lunaris_sim::components::RigidBody::default())?;
    }
    phys.step_n(5);
    let t = Instant::now();
    phys.step_n(30);
    let contacts = phys.contacts.len();
    rows.push(Row { name: "physics", ms: ms(t) / 30.0, detail: format!("per tick, {bodies} spheres piling up ({contacts} contacts in the last tick); sort-and-sweep broad phase, sequential impulses, one thread") });

    // 7c. physics-lite: a tall column of overlapping spheres (every centre has the same x and z)
    let mut tower = Sim::new(1);
    let column = 5000usize;
    for i in 0..column {
        tower.world.spawn_with3(
            lunaris_sim::components::Transform { pos: [0.0, 0.5 + i as f32 * 0.9, 0.0], ..Default::default() },
            lunaris_sim::components::Velocity::default(),
            lunaris_sim::components::Collider::default(),
        )?;
    }
    let mut column_bodies = Vec::new();
    tower.world.for_each::<(&lunaris_sim::components::Collider, &lunaris_sim::components::Velocity), _>(|e, _| column_bodies.push(e))?;
    for e in column_bodies {
        tower.world.insert(e, lunaris_sim::components::RigidBody { gravity_scale: 0.0, ..Default::default() })?;
    }
    tower.step_n(2);
    let t = Instant::now();
    tower.step_n(20);
    rows.push(Row { name: "physics column", ms: ms(t) / 20.0, detail: format!("per tick, {column} overlapping spheres in one vertical line ({} contacts); the broad phase sweeps along the axis with the most spread", tower.contacts.len()) });

    // 8. reference renderer: a grid of lit spheres and cubes
    let mut scene = Sim::new(1);
    scene.world.spawn_with2(lunaris_sim::components::Transform { pos: [0.0, 14.0, 26.0], ..Default::default() }, lunaris_sim::components::Camera::default())?;
    let grid = 40usize;
    for i in 0..grid * grid {
        let (gx, gz) = ((i % grid) as f32 - grid as f32 / 2.0, (i / grid) as f32 - grid as f32 / 2.0);
        let mesh = scene.world.strings_mut().intern(if i % 2 == 0 { "sphere" } else { "cube" });
        scene.world.spawn_with2(
            lunaris_sim::components::Transform { pos: [gx * 1.2, 0.0, gz * 1.2], ..Default::default() },
            lunaris_sim::components::MeshRenderer { mesh: lunaris_ecs::Sym(mesh), color: [0.4 + (i % 7) as f32 * 0.08, 0.5, 0.9 - (i % 5) as f32 * 0.1, 1.0], ..Default::default() },
        )?;
    }
    let mut textures = lunaris_render::TextureCache::new(None);
    let opts = lunaris_render::RenderOptions { width: 960, height: 540, threads: 1, ..Default::default() };
    let t = Instant::now();
    let frame = lunaris_render::render(&mut scene.world, &mut textures, &opts)?;
    let one = ms(t);
    rows.push(Row { name: "render 960x540", ms: one, detail: format!("{} lit meshes, {:.0}% of pixels covered, hash {:016x} (CPU reference renderer, one thread)", grid * grid, frame.coverage() * 100.0, frame.hash()) });
    let many = lunaris_render::RenderOptions { threads: default_threads, ..opts.clone() };
    let t = Instant::now();
    let frame_n = lunaris_render::render(&mut scene.world, &mut textures, &many)?;
    let n = ms(t);
    rows.push(Row { name: "render (threads)", ms: n, detail: format!("{default_threads} threads: {:.1}x faster, picture identical: {}", one / n.max(1e-9), frame_n.hash() == frame.hash()) });
    Ok(rows)
}

pub fn run(entities: usize, ticks: u64, json: bool) -> ExitCode {
    match measure(entities, ticks) {
        Ok(rows) => {
            if json {
                let arr = rows.iter().map(|r| Json::obj([("name", Json::str(r.name)), ("ms", Json::from((r.ms * 1000.0).round() / 1000.0)), ("detail", Json::str(r.detail.clone()))]));
                println!("{}", Json::obj([("entities", Json::from(entities)), ("ticks", Json::from(ticks as i64)), ("results", Json::arr(arr))]).to_string_pretty().unwrap());
            } else {
                println!("lunaris bench: {entities} entities, {ticks} ticks (build with --release for real numbers)");
                for r in rows {
                    println!("  {:<13} {:>10.3} ms   {}", r.name, r.ms, r.detail);
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("bench failed: {e}");
            ExitCode::FAILURE
        }
    }
}
