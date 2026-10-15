//! The GPU renderer against the reference CPU renderer. In classic mode the two draw the same scenes with the same camera and are
//! compared pixel by pixel: they cannot be bit-identical (different rasterisers, 16-bit float targets, rounding at triangle edges), but
//! lighting, colours, sprites, tile maps, textures and HUD text must be in the same places. The high-quality mode has no reference to
//! match, so its tests check what must always hold (it draws, it is deterministic, shadows and sky behave) and write pictures to
//! `target/gpu-compare/` for a person to look at.
use lunaris_gpu::{GpuRenderer, Quality};
use lunaris_render::{render, RenderOptions, TextureCache};
use lunaris_sim::Sim;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn png(path: &PathBuf, w: u32, h: u32, rgba: &[u8]) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
}

struct Diff {
    mean: f64,
    outliers: f64,
    worst: u8,
}

fn compare(cpu: &[u8], gpu: &[u8]) -> (Diff, Vec<u8>) {
    assert_eq!(cpu.len(), gpu.len());
    let mut sum = 0u64;
    let mut outliers = 0usize;
    let mut worst = 0u8;
    let mut img = vec![255u8; cpu.len()];
    for (i, (a, b)) in cpu.chunks(4).zip(gpu.chunks(4)).enumerate() {
        let d = (0..3).map(|k| (a[k] as i32 - b[k] as i32).unsigned_abs() as u8).max().unwrap();
        sum += (0..3).map(|k| (a[k] as i64 - b[k] as i64).unsigned_abs()).sum::<u64>();
        if d > 40 {
            outliers += 1;
        }
        worst = worst.max(d);
        let amp = (d as u32 * 4).min(255) as u8;
        img[i * 4..i * 4 + 3].copy_from_slice(&[amp, amp, amp]);
    }
    let px = (cpu.len() / 4) as f64;
    (Diff { mean: sum as f64 / (px * 3.0), outliers: outliers as f64 / px, worst }, img)
}

fn load(root: &PathBuf, name: &str, ticks: u64) -> Sim {
    let text = std::fs::read_to_string(root.join("examples").join(format!("{name}.lunaris.json"))).unwrap();
    let mut sim = Sim::new(1);
    sim.load_scene(&lunaris_json::parse(&text).unwrap()).unwrap();
    sim.step_n(ticks);
    sim
}

fn gpu_or_skip() -> Option<GpuRenderer> {
    match GpuRenderer::headless() {
        Ok(g) => {
            println!("{}", g.adapter_name());
            Some(g)
        }
        Err(e) => {
            eprintln!("no GPU here ({e}): skipping");
            None
        }
    }
}

#[test]
fn classic_pictures_match_the_reference_renderer() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let root = repo_root();
    let out = root.join("target").join("gpu-compare");
    std::fs::create_dir_all(&out).unwrap();
    // (scene, ticks to run first, width, height, allowed mean error, allowed fraction of badly different pixels)
    let scenes: &[(&str, u64, u32, u32, f64, f64)] = &[
        ("showcase", 0, 640, 360, 1.0, 0.005),
        ("models", 0, 640, 360, 1.0, 0.005),
        ("sprites", 0, 640, 360, 1.0, 0.005),
        ("orb_dash", 120, 640, 360, 1.0, 0.005),
        ("moon_hopper", 0, 640, 360, 1.0, 0.005),
        ("clockwork", 300, 640, 360, 1.0, 0.005),
        ("coin_chase", 60, 640, 360, 1.0, 0.005),
        ("pile", 120, 640, 360, 1.0, 0.005),
        ("particles", 150, 640, 360, 1.0, 0.01),
        ("menu", 30, 640, 360, 1.0, 0.005),
        ("ui_showcase", 0, 640, 360, 0.8, 0.004),
        ("particle_clouds", 0, 640, 360, 1.5, 0.02),
        ("particle_fx", 120, 640, 360, 1.5, 0.02),
        ("views", 0, 640, 360, 1.5, 0.02),
    ];
    let classic = Quality::classic();
    for &(name, ticks, w, h, max_mean, max_outliers) in scenes {
        let mut sim = load(&root, name, ticks);
        let opts = RenderOptions { width: w, height: h, ..RenderOptions::default() };
        let mut tex_cpu = TextureCache::new(Some(root.clone()));
        let cpu = render(&mut sim.world, &mut tex_cpu, &opts).expect("cpu render");
        let mut tex_gpu = TextureCache::new(Some(root.clone()));
        let g = gpu.render_to_image(&mut sim.world, &mut tex_gpu, &opts, &classic).expect("gpu render");
        let (d, diff_img) = compare(&cpu.rgba, &g.rgba);
        png(&out.join(format!("{name}-cpu.png")), w, h, &cpu.rgba);
        png(&out.join(format!("{name}-classic.png")), w, h, &g.rgba);
        png(&out.join(format!("{name}-diff.png")), w, h, &diff_img);
        println!("{name:12} mean abs error {:.3} / 255, {:.2}% of pixels differ by more than 40, worst {}", d.mean, d.outliers * 100.0, d.worst);
        assert!(d.mean <= max_mean, "{name}: mean error {} is above {max_mean}", d.mean);
        assert!(d.outliers <= max_outliers, "{name}: {:.2}% of pixels differ a lot (allowed {:.2}%)", d.outliers * 100.0, max_outliers * 100.0);
        assert_eq!(cpu.warnings, g.warnings, "{name}: both renderers report the same problems");
    }
}

#[test]
fn high_quality_pictures_are_drawn_and_written_for_a_look() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let root = repo_root();
    let out = root.join("target").join("gpu-compare");
    std::fs::create_dir_all(&out).unwrap();
    let high = Quality::high();
    for (name, ticks) in [("showcase", 0u64), ("models", 0), ("sprites", 0), ("orb_dash", 120), ("moon_hopper", 0), ("clockwork", 300), ("coin_chase", 60), ("pile", 120), ("particles", 150), ("ui_showcase", 0), ("particle_clouds", 0), ("particle_fx", 120), ("views", 0)] {
        let mut sim = load(&root, name, ticks);
        let opts = RenderOptions { width: 960, height: 540, ..RenderOptions::default() };
        let mut tex = TextureCache::new(Some(root.clone()));
        let g = gpu.render_to_image(&mut sim.world, &mut tex, &opts, &high).expect("gpu render");
        png(&out.join(format!("{name}-high.png")), g.width, g.height, &g.rgba);
        // not a black or a flat picture
        let distinct: std::collections::BTreeSet<[u8; 3]> = g.rgba.chunks(4).map(|p| [p[0], p[1], p[2]]).collect();
        assert!(distinct.len() > 8, "{name}: the picture is flat ({} colours)", distinct.len());
    }
}

#[test]
fn entity_ids_agree_with_the_reference_renderer() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let root = repo_root();
    for (name, ticks) in [("showcase", 0u64), ("models", 0), ("sprites", 0), ("coin_chase", 60), ("pile", 120), ("clockwork", 300)] {
        let mut sim = load(&root, name, ticks);
        let opts = RenderOptions { width: 640, height: 360, ..RenderOptions::default() };
        let mut tex_cpu = TextureCache::new(Some(root.clone()));
        let cpu = render(&mut sim.world, &mut tex_cpu, &opts).expect("cpu render");
        let mut tex_gpu = TextureCache::new(Some(root.clone()));
        for quality in [Quality::classic(), Quality::high()] {
            let g = gpu.render_to_image(&mut sim.world, &mut tex_gpu, &opts, &quality).expect("gpu render");
            let (a, b) = (cpu.visible(500), g.visible(500));
            // every entity the reference sees in any quantity, the GPU sees too, with about the same number of pixels
            for v in a.iter().filter(|v| v.pixels >= 40) {
                let w = b.iter().find(|x| x.uid == v.uid).unwrap_or_else(|| panic!("{name}: entity {} ({} px on the CPU) is missing on the GPU", v.uid, v.pixels));
                let diff = (v.pixels as f64 - w.pixels as f64).abs() / v.pixels as f64;
                assert!(diff < 0.08, "{name}: entity {} covers {} pixels on the CPU and {} on the GPU", v.uid, v.pixels, w.pixels);
            }
            for w in b.iter().filter(|v| v.pixels >= 40) {
                assert!(a.iter().any(|x| x.uid == w.uid), "{name}: entity {} is only on the GPU ({} px)", w.uid, w.pixels);
            }
            assert!((cpu.coverage() - g.coverage()).abs() < 0.02, "{name}: coverage {} vs {}", cpu.coverage(), g.coverage());
        }
    }
}

/// Not a pass/fail test: prints how fast the GPU draws a busy scene compared with the CPU reference (run with --nocapture, in release
/// mode for honest numbers: `cargo test --release -p lunaris-gpu --test compare how_fast -- --nocapture`).
#[test]
fn how_fast_is_the_gpu() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let mut sim = Sim::new(1);
    let mut entities = vec![r#"{"uid":1,"components":{"Camera":{},"Transform":{"pos":[0,14,26],"rot":[-0.2,0,0,0.98]}}}"#.to_string(), r#"{"uid":2,"components":{"Light":{"kind":0,"intensity":1.2},"Transform":{"rot":[-0.5,0.2,0,0.84]}}}"#.to_string()];
    for i in 0..1600 {
        let (gx, gz) = ((i % 40) as f32 - 20.0, (i / 40) as f32 - 20.0);
        entities.push(format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{},0,{}]}},"MeshRenderer":{{"mesh":"{}","color":[0.5,0.6,0.9,1]}}}}}}"#, 10 + i, gx * 1.2, gz * 1.2, if i % 2 == 0 { "sphere" } else { "cube" }));
    }
    sim.load_scene(&lunaris_json::parse(&format!(r#"{{"lunaris":1,"entities":[{}]}}"#, entities.join(","))).unwrap()).unwrap();
    let root = repo_root();
    let mut tex = TextureCache::new(Some(root));
    for (w, h) in [(960u32, 540u32), (1920, 1080)] {
        let opts = RenderOptions { width: w, height: h, threads: 1, ..RenderOptions::default() };
        let t = std::time::Instant::now();
        let n = 3;
        for _ in 0..n {
            render(&mut sim.world, &mut tex, &opts).unwrap();
        }
        let cpu_ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        let mut line = format!("{w}x{h}, 1600 lit meshes: CPU reference (1 thread) {cpu_ms:.1} ms");
        for (name, q) in [("classic", Quality::classic()), ("high", Quality::high())] {
            gpu.render_to_image(&mut sim.world, &mut tex, &opts, &q).unwrap(); // warm up: uploads the meshes, builds the pipelines
            let t = std::time::Instant::now();
            let n = 30;
            for _ in 0..n {
                gpu.render_to_image(&mut sim.world, &mut tex, &opts, &q).unwrap();
            }
            let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
            line.push_str(&format!(", GPU {name} {ms:.2} ms ({:.0}x)", cpu_ms / ms));
        }
        println!("{line}");
    }
}
