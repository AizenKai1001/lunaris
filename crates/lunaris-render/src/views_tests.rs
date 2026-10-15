//! Cameras that draw into pictures: a monitor in the world, a minimap in the interface.
use super::*;
use lunaris_json::{parse, Json};
use lunaris_sim::{Command, Sim};

fn spawn(sim: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    sim.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn px(f: &Frame, x: u32, y: u32) -> [u8; 3] {
    let o = ((y * f.width + x) * 4) as usize;
    [f.rgba[o], f.rgba[o + 1], f.rgba[o + 2]]
}

/// The main camera (1) looks at a flat screen (2) that shows the view of a second camera (4); that camera, far off to the side, looks at a red ball (5).
fn world() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,4]},"Camera":{"clear":[0,0,0,1]}}"#);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0],"scale":[4,4,1]},"MeshRenderer":{"mesh":"quad","texture":"view:3","emissive":1,"color":[1,1,1,1]}}"#);
    spawn(&mut s, 3, r#"{"CameraView":{"camera":4,"width":32,"height":32}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[100,0,5]},"Camera":{"active":false,"clear":[0,0,1,1]}}"#);
    spawn(&mut s, 5, r#"{"Transform":{"pos":[100,0,0]},"MeshRenderer":{"mesh":"sphere","color":[1,0,0,1],"emissive":1,"shininess":8}}"#);
    s
}

fn picture(s: &mut Sim, textures: &mut TextureCache) -> Frame {
    render(&mut s.world, textures, &RenderOptions { width: 96, height: 64, ..RenderOptions::default() }).unwrap()
}

#[test]
fn a_monitor_shows_what_another_camera_sees_and_follows_it() {
    let mut s = world();
    let mut tex = TextureCache::new(None);
    let f = picture(&mut s, &mut tex);
    assert!(tex.warnings.is_empty(), "{:?}", tex.warnings);
    // in the middle of the monitor the red ball in front of a blue background; the main camera's own background is black
    let c = px(&f, 48, 32);
    assert!(c[0] > 200 && c[1] < 110 && c[2] < 110, "the ball: {c:?}");
    let corner = px(&f, 48 - 24, 32 - 24);
    assert!(corner[2] > 200 && corner[0] < 110, "the monitor's blue: {corner:?}");
    assert_eq!(px(&f, 2, 2), [0, 0, 0], "outside the monitor");
    // move the ball out of the other camera's view: the monitor shows only the background
    s.apply(&Command::Set { uid: 5, component: "Transform".into(), value: parse(r#"{"pos":[100,40,0]}"#).unwrap() }).unwrap();
    let g = picture(&mut s, &mut tex);
    let c2 = px(&g, 48, 32);
    assert!(c2[2] > 200 && c2[0] < 110, "{c2:?}");
    // the picture is the same every time for the same world
    let a = picture(&mut s, &mut TextureCache::new(None)).hash();
    assert_eq!(a, picture(&mut s, &mut TextureCache::new(None)).hash());
    // `every`: a view drawn one picture in three keeps the old picture in between
    s.apply(&Command::Set { uid: 3, component: "CameraView".into(), value: parse(r#"{"every":3}"#).unwrap() }).unwrap();
    s.apply(&Command::Set { uid: 5, component: "Transform".into(), value: parse(r#"{"pos":[100,0,0]}"#).unwrap() }).unwrap();
    let mut slow = TextureCache::new(None);
    let first = picture(&mut s, &mut slow);
    s.apply(&Command::Set { uid: 5, component: "Transform".into(), value: parse(r#"{"pos":[100,40,0]}"#).unwrap() }).unwrap();
    let second = picture(&mut s, &mut slow);
    assert_eq!(px(&first, 48, 32), px(&second, 48, 32), "not redrawn yet");
    picture(&mut s, &mut slow);
    let fourth = picture(&mut s, &mut slow);
    assert_ne!(px(&first, 48, 32), px(&fourth, 48, 32), "redrawn on the fourth");
    // switched off: it keeps its last picture; a view without a camera is a warning
    s.apply(&Command::Set { uid: 3, component: "CameraView".into(), value: parse(r#"{"camera":99,"every":1}"#).unwrap() }).unwrap();
    let mut t = TextureCache::new(None);
    let w = picture(&mut s, &mut t).warnings;
    assert!(w.iter().any(|m| m.contains("no entity 99")), "{w:?}");
}

#[test]
fn a_ui_with_a_camera_view_shows_its_own_view_without_the_interface_or_loops() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,4]},"Camera":{"clear":[0,0,0,1]}}"#);
    // a minimap: a top-down orthographic camera over the arena, shown in the corner by a Ui that has the CameraView itself
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,50,0],"rot":[-0.7071068,0,0,0.7071068]},"Camera":{"active":false,"ortho":true,"ortho_height":20,"clear":[0,0.6,0,1]}}"#);
    spawn(&mut s, 3, r#"{"Ui":{"pos":[0.05,0.05],"size":[0.3,0.45],"color":[0,0,0,0]},"CameraView":{"camera":2,"width":48,"height":48}}"#);
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,0,0],"scale":[4,1,4]},"MeshRenderer":{"mesh":"cube","color":[1,1,0,1],"emissive":1}}"#);
    // a text on the screen must not be in the minimap
    spawn(&mut s, 5, r#"{"Text":{"value":"HUD","pos":[0.05,0.05],"size":0.3,"color":[1,0,1,1],"shadow":false}}"#);
    let mut tex = TextureCache::new(None);
    let f = picture(&mut s, &mut tex);
    assert!(tex.warnings.is_empty(), "{:?}", tex.warnings);
    // the corner shows the green arena floor of the map with the yellow cube in the middle of it: at (0.05..0.35, 0.05..0.5) of 96 x 64
    let inside = px(&f, 8, 8);
    assert!(inside[1] > 100 && inside[0] < 60, "the map's green: {inside:?}");
    let middle = px(&f, (96.0 * 0.2) as u32, (64.0 * 0.275) as u32);
    assert!(middle[0] > 200 && middle[1] > 200 && middle[2] < 60, "the yellow cube seen from above: {middle:?}");
    // the picture the view drew has no interface in it: no magenta letters, although the main picture has them over the corner
    let view = tex.get("view:3");
    assert!(view.texels.iter().all(|t| !(t[0] > 0.8 && t[1] < 0.2 && t[2] > 0.8)), "no HUD letters inside the map");
    assert!(f.rgba.chunks(4).any(|p| p[0] > 200 && p[1] < 80 && p[2] > 200), "while the main picture does show them");
    // a view whose camera sees the entity that shows it does not loop: it just shows the picture of one picture ago
    spawn(&mut s, 6, r#"{"Transform":{"pos":[0,0,0]},"MeshRenderer":{"mesh":"quad","texture":"view:7","emissive":1}}"#);
    spawn(&mut s, 7, r#"{"CameraView":{"camera":1,"width":32,"height":32}}"#);
    let again = picture(&mut s, &mut tex);
    assert_eq!(again.width, 96);
}

