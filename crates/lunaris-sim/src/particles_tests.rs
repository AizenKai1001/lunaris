use super::*;
use components::{Emitter, Particle, Sprite, Transform};
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn count_particles(s: &mut Sim) -> usize {
    let mut n = 0;
    s.world.for_each::<(&Particle,), _>(|_, _| n += 1).unwrap();
    n
}

#[test]
fn an_emitter_settles_at_rate_times_life_and_never_exceeds_its_maximum() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{"pos":[0,1,0]},"Emitter":{"rate":60,"life":1,"life_var":0,"max":1000}}"#);
    s.step_n(30);
    let half = count_particles(&mut s);
    assert!((28..=32).contains(&half), "after half a second about 30 particles: {half}");
    s.step_n(120);
    let steady = count_particles(&mut s);
    assert!((58..=62).contains(&steady), "at steady state about rate x life = 60: {steady}");
    // a lower maximum caps it
    s.apply(&Command::Set { uid: 1, component: "Emitter".into(), value: parse(r#"{"max":20}"#).unwrap() }).unwrap();
    s.step_n(5);
    assert!(count_particles(&mut s) <= 60, "existing particles live on, no new ones while over the maximum");
    s.step_n(90);
    assert_eq!(count_particles(&mut s), 20, "settles at the maximum");
}

#[test]
fn a_burst_throws_once_and_particles_age_fall_shrink_and_fade() {
    let mut s = Sim::new(5);
    spawn(&mut s, 1, r#"{"Transform":{},"Emitter":{"rate":0,"burst":40,"life":1,"life_var":0,"speed":0,"speed_var":0,"spread":0,"size0":1,"size1":0,"gravity":[0,-10,0],"color0":[1,0,0,1],"color1":[0,0,1,0]}}"#);
    s.step();
    assert_eq!(count_particles(&mut s), 40, "the burst");
    let burst_left = s.world.get::<Emitter>(s.world.entity(1).unwrap()).unwrap().burst;
    assert_eq!(burst_left, 0, "a burst is thrown once");
    s.step_n(29); // half a second in
    let (mut sizes, mut ys, mut alphas) = (Vec::new(), Vec::new(), Vec::new());
    s.world.for_each::<(&Particle, &Transform, &Sprite), _>(|_, (_, t, sp)| {
        sizes.push(t.scale[0]);
        ys.push(t.pos[1]);
        alphas.push(sp.color[3]);
    }).unwrap();
    assert_eq!(sizes.len(), 40);
    assert!(sizes.iter().all(|s| (s - 0.5).abs() < 0.05), "half way through its life a particle is half size: {:?}", &sizes[..3]);
    assert!(alphas.iter().all(|a| (a - 0.5).abs() < 0.05), "and half transparent: {:?}", &alphas[..3]);
    assert!(ys.iter().all(|y| *y < -1.0), "gravity pulled it down (x = -0.5 * 10 * 0.5^2 = -1.25): {:?}", &ys[..3]);
    s.step_n(40);
    assert_eq!(count_particles(&mut s), 0, "all gone at the end of their life");
}

#[test]
fn a_cone_of_zero_spread_throws_along_the_direction_and_a_wide_one_goes_everywhere() {
    let mut s = Sim::new(2);
    spawn(&mut s, 1, r#"{"Transform":{},"Emitter":{"rate":0,"burst":100,"life":5,"life_var":0,"speed":3,"speed_var":0,"spread":0,"dir":[1,0,0],"gravity":[0,0,0]}}"#);
    s.step();
    let mut vs = Vec::new();
    s.world.for_each::<(&Particle, &components::Velocity), _>(|_, (_, v)| vs.push(v.lin)).unwrap();
    assert!(vs.iter().all(|v| (v[0] - 3.0).abs() < 1e-3 && v[1].abs() < 1e-3 && v[2].abs() < 1e-3), "straight along +x at speed 3: {:?}", vs[0]);
    let mut s = Sim::new(2);
    spawn(&mut s, 1, r#"{"Transform":{},"Emitter":{"rate":0,"burst":400,"life":5,"life_var":0,"speed":3,"speed_var":0,"spread":1,"dir":[0,1,0],"gravity":[0,0,0]}}"#);
    s.step();
    let mut up = 0;
    let mut down = 0;
    s.world.for_each::<(&Particle, &components::Velocity), _>(|_, (_, v)| {
        let len = (v.lin[0] * v.lin[0] + v.lin[1] * v.lin[1] + v.lin[2] * v.lin[2]).sqrt();
        assert!((len - 3.0).abs() < 1e-3, "speed is the speed: {len}");
        if v.lin[1] > 0.0 { up += 1 } else { down += 1 }
    }).unwrap();
    assert!(up > 120 && down > 120, "spread 1 covers the whole sphere: {up} up, {down} down");
}

#[test]
fn particles_are_deterministic_replay_exactly_and_a_removed_emitter_lets_them_die_out() {
    let build = || {
        let mut s = Sim::new(9);
        spawn(&mut s, 1, r#"{"Transform":{},"Emitter":{"rate":90,"life":0.8,"spread":0.6}}"#);
        spawn(&mut s, 2, r#"{"Transform":{"pos":[5,0,0]},"Emitter":{"rate":30,"life":0.5,"mesh":"sphere"}}"#);
        s
    };
    let template = build();
    let mut live = build();
    live.start_recording().unwrap();
    live.step_n(100);
    live.apply(&Command::Set { uid: 1, component: "Emitter".into(), value: parse(r#"{"burst":25}"#).unwrap() }).unwrap();
    live.step_n(60);
    let rec = live.stop_recording().unwrap();
    let again = Sim::replay(&template, &rec).unwrap();
    assert_eq!(again.state_hash(), live.state_hash(), "a replay throws exactly the same particles");
    assert_eq!(build().state_hash(), build().state_hash());
    // the second emitter makes 3D particles (MeshRenderer), the first sprites
    let mut meshes = 0;
    live.world.for_each::<(&Particle, &components::MeshRenderer), _>(|_, _| meshes += 1).unwrap();
    assert!(meshes > 5, "{meshes} mesh particles");
    // remove both emitters: everything dies out within the longest life
    live.apply(&Command::Despawn { uid: 1 }).unwrap();
    live.apply(&Command::Despawn { uid: 2 }).unwrap();
    live.step_n(80);
    assert_eq!(count_particles(&mut live), 0);
}
