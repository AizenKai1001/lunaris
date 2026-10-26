//! What it costs to gather the static solids of a world for rays, which perception, flying machines, interaction and others each do once a step: `cargo test --release -p lunaris-sim --lib ray_gather_bench -- --ignored --nocapture`.
use super::*;
use crate::vehicle_ray::RayWorld;
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

#[test]
#[ignore]
fn ray_gather_bench() {
    // a streamed city with every block loaded: thousands of doors and crates (boxes, some turned) over 1.5 km
    let mut s = Sim::new(1);
    let mut seed = 99u64;
    let mut rnd = |m: f32| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as f32 / (1u64 << 31) as f32) * m
    };
    for n in 0..13_000u64 {
        let (x, z, yaw) = (rnd(1500.0) - 750.0, rnd(1500.0) - 750.0, rnd(6.28));
        spawn(&mut s, 1000 + n, &format!(r#"{{"Transform":{{"pos":[{x},1,{z}],"rot":[0,{},0,{}],"scale":[1.1,2,0.1]}},"Collider":{{"shape":1,"size":[1,1,1]}}}}"#, (yaw * 0.5).sin(), (yaw * 0.5).cos()));
    }
    for (name, reach) in [("whole map", Some(([-800.0, -50.0, -800.0], [800.0, 100.0, 800.0]))), ("a 200 m box", Some(([-100.0, -50.0, -100.0], [100.0, 100.0, 100.0]))), ("no limit", None)] {
        let t = std::time::Instant::now();
        let mut n = 0;
        for _ in 0..40 {
            let rw = RayWorld::gather(&mut s.world, &s.heightfields, &s.meshes, reach);
            std::hint::black_box(&rw);
            n += 1;
        }
        println!("gather, {name}: {:.3} ms each (13,000 solids in the world) {n}", t.elapsed().as_secs_f64() * 1000.0 / 40.0);
    }
}
