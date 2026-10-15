//! JSON-RPC 2.0 over newline-delimited JSON (the MCP stdio transport). Speaks the Model Context Protocol
//! (`initialize`, `tools/list`, `tools/call`, `ping`) and also plain `lunaris.<tool>` methods.
use crate::engine::{Engine, VERSION};
use crate::tools::tool_defs;
use lunaris_json::{parse, Json};
use std::io::{BufRead, Write};

const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

fn rpc_error(id: Json, code: i64, message: &str, data: Option<Json>) -> Json {
    let mut err = Json::obj([("code", Json::from(code)), ("message", Json::str(message))]);
    if let Some(d) = data {
        err.set("data", d);
    }
    Json::obj([("jsonrpc", Json::str("2.0")), ("id", id), ("error", err)])
}

fn rpc_result(id: Json, result: Json) -> Json {
    Json::obj([("jsonrpc", Json::str("2.0")), ("id", id), ("result", result)])
}

fn text_content(text: String, is_error: bool) -> Json {
    Json::obj([("content", Json::arr([Json::obj([("type", Json::str("text")), ("text", Json::Str(text))])])), ("isError", Json::Bool(is_error))])
}

fn render(v: &Json) -> String {
    // pretty for small results (easy to read), compact for big ones (cheaper to send)
    let compact = v.to_string_compact().unwrap_or_else(|_| v.to_string_lossy());
    if compact.len() <= 4000 {
        v.to_string_pretty().unwrap_or(compact)
    } else {
        compact
    }
}

/// Handle one JSON-RPC message. Returns the response to send, or `None` for notifications.
pub fn handle_message(engine: &mut Engine, msg: &Json) -> Option<Json> {
    let id = msg.get("id").cloned();
    let method = match msg.get("method").and_then(|m| m.as_str()) {
        Some(m) => m,
        None => {
            // a response or garbage: nothing to answer
            return id.map(|i| rpc_error(i, -32600, "invalid request: missing `method`", None));
        }
    };
    let params = msg.get("params").cloned().unwrap_or(Json::Null);
    let is_notification = id.is_none();
    let reply_id = id.unwrap_or(Json::Null);

    let outcome: Result<Json, Json> = match method {
        "initialize" => {
            let want = params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or("");
            let version = if PROTOCOL_VERSIONS.contains(&want) { want } else { PROTOCOL_VERSIONS[0] };
            Ok(Json::obj([
                ("protocolVersion", Json::str(version)),
                ("capabilities", Json::obj([("tools", Json::obj([("listChanged", Json::Bool(false))]))])),
                ("serverInfo", Json::obj([("name", Json::str("lunaris")), ("version", Json::str(VERSION))])),
                ("instructions", Json::str("Lunaris is a deterministic game engine you operate through tools. Call `guide` for how to build things, `describe` to see what is here, then `schema`. Entities are addressed by uid. Build, then `check`, then `render` and look (pass `quality` to choose the GPU renderer's quality, or `reference` for the exact CPU picture; add an `Environment` entity for sky, shadows and glow, and give meshes `metallic`/`roughness`/`texture`). `files` and `model_info` show the models, textures and animation clips you can use. Use `snapshot`/`restore` to experiment safely and `step` then compare `hash` to check determinism.")),
            ]))
        }
        "notifications/initialized" | "notifications/cancelled" | "initialized" => return None,
        "ping" => Ok(Json::Obj(Vec::new())),
        "tools/list" => Ok(Json::obj([(
            "tools",
            Json::arr(tool_defs().into_iter().map(|t| Json::obj([("name", Json::str(t.name)), ("description", Json::str(t.description)), ("inputSchema", t.input_schema)]))),
        )])),
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str());
            match name {
                None => Err(rpc_error(reply_id.clone(), -32602, "tools/call needs a string `name`", None)),
                Some(name) => {
                    let args = params.get("arguments").cloned().unwrap_or(Json::Null);
                    Ok(match engine.call(name, &args) {
                        Ok(v) => match v.get("image") {
                            // images go out as MCP image content so the model sees the picture, not a wall of base64
                            Some(img) if img.get("data").is_some() => {
                                let mut rest = v.clone();
                                if let Json::Obj(p) = &mut rest {
                                    p.retain(|(k, _)| k != "image");
                                }
                                let image = Json::obj([
                                    ("type", Json::str("image")),
                                    ("data", img.get("data").cloned().unwrap_or(Json::Null)),
                                    ("mimeType", img.get("mime").cloned().unwrap_or_else(|| Json::str("image/png"))),
                                ]);
                                let text = Json::obj([("type", Json::str("text")), ("text", Json::Str(render(&rest)))]);
                                Json::obj([("content", Json::arr([image, text])), ("isError", Json::Bool(false))])
                            }
                            _ => text_content(render(&v), false),
                        },
                        Err(e) => text_content(render(&e.to_json()), true),
                    })
                }
            }
        }
        m if m.starts_with("lunaris.") => {
            let tool = &m["lunaris.".len()..];
            match engine.call(tool, &params) {
                Ok(v) => Ok(v),
                Err(e) => Err(rpc_error(reply_id.clone(), -32000, &e.message, Some(e.to_json()))),
            }
        }
        other => Err(rpc_error(reply_id.clone(), -32601, &format!("method not found: {other}"), Some(Json::obj([("hint", Json::str("MCP methods: initialize, tools/list, tools/call, ping. Direct calls use lunaris.<tool>."))])))),
    };
    if is_notification {
        return None;
    }
    Some(match outcome {
        Ok(r) => rpc_result(reply_id, r),
        Err(e) => e,
    })
}

/// `handle_message`, but a panic inside the engine becomes an error response instead of killing the server.
pub fn safe_handle(engine: &mut Engine, msg: &Json) -> Option<Json> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle_message(engine, msg))) {
        Ok(r) => r,
        Err(_) => msg.get("id").cloned().map(|id| {
            rpc_error(
                id,
                -32603,
                "internal error: the engine panicked while handling this request",
                Some(Json::obj([("hint", Json::str("the world may be in an inconsistent state: call `restore` with a snapshot, or `reset`"))])),
            )
        }),
    }
}

/// The longest request line `serve` accepts (a scene or a recording inline can be big, but not unbounded).
pub const MAX_REQUEST_BYTES: usize = 64 << 20;

/// Read one line of at most `limit` bytes into `buf`. `Ok(None)` at the end of the input; `Ok(Some(false))` for a line that was too
/// long (it is read to its end and thrown away, so the next request starts cleanly).
fn read_limited_line(input: &mut impl BufRead, buf: &mut Vec<u8>, limit: usize) -> std::io::Result<Option<bool>> {
    buf.clear();
    let mut too_long = false;
    let mut any = false;
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return Ok(if any { Some(!too_long) } else { None });
        }
        any = true;
        let (take, found_newline) = match chunk.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        if !too_long {
            if buf.len() + take > limit + 1 {
                too_long = true;
                buf.clear();
            } else {
                buf.extend_from_slice(&chunk[..take]);
            }
        }
        input.consume(take);
        if found_newline {
            return Ok(Some(!too_long));
        }
    }
}

/// Read newline-delimited JSON-RPC from `input` and write responses to `output` until the input ends.
pub fn serve(engine: &mut Engine, input: impl BufRead, output: impl Write) -> std::io::Result<()> {
    serve_with_limit(engine, input, output, MAX_REQUEST_BYTES)
}

/// `serve` with a custom request size limit (in bytes).
pub fn serve_with_limit(engine: &mut Engine, mut input: impl BufRead, mut output: impl Write, limit: usize) -> std::io::Result<()> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let fits = match read_limited_line(&mut input, &mut buf, limit)? {
            None => break,
            Some(fits) => fits,
        };
        if !fits {
            let r = rpc_error(Json::Null, -32600, &format!("request too large: a request line may be at most {limit} bytes"), Some(Json::obj([("hint", Json::str("send big data as a file in the workspace and refer to it by path"))])));
            output.write_all(r.to_string_compact().unwrap_or_default().as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
            continue;
        }
        let text = String::from_utf8_lossy(&buf).to_string();
        let line = text.trim();
        if line.is_empty() {
            continue;
        }
        let response = match parse(line) {
            Err(e) => Some(rpc_error(Json::Null, -32700, &format!("parse error: {e}"), Some(Json::obj([("line", Json::from(e.line)), ("column", Json::from(e.column))])))),
            Ok(Json::Arr(batch)) => {
                let replies: Vec<Json> = batch.iter().filter_map(|m| safe_handle(engine, m)).collect();
                if replies.is_empty() { None } else { Some(Json::Arr(replies)) }
            }
            Ok(msg) => safe_handle(engine, &msg),
        };
        if let Some(r) = response {
            let text = r.to_string_compact().unwrap_or_else(|_| r.to_string_lossy());
            output.write_all(text.as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}
