// Particle emitters simulated on the card (docs/FX.md, `"simulate": "gpu"`). Two entry points, both over the emitter's ring of slots:
//
//   cs_step  one step of 1/60 s: the slots the processor says are born in this step get a new particle, the others that are alive move (gravity, drag, wind, curl noise, vortices, attractors, a speed limit, planes);
//   cs_emit  once a picture: every live particle becomes four vertices of a camera-facing, velocity-stretched or flat picture in the vertex layout the particle cloud pipelines draw (position, normal, uv, colour, opacity),
//            a dead one four vertices at one place (the rasteriser drops the empty triangles).
//
// A particle is only where it is, how fast it goes, how old it is, how long it lives and its serial number: its size, colour jitter, spin and flipbook frame are hashes of the serial, so they cost no memory and the same
// slot gives the same particle however often a picture is drawn. The processor decides every birth (rate, bursts, delay) and hands over the first slot, the count and the first serial of each step.

struct Fx {
    model: mat4x4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    cam_fwd: vec4<f32>,
    cam_pos: vec4<f32>,
    // capacity, first slot born, how many born, serial of the first
    ints0: vec4<u32>,
    // seed, flags (1 = the entity's own space, 2 and 4: facing, 0 camera, 1 velocity, 2 axis; 8 = noise), columns, rows of the flipbook
    ints1: vec4<u32>,
    // frames, frame mode (0 over the life, 1 random, 2 at a rate), vortices, attractors
    ints2: vec4<u32>,
    // planes, hit mode (0 bounce, 1 kill, 2 stick)
    ints3: vec4<u32>,
    // step, the clock, life least and most
    times: vec4<f32>,
    // speed least and most, cosine of the spread, radial
    speed: vec4<f32>,
    // the way they fly, the shape (0 point, 1 sphere, 2 box, 3 disc, 4 line)
    dir: vec4<f32>,
    // radius or half sizes; w: shell or ring
    shape_a: vec4<f32>,
    // a line's far end
    shape_b: vec4<f32>,
    // size least and most, aspect, the effect's scale
    size: vec4<f32>,
    color: vec4<f32>,
    tint: vec4<f32>,
    // colour jitter, stretch, frames a second
    misc: vec4<f32>,
    // rotation least and most (radians), spin least and most (radians a second)
    rot: vec4<f32>,
    // gravity, drag
    grav: vec4<f32>,
    // wind, speed limit
    wind: vec4<f32>,
    // strength, size of a swirl, speed, on
    noise: vec4<f32>,
    axisf: vec4<f32>,
    // two vortices, three vectors each: (place, strength) (axis, pull) (radius)
    vort: array<vec4<f32>, 6>,
    // two attractors, two each: (place, strength) (radius, falloff, kill)
    att: array<vec4<f32>, 4>,
    // two planes: (normal, d)
    planes: array<vec4<f32>, 2>,
    // bounce, friction, life lost at a hit
    coll: vec4<f32>,
    spare: vec4<f32>,
};

struct P {
    pos: vec3<f32>,
    age: f32,
    vel: vec3<f32>,
    life: f32,
    serial: u32,
    a: u32,
    b: u32,
    c: u32,
};

@group(0) @binding(0) var<uniform> fx: Fx;
@group(0) @binding(1) var<storage, read_write> state: array<P>;
// 32 samples of (size, opacity, speed, light) over the life, then 32 of the colour
@group(0) @binding(2) var<storage, read> lut: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> out: array<f32>;

const TAU: f32 = 6.28318530718;
const DT: f32 = 0.016666668;

fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

// the k-th random number of a particle, in [0, 1)
fn rnd(serial: u32, k: u32) -> f32 {
    let h = pcg(pcg(serial ^ (fx.ints1.x * 2654435761u)) + k * 2246822519u);
    return f32(h >> 8u) / 16777216.0;
}

fn unit_dir(serial: u32, k: u32) -> vec3<f32> {
    let z = 1.0 - 2.0 * rnd(serial, k);
    let a = TAU * rnd(serial, k + 1u);
    let s = sqrt(max(0.0, 1.0 - z * z));
    return vec3<f32>(s * cos(a), z, s * sin(a));
}

// the three random numbers of a corner of the noise lattice, from one hash (ten bits each)
fn corner(x: i32, y: i32, z: i32) -> vec3<f32> {
    var h = ((bitcast<u32>(x) * 0x8da6b343u) ^ (bitcast<u32>(y) * 0xd8163841u)) ^ (bitcast<u32>(z) * 0xcb1ab31fu);
    h = h ^ (h >> 15u);
    h = h * 0x2c1b3c6du;
    h = h ^ (h >> 12u);
    h = h * 0x297a2d39u;
    h = h ^ (h >> 15u);
    return vec3<f32>(f32(h & 1023u), f32((h >> 10u) & 1023u), f32((h >> 20u) & 1023u)) / 1023.0;
}

// a swirl that never piles particles up: the curl of three smooth noise potentials, which are the three numbers of each corner of the lattice, blended together over the same eight corners
fn curl(p: vec3<f32>) -> vec3<f32> {
    let ip = floor(p);
    let f = p - ip;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let du = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    let xi = i32(ip.x);
    let yi = i32(ip.y);
    let zi = i32(ip.z);
    let c0 = corner(xi, yi, zi);
    let c1 = corner(xi + 1, yi, zi);
    let c2 = corner(xi, yi + 1, zi);
    let c3 = corner(xi + 1, yi + 1, zi);
    let c4 = corner(xi, yi, zi + 1);
    let c5 = corner(xi + 1, yi, zi + 1);
    let c6 = corner(xi, yi + 1, zi + 1);
    let c7 = corner(xi + 1, yi + 1, zi + 1);
    let k1 = c1 - c0;
    let k2 = c2 - c0;
    let k3 = c4 - c0;
    let k4 = c0 - c1 - c2 + c3;
    let k5 = c0 - c2 - c4 + c6;
    let k6 = c0 - c1 - c4 + c5;
    let k7 = -c0 + c1 + c2 - c3 + c4 - c5 - c6 + c7;
    // gx.k is the slope of potential k along x, and so on
    let gx = du.x * (k1 + k4 * u.y + k6 * u.z + k7 * (u.y * u.z));
    let gy = du.y * (k2 + k5 * u.z + k4 * u.x + k7 * (u.z * u.x));
    let gz = du.z * (k3 + k6 * u.x + k5 * u.y + k7 * (u.x * u.y));
    return vec3<f32>(gy.z - gz.y, gz.x - gx.z, gx.y - gy.x);
}

fn lut_at(base: u32, u: f32) -> vec4<f32> {
    let f = clamp(u, 0.0, 1.0) * 31.0;
    let i0 = u32(f);
    let i1 = min(i0 + 1u, 31u);
    return mix(lut[base + i0], lut[base + i1], f - f32(i0));
}

fn born(serial: u32, rel: u32, count: u32) -> P {
    var local = vec3<f32>(0.0);
    var outward = vec3<f32>(0.0, 1.0, 0.0);
    let kind = u32(fx.dir.w + 0.5);
    if (kind == 1u) {
        let d = unit_dir(serial, 0u);
        var r = fx.shape_a.x;
        if (fx.shape_a.w < 0.5) {
            r = r * pow(rnd(serial, 2u), 0.33333334);
        }
        local = d * r;
        outward = d;
    } else if (kind == 2u) {
        let q = vec3<f32>(rnd(serial, 3u) * 2.0 - 1.0, rnd(serial, 4u) * 2.0 - 1.0, rnd(serial, 5u) * 2.0 - 1.0);
        local = q * fx.shape_a.xyz;
        outward = normalize(q + vec3<f32>(0.0001));
    } else if (kind == 3u) {
        let a = TAU * rnd(serial, 6u);
        var rr = fx.shape_a.x;
        if (fx.shape_a.w < 0.5) {
            rr = rr * sqrt(rnd(serial, 7u));
        }
        let d = vec3<f32>(cos(a), 0.0, sin(a));
        local = d * rr;
        outward = d;
    } else if (kind == 4u) {
        local = fx.shape_b.xyz * rnd(serial, 8u);
        outward = normalize(fx.shape_b.xyz + vec3<f32>(0.000001));
    }
    let axis = fx.dir.xyz;
    var dir = axis;
    let cs = fx.speed.z;
    if (cs <= -0.9999) {
        dir = unit_dir(serial, 9u);
    } else if (cs < 0.9999) {
        let cz = 1.0 - rnd(serial, 11u) * (1.0 - cs);
        let sz = sqrt(max(0.0, 1.0 - cz * cz));
        let a = TAU * rnd(serial, 12u);
        var h = vec3<f32>(0.0, 1.0, 0.0);
        if (abs(axis.y) > 0.9) {
            h = vec3<f32>(1.0, 0.0, 0.0);
        }
        let t1 = normalize(cross(axis, h));
        let t2 = cross(axis, t1);
        dir = axis * cz + t1 * (sz * cos(a)) + t2 * (sz * sin(a));
    }
    let radial = fx.speed.w;
    if (radial > 0.0) {
        let m = dir * (1.0 - radial) + outward * radial;
        if (length(m) > 0.00001) {
            dir = normalize(m);
        }
    }
    var vel = dir * mix(fx.speed.x, fx.speed.y, rnd(serial, 13u));
    var pos = local;
    if ((fx.ints1.y & 1u) == 0u) {
        // the world's own space: born where the entity is, as big and as turned as it is
        pos = (fx.model * vec4<f32>(local, 1.0)).xyz;
        vel = (fx.model * vec4<f32>(vel, 0.0)).xyz;
    }
    // born somewhere inside the step: it has already flown for the rest of it
    let age0 = DT * (1.0 - (f32(rel) + 0.5) / f32(count));
    pos = pos + vel * age0;
    let life = mix(fx.times.z, fx.times.w, rnd(serial, 14u));
    return P(pos, age0, vel, life, serial, 0u, 0u, 0u);
}

fn moved(p0: P) -> P {
    var p = p0;
    let dt = fx.times.x;
    p.age = p.age + dt;
    if (p.age >= p.life) {
        p.life = 0.0;
        return p;
    }
    let u = clamp(p.age / p.life, 0.0, 1.0);
    var acc = fx.grav.xyz + fx.wind.xyz;
    if (fx.noise.w > 0.5) {
        let q = p.pos / fx.noise.y + vec3<f32>(0.31, 1.0, 0.17) * (fx.times.y * fx.noise.z);
        acc = acc + curl(q) * fx.noise.x;
    }
    for (var v = 0u; v < fx.ints2.z; v = v + 1u) {
        let c = fx.vort[v * 3u].xyz;
        let strength = fx.vort[v * 3u].w;
        let axis = fx.vort[v * 3u + 1u].xyz;
        let pull = fx.vort[v * 3u + 1u].w;
        let radius = fx.vort[v * 3u + 2u].x;
        let r = p.pos - c;
        let rp = r - axis * dot(r, axis);
        let dist = length(rp);
        if (dist > 0.0001 && dist < radius) {
            let w = 1.0 - dist / radius;
            acc = acc + cross(axis, rp) * (strength * w / dist) + rp * (-pull * w / dist);
        }
    }
    for (var a = 0u; a < fx.ints2.w; a = a + 1u) {
        let c = fx.att[a * 2u].xyz;
        let strength = fx.att[a * 2u].w;
        let radius = fx.att[a * 2u + 1u].x;
        let falloff = fx.att[a * 2u + 1u].y;
        let kill = fx.att[a * 2u + 1u].z;
        let d = c - p.pos;
        let dist = length(d);
        if (kill > 0.0 && dist < kill) {
            p.life = 0.0;
            return p;
        }
        if (dist > 0.0001 && dist < radius) {
            var w = 1.0;
            if (falloff > 0.0) {
                w = pow(max(1.0 - dist / radius, 0.0), falloff);
            }
            acc = acc + d * (strength * w / dist);
        }
    }
    p.vel = p.vel + acc * dt;
    if (fx.grav.w > 0.0) {
        p.vel = p.vel * exp(-fx.grav.w * dt);
    }
    if (fx.wind.w > 0.0) {
        let s = length(p.vel);
        if (s > fx.wind.w) {
            p.vel = p.vel * (fx.wind.w / s);
        }
    }
    p.pos = p.pos + p.vel * (lut_at(0u, u).z * dt);
    for (var k = 0u; k < fx.ints3.x; k = k + 1u) {
        let pl = fx.planes[k];
        let depth = pl.w - dot(pl.xyz, p.pos);
        if (depth > 0.0) {
            if (fx.ints3.y == 1u) {
                p.life = 0.0;
                return p;
            }
            p.pos = p.pos + pl.xyz * depth;
            if (fx.ints3.y == 2u) {
                p.vel = vec3<f32>(0.0);
            } else {
                let vn = dot(p.vel, pl.xyz);
                if (vn < 0.0) {
                    let along = pl.xyz * vn;
                    p.vel = (p.vel - along) * (1.0 - fx.coll.y) - along * fx.coll.x;
                }
            }
            p.age = p.age + p.life * fx.coll.z;
        }
    }
    return p;
}

@compute @workgroup_size(64)
fn cs_step(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let cap = fx.ints0.x;
    if (i >= cap) {
        return;
    }
    let first = fx.ints0.y;
    let count = fx.ints0.z;
    let rel = (i + cap - first) % cap;
    if (count > 0u && rel < count) {
        state[i] = born(fx.ints0.w + rel, rel, count);
        return;
    }
    let p = state[i];
    if (p.life <= 0.0) {
        return;
    }
    state[i] = moved(p);
}

fn put(o: u32, pos: vec3<f32>, nrm: vec3<f32>, uv: vec2<f32>, col: vec3<f32>, tw: f32) {
    out[o] = pos.x;
    out[o + 1u] = pos.y;
    out[o + 2u] = pos.z;
    out[o + 3u] = nrm.x;
    out[o + 4u] = nrm.y;
    out[o + 5u] = nrm.z;
    out[o + 6u] = uv.x;
    out[o + 7u] = uv.y;
    out[o + 8u] = col.x;
    out[o + 9u] = col.y;
    out[o + 10u] = col.z;
    out[o + 11u] = tw;
}

@compute @workgroup_size(64)
fn cs_emit(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let cap = fx.ints0.x;
    if (i >= cap) {
        return;
    }
    let p = state[i];
    let o = i * 48u;
    if (p.life <= 0.0) {
        for (var j = 0u; j < 4u; j = j + 1u) {
            put(o + j * 12u, vec3<f32>(0.0), vec3<f32>(0.0), vec2<f32>(0.0), vec3<f32>(0.0), 0.0);
        }
        return;
    }
    let local = (fx.ints1.y & 1u) != 0u;
    let u = clamp(p.age / p.life, 0.0, 1.0);
    let s = lut_at(0u, u);
    let size = mix(fx.size.x, fx.size.y, rnd(p.serial, 15u)) * s.x * fx.size.w;
    let jit = 1.0 + (rnd(p.serial, 19u) * 2.0 - 1.0) * fx.misc.x;
    let cm = lut_at(32u, u);
    let rgb_d = max(fx.color.rgb * jit * cm.rgb * fx.tint.rgb, vec3<f32>(0.0));
    let alpha = clamp(fx.color.a * cm.a * fx.tint.a * s.y, 0.0, 1.0);
    if (alpha <= 0.002 || abs(size) < 0.000001) {
        for (var j = 0u; j < 4u; j = j + 1u) {
            put(o + j * 12u, vec3<f32>(0.0), vec3<f32>(0.0), vec2<f32>(0.0), vec3<f32>(0.0), 0.0);
        }
        return;
    }
    let rgb = rgb_d * rgb_d;
    var pos = p.pos;
    var vel = p.vel;
    if (local) {
        pos = (fx.model * vec4<f32>(p.pos, 1.0)).xyz;
        vel = (fx.model * vec4<f32>(p.vel, 0.0)).xyz;
    }
    // the flipbook frame
    var frame = 0u;
    let frames = max(fx.ints2.x, 1u);
    if (fx.ints2.y == 0u) {
        frame = min(u32(u * f32(frames)), frames - 1u);
    } else if (fx.ints2.y == 1u) {
        frame = u32(rnd(p.serial, 16u) * f32(frames)) % frames;
    } else {
        frame = u32(p.age * fx.misc.z) % frames;
    }
    let cols = max(fx.ints1.z, 1u);
    let rows = max(fx.ints1.w, 1u);
    let du = 1.0 / f32(cols);
    let dv = 1.0 / f32(rows);
    let u0 = f32(frame % cols) * du;
    let v1 = 1.0 - f32(frame / cols) * dv;
    // the picture's own axes
    let hx = abs(size) * 0.5;
    var hy = hx * fx.size.z;
    let angle = mix(fx.rot.x, fx.rot.y, rnd(p.serial, 17u)) + mix(fx.rot.z, fx.rot.w, rnd(p.serial, 18u)) * p.age;
    let c = cos(angle);
    let sn = sin(angle);
    let facing = (fx.ints1.y >> 1u) & 3u;
    var right = fx.cam_right.xyz;
    var up = fx.cam_up.xyz;
    var normal = -fx.cam_fwd.xyz;
    if (facing == 0u) {
        right = fx.cam_right.xyz * c + fx.cam_up.xyz * sn;
        up = fx.cam_up.xyz * c - fx.cam_right.xyz * sn;
    } else if (facing == 1u) {
        let speed = length(vel);
        if (speed > 0.0001) {
            let along = vel / speed;
            var across = cross(along, fx.cam_fwd.xyz);
            if (length(across) < 0.00001) {
                across = fx.cam_right.xyz;
            } else {
                across = normalize(across);
            }
            right = across;
            up = along;
            hy = hy + speed * fx.misc.y * 0.5;
        }
    } else {
        let axis = fx.axisf.xyz;
        var h = vec3<f32>(0.0, 1.0, 0.0);
        if (abs(axis.y) > 0.9) {
            h = vec3<f32>(1.0, 0.0, 0.0);
        }
        let t1 = normalize(cross(axis, h));
        let t2 = cross(axis, t1);
        right = t1 * c + t2 * sn;
        up = t2 * c - t1 * sn;
        normal = axis;
        if (dot(axis, fx.cam_pos.xyz - pos) < 0.0) {
            normal = -axis;
        }
    }
    var sx = array<f32, 4>(-1.0, 1.0, 1.0, -1.0);
    var sy = array<f32, 4>(-1.0, -1.0, 1.0, 1.0);
    var uu = array<f32, 4>(u0, u0 + du, u0 + du, u0);
    var vv = array<f32, 4>(v1 - dv, v1 - dv, v1, v1);
    for (var j = 0u; j < 4u; j = j + 1u) {
        let corner = pos + right * (sx[j] * hx) + up * (sy[j] * hy);
        put(o + j * 12u, corner, normal, vec2<f32>(uu[j], vv[j]), rgb, alpha);
    }
}
