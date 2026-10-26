// Skinning on the card: a skinned model's vertices are moved from the rest pose by the joint matrices of this picture's pose, one thread a vertex, into the vertex buffer the draw passes read
// (the same twelve floats a vertex as `Renderer::vertex_data`: position, normal, texture coordinates, colour, texture strength). It does what `Rig::skin` does on the processor:
// a vertex with no influence stays where it was, a joint that is not in the matrices is left out.
struct SkinU {
    // the vertices of the model, where its matrices start in `mats` (counted in matrices), and how many it has
    count: u32,
    mat_base: u32,
    joints: u32,
    pad: u32,
};

@group(0) @binding(0) var<storage, read> rest: array<f32>;
// six words a vertex: two with the four joint numbers (16 bits each), then the four weights as floats
@group(0) @binding(1) var<storage, read> jw: array<u32>;
// four columns a matrix
@group(0) @binding(2) var<storage, read> mats: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> skinned: array<f32>;
@group(0) @binding(4) var<uniform> u: SkinU;

@compute @workgroup_size(64)
fn cs_skin(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = gid.x + gid.y * groups.x * 64u;
    if (i >= u.count) {
        return;
    }
    let o = i * 12u;
    let pos = vec3<f32>(rest[o], rest[o + 1u], rest[o + 2u]);
    let nrm = vec3<f32>(rest[o + 3u], rest[o + 4u], rest[o + 5u]);
    let a = jw[i * 6u];
    let b = jw[i * 6u + 1u];
    var joint = array<u32, 4>(a & 0xffffu, a >> 16u, b & 0xffffu, b >> 16u);
    var p = vec3<f32>(0.0);
    var n = vec3<f32>(0.0);
    var total = 0.0;
    for (var k = 0u; k < 4u; k = k + 1u) {
        let w = bitcast<f32>(jw[i * 6u + 2u + k]);
        if (w > 0.0 && joint[k] < u.joints) {
            let m = (u.mat_base + joint[k]) * 4u;
            let c0 = mats[m];
            let c1 = mats[m + 1u];
            let c2 = mats[m + 2u];
            let c3 = mats[m + 3u];
            p = p + w * (c0.xyz * pos.x + c1.xyz * pos.y + c2.xyz * pos.z + c3.xyz);
            n = n + w * (c0.xyz * nrm.x + c1.xyz * nrm.y + c2.xyz * nrm.z);
            total = total + w;
        }
    }
    var out_pos = pos;
    var out_nrm = nrm;
    if (total > 0.0) {
        out_pos = p;
        let len = length(n);
        out_nrm = select(nrm, n / len, len > 0.0);
    }
    skinned[o] = out_pos.x;
    skinned[o + 1u] = out_pos.y;
    skinned[o + 2u] = out_pos.z;
    skinned[o + 3u] = out_nrm.x;
    skinned[o + 4u] = out_nrm.y;
    skinned[o + 5u] = out_nrm.z;
    for (var k = 6u; k < 12u; k = k + 1u) {
        skinned[o + k] = rest[o + k];
    }
}
