//! Runs one episode of a level on the CPU port and prints the body's progress: `climb FILE GENOME LEVEL LAYOUT BODY [TICKS] [EVERY]`.
use lunaris_creature::episode::{run_episode_with, Cfg, TRACE_PER_TICK};
use lunaris_creature::model::NB;
use lunaris_creature::vecmath::*;
use lunaris_creature::{Brain, Flat, Model};
use lunaris_json::parse;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&a[1]).unwrap();
    let which: usize = a[2].parse().unwrap();
    let level: i32 = a[3].parse().unwrap();
    let layout: u64 = a[4].parse().unwrap();
    let body: u64 = a[5].parse().unwrap();
    let ticks: usize = a.get(6).map(|s| s.parse().unwrap()).unwrap_or(720);
    let every: usize = a.get(7).map(|s| s.parse().unwrap()).unwrap_or(30);
    let doc = parse(&text).unwrap();
    let brain = if doc.get("genomes").is_some() { Brain::from_champions_json(&text).unwrap().remove(which) } else { Brain::from_genome_json(&doc) };
    let model = Model::spider();
    let mut tr: Vec<f32> = Vec::new();
    let mut silk: Vec<(bool, f32, f32, usize, [f32; 3])> = Vec::new();
    let mut hook = |c: &lunaris_creature::Creature, _t: usize| silk.push((c.anchored, c.silk_len, c.silk_tension, c.foot.iter().filter(|f| **f).count(), [c.anchor.x, c.anchor.y, c.anchor.z]));
    let out = run_episode_with(&model, brain, &Cfg { layout_seed: layout, body_seed: body, level }, &Flat, ticks, 8, Some(&mut tr), Some(&mut hook));
    println!("{out:?}");
    let n = tr.len() / TRACE_PER_TICK;
    for t in (0..n).step_by(every) {
        let o = t * TRACE_PER_TICK;
        let p = v3(tr[o], tr[o + 1], tr[o + 2]);
        let q = [tr[o + 3], tr[o + 4], tr[o + 5], tr[o + 6]];
        let r = from_quat(&q);
        let up = mulv(&r, v3(0.0, 1.0, 0.0));
        let fw = mulv(&r, v3(1.0, 0.0, 0.0));
        let e = o + NB * 7;
        println!("t{t:4} pos {:+.2} {:+.2} {:+.2}  fwd {:+.2} {:+.2} {:+.2}  up {:+.2} {:+.2} {:+.2}  target {:+.2} {:+.2} {:+.2}  dist {:.2} contacts {} feet {} silk {} len {:.2} T {:.0} anchor {:+.2} {:+.2} {:+.2}", p.x, p.y, p.z, fw.x, fw.y, fw.z, up.x, up.y, up.z, tr[e], tr[e + 2], tr[e + 1], tr[e + 4], tr[e + 6] as i32, silk[t].3, silk[t].0 as i32, silk[t].1, silk[t].2, silk[t].4[0], silk[t].4[1], silk[t].4[2]);
    }
}
