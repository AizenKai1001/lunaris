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
    /// Linear RGBA, row-major, row 0 at the top (v = 1).
    pub texels: Vec<[f32; 4]>,
    /// A height map (0..1, the same size and layout as `texels`) for textures that have a relief: the built-in materials. The GPU
    /// renderer's high-quality mode turns it into bumps and dents; the reference renderer ignores it.
    pub heights: Option<Vec<f32>>,
}

#[inline]
fn lin(c: f32) -> f32 {
    c * c
}

impl Texture {
    /// A sample for the interface: `u` runs left to right and `v` top to bottom (0..1, clamped), nearest texel or bilinear (texel centres at half texels).
    pub fn sample_ui(&self, u: f32, v: f32, smooth: bool) -> [f32; 4] {
        let (w, h) = (self.width.max(1) as f32, self.height.max(1) as f32);
        let at = |x: f32, y: f32| self.texels[(y as usize).min(self.height as usize - 1) * self.width as usize + (x as usize).min(self.width as usize - 1)];
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
        self.texels[(y * self.width + x) as usize]
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
        Some(Texture { width: n, height: n, texels, heights: None })
    }

    fn missing() -> Texture {
        let n = 16u32;
        let mut texels = Vec::new();
        for y in 0..n {
            for x in 0..n {
                texels.push(if (x / 4 + y / 4) % 2 == 0 { [1.0, 0.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0] });
            }
        }
        Texture { width: n, height: n, texels, heights: None }
    }

    fn from_png(path: &std::path::Path) -> Result<Texture, String> {
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
        if w == 0 || h == 0 || w > 4096 || h > 4096 {
            return Err(format!("unsupported size {w}x{h} (max 4096)"));
        }
        let n = (w * h) as usize;
        let channels = rgb.len() / n.max(1);
        if channels < 3 || rgb.len() < n * channels {
            return Err(format!("JPEG: unexpected pixel data ({} bytes for {w}x{h})", rgb.len()));
        }
        let texels = rgb.chunks(channels).take(n).map(|px| [lin(px[0] as f32 / 255.0), lin(px[1] as f32 / 255.0), lin(px[2] as f32 / 255.0), 1.0]).collect();
        Ok(Texture { width: w, height: h, texels, heights: None })
    }

    fn from_png_reader<R: std::io::BufRead + std::io::Seek>(reader: R) -> Result<Texture, String> {
        let mut decoder = png::Decoder::new(reader);
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; reader.output_buffer_size().ok_or("image too large")?];
        let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        let (w, h) = (info.width, info.height);
        if w == 0 || h == 0 || w > 4096 || h > 4096 {
            return Err(format!("unsupported size {w}x{h} (max 4096)"));
        }
        let channels = match info.color_type {
            png::ColorType::Rgba => 4,
            png::ColorType::Rgb => 3,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Grayscale => 1,
            other => return Err(format!("unsupported colour type {other:?}")),
        };
        let mut texels = Vec::with_capacity((w * h) as usize);
        for px in buf[..(w * h) as usize * channels].chunks(channels) {
            let f = |b: u8| lin(b as f32 / 255.0);
            texels.push(match channels {
                4 => [f(px[0]), f(px[1]), f(px[2]), px[3] as f32 / 255.0],
                3 => [f(px[0]), f(px[1]), f(px[2]), 1.0],
                2 => [f(px[0]), f(px[0]), f(px[0]), px[1] as f32 / 255.0],
                _ => [f(px[0]), f(px[0]), f(px[0]), 1.0],
            });
        }
        Ok(Texture { width: w, height: h, texels, heights: None })
    }
}

/// Whether a texture name is one of the built-in procedural pictures (`circle`, `ring`, `checker`, `gradient`) or material textures
/// (`grass`, `dirt`, `stone`, `bricks`, `wood`, `sand`, `concrete`, `tiles`, `metal`).
pub fn is_procedural_texture(name: &str) -> bool {
    matches!(name, "circle" | "ring" | "soft" | "checker" | "gradient") || crate::procedural::MATERIALS.contains(&name)
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
    terrain_meshes: Vec<(TerrainKey, Arc<crate::mesh::Mesh>)>,
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
    /// The triangles of models for physics-side rays (a `MeshCollider`), read here when the simulation's own copy is not at hand.
    collision: BTreeMap<String, Option<Arc<lunaris_sim::meshcol::CollisionMesh>>>,
    collision_stamps: BTreeMap<String, Stamp>,
    /// Convex hulls made here, by model and scale.
    hulls: BTreeMap<(String, [u32; 3]), (u64, Option<Arc<lunaris_sim::hull::Hull>>)>,
    /// The simulation's own copies (`Sim::meshes`), used first so that a model is read once.
    shared_collision: Option<Arc<lunaris_sim::meshcol::MeshStore>>,
    pub warnings: Vec<String>,
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
type TerrainKey = (String, u32, u32, u32, u32);

/// What a water body's mesh depends on: the picture, the step, the size and the relief.
type WaterKey = (String, u32, u32, u32, u32);

impl TextureCache {
    pub fn new(root: Option<PathBuf>) -> TextureCache {
        let fonts = crate::fonts::FontCache::new(root.clone());
        TextureCache { fonts, nested: false, view_ticks: BTreeMap::new(), reload_interval: Duration::from_millis(500), texture_stamps: BTreeMap::new(), model_stamps: BTreeMap::new(), collision: BTreeMap::new(), collision_stamps: BTreeMap::new(), hulls: BTreeMap::new(), shared_collision: None, root, map: BTreeMap::new(), models: BTreeMap::new(), heights: BTreeMap::new(), splats: BTreeMap::new(), splat_stamps: BTreeMap::new(), languages: BTreeMap::new(), water_grids: BTreeMap::new(), terrain_meshes: Vec::new(), water_meshes: Vec::new(), calls: 0, warnings: Vec::new() }
    }

    /// A heightmap of the workspace (a greyscale PNG), read once. `None` (with a warning, and another try after a while) when it cannot be read.
    pub fn heightfield(&mut self, name: &str) -> Option<Arc<lunaris_sim::terrain::Heightfield>> {
        self.calls += 1;
        if let Some(e) = self.heights.get(name) {
            if e.field.is_some() || self.calls.saturating_sub(e.failed_at) < 120 {
                return e.field.clone();
            }
        }
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
        if let Some(e) = self.splats.get(name) {
            let unchanged = !changed(&mut self.splat_stamps, &self.root, self.reload_interval, name);
            if unchanged && (e.splat.is_some() || self.calls.saturating_sub(e.failed_at) < 120) {
                return e.splat.clone();
            }
        }
        let before = self.splats.get(name).and_then(|e| e.splat.clone());
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
        let field = self.heightfield(name)?;
        let key: TerrainKey = (name.to_string(), g.step, g.size[0].to_bits(), g.size[1].to_bits(), g.height.to_bits());
        if let Some((_, m)) = self.terrain_meshes.iter().find(|(k, _)| *k == key) {
            return Some(m.clone());
        }
        let mesh = Arc::new(crate::terrain::build_mesh(&field, g));
        self.terrain_meshes.push((key, mesh.clone()));
        if self.terrain_meshes.len() > 6 {
            self.terrain_meshes.remove(0);
        }
        Some(mesh)
    }

    /// The surface of a water body (kept for the next picture; the few most recent are remembered): the picture `name` read as the heights of the water, with the size, step and relief of
    /// `w`. `None` when the picture cannot be read (a warning says why) or has no water in it.
    pub fn water_mesh(&mut self, name: &str, w: &lunaris_sim::components::WaterBody) -> Option<Arc<crate::mesh::Mesh>> {
        let field = self.heightfield(name)?;
        let key: WaterKey = (name.to_string(), w.step, w.size[0].to_bits(), w.size[1].to_bits(), w.relief.to_bits());
        if let Some((_, m)) = self.water_meshes.iter().find(|(k, _)| *k == key) {
            return m.clone();
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
                Arc::new(crate::mesh::Mesh { pos, nrm, uv, col: Vec::new(), texw: Vec::new(), idx, tex: None, rig: None })
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

    /// An imported model (`.glb` / `.gltf` in the workspace), loaded once. `None` (with a warning) when it cannot be read.
    pub fn model(&mut self, name: &str) -> Option<Arc<crate::mesh::Mesh>> {
        if let Some(m) = self.models.get(name) {
            if !changed(&mut self.model_stamps, &self.root, self.reload_interval, name) {
                return m.clone();
            }
        }
        let before = self.models.get(name).cloned().flatten();
        let loaded = (|| -> Result<crate::gltf::Imported, String> {
            let root = self.root.as_ref().ok_or("no workspace folder to load models from")?;
            let path = {
                let rel = std::path::Path::new(name);
                if rel.is_absolute() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
                    return Err("model paths are relative to the workspace and cannot use `..`".to_string());
                }
                root.join(rel)
            };
            let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            crate::gltf::import(&bytes, path.parent())
        })();
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
        if let Some(t) = self.map.get(name) {
            if !changed(&mut self.texture_stamps, &self.root, self.reload_interval, name) {
                return t.clone();
            }
        }
        let before = self.map.get(name).cloned();
        let tex = if let Some(t) = Texture::procedural(name) {
            t
        } else if name.starts_with("view:") {
            // the picture of a `CameraView` that has not been drawn yet (it comes with the next picture)
            Texture { width: 1, height: 1, texels: vec![[0.0, 0.0, 0.0, 1.0]], heights: None }
        } else {
            let stamp = Stamp { modified: modified_time(&self.root, name), looked: Instant::now() };
            self.texture_stamps.insert(name.to_string(), stamp);
            match self.load_file(name) {
                Ok(t) => t,
                Err(e) => match before {
                    // a picture that was fine and is being rewritten may be half written now: keep showing the old one, the next change reads it again
                    Some(old) => {
                        self.warnings.push(format!("texture `{name}`: it changed on disk but cannot be read now ({e}); the old picture stays"));
                        return old;
                    }
                    None => {
                        self.warnings.push(format!("texture `{name}`: {e} (drawing the magenta placeholder)"));
                        Texture::missing()
                    }
                },
            }
        };
        let tex = Arc::new(tex);
        self.map.insert(name.to_string(), tex.clone());
        tex
    }

    fn load_file(&self, name: &str) -> Result<Texture, String> {
        let root = self.root.as_ref().ok_or("no workspace folder to load files from")?;
        let p = std::path::Path::new(name);
        if p.components().any(|c| !matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir)) {
            return Err("path must be relative and stay inside the workspace".into());
        }
        Texture::from_png(&root.join(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                let px = t.texels[y * 64 + x];
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
