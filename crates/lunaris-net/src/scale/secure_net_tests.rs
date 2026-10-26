//! A server that wants connect tokens, end to end in memory: the token's holder joins under the name in the token, nobody without one does, tokens of another key or game or age or a second use are refused,
//! nothing readable is on the wire after the token, and packets that are forged, changed or sent again are dropped (docs/SECURE.md).
use super::client::{Event, NetClient};
use super::proto::{kind, MAGIC};
use super::secure::*;
use super::server::{Auth, Game, NetServer, ServerConfig, World};
use super::state::{class, EntityState};
use super::transport::{Addr, Link, SimEndpoint, SimNet, Transport};
use std::sync::{Arc, Mutex};

const NOW: u64 = 1_000_000;
const GAME: u64 = 4242;

fn key() -> [u8; 32] {
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(11).wrapping_add(5);
    }
    k
}

/// Who joined, by the name the server was given.
struct Names(Arc<Mutex<Vec<String>>>);

impl Game for Names {
    fn on_join(&mut self, _w: &mut World, _slot: usize, name: &str) -> EntityState {
        self.0.lock().unwrap().push(name.to_string());
        EntityState { class: class::PLAYER, health: 100, ..Default::default() }
    }
}

type Wire = Arc<Mutex<Vec<(bool, Vec<u8>)>>>;

/// A transport that notes every datagram that passes (true: sent by this end) and lets a test put datagrams of its own on the wire from this end's address.
struct Tap {
    inner: SimEndpoint,
    wire: Wire,
    inject: Arc<Mutex<Vec<(Addr, Vec<u8>)>>>,
}

impl Transport for Tap {
    fn send(&mut self, to: Addr, data: &[u8]) {
        self.wire.lock().unwrap().push((true, data.to_vec()));
        self.inner.send(to, data);
    }

    fn recv(&mut self) -> Option<(Addr, Vec<u8>)> {
        for (to, d) in self.inject.lock().unwrap().drain(..) {
            self.inner.send(to, &d);
        }
        let r = self.inner.recv();
        if let Some((_, d)) = &r {
            self.wire.lock().unwrap().push((false, d.clone()));
        }
        r
    }
}

struct Rig {
    net: SimNet,
    server: NetServer<Tap, Names>,
    server_addr: Addr,
    joined: Arc<Mutex<Vec<String>>>,
    wire: Wire,
    now: u64,
}

fn token(server_key: &[u8; 32], protocol: u64, client_id: u64, name: &str, ttl_from: u64, ttl: u64, seed: u8) -> ConnectToken {
    ConnectToken::issue(server_key, protocol, client_id, name, ttl_from, ttl, [seed; 24], [seed.wrapping_add(1); 32], [seed.wrapping_add(2); 32])
}

impl Rig {
    fn new(link: Link, secure: bool) -> Rig {
        let net = SimNet::new(link, 11);
        let ep = net.endpoint();
        let server_addr = ep.addr();
        let wire: Wire = Arc::default();
        let joined = Arc::new(Mutex::new(Vec::new()));
        let mut cfg = ServerConfig::default();
        if secure {
            cfg.auth = Some(Auth { key: key(), protocol_id: GAME, unix_now: Some(NOW) });
        }
        let server = NetServer::new(cfg, Tap { inner: ep, wire: wire.clone(), inject: Arc::default() }, Names(joined.clone()));
        Rig { net, server, server_addr, joined, wire, now: 1000 }
    }

    fn client(&self, token: Option<ConnectToken>, name: &str, salt: u32) -> (NetClient<Tap>, Wire, Arc<Mutex<Vec<(Addr, Vec<u8>)>>>) {
        let wire: Wire = Arc::default();
        let inject = Arc::default();
        let tap = Tap { inner: self.net.endpoint(), wire: wire.clone(), inject: Arc::clone(&inject) };
        let c = match token {
            Some(t) => NetClient::with_token(tap, self.server_addr, t, salt),
            None => NetClient::new(tap, self.server_addr, name, salt),
        };
        (c, wire, inject)
    }

    fn run(&mut self, clients: &mut [&mut NetClient<Tap>], ms: u64) {
        let end = self.now + ms;
        while self.now < end {
            self.now += 10;
            self.net.set_now(self.now);
            self.server.update(self.now);
            for c in clients.iter_mut() {
                c.update(self.now);
            }
        }
    }

    fn joined(&self) -> Vec<String> {
        self.joined.lock().unwrap().clone()
    }
}

fn kinds_of(wire: &Wire) -> Vec<u8> {
    wire.lock().unwrap().iter().map(|(_, d)| d[1]).collect()
}

#[test]
fn a_token_holder_joins_under_the_name_in_its_token_and_nothing_readable_is_on_the_wire() {
    let mut r = Rig::new(Link { latency_ms: 20, jitter_ms: 0, loss: 0.0, duplicate: 0.0 }, true);
    let t = token(&key(), GAME, 77, "Ada", NOW, 60, 3);
    let (mut c, cwire, _) = r.client(Some(t), "ignored", 5);
    r.run(&mut [&mut c], 1500);
    assert!(c.connected(), "joined");
    assert_eq!(r.joined(), vec!["Ada".to_string()], "the name is the one in the token");
    // it plays: the character moves where it says (the server checked who sent it)
    let mut s = EntityState { class: class::PLAYER, ..Default::default() };
    s.set_pos_m([1.0, 0.0, 2.0]);
    c.set_state(s);
    r.run(&mut [&mut c], 600);
    let at = r.server.world.entities.values().find(|e| e.owner.is_some()).map(|e| e.state.pos_m()).unwrap();
    assert!((at[0] - 1.0).abs() < 0.1 && (at[2] - 2.0).abs() < 0.1, "{at:?}");
    // the wire: the name never goes over it, and after the handshake only sealed kinds do
    let all: Vec<(bool, Vec<u8>)> = r.wire.lock().unwrap().iter().chain(cwire.lock().unwrap().iter()).cloned().collect();
    assert!(!all.iter().any(|(_, d)| d.windows(3).any(|w| w == b"Ada")), "the name is nowhere in the clear");
    let allowed = [kind::CONNECT_TOKEN, kind::CHALLENGE, kind::REJECT, kind::RESPONSE_SEALED, kind::ACCEPT_SEALED, kind::DATA_SEALED, kind::DISCONNECT_SEALED];
    assert!(kinds_of(&cwire).iter().all(|k| allowed.contains(k)), "{:?}", kinds_of(&cwire));
    assert!(kinds_of(&cwire).iter().filter(|k| **k == kind::DATA_SEALED).count() > 10, "and plenty of data went");
    assert!(!kinds_of(&cwire).contains(&kind::DATA) && !kinds_of(&cwire).contains(&kind::ACCEPT), "no plain data or accept");
    // leaving is sealed too
    c.disconnect();
    r.run(&mut [&mut c], 300);
    assert_eq!(r.server.stats.leaves, 1);
    assert_eq!(r.server.stats.bad_packets, 0, "nothing was refused on the way");
}

#[test]
fn a_plain_client_is_not_let_into_a_secure_server_and_a_token_holder_not_into_a_plain_one() {
    let mut r = Rig::new(Link::perfect(), true);
    let (mut plain, _, _) = r.client(None, "Mallory", 5);
    r.run(&mut [&mut plain], 2500);
    assert!(!plain.connected() && r.joined().is_empty(), "no token, no entry");
    let mut r = Rig::new(Link::perfect(), false);
    let (mut c, _, _) = r.client(Some(token(&key(), GAME, 1, "Ada", NOW, 60, 3)), "Ada", 5);
    r.run(&mut [&mut c], 2500);
    assert!(!c.connected() && r.joined().is_empty(), "a server with no tokens does not know what to do with one");
    // (and a plain client still joins a plain server)
    let (mut plain, _, _) = r.client(None, "Bob", 6);
    r.run(&mut [&mut plain], 1500);
    assert!(plain.connected());
}

#[test]
fn a_token_of_another_key_game_or_age_is_refused_with_a_reason() {
    for (label, t, want) in [
        ("another server's key", token(&[9u8; 32], GAME, 1, "Ada", NOW, 60, 3), 3u8),
        ("another game", token(&key(), GAME + 1, 1, "Ada", NOW, 60, 4), 3),
        ("expired", token(&key(), GAME, 1, "Ada", NOW - 100, 60, 5), 6),
    ] {
        let mut r = Rig::new(Link::perfect(), true);
        let (mut c, _, _) = r.client(Some(t), "Ada", 5);
        let mut why = None;
        for _ in 0..300 {
            r.run(&mut [&mut c], 10);
            while let Some(e) = c.poll_event() {
                if let Event::Refused(w) = e {
                    why = Some(w);
                }
            }
        }
        assert_eq!(why, Some(want), "{label}");
        assert!(!c.connected() && r.joined().is_empty(), "{label}");
        assert!(r.server.stats.bad_packets >= 1, "{label}");
    }
}

#[test]
fn a_token_is_for_one_connection_and_a_player_coming_back_takes_over_from_the_old_one() {
    let mut r = Rig::new(Link::perfect(), true);
    let t = token(&key(), GAME, 7, "Ada", NOW, 60, 3);
    let (mut a, _, _) = r.client(Some(t.clone()), "Ada", 5);
    r.run(&mut [&mut a], 1000);
    assert!(a.connected());
    // the same token from somewhere else (stolen from the wire is not possible, but a copied token is): refused
    let (mut thief, _, _) = r.client(Some(t), "Ada", 9);
    r.run(&mut [&mut a, &mut thief], 1500);
    assert!(!thief.connected(), "the token is spent");
    assert_eq!(r.joined().len(), 1);
    // the player's next token (the same id) takes over before the server has noticed the first connection is dead
    let (mut b, _, _) = r.client(Some(token(&key(), GAME, 7, "Ada", NOW, 60, 20)), "Ada", 11);
    r.run(&mut [&mut a, &mut b], 1500);
    assert!(b.connected());
    assert_eq!((r.joined().len(), r.server.stats.leaves), (2, 1), "the old connection was let go for the new one");
    let players = r.server.world.entities.values().filter(|e| e.owner.is_some()).count();
    assert_eq!(players, 1);
}

#[test]
fn forged_changed_and_replayed_packets_are_dropped_and_the_player_does_not_notice() {
    let mut r = Rig::new(Link { latency_ms: 20, jitter_ms: 0, loss: 0.0, duplicate: 0.0 }, true);
    let (mut c, cwire, inject) = r.client(Some(token(&key(), GAME, 1, "Ada", NOW, 60, 3)), "Ada", 5);
    r.run(&mut [&mut c], 1500);
    assert!(c.connected());
    let before = r.server.stats.bad_packets;
    let datagrams: Vec<Vec<u8>> = cwire.lock().unwrap().iter().filter(|(sent, d)| *sent && d[1] == kind::DATA_SEALED).map(|(_, d)| d.clone()).collect();
    assert!(datagrams.len() > 5);
    let server = r.server_addr;
    // a replay of something the player sent, from the player's own address
    inject.lock().unwrap().push((server, datagrams[2].clone()));
    r.run(&mut [&mut c], 100);
    assert_eq!(r.server.stats.bad_packets, before + 1, "the replay was refused");
    // the same with one bit changed (a flip in the sealed part, and one in the clear header's connection number)
    let mut flipped = datagrams.last().unwrap().clone();
    let n = flipped.len();
    flipped[n - 5] ^= 1;
    inject.lock().unwrap().push((server, flipped));
    let mut wrong_conn = datagrams.last().unwrap().clone();
    wrong_conn[2] ^= 0x40;
    inject.lock().unwrap().push((server, wrong_conn));
    r.run(&mut [&mut c], 100);
    assert_eq!(r.server.stats.bad_packets, before + 3, "a change and a wrong connection number were refused");
    // a forgery sealed with a key that was guessed, for the right connection and counter
    let mut forged = vec![MAGIC, kind::DATA_SEALED, datagrams[0][2], datagrams[0][3]];
    let mut guess = Channel::new([1u8; 32], [2u8; 32]);
    let aad = [forged[0], forged[1], forged[2], forged[3]];
    guess.seal(&aad, &[0u8; 40], &mut forged);
    inject.lock().unwrap().push((server, forged));
    // a plain DATA, as an attacker who knows the format would make it
    inject.lock().unwrap().push((server, vec![MAGIC, kind::DATA, datagrams[0][2], datagrams[0][3], 0, 0, 0, 0, 0, 0, 0, 0]));
    r.run(&mut [&mut c], 100);
    assert_eq!(r.server.stats.bad_packets, before + 4, "the forgery was refused (the plain one is not even listened to)");
    // meanwhile the player is where it was: connected, and still being heard
    r.run(&mut [&mut c], 1000);
    assert!(c.connected());
    let mut s = EntityState { class: class::PLAYER, ..Default::default() };
    s.set_pos_m([3.0, 0.0, 0.0]);
    c.set_state(s);
    r.run(&mut [&mut c], 400);
    let at = r.server.world.entities.values().find(|e| e.owner.is_some()).map(|e| e.state.pos_m()).unwrap();
    assert!((at[0] - 3.0).abs() < 0.1, "{at:?}");
}

#[test]
fn on_a_bad_network_the_sealed_channel_still_carries_the_match() {
    // loss, doubled packets (each a replay to the window) and reordering by jitter
    let mut r = Rig::new(Link { latency_ms: 40, jitter_ms: 30, loss: 0.05, duplicate: 0.05 }, true);
    let (mut a, _, _) = r.client(Some(token(&key(), GAME, 1, "Ada", NOW, 60, 3)), "Ada", 5);
    let (mut b, _, _) = r.client(Some(token(&key(), GAME, 2, "Bo", NOW, 60, 30)), "Bo", 6);
    r.run(&mut [&mut a, &mut b], 4000);
    assert!(a.connected() && b.connected());
    let mut s = EntityState { class: class::PLAYER, ..Default::default() };
    s.set_pos_m([2.0, 0.0, 0.0]);
    a.set_state(s);
    r.run(&mut [&mut a, &mut b], 3000);
    let seen = b.sample(a.entity, r.now).map(|s| s.pos_m());
    assert!(seen.is_some_and(|p| (p[0] - 2.0).abs() < 0.5), "B sees A where A says it is: {seen:?}");
    assert!(r.server.stats.bad_packets > 0, "the doubled packets were refused by the window, not delivered twice");
    assert!(a.connected() && b.connected());
}
