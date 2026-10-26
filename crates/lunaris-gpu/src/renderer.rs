//! The renderer: owns the device, caches meshes, textures and pipelines, and draws a `Scene` into an image or a window surface.
use crate::compress::{self, compress_level, Block};
use crate::gpu::Gpu;
use crate::pipelines::*;
use crate::util::*;
use bytemuck::{Pod, Zeroable};
use lunaris_ecs::World;
use lunaris_render::math::{cross, dot, length, sub, M4, V3};
use lunaris_render::mesh::Mesh;
use lunaris_render::render::{gather_scene, gather_scene_with, ExplicitCamera, MeshPbr, MeshRef, RenderOptions, Scene, SceneParticles, SceneProbe, View, BLADES_PER_BUNDLE};
use lunaris_render::terrain_layers::{Splat, SplatLayers};
use lunaris_render::Frame;
use lunaris_render::texture::Texture;
use lunaris_render::{font, TextureCache};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_LIGHTS: usize = 16;
/// The most point or spot lights that cast shadows (six faces of a shadow map each); the renderer's own two, up to this many with `Lighting.point_shadows`.
const POINT_SHADOWS: usize = 4;
/// How many reflection probes work at once (the four with the lowest uids); each has six faces.
const MAX_PROBES: usize = 4;
const OUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Matches the reference CPU renderer.
    Classic,
    /// The good-looking one.
    High,
}

/// How a picture is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Quality {
    pub mode: Mode,
    /// Samples per pixel: 1 or 4.
    pub msaa: u32,
    /// Side of one shadow map in texels (three cascades).
    pub shadow_size: u32,
    /// Side of the shadow maps of point lights (six faces each, for the two most important lights); 0 = point lights cast no shadows.
    pub point_shadow_size: u32,
    pub shadows: bool,
    pub ssao: bool,
    pub bloom: bool,
    /// Depth of field (when the `Environment` asks for it).
    pub dof: bool,
    /// Temporal anti-aliasing: the picture is drawn with the pixel grid shifted a little every time and blended with the last ones (a still is drawn several times and averaged).
    pub taa: bool,
}

impl Quality {
    pub fn classic() -> Quality {
        Quality { mode: Mode::Classic, msaa: 1, shadow_size: 1024, point_shadow_size: 0, shadows: false, ssao: false, bloom: false, dof: false, taa: false }
    }

    pub fn low() -> Quality {
        Quality { mode: Mode::High, msaa: 1, shadow_size: 1024, point_shadow_size: 0, shadows: true, ssao: false, bloom: false, dof: false, taa: false }
    }

    pub fn medium() -> Quality {
        Quality { mode: Mode::High, msaa: 4, shadow_size: 1024, point_shadow_size: 512, shadows: true, ssao: false, bloom: true, dof: false, taa: false }
    }

    pub fn high() -> Quality {
        Quality { mode: Mode::High, msaa: 4, shadow_size: 2048, point_shadow_size: 1024, shadows: true, ssao: true, bloom: true, dof: true, taa: true }
    }

    pub fn ultra() -> Quality {
        Quality { mode: Mode::High, msaa: 4, shadow_size: 4096, point_shadow_size: 2048, shadows: true, ssao: true, bloom: true, dof: true, taa: true }
    }

    /// "classic", "low", "medium", "high" or "ultra".
    pub fn from_name(name: &str) -> Option<Quality> {
        match name {
            "classic" => Some(Quality::classic()),
            "low" => Some(Quality::low()),
            "medium" => Some(Quality::medium()),
            "high" => Some(Quality::high()),
            "ultra" => Some(Quality::ultra()),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------------------------------------ GPU data layouts

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ObjectData {
    model: [f32; 16],
    n0: [f32; 4],
    n1: [f32; 4],
    n2: [f32; 4],
    color: [f32; 4],
    mat: [f32; 4],
    misc: [f32; 4],
    uvrect: [f32; 4],
    ids: [u32; 4],
    /// `PbrMaps`: x the normal map's strength, y the occlusion strength, z which maps there are (1 normal, 2 arm, 3 both), w the roughness scale; all 0 for an object without them. Matches `Object.pbr` in `common.wgsl`.
    pbr: [f32; 4],
    /// `MacroVary`: x the strength, y the scale in metres, z the colour lean, w the roughness variation; all 0 for an object without it. Matches `Object.vary` in `common.wgsl`.
    vary: [f32; 4],
}

/// What the shader of a painted terrain (`TerrainLayers`) needs besides its textures: each layer's repeat, relief depth and tint. Matches `SplatU` in `scene.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SplatU {
    scales: [[f32; 4]; 2],
    bump: [f32; 4],
    tint: [[f32; 4]; 4],
    /// x: how many layers, y: 1 when any layer has relief to show
    info: [f32; 4],
    /// each layer's picture averaged over the whole of it (linear)
    avg: [[f32; 4]; 4],
    /// x: the layer that steep ground shows, laid on from the sides (-1: none), y: metres the layer's picture covers before it repeats
    cliff: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PointShadowU {
    vp: [[f32; 16]; 6],
    info: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameU {
    view_proj: [f32; 16],
    inv_view_proj: [f32; 16],
    view: [f32; 16],
    cascade_vp: [[f32; 16]; 3],
    cascade_info: [f32; 4],
    cascade_texel: [f32; 4],
    /// metres of light-space depth of each cascade, and (w) the softness of the sun's shadows (0 = the plain filter)
    cascade_depth: [f32; 4],
    eye: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    sky_top: [f32; 4],
    sky_horizon: [f32; 4],
    ground: [f32; 4],
    fog: [f32; 4],
    clear_color: [f32; 4],
    p0: [f32; 4],
    p1: [f32; 4],
    p2: [f32; 4],
    p3: [f32; 4],
    p4: [f32; 4],
    p5: [f32; 4],
    p6: [f32; 4],
    p7: [f32; 4],
    p8: [f32; 4],
    p9: [f32; 4],
    p10: [f32; 4],
    p11: [f32; 4],
    p12: [f32; 4],
    p13: [f32; 4],
    p14: [f32; 4],
    p15: [f32; 4],
    p16: [f32; 4],
    p17: [f32; 4],
    p18: [f32; 4],
    /// the lens: distortion, zoom, chromatic fringe, soften; then block size, scan lines
    p19: [f32; 4],
    p20: [f32; 4],
    /// volumetric light: density, anisotropy, steps, farthest; then colour and intensity
    p21: [f32; 4],
    p22: [f32; 4],
    /// bounced light: intensity, radius, directions, thickness
    p23: [f32; 4],
    /// the share of the new bounced light in what is shown (the rest is the last pictures')
    p24: [f32; 4],
    /// the sky of the air: turbidity, rayleigh, mie_g, intensity; then (x) on, (y) the glow of fog towards the sun
    p25: [f32; 4],
    p26: [f32; 4],
    /// temporal anti-aliasing: the share of the new picture, and (y) 1 when it is on
    p27: [f32; 4],
    /// water that looks at the depth of what is behind it: x is 1 when the picture has that depth
    p28: [f32; 4],
    /// the sea the terrain meets: the significant wave height and the wave length of the peak
    p29: [f32; 4],
    /// the window of the sea's depth: the world x and z of its first point, 1 over the distance between points, and 1 when there is one
    p30: [f32; 4],
    /// `Lighting`: the fill rgb and the bounce
    p31: [f32; 4],
    /// `Lighting`: how far the lamps' falloff is the inverse-square one, the size of its core, the gain that keeps the old brightness where the two curves meet
    p32: [f32; 4],
    /// the depth of the sea over that window, 128 x 128 points as 16-bit floats, two to a word
    sea: [[u32; 4]; 2048],
    probes: [[f32; 4]; 8],
    prev_view_proj: [f32; 16],
    /// the view-projection of this picture and of the last one, without the sub-pixel shift (temporal anti-aliasing)
    unj_vp: [f32; 16],
    unj_prev_vp: [f32; 16],
    psh: [PointShadowU; POINT_SHADOWS],
    lights: [[f32; 4]; MAX_LIGHTS * 4],
}

fn v4(v: V3, w: f32) -> [f32; 4] {
    [v[0], v[1], v[2], w]
}

// ------------------------------------------------------------------------------------------------ cached resources

/// A run of the triangles of a big mesh that lie near each other (found when the mesh is uploaded, the triangles ordered along a Morton curve): the piece that culling can leave out.
#[derive(Clone, Copy)]
struct Cluster {
    /// The first index and the number of indices.
    first: u32,
    count: u32,
    center: V3,
    radius: f32,
}

/// One level of detail of a mesh: a run of indices (into the same vertices) and the size of the grid cell it was made with (the most a vertex moved is about half of it).
#[derive(Clone, Copy)]
struct LodRange {
    first: u32,
    count: u32,
    cell: f32,
}

/// A mesh made ready for the card on another thread: its interleaved vertices, the indices (clusters and levels of detail in one buffer) and the numbers about them.
struct PreparedMesh {
    /// The vertex and index buffers, made on the worker thread (a big allocation takes the card's driver a while) when it was given the device.
    buffers: Option<(wgpu::Buffer, wgpu::Buffer)>,
    min_lod: u8,
    v: Vec<f32>,
    center: V3,
    radius: f32,
    indices: Vec<u32>,
    clusters: Vec<Cluster>,
    lods: Vec<LodRange>,
    half: V3,
}

/// The levels of a texture on their way to the card a few rows at a time (so that a picture of 40 megabytes costs a few pictures a millisecond or two each and not one of them forty): where the next copy starts.
struct TexUpload {
    tex: wgpu::Texture,
    levels: Vec<(u32, u32, Vec<u8>)>,
    block: Option<Block>,
    /// Bytes a texel takes when the texture is not block-compressed (4 for colours, 1 for a relief).
    bytes_per_texel: u32,
    level: usize,
    row: u32,
}

impl TexUpload {
    fn new(tex: wgpu::Texture, levels: Vec<(u32, u32, Vec<u8>)>, block: Option<Block>, bytes_per_texel: u32) -> TexUpload {
        TexUpload { tex, levels, block, bytes_per_texel, level: 0, row: 0 }
    }

    /// Copy up to `budget` bytes more (at least one row); true when every level is on the card.
    fn step(&mut self, queue: &wgpu::Queue, budget: &mut u64) -> bool {
        while self.level < self.levels.len() {
            let (lw, lh, data) = &self.levels[self.level];
            // rows are rows of blocks when compressed; `unit` is how many texel rows one of them covers
            let (width, row_bytes, rows, unit) = match self.block {
                Some(b) => {
                    let (bw, bh) = (lw.div_ceil(4), lh.div_ceil(4));
                    (bw * 4, bw * b.bytes() as u32, bh, 4)
                }
                None => (*lw, lw * self.bytes_per_texel, *lh, 1),
            };
            let take = (rows - self.row).min(((*budget / row_bytes.max(1) as u64) as u32).max(1));
            let (from, to) = ((self.row * row_bytes) as usize, ((self.row + take) * row_bytes) as usize);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &self.tex, mip_level: self.level as u32, origin: wgpu::Origin3d { x: 0, y: self.row * unit, z: 0 }, aspect: wgpu::TextureAspect::All },
                &data[from..to],
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row_bytes), rows_per_image: Some(take) },
                wgpu::Extent3d { width, height: take * unit, depth_or_array_layers: 1 },
            );
            *budget = budget.saturating_sub((to - from) as u64);
            self.row += take;
            if self.row >= rows {
                self.level += 1;
                self.row = 0;
            }
            if *budget == 0 {
                break;
            }
        }
        self.level >= self.levels.len()
    }
}

/// A picture being copied to the card (asynchronous mode): the texture and its relief are created, the rest follows over the next pictures; it is used once all of it is there.
struct PartialTex {
    key: (usize, bool),
    colour: TexUpload,
    height: Option<TexUpload>,
    gpu: GpuTex,
    /// Keeps the picture's address (the key) from being used for another one meanwhile.
    _tex: Arc<Texture>,
}

struct PartialData {
    key: usize,
    up: TexUpload,
    gpu: GpuTex,
    _tex: Arc<Texture>,
}

/// What a plain one-level texture is made from: a terrain's splat map (four weights a texel) or a heightmap (one float a sample, for the grass).
#[derive(Clone)]
enum PlainSrc {
    Splat(Arc<Splat>),
    Heights(Arc<lunaris_sim::terrain::Heightfield>),
}

impl PlainSrc {
    fn key(&self) -> usize {
        match self {
            PlainSrc::Splat(s) => Arc::as_ptr(s) as usize,
            PlainSrc::Heights(h) => Arc::as_ptr(h) as usize,
        }
    }
}

/// A plain texture made ready on another thread: its bytes, and the texture itself when it was made there too.
struct PreparedPlain {
    texture: Option<wgpu::Texture>,
    w: u32,
    h: u32,
    bytes: Vec<u8>,
    format: wgpu::TextureFormat,
}

struct PlainJob {
    pending: lunaris_render::bg::Pending<PreparedPlain>,
    ready: Option<PreparedPlain>,
    _src: PlainSrc,
    asked: u64,
}

struct PartialPlain {
    src: PlainSrc,
    up: TexUpload,
    view: wgpu::TextureView,
}

/// A mesh being copied to the card (asynchronous mode): its buffers exist, the vertices and indices follow a slice a picture.
struct PartialMesh {
    key: usize,
    gpu: GpuMesh,
    v: Vec<f32>,
    indices: Vec<u32>,
    v_done: usize,
    i_done: usize,
    _mesh: Arc<Mesh>,
}

/// A mesh on its way (asynchronous mode): the thread that prepares it, the result once taken, the mesh (kept so that its address is not used for another one meanwhile) and the last picture that asked.
struct MeshJob {
    pending: lunaris_render::bg::Pending<PreparedMesh>,
    ready: Option<PreparedMesh>,
    _mesh: Arc<Mesh>,
    asked: u64,
}

/// A picture made ready for the card on another thread: its mip levels as they go to the card (blocks when compressed), the average colour, and the relief levels of a built-in material.
struct PreparedTex {
    /// The colour texture and the relief texture, made on the worker thread.
    textures: Option<(wgpu::Texture, Option<wgpu::Texture>)>,
    w: u32,
    h: u32,
    levels: Vec<(u32, u32, Vec<u8>)>,
    avg: [f32; 3],
    block: Option<Block>,
    bytes: u64,
    height: Option<Vec<(u32, u32, Vec<u8>)>>,
}

struct TexJob {
    pending: lunaris_render::bg::Pending<PreparedTex>,
    ready: Option<PreparedTex>,
    _tex: Arc<Texture>,
    asked: u64,
}

/// A normal or occlusion-roughness-metallic map made ready for the card.
struct PreparedData {
    texture: Option<wgpu::Texture>,
    w: u32,
    h: u32,
    levels: Vec<(u32, u32, Vec<u8>)>,
    block: Option<Block>,
    bytes: u64,
}

struct DataJob {
    pending: lunaris_render::bg::Pending<PreparedData>,
    ready: Option<PreparedData>,
    _tex: Arc<Texture>,
    asked: u64,
}

/// A mesh of at least this many triangles gets coarser versions of itself for when it is far off.
const LOD_MIN_TRIANGLES: usize = 600;
/// The grid cells along the biggest side of the mesh's box for each coarser level: a vertex moves by at most about half a cell, 1/768 of the size of the mesh for the first level.
const LOD_GRIDS: [f32; 8] = [384.0, 192.0, 96.0, 48.0, 24.0, 12.0, 6.0, 3.0];
/// How far (pixels, on the screen) a vertex may be from where it was at the level chosen for an object.
const LOD_PIXELS_DEFAULT: f32 = 2.0;

/// How far (pixels) a vertex may be from where it was at the level chosen for an object (`LUNARIS_LOD_PIXELS` sets it; default 2).
fn lod_pixels() -> f32 {
    static V: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("LUNARIS_LOD_PIXELS").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| *v > 0.0).unwrap_or(LOD_PIXELS_DEFAULT))
}

/// The most triangles a mesh is drawn with at its finest (`LUNARIS_LOD_BASE_TRIS` sets it; default 150,000): a photographed tree of 800,000 triangles is drawn at the first of its coarser levels that fits, however close the camera is
/// (a thousand of them in view are the whole frame's cost; at that size a triangle is a pixel or less).
fn lod_base_triangles() -> usize {
    static V: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("LUNARIS_LOD_BASE_TRIS").ok().and_then(|v| v.parse::<usize>().ok()).filter(|v| *v > 0).unwrap_or(150_000))
}

/// The screen area (square pixels) each triangle of an object should have at least (`LUNARIS_LOD_TRI_PIXELS` sets it; default 3; 0 turns the rule off). A triangle smaller than that costs as much to draw as a bigger one (the card shades
/// pixels in blocks of 2 x 2) and adds nothing anyone can see: a tree 80 pixels tall is drawn with a few hundred triangles however many its finest level has.
fn lod_triangle_pixels() -> f32 {
    static V: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("LUNARIS_LOD_TRI_PIXELS").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| *v >= 0.0).unwrap_or(3.0))
}

/// The finest level whose triangles are each at least `tri_pixels` square pixels on the screen, for an object whose bounding sphere has the radius `radius` (metres, scale included) and is `distance` away.
fn density_lod(lods: &[LodRange], distance: f32, radius: f32, focal_px: f32, tri_pixels: f32) -> usize {
    if tri_pixels <= 0.0 || lods.len() < 2 {
        return 0;
    }
    let r_px = radius * focal_px / distance.max(0.05);
    // (a tree or a house fills about half of the circle of its bounding sphere)
    let area = 0.5 * std::f32::consts::PI * r_px * r_px;
    let budget = area / tri_pixels;
    lods.iter().position(|l| (l.count / 3) as f32 <= budget).unwrap_or(lods.len() - 1)
}

/// The first level of detail that has no more triangles than the base budget (0 when the whole mesh has).
fn min_lod_of(lods: &[LodRange]) -> u8 {
    if lods.len() < 2 {
        return 0;
    }
    let budget = lod_base_triangles();
    lods.iter().position(|l| (l.count / 3) as usize <= budget).unwrap_or(lods.len() - 1).min(255) as u8
}

/// How far (shadow-map texels) a vertex may be from where it was at the level a shadow cascade draws.
const SHADOW_LOD_TEXELS: f32 = 0.7;

/// Coarser versions of a mesh by vertex clustering: the box is cut into a grid, all the vertices in a cell are replaced by the one of them nearest to their mean, and the triangles that fall in on themselves are dropped. The
/// result is a list of indices into the same vertices (so the mesh's vertex buffer serves every level, the textures and normals of the kept vertices standing in for the rest). A level that has not shed at least a fifth of
/// the triangles of the one before is not kept. Returns the indices of all the coarser levels one after the other and where each starts; the vertices are the mesh's.
fn make_lods(mesh: &Mesh) -> Vec<(Vec<u32>, f32)> {
    let tris = mesh.idx.len() / 3;
    if tris < LOD_MIN_TRIANGLES || mesh.rig.is_some() || mesh.lod == lunaris_render::mesh::LodMode::Whole {
        return Vec::new();
    }
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &mesh.pos {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]);
    if !(size > 1.0e-6) {
        return Vec::new();
    }
    // the levels do not depend on each other: each is made on a thread of its own, and the rule that a level must shed a fifth of the one before it is applied afterwards, in order
    let made: Vec<(Vec<u32>, f32)> = std::thread::scope(|s| {
        let handles: Vec<_> = LOD_GRIDS.iter().map(|&grid| s.spawn(move || (lod_level(mesh, lo, size / grid), size / grid))).collect();
        handles.into_iter().map(|h| h.join().expect("a level of detail")).collect()
    });
    let mut out: Vec<(Vec<u32>, f32)> = Vec::new();
    let mut previous = tris;
    for (idx, cell) in made {
        let n = idx.len() / 3;
        if n == 0 || n * 5 > previous * 4 {
            continue;
        }
        previous = n;
        out.push((idx, cell));
    }
    out
}

/// One level of `make_lods`: the indices of the mesh on a grid of cells of the given size (the box's low corner is the grid's origin).
fn lod_level(mesh: &Mesh, lo: V3, cell: f32) -> Vec<u32> {
    let key = |p: V3| -> u64 {
        let q = |v: f32, l: f32| (((v - l) / cell) as u64) & 0x1F_FFFF;
        (q(p[0], lo[0]) << 42) | (q(p[1], lo[1]) << 21) | q(p[2], lo[2])
    };
    let keys: Vec<u64> = mesh.pos.iter().map(|p| key(*p)).collect();
    // the mean of the vertices in each cell, then the vertex nearest to it
    let mut sums: FastMap<u64, ([f64; 3], u32)> = FastMap::default();
    for (p, k) in mesh.pos.iter().zip(&keys) {
        let e = sums.entry(*k).or_insert(([0.0; 3], 0));
        for c in 0..3 {
            e.0[c] += p[c] as f64;
        }
        e.1 += 1;
    }
    let mut best: FastMap<u64, (f32, u32)> = FastMap::default();
    for (i, (p, k)) in mesh.pos.iter().zip(&keys).enumerate() {
        let (s, n) = sums[k];
        let mean = [(s[0] / n as f64) as f32, (s[1] / n as f64) as f32, (s[2] / n as f64) as f32];
        let d = (p[0] - mean[0]).powi(2) + (p[1] - mean[1]).powi(2) + (p[2] - mean[2]).powi(2);
        let e = best.entry(*k).or_insert((f32::MAX, i as u32));
        if d < e.0 {
            *e = (d, i as u32);
        }
    }
    let rep: Vec<u32> = keys.iter().map(|k| best[k].1).collect();
    let mut idx: Vec<u32> = Vec::with_capacity(mesh.idx.len() / 2);
    for t in mesh.idx.chunks_exact(3) {
        let (a, b, c) = (rep[t[0] as usize], rep[t[1] as usize], rep[t[2] as usize]);
        if a != b && b != c && a != c {
            // a triangle that the clustering has turned over (its corners moved past one another) would be a hole under back-face culling: left out
            let n_old = cross(sub(mesh.pos[t[1] as usize], mesh.pos[t[0] as usize]), sub(mesh.pos[t[2] as usize], mesh.pos[t[0] as usize]));
            let n_new = cross(sub(mesh.pos[b as usize], mesh.pos[a as usize]), sub(mesh.pos[c as usize], mesh.pos[a as usize]));
            if dot(n_old, n_new) >= 0.0 {
                idx.extend_from_slice(&[a, b, c]);
            }
        }
    }
    idx
}

/// Which level of detail to draw a mesh at, from how far its nearest point is (metres), how much the object is scaled and how many pixels a metre is at one metre: the coarsest level whose vertices (moved by
/// at most half a cell) stay within about a pixel of where they were.
fn choose_lod(lods: &[LodRange], distance: f32, scale: f32, focal_px: f32, min_lod: u8) -> usize {
    choose_lod_at(lods, distance, scale, focal_px, min_lod, lod_pixels())
}

/// `choose_lod` with the tolerance given (pixels).
fn choose_lod_at(lods: &[LodRange], distance: f32, scale: f32, focal_px: f32, min_lod: u8, tolerance: f32) -> usize {
    let mut best = 0;
    for (i, l) in lods.iter().enumerate().skip(1) {
        if l.cell * scale * 0.5 * focal_px / distance.max(0.05) <= tolerance {
            best = i;
        } else {
            break;
        }
    }
    best.max(min_lod as usize)
}

/// A mesh of at least this many triangles is cut into clusters of `CLUSTER_TRIANGLES`.
const CLUSTER_MIN_TRIANGLES: usize = 24_576;
const CLUSTER_TRIANGLES: usize = 4096;

/// The triangles of a big mesh in Morton order (the centre of each triangle placed on a 1024-step grid over the mesh's box, the bits of the three coordinates interleaved), cut into runs of `CLUSTER_TRIANGLES` with a
/// bounding sphere each. `None` for a mesh that is small enough to be culled whole. The order is the same on every run (ties are broken by the triangle's number).
fn make_clusters(mesh: &Mesh) -> Option<(Vec<u32>, Vec<Cluster>)> {
    let tris = mesh.idx.len() / 3;
    if tris < CLUSTER_MIN_TRIANGLES {
        return None;
    }
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in &mesh.pos {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let spread: [f32; 3] = [(hi[0] - lo[0]).max(1.0e-6), (hi[1] - lo[1]).max(1.0e-6), (hi[2] - lo[2]).max(1.0e-6)];
    let spread_bits = |mut x: u32| -> u32 {
        x &= 0x3ff;
        x = (x | (x << 16)) & 0x0300_00ff;
        x = (x | (x << 8)) & 0x0300_f00f;
        x = (x | (x << 4)) & 0x030c_30c3;
        (x | (x << 2)) & 0x0924_9249
    };
    let keys: Vec<u32> = mesh
        .idx
        .chunks_exact(3)
        .map(|t| {
            let (a, b, c) = (mesh.pos[t[0] as usize], mesh.pos[t[1] as usize], mesh.pos[t[2] as usize]);
            let q = |k: usize| (((a[k] + b[k] + c[k]) / 3.0 - lo[k]) / spread[k] * 1023.0).clamp(0.0, 1023.0) as u32;
            spread_bits(q(0)) | (spread_bits(q(1)) << 1) | (spread_bits(q(2)) << 2)
        })
        .collect();
    let mut order: Vec<u32> = (0..tris as u32).collect();
    order.sort_unstable_by_key(|&t| (keys[t as usize], t));
    let mut idx = Vec::with_capacity(mesh.idx.len());
    for &t in &order {
        let t = t as usize * 3;
        idx.extend_from_slice(&mesh.idx[t..t + 3]);
    }
    let clusters = idx
        .chunks(CLUSTER_TRIANGLES * 3)
        .enumerate()
        .map(|(i, run)| {
            let (mut clo, mut chi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for &v in run {
                let p = mesh.pos[v as usize];
                for k in 0..3 {
                    clo[k] = clo[k].min(p[k]);
                    chi[k] = chi[k].max(p[k]);
                }
            }
            let center = [(clo[0] + chi[0]) * 0.5, (clo[1] + chi[1]) * 0.5, (clo[2] + chi[2]) * 0.5];
            let radius = run.iter().map(|&v| length(sub(mesh.pos[v as usize], center))).fold(0.0f32, f32::max);
            Cluster { first: (i * CLUSTER_TRIANGLES * 3) as u32, count: run.len() as u32, center, radius }
        })
        .collect();
    Some((idx, clusters))
}

struct GpuMesh {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    count: u32,
    /// Empty for a mesh that is culled whole.
    clusters: Vec<Cluster>,
    /// The levels of detail: the first is the whole mesh (`0..count`); a small mesh has only that.
    lods: Vec<LodRange>,
    /// The finest level it is drawn at (see `lod_base_triangles`).
    min_lod: u8,
    use_vcol: bool,
    /// The texture of an imported model, and its normal and metallic-roughness maps, and where it is cut out (0 = nowhere).
    texture: Option<Arc<Texture>>,
    normal: Option<Arc<Texture>>,
    arm: Option<Arc<Texture>>,
    cutoff: f32,
    /// Keeps the mesh's address (which is the cache key) from being used for another mesh while it is cached. Weak: the allocation stays, the vertices go when the scene and the texture cache let go of the mesh.
    _keep: Option<std::sync::Weak<Mesh>>,
    last_used: u64,
    center: V3,
    radius: f32,
    /// Half the size of the box round the vertices (its middle is `center`).
    half: V3,
    /// For the vertex buffer of a skinned model's entity: the address of the model whose rest pose the card skins into it every picture (`skin.rs`).
    skin: Option<usize>,
}

/// The numbers of one ring of a grass field (see `GrassU` in `scene.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GrassU {
    ring: [f32; 4],
    orig: [f32; 4],
    tpos: [f32; 4],
    tsize: [f32; 4],
    wind: [f32; 4],
    tuft: [f32; 4],
    ca: [f32; 4],
    cb: [f32; 4],
    eye: [f32; 4],
    /// Single blades: the least and the greatest height (m), the width at the foot (m), how much the blades of a clump share.
    blade: [f32; 4],
    /// Single blades: the bend, the size of a clump (m), the number of blades in a bundle, 1 when the ring is made of blades.
    blade2: [f32; 4],
    cbase: [f32; 4],
    ctip: [f32; 4],
    /// x: 1 when the grass grows only on a layer of the terrain's paint (the mask picture).
    mask: [f32; 4],
}

/// The two numbers that turn a depth-buffer value (reversed: 1 at the near plane, 0 far away) into the distance in front of the camera, for the shaders that read the depth many times (the reflection ray march): for a perspective
/// projection the distance is `1 / (d * k1 + k2)` (k1 = 1/near - 1/far, k2 = 1/far), for an orthographic one `d * k1 + k2`; `proj` is the OpenGL projection (`Scene::proj`) and the GPU's depth is `0.5 * (1 - z_ndc)`.
fn view_depth_constants(proj: &M4) -> (f32, f32) {
    let m = &proj.0;
    if m[15] == 0.0 {
        (2.0 * m[11] / m[14], (m[10] - m[11]) / m[14])
    } else {
        (2.0 / m[10], (m[14] - 1.0) / m[10])
    }
}

/// What a mesh and its material give each object that draws them (worked out once for every pair in a picture).
struct MatInfo {
    id: MeshId,
    tkey: TexKey,
    use_vcol: bool,
    center: V3,
    radius: f32,
    half: V3,
    /// Where an imported material is cut out (alpha cut-off), 0 for none.
    cutoff: f32,
    lods: Vec<LodRange>,
    min_lod: u8,
    whole: u32,
    tex_some: bool,
    tex_heights: bool,
    pbr_eff: Option<MeshPbr>,
    splat: Option<usize>,
    /// The number of its (mesh, texture, terrain) among the picture's, for grouping.
    class: u32,
    /// False while its mesh or a picture it needs is not on the card yet (asynchronous mode): the objects with it stay out of the picture.
    ready: bool,
}

impl MatInfo {
    fn waiting() -> MatInfo {
        MatInfo { id: MeshId::Obj(0), tkey: (0, false, SamplerKind::NearestRepeat, 0, 0, 0), use_vcol: false, center: [0.0; 3], radius: 0.0, half: [0.0; 3], cutoff: 0.0, lods: Vec::new(), min_lod: 0, whole: 0, tex_some: false, tex_heights: false, pbr_eff: None, splat: None, class: 0, ready: false }
    }
}

/// A texture bind group's key: the colour texture, the quality, the sampler, and the addresses of the normal and occlusion-roughness-metallic maps (0 for none).
type TexKey = (usize, bool, SamplerKind, usize, usize, usize);

struct GpuTex {
    view: wgpu::TextureView,
    /// The picture averaged over the whole of it, in linear light (the last level of its mips).
    avg: [f32; 3],
    height: Option<wgpu::TextureView>,
    /// As for meshes: the address is the key, the pixels may go (what is on the card is a copy).
    _keep: Option<std::sync::Weak<Texture>>,
    last_used: u64,
    /// What it takes on the card (all its levels).
    bytes: u64,
}

/// What a picture is for: a normal map and an occlusion-roughness-metallic map are block-compressed in their own ways; a displacement map is left as it is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DataKind {
    Normal = 1,
    Arm = 2,
    Disp = 3,
}

/// Pictures smaller than this (on a side) are not block-compressed: they are little, and sprites and pixel art would only suffer.
const COMPRESS_MIN: u32 = 64;

/// A map's key in `data_textures` and in the texture bind groups' keys: its address with what it is used as in the low bits (an `Arc` is aligned to 8), so one picture used for two things is two textures.
fn data_id(tex: &Arc<Texture>, kind: DataKind) -> usize {
    Arc::as_ptr(tex) as usize | kind as usize
}

/// A splat map on the card: weights in an 8-bit texture (not a colour: no sRGB, no mips).
struct GpuSplat {
    view: wgpu::TextureView,
    _keep: Arc<Splat>,
    last_used: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SamplerKind {
    NearestRepeat,
    NearestClamp,
    Aniso,
    LinearClamp,
}

struct Samplers {
    nearest_repeat: wgpu::Sampler,
    nearest_clamp: wgpu::Sampler,
    aniso: wgpu::Sampler,
    /// The painted terrain's layers: eight taps at a slant are enough (the ground seen at a grazing angle is where sixteen cost most).
    aniso_ground: wgpu::Sampler,
    linear_clamp: wgpu::Sampler,
    shadow: wgpu::Sampler,
    /// The faces of the reflection probes: smooth, between mip levels too.
    probe: wgpu::Sampler,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum MeshId {
    Obj(usize),
    Tile(u64),
    /// An animated model, by its entity's uid: its buffers are rewritten every frame.
    Dyn(u64),
}

struct Bloom {
    levels: usize,
    first: wgpu::BindGroup,
    down: Vec<wgpu::BindGroup>,
    up: Vec<wgpu::BindGroup>,
    views: Vec<wgpu::TextureView>,
}

struct ShadowMap {
    size: u32,
    array_view: wgpu::TextureView,
    layers: Vec<wgpu::TextureView>,
}

/// The size-dependent render targets and the bind groups of the post-processing passes that read them.
struct Targets {
    w: u32,
    h: u32,
    mode: Mode,
    msaa: u32,
    hdr: wgpu::TextureView,
    hdr_ms: Option<wgpu::TextureView>,
    depth: wgpu::TextureView,
    pre_depth: Option<wgpu::TextureView>,
    /// The bounced light and the occlusion, twice: one is made while the other, the last picture's, is the history it is blended with.
    ao: Option<[wgpu::TextureView; 2]>,
    ao_raw: Option<wgpu::TextureView>,
    gi_raw: Option<wgpu::TextureView>,
    bloom: Option<Bloom>,
    bg_resolve: wgpu::BindGroup,
    bg_ssao: Option<wgpu::BindGroup>,
    bg_gi: Option<wgpu::BindGroup>,
    /// Temporal anti-aliasing: the composite's picture, and the two histories that take turns (the bind group of index i writes history i and reads the other).
    taa_in: Option<wgpu::TextureView>,
    taa_hist: Option<[wgpu::TextureView; 2]>,
    bg_taa: Option<[wgpu::BindGroup; 2]>,
    bg_blur: Option<[wgpu::BindGroup; 2]>,
}

/// What a reflection probe was captured of: where it stood and how big its box was (when either changes it is captured again).
#[derive(Clone, Copy, Debug, PartialEq)]
struct ProbeKey {
    uid: u64,
    pos: V3,
    half: V3,
}

/// The captured faces of up to four reflection probes: one array of 24 layers (six per probe), each with a full chain of mip levels.
struct ProbeSet {
    /// The side of a face in texels, and how many mip levels it has.
    size: u32,
    levels: u32,
    array_view: wgpu::TextureView,
    /// By layer, then mip level: one face at one level, to draw into and to read from.
    faces: Vec<Vec<wgpu::TextureView>>,
    baked: [Option<ProbeKey>; MAX_PROBES],
}

/// What the screen-space reflections draw into and read: where each pixel points and how rough it is, how much it reflects, the depth of the nearest surface (see-through ones
/// included), and the layer of reflections the composite adds.
struct SsrTargets {
    w: u32,
    h: u32,
    gbuf_a: wgpu::TextureView,
    gbuf_b: wgpu::TextureView,
    depth: wgpu::TextureView,
    out: wgpu::TextureView,
}

/// The integer picture of which entity covers each pixel, and the depth it is tested with.
struct IdTargets {
    w: u32,
    h: u32,
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    depth: wgpu::TextureView,
}

struct SurfaceState {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

/// One run of identical draws: instances `first .. first + count` of the object list.
#[derive(Clone)]
struct Batch {
    mesh: MeshId,
    tex: TexKey,
    first: u32,
    count: u32,
    cast: bool,
    /// Cut out by the texture's alpha (it needs the texture in the shadow maps and the prepass too).
    cut: bool,
    /// The level of detail (an index into the mesh's `lods`).
    lod: u8,
    /// The passes that draw it: bit 0 the camera's, bits 1 to 3 the cascades; the sprites and clouds, which are not culled, have every bit.
    mask: u8,
    /// A painted terrain: the index of its bind group (splat map and layers) among the picture's `splat_bgs`.
    splat: Option<usize>,
    /// The biggest scale of any object of the batch (a shadow cascade picks its level of detail from it).
    scale: f32,
    /// An opaque mesh with its vertices where its transform puts them (no wind, no waves), not cut out, not a terrain: the early depth pass can draw it, and the main pass then shades only the pixels it won.
    early: bool,
    /// `SceneMesh::depth_role`: 1 only in the depth-only passes, 2 only in the colour passes.
    role: u8,
}

/// Where a picture goes.
enum Out<'a> {
    Image,
    Surface { view: &'a wgpu::TextureView, format: wgpu::TextureFormat },
    /// One face of a reflection probe: the HDR picture as plain light, into this layer of the probes' array.
    Probe { view: &'a wgpu::TextureView },
}

pub struct GpuRenderer {
    pub gpu: Gpu,
    /// Leave out of each pass the meshes whose bounding sphere is wholly outside its view volume (the camera's; a shadow cascade's sides). On by default; off draws everything, which must give the very same picture.
    pub culling: bool,
    /// Draw calls the last picture's passes made (all the passes: the camera's, the cascades', the early depth pass).
    pub last_draws: u32,
    /// Of those, the shadow cascades' and the point lights' shadow faces'.
    pub last_draws_split: [u32; 2],
    /// How many objects the last picture's occlusion culling left out, and how many prisms hid them (`LUNARIS_NO_OCCLUSION` turns it off).
    pub last_occluded: (u32, u32),
    pub occlusion: bool,
    occ_buf: crate::occlusion::OcclusionBuffer,
    /// Occlusion culling pays only where it hides a good share of what the frustum leaves: it is tried every 32 pictures and kept while it hides a quarter or more of the objects it tests (`LUNARIS_OCCLUSION=1` keeps it always on).
    occ_wait: u32,
    occ_always: bool,
    /// Whether the opaque meshes get an early depth pass before the main pass (`LUNARIS_EARLY_Z=1` or `0`; unset: when the picture has enough triangles for it to pay).
    pub early_z: Option<bool>,
    /// Block-compress the pictures sent to the card (BC1, BC3, BC5, BC7: a fourth to an eighth of the memory; on by default when the card can; `LUNARIS_NO_COMPRESS=1` turns it off).
    pub compress: bool,
    /// Where the compressed blocks of the pictures are kept between runs (`<workspace>/.lunaris/cache/textures`, or `LUNARIS_TEXTURE_CACHE=<folder>`; `off` for none): a picture seen again is read from here instead of being compressed again.
    texture_cache: Option<std::path::PathBuf>,
    /// Draw far meshes with their coarser levels of detail (on by default; `LUNARIS_NO_LOD=1` turns it off).
    pub lod: bool,
    /// The screen area (square pixels) each triangle of an object should have at least when a level of detail is chosen (see `lod_triangle_pixels`; 0: the rule is off and only the error counts).
    pub lod_tri_pixels: f32,
    /// The asynchronous mode of a game that is played (off by default: pictures taken by tools and tests wait for everything and are the same every time). A mesh or a picture that is not on the card yet is prepared on other threads
    /// (interleaving, clusters, levels of detail, mips, block compression) while the objects that need it are left out of the pictures, and is copied to the card when it is ready, at most `upload_budget` bytes in a picture (one thing at least).
    /// `LUNARIS_SYNC_UPLOAD=1` turns it off even then.
    pub async_prep: bool,
    pub upload_budget: u64,
    mesh_jobs: HashMap<usize, MeshJob>,
    tex_jobs: HashMap<(usize, bool), TexJob>,
    data_jobs: HashMap<usize, DataJob>,
    partial_meshes: Vec<PartialMesh>,
    partial_textures: Vec<PartialTex>,
    partial_data: Vec<PartialData>,
    plain_jobs: HashMap<usize, PlainJob>,
    partial_plain: Vec<PartialPlain>,
    /// How many triangles the last picture's main view drew (the meshes of the object list at their levels of detail).
    pub last_triangles: u64,
    /// What the point lights' shadow cubes asked for in the last picture: the lights, the casters in their reach, and the objects made for their faces.
    pub last_point_shadows: (usize, usize, usize),
    /// How many objects of the last picture's main view were drawn at each level of detail (the first is the whole mesh).
    pub last_lods: [u32; 10],
    /// The milliseconds each pass of the last picture took on the card (empty unless profiling is on: `LUNARIS_GPU_PROFILE=1`).
    pub last_gpu_ms: Vec<(String, f32)>,
    /// The milliseconds the CPU spent on the last picture, by phase (the same switch).
    pub last_cpu_ms: Vec<(String, f32)>,
    prof: Profiler,
    /// What the last picture did: the meshes in the object list, and how many of them the camera's view volume left out.
    pub last_cull: (u32, u32),
    /// With profiling on: the meshes that put most triangles in the camera's view (triangles of the whole mesh, how many objects use it, the level of detail they are drawn at on average, triangles drawn in all).
    pub last_top_meshes: Vec<(u32, u32, [u32; 10], u64, u32)>,
    shaders: Shaders,
    layouts: Layouts,
    pipes: PipelineCache2,
    samplers: Samplers,
    meshes: HashMap<usize, GpuMesh>,
    tile_meshes: HashMap<u64, GpuMesh>,
    dyn_meshes: HashMap<u64, GpuMesh>,
    /// The vertices of this picture's posed models, laid out for the card on every core before the materials are looked at (`prepare_dynamic_vertices`), by the entity's key; `ensure_dynamic` takes its own.
    dyn_vdata: HashMap<u64, (Vec<f32>, V3, f32, V3)>,
    /// Skinning on the card (`skin.rs`): the pipeline, what the models keep there and this picture's models. Off with `LUNARIS_GPU_SKIN=0` or `set_gpu_skinning(false)`: the processor poses the models then, as before.
    skin: crate::skin::SkinState,
    gpu_skin: bool,
    /// Particle emitters simulated on the card (`fx_gpu.rs`): their buffers and this picture's emitters.
    fx: crate::fx_gpu::FxState,
    textures: HashMap<(usize, bool), GpuTex>,
    splat_textures: HashMap<usize, GpuSplat>,
    tex_bgs: HashMap<TexKey, wgpu::BindGroup>,
    /// Normal and occlusion-roughness-metallic maps: numbers in a plain 8-bit texture (no sRGB), by the address of the picture.
    data_textures: HashMap<usize, GpuTex>,
    /// Group 1 of a painted terrain's draws (it has no colour texture of its own): white, a sampler, a flat relief; made on first use.
    splat_tex_bg: Option<wgpu::BindGroup>,
    /// The pictures of the ground that grass fields stand on, on the card (R32Float, by the picture's address), and the uniform buffers of the rings.
    grass_heights: HashMap<usize, (wgpu::TextureView, Arc<lunaris_sim::terrain::Heightfield>, u64)>,
    grass_bufs: Vec<wgpu::Buffer>,
    /// The depth of the sea (its level minus the ground) over a window round the eye, as the frame uniform carries it (`sea_floor_window`), and which window that is (cell of the eye, level, terrain).
    sea_cache: Option<((i64, i64, u32, usize), Box<[[u32; 4]; 2048]>)>,
    flat_normal: wgpu::TextureView,
    arm_default: wgpu::TextureView,
    flat_height: wgpu::TextureView,
    /// Bind groups of interface pictures: (texture address, filtered), the texture in its display-colour form.
    ui_bgs: HashMap<(usize, bool), wgpu::BindGroup>,
    white: [wgpu::TextureView; 2],
    white_height: wgpu::TextureView,
    frame_buf: wgpu::Buffer,
    objects: wgpu::Buffer,
    objects_cap: u64,
    pass_bufs: Vec<wgpu::Buffer>,
    text_u: wgpu::Buffer,
    text_vb: wgpu::Buffer,
    text_vb_cap: u64,
    text_bg: wgpu::BindGroup,
    shadow: Option<ShadowMap>,
    point_shadow: Option<ShadowMap>,
    shadow_dummy: wgpu::TextureView,
    /// A stand-in for the depth of the opaque things when a picture has none (the water only looks at it when there is one).
    depth_dummy: wgpu::TextureView,
    ao_white: wgpu::TextureView,
    targets: Option<Targets>,
    ssr_targets: Option<SsrTargets>,
    /// A black stand-in for the reflection layer of a picture without reflections.
    ssr_dummy: wgpu::TextureView,
    /// The reflection probes' faces, once there are probes, and a stand-in for a picture without any.
    probe_set: Option<ProbeSet>,
    probe_dummy: wgpu::TextureView,
    /// The view-projection of the previous picture, for motion blur.
    prev_vp: Option<[f32; 16]>,
    fx_frame: u32,
    /// Time for the rain's streaks to fall by (the picture's own clock; nothing in the simulation reads it).
    rain_clock: std::time::Instant,
    /// Which of the two `ao` textures the last picture with bounced light made, and how many pictures the bounce has been blended over.
    gi_flip: usize,
    gi_age: u32,
    /// Temporal anti-aliasing: which history the last picture wrote, how many pictures are blended, the place in the jitter sequence, and the last picture's view-projection without the jitter.
    taa_flip: usize,
    taa_age: u32,
    taa_index: u32,
    prev_unj_vp: Option<[f32; 16]>,
    out_image: Option<(u32, u32, wgpu::Texture)>,
    readback: Option<(u64, wgpu::Buffer)>,
    id_targets: Option<IdTargets>,
    id_readback: Option<(u64, wgpu::Buffer)>,
    /// Entity uids of the last picture, by slot (the id pass writes slot + 1).
    slot_uids: Vec<u64>,
    surface: Option<SurfaceState>,
    frame_no: u64,
    /// How many pictures were presented to the window (a hidden or minimised window skips them).
    pub frames_presented: u64,
    /// The eye adaptation: its state on the card (exposure, average luminance, the exposure aimed at), a copy of it the CPU can read, and the compute pipeline that updates it.
    exposure_buf: wgpu::Buffer,
    exposure_read: wgpu::Buffer,
    ae_pipe: wgpu::ComputePipeline,
    /// The atmosphere integrated for a grid of view directions, once a picture (see `skylut.wgsl`), and what writes it.
    sky_pipe: wgpu::ComputePipeline,
    sky_lut: wgpu::Buffer,
    sky_bg: wgpu::BindGroup,
    /// The last picture adapted (so that the next one can glide from its exposure instead of starting from the measured value).
    ae_valid: bool,
    /// The last picture copied the adaptation state into `exposure_read` (a picture read back as an image does).
    ae_copied: bool,
    /// Seconds a picture drawn into an image advances the eye adaptation by (`None`: it measures and applies at once).
    adapt_dt: Option<f32>,
    /// When the last picture reached the window.
    last_window_pic: Option<std::time::Instant>,
}

/// How long each pass of a picture takes on the card (timestamp queries written at the start and the end of every pass): off unless `LUNARIS_GPU_PROFILE` is set or `profile` is turned on, and it costs nothing then. The
/// times of the last picture are in `GpuRenderer::last_gpu_ms` and in the `measured` numbers of the `render` tool (`gpu_ms.<pass>`, and `gpu_ms.total`).
struct Profiler {
    qs: Option<wgpu::QuerySet>,
    resolve: Option<wgpu::Buffer>,
    read: Option<wgpu::Buffer>,
    /// Nanoseconds per tick of the timestamps.
    period: f32,
    on: bool,
    inside: bool,
    used: std::cell::Cell<u32>,
    labels: std::cell::RefCell<Vec<(String, u32)>>,
    /// The marks inside passes: (pass.label, the stamp before, the stamp of the mark).
    segments: std::cell::RefCell<Vec<(String, u32, u32)>>,
    /// The stamp the next mark counts from, and the name of the pass it is in.
    chain: std::cell::Cell<u32>,
    pass: std::cell::RefCell<String>,
    /// Read the times of a window's pictures a few pictures later instead of waiting for the card (which would make the picture and the card take turns): each picture's stamps go to the next free
    /// buffer of the ring and `collect` takes them back when the card is done.
    ring_mode: std::cell::Cell<bool>,
    ring: std::cell::RefCell<Vec<RingSlot>>,
}

/// One picture's stamps on their way back from the card.
struct RingSlot {
    buf: wgpu::Buffer,
    /// 0: free, 1: the copy is queued, 2: the buffer is being mapped, 3: mapped and ready.
    state: Arc<std::sync::atomic::AtomicU8>,
    n: u32,
    labels: Vec<(String, u32)>,
    segments: Vec<(String, u32, u32)>,
}

const PROFILE_SLOTS: u32 = 128;
const PROFILE_RING: usize = 4;

impl Profiler {
    fn new(gpu: &Gpu) -> Profiler {
        let have = gpu.timestamps;
        let buffer = |label: &str, usage: wgpu::BufferUsages| gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: PROFILE_SLOTS as u64 * 8, usage, mapped_at_creation: false });
        Profiler {
            qs: have.then(|| gpu.device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("profile"), ty: wgpu::QueryType::Timestamp, count: PROFILE_SLOTS })),
            resolve: have.then(|| buffer("profile-resolve", wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC)),
            read: have.then(|| buffer("profile-read", wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST)),
            period: if have { gpu.queue.get_timestamp_period() } else { 0.0 },
            on: have && std::env::var_os("LUNARIS_GPU_PROFILE").is_some(),
            inside: gpu.timestamps_inside,
            ring_mode: std::cell::Cell::new(false),
            ring: std::cell::RefCell::new(if have { (0..PROFILE_RING).map(|_| RingSlot { buf: buffer("profile-ring", wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST), state: Arc::new(std::sync::atomic::AtomicU8::new(0)), n: 0, labels: Vec::new(), segments: Vec::new() }).collect() } else { Vec::new() }),
            used: std::cell::Cell::new(0),
            labels: std::cell::RefCell::new(Vec::new()),
            segments: std::cell::RefCell::new(Vec::new()),
            chain: std::cell::Cell::new(0),
            pass: std::cell::RefCell::new(String::new()),
        }
    }

    /// A new picture: forget the passes of the last one.
    fn begin(&self) {
        self.used.set(0);
        self.labels.borrow_mut().clear();
        self.segments.borrow_mut().clear();
    }

    /// The timestamp writes for a pass called `label` (none when profiling is off or the slots are used up).
    fn writes(&self, label: &str) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        if !self.on {
            return None;
        }
        let i = self.used.get();
        if i + 2 > PROFILE_SLOTS {
            return None;
        }
        self.used.set(i + 2);
        self.labels.borrow_mut().push((label.to_string(), i));
        self.chain.set(i);
        *self.pass.borrow_mut() = label.to_string();
        Some(wgpu::RenderPassTimestampWrites { query_set: self.qs.as_ref()?, beginning_of_pass_write_index: Some(i), end_of_pass_write_index: Some(i + 1) })
    }

    /// A stamp between two draws of the pass being written: the time since the last one (or the start of the pass) is called `label`.
    fn mark(&self, pass: &mut wgpu::RenderPass<'_>, label: &str) {
        let (true, true, Some(qs)) = (self.on, self.inside, &self.qs) else { return };
        let i = self.used.get();
        if i + 1 > PROFILE_SLOTS {
            return;
        }
        self.used.set(i + 1);
        pass.write_timestamp(qs, i);
        self.segments.borrow_mut().push((format!("{}.{label}", self.pass.borrow()), self.chain.get(), i));
        self.chain.set(i);
    }

    /// At the end of the picture's commands: copy the timestamps where they can be read.
    fn resolve(&self, enc: &mut wgpu::CommandEncoder) {
        let n = self.used.get();
        if let (true, true, Some(qs), Some(res), Some(read)) = (self.on, n > 0, &self.qs, &self.resolve, &self.read) {
            enc.resolve_query_set(qs, 0..n, res, 0);
            if self.ring_mode.get() {
                // the next free buffer of the ring (none: this picture's times are lost, the card has not been read for a while)
                let mut ring = self.ring.borrow_mut();
                if let Some(slot) = ring.iter_mut().find(|r| r.state.load(std::sync::atomic::Ordering::Acquire) == 0) {
                    enc.copy_buffer_to_buffer(res, 0, &slot.buf, 0, n as u64 * 8);
                    slot.n = n;
                    slot.labels = self.labels.borrow().clone();
                    slot.segments = self.segments.borrow().clone();
                    slot.state.store(1, std::sync::atomic::Ordering::Release);
                }
            } else {
                enc.copy_buffer_to_buffer(res, 0, read, 0, n as u64 * 8);
            }
        }
    }

    /// The times of the oldest picture the card has finished since the last call (ring mode; never waits).
    fn collect(&self, device: &wgpu::Device) -> Option<Vec<(String, f32)>> {
        use std::sync::atomic::Ordering;
        let _ = device.poll(wgpu::PollType::Poll);
        let mut ring = self.ring.borrow_mut();
        let mut got = None;
        for slot in ring.iter_mut() {
            match slot.state.load(Ordering::Acquire) {
                1 => {
                    // the copy was queued by an earlier picture: ask for the buffer (it comes when the card has run that copy)
                    let st = slot.state.clone();
                    slot.state.store(2, Ordering::Release);
                    slot.buf.slice(..slot.n as u64 * 8).map_async(wgpu::MapMode::Read, move |r| st.store(if r.is_ok() { 3 } else { 0 }, Ordering::Release));
                }
                3 => {
                    let stamps: Vec<u64> = {
                        let Ok(data) = slot.buf.slice(..slot.n as u64 * 8).get_mapped_range() else {
                            slot.state.store(0, Ordering::Release);
                            continue;
                        };
                        data.chunks_exact(8).map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])).collect()
                    };
                    slot.buf.unmap();
                    got = Some(self.decode(&stamps, &slot.labels, &slot.segments));
                    slot.state.store(0, Ordering::Release);
                }
                _ => {}
            }
        }
        got
    }

    /// The milliseconds each pass took (the same name added up), after the card has finished the picture.
    fn read(&self, device: &wgpu::Device) -> Result<Vec<(String, f32)>, String> {
        let n = self.used.get();
        let (true, true, Some(read)) = (self.on, n > 0, &self.read) else { return Ok(Vec::new()) };
        let slice = read.slice(..n as u64 * 8);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("the GPU did not finish: {e}"))?;
        rx.recv().map_err(|_| "the GPU readback was lost".to_string())?.map_err(|e| format!("cannot read the pass times back: {e}"))?;
        let stamps: Vec<u64> = {
            let data = slice.get_mapped_range().map_err(|e| format!("cannot read the pass times back: {e}"))?;
            data.chunks_exact(8).map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])).collect()
        };
        read.unmap();
        Ok(self.decode(&stamps, &self.labels.borrow(), &self.segments.borrow()))
    }

    /// The milliseconds of each pass (the same name added up), and of the marks inside them, from the stamps of one picture.
    fn decode(&self, stamps: &[u64], labels: &[(String, u32)], segments: &[(String, u32, u32)]) -> Vec<(String, f32)> {
        let ticks = |a: u32, b: u32| (stamps[b as usize].saturating_sub(stamps[a as usize]) as f64 * self.period as f64 * 1.0e-6) as f32;
        let mut out: Vec<(String, f32)> = Vec::new();
        for (label, i) in labels.iter() {
            let ms = ticks(*i, *i + 1);
            match out.iter_mut().find(|(l, _)| l == label) {
                Some(e) => e.1 += ms,
                None => out.push((label.clone(), ms)),
            }
        }
        // the marks inside the passes, and what is left of each pass after them
        let mut parts: Vec<(String, f32)> = Vec::new();
        for (label, a, b) in segments.iter() {
            parts.push((label.clone(), ticks(*a, *b)));
        }
        let passes: Vec<String> = parts.iter().filter_map(|(l, _)| l.split_once('.').map(|(p, _)| p.to_string())).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        for p in passes {
            let total = out.iter().find(|(l, _)| *l == p).map_or(0.0, |e| e.1);
            let marked: f32 = parts.iter().filter(|(l, _)| l.starts_with(&format!("{p}."))).map(|e| e.1).sum();
            parts.push((format!("{p}.rest"), (total - marked).max(0.0)));
        }
        out.extend(parts);
        out
    }
}

/// The levels of a mip chain as what goes to the card: the same bytes, or (with `block`) each level as blocks of that format. Also the bytes.
fn pack_levels(levels: Vec<(u32, u32, Vec<u8>)>, block: Option<Block>) -> (Vec<(u32, u32, Vec<u8>)>, u64) {
    let out: Vec<(u32, u32, Vec<u8>)> = match block {
        Some(b) => levels.into_iter().map(|(lw, lh, d)| (lw, lh, compress_level(&d, lw, lh, b))).collect(),
        None => levels,
    };
    let bytes = out.iter().map(|(_, _, d)| d.len() as u64).sum();
    (out, bytes)
}

/// The part of putting a picture on the card that needs no card (so that it can be done on another thread): the bytes, the mips, the block compression (or the blocks read back from the cache).
fn prepare_texture(tex: &Texture, high: bool, smooth: bool, side_limit: u32, compress_on: bool, cache_dir: Option<&std::path::Path>, device: Option<&wgpu::Device>) -> PreparedTex {
    let (mut w, mut h) = (tex.width.max(1), tex.height.max(1));
    let mut bytes = tex.texels.to_rgba8();
    // a picture bigger than the card's biggest texture comes in at the largest level of its own mip chain that fits
    if w > side_limit || h > side_limit {
        let chain = mip_chain(w, h, std::mem::take(&mut bytes), true);
        if let Some((lw, lh, data)) = chain.into_iter().find(|(lw, lh, _)| *lw <= side_limit && *lh <= side_limit) {
            (w, h, bytes) = (lw, lh, data);
        }
    }
    // block-compressed when it is smooth-sampled (the nearest-sampled ones are pixel art), big enough and a multiple of the block size
    let block = (high && smooth && compress_on && w % 4 == 0 && h % 4 == 0 && w.min(h) >= COMPRESS_MIN).then(|| if bytes.chunks_exact(4).all(|p| p[3] == 255) { Block::Bc1 } else { Block::Bc3 });
    let cache_file = block.and_then(|b| cache_dir.map(|d| compress::cache_path(d, b, w, h, compress::hash_bytes(&bytes))));
    let cached = cache_file.as_ref().zip(block).and_then(|(p, b)| compress::load_cached(p, w, h, b));
    let (levels, avg, bytes_on_card) = match cached {
        Some((levels, avg)) => {
            let n = levels.iter().map(|l| l.2.len() as u64).sum();
            (levels, avg, n)
        }
        None => {
            let levels = if high { mip_chain(w, h, bytes, true) } else { vec![(w, h, bytes)] };
            let avg = levels.last().map_or([0.5; 3], |(_, _, d)| [srgb_decode(d[0] as f32 / 255.0), srgb_decode(d[1] as f32 / 255.0), srgb_decode(d[2] as f32 / 255.0)]);
            let (levels, n) = pack_levels(levels, block);
            if let (Some(p), Some(_)) = (&cache_file, block) {
                compress::store_cached(p, &levels, avg);
            }
            (levels, avg, n)
        }
    };
    // the relief of a built-in material (only the high-quality mode uses it)
    let height = match (&tex.heights, high) {
        (Some(hm), true) if hm.len() == (w * h) as usize => {
            let packed: Vec<u8> = hm.iter().flat_map(|v| { let b = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8; [b, b, b, 255] }).collect();
            let chain: Vec<(u32, u32, Vec<u8>)> = mip_chain(w, h, packed, false).into_iter().map(|(lw, lh, d)| (lw, lh, d.chunks_exact(4).map(|p| p[0]).collect())).collect();
            Some(chain)
        }
        _ => None,
    };
    let textures = device.map(|d| {
        let format = match block {
            Some(Block::Bc1) => wgpu::TextureFormat::Bc1RgbaUnormSrgb,
            Some(_) => wgpu::TextureFormat::Bc3RgbaUnormSrgb,
            None if high => wgpu::TextureFormat::Rgba8UnormSrgb,
            None => wgpu::TextureFormat::Rgba8Unorm,
        };
        let colour = texture_object(d, "texture", w, h, levels.len() as u32, format);
        let relief = height.as_ref().map(|chain| texture_object(d, "height", w, h, chain.len() as u32, wgpu::TextureFormat::R8Unorm));
        (colour, relief)
    });
    PreparedTex { textures, w, h, levels, avg, block, bytes: bytes_on_card, height }
}

/// The part of putting a splat map or a heightmap on the card that needs no card: the bytes (a splat map thinned to the card's largest texture), and the texture when it is given the device.
fn prepare_plain(src: &PlainSrc, limit: u32, device: Option<&wgpu::Device>) -> PreparedPlain {
    let (w, h, bytes, format) = match src {
        PlainSrc::Splat(sp) => {
            let limit = limit as usize;
            let step = sp.cols.max(sp.rows).div_ceil(limit).max(1);
            let (w, h) = (sp.cols.div_ceil(step), sp.rows.div_ceil(step));
            let mut bytes = Vec::with_capacity(w * h * 4);
            for y in 0..h {
                let row = (y * step).min(sp.rows - 1) * sp.cols;
                for x in 0..w {
                    bytes.extend_from_slice(&sp.texels[row + (x * step).min(sp.cols - 1)]);
                }
            }
            (w as u32, h as u32, bytes, wgpu::TextureFormat::Rgba8Unorm)
        }
        PlainSrc::Heights(f) => {
            let data: Vec<f32> = f.samples().iter().map(|v| *v as f32 * (1.0 / 65535.0)).collect();
            (f.cols as u32, f.rows as u32, bytemuck::cast_slice(&data).to_vec(), wgpu::TextureFormat::R32Float)
        }
    };
    let texture = device.map(|d| texture_object(d, "plain-texture", w, h, 1, format));
    PreparedPlain { texture, w, h, bytes, format }
}

/// A texture on the card with `levels` mips, that can be copied into.
fn texture_object(device: &wgpu::Device, label: &str, w: u32, h: u32, levels: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// A buffer on the card of at least `len` bytes (a multiple of four), that can be copied into.
fn mesh_buffer(device: &wgpu::Device, label: &str, len: usize, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    // an empty buffer is not allowed to be mapped at creation; pad to one element
    let size = (len.max(4) as u64 + 3) & !3;
    device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
}

/// `prepare_texture` for a normal or occlusion-roughness-metallic map.
fn prepare_data_texture(tex: &Texture, kind: DataKind, compress_on: bool, cache_dir: Option<&std::path::Path>, device: Option<&wgpu::Device>) -> PreparedData {
    let (w, h) = (tex.width.max(1), tex.height.max(1));
    let mut bytes = tex.texels.to_rgba8();
    for p in bytes.chunks_exact_mut(4) {
        p[3] = 255;
    }
    // a normal map is two numbers a texel (BC5; the shader makes the third), a roughness-metallic map four (BC7); a displacement map stays whole
    let block = (compress_on && w % 4 == 0 && h % 4 == 0 && w.min(h) >= COMPRESS_MIN).then_some(match kind {
        DataKind::Normal => Some(Block::Bc5),
        DataKind::Arm => Some(Block::Bc7),
        DataKind::Disp => None,
    });
    let block = block.flatten();
    let cache_file = block.and_then(|b| cache_dir.map(|d| compress::cache_path(d, b, w, h, compress::hash_bytes(&bytes))));
    let (levels, bytes_on_card) = match cache_file.as_ref().zip(block).and_then(|(p, b)| compress::load_cached(p, w, h, b)) {
        Some((levels, _)) => {
            let n = levels.iter().map(|l| l.2.len() as u64).sum();
            (levels, n)
        }
        None => {
            let packed = pack_levels(mip_chain(w, h, bytes, false), block);
            if let (Some(p), Some(_)) = (&cache_file, block) {
                compress::store_cached(p, &packed.0, [0.5; 3]);
            }
            packed
        }
    };
    let texture = device.map(|d| {
        let format = match block {
            Some(Block::Bc5) => wgpu::TextureFormat::Bc5RgUnorm,
            Some(_) => wgpu::TextureFormat::Bc7RgbaUnorm,
            None => wgpu::TextureFormat::Rgba8Unorm,
        };
        texture_object(d, "data-map", w, h, levels.len() as u32, format)
    });
    PreparedData { texture, w, h, levels, block, bytes: bytes_on_card }
}

fn depth_pass<'e>(enc: &'e mut wgpu::CommandEncoder, prof: &Profiler, label: &str, view: &wgpu::TextureView, clear: f32) -> wgpu::RenderPass<'e> {
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store }), stencil_ops: None }),
        timestamp_writes: prof.writes(label),
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

fn bytes_of_floats(v: &[f32]) -> &[u8] {
    bytemuck::cast_slice(v)
}

impl GpuRenderer {
    /// A renderer that draws into images only.
    pub fn headless() -> Result<GpuRenderer, String> {
        GpuRenderer::with_gpu(Gpu::headless()?, None)
    }

    /// A renderer that draws into a window (anything wgpu can make a surface of, for instance an `Arc<winit::window::Window>`).
    pub fn for_window(target: impl Into<wgpu::SurfaceTarget<'static>>, width: u32, height: u32, vsync: bool) -> Result<GpuRenderer, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(target).map_err(|e| format!("cannot make a surface for the window ({e})"))?;
        let gpu = Gpu::with_instance(instance, Some(&surface))?;
        let caps = surface.get_capabilities(&gpu.adapter);
        let mut config = surface.get_default_config(&gpu.adapter, width.max(1), height.max(1)).ok_or("the window cannot be drawn on by this graphics adapter")?;
        if let Some(f) = caps.formats.iter().copied().find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm)) {
            config.format = f;
        }
        config.present_mode = if vsync { wgpu::PresentMode::Fifo } else if caps.present_modes.contains(&wgpu::PresentMode::Immediate) { wgpu::PresentMode::Immediate } else { wgpu::PresentMode::AutoNoVsync };
        config.desired_maximum_frame_latency = 2;
        surface.configure(&gpu.device, &config);
        GpuRenderer::with_gpu(gpu, Some(SurfaceState { surface, config }))
    }

    fn with_gpu(gpu: Gpu, surface: Option<SurfaceState>) -> Result<GpuRenderer, String> {
        let device = &gpu.device;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shaders = Shaders::new(device);
        let layouts = Layouts::new(device);
        let samplers = Samplers {
            nearest_repeat: device.create_sampler(&wgpu::SamplerDescriptor { label: Some("nearest-repeat"), address_mode_u: wgpu::AddressMode::Repeat, address_mode_v: wgpu::AddressMode::Repeat, mag_filter: wgpu::FilterMode::Nearest, min_filter: wgpu::FilterMode::Nearest, mipmap_filter: wgpu::MipmapFilterMode::Nearest, ..Default::default() }),
            nearest_clamp: device.create_sampler(&wgpu::SamplerDescriptor { label: Some("nearest-clamp"), address_mode_u: wgpu::AddressMode::ClampToEdge, address_mode_v: wgpu::AddressMode::ClampToEdge, mag_filter: wgpu::FilterMode::Nearest, min_filter: wgpu::FilterMode::Nearest, mipmap_filter: wgpu::MipmapFilterMode::Nearest, ..Default::default() }),
            aniso: device.create_sampler(&wgpu::SamplerDescriptor { label: Some("aniso"), address_mode_u: wgpu::AddressMode::Repeat, address_mode_v: wgpu::AddressMode::Repeat, mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::MipmapFilterMode::Linear, anisotropy_clamp: 16, ..Default::default() }),
            aniso_ground: device.create_sampler(&wgpu::SamplerDescriptor { label: Some("aniso-ground"), address_mode_u: wgpu::AddressMode::Repeat, address_mode_v: wgpu::AddressMode::Repeat, mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::MipmapFilterMode::Linear, anisotropy_clamp: 8, ..Default::default() }),
            linear_clamp: device.create_sampler(&wgpu::SamplerDescriptor { label: Some("linear-clamp"), address_mode_u: wgpu::AddressMode::ClampToEdge, address_mode_v: wgpu::AddressMode::ClampToEdge, mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::MipmapFilterMode::Nearest, ..Default::default() }),
            shadow: device.create_sampler(&wgpu::SamplerDescriptor { label: Some("shadow"), address_mode_u: wgpu::AddressMode::ClampToEdge, address_mode_v: wgpu::AddressMode::ClampToEdge, mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::MipmapFilterMode::Nearest, compare: Some(wgpu::CompareFunction::LessEqual), ..Default::default() }),
            probe: device.create_sampler(&wgpu::SamplerDescriptor { label: Some("probe"), address_mode_u: wgpu::AddressMode::ClampToEdge, address_mode_v: wgpu::AddressMode::ClampToEdge, mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::MipmapFilterMode::Linear, ..Default::default() }),
        };
        let buf = |label: &str, size: u64, usage: wgpu::BufferUsages| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false });
        let frame_buf = buf("frame", std::mem::size_of::<FrameU>() as u64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let objects_cap = 256u64;
        let objects = buf("objects", objects_cap * std::mem::size_of::<ObjectData>() as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        let pass_bufs: Vec<wgpu::Buffer> = (0..4 + 6 * POINT_SHADOWS).map(|i| buf(&format!("pass{i}"), 64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST)).collect();
        let text_u = buf("text-uniform", 16, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let text_vb_cap = 4096u64;
        let text_vb = buf("text-vertices", text_vb_cap * 32, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST);

        // 1 x 1 stand-ins: white textures for "no texture", a lit shadow array, an unoccluded AO
        let make_tex = |label: &str, w: u32, h: u32, layers: u32, format: wgpu::TextureFormat, usage: wgpu::TextureUsages| device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: layers }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] });
        let white_tex = |label: &str, format: wgpu::TextureFormat| {
            let t = make_tex(label, 1, 1, 1, format, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
            gpu.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &[255, 255, 255, 255], wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) }, wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 });
            t.create_view(&wgpu::TextureViewDescriptor::default())
        };
        let white = [white_tex("white-classic", wgpu::TextureFormat::Rgba8Unorm), white_tex("white-high", wgpu::TextureFormat::Rgba8UnormSrgb)];
        let white_height = {
            let t = make_tex("white-height", 1, 1, 1, wgpu::TextureFormat::R8Unorm, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
            gpu.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &[128], wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1), rows_per_image: Some(1) }, wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 });
            t.create_view(&wgpu::TextureViewDescriptor::default())
        };
        let flat_normal = {
            let t = make_tex("flat-normal", 1, 1, 1, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
            gpu.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &[128, 128, 255, 255], wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) }, wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 });
            t.create_view(&wgpu::TextureViewDescriptor::default())
        };
        let arm_default = {
            let t = make_tex("arm-default", 1, 1, 1, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
            gpu.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &[255, 200, 0, 255], wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) }, wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 });
            t.create_view(&wgpu::TextureViewDescriptor::default())
        };
        let flat_height = {
            let t = make_tex("flat-height", 1, 1, 1, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
            gpu.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &[128, 128, 128, 255], wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) }, wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 });
            t.create_view(&wgpu::TextureViewDescriptor::default())
        };
        let ao_white = {
            let t = make_tex("ao-white", 1, 1, 1, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
            gpu.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &[0, 0, 0, 255], wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) }, wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 });
            t.create_view(&wgpu::TextureViewDescriptor::default())
        };
        let probe_dummy = make_tex("probe-dummy", 1, 1, 1, HDR, wgpu::TextureUsages::TEXTURE_BINDING).create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let ssr_dummy = make_tex("ssr-dummy", 1, 1, 1, HDR, wgpu::TextureUsages::TEXTURE_BINDING).create_view(&wgpu::TextureViewDescriptor::default());
        let depth_dummy = make_tex("depth-dummy", 1, 1, 1, DEPTH, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&wgpu::TextureViewDescriptor::default());
        let shadow_dummy = make_tex("shadow-dummy", 1, 1, 1, DEPTH, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });

        // the font: one texel per font pixel, 95 glyphs of 5 x 8 side by side; 255 where the pixel is lit
        let (gw, gh) = (font::GLYPH_W, font::GLYPH_H);
        let mut data = vec![0u8; 95 * gw * gh];
        for i in 0..95usize {
            let g = font::glyph((32 + i as u8) as char);
            for row in 0..gh {
                for col in 0..gw {
                    if font::lit(g, col, row) {
                        data[row * 95 * gw + i * gw + col] = 255;
                    }
                }
            }
        }
        let atlas_tex = make_tex("font", (95 * gw) as u32, gh as u32, 1, wgpu::TextureFormat::R8Unorm, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        gpu.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &atlas_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &data, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some((95 * gw) as u32), rows_per_image: Some(gh as u32) }, wgpu::Extent3d { width: (95 * gw) as u32, height: gh as u32, depth_or_array_layers: 1 });
        let atlas = atlas_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let text_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("text"),
            layout: &layouts.text0,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: text_u.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&atlas) }, wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&samplers.nearest_clamp) }],
        });
        let prof = Profiler::new(&gpu);
        let exposure_buf = buf("exposure", 16, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST);
        let exposure_read = buf("exposure-read", 16, wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
        let ae_pipe = build_exposure(device, &shaders, &layouts);
        let sky_pipe = build_sky_lut(device, &shaders, &layouts);
        let skin_pipe = build_skin(device, &shaders, &layouts);
        let (fx_step, fx_emit) = build_fx(device, &shaders, &layouts);
        let sky_lut = buf("sky-lut", 64 * 64 * 2 * 16, wgpu::BufferUsages::STORAGE);
        let sky_bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("sky-lut"), layout: &layouts.sky_lut0, entries: &[wgpu::BindGroupEntry { binding: 0, resource: frame_buf.as_entire_binding() }, wgpu::BindGroupEntry { binding: 14, resource: sky_lut.as_entire_binding() }] });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("the renderer's resources are not valid on this GPU: {e}"));
        }
        let bc_ok = gpu.bc;
        Ok(GpuRenderer {
            gpu,
            culling: std::env::var_os("LUNARIS_NO_CULL").is_none(),
            last_draws: 0,
            last_draws_split: [0; 2],
            last_occluded: (0, 0),
            occlusion: std::env::var_os("LUNARIS_NO_OCCLUSION").is_none(),
            occ_buf: crate::occlusion::OcclusionBuffer::new(),
            occ_wait: 0,
            occ_always: std::env::var("LUNARIS_OCCLUSION").is_ok_and(|v| v == "1"),
            early_z: std::env::var("LUNARIS_EARLY_Z").ok().map(|v| v != "0"),
            compress: bc_ok && std::env::var_os("LUNARIS_NO_COMPRESS").is_none(),
            texture_cache: None,
            lod: std::env::var_os("LUNARIS_NO_LOD").is_none(),
            lod_tri_pixels: lod_triangle_pixels(),
            async_prep: false,
            upload_budget: std::env::var("LUNARIS_UPLOAD_BUDGET_MB").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(12) << 20,
            mesh_jobs: HashMap::new(),
            tex_jobs: HashMap::new(),
            data_jobs: HashMap::new(),
            partial_meshes: Vec::new(),
            partial_textures: Vec::new(),
            partial_data: Vec::new(),
            plain_jobs: HashMap::new(),
            partial_plain: Vec::new(),
            last_triangles: 0,
            last_point_shadows: (0, 0, 0),
            last_lods: [0; 10],
            last_gpu_ms: Vec::new(),
            last_cpu_ms: Vec::new(),
            prof,
            last_cull: (0, 0),
            last_top_meshes: Vec::new(),
            shaders,
            layouts,
            pipes: PipelineCache2::new(),
            samplers,
            meshes: HashMap::new(),
            tile_meshes: HashMap::new(),
            dyn_meshes: HashMap::new(),
            dyn_vdata: HashMap::new(),
            skin: crate::skin::SkinState::new(skin_pipe),
            fx: crate::fx_gpu::FxState::new(fx_step, fx_emit),
            gpu_skin: std::env::var_os("LUNARIS_GPU_SKIN").is_none_or(|v| v != "0"),
            textures: HashMap::new(),
            splat_textures: HashMap::new(),
            tex_bgs: HashMap::new(),
            data_textures: HashMap::new(),
            splat_tex_bg: None,
            grass_heights: HashMap::new(),
            grass_bufs: Vec::new(),
            sea_cache: None,
            flat_normal,
            arm_default,
            flat_height,
            ui_bgs: HashMap::new(),
            white,
            white_height,
            frame_buf,
            objects,
            objects_cap,
            pass_bufs,
            text_u,
            text_vb,
            text_vb_cap,
            text_bg,
            shadow: None,
            point_shadow: None,
            shadow_dummy,
            depth_dummy,
            ao_white,
            targets: None,
            ssr_targets: None,
            ssr_dummy,
            probe_set: None,
            probe_dummy,
            prev_vp: None,
            fx_frame: 0,
            rain_clock: std::time::Instant::now(),
            gi_flip: 0,
            gi_age: 0,
            taa_flip: 0,
            taa_age: 0,
            taa_index: 0,
            prev_unj_vp: None,
            out_image: None,
            readback: None,
            id_targets: None,
            id_readback: None,
            slot_uids: Vec::new(),
            surface,
            frame_no: 0,
            frames_presented: 0,
            exposure_buf,
            exposure_read,
            ae_pipe,
            sky_pipe,
            sky_lut,
            sky_bg,
            ae_valid: false,
            ae_copied: false,
            adapt_dt: None,
            last_window_pic: None,
        })
    }

    pub fn adapter_name(&self) -> String {
        self.gpu.name()
    }

    /// Whether the picture is drawn by a software adapter (the processor under a graphics API). Measured on Mesa's llvmpipe: a 960 x 540 picture of 1,600 meshes takes 210 ms there, against 29 to 48 ms for the engine's own CPU renderer (`docs/PERFORMANCE.md`).
    pub fn adapter_is_software(&self) -> bool {
        self.gpu.is_software()
    }

    /// Time every pass of the pictures that follow on the card (what `LUNARIS_GPU_PROFILE` does from the start): their `info` then holds `gpu_ms.*` and the CPU laps. A card without timestamp queries ignores it.
    pub fn set_profile(&mut self, on: bool) {
        self.prof.on = on && self.prof.qs.is_some();
    }

    /// How the eye adaptation (`AutoExposure`) treats the pictures drawn into images (`render_to_image`, `render_frame`): `None` (the default) measures the picture and
    /// applies the exposure at once, so that the same world always draws the same picture (what a tool wants); `Some(seconds)` treats every picture as that many seconds after
    /// the one before and glides from its exposure (a test, or a program that films the world into images). A window always follows the clock.
    /// Skin the animated models on the card (the default) or on the processor.
    pub fn set_gpu_skinning(&mut self, on: bool) {
        self.gpu_skin = on;
    }

    pub fn gpu_skinning(&self) -> bool {
        self.gpu_skin
    }

    pub fn set_adaptation_step(&mut self, seconds: Option<f32>) {
        self.adapt_dt = seconds.filter(|s| s.is_finite() && *s >= 0.0);
    }

    // ------------------------------------------------------------------------------------------ pipelines

    fn need(&mut self, id: PipeId, samples: u32, format: Option<wgpu::TextureFormat>) -> Result<(), String> {
        let key = PipeKey { id, samples, format };
        if !self.pipes.contains(&key) {
            let scope = self.gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let p = build(&self.gpu.device, &self.shaders, &self.layouts, key);
            if let Some(e) = pollster::block_on(scope.pop()) {
                return Err(format!("the {id:?} pipeline did not build: {e}"));
            }
            self.pipes.insert(key, p);
        }
        Ok(())
    }

    fn pipe(&self, id: PipeId, samples: u32, format: Option<wgpu::TextureFormat>) -> &wgpu::RenderPipeline {
        self.pipes.get(&PipeKey { id, samples, format })
    }

    // ------------------------------------------------------------------------------------------ geometry and textures

    /// The interleaved vertex data of a mesh and its bounding sphere (centre, radius).
    fn vertex_data(mesh: &Mesh) -> (Vec<f32>, V3, f32, V3) {
        let n = mesh.pos.len();
        let mut v: Vec<f32> = Vec::with_capacity(n * 12);
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for i in 0..n {
            v.extend_from_slice(&mesh.pos[i]);
            v.extend_from_slice(&mesh.nrm[i]);
            v.extend_from_slice(&mesh.uv[i]);
            v.extend_from_slice(&mesh.col.get(i).copied().unwrap_or([1.0, 1.0, 1.0]));
            v.push(mesh.texw.get(i).copied().unwrap_or(1.0));
            for k in 0..3 {
                lo[k] = lo[k].min(mesh.pos[i][k]);
                hi[k] = hi[k].max(mesh.pos[i][k]);
            }
        }
        let center = if n == 0 { [0.0; 3] } else { [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5] };
        let radius = mesh.pos.iter().map(|p| ((p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2) + (p[2] - center[2]).powi(2)).sqrt()).fold(0.0f32, f32::max);
        let half = if n == 0 { [0.0; 3] } else { [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5, (hi[2] - lo[2]) * 0.5] };
        (v, center, radius, half)
    }

    /// The part of putting a mesh on the card that needs no card (so that it can be done on another thread): the vertices, the clusters, the levels of detail.
    fn prepare_mesh(mesh: &Mesh, device: Option<&wgpu::Device>) -> PreparedMesh {
        let n_verts = mesh.pos.len();
        let t_data = lunaris_render::Timed::new(|| format!("mesh of {n_verts} vertices: vertex data"));
        let (v, center, radius, half) = Self::vertex_data(mesh);
        drop(t_data);
        // a big mesh is cut into clusters (an animated one, whose vertices move, is not)
        let t_clusters = lunaris_render::Timed::new(|| format!("mesh of {n_verts} vertices: clusters"));
        let (mut indices, clusters) = match make_clusters(mesh).filter(|_| mesh.rig.is_none()) {
            Some((i, c)) => (i, c),
            None => (mesh.idx.clone(), Vec::new()),
        };
        drop(t_clusters);
        // the coarser versions follow the whole one in the same index buffer
        let mut lods = vec![LodRange { first: 0, count: mesh.idx.len() as u32, cell: 0.0 }];
        let t_lods = lunaris_render::Timed::new(|| format!("mesh of {n_verts} vertices: levels of detail"));
        for (idx, cell) in make_lods(mesh) {
            lods.push(LodRange { first: indices.len() as u32, count: idx.len() as u32, cell });
            indices.extend_from_slice(&idx);
        }
        drop(t_lods);
        if std::env::var_os("LUNARIS_LOD_REPORT").is_some() {
            eprintln!("mesh of {} vertices: triangles per level {:?}, cells {:?}, radius {:.2}", mesh.pos.len(), lods.iter().map(|l| l.count / 3).collect::<Vec<_>>(), lods.iter().map(|l| l.cell).collect::<Vec<_>>(), radius);
        }
        let min_lod = if mesh.lod == lunaris_render::mesh::LodMode::Full { min_lod_of(&lods) } else { 0 };
        let buffers = device.map(|d| (mesh_buffer(d, "mesh-vertices", v.len() * 4, wgpu::BufferUsages::VERTEX), mesh_buffer(d, "mesh-indices", indices.len() * 4, wgpu::BufferUsages::INDEX)));
        PreparedMesh { buffers, min_lod, v, center, radius, indices, clusters, lods, half }
    }

    fn upload_mesh(&self, mesh: &Mesh, keep: Option<std::sync::Weak<Mesh>>, frame: u64) -> GpuMesh {
        self.finish_mesh(Self::prepare_mesh(mesh, None), mesh, keep, frame)
    }

    /// The prepared mesh's buffers on the card, empty: `(gpu mesh, vertex bytes, index bytes)`.
    fn make_mesh(&self, p: PreparedMesh, mesh: &Mesh, keep: Option<std::sync::Weak<Mesh>>, frame: u64) -> (GpuMesh, Vec<f32>, Vec<u32>) {
        let PreparedMesh { buffers, min_lod, v, center, radius, indices, clusters, lods, half } = p;
        let device = &self.gpu.device;
        let (vbuf, ibuf) = buffers.unwrap_or_else(|| (mesh_buffer(device, "mesh-vertices", v.len() * 4, wgpu::BufferUsages::VERTEX), mesh_buffer(device, "mesh-indices", indices.len() * 4, wgpu::BufferUsages::INDEX)));
        let g = GpuMesh {
            vbuf,
            ibuf,
            count: mesh.idx.len() as u32,
            clusters,
            lods,
            min_lod,
            use_vcol: !mesh.col.is_empty(),
            texture: mesh.tex.clone(),
            normal: mesh.normal_tex.clone(),
            arm: mesh.arm_tex.clone(),
            cutoff: mesh.alpha_cutoff,
            _keep: keep,
            last_used: frame,
            center,
            radius,
            half,
            skin: None,
        };
        (g, v, indices)
    }

    /// The prepared mesh's buffers on the card, filled at once.
    fn finish_mesh(&self, p: PreparedMesh, mesh: &Mesh, keep: Option<std::sync::Weak<Mesh>>, frame: u64) -> GpuMesh {
        let (g, v, indices) = self.make_mesh(p, mesh, keep, frame);
        if !v.is_empty() {
            self.gpu.queue.write_buffer(&g.vbuf, 0, bytes_of_floats(&v));
        }
        if !indices.is_empty() {
            self.gpu.queue.write_buffer(&g.ibuf, 0, bytemuck::cast_slice(&indices));
        }
        g
    }

    /// Start preparing the meshes and pictures of the models the cache holds (read ahead of need by a `StreamZone`'s `prefetch`) for the card, so that a piece of the world that is about to load has its buffers and textures
    /// when its objects first show up, and does not leave a hole for the time they take (asynchronous mode only).
    fn prepare_loaded(&mut self, models: &[Arc<Mesh>], high: bool, frame: u64) {
        for m in models {
            let whole = std::slice::from_ref(m);
            let parts: &[Arc<Mesh>] = if m.parts.is_empty() { whole } else { &m.parts };
            if let Some(w) = &m.depth_whole {
                // the mesh of the solid pieces together stands in for them in the depth-only passes (see `SceneMesh::depth_role`)
                self.mesh_ready(&MeshRef::Shared(w.clone()), frame);
            }
            for p in parts {
                self.mesh_ready(&MeshRef::Shared(p.clone()), frame);
                if high {
                    if let Some(t) = &p.tex {
                        self.texture_ready(t, true, true, frame);
                    }
                    if let Some(t) = &p.normal_tex {
                        self.data_ready(t, DataKind::Normal, frame);
                    }
                    if let Some(t) = &p.arm_tex {
                        self.data_ready(t, DataKind::Arm, frame);
                    }
                }
            }
        }
    }

    /// Copy the next slices of the meshes and pictures that are on their way to the card, as much as the picture's budget allows (asynchronous mode).
    fn pump_uploads(&mut self, frame: u64) {
        let mut budget = self.upload_budget;
        // meshes first (a picture's object waits for both), in the order they were started
        let mut i = 0;
        while i < self.partial_meshes.len() && budget > 0 {
            let pm = &mut self.partial_meshes[i];
            let vbytes: &[u8] = bytes_of_floats(&pm.v);
            let take = |left: usize, budget: &mut u64| -> usize {
                let n = left.min((*budget as usize).max(4096) & !3);
                *budget = budget.saturating_sub(n as u64);
                n
            };
            let n = take(vbytes.len() - pm.v_done, &mut budget);
            if n > 0 {
                self.gpu.queue.write_buffer(&pm.gpu.vbuf, pm.v_done as u64, &vbytes[pm.v_done..pm.v_done + n]);
                pm.v_done += n;
            }
            let ibytes: &[u8] = bytemuck::cast_slice(&pm.indices);
            let n = take(ibytes.len() - pm.i_done, &mut budget);
            if n > 0 {
                self.gpu.queue.write_buffer(&pm.gpu.ibuf, pm.i_done as u64, &ibytes[pm.i_done..pm.i_done + n]);
                pm.i_done += n;
            }
            if pm.v_done >= vbytes.len() && pm.i_done >= ibytes.len() {
                let done = self.partial_meshes.remove(i);
                let mut g = done.gpu;
                g.last_used = frame;
                self.meshes.insert(done.key, g);
            } else {
                i += 1;
            }
        }
        let mut i = 0;
        while i < self.partial_textures.len() && budget > 0 {
            let pt = &mut self.partial_textures[i];
            let mut finished = pt.colour.step(&self.gpu.queue, &mut budget);
            if let Some(h) = pt.height.as_mut() {
                finished &= if budget > 0 || h.level >= h.levels.len() { h.step(&self.gpu.queue, &mut budget) } else { false };
            }
            if finished {
                let done = self.partial_textures.remove(i);
                let mut g = done.gpu;
                g.last_used = frame;
                self.textures.insert(done.key, g);
            } else {
                i += 1;
            }
        }
        let mut i = 0;
        while i < self.partial_plain.len() && budget > 0 {
            if self.partial_plain[i].up.step(&self.gpu.queue, &mut budget) {
                let done = self.partial_plain.remove(i);
                self.finish_plain(done, frame);
            } else {
                i += 1;
            }
        }
        let mut i = 0;
        while i < self.partial_data.len() && budget > 0 {
            let pd = &mut self.partial_data[i];
            if pd.up.step(&self.gpu.queue, &mut budget) {
                let done = self.partial_data.remove(i);
                let mut g = done.gpu;
                g.last_used = frame;
                self.data_textures.insert(done.key, g);
            } else {
                i += 1;
            }
        }
    }

    /// Whether the mesh of `m` is on the card, in the asynchronous mode starting its preparation on another thread when it is not and putting it on the card when it is ready. Everything that does not come
    /// from a file (a built-in shape, an animated model) and everything outside the mode is made on the spot, so for those the answer is yes.
    fn mesh_ready(&mut self, m: &MeshRef, frame: u64) -> bool {
        let MeshRef::Shared(a) = m else { return true };
        let key = Arc::as_ptr(a) as usize;
        if let Some(g) = self.meshes.get_mut(&key) {
            g.last_used = frame;
            return true;
        }
        if !self.async_prep {
            return true;
        }
        if self.partial_meshes.iter().any(|p| p.key == key) {
            return false;
        }
        let device = self.gpu.device.clone();
        let job = self.mesh_jobs.entry(key).or_insert_with(|| {
            let mesh = a.clone();
            MeshJob { pending: lunaris_render::bg::Pool::shared().spawn({ let (mesh, device) = (mesh.clone(), device.clone()); move || Self::prepare_mesh(&mesh, Some(&device)) }), ready: None, _mesh: mesh, asked: frame }
        });
        job.asked = frame;
        if job.ready.is_none() {
            job.ready = job.pending.try_take();
        }
        if job.ready.is_none() {
            return false;
        }
        // prepared: the buffers are made now and filled over the next pictures
        let p = self.mesh_jobs.remove(&key).and_then(|mut j| j.ready.take()).expect("ready");
        let (gpu, v, indices) = self.make_mesh(p, a, Some(Arc::downgrade(a)), frame);
        self.partial_meshes.push(PartialMesh { key, gpu, v, indices, v_done: 0, i_done: 0, _mesh: a.clone() });
        false
    }

    /// An animated model: reuse the buffers of the last frame when the pose has the same size, so that a moving character costs a
    /// write and not an allocation.
    /// Lay out the vertices of the picture's posed models for the card on several threads (a crowd of armed mannequins is 1.1 MB of vertices each, 0.2 ms of one core: docs/20-puppet-lod.md of the game),
    /// then `ensure_dynamic` only has to upload them. Each is a pure function of its mesh, so the picture is the same whatever the number of threads; with fewer than three models the calling thread does it all.
    fn prepare_dynamic_vertices(&mut self, meshes: &[lunaris_render::render::SceneMesh]) {
        self.dyn_vdata.clear();
        let dyns: Vec<(u64, &Arc<Mesh>)> = meshes
            .iter()
            .filter_map(|m| match &m.mesh {
                MeshRef::Dynamic { key, mesh } => Some((*key, mesh)),
                _ => None,
            })
            .collect();
        if dyns.len() < 3 {
            return;
        }
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(8).min(dyns.len());
        let slots: Vec<std::sync::Mutex<Option<(Vec<f32>, V3, f32, V3)>>> = dyns.iter().map(|_| std::sync::Mutex::new(None)).collect();
        let next = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((_, mesh)) = dyns.get(i) else { break };
                    *slots[i].lock().expect("a vertex thread panicked") = Some(Self::vertex_data(mesh));
                });
            }
        });
        for ((key, _), slot) in dyns.iter().zip(slots) {
            self.dyn_vdata.insert(*key, slot.into_inner().expect("a vertex thread panicked").expect("every model was laid out"));
        }
    }

    fn ensure_dynamic(&mut self, key: u64, mesh: &Arc<Mesh>, frame: u64) -> MeshId {
        let (v, center, radius, half) = self.dyn_vdata.remove(&key).unwrap_or_else(|| Self::vertex_data(mesh));
        let same = self.dyn_meshes.get(&key).is_some_and(|g| g.count as usize == mesh.idx.len() && g.vbuf.size() == (((v.len() * 4).max(4) as u64 + 3) & !3));
        if same {
            let g = self.dyn_meshes.get_mut(&key).expect("checked");
            self.gpu.queue.write_buffer(&g.vbuf, 0, bytes_of_floats(&v));
            self.gpu.queue.write_buffer(&g.ibuf, 0, bytemuck::cast_slice(&mesh.idx));
            g.center = center;
            g.radius = radius;
            g.half = half;
            g.texture = mesh.tex.clone();
            g.normal = mesh.normal_tex.clone();
            g.arm = mesh.arm_tex.clone();
            g.cutoff = mesh.alpha_cutoff;
            g.use_vcol = !mesh.col.is_empty();
            g.last_used = frame;
        } else {
            let g = self.upload_mesh(mesh, Some(Arc::downgrade(mesh)), frame);
            self.dyn_meshes.insert(key, g);
        }
        MeshId::Dyn(key)
    }

    /// An emitter the card simulates (`fx_gpu.rs`): its vertex buffer is the one the card fills, so all that is done here is to make the buffers when the emitter is new and to stage its steps for this picture.
    fn ensure_fx_mesh(&mut self, job: &Arc<lunaris_render::fx_sim::GpuEmitterJob>, frame: u64) -> MeshId {
        let (vbuf, ibuf, fresh) = self.fx.ensure(&self.gpu.device, &self.gpu.queue, job, frame);
        let center = job.model.transform_point([0.0; 3]);
        let count = job.capacity.max(1) * 6;
        if fresh || !self.dyn_meshes.contains_key(&job.key) {
            let g = GpuMesh {
                vbuf,
                ibuf,
                count,
                clusters: Vec::new(),
                lods: vec![LodRange { first: 0, count, cell: 0.0 }],
                min_lod: 0,
                use_vcol: true,
                texture: None,
                normal: None,
                arm: None,
                cutoff: 0.0,
                _keep: None,
                last_used: frame,
                center,
                radius: job.radius,
                half: [job.radius; 3],
                skin: None,
            };
            self.dyn_meshes.insert(job.key, g);
        }
        let g = self.dyn_meshes.get_mut(&job.key).expect("just made");
        g.center = center;
        g.radius = job.radius;
        g.half = [job.radius; 3];
        g.last_used = frame;
        // (an emitter is stepped once for a picture, whatever asks for it twice)
        match self.fx.tasks.iter_mut().find(|t| t.key == job.key) {
            Some(t) => t.job = job.clone(),
            None => self.fx.tasks.push(crate::fx_gpu::FxTask { key: job.key, job: job.clone() }),
        }
        MeshId::Dyn(job.key)
    }

    /// What the pictures took of the emitters' steps goes back to the processor, which then forgets them (see `FxWorld::gpu_applied`).
    fn settle_fx(&mut self, textures: &mut TextureCache) {
        for (uid, emitter, epoch, t) in std::mem::take(&mut self.fx.acks) {
            textures.fx.gpu_applied(uid, emitter, epoch, t);
        }
    }

    /// A skinned model of the picture: its vertex buffer is the one the card fills (`skin.rs`), so all that is done here is to stage the pose's matrices, and to give the buffer the box of the pose.
    fn ensure_skinned(&mut self, key: u64, base: &Arc<Mesh>, matrices: &[M4], frame: u64) -> MeshId {
        let rk = Arc::as_ptr(base) as usize;
        if !self.skin.rests.contains_key(&rk) {
            match crate::skin::SkinRest::new(&self.gpu.device, &self.gpu.queue, base, frame) {
                Some(r) => {
                    self.skin.rests.insert(rk, r);
                }
                // (a model with no skeleton is not given as skinned: it is drawn as it lies)
                None => return self.ensure_dynamic(key, base, frame),
            }
        }
        let rest = self.skin.rests.get_mut(&rk).expect("just made");
        rest.last_used = frame;
        let (center, radius, half) = rest.bounds(matrices);
        let ok = self.dyn_meshes.get(&key).is_some_and(|g| g.skin == Some(rk) && g.count as usize == base.idx.len() && g.vbuf.size() >= (((base.pos.len() * 48).max(4) as u64 + 3) & !3));
        if !ok {
            let p = Self::prepare_mesh(base, None);
            let (mut g, v, indices) = self.make_mesh(p, base, Some(Arc::downgrade(base)), frame);
            g.vbuf = mesh_buffer(&self.gpu.device, "skinned-vertices", v.len() * 4, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE);
            if !indices.is_empty() {
                self.gpu.queue.write_buffer(&g.ibuf, 0, bytemuck::cast_slice(&indices));
            }
            g.skin = Some(rk);
            self.dyn_meshes.insert(key, g);
            self.skin.forget(key);
        }
        let g = self.dyn_meshes.get_mut(&key).expect("just made");
        g.center = center;
        g.radius = radius;
        g.half = half;
        g.texture = base.tex.clone();
        g.normal = base.normal_tex.clone();
        g.arm = base.arm_tex.clone();
        g.cutoff = base.alpha_cutoff;
        g.use_vcol = !base.col.is_empty();
        g.last_used = frame;
        let mat_base = (self.skin.staged.len() / 16) as u32;
        for m in matrices {
            self.skin.staged.extend_from_slice(&m.0);
        }
        self.skin.tasks.push(crate::skin::SkinTask { key, rest: rk, out: g.vbuf.clone(), mat_base, joints: matrices.len() as u32 });
        MeshId::Dyn(key)
    }

    fn ensure_mesh(&mut self, m: &MeshRef, frame: u64) -> MeshId {
        let (key, keep) = match m {
            MeshRef::Builtin(b) => (*b as *const Mesh as usize, None),
            MeshRef::Shared(a) => (Arc::as_ptr(a) as usize, Some(Arc::downgrade(a))),
            MeshRef::Dynamic { key, mesh } => return self.ensure_dynamic(*key, mesh, frame),
            MeshRef::Skinned { key, base, matrices } => return self.ensure_skinned(*key, base, matrices, frame),
        };
        if let Some(i) = self.partial_meshes.iter().position(|p| p.key == key) {
            // one that is on its way to the card is finished now
            let pm = self.partial_meshes.remove(i);
            if !pm.v.is_empty() {
                self.gpu.queue.write_buffer(&pm.gpu.vbuf, 0, bytes_of_floats(&pm.v));
            }
            if !pm.indices.is_empty() {
                self.gpu.queue.write_buffer(&pm.gpu.ibuf, 0, bytemuck::cast_slice(&pm.indices));
            }
            self.meshes.insert(key, pm.gpu);
        }
        if !self.meshes.contains_key(&key) {
            let _t = lunaris_render::Timed::new(|| format!("upload mesh of {} vertices", m.get().pos.len()));
            let g = self.upload_mesh(m.get(), keep, frame);
            self.meshes.insert(key, g);
        }
        self.meshes.get_mut(&key).expect("just inserted").last_used = frame;
        MeshId::Obj(key)
    }

    fn ensure_tile_mesh(&mut self, m: &MeshRef, key: u64, frame: u64) -> MeshId {
        if !self.tile_meshes.contains_key(&key) {
            let keep = match m {
                MeshRef::Shared(a) | MeshRef::Dynamic { mesh: a, .. } | MeshRef::Skinned { base: a, .. } => Some(Arc::downgrade(a)),
                MeshRef::Builtin(_) => None,
            };
            let g = self.upload_mesh(m.get(), keep, frame);
            self.tile_meshes.insert(key, g);
        }
        self.tile_meshes.get_mut(&key).expect("just inserted").last_used = frame;
        MeshId::Tile(key)
    }

    /// One particle cloud as an object of the list (its quads are in world space already) and the batch that draws it.
    fn cloud_batch(&mut self, p: &SceneParticles, high: bool, soft_on: bool, frame: u64, objects: &mut Vec<ObjectData>) -> Batch {
        let id = match &p.gpu {
            Some(job) => self.ensure_fx_mesh(job, frame),
            None => self.ensure_mesh(&p.mesh, frame),
        };
        let tkey = self.texture_group(p.tex.as_ref(), None, high, if p.nearest { SamplerKind::NearestClamp } else { SamplerKind::LinearClamp }, frame);
        let data = ObjectData {
            model: lunaris_render::math::M4::identity().0,
            vary: [0.0; 4],
            n0: [1.0, 0.0, 0.0, 0.0],
            n1: [0.0, 1.0, 0.0, 0.0],
            n2: [0.0, 0.0, 1.0, 0.0],
            color: [1.0; 4],
            // (the spare roughness of a particle cloud carries its soft distance: see `soft_fade` in scene.wgsl)
            mat: [0.0, 0.0, 0.0, if soft_on { p.soft } else { 0.0 }],
            misc: [1.0, 1.0, 1.0, 0.0],
            uvrect: [0.0, 0.0, 1.0, 1.0],
            ids: [0, 0, 0, 0],
            pbr: [0.0; 4],
        };
        let batch = Batch { scale: 1.0, mesh: id, tex: tkey, first: objects.len() as u32, count: 1, cast: false, cut: false, lod: 0, mask: 0xff, splat: None, early: false, role: 0 };
        objects.push(data);
        batch
    }

    fn mesh(&self, id: MeshId) -> &GpuMesh {
        match id {
            MeshId::Obj(k) => &self.meshes[&k],
            MeshId::Tile(k) => &self.tile_meshes[&k],
            MeshId::Dyn(k) => &self.dyn_meshes[&k],
        }
    }

    /// Upload a texture for one of the two modes (classic: display colours in an 8-bit UNORM texture; high: an sRGB texture with mips).
    fn ensure_texture(&mut self, tex: &Arc<Texture>, high: bool, smooth: bool, frame: u64) {
        let key = (Arc::as_ptr(tex) as usize, high);
        if let Some(t) = self.textures.get_mut(&key) {
            t.last_used = frame;
            return;
        }
        // one that is on its way to the card is finished now
        if let Some(i) = self.partial_textures.iter().position(|p| p.key == key) {
            let mut pt = self.partial_textures.remove(i);
            let mut all = u64::MAX;
            pt.colour.step(&self.gpu.queue, &mut all);
            if let Some(h) = pt.height.as_mut() {
                h.step(&self.gpu.queue, &mut all);
            }
            self.textures.insert(key, pt.gpu);
            return;
        }
        let _t = lunaris_render::Timed::new(|| format!("upload texture {}x{}", tex.width, tex.height));
        // one that is being prepared on another thread is waited for (a picture that wants a texture at once needs it now)
        let waited = self.tex_jobs.remove(&key).and_then(|mut j| j.ready.take().or_else(|| j.pending.wait()));
        let p = waited.unwrap_or_else(|| prepare_texture(tex, high, smooth, self.gpu.device.limits().max_texture_dimension_2d.max(256), self.compress, self.texture_cache.as_deref(), None));
        self.finish_texture(key, p, tex, frame);
    }

    /// The prepared picture's texture on the card, created and not yet filled: the colour levels, the relief levels when it has some, and the numbers that go with them.
    fn make_texture(&mut self, key: (usize, bool), p: PreparedTex, tex: &Arc<Texture>, frame: u64) -> PartialTex {
        let high = key.1;
        let PreparedTex { textures, w, h, levels, avg, block, bytes, height } = p;
        let mut bytes_on_card = bytes;
        let format = match block {
            Some(Block::Bc1) => wgpu::TextureFormat::Bc1RgbaUnormSrgb,
            Some(_) => wgpu::TextureFormat::Bc3RgbaUnormSrgb,
            None if high => wgpu::TextureFormat::Rgba8UnormSrgb,
            None => wgpu::TextureFormat::Rgba8Unorm,
        };
        let (made_colour, made_relief) = match textures {
            Some((c, r)) => (Some(c), r),
            None => (None, None),
        };
        let t = made_colour.unwrap_or_else(|| texture_object(&self.gpu.device, "texture", w, h, levels.len() as u32, format));
        let view = t.create_view(&wgpu::TextureViewDescriptor::default());
        // the relief of a built-in material (only the high-quality mode uses it)
        let (height_view, height_up) = match height {
            Some(chain) => {
                let ht = made_relief.unwrap_or_else(|| texture_object(&self.gpu.device, "height", w, h, chain.len() as u32, wgpu::TextureFormat::R8Unorm));
                bytes_on_card += chain.iter().map(|(_, _, d)| d.len() as u64).sum::<u64>();
                (Some(ht.create_view(&wgpu::TextureViewDescriptor::default())), Some(TexUpload::new(ht, chain, None, 1)))
            }
            None => (None, None),
        };
        PartialTex { key, colour: TexUpload::new(t, levels, block, 4), height: height_up, gpu: GpuTex { view, avg, height: height_view, _keep: Some(Arc::downgrade(tex)), last_used: frame, bytes: bytes_on_card }, _tex: tex.clone() }
    }

    /// The prepared picture's texture on the card, filled at once.
    fn finish_texture(&mut self, key: (usize, bool), p: PreparedTex, tex: &Arc<Texture>, frame: u64) {
        let mut pt = self.make_texture(key, p, tex, frame);
        let mut unlimited = u64::MAX;
        pt.colour.step(&self.gpu.queue, &mut unlimited);
        if let Some(h) = pt.height.as_mut() {
            h.step(&self.gpu.queue, &mut unlimited);
        }
        self.textures.insert(key, pt.gpu);
    }

    /// Whether a texture is on the card, in the asynchronous mode starting its preparation on another thread when it is not and putting it on the card when it is ready. A picture that is still being read
    /// (`Texture::is_pending`) is not there either. Outside the mode the answer is yes: the texture is made on the spot when it is wanted.
    fn texture_ready(&mut self, tex: &Arc<Texture>, high: bool, smooth: bool, frame: u64) -> bool {
        if Texture::is_pending(tex) {
            return false;
        }
        let key = (Arc::as_ptr(tex) as usize, high);
        if let Some(t) = self.textures.get_mut(&key) {
            t.last_used = frame;
            return true;
        }
        if !self.async_prep {
            return true;
        }
        if self.partial_textures.iter().any(|p| p.key == key) {
            return false;
        }
        let (side_limit, compress, cache, device) = (self.gpu.device.limits().max_texture_dimension_2d.max(256), self.compress, self.texture_cache.clone(), self.gpu.device.clone());
        let job = self.tex_jobs.entry(key).or_insert_with(|| {
            let t = tex.clone();
            TexJob { pending: lunaris_render::bg::Pool::shared().spawn({ let t = t.clone(); move || prepare_texture(&t, high, smooth, side_limit, compress, cache.as_deref(), Some(&device)) }), ready: None, _tex: t, asked: frame }
        });
        job.asked = frame;
        if job.ready.is_none() {
            job.ready = job.pending.try_take();
        }
        if job.ready.is_none() {
            return false;
        }
        let p = self.tex_jobs.remove(&key).and_then(|mut j| j.ready.take()).expect("ready");
        let pt = self.make_texture(key, p, tex, frame);
        self.partial_textures.push(pt);
        false
    }

    /// `texture_ready` for a normal or occlusion-roughness-metallic map.
    fn data_ready(&mut self, tex: &Arc<Texture>, kind: DataKind, frame: u64) -> bool {
        if Texture::is_pending(tex) {
            return false;
        }
        let key = data_id(tex, kind);
        if let Some(t) = self.data_textures.get_mut(&key) {
            t.last_used = frame;
            return true;
        }
        if !self.async_prep {
            return true;
        }
        if self.partial_data.iter().any(|p| p.key == key) {
            return false;
        }
        let (compress, cache, device) = (self.compress, self.texture_cache.clone(), self.gpu.device.clone());
        let job = self.data_jobs.entry(key).or_insert_with(|| {
            let t = tex.clone();
            DataJob { pending: lunaris_render::bg::Pool::shared().spawn({ let t = t.clone(); move || prepare_data_texture(&t, kind, compress, cache.as_deref(), Some(&device)) }), ready: None, _tex: t, asked: frame }
        });
        job.asked = frame;
        if job.ready.is_none() {
            job.ready = job.pending.try_take();
        }
        if job.ready.is_none() {
            return false;
        }
        let p = self.data_jobs.remove(&key).and_then(|mut j| j.ready.take()).expect("ready");
        let pd = self.make_data_texture(key, p, tex, frame);
        self.partial_data.push(pd);
        false
    }

    /// Upload a splat map (once; it is kept while it is used): its four weights per texel as a plain 8-bit RGBA texture. A map bigger than the card's largest texture is
    /// thinned out (every other texel, or every third...) to fit.
    fn ensure_splat_texture(&mut self, s: &Arc<Splat>, frame: u64) -> usize {
        let key = Arc::as_ptr(s) as usize;
        if let Some(g) = self.splat_textures.get_mut(&key) {
            g.last_used = frame;
            return key;
        }
        self.ensure_plain(PlainSrc::Splat(s.clone()), frame);
        key
    }

    /// The bind group of one painted terrain (`TerrainLayers`): the splat map, up to four layer textures with their relief maps, and the numbers of `SplatU`. Made for
    /// this picture and pushed on `bgs`; the index comes back.
    fn splat_group(&mut self, sp: &SplatLayers, macro_tex: Option<&Arc<Texture>>, high: bool, frame: u64, bgs: &mut Vec<wgpu::BindGroup>, macro_bgs: &mut Vec<wgpu::BindGroup>) -> usize {
        let skey = self.ensure_splat_texture(&sp.splat, frame);
        let mut keys: [Option<(usize, bool)>; 4] = [None; 4];
        let mut u: SplatU = Zeroable::zeroed();
        u.tint = [[1.0; 4]; 4];
        u.cliff = [-1.0, 1.0, sp.photo, 0.0];
        for (i, l) in sp.layers.iter().take(4).enumerate() {
            if l.cliff && high {
                u.cliff = [i as f32, (sp.ground[0].max(1.0) / l.uv_scale[0].max(1.0e-3)).max(0.05), sp.photo, 0.0];
            }
            self.ensure_texture(&l.tex, high, true, frame);
            keys[i] = Some((Arc::as_ptr(&l.tex) as usize, high));
            u.scales[i / 2][(i % 2) * 2] = l.uv_scale[0];
            u.scales[i / 2][(i % 2) * 2 + 1] = l.uv_scale[1];
            // only a built-in material has a relief to show
            u.bump[i] = if l.tex.heights.is_some() { l.bump } else { 0.0 };
            let t = if high { display_to_linear(gamma2_to_display(l.tint)) } else { l.tint };
            u.tint[i] = [t[0], t[1], t[2], 0.0];
            let a = self.textures[&(Arc::as_ptr(&l.tex) as usize, high)].avg;
            u.avg[i] = [a[0], a[1], a[2], 0.0];
        }
        u.info = [sp.layers.len().min(4) as f32, if u.bump.iter().any(|b| *b > 0.0) { 1.0 } else { 0.0 }, if macro_tex.is_some() && high { 1.0 } else { 0.0 }, 0.0];
        // the terrain's own colour picture rides in the first group of the splat pipeline (a white one when there is none)
        if let Some(m) = macro_tex.filter(|_| high) {
            self.ensure_texture(m, high, true, frame);
        }
        let macro_key = macro_tex.filter(|_| high).map(|m| (Arc::as_ptr(m) as usize, high));
        let buf = self.gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("splat-uniform"), size: std::mem::size_of::<SplatU>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        self.gpu.queue.write_buffer(&buf, 0, bytemuck::bytes_of(&u));
        let colour = |i: usize| match keys[i] {
            Some(k) => &self.textures[&k].view,
            None => &self.white[high as usize],
        };
        let relief = |i: usize| match keys[i] {
            Some(k) => self.textures[&k].height.as_ref().unwrap_or(&self.white_height),
            None => &self.white_height,
        };
        let layer_sampler = if high { &self.samplers.aniso_ground } else { &self.samplers.nearest_repeat };
        let tv = wgpu::BindingResource::TextureView;
        let bg = self.group(
            "splat",
            &self.layouts.splat2,
            &[
                wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: tv(&self.splat_textures[&skey].view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.samplers.linear_clamp) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(layer_sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: tv(colour(0)) },
                wgpu::BindGroupEntry { binding: 5, resource: tv(colour(1)) },
                wgpu::BindGroupEntry { binding: 6, resource: tv(colour(2)) },
                wgpu::BindGroupEntry { binding: 7, resource: tv(colour(3)) },
                wgpu::BindGroupEntry { binding: 8, resource: tv(relief(0)) },
                wgpu::BindGroupEntry { binding: 9, resource: tv(relief(1)) },
                wgpu::BindGroupEntry { binding: 10, resource: tv(relief(2)) },
                wgpu::BindGroupEntry { binding: 11, resource: tv(relief(3)) },
            ],
        );
        let macro_view = match macro_key {
            Some(k) => &self.textures[&k].view,
            None => &self.white[high as usize],
        };
        let mbg = self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("splat-macro"),
            layout: &self.layouts.tex1_small,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(macro_view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.samplers.aniso) }, wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.white_height) }],
        });
        macro_bgs.push(mbg);
        bgs.push(bg);
        bgs.len() - 1
    }

    /// The bind group (texture + sampler) for drawing with `tex` (None = plain white).
    fn texture_group(&mut self, tex: Option<&Arc<Texture>>, maps: Option<&MeshPbr>, high: bool, kind: SamplerKind, frame: u64) -> TexKey {
        let ptr = match tex {
            Some(t) => {
                self.ensure_texture(t, high, kind == SamplerKind::Aniso, frame);
                Arc::as_ptr(t) as usize
            }
            None => 0,
        };
        let data_ptr = |this: &mut Self, t: &Option<Arc<Texture>>, kind: DataKind| match t {
            Some(t) if high => {
                this.ensure_data_texture(t, kind, frame);
                data_id(t, kind)
            }
            _ => 0,
        };
        let (np, ap, dp) = match maps {
            Some(m) => (data_ptr(self, &m.normal, DataKind::Normal), data_ptr(self, &m.arm, DataKind::Arm), data_ptr(self, &m.disp, DataKind::Disp)),
            None => (0, 0, 0),
        };
        let key = (ptr, high, kind, np, ap, dp);
        if !self.tex_bgs.contains_key(&key) {
            let view = if ptr == 0 { &self.white[high as usize] } else { &self.textures[&(ptr, high)].view };
            let height = if ptr == 0 { &self.white_height } else { self.textures[&(ptr, high)].height.as_ref().unwrap_or(&self.white_height) };
            let normal = if np == 0 { &self.flat_normal } else { &self.data_textures[&np].view };
            let arm = if ap == 0 { &self.arm_default } else { &self.data_textures[&ap].view };
            let disp = if dp == 0 { &self.flat_height } else { &self.data_textures[&dp].view };
            let sampler = match kind {
                SamplerKind::NearestRepeat => &self.samplers.nearest_repeat,
                SamplerKind::NearestClamp => &self.samplers.nearest_clamp,
                SamplerKind::Aniso => &self.samplers.aniso,
                SamplerKind::LinearClamp => &self.samplers.linear_clamp,
            };
            let bg = self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("texture"),
                layout: &self.layouts.tex1,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(height) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(normal) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(arm) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(disp) },
                ],
            });
            self.tex_bgs.insert(key, bg);
        }
        key
    }

    /// The picture of a terrain's heights on the card (once; kept while used): one 32-bit float a sample, 0 to 1.
    fn ensure_grass_heights(&mut self, f: &Arc<lunaris_sim::terrain::Heightfield>, frame: u64) -> usize {
        let key = Arc::as_ptr(f) as usize;
        if let Some(e) = self.grass_heights.get_mut(&key) {
            e.2 = frame;
            return key;
        }
        self.ensure_plain(PlainSrc::Heights(f.clone()), frame);
        key
    }

    /// A plain texture on the card at once (finishing the one that was on its way, or made on the spot).
    fn ensure_plain(&mut self, src: PlainSrc, frame: u64) {
        let key = src.key();
        let _t = lunaris_render::Timed::new(|| format!("upload plain texture {key:x}"));
        let waited = self.plain_jobs.remove(&key).and_then(|mut j| j.ready.take().or_else(|| j.pending.wait()));
        let mut pp = match self.partial_plain.iter().position(|p| p.src.key() == key) {
            Some(i) => self.partial_plain.remove(i),
            None => {
                let p = waited.unwrap_or_else(|| prepare_plain(&src, self.gpu.device.limits().max_texture_dimension_2d.max(256), None));
                self.make_plain(src, p)
            }
        };
        let mut all = u64::MAX;
        pp.up.step(&self.gpu.queue, &mut all);
        self.finish_plain(pp, frame);
    }

    /// The texture of a prepared plain picture, nothing copied into it yet.
    fn make_plain(&self, src: PlainSrc, p: PreparedPlain) -> PartialPlain {
        let PreparedPlain { texture, w, h, bytes, format } = p;
        let t = texture.unwrap_or_else(|| texture_object(&self.gpu.device, "plain-texture", w, h, 1, format));
        let view = t.create_view(&wgpu::TextureViewDescriptor::default());
        PartialPlain { src, up: TexUpload::new(t, vec![(w, h, bytes)], None, 4), view }
    }

    /// A plain texture that is all on the card: where its kind is kept.
    fn finish_plain(&mut self, done: PartialPlain, frame: u64) {
        match done.src {
            PlainSrc::Splat(sp) => {
                self.splat_textures.insert(Arc::as_ptr(&sp) as usize, GpuSplat { view: done.view, _keep: sp, last_used: frame });
            }
            PlainSrc::Heights(hf) => {
                self.grass_heights.insert(Arc::as_ptr(&hf) as usize, (done.view, hf, frame));
            }
        }
    }

    /// Whether a plain texture is on the card: in the asynchronous mode prepared on another thread, then copied over the next pictures.
    fn plain_ready(&mut self, src: PlainSrc, frame: u64) -> bool {
        let key = src.key();
        match &src {
            PlainSrc::Splat(_) => {
                if let Some(g) = self.splat_textures.get_mut(&key) {
                    g.last_used = frame;
                    return true;
                }
            }
            PlainSrc::Heights(_) => {
                if let Some(e) = self.grass_heights.get_mut(&key) {
                    e.2 = frame;
                    return true;
                }
            }
        }
        if !self.async_prep {
            return true;
        }
        if self.partial_plain.iter().any(|p| p.src.key() == key) {
            return false;
        }
        let limit = self.gpu.device.limits().max_texture_dimension_2d.max(256);
        let device = self.gpu.device.clone();
        let job = self.plain_jobs.entry(key).or_insert_with(|| {
            let from = src.clone();
            PlainJob { pending: lunaris_render::bg::Pool::shared().spawn({ let from = from.clone(); move || prepare_plain(&from, limit, Some(&device)) }), ready: None, _src: from, asked: frame }
        });
        job.asked = frame;
        if job.ready.is_none() {
            job.ready = job.pending.try_take();
        }
        if job.ready.is_none() {
            return false;
        }
        let p = self.plain_jobs.remove(&key).and_then(|mut j| j.ready.take()).expect("ready");
        let pp = self.make_plain(src, p);
        self.partial_plain.push(pp);
        false
    }

    /// Upload a normal or occlusion-roughness-metallic map: the picture's own bytes in a plain 8-bit texture (the loader squares colours, so the bytes come back by the root), with mips.
    fn ensure_data_texture(&mut self, tex: &Arc<Texture>, kind: DataKind, frame: u64) {
        let key = data_id(tex, kind);
        if let Some(t) = self.data_textures.get_mut(&key) {
            t.last_used = frame;
            return;
        }
        if let Some(i) = self.partial_data.iter().position(|p| p.key == key) {
            let mut pd = self.partial_data.remove(i);
            let mut all = u64::MAX;
            pd.up.step(&self.gpu.queue, &mut all);
            self.data_textures.insert(key, pd.gpu);
            return;
        }
        let waited = self.data_jobs.remove(&key).and_then(|mut j| j.ready.take().or_else(|| j.pending.wait()));
        let p = waited.unwrap_or_else(|| prepare_data_texture(tex, kind, self.compress, self.texture_cache.as_deref(), None));
        self.finish_data_texture(key, p, tex, frame);
    }

    fn make_data_texture(&mut self, key: usize, p: PreparedData, tex: &Arc<Texture>, frame: u64) -> PartialData {
        let PreparedData { texture, w, h, levels, block, bytes } = p;
        let t = texture.unwrap_or_else(|| {
            let format = match block {
                Some(Block::Bc5) => wgpu::TextureFormat::Bc5RgUnorm,
                Some(_) => wgpu::TextureFormat::Bc7RgbaUnorm,
                None => wgpu::TextureFormat::Rgba8Unorm,
            };
            texture_object(&self.gpu.device, "data-map", w, h, levels.len() as u32, format)
        });
        let view = t.create_view(&wgpu::TextureViewDescriptor::default());
        PartialData { key, up: TexUpload::new(t, levels, block, 4), gpu: GpuTex { view, avg: [0.5; 3], height: None, _keep: Some(Arc::downgrade(tex)), last_used: frame, bytes }, _tex: tex.clone() }
    }

    fn finish_data_texture(&mut self, key: usize, p: PreparedData, tex: &Arc<Texture>, frame: u64) {
        let mut pd = self.make_data_texture(key, p, tex, frame);
        let mut unlimited = u64::MAX;
        pd.up.step(&self.gpu.queue, &mut unlimited);
        self.data_textures.insert(key, pd.gpu);
    }

    /// Forget meshes and textures nothing has used for a while (tile maps that were edited, models that left the scene).
    fn evict_old(&mut self, frame: u64) {
        let dead = |last: u64| frame.saturating_sub(last) > 256;
        self.mesh_jobs.retain(|_, j| frame.saturating_sub(j.asked) <= 600);
        self.tex_jobs.retain(|_, j| frame.saturating_sub(j.asked) <= 600);
        self.data_jobs.retain(|_, j| frame.saturating_sub(j.asked) <= 600);
        self.plain_jobs.retain(|_, j| frame.saturating_sub(j.asked) <= 600);
        self.meshes.retain(|_, g| !dead(g.last_used));
        self.tile_meshes.retain(|_, g| !dead(g.last_used));
        self.dyn_meshes.retain(|_, g| !dead(g.last_used));
        self.skin.retain(|k| self.dyn_meshes.contains_key(&k), frame);
        self.fx.retain(|k| self.dyn_meshes.contains_key(&k));
        self.splat_textures.retain(|_, g| !dead(g.last_used));
        self.grass_heights.retain(|_, e| !dead(e.2));
        let gone_data: Vec<usize> = self.data_textures.iter().filter(|(_, t)| dead(t.last_used)).map(|(k, _)| *k).collect();
        for k in gone_data {
            self.data_textures.remove(&k);
            self.tex_bgs.retain(|bk, _| bk.3 != k && bk.4 != k && bk.5 != k);
        }
        let gone: Vec<(usize, bool)> = self.textures.iter().filter(|(_, t)| dead(t.last_used)).map(|(k, _)| *k).collect();
        for k in gone {
            self.textures.remove(&k);
            self.tex_bgs.retain(|bk, _| (bk.0, bk.1) != k);
            self.ui_bgs.retain(|bk, _| bk.0 != k.0);
        }
    }

    /// The bind groups the runs of interface vertices are drawn with, made when they are first needed.
    fn ui_groups(&mut self, batches: &[UiBatch], frame: u64) -> Vec<(usize, bool)> {
        let mut keys = Vec::with_capacity(batches.len());
        for b in batches {
            let ptr = match &b.tex {
                Some(t) => {
                    self.ensure_texture(t, false, false, frame);
                    Arc::as_ptr(t) as usize
                }
                None => 0,
            };
            let key = (ptr, b.smooth);
            if !self.ui_bgs.contains_key(&key) {
                let view = if ptr == 0 { &self.white[0] } else { &self.textures[&(ptr, false)].view };
                let sampler = if b.smooth { &self.samplers.linear_clamp } else { &self.samplers.nearest_clamp };
                let bg = self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("ui"),
                    layout: &self.layouts.text0,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: self.text_u.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) }, wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) }],
                });
                self.ui_bgs.insert(key, bg);
            }
            keys.push(key);
        }
        keys
    }

    // ------------------------------------------------------------------------------------------ targets

    fn texture(&self, label: &str, w: u32, h: u32, layers: u32, mips: u32, samples: u32, format: wgpu::TextureFormat, usage: wgpu::TextureUsages) -> wgpu::Texture {
        self.gpu.device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: layers }, mip_level_count: mips, sample_count: samples, dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] })
    }

    fn group(&self, label: &str, layout: &wgpu::BindGroupLayout, entries: &[wgpu::BindGroupEntry]) -> wgpu::BindGroup {
        self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(label), layout, entries })
    }

    fn ensure_targets(&mut self, w: u32, h: u32, mode: Mode, msaa: u32) {
        if self.targets.as_ref().is_some_and(|t| t.w == w && t.h == h && t.mode == mode && t.msaa == msaa) {
            return;
        }
        self.targets = None;
        self.gi_age = 0;
        self.taa_age = 0;
        let rt = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let high = mode == Mode::High;
        let samples = if high { msaa } else { 1 };
        let hdr_tex = self.texture("hdr", w, h, 1, 1, 1, HDR, rt);
        let hdr = view(&hdr_tex);
        let hdr_ms = if samples > 1 { Some(view(&self.texture("hdr-ms", w, h, 1, 1, samples, HDR, rt))) } else { None };
        let depth = view(&self.texture("depth", w, h, 1, 1, samples, DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT));
        let bg_resolve = self.group("resolve", &self.layouts.resolve, &[wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&hdr) }]);
        let mut t = Targets { w, h, mode, msaa, hdr, hdr_ms, depth, pre_depth: None, ao: None, ao_raw: None, gi_raw: None, bloom: None, bg_resolve, bg_ssao: None, bg_gi: None, taa_in: None, taa_hist: None, bg_taa: None, bg_blur: None };
        if high {
            let pre = view(&self.texture("pre-depth", w, h, 1, 1, 1, DEPTH, rt));
            // the occlusion and the bounced light are made at half the size of the picture (the blur makes them whole again)
            let (hw, hh) = (w.div_ceil(2).max(1), h.div_ceil(2).max(1));
            let ao_raw = view(&self.texture("ao-raw", hw, hh, 1, 1, 1, AO_RAW, rt));
            let ao = [view(&self.texture("ao", w, h, 1, 1, 1, AO, rt)), view(&self.texture("ao-history", w, h, 1, 1, 1, AO, rt))];
            let gi_raw = view(&self.texture("gi-raw", hw, hh, 1, 1, 1, GI_RAW, rt));
            let frame = self.frame_buf.as_entire_binding();
            t.bg_ssao = Some(self.group("ssao", &self.layouts.ssao, &[wgpu::BindGroupEntry { binding: 0, resource: frame.clone() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&pre) }]));
            t.bg_gi = Some(self.group("gi", &self.layouts.gi0, &[wgpu::BindGroupEntry { binding: 0, resource: frame.clone() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&pre) }, wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&t.hdr) }]));
            let blur_bg = |hist: &wgpu::TextureView| self.group("ssao-blur", &self.layouts.blur, &[wgpu::BindGroupEntry { binding: 0, resource: frame.clone() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&pre) }, wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.samplers.linear_clamp) }, wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&ao_raw) }, wgpu::BindGroupEntry { binding: 14, resource: wgpu::BindingResource::TextureView(&gi_raw) }, wgpu::BindGroupEntry { binding: 15, resource: wgpu::BindingResource::TextureView(hist) }]);
            t.bg_blur = Some([blur_bg(&ao[1]), blur_bg(&ao[0])]);
            // bloom: half size, a chain of single-mip views
            let (bw, bh) = ((w / 2).max(1), (h / 2).max(1));
            let levels = ((bw.min(bh) as f32).log2().floor() as usize).clamp(1, 6);
            let chain = self.texture("bloom", bw, bh, 1, levels as u32, 1, HDR, rt);
            let views: Vec<wgpu::TextureView> = (0..levels).map(|l| chain.create_view(&wgpu::TextureViewDescriptor { base_mip_level: l as u32, mip_level_count: Some(1), ..Default::default() })).collect();
            let first = self.group("bloom-first", &self.layouts.bloom_first, &[wgpu::BindGroupEntry { binding: 0, resource: frame.clone() }, wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&t.hdr) }, wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.samplers.linear_clamp) }, wgpu::BindGroupEntry { binding: 8, resource: self.exposure_buf.as_entire_binding() }]);
            let chain_bg = |src: &wgpu::TextureView| self.group("bloom-chain", &self.layouts.bloom_chain, &[wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(src) }, wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.samplers.linear_clamp) }]);
            let down: Vec<wgpu::BindGroup> = (0..levels - 1).map(|l| chain_bg(&views[l])).collect();
            let up: Vec<wgpu::BindGroup> = (0..levels - 1).map(|l| chain_bg(&views[l + 1])).collect();
            t.bloom = Some(Bloom { levels, first, down, up, views });
            let taa_in = view(&self.texture("taa-in", w, h, 1, 1, 1, HDR, rt));
            let taa_hist = [view(&self.texture("taa-history-a", w, h, 1, 1, 1, HDR, rt)), view(&self.texture("taa-history-b", w, h, 1, 1, 1, HDR, rt))];
            let taa_bg = |read: &wgpu::TextureView| self.group("taa", &self.layouts.taa0, &[wgpu::BindGroupEntry { binding: 0, resource: frame.clone() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&pre) }, wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&taa_in) }, wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.samplers.linear_clamp) }, wgpu::BindGroupEntry { binding: 16, resource: wgpu::BindingResource::TextureView(read) }]);
            t.bg_taa = Some([taa_bg(&taa_hist[1]), taa_bg(&taa_hist[0])]);
            t.taa_in = Some(taa_in);
            t.taa_hist = Some(taa_hist);
            t.pre_depth = Some(pre);
            t.ao_raw = Some(ao_raw);
            t.gi_raw = Some(gi_raw);
            t.ao = Some(ao);
        }
        self.targets = Some(t);
    }

    fn ensure_ssr_targets(&mut self, w: u32, h: u32) {
        if self.ssr_targets.as_ref().is_some_and(|s| s.w == w && s.h == h) {
            return;
        }
        let rt = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let view = |t: wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let st = SsrTargets {
            w,
            h,
            gbuf_a: view(self.texture("ssr-surface", w, h, 1, 1, 1, GBUF_A, rt)),
            gbuf_b: view(self.texture("ssr-reflectance", w, h, 1, 1, 1, GBUF_B, rt)),
            depth: view(self.texture("ssr-depth", w, h, 1, 1, 1, DEPTH, rt)),
            out: view(self.texture("ssr-layer", w, h, 1, 1, 1, HDR, rt)),
        };
        self.ssr_targets = Some(st);
    }

    fn ensure_shadow(&mut self, size: u32) {
        if self.shadow.as_ref().is_some_and(|s| s.size == size) {
            return;
        }
        let tex = self.texture("shadow", size, size, crate::util::CASCADES as u32, 1, 1, DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let array_view = tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let layers = (0..crate::util::CASCADES as u32).map(|i| tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_array_layer: i, array_layer_count: Some(1), ..Default::default() })).collect();
        self.shadow = Some(ShadowMap { size, array_view, layers });
    }

    /// The shadow maps of the (up to two) point lights: six faces each, as twelve layers of one array.
    fn ensure_point_shadow(&mut self, size: u32, lights: usize) {
        // (the map has six faces for each light that can cast: two, or as many as `Lighting.point_shadows` asks for; it grows and keeps what it has)
        let faces = lights.clamp(2, POINT_SHADOWS) as u32 * 6;
        if self.point_shadow.as_ref().is_some_and(|s| s.size == size && s.layers.len() as u32 >= faces) {
            return;
        }
        let tex = self.texture("point-shadow", size, size, faces, 1, 1, DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let array_view = tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let layers = (0..faces).map(|i| tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_array_layer: i, array_layer_count: Some(1), ..Default::default() })).collect();
        self.point_shadow = Some(ShadowMap { size, array_view, layers });
    }

    fn ensure_out_image(&mut self, w: u32, h: u32) {
        if self.out_image.as_ref().is_some_and(|(ow, oh, _)| *ow == w && *oh == h) {
            return;
        }
        let t = self.texture("image", w, h, 1, 1, 1, OUT_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        self.out_image = Some((w, h, t));
    }

    fn ensure_id_targets(&mut self, w: u32, h: u32) {
        if self.id_targets.as_ref().is_some_and(|t| t.w == w && t.h == h) {
            return;
        }
        let tex = self.texture("ids", w, h, 1, 1, 1, IDS, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let depth = self.texture("id-depth", w, h, 1, 1, 1, DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&wgpu::TextureViewDescriptor::default());
        self.id_targets = Some(IdTargets { w, h, tex, view, depth });
        let size = (w * 4).div_ceil(256) as u64 * 256 * h as u64;
        if !self.id_readback.as_ref().is_some_and(|(s, _)| *s >= size) {
            let b = self.gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("id-readback"), size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            self.id_readback = Some((size, b));
        }
    }

    fn ensure_readback(&mut self, size: u64) {
        if self.readback.as_ref().is_some_and(|(s, _)| *s >= size) {
            return;
        }
        let b = self.gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("readback"), size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        self.readback = Some((size, b));
    }

    fn grow_objects(&mut self, count: u64) {
        if count <= self.objects_cap {
            return;
        }
        let cap = count.next_power_of_two();
        self.objects = self.gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("objects"), size: cap * std::mem::size_of::<ObjectData>() as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        self.objects_cap = cap;
    }

    fn grow_text(&mut self, verts: u64) {
        if verts <= self.text_vb_cap {
            return;
        }
        let cap = verts.next_power_of_two();
        self.text_vb = self.gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("text-vertices"), size: cap * 32, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        self.text_vb_cap = cap;
    }

    // ------------------------------------------------------------------------------------------ drawing

    /// The mesh on the card, the texture bind group and the numbers of its material for `m` (see `MatInfo`).
    fn material_info(&mut self, m: &lunaris_render::render::SceneMesh, high: bool, frame_no: u64, splat_bgs: &mut Vec<wgpu::BindGroup>, splat_macro_bgs: &mut Vec<wgpu::BindGroup>) -> MatInfo {
        if !self.mesh_ready(&m.mesh, frame_no) {
            return MatInfo::waiting();
        }
        if let Some(cover) = &m.depth_cover {
            if !self.mesh_ready(&MeshRef::Shared(cover.clone()), frame_no) {
                return MatInfo::waiting();
            }
        }
        let id = self.ensure_mesh(&m.mesh, frame_no);
        let (use_vcol, model_tex, center, radius, half, model_maps, lods, whole, min_lod) = {
            let g = self.mesh(id);
            (g.use_vcol, g.texture.clone(), g.center, g.radius, g.half, (g.normal.clone(), g.arm.clone(), g.cutoff), if g.lods.len() > 1 { g.lods.clone() } else { Vec::new() }, g.count, g.min_lod)
        };
        let tex = m.tex.clone().or(model_tex);
        // an imported model's own normal and metallic-roughness maps, unless the entity has a `PbrMaps` of its own
        let pbr_eff: Option<MeshPbr> = m.pbr.clone().or_else(|| {
            (model_maps.0.is_some() || model_maps.1.is_some()).then(|| MeshPbr { normal: model_maps.0.clone(), arm: model_maps.1.clone(), disp: None, height_scale: 0.0, normal_strength: 1.0, ao_strength: 1.0, roughness_scale: 1.0, untile: false })
        });
        // every picture the material needs is on the card (or on its way: the object waits)
        if self.async_prep && high {
            let mut all = true;
            if let Some(t) = &tex {
                all &= self.texture_ready(t, high, true, frame_no);
            }
            if let Some(p) = &pbr_eff {
                if let Some(t) = &p.normal {
                    all &= self.data_ready(t, DataKind::Normal, frame_no);
                }
                if let Some(t) = &p.arm {
                    all &= self.data_ready(t, DataKind::Arm, frame_no);
                }
                if let Some(t) = &p.disp {
                    all &= self.data_ready(t, DataKind::Disp, frame_no);
                }
            }
            if let Some(sp) = &m.splat {
                all &= self.plain_ready(PlainSrc::Splat(sp.splat.clone()), frame_no);
                for l in sp.layers.iter().take(4) {
                    all &= self.texture_ready(&l.tex, high, true, frame_no);
                }
                if let Some(mt) = &m.macro_tex {
                    all &= self.texture_ready(mt, high, true, frame_no);
                }
            }
            if !all {
                return MatInfo::waiting();
            }
        }
        let splat = m.splat.as_ref().map(|sp| self.splat_group(sp, m.macro_tex.as_ref(), high, frame_no, splat_bgs, splat_macro_bgs));
        let tkey = self.texture_group(tex.as_ref(), pbr_eff.as_ref(), high, if high { SamplerKind::Aniso } else { SamplerKind::NearestRepeat }, frame_no);
        MatInfo { id, tkey, use_vcol, center, radius, half, cutoff: model_maps.2, lods, min_lod, whole, tex_some: tex.is_some(), tex_heights: tex.as_ref().is_some_and(|t| t.heights.is_some()), pbr_eff, splat, class: 0, ready: true }
    }

    fn draw(&mut self, scene: &Scene, w: u32, h: u32, out: Out<'_>, q: &Quality, want_ids: bool, adapt: Option<f32>) -> Result<(), String> {
        let mut cpu: Vec<(String, f32)> = Vec::new();
        let mut lap_at = std::time::Instant::now();
        let mut lap = |name: &str, cpu: &mut Vec<(String, f32)>| {
            let now = std::time::Instant::now();
            cpu.push((name.to_string(), now.duration_since(lap_at).as_secs_f32() * 1000.0));
            lap_at = now;
        };
        // the sixteen lights that matter most for this picture: the sun and other directional lights first, then the point and spot lights by how much of the picture they light: their brightness times the square of the share of the view
        // their reach covers (a lamp by the camera most, a lamp across the street less, and a big bright light over a far tower more than many small lamps near you: it lights what you see); with sixteen or fewer nothing changes
        let lights_sel: Vec<lunaris_render::render::LightData> = if scene.all_lights.len() <= MAX_LIGHTS {
            scene.all_lights.clone()
        } else {
            let eye = scene.cam.pos;
            let mut idx: Vec<usize> = (0..scene.all_lights.len()).collect();
            let key = |l: &lunaris_render::render::LightData| -> f32 {
                if !l.point {
                    return f32::MIN;
                }
                let d = length(sub(l.vec, eye));
                let lum = 0.2126 * l.color[0] + 0.7152 * l.color[1] + 0.0722 * l.color[2];
                -(lum * (l.range / (d - l.range * 0.5).max(1.0)).powi(2))
            };
            idx.sort_by(|&a, &b| key(&scene.all_lights[a]).partial_cmp(&key(&scene.all_lights[b])).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b)));
            idx.truncate(MAX_LIGHTS);
            idx.iter().map(|&i| scene.all_lights[i]).collect()
        };
        let mode = q.mode;
        let high = mode == Mode::High;
        let samples = if high { if q.msaa >= 4 { 4 } else { 1 } } else { 1 };
        self.frame_no += 1;
        let frame_no = self.frame_no;
        let t_pump = lunaris_render::Timed::new(|| "copy to the card".to_string());
        self.pump_uploads(frame_no);
        drop(t_pump);
        let env = &scene.env;
        self.ensure_targets(w, h, mode, samples);
        let out_format = match &out {
            Out::Image => OUT_FORMAT,
            Out::Surface { format, .. } => *format,
            Out::Probe { .. } => HDR,
        };
        // a face of a reflection probe: only the light, none of the picture effects (and no probes of its own)
        let probe_out = matches!(out, Out::Probe { .. });

        // ---- the object list: meshes sorted into instanced runs, then the sprite and tile layers in drawing order
        #[derive(Clone, Copy)]
        struct MeshItem {
            /// Its place in `scene.meshes` (the id picture numbers it one more: 0 is nothing).
            mesh: u32,
            /// What it shares with the objects of the same mesh and material: the index into `infos`, and the number of the (mesh, texture, terrain) it is drawn with, for sorting.
            mat: u32,
            class: u32,
            cast: bool,
            /// Glows (a lamp's bulb): it must not shadow the light that sits inside it.
            glows: bool,
            /// See-through (high quality only): drawn after the opaque meshes, far to near, blended.
            clear: bool,
            /// Distance in front of the camera, for sorting.
            depth: f32,
            sphere: (V3, f32),
            /// The biggest scale of its matrix.
            scale: f32,
            /// Cut out by its texture's alpha.
            cut: bool,
            /// The level of detail it is drawn at, and how many triangles that is.
            lod: u8,
            tris: u32,
            /// Which passes see it: bit 0 the camera's, bits 1 to 3 the three shadow cascades (a caster only).
            mask: u8,
            /// Its vertices are moved by the shader (waves or wind): the early depth pass cannot draw it.
            disp: bool,
            /// `SceneMesh::depth_role`: 1 is drawn in the depth-only passes only, 2 in the colour passes only.
            role: u8,
            /// Its mesh or a picture it needs is not on the card yet: no pass sees it.
            waiting: bool,
        }
        let mut splat_bgs: Vec<wgpu::BindGroup> = Vec::new();
        let mut splat_macro_bgs: Vec<wgpu::BindGroup> = Vec::new();
        // pixels in a metre seen at a metre (the projection's scale), for the levels of detail; none for an orthographic view
        let focal_px = 0.5 * h as f32 * scene.proj.0[5];
        let lod_on = high && self.lod && scene.proj.0[15] == 0.0 && focal_px > 0.0;
        let tri_pixels = self.lod_tri_pixels;
        let mut slots: Vec<u64> = Vec::with_capacity(scene.meshes.len() + scene.layers.len());
        // what a mesh and its material give every object that uses them is worked out once for each pair (a scene of a hundred thousand trees has a few dozen of them); `classes` numbers the (mesh, texture, terrain) triples
        let mut infos: Vec<MatInfo> = Vec::new();
        let mut info_of: FastMap<(usize, usize), u32> = FastMap::default();
        let mut classes: FastMap<(MeshId, TexKey, Option<usize>), u32> = FastMap::default();
        // first, which material each mesh has (look-ups; a new pair is made with the card's help, which needs `self`)
        let mut mats: Vec<u32> = Vec::with_capacity(scene.meshes.len());
        let t_materials = lunaris_render::Timed::new(|| "draw: materials of the meshes".to_string());
        self.prepare_dynamic_vertices(&scene.meshes);
        for m in &scene.meshes {
            slots.push(m.uid);
            let ckey = if m.pbr.is_none() && m.splat.is_none() {
                match &m.mesh {
                    MeshRef::Builtin(b) => Some(*b as *const Mesh as usize),
                    MeshRef::Shared(a) => Some(Arc::as_ptr(a) as usize),
                    MeshRef::Dynamic { .. } | MeshRef::Skinned { .. } => None,
                }
                .map(|mk| (mk, m.tex.as_ref().map_or(0, |t| Arc::as_ptr(t) as usize)))
            } else {
                None
            };
            let found = ckey.and_then(|k| info_of.get(&k).copied());
            let mat = match found {
                Some(i) => i,
                None => {
                    let mut made = self.material_info(m, high, frame_no, &mut splat_bgs, &mut splat_macro_bgs);
                    let next = classes.len() as u32;
                    made.class = *classes.entry((made.id, made.tkey, made.splat)).or_insert(next);
                    infos.push(made);
                    let i = infos.len() as u32 - 1;
                    if let Some(k) = ckey {
                        info_of.insert(k, i);
                    }
                    i
                }
            };
            mats.push(mat);
        }
        drop(t_materials);
        // then each object's own numbers (a function of the mesh alone: split over the cores)
        let blank = MeshItem { mesh: 0, mat: 0, class: 0, cast: false, glows: false, clear: false, depth: 0.0, sphere: ([0.0; 3], 0.0), scale: 1.0, cut: false, lod: 0, tris: 0, mask: 0, disp: false, role: 0, waiting: false };
        let mut items: Vec<MeshItem> = vec![blank; scene.meshes.len()];
        par_chunks_mut(&mut items, 4096, |start, chunk| {
            for (k, slot) in chunk.iter_mut().enumerate() {
                let mi = start + k;
                let m = &scene.meshes[mi];
                let mat = mats[mi];
                let info = &infos[mat as usize];
                let (center, radius, cutoff, whole, tex_some, lods) = (info.center, info.radius, info.cutoff, info.whole, info.tex_some, &info.lods);
                let scale = {
                    let c = |i: usize| (m.model.0[i * 4].powi(2) + m.model.0[i * 4 + 1].powi(2) + m.model.0[i * 4 + 2].powi(2)).sqrt();
                    c(0).max(c(1)).max(c(2))
                };
                let see_through = high && (m.alpha < 0.999 || m.water_look.is_some());
                let wc = m.model.transform_point(center);
                let depth = -scene.view.transform_point(wc)[2];
                // the level of detail: coarser for what is far enough for the loss to stay within a pixel (perspective views of the high qualities only; water and swaying things keep all of theirs)
                let lod = if lod_on && !lods.is_empty() && m.water.is_none() && m.sway.is_none() {
                    let dc = length(sub(wc, scene.cam.pos));
                    let d = (dc - radius * scale).max(0.0);
                    choose_lod(lods, d, scale, focal_px, info.min_lod).max(density_lod(lods, dc, radius * scale, focal_px, tri_pixels)) as u8
                } else {
                    info.min_lod
                };
                *slot = MeshItem { mesh: mi as u32, mat, class: info.class, cast: m.cast_shadow && !see_through && info.ready && m.depth_role != 2, glows: m.emissive > 0.0, clear: see_through, depth, scale, sphere: (wc, radius * scale + m.sway.map_or(0.0, |s| 2.0 * s[0].abs()) + m.water.map_or(0.0, |w| 3.0 * w[0].abs())), cut: high && cutoff > 0.0 && tex_some, lod, tris: lods.get(lod as usize).map_or(whole, |l| l.count) / 3, mask: 0, disp: m.sway.is_some() || m.water.is_some() || m.water_look.is_some(), role: m.depth_role, waiting: !info.ready };
            }
        });
        // the numbers an object gives the shaders, made only for the objects that are drawn (a culled one never needs them)
        let build = |it: &MeshItem, info: &MatInfo| -> ObjectData {
            let m = &scene.meshes[it.mesh as usize];
            let (tex_some, tex_heights, pbr_eff) = (info.tex_some, info.tex_heights, &info.pbr_eff);
            let color = if high { display_to_linear(gamma2_to_display(m.color)) } else { m.color };
            let nm = m.model.normal_matrix();
            // water that is flat (no waves) is still water when it has a look: the surface shader needs the same path
            let waterish = m.water.or(m.water_look.map(|_| [0.0, 8.0, 0.0]));
            let mut data = ObjectData {
                model: m.model.0,
                vary: [0.0; 4],
                // (n0.w: where an imported material is cut out, in the high qualities; water keeps the colour of its shallows in the three w's)
                n0: [nm[0][0], nm[1][0], nm[2][0], if high { info.cutoff } else { 0.0 }],
                n1: [nm[0][1], nm[1][1], nm[2][1], 0.0],
                n2: [nm[0][2], nm[1][2], nm[2][2], 0.0],
                color: [color[0], color[1], color[2], if high { m.alpha } else { 1.0 }],
                mat: [m.shininess as f32, m.emissive, m.metallic, m.roughness],
                misc: [m.uv_scale[0], m.uv_scale[1], if tex_some { 1.0 } else { 0.0 }, if info.use_vcol { 1.0 } else { 0.0 }],
                uvrect: match (waterish, m.sway) {
                    (Some(wv), _) if high => [0.0, wv[0], wv[1], wv[2]],
                    (None, Some(sw)) if high => [0.0, sw[0], sw[1], sw[2]],
                    _ => [if high && tex_heights { m.bump } else { 0.0 }, if high { pbr_eff.as_ref().filter(|p| p.disp.is_some()).map_or(0.0, |p| p.height_scale) } else { 0.0 }, 1.0, 1.0],
                },
                ids: [it.mesh + 1, if !high { 0 } else if waterish.is_some() { 1 } else if m.sway.is_some() { 2 } else { 0 }, 0, 0],
                pbr: match (&m.water_look, pbr_eff) {
                    (Some(wl), _) if high => [wl[0], wl[1], wl[2], 1.0],
                    (_, Some(p)) if high && (p.normal.is_some() || p.arm.is_some() || p.disp.is_some() || p.untile) => [p.normal_strength, p.ao_strength, p.normal.is_some() as u32 as f32 + 2.0 * p.arm.is_some() as u32 as f32 + 4.0 * p.disp.is_some() as u32 as f32 + 8.0 * p.untile as u32 as f32, p.roughness_scale],
                    _ => [0.0; 4],
                },
            };
            if let (Some(v), true) = (&m.vary, high) {
                data.vary = *v;
            }
            // `MaterialExt` rides in the two spare words of the ids, a byte for each number
            if let (Some(e), true) = (&m.ext, high) {
                let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
                data.ids[2] = q(e[0]) | (q(e[1]) << 8) | (q(e[2]) << 16) | (q(e[3]) << 24);
                data.ids[3] = q(e[4]) | (q(e[5]) << 8) | (q(e[6]) << 16);
            }
            if let (Some(wl), true) = (&m.water_look, high) {
                data.n0[3] = wl[3];
                data.n1[3] = wl[4];
                data.n2[3] = wl[5];
            }
            data
        };
        lap("items", &mut cpu);
        // ---- who sees what: the camera, and the three cascades of the sun's shadow (the casters only: what stands in front of a cascade's box still throws its shadow in, so only the sides of the box count). An item that no pass
        // sees is not drawn or even uploaded; the rest are sorted so that those the same passes see stand together: a pass draws whole batches, and a frame with a hundred thousand scattered objects is a few draws, not thousands
        let casters = Casters { spheres: items.iter().filter(|i| i.cast).map(|i| i.sphere).collect() };
        let aspect = w as f32 / h as f32;
        let sun = lights_sel.iter().position(|l| !l.point);
        let shadows_on = high && q.shadows && env.shadows && !scene.meshes.is_empty() && sun.is_some() && !casters.spheres.is_empty();
        let cascades = if shadows_on {
            self.ensure_shadow(q.shadow_size.clamp(256, 8192));
            let sun_dir = lights_sel[sun.expect("checked")].vec;
            // (`Lighting.shadow_distance` pushes the cascades out when it is farther than the Environment's)
            let reach = if high && env.lighting[7] > 0.5 && env.lighting[6] > 0.0 { env.shadow_distance.max(env.lighting[6]) } else { env.shadow_distance };
            Some(fit_cascades(&scene.cam, aspect, sun_dir, reach, q.shadow_size.clamp(256, 8192), &casters))
        } else {
            None
        };
        lap("shadow-fit", &mut cpu);
        let main_fr = self.culling.then(|| Frustum::from_view_proj(&to_gpu_clip(&scene.view_proj), false));
        let shadow_fr: Vec<Option<Frustum>> = (0..3).map(|i| cascades.as_ref().filter(|_| self.culling).map(|c| Frustum::from_view_proj(&c[i].view_proj, true))).collect();
        par_chunks_mut(&mut items, 4096, |_, chunk| {
            for it in chunk.iter_mut() {
                if it.waiting {
                    it.mask = 0;
                    continue;
                }
                let (c, r) = it.sphere;
                // (the picture's sub-pixel shift for the temporal anti-aliasing is not in the planes: a margin of a pixel or so)
                let mut mask = if main_fr.as_ref().is_none_or(|f| f.sphere_visible(c, r + 0.003 * length(sub(c, scene.cam.pos)) + 0.02)) { 1u8 } else { 0 };
                if it.cast && cascades.is_some() {
                    for k in 0..3 {
                        if shadow_fr[k].as_ref().is_none_or(|f| f.sphere_visible(c, r)) {
                            mask |= 2 << k;
                        }
                    }
                }
                it.mask = mask;
            }
        });
        // occlusion: the solid bodies of what is in the picture (the buildings' prisms) hide what is wholly behind them from the camera's pass (the shadow passes keep their objects)
        self.last_occluded = (0, 0);
        if self.occ_wait > 0 && !self.occ_always {
            self.occ_wait -= 1;
        } else if self.occlusion && high && !probe_out && self.culling && scene.proj.0[15] == 0.0 {
            let _t = lunaris_render::Timed::new(|| "draw: occlusion".to_string());
            if self.occ_buf.begin(&scene.view, &scene.proj, scene.cam.pos, aspect) {
                for it in items.iter().filter(|i| i.mask & 1 != 0 && !i.waiting) {
                    let m = &scene.meshes[it.mesh as usize];
                    for o in &m.mesh.get().occluders {
                        let mut c = [[0.0f32; 3]; 8];
                        for k in 0..4 {
                            c[k] = m.model.transform_point([o[2 * k], o[8], o[2 * k + 1]]);
                            c[4 + k] = m.model.transform_point([o[2 * k], o[9], o[2 * k + 1]]);
                        }
                        self.occ_buf.add(&crate::occlusion::Prism { c });
                    }
                }
                if self.occ_buf.drawn > 0 {
                    let mut hidden = 0u32;
                    let mut tested = 0u32;
                    for it in items.iter_mut().filter(|i| i.mask & 1 != 0 && !i.disp && !i.waiting) {
                        tested += 1;
                        let m = &scene.meshes[it.mesh as usize];
                        let info = &infos[it.mat as usize];
                        let mut c = [[0.0f32; 3]; 8];
                        for (k, corner) in c.iter_mut().enumerate() {
                            let local = [info.center[0] + if k & 1 == 0 { -info.half[0] } else { info.half[0] }, info.center[1] + if k & 2 == 0 { -info.half[1] } else { info.half[1] }, info.center[2] + if k & 4 == 0 { -info.half[2] } else { info.half[2] }];
                            *corner = m.model.transform_point(local);
                        }
                        if self.occ_buf.hidden(&c) {
                            it.mask &= !1;
                            hidden += 1;
                        }
                    }
                    self.last_occluded = (hidden, self.occ_buf.drawn);
                    // too little hidden to pay for the pass: leave it out for a while
                    if (hidden as f32) < 0.25 * tested as f32 {
                        self.occ_wait = 32;
                    }
                } else {
                    self.occ_wait = 32;
                }
            }
        }
        self.last_cull = (items.len() as u32, items.iter().filter(|i| i.mask & 1 == 0).count() as u32);
        self.last_triangles = items.iter().filter(|i| i.mask & 1 != 0 && i.role != 1).map(|i| i.tris as u64).sum();
        self.last_lods = [0; 10];
        for it in items.iter().filter(|i| i.mask & 1 != 0) {
            self.last_lods[(it.lod as usize).min(9)] += 1;
        }
        if self.prof.on {
            let mut by: FastMap<u32, (u32, u64, [u32; 10], u32)> = FastMap::default();
            for it in items.iter().filter(|i| i.mask & 1 != 0) {
                let e = by.entry(it.mat).or_insert((0, 0, [0; 10], it.mesh));
                e.0 += 1;
                e.1 += it.tris as u64;
                e.2[(it.lod as usize).min(9)] += 1;
            }
            let mut v: Vec<(u32, u32, [u32; 10], u64, u32)> = by.into_iter().map(|(mat, (n, tris, lods, sample))| (infos[mat as usize].whole / 3, n, lods, tris, scene.meshes[sample as usize].mesh.get().pos.len() as u32)).collect();
            v.sort_by(|a, b| b.3.cmp(&a.3));
            v.truncate(6);
            self.last_top_meshes = v;
        }
        lap("cull", &mut cpu);
        // the solid items, grouped: one number for the (mesh, texture, terrain), the flags, the level of detail and who sees it, and under them the item's own place, so that the sort is a plain one of integers (and the same every time)
        assert!(classes.len() < (1 << 19), "too many different materials in one picture");
        let mut keys: Vec<u64> = Vec::with_capacity(items.len());
        for (i, it) in items.iter().enumerate() {
            if !it.clear && it.mask != 0 {
                keys.push(((it.class as u64) << 45) | (((it.role & 3) as u64) << 43) | ((it.disp as u64) << 42) | ((it.cast as u64) << 41) | ((it.cut as u64) << 40) | (((it.lod & 15) as u64) << 36) | (((it.mask & 15) as u64) << 32) | i as u64);
            }
        }
        keys.sort_unstable();
        let order: Vec<usize> = keys.iter().map(|k| (*k & 0xFFFF_FFFF) as usize).collect();
        lap("sort", &mut cpu);
        let mut objects: Vec<ObjectData> = Vec::with_capacity(keys.len() + scene.layers.len());
        // the bounding sphere of each object of the two lists below, in the same order (what comes after them in `objects` has none and is always drawn)
        let mut obj_sphere: Vec<(V3, f32)> = Vec::with_capacity(keys.len());
        let mut obj_tris: Vec<u32> = Vec::with_capacity(keys.len());
        let mut mesh_batches: Vec<Batch> = Vec::new();
        let mut last_group = u64::MAX;
        for (j, &key) in keys.iter().enumerate() {
            let it = &items[(key & 0xFFFF_FFFF) as usize];
            let info = &infos[it.mat as usize];
            // (the top bits are the class, the flags, the level and the mask: the same ones, the same batch)
            if key >> 32 == last_group {
                let b = mesh_batches.last_mut().expect("a batch is open");
                b.count += 1;
                b.scale = b.scale.max(it.scale);
            } else {
                last_group = key >> 32;
                mesh_batches.push(Batch { scale: it.scale, mesh: info.id, tex: info.tkey, first: j as u32, count: 1, cast: it.cast, cut: it.cut, lod: it.lod, mask: it.mask, splat: info.splat, early: !it.cut && !it.disp && info.splat.is_none(), role: it.role });
            }
            obj_sphere.push(it.sphere);
            obj_tris.push(it.tris);
        }
        // the numbers of the objects, in that order, made on all the cores
        objects.resize(keys.len(), Zeroable::zeroed());
        par_chunks_mut(&mut objects, 4096, |start, chunk| {
            for (k, o) in chunk.iter_mut().enumerate() {
                let it = &items[(keys[start + k] & 0xFFFF_FFFF) as usize];
                *o = build(it, &infos[it.mat as usize]);
            }
        });
        // see-through meshes: far to near, runs of the same mesh and texture drawn together
        let mut clear_order: Vec<usize> = (0..items.len()).filter(|&i| items[i].clear && items[i].mask != 0).collect();
        clear_order.sort_by(|&a, &b| items[b].depth.partial_cmp(&items[a].depth).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b)));
        let mut clear_batches: Vec<Batch> = Vec::new();
        for &i in &clear_order {
            let it = &items[i];
            let info = &infos[it.mat as usize];
            if clear_batches.last().is_some_and(|b| b.mesh == info.id && b.tex == info.tkey) {
                clear_batches.last_mut().expect("checked").count += 1;
            } else {
                clear_batches.push(Batch { scale: 1.0, mesh: info.id, tex: info.tkey, first: objects.len() as u32, count: 1, cast: false, cut: false, lod: it.lod, mask: it.mask, splat: None, early: false, role: 0 });
            }
            objects.push(build(it, info));
            obj_sphere.push(it.sphere);
            obj_tris.push(it.tris);
        }
        // the depth-only passes (shadows, early depth, the id picture) draw the wholes of the models that are drawn in pieces; the colour passes draw the pieces
        let any_role = mesh_batches.iter().any(|b| b.role != 0);
        let depth_list: Vec<Batch> = if any_role { mesh_batches.iter().filter(|b| b.role != 2).cloned().collect() } else { Vec::new() };
        let color_list: Vec<Batch> = if any_role { mesh_batches.iter().filter(|b| b.role != 1).cloned().collect() } else { Vec::new() };
        let depth_batches: &Vec<Batch> = if any_role { &depth_list } else { &mesh_batches };
        let color_batches: &Vec<Batch> = if any_role { &color_list } else { &mesh_batches };
        let have_cut = mesh_batches.iter().any(|b| b.cut);
        // the early depth pass: worth its extra pass when the picture has a lot of triangles (a city: the same pixel is shaded many times over when walls hide what is behind them)
        let zpre_on = high && !probe_out && !want_ids && mesh_batches.iter().any(|b| b.early && b.mask & 1 != 0) && self.early_z.unwrap_or(self.last_triangles >= 120_000);
        lap("visibility+batches", &mut cpu);
        // soft particles need the depth of everything opaque (the prepass' picture, the one deep water reads)
        let soft_on = high && !probe_out && scene.particles.iter().any(|p| p.soft > 0.0);
        // the sprite and tile layers in drawing order; a particle cloud with a `DrawLayer` stands among them (each step names the pipeline that draws it)
        let mut layer_steps: Vec<(PipeId, Batch)> = Vec::new();
        let mut among = scene.particles.iter().filter(|p| p.slot.is_some()).peekable();
        for (i, l) in scene.layers.iter().enumerate() {
            while let Some(p) = among.next_if(|p| p.slot.is_some_and(|s| s <= i)) {
                let batch = self.cloud_batch(p, high, soft_on, frame_no, &mut objects);
                layer_steps.push((if p.additive { PipeId::ParticleAdd } else { PipeId::Particle }, batch));
            }
            slots.push(l.uid);
            let id = if l.mesh_key != 0 { self.ensure_tile_mesh(&l.mesh, l.mesh_key, frame_no) } else { self.ensure_mesh(&l.mesh, frame_no) };
            let tkey = self.texture_group(l.tex.as_ref(), None, high, SamplerKind::NearestClamp, frame_no);
            let color = if high { display_to_linear(gamma2_to_display([l.color[0], l.color[1], l.color[2]])) } else { [l.color[0], l.color[1], l.color[2]] };
            let nm = l.model.normal_matrix();
            let data = ObjectData {
                model: l.model.0,
                vary: [0.0; 4],
                n0: [nm[0][0], nm[1][0], nm[2][0], 0.0],
                n1: [nm[0][1], nm[1][1], nm[2][1], 0.0],
                n2: [nm[0][2], nm[1][2], nm[2][2], 0.0],
                color: [color[0], color[1], color[2], l.color[3]],
                mat: [0.0; 4],
                misc: [1.0, 1.0, if l.tex.is_some() { 1.0 } else { 0.0 }, 0.0],
                uvrect: l.uv,
                ids: [slots.len() as u32, 0, 0, 0],
                pbr: [0.0; 4],
            };
            let same = matches!(layer_steps.last(), Some((PipeId::Sprite, b)) if b.mesh == id && b.tex == tkey);
            if same {
                layer_steps.last_mut().expect("checked").1.count += 1;
            } else {
                layer_steps.push((PipeId::Sprite, Batch { scale: 1.0, mesh: id, tex: tkey, first: objects.len() as u32, count: 1, cast: false, cut: false, lod: 0, mask: 0xff, splat: None, early: false, role: 0 }));
            }
            objects.push(data);
        }
        for p in among {
            let batch = self.cloud_batch(p, high, soft_on, frame_no, &mut objects);
            layer_steps.push((if p.additive { PipeId::ParticleAdd } else { PipeId::Particle }, batch));
        }
        // the other particle clouds: one object each (the quads are in world space already), after everything, alpha-blended ones first, then the added light
        let (mut particle_alpha, mut particle_add): (Vec<Batch>, Vec<Batch>) = (Vec::new(), Vec::new());
        for p in scene.particles.iter().filter(|p| p.slot.is_none()) {
            let batch = self.cloud_batch(p, high, soft_on, frame_no, &mut objects);
            if p.additive {
                particle_add.push(batch);
            } else {
                particle_alpha.push(batch);
            }
        }
        // ---- point lights that cast shadows: the two that matter most (bright, close, with a long reach), each with the casters in its range
        let mut point_sel: Vec<usize> = Vec::new();
        let mut point_batches: Vec<[Vec<Batch>; 6]> = Vec::new();
        let mut point_frs: Vec<[Frustum; 6]> = Vec::new();
        if high && q.point_shadow_size > 0 && env.shadows && !casters.spheres.is_empty() {
            let mut cand: Vec<(f32, usize)> = lights_sel
                .iter()
                .enumerate()
                .take(MAX_LIGHTS)
                .filter(|(_, l)| l.point && l.range > 0.05 && l.color[0] + l.color[1] + l.color[2] > 0.0)
                .map(|(i, l)| {
                    let d = length(sub(l.vec, scene.cam.pos));
                    ((l.color[0] + l.color[1] + l.color[2]) * l.range / (1.0 + d * d), i)
                })
                .collect();
            cand.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
            let shadowed = if high && env.lighting[7] > 0.5 { (env.lighting[8] as usize).min(POINT_SHADOWS) } else { 2 };
            let objects_before = objects.len();
            let mut in_range_total = 0usize;
            for &(_, li) in cand.iter().take(shadowed) {
                let l = &lights_sel[li];
                // the casters in the light's range, and for each of the six faces of its shadow cube only those that the face (a 94 degree view) can see
                let in_range: Vec<usize> = order
                    .iter()
                    .copied()
                    .filter(|&i| {
                        let it = &items[i];
                        let (c, r) = it.sphere;
                        let d = length(sub(c, l.vec));
                        it.cast && d <= l.range + r && !(it.glows && d < r)
                    })
                    .collect();
                let faces = point_light_faces(l.vec, 0.1f32.min(l.range * 0.25), l.range);
                let mut per_face: [Vec<Batch>; 6] = Default::default();
                let frs: [Frustum; 6] = std::array::from_fn(|f| Frustum::from_view_proj(&faces[f], true));
                for f in 0..6 {
                    let fr = frs[f];
                    let batches = &mut per_face[f];
                    for &i in &in_range {
                        let it = &items[i];
                        let (c, r) = it.sphere;
                        if self.culling && !fr.sphere_visible(c, r) {
                            continue;
                        }
                        let info = &infos[it.mat as usize];
                        if batches.last().is_some_and(|b| b.mesh == info.id) {
                            batches.last_mut().expect("checked").count += 1;
                        } else {
                            batches.push(Batch { scale: 1.0, mesh: info.id, tex: info.tkey, first: objects.len() as u32, count: 1, cast: true, cut: false, lod: it.lod, mask: 0xff, splat: None, early: false, role: 0 });
                        }
                        objects.push(build(it, info));
                    }
                }
                in_range_total += in_range.len();
                point_sel.push(li);
                point_batches.push(per_face);
                point_frs.push(frs);
            }
            self.last_point_shadows = (point_sel.len(), in_range_total, objects.len() - objects_before);
        } else {
            self.last_point_shadows = (0, 0, 0);
        }
        // grass: an object (the material numbers) for each field and a uniform for each of its three rings, which cover the ground round the camera from fine tufts near to big ones far
        struct GrassDraw {
            mesh: MeshId,
            tex: TexKey,
            hkey: usize,
            mkey: usize,
            ubuf: usize,
            /// Runs of instances (first, count): the tiles of the ring that the view can see.
            ranges: Vec<(u32, u32)>,
        }
        let mut grass_draws: Vec<GrassDraw> = Vec::new();
        let mut grass_us: Vec<GrassU> = Vec::new();
        if high {
            let eye = scene.cam.pos;
            let grass_fr = Frustum::from_view_proj(&to_gpu_clip(&scene.view_proj), false);
            for g in &scene.grass {
                // its picture of the ground's heights (and of the layer it grows on) and its mesh are on the card, or on their way: the field waits for them
                if self.async_prep {
                    let mut all = self.mesh_ready(&g.mesh, frame_no);
                    all &= self.plain_ready(PlainSrc::Heights(g.heights.clone()), frame_no);
                    if let Some(m) = &g.mask {
                        all &= self.plain_ready(PlainSrc::Heights(m.clone()), frame_no);
                    }
                    if let Some(b) = &g.blades {
                        for m in &b.meshes {
                            all &= self.mesh_ready(&MeshRef::Shared(m.clone()), frame_no);
                        }
                    }
                    if let Some(t) = &g.tex {
                        all &= self.texture_ready(t, true, true, frame_no);
                    }
                    if !all {
                        continue;
                    }
                }
                let id = self.ensure_mesh(&g.mesh, frame_no);
                let (mesh_tex, count) = {
                    let gm = self.mesh(id);
                    (gm.texture.clone(), gm.count)
                };
                if count == 0 {
                    continue;
                }
                let tex = if g.blades.is_some() { None } else { g.tex.clone().or(mesh_tex) };
                let tkey = self.texture_group(tex.as_ref(), None, true, SamplerKind::Aniso, frame_no);
                let hkey = self.ensure_grass_heights(&g.heights, frame_no);
                let mkey = match &g.mask {
                    Some(m) => self.ensure_grass_heights(m, frame_no),
                    None => hkey,
                };
                objects.push(ObjectData {
                    model: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                    vary: [0.0; 4],
                    n0: [1.0, 0.0, 0.0, 0.0],
                    n1: [0.0, 1.0, 0.0, 0.0],
                    n2: [0.0, 0.0, 1.0, 0.0],
                    color: [1.0, 1.0, 1.0, 1.0],
                    mat: [32.0, 0.0, 0.0, 0.92],
                    misc: [1.0, 1.0, if tex.is_some() { 1.0 } else { 0.0 }, 1.0],
                    uvrect: [0.0, 0.0, 1.0, 1.0],
                    ids: [0, 3, 0, 0],
                    pbr: [0.0; 4],
                });
                let slot = (objects.len() - 1) as f32;
                let near_cell = 1.0 / g.density.sqrt();
                // single blades: three rings of 7, 4 and 2 pairs of vertices, the near one short (the blades are small), the far ones wider (a thinner field is as full)
                let edges = if g.blades.is_some() { [0.0, g.radius * 0.14, g.radius * 0.4, g.radius] } else { [0.0, g.radius * 0.26, g.radius * 0.62, g.radius] };
                for k in 0..3usize {
                    let cell = near_cell * 2.4f32.powi(k as i32);
                    let outer = edges[k + 1];
                    let inner = if k == 0 { 0.0 } else { edges[k] * 0.8 };
                    // a whole number of 16 x 16 tiles (the shader numbers the instances tile by tile)
                    let cells = ((((2.0 * outer / cell).ceil() as u32 + 2).min(640)).div_ceil(16) * 16).min(640);
                    let mult = if g.blades.is_some() { 2.4f32.powf(1.2 * k as f32) } else { 2.4f32.powf(0.85 * k as f32) };
                    let ring_mesh = match &g.blades {
                        Some(b) => self.ensure_mesh(&MeshRef::Shared(b.meshes[k].clone()), frame_no),
                        None => id,
                    };
                    let (bl, bl2, cb0, ct0) = match &g.blades {
                        Some(b) => ([b.height[0], b.height[1], b.width, b.clump], [b.bend, b.clump_size, BLADES_PER_BUNDLE as f32, 1.0], [b.base[0], b.base[1], b.base[2], 0.0], [b.tip[0], b.tip[1], b.tip[2], 0.0]),
                        None => ([0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4]),
                    };
                    let ox = (eye[0] / cell).floor() - (cells / 2) as f32;
                    let oz = (eye[2] / cell).floor() - (cells / 2) as f32;
                    grass_us.push(GrassU {
                        ring: [cell, inner, outer, cells as f32],
                        orig: [ox, oz, slot, ((outer - inner) * 0.18).max(0.5)],
                        tpos: [g.terrain_pos[0], g.terrain_pos[1], g.terrain_pos[2], g.terrain_height],
                        tsize: [g.terrain_size[0], g.terrain_size[1], g.heights.cols as f32, g.heights.rows as f32],
                        wind: g.wind,
                        tuft: [g.size[0] * mult, g.size[1] * mult, g.patch_size, g.seed + 17.0 * k as f32],
                        ca: [g.color_a[0], g.color_a[1], g.color_a[2], 0.0],
                        cb: [g.color_b[0], g.color_b[1], g.color_b[2], 0.0],
                        eye: [eye[0], eye[1], eye[2], 0.0],
                        blade: bl,
                        blade2: bl2,
                        cbase: cb0,
                        ctip: ct0,
                        mask: [if g.mask.is_some() { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
                    });
                    // the tiles the view can see: a box round each (a tuft is within a cell and a half of its lattice point, and a clump pulls it towards the middle of the clump; the tallest blade, the wind and the ground's own
                    // height make the box as high as they reach), seen from the camera, and not wholly in the hole in the middle of the ring (what is nearer than `inner` is the finer ring's)
                    let per_row = cells / 16;
                    let pull = if g.blades.is_some() { g.blades.as_ref().map_or(0.0, |b| b.clump_size) } else { 0.0 };
                    let reach_y = g.blades.as_ref().map_or(g.size[1] * mult * 1.3, |b| b.height[1] * 1.7) + 1.0;
                    let (y_lo, y_hi) = (g.terrain_pos[1] + g.terrain_height.min(0.0) - 0.5, g.terrain_pos[1] + g.terrain_height.max(0.0) + reach_y);
                    let mut ranges: Vec<(u32, u32)> = Vec::new();
                    for tile in 0..per_row * per_row {
                        let (tx, tz) = ((tile % per_row) as f32, (tile / per_row) as f32);
                        let lo = [(ox + tx * 16.0 - 0.6) * cell - pull - 1.0, (oz + tz * 16.0 - 0.6) * cell - pull - 1.0];
                        let hi = [(ox + tx * 16.0 + 16.0 + 1.6) * cell + pull + 1.0, (oz + tz * 16.0 + 16.0 + 1.6) * cell + pull + 1.0];
                        // the nearest and the farthest point of the box (on the ground) from the camera
                        let nearest = [eye[0].clamp(lo[0], hi[0]) - eye[0], eye[2].clamp(lo[1], hi[1]) - eye[2]];
                        let farthest = [(lo[0] - eye[0]).abs().max((hi[0] - eye[0]).abs()), (lo[1] - eye[2]).abs().max((hi[1] - eye[2]).abs())];
                        let (near_d, far_d) = ((nearest[0] * nearest[0] + nearest[1] * nearest[1]).sqrt(), (farthest[0] * farthest[0] + farthest[1] * farthest[1]).sqrt());
                        let seen = near_d <= outer + 1.0 && far_d >= inner && grass_fr.aabb_visible([lo[0], y_lo, lo[1]], [hi[0], y_hi, hi[1]]);
                        if !seen {
                            continue;
                        }
                        match ranges.last_mut() {
                            Some(last) if last.0 + last.1 == tile * 256 => last.1 += 256,
                            _ => ranges.push((tile * 256, 256)),
                        }
                    }
                    grass_draws.push(GrassDraw { mesh: ring_mesh, tex: tkey, hkey, mkey, ubuf: grass_us.len() - 1, ranges });
                }
            }
        }
        while self.grass_bufs.len() < grass_us.len() {
            self.grass_bufs.push(self.gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("grass-ring"), size: std::mem::size_of::<GrassU>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }));
        }
        for (i, u) in grass_us.iter().enumerate() {
            self.gpu.queue.write_buffer(&self.grass_bufs[i], 0, bytemuck::bytes_of(u));
        }
        lap("u: shadow casters and grass", &mut cpu);
        self.grow_objects(objects.len().max(1) as u64);
        if !objects.is_empty() {
            self.gpu.queue.write_buffer(&self.objects, 0, bytemuck::cast_slice(&objects));
        }
        lap("u: the objects", &mut cpu);

        let text_slots: Vec<u32> = scene.texts.iter().map(|t| {
            slots.push(t.uid);
            slots.len() as u32
        }).collect();
        self.slot_uids = slots;

        // ---- which effects run
        let have_clear = !clear_batches.is_empty();
        let have_meshes = !scene.meshes.is_empty();
        let ssao_on = high && q.ssao && env.ssao > 0.0 && have_meshes;
        let dof_on = high && q.dof && env.dof > 0.0 && have_meshes;
        // the depth prepass feeds both the ambient occlusion and the depth of field
        // picture effects that look at the depth: light shafts (through the sky) and motion blur (from the camera's own movement)
        let shafts_on = high && q.dof && env.shafts > 0.0 && have_meshes && sun.is_some();
        let blur_on = high && q.dof && env.motion_blur > 0.0 && have_meshes && self.prev_vp.is_some();
        // screen-space reflections look at the depth and the finished picture: the same tier as the other picture effects (high and ultra)
        let refl_on = high && q.dof && have_meshes && env.ssr.is_some();
        if refl_on {
            self.ensure_ssr_targets(w, h);
        }
        // the lens flare looks at the depth where the sun is, to see whether something covers it
        let flare_on = high && q.dof && have_meshes && sun.is_some() && env.lens_flare.is_some();
        let vol_on = high && env.volumetrics[0] > 0.0 && have_meshes;
        let gi_on = high && q.ssao && env.gi[0] > 0.0 && have_meshes && !probe_out;
        let ao_pass = ssao_on || gi_on;
        // the two ao textures take turns; the bounce is blended with the last pictures' (the first picture after the targets were made is all new)
        let gi_alpha = if gi_on { (1.0 / (self.gi_age as f32 + 1.0)).max(0.12) } else { 1.0 };
        // the first picture of a run has no last picture to read the light from (what the targets hold is from some other run): it reads black
        let gi_seen = if self.gi_age == 0 { 0.0 } else { 1.0 };
        if ao_pass {
            self.gi_flip ^= 1;
        }
        if gi_on {
            self.gi_age = (self.gi_age + 1).min(1000);
        } else {
            self.gi_age = 0;
        }
        let gi_idx = self.gi_flip;
        let taa_wants = high && q.taa && !probe_out && have_meshes;
        let water_deep = high && !probe_out && scene.meshes.iter().any(|m| m.water_look.is_some());
        let need_depth = water_deep || soft_on || taa_wants || ao_pass || dof_on || shafts_on || blur_on || refl_on || flare_on || vol_on;
        let bloom_on = high && q.bloom && env.bloom > 0.0;
        // eye adaptation needs something to measure: a flat 2D scene keeps its authored colours
        let ae_on = high && have_meshes && env.auto_exposure.is_some() && !probe_out;

        // ---- the frame uniform
        let taa_on = high && q.taa && !probe_out && have_meshes;
        let vp_unj = to_gpu_clip(&scene.view_proj);
        let mut vp = vp_unj;
        // (a picture that is asked for the entity at each pixel is drawn without the shift, so that the ids are those of the picture's grid)
        if taa_on && !want_ids {
            // the pixel grid is shifted by a point of the Halton sequence (2, 3), up to half a pixel, a different one every picture
            let halton = |mut i: u32, b: u32| -> f32 {
                let (mut f, mut r) = (1.0f32, 0.0f32);
                while i > 0 {
                    f /= b as f32;
                    r += f * (i % b) as f32;
                    i /= b;
                }
                r
            };
            let k = self.taa_index % 8 + 1;
            let (jx, jy) = (halton(k, 2) - 0.5, halton(k, 3) - 0.5);
            let mut shift = lunaris_render::math::M4::identity();
            shift.0[12] = jx * 2.0 / w as f32;
            shift.0[13] = -jy * 2.0 / h as f32;
            vp = shift.mul(&vp_unj);
        }
        let view = scene.view;
        let lin = |c: V3| if high { display_to_linear(c) } else { c };
        let mut fu: FrameU = Zeroable::zeroed();
        fu.view_proj = vp.0;
        fu.inv_view_proj = inverse(&vp).0;
        fu.view = view.0;
        if let Some(c) = &cascades {
            for i in 0..3 {
                fu.cascade_vp[i] = c[i].view_proj.0;
                self.gpu.queue.write_buffer(&self.pass_bufs[i + 1], 0, bytes_of_floats(&c[i].view_proj.0));
            }
            fu.cascade_info = [c[0].far, c[1].far, c[2].far, q.shadow_size.clamp(256, 8192) as f32];
            fu.cascade_texel = [c[0].texel, c[1].texel, c[2].texel, 0.0];
            fu.cascade_depth = [c[0].depth, c[1].depth, c[2].depth, env.shadow_soft];
        }
        self.gpu.queue.write_buffer(&self.pass_bufs[0], 0, bytes_of_floats(&vp.0));
        let psize = q.point_shadow_size.clamp(128, 4096);
        if !point_sel.is_empty() {
            self.ensure_point_shadow(psize, point_sel.len());
        }
        for k in 0..POINT_SHADOWS {
            fu.psh[k].info = [-1.0, 0.1, 1.0, 0.0];
        }
        for (k, &li) in point_sel.iter().enumerate() {
            let l = &lights_sel[li];
            let near = 0.1f32.min(l.range * 0.25);
            let faces = point_light_faces(l.vec, near, l.range);
            for f in 0..6 {
                fu.psh[k].vp[f] = faces[f].0;
                self.gpu.queue.write_buffer(&self.pass_bufs[4 + k * 6 + f], 0, bytes_of_floats(&faces[f].0));
            }
            // the world size of one shadow texel at distance 1 from the light: the faces are 94 degrees wide
            fu.psh[k].info = [li as f32, near, l.range, 2.0 * 47.0f32.to_radians().tan() / psize as f32];
        }
        fu.eye = v4(scene.cam.pos, scene.cam.near);
        if let Some(i) = sun {
            fu.sun_dir = v4(lights_sel[i].vec, 1.0);
            fu.sun_color = v4(lights_sel[i].color, 0.0);
        }
        fu.sky_top = v4(env.atmo_linear.map_or_else(|| lin(env.sky_top), |a| a[0]), 0.0);
        fu.sky_horizon = v4(env.atmo_linear.map_or_else(|| lin(env.sky_horizon), |a| a[1]), 0.0);
        fu.p25 = [env.atmo[1], env.atmo[2], env.atmo[3], env.atmo[4]];
        fu.p26 = [if env.atmo[0] > 0.5 && high { 1.0 } else { 0.0 }, env.atmo[5], env.rain, self.rain_clock.elapsed().as_secs_f32() % 3600.0];
        // the drops of the rain (`Weather.rain`: how hard it falls is how many are drawn)
        let rain_drops: u32 = if env.rain > 0.02 { (env.rain.min(1.0) * 36_000.0) as u32 } else { 0 };
        fu.ground = v4(lin(env.ground), 0.0);
        fu.fog = v4(env.atmo_linear.map_or_else(|| lin(env.fog_color), |a| a[2]), env.fog);
        {
            let c = scene.cam.clear;
            fu.clear_color = v4(lin([c[0], c[1], c[2]]), 0.0);
        }
        let tonemap = match env.tonemap {
            0 => if have_meshes { 1.0 } else { 3.0 },
            n => n as f32,
        };
        fu.p0 = [env.ambient, env.exposure, if ssao_on { env.ssao } else { 0.0 }, if shadows_on { 1.0 } else { 0.0 }];
        {
            // `Lighting`: fill and bounce; the lamps' falloff blended towards inverse-square light, scaled to match the old curve where the two meet (at 60% of the range)
            let lt = env.lighting;
            if high && lt[7] > 0.5 {
                let (k, c) = (0.6f32, lt[5].max(0.01));
                let gain = (1.0 - k * k).powi(2) / ((1.0 - k.powi(4)).powi(2) * (c * c / (k * k + c * c)));
                fu.p31 = [lt[0], lt[1], lt[2], lt[3]];
                fu.p32 = [lt[4], c, gain, 0.0];
            } else {
                fu.p31 = [0.0; 4];
                fu.p32 = [0.0, 0.12, 1.0, 0.0];
            }
        }
        fu.p1 = [w as f32, h as f32, 1.0 / w as f32, 1.0 / h as f32];
        fu.p2 = [if high { 1.0 } else { 0.0 }, lights_sel.len().min(MAX_LIGHTS) as f32, tonemap, env.saturation];
        fu.p3 = [if bloom_on { env.bloom } else { 0.0 }, env.bloom_threshold, env.vignette, if env.sky { 1.0 } else { 0.0 }];
        let (depth_k1, depth_k2) = view_depth_constants(&scene.proj);
        fu.p5 = [if env.sky { env.clouds } else { 0.0 }, psize as f32, depth_k1, depth_k2];
        // depth of field: strength, focus distance, the depth that stays sharp around it
        fu.p6 = [if dof_on { env.dof } else { 0.0 }, env.focus, env.focus_range, 0.0];
        self.fx_frame = self.fx_frame.wrapping_add(1);
        fu.p19 = if high { [env.lens[0], env.lens[1], env.lens[2], env.lens[3]] } else { [0.0, 1.0, 0.0, 0.0] };
        fu.p20 = if high { [env.lens[4], env.lens[5], 0.0, 0.0] } else { [1.0, 0.0, 0.0, 0.0] };
        fu.p21 = if vol_on { [env.volumetrics[0], env.volumetrics[1], env.volumetrics[2], env.volumetrics[3]] } else { [0.0, 0.6, 24.0, 120.0] };
        fu.p22 = if vol_on { [env.volumetrics[4], env.volumetrics[5], env.volumetrics[6], env.volumetrics[7]] } else { [1.0, 1.0, 1.0, 0.0] };
        fu.p23 = if gi_on { env.gi } else { [0.0, 6.0, 10.0, 0.6] };
        fu.p24 = [gi_alpha, gi_seen, 0.0, 0.0];
        fu.p7 = [if shafts_on { env.shafts } else { 0.0 }, if high { env.grain } else { 0.0 }, if high { env.contrast } else { 1.0 }, if blur_on { env.motion_blur } else { 0.0 }];
        fu.p8 = [if high { env.tint[0] } else { 1.0 }, if high { env.tint[1] } else { 1.0 }, if high { env.tint[2] } else { 1.0 }, (self.fx_frame % 4096) as f32];
        // colour grading and the fade (the high qualities only; the identity otherwise)
        fu.p9 = if high { env.grade_a } else { [0.0; 4] };
        fu.p10 = if high { env.grade_b } else { [1.0, 1.0, 1.0, 0.0] };
        fu.p11 = if high { env.grade_c } else { [1.0, 1.0, 1.0, 0.0] };
        fu.p12 = if high { v4(lin([env.fade[0], env.fade[1], env.fade[2]]), env.fade[3]) } else { [0.0; 4] };
        // height fog: density, falloff, the level of the thickest fog
        fu.p13 = if high { [env.hfog[0], env.hfog[1], env.hfog[2], 0.0] } else { [0.0; 4] };
        // eye adaptation: min, max, speed, target; then on, seconds since the last picture, start from the measured value
        let ae_reset = adapt.is_none() || !self.ae_valid;
        fu.p14 = if ae_on { env.auto_exposure.unwrap_or([0.0; 4]) } else { [0.0; 4] };
        fu.p15 = [if ae_on { 1.0 } else { 0.0 }, adapt.unwrap_or(0.0), if ae_reset { 1.0 } else { 0.0 }, 0.0];
        // screen-space reflections: strength, the farthest a ray goes, how thick things are taken to be, on
        fu.p16 = match env.ssr {
            Some(s) if refl_on => [s[0], s[1], s[2], 1.0],
            _ => [0.0; 4],
        };
        // lens flare: intensity, ghosts, halo, on
        fu.p17 = match env.lens_flare {
            Some(f) if flare_on => [f[0], f[1], f[2], 1.0],
            _ => [0.0; 4],
        };
        // reflection probes: how many are baked and how many levels their faces have; then each one's middle and half its box
        let mut probes_baked = 0usize;
        if high && !probe_out {
            if let Some(set) = &self.probe_set {
                for (k, key) in set.baked.iter().enumerate() {
                    if let Some(key) = key {
                        fu.probes[k * 2] = v4(key.pos, 0.0);
                        fu.probes[k * 2 + 1] = v4(key.half, 0.0);
                        probes_baked = k + 1;
                    }
                }
                fu.p18 = [probes_baked as f32, set.levels as f32, 0.0, 0.0];
            }
        }
        fu.prev_view_proj = self.prev_vp.unwrap_or(vp.0);
        fu.unj_vp = vp_unj.0;
        fu.unj_prev_vp = self.prev_unj_vp.unwrap_or(vp_unj.0);
        let taa_alpha = if taa_on { (1.0 / (self.taa_age as f32 + 1.0)).max(0.1) } else { 1.0 };
        fu.p27 = [taa_alpha, if taa_on { 1.0 } else { 0.0 }, if high { env.wet } else { 0.0 }, 0.0];
        // the first sea of the picture (a water mesh with a look): its level and clock reach the terrain's shader (wet sand, caustics)
        let sea = scene.meshes.iter().find(|m| m.water_look.is_some());
        fu.p28 = [if water_deep { 1.0 } else { 0.0 }, sea.map_or(0.0, |m| m.model.0[13]), 0.7, sea.and_then(|m| m.water).map_or(0.0, |w| w[2])];
        fu.p29 = sea.and_then(|m| m.water).map_or([0.0; 4], |w| [w[0], w[1], 0.0, 0.0]);
        // the depth of the sea round the eye, for the waves to shoal and break by (the window is made again when the eye has moved by a cell)
        if water_deep && !scene.floors.is_empty() {
            let level = sea.map_or(0.0, |m| m.model.0[13]);
            let eye = scene.cam.pos;
            let (ix, iz) = ((eye[0] / SEA_CELL).floor() as i64, (eye[2] / SEA_CELL).floor() as i64);
            let key = (ix, iz, level.to_bits(), Arc::as_ptr(&scene.floors[0].heights) as usize);
            let origin = [(ix - SEA_DIM as i64 / 2) as f32 * SEA_CELL, (iz - SEA_DIM as i64 / 2) as f32 * SEA_CELL];
            if !matches!(&self.sea_cache, Some((k, _)) if *k == key) {
                self.sea_cache = Some((key, sea_floor_window(&scene.floors, level, origin)));
            }
            if let Some((_, w)) = &self.sea_cache {
                fu.sea = **w;
            }
            fu.p30 = [origin[0], origin[1], 1.0 / SEA_CELL, 1.0];
        }
        if !probe_out {
            self.prev_unj_vp = Some(vp_unj.0);
        }
        if taa_on {
            self.taa_age = (self.taa_age + 1).min(1000);
            self.taa_index = self.taa_index.wrapping_add(1);
            self.taa_flip ^= 1;
        } else {
            self.taa_age = 0;
        }
        let taa_idx = self.taa_flip;
        self.prev_vp = Some(vp.0);
        fu.p4 = [if shadows_on { sun.expect("checked") as f32 } else { -1.0 }, 1.0, samples as f32, if scene.cam.ortho.is_some() { 1.0 } else { 0.0 }];
        for (i, l) in lights_sel.iter().take(MAX_LIGHTS).enumerate() {
            fu.lights[i * 4] = v4(l.vec, if l.point { 1.0 } else { 0.0 });
            fu.lights[i * 4 + 1] = v4(l.color, l.range);
            // the classic mode draws every spot light as a point light, like the reference renderer
            fu.lights[i * 4 + 2] = v4(l.spot_dir, if high { l.cos_outer } else { -2.0 });
            fu.lights[i * 4 + 3] = [l.cos_inner, 0.0, 0.0, 0.0];
        }
        lap("u: passes and the frame's numbers", &mut cpu);
        self.gpu.queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&fu));

        // ---- text
        let (mut verts, ui_batches) = ui_vertices(scene, w as f32, h as f32, !high);
        let ui_n = (verts.len() / 8) as u32;
        verts.extend(text_vertices(scene, w as f32, h as f32, !high, None));
        // the interface shapes come first in the buffer, then the text, then the text of the entity-id picture
        let nverts = (verts.len() / 8) as u32;
        let ui_keys = self.ui_groups(&ui_batches, frame_no);
        let id_verts = if want_ids { text_vertices(scene, w as f32, h as f32, false, Some(&text_slots)) } else { Vec::new() };
        let n_id_verts = (id_verts.len() / 8) as u32;
        verts.extend_from_slice(&id_verts);
        if nverts > 0 {
            self.grow_text((nverts + n_id_verts) as u64);
            self.gpu.queue.write_buffer(&self.text_vb, 0, bytes_of_floats(&verts));
            self.gpu.queue.write_buffer(&self.text_u, 0, bytes_of_floats(&[w as f32, h as f32, 0.0, 0.0]));
        }

        // ---- pipelines this picture needs
        if want_ids {
            self.need(PipeId::IdMesh, 1, None)?;
            self.need(PipeId::IdSprite, 1, None)?;
            if nverts > 0 {
                self.need(PipeId::IdText, 1, None)?;
            }
            self.ensure_id_targets(w, h);
        }
        self.need(PipeId::Mesh, samples, None)?;
        if zpre_on {
            self.need(PipeId::MeshEarly, samples, None)?;
            self.need(PipeId::DepthEarly, samples, None)?;
        }
        self.need(PipeId::Sprite, samples, None)?;
        if !particle_alpha.is_empty() || layer_steps.iter().any(|s| s.0 == PipeId::Particle) {
            self.need(PipeId::Particle, samples, None)?;
        }
        if rain_drops > 0 {
            self.need(PipeId::Rain, samples, None)?;
        }
        if !particle_add.is_empty() || layer_steps.iter().any(|s| s.0 == PipeId::ParticleAdd) {
            self.need(PipeId::ParticleAdd, samples, None)?;
        }
        if have_clear {
            self.need(PipeId::MeshBlend, samples, None)?;
        }
        if !splat_bgs.is_empty() && self.splat_tex_bg.is_none() {
            let bg = self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("splat-texture"),
                layout: &self.layouts.tex1_small,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&self.white[1]) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.samplers.aniso) }, wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.white_height) }],
            });
            self.splat_tex_bg = Some(bg);
        }
        if !splat_bgs.is_empty() {
            self.need(PipeId::MeshSplat, samples, None)?;
        }
        if !grass_draws.is_empty() {
            self.need(PipeId::Grass, samples, None)?;
        }
        let grass_bgs: Vec<wgpu::BindGroup> = grass_draws
            .iter()
            .map(|d| {
                self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("grass"),
                    layout: &self.layouts.grass2,
                    entries: &[wgpu::BindGroupEntry { binding: 12, resource: self.grass_bufs[d.ubuf].as_entire_binding() }, wgpu::BindGroupEntry { binding: 13, resource: wgpu::BindingResource::TextureView(&self.grass_heights[&d.hkey].0) }, wgpu::BindGroupEntry { binding: 14, resource: wgpu::BindingResource::TextureView(&self.grass_heights[&d.mkey].0) }],
                })
            })
            .collect();
        if high {
            self.need(PipeId::Sky, samples, None)?;
            if probe_out {
                self.need(PipeId::ProbeResolve, 1, None)?;
            } else {
                self.need(PipeId::Composite, samples, Some(if taa_on { HDR } else { out_format }))?;
                if taa_on {
                    self.need(PipeId::Taa, 1, Some(out_format))?;
                }
            }
            if nverts > ui_n && !probe_out {
                self.need(PipeId::TextOut, 1, Some(out_format))?;
            }
            if ui_n > 0 && !probe_out {
                self.need(PipeId::UiOut, 1, Some(out_format))?;
            }
            if shadows_on || !point_sel.is_empty() {
                self.need(PipeId::DepthShadow, 1, None)?;
            }
            if need_depth {
                self.need(PipeId::DepthPre, 1, None)?;
            }
            if have_cut && (shadows_on || !point_sel.is_empty()) {
                self.need(PipeId::DepthShadowCut, 1, None)?;
            }
            if have_cut && need_depth {
                self.need(PipeId::DepthPreCut, 1, None)?;
            }
            if refl_on {
                self.need(PipeId::SsrGbuf, 1, None)?;
                self.need(PipeId::Ssr, 1, None)?;
            }
            if ao_pass {
                self.need(PipeId::Ssao, 1, None)?;
                self.need(PipeId::SsaoBlur, 1, None)?;
            }
            if gi_on {
                self.need(PipeId::Gi, 1, None)?;
            }
            if bloom_on {
                self.need(PipeId::BloomFirst, 1, None)?;
                self.need(PipeId::BloomDown, 1, None)?;
                self.need(PipeId::BloomUp, 1, None)?;
            }
        } else {
            self.need(PipeId::Resolve, 1, Some(out_format))?;
            if nverts > ui_n {
                self.need(PipeId::TextInPass, 1, None)?;
            }
            if ui_n > 0 {
                self.need(PipeId::UiInPass, 1, None)?;
            }
        }

        // ---- bind groups that depend on this frame's buffers
        let ao_view = match (&self.targets, ao_pass) {
            (Some(t), true) => t.ao.as_ref().map(|a| &a[gi_idx]).unwrap_or(&self.ao_white),
            _ => &self.ao_white,
        };
        let shadow_view = match (&self.shadow, shadows_on) {
            (Some(s), true) => &s.array_view,
            _ => &self.shadow_dummy,
        };
        let point_view = match (&self.point_shadow, point_sel.is_empty()) {
            (Some(s), false) => &s.array_view,
            _ => &self.shadow_dummy,
        };
        let probe_view = match (&self.probe_set, probes_baked > 0) {
            (Some(s), true) => &s.array_view,
            _ => &self.probe_dummy,
        };
        lap("u: text, particles", &mut cpu);
        let opaque_depth = match (&self.targets, water_deep || soft_on) {
            (Some(t), true) => t.pre_depth.as_ref().unwrap_or(&self.depth_dummy),
            _ => &self.depth_dummy,
        };
        let scene_bg = self.group(
            "scene",
            &self.layouts.scene0,
            &[
                wgpu::BindGroupEntry { binding: 0, resource: self.frame_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.objects.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(shadow_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.samplers.shadow) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(ao_view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.samplers.linear_clamp) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(point_view) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(probe_view) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&self.samplers.probe) },
                wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(opaque_depth) },
                wgpu::BindGroupEntry { binding: 15, resource: self.sky_lut.as_entire_binding() },
            ],
        );
        let depth_bgs: Vec<wgpu::BindGroup> = (0..4 + 6 * point_sel.len()).map(|i| self.group("depth", &self.layouts.depth0, &[wgpu::BindGroupEntry { binding: 0, resource: self.pass_bufs[i].as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: self.objects.as_entire_binding() }])).collect();

        lap("u: bind groups", &mut cpu);
        // ---- where the picture ends up
        let image_view;
        let out_view: &wgpu::TextureView = match &out {
            Out::Image => {
                self.ensure_out_image(w, h);
                self.ensure_readback((w * 4).div_ceil(256) as u64 * 256 * h as u64);
                image_view = self.out_image.as_ref().expect("just made").2.create_view(&wgpu::TextureViewDescriptor::default());
                &image_view
            }
            Out::Surface { view, .. } => view,
            Out::Probe { view } => view,
        };

        lap("uniforms+upload", &mut cpu);
        // ---- encode
        let t = self.targets.as_ref().expect("targets exist");
        self.prof.begin();
        let mut enc = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        // the animated models' vertices are moved by their joints before anything draws them
        self.skin.encode(&self.gpu.device, &self.gpu.queue, &self.layouts.skin0, &mut enc);
        // and the particles of the emitters the card simulates are stepped and turned into pictures for this view
        if !self.fx.tasks.is_empty() {
            let inv = scene.view.inverse_affine();
            let norm = |v: V3| {
                let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
                [v[0] / l, v[1] / l, v[2] / l]
            };
            let cam = crate::fx_gpu::Cam { pos: scene.cam.pos, forward: norm(inv.transform_dir([0.0, 0.0, -1.0])), right: norm(inv.transform_dir([1.0, 0.0, 0.0])), up: norm(inv.transform_dir([0.0, 1.0, 0.0])) };
            self.fx.encode(&self.gpu.device, &self.gpu.queue, &self.layouts.fx0, &mut enc, &cam);
        }
        let shadow_texel: [f32; 3] = cascades.as_ref().map_or([0.0; 3], |c| [c[0].texel, c[1].texel, c[2].texel]);
        let draw_count = std::cell::Cell::new(0u32);
        let draw_batches = |pass: &mut wgpu::RenderPass<'_>, batches: &[Batch], only_casters: bool, textured: bool, cull: Option<&Frustum>, cut_filter: Option<bool>, bit: u8| {
            for b in batches {
                // (`bit` is this pass's own: 1 the camera's, 2 to 8 the cascades; a batch holds only the objects that the same passes see)
                if (only_casters && !b.cast) || cut_filter.is_some_and(|c| c != b.cut) || b.mask & bit == 0 {
                    continue;
                }
                let g = self.mesh(b.mesh);
                if g.count == 0 {
                    continue;
                }
                let end = b.first + b.count;
                let mut bound = false;
                // a shadow cascade (bit 2, 4 or 8) draws a level as coarse as its texels allow, never a finer one than the picture's
                let lod = if bit & 1 == 0 && bit != 0 && bit != 0xff && g.lods.len() > 1 {
                    (b.lod as usize).max(choose_lod(&g.lods, shadow_texel[(bit.trailing_zeros() as usize).saturating_sub(1).min(2)] * SHADOW_LOD_TEXELS, b.scale, 1.0, g.min_lod))
                } else {
                    b.lod as usize
                };
                let range = g.lods.get(lod).map_or(0..g.count, |l| l.first..l.first + l.count);
                // a big mesh that is drawn once or a few times: only the clusters of it that the view can see (with many copies the whole mesh is drawn instanced: the cost is then the copies'); the coarser levels are whole
                if let (Some(f), false, true, 0) = (cull, g.clusters.is_empty(), b.count <= 4, lod) {
                    for inst in b.first..end {
                        // (the objects that come after the two lists, such as a point light's own copies, have no sphere: their pieces are tested all the same)
                        if obj_sphere.get(inst as usize).is_some_and(|sp| !f.sphere_visible(sp.0, sp.1)) {
                            continue;
                        }
                        draw_count.set(draw_count.get() + 1);
                        if !bound {
                            if textured {
                                pass.set_bind_group(1, &self.tex_bgs[&b.tex], &[]);
                            }
                            pass.set_vertex_buffer(0, g.vbuf.slice(..));
                            pass.set_index_buffer(g.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                            bound = true;
                        }
                        let model = M4(objects[inst as usize].model);
                        let m = &model.0;
                        let scale = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt().max((m[4] * m[4] + m[5] * m[5] + m[6] * m[6]).sqrt()).max((m[8] * m[8] + m[9] * m[9] + m[10] * m[10]).sqrt());
                        let mut run: Option<(u32, u32)> = None;
                        for c in &g.clusters {
                            if f.sphere_visible(model.transform_point(c.center), c.radius * scale) {
                                match &mut run {
                                    Some((_, last)) if *last == c.first => *last = c.first + c.count,
                                    Some((first, last)) => {
                                        pass.draw_indexed(*first..*last, 0, inst..inst + 1);
                                        run = Some((c.first, c.first + c.count));
                                    }
                                    None => run = Some((c.first, c.first + c.count)),
                                }
                            }
                        }
                        if let Some((first, last)) = run {
                            pass.draw_indexed(first..last, 0, inst..inst + 1);
                        }
                    }
                    continue;
                }
                if textured {
                    pass.set_bind_group(1, &self.tex_bgs[&b.tex], &[]);
                }
                pass.set_vertex_buffer(0, g.vbuf.slice(..));
                pass.set_index_buffer(g.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(range, 0, b.first..end);
                draw_count.set(draw_count.get() + 1);
            }
        };
        // the interface shapes in drawing order, each run with its own scissor rectangle when it is inside a scroll view (and the whole picture again after)
        let draw_ui = |pass: &mut wgpu::RenderPass<'_>| {
            let mut cut = false;
            for (b, key) in ui_batches.iter().zip(&ui_keys) {
                match b.scissor {
                    Some([x, y, sw, sh]) => {
                        pass.set_scissor_rect(x, y, sw, sh);
                        cut = true;
                    }
                    None if cut => {
                        pass.set_scissor_rect(0, 0, w, h);
                        cut = false;
                    }
                    None => {}
                }
                pass.set_bind_group(0, &self.ui_bgs[key], &[]);
                pass.draw(b.first..b.first + b.count, 0..1);
            }
            if cut {
                pass.set_scissor_rect(0, 0, w, h);
            }
        };
        let fullscreen = |enc: &mut wgpu::CommandEncoder, label: &str, view: &wgpu::TextureView, load: wgpu::LoadOp<wgpu::Color>, pipe: &wgpu::RenderPipeline, bg: &wgpu::BindGroup| {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: self.prof.writes(label),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, bg, &[]);
            pass.draw(0..3, 0..1);
        };

        // the sky's table for this picture (the atmosphere of the air, when the picture has one)
        if high && env.atmo[0] > 0.5 {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("sky-lut"), timestamp_writes: None });
            pass.set_pipeline(&self.sky_pipe);
            pass.set_bind_group(0, &self.sky_bg, &[]);
            pass.dispatch_workgroups(8, 8, 1);
        }
        let draws_before_shadow = draw_count.get();
        if let (Some(c), Some(s)) = (&cascades, &self.shadow) {
            let _ = c;
            for i in 0..3 {
                let mut pass = depth_pass(&mut enc, &self.prof, "shadow", &s.layers[i], 1.0);
                pass.set_pipeline(self.pipe(PipeId::DepthShadow, 1, None));
                pass.set_bind_group(0, &depth_bgs[i + 1], &[]);
                draw_batches(&mut pass, depth_batches, true, false, shadow_fr[i].as_ref(), Some(false), 2 << i);
                if have_cut {
                    pass.set_pipeline(self.pipe(PipeId::DepthShadowCut, 1, None));
                    draw_batches(&mut pass, depth_batches, true, true, shadow_fr[i].as_ref(), Some(true), 2 << i);
                }
            }
        }
        let draws_shadow = draw_count.get() - draws_before_shadow;
        if let Some(ps) = &self.point_shadow {
            for (k, per_face) in point_batches.iter().enumerate() {
                for f in 0..6 {
                    let mut pass = depth_pass(&mut enc, &self.prof, "point-shadow", &ps.layers[k * 6 + f], 1.0);
                    pass.set_pipeline(self.pipe(PipeId::DepthShadow, 1, None));
                    pass.set_bind_group(0, &depth_bgs[4 + k * 6 + f], &[]);
                    // (with the face's frustum, a big model is drawn only in the pieces that the face can see)
                    draw_batches(&mut pass, &per_face[f], false, false, if self.culling { Some(&point_frs[k][f]) } else { None }, None, 0xff);
                }
            }
        }
        let draws_point = draw_count.get() - draws_before_shadow - draws_shadow;
        let draws_split = [draws_shadow, draws_point];
        if need_depth {
            let pre = t.pre_depth.as_ref().expect("high targets");
            {
                let mut pass = depth_pass(&mut enc, &self.prof, "prepass", pre, 0.0);
                pass.set_pipeline(self.pipe(PipeId::DepthPre, 1, None));
                pass.set_bind_group(0, &depth_bgs[0], &[]);
                draw_batches(&mut pass, depth_batches, false, false, main_fr.as_ref(), Some(false), 1);
                if have_cut {
                    pass.set_pipeline(self.pipe(PipeId::DepthPreCut, 1, None));
                    draw_batches(&mut pass, depth_batches, false, true, main_fr.as_ref(), Some(true), 1);
                }
            }
        }
        if refl_on {
            // every mesh pixel's normal, roughness and reflectance, and the depth of the nearest surface (the water included), for the reflection pass after the main pass
            let st = self.ssr_targets.as_ref().expect("reflection targets");
            fn attach(view: &wgpu::TextureView) -> Option<wgpu::RenderPassColorAttachment<'_>> {
                Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })
            }
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ssr-surfaces"),
                color_attachments: &[attach(&st.gbuf_a), attach(&st.gbuf_b)],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &st.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                timestamp_writes: self.prof.writes("ssr-surfaces"),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &scene_bg, &[]);
            pass.set_pipeline(self.pipe(PipeId::SsrGbuf, 1, None));
            draw_batches(&mut pass, color_batches, false, true, main_fr.as_ref(), None, 1);
            draw_batches(&mut pass, &clear_batches, false, true, main_fr.as_ref(), None, 1);
        }
        if ao_pass {
            let pre = t.pre_depth.as_ref().expect("high targets");
            let _ = pre;
            fullscreen(&mut enc, "ssao", t.ao_raw.as_ref().expect("high targets"), wgpu::LoadOp::Clear(wgpu::Color::WHITE), self.pipe(PipeId::Ssao, 1, None), t.bg_ssao.as_ref().expect("high targets"));
            if gi_on {
                fullscreen(&mut enc, "gi", t.gi_raw.as_ref().expect("high targets"), wgpu::LoadOp::Clear(wgpu::Color::BLACK), self.pipe(PipeId::Gi, 1, None), t.bg_gi.as_ref().expect("high targets"));
            } else {
                // no bounce: the blur reads black
                let _clear = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("gi-clear"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: t.gi_raw.as_ref().expect("high targets"), depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            fullscreen(&mut enc, "ssao-blur", &t.ao.as_ref().expect("high targets")[gi_idx], wgpu::LoadOp::Clear(wgpu::Color::WHITE), self.pipe(PipeId::SsaoBlur, 1, None), &t.bg_blur.as_ref().expect("high targets")[gi_idx]);
        }

        // the opaque meshes' depth, into the main pass's own depth image: the main pass below then runs its (expensive) shaders only for the nearest surface of each pixel
        let early_batches: Vec<Batch> = if zpre_on { depth_batches.iter().filter(|b| b.early).cloned().collect() } else { Vec::new() };
        let early_color: Vec<Batch> = if zpre_on { color_batches.iter().filter(|b| b.early).cloned().collect() } else { Vec::new() };
        if zpre_on {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("early-z"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &t.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                timestamp_writes: self.prof.writes("early-z"),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(self.pipe(PipeId::DepthEarly, samples, None));
            pass.set_bind_group(0, &depth_bgs[0], &[]);
            draw_batches(&mut pass, &early_batches, false, false, main_fr.as_ref(), Some(false), 1);
        }
        // the main pass: sky or flat colour, meshes, then sprites and tile maps far to near, then (classic) the text
        {
            let c = scene.cam.clear;
            let clear = if high {
                let l = display_to_linear([c[0], c[1], c[2]]);
                [l[0], l[1], l[2]]
            } else {
                [c[0] * c[0], c[1] * c[1], c[2] * c[2]]
            };
            let sky = high && env.sky;
            // classic: the flat colour; high quality: transparent, so that the composite can keep the background exactly as authored
            let color_clear = if high { wgpu::Color::TRANSPARENT } else { wgpu::Color { r: clear[0] as f64, g: clear[1] as f64, b: clear[2] as f64, a: 1.0 } };
            let (view, resolve) = match &t.hdr_ms {
                Some(ms) => (ms, Some(&t.hdr)),
                None => (&t.hdr, None),
            };
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: resolve, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(color_clear), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &t.depth, depth_ops: Some(wgpu::Operations { load: if zpre_on { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(0.0) }, store: wgpu::StoreOp::Discard }), stencil_ops: None }),
                timestamp_writes: self.prof.writes("main"),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &scene_bg, &[]);
            if zpre_on {
                // what the early pass drew: the pixels it left are the nearest surface's, shaded once
                pass.set_pipeline(self.pipe(PipeId::MeshEarly, samples, None));
                draw_batches(&mut pass, &early_color, false, true, main_fr.as_ref(), None, 1);
            }
            pass.set_pipeline(self.pipe(PipeId::Mesh, samples, None));
            if splat_bgs.is_empty() && !zpre_on {
                draw_batches(&mut pass, color_batches, false, true, main_fr.as_ref(), None, 1);
                self.prof.mark(&mut pass, "meshes");
            } else {
                let plain: Vec<Batch> = color_batches.iter().filter(|b| b.splat.is_none() && !(zpre_on && b.early)).cloned().collect();
                draw_batches(&mut pass, &plain, false, true, main_fr.as_ref(), None, 1);
                self.prof.mark(&mut pass, "meshes");
                if !splat_bgs.is_empty() {
                    pass.set_pipeline(self.pipe(PipeId::MeshSplat, samples, None));
                    for b in mesh_batches.iter().filter(|b| b.splat.is_some()) {
                        pass.set_bind_group(1, &splat_macro_bgs[b.splat.expect("filtered")], &[]);
                        pass.set_bind_group(2, &splat_bgs[b.splat.expect("filtered")], &[]);
                        draw_batches(&mut pass, std::slice::from_ref(b), false, false, main_fr.as_ref(), None, 1);
                    }
                    self.prof.mark(&mut pass, "terrain");
                    pass.set_pipeline(self.pipe(PipeId::Mesh, samples, None));
                }
            }
            if !grass_draws.is_empty() {
                pass.set_pipeline(self.pipe(PipeId::Grass, samples, None));
                for (i, d) in grass_draws.iter().enumerate() {
                    let gm = self.mesh(d.mesh);
                    pass.set_bind_group(1, &self.tex_bgs[&d.tex], &[]);
                    pass.set_bind_group(2, &grass_bgs[i], &[]);
                    pass.set_vertex_buffer(0, gm.vbuf.slice(..));
                    pass.set_index_buffer(gm.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    for &(first, count) in &d.ranges {
                        pass.draw_indexed(0..gm.count, 0, first..first + count);
                    }
                }
            }
            self.prof.mark(&mut pass, "grass");
            if sky {
                pass.set_pipeline(self.pipe(PipeId::Sky, samples, None));
                pass.draw(0..3, 0..1);
            }
            self.prof.mark(&mut pass, "sky");
            if have_clear {
                pass.set_pipeline(self.pipe(PipeId::MeshBlend, samples, None));
                draw_batches(&mut pass, &clear_batches, false, true, main_fr.as_ref(), None, 1);
            }
            self.prof.mark(&mut pass, "blend");
            pass.set_pipeline(self.pipe(PipeId::Sprite, samples, None));
            let mut current = PipeId::Sprite;
            for (pipe, batch) in &layer_steps {
                if *pipe != current {
                    pass.set_pipeline(self.pipe(*pipe, samples, None));
                    current = *pipe;
                }
                draw_batches(&mut pass, std::slice::from_ref(batch), false, true, None, None, 0xff);
            }
            if !particle_alpha.is_empty() {
                pass.set_pipeline(self.pipe(PipeId::Particle, samples, None));
                draw_batches(&mut pass, &particle_alpha, false, true, None, None, 0xff);
            }
            if !particle_add.is_empty() {
                pass.set_pipeline(self.pipe(PipeId::ParticleAdd, samples, None));
                draw_batches(&mut pass, &particle_add, false, true, None, None, 0xff);
            }
            if rain_drops > 0 {
                pass.set_pipeline(self.pipe(PipeId::Rain, samples, None));
                pass.draw(0..6, 0..rain_drops);
            }
            if !high && ui_n > 0 {
                pass.set_pipeline(self.pipe(PipeId::UiInPass, 1, None));
                pass.set_vertex_buffer(0, self.text_vb.slice(..));
                draw_ui(&mut pass);
            }
            if !high && nverts > ui_n {
                pass.set_pipeline(self.pipe(PipeId::TextInPass, 1, None));
                pass.set_bind_group(0, &self.text_bg, &[]);
                pass.set_vertex_buffer(0, self.text_vb.slice(..));
                pass.draw(ui_n..nverts, 0..1);
            }
        }

        // reflections: from every glossy pixel a ray through the depth of the finished picture, into a layer the composite adds
        if refl_on {
            let st = self.ssr_targets.as_ref().expect("reflection targets");
            let probe_view = match (&self.probe_set, probes_baked > 0) {
                (Some(s), true) => &s.array_view,
                _ => &self.probe_dummy,
            };
            let bg = self.group(
                "ssr",
                &self.layouts.ssr,
                &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.frame_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(t.pre_depth.as_ref().expect("high targets")) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&t.hdr) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.samplers.linear_clamp) },
                    wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(&st.depth) },
                    wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::TextureView(&st.gbuf_a) },
                    wgpu::BindGroupEntry { binding: 11, resource: wgpu::BindingResource::TextureView(&st.gbuf_b) },
                    wgpu::BindGroupEntry { binding: 12, resource: wgpu::BindingResource::TextureView(probe_view) },
                    wgpu::BindGroupEntry { binding: 13, resource: wgpu::BindingResource::Sampler(&self.samplers.probe) },
                    wgpu::BindGroupEntry { binding: 15, resource: self.sky_lut.as_entire_binding() },
                ],
            );
            fullscreen(&mut enc, "ssr", &st.out, wgpu::LoadOp::Clear(wgpu::Color::BLACK), self.pipe(PipeId::Ssr, 1, None), &bg);
        }
        // the composite of this picture: its depth, glow, reflection layer (a black stand-in without reflections) and the state of the eye adaptation
        let composite_bg = if high && !probe_out {
            let b = t.bloom.as_ref().expect("high targets");
            let pre = t.pre_depth.as_ref().expect("high targets");
            let reflections = match (&self.ssr_targets, refl_on) {
                (Some(s), true) => &s.out,
                _ => &self.ssr_dummy,
            };
            let tv = wgpu::BindingResource::TextureView;
            Some(if samples > 1 {
                self.group(
                    "composite-ms",
                    &self.layouts.composite_ms,
                    &[
                        wgpu::BindGroupEntry { binding: 0, resource: self.frame_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: tv(pre) },
                        wgpu::BindGroupEntry { binding: 3, resource: tv(t.hdr_ms.as_ref().expect("multisampled target")) },
                        wgpu::BindGroupEntry { binding: 4, resource: tv(&b.views[0]) },
                        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.samplers.linear_clamp) },
                        wgpu::BindGroupEntry { binding: 7, resource: tv(reflections) },
                        wgpu::BindGroupEntry { binding: 8, resource: self.exposure_buf.as_entire_binding() },
                    ],
                )
            } else {
                self.group(
                    "composite",
                    &self.layouts.composite,
                    &[
                        wgpu::BindGroupEntry { binding: 0, resource: self.frame_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: tv(pre) },
                        wgpu::BindGroupEntry { binding: 2, resource: tv(&t.hdr) },
                        wgpu::BindGroupEntry { binding: 4, resource: tv(&b.views[0]) },
                        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.samplers.linear_clamp) },
                        wgpu::BindGroupEntry { binding: 7, resource: tv(reflections) },
                        wgpu::BindGroupEntry { binding: 8, resource: self.exposure_buf.as_entire_binding() },
                    ],
                )
            })
        } else {
            None
        };
        // the eye adapts to the finished picture before the glow is built from it and the picture is exposed
        let ae_read = ae_on && matches!(out, Out::Image);
        if ae_on {
            let bg = self.group(
                "eye-adaptation",
                &self.layouts.ae0,
                &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.frame_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&t.hdr) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.exposure_buf.as_entire_binding() },
                ],
            );
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("eye-adaptation"), timestamp_writes: None });
                pass.set_pipeline(&self.ae_pipe);
                pass.set_bind_group(0, &bg, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            if ae_read {
                enc.copy_buffer_to_buffer(&self.exposure_buf, 0, &self.exposure_read, 0, 16);
            }
        }
        if high {
            if bloom_on {
                let b = t.bloom.as_ref().expect("high targets");
                fullscreen(&mut enc, "bloom-first", &b.views[0], wgpu::LoadOp::Clear(wgpu::Color::BLACK), self.pipe(PipeId::BloomFirst, 1, None), &b.first);
                for l in 0..b.levels - 1 {
                    fullscreen(&mut enc, "bloom-down", &b.views[l + 1], wgpu::LoadOp::Clear(wgpu::Color::BLACK), self.pipe(PipeId::BloomDown, 1, None), &b.down[l]);
                }
                for l in (0..b.levels - 1).rev() {
                    fullscreen(&mut enc, "bloom-up", &b.views[l], wgpu::LoadOp::Load, self.pipe(PipeId::BloomUp, 1, None), &b.up[l]);
                }
            }
            if probe_out {
                // a face of a reflection probe: the picture as plain light, no exposure, tone mapping or text
                let bg = self.group(
                    "probe-face",
                    &self.layouts.probe_resolve,
                    &[wgpu::BindGroupEntry { binding: 0, resource: self.frame_buf.as_entire_binding() }, wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&t.hdr) }],
                );
                fullscreen(&mut enc, "probe-face", out_view, wgpu::LoadOp::Clear(wgpu::Color::BLACK), self.pipe(PipeId::ProbeResolve, 1, None), &bg);
            } else {
                if taa_on {
                    fullscreen(&mut enc, "composite", t.taa_in.as_ref().expect("taa targets"), wgpu::LoadOp::Clear(wgpu::Color::BLACK), self.pipe(PipeId::Composite, samples, Some(HDR)), composite_bg.as_ref().expect("high composite"));
                    let hist = t.taa_hist.as_ref().expect("taa targets");
                    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("taa"),
                        color_attachments: &[
                            Some(wgpu::RenderPassColorAttachment { view: out_view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } }),
                            Some(wgpu::RenderPassColorAttachment { view: &hist[taa_idx], depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } }),
                        ],
                        depth_stencil_attachment: None,
                        timestamp_writes: self.prof.writes("taa"),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(self.pipe(PipeId::Taa, 1, Some(out_format)));
                    pass.set_bind_group(0, &t.bg_taa.as_ref().expect("taa targets")[taa_idx], &[]);
                    pass.draw(0..3, 0..1);
                } else {
                    fullscreen(&mut enc, "composite", out_view, wgpu::LoadOp::Clear(wgpu::Color::BLACK), self.pipe(PipeId::Composite, samples, Some(out_format)), composite_bg.as_ref().expect("high composite"));
                }
            }
            if nverts > 0 && !probe_out {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("text"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: out_view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: self.prof.writes("text"),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                if ui_n > 0 {
                    pass.set_pipeline(self.pipe(PipeId::UiOut, 1, Some(out_format)));
                    pass.set_vertex_buffer(0, self.text_vb.slice(..));
                    draw_ui(&mut pass);
                }
                if nverts > ui_n {
                    pass.set_pipeline(self.pipe(PipeId::TextOut, 1, Some(out_format)));
                    pass.set_bind_group(0, &self.text_bg, &[]);
                    pass.set_vertex_buffer(0, self.text_vb.slice(..));
                    pass.draw(ui_n..nverts, 0..1);
                }
            }
        } else {
            fullscreen(&mut enc, "resolve", out_view, wgpu::LoadOp::Clear(wgpu::Color::BLACK), self.pipe(PipeId::Resolve, 1, Some(out_format)), &t.bg_resolve);
        }
        // which entity is where
        if want_ids {
            let it = self.id_targets.as_ref().expect("id targets");
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ids"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &it.view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &it.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }), stencil_ops: None }),
                    timestamp_writes: self.prof.writes("ids"),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_bind_group(0, &scene_bg, &[]);
                pass.set_pipeline(self.pipe(PipeId::IdMesh, 1, None));
                draw_batches(&mut pass, depth_batches, false, true, main_fr.as_ref(), None, 1);
                draw_batches(&mut pass, &clear_batches, false, true, main_fr.as_ref(), None, 1);
                pass.set_pipeline(self.pipe(PipeId::IdSprite, 1, None));
                for (_, batch) in layer_steps.iter().filter(|s| s.0 == PipeId::Sprite) {
                    draw_batches(&mut pass, std::slice::from_ref(batch), false, true, None, None, 0xff);
                }
                if n_id_verts > 0 {
                    pass.set_pipeline(self.pipe(PipeId::IdText, 1, None));
                    pass.set_bind_group(0, &self.text_bg, &[]);
                    pass.set_vertex_buffer(0, self.text_vb.slice(nverts as u64 * 32..));
                    pass.draw(0..n_id_verts, 0..1);
                }
            }
            let bpr = (w * 4).div_ceil(256) * 256;
            enc.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo { texture: &it.tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyBufferInfo { buffer: &self.id_readback.as_ref().expect("id readback").1, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(h) } },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
        }
        // a picture wanted as an image is copied into the readback buffer in the same submission
        if matches!(out, Out::Image) {
            let bpr = (w * 4).div_ceil(256) * 256;
            enc.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo { texture: &self.out_image.as_ref().expect("image target").2, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyBufferInfo { buffer: &self.readback.as_ref().expect("readback buffer").1, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(h) } },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
        }
        self.last_draws = draw_count.get();
        self.last_draws_split = draws_split;
        lap("encode", &mut cpu);
        self.prof.resolve(&mut enc);
        self.gpu.queue.submit([enc.finish()]);
        lap("submit", &mut cpu);
        self.last_cpu_ms = cpu;
        self.ae_valid = ae_on;
        self.ae_copied = ae_read;
        if frame_no % 64 == 0 {
            self.evict_old(frame_no);
        }
        let errors = self.gpu.take_errors();
        if !errors.is_empty() {
            return Err(format!("the GPU reported: {}", errors.join("; ")));
        }
        Ok(())
    }

    // ------------------------------------------------------------------------------------------ reflection probes

    fn make_probe_set(&self, size: u32) -> ProbeSet {
        let levels = size.ilog2() + 1;
        let layers = (MAX_PROBES * 6) as u32;
        let tex = self.texture("probes", size, size, layers, levels, 1, HDR, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let array_view = tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let faces = (0..layers)
            .map(|l| {
                (0..levels)
                    .map(|m| tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_mip_level: m, mip_level_count: Some(1), base_array_layer: l, array_layer_count: Some(1), ..Default::default() }))
                    .collect()
            })
            .collect();
        ProbeSet { size, levels, array_view, faces, baked: [None; MAX_PROBES] }
    }

    /// Draw the six faces of the probe `p` into its slot (what the world looks like from where it stands, with the main picture's shadows but none of its effects) and build their mip levels.
    fn bake_probe_slot(&mut self, world: &mut World, textures: &mut TextureCache, slot: usize, p: &SceneProbe, clear: [f32; 4], base: &Quality) -> Result<(), String> {
        let (size, levels) = {
            let s = self.probe_set.as_ref().expect("probe set");
            (s.size, s.levels)
        };
        let q = Quality { mode: Mode::High, msaa: 1, shadow_size: base.shadow_size, point_shadow_size: base.point_shadow_size, shadows: true, ssao: false, bloom: false, dof: false, taa: false };
        let dirs: [(V3, V3); 6] = [([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), ([0.0, 1.0, 0.0], [0.0, 0.0, -1.0]), ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]), ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]), ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0])];
        // what the pictures of the main view keep from one to the next is put back afterwards
        let (ae_valid, prev_vp, nested) = (self.ae_valid, self.prev_vp, textures.nested);
        textures.nested = true;
        let mut result: Result<(), String> = Ok(());
        for (f, (fwd, up)) in dirs.iter().enumerate() {
            let camera = ExplicitCamera { eye: p.pos, target: [p.pos[0] + fwd[0], p.pos[1] + fwd[1], p.pos[2] + fwd[2]], up: *up, fov_y: 94.0, ortho_height: None, near: 0.05, far: 1000.0, clear };
            let opts = RenderOptions { width: size, height: size, view: View::Explicit(camera), supersample: 1, threads: 1 };
            textures.fx_on_card = true;
            let drawn = gather_scene_with(world, textures, &opts, self.gpu_skin).map_err(|e| e.message).and_then(|scene| {
                let view = self.probe_set.as_ref().expect("probe set").faces[slot * 6 + f][0].clone();
                self.draw(&scene, size, size, Out::Probe { view: &view }, &q, false, None)
            });
            if drawn.is_err() {
                result = drawn;
                break;
            }
        }
        textures.nested = nested;
        self.settle_fx(textures);
        textures.warnings.clear();
        self.ae_valid = ae_valid;
        self.prev_vp = prev_vp;
        result?;
        // mip levels: the next level of a face is the average of four texels of the one above it
        self.need(PipeId::ProbeMip, 1, None)?;
        let mut enc = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("probe-mips") });
        {
            let set = self.probe_set.as_ref().expect("probe set");
            for f in 0..6 {
                let layer = slot * 6 + f;
                for m in 1..levels as usize {
                    let bg = self.group(
                        "probe-mip",
                        &self.layouts.bloom_chain,
                        &[wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&set.faces[layer][m - 1]) }, wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.samplers.linear_clamp) }],
                    );
                    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("probe-mip"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &set.faces[layer][m], depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(self.pipe(PipeId::ProbeMip, 1, None));
                    pass.set_bind_group(0, &bg, &[]);
                    pass.draw(0..3, 0..1);
                }
            }
        }
        self.gpu.queue.submit([enc.finish()]);
        let errors = self.gpu.take_errors();
        if !errors.is_empty() {
            return Err(format!("the GPU reported: {}", errors.join("; ")));
        }
        Ok(())
    }

    /// Capture the probes that need it: the first four by uid, those whose position or box changed since they were captured, or (`force`) all of them or one. Returns the uids that were captured.
    fn sync_probes(&mut self, world: &mut World, textures: &mut TextureCache, probes: &[SceneProbe], clear: [f32; 4], base: &Quality, force: Option<Option<u64>>) -> Result<Vec<u64>, String> {
        let used: Vec<&SceneProbe> = probes.iter().take(MAX_PROBES).collect();
        let Some(biggest) = used.iter().map(|p| p.resolution).max() else {
            if let Some(set) = &mut self.probe_set {
                set.baked = [None; MAX_PROBES];
            }
            return Ok(Vec::new());
        };
        let size = biggest.next_power_of_two().clamp(16, 256);
        if self.probe_set.as_ref().is_none_or(|s| s.size != size) {
            self.probe_set = Some(self.make_probe_set(size));
        }
        let mut done = Vec::new();
        for (k, p) in used.iter().enumerate() {
            let key = ProbeKey { uid: p.uid, pos: p.pos, half: p.half };
            let wanted = match force {
                None => false,
                Some(None) => true,
                Some(Some(uid)) => uid == p.uid,
            };
            if wanted || self.probe_set.as_ref().expect("probe set").baked[k] != Some(key) {
                self.probe_set.as_mut().expect("probe set").baked[k] = None;
                self.bake_probe_slot(world, textures, k, p, clear, base)?;
                self.probe_set.as_mut().expect("probe set").baked[k] = Some(key);
                done.push(p.uid);
            }
        }
        for k in used.len()..MAX_PROBES {
            self.probe_set.as_mut().expect("probe set").baked[k] = None;
        }
        Ok(done)
    }

    /// Capture the reflection probes of the world again now (`uid`: only that probe; `None`: all of them) and return the uids that were captured. Pictures of the high qualities capture a probe by themselves
    /// the first time they draw it and when its position or box changes; this is for after the world around it changed.
    pub fn bake_probes(&mut self, world: &mut World, textures: &mut TextureCache, uid: Option<u64>) -> Result<Vec<u64>, String> {
        self.sync_texture_cache(textures);
        let scene = gather_scene(world, textures, &RenderOptions { width: 64, height: 36, ..RenderOptions::default() }).map_err(|e| e.message)?;
        let done = self.sync_probes(world, textures, &scene.probes, scene.cam.clear, &Quality::high(), Some(uid));
        textures.warnings.clear();
        done
    }

    // ------------------------------------------------------------------------------------------ public entry points

    /// Draw the world into an 8-bit image and read it back (what the `render` tool returns, from the GPU).
    pub fn render_to_image(&mut self, world: &mut World, textures: &mut TextureCache, opts: &RenderOptions, quality: &Quality) -> Result<Frame, String> {
        self.render_frame(world, textures, opts, quality, true)
    }

    /// Like `render_to_image`; `want_ids` says whether to work out which entity is at each pixel (`Frame::ids`, `visible`, `pick`).
    /// The depth buffer is not provided (every value is infinity).
    /// Choose where the compressed pictures are kept between runs: `LUNARIS_TEXTURE_CACHE` (a folder, or `off`), else the workspace's `.lunaris/cache/textures`, else nowhere. The first time a folder is used its size is limited to 4 GB.
    fn sync_texture_cache(&mut self, textures: &TextureCache) {
        let want = match std::env::var("LUNARIS_TEXTURE_CACHE") {
            Ok(v) if v == "off" || v.is_empty() => None,
            Ok(v) => Some(std::path::PathBuf::from(v)),
            Err(_) => textures.root().map(|r| r.join(".lunaris").join("cache").join("textures")),
        };
        let want = want.filter(|_| self.compress);
        if want != self.texture_cache {
            if let Some(dir) = &want {
                compress::trim_cache(dir, 4 << 30);
            }
            self.texture_cache = want;
        }
    }

    pub fn render_frame(&mut self, world: &mut World, textures: &mut TextureCache, opts: &RenderOptions, quality: &Quality, want_ids: bool) -> Result<Frame, String> {
        self.sync_texture_cache(textures);
        if opts.width == 0 || opts.height == 0 || opts.width > 8192 || opts.height > 8192 {
            return Err("image size must be between 1x1 and 8192x8192".into());
        }
        let t_all = std::time::Instant::now();
        let proc_before = if self.prof.on { crate::procstat::snapshot() } else { None };
        let t_gather = std::time::Instant::now();
        textures.fx_on_card = true;
        let mut scene = gather_scene_with(world, textures, opts, self.gpu_skin).map_err(|e| e.message)?;
        let gather_ms = t_gather.elapsed().as_secs_f32() * 1000.0;
        scene.warnings.append(&mut textures.warnings);
        let t_probes = std::time::Instant::now();
        if quality.mode == Mode::High {
            self.sync_probes(world, textures, &scene.probes, scene.cam.clear, quality, None)?;
        }
        let probes_ms = t_probes.elapsed().as_secs_f32() * 1000.0;
        let (w, h) = (opts.width, opts.height);
        let adapt = self.adapt_dt;
        // bounced light is read from the last picture and blended over the last ones, and the anti-aliasing blends pictures with the pixel grid shifted: a still is drawn several times, the same ones in the same order
        let gi_still = quality.mode == Mode::High && quality.ssao && scene.env.gi[0] > 0.0;
        let taa_still = quality.mode == Mode::High && quality.taa;
        let t_warm = std::time::Instant::now();
        if gi_still || taa_still {
            self.gi_age = 0;
            self.taa_age = 0;
            self.taa_index = 0;
            self.fx_frame = 0;
            // the pictures before the last do not let the eye adapt any further (a step of no time)
            let warm_adapt = adapt.map(|_| 0.0);
            for _ in 0..7 {
                self.draw(&scene, w, h, Out::Image, quality, false, warm_adapt)?;
            }
        }
        let warm_ms = t_warm.elapsed().as_secs_f32() * 1000.0;
        let t_draw = std::time::Instant::now();
        self.draw(&scene, w, h, Out::Image, quality, want_ids, adapt)?;
        self.settle_fx(textures);
        let draw_ms = t_draw.elapsed().as_secs_f32() * 1000.0;
        let t_read = std::time::Instant::now();
        let bpr = (w * 4).div_ceil(256) * 256;
        let buf = &self.readback.as_ref().expect("the picture was copied").1;
        let slice = buf.slice(..(bpr as u64 * h as u64));
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.gpu.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("the GPU did not finish: {e}"))?;
        rx.recv().map_err(|_| "the GPU readback was lost".to_string())?.map_err(|e| format!("cannot read the picture back: {e}"))?;
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        {
            let data = slice.get_mapped_range().map_err(|e| format!("cannot read the picture back: {e}"))?;
            for y in 0..h as usize {
                let row = &data[y * bpr as usize..y * bpr as usize + (w * 4) as usize];
                rgba.extend_from_slice(row);
            }
        }
        buf.unmap();
        let read_ms = t_read.elapsed().as_secs_f32() * 1000.0;
        // what the eye adaptation measured and did, read from the small buffer the picture copied it into
        let mut info: Vec<(String, f64)> = Vec::new();
        if self.prof.on {
            info.push(("cpu_ms.gather".to_string(), gather_ms as f64));
            info.push(("cpu_ms.draw_call".to_string(), draw_ms as f64));
            info.push(("cpu_ms.sync_probes".to_string(), probes_ms as f64));
            // a still is drawn eight times (seven to settle the anti-aliasing and the bounced light, then the one that is kept): the laps below are the last one's
            info.push(("cpu_ms.warm_draws".to_string(), warm_ms as f64));
            info.push(("cpu_ms.render_frame_so_far".to_string(), t_all.elapsed().as_secs_f32() as f64 * 1000.0));
            if let (Some(before), Some(after)) = (proc_before, crate::procstat::snapshot()) {
                info.extend(crate::procstat::entries(&before, &after, t_all.elapsed().as_secs_f64() * 1000.0));
            }
            info.push(("cpu_ms.wait_and_read".to_string(), read_ms as f64));
            for (k, v) in &self.last_cpu_ms {
                info.push((format!("cpu_ms.{k}"), *v as f64));
            }
        }
        let gpu_ms = self.prof.read(&self.gpu.device)?;
        if !gpu_ms.is_empty() {
            info.push(("gpu_ms.total".to_string(), gpu_ms.iter().filter(|g| !g.0.contains('.')).map(|g| g.1 as f64).sum()));
            info.extend(gpu_ms.iter().map(|(k, v)| (format!("gpu_ms.{k}"), *v as f64)));
        }
        self.last_gpu_ms = gpu_ms;
        info.push(("triangles".to_string(), self.last_triangles as f64));
        info.push(("point_shadow_lights".to_string(), self.last_point_shadows.0 as f64));
        info.push(("point_shadow_casters".to_string(), self.last_point_shadows.1 as f64));
        info.push(("point_shadow_objects".to_string(), self.last_point_shadows.2 as f64));
        for (i, c) in self.last_lods.iter().enumerate() {
            info.push((format!("lod{i}"), *c as f64));
        }
        info.push(("objects_culled".to_string(), self.last_cull.1 as f64));
        info.push(("texture_mb".to_string(), self.textures.values().chain(self.data_textures.values()).map(|t| t.bytes).sum::<u64>() as f64 / 1048576.0));
        if self.ae_copied {
            let slice = self.exposure_read.slice(..16);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            self.gpu.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("the GPU did not finish: {e}"))?;
            rx.recv().map_err(|_| "the GPU readback was lost".to_string())?.map_err(|e| format!("cannot read the exposure back: {e}"))?;
            {
                let data = slice.get_mapped_range().map_err(|e| format!("cannot read the exposure back: {e}"))?;
                let f: Vec<f32> = data.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
                info.push(("exposure".to_string(), f[0] as f64));
                info.push(("average_luminance".to_string(), f[1] as f64));
            }
            self.exposure_read.unmap();
        }
        // the picture is opaque
        for px in rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
        let mut ids = vec![0u64; (w * h) as usize];
        if want_ids {
            let buf = &self.id_readback.as_ref().expect("the ids were copied").1;
            let slice = buf.slice(..(bpr as u64 * h as u64));
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            self.gpu.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("the GPU did not finish: {e}"))?;
            rx.recv().map_err(|_| "the GPU readback was lost".to_string())?.map_err(|e| format!("cannot read the entity ids back: {e}"))?;
            {
                let data = slice.get_mapped_range().map_err(|e| format!("cannot read the entity ids back: {e}"))?;
                for y in 0..h as usize {
                    let row = &data[y * bpr as usize..y * bpr as usize + (w * 4) as usize];
                    for (x, px) in row.chunks_exact(4).enumerate() {
                        let slot = u32::from_le_bytes([px[0], px[1], px[2], px[3]]) as usize;
                        if slot != 0 {
                            ids[y * w as usize + x] = self.slot_uids.get(slot - 1).copied().unwrap_or(0);
                        }
                    }
                }
            }
            buf.unmap();
        }
        Ok(Frame { width: w, height: h, rgba, ids, depth: vec![f32::INFINITY; (w * h) as usize], view_proj: scene.view_proj, camera: scene.cam.desc.clone(), warnings: scene.warnings, info })
    }

    /// Wait for the display's refresh before showing a picture (`vsync`) or show it at once, while the window is open (a settings menu's V-sync switch). Does nothing for a renderer without a window.
    pub fn set_vsync(&mut self, vsync: bool) {
        if let Some(s) = &mut self.surface {
            let caps = s.surface.get_capabilities(&self.gpu.adapter);
            let mode = if vsync { wgpu::PresentMode::Fifo } else if caps.present_modes.contains(&wgpu::PresentMode::Immediate) { wgpu::PresentMode::Immediate } else { wgpu::PresentMode::AutoNoVsync };
            if s.config.present_mode != mode {
                s.config.present_mode = mode;
                s.surface.configure(&self.gpu.device, &s.config);
            }
        }
    }

    /// Resize the window surface (call when the window changes size).
    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(s) = &mut self.surface {
            if width > 0 && height > 0 && (s.config.width != width || s.config.height != height) {
                s.config.width = width;
                s.config.height = height;
                s.surface.configure(&self.gpu.device, &s.config);
            }
        }
    }

    /// Draw the world into the window this renderer was made for. Returns the warnings the picture produced; a window that is hidden or
    /// minimised draws nothing (and returns no warnings).
    pub fn render_to_window(&mut self, world: &mut World, textures: &mut TextureCache, width: u32, height: u32, quality: &Quality) -> Result<Vec<String>, String> {
        self.sync_texture_cache(textures);
        if width == 0 || height == 0 {
            return Ok(Vec::new());
        }
        self.resize(width, height);
        let opts = RenderOptions { width, height, ..RenderOptions::default() };
        let t_gather = lunaris_render::Timed::new(|| "window: gather the scene".to_string());
        textures.fx_on_card = true;
        let mut scene = gather_scene_with(world, textures, &opts, self.gpu_skin).map_err(|e| e.message)?;
        scene.warnings.append(&mut textures.warnings);
        drop(t_gather);
        if self.async_prep {
            let models = textures.loaded_models();
            let frame = self.frame_no + 1;
            self.prepare_loaded(&models, quality.mode == Mode::High, frame);
        }
        if quality.mode == Mode::High {
            let _t = lunaris_render::Timed::new(|| "window: light probes".to_string());
            self.sync_probes(world, textures, &scene.probes, scene.cam.clear, quality, None)?;
        }
        let t_acquire = lunaris_render::Timed::new(|| "window: get the surface".to_string());
        let frame = {
            let s = self.surface.as_ref().ok_or("this renderer has no window")?;
            let mut got = None;
            for _ in 0..2 {
                match s.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                        got = Some(t);
                        break;
                    }
                    wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => s.surface.configure(&self.gpu.device, &s.config),
                    wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return Ok(Vec::new()),
                    wgpu::CurrentSurfaceTexture::Validation => return Err("the window surface is not valid".into()),
                }
            }
            match got {
                Some(t) => t,
                None => return Ok(Vec::new()),
            }
        };
        drop(t_acquire);
        let format = self.surface.as_ref().expect("checked").config.format;
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        // pictures of a window follow each other: the eye adapts by the time between them (it starts from the measured value after a long pause)
        let now = std::time::Instant::now();
        let adapt = self.last_window_pic.map(|t| now.duration_since(t).as_secs_f32()).filter(|d| *d < 2.0).map(|d| d.min(0.25));
        self.last_window_pic = Some(now);
        let t_draw = lunaris_render::Timed::new(|| "window: draw".to_string());
        self.prof.ring_mode.set(true);
        let drawn = self.draw(&scene, width, height, Out::Surface { view: &view, format }, quality, false, adapt);
        self.prof.ring_mode.set(false);
        self.settle_fx(textures);
        drawn?;
        drop(t_draw);
        let t_present = lunaris_render::Timed::new(|| "window: present".to_string());
        self.gpu.queue.present(frame);
        drop(t_present);
        self.frames_presented += 1;
        // with `LUNARIS_GPU_PROFILE=1` the passes of a window's pictures are timed too: the times come back a few pictures later, from the card, without anything waiting for it
        if self.prof.on {
            if let Some(g) = self.prof.collect(&self.gpu.device) {
                self.last_gpu_ms = g;
            }
        }
        Ok(scene.warnings)
    }
}

/// The window of the sea's depth is `SEA_DIM` points each way, `SEA_CELL` metres apart (512 m across, the shore round the eye).
const SEA_DIM: u32 = 128;
const SEA_CELL: f32 = 4.0;

/// A number as the bits of a 16-bit float (round to nearest; too small is 0, too big the largest): enough for a depth in metres.
fn f16_bits(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    if exp <= 0 {
        return sign;
    }
    if exp >= 31 {
        return sign | 0x7bff;
    }
    let m = (b & 0x7f_ffff) + 0x1000;
    if m & 0x80_0000 != 0 {
        return if exp + 1 >= 31 { sign | 0x7bff } else { sign | (((exp + 1) as u16) << 10) };
    }
    sign | ((exp as u16) << 10) | ((m >> 13) as u16)
}

/// The depth of the sea over the window that starts at `origin` (`sea_depth` in `scene.wgsl` reads it, from the frame uniform): at each point the level of the sea minus the height of the ground, so negative on land and 255 (deep) where there
/// is no ground; as 16-bit floats, two to a word, row after row.
fn sea_floor_window(floors: &[lunaris_render::render::SceneFloor], level: f32, origin: [f32; 2]) -> Box<[[u32; 4]; 2048]> {
    let placed: Vec<_> = floors.iter().map(|f| lunaris_sim::terrain::Placed::new(&f.heights, f.pos, &f.terrain)).collect();
    let mut words = Box::new([[0u32; 4]; 2048]);
    for n in 0..(SEA_DIM * SEA_DIM) as usize {
        let (i, j) = ((n % SEA_DIM as usize) as f32, (n / SEA_DIM as usize) as f32);
        let (x, z) = (origin[0] + i * SEA_CELL, origin[1] + j * SEA_CELL);
        let depth = placed.iter().find_map(|p| p.surface(x, z)).map_or(255.0, |y| (level - y).clamp(-16.0, 255.0));
        let (word, half) = (n / 2, n % 2);
        words[word / 4][word % 4] |= (f16_bits(depth) as u32) << (16 * half);
    }
    words
}

#[cfg(test)]
mod depth_tests {
    use super::*;

    #[test]
    fn the_depth_value_gives_the_distance_in_front_of_the_camera_for_both_projections() {
        for proj in [M4::perspective(50.0, 1.6, 0.1, 400.0), M4::perspective(20.0, 1.0, 0.5, 5000.0), M4::ortho(10.0, 1.6, 0.1, 100.0)] {
            let (k1, k2) = view_depth_constants(&proj);
            let ortho = proj.0[15] != 0.0;
            for dist in [0.2f32, 1.0, 7.5, 40.0, 90.0] {
                // a point `dist` in front of the camera (the camera looks down -z), off the axis a little
                let clip = to_gpu_clip(&proj);
                let h = clip.mul_v4([0.3 * dist.min(5.0), 0.2, -dist, 1.0]);
                let d = h[2] / h[3];
                let back = if ortho { d * k1 + k2 } else { 1.0 / (d * k1 + k2) };
                assert!((back - dist).abs() <= 1.0e-3 * dist.max(1.0), "{dist}: depth {d} gave {back}");
            }
        }
    }
}

#[cfg(test)]
mod sea_tests {
    use super::*;

    fn half_to_f32(b: u16) -> f32 {
        let (sign, exp, man) = ((b >> 15) & 1, ((b >> 10) & 31) as i32, (b & 0x3ff) as f32);
        let v = if exp == 0 { man * 2f32.powi(-24) } else { (1.0 + man / 1024.0) * 2f32.powi(exp - 15) };
        if sign == 1 { -v } else { v }
    }

    fn at(words: &[[u32; 4]; 2048], i: usize, j: usize) -> f32 {
        let n = j * SEA_DIM as usize + i;
        let (word, half) = (n / 2, n % 2);
        half_to_f32(((words[word / 4][word % 4] >> (16 * half)) & 0xffff) as u16)
    }

    #[test]
    fn sixteen_bit_floats_are_right() {
        for (v, bits) in [(0.0f32, 0x0000u16), (1.0, 0x3c00), (-2.0, 0xc000), (0.5, 0x3800), (255.0, 0x5bf8), (-16.0, 0xcc00)] {
            assert_eq!(f16_bits(v), bits, "{v}");
        }
        // within a part in a thousand over the depths that matter (a millimetre in the shallows)
        for i in 0..2000 {
            let v = i as f32 * 0.127 - 16.0;
            let back = half_to_f32(f16_bits(v));
            assert!((back - v).abs() <= v.abs() * 0.001 + 2e-4, "{v} came back as {back}");
        }
    }

    #[test]
    fn the_window_of_the_sea_depth_follows_the_ground_and_is_deep_where_there_is_none() {
        // a ramp 64 m wide that rises from 8 m under the sea at x = -32 to 2 m over it at x = 32
        let samples: Vec<u16> = (0..17 * 17).map(|i| ((i % 17) * 4095) as u16).collect();
        let field = Arc::new(lunaris_sim::terrain::Heightfield::new(17, 17, samples).unwrap());
        let terrain = lunaris_sim::components::Terrain { size: [64.0, 64.0], height: 10.0, ..Default::default() };
        let floor = lunaris_render::render::SceneFloor { heights: field, pos: [0.0, -8.0, 0.0], terrain };
        let origin = [-256.0, -256.0];
        let w = sea_floor_window(&[floor], 0.0, origin);
        let point = |x: f32, z: f32| at(&w, ((x - origin[0]) / SEA_CELL) as usize, ((z - origin[1]) / SEA_CELL) as usize);
        // the ground at x is -8 + 10 (x + 32) / 64 * (65520 / 65535): the sea is that deep (negative: land)
        let ground = |x: f32| -8.0 + 10.0 * (x + 32.0) / 64.0 * (65520.0 / 65535.0);
        for x in [-28.0f32, -16.0, 0.0, 12.0, 24.0] {
            assert!((point(x, 0.0) + ground(x)).abs() < 0.12, "x {x}: {} against {}", point(x, 0.0), -ground(x));
        }
        assert!(point(-16.0, 0.0) > 4.0 && point(24.0, 0.0) < 0.0, "deep on one side of the ramp, land on the other");
        assert_eq!(point(-100.0, 0.0), 255.0, "open sea where there is no ground");
        assert_eq!(point(100.0, 100.0), 255.0);
    }
}

#[cfg(test)]
mod detail_tests {
    use super::*;

    /// A bumpy sheet of `n` x `n` cells.
    fn sheet(n: usize) -> Mesh {
        let mut m = Mesh { pos: Vec::new(), nrm: Vec::new(), uv: Vec::new(), col: Vec::new(), texw: Vec::new(), idx: Vec::new(), tex: None, rig: None, normal_tex: None, arm_tex: None, alpha_cutoff: 0.0, lod: lunaris_render::mesh::LodMode::Full, parts: Vec::new(), occluders: Vec::new(), depth_whole: None, emissive_factor: 0.0 };
        for j in 0..=n {
            for i in 0..=n {
                let (x, z) = (i as f32 / n as f32 * 10.0, j as f32 / n as f32 * 10.0);
                m.pos.push([x, 0.3 * (x * 1.7).sin() * (z * 1.3).cos(), z]);
                m.nrm.push([0.0, 1.0, 0.0]);
                m.uv.push([i as f32 / n as f32, j as f32 / n as f32]);
            }
        }
        for j in 0..n {
            for i in 0..n {
                let a = (j * (n + 1) + i) as u32;
                m.idx.extend_from_slice(&[a, a + n as u32 + 1, a + 1, a + 1, a + n as u32 + 1, a + n as u32 + 2]);
            }
        }
        m
    }

    #[test]
    fn coarser_levels_have_fewer_triangles_made_of_the_same_vertices() {
        let m = sheet(120); // 28,800 triangles
        let lods = make_lods(&m);
        assert!(lods.len() >= 3, "{} levels", lods.len());
        let mut last = m.idx.len() / 3;
        let mut last_cell = 0.0;
        for (idx, cell) in &lods {
            let n = idx.len() / 3;
            assert!(n * 5 <= last * 4, "each level sheds at least a fifth: {last} then {n}");
            assert!(*cell > last_cell, "the cells grow");
            assert!(idx.iter().all(|&i| (i as usize) < m.pos.len()), "the levels use the mesh's own vertices");
            for t in idx.chunks_exact(3) {
                assert!(t[0] != t[1] && t[1] != t[2] && t[0] != t[2], "no triangle that has fallen in on itself");
                // seen from above the sheet still faces up
                let (a, b, c) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
                let cross_y = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
                assert!(cross_y >= -1.0e-3, "no triangle turned over");
            }
            last = n;
            last_cell = *cell;
        }
        assert!(make_lods(&sheet(12)).is_empty(), "a small mesh keeps its one level");
    }

    #[test]
    fn the_level_of_detail_follows_the_distance() {
        let lods = [LodRange { first: 0, count: 100, cell: 0.0 }, LodRange { first: 100, count: 50, cell: 0.1 }, LodRange { first: 150, count: 20, cell: 0.4 }];
        let focal = 1000.0;
        assert_eq!(choose_lod_at(&lods, 1.0, 1.0, focal, 0, 1.5), 0, "close: everything");
        assert_eq!(choose_lod_at(&lods, 100.0, 1.0, focal, 0, 1.5), 1, "0.05 m of error at 100 m is half a pixel, 0.2 m is two");
        assert_eq!(choose_lod_at(&lods, 1000.0, 1.0, focal, 0, 1.5), 2);
        assert_eq!(choose_lod_at(&lods, 1000.0, 10.0, focal, 0, 1.5), 1, "a mesh scaled up ten times needs ten times the distance for the same level");
        assert_eq!(choose_lod_at(&lods, 100.0, 10.0, focal, 0, 1.5), 0);
        assert!((0..40).map(|k| choose_lod_at(&lods, 1.0 + k as f32 * 30.0, 1.0, focal, 0, 1.5)).collect::<Vec<_>>().windows(2).all(|w| w[0] <= w[1]), "never finer when farther");
    }

    #[test]
    fn a_far_object_is_drawn_with_triangles_of_a_few_pixels() {
        let lods = [LodRange { first: 0, count: 3 * 100_000, cell: 0.0 }, LodRange { first: 0, count: 3 * 10_000, cell: 0.1 }, LodRange { first: 0, count: 3 * 1000, cell: 0.4 }, LodRange { first: 0, count: 3 * 100, cell: 1.6 }];
        // a radius of 10 m at 100 m with a focal length of 1000 pixels is 100 pixels: half a circle of 15,700 square pixels, at 3 each a budget of 5,200 triangles
        assert_eq!(density_lod(&lods, 100.0, 10.0, 1000.0, 3.0), 2);
        assert_eq!(density_lod(&lods, 1000.0, 10.0, 1000.0, 3.0), 3, "a tenth as big on the screen: a hundredth of the budget");
        assert_eq!(density_lod(&lods, 1.0, 10.0, 1000.0, 3.0), 0, "close: everything");
        assert_eq!(density_lod(&lods, 1000.0, 10.0, 1000.0, 0.0), 0, "the rule can be turned off");
        assert_eq!(density_lod(&lods[..1], 1000.0, 10.0, 1000.0, 3.0), 0, "a mesh without coarser levels has nothing to choose");
    }

    #[test]
    fn a_mesh_with_too_many_triangles_is_never_drawn_at_its_finest() {
        let lods = [LodRange { first: 0, count: 3 * 800_000, cell: 0.0 }, LodRange { first: 0, count: 3 * 170_000, cell: 0.01 }, LodRange { first: 0, count: 3 * 58_000, cell: 0.02 }, LodRange { first: 0, count: 3 * 16_000, cell: 0.05 }];
        assert_eq!(min_lod_of(&lods), 2, "the first level of 150,000 triangles or fewer");
        assert_eq!(min_lod_of(&lods[2..]), 0, "a mesh that fits keeps its whole");
        assert_eq!(min_lod_of(&lods[..1]), 0, "and one with no coarser levels is drawn as it is");
        assert_eq!(choose_lod_at(&lods, 0.5, 1.0, 1000.0, 2, 1.5), 2, "close up it is still the budget level");
        assert_eq!(choose_lod_at(&lods, 10_000.0, 1.0, 1000.0, 2, 1.5), 3, "and far off the coarser ones follow as before");
    }

    #[test]
    fn clusters_cover_every_triangle_once_and_each_has_a_sphere_that_holds_it() {
        let m = sheet(100); // 20,000 triangles: too few for clusters
        assert!(make_clusters(&m).is_none());
        let m = sheet(150); // 45,000
        let (idx, clusters) = make_clusters(&m).expect("clusters");
        assert_eq!(idx.len(), m.idx.len());
        let key = |t: &[u32]| {
            let mut v = [t[0], t[1], t[2]];
            v.sort_unstable();
            v
        };
        let mut a: Vec<[u32; 3]> = m.idx.chunks_exact(3).map(key).collect();
        let mut b: Vec<[u32; 3]> = idx.chunks_exact(3).map(key).collect();
        a.sort_unstable();
        b.sort_unstable();
        assert!(a == b, "the same triangles in another order");
        assert_eq!(clusters.iter().map(|c| c.count as usize).sum::<usize>(), idx.len());
        assert!(clusters.len() >= 10);
        for (k, c) in clusters.iter().enumerate() {
            assert_eq!(c.first as usize, k * CLUSTER_TRIANGLES * 3, "each starts where the one before ended");
            for &v in &idx[c.first as usize..(c.first + c.count) as usize] {
                assert!(length(sub(m.pos[v as usize], c.center)) <= c.radius + 1.0e-4);
            }
            // spatially coherent: smaller than the sheet (7.1 m from the middle to a corner)
            assert!(c.radius < 6.0, "cluster {k} is {} m across", c.radius * 2.0);
        }
        let (idx2, _) = make_clusters(&m).unwrap();
        assert!(idx == idx2, "the same order on every run");
    }
}
