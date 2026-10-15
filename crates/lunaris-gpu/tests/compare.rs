//! The GPU renderer against the reference CPU renderer. In classic mode the two draw the same scenes with the same camera and are
//! compared pixel by pixel: they cannot be bit-identical (different rasterisers, 16-bit float targets, rounding at triangle edges), but
//! lighting, colours, sprites, tile maps, textures and HUD text must be in the same places. The high-quality mode has no reference to
//! match, so its tests check what must always hold (it draws, it is deterministic, shadows and sky behave) and write pictures to
//! `target/gpu-compare/` for a person to look at.
use lunaris_gpu::{GpuRenderer, Quality};
use lunaris_render::{render, RenderOptions, TextureCache};
use lunaris_json::Json;
use lunaris_sim::{Command, Sim};
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

/// `DrawLayer`: three squares at the same z whose layers run against their uid order, a tile map that is nearer in z but in the lowest layer, and a yellow
/// disc of particles that stands among them by its own layer (or, without one, on top of everything). Both renderers must draw the same order.
fn layered_scene(cloud_layer: Option<&str>) -> Sim {
    let cloud = cloud_layer.map_or(String::new(), |l| format!(r#","DrawLayer":{{"layer":{l}}}"#));
    let text = format!(
        r#"{{"lunaris":1,"entities":[
{{"uid":1,"components":{{"Transform":{{"pos":[0,0,5]}},"Camera":{{"ortho":true,"ortho_height":4,"clear":[0,0,0.2,1]}}}}}},
{{"uid":10,"components":{{"Transform":{{"pos":[-0.6,0.2,0]}},"Sprite":{{"size":[2,2],"color":[1,0,0,1]}},"DrawLayer":{{"layer":3}}}}}},
{{"uid":11,"components":{{"Transform":{{"pos":[0,0,0]}},"Sprite":{{"size":[2,2],"color":[0,1,0,1]}},"DrawLayer":{{"layer":2}}}}}},
{{"uid":12,"components":{{"Transform":{{"pos":[0.6,-0.2,0]}},"Sprite":{{"size":[2,2],"color":[0,0,1,1]}},"DrawLayer":{{"layer":1}}}}}},
{{"uid":13,"components":{{"Transform":{{"pos":[0.3,0.3,0]}},"Particles":{{"count":1,"lifetime":4,"looping":false,"cone":0,"speed_range":[0,0],"gravity":[0,0,0],"size":[2.4,2.4],"size_jitter":0,"fade_in":0,"color":[1,1,0,1],"color_end":[1,1,0,1],"texture":"circle","additive":false,"time":1}}{cloud}}}}},
{{"uid":14,"components":{{"Transform":{{"pos":[0,0,1]}},"Tilemap":{{"size":[4,2],"tile":2,"data":"0 0 0 0, 0 0 0 0","color":[0.3,0.3,0.3,1]}},"DrawLayer":{{"layer":-5}}}}}}
]}}"#
    );
    let mut sim = Sim::new(1);
    sim.load_scene(&lunaris_json::parse(&text).unwrap()).unwrap();
    sim
}

/// A pixel is "about" a colour: within `tol` on every channel.
fn about(rgba: &[u8], w: u32, x: u32, y: u32, want: [u8; 3], tol: i32) -> bool {
    let o = ((y * w + x) * 4) as usize;
    (0..3).all(|k| (rgba[o + k] as i32 - want[k] as i32).abs() <= tol)
}

#[test]
fn draw_layers_put_sprites_tile_maps_and_particles_in_the_same_order_in_both_renderers() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let root = repo_root();
    let (w, h) = (320u32, 180u32);
    // 45 pixels a metre, the middle at (160, 90): (name, x, y, the colour there with the cloud between green and red / above red / without a layer)
    let (red, blue, yellow, grey) = ([255u8, 0, 0], [0u8, 0, 255], [255u8, 255, 0], [77u8, 77, 77]);
    let spots: &[(&str, u32, u32, [u8; 3], [u8; 3], [u8; 3])] = &[
        ("red only", 97, 45, red, red, red),
        ("all three, red on top by layer though blue has the highest uid", 160, 90, red, yellow, yellow),
        ("green and blue under the cloud", 191, 63, yellow, yellow, yellow),
        ("blue alone", 223, 130, blue, blue, blue),
        ("the map shows where nothing else is, though it is nearer in z", 25, 157, grey, grey, grey),
    ];
    let classic = Quality::classic();
    for (which, cloud_layer) in [("between green and red", Some("2.5")), ("above red", Some("3.5")), ("no layer", None)] {
        let mut sim = layered_scene(cloud_layer);
        let opts = RenderOptions { width: w, height: h, ..RenderOptions::default() };
        let mut tex_cpu = TextureCache::new(Some(root.clone()));
        let cpu = render(&mut sim.world, &mut tex_cpu, &opts).expect("cpu render");
        let mut tex_gpu = TextureCache::new(Some(root.clone()));
        let g = gpu.render_to_image(&mut sim.world, &mut tex_gpu, &opts, &classic).expect("gpu render");
        let (d, diff_img) = compare(&cpu.rgba, &g.rgba);
        let out = root.join("target").join("gpu-compare");
        std::fs::create_dir_all(&out).unwrap();
        let tag = which.replace(' ', "-");
        png(&out.join(format!("layers-{tag}-cpu.png")), w, h, &cpu.rgba);
        png(&out.join(format!("layers-{tag}-classic.png")), w, h, &g.rgba);
        png(&out.join(format!("layers-{tag}-diff.png")), w, h, &diff_img);
        println!("layers, cloud {which}: mean abs error {:.3}, {:.2}% of pixels differ by more than 40", d.mean, d.outliers * 100.0);
        assert!(d.mean <= 1.5 && d.outliers <= 0.02, "cloud {which}: the renderers differ (mean {}, {:.2}% outliers)", d.mean, d.outliers * 100.0);
        for &(name, x, y, between, above, none) in spots {
            let want = match which {
                "between green and red" => between,
                "above red" => above,
                _ => none,
            };
            assert!(about(&cpu.rgba, w, x, y, want, 8), "cloud {which}, reference renderer, {name} at ({x}, {y}): {:?}", &cpu.rgba[((y * w + x) * 4) as usize..][..3]);
            assert!(about(&g.rgba, w, x, y, want, 12), "cloud {which}, GPU classic, {name} at ({x}, {y}): {:?}", &g.rgba[((y * w + x) * 4) as usize..][..3]);
        }
        // the entity picture follows the same order (a particle cloud is never in it, so the red square is what is picked at the middle)
        for (name, x, y, uid) in [("middle", 160, 90, 10u64), ("blue alone", 223, 130, 12), ("red only", 97, 45, 10)] {
            assert_eq!(cpu.pick(x, y), Some(uid), "cloud {which}, reference renderer, {name}");
            assert_eq!(g.pick(x, y), Some(uid), "cloud {which}, GPU classic, {name}");
        }
        // the high quality mode has no reference to match, but the order of the layers is the same: which channels are lit at each spot
        let hi = gpu.render_to_image(&mut sim.world, &mut TextureCache::new(Some(root.clone())), &opts, &Quality::high()).expect("gpu high render");
        for &(name, x, y, between, above, none) in spots {
            let want = match which {
                "between green and red" => between,
                "above red" => above,
                _ => none,
            };
            if want == grey {
                continue;
            }
            let o = ((y * w + x) * 4) as usize;
            let hi_px = &hi.rgba[o..o + 3];
            assert!((0..3).all(|k| if want[k] > 128 { hi_px[k] > 120 } else { hi_px[k] < 110 }), "cloud {which}, GPU high, {name} at ({x}, {y}): {hi_px:?}, wanted about {want:?}");
        }
    }
}

/// `UiScale`: the interface laid out in the safe area of a picture that is not the shape it was made for. Both renderers draw the same rectangles and texts that `gather_scene` makes, so they must
/// agree at 4:3 and 21:9 as they do at 16:9, and the scale must really change the picture where the shapes differ.
#[test]
fn the_interface_keeps_its_shape_on_other_screens_in_both_renderers() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let root = repo_root();
    let out = root.join("target").join("gpu-compare");
    std::fs::create_dir_all(&out).unwrap();
    let classic = Quality::classic();
    for (name, ticks, max_mean, max_outliers) in [("ui_showcase", 0u64, 1.0, 0.006), ("menu", 30, 1.0, 0.006)] {
        for (w, h) in [(640u32, 360u32), (480, 360), (840, 360)] {
            let make = |scaled: bool| {
                let mut sim = load(&root, name, ticks);
                if scaled {
                    sim.apply(&Command::Spawn { uid: Some(9000), components: vec![("UiScale".to_string(), Json::obj([("aspect", Json::Float(1.7777778))]))] }).unwrap();
                }
                sim
            };
            let opts = RenderOptions { width: w, height: h, ..RenderOptions::default() };
            let mut plain = make(false);
            let mut sim = make(true);
            let mut tex_cpu = TextureCache::new(Some(root.clone()));
            let cpu = render(&mut sim.world, &mut tex_cpu, &opts).expect("cpu render");
            let mut tex_plain = TextureCache::new(Some(root.clone()));
            let unscaled = render(&mut plain.world, &mut tex_plain, &opts).expect("cpu render without a scale");
            let mut tex_gpu = TextureCache::new(Some(root.clone()));
            let g = gpu.render_to_image(&mut sim.world, &mut tex_gpu, &opts, &classic).expect("gpu render");
            let (d, diff_img) = compare(&cpu.rgba, &g.rgba);
            png(&out.join(format!("uiscale-{name}-{w}x{h}-cpu.png")), w, h, &cpu.rgba);
            png(&out.join(format!("uiscale-{name}-{w}x{h}-classic.png")), w, h, &g.rgba);
            png(&out.join(format!("uiscale-{name}-{w}x{h}-diff.png")), w, h, &diff_img);
            println!("ui scale {name} {w}x{h}: mean abs error {:.3}, {:.2}% of pixels differ by more than 40, worst {}", d.mean, d.outliers * 100.0, d.worst);
            assert!(d.mean <= max_mean, "{name} {w}x{h}: mean error {} is above {max_mean}", d.mean);
            assert!(d.outliers <= max_outliers, "{name} {w}x{h}: {:.2}% of pixels differ a lot", d.outliers * 100.0);
            if (w, h) == (640, 360) {
                assert_eq!(cpu.rgba, unscaled.rgba, "{name}: the shape it was made for is drawn as without a scale");
            } else {
                assert_ne!(cpu.rgba, unscaled.rgba, "{name} {w}x{h}: the scale changes the picture");
            }
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
