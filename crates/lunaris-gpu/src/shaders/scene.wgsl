// Lit meshes, sprites and the sky. One shader serves both quality modes: in the classic mode (frame.p2.x == 0) a mesh gets exactly the
// reference renderer's Blinn-Phong lighting in a gamma-2.0 working space; in the high-quality mode (== 1) it gets physically based
// shading with cascaded shadows, ambient occlusion and a procedural sky as the environment.

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<storage, read> objects: array<Object>;
@group(0) @binding(2) var shadow_map: texture_depth_2d_array;
@group(0) @binding(3) var shadow_samp: sampler_comparison;
@group(0) @binding(4) var ao_tex: texture_2d<f32>;
@group(0) @binding(5) var lin_samp: sampler;
@group(0) @binding(6) var point_map: texture_depth_2d_array;
// the faces of the reflection probes (see sky.wgsl) and how to sample them
@group(0) @binding(7) var probe_tex: texture_2d_array<f32>;
@group(0) @binding(8) var probe_samp: sampler;
// the depth of the opaque things of this picture (the depth prepass), for water to see how deep it is
@group(0) @binding(9) var opaque_depth: texture_depth_2d;

@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var tex_samp: sampler;
@group(1) @binding(2) var height_tex: texture_2d<f32>;
// `PbrMaps`: a tangent-space normal map (DirectX convention) and occlusion / roughness / metallic in r / g / b; plain numbers, not colours
@group(1) @binding(3) var normal_tex: texture_2d<f32>;
@group(1) @binding(4) var arm_tex: texture_2d<f32>;
// a height map (white = high) for parallax occlusion mapping
@group(1) @binding(5) var disp_tex: texture_2d<f32>;

// What the maps of the mesh being shaded say, handed from `lit_mesh` to `lit_tail` (a negative roughness means "no map": the object's own numbers count).
var<private> g_ao: f32 = 1.0;
// How much the surface normal changes across this pixel (set by `lit_mesh`, which has the derivatives): `lit_tail` widens the highlight by it, so that a shiny surface with fine bumps or ripples shimmers less (specular anti-aliasing).
var<private> g_nvar: f32 = 0.0;
var<private> g_rough: f32 = -1.0;
var<private> g_metal: f32 = 0.0;

const AMBIENT: f32 = 0.16;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) col: vec3<f32>,
    @location(4) tw: f32,
    @builtin(instance_index) inst: u32,
};

struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) col: vec3<f32>,
    @location(4) tw: f32,
    @location(5) @interpolate(flat) inst: u32,
};

// ---- water: the height of the surface at a point of the plane, a few sines of different directions and lengths that travel with `phase`
fn wave_height(p: vec2<f32>, height: f32, length: f32, phase: f32) -> f32 {
    let k = 6.2831853 / max(length, 0.2);
    var s = sin(dot(p, vec2<f32>(0.8, 0.6)) * k + phase);
    s = s + 0.6 * sin(dot(p, vec2<f32>(-0.5, 0.85)) * k * 1.7 + phase * 1.3 + 1.7);
    s = s + 0.35 * sin(dot(p, vec2<f32>(0.95, -0.3)) * k * 2.9 + phase * 1.9 + 4.1);
    s = s + 0.2 * sin(dot(p, vec2<f32>(-0.2, -0.98)) * k * 4.3 + phase * 2.4 + 2.2);
    return s * height * 0.4;
}

fn wave_normal(p: vec2<f32>, height: f32, length: f32, phase: f32) -> vec3<f32> {
    let e = length * 0.03;
    let hx = wave_height(p + vec2<f32>(e, 0.0), height, length, phase) - wave_height(p - vec2<f32>(e, 0.0), height, length, phase);
    let hz = wave_height(p + vec2<f32>(0.0, e), height, length, phase) - wave_height(p - vec2<f32>(0.0, e), height, length, phase);
    return normalize(vec3<f32>(-hx, 2.0 * e, -hz));
}

@vertex
fn vs_mesh(in: VsIn) -> MeshOut {
    let o = objects[in.inst];
    var wp = o.model * vec4<f32>(in.pos, 1.0);
    if (o.ids.y == 1u) {
        wp.y = wp.y + wave_height(wp.xz, o.uvrect.y, o.uvrect.z, o.uvrect.w);
    }
    if (o.ids.y == 2u) {
        // wind: the part of the model above the entity's position leans and returns, more the higher it is (the foot stays), a little differently from place to place
        let origin = o.model[3].xyz;
        let h = max(wp.y - origin.y, 0.0) / max(o.uvrect.z, 0.1);
        let lean = o.uvrect.y * h * h;
        let ph = o.uvrect.w + origin.x * 0.7 + origin.z * 0.9;
        wp.x = wp.x + lean * (sin(ph) + 0.4 * sin(ph * 2.3 + 1.3));
        wp.z = wp.z + lean * 0.6 * (sin(ph * 0.8 + 2.0) + 0.3 * sin(ph * 1.9));
    }
    let nm = mat3x3<f32>(o.n0.xyz, o.n1.xyz, o.n2.xyz);
    var out: MeshOut;
    out.clip = frame.view_proj * wp;
    out.world = wp.xyz;
    out.nrm = nm * in.nrm;
    out.uv = in.uv;
    out.col = in.col;
    out.tw = in.tw;
    out.inst = in.inst;
    return out;
}

struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_sky(@builtin(vertex_index) i: u32) -> SkyOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: SkyOut;
    // z = 0 is "infinitely far" with the reversed depth range used by the main pass
    out.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    out.ndc = p * 2.0 - 1.0;
    return out;
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
    g_pix = in.clip.xy;
    g_fine = true;
    // the view direction of this pixel: unproject a point on the far plane
    let pa = frame.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let pb = frame.inv_view_proj * vec4<f32>(in.ndc, 0.0001, 1.0);
    let dir = normalize(pb.xyz / pb.w - pa.xyz / pa.w);
    var c: vec3<f32>;
    if (frame.p3.w > 0.5) {
        c = sky_color(dir, true);
    } else {
        c = frame.sky_horizon.rgb;
    }
    return vec4<f32>(c, 1.0);
}

// ------------------------------------------------------------------------------------------------ shadows

// sixteen points of a disc of radius 1 (a Poisson-like spread), for filtering the sun's shadows; and the pixel being shaded, for turning the pattern
var<private> POISSON16: array<vec2<f32>, 16> = array<vec2<f32>, 16>(
    vec2<f32>(-0.94201624, -0.39906216), vec2<f32>(0.94558609, -0.76890725), vec2<f32>(-0.09418410, -0.92938870), vec2<f32>(0.34495938, 0.29387760),
    vec2<f32>(-0.91588581, 0.45771432), vec2<f32>(-0.81544232, -0.87912464), vec2<f32>(-0.38277543, 0.27676845), vec2<f32>(0.97484398, 0.75648379),
    vec2<f32>(0.44323325, -0.97511554), vec2<f32>(0.53742981, -0.47373420), vec2<f32>(-0.26496911, -0.41893023), vec2<f32>(0.79197514, 0.19090188),
    vec2<f32>(-0.24188840, 0.99706507), vec2<f32>(-0.81409955, 0.91437590), vec2<f32>(0.19984126, 0.78641367), vec2<f32>(0.14383161, -0.14100790));

// Light reaching `world` according to one cascade (1 outside its map).
fn cascade_light(c: i32, world: vec3<f32>, n: vec3<f32>) -> f32 {
    var texel = frame.cascade_texel.x;
    if (c == 1) { texel = frame.cascade_texel.y; }
    if (c == 2) { texel = frame.cascade_texel.z; }
    // move the receiver a little along its normal (less self-shadowing on slopes), then look it up in the light's view
    let ndl = clamp(dot(n, frame.sun_dir.xyz), 0.0, 1.0);
    let off = n * (texel * (1.5 + 2.5 * (1.0 - ndl)));
    let lp = frame.cascade_vp[c] * vec4<f32>(world + off, 1.0);
    let uv = vec2<f32>(lp.x * 0.5 + 0.5, 0.5 - lp.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || lp.z > 1.0) {
        return 1.0;
    }
    let ts = 1.0 / frame.cascade_info.w;
    let depth = lp.z - 0.0004;
    let soft = frame.cascade_depth.w;
    if (soft <= 0.0) {
        var sum = 0.0;
        for (var dy = -1; dy <= 1; dy = dy + 1) {
            for (var dx = -1; dx <= 1; dx = dx + 1) {
                sum = sum + textureSampleCompareLevel(shadow_map, shadow_samp, uv + vec2<f32>(f32(dx), f32(dy)) * ts, c, depth);
            }
        }
        return sum / 9.0;
    }
    // contact-hardening shadows (PCSS): look for what blocks the light within a window that is as wide as the biggest penumbra worth having, find how far above this point the blockers are, and filter as widely as
    // the penumbra at that height; the sample pattern is turned by a different angle at every pixel (noise that the eye takes for softness)
    let depth_m = frame.cascade_depth[c];
    let tex_m = texel * 1.0;
    let size_i = vec2<i32>(textureDimensions(shadow_map));
    let ang0 = interleaved_noise(g_pix) * 6.2831853;
    let ca = cos(ang0);
    let sa = sin(ang0);
    let search_texels = clamp(soft * min(depth_m, 14.0) * 0.5 / tex_m, 1.5, 28.0);
    var blockers = 0.0;
    var blocker_sum = 0.0;
    for (var i = 0; i < 16; i = i + 1) {
        let o = POISSON16[i];
        let r = vec2<f32>(o.x * ca - o.y * sa, o.x * sa + o.y * ca) * search_texels;
        let q = clamp(vec2<i32>(uv * vec2<f32>(size_i) + r), vec2<i32>(0), size_i - vec2<i32>(1));
        let d = textureLoad(shadow_map, q, c, 0);
        if (d < depth) {
            blockers = blockers + 1.0;
            blocker_sum = blocker_sum + d;
        }
    }
    if (blockers < 0.5) {
        return 1.0;
    }
    let avg = blocker_sum / blockers;
    let pen_m = max((depth - avg) * depth_m * soft, tex_m);
    let filter_texels = clamp(pen_m / tex_m, 1.2, 28.0);
    var lit = 0.0;
    for (var i = 0; i < 16; i = i + 1) {
        let o = POISSON16[i];
        let r = vec2<f32>(o.x * ca - o.y * sa, o.x * sa + o.y * ca) * filter_texels * ts;
        lit = lit + textureSampleCompareLevel(shadow_map, shadow_samp, uv + r, c, depth);
    }
    return lit / 16.0;
}

// Light reaching `world` from the point light `k` (index into the shadowed lights) standing at `light_pos`.
fn point_shadow(k: i32, light_pos: vec3<f32>, world: vec3<f32>, n: vec3<f32>) -> f32 {
    let info = frame.psh[k].info;
    let dist = length(world - light_pos);
    // move the receiver a little along its normal: the footprint of a shadow texel grows with the distance
    let p = world + n * (dist * info.w * 2.5);
    let d = p - light_pos;
    let a = abs(d);
    var face = 4;
    if (a.x >= a.y && a.x >= a.z) {
        face = select(1, 0, d.x > 0.0);
    } else if (a.y >= a.z) {
        face = select(3, 2, d.y > 0.0);
    } else {
        face = select(5, 4, d.z > 0.0);
    }
    let lp = frame.psh[k].vp[face] * vec4<f32>(p, 1.0);
    if (lp.w <= 0.0) {
        return 1.0;
    }
    let ndc = lp.xyz / lp.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return 1.0;
    }
    // the receiver's reference depth, pulled a little towards the light in world units, as a perspective depth
    let m = max(lp.w - (0.015 + dist * 0.01), info.y);
    let depth = info.z * (m - info.y) / (m * (info.z - info.y));
    let ts = 1.0 / frame.p5.y;
    let layer = k * 6 + face;
    var sum = 0.0;
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            sum = sum + textureSampleCompareLevel(point_map, shadow_samp, uv + vec2<f32>(f32(dx), f32(dy)) * ts, layer, depth);
        }
    }
    return sum / 9.0;
}

fn shadow_factor(world: vec3<f32>, n: vec3<f32>) -> f32 {
    if (frame.p0.w < 0.5) {
        return 1.0;
    }
    let vz = -(frame.view * vec4<f32>(world, 1.0)).z;
    var c = 0;
    var end = frame.cascade_info.x;
    if (vz > frame.cascade_info.x) { c = 1; end = frame.cascade_info.y; }
    if (vz > frame.cascade_info.y) { c = 2; end = frame.cascade_info.z; }
    if (vz > frame.cascade_info.z) { return 1.0; }
    var s = cascade_light(c, world, n);
    // blend into the next cascade near the end of this one, and fade the last one out, so that no seams show
    let t = smoothstep(end * 0.85, end, vz);
    if (t > 0.0) {
        if (c < 2) {
            s = mix(s, cascade_light(c + 1, world, n), t);
        } else {
            s = mix(s, 1.0, t);
        }
    }
    return s;
}

// ------------------------------------------------------------------------------------------------ shading

// The classic lighting: ambient + diffuse + a Blinn-Phong highlight, exactly as the CPU reference renderer does it.
fn shade_classic(base: vec3<f32>, shininess: f32, p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var col = base * AMBIENT;
    let view = normalize(frame.eye.xyz - p);
    let count = u32(frame.p2.y);
    for (var i = 0u; i < count; i = i + 1u) {
        let l = frame.lights[i];
        var ldir: vec3<f32>;
        var atten = 1.0;
        if (l.vec.w > 0.5) {
            let to = l.vec.xyz - p;
            let dist = length(to);
            let k = min(dist / l.color.w, 1.0);
            let a = 1.0 - k * k;
            ldir = select(vec3<f32>(0.0, 1.0, 0.0), to / dist, dist > 0.0);
            atten = a * a;
        } else {
            ldir = l.vec.xyz;
        }
        let ndl = dot(n, ldir);
        if (ndl <= 0.0) {
            continue;
        }
        let lc = l.color.rgb * atten;
        col = col + base * lc * ndl;
        let h = normalize(ldir + view);
        let nh = max(dot(n, h), 0.0);
        col = col + lc * (pow(nh, shininess) * 0.25);
    }
    return col;
}

// Bump mapping without tangents (Mikkelsen): tilt `n` by the screen-space slope of a height field.
fn bump_normal(n: vec3<f32>, dpx: vec3<f32>, dpy: vec3<f32>, dhx: f32, dhy: f32) -> vec3<f32> {
    let r1 = cross(dpy, n);
    let r2 = cross(n, dpx);
    let det = dot(dpx, r1);
    let grad = sign(det) * (dhx * r1 + dhy * r2);
    return normalize(abs(det) * n - grad);
}

// A tangent-space normal map without tangents (Schuler's cotangent frame): the frame comes from how the position and the picture's coordinates change over the screen. `uv` runs down the picture,
// which is the way the green channel of a DirectX normal map points, so the map is read as it is.
fn map_normal(n: vec3<f32>, dpx: vec3<f32>, dpy: vec3<f32>, duvx: vec2<f32>, duvy: vec2<f32>, nm: vec3<f32>) -> vec3<f32> {
    let r1 = cross(dpy, n);
    let r2 = cross(n, dpx);
    // the formula assumes a screen whose y runs up; here it runs down, which turns both axes round: the sign of the determinant (as in `bump_normal`) puts that right, and a mirrored mesh as well
    let sg = select(-1.0, 1.0, dot(dpx, r1) >= 0.0);
    let t = (r1 * duvx.x + r2 * duvy.x) * sg;
    let b = (r1 * duvx.y + r2 * duvy.y) * sg;
    let inv = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-12));
    return normalize(t * (inv * nm.x) + b * (inv * nm.y) + n * nm.z);
}

// Parallax occlusion mapping: march a ray from the eye through the height map's volume (as many layers as the angle needs) and see where it meets the surface; the picture is sampled there. `uv` runs down the picture,
// like the frame of `map_normal`, and the offsets are in uv units (`scale` is a share of one repeat of the picture).
fn parallax_uv(uv: vec2<f32>, v: vec3<f32>, n: vec3<f32>, dpx: vec3<f32>, dpy: vec3<f32>, duvx: vec2<f32>, duvy: vec2<f32>, scale: f32) -> vec2<f32> {
    let r1 = cross(dpy, n);
    let r2 = cross(n, dpx);
    let sg = select(-1.0, 1.0, dot(dpx, r1) >= 0.0);
    let t = (r1 * duvx.x + r2 * duvy.x) * sg;
    let b = (r1 * duvx.y + r2 * duvy.y) * sg;
    let inv = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-12));
    let vt = vec3<f32>(dot(v, t * inv), dot(v, b * inv), dot(v, n));
    let vz = max(vt.z, 0.12);
    let layers = mix(40.0, 12.0, clamp(vt.z, 0.0, 1.0));
    let layer_depth = 1.0 / layers;
    let delta = vt.xy / vz * scale / layers;
    var cur = uv;
    var depth = 0.0;
    var map_depth = 1.0 - textureSampleGrad(disp_tex, tex_samp, cur, duvx, duvy).r;
    var prev_depth = map_depth;
    for (var i = 0; i < 40; i = i + 1) {
        if (depth >= map_depth || f32(i) >= layers) {
            break;
        }
        prev_depth = map_depth;
        cur = cur - delta;
        map_depth = 1.0 - textureSampleGrad(disp_tex, tex_samp, cur, duvx, duvy).r;
        depth = depth + layer_depth;
    }
    let after = map_depth - depth;
    let before = prev_depth - (depth - layer_depth);
    let w = clamp(after / (after - before - 1e-5), 0.0, 1.0);
    return mix(cur, cur + delta, w);
}

fn d_ggx(nh: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn vis_smith(nl: f32, nv: f32, a: f32) -> f32 {
    let a2 = a * a;
    let gv = nl * sqrt(nv * nv * (1.0 - a2) + a2);
    let gl = nv * sqrt(nl * nl * (1.0 - a2) + a2);
    return 0.5 / max(gv + gl, 1e-5);
}

fn fresnel(f0: vec3<f32>, vh: f32) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - vh, 5.0);
}

struct Pbr {
    diffuse: vec3<f32>,
    spec: vec3<f32>,
};

fn shade_pbr(albedo: vec3<f32>, metallic: f32, rough_in: f32, world: vec3<f32>, n: vec3<f32>, v: vec3<f32>, ao: f32, shadow: f32) -> Pbr {
    let rough = clamp(rough_in, 0.045, 1.0);
    let a = rough * rough;
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let diff_col = albedo * (1.0 - metallic);
    let nv = max(dot(n, v), 1e-4);
    var lo_diff = vec3<f32>(0.0);
    var lo_spec = vec3<f32>(0.0);
    let count = u32(frame.p2.y);
    let sun = i32(frame.p4.x);
    for (var i = 0u; i < count; i = i + 1u) {
        let l = frame.lights[i];
        var ldir: vec3<f32>;
        var atten = 1.0;
        if (l.vec.w > 0.5) {
            let to = l.vec.xyz - world;
            let dist = length(to);
            let k = min(dist / l.color.w, 1.0);
            let w = 1.0 - k * k;
            ldir = select(vec3<f32>(0.0, 1.0, 0.0), to / dist, dist > 0.0);
            atten = w * w;
            if (l.spot.w > -1.5) {
                // inside the cone of a spot light, fading out towards its edge
                atten = atten * smoothstep(l.spot.w, l.cone.x, dot(-ldir, l.spot.xyz));
            }
        } else {
            ldir = l.vec.xyz;
        }
        let nl = dot(n, ldir);
        if (nl <= 0.0 || atten <= 0.0) {
            continue;
        }
        var radiance = l.color.rgb * atten;
        if (i32(i) == sun) {
            radiance = radiance * shadow;
        } else if (l.vec.w > 0.5) {
            if (i32(frame.psh[0].info.x) == i32(i)) {
                radiance = radiance * point_shadow(0, l.vec.xyz, world, n);
            } else if (i32(frame.psh[1].info.x) == i32(i)) {
                radiance = radiance * point_shadow(1, l.vec.xyz, world, n);
            }
        }
        let h = normalize(ldir + v);
        let nh = max(dot(n, h), 0.0);
        let vh = max(dot(v, h), 0.0);
        let f = fresnel(f0, vh);
        let spec = d_ggx(nh, a) * vis_smith(nl, nv, a) * f;
        // radiance counts as "irradiance / pi", so the diffuse term needs no 1 / pi and the specular one a factor pi
        lo_diff = lo_diff + (vec3<f32>(1.0) - f) * diff_col * radiance * nl;
        lo_spec = lo_spec + spec * PI * radiance * nl;
    }
    // the sky around the surface
    let amb_diff = sky_irradiance(n) * diff_col * ao;
    let r = reflect(-v, n);
    var env = sky_color(r, rough < 0.3);
    // a rough surface sees a blurred sky
    env = mix(env, sky_irradiance(r), smoothstep(0.15, 0.95, rough));
    let amb_spec = env * env_brdf(f0, rough, nv) * specular_occlusion(nv, ao, rough) * frame.p0.x;
    return Pbr(lo_diff + amb_diff, lo_spec + amb_spec);
}

// What a reflection probe changes about `shade_pbr`'s result: inside its box what it captured takes the place of the sky's reflection and of the sky's ambient light (the probe holds the light of the
// world as it was, so the `ambient` strength is not applied to it), by the weight `w` of the probe. It is added to the result, so that `shade_pbr` itself stays as it was for a picture without probes.
fn shade_probe_change(albedo: vec3<f32>, metallic: f32, rough_in: f32, world: vec3<f32>, n: vec3<f32>, v: vec3<f32>, ao: f32) -> Pbr {
    let rough = clamp(rough_in, 0.045, 1.0);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let diff_col = albedo * (1.0 - metallic);
    let nv = max(dot(n, v), 1e-4);
    let r = reflect(-v, n);
    var env = sky_color(r, rough < 0.3);
    env = mix(env, sky_irradiance(r), smoothstep(0.15, 0.95, rough));
    let ph = probe_light(world, n, r, rough);
    return Pbr((ph.irr - sky_irradiance(n)) * (diff_col * ao * ph.w), (ph.env - env * frame.p0.x) * (env_brdf(f0, rough, nv) * specular_occlusion(nv, ao, rough) * ph.w));
}

// What a mesh pixel looks like, in parts: the light the surface scatters (`diffuse`), the glints and reflections on top (`spec`), how
// much distance fog covers it, and how opaque it is. Opaque meshes add the parts up; see-through ones blend them with the picture.
struct Lit {
    diffuse: vec3<f32>,
    spec: vec3<f32>,
    fog: f32,
    alpha: f32,
};

fn lit_mesh(in: MeshOut) -> Lit {
    let o = objects[in.inst];
    var n = normalize(in.nrm);
    if (o.ids.y == 1u) {
        // the ripples tilt the surface's own normal (flat for a sheet, leaning downstream for a river)
        n = normalize(wave_normal(in.world.xz, o.uvrect.y, o.uvrect.z, o.uvrect.w) + (n - vec3<f32>(0.0, 1.0, 0.0)));
    }
    var base = o.color.rgb;
    if (o.misc.w > 0.5) {
        base = base * in.col;
    }
    // the texture is always sampled (derivatives need uniform control flow); whether it counts is decided afterwards
    let uvs = in.uv * o.misc.xy;
    var tuv = vec2<f32>(uvs.x, 1.0 - uvs.y);
    let dpx = dpdx(in.world);
    let dpy = dpdy(in.world);
    let duvx = dpdx(tuv);
    let duvy = dpdy(tuv);
    let classic = frame.p2.x < 0.5;
    let maps = u32(o.pbr.z + 0.5);
    if (!classic && (maps & 4u) != 0u && o.uvrect.y > 0.0) {
        tuv = parallax_uv(tuv, normalize(frame.eye.xyz - in.world), normalize(in.nrm), dpx, dpy, duvx, duvy, o.uvrect.y);
    }
    let t = textureSampleGrad(tex, tex_samp, tuv, duvx, duvy);
    // the relief of the built-in materials: the height map's slope, as seen from this pixel, tilts the normal
    let relief = textureSampleGrad(height_tex, tex_samp, tuv, duvx, duvy).r * o.uvrect.x;
    let dhx = dpdx(relief);
    let dhy = dpdy(relief);
    if (!classic && o.uvrect.x > 0.0) {
        n = bump_normal(n, dpx, dpy, dhx, dhy);
    }
    // the maps are always sampled (derivatives need uniform control flow); whether they count is decided afterwards
    let nmap = textureSampleGrad(normal_tex, tex_samp, tuv, duvx, duvy).xyz * 2.0 - vec3<f32>(1.0);
    let armv = textureSampleGrad(arm_tex, tex_samp, tuv, duvx, duvy);
    if (!classic && (maps & 1u) != 0u) {
        n = map_normal(n, dpx, dpy, duvx, duvy, vec3<f32>(nmap.xy * o.pbr.x, nmap.z));
    }
    if (!classic && (maps & 2u) != 0u) {
        g_ao = mix(1.0, armv.r, o.pbr.y);
        g_rough = clamp(armv.g * o.pbr.w, 0.045, 1.0);
        g_metal = armv.b;
    }
    // the spread of the normal over the pixel (Kaplanyan / Tokuyoshi): twice the variance of its screen-space slope, capped
    let dndx = dpdx(n);
    let dndy = dpdy(n);
    g_nvar = min(0.5 * (dot(dndx, dndx) + dot(dndy, dndy)), 0.18);
    if (o.misc.z > 0.5 && in.tw > 0.001) {
        if (in.tw > 0.999 && t.a < 0.5) {
            discard;
        }
        // classic textures hold display colours (squared into the working space); the high-quality ones are sRGB textures
        let lin = select(t.rgb, t.rgb * t.rgb, classic);
        base = base * (vec3<f32>(1.0) + in.tw * (lin - vec3<f32>(1.0)));
    }
    if (!classic && o.ids.y == 1u && o.pbr.w > 0.5 && frame.p28.x > 0.5) {
        return lit_water(in, o, base, n);
    }
    return lit_tail(in, o, base, n, classic);
}

// Deep water that knows how deep it is: the distance from this piece of the surface along the view ray to the opaque thing behind it (the prepass' depth) says how much of the bottom shows. The shallows' colour
// (carried in the spare numbers of the normal matrix) fades to the water's own colour with the depth, the water is the clearer the shallower it is, and foam gathers where the water meets what it touches.
fn lit_water(in: MeshOut, o: Object, base: vec3<f32>, n: vec3<f32>) -> Lit {
    let px = vec2<i32>(in.clip.xy);
    let d = textureLoad(opaque_depth, px, 0);
    var thick = 1.0e4;
    if (d > 0.0) {
        let uv = (vec2<f32>(px) + 0.5) * frame.p1.zw;
        let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
        let w4 = frame.inv_view_proj * vec4<f32>(ndc, d, 1.0);
        thick = max(length(w4.xyz / w4.w - frame.eye.xyz) - length(in.world - frame.eye.xyz), 0.0);
    }
    let clarity = o.pbr.x;
    let absorb = 1.0 - exp(-thick / clarity);
    let shallow = vec3<f32>(o.n0.w, o.n1.w, o.n2.w);
    var col = mix(shallow, base, absorb);
    // foam: a band where the water is thin, broken up by a noise that drifts with the waves
    let drift = o.uvrect.w * 0.25;
    let nz = 0.5 * value_noise(in.world.xz * 1.3 + vec2<f32>(drift, -drift)) + 0.5 * value_noise(in.world.xz * 3.1 - vec2<f32>(drift * 1.7, drift));
    let band = 1.0 - smoothstep(0.0, max(o.pbr.y, 0.01), thick);
    let foam = clamp(band * smoothstep(0.25, 0.75, nz + band * 0.5) * o.pbr.z, 0.0, 1.0);
    col = mix(col, vec3<f32>(0.85, 0.88, 0.9), foam);
    var l = lit_tail(in, o, col, n, false);
    // see-through where it is thin, opaque where it is deep; foam is solid; the sky's glint at grazing angles is lit_tail's
    let v = normalize(frame.eye.xyz - in.world);
    let fres = pow(1.0 - clamp(dot(n, v), 0.0, 1.0), 5.0);
    let a = clamp(0.06 + absorb * 0.94, 0.0, 1.0);
    l.alpha = clamp(max(a + (1.0 - a) * fres, foam), 0.0, 1.0);
    return l;
}

// The shading every mesh pixel ends with once its colour and normal are known: the classic lighting, or the physically based one with shadows, ambient occlusion, sky and fog.
fn lit_tail(in: MeshOut, o: Object, base: vec3<f32>, n: vec3<f32>, classic: bool) -> Lit {
    if (classic) {
        var col = shade_classic(base, o.mat.x, in.world, n);
        if (o.mat.y > 0.0) {
            col = col + base * o.mat.y;
        }
        return Lit(col, vec3<f32>(0.0), 0.0, 1.0);
    }
    let v = normalize(frame.eye.xyz - in.world);
    let uv = in.clip.xy * frame.p1.zw;
    let aot = textureSampleLevel(ao_tex, lin_samp, uv, 0.0);
    let ao = aot.a * g_ao;
    let metal = select(o.mat.z, g_metal, g_rough >= 0.0);
    // roughness filtered by the pixel's normal spread: alpha^2 grows by it
    let rough_raw = select(o.mat.w, g_rough, g_rough >= 0.0);
    let rough = sqrt(clamp(rough_raw * rough_raw + g_nvar, 0.0, 1.0));
    var shadow = 1.0;
    if (i32(frame.p4.x) >= 0) {
        g_pix = in.clip.xy;
        shadow = shadow_factor(in.world, n);
    }
    var pbr = shade_pbr(base, metal, rough, in.world, n, v, ao, shadow);
    // light that bounced off the other surfaces in view (`GlobalIllumination`), in the colour of this one
    pbr.diffuse = pbr.diffuse + base * (1.0 - metal) * aot.rgb;
    let fog = fog_amount(in.world);
    // glass is more mirror-like at grazing angles
    let nv = clamp(dot(n, v), 0.0, 1.0);
    let alpha = o.color.a + (1.0 - o.color.a) * pow(1.0 - nv, 5.0);
    if (frame.p18.x > 0.5) {
        let change = shade_probe_change(base, metal, rough, in.world, n, v, ao);
        return Lit(pbr.diffuse + change.diffuse + base * o.mat.y, pbr.spec + change.spec, fog, alpha);
    }
    return Lit(pbr.diffuse + base * o.mat.y, pbr.spec, fog, alpha);
}

// ------------------------------------------------------------------------------------------------ painted terrain

// A terrain with `TerrainLayers`: a splat map (an RGBA picture over the whole ground) weights up to four layer textures, each repeating over the ground at its own rate.
struct SplatU {
    // the repeats of layers 0 and 1 (xy, zw), then of layers 2 and 3
    scales: array<vec4<f32>, 2>,
    // the depth of each layer's relief in metres
    bump: vec4<f32>,
    // each layer's tint, in the working space
    tint: array<vec4<f32>, 4>,
    // x: how many layers there are, y: 1 when any layer has a relief
    info: vec4<f32>,
};

@group(2) @binding(0) var<uniform> splat: SplatU;
@group(2) @binding(1) var splat_map: texture_2d<f32>;
@group(2) @binding(2) var splat_samp: sampler;
@group(2) @binding(3) var layer_samp: sampler;
@group(2) @binding(4) var lay0: texture_2d<f32>;
@group(2) @binding(5) var lay1: texture_2d<f32>;
@group(2) @binding(6) var lay2: texture_2d<f32>;
@group(2) @binding(7) var lay3: texture_2d<f32>;
@group(2) @binding(8) var hgt0: texture_2d<f32>;
@group(2) @binding(9) var hgt1: texture_2d<f32>;
@group(2) @binding(10) var hgt2: texture_2d<f32>;
@group(2) @binding(11) var hgt3: texture_2d<f32>;

// How much of each layer there is at this place of the ground (`uv` runs 0 to 1 over it, the first row of the map at v = 0): the map's channels, blended between texels
// that lie on the corners of the cells, and normalised. The reference renderer does the same sum (`Splat::weights`).
fn splat_weights(uv: vec2<f32>) -> vec4<f32> {
    let dim = vec2<f32>(textureDimensions(splat_map));
    let suv = (clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)) * (dim - vec2<f32>(1.0)) + vec2<f32>(0.5)) / dim;
    let r = textureSampleLevel(splat_map, splat_samp, suv, 0.0);
    let n = splat.info.x;
    let w = vec4<f32>(r.x, select(0.0, r.y, n > 1.5), select(0.0, r.z, n > 2.5), select(0.0, r.w, n > 3.5));
    let s = w.x + w.y + w.z + w.w;
    if (s > 0.0001) {
        return w / s;
    }
    return vec4<f32>(1.0, 0.0, 0.0, 0.0);
}

// One layer at this place: its colour (in the working space) and the height of its relief, 0 to 1.
fn splat_layer(tex: texture_2d<f32>, hgt: texture_2d<f32>, uv: vec2<f32>, scale: vec2<f32>, classic: bool) -> vec4<f32> {
    let s = uv * scale;
    let tuv = vec2<f32>(s.x, 1.0 - s.y);
    let t = textureSample(tex, layer_samp, tuv);
    let h = textureSample(hgt, layer_samp, tuv).r;
    return vec4<f32>(select(t.rgb, t.rgb * t.rgb, classic), h);
}

fn lit_splat(in: MeshOut) -> Lit {
    let o = objects[in.inst];
    var n = normalize(in.nrm);
    let classic = frame.p2.x < 0.5;
    let w = splat_weights(in.uv);
    let a0 = splat_layer(lay0, hgt0, in.uv, splat.scales[0].xy, classic);
    let a1 = splat_layer(lay1, hgt1, in.uv, splat.scales[0].zw, classic);
    let a2 = splat_layer(lay2, hgt2, in.uv, splat.scales[1].xy, classic);
    let a3 = splat_layer(lay3, hgt3, in.uv, splat.scales[1].zw, classic);
    let albedo = a0.rgb * splat.tint[0].rgb * w.x + a1.rgb * splat.tint[1].rgb * w.y + a2.rgb * splat.tint[2].rgb * w.z + a3.rgb * splat.tint[3].rgb * w.w;
    let base = o.color.rgb * albedo;
    // the relief of the built-in materials, as in `lit_mesh`: the slope of the blended height map tilts the normal (centred on a middle height, so that a change of layer is no ridge)
    let relief = (a0.a - 0.5) * splat.bump.x * w.x + (a1.a - 0.5) * splat.bump.y * w.y + (a2.a - 0.5) * splat.bump.z * w.z + (a3.a - 0.5) * splat.bump.w * w.w;
    let dpx = dpdx(in.world);
    let dpy = dpdy(in.world);
    let dhx = dpdx(relief);
    let dhy = dpdy(relief);
    if (!classic && splat.info.y > 0.5) {
        n = bump_normal(n, dpx, dpy, dhx, dhy);
    }
    return lit_tail(in, o, base, n, classic);
}

@fragment
fn fs_mesh(in: MeshOut) -> @location(0) vec4<f32> {
    let l = lit_mesh(in);
    let c = (l.diffuse + l.spec) * (1.0 - l.fog) + fog_color_toward(normalize(in.world - frame.eye.xyz)) * l.fog;
    return vec4<f32>(min(c, vec3<f32>(60000.0)), 1.0);
}

// A painted terrain: always opaque.
@fragment
fn fs_mesh_splat(in: MeshOut) -> @location(0) vec4<f32> {
    let l = lit_splat(in);
    let c = (l.diffuse + l.spec) * (1.0 - l.fog) + fog_color_toward(normalize(in.world - frame.eye.xyz)) * l.fog;
    return vec4<f32>(min(c, vec3<f32>(60000.0)), 1.0);
}

// A see-through mesh: premultiplied alpha, the glints are not dimmed by the transparency.
@fragment
fn fs_mesh_blend(in: MeshOut) -> @location(0) vec4<f32> {
    let l = lit_mesh(in);
    let body = (l.diffuse * (1.0 - l.fog) + fog_color_toward(normalize(in.world - frame.eye.xyz)) * l.fog) * l.alpha;
    let c = body + l.spec * (1.0 - l.fog);
    return vec4<f32>(min(c, vec3<f32>(60000.0)), l.alpha);
}

// ------------------------------------------------------------------------------------------------ screen-space reflections

// What the reflection pass needs to know about every mesh pixel, drawn once for the opaque and the see-through meshes (the nearest wins): where the surface points (xyz, in the world,
// with the relief of its material and the ripples of water) and how rough it is (w), and how much it reflects when seen head on (rgb: 0.04 for a plastic, the colour for a metal) and a 1 in w
// where there is a surface. The base colour and the normal are worked out as `lit_mesh` does it: keep the two in step.
struct GbufOut {
    @location(0) a: vec4<f32>,
    @location(1) b: vec4<f32>,
};

@fragment
fn fs_gbuf(in: MeshOut) -> GbufOut {
    let o = objects[in.inst];
    var n = normalize(in.nrm);
    if (o.ids.y == 1u) {
        n = normalize(wave_normal(in.world.xz, o.uvrect.y, o.uvrect.z, o.uvrect.w) + (n - vec3<f32>(0.0, 1.0, 0.0)));
    }
    var base = o.color.rgb;
    if (o.misc.w > 0.5) {
        base = base * in.col;
    }
    let uvs = in.uv * o.misc.xy;
    let tuv = vec2<f32>(uvs.x, 1.0 - uvs.y);
    let t = textureSample(tex, tex_samp, tuv);
    let relief = textureSample(height_tex, tex_samp, tuv).r * o.uvrect.x;
    let dpx = dpdx(in.world);
    let dpy = dpdy(in.world);
    let dhx = dpdx(relief);
    let dhy = dpdy(relief);
    if (o.uvrect.x > 0.0) {
        n = bump_normal(n, dpx, dpy, dhx, dhy);
    }
    let nmap = textureSample(normal_tex, tex_samp, tuv).xyz * 2.0 - vec3<f32>(1.0);
    let armv = textureSample(arm_tex, tex_samp, tuv);
    let duvx = dpdx(tuv);
    let duvy = dpdy(tuv);
    let maps = u32(o.pbr.z + 0.5);
    var rough = o.mat.w;
    var metal = o.mat.z;
    if ((maps & 1u) != 0u) {
        n = map_normal(n, dpx, dpy, duvx, duvy, vec3<f32>(nmap.xy * o.pbr.x, nmap.z));
    }
    if ((maps & 2u) != 0u) {
        rough = armv.g * o.pbr.w;
        metal = armv.b;
    }
    if (o.misc.z > 0.5 && in.tw > 0.001) {
        if (in.tw > 0.999 && t.a < 0.5) {
            discard;
        }
        base = base * (vec3<f32>(1.0) + in.tw * (t.rgb - vec3<f32>(1.0)));
    }
    var out: GbufOut;
    out.a = vec4<f32>(n, clamp(rough, 0.045, 1.0));
    out.b = vec4<f32>(mix(vec3<f32>(0.04), base, metal), 1.0);
    return out;
}

// ------------------------------------------------------------------------------------------------ entity ids

// The entity-id pass: the same geometry, drawn into an integer image that says which entity is at each pixel. Cut-outs and nearly
// transparent sprite pixels do not count, as in the reference renderer.
@fragment
fn fs_id_mesh(in: MeshOut) -> @location(0) u32 {
    let o = objects[in.inst];
    let uvs = in.uv * o.misc.xy;
    let t = textureSampleLevel(tex, tex_samp, vec2<f32>(uvs.x, 1.0 - uvs.y), 0.0);
    if (o.misc.z > 0.5 && in.tw > 0.999 && t.a < 0.5) {
        discard;
    }
    return o.ids.x;
}

// ------------------------------------------------------------------------------------------------ sprites and tile maps

struct SpriteOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) inst: u32,
};

@vertex
fn vs_sprite(in: VsIn) -> SpriteOut {
    let o = objects[in.inst];
    var out: SpriteOut;
    out.clip = frame.view_proj * (o.model * vec4<f32>(in.pos, 1.0));
    out.uv = o.uvrect.xy + in.uv * o.uvrect.zw;
    out.inst = in.inst;
    return out;
}

@fragment
fn fs_sprite(in: SpriteOut) -> @location(0) vec4<f32> {
    let o = objects[in.inst];
    // v counts from the bottom of the picture, texture rows from the top
    let texel = textureSampleLevel(tex, tex_samp, vec2<f32>(in.uv.x, 1.0 - in.uv.y), 0.0);
    var rgb = vec3<f32>(1.0);
    var ta = 1.0;
    if (o.misc.z > 0.5) {
        let classic = frame.p2.x < 0.5;
        rgb = select(texel.rgb, texel.rgb * texel.rgb, classic);
        ta = texel.a;
    }
    let a = o.color.a * ta;
    if (a <= 0.0) {
        discard;
    }
    return vec4<f32>(o.color.rgb * rgb, a);
}

// Particle clouds: one mesh of camera-facing quads built by the reference renderer's code; the vertex colour is the particle's colour (in the classic
// mode's gamma-2.0 working space) and the texture weight its opacity.
struct ParticleOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) col: vec3<f32>,
    @location(2) a: f32,
};

@vertex
fn vs_particle(in: VsIn) -> ParticleOut {
    var out: ParticleOut;
    out.clip = frame.view_proj * vec4<f32>(in.pos, 1.0);
    out.uv = in.uv;
    out.col = in.col;
    out.a = in.tw;
    return out;
}

fn particle_light(c: vec3<f32>, texel: vec4<f32>) -> vec3<f32> {
    let classic = frame.p2.x < 0.5;
    // the high-quality mode works in true linear light: undo the gamma-2.0 of the particle's colour and sRGB-decode it
    let disp = sqrt(c);
    let lin = select(pow((disp + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), disp / 12.92, disp <= vec3<f32>(0.04045));
    let base = select(lin, c, classic);
    return base * select(texel.rgb, texel.rgb * texel.rgb, classic);
}

@fragment
fn fs_particle(in: ParticleOut) -> @location(0) vec4<f32> {
    let texel = textureSampleLevel(tex, tex_samp, vec2<f32>(in.uv.x, 1.0 - in.uv.y), 0.0);
    let a = in.a * texel.a;
    if (a <= 0.0) {
        discard;
    }
    return vec4<f32>(particle_light(in.col, texel), a);
}

// Added light: the colour times the opacity, summed into the picture.
@fragment
fn fs_particle_add(in: ParticleOut) -> @location(0) vec4<f32> {
    let texel = textureSampleLevel(tex, tex_samp, vec2<f32>(in.uv.x, 1.0 - in.uv.y), 0.0);
    let a = in.a * texel.a;
    if (a <= 0.0) {
        discard;
    }
    return vec4<f32>(particle_light(in.col, texel) * a, a);
}

@fragment
fn fs_id_sprite(in: SpriteOut) -> @location(0) u32 {
    let o = objects[in.inst];
    let texel = textureSampleLevel(tex, tex_samp, vec2<f32>(in.uv.x, 1.0 - in.uv.y), 0.0);
    var ta = 1.0;
    if (o.misc.z > 0.5) {
        ta = texel.a;
    }
    if (o.color.a * ta < 0.5) {
        discard;
    }
    return o.ids.x;
}

// ------------------------------------------------------------------------------------------------ grass made on the card (`GrassField`)

// One ring of one field: the tufts are the cells of a lattice that follows the camera (so they stay where they are as it moves), each placed, sized, tinted and bent by hashes of its cell.
struct GrassU {
    // cell size, inner radius, outer radius, cells along a side
    ring: vec4<f32>,
    // the lattice's corner (in cells, x and z), the object that holds the material, the width of the fade at the edges
    orig: vec4<f32>,
    // the terrain: where it stands (xyz) and how high white is
    tpos: vec4<f32>,
    // its size (x, z) and the picture's columns and rows
    tsize: vec4<f32>,
    // wind: lean of the tops, metres between crests, direction (radians), clock
    wind: vec4<f32>,
    // the smallest and biggest scale of a tuft, 0, a seed
    tuft: vec4<f32>,
    ca: vec4<f32>,
    cb: vec4<f32>,
    // the camera's place
    eye: vec4<f32>,
};

@group(2) @binding(12) var<uniform> gu: GrassU;
@group(2) @binding(13) var grass_heights: texture_2d<f32>;

fn hash_u(a: u32) -> u32 {
    var x = a;
    x = x ^ (x >> 16u);
    x = x * 0x7feb352du;
    x = x ^ (x >> 15u);
    x = x * 0x846ca68bu;
    x = x ^ (x >> 16u);
    return x;
}

fn hash3(ix: i32, iz: i32, seed: f32) -> vec3<f32> {
    let h1 = hash_u(bitcast<u32>(ix) * 73856093u ^ bitcast<u32>(iz) * 19349663u ^ bitcast<u32>(seed));
    let h2 = hash_u(h1 + 0x9e3779b9u);
    let h3 = hash_u(h2 + 0x85ebca6bu);
    return vec3<f32>(f32(h1 & 0xffffffu), f32(h2 & 0xffffffu), f32(h3 & 0xffffffu)) / 16777216.0;
}

// The terrain's height at (x, z): the picture's four nearest samples, blended (the first row is -z).
fn grass_ground(x: f32, z: f32) -> f32 {
    let cols = gu.tsize.z;
    let rows = gu.tsize.w;
    let fx = clamp((x - (gu.tpos.x - gu.tsize.x * 0.5)) / gu.tsize.x * (cols - 1.0), 0.0, cols - 1.001);
    let fz = clamp((z - (gu.tpos.z - gu.tsize.y * 0.5)) / gu.tsize.y * (rows - 1.0), 0.0, rows - 1.001);
    let i = i32(floor(fx));
    let j = i32(floor(fz));
    let tx = fx - floor(fx);
    let tz = fz - floor(fz);
    let h00 = textureLoad(grass_heights, vec2<i32>(i, j), 0).r;
    let h10 = textureLoad(grass_heights, vec2<i32>(i + 1, j), 0).r;
    let h01 = textureLoad(grass_heights, vec2<i32>(i, j + 1), 0).r;
    let h11 = textureLoad(grass_heights, vec2<i32>(i + 1, j + 1), 0).r;
    return gu.tpos.y + mix(mix(h00, h10, tx), mix(h01, h11, tx), tz) * gu.tpos.w;
}

@vertex
fn vs_grass(in: VsIn) -> MeshOut {
    var out: MeshOut;
    out.inst = u32(gu.orig.z);
    out.nrm = vec3<f32>(0.0, 1.0, 0.0);
    out.uv = in.uv;
    out.tw = 1.0;
    out.col = vec3<f32>(1.0);
    out.world = vec3<f32>(0.0);
    out.clip = vec4<f32>(0.0, 0.0, 2.0, 1.0);
    let m = u32(gu.ring.w);
    let ix = i32(gu.orig.x) + i32(in.inst % m);
    let iz = i32(gu.orig.y) + i32(in.inst / m);
    let r = hash3(ix, iz, gu.tuft.w);
    let cell = gu.ring.x;
    let px = (f32(ix) + r.x) * cell;
    let pz = (f32(iz) + r.y) * cell;
    let d = distance(vec2<f32>(px, pz), gu.eye.xz);
    // inside its ring, fading out at the edges (the rings overlap, so the change from one to the next is a swap of scale, not a pop)
    var fade = smoothstep(gu.ring.z, gu.ring.z - gu.orig.w, d);
    if (gu.ring.y > 0.0) {
        fade = fade * smoothstep(gu.ring.y, gu.ring.y + gu.orig.w, d);
    }
    if (fade <= 0.01) {
        return out;
    }
    let ground = grass_ground(px, pz);
    let r2 = hash3(ix * 3 + 1, iz * 7 + 5, gu.tuft.w + 3.0);
    let size = mix(gu.tuft.x, gu.tuft.y, r2.x) * fade;
    let yaw = r.z * 6.2831853;
    let local = in.pos * size;
    let cy = cos(yaw);
    let sy = sin(yaw);
    var wp = vec3<f32>(px + cy * local.x - sy * local.z, ground + local.y - 0.03, pz + sy * local.x + cy * local.z);
    // the wind: a gust that travels across the field (a long wave along the wind with a slow wobble across it), a quick flutter of its own for every tuft; the foot stays, the top leans (by the square of the height)
    let hn = clamp(in.pos.y / 0.62, 0.0, 1.0);
    let dir = vec2<f32>(cos(gu.wind.z), sin(gu.wind.z));
    let side = vec2<f32>(-dir.y, dir.x);
    let k = 6.2831853 / gu.wind.y;
    let t = gu.wind.w;
    let along = dot(vec2<f32>(px, pz), dir);
    let across = dot(vec2<f32>(px, pz), side);
    let gust = 0.5 + 0.5 * sin(along * k - t + 0.8 * sin(across * k * 0.6 + t * 0.2));
    let flutter = 0.22 * sin(t * 4.1 + along * 0.9 + r2.y * 6.28) + 0.12 * sin(t * 6.3 + r2.z * 6.28);
    let lean = hn * hn * gu.wind.x * size * (0.3 + 0.7 * gust);
    let shake = hn * hn * gu.wind.x * size * flutter * 0.5;
    wp.x = wp.x + dir.x * lean + side.x * shake;
    wp.z = wp.z + dir.y * lean + side.y * shake;
    wp.y = wp.y - 0.5 * lean * lean / max(local.y + 0.3, 0.3);
    out.world = wp;
    out.clip = frame.view_proj * vec4<f32>(wp, 1.0);
    // a tint between the two colours, and a darker foot (grass at the bottom of a field is in shade)
    // patches of lush and dry grass over long stretches (a slow noise of the place), and the sun through the blades when looking towards it
    let ps = max(gu.tuft.z, 5.0);
    let n1 = 0.5 + 0.5 * sin(px / ps + sin(pz / (1.37 * ps)) * 2.0);
    let n2 = 0.5 + 0.5 * sin(pz / (0.83 * ps) - px / (1.9 * ps) + 1.3);
    let dry = smoothstep(0.45, 0.9, n1 * 0.65 + n2 * 0.35);
    let tint = mix(gu.ca.rgb, gu.cb.rgb, clamp(r2.y * 0.4 + dry * 0.75, 0.0, 1.0));
    var back = 0.0;
    let sun = i32(frame.p4.x);
    if (sun >= 0) {
        let toward_sun = frame.lights[u32(sun)].vec.xyz;
        back = pow(clamp(dot(normalize(wp - gu.eye.xyz), toward_sun), 0.0, 1.0), 3.0);
    }
    out.col = tint * mix(0.4, 1.0, hn * 0.8 + 0.2 * gust) * (vec3<f32>(1.0) + back * hn * vec3<f32>(1.5, 1.35, 0.55));
    out.nrm = normalize(vec3<f32>(dir.x * lean * 0.8, 1.0, dir.y * lean * 0.8));
    return out;
}
