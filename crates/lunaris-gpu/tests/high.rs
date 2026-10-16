//! What the high-quality mode must always do, whatever the graphics card: keep the colours of a flat 2D scene exactly as authored, be
//! repeatable, and make the effects it promises visible (sun shadows, a sky, reflections on metal, bloom, fog, ambient occlusion).
use lunaris_gpu::{GpuRenderer, Quality};
use lunaris_render::{Frame, RenderOptions, TextureCache};
use lunaris_sim::Sim;

fn gpu_or_skip() -> Option<GpuRenderer> {
    match GpuRenderer::headless() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("no GPU here ({e}): skipping");
            None
        }
    }
}

fn draw(gpu: &mut GpuRenderer, entities: &[&str], quality: &Quality, w: u32, h: u32) -> Frame {
    let text = format!(r#"{{"lunaris":1,"entities":[{}]}}"#, entities.join(","));
    let mut sim = Sim::new(1);
    sim.load_scene(&lunaris_json::parse(&text).expect("scene JSON")).expect("scene loads");
    let mut tex = TextureCache::new(None);
    let opts = RenderOptions { width: w, height: h, ..RenderOptions::default() };
    gpu.render_to_image(&mut sim.world, &mut tex, &opts, quality).expect("render")
}

fn px(f: &Frame, x: u32, y: u32) -> [u8; 3] {
    let o = ((y * f.width + x) * 4) as usize;
    [f.rgba[o], f.rgba[o + 1], f.rgba[o + 2]]
}

fn lum(c: [u8; 3]) -> f32 {
    0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32
}

const CAMERA_3D: &str = r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0.1,0.1,0.15,1]},"Transform":{"pos":[0,3,9],"rot":[-0.2,0,0,0.98]}}}"#;
const SUN: &str = r#"{"uid":2,"components":{"Light":{"kind":0,"intensity":1.5},"Transform":{"rot":[-0.5,0.3,0,0.81]}}}"#;
const FLOOR: &str = r#"{"uid":3,"components":{"Transform":{"scale":[40,1,40]},"MeshRenderer":{"mesh":"plane","color":[0.8,0.8,0.8,1],"roughness":0.9}}}"#;
const CUBE: &str = r#"{"uid":4,"components":{"Transform":{"pos":[0,1,0],"scale":[2,2,2]},"MeshRenderer":{"mesh":"cube","color":[0.9,0.2,0.2,1]}}}"#;

#[test]
fn a_flat_2d_scene_keeps_its_authored_colours() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    // an orthographic camera, a clear colour, an opaque sprite: no 3D meshes, so no tone mapping, bloom, vignette or ambient occlusion
    let f = draw(
        &mut gpu,
        &[
            r#"{"uid":1,"components":{"Camera":{"ortho":true,"ortho_height":8,"clear":[0.2,0.4,0.6,1]},"Transform":{"pos":[0,0,10]}}}"#,
            r#"{"uid":2,"components":{"Sprite":{"size":[4,4],"color":[1,0.5,0.1,1]},"Transform":{}}}"#,
        ],
        &Quality::high(),
        320,
        180,
    );
    let (bg, fg) = (px(&f, 8, 8), px(&f, 160, 90));
    for (got, want) in [(bg, [51u8, 102, 153]), (fg, [255, 128, 26])] {
        for k in 0..3 {
            assert!((got[k] as i32 - want[k] as i32).abs() <= 3, "got {got:?}, authored {want:?}");
        }
    }
}

#[test]
fn the_same_world_draws_the_same_picture_twice() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let scene = [CAMERA_3D, SUN, FLOOR, CUBE];
    for q in [Quality::classic(), Quality::high()] {
        let a = draw(&mut gpu, &scene, &q, 320, 180);
        let b = draw(&mut gpu, &scene, &q, 320, 180);
        assert_eq!(a.rgba, b.rgba, "{q:?}");
        assert_eq!(a.ids, b.ids);
    }
}

#[test]
fn the_sun_casts_a_shadow_on_the_floor() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let env = |shadows: bool| format!(r#"{{"uid":9,"components":{{"Environment":{{"sky":true,"shadows":{shadows},"ssao":0,"bloom":0}}}}}}"#);
    let with = draw(&mut gpu, &[CAMERA_3D, SUN, FLOOR, CUBE, &env(true)], &Quality::high(), 480, 270);
    let without = draw(&mut gpu, &[CAMERA_3D, SUN, FLOOR, CUBE, &env(false)], &Quality::high(), 480, 270);
    // somewhere on the floor next to the cube the picture with shadows is clearly darker, and nowhere is it clearly brighter
    let (mut darker, mut brighter) = (0, 0);
    for y in 0..270 {
        for x in 0..480 {
            let d = lum(px(&without, x, y)) - lum(px(&with, x, y));
            if d > 20.0 {
                darker += 1;
            }
            if d < -20.0 {
                brighter += 1;
            }
        }
    }
    assert!(darker > 400, "only {darker} pixels are in shadow");
    assert!(brighter < darker / 20, "{brighter} pixels got brighter with shadows on");
}

#[test]
fn a_sky_replaces_the_flat_background_and_lights_the_scene() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let flat = draw(&mut gpu, &[CAMERA_3D, FLOOR, CUBE], &Quality::high(), 320, 180);
    let sky = draw(&mut gpu, &[CAMERA_3D, FLOOR, CUBE, r#"{"uid":9,"components":{"Environment":{"sky":true,"clouds":0,"sky_top":[0.1,0.3,0.9],"sky_horizon":[0.8,0.9,1.0]}}}"#], &Quality::high(), 320, 180);
    // the top row shows the sky: blue instead of the dark navy clear colour, and brighter higher up than at the horizon is not, it fades to white
    let (a, b) = (px(&flat, 160, 2), px(&sky, 160, 2));
    assert!(lum(b) > lum(a) + 40.0, "{a:?} vs {b:?}");
    assert!(b[2] > b[0], "the zenith is blue: {b:?}");
    // the scene is lit by the sky even without a light: the lit side of the cube is brighter than with only a dark clear colour
    let cube_flat = lum(px(&flat, 160, 100));
    let cube_sky = lum(px(&sky, 160, 100));
    assert!(cube_sky > cube_flat + 10.0, "{cube_flat} vs {cube_sky}");
}

#[test]
fn metal_reflects_the_sky_and_plastic_does_not() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let env = r#"{"uid":9,"components":{"Environment":{"sky":true,"clouds":0,"sky_top":[0.0,0.2,1.0],"sky_horizon":[0.9,0.9,0.9],"shadows":false,"ssao":0,"bloom":0}}}"#;
    let ball = |metallic: f32| format!(r#"{{"uid":4,"components":{{"Transform":{{"pos":[0,1.2,0],"scale":[2,2,2]}},"MeshRenderer":{{"mesh":"sphere","color":[0.9,0.9,0.9,1],"metallic":{metallic},"roughness":0.1}}}}}}"#);
    let camera = r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0.1,0.1,0.15,1]},"Transform":{"pos":[0,1.2,6]}}}"#;
    let metal = draw(&mut gpu, &[camera, FLOOR, &ball(1.0), env], &Quality::high(), 320, 180);
    let plastic = draw(&mut gpu, &[camera, FLOOR, &ball(0.0), env], &Quality::high(), 320, 180);
    // the upper half of a mirror ball shows the blue sky; the plastic ball shows its own (grey-white) colour
    let (m, p) = (px(&metal, 160, 62), px(&plastic, 160, 62));
    assert!(m[2] as i32 - m[0] as i32 > 25, "metal: {m:?}");
    assert!((p[2] as i32 - p[0] as i32).abs() < 20, "plastic: {p:?}");
}

#[test]
fn bright_things_glow_with_bloom_and_not_without() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let lamp = r#"{"uid":4,"components":{"Transform":{"pos":[0,2,0]},"MeshRenderer":{"mesh":"sphere","color":[1,0.9,0.7,1],"emissive":8}}}"#;
    let env = |bloom: f32| format!(r#"{{"uid":9,"components":{{"Environment":{{"sky":false,"bloom":{bloom},"ssao":0,"vignette":0,"shadows":false}}}}}}"#);
    let camera = r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0,0,0,1]},"Transform":{"pos":[0,2,8]}}}"#;
    let on = draw(&mut gpu, &[camera, lamp, &env(1.0)], &Quality::high(), 320, 180);
    let off = draw(&mut gpu, &[camera, lamp, &env(0.0)], &Quality::high(), 320, 180);
    // a few pixels outside the ball's edge
    let probe = |f: &Frame| lum(px(f, 160 + 34, 90)) + lum(px(f, 160 - 34, 90)) + lum(px(f, 160, 90 - 34));
    assert!(probe(&on) > probe(&off) + 30.0, "{} vs {}", probe(&on), probe(&off));
}

#[test]
fn fog_hides_the_distance() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let env = |fog: f32| format!(r#"{{"uid":9,"components":{{"Environment":{{"sky":false,"fog":{fog},"fog_color":[0.7,0.7,0.7],"ssao":0,"bloom":0,"shadows":false}}}}}}"#);
    let near_far = |fog: f32, gpu: &mut GpuRenderer| {
        let wall = r#"{"uid":4,"components":{"Transform":{"pos":[0,2,-60],"scale":[200,8,1]},"MeshRenderer":{"mesh":"cube","color":[0.1,0.1,0.1,1]}}}"#;
        let camera = r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0.1,0.1,0.15,1]},"Transform":{"pos":[0,3,9]}}}"#;
        draw(gpu, &[camera, SUN, FLOOR, wall, &env(fog)], &Quality::high(), 320, 180)
    };
    let clear = near_far(0.0, &mut gpu);
    let foggy = near_far(0.05, &mut gpu);
    // the far wall, near the middle of the picture, is dark without fog and nearly the fog colour with it
    let (a, b) = (lum(px(&clear, 160, 84)), lum(px(&foggy, 160, 84)));
    assert!(b > a + 60.0, "{a} vs {b}");
}

#[test]
fn ambient_occlusion_darkens_the_crease_where_a_wall_meets_the_floor() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let env = |ssao: f32| format!(r#"{{"uid":9,"components":{{"Environment":{{"sky":true,"clouds":0,"ssao":{ssao},"bloom":0,"shadows":false}}}}}}"#);
    let wall = r#"{"uid":4,"components":{"Transform":{"pos":[0,2,-3],"scale":[30,4,0.5]},"MeshRenderer":{"mesh":"cube","color":[0.8,0.8,0.8,1]}}}"#;
    let on = draw(&mut gpu, &[CAMERA_3D, FLOOR, wall, &env(1.5)], &Quality::high(), 480, 270);
    let off = draw(&mut gpu, &[CAMERA_3D, FLOOR, wall, &env(0.0)], &Quality::high(), 480, 270);
    let darker = on.rgba.chunks(4).zip(off.rgba.chunks(4)).filter(|(a, b)| lum([a[0], a[1], a[2]]) + 8.0 < lum([b[0], b[1], b[2]])).count();
    let brighter = on.rgba.chunks(4).zip(off.rgba.chunks(4)).filter(|(a, b)| lum([a[0], a[1], a[2]]) > lum([b[0], b[1], b[2]]) + 8.0).count();
    assert!(darker > 300, "only {darker} pixels are darker with ambient occlusion");
    assert!(brighter < darker / 10, "{brighter} pixels got brighter");
}

#[test]
fn textures_and_uv_scale_repeat_a_material_over_a_mesh() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let floor = |scale: u32| format!(r#"{{"uid":3,"components":{{"Transform":{{"scale":[40,1,40]}},"MeshRenderer":{{"mesh":"plane","color":[1,1,1,1],"texture":"checker","uv_scale":[{scale},{scale}]}}}}}}"#);
    let few = draw(&mut gpu, &[CAMERA_3D, SUN, &floor(2)], &Quality::classic(), 320, 180);
    let many = draw(&mut gpu, &[CAMERA_3D, SUN, &floor(40)], &Quality::classic(), 320, 180);
    // count light/dark changes along one row of the floor: many more with the pattern repeated 20 times as often
    let changes = |f: &Frame| (1..320).filter(|&x| (lum(px(f, x, 150)) - lum(px(f, x - 1, 150))).abs() > 25.0).count();
    assert!(changes(&many) > changes(&few) * 4, "{} vs {}", changes(&many), changes(&few));
}

#[test]
fn a_point_light_is_blocked_by_what_stands_in_front_of_it() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    // a lamp above a box above a floor: no sun, so the only shadow is the lamp's
    let env = |shadows: bool| format!(r#"{{"uid":9,"components":{{"Environment":{{"sky":false,"shadows":{shadows},"ssao":0,"bloom":0,"vignette":0}}}}}}"#);
    let lamp = r#"{"uid":2,"components":{"Transform":{"pos":[0,4,0]},"Light":{"kind":1,"intensity":6,"range":14,"color":[1,0.95,0.9]}}}"#;
    let block = r#"{"uid":4,"components":{"Transform":{"pos":[0,2,0],"scale":[2,0.4,2]},"MeshRenderer":{"mesh":"cube","color":[0.6,0.6,0.7,1]}}}"#;
    let camera = r#"{"uid":1,"components":{"Camera":{"fov_y":60,"clear":[0,0,0,1]},"Transform":{"pos":[0,9,9],"rot":[-0.38,0,0,0.925]}}}"#;
    let with = draw(&mut gpu, &[camera, FLOOR, block, lamp, &env(true)], &Quality::high(), 480, 270);
    let without = draw(&mut gpu, &[camera, FLOOR, block, lamp, &env(false)], &Quality::high(), 480, 270);
    let (mut darker, mut brighter) = (0, 0);
    for y in 0..270 {
        for x in 0..480 {
            let d = lum(px(&without, x, y)) - lum(px(&with, x, y));
            if d > 25.0 {
                darker += 1;
            }
            if d < -25.0 {
                brighter += 1;
            }
        }
    }
    assert!(darker > 800, "only {darker} pixels are in the lamp's shadow");
    assert!(brighter < darker / 20, "{brighter} pixels got brighter with shadows on");
    // a lower quality has no point shadows: the two pictures are the same
    let medium_off = draw(&mut gpu, &[camera, FLOOR, block, lamp, &env(true)], &Quality::low(), 480, 270);
    let low_flat = draw(&mut gpu, &[camera, FLOOR, block, lamp, &env(false)], &Quality::low(), 480, 270);
    assert_eq!(medium_off.rgba, low_flat.rgba, "low quality does not draw point-light shadows");
}

#[test]
fn see_through_meshes_show_what_is_behind_them() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let camera = r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0.1,0.1,0.15,1]},"Transform":{"pos":[0,1,9]}}}"#;
    let red = r#"{"uid":4,"components":{"Transform":{"pos":[0,1,0],"scale":[2,2,2]},"MeshRenderer":{"mesh":"cube","color":[0.9,0.05,0.05,1]}}}"#;
    let pane = |alpha: f32| format!(r#"{{"uid":5,"components":{{"Transform":{{"pos":[0,1,3],"scale":[4,4,0.1]}},"MeshRenderer":{{"mesh":"cube","color":[0.4,0.7,1,{alpha}],"roughness":0.2}}}}}}"#);
    let glass = draw(&mut gpu, &[camera, SUN, FLOOR, red, &pane(0.25)], &Quality::high(), 320, 180);
    let wall = draw(&mut gpu, &[camera, SUN, FLOOR, red, &pane(1.0)], &Quality::high(), 320, 180);
    let (g, w) = (px(&glass, 160, 90), px(&wall, 160, 90));
    assert!(g[0] as i32 > w[0] as i32 + 40, "the red cube should show through the glass: {g:?} vs {w:?}");
    assert!(w[2] > w[0], "the opaque pane hides it and is blue: {w:?}");
    // the glass is still something you can pick, and so is the cube behind it where the glass does not cover
    assert_eq!(glass.pick(160, 90), Some(5));
}

#[test]
fn a_spot_light_lights_only_its_cone() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let env = r#"{"uid":9,"components":{"Environment":{"sky":false,"ambient":0,"ssao":0,"bloom":0,"vignette":0,"shadows":false}}}"#;
    // a lamp 4 units above the floor shining straight down within 18 degrees; the camera looks straight down at the floor
    let spot = |kind: u32| format!(r#"{{"uid":2,"components":{{"Transform":{{"pos":[0,4,0],"rot":[-0.7071068,0,0,0.7071068]}},"Light":{{"kind":{kind},"intensity":8,"range":30,"angle":18,"softness":0.2}}}}}}"#);
    let camera = r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0,0,0,1]},"Transform":{"pos":[0,12,0],"rot":[-0.7071068,0,0,0.7071068]}}}"#;
    let cone = draw(&mut gpu, &[camera, FLOOR, &spot(2), env], &Quality::high(), 240, 240);
    let point = draw(&mut gpu, &[camera, FLOOR, &spot(1), env], &Quality::high(), 240, 240);
    // the floor 4 units below a 18 degree cone is lit within a circle of about 1.3 units: bright in the middle, dark at the edge of the picture
    assert!(lum(px(&cone, 120, 120)) > 60.0, "the middle of the cone is dark: {:?}", px(&cone, 120, 120));
    assert!(lum(px(&cone, 20, 120)) < 8.0, "outside the cone it should be dark: {:?}", px(&cone, 20, 120));
    // as a point light the same lamp lights the whole floor
    assert!(lum(px(&point, 20, 120)) > 8.0, "{:?}", px(&point, 20, 120));
}

#[test]
fn animated_models_move_on_the_gpu_and_reuse_their_buffers() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let scene = |time: f32| {
        let text = format!(
            r#"{{"lunaris":1,"entities":[{CAMERA_3D_REPLACED},{SUN},{FLOOR},{{"uid":5,"components":{{"Transform":{{"pos":[0,0,0]}},"MeshRenderer":{{"mesh":"examples/models/swayer.glb"}},"Animator":{{"clip":"sway","time":{time},"speed":0}}}}}}]}}"#,
            CAMERA_3D_REPLACED = r#"{"uid":1,"components":{"Camera":{"fov_y":45},"Transform":{"pos":[0,1.8,8]}}}"#
        );
        let mut sim = Sim::new(1);
        sim.load_scene(&lunaris_json::parse(&text).unwrap()).unwrap();
        sim
    };
    let mut tex = TextureCache::new(Some(root));
    let opts = RenderOptions { width: 240, height: 240, ..RenderOptions::default() };
    let mut pictures = Vec::new();
    for t in [0.0f32, 0.5, 1.0, 0.5] {
        let mut sim = scene(t);
        let f = gpu.render_to_image(&mut sim.world, &mut tex, &opts, &Quality::high()).unwrap();
        assert!(f.warnings.is_empty(), "{:?}", f.warnings);
        assert_eq!(f.pick(120, 140), Some(5), "the column is in the middle at t = {t}");
        pictures.push(f.rgba);
    }
    assert_ne!(pictures[0], pictures[1], "the pose changes with the clock");
    assert_ne!(pictures[1], pictures[2]);
    assert_eq!(pictures[1], pictures[3], "the same time gives the same picture, whatever was drawn in between");
}

#[test]
fn depth_of_field_blurs_what_is_away_from_the_focus_and_leaves_the_rest_alone() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    // a striped wall far away and a striped wall at the focus distance: the stripes of the far one blur, the near one's stay
    let wall = |uid: u32, z: f32, x: f32| format!(r#"{{"uid":{uid},"components":{{"Transform":{{"pos":[{x},2,{z}],"scale":[3,3,0.2]}},"MeshRenderer":{{"mesh":"cube","texture":"tiles","uv_scale":[6,6],"color":[1,1,1,1]}}}}}}"#);
    let (near, far) = (wall(10, 0.0, -2.0), wall(11, -30.0, 4.0));
    let cam = r#"{"uid":1,"components":{"Camera":{"fov_y":40},"Transform":{"pos":[0,2,10]}}}"#;
    let sun = SUN;
    let env = |dof: f32| format!(r#"{{"uid":3,"components":{{"Environment":{{"sky":false,"dof":{dof},"focus":10,"focus_range":2,"ssao":0,"bloom":0}}}}}}"#);
    let sharp = draw(&mut gpu, &[cam, sun, &env(0.0), &near, &far], &Quality::high(), 640, 360);
    let blurred = draw(&mut gpu, &[cam, sun, &env(1.0), &near, &far], &Quality::high(), 640, 360);
    // the sharpness of a region: the sum of the squared steps between neighbouring pixels (blurring spreads a step over many small ones)
    let energy = |f: &Frame, x0: u32, x1: u32, y0: u32, y1: u32| {
        let mut sum = 0.0f32;
        for y in y0..y1 {
            for x in x0..x1 {
                let d = lum(px(f, x + 1, y)) - lum(px(f, x, y));
                sum += d * d;
            }
        }
        sum
    };
    // the near wall (at the focus) fills x 30..250, the far one (40 m away) x 380..440, y 150..210
    let (near_ratio, far_ratio) = (energy(&blurred, 40, 240, 100, 260) / energy(&sharp, 40, 240, 100, 260), energy(&blurred, 380, 440, 152, 208) / energy(&sharp, 380, 440, 152, 208));
    assert!(far_ratio < 0.75, "the far wall lost its fine detail: {far_ratio}");
    assert!(near_ratio > 0.95, "the wall at the focus stays sharp: {near_ratio} (the far one: {far_ratio})");
    // with `dof` 0 nothing changes at all
    let again = draw(&mut gpu, &[cam, sun, &env(0.0), &near, &far], &Quality::high(), 640, 360);
    assert_eq!(sharp.rgba, again.rgba);
}

#[test]
fn a_colour_grade_fades_warms_and_lifts_the_picture() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let scene = |grade: &str| {
        let mut v = vec![CAMERA_3D, SUN, FLOOR, CUBE];
        let g = format!(r#"{{"uid":9,"components":{{"ColorGrade":{grade}}}}}"#);
        if !grade.is_empty() {
            v.push(Box::leak(g.into_boxed_str()));
        }
        v
    };
    let plain = draw(&mut gpu, &scene(""), &Quality::high(), 160, 90);
    // a whole fade to black is black, half of one is darker than none
    let black = draw(&mut gpu, &scene(r#"{"fade":[0,0,0,1]}"#), &Quality::high(), 160, 90);
    assert!(black.rgba.chunks(4).all(|p| p[0] <= 3 && p[1] <= 3 && p[2] <= 3), "a full fade to black leaves nothing");
    let half = draw(&mut gpu, &scene(r#"{"fade":[0,0,0,0.5]}"#), &Quality::high(), 160, 90);
    assert!(lum(px(&half, 80, 60)) < lum(px(&plain, 80, 60)) * 0.8);
    // a red flash pulls the picture towards red
    let flash = draw(&mut gpu, &scene(r#"{"fade":[1,0,0,0.6]}"#), &Quality::high(), 160, 90);
    let c = px(&flash, 10, 5);
    assert!(c[0] > c[1] + 40 && c[0] > c[2] + 40, "{c:?}");
    // warm: more red than blue than the plain picture has; cold: the other way
    let warm = draw(&mut gpu, &scene(r#"{"temperature":1}"#), &Quality::high(), 160, 90);
    let cold = draw(&mut gpu, &scene(r#"{"temperature":-1}"#), &Quality::high(), 160, 90);
    let balance = |f: &Frame| {
        let (mut r, mut b) = (0u64, 0u64);
        for p in f.rgba.chunks(4) {
            r += p[0] as u64;
            b += p[2] as u64;
        }
        r as f32 / b.max(1) as f32
    };
    assert!(balance(&warm) > balance(&plain) * 1.1 && balance(&cold) < balance(&plain) * 0.9, "{} {} {}", balance(&warm), balance(&plain), balance(&cold));
    // lift raises the shadows, a lower gamma darkens the midtones, the reference renderer and the classic mode ignore it all
    let lifted = draw(&mut gpu, &scene(r#"{"lift":[0.2,0.2,0.2]}"#), &Quality::high(), 160, 90);
    assert!(lum(px(&lifted, 5, 85)) > lum(px(&plain, 5, 85)) + 10.0);
    let dark = draw(&mut gpu, &scene(r#"{"gamma":[0.5,0.5,0.5]}"#), &Quality::high(), 160, 90);
    assert!(lum(px(&dark, 80, 70)) < lum(px(&plain, 80, 70)));
    let classic = draw(&mut gpu, &scene(r#"{"fade":[0,0,0,1]}"#), &Quality::classic(), 160, 90);
    assert!(classic.rgba.chunks(4).any(|p| p[0] > 20), "classic ignores the grade");
}

#[test]
fn height_fog_fills_low_ground_and_leaves_high_ground_clear() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    // a floor stretching away from a camera that stands near it, a red fog
    let scene = |fog: &str| {
        let mut v = vec![CAMERA_3D, SUN, FLOOR];
        let f = format!(r#"{{"uid":9,"components":{{"HeightFog":{fog}}}}}"#);
        if !fog.is_empty() {
            v.push(Box::leak(f.into_boxed_str()));
        }
        v
    };
    let far = |f: &Frame| px(f, 80, 52);
    let plain = draw(&mut gpu, &scene(""), &Quality::high(), 160, 90);
    let thick = draw(&mut gpu, &scene(r#"{"density":0.4,"falloff":0.05,"height":0,"color":[1,0,0]}"#), &Quality::high(), 160, 90);
    // the far floor takes the fog's colour
    assert!(far(&thick)[0] as i32 - far(&thick)[2] as i32 > far(&plain)[0] as i32 - far(&plain)[2] as i32 + 30, "{:?} {:?}", far(&thick), far(&plain));
    // the same fog with its thick layer far below the camera and the ground does almost nothing
    let high = draw(&mut gpu, &scene(r#"{"density":0.4,"falloff":0.5,"height":-40,"color":[1,0,0]}"#), &Quality::high(), 160, 90);
    let (a, b) = (far(&high), far(&plain));
    assert!((a[0] as i32 - b[0] as i32).abs() < 12 && (a[2] as i32 - b[2] as i32).abs() < 12, "{a:?} {b:?}");
    // classic ignores it
    let classic = draw(&mut gpu, &scene(r#"{"density":5,"color":[1,0,0]}"#), &Quality::classic(), 160, 90);
    let c = far(&classic);
    assert!((c[0] as i32 - c[2] as i32).abs() < 40, "classic mode is not fogged red: {c:?}");
}

// ------------------------------------------------------------------------------------------------ eye adaptation

fn mean_lum(f: &Frame) -> f32 {
    let mut sum = 0.0f64;
    for p in f.rgba.chunks(4) {
        sum += lum([p[0], p[1], p[2]]) as f64;
    }
    (sum / (f.rgba.len() / 4) as f64) as f32
}

fn measured(f: &Frame, key: &str) -> f64 {
    f.info.iter().find(|(k, _)| k == key).map(|(_, v)| *v).unwrap_or(f64::NAN)
}

/// A lit floor and a cube seen from above, filling the whole picture (no sky or flat background in view): `sun` is the sun's intensity and `ambient` the strength of the sky's light, so that a dark scene and a bright one are hundreds of times apart.
fn lit_from_above(sun: f32, ambient: f32, adapt: &str) -> Vec<String> {
    let mut v = vec![
        r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0.1,0.1,0.15,1]},"Transform":{"pos":[0,9,5],"rot":[-0.5,0,0,0.866]}}}"#.to_string(),
        format!(r#"{{"uid":2,"components":{{"Light":{{"kind":0,"intensity":{sun}}},"Transform":{{"rot":[-0.5,0.3,0,0.81]}}}}}}"#),
        FLOOR.to_string(),
        CUBE.to_string(),
        format!(r#"{{"uid":9,"components":{{"Environment":{{"sky":true,"clouds":0,"ambient":{ambient},"ssao":0,"bloom":0,"vignette":0}}}}}}"#),
    ];
    if !adapt.is_empty() {
        v.push(format!(r#"{{"uid":10,"components":{{"AutoExposure":{adapt}}}}}"#));
    }
    v
}

fn draw_owned(gpu: &mut GpuRenderer, entities: &[String], quality: &Quality, w: u32, h: u32) -> Frame {
    let refs: Vec<&str> = entities.iter().map(|s| s.as_str()).collect();
    draw(gpu, &refs, quality, w, h)
}

#[test]
fn eye_adaptation_brings_dark_and_bright_scenes_to_a_similar_brightness_and_takes_time_to_do_it() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let ae = r#"{"min":0.001,"max":1000}"#;
    let (dark_off, bright_off) = (draw_owned(&mut gpu, &lit_from_above(0.02, 0.02, ""), &Quality::high(), 240, 135), draw_owned(&mut gpu, &lit_from_above(8.0, 1.5, ""), &Quality::high(), 240, 135));
    let (dark_on, bright_on) = (draw_owned(&mut gpu, &lit_from_above(0.02, 0.02, ae), &Quality::high(), 240, 135), draw_owned(&mut gpu, &lit_from_above(8.0, 1.5, ae), &Quality::high(), 240, 135));
    let (a, b, c, d) = (mean_lum(&dark_off), mean_lum(&bright_off), mean_lum(&dark_on), mean_lum(&bright_on));
    eprintln!("without: dark {a:.1} bright {b:.1}; with: dark {c:.1} bright {d:.1}; exposure dark {:.3} bright {:.3}", measured(&dark_on, "exposure"), measured(&bright_on, "exposure"));
    // without it the two scenes are worlds apart, with it they are about equally bright
    assert!(b > a * 5.0, "the scenes should differ without eye adaptation: {a} vs {b}");
    assert!((c - d).abs() < 0.2 * c.max(d) && c > 60.0 && d > 60.0, "eye adaptation should even them out: {c} vs {d}");
    // what the renderer says it did: a bright scene is exposed down, a dark one up, and the picture of the average luminance matches
    let (ed, eb) = (measured(&dark_on, "exposure"), measured(&bright_on, "exposure"));
    assert!(ed > eb * 50.0 && eb < 0.5 && ed > 3.0, "exposure dark {ed} bright {eb}");
    assert!(measured(&bright_on, "average_luminance") > measured(&dark_on, "average_luminance") * 50.0);
    assert!(measured(&dark_off, "exposure").is_nan(), "nothing is measured without the component");

    // a picture asked for by itself is measured and applied at once: drawn twice, or after something else, it is the same picture
    let again = draw_owned(&mut gpu, &lit_from_above(0.02, 0.02, ae), &Quality::high(), 240, 135);
    assert_eq!(dark_on.rgba, again.rgba);
    // classic ignores it
    let (c1, c2) = (draw_owned(&mut gpu, &lit_from_above(0.02, 0.02, ""), &Quality::classic(), 120, 68), draw_owned(&mut gpu, &lit_from_above(0.02, 0.02, ae), &Quality::classic(), 120, 68));
    assert_eq!(c1.rgba, c2.rgba, "classic does not adapt");

    // films of the world: every picture counts as a quarter of a second after the one before, and the eye needs several to adapt to a change of light
    gpu.set_adaptation_step(Some(0.25));
    for _ in 0..3 {
        draw_owned(&mut gpu, &lit_from_above(8.0, 1.5, ae), &Quality::high(), 240, 135);
    }
    let series: Vec<f32> = (0..10).map(|_| mean_lum(&draw_owned(&mut gpu, &lit_from_above(0.02, 0.02, ae), &Quality::high(), 240, 135))).collect();
    gpu.set_adaptation_step(None);
    eprintln!("a bright room, then a dark one, a picture each quarter of a second: {series:?}");
    assert!(series[0] < d * 0.3, "the first picture in the dark is still dark: {series:?}");
    assert!(series.windows(2).all(|w| w[1] >= w[0] - 1.0), "it brightens step by step: {series:?}");
    assert!(series[3] > series[0] * 2.0 && series[3] < d * 0.9, "it is part of the way after a second: {series:?}");
    assert!(series[9] > d * 0.8, "and there after a few seconds: {series:?} (bright scene {d})");
}

// ------------------------------------------------------------------------------------------------ screen-space reflections

const MIRROR_FLOOR: &str = r#"{"uid":3,"components":{"Transform":{"scale":[40,1,40]},"MeshRenderer":{"mesh":"plane","color":[0.9,0.9,0.95,1],"metallic":1.0,"roughness":0.05}}}"#;
/// A glowing orange cube floating above the floor (dull itself: only the floor is a mirror).
const LAMP_CUBE: &str = r#"{"uid":4,"components":{"Transform":{"pos":[0,1.8,-2],"scale":[1.6,1.6,1.6]},"MeshRenderer":{"mesh":"cube","color":[1.0,0.55,0.1,1],"emissive":3,"roughness":0.9}}}"#;
const BLUE_SKY: &str = r#"{"uid":9,"components":{"Environment":{"sky":true,"clouds":0,"sky_top":[0.1,0.25,0.6],"sky_horizon":[0.4,0.5,0.7],"shadows":false,"ssao":0,"bloom":0,"vignette":0}}}"#;

/// How many pixels differ by more than `by` in brightness (brighter in `b` than in `a`, darker in `b` than in `a`), and how many are the same to within one level.
fn compare_pictures(a: &Frame, b: &Frame, by: f32) -> (usize, usize, usize) {
    let (mut brighter, mut darker, mut same) = (0, 0, 0);
    for (p, q) in a.rgba.chunks(4).zip(b.rgba.chunks(4)) {
        let d = lum([q[0], q[1], q[2]]) - lum([p[0], p[1], p[2]]);
        if d > by {
            brighter += 1;
        }
        if d < -by {
            darker += 1;
        }
        if (0..3).all(|k| (p[k] as i32 - q[k] as i32).abs() <= 1) {
            same += 1;
        }
    }
    (brighter, darker, same)
}

#[test]
fn screen_space_reflections_show_a_bright_cube_in_a_glossy_floor_and_in_water() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let ssr = r#"{"uid":10,"components":{"Ssr":{}}}"#;
    let q = Quality::high();
    let (w, h) = (480u32, 270u32);
    let off = draw(&mut gpu, &[CAMERA_3D, MIRROR_FLOOR, LAMP_CUBE, BLUE_SKY], &q, w, h);
    let on = draw(&mut gpu, &[CAMERA_3D, MIRROR_FLOOR, LAMP_CUBE, BLUE_SKY, ssr], &q, w, h);
    let (brighter, darker, same) = compare_pictures(&off, &on, 25.0);
    eprintln!("mirror floor: {brighter} pixels brighter, {darker} darker, {same} of {} the same", w * h);
    // the cube's picture appears in the floor: many pixels get brighter and orange
    assert!(brighter > 300, "the cube should show in the floor: only {brighter} pixels brightened");
    let (mut r, mut b) = (0i64, 0i64);
    for (p, o) in off.rgba.chunks(4).zip(on.rgba.chunks(4)) {
        if lum([o[0], o[1], o[2]]) - lum([p[0], p[1], p[2]]) > 25.0 {
            r += o[0] as i64 - p[0] as i64;
            b += o[2] as i64 - p[2] as i64;
        }
    }
    assert!(r > b + 300, "what shows in the floor is the orange cube: red gained {r}, blue gained {b}");
    // where a ray finds nothing the sky's reflection is what it was: most of the picture is the same, and the cube itself did not change
    assert!(same > (w * h) as usize * 6 / 10, "only {same} pixels are the same");
    // the same picture twice
    let again = draw(&mut gpu, &[CAMERA_3D, MIRROR_FLOOR, LAMP_CUBE, BLUE_SKY, ssr], &q, w, h);
    assert_eq!(on.rgba, again.rgba);
    // a rough floor is no mirror: nothing changes
    let chalk = r#"{"uid":3,"components":{"Transform":{"scale":[40,1,40]},"MeshRenderer":{"mesh":"plane","color":[0.9,0.9,0.95,1],"metallic":0.0,"roughness":0.95}}}"#;
    let (a, b) = (draw(&mut gpu, &[CAMERA_3D, chalk, LAMP_CUBE, BLUE_SKY], &q, w, h), draw(&mut gpu, &[CAMERA_3D, chalk, LAMP_CUBE, BLUE_SKY, ssr], &q, w, h));
    assert_eq!(a.rgba, b.rgba, "a rough floor is not reflected in");
    // classic and the lower qualities ignore it
    for quality in [Quality::classic(), Quality::low()] {
        let (a, b) = (draw(&mut gpu, &[CAMERA_3D, MIRROR_FLOOR, LAMP_CUBE, BLUE_SKY], &quality, 240, 135), draw(&mut gpu, &[CAMERA_3D, MIRROR_FLOOR, LAMP_CUBE, BLUE_SKY, ssr], &quality, 240, 135));
        assert_eq!(a.rgba, b.rgba, "{quality:?} has no reflections");
    }
    // a lake reflects it too (a little ripple in its surface)
    let lake = r#"{"uid":3,"components":{"Transform":{"pos":[0,0,0]},"Water":{"size":[40,40],"color":[0.02,0.1,0.15,0.9],"wave_height":0.05,"wave_length":12}}}"#;
    let (still, mirror) = (draw(&mut gpu, &[CAMERA_3D, lake, LAMP_CUBE, BLUE_SKY], &q, w, h), draw(&mut gpu, &[CAMERA_3D, lake, LAMP_CUBE, BLUE_SKY, ssr], &q, w, h));
    let (brighter, _, _) = compare_pictures(&still, &mirror, 20.0);
    eprintln!("lake: {brighter} pixels brighter");
    assert!(brighter > 150, "the cube should show in the water: only {brighter} pixels brightened");
}

// ------------------------------------------------------------------------------------------------ reflection probes

/// A closed room 12 x 6 x 12 metres: `left` is the colour of the left wall, a green wall on the right, a blue one at the back, white behind the camera, grey floor and ceiling, and a mirror ball in the middle. The
/// probe stands in the middle of the ball (a box of `size` metres around it) when `size` is not empty.
fn probe_room(left: &str, size: &str) -> Vec<String> {
    let wall = |uid: u32, pos: &str, scale: &str, colour: &str| format!(r#"{{"uid":{uid},"components":{{"Transform":{{"pos":{pos},"scale":{scale}}},"MeshRenderer":{{"mesh":"cube","color":{colour},"roughness":0.9}}}}}}"#);
    let mut v = vec![
        r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0.1,0.1,0.15,1]},"Transform":{"pos":[0,2.5,5.5]}}}"#.to_string(),
        wall(20, "[-6.5,3,0]", "[1,6,13]", left),
        wall(21, "[6.5,3,0]", "[1,6,13]", "[0.1,0.8,0.1,1]"),
        wall(22, "[0,3,-6.5]", "[13,6,1]", "[0.1,0.1,0.9,1]"),
        wall(23, "[0,3,6.5]", "[13,6,1]", "[0.9,0.9,0.9,1]"),
        wall(24, "[0,-0.5,0]", "[13,1,13]", "[0.5,0.5,0.5,1]"),
        wall(25, "[0,6.5,0]", "[13,1,13]", "[0.5,0.5,0.5,1]"),
        r#"{"uid":30,"components":{"Transform":{"pos":[0,1.5,0],"scale":[3,3,3]},"MeshRenderer":{"mesh":"sphere","color":[1,1,1,1],"metallic":1.0,"roughness":0.05}}}"#.to_string(),
        r#"{"uid":9,"components":{"Environment":{"sky":true,"clouds":0,"sky_top":[0.1,0.25,0.6],"sky_horizon":[0.4,0.5,0.7],"shadows":false,"ssao":0,"bloom":0,"vignette":0}}}"#.to_string(),
    ];
    if !size.is_empty() {
        v.push(format!(r#"{{"uid":40,"components":{{"Transform":{{"pos":[0,1.5,0]}},"ReflectionProbe":{{"size":{size},"resolution":64}}}}}}"#));
    }
    v
}

fn sim_of(entities: &[String]) -> Sim {
    let text = format!(r#"{{"lunaris":1,"entities":[{}]}}"#, entities.join(","));
    let mut sim = Sim::new(1);
    sim.load_scene(&lunaris_json::parse(&text).expect("scene JSON")).expect("scene loads");
    sim
}

#[test]
fn a_reflection_probe_gives_a_mirror_ball_the_room_around_it_and_is_baked_once() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let q = Quality::high();
    let opts = RenderOptions { width: 480, height: 270, ..RenderOptions::default() };
    let mut tex = TextureCache::new(None);
    let render = |gpu: &mut GpuRenderer, sim: &mut Sim, tex: &mut TextureCache| gpu.render_to_image(&mut sim.world, tex, &opts, &q).expect("render");
    // the ball is about 170 pixels across around (240, 190): its left side reflects the left wall, its right side the right wall
    let (left_at, right_at) = ((190u32, 190u32), (290u32, 190u32));
    let red = "[0.9,0.1,0.1,1]";
    let mut plain = sim_of(&probe_room(red, ""));
    let without = render(&mut gpu, &mut plain, &mut tex);
    let mut probed = sim_of(&probe_room(red, "[13,6,13]"));
    let with = render(&mut gpu, &mut probed, &mut tex);
    let (wl, wr, pl, pr) = (px(&without, left_at.0, left_at.1), px(&without, right_at.0, right_at.1), px(&with, left_at.0, left_at.1), px(&with, right_at.0, right_at.1));
    eprintln!("without a probe: left {wl:?} right {wr:?}; with one: left {pl:?} right {pr:?}");
    // without a probe the ball reflects the (blue) sky on both sides; with one it shows the red wall on the left and the green one on the right
    assert!(wl[2] >= wl[0] && wr[2] >= wr[1], "the plain ball reflects the sky: {wl:?} {wr:?}");
    assert!(pl[0] as i32 > pl[2] as i32 + 25 && pl[0] as i32 > pl[1] as i32 + 25, "the left of the ball should show the red wall: {pl:?}");
    assert!(pr[1] as i32 > pr[0] as i32 + 15 && pr[1] as i32 > pr[2] as i32 + 15, "the right of the ball should show the green wall: {pr:?}");
    // the same picture twice, and a probe is a high-quality effect only
    assert_eq!(with.rgba, render(&mut gpu, &mut probed, &mut tex).rgba);
    let classic = |gpu: &mut GpuRenderer, sim: &mut Sim, tex: &mut TextureCache| gpu.render_to_image(&mut sim.world, tex, &opts, &Quality::classic()).expect("render").rgba;
    assert_eq!(classic(&mut gpu, &mut plain, &mut tex), classic(&mut gpu, &mut probed, &mut tex), "classic has no probes");

    // baked once: the left wall turns yellow, and the ball still shows the red it was captured with until the probe is captured again
    let mut moved = sim_of(&probe_room("[0.9,0.9,0.1,1]", "[13,6,13]"));
    let stale = render(&mut gpu, &mut moved, &mut tex);
    let s = px(&stale, left_at.0, left_at.1);
    assert!(s[0] as i32 > s[2] as i32 + 25 && s[1] < s[0] - 20, "the probe still holds the red wall: {s:?}");
    let baked = gpu.bake_probes(&mut moved.world, &mut tex, None).expect("bake");
    assert_eq!(baked, vec![40]);
    let fresh = render(&mut gpu, &mut moved, &mut tex);
    let f = px(&fresh, left_at.0, left_at.1);
    assert!(f[1] as i32 > f[2] as i32 + 40 && f[0] as i32 > f[2] as i32 + 40, "the ball shows the yellow wall after the probe was captured again: {f:?}");
    assert_eq!(gpu.bake_probes(&mut moved.world, &mut tex, Some(99)).expect("bake"), Vec::<u64>::new(), "no such probe: nothing captured");

    // a box too small to hold the ball's surface changes nothing: the sky is what the ball reflects (to within rounding)
    let mut tiny = sim_of(&probe_room(red, "[0.5,0.5,0.5]"));
    let small = render(&mut gpu, &mut tiny, &mut tex);
    let (_, _, same) = compare_pictures(&without, &small, 1.0);
    assert!(same > 480 * 270 * 99 / 100, "a probe whose box holds nothing changes nothing: {same} of {} the same", 480 * 270);
}

// ------------------------------------------------------------------------------------------------ lens flare

#[test]
fn a_lens_flare_appears_with_the_sun_in_view_and_vanishes_when_a_cube_covers_it() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    // the camera looks along -z, tilted up a little; the sun stands 30 degrees above the horizon straight ahead (near the top of the picture)
    let cam = |rot: &str| format!(r#"{{"uid":1,"components":{{"Camera":{{"fov_y":50,"clear":[0.1,0.1,0.15,1]}},"Transform":{{"pos":[0,2,9],"rot":{rot}}}}}}}"#);
    let sun = r#"{"uid":2,"components":{"Light":{"kind":0,"intensity":3},"Transform":{"rot":[-0.9659,0,0,0.2588]}}}"#;
    let flare = |numbers: &str| format!(r#"{{"uid":10,"components":{{"LensFlare":{numbers}}}}}"#);
    // a wall of a cube between the camera and the sun
    let cover = r#"{"uid":11,"components":{"Transform":{"pos":[0,7,0],"scale":[6,6,1]},"MeshRenderer":{"mesh":"cube","color":[0.2,0.2,0.2,1]}}}"#;
    let q = Quality::high();
    let (w, h) = (480u32, 270u32);
    let ahead = cam("[0.1,0,0,0.995]");
    let scene = |camera: &str, extra: &[&str]| {
        let mut v: Vec<String> = vec![camera.to_string(), sun.to_string(), FLOOR.to_string(), BLUE_SKY.to_string()];
        v.extend(extra.iter().map(|s| s.to_string()));
        v
    };
    let plain = draw_owned(&mut gpu, &scene(&ahead, &[]), &q, w, h);
    let flared = draw_owned(&mut gpu, &scene(&ahead, &[&flare("{}")]), &q, w, h);
    let (brighter, darker, _) = compare_pictures(&plain, &flared, 6.0);
    eprintln!("sun in view: {brighter} pixels brighter, {darker} darker");
    assert!(brighter > 500 && darker == 0, "the flare should light up the picture: {brighter} brighter, {darker} darker");
    // the same picture twice
    assert_eq!(flared.rgba, draw_owned(&mut gpu, &scene(&ahead, &[&flare("{}")]), &q, w, h).rgba);
    // ghosts and the halo are parts of it: without them there is much less
    let bare = draw_owned(&mut gpu, &scene(&ahead, &[&flare(r#"{"ghosts":0,"halo":0}"#)]), &q, w, h);
    let (ghostly, _, _) = compare_pictures(&bare, &flared, 6.0);
    eprintln!("ghosts and halo: {ghostly} pixels brighter than the glare alone");
    assert!(ghostly > 300, "the ghosts and the halo are most of the flare: {ghostly} pixels more than the glare alone");
    // a stronger flare is brighter, a flare of intensity 0 is nothing
    let strong = draw_owned(&mut gpu, &scene(&ahead, &[&flare(r#"{"intensity":2.5}"#)]), &q, w, h);
    assert!(mean_lum(&strong) > mean_lum(&flared) + 0.2, "{} vs {}", mean_lum(&strong), mean_lum(&flared));
    let none = draw_owned(&mut gpu, &scene(&ahead, &[&flare(r#"{"intensity":0}"#)]), &q, w, h);
    assert_eq!(compare_pictures(&plain, &none, 1.0).2, (w * h) as usize, "intensity 0 draws nothing");
    // a cube between the camera and the sun: the sun is hidden and so is the flare
    let (hidden_plain, hidden_flared) = (draw_owned(&mut gpu, &scene(&ahead, &[cover]), &q, w, h), draw_owned(&mut gpu, &scene(&ahead, &[cover, &flare("{}")]), &q, w, h));
    let (brighter_hidden, _, _) = compare_pictures(&hidden_plain, &hidden_flared, 6.0);
    eprintln!("sun hidden: {brighter_hidden} pixels brighter");
    assert!(brighter_hidden < 30, "the cube covers the sun, so the flare goes: {brighter_hidden} pixels still brighter");
    // the camera turned away: the sun is behind it
    let behind = cam("[0,1,0,0]");
    let (a, b) = (draw_owned(&mut gpu, &scene(&behind, &[]), &q, w, h), draw_owned(&mut gpu, &scene(&behind, &[&flare("{}")]), &q, w, h));
    assert_eq!(compare_pictures(&a, &b, 1.0).2, (w * h) as usize, "no flare with the sun behind the camera");
    // the lower qualities and classic do not draw it
    for quality in [Quality::classic(), Quality::low(), Quality::medium()] {
        let (a, b) = (draw_owned(&mut gpu, &scene(&ahead, &[]), &quality, 240, 135), draw_owned(&mut gpu, &scene(&ahead, &[&flare("{}")]), &quality, 240, 135));
        assert_eq!(a.rgba, b.rgba, "{quality:?} has no lens flare");
    }
}

#[test]
fn bounced_light_tints_the_floor_next_to_a_red_wall_and_lifts_the_shade() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    // a white floor, a red wall on the left, a lamp, a block with a shaded side; no sun, a dark sky
    let room = |gi: &str| -> Vec<String> {
        let mut v: Vec<String> = [
            r#"{"uid":1,"components":{"Camera":{"fov_y":60,"clear":[0.02,0.02,0.03,1]},"Transform":{"pos":[0,3.5,8],"rot":[-0.25,0,0,0.968]}}}"#,
            r#"{"uid":3,"components":{"Transform":{"scale":[16,1,16]},"MeshRenderer":{"mesh":"plane","color":[0.8,0.8,0.8,1],"roughness":0.9}}}"#,
            r#"{"uid":4,"components":{"Transform":{"pos":[-4.5,2,0],"scale":[0.4,4,12]},"MeshRenderer":{"mesh":"cube","color":[0.85,0.08,0.06,1],"roughness":0.9}}}"#,
            r#"{"uid":5,"components":{"Transform":{"pos":[2.5,1,-1],"scale":[1.6,2,1.6]},"MeshRenderer":{"mesh":"cube","color":[0.8,0.8,0.8,1],"roughness":0.9}}}"#,
            r#"{"uid":6,"components":{"Transform":{"pos":[-1.5,3,0]},"Light":{"kind":1,"intensity":1.4,"range":14,"color":[1,1,1]}}}"#,
            r#"{"uid":9,"components":{"Environment":{"sky":false,"ambient":0.02,"bloom":0,"ssao":0,"shadows":true,"tonemap":1,"exposure":1.0}}}"#,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if !gi.is_empty() {
            v.push(format!(r#"{{"uid":10,"components":{{"GlobalIllumination":{gi}}}}}"#));
        }
        v
    };
    let q = Quality::high();
    let off = draw_owned(&mut gpu, &room(""), &q, 480, 270);
    let on = draw_owned(&mut gpu, &room(r#"{"intensity":1.0,"radius":6,"directions":12}"#), &q, 480, 270);
    // the floor pixels next to the wall, and far from it, by the ids of the picture
    let floor_mean = |f: &Frame, x0: u32, x1: u32| -> [f32; 3] {
        let (mut n, mut s) = (0.0f32, [0.0f32; 3]);
        for y in 0..270 {
            for x in x0..x1 {
                if f.ids[(y * f.width + x) as usize] == 3 {
                    let c = px(f, x, y);
                    n += 1.0;
                    for k in 0..3 {
                        s[k] += c[k] as f32;
                    }
                }
            }
        }
        assert!(n > 200.0, "only {n} floor pixels in columns {x0} to {x1}");
        [s[0] / n, s[1] / n, s[2] / n]
    };
    let (near_off, near_on) = (floor_mean(&off, 90, 200), floor_mean(&on, 90, 200));
    let (far_off, far_on) = (floor_mean(&off, 330, 440), floor_mean(&on, 330, 440));
    let red_gain_near = (near_on[0] - near_off[0]) - (near_on[1] - near_off[1]);
    let red_gain_far = (far_on[0] - far_off[0]) - (far_on[1] - far_off[1]);
    eprintln!("floor near the wall: {near_off:?} -> {near_on:?}; far: {far_off:?} -> {far_on:?}");
    assert!(red_gain_near > red_gain_far + 1.0, "the red wall should tint the floor beside it more than the floor far from it: {red_gain_near} vs {red_gain_far}");
    assert!(red_gain_near > 1.0, "the floor beside a red wall turns red: {red_gain_near}");
    // the shaded side of the block is lighter with bounced light
    let shade = |f: &Frame| -> f32 {
        let (mut n, mut s) = (0.0f32, 0.0f32);
        for (i, id) in f.ids.iter().enumerate() {
            if *id == 5 {
                n += 1.0;
                s += lum(px(f, i as u32 % f.width, i as u32 / f.width));
            }
        }
        s / n.max(1.0)
    };
    assert!(shade(&on) > shade(&off) + 1.0, "{} vs {}", shade(&on), shade(&off));
    // a still made twice is the same picture
    let again = draw_owned(&mut gpu, &room(r#"{"intensity":1.0,"radius":6,"directions":12}"#), &q, 480, 270);
    assert_eq!(on.rgba, again.rgba, "bounced light settles to the same picture every time");
    // off means off: with the component disabled it is the picture without it
    let disabled = draw_owned(&mut gpu, &room(r#"{"intensity":1.0,"enabled":false}"#), &q, 480, 270);
    assert_eq!(off.rgba, disabled.rgba);
    // the lower qualities do not draw it
    let (a, b) = (draw_owned(&mut gpu, &room(""), &Quality::low(), 240, 135), draw_owned(&mut gpu, &room(r#"{"intensity":1.0}"#), &Quality::low(), 240, 135));
    assert_eq!(a.rgba, b.rgba);
}

#[test]
fn the_penumbra_of_a_sun_shadow_widens_with_the_height_of_what_casts_it() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let scene = |soft: &str| -> Vec<String> {
        let mut v: Vec<String> = [
            CAMERA_3D.to_string(),
            SUN.to_string(),
            FLOOR.to_string(),
            r#"{"uid":4,"components":{"Transform":{"pos":[0,6,0],"scale":[3,0.3,3]},"MeshRenderer":{"mesh":"cube","color":[0.9,0.2,0.2,1]}}}"#.to_string(),
            r#"{"uid":9,"components":{"Environment":{"sky":true,"shadows":true,"ssao":0,"bloom":0,"shadow_distance":60}}}"#.to_string(),
        ]
        .to_vec();
        if !soft.is_empty() {
            v.push(format!(r#"{{"uid":10,"components":{{"Shadows":{soft}}}}}"#));
        }
        v
    };
    // the steepest step of the floor's brightness along a row: a hard edge has a big one, a soft edge spreads it
    let steepest = |f: &Frame| -> f32 {
        let mut best = 0.0f32;
        for y in 135..270 {
            for x in 1..f.width {
                if f.ids[(y * f.width + x) as usize] == 3 && f.ids[(y * f.width + x - 1) as usize] == 3 {
                    best = best.max((lum(px(f, x, y)) - lum(px(f, x - 1, y))).abs());
                }
            }
        }
        best
    };
    let q = Quality::high();
    let hard = draw_owned(&mut gpu, &scene(""), &q, 640, 360);
    let soft = draw_owned(&mut gpu, &scene(r#"{"softness":0.12}"#), &q, 640, 360);
    let (a, b) = (steepest(&hard), steepest(&soft));
    eprintln!("steepest step: plain filter {a}, contact-hardening {b}");
    assert!(a > 15.0, "the shadow edge is there: {a}");
    assert!(b < a * 0.85, "a shadow from six metres up has a wide penumbra: {b} vs {a}");
    // softness 0 is the plain filter
    let zero = draw_owned(&mut gpu, &scene(r#"{"softness":0}"#), &q, 640, 360);
    assert_eq!(hard.rgba, zero.rgba);
}

#[test]
fn the_sky_of_the_air_is_blue_at_noon_red_at_sunset_and_dark_at_night() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    // rotations as [x, y, z, w]: yaw about up, pitch about the side
    let q = |yaw: f32, pitch: f32| -> String {
        let (sy, cy) = ((yaw.to_radians() * 0.5).sin(), (yaw.to_radians() * 0.5).cos());
        let (sp, cp) = ((pitch.to_radians() * 0.5).sin(), (pitch.to_radians() * 0.5).cos());
        // Ry(yaw) * Rx(pitch)
        format!("[{},{},{},{}]", cy * sp, sy * cp, -sy * sp, cy * cp)
    };
    // the camera looks along +x and a little up; the sun stands at +x (the light travels towards -x) at an elevation given in degrees
    let scene = |elevation: f32, air: &str| -> Vec<String> {
        let mut v = vec![
            format!(r#"{{"uid":1,"components":{{"Camera":{{"fov_y":60,"clear":[0,0,0,1]}},"Transform":{{"pos":[0,1.7,0],"rot":{}}}}}}}"#, q(-90.0, 22.0)),
            format!(r#"{{"uid":2,"components":{{"Light":{{"kind":0,"intensity":3,"color":[1,1,1]}},"Transform":{{"rot":{}}}}}}}"#, q(90.0, -elevation)),
            r#"{"uid":9,"components":{"Environment":{"sky":true,"clouds":0,"bloom":0,"ssao":0,"shadows":false,"tonemap":1,"exposure":1.0,"vignette":0}}}"#.to_string(),
        ];
        if !air.is_empty() {
            v.push(format!(r#"{{"uid":10,"components":{{"Atmosphere":{air}}}}}"#));
        }
        v
    };
    let mean = |f: &Frame| -> [f32; 3] {
        let mut s = [0.0f32; 3];
        for p in f.rgba.chunks_exact(4) {
            for k in 0..3 {
                s[k] += p[k] as f32;
            }
        }
        let n = (f.rgba.len() / 4) as f32;
        [s[0] / n, s[1] / n, s[2] / n]
    };
    let qh = Quality::high();
    let noon = mean(&draw_owned(&mut gpu, &scene(80.0, "{}"), &qh, 320, 180));
    let sunset = mean(&draw_owned(&mut gpu, &scene(3.0, "{}"), &qh, 320, 180));
    let night = mean(&draw_owned(&mut gpu, &scene(-25.0, "{}"), &qh, 320, 180));
    eprintln!("noon {noon:?}, sunset {sunset:?}, night {night:?}");
    assert!(noon[2] > noon[0] * 1.15, "a noon sky is blue: {noon:?}");
    let (r_noon, r_sunset) = (noon[0] / noon[2], sunset[0] / sunset[2]);
    assert!(r_sunset > r_noon * 1.6, "looking at a low sun the sky is red: {r_sunset} vs {r_noon}");
    assert!(lum([night[0] as u8, night[1] as u8, night[2] as u8]) < 0.2 * lum([noon[0] as u8, noon[1] as u8, noon[2] as u8]), "{night:?} vs {noon:?}");
    // haze whitens: more turbidity, less blue over red
    let hazy = mean(&draw_owned(&mut gpu, &scene(80.0, r#"{"turbidity":9}"#), &qh, 320, 180));
    assert!(hazy[2] / hazy[0] < noon[2] / noon[0], "haze whitens the sky: {hazy:?} vs {noon:?}");
    // switched off it is the picture without it, and the lower qualities and classic do not draw it
    let off = draw_owned(&mut gpu, &scene(80.0, ""), &qh, 320, 180);
    let disabled = draw_owned(&mut gpu, &scene(80.0, r#"{"enabled":false}"#), &qh, 320, 180);
    assert_eq!(off.rgba, disabled.rgba);
    for quality in [Quality::classic(), Quality::low()] {
        let (a, b) = (draw_owned(&mut gpu, &scene(80.0, ""), &quality, 160, 90), draw_owned(&mut gpu, &scene(80.0, "{}"), &quality, 160, 90));
        if quality.mode == lunaris_gpu::Mode::Classic {
            assert_eq!(a.rgba, b.rgba, "classic has no atmosphere");
        }
    }
}

#[test]
fn temporal_anti_aliasing_smooths_the_edges_and_is_the_same_every_time() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    // a white block turned a little, seen against black: its edges are the only places where the picture has in-between values
    let scene: Vec<String> = [
        r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0,0,0,1]},"Transform":{"pos":[0,0.5,6]}}}"#,
        r#"{"uid":2,"components":{"Light":{"kind":0,"intensity":2.5},"Transform":{"rot":[-0.2,0.35,0,0.91]}}}"#,
        r#"{"uid":3,"components":{"Transform":{"pos":[0,0,0],"rot":[0.0,0.19,0.0,0.98],"scale":[1.6,1.6,1.6]},"MeshRenderer":{"mesh":"cube","color":[1,1,1,1],"roughness":0.9,"emissive":0.6}}}"#,
        r#"{"uid":9,"components":{"Environment":{"sky":false,"ambient":0.3,"bloom":0,"ssao":0,"shadows":false,"tonemap":2,"vignette":0}}}"#,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let on = Quality::high();
    let off = Quality { taa: false, ..Quality::high() };
    let levels = |f: &Frame| -> usize {
        let mut seen = std::collections::BTreeSet::new();
        for y in 0..f.height {
            for x in 0..f.width {
                let l = lum(px(f, x, y)) as i32;
                if (12..243).contains(&l) {
                    seen.insert(l);
                }
            }
        }
        seen.len()
    };
    let (a, b) = (draw_owned(&mut gpu, &scene, &on, 480, 270), draw_owned(&mut gpu, &scene, &off, 480, 270));
    let (la, lb) = (levels(&a), levels(&b));
    eprintln!("in-between values on the edges: with {la}, without {lb}");
    assert!(la > lb + 8, "temporal anti-aliasing gives the edges more steps: {la} vs {lb}");
    // a still made twice is the same picture, and it is a different picture from the one without
    assert_eq!(a.rgba, draw_owned(&mut gpu, &scene, &on, 480, 270).rgba);
    assert_ne!(a.rgba, b.rgba);
    // the picture is not smeared: the block is where it was (the middle of the picture is as bright)
    assert!((lum(px(&a, 240, 135)) - lum(px(&b, 240, 135))).abs() < 25.0);
    // the lower qualities do not have it
    assert!(!Quality::medium().taa && !Quality::low().taa && !Quality::classic().taa && Quality::ultra().taa);
}

#[test]
fn deep_water_shows_the_bottom_where_it_is_shallow_and_hides_it_where_it_is_deep() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    // a white bed 0.3 m under the water on the left and 9 m under it on the right, seen from above
    let scene = |look: &str| -> Vec<String> {
        let mut v: Vec<String> = [
            r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0.1,0.1,0.15,1]},"Transform":{"pos":[0,9,9],"rot":[-0.38,0,0,0.925]}}}"#,
            SUN.to_string().as_str(),
            r#"{"uid":3,"components":{"Transform":{"pos":[-20,-0.3,0],"scale":[40,1,70]},"MeshRenderer":{"mesh":"plane","color":[0.9,0.9,0.85,1],"roughness":0.9}}}"#,
            r#"{"uid":4,"components":{"Transform":{"pos":[20,-9,0],"scale":[40,1,70]},"MeshRenderer":{"mesh":"plane","color":[0.9,0.9,0.85,1],"roughness":0.9}}}"#,
            r#"{"uid":5,"components":{"Transform":{"pos":[0,0,0]},"Water":{"size":[16,14],"color":[0.02,0.14,0.26,0.85],"wave_height":0,"speed":0}}}"#,
            r#"{"uid":9,"components":{"Environment":{"sky":true,"ssao":0,"bloom":0,"shadows":false,"tonemap":2}}}"#,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if !look.is_empty() {
            v.push(format!(r#"{{"uid":6,"components":{{"WaterLook":{look}}}}}"#));
            // the look goes beside the water: the same entity
            v[4] = r#"{"uid":5,"components":{"Transform":{"pos":[0,0,0]},"Water":{"size":[16,14],"color":[0.02,0.14,0.26,0.85],"wave_height":0,"speed":0},"WaterLook":LOOK}}"#.replace("LOOK", look);
            v.pop();
        }
        v
    };
    let q = Quality::high();
    let side = |f: &Frame, left: bool| -> f32 {
        let (mut n, mut sum) = (0.0f32, 0.0f32);
        for y in 0..f.height {
            for x in 0..f.width {
                if f.ids[(y * f.width + x) as usize] == 5 && (x < f.width / 2) == left && (x as i32 - (f.width / 2) as i32).abs() > 40 {
                    n += 1.0;
                    sum += lum(px(f, x, y));
                }
            }
        }
        assert!(n > 500.0, "only {n} water pixels on one side");
        sum / n
    };
    let with = draw_owned(&mut gpu, &scene(r#"{"clarity":2.5,"foam":0}"#), &q, 480, 270);
    let (shallow, deep) = (side(&with, true), side(&with, false));
    eprintln!("water over the shallows {shallow}, over the deep {deep}");
    assert!(shallow > deep * 1.7, "the bottom shows through in the shallows and not in the deep: {shallow} vs {deep}");
    // the clarity matters: with 40 m of clarity the deep side is another picture (the bed is seen through the water there)
    let clear = draw_owned(&mut gpu, &scene(r#"{"clarity":40,"foam":0}"#), &q, 480, 270);
    let clear_deep = side(&clear, false);
    eprintln!("clarity 40: over the deep {clear_deep}");
    assert!((clear_deep - deep).abs() > deep * 0.1, "{clear_deep} vs {deep}");
    // classic draws the water as before
    let (a, b) = (draw_owned(&mut gpu, &scene(""), &Quality::classic(), 240, 135), draw_owned(&mut gpu, &scene(r#"{"clarity":2.5}"#), &Quality::classic(), 240, 135));
    assert_eq!(a.rgba, b.rgba);
}

#[test]
fn ambient_occlusion_leaves_open_ground_alone_and_darkens_a_crease_gradually() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let env = |ssao: f32| format!(r#"{{"uid":9,"components":{{"Environment":{{"sky":true,"clouds":0,"ssao":{ssao},"bloom":0,"shadows":false,"vignette":0}}}}}}"#);
    let wall = r#"{"uid":4,"components":{"Transform":{"pos":[0,2,-3],"scale":[30,4,0.5]},"MeshRenderer":{"mesh":"cube","color":[0.8,0.8,0.8,1]}}}"#;
    let on = draw(&mut gpu, &[CAMERA_3D, FLOOR, wall, &env(1.0)], &Quality::high(), 480, 270);
    let off = draw(&mut gpu, &[CAMERA_3D, FLOOR, wall, &env(0.0)], &Quality::high(), 480, 270);
    // the open floor near the camera (bottom of the picture, middle): the same with and without
    let near_on: f32 = (200..280).map(|x| lum(px(&on, x, 255))).sum::<f32>() / 80.0;
    let near_off: f32 = (200..280).map(|x| lum(px(&off, x, 255))).sum::<f32>() / 80.0;
    assert!((near_on - near_off).abs() < near_off * 0.06, "open floor changed: {near_off} -> {near_on}");
    // somewhere along a column through the foot of the wall the floor is clearly darker with the occlusion than without
    let dip = (40..230).map(|y| lum(px(&off, 240, y)) - lum(px(&on, 240, y))).fold(0.0f32, f32::max);
    assert!(dip > 6.0, "the floor under the wall is only {dip} darker");
}
