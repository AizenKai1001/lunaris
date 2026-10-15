//! Tile several PNG pictures into one contact sheet: `montage OUT.png COLUMNS TILE_WIDTH a.png b.png ...` (for looking at many scenes at once).

fn load(path: &str) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap_or_else(|e| panic!("{path}: {e}"))));
    let mut reader = decoder.read_info().expect("png header");
    let mut buf = vec![0; reader.output_buffer_size().expect("png size")];
    let info = reader.next_frame(&mut buf).expect("png data");
    let (w, h) = (info.width, info.height);
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        other => panic!("{path}: unsupported colour type {other:?}"),
    };
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for px in buf[..(w * h) as usize * channels].chunks(channels) {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    (w, h, rgba)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!("usage: montage OUT.png COLUMNS TILE_WIDTH a.png b.png ...");
        std::process::exit(2);
    }
    let cols: usize = args[1].parse().expect("columns");
    let tw: u32 = args[2].parse().expect("tile width");
    let images: Vec<(u32, u32, Vec<u8>)> = args[3..].iter().map(|p| load(p)).collect();
    let th = (tw as f32 * images[0].1 as f32 / images[0].0 as f32).round() as u32;
    let rows = images.len().div_ceil(cols);
    let (sw, sh) = (cols as u32 * tw, rows as u32 * th);
    let mut sheet = vec![20u8; (sw * sh * 4) as usize];
    for (i, (w, h, data)) in images.iter().enumerate() {
        let (ox, oy) = ((i % cols) as u32 * tw, (i / cols) as u32 * th);
        for y in 0..th {
            for x in 0..tw {
                // box filter: average the source pixels this tile pixel covers
                let (x0, x1) = (x * w / tw, ((x + 1) * w / tw).max(x * w / tw + 1).min(*w));
                let (y0, y1) = (y * h / th, ((y + 1) * h / th).max(y * h / th + 1).min(*h));
                let mut acc = [0u32; 3];
                let mut n = 0;
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let o = ((sy * w + sx) * 4) as usize;
                        for k in 0..3 {
                            acc[k] += data[o + k] as u32;
                        }
                        n += 1;
                    }
                }
                let o = (((oy + y) * sw + ox + x) * 4) as usize;
                for k in 0..3 {
                    sheet[o + k] = (acc[k] / n.max(1)) as u8;
                }
                sheet[o + 3] = 255;
            }
        }
    }
    let file = std::fs::File::create(&args[0]).expect("cannot create the sheet");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), sw, sh);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&sheet).unwrap();
}
