//! Language-model back ends. A `Model` turns a conversation into one reply. `OpenAiModel` speaks the OpenAI chat-completions
//! protocol over plain HTTP (works with ForgeInfer's server, llama.cpp, LM Studio and others); `ScriptedModel` plays back fixed
//! replies for tests and offline demos.
use crate::http::post_json;
use lunaris_json::{parse, Json};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(c: impl Into<String>) -> Message {
        Message { role: "system".into(), content: c.into() }
    }
    pub fn user(c: impl Into<String>) -> Message {
        Message { role: "user".into(), content: c.into() }
    }
    pub fn assistant(c: impl Into<String>) -> Message {
        Message { role: "assistant".into(), content: c.into() }
    }
}

pub trait Model {
    fn complete(&mut self, messages: &[Message]) -> Result<String, String>;
    fn name(&self) -> String;
    /// What the server said about the last call (token counts, how much of the prompt was served from its cache, timings), when it
    /// says anything. Kept in the transcript so that a slow or odd session can be debugged.
    fn last_stats(&self) -> Option<Json> {
        None
    }
    /// The model's context window in tokens, when the server says (asked once; `None` = unknown).
    fn context_tokens(&mut self) -> Option<usize> {
        None
    }
}

pub struct OpenAiModel {
    /// Base URL including the version prefix, e.g. `http://127.0.0.1:8080/v1`.
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub temperature: f64,
    pub max_tokens: u64,
    pub timeout: Duration,
    /// A JSON schema the reply must follow (`response_format` of type `json_schema`: servers that support it, like ForgeInfer, llama.cpp
    /// and LM Studio, cannot produce anything else, so the reply always parses). `None` leaves the reply free.
    pub schema: Option<Json>,
    /// A fixed sampling seed (with temperature 0 the same prompt gives the same reply on the same server build).
    pub seed: Option<u64>,
    /// Ask the chat template not to think first (`chat_template_kwargs.enable_thinking = false`; servers that do not know it ignore it).
    pub no_thinking: bool,
    /// How often a "server busy" answer (HTTP 503) is retried, and the wait before the first retry (it doubles).
    pub retries: u32,
    pub retry_wait: Duration,
    stats: Option<Json>,
    /// What `/v1/models` said about the context window (outer `None`: not asked yet).
    ctx: Option<Option<usize>>,
}

impl OpenAiModel {
    pub fn new(base_url: &str, model: &str) -> OpenAiModel {
        OpenAiModel {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key: None,
            temperature: 0.3,
            max_tokens: 300,
            timeout: Duration::from_secs(120),
            schema: None,
            seed: None,
            no_thinking: false,
            retries: 4,
            retry_wait: Duration::from_millis(1500),
            stats: None,
            ctx: None,
        }
    }

    fn body(&self, messages: &[Message]) -> Result<String, String> {
        let mut fields = vec![
            ("model", Json::str(self.model.clone())),
            ("messages", Json::arr(messages.iter().map(|m| Json::obj([("role", Json::str(m.role.clone())), ("content", Json::str(m.content.clone()))])))),
            ("temperature", Json::from(self.temperature)),
            ("max_tokens", Json::from(self.max_tokens as i64)),
            ("stream", Json::Bool(false)),
        ];
        if let Some(seed) = self.seed {
            fields.push(("seed", Json::from(seed as i64)));
        }
        if let Some(schema) = &self.schema {
            fields.push(("response_format", Json::obj([("type", Json::str("json_schema")), ("json_schema", Json::obj([("name", Json::str("action")), ("strict", Json::Bool(true)), ("schema", schema.clone())]))])));
        }
        if self.no_thinking {
            fields.push(("chat_template_kwargs", Json::obj([("enable_thinking", Json::Bool(false))])));
        }
        Json::obj(fields).to_string_compact().map_err(|e| e.to_string())
    }
}

impl Model for OpenAiModel {
    fn name(&self) -> String {
        format!("{} at {}", self.model, self.base_url)
    }

    fn last_stats(&self) -> Option<Json> {
        self.stats.clone()
    }

    /// Ask `GET {base}/models` once: ForgeInfer reports `meta.n_ctx`, vLLM `max_model_len`, others `context_length`.
    fn context_tokens(&mut self) -> Option<usize> {
        if self.ctx.is_none() {
            let mut headers = Vec::new();
            if let Some(k) = &self.api_key {
                headers.push(("Authorization".to_string(), format!("Bearer {k}")));
            }
            let found = crate::http::get(&format!("{}/models", self.base_url), &headers, Duration::from_secs(10))
                .ok()
                .filter(|r| r.status == 200)
                .and_then(|r| parse(&r.body).ok())
                .and_then(|j| {
                    let entries = j.get("data")?.as_array()?.to_vec();
                    let entry = entries.iter().find(|e| e.get("id").and_then(|i| i.as_str()) == Some(self.model.as_str())).or_else(|| entries.first())?.clone();
                    let n = entry.get("meta").and_then(|m| m.get("n_ctx")).or_else(|| entry.get("max_model_len")).or_else(|| entry.get("context_length")).and_then(|v| v.as_u64())?;
                    Some(n as usize)
                });
            self.ctx = Some(found);
        }
        self.ctx.flatten()
    }

    fn complete(&mut self, messages: &[Message]) -> Result<String, String> {
        let body = self.body(messages)?;
        let mut headers = Vec::new();
        if let Some(k) = &self.api_key {
            headers.push(("Authorization".to_string(), format!("Bearer {k}")));
        }
        let url = format!("{}/chat/completions", self.base_url);
        let started = Instant::now();
        let mut wait = self.retry_wait;
        let mut attempt = 0;
        let resp = loop {
            let resp = match post_json(&url, &body, &headers, self.timeout) {
                Ok(r) => r,
                // a retry that cannot even connect: report what the server said before
                Err(e) if attempt == 0 => return Err(e),
                Err(e) => return Err(format!("the model server stayed busy and then went away ({e})")),
            };
            if resp.status == 503 && attempt < self.retries {
                // the server runs one request at a time and its queue is full: wait a moment and ask again
                std::thread::sleep(wait);
                wait *= 2;
                attempt += 1;
                continue;
            }
            break resp;
        };
        if resp.status != 200 {
            let text: String = resp.body.chars().take(300).collect();
            let hint = if resp.status == 400 && (resp.body.contains("n_ctx") || resp.body.contains("context")) { " (the conversation does not fit the model's context window: shorten the history or start the server with a larger --ctx)" } else { "" };
            return Err(format!("the model server answered HTTP {}: {text}{hint}", resp.status));
        }
        let j = parse(&resp.body).map_err(|e| format!("the model server sent invalid JSON: {e}"))?;
        let mut stats = Json::obj([("elapsed_ms", Json::from(started.elapsed().as_millis() as i64)), ("retries", Json::from(attempt as i64))]);
        if let Some(u) = j.get("usage") {
            stats.set("usage", u.clone());
        }
        if let Some(t) = j.get("timings") {
            stats.set("timings", t.clone());
        }
        self.stats = Some(stats);
        j.get("choices")
            .and_then(|c| c.index(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(str::to_string)
            .ok_or_else(|| format!("no choices[0].message.content in the reply: {}", resp.body.chars().take(300).collect::<String>()))
    }
}

/// Plays back fixed replies in order (and repeats the last one), remembering what it was asked.
pub struct ScriptedModel {
    pub replies: Vec<String>,
    pub next: usize,
    pub seen: Vec<Vec<Message>>,
}

impl ScriptedModel {
    pub fn new(replies: Vec<String>) -> ScriptedModel {
        ScriptedModel { replies, next: 0, seen: Vec::new() }
    }
}

impl Model for ScriptedModel {
    fn name(&self) -> String {
        format!("scripted ({} replies)", self.replies.len())
    }

    fn complete(&mut self, messages: &[Message]) -> Result<String, String> {
        self.seen.push(messages.to_vec());
        if self.replies.is_empty() {
            return Err("the scripted model has no replies".into());
        }
        let r = self.replies[self.next.min(self.replies.len() - 1)].clone();
        self.next += 1;
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn openai_model_round_trip_against_a_local_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 16384];
            let mut n = 0;
            loop {
                n += s.read(&mut buf[n..]).unwrap();
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                if let Some(he) = text.find("\r\n\r\n") {
                    let len: usize = text[..he].to_lowercase().lines().find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                    if n >= he + 4 + len {
                        break;
                    }
                }
            }
            let reply = r#"{"choices":[{"message":{"role":"assistant","content":"{\"actions\":[]}"}}]}"#;
            s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{reply}", reply.len()).as_bytes()).unwrap();
            String::from_utf8_lossy(&buf[..n]).to_string()
        });
        let mut m = OpenAiModel::new(&base, "test-model");
        m.api_key = Some("k".into());
        let out = m.complete(&[Message::system("be brief"), Message::user("hi")]).unwrap();
        assert_eq!(out, r#"{"actions":[]}"#);
        let req = h.join().unwrap();
        let body = parse(&req[req.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body.get("model").unwrap().as_str(), Some("test-model"));
        assert_eq!(body.get("messages").unwrap().as_array().unwrap().len(), 2);
        assert_eq!(body.get("stream"), Some(&Json::Bool(false)));
        assert!(req.contains("Authorization: Bearer k"));
    }

    #[test]
    fn server_errors_become_readable_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            // read the whole request first, like a real server (closing with unread data would reset the connection)
            let mut buf = vec![0u8; 16384];
            let mut n = 0;
            loop {
                n += s.read(&mut buf[n..]).unwrap();
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                if let Some(he) = text.find("\r\n\r\n") {
                    let len: usize = text[..he].to_lowercase().lines().find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                    if n >= he + 4 + len {
                        break;
                    }
                }
            }
            s.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 11\r\n\r\nmodel busy!").unwrap();
        });
        let mut m = OpenAiModel::new(&base, "m");
        m.retries = 0;
        let err = m.complete(&[Message::user("x")]).unwrap_err();
        assert!(err.contains("HTTP 503") && err.contains("model busy"), "{err}");
    }

    /// A server that answers every connection from `replies` in turn and hands back the request texts.
    fn serve_replies(replies: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for reply in replies {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = vec![0u8; 65536];
                let mut n = 0;
                loop {
                    n += s.read(&mut buf[n..]).unwrap();
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    if let Some(he) = text.find("\r\n\r\n") {
                        let len: usize = text[..he].to_lowercase().lines().find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                        if n >= he + 4 + len {
                            break;
                        }
                    }
                }
                seen.push(String::from_utf8_lossy(&buf[..n]).to_string());
                s.write_all(reply.as_bytes()).unwrap();
            }
            seen
        });
        (base, h)
    }

    #[test]
    fn a_busy_server_is_retried_and_a_schema_and_stats_travel() {
        let ok = r#"{"choices":[{"message":{"role":"assistant","content":"{}"}}],"usage":{"prompt_tokens":40,"completion_tokens":3,"prompt_tokens_details":{"cached_tokens":32}},"timings":{"prompt_ms":5.5}}"#;
        let (base, h) = serve_replies(vec!["HTTP/1.1 503 Service Unavailable\r\nContent-Length: 4\r\n\r\nbusy".to_string(), format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{ok}", ok.len())]);
        let mut m = OpenAiModel::new(&base, "m");
        m.retry_wait = Duration::from_millis(1);
        m.schema = Some(parse(r#"{"type":"object"}"#).unwrap());
        m.seed = Some(7);
        m.no_thinking = true;
        assert_eq!(m.complete(&[Message::user("x")]).unwrap(), "{}");
        let seen = h.join().unwrap();
        assert_eq!(seen.len(), 2, "asked again after the 503");
        let body = parse(&seen[1][seen[1].find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body.get("response_format").and_then(|r| r.get("type")).and_then(|t| t.as_str()), Some("json_schema"));
        assert_eq!(body.get("response_format").and_then(|r| r.get("json_schema")).and_then(|j| j.get("schema")).and_then(|s| s.get("type")).and_then(|t| t.as_str()), Some("object"));
        assert_eq!(body.get("seed").and_then(|s| s.as_i64()), Some(7));
        assert_eq!(body.get("chat_template_kwargs").and_then(|c| c.get("enable_thinking")), Some(&Json::Bool(false)));
        let stats = m.last_stats().unwrap();
        assert_eq!(stats.get("retries").and_then(|r| r.as_i64()), Some(1));
        assert_eq!(stats.get("usage").and_then(|u| u.get("prompt_tokens_details")).and_then(|d| d.get("cached_tokens")).and_then(|c| c.as_i64()), Some(32));
    }

    #[test]
    fn a_context_overflow_comes_with_a_hint() {
        let body = r#"{"error":"n_ctx exceeded"}"#;
        let (base, _h) = serve_replies(vec![format!("HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{body}", body.len())]);
        let err = OpenAiModel::new(&base, "m").complete(&[Message::user("x")]).unwrap_err();
        assert!(err.contains("does not fit"), "{err}");
    }

    #[test]
    fn scripted_model_repeats_its_last_reply_and_records_prompts() {
        let mut m = ScriptedModel::new(vec!["a".into(), "b".into()]);
        let got: Vec<String> = (0..3).map(|_| m.complete(&[Message::user("q")]).unwrap()).collect();
        assert_eq!(got, ["a", "b", "b"]);
        assert_eq!(m.seen.len(), 3);
    }
}
