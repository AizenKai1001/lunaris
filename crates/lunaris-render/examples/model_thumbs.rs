//! Draw the Project panel's thumbnail of each model given on the command line and put them side by side in one picture (a contact sheet), so the thumbnails can be looked at:
//!
//!     cargo run --release -p lunaris-render --example model_thumbs -- sheet.png examples/models/*.glb examples/caldera/models/*.glb
//!
//! Each tile is 96 x 96 on the editor's panel colour, shown at twice the size; the time to import and draw each model is printed.
use lunaris_render::thumb::thumbnail;
use std::time::Instant;

const SIDE: usize = 96;
const ZOOM: usize = 2;
const GAP: usize = 6;
const COLUMNS: usize = 8;
/// The colour of an empty tile in the editor.
const FIELD: [u8; 3] = [37, 39, 44];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: model_thumbs out.png model.glb [model.glb ...]");
        std::process::exit(2);
    }
    let (out, models) = (&args[0], &args[1..]);
    let rows = models.len().div_ceil(COLUMNS);
    let cell = SIDE * ZOOM + GAP;
    let (w, h) = (COLUMNS.min(models.len()) * cell + GAP, rows * cell + GAP);
    let mut sheet: Vec<u8> = (0..w * h).flat_map(|_| [20, 21, 24, 255]).collect();
    for (n, path) in models.iter().enumerate() {
        let started = Instant::now();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                println!("{path}: cannot read: {e}");
                continue;
            }
        };
        let imported = match lunaris_render::gltf::import(&bytes, std::path::Path::new(path).parent()) {
            Ok(i) => i,
            Err(e) => {
                println!("{path}: cannot import: {e}");
                continue;
            }
        };
        let imported_in = started.elapsed();
        let drawn = Instant::now();
        let Some(t) = thumbnail(&imported.mesh, SIDE) else {
            println!("{path}: nothing to draw");
            continue;
        };
        println!("{path}: {} triangles, import {:.1} ms, thumbnail {:.2} ms", imported.mesh.idx.len() / 3, imported_in.as_secs_f64() * 1e3, drawn.elapsed().as_secs_f64() * 1e3);
        let (x0, y0) = (GAP + (n % COLUMNS) * cell, GAP + (n / COLUMNS) * cell);
        for y in 0..SIDE * ZOOM {
            for x in 0..SIDE * ZOOM {
                let p = &t.rgba[((y / ZOOM) * t.w + x / ZOOM) * 4..][..4];
                let a = p[3] as u32;
                let o = ((y0 + y) * w + x0 + x) * 4;
                for k in 0..3 {
                    sheet[o + k] = ((p[k] as u32 * a + FIELD[k] as u32 * (255 - a)) / 255) as u8;
                }
            }
        }
    }
    std::fs::write(out, lunaris_render::encode_png(w as u32, h as u32, &sheet).expect("png")).expect("write");
    println!("wrote {out} ({w} x {h})");
}
