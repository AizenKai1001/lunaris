// Full-screen passes: the classic resolve (square root back to display colours), ambient occlusion and its blur, the bloom chain, and the
// composite that turns the HDR picture into the final image (exposure, bloom, filmic tone mapping, grading, vignette, sRGB, dithering).
//
// `%MS%` markers are replaced by the host so that the same file serves multisampled and single-sample pictures.

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var hdr_tex: texture_2d<f32>;
@group(0) @binding(3) var hdr_ms: texture_multisampled_2d<f32>;
@group(0) @binding(4) var bloom_tex: texture_2d<f32>;
@group(0) @binding(5) var post_samp: sampler;
@group(0) @binding(6) var ao_in: texture_2d<f32>;

struct FullOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> FullOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: FullOut;
    out.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(p.x, 1.0 - p.y);
    return out;
}

// ------------------------------------------------------------------------------------------------ classic resolve

@fragment
fn fs_resolve(in: FullOut) -> @location(0) vec4<f32> {
    let c = clamp(textureLoad(hdr_tex, vec2<i32>(in.pos.xy), 0).rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(sqrt(c), 1.0);
}

// ------------------------------------------------------------------------------------------------ ambient occlusion

fn world_at(px: vec2<i32>) -> vec3<f32> {
    let size = vec2<i32>(frame.p1.xy);
    let q = clamp(px, vec2<i32>(0), size - vec2<i32>(1));
    let d = textureLoad(depth_tex, q, 0);
    let uv = (vec2<f32>(q) + 0.5) * frame.p1.zw;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let w = frame.inv_view_proj * vec4<f32>(ndc, d, 1.0);
    return w.xyz / w.w;
}

fn depth_at(px: vec2<i32>) -> f32 {
    let size = vec2<i32>(frame.p1.xy);
    return textureLoad(depth_tex, clamp(px, vec2<i32>(0), size - vec2<i32>(1)), 0);
}

const AO_SAMPLES: i32 = 16;

// Output: r = occlusion (1 = open), g = distance from the camera (for the depth-aware blur).
@fragment
fn fs_ssao(in: FullOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    let d0 = depth_at(px);
    if (d0 <= 0.0) {
        return vec4<f32>(1.0, 1.0e6, 0.0, 1.0); // the sky
    }
    let p = world_at(px);
    // the surface normal from the neighbours, taking the side with the smaller depth jump on each axis
    let pl = world_at(px + vec2<i32>(-1, 0));
    let pr = world_at(px + vec2<i32>(1, 0));
    let pu = world_at(px + vec2<i32>(0, -1));
    let pd = world_at(px + vec2<i32>(0, 1));
    var dx = pr - p;
    if (abs(depth_at(px + vec2<i32>(-1, 0)) - d0) < abs(depth_at(px + vec2<i32>(1, 0)) - d0)) {
        dx = p - pl;
    }
    var dy = pd - p;
    if (abs(depth_at(px + vec2<i32>(0, -1)) - d0) < abs(depth_at(px + vec2<i32>(0, 1)) - d0)) {
        dy = p - pu;
    }
    var n = normalize(cross(dx, dy));
    let to_eye = frame.eye.xyz - p;
    if (dot(n, to_eye) < 0.0) {
        n = -n;
    }
    let dist = length(to_eye);
    let radius = clamp(0.45 + dist * 0.02, 0.45, 3.0);
    let bias = 0.02 + dist * 0.002;
    let helper = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.95);
    let t = normalize(cross(helper, n));
    let b = cross(n, t);
    let rnd = interleaved_noise(in.pos.xy);
    var occ = 0.0;
    let size = frame.p1.xy;
    for (var i = 0; i < AO_SAMPLES; i = i + 1) {
        let fi = f32(i);
        let u = (fi + 0.5) / f32(AO_SAMPLES);
        let ang = (fi + rnd) * 2.399963;
        let r = sqrt(u);
        let local = vec3<f32>(r * cos(ang), r * sin(ang), sqrt(max(1.0 - u, 0.0)));
        let s = mix(0.12, 1.0, u * u);
        let sp = p + (t * local.x + b * local.y + n * local.z) * (radius * s);
        let clip = frame.view_proj * vec4<f32>(sp, 1.0);
        if (clip.w <= 0.0) {
            continue;
        }
        let ndc = clip.xy / clip.w;
        let suv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (suv.x < 0.0 || suv.x >= 1.0 || suv.y < 0.0 || suv.y >= 1.0) {
            continue;
        }
        let spx = vec2<i32>(suv * size);
        let scene_pos = world_at(spx);
        let sd = depth_at(spx);
        if (sd <= 0.0) {
            continue;
        }
        let scene_dist = length(scene_pos - frame.eye.xyz);
        let sample_dist = length(sp - frame.eye.xyz);
        let diff = sample_dist - scene_dist;
        if (diff > bias) {
            // ignore occluders that are far in front of the point (they are a different object)
            let range = smoothstep(0.0, 1.0, radius / max(abs(dist - scene_dist), 1.0e-4));
            occ = occ + range;
        }
    }
    let ao = clamp(1.0 - (occ / f32(AO_SAMPLES)) * 1.7 * frame.p0.z, 0.0, 1.0);
    return vec4<f32>(ao, dist, 0.0, 1.0);
}

// A 4 x 4 blur that does not smear across depth edges. Output: r = occlusion.
@fragment
fn fs_ssao_blur(in: FullOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    let size = vec2<i32>(frame.p1.xy);
    let center = textureLoad(ao_in, px, 0);
    var sum = 0.0;
    var wsum = 0.0;
    for (var y = -2; y < 2; y = y + 1) {
        for (var x = -2; x < 2; x = x + 1) {
            let q = clamp(px + vec2<i32>(x, y), vec2<i32>(0), size - vec2<i32>(1));
            let s = textureLoad(ao_in, q, 0);
            let w = exp(-abs(s.g - center.g) * 6.0 / max(center.g, 1.0) * 10.0);
            sum = sum + s.r * w;
            wsum = wsum + w;
        }
    }
    return vec4<f32>(sum / max(wsum, 1.0e-4), 0.0, 0.0, 1.0);
}

// ------------------------------------------------------------------------------------------------ bloom

fn karis(c: vec3<f32>) -> f32 {
    return 1.0 / (1.0 + luminance(c));
}

// Keep only what is brighter than the threshold, with a soft knee.
fn prefilter(c: vec3<f32>) -> vec3<f32> {
    let th = frame.p3.y;
    let br = max(c.r, max(c.g, c.b));
    let knee = th * 0.5 + 1.0e-4;
    var soft = clamp(br - th + knee, 0.0, 2.0 * knee);
    soft = soft * soft / (4.0 * knee);
    let contrib = max(soft, br - th) / max(br, 1.0e-4);
    return c * contrib;
}

// The first step: from the full-size picture to half size, thresholded, weighted so that single very bright pixels do not flicker.
@fragment
fn fs_bloom_first(in: FullOut) -> @location(0) vec4<f32> {
    let dim = vec2<f32>(textureDimensions(hdr_tex));
    let t = 1.0 / dim;
    let uv = in.uv;
    let a = textureSampleLevel(hdr_tex, post_samp, uv + t * vec2<f32>(-2.0, -2.0), 0.0).rgb;
    let b = textureSampleLevel(hdr_tex, post_samp, uv + t * vec2<f32>(0.0, -2.0), 0.0).rgb;
    let c = textureSampleLevel(hdr_tex, post_samp, uv + t * vec2<f32>(2.0, -2.0), 0.0).rgb;
    let d = textureSampleLevel(hdr_tex, post_samp, uv + t * vec2<f32>(-2.0, 0.0), 0.0).rgb;
    let e = textureSampleLevel(hdr_tex, post_samp, uv, 0.0).rgb;
    let f = textureSampleLevel(hdr_tex, post_samp, uv + t * vec2<f32>(2.0, 0.0), 0.0).rgb;
    let g = textureSampleLevel(hdr_tex, post_samp, uv + t * vec2<f32>(-2.0, 2.0), 0.0).rgb;
    let h = textureSampleLevel(hdr_tex, post_samp, uv + t * vec2<f32>(0.0, 2.0), 0.0).rgb;
    let i = textureSampleLevel(hdr_tex, post_samp, uv + t * vec2<f32>(2.0, 2.0), 0.0).rgb;
    let j = textureSampleLevel(hdr_tex, post_samp, uv + t * vec2<f32>(-1.0, -1.0), 0.0).rgb;
    let k = textureSampleLevel(hdr_tex, post_samp, uv + t * vec2<f32>(1.0, -1.0), 0.0).rgb;
    let l = textureSampleLevel(hdr_tex, post_samp, uv + t * vec2<f32>(-1.0, 1.0), 0.0).rgb;
    let m = textureSampleLevel(hdr_tex, post_samp, uv + t * vec2<f32>(1.0, 1.0), 0.0).rgb;
    let g0 = (a + b + d + e) * 0.25;
    let g1 = (b + c + e + f) * 0.25;
    let g2 = (d + e + g + h) * 0.25;
    let g3 = (e + f + h + i) * 0.25;
    let g4 = (j + k + l + m) * 0.25;
    let w0 = karis(g0);
    let w1 = karis(g1);
    let w2 = karis(g2);
    let w3 = karis(g3);
    let w4 = karis(g4);
    let c0 = (g0 * w0 * 0.125 + g1 * w1 * 0.125 + g2 * w2 * 0.125 + g3 * w3 * 0.125 + g4 * w4 * 0.5) / (w0 * 0.125 + w1 * 0.125 + w2 * 0.125 + w3 * 0.125 + w4 * 0.5);
    return vec4<f32>(prefilter(c0), 1.0);
}

@fragment
fn fs_bloom_down(in: FullOut) -> @location(0) vec4<f32> {
    let dim = vec2<f32>(textureDimensions(bloom_tex));
    let t = 1.0 / dim;
    let uv = in.uv;
    let a = textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(-2.0, -2.0), 0.0).rgb;
    let b = textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(0.0, -2.0), 0.0).rgb;
    let c = textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(2.0, -2.0), 0.0).rgb;
    let d = textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(-2.0, 0.0), 0.0).rgb;
    let e = textureSampleLevel(bloom_tex, post_samp, uv, 0.0).rgb;
    let f = textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(2.0, 0.0), 0.0).rgb;
    let g = textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(-2.0, 2.0), 0.0).rgb;
    let h = textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(0.0, 2.0), 0.0).rgb;
    let i = textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(2.0, 2.0), 0.0).rgb;
    let j = textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(-1.0, -1.0), 0.0).rgb;
    let k = textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(1.0, -1.0), 0.0).rgb;
    let l = textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(-1.0, 1.0), 0.0).rgb;
    let m = textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(1.0, 1.0), 0.0).rgb;
    var c0 = e * 0.125;
    c0 = c0 + (a + c + g + i) * 0.03125;
    c0 = c0 + (b + d + f + h) * 0.0625;
    c0 = c0 + (j + k + l + m) * 0.125;
    return vec4<f32>(c0, 1.0);
}

// A 3 x 3 tent filter of the smaller level; the pipeline adds it to what the bigger level already holds.
@fragment
fn fs_bloom_up(in: FullOut) -> @location(0) vec4<f32> {
    let dim = vec2<f32>(textureDimensions(bloom_tex));
    let t = 1.0 / dim;
    let uv = in.uv;
    var c = textureSampleLevel(bloom_tex, post_samp, uv, 0.0).rgb * 4.0;
    c = c + textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(-1.0, 0.0), 0.0).rgb * 2.0;
    c = c + textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(1.0, 0.0), 0.0).rgb * 2.0;
    c = c + textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(0.0, -1.0), 0.0).rgb * 2.0;
    c = c + textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(0.0, 1.0), 0.0).rgb * 2.0;
    c = c + textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(-1.0, -1.0), 0.0).rgb;
    c = c + textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(1.0, -1.0), 0.0).rgb;
    c = c + textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(-1.0, 1.0), 0.0).rgb;
    c = c + textureSampleLevel(bloom_tex, post_samp, uv + t * vec2<f32>(1.0, 1.0), 0.0).rgb;
    return vec4<f32>(c / 16.0, 1.0);
}

// ------------------------------------------------------------------------------------------------ composite

// Stephen Hill's fit of the ACES reference rendering transform and output transform.
fn rrt_odt_fit(v: vec3<f32>) -> vec3<f32> {
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (0.983729 * v + 0.4329510) + 0.238081;
    return a / b;
}

fn aces(color_in: vec3<f32>) -> vec3<f32> {
    let m_in = mat3x3<f32>(vec3<f32>(0.59719, 0.07600, 0.02840), vec3<f32>(0.35458, 0.90834, 0.13383), vec3<f32>(0.04823, 0.01566, 0.83777));
    let m_out = mat3x3<f32>(vec3<f32>(1.60475, -0.10208, -0.00327), vec3<f32>(-0.53108, 1.10813, -0.07276), vec3<f32>(-0.07367, -0.00605, 1.07602));
    var c = m_in * (color_in / 0.6);
    c = rrt_odt_fit(c);
    c = m_out * c;
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

// The Khronos PBR neutral tone mapper: colours below the knee come out as authored; only highlights are compressed.
fn neutral(color_in: vec3<f32>) -> vec3<f32> {
    let start = 0.8 - 0.04;
    let desat = 0.15;
    var color = color_in;
    let x = min(color.r, min(color.g, color.b));
    var offset = 0.04;
    if (x < 0.08) {
        offset = x - 6.25 * x * x;
    }
    color = color - offset;
    let peak = max(color.r, max(color.g, color.b));
    if (peak < start) {
        return color;
    }
    let d = 1.0 - start;
    let new_peak = 1.0 - d * d / (peak + d - start);
    color = color * (new_peak / peak);
    let g = 1.0 - 1.0 / (desat * (peak - new_peak) + 1.0);
    return mix(color, vec3<f32>(new_peak), g);
}

fn tonemap(c: vec3<f32>) -> vec3<f32> {
    let mode = i32(frame.p2.z);
    if (mode == 1) {
        return aces(c);
    }
    if (mode == 2) {
        return neutral(c);
    }
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

// Colour grading and the fade, on the tone mapped colour (the identity settings change nothing).
fn grade(c_in: vec3<f32>) -> vec3<f32> {
    var c = c_in;
    // white balance: warm takes blue and adds red; the tint shift trades green against magenta
    c = c * vec3<f32>(1.0 + 0.25 * frame.p9.w, 1.0 - 0.25 * frame.p10.w, 1.0 - 0.25 * frame.p9.w);
    // gain on the highlights, lift in the shadows, gamma on the midtones
    c = c * frame.p11.xyz;
    c = c + frame.p9.xyz * (vec3<f32>(1.0) - clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)));
    c = pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0) / max(frame.p10.xyz, vec3<f32>(0.01)));
    return mix(c, frame.p12.xyz, frame.p12.w);
}

fn finish(c_in: vec3<f32>, uv: vec2<f32>, frag: vec2<f32>) -> vec4<f32> {
    var c = c_in;
    // saturation
    let l = luminance(c);
    c = mix(vec3<f32>(l), c, frame.p2.w);
    // vignette
    let q = (uv - vec2<f32>(0.5)) * vec2<f32>(frame.p1.x / frame.p1.y, 1.0);
    let v = smoothstep(0.35, 1.25, length(q) * 1.15);
    c = c * (1.0 - frame.p3.z * v);
    c = c * frame.p8.xyz;
    c = grade(c);
    var out_c = c;
    if (frame.p4.y > 0.5) {
        out_c = srgb_encode(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)));
    }
    if (frame.p7.z != 1.0) {
        out_c = clamp((out_c - vec3<f32>(0.5)) * frame.p7.z + vec3<f32>(0.5), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    if (frame.p7.y > 0.0) {
        // film grain: a different pattern every picture, a little stronger in the shadows
        let g = interleaved_noise(frag + vec2<f32>(frame.p8.w * 7.31, frame.p8.w * 3.17)) + interleaved_noise(frag * 1.37 + vec2<f32>(frame.p8.w * 1.9, 5.0)) - 1.0;
        out_c = out_c + vec3<f32>(g * frame.p7.y * 0.12 * (1.2 - 0.6 * luminance(out_c)));
    }
    // dither so that smooth gradients do not band in 8 bits
    let n = interleaved_noise(frag) + interleaved_noise(frag + vec2<f32>(17.0, 31.0)) - 1.0;
    out_c = out_c + vec3<f32>(n / 255.0);
    return vec4<f32>(out_c, 1.0);
}

// One sample of the HDR picture: what was drawn is tone mapped, the flat background (alpha 0) keeps the colour the user chose, and
// bloom glows over both.
fn resolve_sample(c: vec4<f32>, bloom: vec3<f32>) -> vec3<f32> {
    let a = clamp(c.a, 0.0, 1.0);
    let glow = bloom * frame.p0.y;
    let bg = frame.clear.rgb + tonemap(glow);
    if (a <= 0.0) {
        return bg;
    }
    let fg = tonemap(c.rgb / a * frame.p0.y + glow);
    return mix(bg, fg, a);
}

// ---- depth of field: the circle of confusion of a pixel in pixels (0 = sharp), from its distance to the camera
fn coc_at(px: vec2<i32>) -> f32 {
    let d = depth_at(px);
    if (d <= 0.0) {
        return 0.0; // the sky (reverse-Z: nothing was drawn here) stays crisp
    }
    let dist = length(world_at(px) - frame.eye.xyz);
    let off = max(abs(dist - frame.p6.y) - frame.p6.z, 0.0);
    // blur grows with the distance from the sharp zone, relative to how far away the thing is; 13 px at most on a 1080 p picture
    return frame.p6.x * 0.012 * frame.p1.y * clamp(off / max(dist, 1.0), 0.0, 1.0);
}

const GOLDEN_ANGLE: f32 = 2.39996323;

// ---- light shafts: from every pixel, look toward the sun on the screen and add up how much open sky lies on the way
fn shafts_at(uv: vec2<f32>, frag: vec2<f32>) -> vec3<f32> {
    let k = frame.p7.x;
    if (k <= 0.0 || frame.sun_dir.w < 0.5) {
        return vec3<f32>(0.0);
    }
    let clip = frame.view_proj * vec4<f32>(frame.eye.xyz + frame.sun_dir.xyz * 1000.0, 1.0);
    if (clip.w <= 0.0) {
        return vec3<f32>(0.0);
    }
    let ndc = clip.xy / clip.w;
    let sun_uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let size = vec2<i32>(frame.p1.xy);
    let jitter = interleaved_noise(frag);
    var acc = 0.0;
    var weight = 1.0;
    for (var i = 0; i < 28; i = i + 1) {
        let t = (f32(i) + jitter) / 28.0;
        let p = mix(uv, sun_uv, t * 0.97);
        let q = vec2<i32>(p * vec2<f32>(size));
        if (q.x >= 0 && q.y >= 0 && q.x < size.x && q.y < size.y) {
            if (textureLoad(depth_tex, q, 0) <= 0.0) {
                acc = acc + weight;
            }
        }
        weight = weight * 0.94;
    }
    // the rays fade as the sun leaves the picture
    let edge = clamp(1.6 - length((sun_uv - vec2<f32>(0.5)) * 2.0), 0.0, 1.0);
    return frame.sun_color.rgb * (acc / 28.0) * k * 0.25 * edge;
}

// ---- motion blur: where this pixel was in the previous picture, in pixels
fn velocity_px(px: vec2<i32>) -> vec2<f32> {
    let uv = (vec2<f32>(px) + 0.5) * frame.p1.zw;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let w = frame.inv_view_proj * vec4<f32>(ndc, depth_at(px), 1.0);
    let prev = frame.prev_view_proj * vec4<f32>(w.xyz / w.w, 1.0);
    if (prev.w <= 0.0) {
        return vec2<f32>(0.0);
    }
    let pn = prev.xy / prev.w;
    let puv = vec2<f32>(pn.x * 0.5 + 0.5, 0.5 - pn.y * 0.5);
    var v = (uv - puv) * frame.p1.xy * frame.p7.w;
    let len = length(v);
    if (len > 40.0) {
        v = v * (40.0 / len);
    }
    return v;
}

@fragment
fn fs_composite(in: FullOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    let bloom = textureSampleLevel(bloom_tex, post_samp, in.uv, 0.0).rgb * frame.p3.x + shafts_at(in.uv, in.pos.xy);
    var c = textureLoad(hdr_tex, px, 0);
    if (frame.p7.w > 0.0) {
        let v = velocity_px(px);
        if (length(v) > 0.75) {
            var acc = c;
            for (var k = 1; k < 8; k = k + 1) {
                let t = f32(k) / 7.0 - 0.5;
                let q = clamp(px + vec2<i32>(round(v * t)), vec2<i32>(0), vec2<i32>(frame.p1.xy) - vec2<i32>(1));
                acc = acc + textureLoad(hdr_tex, q, 0);
            }
            c = acc / 8.0;
        }
    }
    if (frame.p6.x > 0.0) {
        let r = coc_at(px);
        if (r > 0.5) {
            var acc = c;
            for (var k = 0; k < 15; k = k + 1) {
                let a = f32(k) * GOLDEN_ANGLE;
                let rad = sqrt((f32(k) + 0.5) / 15.0) * r;
                let q = clamp(px + vec2<i32>(i32(round(cos(a) * rad)), i32(round(sin(a) * rad))), vec2<i32>(0), vec2<i32>(frame.p1.xy) - vec2<i32>(1));
                acc = acc + textureLoad(hdr_tex, q, 0);
            }
            c = acc / 16.0;
        }
    }
    return finish(resolve_sample(c, bloom), in.uv, in.pos.xy);
}

// With multisampling: tone map every sample before averaging, so that bright edges stay anti-aliased.
@fragment
fn fs_composite_ms(in: FullOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    let bloom = textureSampleLevel(bloom_tex, post_samp, in.uv, 0.0).rgb * frame.p3.x + shafts_at(in.uv, in.pos.xy);
    let n = i32(frame.p4.z);
    if (frame.p7.w > 0.0) {
        let v = velocity_px(px);
        if (length(v) > 0.75) {
            // moving: one sample of each of eight taps along the movement (the blur is its own anti-aliasing)
            var acc = resolve_sample(textureLoad(hdr_ms, px, 0), bloom);
            for (var k = 1; k < 8; k = k + 1) {
                let t = f32(k) / 7.0 - 0.5;
                let q = clamp(px + vec2<i32>(round(v * t)), vec2<i32>(0), vec2<i32>(frame.p1.xy) - vec2<i32>(1));
                acc = acc + resolve_sample(textureLoad(hdr_ms, q, k % n), bloom);
            }
            return finish(acc / 8.0, in.uv, in.pos.xy);
        }
    }
    if (frame.p6.x > 0.0) {
        let r = coc_at(px);
        if (r > 0.5) {
            // out of focus: gather a ring of taps (one sample each, so the blur itself is the anti-aliasing), tone map each
            var acc = resolve_sample(textureLoad(hdr_ms, px, 0), bloom);
            for (var k = 0; k < 15; k = k + 1) {
                let a = f32(k) * GOLDEN_ANGLE;
                let rad = sqrt((f32(k) + 0.5) / 15.0) * r;
                let q = clamp(px + vec2<i32>(i32(round(cos(a) * rad)), i32(round(sin(a) * rad))), vec2<i32>(0), vec2<i32>(frame.p1.xy) - vec2<i32>(1));
                acc = acc + resolve_sample(textureLoad(hdr_ms, q, k % n), bloom);
            }
            return finish(acc / 16.0, in.uv, in.pos.xy);
        }
    }
    var acc = vec3<f32>(0.0);
    for (var i = 0; i < n; i = i + 1) {
        acc = acc + resolve_sample(textureLoad(hdr_ms, px, i), bloom);
    }
    return finish(acc / f32(n), in.uv, in.pos.xy);
}
