//! The wallhack defence on the game server (docs/NETWORK_SCALE.md "Wallhacks"): a scene with a wall, `NetHost.occlusion`, and players that see only what is not behind it. No art: names and boxes.
use super::client::NetClient;
use super::play::NetDriver;
use super::server::{NetServer, ServerConfig};
use super::simgame::SimGame;
use super::transport::{Link, SimEndpoint, SimNet};
use lunaris_json::{parse, Json};
use lunaris_sim::components::{NetPlayer, NetPuppet, Transform};
use lunaris_sim::{Command, Sim};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// A ground, a wall down the middle (2 m thick, 8 m high, 60 m long: the same as the wall of occlusion_tests.rs), two places to start 50 m apart on either side of it and an avatar.
fn walled_scene(host_extra: &str, wall: bool) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[400,1,400]}}"#);
    if wall {
        spawn(&mut s, 7, r#"{"Transform":{"pos":[0,4,0]},"Collider":{"shape":1,"size":[2,8,60]}}"#);
    }
    spawn(&mut s, 4, &format!(r#"{{"NetHost":{{"player":"Avatar","static_every":3{host_extra}}}}}"#));
    spawn(&mut s, 21, r#"{"Transform":{"pos":[-25,0,0]},"SpawnPoint":{}}"#);
    spawn(&mut s, 22, r#"{"Transform":{"pos":[25,0,0]},"SpawnPoint":{}}"#);
    s.apply(&Command::DefinePrefab {
        name: "Avatar".into(),
        def: parse(r#"{"components":{"Name":{"value":"Avatar"},"Transform":{},"CharacterBody":{"radius":0.35,"height":1.8,"gravity_scale":0},"HitPoints":{"value":100,"max":100,"last":100},"NetAvatar":{}}}"#).unwrap(),
    })
    .unwrap();
    s
}

fn client_scene() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{},"Velocity":{},"NetPlayer":{"puppet":"Mate","kinds":"4 50 DroneModel"}}"#);
    for name in ["Mate", "DroneModel"] {
        s.apply(&Command::DefinePrefab { name: name.into(), def: parse(&format!(r#"{{"components":{{"Name":{{"value":"{name}"}},"Transform":{{}},"NetPuppet":{{}}}}}}"#)).unwrap() }).unwrap();
    }
    s
}

struct Rig {
    net: SimNet,
    server: NetServer<SimEndpoint, SimGame>,
    sims: Vec<Sim>,
    drivers: Vec<NetDriver<SimEndpoint>>,
    now: u64,
}

fn rig(game: SimGame, n: usize) -> Rig {
    let net = SimNet::new(Link { latency_ms: 20, jitter_ms: 0, loss: 0.0, duplicate: 0.0 }, 7);
    let end = net.endpoint();
    let addr = end.addr();
    let server = NetServer::new(ServerConfig::default(), end, game);
    let sims = (0..n).map(|_| client_scene()).collect();
    let drivers = (0..n).map(|i| NetDriver::new(NetClient::new(net.endpoint(), addr, &format!("p{i}"), 50 + i as u32))).collect();
    Rig { net, server, sims, drivers, now: 1000 }
}

impl Rig {
    fn run(&mut self, ms: u64) {
        let end = self.now + ms;
        while self.now < end {
            self.now += 10;
            self.net.set_now(self.now);
            self.server.update(self.now);
            for (d, s) in self.drivers.iter_mut().zip(self.sims.iter_mut()) {
                d.tick_at(s, self.now);
            }
        }
    }
}

/// The puppets a client has: (class, place).
fn puppets(s: &mut Sim) -> Vec<(u32, [f32; 3])> {
    let mut out: Vec<(u32, [f32; 3])> = Vec::new();
    let _ = s.world.for_each::<(&NetPuppet, &Transform), _>(|_, (p, t)| out.push((p.class, t.pos)));
    out
}

fn players_seen(s: &mut Sim) -> usize {
    puppets(s).iter().filter(|p| p.0 == 0).count()
}

fn me(s: &Sim) -> [f32; 3] {
    let e = s.world.entity(1).unwrap();
    assert!(s.world.get::<NetPlayer>(e).unwrap().connected);
    s.world.get::<Transform>(e).unwrap().pos
}

#[test]
fn on_a_game_server_with_occlusion_on_the_other_side_of_a_wall_is_not_told_and_with_it_off_it_is() {
    let mut off = rig(SimGame::new(walled_scene("", true)), 2);
    let mut on = rig(SimGame::new(walled_scene(r#","occlusion":16"#, true)), 2);
    off.run(4000);
    on.run(4000);
    for r in [&mut off, &mut on] {
        let (a, b) = (me(&r.sims[0]), me(&r.sims[1]));
        assert!((a[0] - b[0]).abs() > 49.0 && a[0].abs() == 25.0 && b[0].abs() == 25.0, "the players start on either side of the wall: {a:?} {b:?}");
    }
    assert_eq!((players_seen(&mut off.sims[0]), players_seen(&mut off.sims[1])), (1, 1), "occlusion off: both see the other through the wall");
    assert_eq!((players_seen(&mut on.sims[0]), players_seen(&mut on.sims[1])), (0, 0), "occlusion on: neither is told about the other");
    assert!(on.server.stats.occ_hidden > 0 && on.server.stats.occ_rays > 0, "{:?}", on.server.stats);
    assert_eq!(off.server.stats.occ_rays, 0);
    assert!(on.server.game.stats.sight_builds >= 1 && on.server.game.stats.sight_shapes >= 2, "{:?}", on.server.game.stats);
    assert_eq!(off.server.game.stats.sight_builds, 0, "with it off the static world is never gathered");
}

#[test]
fn a_wall_that_is_built_hides_the_player_and_one_that_is_taken_down_shows_it_again() {
    // (no wall at first: they see each other; the wall goes up at 3 s and the player is dropped; it comes down and they see each other again)
    let mut r = rig(SimGame::new(walled_scene(r#","occlusion":16"#, false)), 2);
    r.run(3000);
    assert_eq!((players_seen(&mut r.sims[0]), players_seen(&mut r.sims[1])), (1, 1));
    spawn(&mut r.server.game.sim, 70, r#"{"Transform":{"pos":[0,4,0]},"Collider":{"shape":1,"size":[2,8,60]}}"#);
    // (the world is gathered again within half a second, the hold is 0.6 s, and the client forgets what it has not heard of for 2 s)
    r.run(4500);
    assert_eq!((players_seen(&mut r.sims[0]), players_seen(&mut r.sims[1])), (0, 0), "the wall is up: they are not told");
    r.server.game.sim.apply(&Command::Despawn { uid: 70 }).unwrap();
    r.run(2500);
    assert_eq!((players_seen(&mut r.sims[0]), players_seen(&mut r.sims[1])), (1, 1), "the wall is down: they are told again");
}

#[test]
fn machines_hide_behind_walls_only_when_the_host_says_so() {
    // a drone (a thing that moves, class 4) behind the wall from the player at x = -25: the one at x = 25 stands near it (within the safe radius)
    let drone = r#"{"Transform":{"pos":[27,1,2]},"NetEntity":{"class":4,"kind":50}}"#;
    let known = |extra: &str| -> (usize, usize) {
        let mut sim = walled_scene(extra, true);
        spawn(&mut sim, 30, drone);
        let mut r = rig(SimGame::new(sim), 2);
        r.run(4000);
        let drones = |s: &mut Sim| puppets(s).iter().filter(|p| p.0 == 4).count();
        let x = |s: &Sim| me(s)[0];
        // (the player at x = -25 is the one the wall is between the drone and)
        let far = if x(&r.sims[0]) < 0.0 { 0 } else { 1 };
        (drones(&mut r.sims[far]), drones(&mut r.sims[1 - far]))
    };
    assert_eq!(known(r#","occlusion":16"#), (1, 1), "machines are always told unless the host asks for them to be hidden");
    assert_eq!(known(r#","occlusion":16,"occlude_machines":true"#), (0, 1), "hidden from the player behind the wall, told to the one next to it");
}

/// The town of tools/build_occlusion_town.py (closed buildings on a grid, 5 boxes each) as a scene to host, with `NetHost.occlusion` `safe`.
fn town(safe: f32, lots: u32, pitch: f32, spawns: u32) -> Sim {
    let mut s = Sim::new(1);
    let half = lots as f32 * pitch / 2.0;
    spawn(&mut s, 2, &format!(r#"{{"Transform":{{"pos":[0,-0.5,0]}},"Collider":{{"shape":1,"size":[{},1,{}]}}}}"#, half * 2.0 + 400.0, half * 2.0 + 400.0));
    spawn(&mut s, 4, &format!(r#"{{"NetHost":{{"player":"Avatar","static_every":15,"occlusion":{safe}}}}}"#));
    let mut uid = 100;
    for i in 0..lots {
        for j in 0..lots {
            let (cx, cz) = ((i as f32 + 0.5) * pitch - half, (j as f32 + 0.5) * pitch - half);
            for (dx, dz, sx, sz) in [(0.0, -10.0, 20.0, 0.5), (0.0, 10.0, 20.0, 0.5), (-10.0, 0.0, 0.5, 20.0), (10.0, 0.0, 0.5, 20.0)] {
                uid += 1;
                spawn(&mut s, uid, &format!(r#"{{"Transform":{{"pos":[{},4,{}]}},"Collider":{{"shape":1,"size":[{sx},8,{sz}]}}}}"#, cx + dx, cz + dz));
            }
            uid += 1;
            spawn(&mut s, uid, &format!(r#"{{"Transform":{{"pos":[{cx},8.25,{cz}]}},"Collider":{{"shape":1,"size":[20,0.5,20]}}}}"#));
        }
    }
    // (spawn points at the crossings of the streets, 200 of them in a fixed pseudo-random order)
    let mut r = 12345u32;
    for k in 0..spawns {
        r = r.wrapping_mul(1664525).wrapping_add(1013904223);
        let i = (r >> 8) % (lots - 1);
        r = r.wrapping_mul(1664525).wrapping_add(1013904223);
        let j = (r >> 8) % (lots - 1);
        uid += 1;
        spawn(&mut s, uid + 10_000 + k as u64, &format!(r#"{{"Transform":{{"pos":[{},0,{}]}},"SpawnPoint":{{}}}}"#, (i + 1) as f32 * pitch - half, (j + 1) as f32 * pitch - half));
    }
    s.apply(&Command::DefinePrefab { name: "Avatar".into(), def: parse(r#"{"components":{"Name":{"value":"Avatar"},"Transform":{},"HitPoints":{"value":100,"max":100,"last":100},"NetAvatar":{}}}"#).unwrap() }).unwrap();
    s
}

/// The packets of the clients made on several threads at once are the packets the one thread makes: the same town, the same bots and the same network, run with one thread and with four, end with the same counts of
/// packets, bytes, records, pairs tested, rays cast and players left out of sight (every one of them depends on what was in the packets before it).
#[test]
fn packets_made_on_several_threads_are_the_same_as_on_one() {
    use super::swarm::Bot;
    let (lots, pitch) = (6u32, 50.0f32);
    let half = lots as f32 * pitch / 2.0;
    let run = |threads: usize| {
        let net = SimNet::new(Link { latency_ms: 25, jitter_ms: 5, loss: 0.02, duplicate: 0.0 }, 5);
        let end = net.endpoint();
        let addr = end.addr();
        let cfg = ServerConfig { max_clients: 80, threads, ..Default::default() };
        let mut server = NetServer::new(cfg, end, SimGame::new(town(16.0, lots, pitch, 70)));
        let mut bots: Vec<Bot<SimEndpoint>> = (0..70).map(|i| Bot::new(net.endpoint(), addr, &format!("bot{i}"), 100 + i as u64, half - 10.0, false)).collect();
        let mut now = 1000u64;
        while now < 1000 + 8000 {
            now += 5;
            net.set_now(now);
            server.update(now);
            for b in bots.iter_mut() {
                b.step(now);
            }
        }
        let s = &server.stats;
        (server.client_count(), s.packets_out, s.bytes_out, s.records_out, s.occ_pairs, s.occ_rays, s.occ_still, s.occ_hidden, s.occ_unaffordable)
    };
    let one = run(1);
    let four = run(4);
    println!("one thread {one:?}
four threads {four:?}");
    assert_eq!(one.0, 70, "everybody joined");
    assert!(one.1 > 1000 && one.7 > 0, "packets were made and some players were out of sight: {one:?}");
    // (not byte for byte: the server lists its entities in the order of a hash table whose order is different every time it is made, so two runs of the SAME thread count differ by about this much too;
    // what must be the same is how many packets there were, and the rest to a small share)
    assert_eq!(one.1, four.1, "the same number of packets");
    let near = |a: u64, b: u64, share: f64| (a as f64 - b as f64).abs() <= share * a.max(b) as f64;
    assert!(near(one.2, four.2, 0.005), "bytes {} and {}", one.2, four.2);
    assert!(near(one.3, four.3, 0.005), "records {} and {}", one.3, four.3);
    assert!(near(one.4, four.4, 0.01), "pairs tested {} and {}", one.4, four.4);
    assert!(near(one.5, four.5, 0.01), "rays {} and {}", one.5, four.5);
    assert!(near(one.7, four.7, 0.01), "players left out of sight {} and {}", one.7, four.7);
}

/// What the threads that make the packets save: 400 bots in a dense town (150 m square: nearly everybody within the interest radius of everybody, the worst case of the wallhack defence), the server's own time for
/// a tick measured with the packets made on 1, 2, 4 and 8 threads. In memory, so that no swarm of bots competes for the processors; `cargo test --release -p lunaris-net --lib threads_bench -- --ignored --nocapture`.
#[test]
#[ignore]
fn threads_bench_the_packets_of_400_dense_bots() {
    use super::swarm::Bot;
    let (lots, pitch) = (3u32, 50.0f32);
    let half = lots as f32 * pitch / 2.0;
    let players: usize = std::env::var("BENCH_PLAYERS").ok().and_then(|v| v.parse().ok()).unwrap_or(400);
    let mut lines = Vec::new();
    for threads in [1usize, 2, 4, 8] {
        let net = SimNet::new(Link { latency_ms: 25, jitter_ms: 5, loss: 0.01, duplicate: 0.0 }, 5);
        let end = net.endpoint();
        let addr = end.addr();
        let cfg = ServerConfig { max_clients: players + 10, threads, profile: true, ..Default::default() };
        let mut server = NetServer::new(cfg, end, SimGame::new(town(16.0, lots, pitch, players as u32)));
        let mut bots: Vec<Bot<SimEndpoint>> = (0..players).map(|i| Bot::new(net.endpoint(), addr, &format!("bot{i}"), 100 + i as u64, half - 5.0, false)).collect();
        let mut now = 1000u64;
        let (mut spent, mut ticks, mut worst) = (0u128, 0u64, 0u128);
        let t_end = now + 16_000;
        while now < t_end {
            now += 5;
            net.set_now(now);
            let t = std::time::Instant::now();
            server.update(now);
            let us = t.elapsed().as_micros();
            if now >= 1000 + 6000 {
                // (the server runs a tick every 33 ms: an update that made one is the long one)
                if us > 1000 {
                    spent += us;
                    ticks += 1;
                    worst = worst.max(us);
                }
            }
            for b in bots.iter_mut() {
                b.step(now);
            }
        }
        let s = &server.stats;
        let ph: Vec<String> = s.phase_ns.iter().zip(super::server::phase::NAMES).map(|(ns, n)| format!("{n} {}", ns / 1000 / s.ticks.max(1))).collect();
        lines.push(format!("{threads} thread(s): {} players, tick {} us on average over {ticks} ticks (the slowest {worst} us); phases (us a tick, all threads added up): {}", server.client_count(), spent / ticks.max(1) as u128, ph.join(", ")));
    }
    println!("\n{}", lines.join("\n"));
}

/// What a wallhack client could still learn: 200 bots walk about a town for 20 s, and every half second each is asked which of the players it holds a fresh record of are really behind a wall (judged with the
/// same rays at the players' true places, standing, any of head, chest and feet). Prints the figures for the defence off and on; `cargo test --release -p lunaris-net --lib leak -- --ignored --nocapture`.
#[test]
#[ignore]
fn leak_what_a_wallhack_client_still_learns_in_a_town_of_200_bots() {
    use super::swarm::Bot;
    use lunaris_sim::sight::Sight;
    let (lots, pitch) = (12u32, 50.0f32);
    let half = lots as f32 * pitch / 2.0;
    let mut report = Vec::new();
    for safe in [0.0f32, 16.0, 24.0] {
        let net = SimNet::new(Link { latency_ms: 25, jitter_ms: 5, loss: 0.02, duplicate: 0.0 }, 5);
        let end = net.endpoint();
        let addr = end.addr();
        let mut server = NetServer::new(ServerConfig { max_clients: 220, ..Default::default() }, end, SimGame::new(town(safe, lots, pitch, 200)));
        let mut bots: Vec<Bot<SimEndpoint>> = (0..200).map(|i| Bot::new(net.endpoint(), addr, &format!("bot{i}"), 100 + i as u64, half - 10.0, false)).collect();
        let sight = Sight::gather(&mut server.game.sim);
        let mut now = 1000u64;
        let (mut pairs, mut hidden_near, mut hidden_far, mut known_hidden_near, mut known_hidden_far, mut known_visible, mut known, mut samples) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
        let t_end = now + 22_000;
        while now < t_end {
            now += 5;
            net.set_now(now);
            server.update(now);
            for b in bots.iter_mut() {
                b.step(now);
            }
            if now >= 1000 + 6000 && (now - 1000) % 500 == 0 {
                // (the places the server has now)
                let place: std::collections::HashMap<u32, [f32; 3]> = server.world.entities.iter().filter(|(_, e)| e.owner.is_some()).map(|(&id, e)| (id, e.state.pos_m())).collect();
                for b in bots.iter() {
                    let Some(&me) = place.get(&b.client.entity) else { continue };
                    samples += 1;
                    for (&id, &p) in place.iter() {
                        let d = ((p[0] - me[0]).powi(2) + (p[2] - me[2]).powi(2)).sqrt();
                        if id == b.client.entity || d > 150.0 {
                            continue;
                        }
                        let eye = [me[0], me[1] + 1.6, me[2]];
                        let seen_truly = [1.7f32, 1.1, 0.25].iter().any(|h| sight.clear(eye, [p[0], p[1] + h, p[2]]));
                        let fresh = b.client.remotes().get(&id).is_some_and(|r| now.saturating_sub(r.last_seen) < 400);
                        pairs += 1;
                        known += fresh as u64;
                        if seen_truly {
                            known_visible += fresh as u64;
                        } else {
                            if d <= safe {
                                hidden_near += 1;
                                known_hidden_near += fresh as u64;
                            } else {
                                hidden_far += 1;
                                known_hidden_far += fresh as u64;
                            }
                        }
                    }
                }
            }
        }
        let per = |x: u64| x as f64 / samples.max(1) as f64;
        let s = &server.stats;
        report.push(format!(
            "safe radius {safe:>4}: per player within 150 m: {:.1} others, of which {:.1} are behind a wall; the client holds fresh records of {:.1} ({:.1} in sight, {:.1} behind a wall within the safe radius, {:.1} behind a wall beyond it: the hold and the look-ahead); the server tick {:.0} us on average ({} pairs tested, {} rays, {} left out, {} tests the tick had no rays for)",
            per(pairs),
            per(hidden_near + hidden_far),
            per(known),
            per(known_visible),
            per(known_hidden_near),
            per(known_hidden_far),
            s.tick_us_total as f64 / s.ticks as f64,
            s.occ_pairs,
            s.occ_rays,
            s.occ_hidden,
            s.occ_unaffordable
        ));
        let leak = known_hidden_far as f64 / hidden_far.max(1) as f64;
        report.push(format!("    of the players behind a wall beyond the safe radius the client holds a fresh record of {:.1} percent", leak * 100.0));
        if safe == 0.0 {
            assert!(leak > 0.7, "with the defence off the hidden are known: {leak}");
        } else {
            assert!(leak < 0.25, "{known_hidden_far} of {hidden_far}");
        }
    }
    println!("\n{}", report.join("\n"));
}
