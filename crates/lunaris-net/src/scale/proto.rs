//! The packets: a header that carries acknowledgements both ways (so each side learns which of its packets arrived, how long they took, and which of its reliable messages to send again),
//! and the messages inside a data packet.
//!
//! ```text
//! handshake   [MAGIC][kind][...]                         kinds CONNECT, CHALLENGE, RESPONSE, ACCEPT, REJECT (the handshake costs the server nothing until a client has answered
//!                                                        its challenge: a spoofed address never gets a slot, and a reply is never bigger than the request that asked for it)
//! data        [MAGIC][DATA][conn u16][seq u16][ack u16][ack_bits u32] then messages [type u8][len varint][body] until the datagram ends
//! disconnect  [MAGIC][DISCONNECT][conn u16]
//! ```
//!
//! Messages: `STATE` (client to server, unreliable, the latest state of the client's own character), `SNAPSHOT` (server to client, unreliable, the entities that matter to it now),
//! `RELIABLE` (either way: a sequence number and a message the game wants to arrive, once, in order: a shot fired, a hit, a kill, the zone shrinking).

use std::collections::{HashMap, VecDeque};

pub const MAGIC: u8 = 0xB7;
pub const PROTOCOL: u8 = 1;

pub mod kind {
    pub const CONNECT: u8 = 1;
    pub const CHALLENGE: u8 = 2;
    pub const RESPONSE: u8 = 3;
    pub const ACCEPT: u8 = 4;
    pub const REJECT: u8 = 5;
    pub const DATA: u8 = 6;
    pub const DISCONNECT: u8 = 7;
    // the same handshake and packets on a server that wants a connect token (docs/SECURE.md): `CONNECT_TOKEN` carries the token's sealed part, the others are the plain ones sealed with the session keys
    pub const CONNECT_TOKEN: u8 = 8;
    pub const RESPONSE_SEALED: u8 = 9;
    pub const ACCEPT_SEALED: u8 = 10;
    pub const DATA_SEALED: u8 = 11;
    pub const DISCONNECT_SEALED: u8 = 12;
}

pub mod msg {
    pub const STATE: u8 = 0x10;
    pub const SNAPSHOT: u8 = 0x20;
    pub const RELIABLE: u8 = 0x30;
}

/// The size of a data packet's header.
pub const HEADER: usize = 1 + 1 + 2 + 2 + 2 + 4;

/// `a` is later than `b` in the circle of 16-bit sequence numbers.
pub fn seq_gt(a: u16, b: u16) -> bool {
    (a > b && a - b <= 32768) || (a < b && b - a > 32768)
}

/// How many packets back an acknowledgement remembers (the number in `ack_bits`, and of the packets whose contents are kept).
const WINDOW: usize = 64;

struct Sent {
    seq: u16,
    at: u64,
    rel: Vec<u16>,
    live: bool,
}

/// The state of one end's sequence numbers and acknowledgements.
pub struct Acks {
    /// The number of the next packet to send.
    pub local_seq: u16,
    remote_seq: u16,
    have_remote: bool,
    /// Bit `i` is set when the packet `remote_seq - 1 - i` arrived.
    recv_bits: u32,
    sent: Vec<Sent>,
    /// A smoothed round trip time in milliseconds (0 until a packet has been acknowledged).
    pub srtt: f64,
    /// Packets sent, and of those the ones acknowledged.
    pub packets_sent: u64,
    pub packets_acked: u64,
}

impl Default for Acks {
    fn default() -> Self {
        Acks::new()
    }
}

impl Acks {
    pub fn new() -> Acks {
        Acks { local_seq: 0, remote_seq: 0, have_remote: false, recv_bits: 0, sent: (0..WINDOW).map(|_| Sent { seq: 0, at: 0, rel: Vec::new(), live: false }).collect(), srtt: 0.0, packets_sent: 0, packets_acked: 0 }
    }

    /// Notes that the packet `seq` of the other end arrived. `false` for a duplicate (its messages must not be handled twice).
    pub fn on_receive(&mut self, seq: u16) -> bool {
        if !self.have_remote {
            self.have_remote = true;
            self.remote_seq = seq;
            self.recv_bits = 0;
            return true;
        }
        if seq == self.remote_seq {
            return false;
        }
        if seq_gt(seq, self.remote_seq) {
            let d = seq.wrapping_sub(self.remote_seq) as u32;
            self.recv_bits = if d > 32 { 0 } else { (if d == 32 { 0 } else { self.recv_bits << d }) | (1u32 << (d - 1)) };
            self.remote_seq = seq;
            true
        } else {
            let d = self.remote_seq.wrapping_sub(seq) as u32;
            if d > 32 {
                return true;
            }
            let bit = 1u32 << (d - 1);
            if self.recv_bits & bit != 0 {
                return false;
            }
            self.recv_bits |= bit;
            true
        }
    }

    /// What to put in the header of the next packet: the latest packet that arrived and the 32 before it.
    pub fn header(&self) -> (u16, u32) {
        (self.remote_seq, self.recv_bits)
    }

    /// Notes a packet that is being sent: its number, the time, and the reliable messages it carries. Returns the number.
    pub fn on_send(&mut self, now: u64, rel: Vec<u16>) -> u16 {
        let seq = self.local_seq;
        self.local_seq = self.local_seq.wrapping_add(1);
        self.packets_sent += 1;
        let slot = &mut self.sent[seq as usize % WINDOW];
        *slot = Sent { seq, at: now, rel, live: true };
        seq
    }

    /// Handles the acknowledgement in a packet's header: every packet of ours it names is delivered, once. Returns the reliable messages that were in them.
    pub fn on_ack(&mut self, ack: u16, bits: u32, now: u64) -> Vec<u16> {
        let mut acked = Vec::new();
        self.ack_one(ack, now, &mut acked);
        for i in 0..32u32 {
            if bits & (1 << i) != 0 {
                self.ack_one(ack.wrapping_sub(1 + i as u16), now, &mut acked);
            }
        }
        acked
    }

    fn ack_one(&mut self, seq: u16, now: u64, acked: &mut Vec<u16>) {
        let slot = &mut self.sent[seq as usize % WINDOW];
        if !slot.live || slot.seq != seq {
            return;
        }
        slot.live = false;
        self.packets_acked += 1;
        let rtt = now.saturating_sub(slot.at) as f64;
        self.srtt = if self.srtt == 0.0 { rtt } else { self.srtt * 0.875 + rtt * 0.125 };
        acked.append(&mut slot.rel);
    }
}

struct Pending {
    seq: u16,
    body: Vec<u8>,
    last_sent: Option<u64>,
    // wanted by the other end at once (a hit on you, a correction), not at its next convenient packet (a kill feed line)
    urgent: bool,
}

/// Reliable messages going out: kept and sent again until the other end has acknowledged a packet that carried them.
#[derive(Default)]
pub struct ReliableOut {
    next_seq: u16,
    pending: VecDeque<Pending>,
}

impl ReliableOut {
    pub fn push(&mut self, body: Vec<u8>) {
        self.push_with(body, true);
    }

    /// A message that may wait for the receiver's next packet (it does not make a quiet client's packet come sooner).
    pub fn push_lazy(&mut self, body: Vec<u8>) {
        self.push_with(body, false);
    }

    fn push_with(&mut self, body: Vec<u8>, urgent: bool) {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.pending.push_back(Pending { seq, body, last_sent: None, urgent });
    }

    /// Whether an urgent message has never been sent: a packet is due now.
    pub fn needs_packet_now(&self) -> bool {
        self.pending.iter().any(|p| p.urgent && p.last_sent.is_none())
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// The messages to put in a packet now: the ones never sent, and the ones last sent more than `rto` ms ago, up to `budget` bytes (the oldest first).
    pub fn due(&mut self, now: u64, rto: u64, budget: usize) -> Vec<(u16, Vec<u8>)> {
        let mut out = Vec::new();
        let mut used = 0usize;
        for p in self.pending.iter_mut() {
            let ready = p.last_sent.is_none_or(|t| now.saturating_sub(t) >= rto);
            if !ready {
                continue;
            }
            let cost = p.body.len() + 6;
            if used + cost > budget && !out.is_empty() {
                break;
            }
            used += cost;
            p.last_sent = Some(now);
            out.push((p.seq, p.body.clone()));
        }
        out
    }

    pub fn acked(&mut self, seq: u16) {
        self.pending.retain(|p| p.seq != seq);
    }
}

/// Reliable messages coming in: handed over once each, in the order they were sent.
#[derive(Default)]
pub struct ReliableIn {
    next: u16,
    ahead: HashMap<u16, Vec<u8>>,
}

impl ReliableIn {
    /// Takes a message; returns the messages that can now be handled (this one and any that were waiting for it), in order.
    pub fn receive(&mut self, seq: u16, body: Vec<u8>) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        if seq == self.next {
            out.push(body);
            self.next = self.next.wrapping_add(1);
            while let Some(b) = self.ahead.remove(&self.next) {
                out.push(b);
                self.next = self.next.wrapping_add(1);
            }
        } else if seq_gt(seq, self.next) && seq.wrapping_sub(self.next) < 256 {
            self.ahead.insert(seq, body);
        }
        out
    }
}

/// The time to wait before sending a reliable message again, from the round trip time.
pub fn rto(srtt: f64) -> u64 {
    ((srtt * 1.5) as u64 + 20).clamp(60, 1000)
}
