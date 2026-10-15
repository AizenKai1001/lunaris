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
fn vs_depth(in: VsIn) -> @builtin(position) vec4<f32> {
    let o = objects[in.inst];
    return pass_u.view_proj * (o.model * vec4<f32>(in.pos, 1.0));
}
