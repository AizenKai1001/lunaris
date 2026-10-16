// Shared by every Lunaris shader: the per-frame uniform block and the colour helpers.
// Colours handed to the shaders are in a linear working space: gamma 2.0 in the classic mode (so that pictures match the reference CPU
// renderer), real sRGB in the high-quality mode.

const PI: f32 = 3.14159265358979;
const MAX_LIGHTS: u32 = 16u;

struct Light {
    // xyz: a direction towards the light (directional) or the position (point); w: 1 for a point light
    vec: vec4<f32>,
    // rgb: colour times intensity; w: the range of a point light
    color: vec4<f32>,
    // spot lights: xyz the direction it shines in, w the cosine of the outer angle (-2: not a spot)
    spot: vec4<f32>,
    // x: the cosine of the inner angle
    cone: vec4<f32>,
};

// The shadow map of one point light: six faces looking along +X, -X, +Y, -Y, +Z, -Z (stored as layers of a 2D array).
struct PointShadow {
    vp: array<mat4x4<f32>, 6>,
    // x: index of the light this belongs to (-1: unused), y: near plane, z: far plane (the light's range), w: the world size of one texel at distance 1
    info: vec4<f32>,
};

struct Frame {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    cascade_vp: array<mat4x4<f32>, 3>,
    // x, y, z: view distance at which each cascade ends; w: shadow map size in texels
    cascade_info: vec4<f32>,
    // world size of one shadow texel for each cascade
    cascade_texel: vec4<f32>,
    // metres of light-space depth of each cascade, and (w) the softness of the sun's shadows
    cascade_depth: vec4<f32>,
    // xyz: camera position; w: near plane
    eye: vec4<f32>,
    // xyz: unit vector towards the sun
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    sky_top: vec4<f32>,
    sky_horizon: vec4<f32>,
    ground: vec4<f32>,
    // rgb: fog colour; w: density
    fog: vec4<f32>,
    // rgb: the flat background colour (linear); the picture's alpha says how much of a pixel is something other than background
    clear: vec4<f32>,
    // ambient, exposure, ssao strength, shadows on
    p0: vec4<f32>,
    // width, height, 1 / width, 1 / height
    p1: vec4<f32>,
    // mode (0 classic, 1 high), light count, tonemap, saturation
    p2: vec4<f32>,
    // bloom strength, bloom threshold, vignette, draw the sky
    p3: vec4<f32>,
    // index of the light that casts the shadows (-1 none), encode to sRGB in the composite, samples per pixel, ortho camera
    p4: vec4<f32>,
    // cloud cover, size of a point-light shadow map in texels, 0, 0
    p5: vec4<f32>,
    // depth of field: strength, focus distance, the depth that stays sharp around it
    p6: vec4<f32>,
    // picture effects: light shafts, film grain, contrast, motion blur
    p7: vec4<f32>,
    // tint rgb, a number that changes every picture (animates the grain)
    p8: vec4<f32>,
    // colour grading: lift rgb + temperature, gamma rgb + tint shift, gain rgb, the fade colour (linear) + how much
    p9: vec4<f32>,
    p10: vec4<f32>,
    p11: vec4<f32>,
    p12: vec4<f32>,
    // height fog: density, falloff, the level of the thickest fog
    p13: vec4<f32>,
    // eye adaptation: the lowest and the highest exposure, how fast it adapts, the brightness it aims at
    p14: vec4<f32>,
    // eye adaptation: on (0/1), the seconds since the last picture, start from the measured value (0/1)
    p15: vec4<f32>,
    // screen-space reflections: strength, the farthest a ray goes, how thick things are taken to be, on (0/1)
    p16: vec4<f32>,
    // lens flare: intensity, how many ghosts, the halo's strength, on (0/1)
    p17: vec4<f32>,
    // reflection probes: how many are baked, how many mip levels their faces have
    p18: vec4<f32>,
    // the lens: distortion, zoom, chromatic fringe, soften; then block size, scan lines
    p19: vec4<f32>,
    p20: vec4<f32>,
    // volumetric light: density, anisotropy, steps, farthest; then colour and intensity
    p21: vec4<f32>,
    p22: vec4<f32>,
    // bounced light: intensity, radius, directions, thickness
    p23: vec4<f32>,
    // the share of the new bounced light in what is shown
    p24: vec4<f32>,
    // the sky of the air: turbidity, rayleigh, mie_g, intensity; then (x) on, (y) the glow of fog towards the sun
    p25: vec4<f32>,
    p26: vec4<f32>,
    // temporal anti-aliasing: the share of the new picture, and (y) 1 when it is on
    p27: vec4<f32>,
    // each probe: its centre (xyz), then half the size of its box (xyz)
    probes: array<vec4<f32>, 8>,
    // the view-projection matrix of the previous picture (motion blur)
    prev_view_proj: mat4x4<f32>,
    // the view-projection of this picture and of the last one, without the sub-pixel shift (temporal anti-aliasing)
    unj_vp: mat4x4<f32>,
    unj_prev_vp: mat4x4<f32>,
    psh: array<PointShadow, 2>,
    lights: array<Light, 16>,
};

fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn srgb_decode(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// A cheap hash for dithering and rotating sample patterns.
fn interleaved_noise(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

// One thing to draw (a mesh instance, a sprite, a tile map), indexed by the instance index.
struct Object {
    model: mat4x4<f32>,
    // the columns of the normal matrix (a 3x3 matrix padded to vec4)
    n0: vec4<f32>,
    n1: vec4<f32>,
    n2: vec4<f32>,
    // linear base colour and alpha
    color: vec4<f32>,
    // shininess, emissive, metallic, roughness
    mat: vec4<f32>,
    // uv scale x, uv scale y, uses a texture (0/1), uses vertex colours (0/1)
    misc: vec4<f32>,
    // meshes: the depth of the relief (world units); sprites: the part of the texture to show (u0, v0, du, dv), v from the bottom
    uvrect: vec4<f32>,
    // x: what the entity-id pass writes (the entity's slot, 0 is "nothing")
    ids: vec4<u32>,
    // `PbrMaps`: normal strength, occlusion strength, which maps (1 normal, 2 arm, 3 both; 0 none), roughness scale
    pbr: vec4<f32>,
};
