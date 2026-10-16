//! The renderer: owns the device, caches meshes, textures and pipelines, and draws a `Scene` into an image or a window surface.
use crate::gpu::Gpu;
use crate::pipelines::*;
use crate::util::*;
use bytemuck::{Pod, Zeroable};
use lunaris_ecs::World;
use lunaris_render::math::{length, sub, V3};
use lunaris_render::mesh::Mesh;
use lunaris_render::render::{gather_scene, ExplicitCamera, MeshPbr, MeshRef, RenderOptions, Scene, SceneParticles, SceneProbe, View};
use lunaris_render::terrain_layers::{Splat, SplatLayers};
use lunaris_render::Frame;
use lunaris_render::texture::Texture;
use lunaris_render::{font, TextureCache};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_LIGHTS: usize = 16;
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
}

impl Quality {
    pub fn classic() -> Quality {
        Quality { mode: Mode::Classic, msaa: 1, shadow_size: 1024, point_shadow_size: 0, shadows: false, ssao: false, bloom: false, dof: false }
    }

    pub fn low() -> Quality {
        Quality { mode: Mode::High, msaa: 1, shadow_size: 1024, point_shadow_size: 0, shadows: true, ssao: false, bloom: false, dof: false }
    }

    pub fn medium() -> Quality {
        Quality { mode: Mode::High, msaa: 4, shadow_size: 1024, point_shadow_size: 512, shadows: true, ssao: false, bloom: true, dof: false }
    }

    pub fn high() -> Quality {
        Quality { mode: Mode::High, msaa: 4, shadow_size: 2048, point_shadow_size: 1024, shadows: true, ssao: true, bloom: true, dof: true }
    }

    pub fn ultra() -> Quality {
        Quality { mode: Mode::High, msaa: 4, shadow_size: 4096, point_shadow_size: 2048, shadows: true, ssao: true, bloom: true, dof: true }
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
    probes: [[f32; 4]; 8],
    prev_view_proj: [f32; 16],
    psh: [PointShadowU; 2],
    lights: [[f32; 4]; MAX_LIGHTS * 4],
}

fn v4(v: V3, w: f32) -> [f32; 4] {
    [v[0], v[1], v[2], w]
}

// ------------------------------------------------------------------------------------------------ cached resources

struct GpuMesh {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    count: u32,
    use_vcol: bool,
    /// The texture of an imported model.
    texture: Option<Arc<Texture>>,
    /// Keeps the mesh (and so its address, which is the cache key) alive while it is cached.
    _keep: Option<Arc<Mesh>>,
    last_used: u64,
    center: V3,
    radius: f32,
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
}

/// A texture bind group's key: the colour texture, the quality, the sampler, and the addresses of the normal and occlusion-roughness-metallic maps (0 for none).
type TexKey = (usize, bool, SamplerKind, usize, usize, usize);

struct GpuTex {
    view: wgpu::TextureView,
    height: Option<wgpu::TextureView>,
    _keep: Option<Arc<Texture>>,
    last_used: u64,
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
    /// A painted terrain: the index of its bind group (splat map and layers) among the picture's `splat_bgs`.
    splat: Option<usize>,
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
    shaders: Shaders,
    layouts: Layouts,
    pipes: PipelineCache2,
    samplers: Samplers,
    meshes: HashMap<usize, GpuMesh>,
    tile_meshes: HashMap<u64, GpuMesh>,
    dyn_meshes: HashMap<u64, GpuMesh>,
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
    /// Which of the two `ao` textures the last picture with bounced light made, and how many pictures the bounce has been blended over.
    gi_flip: usize,
    gi_age: u32,
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
    /// The last picture adapted (so that the next one can glide from its exposure instead of starting from the measured value).
    ae_valid: bool,
    /// The last picture copied the adaptation state into `exposure_read` (a picture read back as an image does).
    ae_copied: bool,
    /// Seconds a picture drawn into an image advances the eye adaptation by (`None`: it measures and applies at once).
    adapt_dt: Option<f32>,
    /// When the last picture reached the window.
    last_window_pic: Option<std::time::Instant>,
}

fn depth_pass<'e>(enc: &'e mut wgpu::CommandEncoder, label: &str, view: &wgpu::TextureView, clear: f32) -> wgpu::RenderPass<'e> {
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store }), stencil_ops: None }),
        timestamp_writes: None,
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
            linear_clamp: device.create_sampler(&wgpu::SamplerDescriptor { label: Some("linear-clamp"), address_mode_u: wgpu::AddressMode::ClampToEdge, address_mode_v: wgpu::AddressMode::ClampToEdge, mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::MipmapFilterMode::Nearest, ..Default::default() }),
            shadow: device.create_sampler(&wgpu::SamplerDescriptor { label: Some("shadow"), address_mode_u: wgpu::AddressMode::ClampToEdge, address_mode_v: wgpu::AddressMode::ClampToEdge, mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::MipmapFilterMode::Nearest, compare: Some(wgpu::CompareFunction::LessEqual), ..Default::default() }),
            probe: device.create_sampler(&wgpu::SamplerDescriptor { label: Some("probe"), address_mode_u: wgpu::AddressMode::ClampToEdge, address_mode_v: wgpu::AddressMode::ClampToEdge, mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::MipmapFilterMode::Linear, ..Default::default() }),
        };
        let buf = |label: &str, size: u64, usage: wgpu::BufferUsages| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false });
        let frame_buf = buf("frame", std::mem::size_of::<FrameU>() as u64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let objects_cap = 256u64;
        let objects = buf("objects", objects_cap * std::mem::size_of::<ObjectData>() as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        let pass_bufs: Vec<wgpu::Buffer> = (0..16).map(|i| buf(&format!("pass{i}"), 64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST)).collect();
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
        let exposure_buf = buf("exposure", 16, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST);
        let exposure_read = buf("exposure-read", 16, wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
        let ae_pipe = build_exposure(device, &shaders, &layouts);
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("the renderer's resources are not valid on this GPU: {e}"));
        }
        Ok(GpuRenderer {
            gpu,
            shaders,
            layouts,
            pipes: PipelineCache2::new(),
            samplers,
            meshes: HashMap::new(),
            tile_meshes: HashMap::new(),
            dyn_meshes: HashMap::new(),
            textures: HashMap::new(),
            splat_textures: HashMap::new(),
            tex_bgs: HashMap::new(),
            data_textures: HashMap::new(),
            splat_tex_bg: None,
            grass_heights: HashMap::new(),
            grass_bufs: Vec::new(),
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
            ao_white,
            targets: None,
            ssr_targets: None,
            ssr_dummy,
            probe_set: None,
            probe_dummy,
            prev_vp: None,
            fx_frame: 0,
            gi_flip: 0,
            gi_age: 0,
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
            ae_valid: false,
            ae_copied: false,
            adapt_dt: None,
            last_window_pic: None,
        })
    }

    pub fn adapter_name(&self) -> String {
        self.gpu.name()
    }

    /// How the eye adaptation (`AutoExposure`) treats the pictures drawn into images (`render_to_image`, `render_frame`): `None` (the default) measures the picture and
    /// applies the exposure at once, so that the same world always draws the same picture (what a tool wants); `Some(seconds)` treats every picture as that many seconds after
    /// the one before and glides from its exposure (a test, or a program that films the world into images). A window always follows the clock.
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
    fn vertex_data(mesh: &Mesh) -> (Vec<f32>, V3, f32) {
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
        (v, center, radius)
    }

    fn upload_mesh(&self, mesh: &Mesh, keep: Option<Arc<Mesh>>, frame: u64) -> GpuMesh {
        let (v, center, radius) = Self::vertex_data(mesh);
        let device = &self.gpu.device;
        let make = |label: &str, bytes: &[u8], usage: wgpu::BufferUsages| {
            // an empty buffer is not allowed to be mapped at creation; pad to one element
            let size = (bytes.len().max(4) as u64 + 3) & !3;
            let b = device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            if !bytes.is_empty() {
                self.gpu.queue.write_buffer(&b, 0, bytes);
            }
            b
        };
        GpuMesh {
            vbuf: make("mesh-vertices", bytes_of_floats(&v), wgpu::BufferUsages::VERTEX),
            ibuf: make("mesh-indices", bytemuck::cast_slice(&mesh.idx), wgpu::BufferUsages::INDEX),
            count: mesh.idx.len() as u32,
            use_vcol: !mesh.col.is_empty(),
            texture: mesh.tex.clone(),
            _keep: keep,
            last_used: frame,
            center,
            radius,
        }
    }

    /// An animated model: reuse the buffers of the last frame when the pose has the same size, so that a moving character costs a
    /// write and not an allocation.
    fn ensure_dynamic(&mut self, key: u64, mesh: &Arc<Mesh>, frame: u64) -> MeshId {
        let (v, center, radius) = Self::vertex_data(mesh);
        let same = self.dyn_meshes.get(&key).is_some_and(|g| g.count as usize == mesh.idx.len() && g.vbuf.size() == (((v.len() * 4).max(4) as u64 + 3) & !3));
        if same {
            let g = self.dyn_meshes.get_mut(&key).expect("checked");
            self.gpu.queue.write_buffer(&g.vbuf, 0, bytes_of_floats(&v));
            self.gpu.queue.write_buffer(&g.ibuf, 0, bytemuck::cast_slice(&mesh.idx));
            g.center = center;
            g.radius = radius;
            g.texture = mesh.tex.clone();
            g.use_vcol = !mesh.col.is_empty();
            g.last_used = frame;
        } else {
            let g = self.upload_mesh(mesh, Some(mesh.clone()), frame);
            self.dyn_meshes.insert(key, g);
        }
        MeshId::Dyn(key)
    }

    fn ensure_mesh(&mut self, m: &MeshRef, frame: u64) -> MeshId {
        let (key, keep) = match m {
            MeshRef::Builtin(b) => (*b as *const Mesh as usize, None),
            MeshRef::Shared(a) => (Arc::as_ptr(a) as usize, Some(a.clone())),
            MeshRef::Dynamic { key, mesh } => return self.ensure_dynamic(*key, mesh, frame),
        };
        if !self.meshes.contains_key(&key) {
            let g = self.upload_mesh(m.get(), keep, frame);
            self.meshes.insert(key, g);
        }
        self.meshes.get_mut(&key).expect("just inserted").last_used = frame;
        MeshId::Obj(key)
    }

    fn ensure_tile_mesh(&mut self, m: &MeshRef, key: u64, frame: u64) -> MeshId {
        if !self.tile_meshes.contains_key(&key) {
            let keep = match m {
                MeshRef::Shared(a) | MeshRef::Dynamic { mesh: a, .. } => Some(a.clone()),
                MeshRef::Builtin(_) => None,
            };
            let g = self.upload_mesh(m.get(), keep, frame);
            self.tile_meshes.insert(key, g);
        }
        self.tile_meshes.get_mut(&key).expect("just inserted").last_used = frame;
        MeshId::Tile(key)
    }

    /// One particle cloud as an object of the list (its quads are in world space already) and the batch that draws it.
    fn cloud_batch(&mut self, p: &SceneParticles, high: bool, frame: u64, objects: &mut Vec<ObjectData>) -> Batch {
        let id = self.ensure_mesh(&p.mesh, frame);
        let tkey = self.texture_group(p.tex.as_ref(), None, high, SamplerKind::LinearClamp, frame);
        let data = ObjectData {
            model: lunaris_render::math::M4::identity().0,
            n0: [1.0, 0.0, 0.0, 0.0],
            n1: [0.0, 1.0, 0.0, 0.0],
            n2: [0.0, 0.0, 1.0, 0.0],
            color: [1.0; 4],
            mat: [0.0; 4],
            misc: [1.0, 1.0, 1.0, 0.0],
            uvrect: [0.0, 0.0, 1.0, 1.0],
            ids: [0, 0, 0, 0],
            pbr: [0.0; 4],
        };
        let batch = Batch { mesh: id, tex: tkey, first: objects.len() as u32, count: 1, cast: false, splat: None };
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
    fn ensure_texture(&mut self, tex: &Arc<Texture>, high: bool, frame: u64) {
        let key = (Arc::as_ptr(tex) as usize, high);
        if let Some(t) = self.textures.get_mut(&key) {
            t.last_used = frame;
            return;
        }
        let (w, h) = (tex.width.max(1), tex.height.max(1));
        let mut bytes = Vec::with_capacity((w * h * 4) as usize);
        let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        for t in &tex.texels {
            bytes.extend_from_slice(&[to8(t[0].max(0.0).sqrt()), to8(t[1].max(0.0).sqrt()), to8(t[2].max(0.0).sqrt()), to8(t[3])]);
        }
        let levels = if high { mip_chain(w, h, bytes, true) } else { vec![(w, h, bytes)] };
        let format = if high { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm };
        let t = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("texture"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (i, (lw, lh, data)) in levels.iter().enumerate() {
            self.gpu.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &t, mip_level: i as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, data, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(lw * 4), rows_per_image: Some(*lh) }, wgpu::Extent3d { width: *lw, height: *lh, depth_or_array_layers: 1 });
        }
        let view = t.create_view(&wgpu::TextureViewDescriptor::default());
        // the relief of a built-in material (only the high-quality mode uses it)
        let height = match (&tex.heights, high) {
            (Some(hm), true) if hm.len() == (w * h) as usize => {
                let packed: Vec<u8> = hm.iter().flat_map(|v| { let b = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8; [b, b, b, 255] }).collect();
                let chain = mip_chain(w, h, packed, false);
                let ht = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("height"),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: chain.len() as u32,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                for (i, (lw, lh, data)) in chain.iter().enumerate() {
                    let r: Vec<u8> = data.chunks_exact(4).map(|p| p[0]).collect();
                    self.gpu.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &ht, mip_level: i as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &r, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(*lw), rows_per_image: Some(*lh) }, wgpu::Extent3d { width: *lw, height: *lh, depth_or_array_layers: 1 });
                }
                Some(ht.create_view(&wgpu::TextureViewDescriptor::default()))
            }
            _ => None,
        };
        self.textures.insert(key, GpuTex { view, height, _keep: Some(tex.clone()), last_used: frame });
    }

    /// Upload a splat map (once; it is kept while it is used): its four weights per texel as a plain 8-bit RGBA texture. A map bigger than the card's largest texture is
    /// thinned out (every other texel, or every third...) to fit.
    fn ensure_splat_texture(&mut self, s: &Arc<Splat>, frame: u64) -> usize {
        let key = Arc::as_ptr(s) as usize;
        if let Some(g) = self.splat_textures.get_mut(&key) {
            g.last_used = frame;
            return key;
        }
        let limit = self.gpu.device.limits().max_texture_dimension_2d.max(256) as usize;
        let step = s.cols.max(s.rows).div_ceil(limit).max(1);
        let (w, h) = (s.cols.div_ceil(step), s.rows.div_ceil(step));
        let mut bytes = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                bytes.extend_from_slice(&s.texels[(y * step).min(s.rows - 1) * s.cols + (x * step).min(s.cols - 1)]);
            }
        }
        let t = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("splat-map"),
            size: wgpu::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.gpu.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &bytes, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w as u32 * 4), rows_per_image: Some(h as u32) }, wgpu::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 });
        let view = t.create_view(&wgpu::TextureViewDescriptor::default());
        self.splat_textures.insert(key, GpuSplat { view, _keep: s.clone(), last_used: frame });
        key
    }

    /// The bind group of one painted terrain (`TerrainLayers`): the splat map, up to four layer textures with their relief maps, and the numbers of `SplatU`. Made for
    /// this picture and pushed on `bgs`; the index comes back.
    fn splat_group(&mut self, sp: &SplatLayers, high: bool, frame: u64, bgs: &mut Vec<wgpu::BindGroup>) -> usize {
        let skey = self.ensure_splat_texture(&sp.splat, frame);
        let mut keys: [Option<(usize, bool)>; 4] = [None; 4];
        let mut u: SplatU = Zeroable::zeroed();
        u.tint = [[1.0; 4]; 4];
        for (i, l) in sp.layers.iter().take(4).enumerate() {
            self.ensure_texture(&l.tex, high, frame);
            keys[i] = Some((Arc::as_ptr(&l.tex) as usize, high));
            u.scales[i / 2][(i % 2) * 2] = l.uv_scale[0];
            u.scales[i / 2][(i % 2) * 2 + 1] = l.uv_scale[1];
            // only a built-in material has a relief to show
            u.bump[i] = if l.tex.heights.is_some() { l.bump } else { 0.0 };
            let t = if high { display_to_linear(gamma2_to_display(l.tint)) } else { l.tint };
            u.tint[i] = [t[0], t[1], t[2], 0.0];
        }
        u.info = [sp.layers.len().min(4) as f32, if u.bump.iter().any(|b| *b > 0.0) { 1.0 } else { 0.0 }, 0.0, 0.0];
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
        let layer_sampler = if high { &self.samplers.aniso } else { &self.samplers.nearest_repeat };
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
        bgs.push(bg);
        bgs.len() - 1
    }

    /// The bind group (texture + sampler) for drawing with `tex` (None = plain white).
    fn texture_group(&mut self, tex: Option<&Arc<Texture>>, maps: Option<&MeshPbr>, high: bool, kind: SamplerKind, frame: u64) -> TexKey {
        let ptr = match tex {
            Some(t) => {
                self.ensure_texture(t, high, frame);
                Arc::as_ptr(t) as usize
            }
            None => 0,
        };
        let data_ptr = |this: &mut Self, t: &Option<Arc<Texture>>| match t {
            Some(t) if high => {
                this.ensure_data_texture(t, frame);
                Arc::as_ptr(t) as usize
            }
            _ => 0,
        };
        let (np, ap, dp) = match maps {
            Some(m) => (data_ptr(self, &m.normal), data_ptr(self, &m.arm), data_ptr(self, &m.disp)),
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
        let (w, h) = (f.cols as u32, f.rows as u32);
        let t = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("grass-heights"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let data: Vec<f32> = f.samples().iter().map(|v| *v as f32 * (1.0 / 65535.0)).collect();
        self.gpu.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, bytemuck::cast_slice(&data), wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
        self.grass_heights.insert(key, (t.create_view(&wgpu::TextureViewDescriptor::default()), f.clone(), frame));
        key
    }

    /// Upload a normal or occlusion-roughness-metallic map: the picture's own bytes in a plain 8-bit texture (the loader squares colours, so the bytes come back by the root), with mips.
    fn ensure_data_texture(&mut self, tex: &Arc<Texture>, frame: u64) {
        let key = Arc::as_ptr(tex) as usize;
        if let Some(t) = self.data_textures.get_mut(&key) {
            t.last_used = frame;
            return;
        }
        let (w, h) = (tex.width.max(1), tex.height.max(1));
        let to8 = |v: f32| (v.clamp(0.0, 1.0).sqrt() * 255.0 + 0.5) as u8;
        let mut bytes = Vec::with_capacity((w * h * 4) as usize);
        for t in &tex.texels {
            bytes.extend_from_slice(&[to8(t[0]), to8(t[1]), to8(t[2]), 255]);
        }
        let levels = mip_chain(w, h, bytes, false);
        let t = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("data-map"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (i, (lw, lh, data)) in levels.iter().enumerate() {
            self.gpu.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &t, mip_level: i as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, data, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(lw * 4), rows_per_image: Some(*lh) }, wgpu::Extent3d { width: *lw, height: *lh, depth_or_array_layers: 1 });
        }
        self.data_textures.insert(key, GpuTex { view: t.create_view(&wgpu::TextureViewDescriptor::default()), height: None, _keep: Some(tex.clone()), last_used: frame });
    }

    /// Forget meshes and textures nothing has used for a while (tile maps that were edited, models that left the scene).
    fn evict_old(&mut self, frame: u64) {
        let dead = |last: u64| frame.saturating_sub(last) > 256;
        self.meshes.retain(|_, g| !dead(g.last_used));
        self.tile_meshes.retain(|_, g| !dead(g.last_used));
        self.dyn_meshes.retain(|_, g| !dead(g.last_used));
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
                    self.ensure_texture(t, false, frame);
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
        let rt = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let high = mode == Mode::High;
        let samples = if high { msaa } else { 1 };
        let hdr_tex = self.texture("hdr", w, h, 1, 1, 1, HDR, rt);
        let hdr = view(&hdr_tex);
        let hdr_ms = if samples > 1 { Some(view(&self.texture("hdr-ms", w, h, 1, 1, samples, HDR, rt))) } else { None };
        let depth = view(&self.texture("depth", w, h, 1, 1, samples, DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT));
        let bg_resolve = self.group("resolve", &self.layouts.resolve, &[wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&hdr) }]);
        let mut t = Targets { w, h, mode, msaa, hdr, hdr_ms, depth, pre_depth: None, ao: None, ao_raw: None, gi_raw: None, bloom: None, bg_resolve, bg_ssao: None, bg_gi: None, bg_blur: None };
        if high {
            let pre = view(&self.texture("pre-depth", w, h, 1, 1, 1, DEPTH, rt));
            let ao_raw = view(&self.texture("ao-raw", w, h, 1, 1, 1, AO_RAW, rt));
            let ao = [view(&self.texture("ao", w, h, 1, 1, 1, AO, rt)), view(&self.texture("ao-history", w, h, 1, 1, 1, AO, rt))];
            let gi_raw = view(&self.texture("gi-raw", w, h, 1, 1, 1, GI_RAW, rt));
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
    fn ensure_point_shadow(&mut self, size: u32) {
        if self.point_shadow.as_ref().is_some_and(|s| s.size == size) {
            return;
        }
        let tex = self.texture("point-shadow", size, size, 12, 1, 1, DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let array_view = tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let layers = (0..12u32).map(|i| tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_array_layer: i, array_layer_count: Some(1), ..Default::default() })).collect();
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

    fn draw(&mut self, scene: &Scene, w: u32, h: u32, out: Out<'_>, q: &Quality, want_ids: bool, adapt: Option<f32>) -> Result<(), String> {
        // the sixteen lights that matter most for this picture: the sun and other directional lights first, then the point and spot lights nearest the camera (a light's reach counts, so a big far lamp is not lost behind many small near ones);
        // with sixteen or fewer nothing changes
        let lights_sel: Vec<lunaris_render::render::LightData> = if scene.all_lights.len() <= MAX_LIGHTS {
            scene.all_lights.clone()
        } else {
            let eye = scene.cam.pos;
            let mut idx: Vec<usize> = (0..scene.all_lights.len()).collect();
            let key = |l: &lunaris_render::render::LightData| if l.point { let d = length(sub(l.vec, eye)); 1.0 + d - l.range * 0.5 } else { f32::MIN };
            idx.sort_by(|&a, &b| key(&scene.all_lights[a]).partial_cmp(&key(&scene.all_lights[b])).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b)));
            idx.truncate(MAX_LIGHTS);
            idx.iter().map(|&i| scene.all_lights[i]).collect()
        };
        let mode = q.mode;
        let high = mode == Mode::High;
        let samples = if high { if q.msaa >= 4 { 4 } else { 1 } } else { 1 };
        self.frame_no += 1;
        let frame_no = self.frame_no;
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
        struct MeshItem {
            id: MeshId,
            tex: TexKey,
            cast: bool,
            /// Glows (a lamp's bulb): it must not shadow the light that sits inside it.
            glows: bool,
            /// See-through (high quality only): drawn after the opaque meshes, far to near, blended.
            clear: bool,
            /// Distance in front of the camera, for sorting.
            depth: f32,
            data: ObjectData,
            sphere: (V3, f32),
            /// A painted terrain's bind group (see `Batch::splat`).
            splat: Option<usize>,
        }
        let mut splat_bgs: Vec<wgpu::BindGroup> = Vec::new();
        let mut items: Vec<MeshItem> = Vec::with_capacity(scene.meshes.len());
        let mut slots: Vec<u64> = Vec::with_capacity(scene.meshes.len() + scene.layers.len());
        for m in &scene.meshes {
            slots.push(m.uid);
            let id = self.ensure_mesh(&m.mesh, frame_no);
            let (use_vcol, model_tex, center, radius) = {
                let g = self.mesh(id);
                (g.use_vcol, g.texture.clone(), g.center, g.radius)
            };
            let tex = m.tex.clone().or(model_tex);
            let splat = m.splat.as_ref().map(|sp| self.splat_group(sp, high, frame_no, &mut splat_bgs));
            let tkey = self.texture_group(tex.as_ref(), m.pbr.as_ref(), high, if high { SamplerKind::Aniso } else { SamplerKind::NearestRepeat }, frame_no);
            let color = if high { display_to_linear(gamma2_to_display(m.color)) } else { m.color };
            let nm = m.model.normal_matrix();
            let scale = {
                let c = |i: usize| (m.model.0[i * 4].powi(2) + m.model.0[i * 4 + 1].powi(2) + m.model.0[i * 4 + 2].powi(2)).sqrt();
                c(0).max(c(1)).max(c(2))
            };
            let data = ObjectData {
                model: m.model.0,
                n0: [nm[0][0], nm[1][0], nm[2][0], 0.0],
                n1: [nm[0][1], nm[1][1], nm[2][1], 0.0],
                n2: [nm[0][2], nm[1][2], nm[2][2], 0.0],
                color: [color[0], color[1], color[2], if high { m.alpha } else { 1.0 }],
                mat: [m.shininess as f32, m.emissive, m.metallic, m.roughness],
                misc: [m.uv_scale[0], m.uv_scale[1], if tex.is_some() { 1.0 } else { 0.0 }, if use_vcol { 1.0 } else { 0.0 }],
                uvrect: match (m.water, m.sway) {
                    (Some(wv), _) if high => [0.0, wv[0], wv[1], wv[2]],
                    (None, Some(sw)) if high => [0.0, sw[0], sw[1], sw[2]],
                    _ => [if high && tex.as_ref().is_some_and(|t| t.heights.is_some()) { m.bump } else { 0.0 }, if high { m.pbr.as_ref().filter(|p| p.disp.is_some()).map_or(0.0, |p| p.height_scale) } else { 0.0 }, 1.0, 1.0],
                },
                ids: [slots.len() as u32, if !high { 0 } else if m.water.is_some() { 1 } else if m.sway.is_some() { 2 } else { 0 }, 0, 0],
                pbr: match &m.pbr {
                    Some(p) if high && (p.normal.is_some() || p.arm.is_some() || p.disp.is_some()) => [p.normal_strength, p.ao_strength, p.normal.is_some() as u32 as f32 + 2.0 * p.arm.is_some() as u32 as f32 + 4.0 * p.disp.is_some() as u32 as f32, p.roughness_scale],
                    _ => [0.0; 4],
                },
            };
            let see_through = high && m.alpha < 0.999;
            let depth = -scene.view.transform_point(m.model.transform_point(center))[2];
            items.push(MeshItem { id, tex: tkey, cast: m.cast_shadow && !see_through, glows: m.emissive > 0.0, clear: see_through, depth, data, sphere: (m.model.transform_point(center), radius * scale), splat });
        }
        let mut order: Vec<usize> = (0..items.len()).filter(|&i| !items[i].clear).collect();
        order.sort_by_key(|&i| (items[i].id, items[i].tex.0, items[i].tex.2 as u8, items[i].tex.3, items[i].tex.4, items[i].tex.5, items[i].cast, items[i].splat));
        let mut objects: Vec<ObjectData> = Vec::with_capacity(items.len() + scene.layers.len());
        let mut mesh_batches: Vec<Batch> = Vec::new();
        for &i in &order {
            let it = &items[i];
            let same = mesh_batches.last().is_some_and(|b| b.mesh == it.id && b.tex == it.tex && b.cast == it.cast && b.splat == it.splat);
            if same {
                mesh_batches.last_mut().expect("checked").count += 1;
            } else {
                mesh_batches.push(Batch { mesh: it.id, tex: it.tex, first: objects.len() as u32, count: 1, cast: it.cast, splat: it.splat });
            }
            objects.push(it.data);
        }
        // see-through meshes: far to near, runs of the same mesh and texture drawn together
        let mut clear_order: Vec<usize> = (0..items.len()).filter(|&i| items[i].clear).collect();
        clear_order.sort_by(|&a, &b| items[b].depth.partial_cmp(&items[a].depth).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b)));
        let mut clear_batches: Vec<Batch> = Vec::new();
        for &i in &clear_order {
            let it = &items[i];
            if clear_batches.last().is_some_and(|b| b.mesh == it.id && b.tex == it.tex) {
                clear_batches.last_mut().expect("checked").count += 1;
            } else {
                clear_batches.push(Batch { mesh: it.id, tex: it.tex, first: objects.len() as u32, count: 1, cast: false, splat: None });
            }
            objects.push(it.data);
        }
        let casters = Casters { spheres: items.iter().filter(|i| i.cast).map(|i| i.sphere).collect() };
        // the sprite and tile layers in drawing order; a particle cloud with a `DrawLayer` stands among them (each step names the pipeline that draws it)
        let mut layer_steps: Vec<(PipeId, Batch)> = Vec::new();
        let mut among = scene.particles.iter().filter(|p| p.slot.is_some()).peekable();
        for (i, l) in scene.layers.iter().enumerate() {
            while let Some(p) = among.next_if(|p| p.slot.is_some_and(|s| s <= i)) {
                let batch = self.cloud_batch(p, high, frame_no, &mut objects);
                layer_steps.push((if p.additive { PipeId::ParticleAdd } else { PipeId::Particle }, batch));
            }
            slots.push(l.uid);
            let id = if l.mesh_key != 0 { self.ensure_tile_mesh(&l.mesh, l.mesh_key, frame_no) } else { self.ensure_mesh(&l.mesh, frame_no) };
            let tkey = self.texture_group(l.tex.as_ref(), None, high, SamplerKind::NearestClamp, frame_no);
            let color = if high { display_to_linear(gamma2_to_display([l.color[0], l.color[1], l.color[2]])) } else { [l.color[0], l.color[1], l.color[2]] };
            let nm = l.model.normal_matrix();
            let data = ObjectData {
                model: l.model.0,
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
                layer_steps.push((PipeId::Sprite, Batch { mesh: id, tex: tkey, first: objects.len() as u32, count: 1, cast: false, splat: None }));
            }
            objects.push(data);
        }
        for p in among {
            let batch = self.cloud_batch(p, high, frame_no, &mut objects);
            layer_steps.push((if p.additive { PipeId::ParticleAdd } else { PipeId::Particle }, batch));
        }
        // the other particle clouds: one object each (the quads are in world space already), after everything, alpha-blended ones first, then the added light
        let (mut particle_alpha, mut particle_add): (Vec<Batch>, Vec<Batch>) = (Vec::new(), Vec::new());
        for p in scene.particles.iter().filter(|p| p.slot.is_none()) {
            let batch = self.cloud_batch(p, high, frame_no, &mut objects);
            if p.additive {
                particle_add.push(batch);
            } else {
                particle_alpha.push(batch);
            }
        }
        // ---- point lights that cast shadows: the two that matter most (bright, close, with a long reach), each with the casters in its range
        let mut point_sel: Vec<usize> = Vec::new();
        let mut point_batches: Vec<Vec<Batch>> = Vec::new();
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
            for &(_, li) in cand.iter().take(2) {
                let l = &lights_sel[li];
                let mut batches: Vec<Batch> = Vec::new();
                for &i in &order {
                    let it = &items[i];
                    if !it.cast {
                        continue;
                    }
                    let (c, r) = it.sphere;
                    let d = length(sub(c, l.vec));
                    if d > l.range + r || (it.glows && d < r) {
                        continue;
                    }
                    if batches.last().is_some_and(|b| b.mesh == it.id) {
                        batches.last_mut().expect("checked").count += 1;
                    } else {
                        batches.push(Batch { mesh: it.id, tex: it.tex, first: objects.len() as u32, count: 1, cast: true, splat: None });
                    }
                    objects.push(it.data);
                }
                point_sel.push(li);
                point_batches.push(batches);
            }
        }
        // grass: an object (the material numbers) for each field and a uniform for each of its three rings, which cover the ground round the camera from fine tufts near to big ones far
        struct GrassDraw {
            mesh: MeshId,
            tex: TexKey,
            hkey: usize,
            ubuf: usize,
            instances: u32,
        }
        let mut grass_draws: Vec<GrassDraw> = Vec::new();
        let mut grass_us: Vec<GrassU> = Vec::new();
        if high {
            let eye = scene.cam.pos;
            for g in &scene.grass {
                let id = self.ensure_mesh(&g.mesh, frame_no);
                let (mesh_tex, count) = {
                    let gm = self.mesh(id);
                    (gm.texture.clone(), gm.count)
                };
                if count == 0 {
                    continue;
                }
                let tex = g.tex.clone().or(mesh_tex);
                let tkey = self.texture_group(tex.as_ref(), None, true, SamplerKind::Aniso, frame_no);
                let hkey = self.ensure_grass_heights(&g.heights, frame_no);
                objects.push(ObjectData {
                    model: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                    n0: [1.0, 0.0, 0.0, 0.0],
                    n1: [0.0, 1.0, 0.0, 0.0],
                    n2: [0.0, 0.0, 1.0, 0.0],
                    color: [1.0, 1.0, 1.0, 1.0],
                    mat: [32.0, 0.0, 0.0, 0.92],
                    misc: [1.0, 1.0, if tex.is_some() { 1.0 } else { 0.0 }, 1.0],
                    uvrect: [0.0, 0.0, 1.0, 1.0],
                    ids: [0, 0, 0, 0],
                    pbr: [0.0; 4],
                });
                let slot = (objects.len() - 1) as f32;
                let near_cell = 1.0 / g.density.sqrt();
                let edges = [0.0, g.radius * 0.26, g.radius * 0.62, g.radius];
                for k in 0..3usize {
                    let cell = near_cell * 2.4f32.powi(k as i32);
                    let outer = edges[k + 1];
                    let inner = if k == 0 { 0.0 } else { edges[k] * 0.8 };
                    let cells = ((2.0 * outer / cell).ceil() as u32 + 2).min(640);
                    let mult = 2.4f32.powf(0.85 * k as f32);
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
                    });
                    grass_draws.push(GrassDraw { mesh: id, tex: tkey, hkey, ubuf: grass_us.len() - 1, instances: cells * cells });
                }
            }
        }
        while self.grass_bufs.len() < grass_us.len() {
            self.grass_bufs.push(self.gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("grass-ring"), size: std::mem::size_of::<GrassU>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }));
        }
        for (i, u) in grass_us.iter().enumerate() {
            self.gpu.queue.write_buffer(&self.grass_bufs[i], 0, bytemuck::bytes_of(u));
        }
        self.grow_objects(objects.len().max(1) as u64);
        if !objects.is_empty() {
            self.gpu.queue.write_buffer(&self.objects, 0, bytemuck::cast_slice(&objects));
        }

        let text_slots: Vec<u32> = scene.texts.iter().map(|t| {
            slots.push(t.uid);
            slots.len() as u32
        }).collect();
        self.slot_uids = slots;

        // ---- which effects run
        let have_clear = !clear_batches.is_empty();
        let have_meshes = !scene.meshes.is_empty();
        let sun = lights_sel.iter().position(|l| !l.point);
        let shadows_on = high && q.shadows && env.shadows && have_meshes && sun.is_some() && !casters.spheres.is_empty();
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
        let need_depth = ao_pass || dof_on || shafts_on || blur_on || refl_on || flare_on || vol_on;
        let bloom_on = high && q.bloom && env.bloom > 0.0;
        // eye adaptation needs something to measure: a flat 2D scene keeps its authored colours
        let ae_on = high && have_meshes && env.auto_exposure.is_some() && !probe_out;
        let aspect = w as f32 / h as f32;

        // ---- the frame uniform
        let vp = to_gpu_clip(&scene.view_proj);
        let view = scene.view;
        let lin = |c: V3| if high { display_to_linear(c) } else { c };
        let mut fu: FrameU = Zeroable::zeroed();
        fu.view_proj = vp.0;
        fu.inv_view_proj = inverse(&vp).0;
        fu.view = view.0;
        let cascades = if shadows_on {
            self.ensure_shadow(q.shadow_size.clamp(256, 8192));
            let sun_dir = lights_sel[sun.expect("checked")].vec;
            Some(fit_cascades(&scene.cam, aspect, sun_dir, env.shadow_distance, q.shadow_size.clamp(256, 8192), &casters))
        } else {
            None
        };
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
            self.ensure_point_shadow(psize);
        }
        for k in 0..2 {
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
        fu.sky_top = v4(lin(env.sky_top), 0.0);
        fu.sky_horizon = v4(lin(env.sky_horizon), 0.0);
        fu.ground = v4(lin(env.ground), 0.0);
        fu.fog = v4(lin(env.fog_color), env.fog);
        {
            let c = scene.cam.clear;
            fu.clear_color = v4(lin([c[0], c[1], c[2]]), 0.0);
        }
        let tonemap = match env.tonemap {
            0 => if have_meshes { 1.0 } else { 3.0 },
            n => n as f32,
        };
        fu.p0 = [env.ambient, env.exposure, if ssao_on { env.ssao } else { 0.0 }, if shadows_on { 1.0 } else { 0.0 }];
        fu.p1 = [w as f32, h as f32, 1.0 / w as f32, 1.0 / h as f32];
        fu.p2 = [if high { 1.0 } else { 0.0 }, lights_sel.len().min(MAX_LIGHTS) as f32, tonemap, env.saturation];
        fu.p3 = [if bloom_on { env.bloom } else { 0.0 }, env.bloom_threshold, env.vignette, if env.sky { 1.0 } else { 0.0 }];
        fu.p5 = [if env.sky { env.clouds } else { 0.0 }, psize as f32, 0.0, 0.0];
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
        self.prev_vp = Some(vp.0);
        fu.p4 = [if shadows_on { sun.expect("checked") as f32 } else { -1.0 }, 1.0, samples as f32, if scene.cam.ortho.is_some() { 1.0 } else { 0.0 }];
        for (i, l) in lights_sel.iter().take(MAX_LIGHTS).enumerate() {
            fu.lights[i * 4] = v4(l.vec, if l.point { 1.0 } else { 0.0 });
            fu.lights[i * 4 + 1] = v4(l.color, l.range);
            // the classic mode draws every spot light as a point light, like the reference renderer
            fu.lights[i * 4 + 2] = v4(l.spot_dir, if high { l.cos_outer } else { -2.0 });
            fu.lights[i * 4 + 3] = [l.cos_inner, 0.0, 0.0, 0.0];
        }
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
        self.need(PipeId::Sprite, samples, None)?;
        if !particle_alpha.is_empty() || layer_steps.iter().any(|s| s.0 == PipeId::Particle) {
            self.need(PipeId::Particle, samples, None)?;
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
                    entries: &[wgpu::BindGroupEntry { binding: 12, resource: self.grass_bufs[d.ubuf].as_entire_binding() }, wgpu::BindGroupEntry { binding: 13, resource: wgpu::BindingResource::TextureView(&self.grass_heights[&d.hkey].0) }],
                })
            })
            .collect();
        if high {
            self.need(PipeId::Sky, samples, None)?;
            if probe_out {
                self.need(PipeId::ProbeResolve, 1, None)?;
            } else {
                self.need(PipeId::Composite, samples, Some(out_format))?;
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
            ],
        );
        let depth_bgs: Vec<wgpu::BindGroup> = (0..16).map(|i| self.group("depth", &self.layouts.depth0, &[wgpu::BindGroupEntry { binding: 0, resource: self.pass_bufs[i].as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: self.objects.as_entire_binding() }])).collect();

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

        // ---- encode
        let t = self.targets.as_ref().expect("targets exist");
        let mut enc = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        let draw_batches = |pass: &mut wgpu::RenderPass<'_>, batches: &[Batch], only_casters: bool, textured: bool| {
            for b in batches {
                if only_casters && !b.cast {
                    continue;
                }
                let g = self.mesh(b.mesh);
                if g.count == 0 {
                    continue;
                }
                if textured {
                    pass.set_bind_group(1, &self.tex_bgs[&b.tex], &[]);
                }
                pass.set_vertex_buffer(0, g.vbuf.slice(..));
                pass.set_index_buffer(g.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..g.count, 0, b.first..b.first + b.count);
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
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, bg, &[]);
            pass.draw(0..3, 0..1);
        };

        if let (Some(c), Some(s)) = (&cascades, &self.shadow) {
            let _ = c;
            for i in 0..3 {
                let mut pass = depth_pass(&mut enc, "shadow", &s.layers[i], 1.0);
                pass.set_pipeline(self.pipe(PipeId::DepthShadow, 1, None));
                pass.set_bind_group(0, &depth_bgs[i + 1], &[]);
                draw_batches(&mut pass, &mesh_batches, true, false);
            }
        }
        if let Some(ps) = &self.point_shadow {
            for (k, batches) in point_batches.iter().enumerate() {
                for f in 0..6 {
                    let mut pass = depth_pass(&mut enc, "point-shadow", &ps.layers[k * 6 + f], 1.0);
                    pass.set_pipeline(self.pipe(PipeId::DepthShadow, 1, None));
                    pass.set_bind_group(0, &depth_bgs[4 + k * 6 + f], &[]);
                    draw_batches(&mut pass, batches, false, false);
                }
            }
        }
        if need_depth {
            let pre = t.pre_depth.as_ref().expect("high targets");
            {
                let mut pass = depth_pass(&mut enc, "prepass", pre, 0.0);
                pass.set_pipeline(self.pipe(PipeId::DepthPre, 1, None));
                pass.set_bind_group(0, &depth_bgs[0], &[]);
                draw_batches(&mut pass, &mesh_batches, false, false);
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
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &scene_bg, &[]);
            pass.set_pipeline(self.pipe(PipeId::SsrGbuf, 1, None));
            draw_batches(&mut pass, &mesh_batches, false, true);
            draw_batches(&mut pass, &clear_batches, false, true);
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
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &t.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }), stencil_ops: None }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &scene_bg, &[]);
            if sky {
                pass.set_pipeline(self.pipe(PipeId::Sky, samples, None));
                pass.draw(0..3, 0..1);
            }
            pass.set_pipeline(self.pipe(PipeId::Mesh, samples, None));
            if splat_bgs.is_empty() {
                draw_batches(&mut pass, &mesh_batches, false, true);
            } else {
                let plain: Vec<Batch> = mesh_batches.iter().filter(|b| b.splat.is_none()).cloned().collect();
                draw_batches(&mut pass, &plain, false, true);
                pass.set_pipeline(self.pipe(PipeId::MeshSplat, samples, None));
                for b in mesh_batches.iter().filter(|b| b.splat.is_some()) {
                    pass.set_bind_group(1, self.splat_tex_bg.as_ref().expect("made above"), &[]);
                    pass.set_bind_group(2, &splat_bgs[b.splat.expect("filtered")], &[]);
                    draw_batches(&mut pass, std::slice::from_ref(b), false, false);
                }
                pass.set_pipeline(self.pipe(PipeId::Mesh, samples, None));
            }
            if !grass_draws.is_empty() {
                pass.set_pipeline(self.pipe(PipeId::Grass, samples, None));
                for (i, d) in grass_draws.iter().enumerate() {
                    let gm = self.mesh(d.mesh);
                    pass.set_bind_group(1, &self.tex_bgs[&d.tex], &[]);
                    pass.set_bind_group(2, &grass_bgs[i], &[]);
                    pass.set_vertex_buffer(0, gm.vbuf.slice(..));
                    pass.set_index_buffer(gm.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..gm.count, 0, 0..d.instances);
                }
            }
            if have_clear {
                pass.set_pipeline(self.pipe(PipeId::MeshBlend, samples, None));
                draw_batches(&mut pass, &clear_batches, false, true);
            }
            pass.set_pipeline(self.pipe(PipeId::Sprite, samples, None));
            let mut current = PipeId::Sprite;
            for (pipe, batch) in &layer_steps {
                if *pipe != current {
                    pass.set_pipeline(self.pipe(*pipe, samples, None));
                    current = *pipe;
                }
                draw_batches(&mut pass, std::slice::from_ref(batch), false, true);
            }
            if !particle_alpha.is_empty() {
                pass.set_pipeline(self.pipe(PipeId::Particle, samples, None));
                draw_batches(&mut pass, &particle_alpha, false, true);
            }
            if !particle_add.is_empty() {
                pass.set_pipeline(self.pipe(PipeId::ParticleAdd, samples, None));
                draw_batches(&mut pass, &particle_add, false, true);
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
                fullscreen(&mut enc, "composite", out_view, wgpu::LoadOp::Clear(wgpu::Color::BLACK), self.pipe(PipeId::Composite, samples, Some(out_format)), composite_bg.as_ref().expect("high composite"));
            }
            if nverts > 0 && !probe_out {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("text"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: out_view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
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
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_bind_group(0, &scene_bg, &[]);
                pass.set_pipeline(self.pipe(PipeId::IdMesh, 1, None));
                draw_batches(&mut pass, &mesh_batches, false, true);
                draw_batches(&mut pass, &clear_batches, false, true);
                pass.set_pipeline(self.pipe(PipeId::IdSprite, 1, None));
                for (_, batch) in layer_steps.iter().filter(|s| s.0 == PipeId::Sprite) {
                    draw_batches(&mut pass, std::slice::from_ref(batch), false, true);
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
        self.gpu.queue.submit([enc.finish()]);
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
        let q = Quality { mode: Mode::High, msaa: 1, shadow_size: base.shadow_size, point_shadow_size: base.point_shadow_size, shadows: true, ssao: false, bloom: false, dof: false };
        let dirs: [(V3, V3); 6] = [([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), ([0.0, 1.0, 0.0], [0.0, 0.0, -1.0]), ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]), ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]), ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0])];
        // what the pictures of the main view keep from one to the next is put back afterwards
        let (ae_valid, prev_vp, nested) = (self.ae_valid, self.prev_vp, textures.nested);
        textures.nested = true;
        let mut result: Result<(), String> = Ok(());
        for (f, (fwd, up)) in dirs.iter().enumerate() {
            let camera = ExplicitCamera { eye: p.pos, target: [p.pos[0] + fwd[0], p.pos[1] + fwd[1], p.pos[2] + fwd[2]], up: *up, fov_y: 94.0, ortho_height: None, near: 0.05, far: 1000.0, clear };
            let opts = RenderOptions { width: size, height: size, view: View::Explicit(camera), supersample: 1, threads: 1 };
            let drawn = gather_scene(world, textures, &opts).map_err(|e| e.message).and_then(|scene| {
                let view = self.probe_set.as_ref().expect("probe set").faces[slot * 6 + f][0].clone();
                self.draw(&scene, size, size, Out::Probe { view: &view }, &q, false, None)
            });
            if drawn.is_err() {
                result = drawn;
                break;
            }
        }
        textures.nested = nested;
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
    pub fn render_frame(&mut self, world: &mut World, textures: &mut TextureCache, opts: &RenderOptions, quality: &Quality, want_ids: bool) -> Result<Frame, String> {
        if opts.width == 0 || opts.height == 0 || opts.width > 8192 || opts.height > 8192 {
            return Err("image size must be between 1x1 and 8192x8192".into());
        }
        let mut scene = gather_scene(world, textures, opts).map_err(|e| e.message)?;
        scene.warnings.append(&mut textures.warnings);
        if quality.mode == Mode::High {
            self.sync_probes(world, textures, &scene.probes, scene.cam.clear, quality, None)?;
        }
        let (w, h) = (opts.width, opts.height);
        let adapt = self.adapt_dt;
        // bounced light is read from the last picture and blended over the last ones: a still lets it settle (seven pictures, a running average)
        if quality.mode == Mode::High && quality.ssao && scene.env.gi[0] > 0.0 {
            // the same pictures in the same order every time: a still made twice is the same picture
            self.gi_age = 0;
            self.fx_frame = 0;
            for _ in 0..7 {
                self.draw(&scene, w, h, Out::Image, quality, false, adapt)?;
            }
        }
        self.draw(&scene, w, h, Out::Image, quality, want_ids, adapt)?;
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
        // what the eye adaptation measured and did, read from the small buffer the picture copied it into
        let mut info: Vec<(String, f64)> = Vec::new();
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
        if width == 0 || height == 0 {
            return Ok(Vec::new());
        }
        self.resize(width, height);
        let opts = RenderOptions { width, height, ..RenderOptions::default() };
        let mut scene = gather_scene(world, textures, &opts).map_err(|e| e.message)?;
        scene.warnings.append(&mut textures.warnings);
        if quality.mode == Mode::High {
            self.sync_probes(world, textures, &scene.probes, scene.cam.clear, quality, None)?;
        }
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
        let format = self.surface.as_ref().expect("checked").config.format;
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        // pictures of a window follow each other: the eye adapts by the time between them (it starts from the measured value after a long pause)
        let now = std::time::Instant::now();
        let adapt = self.last_window_pic.map(|t| now.duration_since(t).as_secs_f32()).filter(|d| *d < 2.0).map(|d| d.min(0.25));
        self.last_window_pic = Some(now);
        self.draw(&scene, width, height, Out::Surface { view: &view, format }, quality, false, adapt)?;
        self.gpu.queue.present(frame);
        self.frames_presented += 1;
        Ok(scene.warnings)
    }
}
