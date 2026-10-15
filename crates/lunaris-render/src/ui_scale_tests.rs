//! `UiScale` and `UiEdge`: the interface keeps its shape on a picture that is not the shape it was made for (docs/UI.md, "Other screens"). The GPU renderer draws the same
//! `SceneRect`s and `SceneText`s, so these tests are about the reference picture and `crates/lunaris-gpu/tests/compare.rs` compares it with the GPU's.
use super::*;
use lunaris_json::{parse, Json};
use lunaris_sim::ui_kit::safe_area;
use lunaris_sim::{Command, Sim};

const SIXTEEN_NINE: f32 = 1.777_777_8;

fn spawn(sim: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    sim.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn testdata() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

fn draw(s: &mut Sim, w: u32, h: u32) -> Frame {
    let mut tex = TextureCache::new(Some(testdata()));
    let f = render(&mut s.world, &mut tex, &RenderOptions { width: w, height: h, ..RenderOptions::default() }).unwrap();
    assert!(f.warnings.is_empty(), "{:?}", f.warnings);
    f
}

fn px(f: &Frame, x: u32, y: u32) -> [u8; 3] {
    let o = ((y * f.width + x) * 4) as usize;
    [f.rgba[o], f.rgba[o + 1], f.rgba[o + 2]]
}

const CLEAR: [u8; 3] = [26, 26, 51];

/// A menu made for 16:9: a square button, a bar, a bitmap text, a TrueType text and a rounded gradient panel, over a dark blue background.
fn menu(scale: bool) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,3]},"Camera":{"clear":[0.1,0.1,0.2,1]}}"#);
    if scale {
        spawn(&mut s, 2, r#"{"UiScale":{"aspect":1.7777778}}"#);
    }
    spawn(&mut s, 10, r#"{"Ui":{"pos":[0.1,0.2],"size":[0.1,0.17777778],"color":[1,0,0,1]}}"#);
    spawn(&mut s, 11, r#"{"Ui":{"pos":[0.3,0.8],"size":[0.4,0.05],"color":[0,1,0,1],"back_color":[0.5,0.5,0.5,1],"fill":0.5,"border":0.004,"border_color":[1,1,1,1]}}"#);
    spawn(&mut s, 12, r#"{"Text":{"value":"HELLO","pos":[0.6,0.2],"size":0.08,"color":[1,1,0,1],"shadow":true}}"#);
    spawn(&mut s, 13, r#"{"Text":{"value":"Ab","pos":[0.6,0.4],"size":0.1,"color":[1,1,1,1]},"TextFont":{"file":"pixel.ttf"}}"#);
    spawn(&mut s, 14, r#"{"Ui":{"pos":[0.45,0.55],"size":[0.2,0.15],"color":[1,0.5,0,1],"text":"OK","text_size":0.05},"UiStyle":{"radius":0.03,"color2":[0.2,0,0.5,1],"shadow_color":[0,0,0,0.5],"shadow_offset":[0.004,0.008]}}"#);
    s
}

/// The box of the pixels of one colour: (x0, y0, x1, y1) inclusive, and how many.
fn bbox(f: &Frame, is: impl Fn([u8; 3]) -> bool) -> Option<(u32, u32, u32, u32, usize)> {
    let mut b: Option<(u32, u32, u32, u32, usize)> = None;
    for y in 0..f.height {
        for x in 0..f.width {
            if is(px(f, x, y)) {
                b = Some(match b {
                    None => (x, y, x, y, 1),
                    Some((x0, y0, x1, y1, n)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y), n + 1),
                });
            }
        }
    }
    b
}

fn red(p: [u8; 3]) -> bool {
    p[0] > 200 && p[1] < 60 && p[2] < 60
}

#[test]
fn a_picture_of_the_shape_the_interface_was_made_for_is_drawn_exactly_as_without_a_scale() {
    for (w, h) in [(640, 360), (1920, 1080), (321, 181)] {
        let (plain, scaled) = (draw(&mut menu(false), w, h), draw(&mut menu(true), w, h));
        assert_eq!(plain.hash(), scaled.hash(), "{w}x{h}");
    }
}

#[test]
fn on_another_shape_the_safe_area_is_the_made_for_picture_pixel_for_pixel_and_the_rest_is_untouched() {
    let reference = draw(&mut menu(true), 640, 360);
    assert!(red(px(&reference, 80, 100)) && px(&reference, 5, 5) == CLEAR, "the reference menu is drawn");
    for (w, h) in [(640u32, 480u32), (840, 360), (800, 800), (480, 640), (1000, 360)] {
        let f = draw(&mut menu(true), w, h);
        let (ax, ay, aw, ah) = safe_area(SIXTEEN_NINE, w, h);
        assert_eq!((aw, ah), if w * 9 > h * 16 { (h * 16 / 9, h) } else { (w, w * 9 / 16) }, "{w}x{h}");
        // the safe area is the 640 x 360 picture scaled to its size: for these sizes whole pixels (same size) or equal in shape, so compare where the sizes agree
        if (aw, ah) == (640, 360) {
            for y in 0..360 {
                for x in 0..640 {
                    assert_eq!(px(&f, ax + x, ay + y), px(&reference, x, y), "{w}x{h} at ({x}, {y}) of the safe area");
                }
            }
        } else {
            // a different size: the same menu, not the same pixels; the red button is still square
            let b = bbox(&f, red).expect("the button is drawn");
            assert_eq!((b.2 - b.0).abs_diff(b.3 - b.1) <= 1, true, "{w}x{h}: the button is {} x {}", b.2 - b.0 + 1, b.3 - b.1 + 1);
        }
        // nothing at all outside the safe area (this world has only the interface in it)
        for (x, y) in [(0, 0), (w - 1, h - 1), (0, h - 1), (w - 1, 0), (ax.saturating_sub(1), ay + ah / 2), (ax + aw + (w - ax - aw) / 2, ay)] {
            if x < ax || x >= ax + aw || y < ay || y >= ay + ah {
                assert_eq!(px(&f, x, y), CLEAR, "{w}x{h}: ({x}, {y}) is in the margin");
            }
        }
    }
}

#[test]
fn without_a_scale_the_same_menu_stretches_and_with_one_the_square_stays_square() {
    let (stretched, kept) = (draw(&mut menu(false), 640, 480), draw(&mut menu(true), 640, 480));
    let (a, b) = (bbox(&stretched, red).unwrap(), bbox(&kept, red).unwrap());
    assert_eq!((a.2 - a.0 + 1, a.3 - a.1 + 1), (64, 85), "no scale: 0.1 of the width by 0.1778 of the height");
    assert_eq!((b.2 - b.0 + 1, b.3 - b.1 + 1), (64, 64), "with a scale: a square");
    // and at 21:9 the picture is wider but the square is the same square, 100 pixels in from the left margin
    let wide = draw(&mut menu(true), 840, 360);
    let w = bbox(&wide, red).unwrap();
    assert_eq!((w.0, w.1, w.2 - w.0 + 1, w.3 - w.1 + 1), (100 + 64, 72, 64, 64), "{w:?}");
    // the letters keep their size: the bitmap text takes as many pixels at 21:9 and at 4:3 as at 16:9
    let yellow = |p: [u8; 3]| p[0] > 200 && p[1] > 200 && p[2] < 80;
    let n = |f: &Frame| bbox(f, yellow).unwrap().4;
    let (sixteen, four, twenty_one) = (draw(&mut menu(true), 640, 360), draw(&mut menu(true), 640, 480), draw(&mut menu(true), 840, 360));
    assert_eq!((n(&four), n(&twenty_one)), (n(&sixteen), n(&sixteen)));
}

fn quad(pos: &str, edge: &str) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,3]},"Camera":{"clear":[0.1,0.1,0.2,1]}}"#);
    spawn(&mut s, 2, r#"{"UiScale":{"aspect":1.7777778}}"#);
    spawn(&mut s, 3, &format!(r#"{{"Ui":{{"pos":{pos},"size":[0.1,0.17777778],"color":[1,0,0,1]}}{edge}}}"#));
    s
}

#[test]
fn an_edge_keeps_an_element_the_same_distance_from_the_edge_of_the_picture() {
    // 21:9 (840 x 360): margins of 100 pixels at each side. A box 12.8 pixels in from the left of the made-for picture (and 12.8 in from its right: x 0.88 + 0.1).
    let left = |edge: &str| bbox(&draw(&mut quad("[0.02,0.1]", edge), 840, 360), red).unwrap();
    let right = |edge: &str| bbox(&draw(&mut quad("[0.88,0.1]", edge), 840, 360), red).unwrap();
    assert_eq!(left("").0, 113, "in the safe area, 100 + 12.8");
    assert_eq!(left(r#","UiEdge":{"x":0.5}"#).0, 113, "0.5 is the middle: no change");
    assert_eq!(left(r#","UiEdge":{"x":0}"#).0, 13, "pinned to the left edge of the picture");
    assert_eq!(right(r#","UiEdge":{"x":1}"#).2, 840 - 14, "pinned to the right edge: 12.8 from it");
    assert_eq!(right("").2, 740 - 14, "in the safe area: 12.8 from its right edge, 100 in");
    // the other way round, 4:3 (640 x 480): margins of 60 pixels at the top and the bottom
    let tall = |pos: &str, edge: &str| bbox(&draw(&mut quad(pos, edge), 640, 480), red).unwrap();
    assert_eq!(tall("[0.1,0.05]", "").1, 60 + 18, "0.05 of 360 = 18, below the top margin");
    assert_eq!(tall("[0.1,0.05]", r#","UiEdge":{"y":0}"#).1, 18, "flush with the top of the picture");
    assert_eq!(tall("[0.1,0.77]", r#","UiEdge":{"y":1}"#).3, 460, "pinned to the bottom edge of the picture: the box ends 0.0522 of 360 = 18.8 pixels above it");
    // a size does not change with the edge
    assert_eq!(left(r#","UiEdge":{"x":0}"#).2 - left(r#","UiEdge":{"x":0}"#).0 + 1, 64);
}

#[test]
fn a_screen_element_covers_the_whole_picture_and_nothing_else_leaves_the_safe_area() {
    let build = |edge: &str| {
        let mut s = quad("[0,0]", edge);
        s.apply(&Command::Set { uid: 3, component: "Ui".into(), value: parse(r#"{"size":[1,1]}"#).unwrap() }).unwrap();
        s
    };
    let f = draw(&mut build(""), 840, 360);
    assert!(px(&f, 50, 100) == CLEAR && red(px(&f, 100, 100)) && red(px(&f, 739, 100)) && px(&f, 740, 100) == CLEAR, "in the safe area only");
    let f = draw(&mut build(r#","UiEdge":{"screen":true}"#), 840, 360);
    assert!(red(px(&f, 0, 0)) && red(px(&f, 839, 359)) && red(px(&f, 50, 100)), "over the whole picture");
}

#[test]
fn a_pin_to_the_world_stays_on_its_point_whatever_the_shape_of_the_picture() {
    let build = || {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,5]},"Camera":{"ortho":true,"ortho_height":4,"clear":[0.1,0.1,0.2,1]}}"#);
        spawn(&mut s, 2, r#"{"UiScale":{"aspect":1.7777778}}"#);
        spawn(&mut s, 3, r#"{"Ui":{"size":[0.05,0.08888889],"color":[1,0,0,1]},"UiAnchor":{"world":[1,0.5,0]}}"#);
        s
    };
    for (w, h) in [(640, 360), (840, 360), (640, 480)] {
        let f = draw(&mut build(), w, h);
        let want = f.world_to_pixel([1.0, 0.5, 0.0]).unwrap();
        let b = bbox(&f, red).unwrap();
        let centre = ((b.0 + b.2) as f32 / 2.0 + 0.5, (b.1 + b.3) as f32 / 2.0 + 0.5);
        assert!((centre.0 - want[0]).abs() <= 1.0 && (centre.1 - want[1]).abs() <= 1.0, "{w}x{h}: the tag is at {centre:?}, the point at {want:?}");
        // and its size is the made-for size (0.05 x 0.0889 of a safe area of the picture's height)
        let (_, _, aw, ah) = safe_area(SIXTEEN_NINE, w, h);
        assert!(((b.2 - b.0 + 1) as f32 - aw as f32 * 0.05).abs() <= 1.0 && ((b.3 - b.1 + 1) as f32 - ah as f32 * 0.088_888_89).abs() <= 1.0, "{w}x{h}: {b:?}");
    }
}

#[test]
fn the_lowest_numbered_scale_counts_and_a_nonsense_one_is_ignored() {
    let mut s = menu(true);
    spawn(&mut s, 3, r#"{"UiScale":{"aspect":1.0}}"#);
    let one = draw(&mut s, 640, 480);
    let mut t = menu(true);
    spawn(&mut t, 3, r#"{"UiScale":{"aspect":1.0}}"#);
    assert_eq!(one.hash(), draw(&mut t, 640, 480).hash());
    // uid 2 (16:9) wins over uid 3 (1:1): the red square is 64 pixels
    assert_eq!(bbox(&one, red).map(|b| (b.2 - b.0 + 1, b.3 - b.1 + 1)), Some((64, 64)));
    // 0, a negative number and a huge one do nothing: the picture is the stretched one
    let plain = draw(&mut menu(false), 640, 480).hash();
    for bad in ["0", "-2", "1e9"] {
        let mut m = menu(false);
        spawn(&mut m, 2, &format!(r#"{{"UiScale":{{"aspect":{bad}}}}}"#));
        assert_eq!(draw(&mut m, 640, 480).hash(), plain, "aspect {bad}");
    }
}
