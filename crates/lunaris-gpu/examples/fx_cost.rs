//! What the lighting effects cost: draws a scene file at a size in the high quality with none of its effects, with each one alone and with all of them, and prints the milliseconds a picture takes (read-back included, the same for all, so
//! the differences are what each effect costs): `cargo run --release -p lunaris-gpu --example fx_cost -- examples/lighting_showcase.lunaris.json [width height [pictures [quality]]]`.
//! The effects are the entities of the scene that carry `AutoExposure`, `Ssr`, `LensFlare` or `ReflectionProbe`; everything else stays. The variants take turns in rounds of ten pictures, so that a card that
//! warms up or a machine that is busy for a moment affects all of them alike; the figure is the median of a variant's pictures.
use lunaris_gpu::{GpuRenderer, Quality};
use lunaris_json::Json;
use lunaris_render::{RenderOptions, TextureCache};
use lunaris_sim::Sim;
use std::time::Instant;

const EFFECTS: [&str; 4] = ["AutoExposure", "Ssr", "LensFlare", "ReflectionProbe"];

/// The scene with the entities that carry an effect component not named in `keep` taken out.
fn variant(scene: &Json, keep: &[&str]) -> Json {
    let Some(pairs) = scene.as_object() else { panic!("not a scene file") };
    let mut out: Vec<(String, Json)> = Vec::new();
    for (key, value) in pairs {
        if key == "entities" {
            let kept: Vec<Json> = value
                .as_array()
                .expect("entities is a list")
                .iter()
                .filter(|e| {
                    let components = e.get("components");
                    EFFECTS.iter().all(|fx| keep.contains(fx) || components.and_then(|c| c.get(fx)).is_none())
                })
                .cloned()
                .collect();
            out.push((key.clone(), Json::Arr(kept)));
        } else {
            out.push((key.clone(), value.clone()));
        }
    }
    Json::Obj(out)
}

struct Run {
    name: String,
    sim: Sim,
    tex: TextureCache,
    times: Vec<f64>,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().cloned().unwrap_or_else(|| "examples/lighting_showcase.lunaris.json".to_string());
    let (w, h) = (args.get(1).and_then(|v| v.parse().ok()).unwrap_or(1920u32), args.get(2).and_then(|v| v.parse().ok()).unwrap_or(1080u32));
    let pictures: usize = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(100);
    let quality = Quality::from_name(args.get(4).map(String::as_str).unwrap_or("high")).expect("quality: classic, low, medium, high, ultra");
    let text = std::fs::read_to_string(&path).expect("cannot read the scene");
    let scene = lunaris_json::parse(&text).expect("scene is not JSON");
    let root = std::path::Path::new(&path).parent().and_then(|p| p.parent()).map(|p| p.to_path_buf()).unwrap_or_else(|| ".".into());
    let mut gpu = GpuRenderer::headless().expect("no GPU");
    let opts = RenderOptions { width: w, height: h, ..RenderOptions::default() };
    let mut runs: Vec<Run> = Vec::new();
    let mut add = |name: &str, keep: &[&str]| {
        let mut sim = Sim::new(1);
        sim.load_scene(&variant(&scene, keep)).expect("the scene loads");
        runs.push(Run { name: name.to_string(), sim, tex: TextureCache::new(Some(root.clone())), times: Vec::new() });
    };
    add("no effect", &[]);
    for fx in EFFECTS {
        add(fx, &[fx]);
    }
    add("all four", &EFFECTS);
    // the first pictures build the pipelines, upload the meshes and capture the probes
    for r in runs.iter_mut() {
        for _ in 0..5 {
            gpu.render_frame(&mut r.sim.world, &mut r.tex, &opts, &quality, false).expect("render");
        }
    }
    // capturing the probes again (what `bake_probe` does): six faces and their mip levels for each probe, once
    if let Some(r) = runs.iter_mut().find(|r| r.name == "ReflectionProbe") {
        let mut captures: Vec<f64> = Vec::new();
        for _ in 0..5 {
            let t = Instant::now();
            let done = gpu.bake_probes(&mut r.sim.world, &mut r.tex, None).expect("bake");
            let _ = gpu.gpu.device.poll(wgpu::PollType::wait_indefinitely());
            captures.push(t.elapsed().as_secs_f64() * 1000.0 / done.len().max(1) as f64);
        }
        captures.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("capturing a probe (six faces at the scene's size, mip levels): {:.2} ms each, median of 5", captures[2]);
    }
    for _ in 0..pictures.div_ceil(10) {
        for r in runs.iter_mut() {
            for _ in 0..10 {
                let t = Instant::now();
                gpu.render_frame(&mut r.sim.world, &mut r.tex, &opts, &quality, false).expect("render");
                r.times.push(t.elapsed().as_secs_f64() * 1000.0);
            }
        }
    }
    println!("{} | {w} x {h}, {} pictures each, median, read-back included", gpu.adapter_name(), runs[0].times.len());
    let mut base = 0.0;
    for (i, r) in runs.iter_mut().enumerate() {
        r.times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = r.times[r.times.len() / 2];
        if i == 0 {
            base = median;
            println!("{:<18} {median:7.2} ms", r.name);
        } else {
            println!("{:<18} {median:7.2} ms   ({:+.2} ms)", r.name, median - base);
        }
    }
}
