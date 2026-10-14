//! Reranking — scoring passages against a query with a cross-encoder on a server.
//!
//! The second stage of SPEC §6, and always on (§6.3). The first stage over-fetches
//! cheaply; this one decides the order. The measured gap is large: BM25 goes from **14.8
//! to 33.4** nDCG@10 when reranked, and reranked BM25 beats an expensively-trained
//! reasoning-tuned dense retriever used alone at 29.1.
//!
//! ## The one request with no OpenAI spec
//!
//! OpenAI publishes no rerank endpoint, so this speaks the shape Cohere and Jina
//! published and everyone else copied — llama-server under `--reranking`, vLLM, TEI,
//! Infinity, LocalAI:
//!
//! ```text
//!   POST {base_url}/rerank   {"model", "query", "documents": [...], "top_n"}
//!   →                        {"results": [{"index", "relevance_score"}, ...]}
//! ```
//!
//! The recipe a cross-encoder needs — Qwen3-Reranker's chat template, reading `yes` and
//! `no` off the final logits — is the server's to apply, which is what it is for. The
//! scores come back in whatever range the server uses; only their order is relied on.

use std::time::Duration;

use serde::Deserialize;

use crate::inference::{Client, Endpoint};

/// A rerank is forty passages of ~300 tokens: seconds on a laptop, and a search is
/// waiting on it.
const TIMEOUT: Duration = Duration::from_secs(60);

/// A connection to one rerank server, for one model.
#[derive(Debug)]
pub struct Reranker {
    client: Client,
}

#[derive(Deserialize)]
struct RerankResponse {
    results: Vec<RerankRow>,
}

#[derive(Deserialize)]
struct RerankRow {
    index: usize,
    relevance_score: f32,
}

impl Reranker {
    pub fn new(endpoint: Endpoint) -> anyhow::Result<Self> {
        Ok(Self {
            client: Client::new(endpoint, TIMEOUT)?,
        })
    }

    /// One score per document, in the documents' order.
    pub async fn score(&self, query: &str, documents: &[String]) -> anyhow::Result<Vec<f32>> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let bytes = self
            .client
            .post_json(&serde_json::json!({
                "model": self.client.endpoint.model,
                "query": query,
                "documents": documents,
                "top_n": documents.len(),
            }))
            .await?;
        parse_response(&bytes, documents.len())
    }
}

/// Scores in document order, read by each row's `index`. A server answers best first,
/// and one that drops a document has said nothing about it — so every document must be
/// scored, once, or the response is refused rather than paired up arbitrarily.
fn parse_response(bytes: &[u8], expected: usize) -> anyhow::Result<Vec<f32>> {
    let response: RerankResponse = serde_json::from_slice(bytes).map_err(|e| {
        let head: String = String::from_utf8_lossy(bytes).chars().take(200).collect();
        anyhow::anyhow!("the rerank response did not parse: {e} — starts `{head}`")
    })?;
    let mut scores: Vec<Option<f32>> = vec![None; expected];
    for row in response.results {
        anyhow::ensure!(
            row.index < expected,
            "the reranker returned index {} for {expected} documents",
            row.index
        );
        anyhow::ensure!(
            row.relevance_score.is_finite(),
            "the reranker scored document {} as {}",
            row.index,
            row.relevance_score
        );
        anyhow::ensure!(
            scores[row.index].replace(row.relevance_score).is_none(),
            "the reranker scored document {} twice",
            row.index
        );
    }
    scores
        .into_iter()
        .enumerate()
        .map(|(i, s)| s.ok_or_else(|| anyhow::anyhow!("the reranker did not score document {i}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Best-first rows land on the documents they name.
    #[test]
    fn scores_come_back_in_document_order() {
        let body = serde_json::json!({"results": [
            {"index": 2, "relevance_score": 0.9},
            {"index": 0, "relevance_score": 0.4},
            {"index": 1, "relevance_score": -1.5},
        ]});
        let scores = parse_response(&serde_json::to_vec(&body).unwrap(), 3).unwrap();
        assert_eq!(scores, vec![0.4, -1.5, 0.9]);
    }

    #[test]
    fn a_document_left_unscored_is_refused() {
        let body = serde_json::json!({"results": [{"index": 0, "relevance_score": 0.4}]});
        let err = parse_response(&serde_json::to_vec(&body).unwrap(), 2).unwrap_err();
        assert!(err.to_string().contains("document 1"), "{err}");
    }

    #[tokio::test]
    async fn the_passage_that_shares_the_query_wins() {
        let server = crate::inference::fake::serve().await;
        let reranker =
            Reranker::new(server.endpoint(crate::inference::Capability::Rerank)).unwrap();
        let scores = reranker
            .score(
                "alley vacation hearing",
                &[
                    "bees in residential zones".to_string(),
                    "public hearing on the alley vacation".to_string(),
                ],
            )
            .await
            .unwrap();
        assert!(scores[1] > scores[0], "{scores:?}");
    }
}
