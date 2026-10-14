//! `transcribe` — turn stored audio into timestamped text.
//!
//! The twin of [`crate::ops::embed`], and it inherits that op's shape: read the pool, send
//! the work to the server `[inference.transcribe]` names, write a `Blob → Blob`
//! derivation, and make resumability a consequence of the log rather than a checkpoint
//! file.
//!
//! ## Resumability, again for free
//!
//! A 3-hour meeting is tens of minutes of inference, so interruption is the normal case.
//! There is no progress file, because the log already answers the question:
//!
//! ```text
//! audio blobs observed  −  blobs that already have a transcript derivation
//! ```
//!
//! Kill it after four meetings and re-run; it starts at the fifth. A transcript written by
//! the in-process `whisper.cpp` worker this replaced counts as one: the audio has not
//! changed, and re-transcribing a backlog to change who did it is hours for nothing.
//!
//! ## Why the tier is on every derivation
//!
//! SPEC §4.6: recording `tool`, `version` **and** `model_tier` is what makes *"the source
//! changed"* mechanically distinguishable from *"this ran on a weaker model"*.
//! Transcription is the case that argument was written for — the same audio through
//! `whisper-tiny` and `whisper-large-v3-turbo` yields materially different text, and
//! without the tier a re-run would look like the recording changed. The tier is the model
//! as the server was asked for it, with the server's `host:port` where a quantization used
//! to be.

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::InferenceConfig;
use crate::content::{ContentKind, SNIFF_BYTES};
use crate::inference::Capability;
use crate::prelude::*;
use crate::store::LogRecord;
use crate::transcribe::Transcriber;

/// The tool name recorded on every derivation this op writes.
const TOOL: &str = "transcription";

/// The tool name the in-process worker recorded. Its transcripts are transcripts.
const LEGACY_TOOL: &str = "whisper.cpp";

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct TranscribeArgs {
    /// Source to transcribe. Omit for every source in the store.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,

    /// The model to ask the transcription server for.
    ///
    /// Unset means "nobody said here", which falls through to `[inference.transcribe]
    /// model` in the config file. A clap default here would make a typed model
    /// indistinguishable from an absent one, and the file's standing preference would be
    /// read and never used.
    #[arg(long)]
    #[serde(default)]
    pub model: Option<String>,

    /// Spoken language, e.g. `en`. Omit to let Whisper detect it per recording.
    #[arg(long)]
    #[serde(default)]
    pub language: Option<String>,

    /// Stop after this many recordings.
    #[arg(long)]
    #[serde(default)]
    pub limit: Option<usize>,

    /// Re-transcribe audio that already has a derivation — the path after a tier change.
    #[arg(long)]
    #[serde(default)]
    pub refresh: bool,

    /// Report the work without sending anything to the server.
    #[arg(long)]
    #[serde(default)]
    pub dry_run: bool,
}

/// So [`crate::ops::run`] inherits the CLI's defaults. `model: None` is "nobody said",
/// which resolves through the config file to `[inference.transcribe] model`.
impl Default for TranscribeArgs {
    fn default() -> Self {
        Self {
            source: None,
            model: None,
            language: None,
            limit: None,
            refresh: false,
            dry_run: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TranscribedItem {
    pub url: String,
    pub blob_sha: String,
    pub transcript_sha: String,
    pub duration_ms: u64,
    pub segments: usize,
    pub chars: usize,
    pub language: String,
    /// Seconds of audio per second of wall clock. The number that decides whether a
    /// backlog is an afternoon or a fortnight.
    pub realtime_factor: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TranscribeFailure {
    pub url: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TranscribeReport {
    pub sources: Vec<String>,
    pub model: String,
    /// The server's `base_url`.
    pub server: String,
    /// Audio blobs found in the pool.
    pub audio_found: usize,
    /// Skipped because a transcript already existed.
    pub already_transcribed: usize,
    pub attempted: usize,
    pub transcribed: usize,
    pub failed: usize,
    /// Still outstanding after this run, from `--limit`. The one figure here that is an
    /// instruction rather than a result: it says run it again.
    #[serde(default)]
    pub remaining: usize,
    pub audio_ms: u64,
    pub transcribed_chars: usize,
    pub items: Vec<TranscribedItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<TranscribeFailure>,
}

/// Transcribe collected audio on the configured transcription server.
#[op(long_running, reach = "operator", group = "stage")]
pub async fn transcribe(
    ctx: &Ctx,
    args: TranscribeArgs,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<TranscribeReport> {
    let inference = crate::config::Config::load()?.inference;
    transcribe_with(ctx, args, &inference, progress, cancel).await
}

/// [`transcribe`], against the servers `inference` names — `run` passes the config it has
/// open rather than have the file read twice.
pub(crate) async fn transcribe_with(
    ctx: &Ctx,
    args: TranscribeArgs,
    inference: &InferenceConfig,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<TranscribeReport> {
    // The flag, else the config file's standing preference.
    let mut endpoint = inference.endpoint(Capability::Transcribe);
    if let Some(model) = &args.model {
        endpoint = endpoint.with_model(model);
    }

    let sources = match &args.source {
        Some(s) => vec![SourceId::new(s.clone())?],
        None => ctx.store.sources().await?,
    };

    let mut report = TranscribeReport {
        sources: sources.iter().map(|s| s.to_string()).collect(),
        model: endpoint.model.clone(),
        server: endpoint.base_url.clone(),
        audio_found: 0,
        already_transcribed: 0,
        attempted: 0,
        transcribed: 0,
        failed: 0,
        remaining: 0,
        audio_ms: 0,
        transcribed_chars: 0,
        items: Vec::new(),
        failures: Vec::new(),
    };

    // ---- work list -----------------------------------------------------------------
    // Built before anything is sent, so `--dry-run` costs nothing.
    let mut todo: Vec<(SourceId, Resource, Observation)> = Vec::new();

    for source in &sources {
        let replay = ctx.store.replay(source).await?;
        // Keyed by tool: a text derivation of a video's *metadata* must not be mistaken
        // for a transcript of its audio.
        let mut transcribed = replay.derived_by(TOOL);
        transcribed.extend(replay.derived_by(LEGACY_TOOL));

        for (resource, obs) in replay.latest_observations() {
            // The head, and only the head. This asks "is this audio?" of every blob in
            // the corpus, and `get_blob` answers it by reading the whole file and hashing
            // it — so building a work list over a store of PDFs used to read and hash
            // every PDF in it.
            let head = ctx.store.blob_head(&obs.blob_sha, SNIFF_BYTES).await?;
            if ContentKind::classify(&obs.meta, &head) != ContentKind::Audio {
                continue;
            }
            report.audio_found += 1;

            if !args.refresh && transcribed.contains(&obs.blob_sha) {
                report.already_transcribed += 1;
                continue;
            }
            todo.push((source.clone(), resource, obs));
        }
    }

    // Counted before the cut, exactly as `embed` and `extract` do. A `--limit` that
    // truncates in silence is the defect "a cap that is not reported is data loss" was
    // written against, and this was the one stage it missed: a run said `12 transcribed`
    // and never said 300 recordings were still waiting.
    let outstanding = todo.len();
    if let Some(limit) = args.limit {
        todo.truncate(limit);
    }
    report.remaining = outstanding - todo.len();

    if args.dry_run {
        progress.say(format!("{} recordings would be transcribed", todo.len()));
        report.attempted = todo.len();
        return Ok(report);
    }

    if todo.is_empty() {
        progress.say("nothing to transcribe");
        return Ok(report);
    }

    let transcriber = Transcriber::new(endpoint, args.language.clone())?;

    // ---- transcribe ----------------------------------------------------------------
    let tier = transcriber.tier();
    let total = todo.len() as u64;

    for (i, (source, resource, obs)) in todo.into_iter().enumerate() {
        // The coarsest boundary in the pipeline — one recording can be four hours on the
        // server. Dropping the request is what shutdown has to reach.
        cancel.check()?;
        progress.step(
            format!("{}/{} {}", i + 1, total, short(&resource.natural_key)),
            i as u64,
            total,
        );
        report.attempted += 1;

        let audio = ctx.store.blob_path_of(&obs.blob_sha);
        let started = std::time::Instant::now();

        let transcript = match transcriber.transcribe(&audio, progress).await {
            Ok(t) => t,
            // A server that is not there, refuses the key or answers without timestamps
            // will do the same for every recording after this one.
            Err(e) if crate::inference::is_fatal(&e) => return Err(e),
            Err(e) => {
                report.failed += 1;
                report.failures.push(TranscribeFailure {
                    url: resource.natural_key.clone(),
                    reason: format!("{e:#}"),
                });
                continue;
            }
        };
        let elapsed = started.elapsed().as_secs_f64();

        let title = obs.meta.get("title").map(String::as_str);
        let markdown = transcript.to_markdown(title);
        let to_sha = ctx.store.put_blob(markdown.as_bytes()).await?;

        ctx.store
            .append(
                &source,
                &LogRecord::Derivation(Derivation {
                    from_sha: obs.blob_sha.clone(),
                    to_sha: to_sha.clone(),
                    tool: TOOL.to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    model_tier: Some(tier.clone()),
                    at: Timestamp::now(),
                    // The structured twin of the timestamps in the markdown. §4.3 puts
                    // anchors on the derivation precisely so audio and PDFs can share
                    // one re-derivation path while anchoring differently.
                    anchors: transcript.anchors(),
                }),
            )
            .await?;

        report.transcribed += 1;
        report.audio_ms += transcript.duration_ms;
        report.transcribed_chars += markdown.chars().count();
        report.items.push(TranscribedItem {
            url: resource.natural_key.clone(),
            blob_sha: obs.blob_sha.to_string(),
            transcript_sha: to_sha.to_string(),
            duration_ms: transcript.duration_ms,
            segments: transcript.segments.len(),
            chars: markdown.chars().count(),
            language: transcript.language.clone(),
            realtime_factor: if elapsed > 0.0 {
                (transcript.duration_ms as f64 / 1000.0) / elapsed
            } else {
                0.0
            },
        });
    }

    progress.step(
        format!(
            "{} transcribed, {} failed",
            report.transcribed, report.failed
        ),
        total,
        total,
    );
    Ok(report)
}

/// Trims a URL to something that fits a progress line.
fn short(url: &str) -> String {
    url.rsplit('/')
        .next()
        .unwrap_or(url)
        .chars()
        .take(48)
        .collect()
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// The counters and the realtime factor, under the model and the server that ran it.
impl Render for TranscribeReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let model = format!("{} · {}", self.model, self.server);
        p.title(&self.sources.join(", "), &model)?;
        p.nest(|p| {
            p.figures(&[
                (self.audio_found as u64, "audio blobs found"),
                (self.already_transcribed as u64, "already transcribed"),
                (self.attempted as u64, "attempted"),
                (self.transcribed as u64, "transcribed"),
                (self.failed as u64, "failed"),
            ])?;

            p.blank()?;
            let totals = format!(
                "{} of audio · {} of text",
                render::duration(self.audio_ms as f64 / 1000.0),
                render::count(self.transcribed_chars as u64),
            );
            p.line(p.paint(&totals, Ink::Dim))?;

            // The one figure here that is an instruction. Said on the line a person
            // reads, because a `--limit` nobody is told about reads as a finished corpus.
            if self.remaining > 0 {
                p.marked(
                    Mark::Warn,
                    p.paint(
                        &format!(
                            "{} recording(s) left by --limit; run it again to continue",
                            render::count(self.remaining as u64)
                        ),
                        Ink::Dim,
                    ),
                )?;
            }

            if !self.items.is_empty() {
                p.section("transcribed")?;
                for item in &self.items {
                    item.render(p)?;
                }
            }

            if !self.failures.is_empty() {
                p.section("failures")?;
                for failure in &self.failures {
                    failure.render(p)?;
                }
            }
            Ok(())
        })
    }
}

impl Render for TranscribedItem {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let head = render::truncate(&self.url, p.width().saturating_sub(4));
        p.marked(Mark::Ok, head)?;
        let note = format!(
            "{} · {} · {} segments · {:.1}× realtime",
            render::duration(self.duration_ms as f64 / 1000.0),
            self.language,
            render::count(self.segments as u64),
            self.realtime_factor,
        );
        p.nest(|p| p.line(p.paint(&note, Ink::Dim)))
    }
}

impl Render for TranscribeFailure {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.marked(
            Mark::Bad,
            render::truncate(&self.url, p.width().saturating_sub(4)),
        )?;
        p.nest(|p| p.wrapped(&render::one_line(&self.reason), Ink::Dim))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::store::Store;

    async fn ctx() -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        (dir, Ctx::new(store))
    }

    fn args() -> TranscribeArgs {
        TranscribeArgs {
            dry_run: true,
            ..TranscribeArgs::default()
        }
    }

    /// The op against the default `[inference]`, never the config file of whatever
    /// machine runs the tests. Nothing here reaches the server: the runs are dry or have
    /// nothing to send.
    async fn run(ctx: &Ctx, args: TranscribeArgs) -> TranscribeReport {
        transcribe_with(
            ctx,
            args,
            &InferenceConfig::default(),
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap()
    }

    /// A 12-byte ISO-BMFF header is enough for [`content_kind`] to call it audio, which
    /// is what the work list keys on.
    fn m4a() -> Vec<u8> {
        let mut b = vec![0, 0, 0, 0x20];
        b.extend_from_slice(b"ftypM4A ");
        b.extend_from_slice(&[0u8; 32]);
        b
    }

    async fn observe(ctx: &Ctx, source: &str, key: &str, bytes: &[u8]) -> Observation {
        let src = SourceId::new(source).unwrap();
        let resource = Resource::new(src, key);
        ctx.store
            .record_observation(&resource, bytes, Timestamp::now(), BTreeMap::new())
            .await
            .unwrap()
    }

    /// The whole point of `--dry-run`: plan the work without sending any of it. The
    /// default server is not running in a test, so reaching for it would fail here.
    #[tokio::test]
    async fn a_dry_run_finds_the_audio_without_reaching_the_server() {
        let (_d, ctx) = ctx().await;
        observe(&ctx, "tampa", "https://youtube.com/watch?v=a", &m4a()).await;
        observe(
            &ctx,
            "tampa",
            "https://tampa.gov/x.pdf",
            b"%PDF-1.7 not audio",
        )
        .await;

        let report = run(&ctx, args()).await;
        assert_eq!(report.audio_found, 1, "the PDF must not be queued");
        assert_eq!(report.attempted, 1);
        assert_eq!(report.transcribed, 0, "a dry run transcribes nothing");
    }

    /// Resumability is a consequence of the log, not a checkpoint file (see module docs).
    #[tokio::test]
    async fn an_existing_transcript_drops_out_of_the_work_list() {
        let (_d, ctx) = ctx().await;
        let obs = observe(&ctx, "tampa", "https://youtube.com/watch?v=a", &m4a()).await;

        let text = ctx.store.put_blob(b"[00:00:00] hello").await.unwrap();
        ctx.store
            .append(
                &SourceId::new("tampa").unwrap(),
                &LogRecord::Derivation(Derivation {
                    from_sha: obs.blob_sha.clone(),
                    to_sha: text,
                    tool: TOOL.to_string(),
                    version: "1.8.3".into(),
                    model_tier: None,
                    at: Timestamp::now(),
                    anchors: vec![],
                }),
            )
            .await
            .unwrap();

        let report = run(&ctx, args()).await;
        assert_eq!(report.already_transcribed, 1);
        assert_eq!(report.attempted, 0);

        // `--refresh` is the path after a tier change.
        let report = run(
            &ctx,
            TranscribeArgs {
                refresh: true,
                ..args()
            },
        )
        .await;
        assert_eq!(report.attempted, 1);
    }

    /// A transcript the in-process worker wrote is a transcript. Moving to a server must
    /// not re-transcribe a backlog that is already searchable.
    #[tokio::test]
    async fn a_transcript_from_the_old_worker_still_counts() {
        let (_d, ctx) = ctx().await;
        let obs = observe(&ctx, "tampa", "https://youtube.com/watch?v=a", &m4a()).await;
        let text = ctx.store.put_blob(b"[00:00:00] hello").await.unwrap();
        ctx.store
            .append(
                &SourceId::new("tampa").unwrap(),
                &LogRecord::Derivation(Derivation {
                    from_sha: obs.blob_sha.clone(),
                    to_sha: text,
                    tool: LEGACY_TOOL.to_string(),
                    version: "1.8.3".into(),
                    model_tier: None,
                    at: Timestamp::now(),
                    anchors: vec![],
                }),
            )
            .await
            .unwrap();

        let report = run(&ctx, args()).await;
        assert_eq!(report.already_transcribed, 1);
        assert_eq!(report.attempted, 0);
    }

    /// A derivation of the *metadata* for the same video must not be read as a transcript
    /// of its audio — which is why the skip key includes the tool.
    #[tokio::test]
    async fn a_derivation_by_another_tool_does_not_count_as_a_transcript() {
        let (_d, ctx) = ctx().await;
        let obs = observe(&ctx, "tampa", "https://youtube.com/watch?v=a", &m4a()).await;

        let text = ctx
            .store
            .put_blob(b"extracted by something else")
            .await
            .unwrap();
        ctx.store
            .append(
                &SourceId::new("tampa").unwrap(),
                &LogRecord::Derivation(Derivation {
                    from_sha: obs.blob_sha.clone(),
                    to_sha: text,
                    tool: "htmd".into(),
                    version: "0.5.5".into(),
                    model_tier: None,
                    at: Timestamp::now(),
                    anchors: vec![],
                }),
            )
            .await
            .unwrap();

        let report = run(&ctx, args()).await;
        assert_eq!(report.already_transcribed, 0);
        assert_eq!(report.attempted, 1);
    }

    #[tokio::test]
    async fn an_empty_store_needs_no_server_at_all() {
        let (_d, ctx) = ctx().await;
        let report = run(
            &ctx,
            TranscribeArgs {
                dry_run: false,
                ..args()
            },
        )
        .await;
        assert_eq!(report.attempted, 0);
    }
}
