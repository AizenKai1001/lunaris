//! The live API of a running window (`lunaris play scene --api PORT`): the engine's whole tool set (`get`, `set`, `input`, `query`, `raycast`, `nearby`, `path`, `render`, `step`, ...: `lunaris tools`) as
//! newline-delimited JSON-RPC (the MCP protocol of `lunaris serve`) on a TCP port of this machine (127.0.0.1 only: nobody else on the network can reach it), played while the window runs. An AI, a script or a person
//! with a terminal reads the state of the game as it is (`get uid 100` is the character) and plays it: the keys are `input` (`{"values": {"w": true, "shift": true}}`), a teleport or a change of anything is `set`.
//! The requests are answered between two pictures, on the thread that plays the game, so a request never sees a half-played tick. `lunaris serve --connect HOST:PORT` is the same port as a stdio MCP server, for
//! the clients that only speak that. `tools/agent.py` of the Lunaris game (the project folder) is a client with the helpers of a character: state, keys, look, go to a place.
use lunaris_api::{safe_handle, Engine};
use lunaris_json::{parse, Json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

/// The longest request line (bytes): the same as the stdio server's.
const MAX_LINE: usize = lunaris_api::MAX_REQUEST_BYTES;

struct Request {
    line: String,
    reply: Sender<String>,
}

pub struct LiveApi {
    rx: Receiver<Request>,
    pub addr: SocketAddr,
    /// How many requests have been answered.
    pub served: u64,
}

impl LiveApi {
    /// Listen on `127.0.0.1:port` (0: any free port; `addr` says which).
    pub fn start(port: u16) -> std::io::Result<LiveApi> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let addr = listener.local_addr()?;
        let (tx, rx) = channel::<Request>();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(stream) = conn else { continue };
                let tx = tx.clone();
                std::thread::spawn(move || serve_connection(stream, tx));
            }
        });
        Ok(LiveApi { rx, addr, served: 0 })
    }

    /// Answer the requests that have arrived, for at most `budget` (a picture is not held up by a long queue). Called by the loop that plays the game, between two pictures.
    pub fn pump(&mut self, engine: &mut Engine, budget: Duration) {
        let start = Instant::now();
        while let Ok(req) = self.rx.try_recv() {
            let reply = answer(engine, &req.line);
            self.served += 1;
            let _ = req.reply.send(reply);
            if start.elapsed() >= budget {
                break;
            }
        }
    }
}

/// One request line (a JSON-RPC message or a batch) to the line of its answer (empty for a notification).
pub fn answer(engine: &mut Engine, line: &str) -> String {
    let response = match parse(line) {
        Err(e) => Some(Json::obj([
            ("jsonrpc", Json::str("2.0")),
            ("id", Json::Null),
            ("error", Json::obj([("code", Json::from(-32700i64)), ("message", Json::str(&format!("parse error: {e}")))])),
        ])),
        Ok(Json::Arr(batch)) => {
            let replies: Vec<Json> = batch.iter().filter_map(|m| safe_handle(engine, m)).collect();
            if replies.is_empty() {
                None
            } else {
                Some(Json::Arr(replies))
            }
        }
        Ok(msg) => safe_handle(engine, &msg),
    };
    response.map(|r| r.to_string_compact().unwrap_or_else(|_| r.to_string_lossy())).unwrap_or_default()
}

fn serve_connection(stream: TcpStream, requests: Sender<Request>) {
    let _ = stream.set_nodelay(true);
    let Ok(write_half) = stream.try_clone() else { return };
    let (reply_tx, reply_rx) = channel::<String>();
    std::thread::spawn(move || {
        let mut w = write_half;
        for line in reply_rx {
            if line.is_empty() {
                continue; // (a notification has no answer)
            }
            if w.write_all(line.as_bytes()).is_err() || w.write_all(b"\n").is_err() || w.flush().is_err() {
                break;
            }
        }
    });
    let mut reader = BufReader::new(stream);
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        let n = match reader.by_ref().take(MAX_LINE as u64 + 1).read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if n > MAX_LINE {
            break; // (a line that long is not a request)
        }
        let line = String::from_utf8_lossy(&buf).trim().to_string();
        if line.is_empty() {
            continue;
        }
        if requests.send(Request { line, reply: reply_tx.clone() }).is_err() {
            break;
        }
    }
}

/// `lunaris serve --connect HOST:PORT`: the live game's API as a stdio server (what an MCP client launches): every line read is sent to the game, every answer is written back.
pub fn relay(addr: &str) -> std::io::Result<()> {
    let stream = TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;
    let mut to_game = stream.try_clone()?;
    let mut from_game = BufReader::new(stream);
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let mut line = String::new();
    loop {
        line.clear();
        if stdin.lock().read_line(&mut line)? == 0 {
            return Ok(());
        }
        if line.trim().is_empty() {
            continue;
        }
        to_game.write_all(line.trim().as_bytes())?;
        to_game.write_all(b"\n")?;
        // (a notification has no answer: the game answers a request, a batch of notifications gets nothing back: such a line is not sent by the clients that matter)
        if is_notification(line.trim()) {
            continue;
        }
        let mut answer = String::new();
        if from_game.read_line(&mut answer)? == 0 {
            return Ok(());
        }
        out.write_all(answer.as_bytes())?;
        out.flush()?;
    }
}

fn is_notification(line: &str) -> bool {
    matches!(parse(line), Ok(m) if m.get("method").is_some() && m.get("id").is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A request sent to the live API over TCP is answered by `pump`, which the loop of the game calls between pictures: the answer is the one `serve` gives; a bad line gets an error and the connection goes on.
    #[test]
    fn the_live_api_answers_json_rpc_over_tcp_between_pictures() {
        let mut engine = Engine::new(1, std::env::temp_dir());
        let mut api = LiveApi::start(0).unwrap();
        let mut client = TcpStream::connect(api.addr).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut reader = BufReader::new(client.try_clone().unwrap());
        let mut ask = |text: &str, api: &mut LiveApi, engine: &mut Engine| -> String {
            client.write_all(text.as_bytes()).unwrap();
            client.write_all(b"\n").unwrap();
            // (the game's loop: wait for the request to arrive, answer it)
            let mut waited = 0;
            while api.served == 0 && waited < 400 {
                api.pump(engine, Duration::from_millis(5));
                std::thread::sleep(Duration::from_millis(5));
                waited += 1;
            }
            api.served = 0;
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            line
        };
        let spawn = ask(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"spawn","arguments":{"uid":7,"components":{"Transform":{"pos":[1,2,3]}}}}}"#, &mut api, &mut engine);
        assert!(spawn.contains("\"id\":1") && !spawn.contains("\"error\""), "{spawn}");
        let get = ask(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get","arguments":{"uid":7}}}"#, &mut api, &mut engine);
        assert!(get.contains("\"id\":2") && get.contains("pos"), "{get}");
        let bad = ask("this is not json", &mut api, &mut engine);
        assert!(bad.contains("-32700"), "{bad}");
        let again = ask(r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#, &mut api, &mut engine);
        assert!(again.contains("\"id\":3"), "{again}");
    }
}
