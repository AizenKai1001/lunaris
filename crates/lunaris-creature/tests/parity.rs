//! The port against the trainer: every golden episode of `tests/data/golden.json` (made by `spidergpu golden` from the trainer's CPU twin, which the GPU matches bit for bit) is run here, and the
//! results, the poses of the prosoma, the abdomen and the last foot every 20th tick, and a hash of the whole trace must be the same bits.
use lunaris_creature::episode::{run_episode, Cfg, TRACE_PER_TICK};
use lunaris_creature::ground::{Flat, Grid, Ground, TERRAIN_N};
use lunaris_creature::model::NB;
use lunaris_creature::{Brain, Model};
use lunaris_json::Json;

fn fnv(bits: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bits {
        for k in 0..4 {
            h ^= ((b >> (8 * k)) & 255) as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

fn golden() -> Json {
    lunaris_json::parse(include_str!("data/golden.json")).expect("the golden file is JSON")
}

fn int(j: &Json, key: &str) -> i64 {
    j.get(key).and_then(|v| v.as_i64()).unwrap_or_else(|| panic!("no number {key}"))
}

#[test]
fn the_golden_episodes_are_reproduced_bit_for_bit() {
    let doc = golden();
    assert_eq!(int(&doc, "trace_per_tick") as usize, TRACE_PER_TICK, "the trace layout of the trainer and of the crate");
    let model = Model::spider();
    // the terrains the cases use
    let mut terrains: Vec<(i64, Vec<f32>)> = Vec::new();
    for t in doc.get("terrains").and_then(|v| v.as_array()).unwrap() {
        let h: Vec<f32> = t.get("heights_bits").and_then(|v| v.as_array()).unwrap().iter().map(|b| f32::from_bits(b.as_i64().unwrap() as u32)).collect();
        assert_eq!(h.len(), TERRAIN_N * TERRAIN_N);
        terrains.push((int(t, "id"), h));
    }
    let cases = doc.get("cases").and_then(|v| v.as_array()).unwrap();
    assert!(cases.len() >= 6);
    for case in cases {
        let name = case.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let brain = Brain::from_genome_json(case.get("genome").unwrap());
        let cfg = Cfg { layout_seed: int(case, "layout") as u64, body_seed: int(case, "body") as u64, level: int(case, "level") as i32 };
        let tid = int(case, "terrain");
        let ticks = int(case, "ticks") as usize;
        let rounds = int(case, "rounds") as i32;
        let flat = Flat;
        let grid_h;
        let grid;
        let ground: &dyn Ground = if tid == 0 {
            &flat
        } else {
            grid_h = terrains.iter().find(|(id, _)| *id == tid).map(|(_, h)| h.clone()).expect("the terrain of the case");
            grid = Grid { h: &grid_h };
            &grid
        };
        let mut trace: Vec<f32> = Vec::new();
        let out = run_episode(&model, brain, &cfg, ground, ticks, rounds, Some(&mut trace));
        // the results
        let want: Vec<u32> = case.get("out_bits").and_then(|v| v.as_array()).unwrap().iter().map(|b| b.as_i64().unwrap() as u32).collect();
        let got = [out.score, out.reached, out.last_dist, out.stand, out.fall, out.first_tick, out.broken, out.air, out.cv, out.vmean, out.bob].map(|v| v.to_bits());
        assert_eq!(want.as_slice(), got.as_slice(), "{name} level {} terrain {tid}: the episode's results differ", int(case, "level"));
        // the poses at every 20th tick
        let cps: Vec<u32> = case.get("checkpoints").and_then(|v| v.as_array()).unwrap().iter().map(|b| b.as_i64().unwrap() as u32).collect();
        let mut i = 0;
        for tk in (0..ticks).step_by(20) {
            let o = tk * TRACE_PER_TICK;
            for body in [0usize, 1, NB - 1] {
                for k in 0..7 {
                    let g = trace.get(o + body * 7 + k).map(|v| v.to_bits());
                    assert_eq!(Some(cps[i]), g, "{name} level {} terrain {tid}: tick {tk} body {body} value {k} differs", int(case, "level"));
                    i += 1;
                }
            }
        }
        // the whole trace
        let bits: Vec<u32> = trace.iter().map(|v| v.to_bits()).collect();
        assert_eq!(int(case, "trace_hash") as u64, fnv(&bits), "{name} level {} terrain {tid}: the whole trace differs", int(case, "level"));
    }
}

#[test]
fn the_model_has_the_floats_of_the_trainer() {
    let m = Model::spider();
    // the spider has a prosoma, an abdomen and 8 legs of 6 segments; the root has no parent, every other body does
    assert_eq!(m.parent[0], -1.0);
    for i in 1..NB {
        assert!(m.parent[i] >= 0.0);
    }
    assert!(m.inv_mass.iter().all(|v| *v > 0.0));
}
