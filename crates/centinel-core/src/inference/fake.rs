//! A model server for tests: embeddings, rerank and transcription, spoken over a raw
//! socket the way `workspace::fake_jev` speaks Jev.
//!
//! The "model" is a bag of words. A text's vector counts its words into [`DIMS`] buckets,
//! so two passages that share words are near each other and two that share none are not —
//! enough for a search test to assert on *which* passage wins without loading weights. A
//! reranker scores the share of the query's words a passage carries. A transcription is
//! a fixed council meeting.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{Capability, Endpoint};

/// The fake embedder's width.
pub(crate) const DIMS: usize = 64;

/// A running fake. Dropping it leaves the listener running until the test ends, which is
/// what `fake_jev` does too.
#[derive(Clone)]
pub(crate) struct FakeServer {
    pub base_url: String,
    /// Requests seen, by path: `embeddings`, `rerank`, `audio/transcriptions`.
    hits: Arc<Mutex<HashMap<String, usize>>>,
}

impl FakeServer {
    /// An endpoint for one capability on this server.
    pub fn endpoint(&self, capability: Capability) -> Endpoint {
        let model = match capability {
            Capability::Embed => "fake-embed",
            Capability::Rerank => "fake-rerank",
            Capability::Transcribe => "fake-whisper",
        };
        Endpoint::new(capability, &self.base_url, model, None)
    }

    /// An `[inference]` block pointing every capability here.
    pub fn config(&self) -> crate::config::InferenceConfig {
        crate::config::Config::parse(&format!(
            "[inference]\nbase_url = \"{}\"\n\
             [inference.embed]\nmodel = \"fake-embed\"\nquery_prefix = \"\"\n\
             [inference.rerank]\nmodel = \"fake-rerank\"\n\
             [inference.transcribe]\nmodel = \"fake-whisper\"\n",
            self.base_url
        ))
        .unwrap()
        .inference
    }

    pub fn hits(&self, path: &str) -> usize {
        self.hits.lock().unwrap().get(path).copied().unwrap_or(0)
    }
}

/// A server whose embeddings are [`bag_of_words`] with `salt` mixed into the bucket hash.
/// Two servers with different salts are two models: same width, different space.
pub(crate) async fn serve_salted(salt: u64) -> FakeServer {
    serve_with(salt, None).await
}

/// A server that answers every request with `status` — an outage, for the tests of what a
/// stage does when one never ends.
pub(crate) async fn serve_failing(status: u16) -> FakeServer {
    serve_with(0, Some(status)).await
}

async fn serve_with(salt: u64, failing: Option<u16>) -> FakeServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits: Arc<Mutex<HashMap<String, usize>>> = Arc::default();
    let seen = hits.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let seen = seen.clone();
            tokio::spawn(async move {
                let Some((path, body)) = read_request(&mut socket).await else {
                    return;
                };
                let path = path.trim_start_matches("/v1/").to_string();
                *seen.lock().unwrap().entry(path.clone()).or_default() += 1;
                let (status, answer) = match (failing, path.as_str()) {
                    (Some(status), _) => (status, json!({"error": "unavailable"}).to_string()),
                    (None, "embeddings") => (200, embeddings(&body, salt)),
                    (None, "rerank") => (200, rerank(&body)),
                    (None, "audio/transcriptions") => (200, transcription(&body)),
                    _ => (404, json!({"error": "not found"}).to_string()),
                };
                let reason = if status == 200 { "OK" } else { "Error" };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    FakeServer {
        base_url: format!("http://{addr}/v1"),
        hits,
    }
}

/// An `[inference]` pointing every capability at a port nothing listens on.
pub(crate) fn nothing_listening() -> crate::config::InferenceConfig {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    crate::config::InferenceConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        ..Default::default()
    }
}

/// The fake every test uses unless it needs a second space.
pub(crate) async fn serve() -> FakeServer {
    serve_salted(0).await
}

/// The request line's path and the body, by `Content-Length` or chunked.
async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let body_start = loop {
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..body_start]).to_ascii_lowercase();
    let path = head.split_whitespace().nth(1)?.to_string();
    let length: Option<usize> = head.lines().find_map(|l| {
        l.strip_prefix("content-length:")
            .and_then(|v| v.trim().parse().ok())
    });
    let chunked = head.contains("transfer-encoding: chunked");
    loop {
        let done = match length {
            Some(length) => buf.len() >= body_start + length,
            None if chunked => buf.ends_with(b"0\r\n\r\n"),
            None => true,
        };
        if done {
            break;
        }
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Some((path, buf[body_start..].to_vec()))
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The fake model: each word counted into a bucket chosen by hashing it with `salt`.
pub(crate) fn bag_of_words(text: &str, salt: u64) -> Vec<f32> {
    use std::hash::{Hash, Hasher};
    let mut v = vec![0.0f32; DIMS];
    for word in words(text) {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (salt, &word).hash(&mut h);
        v[(h.finish() % DIMS as u64) as usize] += 1.0;
    }
    v
}

fn embeddings(body: &[u8], salt: u64) -> String {
    let request: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let inputs: Vec<String> = match &request["input"] {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(a) => a
            .iter()
            .map(|v| v.as_str().unwrap_or_default().to_string())
            .collect(),
        _ => Vec::new(),
    };
    // Reversed, so a client that trusts row order rather than `index` crosses its wires.
    let data: Vec<_> = inputs
        .iter()
        .enumerate()
        .rev()
        .map(|(i, text)| json!({"index": i, "embedding": bag_of_words(text, salt)}))
        .collect();
    let tokens: usize = inputs.iter().map(|t| words(t).len()).sum();
    json!({"data": data, "usage": {"prompt_tokens": tokens, "total_tokens": tokens}}).to_string()
}

fn rerank(body: &[u8]) -> String {
    let request: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let query = words(request["query"].as_str().unwrap_or_default());
    let mut results: Vec<_> = request["documents"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, doc)| {
            let doc = words(doc.as_str().unwrap_or_default());
            let shared = query.iter().filter(|w| doc.contains(w)).count();
            let score = shared as f64 / query.len().max(1) as f64;
            (i, score)
        })
        .collect();
    // Best first, as Cohere and llama-server both answer — the client must not care.
    results.sort_by(|a, b| b.1.total_cmp(&a.1));
    let results: Vec<_> = results
        .into_iter()
        .map(|(index, score)| json!({"index": index, "relevance_score": score}))
        .collect();
    json!({"model": request["model"], "results": results}).to_string()
}

/// A council meeting, if the form asked for `verbose_json` and carried a file.
fn transcription(body: &[u8]) -> String {
    let form = String::from_utf8_lossy(body);
    if !form.contains("verbose_json") || !form.contains("name=\"file\"") {
        return json!({"text": "no segments without verbose_json"}).to_string();
    }
    json!({
        "task": "transcribe",
        "language": "english",
        "duration": 4273.5,
        "text": "The council meeting will come to order. Item four is the alley vacation.",
        "segments": [
            {"id": 0, "start": 0.0, "end": 1.82, "text": " The council meeting will come to order.", "no_speech_prob": 0.01},
            {"id": 1, "start": 4271.0, "end": 4273.5, "text": " Item four is the alley vacation.", "no_speech_prob": 0.02},
        ],
    })
    .to_string()
}
