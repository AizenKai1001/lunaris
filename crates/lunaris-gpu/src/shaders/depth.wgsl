// Depth-only drawing of the meshes: the shadow maps (one pass per cascade) and the depth prepass the ambient occlusion reads.

struct Pass {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> pass_u: Pass;
@group(0) @binding(1) var<storage, read> objects: array<Object>;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @builtin(instance_index) inst: u32,
};

@vertex
fn vs_depth(in: VsIn) -> @builtin(position) @invariant vec4<f32> {
    let o = objects[in.inst];
    return pass_u.view_proj * (o.model * vec4<f32>(in.pos, 1.0));
}

// The same for a cut-out (an imported material with `alphaMode` MASK, n0.w is where it is cut): the picture's alpha decides, so the shadow of a leaf is the leaf's and not its card's.
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var tex_samp: sampler;

struct CutIn {
    @location(0) pos: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @builtin(instance_index) inst: u32,
};

struct CutOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) inst: u32,
};

@vertex
fn vs_depth_cut(in: CutIn) -> CutOut {
    let o = objects[in.inst];
    var out: CutOut;
    out.clip = pass_u.view_proj * (o.model * vec4<f32>(in.pos, 1.0));
    out.uv = in.uv;
    out.inst = in.inst;
    return out;
}

@fragment
fn fs_depth_cut(in: CutOut) {
    let o = objects[in.inst];
    let uvs = in.uv * o.misc.xy;
    let t = textureSample(tex, tex_samp, vec2<f32>(uvs.x, 1.0 - uvs.y));
    if (t.a < o.n0.w) {
        discard;
    }
}
