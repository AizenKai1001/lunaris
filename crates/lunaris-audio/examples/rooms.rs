//! What the acoustic ray tracer says about some rooms, next to the textbook formula, and what it costs.
//!
//!     cargo run --release -p lunaris-audio --example rooms
use lunaris_audio::trace::{analyse_room, trace_source, Occluder, Scene, Shape};
use std::time::Instant;

fn room(w: f32, h: f32, l: f32, absorption: f32) -> Vec<Occluder> {
    let (hw, hh, hl, t) = (w / 2.0, h / 2.0, l / 2.0, 0.1);
    let b = |c: [f32; 3], half: [f32; 3]| Occluder { shape: Shape::Box { center: c, half }, absorption, transmission: 0.02 };
    vec![
        b([0.0, -hh - t, 0.0], [hw + 0.2, t, hl + 0.2]),
        b([0.0, hh + t, 0.0], [hw + 0.2, t, hl + 0.2]),
        b([-hw - t, 0.0, 0.0], [t, hh, hl]),
        b([hw + t, 0.0, 0.0], [t, hh, hl]),
        b([0.0, 0.0, -hl - t], [hw, hh, t]),
        b([0.0, 0.0, hl + t], [hw, hh, t]),
    ]
}

fn eyring(w: f32, h: f32, l: f32, a: f32) -> f32 {
    let (v, s) = (w * h * l, 2.0 * (w * h + h * l + w * l));
    0.1611 * v / (-s * (1.0 - a).ln())
}

fn main() {
    println!("{:<28} {:>9} {:>9} {:>8} {:>8}", "room (w x h x l, absorption)", "Eyring s", "traced s", "closed", "free path");
    for (name, w, h, l, a) in [("bathroom 3x2.5x3, 0.03", 3.0, 2.5, 3.0, 0.03), ("living room 6x3x5, 0.2", 6.0, 3.0, 5.0, 0.2), ("classroom 9x3x8, 0.12", 9.0, 3.0, 8.0, 0.12), ("sports hall 30x10x40, 0.1", 30.0, 10.0, 40.0, 0.1), ("studio 5x3x4, 0.5", 5.0, 3.0, 4.0, 0.5)] {
        let scene = Scene::new(room(w, h, l, a));
        let r = analyse_room(&scene, [0.0, 0.0, 0.0], 256, 400);
        println!("{name:<28} {:>9.2} {:>9.2} {:>8.2} {:>7.1}m", eyring(w, h, l, a), r.rt60, r.closedness, r.mean_free_path);
    }
    let field = Scene::new(vec![Occluder { shape: Shape::Box { center: [0.0, -0.5, 0.0], half: [500.0, 0.5, 500.0] }, absorption: 0.1, transmission: 0.0 }]);
    let r = analyse_room(&field, [0.0, 1.7, 0.0], 256, 400);
    println!("{:<28} {:>9} {:>9.2} {:>8.2} {:>7.1}m", "open field (floor only)", "-", r.rt60, r.closedness, r.mean_free_path);

    // the cost, in a scene of many boxes
    for n in [100usize, 1_000, 10_000] {
        let mut occ = room(40.0, 10.0, 40.0, 0.1);
        let side = (n as f32).sqrt().ceil() as usize;
        for i in 0..n {
            let (x, z) = ((i % side) as f32 / side as f32 * 36.0 - 18.0, (i / side) as f32 / side as f32 * 36.0 - 18.0);
            occ.push(Occluder { shape: Shape::Box { center: [x, -3.0 + (i % 5) as f32 * 0.5, z], half: [0.3, 0.8, 0.3] }, absorption: 0.2, transmission: 0.1 });
        }
        let t0 = Instant::now();
        let scene = Scene::new(occ);
        let build = t0.elapsed();
        let t1 = Instant::now();
        let room = analyse_room(&scene, [0.0, 3.0, 0.0], 256, 400);
        let analyse = t1.elapsed();
        let t2 = Instant::now();
        let trace = trace_source(&scene, [0.0, 3.0, 0.0], [10.0, 3.0, 5.0], 96, 0.3);
        let source = t2.elapsed();
        println!("{n:>6} boxes: build {build:?}, room analysis (256 rays, up to 400 bounces) {analyse:?} -> rt60 {:.2} s, one source (96 rays) {source:?} -> {} echoes", room.rt60, trace.reflections.len());
    }
}
