//! Live graphs: `UiPlot`.
use super::*;
use components::{Ui, UiPlot, UiPlotBar};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn setr(s: &mut Sim, name: &str, v: f64) {
    s.world.set_resource(name, Some(v)).unwrap();
}

/// The bars of the plot with `uid`, from the left: (visible, top, height).
fn bars(s: &mut Sim, uid: u64) -> Vec<(bool, f32, f32)> {
    let mut found: Vec<(u32, bool, f32, f32)> = Vec::new();
    s.world
        .for_each::<(&UiPlotBar, &Ui), _>(|_, (b, u)| {
            if b.plot.0 == uid {
                found.push((b.index, u.visible, u.pos[1], u.size[1]));
            }
        })
        .unwrap();
    found.sort_by_key(|f| f.0);
    found.into_iter().map(|f| (f.1, f.2, f.3)).collect()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

#[test]
fn a_plot_makes_its_bars_once_and_scrolls_a_new_sample_in_from_the_right() {
    let mut s = Sim::new(1);
    spawn(&mut s, 10, r#"{"Ui":{"pos":[0.1,0.2],"size":[0.4,0.2]},"UiPlot":{"source":"g","every":1,"samples":10}}"#);
    for tick in 0..10 {
        setr(&mut s, "g", tick as f64);
        s.step();
    }
    let b = bars(&mut s, 10);
    assert_eq!(b.len(), 10, "one bar per sample");
    // auto scale from zero to the largest sample (9): the newest bar is the full height and the first (a sample of 0) is empty
    assert!(close(b[9].2, 0.2) && close(b[9].1, 0.2) && b[9].0);
    assert!(!b[0].0);
    assert!(close(b[5].2, 0.2 * 5.0 / 9.0));
    for _ in 0..5 {
        s.step();
    }
    assert_eq!(bars(&mut s, 10).len(), 10, "the bars are made once");
    // the graph now shows 5 .. 9 and then five more 9s (the resource stayed at 9): scrolled five places to the left
    let b = bars(&mut s, 10);
    assert!(close(b[0].2, 0.2 * 5.0 / 9.0), "{b:?}");
    assert!(close(b[9].2, 0.2));
}

#[test]
fn bars_before_the_first_sample_stay_empty_and_every_sets_the_pace() {
    let mut s = Sim::new(1);
    spawn(&mut s, 10, r#"{"Ui":{"pos":[0.0,0.0],"size":[1.0,0.5]},"UiPlot":{"source":"g","every":3,"samples":10,"min":0,"max":10}}"#);
    setr(&mut s, "g", 5.0);
    for _ in 0..9 {
        s.step();
    }
    let plot = *s.world.get::<UiPlot>(s.world.entity(10).unwrap()).unwrap();
    assert_eq!(plot.count, 3, "9 ticks at one sample every 3");
    let b = bars(&mut s, 10);
    assert!(b[..7].iter().all(|bar| !bar.0), "nothing to show yet at the left");
    assert!(b[7..].iter().all(|bar| bar.0 && close(bar.2, 0.25)), "a fixed scale 0..10: 5 is half of 0.5");
}

#[test]
fn a_fixed_scale_clamps_and_a_log_scale_plots_orders_of_magnitude() {
    let mut s = Sim::new(1);
    spawn(&mut s, 10, r#"{"Ui":{"pos":[0.0,0.0],"size":[1.0,1.0]},"UiPlot":{"source":"g","every":1,"samples":4,"min":0,"max":10}}"#);
    for v in [-5.0, 5.0, 10.0, 99.0] {
        setr(&mut s, "g", v);
        s.step();
    }
    let b = bars(&mut s, 10);
    assert!(!b[0].0 && close(b[1].2, 0.5) && close(b[2].2, 1.0) && close(b[3].2, 1.0), "{b:?}");

    let mut s = Sim::new(1);
    spawn(&mut s, 20, r#"{"Ui":{"pos":[0.0,0.0],"size":[1.0,1.0]},"UiPlot":{"source":"n","every":1,"samples":3,"log":true}}"#);
    for v in [10.0, 1000.0, 100000.0] {
        setr(&mut s, "n", v);
        s.step();
    }
    let b = bars(&mut s, 20);
    // log10: 1, 3, 5 on a scale from the lowest (1) to the highest (5)
    assert!(!b[0].0 && close(b[1].2, 0.5) && close(b[2].2, 1.0), "{b:?}");
    let plot = *s.world.get::<UiPlot>(s.world.entity(20).unwrap()).unwrap();
    assert!(close(plot.scale_lo, 1.0) && close(plot.scale_hi, 5.0));
}

#[test]
fn bars_follow_a_plot_that_moves_and_hide_with_it() {
    let mut s = Sim::new(1);
    spawn(&mut s, 10, r#"{"Ui":{"pos":[0.0,0.0],"size":[0.5,0.5]},"UiPlot":{"source":"g","every":1,"samples":2,"min":0,"max":1}}"#);
    setr(&mut s, "g", 1.0);
    s.step();
    s.step();
    let e = s.world.entity(10).unwrap();
    s.world.get_mut::<Ui>(e).unwrap().pos = [0.25, 0.4];
    s.step();
    let b = bars(&mut s, 10);
    assert!(close(b[1].1, 0.4) && close(b[1].2, 0.5));
    s.world.get_mut::<Ui>(e).unwrap().visible = false;
    s.step();
    assert!(bars(&mut s, 10).iter().all(|bar| !bar.0), "a hidden plot hides its bars");
}
