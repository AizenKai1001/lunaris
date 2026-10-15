//! Scroll views in the reference picture (docs/UI.md, "Scroll views"): everything inside a `UiScroll` is cut at its viewport, in whole pixels, partial lines of text included, and a scene without a scroll
//! view is drawn as it was. The GPU renderer draws the same `SceneRect`s and `SceneText`s with scissor rectangles; `crates/lunaris-gpu/tests/compare.rs` compares the two.
use super::*;
use lunaris_json::{parse, Json};
use lunaris_sim::{Command, Sim};

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
const W: u32 = 200;
const H: u32 = 100;

/// Things that stick out of a viewport on every side. With `inside` they are children of the view (`UiScroll`, x 0.1 to 0.6, y 0.2 to 0.7: pixels 20..120 by 20..70 on a 200 x 100 picture), without
/// they are loose, so the two pictures differ exactly in what the clip takes away.
fn scene(inside: bool) -> Sim {
    let parent = if inside { r#","Parent":{"target":2}"# } else { "" };
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,3]},"Camera":{"clear":[0.1,0.1,0.2,1]}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.1,0.2],"size":[0.5,0.5],"color":[0,0,0,0]},"UiScroll":{"drag":false}}"#);
    // a flat red band, wider and higher than the viewport on every side
    spawn(&mut s, 3, &format!(r#"{{"Ui":{{"pos":[0.0,0.1],"size":[1.0,0.3],"color":[1,0,0,1]}}{parent}}}"#));
    // a rounded gradient panel with a border and a shadow, half out of the viewport at the right
    spawn(&mut s, 4, &format!(r#"{{"Ui":{{"pos":[0.5,0.45],"size":[0.4,0.2],"color":[0.2,0.8,0.3,1],"border":0.01,"border_color":[1,1,1,1]}},"UiStyle":{{"radius":0.05,"color2":[0.0,0.3,0.1,1],"shadow_color":[0,0,0,0.6],"shadow_offset":[0.01,0.02]}}{parent}}}"#));
    // a button label in the built-in font, cut through the middle by the top edge
    spawn(&mut s, 5, &format!(r#"{{"Ui":{{"pos":[0.15,0.1],"size":[0.4,0.2],"color":[0,0,0,0],"text":"WWWW","text_size":0.3,"text_color":[1,1,0,1]}}{parent}}}"#));
    // a HUD text in the built-in font that runs out of the viewport at the bottom, and a TrueType one that runs out at the left
    spawn(&mut s, 6, &format!(r#"{{"Text":{{"value":"HHHH","pos":[0.2,0.6],"size":0.25,"color":[0,1,1,1],"shadow":true}}{parent}}}"#));
    spawn(&mut s, 7, &format!(r#"{{"Text":{{"value":"Ab","pos":[0.02,0.3],"size":0.3,"color":[1,0,1,1]}},"TextFont":{{"file":"pixel.ttf"}}{parent}}}"#));
    // a TrueType label of a Ui, and a picture
    spawn(&mut s, 8, &format!(r#"{{"Ui":{{"pos":[0.4,0.1],"size":[0.4,0.25],"color":[0,0,0,0],"text":"Ab","text_size":0.3,"text_color":[0.5,1,0.5,1]}},"TextFont":{{"file":"pixel.ttf"}}{parent}}}"#));
    spawn(&mut s, 9, &format!(r#"{{"Ui":{{"pos":[0.5,0.15],"size":[0.3,0.4],"color":[0,0,0,0]}},"UiStyle":{{"image":"checker","tint":[1,1,1,1]}}{parent}}}"#));
    s
}

fn in_view(x: u32, y: u32) -> bool {
    (20..120).contains(&x) && (20..70).contains(&y)
}

#[test]
fn what_is_inside_a_scroll_view_is_cut_at_its_viewport_and_nothing_else_changes() {
    let (loose, cut) = (draw(&mut scene(false), W, H), draw(&mut scene(true), W, H));
    let mut differs = 0;
    let mut kinds = [0usize; 3];
    for y in 0..H {
        for x in 0..W {
            if in_view(x, y) {
                assert_eq!(px(&cut, x, y), px(&loose, x, y), "inside the viewport at ({x}, {y}) the picture is what it is without the view");
            } else {
                assert_eq!(px(&cut, x, y), CLEAR, "outside the viewport at ({x}, {y}) nothing is drawn");
                if px(&loose, x, y) != CLEAR {
                    differs += 1;
                    kinds[if x < 20 { 0 } else if x >= 120 { 1 } else { 2 }] += 1;
                }
            }
        }
    }
    assert!(differs > 1000, "the loose picture really has things out there: {differs}");
    assert!(kinds.iter().all(|k| *k > 100), "cut at the left, at the right and above or below: {kinds:?}");
    // and the viewport is not empty (the cut did not hide everything)
    let drawn = (0..H).flat_map(|y| (0..W).map(move |x| (x, y))).filter(|(x, y)| in_view(*x, *y) && px(&cut, *x, *y) != CLEAR).count();
    assert!(drawn > 2000, "{drawn}");
}

#[test]
fn partial_lines_of_text_are_drawn_in_part_in_both_fonts() {
    // each kind of text on its own, in a picture where only it is drawn: the cut picture is the loose picture inside the viewport
    for keep in [5u64, 6, 7, 8] {
        let make = |inside: bool| {
            let mut s = scene(inside);
            for uid in [3u64, 4, 5, 6, 7, 8, 9] {
                if uid != keep {
                    let e = s.world.entity(uid).unwrap();
                    let _ = s.world.despawn(e);
                }
            }
            s
        };
        let (loose, cut) = (draw(&mut make(false), W, H), draw(&mut make(true), W, H));
        let (mut inside_n, mut cropped) = (0, 0);
        for y in 0..H {
            for x in 0..W {
                if in_view(x, y) {
                    assert_eq!(px(&cut, x, y), px(&loose, x, y), "text {keep} at ({x}, {y})");
                    inside_n += (px(&cut, x, y) != CLEAR) as usize;
                } else {
                    assert_eq!(px(&cut, x, y), CLEAR, "text {keep} leaks at ({x}, {y})");
                    cropped += (px(&loose, x, y) != CLEAR) as usize;
                }
            }
        }
        assert!(inside_n > 50 && cropped > 50, "text {keep}: {inside_n} pixels kept, {cropped} cut away");
    }
}

#[test]
fn a_view_that_covers_everything_changes_nothing() {
    // a viewport as big as the picture around the same things: identical picture, pixel for pixel
    let loose = draw(&mut scene(false), W, H);
    let mut s = scene(true);
    let e = s.world.entity(2).unwrap();
    {
        let u = s.world.get_mut::<lunaris_sim::components::Ui>(e).unwrap();
        u.pos = [0.0, 0.0];
        u.size = [1.0, 1.0];
    }
    let covered = draw(&mut s, W, H);
    assert_eq!(covered.hash(), loose.hash());
}

#[test]
fn shapes_wholly_outside_the_viewport_are_not_even_gathered() {
    let mut s = scene(true);
    let opts = RenderOptions { width: W, height: H, ..RenderOptions::default() };
    let mut tex = TextureCache::new(Some(testdata()));
    let before = crate::render::gather_scene(&mut s.world, &mut tex, &opts).unwrap();
    assert!(before.rects.iter().all(|r| r.clip.is_none_or(|c| r.x1 > c[0] && r.x0 < c[2] && r.y1 > c[1] && r.y0 < c[3])));
    assert!(before.rects.iter().any(|r| r.clip.is_some()) && before.texts.iter().any(|t| t.clip.is_some()));
    // move the panel (4) far below the viewport
    let e = s.world.entity(4).unwrap();
    s.world.get_mut::<lunaris_sim::components::Ui>(e).unwrap().pos = [0.5, 0.9];
    let after = crate::render::gather_scene(&mut s.world, &mut tex, &opts).unwrap();
    assert!(after.rects.iter().filter(|r| r.uid == 4).count() == 0 && before.rects.iter().filter(|r| r.uid == 4).count() > 0);
}

#[test]
fn a_group_that_is_not_shown_draws_nothing_below_it_and_shows_it_again_when_it_is() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,3]},"Camera":{"clear":[0.1,0.1,0.2,1]}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.1,0.1],"size":[0.3,0.3],"color":[1,0,0,1],"text":"HI","text_size":0.2},"UiGroup":{"shown":false}}"#);
    spawn(&mut s, 3, r#"{"Ui":{"pos":[0.5,0.1],"size":[0.3,0.3],"color":[0,1,0,1]},"Parent":{"target":2}}"#);
    spawn(&mut s, 4, r#"{"Text":{"value":"HHHH","pos":[0.1,0.6],"size":0.2,"color":[0,1,1,1]},"Parent":{"target":2}}"#);
    spawn(&mut s, 5, r#"{"Ui":{"pos":[0.5,0.6],"size":[0.3,0.3],"color":[0,0,1,1]}}"#);
    let hidden = draw(&mut s, W, H);
    assert_eq!(px(&hidden, 40, 20), CLEAR, "the group's own rectangle is not drawn");
    assert_eq!(px(&hidden, 120, 20), CLEAR, "nor its child");
    assert_eq!(px(&hidden, 140, 80), [0, 0, 255], "what is outside the group is drawn");
    let lit = (0..H).flat_map(|y| (0..W).map(move |x| (x, y))).filter(|(x, y)| px(&hidden, *x, *y) != CLEAR && *x < 95).count();
    assert_eq!(lit, 0, "and no text of the hidden children either");
    let e = s.world.entity(2).unwrap();
    s.world.get_mut::<lunaris_sim::components::UiGroup>(e).unwrap().shown = true;
    let shown = draw(&mut s, W, H);
    assert_eq!(px(&shown, 40, 20), [255, 0, 0]);
    assert_eq!(px(&shown, 120, 20), [0, 255, 0]);
    assert_ne!(shown.hash(), hidden.hash());
}

#[test]
fn the_clip_follows_the_safe_area_of_a_ui_scale() {
    // 21:9 window, a 16:9 interface: the viewport is in the safe area, so the cut is in the same place relative to it
    let (w, h) = (840u32, 360u32);
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,3]},"Camera":{"clear":[0.1,0.1,0.2,1]}}"#);
    spawn(&mut s, 9, r#"{"UiScale":{"aspect":1.7777778}}"#);
    spawn(&mut s, 2, r#"{"Ui":{"pos":[0.25,0.25],"size":[0.5,0.5],"color":[0,0,0,0]},"UiScroll":{}}"#);
    spawn(&mut s, 3, r#"{"Ui":{"pos":[0.0,0.0],"size":[1.0,1.0],"color":[1,0,0,1]},"Parent":{"target":2}}"#);
    let f = draw(&mut s, w, h);
    // the safe area is x 100..740, y 0..360; the viewport is its middle quarter: x 260..580, y 90..270
    for (x, y, red) in [(259u32, 180u32, false), (260, 180, true), (579, 180, true), (580, 180, false), (400, 89, false), (400, 90, true), (400, 269, true), (400, 270, false), (50, 180, false)] {
        assert_eq!(px(&f, x, y) != CLEAR, red, "({x}, {y})");
    }
}
