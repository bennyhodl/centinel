//! Embedding — chunk text in, vectors out, from the server `[inference.embed]` names.
//!
//! The request is OpenAI's `POST /embeddings`, which llama-server (`--embedding`),
//! Ollama, vLLM, LM Studio and OpenRouter all answer. What the server does to produce a
//! vector — quantization, pooling, batch shape — is its business; what comes back is held
//! to one contract here, whichever server sent it.
//!
//! ## The contract
//!
//! One vector per input, in input order, every one the same width, finite, and unit
//! length. Order is read from each row's `index`, never from row order: the caller zips
//! the result against chunk hashes, and trusting arrival order would hand chunk 3 another
//! chunk's vector the day a server reorders. Normalization is done here rather than
//! trusted — normalizing a unit vector is a no-op, and cosine being a plain dot product is
//! a promise `search` relies on.
//!
//! ## The query prefix is the operator's to state
//!
//! Some models are asymmetric: Qwen3-Embedding wants a query wrapped as
//! `Instruct: {task}\nQuery:{q}` and a document embedded bare, and embedding queries bare
//! produces *plausible* vectors and quietly worse retrieval. Others — OpenAI's — want no
//! prefix at all. The server cannot say which, so `[inference.embed] query_prefix` does,
//! and a written `[inference.embed]` table must state it, even as `""`.

use std::time::Duration;

use serde::Deserialize;

use crate::inference::{Client, Endpoint, Fatal};
use crate::spend::{self, Spend};
use crate::vectors::Space;

/// The prefix Qwen3-Embedding is trained with, which is the default model's.
pub const QWEN3_QUERY_PREFIX: &str = "Instruct: Given a web search query, retrieve relevant \
                                      passages that answer the query\nQuery:";

/// One embedding request may sit on the wire this long before it is a failure. Generous,
/// because a batch of 64 chunks on a laptop's GPU is a real piece of work.
const TIMEOUT: Duration = Duration::from_secs(120);

/// A connection to one embedding server, for one model.
#[derive(Debug)]
pub struct Embedder {
    client: Client,
    query_prefix: String,
    ledger: spend::Ledger,
    /// The width every vector must have, once a table has said what it holds.
    dims: Option<usize>,
}

impl Embedder {
    /// Reads the key and builds the client, failing loudly when a named key is absent.
    /// Every request it answers is written to `ledger`.
    pub fn new(
        endpoint: Endpoint,
        query_prefix: &str,
        ledger: spend::Ledger,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            client: Client::new(endpoint, TIMEOUT)?,
            query_prefix: query_prefix.to_string(),
            ledger,
            dims: None,
        })
    }

    /// Holds every response to `dims`. A server that answers at another width is serving
    /// another model, and that is [`Fatal`]: every later batch would be refused the same.
    pub fn with_dims(mut self, dims: usize) -> Self {
        self.dims = Some(dims);
        self
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.client.endpoint
    }

    /// The vector space this embedder's vectors are in: the model, and where it ran.
    pub fn space(&self) -> Space {
        Space::of(&self.client.endpoint)
    }

    /// Asks the server what width it embeds at, with one short request. A new table has
    /// to be created at a width before the first row is written, and nothing but the
    /// server knows it.
    pub async fn probe_dims(&self) -> anyhow::Result<usize> {
        let vector = self
            .call(&["centinel".to_string()], spend::Stage::Embed)
            .await?
            .remove(0);
        Ok(vector.len())
    }

    /// Embeds a query, with the configured prefix.
    pub async fn embed_query(&self, query: &str) -> anyhow::Result<Vec<f32>> {
        let text = format!("{}{query}", self.query_prefix);
        Ok(self.call(&[text], spend::Stage::Query).await?.remove(0))
    }

    /// Embeds documents, bare, order preserved.
    pub async fn embed_documents<S: AsRef<str>>(
        &self,
        texts: &[S],
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let owned: Vec<String> = texts.iter().map(|t| t.as_ref().to_string()).collect();
        self.call(&owned, spend::Stage::Embed).await
    }

    /// One request, parsed, verified, and written to the ledger. A request that failed is
    /// not written: a server bills what it answered.
    async fn call(&self, inputs: &[String], stage: spend::Stage) -> anyhow::Result<Vec<Vec<f32>>> {
        let endpoint = &self.client.endpoint;
        let bytes = self
            .client
            .post_json(&serde_json::json!({
                "model": endpoint.model,
                "input": inputs,
                "encoding_format": "float",
            }))
            .await?;
        let (vectors, usage) = parse_response(&bytes, inputs.len(), self.dims)?;
        self.ledger.record(
            Spend::new(stage, &endpoint.ledger_model(), usage.prompt_tokens, 0)
                .reported(usage.cost),
        );
        Ok(vectors)
    }
}

/// The OpenAI response shape. Unknown fields are ignored.
#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingRow>,
    #[serde(default)]
    usage: Usage,
}

/// What a server says a request used. `cost` is OpenRouter's own figure, in USD, when it
/// sends one; without it the tokens are priced from [`crate::spend::PRICES`]. A response
/// with no usage block at all still embeds, and is written as zero tokens, not a guess.
#[derive(Debug, Default, Deserialize, PartialEq)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u64,
    cost: Option<f64>,
}

#[derive(Deserialize)]
struct EmbeddingRow {
    index: usize,
    embedding: Vec<f32>,
}

/// Parses a response and holds it to the contract in the module docs.
fn parse_response(
    bytes: &[u8],
    expected: usize,
    dims: Option<usize>,
) -> anyhow::Result<(Vec<Vec<f32>>, Usage)> {
    let response: EmbeddingResponse = serde_json::from_slice(bytes).map_err(|e| {
        let head: String = String::from_utf8_lossy(bytes).chars().take(200).collect();
        anyhow::anyhow!("the embedding response did not parse: {e} — starts `{head}`")
    })?;

    let mut width = dims;
    let mut out: Vec<Option<Vec<f32>>> = vec![None; expected];
    for row in response.data {
        anyhow::ensure!(
            row.index < expected,
            "the server returned index {} for {expected} inputs",
            row.index
        );
        match width {
            Some(w) if w != row.embedding.len() => {
                return Err(Fatal(format!(
                    "the server returned a {}-dimensional vector where {w} were expected — \
                     it is serving a different model than the one these vectors came from",
                    row.embedding.len()
                ))
                .into());
            }
            Some(_) => {}
            None => width = Some(row.embedding.len()),
        }
        anyhow::ensure!(
            row.embedding.iter().all(|x| x.is_finite()),
            "input {} came back non-finite",
            row.index
        );
        let slot = &mut out[row.index];
        anyhow::ensure!(
            slot.is_none(),
            "the server returned index {} twice",
            row.index
        );
        *slot = Some(normalize(&row.embedding));
    }

    let vectors = out
        .into_iter()
        .enumerate()
        .map(|(i, v)| {
            v.ok_or_else(|| anyhow::anyhow!("the server returned no vector for input {i}"))
        })
        .collect::<anyhow::Result<_>>()?;
    Ok((vectors, response.usage))
}

/// Scales a vector to unit length, so cosine similarity is a dot product.
pub(crate) fn normalize(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm == 0.0 {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

/// Cosine similarity of two normalized vectors.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_produces_unit_vectors() {
        let v = normalize(&[3.0, 4.0]);
        assert!((v[0] - 0.6).abs() < 1e-6);
        assert!((v[1] - 0.8).abs() < 1e-6);
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalizing_a_zero_vector_does_not_divide_by_zero() {
        assert_eq!(normalize(&[0.0, 0.0]), vec![0.0, 0.0]);
    }

    fn body(rows: &[(usize, Vec<f32>)]) -> Vec<u8> {
        let data: Vec<_> = rows
            .iter()
            .map(|(i, v)| serde_json::json!({"index": i, "embedding": v}))
            .collect();
        serde_json::to_vec(&serde_json::json!({
            "data": data,
            "usage": {"prompt_tokens": 7, "total_tokens": 7, "cost": 0.0000014}
        }))
        .unwrap()
    }

    /// The tokens and OpenRouter's own cost come back with the vectors, and a response
    /// without a usage block — llama-server sends one, not every server does — still parses.
    #[test]
    fn usage_is_read_and_optional() {
        let (_, usage) = parse_response(&body(&[(0, vec![1.0, 0.0])]), 1, Some(2)).unwrap();
        assert_eq!(
            usage,
            Usage {
                prompt_tokens: 7,
                cost: Some(0.0000014)
            }
        );
        let bare = serde_json::to_vec(&serde_json::json!({
            "data": [{"index": 0, "embedding": [1.0, 0.0]}]
        }))
        .unwrap();
        assert_eq!(
            parse_response(&bare, 1, Some(2)).unwrap().1,
            Usage::default()
        );
    }

    /// Rows come back keyed by `index`, and the contract is input order — so a
    /// reordered response must land straight, not crossed.
    #[test]
    fn vectors_are_returned_in_input_order_whatever_the_row_order() {
        let bytes = body(&[(1, vec![0.0, 1.0]), (0, vec![1.0, 0.0])]);
        let (out, _) = parse_response(&bytes, 2, None).unwrap();
        assert_eq!(out[0], vec![1.0, 0.0]);
        assert_eq!(out[1], vec![0.0, 1.0]);
    }

    #[test]
    fn vectors_are_normalized_here_not_trusted() {
        let (out, _) = parse_response(&body(&[(0, vec![3.0, 4.0])]), 1, None).unwrap();
        assert!((out[0][0] - 0.6).abs() < 1e-6);
        assert!((out[0][1] - 0.8).abs() < 1e-6);
    }

    /// A server answering at another width is serving another model. That ends the run —
    /// every later batch would be refused the same — before anything is written.
    #[test]
    fn a_wrong_width_is_fatal() {
        let err = parse_response(&body(&[(0, vec![1.0, 0.0, 0.0])]), 1, Some(2)).unwrap_err();
        assert!(crate::inference::is_fatal(&err), "{err}");
        assert!(err.to_string().contains("3-dimensional"), "{err}");

        let mixed = body(&[(0, vec![1.0, 0.0]), (1, vec![1.0, 0.0, 0.0])]);
        assert!(
            parse_response(&mixed, 2, None).is_err(),
            "the first row pins the width when nothing else has"
        );
    }

    #[test]
    fn a_missing_or_duplicate_row_is_refused() {
        let missing = body(&[(0, vec![1.0, 0.0])]);
        assert!(
            parse_response(&missing, 2, None).is_err(),
            "one of two absent"
        );

        let twice = body(&[(0, vec![1.0, 0.0]), (0, vec![0.0, 1.0])]);
        assert!(parse_response(&twice, 2, None).is_err(), "index 0 twice");

        let stray = body(&[(5, vec![1.0, 0.0])]);
        assert!(
            parse_response(&stray, 1, None).is_err(),
            "index out of range"
        );
    }

    /// The prefix is applied to queries and never to documents — the asymmetry is the
    /// model's, and the fake server echoes what it was sent so the test can see it.
    #[tokio::test]
    async fn the_prefix_goes_on_queries_only() {
        let server = crate::inference::fake::serve().await;
        let dir = tempfile::tempdir().unwrap();
        let ledger = spend::Ledger::new(&crate::store::Store::at(dir.path()));
        let endpoint = server.endpoint(crate::inference::Capability::Embed);
        let embedder = Embedder::new(endpoint, "query about: ", ledger).unwrap();

        let query = embedder.embed_query("water").await.unwrap();
        let prefixed = embedder
            .embed_documents(&["query about: water"])
            .await
            .unwrap();
        let bare = embedder.embed_documents(&["water"]).await.unwrap();
        assert!((cosine(&query, &prefixed[0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&query, &bare[0]) < 0.999);
    }
}
