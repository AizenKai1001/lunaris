//! `DrawLayer`: the drawing order of sprites, tile maps, decals and particle clouds apart from their position (docs/GAMEPLAY.md, "Draw order").
//! The scene is gathered once for both renderers, so what is checked here is the order of `Scene::layers` and the picture of the reference renderer; the
//! GPU renderer against the same scenes is `crates/lunaris-gpu/tests/compare.rs`.
use super::*;
use crate::render::gather_scene;
use lunaris_json::{parse, Json};
use lunaris_sim::components::DrawLayer;
use lunaris_sim::{Command, Sim};

fn spawn(sim: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    sim.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn picture(sim: &mut Sim) -> Frame {
    let mut tex = TextureCache::new(None);
    render(&mut sim.world, &mut tex, &RenderOptions { width: 100, height: 100, ..RenderOptions::default() }).unwrap()
}

fn px(f: &Frame, x: u32, y: u32) -> [u8; 3] {
    let o = ((y * f.width + x) * 4) as usize;
    [f.rgba[o], f.rgba[o + 1], f.rgba[o + 2]]
}

/// A flat camera 4 metres high (25 pixels a metre) over a dark blue background.
fn flat_camera(sim: &mut Sim) {
    spawn(sim, 1, r#"{"Transform":{"pos":[0,0,5]},"Camera":{"ortho":true,"ortho_height":4,"clear":[0,0,0.2,1]}}"#);
}

/// Two 2 x 2 sprites exactly on top of each other: uid 2 is red, uid 3 is green. Which one is on top at the middle?
fn on_top(first: (Option<f32>, f32), second: (Option<f32>, f32)) -> u64 {
    let mut s = Sim::new(1);
    flat_camera(&mut s);
    for (uid, color, (layer, z)) in [(2u64, "[1,0,0,1]", first), (3, "[0,1,0,1]", second)] {
        let draw_layer = layer.map_or(String::new(), |l| format!(r#","DrawLayer":{{"layer":{l}}}"#));
        spawn(&mut s, uid, &format!(r#"{{"Transform":{{"pos":[0,0,{z}]}},"Sprite":{{"size":[2,2],"color":{color}}}{draw_layer}}}"#));
    }
    picture(&mut s).pick(50, 50).expect("a sprite is there")
}

#[test]
fn without_a_layer_the_depth_and_then_the_uid_decide_as_ever() {
    assert_eq!(on_top((None, 0.0), (None, 0.0)), 3, "same depth: the higher uid is on top");
    assert_eq!(on_top((None, 1.0), (None, 0.0)), 2, "the nearer one is on top");
    assert_eq!(on_top((None, -1.0), (None, 0.0)), 3);
}

#[test]
fn a_higher_layer_is_on_top_whatever_the_z_and_the_uid() {
    // the same z, different layers: the layer decides (and the uid, which would have chosen 3, does not)
    assert_eq!(on_top((Some(1.0), 0.0), (Some(0.0), 0.0)), 2);
    assert_eq!(on_top((Some(0.0), 0.0), (Some(1.0), 0.0)), 3);
    assert_eq!(on_top((Some(5.0), 0.0), (Some(-5.0), 0.0)), 2);
    // a layer beats a nearer z: the sprite far behind in z with the bigger layer is on top
    assert_eq!(on_top((Some(1.0), -3.0), (Some(0.0), 3.0)), 2);
    assert_eq!(on_top((Some(0.0), 3.0), (Some(1.0), -3.0)), 3);
    // fractions order too (a top-down game sets the layer to minus the y)
    assert_eq!(on_top((Some(0.25), 0.0), (Some(0.125), 0.0)), 2);
    assert_eq!(on_top((Some(-0.25), 0.0), (Some(-0.5), 0.0)), 2);
}

#[test]
fn an_entity_without_the_component_is_layer_zero() {
    // above and below the unlayered ones
    assert_eq!(on_top((Some(1.0), 0.0), (None, 0.0)), 2);
    assert_eq!(on_top((Some(-1.0), 0.0), (None, 0.0)), 3);
    assert_eq!(on_top((None, 0.0), (Some(-1.0), 0.0)), 2);
    // layer 0 mixes with them by depth, like nothing had been said
    assert_eq!(on_top((Some(0.0), 1.0), (None, 0.0)), 2);
    assert_eq!(on_top((Some(0.0), 0.0), (None, 0.0)), 3);
}

#[test]
fn inside_one_layer_the_depth_and_then_the_uid_decide() {
    assert_eq!(on_top((Some(2.0), 1.0), (Some(2.0), 0.0)), 2);
    assert_eq!(on_top((Some(2.0), 0.0), (Some(2.0), 1.0)), 3);
    assert_eq!(on_top((Some(2.0), 0.0), (Some(2.0), 0.0)), 3);
}

#[test]
fn a_scene_that_never_uses_the_component_is_drawn_exactly_as_one_with_layer_zero_everywhere() {
    let build = |layered: bool| {
        let mut s = Sim::new(1);
        flat_camera(&mut s);
        let extra = if layered { r#","DrawLayer":{"layer":0}"# } else { "" };
        spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Tilemap":{{"size":[2,2],"tile":2,"data":"0 0 0 0","color":[0.5,0.2,0.1,1]}}{extra}}}"#));
        spawn(&mut s, 3, &format!(r#"{{"Transform":{{"pos":[0.4,0,1]}},"Sprite":{{"size":[2,2],"color":[0,1,0,0.7]}}{extra}}}"#));
        spawn(&mut s, 4, &format!(r#"{{"Transform":{{"pos":[-0.4,0.3,0.5]}},"Sprite":{{"size":[1,1],"color":[1,0,0,1]}}{extra}}}"#));
        spawn(&mut s, 5, &format!(r#"{{"Transform":{{"pos":[0,0,0.2]}},"Particles":{{"count":1,"lifetime":4,"looping":false,"cone":0,"speed_range":[0,0],"gravity":[0,0,0],"size":[2,2],"size_jitter":0,"fade_in":0,"color":[1,1,0,0.5],"color_end":[1,1,0,0.5],"texture":"circle","time":1}}}}"#));
        picture(&mut s).hash()
    };
    // (the cloud is the one thing that moves from "after everything" to "among the layers" when it has a layer, so it is left without one here)
    assert_eq!(build(false), build(true));
}

#[test]
fn a_tile_map_takes_its_place_by_layer_too() {
    let mut s = Sim::new(1);
    flat_camera(&mut s);
    // the map is nearer in z, and the sprite stands in front of it by layer
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,1]},"Tilemap":{"size":[1,1],"tile":2,"data":"0","color":[1,0,0,1]},"DrawLayer":{"layer":0}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,0,0]},"Sprite":{"size":[1,1],"color":[0,1,0,1]},"DrawLayer":{"layer":1}}"#);
    let f = picture(&mut s);
    assert_eq!(f.pick(50, 50), Some(3), "the sprite is in front of the map by layer, though the map is nearer");
    assert_eq!(f.pick(30, 50), Some(2), "and the map shows around it");
    // a ground layer under everything: the map goes below an unlayered sprite that is farther away
    s.apply(&Command::Set { uid: 2, component: "DrawLayer".into(), value: parse(r#"{"layer":-1}"#).unwrap() }).unwrap();
    s.apply(&Command::Remove { uid: 3, component: "DrawLayer".into() }).unwrap();
    s.apply(&Command::Set { uid: 3, component: "Transform".into(), value: parse(r#"{"pos":[0,0,-4]}"#).unwrap() }).unwrap();
    assert_eq!(picture(&mut s).pick(50, 50), Some(3));
    // a decal is a sprite and takes its layer too
    spawn(&mut s, 4, r#"{"Transform":{"pos":[0,0,0]},"Decal":{"texture":"circle","size":[1,1],"color":[0,0,1,1],"normal":[0,0,1]},"DrawLayer":{"layer":10}}"#);
    let c = px(&picture(&mut s), 50, 50);
    assert!(c[2] > 200 && c[1] < 60, "the decal with the highest layer is on top: {c:?}");
}

/// A back sprite (blue, 4 x 4 metres), a red disc of particles 2 metres across and a front sprite (green, 1 x 1) at the middle, in layers 0 and 2 and the cloud's own.
fn with_a_cloud(cloud_layer: Option<f32>) -> Sim {
    let mut s = Sim::new(1);
    flat_camera(&mut s);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"Sprite":{"size":[4,4],"color":[0,0,1,1]},"DrawLayer":{"layer":0}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,0,0]},"Sprite":{"size":[1,1],"color":[0,1,0,1]},"DrawLayer":{"layer":2}}"#);
    let layer = cloud_layer.map_or(String::new(), |l| format!(r#","DrawLayer":{{"layer":{l}}}"#));
    spawn(
        &mut s,
        4,
        &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Particles":{{"count":1,"lifetime":4,"looping":false,"cone":0,"speed_range":[0,0],"gravity":[0,0,0],"size":[2,2],"size_jitter":0,"fade_in":0,"color":[1,0,0,1],"color_end":[1,0,0,1],"texture":"circle","additive":false,"time":1}}{layer}}}"#),
    );
    s
}

#[test]
fn a_particle_cloud_with_a_layer_stands_among_the_sprites() {
    let (middle, beside_the_front, corner) = ((50, 50), (32, 50), (5, 5));
    // between the back and the front sprite: the front sprite hides it, the back one does not
    let f = picture(&mut with_a_cloud(Some(1.0)));
    let (m, b, c) = (px(&f, middle.0, middle.1), px(&f, beside_the_front.0, beside_the_front.1), px(&f, corner.0, corner.1));
    assert!(m[1] > 200 && m[0] < 40, "the front sprite is on top of the cloud at the middle: {m:?}");
    assert!(b[0] > 200 && b[2] < 40, "the cloud is on top of the back sprite: {b:?}");
    assert!(c[2] > 200 && c[0] < 40, "and the back sprite is all there is in the corner: {c:?}");
    // behind the back sprite: not seen at all
    let f = picture(&mut with_a_cloud(Some(-1.0)));
    let b = px(&f, beside_the_front.0, beside_the_front.1);
    assert!(b[2] > 200 && b[0] < 40, "a cloud under the back sprite is hidden by it: {b:?}");
    // above the front sprite: over everything
    let f = picture(&mut with_a_cloud(Some(3.0)));
    let m = px(&f, middle.0, middle.1);
    assert!(m[0] > 200 && m[1] < 40, "a cloud above the front sprite is on top at the middle: {m:?}");
    // without the component it is drawn after everything, as ever
    let f = picture(&mut with_a_cloud(None));
    let m = px(&f, middle.0, middle.1);
    assert!(m[0] > 200 && m[1] < 40, "a cloud without a layer stays on top of the sprites: {m:?}");
}

#[test]
fn the_gathered_scene_lists_the_layers_in_order_and_says_where_each_cloud_goes() {
    let mut s = with_a_cloud(Some(1.0));
    let mut tex = TextureCache::new(None);
    let scene = gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 100, height: 100, ..RenderOptions::default() }).unwrap();
    assert_eq!(scene.layers.iter().map(|l| l.uid).collect::<Vec<_>>(), vec![2, 3]);
    assert_eq!(scene.particles.len(), 1);
    assert_eq!(scene.particles[0].slot, Some(1), "after the back sprite and before the front one");
    for (layer, slot) in [(-1.0, 0), (0.0, 1), (1.0, 1), (2.0, 2), (9.0, 2)] {
        s.apply(&Command::Set { uid: 4, component: "DrawLayer".into(), value: parse(&format!(r#"{{"layer":{layer}}}"#)).unwrap() }).unwrap();
        let scene = gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 100, height: 100, ..RenderOptions::default() }).unwrap();
        assert_eq!(scene.particles[0].slot, Some(slot), "layer {layer}");
    }
    // no layer: no slot
    s.apply(&Command::Remove { uid: 4, component: "DrawLayer".into() }).unwrap();
    let scene = gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 100, height: 100, ..RenderOptions::default() }).unwrap();
    assert_eq!(scene.particles[0].slot, None);
}

#[test]
fn several_clouds_among_the_layers_keep_their_order_and_the_others_come_first_in_the_list() {
    let mut s = with_a_cloud(Some(1.0));
    let cloud = |layer: &str| {
        let extra = if layer.is_empty() { String::new() } else { format!(r#","DrawLayer":{{"layer":{layer}}}"#) };
        format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Particles":{{"count":1,"lifetime":4,"looping":false,"cone":0,"speed_range":[0,0],"gravity":[0,0,0],"size":[1,1],"additive":false,"time":1}}{extra}}}"#)
    };
    spawn(&mut s, 5, &cloud("1.5"));
    spawn(&mut s, 6, &cloud("0.5"));
    spawn(&mut s, 7, &cloud(""));
    let mut tex = TextureCache::new(None);
    let scene = gather_scene(&mut s.world, &mut tex, &RenderOptions { width: 100, height: 100, ..RenderOptions::default() }).unwrap();
    let order: Vec<(u64, Option<usize>)> = scene.particles.iter().map(|p| (p.uid, p.slot)).collect();
    assert_eq!(order, vec![(7, None), (6, Some(1)), (4, Some(1)), (5, Some(1))], "the cloud without a layer first (drawn last), then by layer");
}

#[test]
fn a_layer_that_is_not_a_number_counts_as_zero() {
    let mut s = Sim::new(1);
    flat_camera(&mut s);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,0,0]},"Sprite":{"size":[2,2],"color":[1,0,0,1]},"DrawLayer":{"layer":0}}"#);
    spawn(&mut s, 3, r#"{"Transform":{"pos":[0,0,0]},"Sprite":{"size":[2,2],"color":[0,1,0,1]},"DrawLayer":{"layer":0}}"#);
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let e = s.world.entity(2).unwrap();
        s.world.get_mut::<DrawLayer>(e).unwrap().layer = bad;
        let f = picture(&mut s);
        assert_eq!(f.pick(50, 50), Some(3), "{bad}: as layer 0 the higher uid is on top, as with no component");
    }
}
