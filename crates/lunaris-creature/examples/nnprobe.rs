//! What a brain does on each level, with the CPU port: `nnprobe FILE GENOME LEVELS EPISODES [THREADS]` (LEVELS: `all`, a number or `a-b`). For every level: how many episodes solved it, the mean score, the
//! highest the body got (metres of the creature's own world), and how much it used the silk: the share of ticks the thread was anchored and the longest thread. Individual bodies (body seed = episode + 1),
//! layouts 1000 + episode, 480 ticks from level 10 and 720 before, 3 rounds: what the trainer's `inspect` does, plus a look inside.
use lunaris_creature::episode::{level_of, run_episode_with, Cfg, EpisodeOut};
use lunaris_creature::{Brain, Creature, Flat, Model};
use lunaris_json::parse;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("usage: nnprobe FILE GENOME LEVELS EPISODES [THREADS]");
        std::process::exit(2);
    }
    let text = std::fs::read_to_string(&a[1]).expect("file");
    let which: usize = a[2].parse().unwrap();
    let levels: Vec<i32> = if a[3] == "all" {
        (0..25).collect()
    } else if let Some((lo, hi)) = a[3].split_once('-') {
        (lo.parse().unwrap()..=hi.parse().unwrap()).collect()
    } else {
        vec![a[3].parse().unwrap()]
    };
    let n: usize = a[4].parse().unwrap();
    let threads: usize = a.get(5).map(|s| s.parse().unwrap()).unwrap_or(4);
    let doc = parse(&text).expect("json");
    let brain = if doc.get("genomes").is_some() { Brain::from_champions_json(&text).unwrap().remove(which) } else { Brain::from_genome_json(&doc) };
    let mut brain = brain;
    // NOSILK=1: the body net's hold output is pushed far below zero, so the thread is never held (what the same brain does without silk)
    if std::env::var("NOSILK").is_ok() {
        if let Some(w) = brain.nn.as_mut() {
            use lunaris_creature::nn::{NNB_H, NNB_IN, NNB_OUT, NN_LEG_P};
            w[NN_LEG_P + NNB_H * NNB_IN + NNB_H * NNB_H + NNB_H + NNB_OUT * NNB_H + 1] = -1.0e6;
        }
    }
    let model = Model::spider();
    for &level in &levels {
        let need = level_of(level).need as f32;
        // as the trainer's episode_ticks: the tall wall and the tall furniture are given longer
        let ticks: usize = if let Some(v) = std::env::var("NNTICKS").ok().and_then(|v| v.parse().ok()) { v } else { match level {
            15 | 16 => 600,
            17 => 720,
            18 => 900,
            19 => 1200,
            21 => 720,
            22 => 900,
            23 => 1500,
            24 => 2400,
            10..=24 => 480,
            _ => 720,
        } };
        // (result, highest y, ticks anchored, longest thread, silk work in joules)
        let rows: Vec<(EpisodeOut, f32, f32, f32, f32)> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..threads)
                .map(|t| {
                    let (model, brain) = (&model, &brain);
                    s.spawn(move || {
                        let mut out = Vec::new();
                        let mut i = t;
                        while i < n {
                            let cfg = Cfg { layout_seed: 1000 + i as u64, body_seed: i as u64 + 1, level };
                            let (mut top, mut held, mut longest, mut work, mut prev_len) = (0.0f32, 0u32, 0.0f32, 0.0f32, f32::NAN);
                            let show = std::env::var("NNTRACE").map(|v| v.parse::<usize>().ok() == Some(i)).unwrap_or(false);
                            let mut hook = |c: &Creature, tick: usize| {
                                let (p, _) = c.pose(0);
                                if show && tick % 60 == 0 {
                                    println!("  episode {i} tick {tick:4}: prosoma ({:+.2} {:+.2} {:+.2}) feet {} thread {}", p.x, p.y, p.z, c.foot.iter().filter(|f| **f).count(), if c.anchored { format!("{:.2} m, {:.0} N, anchor ({:+.2} {:+.2} {:+.2})", c.silk_len, c.silk_tension, c.anchor.x, c.anchor.y, c.anchor.z) } else { "-".to_string() });
                                }
                                top = top.max(p.y);
                                if c.anchored {
                                    held += 1;
                                    longest = longest.max(c.silk_len);
                                    // the work of reeling: the tension times the thread taken in this tick
                                    if prev_len == prev_len && prev_len > c.silk_len {
                                        work += c.silk_tension * (prev_len - c.silk_len);
                                    }
                                    prev_len = c.silk_len;
                                } else {
                                    prev_len = f32::NAN;
                                }
                            };
                            let r = run_episode_with(model, brain.clone(), &cfg, &Flat, ticks, 3, None, Some(&mut hook));
                            if std::env::var("NNLIST").is_ok() {
                                println!("  episode {i}: reached {} score {:.2} highest {:.2} stand {:.2} fall {:.2} broken {}", r.reached, r.score, top, r.stand, r.fall, r.broken);
                            }
                            out.push((r, top, held as f32 / ticks as f32, longest, work));
                            i += threads;
                        }
                        out
                    })
                })
                .collect();
            hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
        });
        let m = rows.len() as f32;
        let solved = rows.iter().filter(|r| r.0.reached >= need).count();
        let score = rows.iter().map(|r| r.0.score).sum::<f32>() / m;
        let top = rows.iter().map(|r| r.1).sum::<f32>() / m;
        let held = rows.iter().map(|r| r.2).sum::<f32>() / m;
        let longest = rows.iter().map(|r| r.3).fold(0.0f32, f32::max);
        let work = rows.iter().map(|r| r.4).sum::<f32>() / m;
        let firsts: Vec<f32> = rows.iter().filter(|r| r.0.first_tick >= 0.0).map(|r| r.0.first_tick).collect();
        let first = if firsts.is_empty() { -1.0 } else { firsts.iter().sum::<f32>() / firsts.len() as f32 };
        let users = rows.iter().filter(|r| r.2 > 0.05).count();
        println!("level {:2}: solved {solved:3}/{}  mean score {score:7.2}  highest {top:5.2} m  thread anchored {:4.1}% of ticks ({users} episodes more than 5%), longest {longest:.2} m, reeling work {work:6.1} J, first target at tick {first:.0} of {ticks}", level + 1, rows.len(), held * 100.0);
    }
}
