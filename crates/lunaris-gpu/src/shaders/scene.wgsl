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
// How much sun shines through the blade being shaded (grass sends it in the length of its normal: one plus the amount); `lit_tail` adds it where the sun reaches, so the shadow of a rock falls on the blades too.
var<private> g_trans: f32 = 0.0;
// Whether the object being shaded is grass (its object has kind 3): its highlights are dull (a blade is a diffuse thing; only its backlit sheen is strong, and that is the sun through it).
var<private> g_grass: bool = false;
// The variance of the slope of the water's waves that were too fine for the pixels to show (`wave_normal` keeps it): `lit_mesh` turns it into roughness, so that distant water is a soft sheen and not a mirror or a shimmer.
var<private> g_lost: f32 = 0.0;
// The Jacobian of the sea's sideways displacement (`ocean_normal` sets it): 1 where the surface is as it rests, falling to 0 where the crests fold over (the waves are breaking: `lit_water` makes foam of it).
var<private> g_jac: f32 = 1.0;
// Whether this pixel is the sea (the reflections of its facets are folded above the horizon).
var<private> g_sea: bool = false;
// How near the sea is to breaking here (`ocean_shoal`: 0 in deep water, above a third the waves break), and how high this point is on its waves (about -1 in a trough, 1 on a crest).
var<private> g_break: f32 = 0.0;
var<private> g_crest: f32 = 0.0;
// `MaterialExt` of the object being shaded (set by `lit_mesh` from the two spare words of its ids): the clear coat's strength and roughness, the sheen, and the subsurface strength with its colour.
var<private> g_coat: vec2<f32> = vec2<f32>(0.0);
var<private> g_sheen: f32 = 0.0;
var<private> g_sss: vec4<f32> = vec4<f32>(0.0);
var<private> g_rough: f32 = -1.0;
// The roughness factor of a `MacroVary` surface (1: none).
var<private> g_macro_rough: f32 = 1.0;
// Light from the windows of a far wall that are lit at night (`lit_mesh` makes it where the object's emissive is negative); `lit_tail` adds it.
var<private> g_win: vec3<f32> = vec3<f32>(0.0);
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

// The mesh vertex shader's output: the same as `MeshOut` with the position `@invariant`, so that the early depth pass (depth.wgsl) and the main pass produce exactly the same depth for the same triangle.
struct MeshVsOut {
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) col: vec3<f32>,
    @location(4) tw: f32,
    @location(5) @interpolate(flat) inst: u32,
};

// OCEAN BEGIN (the table is made by tools/gen_ocean.py; edit the script, not the numbers)
// ---- water: the surface of the sea is a sum of waves drawn from a real ocean spectrum, not hand-picked sines. JONSWAP (Hasselmann et al. 1973) for how the energy falls with the frequency, Hasselmann/Mitsuyasu
// spreading (1980) for the directions (the long waves in a narrow band round the wind, the short ones every way), the amplitudes found by integrating the spectrum over a bin of frequency so that the surface has the
// variance the spectrum says: `height` is the significant wave height Hs (the mean of the highest third), `length` the wave length of the peak, the speed of every wave the one of deep water (omega = sqrt(g k): a short
// wave is slow, so the shape changes as it travels), and `phase` the clock in seconds. 25 waves (from 2.6 peak lengths down to a seventh of it, crowded round the peak with amplitudes drawn from a Rayleigh distribution so that there is no lattice) move the mesh
// and the bodies that float on it (the same numbers are in `floaters.rs`); 24 finer ones, down to a few millimetres (with surface tension in their speed), only tilt the normal. The wave lengths are not in
// simple ratios and the directions and phases are drawn, so the sum never comes round again. Each wave is a Gerstner wave (Tessendorf 2001, "Simulating Ocean Water"): the points of the surface also move sideways towards
// the crests (the "choppy" displacement), which sharpens the crests and flattens the troughs the way the real sea does, and where the displacement folds the surface over itself (the Jacobian of the mapping falls to zero)
// the waves are breaking: foam. x, z: the direction, y: the wave number as a multiple of the peak's, w: the amplitude as a multiple of Hs; the phase is in the second table.
var<private> OCEAN_VW: array<vec4<f32>, 25> = array<vec4<f32>, 25>(vec4<f32>(0.80000067, 0.59999911, 0.434, 0.0098440602), vec4<f32>(0.86382524, -0.50379158, 0.532, 0.016177173), vec4<f32>(0.37230102, 0.92811204, 0.6156, 0.079437833), vec4<f32>(0.93620625, 0.35145107, 0.69255, 0.047628978), vec4<f32>(-0.3175663, 0.94823607, 0.765225, 0.069579758), vec4<f32>(0.70856986, 0.70564067, 0.83235, 0.089447417), vec4<f32>(0.98205244, 0.18860806, 0.89745, 0.11370389), vec4<f32>(0.42955694, 0.90303977, 0.960175, 0.12170799), vec4<f32>(0.83990278, 0.54273688, 1.019875, 0.13608808), vec4<f32>(0.99041328, -0.13813595, 1.081375, 0.051608111), vec4<f32>(0.62616969, 0.77968681, 1.14995, 0.085739014), vec4<f32>(0.93953627, 0.3424494, 1.22625, 0.012438399), vec4<f32>(0.13752948, 0.99049767, 1.310625, 0.043114563), vec4<f32>(0.76754506, 0.640995, 1.40965, 0.036587199), vec4<f32>(0.99999962, -0.00087576157, 1.5246, 0.029372964), vec4<f32>(0.42210992, 0.90654466, 1.6632, 0.095919031), vec4<f32>(0.90322141, 0.4291749, 1.8348, 0.10702956), vec4<f32>(0.46565571, -0.88496596, 2.0433, 0.0039202489), vec4<f32>(0.62208369, 0.78295075, 2.3079, 0.0127442), vec4<f32>(0.99923144, 0.03919851, 2.6533, 0.04217132), vec4<f32>(-0.078472099, 0.99691631, 3.0927, 0.079070814), vec4<f32>(0.83207906, 0.55465704, 3.66, 0.071612728), vec4<f32>(0.71563773, -0.69847165, 4.4, 0.063075106), vec4<f32>(0.22241715, 0.9749516, 5.39, 0.046099261), vec4<f32>(0.99444185, 0.10528726, 6.615, 0.026652314));
var<private> OCEAN_VP: array<f32, 25> = array<f32, 25>(4.6819227, 1.3359719, 5.3992607, 0.93006218, 1.2793331, 0.56733557, 4.2140777, 0.31715633, 6.078139, 1.6928003, 6.2577439, 1.296134, 5.7680196, 6.2137731, 4.9825238, 3.6480765, 6.2732576, 1.3549507, 1.0680043, 5.9529851, 0.2895483, 1.204931, 4.3558, 3.4653057, 3.3905129);
var<private> OCEAN_FW: array<vec4<f32>, 24> = array<vec4<f32>, 24>(vec4<f32>(-0.81515033, 0.57924946, 8.1074114, 0.023067272), vec4<f32>(0.65186326, 0.75833652, 10.027475, 0.014254225), vec4<f32>(0.90473561, -0.42597357, 12.402263, 0.022915184), vec4<f32>(-0.12150026, 0.9925914, 15.339468, 0.016705202), vec4<f32>(0.92092167, 0.38974772, 18.972286, 0.010665411), vec4<f32>(0.25366548, -0.96729201, 23.465457, 0.0087454451), vec4<f32>(0.40471059, 0.91444482, 29.022738, 0.0030677936), vec4<f32>(0.9960741, -0.088523391, 35.896139, 0.010540033), vec4<f32>(-0.53025887, 0.84783579, 44.397355, 0.0031612341), vec4<f32>(0.77452307, 0.63254566, 54.911899, 0.0015722562), vec4<f32>(0.76509037, -0.64392292, 67.916582, 0.004931766), vec4<f32>(0.098337996, 0.99515307, 84.00114, 0.0011128038), vec4<f32>(0.97585087, 0.21843783, 103.89498, 0.0019249748), vec4<f32>(-0.9824626, 0.18645977, 128.50023, 0.0014918312), vec4<f32>(0.56445849, 0.82546145, 158.9327, 0.0030544403), vec4<f32>(0.95577582, -0.29409622, 196.57244, 0.00072275702), vec4<f32>(-0.26964415, 0.96296004, 243.12633, 0.00059723821), vec4<f32>(0.872962, 0.48778822, 300.70548, 0.00059560589), vec4<f32>(0.51555664, -0.8568555, 371.92101, 0.00052787457), vec4<f32>(0.2947202, 0.95558359, 460.00237, 0.00033821126), vec4<f32>(0.99946312, 0.032763839, 568.94388, 0.00021265495), vec4<f32>(-0.70413218, 0.71006892, 703.68581, 0.00048152463), vec4<f32>(0.70148875, 0.71268052, 870.33842, 0.00019950108), vec4<f32>(0.86086948, -0.50882584, 1076.4591, 0.00015503213));
var<private> OCEAN_FP: array<f32, 24> = array<f32, 24>(3.8572473, 2.5121463, 4.0946337, 3.0127043, 2.8884061, 1.36057, 3.2738828, 4.8805954, 3.4528924, 1.0850533, 5.7732482, 3.9808798, 4.8826756, 1.0176763, 5.115979, 1.7273303, 4.2598714, 1.4046676, 1.3781741, 1.6760523, 2.9901533, 0.52791088, 2.3972465, 0.14896468);
const OCEAN_SK: f32 = 2.6168607;
const OCEAN_WARP_AMP: f32 = 0.2;
const OCEAN_W1D: vec2<f32> = vec2<f32>(0.83, -0.557);
const OCEAN_W1K: f32 = 1.1855067;
const OCEAN_W1P: f32 = 1.3;
const OCEAN_W2D: vec2<f32> = vec2<f32>(0.37, 0.929);
const OCEAN_W2K: f32 = 1.6110732;
const OCEAN_W2P: f32 = 4.1;

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
    for (var i = 0; i < 25; i = i + 1) {
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

// The normal of the surface at the point that rests at p, from all the waves (the 25 that move the mesh worked out exactly with their sideways displacement: the normal of the displaced surface, and its Jacobian
// `g_jac`; the 24 finer ones as slopes), each faded out when it is finer than a pixel `fw` metres across can show (it would only shimmer). What the pixel cannot show is not lost: the mean square of the slope of
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
    for (var i = 0; i < 25; i = i + 1) {
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
    for (var i = 0; i < 24; i = i + 1) {
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

// Caustics: the net of bright lines that the waves cast on the bottom, a few passes of a folded sine pattern (Dave Hoskins' "tileable water caustic" shader pattern), 0 to about 1.3 and 0.2 on average (a caustic only gathers the sun's
// light into lines, it adds none: `lit_splat` divides by the mean). It repeats every unit of p.
fn caustic(p: vec2<f32>, t: f32) -> f32 {
    // (the phase is taken from -250: the pattern's divisions need a point far from the origin, as in the original shader, or they blow up)
    let q = fract(p) * 6.2831853 - vec2<f32>(250.0);
    var i = q;
    var c = 1.0;
    for (var n = 0; n < 5; n = n + 1) {
        let tt = t * (1.0 - 3.5 / f32(n + 1));
        i = q + vec2<f32>(cos(tt - i.x) + sin(tt + i.y), sin(tt - i.y) + cos(tt + i.x));
        c = c + 1.0 / length(vec2<f32>(q.x / (sin(i.x + tt) / 0.005), q.y / (cos(i.y + tt) / 0.005)));
    }
    c = c / 5.0;
    c = 1.17 - pow(c, 1.4);
    return pow(abs(c), 8.0);
}

@vertex
fn vs_mesh(in: VsIn) -> MeshVsOut {
    let o = objects[in.inst];
    var wp = o.model * vec4<f32>(in.pos, 1.0);
    // water carries where its point rests in `uv` (the waves move the point sideways; the normal and the foam are worked out at the rest position)
    var rest_uv = in.uv;
    if (o.ids.y == 1u) {
        rest_uv = wp.xz;
        // far away the mesh cannot hold a wave shorter than its cells (it would show as a lattice): the displacement fades out with the distance, the normal keeps the look
        let away = distance(wp.xyz, frame.eye.xyz);
        let d = ocean_disp(wp.xz, o.uvrect.y, o.uvrect.z, o.uvrect.w, o.model[3].y) * (1.0 - smoothstep(80.0, 500.0, away));
        wp = vec4<f32>(wp.x + d.x, wp.y + d.y, wp.z + d.z, wp.w);
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
    var out: MeshVsOut;
    out.clip = frame.view_proj * wp;
    out.world = wp.xyz;
    out.nrm = nm * in.nrm;
    out.uv = rest_uv;
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
    // the bias in metres (a couple of centimetres and a texel or so), not in the map's own units: a cascade's depth range grows with the biggest caster in the scene (a ground a kilometre wide makes it more than a kilometre), and a
    // constant fraction of that range was half a metre, which floated the shadows of poles and feet off the ground
    let depth = lp.z - (0.02 + texel * 1.2) / max(frame.cascade_depth[c], 1.0);
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
    // (with temporal anti-aliasing on the pattern is turned a different amount each picture, so that the few taps below add up over the pictures to many)
    let ang0 = (interleaved_noise(g_pix) + select(0.0, frame.p8.w * 0.6180339, frame.p27.y > 0.5)) * 6.2831853;
    let ca = cos(ang0);
    let sa = sin(ang0);
    let search_texels = clamp(soft * min(depth_m, 14.0) * 0.5 / tex_m, 1.5, 28.0);
    var blockers = 0.0;
    var blocker_sum = 0.0;
    for (var i = 0; i < 8; i = i + 1) {
        let o = POISSON16[i * 2];
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
    for (var i = 0; i < 12; i = i + 1) {
        let o = POISSON16[i];
        let r = vec2<f32>(o.x * ca - o.y * sa, o.x * sa + o.y * ca) * filter_texels * ts;
        lit = lit + textureSampleCompareLevel(shadow_map, shadow_samp, uv + r, c, depth);
    }
    return lit / 12.0;
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

// A normal map's texel as a vector in -1..1: the picture holds x and y (a block-compressed one has only those two) and z is made from them, a unit vector pointing out of the surface.
fn normal_xyz(t: vec4<f32>) -> vec3<f32> {
    let xy = t.xy * 2.0 - vec2<f32>(1.0);
    return vec3<f32>(xy, sqrt(max(1.0 - dot(xy, xy), 0.0)));
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

// Estevez and Kulla's Charlie distribution for the soft glow of cloth.
fn d_charlie(nh: f32, rough: f32) -> f32 {
    let inv_a = 1.0 / max(rough, 0.05);
    let sin2 = max(1.0 - nh * nh, 0.0);
    return (2.0 + inv_a) * pow(sin2, inv_a * 0.5) / (2.0 * PI);
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
    var lo_sheen = vec3<f32>(0.0);
    var lo_coat = 0.0;
    let count = u32(frame.p2.y);
    let sun = i32(frame.p4.x);
    for (var i = 0u; i < count; i = i + 1u) {
        let l = frame.lights[i];
        var ldir: vec3<f32>;
        var atten = 1.0;
        if (l.vec.w > 0.5) {
            let to = l.vec.xyz - world;
            let dist = length(to);
            ldir = select(vec3<f32>(0.0, 1.0, 0.0), to / dist, dist > 0.0);
            atten = lamp_atten(dist, l.color.w);
            if (l.spot.w > -1.5) {
                // inside the cone of a spot light, fading out towards its edge
                atten = atten * smoothstep(l.spot.w, l.cone.x, dot(-ldir, l.spot.xyz));
            }
        } else {
            ldir = l.vec.xyz;
        }
        let nl_raw = dot(n, ldir);
        // with subsurface the light wraps a little way round the terminator
        let wrap = g_sss.x * 0.6;
        let nlw = clamp((nl_raw + wrap) / (1.0 + wrap), 0.0, 1.0);
        if ((nl_raw <= 0.0 && nlw <= 0.0) || atten <= 0.0) {
            continue;
        }
        let nl = max(nl_raw, 0.0);
        var radiance = l.color.rgb * atten;
        if (i32(i) == sun) {
            radiance = radiance * shadow;
        } else if (l.vec.w > 0.5) {
            for (var k = 0; k < 4; k = k + 1) {
                if (i32(frame.psh[k].info.x) == i32(i)) {
                    radiance = radiance * point_shadow(k, l.vec.xyz, world, n);
                    break;
                }
            }
        }
        let h = normalize(ldir + v);
        let nh = max(dot(n, h), 0.0);
        let vh = max(dot(v, h), 0.0);
        let f = fresnel(f0, vh);
        let spec = d_ggx(nh, a) * vis_smith(nl, nv, a) * f;
        // radiance counts as "irradiance / pi", so the diffuse term needs no 1 / pi and the specular one a factor pi
        lo_diff = lo_diff + (vec3<f32>(1.0) - f) * diff_col * radiance * mix(nl, nlw, g_sss.x);
        lo_spec = lo_spec + spec * PI * radiance * nl;
        if (g_sss.x > 0.0) {
            // the colour the light picks up in the body shows at the terminator
            lo_diff = lo_diff + g_sss.yzw * diff_col * radiance * (0.3 * g_sss.x * exp(-nl_raw * nl_raw * 6.0) * nlw);
        }
        if (g_sheen > 0.0) {
            lo_sheen = lo_sheen + radiance * (d_charlie(nh, 0.45) / (4.0 * (nl + nv - nl * nv) + 1e-3) * PI * nl);
        }
        if (g_coat.x > 0.0) {
            let ac = max(g_coat.y * g_coat.y, 0.002);
            let fc = 0.04 + 0.96 * pow(1.0 - vh, 5.0);
            lo_coat = lo_coat + d_ggx(nh, ac) * (0.25 / max(vh * vh, 1e-3)) * fc * PI * nl * (radiance.x + radiance.y + radiance.z) * (1.0 / 3.0);
        }
    }
    // the sky around the surface
    let amb_diff = (sky_irradiance(n) + sun_bounce(n)) * diff_col * ao;
    var r = reflect(-v, n);
    if (g_sea) {
        // a facet of the sea that tips away from the eye would reflect the ground below the horizon: it is hidden by the waves in front of it, and what it shows is the sky (a second reflection), so the reflection is folded up
        r.y = abs(r.y);
    }
    // (the sun's disc is some fifty times white: seen in water it would bloom over a third of the picture, so a reflection is held to a few times white, which the eye reads as the same dazzle)
    var env = min(sky_color(r, rough < 0.3), vec3<f32>(7.0));
    // a rough surface sees a blurred sky
    env = mix(env, sky_irradiance(r), smoothstep(0.15, 0.95, rough));
    let amb_spec = env * env_brdf(f0, rough, nv) * specular_occlusion(nv, ao, rough) * frame.p0.x;
    var diffuse = lo_diff + amb_diff;
    var spec = lo_spec + amb_spec;
    if (g_sheen > 0.0) {
        // a glow at grazing angles in a tint of the base colour: from the lights and from the sky
        let sheen_col = mix(vec3<f32>(1.0), albedo, 0.35) * (0.7 * g_sheen);
        // velvet is dark head-on and glows at the rim: the lights' share counts a fifth face-on and in full at grazing angles
        let graze = 0.2 + 0.8 * pow(1.0 - nv, 2.0);
        diffuse = diffuse + sheen_col * (lo_sheen * graze + sky_irradiance(n) * (0.35 * pow(1.0 - nv, 4.0)));
    }
    if (g_coat.x > 0.0) {
        // the lacquer: it takes a share of the light (more at grazing angles) from what is below, and adds its own sharp highlight and mirror
        let fcn = (0.04 + 0.96 * pow(1.0 - nv, 5.0)) * g_coat.x;
        let env_coat = sky_color(r, true) * env_brdf(vec3<f32>(0.04), max(g_coat.y, 0.045), nv) * specular_occlusion(nv, ao, g_coat.y) * frame.p0.x;
        diffuse = diffuse * (1.0 - fcn);
        spec = spec * (1.0 - fcn) + vec3<f32>(lo_coat) * g_coat.x + env_coat * g_coat.x;
    }
    return Pbr(diffuse, spec);
}

// What a reflection probe changes about `shade_pbr`'s result: inside its box what it captured takes the place of the sky's reflection and of the sky's ambient light (the probe holds the light of the
// world as it was, so the `ambient` strength is not applied to it), by the weight `w` of the probe. It is added to the result, so that `shade_pbr` itself stays as it was for a picture without probes.
fn shade_probe_change(albedo: vec3<f32>, metallic: f32, rough_in: f32, world: vec3<f32>, n: vec3<f32>, v: vec3<f32>, ao: f32) -> Pbr {
    let rough = clamp(rough_in, 0.045, 1.0);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let diff_col = albedo * (1.0 - metallic);
    let nv = max(dot(n, v), 1e-4);
    var r = reflect(-v, n);
    if (g_sea) {
        // a facet of the sea that tips away from the eye would reflect the ground below the horizon: it is hidden by the waves in front of it, and what it shows is the sky (a second reflection), so the reflection is folded up
        r.y = abs(r.y);
    }
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

// ---- macro variation of a mesh (`MacroVary`): smooth noise from the world position at three scales, so that a wall of one small repeated texture has darker and lighter patches at every distance. Returns the factor for
// the colour (rgb) and for the roughness (w).
fn value_noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(mix(hash31(i), hash31(i + vec3<f32>(1.0, 0.0, 0.0)), u.x), mix(hash31(i + vec3<f32>(0.0, 1.0, 0.0)), hash31(i + vec3<f32>(1.0, 1.0, 0.0)), u.x), u.y);
    let b = mix(mix(hash31(i + vec3<f32>(0.0, 0.0, 1.0)), hash31(i + vec3<f32>(1.0, 0.0, 1.0)), u.x), mix(hash31(i + vec3<f32>(0.0, 1.0, 1.0)), hash31(i + vec3<f32>(1.0, 1.0, 1.0)), u.x), u.y);
    return mix(a, b, u.z);
}

fn macro_vary(world: vec3<f32>, v: vec4<f32>) -> vec4<f32> {
    let p = world / max(v.y, 0.5);
    // the three scales: the whole size, a fifth and a twenty-fifth, weighted 0.5, 0.32, 0.18 (a value noise sits near the middle, so the sum is stretched back out a little)
    let c = 0.5 * value_noise3(p) + 0.32 * value_noise3(p * 5.0 + vec3<f32>(17.3, 3.1, 8.7)) + 0.18 * value_noise3(p * 25.0 + vec3<f32>(41.7, 29.9, 13.3));
    let d = clamp((c - 0.5) * 2.4, -1.0, 1.0);
    // the colour's lean (warm to cool) comes from a noise of its own, a little wider
    let lean = clamp((value_noise3(p * 0.7 + vec3<f32>(91.0, 57.0, 23.0)) - 0.5) * 2.4, -1.0, 1.0);
    let gain = max(1.0 + 2.0 * v.x * d, 0.05);
    let tint = vec3<f32>(1.0 + 0.5 * v.z * lean, 1.0 + 0.1 * v.z * lean, 1.0 - 0.5 * v.z * lean);
    let rough = clamp(1.0 - 1.5 * v.w * d, 0.3, 2.0);
    return vec4<f32>(gain * tint, rough);
}

fn lit_mesh(in: MeshOut) -> Lit {
    let o = objects[in.inst];
    var n = normalize(in.nrm);
    // only grass (its object has kind 3) carries the amount in its normal's length; the normal of anything else is scaled by its transform
    g_trans = select(0.0, max(length(in.nrm) - 1.0, 0.0), o.ids.y == 3u);
    g_grass = o.ids.y == 3u;
    // (for water `uv` is where the surface rests: the pixel's size there says which waves it can show)
    let pixel_m = length(fwidth(in.uv));
    if (o.ids.y == 1u) {
        // the waves tilt the surface's own normal (flat for a sheet, leaning downstream for a river)
        n = normalize(ocean_normal(in.uv, pixel_m, o.uvrect.y, o.uvrect.z, o.uvrect.w, o.model[3].y) + (n - vec3<f32>(0.0, 1.0, 0.0)));
        // the waves too small for the pixel are the roughness: GGX's alpha (the roughness squared) grows with the root of their slope variance
        let alpha0 = o.mat.w * o.mat.w;
        g_rough = clamp(sqrt(sqrt(alpha0 * alpha0 + g_lost)), 0.03, 0.5);
        g_metal = 0.0;
        g_sea = true;
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
    // `PbrMaps.untile`: the texture and the maps are blended from three offset copies, so no repetition shows
    let untile = !classic && (maps & 8u) != 0u;
    var hx: HexT;
    var t: vec4<f32>;
    if (untile) {
        hx = hex_grid(tuv, 0.0);
        t = hex_colour(tex, tex_samp, hx, duvx, duvy);
    } else {
        t = textureSampleGrad(tex, tex_samp, tuv, duvx, duvy);
    }
    // a cut-out (an imported material with `alphaMode` MASK; n0.w is where): the surface is not there where the texture's alpha is below it
    if (!classic && o.ids.y != 1u && o.n0.w > 0.0 && o.misc.z > 0.5 && t.a < o.n0.w) {
        discard;
    }
    // the relief of the built-in materials: the height map's slope, as seen from this pixel, tilts the normal
    var relief = textureSampleGrad(height_tex, tex_samp, tuv, duvx, duvy).r * o.uvrect.x;
    if (untile) {
        relief = hex_fetch(height_tex, tex_samp, hx, duvx, duvy).r * o.uvrect.x;
    }
    let dhx = dpdx(relief);
    let dhy = dpdy(relief);
    if (!classic && o.uvrect.x > 0.0) {
        n = bump_normal(n, dpx, dpy, dhx, dhy);
    }
    // the maps are always sampled (derivatives need uniform control flow); whether they count is decided afterwards
    var nmap = normal_xyz(textureSampleGrad(normal_tex, tex_samp, tuv, duvx, duvy));
    var armv = textureSampleGrad(arm_tex, tex_samp, tuv, duvx, duvy);
    if (untile) {
        nmap = normal_xyz(hex_fetch(normal_tex, tex_samp, hx, duvx, duvy));
        armv = hex_fetch(arm_tex, tex_samp, hx, duvx, duvy);
    }
    if (!classic && (maps & 1u) != 0u) {
        n = map_normal(n, dpx, dpy, duvx, duvy, vec3<f32>(nmap.xy * o.pbr.x, nmap.z));
    }
    if (!classic && (maps & 2u) != 0u) {
        g_ao = mix(1.0, armv.r, o.pbr.y);
        g_rough = clamp(armv.g * o.pbr.w, 0.045, 1.0);
        g_metal = armv.b;
    }
    // `MaterialExt`: a byte for each number in the two spare words of the ids
    g_coat = vec2<f32>(f32(o.ids.z & 255u), f32((o.ids.z >> 8u) & 255u)) / 255.0;
    g_sheen = f32((o.ids.z >> 16u) & 255u) / 255.0;
    g_sss = vec4<f32>(f32((o.ids.z >> 24u) & 255u), f32(o.ids.w & 255u), f32((o.ids.w >> 8u) & 255u), f32((o.ids.w >> 16u) & 255u)) / 255.0;
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
    if (!classic && (o.vary.x + o.vary.z + o.vary.w) > 0.0) {
        let mv = macro_vary(in.world, o.vary);
        base = base * mv.rgb;
        g_macro_rough = mv.w;
    }
    if (!classic && o.ids.y == 1u && o.pbr.w > 0.5 && frame.p28.x > 0.5) {
        return lit_water(in, o, base, n);
    }
    // the windows of a far wall (the part's emissive is negative while it is night): the texture's alpha is the glass, a hash of the bay says whether the light is on
    if (!classic && o.mat.y < 0.0) {
        let bay = floor(tuv);
        let h = win_hash(bay + floor(in.world.xz * vec2<f32>(0.173, 0.211)));
        let h2 = win_hash(bay * 1.7 + vec2<f32>(13.1, 5.7) + floor(in.world.xz * vec2<f32>(0.173, 0.211)));
        let on = step(h, 0.34);
        let tone = mix(vec3<f32>(1.0, 0.70, 0.40), vec3<f32>(0.78, 0.86, 1.0), step(0.8, h2));
        g_win = tone * (2.6 * (-o.mat.y) * on * t.a * (0.6 + 0.8 * h2));
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
    // the wash: the band of foam breathes in and out with the waves, a different way from place to place
    let wash = 0.7 + 0.4 * sin(o.uvrect.w * 0.7 + nz * 6.0 + in.world.x * 0.05);
    let band = 1.0 - smoothstep(0.0, max(o.pbr.y * wash, 0.01), thick);
    // whitecaps: where the waves' crests fold over (the Jacobian of their sideways displacement falls to zero) the sea breaks, in tufts that the noise breaks up and that drift with the water
    let fold = clamp((0.45 - g_jac) / 0.45, 0.0, 1.0);
    let caps = clamp(fold * (0.3 + 1.4 * nz), 0.0, 1.0) * o.pbr.z;
    // surf: where the sea is shallow enough for its waves to break the crests carry foam, in tufts
    let surf = smoothstep(0.3, 0.7, g_break) * smoothstep(0.05, 0.8, g_crest) * clamp(0.15 + 1.4 * nz, 0.0, 1.0) * o.pbr.z;
    let foam = clamp(max(max(band * smoothstep(0.25, 0.75, nz + band * 0.5) * o.pbr.z, caps), surf), 0.0, 1.0);
    col = mix(col, vec3<f32>(0.85, 0.88, 0.9), foam);
    var l = lit_tail(in, o, col, n, false);
    // see-through where it is thin, opaque where it is deep; foam is solid; the sky's glint at grazing angles is lit_tail's
    let v = normalize(frame.eye.xyz - in.world);
    let fres = pow(1.0 - clamp(dot(n, v), 0.0, 1.0), 5.0);
    // light that comes through the crests towards the eye (subsurface scattering, the practice of Atlas and Crest: the glow of a wave seen against the sun): strongest when looking towards the sun, on the sides of the waves
    // that face away from it and near the crests, in the colour of the shallows
    if (i32(frame.p4.x) >= 0) {
        let sun = normalize(frame.sun_dir.xyz);
        let toward = pow(clamp(dot(sun, -v), 0.0, 1.0), 4.0);
        let facing = pow(clamp(0.5 - 0.5 * dot(sun, n), 0.0, 1.0), 3.0);
        let crest = smoothstep(-0.3, 0.9, g_crest);
        l.diffuse = l.diffuse + shallow * frame.lights[u32(frame.p4.x)].color.rgb * (toward * facing * crest * clamp(sun.y * 4.0, 0.0, 1.0) * 0.5);
    }
    let a = clamp(0.06 + absorb * 0.94, 0.0, 1.0);
    l.alpha = clamp(max(a + (1.0 - a) * fres, foam), 0.0, 1.0);
    return l;
}

// The shading every mesh pixel ends with once its colour and normal are known: the classic lighting, or the physically based one with shadows, ambient occlusion, sky and fog.
fn win_hash(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(443.897, 441.423));
    q = q + dot(q, q + 19.19);
    return fract(q.x * q.y);
}

// A smooth value noise of the world's ground plane, for where the puddles are.
fn wet_hash(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 456.21));
    q = q + dot(q, q + 45.32);
    return fract(q.x * q.y);
}

fn wet_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(wet_hash(i), wet_hash(i + vec2<f32>(1.0, 0.0)), u.x), mix(wet_hash(i + vec2<f32>(0.0, 1.0)), wet_hash(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

fn lit_tail(in: MeshOut, o: Object, base_in: vec3<f32>, n: vec3<f32>, classic: bool) -> Lit {
    var base = base_in;
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
    let rough_base = select(o.mat.w, g_rough, g_rough >= 0.0);
    let rough_raw = select(rough_base, clamp(rough_base * g_macro_rough, 0.045, 1.0), g_macro_rough != 1.0);
    var rough = sqrt(clamp(rough_raw * rough_raw + g_nvar, 0.0, 1.0));
    // a wet world: flat ground darker and glossy in patches, walls a little
    let wet_k = frame.p27.z;
    if (wet_k > 0.001 && !g_grass) {
        let up = smoothstep(0.55, 0.92, n.y);
        let patches = smoothstep(0.28, 0.85, wet_noise(in.world.xz * 0.22) * 0.6 + wet_noise(in.world.xz * 0.9) * 0.4);
        let w = wet_k * clamp(up * (0.5 + 0.5 * patches) + 0.2 * (1.0 - up), 0.0, 1.0) * (1.0 - metal);
        base = base * mix(1.0, 0.6, w);
        rough = mix(rough, 0.17, w * 0.85);
    }
    var shadow = 1.0;
    if (i32(frame.p4.x) >= 0) {
        g_pix = in.clip.xy;
        shadow = shadow_factor(in.world, n);
    }
    var pbr = shade_pbr(base, metal, rough, in.world, n, v, ao, shadow);
    if (g_grass) {
        pbr.spec = pbr.spec * 0.3;
    }
    if (g_trans > 0.0 && i32(frame.p4.x) >= 0) {
        // the sun through the blade, in the blade's colour, where the sun reaches it
        pbr.diffuse = pbr.diffuse + base * frame.lights[u32(frame.p4.x)].color.rgb * vec3<f32>(0.4, 0.36, 0.12) * (g_trans * shadow);
    }
    // light that bounced off the other surfaces in view (`GlobalIllumination`), in the colour of this one
    pbr.diffuse = pbr.diffuse + base * (1.0 - metal) * aot.rgb;
    let fog = fog_amount(in.world);
    // glass is more mirror-like at grazing angles
    let nv = clamp(dot(n, v), 0.0, 1.0);
    let alpha = o.color.a + (1.0 - o.color.a) * pow(1.0 - nv, 5.0);
    if (frame.p18.x > 0.5) {
        let change = shade_probe_change(base, metal, rough, in.world, n, v, ao);
        return Lit(pbr.diffuse + change.diffuse + base * max(o.mat.y, 0.0) + g_win, pbr.spec + change.spec, fog, alpha);
    }
    return Lit(pbr.diffuse + base * max(o.mat.y, 0.0) + g_win, pbr.spec, fog, alpha);
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
    // each layer's picture averaged over the whole of it (linear), found once on the CPU
    avg: array<vec4<f32>, 4>,
    // x: the layer that steep ground shows, laid on from the sides (-1: none), y: metres its picture covers before it repeats, z: how much of the colour is the terrain's own photograph
    cliff: vec4<f32>,
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

// ---- no two places alike: texture de-tiling by hex-tiling (Morten Mikkelsen, "Practical Real-Time Hex-Tiling", JCGT 11(3), 2022; after Heitz and Neyret's stochastic texturing). The plane is cut into a hexagonal grid
// (the triangles of a skewed grid, three corners each); every corner gives the texture its own random offset (and turn); a pixel is the blend of the three copies its triangle's corners ask for, weighted by how
// close it is to each corner (sharpened, so the blend is narrow and the texture's contrast stays) and by the copies' brightness (so a dark patch does not wash a bright one out).
struct HexT {
    st1: vec2<f32>,
    st2: vec2<f32>,
    st3: vec2<f32>,
    r1: mat2x2<f32>,
    r2: mat2x2<f32>,
    r3: mat2x2<f32>,
    w: vec3<f32>,
};

// The weights the last `hex_colour` found: the other maps of the same surface (normal, roughness, height) are blended with the same ones, so that they stay in step with the colour.
var<private> g_hex_w: vec3<f32> = vec3<f32>(1.0, 0.0, 0.0);

fn hex_rot(h: f32, strength: f32) -> mat2x2<f32> {
    let a = (h * 2.0 - 1.0) * 3.14159265 * strength;
    let c = cos(a);
    let s = sin(a);
    return mat2x2<f32>(vec2<f32>(c, s), vec2<f32>(-s, c));
}

// `rot_strength` 0 only moves the copies about (normal maps stay right: a turned copy would need its normals turned too); 1 turns them by up to half a turn either way.
fn hex_grid(st: vec2<f32>, rot_strength: f32) -> HexT {
    let g = st * 3.4641016;
    let skewed = vec2<f32>(g.x - 0.57735027 * g.y, 1.15470054 * g.y);
    let base = vec2<i32>(floor(skewed));
    let f = fract(skewed);
    let z = 1.0 - f.x - f.y;
    let s = step(0.0, -z);
    let s2 = 2.0 * s - 1.0;
    let i_s = i32(s);
    let v1 = base + vec2<i32>(i_s, i_s);
    let v2 = base + vec2<i32>(i_s, 1 - i_s);
    let v3 = base + vec2<i32>(1 - i_s, i_s);
    let h1 = hash3(v1.x, v1.y, 17.0);
    let h2 = hash3(v2.x, v2.y, 17.0);
    let h3 = hash3(v3.x, v3.y, 17.0);
    var t: HexT;
    t.r1 = hex_rot(h1.z, rot_strength);
    t.r2 = hex_rot(h2.z, rot_strength);
    t.r3 = hex_rot(h3.z, rot_strength);
    let c1 = vec2<f32>(f32(v1.x) + 0.5 * f32(v1.y), f32(v1.y) / 1.15470054) / 3.4641016;
    let c2 = vec2<f32>(f32(v2.x) + 0.5 * f32(v2.y), f32(v2.y) / 1.15470054) / 3.4641016;
    let c3 = vec2<f32>(f32(v3.x) + 0.5 * f32(v3.y), f32(v3.y) / 1.15470054) / 3.4641016;
    t.st1 = t.r1 * (st - c1) + c1 + h1.xy;
    t.st2 = t.r2 * (st - c2) + c2 + h2.xy;
    t.st3 = t.r3 * (st - c3) + c3 + h3.xy;
    t.w = vec3<f32>(-z * s2, s - f.y * s2, s - f.x * s2);
    return t;
}

fn hex_gain(x: vec3<f32>, r: f32) -> vec3<f32> {
    let k = log(1.0 - r) / log(0.5);
    let s = 2.0 * step(vec3<f32>(0.5), x);
    let m = 2.0 * (1.0 - s);
    let res = 0.5 * s + 0.25 * m * pow(max(vec3<f32>(0.0), s + x * m), vec3<f32>(k));
    return res / (res.x + res.y + res.z);
}

// The colour: three copies, weighted by their corners' closeness and brightness; the weights are kept for the maps that go with it.
fn hex_colour(tex: texture_2d<f32>, samp: sampler, t: HexT, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    let c1 = textureSampleGrad(tex, samp, t.st1, t.r1 * dx, t.r1 * dy);
    let c2 = textureSampleGrad(tex, samp, t.st2, t.r2 * dx, t.r2 * dy);
    let c3 = textureSampleGrad(tex, samp, t.st3, t.r3 * dx, t.r3 * dy);
    let lw = vec3<f32>(0.299, 0.587, 0.114);
    var dw = vec3<f32>(dot(c1.rgb, lw), dot(c2.rgb, lw), dot(c3.rgb, lw));
    dw = mix(vec3<f32>(1.0), dw, 0.6);
    var w = dw * pow(t.w, vec3<f32>(7.0));
    w = w / (w.x + w.y + w.z);
    w = hex_gain(w, 0.75);
    g_hex_w = w;
    return w.x * c1 + w.y * c2 + w.z * c3;
}

// Another map of the same surface, blended with the weights `hex_colour` found.
fn hex_fetch(tex: texture_2d<f32>, samp: sampler, t: HexT, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    let c1 = textureSampleGrad(tex, samp, t.st1, t.r1 * dx, t.r1 * dy);
    let c2 = textureSampleGrad(tex, samp, t.st2, t.r2 * dx, t.r2 * dy);
    let c3 = textureSampleGrad(tex, samp, t.st3, t.r3 * dx, t.r3 * dy);
    return g_hex_w.x * c1 + g_hex_w.y * c2 + g_hex_w.z * c3;
}

// One layer at this place: its colour (in the working space) and the height of its relief, 0 to 1.
fn splat_layer(tex: texture_2d<f32>, hgt: texture_2d<f32>, uv: vec2<f32>, scale: vec2<f32>, classic: bool) -> vec4<f32> {
    let s = uv * scale;
    let tuv = vec2<f32>(s.x, 1.0 - s.y);
    if (!classic) {
        // a ground of 8 km repeating a photograph a thousand times: every layer is hex-tiled (the turn too: the height of a layer is a number, not a direction)
        let dx = dpdx(tuv);
        let dy = dpdy(tuv);
        let ht = hex_grid(tuv, 0.6);
        let tc = hex_colour(tex, layer_samp, ht, dx, dy);
        let hh = hex_fetch(hgt, layer_samp, ht, dx, dy).r;
        return vec4<f32>(tc.rgb, hh);
    }
    let t = textureSample(tex, layer_samp, tuv);
    let h = textureSample(hgt, layer_samp, tuv).r;
    return vec4<f32>(select(t.rgb, t.rgb * t.rgb, classic), h);
}

// What a layer's look-up needs of the screen's derivatives, worked out where all the pixels of a quad are (outside the branches that leave a layer out where nobody has painted it).
struct LayerCoord {
    tuv: vec2<f32>,
    dx: vec2<f32>,
    dy: vec2<f32>,
};

fn layer_coord(uv: vec2<f32>, scale: vec2<f32>) -> LayerCoord {
    let s = uv * scale;
    let tuv = vec2<f32>(s.x, 1.0 - s.y);
    return LayerCoord(tuv, dpdx(tuv), dpdy(tuv));
}

// One layer, hex-tiled (the high qualities): its colour and, when it has a relief to show, the height of it.
fn splat_layer_hex(tex: texture_2d<f32>, hgt: texture_2d<f32>, c: LayerCoord, relief: bool) -> vec4<f32> {
    let ht = hex_grid(c.tuv, 0.6);
    let tc = hex_colour(tex, layer_samp, ht, c.dx, c.dy);
    var hh = 0.5;
    if (relief) {
        hh = hex_fetch(hgt, layer_samp, ht, c.dx, c.dy).r;
    }
    return vec4<f32>(tc.rgb, hh);
}

fn lit_splat(in: MeshOut) -> Lit {
    let o = objects[in.inst];
    var n = normalize(in.nrm);
    let classic = frame.p2.x < 0.5;
    var w = splat_weights(in.uv);
    var a0 = vec4<f32>(0.0, 0.0, 0.0, 0.5);
    var a1 = a0;
    var a2 = a0;
    var a3 = a0;
    if (classic) {
        a0 = splat_layer(lay0, hgt0, in.uv, splat.scales[0].xy, classic);
        a1 = splat_layer(lay1, hgt1, in.uv, splat.scales[0].zw, classic);
        a2 = splat_layer(lay2, hgt2, in.uv, splat.scales[1].xy, classic);
        a3 = splat_layer(lay3, hgt3, in.uv, splat.scales[1].zw, classic);
    } else {
        // a layer that hardly counts at this pixel is not looked at (a painted ground has one or two layers here; each costs six hex-tiled anisotropic taps): the weights of the others make up the whole
        let kept = select(vec4<f32>(0.0), w, w > vec4<f32>(0.012));
        w = kept / max(kept.x + kept.y + kept.z + kept.w, 1.0e-4);
        let c0 = layer_coord(in.uv, splat.scales[0].xy);
        let c1 = layer_coord(in.uv, splat.scales[0].zw);
        let c2 = layer_coord(in.uv, splat.scales[1].xy);
        let c3 = layer_coord(in.uv, splat.scales[1].zw);
        // the layers' detail fades out with the distance (what 400 m away shows is the photograph and the layers' averages, not their texels): a pixel beyond it takes no tap at all, which is most of the pixels of a landscape
        let detail = 1.0 - smoothstep(110.0, 380.0, distance(in.world, frame.eye.xyz));
        a0 = vec4<f32>(splat.avg[0].rgb, 0.5);
        a1 = vec4<f32>(splat.avg[1].rgb, 0.5);
        a2 = vec4<f32>(splat.avg[2].rgb, 0.5);
        a3 = vec4<f32>(splat.avg[3].rgb, 0.5);
        if (detail > 0.0) {
            if (w.x > 0.0) {
                let t = splat_layer_hex(lay0, hgt0, c0, splat.bump.x > 0.0);
                a0 = vec4<f32>(mix(a0.rgb, t.rgb, detail), mix(0.5, t.a, detail));
            }
            if (w.y > 0.0) {
                let t = splat_layer_hex(lay1, hgt1, c1, splat.bump.y > 0.0);
                a1 = vec4<f32>(mix(a1.rgb, t.rgb, detail), mix(0.5, t.a, detail));
            }
            if (w.z > 0.0) {
                let t = splat_layer_hex(lay2, hgt2, c2, splat.bump.z > 0.0);
                a2 = vec4<f32>(mix(a2.rgb, t.rgb, detail), mix(0.5, t.a, detail));
            }
            if (w.w > 0.0) {
                let t = splat_layer_hex(lay3, hgt3, c3, splat.bump.w > 0.0);
                a3 = vec4<f32>(mix(a3.rgb, t.rgb, detail), mix(0.5, t.a, detail));
            }
        }
    }
    let albedo = a0.rgb * splat.tint[0].rgb * w.x + a1.rgb * splat.tint[1].rgb * w.y + a2.rgb * splat.tint[2].rgb * w.z + a3.rgb * splat.tint[3].rgb * w.w;
    // macro variation (the landscape technique of Unreal's materials): three scales of noise over the ground, a few per cent each, break up what the photographs, however well hex-tiled, cannot: wide stretches of the same shade
    var macro_shade = 1.0;
    if (!classic) {
        let wp = in.world.xz;
        macro_shade = 0.84 + 0.32 * (0.5 * value_noise(wp / 170.0) + 0.3 * value_noise(wp / 31.0) + 0.2 * value_noise(wp / 6.5));
    }
    // the terrain's own colour picture (Tellus's albedo): the colour of the ground at every distance, and the layers' photographs, which no picture of 8 km can hold, give it detail as a ratio to their own average (the
    // detail-over-macro technique of terrain engines: Overgrowth tints its detail textures by their difference from the colour map; O3DE blends detail materials over a macro material)
    var ground_albedo = albedo;
    if (!classic && splat.info.z > 0.5) {
        let macro_c = textureSampleLevel(tex, tex_samp, in.uv, 0.0).rgb;
        let lw = vec3<f32>(0.2126, 0.7152, 0.0722);
        let avg0 = splat.avg[0].rgb * splat.tint[0].rgb * w.x;
        let avg1 = splat.avg[1].rgb * splat.tint[1].rgb * w.y;
        let avg2 = splat.avg[2].rgb * splat.tint[2].rgb * w.z;
        let avg3 = splat.avg[3].rgb * splat.tint[3].rgb * w.w;
        let avg_lum = max(dot(avg0 + avg1 + avg2 + avg3, lw), 0.02);
        let ratio = clamp(dot(albedo, lw) / avg_lum, 0.35, 2.6);
        ground_albedo = mix(albedo, macro_c * ratio, splat.cliff.z);
    }
    // a cliff: the photograph and the layers are laid on from above and stretch down a wall (a texel of 24 cm covers metres of a vertical face), so where the ground is steep the layer flagged `cliff` is laid on
    // from the sides instead (triplanar, the two vertical planes), and takes the photograph's place, keeping its broad light and dark (the photograph at a coarse mip level) as a ratio to the layer's own average
    if (!classic && splat.cliff.x >= 0.0) {
        let steep = 1.0 - smoothstep(0.36, 0.64, n.y);
        let m = splat.cliff.y;
        let px = in.world.zy / m;
        let pz = in.world.xy / m;
        let gxx = dpdx(px);
        let gxy = dpdy(px);
        let gzx = dpdx(pz);
        let gzy = dpdy(pz);
        if (steep > 0.01) {
            let k = i32(splat.cliff.x + 0.5);
            var cx = vec3<f32>(0.0);
            var cz = vec3<f32>(0.0);
            if (k == 0) {
                cx = textureSampleGrad(lay0, layer_samp, px, gxx, gxy).rgb;
                cz = textureSampleGrad(lay0, layer_samp, pz, gzx, gzy).rgb;
            } else if (k == 1) {
                cx = textureSampleGrad(lay1, layer_samp, px, gxx, gxy).rgb;
                cz = textureSampleGrad(lay1, layer_samp, pz, gzx, gzy).rgb;
            } else if (k == 2) {
                cx = textureSampleGrad(lay2, layer_samp, px, gxx, gxy).rgb;
                cz = textureSampleGrad(lay2, layer_samp, pz, gzx, gzy).rgb;
            } else {
                cx = textureSampleGrad(lay3, layer_samp, px, gxx, gxy).rgb;
                cz = textureSampleGrad(lay3, layer_samp, pz, gzx, gzy).rgb;
            }
            let ax = pow(abs(n.x), 4.0);
            let az = pow(abs(n.z), 4.0);
            let sides = (cx * ax + cz * az) / max(ax + az, 1.0e-4) * splat.tint[k].rgb;
            let lw2 = vec3<f32>(0.2126, 0.7152, 0.0722);
            var wall = sides;
            if (splat.info.z > 0.5) {
                let coarse = textureSampleLevel(tex, tex_samp, in.uv, 5.0).rgb;
                let avg_k = max(dot(splat.avg[k].rgb * splat.tint[k].rgb, lw2), 0.02);
                // the photograph's own colour (grey limestone, white snow) with the layer's detail as a ratio to the layer's average, the same detail-over-macro rule as above
                wall = coarse * clamp(dot(sides, lw2) / avg_k, 0.4, 2.4);
            }
            ground_albedo = mix(ground_albedo, wall, steep);
        }
    }
    var base = o.color.rgb * ground_albedo * macro_shade;
    // the shore, when the picture has water (frame.p28: x 1, the level of the sea, how far up the swash reaches, the clock of the waves): the sand the sea has just left is wet, half as bright and shiny (the rule the landscape
    // artists give: wet ground is about half as bright as dry, and smooth), the edge irregular and moving with the waves; and under the water the bottom carries the caustics of the sun through the ripples
    if (!classic && frame.p28.x > 0.5) {
        let cpx = length(fwidth(in.world.xz));
        let level = frame.p28.y;
        let clock = frame.p28.w;
        // the surface is not at the level but where the waves put it (as far as the mesh carries them): the sea runs up and down the sand, and a trough that is lower than the sand leaves it wet, not 'under water'
        var surface = level;
        if (abs(in.world.y - level) < frame.p29.x * 1.4 + 1.0) {
            let fade = 1.0 - smoothstep(80.0, 500.0, distance(in.world, frame.eye.xyz));
            surface = level + ocean_disp(in.world.xz, frame.p29.x, frame.p29.y, clock, level).y * fade;
        }
        let above = in.world.y - surface;
        let nzw = value_noise(in.world.xz * 0.28 + vec2<f32>(clock * 0.04, 0.0));
        let reach = max(frame.p28.z * (0.7 + 0.3 * sin(clock * 0.8 + nzw * 7.0)) + 0.25 * (nzw - 0.5), 0.05);
        let wet = 1.0 - smoothstep(0.0, reach, above);
        base = base * mix(1.0, 0.58, wet);
        // (only the film on the sand above the water shines: the bottom under the water is as matt as any other wet sand, the glint is the water's)
        g_rough = select(mix(0.9, 0.42, wet), 0.9, above < 0.0);
        g_metal = 0.0;
        if (above < 0.0) {
            let depth = -above;
            // two nets at different sizes and angles (one alone shows as a regular grid from a distance)
            let pw = in.world.xz;
            let cc = 0.5 * (caustic(pw * 0.23, clock * 0.5) + caustic(vec2<f32>(pw.x * 0.8 - pw.y * 0.6, pw.x * 0.6 + pw.y * 0.8) * 0.171 + vec2<f32>(3.1, 1.7), clock * 0.43 + 5.0));
            let sun_up = clamp(normalize(frame.sun_dir.xyz).y * 3.0, 0.0, 1.0);
            // the sun's light is gathered into lines (several times the average where they are) and loses its edges with the depth (the sun is not a point) and with the pixel size (a net of 4 m cells seen from a long way off is its average)
            let focus = clamp(depth / 0.3, 0.0, 1.0) * exp(-depth * 0.45) * sun_up * (1.0 - smoothstep(0.35, 1.4, cpx / 4.35));
            base = base * mix(1.0, cc * 5.0, focus);
        }
    }
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
    let pixel_m = length(fwidth(in.uv));
    if (o.ids.y == 1u) {
        n = normalize(ocean_normal(in.uv, pixel_m, o.uvrect.y, o.uvrect.z, o.uvrect.w, o.model[3].y) + (n - vec3<f32>(0.0, 1.0, 0.0)));
    }
    var base = o.color.rgb;
    if (o.misc.w > 0.5) {
        base = base * in.col;
    }
    let uvs = in.uv * o.misc.xy;
    let tuv = vec2<f32>(uvs.x, 1.0 - uvs.y);
    let t = textureSample(tex, tex_samp, tuv);
    if (o.ids.y != 1u && o.n0.w > 0.0 && o.misc.z > 0.5 && t.a < o.n0.w) {
        discard;
    }
    let relief = textureSample(height_tex, tex_samp, tuv).r * o.uvrect.x;
    let dpx = dpdx(in.world);
    let dpy = dpdy(in.world);
    let dhx = dpdx(relief);
    let dhy = dpdy(relief);
    if (o.uvrect.x > 0.0) {
        n = bump_normal(n, dpx, dpy, dhx, dhy);
    }
    let nmap = normal_xyz(textureSample(normal_tex, tex_samp, tuv));
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
    // the picture's soft distance in metres (0: it has none)
    @location(3) soft: f32,
};

@vertex
fn vs_particle(in: VsIn) -> ParticleOut {
    var out: ParticleOut;
    out.clip = frame.view_proj * vec4<f32>(in.pos, 1.0);
    out.uv = in.uv;
    out.col = in.col;
    out.a = in.tw;
    out.soft = objects[in.inst].mat.w;
    return out;
}

// Soft particles: a picture fades out over `soft` metres in front of the surface behind it, so that smoke and fire do not cut a hard line where they meet the ground or a wall. The distance is measured along the view ray, between
// the particle and the opaque thing the prepass saw there (the same depth deep water reads).
fn soft_fade(clip: vec4<f32>, soft: f32) -> f32 {
    if (soft <= 0.0) {
        return 1.0;
    }
    let px = vec2<i32>(clip.xy);
    let d = textureLoad(opaque_depth, px, 0);
    if (d <= 0.0) {
        return 1.0;
    }
    let uv = (vec2<f32>(px) + 0.5) * frame.p1.zw;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let behind = frame.inv_view_proj * vec4<f32>(ndc, d, 1.0);
    let here = frame.inv_view_proj * vec4<f32>(ndc, clip.z, 1.0);
    let thick = length(behind.xyz / behind.w - frame.eye.xyz) - length(here.xyz / here.w - frame.eye.xyz);
    let f = clamp(thick / soft, 0.0, 1.0);
    return f * f * (3.0 - 2.0 * f);
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
    let a = in.a * texel.a * soft_fade(in.clip, in.soft);
    if (a <= 0.0) {
        discard;
    }
    return vec4<f32>(particle_light(in.col, texel), a);
}

// Rain: every drop is a stretched quad along its fall, placed by the vertex shader alone (the drop's number is the instance, its place a hash of that number and the clock, wrapped round the camera so that a
// drop stays where it is in the world while the camera moves: the parallax is right, the drops are hidden behind what hides them by the depth test, and the cost is that of a few thousand quads). The
// camera's 40 x 22 x 40 m box holds the drops; they fade towards its edges so that none pops in. `frame.p26.z` is how hard it rains, `.w` the clock.
struct RainOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fade: f32,
};

fn rain_pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn rain_rand(i: u32, k: u32) -> f32 {
    return f32(rain_pcg(i * 4u + k) >> 8u) / 16777216.0;
}

@vertex
fn vs_rain(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> RainOut {
    var out: RainOut;
    let corner = array<vec2<f32>, 6>(vec2<f32>(-1.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0));
    let c = corner[vi];
    let size = vec3<f32>(40.0, 22.0, 40.0);
    let speed = 9.0 + 3.0 * rain_rand(ii, 3u);
    let dir = normalize(vec3<f32>(0.16, -1.0, 0.06));
    let t = frame.p26.w;
    // where the drop is: its place in the box plus its fall, wrapped round the camera (the box reaches 18 m above it and 6 m below)
    var p = vec3<f32>(rain_rand(ii, 0u), rain_rand(ii, 1u), rain_rand(ii, 2u)) * size + dir * speed * t;
    let cam = frame.eye.xyz;
    let centre = cam + vec3<f32>(0.0, 5.0, 0.0);
    let r = fract((p - centre) / size + vec3<f32>(0.5)) - vec3<f32>(0.5);
    p = centre + r * size;
    let to_eye = cam - p;
    let dist = length(to_eye);
    let len = 0.5 + 0.3 * rain_rand(ii, 3u);
    let width = max(0.011, dist * 0.0017);
    let along = p - dir * len * (1.0 - c.y);
    let side = normalize(cross(dir, to_eye / max(dist, 0.001)));
    let world = along + side * (c.x * width * 0.5);
    out.clip = frame.view_proj * vec4<f32>(world, 1.0);
    out.uv = vec2<f32>(c.x, c.y);
    // fade towards the edges of the box and the far distance; the nearest ones (a hand's breadth from the lens) are thinned out
    let edge = max(max(abs(r.x), abs(r.z)) * 2.0, abs(r.y) * 2.0);
    out.fade = (1.0 - smoothstep(0.62, 1.0, edge)) * smoothstep(0.4, 1.2, dist);
    return out;
}

@fragment
fn fs_rain(in: RainOut) -> @location(0) vec4<f32> {
    let across = 1.0 - in.uv.x * in.uv.x;
    // bright at the head (the lower end), the tail fading up the fall
    let along = smoothstep(0.0, 0.85, in.uv.y) * (1.0 - smoothstep(0.93, 1.0, in.uv.y));
    let a = across * across * along * in.fade * 0.55 * clamp(frame.p26.z * 1.5, 0.0, 1.0);
    if (a <= 0.003) {
        discard;
    }
    // lit by the sky (dull and grey by day, faint at night)
    let sky = frame.sky_horizon.rgb;
    let col = vec3<f32>(0.05) + sky * 0.9 + vec3<f32>(0.02, 0.025, 0.03);
    return vec4<f32>(col, a);
}

// Added light: the colour times the opacity, summed into the picture.
@fragment
fn fs_particle_add(in: ParticleOut) -> @location(0) vec4<f32> {
    let texel = textureSampleLevel(tex, tex_samp, vec2<f32>(in.uv.x, 1.0 - in.uv.y), 0.0);
    let a = in.a * texel.a * soft_fade(in.clip, in.soft);
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
    // single blades: least and greatest height, width at the foot, how much the blades of a clump share
    blade: vec4<f32>,
    // the bend, the size of a clump, the blades in a bundle, 1 when the ring is made of blades
    blade2: vec4<f32>,
    cbase: vec4<f32>,
    ctip: vec4<f32>,
    // x: 1 when the grass grows only on a layer of the terrain's paint
    mask: vec4<f32>,
};

@group(2) @binding(12) var<uniform> gu: GrassU;
@group(2) @binding(13) var grass_heights: texture_2d<f32>;
@group(2) @binding(14) var grass_mask: texture_2d<f32>;

// How much of the ground at (x, z) is the layer the grass grows on, 0 to 1 (the mask picture covers the terrain; its four nearest samples, blended).
fn grass_mask_at(x: f32, z: f32) -> f32 {
    let dim = vec2<f32>(textureDimensions(grass_mask));
    let u = clamp((x - (gu.tpos.x - gu.tsize.x * 0.5)) / gu.tsize.x, 0.0, 1.0);
    let v = clamp((z - (gu.tpos.z - gu.tsize.y * 0.5)) / gu.tsize.y, 0.0, 1.0);
    let f = vec2<f32>(u, v) * (dim - vec2<f32>(1.001));
    let i = vec2<i32>(floor(f));
    let t = f - floor(f);
    let a = textureLoad(grass_mask, i, 0).r;
    let b = textureLoad(grass_mask, i + vec2<i32>(1, 0), 0).r;
    let c = textureLoad(grass_mask, i + vec2<i32>(0, 1), 0).r;
    let d = textureLoad(grass_mask, i + vec2<i32>(1, 1), 0).r;
    return mix(mix(a, b, t.x), mix(c, d, t.x), t.y);
}

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

// Whether (x, z) is over the terrain's own heightmap.
fn grass_on_terrain(x: f32, z: f32) -> bool {
    let tu = (x - (gu.tpos.x - gu.tsize.x * 0.5)) / gu.tsize.x;
    let tv = (z - (gu.tpos.z - gu.tsize.y * 0.5)) / gu.tsize.y;
    return tu >= 0.0 && tu <= 1.0 && tv >= 0.0 && tv <= 1.0;
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

// The clump a place belongs to (the nearest of the jittered points of a lattice of `size` metres, looked for in the nine cells round it): x, y, z are the clump's three hashes, w the distance to its middle in clumps.
fn clump_at(p: vec2<f32>, size: f32, seed: f32) -> vec4<f32> {
    let g = p / size;
    let c0 = vec2<i32>(floor(g));
    var best = 1.0e9;
    var hc = vec3<f32>(0.0);
    for (var j = -1; j <= 1; j = j + 1) {
        for (var i = -1; i <= 1; i = i + 1) {
            let cc = c0 + vec2<i32>(i, j);
            let h = hash3(cc.x, cc.y, seed);
            let d = distance(g, vec2<f32>(f32(cc.x), f32(cc.y)) + h.xy);
            if (d < best) {
                best = d;
                hc = h;
            }
        }
    }
    return vec4<f32>(hc, best);
}

// The middle of the clump a place belongs to (the same nearest point `clump_at` finds), in metres.
fn clump_centre(p: vec2<f32>, size: f32, seed: f32) -> vec2<f32> {
    let g = p / size;
    let c0 = vec2<i32>(floor(g));
    var best = 1.0e9;
    var centre = vec2<f32>(0.0);
    for (var j = -1; j <= 1; j = j + 1) {
        for (var i = -1; i <= 1; i = i + 1) {
            let cc = c0 + vec2<i32>(i, j);
            let h = hash3(cc.x, cc.y, seed);
            let pt = vec2<f32>(f32(cc.x), f32(cc.y)) + h.xy;
            let d = distance(g, pt);
            if (d < best) {
                best = d;
                centre = pt;
            }
        }
    }
    return centre * size;
}

// Single blades (`GrassField` with no mesh): every vertex of a bundle works out its own blade (a hash of the cell and the blade's number), its clump, the cubic curve of the blade and its place along it. See docs/GPU.md, "Grass made of blades".
// The instances of a ring are numbered tile by tile (tiles of 16 x 16 cells, the tiles row by row, the cells of a tile row by row): the renderer leaves out the tiles that the view cannot see (a draw per run of
// neighbouring tiles that can be) instead of working out every tuft of the ring only to throw most of them away.
const GRASS_TILE: u32 = 16u;

fn grass_cell(inst: u32) -> vec2<i32> {
    let per_row = u32(gu.ring.w) / GRASS_TILE;
    let tile = inst / (GRASS_TILE * GRASS_TILE);
    let local = inst % (GRASS_TILE * GRASS_TILE);
    return vec2<i32>(i32((tile % per_row) * GRASS_TILE + local % GRASS_TILE), i32((tile / per_row) * GRASS_TILE + local / GRASS_TILE));
}

fn blade_vertex(in: VsIn, out_in: MeshOut) -> MeshOut {
    var out = out_in;
    let m = u32(gu.ring.w);
    let cell_xy = grass_cell(in.inst);
    let ix = i32(gu.orig.x) + cell_xy.x;
    let iz = i32(gu.orig.y) + cell_xy.y;
    let blade = u32(in.pos.z + 0.5);
    let side = in.pos.x;
    let t = in.pos.y;
    let cell = gu.ring.x;
    let seed = gu.tuft.w;
    let bid = i32(blade);
    let r = hash3(ix * 4 + bid, iz * 4 + bid * 3 + 1, seed);
    // the root: anywhere within a cell and a half of its lattice point (a lattice of single points would show as rows), then drawn towards the middle of its clump, so that grass grows in tufts with bare ground between them
    let q0 = vec2<f32>((f32(ix) + r.x * 2.2 - 0.6) * cell, (f32(iz) + r.y * 2.2 - 0.6) * cell);
    let tuft_at = clump_centre(q0, gu.blade2.y, seed + 41.0);
    let pulled = mix(q0, tuft_at, 0.62 * gu.blade.w * (0.4 + 0.6 * r.z));
    let px = pulled.x;
    let pz = pulled.y;
    // a blade grows only on its own terrain: beyond the heightmap's edge there is no ground to read (the look-ups clamp to the edge, which would hang a sheet of grass in the air over the next terrain)
    if (!grass_on_terrain(px, pz)) {
        return out;
    }
    let d = distance(vec2<f32>(px, pz), gu.eye.xz);
    var fade = smoothstep(gu.ring.z, gu.ring.z - gu.orig.w, d);
    if (gu.ring.y > 0.0) {
        fade = fade * smoothstep(gu.ring.y, gu.ring.y + gu.orig.w, d);
    }
    if (fade <= 0.01) {
        return out;
    }
    // grass only on its own kind of ground: the density follows the painted layer (a blade stays where its hash is under the layer's weight)
    var grown = 1.0;
    if (gu.mask.x > 0.5) {
        let m = grass_mask_at(px, pz);
        if (r.z > m * 1.15) {
            return out;
        }
        grown = smoothstep(0.05, 0.6, m);
    }
    let ground = grass_ground(px, pz);
    // the clump: its hashes set the shape and colour the blades of it share (Ghost of Tsushima's Voronoi clumps); a blade is the mix of its own randomness and its clump's
    let cl = clump_at(vec2<f32>(px, pz), gu.blade2.y, seed + 41.0);
    let rp = hash3(ix * 5 + bid + 11, iz * 3 + bid * 7 + 2, seed + 7.0);
    let xp = rp.x;
    let xc = cl.x;
    let c = gu.blade.w;
    let x = (1.0 - c) * xp + c * xc;
    // a few tall stems among the rest, and the edges of a clump a little shorter
    let tall = select(1.0, 1.6, rp.z > 0.93);
    let h = mix(gu.blade.x, gu.blade.y, x) * tall * fade * mix(0.55, 1.0, grown) * (1.0 - 0.25 * smoothstep(0.6, 1.0, cl.w));
    let widen = gu.tuft.x;
    let w0 = gu.blade.z * mix(0.8, 1.3, xp) * widen;
    // which way it faces: the clump has a heading, the blade strays from it
    let yaw = (xc * 0.33 + mix(-0.15, 0.15, xp) + select(0.0, 0.5, rp.y > 0.7)) * 6.2831853;
    let cy = cos(yaw);
    let sy = sin(yaw);
    let fwd = vec3<f32>(-sy, 0.0, cy);
    let right = vec3<f32>(cy, 0.0, sy);
    // how far it leans over: the clump's tendency and the blade's own (a cubic curve: nearly upright at the foot, bent over at the tip)
    let lean = gu.blade2.x * mix(0.25, 1.0, mix(xp, xc, 0.6));
    let p0 = vec3<f32>(0.0);
    let p1 = vec3<f32>(0.0, 0.45 * h, 0.05 * h * lean);
    let p2 = vec3<f32>(0.0, 0.82 * h * (1.0 - 0.3 * lean), 0.45 * h * lean);
    let p3 = vec3<f32>(0.0, h * (1.0 - 0.5 * lean), 0.82 * h * lean);
    let u = 1.0 - t;
    let curve = p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t);
    let tan_l = normalize(3.0 * u * u * (p1 - p0) + 6.0 * u * t * (p2 - p1) + 3.0 * t * t * (p3 - p2) + vec3<f32>(0.0, 1.0e-4, 0.0));
    // the wind: a gust that travels across the field and a flutter of the blade's own; the foot stays, the top leans (the square of the height along the blade)
    let dir = vec2<f32>(cos(gu.wind.z), sin(gu.wind.z));
    let sdir = vec2<f32>(-dir.y, dir.x);
    let k = 6.2831853 / gu.wind.y;
    let clock = gu.wind.w;
    let along = dot(vec2<f32>(px, pz), dir);
    let across = dot(vec2<f32>(px, pz), sdir);
    let gust = 0.5 + 0.5 * sin(along * k - clock + 0.8 * sin(across * k * 0.6 + clock * 0.2));
    let flutter = 0.22 * sin(clock * 4.1 + along * 0.9 + xp * 6.28) + 0.12 * sin(clock * 6.3 + rp.z * 6.28);
    let sway = t * t * gu.wind.x * h * (0.35 + 0.65 * gust);
    let shake = t * t * gu.wind.x * h * flutter * 0.4;
    // the ribbon: tapering from the foot to the tip
    let half = w0 * select(1.0 - 0.75 * t * t, 0.0, side == 0.0) * 0.5;
    var local = curve + right * (side * half);
    var wp = vec3<f32>(px, ground - 0.02, pz) + right * local.x + vec3<f32>(0.0, local.y, 0.0) + fwd * local.z;
    wp.x = wp.x + dir.x * sway + sdir.x * shake;
    wp.z = wp.z + dir.y * sway + sdir.y * shake;
    // a blade seen edge-on is widened a little in the screen's direction so it does not vanish (Ghost of Tsushima does the same)
    let view = normalize(wp - gu.eye.xyz);
    let face = normalize(cross(vec3<f32>(right.x, 0.0, right.z), tan_l));
    let edge_on = 1.0 - abs(dot(view, face));
    wp = wp + right * (side * w0 * 0.5 * edge_on * edge_on * 0.8);
    out.world = wp;
    out.clip = frame.view_proj * vec4<f32>(wp, 1.0);
    out.uv = in.uv;
    // normals: the face of the ribbon, bent outward across its width so that it is round, tilted a little towards the sky, and turned to face the eye (a blade is lit from both sides)
    var n = normalize(cross(right, tan_l));
    n = normalize(n + right * (side * 0.55));
    n = normalize(mix(n, vec3<f32>(0.0, 1.0, 0.0), 0.25));
    n = select(-n, n, dot(n, -view) > 0.0);
    // the colour: a gradient from the foot to the tip, darker at the foot (the ambient occlusion of a dense field: the root is shaded by the blades around it), the patches of lush and dry grass of the field and a share of the clump's own tint
    let ps = max(gu.tuft.z, 5.0);
    let n1 = 0.5 + 0.5 * sin(px / ps + sin(pz / (1.37 * ps)) * 2.0);
    let n2 = 0.5 + 0.5 * sin(pz / (0.83 * ps) - px / (1.9 * ps) + 1.3);
    let dry = smoothstep(0.45, 0.9, n1 * 0.65 + n2 * 0.35);
    let patch_tint = mix(gu.ca.rgb, gu.cb.rgb, clamp(cl.y * 0.35 + dry * 0.75, 0.0, 1.0));
    let ao = mix(0.22, 1.0, pow(t, 1.4));
    let body = mix(gu.cbase.rgb, gu.ctip.rgb, pow(t, 1.3));
    out.col = body * patch_tint * ao * mix(0.8, 1.2, xc);
    // the sun through the blade is added where the sun reaches it (`lit_tail`), in the length of the normal
    var back = 0.0;
    let sun = i32(frame.p4.x);
    if (sun >= 0) {
        let toward_sun = frame.lights[u32(sun)].vec.xyz;
        back = pow(clamp(dot(view, toward_sun), 0.0, 1.0), 3.0);
    }
    out.nrm = n * (1.0 + back * t);
    out.tw = 1.0;
    return out;
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
    if (gu.blade2.w > 0.5) {
        return blade_vertex(in, out);
    }
    let m = u32(gu.ring.w);
    let cell_xy = grass_cell(in.inst);
    let ix = i32(gu.orig.x) + cell_xy.x;
    let iz = i32(gu.orig.y) + cell_xy.y;
    let r = hash3(ix, iz, gu.tuft.w);
    let cell = gu.ring.x;
    let px = (f32(ix) + r.x) * cell;
    let pz = (f32(iz) + r.y) * cell;
    // a tuft grows only on its own terrain (the height look-ups clamp to the edge: beyond it they would hang a sheet of grass in the air over the next terrain)
    if (!grass_on_terrain(px, pz)) {
        return out;
    }
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
    out.col = tint * mix(0.4, 1.0, hn * 0.8 + 0.2 * gust);
    out.nrm = normalize(vec3<f32>(dir.x * lean * 0.8, 1.0, dir.y * lean * 0.8)) * (1.0 + back * hn);
    return out;
}
