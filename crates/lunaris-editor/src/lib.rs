//! The Lunaris editor is a web page. This crate is the small local server behind it (standard library only): it serves one
//! HTML file and forwards `POST /rpc` to the engine's tool API, exactly like an AI agent or the CLI would call it. Nothing in
//! the page can do what a tool call cannot.
//!
//! Safety, because it listens on a port:
//! - it binds `127.0.0.1` only;
//! - requests must carry the right `Host` (blocks DNS rebinding) and, if present, a matching `Origin`;
//! - `/rpc` needs a secret token that is embedded in the served page (a page on another site cannot read it: no CORS
//!   headers are ever sent) and a JSON content type (forces a preflight that is never answered);
//! - bodies and headers are size limited and connections time out.
use lunaris_api::{safe_handle, Engine};
use lunaris_json::parse;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const INDEX: &str = include_str!("index.html");
const TOKEN_MARK: &str = "__LUNARIS_TOKEN__";
const SCENE_MARK: &str = "__LUNARIS_SCENE__";
const MAX_HEADER: usize = 16 * 1024;
const MAX_BODY: usize = 8 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Server {
    listener: TcpListener,
    engine: Arc<Mutex<Engine>>,
    token: String,
    scene: String,
}

fn random_token() -> String {
    // `RandomState` is seeded by the operating system; mixing in the clock and process id costs nothing
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut out = String::new();
    for salt in 0..2u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(nanos);
        h.write_u32(std::process::id());
        h.write_u64(salt);
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

enum Fail {
    Status(u16, &'static str),
    Io,
}

fn read_request(stream: &mut TcpStream) -> Result<Request, Fail> {
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            if i > MAX_HEADER {
                return Err(Fail::Status(431, "request headers too large"));
            }
            break i;
        }
        if buf.len() > MAX_HEADER {
            return Err(Fail::Status(431, "request headers too large"));
        }
        match stream.read(&mut tmp) {
            Ok(0) => return Err(Fail::Io),
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => return Err(Fail::Io),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split(' ');
    let (method, target) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
    if method.is_empty() || target.is_empty() {
        return Err(Fail::Status(400, "bad request line"));
    }
    let path = target.split('?').next().unwrap_or("").to_string();
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))).collect();
    let length: usize = match headers.iter().find(|(k, _)| k == "content-length") {
        Some((_, v)) => v.parse().map_err(|_| Fail::Status(400, "bad content-length"))?,
        None => 0,
    };
    if length > MAX_BODY {
        return Err(Fail::Status(413, "body too large"));
    }
    if headers.iter().any(|(k, v)| k == "transfer-encoding" && !v.eq_ignore_ascii_case("identity")) {
        return Err(Fail::Status(501, "chunked request bodies are not supported"));
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        match stream.read(&mut tmp) {
            Ok(0) => return Err(Fail::Io),
            Ok(n) => body.extend_from_slice(&tmp[..n]),
            Err(_) => return Err(Fail::Io),
        }
    }
    body.truncate(length);
    Ok(Request { method, path, headers, body })
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn plain(stream: &mut TcpStream, status: u16, message: &str) {
    respond(stream, status, "text/plain; charset=utf-8", message.as_bytes());
}

/// Text that is safe to place inside a double-quoted HTML attribute.
fn attr_safe(text: &str) -> String {
    text.chars().map(|c| match c {
        '&' => "&amp;".to_string(),
        '<' => "&lt;".to_string(),
        '>' => "&gt;".to_string(),
        '"' => "&quot;".to_string(),
        c if c.is_control() => String::new(),
        c => c.to_string(),
    }).collect()
}

fn handle(mut stream: TcpStream, engine: &Mutex<Engine>, token: &str, scene: &str, port: u16) {
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok();
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok();
    let req = match read_request(&mut stream) {
        Ok(r) => r,
        Err(Fail::Status(code, msg)) => return plain(&mut stream, code, msg),
        Err(Fail::Io) => return,
    };
    let allowed_hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !req.header("host").is_some_and(|h| allowed_hosts.iter().any(|a| a == h)) {
        return plain(&mut stream, 403, "wrong Host header: open the editor at http://127.0.0.1:<port>/");
    }
    if let Some(origin) = req.header("origin") {
        if !allowed_hosts.iter().any(|a| origin == format!("http://{a}")) {
            return plain(&mut stream, 403, "cross-origin requests are refused");
        }
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => {
            // LUNARIS_EDITOR_PAGE=path/to/index.html serves the page from disk (for working on the page without rebuilding)
            let page = std::env::var_os("LUNARIS_EDITOR_PAGE").and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_else(|| INDEX.to_string());
            respond(&mut stream, 200, "text/html; charset=utf-8", page.replace(TOKEN_MARK, token).replace(SCENE_MARK, &attr_safe(scene)).as_bytes())
        }
        ("GET", "/favicon.ico") => respond(&mut stream, 204, "image/x-icon", b""),
        ("POST", "/rpc") => {
            if req.header("x-lunaris-token") != Some(token) {
                return plain(&mut stream, 403, "missing or wrong X-Lunaris-Token");
            }
            if !req.header("content-type").is_some_and(|c| c.starts_with("application/json")) {
                return plain(&mut stream, 415, "send Content-Type: application/json");
            }
            let text = match std::str::from_utf8(&req.body) {
                Ok(t) => t,
                Err(_) => return plain(&mut stream, 400, "the body is not UTF-8"),
            };
            let reply = match parse(text) {
                Err(e) => {
                    let msg = format!("parse error: {e}");
                    lunaris_json::Json::obj([
                        ("jsonrpc", lunaris_json::Json::str("2.0")),
                        ("id", lunaris_json::Json::Null),
                        ("error", lunaris_json::Json::obj([("code", lunaris_json::Json::from(-32700)), ("message", lunaris_json::Json::Str(msg))])),
                    ])
                    .to_string_compact()
                    .unwrap_or_default()
                }
                Ok(msg) => {
                    let mut guard = engine.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    let rev_before = guard.revision();
                    match safe_handle(&mut guard, &msg) {
                        Some(mut r) => {
                            // the revision before and after this call, so a client can tell that someone else changed the world
                            // between its previous call and this one (the editor page refreshes itself when that happens)
                            if matches!(r, lunaris_json::Json::Obj(_)) {
                                r.set("rev_before", lunaris_json::Json::from(rev_before as i64));
                                r.set("rev", lunaris_json::Json::from(guard.revision() as i64));
                            }
                            r.to_string_compact().unwrap_or_else(|_| r.to_string_lossy())
                        }
                        None => String::new(),
                    }
                }
            };
            if reply.is_empty() {
                respond(&mut stream, 204, "application/json", b"");
            } else {
                respond(&mut stream, 200, "application/json", reply.as_bytes());
            }
        }
        (_, "/") | (_, "/index.html") | (_, "/rpc") => plain(&mut stream, 405, "method not allowed"),
        _ => plain(&mut stream, 404, "not found"),
    }
}

impl Server {
    /// Listen on `127.0.0.1:port` (0 picks a free port).
    pub fn bind(engine: Engine, port: u16) -> std::io::Result<Server> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        Ok(Server { listener, engine: Arc::new(Mutex::new(engine)), token: random_token(), scene: "scene.lunaris.json".to_string() })
    }

    /// The scene path shown in the page's file box (the page still reads and writes through the tools).
    pub fn with_scene(mut self, path: &str) -> Server {
        self.scene = path.to_string();
        self
    }

    pub fn addr(&self) -> SocketAddr {
        self.listener.local_addr().expect("a bound listener has an address")
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.addr().port())
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Accept connections until the process ends; each connection is handled on its own thread, tool calls are serialised
    /// by the engine lock.
    pub fn serve(self) -> std::io::Result<()> {
        let port = self.addr().port();
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let engine = Arc::clone(&self.engine);
                    let token = self.token.clone();
                    let scene = self.scene.clone();
                    std::thread::spawn(move || handle(stream, &engine, &token, &scene, port));
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunaris_json::Json;

    fn start() -> (u16, String) {
        let dir = std::env::temp_dir();
        let server = Server::bind(Engine::new(7, dir), 0).unwrap();
        let (port, token) = (server.addr().port(), server.token().to_string());
        std::thread::spawn(move || {
            let _ = server.serve();
        });
        (port, token)
    }

    /// Send raw bytes, return (status, headers+body text).
    fn raw(port: u16, request: &str) -> (u16, String) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        s.write_all(request.as_bytes()).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        let text = String::from_utf8_lossy(&out).to_string();
        let status = text.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        (status, text)
    }

    fn post(port: u16, host: &str, token: Option<&str>, content_type: &str, body: &str) -> (u16, String) {
        let tok = token.map(|t| format!("X-Lunaris-Token: {t}\r\n")).unwrap_or_default();
        raw(port, &format!("POST /rpc HTTP/1.1\r\nHost: {host}\r\n{tok}Content-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()))
    }

    fn body_of(text: &str) -> &str {
        &text[text.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0)..]
    }

    #[test]
    fn the_page_is_served_with_the_token_and_no_cors_headers() {
        let (port, token) = start();
        let (status, text) = raw(port, &format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"));
        assert_eq!(status, 200);
        assert!(body_of(&text).contains(&token), "the page must carry this run's token");
        assert!(!body_of(&text).contains(TOKEN_MARK));
        assert!(!text.to_ascii_lowercase().contains("access-control-allow"), "never send CORS headers");
        assert!(text.contains("frame-ancestors 'none'"));
    }

    #[test]
    fn the_scene_name_shown_in_the_page_cannot_break_out_of_its_attribute() {
        let server = Server::bind(Engine::new(7, std::env::temp_dir()), 0).unwrap().with_scene("a\"><script>alert(1)</script>.json");
        let port = server.addr().port();
        std::thread::spawn(move || {
            let _ = server.serve();
        });
        let (_, text) = raw(port, &format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"));
        let page = body_of(&text);
        assert!(!page.contains("<script>alert(1)"), "the scene name must be escaped");
        assert!(page.contains("a&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;.json"));
        assert!(!page.contains(SCENE_MARK));
    }

    #[test]
    fn rpc_runs_tools_for_a_client_with_the_token() {
        let (port, token) = start();
        let host = format!("127.0.0.1:{port}");
        let (status, text) = post(port, &host, Some(&token), "application/json", r#"{"jsonrpc":"2.0","id":5,"method":"lunaris.describe","params":{}}"#);
        assert_eq!(status, 200, "{text}");
        let j = parse(body_of(&text)).unwrap();
        assert_eq!(j.get("id"), Some(&Json::from(5)));
        assert_eq!(j.get("result").and_then(|r| r.get("engine")).and_then(|e| e.as_str()), Some("lunaris"));
        // the same engine serves every request: state carries over
        let (_, t) = post(port, &host, Some(&token), "application/json", r#"{"jsonrpc":"2.0","id":6,"method":"lunaris.spawn","params":{"components":{"Name":{"value":"Edited"}}}}"#);
        assert!(body_of(&t).contains("\"ok\":true"), "{t}");
        let (_, t) = post(port, &host, Some(&token), "application/json", r#"{"jsonrpc":"2.0","id":7,"method":"lunaris.query","params":{"select":["Name"]}}"#);
        assert!(body_of(&t).contains("Edited"), "{t}");
        // tool errors come back as JSON-RPC errors with the hint
        let (_, t) = post(port, &host, Some(&token), "application/json", r#"{"jsonrpc":"2.0","id":8,"method":"lunaris.get","params":{"uid":99999}}"#);
        assert!(body_of(&t).contains("\"error\""), "{t}");
    }

    #[test]
    fn each_reply_says_which_revisions_the_call_used_so_clients_can_spot_other_clients_edits() {
        let (port, token) = start();
        let host = format!("127.0.0.1:{port}");
        let rpc = |id: u32, method: &str, params: &str| {
            let (_, t) = post(port, &host, Some(&token), "application/json", &format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"lunaris.{method}","params":{params}}}"#));
            let j = parse(body_of(&t)).unwrap();
            let n = |k: &str| j.get(k).and_then(|v| v.as_u64()).unwrap_or_else(|| panic!("`{k}` missing in {t}"));
            (n("rev_before"), n("rev"))
        };
        let (b0, a0) = rpc(1, "revision", "{}");
        assert_eq!(b0, a0, "a read does not move the revision");
        let (b1, a1) = rpc(2, "spawn", r#"{"components":{"Transform":{}}}"#);
        assert_eq!(b1, a0, "the next call starts where the last one ended");
        assert!(a1 > b1, "an edit moves it");
        let (b2, a2) = rpc(3, "query", "{}");
        assert_eq!((b2, a2), (a1, a1));
    }

    #[test]
    fn requests_without_the_token_or_from_other_sites_are_refused() {
        let (port, token) = start();
        let host = format!("127.0.0.1:{port}");
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"lunaris.describe"}"#;
        assert_eq!(post(port, &host, None, "application/json", body).0, 403);
        assert_eq!(post(port, &host, Some("wrong"), "application/json", body).0, 403);
        assert_eq!(post(port, &host, Some(&token), "text/plain", body).0, 415);
        // DNS rebinding: the browser sends the attacker's host name
        assert_eq!(post(port, &format!("evil.example:{port}"), Some(&token), "application/json", body).0, 403);
        // a page on another origin
        let (status, _) = raw(port, &format!("POST /rpc HTTP/1.1\r\nHost: {host}\r\nOrigin: https://evil.example\r\nX-Lunaris-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()));
        assert_eq!(status, 403);
        assert_eq!(raw(port, &format!("GET / HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n")).0, 403);
        // preflight is never answered
        assert_eq!(raw(port, &format!("OPTIONS /rpc HTTP/1.1\r\nHost: {host}\r\nOrigin: https://evil.example\r\nConnection: close\r\n\r\n")).0, 403);
    }

    #[test]
    fn malformed_and_oversized_requests_get_clear_errors() {
        let (port, token) = start();
        let host = format!("127.0.0.1:{port}");
        assert_eq!(raw(port, &format!("GET /nothing HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")).0, 404);
        assert_eq!(raw(port, &format!("PUT / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")).0, 405);
        assert_eq!(raw(port, &format!("POST /rpc HTTP/1.1\r\nHost: {host}\r\nX-Lunaris-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: 99999999\r\nConnection: close\r\n\r\n")).0, 413);
        let (status, text) = post(port, &host, Some(&token), "application/json", "{not json");
        assert_eq!(status, 200);
        assert!(body_of(&text).contains("-32700"), "{text}");
        let big_header = format!("GET / HTTP/1.1\r\nHost: {host}\r\nX-Pad: {}\r\n\r\n", "a".repeat(20_000));
        assert_eq!(raw(port, &big_header).0, 431);
    }
}
