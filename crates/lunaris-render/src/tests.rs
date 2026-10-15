use super::*;
use lunaris_json::{parse, Json};
use lunaris_sim::{Command, Sim};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn spawn(sim: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = j(comps) else { panic!() };
    sim.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn cube(sim: &mut Sim, uid: u64, pos: &str, color: &str) {
    spawn(sim, uid, &format!(r#"{{"Transform":{{"pos":{pos}}},"MeshRenderer":{{"mesh":"cube","color":{color}}}}}"#));
}

fn camera_at(sim: &mut Sim, uid: u64, pos: &str) {
    spawn(sim, uid, &format!(r#"{{"Transform":{{"pos":{pos}}},"Camera":{{}}}}"#));
}

fn render_world(sim: &mut Sim, w: u32, h: u32) -> Frame {
    let mut tex = TextureCache::new(None);
    render(&mut sim.world, &mut tex, &RenderOptions { width: w, height: h, ..RenderOptions::default() }).unwrap()
}

fn px(f: &Frame, x: u32, y: u32) -> [u8; 3] {
    let o = ((y * f.width + x) * 4) as usize;
    [f.rgba[o], f.rgba[o + 1], f.rgba[o + 2]]
}

#[test]
fn a_cube_in_front_of_the_camera_is_drawn_in_the_middle() {
    let mut s = Sim::new(1);
    camera_at(&mut s, 1, "[0,0,3]");
    cube(&mut s, 2, "[0,0,0]", "[1,0.1,0.1,1]");
    let f = render_world(&mut s, 200, 150);
    assert_eq!(f.pick(100, 75), Some(2));
    assert_eq!(f.pick(2, 2), None);
    assert_eq!(f.pick(197, 147), None);
    let c = px(&f, 100, 75);
    assert!(c[0] > c[1] && c[0] > c[2], "{c:?}");
    let cov = f.coverage();
    assert!((0.05..0.5).contains(&cov), "coverage {cov}");
    let v = f.visible(10);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].uid, 2);
    // the projected centre of the cube is the image centre
    let p = f.world_to_pixel([0.0, 0.0, 0.0]).unwrap();
    assert!((p[0] - 100.0).abs() < 0.01 && (p[1] - 75.0).abs() < 0.01, "{p:?}");
    assert!(f.warnings.is_empty());
}

#[test]
fn the_front_surface_is_drawn_not_the_back_one() {
    // A sphere of radius 0.5 at the origin seen from z = 4: the centre pixel must show the near surface (distance 3.5),
    // not the far one (4.5). This catches inverted face culling, which a plain "is something drawn" check does not.
    let mut s = Sim::new(1);
    camera_at(&mut s, 1, "[0,0,4]");
    spawn(&mut s, 2, r#"{"Transform":{},"MeshRenderer":{"mesh":"sphere"}}"#);
    let f = render_world(&mut s, 101, 101);
    let proj = crate::math::M4::perspective(60.0, 1.0, 0.1, 1000.0);
    let ndc = |d: f32| {
        let c = proj.mul_v4([0.0, 0.0, -d, 1.0]);
        c[2] / c[3]
    };
    let got = f.depth[50 * 101 + 50];
    assert!((got - ndc(3.5)).abs() < 1e-4, "depth {got} vs near surface {} and far surface {}", ndc(3.5), ndc(4.5));
    // and a floor seen from above is visible (its front faces point up)
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,3,0],"rot":[-0.7071068,0,0,0.7071068]},"Camera":{}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"scale":[4,1,4]},"MeshRenderer":{"mesh":"plane"}}"#);
    assert_eq!(render_world(&mut s, 40, 40).pick(20, 20), Some(2));
    // seen from below it is culled
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,-3,0],"rot":[0.7071068,0,0,0.7071068]},"Camera":{}}"#);
    s.apply(&Command::Set { uid: 1, component: "Camera".into(), value: j(r#"{"active":false}"#) }).unwrap();
    assert_eq!(render_world(&mut s, 40, 40).pick(20, 20), None);
}

#[test]
fn rendering_is_deterministic_and_independent_of_storage_order() {
    let build = |reverse: bool| {
        let mut s = Sim::new(1);
        let mut items: Vec<(u64, &str)> = vec![(10, "[-1,0,0]"), (11, "[1,0,0]"), (12, "[0,1.2,-1]"), (13, "[0,-1.2,0.5]")];
        if reverse {
            items.reverse();
        }
        camera_at(&mut s, 1, "[0,0,6]");
        for (uid, pos) in items {
            cube(&mut s, uid, pos, &format!("[{},0.6,{},1]", 0.2 + (uid % 4) as f32 * 0.2, 1.0 - (uid % 3) as f32 * 0.3));
        }
        spawn(&mut s, 20, r#"{"Transform":{"pos":[0,0,0]},"MeshRenderer":{"mesh":"sphere","color":[0.9,0.9,0.2,1]}}"#);
        render_world(&mut s, 160, 120)
    };
    let (a, b, c) = (build(false), build(false), build(true));
    assert_eq!(a.hash(), b.hash());
    assert_eq!(a.hash(), c.hash(), "image must not depend on spawn order");
    assert_eq!(a.rgba, c.rgba);
    assert_eq!(a.ids, c.ids);
}

#[test]
fn nearer_surfaces_hide_farther_ones_whatever_the_uid_order() {
    for (front_uid, back_uid) in [(2u64, 3u64), (3, 2)] {
        let mut s = Sim::new(1);
        camera_at(&mut s, 1, "[0,0,6]");
        cube(&mut s, front_uid, "[0,0,1]", "[1,0,0,1]");
        cube(&mut s, back_uid, "[0,0,-2]", "[0,0,1,1]");
        let f = render_world(&mut s, 120, 120);
        assert_eq!(f.pick(60, 60), Some(front_uid));
        assert!(f.visible(5).iter().any(|v| v.uid == back_uid) || f.pick(60, 60) == Some(front_uid));
    }
}

#[test]
fn orthographic_sprites_cover_exactly_the_expected_pixels() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,10]},"Camera":{"ortho":true,"ortho_height":4}}"#);
    spawn(&mut s, 2, r#"{"Transform":{},"Sprite":{"size":[2,2],"color":[1,1,1,1]}}"#);
    let f = render_world(&mut s, 100, 100);
    assert_eq!(f.pick(50, 50), Some(2));
    assert_eq!(f.pick(25, 25), Some(2));
    assert_eq!(f.pick(74, 74), Some(2));
    assert_eq!(f.pick(24, 50), None);
    assert_eq!(f.pick(75, 50), None);
    let v = f.visible(5);
    assert_eq!(v[0].pixels, 50 * 50, "{v:?}");
    assert_eq!(v[0].bbox, [25, 25, 74, 74]);
}

#[test]
fn neighbouring_translucent_sprites_have_no_seam() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,10]},"Camera":{"ortho":true,"ortho_height":4,"clear":[0,0,0,1]}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[-0.5,0,0]},"Sprite":{"size":[1,2],"color":[1,1,1,0.5]}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0.5,0,0]},"Sprite":{"size":[1,2],"color":[1,1,1,0.5]}}"#);
    let f = render_world(&mut s, 100, 100);
    let reference = px(&f, 40, 40);
    assert!(reference[0] > 20, "{reference:?}");
    let mut n = 0;
    for y in 25..75 {
        for x in 25..75 {
            assert_eq!(px(&f, x, y), reference, "pixel ({x},{y}) was blended twice or missed");
            n += 1;
        }
    }
    assert_eq!(n, 2500);
}

#[test]
fn sprites_blend_back_to_front_and_respect_3d_depth() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,10]},"Camera":{"ortho":true,"ortho_height":4,"clear":[0,0,0,1]}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"Sprite":{"size":[2,2],"color":[1,0,0,1]}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0.5,0,1]},"Sprite":{"size":[2,2],"color":[0,0,1,1]}}"#);
    let f = render_world(&mut s, 100, 100);
    assert_eq!(f.pick(50, 50), Some(3), "the nearer sprite is on top");
    let c = px(&f, 50, 50);
    assert!(c[2] > 200 && c[0] < 10, "{c:?}");
    // a solid cube in front of a sprite hides it
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,0,5],"scale":[1,1,1]},"MeshRenderer":{"mesh":"cube","color":[0,1,0,1]}}"#);
    let f = render_world(&mut s, 100, 100);
    assert_eq!(f.pick(50, 50), Some(4));
}

#[test]
fn geometry_crossing_the_near_plane_is_clipped_not_broken() {
    let mut s = Sim::new(1);
    camera_at(&mut s, 1, "[0,1,0]");
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0],"scale":[200,1,200]},"MeshRenderer":{"mesh":"plane","color":[0.3,0.8,0.3,1]}}"#);
    let f = render_world(&mut s, 120, 90);
    assert_eq!(f.pick(60, 89), Some(2), "the floor under the camera must be visible at the bottom of the image");
    assert_eq!(f.pick(60, 0), None, "the sky above the horizon is empty");
    assert!(f.depth.iter().all(|d| !d.is_nan()));
    assert!(f.rgba.iter().any(|&b| b != 0));
}

#[test]
fn lights_shade_surfaces_and_unknown_meshes_warn() {
    let mut s = Sim::new(1);
    camera_at(&mut s, 1, "[0,0,4]");
    spawn(&mut s, 2, r#"{"Transform":{},"MeshRenderer":{"mesh":"sphere","color":[1,1,1,1]}}"#);
    // directional light shining towards -X: lights the +X side of the sphere
    spawn(&mut s, 3, r#"{"Transform":{"rot":[0,0.7071068,0,0.7071068]},"Light":{"kind":0,"intensity":1.5}}"#);
    let f = render_world(&mut s, 120, 120);
    let lit = px(&f, 68, 60);
    let dark = px(&f, 52, 60);
    assert!(lit[0] > dark[0] + 40, "lit side {lit:?} vs dark side {dark:?}");
    spawn(&mut s, 4, r#"{"Transform":{},"MeshRenderer":{"mesh":"teapot"}}"#);
    let f = render_world(&mut s, 60, 60);
    assert!(f.warnings.iter().any(|w| w.contains("teapot")), "{:?}", f.warnings);
    let f2 = render_world(&mut s, 60, 60);
    assert_eq!(f2.warnings.len(), f.warnings.len());
}

#[test]
fn explicit_cameras_do_not_need_entities_and_supersampling_smooths_edges() {
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{},"MeshRenderer":{"mesh":"sphere","color":[1,1,1,1]}}"#);
    let mut tex = TextureCache::new(None);
    let view = View::Explicit(ExplicitCamera { eye: [4.0, 1.0, 0.0], target: [0.0, 0.0, 0.0], ..ExplicitCamera::default() });
    let a = render(&mut s.world, &mut tex, &RenderOptions { width: 100, height: 100, view: view.clone(), supersample: 1, threads: 1 }).unwrap();
    let b = render(&mut s.world, &mut tex, &RenderOptions { width: 100, height: 100, view, supersample: 2, threads: 1 }).unwrap();
    assert_eq!(a.pick(50, 50), Some(2));
    assert_eq!(b.pick(50, 50), Some(2));
    let distinct = |f: &Frame| {
        let mut v: Vec<[u8; 3]> = (0..f.width * f.height).map(|i| px(f, i % f.width, i / f.width)).collect();
        v.sort();
        v.dedup();
        v.len()
    };
    assert!(distinct(&b) > distinct(&a), "supersampling should add intermediate edge colours ({} vs {})", distinct(&b), distinct(&a));
    assert!(a.camera.contains("explicit"));
}

#[test]
fn png_output_is_valid_and_the_hash_tracks_content() {
    let mut s = Sim::new(1);
    camera_at(&mut s, 1, "[0,0,3]");
    cube(&mut s, 2, "[0,0,0]", "[1,0,0,1]");
    let a = render_world(&mut s, 64, 48);
    let bytes = a.png().unwrap();
    assert_eq!(&bytes[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let dec = png::Decoder::new(std::io::Cursor::new(&bytes));
    let mut reader = dec.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!((info.width, info.height), (64, 48));
    assert_eq!(&buf[..info.buffer_size()], &a.rgba[..]);
    s.apply(&Command::Set { uid: 2, component: "MeshRenderer".into(), value: j(r#"{"color":[0,1,0,1]}"#) }).unwrap();
    assert_ne!(a.hash(), render_world(&mut s, 64, 48).hash());
}

#[test]
fn bad_inputs_are_errors_with_hints() {
    let mut s = Sim::new(1);
    let mut tex = TextureCache::new(None);
    let e = render(&mut s.world, &mut tex, &RenderOptions { width: 0, ..RenderOptions::default() }).unwrap_err();
    assert_eq!(e.code, "bad_size");
    let e = render(&mut s.world, &mut tex, &RenderOptions { view: View::World(Some(99)), ..RenderOptions::default() }).unwrap_err();
    assert_eq!(e.code, "no_such_camera");
    // no camera at all: default view plus a warning
    let f = render(&mut s.world, &mut tex, &RenderOptions::default()).unwrap();
    assert!(f.warnings[0].contains("default view"));
}

#[test]
fn showcase_frames_regression_guard() {
    // Pins the exact pixels of the two example scenes so any accidental change to the renderer is noticed. The values were
    // recorded from this implementation after looking at the images (see examples/): they are regression guards, not an
    // external reference.
    let load = |text: &str| {
        let mut s = Sim::new(1);
        lunaris_ecs::load_scene_json(&mut s.world, &j(text)).unwrap();
        s
    };
    let mut tex = TextureCache::new(None);

    let mut s3d = load(include_str!("../../../examples/showcase.lunaris.json"));
    let view = View::Explicit(ExplicitCamera { eye: [3.5, 2.6, 6.0], target: [0.0, 0.2, 0.0], ..ExplicitCamera::default() });
    let f = render(&mut s3d.world, &mut tex, &RenderOptions { width: 800, height: 450, view, supersample: 2, threads: 1 }).unwrap();
    assert_eq!(f.hash(), 0x35c2_7fc6_b504_c02c, "3D showcase changed: {:016x}", f.hash());
    let names: Vec<u64> = f.visible(10).iter().map(|v| v.uid).collect();
    assert!(names.contains(&2) && names.contains(&3) && names.contains(&4), "cube, sphere and cylinder must all be visible: {names:?}");

    let mut s2d = load(include_str!("../../../examples/sprites.lunaris.json"));
    let f = render(&mut s2d.world, &mut tex, &RenderOptions { width: 800, height: 450, ..RenderOptions::default() }).unwrap();
    assert_eq!(f.hash(), 0xd165_3759_07db_0092, "2D showcase changed: {:016x}", f.hash());
    assert_eq!(f.coverage(), 1.0);
}

#[test]
fn hud_text_fills_in_resources_claims_its_pixels_and_follows_the_state() {
    let mut s = Sim::new(1);
    camera_at(&mut s, 1, "[0,0,3]");
    spawn(&mut s, 2, r#"{"Text":{"value":"Score {score}","pos":[0.05,0.1],"size":0.2,"color":[1,1,0,1],"shadow":false}}"#);
    let before = render_world(&mut s, 200, 100);
    s.apply(&Command::SetResource { name: "score".into(), value: Some(7.0) }).unwrap();
    let after = render_world(&mut s, 200, 100);
    assert_ne!(before.hash(), after.hash(), "the picture changes when the resource changes");
    // the text is yellow and picks as its entity; the background does not
    let lit: Vec<(u32, u32)> = (0..100).flat_map(|y| (0..200).map(move |x| (x, y))).filter(|&(x, y)| after.pick(x, y) == Some(2)).collect();
    assert!(lit.len() > 100, "{} text pixels", lit.len());
    let (x, y) = lit[lit.len() / 2];
    let c = px(&after, x, y);
    assert!(c[0] > 200 && c[1] > 200 && c[2] < 60, "{c:?}");
    // all text pixels lie in the band below the anchor (10% of the height down, 20% tall) and right of the anchor
    assert!(lit.iter().all(|&(x, y)| x >= 10 && (10..=30).contains(&y)), "{:?}", lit.iter().map(|p| p.1).max());
    // an unset resource reads 0 and the same state gives the same picture
    let again = render_world(&mut s, 200, 100);
    assert_eq!(after.hash(), again.hash());
    assert!(after.visible(10).iter().any(|v| v.uid == 2));
}

#[test]
fn text_alignment_and_visibility() {
    let mut s = Sim::new(1);
    camera_at(&mut s, 1, "[0,0,3]");
    spawn(&mut s, 2, r#"{"Text":{"value":"HI","pos":[0.5,0.5],"size":0.2,"align":1,"shadow":false}}"#);
    let f = render_world(&mut s, 200, 100);
    let xs: Vec<u32> = (0..100).flat_map(|y| (0..200).map(move |x| (x, y))).filter(|&(x, y)| f.pick(x, y) == Some(2)).map(|p| p.0).collect();
    let (lo, hi) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
    assert!(lo < 100 && hi > 100 && (100 - lo as i32 - (hi as i32 - 100)).abs() <= 4, "centred around x = 100: {lo}..{hi}");
    s.apply(&Command::Set { uid: 2, component: "Text".into(), value: j(r#"{"visible":false}"#) }).unwrap();
    assert_eq!(render_world(&mut s, 200, 100).pick(lo + 1, 50), None);
}

/// Writes every printable glyph to target/renders/font_sheet.png so the font can be checked by eye.
#[test]
fn the_font_sheet_renders() {
    let mut s = Sim::new(1);
    camera_at(&mut s, 1, "[0,0,3]");
    let rows = ["ABCDEFGHIJKLMNOPQRSTUVWXYZ", "abcdefghijklmnopqrstuvwxyz", "0123456789 !\"#$%&'()*+,-./", ":;<=>?@[\\]^_`{|}~"];
    for (i, r) in rows.iter().enumerate() {
        let v = r.replace('\\', "\\\\").replace('"', "\\\"");
        spawn(&mut s, 10 + i as u64, &format!(r#"{{"Text":{{"value":"{v}","pos":[0.02,{}],"size":0.1,"shadow":false}}}}"#, 0.02 + 0.24 * i as f32));
    }
    let f = render_world(&mut s, 420, 160);
    assert!(f.coverage() > 0.05);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("renders");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("font_sheet.png"), f.png().unwrap()).unwrap();
}

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn imported_models_are_drawn_textured_picked_and_hit_by_rays() {
    let mut s = Sim::new(1);
    camera_at(&mut s, 1, "[0,1,4]");
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"MeshRenderer":{"mesh":"examples/models/crate.glb"}}"#);
    let mut tex = TextureCache::new(Some(repo_root()));
    let f = render(&mut s.world, &mut tex, &RenderOptions { width: 160, height: 120, ..RenderOptions::default() }).unwrap();
    assert!(f.warnings.is_empty(), "{:?}", f.warnings);
    assert_eq!(f.pick(80, 70), Some(2), "the crate is in the middle of the view");
    let visible = f.visible(5);
    assert_eq!(visible.len(), 1);
    assert!(visible[0].pixels > 800, "{} pixels", visible[0].pixels);
    // the crate's texture has two tones, so its front face is not one flat colour; the pyramid on top is plain green
    let front: std::collections::BTreeSet<[u8; 3]> = (60..90).flat_map(|x| (60..90).map(move |y| (x, y))).map(|(x, y)| px(&f, x, y)).collect();
    assert!(front.len() >= 2, "a textured face shows several colours");
    // a ray from the camera hits the model's front face (z = 0.5) at distance 3.5, and the model is cached for the next frame
    let h = raycast_with(&mut s.world, &mut tex, [0.0, 0.0, 4.0], [0.0, 0.0, -1.0], 50.0, RayTargets::Visuals).unwrap().unwrap();
    assert_eq!(h.uid, 2);
    assert!((h.distance - 3.5).abs() < 1e-3 && (h.normal[2] - 1.0).abs() < 1e-3, "{h:?}");
    // it also agrees with the image: the same ray misses beside the crate
    assert!(raycast_with(&mut s.world, &mut tex, [3.0, 0.0, 4.0], [0.0, 0.0, -1.0], 50.0, RayTargets::Visuals).unwrap().is_none());
    // without a workspace the model cannot load: a warning explains, nothing is drawn, nothing panics
    let mut none = TextureCache::new(None);
    let f = render(&mut s.world, &mut none, &RenderOptions { width: 40, height: 30, ..RenderOptions::default() }).unwrap();
    assert!(f.warnings.iter().any(|w| w.contains("crate.glb")), "{:?}", f.warnings);
    assert_eq!(f.pick(20, 15), None);
}

#[test]
fn model_paths_cannot_leave_the_workspace() {
    let mut s = Sim::new(1);
    camera_at(&mut s, 1, "[0,1,4]");
    spawn(&mut s, 2, r#"{"Transform":{},"MeshRenderer":{"mesh":"../../etc/passwd.glb"}}"#);
    let mut tex = TextureCache::new(Some(repo_root()));
    let f = render(&mut s.world, &mut tex, &RenderOptions { width: 40, height: 30, ..RenderOptions::default() }).unwrap();
    assert!(f.warnings.iter().any(|w| w.contains("cannot use `..`")), "{:?}", f.warnings);
}

#[test]
fn the_picture_never_depends_on_the_number_of_render_threads() {
    // a busy scene: lit meshes crossing band edges, a huge floor, sprites with blending, a model, text, supersampling
    let mut s = Sim::new(3);
    camera_at(&mut s, 1, "[0,6,12]");
    s.apply(&Command::Set { uid: 1, component: "Transform".into(), value: j(r#"{"rot":[-0.25,0,0,0.968]}"#) }).unwrap();
    spawn(&mut s, 2, r#"{"Transform":{"scale":[60,1,60]},"MeshRenderer":{"mesh":"plane","color":[0.3,0.5,0.35,1]}}"#);
    for i in 0..60u64 {
        let (x, z) = ((i % 10) as f32 * 2.2 - 10.0, (i / 10) as f32 * 2.0 - 4.0);
        let mesh = ["cube", "sphere", "cylinder"][(i % 3) as usize];
        spawn(&mut s, 10 + i, &format!(r#"{{"Transform":{{"pos":[{x},{},{z}],"rot":[0,{},0,0.9]}},"MeshRenderer":{{"mesh":"{mesh}","color":[0.9,{},0.3,1]}}}}"#, 0.5 + (i % 4) as f32 * 0.3, (i as f32) * 0.01, 0.2 + (i % 5) as f32 * 0.15));
    }
    spawn(&mut s, 100, r#"{"Transform":{"pos":[0,3,2]},"MeshRenderer":{"mesh":"examples/models/crate.glb"}}"#);
    spawn(&mut s, 101, r#"{"Transform":{"pos":[1,2,6],"scale":[3,3,1]},"Sprite":{"texture":"circle","color":[1,1,0,0.6]}}"#);
    spawn(&mut s, 102, r#"{"Text":{"value":"threads {n}","pos":[0.3,0.4],"size":0.1}}"#);
    s.apply(&Command::SetResource { name: "n".into(), value: Some(4.0) }).unwrap();
    let mut tex = TextureCache::new(Some(repo_root()));
    for ss in [1u32, 2] {
        let reference = render(&mut s.world, &mut tex, &RenderOptions { width: 333, height: 211, supersample: ss, threads: 1, ..RenderOptions::default() }).unwrap();
        assert!(reference.coverage() > 0.3 && reference.visible(100).len() > 10, "the busy scene must be busy: coverage {}, visible {}", reference.coverage(), reference.visible(100).len());
        for threads in [2usize, 3, 8, 16] {
            let f = render(&mut s.world, &mut tex, &RenderOptions { width: 333, height: 211, supersample: ss, threads, ..RenderOptions::default() }).unwrap();
            assert_eq!(f.hash(), reference.hash(), "supersample {ss}, {threads} threads");
            assert_eq!(f.ids, reference.ids, "entity ids, {threads} threads");
            assert_eq!(f.depth, reference.depth, "depth, {threads} threads");
        }
    }
}

fn ortho_camera(s: &mut Sim, uid: u64, height: f32) {
    spawn(s, uid, &format!(r#"{{"Transform":{{"pos":[0,0,5]}},"Camera":{{"ortho":true,"ortho_height":{height},"clear":[0,0,0.2,1]}}}}"#));
}

#[test]
fn sprite_sheets_show_one_frame_at_a_time() {
    // the built-in `gradient` texture runs dark to bright from left to right: a sheet of 4 columns gives 4 frames of rising brightness
    let mut s = Sim::new(1);
    ortho_camera(&mut s, 1, 2.0);
    spawn(&mut s, 2, r#"{"Transform":{},"Sprite":{"size":[1.8,1.8],"texture":"gradient","sheet":[4,1],"frame":0}}"#);
    let bright = |s: &mut Sim| {
        let f = render_world(s, 100, 100);
        let c = px(&f, 50, 50);
        c[0] as u32 + c[1] as u32 + c[2] as u32
    };
    let mut seen = Vec::new();
    for frame in 0..4u32 {
        s.apply(&Command::Set { uid: 2, component: "Sprite".into(), value: j(&format!(r#"{{"frame":{frame}}}"#)) }).unwrap();
        seen.push(bright(&mut s));
    }
    assert!(seen.windows(2).all(|w| w[0] < w[1]), "each later frame of the gradient is brighter: {seen:?}");
    // frames wrap around, and a 2 x 2 sheet counts row by row from the top left
    s.apply(&Command::Set { uid: 2, component: "Sprite".into(), value: j(r#"{"frame":4}"#) }).unwrap();
    assert_eq!(bright(&mut s), seen[0]);
    s.apply(&Command::Set { uid: 2, component: "Sprite".into(), value: j(r#"{"sheet":[2,2],"frame":1}"#) }).unwrap();
    let top_right = bright(&mut s);
    s.apply(&Command::Set { uid: 2, component: "Sprite".into(), value: j(r#"{"frame":2}"#) }).unwrap();
    let bottom_left = bright(&mut s);
    assert!(top_right > bottom_left, "frame 1 is the right half of the top row, frame 2 the left half of the bottom row");
}

#[test]
fn a_tilemap_draws_its_cells_and_leaves_gaps_empty() {
    let mut s = Sim::new(1);
    ortho_camera(&mut s, 1, 2.0);
    // 3 x 2 tiles of one unit, centred on the origin; tiles come from a 4 x 1 gradient sheet, -1 is empty
    spawn(&mut s, 2, r#"{"Transform":{},"Tilemap":{"texture":"gradient","sheet":[4,1],"size":[3,2],"tile":1,"data":"0 1 -1, 2 -1 3"}}"#);
    let f = render_world(&mut s, 300, 200);
    assert!(f.warnings.is_empty(), "{:?}", f.warnings);
    // tile centres: columns at x = -1, 0, +1 (pixels 50, 150, 250), rows at y = +0.5 and -0.5 (pixels 50 and 150)
    let at = |c: u32, r: u32| (f.pick(50 + 100 * c, 50 + 100 * r), px(&f, 50 + 100 * c, 50 + 100 * r));
    let (a, b, c) = (at(0, 0), at(1, 0), at(2, 0));
    let (d, e, g) = (at(0, 1), at(1, 1), at(2, 1));
    assert_eq!((a.0, b.0, d.0, g.0), (Some(2), Some(2), Some(2), Some(2)), "filled cells belong to the entity");
    assert_eq!((c.0, e.0), (None, None), "empty cells show the background");
    assert_eq!(c.1, [0, 0, 51], "the clear colour (0, 0, 0.2) shows through the gap");
    let sum = |p: [u8; 3]| p[0] as u32 + p[1] as u32 + p[2] as u32;
    assert!(sum(a.1) < sum(b.1) && sum(b.1) < sum(d.1) && sum(d.1) < sum(g.1), "tiles 0, 1, 2, 3 get brighter along the gradient: {a:?} {b:?} {d:?} {g:?}");
    // changing the data changes the picture; a hidden map draws nothing
    let before = f.hash();
    s.apply(&Command::Set { uid: 2, component: "Tilemap".into(), value: j(r#"{"data":"3 3 3 3 3 3"}"#) }).unwrap();
    assert_ne!(render_world(&mut s, 300, 200).hash(), before);
    s.apply(&Command::Set { uid: 2, component: "Tilemap".into(), value: j(r#"{"visible":false}"#) }).unwrap();
    assert_eq!(render_world(&mut s, 300, 200).pick(150, 100), None);
}

#[test]
fn tilemaps_sort_with_sprites_by_depth() {
    let mut s = Sim::new(1);
    ortho_camera(&mut s, 1, 2.0);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"Tilemap":{"size":[1,1],"tile":2,"data":"0","color":[1,0,0,1]}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,0,1]},"Sprite":{"size":[1,1],"color":[0,1,0,1]}}"#);
    let f = render_world(&mut s, 100, 100);
    assert_eq!(f.pick(50, 50), Some(3), "the sprite is nearer the camera than the map");
    assert_eq!(f.pick(5, 5), Some(2), "and the map shows around it");
}

#[test]
fn absurd_camera_values_give_an_image_or_an_error_never_a_panic() {
    // found by the API fuzzer: a target 1e300 units away made the row-range arithmetic overflow
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"scale":[1,1,1]},"MeshRenderer":{"mesh":"cube"}}"#);
    let mut tex = TextureCache::new(None);
    for (eye, target) in [([0.0, 3_000_009.0, 8.0], [9.2e18, f32::MAX, 0.0]), ([0.0, 3.0, 8.0], [0.0, 0.0, -9.2e18]), ([1e30, 1e30, 1e30], [0.0, 0.0, 0.0]), ([f32::MAX, 0.0, 0.0], [0.0, 0.0, 0.0])] {
        let view = View::Explicit(ExplicitCamera { eye, target, ..ExplicitCamera::default() });
        for threads in [1, 4] {
            let r = render(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 48, view: view.clone(), supersample: 1, threads });
            assert!(r.is_ok() || r.is_err()); // reaching this line is the test: no panic
        }
    }
}

#[test]
fn emissive_surfaces_glow_in_the_dark_and_zero_changes_nothing() {
    let draw = |emissive: &str| {
        let mut s = Sim::new(1);
        camera_at(&mut s, 1, "[0,0,3]");
        // no lights in the scene: only the small ambient term lights a surface
        spawn(&mut s, 2, &format!(r#"{{"Transform":{{}},"MeshRenderer":{{"mesh":"sphere","color":[0.25,0.15,0.03,1]{emissive}}}}}"#));
        render_world(&mut s, 80, 60)
    };
    let (plain, explicit_zero, glowing) = (draw(""), draw(r#","emissive":0"#), draw(r#","emissive":1"#));
    assert_eq!(plain.hash(), explicit_zero.hash(), "emissive 0 is the same picture as not saying it");
    let (dark, bright) = (px(&plain, 40, 30).map(|v| v as i32), px(&glowing, 40, 30).map(|v| v as i32));
    assert!(bright[0] > dark[0] + 20 && bright[1] > dark[1] + 10 && bright[2] >= dark[2], "{dark:?} -> {bright:?}");
    // the glow keeps the colour's hue (orange: red above green above blue)
    assert!(bright[0] > bright[1] && bright[1] > bright[2], "{bright:?}");
}

#[test]
fn a_mesh_texture_repeats_uv_scale_times_and_replaces_nothing_else() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,6,0],"rot":[-0.7071068,0,0,0.7071068]},"Camera":{}}"#);
    // a floor seen from straight above, lit from above: its brightness varies only with the texture
    spawn(&mut s, 2, r#"{"Transform":{"scale":[6,1,6]},"MeshRenderer":{"mesh":"plane","color":[1,1,1,1],"texture":"checker","uv_scale":[1,1]}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"rot":[-0.7071,0,0,0.7071]},"Light":{"kind":0}}"#);
    let few = render_world(&mut s, 200, 200);
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<lunaris_sim::components::MeshRenderer>(e).unwrap().uv_scale = [8.0, 8.0];
    let many = render_world(&mut s, 200, 200);
    let changes = |f: &Frame| (1..200).filter(|&x| (px(f, x, 100)[0] as i32 - px(f, x - 1, 100)[0] as i32).abs() > 40).count();
    assert!(changes(&many) > changes(&few) * 3, "{} vs {}", changes(&many), changes(&few));
    assert!(few.warnings.is_empty() && many.warnings.is_empty(), "{:?}", many.warnings);
}

#[test]
fn every_material_texture_loads_without_warnings_and_has_a_height_map() {
    let mut c = TextureCache::new(None);
    for name in crate::procedural::MATERIALS {
        assert!(is_procedural_texture(name), "{name}");
        let t = c.get(name);
        assert_eq!(t.heights.as_ref().map(|h| h.len()), Some((t.width * t.height) as usize), "{name}");
    }
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    // the little pictures have no relief
    assert!(c.get("circle").heights.is_none());
}

#[test]
fn the_scene_knows_its_environment_and_derives_one_when_there_is_none() {
    use crate::render::gather_scene;
    let gather = |s: &mut Sim| {
        let mut tex = TextureCache::new(None);
        gather_scene(&mut s.world, &mut tex, &RenderOptions::default()).unwrap()
    };
    // a 2D scene: no effects, so that its colours stay as authored
    let mut flat = Sim::new(1);
    spawn(&mut flat, 1, r#"{"Transform":{},"Camera":{"ortho":true,"clear":[0.2,0.4,0.6,1]}}"#);
    spawn(&mut flat, 2, r#"{"Transform":{},"Sprite":{}}"#);
    let e = gather(&mut flat).env;
    assert!(!e.explicit && !e.sky && e.bloom == 0.0 && e.vignette == 0.0 && e.ssao == 0.0, "{e:?}");
    // a 3D scene gets a little of everything
    let mut solid = Sim::new(1);
    camera_at(&mut solid, 1, "[0,0,5]");
    cube(&mut solid, 2, "[0,0,0]", "[1,1,1,1]");
    let e = gather(&mut solid).env;
    assert!(!e.explicit && e.bloom > 0.0 && e.ssao > 0.0 && e.shadows, "{e:?}");
    // an Environment entity wins (the lowest uid), with values clamped into their ranges
    spawn(&mut solid, 9, r#"{"Environment":{"sky":true,"fog":0.02,"bloom":9,"exposure":-4,"tonemap":99}}"#);
    spawn(&mut solid, 10, r#"{"Environment":{"fog":0.5}}"#);
    let e = gather(&mut solid).env;
    assert!(e.explicit && e.sky && e.fog == 0.02 && e.bloom == 2.0 && e.exposure == 0.0 && e.tonemap == 3, "{e:?}");
}

#[test]
fn roughness_follows_the_shininess_unless_the_material_sets_it() {
    use crate::render::gather_scene;
    let mut s = Sim::new(1);
    camera_at(&mut s, 1, "[0,0,5]");
    spawn(&mut s, 2, r#"{"Transform":{},"MeshRenderer":{"mesh":"sphere","shininess":2}}"#);
    spawn(&mut s, 3, r#"{"Transform":{},"MeshRenderer":{"mesh":"sphere","shininess":512}}"#);
    spawn(&mut s, 4, r#"{"Transform":{},"MeshRenderer":{"mesh":"sphere","roughness":0.3,"metallic":2}}"#);
    let mut tex = TextureCache::new(None);
    let scene = gather_scene(&mut s.world, &mut tex, &RenderOptions::default()).unwrap();
    let by_uid = |uid: u64| scene.meshes.iter().find(|m| m.uid == uid).unwrap();
    assert!(by_uid(2).roughness > 0.8, "{}", by_uid(2).roughness);
    assert!(by_uid(3).roughness < 0.3, "{}", by_uid(3).roughness);
    assert_eq!(by_uid(4).roughness, 0.3);
    assert_eq!(by_uid(4).metallic, 1.0, "metallic is clamped to 1");
}

#[test]
fn a_skinned_model_imports_with_its_skeleton_and_clips() {
    let mut c = TextureCache::new(Some(repo_root()));
    let m = c.model("examples/models/swayer.glb").expect("the model loads");
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    let rig = m.rig.as_ref().expect("a rig");
    assert_eq!(rig.joints.len(), 4);
    assert_eq!(rig.clips.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["sway", "nod"]);
    assert!((rig.clips[0].duration - 2.0).abs() < 1e-5 && (rig.clips[1].duration - 1.2).abs() < 1e-5);
    // the shown mesh is the rest pose: a column from y = 0 to 3.2, straight
    let (lo, hi) = m.pos.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
    assert!(lo.abs() < 1e-4 && (hi - 3.2).abs() < 1e-4, "{lo} .. {hi}");
    let top = m.pos.iter().filter(|p| p[1] > 3.19).map(|p| p[0]).fold(0.0f32, |a, x| a + x) / 8.0;
    assert!(top.abs() < 1e-4, "straight at rest: {top}");
    // a quarter of the way through "sway" the top leans over
    let posed = rig.pose(&m, Some(0), 0.5, true);
    let lean = posed.pos.iter().filter(|p| p[1] > 2.6).map(|p| p[0]).fold(0.0f32, |a, x| a + x) / posed.pos.iter().filter(|p| p[1] > 2.6).count() as f32;
    assert!(lean.abs() > 0.15, "the top should have moved sideways: {lean}");
    // posing is exact and repeatable
    let again = rig.pose(&m, Some(0), 0.5, true);
    assert_eq!(posed.pos, again.pos);
    // looping: a whole number of clip lengths later is the same pose; holding does not wrap
    let later = rig.pose(&m, Some(0), 0.5 + 2.0 * 3.0, true);
    for (a, b) in posed.pos.iter().zip(&later.pos) {
        assert!((0..3).all(|k| (a[k] - b[k]).abs() < 1e-4));
    }
}

#[test]
fn an_animator_poses_the_model_in_the_picture_and_the_clock_is_the_state() {
    let render_in_repo = |s: &mut Sim| {
        let mut tex = TextureCache::new(Some(repo_root()));
        render(&mut s.world, &mut tex, &RenderOptions { width: 200, height: 200, ..RenderOptions::default() }).unwrap()
    };
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.6,7]},"Camera":{"fov_y":45}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"rot":[-0.7071,0,0,0.7071]},"Light":{"kind":0}}"#);
    spawn(&mut s, 3, r#"{"Transform":{},"MeshRenderer":{"mesh":"examples/models/swayer.glb","color":[1,1,1,1]},"Animator":{"clip":"sway","time":0.0,"speed":0.0}}"#);
    let still = render_in_repo(&mut s);
    // the simulation advances the clock of a playing animator, and not of one with speed 0
    assert_eq!(s.world.get::<lunaris_sim::components::Animator>(s.world.entity(3).unwrap()).unwrap().time, 0.0);
    s.step_n(30);
    assert_eq!(render_in_repo(&mut s).hash(), still.hash(), "speed 0: nothing moves");
    let e = s.world.entity(3).unwrap();
    s.world.get_mut::<lunaris_sim::components::Animator>(e).unwrap().speed = 1.0;
    s.step_n(30); // half a second at 60 ticks per second
    let t = s.world.get::<lunaris_sim::components::Animator>(e).unwrap().time;
    assert!((t - 0.5).abs() < 1e-4, "{t}");
    let moved = render_in_repo(&mut s);
    assert_ne!(moved.hash(), still.hash(), "the pose changed");
    assert_eq!(render_in_repo(&mut s).hash(), moved.hash(), "the same time draws the same picture");
    assert_eq!(moved.pick(100, 150), Some(3));
    assert!(moved.warnings.is_empty(), "{:?}", moved.warnings);
    // an unknown clip is reported with the names that exist
    s.world.get_mut::<lunaris_sim::components::Animator>(e).unwrap().clip = lunaris_ecs::Sym(s.world.strings_mut().intern("dance"));
    let w = render_in_repo(&mut s).warnings;
    assert!(w.iter().any(|m| m.contains("no animation called `dance`") && m.contains("sway, nod")), "{w:?}");
}

#[test]
fn ui_rectangles_borders_bars_labels_and_button_states_are_drawn_in_layer_order() {
    let mut sim = Sim::new(1);
    spawn(&mut sim, 1, r#"{"Camera":{"clear":[0,0,0,1]},"Transform":{"pos":[0,0,5]}}"#);
    // a panel with a border over the left half, a bar filled 25% in the right half, a button, and a higher layer overlapping the panel
    spawn(&mut sim, 2, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.4,0.3],"color":[1,0,0,1],"border":0.05,"border_color":[0,1,0,1]}}"#);
    spawn(&mut sim, 3, r#"{"Ui":{"pos":[0.6,0.1],"size":[0.3,0.2],"color":[0,0,1,1],"back_color":[1,1,1,1],"fill":0.25}}"#);
    spawn(&mut sim, 4, r#"{"Ui":{"pos":[0.6,0.6],"size":[0.3,0.2],"color":[0.4,0.4,0.4,1],"button":true}}"#);
    spawn(&mut sim, 5, r#"{"Ui":{"pos":[0.4,0.3],"size":[0.2,0.2],"color":[1,1,0,1],"layer":1}}"#);
    let f = render_world(&mut sim, 200, 100);
    assert_eq!(px(&f, 5, 5), [0, 0, 0], "outside every rectangle: the clear colour");
    assert_eq!(px(&f, 21, 15), [0, 255, 0], "inside the panel's border (5 px)");
    assert_eq!(px(&f, 35, 25), [255, 0, 0], "inside the border: the panel colour");
    assert_eq!(px(&f, 125, 15), [0, 0, 255], "the filled quarter of the bar (x from 120 to 135)");
    assert_eq!(px(&f, 170, 15), [255, 255, 255], "the rest of the bar");
    assert_eq!(px(&f, 100, 40), [255, 255, 0], "the higher layer is on top of the panel it overlaps");
    assert_eq!(px(&f, 140, 70), [102, 102, 102], "a button at rest");
    // under the pointer a button is lighter, pressed darker
    let e = sim.world.entity(4).unwrap();
    sim.world.get_mut::<lunaris_sim::components::Ui>(e).unwrap().hover = true;
    assert_eq!(px(&render_world(&mut sim, 200, 100), 140, 70), [128, 128, 128]);
    sim.world.get_mut::<lunaris_sim::components::Ui>(e).unwrap().pressed = true;
    assert_eq!(px(&render_world(&mut sim, 200, 100), 140, 70), [71, 71, 71]);
    // a hidden one is not drawn, and a label shows its text
    sim.world.get_mut::<lunaris_sim::components::Ui>(e).unwrap().visible = false;
    assert_eq!(px(&render_world(&mut sim, 200, 100), 140, 70), [0, 0, 0]);
    spawn(&mut sim, 6, r#"{"Ui":{"pos":[0.1,0.7],"size":[0.4,0.2],"color":[0,0,0,1],"text":"HI","text_size":0.1,"text_color":[1,1,1,1]}}"#);
    let f = render_world(&mut sim, 200, 100);
    let lit = (70..90).flat_map(|y| (20..100).map(move |x| (x, y))).filter(|&(x, y)| px(&f, x, y) == [255, 255, 255]).count();
    assert!(lit > 20, "the label's letters are drawn in its rectangle ({lit} white pixels)");
}

#[test]
fn animation_layers_change_the_pose_blend_by_weight_and_report_unknown_names() {
    let render_in_repo = |s: &mut Sim| {
        let mut tex = TextureCache::new(Some(repo_root()));
        render(&mut s.world, &mut tex, &RenderOptions { width: 200, height: 200, ..RenderOptions::default() }).unwrap()
    };
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1.6,7]},"Camera":{"fov_y":45}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"rot":[-0.7071,0,0,0.7071]},"Light":{"kind":0}}"#);
    spawn(&mut s, 3, r#"{"Transform":{},"MeshRenderer":{"mesh":"examples/models/swayer.glb","color":[1,1,1,1]},"Animator":{"clip":"sway","time":0.5,"speed":0.0},"AnimLayers":{"clip_b":"nod","weight_b":0.0,"sync_b":false,"speed_b":0.0,"time_b":0.4}}"#);
    let base = render_in_repo(&mut s);
    let e = s.world.entity(3).unwrap();
    s.world.get_mut::<lunaris_sim::components::AnimLayers>(e).unwrap().weight_b = 1.0;
    let nodding = render_in_repo(&mut s);
    assert_ne!(nodding.hash(), base.hash(), "the extra clip changes the picture");
    s.world.get_mut::<lunaris_sim::components::AnimLayers>(e).unwrap().weight_b = 0.5;
    let half = render_in_repo(&mut s);
    assert!(half.hash() != base.hash() && half.hash() != nodding.hash(), "half way is a third picture");
    assert!(nodding.warnings.is_empty(), "{:?}", nodding.warnings);
    // an unknown clip or joint is reported
    let bad_clip = lunaris_ecs::Sym(s.world.strings_mut().intern("dance"));
    s.world.get_mut::<lunaris_sim::components::AnimLayers>(e).unwrap().clip_b = bad_clip;
    let w = render_in_repo(&mut s).warnings;
    assert!(w.iter().any(|m| m.contains("no animation called `dance`") && m.contains("AnimLayers")), "{w:?}");
    let nod = lunaris_ecs::Sym(s.world.strings_mut().intern("nod"));
    let nowhere = lunaris_ecs::Sym(s.world.strings_mut().intern("nowhere"));
    {
        let l = s.world.get_mut::<lunaris_sim::components::AnimLayers>(e).unwrap();
        l.clip_b = nod;
        l.root_b = nowhere;
    }
    let w = render_in_repo(&mut s).warnings;
    assert!(w.iter().any(|m| m.contains("no joint called `nowhere`")), "{w:?}");
}

#[test]
fn parallax_shifts_a_layer_against_the_camera_by_its_factor_and_nothing_else() {
    let centre_x = |s: &mut Sim| {
        let f = render_world(s, 100, 100);
        let xs: Vec<u32> = (0..100).flat_map(|y| (0..100).map(move |x| (x, y))).filter(|&(x, y)| px(&f, x, y)[0] > 200).map(|p| p.0).collect();
        assert!(!xs.is_empty());
        (xs.iter().min().unwrap() + xs.iter().max().unwrap()) as f32 / 2.0
    };
    let mut s = Sim::new(1);
    ortho_camera(&mut s, 1, 2.0);
    spawn(&mut s, 2, r#"{"Transform":{},"Sprite":{"size":[0.4,0.4],"texture":"circle"}}"#);
    let home = centre_x(&mut s);
    assert!((home - 49.5).abs() < 1.5, "{home}");
    // the camera moves right by 0.5 (a quarter of the picture): an ordinary sprite slides left by 25 pixels
    s.apply(&Command::Set { uid: 1, component: "Transform".into(), value: j(r#"{"pos":[0.5,0,5]}"#) }).unwrap();
    assert!((centre_x(&mut s) - (home - 25.0)).abs() < 1.5);
    // with parallax factor 1 it moves with the camera (stays in the middle), 0.5 it drifts at half speed, 0 it is an ordinary sprite
    for (factor, shift) in [(1.0, 0.0), (0.5, -12.5), (0.0, -25.0)] {
        s.apply(&Command::Add { uid: 2, component: "Parallax".into(), value: j(&format!(r#"{{"factor":[{factor},{factor}]}}"#)) }).unwrap();
        let x = centre_x(&mut s);
        assert!((x - (home + shift)).abs() < 1.5, "factor {factor}: {x} vs {}", home + shift);
    }
    // the entity itself did not move
    assert_eq!(s.world.get::<lunaris_sim::components::Transform>(s.world.entity(2).unwrap()).unwrap().pos, [0.0, 0.0, 0.0]);
}

