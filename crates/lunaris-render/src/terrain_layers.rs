//! Painted terrain materials (`TerrainLayers`, docs/TERRAIN.md "Layers"): a **splat map** (an RGBA picture over the whole ground) says how much of each
//! of up to four materials is at each place, and the layers are textures that repeat across the ground. This file reads the splat map, parses the
//! text that names the layers, and blends them at a point: the reference renderer calls [`SplatLayers::colour`] for every pixel of the ground, the
//! GPU renderer (`lunaris-gpu`) does the same sum in its shader, and the `terrain_layer` tool and `scatter` ask [`Splat::weights`] which layer is where.
//!
//! The splat map is registered like the heightmap: its texel `(i, j)` lies on the corner `(i, j)` of a grid that spans the ground, row 0 at the north
//! edge (-z) and column 0 at the west edge (-x), so a 513 x 513 map over a 513 x 513 heightmap has one weight at every vertex, and a map of another
//! size is stretched over the same ground. Between texels the weights are blended bilinearly, then normalised.
use crate::texture::{Texture, TextureCache};
use std::path::Path;
use std::sync::Arc;

/// The most layers: the red, green, blue and alpha channels of the splat map.
pub const MAX_LAYERS: usize = 4;

/// The most pixels a splat map may have (a 8192 x 8192 picture).
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// A splat map: four weights (0 to 255) per texel, row by row from the north edge.
#[derive(Debug)]
pub struct Splat {
    pub cols: usize,
    pub rows: usize,
    /// How many channels the picture had: 1 (grey), 2 (grey and alpha), 3 (RGB) or 4 (RGBA). Weight `k` is the picture's `k`-th channel, and 0 when it has none.
    pub channels: usize,
    pub texels: Vec<[u8; 4]>,
}

/// A number held to 0 to 1 (and 0 for NaN).
fn unit(v: f32) -> f32 {
    if v >= 1.0 {
        1.0
    } else if v >= 0.0 {
        v
    } else {
        0.0
    }
}

impl Splat {
    pub fn new(cols: usize, rows: usize, channels: usize, texels: Vec<[u8; 4]>) -> Result<Splat, String> {
        if cols == 0 || rows == 0 || texels.len() != cols * rows {
            return Err("the splat map has no texels or the wrong number of them".to_string());
        }
        Ok(Splat { cols, rows, channels: channels.clamp(1, 4), texels })
    }

    /// Decode a PNG held in memory: any size up to 64 million pixels, 8 or 16 bits (16 are cut to 8), grey, grey with alpha, RGB, RGBA or a palette.
    pub fn from_png(bytes: &[u8]) -> Result<Splat, String> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let (w, h) = (reader.info().width as u64, reader.info().height as u64);
        if w == 0 || h == 0 || w * h > MAX_PIXELS {
            return Err(format!("{w} x {h} pixels is more than a splat map may have ({MAX_PIXELS})"));
        }
        let mut buf = vec![0u8; reader.output_buffer_size().ok_or("the picture is too large")?];
        let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        let channels = match info.color_type {
            png::ColorType::Rgba => 4,
            png::ColorType::Rgb => 3,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Grayscale => 1,
            other => return Err(format!("unsupported colour type {other:?}")),
        };
        let n = (w * h) as usize;
        if buf.len() < n * channels {
            return Err("the picture data is shorter than the picture".to_string());
        }
        let texels = buf[..n * channels]
            .chunks_exact(channels)
            .map(|px| match channels {
                4 => [px[0], px[1], px[2], px[3]],
                3 => [px[0], px[1], px[2], 0],
                2 => [px[0], px[1], 0, 0],
                _ => [px[0], 0, 0, 0],
            })
            .collect();
        Splat::new(w as usize, h as usize, channels, texels)
    }

    /// Read a splat map of the workspace (a relative path without `..`).
    pub fn read(root: &Path, name: &str) -> Result<Splat, String> {
        let rel = Path::new(name);
        if name.is_empty() || rel.is_absolute() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(format!("`{name}` is not a path inside the workspace"));
        }
        let path = root.join(rel);
        let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Splat::from_png(&bytes).map_err(|e| format!("{name}: {e}"))
    }

    /// The four channels at (`u`, `v`), each 0 to 1: `u` runs from the west edge of the ground to the east edge and `v` from the north edge to the south edge (both 0 to 1,
    /// held to that). Texel `(i, j)` is at `u = i / (cols - 1)`; in between the texels are blended bilinearly. Only `+ - * /`: the same on every machine.
    pub fn raw(&self, u: f32, v: f32) -> [f32; 4] {
        let fx = unit(u) * (self.cols - 1) as f32;
        let fy = unit(v) * (self.rows - 1) as f32;
        let (x0f, y0f) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0f, fy - y0f);
        let (x0, y0) = (x0f as usize, y0f as usize);
        let (x1, y1) = ((x0 + 1).min(self.cols - 1), (y0 + 1).min(self.rows - 1));
        let at = |x: usize, y: usize| self.texels[y * self.cols + x];
        let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
        let mut out = [0.0f32; 4];
        for k in 0..4 {
            let (a, b, c, d) = (a[k] as f32 * (1.0 / 255.0), b[k] as f32 * (1.0 / 255.0), c[k] as f32 * (1.0 / 255.0), d[k] as f32 * (1.0 / 255.0));
            let top = a + (b - a) * tx;
            let bottom = c + (d - c) * tx;
            out[k] = top + (bottom - top) * ty;
        }
        out
    }

    /// How much of each of the first `layers` layers there is at (`u`, `v`): the channels, normalised to add up to 1. Where none of them has any weight the first layer is all there is.
    pub fn weights(&self, u: f32, v: f32, layers: usize) -> [f32; 4] {
        let r = self.raw(u, v);
        let n = layers.min(MAX_LAYERS);
        let mut w = [0.0f32; 4];
        w[..n].copy_from_slice(&r[..n]);
        let s = w[0] + w[1] + w[2] + w[3];
        if s > 1.0e-4 {
            [w[0] / s, w[1] / s, w[2] / s, w[3] / s]
        } else {
            [1.0, 0.0, 0.0, 0.0]
        }
    }
}

/// One line of the `TerrainLayers.layers` text.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerSpec {
    pub name: String,
    /// A built-in material or a picture of the workspace.
    pub texture: String,
    /// How many times the texture repeats across the ground (`None`: the terrain's own `uv_scale`).
    pub uv_scale: Option<[f32; 2]>,
    /// The depth in metres of the relief of a built-in material (the GPU renderer's high qualities).
    pub bump: f32,
    /// A display colour the texture is multiplied by.
    pub tint: [f32; 3],
    /// The layer that steep ground (a cliff) shows, laid on from the sides (the GPU renderer's high qualities).
    pub cliff: bool,
}

/// The syntax in one line, for messages.
pub const LAYER_SYNTAX: &str = "NAME [TEXTURE] [uv_scale=N] [bump=D] [tint=R,G,B] [cliff=1]";

fn numbers(text: &str, want: &[usize], what: &str, example: &str, line: usize) -> Result<Vec<f32>, String> {
    let v: Vec<f32> = text.split(',').map(|p| p.parse::<f32>().map_err(|_| format!("line {line}: `{what}` needs numbers, `{text}` is not"))).collect::<Result<_, _>>()?;
    if !want.contains(&v.len()) || v.iter().any(|x| !x.is_finite()) {
        let count = want.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(" or ");
        return Err(format!("line {line}: `{what}` takes {count} number{} (finite ones), as in `{what}={example}`", if want == [1] { "" } else { "s" }));
    }
    Ok(v)
}

/// Read the layer text: one layer a line, `NAME [TEXTURE] [uv_scale=N] [bump=D] [tint=R,G,B]`; blank lines and lines that start with `#` are skipped. At most
/// four layers. The error says which line and what is wrong with it.
pub fn parse_layers(text: &str) -> Result<Vec<LayerSpec>, String> {
    let mut out: Vec<LayerSpec> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let t = raw.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let mut words = t.split_whitespace();
        let name = words.next().unwrap_or("");
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') || name.contains('=') {
            return Err(format!("line {line}: `{name}` is not a layer name (letters, digits, `_` and `-`); a line is `{LAYER_SYNTAX}`"));
        }
        let mut spec = LayerSpec { name: name.to_string(), texture: name.to_string(), uv_scale: None, bump: 0.03, tint: [1.0; 3], cliff: false };
        let mut named_texture = false;
        for w in words {
            match w.split_once('=') {
                Some(("uv_scale", v)) => {
                    let n = numbers(v, &[1, 2], "uv_scale", "48", line)?;
                    let s = [n[0], *n.get(1).unwrap_or(&n[0])];
                    if !s.iter().all(|x| *x > 0.0 && *x <= 100_000.0) {
                        return Err(format!("line {line}: `uv_scale` must be above 0 (how many times the texture repeats across the ground), not `{v}`"));
                    }
                    spec.uv_scale = Some(s);
                }
                Some(("bump", v)) => {
                    let n = numbers(v, &[1], "bump", "0.03", line)?;
                    if !(0.0..=1.0).contains(&n[0]) {
                        return Err(format!("line {line}: `bump` is a depth in metres from 0 to 1, not `{v}`"));
                    }
                    spec.bump = n[0];
                }
                Some(("tint", v)) => {
                    let n = numbers(v, &[3], "tint", "1.1,1.1,1.2", line)?;
                    if !n.iter().all(|x| (0.0..=4.0).contains(x)) {
                        return Err(format!("line {line}: each number of `tint` is from 0 to 4, not `{v}`"));
                    }
                    spec.tint = [n[0], n[1], n[2]];
                }
                Some(("cliff", v)) => {
                    let n = numbers(v, &[1], "cliff", "1", line)?;
                    spec.cliff = n[0] != 0.0;
                }
                Some((key, _)) => return Err(format!("line {line}: `{key}` is not an option of a layer (they are `uv_scale`, `bump`, `tint` and `cliff`); a line is `{LAYER_SYNTAX}`")),
                None if !named_texture => {
                    spec.texture = w.to_string();
                    named_texture = true;
                }
                None => return Err(format!("line {line}: `{w}` is one word too many (the texture is the second word; options are `key=value`); a line is `{LAYER_SYNTAX}`")),
            }
        }
        if out.iter().any(|l| l.name == spec.name) {
            return Err(format!("line {line}: there is already a layer called `{}`", spec.name));
        }
        if out.len() == MAX_LAYERS {
            return Err(format!("line {line}: at most {MAX_LAYERS} layers (the red, green, blue and alpha channels of the splat map)"));
        }
        if spec.cliff && out.iter().any(|l| l.cliff) {
            return Err(format!("line {line}: only one layer can be the `cliff` layer"));
        }
        out.push(spec);
    }
    if out.is_empty() {
        return Err(format!("no layers: write one on each line, `{LAYER_SYNTAX}`, for example `grass grass uv_scale=64`"));
    }
    Ok(out)
}

/// A layer ready to draw.
pub struct SplatLayer {
    pub name: String,
    pub tex: Arc<Texture>,
    /// How many times the texture repeats across the ground.
    pub uv_scale: [f32; 2],
    /// Depth of the relief in metres (GPU high qualities).
    pub bump: f32,
    /// The colour the texture is multiplied by, in the renderer's gamma-2 working space.
    pub tint: [f32; 3],
    /// Steep ground shows this layer, laid on from the sides.
    pub cliff: bool,
}

/// What the renderers need to draw a painted terrain: the splat map and one to four layers.
pub struct SplatLayers {
    pub splat: Arc<Splat>,
    pub layers: Vec<SplatLayer>,
    /// The size of the ground in metres (x, z): what the layers' repeats are measured against.
    pub ground: [f32; 2],
    /// How much of the ground's colour is the terrain's own photograph where it has one (`TerrainLayers.photo`).
    pub photo: f32,
}

impl SplatLayers {
    /// The colour of the ground at (`u`, `v`) over the ground (0 to 1 each way, `v` from the north edge), in the working space: the layers' textures, each at its own repeat,
    /// weighted by the splat map. This is the reference renderer's per-pixel sum; the GPU renderer's shader does the same.
    pub fn colour(&self, u: f32, v: f32) -> [f32; 3] {
        let w = self.splat.weights(u, v, self.layers.len());
        let mut c = [0.0f32; 3];
        for (l, wk) in self.layers.iter().zip(w) {
            if wk <= 0.0 {
                continue;
            }
            // textures repeat; `Texture::sample` counts v from the bottom, as the mesh's uv do
            let t = l.tex.sample((u * l.uv_scale[0]).rem_euclid(1.0), (v * l.uv_scale[1]).rem_euclid(1.0));
            c = [c[0] + wk * l.tint[0] * t[0], c[1] + wk * l.tint[1] * t[1], c[2] + wk * l.tint[2] * t[2]];
        }
        c
    }

    pub fn names(&self) -> Vec<&str> {
        self.layers.iter().map(|l| l.name.as_str()).collect()
    }
}

/// Make the layers of one terrain from the two texts of its `TerrainLayers`. A problem (an empty or unreadable splat map, a mistake in the text) comes back as a
/// warning in `warnings` and `None`: the terrain is then drawn with its own texture, as without the component.
pub fn gather(textures: &mut TextureCache, uid: u64, terrain_uv_scale: [f32; 2], ground: [f32; 2], photo: f32, splat: &str, layers: &str, warnings: &mut Vec<String>) -> Option<Arc<SplatLayers>> {
    let specs = match parse_layers(layers) {
        Ok(s) => s,
        Err(e) => {
            warnings.push(format!("entity {uid}: TerrainLayers.layers: {e} (the terrain is drawn with its own texture)"));
            return None;
        }
    };
    if splat.is_empty() {
        warnings.push(format!("entity {uid}: TerrainLayers.splat is empty: name an RGBA picture of the workspace (the terrain is drawn with its own texture)"));
        return None;
    }
    let Some(map) = textures.splat(splat) else {
        warnings.push(format!("entity {uid}: the splat map `{splat}` could not be read (the terrain is drawn with its own texture)"));
        return None;
    };
    let default_scale = if terrain_uv_scale.iter().all(|v| v.is_finite() && *v > 0.0) { terrain_uv_scale } else { [64.0, 64.0] };
    let layers = specs
        .into_iter()
        .map(|s| SplatLayer {
            tex: textures.get(&s.texture),
            uv_scale: s.uv_scale.unwrap_or(default_scale),
            bump: s.bump,
            tint: [s.tint[0] * s.tint[0], s.tint[1] * s.tint[1], s.tint[2] * s.tint[2]],
            cliff: s.cliff,
            name: s.name,
        })
        .collect();
    Some(Arc::new(SplatLayers { splat: map, layers, ground, photo: if photo.is_finite() { photo.clamp(0.0, 1.0) } else { 0.85 } }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn splat_2x2(texels: [[u8; 4]; 4]) -> Splat {
        Splat::new(2, 2, 4, texels.to_vec()).unwrap()
    }

    #[test]
    fn one_layer_can_be_the_cliff_layer() {
        let specs = parse_layers("grass grass
rock stone cliff=1
sand sand cliff=0").unwrap();
        assert_eq!(specs.iter().map(|s| s.cliff).collect::<Vec<_>>(), [false, true, false]);
        let e = parse_layers("a grass cliff=1
b stone cliff=1").unwrap_err();
        assert!(e.contains("only one layer"), "{e}");
        assert!(parse_layers("a grass cliff=yes").is_err());
    }

    #[test]
    fn weights_blend_bilinearly_between_corner_registered_texels_and_add_up_to_one() {
        // the four corners: red at the north-west, green at the north-east, blue at the south-west, alpha at the south-east
        let s = splat_2x2([[255, 0, 0, 0], [0, 255, 0, 0], [0, 0, 255, 0], [0, 0, 0, 255]]);
        assert_eq!(s.weights(0.0, 0.0, 4), [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(s.weights(1.0, 0.0, 4), [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(s.weights(0.0, 1.0, 4), [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(s.weights(1.0, 1.0, 4), [0.0, 0.0, 0.0, 1.0]);
        let mid = s.weights(0.5, 0.5, 4);
        for w in mid {
            assert!((w - 0.25).abs() < 1e-6, "{mid:?}");
        }
        let edge = s.weights(0.5, 0.0, 4);
        assert!((edge[0] - 0.5).abs() < 1e-6 && (edge[1] - 0.5).abs() < 1e-6 && edge[2] == 0.0, "{edge:?}");
        // outside the ground and NaN are held to the edge
        assert_eq!(s.weights(-3.0, f32::NAN, 4), [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(s.weights(9.0, 9.0, 4), [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn layers_that_do_not_exist_get_no_weight_and_an_empty_texel_is_the_first_layer() {
        let s = splat_2x2([[0, 0, 0, 255]; 4]);
        // the alpha is the weight of a fourth layer: with only three layers it counts for nothing and the first one is all there is
        assert_eq!(s.weights(0.3, 0.3, 3), [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(s.weights(0.3, 0.3, 4), [0.0, 0.0, 0.0, 1.0]);
        // unnormalised weights are made to add up to 1
        let s = splat_2x2([[100, 100, 0, 0]; 4]);
        assert_eq!(s.weights(0.5, 0.5, 4), [0.5, 0.5, 0.0, 0.0]);
        let s = splat_2x2([[30, 10, 0, 0]; 4]);
        let w = s.weights(0.5, 0.5, 2);
        assert!((w[0] - 0.75).abs() < 1e-6 && (w[1] - 0.25).abs() < 1e-6);
        assert_eq!(s.weights(0.5, 0.5, 0), [1.0, 0.0, 0.0, 0.0]);
    }

    fn rgba_png(w: u32, h: u32, color: png::ColorType, bytes: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut data, w, h);
            enc.set_color(color);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(bytes).unwrap();
        }
        data
    }

    #[test]
    fn a_png_gives_its_channels_as_weights_and_missing_channels_are_zero() {
        let rgba = Splat::from_png(&rgba_png(2, 1, png::ColorType::Rgba, &[10, 20, 30, 40, 50, 60, 70, 80])).unwrap();
        assert_eq!((rgba.cols, rgba.rows, rgba.channels), (2, 1, 4));
        assert_eq!(rgba.texels, vec![[10, 20, 30, 40], [50, 60, 70, 80]]);
        let rgb = Splat::from_png(&rgba_png(1, 1, png::ColorType::Rgb, &[9, 8, 7])).unwrap();
        assert_eq!((rgb.channels, rgb.texels[0]), (3, [9, 8, 7, 0]));
        let grey = Splat::from_png(&rgba_png(1, 1, png::ColorType::Grayscale, &[200])).unwrap();
        assert_eq!((grey.channels, grey.texels[0]), (1, [200, 0, 0, 0]));
        // 16 bits are cut to the high byte
        let mut data = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut data, 1, 1);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Sixteen);
            enc.write_header().unwrap().write_image_data(&[0x80, 0x01, 0xFF, 0xFF, 0x00, 0x00, 0x12, 0x34]).unwrap();
        }
        assert_eq!(Splat::from_png(&data).unwrap().texels[0], [0x80, 0xFF, 0x00, 0x12]);
        assert!(Splat::from_png(b"not a png").is_err());
        assert!(Splat::read(std::path::Path::new("."), "../x.png").is_err());
        assert!(Splat::read(std::path::Path::new("."), "").is_err());
    }

    #[test]
    fn the_layer_text_is_read_line_by_line_and_mistakes_say_which_line() {
        let specs = parse_layers("# four materials\ngrass grass uv_scale=96\nrock dirt.png uv_scale=40,20 bump=0.1 tint=0.8,0.8,0.9\n\nsand\nsnow-cap concrete tint=1.5,1.5,1.5").unwrap();
        assert_eq!(specs.len(), 4);
        assert_eq!((specs[0].name.as_str(), specs[0].texture.as_str(), specs[0].uv_scale), ("grass", "grass", Some([96.0, 96.0])));
        assert_eq!((specs[1].texture.as_str(), specs[1].uv_scale, specs[1].bump, specs[1].tint), ("dirt.png", Some([40.0, 20.0]), 0.1, [0.8, 0.8, 0.9]));
        assert_eq!((specs[2].name.as_str(), specs[2].texture.as_str(), specs[2].uv_scale, specs[2].bump), ("sand", "sand", None, 0.03));
        assert_eq!(specs[3].name, "snow-cap");
        for (text, word) in [
            ("", "no layers"),
            ("# only a comment", "no layers"),
            ("grass\nrock\nsand\nsnow\nlava", "line 5: at most 4"),
            ("grass\ngrass", "line 2: there is already a layer called `grass`"),
            ("gr@ss", "not a layer name"),
            ("grass grass.png extra", "one word too many"),
            ("grass uv_scale=0", "above 0"),
            ("grass uv_scale=a", "needs numbers"),
            ("grass uv_scale=1,2,3", "takes 1 or 2 numbers"),
            ("grass bump=2", "from 0 to 1"),
            ("grass tint=1,2", "takes 3 numbers"),
            ("grass tint=1,2,9", "from 0 to 4"),
            ("grass shiny=1", "not an option"),
            ("grass\nrock bump=nan", "line 2"),
        ] {
            let e = parse_layers(text).unwrap_err();
            assert!(e.contains(word), "`{text}` gave `{e}`, wanted `{word}`");
        }
    }
}
