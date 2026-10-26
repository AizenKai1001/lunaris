//! Scores a brain on the trainer's task with the CPU port: `eval FILE GENOME LEVEL EPISODES [THREADS]`.
//! FILE is a champions file (GENOME picks the brain, 0 the best) or one genome (`setup`/`predict`/`learn`). Prints how many episodes solved the level (reached the targets it needs), the mean score,
//! the mean number of targets reached, the speed and its coefficient of variation. Flat ground, individual bodies (body seed = episode number), 720 ticks, 8 rounds, as the trainer does.
use lunaris_creature::episode::{level_of, run_episode, Cfg, EpisodeOut};
use lunaris_creature::{Brain, Flat, Model};
use lunaris_json::parse;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("usage: eval FILE GENOME LEVEL EPISODES [THREADS]");
        std::process::exit(2);
    }
    let text = std::fs::read_to_string(&a[1]).expect("file");
    let which: usize = a[2].parse().unwrap();
    let level: i32 = a[3].parse().unwrap();
    let n: usize = a[4].parse().unwrap();
    let threads: usize = a.get(5).map(|s| s.parse().unwrap()).unwrap_or(4);
    let doc = parse(&text).expect("json");
    let brain = if doc.get("genomes").is_some() { Brain::from_champions_json(&text).unwrap().remove(which) } else { Brain::from_genome_json(&doc) };
    let mut model = Model::spider();
    if let Ok(s) = std::env::var("EVAL_SWING_SCALE") {
        let k: f32 = s.parse().unwrap();
        for leg in 0..lunaris_creature::N_LEGS {
            model.tmax[2 + 6 * leg] *= k;
        }
    }
    if let Ok(s) = std::env::var("EVAL_SUB") {
        model.sub = s.parse().unwrap();
    }
    if let Ok(s) = std::env::var("EVAL_RELAX") {
        model.relax = s.parse().unwrap();
    }
    let rounds: i32 = std::env::var("EVAL_ROUNDS").ok().and_then(|s| s.parse().ok()).unwrap_or(3);
    let need = level_of(level).need as f32;
    let results: Vec<EpisodeOut> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..threads)
            .map(|t| {
                let (model, brain) = (&model, &brain);
                s.spawn(move || {
                    let mut out = Vec::new();
                    let mut i = t;
                    while i < n {
                        let cfg = Cfg { layout_seed: 1000 + i as u64, body_seed: 1 + i as u64, level };
                        out.push((i, run_episode(model, brain.clone(), &cfg, &Flat, 720, rounds, None)));
                        i += threads;
                    }
                    out
                })
            })
            .collect();
        let mut all: Vec<(usize, EpisodeOut)> = hs.into_iter().flat_map(|h| h.join().unwrap()).collect();
        all.sort_by_key(|x| x.0);
        all.into_iter().map(|x| x.1).collect()
    });
    let m = results.len() as f32;
    let solved = results.iter().filter(|r| r.reached >= need && r.broken == 0.0).count() as f32 / m;
    let mean = |f: fn(&EpisodeOut) -> f32| results.iter().map(f).sum::<f32>() / m;
    println!(
        "level {level}: solved {:.0}%  score {:.2}  reached {:.2}  last_dist {:.2}  speed {:.2} m/s  cv {:.2}  stand {:.2}  air {:.2}  broken {:.0}%  first_tick {:.0}",
        solved * 100.0,
        mean(|r| r.score),
        mean(|r| r.reached),
        mean(|r| r.last_dist),
        mean(|r| r.vmean),
        mean(|r| r.cv),
        mean(|r| r.stand),
        mean(|r| r.air),
        mean(|r| r.broken) * 100.0,
        mean(|r| r.first_tick)
    );
}
