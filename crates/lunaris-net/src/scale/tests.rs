//! The big-match netcode, played in memory: a hundred bots against a server over a network with latency, jitter, loss and doubled packets.

use super::bytes::{R, W};
use super::client::{Event, NetClient};
use super::proto::{kind, seq_gt, Acks, ReliableIn, ReliableOut, MAGIC, PROTOCOL};
use super::server::{Ctx, Game, NetServer, ServerConfig, World};
use super::state::{self, class, Detail, EntityState};
use super::swarm::{cmd, Bot, DemoGame};
use super::transport::{Link, SimEndpoint, SimNet, Transport};

struct Match {
    net: SimNet,
    server: NetServer<SimEndpoint, DemoGame>,
    bots: Vec<Bot<SimEndpoint>>,
    now: u64,
}

impl Match {
    fn new(n: usize, half: f32, link: Link, seed: u64, fire: bool, cfg: ServerConfig) -> Match {
        let net = SimNet::new(link, seed);
        let ep = net.endpoint();
        let addr = ep.addr();
        let mut game = DemoGame::new(half, 400, seed);
        let mut world_items = World::default();
        game.scatter(&mut world_items);
        let mut server = NetServer::new(cfg, ep, game);
        // (the loot goes in the server's world)
        for (_, e) in world_items.entities.drain() {
            server.world.spawn_state(e.state);
        }
        let bots = (0..n).map(|i| Bot::new(net.endpoint(), addr, &format!("bot{i}"), seed + i as u64 + 1, half, fire)).collect();
        Match { net, server, bots, now: 1000 }
    }

    fn run(&mut self, ms: u64) {
        let end = self.now + ms;
        while self.now < end {
            self.now += 5;
            self.net.set_now(self.now);
            self.server.update(self.now);
            for b in self.bots.iter_mut() {
                b.step(self.now);
            }
        }
    }

    fn players(&self) -> Vec<(u32, [f32; 3])> {
        self.server.world.entities.iter().filter(|(_, e)| e.owner.is_some()).map(|(&id, e)| (id, e.state.pos_m())).collect()
    }
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[test]
fn integers_and_records_round_trip_and_bad_bytes_never_panic() {
    let mut w = W::new();
    for v in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
        w.varu(v);
    }
    for v in [0i64, -1, 1, -64, 63, i32::MIN as i64, i64::MAX, i64::MIN] {
        w.vari(v);
    }
    let mut r = R::new(&w.buf);
    for v in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
        assert_eq!(r.varu(), Some(v));
    }
    for v in [0i64, -1, 1, -64, 63, i32::MIN as i64, i64::MAX, i64::MIN] {
        assert_eq!(r.vari(), Some(v));
    }
    assert_eq!(r.u8(), None, "the end");
    let mut s = EntityState { id: 4000, class: class::PLAYER, health: 77, flags: 5, ..Default::default() };
    s.set_pos_m([-123.456, 7.25, 999.9]);
    s.set_yaw_rad(3.0);
    s.set_pitch_rad(-0.4);
    s.set_vel_ms([3.5, -1.0, 8.0]);
    s.anim = 12;
    s.set_anim_time(1.5);
    s.extra = 300;
    let mut w = W::new();
    state::write(&mut w, &s, Detail::Full);
    let full = w.len();
    let (t, d) = state::read(&mut R::new(&w.buf)).unwrap();
    assert_eq!((t, d), (s, Detail::Full));
    let mut w = W::new();
    state::write(&mut w, &s, Detail::Basic);
    let (t, d) = state::read(&mut R::new(&w.buf)).unwrap();
    assert_eq!(d, Detail::Basic);
    assert_eq!((t.id, t.pos, t.yaw, t.flags, t.health), (s.id, s.pos, s.yaw, s.flags, s.health));
    assert!(w.len() < full && full < 36, "a full record is {full} bytes, a basic one {}", w.len());
    assert!((s.pos_m()[0] + 123.456).abs() < 0.02 && (s.yaw_rad() - 3.0).abs() < 0.001);
    // garbage and every prefix of a record: no panic, just None
    let mut rng = 12345u32;
    for _ in 0..2000 {
        let n = (rng % 40) as usize;
        let bytes: Vec<u8> = (0..n)
            .map(|_| {
                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                (rng >> 24) as u8
            })
            .collect();
        let _ = state::read(&mut R::new(&bytes));
    }
    for n in 0..w.len() {
        assert!(state::read(&mut R::new(&w.buf[..n])).is_none() || n >= 14);
    }
}

#[test]
fn sequence_numbers_acknowledgements_and_reliable_messages_survive_wraparound_loss_and_doubling() {
    assert!(seq_gt(1, 0) && seq_gt(0, 65535) && !seq_gt(65535, 0) && !seq_gt(5, 5));
    // A sends packets 65530 .. 70, B receives them with every third one lost and some doubled: A learns exactly which arrived
    let (mut a, mut b) = (Acks::new(), Acks::new());
    a.local_seq = 65530;
    let mut arrived = Vec::new();
    for i in 0..80u32 {
        let seq = a.on_send(i as u64 * 10, vec![]);
        if i % 3 != 2 {
            assert!(b.on_receive(seq), "first time");
            arrived.push(seq);
            if i % 5 == 0 {
                assert!(!b.on_receive(seq), "a doubled packet is noticed");
            }
        }
    }
    let (ack, bits) = b.header();
    a.on_ack(ack, bits, 900);
    assert_eq!(ack, *arrived.last().unwrap(), "the newest that arrived");
    let expected = arrived.iter().filter(|&&q| ack.wrapping_sub(q) <= 32).count() as u64;
    assert_eq!(a.packets_acked, expected, "A knows exactly which of the last 33 packets arrived (and not the lost ones)");
    // reliable: three messages, the packets that carried the first two are lost once, then everything is acknowledged; the receiver gets each once, in order
    let mut out = ReliableOut::default();
    let mut inn = ReliableIn::default();
    for i in 0..3u8 {
        out.push(vec![i]);
    }
    let first = out.due(0, 100, 1000);
    assert_eq!(first.len(), 3);
    assert!(out.due(50, 100, 1000).is_empty(), "not again before the timeout");
    let again = out.due(120, 100, 1000);
    assert_eq!(again.len(), 3, "sent again after the timeout");
    // the third message arrives first, then the second twice, then the first: delivered 0, 1, 2 once each
    assert!(inn.receive(2, vec![2]).is_empty());
    assert!(inn.receive(1, vec![1]).is_empty());
    assert!(inn.receive(1, vec![1]).is_empty());
    let got = inn.receive(0, vec![0]);
    assert_eq!(got, vec![vec![0], vec![1], vec![2]]);
    assert!(inn.receive(0, vec![0]).is_empty(), "a message that has been handled is not handled again");
    for s in 0..3 {
        out.acked(s);
    }
    assert!(out.is_empty());
}

#[test]
fn a_hundred_clients_connect_and_see_the_players_near_them() {
    let mut m = Match::new(100, 400.0, Link { latency_ms: 20, jitter_ms: 0, loss: 0.0, duplicate: 0.0 }, 1, false, ServerConfig::default());
    m.run(4000);
    assert_eq!(m.server.client_count(), 100);
    assert!(m.bots.iter().all(|b| b.client.connected()), "all connected");
    let truth = m.players();
    let mut missing = 0;
    let mut seen = 0;
    let mut too_far = 0;
    for b in &m.bots {
        let me = b.pos;
        for &(id, p) in &truth {
            if id == b.client.entity {
                continue;
            }
            let d = dist2(me, p);
            if d < 120.0 {
                seen += 1;
                if !b.client.remotes().contains_key(&id) {
                    missing += 1;
                }
            }
        }
        for (&id, r) in b.client.remotes() {
            if r.latest().is_some_and(|l| l.class == class::PLAYER) {
                let p = truth.iter().find(|t| t.0 == id).map(|t| t.1).unwrap();
                if dist2(me, p) > 150.0 + 60.0 {
                    too_far += 1;
                }
            }
        }
    }
    assert!(seen > 50, "the map has players near each other: {seen}");
    assert_eq!(missing, 0, "every player within 120 m of a client is known to it ({seen} pairs)");
    assert_eq!(too_far, 0, "no player far beyond the interest radius is sent");
    // loot is known too, and only the loot near
    let b = &m.bots[0];
    let items = b.client.remotes().values().filter(|r| r.latest().is_some_and(|l| l.class == class::ITEM)).count();
    let items_near = m.server.world.entities.values().filter(|e| e.state.class == class::ITEM && dist2(b.pos, e.state.pos_m()) < 140.0).count();
    assert!(items + 3 >= items_near && items <= items_near + 40, "loot: {items} known, {items_near} near");
}

#[test]
fn on_a_bad_network_they_still_see_each_other_and_every_command_is_handled_once() {
    let mut m = Match::new(100, 300.0, Link::internet(), 7, true, ServerConfig::default());
    m.run(3000);
    assert!(m.bots.iter().all(|b| b.client.connected()));
    for b in m.bots.iter_mut() {
        b.reset_staleness();
    }
    m.run(7000);
    // staleness: the longest a player within 30 m went without a record arriving (30 a second, 3 percent lost, 20 ms of jitter, doubled ones)
    let stalest: f64 = m.bots.iter().map(|b| b.stalest_ms).fold(0.0, f64::max);
    assert!(stalest < 450.0, "the longest a near player went without a record was {stalest} ms");
    // commands: stop shooting, let what is in flight arrive, compare
    for b in m.bots.iter_mut() {
        b.set_fire(false);
    }
    m.run(4000);
    let fired: u64 = m.bots.iter().map(|b| b.fired).sum();
    assert!(fired > 200, "they shot: {fired}");
    assert_eq!(m.server.game.shots, fired, "each command arrived exactly once (3 percent of the packets were lost and some doubled)");
    // the hits the server decided came back as events, once each
    let taken: u64 = m.bots.iter().map(|b| b.hits_taken).sum();
    assert!(m.server.game.hits > 5, "shots hit: {}", m.server.game.hits);
    assert_eq!(taken, m.server.game.hits, "every victim was told once");
    let (sent, bytes, lost) = m.net.totals();
    assert!(lost as f64 / sent as f64 > 0.02, "the network really was bad: {lost} of {sent} lost, {bytes} bytes");
}

#[test]
fn a_crowd_in_one_square_fits_every_packet_and_the_bandwidth_per_client_is_bounded() {
    // a hundred players within 60 m of each other: every client is a candidate for ninety-nine records
    let mut m = Match::new(100, 30.0, Link { latency_ms: 25, jitter_ms: 10, loss: 0.01, duplicate: 0.0 }, 3, false, ServerConfig::default());
    // (the nearest twenty players of each client are sent at full rate however many are near: within 8 m there are about five of the hundred)
    for b in m.bots.iter_mut() {
        b.client.near_m = 8.0;
    }
    m.run(3000);
    for b in m.bots.iter_mut() {
        b.reset_staleness();
    }
    m.run(5000);
    assert_eq!(m.server.client_count(), 100);
    // the players next to a client, the ones that matter most, were seen smoothly even though the packets were full
    let gap = m.bots.iter().map(|b| b.stalest_ms).fold(0.0, f64::max);
    println!("a crowd of 100: the longest a player within 8 m went between two records was {gap:.0} ms");
    assert!(gap < 250.0, "the nearest players are the freshest: the longest gap was {gap} ms");
    let secs = 8.0 - 1.0;
    let per_client_out = m.server.stats.bytes_out as f64 / 100.0 / secs;
    // the budget is 900 bytes of records and a few hundred of reliable messages a packet, thirty packets a second
    assert!(per_client_out < 36_000.0, "{per_client_out:.0} bytes a second to each client");
    let per_client_in: f64 = m.bots.iter().map(|b| b.client.bytes_in as f64).sum::<f64>() / 100.0 / secs;
    assert!(per_client_in < 36_000.0, "{per_client_in:.0} received");
    // everybody near is still known, and the nearest are fresh
    let truth = m.players();
    let mut known = 0usize;
    let mut total = 0usize;
    for b in &m.bots {
        for &(id, _) in &truth {
            if id != b.client.entity {
                total += 1;
                if b.client.remotes().contains_key(&id) {
                    known += 1;
                }
            }
        }
    }
    assert!(known as f64 / total as f64 > 0.97, "{known} of {total} players known in a crowd");
    let s = &m.server.stats;
    println!(
        "a crowd of 100: {:.1} KB/s to each client, {:.1} records a packet, the server tick took {:.0} us on average and {} us at most",
        per_client_out / 1000.0,
        s.records_out as f64 / (s.packets_out as f64).max(1.0),
        s.tick_us_total as f64 / s.ticks as f64,
        s.tick_us_max
    );
}

/// The gaps (in ticks) between records of players within 30 m, over all the bots: (how many gaps, the share at least this long for each of 1, 2, 3, 5, 10 ticks, the longest).
fn gap_report(m: &Match) -> (u64, [f64; 5], usize) {
    let mut all = [0u64; 32];
    for b in &m.bots {
        for (i, n) in b.client.near_gaps.iter().enumerate() {
            all[i] += n;
        }
    }
    let total: u64 = all.iter().sum();
    let at_least = |t: usize| all[t..].iter().sum::<u64>() as f64 / total.max(1) as f64;
    let longest = all.iter().rposition(|&n| n > 0).unwrap_or(0);
    (total, [at_least(1), at_least(2), at_least(3), at_least(5), at_least(10)], longest)
}

#[test]
fn two_hundred_players_in_one_crowd_still_see_their_neighbours_often_and_none_for_long() {
    // two hundred within 60 m of each other (everybody is in sight of everybody: more near players than a packet holds) with a lossy network
    let cfg = ServerConfig { max_clients: 200, ..ServerConfig::default() };
    let mut m = Match::new(200, 30.0, Link { latency_ms: 25, jitter_ms: 10, loss: 0.01, duplicate: 0.0 }, 5, false, cfg);
    // (the ones that matter are the nearest few: within 8 m there are about ten of the two hundred)
    for b in m.bots.iter_mut() {
        b.client.near_m = 8.0;
    }
    m.run(3000);
    for b in m.bots.iter_mut() {
        b.reset_staleness();
    }
    m.run(5000);
    assert_eq!(m.server.client_count(), 200);
    let (n, share, longest) = gap_report(&m);
    println!("a crowd of 200: {n} gaps between records of players within 8 m; at least 2 ticks {:.1}%, 3 {:.1}%, 5 {:.2}%, 10 {:.3}%; the longest {longest} ticks", share[1] * 100.0, share[2] * 100.0, share[3] * 100.0, share[4] * 100.0);
    assert!(n > 1000, "there were near players: {n} gaps");
    assert!(share[3] < 0.02, "no more than 2 percent of the gaps are 5 ticks or more: {:.2}%", share[3] * 100.0);
    assert!(longest <= 10, "no near player goes {longest} ticks without a record");
}

#[test]
fn a_heap_of_loot_does_not_push_the_players_out_of_the_packets_and_the_nearest_of_it_is_known() {
    let mut m = Match::new(30, 20.0, Link { latency_ms: 20, jitter_ms: 5, loss: 0.0, duplicate: 0.0 }, 11, false, ServerConfig::default());
    // 800 things on the ground within 40 m by 20 m: far more than a packet's loot share, and every client sees all of them
    for i in 0..800 {
        m.server.world.spawn(class::ITEM, [(i % 40) as f32 - 20.0, 0.0, (i / 40) as f32 - 10.0]);
    }
    m.run(3000);
    for b in m.bots.iter_mut() {
        b.reset_staleness();
    }
    m.run(4000);
    let truth = m.players();
    for b in &m.bots {
        for &(id, _) in &truth {
            if id != b.client.entity {
                assert!(b.client.remotes().contains_key(&id), "a player is known in spite of the loot");
            }
        }
        let items = b.client.remotes().values().filter(|r| r.latest().is_some_and(|l| l.class == class::ITEM)).count();
        assert!(items > 100, "the nearest loot is known: {items}");
    }
    let gap = m.bots.iter().map(|b| b.stalest_ms).fold(0.0, f64::max);
    assert!(gap < 200.0, "the players near are seen smoothly in the middle of a heap of loot: {gap} ms");
}

#[test]
fn a_move_the_character_could_not_make_is_refused_and_the_client_is_told_to_go_back() {
    let net = SimNet::new(Link { latency_ms: 10, jitter_ms: 0, loss: 0.0, duplicate: 0.0 }, 5);
    let ep = net.endpoint();
    let addr = ep.addr();
    let mut server = NetServer::new(ServerConfig::default(), ep, DemoGame::new(100.0, 0, 1));
    let mut c = NetClient::new(net.endpoint(), addr, "cheat", 99);
    let mut now = 1000u64;
    let mut spawn = [0.0f32; 3];
    let mut told = None;
    let mut tele_at = 0;
    for _ in 0..400 {
        now += 5;
        net.set_now(now);
        server.update(now);
        c.update(now);
        while let Some(e) = c.poll_event() {
            match e {
                Event::Connected { pos, .. } => spawn = pos,
                Event::Correct { pos } => told = Some(pos),
                _ => {}
            }
        }
        if c.connected() {
            let mut s = EntityState { class: class::PLAYER, health: 100, ..Default::default() };
            if tele_at == 0 && now > 1500 {
                tele_at = now;
            }
            // honest for a while (a walk of 5 m/s), then a jump of 500 m
            let t = (now - 1000) as f32 / 1000.0;
            let mut p = [spawn[0] + 5.0 * t.min(0.4), spawn[1], spawn[2]];
            if tele_at != 0 {
                p[0] += 500.0;
            }
            s.set_pos_m(p);
            c.set_state(s);
        }
    }
    let told = told.expect("told to go back");
    assert!((told[0] - spawn[0]).abs() < 4.0, "the place the server kept: {told:?} from {spawn:?}");
    let e = server.world.get(c.entity).unwrap();
    assert!((e.state.pos_m()[0] - spawn[0]).abs() < 4.0, "the server never took the 500 m jump");
    assert!(server.stats.corrections >= 1);
}

#[test]
fn a_shot_hits_where_the_target_was_when_the_shooter_pulled_the_trigger() {
    let mut w = World::default();
    let shooter = w.spawn(class::PLAYER, [0.0, 0.0, 0.0]);
    let target = w.spawn(class::PLAYER, [0.0, 0.0, 20.0]);
    for id in [shooter, target] {
        w.get_mut(id).unwrap().owner = Some(id as usize);
    }
    // the target stood at z 20 for ticks 1 to 10, then ran sideways 8 m by tick 20
    for tick in 1..=20u32 {
        let x = if tick > 10 { (tick - 10) as f32 * 0.8 } else { 0.0 };
        w.get_mut(target).unwrap().state.set_pos_m([x, 0.0, 20.0]);
        w.record(tick, 40);
    }
    let o = [0.0, 1.0, 0.0];
    let dir = [0.0, 0.0, 1.0];
    // now it is at x = 8: a shot straight ahead misses it, but one the shooter fired when it saw tick 8 hits
    assert!(w.hitscan(shooter, o, dir, 20, 100.0, None).is_none());
    let (id, d) = w.hitscan(shooter, o, dir, 8, 100.0, None).expect("hit in the past");
    assert_eq!(id, target);
    assert!((d - 19.55).abs() < 0.1, "the near side of the cylinder: {d}");
    // a wall at 10 m is in the way; out of range; a shot over its head; the dead are not hit
    assert!(w.hitscan(shooter, o, dir, 8, 100.0, Some(10.0)).is_none());
    assert!(w.hitscan(shooter, o, dir, 8, 15.0, None).is_none());
    assert!(w.hitscan(shooter, [0.0, 3.0, 0.0], dir, 8, 100.0, None).is_none());
    w.get_mut(target).unwrap().state.health = 0;
    assert!(w.hitscan(shooter, o, dir, 8, 100.0, None).is_none());
}

#[test]
fn a_client_that_goes_quiet_is_dropped_and_its_slot_is_used_again_and_a_full_server_refuses() {
    let cfg = ServerConfig { max_clients: 4, timeout_ms: 2000, ..Default::default() };
    let mut m = Match::new(4, 100.0, Link { latency_ms: 10, jitter_ms: 0, loss: 0.0, duplicate: 0.0 }, 9, false, cfg);
    m.run(1500);
    assert_eq!(m.server.client_count(), 4);
    // the fifth is refused
    let addr = m.server.transport.addr();
    let mut extra = Bot::new(m.net.endpoint(), addr, "late", 77, 100.0, false);
    for _ in 0..400 {
        m.now += 5;
        m.net.set_now(m.now);
        m.server.update(m.now);
        for b in m.bots.iter_mut() {
            b.step(m.now);
        }
        extra.client.update(m.now);
    }
    assert!(!extra.client.connected());
    assert!(matches!(extra.client.poll_event(), Some(Event::Refused(2))), "told that the server is full");
    // bot 0 stops answering: it is dropped after the timeout, and the late one takes the slot
    let mut quiet = m.bots.remove(0);
    quiet.client.disconnect();
    drop(quiet);
    for _ in 0..20 {
        m.now += 200;
        m.net.set_now(m.now);
        m.server.update(m.now);
        for b in m.bots.iter_mut() {
            b.step(m.now);
        }
    }
    // (the disconnect message freed the slot at once; the server is not full any more)
    assert_eq!(m.server.client_count(), 3);
    let mut again = Bot::new(m.net.endpoint(), addr, "again", 78, 100.0, false);
    for _ in 0..200 {
        m.now += 5;
        m.net.set_now(m.now);
        m.server.update(m.now);
        for b in m.bots.iter_mut() {
            b.step(m.now);
        }
        again.step(m.now);
    }
    assert!(again.client.connected());
    assert_eq!(m.server.client_count(), 4);
    // a client that just stops (no goodbye) is dropped by the timeout
    let silent = m.bots.pop().unwrap();
    drop(silent);
    for _ in 0..40 {
        m.now += 100;
        m.net.set_now(m.now);
        m.server.update(m.now);
        for b in m.bots.iter_mut() {
            b.step(m.now);
        }
        again.step(m.now);
    }
    assert_eq!(m.server.client_count(), 3, "the silent one was dropped after the timeout");
}

#[test]
fn the_handshake_gets_through_a_network_that_loses_half_of_everything() {
    let mut m = Match::new(20, 100.0, Link { latency_ms: 30, jitter_ms: 30, loss: 0.5, duplicate: 0.1 }, 11, false, ServerConfig::default());
    m.run(15_000);
    let connected = m.bots.iter().filter(|b| b.client.connected()).count();
    assert_eq!(connected, 20, "{connected} of 20 got in through 50 percent loss each way (75 percent of exchanges fail)");
    assert!(m.server.client_count() <= 20, "no client got two slots");
}

#[test]
fn a_connect_that_arrives_twice_is_given_the_same_challenge_both_times() {
    // (a doubled datagram, or one sent again because the challenge was lost: a new challenge for the second would make the client's answer to the first one wrong, for good)
    let net = SimNet::new(Link::perfect(), 5);
    let server_end = net.endpoint();
    let addr = server_end.addr();
    let mut server = NetServer::new(ServerConfig::default(), server_end, DemoGame::new(100.0, 0, 1));
    let mut end = net.endpoint();
    let mut w = W::new();
    w.u8(MAGIC);
    w.u8(kind::CONNECT);
    w.u8(PROTOCOL);
    w.u32(1234);
    w.bytes(b"twice");
    end.send(addr, &w.buf);
    end.send(addr, &w.buf);
    net.set_now(10);
    server.update(10);
    let a = end.recv().expect("a challenge").1;
    let b = end.recv().expect("a second challenge").1;
    assert_eq!(a[1], kind::CHALLENGE);
    assert_eq!(a, b, "the same challenge twice");
}

#[test]
fn a_game_decides_what_happens_to_a_command_and_to_a_tick() {
    struct Counter {
        ticks: u32,
        commands: Vec<(usize, Vec<u8>)>,
    }
    impl Game for Counter {
        fn on_join(&mut self, _w: &mut World, _slot: usize, _name: &str) -> EntityState {
            EntityState { class: class::PLAYER, health: 100, ..Default::default() }
        }
        fn on_command(&mut self, ctx: &mut Ctx, slot: usize, data: &[u8]) {
            self.commands.push((slot, data.to_vec()));
            ctx.out.to(slot, vec![9, data.len() as u8]);
        }
        fn on_tick(&mut self, ctx: &mut Ctx) {
            self.ticks += 1;
            if ctx.tick == 30 {
                ctx.out.all(vec![42]);
            }
        }
    }
    let net = SimNet::new(Link { latency_ms: 5, jitter_ms: 5, loss: 0.2, duplicate: 0.05 }, 21);
    let ep = net.endpoint();
    let addr = ep.addr();
    let mut server = NetServer::new(ServerConfig::default(), ep, Counter { ticks: 0, commands: Vec::new() });
    let mut c = NetClient::new(net.endpoint(), addr, "x", 5);
    let mut now = 0u64;
    let mut sent = false;
    let mut got = Vec::new();
    for _ in 0..1500 {
        now += 5;
        net.set_now(now);
        server.update(now);
        c.update(now);
        while let Some(e) = c.poll_event() {
            if let Event::Game(d) = e {
                got.push(d);
            }
        }
        if c.connected() && !sent {
            sent = true;
            for i in 0..5u8 {
                c.command(vec![cmd::FIRE + 100 + i, i]);
            }
        }
    }
    assert_eq!(server.game.commands.len(), 5, "every command once");
    for (i, (slot, d)) in server.game.commands.iter().enumerate() {
        assert_eq!((*slot, d[1]), (0, i as u8), "in order");
    }
    // five replies and the broadcast at tick 30, each once
    assert_eq!(got.iter().filter(|d| d[0] == 9).count(), 5);
    assert_eq!(got.iter().filter(|d| d[0] == 42).count(), 1);
    assert!(server.game.ticks > 200, "the server ticked {} times in {} ms", server.game.ticks, now);
}

#[test]
fn two_hundred_players_over_a_wide_map_cost_the_server_little() {
    let cfg = ServerConfig { max_clients: 200, ..Default::default() };
    let mut m = Match::new(200, 1500.0, Link { latency_ms: 30, jitter_ms: 15, loss: 0.02, duplicate: 0.0 }, 5, true, cfg);
    m.run(6000);
    assert_eq!(m.server.client_count(), 200);
    let s = &m.server.stats;
    let avg = s.tick_us_total as f64 / s.ticks as f64;
    println!("200 players: the server tick takes {avg:.0} us on average, {} us at most ({} ticks), {:.1} KB/s out to each", s.tick_us_max, s.ticks, s.bytes_out as f64 / 200.0 / 5.0 / 1000.0);
    // a tick has 33 ms: even in a build without optimisation the work of 200 players is a small part of it
    assert!(avg < 20_000.0, "average tick {avg} us");
    // most of the players of a wide map have nobody near: they get a packet every third tick (ten a second), not every tick
    let per_client = s.packets_out as f64 / 200.0 / 6.0;
    println!("200 players: {per_client:.1} packets a second to each (a tick each would be 30)");
    // (a shot or a hit is a reliable message, which keeps its client on the every-tick path until it is acknowledged: these bots shoot all the time)
    assert!(per_client < 20.0, "{per_client:.1} packets a second to each client");
}

#[test]
fn a_client_with_nobody_near_gets_a_packet_every_third_tick_and_still_all_the_loot_and_a_near_player_every_tick() {
    // players placed where the test wants them: slot 0 at the origin, slot 1 at 500 m, and a player called "near" at 10 m from slot 0
    struct Spread;
    impl Game for Spread {
        fn on_join(&mut self, _w: &mut World, slot: usize, name: &str) -> EntityState {
            let mut s = EntityState { class: class::PLAYER, health: 100, ..Default::default() };
            s.set_pos_m(if name == "near" { [10.0, 0.0, 0.0] } else { [slot as f32 * 500.0, 0.0, 0.0] });
            s
        }
    }
    let net = SimNet::new(Link { latency_ms: 10, jitter_ms: 0, loss: 0.0, duplicate: 0.0 }, 31);
    let server_end = net.endpoint();
    let addr = server_end.addr();
    let mut server = NetServer::new(ServerConfig::default(), server_end, Spread);
    // 40 things on the ground 20 to 40 m from the first player
    for i in 0..40 {
        server.world.spawn(class::ITEM, [20.0 + (i % 8) as f32 * 3.0, 0.0, (i / 8) as f32 * 3.0]);
    }
    let mut a = NetClient::new(net.endpoint(), addr, "a", 101);
    let mut b = NetClient::new(net.endpoint(), addr, "b", 102);
    let mut now = 1000u64;
    let mut run = |ms: u64, server: &mut NetServer<SimEndpoint, Spread>, clients: &mut [&mut NetClient<SimEndpoint>]| {
        let end = now + ms;
        while now < end {
            now += 5;
            net.set_now(now);
            server.update(now);
            for c in clients.iter_mut() {
                c.update(now);
            }
        }
    };
    run(2000, &mut server, &mut [&mut a, &mut b]);
    assert!(a.connected() && b.connected());
    // two players far from each other: ten packets a second each, not thirty
    let before = server.stats.packets_out;
    run(2000, &mut server, &mut [&mut a, &mut b]);
    let quiet = server.stats.packets_out - before;
    assert!(quiet >= 30 && quiet <= 60, "two quiet players, two seconds: {quiet} packets (every tick would be 120)");
    let loot = a.remotes().values().filter(|r| r.latest().is_some_and(|l| l.class == class::ITEM)).count();
    assert!(loot >= 38, "the loot around the first player is known in spite of the packets being every third tick: {loot} of 40");
    // a player arrives 10 m from the first: both of them are sent a packet every tick, the far one still every third
    let mut c = NetClient::new(net.endpoint(), addr, "near", 103);
    run(1000, &mut server, &mut [&mut a, &mut b, &mut c]);
    let before = server.stats.packets_out;
    run(2000, &mut server, &mut [&mut a, &mut b, &mut c]);
    let busy = server.stats.packets_out - before;
    assert!(busy >= 120 && busy <= 170, "one pair near each other and one alone, two seconds: {busy} packets (60 + 60 + 20 = 140)");
    assert!(a.remotes().values().any(|r| r.latest().is_some_and(|l| l.class == class::PLAYER && l.pos_m()[0] < 20.0)), "the near player is known");
}

// --- hostile clients (found by the 2026-10-22 security review: docs/SECURE.md, docs/NETWORK_SCALE.md) ---

fn raw_connect(net: &SimNet, addr: super::transport::Addr, name: &str, salt: u32) -> SimEndpoint {
    let mut ep = net.endpoint();
    let mut w = W::new();
    w.u8(MAGIC);
    w.u8(kind::CONNECT);
    w.u8(PROTOCOL);
    w.u32(salt);
    w.bytes(name.as_bytes());
    ep.send(addr, &w.buf);
    ep
}

/// Finishes the handshake begun by `raw_connect` once the server has answered its CHALLENGE (the caller has already stepped the server).
fn raw_finish(ep: &mut SimEndpoint, addr: super::transport::Addr, salt: u32) {
    let mut server_salt = None;
    while let Some((_, d)) = ep.recv() {
        if d.len() >= 10 && d[1] == kind::CHALLENGE {
            server_salt = Some(u32::from_le_bytes([d[6], d[7], d[8], d[9]]));
        }
    }
    let Some(ss) = server_salt else { return };
    let mut w = W::new();
    w.u8(MAGIC);
    w.u8(kind::RESPONSE);
    w.u32(salt ^ ss);
    ep.send(addr, &w.buf);
}

fn raw_accept(ep: &mut SimEndpoint) -> Option<(u16, u32)> {
    let mut out = None;
    while let Some((_, d)) = ep.recv() {
        if d.len() >= 16 && d[1] == kind::ACCEPT {
            out = Some((u16::from_le_bytes([d[2], d[3]]), u32::from_le_bytes([d[8], d[9], d[10], d[11]])));
        }
    }
    out
}

fn one_state(pos: [i32; 3]) -> Vec<u8> {
    let s = EntityState { class: class::PLAYER, health: 100, pos, ..Default::default() };
    let mut body = W::new();
    state::write_own(&mut body, &s);
    let mut w = W::new();
    w.u8(super::proto::msg::STATE);
    w.bytes(&body.buf);
    w.buf
}

fn data_packet(conn: u16, seq: u16, parts: &[Vec<u8>]) -> Vec<u8> {
    let mut w = W::new();
    w.u8(MAGIC);
    w.u8(kind::DATA);
    w.u16(conn);
    w.u16(seq);
    w.u16(0);
    w.u32(0);
    for p in parts {
        w.raw(p);
    }
    w.buf
}

/// A raw client that is in the game: (its endpoint, its connection number, its entity, the server's address, the clock).
fn raw_player(net: &SimNet, addr: super::transport::Addr, server: &mut NetServer<SimEndpoint, DemoGame>, name: &str, salt: u32, now: &mut u64) -> (SimEndpoint, u16, u32) {
    let mut ep = raw_connect(net, addr, name, salt);
    *now += 10;
    net.set_now(*now);
    server.update(*now);
    raw_finish(&mut ep, addr, salt);
    *now += 10;
    net.set_now(*now);
    server.update(*now);
    let (conn, entity) = raw_accept(&mut ep).expect("accepted");
    (ep, conn, entity)
}

/// On a plain server a name is all the identity there is: a second connection with the name of a player who is in is REFUSED (it used to push the first one out, so anyone could kick anyone by typing the name).
#[test]
fn a_second_connection_under_the_same_name_is_refused_on_a_plain_server() {
    let net = SimNet::new(Link::perfect(), 3);
    let srv_ep = net.endpoint();
    let addr = srv_ep.addr();
    let mut server = NetServer::new(ServerConfig::default(), srv_ep, DemoGame::new(50.0, 0, 1));
    let mut now = 1000u64;
    let mut first = NetClient::new(net.endpoint(), addr, "lighthouse_keeper", 1);
    let mut first_entity = 0u32;
    for _ in 0..60 {
        now += 5;
        net.set_now(now);
        server.update(now);
        first.update(now);
        while let Some(e) = first.poll_event() {
            if let Event::Connected { entity, .. } = e {
                first_entity = entity;
            }
        }
    }
    assert!(first.connected() && first_entity != 0, "the first player is in");
    let mut second = NetClient::new(net.endpoint(), addr, "lighthouse_keeper", 2);
    let mut told = None;
    for _ in 0..60 {
        now += 5;
        net.set_now(now);
        server.update(now);
        first.update(now);
        second.update(now);
        while let Some(e) = second.poll_event() {
            if let Event::Refused(why) = e {
                told = Some(why);
            }
        }
    }
    assert_eq!(server.client_count(), 1, "still one player");
    assert!(!second.connected(), "the newcomer is not in");
    assert_eq!(told, Some(3), "it is told that the name is in the game");
    assert!(server.world.get(first_entity).is_some() && first.connected(), "the first player's character is untouched");
}

/// Many movement records in ONE datagram all run in one server tick: the climb they may make is a pool (like the horizontal one), not an allowance for each record.
#[test]
fn many_state_records_in_one_datagram_cannot_climb_past_the_vertical_budget() {
    let net = SimNet::new(Link::perfect(), 4);
    let srv_ep = net.endpoint();
    let addr = srv_ep.addr();
    let cfg = ServerConfig::default();
    let (max_up, burst_s) = (cfg.max_up, cfg.burst_s);
    let mut server = NetServer::new(cfg, srv_ep, DemoGame::new(50.0, 0, 1));
    let mut now = 1000u64;
    let (mut ep, conn, entity) = raw_player(&net, addr, &mut server, "climber", 77, &mut now);
    let start = server.world.get(entity).unwrap().state.pos;
    // one datagram, 30 records, each one metre above the last
    let mut parts = Vec::new();
    let mut y = start[1];
    for _ in 0..30 {
        y += (1.0 * state::POS_SCALE) as i32;
        parts.push(one_state([start[0], y, start[2]]));
    }
    ep.send(addr, &data_packet(conn, 1, &parts));
    now += 5;
    net.set_now(now);
    server.update(now);
    let ended = server.world.get(entity).unwrap().state.pos;
    let climbed_m = (ended[1] - start[1]) as f32 / state::POS_SCALE;
    assert!(climbed_m <= max_up * burst_s + 0.2, "30 stacked records climbed {climbed_m:.1} m: the budget is {:.1} m", max_up * burst_s);
    assert!(climbed_m >= 1.0, "an honest part of it is let through");
    assert!(server.stats.corrections > 0, "the rest was refused");
}

/// One client cannot force unbounded work into a tick: past `MSG_RATE` messages a second the rest of what it sends is dropped (and counted).
#[test]
fn a_client_that_floods_messages_is_throttled() {
    let net = SimNet::new(Link::perfect(), 5);
    let srv_ep = net.endpoint();
    let addr = srv_ep.addr();
    let mut server = NetServer::new(ServerConfig::default(), srv_ep, DemoGame::new(50.0, 0, 1));
    let mut now = 1000u64;
    let (mut ep, conn, entity) = raw_player(&net, addr, &mut server, "flooder", 78, &mut now);
    let here = server.world.get(entity).unwrap().state.pos;
    let parts: Vec<Vec<u8>> = (0..70).map(|_| one_state(here)).collect();
    for seq in 1..=10u16 {
        ep.send(addr, &data_packet(conn, seq, &parts));
    }
    now += 5;
    net.set_now(now);
    server.update(now);
    assert!(server.stats.throttled > 0, "700 messages in one instant are more than a client may send: {}", server.stats.throttled);
    assert_eq!(server.client_count(), 1, "the flooder is throttled, not dropped, and nobody else is touched");
}

#[test]
fn one_address_may_hold_only_as_many_slots_as_the_server_allows() {
    // (every bot of the virtual network has the address 127.0.0.1)
    let cfg = ServerConfig { max_clients: 8, max_per_ip: 2, ..Default::default() };
    let mut m = Match::new(3, 100.0, Link { latency_ms: 10, jitter_ms: 0, loss: 0.0, duplicate: 0.0 }, 9, false, cfg);
    m.run(1500);
    assert_eq!(m.server.client_count(), 2, "two of the three got in");
    assert!(m.server.stats.refused >= 1, "and the third was refused (REFUSED_ADDRESS, code 7: the bots' own event queues are drained by the harness)");
    assert_eq!(crate::scale::server::REFUSED_ADDRESS, 7);
    // with no limit (the default) all three are in
    let mut m = Match::new(3, 100.0, Link { latency_ms: 10, jitter_ms: 0, loss: 0.0, duplicate: 0.0 }, 9, false, ServerConfig { max_clients: 8, ..Default::default() });
    m.run(1500);
    assert_eq!(m.server.client_count(), 3);
}

