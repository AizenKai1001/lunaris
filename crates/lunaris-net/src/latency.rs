//! A network you can make worse on purpose: a proxy that sits between a client and a host and holds every message back for a while, so
//! that rollback (and lockstep, which suffers) can be tried on one computer, by a test or by a person with two windows.
//!
//! **What it simulates, honestly.** TCP, which this crate uses, never loses data: a lost packet is sent again by the operating system and
//! the message arrives late. So "loss" here is not messages disappearing; it is what loss looks like to the game, which is a message that
//! comes much later than the rest (we add `max(3 x latency, 60 ms)`), and with jitter, messages that overtake each other. Nothing is ever
//! dropped for good, which is why a game played through it must end bit-identical to one played without it; if it does not, there is a
//! bug in the netcode, and that is what the tests use it for. Optional duplicates test that a message that arrives twice is harmless.
//!
//! The delay is **one way**, added in each direction: a latency of 50 ms makes the round trip 100 ms longer.
use crate::wire::Xorshift;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// How a simulated link behaves. All zero (the default) is a perfect link.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkSim {
    /// Every message is held back this long, in each direction.
    pub latency_ms: u64,
    /// ...plus or minus a random amount up to this (so messages can overtake each other).
    pub jitter_ms: u64,
    /// The chance, in percent, that a message is "lost and sent again": delivered `max(3 x latency, 60 ms)` late.
    pub loss_pct: f64,
    /// The chance, in percent, that a message is delivered twice.
    pub dup_pct: f64,
    /// Seed of the random choices, so the same settings give the same delays.
    pub seed: u64,
}

impl Default for LinkSim {
    fn default() -> LinkSim {
        LinkSim { latency_ms: 0, jitter_ms: 0, loss_pct: 0.0, dup_pct: 0.0, seed: 1 }
    }
}

impl LinkSim {
    /// A link with this one-way latency and jitter, in milliseconds.
    pub fn new(latency_ms: u64, jitter_ms: u64) -> LinkSim {
        LinkSim { latency_ms, jitter_ms, ..LinkSim::default() }
    }

    /// Does this change anything at all?
    pub fn is_off(&self) -> bool {
        self.latency_ms == 0 && self.jitter_ms == 0 && self.loss_pct <= 0.0 && self.dup_pct <= 0.0
    }

    /// How long the next message is held back, whether that included a "loss", and how long after it a duplicate follows (if one does).
    fn plan(&self, rng: &mut Xorshift) -> (Duration, bool, Option<Duration>) {
        let jitter = if self.jitter_ms > 0 { (rng.unit() * 2.0 - 1.0) * self.jitter_ms as f64 } else { 0.0 };
        let mut ms = (self.latency_ms as f64 + jitter).max(0.0);
        let lost = self.loss_pct > 0.0 && rng.unit() * 100.0 < self.loss_pct;
        if lost {
            ms += (3 * self.latency_ms).max(60) as f64;
        }
        let dup = (self.dup_pct > 0.0 && rng.unit() * 100.0 < self.dup_pct).then(|| Duration::from_millis(self.latency_ms.max(5)));
        (Duration::from_micros((ms * 1000.0) as u64), lost, dup)
    }
}

#[derive(Default)]
struct Counters {
    lines: AtomicU64,
    late: AtomicU64,
    duplicated: AtomicU64,
    overtaken: AtomicU64,
}

/// What a proxy has done so far.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProxyReport {
    /// Messages passed on (both directions).
    pub lines: u64,
    /// Messages that were held back extra long, as if they had been lost and sent again.
    pub late: u64,
    /// Messages delivered a second time.
    pub duplicated: u64,
    /// Messages delivered after a message that was sent later: the order was changed.
    pub overtaken: u64,
}

/// A proxy in front of a host: connect to `addr()` instead of the host's address and everything is delayed as `LinkSim` says. It lives
/// until it is dropped; connections that are already open go on working.
pub struct LatencyProxy {
    addr: String,
    stop: Arc<AtomicBool>,
    counters: Arc<Counters>,
}

impl LatencyProxy {
    /// Listen on a free port of this machine and relay every connection to `target` (`host:port`).
    pub fn start(target: &str, sim: LinkSim) -> Result<LatencyProxy, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("cannot start the latency proxy: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let addr = format!("127.0.0.1:{}", listener.local_addr().map_err(|e| e.to_string())?.port());
        let stop = Arc::new(AtomicBool::new(false));
        let counters = Arc::new(Counters::default());
        let (stop2, counters2, target) = (stop.clone(), counters.clone(), target.to_string());
        std::thread::spawn(move || {
            let mut connections = 0u64;
            while !stop2.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((client, _)) => {
                        connections += 1;
                        let _ = client.set_nonblocking(false);
                        let Ok(host) = TcpStream::connect(&target) else { continue }; // the client sees its connection end
                        let _ = (client.set_nodelay(true), host.set_nodelay(true));
                        let (Ok(client2), Ok(host2)) = (client.try_clone(), host.try_clone()) else { continue };
                        // two directions, each with its own random stream
                        pipe(client, host2, sim, sim.seed.wrapping_mul(2).wrapping_add(connections * 7919), counters2.clone());
                        pipe(host, client2, sim, sim.seed.wrapping_mul(2).wrapping_add(connections * 7919 + 1), counters2.clone());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(2)),
                    Err(_) => break,
                }
            }
        });
        Ok(LatencyProxy { addr, stop, counters })
    }

    /// The address clients connect to instead of the host's.
    pub fn addr(&self) -> &str {
        &self.addr
    }

    pub fn report(&self) -> ProxyReport {
        let c = &self.counters;
        ProxyReport { lines: c.lines.load(Ordering::SeqCst), late: c.late.load(Ordering::SeqCst), duplicated: c.duplicated.load(Ordering::SeqCst), overtaken: c.overtaken.load(Ordering::SeqCst) }
    }
}

impl Drop for LatencyProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// One direction: read lines from `src`, hold each back as the plan says, write them to `dst` in the order they fall due.
fn pipe(src: TcpStream, dst: TcpStream, sim: LinkSim, seed: u64, counters: Arc<Counters>) {
    let (tx, rx) = mpsc::channel::<(Instant, u64, String)>();
    let counters2 = counters.clone();
    std::thread::spawn(move || {
        let mut rng = Xorshift::new(seed);
        let mut reader = BufReader::new(src);
        let mut seq = 0u64;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return, // dropping `tx` lets the writer finish what is due and end
                Ok(_) => {}
            }
            let (delay, lost, dup) = sim.plan(&mut rng);
            counters.lines.fetch_add(1, Ordering::SeqCst);
            if lost {
                counters.late.fetch_add(1, Ordering::SeqCst);
            }
            let now = Instant::now();
            seq += 1;
            if let Some(extra) = dup {
                counters.duplicated.fetch_add(1, Ordering::SeqCst);
                let _ = tx.send((now + delay + extra, seq, line.clone()));
            }
            if tx.send((now + delay, seq, line)).is_err() {
                return;
            }
        }
    });
    std::thread::spawn(move || {
        let mut dst = dst;
        let mut heap: BinaryHeap<Reverse<(Instant, u64, String)>> = BinaryHeap::new();
        let mut open = true;
        let mut newest_sent = 0u64;
        loop {
            // what has been read since last time
            loop {
                match rx.try_recv() {
                    Ok(item) => heap.push(Reverse(item)),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        open = false;
                        break;
                    }
                }
            }
            // what is due
            let now = Instant::now();
            while heap.peek().is_some_and(|Reverse((due, _, _))| *due <= now) {
                let Some(Reverse((_, seq, line))) = heap.pop() else { break };
                if seq < newest_sent {
                    counters2.overtaken.fetch_add(1, Ordering::SeqCst);
                }
                newest_sent = newest_sent.max(seq);
                if dst.write_all(line.as_bytes()).is_err() {
                    let _ = dst.shutdown(Shutdown::Both);
                    return;
                }
            }
            match heap.peek() {
                None if !open => break,
                None => match rx.recv() {
                    Ok(item) => heap.push(Reverse(item)),
                    Err(_) => open = false,
                },
                Some(Reverse((due, _, _))) => {
                    let wait = due.saturating_duration_since(Instant::now());
                    if open {
                        match rx.recv_timeout(wait) {
                            Ok(item) => heap.push(Reverse(item)),
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => open = false,
                        }
                    } else {
                        std::thread::sleep(wait);
                    }
                }
            }
        }
        let _ = dst.shutdown(Shutdown::Write);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// A host that answers every line with the same line, with the time it took to arrive.
    fn echo_server() -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = format!("127.0.0.1:{}", listener.local_addr().expect("addr").port());
        let h = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().expect("accept");
            let mut r = BufReader::new(s.try_clone().expect("clone"));
            let mut line = String::new();
            while r.read_line(&mut line).unwrap_or(0) > 0 {
                let _ = s.write_all(line.as_bytes());
                line.clear();
            }
        });
        (addr, h)
    }

    #[test]
    fn a_perfect_link_passes_lines_through_in_order() {
        let (addr, host) = echo_server();
        let proxy = LatencyProxy::start(&addr, LinkSim::default()).expect("proxy");
        let mut c = TcpStream::connect(proxy.addr()).expect("connect");
        for i in 0..20 {
            c.write_all(format!("line {i}\n").as_bytes()).expect("write");
        }
        let mut r = BufReader::new(c.try_clone().expect("clone"));
        for i in 0..20 {
            let mut l = String::new();
            r.read_line(&mut l).expect("read");
            assert_eq!(l, format!("line {i}\n"));
        }
        drop((c, r));
        host.join().expect("host");
        assert_eq!(proxy.report().overtaken, 0);
    }

    #[test]
    fn latency_delays_both_directions_and_jitter_can_change_the_order() {
        let (addr, host) = echo_server();
        let proxy = LatencyProxy::start(&addr, LinkSim { latency_ms: 30, jitter_ms: 25, seed: 5, ..LinkSim::default() }).expect("proxy");
        let mut c = TcpStream::connect(proxy.addr()).expect("connect");
        let t0 = Instant::now();
        for i in 0..40 {
            c.write_all(format!("{i}\n").as_bytes()).expect("write");
        }
        let mut r = BufReader::new(c.try_clone().expect("clone"));
        let mut got = Vec::new();
        for _ in 0..40 {
            let mut l = String::new();
            r.read_line(&mut l).expect("read");
            got.push(l.trim().parse::<u32>().expect("number"));
        }
        let took = t0.elapsed();
        // there and back: at least about two latencies minus the jitter on both legs
        assert!(took >= Duration::from_millis(15), "{took:?}");
        let mut sorted = got.clone();
        sorted.sort();
        assert_eq!(sorted, (0..40).collect::<Vec<_>>(), "nothing is lost and nothing is invented");
        assert_ne!(got, sorted, "with this much jitter, 40 messages sent at once do not stay in order");
        assert!(proxy.report().overtaken > 0);
        drop((c, r));
        host.join().expect("host");
    }

    #[test]
    fn a_lost_message_arrives_late_not_never_and_a_duplicate_arrives_twice() {
        let (addr, host) = echo_server();
        let proxy = LatencyProxy::start(&addr, LinkSim { latency_ms: 5, loss_pct: 100.0, dup_pct: 100.0, seed: 3, ..LinkSim::default() }).expect("proxy");
        let mut c = TcpStream::connect(proxy.addr()).expect("connect");
        let t0 = Instant::now();
        c.write_all(b"hello\n").expect("write");
        let mut r = BufReader::new(c.try_clone().expect("clone"));
        let mut l = String::new();
        r.read_line(&mut l).expect("read");
        assert_eq!(l, "hello\n");
        assert!(t0.elapsed() >= Duration::from_millis(100), "two legs, each held back by the resend delay: {:?}", t0.elapsed());
        let rep = proxy.report();
        assert!(rep.late >= 1 && rep.duplicated >= 1, "{rep:?}");
        c.shutdown(Shutdown::Write).expect("shutdown");
        let mut rest = String::new();
        r.read_to_string(&mut rest).expect("rest");
        assert!(rest.contains("hello"), "the duplicates came too: {rest:?}");
        host.join().expect("host");
    }
}
