//! Thumbnails for the Project panel: the tile of a picture shows the picture (shrunk to at most 64 pixels on its long side), the tile of a model a small picture of the model
//! (`lunaris_render::thumb`) and the tile of a sound its waveform, instead of a generic icon. A file is decoded on a worker thread, a few at a time, so the panel never waits for a
//! big file; a tile shows its icon until its thumbnail is ready.
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use lunaris_audio::Clip;
use lunaris_render::texture::{Texels, Texture};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

/// The longest side of a thumbnail, in pixels.
pub const THUMB_SIDE: usize = 64;
/// The side of a model's thumbnail (a model is drawn from the geometry, so it can be sharper than a shrunk picture).
pub const MODEL_THUMB_SIDE: usize = 96;
/// Pictures bigger than this are not decoded for a thumbnail (they keep their icon).
const MAX_FILE_BYTES: u64 = 24_000_000;
/// Models bigger than this are not read for a thumbnail.
const MAX_MODEL_BYTES: u64 = 64_000_000;
/// Sounds bigger than this are not decoded for a thumbnail.
const MAX_SOUND_BYTES: u64 = 32_000_000;
/// How many pictures are being decoded at once.
const MAX_IN_FLIGHT: usize = 3;

/// A picture shrunk for a tile: RGBA, 8 bits, straight alpha.
pub struct Small {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

fn to_display(v: f32) -> u8 {
    (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8
}

/// The texture as RGBA8 in display colour, averaged down (a box filter) so that its long side is at most `max_side`; a picture already that small is kept as it is.
pub fn shrink(t: &Texture, max_side: usize) -> Small {
    let (w, h) = (t.width as usize, t.height as usize);
    let step = w.max(h).div_ceil(max_side).max(1);
    let (tw, th) = (w.div_ceil(step), h.div_ceil(step));
    let px = |x: usize, y: usize| -> [f32; 4] {
        match &t.texels {
            Texels::Srgb8(v) => {
                let p = v[y * w + x];
                [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32]
            }
            Texels::F32(v) => {
                let p = v[y * w + x];
                [to_display(p[0]) as f32, to_display(p[1]) as f32, to_display(p[2]) as f32, p[3].clamp(0.0, 1.0) * 255.0]
            }
        }
    };
    let mut rgba = Vec::with_capacity(tw * th * 4);
    for ty in 0..th {
        for tx in 0..tw {
            let (mut sum, mut n) = ([0.0f32; 4], 0.0f32);
            for y in ty * step..((ty + 1) * step).min(h) {
                for x in tx * step..((tx + 1) * step).min(w) {
                    let p = px(x, y);
                    // colour counts in proportion to its alpha, so a transparent pixel's hidden colour does not bleed in
                    let a = p[3] / 255.0;
                    sum[0] += p[0] * a;
                    sum[1] += p[1] * a;
                    sum[2] += p[2] * a;
                    sum[3] += p[3];
                    n += 1.0;
                }
            }
            let alpha = sum[3] / n.max(1.0);
            let weight = (sum[3] / 255.0).max(1e-6);
            rgba.extend_from_slice(&[(sum[0] / weight) as u8, (sum[1] / weight) as u8, (sum[2] / weight) as u8, (alpha + 0.5) as u8]);
        }
    }
    Small { w: tw, h: th, rgba }
}

/// A picture of a sound: its loudness along the clip, one column of pixels for each stretch of it, mirrored about the middle line (the peak in a pale colour, the
/// average loudness in a stronger one) on a clear background. The clip is shown as loud as it gets (scaled so that its loudest moment reaches the edge, at most 20 times
/// up), so that a quiet sound's shape can be seen too; silence is a thin line. `None` for a clip with nothing in it.
pub fn waveform(clip: &Clip, side: usize) -> Option<Small> {
    let frames = clip.frames();
    if frames == 0 || side < 4 {
        return None;
    }
    let ch = clip.channels.max(1) as usize;
    let loudest = clip.data.iter().fold(0.0f32, |m, s| if s.is_finite() { m.max(s.abs()) } else { m });
    let gain = if loudest > 0.0 { (1.0 / loudest).min(20.0) } else { 1.0 };
    let mid = (side as f32 - 1.0) / 2.0;
    let (pale, strong) = ([70u8, 130, 140, 255], [120u8, 220, 230, 255]);
    let mut rgba = vec![0u8; side * side * 4];
    for x in 0..side {
        let a = x * frames / side;
        let b = ((x + 1) * frames / side).clamp(a + 1, frames);
        let (mut peak, mut sum, mut n) = (0.0f32, 0.0f32, 0usize);
        for s in &clip.data[a * ch..b * ch] {
            if s.is_finite() {
                peak = peak.max(s.abs());
                sum += s * s;
            }
            n += 1;
        }
        let rms = (sum / n.max(1) as f32).sqrt();
        let (reach_peak, reach_rms) = ((peak * gain).min(1.0) * mid, (rms * gain).min(1.0) * mid);
        for y in 0..side {
            let d = (y as f32 - mid).abs();
            if d <= reach_peak.max(0.5) {
                let c = if d <= reach_rms { strong } else { pale };
                rgba[(y * side + x) * 4..][..4].copy_from_slice(&c);
            }
        }
    }
    Some(Small { w: side, h: side, rgba })
}

/// The thumbnail of the file at `file`: a picture shrunk, a model drawn or a sound's waveform. `None` for what the decoders do not take.
fn decode(file: &Path, kind: Kind) -> Option<Small> {
    let bytes = std::fs::read(file).ok()?;
    if kind == Kind::Sound {
        return waveform(&lunaris_audio::codec::decode_any(&bytes).ok()?, THUMB_SIDE);
    }
    if kind == Kind::Model {
        let imported = lunaris_render::gltf::import(&bytes, file.parent()).ok()?;
        let t = lunaris_render::thumb::thumbnail(&imported.mesh, MODEL_THUMB_SIDE)?;
        return Some(Small { w: t.w, h: t.h, rgba: t.rgba });
    }
    Texture::from_image_bytes(&bytes).ok().map(|t| shrink(&t, THUMB_SIDE))
}

/// What a file is, as far as the thumbnails care.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Picture,
    Model,
    Sound,
}

fn kind_of(path: &str) -> Kind {
    let l = path.to_ascii_lowercase();
    if lunaris_render::is_model_name(path) {
        Kind::Model
    } else if l.ends_with(".wav") || l.ends_with(".ogg") || l.ends_with(".mp3") {
        Kind::Sound
    } else {
        Kind::Picture
    }
}

enum Slot {
    Loading,
    Ready(TextureHandle),
    /// Not a picture the decoder takes, or too big: the tile keeps its icon.
    None,
}

pub struct Thumbs {
    root: PathBuf,
    tiles: HashMap<String, Slot>,
    tx: Sender<(String, Option<Small>)>,
    rx: Receiver<(String, Option<Small>)>,
    in_flight: usize,
}

impl Thumbs {
    pub fn new(root: PathBuf) -> Thumbs {
        let (tx, rx) = channel();
        Thumbs { root, tiles: HashMap::new(), tx, rx, in_flight: 0 }
    }

    /// How many thumbnails are ready (for tests).
    pub fn ready(&self) -> usize {
        self.tiles.values().filter(|s| matches!(s, Slot::Ready(_))).count()
    }

    /// The thumbnail of the picture or model at `path` (relative to the workspace; `bytes` is its size, so that a file that changed is read again), when it is ready. The first call
    /// for a file starts decoding it.
    pub fn get(&mut self, ctx: &egui::Context, path: &str, bytes: u64) -> Option<&TextureHandle> {
        while let Ok((key, small)) = self.rx.try_recv() {
            self.in_flight = self.in_flight.saturating_sub(1);
            let slot = match small {
                Some(s) => Slot::Ready(ctx.load_texture(format!("thumb:{key}"), ColorImage::from_rgba_unmultiplied([s.w, s.h], &s.rgba), TextureOptions::LINEAR)),
                None => Slot::None,
            };
            self.tiles.insert(key, slot);
            ctx.request_repaint();
        }
        let key = format!("{path}#{bytes}");
        if !self.tiles.contains_key(&key) {
            let kind = kind_of(path);
            if bytes > match kind { Kind::Model => MAX_MODEL_BYTES, Kind::Sound => MAX_SOUND_BYTES, Kind::Picture => MAX_FILE_BYTES } {
                self.tiles.insert(key.clone(), Slot::None);
            } else if self.in_flight < MAX_IN_FLIGHT {
                self.tiles.insert(key.clone(), Slot::Loading);
                self.in_flight += 1;
                let (tx, file, name) = (self.tx.clone(), self.root.join(path), key.clone());
                std::thread::spawn(move || {
                    // a file the decoder chokes on must not leave its slot loading for ever
                    let small = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode(&file, kind))).ok().flatten();
                    let _ = tx.send((name, small));
                });
            }
        }
        match self.tiles.get(&key) {
            Some(Slot::Ready(t)) => Some(t),
            Some(Slot::Loading) => {
                // look again soon: the worker will have finished
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(s: &Small, x: usize) -> usize {
        (0..s.h).filter(|&y| s.rgba[(y * s.w + x) * 4 + 3] != 0).count()
    }

    #[test]
    fn a_sound_is_drawn_as_its_loudness_along_the_clip_and_a_quiet_one_is_made_visible() {
        // half a second of silence, then half a second of a loud tone, then a 6 dB quieter tone: three stretches of different height
        let rate = 8000u32;
        let mut data = vec![0.0f32; 4000];
        data.extend((0..4000).map(|i| (i as f32 * 0.3).sin() * 0.8));
        data.extend((0..4000).map(|i| (i as f32 * 0.3).sin() * 0.4));
        let w = waveform(&Clip::mono(rate, data), 64).unwrap();
        assert_eq!((w.w, w.h), (64, 64));
        let (silent, loud, soft) = (column(&w, 5), column(&w, 28), column(&w, 55));
        assert!(silent <= 2, "silence is a thin line: {silent}");
        assert!(loud > soft && soft > silent, "{silent} < {soft} < {loud}");
        assert!(loud >= 60, "the loudest stretch reaches the edges: {loud}");
        assert!((soft as f32 / loud as f32 - 0.5).abs() < 0.12, "half the amplitude is about half the height: {soft} of {loud}");
        // the average loudness is the stronger colour, in the middle of a column; a corner is clear
        assert_eq!(w.rgba[3], 0);
        assert!(w.rgba[(32 * 64 + 28) * 4 + 1] > w.rgba[(2 * 64 + 28) * 4 + 1], "the middle is the stronger colour");
        // a very quiet clip is scaled up (at most twenty times), a clip with nothing in it gives nothing, stereo counts both channels
        let quiet = waveform(&Clip::mono(rate, (0..4000).map(|i| (i as f32 * 0.3).sin() * 0.05).collect()), 64).unwrap();
        assert!(column(&quiet, 30) >= 60, "{}", column(&quiet, 30));
        let very_quiet = waveform(&Clip::mono(rate, (0..4000).map(|i| (i as f32 * 0.3).sin() * 0.01).collect()), 64).unwrap();
        assert!((8..=20).contains(&column(&very_quiet, 30)), "twenty times and no more: {}", column(&very_quiet, 30));
        assert!(waveform(&Clip::mono(rate, Vec::new()), 64).is_none());
        let stereo = Clip { rate, channels: 2, data: (0..8000).map(|i| if i % 2 == 0 { 0.0 } else { 0.9 }).collect() };
        assert!(column(&waveform(&stereo, 64).unwrap(), 10) >= 60, "the right channel is loud");
    }

    #[test]
    fn files_are_told_apart_by_their_names() {
        assert!(kind_of("a/b/tree.GLB") == Kind::Model && kind_of("x.gltf") == Kind::Model);
        assert!(kind_of("beep.wav") == Kind::Sound && kind_of("m/Song.OGG") == Kind::Sound && kind_of("x.mp3") == Kind::Sound);
        assert!(kind_of("flag.png") == Kind::Picture && kind_of("noname") == Kind::Picture);
    }

    fn tex(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Texture {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.push(f(x, y));
            }
        }
        Texture { width: w, height: h, texels: Texels::Srgb8(v), heights: None }
    }

    #[test]
    fn a_big_picture_is_shrunk_to_the_long_side_and_its_colours_are_averaged() {
        // 256 x 128, left half red, right half blue: 64 x 32, red on the left, blue on the right
        let t = tex(256, 128, |x, _| if x < 128 { [200, 0, 0, 255] } else { [0, 0, 200, 255] });
        let s = shrink(&t, 64);
        assert_eq!((s.w, s.h), (64, 32));
        assert_eq!(&s.rgba[0..4], &[200, 0, 0, 255]);
        assert_eq!(&s.rgba[(63 * 4)..(63 * 4 + 4)], &[0, 0, 200, 255]);
        // a checker of black and white 2 pixels wide, shrunk by 4, is grey
        let c = tex(128, 128, |x, y| if (x / 2 + y / 2) % 2 == 0 { [0, 0, 0, 255] } else { [255, 255, 255, 255] });
        let g = shrink(&c, 32);
        assert!(g.rgba.chunks(4).all(|p| (p[0] as i32 - 127).abs() <= 2 && p[3] == 255), "{:?}", &g.rgba[..8]);
    }

    #[test]
    fn a_small_picture_is_kept_and_hidden_colour_under_transparency_does_not_bleed() {
        let t = tex(10, 6, |x, _| [x as u8 * 10, 0, 0, 255]);
        let s = shrink(&t, 64);
        assert_eq!((s.w, s.h), (10, 6));
        assert_eq!(&s.rgba[4 * 3..4 * 3 + 4], &[30, 0, 0, 255]);
        // half of the 2 x 1 block is opaque red, half is fully transparent green: the result is red at half alpha, not brown
        let b = tex(2, 1, |x, _| if x == 0 { [255, 0, 0, 255] } else { [0, 255, 0, 0] });
        let s = shrink(&b, 1);
        assert_eq!((s.w, s.h), (1, 1));
        assert_eq!(s.rgba[0], 255);
        assert_eq!(s.rgba[1], 0);
        assert!((s.rgba[3] as i32 - 128).abs() <= 1);
    }
}
