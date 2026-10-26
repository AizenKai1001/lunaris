//! Textures for sprites: a few procedural ones plus PNG files from the workspace. Texels are stored as linear RGBA (the
//! renderer works in a gamma-2.0 linear space, see `Frame`).
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// When a file-backed asset was last changed on disk (as it was loaded), and when that was last looked at again.
struct Stamp {
    modified: Option<SystemTime>,
    looked: Instant,
}

fn modified_time(root: &Option<PathBuf>, name: &str) -> Option<SystemTime> {
    std::fs::metadata(root.as_ref()?.join(name)).and_then(|m| m.modified()).ok()
}

/// Has the file behind `name` changed since it was loaded? Looks at the disk at most once per `interval` for each name.
fn changed(stamps: &mut BTreeMap<String, Stamp>, root: &Option<PathBuf>, interval: Duration, name: &str) -> bool {
    let Some(st) = stamps.get_mut(name) else { return false };
    if st.looked.elapsed() < interval {
        return false;
    }
    st.looked = Instant::now();
    let now = modified_time(root, name);
    if now == st.modified {
        return false;
    }
    st.modified = now;
    true
}

#[derive(Debug)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    /// RGBA, row-major, row 0 at the top (v = 1).
    pub texels: Texels,
    /// A height map (0..1, the same size and layout as `texels`) for textures that have a relief: the built-in materials. The GPU
    /// renderer's high-quality mode turns it into bumps and dents; the reference renderer ignores it.
    pub heights: Option<Vec<f32>>,
}

#[inline]
fn lin(c: f32) -> f32 {
    c * c
}

/// The pixels of a texture. A picture file is held as bytes (4 a texel: a 1024 x 1024 patch of a photographed city is 4 MB, not 16), a made picture (the built-in materials, a font page, a camera view) as floats.
#[derive(Debug, Clone, PartialEq)]
pub enum Texels {
    /// Linear RGBA.
    F32(Vec<[f32; 4]>),
    /// What a PNG or JPEG holds: display-colour red, green and blue (the working space is gamma 2, `lin`), and alpha (linear).
    Srgb8(Vec<[u8; 4]>),
}

/// The biggest picture a file may hold: 16384 a side (1 GB as bytes). A card that cannot take it gets it halved when it is uploaded.
const MAX_SIDE: u32 = 16384;

fn size_ok(w: u32, h: u32) -> bool {
    w > 0 && h > 0 && w <= MAX_SIDE && h <= MAX_SIDE
}

/// `lin(b / 255)` for every byte b: the very floats a texel was made of when pictures were decoded straight into floats.
fn linear_of_byte() -> &'static [f32; 256] {
    static TABLE: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0.0f32; 256];
        for (b, v) in t.iter_mut().enumerate() {
            *v = lin(b as f32 / 255.0);
        }
        t
    })
}

impl Texels {
    pub fn len(&self) -> usize {
        match self {
            Texels::F32(v) => v.len(),
            Texels::Srgb8(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Texel `i` as linear RGBA floats.
    #[inline]
    pub fn get(&self, i: usize) -> [f32; 4] {
        match self {
            Texels::F32(v) => v[i],
            Texels::Srgb8(v) => {
                let (p, t) = (v[i], linear_of_byte());
                [t[p[0] as usize], t[p[1] as usize], t[p[2] as usize], p[3] as f32 / 255.0]
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = [f32; 4]> + '_ {
        (0..self.len()).map(|i| self.get(i))
    }

    /// The texels as 8-bit display colour and alpha, 4 bytes each (what the card's texture is made of; for a picture that came from a file this is the file's own pixels).
    pub fn to_rgba8(&self) -> Vec<u8> {
        match self {
            Texels::Srgb8(v) => v.iter().flatten().copied().collect(),
            Texels::F32(v) => {
                let to8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                let mut bytes = Vec::with_capacity(v.len() * 4);
                for t in v {
                    bytes.extend_from_slice(&[to8(t[0].max(0.0).sqrt()), to8(t[1].max(0.0).sqrt()), to8(t[2].max(0.0).sqrt()), to8(t[3])]);
                }
                bytes
            }
        }
    }
}

impl Texture {
    /// A sample for the interface: `u` runs left to right and `v` top to bottom (0..1, clamped), nearest texel or bilinear (texel centres at half texels).
    pub fn sample_ui(&self, u: f32, v: f32, smooth: bool) -> [f32; 4] {
        let (w, h) = (self.width.max(1) as f32, self.height.max(1) as f32);
        let at = |x: f32, y: f32| self.texels.get((y as usize).min(self.height as usize - 1) * self.width as usize + (x as usize).min(self.width as usize - 1));
        if !smooth {
            return at((u * w).floor().clamp(0.0, w - 1.0), (v * h).floor().clamp(0.0, h - 1.0));
        }
        let (fx, fy) = ((u * w - 0.5).clamp(0.0, w - 1.0), (v * h - 0.5).clamp(0.0, h - 1.0));
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let (a, b, c, d) = (at(x0, y0), at(x0 + 1.0, y0), at(x0, y0 + 1.0), at(x0 + 1.0, y0 + 1.0));
        let mut out = [0.0; 4];
        for k in 0..4 {
            let top = a[k] + (b[k] - a[k]) * tx;
            let bottom = c[k] + (d[k] - c[k]) * tx;
            out[k] = top + (bottom - top) * ty;
        }
        out
    }

    /// Nearest-neighbour sample. `u` runs left to right, `v` bottom to top (both 0..1, clamped).
    #[inline]
    pub fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let x = ((u.clamp(0.0, 1.0) * self.width as f32) as u32).min(self.width - 1);
        let y = (((1.0 - v.clamp(0.0, 1.0)) * self.height as f32) as u32).min(self.height - 1);
        self.texels.get((y * self.width + x) as usize)
    }

    fn procedural(name: &str) -> Option<Texture> {
        if let Some(t) = crate::procedural::generate(name) {
            return Some(t);
        }
        let n = 64u32;
        let mut texels = Vec::with_capacity((n * n) as usize);
        for y in 0..n {
            for x in 0..n {
                let (fx, fy) = ((x as f32 + 0.5) / n as f32 - 0.5, (y as f32 + 0.5) / n as f32 - 0.5);
                let r2 = fx * fx + fy * fy;
                let px = match name {
                    "circle" => [1.0, 1.0, 1.0, if r2 <= 0.25 { 1.0 } else { 0.0 }],
                    // a square for particles that must look made of blocks: all of it opaque white
                    "square" => [1.0, 1.0, 1.0, 1.0],
                    // a soft dot for particles: opaque in the middle, fading smoothly to nothing at the edge
                    "soft" => {
                        let k = (1.0 - r2.sqrt() * 2.0).max(0.0);
                        [1.0, 1.0, 1.0, k * k * (3.0 - 2.0 * k)]
                    }
                    "ring" => [1.0, 1.0, 1.0, if (0.16..=0.25).contains(&r2) { 1.0 } else { 0.0 }],
                    "checker" => {
                        let on = ((x / 8) + (y / 8)) % 2 == 0;
                        let c = if on { 1.0 } else { 0.35 };
                        [lin(c), lin(c), lin(c), 1.0]
                    }
                    "gradient" => {
                        let t = x as f32 / (n - 1) as f32;
                        [lin(t), lin(t), lin(t), 1.0]
                    }
                    _ => return None,
                };
                texels.push(px);
            }
        }
        Some(Texture { width: n, height: n, texels: Texels::F32(texels), heights: None })
    }

    /// The picture a name that is still being read stands for (`TextureCache::get` in the asynchronous mode): the same one every time, so that a renderer can tell it by `is_pending` and leave the objects that
    /// have it out of the picture until the real one is here.
    pub fn pending() -> Arc<Texture> {
        static PENDING: std::sync::OnceLock<Arc<Texture>> = std::sync::OnceLock::new();
        PENDING.get_or_init(|| Arc::new(Texture { width: 1, height: 1, texels: Texels::F32(vec![[0.5, 0.5, 0.5, 1.0]]), heights: None })).clone()
    }

    /// Whether this is the picture of a name that is still being read (see `pending`).
    pub fn is_pending(t: &Arc<Texture>) -> bool {
        Arc::ptr_eq(t, &Texture::pending())
    }

    fn missing() -> Texture {
        let n = 16u32;
        let mut texels = Vec::new();
        for y in 0..n {
            for x in 0..n {
                texels.push(if (x / 4 + y / 4) % 2 == 0 { [1.0, 0.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0] });
            }
        }
        Texture { width: n, height: n, texels: Texels::F32(texels), heights: None }
    }

    pub(crate) fn from_png(path: &std::path::Path) -> Result<Texture, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        Texture::from_image_bytes(&bytes)
    }

    /// Decode a picture held in memory (for example embedded in a model): a PNG, or a JPEG (baseline or progressive, grey or colour), told apart by their first bytes.
    pub fn from_image_bytes(bytes: &[u8]) -> Result<Texture, String> {
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Texture::from_jpeg_bytes(bytes)
        } else {
            Texture::from_png_reader(std::io::Cursor::new(bytes))
        }
    }

    /// Decode a PNG held in memory.
    pub fn from_png_bytes(bytes: &[u8]) -> Result<Texture, String> {
        Texture::from_png_reader(std::io::Cursor::new(bytes))
    }

    /// Decode a JPEG held in memory (the colour is sRGB, like a PNG's).
    pub fn from_jpeg_bytes(bytes: &[u8]) -> Result<Texture, String> {
        use zune_jpeg::zune_core::bytestream::ZCursor;
        let mut decoder = zune_jpeg::JpegDecoder::new(ZCursor::new(bytes));
        let rgb = decoder.decode().map_err(|e| format!("JPEG: {e:?}"))?;
        let info = decoder.info().ok_or("JPEG: no picture size")?;
        let (w, h) = (info.width as u32, info.height as u32);
        if !size_ok(w, h) {
            return Err(format!("unsupported size {w}x{h} (at most {MAX_SIDE} a side)"));
        }
        let n = (w * h) as usize;
        let channels = rgb.len() / n.max(1);
        if channels < 3 || rgb.len() < n * channels {
            return Err(format!("JPEG: unexpected pixel data ({} bytes for {w}x{h})", rgb.len()));
        }
        let texels = rgb.chunks(channels).take(n).map(|px| [px[0], px[1], px[2], 255]).collect();
        Ok(Texture { width: w, height: h, texels: Texels::Srgb8(texels), heights: None })
    }

    fn from_png_reader<R: std::io::BufRead + std::io::Seek>(reader: R) -> Result<Texture, String> {
        let mut decoder = png::Decoder::new(reader);
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        // the size is known before the pixels are: refuse a picture that is too big before a buffer is made for it
        let (w, h) = (reader.info().width, reader.info().height);
        if !size_ok(w, h) {
            return Err(format!("unsupported size {w}x{h} (at most {MAX_SIDE} a side)"));
        }
        let mut buf = vec![0u8; reader.output_buffer_size().ok_or("image too large")?];
        let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        let channels = match info.color_type {
            png::ColorType::Rgba => 4,
            png::ColorType::Rgb => 3,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Grayscale => 1,
            other => return Err(format!("unsupported colour type {other:?}")),
        };
        let mut texels = Vec::with_capacity((w * h) as usize);
        for px in buf[..(w * h) as usize * channels].chunks(channels) {
            texels.push(match channels {
                4 => [px[0], px[1], px[2], px[3]],
                3 => [px[0], px[1], px[2], 255],
                2 => [px[0], px[0], px[0], px[1]],
                _ => [px[0], px[0], px[0], 255],
            });
        }
        Ok(Texture { width: w, height: h, texels: Texels::Srgb8(texels), heights: None })
    }
}

/// Whether a texture name is one of the built-in procedural pictures (`circle`, `ring`, `checker`, `gradient`) or material textures
/// (`grass`, `dirt`, `stone`, `bricks`, `wood`, `sand`, `concrete`, `tiles`, `metal`).
pub fn is_procedural_texture(name: &str) -> bool {
    matches!(name, "circle" | "ring" | "soft" | "square" | "checker" | "gradient") || crate::procedural::MATERIALS.contains(&name)
}

/// Note that `name` was asked for in picture `frame` (without making a new string when it is already there).
fn touch(used: &mut BTreeMap<String, u64>, name: &str, frame: u64) {
    match used.get_mut(name) {
        Some(f) => *f = frame,
        None => {
            used.insert(name.to_string(), frame);
        }
    }
}

/// An effect that was read (or the reason it could not be) and the number that tells it from another text.
struct FxEntry {
    effect: Result<Arc<lunaris_sim::fx::Effect>, String>,
    key: u64,
}

fn text_key(text: &str) -> u64 {
    // FNV-1a: only used to tell two texts apart
    text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

/// Loads textures by name: built-in procedural names, otherwise a PNG path relative to `root`.
pub struct TextureCache {
    root: Option<PathBuf>,
    map: BTreeMap<String, Arc<Texture>>,
    models: BTreeMap<String, Option<Arc<crate::mesh::Mesh>>>,
    heights: BTreeMap<String, HeightEntry>,
    /// The splat maps of painted terrains (`TerrainLayers`), read again when the file changes.
    splats: BTreeMap<String, SplatEntry>,
    splat_stamps: BTreeMap<String, Stamp>,
    languages: BTreeMap<String, Arc<BTreeMap<String, String>>>,
    water_grids: BTreeMap<usize, Arc<crate::mesh::Mesh>>,
    /// The meshes of terrains with the picture that last asked for each: a streamed world draws a dozen tiles at once, so these are let go by `trim` (and not by count, which would make them again every picture).
    terrain_meshes: Vec<(TerrainKey, Arc<crate::mesh::Mesh>, u64)>,
    /// The surfaces of water bodies (`None`: a picture with no water in it).
    water_meshes: Vec<(WaterKey, Option<Arc<crate::mesh::Mesh>>)>,
    calls: u64,
    /// The TrueType fonts of the workspace and the letters drawn from them.
    pub fonts: crate::fonts::FontCache,
    /// True while a `CameraView` is being drawn: that picture has no interface and no views.
    pub nested: bool,
    view_ticks: BTreeMap<u64, u64>,
    /// Textures and models that are files are read again when the file changes on disk (looked at no more often than this; default half a second),
    /// so a picture saved in an image editor shows up in a running game.
    pub reload_interval: Duration,
    texture_stamps: BTreeMap<String, Stamp>,
    model_stamps: BTreeMap<String, Stamp>,
    /// The effects (docs/FX.md) of `ParticleSystem`s, by file name or (for JSON written in the component) by a hash of the text, with what was wrong with one that could not be read; a file is read again when it changes.
    fx_effects: BTreeMap<String, FxEntry>,
    fx_stamps: BTreeMap<String, Stamp>,
    /// The particles of every `ParticleSystem` (`fx_sim`): stepped by the pictures, kept here between them.
    pub fx: crate::fx_sim::FxWorld,
    /// True while the GPU renderer is drawing (it sets it before it gathers a scene, the reference renderer clears it): the emitters that ask for the card (`simulate: gpu`) are then only scheduled here and stepped there.
    pub fx_on_card: bool,
    /// The picture the cache is drawing (`begin_frame` counts them) and, for the pictures, models, heightmaps and splat maps that are files, the last picture that asked for each:
    /// `trim` lets go of what nothing has asked for in a long while, so a world that streams its cells in and out does not keep every cell it ever showed.
    frame: u64,
    /// The pictures decoded so far (shared with the loader's threads: a picture is decoded once however many ask) and the loader that reads files ahead of need (made when the first request comes).
    decoded: crate::loader::Decoded,
    loader: Option<crate::loader::Loader>,
    /// How many threads the loader gets (0: no prefetching, everything is read when it is wanted; the environment variable LUNARIS_LOADER_THREADS sets it; default: up to 3).
    pub loader_threads: usize,
    /// How many pictures a file may go unasked for before the cache lets go of it (default 600: ten seconds at 60 a second; the environment variable LUNARIS_TRIM_IDLE sets it).
    pub trim_idle: u64,
    used: BTreeMap<String, u64>,
    model_used: BTreeMap<String, u64>,
    height_used: BTreeMap<String, u64>,
    splat_used: BTreeMap<String, u64>,
    /// The triangles of models for physics-side rays (a `MeshCollider`), read here when the simulation's own copy is not at hand.
    collision: BTreeMap<String, Option<Arc<lunaris_sim::meshcol::CollisionMesh>>>,
    collision_stamps: BTreeMap<String, Stamp>,
    /// Convex hulls made here, by model and scale.
    hulls: BTreeMap<(String, [u32; 3]), (u64, Option<Arc<lunaris_sim::hull::Hull>>)>,
    /// The simulation's own copies (`Sim::meshes`), used first so that a model is read once.
    shared_collision: Option<Arc<lunaris_sim::meshcol::MeshStore>>,
    /// The simulation's own ground (heightmaps) for what looks at the world's colliders from the picture (a particle that bounces off the world: `fx_sim`).
    shared_terrains: Option<Arc<lunaris_sim::terrain::Terrains>>,
    pub warnings: Vec<String>,
    /// The asynchronous mode of a game that is played (off by default: pictures taken by tools and tests wait for everything and are the same every time): what a picture asks for that is not here yet is read
    /// and built on other threads, and the answer is "not yet" (`None`, or the picture `Texture::pending`) until it is; the objects that need it are left out of the pictures meanwhile.
    pub async_jobs: bool,
    /// Terrain meshes and water surfaces being built on the pool's threads (asynchronous mode).
    terrain_jobs: Vec<(TerrainKey, crate::bg::Pending<Arc<crate::mesh::Mesh>>, u64)>,
    water_jobs: Vec<(WaterKey, crate::bg::Pending<Option<Arc<crate::mesh::Mesh>>>, u64)>,
}

/// A language is a plain name (`en`, `de-AT`) or a path of the workspace without its `.json` (`examples/depths/lang/en`): letters, digits, `-`, `_` and `/` between names, no dots, nothing absolute.
pub fn language_name_ok(language: &str) -> bool {
    !language.is_empty() && language.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/')) && !language.starts_with('/') && !language.ends_with('/') && !language.contains("//")
}

fn flatten_texts(j: &lunaris_json::Json, prefix: &str, out: &mut BTreeMap<String, String>) {
    match j {
        lunaris_json::Json::Str(s) if !prefix.is_empty() => {
            out.insert(prefix.to_string(), s.clone());
        }
        lunaris_json::Json::Obj(pairs) => {
            for (k, v) in pairs {
                flatten_texts(v, &if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") }, out);
            }
        }
        _ => {}
    }
}

/// A heightmap that was read (or could not be: it is tried again after a while).
struct HeightEntry {
    field: Option<Arc<lunaris_sim::terrain::Heightfield>>,
    failed_at: u64,
}

/// A splat map that was read (or could not be: it is tried again after a while).
struct SplatEntry {
    splat: Option<Arc<crate::terrain_layers::Splat>>,
    failed_at: u64,
}

/// What a terrain mesh depends on: the heightmap, the step, the size and the relief.
type TerrainKey = (String, u32, u32, u32, u32, u32);

/// What a water body's mesh depends on: the picture, the step, the size and the relief.
type WaterKey = (String, u32, u32, u32, u32);

impl TextureCache {
    /// The workspace folder that names are looked up in (`None` for a cache of built-in names only).
    pub fn root(&self) -> Option<&std::path::Path> {
        self.root.as_deref()
    }

    pub fn new(root: Option<PathBuf>) -> TextureCache {
        let fonts = crate::fonts::FontCache::new(root.clone());
        TextureCache { fonts, nested: false, view_ticks: BTreeMap::new(), reload_interval: Duration::from_millis(500), texture_stamps: BTreeMap::new(), model_stamps: BTreeMap::new(), collision: BTreeMap::new(), collision_stamps: BTreeMap::new(), hulls: BTreeMap::new(), shared_collision: None, shared_terrains: None, root, map: BTreeMap::new(), models: BTreeMap::new(), heights: BTreeMap::new(), splats: BTreeMap::new(), splat_stamps: BTreeMap::new(), languages: BTreeMap::new(), water_grids: BTreeMap::new(), terrain_meshes: Vec::new(), water_meshes: Vec::new(), calls: 0, frame: 0, decoded: crate::loader::Decoded::default(), loader: None, loader_threads: std::env::var("LUNARIS_LOADER_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).clamp(1, 3))), trim_idle: std::env::var("LUNARIS_TRIM_IDLE").ok().and_then(|v| v.parse().ok()).unwrap_or(600), used: BTreeMap::new(), model_used: BTreeMap::new(), height_used: BTreeMap::new(), splat_used: BTreeMap::new(), warnings: Vec::new(), async_jobs: false, terrain_jobs: Vec::new(), water_jobs: Vec::new(), fx_effects: BTreeMap::new(), fx_stamps: BTreeMap::new(), fx: Default::default(), fx_on_card: false }
    }

    /// A heightmap of the workspace (a greyscale PNG), read once. `None` (with a warning, and another try after a while) when it cannot be read.
    pub fn heightfield(&mut self, name: &str) -> Option<Arc<lunaris_sim::terrain::Heightfield>> {
        self.calls += 1;
        touch(&mut self.height_used, name, self.frame);
        let defer = self.can_defer();
        self.settle(if defer { None } else { Some((crate::loader::Kind::Heights, name)) });
        if defer && !self.heights.contains_key(name) {
            // being read on another thread: not yet
            self.prefetch(crate::loader::Kind::Heights, name);
            return None;
        }
        if let Some(e) = self.heights.get(name) {
            if e.field.is_some() || self.calls.saturating_sub(e.failed_at) < 120 {
                return e.field.clone();
            }
        }
        let _t = crate::Timed::new(|| format!("read heightmap {name}"));
        let loaded = match &self.root {
            Some(root) => lunaris_sim::terrain::Heightfield::read(root, name),
            None => Err("no workspace folder to read heightmaps from".to_string()),
        };
        let field = match loaded {
            Ok(h) => Some(Arc::new(h)),
            Err(e) => {
                let msg = format!("terrain {name}: {e}");
                if !self.warnings.contains(&msg) {
                    self.warnings.push(msg);
                }
                None
            }
        };
        self.heights.insert(name.to_string(), HeightEntry { field: field.clone(), failed_at: self.calls });
        field
    }

    /// The splat map of a painted terrain (`TerrainLayers.splat`, an RGBA PNG of the workspace), read once and again when the file changes. `None` (with a warning, and another
    /// try after a while) when it cannot be read; a map that was fine and cannot be read now (it is being written) keeps the old copy.
    pub fn splat(&mut self, name: &str) -> Option<Arc<crate::terrain_layers::Splat>> {
        self.calls += 1;
        touch(&mut self.splat_used, name, self.frame);
        let defer = self.can_defer();
        self.settle(if defer { None } else { Some((crate::loader::Kind::Splat, name)) });
        if defer && !self.splats.contains_key(name) {
            self.prefetch(crate::loader::Kind::Splat, name);
            return None;
        }
        if let Some(e) = self.splats.get(name) {
            let unchanged = !changed(&mut self.splat_stamps, &self.root, self.reload_interval, name);
            if unchanged && (e.splat.is_some() || self.calls.saturating_sub(e.failed_at) < 120) {
                return e.splat.clone();
            }
        }
        let before = self.splats.get(name).and_then(|e| e.splat.clone());
        let _t = crate::Timed::new(|| format!("read splat map {name}"));
        let loaded = match &self.root {
            Some(root) => crate::terrain_layers::Splat::read(root, name),
            None => Err("no workspace folder to read splat maps from".to_string()),
        };
        let splat = match loaded {
            Ok(m) => Some(Arc::new(m)),
            Err(e) => {
                let msg = if before.is_some() { format!("splat map {name}: it changed on disk but cannot be read now ({e}); the old map stays") } else { format!("splat map {name}: {e}") };
                if !self.warnings.contains(&msg) {
                    self.warnings.push(msg);
                }
                before
            }
        };
        self.splat_stamps.entry(name.to_string()).and_modify(|s| s.looked = Instant::now()).or_insert_with(|| Stamp { modified: modified_time(&self.root, name), looked: Instant::now() });
        self.splats.insert(name.to_string(), SplatEntry { splat: splat.clone(), failed_at: self.calls });
        splat
    }

    /// The mesh of a terrain (kept for the next picture; the few most recent sizes are remembered).
    pub fn terrain_mesh(&mut self, name: &str, g: &lunaris_sim::components::Terrain) -> Option<Arc<crate::mesh::Mesh>> {
        let key: TerrainKey = (name.to_string(), g.step, g.size[0].to_bits(), g.size[1].to_bits(), g.height.to_bits(), g.skirt.to_bits());
        if let Some((_, m, last)) = self.terrain_meshes.iter_mut().find(|(k, _, _)| *k == key) {
            *last = self.frame;
            return Some(m.clone());
        }
        let field = self.heightfield(name)?;
        if self.can_defer() {
            // built on another thread: the answer is "not yet" until it is done
            let at = self.terrain_jobs.iter().position(|(k, _, _)| *k == key);
            let at = at.unwrap_or_else(|| {
                let g = *g;
                self.terrain_jobs.push((key.clone(), crate::bg::Pool::shared().spawn(move || Arc::new(crate::terrain::build_mesh(&field, &g))), self.frame));
                self.terrain_jobs.len() - 1
            });
            self.terrain_jobs[at].2 = self.frame;
            let mesh = self.terrain_jobs[at].1.try_take()?;
            self.terrain_jobs.remove(at);
            self.terrain_meshes.push((key, mesh.clone(), self.frame));
            return Some(mesh);
        }
        let _t = crate::Timed::new(|| format!("terrain mesh {name} step {}", g.step));
        let mesh = Arc::new(crate::terrain::build_mesh(&field, g));
        self.terrain_meshes.push((key, mesh.clone(), self.frame));
        Some(mesh)
    }

    /// Whether what is not here yet is to be read and built on other threads (`async_jobs`, a workspace folder to read from and a loader).
    fn can_defer(&self) -> bool {
        self.async_jobs && self.root.is_some() && self.loader_threads > 0
    }

    /// The surface of a water body (kept for the next picture; the few most recent are remembered): the picture `name` read as the heights of the water, with the size, step and relief of
    /// `w`. `None` when the picture cannot be read (a warning says why) or has no water in it.
    pub fn water_mesh(&mut self, name: &str, w: &lunaris_sim::components::WaterBody) -> Option<Arc<crate::mesh::Mesh>> {
        let key: WaterKey = (name.to_string(), w.step, w.size[0].to_bits(), w.size[1].to_bits(), w.relief.to_bits());
        if let Some((_, m)) = self.water_meshes.iter().find(|(k, _)| *k == key) {
            return m.clone();
        }
        let field = self.heightfield(name)?;
        if self.can_defer() {
            let at = self.water_jobs.iter().position(|(k, _, _)| *k == key);
            let at = at.unwrap_or_else(|| {
                let w = w.clone();
                self.water_jobs.push((key.clone(), crate::bg::Pool::shared().spawn(move || crate::water_body::build_mesh(&field, &w).map(Arc::new)), self.frame));
                self.water_jobs.len() - 1
            });
            self.water_jobs[at].2 = self.frame;
            let mesh = self.water_jobs[at].1.try_take()?;
            self.water_jobs.remove(at);
            self.water_meshes.push((key, mesh.clone()));
            if self.water_meshes.len() > 6 {
                self.water_meshes.remove(0);
            }
            return mesh;
        }
        let mesh = crate::water_body::build_mesh(&field, w).map(Arc::new);
        self.water_meshes.push((key, mesh.clone()));
        if self.water_meshes.len() > 6 {
            self.water_meshes.remove(0);
        }
        mesh
    }

    /// The texts of a language (`lang/<language>.json` of the workspace), read once: `{"hello": "Hello", "menu": {"play": "Play"}}` gives the keys
    /// `hello` and `menu.play`. A language that cannot be read has no texts (the keys show) and a warning.
    pub fn translations(&mut self, language: &str) -> Arc<BTreeMap<String, String>> {
        if let Some(t) = self.languages.get(language) {
            return t.clone();
        }
        let mut table = BTreeMap::new();
        // a name is a file of the workspace's `lang` folder (`en` is `lang/en.json`); a name with slashes is the path of the file without its `.json`
        // (`examples/depths/lang/en`), so that a game can keep its texts beside its other files
        let loaded = match &self.root {
            Some(root) if language_name_ok(language) => {
                let rel = if language.contains('/') { format!("{language}.json") } else { format!("lang/{language}.json") };
                std::fs::read_to_string(root.join(rel)).map_err(|e| e.to_string()).and_then(|t| lunaris_json::parse(&t).map_err(|e| e.to_string()))
            }
            _ => Err("not a language name".to_string()),
        };
        match loaded {
            Ok(j) => flatten_texts(&j, "", &mut table),
            Err(e) => self.warnings.push(format!("language `{language}`: {e}")),
        }
        let t = Arc::new(table);
        self.languages.insert(language.to_string(), t.clone());
        t
    }

    /// A flat grid of `n` x `n` cells over the unit square (x and z from -0.5 to 0.5, y 0, facing up), for water: scaled to the sheet's size, and displaced by the GPU.
    pub fn water_grid(&mut self, n: usize) -> Arc<crate::mesh::Mesh> {
        let n = n.clamp(2, 400);
        self.water_grids
            .entry(n)
            .or_insert_with(|| {
                let (mut pos, mut nrm, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
                for j in 0..=n {
                    for i in 0..=n {
                        let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
                        pos.push([u - 0.5, 0.0, v - 0.5]);
                        nrm.push([0.0, 1.0, 0.0]);
                        uv.push([u, v]);
                    }
                }
                let stride = (n + 1) as u32;
                for j in 0..n as u32 {
                    for i in 0..n as u32 {
                        let a = j * stride + i;
                        idx.extend_from_slice(&[a, a + stride, a + 1, a + 1, a + stride, a + stride + 1]);
                    }
                }
                Arc::new(crate::mesh::Mesh { pos, nrm, uv, col: Vec::new(), texw: Vec::new(), idx, tex: None, rig: None, normal_tex: None, arm_tex: None, alpha_cutoff: 0.0, lod: crate::mesh::LodMode::Full, parts: Vec::new(), occluders: Vec::new(), depth_whole: None, emissive_factor: 0.0 })
            })
            .clone()
    }

    /// Read every language again when it is next needed (after the files changed).
    pub fn forget_languages(&mut self) {
        self.languages.clear();
    }

    /// Read every heightmap again when it is next needed (after the files changed).
    pub fn forget_heightmaps(&mut self) {
        self.heights.clear();
        self.water_meshes.clear();
        self.splats.clear();
        self.splat_stamps.clear();
        self.terrain_meshes.clear();
    }

    /// A picture is being drawn: what is asked for from here on counts as used now.
    /// The number of the picture being drawn (`begin_frame` counts them).
    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn begin_frame(&mut self) {
        self.frame += 1;
        self.settle(None);
    }

    /// Ask for a file to be loaded on another thread, because something will want it soon (a streamed piece of the world about to come into range): when it is wanted it is here, or, if it is still
    /// being read, waited for, so a request can only save time. Pictures, models, heightmaps and splat maps; the built-in names and anything already loaded are left alone.
    pub fn prefetch(&mut self, kind: crate::loader::Kind, name: &str) {
        use crate::loader::Kind;
        let Some(root) = self.root.clone() else { return };
        if self.loader_threads == 0 || name.is_empty() || is_procedural_texture(name) || name.starts_with("view:") {
            return;
        }
        let have = match kind {
            Kind::Texture => {
                touch(&mut self.used, name, self.frame);
                self.map.contains_key(name)
            }
            Kind::Model => {
                touch(&mut self.model_used, name, self.frame);
                self.models.contains_key(name)
            }
            Kind::Heights => {
                touch(&mut self.height_used, name, self.frame);
                self.heights.get(name).is_some_and(|e| e.field.is_some())
            }
            Kind::Splat => {
                touch(&mut self.splat_used, name, self.frame);
                self.splats.get(name).is_some_and(|e| e.splat.is_some())
            }
        };
        if have {
            return;
        }
        let (decoded, workers) = (self.decoded.clone(), self.loader_threads);
        self.loader.get_or_insert_with(|| crate::loader::Loader::new(root, decoded, workers)).request(kind, name);
    }

    /// Whether a file is already in memory (loaded here or by the loader and taken back) and not still on its way.
    pub fn is_resident(&self, kind: crate::loader::Kind, name: &str) -> bool {
        use crate::loader::Kind;
        match kind {
            Kind::Texture => self.map.contains_key(name),
            Kind::Model => self.models.contains_key(name),
            Kind::Heights => self.heights.get(name).is_some_and(|e| e.field.is_some()),
            Kind::Splat => self.splats.get(name).is_some_and(|e| e.splat.is_some()),
        }
    }

    /// The models that are in memory (read for a picture, or ahead of one by `prefetch`): the GPU renderer's asynchronous mode prepares their meshes and pictures for the card before anything draws them.
    pub fn loaded_models(&self) -> Vec<Arc<crate::mesh::Mesh>> {
        self.models.values().flatten().cloned().collect()
    }

    /// Whether a file has been asked for and is still being read on another thread.
    pub fn waiting_for(&self, kind: crate::loader::Kind, name: &str) -> bool {
        self.loader.as_ref().is_some_and(|l| l.is_pending(kind, name))
    }

    /// How many requests are still being read.
    pub fn loading(&self) -> usize {
        self.loader.as_ref().map_or(0, |l| l.pending())
    }

    /// Take what the loader has finished, and (with `wait`) first wait for that one request if it has not.
    fn settle(&mut self, wait: Option<(crate::loader::Kind, &str)>) {
        let Some(loader) = self.loader.as_mut() else { return };
        let done = match wait {
            Some((kind, name)) if loader.is_pending(kind, name) => loader.wait_for(kind, name),
            _ => loader.poll(),
        };
        for d in done {
            self.apply(d);
        }
    }

    /// Put a loaded file where the synchronous path would have put it.
    fn apply(&mut self, d: crate::loader::Done) {
        use crate::loader::{Kind, Payload};
        let name = d.name;
        match (d.kind, d.result) {
            (Kind::Texture, Ok(Payload::Texture(t))) => {
                self.texture_stamps.insert(name.clone(), Stamp { modified: modified_time(&self.root, &name), looked: Instant::now() });
                self.map.insert(name, t);
            }
            (Kind::Texture, Err(e)) => {
                self.decoded.forget(&name);
                self.warnings.push(format!("texture `{name}`: {e} (drawing the magenta placeholder)"));
                self.map.insert(name, Arc::new(Texture::missing()));
            }
            (Kind::Model, Ok(Payload::Model(i))) => {
                for w in i.warnings {
                    self.warnings.push(format!("model {name}: {w}"));
                }
                self.model_stamps.insert(name.clone(), Stamp { modified: modified_time(&self.root, &name), looked: Instant::now() });
                self.models.insert(name, Some(Arc::new(i.mesh)));
            }
            (Kind::Model, Err(e)) => {
                self.warnings.push(format!("model {name}: {e}"));
                self.models.insert(name, None);
            }
            (Kind::Heights, Ok(Payload::Heights(h))) => {
                self.heights.insert(name, HeightEntry { field: Some(Arc::new(h)), failed_at: self.calls });
            }
            (Kind::Heights, Err(e)) => {
                let msg = format!("terrain {name}: {e}");
                if !self.warnings.contains(&msg) {
                    self.warnings.push(msg);
                }
                self.heights.insert(name, HeightEntry { field: None, failed_at: self.calls });
            }
            (Kind::Splat, Ok(Payload::Splat(m))) => {
                self.splat_stamps.insert(name.clone(), Stamp { modified: modified_time(&self.root, &name), looked: Instant::now() });
                self.splats.insert(name, SplatEntry { splat: Some(Arc::new(m)), failed_at: self.calls });
            }
            (Kind::Splat, Err(e)) => {
                let msg = format!("splat map {name}: {e}");
                if !self.warnings.contains(&msg) {
                    self.warnings.push(msg);
                }
                self.splats.insert(name, SplatEntry { splat: None, failed_at: self.calls });
            }
            _ => {}
        }
    }

    /// Let go of the pictures, models, heightmaps and splat maps that no picture has asked for in the last `idle` pictures (a hundred megabytes a cell of a photographed city), and return how many.
    /// Something the scene still holds is only dropped from the cache: it lives as long as its holder does, and is read again from its file when it is asked for again.
    pub fn trim(&mut self, idle: u64) -> usize {
        let frame = self.frame;
        let stale = |used: &BTreeMap<String, u64>| -> Vec<String> { used.iter().filter(|(_, &f)| frame.saturating_sub(f) > idle).map(|(k, _)| k.clone()).collect() };
        let mut n = 0;
        for k in stale(&self.used) {
            // (the built-in pictures and the pictures of camera views are not files: they stay)
            if !is_procedural_texture(&k) && !k.starts_with("view:") && self.map.contains_key(&k) {
                self.decoded.forget(&k);
                self.map.remove(&k);
                self.texture_stamps.remove(&k);
                self.used.remove(&k);
                n += 1;
            }
        }
        for k in stale(&self.model_used) {
            if self.models.remove(&k).is_some() {
                self.model_stamps.remove(&k);
                n += 1;
            }
            self.model_used.remove(&k);
        }
        for k in stale(&self.height_used) {
            if self.heights.remove(&k).is_some() {
                n += 1;
            }
            self.height_used.remove(&k);
        }
        self.decoded.prune();
        self.terrain_jobs.retain(|(_, _, last)| frame.saturating_sub(*last) <= idle);
        self.water_jobs.retain(|(_, _, last)| frame.saturating_sub(*last) <= idle);
        let before = self.terrain_meshes.len();
        self.terrain_meshes.retain(|(_, _, last)| frame.saturating_sub(*last) <= idle);
        n += before - self.terrain_meshes.len();
        for k in stale(&self.splat_used) {
            if self.splats.remove(&k).is_some() {
                self.splat_stamps.remove(&k);
                n += 1;
            }
            self.splat_used.remove(&k);
        }
        n
    }

    /// An imported model (`.glb` / `.gltf` in the workspace), loaded once. `None` (with a warning) when it cannot be read.
    /// The effect a `ParticleSystem.effect` names: a file of the workspace (read again when it changes on disk), or the JSON itself when the text starts with `{`. What is wrong with it is a warning (once) and an `Err` ever after.
    pub fn fx_effect(&mut self, text: &str) -> Result<(Arc<lunaris_sim::fx::Effect>, u64), String> {
        let trimmed = text.trim();
        let inline = trimmed.starts_with('{');
        let name = if inline { format!("{{{:016x}", text_key(trimmed)) } else { trimmed.to_string() };
        if let Some(e) = self.fx_effects.get(&name) {
            if inline || !changed(&mut self.fx_stamps, &self.root, self.reload_interval, &name) {
                return e.effect.clone().map(|a| (a, e.key));
            }
        }
        let source: Result<String, String> = if inline {
            Ok(trimmed.to_string())
        } else if trimmed.is_empty() {
            Err("no effect (the `effect` field is empty)".to_string())
        } else {
            (|| {
                let root = self.root.as_ref().ok_or("no workspace folder to read effects from")?;
                let rel = std::path::Path::new(trimmed);
                if rel.is_absolute() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
                    return Err("effect paths are relative to the workspace and cannot use `..`".to_string());
                }
                std::fs::read_to_string(root.join(rel)).map_err(|e| format!("cannot read {}: {e}", root.join(rel).display()))
            })()
        };
        let label = if inline { "effect (inline)".to_string() } else { format!("effect {name}") };
        let key = source.as_ref().map_or(0, |s| text_key(s));
        let effect = source.and_then(|s| lunaris_sim::fx::parse(&s)).map(|e| {
            for w in &e.warnings {
                self.warnings.push(format!("{label}: {w}"));
            }
            Arc::new(e)
        });
        if let Err(e) = &effect {
            self.warnings.push(format!("{label}: {e}"));
        }
        if !inline {
            self.fx_stamps.insert(name.clone(), Stamp { modified: modified_time(&self.root, &name), looked: Instant::now() });
        }
        self.fx_effects.insert(name, FxEntry { effect: effect.clone(), key });
        effect.map(|a| (a, key))
    }

    pub fn model(&mut self, name: &str) -> Option<Arc<crate::mesh::Mesh>> {
        touch(&mut self.model_used, name, self.frame);
        let defer = self.can_defer();
        self.settle(if defer { None } else { Some((crate::loader::Kind::Model, name)) });
        if defer && !self.models.contains_key(name) {
            self.prefetch(crate::loader::Kind::Model, name);
            return None;
        }
        if let Some(m) = self.models.get(name) {
            if !changed(&mut self.model_stamps, &self.root, self.reload_interval, name) {
                return m.clone();
            }
        }
        let before = self.models.get(name).cloned().flatten();
        let _t = crate::Timed::new(|| format!("read model {name}"));
        let read = (|| -> Result<(PathBuf, Vec<u8>), String> {
            let root = self.root.as_ref().ok_or("no workspace folder to load models from")?;
            let path = {
                let rel = std::path::Path::new(name);
                if rel.is_absolute() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
                    return Err("model paths are relative to the workspace and cannot use `..`".to_string());
                }
                root.join(rel)
            };
            // a model the procedural provider makes (`lunaris_sim::procgen`) has no file
            let bytes = match lunaris_sim::procgen::file(name) {
                Some(made) => made?,
                None => std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?,
            };
            Ok((path, bytes))
        })();
        // the pictures a model names as files come from this cache: one that a terrain or another model has loaded is not decoded again, and is on the card once
        let folder = std::path::Path::new(name).parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let loaded = read.and_then(|(path, bytes)| {
            let mut resolver = |uri: &str| -> Option<Arc<Texture>> {
                let joined = folder.join(uri).to_string_lossy().replace('\\', "/");
                // (a file that is not there is left to the model, whose own reading says so in a warning)
                if !self.root.as_ref().is_some_and(|r| r.join(&joined).is_file()) {
                    return None;
                }
                Some(self.get_now(&joined))
            };
            crate::gltf::import_with(&bytes, path.parent(), Some(&mut resolver))
        });
        let mesh = match loaded {
            Ok(i) => {
                for w in i.warnings {
                    self.warnings.push(format!("model {name}: {w}"));
                }
                Some(Arc::new(i.mesh))
            }
            Err(e) => {
                self.warnings.push(if before.is_some() { format!("model {name}: it changed on disk but cannot be read now ({e}); the old model stays") } else { format!("model {name}: {e}") });
                before
            }
        };
        self.model_stamps.entry(name.to_string()).and_modify(|s| s.looked = Instant::now()).or_insert_with(|| Stamp { modified: modified_time(&self.root, name), looked: Instant::now() });
        self.models.insert(name.to_string(), mesh.clone());
        mesh
    }

    /// Use the models that the simulation has already read for its mesh colliders (`Sim::meshes`): a ray then needs no second reading.
    pub fn share_collision_meshes(&mut self, store: Arc<lunaris_sim::meshcol::MeshStore>) {
        self.shared_collision = Some(store);
    }

    /// Let the pictures look at the world as the simulation does: its mesh colliders and its terrain (`Sim::meshes`, `Sim::heightfields`). Particles that collide with the world (`collide.world`) need it; call it before drawing (it only keeps two shared pointers).
    pub fn share_world(&mut self, meshes: Arc<lunaris_sim::meshcol::MeshStore>, terrains: Arc<lunaris_sim::terrain::Terrains>) {
        self.shared_collision = Some(meshes);
        self.shared_terrains = Some(terrains);
    }

    /// The world's mesh colliders and terrain as `share_world` gave them (empty when it was not called).
    pub fn world_solids(&self) -> (Arc<lunaris_sim::meshcol::MeshStore>, Arc<lunaris_sim::terrain::Terrains>) {
        (self.shared_collision.clone().unwrap_or_default(), self.shared_terrains.clone().unwrap_or_default())
    }

    /// The triangles of a model file as physics sees them (see `lunaris_sim::meshcol`): the simulation's own copy if it has one, otherwise
    /// read here, once, and again when the file changes. `None` (with a warning) when the model cannot be read.
    pub fn collision_mesh(&mut self, name: &str) -> Option<Arc<lunaris_sim::meshcol::CollisionMesh>> {
        if let Some(m) = self.shared_collision.as_ref().and_then(|s| s.by_name.get(name)) {
            return Some(m.clone());
        }
        if let Some(m) = self.collision.get(name) {
            if !changed(&mut self.collision_stamps, &self.root, self.reload_interval, name) {
                return m.clone();
            }
        }
        let before = self.collision.get(name).cloned().flatten();
        let loaded = match &self.root {
            Some(root) => lunaris_sim::meshcol::CollisionMesh::read(root, name),
            None => Err("no workspace folder to load models from".to_string()),
        };
        let mesh = match loaded {
            Ok(m) => Some(Arc::new(m)),
            Err(e) => {
                let msg = if before.is_some() { format!("collision of {name}: it changed on disk but cannot be read now ({e}); the old model stays") } else { format!("collision of {name}: {e}") };
                if !self.warnings.contains(&msg) {
                    self.warnings.push(msg);
                }
                before
            }
        };
        self.collision_stamps.entry(name.to_string()).and_modify(|s| s.looked = Instant::now()).or_insert_with(|| Stamp { modified: modified_time(&self.root, name), looked: Instant::now() });
        self.collision.insert(name.to_string(), mesh.clone());
        mesh
    }

    /// The convex hull of a model stretched by `scale` (a `ConvexCollider`): the simulation's own when it is shared, otherwise made here (once for
    /// each model and scale). `None` (with a warning) when the model cannot be read or has no volume.
    pub fn hull(&mut self, name: &str, scale: [f32; 3]) -> Option<Arc<lunaris_sim::hull::Hull>> {
        if let Some(s) = &self.shared_collision {
            if s.by_name.contains_key(name) {
                return s.hull(name, scale).ok();
            }
        }
        let key = (name.to_string(), [scale[0].to_bits(), scale[1].to_bits(), scale[2].to_bits()]);
        let mesh = self.collision_mesh(name)?;
        if let Some((hash, h)) = self.hulls.get(&key) {
            if *hash == mesh.hash {
                return h.clone();
            }
        }
        let hull = match lunaris_sim::hull::Hull::build(&mesh.hull_points(), scale) {
            Ok(h) => Some(Arc::new(h)),
            Err(e) => {
                self.warnings.push(format!("convex hull of {name}: {e}"));
                None
            }
        };
        if self.hulls.len() >= 256 {
            self.hulls.clear();
        }
        self.hulls.insert(key, (mesh.hash, hull.clone()));
        hull
    }

    /// How many pictures a `CameraView` has been asked about before this one (0 the first time).
    pub fn view_tick(&mut self, uid: u64) -> u64 {
        let t = self.view_ticks.entry(uid).or_insert(0);
        let before = *t;
        *t += 1;
        before
    }

    /// Store a picture under a name (a `CameraView`'s `view:UID`): textures that are asked for by that name get it.
    pub fn put(&mut self, name: &str, tex: Texture) {
        self.map.insert(name.to_string(), Arc::new(tex));
    }

    pub fn get(&mut self, name: &str) -> Arc<Texture> {
        if self.can_defer() && !self.map.contains_key(name) && !is_procedural_texture(name) && !name.starts_with("view:") {
            // a file that is not decoded yet: read on another thread; until it is here the picture stands in (`Texture::is_pending`)
            touch(&mut self.used, name, self.frame);
            self.settle(None);
            if !self.map.contains_key(name) {
                self.prefetch(crate::loader::Kind::Texture, name);
                return Texture::pending();
            }
        }
        self.get_now(name)
    }

    /// The picture `name`, read and decoded now if it is not here yet.
    pub fn get_now(&mut self, name: &str) -> Arc<Texture> {
        touch(&mut self.used, name, self.frame);
        self.settle(Some((crate::loader::Kind::Texture, name)));
        if let Some(t) = self.map.get(name) {
            if !changed(&mut self.texture_stamps, &self.root, self.reload_interval, name) {
                return t.clone();
            }
        }
        let before = self.map.get(name).cloned();
        if before.is_some() {
            // it changed on disk: decode it again, not the copy the table holds
            self.decoded.forget(name);
        }
        let tex = if let Some(t) = Texture::procedural(name) {
            Arc::new(t)
        } else if name.starts_with("view:") {
            // the picture of a `CameraView` that has not been drawn yet (it comes with the next picture)
            Arc::new(Texture { width: 1, height: 1, texels: Texels::F32(vec![[0.0, 0.0, 0.0, 1.0]]), heights: None })
        } else {
            let stamp = Stamp { modified: modified_time(&self.root, name), looked: Instant::now() };
            self.texture_stamps.insert(name.to_string(), stamp);
            match self.load_file(name) {
                Ok(t) => t,
                Err(e) => {
                    self.decoded.forget(name);
                    match before {
                        // a picture that was fine and is being rewritten may be half written now: keep showing the old one, the next change reads it again
                        Some(old) => {
                            self.warnings.push(format!("texture `{name}`: it changed on disk but cannot be read now ({e}); the old picture stays"));
                            return old;
                        }
                        None => {
                            self.warnings.push(format!("texture `{name}`: {e} (drawing the magenta placeholder)"));
                            Arc::new(Texture::missing())
                        }
                    }
                }
            }
        };
        self.map.insert(name.to_string(), tex.clone());
        tex
    }

    /// The decoded picture of a file of the workspace (shared with whatever else has decoded it).
    fn load_file(&self, name: &str) -> Result<Arc<Texture>, String> {
        let _t = crate::Timed::new(|| format!("decode picture {name}"));
        let root = self.root.as_ref().ok_or("no workspace folder to load files from")?;
        self.decoded.picture(root, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_texels_read_as_the_floats_a_picture_used_to_be_decoded_into_and_go_to_the_card_unchanged() {
        let bytes: Vec<[u8; 4]> = (0..=255u32).map(|b| [b as u8, (255 - b) as u8, (b / 2) as u8, b as u8]).collect();
        let packed = Texels::Srgb8(bytes.clone());
        let floats = Texels::F32(bytes.iter().map(|p| [lin(p[0] as f32 / 255.0), lin(p[1] as f32 / 255.0), lin(p[2] as f32 / 255.0), p[3] as f32 / 255.0]).collect());
        assert_eq!(packed.len(), 256);
        for i in 0..256 {
            assert_eq!(packed.get(i), floats.get(i), "texel {i} reads as the same floats");
        }
        // the card's texture is the picture's own bytes, from the bytes and (through the square root) from the floats alike
        let flat: Vec<u8> = bytes.iter().flatten().copied().collect();
        assert_eq!(packed.to_rgba8(), flat);
        assert_eq!(floats.to_rgba8(), flat);
        assert_eq!(packed.iter().count(), 256);
    }

    #[test]
    fn pictures_up_to_16384_texels_a_side_are_taken_and_bigger_ones_refused_before_their_pixels_are_read() {
        let png = |w: u32, h: u32| {
            let mut out = Vec::new();
            {
                let mut enc = png::Encoder::new(&mut out, w, h);
                enc.set_color(png::ColorType::Grayscale);
                enc.set_depth(png::BitDepth::Eight);
                enc.write_header().unwrap().write_image_data(&vec![90u8; (w * h) as usize]).unwrap();
            }
            out
        };
        let t = Texture::from_png_bytes(&png(9000, 2)).unwrap();
        assert_eq!((t.width, t.height, t.texels.len()), (9000, 2, 18000));
        assert!(Texture::from_png_bytes(&png(16384, 1)).is_ok());
        let err = Texture::from_png_bytes(&png(16385, 1)).unwrap_err();
        assert!(err.contains("unsupported size"), "{err}");
    }

    #[test]
    fn what_no_picture_has_asked_for_in_a_while_is_let_go_and_read_again_when_asked_for() {
        let dir = std::env::temp_dir().join(format!("lunaris-trim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 80, 80);
            enc.set_color(png::ColorType::Grayscale);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&vec![120u8; 80 * 80]).unwrap();
        }
        std::fs::write(dir.join("big.png"), out).unwrap();
        let mut c = TextureCache::new(Some(dir));
        c.begin_frame();
        let first = c.get("big.png");
        assert_eq!(c.trim(10), 0, "just asked for");
        for _ in 0..20 {
            c.begin_frame();
        }
        assert_eq!(c.trim(10), 1, "not asked for in twenty pictures");
        let again = c.get("big.png");
        assert!(!Arc::ptr_eq(&first, &again), "it is read from its file again");
        assert_eq!((first.width, again.width, first.texels.len()), (80, 80, again.texels.len()));
        // one that every picture asks for stays; the built-in ones are never let go
        c.get("checker");
        for _ in 0..30 {
            c.begin_frame();
            c.get("big.png");
            assert_eq!(c.trim(10), 0);
        }
    }

    #[test]
    fn a_model_takes_the_pictures_it_names_from_the_cache_so_that_one_picture_is_decoded_and_held_once() {
        let dir = std::env::temp_dir().join(format!("lunaris-modelpic-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("m")).unwrap();
        let mut png = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png, 8, 8);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&vec![200u8; 8 * 8 * 3]).unwrap();
        }
        std::fs::write(dir.join("m").join("pic.png"), png).unwrap();
        let f = |v: &[f32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let mut bin = f(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        bin.extend(f(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]));
        bin.extend([0u8, 0, 1, 0, 2, 0, 0, 0]);
        std::fs::write(dir.join("m").join("tri.bin"), &bin).unwrap();
        let gltf = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"TEXCOORD_0":1}},"indices":2,"material":0}}]}}],
            "materials":[{{"pbrMetallicRoughness":{{"baseColorTexture":{{"index":0}}}}}}],"textures":[{{"source":0}}],"images":[{{"uri":"pic.png"}}],
            "buffers":[{{"byteLength":{},"uri":"tri.bin"}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":24}},{{"buffer":0,"byteOffset":60,"byteLength":6}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"}},{{"bufferView":2,"componentType":5123,"count":3,"type":"SCALAR"}}]}}"#,
            bin.len()
        );
        std::fs::write(dir.join("m").join("tri.gltf"), gltf).unwrap();
        let mut c = TextureCache::new(Some(dir));
        let model = c.model("m/tri.gltf").expect("the model loads");
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        let pic = c.get("m/pic.png");
        assert!(Arc::ptr_eq(model.tex.as_ref().expect("textured"), &pic), "the model's picture is the cache's: one decode, one copy, one upload");
    }

    #[test]
    fn a_decoded_picture_takes_four_bytes_a_texel() {
        let t = Texture::from_image_bytes(&std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata").join("jpg_prog.jpg")).unwrap()).unwrap();
        assert!(matches!(t.texels, Texels::Srgb8(ref v) if v.len() == 64 * 48));
    }

    #[test]
    fn procedural_textures_have_the_right_shape() {
        let mut c = TextureCache::new(None);
        let circle = c.get("circle");
        assert_eq!(circle.sample(0.5, 0.5)[3], 1.0);
        assert_eq!(circle.sample(0.02, 0.02)[3], 0.0);
        let ring = c.get("ring");
        assert_eq!(ring.sample(0.5, 0.5)[3], 0.0);
        let g = c.get("gradient");
        assert!(g.sample(0.95, 0.5)[0] > g.sample(0.05, 0.5)[0]);
        assert!(c.warnings.is_empty());
    }

    #[test]
    fn missing_texture_gives_a_visible_placeholder_and_a_warning() {
        let mut c = TextureCache::new(None);
        let t = c.get("nope.png");
        assert_eq!(t.sample(0.01, 0.99)[0], 1.0);
        assert_eq!(c.warnings.len(), 1);
        let mut c = TextureCache::new(Some(std::env::temp_dir()));
        c.get("../../etc/passwd");
        assert!(c.warnings[0].contains("inside the workspace"));
    }

    #[test]
    fn png_round_trip() {
        let dir = std::env::temp_dir().join(format!("lunaris-tex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.png");
        {
            let file = std::fs::File::create(&path).unwrap();
            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), 2, 1);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[255, 0, 0, 255, 0, 0, 255, 128]).unwrap();
        }
        let mut c = TextureCache::new(Some(dir));
        let t = c.get("t.png");
        assert_eq!((t.width, t.height), (2, 1));
        assert_eq!(t.sample(0.1, 0.5)[0], 1.0);
        assert!((t.sample(0.9, 0.5)[3] - 128.0 / 255.0).abs() < 1e-6);
        assert!(c.warnings.is_empty());
    }

    /// JPEGs (baseline colour with subsampled chroma, grey, progressive) read as Pillow reads them, within a few levels: the decoders do not promise the same bits.
    #[test]
    fn jpegs_decode_and_agree_with_pillow() {
        let points = [(4usize, 4usize), (16, 10), (50, 5), (8, 23), (50, 24), (4, 40), (50, 40), (33, 40)];
        let cases: [(&str, [[u8; 3]; 8]); 3] = [
            ("jpg_color.jpg", [[202, 30, 28], [249, 240, 21], [29, 30, 200], [30, 200, 42], [30, 200, 41], [200, 30, 30], [29, 30, 200], [29, 30, 200]]),
            ("jpg_gray.jpg", [[81; 3], [218; 3], [49; 3], [131; 3], [131; 3], [81; 3], [49; 3], [49; 3]]),
            ("jpg_prog.jpg", [[200, 30, 30], [250, 237, 37], [29, 30, 200], [31, 203, 41], [30, 200, 39], [200, 30, 30], [29, 30, 200], [29, 30, 200]]),
        ];
        for (name, want) in cases {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata").join(name);
            let bytes = std::fs::read(&path).unwrap();
            let t = Texture::from_image_bytes(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!((t.width, t.height), (64, 48), "{name}");
            for (k, &(x, y)) in points.iter().enumerate() {
                let px = t.texels.get(y * 64 + x);
                for c in 0..3 {
                    // the texel is in the working (gamma 2) space: back to 0..255 display levels
                    let level = px[c].sqrt() * 255.0;
                    assert!((level - want[k][c] as f32).abs() <= 8.0, "{name} at {x},{y} channel {c}: got {level}, Pillow {}", want[k][c]);
                }
            }
        }
        // through the texture cache by name, and garbage is a clear error
        let mut cache = TextureCache::new(Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")));
        assert_eq!(cache.get("jpg_color.jpg").width, 64);
        assert!(cache.warnings.is_empty(), "{:?}", cache.warnings);
        assert!(Texture::from_image_bytes(&[0xFF, 0xD8, 0xFF, 0x00, 1, 2, 3]).is_err());
    }

    fn write_png(path: &std::path::Path, rgba: [u8; 4]) {
        let file = std::fs::File::create(path).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), 1, 1);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    }

    fn touch_later(path: &std::path::Path, seconds: u64) {
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(seconds)).unwrap();
    }

    /// A picture or a model saved over while the game runs is read again; one that is mid-write (unreadable) keeps the old one.
    #[test]
    fn textures_and_models_are_read_again_when_their_files_change() {
        let dir = std::env::temp_dir().join(format!("lunaris-reload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = TextureCache::new(Some(dir.clone()));
        c.reload_interval = Duration::ZERO;
        write_png(&dir.join("r.png"), [255, 0, 0, 255]);
        let a = c.get("r.png");
        assert_eq!(a.sample(0.5, 0.5)[0], 1.0);
        // untouched: the same picture (the same Arc, so the GPU keeps its copy)
        assert!(Arc::ptr_eq(&a, &c.get("r.png")));
        write_png(&dir.join("r.png"), [0, 255, 0, 255]);
        touch_later(&dir.join("r.png"), 10);
        let b = c.get("r.png");
        assert!(!Arc::ptr_eq(&a, &b), "a new picture");
        assert_eq!((b.sample(0.5, 0.5)[0], b.sample(0.5, 0.5)[1]), (0.0, 1.0));
        // broken in the middle of a write: the green one stays and a warning says why
        std::fs::write(dir.join("r.png"), b"half a pic").unwrap();
        touch_later(&dir.join("r.png"), 20);
        let kept = c.get("r.png");
        assert!(Arc::ptr_eq(&kept, &b), "the old picture stays");
        assert!(c.warnings.iter().any(|w| w.contains("changed on disk")), "{:?}", c.warnings);
        // a model: another file under the same name is another mesh
        let models = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/models");
        std::fs::copy(models.join("crate.glb"), dir.join("m.glb")).unwrap();
        let first = c.model("m.glb").unwrap();
        assert!(Arc::ptr_eq(&first, &c.model("m.glb").unwrap()));
        std::fs::copy(models.join("swayer.glb"), dir.join("m.glb")).unwrap();
        touch_later(&dir.join("m.glb"), 10);
        let second = c.model("m.glb").unwrap();
        assert!(!Arc::ptr_eq(&first, &second) && first.pos.len() != second.pos.len(), "{} vs {}", first.pos.len(), second.pos.len());
        // with the default interval a change is not noticed at once
        let mut slow = TextureCache::new(Some(dir.clone()));
        write_png(&dir.join("s.png"), [1, 2, 3, 255]);
        let s1 = slow.get("s.png");
        write_png(&dir.join("s.png"), [9, 9, 9, 255]);
        touch_later(&dir.join("s.png"), 30);
        assert!(Arc::ptr_eq(&s1, &slow.get("s.png")), "looked at within half a second: not again yet");
    }
}
