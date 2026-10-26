//! The wallhack defence (occlusion.rs, docs/NETWORK_SCALE.md "Wallhacks") played in memory: players with a wall between them, a server whose `Game` knows where the wall is. No art: boxes and numbers.
//!
//! The wall is 2 m thick (x from -1 to 1), 60 m long (z from -30 to 30) and 8 m high, so a player at x = -20 sees a player at x = 20 only when the line between them passes beyond one end of it.
use super::client::NetClient;
use super::server::{Ctx, Game, LevelChange, NetServer, ServerConfig, World};
use super::state::{class, EntityState};
use super::transport::{Link, SimEndpoint, SimNet};
use std::cell::Cell;

type V3 = [f32; 3];

#[derive(Clone, Copy)]
struct Wall {
    lo: V3,
    hi: V3,
}

/// A game whose level is a few boxes, and who starts where the table says.
struct Walled {
    safe: f32,
    walls: Vec<Wall>,
    starts: Vec<(&'static str, V3)>,
    rays: Cell<u64>,
    version: u32,
    low: u16,
    /// The milliseconds every tick of the game takes (it waits that long): a game that is too slow for its server.
    delay_ms: u64,
    /// What the last change of the level touched, when the game says (a test that changes `walls` and `version` sets it; the default is "not known").
    change: LevelChange,
}

impl Walled {
    fn new(safe: f32, starts: &[(&'static str, V3)]) -> Walled {
        Walled { safe, walls: vec![Wall { lo: [-1.0, 0.0, -30.0], hi: [1.0, 8.0, 30.0] }], starts: starts.to_vec(), rays: Cell::new(0), version: 1, low: 0, delay_ms: 0, change: LevelChange::default() }
    }
}

fn hits(a: V3, b: V3, w: &Wall) -> bool {
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for k in 0..3 {
        let d = b[k] - a[k];
        if d.abs() < 1.0e-9 {
            if a[k] < w.lo[k] || a[k] > w.hi[k] {
                return false;
            }
        } else {
            let (mut u, mut v) = ((w.lo[k] - a[k]) / d, (w.hi[k] - a[k]) / d);
            if u > v {
                std::mem::swap(&mut u, &mut v);
            }
            t0 = t0.max(u);
            t1 = t1.min(v);
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

impl Game for Walled {
    fn on_join(&mut self, _w: &mut World, _slot: usize, name: &str) -> EntityState {
        let mut s = EntityState { class: class::PLAYER, health: 100, ..Default::default() };
        s.set_pos_m(self.starts.iter().find(|(n, _)| *n == name).map_or([0.0; 3], |(_, p)| *p));
        s
    }
    fn on_tick(&mut self, _ctx: &mut Ctx) {
        if self.delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(self.delay_ms));
        }
    }
    fn occlusion(&self) -> f32 {
        self.safe
    }
    fn blocked(&self, from: V3, to: V3) -> bool {
        self.rays.set(self.rays.get() + 1);
        self.walls.iter().any(|w| hits(from, to, w))
    }
    fn level_version(&self) -> u32 {
        self.version
    }
    fn level_change(&self) -> LevelChange {
        self.change.clone()
    }
    fn low_flags(&self) -> u16 {
        self.low
    }
}

/// What the test knows, whatever the server says: could a player at `a` see one at `b` (standing, any of head, chest and feet)?
fn truth(walls: &[Wall], a: V3, b: V3) -> bool {
    let eye = [a[0], a[1] + 1.6, a[2]];
    [1.7, 1.1, 0.25].iter().any(|h| !walls.iter().any(|w| hits(eye, [b[0], b[1] + h, b[2]], w)))
}

struct Arena {
    server: NetServer<SimEndpoint, Walled>,
    clients: Vec<NetClient<SimEndpoint>>,
    net: SimNet,
    now: u64,
}

impl Arena {
    fn new(game: Walled, cfg: ServerConfig, names: &[&str]) -> Arena {
        let net = SimNet::new(Link { latency_ms: 20, jitter_ms: 0, loss: 0.0, duplicate: 0.0 }, 11);
        let ep = net.endpoint();
        let addr = ep.addr();
        let server = NetServer::new(cfg, ep, game);
        let clients = names.iter().enumerate().map(|(i, n)| NetClient::new(net.endpoint(), addr, n, 100 + i as u32)).collect();
        Arena { server, clients, net, now: 1000 }
    }

    /// Runs `ms` milliseconds. `drive(client, t)` says where client `client` is and how it moves `t` ms into this run; `each(arena, t)` is called after every step.
    fn run(&mut self, ms: u64, drive: &dyn Fn(usize, u64) -> (V3, V3), mut each: impl FnMut(&Arena, u64)) {
        let t0 = self.now;
        while self.now < t0 + ms {
            self.now += 5;
            self.net.set_now(self.now);
            self.server.update(self.now);
            for i in 0..self.clients.len() {
                self.clients[i].update(self.now);
                if self.clients[i].connected() {
                    let (p, v) = drive(i, self.now - t0);
                    let mut s = EntityState { class: class::PLAYER, health: 100, ..Default::default() };
                    s.set_pos_m(p);
                    s.set_vel_ms(v);
                    self.clients[i].set_state(s);
                }
            }
            each(self, self.now - t0);
        }
    }

    fn knows(&self, who: usize, whom: usize) -> bool {
        self.clients[who].remotes().contains_key(&self.clients[whom].entity)
    }

    /// The server tick of the newest record client `who` holds of client `whom` (0: none).
    fn last_tick(&self, who: usize, whom: usize) -> u32 {
        self.clients[who].remotes().get(&self.clients[whom].entity).map_or(0, |r| r.last_tick())
    }
}

fn still(p: V3) -> (V3, V3) {
    (p, [0.0; 3])
}

const A: V3 = [-20.0, 0.0, 0.0];
const B: V3 = [20.0, 0.0, 0.0];

#[test]
fn a_player_behind_a_wall_beyond_the_safe_radius_is_never_sent_and_what_is_not_a_player_is() {
    // (the things that are not players: loot, a projectile and a machine)
    let cfg = ServerConfig { max_clients: 4, ..Default::default() };
    let mut off = Arena::new(Walled::new(0.0, &[("a", A), ("b", B)]), cfg.clone(), &["a", "b"]);
    let mut on = Arena::new(Walled::new(16.0, &[("a", A), ("b", B)]), cfg, &["a", "b"]);
    // a thing on the ground, a thing in flight and a machine (class 4) behind the wall too
    on.server.world.spawn(class::ITEM, [22.0, 0.0, 3.0]);
    on.server.world.spawn(class::PROJECTILE, [22.0, 1.0, -3.0]);
    on.server.world.spawn(4, [24.0, 0.0, 0.0]);
    let drive = |i: usize, _t: u64| still(if i == 0 { A } else { B });
    let mut seen_by_on = 0u32;
    off.run(4000, &drive, |_, _| {});
    on.run(4000, &drive, |m, _| {
        if m.knows(0, 1) || m.knows(1, 0) {
            seen_by_on += 1;
        }
    });
    assert!(off.knows(0, 1) && off.knows(1, 0), "with the defence off they see each other through the wall: that is the wallhack");
    assert_eq!(seen_by_on, 0, "with it on neither ever has a record of the other, in any of 800 steps");
    assert!(on.server.stats.occ_hidden > 0 && on.server.stats.occ_pairs > 0, "{:?}", on.server.stats);
    let loot = on.clients[0].remotes().values().filter(|r| r.latest().is_some_and(|l| l.class == class::ITEM)).count();
    assert_eq!(loot, 1, "loot is not hidden by the defence");
    let others: Vec<u8> = on.clients[0].remotes().values().filter_map(|r| r.latest().map(|l| l.class)).filter(|&c| c == class::PROJECTILE || c == 4).collect();
    assert_eq!(others.len(), 2, "by default only players hide behind walls: {others:?}");
}

#[test]
fn inside_the_safe_radius_a_player_is_always_sent_whatever_is_in_the_way() {
    let near_a = [-5.0, 0.0, 0.0];
    let near_b = [5.0, 0.0, 0.0];
    let mut m = Arena::new(Walled::new(16.0, &[("a", near_a), ("b", near_b)]), ServerConfig { max_clients: 4, ..Default::default() }, &["a", "b"]);
    m.run(3000, &|i, _| still(if i == 0 { near_a } else { near_b }), |_, _| {});
    assert!(m.knows(0, 1) && m.knows(1, 0), "ten metres apart with a wall between them, in a world with a safe radius of 16 m");
    // ... and one step beyond the radius they stop being told (the wall is in the way)
    let mut far = Arena::new(Walled::new(8.0, &[("a", near_a), ("b", near_b)]), ServerConfig { max_clients: 4, ..Default::default() }, &["a", "b"]);
    far.run(4000, &|i, _| still(if i == 0 { near_a } else { near_b }), |_, _| {});
    assert!(!far.knows(0, 1) && !far.knows(1, 0), "ten metres apart, safe radius 8 m: hidden");
}

/// One player walks along the wall at 8 m/s from behind it (z = 20 + `shift`) to z = 110 while the other stands: the line opens when the walker is at z = 63. With `observer_walks` it is the player who is shown
/// others who walks (along x = -20) and the one he comes to see who stands. Returns how many milliseconds before the line opened the watcher had its first record of the target, and how old the newest was when it opened.
fn the_lead(cfg: ServerConfig, safe: f32, observer_walks: bool, shift: f32, wait: u64) -> (i64, i64) {
    let start = |walker: bool| if walker { [if observer_walks { -20.0 } else { 20.0 }, 0.0, 20.0 + shift] } else { [if observer_walks { 20.0 } else { -20.0 }, 0.0, 0.0] };
    // (client 0 is the watcher, client 1 the target: the walker is client 0 or 1)
    let (watcher, target) = (0usize, 1usize);
    let walker = if observer_walks { watcher } else { target };
    let game = Walled::new(safe, &[("a", start(walker == 0)), ("b", start(walker == 1))]);
    let walls = game.walls.clone();
    let mut m = Arena::new(game, cfg, &["a", "b"]);
    let place = move |i: usize, t: u64| -> (V3, V3) {
        let mut p = start(i == walker);
        if i == walker && t > wait {
            // (it stands where it is for `wait` ms, then runs)
            p[2] += 8.0 * (t - wait) as f32 / 1000.0;
            (p, [0.0, 0.0, 8.0])
        } else if i == walker {
            (p, [0.0; 3])
        } else {
            still(p)
        }
    };
    let (mut first, mut opened, mut age) = (None, None, i64::MAX);
    m.run(9000 + wait, &place, |a, t| {
        if opened.is_none() && truth(&walls, place(watcher, t).0, place(target, t).0) {
            opened = Some(t as i64);
            let last = a.last_tick(watcher, target);
            age = if last == 0 { i64::MAX } else { (a.server.world.tick - last) as i64 * 1000 / 30 };
        }
        if first.is_none() && a.knows(watcher, target) {
            first = Some(t as i64);
        }
    });
    let opened = opened.expect("the line opens");
    (opened - first.expect("the watcher comes to know the target"), age)
}

#[test]
fn walking_round_the_wall_the_player_is_sent_before_the_line_opens_and_never_pops_in_late_at_a_run() {
    let cfg = ServerConfig { max_clients: 4, ..Default::default() };
    // a client shows others 100 ms in the past: to have the player in the picture on the tick the line opens it needs records from before that, and a tick or two to interpolate between
    let interp = 100 + 33;
    for observer_walks in [false, true] {
        let (mut least, mut most, mut oldest) = (i64::MAX, 0, 0);
        // (the walker starts a little further on each time, so that the line opens at every phase of the tests that are made every few ticks)
        for shift in [0.0, 0.7, 1.4, 2.1, 2.9, 3.6, 4.3, 5.0, 5.8, 6.5] {
            let (lead, age) = the_lead(cfg.clone(), 16.0, observer_walks, shift, 0);
            (least, most, oldest) = (least.min(lead), most.max(lead), oldest.max(age));
        }
        println!("occlusion look-ahead, {} walks at 8 m/s: the first record came {least} to {most} ms before the line opened, the newest was at most {oldest} ms old when it did", if observer_walks { "the watcher" } else { "the target" });
        assert!(least >= interp, "a record came {least} ms before the line opened: that is late (a client shows others {interp} ms behind)");
        assert!(most <= 1000, "a record came {most} ms before the line opened: that is a wallhack's gift of 8 metres");
        assert!(oldest <= 200, "the newest record is {oldest} ms old at the moment the line opens");
    }
    // without the look-ahead (the same runs with `look_ms` 0) it is late: the proof that the test can fail
    let mut late = cfg.clone();
    late.occlusion.look_ms = 0;
    let least = [0.0, 1.4, 2.9, 4.3].iter().map(|&shift| the_lead(late.clone(), 16.0, false, shift, 0).0).min().unwrap();
    println!("occlusion without look-ahead: the first record came {least} ms before the line opened at the worst");
    assert!(least < interp, "{least}");
}

/// B stands in sight of A at z = 66 (the line from A passes the end of the wall) and steps behind the wall to z = 61 at `3000 + phase` ms, for `back` milliseconds. Returns the longest time (milliseconds) A went
/// without a new record of B from 2 s on, until 1.5 s after B came back.
fn longest_gap(cfg: ServerConfig, back: u64, phase: u64) -> i64 {
    let mut m = Arena::new(Walled::new(16.0, &[("a", [-20.0, 0.0, 0.0]), ("b", [20.0, 0.0, 66.0])]), cfg, &["a", "b"]);
    let drive = move |i: usize, t: u64| -> (V3, V3) {
        if i == 0 {
            return still([-20.0, 0.0, 0.0]);
        }
        still([20.0, 0.0, if (3000 + phase..3000 + phase + back).contains(&t) { 61.0 } else { 66.0 }])
    };
    let end = 3000 + phase + back + 1500;
    let (mut gap, mut prev, mut changed) = (0i64, 0u32, 0i64);
    m.run(end, &drive, |a, t| {
        let t = t as i64;
        let k = a.last_tick(0, 1);
        if k != prev {
            if prev != 0 && t >= 2000 {
                gap = gap.max(t - changed);
            }
            prev = k;
            changed = t;
        }
    });
    gap.max(end as i64 - changed)
}

/// The longest gap over ducks that begin at different moments of the tests that are made every few ticks.
fn worst_gap(cfg: &ServerConfig, back: u64) -> i64 {
    (0..12).map(|k| longest_gap(cfg.clone(), back, k * 30)).max().unwrap()
}

#[test]
fn a_player_who_stood_still_behind_the_wall_and_then_runs_is_sent_in_time() {
    // it has stood still for 3 s (the pair keeps its answer without a ray for 0.6 s at a time) 3 to 5 m short of where the line opens, and runs at 8 m/s: the line opens half a second later
    let cfg = ServerConfig { max_clients: 4, ..Default::default() };
    let (mut least, mut most) = (i64::MAX, i64::MIN);
    for observer_walks in [false, true] {
        for shift in [37.0, 37.9, 38.8, 39.6, 40.5] {
            for wait in [3000, 3070, 3140, 3210, 3280, 3350] {
                let (lead, _) = the_lead(cfg.clone(), 16.0, observer_walks, shift, wait);
                (least, most) = (least.min(lead), most.max(lead));
            }
        }
    }
    println!("occlusion, standing still and then running: the first record came {least} to {most} ms before the line opened");
    assert!(least >= 133, "a record came {least} ms before the line opened: that is late");
}

#[test]
fn a_player_the_game_puts_somewhere_else_is_looked_at_again_at_once() {
    // B has stood behind the wall for 3 s (the pair keeps its answer without a ray); the game puts it (a respawn) 65 m up the wall, in sight (76 m from A, in the mid tier: the far tier has a schedule of its own, 3 a second, for anything new): A is told within a fifth of a second, not when the pair would have been looked at
    let mut m = Arena::new(Walled::new(16.0, &[("a", A), ("b", B)]), ServerConfig { max_clients: 4, ..Default::default() }, &["a", "b"]);
    let drive = |i: usize, _t: u64| still(if i == 0 { A } else { B });
    m.run(3000, &drive, |_, _| {});
    assert!(!m.knows(0, 1));
    let id = m.clients[1].entity;
    m.server.world.get_mut(id).unwrap().state.set_pos_m([20.0, 0.0, 65.0]);
    let mut told_at = None;
    // (the client of B goes on saying it stands at the old place: the server refuses it as a jump of 65 m, and keeps B where the game put it)
    m.run(600, &drive, |a, t| {
        if told_at.is_none() && a.knows(0, 1) {
            told_at = Some(t);
        }
    });
    let told_at = told_at.expect("A is told about B");
    println!("occlusion: a player put somewhere else was told {told_at} ms later");
    assert!(told_at <= 200, "told {told_at} ms after the game put B in sight");
}

#[test]
fn a_player_who_ducks_behind_the_wall_for_a_moment_stays_known_and_one_who_stays_there_is_dropped() {
    let cfg = ServerConfig { max_clients: 4, ..Default::default() };
    // in sight and not ducking at all: the records come ten times a second (mid distance)
    let steady = worst_gap(&cfg, 0);
    println!("occlusion hysteresis: the longest wait for a record of a player in sight is {steady} ms");
    assert!(steady <= 150, "{steady}");
    // a duck of 0.2 s, begun at any moment of the retest period: no gap in the records
    let gap = worst_gap(&cfg, 200);
    let longer = worst_gap(&cfg, 400);
    println!("occlusion hysteresis: ducking behind the wall for 200 ms, the longest wait for a record is {gap} ms (for 400 ms: {longer} ms)");
    assert!(gap <= 150, "behind the wall for 200 ms the records went on: waited {gap} ms");
    let gone = worst_gap(&cfg, 3000);
    println!("occlusion hysteresis: three seconds behind the wall: the longest wait is {gone} ms");
    assert!(gone >= 1500, "three seconds behind the wall: the records stop after the hold ({gone} ms)");
    // with no hold (and no look-ahead) a duck of 0.2 s cuts the records: the test can fail
    let mut none = cfg;
    none.occlusion.hold_ms = 0;
    none.occlusion.look_ms = 0;
    let gap = worst_gap(&none, 200);
    println!("occlusion hysteresis: without the hold, 200 ms behind the wall: the longest wait is {gap} ms");
    assert!(gap >= 200, "{gap}");
}

#[test]
fn a_crouched_player_behind_low_cover_is_hidden_and_a_standing_one_is_not() {
    // a wall 1.5 m high: a standing player's head (1.7 m) is over it, a crouched player's is not
    let mk = |low: u16| {
        let mut g = Walled::new(16.0, &[("a", A), ("b", B)]);
        g.walls = vec![Wall { lo: [-1.0, 0.0, -30.0], hi: [1.0, 1.5, 30.0] }];
        g.low = low;
        g
    };
    let run = |flags: u16, low: u16| {
        let mut m = Arena::new(mk(low), ServerConfig { max_clients: 4, ..Default::default() }, &["a", "b"]);
        // (the crouch bit is in the flags the client reports)
        let t0 = m.now;
        while m.now < t0 + 4000 {
            m.now += 5;
            m.net.set_now(m.now);
            m.server.update(m.now);
            for i in 0..2 {
                m.clients[i].update(m.now);
                if m.clients[i].connected() {
                    let mut s = EntityState { class: class::PLAYER, health: 100, flags: if i == 1 { flags } else { 0 }, ..Default::default() };
                    s.set_pos_m(if i == 0 { A } else { B });
                    m.clients[i].set_state(s);
                }
            }
        }
        m.knows(0, 1)
    };
    assert!(run(0, 16), "standing: the head is over the cover");
    assert!(!run(16, 16), "crouched behind cover that is higher than a crouched player's head");
    assert!(run(16, 0), "the game that does not say which bit is the crouch is told nothing: the player is treated as standing");
}

#[test]
fn a_tick_pays_for_a_bounded_number_of_rays_and_a_crowd_costs_a_fraction_of_testing_every_pair_every_tick() {
    // 40 players over 100 x 100 m, half of them walking, a wall down the middle
    let names: Vec<String> = (0..40).map(|i| format!("p{i}")).collect();
    let starts: Vec<(&'static str, V3)> = names.iter().enumerate().map(|(i, n)| (&*Box::leak(n.clone().into_boxed_str()), [(i % 8) as f32 * 12.0 - 48.0, 0.0, (i / 8) as f32 * 20.0 - 40.0])).collect();
    let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let cfg = ServerConfig { max_clients: 40, ..Default::default() };
    let mut m = Arena::new(Walled::new(16.0, &starts), cfg.clone(), &name_refs);
    let base: Vec<V3> = starts.iter().map(|s| s.1).collect();
    let drive = move |i: usize, t: u64| -> (V3, V3) {
        let p = base[i];
        if i % 2 == 0 {
            // (walks along z and back, 6 m/s)
            let phase = (t as f32 / 1000.0 * 6.0) % 40.0;
            let z = if phase < 20.0 { phase } else { 40.0 - phase };
            let v = if phase < 20.0 { 6.0 } else { -6.0 };
            ([p[0], 0.0, p[2] + z], [0.0, 0.0, v])
        } else {
            still(p)
        }
    };
    m.run(6000, &drive, |_, _| {});
    let s = &m.server.stats;
    let ticks = s.ticks as f64;
    let naive_pairs = 40.0 * 39.0 * ticks;
    println!("40 players, {} ticks: {} pairs tested ({:.1}% of testing every pair every tick), {} rays, {} pairs kept their answer because neither moved, {} unaffordable, {} left out of packets", s.ticks, s.occ_pairs, 100.0 * s.occ_pairs as f64 / naive_pairs, s.occ_rays, s.occ_still, s.occ_unaffordable, s.occ_hidden);
    assert!(s.occ_pairs as f64 <= 0.25 * naive_pairs, "{} pairs tested of {naive_pairs}", s.occ_pairs);
    assert!(s.occ_rays <= (s.ticks + 4) * 40 * cfg.occlusion.rays_per_client as u64, "{} rays", s.occ_rays);
    assert_eq!(s.occ_rays, m.server.game.rays.get(), "every ray the server counted was asked of the game");
    assert!(s.occ_still > 0, "pairs of players who stand still keep their answer");
    assert!(s.occ_hidden > 0 && s.occ_unaffordable * 100 < s.occ_pairs, "{} of {} tests had no rays", s.occ_unaffordable, s.occ_pairs);
    // with no rays to spend the near pairs are told (a short leak, never a near player who is not shown) and the far ones wait (a short delay): nobody is told anything the walls would hide only beyond 80 m
    let mut starved = cfg;
    starved.occlusion.rays_per_client = 0;
    let mut m = Arena::new(Walled::new(16.0, &starts), starved, &name_refs);
    let base: Vec<V3> = starts.iter().map(|s| s.1).collect();
    m.run(3000, &move |i, _| still(base[i]), |_, _| {});
    let s = &m.server.stats;
    assert_eq!(s.occ_rays, 0);
    assert!(s.occ_unaffordable > 0 && s.occ_hidden > 0);
    let planar = |a: V3, b: V3| ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    let near: Vec<usize> = (1..40).filter(|&j| planar(starts[0].1, starts[j].1) <= 80.0).collect();
    let known: Vec<usize> = (1..40).filter(|&j| m.knows(0, j)).collect();
    assert!(near.len() >= 20 && near.len() < 39, "{}", near.len());
    assert_eq!(known, near, "the first player knows exactly the players within 80 m, wall or no wall");
}

#[test]
fn what_was_found_out_is_asked_again_when_the_level_changes() {
    let mut m = Arena::new(Walled::new(16.0, &[("a", A), ("b", B)]), ServerConfig { max_clients: 4, ..Default::default() }, &["a", "b"]);
    // the wall is not there at first (it is built at 3 s): they see each other
    m.server.game.walls.clear();
    let drive = |i: usize, _t: u64| still(if i == 0 { A } else { B });
    m.run(3000, &drive, |_, _| {});
    assert!(m.knows(0, 1), "no wall: known");
    let before = m.last_tick(0, 1);
    m.server.game.walls.push(Wall { lo: [-1.0, 0.0, -30.0], hi: [1.0, 8.0, 30.0] });
    m.server.game.version += 1;
    m.run(3000, &drive, |_, _| {});
    // (the hold has passed: the records stopped some time ago, and the newest is not a recent one)
    let after = m.last_tick(0, 1);
    assert!(after < m.server.world.tick - 60, "the records of the player behind the new wall stopped: {before} -> {after} at tick {}", m.server.world.tick);
    // ... and a wall taken down shows the player again
    m.server.game.walls.clear();
    m.server.game.version += 1;
    m.run(2000, &drive, |_, _| {});
    assert!(m.last_tick(0, 1) > m.server.world.tick - 30, "the wall is gone: the player is told again");
}

#[test]
fn a_shoulder_or_a_head_past_the_edge_of_a_wall_counts_as_in_sight() {
    use super::occlusion::{sees, Occlusion, Params};
    let game = Walled::new(16.0, &[]);
    let p = Params::new(&Occlusion::default(), 1, 30, 16.0, 35.0, 80.0, 16.0, 1, 0);
    let mut obs = EntityState { class: class::PLAYER, ..Default::default() };
    obs.set_pos_m(A);
    // B stands still on x = 20 and steps along z, out from behind the end of the wall: where does the line open for the middle of its body (head, chest, feet), and where for the server's tests?
    let (mut by_body, mut by_server) = (None, None);
    for k in 0..400 {
        let z = 60.0 + k as f32 * 0.02;
        let mut t = EntityState { class: class::PLAYER, id: 9, ..Default::default() };
        t.set_pos_m([20.0, 0.0, z]);
        if by_body.is_none() && truth(&game.walls, A, t.pos_m()) {
            by_body = Some(t.pos_m()[2]);
        }
        let (mut budget, mut rays) = (100, 0);
        if by_server.is_none() && sees(&game, &p, &obs, &t, false, &mut budget, &mut rays) == Some(true) {
            by_server = Some(t.pos_m()[2]);
        }
    }
    let (body, server) = (by_body.unwrap(), by_server.unwrap());
    println!("the line opens for the middle of the body at z = {body:.2} and for the shoulders at z = {server:.2}");
    assert!(server < body - 0.05, "a shoulder that is past the edge is in sight before the middle of the body is: {server} against {body}");
    // and a head over a ledge: a wall 1.5 m high hides the chest of a standing player, not its head
    let mut low = Walled::new(16.0, &[]);
    low.walls = vec![Wall { lo: [-1.0, 0.0, -30.0], hi: [1.0, 1.5, 30.0] }];
    let mut t = EntityState { class: class::PLAYER, id: 9, ..Default::default() };
    t.set_pos_m(B);
    let (mut budget, mut rays) = (100, 0);
    assert_eq!(sees(&low, &p, &obs, &t, false, &mut budget, &mut rays), Some(true));
    assert_eq!(rays, 2, "the chest is tried first, then the head");
    let (mut budget, mut rays) = (1, 0);
    assert_eq!(sees(&low, &p, &obs, &t, false, &mut budget, &mut rays), None, "out of rays after the chest, before the head: not known");
}

/// A fixed match (20 players walking to and fro in a 120 m square, 15 things on the ground, over a network with latency, jitter and loss, 6 s) played with the defence off, reduced to the numbers that every packet,
/// record and byte of it adds to: what the server sent and received and what the clients received. They are what the same match gave in the netcode before the defence existed (taken on 2026-10-22 from the engine
/// without it), so a game that does not turn it on is byte for byte what it was.
fn old_match_numbers() -> [u64; 6] {
    struct Plain;
    fn spot(i: u32) -> (f32, f32) {
        let h = |k: u32| (i.wrapping_mul(2_654_435_761).wrapping_add(k.wrapping_mul(40_503)).wrapping_mul(2_246_822_519) >> 8) as f32 / (1u32 << 24) as f32;
        (h(1) * 120.0, h(7) * 120.0)
    }
    impl Game for Plain {
        fn on_join(&mut self, _w: &mut World, _slot: usize, name: &str) -> EntityState {
            let i: u32 = name[1..].parse().unwrap();
            let mut s = EntityState { class: class::PLAYER, health: 100, ..Default::default() };
            let (x, z) = spot(i);
            s.set_pos_m([x, 0.0, z]);
            s
        }
    }
    let net = SimNet::new(Link { latency_ms: 30, jitter_ms: 10, loss: 0.02, duplicate: 0.005 }, 77);
    let ep = net.endpoint();
    let addr = ep.addr();
    let mut server = NetServer::new(ServerConfig { max_clients: 20, ..Default::default() }, ep, Plain);
    for i in 0..15 {
        let (x, z) = spot(100 + i);
        server.world.spawn(class::ITEM, [x, 0.0, z]);
    }
    let mut clients: Vec<NetClient<SimEndpoint>> = (0..20).map(|i| NetClient::new(net.endpoint(), addr, &format!("p{i}"), 500 + i as u32)).collect();
    let mut now = 1000u64;
    while now < 7000 {
        now += 5;
        net.set_now(now);
        server.update(now);
        for (i, c) in clients.iter_mut().enumerate() {
            c.update(now);
            if c.connected() {
                // (to and fro along a direction of its own: 15 m either way, a turn every 8 s)
                let (x, z) = spot(i as u32);
                let a = i as f32 * 0.7;
                let w = std::f32::consts::TAU / 8.0;
                let t = (now - 1000) as f32 / 1000.0;
                let (s, v) = ((w * t).sin() * 15.0, (w * t).cos() * 15.0 * w);
                let mut st = EntityState { class: class::PLAYER, health: 100, ..Default::default() };
                st.set_pos_m([x + a.cos() * s, 0.0, z + a.sin() * s]);
                st.set_vel_ms([a.cos() * v, 0.0, a.sin() * v]);
                c.set_state(st);
            }
        }
    }
    let s = &server.stats;
    let (bytes_in, records_in) = clients.iter().fold((0, 0), |(b, r), c| (b + c.bytes_in, r + c.records_in));
    [s.packets_out, s.bytes_out, s.records_out, s.packets_in, bytes_in, records_in]
}

#[test]
fn with_the_defence_off_the_netcode_is_what_it_was() {
    // packets out, bytes out and records out of the server; packets in at the server; bytes in and records in at the 20 clients (the engine without the defence gave exactly these, run after run)
    assert_eq!(old_match_numbers(), [3534, 805686, 38229, 3289, 791489, 37269]);
}

#[test]
fn the_governor_cuts_the_rays_when_the_ticks_are_too_long_gives_them_back_when_they_are_quick_and_never_touches_them_otherwise() {
    use super::occlusion::Governor;
    // off: always the whole of them, whatever the ticks take
    let mut g = Governor::default();
    for _ in 0..100 {
        assert_eq!(g.update(90_000, 0), 1.0);
    }
    // on, with the ticks inside the limit (24 ms): not a ray less, and the same when they are a hair under
    let mut g = Governor::default();
    for k in 0..300 {
        let scale = g.update(if k % 2 == 0 { 24_000 } else { 20_000 }, 24);
        assert_eq!(scale, 1.0, "tick {k}");
    }
    // over (40 ms against 24): the share falls every tick and stops at a quarter
    let mut last = 1.0f32;
    let mut cut = 0;
    for k in 0..60 {
        let scale = g.update(40_000, 24);
        assert!(scale <= last && scale >= Governor::LEAST, "tick {k}: {scale}");
        assert!(scale >= last * 0.9 - 1.0e-6, "at most a tenth a tick: {last} to {scale}");
        if scale < last {
            cut += 1;
        }
        last = scale;
    }
    assert_eq!(last, Governor::LEAST, "40 ms ticks against a limit of 24 take it to the least: {last}");
    assert!(cut >= 10, "it fell for several ticks, not at once: {cut}");
    // and the ticks are quick again: it comes back, never above all of them, never in one step
    let mut back = 0;
    for k in 0..200 {
        let scale = g.update(8_000, 24);
        assert!(scale >= last && scale <= 1.0, "tick {k}: {scale}");
        assert!(scale <= last * 1.031, "a few in a hundred a tick, no more: {last} to {scale}");
        if scale == 1.0 && back == 0 {
            back = k;
        }
        last = scale;
    }
    assert!(back > 20 && back < 120, "whole again after a second or two of quick ticks: tick {back}");
    // (the same ticks give the same shares: it is a function of the times alone)
    let (mut a, mut b) = (Governor::default(), Governor::default());
    for k in 0..500u64 {
        let us = 10_000 + (k * 7919) % 40_000;
        assert_eq!(a.update(us, 24), b.update(us, 24));
    }
}

#[test]
fn a_server_that_cannot_keep_its_ticks_short_spends_fewer_rays_and_goes_back_to_all_of_them_when_it_can() {
    // 40 players over a wall, so that every tick has pairs to test; ticks that take 30 ms for the game alone against a limit of 25 ms
    let names: Vec<String> = (0..40).map(|i| format!("p{i}")).collect();
    let starts: Vec<(&'static str, V3)> = names.iter().enumerate().map(|(i, n)| (&*Box::leak(n.clone().into_boxed_str()), [(i % 8) as f32 * 12.0 - 48.0, 0.0, (i / 8) as f32 * 20.0 - 40.0])).collect();
    let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let mut cfg = ServerConfig { max_clients: 40, ..Default::default() };
    cfg.occlusion.governor_ms = 25;
    // (few enough rays a client that they are all spent every tick: that is where fewer of them show)
    cfg.occlusion.rays_per_client = 30;
    let mut m = Arena::new(Walled::new(16.0, &starts), cfg, &name_refs);
    let base: Vec<V3> = starts.iter().map(|s| s.1).collect();
    // (they all walk, so that there is always something to test)
    let drive = move |i: usize, t: u64| -> (V3, V3) {
        let p = base[i];
        let phase = (t as f32 / 1000.0 * 6.0 + i as f32 * 3.0) % 40.0;
        let (z, v) = if phase < 20.0 { (phase, 6.0) } else { (40.0 - phase, -6.0) };
        ([p[0], 0.0, p[2] + z], [0.0, 0.0, v])
    };
    let rays_over = |m: &mut Arena, ms: u64| {
        let (r0, t0) = (m.server.stats.occ_rays, m.server.stats.ticks);
        m.run(ms, &drive, |_, _| {});
        (m.server.stats.occ_rays - r0) as f64 / (m.server.stats.ticks - t0).max(1) as f64
    };
    // quick ticks: all the rays are there to spend
    let quick = rays_over(&mut m, 4000);
    assert_eq!(m.server.occlusion_scale(), 1.0);
    assert_eq!(m.server.stats.occ_throttled, 0);
    // a slow game: the share falls to a quarter, and the rays a tick with it
    m.server.game.delay_ms = 30;
    m.run(1500, &drive, |_, _| {});
    assert!(m.server.occlusion_scale() < 0.5, "the share after a second and a half of slow ticks: {}", m.server.occlusion_scale());
    let slow = rays_over(&mut m, 1500);
    assert!(m.server.stats.occ_throttled > 0);
    assert!(slow < quick * 0.5, "{slow} rays a tick while the ticks were slow against {quick}");
    // and a game that is quick again gets them all back
    m.server.game.delay_ms = 0;
    m.run(4000, &drive, |_, _| {});
    assert_eq!(m.server.occlusion_scale(), 1.0, "all of them again");
    let again = rays_over(&mut m, 3000);
    assert!(again > quick * 0.7, "{again} rays a tick once it is quick again against {quick}");
}

#[test]
fn the_profile_keeps_the_time_of_every_tick_the_slowest_one_with_its_phases_and_the_canaries() {
    use super::server::phase;
    let cfg = ServerConfig { max_clients: 4, profile: true, ..Default::default() };
    let mut m = Arena::new(Walled::new(16.0, &[("a", A), ("b", B)]), cfg, &["a", "b"]);
    // a game that takes 3 ms of every tick, and one tick in ten 12 ms
    m.server.game.delay_ms = 3;
    let drive = |i: usize, _t: u64| still(if i == 0 { A } else { B });
    m.run(900, &drive, |_, _| {});
    m.server.game.delay_ms = 12;
    m.run(40, &drive, |_, _| {});
    m.server.game.delay_ms = 3;
    m.run(500, &drive, |_, _| {});
    let log = m.server.take_tick_log();
    assert!(log.len() > 30, "{} ticks", log.len());
    assert!(log.iter().all(|t| t.0 >= 3000), "every tick took at least the game's 3 ms by the clock");
    let worst = m.server.take_worst();
    assert_eq!(worst.us as u32, log.iter().map(|t| t.0).max().unwrap(), "the slowest tick is the longest of them");
    assert!(worst.us >= 12_000 && worst.phase_ns[phase::GAME] >= 12_000_000, "it is the slow one, and its time is in the game's phase: {} us, {:?}", worst.us, worst.phase_ns);
    assert!(worst.canary_ns[0] > 0 && worst.canary_ns[1] > 0, "the canaries ran: {:?}", worst.canary_ns);
    assert!(worst.players == 2 && worst.tick > 0);
    let canaries = m.server.take_canary_log();
    assert_eq!(canaries.len(), log.len());
    // what was taken is gone, and what is kept is not kept when the profile is off
    assert!(m.server.take_tick_log().is_empty() && m.server.take_worst().us == 0);
    let mut quiet = Arena::new(Walled::new(16.0, &[("a", A), ("b", B)]), ServerConfig { max_clients: 4, ..Default::default() }, &["a", "b"]);
    quiet.run(500, &drive, |_, _| {});
    assert!(quiet.server.take_tick_log().is_empty() && quiet.server.take_worst().us == 0 && quiet.server.take_canary_log().is_empty());
}

#[test]
fn a_change_of_the_level_somewhere_the_lines_of_a_pair_do_not_go_leaves_its_answer_and_one_on_them_asks_again() {
    let drive = |i: usize, _t: u64| still(if i == 0 { A } else { B });
    let wall = Wall { lo: [-1.0, 0.0, -30.0], hi: [1.0, 8.0, 30.0] };
    // the two players stand still with the wall between them; a box is built a kilometre away; the game says where (or does not)
    let far_change = |region: Option<(V3, V3)>| {
        let mut m = Arena::new(Walled::new(16.0, &[("a", A), ("b", B)]), ServerConfig { max_clients: 4, ..Default::default() }, &["a", "b"]);
        m.run(3000, &drive, |_, _| {});
        assert!(!m.knows(0, 1), "the wall hides them");
        let from = m.server.game.version;
        let far = Wall { lo: [900.0, 0.0, 900.0], hi: [902.0, 8.0, 960.0] };
        m.server.game.walls.push(far);
        m.server.game.version += 1;
        m.server.game.change = LevelChange { from, regions: region.map(|_| vec![(far.lo, far.hi)]).unwrap_or_default() };
        let before = m.server.stats.occ_rays;
        m.run(1000, &drive, |_, _| {});
        assert!(!m.knows(0, 1));
        m.server.stats.occ_rays - before
    };
    let without = far_change(None);
    let with_region = far_change(Some(([0.0; 3], [0.0; 3])));
    assert!(without >= 5, "the answer is asked again when nothing is said about the change: {without} rays");
    assert_eq!(with_region, 0, "and not when the change was somewhere else: {with_region} rays");
    // the wall between them is taken down and the game says where it was: the lines go through that region, the pair is asked again and found in sight
    let mut m = Arena::new(Walled::new(16.0, &[("a", A), ("b", B)]), ServerConfig { max_clients: 4, ..Default::default() }, &["a", "b"]);
    m.run(3000, &drive, |_, _| {});
    assert!(!m.knows(0, 1));
    let from = m.server.game.version;
    m.server.game.walls.clear();
    m.server.game.version += 1;
    m.server.game.change = LevelChange { from, regions: vec![(wall.lo, wall.hi)] };
    m.run(1500, &drive, |_, _| {});
    assert!(m.knows(0, 1) && m.last_tick(0, 1) + 30 > m.server.world.tick, "the wall is gone: they see each other");
    // (a region that is wrong is the game's to get right: it is the game's word, which the server takes)
}

#[test]
fn what_is_taken_for_the_lines_of_a_pair_holds_every_line_of_it() {
    use super::occlusion::{lines_meet, Occlusion, Params};
    // pairs of players, standing and moving, against boxes: when the pair says its lines miss the box, not one of the lines `sees` casts goes through it
    let p = Params::new(&Occlusion::default(), 1, 30, 16.0, 35.0, 80.0, 16.0, 1, 0);
    let mut seed = 0x853C_49E6_748F_EA9Bu64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u64 << 24) as f32
    };
    struct Record(std::cell::RefCell<Vec<(V3, V3)>>);
    impl Game for Record {
        fn on_join(&mut self, _w: &mut World, _slot: usize, _name: &str) -> EntityState {
            EntityState::default()
        }
        fn blocked(&self, from: V3, to: V3) -> bool {
            self.0.borrow_mut().push((from, to));
            true
        }
    }
    // does the segment a..b meet the box? (the slab test, with a little room)
    let meets = |a: V3, b: V3, lo: V3, hi: V3| -> bool {
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for k in 0..3 {
            let d = b[k] - a[k];
            if d.abs() < 1.0e-9 {
                if a[k] < lo[k] - 1.0e-3 || a[k] > hi[k] + 1.0e-3 {
                    return false;
                }
            } else {
                let (mut u, mut v) = ((lo[k] - 1.0e-3 - a[k]) / d, (hi[k] + 1.0e-3 - a[k]) / d);
                if u > v {
                    std::mem::swap(&mut u, &mut v);
                }
                t0 = t0.max(u);
                t1 = t1.min(v);
                if t0 > t1 {
                    return false;
                }
            }
        }
        true
    };
    let (mut said_no, mut said_yes) = (0, 0);
    for k in 0..20_000 {
        let mut obs = EntityState { class: class::PLAYER, ..Default::default() };
        let mut tgt = EntityState { id: 9, class: class::PLAYER, ..Default::default() };
        let speed = if k % 3 == 0 { 0.0 } else { 1.0 + rnd() * 7.0 };
        obs.set_pos_m([rnd() * 100.0 - 50.0, rnd() * 3.0, rnd() * 100.0 - 50.0]);
        tgt.set_pos_m([rnd() * 100.0 - 50.0, rnd() * 3.0, rnd() * 100.0 - 50.0]);
        let (a, b) = (rnd() * 6.28, rnd() * 6.28);
        obs.set_vel_ms([a.sin() * speed, 0.0, a.cos() * speed]);
        tgt.set_vel_ms([b.sin() * speed, 0.0, b.cos() * speed]);
        tgt.flags = (k % 5 == 0) as u16;
        // (half of the boxes are put near the line between the two, the others anywhere)
        let (op, tp) = (obs.pos_m(), tgt.pos_m());
        let u = rnd();
        let c = if k % 2 == 0 {
            [rnd() * 140.0 - 70.0, rnd() * 4.0, rnd() * 140.0 - 70.0]
        } else {
            [op[0] + (tp[0] - op[0]) * u + rnd() * 8.0 - 4.0, op[1] + (tp[1] - op[1]) * u + rnd() * 4.0, op[2] + (tp[2] - op[2]) * u + rnd() * 8.0 - 4.0]
        };
        let h = [rnd() * 3.0 + 0.1, rnd() * 3.0 + 0.1, rnd() * 3.0 + 0.1];
        let (lo, hi) = ([c[0] - h[0], c[1] - h[1], c[2] - h[2]], [c[0] + h[0], c[1] + h[1], c[2] + h[2]]);
        let game = Record(Default::default());
        let (mut budget, mut rays) = (100u32, 0u64);
        super::occlusion::sees(&game, &p, &obs, &tgt, tgt.flags & 1 != 0, &mut budget, &mut rays);
        let lines = game.0.borrow().clone();
        assert!(lines.len() >= 5);
        let mut region = p.clone();
        region.regions = vec![(lo, hi)];
        if lines_meet(&region, &obs, &tgt) {
            said_yes += 1;
        } else {
            said_no += 1;
            for (from, to) in &lines {
                assert!(!meets(*from, *to, lo, hi), "pair {k}: the line {from:?} to {to:?} goes through the box {lo:?} {hi:?}, which the pair said its lines miss");
            }
        }
    }
    assert!(said_no > 4000 && said_yes > 2500, "both answers are given: {said_no} {said_yes}");
}
