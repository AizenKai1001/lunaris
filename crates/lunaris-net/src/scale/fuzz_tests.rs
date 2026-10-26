//! Hostile input (docs/SERVER_ROBUSTNESS.md): the server is fed what no honest client sends. Random bytes, real packets with bits flipped and cut short, a flood of handshakes, a client that never
//! acknowledges anything, numbers at the ends of their ranges, commands with names and arguments from the wrong side of the wire, names with control characters in them, and the admin command file in
//! shapes nobody expected. Every test runs in memory (a transport the test writes the datagrams into, from any address it likes) and is deterministic. `LUNARIS_FUZZ_ROUNDS=N` multiplies the work of the
//! loops by N for a long run (`cargo test --release -p lunaris-net --lib fuzz_ -- --nocapture` prints the costs of the worst datagrams).
use super::admin::Admin;
use super::bytes::{R, W};
use super::proto::{kind, msg, MAGIC, PROTOCOL};
use super::secure::{open_token, ConnectToken, MAX_NAME};
use super::server::{Ctx, Game, NetServer, ServerConfig, World, MAX_BACKLOG};
use super::simgame::{self, SimGame};
use super::state::{self, class, EntityState};
use super::transport::{Addr, Transport};
use lunaris_json::{parse, Json};
use lunaris_sim::{Command, Sim};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// A transport that the test writes datagrams into (`inbox`, from any address) and reads the server's answers from (`outbox`).
#[derive(Default)]
struct Pipe {
    inbox: VecDeque<(Addr, Vec<u8>)>,
    outbox: Vec<(Addr, Vec<u8>)>,
    /// When on, the data packets the test writes carry the acknowledgement of the newest data packet the server sent to that address: a client that is not deaf.
    acking: bool,
    newest: HashMap<Addr, u16>,
}

impl Transport for Pipe {
    fn send(&mut self, to: Addr, data: &[u8]) {
        if data.len() >= 12 && data[1] == kind::DATA {
            self.newest.insert(to, u16::from_le_bytes([data[4], data[5]]));
        }
        self.outbox.push((to, data.to_vec()));
    }

    fn recv(&mut self) -> Option<(Addr, Vec<u8>)> {
        let (from, mut d) = self.inbox.pop_front()?;
        if self.acking && d.len() >= 12 && d[1] == kind::DATA {
            if let Some(seq) = self.newest.get(&from) {
                d[6..8].copy_from_slice(&seq.to_le_bytes());
                d[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
            }
        }
        Some((from, d))
    }
}

/// A game that does nothing, except (with `chatter`) say something to everybody every tick.
#[derive(Default)]
struct Quiet {
    chatter: bool,
    commands: usize,
    joined: Vec<String>,
}

impl Game for Quiet {
    fn on_join(&mut self, _w: &mut World, _slot: usize, name: &str) -> EntityState {
        self.joined.push(name.to_string());
        EntityState { class: class::PLAYER, health: 100, ..Default::default() }
    }

    fn on_command(&mut self, _ctx: &mut Ctx, _slot: usize, _data: &[u8]) {
        self.commands += 1;
    }

    fn on_tick(&mut self, ctx: &mut Ctx) {
        if self.chatter {
            ctx.out.all(vec![0x49, 1, 2, 3, 4, 5, 6, 7, 8]);
        }
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| (self.next() >> 24) as u8).collect()
    }

    /// Between 0 and `max - 1` random bytes.
    fn blob(&mut self, max: u64) -> Vec<u8> {
        let n = self.below(max) as usize;
        self.bytes(n)
    }
}

/// How many times the loops run: the default keeps the whole file under a few seconds.
fn rounds() -> usize {
    std::env::var("LUNARIS_FUZZ_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(1).max(1)
}

fn addr(i: u32) -> Addr {
    SocketAddr::from(([10, (i >> 16) as u8, (i >> 8) as u8, i as u8], 20_000 + (i % 40_000) as u16))
}

fn plain_server<G: Game>(game: G) -> NetServer<Pipe, G> {
    NetServer::new(ServerConfig::default(), Pipe::default(), game)
}

/// A client that speaks the protocol by hand.
struct Raw {
    addr: Addr,
    conn: u16,
    seq: u16,
    rel: u16,
}

fn connect_packet(salt: u32, name: &[u8]) -> Vec<u8> {
    let mut w = W::new();
    w.u8(MAGIC);
    w.u8(kind::CONNECT);
    w.u8(PROTOCOL);
    w.u32(salt);
    w.bytes(name);
    w.buf
}

/// The handshake, by hand: `None` if the server did not let the client in.
fn hello<G: Game>(s: &mut NetServer<Pipe, G>, from: Addr, name: &[u8], salt: u32, now: &mut u64) -> Option<Raw> {
    s.transport.outbox.retain(|(to, _)| *to != from);
    s.transport.inbox.push_back((from, connect_packet(salt, name)));
    *now += 5;
    s.update(*now);
    let challenge = s.transport.outbox.iter().find(|(to, d)| *to == from && d.get(1) == Some(&kind::CHALLENGE))?.1.clone();
    let mut r = R::new(&challenge[2..]);
    let (cs, ss) = (r.u32()?, r.u32()?);
    let mut w = W::new();
    w.u8(MAGIC);
    w.u8(kind::RESPONSE);
    w.u32(cs ^ ss);
    s.transport.inbox.push_back((from, w.buf));
    *now += 5;
    s.update(*now);
    let accept = s.transport.outbox.iter().find(|(to, d)| *to == from && d.get(1) == Some(&kind::ACCEPT))?.1.clone();
    let conn = R::new(&accept[2..]).u16()?;
    Some(Raw { addr: from, conn, seq: 0, rel: 0 })
}

impl Raw {
    /// A data packet with these messages in it (it acknowledges nothing).
    fn data(&mut self, msgs: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut w = W::new();
        w.u8(MAGIC);
        w.u8(kind::DATA);
        w.u16(self.conn);
        w.u16(self.seq);
        w.u16(0);
        w.u32(0);
        self.seq = self.seq.wrapping_add(1);
        for (t, body) in msgs {
            w.u8(*t);
            w.bytes(body);
        }
        w.buf
    }

    fn state(s: &EntityState) -> (u8, Vec<u8>) {
        let mut w = W::new();
        state::write_own(&mut w, s);
        (msg::STATE, w.buf)
    }

    fn reliable(&mut self, payload: &[u8]) -> (u8, Vec<u8>) {
        let mut body = self.rel.to_le_bytes().to_vec();
        self.rel = self.rel.wrapping_add(1);
        body.extend_from_slice(payload);
        (msg::RELIABLE, body)
    }
}

/// Run the server on for `ms` of its clock with one datagram of nothing, so that ticks happen.
fn tick_for<G: Game>(s: &mut NetServer<Pipe, G>, now: &mut u64, ms: u64) {
    let end = *now + ms;
    while *now < end {
        *now += 10;
        s.update(*now);
        s.transport.outbox.clear();
    }
}

/// What handling one datagram costs: the server is updated at the same instant, so no tick runs, only the reading of it.
fn cost<G: Game>(s: &mut NetServer<Pipe, G>, now: u64, from: Addr, d: Vec<u8>) -> Duration {
    s.transport.inbox.push_back((from, d));
    let t = Instant::now();
    s.update(now);
    t.elapsed()
}

#[derive(Default)]
struct Costs {
    all: Vec<(Duration, &'static str)>,
}

impl Costs {
    fn note(&mut self, d: Duration, what: &'static str) {
        self.all.push((d, what));
    }

    fn report(&mut self, title: &str) -> Duration {
        self.all.sort_by_key(|c| c.0);
        let n = self.all.len().max(1);
        let total: Duration = self.all.iter().map(|c| c.0).sum();
        let worst = self.all.last().copied().unwrap_or((Duration::ZERO, ""));
        println!("{title}: {} datagrams, mean {:?}, p99.9 {:?}, worst {:?} ({})", self.all.len(), total / n as u32, self.all.get(n * 999 / 1000).map_or(Duration::ZERO, |c| c.0), worst.0, worst.1);
        worst.0
    }
}

#[test]
fn fuzz_garbage_datagrams_never_panic_and_each_costs_little() {
    let mut s = plain_server(Quiet::default());
    let mut now = 1000;
    let legit = hello(&mut s, addr(1), b"legit", 7, &mut now).expect("the honest player gets in");
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let mut costs = Costs::default();
    for i in 0..40_000 * rounds() {
        let (from, d, what) = match i % 5 {
            // nothing but noise, up to a whole receive buffer
            0 => (addr(100 + rng.below(5000) as u32), rng.blob(2049), "noise"),
            // the right first byte and a kind, then noise
            1 => {
                let mut d = vec![MAGIC, 1 + rng.below(12) as u8];
                d.extend(rng.blob(1300));
                (addr(100 + rng.below(5000) as u32), d, "kind and noise")
            }
            // a data packet for the real player's connection, from somebody else (a stolen session number)
            2 => {
                let mut d = vec![MAGIC, kind::DATA];
                d.extend_from_slice(&legit.conn.to_le_bytes());
                d.extend(rng.blob(1200));
                (addr(100 + rng.below(5000) as u32), d, "stolen session")
            }
            // a data packet from the player's own address with soup for messages (the parser's work)
            3 => {
                let mut d = vec![MAGIC, kind::DATA];
                d.extend_from_slice(&legit.conn.to_le_bytes());
                d.extend(rng.bytes(8));
                let msgs = rng.below(400) as usize;
                for _ in 0..msgs {
                    d.push([msg::STATE, msg::RELIABLE, msg::SNAPSHOT, 0, 0xFF][rng.below(5) as usize]);
                    let n = rng.below(40) as usize;
                    d.push(n as u8);
                    d.extend(rng.bytes(n));
                }
                d.truncate(2048);
                (legit.addr, d, "message soup")
            }
            // handshakes with lengths that lie
            _ => {
                let mut d = vec![MAGIC, kind::CONNECT, PROTOCOL];
                d.extend(rng.bytes(4));
                d.extend_from_slice(&[0xFF; 9]);
                d.push(0x7F);
                d.extend(rng.blob(60));
                (addr(100 + rng.below(5000) as u32), d, "lying handshake")
            }
        };
        let c = cost(&mut s, now, from, d);
        costs.note(c, what);
    }
    let worst = costs.report("garbage");
    assert!(worst < Duration::from_millis(20), "one datagram took {worst:?}");
    assert!(s.pending_len() <= 4096, "the table of half-open handshakes is bounded");
    assert_eq!(s.client_count(), 1, "nobody else got in");
    // the honest player is still in and still moves
    let mut raw = Raw { addr: legit.addr, conn: legit.conn, seq: 30_000, rel: 0 };
    let mut st = EntityState { class: class::PLAYER, health: 100, ..Default::default() };
    st.set_pos_m([0.5, 0.0, 0.0]);
    let d = raw.data(&[Raw::state(&st)]);
    s.transport.inbox.push_back((raw.addr, d));
    tick_for(&mut s, &mut now, 100);
    let entity = s.entity_of(0).unwrap();
    assert!(s.world.get(entity).unwrap().state.pos_m()[0] > 0.4, "the honest player's move was taken after the storm");
}

#[test]
fn fuzz_real_packets_with_bits_flipped_cut_short_and_doubled_never_panic() {
    let mut s = plain_server(Quiet { chatter: true, ..Default::default() });
    let mut now = 1000;
    let mut clients: Vec<Raw> = (0..3).map(|i| hello(&mut s, addr(i + 1), format!("p{i}").as_bytes(), 100 + i, &mut now).unwrap()).collect();
    // a corpus: what the clients send in a couple of seconds and what the server sends back
    let mut corpus: Vec<Vec<u8>> = vec![connect_packet(5, b"corpus")];
    let mut st = EntityState { class: class::PLAYER, health: 100, ..Default::default() };
    for step in 0..60 {
        for (k, c) in clients.iter_mut().enumerate() {
            st.set_pos_m([step as f32 * 0.1, 0.0, k as f32]);
            st.flags = step as u16;
            let r = c.reliable(&simgame::action("build:wall", [1.0, 2.0, 3.0, 4.0]));
            let d = c.data(&[Raw::state(&st), r]);
            corpus.push(d.clone());
            s.transport.inbox.push_back((c.addr, d));
        }
        tick_for(&mut s, &mut now, 33);
    }
    s.transport.outbox.clear();
    tick_for(&mut s, &mut now, 200);
    // (the server's own packets: they have the same header, so they are fed back too)
    let mut sent: Vec<Vec<u8>> = Vec::new();
    for step in 0..10 {
        for c in clients.iter_mut() {
            let d = c.data(&[Raw::state(&st)]);
            s.transport.inbox.push_back((c.addr, d));
        }
        now += 33;
        s.update(now);
        sent.extend(s.transport.outbox.drain(..).map(|(_, d)| d));
        let _ = step;
    }
    corpus.extend(sent);
    let mut rng = Rng(77);
    let mut costs = Costs::default();
    for _ in 0..rounds() {
        for original in corpus.clone() {
            for _ in 0..60 {
                let mut d = original.clone();
                match rng.below(6) {
                    0 => {
                        for _ in 0..1 + rng.below(8) {
                            let at = rng.below(d.len() as u64) as usize;
                            d[at] ^= 1 << rng.below(8);
                        }
                    }
                    1 => d.truncate(rng.below(d.len() as u64 + 1) as usize),
                    2 => d.extend(rng.blob(400)),
                    3 => {
                        let at = rng.below(d.len() as u64) as usize;
                        d[at] = [0, 0xFF, 0x7F, 0x80][rng.below(4) as usize];
                    }
                    4 => {
                        let n = d.len();
                        d.extend_from_within(..n);
                    }
                    _ => {
                        // the lengths of the messages made huge
                        for b in d.iter_mut().skip(12).step_by(7) {
                            *b = 0xFF;
                        }
                    }
                }
                // from the real player's address (as if somebody could spoof it) and from a stranger
                let from = if rng.below(2) == 0 { clients[rng.below(3) as usize].addr } else { addr(900 + rng.below(100) as u32) };
                let c = cost(&mut s, now, from, d);
                costs.note(c, "mutated");
            }
        }
    }
    let worst = costs.report("mutated real packets");
    assert!(worst < Duration::from_millis(20), "one datagram took {worst:?}");
    assert!(s.pending_len() <= 4096);
    // the server still runs, whoever was dropped on the way
    tick_for(&mut s, &mut now, 500);
    assert!(s.stats.ticks > 100);
}

#[test]
fn fuzz_a_flood_of_handshakes_cannot_lock_a_real_player_out() {
    // from five thousand different addresses (spoofed sources cost the attacker nothing), one CONNECT each: the table of half-open handshakes is full
    let mut s = plain_server(Quiet::default());
    let mut now = 1000;
    for i in 0..5000u32 {
        s.transport.inbox.push_back((addr(10_000 + i), connect_packet(i, b"x")));
    }
    now += 5;
    s.update(now);
    assert!(s.pending_len() <= 4096);
    s.transport.outbox.clear();
    let ok = hello(&mut s, addr(3), b"honest", 9, &mut now);
    assert!(ok.is_some(), "a real player gets in while a flood of CONNECTs from other addresses fills the table");
    // the same from ONE address and thousands of ports (it costs the attacker no spoofing at all)
    let mut s = plain_server(Quiet::default());
    let mut now = 1000;
    for port in 1024..7000u16 {
        s.transport.inbox.push_back((SocketAddr::from(([203, 0, 113, 9], port)), connect_packet(port as u32, b"x")));
    }
    now += 5;
    s.update(now);
    let ok = hello(&mut s, addr(4), b"honest", 9, &mut now);
    assert!(ok.is_some(), "a real player gets in while one address opens thousands of handshakes");
    assert!(s.pending_len() <= 200, "one address holds only a few half-open handshakes ({} are held)", s.pending_len());
}

#[test]
fn fuzz_a_client_that_never_acknowledges_does_not_make_the_server_keep_everything_for_ever() {
    let mut s = plain_server(Quiet { chatter: true, ..Default::default() });
    let mut now = 1000;
    let mut c = hello(&mut s, addr(1), b"deaf", 3, &mut now).unwrap();
    let mut peak = 0;
    // two minutes of a client that says it is alive every 100 ms and never acknowledges a packet
    for _ in 0..1200 {
        let d = c.data(&[]);
        s.transport.inbox.push_back((c.addr, d));
        tick_for(&mut s, &mut now, 100);
        peak = peak.max(s.reliable_backlog(0));
    }
    assert!(peak <= MAX_BACKLOG + 64, "the messages kept for a client that acknowledges nothing grew to {peak}");
    assert_eq!(s.client_count(), 0, "a client that acknowledges nothing is removed");
    assert_eq!(s.stats.backlog_kicks, 1);
}

#[test]
fn fuzz_positions_at_the_ends_of_the_number_range_are_refused_not_a_crash() {
    let mut s = plain_server(Quiet::default());
    let mut now = 1000;
    let mut c = hello(&mut s, addr(1), b"edge", 3, &mut now).unwrap();
    for pos in [[i32::MIN, 0, 0], [0, i32::MIN, 0], [0, 0, i32::MIN], [i32::MAX, i32::MAX, i32::MAX], [i32::MIN, i32::MIN, i32::MIN], [5_999_999, -5_999_999, 6_000_000]] {
        let st = EntityState { class: class::PLAYER, health: 100, pos, ..Default::default() };
        let d = c.data(&[Raw::state(&st)]);
        s.transport.inbox.push_back((c.addr, d));
        tick_for(&mut s, &mut now, 40);
        let p = s.world.get(s.entity_of(0).unwrap()).unwrap().state.pos;
        assert!(p.iter().all(|v| v.abs() < 6_000_000), "a position {pos:?} was believed: {p:?}");
    }
    // and from a place the game put the character at the far end of the range (a NaN that became the largest integer)
    let e = s.entity_of(0).unwrap();
    s.world.get_mut(e).unwrap().state.pos = [i32::MAX, i32::MAX, i32::MAX];
    let st = EntityState { class: class::PLAYER, health: 100, pos: [i32::MIN, i32::MIN, i32::MIN], ..Default::default() };
    let d = c.data(&[Raw::state(&st)]);
    s.transport.inbox.push_back((c.addr, d));
    tick_for(&mut s, &mut now, 40);
    assert_eq!(s.client_count(), 1);
}

#[test]
fn fuzz_a_name_is_made_safe_at_the_door() {
    let mut s = plain_server(Quiet::default());
    let mut now = 1000;
    let bad: [&[u8]; 6] = [b"evil\nip 1.2.3.4", b"a#b", b"tab\there\x07bell", "rtl\u{202E}name".as_bytes(), b"   padded   name   ", b"\xff\xfe\x00broken"];
    for (i, name) in bad.iter().enumerate() {
        let c = hello(&mut s, addr(i as u32 + 1), name, i as u32, &mut now);
        assert!(c.is_some(), "{:?} gets in", String::from_utf8_lossy(name));
        let got = s.name_of(i).unwrap().to_string();
        assert!(!got.chars().any(|c| c.is_control() || c == '#' || ('\u{202A}'..='\u{202E}').contains(&c) || ('\u{2066}'..='\u{2069}').contains(&c) || c == '\u{FFFD}'), "the name {got:?} still has characters that do not belong in a name");
        assert_eq!(got, got.trim(), "a name has no spaces at the ends");
        assert!(!got.contains("  "), "or twice in a row");
    }
    // a name that is empty after cleaning gets a name, and two of them are two players
    let a = hello(&mut s, addr(50), b"\n\n", 1, &mut now);
    let b = hello(&mut s, addr(51), b"\x01", 2, &mut now);
    assert!(a.is_some() && b.is_some());
    assert_ne!(s.name_of(6), s.name_of(7));
    assert!(s.name_of(6).unwrap().len() > 0);
}

#[test]
fn fuzz_the_admin_can_kick_and_ban_a_player_whose_name_has_a_space_in_it() {
    let dir = scratch("admin_spaces");
    let mut s = plain_server(Quiet::default());
    let mut now = 1000;
    hello(&mut s, addr(1), b"Big Bob", 1, &mut now).unwrap();
    hello(&mut s, addr(2), b"Bob", 2, &mut now).unwrap();
    let mut admin = Admin::open(&dir, &mut s).unwrap();
    let reply = admin.run(&mut s, "kick Big Bob");
    assert!(reply.contains("removed"), "{reply}");
    assert_eq!(s.client_count(), 1, "Big Bob went, Bob stayed");
    assert_eq!(s.name_of(1), Some("Bob"));
    hello(&mut s, addr(3), b"Big Bob", 3, &mut now).unwrap();
    let reply = admin.run(&mut s, "ban Big Bob 2h");
    assert!(reply.contains("banned for 2 h"), "{reply}");
    assert!(hello(&mut s, addr(4), b"Big Bob", 4, &mut now).is_none(), "the ban holds");
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("lunaris_fuzz_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn fuzz_the_admin_command_file_survives_binary_junk_and_huge_lines() {
    let dir = scratch("admin_file");
    let mut s = plain_server(Quiet::default());
    let mut now = 1000u64;
    let mut admin = Admin::open(&dir, &mut s).unwrap();
    let cmds = dir.join("commands.txt");
    let append = |bytes: &[u8]| {
        use std::io::Write;
        std::fs::OpenOptions::new().append(true).open(&cmds).unwrap().write_all(bytes).unwrap();
    };
    let answered = |n: usize| std::fs::read_to_string(dir.join("output.log")).map_or(0, |t| t.matches("0 players in").count()) >= n;
    // a line of bytes that are not text, then a command
    append(b"\xff\xfe\x80\x81 not text\n");
    append(b"status\n");
    now += 600;
    admin.poll(&mut s, now);
    assert!(answered(1), "a command after a line of junk bytes is still run");
    // more than a megabyte in one go, with a two-byte letter on the 1 MiB mark
    let mut big = vec![b'#'; (1 << 20) - 1];
    big.extend_from_slice("\u{e9}".as_bytes());
    big.extend_from_slice(b" a comment\nstatus\n");
    append(&big);
    for _ in 0..4 {
        now += 600;
        admin.poll(&mut s, now);
    }
    assert!(answered(2), "a command after a megabyte of comments is still run");
    // one line with no end for two megabytes, then an end and a command
    append(&vec![b'x'; 2 << 20]);
    now += 600;
    admin.poll(&mut s, now);
    append(b"\nstatus\n");
    for _ in 0..8 {
        now += 600;
        admin.poll(&mut s, now);
    }
    assert!(answered(3), "a command after a line of two megabytes is still run");
}

#[test]
fn fuzz_the_ban_file_is_written_whole_or_not_at_all() {
    let dir = scratch("bans");
    let mut s = plain_server(Quiet::default());
    let mut now = 1000;
    hello(&mut s, addr(1), b"cheater", 1, &mut now).unwrap();
    let mut admin = Admin::open(&dir, &mut s).unwrap();
    std::fs::OpenOptions::new().append(true).open(dir.join("commands.txt")).and_then(|mut f| std::io::Write::write_all(&mut f, b"ban cheater\n")).unwrap();
    admin.poll(&mut s, 5000);
    assert!(std::fs::read_to_string(dir.join("bans.txt")).unwrap().contains("name cheater"));
    let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".tmp")).collect();
    assert!(leftovers.is_empty(), "no half-written file is left behind: {leftovers:?}");
}

#[test]
fn fuzz_a_token_for_a_name_in_any_script_is_good() {
    let key = [9u8; 32];
    for name in ["Ada", "あいぜんあいぜんあいぜんあいぜんあいぜん", "ÄÖÜäöüÄÖÜäöüÄÖÜäöüÄÖÜäöüÄÖÜäöüÄÖÜ", "😀😀😀😀😀😀😀😀😀😀😀😀", "x"] {
        let t = ConnectToken::issue(&key, 5, 1, name, 100, 60, [3; 24], [4; 32], [5; 32]);
        let claims = open_token(&key, 5, t.expire, &t.nonce, &t.private).unwrap_or_else(|| panic!("the server cannot open a token for {name:?}"));
        assert!(claims.name.len() <= MAX_NAME && name.starts_with(&claims.name), "{name:?} became {:?}", claims.name);
        assert!(!claims.name.is_empty());
    }
}

// ------------------------------------------------------------------------------------------------------------------ the game server

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

/// The scene of the server tests of `simgame_tests`: a ground, building rules, two places to start, an avatar with a weapon and a build tool.
fn game_scene() -> Sim {
    game_scene_saving("")
}

fn game_scene_saving(save: &str) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[400,1,400]}}"#);
    spawn(&mut s, 3, r#"{"BuildRules":{"text":"rules step=90\npiece foundation size=4,0.5,4 hp=500 free=1 anchor=1\n"}}"#);
    let save = if save.is_empty() { String::new() } else { format!(r#","save":"{}","save_every":100000,"persist":"Transform Inventory HitPoints""#, save.replace(char::from(92), "/")) };
    spawn(&mut s, 4, &format!(r#"{{"NetHost":{{"player":"Avatar","static_every":3,"drop":"Pickup","chat":30,"party":3{save}}}}}"#));
    spawn(&mut s, 21, r#"{"Transform":{"pos":[0,0,0]},"SpawnPoint":{}}"#);
    spawn(&mut s, 6, r#"{"ItemTable":{"text":"item bandage | Bandage | stack=10\nitem axe | Axe | stack=1\n"}}"#);
    let avatar = r#"{"components":{"Name":{"value":"Avatar"},"Transform":{},"CharacterBody":{"radius":0.35,"height":1.8,"gravity_scale":0},
        "HitPoints":{"value":100,"max":100,"last":100},"Inventory":{"size":12},"NetAvatar":{}},
       "children":[{"components":{"Name":{"value":"Head"},"LocalTransform":{"pos":[0,1.6,0]},
        "Weapon":{"damage":60,"spread":0,"recoil":[0,0],"rate":120,"auto":true,"infinite":true,"noise":0,"range":80},"BuildTool":{"reach":12},"Interactor":{"reach":3,"look":false}}}]}"#;
    s.apply(&Command::DefinePrefab { name: "Avatar".into(), def: parse(avatar).unwrap() }).unwrap();
    s.apply(&Command::DefinePrefab { name: "Pickup".into(), def: parse(r#"{"components":{"Name":{"value":"Pickup"},"Transform":{},"Interactable":{"kind":3,"prompt":"Take"},"NetEntity":{"class":1,"kind":20}}}"#).unwrap() }).unwrap();
    s
}

/// An action message as the client sends it, but with the name and the four numbers as raw bytes (a real client cannot send a name of any length or a number that is not a number).
fn raw_action(name: &[u8], args: [u32; 4], claimed_len: Option<u8>) -> Vec<u8> {
    let mut w = W::new();
    w.u8(simgame::msg::ACTION);
    w.u8(claimed_len.unwrap_or(name.len() as u8));
    w.raw(name);
    for a in args {
        w.u32(a);
    }
    w.buf
}

#[test]
fn fuzz_commands_that_no_client_sends_leave_the_game_server_standing() {
    let mut s = plain_server(SimGame::new(game_scene()));
    s.transport.acking = true;
    let mut now = 1000;
    let mut c = hello(&mut s, addr(1), b"mallory", 1, &mut now).unwrap();
    tick_for(&mut s, &mut now, 200);
    let strings_before = s.game.sim.world.strings().len();
    let entities_before = s.game.sim.world.len();
    let mut rng = Rng(4242);
    let names: [&[u8]; 18] = [b"inv_move", b"inv_use", b"inv_drop", b"inv_craft", b"inv_craft_start", b"inv_craft_cancel", b"build:foundation", b"build:", b"place", b"party_invite", b"party_accept", b"party_leave", b"party_kick", b"party_", b"use", b"reload", b"", b"party_say"];
    let numbers: [u32; 16] = [0, 1, 0x3F80_0000, 0xBF80_0000, 0x42C6_0000, 0x4348_0000, 0x7F7F_FFFF, 0xFF7F_FFFF, 0x7FC0_0000, 0x7F80_0000, 0xFF80_0000, 0x4E6E_6B28, 0x0000_0001, 0x42FE_0000, 0x4640_0000, 0x4220_0000];
    let t0 = Instant::now();
    let n = 6000 * rounds();
    for i in 0..n {
        let mut msgs: Vec<(u8, Vec<u8>)> = Vec::new();
        for _ in 0..1 + rng.below(4) {
            let payload = match rng.below(10) {
                // a known command with extreme arguments
                0..=4 => {
                    let name = names[rng.below(names.len() as u64) as usize];
                    raw_action(name, [numbers[rng.below(16) as usize], numbers[rng.below(16) as usize], numbers[rng.below(16) as usize], numbers[rng.below(16) as usize]], None)
                }
                // a name nobody has ever used, different every time and as long as can be
                5..=6 => {
                    let name = rng.blob(255);
                    let name = if name.is_empty() { vec![b'x'] } else { name };
                    raw_action(&name, [1, 2, 3, 4], None)
                }
                // a length byte that lies
                7 => raw_action(b"inv_move", [0; 4], Some(rng.below(256) as u8)),
                // text to say, too long, with controls, and the party's own words
                8 => {
                    let mut w = vec![simgame::msg::SAY];
                    let long = rng.below(3) == 0;
                    w.extend(rng.blob(if long { 4000 } else { 100 }));
                    w
                }
                // anything at all, with the first byte of a message the game knows
                _ => {
                    let mut w = vec![[simgame::msg::CONTROL, simgame::msg::ACTION, simgame::msg::SAY, simgame::msg::RIDE, simgame::msg::VIEW, 0xFF][rng.below(6) as usize]];
                    w.extend(rng.blob(60));
                    w
                }
            };
            let m = c.reliable(&payload);
            msgs.push(m);
        }
        let d = c.data(&msgs);
        s.transport.inbox.push_back((c.addr, d));
        // (a client may send 300 messages a second: the pace of a client that is not throttled)
        if i % 3 == 0 {
            tick_for(&mut s, &mut now, 30);
        } else {
            s.update(now);
        }
    }
    println!("{n} datagrams of hostile commands in {:?}", t0.elapsed());
    tick_for(&mut s, &mut now, 300);
    assert_eq!(s.client_count(), 1, "the server stands and so does the player");
    let strings = s.game.sim.world.strings().len() - strings_before;
    assert!(strings <= 600, "{strings} new strings were made in the world's table by names a client chose (they are never taken out again)");
    let entities = s.game.sim.world.len() as i64 - entities_before as i64;
    assert!(entities < 200, "{entities} entities appeared out of commands");
}

#[test]
fn fuzz_joining_and_leaving_with_names_nobody_used_before_does_not_grow_the_server_without_end() {
    let mut s = plain_server(SimGame::new(game_scene()));
    let mut now = 1000;
    let mut first = hello(&mut s, addr(1), b"first", 1, &mut now).unwrap();
    tick_for(&mut s, &mut now, 100);
    let before = s.game.sim.world.strings().len();
    let n = 400 * rounds();
    let t0 = Instant::now();
    for i in 0..n {
        let name = format!("visitor {i:06}");
        let from = addr(5000 + i as u32);
        let c = hello(&mut s, from, name.as_bytes(), i as u32, &mut now);
        assert!(c.is_some());
        let d = {
            let mut w = W::new();
            w.u8(MAGIC);
            w.u8(kind::DISCONNECT);
            w.u16(c.unwrap().conn);
            w.buf
        };
        s.transport.inbox.push_back((from, d));
        let d = first.data(&[]);
        s.transport.inbox.push_back((first.addr, d));
        tick_for(&mut s, &mut now, 10);
    }
    println!("{n} joins and leaves with unique names in {:?}: the string table grew by {}, entities {}", t0.elapsed(), s.game.sim.world.strings().len() - before, s.game.sim.world.len());
    assert_eq!(s.client_count(), 1, "everybody who came went, the first player is still in");
    // (one string is kept for every name: that is known and measured here; what must not be is a growth that is more than a few strings for a name)
    assert!(s.game.sim.world.strings().len() - before <= 3 * n, "the table grew by more than three strings for every name");
}

// ------------------------------------------------------------------------------------------------------------------ the world save

/// A server with the game scene that saves to `path`, one player in, and a mark in the world (an entity with this uid) before the first save.
fn saving_server(path: &std::path::Path, marks: &[u64]) -> (NetServer<Pipe, SimGame>, u64) {
    let mut sim = game_scene_saving(&path.to_string_lossy());
    for uid in marks {
        spawn(&mut sim, *uid, r#"{"Transform":{"pos":[10,0,10]}}"#);
    }
    let mut s = plain_server(SimGame::new(sim));
    let mut now = 1000;
    hello(&mut s, addr(1), b"ada", 1, &mut now).unwrap();
    (s, now)
}

fn marked(s: &NetServer<Pipe, SimGame>, uid: u64) -> bool {
    s.game.sim.world.entity(uid).is_some()
}

fn corrupt_files(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".corrupt-")).count()
}

#[test]
fn fuzz_a_save_has_a_checksum_and_the_one_before_it_is_kept() {
    let dir = scratch("save_checksum");
    let path = dir.join("world.json");
    let (mut s, _) = saving_server(&path, &[700]);
    assert!(s.game.save_now());
    assert!(!path.with_extension("bak").exists(), "the first save has nothing before it");
    s.game.sim.apply(&Command::Spawn { uid: Some(9701), components: vec![] }).unwrap();
    assert!(s.game.save_now());
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(simgame::check_save(&text), Ok(true), "a save carries a checksum that is right");
    let bak = std::fs::read_to_string(path.with_extension("bak")).unwrap();
    assert_eq!(simgame::check_save(&bak), Ok(true));
    assert!(bak.len() < text.len() || bak != text, "the save before it is the earlier one");
    // it is still JSON, and the world loads from it
    assert!(parse(&text).is_ok());
    assert!(!path.with_extension("tmp").exists(), "nothing half written is left");
    // a cut, a flipped digit and an extra byte are all found
    let flipped = {
        let mut b = text.clone().into_bytes();
        let at = b.iter().enumerate().skip(b.len() / 2).find(|(_, c)| c.is_ascii_digit()).unwrap().0;
        b[at] = if b[at] == b'7' { b'8' } else { b'7' };
        String::from_utf8(b).unwrap()
    };
    for bad in [text[..text.len() / 2].to_string(), text[..text.len() - 3].to_string(), flipped, format!("{text}x"), text.replace("world_version", "world_versioN")] {
        assert!(simgame::check_save(&bad).is_err(), "a damaged save is not taken for a whole one");
    }
    assert_eq!(simgame::check_save("{\"a\":1}\n"), Ok(false), "a save of an older server has no checksum and is read as it is");
}

#[test]
fn fuzz_a_damaged_world_save_is_kept_aside_and_the_save_before_it_is_used() {
    for how in ["cut in half", "one digit changed", "not text", "emptied", "garbage appended"] {
        let dir = scratch("save_damaged");
        let path = dir.join("world.json");
        let (mut s, _) = saving_server(&path, &[700]);
        assert!(s.game.save_now());
        s.game.sim.apply(&Command::Spawn { uid: Some(9701), components: vec![] }).unwrap();
        assert!(s.game.save_now());
        let mut b = std::fs::read(&path).unwrap();
        match how {
            "cut in half" => b.truncate(b.len() / 2),
            "one digit changed" => {
                let at = b.iter().enumerate().skip(b.len() / 2).find(|(_, c)| c.is_ascii_digit()).unwrap().0;
                b[at] = if b[at] == b'7' { b'8' } else { b'7' };
            }
            "not text" => b.iter_mut().skip(100).step_by(50).for_each(|c| *c = 0xFF),
            "emptied" => b.clear(),
            _ => b.extend_from_slice(b"{\"junk\":1}"),
        }
        std::fs::write(&path, &b).unwrap();
        // a new server over the same files
        let (s2, _) = saving_server(&path, &[]);
        assert!(marked(&s2, 700), "{how}: the world is the one of the save before the damaged one");
        assert!(!marked(&s2, 9701), "{how}: and not the damaged one");
        assert_eq!(corrupt_files(&dir), 1, "{how}: the damaged file is kept aside");
        assert!(!path.exists() || simgame::check_save(&std::fs::read_to_string(&path).unwrap()).is_ok(), "{how}: nothing damaged is left in its place");
        // and it saves again, whole, with the good one still before it
        let mut s2 = s2;
        assert!(s2.game.save_now());
        assert_eq!(simgame::check_save(&std::fs::read_to_string(&path).unwrap()), Ok(true));
        assert_eq!(corrupt_files(&dir), 1);
    }
}

#[test]
fn fuzz_two_damaged_saves_start_a_fresh_world_and_keep_the_files() {
    let dir = scratch("save_both");
    let path = dir.join("world.json");
    let (mut s, _) = saving_server(&path, &[700]);
    assert!(s.game.save_now());
    assert!(s.game.save_now());
    for f in [path.clone(), path.with_extension("bak")] {
        let mut b = std::fs::read(&f).unwrap();
        b.truncate(b.len() / 3);
        std::fs::write(&f, b).unwrap();
    }
    let (s2, _) = saving_server(&path, &[]);
    assert!(!marked(&s2, 700), "no usable save: the scene's own world");
    assert_eq!(s2.client_count(), 1, "the server runs");
    assert!(corrupt_files(&dir) >= 1 && path.with_extension("bak").exists(), "the damaged files are not thrown away");
}

#[test]
fn fuzz_a_half_written_temporary_file_is_never_read_and_is_replaced_by_the_next_save() {
    let dir = scratch("save_torn");
    let path = dir.join("world.json");
    let (mut s, _) = saving_server(&path, &[700]);
    assert!(s.game.save_now());
    // the server was killed while it wrote the next save: a torn .tmp beside the good one
    std::fs::write(path.with_extension("tmp"), b"{\"world_version\": 5, \"entities\": [ {\"uid\": 1").unwrap();
    let (mut s2, _) = saving_server(&path, &[]);
    assert!(marked(&s2, 700), "the good save is loaded, the torn file is not looked at");
    assert_eq!(corrupt_files(&dir), 0);
    assert!(s2.game.save_now());
    assert!(!path.with_extension("tmp").exists());
    assert_eq!(simgame::check_save(&std::fs::read_to_string(&path).unwrap()), Ok(true));
}

#[test]
fn fuzz_a_save_that_cannot_be_written_is_counted_and_the_old_one_stays() {
    let dir = scratch("save_fail");
    let path = dir.join("world.json");
    let (mut s, _) = saving_server(&path, &[700]);
    assert!(s.game.save_now());
    let before = std::fs::read(&path).unwrap();
    // the folder of the temporary file is a file: nothing can be written there
    let blocked = dir.join("sub");
    std::fs::write(&blocked, b"x").unwrap();
    s.game.sim.apply(&Command::Spawn { uid: Some(9702), components: vec![] }).unwrap();
    // (a directory in the place of the temporary file makes the write fail)
    std::fs::create_dir_all(path.with_extension("tmp")).unwrap();
    assert!(!s.game.save_now(), "the save could not be written");
    assert_eq!(s.game.stats.saves_failed, 1);
    assert_eq!(std::fs::read(&path).unwrap(), before, "the save that was there is untouched");
}

// ------------------------------------------------------------------------------------------------------------------ a server that wants connect tokens

#[test]
fn fuzz_connect_tokens_and_sealed_packets_of_garbage_never_panic_and_a_real_token_still_joins() {
    let key = [7u8; 32];
    let mut cfg = ServerConfig::default();
    cfg.auth = Some(super::server::Auth { key, protocol_id: 55, unix_now: Some(1_000_000) });
    let mut s = NetServer::new(cfg, Pipe::default(), Quiet::default());
    let mut now = 1000u64;
    let token_packet = |t: &ConnectToken, salt: u32| {
        let mut w = W::new();
        w.u8(MAGIC);
        w.u8(kind::CONNECT_TOKEN);
        w.u8(PROTOCOL);
        w.u32(salt);
        w.raw(&t.protocol_id.to_le_bytes());
        w.raw(&t.expire.to_le_bytes());
        w.raw(&t.nonce);
        w.bytes(&t.private);
        w.buf
    };
    let seed = ConnectToken::issue(&key, 55, 1, "seed", 1_000_000, 300, [9; 24], [1; 32], [2; 32]);
    let good = token_packet(&seed, 5);
    let mut rng = Rng(31337);
    let mut costs = Costs::default();
    for i in 0..20_000 * rounds() {
        let (from, d, what): (Addr, Vec<u8>, &'static str) = match i % 4 {
            0 => {
                let mut d = good.clone();
                for _ in 0..1 + rng.below(6) {
                    let at = rng.below(d.len() as u64) as usize;
                    d[at] ^= 1 << rng.below(8);
                }
                (addr(100 + rng.below(3000) as u32), d, "token with bits flipped")
            }
            1 => {
                let mut d = good.clone();
                d.truncate(rng.below(good.len() as u64 + 1) as usize);
                (addr(100 + rng.below(3000) as u32), d, "token cut short")
            }
            2 => {
                let mut d = vec![MAGIC, [kind::CONNECT_TOKEN, kind::RESPONSE_SEALED, kind::DATA_SEALED, kind::DISCONNECT_SEALED, kind::CONNECT, kind::DATA][rng.below(6) as usize]];
                d.extend(rng.blob(1300));
                (addr(100 + rng.below(3000) as u32), d, "sealed kinds and noise")
            }
            _ => {
                // a token of this key and game with nothing in it that fits (a name too long, keys too short): sealed correctly, so it gets past the seal
                let mut private = rng.blob(200);
                private.extend_from_slice(&[0u8; 8]);
                let mut t = seed.clone();
                t.private = private;
                t.nonce = {
                    let mut n = [0u8; 24];
                    n.copy_from_slice(&rng.bytes(24));
                    n
                };
                (addr(100 + rng.below(3000) as u32), token_packet(&t, rng.next() as u32), "a made-up sealed part")
            }
        };
        let c = cost(&mut s, now, from, d);
        costs.note(c, what);
    }
    let worst = costs.report("tokens and sealed packets");
    assert!(worst < Duration::from_millis(20), "one datagram took {worst:?}");
    assert_eq!(s.client_count(), 0, "nobody got in with garbage");
    assert!(s.pending_len() <= 4096);
    // a real token, a fresh one, still gets a challenge, answers it sealed, and is accepted; the same token from another address is refused
    let tok = ConnectToken::issue(&key, 55, 2, "real", 1_000_000, 300, [3; 24], [4; 32], [5; 32]);
    let me = addr(7);
    s.transport.outbox.clear();
    s.transport.inbox.push_back((me, token_packet(&tok, 77)));
    now += 5;
    s.update(now);
    let challenge = s.transport.outbox.iter().find(|(to, d)| *to == me && d.get(1) == Some(&kind::CHALLENGE)).expect("a challenge for a good token").1.clone();
    let mut r = R::new(&challenge[2..]);
    let (cs, ss) = (r.u32().unwrap(), r.u32().unwrap());
    let mut ch = tok.channel();
    let mut out = vec![MAGIC, kind::RESPONSE_SEALED];
    ch.seal(&[MAGIC, kind::RESPONSE_SEALED], &(cs ^ ss).to_le_bytes(), &mut out);
    s.transport.inbox.push_back((me, out));
    now += 5;
    s.update(now);
    assert!(s.transport.outbox.iter().any(|(to, d)| *to == me && d.get(1) == Some(&kind::ACCEPT_SEALED)), "the holder of a real token is accepted");
    assert_eq!(s.client_count(), 1);
    s.transport.outbox.clear();
    s.transport.inbox.push_back((addr(8), token_packet(&tok, 78)));
    now += 5;
    s.update(now);
    assert!(!s.transport.outbox.iter().any(|(to, d)| *to == addr(8) && d.get(1) == Some(&kind::CHALLENGE)), "a token used from one address is not good from another");
}

#[test]
fn fuzz_a_save_written_over_a_damaged_one_does_not_make_the_damaged_one_the_save_before() {
    let dir = scratch("save_over_damaged");
    let path = dir.join("world.json");
    let (mut s, _) = saving_server(&path, &[700]);
    assert!(s.game.save_now());
    assert!(s.game.save_now());
    let good_before = std::fs::read(path.with_extension("bak")).unwrap();
    // the live file rots on the disk while the server runs: a digit changes
    let mut b = std::fs::read(&path).unwrap();
    let at = b.iter().enumerate().skip(b.len() / 2).find(|(_, c)| c.is_ascii_digit()).unwrap().0;
    b[at] = if b[at] == b'7' { b'8' } else { b'7' };
    std::fs::write(&path, &b).unwrap();
    s.game.sim.apply(&Command::Spawn { uid: Some(9703), components: vec![] }).unwrap();
    assert!(s.game.save_now());
    assert_eq!(corrupt_files(&dir), 1, "the rotten file was put aside, not kept as the save before");
    assert_eq!(std::fs::read(path.with_extension("bak")).unwrap(), good_before, "the save before the new one is still the good one from before the rot");
    assert_eq!(simgame::check_save(&std::fs::read_to_string(&path).unwrap()), Ok(true));
}
