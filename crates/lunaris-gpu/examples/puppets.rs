//! What a crowd of skinned models costs the GPU renderer, with the models skinned on the card and on the processor:
//! `PUPPET_ROOT=<game folder> cargo run --release -p lunaris-gpu --example puppets -- [count [width height [quality]]]`
//! (default 60 armed mannequins, 1280x720, high). The mannequins are `assets/models/player_rifle.glb` of the game folder, walking at different moments of the clip; the camera looks at all of them.
//! Prints the time of a picture (gather, the card's work, the read-back) with `set_gpu_skinning(false)` and `(true)`; with `LUNARIS_GPU_PROFILE=1` also where a picture spent its time.
use lunaris_gpu::{GpuRenderer, Quality};
use lunaris_render::{RenderOptions, TextureCache};
use lunaris_sim::Sim;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let count: u64 = args.first().and_then(|v| v.parse().ok()).unwrap_or(60);
    let (w, h) = (args.get(1).and_then(|v| v.parse().ok()).unwrap_or(1280u32), args.get(2).and_then(|v| v.parse().ok()).unwrap_or(720u32));
    let quality = Quality::from_name(args.get(3).map(String::as_str).unwrap_or("high")).expect("quality: classic, low, medium, high, ultra");
    let root = std::path::PathBuf::from(std::env::var("PUPPET_ROOT").expect("PUPPET_ROOT: the game folder (with assets/models/player_rifle.glb)"));
    let mut sim = Sim::new(1);
    sim.asset_root = Some(root.clone());
    let mut entities = vec![
        r#"{"uid":1,"components":{"Camera":{"fov_y":50},"Transform":{"pos":[0,5,26],"rot":[-0.12,0,0,0.99]}}}"#.to_string(),
        r#"{"uid":2,"components":{"Light":{"kind":0,"intensity":1.5},"Transform":{"rot":[-0.5,0.2,0,0.84]}}}"#.to_string(),
        r#"{"uid":3,"components":{"Environment":{}}}"#.to_string(),
        r#"{"uid":4,"components":{"Transform":{"pos":[0,-0.05,0],"scale":[80,0.1,80]},"MeshRenderer":{"mesh":"cube","color":[0.35,0.4,0.3,1]}}}"#.to_string(),
    ];
    for k in 0..count {
        let (x, z) = (((k % 10) as f32 - 4.5) * 2.2, ((k / 10) as f32) * -2.2);
        let t = 0.05 * k as f32;
        entities.push(format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{x},0,{z}]}},"MeshRenderer":{{"mesh":"assets/models/player_rifle.glb","color":[1,1,1,1]}},"Animator":{{"clip":"rifle_walk","time":{t},"speed":0.0,"looping":true}}}}}}"#, 10 + k));
    }
    sim.load_scene(&lunaris_json::parse(&format!(r#"{{"lunaris":1,"entities":[{}]}}"#, entities.join(","))).expect("json")).expect("scene");
    let mut tex = TextureCache::new(Some(root));
    let opts = RenderOptions { width: w, height: h, threads: 1, ..RenderOptions::default() };
    let mut gpu = GpuRenderer::headless().expect("no GPU");
    println!("{count} armed mannequins, {w}x{h} {} on {}", args.get(3).map(String::as_str).unwrap_or("high"), gpu.adapter_name());
    let mut pictures = Vec::new();
    for on_card in [false, true, false, true] {
        gpu.set_gpu_skinning(on_card);
        // a first picture uploads the models and makes the pipelines
        gpu.render_to_image(&mut sim.world, &mut tex, &opts, &quality).expect("render");
        let n = 12;
        let t = std::time::Instant::now();
        let mut last = None;
        for _ in 0..n {
            last = Some(gpu.render_to_image(&mut sim.world, &mut tex, &opts, &quality).expect("render"));
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("skinned {}: {ms:7.2} ms a picture (with the read-back)", if on_card { "on the card     " } else { "on the processor" });
        if let Some(f) = &last {
            for (name, v) in &f.info {
                if name.starts_with("cpu_ms.") || name.starts_with("gpu_ms.") {
                    println!("    {name:<28}{v:8.2} ms");
                }
            }
            pictures.push((on_card, f.rgba.clone()));
        }
    }
    // the pictures agree
    let (a, b) = (&pictures[0].1, &pictures[1].1);
    let diff: f64 = a.iter().zip(b.iter()).map(|(x, y)| (*x as f64 - *y as f64).abs()).sum::<f64>() / a.len() as f64;
    println!("mean difference between the two pictures: {diff:.4} / 255");
}
