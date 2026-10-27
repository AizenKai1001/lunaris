//! Hostile input, the other way (docs/SERVER_ROBUSTNESS.md: "the client side is not fuzzed"): a client that plays a real match against a real server through a wrapper that damages what the server sends.
//! Bits are flipped, datagrams are cut short, doubled, made longer, filled with the ends of the number range or replaced by noise from the server's address, in the handshake and in the match. A server
//! that is hostile, broken or only unlucky must never make a client panic, hang or run out of memory; and the client must still get into the game when only some of what it is sent is damaged.
use super::client::Event;
use super::server::{NetServer, ServerConfig, World};
use super::swarm::{Bot, DemoGame};
use super::transport::{Addr, Link, SimEndpoint, SimNet, Transport};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

/// How hostile the wrapper is, and what it has done (shared with the test, which owns the client that owns the wrapper).
#[derive(Default)]
struct Knobs {
    p: f32,
    noise: f32,
    damaged: u64,
    passed: u64,
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, p: f32) -> bool {
        ((self.next() >> 40) as f32 / (1u64 << 24) as f32) < p
    }
}

/// What the client sees: the datagrams of the real network, each damaged with chance `p`, some doubled, and now and then noise from the server's address.
struct Hostile {
    inner: SimEndpoint,
    server: Addr,
    rng: Rng,
    knobs: Rc<RefCell<Knobs>>,
    queue: VecDeque<(Addr, Vec<u8>)>,
}

impl Hostile {
    fn new(inner: SimEndpoint, server: Addr, seed: u64, knobs: Rc<RefCell<Knobs>>) -> Hostile {
        Hostile { inner, server, rng: Rng(seed), knobs, queue: VecDeque::new() }
    }

    fn damage(&mut self, data: &mut Vec<u8>) {
        match self.rng.below(8) {
            // bits flipped
            0 | 1 => {
                for _ in 0..1 + self.rng.below(6) {
                    if !data.is_empty() {
                        let i = self.rng.below(data.len());
                        data[i] ^= 1 << self.rng.below(8);
                    }
                }
            }
            // cut short
            2 => {
                let n = self.rng.below(data.len() + 1);
                data.truncate(n);
            }
            // made longer
            3 => {
                for _ in 0..1 + self.rng.below(64) {
                    data.push(self.rng.next() as u8);
                }
            }
            // a run of the ends of the byte range (a length or a count that says "huge")
            4 => {
                if !data.is_empty() {
                    let (start, len) = (self.rng.below(data.len()), 1 + self.rng.below(6));
                    let fill = [0x00u8, 0xFF, 0x7F, 0x80][self.rng.below(4)];
                    for b in data.iter_mut().skip(start).take(len) {
                        *b = fill;
                    }
                }
            }
            // a byte of the header (magic, kind) or the first bytes of the body replaced
            5 => {
                if !data.is_empty() {
                    let i = self.rng.below(data.len().min(12));
                    data[i] = self.rng.next() as u8;
                }
            }
            // the same datagram twice, the second damaged too
            6 => {
                let mut again = data.clone();
                if !again.is_empty() {
                    let i = self.rng.below(again.len());
                    again[i] = self.rng.next() as u8;
                }
                self.queue.push_back((self.server, again));
            }
            // nothing is left of it but noise of the same length
            _ => {
                for b in data.iter_mut() {
                    *b = self.rng.next() as u8;
                }
            }
        }
    }
}

impl Transport for Hostile {
    fn send(&mut self, to: Addr, data: &[u8]) {
        self.inner.send(to, data);
    }

    fn recv(&mut self) -> Option<(Addr, Vec<u8>)> {
        if let Some(q) = self.queue.pop_front() {
            return Some(q);
        }
        let (p, noise) = {
            let k = self.knobs.borrow();
            (k.p, k.noise)
        };
        if self.rng.chance(noise) {
            let n = self.rng.below(300);
            let mut noise: Vec<u8> = (0..n).map(|_| self.rng.next() as u8).collect();
            // (now and then it starts like a real packet)
            if self.rng.chance(0.5) && noise.len() > 2 {
                noise[0] = super::proto::MAGIC;
                noise[1] = (self.rng.below(12)) as u8;
            }
            return Some((self.server, noise));
        }
        let (from, mut data) = self.inner.recv()?;
        if from == self.server && self.rng.chance(p) {
            self.knobs.borrow_mut().damaged += 1;
            self.damage(&mut data);
        } else {
            self.knobs.borrow_mut().passed += 1;
        }
        Some((from, data))
    }
}

/// A match of a few bots, `hostile` of them behind a damaging wrapper: runs `ms` milliseconds and says how many of the damaged ones got in.
fn run_match(seed: u64, p: f32, noise: f32, hostile: usize, ms: u64) -> (usize, u64, u64, u64) {
    let net = SimNet::new(Link { latency_ms: 20, jitter_ms: 10, loss: 0.05, duplicate: 0.05 }, seed);
    let ep = net.endpoint();
    let addr = ep.addr();
    let mut game = DemoGame::new(100.0, 300, seed);
    let mut items = World::default();
    game.scatter(&mut items);
    let mut server = NetServer::new(ServerConfig::default(), ep, game);
    for (_, e) in items.entities.drain() {
        server.world.spawn_state(e.state);
    }
    // honest bots to make the world busy (positions, shots, hits, kills the hostile clients are told about), and the damaged ones
    let mut honest: Vec<Bot<SimEndpoint>> = (0..6).map(|i| Bot::new(net.endpoint(), addr, &format!("honest{i}"), seed + i as u64 + 1, 100.0, true)).collect();
    let knobs: Vec<Rc<RefCell<Knobs>>> = (0..hostile).map(|_| Rc::new(RefCell::new(Knobs { p, noise, ..Knobs::default() }))).collect();
    let mut bots: Vec<Bot<Hostile>> = (0..hostile).map(|i| Bot::new(Hostile::new(net.endpoint(), addr, seed * 31 + i as u64, knobs[i].clone()), addr, &format!("hostile{i}"), seed + 100 + i as u64, 100.0, true)).collect();
    let mut now = 1000u64;
    let mut connected_ever = vec![false; hostile];
    let end = now + ms;
    while now < end {
        now += 5;
        net.set_now(now);
        server.update(now);
        for b in honest.iter_mut() {
            b.step(now);
        }
        for (i, b) in bots.iter_mut().enumerate() {
            b.step(now);
            connected_ever[i] |= b.client.connected();
            // what a game does with what it has been told: look at every other player
            let ids: Vec<u32> = b.client.remotes().keys().copied().collect();
            for id in ids.iter().take(40) {
                let _ = b.client.sample(*id, now);
            }
            let _ = (b.client.server_ms(now), b.client.rtt_ms());
        }
    }
    let damaged: u64 = knobs.iter().map(|k| k.borrow().damaged).sum();
    let passed: u64 = knobs.iter().map(|k| k.borrow().passed).sum();
    (connected_ever.iter().filter(|c| **c).count(), damaged, passed, server.client_count() as u64)
}

#[test]
fn a_client_whose_server_damages_a_third_of_its_datagrams_never_panics_and_still_gets_in() {
    let (mut total_damaged, mut total_in) = (0, 0);
    for seed in 1..=16u64 {
        let (connected, damaged, _passed, _clients) = run_match(seed, 0.33, 0.02, 4, 12_000);
        total_damaged += damaged;
        total_in += connected;
    }
    // (a bit flipped in the challenge can leave a client answering a challenge that was never made, for good: on a real network the checksum of a datagram keeps that from happening, and a server that
    // wants to keep a client out can simply say nothing; so not every client has to get in, but most do)
    assert!(total_in >= 16 * 4 / 2, "only {total_in} of 64 clients got in although two thirds of what they were sent was whole");
    assert!(total_damaged > 3_000, "{total_damaged} datagrams were damaged: the test has to reach the client's parsers");
}

#[test]
fn a_client_whose_server_damages_everything_never_panics() {
    // (nothing whole ever arrives: the client cannot get in, and must not fall over trying)
    let mut total_damaged = 0;
    for seed in 20..=27u64 {
        let (_connected, damaged, _passed, _clients) = run_match(seed, 1.0, 0.3, 3, 4_000);
        total_damaged += damaged;
    }
    assert!(total_damaged > 40, "{total_damaged} datagrams were damaged");
}

#[test]
fn a_client_that_is_in_the_game_when_the_damage_begins_never_panics() {
    // the handshake is honest for 3 seconds (so that the client is in, with positions and events flowing), then the server turns hostile
    let net = SimNet::new(Link { latency_ms: 15, jitter_ms: 5, loss: 0.02, duplicate: 0.02 }, 99);
    let ep = net.endpoint();
    let addr = ep.addr();
    let mut game = DemoGame::new(100.0, 300, 99);
    let mut items = World::default();
    game.scatter(&mut items);
    let mut server = NetServer::new(ServerConfig::default(), ep, game);
    for (_, e) in items.entities.drain() {
        server.world.spawn_state(e.state);
    }
    let mut honest: Vec<Bot<SimEndpoint>> = (0..8).map(|i| Bot::new(net.endpoint(), addr, &format!("honest{i}"), 100 + i as u64, 100.0, true)).collect();
    let knobs = Rc::new(RefCell::new(Knobs::default()));
    let mut victim = Bot::new(Hostile::new(net.endpoint(), addr, 4242, knobs.clone()), addr, "victim", 7, 100.0, true);
    let mut now = 1000u64;
    for phase in 0..4 {
        // phase 0: honest; then 10, 40 and 90 percent damage and noise
        let (p, noise) = [(0.0, 0.0), (0.1, 0.01), (0.4, 0.05), (0.9, 0.2)][phase];
        *knobs.borrow_mut() = Knobs { p, noise, ..Knobs::default() };
        let end = now + 3_000;
        while now < end {
            now += 5;
            net.set_now(now);
            server.update(now);
            for b in honest.iter_mut() {
                b.step(now);
            }
            victim.step(now);
            if phase == 0 && now > 2_500 {
                assert!(victim.client.connected(), "the honest phase gets the client in");
            }
        }
        if phase == 0 {
            assert!(!victim.client.remotes().is_empty(), "in the honest phase the client sees the other players");
        }
    }
    // (the bot has taken every event the client had: the test looks at what is left)
    while let Some(e) = victim.client.poll_event() {
        let _ = matches!(e, Event::Game(_) | Event::Correct { .. });
    }
}
