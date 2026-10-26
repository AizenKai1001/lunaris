//! What the walk looks like in numbers: `gaitprobe FILE GENOME LEVEL [EPISODE] [TICKS]` runs one episode on the level and prints, for the eight legs, which feet are down at every 3rd tick as a strip (# down, . up),
//! then the gait's figures: the duty of each leg (share of the time down), its steps a second, how the two tetrapods (legs 0 2 5 7 against 1 3 4 6) alternate (1: perfectly opposite, 0: unrelated, -1: together),
//! how many feet are down on average and the share of ticks with fewer than 4, the body's speed (mean, uneven), its vertical wobble and turning wobble, and how much the thread is held and reeled.
//! NOSILK=1 forces the thread off (as in `nnprobe`).
use lunaris_creature::episode::{run_episode_with, Cfg};
use lunaris_creature::{Brain, Creature, Flat, Model, N_LEGS};
use lunaris_json::parse;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: gaitprobe FILE GENOME LEVEL [EPISODE] [TICKS]");
        std::process::exit(2);
    }
    let text = std::fs::read_to_string(&a[1]).expect("file");
    let which: usize = a[2].parse().unwrap();
    let level: i32 = a[3].parse().unwrap();
    let ep: u64 = a.get(4).map(|s| s.parse().unwrap()).unwrap_or(0);
    let ticks: usize = a.get(5).map(|s| s.parse().unwrap()).unwrap_or(720);
    let doc = parse(&text).expect("json");
    let mut brain = if doc.get("genomes").is_some() { Brain::from_champions_json(&text).unwrap().remove(which) } else { Brain::from_genome_json(&doc) };
    if std::env::var("NOSILK").is_ok() {
        if let Some(w) = brain.nn.as_mut() {
            use lunaris_creature::nn::{NNB_H, NNB_IN, NNB_OUT, NN_LEG_P};
            if w.len() == lunaris_creature::nn::NNP {
                w[NN_LEG_P + NNB_H * NNB_IN + NNB_H * NNB_H + NNB_H + NNB_OUT * NNB_H + 1] = -1.0e6;
            }
        }
    }
    let model = Model::spider();
    let cfg = Cfg { layout_seed: 1000 + ep, body_seed: ep + 1, level };
    // per tick: the feet, the speed along the ground, the head's height, the head's yaw rate, the thread
    let mut feet: Vec<[bool; N_LEGS]> = Vec::new();
    let mut speed: Vec<f32> = Vec::new();
    let mut height: Vec<f32> = Vec::new();
    let mut yaw: Vec<f32> = Vec::new();
    let mut held = 0usize;
    let mut prev_len = f32::NAN;
    let mut reeled = 0.0f32;
    let mut hook = |c: &Creature, _t: usize| {
        feet.push(c.foot);
        let v = c.velocity(0);
        speed.push((v.x * v.x + v.z * v.z).sqrt());
        height.push(c.pose(0).0.y);
        let f = c.facing();
        yaw.push(f.z.atan2(f.x));
        if c.anchored {
            held += 1;
            if prev_len == prev_len && prev_len > c.silk_len {
                reeled += c.silk_tension * (prev_len - c.silk_len);
            }
            prev_len = c.silk_len;
        } else {
            prev_len = f32::NAN;
        }
    };
    let out = run_episode_with(&model, brain, &cfg, &Flat, ticks, 3, None, Some(&mut hook));
    let n = feet.len();
    let yaw: Vec<f32> = (0..n).map(|i| if i == 0 { 0.0 } else { let mut d = yaw[i] - yaw[i - 1]; while d > std::f32::consts::PI { d -= 2.0 * std::f32::consts::PI; } while d < -std::f32::consts::PI { d += 2.0 * std::f32::consts::PI; } d * 60.0 }).collect();
    println!("level {} episode {ep}: reached {} score {:.2} stand {:.2} air {:.2} cv {:.2} bob {:.3}", level + 1, out.reached, out.score, out.stand, out.air, out.cv, out.bob);
    println!("feet down (every 3rd tick from tick 120; # down, . up), legs 0..7 = right 1-4 then left 1-4:");
    for l in 0..N_LEGS {
        let s: String = (120..(120 + 270).min(n)).step_by(3).map(|t| if feet[t][l] { '#' } else { '.' }).collect();
        println!("  leg {l}: {s}");
    }
    // the gait's figures, from tick 60 on (after it has stood up)
    let t0 = 60.min(n);
    let secs = (n - t0) as f32 / 60.0;
    let mut duty = [0.0f32; N_LEGS];
    let mut steps = [0.0f32; N_LEGS];
    for l in 0..N_LEGS {
        duty[l] = (t0..n).filter(|&t| feet[t][l]).count() as f32 / (n - t0) as f32;
        steps[l] = (t0 + 1..n).filter(|&t| feet[t][l] && !feet[t - 1][l]).count() as f32 / secs;
    }
    println!("duty  {}", duty.iter().map(|d| format!("{:4.2}", d)).collect::<Vec<_>>().join(" "));
    println!("steps/s {}", steps.iter().map(|d| format!("{:4.1}", d)).collect::<Vec<_>>().join(" "));
    let a_set = [0usize, 2, 5, 7];
    let b_set = [1usize, 3, 4, 6];
    // the alternation of the two tetrapods: the correlation of (feet of A down) with (feet of B down)
    let sa: Vec<f32> = (t0..n).map(|t| a_set.iter().filter(|&&l| feet[t][l]).count() as f32).collect();
    let sb: Vec<f32> = (t0..n).map(|t| b_set.iter().filter(|&&l| feet[t][l]).count() as f32).collect();
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    let (ma, mb) = (mean(&sa), mean(&sb));
    let cov = sa.iter().zip(sb.iter()).map(|(x, y)| (x - ma) * (y - mb)).sum::<f32>();
    let va = sa.iter().map(|x| (x - ma) * (x - ma)).sum::<f32>();
    let vb = sb.iter().map(|y| (y - mb) * (y - mb)).sum::<f32>();
    let corr = if va > 0.0 && vb > 0.0 { cov / (va * vb).sqrt() } else { 0.0 };
    let down: Vec<f32> = (t0..n).map(|t| feet[t].iter().filter(|f| **f).count() as f32).collect();
    let few = down.iter().filter(|d| **d < 4.0).count() as f32 / down.len() as f32;
    println!("tetrapod alternation {corr:+.2} (the alternating tetrapod gait is near -1), feet down on average {:.1}, share of the time with under 4 down {:.0}%", mean(&down), few * 100.0);
    let sp = &speed[t0..];
    let ms = mean(sp);
    let sd = (sp.iter().map(|v| (v - ms) * (v - ms)).sum::<f32>() / sp.len() as f32).sqrt();
    let hh = &height[t0..];
    let mh = mean(hh);
    let hsd = (hh.iter().map(|v| (v - mh) * (v - mh)).sum::<f32>() / hh.len() as f32).sqrt();
    let ys = &yaw[t0..];
    let yrms = (ys.iter().map(|v| v * v).sum::<f32>() / ys.len() as f32).sqrt();
    println!("speed {ms:.2} m/s (spread {sd:.2}), the head's height {mh:.2} m (wobble {hsd:.3}), turning wobble {yrms:.2} rad/s, thread held {:.0}% of the time, reeling work {reeled:.0} J", 100.0 * held as f32 / n as f32);
}
