//! `embed` — turn indexed chunks into vectors.
//!
//! The expensive stage. Separate from `index` for the same reason `collect` is separate
//! from `extract`: `index` is minutes and rebuildable, this is hours.
//!
//! Vectors come from the server `[inference.embed]` names (see [`crate::embed`]) and are
//! written straight to [`crate::vectors`], which is where `search` reads them. There is no
//! intermediate cache — see that module for why the one there used to be did not survive
//! being measured.
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
//! for the same chunk, and only then the model. A recrawl that changes a page gives it a
//! new derived text and so new placements, nearly all of them for passages the table
//! already has under the old text; those are copied, and the model sees only what is
//! actually new.
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
//! ## The pipeline
//!
//! A reader thread keeps windows of chunk text coming off SQLite, `concurrency` requests
//! are in flight at once to hide the round trip, and a writer task appends what has come
//! back — so the server waits on neither SQLite nor Lance. How wide a request is and how
//! many are in flight are `[inference.embed] batch` and `concurrency`; how the server
//! packs the work onto its GPU is the server's tuning, not this binary's.
//!
//! ## Moving a table to another server
//!
//! The table records the [`Space`] its vectors are in — the model, and the server that ran
//! it — and refuses rows from any other. `--adopt` is the one way across without
//! re-embedding: it sends a sample of stored chunks to the configured server, compares
//! what comes back with what is stored, and rewrites the record only when every pair
//! agrees past [`ADOPT_MIN_COSINE`]. The same weights behind a new server reproduce their
//! vectors to within rounding; a different model, or the same one at a coarser
//! quantization, does not, and the refusal says what was measured and what to run instead.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context as _;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::InferenceConfig;
use crate::embed::Embedder;
use crate::index::Index;
use crate::inference::{Capability, is_fatal};
use crate::prelude::*;
use crate::vectors::{Row, Space, VectorLock, VectorTable, parse_key};
use crate::workspace::classifications::Classifications;
use crate::workspace::{DocState, DocumentId};

/// Chunks per lookup when a placement's vector is copied from another row of the table.
/// One `IN (…)` predicate each, served by the hash index once `maintain` has built it.
const COPY_WINDOW: usize = 256;

/// Stored chunks `--adopt` asks the new server to reproduce. Enough to catch a different
/// model — which disagrees on every chunk — at the price of one request.
const ADOPT_SAMPLE: usize = 32;

/// Chunks drawn evenly across the index for `--adopt` to look up in the table, of which the
/// first [`ADOPT_SAMPLE`] with a stored vector are sent. Wide, so a partly embedded corpus
/// still yields a full sample; one `IN (…)` lookup, so it costs nothing to be wide.
const ADOPT_CANDIDATES: usize = 1024;

/// The agreement `--adopt` requires on **every** sampled chunk, as cosine similarity.
///
/// The same weights behind another server land at 0.999 and above: pooling, quantization
/// and normalization are the same, and what differs is the order floating-point sums run
/// in. A different model, or the same one at a coarser quantization, lands well under this
/// on most chunks — and a table that ranks 1% of its chunks against the wrong space is
/// worse than one re-embedded.
pub const ADOPT_MIN_COSINE: f32 = 0.99;

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct EmbedArgs {
    /// The model to ask the embedding server for, for this run.
    ///
    /// Unset means "nobody said here", which falls through to `[inference.embed] model`.
    /// Changing it is a new vector space, not an upgrade (SPEC §6.2): the table refuses
    /// the new model's vectors until `--adopt` has measured that they match.
    #[arg(long)]
    #[serde(default)]
    pub model: Option<String>,

    /// Chunks per request. Unset falls through to `[inference.embed] batch`.
    #[arg(long, value_name = "N")]
    #[serde(default)]
    pub batch: Option<usize>,

    /// Stop after this many chunks. The way to sample a corpus before committing hours.
    #[arg(long)]
    #[serde(default)]
    pub limit: Option<usize>,

    /// Report what would be embedded and exit. With `--adopt`, measure and report without
    /// rewriting anything.
    #[arg(long)]
    #[serde(default)]
    pub dry_run: bool,

    /// Build an approximate vector index after embedding. Compare speed and recall
    /// with `search --exact` on representative queries before adopting it.
    #[arg(long)]
    #[serde(default)]
    pub ann_index: bool,

    /// Take the stored vectors over to the configured server: embed a sample of stored
    /// chunks there, and record the server as the table's own if every one matches what is
    /// stored. Refused, with what was measured, if they do not.
    #[arg(long)]
    #[serde(default)]
    pub adopt: bool,
}

/// A chunk that could not be embedded. One failure does not abandon the run.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Skipped {
    pub chunk_hash: String,
    pub reason: String,
}

/// What `--adopt` measured.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Adoption {
    /// The space the table recorded.
    pub from: Space,
    /// The configured server's.
    pub to: Space,
    /// Stored chunks the server was asked to reproduce.
    pub sampled: usize,
    pub min_cosine: f32,
    pub mean_cosine: f32,
    /// Whether the table now records `to`. False on a dry run, and when it already did.
    pub adopted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct EmbedReport {
    pub model: String,
    /// The embedding server's `base_url`.
    pub server: String,
    /// The table's width. Absent on a dry run before any table exists: only the server
    /// knows, and a dry run does not ask it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dims: Option<usize>,
    /// Where the vectors went — `chunks.lance/`.
    pub vectors: std::path::PathBuf,
    /// Chunk placements in the index: one for each chunk in each document that holds it.
    pub indexed: usize,
    /// Placements that already had a row when the run started — the resumed-past portion.
    pub already_embedded: usize,
    /// Placements written this run from a vector the table already held under another
    /// document. Nothing was sent to a model for them.
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
    /// Chunks per request. Reported because it is what makes `chunks_per_sec` mean
    /// anything — the same corpus and the same server give different rates at different
    /// widths. Absent on a dry run and on a run the copies finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adoption: Option<Adoption>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<Skipped>,
}

/// A chunk's hash as 32 bytes and its document as a position in the index's document
/// list: a placement, small enough that a million and a half of them sit in memory.
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
    let inference = crate::config::Config::load()?.inference;
    embed_with(ctx, args, &inference, progress, cancel).await
}

/// [`embed`], against the server `inference` names — `run` passes the config it has open
/// rather than have the file read twice.
pub(crate) async fn embed_with(
    ctx: &Ctx,
    args: EmbedArgs,
    inference: &InferenceConfig,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<EmbedReport> {
    anyhow::ensure!(args.batch != Some(0), "--batch must be at least 1");
    // Before the work list is worked out, so a second run cannot work out the same one.
    let _run = match args.dry_run {
        true => None,
        false => Some(hold_embed_lock(&ctx.store)?),
    };

    let mut endpoint = inference.endpoint(Capability::Embed);
    if let Some(model) = &args.model {
        endpoint = endpoint.with_model(model);
    }
    let wanted = Space::of(&endpoint);
    // Built only where something will be sent, because building one reads the key, and a
    // run that sends nothing — a plan, a copy-only `--limit 0`, a corpus already embedded —
    // must not fail for want of one.
    let embedder = || {
        Embedder::new(
            endpoint.clone(),
            &inference.embed.query_prefix,
            crate::spend::Ledger::new(&ctx.store),
        )
    };

    // Shortest first, so a request carries texts of one size and the long tail arrives
    // together at the end of the run. Said before it runs, not after: on a large corpus it
    // is the first thing a caller waits on.
    progress.say("reading the index");
    let index = Index::open(ctx.store.require_index()?)?;
    let placements = index.placements_by_length()?;

    // Chunks for `--adopt` to compare, drawn from the index — the side that knows which
    // chunks are current — and read now, while the index is to hand: `adopt` awaits, and a
    // SQLite connection is not `Sync`.
    let candidates = match args.adopt {
        true => adoption_candidates(&index, &placements.pairs)?,
        false => Vec::new(),
    };

    // The table says what space it is in. `--adopt` is the one way to change its mind;
    // otherwise another space is refused before anything is sent or written.
    let mut adoption = None;
    let table = match ctx.store.vectors_path().exists() {
        true => {
            let mut existing = VectorTable::open_existing(&ctx.store.vectors_db()).await?;
            if args.adopt {
                let measured = adopt(
                    ctx,
                    &mut existing,
                    &embedder()?,
                    &candidates,
                    args.dry_run,
                    progress,
                )
                .await?;
                adoption = Some(measured);
            }
            // Opened again in the space the table now records, which is what checks the
            // width and the layout too — and refuses another space with the way forward.
            let space = match &adoption {
                Some(a) if !a.adopted => existing.space().clone(),
                _ => wanted.clone(),
            };
            Some(VectorTable::open(&ctx.store.vectors_db(), &space, existing.dims()).await?)
        }
        false => {
            // Before anything is embedded beside it, a store whose vectors are in the old
            // layout is told what to do with them.
            crate::vectors::refuse_old_layout(&ctx.store.vectors_db()).await?;
            anyhow::ensure!(
                !args.adopt,
                "there are no stored vectors to adopt — run `centinel embed` without --adopt"
            );
            None
        }
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
        model: endpoint.model.clone(),
        server: endpoint.base_url.clone(),
        dims: table.as_ref().map(VectorTable::dims),
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
        adoption,
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
        None => {
            // A new table needs a width before its first row, and only the server knows it.
            progress.say(format!("asking {endpoint} for its width"));
            let dims = embedder()?.probe_dims().await?;
            VectorTable::open(&ctx.store.vectors_db(), &wanted, dims).await?
        }
    };
    let dims = table.dims();
    let base = EmbedReport {
        dims: Some(dims),
        ..base
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

    // Copies first: a chunk the table already holds under another document. No model is
    // asked anything for them.
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

    let batch = args.batch.unwrap_or(inference.embed.batch);
    // Said before the first byte moves: where chunk text is going, and how.
    progress.say(format!(
        "{endpoint} — {batch} chunks per request, {} in flight",
        inference.embed.concurrency
    ));
    let chunks_before = writer.chunks;
    let (writer, skipped) = run(
        embedder()?.with_dims(dims),
        index,
        writer,
        todo,
        Pipeline {
            batch,
            in_flight: inference.embed.concurrency,
        },
        progress,
        cancel,
    )
    .await?;

    cancel.check()?;
    progress.say("maintaining the vector table");
    let done = maintain(&ctx.store, &table, args.ann_index).await?;
    crate::workspace::sync_vectors(&ctx.store).await?;
    Ok(finish(
        &writer,
        writer.chunks - chunks_before,
        skipped,
        Some(batch),
        done,
    ))
}

/// Measures whether `embedder` reproduces the stored vectors, and when every sampled pair
/// agrees past [`ADOPT_MIN_COSINE`], records its space as the table's — unless this is a
/// dry run, which measures and reports and writes nothing.
///
/// Refused with the figures and with what to run instead, because a refusal here is the
/// moment somebody decides between starting the right server and re-embedding for hours.
/// Up to [`ADOPT_CANDIDATES`] distinct chunks, evenly spaced through the index's work order
/// — which runs shortest first, so the sample spans every length — with their text.
fn adoption_candidates(
    index: &Index,
    pairs: &[(String, u32)],
) -> anyhow::Result<Vec<(String, String)>> {
    let step = (pairs.len() / ADOPT_CANDIDATES).max(1);
    let mut seen = HashSet::new();
    let hashes: Vec<String> = pairs
        .iter()
        .step_by(step)
        .map(|(hash, _)| hash.clone())
        .filter(|hash| seen.insert(hash.clone()))
        .take(ADOPT_CANDIDATES)
        .collect();
    let mut texts = index.texts_of(&hashes)?;
    Ok(hashes
        .into_iter()
        .filter_map(|hash| texts.remove(&hash).map(|text| (hash, text)))
        .collect())
}

async fn adopt(
    ctx: &Ctx,
    table: &mut VectorTable,
    embedder: &Embedder,
    candidates: &[(String, String)],
    dry_run: bool,
    progress: &Progress,
) -> anyhow::Result<Adoption> {
    let from = table.space().clone();
    let to = embedder.space();
    if from == to {
        progress.say(format!("the table already records {to}"));
        return Ok(Adoption {
            from,
            to,
            sampled: 0,
            min_cosine: 1.0,
            mean_cosine: 1.0,
            adopted: false,
        });
    }

    // The current chunks the table holds a vector for, in candidate order.
    let hashes: Vec<String> = candidates.iter().map(|(hash, _)| hash.clone()).collect();
    let stored = table.vectors_of(&hashes).await?;
    let pairs: Vec<(String, Vec<f32>)> = candidates
        .iter()
        .filter_map(|(hash, text)| stored.get(hash).map(|v| (text.clone(), v.clone())))
        .take(ADOPT_SAMPLE)
        .collect();
    anyhow::ensure!(
        !pairs.is_empty(),
        "none of {} chunks drawn across the index has a stored vector, so there is nothing \
         to compare — delete {} and re-run `centinel embed`",
        candidates.len(),
        ctx.store.vectors_path().display()
    );

    progress.say(format!(
        "asking {} to reproduce {} stored chunks",
        embedder.endpoint(),
        pairs.len()
    ));
    let texts: Vec<&str> = pairs.iter().map(|(text, _)| text.as_str()).collect();
    let fresh = embedder
        .embed_documents(&texts)
        .await
        .map_err(|e| e.context(adopt_refusal(&from, &to, None)))?;
    anyhow::ensure!(
        fresh.len() == pairs.len(),
        "the server answered {} vectors for {} chunks",
        fresh.len(),
        pairs.len()
    );
    if let Some(vector) = fresh.first()
        && vector.len() != table.dims()
    {
        anyhow::bail!(
            "{} answers {}-dimensional vectors and the table holds {} — {}",
            embedder.endpoint(),
            vector.len(),
            table.dims(),
            adopt_refusal(&from, &to, None)
        );
    }

    let cosines: Vec<f32> = pairs
        .iter()
        .zip(&fresh)
        .map(|((_, stored), fresh)| crate::embed::cosine(stored, fresh))
        .collect();
    let min_cosine = cosines.iter().copied().fold(f32::INFINITY, f32::min);
    let mean_cosine = cosines.iter().sum::<f32>() / cosines.len() as f32;
    if min_cosine < ADOPT_MIN_COSINE {
        anyhow::bail!(
            "{}",
            adopt_refusal(&from, &to, Some((pairs.len(), min_cosine, mean_cosine)))
        );
    }

    let adopted = !dry_run;
    if adopted {
        let lock = VectorLock::take(&ctx.store).await?;
        table.adopt(&lock, &to).await?;
    }
    Ok(Adoption {
        from,
        to,
        sampled: pairs.len(),
        min_cosine,
        mean_cosine,
        adopted,
    })
}

/// Why `--adopt` said no, and what would have it say yes.
///
/// The way forward depends on where the stored vectors came from, and the table says:
/// a server it names can be pointed at again; an `openrouter/…` id is OpenRouter's API;
/// a bare id is weights the in-process embedder ran, which llama-server runs the same way.
fn adopt_refusal(from: &Space, to: &Space, measured: Option<(usize, f32, f32)>) -> String {
    let mut out = match measured {
        Some((n, min, mean)) => format!(
            "{to} does not reproduce the vectors {from} wrote: across {n} stored chunks the \
             agreement was {min:.4} at worst and {mean:.4} on average, and adopting needs \
             {ADOPT_MIN_COSINE} on every one."
        ),
        None => format!("{to} cannot stand in for {from}."),
    };
    let way = match (&from.endpoint, from.model.strip_prefix("openrouter/")) {
        (Some(endpoint), _) => format!(
            "Point [inference.embed] back at the server that wrote them:\n\n{}",
            crate::config::embed_table(Some(endpoint), None, &from.model)
        ),
        (None, Some(slug)) => format!(
            "They came from OpenRouter. To keep searching them there:\n\n{}",
            crate::config::embed_table(
                Some(crate::config::OPENROUTER_BASE_URL),
                Some(crate::config::OPENROUTER_KEY),
                slug,
            )
        ),
        (None, None) => match from.model.as_str() {
            "qwen3-embedding-4b" | "qwen3-embedding-0.6b" => {
                let size = if from.model.ends_with("4b") {
                    "4B"
                } else {
                    "0.6B"
                };
                format!(
                    "They came from the in-process embedder, which ran the Q8_0 GGUF with \
                     last-token pooling. llama-server runs the same weights the same way:\n\n\
                     llama-server -hf Qwen/Qwen3-Embedding-{size}-GGUF:Q8_0 --embedding \
                     --pooling last --ubatch-size 2048 --batch-size 2048\n\n\
                     then point [inference.embed] at it and run `centinel embed --adopt` again."
                )
            }
            other => format!(
                "They came from `{other}`, run in-process. Serve those weights with last-token \
                 pooling and run `centinel embed --adopt` again."
            ),
        },
    };
    out.push_str("\n\n");
    out.push_str(&way);
    out.push_str(
        "\n\nOr start over in the new space: delete chunks.lance and run `centinel embed`.",
    );
    out
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

/// How a run's requests are shaped: chunks per request, and requests in flight.
struct Pipeline {
    batch: usize,
    in_flight: usize,
}

/// Windows either side of the requests may run ahead: the reader keeps this many read,
/// the writer this many not yet committed. One would do; two absorbs jitter. More buys
/// nothing and widens the gap between the progress count and the table.
const PIPELINE_SLACK: usize = 2;

/// The pipeline: read → request → write.
///
/// The reader thread keeps windows of text coming off SQLite, `in_flight` requests overlap
/// the server's latency, and the writer task writes what has come back. `buffered`
/// completes in submission order, so the writer receives windows in work-list order.
///
/// The durable count is the writer's. Lance commits a version per append, a row's
/// identity is its chunk and document, and the next run subtracts what is stored — so
/// stopping mid-corpus costs nothing but what was in flight. The progress count runs at
/// most a few windows ahead of the table; the report's figures are what actually landed.
async fn run(
    embedder: Embedder,
    index: Index,
    writer: Writer,
    todo: Vec<String>,
    shape: Pipeline,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<(Writer, Vec<Skipped>)> {
    use futures::StreamExt;

    let total = todo.len() as u64;
    let started = Instant::now();
    progress.step("embedding", 0, total);

    // The reader owns the index: a SQLite connection does not share across threads, and
    // nothing else needs it once the work list exists.
    let (feed_tx, feed_rx) =
        tokio::sync::mpsc::channel::<anyhow::Result<(Vec<String>, Vec<String>)>>(PIPELINE_SLACK);
    let batch = shape.batch;
    let reader = std::thread::spawn(move || {
        for window in todo.chunks(batch) {
            let texts = index.chunk_texts(window);
            let failed = texts.is_err();
            // A send fails only when the request side hung up, and that side already
            // carries whatever error stopped it.
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

    let requested: anyhow::Result<()> = {
        let embedder = &embedder;
        let windows = futures::stream::unfold(feed_rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });
        let mut results = std::pin::pin!(
            windows
                .map(move |item| async move {
                    let (window, texts) = item?;
                    embed_with_recovery(
                        &window,
                        &texts,
                        |group| async move { embedder.embed_documents(&group).await },
                        cancel,
                        progress,
                    )
                    .await
                })
                .buffered(shape.in_flight)
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

    // Wind down in dependency order: hang up on the writer, let it drain what is in
    // flight, then ask it what landed. Every window it confirms is durable however this
    // run ended.
    drop(write_tx);
    let written = writer.await?;
    let _ = reader.join();

    // The writer's error outranks the request error, because a dead writer is *why* the
    // request side's send failed.
    match (written, requested) {
        (Err(append_error), _) => Err(append_error),
        (_, Err(request_error)) => Err(request_error),
        (Ok(writer), Ok(())) => Ok((writer, skipped)),
    }
}

type EmbeddedWindow = (Vec<(String, Vec<f32>)>, Vec<Skipped>);

/// Bisects a failed window so healthy chunks still go in groups: a window of 64 with one
/// chunk the server will not take costs a dozen requests, not 64. The explicit stack
/// preserves input order, bounds memory, and checks cancellation between retries.
///
/// A [`crate::inference::Fatal`] failure is not bisected: a server that is not there, or
/// that refuses the key, refuses every half the same way, and the run ends on it.
async fn embed_with_recovery<F, Fut>(
    hashes: &[String],
    texts: &[String],
    embed: F,
    cancel: &Cancel,
    progress: &Progress,
) -> anyhow::Result<EmbeddedWindow>
where
    F: Fn(Vec<String>) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<Vec<f32>>>>,
{
    let mut pending: Vec<_> = std::iter::once(0..texts.len()).collect();
    let mut entries = Vec::with_capacity(texts.len());
    let mut skipped = Vec::new();
    let mut reported = false;
    while let Some(range) = pending.pop() {
        cancel.check()?;
        match embed(texts[range.clone()].to_vec()).await {
            Ok(vectors) => {
                anyhow::ensure!(
                    vectors.len() == range.len(),
                    "embedding result count mismatch"
                );
                entries.extend(hashes[range].iter().cloned().zip(vectors));
            }
            Err(error) if is_fatal(&error) => return Err(error),
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
        let detail = match self.dims {
            Some(dims) => format!("{} · {dims} dims", self.server),
            None => self.server.clone(),
        };
        p.title(&self.model, &detail)?;
        p.nest(|p| {
            // First, because it decides whether anything below happened in the space the
            // reader thinks it did.
            if let Some(a) = &self.adoption {
                let measured = format!(
                    "{} chunks agree {:.4} at worst, {:.4} on average",
                    a.sampled, a.min_cosine, a.mean_cosine
                );
                let line = match (a.adopted, a.sampled) {
                    (_, 0) => format!("the table already records {}", a.to),
                    (true, _) => format!("adopted from {} — {measured}", a.from),
                    (false, _) => format!("would adopt from {} — {measured}", a.from),
                };
                p.marked(Mark::Ok, line)?;
                p.blank()?;
            }
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
                rate.push_str(&format!(" \u{00b7} {batch} per request"));
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

    /// A request answered at once with what `answer` says of the group.
    fn ready(
        answer: anyhow::Result<Vec<Vec<f32>>>,
    ) -> std::future::Ready<anyhow::Result<Vec<Vec<f32>>>> {
        std::future::ready(answer)
    }

    #[tokio::test]
    async fn a_bad_chunk_does_not_serialize_the_healthy_batch() {
        let texts: Vec<_> = (0..64).map(|i| i.to_string()).collect();
        let calls = std::cell::Cell::new(0);
        let (entries, skipped) = embed_with_recovery(
            &texts,
            &texts,
            |group| {
                calls.set(calls.get() + 1);
                ready(if group.iter().any(|text| text == "17") {
                    Err(anyhow::anyhow!("bad chunk"))
                } else {
                    Ok(group
                        .iter()
                        .map(|text| vec![text.parse::<f32>().unwrap()])
                        .collect())
                })
            },
            &Cancel::none(),
            &Progress::none(),
        )
        .await
        .unwrap();
        assert_eq!(
            calls.get(),
            13,
            "only the failing half is split at each level"
        );
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

    #[tokio::test]
    async fn smaller_groups_recover_a_batch_that_exceeds_capacity() {
        let texts: Vec<_> = (0..40).map(|i| i.to_string()).collect();
        let (entries, skipped) = embed_with_recovery(
            &texts,
            &texts,
            |group| {
                ready(if group.len() <= 10 {
                    Ok(vec![vec![1.0]; group.len()])
                } else {
                    Err(anyhow::anyhow!("capacity exceeded"))
                })
            },
            &Cancel::none(),
            &Progress::none(),
        )
        .await
        .unwrap();
        assert_eq!(entries.len(), 40);
        assert!(skipped.is_empty());
    }

    #[tokio::test]
    async fn cancellation_stops_retries_after_a_failed_batch() {
        let texts = vec!["a".to_string(), "b".to_string()];
        let (canceller, cancel) = Cancel::channel();
        let calls = std::cell::Cell::new(0);
        let result = embed_with_recovery(
            &texts,
            &texts,
            |_| {
                calls.set(calls.get() + 1);
                canceller.cancel();
                ready(Err(anyhow::anyhow!("capacity exceeded")))
            },
            &cancel,
            &Progress::none(),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
    }

    /// A server that is not there refuses every half the same way. Bisecting it would be
    /// a request per chunk before the run admitted it; it ends on the first.
    #[tokio::test]
    async fn a_fatal_failure_is_not_bisected() {
        let texts: Vec<_> = (0..64).map(|i| i.to_string()).collect();
        let calls = std::cell::Cell::new(0);
        let err = embed_with_recovery(
            &texts,
            &texts,
            |_| {
                calls.set(calls.get() + 1);
                ready(Err(
                    crate::inference::Fatal("connection refused".into()).into()
                ))
            },
            &Cancel::none(),
            &Progress::none(),
        )
        .await
        .unwrap_err();
        assert!(is_fatal(&err));
        assert_eq!(calls.get(), 1);
    }

    /// The op against the default `[inference]`, never the config file of whatever
    /// machine runs the tests. Shadows [`super::embed`] so every test below reads the
    /// same; the ones that reach a server use [`embed_on`].
    async fn embed(
        ctx: &Ctx,
        args: EmbedArgs,
        progress: &Progress,
        cancel: &Cancel,
    ) -> anyhow::Result<EmbedReport> {
        embed_with(ctx, args, &InferenceConfig::default(), progress, cancel).await
    }

    /// The op against `server`.
    async fn embed_on(
        ctx: &Ctx,
        server: &crate::inference::fake::FakeServer,
        args: EmbedArgs,
    ) -> anyhow::Result<EmbedReport> {
        embed_with(
            ctx,
            args,
            &server.config(),
            &Progress::none(),
            &Cancel::none(),
        )
        .await
    }

    /// The space the default `[inference]` embeds in, which the tables below are seeded in.
    fn space() -> Space {
        Space::of(&InferenceConfig::default().endpoint(Capability::Embed))
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
    async fn a_dry_run_reports_the_work_without_reaching_the_server() {
        let (_d, ctx) = indexed_store(7).await;
        let report = embed(
            &ctx,
            EmbedArgs {
                batch: None,
                limit: None,
                dry_run: true,
                ann_index: false,
                ..Default::default()
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
        assert_eq!(
            report.dims, None,
            "only the server knows, and a plan does not ask"
        );
    }

    /// The dry run must work with no server at all — it exists to plan a run before
    /// committing to one.
    #[tokio::test]
    async fn a_dry_run_needs_no_server() {
        let (_d, ctx) = indexed_store(3).await;
        let report = embed_with(
            &ctx,
            EmbedArgs {
                batch: Some(8),
                dry_run: true,
                ..Default::default()
            },
            &crate::inference::fake::nothing_listening(),
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
                batch: None,
                limit: None,
                dry_run: false,
                ann_index: false,
                ..Default::default()
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
        let table = VectorTable::open(&ctx.store.vectors_db(), &space(), 2560)
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
        // Read through a fresh handle: the one above was opened before the run wrote.
        let reopened = VectorTable::open_existing(&ctx.store.vectors_db())
            .await
            .unwrap();
        assert_eq!(reopened.len().await.unwrap(), 8);
    }

    /// The field failure end to end: a data file the storage never wrote blocks the
    /// pre-flight scan, the error names `centinel verify`, and after `verify --repair`
    /// the dropped rows are back on the work list rather than left as ghost rows.
    #[tokio::test]
    async fn a_repaired_table_puts_the_dropped_chunks_back_on_the_work_list() {
        let (_dir, ctx) = indexed_store(3).await;

        // Every placement stored, in two fragments; then one fragment's file goes bad.
        let table = VectorTable::open(&ctx.store.vectors_db(), &space(), 2560)
            .await
            .unwrap();
        let lock = VectorLock::take(&ctx.store).await.unwrap();
        table.append(&lock, &[stored(0)]).await.unwrap();
        table.append(&lock, &[stored(1), stored(2)]).await.unwrap();
        drop(lock);
        super::super::verify::tests::damage_a_data_file(&ctx);

        let plan = || EmbedArgs {
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
        let table = VectorTable::open(&ctx.store.vectors_db(), &space(), 2560)
            .await
            .unwrap();
        let lock = VectorLock::take(&ctx.store).await.unwrap();
        table.append(&lock, &[stored(0), stored(1)]).await.unwrap();
        drop(lock);

        let report = embed(
            &ctx,
            EmbedArgs {
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
        let table = VectorTable::open(&ctx.store.vectors_db(), &space(), 2560)
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
    /// server to show it.
    #[tokio::test]
    async fn stale_vectors_are_counted_by_a_plan_and_pruned_by_a_run() {
        let (_dir, ctx) = indexed_store(3).await;

        let table = VectorTable::open(&ctx.store.vectors_db(), &space(), 2560)
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

        let reopened = VectorTable::open(&ctx.store.vectors_db(), &space(), 2560)
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

    /// A dry run is a plan, so it must not leave a table behind on a store that had
    /// none — the same rule `doctor` follows for the models directory.
    #[tokio::test]
    async fn a_dry_run_creates_no_table() {
        let (_d, ctx) = indexed_store(3).await;
        embed(
            &ctx,
            EmbedArgs {
                batch: Some(8),
                limit: None,
                dry_run: true,
                ann_index: false,
                ..Default::default()
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
                batch: Some(0),
                limit: None,
                dry_run: true,
                ann_index: false,
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("--batch"), "{err}");
    }

    /// The MCP path, where args arrive as JSON rather than as typed words.
    #[test]
    fn args_take_the_batch_as_a_number_and_leave_it_unset_otherwise() {
        let parse = |json: &str| serde_json::from_str::<EmbedArgs>(json).unwrap().batch;
        assert_eq!(parse(r#"{"batch": 64}"#), Some(64));
        assert_eq!(parse("{}"), None, "unset falls through to the config");
    }

    /// The model takes `batch`'s route: absent stays absent until the op asks the config.
    /// A serde default here would resurrect the bug where the config's model was read and
    /// never used, one surface over.
    #[test]
    fn an_absent_model_stays_absent_until_the_config_answers() {
        let parse = |json: &str| serde_json::from_str::<EmbedArgs>(json).unwrap().model;
        assert_eq!(parse("{}"), None);
        assert_eq!(
            parse(r#"{"model": "qwen3-embedding-0.6b"}"#),
            Some("qwen3-embedding-0.6b".into())
        );
    }

    /// One table, so switching models is refused rather than silently mixed. A vector
    /// from another model is in another space and still returns a ranked list, which is
    /// why the refusal has to happen before anything is written (§6.2).
    #[tokio::test]
    async fn a_second_model_is_refused_rather_than_mixed_in() {
        let (_d, ctx) = indexed_store(2).await;
        VectorTable::open(&ctx.store.vectors_db(), &space(), 2560)
            .await
            .unwrap();

        let err = embed(
            &ctx,
            EmbedArgs {
                model: Some("qwen3-embedding-0.6b".into()),
                dry_run: true,
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("not comparable"), "{err}");
        assert!(
            err.contains("centinel embed --adopt"),
            "names the fix: {err}"
        );
    }

    // ── against a server ──────────────────────────────────────────────────────

    /// The whole stage: the server's width makes the table, every chunk goes through it,
    /// the table records the server, and a second run has nothing to send.
    #[tokio::test]
    async fn a_run_sends_the_work_to_the_server_and_stores_what_comes_back() {
        let server = crate::inference::fake::serve().await;
        let (_d, ctx) = indexed_store(5).await;

        let report = embed_on(&ctx, &server, EmbedArgs::default()).await.unwrap();
        assert_eq!(report.embedded, 5);
        assert_eq!(report.remaining, 0);
        assert_eq!(report.dims, Some(crate::inference::fake::DIMS));
        assert_eq!(report.server, server.base_url);

        let table = VectorTable::open_existing(&ctx.store.vectors_db())
            .await
            .unwrap();
        assert_eq!(
            table.space(),
            &Space::of(&server.endpoint(Capability::Embed))
        );
        // Each passage's row holds its own vector, not a neighbour's: the fake answers
        // rows in reverse, so a client trusting row order would cross them.
        let own =
            crate::embed::normalize(&crate::inference::fake::bag_of_words("passage number 3", 0));
        let stored = table
            .vectors_of(std::slice::from_ref(&passage(3).chunk_hash))
            .await
            .unwrap();
        assert!(crate::embed::cosine(&own, &stored[&passage(3).chunk_hash]) > 0.9999);

        let again = embed_on(&ctx, &server, EmbedArgs::default()).await.unwrap();
        assert_eq!(again.embedded, 0);
        assert_eq!(again.already_embedded, 5);
        assert!(
            ctx.store.spend_dir().exists(),
            "the server's answers are in the ledger"
        );
    }

    /// No server is a stage that fails once, loudly, naming it — not one that skips the
    /// corpus a chunk at a time and reports success.
    #[tokio::test]
    async fn no_server_ends_the_run_and_says_where_it_looked() {
        let (_d, ctx) = indexed_store(5).await;
        let config = crate::inference::fake::nothing_listening();
        let err = embed_with(
            &ctx,
            EmbedArgs::default(),
            &config,
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap_err();
        assert!(is_fatal(&err), "{err:#}");
        assert!(format!("{err:#}").contains(&config.base_url), "{err:#}");
    }

    /// An outage that does not end stops the stage after a bounded number of requests,
    /// rather than splitting every batch down to single chunks and skipping the corpus one
    /// at a time.
    #[tokio::test]
    async fn a_server_that_stays_down_ends_the_run_after_three_attempts() {
        let server = crate::inference::fake::serve_failing(503).await;
        let (_d, ctx) = indexed_store(5).await;
        seed(&ctx, 1, &Space::of(&server.endpoint(Capability::Embed)), 0).await;

        let err = embed_on(&ctx, &server, EmbedArgs::default())
            .await
            .unwrap_err();
        assert!(is_fatal(&err), "{err:#}");
        assert!(format!("{err:#}").contains("503"), "{err:#}");
        assert_eq!(
            server.hits("embeddings"),
            3,
            "one batch, three attempts, no split"
        );
    }

    /// Stores vectors for every passage in an [`indexed_store`] as `salt`'s fake model
    /// embeds them, under `space`.
    async fn seed(ctx: &Ctx, n: usize, space: &Space, salt: u64) {
        let table = VectorTable::open(&ctx.store.vectors_db(), space, crate::inference::fake::DIMS)
            .await
            .unwrap();
        let rows: Vec<Row> = (0..n)
            .map(|i| Row {
                vector: crate::embed::normalize(&crate::inference::fake::bag_of_words(
                    &format!("passage number {i}"),
                    salt,
                )),
                ..stored(i)
            })
            .collect();
        let lock = VectorLock::take(&ctx.store).await.unwrap();
        table.append(&lock, &rows).await.unwrap();
    }

    /// The migration this flag exists for: a table the in-process embedder wrote, and a
    /// server running the same model. Measured, adopted, and embedding carries on into the
    /// same table with nothing re-embedded.
    #[tokio::test]
    async fn a_server_that_reproduces_the_table_adopts_it() {
        let server = crate::inference::fake::serve().await;
        let (_d, ctx) = indexed_store(6).await;
        let in_process = Space {
            model: "qwen3-embedding-4b".into(),
            endpoint: None,
        };
        seed(&ctx, 4, &in_process, 0).await;

        // Without the flag, the new server is another space and is refused.
        let refused = embed_on(&ctx, &server, EmbedArgs::default())
            .await
            .unwrap_err()
            .to_string();
        assert!(refused.contains("--adopt"), "{refused}");

        // A dry run measures and writes nothing.
        let plan = embed_on(
            &ctx,
            &server,
            EmbedArgs {
                adopt: true,
                dry_run: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let measured = plan.adoption.unwrap();
        assert!(!measured.adopted);
        assert_eq!(measured.sampled, 4);
        assert!(measured.min_cosine > 0.9999, "{measured:?}");
        let table = VectorTable::open_existing(&ctx.store.vectors_db())
            .await
            .unwrap();
        assert_eq!(table.space(), &in_process, "a dry run rewrites nothing");

        let report = embed_on(
            &ctx,
            &server,
            EmbedArgs {
                adopt: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(report.adoption.unwrap().adopted);
        assert_eq!(report.already_embedded, 4, "nothing stored was re-embedded");
        assert_eq!(
            report.embedded, 2,
            "and the rest went to the adopted server"
        );
        let table = VectorTable::open_existing(&ctx.store.vectors_db())
            .await
            .unwrap();
        assert_eq!(
            table.space(),
            &Space::of(&server.endpoint(Capability::Embed))
        );
    }

    /// A server running another model is refused with what was measured and with the
    /// command that serves the model the table came from — and the table is left as it was.
    #[tokio::test]
    async fn a_server_in_another_space_is_refused_with_the_way_back() {
        let other = crate::inference::fake::serve_salted(1).await;
        let (_d, ctx) = indexed_store(4).await;
        let in_process = Space {
            model: "qwen3-embedding-4b".into(),
            endpoint: None,
        };
        seed(&ctx, 4, &in_process, 0).await;

        let err = embed_on(
            &ctx,
            &other,
            EmbedArgs {
                adopt: true,
                ..Default::default()
            },
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("at worst"), "the figures: {err}");
        assert!(
            err.contains("llama-server -hf Qwen/Qwen3-Embedding-4B-GGUF:Q8_0"),
            "the way back: {err}"
        );
        assert!(
            err.contains("delete chunks.lance"),
            "the way forward: {err}"
        );

        let table = VectorTable::open_existing(&ctx.store.vectors_db())
            .await
            .unwrap();
        assert_eq!(table.space(), &in_process);
    }

    /// A table OpenRouter wrote names OpenRouter as the way back, in the config to paste.
    #[test]
    fn an_openrouter_table_is_pointed_back_at_openrouter() {
        let from = Space {
            model: "openrouter/qwen/qwen3-embedding-8b".into(),
            endpoint: None,
        };
        let to = space();
        let text = adopt_refusal(&from, &to, Some((32, 0.41, 0.63)));

        // The block is pasted as it is printed, so it has to parse — the query prefix
        // included, which a written `[inference.embed]` must state.
        let block = text
            .split("\n\n")
            .find(|p| p.starts_with("[inference.embed]"))
            .unwrap();
        let parsed = crate::config::Config::parse(block).unwrap().inference;
        assert_eq!(parsed.embed.model, "qwen/qwen3-embedding-8b");
        assert_eq!(
            parsed.embed.base_url.as_deref(),
            Some("https://openrouter.ai/api/v1")
        );
        assert_eq!(
            parsed.embed.api_key_env.as_deref(),
            Some("OPENROUTER_API_KEY")
        );
        assert_eq!(parsed.embed.query_prefix, crate::embed::QWEN3_QUERY_PREFIX);
    }

    /// Adoption asks the index which chunks are current, so rows the table still holds for
    /// chunks a re-index dropped cannot crowd out the ones that compare.
    #[tokio::test]
    async fn stale_rows_do_not_starve_an_adoption_sample() {
        let server = crate::inference::fake::serve().await;
        let (_d, ctx) = indexed_store(3).await;
        let in_process = Space {
            model: "qwen3-embedding-4b".into(),
            endpoint: None,
        };
        let table = VectorTable::open(
            &ctx.store.vectors_db(),
            &in_process,
            crate::inference::fake::DIMS,
        )
        .await
        .unwrap();
        let stale: Vec<Row> = (100..300)
            .map(|i| Row {
                vector: vec![0.5; crate::inference::fake::DIMS],
                ..stored(i)
            })
            .collect();
        let lock = VectorLock::take(&ctx.store).await.unwrap();
        table.append(&lock, &stale).await.unwrap();
        drop(lock);
        seed(&ctx, 3, &in_process, 0).await;

        let plan = embed_on(
            &ctx,
            &server,
            EmbedArgs {
                adopt: true,
                dry_run: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(plan.adoption.unwrap().sampled, 3);
    }

    /// A run with nothing to send needs no key: the key is read where a request is made.
    #[tokio::test]
    async fn a_run_that_sends_nothing_needs_no_key() {
        let (_d, ctx) = indexed_store(2).await;
        let table = VectorTable::open(&ctx.store.vectors_db(), &space(), 2560)
            .await
            .unwrap();
        let lock = VectorLock::take(&ctx.store).await.unwrap();
        table.append(&lock, &[stored(0), stored(1)]).await.unwrap();
        drop(lock);

        let config = InferenceConfig {
            api_key_env: Some("CENTINEL_TEST_KEY_THAT_IS_NEVER_SET".into()),
            ..Default::default()
        };
        let report = embed_with(
            &ctx,
            EmbedArgs::default(),
            &config,
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(report.already_embedded, 2);
    }
}
