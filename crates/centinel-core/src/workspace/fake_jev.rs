//! A loopback stand-in for Jev, for tests that run the whole classifier.

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A server that answers like Jev, on loopback. A yes-or-no question is 0.95, or 0.05
/// when the text says MENU; a choice gives 0.95 to `navigation` when the text says MENU
/// and to `record` otherwise, so one document is junk and the other is not, and the test
/// knows which. It answers exactly the questions a request carries, and reports 100 input
/// tokens a request, so a run's token count says how many requests it sent.
pub(crate) async fn fake_jev() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let body_start = loop {
                    let n = socket.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..body_start]).to_string();
                let length: usize = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                while buf.len() < body_start + length {
                    let n = socket.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let request: serde_json::Value =
                    serde_json::from_slice(&buf[body_start..]).unwrap();
                let text = request["state"]["text"].as_str().unwrap_or_default();
                let junk = text.contains("MENU");
                let mut answers = serde_json::Map::new();
                for (id, question) in request["questions"].as_object().unwrap() {
                    if question["type"] == "noul" {
                        let yes = if junk { 0.05 } else { 0.95 };
                        answers.insert(id.clone(), json!({ "noul": yes }));
                        continue;
                    }
                    let options: Vec<&String> =
                        question["criteria"].as_object().unwrap().keys().collect();
                    let want = if junk { "navigation" } else { "record" };
                    let pick = options
                        .iter()
                        .find(|o| o.as_str() == want)
                        .unwrap_or(&options[0]);
                    let rest = 0.05 / (options.len() - 1) as f64;
                    let probabilities: serde_json::Map<String, serde_json::Value> = options
                        .iter()
                        .map(|o| ((*o).clone(), json!(if o == pick { 0.95 } else { rest })))
                        .collect();
                    answers.insert(id.clone(), json!({ "choice": pick, "probabilities": probabilities, "confidence": 0.9 }));
                }
                let body = json!({ "model": "jev-test", "answers": answers, "usage": { "input_tokens": 100, "output_tokens": 4 } }).to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    format!("http://{addr}/v1/systemone")
}
