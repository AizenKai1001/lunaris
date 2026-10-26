//! Prints what a brain does tick by tick on flat ground: `trace FILE GENOME LEVEL [TICKS] [EVERY]`.
use lunaris_creature::{Brain, Creature, Flat, Model};
use lunaris_json::parse;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&a[1]).unwrap();
    let which: usize = a[2].parse().unwrap();
    let ticks: usize = a.get(4).map(|s| s.parse().unwrap()).unwrap_or(240);
    let every: usize = a.get(5).map(|s| s.parse().unwrap()).unwrap_or(6);
    let doc = parse(&text).unwrap();
    let brain = if doc.get("genomes").is_some() { Brain::from_champions_json(&text).unwrap().remove(which) } else { Brain::from_genome_json(&doc) };
    let model = Model::spider();
    let seed: u64 = a.get(9).map(|s| s.parse().unwrap()).unwrap_or(1);
    let mut c = Creature::new(&model, brain, seed, &Flat);
    let gx: f32 = a.get(6).map(|s| s.parse().unwrap()).unwrap_or(1.0e4);
    let gz: f32 = a.get(7).map(|s| s.parse().unwrap()).unwrap_or(0.0);
    let yaw: f32 = a.get(8).map(|s| s.parse().unwrap()).unwrap_or(0.0);
    c.place(0.0, 0.0, yaw, &Flat);
    for t in 0..ticks {
        c.step([gx, 0.27, gz], &Flat);
        if t % every == 0 {
            let (p, _) = c.pose(0);
            let v = c.velocity(0);
            let f = c.facing();
            let heading = (-f.z).atan2(f.x).to_degrees();
            let dist = ((gx - p.x).powi(2) + (gz - p.z).powi(2)).sqrt();
            let feet: String = c.foot.iter().map(|f| if *f { '#' } else { '.' }).collect();
            let cmd = |leg: usize| format!("{:+.2} {:+.2} {:+.2} {:+.2}", c.command(2 + 6 * leg), c.command(3 + 6 * leg), c.command(4 + 6 * leg), c.command(5 + 6 * leg));
            println!("t{t:4} P {:.2} y {:.3} pos {:+.2} {:+.2} head {:+4.0} dist {dist:.2} v {:+.2} {:+.2} feet {feet} | leg0 {} | leg4 {}", c.pressure, p.y, p.x, p.z, heading, v.x, v.z, cmd(0), cmd(4));
        }
    }
}
