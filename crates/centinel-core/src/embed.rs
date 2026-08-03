//! Embedding — turning chunk text into vectors, locally.
//!
//! `llama.cpp` in-process via [`llama_cpp_2`]. No server, no sidecar, no second language
//! runtime: a model is a file on disk that this process loads (SPEC §2.1, §3.1).
//!
//! ## Why GGUF and not ONNX
//!
//! Measured, in SPEC §6.2.1. The `onnx-community` exports are decoder graphs carrying a
//! KV cache, and CoreML refuses tensors with zero elements — which is exactly what an
//! empty cache is — so ONNX on Apple Silicon is permanently CPU-bound. `llama.cpp` has
//! first-class Metal and CUDA backends, and GGUF is the format quantization ladders are
//! actually published in.
//!
//! ## The recipe is not obvious and getting it wrong fails silently
//!
//! Qwen3-Embedding needs three things that a generic embedding wrapper would not do:
//!
//! 1. **Last-token pooling**, not mean pooling ([`LlamaPoolingType::Last`]).
//! 2. **An instruction prefix on queries only.** Documents are embedded bare. The
//!    asymmetry is the model's, not ours — see [`QUERY_INSTRUCTION`].
//! 3. **L2 normalization**, so cosine similarity is a dot product.
//!
//! Each of these produces *plausible* vectors when wrong — slightly worse retrieval, no
//! error anywhere. Hence [`tests::the_recipe_separates_a_relevant_document_from_an_irrelevant_one`],
//! which asserts on semantics rather than on shapes.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use llama_cpp_2::context::params::{LlamaContextParams, LlamaPoolingType};
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};

use crate::models::{self, ModelRole, ModelSpec};

/// The task description prepended to **queries**.
///
/// Qwen3-Embedding is instruction-aware and asymmetric: a query is wrapped as
/// `Instruct: {task}\nQuery:{q}`, a document is embedded as-is. Embedding documents with
/// the prefix too would put both on the same side of a relationship the model was
/// trained to treat as directional.
pub const QUERY_INSTRUCTION: &str =
    "Given a web search query, retrieve relevant passages that answer the query";

/// Context window. Chunks target 1,200 characters (~300 tokens), so this is generous
/// while staying far below the model's 32K — a full-size context would allocate a KV
/// cache far larger than any chunk needs.
const DEFAULT_CONTEXT_TOKENS: u32 = 4096;

/// Offload everything the backend will take. Ignored on a CPU-only build.
const GPU_LAYERS: u32 = 1000;

/// `llama.cpp`'s backend is global process state and must be initialised exactly once.
///
/// Its logging is routed into `tracing` and **off by default**. Left alone, `llama.cpp`
/// writes graph and buffer diagnostics straight to stderr — which would corrupt nothing
/// under `centinel mcp` (that protocol owns stdout) but would bury every progress bar
/// and every op's output under hundreds of lines. `--verbose` is the way to see it.
fn backend() -> anyhow::Result<&'static LlamaBackend> {
    static BACKEND: OnceLock<Result<LlamaBackend, String>> = OnceLock::new();
    BACKEND
        .get_or_init(|| {
            llama_cpp_2::send_logs_to_tracing(llama_cpp_2::LogOptions::default());
            LlamaBackend::init().map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| anyhow::anyhow!("initialising llama.cpp: {e}"))
}

/// A loaded embedding model.
///
/// Loading is the expensive part, so this is built once and reused. A short-lived CLI
/// invocation pays it per run; a long-lived `serve`/`mcp` process pays it once.
pub struct Embedder {
    model: LlamaModel,
    spec: &'static ModelSpec,
    variant: &'static str,
    context_tokens: u32,
}

impl std::fmt::Debug for Embedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Embedder")
            .field("model", &self.spec.id)
            .field("variant", &self.variant)
            .field("dims", &self.dims())
            .finish()
    }
}

impl Embedder {
    /// Loads an embedder from the weights cache.
    ///
    /// Fails loudly when weights are absent rather than downloading them — SPEC §3.2
    /// makes fetching an explicit operator action, so that a scheduled run can fail on a
    /// missing model but never decide to pull gigabytes on its own.
    pub fn load(root: &Path, model_id: &str, variant: Option<&str>) -> anyhow::Result<Self> {
        let spec = models::require(model_id)?;
        anyhow::ensure!(
            spec.role == ModelRole::Embedding,
            "`{model_id}` is a {} model, not an embedder",
            spec.role
        );
        let variant = spec.variant(variant)?;
        let path = Self::weights_path(root, spec, variant.name)?;

        let params = LlamaModelParams::default().with_n_gpu_layers(GPU_LAYERS);
        let model = LlamaModel::load_from_file(backend()?, &path, &params)
            .map_err(|e| anyhow::anyhow!("loading {}: {e}", path.display()))?;

        Ok(Self {
            model,
            spec,
            variant: variant.name,
            context_tokens: DEFAULT_CONTEXT_TOKENS,
        })
    }

    /// Locates a variant's single GGUF file, with an actionable error when it is absent.
    fn weights_path(
        root: &Path,
        spec: &'static ModelSpec,
        variant: &str,
    ) -> anyhow::Result<PathBuf> {
        let v = spec.variant(Some(variant))?;
        let file = v
            .files
            .first()
            .ok_or_else(|| anyhow::anyhow!("{}/{variant} declares no files", spec.id))?;
        let path = spec.dir(root).join(file.path);
        anyhow::ensure!(
            path.is_file(),
            "weights missing: {}\n  run `centinel models pull {} --variant {variant}`",
            path.display(),
            spec.id
        );
        Ok(path)
    }

    pub fn model_id(&self) -> &'static str {
        self.spec.id
    }

    pub fn variant(&self) -> &'static str {
        self.variant
    }

    /// Output width. Part of §5.2's cache key `(chunk_hash, model_id, dims)`.
    pub fn dims(&self) -> usize {
        self.model.n_embd() as usize
    }

    /// Embeds a query, applying the instruction prefix.
    pub fn embed_query(&self, query: &str) -> anyhow::Result<Vec<f32>> {
        let prompt = format!("Instruct: {QUERY_INSTRUCTION}\nQuery:{query}");
        Ok(self.embed_batch(&[prompt])?.remove(0))
    }

    /// Embeds documents, bare. Order is preserved.
    pub fn embed_documents<S: AsRef<str>>(&self, texts: &[S]) -> anyhow::Result<Vec<Vec<f32>>> {
        let owned: Vec<String> = texts.iter().map(|t| t.as_ref().to_string()).collect();
        self.embed_batch(&owned)
    }

    /// The single inference path. Both public entry points route through here so a query
    /// and a document can never be embedded by subtly different code.
    fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let tokenized: Vec<Vec<_>> = texts
            .iter()
            .map(|t| {
                self.model
                    .str_to_token(t, AddBos::Always)
                    .map_err(|e| anyhow::anyhow!("tokenizing: {e}"))
            })
            .collect::<anyhow::Result<_>>()?;

        // Refused rather than truncated. A silently shortened chunk would be indexed
        // under a `chunk_hash` covering text that was never embedded, which makes the
        // cache lie about what it holds.
        if let Some(long) = tokenized
            .iter()
            .find(|t| t.len() > self.context_tokens as usize)
        {
            anyhow::bail!(
                "a text tokenizes to {} tokens, over the {} context. Chunk it smaller.",
                long.len(),
                self.context_tokens
            );
        }

        let params = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZeroU32::new(self.context_tokens))
            .with_n_batch(self.context_tokens)
            // Last-token pooling. Set explicitly rather than trusting GGUF metadata,
            // because the wrong pooling yields usable-looking vectors and no error.
            .with_pooling_type(LlamaPoolingType::Last)
            .with_embeddings(true);

        let mut ctx = self
            .model
            .new_context(backend()?, params)
            .map_err(|e| anyhow::anyhow!("creating context: {e}"))?;

        let mut out = Vec::with_capacity(texts.len());
        for tokens in &tokenized {
            let mut batch = LlamaBatch::new(self.context_tokens as usize, 1);
            batch
                .add_sequence(tokens, 0, false)
                .map_err(|e| anyhow::anyhow!("building batch: {e}"))?;

            ctx.clear_kv_cache();
            ctx.decode(&mut batch)
                .map_err(|e| anyhow::anyhow!("decode failed: {e}"))?;

            let raw = ctx
                .embeddings_seq_ith(0)
                .map_err(|e| anyhow::anyhow!("reading embeddings: {e}"))?;
            out.push(normalize(raw));
        }

        Ok(out)
    }
}

/// L2 normalization, so cosine similarity is a plain dot product.
fn normalize(v: &[f32]) -> Vec<f32> {
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

    /// Loading weights is expensive and they may not be present, so the inference tests
    /// are opt-in: `CENTINEL_TEST_MODELS=1 cargo test`.
    fn embedder() -> Option<Embedder> {
        if std::env::var("CENTINEL_TEST_MODELS").is_err() {
            return None;
        }
        let root = models::models_dir().ok()?;
        Embedder::load(&root, "qwen3-embedding-4b", None).ok()
    }

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

    #[test]
    fn loading_a_reranker_as_an_embedder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let err = Embedder::load(dir.path(), "qwen3-reranker-0.6b", None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not an embedder"), "{err}");
    }

    /// SPEC §3.2: missing weights are fatal like a missing binary, and the error names
    /// the command that fixes it.
    #[test]
    fn missing_weights_name_the_pull_command() {
        let dir = tempfile::tempdir().unwrap();
        let err = Embedder::load(dir.path(), "qwen3-embedding-4b", None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("weights missing"), "{err}");
        assert!(
            err.contains("centinel models pull qwen3-embedding-4b"),
            "{err}"
        );
    }

    #[test]
    fn an_unknown_variant_is_refused_before_touching_disk() {
        let dir = tempfile::tempdir().unwrap();
        let err = Embedder::load(dir.path(), "qwen3-embedding-4b", Some("q2_k"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("q8_0"), "should list real variants: {err}");
    }

    /// The test that would catch a wrong recipe.
    ///
    /// Mean pooling instead of last-token, or the instruction prefix on documents, both
    /// produce vectors of the right shape and unit norm — nothing errors. What breaks is
    /// *meaning*, so this asserts on meaning: a query about lobbying spend must land
    /// closer to the lobbying document than to the one about bin collection.
    #[test]
    fn the_recipe_separates_a_relevant_document_from_an_irrelevant_one() {
        let Some(embedder) = embedder() else {
            eprintln!("skipping: set CENTINEL_TEST_MODELS=1 with weights pulled");
            return;
        };

        let query = embedder
            .embed_query("how much did the city spend on lobbying")
            .unwrap();
        let docs = embedder
            .embed_documents(&[
                "The registered lobbyist meeting log for the fourth quarter reports \
                 expenditures of $48,000 on outside government-relations counsel.",
                "Solid waste collection occurs weekly on Tuesdays. Place bins at the \
                 curb by 6am.",
            ])
            .unwrap();

        let relevant = cosine(&query, &docs[0]);
        let irrelevant = cosine(&query, &docs[1]);
        assert!(
            relevant > irrelevant + 0.15,
            "the recipe is wrong: relevant={relevant:.4} irrelevant={irrelevant:.4}"
        );
        assert_eq!(query.len(), embedder.dims());
    }

    /// The embedding cache is keyed by `chunk_hash`, which is only sound if the same
    /// text always yields the same vector.
    #[test]
    fn embedding_is_deterministic() {
        let Some(embedder) = embedder() else {
            eprintln!("skipping: set CENTINEL_TEST_MODELS=1 with weights pulled");
            return;
        };
        let text = ["The Board approved the stormwater appropriation."];
        let a = embedder.embed_documents(&text).unwrap();
        let b = embedder.embed_documents(&text).unwrap();
        assert_eq!(a, b, "a cache keyed by chunk_hash requires this");
    }

    #[test]
    fn an_empty_batch_is_not_an_error() {
        let Some(embedder) = embedder() else { return };
        assert!(embedder.embed_documents::<&str>(&[]).unwrap().is_empty());
    }
}
