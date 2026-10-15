// HUD text: quads sampled from the 95-glyph font atlas (one texel per font pixel). Positions are in pixels from the top left of the picture.

struct TextU {
    size: vec4<f32>,
};

@group(0) @binding(0) var<uniform> text_u: TextU;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_samp: sampler;

struct VsIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_text(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = vec4<f32>(in.pos.x / text_u.size.x * 2.0 - 1.0, 1.0 - in.pos.y / text_u.size.y * 2.0, 0.0, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

@fragment
fn fs_text_id(in: VsOut) -> @location(0) u32 {
    let lit = textureSampleLevel(atlas, atlas_samp, in.uv, 0.0).r;
    if (lit < 0.5) {
        discard;
    }
    return u32(in.color.x + 0.5);
}

// Interface shapes: the bound texture (white for a plain colour, a picture, or a page of glyphs whose alpha is the coverage) times the vertex colour.
@fragment
fn fs_ui(in: VsOut) -> @location(0) vec4<f32> {
    let t = textureSampleLevel(atlas, atlas_samp, in.uv, 0.0);
    return t * in.color;
}

// The classic mode's gamma-2.0 working space: the picture's display colours are squared too.
@fragment
fn fs_ui_sq(in: VsOut) -> @location(0) vec4<f32> {
    let t = textureSampleLevel(atlas, atlas_samp, in.uv, 0.0);
    return vec4<f32>(t.rgb * t.rgb, t.a) * in.color;
}

@fragment
fn fs_text(in: VsOut) -> @location(0) vec4<f32> {
    let lit = textureSampleLevel(atlas, atlas_samp, in.uv, 0.0).r;
    if (lit < 0.5) {
        discard;
    }
    return in.color;
}
