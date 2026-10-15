//! Lines of JSON over TCP: a writer, and a reader thread that turns a connection into a channel of messages.
use lunaris_json::{parse, Json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};

/// The longest line either side accepts. Bigger than any real message; stops a broken peer from filling memory.
const MAX_LINE: u64 = 1 << 20;

/// How many bytes may wait in one connection's outgoing queue (`Outbox`) before the host gives up on it: a spectator whose network is
/// slower than the game must not make the host's memory grow without end.
pub const MAX_QUEUE: usize = 32 << 20;

pub fn send(stream: &mut TcpStream, v: &Json) -> Result<(), String> {
    let mut line = v.to_string_compact().map_err(|e| e.to_string())?;
    line.push('\n');
    stream.write_all(line.as_bytes()).map_err(|e| format!("the connection broke: {e}"))
}

/// Read lines from `stream` until the connection ends (or a line is not JSON or is too long); every parsed message goes to `on_msg`,
/// which returns false to stop reading.
fn read_lines(stream: TcpStream, mut on_msg: impl FnMut(Json) -> bool) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.by_ref().take(MAX_LINE).read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if !line.ends_with('\n') {
                    break; // too long, or the connection ended in the middle of a line
                }
                match parse(line.trim()) {
                    Ok(v) => {
                        if !on_msg(v) {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
}

/// Read lines from `stream` on a thread; every parsed message goes to the channel, which closes when the connection does (or when a
/// line is not JSON or is too long).
pub fn spawn_reader(stream: TcpStream) -> mpsc::Receiver<Json> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || read_lines(stream, |v| tx.send(v).is_ok()));
    rx
}

/// Like `spawn_reader`, but the messages are handed to `on_msg` on the reader's own thread (no channel in between), and `on_end` runs when
/// the connection ends. The host uses it to tag every message with the connection it came from.
pub fn spawn_line_reader(stream: TcpStream, on_msg: impl FnMut(Json) -> bool + Send + 'static, on_end: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        read_lines(stream, on_msg);
        on_end();
    });
}

/// The sending side of a connection that never blocks whoever sends: lines are queued and a thread of its own writes them. A relay that
/// writes straight to every client stops for everybody when one client stops reading; with an `Outbox` it only loses that client.
pub struct Outbox {
    tx: Option<mpsc::Sender<String>>,
    queued: Arc<AtomicUsize>,
    /// Set by the writer thread when a write failed (the peer is gone).
    broken: Arc<AtomicBool>,
    stream: TcpStream,
}

impl Outbox {
    /// Take over writing to `stream` (the caller keeps no other writer).
    pub fn new(stream: TcpStream) -> std::io::Result<Outbox> {
        let (tx, rx) = mpsc::channel::<String>();
        let queued = Arc::new(AtomicUsize::new(0));
        let broken = Arc::new(AtomicBool::new(false));
        let mut writer = stream.try_clone()?;
        let (q, b) = (queued.clone(), broken.clone());
        std::thread::spawn(move || {
            while let Ok(line) = rx.recv() {
                let n = line.len();
                if writer.write_all(line.as_bytes()).is_err() {
                    b.store(true, Ordering::SeqCst);
                    q.store(0, Ordering::SeqCst);
                    // keep draining so that senders never block, but stop writing
                    while rx.recv().is_ok() {}
                    return;
                }
                q.fetch_sub(n, Ordering::SeqCst);
            }
            // everything queued has been written: a clean end of the stream is how the peer learns it is over
            let _ = writer.shutdown(Shutdown::Write);
        });
        Ok(Outbox { tx: Some(tx), queued, broken, stream })
    }

    /// Queue one message. False when the connection is broken or too far behind (the caller should drop it).
    pub fn send(&self, v: &Json) -> bool {
        match v.to_string_compact() {
            Ok(mut line) => {
                line.push('\n');
                self.send_line(line)
            }
            Err(_) => false,
        }
    }

    /// Queue a line that is already JSON and ends with a newline.
    pub fn send_line(&self, line: String) -> bool {
        if self.broken.load(Ordering::SeqCst) || self.queued.load(Ordering::SeqCst) > MAX_QUEUE {
            return false;
        }
        let Some(tx) = &self.tx else { return false };
        self.queued.fetch_add(line.len(), Ordering::SeqCst);
        tx.send(line).is_ok()
    }

    /// How many bytes are waiting to be written.
    pub fn queued_bytes(&self) -> usize {
        self.queued.load(Ordering::SeqCst)
    }

    /// Finish politely: what is queued is still written, then the stream ends.
    pub fn close(&mut self) {
        self.tx = None;
    }

    /// Cut the connection now, whatever is queued (its reader, too, sees the end).
    pub fn kill(&mut self) {
        self.tx = None;
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

pub fn hex(h: u64) -> String {
    format!("{h:016x}")
}

pub fn bye(reason: impl Into<String>) -> Json {
    Json::obj([("bye", Json::obj([("reason", Json::str(reason))]))])
}

/// A goodbye that says on which tick the game ended (the confirmed tick every copy must end on).
pub fn bye_at(reason: impl Into<String>, tick: u64) -> Json {
    Json::obj([("bye", Json::obj([("reason", Json::str(reason)), ("tick", Json::from(tick as i64))]))])
}

pub fn reason_of(msg: &Json) -> String {
    msg.get("bye").and_then(|b| b.get("reason")).and_then(|r| r.as_str()).unwrap_or("?").to_string()
}

/// A tiny pseudo-random generator for the places that need unpredictability but not secrecy (the latency simulator, seat tokens).
/// xorshift64*: a few lines, no dependency, and the same sequence for the same seed, which keeps simulated networks repeatable.
pub struct Xorshift(pub u64);

impl Xorshift {
    pub fn new(seed: u64) -> Xorshift {
        // a zero state would stay zero for ever
        Xorshift(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}
