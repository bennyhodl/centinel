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
//! the table being content-addressed and every append a commit, exactly as `collect`'s
//! resumability falls out of the log. It is also why a **monthly recrawl is cheap**: a
//! re-crawled site is ~95% identical, identical text has an identical `chunk_hash`, and
//! only genuinely new chunks reach the model (SPEC §6.1).
//!
//! ## One row per placement, and most rows are copies
//!
//! The table holds a row for every chunk **placed in a document**, carrying the
//! document's source, tags and exclusion so a search can filter inside the vector query
//! (see [`crate::vectors`]). The work list is therefore placements, not chunks, and a
//! placement's vector comes from the cheapest place that has it: another document's row
//! for the same chunk, the `vectors` table a store from before this layout still holds,
//! and only then the model. A recrawl that changes a page gives it a new derived text and
//! so new placements, nearly all of them for passages the table already has under the
//! old text; those are copied, and the model sees only what is actually new. Upgrading a
//! store is the same loop with nothing new: `centinel embed --limit 0` fills the table
//! from `vectors` with nothing sent to a model, resumably, by the same set difference.
//!
//! Each row is written with what the workspace says about its document at that moment,
//! read under the lock every writer of the table takes, and the workspace's own queue of
//! changed documents is drained before the first row and after the last — so a row is
//! never older than a change the workspace already made without a pending revision
//! saying so.
//!
//! ## The subtraction runs the other way too
//!
//! Rows for placements the index no longer has are deleted ([`VectorTable::remove`]) —
//! after the copies, because a re-derived page's unchanged chunks are copied off its old
//! rows before they go. A rebuilt index or a `--rebuild --source` used to leave such rows
//! behind for good — nothing read them, `search` counted them, and on one store they were
//! 1.09 million of 1.35 million rows and four of every five bytes a query scanned. One
//! stage owns the table in both directions, so there is one definition of a stored row: a
//! placement the index has. After the run the table is compacted and its superseded
//! versions dropped ([`VectorTable::maintain`]), which is what keeps a query from reading
//! one fragment per batch ever appended.
//!
//! The one thing this will not do is empty the table on an empty index: zero placements is
//! an index before `centinel index` has run. The cost that remains is an index rebuilt
//! and partly re-indexed when `embed` runs: the rows it lacks go, and come back at the
//! price of embedding them again. `run` orders `index` before `embed` for exactly that
//! reason, and the report says what was pruned.
//!
//! Only one `embed` runs on a store at a time (`workspace/embed.lock`), so two runs never
//! work out the same missing rows and append them twice.
//!
//! ## Batching is not optional
//!
//! A batch is one `decode` call over many chunks: one `llama.cpp` context, every chunk
//! its own sequence, the call run as bounded physical passes (see
//! [`crate::embed::EmbedSession`]). Two costs collapse into it. The context and its KV
//! cache are built per call rather than per text — measured on an M1 Max, one chunk per
//! call gave 6.1 chunks/sec against 18.5 for groups of 32, when that amortisation was
//! all a group bought. And a single ~300-token chunk leaves a GPU almost entirely idle,
//! which is what the packed call now claims.
//!
//! So the batch is the unit of work here rather than the chunk, and how wide it should be
//! is a property of the *machine* rather than of the corpus — see [`BatchSize`].
//!
//! The context stands for the whole run, and I/O rides beside the decode rather than
//! between decodes: a reader thread keeps the next batch's text ready and a writer task
//! appends the last batch's vectors, so the GPU waits on neither SQLite nor Lance — see
//! [`run`].
//!
//! ## Remote is the same loop with the decode swapped out
//!
//! An `openrouter/…` model routes this stage through [`crate::remote`] instead of local
//! weights — the work list, the resume subtraction, the writer and the skip accounting
//! are unchanged, and the remote model id keys its own vector table, so the two kinds of
//! run cannot touch each other's spaces. What changes is the middle of the pipeline:
//! HTTP requests in flight instead of a standing `llama.cpp` context ([`run_remote`]),
//! and a batch that is a request body rather than a KV-cache reservation.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context as _;
use futures::TryStreamExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::embed::{Embedder, SessionOptions};
use crate::index::Index;
use crate::models;
use crate::prelude::*;
use crate::remote::{self, EmbeddingBackend, RemoteEmbedder};
use crate::vectors::{LegacyVectors, Row, VectorLock, VectorTable, parse_key};
use crate::workspace::classifications::Classifications;
use crate::workspace::{DocState, DocumentId};

/// The batch to fall back on when the machine will not say what it can hold. Measured,
/// on an M1 Max, back when a group only amortised context creation.
const DEFAULT_BATCH: usize = 32;

/// Chunks per lookup when a placement's vector is copied from another row of the table.
/// One `IN (…)` predicate each, served by the hash index once `maintain` has built it.
const COPY_WINDOW: usize = 256;

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
    /// upgrade — vectors from two models share no space (SPEC §6.2). An `openrouter/…`
    /// id embeds remotely: chunk text goes to openrouter.ai, and $OPENROUTER_API_KEY
    /// must be set.
    ///
    /// Unset means "nobody said here", which falls through to `[defaults] embed_model`
    /// in the config file and from there to `qwen3-embedding-4b` — `--batch`'s route
    /// exactly. A clap default here would make a typed model indistinguishable from an
    /// absent one, and the file's standing preference would be read and never used.
    #[arg(long)]
    #[serde(default)]
    pub model: Option<String>,

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

    /// Build an approximate vector index after embedding. Compare speed and recall
    /// with `search --exact` on representative queries before adopting it.
    #[arg(long)]
    #[serde(default)]
    pub ann_index: bool,
}

/// `model: None` and `batch: None` are not "no model" and "do not batch" — both mean
/// "nobody said", which resolves through the config file to `[defaults] embed_model` and
/// `embed_batch`. Keeping [`crate::ops::run`] and the CLI on the same unset value is what
/// stops a second default from quietly shadowing the file's standing preference.
impl Default for EmbedArgs {
    fn default() -> Self {
        Self {
            model: None,
            variant: None,
            batch: None,
            limit: None,
            dry_run: false,
            ann_index: false,
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
    /// Where the vectors went — `chunks.lance/`.
    pub vectors: std::path::PathBuf,
    /// Chunk placements in the index: one for each chunk in each document that holds it.
    pub indexed: usize,
    /// Placements that already had a row when the run started — the resumed-past portion.
    pub already_embedded: usize,
    /// Placements written this run from a vector the store already held: another
    /// document's row for the same chunk, or the `vectors` table from before this layout.
    /// Nothing was sent to a model for them.
    #[serde(default)]
    pub filled: usize,
    /// Chunks the model embedded this run; their placements were written with them.
    pub embedded: usize,
    /// Placements still without a row after this run, from `--limit` or from failures.
    pub remaining: usize,
    /// Rows for placements the index no longer had when the run started — what a rebuilt
    /// index, a cleared source or a re-extracted page leaves behind.
    #[serde(default)]
    pub stale: usize,
    /// Of those, how many this run deleted. Zero on a dry run.
    #[serde(default)]
    pub pruned: usize,
    /// Superseded table versions whose files this run removed.
    #[serde(default)]
    pub versions_removed: u64,
    pub elapsed_secs: f64,
    /// Sustained rate of the model, for planning the rest of a corpus.
    pub chunks_per_sec: f64,
    /// Chunks per forward pass, once the three tiers settled it. Reported because it is
    /// what makes `chunks_per_sec` mean anything — the same corpus and the same model
    /// give different rates at different widths.
    ///
    /// Absent on a dry run, which never loads the weights `auto` reads the machine
    /// through, and so never chooses; and on a run the copies finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<Skipped>,
}

/// A chunk's hash as 32 bytes and its document as a position in the index's document
/// list: a placement, small enough that a million and a half of them sit beside a model.
type Pair = ([u8; 32], u32);

fn hash_bytes(hex: &str) -> anyhow::Result<[u8; 32]> {
    hex::decode(hex)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| anyhow::anyhow!("`{hex}` is not a chunk hash"))
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
    // Before the work list is worked out, so a second run cannot work out the same one.
    let _run = match args.dry_run {
        true => None,
        false => Some(hold_embed_lock(&ctx.store)?),
    };

    // Shortest first, so batches carry texts of one kind and the oversized tail — the
    // chunks that force a bespoke context — arrives together at the end of the run.
    // Said before it runs, not after: on a large corpus it is the first thing a caller
    // waits on.
    progress.say("reading the index");
    let index = Index::open(ctx.store.require_index()?)?;
    let placements = index.placements_by_length()?;

    // The flag, else the config file's standing preference — [`resolve_batch`]'s first
    // two tiers, resolved here so `centinel embed` typed by hand embeds with the model
    // the file names. `run` reads the file once and passes its value down.
    let model = match args.model.clone() {
        Some(typed) => typed,
        None => crate::config::Config::load()?.defaults.embed_model,
    };

    // Dimensions come from a registry — local or remote — so the table can be opened,
    // and the outstanding work computed, before a multi-gigabyte model is loaded or a
    // key is read. A dry run touches neither.
    let backend = remote::backend_for(&model)?;
    let (model_id, dims): (&'static str, usize) = match backend {
        EmbeddingBackend::Local(spec) => (
            spec.id,
            spec.dims
                .ok_or_else(|| anyhow::anyhow!("`{}` is not an embedding model", model))?
                as usize,
        ),
        EmbeddingBackend::Remote(spec) => {
            // A quantization is a fact about weights on this machine. A remote model
            // keeps none here, and accepting the flag would record a variant that
            // names nothing.
            anyhow::ensure!(
                args.variant.is_none(),
                "`{}` runs remotely and has no quantization variant — drop --variant",
                spec.id
            );
            (spec.id, spec.dims as usize)
        }
    };

    // Opening creates the table, and `--dry-run` must leave nothing behind — so an
    // absent table is read as "nothing stored" rather than created to be asked. An
    // existing one is opened either way, because that is what checks the model.
    let table = if ctx.store.vectors_path().exists() {
        Some(VectorTable::open(&ctx.store.vectors_db(), model_id, dims).await?)
    } else {
        None
    };

    // The work list is a set difference over placements. The table's keys are streamed
    // past the index's pairs rather than collected: what is in both is done, what is only
    // in the table is stale, and what is only in the index is the work.
    let mut all: HashSet<Pair> = HashSet::with_capacity(placements.pairs.len());
    for (hash, document) in &placements.pairs {
        all.insert((hash_bytes(hash)?, *document));
    }
    let mut todo = all.clone();
    let by_document: HashMap<&DocumentId, u32> = placements
        .documents
        .iter()
        .enumerate()
        .map(|(i, document)| (document, i as u32))
        .collect();
    // Rows the index no longer has, by key, and every chunk the table holds a vector for
    // under any document — the index's or one it no longer has. A recrawl that re-derives
    // a page keeps most of its chunks, and the old derivation's rows are where their
    // vectors are copied from before those rows are pruned.
    let mut stale: Vec<String> = Vec::new();
    let mut known: HashSet<[u8; 32]> = HashSet::new();
    if let Some(table) = &table {
        progress.say("checking stored vectors");
        table
            .keys(|key| {
                let Some((hash, document)) = parse_key(key) else {
                    stale.push(key.to_owned());
                    return;
                };
                let Ok(hash) = hash_bytes(hash) else {
                    stale.push(key.to_owned());
                    return;
                };
                known.insert(hash);
                match by_document.get(&document) {
                    Some(&document) if all.contains(&(hash, document)) => {
                        todo.remove(&(hash, document));
                    }
                    _ => stale.push(key.to_owned()),
                }
            })
            .await
            .with_context(|| verify_hint("reading the stored keys"))?;
    }
    // Zero placements is what an index looks like before `centinel index` has run, not a
    // statement that every row is stale, and rows are what cost hours or money to replace.
    let prunable = !placements.pairs.is_empty();
    let already_embedded = all.len() - todo.len();
    let outstanding = todo.len();
    drop(all);

    // The work grouped by chunk, in index order: each chunk once, with the documents
    // still owed a row for it.
    let mut order: Vec<String> = Vec::new();
    let mut pending: HashMap<[u8; 32], Vec<u32>> = HashMap::new();
    for (hash, document) in &placements.pairs {
        let bytes = hash_bytes(hash)?;
        if !todo.contains(&(bytes, *document)) {
            continue;
        }
        let documents = pending.entry(bytes).or_default();
        if documents.is_empty() {
            order.push(hash.clone());
        }
        documents.push(*document);
    }
    drop(todo);

    let base = EmbedReport {
        model: model_id.to_string(),
        variant: match backend {
            EmbeddingBackend::Local(spec) => args
                .variant
                .clone()
                .unwrap_or_else(|| spec.default_variant.to_string()),
            // The provider stands where the quantization would: it is the honest
            // answer a remote run can give to "what produced these vectors".
            EmbeddingBackend::Remote(_) => "openrouter".to_string(),
        },
        dims,
        vectors: ctx.store.vectors_path(),
        indexed: placements.pairs.len(),
        already_embedded,
        filled: 0,
        embedded: 0,
        remaining: outstanding,
        stale: stale.len(),
        pruned: 0,
        versions_removed: 0,
        elapsed_secs: 0.0,
        chunks_per_sec: 0.0,
        batch: None,
        skipped: Vec::new(),
    };

    if args.dry_run {
        return Ok(base);
    }
    // No table and nothing to embed: nothing to reconcile and nothing to maintain, and
    // creating an empty table to say so would leave the store looking embedded.
    let table = match table {
        Some(table) => table,
        None if order.is_empty() => return Ok(base),
        None => VectorTable::open(&ctx.store.vectors_db(), model_id, dims).await?,
    };

    // What the workspace changed since the last drain lands first, so no row written
    // below is older than a change the workspace already made.
    crate::workspace::sync_vectors(&ctx.store).await?;

    let started = Instant::now();
    let mut writer = Writer::new(
        table.clone(),
        ctx.store.clone(),
        Arc::new(placements.documents),
        pending,
    )?;

    // Copies first: a chunk the table already holds under another document, then the
    // table from before this layout. Neither asks a model anything.
    let copyable: Vec<String> = order
        .iter()
        .filter(|hash| hash_bytes(hash).is_ok_and(|bytes| known.contains(&bytes)))
        .cloned()
        .collect();
    drop(known);
    if !copyable.is_empty() {
        progress.step(
            "copying vectors held under other documents",
            0,
            copyable.len() as u64,
        );
        for (i, window) in copyable.chunks(COPY_WINDOW).enumerate() {
            cancel.check()?;
            let vectors = table.vectors_of(window).await?;
            writer.write(vectors.into_iter().collect()).await?;
            progress.step(
                "copying vectors held under other documents",
                ((i + 1) * COPY_WINDOW).min(copyable.len()) as u64,
                copyable.len() as u64,
            );
        }
    }
    if writer.has_pending()
        && let Some(legacy) = LegacyVectors::open(&ctx.store.vectors_db(), model_id).await?
    {
        anyhow::ensure!(
            legacy.dims() == dims,
            "the `vectors` table holds {}-dimensional vectors, not {dims}",
            legacy.dims()
        );
        progress.say("copying vectors from the vectors table");
        let mut rows = legacy.rows().await?;
        while let Some(batch) = rows.try_next().await? {
            cancel.check()?;
            let wanted: Vec<_> = batch
                .into_iter()
                .filter(|(hash, _)| writer.wants(hash))
                .collect();
            writer.write(wanted).await?;
            if !writer.has_pending() {
                break;
            }
        }
    }
    let filled = writer.rows;

    // Pruned after the copies, which may have read a vector off a row about to go.
    let pruned = match stale.len() {
        n if n > 0 && prunable => {
            cancel.check()?;
            progress.say(format!("pruning {n} rows the index no longer has"));
            let lock = VectorLock::take(&ctx.store).await?;
            table.remove(&lock, &stale).await?;
            n
        }
        _ => 0,
    };
    drop(stale);

    // The rest through the model, shortest first, as many as `--limit` allows.
    let mut todo: Vec<String> = order
        .into_iter()
        .filter(|hash| writer.wants(hash))
        .collect();
    if let Some(limit) = args.limit {
        todo.truncate(limit);
    }
    let finish = |writer: &Writer,
                  embedded: usize,
                  skipped: Vec<Skipped>,
                  batch: Option<usize>,
                  done: crate::vectors::Maintenance| {
        let elapsed = started.elapsed().as_secs_f64();
        EmbedReport {
            filled,
            embedded,
            remaining: writer.remaining_rows(),
            pruned,
            versions_removed: done.versions_removed,
            elapsed_secs: elapsed,
            chunks_per_sec: embedded as f64 / elapsed.max(f64::EPSILON),
            batch,
            skipped,
            ..base
        }
    };
    if todo.is_empty() {
        cancel.check()?;
        progress.say("maintaining the vector table");
        let done = maintain(&ctx.store, &table, args.ann_index).await?;
        crate::workspace::sync_vectors(&ctx.store).await?;
        return Ok(finish(&writer, 0, Vec::new(), None, done));
    }

    let chunks_before = writer.chunks;
    let (writer, skipped, batch_size) = match backend {
        EmbeddingBackend::Local(spec) => {
            progress.say(format!(
                "loading {} ({})",
                spec.id,
                args.variant.as_deref().unwrap_or(spec.default_variant)
            ));
            let embedder =
                load_embedder(&args, spec.id, crate::spend::Ledger::new(&ctx.store)).await?;
            progress.say(embedder.device_summary());
            anyhow::ensure!(
                embedder.dims() == dims,
                "{} reports {} dimensions, the registry pins {dims}",
                spec.id,
                embedder.dims()
            );

            // After the load, not before it: `auto` asks the loaded model what a
            // sequence costs and asks the backend how much memory is left once the
            // weights are on it.
            let batch_size = resolve_batch(args.batch, &embedder)?;
            progress.say(format!("{batch_size} chunks per pass"));

            // The whole run goes into one blocking task rather than one per batch.
            // Inference is CPU/GPU-bound and would stall the async runtime — which
            // matters here more than usual, because an HTTP caller's connection has to
            // stay responsive across hours. One task also means the model is moved once
            // instead of round-tripping per batch.
            let (writer, skipped) = {
                let host = Host {
                    progress: progress.clone(),
                    cancel: cancel.clone(),
                    // The table's API is async and the loop is not. A handle captured
                    // here lets the blocking thread drive an append to completion
                    // without a runtime of its own — safe precisely because a
                    // `spawn_blocking` thread is not a runtime worker, so parking it
                    // starves nothing.
                    handle: tokio::runtime::Handle::current(),
                };
                tokio::task::spawn_blocking(move || {
                    run(embedder, index, writer, todo, batch_size, host)
                })
                .await??
            };
            (writer, skipped, batch_size)
        }
        EmbeddingBackend::Remote(spec) => {
            // Said before the first byte moves, because it is the run's one departure
            // from §2.1: this stage, this model, chunk text to openrouter.ai.
            progress.say(format!("{} — chunk text is sent to openrouter.ai", spec.id));
            let embedder = RemoteEmbedder::new(spec, crate::spend::Ledger::new(&ctx.store))?;
            let batch_size = resolve_remote_batch(args.batch)?;
            progress.say(format!("{batch_size} chunks per request"));
            let (writer, skipped) =
                run_remote(embedder, index, writer, todo, batch_size, progress, cancel).await?;
            (writer, skipped, batch_size)
        }
    };

    cancel.check()?;
    progress.say("maintaining the vector table");
    let done = maintain(&ctx.store, &table, args.ann_index).await?;
    crate::workspace::sync_vectors(&ctx.store).await?;
    Ok(finish(
        &writer,
        writer.chunks - chunks_before,
        skipped,
        Some(batch_size),
        done,
    ))
}

/// The one writer of rows for a run: it knows which documents still owe a row for each
/// chunk, and for every vector it is handed it writes those rows with what the workspace
/// says about each document right now — read under the table's lock, so a change the
/// workspace makes after the read is a revision the next sync carries.
struct Writer {
    table: VectorTable,
    store: Store,
    documents: Arc<Vec<DocumentId>>,
    /// The documents owed a row for each chunk, by hash. A chunk written leaves the map.
    pending: HashMap<[u8; 32], Vec<u32>>,
    classifications: Classifications,
    /// Chunks written, whichever way their vector arrived.
    chunks: usize,
    /// Rows written.
    rows: usize,
}

impl Writer {
    fn new(
        table: VectorTable,
        store: Store,
        documents: Arc<Vec<DocumentId>>,
        pending: HashMap<[u8; 32], Vec<u32>>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            table,
            classifications: Classifications::open(&store)?,
            store,
            documents,
            pending,
            chunks: 0,
            rows: 0,
        })
    }

    /// Whether some document still owes a row for this chunk.
    fn wants(&self, hash: &str) -> bool {
        hash_bytes(hash).is_ok_and(|bytes| self.pending.contains_key(&bytes))
    }

    fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Placements still without a row.
    fn remaining_rows(&self) -> usize {
        self.pending.values().map(Vec::len).sum()
    }

    /// Writes the rows these vectors are owed, one durable commit. A chunk nobody owes a
    /// row for any more is skipped: a copy and the model may both offer it.
    async fn write(&mut self, entries: Vec<(String, Vec<f32>)>) -> anyhow::Result<()> {
        let mut owed: Vec<(String, u32, Vec<f32>)> = Vec::new();
        for (hash, vector) in entries {
            let Some(documents) = self.pending.remove(&hash_bytes(&hash)?) else {
                continue;
            };
            self.chunks += 1;
            for document in documents {
                owed.push((hash.clone(), document, vector.clone()));
            }
        }
        if owed.is_empty() {
            return Ok(());
        }
        let lock = VectorLock::take(&self.store).await?;
        let mut states: HashMap<u32, DocState> = HashMap::new();
        let mut rows = Vec::with_capacity(owed.len());
        for (chunk_hash, document, vector) in owed {
            let id = &self.documents[document as usize];
            let state = match states.get(&document) {
                Some(state) => state.clone(),
                None => {
                    let state = self.classifications.state_of(id)?;
                    states.insert(document, state.clone());
                    state
                }
            };
            rows.push(Row {
                chunk_hash,
                document: id.clone(),
                state,
                vector,
            });
        }
        self.table.append(&lock, &rows).await?;
        self.rows += rows.len();
        Ok(())
    }
}

/// Compacts the table and brings its indexes up to date, under the write lock — released
/// before returning, since the sync that follows takes it again.
async fn maintain(
    store: &Store,
    table: &VectorTable,
    build_index: bool,
) -> anyhow::Result<crate::vectors::Maintenance> {
    let lock = VectorLock::take(store).await?;
    table
        .maintain(&lock, build_index)
        .await
        .with_context(|| verify_hint("maintaining the vector table"))
}

/// Held for a whole `embed`, so two runs cannot both work out the same missing rows and
/// both append them. Taken without waiting: the second run is told the first is going.
/// Released when the file is dropped at the end of the run.
fn hold_embed_lock(store: &Store) -> anyhow::Result<std::fs::File> {
    std::fs::create_dir_all(store.workspace_dir())?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(store.embed_lock_path())?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => anyhow::bail!(
            "another `centinel embed` is writing this store's vectors; wait for it to finish"
        ),
        Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

/// Context for a failed pass over the whole vector table, naming the op that can say
/// which fragment failed.
///
/// `hashes` and `maintain` read every fragment, so they are where a data file that storage
/// silently truncated first shows up — as a read error with no fragment named, because a
/// scan only knows that it failed. Nothing here guesses at the cause: a read error is one
/// possibility, and `centinel verify` is what tells. The underlying error stays beneath
/// this line.
fn verify_hint(doing: &str) -> String {
    format!(
        "{doing} failed; if this is a read error, run `centinel verify` to find the \
         fragments Lance cannot read"
    )
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

/// Chunks per request when the operator and the config both said `auto`.
///
/// The local `auto` reads the machine because the KV cache is the cost; a request has no
/// KV cache, and its costs pull mildly in both directions — fewer round trips against
/// more lost to one failed batch's one-at-a-time retry. 128 chunks is ~40K tokens,
/// comfortably inside every curated model's request ceiling.
const REMOTE_DEFAULT_BATCH: usize = 128;

/// Requests in flight at once. What hides the network's latency, as the reader thread
/// hides SQLite's: while one batch is on the wire, three more are being answered. More
/// would mostly exercise the provider's rate limiter.
const REMOTE_IN_FLIGHT: usize = 4;

/// The flag, else the config file's standing preference, else [`REMOTE_DEFAULT_BATCH`].
///
/// [`resolve_batch`]'s three tiers, with one difference: `auto` is a question about the
/// machine, no machine is involved, and so it resolves to a constant rather than to a
/// memory reading.
fn resolve_remote_batch(flag: Option<BatchSize>) -> anyhow::Result<usize> {
    let chosen = match flag {
        Some(explicit) => explicit,
        None => crate::config::Config::load()?.defaults.embed_batch,
    };
    Ok(match chosen {
        BatchSize::Fixed(n) => n,
        BatchSize::Auto => REMOTE_DEFAULT_BATCH,
    })
}

/// The remote pipeline: read → request → write, with [`REMOTE_IN_FLIGHT`] requests on
/// the wire at once.
///
/// [`run`]'s shape with the decode swapped for HTTP: the reader thread keeps windows of
/// text coming off SQLite, in-flight requests overlap the network's latency, and the
/// writer task writes what has come back. `buffered` completes in submission order, so
/// the writer receives windows in work-list order and the durable count means what it
/// does locally: everything before it landed.
///
/// Failure handling is [`run`]'s, plus one distinction the local path never needed: a
/// refused key fails every chunk identically, so it ends the run ([`remote::is_fatal`])
/// instead of skipping a corpus one round trip at a time.
async fn run_remote(
    embedder: RemoteEmbedder,
    index: Index,
    writer: Writer,
    todo: Vec<String>,
    batch_size: usize,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<(Writer, Vec<Skipped>)> {
    use futures::StreamExt;

    let total = todo.len() as u64;
    let started = Instant::now();
    progress.step("embedding", 0, total);

    // The reader owns the index, exactly as in `run`: a SQLite connection does not
    // share across threads, and nothing else needs it once the work list exists.
    let (feed_tx, feed_rx) =
        tokio::sync::mpsc::channel::<anyhow::Result<(Vec<String>, Vec<String>)>>(PIPELINE_SLACK);
    let reader = std::thread::spawn(move || {
        for window in todo.chunks(batch_size) {
            let texts = index.chunk_texts(window);
            let failed = texts.is_err();
            if feed_tx
                .blocking_send(texts.map(|t| (window.to_vec(), t)))
                .is_err()
                || failed
            {
                return;
            }
        }
    });

    let (write_tx, mut write_rx) =
        tokio::sync::mpsc::channel::<Vec<(String, Vec<f32>)>>(PIPELINE_SLACK);
    let writer = tokio::spawn(async move {
        let mut writer = writer;
        while let Some(entries) = write_rx.recv().await {
            writer.write(entries).await?;
        }
        anyhow::Ok(writer)
    });

    let mut sent = 0usize;
    let mut skipped: Vec<Skipped> = Vec::new();

    let decoded: anyhow::Result<()> = {
        let embedder = &embedder;
        let windows = futures::stream::unfold(feed_rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });
        let mut results = std::pin::pin!(
            windows
                .map(move |item| async move {
                    let (window, texts) = item?;
                    embed_window(embedder, window, texts).await
                })
                .buffered(REMOTE_IN_FLIGHT)
        );

        let writer_gone = || anyhow::anyhow!("the writer stopped; its error follows");
        async {
            while let Some(outcome) = results.next().await {
                cancel.check()?;
                let (entries, misses) = outcome?;
                sent += entries.len();
                skipped.extend(misses);
                if !entries.is_empty() {
                    write_tx.send(entries).await.map_err(|_| writer_gone())?;
                }
                let done = (sent + skipped.len()) as u64;
                let rate = sent as f64 / started.elapsed().as_secs_f64().max(f64::EPSILON);
                progress.step(format!("embedding ({rate:.1}/sec)"), done, total);
            }
            Ok(())
        }
        .await
        // The block ends here so the stream — and the feed receiver inside it — is
        // dropped, which is what unhooks a reader still mid-send.
    };

    // Wind down in dependency order, as `run` does: hang up on the writer, let it drain
    // what is in flight, then ask it what landed.
    drop(write_tx);
    let written = writer.await?;
    let _ = reader.join();

    // The writer's error outranks the request error, because a dead writer is *why* the
    // request side's send failed.
    match (written, decoded) {
        (Err(append_error), _) => Err(append_error),
        (_, Err(decode_error)) => Err(decode_error),
        (Ok(writer), Ok(())) => Ok((writer, skipped)),
    }
}

/// One window through the remote embedder: the batch, then chunk-at-a-time for a batch
/// that failed — [`run`]'s recovery, one network hop up.
///
/// The failures that arrive here: a text the provider will not take, a request that
/// outlived its retries, a response that failed verification. One chunk at a time
/// isolates the first kind; a chunk that still fails alone is skipped with its reason
/// on the report rather than allowed to end the run. A refused key is the exception —
/// see [`run_remote`].
async fn embed_window(
    embedder: &RemoteEmbedder,
    window: Vec<String>,
    texts: Vec<String>,
) -> anyhow::Result<EmbeddedWindow> {
    match embedder.embed_documents(&texts).await {
        Ok(vectors) => Ok((window.into_iter().zip(vectors).collect(), Vec::new())),
        Err(batch_error) if remote::is_fatal(&batch_error) => Err(batch_error),
        Err(batch_error) => {
            tracing::debug!(%batch_error, "batch failed; retrying individually");
            let mut entries = Vec::new();
            let mut misses = Vec::new();
            for (hash, text) in window.into_iter().zip(&texts) {
                match embedder.embed_documents(std::slice::from_ref(text)).await {
                    Ok(mut v) => entries.push((hash, v.remove(0))),
                    Err(e) if remote::is_fatal(&e) => return Err(e),
                    Err(e) => misses.push(Skipped {
                        chunk_hash: hash,
                        reason: format!("{e:#}"),
                    }),
                }
            }
            Ok((entries, misses))
        }
    }
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

/// Batches either side of the decode may run ahead: the reader keeps this many read,
/// the writer this many not yet committed. One would do; two absorbs jitter. More buys
/// nothing and widens the gap between the progress count and the table.
const PIPELINE_SLACK: usize = 2;

/// The pipeline: read → decode → write, each on its own thread.
///
/// The GPU must never wait on SQLite or on Lance. A reader thread keeps the next
/// window's text ready, this thread decodes through one standing session, and a writer
/// task writes what the last decode produced — the seconds of inference overlap the
/// milliseconds of I/O instead of following them.
///
/// The durable count is the writer's. Lance commits a version per append, a row's
/// identity is its chunk and document, and the next run subtracts what is stored — so
/// stopping mid-corpus costs nothing but what was in flight. The progress count runs at
/// most [`PIPELINE_SLACK`] batches ahead of the table; the report's figures are what
/// actually landed.
fn run(
    embedder: Embedder,
    index: Index,
    writer: Writer,
    todo: Vec<String>,
    batch_size: usize,
    host: Host,
) -> anyhow::Result<(Writer, Vec<Skipped>)> {
    let Host {
        progress,
        cancel,
        handle,
    } = host;
    // Create the GPU context before starting threads that would otherwise survive
    // a failed allocation without being joined.
    let mut session = embedder.session(SessionOptions {
        batch: batch_size,
        ..SessionOptions::default()
    })?;
    let total = todo.len() as u64;
    let started = Instant::now();
    progress.step("embedding", 0, total);

    // The reader owns the index: a SQLite connection does not share across threads, and
    // nothing else needs it once the work list exists.
    let (feed_tx, feed_rx) =
        std::sync::mpsc::sync_channel::<anyhow::Result<(Vec<String>, Vec<String>)>>(PIPELINE_SLACK);
    let reader = std::thread::spawn(move || {
        for window in todo.chunks(batch_size) {
            let texts = index.chunk_texts(window);
            let failed = texts.is_err();
            // A send fails only when the decode side hung up, and that side already
            // carries whatever error stopped it.
            if feed_tx.send(texts.map(|t| (window.to_vec(), t))).is_err() || failed {
                return;
            }
        }
    });

    let (write_tx, mut write_rx) =
        tokio::sync::mpsc::channel::<Vec<(String, Vec<f32>)>>(PIPELINE_SLACK);
    let writer = handle.spawn(async move {
        let mut writer = writer;
        while let Some(entries) = write_rx.recv().await {
            writer.write(entries).await?;
        }
        anyhow::Ok(writer)
    });

    let mut sent = 0usize;
    let mut skipped: Vec<Skipped> = Vec::new();
    let decoded: anyhow::Result<()> = (|| {
        let writer_gone = || anyhow::anyhow!("the writer stopped; its error follows");
        for item in feed_rx.iter() {
            cancel.check()?;
            let (window, texts) = item?;

            let (entries, misses) = embed_with_recovery(
                &window,
                &texts,
                &mut |texts| session.embed(texts),
                &cancel,
                &progress,
            )?;
            sent += entries.len();
            skipped.extend(misses);
            if !entries.is_empty() {
                write_tx.blocking_send(entries).map_err(|_| writer_gone())?;
            }

            let done = (sent + skipped.len()) as u64;
            let rate = sent as f64 / started.elapsed().as_secs_f64().max(f64::EPSILON);
            progress.step(format!("embedding ({rate:.1}/sec)"), done, total);
        }
        Ok(())
    })();

    // Wind down in dependency order: hang up on the reader, let the writer drain what
    // is in flight, then ask it what landed. Every batch it confirms is durable however
    // this run ended.
    drop(feed_rx);
    drop(write_tx);
    let written = handle.block_on(writer)?;
    let _ = reader.join();

    // The writer's error outranks the decode error, because a dead writer is *why* the
    // decode side's `blocking_send` failed.
    match (written, decoded) {
        (Err(append_error), _) => Err(append_error),
        (_, Err(decode_error)) => Err(decode_error),
        (Ok(writer), Ok(())) => Ok((writer, skipped)),
    }
}

type EmbeddedWindow = (Vec<(String, Vec<f32>)>, Vec<Skipped>);

/// Bisect failed batches so healthy chunks still run together. The explicit stack
/// preserves input order, bounds memory, and checks cancellation between retries.
fn embed_with_recovery(
    hashes: &[String],
    texts: &[String],
    embed: &mut impl FnMut(&[String]) -> anyhow::Result<Vec<Vec<f32>>>,
    cancel: &Cancel,
    progress: &Progress,
) -> anyhow::Result<EmbeddedWindow> {
    let mut pending: Vec<_> = std::iter::once(0..texts.len()).collect();
    let mut entries = Vec::with_capacity(texts.len());
    let mut skipped = Vec::new();
    let mut reported = false;
    while let Some(range) = pending.pop() {
        cancel.check()?;
        match embed(&texts[range.clone()]) {
            Ok(vectors) => {
                anyhow::ensure!(
                    vectors.len() == range.len(),
                    "embedding result count mismatch"
                );
                entries.extend(hashes[range].iter().cloned().zip(vectors));
            }
            Err(error) if range.len() > 1 => {
                if !reported {
                    progress.say(format!("batch failed; retrying smaller groups: {error:#}"));
                    reported = true;
                }
                let middle = range.start + range.len() / 2;
                pending.push(middle..range.end);
                pending.push(range.start..middle);
            }
            Err(error) => skipped.push(Skipped {
                chunk_hash: hashes[range.start].clone(),
                reason: format!("{error:#}"),
            }),
        }
    }
    Ok((entries, skipped))
}

/// Loads the model off the async runtime — it is seconds of blocking file and GPU work.
async fn load_embedder(
    args: &EmbedArgs,
    model_id: &'static str,
    ledger: crate::spend::Ledger,
) -> anyhow::Result<Embedder> {
    let variant = args.variant.clone();
    tokio::task::spawn_blocking(move || {
        let root = models::models_dir()?;
        Embedder::load(&root, model_id, variant.as_deref(), ledger)
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
            let mut figures = vec![
                (self.indexed as u64, "placements indexed"),
                (self.already_embedded as u64, "already stored"),
            ];
            // Only when there were any: a run with nothing to copy has nothing to say
            // here, and a figure printed every run is a figure nobody reads.
            if self.filled > 0 {
                figures.push((self.filled as u64, "filled from stored vectors"));
            }
            figures.push((self.embedded as u64, "chunks embedded"));
            figures.push((self.remaining as u64, "remaining"));
            if self.stale > 0 {
                figures.push((self.stale as u64, "stale"));
                figures.push((self.pruned as u64, "pruned"));
            }
            p.figures(&figures)?;

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
    use crate::vectors::key_of;

    #[test]
    fn a_bad_chunk_does_not_serialize_the_healthy_batch() {
        let texts: Vec<_> = (0..64).map(|i| i.to_string()).collect();
        let mut calls = 0;
        let (entries, skipped) = embed_with_recovery(
            &texts,
            &texts,
            &mut |group| {
                calls += 1;
                anyhow::ensure!(!group.iter().any(|text| text == "17"), "bad chunk");
                Ok(group
                    .iter()
                    .map(|text| vec![text.parse::<f32>().unwrap()])
                    .collect())
            },
            &Cancel::none(),
            &Progress::none(),
        )
        .unwrap();
        assert_eq!(calls, 13, "only the failing half is split at each level");
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].chunk_hash, "17");
        let expected: Vec<_> = texts.iter().filter(|text| *text != "17").cloned().collect();
        assert_eq!(
            entries
                .iter()
                .map(|(hash, _)| hash.clone())
                .collect::<Vec<_>>(),
            expected
        );
        for (hash, vector) in entries {
            assert_eq!(hash.parse::<f32>().unwrap(), vector[0]);
        }
    }

    #[test]
    fn smaller_groups_recover_a_batch_that_exceeds_capacity() {
        let texts: Vec<_> = (0..40).map(|i| i.to_string()).collect();
        let (entries, skipped) = embed_with_recovery(
            &texts,
            &texts,
            &mut |group| {
                anyhow::ensure!(group.len() <= 10, "capacity exceeded");
                Ok(vec![vec![1.0]; group.len()])
            },
            &Cancel::none(),
            &Progress::none(),
        )
        .unwrap();
        assert_eq!(entries.len(), 40);
        assert!(skipped.is_empty());
    }

    #[test]
    fn cancellation_stops_retries_after_a_failed_batch() {
        let texts = vec!["a".to_string(), "b".to_string()];
        let (canceller, cancel) = Cancel::channel();
        let mut calls = 0;
        let result = embed_with_recovery(
            &texts,
            &texts,
            &mut |_| {
                calls += 1;
                canceller.cancel();
                anyhow::bail!("capacity exceeded")
            },
            &cancel,
            &Progress::none(),
        );
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    /// Typed explicitly in every test, because `None` reads whatever config file the
    /// machine running the tests happens to keep.
    fn default_model() -> Option<String> {
        Some("qwen3-embedding-4b".to_string())
    }

    /// The one derived text every test document here is cut from, so an exclusion can
    /// name a document the way the ledger does: source, address, derived blob.
    const DERIVED: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    /// Where `chunk` sits in the document at `resource`, in source `test`.
    fn placement(chunk: &Chunk, resource: &str) -> Placement {
        Placement {
            source: "test".into(),
            resource: resource.into(),
            blob_sha: "0".repeat(64),
            derived_sha: DERIVED.into(),
            ordinal: chunk.ordinal,
            heading: String::new(),
            char_start: chunk.char_start,
            char_end: chunk.char_end,
            observed_at: "2026-01-01T00:00:00Z".into(),
            tool: "test".into(),
            title: None,
        }
    }

    /// The text of the `i`th document in an [`indexed_store`], at `https://example.gov/{i}`.
    fn passage(i: usize) -> Chunk {
        Chunk::new(format!("passage number {i}"), i, String::new(), 0)
    }

    /// A store with `n` chunks indexed, and nothing embedded.
    async fn indexed_store(n: usize) -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let mut index = Index::open(store.index_path()).unwrap();

        for i in 0..n {
            let chunk = passage(i);
            index
                .insert(
                    &chunk,
                    &placement(&chunk, &format!("https://example.gov/{i}")),
                )
                .unwrap();
        }
        (dir, Ctx::new(store))
    }

    /// The document at `https://example.gov/{i}` in an [`indexed_store`].
    fn document(i: usize) -> DocumentId {
        DocumentId {
            source: "test".into(),
            resource: format!("https://example.gov/{i}"),
            derived_sha: DERIVED.into(),
        }
    }

    /// A row for `passage(i)` in its document, with a zero vector of the default width.
    fn stored(i: usize) -> Row {
        Row {
            chunk_hash: passage(i).chunk_hash,
            document: document(i),
            state: DocState::default(),
            vector: vec![0.0; 2560],
        }
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
                ann_index: false,
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
                ann_index: false,
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
                ann_index: false,
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
    ///
    /// And the coverage claim with it. `embed --dry-run` is where "how much of the corpus
    /// has a vector" is answered — `search` used to print a share from a plain chunk
    /// count, which ignored the shapes below — so its figures are proven here against
    /// each of them: a passage carried by two documents is two placements with one
    /// vector; a row for a document the index no longer has is stale; and the second
    /// document's row is filled from the first's without a model.
    #[tokio::test]
    async fn stored_chunks_are_subtracted_from_the_work_list() {
        let (_dir, ctx) = indexed_store(10).await;

        // The same passage under two addresses: one chunk, two placements.
        let shared = Chunk::new("the same notice, carried twice".into(), 0, String::new(), 0);
        let shared_in = |resource: &str| DocumentId {
            source: "test".into(),
            resource: resource.into(),
            derived_sha: DERIVED.into(),
        };
        {
            let mut index = Index::open(ctx.store.require_index().unwrap()).unwrap();
            for resource in [
                "https://example.gov/shared-a",
                "https://example.gov/shared-b",
            ] {
                index
                    .insert(&shared, &placement(&shared, resource))
                    .unwrap();
            }
        }

        // Stored: five passages in their documents, the shared one under its first
        // address, and a row for a document the index never had.
        let table = VectorTable::open(&ctx.store.vectors_db(), "qwen3-embedding-4b", 2560)
            .await
            .unwrap();
        let mut seeded: Vec<Row> = (0..=4).map(stored).collect();
        seeded.push(Row {
            chunk_hash: shared.chunk_hash.clone(),
            document: shared_in("https://example.gov/shared-a"),
            state: DocState::default(),
            vector: vec![0.5; 2560],
        });
        seeded.push(Row {
            document: shared_in("https://example.gov/gone"),
            ..stored(9)
        });
        let lock = VectorLock::take(&ctx.store).await.unwrap();
        table.append(&lock, &seeded).await.unwrap();
        drop(lock);

        let report = embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                dry_run: true,
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();

        // Ten passages plus the shared one twice.
        assert_eq!(report.indexed, 12, "one row per placement");
        assert_eq!(report.already_embedded, 6);
        assert_eq!(
            report.remaining, 6,
            "only placements without a row are work"
        );
        assert_eq!(report.stale, 1, "the row for the vanished document");
        assert_eq!(report.filled, 0, "a plan copies nothing");

        // A run that may embed nothing still fills what it can copy: the shared chunk's
        // second placement gets the first's vector, passage 9 gets the vector the stale
        // row holds for the same chunk, and then the stale row goes.
        let report = embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                limit: Some(0),
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(report.filled, 2);
        assert_eq!(report.embedded, 0);
        assert_eq!(
            report.remaining, 4,
            "the four passages no row holds wait for a model"
        );
        assert_eq!(report.pruned, 1);
        let vectors = table
            .vectors_of(std::slice::from_ref(&shared.chunk_hash))
            .await
            .unwrap();
        assert_eq!(vectors[&shared.chunk_hash], vec![0.5; 2560]);
        assert_eq!(table.len().await.unwrap(), 8);
    }

    /// The field failure end to end: a data file the storage never wrote blocks the
    /// pre-flight scan, the error names `centinel verify`, and after `verify --repair`
    /// the dropped rows are back on the work list rather than left as ghost rows.
    #[tokio::test]
    async fn a_repaired_table_puts_the_dropped_chunks_back_on_the_work_list() {
        let (_dir, ctx) = indexed_store(3).await;

        // Every placement stored, in two fragments; then one fragment's file goes bad.
        let table = VectorTable::open(&ctx.store.vectors_db(), "qwen3-embedding-4b", 2560)
            .await
            .unwrap();
        let lock = VectorLock::take(&ctx.store).await.unwrap();
        table.append(&lock, &[stored(0)]).await.unwrap();
        table.append(&lock, &[stored(1), stored(2)]).await.unwrap();
        drop(lock);
        super::super::verify::tests::damage_a_data_file(&ctx);

        let plan = || EmbedArgs {
            model: default_model(),
            dry_run: true,
            ..Default::default()
        };
        let err = embed(&ctx, plan(), &Progress::none(), &Cancel::none())
            .await
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("centinel verify"),
            "the pre-flight names the way out: {err:#}"
        );

        let repaired = crate::ops::verify(
            &ctx,
            crate::ops::VerifyTableArgs { repair: true },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap()
        .repaired
        .unwrap();
        assert_eq!(repaired.fragments_dropped, 1);

        let report = embed(&ctx, plan(), &Progress::none(), &Cancel::none())
            .await
            .unwrap();
        assert_eq!(report.indexed, 3);
        assert_eq!(report.already_embedded, 3 - repaired.estimated_rows);
        assert_eq!(
            report.remaining, repaired.estimated_rows,
            "the dropped rows are work again, not ghost rows"
        );
        assert_eq!(report.stale, 0);
    }

    /// An index rebuilt and not yet re-indexed holds no placements. That is not a
    /// statement that every row is stale: rows cost hours or money to put back.
    #[tokio::test]
    async fn an_empty_index_prunes_nothing() {
        let (_dir, ctx) = indexed_store(0).await;
        let table = VectorTable::open(&ctx.store.vectors_db(), "qwen3-embedding-4b", 2560)
            .await
            .unwrap();
        let lock = VectorLock::take(&ctx.store).await.unwrap();
        table.append(&lock, &[stored(0), stored(1)]).await.unwrap();
        drop(lock);

        let report = embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(report.stale, 2, "counted, so the report says so");
        assert_eq!(report.pruned, 0);
        let reopened = VectorTable::open_existing(&ctx.store.vectors_db())
            .await
            .unwrap();
        assert_eq!(reopened.len().await.unwrap(), 2);
    }

    /// A page re-extracted to new derived text keeps most of its chunks. Their vectors
    /// are copied off the old derivation's rows before those rows are pruned, so nothing
    /// goes back through a model.
    #[tokio::test]
    async fn a_rederived_page_copies_its_unchanged_chunks_before_the_old_rows_go() {
        let (_dir, ctx) = indexed_store(2).await;
        let table = VectorTable::open(&ctx.store.vectors_db(), "qwen3-embedding-4b", 2560)
            .await
            .unwrap();
        let previous = |i: usize| Row {
            document: DocumentId {
                derived_sha: "2".repeat(64),
                ..document(i)
            },
            ..stored(i)
        };
        let lock = VectorLock::take(&ctx.store).await.unwrap();
        table
            .append(&lock, &[previous(0), previous(1)])
            .await
            .unwrap();
        drop(lock);

        let report = embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                limit: Some(0),
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(report.filled, 2, "both chunks copied from the old rows");
        assert_eq!(report.embedded, 0);
        assert_eq!(report.remaining, 0);
        assert_eq!(report.pruned, 2, "and then the old rows went");
        assert!(!ctx.store.spend_dir().exists(), "no model ran");
    }

    /// The subtraction the other way. A rebuilt index or a cleared source leaves rows
    /// no placement claims: a dry run counts them, a run removes them once nothing is left
    /// to copy off them — and with every placement already stored, the run needs no
    /// weights to show it.
    #[tokio::test]
    async fn stale_vectors_are_counted_by_a_plan_and_pruned_by_a_run() {
        let (_dir, ctx) = indexed_store(3).await;

        let table = VectorTable::open(&ctx.store.vectors_db(), "qwen3-embedding-4b", 2560)
            .await
            .unwrap();
        let mut seeded: Vec<Row> = (0..3).map(stored).collect();
        // Two rows for documents the index does not have: one a page re-extracted to a
        // new derived text, one an address gone altogether.
        seeded.push(Row {
            document: DocumentId {
                derived_sha: "2".repeat(64),
                ..document(0)
            },
            ..stored(0)
        });
        seeded.push(Row {
            document: document(7),
            ..stored(1)
        });
        let lock = VectorLock::take(&ctx.store).await.unwrap();
        table.append(&lock, &seeded).await.unwrap();
        drop(lock);

        let plan = embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                dry_run: true,
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(plan.stale, 2);
        assert_eq!(plan.pruned, 0, "a plan removes nothing");
        assert_eq!(table.len().await.unwrap(), 5);

        let report = embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(report.stale, 2);
        assert_eq!(report.pruned, 2);
        assert_eq!(report.already_embedded, 3);
        assert_eq!(report.embedded, 0);

        let reopened = VectorTable::open(&ctx.store.vectors_db(), "qwen3-embedding-4b", 2560)
            .await
            .unwrap();
        let mut keys = HashSet::new();
        reopened
            .keys(|key| {
                keys.insert(key.to_string());
            })
            .await
            .unwrap();
        assert_eq!(
            keys,
            (0..3)
                .map(|i| key_of(&passage(i).chunk_hash, &document(i)))
                .collect::<HashSet<_>>(),
            "exactly the index's placements remain"
        );
    }

    /// Upgrading a store is `embed --limit 0`: every placement whose chunk the `vectors`
    /// table from before this layout holds is written from it, nothing is sent to a model,
    /// the legacy table is left as it was, and each row carries what the workspace says
    /// about its document — here, one the operator took out of search.
    #[tokio::test]
    async fn a_legacy_table_fills_the_new_one_without_a_model() {
        let (_dir, ctx) = indexed_store(3).await;
        let legacy: Vec<(String, Vec<f32>)> = (0..3)
            .map(|i| {
                let mut vector = vec![0.0; 2560];
                vector[i] = 1.0;
                (passage(i).chunk_hash, vector)
            })
            .collect();
        crate::vectors::tests::legacy_table(&ctx.store.vectors_db(), "qwen3-embedding-4b", &legacy)
            .await;
        Classifications::open(&ctx.store)
            .unwrap()
            .set_usage(&document(1), true, "review")
            .unwrap();

        let report = embed(
            &ctx,
            EmbedArgs {
                model: default_model(),
                limit: Some(0),
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(report.filled, 3);
        assert_eq!(report.embedded, 0);
        assert_eq!(report.remaining, 0);
        assert!(!ctx.store.spend_dir().exists(), "no model ran");

        let table = VectorTable::open_existing(&ctx.store.vectors_db())
            .await
            .unwrap();
        assert_eq!(table.len().await.unwrap(), 3);
        let mut query = vec![0.0; 2560];
        query[1] = 1.0;
        let hits = table
            .nearest(&query, 3, &crate::index::Filter::default())
            .await
            .unwrap();
        assert!(
            hits.iter().all(|h| h.document != document(1)),
            "the excluded document's row was written excluded: {hits:?}"
        );
        assert_eq!(hits.len(), 2);
        assert!(
            crate::vectors::LegacyVectors::open(&ctx.store.vectors_db(), "qwen3-embedding-4b")
                .await
                .unwrap()
                .is_some(),
            "the legacy table is left for the operator to delete"
        );
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
                ann_index: false,
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
                ann_index: false,
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

    /// The model takes `batch`'s route: absent stays absent until the op asks the config.
    /// A serde default here would resurrect the bug where `[defaults] embed_model` was
    /// read and never used, one surface over.
    #[test]
    fn an_absent_model_stays_absent_until_the_config_answers() {
        let parse = |json: &str| serde_json::from_str::<EmbedArgs>(json).unwrap().model;
        assert_eq!(parse("{}"), None);
        assert_eq!(
            parse(r#"{"model": "qwen3-embedding-0.6b"}"#),
            Some("qwen3-embedding-0.6b".into())
        );
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

    /// The remote registry pins dimensions exactly so this works: a dry run plans a
    /// remote embed with no key, no network and no weights.
    #[tokio::test]
    async fn a_remote_dry_run_needs_no_key_and_no_network() {
        let (_d, ctx) = indexed_store(5).await;
        let report = embed(
            &ctx,
            EmbedArgs {
                model: Some("openrouter/qwen/qwen3-embedding-8b".into()),
                dry_run: true,
                ann_index: false,
                ..EmbedArgs::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(report.dims, 4096);
        assert_eq!(report.remaining, 5);
        assert_eq!(report.variant, "openrouter");
    }

    /// A quantization names weights on this machine; a remote model keeps none here.
    #[tokio::test]
    async fn a_variant_on_a_remote_model_is_refused() {
        let (_d, ctx) = indexed_store(1).await;
        let err = embed(
            &ctx,
            EmbedArgs {
                model: Some("openrouter/qwen/qwen3-embedding-8b".into()),
                variant: Some("q8_0".into()),
                dry_run: true,
                ann_index: false,
                ..EmbedArgs::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("--variant"), "{err}");
    }

    #[tokio::test]
    async fn a_reranker_is_refused_as_an_embedding_model() {
        let (_d, ctx) = indexed_store(1).await;
        let err = embed(
            &ctx,
            EmbedArgs {
                model: Some("qwen3-reranker-0.6b".into()),
                variant: None,
                batch: Some(BatchSize::Fixed(8)),
                limit: None,
                dry_run: true,
                ann_index: false,
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
                model: Some("qwen3-embedding-0.6b".into()),
                variant: None,
                batch: Some(BatchSize::Fixed(8)),
                limit: None,
                dry_run: true,
                ann_index: false,
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
