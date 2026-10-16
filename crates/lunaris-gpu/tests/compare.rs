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
    // the ground of a terrain and the water of a water body are read from the workspace
    sim.asset_root = Some(root.clone());
    sim.load_scene(&lunaris_json::parse(&text).unwrap()).unwrap();
    sim.step_n(ticks);
    sim
}

/// One renderer for every test of this file (they take turns): building one compiles every pipeline it uses, which takes minutes on a machine that draws with the CPU, and a test needs only to
/// find the renderer as a fresh picture would, which a still always does (its effects start from nothing).
fn gpu_or_skip() -> Option<std::sync::MutexGuard<'static, GpuRenderer>> {
    static GPU: std::sync::OnceLock<Option<std::sync::Mutex<GpuRenderer>>> = std::sync::OnceLock::new();
    let cell = GPU.get_or_init(|| match GpuRenderer::headless() {
        Ok(g) => {
            println!("{}", g.adapter_name());
            Some(std::sync::Mutex::new(g))
        }
        Err(e) => {
            eprintln!("no GPU here ({e}): skipping");
            None
        }
    });
    cell.as_ref().map(|m| m.lock().unwrap_or_else(|e| e.into_inner()))
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
        ("ui_kit3", 3, 640, 360, 1.0, 0.006),
        ("particle_clouds", 0, 640, 360, 1.5, 0.02),
        ("particle_fx", 120, 640, 360, 1.5, 0.02),
        ("views", 0, 640, 360, 1.5, 0.02),
        // a painted valley with a lake: the crates have settled on the water
        ("terrain_layers", 480, 640, 360, 1.0, 0.005),
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
    for (name, ticks) in [("showcase", 0u64), ("models", 0), ("sprites", 0), ("orb_dash", 120), ("moon_hopper", 0), ("clockwork", 300), ("coin_chase", 60), ("pile", 120), ("particles", 150), ("ui_showcase", 0), ("ui_kit3", 3), ("particle_clouds", 0), ("particle_fx", 120), ("views", 0), ("terrain_layers", 480)] {
        let mut sim = load(&root, name, ticks);
        // small: a picture in a high quality is eight draws (its effects settle), and a machine that draws with the CPU makes each of them slowly
        let opts = RenderOptions { width: 480, height: 270, ..RenderOptions::default() };
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
    for (name, ticks) in [("showcase", 0u64), ("models", 0), ("sprites", 0), ("coin_chase", 60), ("pile", 120), ("clockwork", 300), ("terrain_layers", 480)] {
        let mut sim = load(&root, name, ticks);
        let opts = RenderOptions { width: 320, height: 180, ..RenderOptions::default() };
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

/// Scroll views: things that stick out of a viewport on every side (rounded and flat shapes, built-in and TrueType text of a `Ui` and of a `Text`, a picture). The reference renderer crops them
/// pixel by pixel; the GPU renderer sets scissor rectangles and cuts the letter quads. Both must leak nothing outside the viewport and agree inside it.
#[test]
fn scroll_views_cut_the_interface_at_the_viewport_the_same_way_in_both_renderers() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let root = repo_root();
    let font = r#","TextFont":{"file":"examples/fonts/pixel.ttf"}"#;
    let (w, h) = (320u32, 180u32);
    let make = || {
        let parent = r#","Parent":{"target":2}"#;
        let text = format!(
            r#"{{"lunaris":1,"entities":[
{{"uid":1,"components":{{"Transform":{{"pos":[0,0,3]}},"Camera":{{"clear":[0.1,0.1,0.2,1]}}}}}},
{{"uid":2,"components":{{"Ui":{{"pos":[0.1,0.2],"size":[0.55,0.55],"color":[0,0,0,0]}},"UiScroll":{{"drag":false}}}}}},
{{"uid":3,"components":{{"Ui":{{"pos":[0.0,0.1],"size":[1.0,0.3],"color":[1,0,0,1]}}{parent}}}}},
{{"uid":4,"components":{{"Ui":{{"pos":[0.5,0.45],"size":[0.4,0.2],"color":[0.2,0.8,0.3,1],"border":0.01,"border_color":[1,1,1,1]}},"UiStyle":{{"radius":0.05,"color2":[0.0,0.3,0.1,1],"shadow_color":[0,0,0,0.6],"shadow_offset":[0.01,0.02]}}{parent}}}}},
{{"uid":5,"components":{{"Ui":{{"pos":[0.15,0.1],"size":[0.4,0.2],"color":[0,0,0,0],"text":"WWWW","text_size":0.3,"text_color":[1,1,0,1]}}{parent}}}}},
{{"uid":6,"components":{{"Text":{{"value":"HHHH","pos":[0.2,0.6],"size":0.25,"color":[0,1,1,1],"shadow":true}}{parent}}}}},
{{"uid":7,"components":{{"Text":{{"value":"Ab","pos":[0.02,0.3],"size":0.3,"color":[1,0,1,1]}}{font}{parent}}}}},
{{"uid":8,"components":{{"Ui":{{"pos":[0.4,0.1],"size":[0.4,0.25],"color":[0,0,0,0],"text":"Ab","text_size":0.3,"text_color":[0.5,1,0.5,1]}}{font}{parent}}}}},
{{"uid":9,"components":{{"Ui":{{"pos":[0.5,0.15],"size":[0.3,0.4],"color":[0,0,0,0]}},"UiStyle":{{"image":"checker","tint":[1,1,1,1]}}{parent}}}}}
]}}"#
        );
        let mut sim = Sim::new(1);
        sim.load_scene(&lunaris_json::parse(&text).unwrap()).unwrap();
        sim
    };
    let mut sim = make();
    let opts = RenderOptions { width: w, height: h, ..RenderOptions::default() };
    let mut tex_cpu = TextureCache::new(Some(root.clone()));
    let cpu = render(&mut sim.world, &mut tex_cpu, &opts).expect("cpu render");
    let mut tex_gpu = TextureCache::new(Some(root.clone()));
    let g = gpu.render_to_image(&mut sim.world, &mut tex_gpu, &opts, &Quality::classic()).expect("gpu render");
    let out = root.join("target").join("gpu-compare");
    std::fs::create_dir_all(&out).unwrap();
    let (d, diff_img) = compare(&cpu.rgba, &g.rgba);
    png(&out.join("scroll-cpu.png"), w, h, &cpu.rgba);
    png(&out.join("scroll-classic.png"), w, h, &g.rgba);
    png(&out.join("scroll-diff.png"), w, h, &diff_img);
    println!("scroll views: mean abs error {:.3}, {:.2}% of pixels differ by more than 40, worst {}", d.mean, d.outliers * 100.0, d.worst);
    assert!(d.mean <= 1.0 && d.outliers <= 0.004, "the renderers differ (mean {}, {:.2}% outliers)", d.mean, d.outliers * 100.0);
    // nothing outside the viewport (x 32..208, y 36..135 on this picture) but the clear colour, in either picture
    let (vx0, vy0, vx1, vy1) = (32u32, 36u32, 208u32, 135u32);
    let clear = [26u8, 26, 51];
    let (mut inside_lit, mut cut) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let o = ((y * w + x) * 4) as usize;
            let in_view = (vx0..vx1).contains(&x) && (vy0..vy1).contains(&y);
            for (name, rgba, tol) in [("reference", &cpu.rgba, 0), ("GPU", &g.rgba, 3)] {
                let is_clear = (0..3).all(|k| (rgba[o + k] as i32 - clear[k] as i32).abs() <= tol);
                if !in_view {
                    assert!(is_clear, "{name} renderer: something is drawn outside the viewport at ({x}, {y}): {:?}", &rgba[o..o + 3]);
                }
            }
            if in_view && !(0..3).all(|k| (cpu.rgba[o + k] as i32 - clear[k] as i32).abs() <= 0) {
                inside_lit += 1;
            }
        }
    }
    // and the same things without the clip do reach outside (so the test would notice a missing clip)
    let mut loose = make();
    for uid in 3..=9u64 {
        let e = loose.world.entity(uid).unwrap();
        loose.world.remove_typed::<lunaris_sim::components::Parent>(e).unwrap();
    }
    let mut tex_loose = TextureCache::new(Some(root.clone()));
    let gl = gpu.render_to_image(&mut loose.world, &mut tex_loose, &opts, &Quality::classic()).expect("gpu render");
    for y in 0..h {
        for x in 0..w {
            let o = ((y * w + x) * 4) as usize;
            if !((vx0..vx1).contains(&x) && (vy0..vy1).contains(&y)) && !(0..3).all(|k| (gl.rgba[o + k] as i32 - clear[k] as i32).abs() <= 3) {
                cut += 1;
            }
        }
    }
    assert!(inside_lit > 3000 && cut > 3000, "{inside_lit} lit pixels inside, {cut} outside without the view");
}

/// Radial fills: sectors of a circle, rings and clock wipes over a box, clockwise and not, from several angles. The reference renderer tests every pixel; the GPU renderer draws triangles.
#[test]
fn radial_fills_are_the_same_shape_in_both_renderers() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let root = repo_root();
    let (w, h) = (480u32, 270u32);
    // (x, y, size in height fractions, radial) on a 16:9 picture: the boxes are square in pixels
    let dials = [
        (0.04, 0.06, r#"{"value":0.3}"#),
        (0.28, 0.06, r#"{"value":0.6,"clockwise":false,"start":45}"#),
        (0.52, 0.06, r#"{"value":0.4,"start":200,"shape":1}"#),
        (0.76, 0.06, r#"{"value":0.8,"inner":0.6,"start":90}"#),
        (0.04, 0.55, r#"{"value":0.95,"inner":0.8,"clockwise":false}"#),
        (0.28, 0.55, r#"{"value":0.125,"shape":1,"start":315}"#),
        (0.52, 0.55, r#"{"value":0.5,"start":270,"inner":0.3}"#),
        (0.76, 0.55, r#"{"value":0.7,"shape":1,"clockwise":false,"start":100}"#),
    ];
    let mut entities = vec![r#"{"uid":1,"components":{"Transform":{"pos":[0,0,3]},"Camera":{"clear":[0.1,0.1,0.2,1]}}}"#.to_string()];
    for (i, (x, y, radial)) in dials.iter().enumerate() {
        let (bw, bh) = (0.36 * 270.0 / 480.0, 0.36);
        entities.push(format!(
            r#"{{"uid":{},"components":{{"Ui":{{"pos":[{x},{y}],"size":[{bw},{bh}],"color":[0.9,{},0.2,0.85],"back_color":[0.15,0.2,0.35,0.9]}},"UiRadial":{radial}}}}}"#,
            10 + i,
            0.3 + 0.08 * i as f32
        ));
    }
    let mut sim = Sim::new(1);
    sim.load_scene(&lunaris_json::parse(&format!(r#"{{"lunaris":1,"entities":[{}]}}"#, entities.join(","))).unwrap()).unwrap();
    let opts = RenderOptions { width: w, height: h, ..RenderOptions::default() };
    let mut tex_cpu = TextureCache::new(Some(root.clone()));
    let cpu = render(&mut sim.world, &mut tex_cpu, &opts).expect("cpu render");
    let mut tex_gpu = TextureCache::new(Some(root.clone()));
    let g = gpu.render_to_image(&mut sim.world, &mut tex_gpu, &opts, &Quality::classic()).expect("gpu render");
    let out = root.join("target").join("gpu-compare");
    std::fs::create_dir_all(&out).unwrap();
    let (d, diff_img) = compare(&cpu.rgba, &g.rgba);
    png(&out.join("radial-cpu.png"), w, h, &cpu.rgba);
    png(&out.join("radial-classic.png"), w, h, &g.rgba);
    png(&out.join("radial-diff.png"), w, h, &diff_img);
    println!("radial fills: mean abs error {:.3}, {:.2}% of pixels differ by more than 40, worst {}", d.mean, d.outliers * 100.0, d.worst);
    // eight big dials are mostly edge: the reference renderer anti-aliases the circle and the card fills a polygon, and a pixel whose middle is exactly on a straight edge goes either way
    assert!(d.mean <= 1.0 && d.outliers <= 0.012, "the renderers differ (mean {}, {:.2}% outliers)", d.mean, d.outliers * 100.0);
    // the picture is not empty: the dials are really there
    let lit = cpu.rgba.chunks(4).filter(|p| p[0] > 120).count();
    assert!(lit > 8000, "{lit} pixels are drawn");
    // the high quality mode draws them too (no reference to compare with there)
    let hi = gpu.render_to_image(&mut sim.world, &mut TextureCache::new(Some(root.clone())), &opts, &Quality::high()).expect("gpu high render");
    png(&out.join("radial-high.png"), w, h, &hi.rgba);
    assert!(hi.rgba.chunks(4).filter(|p| p[0] > 120).count() > 6000);
}

/// Drive a scene with the pointer: `(x, y, down)` for each of the given ticks.
fn point(sim: &mut Sim, x: f64, y: f64, down: bool, ticks: u64) {
    for (name, v) in [("input.mouse_x", x), ("input.mouse_y", y), ("input.mouse_left", down as u8 as f64)] {
        sim.world.set_resource(name, Some(v)).unwrap();
    }
    sim.step_n(ticks);
}

/// The example `ui_kit3` in the states a person would see it in: a drop-down open over the scene, a tab switched, the list scrolled to its end with the bar at the bottom. Both renderers must draw each state
/// the same (the open list is drawn over the other widgets; hidden panels are not drawn at all).
#[test]
fn the_ui_kit3_widgets_are_drawn_the_same_in_both_renderers_in_every_state() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let root = repo_root();
    let out = root.join("target").join("gpu-compare");
    std::fs::create_dir_all(&out).unwrap();
    let (w, h) = (640u32, 360u32);
    let mut sim = load(&root, "ui_kit3", 3);
    let opts = RenderOptions { width: w, height: h, ..RenderOptions::default() };
    let mut check = |sim: &mut Sim, name: &str| {
        let mut tex_cpu = TextureCache::new(Some(root.clone()));
        let cpu = render(&mut sim.world, &mut tex_cpu, &opts).expect("cpu render");
        let mut tex_gpu = TextureCache::new(Some(root.clone()));
        let g = gpu.render_to_image(&mut sim.world, &mut tex_gpu, &opts, &Quality::classic()).expect("gpu render");
        let (d, diff_img) = compare(&cpu.rgba, &g.rgba);
        png(&out.join(format!("ui_kit3-{name}-cpu.png")), w, h, &cpu.rgba);
        png(&out.join(format!("ui_kit3-{name}-classic.png")), w, h, &g.rgba);
        png(&out.join(format!("ui_kit3-{name}-diff.png")), w, h, &diff_img);
        println!("ui_kit3 {name}: mean abs error {:.3}, {:.2}% of pixels differ by more than 40, worst {}", d.mean, d.outliers * 100.0, d.worst);
        assert!(d.mean <= 1.0 && d.outliers <= 0.006, "{name}: the renderers differ (mean {}, {:.2}% outliers)", d.mean, d.outliers * 100.0);
        cpu.rgba
    };
    let start = check(&mut sim, "start");
    // the drop-down open, the pointer on its second option
    point(&mut sim, 0.49, 0.185, true, 2);
    point(&mut sim, 0.49, 0.185, false, 4);
    point(&mut sim, 0.49, 0.325, false, 2);
    let open = check(&mut sim, "dropdown-open");
    assert_ne!(start, open, "the open list is in the picture");
    // closed again by a press outside, the second tab selected
    point(&mut sim, 0.9, 0.9, true, 2);
    point(&mut sim, 0.9, 0.9, false, 2);
    point(&mut sim, 0.49, 0.5275, true, 2);
    point(&mut sim, 0.49, 0.5275, false, 3);
    let tab = check(&mut sim, "second-tab");
    assert_ne!(open, tab);
    assert_ne!(start, tab);
    // the list scrolled to its end
    sim.world.set_resource("input.mouse_wheel", Some(-1.0)).unwrap();
    point(&mut sim, 0.15, 0.35, false, 40);
    sim.world.set_resource("input.mouse_wheel", Some(0.0)).unwrap();
    sim.step_n(2);
    let end = check(&mut sim, "list-end");
    assert_ne!(tab, end);
    // the sword picked up and held over the second slot, then dropped there
    point(&mut sim, 0.75, 0.239, true, 2);
    point(&mut sim, 0.88, 0.30, true, 3);
    let held = check(&mut sim, "item-held");
    assert_ne!(end, held);
    point(&mut sim, 0.88, 0.30, false, 3);
    let dropped = check(&mut sim, "item-dropped");
    assert_ne!(held, dropped);
    assert_ne!(end, dropped, "the sword is in the other slot");
    // the pointer leaves (a press takes a tooltip away until it has), comes back and rests on the sword: its tooltip is up
    point(&mut sim, 0.5, 0.95, false, 2);
    point(&mut sim, 0.89, 0.239, false, 45);
    let tip = check(&mut sim, "tooltip");
    assert_ne!(dropped, tip, "the tooltip box is in the picture");
    // the ability used: the ring and the clock wipe part of the way round
    point(&mut sim, 0.835, 0.612, true, 2);
    point(&mut sim, 0.835, 0.612, false, 62);
    let cool = check(&mut sim, "cooldown");
    assert_ne!(tip, cool);
}

// ------------------------------------------------------------------------------------------------ painted terrain (`TerrainLayers`)

fn write_grey16(path: &PathBuf, n: u32, samples: &[u16]) {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for v in samples {
        bytes.extend_from_slice(&v.to_be_bytes());
    }
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path).unwrap()), n, n);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Sixteen);
    enc.write_header().unwrap().write_image_data(&bytes).unwrap();
}

fn write_rgba8(path: &PathBuf, w: u32, h: u32, rgba: &[u8]) {
    png(path, w, h, rgba);
}

/// The scene text of a ground made of `heightmap`, painted by `splat` with `layers`, seen by an explicit camera (so no camera entity): a sun, a sky, and the ground.
fn painted_scene(dir: &str, layers: &str, size: f32, relief: f32) -> Sim {
    let text = format!(
        r#"{{"lunaris":1,"entities":[
{{"uid":1,"components":{{"Transform":{{"rot":[-0.5,0.32,0.12,0.8]}},"Light":{{"kind":0,"color":[1,0.95,0.85],"intensity":2.2}}}}}},
{{"uid":2,"components":{{"Environment":{{"sky":true,"sky_top":[0.2,0.4,0.8],"sky_horizon":[0.8,0.85,0.95],"clouds":0.3,"bloom":0.1}}}}}},
{{"uid":3,"components":{{"Transform":{{"pos":[0,0,0]}},"Terrain":{{"heightmap":"{dir}/hills.png","size":[{size},{size}],"height":{relief},"color":[1,1,1,1],"uv_scale":[16,16]}},"TerrainLayers":{{"splat":"{dir}/splat.png","layers":{}}}}}}}
]}}"#,
        lunaris_json::Json::str(layers).to_string_lossy()
    );
    let mut sim = Sim::new(1);
    sim.load_scene(&lunaris_json::parse(&text).unwrap()).unwrap();
    sim
}

fn above(eye: [f32; 3], target: [f32; 3], up: [f32; 3], fov: f32) -> lunaris_render::View {
    lunaris_render::View::Explicit(lunaris_render::ExplicitCamera { eye, target, up, fov_y: fov, ..Default::default() })
}

/// Hills painted with the four built-in materials by height: the two renderers must blend the same layers in the same places. The classic GPU picture is compared with the reference pixel by pixel
/// (nearest-neighbour textures, bilinear splat); the high-quality picture is written for a look and must show every layer.
#[test]
fn a_painted_terrain_is_blended_the_same_way_in_both_renderers() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let root = repo_root();
    let out = root.join("target").join("gpu-compare");
    let ws = out.join("painted");
    std::fs::create_dir_all(&ws).unwrap();
    // 65 x 65 hills; the splat map has a weight per layer: sand low, grass above it, rock in a few patches, snow on the high ground
    let n = 65usize;
    let mut heights = vec![0u16; n * n];
    let mut splat = Vec::with_capacity(n * n * 4);
    for j in 0..n {
        for i in 0..n {
            let (x, z) = (i as f32 / (n - 1) as f32, j as f32 / (n - 1) as f32);
            let h = (0.5 + 0.28 * (x * 9.0).sin() * (z * 7.0 + 1.0).cos() + 0.25 * ((x - 0.5) * 3.0).cos() * ((z - 0.5) * 3.0).cos() - 0.25).clamp(0.0, 1.0);
            heights[j * n + i] = (h * 65535.0) as u16;
            let step = |a: f32, b: f32, v: f32| ((v - a) / (b - a)).clamp(0.0, 1.0);
            let sand = 1.0 - step(0.30, 0.42, h);
            let snow = step(0.62, 0.74, h);
            let rock = (((x * 23.0).sin() * (z * 19.0).sin()) * 2.0).clamp(0.0, 1.0) * (1.0 - snow);
            let grass = (1.0 - sand - snow).max(0.0) * (1.0 - rock);
            splat.extend_from_slice(&[(grass * 255.0) as u8, (rock * 255.0) as u8, (sand * 255.0) as u8, (snow * 255.0) as u8]);
        }
    }
    write_grey16(&ws.join("hills.png"), n as u32, &heights);
    write_rgba8(&ws.join("splat.png"), n as u32, n as u32, &splat);
    let layers = "grass grass uv_scale=10\nrock stone uv_scale=8\nsand sand uv_scale=12\nsnow concrete uv_scale=10 tint=1.7,1.7,1.8";
    let mut sim = painted_scene("target/gpu-compare/painted", layers, 96.0, 18.0);
    let (w, h) = (640u32, 360u32);
    let view = above([0.0, 30.0, 62.0], [0.0, 4.0, 0.0], [0.0, 1.0, 0.0], 58.0);
    let opts = RenderOptions { width: w, height: h, view: view.clone(), ..RenderOptions::default() };
    let mut tex_cpu = TextureCache::new(Some(root.clone()));
    let cpu = render(&mut sim.world, &mut tex_cpu, &opts).expect("cpu render");
    let mut tex_gpu = TextureCache::new(Some(root.clone()));
    let g = gpu.render_to_image(&mut sim.world, &mut tex_gpu, &opts, &Quality::classic()).expect("gpu render");
    assert!(cpu.warnings.is_empty() && g.warnings.is_empty(), "{:?} {:?}", cpu.warnings, g.warnings);
    let (d, diff_img) = compare(&cpu.rgba, &g.rgba);
    png(&out.join("painted-cpu.png"), w, h, &cpu.rgba);
    png(&out.join("painted-classic.png"), w, h, &g.rgba);
    png(&out.join("painted-diff.png"), w, h, &diff_img);
    println!("painted terrain: mean abs error {:.3}, {:.2}% of pixels differ by more than 40, worst {}", d.mean, d.outliers * 100.0, d.worst);
    assert!(d.mean <= 1.0 && d.outliers <= 0.005, "the renderers differ (mean {}, {:.2}% outliers)", d.mean, d.outliers * 100.0);
    // the ground is what is picked, in both
    assert_eq!(cpu.pick(320, 250), Some(3));
    assert_eq!(g.pick(320, 250), Some(3));
    // the layers are really there: the picture has far more colours than the one texture the same terrain would have, and it differs from the plain terrain
    let colours: std::collections::BTreeSet<[u8; 3]> = cpu.rgba.chunks(4).map(|p| [p[0], p[1], p[2]]).collect();
    assert!(colours.len() > 400, "{} colours", colours.len());
    let hi = gpu.render_to_image(&mut sim.world, &mut TextureCache::new(Some(root.clone())), &opts, &Quality::high()).expect("gpu high render");
    png(&out.join("painted-high.png"), w, h, &hi.rgba);
    let again = gpu.render_to_image(&mut sim.world, &mut TextureCache::new(Some(root.clone())), &opts, &Quality::high()).expect("gpu high render");
    assert_eq!(hi.rgba, again.rgba, "the same world draws the same picture twice");
    let hi_colours: std::collections::BTreeSet<[u8; 3]> = hi.rgba.chunks(4).map(|p| [p[0], p[1], p[2]]).collect();
    assert!(hi_colours.len() > 1000, "{} colours in high quality", hi_colours.len());
    // without the component the same ground is drawn another way in every quality
    let e = sim.world.entity(3).unwrap();
    sim.world.remove_typed::<lunaris_sim::components::TerrainLayers>(e).unwrap();
    let plain_cpu = render(&mut sim.world, &mut TextureCache::new(Some(root.clone())), &opts).expect("cpu render");
    let plain_hi = gpu.render_to_image(&mut sim.world, &mut TextureCache::new(Some(root.clone())), &opts, &Quality::high()).expect("gpu high render");
    assert_ne!(plain_cpu.rgba, cpu.rgba);
    assert_ne!(plain_hi.rgba, hi.rgba);
}

/// Four solid layers (red, green, blue, yellow) on the four quarters of the ground, seen from above: each layer is where the splat map puts it, row 0 of the map is the north edge, and the
/// same holds in the classic and in the high qualities.
#[test]
fn each_layer_is_where_the_splat_map_puts_it_in_every_quality() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let root = repo_root();
    let out = root.join("target").join("gpu-compare");
    let ws = out.join("quarters");
    std::fs::create_dir_all(&ws).unwrap();
    write_grey16(&ws.join("hills.png"), 9, &vec![0u16; 81]);
    // 8 x 8 splat: north-west layer 1 (red), north-east layer 2 (green), south-west layer 3 (blue), south-east layer 4 (yellow)
    let mut splat = Vec::new();
    for row in 0..8 {
        for col in 0..8 {
            splat.extend_from_slice(&match (row < 4, col < 4) {
                (true, true) => [255u8, 0, 0, 0],
                (true, false) => [0, 255, 0, 0],
                (false, true) => [0, 0, 255, 0],
                (false, false) => [0, 0, 0, 255],
            });
        }
    }
    write_rgba8(&ws.join("splat.png"), 8, 8, &splat);
    for (name, rgb) in [("red", [255u8, 0, 0]), ("green", [0, 255, 0]), ("blue", [0, 0, 255]), ("yellow", [255, 255, 0])] {
        let mut px = Vec::new();
        for _ in 0..16 {
            px.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        write_rgba8(&ws.join(format!("{name}.png")), 4, 4, &px);
    }
    let layers: String = ["red", "green", "blue", "yellow"].iter().map(|c| format!("{c} target/gpu-compare/quarters/{c}.png uv_scale=4")).collect::<Vec<_>>().join("
");
    let mut sim = painted_scene("target/gpu-compare/quarters", &layers, 32.0, 4.0);
    let (w, h) = (200u32, 200u32);
    // from straight above with north at the top of the picture
    let opts = RenderOptions { width: w, height: h, view: above([0.0, 40.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, -1.0], 45.0), ..RenderOptions::default() };
    // which of the four colours a pixel is, by how far one channel stands out (the high quality has a sun, a sky and a filmic tone curve that wash colours out: a smaller margin there)
    let hue = |c: &[u8], margin: i32| -> &'static str {
        let (r, g, b) = (c[0] as i32, c[1] as i32, c[2] as i32);
        if r - g > margin && r - b > margin {
            "red"
        } else if g - r > margin && g - b > margin {
            "green"
        } else if b - r > margin && b - g > margin {
            "blue"
        } else if r - b > margin && g - b > margin && (r - g).abs() < margin + 20 {
            "yellow"
        } else {
            "?"
        }
    };
    // the middle of each quarter of the picture (the 32 m ground covers about 170 pixels)
    let spots = [("red", 45u32, 45u32), ("green", 155, 45), ("blue", 45, 155), ("yellow", 155, 155)];
    let mut tex_cpu = TextureCache::new(Some(root.clone()));
    let cpu = render(&mut sim.world, &mut tex_cpu, &opts).expect("cpu render");
    assert!(cpu.warnings.is_empty(), "{:?}", cpu.warnings);
    let g = gpu.render_to_image(&mut sim.world, &mut TextureCache::new(Some(root.clone())), &opts, &Quality::classic()).expect("gpu classic render");
    let hi = gpu.render_to_image(&mut sim.world, &mut TextureCache::new(Some(root.clone())), &opts, &Quality::high()).expect("gpu high render");
    png(&out.join("quarters-cpu.png"), w, h, &cpu.rgba);
    png(&out.join("quarters-classic.png"), w, h, &g.rgba);
    png(&out.join("quarters-high.png"), w, h, &hi.rgba);
    for (picture, name, margin) in [(&cpu, "reference", 50), (&g, "GPU classic", 50), (&hi, "GPU high", 15)] {
        for &(want, x, y) in &spots {
            let o = ((y * w + x) * 4) as usize;
            assert_eq!(hue(&picture.rgba[o..o + 3], margin), want, "{name} at ({x}, {y}): {:?}", &picture.rgba[o..o + 3]);
        }
    }
    // the weights blend smoothly: half way between the red and the green quarter, along the north, both show
    let mid = |p: &lunaris_render::Frame| -> [u8; 3] {
        let o = ((45 * w + 100) * 4) as usize;
        [p.rgba[o], p.rgba[o + 1], p.rgba[o + 2]]
    };
    for (picture, name) in [(&cpu, "reference"), (&g, "GPU classic")] {
        let m = mid(picture);
        assert!(m[0] > 60 && m[1] > 60, "{name}: red and green blend between the quarters: {m:?}");
    }
}

// ------------------------------------------------------------------------------------------------ water bodies (`WaterBody`)

/// Two lakes at different levels in two basins of a hilly ground, from one 16-bit picture of the water's heights: the surface is where the picture says in both renderers, each lake at its level,
/// hidden by the banks that stand higher, and the high quality rolls waves over it.
#[test]
fn a_water_body_follows_its_picture_in_both_renderers() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let root = repo_root();
    let out = root.join("target").join("gpu-compare");
    let ws = out.join("lakes");
    std::fs::create_dir_all(&ws).unwrap();
    // 97 x 97 samples over 96 m, 24 m of relief: a ridge between a west basin and an east basin
    let n = 97usize;
    let relief = 24.0f32;
    let ground = |x: f32, z: f32| -> f32 {
        // metres from the middle, -48 to 48
        let (west, east) = (((x + 22.0).powi(2) + z.powi(2)) / 230.0, ((x - 24.0).powi(2) + (z - 4.0).powi(2)) / 190.0);
        (14.0 - 11.0 * (-west).exp() - 9.0 * (-east).exp() + 0.5 * (x * 0.3).sin() * (z * 0.25).cos()).clamp(0.0, relief)
    };
    let mut heights = vec![0u16; n * n];
    let mut water = vec![0u16; n * n];
    for j in 0..n {
        for i in 0..n {
            let (x, z) = (i as f32 - 48.0, j as f32 - 48.0);
            let g = ground(x, z);
            heights[j * n + i] = (g / relief * 65535.0) as u16;
            // the west lake stands 5.5 m up, the east one 8 m: wet where the ground is under the surface
            let level = if x < 0.0 { 5.5 } else { 8.0 };
            if g < level {
                water[j * n + i] = (level / relief * 65535.0) as u16;
            }
        }
    }
    write_grey16(&ws.join("ground.png"), n as u32, &heights);
    write_grey16(&ws.join("water.png"), n as u32, &water);
    let scene = |with_water: bool| {
        let lake = if with_water { r#",{"uid":4,"components":{"Transform":{"pos":[0,0,0]},"WaterBody":{"heightmap":"target/gpu-compare/lakes/water.png","size":[96,96],"relief":24,"color":[0.05,0.3,0.45,0.85],"wave_height":0.25,"wave_length":7}}}"# } else { "" };
        let text = format!(
            r#"{{"lunaris":1,"entities":[
{{"uid":1,"components":{{"Transform":{{"rot":[-0.5,0.32,0.12,0.8]}},"Light":{{"kind":0,"color":[1,0.95,0.85],"intensity":2.2}}}}}},
{{"uid":2,"components":{{"Environment":{{"sky":true,"sky_top":[0.2,0.4,0.8],"sky_horizon":[0.8,0.85,0.95],"clouds":0.3,"bloom":0.1}}}}}},
{{"uid":3,"components":{{"Transform":{{"pos":[0,0,0]}},"Terrain":{{"heightmap":"target/gpu-compare/lakes/ground.png","size":[96,96],"height":24,"color":[0.45,0.55,0.35,1]}}}}}}{lake}
]}}"#
        );
        let mut sim = Sim::new(1);
        sim.load_scene(&lunaris_json::parse(&text).unwrap()).unwrap();
        sim
    };
    let (w, h) = (640u32, 360u32);
    let opts = RenderOptions { width: w, height: h, view: above([0.0, 46.0, 70.0], [0.0, 3.0, 0.0], [0.0, 1.0, 0.0], 55.0), ..RenderOptions::default() };
    let mut sim = scene(true);
    let cpu = render(&mut sim.world, &mut TextureCache::new(Some(root.clone())), &opts).expect("cpu render");
    let g = gpu.render_to_image(&mut sim.world, &mut TextureCache::new(Some(root.clone())), &opts, &Quality::classic()).expect("gpu render");
    assert!(cpu.warnings.is_empty() && g.warnings.is_empty(), "{:?} {:?}", cpu.warnings, g.warnings);
    let (d, diff_img) = compare(&cpu.rgba, &g.rgba);
    png(&out.join("lakes-cpu.png"), w, h, &cpu.rgba);
    png(&out.join("lakes-classic.png"), w, h, &g.rgba);
    png(&out.join("lakes-diff.png"), w, h, &diff_img);
    println!("water bodies: mean abs error {:.3}, {:.2}% of pixels differ by more than 40, worst {}", d.mean, d.outliers * 100.0, d.worst);
    assert!(d.mean <= 1.0 && d.outliers <= 0.005, "the renderers differ (mean {}, {:.2}% outliers)", d.mean, d.outliers * 100.0);
    // the water is what is picked over the lakes and the ground over the banks, in both renderers (the id pictures)
    let water_pixels = |f: &lunaris_render::Frame| f.ids.iter().filter(|i| **i == 4).count();
    assert!(water_pixels(&cpu) > 1000, "{} water pixels", water_pixels(&cpu));
    let (a, b) = (water_pixels(&cpu) as f64, water_pixels(&g) as f64);
    assert!((a - b).abs() / a < 0.03, "the lakes cover {a} pixels in the reference and {b} on the GPU");
    // without the water entity the same ground is a different picture; with it the high quality is blue where the lakes are and not flat
    let mut dry = scene(false);
    let dry_cpu = render(&mut dry.world, &mut TextureCache::new(Some(root.clone())), &opts).expect("cpu render");
    assert_ne!(dry_cpu.rgba, cpu.rgba);
    let hi = gpu.render_to_image(&mut sim.world, &mut TextureCache::new(Some(root.clone())), &opts, &Quality::high()).expect("gpu high render");
    png(&out.join("lakes-high.png"), w, h, &hi.rgba);
    let bluish = |f: &lunaris_render::Frame, left: bool| {
        let mut n = 0;
        for (k, id) in f.ids.iter().enumerate() {
            let x = k as u32 % f.width;
            if *id == 4 && (x < f.width / 2) == left {
                let p = &f.rgba[k * 4..k * 4 + 3];
                if p[2] > p[0] + 8 {
                    n += 1;
                }
            }
        }
        n
    };
    assert!(bluish(&hi, true) > 300 && bluish(&hi, false) > 300, "both lakes are blue in high quality: {} {}", bluish(&hi, true), bluish(&hi, false));
    let again = gpu.render_to_image(&mut sim.world, &mut TextureCache::new(Some(root.clone())), &opts, &Quality::high()).expect("gpu high render");
    assert_eq!(hi.rgba, again.rgba, "the same world draws the same picture twice");
    // the waves are in the simulation's clock: later in time the surface is not the same picture
    sim.world.get_mut::<lunaris_sim::components::WaterBody>(sim.world.entity(4).unwrap()).unwrap().phase = 2.0;
    let later = gpu.render_to_image(&mut sim.world, &mut TextureCache::new(Some(root.clone())), &opts, &Quality::high()).expect("gpu high render");
    assert_ne!(hi.rgba, later.rgba, "the waves move with the phase");
}

/// Not a pass/fail test: prints how fast the GPU draws a busy scene compared with the CPU reference (run with --nocapture, in release
/// mode for honest numbers: `cargo test --release -p lunaris-gpu --test compare how_fast -- --nocapture`).
/// A speed measurement, not a check (it asserts nothing): 30 pictures of 1600 meshes at two sizes in two qualities. A picture asked for by `render_to_image` is a still, which in the high qualities with
/// temporal anti-aliasing is drawn eight times, so the numbers are for stills; and on a machine that draws with the CPU it takes hours, so it only runs when asked:
/// `cargo test --release -p lunaris-gpu --test compare how_fast -- --ignored --nocapture`.
#[test]
#[ignore = "a speed measurement: run it with --ignored --nocapture"]
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
