//! `embed` — turn indexed chunks into vectors.
//!
//! The expensive stage, and the one the whole caching design exists for. Separate from
//! `index` for the same reason `collect` is separate from `extract`: `index` is minutes
//! and rebuildable, this is hours and worth keeping.
//!
//! ## Resumability is a consequence, not a feature
//!
//! There is no checkpoint file. The work list is
//!
//! ```text
//!   chunk hashes in the index  −  chunk hashes already in the cache
//! ```
//!
//! so killing a run at chunk 40,000 and re-running starts at 40,001. That falls out of
//! the cache being append-only and content-addressed, exactly as `collect`'s
//! resumability falls out of the log. It is also why a **monthly recrawl is cheap**: a
//! re-crawled site is ~95% identical, identical text has an identical `chunk_hash`, and
//! only genuinely new chunks reach the model (SPEC §6.1).
//!
//! ## Batching is not optional
//!
//! A `llama.cpp` context — and its KV cache — is built per call, not per text. Measured
//! on an M1 Max: one chunk per call gives 6.1 chunks/sec, batches of 32 give 18.5. A
//! naive loop would spend two thirds of a multi-hour run on allocation, so the batch is
//! the unit of work here rather than the chunk.

use std::time::Instant;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::embed::Embedder;
use crate::index::Index;
use crate::models;
use crate::prelude::*;
use crate::vectors::VectorCache;

/// Large enough to amortise context creation, small enough that a batch of long chunks
/// does not blow past the embedder's context window in aggregate.
const DEFAULT_BATCH: usize = 32;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct EmbedArgs {
    /// Embedding model. Changing this is a full re-embed into a separate cache, not an
    /// upgrade — vectors from two models share no space (SPEC §6.2).
    #[arg(long, default_value = "qwen3-embedding-4b")]
    #[serde(default = "default_model")]
    pub model: String,

    /// Quantization. Defaults to the registry's choice for the model.
    #[arg(long)]
    #[serde(default)]
    pub variant: Option<String>,

    /// Chunks per forward pass.
    #[arg(long, default_value_t = DEFAULT_BATCH)]
    #[serde(default = "default_batch")]
    pub batch: usize,

    /// Stop after this many chunks. The way to sample a corpus before committing hours.
    #[arg(long)]
    #[serde(default)]
    pub limit: Option<usize>,

    /// Report what would be embedded and exit.
    #[arg(long)]
    #[serde(default)]
    pub dry_run: bool,
}

fn default_model() -> String {
    "qwen3-embedding-4b".to_string()
}

fn default_batch() -> usize {
    DEFAULT_BATCH
}

/// So [`crate::ops::run`] batches as the CLI does — the measured 6.1 vs 18.5 chunks/sec
/// difference documented above is not something a second default should be able to lose.
impl Default for EmbedArgs {
    fn default() -> Self {
        Self {
            model: default_model(),
            variant: None,
            batch: default_batch(),
            limit: None,
            dry_run: false,
        }
    }
}

/// A chunk that could not be embedded. One failure does not abandon the run.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Skipped {
    pub chunk_hash: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct EmbedReport {
    pub model: String,
    pub variant: String,
    pub dims: usize,
    pub cache: std::path::PathBuf,
    /// Chunks in the index.
    pub indexed: usize,
    /// Already cached when the run started — the resumed-past portion.
    pub already_cached: usize,
    pub embedded: usize,
    /// Still outstanding after this run, from `--limit` or from failures.
    pub remaining: usize,
    pub elapsed_secs: f64,
    /// Sustained rate, for planning the rest of a corpus.
    pub chunks_per_sec: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<Skipped>,
}

/// Embed indexed chunks into the durable vector cache.
#[op(long_running, group = "stage")]
pub async fn embed(ctx: &Ctx, args: EmbedArgs, progress: &Progress) -> anyhow::Result<EmbedReport> {
    anyhow::ensure!(args.batch > 0, "--batch must be at least 1");

    let index = Index::open(ctx.store.require_index()?)?;
    let indexed = index.chunk_hashes()?;

    // Dimensions come from the registry so the cache can be opened — and the outstanding
    // work computed — before a multi-gigabyte model is loaded. A dry run never loads one.
    let spec = models::require(&args.model)?;
    let dims = spec
        .dims
        .ok_or_else(|| anyhow::anyhow!("`{}` is not an embedding model", args.model))?
        as usize;

    let cache = VectorCache::open(&ctx.store.vector_cache_dir(), spec.id, dims)?;
    let cached = cache.hashes()?;
    let already_cached = indexed.iter().filter(|h| cached.contains(*h)).count();

    let mut todo: Vec<String> = indexed
        .iter()
        .filter(|h| !cached.contains(*h))
        .cloned()
        .collect();
    let outstanding = todo.len();
    if let Some(limit) = args.limit {
        todo.truncate(limit);
    }

    let base = EmbedReport {
        model: spec.id.to_string(),
        variant: args
            .variant
            .clone()
            .unwrap_or_else(|| spec.default_variant.to_string()),
        dims,
        cache: cache.path().to_path_buf(),
        indexed: indexed.len(),
        already_cached,
        embedded: 0,
        remaining: outstanding,
        elapsed_secs: 0.0,
        chunks_per_sec: 0.0,
        skipped: Vec::new(),
    };

    if args.dry_run || todo.is_empty() {
        if todo.is_empty() {
            progress.say("nothing to embed — every indexed chunk is already cached");
        }
        return Ok(base);
    }

    progress.say(format!(
        "loading {} ({})",
        spec.id,
        args.variant.as_deref().unwrap_or(spec.default_variant)
    ));
    let embedder = load_embedder(&args, spec.id).await?;
    anyhow::ensure!(
        embedder.dims() == dims,
        "{} reports {} dimensions, the registry pins {dims}",
        spec.id,
        embedder.dims()
    );

    // The whole run goes into one blocking task rather than one per batch. Inference is
    // CPU/GPU-bound and would stall the async runtime — which matters here more than
    // usual, because an HTTP caller's connection has to stay responsive across hours.
    // One task also means the model is moved once instead of round-tripping per batch.
    let started = Instant::now();
    let (embedded, skipped) = {
        let batch_size = args.batch;
        let cache = cache.clone();
        let progress = progress.clone();
        tokio::task::spawn_blocking(move || run(embedder, index, cache, todo, batch_size, progress))
            .await??
    };

    let elapsed = started.elapsed().as_secs_f64();
    Ok(EmbedReport {
        embedded,
        remaining: outstanding - embedded,
        elapsed_secs: elapsed,
        chunks_per_sec: embedded as f64 / elapsed.max(f64::EPSILON),
        skipped,
        ..base
    })
}

/// The blocking loop: batch → embed → append, until the work list is exhausted.
fn run(
    embedder: Embedder,
    index: Index,
    cache: VectorCache,
    todo: Vec<String>,
    batch_size: usize,
    progress: Progress,
) -> anyhow::Result<(usize, Vec<Skipped>)> {
    let total = todo.len() as u64;
    let started = Instant::now();
    let mut embedded = 0usize;
    let mut skipped: Vec<Skipped> = Vec::new();
    progress.step("embedding", 0, total);

    for window in todo.chunks(batch_size) {
        let texts = index.chunk_texts(window)?;

        match embedder.embed_documents(&texts) {
            Ok(vectors) => {
                let entries: Vec<(String, Vec<f32>)> =
                    window.iter().cloned().zip(vectors).collect();
                // Written before the counter moves: the cache is the only record of
                // progress, so a crash between the two must lose work, never invent it.
                cache.append(&entries)?;
                embedded += entries.len();
            }
            // A batch fails as a unit — usually because one chunk is over-long — so it
            // is retried one at a time. Otherwise a single bad chunk costs the other 31,
            // and on a corpus this size that compounds.
            Err(batch_error) => {
                tracing::debug!(%batch_error, "batch failed; retrying individually");
                for hash in window {
                    let text = index.chunk_texts(std::slice::from_ref(hash))?;
                    match embedder.embed_documents(&text) {
                        Ok(mut v) => {
                            cache.append(&[(hash.clone(), v.remove(0))])?;
                            embedded += 1;
                        }
                        Err(e) => skipped.push(Skipped {
                            chunk_hash: hash.clone(),
                            reason: format!("{e:#}"),
                        }),
                    }
                }
            }
        }

        let done = (embedded + skipped.len()) as u64;
        let rate = embedded as f64 / started.elapsed().as_secs_f64().max(f64::EPSILON);
        progress.step(format!("embedding ({rate:.1}/sec)"), done, total);
    }

    Ok((embedded, skipped))
}

/// Loads the model off the async runtime — it is seconds of blocking file and GPU work.
async fn load_embedder(args: &EmbedArgs, model_id: &'static str) -> anyhow::Result<Embedder> {
    let variant = args.variant.clone();
    tokio::task::spawn_blocking(move || {
        let root = models::models_dir()?;
        Embedder::load(&root, model_id, variant.as_deref())
    })
    .await?
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// The counters, plus the rate — which is the only figure anyone plans with.
///
/// `remaining` and `chunks_per_sec` together answer the question actually being asked,
/// which is never "how many did you do" but "how long until the rest is done". So the
/// estimate is computed and printed rather than left as two numbers to divide.
impl Render for EmbedReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.title(
            &self.model,
            &format!("{} · {} dims", self.variant, self.dims),
        )?;
        p.nest(|p| {
            p.figures(&[
                (self.indexed as u64, "chunks indexed"),
                (self.already_cached as u64, "already cached"),
                (self.embedded as u64, "embedded"),
                (self.remaining as u64, "remaining"),
            ])?;

            p.blank()?;
            let rate = format!(
                "{} at {:.1} chunks/sec",
                render::duration(self.elapsed_secs),
                self.chunks_per_sec,
            );
            p.line(p.paint(&rate, Ink::Dim))?;

            // Only when there is something to estimate, and only when a rate exists to
            // estimate from — dividing by a zero rate would print `inf`.
            if self.remaining > 0 && self.chunks_per_sec > 0.0 {
                let eta = self.remaining as f64 / self.chunks_per_sec;
                let text = format!("about {} left — re-run to continue", render::duration(eta));
                p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
            }

            if !self.skipped.is_empty() {
                p.section("skipped")?;
                for item in &self.skipped {
                    let text = format!(
                        "{}  {}",
                        render::short_sha(&item.chunk_hash),
                        render::one_line(&item.reason)
                    );
                    p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::Chunk;
    use crate::index::Placement;
    use crate::store::Store;

    /// A store with `n` chunks indexed, and nothing embedded.
    async fn indexed_store(n: usize) -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let mut index = Index::open(store.index_path()).unwrap();

        for i in 0..n {
            let chunk = Chunk::new(format!("passage number {i}"), i, String::new(), 0);
            index
                .insert(
                    &chunk,
                    &Placement {
                        source: "test".into(),
                        resource: format!("https://example.gov/{i}"),
                        blob_sha: "0".repeat(64),
                        derived_sha: "1".repeat(64),
                        ordinal: i,
                        heading: String::new(),
                        char_start: chunk.char_start,
                        char_end: chunk.char_end,
                        observed_at: "2026-01-01T00:00:00Z".into(),
                        tool: "test".into(),
                        title: None,
                    },
                )
                .unwrap();
        }
        (dir, Ctx::new(store))
    }

    #[tokio::test]
    async fn a_dry_run_reports_the_work_without_loading_a_model() {
        let (_d, ctx) = indexed_store(7).await;
        let report = embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                variant: None,
                batch: DEFAULT_BATCH,
                limit: None,
                dry_run: true,
            },
            &Progress::none(),
        )
        .await
        .unwrap();

        assert_eq!(report.indexed, 7);
        assert_eq!(report.already_cached, 0);
        assert_eq!(report.remaining, 7);
        assert_eq!(report.embedded, 0);
        assert_eq!(report.dims, 2560);
    }

    /// The dry run must work on a machine with no weights at all — it exists to plan a
    /// run before committing to one.
    #[tokio::test]
    async fn a_dry_run_needs_no_weights() {
        let (_d, ctx) = indexed_store(3).await;
        let report = embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                variant: None,
                batch: 8,
                limit: None,
                dry_run: true,
            },
            &Progress::none(),
        )
        .await;
        assert!(report.is_ok(), "{:?}", report.err());
    }

    #[tokio::test]
    async fn an_empty_index_is_not_an_error() {
        let (_d, ctx) = indexed_store(0).await;
        let report = embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                variant: None,
                batch: DEFAULT_BATCH,
                limit: None,
                dry_run: false,
            },
            &Progress::none(),
        )
        .await
        .unwrap();
        assert_eq!(report.indexed, 0);
        assert_eq!(report.embedded, 0);
    }

    /// The resumability claim, without running a model: pre-seed the cache and confirm
    /// the work list is the difference rather than the whole index.
    #[tokio::test]
    async fn cached_chunks_are_subtracted_from_the_work_list() {
        let (_dir, ctx) = indexed_store(10).await;
        let index = Index::open(ctx.store.require_index().unwrap()).unwrap();
        let hashes = index.chunk_hashes().unwrap();

        let cache =
            VectorCache::open(&ctx.store.vector_cache_dir(), "qwen3-embedding-4b", 2560).unwrap();
        let seeded: Vec<(String, Vec<f32>)> = hashes[..4]
            .iter()
            .map(|h| (h.clone(), vec![0.0; 2560]))
            .collect();
        cache.append(&seeded).unwrap();

        let report = embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                variant: None,
                batch: DEFAULT_BATCH,
                limit: None,
                dry_run: true,
            },
            &Progress::none(),
        )
        .await
        .unwrap();

        assert_eq!(report.indexed, 10);
        assert_eq!(report.already_cached, 4);
        assert_eq!(report.remaining, 6, "only uncached chunks are work");
    }

    #[tokio::test]
    async fn a_zero_batch_is_refused() {
        let (_d, ctx) = indexed_store(1).await;
        let err = embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                variant: None,
                batch: 0,
                limit: None,
                dry_run: true,
            },
            &Progress::none(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("--batch"), "{err}");
    }

    #[tokio::test]
    async fn a_reranker_is_refused_as_an_embedding_model() {
        let (_d, ctx) = indexed_store(1).await;
        let err = embed(
            &ctx,
            EmbedArgs {
                model: "qwen3-reranker-0.6b".into(),
                variant: None,
                batch: 8,
                limit: None,
                dry_run: true,
            },
            &Progress::none(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("not an embedding model"), "{err}");
    }

    /// Two embedders write to different files, so switching models can never append
    /// incomparable vectors to an existing cache (§6.2).
    #[tokio::test]
    async fn each_model_gets_its_own_cache_file() {
        let (_d, ctx) = indexed_store(2).await;
        let mut paths = Vec::new();
        for model in ["qwen3-embedding-4b", "qwen3-embedding-0.6b"] {
            let report = embed(
                &ctx,
                EmbedArgs {
                    model: model.into(),
                    variant: None,
                    batch: 8,
                    limit: None,
                    dry_run: true,
                },
                &Progress::none(),
            )
            .await
            .unwrap();
            paths.push(report.cache);
        }
        assert_ne!(paths[0], paths[1]);
    }
}
