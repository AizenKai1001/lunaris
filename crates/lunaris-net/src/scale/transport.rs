//! Datagrams in and out. The server and the client only know the [`Transport`] trait: [`UdpTransport`] is a real UDP socket, [`SimNet`] is a network in memory with a clock the test
//! owns, a latency, a jitter that reorders, packets that are lost or doubled (all from a seeded generator: the same run happens every time), so that a hundred clients and a server
//! can be played against each other in a test in a second, with the network as bad as you like.

use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::net::{SocketAddr, UdpSocket};
use std::rc::Rc;

pub type Addr = SocketAddr;

/// The biggest datagram anything here sends: under the usual 1500-byte limit of a link with room for the headers of IP and UDP and a VPN.
pub const MAX_DATAGRAM: usize = 1200;

pub trait Transport {
    /// Sends one datagram (it may be lost: nothing is said).
    fn send(&mut self, to: Addr, data: &[u8]);
    /// The next datagram that has arrived, if any.
    fn recv(&mut self) -> Option<(Addr, Vec<u8>)>;
}

/// A UDP socket that never blocks.
pub struct UdpTransport {
    socket: UdpSocket,
    buf: Vec<u8>,
}

impl UdpTransport {
    pub fn bind(addr: &str) -> std::io::Result<UdpTransport> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        Ok(UdpTransport { socket, buf: vec![0u8; 2048] })
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }
}

impl Transport for UdpTransport {
    fn send(&mut self, to: Addr, data: &[u8]) {
        let _ = self.socket.send_to(data, to);
    }

    fn recv(&mut self) -> Option<(Addr, Vec<u8>)> {
        loop {
            match self.socket.recv_from(&mut self.buf) {
                Ok((n, from)) => return Some((from, self.buf[..n].to_vec())),
                // (Windows tells a socket that an earlier datagram of its was refused: not a reason to stop listening)
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => continue,
                Err(_) => return None,
            }
        }
    }
}

/// How bad a link is.
#[derive(Clone, Copy, Debug)]
pub struct Link {
    /// Milliseconds a datagram takes at least.
    pub latency_ms: u64,
    /// Up to this many milliseconds more, at random (which reorders).
    pub jitter_ms: u64,
    /// The share (0 to 1) of datagrams that are lost.
    pub loss: f32,
    /// The share that arrive twice.
    pub duplicate: f32,
}

impl Link {
    pub fn perfect() -> Link {
        Link { latency_ms: 0, jitter_ms: 0, loss: 0.0, duplicate: 0.0 }
    }

    pub fn internet() -> Link {
        Link { latency_ms: 40, jitter_ms: 20, loss: 0.03, duplicate: 0.005 }
    }
}

struct SimInner {
    now: u64,
    rng: u64,
    next_port: u16,
    default_link: Link,
    // a link per address (what a datagram from or to it goes through: the worse of the two ends)
    links: HashMap<Addr, Link>,
    // the datagrams on their way, by the address they go to: (arrival time, order, from, bytes)
    queues: HashMap<Addr, BinaryHeap<Reverse<(u64, u64, Addr, Vec<u8>)>>>,
    order: u64,
    /// Datagrams and bytes put on the network, and lost.
    pub sent: u64,
    pub sent_bytes: u64,
    pub lost: u64,
}

impl SimInner {
    fn rand(&mut self) -> f32 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }
}

/// A network in memory: [`SimNet::endpoint`] makes a transport that can send to the others, [`SimNet::set_now`] moves the clock.
#[derive(Clone)]
pub struct SimNet {
    inner: Rc<RefCell<SimInner>>,
}

impl SimNet {
    pub fn new(link: Link, seed: u64) -> SimNet {
        SimNet {
            inner: Rc::new(RefCell::new(SimInner {
                now: 0,
                rng: seed.max(1),
                next_port: 10_000,
                default_link: link,
                links: HashMap::new(),
                queues: HashMap::new(),
                order: 0,
                sent: 0,
                sent_bytes: 0,
                lost: 0,
            })),
        }
    }

    pub fn endpoint(&self) -> SimEndpoint {
        let mut s = self.inner.borrow_mut();
        let port = s.next_port;
        s.next_port += 1;
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        s.queues.insert(addr, BinaryHeap::new());
        SimEndpoint { net: self.clone(), addr }
    }

    pub fn set_now(&self, ms: u64) {
        self.inner.borrow_mut().now = ms;
    }

    pub fn now(&self) -> u64 {
        self.inner.borrow().now
    }

    /// The link of one endpoint: what its datagrams, in and out, go through (a player on a bad connection).
    pub fn set_link(&self, addr: Addr, link: Link) {
        self.inner.borrow_mut().links.insert(addr, link);
    }

    /// (datagrams sent, bytes sent, datagrams lost) since the start.
    pub fn totals(&self) -> (u64, u64, u64) {
        let s = self.inner.borrow();
        (s.sent, s.sent_bytes, s.lost)
    }
}

pub struct SimEndpoint {
    net: SimNet,
    addr: Addr,
}

impl SimEndpoint {
    pub fn addr(&self) -> Addr {
        self.addr
    }
}

impl Transport for SimEndpoint {
    fn send(&mut self, to: Addr, data: &[u8]) {
        let mut s = self.net.inner.borrow_mut();
        s.sent += 1;
        s.sent_bytes += data.len() as u64;
        // (one hop through the network: the default link; an endpoint with a link of its own, a player on a bad connection, adds its own to what its datagrams go through, in and out)
        let link = match (s.links.get(&self.addr).copied(), s.links.get(&to).copied()) {
            (None, None) => s.default_link,
            (Some(a), None) | (None, Some(a)) => Link { latency_ms: a.latency_ms.max(s.default_link.latency_ms), jitter_ms: a.jitter_ms.max(s.default_link.jitter_ms), loss: 1.0 - (1.0 - a.loss) * (1.0 - s.default_link.loss), duplicate: a.duplicate.max(s.default_link.duplicate) },
            (Some(a), Some(b)) => Link { latency_ms: a.latency_ms.max(b.latency_ms), jitter_ms: a.jitter_ms.max(b.jitter_ms), loss: 1.0 - (1.0 - a.loss) * (1.0 - b.loss), duplicate: a.duplicate.max(b.duplicate) },
        };
        if !s.queues.contains_key(&to) {
            return;
        }
        if s.rand() < link.loss {
            s.lost += 1;
            return;
        }
        let copies = if s.rand() < link.duplicate { 2 } else { 1 };
        for _ in 0..copies {
            let jitter = (s.rand() * (link.jitter_ms as f32 + 1.0)) as u64;
            let at = s.now + link.latency_ms + jitter.min(link.jitter_ms);
            s.order += 1;
            let order = s.order;
            let from = self.addr;
            if let Some(q) = s.queues.get_mut(&to) {
                q.push(Reverse((at, order, from, data.to_vec())));
            }
        }
    }

    fn recv(&mut self) -> Option<(Addr, Vec<u8>)> {
        let mut s = self.net.inner.borrow_mut();
        let now = s.now;
        let q = s.queues.get_mut(&self.addr)?;
        if q.peek().is_some_and(|Reverse((at, _, _, _))| *at <= now) {
            let Reverse((_, _, from, bytes)) = q.pop()?;
            return Some((from, bytes));
        }
        None
    }
}
