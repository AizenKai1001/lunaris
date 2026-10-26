//! Lunaris ECS against other Rust ECS crates, on the workloads of the public `ecs_bench_suite` (rust-gamedev, archived, pinned to 2021 crate versions):
//! simple_insert, simple_iter, frag_iter, add_remove. Same data sizes, current crate versions, one timer for everyone, one thread.
//!
//!     cargo run --release --features all            (every adapter)
//!     cargo run --release --features hecs -- --only hecs --only lunaris
//!     cargo run --release -- --json out.json
//!
//! Each number is the median of 15 samples (a sample runs the workload enough times to take about 20 ms); the minimum is printed too.
mod floor;
mod lunaris;
#[cfg(feature = "hecs")]
mod hecs_adapter;
#[cfg(feature = "bevy")]
mod bevy_adapter;
#[cfg(feature = "legion")]
mod legion_adapter;
#[cfg(feature = "shipyard")]
mod shipyard_adapter;
#[cfg(feature = "specs")]
mod specs_adapter;

use std::time::Instant;

pub struct Row {
    pub engine: &'static str,
    pub bench: &'static str,
    /// Entities touched per run of the workload (to turn the time into ns per entity).
    pub n: usize,
    pub median_ns: f64,
    pub min_ns: f64,
}

/// Time `f`: three warm-up runs, then 15 samples, each of enough runs to take about 20 ms. Returns (median, min) in ns per run.
pub fn measure<F: FnMut()>(mut f: F) -> (f64, f64) {
    for _ in 0..3 {
        f();
    }
    let t = Instant::now();
    f();
    let one = t.elapsed().as_secs_f64();
    let iters = ((0.02 / one.max(1e-9)).ceil() as u64).clamp(1, 200_000);
    let mut samples: Vec<f64> = (0..15)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..iters {
                f();
            }
            t.elapsed().as_secs_f64() * 1e9 / iters as f64
        })
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (samples[samples.len() / 2], samples[0])
}

pub fn push(rows: &mut Vec<Row>, engine: &'static str, bench: &'static str, n: usize, (median_ns, min_ns): (f64, f64)) {
    rows.push(Row { engine, bench, n, median_ns, min_ns });
}

pub const INSERT_N: usize = 10_000;
pub const ITER_N: usize = 10_000;
pub const ITER_BIG_N: usize = 1_000_000;
pub const FRAG_TYPES: usize = 26;
pub const FRAG_PER: usize = 20;
pub const ADD_REMOVE_N: usize = 10_000;

fn main() {
    // glibc serves big allocations with mmap (fresh pages: every first touch is a page fault) until the first free of a big block teaches it
    // to use the heap instead. A game that has been running for a second is in the second state; without this line the result of the first
    // benchmark depends on whether something else happened to free a big block before it. Same for every engine here. The block must be at most 32 MiB:
    // glibc raises its mmap threshold only for a freed block up to that size (`DEFAULT_MMAP_THRESHOLD_MAX`), so a bigger one (33 MiB was used first) teaches it nothing and every
    // benchmark that allocates a few hundred kilobytes keeps paying for fresh pages (62 ns an entity to spawn instead of 24 for Lunaris on 2026-10-25).
    drop(std::hint::black_box(vec![1u8; 24 << 20]));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut only: Vec<String> = Vec::new();
    let mut json: Option<String> = None;
    let mut memory: Option<usize> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--only" => {
                i += 1;
                only.push(args[i].clone());
            }
            "--json" => {
                i += 1;
                json = Some(args[i].clone());
            }
            "--memory" => {
                i += 1;
                memory = Some(args[i].parse().expect("--memory takes a number of entities"));
            }
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    let want = |name: &str| only.is_empty() || only.iter().any(|o| o == name);
    if let Some(n) = memory {
        memory_mode(n, &only);
        return;
    }

    let mut rows: Vec<Row> = Vec::new();
    if want("floor") {
        floor::run(&mut rows);
    }
    if want("lunaris") {
        lunaris::run(&mut rows);
    }
    #[cfg(feature = "hecs")]
    if want("hecs") {
        hecs_adapter::run(&mut rows);
    }
    #[cfg(feature = "bevy")]
    if want("bevy_ecs") {
        bevy_adapter::run(&mut rows);
    }
    #[cfg(feature = "legion")]
    if want("legion") {
        legion_adapter::run(&mut rows);
    }
    #[cfg(feature = "shipyard")]
    if want("shipyard") {
        shipyard_adapter::run(&mut rows);
    }
    #[cfg(feature = "specs")]
    if want("specs") {
        specs_adapter::run(&mut rows);
    }
    print_table(&rows);
    if let Some(path) = json {
        let mut out = String::from("[\n");
        for (k, r) in rows.iter().enumerate() {
            out.push_str(&format!("  {{\"engine\":\"{}\",\"bench\":\"{}\",\"n\":{},\"median_ns\":{:.1},\"min_ns\":{:.1}}}{}\n", r.engine, r.bench, r.n, r.median_ns, r.min_ns, if k + 1 < rows.len() { "," } else { "" }));
        }
        out.push_str("]\n");
        std::fs::write(&path, out).expect("write json");
        println!("wrote {path}");
    }
}

/// Resident memory of this process in KB (Linux `VmRSS`).
fn resident_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find(|l| l.starts_with("VmRSS:"))?.split_whitespace().nth(1)?.parse().ok()
}

const ENGINES: [&str; 6] = ["lunaris", "hecs", "bevy_ecs", "legion", "shipyard", "specs"];

/// Bytes of RAM an entity costs with the four components of simple_insert (64 + 12 + 12 + 12 = 100 bytes of data): one engine at a time, each in a process of its own so that
/// what another crate left in the allocator does not count (`--memory N --only NAME`; without `--only`, every engine that was built, one process each). Linux only.
fn memory_mode(n: usize, only: &[String]) {
    if only.len() != 1 {
        let exe = std::env::current_exe().expect("own path");
        println!("memory: resident growth of a world of {n} entities with the four components of simple_insert (100 bytes of data each), one process per engine");
        for name in ENGINES.iter().filter(|e| only.is_empty() || only.iter().any(|o| o == *e)) {
            let _ = std::process::Command::new(&exe).args(["--memory", &n.to_string(), "--only", name]).status();
        }
        return;
    }
    let name = only[0].as_str();
    let before = resident_kb();
    let world: Option<Box<dyn std::any::Any>> = match name {
        "lunaris" => Some(lunaris::hold(n)),
        #[cfg(feature = "hecs")]
        "hecs" => Some(hecs_adapter::hold(n)),
        #[cfg(feature = "bevy")]
        "bevy_ecs" => Some(bevy_adapter::hold(n)),
        #[cfg(feature = "legion")]
        "legion" => Some(legion_adapter::hold(n)),
        #[cfg(feature = "shipyard")]
        "shipyard" => Some(shipyard_adapter::hold(n)),
        #[cfg(feature = "specs")]
        "specs" => Some(specs_adapter::hold(n)),
        _ => None,
    };
    let after = resident_kb();
    match (world, before, after) {
        (Some(w), Some(a), Some(b)) => {
            println!("  {:<10} {:>7.1} MB resident = {:>5.0} bytes/entity", name, (b - a) as f64 / 1024.0, (b - a) as f64 * 1024.0 / n as f64);
            drop(std::hint::black_box(w));
        }
        (None, ..) => println!("  {name:<10} not built in (see --features)"),
        _ => println!("  {name:<10} the platform does not report resident memory (Linux only)"),
    }
}

fn print_table(rows: &[Row]) {
    let mut benches: Vec<&'static str> = Vec::new();
    for r in rows {
        if !benches.contains(&r.bench) {
            benches.push(r.bench);
        }
    }
    println!("ecs_compare: ns per entity, median of 15 samples (min in brackets); lower is better; one thread");
    for b in benches {
        println!("\n{b}");
        let mut group: Vec<&Row> = rows.iter().filter(|r| r.bench == b).collect();
        group.sort_by(|a, b| a.median_ns.partial_cmp(&b.median_ns).unwrap());
        let best = group.first().map(|r| r.median_ns).unwrap_or(1.0);
        for r in group {
            println!("  {:<10} {:>9.2} ns/entity  ({:>9.2})   {:>7.1} us per run   {:>5.2}x the fastest", r.engine, r.median_ns / r.n as f64, r.min_ns / r.n as f64, r.median_ns / 1000.0, r.median_ns / best);
        }
    }
}
