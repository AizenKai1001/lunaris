//! The client of a big match: connects (a handshake the server answers cheaply), sends the state of its own character thirty times a second, sends commands reliably, and keeps what the
//! server says about the others: a few recent records of each, from which [`NetClient::sample`] gives where an entity is at a time a little in the past (interpolated between the two
//! records round it, so that a player who moves at thirty updates a second, or ten, or three, or with a lost packet or two in the middle, still moves smoothly), or a little past the
//! last record (extrapolated from its velocity, for no longer than a quarter of a second).

use std::collections::{HashMap, VecDeque};

use super::bytes::{R, W};
use super::interest::is_static;
use super::proto::{kind, msg, rto, Acks, ReliableIn, ReliableOut, MAGIC, PROTOCOL};
use super::secure::{self, Channel, ConnectToken};
use super::server::ev;
use super::state::{self, class, EntityState};
use super::transport::{Addr, Transport};

/// What happens to a client that the game must know about.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The server has taken us in: our slot, the id of our character, where it starts (metres), and how many ticks a second the server runs.
    Connected { slot: u16, entity: u32, pos: [f32; 3], tick_hz: u16 },
    /// The server refused us (1: another protocol version, 2: full) or went quiet.
    Refused(u8),
    Disconnected,
    /// "Go back to this place": the server did not accept our last move (metres).
    Correct { pos: [f32; 3] },
    /// A message of the game (the first byte is its kind, below 0xF0).
    Game(Vec<u8>),
}

/// The recent records of one entity.
pub struct Remote {
    /// (server tick, record), oldest first.
    samples: VecDeque<(u32, EntityState)>,
    pub last_seen: u64,
}

impl Remote {
    pub fn latest(&self) -> Option<&EntityState> {
        self.samples.back().map(|s| &s.1)
    }

    /// The server tick of the latest record.
    pub fn last_tick(&self) -> u32 {
        self.samples.back().map_or(0, |s| s.0)
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Phase {
    Connecting,
    Challenged,
    Connected,
    Closed,
}

pub struct NetClient<T: Transport> {
    transport: T,
    server: Addr,
    name: String,
    phase: Phase,
    salt: u32,
    server_salt: u32,
    last_try: u64,
    conn: u16,
    acks: Acks,
    rel_out: ReliableOut,
    rel_in: ReliableIn,
    last_recv: u64,
    last_sent: u64,
    pub tick_hz: u16,
    pub slot: u16,
    pub entity: u32,
    events: VecDeque<Event>,
    remotes: HashMap<u32, Remote>,
    own: Option<EntityState>,
    // the server's clock as this one sees it: (server time in ms) - (our time in ms)
    clock_offset: Option<f64>,
    pub interp_ms: f64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub packets_in: u64,
    pub packets_out: u64,
    pub records_in: u64,
    seen_snapshot: u32,
    send_gap_ms: u64,
    /// The longest (milliseconds of the server's time) a player that was within `NEAR_M` of our own character at both ends went between two of its records: the figure that says whether
    /// the people next to you are seen smoothly. (A player that goes out of range or is put somewhere else is not a gap.)
    pub near_gap_ms: u64,
    /// How many of those gaps there were of each length in ticks (the last place is 31 and more): `near_gaps[1]` is a record every tick.
    pub near_gaps: [u64; 32],
    /// How near a player is to count in the gap figures (metres; [`NEAR_M`] to start with).
    pub near_m: f32,
    last_prune: u64,
    /// With a connect token (docs/SECURE.md): the token, and the channel everything is sealed through.
    token: Option<ConnectToken>,
    chan: Option<Channel>,
}

/// How near a player is, to start with, to count in [`NetClient::near_gap_ms`].
pub const NEAR_M: f32 = 30.0;

impl<T: Transport> NetClient<T> {
    pub fn new(transport: T, server: Addr, name: &str, salt: u32) -> NetClient<T> {
        NetClient {
            transport,
            server,
            name: name.to_string(),
            phase: Phase::Connecting,
            salt,
            server_salt: 0,
            last_try: 0,
            conn: 0,
            acks: Acks::new(),
            rel_out: ReliableOut::default(),
            rel_in: ReliableIn::default(),
            last_recv: 0,
            last_sent: 0,
            tick_hz: 30,
            slot: 0,
            entity: 0,
            events: VecDeque::new(),
            remotes: HashMap::new(),
            own: None,
            clock_offset: None,
            interp_ms: 100.0,
            bytes_in: 0,
            bytes_out: 0,
            packets_in: 0,
            packets_out: 0,
            records_in: 0,
            seen_snapshot: 0,
            send_gap_ms: 33,
            near_gap_ms: 0,
            near_gaps: [0; 32],
            near_m: NEAR_M,
            last_prune: 0,
            token: None,
            chan: None,
        }
    }

    /// A client for a server that wants connect tokens (`ServerConfig::auth`): the name is the one the token was made for, and everything that is sent is sealed with the token's keys.
    pub fn with_token(transport: T, server: Addr, token: ConnectToken, salt: u32) -> NetClient<T> {
        let mut c = NetClient::new(transport, server, &token.name, salt);
        c.chan = Some(token.channel());
        c.token = Some(token);
        c
    }

    pub fn connected(&self) -> bool {
        self.phase == Phase::Connected
    }

    pub fn rtt_ms(&self) -> f64 {
        self.acks.srtt
    }

    pub fn addr(&self) -> Addr {
        self.server
    }

    /// The state of our own character, to be sent with the next packet (every thirtieth of a second or so).
    pub fn set_state(&mut self, s: EntityState) {
        self.own = Some(s);
    }

    /// A command for the game on the server, delivered once, in order (the first byte is its kind, below 0xF0).
    pub fn command(&mut self, data: Vec<u8>) {
        self.rel_out.push(data);
    }

    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    pub fn remotes(&self) -> &HashMap<u32, Remote> {
        &self.remotes
    }

    /// Leaves the match (the server is told, once, and may not hear: it drops us in a few seconds then).
    pub fn disconnect(&mut self) {
        if self.phase == Phase::Connected {
            let mut w = W::new();
            w.u8(MAGIC);
            w.u8(kind::DISCONNECT);
            w.u16(self.conn);
            if let Some(ch) = self.chan.as_mut() {
                let mut out = vec![MAGIC, kind::DISCONNECT_SEALED, w.buf[2], w.buf[3]];
                let aad = [out[0], out[1], out[2], out[3]];
                ch.seal(&aad, &[], &mut out);
                self.transport.send(self.server, &out);
            } else {
                self.transport.send(self.server, &w.buf);
            }
        }
        self.phase = Phase::Closed;
    }

    /// The server's clock now, in milliseconds, as far as this client can tell (`None` before the first snapshot).
    pub fn server_ms(&self, now: u64) -> Option<f64> {
        self.clock_offset.map(|o| now as f64 + o)
    }

    /// The record of entity `id` as it was at the time the picture should show: `interp_ms` before the server's present, interpolated between the records round it; if there is none
    /// that late yet, extrapolated from the last record's velocity for up to a quarter of a second.
    pub fn sample(&self, id: u32, now: u64) -> Option<EntityState> {
        let r = self.remotes.get(&id)?;
        let render = self.server_ms(now)? - self.interp_ms;
        let ms_of = |t: u32| t as f64 * 1000.0 / self.tick_hz as f64;
        let first = r.samples.front()?;
        if render <= ms_of(first.0) {
            return Some(first.1);
        }
        for w in r.samples.iter().collect::<Vec<_>>().windows(2) {
            let (a, b) = (w[0], w[1]);
            let (ta, tb) = (ms_of(a.0), ms_of(b.0));
            if render >= ta && render <= tb {
                let t = if tb > ta { ((render - ta) / (tb - ta)) as f32 } else { 1.0 };
                return Some(state::lerp(&a.1, &b.1, t));
            }
        }
        let last = r.samples.back()?;
        let over = ((render - ms_of(last.0)) / 1000.0).clamp(0.0, 0.25) as f32;
        let mut s = last.1;
        let v = s.vel_ms();
        let p = s.pos_m();
        s.set_pos_m([p[0] + v[0] * over, p[1] + v[1] * over, p[2] + v[2] * over]);
        Some(s)
    }

    /// Forget what has not been heard of for two seconds (out of range, or gone); a thing that does not move is sent once a second, so it gets five (a lost packet or two must not make it blink out).
    /// Done on a clock, not when a snapshot arrives: a client with nobody in range gets no snapshots at all.
    fn prune(&mut self, now: u64) {
        self.remotes.retain(|_, r| {
            let limit = if r.latest().is_some_and(|l| is_static(l.class)) { 5000 } else { 2000 };
            now.saturating_sub(r.last_seen) < limit
        });
    }

    /// Reads what has arrived, sends what is due, at time `now` (milliseconds).
    pub fn update(&mut self, now: u64) {
        if now >= self.last_prune + 250 {
            self.last_prune = now;
            self.prune(now);
        }
        while let Some((from, data)) = self.transport.recv() {
            if from != self.server {
                continue;
            }
            self.packets_in += 1;
            self.bytes_in += data.len() as u64;
            self.handle(&data, now);
        }
        match self.phase {
            Phase::Connecting | Phase::Challenged => {
                if now >= self.last_try + 250 || self.last_try == 0 {
                    self.last_try = now.max(1);
                    let mut w = W::new();
                    w.u8(MAGIC);
                    if self.phase == Phase::Connecting {
                        if let Some(t) = &self.token {
                            w.u8(kind::CONNECT_TOKEN);
                            w.u8(PROTOCOL);
                            w.u32(self.salt);
                            w.raw(&t.protocol_id.to_le_bytes());
                            w.raw(&t.expire.to_le_bytes());
                            w.raw(&t.nonce);
                            w.bytes(&t.private);
                        } else {
                            w.u8(kind::CONNECT);
                            w.u8(PROTOCOL);
                            w.u32(self.salt);
                            w.bytes(self.name.as_bytes());
                        }
                    } else if let Some(ch) = self.chan.as_mut() {
                        let mut proof = W::new();
                        proof.u32(self.salt ^ self.server_salt);
                        w.u8(kind::RESPONSE_SEALED);
                        ch.seal(&[MAGIC, kind::RESPONSE_SEALED], &proof.buf, &mut w.buf);
                    } else {
                        w.u8(kind::RESPONSE);
                        w.u32(self.salt ^ self.server_salt);
                    }
                    self.packets_out += 1;
                    self.bytes_out += w.len() as u64;
                    self.transport.send(self.server, &w.buf);
                }
            }
            Phase::Connected => {
                if now.saturating_sub(self.last_recv) > 5000 {
                    self.phase = Phase::Closed;
                    self.events.push_back(Event::Disconnected);
                    return;
                }
                if now >= self.last_sent + self.send_gap_ms {
                    self.send_data(now);
                }
            }
            Phase::Closed => {}
        }
    }

    fn send_data(&mut self, now: u64) {
        let mut w = W::new();
        let (ack, bits) = self.acks.header();
        let seq = self.acks.local_seq;
        w.u8(MAGIC);
        w.u8(kind::DATA);
        w.u16(self.conn);
        w.u16(seq);
        w.u16(ack);
        w.u32(bits);
        let rel = self.rel_out.due(now, rto(self.acks.srtt), 600);
        let mut rel_seqs = Vec::with_capacity(rel.len());
        for (rseq, body) in &rel {
            w.u8(msg::RELIABLE);
            w.varu(body.len() as u64 + 2);
            w.u16(*rseq);
            w.raw(body);
            rel_seqs.push(*rseq);
        }
        if let Some(s) = self.own {
            let mut b = W::new();
            state::write_own(&mut b, &s);
            w.u8(msg::STATE);
            w.varu(b.len() as u64);
            w.raw(&b.buf);
        }
        self.acks.on_send(now, rel_seqs);
        self.last_sent = now;
        self.packets_out += 1;
        if let Some(ch) = self.chan.as_mut() {
            let mut out = Vec::with_capacity(w.len() + secure::OVERHEAD);
            out.extend_from_slice(&[MAGIC, kind::DATA_SEALED, w.buf[2], w.buf[3]]);
            let aad = [out[0], out[1], out[2], out[3]];
            ch.seal(&aad, &w.buf[4..], &mut out);
            self.bytes_out += out.len() as u64;
            self.transport.send(self.server, &out);
        } else {
            self.bytes_out += w.len() as u64;
            self.transport.send(self.server, &w.buf);
        }
    }

    /// What came in: with a connect token only the challenge and a refusal are believed in the clear, the accept and the data are opened first (a forged or changed or replayed one is dropped).
    fn handle(&mut self, d: &[u8], now: u64) {
        if d.len() < 2 || d[0] != MAGIC {
            return;
        }
        let Some(ch) = self.chan.as_mut() else { return self.handle_plain(d, now) };
        let rebuilt = match d[1] {
            kind::CHALLENGE | kind::REJECT => return self.handle_plain(d, now),
            kind::ACCEPT_SEALED => ch.open(&d[..2], &d[2..]).map(|plain| {
                let mut v = vec![MAGIC, kind::ACCEPT];
                v.extend_from_slice(&plain);
                v
            }),
            kind::DATA_SEALED if d.len() >= 4 => ch.open(&d[..4], &d[4..]).map(|plain| {
                let mut v = vec![MAGIC, kind::DATA, d[2], d[3]];
                v.extend_from_slice(&plain);
                v
            }),
            _ => None,
        };
        if let Some(v) = rebuilt {
            self.handle_plain(&v, now);
        }
    }

    fn handle_plain(&mut self, d: &[u8], now: u64) {
        let mut r = R::new(&d[2..]);
        match d[1] {
            kind::CHALLENGE if self.phase == Phase::Connecting => {
                let (Some(cs), Some(ss)) = (r.u32(), r.u32()) else { return };
                if cs == self.salt {
                    self.server_salt = ss;
                    self.phase = Phase::Challenged;
                    self.last_try = 0;
                }
            }
            kind::ACCEPT if self.phase != Phase::Connected && self.phase != Phase::Closed => {
                let (Some(conn), Some(slot), Some(hz), Some(entity), Some(tick)) = (r.u16(), r.u16(), r.u16(), r.u32(), r.u32()) else { return };
                let mut pos = [0.0f32; 3];
                for p in pos.iter_mut() {
                    let Some(v) = r.vari() else { return };
                    *p = v as f32 / state::POS_SCALE;
                }
                self.conn = conn;
                self.slot = slot;
                self.tick_hz = hz.max(1);
                self.entity = entity;
                self.phase = Phase::Connected;
                self.last_recv = now;
                self.seen_snapshot = tick;
                self.events.push_back(Event::Connected { slot, entity, pos, tick_hz: hz });
            }
            kind::REJECT => {
                if let Some(why) = r.u8() {
                    self.phase = Phase::Closed;
                    self.events.push_back(Event::Refused(why));
                }
            }
            kind::DATA if self.phase == Phase::Connected => {
                let mut r = R::new(d);
                let _ = (r.u8(), r.u8());
                let (Some(conn), Some(seq), Some(ack), Some(bits)) = (r.u16(), r.u16(), r.u16(), r.u32()) else { return };
                if conn != self.conn {
                    return;
                }
                self.last_recv = now;
                let fresh = self.acks.on_receive(seq);
                for rel in self.acks.on_ack(ack, bits, now) {
                    self.rel_out.acked(rel);
                }
                if !fresh {
                    return;
                }
                while r.left() > 0 {
                    let (Some(t), Some(body)) = (r.u8(), r.bytes()) else { break };
                    match t {
                        msg::SNAPSHOT => self.on_snapshot(body, now),
                        msg::RELIABLE => {
                            let mut rr = R::new(body);
                            if let Some(rseq) = rr.u16() {
                                for payload in self.rel_in.receive(rseq, body[2..].to_vec()) {
                                    self.on_message(payload);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn on_message(&mut self, payload: Vec<u8>) {
        match payload.first() {
            Some(&ev::CORRECT) => {
                let mut r = R::new(&payload[1..]);
                let mut pos = [0.0f32; 3];
                for p in pos.iter_mut() {
                    let Some(v) = r.vari() else { return };
                    *p = v as f32 / state::POS_SCALE;
                }
                self.events.push_back(Event::Correct { pos });
            }
            Some(_) => self.events.push_back(Event::Game(payload)),
            None => {}
        }
    }

    fn on_snapshot(&mut self, body: &[u8], now: u64) {
        let mut r = R::new(body);
        let (Some(tick), Some(count)) = (r.u32(), r.varu()) else { return };
        // the clock: how far the server's is from ours (half the round trip is the time the snapshot has been on its way)
        let server_ms = tick as f64 * 1000.0 / self.tick_hz as f64;
        let sample = server_ms + self.acks.srtt * 0.5 - now as f64;
        self.clock_offset = Some(match self.clock_offset {
            None => sample,
            Some(o) => o + (sample - o) * 0.05,
        });
        let own_pos = self.own.as_ref().map(|o| o.pos_m());
        for _ in 0..count {
            let Some((s, _)) = state::read(&mut r) else { break };
            self.records_in += 1;
            let rem = self.remotes.entry(s.id).or_insert_with(|| Remote { samples: VecDeque::new(), last_seen: now });
            if let (Some(me), Some(before)) = (own_pos, rem.latest()) {
                let near_m = self.near_m;
                let near = |p: [f32; 3]| ((p[0] - me[0]).powi(2) + (p[2] - me[2]).powi(2)).sqrt() < near_m;
                // (in the server's time, the ticks between the two records: how often the server sent it, which a slow reader of the socket does not change)
                if s.class == class::PLAYER && tick > rem.last_tick() && near(before.pos_m()) && near(s.pos_m()) {
                    let ticks = (tick - rem.last_tick()) as u64;
                    self.near_gap_ms = self.near_gap_ms.max(ticks * 1000 / self.tick_hz.max(1) as u64);
                    self.near_gaps[ticks.min(31) as usize] += 1;
                }
            }
            rem.last_seen = now;
            if rem.samples.back().is_none_or(|b| tick > b.0) {
                rem.samples.push_back((tick, s));
                while rem.samples.len() > 6 {
                    rem.samples.pop_front();
                }
            } else if let Some(i) = rem.samples.iter().position(|x| x.0 == tick) {
                rem.samples[i].1 = s;
            }
        }
        self.seen_snapshot = tick;
    }
}
