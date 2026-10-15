//! How the GPU renderer scales with the number of meshes: `cargo run --release -p lunaris-gpu --example many -- [count [width height [quality]]]`
//! (default 20000 meshes, 1280x720, high). Prints the cost of gathering the scene (the CPU part both renderers share) and of drawing it.
use lunaris_gpu::{GpuRenderer, Quality};
use lunaris_render::render::gather_scene;
use lunaris_render::{RenderOptions, TextureCache};
use lunaris_sim::Sim;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let count: usize = args.first().and_then(|v| v.parse().ok()).unwrap_or(20_000);
    let (w, h) = (args.get(1).and_then(|v| v.parse().ok()).unwrap_or(1280u32), args.get(2).and_then(|v| v.parse().ok()).unwrap_or(720u32));
    let quality = Quality::from_name(args.get(3).map(String::as_str).unwrap_or("high")).expect("quality: classic, low, medium, high, ultra");
    let mut sim = Sim::new(1);
    let side = (count as f64).sqrt().ceil() as usize;
    let mut entities = vec![
        r#"{"uid":1,"components":{"Camera":{},"Transform":{"pos":[0,60,110],"rot":[-0.3,0,0,0.95]}}}"#.to_string(),
        r#"{"uid":2,"components":{"Light":{"kind":0,"intensity":1.5},"Transform":{"rot":[-0.5,0.2,0,0.84]}}}"#.to_string(),
        r#"{"uid":3,"components":{"Environment":{}}}"#.to_string(),
    ];
    for i in 0..count {
        let (gx, gz) = ((i % side) as f32 - side as f32 / 2.0, (i / side) as f32 - side as f32 / 2.0);
        entities.push(format!(
            r#"{{"uid":{},"components":{{"Transform":{{"pos":[{},{},{}],"scale":[0.8,0.8,0.8]}},"MeshRenderer":{{"mesh":"{}","color":[{},0.6,{},1]}}}}}}"#,
            10 + i,
            gx * 1.2,
            ((i * 7) % 5) as f32 * 0.2,
            gz * 1.2,
            if i % 2 == 0 { "sphere" } else { "cube" },
            0.3 + (i % 7) as f32 * 0.1,
            0.3 + (i % 5) as f32 * 0.12
        ));
    }
    let t = std::time::Instant::now();
    sim.load_scene(&lunaris_json::parse(&format!(r#"{{"lunaris":1,"entities":[{}]}}"#, entities.join(","))).expect("json")).expect("scene");
    println!("{count} meshes loaded in {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    let mut tex = TextureCache::new(None);
    let opts = RenderOptions { width: w, height: h, threads: 1, ..RenderOptions::default() };
    let t = std::time::Instant::now();
    let n = 3;
    for _ in 0..n {
        let scene = gather_scene(&mut sim.world, &mut tex, &opts).expect("gather");
        std::hint::black_box(scene.meshes.len());
    }
    println!("gather_scene: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    let mut gpu = GpuRenderer::headless().expect("no GPU");
    let t = std::time::Instant::now();
    gpu.render_to_image(&mut sim.world, &mut tex, &opts, &quality).expect("render");
    println!("first frame (uploads, pipelines): {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    let t = std::time::Instant::now();
    let n = 10;
    for _ in 0..n {
        gpu.render_to_image(&mut sim.world, &mut tex, &opts, &quality).expect("render");
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    println!("{w}x{h} {} on {}: {ms:.2} ms per frame with read-back = {:.0} meshes per millisecond", args.get(3).map(String::as_str).unwrap_or("high"), gpu.adapter_name(), count as f64 / ms);
}
