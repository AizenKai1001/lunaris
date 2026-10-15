//! A mind's long-term memory: what happened is written down as short notes, each turned into a vector by an embedding model, and the notes
//! most like the present situation are put back in front of the model. The conversation history is cut now and then (that is how a small
//! model's window is kept useful); the memory is what remembers across the cuts.
//!
//! The embedding server is any OpenAI-style `/v1/embeddings` (ForgeInfer serves nomic-embed-text-v1.5 that way). Models of that family want
//! a task prefix in the text itself: `search_document: ` for what is stored and `search_query: ` for the question.
use crate::http::post_json;
use lunaris_json::{parse, Json};
use std::time::Duration;

pub trait Embedder {
    /// One vector per text. `query` says the texts are questions to look up with rather than notes to store.
    fn embed(&mut self, texts: &[String], query: bool) -> Result<Vec<Vec<f32>>, String>;
}

pub struct OpenAiEmbedder {
    /// Base URL including the version prefix, e.g. `http://127.0.0.1:8081/v1`.
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub timeout: Duration,
    pub doc_prefix: String,
    pub query_prefix: String,
}

impl OpenAiEmbedder {
    pub fn new(base_url: &str, model: &str) -> OpenAiEmbedder {
        OpenAiEmbedder { base_url: base_url.trim_end_matches('/').to_string(), model: model.to_string(), api_key: None, timeout: Duration::from_secs(30), doc_prefix: "search_document: ".to_string(), query_prefix: "search_query: ".to_string() }
    }
}

impl Embedder for OpenAiEmbedder {
    fn embed(&mut self, texts: &[String], query: bool) -> Result<Vec<Vec<f32>>, String> {
        let prefix = if query { &self.query_prefix } else { &self.doc_prefix };
        let input = Json::arr(texts.iter().map(|t| Json::str(format!("{prefix}{t}"))));
        let body = Json::obj([("model", Json::str(self.model.clone())), ("input", input), ("encoding_format", Json::str("float"))]).to_string_compact().map_err(|e| e.to_string())?;
        let mut headers = Vec::new();
        if let Some(k) = &self.api_key {
            headers.push(("Authorization".to_string(), format!("Bearer {k}")));
        }
        let resp = post_json(&format!("{}/embeddings", self.base_url), &body, &headers, self.timeout)?;
        if resp.status != 200 {
            return Err(format!("the embedding server answered HTTP {}: {}", resp.status, resp.body.chars().take(200).collect::<String>()));
        }
        let j = parse(&resp.body).map_err(|e| format!("the embedding server sent invalid JSON: {e}"))?;
        let mut rows: Vec<(usize, Vec<f32>)> = Vec::new();
        for (i, d) in j.get("data").and_then(|d| d.as_array()).unwrap_or(&[]).iter().enumerate() {
            let v: Vec<f32> = d.get("embedding").and_then(|e| e.as_array()).ok_or("an embedding is missing from the reply")?.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect();
            rows.push((d.get("index").and_then(|x| x.as_u64()).map(|x| x as usize).unwrap_or(i), v));
        }
        rows.sort_by_key(|r| r.0);
        if rows.len() != texts.len() {
            return Err(format!("asked for {} embeddings, got {}", texts.len(), rows.len()));
        }
        Ok(rows.into_iter().map(|r| r.1).collect())
    }
}

fn normalized(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 1.0e-12 {
        for x in &mut v {
            *x /= n;
        }
    }
    v
}

struct Note {
    tick: u64,
    text: String,
    vec: Vec<f32>,
}

/// Notes with their vectors, and the lookup by similarity (cosine).
pub struct Memory {
    embedder: Box<dyn Embedder>,
    notes: Vec<Note>,
    /// The oldest notes are forgotten beyond this many.
    pub capacity: usize,
}

impl Memory {
    pub fn new(embedder: Box<dyn Embedder>) -> Memory {
        Memory { embedder, notes: Vec::new(), capacity: 2000 }
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// Write a note down (one call to the embedding server).
    pub fn remember(&mut self, tick: u64, text: &str) -> Result<(), String> {
        let v = self.embedder.embed(&[text.to_string()], false)?.into_iter().next().ok_or("no embedding came back")?;
        self.notes.push(Note { tick, text: text.to_string(), vec: normalized(v) });
        if self.notes.len() > self.capacity {
            self.notes.remove(0);
        }
        Ok(())
    }

    /// The `k` notes most like `query`, best first, as (similarity, tick, text); notes written at or after `before` are left out (the
    /// recent ones are in the conversation already). Notes below `min_score` are not worth showing.
    pub fn recall(&mut self, query: &str, k: usize, before: u64, min_score: f32) -> Result<Vec<(f32, u64, String)>, String> {
        let candidates: Vec<usize> = (0..self.notes.len()).filter(|&i| self.notes[i].tick < before).collect();
        if candidates.is_empty() || k == 0 {
            return Ok(Vec::new());
        }
        let q = normalized(self.embedder.embed(&[query.to_string()], true)?.into_iter().next().ok_or("no embedding came back")?);
        let mut scored: Vec<(f32, usize)> = candidates.iter().map(|&i| (self.notes[i].vec.iter().zip(&q).map(|(a, b)| a * b).sum::<f32>(), i)).filter(|(s, _)| *s >= min_score).collect();
        // best first; equal scores: the newer note
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
        Ok(scored.into_iter().take(k).map(|(s, i)| (s, self.notes[i].tick, self.notes[i].text.clone())).collect())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Words hashed into 64 buckets: texts that share words are similar. Enough to test the lookup without a model.
    pub struct WordsEmbedder;

    impl Embedder for WordsEmbedder {
        fn embed(&mut self, texts: &[String], _query: bool) -> Result<Vec<Vec<f32>>, String> {
            Ok(texts
                .iter()
                .map(|t| {
                    let mut v = vec![0.0f32; 64];
                    for w in t.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2) {
                        let h = w.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
                        v[(h % 64) as usize] += 1.0;
                    }
                    v
                })
                .collect())
        }
    }

    #[test]
    fn the_notes_most_like_the_question_come_back_best_first_and_recent_ones_are_left_out() {
        let mut m = Memory::new(Box::new(WordsEmbedder));
        for (tick, text) in [(10, "found a red key behind the waterfall"), (20, "the bridge was guarded by two wolves"), (30, "traded the old sword with the blacksmith"), (40, "heard wolves howling near the bridge at night"), (50, "slept in the inn")] {
            m.remember(tick, text).unwrap();
        }
        let r = m.recall("which animals are at the bridge", 2, 50, 0.1).unwrap();
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|(_, _, t)| t.contains("bridge")), "{r:?}");
        assert!(r[0].0 >= r[1].0);
        // notes from `before` on are not offered, and a question like nothing stored finds nothing
        assert!(m.recall("wolves", 5, 20, 0.1).unwrap().iter().all(|(_, tick, _)| *tick < 20));
        assert!(m.recall("a b", 3, 100, 0.1).unwrap().is_empty());
    }

    #[test]
    fn the_openai_embedder_sends_prefixed_input_and_reads_vectors_back_in_index_order() {
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
            // out of order on purpose
            let reply = r#"{"data":[{"index":1,"embedding":[0,2]},{"index":0,"embedding":[1,0]}]}"#;
            s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{reply}", reply.len()).as_bytes()).unwrap();
            String::from_utf8_lossy(&buf[..n]).to_string()
        });
        let mut e = OpenAiEmbedder::new(&base, "nomic");
        let v = e.embed(&["a".to_string(), "b".to_string()], true).unwrap();
        assert_eq!(v, vec![vec![1.0, 0.0], vec![0.0, 2.0]]);
        let req = h.join().unwrap();
        let body = parse(&req[req.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body.get("input").unwrap().index(0).unwrap().as_str(), Some("search_query: a"));
        assert_eq!(body.get("model").unwrap().as_str(), Some("nomic"));
    }
}
