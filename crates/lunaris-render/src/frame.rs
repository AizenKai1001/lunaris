//! A rendered image plus everything an agent needs to reason about it: which entity is at each pixel, how deep, a hash,
//! and the camera matrices to map world points to pixels.
use crate::math::{M4, V3};
use lunaris_ecs::StateHasher;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Visible {
    pub uid: u64,
    pub pixels: u32,
    /// Inclusive pixel rectangle `[min_x, min_y, max_x, max_y]`.
    pub bbox: [u32; 4],
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Display colour, 8 bits per channel, row-major from the top-left.
    pub rgba: Vec<u8>,
    /// Entity uid drawn at each pixel (0 = background).
    pub ids: Vec<u64>,
    /// Normalised device depth in [-1, 1] (infinity where nothing was drawn).
    pub depth: Vec<f32>,
    pub view_proj: M4,
    /// Which camera was used, in words.
    pub camera: String,
    pub warnings: Vec<String>,
    /// What the renderer measured while drawing this picture (the GPU renderer's eye adaptation: `exposure`, `average_luminance`), by name. Never written into the simulation.
    pub info: Vec<(String, f64)>,
}

impl Frame {
    /// Hash of the image. Rendering is deterministic, so equal hashes mean identical pictures.
    pub fn hash(&self) -> u64 {
        let mut h = StateHasher::default();
        h.u64(self.width as u64);
        h.u64(self.height as u64);
        for chunk in self.rgba.chunks(8) {
            let mut w = [0u8; 8];
            w[..chunk.len()].copy_from_slice(chunk);
            h.u64(u64::from_le_bytes(w));
        }
        h.finish()
    }

    pub fn png(&self) -> Result<Vec<u8>, String> {
        encode_png(self.width, self.height, &self.rgba)
    }

    /// Entities that appear in the image, largest first.
    pub fn visible(&self, limit: usize) -> Vec<Visible> {
        let mut map: BTreeMap<u64, (u32, u32, u32, u32, u32)> = BTreeMap::new();
        for y in 0..self.height {
            for x in 0..self.width {
                let id = self.ids[(y * self.width + x) as usize];
                if id == 0 {
                    continue;
                }
                let e = map.entry(id).or_insert((0, x, y, x, y));
                e.0 += 1;
                e.1 = e.1.min(x);
                e.2 = e.2.min(y);
                e.3 = e.3.max(x);
                e.4 = e.4.max(y);
            }
        }
        let mut v: Vec<Visible> = map.into_iter().map(|(uid, (pixels, a, b, c, d))| Visible { uid, pixels, bbox: [a, b, c, d] }).collect();
        v.sort_by(|a, b| b.pixels.cmp(&a.pixels).then(a.uid.cmp(&b.uid)));
        v.truncate(limit);
        v
    }

    pub fn pick(&self, x: u32, y: u32) -> Option<u64> {
        if x >= self.width || y >= self.height {
            return None;
        }
        match self.ids[(y * self.width + x) as usize] {
            0 => None,
            id => Some(id),
        }
    }

    /// Fraction of pixels showing an entity (not the background).
    pub fn coverage(&self) -> f32 {
        self.ids.iter().filter(|&&i| i != 0).count() as f32 / self.ids.len().max(1) as f32
    }

    /// Where a world point lands in the image (pixel coordinates, y down), or `None` when it is behind the camera.
    pub fn world_to_pixel(&self, p: V3) -> Option<[f32; 2]> {
        let c = self.view_proj.mul_v4([p[0], p[1], p[2], 1.0]);
        if c[3] <= 0.0 {
            return None;
        }
        let (nx, ny) = (c[0] / c[3], c[1] / c[3]);
        Some([(nx * 0.5 + 0.5) * self.width as f32, (1.0 - (ny * 0.5 + 0.5)) * self.height as f32])
    }
}

/// Encode an RGBA image (row 0 at the top) as a PNG.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}
