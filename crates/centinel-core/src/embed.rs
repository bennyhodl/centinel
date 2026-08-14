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

use std::path::Path;
use std::sync::OnceLock;

use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::context::params::{LlamaContextParams, LlamaPoolingType};
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::token::LlamaToken;
use llama_cpp_2::{LlamaBackendDeviceType, list_llama_ggml_backend_devices};

use crate::models::{self, ModelRole, ModelSpec};

/// The task description prepended to **queries**.
///
/// Qwen3-Embedding is instruction-aware and asymmetric: a query is wrapped as
/// `Instruct: {task}\nQuery:{q}`, a document is embedded as-is. Embedding documents with
/// the prefix too would put both on the same side of a relationship the model was
/// trained to treat as directional.
pub const QUERY_INSTRUCTION: &str =
    "Given a web search query, retrieve relevant passages that answer the query";

/// The longest a single text may tokenize to.
///
/// A validation bound, not a context size — the context is sized to the group being
/// embedded (see [`Embedder::decode_group`]). Chunks target 1,200 characters (~300
/// tokens), so this is generous while staying far below the model's 32K.
const MAX_CHUNK_TOKENS: u32 = 4096;

/// Cells llama.cpp rounds a per-sequence context up to (`GGML_PAD(n_ctx_seq, 256)` in
/// `llama-context.cpp`). Stated here so the arithmetic below asks for what it will be
/// charged: a smaller request is rounded up and paid for anyway.
const CTX_PAD: u32 = 256;

/// llama.cpp's `LLAMA_MAX_SEQ`. A batch wider than this is refused by
/// `llama_batch_allocr::init`, so a longer list of texts is split rather than passed on.
const MAX_SEQUENCES: usize = 256;

/// Cells one sequence reserves in a session's standing context, and what one sequence in
/// an auto-sized batch is assumed to cost.
///
/// A standing context has to be sized before any text is tokenized, so this stands in
/// for the real length: chunks target 1,200 characters (~300 tokens) and [`CTX_PAD`]
/// rounds that up, which is what a typical chunk actually costs. The rare text that
/// tokenizes past it rides through a bespoke context instead ([`EmbedSession::embed`]).
const NOMINAL_SEQ_CELLS: u64 = 512;

/// Tokens one *physical* pass may carry (`n_ubatch`).
///
/// A batch is one `decode` call, but llama.cpp runs the call as physical passes of at
/// most this many tokens, and the pass — not the call — is what sizes the attention
/// buffers: the mask alone is `n_ubatch²` half-floats. Uncapped, the whole group was one
/// graph, and that shape is off every backend's tested path — measured here as NaN
/// vectors at ~12,000 tokens and a Metal segfault at ~38,000. 2,048 is a width
/// llama.cpp's own tooling exercises constantly, so it is a width the kernels can be
/// trusted at; passes queue back to back inside the call, so the cap costs no idle time.
const UBATCH_TOKENS: u32 = 2048;

/// The share of free device memory an auto-sized batch may plan on.
///
/// Half, because the KV cache is not all a decode allocates — the graph's compute buffers
/// and the attention mask come out of the same pool — and because the CPU backend answers
/// the free-memory question with the machine's *whole* RAM ("free is ill-defined, assume
/// all of it is free" — `ggml-cpu.cpp`), not the part nothing else wants.
const MEMORY_FRACTION: f64 = 0.5;

/// Ceiling on an auto-sized batch.
///
/// Well under llama.cpp's 256-sequence limit, because the gain flattens once the GPU is
/// saturated and the cost of a failure does not: `ops::embed` retries a failed group one
/// chunk at a time, so a wide group turns one bad chunk into a long serial retry.
const AUTO_MAX_BATCH: usize = 128;

/// Offload everything the backend will take. Ignored on a CPU-only build.
const GPU_LAYERS: u32 = 1000;

/// Whether a context may use flash attention.
///
/// `Auto` is llama.cpp's shipped default: on where the backend supports the shape, off
/// where it does not. A parameter rather than a constant because it is the one
/// throughput lever that changes the arithmetic *route* — the same attention computed
/// tile-wise, bit-different in the last places. `embed_bench` sweeps it against a parity
/// check; a machine where it fails parity runs `Disabled` and loses speed, not quality.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlashAttention {
    #[default]
    Auto,
    Disabled,
    Enabled,
}

impl FlashAttention {
    /// The value llama.cpp's `llama_flash_attn_type` gives this policy.
    fn policy(self) -> i32 {
        match self {
            Self::Auto => -1,
            Self::Disabled => 0,
            Self::Enabled => 1,
        }
    }
}

/// The shape of an [`EmbedSession`]. Every field has a default a bench has not vetoed.
#[derive(Clone, Copy, Debug)]
pub struct SessionOptions {
    /// Sequences the standing context holds — the embedding batch.
    pub batch: usize,
    /// Tokens one physical pass carries. [`UBATCH_TOKENS`] unless a bench said otherwise.
    pub ubatch: u32,
    /// See [`FlashAttention`].
    pub flash: FlashAttention,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            // The measured fallback `ops::embed` also uses when the machine will not
            // say what it can hold.
            batch: 32,
            ubatch: UBATCH_TOKENS,
            flash: FlashAttention::default(),
        }
    }
}

/// `llama.cpp`'s backend is global process state and must be initialised exactly once.
///
/// Its logging is routed into `tracing` and **off by default**. Left alone, `llama.cpp`
/// writes graph and buffer diagnostics straight to stderr — which would corrupt nothing
/// under `centinel mcp` (that protocol owns stdout) but would bury every progress bar
/// and every op's output under hundreds of lines. `--verbose` is the way to see it.
pub(crate) fn backend() -> anyhow::Result<&'static LlamaBackend> {
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
        // Through `models::resolve`, which checks each file against its *pinned size*.
        // This used to test `path.is_file()`, so a truncated download read as installed
        // here and as missing to `doctor`, and the load failed somewhere inside
        // llama.cpp rather than naming the model to pull.
        let found = models::resolve(model_id, ModelRole::Embedding, variant, root)?;

        let params = LlamaModelParams::default().with_n_gpu_layers(GPU_LAYERS);
        let model = LlamaModel::load_from_file(backend()?, &found.path, &params)
            .map_err(|e| anyhow::anyhow!("loading {}: {e}", found.path.display()))?;

        Ok(Self {
            model,
            spec: found.spec,
            variant: found
                .spec
                .variant(Some(&found.variant))
                .expect("a resolved variant is a spec variant")
                .name,
        })
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
    ///
    /// Split at [`MAX_SEQUENCES`] rather than refused above it, so the public contract
    /// stays "any number of texts, order preserved" whatever llama.cpp's batch limit is.
    fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let tokenized = self.tokenize(texts)?;
        let mut out = Vec::with_capacity(texts.len());
        for group in tokenized.chunks(MAX_SEQUENCES) {
            out.extend(self.decode_group(group, FlashAttention::default())?);
        }
        Ok(out)
    }

    /// Tokenizes every text, refusing any that runs too long.
    ///
    /// Refused rather than truncated. A silently shortened chunk would be indexed
    /// under a `chunk_hash` covering text that was never embedded, which makes the
    /// cache lie about what it holds.
    fn tokenize<S: AsRef<str>>(&self, texts: &[S]) -> anyhow::Result<Vec<Vec<LlamaToken>>> {
        let tokenized: Vec<Vec<LlamaToken>> = texts
            .iter()
            .map(|t| {
                self.model
                    .str_to_token(t.as_ref(), AddBos::Always)
                    .map_err(|e| anyhow::anyhow!("tokenizing: {e}"))
            })
            .collect::<anyhow::Result<_>>()?;

        let longest = tokenized.iter().map(Vec::len).max().unwrap_or(0);
        anyhow::ensure!(
            longest <= MAX_CHUNK_TOKENS as usize,
            "a text tokenizes to {longest} tokens, over the {MAX_CHUNK_TOKENS} a chunk \
             may be. Chunk it smaller."
        );
        Ok(tokenized)
    }

    /// One context, one batch, one `decode` call — the group is the unit of inference.
    ///
    /// Each text enters as its own sequence and its vector is read back by `seq_id`, so
    /// a group of 32 is one call rather than 32 — and the calls this replaced were
    /// mostly a GPU waiting for the next lone ~300-token chunk. Inside the call,
    /// llama.cpp runs whole sequences in physical passes of at most [`UBATCH_TOKENS`]
    /// tokens, back to back; see that constant for why the pass is capped and the call
    /// is not. One text longer than the cap raises it, because a pooled sequence cannot
    /// split across passes.
    ///
    /// The context is sized to the group in hand. llama.cpp gives every sequence its own
    /// KV stream of `n_ctx / n_seq_max` cells rounded up to [`CTX_PAD`], so `n_ctx` is
    /// stated as that product and the per-sequence share follows the longest text. A
    /// fixed [`MAX_CHUNK_TOKENS`] would make every sequence reserve eight times what a
    /// chunk uses — for a group of 128, 77 GB of KV cache that nothing reads.
    ///
    /// One long text therefore inflates its whole group. That is affordable because
    /// chunks are cut to a target length ([`crate::chunk`]) and so arrive near-uniform,
    /// and because a group too large for the machine fails at `new_context` or `decode`
    /// and is retried a chunk at a time by [`crate::ops::embed`] — the same recovery as
    /// before, now reached by a group that will not fit as well as by a chunk that will
    /// not tokenize.
    fn decode_group(
        &self,
        group: &[Vec<LlamaToken>],
        flash: FlashAttention,
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        let sequences = group.len() as u32;
        let tokens: usize = group.iter().map(Vec::len).sum();
        let longest = group.iter().map(Vec::len).max().unwrap_or(0).max(1) as u32;
        let per_sequence = longest.div_ceil(CTX_PAD) * CTX_PAD;

        let params = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZeroU32::new(per_sequence * sequences))
            // `n_batch` bounds the call and stays the whole group; `n_ubatch` bounds the
            // physical pass and does not. At their defaults llama.cpp would cap the
            // *call* at 2,048 tokens and refuse the group outright.
            .with_n_batch(tokens as u32)
            .with_n_ubatch(UBATCH_TOKENS.max(longest))
            .with_n_seq_max(sequences)
            // Last-token pooling. Set explicitly rather than trusting GGUF metadata,
            // because the wrong pooling yields usable-looking vectors and no error.
            .with_pooling_type(LlamaPoolingType::Last)
            .with_embeddings(true)
            .with_flash_attention_policy(flash.policy());

        let mut ctx = self
            .model
            .new_context(backend()?, params)
            .map_err(|e| anyhow::anyhow!("creating context: {e}"))?;
        decode_into(&mut ctx, group)
    }

    /// A batch size for this machine, or `None` where no backend device will say how much
    /// memory it has.
    ///
    /// What a batch costs is KV cache: each sequence reserves its own stream of context
    /// cells, and a cell holds K and V for every layer. Both halves are numbers the loaded
    /// model states exactly, so this is arithmetic rather than a table of machines.
    ///
    /// Asked *after* the weights are loaded, because a GPU's free figure then already has
    /// them subtracted — Metal reports `recommendedMaxWorkingSetSize −
    /// currentAllocatedSize`, CUDA reports `cudaMemGetInfo` — so nothing here needs the
    /// model's size on disk or a guess at what the backend did with it.
    pub fn auto_batch(&self) -> Option<usize> {
        Some(batch_for_budget(
            free_device_memory()?,
            self.kv_bytes_per_cell(),
        ))
    }

    /// Bytes of KV cache one context cell costs.
    fn kv_bytes_per_cell(&self) -> u64 {
        let heads = u64::from(self.model.n_head().max(1));
        let head_width = self.model.n_embd().max(0) as u64 / heads;
        // K and V, two bytes apiece — the cache defaults to f16.
        u64::from(self.model.n_layer()) * u64::from(self.model.n_head_kv()) * head_width * 2 * 2
    }

    /// A standing context, sized for `options.batch` sequences of ordinary chunks.
    ///
    /// [`Self::embed_documents`] builds a context per call, which is right for a query
    /// and wrong for a corpus: a million-chunk run would pay the same KV allocation
    /// tens of thousands of times for identically shaped groups. A session allocates
    /// once and clears cache *metadata* between decodes — the buffers stay put.
    pub fn session(&self, options: SessionOptions) -> anyhow::Result<EmbedSession<'_>> {
        let seqs = options.batch.clamp(1, MAX_SEQUENCES);
        let cells = NOMINAL_SEQ_CELLS as u32;
        let params = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZeroU32::new(cells * seqs as u32))
            .with_n_batch(cells * seqs as u32)
            // The pass must at least hold the longest sequence the standing shape
            // admits, however small a width the options ask for.
            .with_n_ubatch(options.ubatch.max(cells))
            .with_n_seq_max(seqs as u32)
            .with_pooling_type(LlamaPoolingType::Last)
            .with_embeddings(true)
            .with_flash_attention_policy(options.flash.policy());

        let ctx = self
            .model
            .new_context(backend()?, params)
            .map_err(|e| anyhow::anyhow!("creating session context: {e}"))?;
        Ok(EmbedSession {
            embedder: self,
            ctx,
            seqs,
            flash: options.flash,
        })
    }
}

/// A reusable inference context — [`Embedder::embed_documents`] minus the per-call
/// allocation. The throughput path; `ops::embed` drives a corpus through one of these.
///
/// A group that does not fit the standing shape — any text longer than
/// [`NOMINAL_SEQ_CELLS`] — falls back to a bespoke context for that group alone, the
/// same shape `embed_documents` builds. With the work list sorted shortest-first
/// (`Index::chunk_hashes_by_length`) such groups cluster at the tail of a run instead
/// of surfacing in the middle of every group they would otherwise visit.
pub struct EmbedSession<'m> {
    embedder: &'m Embedder,
    ctx: LlamaContext<'m>,
    seqs: usize,
    flash: FlashAttention,
}

impl EmbedSession<'_> {
    /// Embeds documents, bare, order preserved — the session-shaped
    /// [`Embedder::embed_documents`].
    pub fn embed<S: AsRef<str>>(&mut self, texts: &[S]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let tokenized = self.embedder.tokenize(texts)?;
        let mut out = Vec::with_capacity(texts.len());
        for group in tokenized.chunks(self.seqs) {
            let longest = group.iter().map(Vec::len).max().unwrap_or(0);
            if longest <= NOMINAL_SEQ_CELLS as usize {
                // Metadata only, never a wipe of the buffers themselves — clearing data
                // would memset gigabytes between every pair of batches. Stale cells left
                // behind by a failed clear would enter the next decode as context, so a
                // `false` here is an error, not a shrug.
                let cleared = self
                    .ctx
                    .clear_kv_cache_seq(None, None, None)
                    .map_err(|e| anyhow::anyhow!("clearing the KV cache: {e}"))?;
                anyhow::ensure!(cleared, "the backend refused to clear the KV cache");
                out.extend(decode_into(&mut self.ctx, group)?);
            } else {
                out.extend(self.embedder.decode_group(group, self.flash)?);
            }
        }
        Ok(out)
    }
}

/// Free memory on the device inference will run on, or `None` where nothing reports any.
///
/// GPUs first, because [`GPU_LAYERS`] puts the model on one wherever there is one, and
/// the CPU device would answer with the whole machine's RAM. Zero is read as "will not
/// say" rather than "has none": a backend that does not implement the query reports zero,
/// and so does a card with nothing left — neither is a number to size a run from, and the
/// caller has a measured default to fall back on.
fn free_device_memory() -> Option<u64> {
    use LlamaBackendDeviceType::{Cpu, Gpu, IntegratedGpu};

    let devices = list_llama_ggml_backend_devices();
    let most_free = |kinds: &[LlamaBackendDeviceType]| {
        devices
            .iter()
            .filter(|d| kinds.contains(&d.device_type))
            .map(|d| d.memory_free as u64)
            .max()
    };
    most_free(&[Gpu, IntegratedGpu])
        .or_else(|| most_free(&[Cpu]))
        .filter(|&free| free > 0)
}

/// The batch a memory budget affords. Separate from the device query so the shape of the
/// curve is testable on a machine with no weights and no GPU.
fn batch_for_budget(free_bytes: u64, kv_bytes_per_cell: u64) -> usize {
    let budget = (free_bytes as f64 * MEMORY_FRACTION) as u64;
    let per_sequence = kv_bytes_per_cell.max(1) * NOMINAL_SEQ_CELLS;
    usize::try_from(budget / per_sequence)
        .unwrap_or(AUTO_MAX_BATCH)
        .clamp(1, AUTO_MAX_BATCH)
}

/// Packs a group into one `decode` on `ctx` and reads each sequence's vector back.
///
/// Read back by `seq_id`, which is the order the texts arrived in — the caller zips the
/// result against its chunk hashes, so the order is not cosmetic.
fn decode_into(
    ctx: &mut LlamaContext<'_>,
    group: &[Vec<LlamaToken>],
) -> anyhow::Result<Vec<Vec<f32>>> {
    let tokens: usize = group.iter().map(Vec::len).sum();
    let mut batch = LlamaBatch::new(tokens, group.len() as i32);
    for (seq, text) in group.iter().enumerate() {
        batch
            .add_sequence(text, seq as i32, false)
            .map_err(|e| anyhow::anyhow!("building batch: {e}"))?;
    }
    ctx.decode(&mut batch)
        .map_err(|e| anyhow::anyhow!("decode failed: {e}"))?;

    (0..group.len() as i32)
        .map(|seq| {
            let raw = ctx
                .embeddings_seq_ith(seq)
                .map_err(|e| anyhow::anyhow!("reading embeddings: {e}"))?;
            // A non-finite value is the backend failing numerically, and it is
            // contagious: `normalize` would spread one NaN across the whole vector, and
            // Lance refuses the row — which, before this check, aborted an entire run at
            // the append. Refused here instead, so the caller's one-at-a-time retry
            // isolates the bad sequence and reports it by hash.
            anyhow::ensure!(
                raw.iter().all(|x| x.is_finite()),
                "sequence {seq} came back non-finite — a numerical failure in the backend"
            );
            Ok(normalize(raw))
        })
        .collect()
}

/// L2 normalization, so cosine similarity is a plain dot product.
///
/// `pub(crate)` because [`crate::remote`] holds its vectors to the same contract —
/// one definition of unit-length, wherever the inference ran.
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

    /// `qwen3-embedding-4b`: 36 layers, 8 KV heads, 2,560 wide over 20 heads — 128 per
    /// head. K and V at two bytes each is 147,456 bytes a cell, which is what makes a
    /// batch expensive enough to be worth sizing.
    const QWEN3_4B_KV_CELL: u64 = 147_456;

    /// The point of `auto`: a machine with memory to spare uses it, and one without does
    /// not pretend to.
    #[test]
    fn an_auto_batch_follows_the_memory_it_is_given() {
        let for_gib = |gib: u64| batch_for_budget(gib << 30, QWEN3_4B_KV_CELL);
        assert_eq!(for_gib(4), 28);
        assert_eq!(for_gib(16), 113);
        assert_eq!(
            for_gib(128),
            AUTO_MAX_BATCH,
            "a big machine stops at the ceiling, not at what it could hold"
        );
    }

    /// A batch of zero embeds nothing and would loop forever asking for it.
    #[test]
    fn an_auto_batch_never_reaches_zero() {
        assert_eq!(batch_for_budget(0, QWEN3_4B_KV_CELL), 1);
        assert_eq!(batch_for_budget(1 << 20, QWEN3_4B_KV_CELL), 1);
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

    /// The failure a packed batch makes possible: every text now shares one `decode`, and
    /// a crossed `seq_id` would hand chunk 3 the vector for chunk 1 — right shape, right
    /// norm, wrong document, and nothing in the run would say so. So each text is embedded
    /// in a group and again alone, and the two must be the same vector.
    ///
    /// Not `assert_eq`: a group and a lone text take different paths through the graph, so
    /// the last bits of the floats differ. That is harmless — a chunk is embedded once and
    /// stored under its hash, never compared against a second embedding of itself.
    #[test]
    fn a_group_gives_each_text_its_own_vector() {
        let Some(embedder) = embedder() else {
            eprintln!("skipping: set CENTINEL_TEST_MODELS=1 with weights pulled");
            return;
        };

        let texts = [
            "The Board approved the stormwater appropriation.",
            "Solid waste collection occurs weekly on Tuesdays.",
            "The lobbyist meeting log reports $48,000 in expenditures.",
        ];
        let grouped = embedder.embed_documents(&texts).unwrap();
        assert_eq!(
            grouped.len(),
            texts.len(),
            "order and count are the contract"
        );

        for (i, text) in texts.iter().enumerate() {
            let alone = embedder.embed_documents(&[*text]).unwrap().remove(0);
            let same = cosine(&grouped[i], &alone);
            assert!(
                same > 0.999,
                "text {i} came back as another sequence: {same}"
            );
        }
    }

    /// The session is the corpus path and `embed_documents` the query path, and a vector
    /// must not depend on which one produced it — a corpus embedded by one and searched
    /// through the other is the ordinary case, not a special one.
    ///
    /// The second call through the same session is the part that would catch a broken
    /// KV clear: stale cells from call one entering call two as context would move the
    /// vectors, loudly, here.
    #[test]
    fn a_session_matches_the_plain_path_and_survives_reuse() {
        let Some(embedder) = embedder() else {
            eprintln!("skipping: set CENTINEL_TEST_MODELS=1 with weights pulled");
            return;
        };

        let texts = [
            "The Board approved the stormwater appropriation.",
            "Solid waste collection occurs weekly on Tuesdays.",
            "The lobbyist meeting log reports $48,000 in expenditures.",
        ];
        let plain = embedder.embed_documents(&texts).unwrap();

        // A batch of 2 over 3 texts forces the session to split — both the full and the
        // ragged group go through the standing context.
        let mut session = embedder
            .session(SessionOptions {
                batch: 2,
                ..SessionOptions::default()
            })
            .unwrap();

        let first = session.embed(&texts).unwrap();
        let second = session.embed(&texts).unwrap();
        for i in 0..texts.len() {
            let parity = cosine(&first[i], &plain[i]);
            assert!(
                parity > 0.999,
                "text {i} differs from the plain path: {parity}"
            );
            let reuse = cosine(&first[i], &second[i]);
            assert!(reuse > 0.999, "text {i} moved on a reused context: {reuse}");
        }
    }
}
