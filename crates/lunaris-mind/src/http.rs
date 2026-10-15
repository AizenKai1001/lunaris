//! A tiny HTTP/1.1 client: one blocking POST of a JSON body over plain TCP. Enough to talk to a local or LAN model server
//! (OpenAI-compatible). `https://` is deliberately not supported (no TLS dependency); use a trusted network or a tunnel.
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

fn split_url(url: &str) -> Result<(String, u16, String), String> {
    if url.starts_with("https://") {
        return Err("https:// is not supported (no TLS in Lunaris); use an http:// address on a trusted network".into());
    }
    let rest = url.strip_prefix("http://").ok_or_else(|| format!("`{url}` must start with http://"))?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().map_err(|_| format!("bad port in `{url}`"))?),
        None => (hostport.to_string(), 80),
    };
    if host.is_empty() {
        return Err(format!("no host in `{url}`"));
    }
    Ok((host, port, path))
}

fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = data.windows(2).position(|w| w == b"\r\n").ok_or("truncated chunked body")?;
        let size_text = std::str::from_utf8(&data[..line_end]).map_err(|_| "bad chunk size")?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16).map_err(|_| "bad chunk size")?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size + 2 {
            return Err("truncated chunk".into());
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

pub fn post_json(url: &str, body: &str, headers: &[(String, String)], timeout: Duration) -> Result<Response, String> {
    request("POST", url, Some(body), headers, timeout)
}

/// One blocking GET (for example `/v1/models`).
pub fn get(url: &str, headers: &[(String, String)], timeout: Duration) -> Result<Response, String> {
    request("GET", url, None, headers, timeout)
}

fn request(method: &str, url: &str, body: Option<&str>, headers: &[(String, String)], timeout: Duration) -> Result<Response, String> {
    let (host, port, path) = split_url(url)?;
    let body_text = body.unwrap_or("");
    let addr = (host.as_str(), port).to_socket_addrs().map_err(|e| format!("cannot resolve {host}: {e}"))?.next().ok_or_else(|| format!("no address for {host}"))?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("cannot connect to {host}:{port}: {e}"))?;
    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_write_timeout(Some(timeout)).ok();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n", body_text.len());
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).and_then(|_| stream.write_all(body_text.as_bytes())).map_err(|e| format!("send failed: {e}"))?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| format!("read failed (timeout {timeout:?}?): {e}"))?;
    let head_end = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("malformed response: no header end")?;
    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
    let mut lines = head.lines();
    let status_line = lines.next().ok_or("empty response")?;
    let status: u16 = status_line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| format!("bad status line `{status_line}`"))?;
    let mut chunked = false;
    let mut length: Option<usize> = None;
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            if k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked") {
                chunked = true;
            }
            if k == "content-length" {
                length = v.parse().ok();
            }
        }
    }
    let payload = &raw[head_end + 4..];
    let bytes = if chunked {
        decode_chunked(payload)?
    } else if let Some(n) = length {
        payload[..n.min(payload.len())].to_vec()
    } else {
        payload.to_vec()
    };
    Ok(Response { status, body: String::from_utf8_lossy(&bytes).to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// Serve exactly one request with a canned raw response; returns the address and a handle yielding the request text.
    fn serve_once(response: Vec<u8>) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}/v1/chat/completions", listener.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let n = s.read(&mut tmp).unwrap();
                buf.extend_from_slice(&tmp[..n]);
                if let Some(he) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..he]).to_lowercase();
                    let len: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).map(|v| v.trim().parse().unwrap()).unwrap_or(0);
                    if buf.len() >= he + 4 + len {
                        break;
                    }
                }
            }
            s.write_all(&response).unwrap();
            String::from_utf8_lossy(&buf).to_string()
        });
        (addr, h)
    }

    #[test]
    fn plain_content_length_response() {
        let body = r#"{"ok":true}"#;
        let (url, h) = serve_once(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes());
        let r = post_json(&url, r#"{"q":1}"#, &[("Authorization".into(), "Bearer test".into())], Duration::from_secs(5)).unwrap();
        assert_eq!((r.status, r.body.as_str()), (200, body));
        let req = h.join().unwrap();
        assert!(req.starts_with("POST /v1/chat/completions HTTP/1.1"), "{req}");
        assert!(req.contains("Authorization: Bearer test") && req.ends_with(r#"{"q":1}"#));
    }

    #[test]
    fn chunked_response_and_error_status() {
        let (url, _h) = serve_once(b"HTTP/1.1 500 Internal Server Error\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n".to_vec());
        let r = post_json(&url, "{}", &[], Duration::from_secs(5)).unwrap();
        assert_eq!((r.status, r.body.as_str()), (500, "hello world"));
    }

    #[test]
    fn url_problems_are_explained() {
        assert!(post_json("https://example.com/x", "{}", &[], Duration::from_secs(1)).unwrap_err().contains("not supported"));
        assert!(post_json("ftp://x", "{}", &[], Duration::from_secs(1)).unwrap_err().contains("http://"));
        // nothing listens on port 1
        assert!(post_json("http://127.0.0.1:1/x", "{}", &[], Duration::from_millis(500)).unwrap_err().contains("cannot connect"));
    }
}
