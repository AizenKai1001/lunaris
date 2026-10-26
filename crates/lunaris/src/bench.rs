//! `lunaris bench`: measures the core on this machine. Run it with `cargo run --release -p lunaris -- bench`. `bench --gpu` measures the graphics card instead (`gpu_rows`).
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

/// A building made of triangles: four walls and a roof of `side` by `height` metres, every face cut into `n` by `n` squares of two triangles (a shell like the ones a city is made of).
fn block_mesh(side: f32, height: f32, n: usize) -> lunaris_sim::meshcol::CollisionMesh {
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut face = |o: [f32; 3], u: [f32; 3], v: [f32; 3]| {
        let base = verts.len() as u32;
        for j in 0..=n {
            for i in 0..=n {
                let (a, b) = (i as f32 / n as f32, j as f32 / n as f32);
                verts.push([o[0] + u[0] * a + v[0] * b, o[1] + u[1] * a + v[1] * b, o[2] + u[2] * a + v[2] * b]);
            }
        }
        let w = n as u32 + 1;
        for j in 0..n as u32 {
            for i in 0..n as u32 {
                let p = base + j * w + i;
                tris.push([p, p + 1, p + w]);
                tris.push([p + 1, p + w + 1, p + w]);
            }
        }
    };
    let (s, h) = (side, height);
    face([0.0, 0.0, 0.0], [s, 0.0, 0.0], [0.0, h, 0.0]);
    face([0.0, 0.0, s], [s, 0.0, 0.0], [0.0, h, 0.0]);
    face([0.0, 0.0, 0.0], [0.0, 0.0, s], [0.0, h, 0.0]);
    face([s, 0.0, 0.0], [0.0, 0.0, s], [0.0, h, 0.0]);
    face([0.0, h, 0.0], [s, 0.0, 0.0], [0.0, 0.0, s]);
    lunaris_sim::meshcol::CollisionMesh::new(verts, tris).expect("a closed shell of triangles")
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Memory this process holds in RAM right now, in KB (Linux: `VmRSS`; Windows: the working set); `None` where the platform does not say.
fn resident_kb() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
        line.split_whitespace().nth(1)?.parse().ok()
    }
    #[cfg(windows)]
    {
        #[repr(C)]
        struct Counters {
            cb: u32,
            page_faults: u32,
            peak_working_set: usize,
            working_set: usize,
            rest: [usize; 6],
        }
        extern "system" {
            fn GetCurrentProcess() -> isize;
            fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
        }
        let mut c = Counters { cb: std::mem::size_of::<Counters>() as u32, page_faults: 0, peak_working_set: 0, working_set: 0, rest: [0; 6] };
        // SAFETY: `c` is a valid PROCESS_MEMORY_COUNTERS-sized buffer and `cb` says its size.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
        (ok != 0).then_some(c.working_set as u64 / 1024)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}

/// The rows, and the memory an entity cost in bytes where the platform says (the performance gate checks it: unlike a time it does not depend on the machine).
/// The characters row: 200 capsules walking in a city of 36 mesh shells and about 3 300 boxes (the characters stage of the city server: the obstacles of the whole world are
/// gathered every tick). `LUNARIS_BENCH_ONLY=characters lunaris bench --ticks 1000` runs only this, five samples of a fifth of the ticks each (a profiler's food).
fn characters_row(per_sample: u64) -> lunaris_ecs::Result<Row> {
    let mut city = Sim::new(1);
    let spawn = |sim: &mut Sim, comps: Vec<(&str, String)>| -> lunaris_ecs::Result<()> {
        sim.apply(&lunaris_sim::Command::Spawn { uid: None, components: comps.into_iter().map(|(n, j)| (n.to_string(), lunaris_json::parse(&j).expect("json"))).collect() })?;
        Ok(())
    };
    spawn(&mut city, vec![("Transform", r#"{"pos":[100,-0.5,100],"scale":[400,1,400]}"#.to_string()), ("Collider", r#"{"shape":1}"#.to_string())])?;
    std::sync::Arc::make_mut(&mut city.meshes).by_name.insert("block".to_string(), std::sync::Arc::new(block_mesh(12.0, 10.0, 20)));
    for gz in 0..6 {
        for gx in 0..6 {
            spawn(&mut city, vec![("Transform", format!(r#"{{"pos":[{},0,{}]}}"#, gx as f32 * 20.0, gz as f32 * 20.0)), ("MeshCollider", r#"{"mesh":"block"}"#.to_string())])?;
        }
    }
    // street furniture, parked cars, walls: boxes on a grid between the shells
    for i in 0..4000usize {
        let (gx, gz) = ((i % 80) as f32 * 2.5 - 40.0, (i / 80) as f32 * 3.0 - 20.0);
        if (gx + 10.0).rem_euclid(20.0) < 8.0 && (gz + 10.0).rem_euclid(20.0) < 8.0 {
            continue;
        }
        spawn(&mut city, vec![("Transform", format!(r#"{{"pos":[{gx},0.5,{gz}]}}"#)), ("Collider", r#"{"shape":1,"size":[1,1,1]}"#.to_string())])?;
    }
    let people = 200usize;
    for i in 0..people {
        let a = i as f32 * 0.7;
        spawn(&mut city, vec![("Transform", format!(r#"{{"pos":[{},0,{}]}}"#, (i % 20) as f32 * 5.0 - 5.0, (i / 20) as f32 * 11.0 - 6.0)), ("CharacterBody", format!(r#"{{"walk":[{},{}]}}"#, a.cos() * 3.0, a.sin() * 3.0))])?;
    }
    city.step_n(3);
    let mut times = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        city.step_n(per_sample);
        times.push(ms(t) / per_sample as f64);
    }
    times.sort_by(|a, b| a.total_cmp(b));
    let mut detail = format!("per tick, {people} walking capsules among 36 mesh shells and {} boxes ({} entities in all; one thread)", city.world.len() - people - 37, city.world.len());
    if per_sample > 10 {
        // a long run (for a profiler): where the time of a tick goes, by the engine's own profile
        city.profile = Some(Default::default());
        city.step_n(per_sample);
        if let Some(p) = city.profile.take() {
            let mut stages: Vec<(&String, &lunaris_sim::sim::StageTime)> = p.stages.iter().filter(|(n, _)| n.starts_with("characters")).collect();
            stages.sort_by(|a, b| b.1.total_ms.total_cmp(&a.1.total_ms));
            for (name, s) in stages {
                detail.push_str(&format!("
    {name}: {:.3} ms a tick on average, {:.1} ms the slowest", s.total_ms / s.calls.max(1) as f64, s.max_ms));
            }
        }
    }
    Ok(Row { name: "characters", ms: times[2], detail })
}

/// The gather row: the static world as the rays of a car, a drone or a probe see it, gathered from a city of 36 mesh shells and about 3 300 boxes. The row is a gather of a 60 m box (what a car or a drone asks for, five
/// or six times a step in the city server); the detail says what the whole world costs (what a probe or the wallhack defence asks for). `LUNARIS_BENCH_ONLY=gather lunaris bench` runs only this.
fn gather_city() -> lunaris_ecs::Result<Sim> {
    let mut city = Sim::new(1);
    let spawn = |sim: &mut Sim, comps: Vec<(&str, String)>| -> lunaris_ecs::Result<()> {
        sim.apply(&lunaris_sim::Command::Spawn { uid: None, components: comps.into_iter().map(|(n, j)| (n.to_string(), lunaris_json::parse(&j).expect("json"))).collect() })?;
        Ok(())
    };
    spawn(&mut city, vec![("Transform", r#"{"pos":[100,-0.5,100],"scale":[400,1,400]}"#.to_string()), ("Collider", r#"{"shape":1}"#.to_string())])?;
    std::sync::Arc::make_mut(&mut city.meshes).by_name.insert("block".to_string(), std::sync::Arc::new(block_mesh(12.0, 10.0, 20)));
    for gz in 0..6 {
        for gx in 0..6 {
            spawn(&mut city, vec![("Transform", format!(r#"{{"pos":[{},0,{}]}}"#, gx as f32 * 20.0, gz as f32 * 20.0)), ("MeshCollider", r#"{"mesh":"block"}"#.to_string())])?;
        }
    }
    for i in 0..4000usize {
        let (gx, gz) = ((i % 80) as f32 * 2.5 - 40.0, (i / 80) as f32 * 3.0 - 20.0);
        if (gx + 10.0).rem_euclid(20.0) < 8.0 && (gz + 10.0).rem_euclid(20.0) < 8.0 {
            continue;
        }
        spawn(&mut city, vec![("Transform", format!(r#"{{"pos":[{gx},0.5,{gz}]}}"#)), ("Collider", r#"{"shape":1,"size":[1,1,1]}"#.to_string())])?;
    }
    city.step_n(2);
    Ok(city)
}

fn gather_row() -> lunaris_ecs::Result<Row> {
    let mut city = gather_city()?;
    let time = |city: &mut Sim, f: &dyn Fn(&mut Sim) -> usize| -> (f64, usize) {
        let mut n = 0;
        let mut v = Vec::new();
        for _ in 0..9 {
            let t = Instant::now();
            for _ in 0..50 {
                n = f(city);
            }
            v.push(ms(t) / 50.0);
        }
        v.sort_by(|a, b| a.total_cmp(b));
        (v[4], n)
    };
    let (near_ms, near_n) = time(&mut city, &|c| lunaris_sim::sight::Sight::gather_in(c, [10.0, -5.0, 10.0], [70.0, 20.0, 70.0]).shapes());
    let (all_ms, all_n) = time(&mut city, &|c| lunaris_sim::sight::Sight::gather(c).shapes());
    Ok(Row { name: "gather", ms: near_ms, detail: format!("one gather of the static world in a 60 m box ({near_n} shapes of {all_n}); the whole world takes {all_ms:.3} ms ({} entities)", city.world.len()) })
}

/// The probe rows: `probe::step` for 150 probes (a cold or roof check on each avatar of a city server) in the city of the gather row. `probe idle`: none of them casts (no ground cast, no ray), which a game that
/// lets its probes cast every few steps does on the steps in between; `probe cast`: every probe casts its ground and a ray (one gather of the world, then 300 casts). `LUNARIS_BENCH_ONLY=probe lunaris bench` runs only these.
fn probe_rows() -> lunaris_ecs::Result<Vec<Row>> {
    let mut city = gather_city()?;
    let mut ids = Vec::new();
    for i in 0..150usize {
        let (x, z) = ((i % 15) as f32 * 8.0 - 40.0, (i / 15) as f32 * 9.0 - 30.0);
        let e = city.world.spawn_with2(lunaris_sim::components::Transform { pos: [x, 1.5, z], ..Default::default() }, lunaris_sim::components::Probe { up: 0.0, depth: 0.0, ..Default::default() })?;
        ids.push(e);
    }
    let time = |city: &mut Sim| -> f64 {
        let mut v = Vec::new();
        for _ in 0..9 {
            let t = Instant::now();
            for _ in 0..30 {
                lunaris_sim::probe::step(&mut city.world, &city.heightfields, &city.meshes);
            }
            v.push(ms(t) / 30.0);
        }
        v.sort_by(|a, b| a.total_cmp(b));
        v[4]
    };
    let idle = time(&mut city);
    for &e in &ids {
        if let Some(p) = city.world.get_mut::<lunaris_sim::components::Probe>(e) {
            *p = lunaris_sim::components::Probe { up: 2.0, depth: 20.0, ray_max: 30.0, ray_dir: [0.0, 0.0, -1.0], ray_from: [0.0, 0.5, 0.0], ..Default::default() };
        }
    }
    let cast = time(&mut city);
    Ok(vec![
        Row { name: "probe idle", ms: idle, detail: "probe::step for 150 probes that do not cast (the world is not gathered), in the city of the gather row".to_string() },
        Row { name: "probe cast", ms: cast, detail: "probe::step for 150 probes that each cast their ground and a ray: one gather of the whole static world and 300 casts".to_string() },
    ])
}

/// Thirty of the shipped campfires in three rows, a camera in front of them (the scene of the `fx campfires` row and of the GPU rows).
fn campfire_sim() -> lunaris_ecs::Result<Sim> {
    let effect = lunaris_json::Json::str(include_str!("../../../examples/fx/campfire.fx.json")).to_string_compact().unwrap_or_default();
    let mut ents = vec![r#"{"uid":1,"components":{"Camera":{"fov_y":60},"Transform":{"pos":[0,4,26],"rot":[-0.08,0,0,0.9968]}}}"#.to_string()];
    for i in 0..30usize {
        ents.push(format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{},0,{}]}},"ParticleSystem":{{"effect":{effect}}}}}}}"#, 10 + i, (i % 10) as f32 * 3.0 - 13.5, (i / 10) as f32 * -4.0));
    }
    let text = format!(r#"{{"lunaris":1,"entities":[{}]}}"#, ents.join(","));
    let mut sim = Sim::new(1);
    sim.load_scene(&lunaris_json::parse(&text).map_err(|e| lunaris_ecs::EcsError::new("bad_scene", e.to_string()))?)?;
    Ok(sim)
}

/// A 40 by 40 grid of lit spheres and cubes seen from above and in front (the scene of the CPU render rows and of the GPU ones).
fn sphere_grid() -> lunaris_ecs::Result<Sim> {
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
    Ok(scene)
}

/// `lunaris bench --gpu`: how long the card takes to draw a picture, measured as the wall time of `render_to_image` (the picture and its read-back to the processor, median of several) and, where the card has
/// timestamp queries, the time its own passes took. Nothing to measure (and no row) on a machine without a graphics adapter. A `high` still is eight pictures (seven that settle the anti-aliasing and the
/// bounced light, and the one that is kept), so its row says more about a still than about a game's frame: the `medium` row is one picture.
fn gpu_rows() -> lunaris_ecs::Result<Vec<Row>> {
    let mut renderer = match lunaris_gpu::GpuRenderer::headless() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bench --gpu: no graphics adapter ({e}); nothing to measure");
            return Ok(Vec::new());
        }
    };
    renderer.set_profile(true);
    let adapter = renderer.adapter_name();
    let opts = lunaris_render::RenderOptions { width: 960, height: 540, threads: lunaris_sim::default_threads(), ..Default::default() };
    let mut rows = Vec::new();
    let cases: [(&'static str, &str, usize, fn() -> lunaris_ecs::Result<Sim>); 3] = [("gpu spheres medium", "medium", 21, sphere_grid), ("gpu spheres high", "high", 9, sphere_grid), ("gpu campfires high", "high", 9, campfire_sim)];
    for (name, quality, runs, make) in cases {
        let mut sim = make()?;
        let q = lunaris_gpu::Quality::from_name(quality).ok_or_else(|| lunaris_ecs::EcsError::new("bad_quality", quality.to_string()))?;
        let mut textures = lunaris_render::TextureCache::new(None);
        let (mut wall, mut card) = (Vec::new(), Vec::new());
        // where the processor's time goes: gathering the scene, the pictures that only settle a still, the last picture's own encoding, the wait for the card and the read-back
        let mut phases: Vec<(&str, &str, Vec<f64>)> = vec![("gather", "cpu_ms.gather", Vec::new()), ("warm-up pictures", "cpu_ms.warm_draws", Vec::new()), ("encode", "cpu_ms.draw_call", Vec::new()), ("wait and read-back", "cpu_ms.wait_and_read", Vec::new())];
        let mut triangles = 0.0;
        for i in 0..runs + 2 {
            // the campfires are stepped one tick between pictures, as a game would
            if name.contains("campfires") {
                sim.step();
            }
            let t = Instant::now();
            let frame = renderer.render_to_image(&mut sim.world, &mut textures, &opts, &q).map_err(|e| lunaris_ecs::EcsError::new("gpu_failed", e))?;
            let took = ms(t);
            // the first two pictures pay for compiling the pipelines and uploading the meshes
            if i >= 2 {
                wall.push(took);
                if let Some((_, v)) = frame.info.iter().find(|(k, _)| k == "gpu_ms.total") {
                    card.push(*v);
                }
                triangles = frame.info.iter().find(|(k, _)| k == "triangles").map_or(triangles, |(_, v)| *v);
                for (_, key, values) in phases.iter_mut() {
                    if let Some((_, v)) = frame.info.iter().find(|(k, _)| k == key) {
                        values.push(*v);
                    }
                }
            }
        }
        wall.sort_by(|a, b| a.total_cmp(b));
        card.sort_by(|a, b| a.total_cmp(b));
        let on_card = card.get(card.len() / 2).map_or(String::new(), |v| format!("; the card's own passes {v:.2} ms"));
        let tris = if triangles > 0.0 { format!(", {triangles:.0} triangles") } else { String::new() };
        let split: Vec<String> = phases
            .iter_mut()
            .filter(|(_, _, v)| !v.is_empty())
            .map(|(label, _, v)| {
                v.sort_by(|a, b| a.total_cmp(b));
                format!("{label} {:.2}", v[v.len() / 2])
            })
            .collect();
        let split = if split.is_empty() { String::new() } else { format!("; the processor (ms): {}", split.join(", ")) };
        rows.push(Row { name, ms: wall[wall.len() / 2], detail: format!("{adapter}, 960x540 {quality}: one picture drawn and read back, median of {runs}{on_card}{split}{tris}") });
    }
    Ok(rows)
}

/// The particle-system row: thirty of the shipped campfires (flames, embers, smoke: about 80 live particles each, all in view) stepped one tick a picture and drawn into the scene's meshes (`gather_scene`, the CPU side of a picture; the effects' share is the
/// rest after the same scene without them). `LUNARIS_BENCH_ONLY=fx lunaris bench` runs just this one.
fn fx_row() -> lunaris_ecs::Result<Row> {
    let mut sim = campfire_sim()?;
    let mut textures = lunaris_render::TextureCache::new(None);
    let opts = lunaris_render::RenderOptions { width: 960, height: 540, threads: lunaris_sim::default_threads(), ..Default::default() };
    // the first picture fills the fires up (it steps each from before time 0)
    lunaris_render::render::gather_scene(&mut sim.world, &mut textures, &opts)?;
    let mut v = Vec::new();
    for _ in 0..9 {
        let mut total = 0.0;
        for _ in 0..30 {
            sim.step();
            let t = Instant::now();
            lunaris_render::render::gather_scene(&mut sim.world, &mut textures, &opts)?;
            total += ms(t);
        }
        v.push(total / 30.0);
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let live = textures.fx.stats.particles;
    Ok(Row { name: "fx campfires", ms: v[4], detail: format!("one picture's gather (CPU side) of 30 campfires, {live} live particles, each stepped one tick and drawn as meshes, {} threads", lunaris_sim::default_threads()) })
}

/// The CPU reference renderer's rows: one 960 x 540 picture of the sphere grid on one thread and on all of them. The first picture of a scene pays for building the meshes, so each row is the median of
/// five pictures after one that warms up (`LUNARIS_BENCH_ONLY=render lunaris bench` runs just these; with `LUNARIS_TIMING=1 LUNARIS_TIMING_MIN=0` the stages of a picture are printed).
fn render_rows(default_threads: usize) -> lunaris_ecs::Result<Vec<Row>> {
    let mut scene = sphere_grid()?;
    let grid = 40usize;
    let mut textures = lunaris_render::TextureCache::new(None);
    let opts = lunaris_render::RenderOptions { width: 960, height: 540, threads: 1, ..Default::default() };
    let median = |scene: &mut Sim, textures: &mut lunaris_render::TextureCache, o: &lunaris_render::RenderOptions| -> lunaris_ecs::Result<(f64, lunaris_render::Frame)> {
        let mut last = lunaris_render::render(&mut scene.world, textures, o)?;
        let mut v = Vec::new();
        for _ in 0..5 {
            let t = Instant::now();
            last = lunaris_render::render(&mut scene.world, textures, o)?;
            v.push(ms(t));
        }
        v.sort_by(|a, b| a.total_cmp(b));
        Ok((v[2], last))
    };
    let (one, frame) = median(&mut scene, &mut textures, &opts)?;
    let mut rows = vec![Row { name: "render 960x540", ms: one, detail: format!("{} lit meshes, {:.0}% of pixels covered, hash {:016x} (CPU reference renderer, one thread, median of five after one that warms up)", grid * grid, frame.coverage() * 100.0, frame.hash()) }];
    let many = lunaris_render::RenderOptions { threads: default_threads, ..opts.clone() };
    let (n, frame_n) = median(&mut scene, &mut textures, &many)?;
    rows.push(Row { name: "render (threads)", ms: n, detail: format!("{default_threads} threads: {:.1}x faster, picture identical: {}", one / n.max(1e-9), frame_n.hash() == frame.hash()) });
    Ok(rows)
}

fn measure(entities: usize, ticks: u64, gpu: bool) -> Result<(Vec<Row>, Option<f64>)> {
    if gpu {
        return Ok((gpu_rows()?, None));
    }
    if std::env::var("LUNARIS_BENCH_ONLY").as_deref() == Ok("render") {
        return Ok((render_rows(lunaris_sim::default_threads())?, None));
    }
    if std::env::var("LUNARIS_BENCH_ONLY").as_deref() == Ok("fx") {
        return Ok((vec![fx_row()?], None));
    }
    if std::env::var("LUNARIS_BENCH_ONLY").as_deref() == Ok("probe") {
        return Ok((probe_rows()?, None));
    }
    if std::env::var("LUNARIS_BENCH_ONLY").as_deref() == Ok("characters") {
        return Ok((vec![characters_row((ticks / 5).max(1))?], None));
    }
    if std::env::var("LUNARIS_BENCH_ONLY").as_deref() == Ok("gather") {
        return Ok((vec![gather_row()?], None));
    }
    let mut rows = Vec::new();
    let mut sim = Sim::new(1);

    // 1. spawn straight into the final archetype
    let rss_before = resident_kb();
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
    // what the entities cost in RAM: the growth of the resident set (the columns, the uid table and the slot table), against the bytes of the components themselves
    let rss_after = resident_kb();
    let bytes_per_entity = match (rss_before, rss_after) {
        (Some(a), Some(b)) if b > a => Some((b - a) as f64 * 1024.0 / entities as f64),
        _ => None,
    };
    let held = match (rss_before, rss_after) {
        (Some(a), Some(b)) if b > a => format!("; {:.1} MB resident = {:.0} bytes/entity (the components alone are {} bytes)", (b - a) as f64 / 1024.0, (b - a) as f64 * 1024.0 / entities as f64, std::mem::size_of::<Transform>() + std::mem::size_of::<Velocity>()),
        _ => String::new(),
    };
    rows.push(Row { name: "spawn", ms: spawn_ms, detail: format!("{:.2} M entities/s{held}", entities as f64 / spawn_ms / 1000.0) });

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
    let hash_ms = ms(t);
    // how much of it is putting the entities in uid order (the canonical order of the hash)
    let t = Instant::now();
    let ordered = sim.world.entities_by_uid().len();
    let order_ms = ms(t);
    rows.push(Row { name: "state hash", ms: hash_ms, detail: format!("{h:016x} (canonical order, field by field; {order_ms:.1} ms of it is putting the {ordered} entities in uid order)") });

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

    // 7d. navigation: one path search over open ground, and one across a city of 36 shells of 4 000 triangles each (the cost the city server pays for every search: the grid is
    // built from the triangles of the area)
    {
        let mut nav = Sim::new(1);
        nav.apply(&lunaris_sim::Command::Spawn { uid: None, components: vec![("Transform".into(), lunaris_json::parse(r#"{"pos":[50,-0.5,50],"scale":[240,1,240]}"#).expect("json")), ("Collider".into(), lunaris_json::parse(r#"{"shape":1}"#).expect("json"))] })?;
        let median = |sim: &mut Sim, from: [f32; 3], to: [f32; 3], keep: bool| -> (f64, usize) {
            let mut times = Vec::new();
            let mut points = 0;
            for _ in 0..5 {
                let t = Instant::now();
                let r = lunaris_sim::nav::find_path_with(&mut sim.world, from, to, lunaris_sim::nav::NavParams { keep, ..Default::default() }, &sim.heightfields, &sim.meshes);
                times.push(ms(t));
                points = r.map(|p| p.points.len()).unwrap_or(0);
            }
            times.sort_by(|a, b| a.total_cmp(b));
            (times[2], points)
        };
        let (open_ms, open_points) = median(&mut nav, [-8.0, 0.0, -8.0], [108.0, 0.0, 108.0], false);
        rows.push(Row { name: "nav open", ms: open_ms, detail: format!("one path search over open ground, 164 m ({open_points} waypoints)") });
        std::sync::Arc::make_mut(&mut nav.meshes).by_name.insert("block".to_string(), std::sync::Arc::new(block_mesh(12.0, 10.0, 20)));
        let tris = nav.meshes.by_name["block"].tris.len();
        for gz in 0..6 {
            for gx in 0..6 {
                nav.apply(&lunaris_sim::Command::Spawn { uid: None, components: vec![("Transform".into(), lunaris_json::parse(&format!(r#"{{"pos":[{},0,{}]}}"#, gx as f32 * 20.0, gz as f32 * 20.0)).expect("json")), ("MeshCollider".into(), lunaris_json::parse(r#"{"mesh":"block"}"#).expect("json"))] })?;
            }
        }
        let (city_ms, city_points) = median(&mut nav, [-8.0, 0.0, -8.0], [108.0, 0.0, 108.0], false);
        rows.push(Row { name: "nav city", ms: city_ms, detail: format!("one path search across 36 shells of {tris} triangles ({} in all), 164 m ({city_points} waypoints)", 36 * tris) });
        // the same search with the cells kept between searches (`NavLimits.keep_cells`): the first of the five pays, the median is the search that finds its cells made
        let (kept_ms, _) = median(&mut nav, [-8.0, 0.0, -8.0], [108.0, 0.0, 108.0], true);
        rows.push(Row { name: "nav city kept", ms: kept_ms, detail: format!("the same search with the cells of the grid kept between searches (`NavParams.keep`), repeated over the same streets: the cost of a search that finds its cells made ({:.0} times faster)", city_ms / kept_ms.max(1e-9)) });
    }

    rows.push(characters_row(10)?);
    rows.push(gather_row()?);
    rows.push(fx_row()?);
    rows.extend(probe_rows()?);

    // 8. reference renderer: a grid of lit spheres and cubes
    rows.extend(render_rows(default_threads)?);
    Ok((rows, bytes_per_entity))
}

pub fn run(entities: usize, ticks: u64, json: bool, gpu: bool) -> ExitCode {
    match measure(entities, ticks, gpu) {
        Ok((rows, bytes_per_entity)) => {
            if json {
                let arr = rows.iter().map(|r| Json::obj([("name", Json::str(r.name)), ("ms", Json::from((r.ms * 1000.0).round() / 1000.0)), ("detail", Json::str(r.detail.clone()))]));
                println!("{}", Json::obj([("entities", Json::from(entities)), ("ticks", Json::from(ticks as i64)), ("bytes_per_entity", bytes_per_entity.map(|b| Json::from((b * 10.0).round() / 10.0)).unwrap_or(Json::Null)), ("results", Json::arr(arr))]).to_string_pretty().unwrap());
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
