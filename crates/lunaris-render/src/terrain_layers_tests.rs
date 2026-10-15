//! Painted terrain in the reference renderer: the splat map weights the layers' textures over the ground, row 0 of the map is the north edge, and a terrain
//! without `TerrainLayers` (or with a mistake in it) is drawn as it always was.
use crate::{render, ExplicitCamera, Frame, RenderOptions, TextureCache, View};
use lunaris_json::Json;
use lunaris_sim::{Command, Sim};
use std::path::PathBuf;

fn write_png(dir: &std::path::Path, name: &str, w: u32, h: u32, color: png::ColorType, depth: png::BitDepth, bytes: &[u8]) {
    let mut data = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut data, w, h);
        enc.set_color(color);
        enc.set_depth(depth);
        enc.write_header().unwrap().write_image_data(bytes).unwrap();
    }
    std::fs::write(dir.join(name), data).unwrap();
}

/// A fresh workspace with a flat 17 x 17 heightmap (all black: the ground lies at y = 0), and two solid-colour textures, red and blue.
fn workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lunaris-render-layers-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_png(&dir, "flat.png", 17, 17, png::ColorType::Grayscale, png::BitDepth::Sixteen, &vec![0u8; 17 * 17 * 2]);
    write_png(&dir, "red.png", 2, 2, png::ColorType::Rgb, png::BitDepth::Eight, &[255, 0, 0].repeat(4));
    write_png(&dir, "blue.png", 2, 2, png::ColorType::Rgb, png::BitDepth::Eight, &[0, 0, 255].repeat(4));
    dir
}

fn spawn(sim: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = lunaris_json::parse(comps).unwrap() else { panic!() };
    sim.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// A 32 x 32 m ground at y = 0 (the default sun lights it: its highlight adds grey to every colour, so hues are told by how far one channel stands out), and (when `layers` is given) painted with the splat map `splat`.
fn ground(splat: Option<&str>, layers: &str) -> Sim {
    let mut s = Sim::new(1);
    let painted = splat.map(|sp| format!(r#","TerrainLayers":{}"#, Json::obj([("splat", Json::str(sp)), ("layers", Json::str(layers))]).to_string_lossy())).unwrap_or_default();
    spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,0,0]}},"Terrain":{{"heightmap":"flat.png","size":[32,32],"height":4,"color":[1,1,1,1],"texture":"checker","uv_scale":[4,4]}}{painted}}}"#));
    s
}

/// The picture of the ground from straight above, north at the top, 160 x 160 pixels: the ground (32 m) fills the middle 128 pixels or so.
fn from_above(s: &mut Sim, dir: &PathBuf) -> Frame {
    let mut tex = TextureCache::new(Some(dir.clone()));
    let view = View::Explicit(ExplicitCamera { eye: [0.0, 40.0, 0.0], target: [0.0, 0.0, 0.0], up: [0.0, 0.0, -1.0], fov_y: 45.0, ..ExplicitCamera::default() });
    render(&mut s.world, &mut tex, &RenderOptions { width: 160, height: 160, view, ..RenderOptions::default() }).unwrap()
}

fn px(f: &Frame, x: u32, y: u32) -> [i32; 3] {
    let o = ((y * f.width + x) * 4) as usize;
    [f.rgba[o] as i32, f.rgba[o + 1] as i32, f.rgba[o + 2] as i32]
}

/// Which channel stands out by at least 50 levels: 1 red, 2 green, 3 blue, 0 none (grey, or white).
fn hue(c: [i32; 3]) -> u8 {
    (0..3).find(|&k| (0..3).all(|o| o == k || c[k] - c[o] >= 50)).map_or(0, |k| k as u8 + 1)
}

fn red_ish(c: [i32; 3]) -> bool {
    hue(c) == 1
}

fn blue_ish(c: [i32; 3]) -> bool {
    hue(c) == 3
}

#[test]
fn the_splat_map_weights_the_layers_across_the_ground_and_its_first_row_is_the_north_edge() {
    let dir = workspace("blend");
    // 5 x 5 splat: red layer on the west two columns, blue on the east two, a blend in the middle column; and the whole north row (row 0) is red
    let mut bytes = Vec::new();
    for row in 0..5 {
        for col in 0..5 {
            let px: [u8; 4] = if row == 0 { [255, 0, 0, 0] } else if col < 2 { [255, 0, 0, 0] } else if col > 2 { [0, 255, 0, 0] } else { [128, 128, 0, 0] };
            bytes.extend_from_slice(&px);
        }
    }
    write_png(&dir, "splat.png", 5, 5, png::ColorType::Rgba, png::BitDepth::Eight, &bytes);
    let mut s = ground(Some("splat.png"), "red red.png\nblue blue.png");
    let f = from_above(&mut s, &dir);
    assert!(f.warnings.is_empty(), "{:?}", f.warnings);
    // the ground is the whole middle of the picture; the layers show in the right places
    assert_eq!(f.pick(80, 80), Some(2));
    assert!(red_ish(px(&f, 30, 100)) && red_ish(px(&f, 40, 130)), "west is red: {:?} {:?}", px(&f, 30, 100), px(&f, 40, 130));
    assert!(blue_ish(px(&f, 125, 100)) && blue_ish(px(&f, 150, 130)), "east is blue: {:?} {:?}", px(&f, 125, 100), px(&f, 150, 130));
    let mid = px(&f, 80, 110);
    assert!(mid[0] - mid[1] > 15 && mid[2] - mid[1] > 15, "the middle column is a blend of both: {mid:?}");
    // row 0 of the map is the north edge: the top of the picture is red all the way across, even where the south part of the east is blue
    assert!(red_ish(px(&f, 125, 5)) && red_ish(px(&f, 30, 5)), "north row: {:?} {:?}", px(&f, 125, 5), px(&f, 30, 5));
    assert!(blue_ish(px(&f, 125, 100)), "while south of it the east is blue");
    // the ground's own texture (a checker) and its alpha play no part
    assert!(f.rgba.chunks(4).filter(|p| p[0] == p[1] && p[1] == p[2] && p[0] > 60 && p[0] < 250).count() < 400, "no grey checker squares");
}

#[test]
fn without_the_component_or_with_a_mistake_in_it_the_terrain_is_drawn_with_its_own_texture() {
    let dir = workspace("plain");
    write_png(&dir, "splat.png", 2, 2, png::ColorType::Rgba, png::BitDepth::Eight, &[255, 0, 0, 0].repeat(4));
    let mut plain = ground(None, "");
    let reference = from_above(&mut plain, &dir);
    assert!(reference.warnings.is_empty());
    // the checker texture shows: grey squares of two shades
    let greys: std::collections::BTreeSet<i32> = reference.rgba.chunks(4).filter(|p| p[0] == p[1] && p[1] == p[2] && p[0] > 20).map(|p| p[0] as i32).collect();
    assert!(greys.len() > 1, "the checker squares show on the plain ground");
    // removing the component gives the same picture byte for byte
    let mut was_painted = ground(Some("splat.png"), "red red.png");
    let painted = from_above(&mut was_painted, &dir);
    assert_ne!(painted.rgba, reference.rgba, "painted ground looks different");
    let e = was_painted.world.entity(2).unwrap();
    was_painted.world.remove_typed::<lunaris_sim::components::TerrainLayers>(e).unwrap();
    assert_eq!(from_above(&mut was_painted, &dir).rgba, reference.rgba);
    // a mistake in the text, an empty or a missing splat map: a warning that says what, and the plain picture
    for (splat, layers, word) in [("splat.png", "red red.png uv_scale=0", "TerrainLayers.layers"), ("splat.png", "", "no layers"), ("", "red red.png", "splat is empty"), ("nothing.png", "red red.png", "nothing.png")] {
        let mut s = ground(Some(splat), layers);
        let f = from_above(&mut s, &dir);
        assert!(f.warnings.iter().any(|w| w.contains(word) && w.contains("own texture") || w.contains("nothing.png") && word == "nothing.png"), "{word}: {:?}", f.warnings);
        assert_eq!(f.rgba, reference.rgba, "{word}: drawn as before");
    }
}

#[test]
fn the_layers_repeat_across_the_ground_and_tint_and_the_terrain_colour_multiply_them() {
    let dir = workspace("repeat");
    // a 2 x 2 texture of four colours, so that the repeat is visible: red, green / blue, white
    write_png(&dir, "quad.png", 2, 2, png::ColorType::Rgb, png::BitDepth::Eight, &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
    write_png(&dir, "splat.png", 2, 2, png::ColorType::Rgba, png::BitDepth::Eight, &[255, 0, 0, 0].repeat(4));
    let mut once = ground(Some("splat.png"), "q quad.png uv_scale=1");
    let a = from_above(&mut once, &dir);
    let mut twice = ground(Some("splat.png"), "q quad.png uv_scale=2");
    let b = from_above(&mut twice, &dir);
    // one repeat: a whole texel of the picture covers the north-west quarter of the ground; two repeats: the same corner holds all four colours
    let colours = |f: &Frame| -> usize {
        let mut set = std::collections::BTreeSet::new();
        for y in 20..80 {
            for x in 20..80 {
                set.insert(hue(px(f, x, y)));
            }
        }
        set.len()
    };
    assert_eq!(colours(&a), 1, "one texel over the quarter");
    assert_eq!(colours(&b), 4, "four colours in the same corner with twice the repeats");
    // the tint of a layer and the colour of the terrain multiply: a tint of 0 on red, green and blue leaves black
    let mut dark = ground(Some("splat.png"), "q quad.png uv_scale=1 tint=0,0,0");
    let d = from_above(&mut dark, &dir);
    assert_eq!(hue(px(&d, 50, 50)), 0, "nothing of the texture's colour is left, only the sun's highlight: {:?}", px(&d, 50, 50));
    let mut half = ground(Some("splat.png"), "q quad.png uv_scale=1");
    half.world.get_mut::<lunaris_sim::components::Terrain>(half.world.entity(2).unwrap()).unwrap().color = [0.25, 0.25, 0.25, 1.0];
    let h = from_above(&mut half, &dir);
    assert!(hue(px(&a, 50, 50)) != 0 && hue(px(&h, 50, 50)) == 0, "a quarter of the colour is much less red: {:?} against {:?}", px(&h, 50, 50), px(&a, 50, 50));
}

#[test]
fn a_splat_map_saved_over_while_the_game_runs_is_read_again() {
    let dir = workspace("reload");
    write_png(&dir, "splat.png", 2, 2, png::ColorType::Rgba, png::BitDepth::Eight, &[255, 0, 0, 0].repeat(4));
    let mut s = ground(Some("splat.png"), "red red.png\nblue blue.png");
    let mut tex = TextureCache::new(Some(dir.clone()));
    tex.reload_interval = std::time::Duration::ZERO;
    let draw = |s: &mut Sim, tex: &mut TextureCache| {
        let view = View::Explicit(ExplicitCamera { eye: [0.0, 40.0, 0.0], target: [0.0, 0.0, 0.0], up: [0.0, 0.0, -1.0], fov_y: 45.0, ..ExplicitCamera::default() });
        render(&mut s.world, tex, &RenderOptions { width: 160, height: 160, view, ..RenderOptions::default() }).unwrap()
    };
    assert!(red_ish(px(&draw(&mut s, &mut tex), 80, 80)));
    write_png(&dir, "splat.png", 2, 2, png::ColorType::Rgba, png::BitDepth::Eight, &[0, 255, 0, 0].repeat(4));
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
    std::fs::OpenOptions::new().write(true).open(dir.join("splat.png")).unwrap().set_modified(later).unwrap();
    assert!(blue_ish(px(&draw(&mut s, &mut tex), 80, 80)), "the new map is used");
}
