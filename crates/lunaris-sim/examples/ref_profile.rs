//! Where does a step of the reference physics go in a scene? Loads a scene, steps it, and prints the share of each part of the reference step.
//!
//!     cargo run --release -p lunaris-sim --example ref_profile -- examples/dominoes.lunaris.json [ticks]
use lunaris_json::parse;
use lunaris_sim::Sim;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().expect("a scene file");
    let ticks: u32 = args.get(1).and_then(|t| t.parse().ok()).unwrap_or(600);
    let text = std::fs::read_to_string(path).expect("the scene file");
    let scene = parse(&text).expect("a scene");
    let mut sim = Sim::new(1);
    sim.load_scene(&scene).expect("loading the scene");
    sim.step_n(5);
    lunaris_sim::physics::reference_profile(true);
    let _ = lunaris_sim::physics::reference_stage_ns();
    let t0 = std::time::Instant::now();
    for _ in 0..ticks {
        sim.step();
    }
    let wall = t0.elapsed().as_secs_f64();
    let ns = lunaris_sim::physics::reference_stage_ns();
    let total: u64 = ns.iter().sum();
    println!("{path}: {:.3} ms a tick, {:.3} ms of it in the reference step", wall * 1e3 / ticks as f64, total as f64 / 1e6 / ticks as f64);
    for (name, t) in lunaris_sim::physics::REF_STAGES.iter().zip(ns) {
        println!("   {name:22} {:7.3} ms a tick  {:5.1}%", t as f64 / 1e6 / ticks as f64, t as f64 * 100.0 / total.max(1) as f64);
    }
}
