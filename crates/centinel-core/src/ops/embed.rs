//! `embed` — turn indexed chunks into vectors.
//!
//! The expensive stage. Separate from `index` for the same reason `collect` is separate
//! from `extract`: `index` is minutes and rebuildable, this is hours.
//!
//! Vectors are written straight to [`crate::vectors`], which is where `search` reads
//! them. There is no intermediate cache — see that module for why the one there used to
//! be did not survive being measured.
//!
//! ## Resumability is a consequence, not a feature
//!
//! There is no checkpoint file. The work list is
//!
//! ```text
//!   chunk hashes in the index  −  chunk hashes already in the table
//! ```
//!
//! so killing a run at chunk 40,000 and re-running starts at 40,001. That falls out of
//! the table being append-only and content-addressed, exactly as `collect`'s
//! resumability falls out of the log. It is also why a **monthly recrawl is cheap**: a
//! re-crawled site is ~95% identical, identical text has an identical `chunk_hash`, and
//! only genuinely new chunks reach the model (SPEC §6.1).
//!
//! ## Batching is not optional
//!
//! A batch is one forward pass over many chunks: one `llama.cpp` context, one `decode`,
//! every chunk in it as its own sequence (see [`crate::embed::Embedder`]). Two costs
//! collapse into it. The context and its KV cache are built per call rather than per
//! text — measured on an M1 Max, one chunk per call gave 6.1 chunks/sec against 18.5 for
//! groups of 32, when that amortisation was all a group bought. And a single ~300-token
//! chunk leaves a GPU almost entirely idle, which is what the packed pass now claims.
//!
//! So the batch is the unit of work here rather than the chunk, and how wide it should be
//! is a property of the *machine* rather than of the corpus — see [`BatchSize`].

use std::time::Instant;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::embed::Embedder;
use crate::index::Index;
use crate::models;
use crate::prelude::*;
use crate::vectors::VectorTable;

/// The batch to fall back on when the machine will not say what it can hold. Measured,
/// on an M1 Max, back when a group only amortised context creation.
const DEFAULT_BATCH: usize = 32;

/// The word for "size it to this machine", in the config file and on the flag alike.
pub const AUTO: &str = "auto";

/// How wide a batch to embed in: a count of chunks, or [`AUTO`].
///
/// A sentinel beside the real values, as [`crate::config::SYSTEM_DEFAULT`] already does
/// for "let something else decide" — one idiom for that question rather than two. It
/// reads the same in either place it is written:
///
/// ```toml
/// embed_batch = "auto"   # or embed_batch = 64
/// ```
/// ```console
/// centinel embed --batch auto   # or --batch 64
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchSize {
    /// Settled when the model loads, from the free memory the backend reports.
    Auto,
    /// Exactly this many chunks per forward pass.
    Fixed(usize),
}

impl BatchSize {
    /// The one parse, so the flag and the config file cannot disagree about what is legal.
    ///
    /// Zero is refused here rather than clamped: `--batch 0` and `embed_batch = 0` are
    /// both somebody meaning something, and neither means "one".
    fn parse(text: &str) -> Result<Self, String> {
        if text.eq_ignore_ascii_case(AUTO) {
            return Ok(Self::Auto);
        }
        match text.parse::<usize>() {
            Ok(0) => Err("a batch of 0 embeds nothing; give a count or `auto`".into()),
            Ok(n) => Ok(Self::Fixed(n)),
            Err(_) => Err(format!(
                "expected a count of chunks or `{AUTO}`, got `{text}`"
            )),
        }
    }
}

/// Written back as it was written: a number stays a number, so a config file this tool
/// prints is a config file it would accept.
impl Serialize for BatchSize {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => s.serialize_str(AUTO),
            Self::Fixed(n) => s.serialize_u64(*n as u64),
        }
    }
}

/// Both TOML spellings, and both JSON ones. TOML hands integers over as `i64` and JSON as
/// `u64`, so a field that accepts `64` has to answer to both or accept it in only one of
/// the two places it can be written.
impl<'de> Deserialize<'de> for BatchSize {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Either;

        impl serde::de::Visitor<'_> for Either {
            type Value = BatchSize;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "a count of chunks or `{AUTO}`")
            }

            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<BatchSize, E> {
                BatchSize::parse(text).map_err(E::custom)
            }

            fn visit_u64<E: serde::de::Error>(self, n: u64) -> Result<BatchSize, E> {
                BatchSize::parse(&n.to_string()).map_err(E::custom)
            }

            fn visit_i64<E: serde::de::Error>(self, n: i64) -> Result<BatchSize, E> {
                BatchSize::parse(&n.to_string()).map_err(E::custom)
            }
        }

        d.deserialize_any(Either)
    }
}

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

    /// Chunks per forward pass — a count, or `auto` for what this machine can hold.
    ///
    /// Unset means "nobody said here", which falls through to `[defaults] embed_batch` in
    /// the config file and from there to `auto`. Typing it is how one run departs from a
    /// machine's standing preference, not how the preference gets stated.
    #[arg(long, value_name = "N|auto", value_parser = BatchSize::parse)]
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub batch: Option<BatchSize>,

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

/// `batch: None` is not "do not batch" — it is "nobody said", which resolves through the
/// config file to `auto`. Keeping [`crate::ops::run`] and the CLI on the same unset value
/// is what stops a second default from quietly reintroducing a number nobody measured.
impl Default for EmbedArgs {
    fn default() -> Self {
        Self {
            model: default_model(),
            variant: None,
            batch: None,
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
    /// Where the vectors went — `vectors.lance/`.
    pub vectors: std::path::PathBuf,
    /// Chunks in the index.
    pub indexed: usize,
    /// Already stored when the run started — the resumed-past portion.
    pub already_embedded: usize,
    pub embedded: usize,
    /// Still outstanding after this run, from `--limit` or from failures.
    pub remaining: usize,
    pub elapsed_secs: f64,
    /// Sustained rate, for planning the rest of a corpus.
    pub chunks_per_sec: f64,
    /// Chunks per forward pass, once the three tiers settled it. Reported because it is
    /// what makes `chunks_per_sec` mean anything — the same corpus and the same model
    /// give different rates at different widths.
    ///
    /// Absent on a dry run, which never loads the weights `auto` reads the machine
    /// through, and so never chooses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<Skipped>,
}

/// Embed indexed chunks into the vector table.
#[op(long_running, reach = "operator", group = "stage")]
pub async fn embed(
    ctx: &Ctx,
    args: EmbedArgs,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<EmbedReport> {
    // `BatchSize::parse` refuses a zero typed on the flag or written in the config, so
    // this catches only the third way in: a caller building `EmbedArgs` in code.
    let zero = Some(BatchSize::Fixed(0));
    anyhow::ensure!(args.batch != zero, "--batch must be at least 1");

    let index = Index::open(ctx.store.require_index()?)?;
    let indexed = index.chunk_hashes()?;

    // Dimensions come from the registry so the table can be opened — and the outstanding
    // work computed — before a multi-gigabyte model is loaded. A dry run never loads one.
    let spec = models::require(&args.model)?;
    let dims = spec
        .dims
        .ok_or_else(|| anyhow::anyhow!("`{}` is not an embedding model", args.model))?
        as usize;

    // Opening creates the table, and `--dry-run` must leave nothing behind — so an
    // absent table is read as "nothing stored" rather than created to be asked. An
    // existing one is opened either way, because that is what checks the model.
    let stored = if ctx.store.vectors_path().exists() {
        VectorTable::open(&ctx.store.vectors_db(), spec.id, dims)
            .await?
            .hashes()
            .await?
    } else {
        std::collections::HashSet::new()
    };
    let already_embedded = indexed.iter().filter(|h| stored.contains(*h)).count();

    let mut todo: Vec<String> = indexed
        .iter()
        .filter(|h| !stored.contains(*h))
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
        vectors: ctx.store.vectors_path(),
        indexed: indexed.len(),
        already_embedded,
        embedded: 0,
        remaining: outstanding,
        elapsed_secs: 0.0,
        chunks_per_sec: 0.0,
        batch: None,
        skipped: Vec::new(),
    };

    if args.dry_run || todo.is_empty() {
        if todo.is_empty() {
            progress.say("nothing to embed — every indexed chunk is already stored");
        }
        return Ok(base);
    }

    let table = VectorTable::open(&ctx.store.vectors_db(), spec.id, dims).await?;

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

    // After the load, not before it: `auto` asks the loaded model what a sequence costs
    // and asks the backend how much memory is left once the weights are on it.
    let batch_size = resolve_batch(args.batch, &embedder)?;
    progress.say(format!("{batch_size} chunks per pass"));

    // The whole run goes into one blocking task rather than one per batch. Inference is
    // CPU/GPU-bound and would stall the async runtime — which matters here more than
    // usual, because an HTTP caller's connection has to stay responsive across hours.
    // One task also means the model is moved once instead of round-tripping per batch.
    let started = Instant::now();
    let (embedded, skipped) = {
        let table = table.clone();
        let host = Host {
            progress: progress.clone(),
            cancel: cancel.clone(),
            // The table's API is async and the loop is not. A handle captured here lets
            // the blocking thread drive an append to completion without a runtime of its
            // own — safe precisely because a `spawn_blocking` thread is not a runtime
            // worker, so parking it starves nothing.
            handle: tokio::runtime::Handle::current(),
        };
        tokio::task::spawn_blocking(move || run(embedder, index, table, todo, batch_size, host))
            .await??
    };

    let elapsed = started.elapsed().as_secs_f64();
    Ok(EmbedReport {
        embedded,
        remaining: outstanding - embedded,
        elapsed_secs: elapsed,
        chunks_per_sec: embedded as f64 / elapsed.max(f64::EPSILON),
        batch: Some(batch_size),
        skipped,
        ..base
    })
}

/// The flag, else the config file's standing preference, else what the machine affords.
///
/// Three tiers because the right batch size is a property of the *machine*: a laptop and
/// a box with 128 GB of unified memory want different numbers for the same corpus, and
/// neither operator should have to remember theirs on every invocation. So the config
/// file is where it is stated, the flag is how one run departs from it, and `auto` is what
/// a machine that has never said anything gets — a number read off it, rather than the
/// same 32 as everything else.
fn resolve_batch(flag: Option<BatchSize>, embedder: &Embedder) -> anyhow::Result<usize> {
    let chosen = match flag {
        Some(explicit) => explicit,
        // Read here rather than passed in, so `centinel embed` typed by hand honours the
        // preference too. `run` has the file open already and passes its value down.
        None => crate::config::Config::load()?.defaults.embed_batch,
    };
    Ok(match chosen {
        BatchSize::Fixed(n) => n,
        // A backend that will not say how much memory it has gets the measured default
        // rather than a guess built on nothing.
        BatchSize::Auto => embedder.auto_batch().unwrap_or(DEFAULT_BATCH),
    })
}

/// What the blocking loop needs from the async world it was spawned out of.
///
/// One struct rather than three parameters because they are one thing: the loop runs on a
/// thread with no runtime and no caller, and these are its three ways back to both.
struct Host {
    progress: Progress,
    cancel: Cancel,
    handle: tokio::runtime::Handle,
}

/// The blocking loop: batch → embed → append, until the work list is exhausted.
fn run(
    embedder: Embedder,
    index: Index,
    table: VectorTable,
    todo: Vec<String>,
    batch_size: usize,
    host: Host,
) -> anyhow::Result<(usize, Vec<Skipped>)> {
    let Host {
        progress,
        cancel,
        handle,
    } = host;
    let total = todo.len() as u64;
    let started = Instant::now();
    let mut embedded = 0usize;
    let mut skipped: Vec<Skipped> = Vec::new();
    progress.step("embedding", 0, total);

    for window in todo.chunks(batch_size) {
        // The item boundary is one batch, which is also what `table.append` commits.
        // Chunk identity is the hash of its text, so what was stored stays stored and
        // the next run subtracts it — stopping mid-corpus costs nothing but the batch
        // that never started.
        cancel.check()?;

        let texts = index.chunk_texts(window)?;

        match embedder.embed_documents(&texts) {
            Ok(vectors) => {
                let entries: Vec<(String, Vec<f32>)> =
                    window.iter().cloned().zip(vectors).collect();
                // Written before the counter moves: the table is the only record of
                // progress, so a crash between the two must lose work, never invent it.
                handle.block_on(table.append(&entries))?;
                embedded += entries.len();
            }
            // A batch fails as a unit, so it is retried one at a time. Otherwise a single
            // bad chunk costs the other 31, and on a corpus this size that compounds.
            //
            // The outward behaviour is unchanged now that the group shares one `decode`,
            // but the failure it recovers from is a wider one. It used to be a chunk that
            // would not tokenize; it is now also a group the machine could not hold — one
            // `new_context` for a KV cache too large, or one `decode` out of memory. Both
            // come back here, and one chunk at a time is the right answer to both.
            Err(batch_error) => {
                tracing::debug!(%batch_error, "batch failed; retrying individually");
                for hash in window {
                    let text = index.chunk_texts(std::slice::from_ref(hash))?;
                    match embedder.embed_documents(&text) {
                        Ok(mut v) => {
                            handle.block_on(table.append(&[(hash.clone(), v.remove(0))]))?;
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
                (self.already_embedded as u64, "already embedded"),
                (self.embedded as u64, "embedded"),
                (self.remaining as u64, "remaining"),
            ])?;

            p.blank()?;
            // The batch beside the rate, because it is what the rate is a rate *at* —
            // two runs of the same corpus are only comparable at the same width.
            let mut rate = format!(
                "{} at {:.1} chunks/sec",
                render::duration(self.elapsed_secs),
                self.chunks_per_sec,
            );
            if let Some(batch) = self.batch {
                rate.push_str(&format!(" \u{00b7} {batch} per pass"));
            }
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
                batch: None,
                limit: None,
                dry_run: true,
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();

        assert_eq!(report.indexed, 7);
        assert_eq!(report.already_embedded, 0);
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
                batch: Some(BatchSize::Fixed(8)),
                limit: None,
                dry_run: true,
            },
            &Progress::none(),
            &Cancel::none(),
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
                batch: None,
                limit: None,
                dry_run: false,
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(report.indexed, 0);
        assert_eq!(report.embedded, 0);
    }

    /// The resumability claim, without running a model: pre-seed the table and confirm
    /// the work list is the difference rather than the whole index.
    #[tokio::test]
    async fn stored_chunks_are_subtracted_from_the_work_list() {
        let (_dir, ctx) = indexed_store(10).await;
        let index = Index::open(ctx.store.require_index().unwrap()).unwrap();
        let hashes = index.chunk_hashes().unwrap();

        let table = VectorTable::open(&ctx.store.vectors_db(), "qwen3-embedding-4b", 2560)
            .await
            .unwrap();
        let seeded: Vec<(String, Vec<f32>)> = hashes[..4]
            .iter()
            .map(|h| (h.clone(), vec![0.0; 2560]))
            .collect();
        table.append(&seeded).await.unwrap();

        let report = embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                variant: None,
                batch: None,
                limit: None,
                dry_run: true,
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();

        assert_eq!(report.indexed, 10);
        assert_eq!(report.already_embedded, 4);
        assert_eq!(report.remaining, 6, "only unembedded chunks are work");
    }

    /// A dry run is a plan, so it must not leave a table behind on a store that had
    /// none — the same rule `doctor` follows for the models directory.
    #[tokio::test]
    async fn a_dry_run_creates_no_table() {
        let (_d, ctx) = indexed_store(3).await;
        embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                variant: None,
                batch: Some(BatchSize::Fixed(8)),
                limit: None,
                dry_run: true,
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert!(!ctx.store.vectors_path().exists());
    }

    #[tokio::test]
    async fn a_zero_batch_is_refused() {
        let (_d, ctx) = indexed_store(1).await;
        let err = embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                variant: None,
                batch: Some(BatchSize::Fixed(0)),
                limit: None,
                dry_run: true,
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("--batch"), "{err}");
    }

    /// The two spellings, and the three ways of writing neither.
    #[test]
    fn a_batch_size_is_a_count_or_the_word_auto() {
        assert_eq!(BatchSize::parse(AUTO), Ok(BatchSize::Auto));
        assert_eq!(BatchSize::parse("AUTO"), Ok(BatchSize::Auto));
        assert_eq!(BatchSize::parse("64"), Ok(BatchSize::Fixed(64)));
        assert!(BatchSize::parse("0").is_err(), "0 embeds nothing");
        assert!(BatchSize::parse("-8").is_err());
        assert!(BatchSize::parse("lots").is_err());
    }

    /// The MCP path, where args arrive as JSON rather than as typed words. A client
    /// following the schema sends a string; one reading the TOML sends a number. Both are
    /// the same request and neither should be the one that fails.
    #[test]
    fn args_accept_the_batch_as_a_string_or_a_number() {
        let parse = |json: &str| serde_json::from_str::<EmbedArgs>(json).unwrap().batch;
        assert_eq!(parse(r#"{"batch": "auto"}"#), Some(BatchSize::Auto));
        assert_eq!(parse(r#"{"batch": 64}"#), Some(BatchSize::Fixed(64)));
        assert_eq!(parse("{}"), None, "unset falls through to the config");
    }

    /// Round-tripped as it was written, so a number survives as a number — `run` and the
    /// scheduler both serialize their args and read them back.
    #[test]
    fn a_batch_size_survives_a_round_trip() {
        for size in [BatchSize::Auto, BatchSize::Fixed(64)] {
            let json = serde_json::to_string(&size).unwrap();
            assert_eq!(serde_json::from_str::<BatchSize>(&json).unwrap(), size);
        }
        assert_eq!(
            serde_json::to_string(&BatchSize::Auto).unwrap(),
            r#""auto""#
        );
        assert_eq!(serde_json::to_string(&BatchSize::Fixed(64)).unwrap(), "64");
    }

    #[tokio::test]
    async fn a_reranker_is_refused_as_an_embedding_model() {
        let (_d, ctx) = indexed_store(1).await;
        let err = embed(
            &ctx,
            EmbedArgs {
                model: "qwen3-reranker-0.6b".into(),
                variant: None,
                batch: Some(BatchSize::Fixed(8)),
                limit: None,
                dry_run: true,
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("not an embedding model"), "{err}");
    }

    /// One table, so switching models is refused rather than silently mixed. A vector
    /// from another model is in another space and still returns a ranked list, which is
    /// why the refusal has to happen before anything is written (§6.2).
    #[tokio::test]
    async fn a_second_model_is_refused_rather_than_mixed_in() {
        let (_d, ctx) = indexed_store(2).await;
        VectorTable::open(&ctx.store.vectors_db(), "qwen3-embedding-4b", 2560)
            .await
            .unwrap();

        let err = embed(
            &ctx,
            EmbedArgs {
                model: "qwen3-embedding-0.6b".into(),
                variant: None,
                batch: Some(BatchSize::Fixed(8)),
                limit: None,
                dry_run: true,
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap_err()
        .to_string();

        // Which of the two guards catches it depends on the pair — `0.6b` is 1024-dim,
        // so the width check fires before the model check. Either is a refusal, and both
        // have to name the way out; the model-id path is asserted on its own in
        // `vectors`, where two models share a width.
        assert!(
            err.contains("centinel embed"),
            "refused, and names the fix: {err}"
        );
    }
}
