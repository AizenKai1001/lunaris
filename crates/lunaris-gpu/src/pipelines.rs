//! Shader modules, bind group layouts and render pipelines. Pipelines are created on first use and cached by what they depend on: the
//! kind of pass, the multisample count and the colour format they draw into.
use std::borrow::Cow;
use std::collections::HashMap;
use wgpu::*;

pub const HDR: TextureFormat = TextureFormat::Rgba16Float;
pub const DEPTH: TextureFormat = TextureFormat::Depth32Float;
pub const AO_RAW: TextureFormat = TextureFormat::Rg16Float;
/// What the blur makes: the bounced light in rgb and the ambient occlusion in a.
pub const AO: TextureFormat = TextureFormat::Rgba16Float;
pub const GI_RAW: TextureFormat = TextureFormat::Rgba16Float;
pub const IDS: TextureFormat = TextureFormat::R32Uint;
/// The surface pictures of the screen-space reflections: where each pixel points and how rough it is, how much it reflects head on.
pub const GBUF_A: TextureFormat = TextureFormat::Rgba16Float;
pub const GBUF_B: TextureFormat = TextureFormat::Rgba8Unorm;
/// Bytes of one interleaved mesh vertex: position 3, normal 3, uv 2, colour 3, texture weight 1 (floats).
pub const MESH_STRIDE: u64 = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PipeId {
    Mesh,
    /// Painted terrain (`TerrainLayers`): the mesh shader with a splat map and up to four layers in a third bind group.
    MeshSplat,
    /// Grass made on the card (`GrassField`): the mesh shader's fragment with a vertex shader that places the tufts itself.
    Grass,
    /// See-through meshes: sorted far to near, blended over the picture, not writing depth.
    MeshBlend,
    Sprite,
    /// Particle clouds: alpha-blended and added.
    Particle,
    ParticleAdd,
    IdMesh,
    IdSprite,
    Sky,
    /// Text drawn inside the main pass (classic mode: into the linear picture, with the pass's depth attachment).
    TextInPass,
    /// Text in the entity-id picture.
    IdText,
    /// Text drawn on top of the finished picture (high-quality mode), no depth.
    TextOut,
    /// Interface shapes (rectangles, pictures, TrueType letters): the same two places as the text.
    UiInPass,
    UiOut,
    DepthPre,
    DepthShadow,
    Resolve,
    Ssao,
    /// Temporal anti-aliasing: the picture blended with the last ones, drawn to the output and to the history.
    Taa,
    /// Bounced light from the last picture (`GlobalIllumination`).
    Gi,
    SsaoBlur,
    BloomFirst,
    BloomDown,
    BloomUp,
    Composite,
    /// The mesh pixels' normals, roughness and reflectance for the screen-space reflections (two colour targets, one sample).
    SsrGbuf,
    /// The screen-space reflections themselves (full screen).
    Ssr,
    /// A face of a reflection probe: the finished HDR picture as plain light.
    ProbeResolve,
    /// The next mip level of a face of a probe.
    ProbeMip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PipeKey {
    pub id: PipeId,
    pub samples: u32,
    pub format: Option<TextureFormat>,
}

pub struct Shaders {
    pub scene: ShaderModule,
    pub depth: ShaderModule,
    pub post: ShaderModule,
    pub post_ms: ShaderModule,
    pub text: ShaderModule,
    /// The compute shader of the eye adaptation.
    pub exposure: ShaderModule,
}

pub struct Layouts {
    pub scene0: BindGroupLayout,
    pub tex1: BindGroupLayout,
    /// `tex1` without the normal and arm maps: a painted terrain's pipeline has its own nine textures and the card allows 16 per stage.
    pub tex1_small: BindGroupLayout,
    /// A grass field: its numbers and the picture of the ground (heights) it stands on.
    pub grass2: BindGroupLayout,
    pub splat2: BindGroupLayout,
    pub depth0: BindGroupLayout,
    pub text0: BindGroupLayout,
    pub resolve: BindGroupLayout,
    pub ssao: BindGroupLayout,
    pub gi0: BindGroupLayout,
    pub taa0: BindGroupLayout,
    pub blur: BindGroupLayout,
    pub bloom_first: BindGroupLayout,
    pub bloom_chain: BindGroupLayout,
    pub composite: BindGroupLayout,
    pub composite_ms: BindGroupLayout,
    /// The screen-space reflections: the frame, the opaque depth, the picture, a sampler, the depth of the nearest surface, the two surface pictures, the probes' faces and their sampler.
    pub ssr: BindGroupLayout,
    /// A face of a reflection probe being finished: the frame (for the background colour) and the picture.
    pub probe_resolve: BindGroupLayout,
    /// The eye adaptation (compute): the frame uniform, the picture, the state it keeps.
    pub ae0: BindGroupLayout,
}

const COMMON: &str = include_str!("shaders/common.wgsl");
/// The sky, the environment BRDF and the fog along a ray: what the mesh shader and the post-processing passes share.
const SKY: &str = include_str!("shaders/sky.wgsl");

fn module(device: &Device, label: &str, source: &str) -> ShaderModule {
    device.create_shader_module(ShaderModuleDescriptor { label: Some(label), source: ShaderSource::Wgsl(Cow::Owned(source.to_string())) })
}

impl Shaders {
    pub fn new(device: &Device) -> Shaders {
        let with_common = |s: &str| format!("{COMMON}\n{s}");
        let with_sky = |s: &str| format!("{COMMON}\n{SKY}\n{s}");
        Shaders {
            scene: module(device, "scene", &with_sky(include_str!("shaders/scene.wgsl"))),
            depth: module(device, "depth", &with_common(include_str!("shaders/depth.wgsl"))),
            post: module(device, "post", &with_sky(include_str!("shaders/post.wgsl"))),
            // the multisampled composite is the same file; only the entry point differs, so one module is enough, but a second one
            // keeps the single-sample layout free of the multisampled binding
            post_ms: module(device, "post-ms", &with_sky(include_str!("shaders/post.wgsl"))),
            text: module(device, "text", include_str!("shaders/text.wgsl")),
            exposure: module(device, "exposure", &with_common(include_str!("shaders/exposure.wgsl"))),
        }
    }
}

fn entry(binding: u32, ty: BindingType) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry { binding, visibility: ShaderStages::VERTEX_FRAGMENT, ty, count: None }
}

fn uniform() -> BindingType {
    BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }
}

fn storage_ro() -> BindingType {
    BindingType::Buffer { ty: BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }
}

fn tex_float() -> BindingType {
    BindingType::Texture { sample_type: TextureSampleType::Float { filterable: true }, view_dimension: TextureViewDimension::D2, multisampled: false }
}

fn tex_float_array() -> BindingType {
    BindingType::Texture { sample_type: TextureSampleType::Float { filterable: true }, view_dimension: TextureViewDimension::D2Array, multisampled: false }
}

fn tex_unfiltered() -> BindingType {
    BindingType::Texture { sample_type: TextureSampleType::Float { filterable: false }, view_dimension: TextureViewDimension::D2, multisampled: false }
}

fn tex_depth() -> BindingType {
    BindingType::Texture { sample_type: TextureSampleType::Depth, view_dimension: TextureViewDimension::D2, multisampled: false }
}

fn tex_depth_array() -> BindingType {
    BindingType::Texture { sample_type: TextureSampleType::Depth, view_dimension: TextureViewDimension::D2Array, multisampled: false }
}

fn tex_ms() -> BindingType {
    BindingType::Texture { sample_type: TextureSampleType::Float { filterable: false }, view_dimension: TextureViewDimension::D2, multisampled: true }
}

fn sampler_filtering() -> BindingType {
    BindingType::Sampler(SamplerBindingType::Filtering)
}

fn sampler_compare() -> BindingType {
    BindingType::Sampler(SamplerBindingType::Comparison)
}

impl Layouts {
    pub fn new(device: &Device) -> Layouts {
        let make = |label: &str, entries: &[BindGroupLayoutEntry]| device.create_bind_group_layout(&BindGroupLayoutDescriptor { label: Some(label), entries });
        Layouts {
            scene0: make("scene0", &[entry(0, uniform()), entry(1, storage_ro()), entry(2, tex_depth_array()), entry(3, sampler_compare()), entry(4, tex_float()), entry(5, sampler_filtering()), entry(6, tex_depth_array()), entry(7, tex_float_array()), entry(8, sampler_filtering()), entry(9, tex_depth())]),
            tex1: make("tex1", &[entry(0, tex_float()), entry(1, sampler_filtering()), entry(2, tex_float()), entry(3, tex_float()), entry(4, tex_float()), entry(5, tex_float())]),
            tex1_small: make("tex1-small", &[entry(0, tex_float()), entry(1, sampler_filtering()), entry(2, tex_float())]),
            grass2: make("grass2", &[entry(12, uniform()), entry(13, tex_unfiltered())]),
            // a painted terrain: its numbers, the splat map and its sampler, the layers' sampler, four colour textures and four relief maps
            splat2: make("splat2", &[entry(0, uniform()), entry(1, tex_float()), entry(2, sampler_filtering()), entry(3, sampler_filtering()), entry(4, tex_float()), entry(5, tex_float()), entry(6, tex_float()), entry(7, tex_float()), entry(8, tex_float()), entry(9, tex_float()), entry(10, tex_float()), entry(11, tex_float())]),
            depth0: make("depth0", &[entry(0, uniform()), entry(1, storage_ro())]),
            text0: make("text0", &[entry(0, uniform()), entry(1, tex_float()), entry(2, sampler_filtering())]),
            resolve: make("resolve", &[entry(2, tex_float())]),
            ssao: make("ssao", &[entry(0, uniform()), entry(1, tex_depth())]),
            taa0: make("taa0", &[entry(0, uniform()), entry(1, tex_depth()), entry(2, tex_float()), entry(5, sampler_filtering()), entry(16, tex_float())]),
            gi0: make("gi0", &[entry(0, uniform()), entry(1, tex_depth()), entry(2, tex_float())]),
            blur: make("ssao-blur", &[entry(0, uniform()), entry(1, tex_depth()), entry(5, sampler_filtering()), entry(6, tex_float()), entry(14, tex_float()), entry(15, tex_float())]),
            bloom_first: make("bloom-first", &[entry(0, uniform()), entry(2, tex_float()), entry(5, sampler_filtering()), entry(8, storage_ro())]),
            bloom_chain: make("bloom-chain", &[entry(4, tex_float()), entry(5, sampler_filtering())]),
            composite: make("composite", &[entry(0, uniform()), entry(1, tex_depth()), entry(2, tex_float()), entry(4, tex_float()), entry(5, sampler_filtering()), entry(7, tex_float()), entry(8, storage_ro())]),
            composite_ms: make("composite-ms", &[entry(0, uniform()), entry(1, tex_depth()), entry(3, tex_ms()), entry(4, tex_float()), entry(5, sampler_filtering()), entry(7, tex_float()), entry(8, storage_ro())]),
            ssr: make("ssr", &[entry(0, uniform()), entry(1, tex_depth()), entry(2, tex_float()), entry(5, sampler_filtering()), entry(9, tex_depth()), entry(10, tex_float()), entry(11, tex_float()), entry(12, tex_float_array()), entry(13, sampler_filtering())]),
            probe_resolve: make("probe-resolve", &[entry(0, uniform()), entry(2, tex_float())]),
            ae0: make(
                "ae0",
                &[
                    BindGroupLayoutEntry { binding: 0, visibility: ShaderStages::COMPUTE, ty: uniform(), count: None },
                    BindGroupLayoutEntry { binding: 1, visibility: ShaderStages::COMPUTE, ty: tex_float(), count: None },
                    BindGroupLayoutEntry { binding: 2, visibility: ShaderStages::COMPUTE, ty: BindingType::Buffer { ty: BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }, count: None },
                ],
            ),
        }
    }
}

fn mesh_vertex_layout() -> VertexBufferLayout<'static> {
    const ATTRS: [VertexAttribute; 5] = vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x3, 4 => Float32];
    VertexBufferLayout { array_stride: MESH_STRIDE, step_mode: VertexStepMode::Vertex, attributes: &ATTRS }
}

fn text_vertex_layout() -> VertexBufferLayout<'static> {
    const ATTRS: [VertexAttribute; 3] = vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
    VertexBufferLayout { array_stride: 32, step_mode: VertexStepMode::Vertex, attributes: &ATTRS }
}

fn depth_state(write: bool, compare: CompareFunction, bias: DepthBiasState) -> DepthStencilState {
    DepthStencilState { format: DEPTH, depth_write_enabled: Some(write), depth_compare: Some(compare), stencil: StencilState::default(), bias }
}

const ALPHA_BLEND: BlendState = BlendState {
    color: BlendComponent { src_factor: BlendFactor::SrcAlpha, dst_factor: BlendFactor::OneMinusSrcAlpha, operation: BlendOperation::Add },
    alpha: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::OneMinusSrcAlpha, operation: BlendOperation::Add },
};

const PREMULTIPLIED: BlendState = BlendState {
    color: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::OneMinusSrcAlpha, operation: BlendOperation::Add },
    alpha: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::OneMinusSrcAlpha, operation: BlendOperation::Add },
};

const ADD_BLEND: BlendState = BlendState {
    color: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
    alpha: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
};

pub struct PipelineCache2 {
    map: HashMap<PipeKey, RenderPipeline>,
}

impl PipelineCache2 {
    pub fn new() -> PipelineCache2 {
        PipelineCache2 { map: HashMap::new() }
    }

    pub fn get(&self, key: &PipeKey) -> &RenderPipeline {
        &self.map[key]
    }

    pub fn contains(&self, key: &PipeKey) -> bool {
        self.map.contains_key(key)
    }

    pub fn insert(&mut self, key: PipeKey, p: RenderPipeline) {
        self.map.insert(key, p);
    }
}

/// The compute pipeline of the eye adaptation.
pub fn build_exposure(device: &Device, sh: &Shaders, ly: &Layouts) -> ComputePipeline {
    let pl = device.create_pipeline_layout(&PipelineLayoutDescriptor { label: Some("ae"), bind_group_layouts: &[Some(&ly.ae0)], immediate_size: 0 });
    device.create_compute_pipeline(&ComputePipelineDescriptor { label: Some("eye-adaptation"), layout: Some(&pl), module: &sh.exposure, entry_point: Some("cs_adapt"), compilation_options: Default::default(), cache: None })
}

/// Build the pipeline for `key`.
pub fn build(device: &Device, sh: &Shaders, ly: &Layouts, key: PipeKey) -> RenderPipeline {
    let layout = |label: &str, groups: &[&BindGroupLayout]| {
        let v: Vec<Option<&BindGroupLayout>> = groups.iter().map(|g| Some(*g)).collect();
        device.create_pipeline_layout(&PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &v, immediate_size: 0 })
    };
    let ms = MultisampleState { count: key.samples, mask: !0, alpha_to_coverage_enabled: false };
    let none = MultisampleState::default();
    let tri = |cull: Option<Face>| PrimitiveState { topology: PrimitiveTopology::TriangleList, strip_index_format: None, front_face: FrontFace::Ccw, cull_mode: cull, unclipped_depth: false, polygon_mode: PolygonMode::Fill, conservative: false };
    let target = |format: TextureFormat, blend: Option<BlendState>| Some(ColorTargetState { format, blend, write_mask: ColorWrites::ALL });
    let no_bias = DepthBiasState::default();
    match key.id {
        PipeId::Mesh => {
            let pl = layout("mesh", &[&ly.scene0, &ly.tex1]);
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("mesh"),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.scene, entry_point: Some("vs_mesh"), compilation_options: Default::default(), buffers: &[Some(mesh_vertex_layout())] },
                primitive: tri(Some(Face::Back)),
                depth_stencil: Some(depth_state(true, CompareFunction::Greater, no_bias)),
                multisample: ms,
                fragment: Some(FragmentState { module: &sh.scene, entry_point: Some("fs_mesh"), compilation_options: Default::default(), targets: &[target(HDR, None)] }),
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::Grass => {
            let pl = layout("grass", &[&ly.scene0, &ly.tex1, &ly.grass2]);
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("grass"),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.scene, entry_point: Some("vs_grass"), compilation_options: Default::default(), buffers: &[Some(mesh_vertex_layout())] },
                primitive: tri(None),
                depth_stencil: Some(depth_state(true, CompareFunction::Greater, no_bias)),
                multisample: ms,
                fragment: Some(FragmentState { module: &sh.scene, entry_point: Some("fs_mesh"), compilation_options: Default::default(), targets: &[target(HDR, None)] }),
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::Taa => {
            let pl = layout("taa", &[&ly.taa0]);
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("fs_taa"),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.post, entry_point: Some("vs_full"), compilation_options: Default::default(), buffers: &[] },
                primitive: tri(None),
                depth_stencil: None,
                multisample: none,
                fragment: Some(FragmentState { module: &sh.post, entry_point: Some("fs_taa"), compilation_options: Default::default(), targets: &[target(key.format.expect("format"), None), target(HDR, None)] }),
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::MeshSplat => {
            let pl = layout("mesh-splat", &[&ly.scene0, &ly.tex1_small, &ly.splat2]);
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("mesh-splat"),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.scene, entry_point: Some("vs_mesh"), compilation_options: Default::default(), buffers: &[Some(mesh_vertex_layout())] },
                primitive: tri(Some(Face::Back)),
                depth_stencil: Some(depth_state(true, CompareFunction::Greater, no_bias)),
                multisample: ms,
                fragment: Some(FragmentState { module: &sh.scene, entry_point: Some("fs_mesh_splat"), compilation_options: Default::default(), targets: &[target(HDR, None)] }),
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::MeshBlend => {
            let pl = layout("mesh-blend", &[&ly.scene0, &ly.tex1]);
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("mesh-blend"),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.scene, entry_point: Some("vs_mesh"), compilation_options: Default::default(), buffers: &[Some(mesh_vertex_layout())] },
                primitive: tri(Some(Face::Back)),
                depth_stencil: Some(depth_state(false, CompareFunction::Greater, no_bias)),
                multisample: ms,
                fragment: Some(FragmentState { module: &sh.scene, entry_point: Some("fs_mesh_blend"), compilation_options: Default::default(), targets: &[target(HDR, Some(PREMULTIPLIED))] }),
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::Sprite => {
            let pl = layout("sprite", &[&ly.scene0, &ly.tex1]);
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("sprite"),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.scene, entry_point: Some("vs_sprite"), compilation_options: Default::default(), buffers: &[Some(mesh_vertex_layout())] },
                primitive: tri(None),
                depth_stencil: Some(depth_state(false, CompareFunction::Greater, no_bias)),
                multisample: ms,
                fragment: Some(FragmentState { module: &sh.scene, entry_point: Some("fs_sprite"), compilation_options: Default::default(), targets: &[target(HDR, Some(ALPHA_BLEND))] }),
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::Particle | PipeId::ParticleAdd => {
            let add = key.id == PipeId::ParticleAdd;
            let pl = layout("particle", &[&ly.scene0, &ly.tex1]);
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("particle"),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.scene, entry_point: Some("vs_particle"), compilation_options: Default::default(), buffers: &[Some(mesh_vertex_layout())] },
                primitive: tri(None),
                depth_stencil: Some(depth_state(false, CompareFunction::Greater, no_bias)),
                multisample: ms,
                fragment: Some(FragmentState { module: &sh.scene, entry_point: Some(if add { "fs_particle_add" } else { "fs_particle" }), compilation_options: Default::default(), targets: &[target(HDR, Some(if add { ADD_BLEND } else { ALPHA_BLEND }))] }),
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::IdMesh | PipeId::IdSprite => {
            let mesh = key.id == PipeId::IdMesh;
            let pl = layout("ids", &[&ly.scene0, &ly.tex1]);
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("ids"),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.scene, entry_point: Some(if mesh { "vs_mesh" } else { "vs_sprite" }), compilation_options: Default::default(), buffers: &[Some(mesh_vertex_layout())] },
                primitive: tri(if mesh { Some(Face::Back) } else { None }),
                depth_stencil: Some(depth_state(mesh, CompareFunction::Greater, no_bias)),
                multisample: none,
                fragment: Some(FragmentState { module: &sh.scene, entry_point: Some(if mesh { "fs_id_mesh" } else { "fs_id_sprite" }), compilation_options: Default::default(), targets: &[target(IDS, None)] }),
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::Sky => {
            let pl = layout("sky", &[&ly.scene0]);
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("sky"),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.scene, entry_point: Some("vs_sky"), compilation_options: Default::default(), buffers: &[] },
                primitive: tri(None),
                depth_stencil: Some(depth_state(false, CompareFunction::Always, no_bias)),
                multisample: ms,
                fragment: Some(FragmentState { module: &sh.scene, entry_point: Some("fs_sky"), compilation_options: Default::default(), targets: &[target(HDR, None)] }),
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::TextInPass => {
            let pl = layout("text-in-pass", &[&ly.text0]);
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("text-in-pass"),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.text, entry_point: Some("vs_text"), compilation_options: Default::default(), buffers: &[Some(text_vertex_layout())] },
                primitive: tri(None),
                depth_stencil: Some(depth_state(false, CompareFunction::Always, no_bias)),
                multisample: ms,
                fragment: Some(FragmentState { module: &sh.text, entry_point: Some("fs_text"), compilation_options: Default::default(), targets: &[target(HDR, Some(ALPHA_BLEND))] }),
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::UiInPass | PipeId::UiOut => {
            let in_pass = key.id == PipeId::UiInPass;
            let pl = layout("ui", &[&ly.text0]);
            let format = if in_pass { HDR } else { key.format.expect("an interface pipeline needs a format") };
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("ui"),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.text, entry_point: Some("vs_text"), compilation_options: Default::default(), buffers: &[Some(text_vertex_layout())] },
                primitive: tri(None),
                depth_stencil: if in_pass { Some(depth_state(false, CompareFunction::Always, no_bias)) } else { None },
                multisample: if in_pass { ms } else { none },
                fragment: Some(FragmentState { module: &sh.text, entry_point: Some(if in_pass { "fs_ui_sq" } else { "fs_ui" }), compilation_options: Default::default(), targets: &[target(format, Some(ALPHA_BLEND))] }),
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::IdText => {
            let pl = layout("text-id", &[&ly.text0]);
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("text-id"),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.text, entry_point: Some("vs_text"), compilation_options: Default::default(), buffers: &[Some(text_vertex_layout())] },
                primitive: tri(None),
                depth_stencil: Some(depth_state(false, CompareFunction::Always, no_bias)),
                multisample: none,
                fragment: Some(FragmentState { module: &sh.text, entry_point: Some("fs_text_id"), compilation_options: Default::default(), targets: &[target(IDS, None)] }),
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::TextOut => {
            let pl = layout("text-out", &[&ly.text0]);
            let format = key.format.expect("a text pipeline needs a format");
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("text-out"),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.text, entry_point: Some("vs_text"), compilation_options: Default::default(), buffers: &[Some(text_vertex_layout())] },
                primitive: tri(None),
                depth_stencil: None,
                multisample: none,
                fragment: Some(FragmentState { module: &sh.text, entry_point: Some("fs_text"), compilation_options: Default::default(), targets: &[target(format, Some(ALPHA_BLEND))] }),
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::SsrGbuf => {
            let pl = layout("ssr-gbuf", &[&ly.scene0, &ly.tex1]);
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("ssr-gbuf"),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.scene, entry_point: Some("vs_mesh"), compilation_options: Default::default(), buffers: &[Some(mesh_vertex_layout())] },
                primitive: tri(Some(Face::Back)),
                depth_stencil: Some(depth_state(true, CompareFunction::Greater, no_bias)),
                multisample: none,
                fragment: Some(FragmentState { module: &sh.scene, entry_point: Some("fs_gbuf"), compilation_options: Default::default(), targets: &[target(GBUF_A, None), target(GBUF_B, None)] }),
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::DepthPre | PipeId::DepthShadow => {
            let shadow = key.id == PipeId::DepthShadow;
            let pl = layout("depth", &[&ly.depth0]);
            let (compare, bias) = if shadow { (CompareFunction::Less, DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 }) } else { (CompareFunction::Greater, no_bias) };
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some(if shadow { "shadow" } else { "prepass" }),
                layout: Some(&pl),
                vertex: VertexState { module: &sh.depth, entry_point: Some("vs_depth"), compilation_options: Default::default(), buffers: &[Some(mesh_vertex_layout())] },
                // shadow maps draw back faces too (thin walls still shadow); the prepass matches the main pass
                primitive: tri(if shadow { None } else { Some(Face::Back) }),
                depth_stencil: Some(depth_state(true, compare, bias)),
                multisample: none,
                fragment: None,
                multiview_mask: None,
                cache: None,
            })
        }
        PipeId::Resolve | PipeId::Ssao | PipeId::Gi | PipeId::SsaoBlur | PipeId::BloomFirst | PipeId::BloomDown | PipeId::BloomUp | PipeId::Composite | PipeId::Ssr | PipeId::ProbeResolve | PipeId::ProbeMip => {
            let (module, bgl, fs, format, blend): (&ShaderModule, &BindGroupLayout, &str, TextureFormat, Option<BlendState>) = match key.id {
                PipeId::Resolve => (&sh.post, &ly.resolve, "fs_resolve", key.format.expect("format"), None),
                PipeId::Ssao => (&sh.post, &ly.ssao, "fs_ssao", AO_RAW, None),
                PipeId::Gi => (&sh.post, &ly.gi0, "fs_gi", GI_RAW, None),
                PipeId::SsaoBlur => (&sh.post, &ly.blur, "fs_ssao_blur", AO, None),
                PipeId::BloomFirst => (&sh.post, &ly.bloom_first, "fs_bloom_first", HDR, None),
                PipeId::BloomDown => (&sh.post, &ly.bloom_chain, "fs_bloom_down", HDR, None),
                PipeId::BloomUp => (&sh.post, &ly.bloom_chain, "fs_bloom_up", HDR, Some(ADD_BLEND)),
                PipeId::Ssr => (&sh.post, &ly.ssr, "fs_ssr", HDR, None),
                PipeId::ProbeResolve => (&sh.post, &ly.probe_resolve, "fs_probe_resolve", HDR, None),
                PipeId::ProbeMip => (&sh.post, &ly.bloom_chain, "fs_probe_mip", HDR, None),
                _ => {
                    if key.samples > 1 {
                        (&sh.post_ms, &ly.composite_ms, "fs_composite_ms", key.format.expect("format"), None)
                    } else {
                        (&sh.post, &ly.composite, "fs_composite", key.format.expect("format"), None)
                    }
                }
            };
            let pl = layout("post", &[bgl]);
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&pl),
                vertex: VertexState { module, entry_point: Some("vs_full"), compilation_options: Default::default(), buffers: &[] },
                primitive: tri(None),
                depth_stencil: None,
                multisample: none,
                fragment: Some(FragmentState { module, entry_point: Some(fs), compilation_options: Default::default(), targets: &[target(format, blend)] }),
                multiview_mask: None,
                cache: None,
            })
        }
    }
}
