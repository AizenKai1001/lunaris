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

@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var tex_samp: sampler;
@group(1) @binding(2) var height_tex: texture_2d<f32>;

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

// ------------------------------------------------------------------------------------------------ the sky

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    q = q + dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn hash31(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.1031);
    q = q + dot(q, q.zyx + 31.32);
    return fract((q.x + q.y) * q.z);
}

// Stars on a dark sky: a grid of cells on the sphere, a few with a star at a random spot, a few of those bright.
fn stars(dir: vec3<f32>) -> vec3<f32> {
    let night = 1.0 - smoothstep(0.015, 0.2, luminance(frame.sky_top.rgb));
    if (night <= 0.0 || dir.y <= 0.0) {
        return vec3<f32>(0.0);
    }
    let g = dir * 90.0;
    let cell = floor(g);
    let h = hash31(cell);
    if (h < 0.985) {
        return vec3<f32>(0.0);
    }
    let center = vec3<f32>(hash31(cell + 17.0), hash31(cell + 41.0), hash31(cell + 73.0));
    let d = length(fract(g) - center);
    let mag = (h - 0.985) / 0.015;
    let tint = mix(vec3<f32>(1.0, 0.85, 0.75), vec3<f32>(0.8, 0.9, 1.0), hash31(cell + 5.0));
    return tint * smoothstep(0.2, 0.0, d) * (0.1 + 1.8 * mag * mag) * night * smoothstep(0.0, 0.3, dir.y);
}

fn cloud_fbm(p_in: vec2<f32>) -> f32 {
    var s = 0.0;
    var a = 0.5;
    var p = p_in;
    for (var i = 0; i < 5; i = i + 1) {
        s = s + a * value_noise(p);
        p = p * 2.03 + vec2<f32>(1.7, 9.2);
        a = a * 0.5;
    }
    return s;
}

// A layer of clouds on a plane high above the viewer: rgb colour, a coverage.
fn cloud_layer(dir: vec3<f32>) -> vec4<f32> {
    let cover = frame.p5.x;
    if (cover <= 0.001 || dir.y < 0.012) {
        return vec4<f32>(0.0);
    }
    let p = dir.xz / (dir.y + 0.2) * 1.5;
    let n = cloud_fbm(p);
    let d = smoothstep(0.66 - cover * 0.4, 0.9, n);
    // the side facing the sun is bright, the far side grey
    let toward = cloud_fbm(p + frame.sun_dir.xz * 0.12);
    let lit = clamp(0.55 + (n - toward) * 5.0, 0.0, 1.0);
    let bright = frame.sun_color.rgb * 0.35 + frame.sky_horizon.rgb * 0.75 + vec3<f32>(0.12);
    let dark = frame.sky_top.rgb * 0.35 + frame.sky_horizon.rgb * 0.3 + frame.ground.rgb * 0.15;
    let fade = smoothstep(0.012, 0.25, dir.y);
    return vec4<f32>(mix(dark, bright, lit), d * fade);
}

fn sky_color(dir: vec3<f32>, with_sun: bool) -> vec3<f32> {
    let t = dir.y;
    var c: vec3<f32>;
    if (t >= 0.0) {
        c = mix(frame.sky_horizon.rgb, frame.sky_top.rgb, pow(clamp(t, 0.0, 1.0), 0.36));
    } else {
        c = mix(frame.sky_horizon.rgb, frame.ground.rgb, clamp(-t * 6.0, 0.0, 1.0));
    }
    if (with_sun && frame.sun_dir.w > 0.5 && t > -0.02) {
        let cs = max(dot(dir, frame.sun_dir.xyz), 0.0);
        let disc = smoothstep(0.99900, 0.99935, cs);
        let glow = pow(cs, 16.0) * 0.10 + pow(cs, 300.0) * 0.5;
        c = c + frame.sun_color.rgb * (disc * 14.0 + glow);
    }
    if (with_sun) {
        c = c + stars(dir);
        let cl = cloud_layer(dir);
        c = mix(c, cl.rgb, cl.a * 0.92);
    }
    return c;
}

// The light the sky sends to a surface that faces `n`: the lower hemisphere is the ground, the upper one the sky.
fn sky_irradiance(n: vec3<f32>) -> vec3<f32> {
    let up = mix(frame.sky_horizon.rgb, frame.sky_top.rgb, 0.5);
    return mix(frame.ground.rgb, up, smoothstep(-0.6, 0.8, n.y)) * frame.p0.x;
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
    var sum = 0.0;
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            sum = sum + textureSampleCompareLevel(shadow_map, shadow_samp, uv + vec2<f32>(f32(dx), f32(dy)) * ts, c, depth);
        }
    }
    return sum / 9.0;
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

// Karis' analytic fit of the split-sum environment BRDF: the scale and bias applied to the specular colour.
fn env_brdf(f0: vec3<f32>, rough: f32, nv: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = rough * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * nv)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + vec3<f32>(ab.y);
}

fn specular_occlusion(nv: f32, ao: f32, rough: f32) -> f32 {
    return clamp(pow(nv + ao, exp2(-16.0 * rough - 1.0)) - 1.0 + ao, 0.0, 1.0);
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
    let tuv = vec2<f32>(uvs.x, 1.0 - uvs.y);
    let t = textureSample(tex, tex_samp, tuv);
    // the relief of the built-in materials: the height map's slope, as seen from this pixel, tilts the normal
    let relief = textureSample(height_tex, tex_samp, tuv).r * o.uvrect.x;
    let dpx = dpdx(in.world);
    let dpy = dpdy(in.world);
    let dhx = dpdx(relief);
    let dhy = dpdy(relief);
    let classic = frame.p2.x < 0.5;
    if (!classic && o.uvrect.x > 0.0) {
        n = bump_normal(n, dpx, dpy, dhx, dhy);
    }
    if (o.misc.z > 0.5 && in.tw > 0.001) {
        if (in.tw > 0.999 && t.a < 0.5) {
            discard;
        }
        // classic textures hold display colours (squared into the working space); the high-quality ones are sRGB textures
        let lin = select(t.rgb, t.rgb * t.rgb, classic);
        base = base * (vec3<f32>(1.0) + in.tw * (lin - vec3<f32>(1.0)));
    }
    return lit_tail(in, o, base, n, classic);
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
    let ao = textureSampleLevel(ao_tex, lin_samp, uv, 0.0).r;
    var shadow = 1.0;
    if (i32(frame.p4.x) >= 0) {
        shadow = shadow_factor(in.world, n);
    }
    let pbr = shade_pbr(base, o.mat.z, o.mat.w, in.world, n, v, ao, shadow);
    let fog = fog_amount(in.world);
    // glass is more mirror-like at grazing angles
    let nv = clamp(dot(n, v), 0.0, 1.0);
    let alpha = o.color.a + (1.0 - o.color.a) * pow(1.0 - nv, 5.0);
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

// How much of the way to `world` is fog: the uniform fog of the Environment plus the height fog (a density that thins out upwards, summed exactly along the ray).
fn fog_amount(world: vec3<f32>) -> f32 {
    let to = world - frame.eye.xyz;
    let dist = length(to);
    var depth = 0.0;
    if (frame.fog.w > 0.0) {
        depth = frame.fog.w * dist;
    }
    if (frame.p13.x > 0.0) {
        let k = max(frame.p13.y, 0.0001);
        let a = clamp(k * to.y, -20.0, 20.0);
        var avg = exp(min(-k * (frame.eye.y - frame.p13.z), 20.0));
        if (abs(a) > 0.0001) {
            avg = avg * (1.0 - exp(-a)) / a;
        }
        depth = depth + frame.p13.x * dist * avg;
    }
    return clamp(1.0 - exp(-depth), 0.0, 1.0);
}

@fragment
fn fs_mesh(in: MeshOut) -> @location(0) vec4<f32> {
    let l = lit_mesh(in);
    let c = (l.diffuse + l.spec) * (1.0 - l.fog) + frame.fog.rgb * l.fog;
    return vec4<f32>(min(c, vec3<f32>(60000.0)), 1.0);
}

// A painted terrain: always opaque.
@fragment
fn fs_mesh_splat(in: MeshOut) -> @location(0) vec4<f32> {
    let l = lit_splat(in);
    let c = (l.diffuse + l.spec) * (1.0 - l.fog) + frame.fog.rgb * l.fog;
    return vec4<f32>(min(c, vec3<f32>(60000.0)), 1.0);
}

// A see-through mesh: premultiplied alpha, the glints are not dimmed by the transparency.
@fragment
fn fs_mesh_blend(in: MeshOut) -> @location(0) vec4<f32> {
    let l = lit_mesh(in);
    let body = (l.diffuse * (1.0 - l.fog) + frame.fog.rgb * l.fog) * l.alpha;
    let c = body + l.spec * (1.0 - l.fog);
    return vec4<f32>(min(c, vec3<f32>(60000.0)), l.alpha);
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
