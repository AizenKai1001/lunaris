//! Radial fills (`UiRadial`, docs/UI.md, "Radial fill") in the reference picture: a sector of a circle, a ring or the whole rectangle, clockwise or not, from any starting angle.
//! `crates/lunaris-gpu/tests/compare.rs` compares the GPU's picture of the same shapes with this one.
use super::*;
use crate::render::Sweep;
use lunaris_json::{parse, Json};
use lunaris_sim::{Command, Sim};

fn spawn(sim: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    sim.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn draw(s: &mut Sim, w: u32, h: u32) -> Frame {
    let mut tex = TextureCache::new(None);
    let f = render(&mut s.world, &mut tex, &RenderOptions { width: w, height: h, ..RenderOptions::default() }).unwrap();
    assert!(f.warnings.is_empty(), "{:?}", f.warnings);
    f
}

fn px(f: &Frame, x: u32, y: u32) -> [u8; 3] {
    let o = ((y * f.width + x) * 4) as usize;
    [f.rgba[o], f.rgba[o + 1], f.rgba[o + 2]]
}

const CLEAR: [u8; 3] = [26, 26, 51];

/// A 100 x 100 pixel box in the middle of a 200 x 100 picture (x 50..150), with a radial fill in red.
fn dial(radial: &str, ui_extra: &str) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,0,3]},"Camera":{"clear":[0.1,0.1,0.2,1]}}"#);
    spawn(&mut s, 2, &format!(r#"{{"Ui":{{"pos":[0.25,0.0],"size":[0.5,1.0],"color":[1,0,0,1]{ui_extra}}},"UiRadial":{radial}}}"#));
    s
}

fn lit(f: &Frame, x: u32, y: u32) -> bool {
    px(f, x, y) != CLEAR
}

/// Which of the four quadrants of the dial (top right, bottom right, bottom left, top left) have their middle drawn.
fn quadrants(f: &Frame) -> [bool; 4] {
    [lit(f, 125, 25), lit(f, 125, 75), lit(f, 75, 75), lit(f, 75, 25)]
}

#[test]
fn a_sector_starts_at_twelve_and_runs_clockwise_or_not_from_any_angle() {
    for (radial, want, what) in [
        (r#"{"value":0.25}"#, [true, false, false, false], "a quarter from twelve o'clock: the top right"),
        (r#"{"value":0.5}"#, [true, true, false, false], "half: the right side"),
        (r#"{"value":0.75}"#, [true, true, true, false], "three quarters"),
        (r#"{"value":1.0}"#, [true, true, true, true], "all of it"),
        (r#"{"value":0.0}"#, [false, false, false, false], "nothing"),
        (r#"{"value":0.25,"clockwise":false}"#, [false, false, false, true], "counter-clockwise from twelve: the top left"),
        (r#"{"value":0.25,"start":90}"#, [false, true, false, false], "from three o'clock: the bottom right"),
        (r#"{"value":0.25,"start":180}"#, [false, false, true, false], "from six o'clock: the bottom left"),
        (r#"{"value":0.25,"start":270}"#, [false, false, false, true], "from nine o'clock: the top left"),
        (r#"{"value":0.25,"start":90,"clockwise":false}"#, [true, false, false, false], "from three o'clock, back: the top right"),
        (r#"{"value":0.25,"start":-90}"#, [false, false, false, true], "a negative start is just another angle"),
        (r#"{"value":0.25,"start":450}"#, [false, true, false, false], "and so is one over a turn"),
        (r#"{"value":3.0}"#, [true, true, true, true], "a value over 1 is all of it"),
        (r#"{"value":-1.0}"#, [false, false, false, false], "and one below 0 is nothing"),
    ] {
        let f = draw(&mut dial(radial, ""), 200, 100);
        assert_eq!(quadrants(&f), want, "{what}: {radial}");
    }
}

#[test]
fn the_circle_is_inscribed_in_the_box_and_the_whole_box_shape_reaches_the_corners() {
    let circle = draw(&mut dial(r#"{"value":1.0}"#, ""), 200, 100);
    assert!(lit(&circle, 100, 51) && lit(&circle, 51, 50) && lit(&circle, 148, 50) && lit(&circle, 100, 2), "the middle and the four ends of the circle");
    assert!(!lit(&circle, 52, 2) && !lit(&circle, 148, 98) && !lit(&circle, 60, 10), "the corners of the box are outside the circle");
    assert!(!lit(&circle, 45, 50) && !lit(&circle, 155, 50), "and the picture beside the box is untouched");
    // the edge is anti-aliased: along the diagonal some pixels are partly the colour, neither clear nor full red
    let partly = (0..100).filter(|i| ![CLEAR, [255, 0, 0]].contains(&px(&circle, 50 + i, *i))).count();
    assert!(partly > 0, "a soft edge");
    let boxed = draw(&mut dial(r#"{"value":1.0,"shape":1}"#, ""), 200, 100);
    assert!(lit(&boxed, 52, 2) && lit(&boxed, 148, 98) && lit(&boxed, 60, 10), "the whole box");
    assert!(!lit(&boxed, 45, 50));
    // a clock wipe over the box: a quarter from twelve to three is the top right quarter of the box, corner and all
    let wipe = draw(&mut dial(r#"{"value":0.25,"shape":1}"#, ""), 200, 100);
    assert!(lit(&wipe, 148, 2) && lit(&wipe, 125, 25) && !lit(&wipe, 75, 25) && !lit(&wipe, 148, 98) && !lit(&wipe, 75, 75));
    // a quarter turn is a quarter of the area, of the box or of the circle (within the circle's rounded edge)
    let count = |f: &Frame| (0..100).flat_map(|y| (50..150).map(move |x| (x, y))).filter(|(x, y)| lit(f, *x, *y)).count() as f32;
    assert!((count(&wipe) - 2500.0).abs() < 30.0, "{}", count(&wipe));
    let disc = count(&circle);
    // (a pixel that is only partly in the circle counts as drawn here, which is about half a pixel all round the edge)
    assert!((disc - std::f32::consts::PI * 2500.0).abs() < 250.0, "{disc}");
    let quarter = count(&draw(&mut dial(r#"{"value":0.25}"#, ""), 200, 100));
    assert!((quarter - disc / 4.0).abs() < 100.0, "{quarter} of {disc}");
}

#[test]
fn a_ring_has_a_hole_and_a_track_shows_behind_the_part_that_is_filled() {
    let ring = draw(&mut dial(r#"{"value":1.0,"inner":0.6}"#, ""), 200, 100);
    assert!(!lit(&ring, 100, 50) && !lit(&ring, 100, 40) && !lit(&ring, 120, 50), "the hole: up to 0.6 of the radius, 30 pixels");
    assert!(lit(&ring, 100, 10) && lit(&ring, 135, 50) && lit(&ring, 100, 90), "the ring");
    // a track (back_color) is the whole shape behind; the colour is the part that is filled
    let f = draw(&mut dial(r#"{"value":0.25,"inner":0.6}"#, r#","back_color":[0,0,1,1]"#), 200, 100);
    assert_eq!(px(&f, 125, 20), [255, 0, 0], "the filled quarter");
    assert_eq!(px(&f, 75, 20), [0, 0, 255], "the rest of the ring is the track");
    assert_eq!(px(&f, 100, 50), CLEAR, "and the hole is a hole in the track too");
    // with a value of 0 only the track is there
    let f = draw(&mut dial(r#"{"value":0.0}"#, r#","back_color":[0,0,1,1]"#), 200, 100);
    assert_eq!((px(&f, 125, 25), px(&f, 75, 75)), ([0, 0, 255], [0, 0, 255]));
}

#[test]
fn it_takes_the_buttons_shading_replaces_the_bar_and_ignores_the_border_and_a_picture() {
    // a button that is pressed is darker, like any button
    let mut s = dial(r#"{"value":1.0}"#, r#","button":true,"pressed":true"#);
    let pressed = draw(&mut s, 200, 100);
    assert!(px(&pressed, 100, 50)[0] > 150 && px(&pressed, 100, 50)[0] < 200, "darker than the plain red: {:?}", px(&pressed, 100, 50));
    // `fill` (a bar) does not apply: a half bar is still all of the dial, and the border is not drawn round a circle
    let mut s = dial(r#"{"value":1.0}"#, r#","fill":0.5,"border":0.05,"border_color":[0,1,0,1]"#);
    let f = draw(&mut s, 200, 100);
    assert_eq!(px(&f, 100, 50), [255, 0, 0]);
    assert_eq!(px(&f, 140, 50), [255, 0, 0]);
    assert!((0..200).all(|x| (0..100).all(|y| px(&f, x, y)[1] < 200)), "no green border");
    // the label is still drawn on top
    let mut s = dial(r#"{"value":0.5}"#, r#","text":"HI","text_size":0.4,"text_color":[1,1,1,1]"#);
    let f = draw(&mut s, 200, 100);
    assert!((60..140).any(|x| (20..80).any(|y| px(&f, x, y) == [255, 255, 255])));
}

#[test]
fn the_same_dial_is_the_same_picture_and_sweeps_know_which_directions_they_cover() {
    let a = draw(&mut dial(r#"{"value":0.37,"start":33,"clockwise":false,"inner":0.2}"#, ""), 200, 100);
    let b = draw(&mut dial(r#"{"value":0.37,"start":33,"clockwise":false,"inner":0.2}"#, ""), 200, 100);
    assert_eq!(a.hash(), b.hash());
    // the test itself: directions are from the middle, x right and y down; 0 degrees is up and the turn is clockwise
    let s = |start: f32, amount: f32, clockwise: bool| Sweep { start, amount, clockwise, shape: 0, inner: 0.0 };
    let q = s(0.0, 0.25, true);
    assert!(q.contains(1.0, -1.0) && q.contains(0.0, -1.0) && q.contains(1.0, 0.0), "up, up-right and right (the straight edges count)");
    assert!(!q.contains(1.0, 1.0) && !q.contains(-1.0, -1.0) && !q.contains(-1.0, 0.0));
    let three = s(0.0, 0.75, true);
    assert!(three.contains(1.0, 1.0) && three.contains(-1.0, 1.0) && three.contains(-1.0, 0.0) && !three.contains(-1.0, -1.0) && three.contains(0.0, -1.0));
    let back = s(0.0, 0.25, false);
    assert!(back.contains(-1.0, -1.0) && !back.contains(1.0, -1.0));
    assert_eq!(s(0.0, 0.5, true).span(), (0.0, 180.0));
    assert_eq!(s(0.0, 0.5, false).span(), (-180.0, 180.0));
    assert!(!s(0.0, 0.0, true).contains(0.0, -1.0) && s(0.0, 1.0, true).contains(0.3, 0.7));
    // nonsense is nothing, not a panic
    assert!(!s(f32::NAN, f32::NAN, true).contains(0.0, -1.0));
}
