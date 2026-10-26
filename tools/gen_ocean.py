"""The sea's waves: a table sampled from a real ocean spectrum, for the shader (`ocean_*` in scene.wgsl) and its copy in the simulation (floaters.rs), which must agree to the digit.

    python3 tools/gen_ocean.py            # rewrites the block between the OCEAN BEGIN / OCEAN END markers in both files

The spectrum is JONSWAP (Hasselmann et al. 1973: peak enhancement 3.3, sigma 0.07 below and 0.09 above the peak) with the directional spreading of Hasselmann et al. 1980 / Mitsuyasu
(cos^(2s) of half the angle off the wind, s = 11.5 x^5 below the peak and 11.5 x^-2.5 above it, x = omega / omega_p): the long waves run in a narrow band of directions, the short ones go every way.
The amplitudes are found by integrating the spectrum over a bin of frequency, so that the surface has the variance the spectrum says (significant height Hs = 4 sqrt(m0)); the table is for Hs = 1 and
peak wave number 1, and the shader scales it with the real Hs and the real peak (k = kp m, a = Hs A). Deep water: omega^2 = g k (the short waves of the fine set also get surface tension,
omega^2 = g k (1 + k^2/km^2), km = 363 rad/m). One wave per bin, its direction drawn from the spreading by a low-discrepancy sequence and its phase at random (a fixed seed): the long ones move the mesh and the
bodies that float (the "vertex set": the peak of the spectrum is covered by a dozen, with Rayleigh amplitudes, so there is no lattice), and the finer ones, down to a few millimetres, only tilt the normal (the "fine set").
"""
import math
import random
import re

GAMMA = 3.3
SP = 8.0
WIND = 0.6435  # radians, the direction the waves run (as before)
V_EDGES = [0.62, 0.70, 0.76, 0.81, 0.855, 0.895, 0.93, 0.965, 0.995, 1.025, 1.055, 1.09, 1.125, 1.165, 1.21, 1.26, 1.32, 1.39, 1.47, 1.57, 1.69, 1.83, 2.00, 2.20, 2.45, 2.70]
NF = 24
# the domain warp: the waves are evaluated at a point bent by two slow sines (directions, wavelengths in peak lengths, phases), 0.2 of a peak length at most
WARP_AMP = 0.2
WARP = [((0.83, -0.557), 5.3, 1.3), ((0.37, 0.929), 3.9, 4.1)]
F_TOP = 34.6


def shape(x):
    sigma = 0.07 if x <= 1.0 else 0.09
    r = math.exp(-((x - 1.0) ** 2) / (2.0 * sigma * sigma))
    return x ** -5 * math.exp(-1.25 * x ** -4) * GAMMA ** r


def integrate(a, b, n=400):
    h = (b - a) / n
    s = 0.5 * (shape(a) + shape(b))
    for i in range(1, n):
        s += shape(a + i * h)
    return s * h


I_TOTAL = integrate(0.25, 80.0, 60000)


def spread_s(x):
    s = SP * x ** 5 if x < 1.0 else SP * x ** -2.5
    return max(s, 1.0)


def quantile(s, u):
    n = 1440
    ds = [(-math.pi + 2 * math.pi * i / n) for i in range(n + 1)]
    w = [max(math.cos(d / 2.0), 0.0) ** (2.0 * s) for d in ds]
    cdf = [0.0]
    for i in range(1, n + 1):
        cdf.append(cdf[-1] + 0.5 * (w[i] + w[i - 1]))
    tot = cdf[-1]
    t = u * tot
    for i in range(1, n + 1):
        if cdf[i] >= t:
            f = (t - cdf[i - 1]) / max(cdf[i] - cdf[i - 1], 1e-30)
            return ds[i - 1] + f * (ds[i] - ds[i - 1])
    return 0.0


bins = [(V_EDGES[i], V_EDGES[i + 1]) for i in range(len(V_EDGES) - 1)]
ratio = (F_TOP / V_EDGES[-1]) ** (1.0 / NF)
fbins = [(V_EDGES[-1] * ratio ** i, V_EDGES[-1] * ratio ** (i + 1)) for i in range(NF)]


def comps(rng, bs, start):
    """One wave per bin. The amplitudes of the real sea's components are Rayleigh-distributed (the FFT oceans draw them from a Gaussian): the bin's energy times a draw with mean 1, then the set is scaled
    back to exactly the energy the spectrum says. Equal amplitudes would make the waves at the peak beat in a regular lattice."""
    out = []
    want = 0.0
    for i, (lo, hi) in enumerate(bs):
        xc = math.sqrt(lo * hi)
        var = (1.0 / 16.0) * integrate(lo, hi) / I_TOTAL
        want += var
        a = math.sqrt(2.0 * var) * math.sqrt(-math.log(max(rng.random(), 1e-9)))
        u = (0.5 + (start + i) * 0.6180339887498949) % 1.0
        ang = WIND + quantile(spread_s(xc), u)
        out.append([math.cos(ang), math.sin(ang), xc * xc, a, rng.uniform(0.0, 2.0 * math.pi)])
    have = sum(c[3] ** 2 / 2 for c in out)
    k = math.sqrt(want / have)
    for c in out:
        c[3] *= k
    return [tuple(c) for c in out]


# the first seed whose draw has no wave that owns the sea (the peak is covered by many, none above a sixth of the energy) and whose dominant waves do not all run the same way
seed = 20261016
while True:
    rng = random.Random(seed)
    V = comps(rng, bins, 0)
    F = comps(rng, fbins, len(bins))
    e = sorted((c[3] ** 2 for c in V), reverse=True)
    share = e[0] / sum(e)
    top = [c for c in sorted(V, key=lambda c: -c[3])[:5]]
    spread = max(math.atan2(c[1], c[0]) for c in top) - min(math.atan2(c[1], c[0]) for c in top)
    if share < 0.16 and spread > 0.35:
        break
    seed += 1
print("seed %d: strongest wave has %.3f of the energy, the five strongest span %.2f rad" % (seed, share, spread))
SK = sum(c[2] * c[3] for c in V)
AMP = sum(c[3] for c in V)
var_v = sum(c[3] ** 2 / 2 for c in V)
var_f = sum(c[3] ** 2 / 2 for c in F)
mss = sum(0.5 * (c[3] * c[2]) ** 2 for c in V + F)
print("variance vertex %.5f fine %.6f total %.5f (m0 of Hs=1 is %.5f)" % (var_v, var_f, var_v + var_f, 1 / 16))
print("sum A m (vertex) %.3f, sum A %.3f, mean square slope per Hs*kp squared %.4f" % (SK, AMP, mss))


def f(v):
    return "%.8g" % v


def wgsl_arrays():
    def vec4s(cs):
        return ", ".join("vec4<f32>(%s, %s, %s, %s)" % (f(c[0]), f(c[1]), f(c[2]), f(c[3])) for c in cs)

    def floats(cs):
        return ", ".join(f(c[4]) for c in cs)

    return (
        "var<private> OCEAN_VW: array<vec4<f32>, %d> = array<vec4<f32>, %d>(%s);\n" % (len(V), len(V), vec4s(V))
        + "var<private> OCEAN_VP: array<f32, %d> = array<f32, %d>(%s);\n" % (len(V), len(V), floats(V))
        + "var<private> OCEAN_FW: array<vec4<f32>, %d> = array<vec4<f32>, %d>(%s);\n" % (len(F), len(F), vec4s(F))
        + "var<private> OCEAN_FP: array<f32, %d> = array<f32, %d>(%s);\n" % (len(F), len(F), floats(F))
        + "const OCEAN_SK: f32 = %s;\n" % f(SK)
        + "const OCEAN_WARP_AMP: f32 = %s;\n" % f(WARP_AMP)
        + "".join("const OCEAN_W%dD: vec2<f32> = vec2<f32>(%s, %s);\nconst OCEAN_W%dK: f32 = %s;\nconst OCEAN_W%dP: f32 = %s;\n" % (i + 1, f(w[0][0]), f(w[0][1]), i + 1, f(2 * math.pi / w[1]), i + 1, f(w[2])) for i, w in enumerate(WARP))
    )


WGSL = """// OCEAN BEGIN (the table is made by tools/gen_ocean.py; edit the script, not the numbers)
// ---- water: the surface of the sea is a sum of waves drawn from a real ocean spectrum, not hand-picked sines. JONSWAP (Hasselmann et al. 1973) for how the energy falls with the frequency, Hasselmann/Mitsuyasu
// spreading (1980) for the directions (the long waves in a narrow band round the wind, the short ones every way), the amplitudes found by integrating the spectrum over a bin of frequency so that the surface has the
// variance the spectrum says: `height` is the significant wave height Hs (the mean of the highest third), `length` the wave length of the peak, the speed of every wave the one of deep water (omega = sqrt(g k): a short
// wave is slow, so the shape changes as it travels), and `phase` the clock in seconds. %(nv)d waves (from 2.6 peak lengths down to a seventh of it, crowded round the peak with amplitudes drawn from a Rayleigh distribution so that there is no lattice) move the mesh
// and the bodies that float on it (the same numbers are in `floaters.rs`); %(nf)d finer ones, down to a few millimetres (with surface tension in their speed), only tilt the normal. The wave lengths are not in
// simple ratios and the directions and phases are drawn, so the sum never comes round again. Each wave is a Gerstner wave (Tessendorf 2001, "Simulating Ocean Water"): the points of the surface also move sideways towards
// the crests (the "choppy" displacement), which sharpens the crests and flattens the troughs the way the real sea does, and where the displacement folds the surface over itself (the Jacobian of the mapping falls to zero)
// the waves are breaking: foam. x, z: the direction, y: the wave number as a multiple of the peak's, w: the amplitude as a multiple of Hs; the phase is in the second table.
%(arrays)s
// How much of the sideways pull the sea takes: all of it (a Gerstner wave at full steepness has a cusp), unless the sum of the slopes of all the waves would fold the surface everywhere (a huge sea): then less.
fn ocean_chop(height: f32, length: f32) -> f32 {
    let kp = 6.2831853 / max(length, 0.2);
    return min(1.0, 0.85 / max(height * kp * OCEAN_SK, 1.0e-4));
}

// The waves are not evaluated at p itself but at a point bent by two slow sines (a domain warp: the crest lines curve gently and the local wave length varies by a quarter or so over a few peak lengths), so that the few waves that
// carry most of the energy never cross in the regular egg-carton lattice that a handful of plane waves make. Its derivatives are needed for the normal (the chain rule), as a, b, c, d: dq_x/dx, dq_x/dz, dq_z/dx, dq_z/dz.
fn ocean_warp(p: vec2<f32>, length: f32) -> vec2<f32> {
    let f1 = dot(p, OCEAN_W1D) * OCEAN_W1K / length + OCEAN_W1P;
    let f2 = dot(p, OCEAN_W2D) * OCEAN_W2K / length + OCEAN_W2P;
    return p + OCEAN_WARP_AMP * length * vec2<f32>(sin(f1), sin(f2));
}

fn ocean_warp_jac(p: vec2<f32>, length: f32) -> vec4<f32> {
    let f1 = dot(p, OCEAN_W1D) * OCEAN_W1K / length + OCEAN_W1P;
    let f2 = dot(p, OCEAN_W2D) * OCEAN_W2K / length + OCEAN_W2P;
    let c1 = OCEAN_WARP_AMP * OCEAN_W1K * cos(f1);
    let c2 = OCEAN_WARP_AMP * OCEAN_W2K * cos(f2);
    return vec4<f32>(1.0 + c1 * OCEAN_W1D.x, c1 * OCEAN_W1D.y, c2 * OCEAN_W2D.x, 1.0 + c2 * OCEAN_W2D.y);
}

// The depth of the sea at one point of the window of it round the eye (`frame.sea`: 128 x 128 points four metres apart, made by the renderer from the terrains, as 16-bit floats two to a word; the uniform and not a texture because the
// painted terrain's pipeline already uses all sixteen textures a portable device has).
fn sea_point(i: u32, j: u32) -> f32 {
    let n = j * 128u + i;
    let word = n >> 1u;
    let pair = unpack2x16float(frame.sea[word >> 2u][word & 3u]);
    return select(pair.x, pair.y, (n & 1u) == 1u);
}

// The depth of the sea at p: the level minus the ground; outside the window, and when there is none, the sea is deep.
fn sea_depth(p: vec2<f32>) -> f32 {
    if (frame.p30.w < 0.5) {
        return 1000.0;
    }
    let u = (p - frame.p30.xy) * frame.p30.z;
    if (u.x < 0.0 || u.y < 0.0 || u.x >= 127.0 || u.y >= 127.0) {
        return 1000.0;
    }
    let i = vec2<u32>(floor(u));
    let f = u - floor(u);
    let a = sea_point(i.x, i.y);
    let b = sea_point(i.x + 1u, i.y);
    let c = sea_point(i.x, i.y + 1u);
    let d = sea_point(i.x + 1u, i.y + 1u);
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

// Waves in shallow water: the height of a wave grows as the depth to the power minus one quarter once the depth is less than half its length (Green's law: the energy flux is kept while the wave slows down), and the sea
// cannot keep a significant height of more than about half the depth: the waves break, and the height falls to nothing where the water does. x: the factor on the height of every wave, y: how near the sea is to breaking (the
// height it would have without the limit over the depth; foam from about a third).
fn ocean_shoal(p: vec2<f32>, height: f32, length: f32, level: f32) -> vec2<f32> {
    // (only water at the level of the sea whose depth the window holds: a lake higher up, or a river, has its own)
    if (abs(level - frame.p28.y) > 0.05) {
        return vec2<f32>(1.0, 0.0);
    }
    let d = max(sea_depth(p), 0.0);
    if (d >= 0.6 * length) {
        return vec2<f32>(1.0, 0.0);
    }
    let grow = pow(clamp(0.5 * length / max(d, 0.1), 1.0, 8.0), 0.25);
    let tried = height * grow;
    let hs = min(tried, 0.5 * d);
    return vec2<f32>(hs / max(height, 1.0e-4), tried / max(d, 0.05));
}

// Where the point of the surface that rests at p goes: x and z are how far the waves move it sideways, y its height. (The mesh's vertices are put there.)
fn ocean_disp(p: vec2<f32>, height: f32, length: f32, phase: f32, level: f32) -> vec3<f32> {
    let kp = 6.2831853 / max(length, 0.2);
    let lam = ocean_chop(height, length);
    let q = ocean_warp(p, length);
    let sh = ocean_shoal(p, height, length, level);
    var h = 0.0;
    var dx = 0.0;
    var dz = 0.0;
    for (var i = 0; i < %(nv)d; i = i + 1) {
        let w = OCEAN_VW[i];
        let k = kp * w.z;
        let a = height * sh.x * w.w;
        let th = k * dot(q, w.xy) - sqrt(9.81 * k) * phase + OCEAN_VP[i];
        let s = sin(th);
        h = h + a * cos(th);
        dx = dx - lam * a * w.x * s;
        dz = dz - lam * a * w.y * s;
    }
    return vec3<f32>(dx, h, dz);
}

// The normal of the surface at the point that rests at p, from all the waves (the %(nv)d that move the mesh worked out exactly with their sideways displacement: the normal of the displaced surface, and its Jacobian
// `g_jac`; the %(nf)d finer ones as slopes), each faded out when it is finer than a pixel `fw` metres across can show (it would only shimmer). What the pixel cannot show is not lost: the mean square of the slope of
// the faded waves goes to `g_lost`, and `lit_mesh` makes it the roughness of the surface (Bruneton, Neyret and Holzschuch 2010: the waves smaller than a pixel are a microfacet distribution whose width is their slope
// variance), so that distant water is a soft broad glitter and not a mirror or a moire.
fn ocean_normal(p: vec2<f32>, fw: f32, height: f32, length: f32, phase: f32, level: f32) -> vec3<f32> {
    let kp = 6.2831853 / max(length, 0.2);
    let lam = ocean_chop(height, length);
    let fwp = max(fw, 1.0e-4);
    let q = ocean_warp(p, length);
    let wj = ocean_warp_jac(p, length);
    let sh = ocean_shoal(p, height, length, level);
    var hh = 0.0;
    var hx = 0.0;
    var hz = 0.0;
    var dxx = 0.0;
    var dzz = 0.0;
    var dxz = 0.0;
    var lost = 0.0;
    for (var i = 0; i < %(nv)d; i = i + 1) {
        let w = OCEAN_VW[i];
        let k = kp * w.z;
        let a = height * sh.x * w.w;
        let keep = 1.0 - smoothstep(0.3, 1.1, k * fwp);
        if (keep <= 0.0) {
            lost = lost + 0.5 * a * a * k * k;
            continue;
        }
        let th = k * dot(q, w.xy) - sqrt(9.81 * k) * phase + OCEAN_VP[i];
        let ak = a * k * keep;
        let s = sin(th);
        let c = cos(th);
        hh = hh + a * keep * c;
        hx = hx - ak * w.x * s;
        hz = hz - ak * w.y * s;
        let lk = lam * ak * c;
        dxx = dxx - lk * w.x * w.x;
        dzz = dzz - lk * w.y * w.y;
        dxz = dxz - lk * w.x * w.y;
        lost = lost + 0.5 * a * a * k * k * (1.0 - keep * keep);
    }
    for (var i = 0; i < %(nf)d; i = i + 1) {
        let w = OCEAN_FW[i];
        let k = kp * w.z;
        let a = height * sh.x * w.w;
        let keep = 1.0 - smoothstep(0.3, 1.1, k * fwp);
        if (keep <= 0.0) {
            lost = lost + 0.5 * a * a * k * k;
            continue;
        }
        let th = k * dot(q, w.xy) - sqrt(9.81 * k * (1.0 + k * k * 7.589e-6)) * phase + OCEAN_FP[i];
        let ak = a * k * keep * sin(th);
        hx = hx - ak * w.x;
        hz = hz - ak * w.y;
        lost = lost + 0.5 * a * a * k * k * (1.0 - keep * keep);
    }
    // everything above is a derivative with respect to the bent point q: the chain rule takes it to p (the gradient of the height, and the matrix of the sideways displacement)
    let gx = hx * wj.x + hz * wj.z;
    let gz = hx * wj.y + hz * wj.w;
    let exx = dxx * wj.x + dxz * wj.z;
    let exz = dxx * wj.y + dxz * wj.w;
    let ezx = dxz * wj.x + dzz * wj.z;
    let ezz = dxz * wj.y + dzz * wj.w;
    let jac = (1.0 + exx) * (1.0 + ezz) - exz * ezx;
    g_lost = lost;
    g_jac = jac;
    g_break = sh.y;
    g_crest = hh / max(height * sh.x * 0.9, 1.0e-3);
    // the normal of the surface (u + D(u), h(u)): the cross product of its two tangents, y being the Jacobian (kept well off zero: where the surface folds the normal still has to point somewhere, and a normal tipped past the horizon would reflect the ground)
    // (a smooth maximum: a hard clamp would leave a visible contour where the surface starts to fold)
    let jy = 0.5 * (jac + 0.3 + sqrt((jac - 0.3) * (jac - 0.3) + 0.04));
    return normalize(vec3<f32>(-(gx * (1.0 + ezz) - ezx * gz), jy, -((1.0 + exx) * gz - gx * exz)));
}
// OCEAN END
"""


RUST = """// OCEAN BEGIN (the table is made by tools/gen_ocean.py; edit the script, not the numbers)
/// The waves that move the mesh and the bodies that float (`ocean_disp` in `scene.wgsl`, sampled from a JONSWAP spectrum): for each its direction as (cos, sin), the wave number as a multiple of the
/// peak's, its amplitude as a multiple of the significant height, and its phase.
const OCEAN_V: [[f32; 5]; %(nv)d] = [
%(rows)s
];
/// The sum of the amplitudes (a multiple of the significant height): the surface never stands higher than this above its level.
const OCEAN_AMP_SUM: f32 = %(amp)s;
/// The domain warp (`ocean_warp` in `scene.wgsl`): for each of two slow sines, its direction, its wave number (per peak length) and its phase; the waves are evaluated at a point bent by them, up to `OCEAN_WARP_AMP` peak lengths.
const OCEAN_WARP: [[f32; 4]; 2] = [%(w1)s, %(w2)s];
const OCEAN_WARP_AMP: f32 = %(wamp)s;
/// The sum of amplitude times wave number (`OCEAN_SK` in `scene.wgsl`): how steep the sea can get, which bounds the sideways pull of the waves.
const OCEAN_SK: f32 = %(sk)s;
// OCEAN END
"""


def rust_block():
    rows = "\n".join("    [%s, %s, %s, %s, %s]," % tuple(f(v) for v in c) for c in V)
    w = ["[%s, %s, %s, %s]" % (f(d[0]), f(d[1]), f(2 * math.pi / m), f(ph)) for (d, m, ph) in WARP]
    return RUST % {"nv": len(V), "rows": rows, "amp": f(AMP), "sk": f(SK), "w1": w[0], "w2": w[1], "wamp": f(WARP_AMP)}


def patch(path, new, first_anchor_start, first_anchor_end):
    s = open(path, newline="").read()
    crlf = "\r\n" in s
    s = s.replace("\r\n", "\n")
    if "// OCEAN BEGIN" in s:
        a = s.index("// OCEAN BEGIN")
        b = s.index("// OCEAN END") + len("// OCEAN END\n")
    else:
        a = s.index(first_anchor_start)
        b = s.index(first_anchor_end)
    s = s[:a] + new + ("\n" if not new.endswith("\n\n") and not s[b:].startswith("\n") else "") + s[b:]
    if crlf:
        s = s.replace("\n", "\r\n")
    open(path, "w", newline="").write(s)


wgsl = WGSL % {"arrays": wgsl_arrays(), "nv": len(V), "nf": len(F)}
patch("crates/lunaris-gpu/src/shaders/scene.wgsl", wgsl, "// ---- water: the height of the surface at a point of the plane", "// Caustics: the net of bright lines")
patch("crates/lunaris-sim/src/floaters.rs", rust_block(), "/// The waves the renderers draw", "enum Kind<'a> {")
print("ocean table written")
