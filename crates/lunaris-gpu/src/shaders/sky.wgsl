// The parts of the picture's light that only need the frame uniform, shared by the mesh shader (scene.wgsl) and the full-screen passes (post.wgsl):
// the procedural sky and its clouds and stars, the light the sky sends to a surface, the split-sum environment BRDF and the fog along a ray.
// They were part of scene.wgsl; the screen-space reflections need them in the post-processing pass too.

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

// ---- the sky of the air (`Atmosphere`): Nishita's single scattering, summed along the view ray in 16 steps with 6 towards the sun at each. The same sums as `lunaris_render::atmosphere` (kilometres).
const R_EARTH: f32 = 6360.0;
const R_TOP: f32 = 6420.0;

fn far_hit(o: vec3<f32>, d: vec3<f32>, r: f32) -> f32 {
    let b = dot(o, d);
    let c = dot(o, o) - r * r;
    let disc = b * b - c;
    if (disc < 0.0) {
        return -1.0;
    }
    return -b + sqrt(disc);
}

fn atmosphere_sky(dir: vec3<f32>) -> vec3<f32> {
    let beta_r = vec3<f32>(0.0058, 0.0135, 0.0331);
    let beta_m = 0.021;
    let turb = frame.p25.x * 0.5;
    let ray = frame.p25.y;
    let g = frame.p25.z;
    let sun = normalize(frame.sun_dir.xyz);
    let h0 = clamp(frame.eye.y * 0.001, 0.0, 5.0);
    let o = vec3<f32>(0.0, R_EARTH + h0 + 0.0005, 0.0);
    let d = normalize(vec3<f32>(dir.x, max(dir.y, 0.0), dir.z));
    let t_max = far_hit(o, d, R_TOP);
    if (t_max <= 0.0) {
        return vec3<f32>(0.0);
    }
    let ds = t_max / 16.0;
    var od_r = 0.0;
    var od_m = 0.0;
    var sum_r = vec3<f32>(0.0);
    var sum_m = vec3<f32>(0.0);
    for (var i = 0; i < 16; i = i + 1) {
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
        let dl = tl / 6.0;
        var odr_l = 0.0;
        var odm_l = 0.0;
        var shadow = false;
        for (var j = 0; j < 6; j = j + 1) {
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
    let cosg = dot(d, sun);
    let ph_r = 3.0 / (16.0 * PI) * (1.0 + cosg * cosg);
    let ph_m = 3.0 / (8.0 * PI) * ((1.0 - g * g) * (1.0 + cosg * cosg)) / ((2.0 + g * g) * pow(max(1.0 + g * g - 2.0 * g * cosg, 1.0e-4), 1.5));
    return 20.0 * frame.p25.w * (sum_r * beta_r * ph_r + sum_m * beta_m * ph_m);
}

// The colour fog has looking along `dir`: its own colour, and (with the sky of the air) brighter towards the sun, the haze that glows round it.
fn fog_color_toward(dir: vec3<f32>) -> vec3<f32> {
    if (frame.p26.x < 0.5) {
        return frame.fog.rgb;
    }
    let cs = max(dot(dir, normalize(frame.sun_dir.xyz)), 0.0);
    return frame.fog.rgb * (1.0 + frame.p26.y * pow(cs, 6.0)) + frame.sun_color.rgb * (0.04 * frame.p26.y * pow(cs, 24.0));
}

fn sky_color(dir: vec3<f32>, with_sun: bool) -> vec3<f32> {
    let t = dir.y;
    var c: vec3<f32>;
    if (frame.p26.x > 0.5 && with_sun) {
        // the sky of the air, and below the horizon the ground
        c = atmosphere_sky(dir);
        if (t < 0.0) {
            c = mix(c, frame.ground.rgb, clamp(-t * 6.0, 0.0, 1.0));
        }
    } else if (t >= 0.0) {
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


// ------------------------------------------------------------------------------------------------ reflection probes

// A probe's cube is six layers of the array `probe_tex` (the layers of probe `slot` start at 6 * slot: +X, -X, +Y, -Y, +Z, -Z), each drawn with a 94 degree view whose up is +Y (-Z for the +Y face, +Z for the -Y
// face): the same faces as the point lights' shadow maps. This is the light the probe saw in the direction `d` (any length, the probe's own middle is the origin), blurred by the mip level `lod`.
fn probe_sample(slot: i32, d: vec3<f32>, lod: f32) -> vec3<f32> {
    let a = abs(d);
    var face = 4;
    var fwd = vec3<f32>(0.0, 0.0, 1.0);
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if (a.x >= a.y && a.x >= a.z) {
        if (d.x > 0.0) {
            face = 0;
            fwd = vec3<f32>(1.0, 0.0, 0.0);
        } else {
            face = 1;
            fwd = vec3<f32>(-1.0, 0.0, 0.0);
        }
    } else if (a.y >= a.z) {
        if (d.y > 0.0) {
            face = 2;
            fwd = vec3<f32>(0.0, 1.0, 0.0);
            up = vec3<f32>(0.0, 0.0, -1.0);
        } else {
            face = 3;
            fwd = vec3<f32>(0.0, -1.0, 0.0);
            up = vec3<f32>(0.0, 0.0, 1.0);
        }
    } else if (d.z < 0.0) {
        face = 5;
        fwd = vec3<f32>(0.0, 0.0, -1.0);
    }
    let right = cross(fwd, up);
    let depth = max(dot(d, fwd), 1.0e-5);
    // 94 degrees: tan(47 degrees) = 1.0723687
    let k = 0.5 / 1.0723687;
    let uv = vec2<f32>(0.5 + dot(d, right) / depth * k, 0.5 - dot(d, up) / depth * k);
    return textureSampleLevel(probe_tex, probe_samp, uv, slot * 6 + face, lod).rgb;
}

struct ProbeLight {
    // what the probe saw in the mirror direction, and (blurred to nothing) around the normal
    env: vec3<f32>,
    irr: vec3<f32>,
    // how far to trust it: 0 outside every probe's box, 1 well inside one
    w: f32,
};

// The probe whose box holds `world` (the smallest of those that do) and what it makes of a surface there with the normal `n`, the mirror direction `r` and the roughness `rough`. The reflection is bent
// to come from where the ray leaves the box (box projection), so the walls of a room are where they are.
fn probe_light(world: vec3<f32>, n: vec3<f32>, r: vec3<f32>, rough: f32) -> ProbeLight {
    var res: ProbeLight;
    res.env = vec3<f32>(0.0);
    res.irr = vec3<f32>(0.0);
    res.w = 0.0;
    var best = -1;
    var best_volume = 1.0e30;
    let count = i32(frame.p18.x);
    for (var i = 0; i < 4; i = i + 1) {
        if (i >= count) {
            break;
        }
        let half_box = frame.probes[i * 2 + 1].xyz;
        let q = abs(world - frame.probes[i * 2].xyz) / half_box;
        if (max(q.x, max(q.y, q.z)) < 1.0) {
            let volume = half_box.x * half_box.y * half_box.z;
            if (volume < best_volume) {
                best_volume = volume;
                best = i;
            }
        }
    }
    if (best < 0) {
        return res;
    }
    let mid = frame.probes[best * 2].xyz;
    let half_size = frame.probes[best * 2 + 1].xyz;
    let q = abs(world - mid) / half_size;
    res.w = 1.0 - smoothstep(0.66, 1.0, max(q.x, max(q.y, q.z)));
    let dir = select(r, vec3<f32>(1.0e-6), abs(r) < vec3<f32>(1.0e-6));
    let t1 = (mid - half_size - world) / dir;
    let t2 = (mid + half_size - world) / dir;
    let far = max(t1, t2);
    let t = max(min(far.x, min(far.y, far.z)), 0.0);
    let levels = frame.p18.y;
    res.env = probe_sample(best, world + r * t - mid, clamp(rough * (levels - 1.0), 0.0, levels - 1.0));
    res.irr = probe_sample(best, n, max(levels - 3.0, 0.0));
    return res;
}
