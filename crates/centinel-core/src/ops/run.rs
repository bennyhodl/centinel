//! `run` — the whole pipeline, over every configured source.
//!
//! The other ops are the stages of one process, and typing them in order is a chore that
//! also has to be got right: `index` before `extract` silently indexes nothing. This op
//! is that order, written down once, driven by `centinel.toml`.
//!
//! ## Incremental is inherited, not implemented
//!
//! Nothing here diffs anything. Every stage already skips work it has done — `collect`
//! subtracts observed resources from the latest DiscoveryRun, `extract` skips blobs that
//! already have a derivation, `index` skips derivations already chunked in, and `embed`
//! subtracts cached chunk hashes from indexed ones. Each falls out of the append-only log
//! or the content-addressed cache rather than from a checkpoint file (SPEC §5.2, §6.1).
//!
//! So a second run does nothing, at every stage, for the same structural reason the first
//! one was resumable. That is what makes this the cron command: twice a day costs one
//! sitemap walk per source plus whatever genuinely changed.
//!
//! ## Two phases, because model loads dominate
//!
//! ```text
//!   per source   discover → collect          (network-bound, per-host paced)
//!   then once    extract → transcribe → index → embed
//! ```
//!
//! Acquisition is per source because politeness is per host and a 403 on one site must
//! not stop the next. Derivation is corpus-wide because `transcribe` and `embed` each
//! build a multi-gigabyte model, and doing that once beats doing it per source — with
//! twenty sources the naive chaining spends more time loading weights than embedding.
//!
//! It also fixes an ordering hazard for free: `index` runs after *every* source has
//! extracted, so a chunk that appears in two sources is placed against both.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::acquire::{self, CollectOpts, DiscoverOpts};
use crate::config::Config;
use crate::op::TOTAL_TRACK;
use crate::prelude::*;
use crate::sources::{self, Overrides};

use super::{EmbedArgs, ExtractArgs, IndexArgs, TranscribeArgs};

/// One step of the pipeline.
///
/// `discover` and `collect` name what happens, not how: for a website they are a sitemap
/// walk and HTTP GETs, for a channel a playlist listing and `yt-dlp`. That the same two
/// words fit both is SPEC §4.1's claim that the kinds differ only in acquisition — and
/// since that claim became a trait, this file no longer knows which one it is driving.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    JsonSchema,
    clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
#[clap(rename_all = "kebab-case")]
pub enum Stage {
    /// Enumerate what the source declares it has.
    Discover,
    /// Fetch what discovery found and the store lacks.
    Collect,
    /// Derive text from collected documents.
    Extract,
    /// Derive text from collected audio. Channel sources only.
    Transcribe,
    /// Chunk derived text into the search index.
    Index,
    /// Turn indexed chunks into vectors.
    Embed,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Self::Discover => "discover",
            Self::Collect => "collect",
            Self::Extract => "extract",
            Self::Transcribe => "transcribe",
            Self::Index => "index",
            Self::Embed => "embed",
        }
    }
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct RunArgs {
    /// Source to run. Repeatable. Omit for every enabled source in the config.
    ///
    /// Naming a source runs it even when its block says `enabled = false` — the flag is
    /// a default, and typing the id is an instruction.
    #[arg(long = "source", value_name = "ID")]
    #[serde(default)]
    pub sources: Vec<String>,

    /// Stage to skip. Repeatable.
    ///
    /// `--skip embed` is the common one: it stops before the hours-long stage, leaving
    /// a corpus that is collected, extracted and keyword-searchable but not yet
    /// semantically searchable. A later run picks the embedding up where this left it.
    #[arg(long, value_name = "STAGE")]
    #[serde(default)]
    pub skip: Vec<Stage>,

    /// Stop collection after this many addresses, per source.
    ///
    /// The way to try a source before committing an hour to it. Deliberately **not**
    /// applied to discovery: a DiscoveryRun is a full snapshot (§4.3), and a truncated
    /// one would look like a source that shrank — corrupting the very signal the
    /// snapshots exist to carry.
    #[arg(long)]
    #[serde(default)]
    pub limit: Option<usize>,

    /// Redo work already done — refetch, re-derive, re-transcribe.
    ///
    /// The path after upgrading an extractor or a model tier. Without it every stage
    /// skips what it has already done, which is the normal and much cheaper case.
    #[arg(long)]
    #[serde(default)]
    pub refresh: bool,

    /// Report the plan and exit. Touches neither the network nor the store.
    #[arg(long)]
    #[serde(default)]
    pub dry_run: bool,

    /// Config file. Defaults to the usual search path.
    #[arg(long, value_name = "FILE")]
    #[serde(default)]
    pub config: Option<String>,
}

/// What became of one stage.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StageStatus {
    Ran,
    /// Not attempted, and why — a skipped stage is not a failed one.
    Skipped {
        reason: String,
    },
    /// Attempted and failed. The run continues; other sources are unaffected.
    Failed {
        error: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct StageRun {
    pub stage: Stage,
    #[serde(flatten)]
    pub status: StageStatus,
    /// One line for a person: `1,847 urls · 12 new`.
    pub summary: String,
    /// The same numbers for a machine, named as the underlying report names them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub figures: BTreeMap<String, u64>,
    /// What this stage newly did. Zero across a whole run means the corpus is current —
    /// the number a scheduled run exists to produce.
    pub new: u64,
    pub elapsed_secs: f64,
}

impl StageRun {
    fn skipped(stage: Stage, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            stage,
            summary: reason.clone(),
            status: StageStatus::Skipped { reason },
            figures: BTreeMap::new(),
            new: 0,
            elapsed_secs: 0.0,
        }
    }

    fn failed(stage: Stage, error: impl std::fmt::Display, elapsed: f64) -> Self {
        let error = error.to_string();
        Self {
            stage,
            summary: crate::render::one_line(&error),
            status: StageStatus::Failed { error },
            figures: BTreeMap::new(),
            new: 0,
            elapsed_secs: elapsed,
        }
    }

    fn ran(
        stage: Stage,
        new: u64,
        summary: impl Into<String>,
        figures: &[(&str, u64)],
        elapsed: f64,
    ) -> Self {
        Self {
            stage,
            status: StageStatus::Ran,
            summary: summary.into(),
            figures: figures.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            new,
            elapsed_secs: elapsed,
        }
    }

    /// Folds a Source's own figures in beside the stage's.
    ///
    /// Kept open-ended because what a Source counts is the Source's business: a crawled
    /// site reports `disallowed`, a channel reports `rejected`, and a third kind will
    /// report something nobody has thought of yet.
    fn with_figures(mut self, extra: BTreeMap<String, u64>) -> Self {
        self.figures.extend(extra);
        self
    }

    pub fn is_failure(&self) -> bool {
        matches!(self.status, StageStatus::Failed { .. })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SourceRun {
    pub source: String,
    pub kind: SourceKind,
    /// The site or channel URL this source was acquired from.
    pub target: String,
    pub stages: Vec<StageRun>,
    pub elapsed_secs: f64,
}

impl SourceRun {
    pub fn failed(&self) -> bool {
        self.stages.iter().any(StageRun::is_failure)
    }

    /// Documents this source newly stored.
    pub fn new_documents(&self) -> u64 {
        self.stages
            .iter()
            .filter(|s| s.stage == Stage::Collect)
            .map(|s| s.new)
            .sum()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RunReport {
    /// The config that was read, or `None` when none was found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    /// Acquisition, one entry per source, in config order.
    pub sources: Vec<SourceRun>,
    /// Derivation, corpus-wide, in pipeline order.
    pub derive: Vec<StageRun>,
    /// Documents newly stored across every source. **Zero means nothing changed** — the
    /// answer a scheduled run is asked for.
    pub new_documents: u64,
    /// Chunks newly embedded.
    pub new_chunks: u64,
    /// Stages that were attempted and failed, anywhere in the run.
    pub failed_stages: u64,
    pub elapsed_secs: f64,
    pub dry_run: bool,
}

impl RunReport {
    /// True when the run did no new work anywhere — the cheap, expected outcome.
    ///
    /// Asked of *every* stage rather than of the two headline counters. A run that
    /// collected nothing but worked through an extraction backlog is not quiet, and
    /// saying so from the collect count alone would report "nothing new" over the top of
    /// five hundred newly derived documents.
    pub fn is_quiet(&self) -> bool {
        self.failed_stages == 0 && self.all_stages().all(|s| s.new == 0)
    }

    /// Documents derived this run, for the headline when nothing was collected.
    fn newly_derived(&self) -> u64 {
        self.derive
            .iter()
            .filter(|s| matches!(s.stage, Stage::Extract | Stage::Transcribe))
            .map(|s| s.new)
            .sum()
    }

    fn all_stages(&self) -> impl Iterator<Item = &StageRun> {
        self.sources
            .iter()
            .flat_map(|s| s.stages.iter())
            .chain(self.derive.iter())
    }
}

/// Collect everything new for every configured source, then index and embed it.
///
/// Reads `centinel.toml`, and for each `[[source]]` runs discover and collect, then runs
/// extract, transcribe, index and embed once across the corpus. Every stage skips work
/// it has already done, so running this on a schedule costs only what actually changed.
#[op(long_running, group = "pipeline")]
pub async fn run(ctx: &Ctx, args: RunArgs, progress: &Progress) -> anyhow::Result<RunReport> {
    let started = Instant::now();

    let (config, config_path) = match &args.config {
        Some(p) => {
            let path = PathBuf::from(p);
            (Config::from_file(&path)?, Some(path))
        }
        None => (Config::load()?, Config::locate()),
    };
    let selected = config.selected(&args.sources)?;

    let mut report = RunReport {
        config: config_path.map(|p| p.display().to_string()),
        sources: Vec::new(),
        derive: Vec::new(),
        new_documents: 0,
        new_chunks: 0,
        failed_stages: 0,
        elapsed_secs: 0.0,
        dry_run: args.dry_run,
    };

    if selected.is_empty() {
        report.elapsed_secs = started.elapsed().as_secs_f64();
        return Ok(report);
    }

    let skip = |stage: Stage| args.skip.contains(&stage);

    // Built before anything runs. A source that cannot be constructed is a config error,
    // and finding it after nineteen crawls is finding it too late — the same argument
    // `Config::validate` makes, applied to the half of the description only the adapter
    // can check. Building once also means a site's per-host pacing survives from
    // discovery into collection instead of restarting between them.
    let mut built: Vec<Box<dyn Source>> = Vec::with_capacity(selected.len());
    for cfg in &selected {
        built.push(sources::from_config(
            cfg,
            &config.defaults,
            &Overrides::default(),
        )?);
    }

    // The aggregate bar's denominator: two acquisition stages per source, plus the
    // corpus-wide tail. Counted before anything runs so the bar never grows a total
    // underneath someone watching it.
    let yields_audio = built.iter().any(|s| s.yields_audio());
    let derive_stages: Vec<Stage> = [
        Stage::Extract,
        Stage::Transcribe,
        Stage::Index,
        Stage::Embed,
    ]
    .into_iter()
    .filter(|s| *s != Stage::Transcribe || yields_audio)
    .collect();
    let total = (built.len() * 2 + derive_stages.len()) as u64;
    let mut done = 0u64;

    // ── acquire, per source ───────────────────────────────────────────────────
    for source in &built {
        let source_started = Instant::now();
        let id = source.id().to_string();

        let mut stages = Vec::new();
        for stage in [Stage::Discover, Stage::Collect] {
            progress.track(
                TOTAL_TRACK,
                format!("{id} · {}", stage.name()),
                done,
                total,
                Unit::Count,
            );
            progress.say(format!("{id} · {}", stage.name()));
            done += 1;

            let outcome = if skip(stage) {
                StageRun::skipped(stage, "--skip")
            } else if args.dry_run {
                StageRun::skipped(stage, "--dry-run")
            } else if stage == Stage::Collect && stages.iter().any(StageRun::is_failure) {
                // Collecting against a discovery that just failed would acquire the
                // *previous* snapshot and report success. Better to say why it stopped.
                StageRun::skipped(stage, "discover failed")
            } else {
                run_acquisition(ctx, source.as_ref(), &args, stage, progress).await
            };
            stages.push(outcome);
        }

        let source_run = SourceRun {
            source: id,
            kind: source.kind(),
            target: source.target().to_string(),
            stages,
            elapsed_secs: source_started.elapsed().as_secs_f64(),
        };
        report.new_documents += source_run.new_documents();
        report.sources.push(source_run);
    }

    // ── derive, once ──────────────────────────────────────────────────────────
    //
    // Scoped to the named sources when the caller named some, and to the whole store
    // otherwise. `embed` has no source axis at all — the vector cache is keyed by chunk
    // hash, which is corpus-wide by construction (SPEC §5.2).
    let scope: Vec<String> = if args.sources.is_empty() {
        Vec::new()
    } else {
        selected.iter().map(|s| s.id.clone()).collect()
    };

    for stage in derive_stages {
        progress.track(TOTAL_TRACK, stage.name(), done, total, Unit::Count);
        progress.say(stage.name());
        done += 1;

        let outcome = if skip(stage) {
            StageRun::skipped(stage, "--skip")
        } else if args.dry_run {
            StageRun::skipped(stage, "--dry-run")
        } else {
            run_derivation(ctx, &config, &args, stage, &scope, progress).await
        };
        if stage == Stage::Embed {
            report.new_chunks += outcome.new;
        }
        report.derive.push(outcome);
    }

    progress.track(TOTAL_TRACK, "done", total, total, Unit::Count);

    report.failed_stages = report.all_stages().filter(|s| s.is_failure()).count() as u64;
    report.elapsed_secs = started.elapsed().as_secs_f64();
    Ok(report)
}

/// Runs one acquisition stage, converting either outcome into a [`StageRun`].
///
/// Errors are captured rather than propagated: one source's WAF block must not cancel the
/// nineteen after it, and a run that collected most of a corpus should say so.
///
/// This used to be a 212-line `match (stage, acquisition)` with four arms, two of them
/// re-discriminating a report variant that could not occur. Both halves of the variation
/// now sit behind [`Source`], so what is left is the shaping — the stage's numbers turned
/// into a line a person reads — which is all this function was ever meant to be.
async fn run_acquisition(
    ctx: &Ctx,
    source: &dyn Source,
    args: &RunArgs,
    stage: Stage,
    progress: &Progress,
) -> StageRun {
    let t0 = Instant::now();
    let secs = || t0.elapsed().as_secs_f64();

    match stage {
        Stage::Discover => {
            // No `limit` here: see `RunArgs::limit`. A truncated snapshot would read as a
            // source that shrank.
            match acquire::discover(&ctx.store, source, &DiscoverOpts::default(), progress).await {
                Ok(r) => StageRun::ran(
                    stage,
                    r.new as u64,
                    format!(
                        "{} {} \u{00b7} {} new",
                        render::count(r.found as u64),
                        noun(r.found as u64, "address", "addresses"),
                        render::count(r.new as u64)
                    ),
                    &[("found", r.found as u64), ("new", r.new as u64)],
                    secs(),
                )
                .with_figures(r.figures),
                Err(e) => StageRun::failed(stage, e, secs()),
            }
        }

        Stage::Collect => {
            let opts = CollectOpts {
                limit: args.limit,
                refresh: args.refresh,
                matches: Vec::new(),
                ..Default::default()
            };
            match acquire::collect(&ctx.store, source, &opts, progress).await {
                Ok(r) => {
                    let mut summary = format!(
                        "{} acquired \u{00b7} {} changed \u{00b7} {}",
                        render::count(r.stored as u64),
                        render::count(r.changed as u64),
                        render::bytes(r.bytes)
                    );
                    // A wall of refusals with nothing stored is indistinguishable from an
                    // empty source in the counters alone.
                    if r.blocked > 0 {
                        summary.push_str(&format!(
                            " \u{00b7} {} blocked",
                            render::count(r.blocked as u64)
                        ));
                    }
                    StageRun::ran(
                        stage,
                        r.stored as u64,
                        summary,
                        &[
                            ("stored", r.stored as u64),
                            ("changed", r.changed as u64),
                            ("already_had", r.already_had as u64),
                            ("failed", r.failed as u64),
                            ("blocked", r.blocked as u64),
                            ("remaining", r.remaining as u64),
                            ("bytes", r.bytes),
                        ],
                        secs(),
                    )
                    .with_figures(
                        r.parts
                            .into_iter()
                            .map(|(k, v)| (format!("part_{k}"), v as u64))
                            .collect(),
                    )
                }
                Err(e) => StageRun::failed(stage, e, secs()),
            }
        }

        // `run` only ever calls this with the two acquisition stages.
        stage => StageRun::skipped(stage, "not an acquisition stage"),
    }
}

/// Runs one corpus-wide derivation stage across `scope` (empty means every source).
async fn run_derivation(
    ctx: &Ctx,
    config: &Config,
    args: &RunArgs,
    stage: Stage,
    scope: &[String],
    progress: &Progress,
) -> StageRun {
    let t0 = Instant::now();

    // One call per named source, or one unscoped call covering the store. Folding the
    // results keeps the report shape identical either way.
    let targets: Vec<Option<String>> = if scope.is_empty() {
        vec![None]
    } else {
        scope.iter().cloned().map(Some).collect()
    };

    match stage {
        Stage::Extract => {
            let mut extracted = 0u64;
            let mut chars = 0u64;
            let mut unextractable = 0u64;
            let mut ocr = 0u64;
            for target in targets {
                match super::extract(
                    ctx,
                    ExtractArgs {
                        source: target,
                        limit: args.limit,
                        refresh: args.refresh,
                        ..Default::default()
                    },
                    progress,
                )
                .await
                {
                    Ok(r) => {
                        extracted += r.extracted as u64;
                        chars += r.chars_of_text as u64;
                        unextractable += r.unextractable as u64;
                        ocr += r.ocr_pages_pending as u64;
                    }
                    Err(e) => return StageRun::failed(stage, e, t0.elapsed().as_secs_f64()),
                }
            }
            StageRun::ran(
                stage,
                extracted,
                format!(
                    "{} {} · {} chars",
                    crate::render::count(extracted),
                    if extracted == 1 {
                        "document"
                    } else {
                        "documents"
                    },
                    crate::render::count(chars)
                ),
                &[
                    ("extracted", extracted),
                    ("chars_of_text", chars),
                    ("unextractable", unextractable),
                    ("ocr_pages_pending", ocr),
                ],
                t0.elapsed().as_secs_f64(),
            )
        }

        Stage::Transcribe => {
            let model = &config.defaults.transcribe_model;
            if let Some(reason) = missing_model(model, crate::models::ModelRole::Transcription) {
                return StageRun::skipped(stage, reason);
            }
            let mut transcribed = 0u64;
            let mut failed = 0u64;
            let mut chars = 0u64;
            for target in targets {
                match super::transcribe(
                    ctx,
                    TranscribeArgs {
                        source: target,
                        model: model.clone(),
                        language: Some(config.defaults.lang.clone()),
                        limit: args.limit,
                        refresh: args.refresh,
                        ..Default::default()
                    },
                    progress,
                )
                .await
                {
                    Ok(r) => {
                        transcribed += r.transcribed as u64;
                        failed += r.failed as u64;
                        chars += r.transcribed_chars as u64;
                    }
                    Err(e) => return StageRun::failed(stage, e, t0.elapsed().as_secs_f64()),
                }
            }
            StageRun::ran(
                stage,
                transcribed,
                format!(
                    "{} {} · {} chars",
                    crate::render::count(transcribed),
                    if transcribed == 1 {
                        "recording"
                    } else {
                        "recordings"
                    },
                    crate::render::count(chars)
                ),
                &[
                    ("transcribed", transcribed),
                    ("failed", failed),
                    ("transcribed_chars", chars),
                ],
                t0.elapsed().as_secs_f64(),
            )
        }

        Stage::Index => {
            let mut documents = 0u64;
            let mut chunks = 0u64;
            let mut deduplicated = 0u64;
            let mut total_chunks = 0u64;
            for target in targets {
                match super::index(
                    ctx,
                    IndexArgs {
                        source: target,
                        // A re-extraction produces a *new* derived blob, so its chunks are
                        // added — and the previous extraction's chunks stay, because
                        // nothing removes them. Search then returns both, which is the
                        // corpus quietly answering twice. `--refresh` clears the scope it
                        // is refreshing.
                        rebuild: args.refresh,
                        ..Default::default()
                    },
                    progress,
                )
                .await
                {
                    Ok(r) => {
                        documents += r.documents_indexed as u64;
                        chunks += r.chunks_written as u64;
                        deduplicated += r.chunks_deduplicated as u64;
                        total_chunks = r.total_chunks as u64;
                    }
                    Err(e) => return StageRun::failed(stage, e, t0.elapsed().as_secs_f64()),
                }
            }
            StageRun::ran(
                stage,
                documents,
                format!(
                    "{} {} · {} chunks",
                    crate::render::count(documents),
                    if documents == 1 {
                        "document"
                    } else {
                        "documents"
                    },
                    crate::render::count(chunks)
                ),
                &[
                    ("documents_indexed", documents),
                    ("chunks_written", chunks),
                    ("chunks_deduplicated", deduplicated),
                    ("total_chunks", total_chunks),
                ],
                t0.elapsed().as_secs_f64(),
            )
        }

        Stage::Embed => {
            let model = &config.defaults.embed_model;
            if let Some(reason) = missing_model(model, crate::models::ModelRole::Embedding) {
                return StageRun::skipped(stage, reason);
            }
            match super::embed(
                ctx,
                EmbedArgs {
                    model: model.clone(),
                    limit: args.limit,
                    ..Default::default()
                },
                progress,
            )
            .await
            {
                Ok(r) => StageRun::ran(
                    stage,
                    r.embedded as u64,
                    format!(
                        "{} {} · {:.1}/sec",
                        crate::render::count(r.embedded as u64),
                        if r.embedded == 1 { "chunk" } else { "chunks" },
                        r.chunks_per_sec
                    ),
                    &[
                        ("embedded", r.embedded as u64),
                        ("already_cached", r.already_cached as u64),
                        ("remaining", r.remaining as u64),
                    ],
                    t0.elapsed().as_secs_f64(),
                ),
                Err(e) => StageRun::failed(stage, e, t0.elapsed().as_secs_f64()),
            }
        }

        // Acquisition stages never reach here.
        stage => StageRun::skipped(stage, "not a derivation stage"),
    }
}

/// Why a model-backed stage cannot run, or `None` when it can.
///
/// Checked before the stage rather than inside it, because the alternative is failing an
/// hour of crawling at the last step over a download that was never started. A missing
/// model is a **skip**, not a failure: what was collected is collected, and the stage
/// resumes on the next run once the weights are there.
fn missing_model(id: &str, role: crate::models::ModelRole) -> Option<String> {
    let dir = match crate::models::models_dir() {
        Ok(dir) => dir,
        Err(e) => return Some(format!("model directory unavailable: {e}")),
    };
    // The error already carries the command that fixes it, in the one spelling there is.
    crate::models::resolve(id, role, None, &dir)
        .err()
        .map(|e| e.to_string())
}

// ── rendering ─────────────────────────────────────────────────────────────────

/// The headline first, then per-source acquisition, then the corpus-wide tail.
///
/// The number a scheduled run is read for is "did anything change", so that is the line
/// beside the title. A quiet run is meant to be one glance and no reading, which is why
/// the stage detail collapses when nothing happened.
impl Render for RunReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        if self.sources.is_empty() {
            p.line(p.paint("No sources configured.", Ink::Dim))?;
            p.blank()?;
            return p.note("centinel source add tampa --site https://www.tampa.gov");
        }

        let headline = if self.dry_run {
            "dry run — nothing was fetched".to_string()
        } else if self.is_quiet() {
            "nothing new".to_string()
        } else {
            let mut parts = Vec::new();
            if self.new_documents > 0 {
                parts.push(format!(
                    "{} new {}",
                    render::count(self.new_documents),
                    noun(self.new_documents, "document", "documents")
                ));
            } else if self.newly_derived() > 0 {
                // Nothing was fetched, but a backlog was worked through — which is what
                // a run after `--skip extract`, or after a tool upgrade, looks like.
                parts.push(format!(
                    "{} {} derived",
                    render::count(self.newly_derived()),
                    noun(self.newly_derived(), "document", "documents")
                ));
            }
            if self.new_chunks > 0 {
                parts.push(format!("{} embedded", render::count(self.new_chunks)));
            }
            if self.failed_stages > 0 {
                parts.push(format!(
                    "{} failed",
                    render::plural(self.failed_stages as usize, "stage", "stages")
                ));
            }
            if parts.is_empty() {
                "nothing new".to_string()
            } else {
                parts.join(" · ")
            }
        };

        // The answer leads and the context trails it. `title` paints the first argument
        // bold and the second dim, and "nothing new" is what the command was asked —
        // the source count and the clock are how long it took to say it.
        let aside = format!(
            "{} · {}",
            render::plural(self.sources.len(), "source", "sources"),
            render::duration(self.elapsed_secs)
        );
        p.title(&headline, &aside)?;

        p.nest(|p| {
            for source in &self.sources {
                source.render(p)?;
            }
            // Always shown when there is one. A derivation stage that skipped itself
            // because its model is missing is the most important line in the report,
            // and collapsing the block to keep a quiet run short would hide exactly it.
            if !self.derive.is_empty() {
                p.blank()?;
                for stage in &self.derive {
                    render_stage(p, stage, true)?;
                }
            }
            Ok(())
        })
    }
}

impl Render for SourceRun {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let mark = Mark::from_ok(!self.failed());
        let head = format!(
            "{}  {}  {}",
            p.paint(&format!("{:<20}", self.source), mark.ink()),
            p.paint(&format!("{:<8}", self.kind), Ink::Dim),
            p.paint(&render::duration(self.elapsed_secs), Ink::Dim),
        );
        p.marked(mark, head)?;

        p.nest(|p| {
            for stage in &self.stages {
                render_stage(p, stage, false)?;
            }
            Ok(())
        })
    }
}

/// Width of the stage-name column. Fits `transcribe` plus a gutter.
const STAGE_COL: usize = 11;

/// The noun alone. [`render::plural`] returns the count with it, which reads wrong when
/// the count has to lead — "12 documents new" rather than "12 new documents".
fn noun(n: u64, singular: &'static str, plural: &'static str) -> &'static str {
    if n == 1 { singular } else { plural }
}

/// One stage line. `standalone` marks the corpus-wide tail, which has no source above it
/// and so carries its own glyph.
fn render_stage(p: &mut Painter<'_>, stage: &StageRun, standalone: bool) -> std::io::Result<()> {
    let name = format!("{:<STAGE_COL$}", stage.stage.name());
    let (ink, text) = match &stage.status {
        StageStatus::Ran => (Ink::Plain, stage.summary.clone()),
        StageStatus::Skipped { reason } => (Ink::Dim, format!("skipped — {reason}")),
        StageStatus::Failed { error } => (Ink::Red, render::one_line(error)),
    };

    // A stage that did nothing is dimmed whole, so a quiet run reads as one grey block
    // and anything that moved stands out of it without being hunted for.
    let quiet = matches!(stage.status, StageStatus::Ran) && stage.new == 0;
    let name_ink = if quiet { Ink::Dim } else { Ink::Label };
    let body_ink = if quiet { Ink::Dim } else { ink };

    // `p.width()` is already net of the current indent, so this only has to account for
    // the name column and, when standalone, the `✓ ` the mark adds.
    let width = p
        .width()
        .saturating_sub(STAGE_COL + if standalone { 2 } else { 0 });
    let line = format!(
        "{}{}",
        p.paint(&name, name_ink),
        p.paint(&render::truncate(&text, width), body_ink),
    );

    if standalone {
        let mark = match stage.status {
            StageStatus::Failed { .. } => Mark::Bad,
            StageStatus::Skipped { .. } => Mark::None,
            StageStatus::Ran => Mark::Ok,
        };
        p.marked(mark, line)
    } else {
        p.line(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage_ran(stage: Stage, new: u64) -> StageRun {
        StageRun::ran(stage, new, "summary", &[("x", new)], 1.0)
    }

    fn source_run(id: &str, stages: Vec<StageRun>) -> SourceRun {
        SourceRun {
            source: id.into(),
            kind: SourceKind::Site,
            target: "https://x.gov".into(),
            stages,
            elapsed_secs: 1.0,
        }
    }

    fn report(sources: Vec<SourceRun>, derive: Vec<StageRun>) -> RunReport {
        let mut r = RunReport {
            config: Some("centinel.toml".into()),
            new_documents: sources.iter().map(SourceRun::new_documents).sum(),
            new_chunks: derive
                .iter()
                .filter(|s| s.stage == Stage::Embed)
                .map(|s| s.new)
                .sum(),
            sources,
            derive,
            failed_stages: 0,
            elapsed_secs: 12.0,
            dry_run: false,
        };
        r.failed_stages = r.all_stages().filter(|s| s.is_failure()).count() as u64;
        r
    }

    fn render_to_string(report: &RunReport) -> String {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    /// The whole point of a scheduled run: a second one says so in one word.
    #[test]
    fn a_run_that_found_nothing_is_quiet() {
        let r = report(
            vec![source_run(
                "tampa",
                vec![stage_ran(Stage::Discover, 0), stage_ran(Stage::Collect, 0)],
            )],
            vec![stage_ran(Stage::Extract, 0), stage_ran(Stage::Embed, 0)],
        );
        assert!(r.is_quiet());
        assert!(render_to_string(&r).contains("nothing new"));
    }

    #[test]
    fn new_documents_counts_collection_not_every_stage() {
        // 12 collected becoming 15 extracted and 400 embedded is one number, not three.
        let r = report(
            vec![source_run(
                "tampa",
                vec![
                    stage_ran(Stage::Discover, 40),
                    stage_ran(Stage::Collect, 12),
                ],
            )],
            vec![stage_ran(Stage::Extract, 15), stage_ran(Stage::Embed, 400)],
        );
        assert_eq!(r.new_documents, 12);
        assert_eq!(r.new_chunks, 400);
        assert!(!r.is_quiet());

        let out = render_to_string(&r);
        assert!(out.contains("12 new documents"), "{out}");
        assert!(out.contains("400 embedded"), "{out}");
    }

    #[test]
    fn a_failed_stage_is_counted_and_shown() {
        let r = report(
            vec![source_run(
                "pinellas",
                vec![
                    StageRun::failed(Stage::Discover, "HTTP 403 from robots.txt", 2.0),
                    StageRun::skipped(Stage::Collect, "discover failed"),
                ],
            )],
            vec![],
        );
        assert_eq!(r.failed_stages, 1);
        assert!(!r.is_quiet());

        let out = render_to_string(&r);
        assert!(out.contains("403"), "{out}");
        assert!(out.contains("discover failed"), "{out}");
        assert!(out.contains("1 stage failed"), "{out}");
    }

    /// A run that fetched nothing but cleared an extraction backlog is not quiet, and
    /// the headline has to say what it did rather than fall back to "nothing new".
    #[test]
    fn work_done_without_collection_still_reads_as_work() {
        let r = report(
            vec![source_run("tampa", vec![stage_ran(Stage::Collect, 0)])],
            vec![stage_ran(Stage::Extract, 500)],
        );
        assert_eq!(r.new_documents, 0);
        assert!(!r.is_quiet());

        let out = render_to_string(&r);
        assert!(out.contains("500 documents derived"), "{out}");
        assert!(!out.contains("nothing new"), "{out}");
    }

    /// A skipped stage must not read as a broken one — SPEC's `Blocked` is not `Gone`,
    /// and the same distinction applies to work nobody asked for.
    #[test]
    fn skipping_is_not_failing() {
        let skipped = StageRun::skipped(Stage::Embed, "--skip");
        assert!(!skipped.is_failure());

        let r = report(
            vec![source_run("tampa", vec![stage_ran(Stage::Collect, 0)])],
            vec![skipped],
        );
        assert_eq!(r.failed_stages, 0);
        assert!(render_to_string(&r).contains("skipped"));
    }

    #[test]
    fn an_empty_config_points_at_the_command_that_fixes_it() {
        let r = report(vec![], vec![]);
        let out = render_to_string(&r);
        assert!(out.contains("No sources configured"), "{out}");
        assert!(out.contains("centinel source add"), "{out}");
    }

    #[test]
    fn a_dry_run_says_it_fetched_nothing() {
        let mut r = report(
            vec![source_run(
                "tampa",
                vec![
                    StageRun::skipped(Stage::Discover, "--dry-run"),
                    StageRun::skipped(Stage::Collect, "--dry-run"),
                ],
            )],
            vec![],
        );
        r.dry_run = true;
        assert!(render_to_string(&r).contains("dry run"));
    }

    /// The report is re-deserialized from its own JSON on the render path, so a field
    /// that cannot round-trip breaks the CLI after the work is done.
    #[test]
    fn the_report_round_trips_through_json() {
        let r = report(
            vec![source_run(
                "tampa",
                vec![stage_ran(Stage::Discover, 3), stage_ran(Stage::Collect, 1)],
            )],
            vec![StageRun::skipped(Stage::Embed, "--skip")],
        );
        let json = serde_json::to_value(&r).unwrap();
        let back: RunReport = serde_json::from_value(json).unwrap();
        assert_eq!(back.sources.len(), 1);
        assert_eq!(back.new_documents, 1);
        assert!(matches!(back.derive[0].status, StageStatus::Skipped { .. }));
    }

    #[test]
    fn stage_names_match_the_ops_they_call() {
        assert_eq!(Stage::Discover.name(), "discover");
        assert_eq!(Stage::Index.name(), "index");
        for stage in [
            Stage::Discover,
            Stage::Collect,
            Stage::Extract,
            Stage::Transcribe,
            Stage::Index,
            Stage::Embed,
        ] {
            // Every stage but `transcribe` is a registered op of the same name; the
            // transcribe op is registered too, so all six must resolve.
            assert!(
                crate::op::find(stage.name()).is_some(),
                "stage `{}` names no op",
                stage.name()
            );
        }
    }
}
