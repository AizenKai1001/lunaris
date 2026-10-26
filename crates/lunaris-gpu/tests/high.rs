//! What the high-quality mode must always do, whatever the graphics card: keep the colours of a flat 2D scene exactly as authored, be
//! repeatable, and make the effects it promises visible (sun shadows, a sky, reflections on metal, bloom, fog, ambient occlusion).
use lunaris_gpu::{GpuRenderer, Quality};
use lunaris_render::{Frame, RenderOptions, TextureCache};
use lunaris_sim::Sim;

/// One renderer for every test of this file (they take turns): building one compiles every pipeline it uses, which takes minutes on a machine that draws with the CPU, and a test needs only to
/// find the renderer as a fresh picture would, which a still always does (its effects start from nothing).
fn gpu_or_skip() -> Option<std::sync::MutexGuard<'static, GpuRenderer>> {
    static GPU: std::sync::OnceLock<Option<std::sync::Mutex<GpuRenderer>>> = std::sync::OnceLock::new();
    let cell = GPU.get_or_init(|| match GpuRenderer::headless() {
        Ok(g) => {
            println!("{}", g.adapter_name());
            Some(std::sync::Mutex::new(g))
        }
        Err(e) => {
            eprintln!("no GPU here ({e}): skipping");
            None
        }
    });
    cell.as_ref().map(|m| m.lock().unwrap_or_else(|e| e.into_inner()))
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

#[test]
fn clouds_have_a_bright_side_and_a_shaded_underside() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let sky = |clouds: f32| format!(r#"{{"uid":9,"components":{{"Environment":{{"sky":true,"clouds":{clouds},"shadows":false,"ssao":0,"bloom":0,"vignette":0,"tonemap":1,"ambient":1.0}}}}}}"#);
    // a camera looking up into the sky from the ground, the sun a little to one side
    let cam = |x: f32| format!(r#"{{"uid":1,"components":{{"Camera":{{"fov_y":70}},"Transform":{{"pos":[{x},2,0],"rot":[0.42,0,0,0.907]}}}}}}"#);
    let sun = r#"{"uid":2,"components":{"Light":{"kind":0,"intensity":3.0},"Transform":{"rot":[-0.3,0.3,0,0.9]}}}"#;
    let clear = draw(&mut gpu, &[&cam(0.0), sun, &sky(0.0)], &Quality::high(), 320, 180);
    let cloudy = draw(&mut gpu, &[&cam(0.0), sun, &sky(0.8)], &Quality::high(), 320, 180);
    let spread = |f: &Frame| {
        let l: Vec<f32> = f.rgba.chunks(4).map(|p| lum([p[0], p[1], p[2]])).collect();
        let mean = l.iter().sum::<f32>() / l.len() as f32;
        (l.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / l.len() as f32).sqrt()
    };
    // clouds make the sky vary clearly more than the smooth clear one (which has its own sun glow), and have both light and shaded parts
    assert!(spread(&cloudy) > spread(&clear) * 1.4, "{} vs {}", spread(&cloudy), spread(&clear));
    let lums: Vec<f32> = cloudy.rgba.chunks(4).map(|p| lum([p[0], p[1], p[2]])).collect();
    let (lo, hi) = (lums.iter().cloned().fold(f32::MAX, f32::min), lums.iter().cloned().fold(0.0f32, f32::max));
    assert!(hi > lo + 50.0, "{lo} .. {hi}");
}

/// A ball of `material` seen from the front, nothing else in view: a flat background, no sky, a sun (`sun_rot` turns it) and the ambient light as given.
fn ball(gpu: &mut GpuRenderer, material: &str, ext: &str, sun_rot: &str, ambient: f32) -> Frame {
    let cam = r#"{"uid":1,"components":{"Camera":{"fov_y":30,"clear":[0.0,0.0,0.0,1]},"Transform":{"pos":[0,0,7]}}}"#.to_string();
    let sun = format!(r#"{{"uid":2,"components":{{"Light":{{"kind":0,"intensity":3.0}},"Transform":{{"rot":{sun_rot}}}}}}}"#);
    let sphere = format!(r#"{{"uid":3,"components":{{"Transform":{{"scale":[3,3,3]}},"MeshRenderer":{{"mesh":"sphere",{material}}}{ext}}}}}"#);
    let env = format!(r#"{{"uid":9,"components":{{"Environment":{{"sky":false,"ambient":{ambient},"ssao":0,"bloom":0,"vignette":0,"shadows":false,"tonemap":1}}}}}}"#);
    draw(gpu, &[&cam, &sun, &sphere, &env], &Quality::high(), 200, 200)
}

fn brightest(f: &Frame) -> f32 {
    f.rgba.chunks(4).map(|p| lum([p[0], p[1], p[2]])).fold(0.0, f32::max)
}

#[test]
fn a_clear_coat_adds_a_sharp_highlight_over_a_rough_base() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let base = r#""color":[0.6,0.05,0.05,1],"metallic":0.0,"roughness":0.85"#;
    // the sun shines from up and a little to the right, in front: the highlight is on the upper right of the ball
    let sun = "[-0.35,0.25,0.0,0.9]";
    let plain = ball(&mut gpu, base, "", sun, 0.3);
    let coated = ball(&mut gpu, base, r#","MaterialExt":{"clearcoat":1.0,"clearcoat_roughness":0.05}"#, sun, 0.3);
    let (a, b) = (brightest(&plain), brightest(&coated));
    assert!(b > a + 40.0, "the coat should add a bright highlight: {a} -> {b}");
    // away from the highlight the coat only dims the base a little
    let (p, c) = (lum(px(&plain, 60, 150)), lum(px(&coated, 60, 150)));
    assert!(c < p * 1.15 + 6.0, "{p} vs {c}");
}

#[test]
fn sheen_brightens_the_rim_of_cloth_and_leaves_the_middle_alone() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let base = r#""color":[0.05,0.1,0.35,1],"metallic":0.0,"roughness":0.9"#;
    let sun = "[-0.2,0.2,0.0,0.96]";
    let plain = ball(&mut gpu, base, "", sun, 1.0);
    let velvet = ball(&mut gpu, base, r#","MaterialExt":{"sheen":1.0}"#, sun, 1.0);
    // a ring of pixels near the edge of the ball, and its middle
    let rim = |f: &Frame| (0..360).step_by(15).map(|d| { let a = (d as f32).to_radians(); lum(px(f, (100.0 + 70.0 * a.cos()) as u32, (100.0 + 70.0 * a.sin()) as u32)) }).sum::<f32>() / 24.0;
    let (r0, r1) = (rim(&plain), rim(&velvet));
    eprintln!("rim {r0} -> {r1}");
    assert!(r1 > r0 * 1.15 + 3.0, "the rim should glow: {r0} -> {r1}");
    let (m0, m1) = (lum(px(&plain, 100, 100)), lum(px(&velvet, 100, 100)));
    assert!((m1 - m0).abs() < m0 * 0.25 + 4.0, "the middle should stay: {m0} -> {m1}");
}

#[test]
fn subsurface_lets_light_wrap_round_the_terminator_in_its_own_colour() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let base = r#""color":[0.8,0.6,0.5,1],"metallic":0.0,"roughness":0.6"#;
    // the sun shines across the picture: half the ball is lit, half in the dark (no ambient)
    let sun = "[0,0.7071,0,0.7071]";
    let plain = ball(&mut gpu, base, "", sun, 0.0);
    let skin = ball(&mut gpu, base, r#","MaterialExt":{"subsurface":1.0,"subsurface_color":[1.0,0.2,0.1]}"#, sun, 0.0);
    // the dark half is the one whose middle column is darker in the plain picture
    let half = |f: &Frame, left: bool| -> [f32; 3] {
        let (x0, x1) = if left { (80u32, 99u32) } else { (101u32, 120u32) };
        let mut sum = [0.0f32; 3];
        let mut n = 0.0;
        for y in 85..115u32 {
            for x in x0..x1 {
                let p = px(f, x, y);
                for k in 0..3 {
                    sum[k] += p[k] as f32;
                }
                n += 1.0;
            }
        }
        [sum[0] / n, sum[1] / n, sum[2] / n]
    };
    let left_dark = lum(half(&plain, true).map(|v| v as u8)) < lum(half(&plain, false).map(|v| v as u8));
    let (dark_plain, dark_skin) = (half(&plain, left_dark), half(&skin, left_dark));
    assert!(dark_skin[0] > dark_plain[0] * 1.3 + 4.0, "light should wrap round into the dark side: {dark_plain:?} -> {dark_skin:?}");
    assert!(dark_skin[0] - dark_skin[1] > dark_plain[0] - dark_plain[1] + 3.0, "and be reddish: {dark_plain:?} -> {dark_skin:?}");
}

/// A grass field of single blades on a small flat terrain (a 16-bit picture the test writes), the camera low and looking out over it, the sun low behind.
fn meadow(gpu: &mut GpuRenderer, field: &str, ground: &str, camera_y: f32) -> Frame {
    let dir = std::env::temp_dir().join(format!("lunaris-grass-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut png = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png, 17, 17);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Sixteen);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&vec![0x80u8; 17 * 17 * 2]).unwrap();
    }
    std::fs::write(dir.join("flat.png"), &png).unwrap();
    let cam = format!(r#"{{"uid":1,"components":{{"Camera":{{"fov_y":55,"clear":[0,0,0,1]}},"Transform":{{"pos":[0,{camera_y},0],"rot":[-0.12,0,0,0.9928]}}}}}}"#);
    let sun = r#"{"uid":2,"components":{"Light":{"kind":0,"intensity":3.0},"Transform":{"rot":[-0.35,0.9,0.0,0.3]}}}"#;
    let terrain = format!(r#"{{"uid":3,"components":{{"Transform":{{"pos":[0,0,0]}},"Terrain":{{"heightmap":"flat.png","size":[400,400],"height":1,"color":{ground}}}}}}}"#);
    let grass = format!(r#"{{"uid":4,"components":{{"GrassField":{field}}}}}"#);
    let env = r#"{"uid":9,"components":{"Environment":{"sky":true,"clouds":0,"ambient":1.0,"ssao":0,"bloom":0,"vignette":0,"shadows":true,"tonemap":1}}}"#;
    let text = format!(r#"{{"lunaris":1,"entities":[{cam},{sun},{terrain},{grass},{env}]}}"#);
    let mut sim = Sim::new(1);
    sim.asset_root = Some(dir.clone());
    sim.load_scene(&lunaris_json::parse(&text).expect("scene JSON")).expect("scene loads");
    sim.step_n(2);
    let mut tex = TextureCache::new(Some(dir));
    let opts = RenderOptions { width: 320, height: 180, ..RenderOptions::default() };
    gpu.render_to_image(&mut sim.world, &mut tex, &opts, &Quality::high()).expect("render")
}

#[test]
fn a_field_of_single_blades_is_full_of_detail_and_a_bare_one_is_not() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let ground = "[0.2,0.15,0.1,1]";
    let field = r#"{"terrain":3,"radius":120,"density":20,"height":[0.3,0.6]}"#;
    let bare = meadow(&mut gpu, r#"{"terrain":3,"enabled":false}"#, ground, 1.4);
    let grass = meadow(&mut gpu, field, ground, 1.4);
    // the foreground (bottom third of the picture): bare ground is smooth, the blades make a lot of edges and a green that the brown ground does not have
    let (g_lo, g_hi) = (120u32, 179u32);
    let busy = |f: &Frame| -> f32 {
        let mut sum = 0.0;
        let mut n = 0.0;
        for y in g_lo..g_hi {
            for x in 1..319u32 {
                sum += (lum(px(f, x, y)) - lum(px(f, x - 1, y))).abs();
                n += 1.0;
            }
        }
        sum / n
    };
    let green = |f: &Frame| -> f32 { (g_lo..g_hi).map(|y| (0..320u32).map(|x| { let p = px(f, x, y); p[1] as f32 - 0.5 * (p[0] as f32 + p[2] as f32) }).sum::<f32>()).sum::<f32>() / (59.0 * 320.0) };
    assert!(busy(&grass) > busy(&bare) * 3.0 + 1.0, "{} vs {}", busy(&grass), busy(&bare));
    assert!(green(&grass) > green(&bare) + 4.0, "{} vs {}", green(&grass), green(&bare));
    // the same field twice is the same picture
    let again = meadow(&mut gpu, field, ground, 1.4);
    assert_eq!(grass.rgba, again.rgba);
}

#[test]
fn macro_vary_breaks_up_a_flat_colour_in_big_patches_and_leaves_a_surface_without_it_alone() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    // a flat grey ground 40 m across seen from straight above, lit evenly: with nothing on it every block of the picture is as bright as every other
    let scene = |gpu: &mut GpuRenderer, vary: &str| {
        let cam = r#"{"uid":1,"components":{"Camera":{"fov_y":40},"Transform":{"pos":[0,25,0],"rot":[-0.7071,0,0,0.7071]}}}"#;
        let plane = format!(r#"{{"uid":3,"components":{{"Transform":{{"scale":[40,1,40]}},"MeshRenderer":{{"mesh":"plane","color":[0.5,0.5,0.5,1],"roughness":1.0}}{vary}}}}}"#);
        let env = r#"{"uid":9,"components":{"Environment":{"sky":false,"ambient":1.0,"ssao":0,"bloom":0,"vignette":0,"shadows":false,"tonemap":1}}}"#;
        draw(gpu, &[cam, &plane, env], &Quality::high(), 320, 180)
    };
    // how much the brightness differs from one 20 x 20 pixel block to the next, as a share of the mean
    let spread = |f: &Frame| -> f32 {
        let mut means = Vec::new();
        for by in 1..8u32 {
            for bx in 1..15u32 {
                let mut s = 0.0;
                for y in 0..20 {
                    for x in 0..20 {
                        s += lum(px(f, bx * 20 + x, by * 20 + y));
                    }
                }
                means.push(s / 400.0);
            }
        }
        let mean = means.iter().sum::<f32>() / means.len() as f32;
        (means.iter().map(|m| (m - mean) * (m - mean)).sum::<f32>() / means.len() as f32).sqrt() / mean
    };
    let plain = scene(&mut gpu, "");
    let varied = scene(&mut gpu, r#","MacroVary":{"strength":0.5,"scale":8,"tint":0.0,"rough":0.0}"#);
    let (a, b) = (spread(&plain), spread(&varied));
    assert!(a < 0.02, "a flat ground is flat: {a}");
    assert!(b > 0.08 && b > a * 4.0, "with macro variation the ground has patches: {a} -> {b}");
    // all three numbers at 0 draw the surface exactly as it was, and the noise is still: the same picture twice
    let zero = scene(&mut gpu, r#","MacroVary":{"strength":0,"tint":0,"rough":0}"#);
    assert_eq!(zero.rgba, plain.rgba, "a MacroVary with nothing in it changes nothing");
    assert_eq!(scene(&mut gpu, r#","MacroVary":{"strength":0.5,"scale":8,"tint":0.0,"rough":0.0}"#).rgba, varied.rgba, "the patches do not move");
    // the colour lean makes some places warmer and some cooler (red against blue differs from block to block)
    let leaned = scene(&mut gpu, r#","MacroVary":{"strength":0,"scale":8,"tint":1.0,"rough":0.0}"#);
    let ratio = |f: &Frame, bx: u32, by: u32| {
        let (mut r, mut bl) = (0.0f32, 0.0f32);
        for y in 0..20 {
            for x in 0..20 {
                let c = px(f, bx * 20 + x, by * 20 + y);
                r += c[0] as f32;
                bl += c[2] as f32;
            }
        }
        r / bl
    };
    let ratios: Vec<f32> = (1..8).flat_map(|by| (1..15).map(move |bx| (bx, by))).map(|(bx, by)| ratio(&leaned, bx, by)).collect();
    let (lo, hi) = (ratios.iter().cloned().fold(f32::MAX, f32::min), ratios.iter().cloned().fold(f32::MIN, f32::max));
    assert!(hi - lo > 0.1, "some blocks lean warm and some cool: {lo} .. {hi}");
    // (LUNARIS_TEST_DUMP=dir: a street of repeated textures with and without it, for looking at)
    if let Ok(dir) = std::env::var("LUNARIS_TEST_DUMP") {
        for (name, vary) in [("macro_plain", ""), ("macro_varied", r#","MacroVary":{"strength":0.35,"scale":18,"tint":0.35,"rough":0.3}"#)] {
            let mut ents: Vec<String> = vec![
                r#"{"uid":1,"components":{"Camera":{"fov_y":55},"Transform":{"pos":[0,3,28],"rot":[-0.05,0,0,0.9987]}}}"#.to_string(),
                r#"{"uid":2,"components":{"Light":{"kind":0,"intensity":2.2},"Transform":{"rot":[-0.45,0.35,0,0.82]}}}"#.to_string(),
                format!(r#"{{"uid":3,"components":{{"Transform":{{"scale":[200,1,200]}},"MeshRenderer":{{"mesh":"plane","color":[1,1,1,1],"texture":"concrete","uv_scale":[160,160],"roughness":0.9}}{vary}}}}}"#),
                r#"{"uid":9,"components":{"Environment":{"sky":true,"ambient":0.5,"ssao":1,"bloom":0,"vignette":0,"shadows":true,"tonemap":1}}}"#.to_string(),
            ];
            for i in 0..8u32 {
                for (k, x) in [-13.0f32, 13.0].iter().enumerate() {
                    let z = -4.0 - 16.0 * i as f32;
                    ents.push(format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{x},12,{z}],"scale":[10,24,12]}},"MeshRenderer":{{"mesh":"cube","color":[1,1,1,1],"texture":"concrete","uv_scale":[3,6],"roughness":0.85}}{vary}}}}}"#, 20 + i * 2 + k as u32));
                }
            }
            let refs: Vec<&str> = ents.iter().map(|s| s.as_str()).collect();
            let f = draw(&mut gpu, &refs, &Quality::high(), 1280, 720);
            let file = std::fs::File::create(std::path::Path::new(&dir).join(format!("{name}.png"))).expect("dump file");
            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), f.width, f.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().expect("png header").write_image_data(&f.rgba).expect("png data");
        }
    }
}

#[test]
fn untile_breaks_up_the_repetition_of_a_texture_and_leaves_it_alone_otherwise() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    // a 64 x 64 picture of noise (deterministic), repeated three times over a plane seen from above
    let dir = std::env::temp_dir().join(format!("lunaris-untile-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut state = 12345u32;
    let mut px_rgba = Vec::new();
    for _ in 0..64 * 64 {
        for _ in 0..3 {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            px_rgba.push((state >> 24) as u8);
        }
        px_rgba.push(255);
    }
    let mut data = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut data, 64, 64);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&px_rgba).unwrap();
    }
    std::fs::write(dir.join("noise.png"), &data).unwrap();
    let mut scene = |maps: &str| {
        let cam = r#"{"uid":1,"components":{"Camera":{"fov_y":40},"Transform":{"pos":[0,10,0],"rot":[-0.7071,0,0,0.7071]}}}"#;
        let plane = format!(r#"{{"uid":3,"components":{{"Transform":{{"scale":[10,1,10]}},"MeshRenderer":{{"mesh":"plane","color":[1,1,1,1],"texture":"noise.png","uv_scale":[3,3],"roughness":1.0}}{maps}}}}}"#);
        let env = r#"{"uid":9,"components":{"Environment":{"sky":false,"ambient":1.0,"ssao":0,"bloom":0,"vignette":0,"shadows":false,"tonemap":1}}}"#;
        let text = format!(r#"{{"lunaris":1,"entities":[{cam},{plane},{env}]}}"#);
        let mut sim = Sim::new(1);
        sim.asset_root = Some(dir.clone());
        sim.load_scene(&lunaris_json::parse(&text).expect("scene JSON")).expect("scene loads");
        let mut tex = TextureCache::new(Some(dir.clone()));
        let opts = RenderOptions { width: 320, height: 180, ..RenderOptions::default() };
        gpu.render_to_image(&mut sim.world, &mut tex, &opts, &Quality::high()).expect("render")
    };
    // how much the picture repeats itself: the correlation of a stretch of a row with the stretch one period further on
    let repeat = |f: &Frame| -> f32 {
        // the plane is 10 m wide, seen over 2 x 10 m x tan(20 deg) x (320 / 180) = 12.9 m of the picture's width: a period of 3.33 m is 82 pixels
        let p = 82u32;
        let (mut sab, mut saa, mut sbb) = (0.0f64, 0.0f64, 0.0f64);
        for y in 70..110u32 {
            for x in 42..(42 + 80) {
                let a = lum(px(f, x, y)) as f64 - 128.0;
                let b = lum(px(f, x + p, y)) as f64 - 128.0;
                sab += a * b;
                saa += a * a;
                sbb += b * b;
            }
        }
        (sab / (saa * sbb).sqrt()) as f32
    };
    let plain = scene("");
    let untiled = scene(r#","PbrMaps":{"untile":true}"#);
    let (a, b) = (repeat(&plain), repeat(&untiled));
    assert!(a > 0.85, "a repeated texture repeats: {a}");
    assert!(b < a - 0.3, "untiled it does not come round again: {a} -> {b}");
    // and the picture is not flat or black
    let distinct: std::collections::BTreeSet<[u8; 3]> = untiled.rgba.chunks(4).map(|p| [p[0], p[1], p[2]]).collect();
    assert!(distinct.len() > 50);
}

#[test]
fn grass_grows_only_on_the_layer_it_is_masked_to() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let dir = std::env::temp_dir().join(format!("lunaris-grassmask-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let write_png = |name: &str, w: u32, h: u32, color: png::ColorType, depth: png::BitDepth, data: &[u8]| {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(color);
            enc.set_depth(depth);
            enc.write_header().unwrap().write_image_data(data).unwrap();
        }
        std::fs::write(dir.join(name), out).unwrap();
    };
    write_png("flat.png", 17, 17, png::ColorType::Grayscale, png::BitDepth::Sixteen, &vec![0x80u8; 17 * 17 * 2]);
    // the west half of the ground is painted with layer 1, the east half with layer 2
    let mut splat = Vec::new();
    for _ in 0..17 {
        for x in 0..17 {
            splat.extend_from_slice(if x < 8 { &[255u8, 0, 0, 0] } else if x > 8 { &[0u8, 255, 0, 0] } else { &[128u8, 128, 0, 0] });
        }
    }
    write_png("splat.png", 17, 17, png::ColorType::Rgba, png::BitDepth::Eight, &splat);
    let cam = r#"{"uid":1,"components":{"Camera":{"fov_y":60,"clear":[0,0,0,1]},"Transform":{"pos":[0,1.4,6],"rot":[-0.1,0,0,0.995]}}}"#;
    let sun = r#"{"uid":2,"components":{"Light":{"kind":0,"intensity":3.0},"Transform":{"rot":[-0.35,0.9,0.0,0.3]}}}"#;
    let terrain = r#"{"uid":3,"components":{"Transform":{"pos":[0,0,0]},"Terrain":{"heightmap":"flat.png","size":[40,40],"height":1},"TerrainLayers":{"splat":"splat.png","layers":"a dirt\nb dirt"}}}"#;
    let env = r#"{"uid":9,"components":{"Environment":{"sky":true,"clouds":0,"ambient":1.0,"ssao":0,"bloom":0,"vignette":0,"shadows":true,"tonemap":1}}}"#;
    let mut photo = |grass: &str| -> Frame {
        let text = format!(r#"{{"lunaris":1,"entities":[{cam},{sun},{terrain},{{"uid":4,"components":{{"GrassField":{grass}}}}},{env}]}}"#);
        let mut sim = Sim::new(1);
        sim.asset_root = Some(dir.clone());
        sim.load_scene(&lunaris_json::parse(&text).expect("scene JSON")).expect("scene loads");
        let mut tex = TextureCache::new(Some(dir.clone()));
        let opts = RenderOptions { width: 320, height: 180, ..RenderOptions::default() };
        gpu.render_to_image(&mut sim.world, &mut tex, &opts, &Quality::high()).expect("render")
    };
    let masked = photo(r#"{"terrain":3,"radius":30,"density":25,"height":[0.4,0.7],"mask_layer":1}"#);
    // the foreground band of the picture, the west half and the east half: how many pixels are the saturated green of a blade (the dirt under them is brown)
    let blades = |f: &Frame, x0: u32, x1: u32| -> usize {
        (100..175u32).map(|y| (x0..x1).filter(|&x| { let p = px(f, x, y); p[1] as i32 > p[0] as i32 + 12 && p[1] as i32 > p[2] as i32 + 12 }).count()).sum()
    };
    let (west, east) = (blades(&masked, 0, 150), blades(&masked, 170, 320));
    // (the splat map is bilinear: the grass thins out over a cell or two into the other layer, so some of it stands on the east side of the border)
    assert!(west > east * 2 + 100, "the grass is on the painted layer, not on the other: west {west}, east {east}");
}

#[test]
fn grass_stays_on_its_own_terrain_and_does_not_hang_in_the_air_over_the_next_one() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let dir = std::env::temp_dir().join(format!("lunaris-grassedge-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let write_png = |name: &str, data: &[u8]| {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 17, 17);
            enc.set_color(png::ColorType::Grayscale);
            enc.set_depth(png::BitDepth::Sixteen);
            enc.write_header().unwrap().write_image_data(data).unwrap();
        }
        std::fs::write(dir.join(name), out).unwrap();
    };
    write_png("low.png", &vec![0u8; 17 * 17 * 2]);
    write_png("high.png", &vec![0xffu8; 17 * 17 * 2]);
    std::fs::copy(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hollowlight/hills/grass.glb"), dir.join("grass.glb")).unwrap();
    // the camera stands on the low west terrain; the grass grows on the high east one next to it (its heights, read beyond its edge, would put grass 6 m up over the camera)
    let cam = r#"{"uid":1,"components":{"Camera":{"fov_y":60,"clear":[0,0,0,1]},"Transform":{"pos":[-10,1.4,6],"rot":[-0.1,0,0,0.995]}}}"#;
    let sun = r#"{"uid":2,"components":{"Light":{"kind":0,"intensity":3.0},"Transform":{"rot":[-0.35,0.9,0.0,0.3]}}}"#;
    let west = r#"{"uid":3,"components":{"Transform":{"pos":[-20,0,0]},"Terrain":{"heightmap":"low.png","size":[40,40],"height":6}}}"#;
    let east = r#"{"uid":5,"components":{"Transform":{"pos":[20,0,0]},"Terrain":{"heightmap":"high.png","size":[40,40],"height":6}}}"#;
    let env = r#"{"uid":9,"components":{"Environment":{"sky":true,"clouds":0,"ambient":1.0,"ssao":0,"bloom":0,"vignette":0,"shadows":false,"tonemap":1}}}"#;
    for grass in [r#"{"terrain":5,"mesh":"grass.glb","radius":30,"density":25}"#, r#"{"terrain":5,"radius":30,"density":25}"#] {
        let text = format!(r#"{{"lunaris":1,"entities":[{cam},{sun},{west},{east},{{"uid":4,"components":{{"GrassField":{grass}}}}},{env}]}}"#);
        let mut sim = Sim::new(1);
        sim.asset_root = Some(dir.clone());
        sim.load_scene(&lunaris_json::parse(&text).expect("scene JSON")).expect("scene loads");
        let mut tex = TextureCache::new(Some(dir.clone()));
        let opts = RenderOptions { width: 320, height: 180, ..RenderOptions::default() };
        let f = gpu.render_to_image(&mut sim.world, &mut tex, &opts, &Quality::high()).expect("render");
        // the top of the picture is sky (the horizon is below row 70): any green there is grass hanging in the air
        let green = (0..60u32).map(|y| (0..320u32).filter(|&x| { let p = px(&f, x, y); p[1] as i32 > p[0] as i32 + 12 && p[1] as i32 > p[2] as i32 + 12 }).count()).sum::<usize>();
        assert!(green < 20, "grass hangs in the air over the other terrain: {green} green pixels in the sky ({grass})");
    }
}

#[test]
fn a_cliff_layer_replaces_the_stretched_ground_texture_on_a_steep_wall() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let dir = std::env::temp_dir().join(format!("lunaris-cliff-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let write_png = |name: &str, color: png::ColorType, depth: png::BitDepth, data: &[u8]| {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 17, 17);
            enc.set_color(color);
            enc.set_depth(depth);
            enc.write_header().unwrap().write_image_data(data).unwrap();
        }
        std::fs::write(dir.join(name), out).unwrap();
    };
    // a plain at 0 and a plateau at 20 m with a wall between them in one cell (a cliff facing -x)
    let mut h = Vec::new();
    for _ in 0..17 {
        for x in 0..17 {
            h.extend_from_slice(&(if x < 8 { 0u16 } else { 65535u16 }).to_be_bytes());
        }
    }
    write_png("wall.png", png::ColorType::Grayscale, png::BitDepth::Sixteen, &h);
    write_png("splat.png", png::ColorType::Rgba, png::BitDepth::Eight, &vec![255u8, 0, 0, 0].repeat(17 * 17));
    let cam = r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0,0,0,1]},"Transform":{"pos":[-14,9,0],"rot":[0,-0.7071,0,0.7071]}}}"#;
    let sun = r#"{"uid":2,"components":{"Light":{"kind":0,"intensity":3.0},"Transform":{"rot":[-0.3,0.4,0.0,0.85]}}}"#;
    let env = r#"{"uid":9,"components":{"Environment":{"sky":true,"clouds":0,"ambient":1.0,"ssao":0,"bloom":0,"vignette":0,"shadows":false,"tonemap":1}}}"#;
    let mut photo = |layers: &str| -> Frame {
        let terrain = format!(r#"{{"uid":3,"components":{{"Transform":{{"pos":[0,0,0]}},"Terrain":{{"heightmap":"wall.png","size":[40,40],"height":20,"color":[1,1,1,1]}},"TerrainLayers":{{"splat":"splat.png","layers":"{layers}"}}}}}}"#);
        let text = format!(r#"{{"lunaris":1,"entities":[{cam},{sun},{terrain},{env}]}}"#);
        let mut sim = Sim::new(1);
        sim.asset_root = Some(dir.clone());
        sim.load_scene(&lunaris_json::parse(&text).expect("scene JSON")).expect("scene loads");
        let mut tex = TextureCache::new(Some(dir.clone()));
        let opts = RenderOptions { width: 320, height: 180, ..RenderOptions::default() };
        gpu.render_to_image(&mut sim.world, &mut tex, &opts, &Quality::high()).expect("render")
    };
    // the wall fills the middle of the picture; how green it is (the ground layer is grass, the cliff layer is concrete)
    let greenness = |f: &Frame| -> f32 {
        let mut sum = 0.0;
        for y in 70..110u32 {
            for x in 130..190u32 {
                let p = px(f, x, y);
                sum += p[1] as f32 - (p[0] as f32 + p[2] as f32) * 0.5;
            }
        }
        sum / (40.0 * 60.0)
    };
    let plain = photo("grass grass\\nstone concrete");
    let cliffs = photo("grass grass\\nstone concrete cliff=1");
    let (a, b) = (greenness(&plain), greenness(&cliffs));
    assert!(a > b + 8.0, "the wall shows the grass layer stretched down it without a cliff layer ({a:.1}) and the concrete with one ({b:.1})");
}

#[test]
fn culling_leaves_the_picture_exactly_as_it_was_and_leaves_out_what_the_view_cannot_see() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    // 100 cubes on a grid 180 m across; the camera stands on the south edge looking north, so the rows behind it and the ones far to the sides are out of the view; the sun casts their shadows
    let cubes: Vec<String> = (0..100)
        .map(|i| format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{},1,{}],"scale":[2,2,2]}},"MeshRenderer":{{"mesh":"cube","color":[0.9,{},0.2,1]}}}}}}"#, 10 + i, (i % 10) as f32 * 20.0 - 90.0, (i / 10) as f32 * 20.0 - 90.0, 0.2 + 0.07 * (i % 7) as f32))
        .collect();
    let env = r#"{"uid":9,"components":{"Environment":{"sky":true,"clouds":0,"ambient":1.0,"ssao":1.0,"bloom":0,"vignette":0,"shadows":true,"tonemap":1,"shadow_distance":250}}}"#;
    let mut list: Vec<&str> = vec![CAMERA_3D, SUN, FLOOR, env];
    list.extend(cubes.iter().map(|s| s.as_str()));
    gpu.culling = false;
    let all = draw(&mut gpu, &list, &Quality::high(), 320, 180);
    let uncut = gpu.last_cull;
    gpu.culling = true;
    let seen = draw(&mut gpu, &list, &Quality::high(), 320, 180);
    let cut = gpu.last_cull;
    assert_eq!(uncut.1, 0, "nothing is left out with culling off");
    assert!(cut.0 == uncut.0 && cut.1 >= 40, "most of the cubes are out of the view: {cut:?}");
    assert!(all.rgba == seen.rgba, "the picture is the same with and without culling ({} of {} bytes differ)", all.rgba.iter().zip(&seen.rgba).filter(|(a, b)| a != b).count(), all.rgba.len());
}

#[test]
fn a_cut_out_material_is_cut_out_of_the_picture_the_shadow_and_the_depth_where_its_texture_is_clear() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let dir = std::env::temp_dir().join(format!("lunaris-cutout-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let write_png = |name: &str, rgba: [u8; 4]| {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&rgba.repeat(4)).unwrap();
        }
        std::fs::write(dir.join(name), out).unwrap();
    };
    write_png("clear.png", [255, 0, 0, 0]);
    write_png("solid.png", [255, 0, 0, 255]);
    // a 4 m square lying flat, with one material that is cut out where its picture's alpha is under a half and drawn from both sides
    let floats = |v: &[f32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    let mut bin = floats(&[-2.0, 0.0, -2.0, 2.0, 0.0, -2.0, 2.0, 0.0, 2.0, -2.0, 0.0, 2.0]);
    bin.extend(floats(&[0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0]));
    bin.extend(floats(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]));
    bin.extend([0u8, 0, 1, 0, 2, 0, 0, 0, 2, 0, 3, 0]);
    std::fs::write(dir.join("quad.bin"), &bin).unwrap();
    for (name, picture) in [("quad_clear.gltf", "clear.png"), ("quad_solid.gltf", "solid.png")] {
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"NORMAL":1,"TEXCOORD_0":2}},"indices":3,"material":0}}]}}],
            "materials":[{{"pbrMetallicRoughness":{{"baseColorTexture":{{"index":0}}}},"alphaMode":"MASK","alphaCutoff":0.5,"doubleSided":true}}],
            "textures":[{{"source":0}}],"images":[{{"uri":"{picture}"}}],
            "buffers":[{{"byteLength":{},"uri":"quad.bin"}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":48}},{{"buffer":0,"byteOffset":48,"byteLength":48}},{{"buffer":0,"byteOffset":96,"byteLength":32}},{{"buffer":0,"byteOffset":128,"byteLength":12}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3"}},{{"bufferView":1,"componentType":5126,"count":4,"type":"VEC3"}},{{"bufferView":2,"componentType":5126,"count":4,"type":"VEC2"}},{{"bufferView":3,"componentType":5123,"count":6,"type":"SCALAR"}}]}}"#,
            bin.len()
        );
        std::fs::write(dir.join(name), json).unwrap();
    }
    let cam = r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0.1,0.1,0.15,1]},"Transform":{"pos":[0,14,8],"rot":[-0.55,0,0,0.84]}}}"#;
    let env = r#"{"uid":9,"components":{"Environment":{"sky":true,"clouds":0,"ambient":1.0,"ssao":1.0,"bloom":0,"vignette":0,"shadows":true,"tonemap":1,"shadow_distance":60}}}"#;
    let mut photo = |model: Option<&str>| -> Frame {
        let quad = model.map(|m| format!(r#",{{"uid":5,"components":{{"Transform":{{"pos":[0,2.5,0]}},"MeshRenderer":{{"mesh":"{m}"}}}}}}"#)).unwrap_or_default();
        let text = format!(r#"{{"lunaris":1,"entities":[{cam},{SUN},{FLOOR},{env}{quad}]}}"#);
        let mut sim = Sim::new(1);
        sim.asset_root = Some(dir.clone());
        sim.load_scene(&lunaris_json::parse(&text).expect("scene JSON")).expect("scene loads");
        let mut tex = TextureCache::new(Some(dir.clone()));
        let opts = RenderOptions { width: 320, height: 180, ..RenderOptions::default() };
        gpu.render_to_image(&mut sim.world, &mut tex, &opts, &Quality::high()).expect("render")
    };
    let (none, clear, solid) = (photo(None), photo(Some("quad_clear.gltf")), photo(Some("quad_solid.gltf")));
    let diff = |a: &Frame, b: &Frame| a.rgba.iter().zip(&b.rgba).map(|(x, y)| (*x as f32 - *y as f32).abs()).sum::<f32>() / a.rgba.len() as f32;
    let (d_clear, d_solid) = (diff(&clear, &none), diff(&solid, &none));
    assert!(d_clear < 0.5, "a square whose picture is clear is not there at all (the same picture as without it: {d_clear} levels off)");
    assert!(d_solid > 6.0 * d_clear.max(0.1), "a solid one is there, with its shadow ({d_solid} levels off against {d_clear})");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn levels_of_detail_change_a_far_picture_by_less_than_a_pixel_and_a_near_one_not_at_all() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let dir = std::env::temp_dir().join(format!("lunaris-lod-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // a hill of 150 x 150 samples (45,000 triangles: enough for levels of detail and for clusters)
    let n = 150usize;
    let mut samples = Vec::with_capacity(n * n * 2);
    for j in 0..n {
        for i in 0..n {
            let (x, z) = (i as f32 / n as f32 * 6.0, j as f32 / n as f32 * 6.0);
            let v = (0.5 + 0.25 * x.sin() * z.cos() + 0.2 * (x * 0.4 + z * 0.7).sin()).clamp(0.0, 1.0);
            samples.extend_from_slice(&((v * 65535.0) as u16).to_be_bytes());
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, n as u32, n as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Sixteen);
        enc.write_header().unwrap().write_image_data(&samples).unwrap();
    }
    std::fs::write(dir.join("hill.png"), out).unwrap();
    let env = r#"{"uid":9,"components":{"Environment":{"sky":true,"clouds":0,"ambient":1.0,"ssao":1.0,"bloom":0,"vignette":0,"shadows":true,"tonemap":1,"shadow_distance":400}}}"#;
    let photo = |gpu: &mut GpuRenderer, z: f32| -> Frame {
        let hill = format!(r#"{{"uid":4,"components":{{"Transform":{{"pos":[-20,0,{z}]}},"Terrain":{{"heightmap":"hill.png","size":[40,40],"height":8,"color":[0.5,0.6,0.4,1]}}}}}}"#);
        let text = format!(r#"{{"lunaris":1,"entities":[{CAMERA_3D},{SUN},{env},{hill}]}}"#).replace(r#""pos":[0,3,9]"#, r#""pos":[0,3,0]"#).replace(r#""rot":[-0.2,0,0,0.98]"#, r#""rot":[0,0,0,1]"#);
        let mut sim = Sim::new(1);
        sim.asset_root = Some(dir.clone());
        sim.load_scene(&lunaris_json::parse(&text).expect("scene JSON")).expect("scene loads");
        let mut tex = TextureCache::new(Some(dir.clone()));
        let opts = RenderOptions { width: 320, height: 180, ..RenderOptions::default() };
        gpu.render_to_image(&mut sim.world, &mut tex, &opts, &Quality::high()).expect("render")
    };
    // (only the error counts here: the rule that gives a triangle a few pixels would take levels off a hill that fills a picture of 320 x 180 pixels with 45,000 of them)
    gpu.lod_tri_pixels = 0.0;
    for (z, label) in [(-10.0f32, "near"), (-420.0, "far")] {
        gpu.lod = false;
        let full = photo(&mut gpu, z);
        let full_tris = gpu.last_triangles;
        gpu.lod = true;
        let coarse = photo(&mut gpu, z);
        let coarse_tris = gpu.last_triangles;
        let mean = full.rgba.iter().zip(&coarse.rgba).map(|(a, b)| (*a as f32 - *b as f32).abs()).sum::<f32>() / full.rgba.len() as f32;
        if label == "near" {
            assert_eq!(full_tris, coarse_tris, "a near mesh keeps all of its triangles");
            assert!(full.rgba == coarse.rgba, "and so the picture is the same");
        } else {
            assert!(coarse_tris * 2 < full_tris, "far off, fewer than half the triangles: {coarse_tris} of {full_tris}");
            assert!(mean < 1.0, "the picture of the coarser levels is within a level of the full one: {mean}");
        }
    }
    gpu.lod = true;
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_picture_with_few_pixels_gets_a_coarser_mesh_than_the_same_scene_in_many() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let dir = std::env::temp_dir().join(format!("lunaris-lodpix-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let n = 150usize;
    let mut samples = Vec::with_capacity(n * n * 2);
    for j in 0..n {
        for i in 0..n {
            let v = (0.5 + 0.2 * ((i as f32) * 0.2).sin() * ((j as f32) * 0.2).cos()).clamp(0.0, 1.0);
            samples.extend_from_slice(&((v * 65535.0) as u16).to_be_bytes());
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, n as u32, n as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Sixteen);
        enc.write_header().unwrap().write_image_data(&samples).unwrap();
    }
    std::fs::write(dir.join("hill.png"), out).unwrap();
    let hill = r#"{"uid":4,"components":{"Transform":{"pos":[-20,0,-10]},"Terrain":{"heightmap":"hill.png","size":[40,40],"height":8,"color":[0.5,0.6,0.4,1]}}}"#;
    let text = format!(r#"{{"lunaris":1,"entities":[{CAMERA_3D},{SUN},{hill}]}}"#).replace(r#""pos":[0,3,9]"#, r#""pos":[0,3,0]"#).replace(r#""rot":[-0.2,0,0,0.98]"#, r#""rot":[0,0,0,1]"#);
    let mut tris = |w: u32, h: u32, rule: f32| -> u64 {
        gpu.lod_tri_pixels = rule;
        let mut sim = Sim::new(1);
        sim.asset_root = Some(dir.clone());
        sim.load_scene(&lunaris_json::parse(&text).expect("scene JSON")).expect("scene loads");
        let mut tex = TextureCache::new(Some(dir.clone()));
        gpu.render_to_image(&mut sim.world, &mut tex, &RenderOptions { width: w, height: h, ..RenderOptions::default() }, &Quality::high()).expect("render");
        gpu.last_triangles
    };
    let (small_off, small_on) = (tris(160, 90, 0.0), tris(160, 90, 3.0));
    let (big_off, big_on) = (tris(1280, 720, 0.0), tris(1280, 720, 3.0));
    assert_eq!(small_off, big_off, "without the rule the same level is drawn at both sizes");
    assert!(small_on * 2 < small_off, "in a picture of 160 x 90 pixels a triangle of a pixel is not worth drawing: {small_on} of {small_off}");
    assert!(big_on > small_on * 2, "in a large picture the same hill keeps far more: {big_on} against {small_on}");
    gpu.lod_tri_pixels = lod_tri_default();
    let _ = std::fs::remove_dir_all(&dir);
}

fn lod_tri_default() -> f32 {
    3.0
}

/// Linear luminance of a display pixel (the high-quality picture is sRGB encoded).
fn lin_lum(c: [u8; 3]) -> f32 {
    let d = |v: u8| {
        let x = v as f32 / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * d(c[0]) + 0.7152 * d(c[1]) + 0.0722 * d(c[2])
}

#[test]
fn lighting_fills_the_shade_bounces_the_sun_and_gives_lamps_a_real_falloff_and_a_world_without_it_is_unchanged() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let q = Quality::high();
    let (w, h) = (480u32, 270u32);
    // a black sky and no ambient of its own; the sun straight down; a slab over the floor throws a shadow no light reaches
    let shade_scene = |extra: &str| -> Vec<String> {
        let mut v: Vec<String> = [
            r#"{"uid":1,"components":{"Camera":{"fov_y":55,"clear":[0,0,0,1]},"Transform":{"pos":[0,5,9],"rot":[-0.3,0,0,0.954]}}}"#,
            r#"{"uid":2,"components":{"Light":{"kind":0,"intensity":3.0},"Transform":{"rot":[-0.7071,0,0,0.7071]}}}"#,
            r#"{"uid":3,"components":{"Transform":{"scale":[40,1,40]},"MeshRenderer":{"mesh":"plane","color":[0.8,0.8,0.8,1],"roughness":0.9}}}"#,
            r#"{"uid":4,"components":{"Transform":{"pos":[0,3,0],"scale":[5,0.2,5]},"MeshRenderer":{"mesh":"cube","color":[0.5,0.5,0.5,1]}}}"#,
            r#"{"uid":9,"components":{"Environment":{"sky":false,"sky_top":[0,0,0],"sky_horizon":[0,0,0],"ground":[0,0,0],"ambient":1.0,"bloom":0,"ssao":0,"shadows":true,"tonemap":3,"exposure":1.0}}}"#,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if !extra.is_empty() {
            v.push(format!(r#"{{"uid":10,"components":{{"Lighting":{extra}}}}}"#));
        }
        v
    };
    let draw_scene = |gpu: &mut GpuRenderer, v: &[String], w: u32, h: u32| -> Frame {
        let refs: Vec<&str> = v.iter().map(|s| s.as_str()).collect();
        draw(gpu, &refs, &q, w, h)
    };
    // the darkest hundredth of the floor: the shadow of the slab
    let shadow_level = |f: &Frame| -> f32 {
        let mut l: Vec<f32> = (0..f.ids.len()).filter(|&i| f.ids[i] == 3).map(|i| lin_lum(px(f, i as u32 % f.width, i as u32 / f.width))).collect();
        assert!(l.len() > 2000, "{} floor pixels", l.len());
        l.sort_by(|a, b| a.total_cmp(b));
        l[..l.len() / 100].iter().sum::<f32>() / (l.len() / 100) as f32
    };
    let plain = draw_scene(&mut gpu, &shade_scene(""), w, h);
    let off = draw_scene(&mut gpu, &shade_scene(r#"{"enabled":false}"#), w, h);
    assert_eq!(plain.rgba, off.rgba, "a disabled Lighting changes nothing");
    let base = shadow_level(&plain);
    assert!(base < 0.004, "with no sky the shadow is black: {base}");
    // fill: the shade is no longer black
    let filled = shadow_level(&draw_scene(&mut gpu, &shade_scene(r#"{"fill":[0.05,0.05,0.05],"bounce":0,"lamp_falloff":0,"shadow_distance":0,"softness":0}"#), w, h));
    eprintln!("shadow: plain {base}, with fill {filled}");
    assert!(filled > base + 0.02, "fill lifts the shade: {base} -> {filled}");
    // bounce: the sun's light comes back as ambient light and lifts the shade too (nothing without a sun above the horizon)
    let bounced = shadow_level(&draw_scene(&mut gpu, &shade_scene(r#"{"fill":[0,0,0],"bounce":0.3,"lamp_falloff":0,"shadow_distance":0,"softness":0}"#), w, h));
    eprintln!("shadow with bounce {bounced}");
    assert!(bounced > base + 0.02, "bounce lifts the shade: {base} -> {bounced}");
    // the lit floor is not changed by fill alone by more than the fill itself
    let lamp_scene = |extra: &str| -> Vec<String> {
        let mut v: Vec<String> = [
            r#"{"uid":1,"components":{"Camera":{"fov_y":60,"clear":[0,0,0,1]},"Transform":{"pos":[0,12,0],"rot":[-0.7071,0,0,0.7071]}}}"#,
            r#"{"uid":3,"components":{"Transform":{"scale":[60,1,60]},"MeshRenderer":{"mesh":"plane","color":[0.8,0.8,0.8,1],"roughness":0.9}}}"#,
            r#"{"uid":6,"components":{"Transform":{"pos":[0,3,0]},"Light":{"kind":1,"intensity":0.4,"range":10,"color":[1,1,1]}}}"#,
            r#"{"uid":9,"components":{"Environment":{"sky":false,"sky_top":[0,0,0],"sky_horizon":[0,0,0],"ground":[0,0,0],"ambient":1.0,"bloom":0,"ssao":0,"shadows":false,"tonemap":3,"exposure":1.0}}}"#,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if !extra.is_empty() {
            v.push(format!(r#"{{"uid":10,"components":{{"Lighting":{extra}}}}}"#));
        }
        v
    };
    // the floor under the lamp (3 m away) and 5.2 m to the side (6 m away: 60% of the range); the camera looks straight down from 12 m, 60 degrees wide
    let at = |f: &Frame, offset: f32| -> f32 {
        let y = (h as f32 * 0.5 + offset / (12.0 * 30f32.to_radians().tan()) * h as f32 * 0.5) as u32;
        let x = w / 2;
        let mut s = 0.0;
        for dx in 0..5 {
            s += lin_lum(px(f, x - 2 + dx, y.min(h - 1)));
        }
        s / 5.0
    };
    let old = draw_scene(&mut gpu, &lamp_scene(r#"{"fill":[0,0,0],"bounce":0,"lamp_falloff":0,"shadow_distance":0,"softness":0}"#), w, h);
    let real = draw_scene(&mut gpu, &lamp_scene(r#"{"fill":[0,0,0],"bounce":0,"lamp_falloff":1,"shadow_distance":0,"softness":0}"#), w, h);
    let (old_c, old_e, new_c, new_e) = (at(&old, 0.0), at(&old, 5.2), at(&real, 0.0), at(&real, 5.2));
    eprintln!("lamp pool: old centre {old_c} edge {old_e}; real centre {new_c} edge {new_e}");
    assert!(old_c > 0.05 && new_c > 0.05, "the pool is lit");
    assert!(((new_e - old_e) / old_e).abs() < 0.25, "the two curves meet at 60% of the range: {old_e} vs {new_e}");
    assert!(new_c / new_e > 1.4 * (old_c / old_e), "a real lamp is brighter at the core compared with its tail: {} vs {}", new_c / new_e, old_c / old_e);
    // no component: the legacy falloff exactly
    let none = draw_scene(&mut gpu, &lamp_scene(""), w, h);
    assert_eq!(none.rgba, old.rgba, "lamp_falloff 0 is the old curve to the bit");
}

#[test]
fn a_big_bright_light_over_a_far_wall_is_kept_among_many_small_lamps_nearer_the_camera() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let q = Quality::high();
    let (w, h) = (320u32, 180u32);
    // a white wall 340 m from the camera, lit by a big light 60 m in front of it (range 200: it does not reach the camera); `lamps` tiny lamps stand by the camera
    let scene = |lamps: usize| -> Vec<String> {
        let mut v: Vec<String> = vec![
            r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0,0,0,1]},"Transform":{"pos":[400,100,0],"rot":[0,0.7071,0,0.7071]}}}"#.to_string(),
            r#"{"uid":3,"components":{"Transform":{"pos":[0,100,0],"scale":[2,200,200]},"MeshRenderer":{"mesh":"cube","color":[0.8,0.8,0.8,1],"roughness":0.9}}}"#.to_string(),
            r#"{"uid":5,"components":{"Transform":{"pos":[60,100,0]},"Light":{"kind":1,"intensity":0.5,"range":200,"color":[1,1,1]}}}"#.to_string(),
            r#"{"uid":9,"components":{"Environment":{"sky":false,"sky_top":[0,0,0],"sky_horizon":[0,0,0],"ground":[0,0,0],"ambient":0.0,"bloom":0,"ssao":0,"shadows":false,"tonemap":3,"exposure":1.0}}}"#.to_string(),
        ];
        for i in 0..lamps {
            v.push(format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{},100,3]}},"Light":{{"kind":1,"intensity":0.0001,"range":3,"color":[1,1,1]}}}}}}"#, 20 + i, 399.0 + i as f32 * 0.1));
        }
        v
    };
    let wall = |gpu: &mut GpuRenderer, lamps: usize| -> f32 {
        let s = scene(lamps);
        let refs: Vec<&str> = s.iter().map(|x| x.as_str()).collect();
        let f = draw(gpu, &refs, &q, w, h);
        let (mut n, mut sum) = (0.0f32, 0.0f32);
        for i in 0..f.ids.len() {
            if f.ids[i] == 3 {
                n += 1.0;
                sum += lin_lum(px(&f, i as u32 % f.width, i as u32 / f.width));
            }
        }
        assert!(n > 1000.0, "{n} wall pixels");
        sum / n
    };
    let few = wall(&mut gpu, 10);
    let many = wall(&mut gpu, 40);
    eprintln!("wall: with 11 lights {few}, with 41 lights {many}");
    assert!(few > 0.03, "the big light lights the wall: {few}");
    assert!(many > 0.8 * few, "the big light is still one of the sixteen when there are many lamps: {few} vs {many}");
}

#[test]
fn lighting_can_let_four_lamps_cast_shadows_where_two_do_by_default() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let q = Quality::high();
    let (w, h) = (640u32, 360u32);
    // three lamps 12 m apart, each with a cube beside it that throws a shadow on the floor; the camera looks straight down from 18 m
    let scene = |points: &str| -> Vec<String> {
        let mut v: Vec<String> = vec![
            r#"{"uid":1,"components":{"Camera":{"fov_y":60,"clear":[0,0,0,1]},"Transform":{"pos":[0,18,0],"rot":[-0.7071,0,0,0.7071]}}}"#.to_string(),
            r#"{"uid":3,"components":{"Transform":{"scale":[80,1,80]},"MeshRenderer":{"mesh":"plane","color":[0.8,0.8,0.8,1],"roughness":0.9}}}"#.to_string(),
            r#"{"uid":9,"components":{"Environment":{"sky":false,"sky_top":[0,0,0],"sky_horizon":[0,0,0],"ground":[0,0,0],"ambient":0.0,"bloom":0,"ssao":0,"shadows":true,"tonemap":3,"exposure":1.0}}}"#.to_string(),
        ];
        for (i, x) in [-12.0f32, 0.0, 12.0].iter().enumerate() {
            v.push(format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{},4,0]}},"Light":{{"kind":1,"intensity":0.5,"range":9,"color":[1,1,1]}}}}}}"#, 20 + i, x));
            v.push(format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{},1.2,2.0],"scale":[1.4,1.4,1.4]}},"MeshRenderer":{{"mesh":"cube","color":[0.5,0.5,0.5,1]}}}}}}"#, 30 + i, x));
        }
        if !points.is_empty() {
            v.push(format!(r#"{{"uid":10,"components":{{"Lighting":{{"fill":[0,0,0],"bounce":0,"lamp_falloff":0,"shadow_distance":0,"softness":0,"point_shadows":{points}}}}}}}"#));
        }
        v
    };
    // the floor in the shadow of each lamp's cube (the cube stands 2 m to the side of the lamp, the shadow falls further out) against the floor at the same distance on the other side of the lamp
    let ratios = |gpu: &mut GpuRenderer, points: &str| -> Vec<f32> {
        let s = scene(points);
        let refs: Vec<&str> = s.iter().map(|x| x.as_str()).collect();
        let f = draw(gpu, &refs, &q, w, h);
        if let Ok(dir) = std::env::var("LUNARIS_TEST_DUMP") {
            let file = std::fs::File::create(std::path::Path::new(&dir).join(format!("psh_{}.png", if points.is_empty() { "two" } else { points }))).expect("dump file");
            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), f.width, f.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().expect("png header").write_image_data(&f.rgba).expect("png data");
        }
        let half = 18.0 * 30f32.to_radians().tan();
        let at = |x: f32, z: f32| -> f32 {
            let (px_, py_) = ((w as f32 * 0.5 + x / (half * w as f32 / h as f32) * w as f32 * 0.5) as u32, (h as f32 * 0.5 + z / half * h as f32 * 0.5) as u32);
            let mut s = 0.0;
            for dy in 0..3 {
                for dx in 0..3 {
                    s += lin_lum(px(&f, px_ - 1 + dx, py_ - 1 + dy));
                }
            }
            s / 9.0
        };
        [-12.0f32, 0.0, 12.0].iter().map(|&x| at(x, 3.6) / at(x, -3.6).max(1e-4)).collect()
    };
    let four = ratios(&mut gpu, "4");
    let two = ratios(&mut gpu, "");
    eprintln!("shadow ratios, two shadowed lamps {two:?}, four {four:?}");
    assert!(four.iter().all(|r| *r < 0.6), "with four, every lamp's cube throws a shadow: {four:?}");
    assert!(two.iter().any(|r| *r > 0.8), "with two, one lamp throws none: {two:?}");
}

/// A `ParticleSystem` entity at `pos` whose effect is `effect` (the JSON, put in the component as text).
fn fx_entity(uid: u32, pos: [f32; 3], effect: &str) -> String {
    let text = lunaris_json::Json::str(effect).to_string_compact().expect("text");
    format!(r#"{{"uid":{uid},"components":{{"Transform":{{"pos":[{},{},{}]}},"ParticleSystem":{{"effect":{text}}}}}}}"#, pos[0], pos[1], pos[2])
}

#[test]
fn soft_particles_fade_out_where_they_meet_a_surface_and_hard_ones_cut_a_line() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    // a big opaque disc of a particle standing across a floor, seen from the side and a little above: a hard picture is cut by the floor in a straight line, a soft one thins out to nothing as it nears the floor
    let scene = |gpu: &mut GpuRenderer, soft: f32| {
        let effect = format!(
            r#"{{"emitters":[{{"loop":false,"spawn":{{"bursts":[{{"time":0,"count":1}}]}},"init":{{"life":100,"speed":0,"size":5,"color":[1,1,1,1]}},"render":{{"texture":"circle","blend":"alpha","soft":{soft}}}}}]}}"#
        );
        let cam = r#"{"uid":1,"components":{"Camera":{"fov_y":40,"clear":[0,0,0,1]},"Transform":{"pos":[0,2.2,9],"rot":[-0.12,0,0,0.9928]}}}"#;
        let floor = r#"{"uid":3,"components":{"Transform":{"scale":[40,1,40]},"MeshRenderer":{"mesh":"plane","color":[0.05,0.05,0.05,1],"roughness":1.0}}}"#;
        let env = r#"{"uid":9,"components":{"Environment":{"sky":false,"ambient":0.4,"ssao":0,"bloom":0,"vignette":0,"shadows":false,"tonemap":1}}}"#;
        let fx = fx_entity(4, [0.0, 1.5, 0.0], &effect);
        draw(gpu, &[cam, floor, env, &fx], &Quality::high(), 320, 180)
    };
    let hard = scene(&mut gpu, 0.0);
    let soft = scene(&mut gpu, 1.5);
    // down the middle column: where the white disc stops and the dark floor shows
    let column = |f: &Frame| -> Vec<f32> { (0..180).map(|y| lum(px(f, 160, y))).collect() };
    let (h, s) = (column(&hard), column(&soft));
    let edge = (0..180usize).rev().find(|&y| h[y] > 120.0).expect("the disc is in the picture");
    assert!(edge > 20 && edge < 175, "the cut is somewhere in the picture: row {edge}");
    // right above the cut the hard picture is still white and the soft one has faded; far above it they agree
    let band = |v: &[f32], a: usize, b: usize| v[a..=b].iter().sum::<f32>() / (b - a + 1) as f32;
    let (near_h, near_s) = (band(&h, edge - 4, edge - 1), band(&s, edge - 4, edge - 1));
    let top = edge.saturating_sub(60);
    let (far_h, far_s) = (band(&h, top, top + 3), band(&s, top, top + 3));
    eprintln!("cut at row {edge}: hard {near_h:.0} soft {near_s:.0}; {} rows up: hard {far_h:.0} soft {far_s:.0}; soft at the cut {:.0}", edge - top, band(&s, edge - 1, edge));
    assert!(near_h > 150.0 && near_s < near_h * 0.8, "at the cut: hard {near_h:.0}, soft {near_s:.0}");
    assert!(far_s > far_h * 0.85, "away from the floor the picture is whole: hard {far_h:.0}, soft {far_s:.0}");
    assert!(band(&s, edge - 1, edge) < far_s * 0.7, "it thins out towards the floor: {:.0} against {far_s:.0}", band(&s, edge - 1, edge));
    // a soft distance of 0 is exactly what a picture without the field is
    assert_eq!(scene(&mut gpu, 0.0).rgba, hard.rgba);
}

#[test]
fn the_light_of_a_particle_lights_the_floor_round_it() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let scene = |gpu: &mut GpuRenderer, light: &str| {
        let effect = format!(
            r#"{{"emitters":[{{"loop":false,"spawn":{{"bursts":[{{"time":0,"count":1}}]}},"init":{{"life":100,"speed":0,"size":0.3,"color":[1,0.5,0.2,1]}},"render":{{"texture":"soft","blend":"add"{light}}}}}]}}"#
        );
        let cam = r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0,0,0,1]},"Transform":{"pos":[0,6,8],"rot":[-0.38,0,0,0.925]}}}"#;
        let floor = r#"{"uid":3,"components":{"Transform":{"scale":[40,1,40]},"MeshRenderer":{"mesh":"plane","color":[0.6,0.6,0.6,1],"roughness":1.0}}}"#;
        let env = r#"{"uid":9,"components":{"Environment":{"sky":false,"ambient":0.05,"ssao":0,"bloom":0,"vignette":0,"shadows":false,"tonemap":1}}}"#;
        let fx = fx_entity(4, [0.0, 1.2, 0.0], &effect);
        draw(gpu, &[cam, floor, env, &fx], &Quality::high(), 320, 180)
    };
    let dark = scene(&mut gpu, "");
    let lit = scene(&mut gpu, r#","light":{"intensity":30,"range":5,"max":1}"#);
    // the floor a metre or two from the spark is brighter and warmer; far away it is as it was
    let patch = |f: &Frame, x: u32, y: u32| -> [f32; 3] {
        let mut s = [0.0f32; 3];
        for dy in 0..6 {
            for dx in 0..6 {
                let c = px(f, x + dx, y + dy);
                for k in 0..3 {
                    s[k] += c[k] as f32 / 36.0;
                }
            }
        }
        s
    };
    let (a, b) = (patch(&dark, 168, 64), patch(&lit, 168, 64));
    assert!(lum([b[0] as u8, b[1] as u8, b[2] as u8]) > lum([a[0] as u8, a[1] as u8, a[2] as u8]) + 8.0, "near: {a:?} -> {b:?}");
    assert!(b[0] > b[2] + 3.0, "warm: {b:?}");
    let (c, d) = (patch(&dark, 300, 8), patch(&lit, 300, 8));
    assert!((d[0] - c[0]).abs() < 3.0, "far: {c:?} -> {d:?}");
}

/// Every ready-made effect of `examples/fx` on the card at a few clocks, as pictures (`LUNARIS_TEST_DUMP=dir` writes them; without it the test only checks that each effect puts something in the picture and that it is the same picture twice).
#[test]
fn the_example_effects_draw_and_play_on_the_card() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // (file, camera distance, camera height, height looked at, clocks)
    let table: [(&str, f32, f32, f32, &[f32]); 18] = [
        ("campfire", 5.0, 1.6, 0.8, &[0.5, 2.0, 5.0]),
        ("burning_wreck", 12.0, 3.5, 1.8, &[1.0, 3.0, 6.0]),
        ("muzzle_flash", 2.5, 1.4, 1.2, &[0.02, 0.06, 0.25]),
        ("bullet_impact", 3.0, 1.4, 0.5, &[0.05, 0.2, 0.6]),
        ("explosion", 26.0, 7.0, 4.0, &[0.1, 0.4, 1.0, 3.0]),
        ("tracer", 10.0, 1.6, 1.2, &[0.05, 0.15]),
        ("toxic_dust", 14.0, 1.8, 1.5, &[3.0, 6.0]),
        ("rain_splash", 12.0, 5.0, 0.0, &[1.0, 1.4]),
        ("mind_motes", 40.0, 4.0, 3.0, &[2.0, 6.0, 12.0]),
        ("voxel/fire_barrel", 4.0, 1.8, 1.0, &[1.0, 3.0]),
        ("voxel/muzzle_flash", 2.0, 1.2, 1.2, &[0.03, 0.08, 0.5]),
        ("voxel/impact_concrete", 2.0, 1.2, 0.3, &[0.1, 0.3, 0.8]),
        ("voxel/tracer", 8.0, 1.5, 1.3, &[0.05, 0.1]),
        ("voxel/explosion", 22.0, 6.0, 3.5, &[0.1, 0.4, 1.2, 3.0]),
        ("voxel/toxic_dust", 12.0, 1.8, 1.5, &[4.0]),
        ("voxel/rain_splash", 9.0, 4.0, 0.0, &[1.0]),
        ("voxel/machine_death", 3.5, 1.5, 0.8, &[0.1, 0.4, 1.0]),
        ("voxel/dust_puff", 3.5, 1.2, 0.6, &[0.2, 0.8]),
    ];
    for (name, dist, height, ty, clocks) in table {
        let a = (height - ty).atan2(dist);
        let cam = format!(r#"{{"uid":1,"components":{{"Camera":{{"fov_y":55,"clear":[0.02,0.03,0.07,1]}},"Transform":{{"pos":[0,{height},{dist}],"rot":[{},0,0,{}]}}}}}}"#, -(a / 2.0).sin(), (a / 2.0).cos());
        let floor = r#"{"uid":3,"components":{"Transform":{"scale":[200,1,200]},"MeshRenderer":{"mesh":"plane","texture":"stone","uv_scale":[80,80],"color":[0.4,0.42,0.46,1],"roughness":0.95}}}"#;
        let moon = r#"{"uid":2,"components":{"Light":{"kind":0,"color":[0.5,0.6,0.95],"intensity":0.9},"Transform":{"rot":[-0.55,0.3,0.1,0.77]}}}"#;
        let env = r#"{"uid":9,"components":{"Environment":{"sky":true,"sky_top":[0.03,0.05,0.12],"sky_horizon":[0.12,0.16,0.26],"ambient":0.4,"bloom":0.5,"bloom_threshold":0.8,"fog":0.003,"fog_color":[0.1,0.13,0.2]}}}"#;
        let fx = format!(r#"{{"uid":4,"components":{{"Transform":{{"pos":[0,0.05,0]}},"ParticleSystem":{{"effect":"examples/fx/{name}.fx.json","playing":false,"time":0}}}}}}"#);
        let text = format!(r#"{{"lunaris":1,"entities":[{cam},{floor},{moon},{env},{fx}]}}"#);
        let mut tex = TextureCache::new(Some(root.clone()));
        let mut sim = Sim::new(1);
        sim.load_scene(&lunaris_json::parse(&text).expect("scene JSON")).expect("scene loads");
        let mut frames = Vec::new();
        for &c in clocks {
            sim.apply(&lunaris_sim::Command::Set { uid: 4, component: "ParticleSystem".into(), value: lunaris_json::parse(&format!(r#"{{"time":{c}}}"#)).unwrap() }).unwrap();
            let opts = RenderOptions { width: 640, height: 360, ..RenderOptions::default() };
            frames.push((c, gpu.render_to_image(&mut sim.world, &mut tex, &opts, &Quality::high()).expect("render")));
        }
        assert!(tex.warnings.is_empty(), "{name}: {:?}", tex.warnings);
        // each effect shows up: some picture of it differs from the bare floor (an empty system at the first clock)
        let busiest = frames.iter().map(|(_, f)| f.rgba.chunks(4).filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 220).count()).max().unwrap();
        assert!(busiest > 20, "{name}: nothing in the picture ({busiest})");
        if let Ok(dir) = std::env::var("LUNARIS_TEST_DUMP") {
            for (c, f) in &frames {
                let file = std::fs::File::create(std::path::Path::new(&dir).join(format!("fx_{}_{c}.png", name.replace('/', "_")))).expect("dump file");
                let mut enc = png::Encoder::new(std::io::BufWriter::new(file), f.width, f.height);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                enc.write_header().expect("png header").write_image_data(&f.rgba).expect("png data");
            }
        }
    }
}

/// A `ParticleSystem` entity as `fx_entity` makes it, with its clock set (the picture is then the first the system is in, stepped up to that clock).
fn fx_entity_at(uid: u32, pos: [f32; 3], effect: &str, time: f32) -> String {
    let text = lunaris_json::Json::str(effect).to_string_compact().expect("text");
    format!(r#"{{"uid":{uid},"components":{{"Transform":{{"pos":[{},{},{}]}},"ParticleSystem":{{"effect":{text},"time":{time},"playing":false}}}}}}"#, pos[0], pos[1], pos[2])
}

/// The middle of what is lit in a picture (pixels, x then y) and how much light there is.
fn lit_centre(f: &Frame, floor: f32) -> Option<(f32, f32, f32)> {
    let (mut sx, mut sy, mut sw) = (0.0f64, 0.0f64, 0.0f64);
    for y in 0..f.height {
        for x in 0..f.width {
            let w = (lum(px(f, x, y)) - floor).max(0.0) as f64;
            sx += x as f64 * w;
            sy += y as f64 * w;
            sw += w;
        }
    }
    (sw > 0.0).then(|| ((sx / sw) as f32, (sy / sw) as f32, sw as f32))
}

fn fx_scene(effect: &str, time: f32) -> Vec<String> {
    vec![
        r#"{"uid":1,"components":{"Camera":{"fov_y":40,"clear":[0,0,0,1]},"Transform":{"pos":[0,0,10]}}}"#.to_string(),
        r#"{"uid":9,"components":{"Environment":{"sky":false,"ambient":1.0,"ssao":0,"bloom":0,"vignette":0,"shadows":false,"tonemap":1}}}"#.to_string(),
        fx_entity_at(4, [0.0, 0.0, 0.0], effect, time),
    ]
}

#[test]
fn an_emitter_the_card_simulates_flies_where_the_steps_say() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let (w, h) = (320u32, 180u32);
    // one white disc of a particle thrown along +x at 4 m/s; a camera 10 m away with a 40 degree view sees 24.7 pixels to the metre
    let flight = |gpu: &mut GpuRenderer, update: &str, time: f32| {
        let effect = format!(
            r#"{{"emitters":[{{"simulate":"gpu","space":"world","loop":false,"spawn":{{"bursts":[{{"time":0,"count":1}}]}},"init":{{"life":100,"speed":4,"direction":[1,0,0],"size":0.5,"color":[1,1,1,1]}},"update":{update},"render":{{"texture":"circle","blend":"add"}}}}]}}"#
        );
        let ents = fx_scene(&effect, time);
        let refs: Vec<&str> = ents.iter().map(|s| s.as_str()).collect();
        draw(gpu, &refs, &Quality::classic(), w, h)
    };
    let (a, b) = (flight(&mut gpu, "{}", 0.25), flight(&mut gpu, "{}", 0.75));
    let (ca, cb) = (lit_centre(&a, 8.0).expect("lit at 0.25"), lit_centre(&b, 8.0).expect("lit at 0.75"));
    // half a second at 4 m/s is 2 m: about 49 pixels to the right, and no move up or down
    let dx = cb.0 - ca.0;
    assert!((dx - 49.4).abs() < 4.0, "moved {dx} pixels: {ca:?} -> {cb:?}");
    assert!((cb.1 - ca.1).abs() < 1.5, "level: {ca:?} -> {cb:?}");
    // the size is the particle's: 0.5 m is about 12 pixels across
    let area = a.rgba.chunks(4).filter(|p| p[0] > 100).count();
    assert!((60..220).contains(&area), "{area} pixels");
    // gravity bends the flight down (the picture's y grows downwards): 0.5 g t^2 at 0.75 s is 1.4 m, about 35 pixels
    let g = flight(&mut gpu, r#"{"gravity":[0,-5,0]}"#, 0.75);
    let cg = lit_centre(&g, 8.0).expect("lit with gravity");
    assert!((cg.1 - cb.1 - 0.5 * 5.0 * 0.75 * 0.75 * 24.7).abs() < 5.0, "fell {} pixels", cg.1 - cb.1);
    // the same history is the same picture
    assert_eq!(flight(&mut gpu, "{}", 0.75).rgba, b.rgba);
    // a plane stops it: a floor at y = -1 (24.7 pixels below the middle) that bounces nothing back, so it ends on the floor line however long it falls
    let floor = flight(&mut gpu, r#"{"gravity":[0,-30,0],"collide":{"bounce":0,"friction":1,"planes":[{"normal":[0,1,0],"d":-1}]}}"#, 1.5);
    let cf = lit_centre(&floor, 8.0).expect("lit on the floor");
    assert!(cf.1 > 90.0 + 24.7 - 3.0 && cf.1 < 90.0 + 24.7 + 3.0, "on the floor line: {cf:?}");
}

#[test]
fn the_card_and_the_processor_make_clouds_of_the_same_kind() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let effect = r#"{"emitters":[{"simulate":"gpu","space":"world","spawn":{"rate":600,"shape":{"type":"sphere","radius":0.6}},"init":{"life":[1.0,2.0],"speed":[0.5,2.5],"spread":180,"size":[0.08,0.16],"color":[1,0.6,0.3,1]},"update":{"gravity":[0,-1,0],"drag":0.4,"noise":{"strength":1.5,"scale":1.2,"speed":0.3}},"curves":{"alpha":[[0,0],[0.15,1],[1,0]]},"render":{"texture":"soft","blend":"add"}}]}"#;
    let ents = fx_scene(effect, 4.0);
    let on_the_card = {
        let refs: Vec<&str> = ents.iter().map(|s| s.as_str()).collect();
        draw(&mut gpu, &refs, &Quality::classic(), 320, 180)
    };
    // the reference renderer steps the same emitter on the processor (it is drawn there as any other)
    let on_the_cpu = {
        let text = format!(r#"{{"lunaris":1,"entities":[{}]}}"#, ents.join(","));
        let mut sim = Sim::new(1);
        sim.load_scene(&lunaris_json::parse(&text).expect("scene JSON")).expect("scene loads");
        lunaris_render::render(&mut sim.world, &mut TextureCache::new(None), &RenderOptions { width: 320, height: 180, ..RenderOptions::default() }).expect("render")
    };
    let (g, c) = (lit_centre(&on_the_card, 4.0).expect("the card's cloud is in the picture"), lit_centre(&on_the_cpu, 4.0).expect("the processor's cloud is in the picture"));
    // the same amount of light, about the same middle, and the same width (the particles are not the same ones: other random numbers)
    assert!((g.2 / c.2 - 1.0).abs() < 0.35, "light: card {} against processor {}", g.2, c.2);
    assert!((g.0 - c.0).abs() < 12.0 && (g.1 - c.1).abs() < 12.0, "middle: card {g:?} against processor {c:?}");
    let spread = |f: &Frame, mid: (f32, f32, f32)| -> f32 {
        let (mut s, mut n) = (0.0f64, 0.0f64);
        for y in 0..f.height {
            for x in 0..f.width {
                let w = (lum(px(f, x, y)) - 4.0).max(0.0) as f64;
                s += w * (((x as f32 - mid.0).powi(2) + (y as f32 - mid.1).powi(2)) as f64);
                n += w;
            }
        }
        (s / n).sqrt() as f32
    };
    let (sg, sc) = (spread(&on_the_card, g), spread(&on_the_cpu, c));
    assert!((sg / sc - 1.0).abs() < 0.3, "width: card {sg} against processor {sc}");
}

#[test]
fn ten_thousand_motes_on_the_card_cost_the_processor_nothing_it_can_measure() {
    let Some(mut gpu) = gpu_or_skip() else { return };
    let effect = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fx/mind_motes.fx.json")).expect("the example effect");
    let mut ents = fx_scene(&effect, 14.0);
    ents[0] = r#"{"uid":1,"components":{"Camera":{"fov_y":50,"clear":[0,0,0,1]},"Transform":{"pos":[0,3,32]}}}"#.to_string();
    let text = format!(r#"{{"lunaris":1,"entities":[{}]}}"#, ents.join(","));
    let mut sim = Sim::new(1);
    sim.load_scene(&lunaris_json::parse(&text).expect("scene JSON")).expect("scene loads");
    let mut tex = TextureCache::new(None);
    let opts = RenderOptions { width: 640, height: 360, ..RenderOptions::default() };
    let first = gpu.render_to_image(&mut sim.world, &mut tex, &opts, &Quality::high()).expect("render");
    let busy = |f: &Frame| f.rgba.chunks(4).filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 60).count();
    assert!(busy(&first) > 1500, "the motes are in the picture: {}", busy(&first));
    // the processor holds none of the motes: only the glow's few particles live there
    assert!(tex.fx.stats.particles < 20, "{} particles on the processor", tex.fx.stats.particles);
    // a second of pictures with the clock running: the processor's share is the schedule of the births
    let t0 = std::time::Instant::now();
    let mut last = first.clone();
    for k in 0..60 {
        sim.apply(&lunaris_sim::Command::Set { uid: 4, component: "ParticleSystem".into(), value: lunaris_json::parse(&format!(r#"{{"time":{}}}"#, 14.0 + (k + 1) as f32 / 60.0)).unwrap() }).unwrap();
        last = gpu.render_to_image(&mut sim.world, &mut tex, &opts, &Quality::high()).expect("render");
    }
    eprintln!("60 pictures of the mind on the card: {:.2} ms each (a whole picture, read back to the processor)", t0.elapsed().as_secs_f64() * 1000.0 / 60.0);
    assert_ne!(last.rgba, first.rgba, "it moved");
    assert!(busy(&last) > 1500);
}
