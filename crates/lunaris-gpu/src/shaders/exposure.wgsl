// Eye adaptation (`AutoExposure`): measure how bright the picture is and move the exposure towards the value that brings it to the target.
//
// One workgroup of 256 threads reads a 64 x 64 grid of points spread over the (resolved, HDR) picture, adds up the logarithms of their luminance (the middle of the picture
// counts a little more than the corners, the flat background not at all) and the first thread works out the geometric mean and adapts the exposure kept in `state`:
//   state[0].x = the exposure the composite applies, .y = the average luminance measured, .z = the exposure that was aimed at.
// The exposure only depends on the picture and the previous exposure, so the picture never feeds back into what it measures (the measured light is not yet exposed).

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var hdr_tex: texture_2d<f32>;
@group(0) @binding(2) var<storage, read_write> state: array<vec4<f32>>;

var<workgroup> sum_log: array<f32, 256>;
var<workgroup> sum_w: array<f32, 256>;

const GRID: u32 = 64u;

@compute @workgroup_size(256)
fn cs_adapt(@builtin(local_invocation_index) li: u32) {
    let size = vec2<f32>(frame.p1.xy);
    var acc = 0.0;
    var wacc = 0.0;
    for (var k = 0u; k < 16u; k = k + 1u) {
        let idx = li + k * 256u;
        let g = vec2<f32>(f32(idx % GRID), f32(idx / GRID));
        let uv = (g + vec2<f32>(0.5)) / f32(GRID);
        let px = vec2<i32>(min(uv * size, size - vec2<f32>(1.0)));
        let c = textureLoad(hdr_tex, px, 0);
        let a = clamp(c.a, 0.0, 1.0);
        if (a > 0.02) {
            let lum = luminance(max(c.rgb / a, vec3<f32>(0.0)));
            let d = (uv - vec2<f32>(0.5)) * 2.0;
            let w = a * (1.0 - 0.6 * min(dot(d, d), 1.0));
            acc = acc + log2(max(lum, 0.0001)) * w;
            wacc = wacc + w;
        }
    }
    sum_log[li] = acc;
    sum_w[li] = wacc;
    workgroupBarrier();
    for (var s = 128u; s > 0u; s = s >> 1u) {
        if (li < s) {
            sum_log[li] = sum_log[li] + sum_log[li + s];
            sum_w[li] = sum_w[li] + sum_w[li + s];
        }
        workgroupBarrier();
    }
    if (li == 0u) {
        let prev = state[0].x;
        let w = sum_w[0];
        var avg = 0.0;
        var aim = 1.0;
        if (w > 4.0) {
            avg = exp2(sum_log[0] / w);
            aim = frame.p14.w / avg;
        } else if (prev > 0.0) {
            // nothing but the flat background to measure: stay as we are
            aim = prev;
        }
        aim = clamp(aim, frame.p14.x, frame.p14.y);
        var e = aim;
        if (frame.p15.z < 0.5 && prev > 0.0) {
            // glide in stops: each second covers 1 - e^-speed of what is left
            let k = 1.0 - exp(-frame.p14.z * frame.p15.y);
            e = exp2(mix(log2(prev), log2(aim), k));
        }
        e = clamp(e, frame.p14.x, frame.p14.y);
        state[0] = vec4<f32>(e, avg, aim, 0.0);
    }
}
