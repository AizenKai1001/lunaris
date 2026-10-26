//! How well do stacks stand? A tower of N one-metre boxes (each can turn: an `AngularVelocity`) on a floor, started a hair out of line, stepped for ten
//! seconds; prints how far the top box drifted sideways, how far it sank, and the biggest speed at the end. Also a pyramid and a wall of boxes.
//!
//!     cargo run --release -p lunaris-sim --example stack_bench
use lunaris_json::{parse, Json};
use lunaris_sim::components::Transform;
use lunaris_sim::{Command, Sim};

fn spawn(sim: &mut Sim, uid: u64, json: &str) {
    let Json::Obj(components) = parse(json).unwrap() else { panic!() };
    sim.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn pos(sim: &Sim, uid: u64) -> [f32; 3] {
    sim.world.get::<Transform>(sim.world.entity(uid).unwrap()).unwrap().pos
}

fn floor(sim: &mut Sim) {
    spawn(sim, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[60,1,60]}}"#);
}

fn cube(sim: &mut Sim, uid: u64, p: [f32; 3], friction: f32) {
    spawn(sim, uid, &format!(r#"{{"Transform":{{"pos":{p:?}}},"Velocity":{{}},"AngularVelocity":{{"damping":0.05}},"Collider":{{"shape":1,"size":[1,1,1]}},"RigidBody":{{"mass":1,"restitution":0,"friction":{friction}}}}}"#));
}

fn tower(n: u64, friction: f32, reference: bool) -> (f32, f32, f32) {
    let mut sim = Sim::new(1);
    sim.reference_physics = reference;
    floor(&mut sim);
    for k in 0..n {
        // (a hair out of line, the same hair every run)
        let off = ((k * 7919) % 13) as f32 * 0.0008 - 0.005;
        cube(&mut sim, 10 + k, [off, 0.5 + k as f32 * 1.0, -off * 0.7], friction);
    }
    sim.step_n(600);
    let top = pos(&sim, 10 + n - 1);
    let drift = (top[0] * top[0] + top[2] * top[2]).sqrt();
    let sink = (n as f32 - 0.5) - top[1];
    let mut fastest = 0.0f32;
    for k in 0..n {
        let v = sim.world.get::<lunaris_sim::components::Velocity>(sim.world.entity(10 + k).unwrap()).unwrap().lin;
        fastest = fastest.max((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt());
    }
    (drift, sink, fastest)
}

/// A pyramid of `rows` rows (the bottom row has `rows` boxes, half a box apart is not used: boxes touch side by side with a hair of gap).
fn pyramid(rows: u64) -> (f32, f32) {
    let mut sim = Sim::new(1);
    floor(&mut sim);
    let mut uid = 10;
    let mut ids = Vec::new();
    for r in 0..rows {
        let count = rows - r;
        for c in 0..count {
            let x = (c as f32 - (count as f32 - 1.0) * 0.5) * 1.002;
            cube(&mut sim, uid, [x, 0.5 + r as f32 * 1.0, 0.0], 0.6);
            ids.push((uid, x, 0.5 + r as f32));
            uid += 1;
        }
    }
    sim.step_n(600);
    let (mut worst, mut sink) = (0.0f32, 0.0f32);
    for (id, x, y) in ids {
        let p = pos(&sim, id);
        worst = worst.max(((p[0] - x).powi(2) + p[2].powi(2)).sqrt());
        sink = sink.max(y - p[1]);
    }
    (worst, sink)
}

fn main() {
    println!("towers of 1 m boxes, friction 0.6, after 10 s: drift of the top box (cm), how far it sank (cm), top speed (m/s)");
    for n in [2u64, 4, 6, 8, 10, 12, 16, 24, 32, 48] {
        let (d, s, v) = tower(n, 0.6, false);
        println!("  {n:2} boxes: drift {:6.1} cm, sank {:6.1} cm, speed {v:.3}", d * 100.0, s * 100.0);
    }
    println!("pyramids (a row of N at the bottom): the worst box's move (cm), the deepest sink (cm)");
    for rows in [3u64, 5, 7] {
        let (w, s) = pyramid(rows);
        println!("  {rows} rows: moved {:6.1} cm, sank {:6.1} cm", w * 100.0, s * 100.0);
    }
}
