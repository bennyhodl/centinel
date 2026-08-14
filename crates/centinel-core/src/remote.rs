//! Remote embedding — chunk text sent to OpenRouter instead of through local weights.
//!
//! SPEC §2.1 says no inference leaves the machine, and §2.3 sets the rule for touching
//! that kind of constraint: reopening it is done explicitly or not at all. This module
//! is the explicit reopening, for exactly one stage. An operator who names an
//! `openrouter/…` model has said, in the one place a model is ever named, that chunk
//! text — and at search time, the query — may go to `openrouter.ai`. Nothing else
//! leaves: blobs, logs, provenance and the index stay where §5 puts them, and no code
//! path falls back to the network when local weights are missing. Absent weights still
//! fail loudly with the pull command, never with a quiet upload.
//!
//! ## The id is the routing
//!
//! A remote model's id carries the `openrouter/` prefix, and what follows it is the
//! slug OpenRouter is asked for, verbatim. The id is written into the vector table
//! exactly as a local id is, so `search` can reproduce query vectors years later by
//! reading the table: the id says not just *which* model but *where it ran*. A field
//! beside the id would say the same thing only to code that remembered to read it.
//!
//! ## Same weights, still two spaces
//!
//! `openrouter/qwen/qwen3-embedding-4b` is the model the local default runs. It still
//! gets its own table identity, because the vector space is a property of the model
//! *as run* — quantization differs (Q8_0 here, the provider's serving there), and
//! parity between the two is a measurement nobody has made. Mixing them would probably
//! work and would degrade retrieval silently if it did not, which is the failure mode
//! [`crate::embed`] spends its module docs on. Two honest tables beat one confident
//! wrong one; a measured parity bench can merge them later.
//!
//! ## Pinned dimensions
//!
//! Each entry pins `dims` the way the local registry does, because the width is part
//! of §5.2's cache key and because `embed --dry-run` must plan a run with no network
//! and no key. The pin is verified against every response, so a provider serving a
//! different width is refused before anything is written.
//!
//! ## The recipe still applies
//!
//! The Qwen3 models are instruction-aware and asymmetric wherever they run, so the
//! query prefix ([`crate::embed::QUERY_INSTRUCTION`]) is applied client-side for them
//! and for no one else — `instructed` on the spec is that fact, stated per model
//! rather than guessed from the id. L2 normalization is also done here, not trusted
//! to the provider: normalizing an already-normalized vector is a no-op, and cosine
//! being a plain dot product is a promise `search` relies on.

use serde::Deserialize;

use crate::models::{self, ModelSpec};

/// The environment variable holding the API key.
///
/// Environment-only on purpose: the config file is committed, pasted into issues and
/// printed by `doctor`, and a secret that lives there eventually does all three.
pub const ENV_API_KEY: &str = "OPENROUTER_API_KEY";

/// What marks a model id as remote, and what is stripped to get the OpenRouter slug.
pub const PREFIX: &str = "openrouter/";

const ENDPOINT: &str = "https://openrouter.ai/api/v1/embeddings";

/// Attempts per request. The first is the request; the rest cover a 429 or a wobble in
/// the network, with the pause doubling each time. A failure that survives all of them
/// reaches `ops::embed`'s one-at-a-time retry, so this is the cheap layer, not the
/// only one.
const ATTEMPTS: u32 = 3;

/// The first retry pause, in milliseconds. Doubles per attempt.
const BACKOFF_MS: u64 = 1_000;

/// One embedding request may sit on the wire this long before it is a failure.
/// Generous, because a batch of 128 chunks is a real piece of work for the far end.
const TIMEOUT_SECS: u64 = 120;

/// An embedding model OpenRouter serves.
///
/// The remote peer of [`models::ModelSpec`], and deliberately thinner: no files, no
/// revision, no variants — the weights never land on this machine, so there is nothing
/// to pin a digest of. What must still be pinned is pinned: the width, and whether the
/// model wants the query instruction.
#[derive(Clone, Copy, Debug)]
pub struct RemoteModelSpec {
    /// What the operator types and what the vector table records. Always carries
    /// [`PREFIX`]; the remainder is the OpenRouter slug, verbatim.
    pub id: &'static str,
    pub about: &'static str,
    /// Output width. Part of §5.2's cache key, and verified against every response.
    pub dims: u32,
    /// Whether queries get [`crate::embed::QUERY_INSTRUCTION`]. The Qwen3 recipe is
    /// the model's wherever it runs; a symmetric model must not have it.
    pub instructed: bool,
    /// SPDX identifier for open weights, or the provider's name for weights that are
    /// only ever an API.
    pub license: &'static str,
}

impl RemoteModelSpec {
    /// The model name OpenRouter is asked for — the id with [`PREFIX`] stripped.
    pub fn slug(&self) -> &'static str {
        &self.id[PREFIX.len()..]
    }
}

/// Every remote embedding model Centinel knows the shape of.
///
/// Curated rather than open-ended, for the same reason the local registry is: the
/// width has to be pinned before the first byte is fetched, and an entry is the place
/// that pins it. OpenRouter serves ~30 embedding models; adding one is one entry here.
pub static REMOTE_REGISTRY: &[RemoteModelSpec] = &[
    RemoteModelSpec {
        id: "openrouter/qwen/qwen3-embedding-8b",
        about: "The strongest Qwen3 embedder — 4096-dim, 32K context. Too big for most \
                laptops locally; the reason to embed remotely at all.",
        dims: 4096,
        instructed: true,
        license: "Apache-2.0",
    },
    RemoteModelSpec {
        id: "openrouter/qwen/qwen3-embedding-4b",
        about: "The local default's weights, served remotely. A separate vector space \
                all the same — see the module docs.",
        dims: 2560,
        instructed: true,
        license: "Apache-2.0",
    },
    RemoteModelSpec {
        id: "openrouter/openai/text-embedding-3-large",
        about: "OpenAI's larger embedder, 3072-dim. Symmetric: no query instruction.",
        dims: 3072,
        instructed: false,
        license: "proprietary (OpenAI)",
    },
    RemoteModelSpec {
        id: "openrouter/openai/text-embedding-3-small",
        about: "OpenAI's smaller embedder, 1536-dim. Symmetric: no query instruction.",
        dims: 1536,
        instructed: false,
        license: "proprietary (OpenAI)",
    },
];

/// The remote spec for an id, or `None` when the id is not a remote one.
pub fn spec(id: &str) -> Option<&'static RemoteModelSpec> {
    REMOTE_REGISTRY.iter().find(|m| m.id == id)
}

/// Where an embedding model runs. The one seam between `llama.cpp` and OpenRouter:
/// `ops::embed` and `ops::search` both route through [`backend_for`], so the two
/// stages cannot disagree about what an id means.
#[derive(Clone, Copy, Debug)]
pub enum EmbeddingBackend {
    Local(&'static ModelSpec),
    Remote(&'static RemoteModelSpec),
}

/// Resolves a model id to the backend that runs it.
///
/// The prefix is checked first, so a misspelled remote id is refused with the remote
/// list rather than falling through to the local registry's refusal — which would name
/// only local models at somebody who plainly wanted a remote one.
pub fn backend_for(id: &str) -> anyhow::Result<EmbeddingBackend> {
    if let Some(found) = spec(id) {
        return Ok(EmbeddingBackend::Remote(found));
    }
    if id.starts_with(PREFIX) {
        let known: Vec<_> = REMOTE_REGISTRY.iter().map(|m| m.id).collect();
        anyhow::bail!(
            "unknown remote model `{id}` — try one of: {}",
            known.join(", ")
        );
    }
    models::require(id).map(EmbeddingBackend::Local)
}

/// The key was refused, or was never given. The one failure that must stop a run
/// rather than skip a chunk: every later request would fail the same way, and a
/// 400,000-chunk corpus skipped one chunk at a time is a run that looks alive for
/// hours while doing nothing.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct KeyRefused(String);

/// Whether an error anywhere in `err`'s chain is a [`KeyRefused`].
///
/// `ops::embed` retries a failed batch one chunk at a time, which is right for a chunk
/// the provider will not take and wrong for a key the provider will not take — this is
/// how the loop tells them apart.
pub fn is_fatal(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.downcast_ref::<KeyRefused>().is_some())
}

/// A connection to OpenRouter's embedding endpoint, for one model.
///
/// The remote peer of [`crate::embed::Embedder`]. Construction reads the key and
/// builds the client and is cheap — the expensive part is on the far end — so unlike
/// the local embedder there is nothing here to amortise across calls beyond the
/// connection pool `reqwest` already keeps.
pub struct RemoteEmbedder {
    client: reqwest::Client,
    spec: &'static RemoteModelSpec,
    key: String,
}

impl std::fmt::Debug for RemoteEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The key stays out of Debug the way it stays out of the config file.
        f.debug_struct("RemoteEmbedder")
            .field("model", &self.spec.id)
            .field("dims", &self.spec.dims)
            .finish()
    }
}

impl RemoteEmbedder {
    /// Builds a client, failing loudly when the key is absent — with the fix, the way
    /// missing weights name `centinel models pull`.
    pub fn new(spec: &'static RemoteModelSpec) -> anyhow::Result<Self> {
        let key = std::env::var(ENV_API_KEY).unwrap_or_default();
        let key = key.trim().to_string();
        anyhow::ensure!(
            !key.is_empty(),
            "{} needs an OpenRouter API key — export {ENV_API_KEY}=sk-or-…",
            spec.id
        );
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
            .user_agent(concat!("centinel/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| anyhow::anyhow!("building the HTTP client: {e}"))?;
        Ok(Self { client, spec, key })
    }

    pub fn model_id(&self) -> &'static str {
        self.spec.id
    }

    /// Output width. Part of §5.2's cache key `(chunk_hash, model_id, dims)`.
    pub fn dims(&self) -> usize {
        self.spec.dims as usize
    }

    /// Embeds a query, applying the instruction prefix where the model wants one —
    /// the remote half of [`crate::embed::Embedder::embed_query`].
    pub async fn embed_query(&self, query: &str) -> anyhow::Result<Vec<f32>> {
        let text = if self.spec.instructed {
            format!(
                "Instruct: {}\nQuery:{query}",
                crate::embed::QUERY_INSTRUCTION
            )
        } else {
            query.to_string()
        };
        Ok(self.call(&[text]).await?.remove(0))
    }

    /// Embeds documents, bare, order preserved — the remote half of
    /// [`crate::embed::Embedder::embed_documents`]. Documents are bare for an
    /// instructed model too; the asymmetry is the model's, not ours.
    pub async fn embed_documents<S: AsRef<str>>(
        &self,
        texts: &[S],
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let owned: Vec<String> = texts.iter().map(|t| t.as_ref().to_string()).collect();
        self.call(&owned).await
    }

    /// One request, retried through transient failures, parsed and verified.
    async fn call(&self, inputs: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let body = serde_json::json!({
            "model": self.spec.slug(),
            "input": inputs,
            "encoding_format": "float",
        });
        let body = serde_json::to_vec(&body)?;

        let mut last = None;
        for attempt in 0..ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(
                    BACKOFF_MS << (attempt - 1),
                ))
                .await;
            }
            match self.send(&body).await {
                Ok(bytes) => return parse_response(&bytes, inputs.len(), self.dims()),
                // A refused key will be refused again in one second. Stop here so the
                // caller's per-chunk retry does not turn one bad key into a day of it.
                Err(e) if is_fatal(&e) => return Err(e),
                Err(e) => {
                    tracing::debug!(attempt, error = %e, "embedding request failed");
                    last = Some(e);
                }
            }
        }
        Err(last.expect("ATTEMPTS is nonzero").context(format!(
            "openrouter did not take the request after {ATTEMPTS} attempts"
        )))
    }

    /// One HTTP exchange: the raw response body, or which kind of no.
    async fn send(&self, body: &[u8]) -> anyhow::Result<Vec<u8>> {
        let response = self
            .client
            .post(ENDPOINT)
            .bearer_auth(&self.key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_vec())
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("reaching openrouter: {e}"))?;

        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| anyhow::anyhow!("reading openrouter's response: {e}"))?;

        if status.is_success() {
            return Ok(bytes.to_vec());
        }
        // The body is OpenRouter's own account of what was wrong, so it rides along —
        // truncated, because an HTML error page would otherwise be the whole message.
        let detail: String = String::from_utf8_lossy(&bytes).chars().take(300).collect();
        let line = format!("openrouter answered {status}: {detail}");
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(KeyRefused(format!("{line} — check ${ENV_API_KEY}")).into());
        }
        Err(anyhow::anyhow!(line))
    }
}

/// The OpenAI response shape OpenRouter answers in. Unknown fields are ignored, so
/// `usage` and whatever gets added beside it cost nothing here.
#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingRow>,
}

#[derive(Deserialize)]
struct EmbeddingRow {
    index: usize,
    embedding: Vec<f32>,
}

/// Parses a response and holds it to the contract: one vector per input, in input
/// order, at the pinned width, finite, and unit-length.
///
/// Order comes from each row's `index`, not from row order — the shape's own promise
/// of position, and the caller zips the result against chunk hashes, so trusting row
/// order would hand chunk 3 another chunk's vector the day a provider reorders. The
/// same crossed-wire failure [`crate::embed`]'s `seq_id` readback guards against,
/// guarded the same way: by the identifier, never by arrival.
fn parse_response(bytes: &[u8], expected: usize, dims: usize) -> anyhow::Result<Vec<Vec<f32>>> {
    let response: EmbeddingResponse = serde_json::from_slice(bytes).map_err(|e| {
        let head: String = String::from_utf8_lossy(bytes).chars().take(200).collect();
        anyhow::anyhow!("openrouter's response did not parse: {e} — starts `{head}`")
    })?;

    let mut out: Vec<Option<Vec<f32>>> = vec![None; expected];
    for row in response.data {
        anyhow::ensure!(
            row.index < expected,
            "openrouter returned index {} for {expected} inputs",
            row.index
        );
        anyhow::ensure!(
            row.embedding.len() == dims,
            "openrouter returned a {}-dim vector where the registry pins {dims} — \
             the served model does not match the spec",
            row.embedding.len()
        );
        anyhow::ensure!(
            row.embedding.iter().all(|x| x.is_finite()),
            "input {} came back non-finite",
            row.index
        );
        let slot = &mut out[row.index];
        anyhow::ensure!(
            slot.is_none(),
            "openrouter returned index {} twice",
            row.index
        );
        *slot = Some(crate::embed::normalize(&row.embedding));
    }

    out.into_iter()
        .enumerate()
        .map(|(i, v)| {
            v.ok_or_else(|| anyhow::anyhow!("openrouter returned no vector for input {i}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_remote_id_carries_the_prefix_and_is_unique() {
        for spec in REMOTE_REGISTRY {
            assert!(spec.id.starts_with(PREFIX), "{}", spec.id);
            assert!(!spec.slug().is_empty(), "{}", spec.id);
            assert!(spec.dims > 0, "{}", spec.id);
            let n = REMOTE_REGISTRY.iter().filter(|m| m.id == spec.id).count();
            assert_eq!(n, 1, "duplicate id {}", spec.id);
        }
    }

    /// The ids must not collide with the local registry: one id, one backend, or
    /// [`backend_for`] answers differently depending on check order.
    #[test]
    fn remote_ids_shadow_no_local_model() {
        for spec in REMOTE_REGISTRY {
            assert!(models::require(spec.id).is_err(), "{} is both", spec.id);
        }
    }

    #[test]
    fn the_slug_is_the_id_without_the_prefix() {
        let spec = spec("openrouter/qwen/qwen3-embedding-8b").unwrap();
        assert_eq!(spec.slug(), "qwen/qwen3-embedding-8b");
    }

    #[test]
    fn backend_routing_answers_by_id_alone() {
        assert!(matches!(
            backend_for("qwen3-embedding-4b").unwrap(),
            EmbeddingBackend::Local(_)
        ));
        assert!(matches!(
            backend_for("openrouter/qwen/qwen3-embedding-8b").unwrap(),
            EmbeddingBackend::Remote(_)
        ));
    }

    /// A misspelled remote id gets the remote list, not the local one — the person who
    /// typed the prefix has already said which registry they meant.
    #[test]
    fn an_unknown_remote_id_names_the_remote_models() {
        let err = backend_for("openrouter/qwen/qwen3-embedding-9b")
            .unwrap_err()
            .to_string();
        assert!(err.contains("openrouter/qwen/qwen3-embedding-8b"), "{err}");
        assert!(!err.contains("centinel models pull"), "{err}");
    }

    #[test]
    fn a_missing_key_names_the_variable() {
        // The variable may be set on a dev machine; the guard is against reading it.
        let spec = spec("openrouter/qwen/qwen3-embedding-8b").unwrap();
        if std::env::var(ENV_API_KEY).is_ok() {
            return;
        }
        let err = RemoteEmbedder::new(spec).unwrap_err().to_string();
        assert!(err.contains(ENV_API_KEY), "{err}");
    }

    fn body(rows: &[(usize, Vec<f32>)]) -> Vec<u8> {
        let data: Vec<_> = rows
            .iter()
            .map(|(i, v)| serde_json::json!({"index": i, "embedding": v}))
            .collect();
        serde_json::to_vec(&serde_json::json!({"data": data, "usage": {"total_tokens": 7}}))
            .unwrap()
    }

    /// Rows come back keyed by `index`, and the contract is input order — so a
    /// reordered response must land straight, not crossed.
    #[test]
    fn vectors_are_returned_in_input_order_whatever_the_row_order() {
        let bytes = body(&[(1, vec![0.0, 1.0]), (0, vec![1.0, 0.0])]);
        let out = parse_response(&bytes, 2, 2).unwrap();
        assert_eq!(out[0], vec![1.0, 0.0]);
        assert_eq!(out[1], vec![0.0, 1.0]);
    }

    #[test]
    fn vectors_are_normalized_here_not_trusted() {
        let bytes = body(&[(0, vec![3.0, 4.0])]);
        let out = parse_response(&bytes, 1, 2).unwrap();
        assert!((out[0][0] - 0.6).abs() < 1e-6);
        assert!((out[0][1] - 0.8).abs() < 1e-6);
    }

    /// The pin is the guard against a provider quietly serving a different model:
    /// the wrong width is refused before anything is written.
    #[test]
    fn a_wrong_width_is_refused() {
        let bytes = body(&[(0, vec![1.0, 0.0, 0.0])]);
        let err = parse_response(&bytes, 1, 2).unwrap_err().to_string();
        assert!(err.contains("pins 2"), "{err}");
    }

    #[test]
    fn a_missing_or_duplicate_row_is_refused() {
        let missing = body(&[(0, vec![1.0, 0.0])]);
        assert!(parse_response(&missing, 2, 2).is_err(), "one of two absent");

        let twice = body(&[(0, vec![1.0, 0.0]), (0, vec![0.0, 1.0])]);
        assert!(parse_response(&twice, 2, 2).is_err(), "index 0 twice");

        let stray = body(&[(5, vec![1.0, 0.0])]);
        assert!(parse_response(&stray, 1, 2).is_err(), "index out of range");
    }

    #[test]
    fn a_non_finite_vector_is_refused() {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "data": [{"index": 0, "embedding": [1.0, f64::NAN]}]
        }));
        // JSON has no NaN; a provider would have to send `null`, which fails the
        // f32 parse — either way it cannot reach the table. Assert on the parse.
        assert!(bytes.is_err() || parse_response(&bytes.unwrap(), 1, 2).is_err());
    }

    #[test]
    fn only_a_refused_key_is_fatal() {
        let fatal: anyhow::Error = KeyRefused("401".into()).into();
        assert!(is_fatal(&fatal));
        assert!(is_fatal(&fatal.context("wrapped")));
        assert!(!is_fatal(&anyhow::anyhow!("openrouter answered 429")));
    }
}
