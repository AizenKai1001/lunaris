//! What the wallhack defence costs a server tick, measured in memory on a real scene: `SURVIVAL_SCENE=path/to/survival.lunaris.json cargo test --release -p lunaris-net occlusion_bench -- --ignored --nocapture`.
//!
//! 200 players random-walk from the middle of the scene out over a 600 m map (the way the swarm's bots do: 0, 2, 5 or 8 m/s, a new heading every 0.8 to 3.3 s) and the server's own occlusion filter
//! ([`Sightings::filter`]) is run for every one of them on every tick, against the static world of the scene, as `SimGame` gathers it. The answers of a whole run are hashed: an optimisation that does not change
//! what is told to whom must not change the hash.
use super::interest::Candidate;
use super::occlusion::{Counters, Occlusion, Params, Sightings};
use super::server::{Ctx, Game, Outbox, World};
use super::simgame::SimGame;
use super::state::{class, EntityState};
use lunaris_api::Engine;
use lunaris_json::Json;
use std::cell::{Cell, RefCell};
use std::time::Instant;

type V3 = [f32; 3];

struct Rng(u64);

impl Rng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

struct Walker {
    pos: V3,
    heading: f32,
    speed: f32,
    next_turn: f32,
}

/// Loads the scene as the server does and runs a few ticks so that `NetHost` is read and the static world gathered.
fn game_of(scene: &str) -> SimGame {
    let path = std::path::Path::new(scene);
    let root = path.parent().and_then(|p| p.parent()).unwrap_or(std::path::Path::new(".")).to_path_buf();
    let rel = format!("scenes/{}", path.file_name().unwrap().to_string_lossy());
    let mut engine = Engine::new(1, root);
    engine.call("load_scene", &Json::obj([("path", Json::str(rel))])).expect("the scene loads");
    let mut game = SimGame::new(engine.sim);
    let mut world = World::default();
    let mut out = Outbox::default();
    for tick in 1..=3 {
        world.tick = tick;
        let mut ctx = Ctx { world: &mut world, out: &mut out, tick, tick_hz: 30 };
        game.on_tick(&mut ctx);
    }
    game
}

/// Records every ray that is asked and the processor's cycles spent answering, to be asked again alone.
struct Tape<'a> {
    inner: &'a SimGame,
    rays: RefCell<Vec<(V3, V3)>>,
    on: Cell<bool>,
    /// The cycles of the time stamp counter spent in `blocked`.
    spent: Cell<u64>,
}

#[cfg(target_arch = "x86_64")]
fn cycles() -> u64 {
    // SAFETY: reads the time stamp counter, which every x86-64 processor has.
    unsafe { std::arch::x86_64::_rdtsc() }
}

#[cfg(not(target_arch = "x86_64"))]
fn cycles() -> u64 {
    0
}

/// How many cycles of the time stamp counter go by in a nanosecond.
fn tsc_ghz() -> f64 {
    let (t, c) = (Instant::now(), cycles());
    while t.elapsed().as_millis() < 50 {}
    (cycles() - c) as f64 / t.elapsed().as_nanos() as f64
}

impl Game for Tape<'_> {
    fn on_join(&mut self, _w: &mut World, _slot: usize, _name: &str) -> EntityState {
        EntityState::default()
    }
    fn occlusion(&self) -> f32 {
        self.inner.occlusion()
    }
    fn blocked(&self, from: V3, to: V3) -> bool {
        if self.on.get() {
            self.rays.borrow_mut().push((from, to));
        }
        let t = cycles();
        let b = self.inner.blocked(from, to);
        self.spent.set(self.spent.get() + cycles() - t);
        b
    }
    fn level_version(&self) -> u32 {
        self.inner.level_version()
    }
    fn low_flags(&self) -> u16 {
        self.inner.low_flags()
    }
}

struct Run {
    ns_per_tick: f64,
    /// What `blocked` took of that (nanoseconds a tick), by the time stamp counter.
    rays_ns: f64,
    counters: Counters,
    hash: u64,
    told: u64,
}

fn play(scene: &str, bots: usize, ticks: u32, warm: u32, tape: Option<usize>) -> (Run, Vec<(V3, V3)>) {
    let game = game_of(scene);
    assert!(game.occlusion() > 0.0, "the scene has no occlusion");
    let tape_game = Tape { inner: &game, rays: RefCell::new(Vec::new()), on: Cell::new(false), spent: Cell::new(0) };
    let cfg = Occlusion::default();
    let per = cfg.rays_per_client;
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut walkers: Vec<Walker> = (0..bots).map(|_| Walker { pos: [(rng.f() - 0.5) * 60.0, 0.0, (rng.f() - 0.5) * 60.0], heading: rng.f() * 6.28, speed: 0.0, next_turn: 0.0 }).collect();
    let mut states: Vec<EntityState> = (0..bots).map(|i| EntityState { id: i as u32 + 1, class: class::PLAYER, health: 100, ..Default::default() }).collect();
    let mut seen: Vec<Sightings> = (0..bots).map(|_| Sightings::default()).collect();
    let mut budget: Vec<(u32, u32)> = vec![(per * 4, 0); bots];
    let mut cands: Vec<Candidate> = Vec::new();
    let (mut counters, mut hash, mut told) = (Counters::default(), 0xcbf2_9ce4_8422_2325u64, 0u64);
    let (mut spent, mut counted) = (0u128, 0u32);
    let dt = 1.0 / 30.0;
    for tick in 1..=ticks {
        let now = tick as f32 * dt;
        for (w, s) in walkers.iter_mut().zip(states.iter_mut()) {
            if now >= w.next_turn {
                w.heading += (rng.f() - 0.5) * 2.4;
                w.speed = [0.0, 2.0, 5.0, 8.0][(rng.f() * 3.99) as usize];
                w.next_turn = now + 0.8 + rng.f() * 2.5;
            }
            let (sn, cs) = w.heading.sin_cos();
            w.pos[0] += sn * w.speed * dt;
            w.pos[2] += cs * w.speed * dt;
            if w.pos[0].abs() > 300.0 || w.pos[2].abs() > 300.0 {
                w.pos[0] = w.pos[0].clamp(-300.0, 300.0);
                w.pos[2] = w.pos[2].clamp(-300.0, 300.0);
                w.heading += std::f32::consts::PI;
            }
            s.set_pos_m(w.pos);
            s.set_vel_ms([sn * w.speed, 0.0, cs * w.speed]);
        }
        let params = Params::new(&cfg, tick, 30, game.occlusion(), 35.0, 80.0, 16.0, game.level_version(), 0);
        tape_game.on.set(tape.is_some_and(|t| tick as usize >= t && tick as usize <= t + 30));
        if tick == warm {
            tape_game.spent.set(0);
        }
        let mut tick_ns = 0u128;
        for (i, own) in states.iter().enumerate() {
            cands.clear();
            let op = own.pos_m();
            for (j, s) in states.iter().enumerate() {
                if j == i {
                    continue;
                }
                let p = s.pos_m();
                let dist = ((p[0] - op[0]).powi(2) + (p[2] - op[2]).powi(2)).sqrt();
                if dist <= 150.0 {
                    cands.push(Candidate { idx: j as u32, id: s.id, dist, class: s.class });
                }
            }
            let (mut rays, last) = budget[i];
            rays = rays.saturating_add(per.saturating_mul(tick.wrapping_sub(last).min(8))).min(per.saturating_mul(4));
            let mut n = Counters::default();
            let t = Instant::now();
            seen[i].filter(&tape_game, &params, own, &states, &mut cands, &mut rays, &mut n);
            tick_ns += t.elapsed().as_nanos();
            budget[i] = (rays, tick);
            if tick > warm {
                counters.pairs += n.pairs;
                counters.rays += n.rays;
                counters.still += n.still;
                counters.hidden += n.hidden;
                counters.unaffordable += n.unaffordable;
                told += cands.len() as u64;
                for c in &cands {
                    hash = (hash ^ (tick as u64) << 32 ^ (i as u64) << 16 ^ c.id as u64).wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
        if tick > warm {
            spent += tick_ns;
            counted += 1;
        }
    }
    let rays = tape_game.rays.take();
    let rays_ns = tape_game.spent.get() as f64 / tsc_ghz() / counted.max(1) as f64;
    (Run { ns_per_tick: spent as f64 / counted.max(1) as f64, rays_ns, counters, hash, told }, rays)
}

fn report(name: &str, r: &Run, ticks: u32) {
    let c = &r.counters;
    let t = ticks as f64;
    println!(
        "{name}: {:.0} us a tick (the rays {:.0} us of it); a tick: pairs {:.0} rays {:.0} still {:.0} hidden {:.0} unaffordable {:.0} told {:.0}; answers hash {:016x}",
        r.ns_per_tick / 1000.0,
        r.rays_ns / 1000.0,
        c.pairs as f64 / t,
        c.rays as f64 / t,
        c.still as f64 / t,
        c.hidden as f64 / t,
        c.unaffordable as f64 / t,
        r.told as f64 / t,
        r.hash
    );
}

#[test]
#[ignore]
fn occlusion_bench_survival_scene() {
    let Ok(scene) = std::env::var("SURVIVAL_SCENE") else {
        println!("SURVIVAL_SCENE is not set: nothing to measure");
        return;
    };
    let bots: usize = std::env::var("BENCH_BOTS").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let (ticks, warm) = (1200u32, 600u32);
    let mut best: Option<Run> = None;
    let mut tape = Vec::new();
    for round in 0..3 {
        let (run, rays) = play(&scene, bots, ticks, warm, if round == 0 { Some(1000) } else { None });
        if round == 0 {
            tape = rays;
        }
        report(&format!("round {round}"), &run, ticks - warm);
        if best.as_ref().is_none_or(|b| run.ns_per_tick < b.ns_per_tick) {
            best = Some(run);
        }
    }
    report("best", &best.unwrap(), ticks - warm);
    // the rays alone: the ones of 30 ticks asked again in the same order
    let game = game_of(&scene);
    let mut blocked = 0usize;
    let t = Instant::now();
    for _ in 0..5 {
        blocked = 0;
        for (a, b) in &tape {
            if game.blocked(*a, *b) {
                blocked += 1;
            }
        }
    }
    let ns = t.elapsed().as_nanos() as f64 / 5.0;
    println!("rays alone: {} rays ({} blocked) took {:.0} ns each", tape.len(), blocked, ns / tape.len().max(1) as f64);
}

/// The visit of a pair that is known and not due, on its own: 200 players standing still, the answers found in a first tick and then asked for again and again in the same tick (nothing is due, nobody has moved).
#[test]
#[ignore]
fn occlusion_bench_cached_visits() {
    let Ok(scene) = std::env::var("SURVIVAL_SCENE") else { return };
    let game = game_of(&scene);
    let bots = 200usize;
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let states: Vec<EntityState> = (0..bots)
        .map(|i| {
            let mut s = EntityState { id: i as u32 + 1, class: class::PLAYER, health: 100, ..Default::default() };
            s.set_pos_m([(rng.f() - 0.5) * 100.0, 0.0, (rng.f() - 0.5) * 100.0]);
            s
        })
        .collect();
    let cfg = Occlusion::default();
    let params = Params::new(&cfg, 100, 30, game.occlusion(), 35.0, 80.0, 16.0, game.level_version(), 0);
    let mut seen: Vec<Sightings> = (0..bots).map(|_| Sightings::default()).collect();
    let mut cands: Vec<Candidate> = Vec::new();
    let build = |i: usize, cands: &mut Vec<Candidate>| {
        cands.clear();
        let op = states[i].pos_m();
        for (j, s) in states.iter().enumerate() {
            if j != i {
                let p = s.pos_m();
                cands.push(Candidate { idx: j as u32, id: s.id, dist: ((p[0] - op[0]).powi(2) + (p[2] - op[2]).powi(2)).sqrt(), class: s.class });
            }
        }
    };
    // (a first tick with all the rays there are, so that every pair is known)
    for i in 0..bots {
        build(i, &mut cands);
        let (mut rays, mut n) = (u32::MAX, Counters::default());
        seen[i].filter(&game, &params, &states[i], &states, &mut cands, &mut rays, &mut n);
    }
    // (the same tick again and again: every pair is known, nothing is due, nobody has moved: the pairs within the safe radius are written again, the others looked up)
    let rounds = 40;
    let mut kept = 0usize;
    let (mut best, mut best_base) = (f64::MAX, f64::MAX);
    for _ in 0..7 {
        let t = Instant::now();
        for _ in 0..rounds {
            for i in 0..bots {
                build(i, &mut cands);
                let (mut rays, mut n) = (0u32, Counters::default());
                seen[i].filter(&game, &params, &states[i], &states, &mut cands, &mut rays, &mut n);
                kept += cands.len();
            }
        }
        best = best.min(t.elapsed().as_nanos() as f64);
        // (the same without the filter: what building the candidates costs)
        let t = Instant::now();
        for _ in 0..rounds {
            for i in 0..bots {
                build(i, &mut cands);
                kept += cands.len() & 1;
            }
        }
        best_base = best_base.min(t.elapsed().as_nanos() as f64);
    }
    let visits = (rounds * bots * (bots - 1)) as f64;
    println!("cached visits: {:.1} ns each (the candidates cost {:.1} ns each of that); {} kept", (best - best_base) / visits, best_base / visits, kept);
}

/// The game's part of a server tick (`SimGame::on_tick`: the players' input onto their avatars, two steps of the simulation, what the players are told, the world replicated) with 200 players in the scene, without the network:
/// `SURVIVAL_SCENE=... cargo test --release -p lunaris-net game_phase_bench -- --ignored --nocapture`.
#[test]
#[ignore]
fn game_phase_bench_survival_scene() {
    let Ok(scene) = std::env::var("SURVIVAL_SCENE") else { return };
    let bots: usize = std::env::var("BENCH_BOTS").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let mut best = (f64::MAX, 0.0, 0.0, 0.0, 0.0);
    for _round in 0..3 {
        // (the world is not read back from the save of the round before: every round starts from the scene)
        let root = std::path::Path::new(&scene).parent().and_then(|p| p.parent()).unwrap().to_path_buf();
        let _ = std::fs::remove_file(root.join("saves/survival_world.json"));
        let mut game = game_of(&scene);
        if std::env::var("BENCH_SIMPROFILE").is_ok() {
            game.sim.profile = Some(Default::default());
        }
        let mut world = World::default();
        let mut out = Outbox::default();
        let mut rng = Rng(0x5EED_1234_ABCD_EF01);
        // the players join as the server lets them: the game makes their character, the server gives it an id and an owner
        let mut ids = Vec::new();
        for slot in 0..bots {
            let st = game.on_join(&mut world, slot, &format!("bot{slot}"));
            let id = world.spawn_state(st);
            world.entities.get_mut(&id).unwrap().owner = Some(slot);
            ids.push(id);
        }
        let mut walkers: Vec<Walker> = ids.iter().map(|id| Walker { pos: world.get(*id).unwrap().state.pos_m(), heading: rng.f() * 6.28, speed: 0.0, next_turn: 0.0 }).collect();
        let (warm, ticks) = (150u32, 450u32);
        let (mut total, mut input, mut step, mut mirror) = (0u128, 0u64, 0u64, 0u64);
        let mut worst = 0u128;
        for tick in 1..=ticks {
            let now = tick as f32 / 30.0;
            for (w, id) in walkers.iter_mut().zip(&ids) {
                if now >= w.next_turn {
                    w.heading += (rng.f() - 0.5) * 2.4;
                    w.speed = [0.0, 2.0, 5.0, 8.0][(rng.f() * 3.99) as usize];
                    w.next_turn = now + 0.8 + rng.f() * 2.5;
                }
                let (sn, cs) = w.heading.sin_cos();
                w.pos[0] += sn * w.speed / 30.0;
                w.pos[2] += cs * w.speed / 30.0;
                if let Some(e) = world.entities.get_mut(id) {
                    e.state.set_pos_m(w.pos);
                    e.state.set_vel_ms([sn * w.speed, 0.0, cs * w.speed]);
                }
            }
            world.tick = tick + 3;
            let before = (game.stats.input_us, game.stats.step_us, game.stats.mirror_us);
            let t = Instant::now();
            {
                let mut ctx = Ctx { world: &mut world, out: &mut out, tick: tick + 3, tick_hz: 30 };
                game.on_tick(&mut ctx);
            }
            let us = t.elapsed().as_nanos();
            out = Outbox::default();
            if tick > warm {
                total += us;
                worst = worst.max(us);
                input += game.stats.input_us - before.0;
                step += game.stats.step_us - before.1;
                mirror += game.stats.mirror_us - before.2;
            }
        }
        // the periodic save, started on the tick's own thread as the server does: what holds the tick up is the copy of the world (the writing is on a thread of its own, waited for here)
        let entities = game.sim.world.len();
        let mut held = Vec::new();
        for _ in 0..5 {
            let t = Instant::now();
            game.save_in_background();
            held.push(t.elapsed().as_micros());
            game.save_now();
        }
        held.sort_unstable();
        println!("starting a save held the tick up {} us (the median of 5; {} entities with the {} avatars)", held[2], entities, bots);
        // (what of that is the copy of the simulation alone, and of that the world's and the scripts')
        let mut copy_us = Vec::new();
        let mut world_us = Vec::new();
        for _ in 0..5 {
            let t = Instant::now();
            let c = game.sim.clone();
            copy_us.push(t.elapsed().as_micros());
            drop(c);
            let t = Instant::now();
            let w = game.sim.world.clone();
            world_us.push(t.elapsed().as_micros());
            drop(w);
        }
        copy_us.sort_unstable();
        world_us.sort_unstable();
        println!("the copy of the simulation alone takes {} us, of the world alone {} us", copy_us[2], world_us[2]);
        let time = |f: &dyn Fn()| {
            let mut v: Vec<u128> = (0..5)
                .map(|_| {
                    let t = Instant::now();
                    f();
                    t.elapsed().as_micros()
                })
                .collect();
            v.sort_unstable();
            v[2]
        };
        println!(
            "  of the rest: the scripts {} us, their prefabs {} us, the nav {} us, the events {} us, the contacts {} us",
            time(&|| drop(game.sim.scripts.clone())),
            time(&|| drop(game.sim.scripts.prefabs.clone())),
            time(&|| drop(game.sim.nav.clone())),
            time(&|| drop(game.sim.events.clone())),
            time(&|| drop(game.sim.contacts.clone()))
        );
        let n = (ticks - warm) as f64;
        let run = (total as f64 / n / 1000.0, input as f64 / n, step as f64 / n, mirror as f64 / n, worst as f64 / 1.0e6);
        println!("game phase: {:.0} us a tick (input {:.0}, simulation {:.0}, replication {:.0}), the slowest tick {:.1} ms; the state of the simulation after {} ticks: {:016x}", run.0, run.1, run.2, run.3, run.4, ticks, game.sim.state_hash());
        if let Some(p) = &game.sim.profile {
            let mut rows: Vec<_> = p.stages.iter().filter(|(n, _)| !n.contains("(all)")).collect();
            rows.sort_by(|a, b| b.1.total_ms.total_cmp(&a.1.total_ms));
            for (n, t) in rows.iter().take(22) {
                println!("    {:7.4} ms a step {:7.3} max  {}", t.total_ms / p.ticks.max(1) as f64, t.max_ms, n);
            }
        }
        if run.0 < best.0 {
            best = run;
        }
    }
    println!("game phase, best: {:.0} us a tick (input {:.0}, simulation {:.0}, replication {:.0}), the slowest tick {:.1} ms", best.0, best.1, best.2, best.3, best.4);
}
