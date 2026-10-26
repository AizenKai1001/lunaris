//! How accurate is the solver at different numbers of rounds? `jointerr FILE GENOME [GX GZ]`: walks the brain 480 ticks on flat ground for each (rounds, relax) and prints the worst joint gap (mean and maximum over the ticks) and how far it got.
use lunaris_creature::{Brain, Creature, Flat, Model};
use lunaris_json::parse;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&a[1]).unwrap();
    let which: usize = a[2].parse().unwrap();
    let (gx, gz): (f32, f32) = (a.get(3).map(|s| s.parse().unwrap()).unwrap_or(8.0), a.get(4).map(|s| s.parse().unwrap()).unwrap_or(0.0));
    let doc = parse(&text).unwrap();
    let brain = if doc.get("genomes").is_some() { Brain::from_champions_json(&text).unwrap().remove(which) } else { Brain::from_genome_json(&doc) };
    for (rounds, relax, ws) in [(8, 2.0f32, 0.9f32), (4, 2.0, 0.9), (3, 1.0, 0.9), (3, 1.0, 1.0), (2, 1.0, 0.9), (2, 1.0, 1.0), (2, 0.0, 1.0), (1, 1.0, 1.0)] {
        let mut model = Model::spider();
        model.relax = relax;
        model.ws = ws;
        let mut worst_sum = 0.0f64;
        let mut worst_max = 0.0f32;
        let mut dist = 0.0f32;
        for seed in 1..=4u64 {
            let mut c = Creature::new(&model, brain.clone(), seed, &Flat);
            c.rounds = rounds;
            for _ in 0..480 {
                c.step([gx, 0.27, gz], &Flat);
                let e = c.joint_error();
                worst_sum += e as f64;
                if e > worst_max {
                    worst_max = e;
                }
            }
            dist += c.pose(0).0.x;
        }
        println!("rounds {rounds} relax {relax} warm start {ws}: worst joint gap mean {:.2} mm, max {:.2} mm; walked {:.2} m in 8 s (mean of 4 bodies)", worst_sum / (4.0 * 480.0) * 1000.0, worst_max * 1000.0, dist / 4.0);
    }
}
