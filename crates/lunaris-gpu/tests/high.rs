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
