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
// the reflections the screen-space pass found (composite), and what that pass reads besides the picture and the opaque depth: the depth of the nearest surface of every pixel
// (see-through ones included), where it points and how rough it is (xyz, w), how much it reflects head on and whether there is a surface (rgb, w)
@group(0) @binding(7) var ssr_tex: texture_2d<f32>;
@group(0) @binding(9) var surf_depth: texture_depth_2d;
@group(0) @binding(10) var gbuf_a: texture_2d<f32>;
@group(0) @binding(11) var gbuf_b: texture_2d<f32>;
// the faces of the reflection probes (see sky.wgsl) and how to sample them
@group(0) @binding(12) var probe_tex: texture_2d_array<f32>;
@group(0) @binding(13) var probe_samp: sampler;
// the state of the eye adaptation (see exposure.wgsl): x is the exposure it has settled on
@group(0) @binding(8) var<storage, read> ae_state: array<vec4<f32>>;

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

// ------------------------------------------------------------------------------------------------ screen-space reflections

fn view_depth(p: vec3<f32>) -> f32 {
    return -(frame.view * vec4<f32>(p, 1.0)).z;
}

// How far in front of the camera (along its view) the nearest opaque thing at a pixel is; a huge number where nothing was drawn (the sky).
fn scene_view_depth(px: vec2<i32>) -> f32 {
    let d = depth_at(px);
    if (d <= 0.0) {
        return 1.0e9;
    }
    return view_depth(world_at(px));
}

// The way along a ray in the picture: the part (0 to 1) of the ray's length reached when a fraction `u` of the ray's length on the screen is done. The screen is not linear in the
// world (perspective), 1 / w is: that is what makes the steps of the march the same size on the screen.
fn ray_part(u: f32, w0: f32, w1: f32) -> f32 {
    if (abs(w1 - w0) > 1.0e-4) {
        return (1.0 / mix(1.0 / w0, 1.0 / w1, u) - w0) / (w1 - w0);
    }
    return u;
}

// The light a glossy pixel gets from what the picture shows in its mirror direction, minus the sky's reflection the mesh shader already put there: the composite adds it. A ray is
// marched through the depth of the picture in steps of a couple of pixels (at most 96), a hit is a ray that passes from in front of the surface to behind it within `thickness`
// (a surface seen from behind, one that faces away from the ray, is no hit), and the place is found to a fraction of a step by halving.
@fragment
fn fs_ssr(in: FullOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    let gb = textureLoad(gbuf_b, px, 0);
    if (gb.a < 0.5) {
        return vec4<f32>(0.0);
    }
    let ga = textureLoad(gbuf_a, px, 0);
    let rough = ga.w;
    let strength = frame.p16.x * (1.0 - smoothstep(0.3, 0.7, rough));
    if (strength <= 0.0) {
        return vec4<f32>(0.0);
    }
    let d0 = textureLoad(surf_depth, px, 0);
    if (d0 <= 0.0) {
        return vec4<f32>(0.0);
    }
    let size = frame.p1.xy;
    let uv0 = (vec2<f32>(px) + 0.5) * frame.p1.zw;
    let ndc0 = vec2<f32>(uv0.x * 2.0 - 1.0, 1.0 - uv0.y * 2.0);
    let wp = frame.inv_view_proj * vec4<f32>(ndc0, d0, 1.0);
    let p0 = wp.xyz / wp.w;
    let n = normalize(ga.xyz);
    var v = normalize(p0 - frame.eye.xyz);
    if (frame.p4.w > 0.5) {
        // an orthographic camera: every ray of sight runs along the camera's own forward
        v = -normalize(vec3<f32>(frame.view[0][2], frame.view[1][2], frame.view[2][2]));
    }
    let r = reflect(v, n);
    // the ray: from the surface, at most `max_distance` long, and not past the camera's near plane
    let z0 = view_depth(p0);
    let rz = -(frame.view * vec4<f32>(r, 0.0)).z;
    var len = max(frame.p16.y, 0.1);
    if (rz < 0.0) {
        len = min(len, max(z0 - frame.eye.w - 0.02, 0.0) / -rz);
    }
    if (len < 0.05) {
        return vec4<f32>(0.0);
    }
    let h0 = frame.view_proj * vec4<f32>(p0, 1.0);
    let h1 = frame.view_proj * vec4<f32>(p0 + r * len, 1.0);
    if (h0.w <= 0.0 || h1.w <= 0.0) {
        return vec4<f32>(0.0);
    }
    let s0 = vec2<f32>(h0.x / h0.w * 0.5 + 0.5, 0.5 - h0.y / h0.w * 0.5) * size;
    let s1 = vec2<f32>(h1.x / h1.w * 0.5 + 0.5, 0.5 - h1.y / h1.w * 0.5) * size;
    let dir = s1 - s0;
    // only the part of the ray that stays on the screen
    var u_end = 1.0;
    if (dir.x > 0.0) { u_end = min(u_end, (size.x - 1.0 - s0.x) / dir.x); }
    if (dir.x < 0.0) { u_end = min(u_end, -s0.x / dir.x); }
    if (dir.y > 0.0) { u_end = min(u_end, (size.y - 1.0 - s0.y) / dir.y); }
    if (dir.y < 0.0) { u_end = min(u_end, -s0.y / dir.y); }
    let span = length(dir) * u_end;
    if (span < 2.0) {
        return vec4<f32>(0.0);
    }
    let steps = clamp(i32(span * 0.5), 12, 96);
    let jitter = interleaved_noise(in.pos.xy);
    let bias = 0.03 + 0.01 * z0;
    let thick = max(frame.p16.z, 0.01);
    let rzl = rz * len;
    var hit = false;
    var lo = 0.0;
    var hi = 0.0;
    var prev_u = 0.0;
    var prev_diff = -1.0;
    var prev_z = z0;
    for (var i = 0; i < steps; i = i + 1) {
        let u = (f32(i) + 0.25 + 0.75 * jitter) / f32(steps) * u_end;
        let zr = z0 + rzl * ray_part(u, h0.w, h1.w);
        let diff = zr - scene_view_depth(vec2<i32>(s0 + dir * u));
        // from in front of the surface to behind it, and not far behind it (a thin thing the ray passes behind is no mirror)
        if (diff > bias && prev_diff <= bias && diff < thick + (zr - prev_z) + 0.02 * zr) {
            hit = true;
            lo = prev_u;
            hi = u;
            break;
        }
        prev_u = u;
        prev_diff = diff;
        prev_z = zr;
    }
    if (!hit) {
        return vec4<f32>(0.0);
    }
    for (var k = 0; k < 6; k = k + 1) {
        let mid = 0.5 * (lo + hi);
        let diff = z0 + rzl * ray_part(mid, h0.w, h1.w) - scene_view_depth(vec2<i32>(s0 + dir * mid));
        if (diff > bias) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let hit_px = s0 + dir * hi;
    let hq = vec2<i32>(hit_px);
    let huv = hit_px / size;
    let hit_z = z0 + rzl * ray_part(hi, h0.w, h1.w);
    if (hit_z - scene_view_depth(hq) > thick + 0.02 * hit_z) {
        return vec4<f32>(0.0);
    }
    let seen = textureSampleLevel(hdr_tex, post_samp, huv, 0.0);
    if (seen.a < 0.5) {
        return vec4<f32>(0.0);
    }
    // a surface that faces away from the ray is seen from behind: no reflection
    let nh = textureLoad(gbuf_a, hq, 0).xyz;
    var facing = 1.0;
    if (dot(nh, nh) > 0.25) {
        facing = smoothstep(0.35, -0.1, dot(normalize(nh), r));
    }
    let edge = smoothstep(0.0, 0.08, min(min(huv.x, 1.0 - huv.x), min(huv.y, 1.0 - huv.y)));
    let reach = 1.0 - smoothstep(0.65, 1.0, len * ray_part(hi, h0.w, h1.w) / max(frame.p16.y, 0.1));
    let conf = strength * edge * reach * facing;
    // swap the sky's reflection for what was seen, as the mesh shader weighs it (the environment BRDF, fog)
    let rr = clamp(rough, 0.045, 1.0);
    let nv = max(dot(n, -v), 1.0e-4);
    let brdf = env_brdf(gb.rgb, rr, nv);
    var env = sky_color(r, rr < 0.3);
    env = mix(env, sky_irradiance(r), smoothstep(0.15, 0.95, rr));
    var old = env * frame.p0.x;
    if (frame.p18.x > 0.5) {
        // inside a probe's box the reflection that was there is the probe's
        let ph = probe_light(p0, n, r, rr);
        old = mix(old, ph.env, ph.w);
    }
    let add = (min(seen.rgb / seen.a, vec3<f32>(200.0)) - old) * brdf * conf * (1.0 - fog_amount(p0));
    return vec4<f32>(add, 1.0);
}

// A face of a reflection probe, finished: the picture drawn into `hdr_tex` as plain light (the flat background colour shows behind what is not drawn).
@fragment
fn fs_probe_resolve(in: FullOut) -> @location(0) vec4<f32> {
    let c = textureLoad(hdr_tex, vec2<i32>(in.pos.xy), 0);
    let a = clamp(c.a, 0.0, 1.0);
    return vec4<f32>(min(c.rgb + frame.clear.rgb * (1.0 - a), vec3<f32>(60000.0)), 1.0);
}

// The next, smaller mip level of a face: the average of four texels of the one above (a bilinear lookup in the middle of them).
@fragment
fn fs_probe_mip(in: FullOut) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSampleLevel(bloom_tex, post_samp, in.uv, 0.0).rgb, 1.0);
}

// The reflection layer at a pixel (nothing when the effect is off).
fn ssr_at(q: vec2<i32>) -> vec3<f32> {
    if (frame.p16.w < 0.5) {
        return vec3<f32>(0.0);
    }
    return textureLoad(ssr_tex, clamp(q, vec2<i32>(0), vec2<i32>(frame.p1.xy) - vec2<i32>(1)), 0).rgb;
}

// ------------------------------------------------------------------------------------------------ lens flare

// Where the sun is on the screen (xy: in uv units, z: 1 when it is in front of the camera) and the radius of its disc there in pixels (w).
fn sun_on_screen() -> vec4<f32> {
    let dir = normalize(frame.sun_dir.xyz);
    let clip = frame.view_proj * vec4<f32>(frame.eye.xyz + dir * 1000.0, 1.0);
    if (clip.w <= 0.0) {
        return vec4<f32>(0.0);
    }
    let ndc = clip.xy / clip.w;
    // the sun's disc in the sky shader is about 2.1 degrees across: where its upper edge lands says how big it is on the screen
    let up = vec3<f32>(frame.view[0][1], frame.view[1][1], frame.view[2][1]);
    let edge = frame.view_proj * vec4<f32>(frame.eye.xyz + normalize(dir + up * 0.0375) * 1000.0, 1.0);
    let radius = length((edge.xy / max(edge.w, 1.0e-4) - ndc) * 0.5 * frame.p1.xy);
    return vec4<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, 1.0, radius);
}

// The share of the sun's disc that is not hidden: depth taps on the disc (nothing drawn there is sky; the part outside the picture counts as open).
fn sun_open(sun: vec4<f32>) -> f32 {
    let size = frame.p1.xy;
    let c = sun.xy * size;
    let rad = max(sun.w, 3.0);
    var open = 0.0;
    for (var i = 0; i < 16; i = i + 1) {
        let a = f32(i) * GOLDEN_ANGLE;
        let p = c + vec2<f32>(cos(a), sin(a)) * (sqrt((f32(i) + 0.5) / 16.0) * rad);
        if (p.x < 0.0 || p.y < 0.0 || p.x >= size.x || p.y >= size.y) {
            open = open + 1.0;
        } else if (textureLoad(depth_tex, vec2<i32>(p), 0) <= 0.0) {
            open = open + 1.0;
        }
    }
    return open / 16.0;
}

// The eight ghosts: where each lies on the line through the middle of the picture (1 is the sun, 0 the middle, -1 the sun's mirror image), its size (a share of the picture's height),
// its colour and how bright it is.
fn ghost_along(i: i32) -> f32 {
    if (i == 0) { return 0.5; }
    if (i == 1) { return -0.35; }
    if (i == 2) { return -0.8; }
    if (i == 3) { return 0.25; }
    if (i == 4) { return -1.3; }
    if (i == 5) { return 0.75; }
    if (i == 6) { return -1.8; }
    return -0.1;
}

fn ghost_radius(i: i32) -> f32 {
    if (i == 0) { return 0.05; }
    if (i == 1) { return 0.09; }
    if (i == 2) { return 0.035; }
    if (i == 3) { return 0.12; }
    if (i == 4) { return 0.06; }
    if (i == 5) { return 0.045; }
    if (i == 6) { return 0.15; }
    return 0.03;
}

fn ghost_tint(i: i32) -> vec3<f32> {
    if (i == 0) { return vec3<f32>(1.0, 0.7, 0.35); }
    if (i == 1) { return vec3<f32>(0.35, 0.65, 1.0); }
    if (i == 2) { return vec3<f32>(0.45, 1.0, 0.55); }
    if (i == 3) { return vec3<f32>(1.0, 0.45, 0.4); }
    if (i == 4) { return vec3<f32>(0.65, 0.45, 1.0); }
    if (i == 5) { return vec3<f32>(1.0, 0.9, 0.55); }
    if (i == 6) { return vec3<f32>(0.35, 0.9, 0.95); }
    return vec3<f32>(1.0, 0.65, 0.85);
}

fn ghost_weight(i: i32) -> f32 {
    if (i == 0) { return 0.6; }
    if (i == 1) { return 0.5; }
    if (i == 2) { return 0.4; }
    if (i == 3) { return 0.4; }
    if (i == 4) { return 0.3; }
    if (i == 5) { return 0.3; }
    if (i == 6) { return 0.2; }
    return 0.35;
}

// The light the lens adds at a pixel (uv): a glare around the sun, soft coloured discs along the line from the sun through the middle, a ring around the middle. Nothing without a sun
// in front of the camera; it fades out as the sun leaves the picture and as what is drawn covers the sun (the depth is read only where the flare is not zero).
fn flare_at(uv: vec2<f32>) -> vec3<f32> {
    if (frame.p4.w > 0.5 || frame.sun_dir.w < 0.5) {
        return vec3<f32>(0.0);
    }
    let sun = sun_on_screen();
    if (sun.z < 0.5) {
        return vec3<f32>(0.0);
    }
    let off = max(abs(sun.x - 0.5), abs(sun.y - 0.5)) * 2.0;
    let fade = 1.0 - smoothstep(0.95, 1.4, off);
    if (fade <= 0.0) {
        return vec3<f32>(0.0);
    }
    // the picture with its middle at 0 and its height 1 (so that the discs are round)
    let k = vec2<f32>(frame.p1.x / frame.p1.y, 1.0);
    let p = (uv - vec2<f32>(0.5)) * k;
    let s = (sun.xy - vec2<f32>(0.5)) * k;
    var col = vec3<f32>(0.0);
    let d = length(p - s);
    col = col + vec3<f32>(1.0, 0.92, 0.8) * (0.55 * exp(-d * d / 0.004) + 0.16 * exp(-d / 0.12));
    let n = i32(frame.p17.y);
    for (var i = 0; i < 8; i = i + 1) {
        if (i >= n) {
            break;
        }
        let gd = length(p - s * ghost_along(i)) / ghost_radius(i);
        // a soft disc, brighter at its rim
        let body = (1.0 - smoothstep(0.85, 1.0, gd)) * (0.25 + 0.75 * smoothstep(0.3, 0.92, gd));
        col = col + ghost_tint(i) * (body * ghost_weight(i));
    }
    // the halo: a ring around the middle of the picture, brightest on the side away from the sun
    let ring_t = (length(p) - 0.38) / 0.025;
    var away = 1.0;
    if (length(s) > 1.0e-3 && length(p) > 1.0e-3) {
        away = max(dot(normalize(p), -normalize(s)), 0.0);
    }
    col = col + vec3<f32>(0.8, 0.85, 1.0) * (exp(-ring_t * ring_t) * (0.5 + 1.0 * away * away) * frame.p17.z);
    if (max(col.r, max(col.g, col.b)) < 0.002) {
        return vec3<f32>(0.0);
    }
    let sc = frame.sun_color.rgb;
    let top = max(sc.r, max(sc.g, sc.b));
    return col * (sc / max(top, 1.0e-3)) * (frame.p17.x * 0.2 * clamp(top, 0.25, 6.0) * fade * sun_open(sun));
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

// The exposure the eye adaptation has settled on, a multiplier of the light of the scene (1 when there is none). Without the effect the passes below keep their old arithmetic
// in a branch of their own (the graphics driver compiles `x * (e * 1.0)` differently from `x * e`, and a picture must not change when none of the new effects is in the world).
fn ae_factor() -> f32 {
    if (frame.p15.x > 0.5) {
        return ae_state[0].x;
    }
    return 1.0;
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
    // what glows is judged after the eye has adapted (a lamp in a dark room glows, the same lamp in the sun does not)
    var lit = c0;
    if (frame.p15.x > 0.5) {
        lit = c0 * ae_factor();
    }
    return vec4<f32>(prefilter(lit), 1.0);
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
fn resolve_sample(c: vec4<f32>, bloom: vec3<f32>, ae: f32, ssr: vec3<f32>) -> vec3<f32> {
    let a = clamp(c.a, 0.0, 1.0);
    let glow = bloom * frame.p0.y;
    let bg = frame.clear.rgb + tonemap(glow);
    if (a <= 0.0) {
        return bg;
    }
    var fg: vec3<f32>;
    if (frame.p16.w > 0.5) {
        // the reflections found on the screen are scene light too: they are exposed with it
        fg = tonemap((c.rgb / a + ssr) * (frame.p0.y * ae) + glow);
    } else if (frame.p15.x > 0.5) {
        fg = tonemap(c.rgb / a * (frame.p0.y * ae) + glow);
    } else {
        fg = tonemap(c.rgb / a * frame.p0.y + glow);
    }
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


// ---- volumetric light (`Volumetrics`): from the eye to the first thing seen, the glow of every point and spot light in the air on the way
fn hg_phase(cos_t: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (12.566371 * pow(max(1.0 + g2 - 2.0 * g * cos_t, 0.001), 1.5));
}

fn volumetric_at(px: vec2<i32>, pos: vec2<f32>) -> vec3<f32> {
    let eye = frame.eye.xyz;
    let size = vec2<i32>(frame.p1.xy);
    let q = clamp(px, vec2<i32>(0), size - vec2<i32>(1));
    let d = textureLoad(depth_tex, q, 0);
    let uv = (vec2<f32>(q) + 0.5) * frame.p1.zw;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let w4 = frame.inv_view_proj * vec4<f32>(ndc, max(d, 0.0001), 1.0);
    let to_pt = w4.xyz / w4.w - eye;
    let dir = normalize(to_pt);
    let dist = min(length(to_pt), frame.p21.w);
    let n = max(i32(frame.p21.z), 4);
    let dt = dist / f32(n);
    let jitter = interleaved_noise(pos + vec2<f32>(frame.p8.w * 3.7, 1.3));
    let density = frame.p21.x;
    let g = frame.p21.y;
    let count = u32(frame.p2.y);
    var acc = vec3<f32>(0.0);
    var tr = 1.0;
    for (var i = 0; i < n; i = i + 1) {
        let t = (f32(i) + jitter) * dt;
        let p = eye + dir * t;
        var lum = vec3<f32>(0.0);
        for (var li = 0u; li < count; li = li + 1u) {
            let l = frame.lights[li];
            if (l.vec.w < 0.5) {
                continue;
            }
            let tl = l.vec.xyz - p;
            let dl = length(tl);
            if (dl > l.color.w) {
                continue;
            }
            let k = dl / l.color.w;
            let a = 1.0 - k * k;
            var att = a * a;
            let ld = tl / max(dl, 0.001);
            if (l.spot.w > -1.5) {
                att = att * smoothstep(l.spot.w, l.cone.x, dot(-ld, l.spot.xyz));
            }
            if (att > 0.0) {
                lum = lum + l.color.rgb * (att * hg_phase(dot(ld, dir), g));
            }
        }
        acc = acc + lum * (density * tr * dt);
        tr = tr * exp(-density * dt);
    }
    return acc * frame.p22.rgb * frame.p22.w;
}

fn composite_core(pos: vec2<f32>) -> vec3<f32> {
    let uv = pos / frame.p1.xy;
    let px = vec2<i32>(pos);
    let ae = ae_factor();
    var rays = shafts_at(uv, pos);
    if (frame.p21.x > 0.0) {
        rays = rays + volumetric_at(px, pos);
    }
    if (frame.p17.w > 0.5) {
        rays = rays + flare_at(uv);
    }
    var bloom = textureSampleLevel(bloom_tex, post_samp, uv, 0.0).rgb * frame.p3.x + rays;
    if (frame.p15.x > 0.5) {
        bloom = textureSampleLevel(bloom_tex, post_samp, uv, 0.0).rgb * frame.p3.x + rays * ae;
    }
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
    return resolve_sample(c, bloom, ae, ssr_at(px));
}


// With multisampling: tone map every sample before averaging, so that bright edges stay anti-aliased.
fn composite_core_ms(pos: vec2<f32>) -> vec3<f32> {
    let uv = pos / frame.p1.xy;
    let px = vec2<i32>(pos);
    let ae = ae_factor();
    var rays = shafts_at(uv, pos);
    if (frame.p21.x > 0.0) {
        rays = rays + volumetric_at(px, pos);
    }
    if (frame.p17.w > 0.5) {
        rays = rays + flare_at(uv);
    }
    var bloom = textureSampleLevel(bloom_tex, post_samp, uv, 0.0).rgb * frame.p3.x + rays;
    if (frame.p15.x > 0.5) {
        bloom = textureSampleLevel(bloom_tex, post_samp, uv, 0.0).rgb * frame.p3.x + rays * ae;
    }
    let n = i32(frame.p4.z);
    if (frame.p7.w > 0.0) {
        let v = velocity_px(px);
        if (length(v) > 0.75) {
            // moving: one sample of each of eight taps along the movement (the blur is its own anti-aliasing)
            var acc = resolve_sample(textureLoad(hdr_ms, px, 0), bloom, ae, ssr_at(px));
            for (var k = 1; k < 8; k = k + 1) {
                let t = f32(k) / 7.0 - 0.5;
                let q = clamp(px + vec2<i32>(round(v * t)), vec2<i32>(0), vec2<i32>(frame.p1.xy) - vec2<i32>(1));
                acc = acc + resolve_sample(textureLoad(hdr_ms, q, k % n), bloom, ae, ssr_at(q));
            }
            return acc / 8.0;
        }
    }
    if (frame.p6.x > 0.0) {
        let r = coc_at(px);
        if (r > 0.5) {
            // out of focus: gather a ring of taps (one sample each, so the blur itself is the anti-aliasing), tone map each
            var acc = resolve_sample(textureLoad(hdr_ms, px, 0), bloom, ae, ssr_at(px));
            for (var k = 0; k < 15; k = k + 1) {
                let a = f32(k) * GOLDEN_ANGLE;
                let rad = sqrt((f32(k) + 0.5) / 15.0) * r;
                let q = clamp(px + vec2<i32>(i32(round(cos(a) * rad)), i32(round(sin(a) * rad))), vec2<i32>(0), vec2<i32>(frame.p1.xy) - vec2<i32>(1));
                acc = acc + resolve_sample(textureLoad(hdr_ms, q, k % n), bloom, ae, ssr_at(q));
            }
            return acc / 16.0;
        }
    }
    var acc = vec3<f32>(0.0);
    for (var i = 0; i < n; i = i + 1) {
        acc = acc + resolve_sample(textureLoad(hdr_ms, px, i), bloom, ae, ssr_at(px));
    }
    return acc / f32(n);
}


// ---- the lens (`Lens`): where the picture a pixel shows comes from, the colour fringes, the soft sensor, the blocks, the scan lines
fn lens_on() -> bool {
    return frame.p19.x != 0.0 || frame.p19.y != 1.0 || frame.p19.z > 0.0 || frame.p19.w > 0.0 || frame.p20.x > 1.5 || frame.p20.y > 0.0;
}

// output pixel -> the pixel of the picture it shows (`extra` bends the red and the blue a little differently from the green)
fn lens_src(pos: vec2<f32>, extra: f32) -> vec2<f32> {
    let dims = frame.p1.xy;
    let asp = dims.x / dims.y;
    var u = (pos / dims - vec2<f32>(0.5)) * 2.0;
    u.x = u.x * asp;
    u = u * frame.p19.y;
    let s = 1.0 + (frame.p19.x + extra) * dot(u, u);
    var v = u * s;
    v.x = v.x / asp;
    return (v * 0.5 + vec2<f32>(0.5)) * dims;
}

fn lens_inside(q: vec2<f32>) -> bool {
    return q.x >= 0.0 && q.y >= 0.0 && q.x < frame.p1.x && q.y < frame.p1.y;
}

// One cheap look at the picture at a pixel (no light shafts, no blurs): the red and blue of a colour fringe and the taps of a soft sensor come from it, so that the heavy composite above is written once per entry point.
fn quick_plain(q: vec2<f32>) -> vec3<f32> {
    let px = clamp(vec2<i32>(q), vec2<i32>(0), vec2<i32>(frame.p1.xy) - vec2<i32>(1));
    let bloom = textureSampleLevel(bloom_tex, post_samp, (vec2<f32>(px) + 0.5) * frame.p1.zw, 0.0).rgb * frame.p3.x;
    return resolve_sample(textureLoad(hdr_tex, px, 0), bloom, ae_factor(), ssr_at(px));
}

fn quick_ms(q: vec2<f32>) -> vec3<f32> {
    let px = clamp(vec2<i32>(q), vec2<i32>(0), vec2<i32>(frame.p1.xy) - vec2<i32>(1));
    let bloom = textureSampleLevel(bloom_tex, post_samp, (vec2<f32>(px) + 0.5) * frame.p1.zw, 0.0).rgb * frame.p3.x;
    return resolve_sample(textureLoad(hdr_ms, px, 0), bloom, ae_factor(), ssr_at(px));
}

// What the lens does to the picture at an output pixel: where it looks (barrel, blocks), the fringe, the soft sensor, the scan lines. The picture itself is worked out once, at the pixel the lens looks at.
fn lens_plain(pos_in: vec2<f32>) -> vec3<f32> {
    let on = lens_on();
    var p = pos_in;
    if (on && frame.p20.x > 1.5) {
        p = (floor(p / frame.p20.x) + vec2<f32>(0.5)) * frame.p20.x;
    }
    var qg = p;
    if (on) {
        qg = lens_src(p, 0.0);
        if (!lens_inside(qg)) {
            return vec3<f32>(0.0);
        }
    }
    var col = composite_core(qg);
    if (on) {
        let c = frame.p19.z;
        if (c > 0.0) {
            // the fringe is how much the red and the blue differ between where they are looked up and where green is: that keeps what the full picture holds (glows, light shafts)
            let here = quick_plain(qg);
            col.r = col.r + (quick_plain(lens_src(p, c * 6.0)).r - here.r);
            col.b = col.b + (quick_plain(lens_src(p, 0.0 - c * 6.0)).b - here.b);
        }
        if (frame.p19.w > 0.0) {
            let o = frame.p19.w;
            let around = (quick_plain(qg + vec2<f32>(o, 0.0)) + quick_plain(qg - vec2<f32>(o, 0.0)) + quick_plain(qg + vec2<f32>(0.0, o)) + quick_plain(qg - vec2<f32>(0.0, o))) * 0.25;
            col = mix(col, around, 0.6);
        }
        if (frame.p20.y > 0.0) {
            col = col * (1.0 - frame.p20.y * 0.4 * (0.5 + 0.5 * cos(pos_in.y * 3.14159265)));
        }
    }
    return col;
}

fn lens_ms(pos_in: vec2<f32>) -> vec3<f32> {
    let on = lens_on();
    var p = pos_in;
    if (on && frame.p20.x > 1.5) {
        p = (floor(p / frame.p20.x) + vec2<f32>(0.5)) * frame.p20.x;
    }
    var qg = p;
    if (on) {
        qg = lens_src(p, 0.0);
        if (!lens_inside(qg)) {
            return vec3<f32>(0.0);
        }
    }
    var col = composite_core_ms(qg);
    if (on) {
        let c = frame.p19.z;
        if (c > 0.0) {
            // the fringe is how much the red and the blue differ between where they are looked up and where green is: that keeps what the full picture holds (glows, light shafts)
            let here = quick_ms(qg);
            col.r = col.r + (quick_ms(lens_src(p, c * 6.0)).r - here.r);
            col.b = col.b + (quick_ms(lens_src(p, 0.0 - c * 6.0)).b - here.b);
        }
        if (frame.p19.w > 0.0) {
            let o = frame.p19.w;
            let around = (quick_ms(qg + vec2<f32>(o, 0.0)) + quick_ms(qg - vec2<f32>(o, 0.0)) + quick_ms(qg + vec2<f32>(0.0, o)) + quick_ms(qg - vec2<f32>(0.0, o))) * 0.25;
            col = mix(col, around, 0.6);
        }
        if (frame.p20.y > 0.0) {
            col = col * (1.0 - frame.p20.y * 0.4 * (0.5 + 0.5 * cos(pos_in.y * 3.14159265)));
        }
    }
    return col;
}

@fragment
fn fs_composite(in: FullOut) -> @location(0) vec4<f32> {
    return finish(lens_plain(in.pos.xy), in.uv, in.pos.xy);
}

@fragment
fn fs_composite_ms(in: FullOut) -> @location(0) vec4<f32> {
    return finish(lens_ms(in.pos.xy), in.uv, in.pos.xy);
}
