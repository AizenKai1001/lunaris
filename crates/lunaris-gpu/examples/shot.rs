//! Draw a scene file with the GPU renderer and save the picture: `cargo run --release -p lunaris-gpu --example shot -- examples/showcase.lunaris.json out.png [width height [quality [ticks]]]`
//! (quality: classic, low, medium, high, ultra).
use lunaris_gpu::{GpuRenderer, Quality};
use lunaris_render::{RenderOptions, TextureCache};
use lunaris_sim::Sim;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: shot SCENE.json OUT.png [width height [quality [ticks]]]");
        std::process::exit(2);
    }
    let (w, h) = (args.get(2).and_then(|v| v.parse().ok()).unwrap_or(960u32), args.get(3).and_then(|v| v.parse().ok()).unwrap_or(540u32));
    let quality = Quality::from_name(args.get(4).map(String::as_str).unwrap_or("high")).expect("quality: classic, low, medium, high, ultra");
    let ticks: u64 = args.get(5).and_then(|v| v.parse().ok()).unwrap_or(0);
    let text = std::fs::read_to_string(&args[0]).expect("cannot read the scene");
    let mut sim = Sim::new(1);
    sim.load_scene(&lunaris_json::parse(&text).expect("scene is not JSON")).expect("scene does not load");
    sim.step_n(ticks);
    let root = std::path::Path::new(&args[0]).parent().and_then(|p| p.parent()).map(|p| p.to_path_buf()).unwrap_or_else(|| ".".into());
    let mut tex = TextureCache::new(Some(root));
    let mut gpu = GpuRenderer::headless().expect("no GPU");
    let opts = RenderOptions { width: w, height: h, ..RenderOptions::default() };
    // draw once to warm up, then time a few frames
    let t = std::time::Instant::now();
    let f = gpu.render_to_image(&mut sim.world, &mut tex, &opts, &quality).expect("render");
    let first = t.elapsed();
    let n = 10;
    let t = std::time::Instant::now();
    for _ in 0..n {
        gpu.render_to_image(&mut sim.world, &mut tex, &opts, &quality).expect("render");
    }
    println!("{} | first frame {:.1} ms, then {:.2} ms per frame (with readback)", gpu.adapter_name(), first.as_secs_f64() * 1000.0, t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    for w in &f.warnings {
        println!("warning: {w}");
    }
    let file = std::fs::File::create(&args[1]).expect("cannot create the picture");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), f.width, f.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&f.rgba).unwrap();
}
