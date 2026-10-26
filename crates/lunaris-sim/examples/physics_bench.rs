//! Benchmark of the physics step: each scene is run with the reference step and with the fast one, the best of three runs of each, and the
//! state hashes of the two must be equal. Run it on a machine that is not busy with something else:
//!
//!     cargo run --release -p lunaris-sim --example physics_bench            (all scenes)
//!     cargo run --release -p lunaris-sim --example physics_bench -- evo     (the scenes whose names start with "evo")
use lunaris_ecs::Entity;
use lunaris_json::{parse, Json};
use lunaris_sim::components::Velocity;
use lunaris_sim::{Command, Sim};
use std::time::Instant;

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

struct Lcg(u64);
impl Lcg {
    fn f(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }
}

fn spawn(sim: &mut Sim, uid: &mut u64, comps: Vec<(&str, String)>) -> u64 {
    let id = *uid;
    sim.apply(&Command::Spawn { uid: Some(id), components: comps.into_iter().map(|(n, v)| (n.to_string(), j(&v))).collect() }).unwrap();
    *uid += 1;
    id
}

/// rooms of 12 static boxes and one dynamic sphere (a creature body moved by velocity, no gravity)
fn evo(rooms: usize) -> (Sim, Vec<Entity>) {
    let mut sim = Sim::new(5);
    let mut uid = 1u64;
    let g = (rooms as f32).sqrt().ceil() as usize;
    let mut rng = Lcg(77);
    let mut bodies = Vec::new();
    for r in 0..rooms {
        let (cx, cz) = ((r % g) as f32 * 40.0, (r / g) as f32 * 40.0);
        for i in 0..12 {
            let a = (i as f32) / 12.0 * std::f32::consts::TAU;
            let rr = 4.0 + 2.0 * ((i * 7) % 3) as f32;
            spawn(&mut sim, &mut uid, vec![("Transform", format!(r#"{{"pos":[{},0.5,{}]}}"#, cx + rr * a.cos(), cz + rr * a.sin())), ("Collider", r#"{"shape":1,"size":[1,1,1]}"#.into())]);
        }
        let a = rng.f() * std::f32::consts::TAU;
        let id = spawn(
            &mut sim,
            &mut uid,
            vec![
                ("Transform", format!(r#"{{"pos":[{cx},0.5,{cz}]}}"#)),
                ("Velocity", format!(r#"{{"lin":[{},0,{}]}}"#, 1.5 * a.cos(), 1.5 * a.sin())),
                ("Collider", r#"{"shape":0,"size":[0.8,0.8,0.8]}"#.into()),
                ("RigidBody", r#"{"mass":1,"gravity_scale":0}"#.into()),
            ],
        );
        bodies.push(sim.world.entity(id).unwrap());
    }
    (sim, bodies)
}

fn statics_only(n: usize) -> (Sim, Vec<Entity>) {
    let mut sim = Sim::new(5);
    let mut uid = 1u64;
    let g = ((n / 12) as f32).sqrt().ceil() as usize;
    for r in 0..n / 12 {
        let (cx, cz) = ((r % g) as f32 * 40.0, (r / g) as f32 * 40.0);
        for i in 0..12 {
            let a = (i as f32) / 12.0 * std::f32::consts::TAU;
            let rr = 4.0 + 2.0 * ((i * 7) % 3) as f32;
            spawn(&mut sim, &mut uid, vec![("Transform", format!(r#"{{"pos":[{},0.5,{}]}}"#, cx + rr * a.cos(), cz + rr * a.sin())), ("Collider", r#"{"shape":1,"size":[1,1,1]}"#.into())]);
        }
    }
    (sim, Vec::new())
}


/// 3,000 still boxes and 20 boxes that a system moves every tick without giving them a velocity: they share one archetype, so a cache of that
/// archetype is out of date every tick.
fn movers(n: usize) -> (Sim, Vec<Entity>) {
    let (mut sim, _) = statics_only(n);
    let mut uid = 1_000_000u64;
    let mut moving = Vec::new();
    for k in 0..20 {
        let id = spawn(&mut sim, &mut uid, vec![("Transform", format!(r#"{{"pos":[{},0.5,{}]}}"#, k as f32 * 3.0, 2.0)), ("Collider", r#"{"shape":1,"size":[1,1,1]}"#.into())]);
        moving.push(sim.world.entity(id).unwrap());
    }
    // (the spawned ones have the same archetype as the others: Transform + Collider)
    let mut dyn_bodies = Vec::new();
    for k in 0..40 {
        let id = spawn(&mut sim, &mut uid, vec![("Transform", format!(r#"{{"pos":[{},0.6,{}]}}"#, k as f32 * 1.5, 2.0)), ("Velocity", r#"{"lin":[0.5,0,0]}"#.into()), ("Collider", r#"{"shape":0,"size":[0.8,0.8,0.8]}"#.into()), ("RigidBody", r#"{"mass":1,"gravity_scale":0}"#.into())]);
        dyn_bodies.push(sim.world.entity(id).unwrap());
    }
    sim.add_system("slide", move |world, ctx| {
        for (k, e) in moving.iter().enumerate() {
            if let Some(t) = world.get_mut::<lunaris_sim::components::Transform>(*e) {
                t.pos[0] = k as f32 * 3.0 + ((ctx.tick % 200) as f32) * 0.02;
            }
        }
    });
    (sim, dyn_bodies)
}

/// n spheres dropped at random onto a floor
fn pile(n: usize) -> (Sim, Vec<Entity>) {
    let mut sim = Sim::new(5);
    let mut uid = 1u64;
    let mut rng = Lcg(3);
    spawn(&mut sim, &mut uid, vec![("Transform", r#"{"pos":[0,-0.5,0]}"#.into()), ("Collider", r#"{"shape":1,"size":[200,1,200]}"#.into())]);
    let mut bodies = Vec::new();
    for _ in 0..n {
        let (x, z, y) = ((rng.f() - 0.5) * 60.0, (rng.f() - 0.5) * 60.0, 0.6 + rng.f() * 25.0);
        let id = spawn(&mut sim, &mut uid, vec![("Transform", format!(r#"{{"pos":[{x},{y},{z}]}}"#)), ("Velocity", "{}".into()), ("Collider", r#"{"shape":0,"size":[1,1,1]}"#.into()), ("RigidBody", r#"{"mass":1,"restitution":0.2}"#.into())]);
        bodies.push(sim.world.entity(id).unwrap());
    }
    (sim, bodies)
}

/// boxes, spheres, capsules, rotated boxes and a few triggers, with walls
fn mixed(n: usize) -> (Sim, Vec<Entity>) {
    let mut sim = Sim::new(5);
    let mut uid = 1u64;
    let mut rng = Lcg(11);
    spawn(&mut sim, &mut uid, vec![("Transform", r#"{"pos":[0,-0.5,0]}"#.into()), ("Collider", r#"{"shape":1,"size":[120,1,120]}"#.into())]);
    for (x, z, sx, sz) in [(-30.0, 0.0, 1.0, 60.0), (30.0, 0.0, 1.0, 60.0), (0.0, -30.0, 60.0, 1.0), (0.0, 30.0, 60.0, 1.0)] {
        spawn(&mut sim, &mut uid, vec![("Transform", format!(r#"{{"pos":[{x},2,{z}]}}"#)), ("Collider", format!(r#"{{"shape":1,"size":[{sx},4,{sz}]}}"#))]);
    }
    for _ in 0..40 {
        let (x, z) = ((rng.f() - 0.5) * 50.0, (rng.f() - 0.5) * 50.0);
        spawn(&mut sim, &mut uid, vec![("Transform", format!(r#"{{"pos":[{x},0.6,{z}],"rot":[0,0.3826834,0,0.9238795]}}"#)), ("Collider", r#"{"shape":1,"size":[1.2,1.2,1.2]}"#.into())]);
    }
    for _ in 0..6 {
        let (x, z) = ((rng.f() - 0.5) * 40.0, (rng.f() - 0.5) * 40.0);
        spawn(&mut sim, &mut uid, vec![("Transform", format!(r#"{{"pos":[{x},1.0,{z}]}}"#)), ("Collider", r#"{"shape":0,"size":[5,5,5],"trigger":true}"#.into())]);
    }
    let mut bodies = Vec::new();
    for k in 0..n {
        let (x, z, y) = ((rng.f() - 0.5) * 50.0, (rng.f() - 0.5) * 50.0, 0.8 + rng.f() * 10.0);
        let shape = k % 3;
        let size = match shape {
            0 => r#"{"shape":0,"size":[0.8,0.8,0.8]}"#,
            1 => r#"{"shape":1,"size":[0.8,0.6,0.8]}"#,
            _ => r#"{"shape":2,"size":[0.5,1.6,0.5]}"#,
        };
        let id = spawn(&mut sim, &mut uid, vec![("Transform", format!(r#"{{"pos":[{x},{y},{z}]}}"#)), ("Velocity", format!(r#"{{"lin":[{},0,{}]}}"#, rng.f() * 4.0 - 2.0, rng.f() * 4.0 - 2.0)), ("Collider", size.into()), ("RigidBody", r#"{"mass":1,"restitution":0.3}"#.into())]);
        bodies.push(sim.world.entity(id).unwrap());
    }
    (sim, bodies)
}

/// `n` boxes in stacks of three on a big floor, spaced out, settled for 150 ticks before the clock starts; with `sleep` the world has a `PhysicsSleep`, so
/// the settled stacks are asleep when it does.
fn resting(n: usize, sleep: bool) -> (Sim, Vec<Entity>) {
    let mut sim = Sim::new(1);
    let mut uid = 1;
    spawn(&mut sim, &mut uid, vec![("Transform", r#"{"pos":[0,-0.5,0]}"#.into()), ("Collider", r#"{"shape":1,"size":[600,1,600]}"#.into()), ("RigidBody", r#"{"friction":0.8,"restitution":0}"#.into())]);
    if sleep {
        spawn(&mut sim, &mut uid, vec![("PhysicsSleep", r#"{"time":0.5}"#.into())]);
    }
    let stacks = n.div_ceil(3);
    let side = (stacks as f32).sqrt().ceil() as usize;
    let mut bodies = Vec::new();
    for k in 0..n {
        let (s, h) = (k / 3, k % 3);
        let (x, z) = ((s % side) as f32 * 2.5 - side as f32 * 1.25, (s / side) as f32 * 2.5 - side as f32 * 1.25);
        let id = spawn(&mut sim, &mut uid, vec![("Transform", format!(r#"{{"pos":[{x},{},{z}]}}"#, 0.5 + h as f32 * 1.001)), ("Velocity", "{}".into()), ("Collider", r#"{"shape":1,"size":[1,1,1]}"#.into()), ("RigidBody", r#"{"mass":1,"restitution":0,"friction":0.6}"#.into())]);
        bodies.push(sim.world.entity(id).unwrap());
    }
    sim.step_n(150);
    (sim, bodies)
}

fn run_one(build: &dyn Fn() -> (Sim, Vec<Entity>), reference: bool, ticks: u32, steer: bool) -> (f64, u64, usize, usize) {
    let (mut sim, bodies) = build();
    sim.reference_physics = reference;
    // (THREADS=n: the worker threads of the step; the default is half the cores)
    if let Some(n) = std::env::var("THREADS").ok().and_then(|v| v.parse::<usize>().ok()) {
        sim.threads = n.max(1);
    }
    sim.step_n(5);
    let t = Instant::now();
    for k in 0..ticks {
        if steer {
            for (i, e) in bodies.iter().enumerate() {
                if let Some(v) = sim.world.get_mut::<Velocity>(*e) {
                    v.lin = [(((i as u32 + k / 8) % 5) as f32 - 2.0) * 0.75, 0.0, (((i as u32 * 3 + k / 8) % 5) as f32 - 2.0) * 0.75];
                }
            }
        }
        sim.step();
    }
    (t.elapsed().as_secs_f64(), sim.state_hash(), sim.world.len(), bodies.len().max(1))
}

fn run(name: &str, build: &dyn Fn() -> (Sim, Vec<Entity>), ticks: u32, steer: bool) {
    // the machine is shared with other jobs: the best of three runs of each
    let best = |reference: bool| {
        let mut r = run_one(build, reference, ticks, steer);
        for _ in 0..2 {
            let x = run_one(build, reference, ticks, steer);
            if x.0 < r.0 {
                r = x;
            }
        }
        r
    };
    let (t_ref, h_ref, ents, n_dyn) = best(true);
    let (t_new, h_new, _, _) = best(false);
    if std::env::var("STAGES").is_ok() {
        // one more run of the fast step with its parts timed
        let (mut sim, bodies) = build();
        sim.step_n(5);
        sim.physics.set_profile(true);
        let _ = sim.physics.stage_ns();
        for k in 0..ticks {
            if steer {
                for (i, e) in bodies.iter().enumerate() {
                    if let Some(v) = sim.world.get_mut::<Velocity>(*e) {
                        v.lin = [(((i as u32 + k / 8) % 5) as f32 - 2.0) * 0.75, 0.0, (((i as u32 * 3 + k / 8) % 5) as f32 - 2.0) * 0.75];
                    }
                }
            }
            sim.step();
        }
        let ns = sim.physics.stage_ns();
        let total: u64 = ns.iter().sum();
        let parts: Vec<String> = lunaris_sim::physics::STAGES.iter().zip(ns).map(|(n, t)| format!("{n} {:.0}%", t as f64 * 100.0 / total.max(1) as f64)).collect();
        println!("    stages of the fast step ({:.3} ms a tick): {}", total as f64 / 1e6 / ticks as f64, parts.join(", "));
    }
    let per = |t: f64| t * 1e9 / (ticks as f64 * n_dyn as f64);
    println!(
        "{name:<22} ref {:7.3} ms/tick ({:6.0} ns/body)  fast {:7.3} ms/tick ({:6.0} ns/body)  speedup {:5.1}x  entities {ents:6}  hashes {}",
        t_ref * 1e3 / ticks as f64,
        per(t_ref),
        t_new * 1e3 / ticks as f64,
        per(t_new),
        t_ref / t_new,
        if h_ref == h_new { format!("EQUAL {h_new:016x}") } else { format!("DIFFER ref {h_ref:016x} fast {h_new:016x}") }
    );
}

fn main() {
    let which: Vec<String> = std::env::args().skip(1).collect();
    let all = which.is_empty();
    let want = |n: &str| all || which.iter().any(|w| n.starts_with(w.as_str()));
    if want("statics") {
        run("statics only 3072", &|| statics_only(3072), 100, false);
    }
    if want("evo256") {
        run("evo 256 rooms", &|| evo(256), 100, true);
    }
    if want("evo1024") {
        run("evo 1024 rooms", &|| evo(1024), 60, true);
    }
    if want("movers") {
        run("movers 3000+20", &|| movers(3072), 100, false);
    }
    if want("rest") {
        run("rest 3000 boxes", &|| resting(3000, false), 60, false);
        run("rest 3000 asleep", &|| resting(3000, true), 60, false);
    }
    if want("pile") {
        run("pile 5000 spheres", &|| pile(5000), 60, false);
    }
    if want("mixed") {
        run("mixed 600 bodies", &|| mixed(600), 100, false);
    }
}
