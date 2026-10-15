//! The interface as the reference renderer draws it: rounded corners, gradients, pictures, TrueType letters, pins to the world, drop shadows.
use super::*;
use lunaris_json::{parse, Json};
use lunaris_sim::{Command, Sim};

fn spawn(sim: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    sim.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn world() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,3]},"Camera":{}}"#);
    s
}

fn testdata() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

fn draw(s: &mut Sim, w: u32, h: u32) -> (Frame, Vec<String>) {
    let mut tex = TextureCache::new(Some(testdata()));
    let f = render(&mut s.world, &mut tex, &RenderOptions { width: w, height: h, ..RenderOptions::default() }).unwrap();
    let w = f.warnings.clone();
    (f, w)
}

fn px(f: &Frame, x: u32, y: u32) -> [u8; 3] {
    let o = ((y * f.width + x) * 4) as usize;
    [f.rgba[o], f.rgba[o + 1], f.rgba[o + 2]]
}

#[test]
fn rounded_corners_cut_the_shape_and_a_gradient_runs_from_top_to_bottom() {
    let mut empty = world();
    let (bare, _) = draw(&mut empty, 100, 100);
    let mut s = world();
    // a 50 x 50 box whose corner radius is half of it: a disc, red at the top and blue at the bottom
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.25,0.25],"size":[0.5,0.5],"color":[1,0,0,1]},"UiStyle":{"radius":0.25,"color2":[0,0,1,1]}}"#);
    let (f, warnings) = draw(&mut s, 100, 100);
    assert!(warnings.is_empty(), "{warnings:?}");
    // the corners of the box are not drawn at all, the middle is
    for (x, y) in [(26, 26), (73, 26), (26, 73), (73, 73)] {
        assert_eq!(px(&f, x, y), px(&bare, x, y), "{x},{y} is outside the disc");
    }
    let (top, mid, bottom) = (px(&f, 50, 28), px(&f, 50, 50), px(&f, 50, 71));
    assert!(top[0] > 200 && top[2] < 90, "red at the top: {top:?}");
    assert!(bottom[2] > 200 && bottom[0] < 90, "blue at the bottom: {bottom:?}");
    // halfway the two mix evenly (in the renderer's gamma-2 space: 0.5 of each is a display level of 180)
    assert!((mid[0] as i32 - 180).abs() < 12 && (mid[2] as i32 - 180).abs() < 12, "{mid:?}");
    // a flat rectangle without a style is untouched by all of this: its corners are drawn
    let mut flat = world();
    spawn(&mut flat, 2, r#"{"Ui":{"pos":[0.25,0.25],"size":[0.5,0.5],"color":[1,0,0,1]}}"#);
    let (g, _) = draw(&mut flat, 100, 100);
    assert_eq!(px(&g, 26, 26)[0], 255);
}

#[test]
fn a_border_follows_the_rounded_shape_and_a_drop_shadow_lies_behind() {
    let mut s = world();
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.2,0.2],"size":[0.5,0.5],"color":[0,0.6,0,1],"border":0.04,"border_color":[1,1,1,1]},"UiStyle":{"radius":0.12,"shadow_color":[0,0,0,1],"shadow_offset":[0.06,0.06]}}"#);
    let (f, _) = draw(&mut s, 100, 100);
    // the middle is the fill, the edge in the middle of a side is the border
    assert!(px(&f, 45, 45)[1] > 130 && px(&f, 45, 45)[0] < 40, "{:?}", px(&f, 45, 45));
    assert_eq!(px(&f, 45, 21), [255, 255, 255], "the border at the top");
    // the shadow shows below and to the right of the box (shifted 6 pixels) and nowhere above it
    let mut bare = world();
    let (b, _) = draw(&mut bare, 100, 100);
    assert_eq!(px(&f, 74, 50), [0, 0, 0], "inside the shadow, outside the box");
    assert_eq!(px(&f, 10, 10), px(&b, 10, 10));
}

#[test]
fn a_picture_fills_the_box_keeps_its_proportions_or_crops_and_is_tinted() {
    // the built-in "gradient" texture is dark on the left and white on the right
    let mut s = world();
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.8,0.8],"color":[0,0,0,0]},"UiStyle":{"image":"gradient"}}"#);
    let (f, w) = draw(&mut s, 100, 100);
    assert!(w.is_empty(), "{w:?}");
    assert!(px(&f, 14, 50)[0] < px(&f, 85, 50)[0] && px(&f, 85, 50)[0] > 200, "{:?} {:?}", px(&f, 14, 50), px(&f, 85, 50));
    // tinted red: no green or blue is left
    s.apply(&Command::Set { uid: 2, component: "UiStyle".into(), value: parse(r#"{"tint":[1,0,0,1]}"#).unwrap() }).unwrap();
    let (r, _) = draw(&mut s, 100, 100);
    let c = px(&r, 85, 50);
    assert!(c[0] > 200 && c[1] < 10 && c[2] < 10, "{c:?}");
    // a 64 x 64 picture in a 100 x 40 box: kept whole it is 40 x 40 in the middle; cropped it fills the box
    let mut s = world();
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.25,0.3],"size":[0.5,0.4],"color":[0,0,0,0]},"UiStyle":{"image":"circle","fit":1}}"#);
    let (bare, _) = draw(&mut world(), 200, 100);
    let (contain, _) = draw(&mut s, 200, 100);
    assert_ne!(px(&contain, 100, 50), px(&bare, 100, 50), "the disc is in the middle");
    assert_eq!(px(&contain, 62, 50), px(&bare, 62, 50), "nothing left of the 40 x 40 square (80..120)");
    s.apply(&Command::Set { uid: 2, component: "UiStyle".into(), value: parse(r#"{"fit":2}"#).unwrap() }).unwrap();
    let (cover, _) = draw(&mut s, 200, 100);
    // the disc is now 100 pixels wide, so a point 30 pixels from the middle is still inside it
    assert_ne!(px(&cover, 125, 50), px(&bare, 125, 50));
    // a name that is no picture draws the placeholder and says so
    s.apply(&Command::Set { uid: 2, component: "UiStyle".into(), value: parse(r#"{"image":"nope.png","fit":0}"#).unwrap() }).unwrap();
    let (_, w) = draw(&mut s, 200, 100);
    assert!(w.iter().any(|m| m.contains("nope.png")), "{w:?}");
}

#[test]
fn truetype_text_is_drawn_on_whole_pixels_and_a_missing_font_falls_back_to_the_built_in_one() {
    let mut s = world();
    // the test font's pixels are 100 of 800 units: at 16 pixels to the em each is exactly 2 x 2 pixels
    spawn(&mut s, 2, r#"{"Text":{"value":"H","pos":[0.1,0.1],"size":0.16,"color":[1,1,1,1],"shadow":false},"TextFont":{"file":"pixel.ttf"}}"#);
    let (f, w) = draw(&mut s, 200, 100);
    assert!(w.is_empty(), "{w:?}");
    // an H is two bars 2 pixels wide and 7 font pixels (14 pixels) tall from y = 10, joined by a bar in the fourth row
    assert_eq!(px(&f, 20, 12), [255, 255, 255], "left bar");
    assert_eq!(px(&f, 21, 23), [255, 255, 255], "left bar, last row");
    assert_eq!(px(&f, 28, 12), [255, 255, 255], "right bar");
    let bare = px(&draw(&mut world(), 200, 100).0, 25, 12);
    assert_eq!(px(&f, 25, 12), bare, "between the bars, above the crossbar");
    assert_eq!(px(&f, 25, 17), [255, 255, 255], "the crossbar");
    assert_eq!(px(&f, 25, 24), bare, "below the baseline");
    // the same picture twice, and at another supersampling the letters keep their pixels
    assert_eq!(draw(&mut s, 200, 100).0.hash(), f.hash());
    // a label of a Ui button with the font, and centred
    spawn(&mut s, 3, r#"{"Ui":{"pos":[0.3,0.5],"size":[0.4,0.3],"color":[0,0,0.5,1],"text":"HH","text_size":0.16,"text_color":[1,1,0,1]},"TextFont":{"file":"pixel.ttf"}}"#);
    let (g, w) = draw(&mut s, 200, 100);
    assert!(w.is_empty(), "{w:?}");
    let yellow: Vec<u32> = (0..200).filter(|&x| (50..80).any(|y| px(&g, x, y) == [255, 255, 0])).collect();
    let (lo, hi) = (*yellow.first().unwrap(), *yellow.last().unwrap());
    assert!((100 - lo as i32 - (hi as i32 - 99)).abs() <= 3, "centred in the box (60..140): {lo}..{hi}");
    // a font that is not there: a warning, and the built-in letters are drawn instead (they pick as the entity)
    s.apply(&Command::Set { uid: 2, component: "TextFont".into(), value: parse(r#"{"file":"nope.ttf"}"#).unwrap() }).unwrap();
    let (h, w) = draw(&mut s, 200, 100);
    assert!(w.iter().any(|m| m.contains("nope.ttf")), "{w:?}");
    assert!((0..200).any(|x| (0..100).any(|y| h.pick(x, y) == Some(2))));
}

#[test]
fn a_ui_pinned_to_the_world_follows_the_point_and_hides_behind_the_camera() {
    let mut s = world();
    spawn(&mut s, 5, r#"{"Transform":{"pos":[0,0,0]}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"size":[0.2,0.1],"color":[1,0,0,1]},"UiAnchor":{"entity":5,"offset":[0,-0.25]}}"#);
    let (f, _) = draw(&mut s, 100, 100);
    // the point is in the middle of the picture, the box centred on it and moved a quarter of the height up
    assert!(px(&f, 50, 25)[0] > 200, "{:?}", px(&f, 50, 25));
    assert!(px(&f, 50, 60)[0] < 100);
    // moved to the right, it follows
    s.apply(&Command::Set { uid: 5, component: "Transform".into(), value: parse(r#"{"pos":[0.6,0,0]}"#).unwrap() }).unwrap();
    let (g, _) = draw(&mut s, 100, 100);
    assert!(px(&g, 50, 25)[0] < 100 && (50..100).any(|x| px(&g, x, 25)[0] > 200));
    // behind the camera (the camera looks down -z from z = 3): not drawn
    s.apply(&Command::Set { uid: 5, component: "Transform".into(), value: parse(r#"{"pos":[0,0,8]}"#).unwrap() }).unwrap();
    let (h, _) = draw(&mut s, 100, 100);
    assert!((0..100).all(|x| px(&h, x, 25)[0] < 100));
}

#[test]
fn text_breaks_at_newlines_and_wraps_at_spaces_when_asked() {
    let mut s = world();
    // the pixel font at 16 pixels: letters 12 pixels apart, lines 16 pixels tall and 4 apart (a quarter of the size)
    spawn(&mut s, 2, r#"{"Text":{"value":"AA BB\nCC","pos":[0.05,0.05],"size":0.16,"color":[1,1,1,1],"shadow":false},"TextFont":{"file":"pixel.ttf"}}"#);
    let (f, _) = draw(&mut s, 200, 100);
    let lit = |f: &Frame, rows: std::ops::Range<u32>| rows.into_iter().any(|y| (0..200).any(|x| px(f, x, y) == [255, 255, 255]));
    assert!(lit(&f, 5..19), "the first line");
    assert!(!lit(&f, 20..25), "the gap between lines");
    assert!(lit(&f, 25..39), "the second line, after the newline");
    assert!(!lit(&f, 40..100), "and no third line");
    // a line width of 0.2 of the picture (40 pixels) puts each word of "AA BB" on a line of its own
    s.apply(&Command::Set { uid: 2, component: "TextFont".into(), value: parse(r#"{"wrap":0.2}"#).unwrap() }).unwrap();
    let (g, _) = draw(&mut s, 200, 100);
    assert!(lit(&g, 25..39) && lit(&g, 45..59), "three lines now");
    assert!(!lit(&g, 60..100));
    // a box label is centred as a block of lines
    spawn(&mut s, 3, r#"{"Ui":{"pos":[0.5,0.0],"size":[0.5,1.0],"color":[0,0,0.4,1],"text":"AA\nBB","text_size":0.16,"text_color":[1,1,0,1]},"TextFont":{"file":"pixel.ttf"}}"#);
    let (h, _) = draw(&mut s, 200, 100);
    let rows: Vec<u32> = (0..100).filter(|&y| (100..200).any(|x| px(&h, x, y) == [255, 255, 0])).collect();
    let (top, bottom) = (*rows.first().unwrap(), *rows.last().unwrap());
    // two 16 pixel lines and a 4 pixel gap are 36 pixels tall: from y = 32 to 68 in the 100 pixel box, the letters inside
    assert!((30..40).contains(&top) && (60..70).contains(&bottom), "{top}..{bottom}");
}

