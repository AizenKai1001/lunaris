//! Painted terrain as an agent meets it: `terrain_layer`, `scatter` on a layer, and what `check` says about a splat map and a layer text that do not work.
use crate::tests::engine;
use lunaris_json::{parse, Json};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn problems(e: &mut crate::Engine) -> Vec<String> {
    let c = e.call("check", &Json::Null).unwrap();
    c.get("problems").unwrap().as_array().unwrap().iter().map(|p| p.get("message").unwrap().as_str().unwrap().to_string()).collect()
}

fn write_png(path: &std::path::Path, w: u32, h: u32, color: png::ColorType, depth: png::BitDepth, bytes: &[u8]) {
    let mut data = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut data, w, h);
        enc.set_color(color);
        enc.set_depth(depth);
        enc.write_header().unwrap().write_image_data(bytes).unwrap();
    }
    std::fs::write(path, data).unwrap();
}

/// A 33 x 33 flat ground of 64 m, and a 4 x 4 splat map: the west half is the red channel (layer 1), the east half the green one (layer 2).
fn painted(e: &mut crate::Engine, tag: &str, layers: &str) {
    let dir = e.root.clone();
    write_png(&dir.join(format!("{tag}_flat.png")), 33, 33, png::ColorType::Grayscale, png::BitDepth::Sixteen, &vec![0u8; 33 * 33 * 2]);
    let mut splat = Vec::new();
    for _row in 0..4 {
        for col in 0..4 {
            splat.extend_from_slice(&if col < 2 { [255u8, 0, 0, 0] } else { [0, 255, 0, 0] });
        }
    }
    write_png(&dir.join(format!("{tag}_splat.png")), 4, 4, png::ColorType::Rgba, png::BitDepth::Eight, &splat);
    let layers = Json::str(layers).to_string_lossy();
    e.call("spawn", &j(&format!(r#"{{"uid":1,"components":{{"Transform":{{"pos":[0,0,0]}},"Terrain":{{"heightmap":"{tag}_flat.png","size":[64,64],"height":5,"color":[1,1,1,1]}},"TerrainLayers":{{"splat":"{tag}_splat.png","layers":{layers}}}}}}}"#))).unwrap();
}

fn set_field(e: &mut crate::Engine, uid: u64, component: &str, field: &str, value: &str) {
    e.call("set", &j(&format!(r#"{{"uid":{uid},"component":"{component}","value":{{"{field}":{value}}}}}"#))).unwrap();
}

fn weights(e: &mut crate::Engine, x: f64, z: f64) -> Json {
    e.call("terrain_layer", &j(&format!(r#"{{"x":{x},"z":{z}}}"#))).unwrap()
}

#[test]
fn terrain_layer_says_which_material_is_under_a_point_and_how_much_of_each() {
    let mut e = engine();
    painted(&mut e, "tl1", "grass grass uv_scale=32\nrock stone");
    // west: grass, east: rock, the middle is half and half (the splat map blends between its texels, which lie on the corners of the ground's cells)
    let west = weights(&mut e, -25.0, 3.0);
    assert_eq!(west.get("dominant").unwrap().as_str(), Some("grass"), "{west:?}");
    assert_eq!(west.get("dominant_index").unwrap().as_u64(), Some(0));
    assert_eq!(west.get("terrain").unwrap().as_u64(), Some(1));
    let layers = west.get("layers").unwrap().as_array().unwrap();
    assert_eq!(layers.len(), 2);
    assert_eq!((layers[0].get("name").unwrap().as_str(), layers[0].get("texture").unwrap().as_str()), (Some("grass"), Some("grass")));
    assert_eq!((layers[1].get("name").unwrap().as_str(), layers[1].get("texture").unwrap().as_str()), (Some("rock"), Some("stone")));
    assert_eq!(layers[0].get("weight").unwrap().as_f64(), Some(1.0));
    let east = weights(&mut e, 25.0, -9.0);
    assert_eq!(east.get("dominant").unwrap().as_str(), Some("rock"), "{east:?}");
    assert_eq!(east.get("weights").unwrap().as_array().unwrap().len(), 4);
    let middle = weights(&mut e, 0.0, 0.0);
    let w: Vec<f64> = middle.get("weights").unwrap().as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((w[0] - 0.5).abs() < 0.01 && (w[1] - 0.5).abs() < 0.01 && w[2] == 0.0 && w[3] == 0.0, "{w:?}");
    // a third of the way from the west texel pair to the east pair: more grass than rock
    let near = weights(&mut e, -6.0, 0.0);
    let (g, r) = (near.get("layers").unwrap().index(0).unwrap().get("weight").unwrap().as_f64().unwrap(), near.get("layers").unwrap().index(1).unwrap().get("weight").unwrap().as_f64().unwrap());
    assert!(g > r && (g + r - 1.0).abs() < 0.001, "{g} {r}");
    // outside the ground, an unknown or unpainted terrain, bad numbers
    assert_eq!(e.call("terrain_layer", &j(r#"{"x":50,"z":0}"#)).unwrap_err().code, "no_terrain");
    assert!(e.call("terrain_layer", &j(r#"{"x":"a","z":0}"#)).is_err());
    assert!(e.call("terrain_layer", &j(r#"{"x":0}"#)).is_err());
    assert!(e.call("terrain_layer", &j(r#"{"x":0,"z":0,"terrain":99}"#)).is_err());
    e.call("remove_component", &j(r#"{"uid":1,"component":"TerrainLayers"}"#)).unwrap();
    assert_eq!(e.call("terrain_layer", &j(r#"{"x":0,"z":0}"#)).unwrap_err().code, "no_layers");
    assert_eq!(e.call("terrain_layer", &j(r#"{"x":0,"z":0,"terrain":1}"#)).unwrap_err().code, "no_layers");
}

#[test]
fn a_painted_terrain_that_cannot_be_asked_about_says_why() {
    let mut e = engine();
    painted(&mut e, "tl2", "grass\nrock stone");
    // a mistake in the text
    set_field(&mut e, 1, "TerrainLayers", "layers", r#""grass uv_scale=0""#);
    let err = e.call("terrain_layer", &j(r#"{"x":0,"z":0}"#)).unwrap_err();
    assert_eq!(err.code, "bad_layers", "{err:?}");
    assert!(err.message.contains("uv_scale"), "{}", err.message);
    // a splat map that is not there
    set_field(&mut e, 1, "TerrainLayers", "layers", r#""grass""#);
    set_field(&mut e, 1, "TerrainLayers", "splat", r#""nowhere.png""#);
    let err = e.call("terrain_layer", &j(r#"{"x":0,"z":0}"#)).unwrap_err();
    assert_eq!(err.code, "splat_unreadable", "{err:?}");
    assert!(err.message.contains("nowhere.png"), "{}", err.message);
    // no terrain at all
    let mut empty = engine();
    assert_eq!(empty.call("terrain_layer", &j(r#"{"x":0,"z":0}"#)).unwrap_err().code, "no_terrain");
}

fn spots(e: &mut crate::Engine, r: &Json) -> Vec<[f64; 3]> {
    let (first, last) = (r.get("first_uid").unwrap().as_u64().unwrap(), r.get("last_uid").unwrap().as_u64().unwrap());
    (first..=last)
        .map(|u| {
            let t = e.call("get", &j(&format!(r#"{{"uid":{u},"components":["Transform"]}}"#))).unwrap();
            let p = t.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap();
            [p.index(0).unwrap().as_f64().unwrap(), p.index(1).unwrap().as_f64().unwrap(), p.index(2).unwrap().as_f64().unwrap()]
        })
        .collect()
}

#[test]
fn scatter_can_keep_to_one_layer() {
    let mut e = engine();
    painted(&mut e, "tl3", "grass\nrock stone");
    // rocks on the rock, which is the east half, by name; trees on the grass (the west half) by number, and only where it is nearly all grass
    let rocks = e.call("scatter", &j(r#"{"count":60,"seed":3,"layer":"rock","components":{"Name":{"value":"Rock"}}}"#)).unwrap();
    assert_eq!(rocks.get("placed").unwrap().as_u64(), Some(60), "{rocks:?}");
    for p in spots(&mut e, &rocks) {
        assert!(p[0] >= -0.01, "a rock on the grass at {p:?}");
    }
    let trees = e.call("scatter", &j(r#"{"count":60,"seed":4,"layer":0,"layer_min":0.9,"components":{"Name":{"value":"Tree"}}}"#)).unwrap();
    for p in spots(&mut e, &trees) {
        assert!(p[0] < -5.0, "a tree where the grass is not nearly all there is: {p:?}");
    }
    // `layer_min` 0 puts them anywhere; the same seed without a layer gives the same positions as with `layer_min` 0
    let anywhere = e.call("scatter", &j(r#"{"count":60,"seed":4,"layer":0,"layer_min":0,"components":{"Name":{"value":"X"}}}"#)).unwrap();
    let plain = e.call("scatter", &j(r#"{"count":60,"seed":4,"components":{"Name":{"value":"X"}}}"#)).unwrap();
    let (a, b) = (spots(&mut e, &anywhere), spots(&mut e, &plain));
    assert_eq!(a, b);
    assert!(a.iter().any(|p| p[0] > 10.0) && a.iter().any(|p| p[0] < -10.0));
    // mistakes: an unknown layer, a number too high, a weight out of range, a layer that nothing is painted with
    assert_eq!(e.call("scatter", &j(r#"{"count":5,"components":{},"layer":"snow"}"#)).unwrap_err().code, "no_such_layer");
    assert_eq!(e.call("scatter", &j(r#"{"count":5,"components":{},"layer":7}"#)).unwrap_err().code, "no_such_layer");
    assert_eq!(e.call("scatter", &j(r#"{"count":5,"components":{},"layer":"rock","layer_min":2}"#)).unwrap_err().code, "out_of_range");
    assert!(e.call("scatter", &j(r#"{"count":5,"components":{},"layer":true}"#)).is_err());
    set_field(&mut e, 1, "TerrainLayers", "layers", r#""grass\nrock stone\nsand""#);
    let none = e.call("scatter", &j(r#"{"count":5,"components":{},"layer":"sand"}"#)).unwrap_err();
    assert_eq!(none.code, "no_room");
    assert!(none.message.contains("sand"), "{}", none.message);
    // a terrain without layers has none to keep to
    e.call("remove_component", &j(r#"{"uid":1,"component":"TerrainLayers"}"#)).unwrap();
    assert_eq!(e.call("scatter", &j(r#"{"count":5,"components":{},"layer":"rock"}"#)).unwrap_err().code, "no_layers");
}

#[test]
fn check_finds_what_leaves_a_painted_ground_in_one_colour() {
    // a good one is quiet
    let mut good = engine();
    painted(&mut good, "tl4", "grass\nrock stone");
    let p = problems(&mut good);
    assert!(p.iter().all(|m| !m.contains("TerrainLayers") && !m.contains("layer") && !m.contains("splat")), "{p:?}");
    // the default green tints every layer, and the terrain's own texture is not used
    let mut tinted = engine();
    painted(&mut tinted, "tl5", "grass\nrock stone");
    set_field(&mut tinted, 1, "Terrain", "color", r#"[0.34,0.48,0.27,1]"#);
    set_field(&mut tinted, 1, "Terrain", "texture", r#""grass""#);
    let p = problems(&mut tinted);
    assert!(p.iter().any(|m| m.contains("default green")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("Terrain.texture") && m.contains("not used")), "{p:?}");
    // the text, the textures, the splat map
    for (layers, splat, word) in [
        ("grass uv_scale=0", "tl6_splat.png", "TerrainLayers.layers: line 1"),
        ("grass\nrock nothing.png", "tl6_splat.png", "the texture `nothing.png`"),
        ("grass\nrock stone", "gone.png", "the splat map `gone.png` is not a file"),
        ("grass\nrock stone", "", "TerrainLayers.splat is empty"),
        ("grass\nrock stone\nsand\nsnow", "tl6_splat.png", "layer 3 (`sand`) never shows"),
    ] {
        let mut e = engine();
        painted(&mut e, "tl6", "grass\nrock stone");
        set_field(&mut e, 1, "TerrainLayers", "layers", &Json::str(layers).to_string_lossy());
        set_field(&mut e, 1, "TerrainLayers", "splat", &Json::str(splat).to_string_lossy());
        let p = problems(&mut e);
        assert!(p.iter().any(|m| m.contains(word)), "{word}: {p:?}");
    }
    // a splat map with fewer channels than layers, and one that is not a picture
    let mut rgb = engine();
    painted(&mut rgb, "tl7", "grass\nrock stone\nsand\nsnow");
    write_png(&rgb.root.join("tl7_rgb.png"), 2, 2, png::ColorType::Rgb, png::BitDepth::Eight, &[255, 0, 0, 0, 255, 0, 0, 0, 255, 9, 9, 9]);
    set_field(&mut rgb, 1, "TerrainLayers", "splat", r#""tl7_rgb.png""#);
    let p = problems(&mut rgb);
    assert!(p.iter().any(|m| m.contains("layer 4 (`snow`)") && m.contains("alpha channel") && m.contains("3 channels")), "{p:?}");
    std::fs::write(rgb.root.join("tl7_bad.png"), b"not a picture").unwrap();
    set_field(&mut rgb, 1, "TerrainLayers", "splat", r#""tl7_bad.png""#);
    let p = problems(&mut rgb);
    assert!(p.iter().any(|m| m.contains("tl7_bad.png") && m.contains("could not be read")), "{p:?}");
    // layers without a terrain to paint
    let mut alone = engine();
    alone.call("spawn", &j(r#"{"uid":1,"components":{"TerrainLayers":{"splat":"x.png","layers":"grass"}}}"#)).unwrap();
    let p = problems(&mut alone);
    assert!(p.iter().any(|m| m.contains("no Terrain")), "{p:?}");
}

/// The example scene (`tools/build_terrain_layers.py`): what the tools say at a few places of the painted, watered valley is what the builder made there.
#[test]
fn the_example_valley_is_painted_and_watered_where_its_builder_put_things() {
    let mut e = crate::Engine::new(1, std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(".."));
    e.call("load_scene", &j(r#"{"path":"examples/terrain_layers.lunaris.json"}"#)).unwrap();
    let c = e.call("check", &Json::Null).unwrap();
    assert_eq!(c.get("errors").unwrap().as_u64(), Some(0), "{c:?}");
    // (x, z, the layer that is most of the ground there): the grass where the hero starts, sand on the beach and the lake's bed, grass on the river's banks, rock and snow up in the mountains
    for (x, z, want) in [(2.0, 28.0, "grass"), (-8.0, 28.0, "sand"), (-30.0, 28.0, "sand"), (24.0, -14.0, "grass"), (-60.0, -90.0, "rock"), (-75.0, -104.0, "snow"), (100.0, 100.0, "grass")] {
        let r = e.call("terrain_layer", &j(&format!(r#"{{"x":{x},"z":{z}}}"#))).unwrap();
        assert_eq!(r.get("dominant").unwrap().as_str(), Some(want), "at ({x}, {z}): {r:?}");
    }
    // a footstep sound by the ground: the hero's start and the mountain are different materials
    let a = e.call("terrain_layer", &j(r#"{"x":2,"z":28}"#)).unwrap();
    let b = e.call("terrain_layer", &j(r#"{"x":-75,"z":-104}"#)).unwrap();
    assert_ne!(a.get("dominant"), b.get("dominant"));
    // the lake is 4 m up and 3.6 m deep in the middle; the river steps down towards it; the start is dry
    let depth = |e: &mut crate::Engine, x: f64, z: f64| e.call("water_depth", &j(&format!(r#"{{"x":{x},"z":{z}}}"#))).unwrap();
    let lake = depth(&mut e, -30.0, 28.0);
    assert_eq!(lake.get("wet").unwrap().as_bool(), Some(true));
    assert!((lake.get("surface").unwrap().as_f64().unwrap() - 4.0).abs() < 0.01 && (lake.get("depth").unwrap().as_f64().unwrap() - 3.6).abs() < 0.1, "{lake:?}");
    let levels: Vec<f64> = [(30.0, -48.0), (24.0, -14.0), (6.0, 0.0)].iter().map(|(x, z)| depth(&mut e, *x, *z)).map(|r| r.get("surface").unwrap().as_f64().unwrap()).collect();
    assert!(levels[0] > levels[1] + 3.0 && levels[1] > levels[2] + 2.0 && levels[2] > 4.5, "the river falls down the valley to the lake: {levels:?}");
    assert_eq!(depth(&mut e, 2.0, 28.0).get("wet").unwrap().as_bool(), Some(false));
}
