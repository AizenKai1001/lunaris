//! A creature that only holds and reels a thread towards a point high up: does the body rise? `silkprobe [GRIP_ON_FLOOR]`
use lunaris_creature::vecmath::*;
use lunaris_creature::{Brain, Creature, Flat, Model, Solids, WithSolids};
use lunaris_json::parse;

fn main() {
    let brain_json = r#"{"setup":[],"predict":[[1,1,0,0,0,0,-1000],[1,2,0,0,0,0,-1000],[1,3,0,0,0,0,-1000]],"learn":[[1,24,0,0,0,0,600],[1,28,0,0,0,0,1000],[1,29,0,0,0,0,1000]]}"#;
    let doc = parse(brain_json).unwrap();
    let brain = Brain::from_genome_json(&doc);
    let model = Model::spider();
    let mut solids = Solids::default();
    // a wall 1.2 m ahead, tall
    solids.add(v3(1.3, 1.6, 0.0), v3(0.0, 1.0, 0.0), 0.0, v3(0.1, 1.6, 2.0));
    let ground = WithSolids { base: &Flat, solids };
    let mut c = Creature::new(&model, brain, 1, &ground);
    for t in 0..360 {
        c.step([1.1, 2.4, 0.0], &ground);
        if t % 20 == 0 {
            let (p, _) = c.pose(0);
            let v = c.velocity(0);
            println!("t{t:3} pos {:+.2} {:+.2} {:+.2} v {:+.2} {:+.2} {:+.2} silk {} len {:.2} T {:.0} feet {}", p.x, p.y, p.z, v.x, v.y, v.z, c.anchored as i32, c.silk_len, c.silk_tension, c.foot.iter().filter(|f| **f).count());
        }
    }
}
