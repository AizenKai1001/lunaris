// The atmosphere's in-scattered light, integrated once a picture for a grid of view directions (and read back by `atmosphere_sky` in sky.wgsl with a bilinear look-up and the phase functions applied per pixel):
// a LUT of the kind Hillaire's sky-view table is (Hillaire, "A Scalable and Production Ready Sky and Atmosphere Rendering Technique", 2020), here for single scattering. Along the one axis the angle between the
// view's heading and the sun's (0 to 180 degrees: the sky is the same on both sides of the sun's vertical plane), along the other the elevation of the view (0 to 90 degrees, squared so that the horizon, where the
// sky changes fastest, has most of the rows). Two numbers for each: the Rayleigh sum and the Mie sum (colour each, without their phase functions: those depend on the view alone and are exact per pixel).

@group(0) @binding(0) var<uniform> frame: Frame;
// (the sky file also holds the reflection probes' look-up, which names these: stand-ins, this pass does not use them)
@group(0) @binding(7) var probe_tex: texture_2d_array<f32>;
@group(0) @binding(8) var probe_samp: sampler;
@group(0) @binding(14) var<storage, read_write> lut_out: array<vec4<f32>>;

const LUT_VIEW_STEPS: i32 = 24;
const LUT_LIGHT_STEPS: i32 = 8;

@compute @workgroup_size(8, 8, 1)
fn cs_sky_lut(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= 64u || id.y >= 64u) {
        return;
    }
    let phi = f32(id.x) / 63.0 * PI;
    let v = f32(id.y) / 63.0;
    let elev = v * v * (0.5 * PI);
    let sun = normalize(frame.sun_dir.xyz);
    var sx = vec2<f32>(1.0, 0.0);
    if (length(sun.xz) > 1.0e-4) {
        sx = normalize(sun.xz);
    }
    let perp = vec2<f32>(-sx.y, sx.x);
    let hz = sx * cos(phi) + perp * sin(phi);
    let d = vec3<f32>(hz.x * cos(elev), sin(elev), hz.y * cos(elev));
    let ray = frame.p25.y;
    let turb = frame.p25.x * 0.5;
    let h0 = clamp(frame.eye.y * 0.001, 0.0, 5.0);
    let o = vec3<f32>(0.0, R_EARTH + h0 + 0.0005, 0.0);
    let beta_r = vec3<f32>(0.0058, 0.0135, 0.0331);
    let beta_m = 0.021;
    // a view ray is followed for at most 120 km (see `atmosphere_sky`)
    let t_max = min(far_hit(o, d, R_TOP), 120.0);
    var sum_r = vec3<f32>(0.0);
    var sum_m = vec3<f32>(0.0);
    if (t_max > 0.0) {
        let ds = t_max / f32(LUT_VIEW_STEPS);
        var od_r = 0.0;
        var od_m = 0.0;
        for (var i = 0; i < LUT_VIEW_STEPS; i = i + 1) {
            let p = o + d * ((f32(i) + 0.5) * ds);
            let h = length(p) - R_EARTH;
            let hr = exp(-h / 8.0) * ds * ray;
            let hm = exp(-h / 1.2) * ds * turb;
            od_r = od_r + hr;
            od_m = od_m + hm;
            let tl = far_hit(p, sun, R_TOP);
            if (tl <= 0.0) {
                continue;
            }
            let dl = tl / f32(LUT_LIGHT_STEPS);
            var odr_l = 0.0;
            var odm_l = 0.0;
            var shadow = false;
            for (var j = 0; j < LUT_LIGHT_STEPS; j = j + 1) {
                let q = p + sun * ((f32(j) + 0.5) * dl);
                let hq = length(q) - R_EARTH;
                if (hq < 0.0) {
                    shadow = true;
                    break;
                }
                odr_l = odr_l + exp(-hq / 8.0) * dl * ray;
                odm_l = odm_l + exp(-hq / 1.2) * dl * turb;
            }
            if (shadow) {
                continue;
            }
            let att = exp(-(beta_r * (od_r + odr_l) + vec3<f32>(beta_m * 1.1 * (od_m + odm_l))));
            sum_r = sum_r + att * hr;
            sum_m = sum_m + att * hm;
        }
    }
    let at = (id.y * 64u + id.x) * 2u;
    lut_out[at] = vec4<f32>(sum_r, 0.0);
    lut_out[at + 1u] = vec4<f32>(sum_m, 0.0);
}
