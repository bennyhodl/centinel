//! `transcribe` — turn stored audio into timestamped text.
//!
//! The local-inference twin of [`crate::ops::embed`], and it inherits that op's shape:
//! read the pool, never the network; write a `Blob → Blob` derivation; make resumability
//! a consequence of the log rather than a checkpoint file.
//!
//! ## Resumability, again for free
//!
//! A 3-hour meeting is tens of minutes of inference, so interruption is the normal case.
//! There is no progress file, because the log already answers the question:
//!
//! ```text
//! audio blobs observed  −  blobs that already have a whisper derivation
//! ```
//!
//! Kill it after four meetings and re-run; it starts at the fifth.
//!
//! ## Why the tier is on every derivation
//!
//! SPEC §4.6: recording `tool`, `version` **and** `model_tier` is what makes *"the source
//! changed"* mechanically distinguishable from *"this ran on a weaker machine with a
//! smaller whisper tier"*. Transcription is the case that argument was written for — the
//! same audio through `whisper-tiny` and `whisper-large-v3-turbo` yields materially
//! different text, and without the tier a re-run would look like the recording changed.

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fetch::{SNIFF_BYTES, content_kind};
use crate::prelude::*;
use crate::store::LogRecord;
use crate::transcribe::{Transcriber, WORKER};

/// The tool name recorded on every derivation this op writes.
const TOOL: &str = "whisper.cpp";

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct TranscribeArgs {
    /// Source to transcribe. Omit for every source in the store.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,

    /// Whisper model id.
    #[arg(long, default_value = "whisper-large-v3-turbo")]
    #[serde(default = "default_model")]
    pub model: String,

    /// Quantization. Defaults to whichever is installed.
    #[arg(long)]
    #[serde(default)]
    pub variant: Option<String>,

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

    /// Transcribe without VAD.
    ///
    /// Off by default and deliberately awkward to reach: VAD is the documented mitigation
    /// for Whisper hallucinating over the dead air a council recording is full of.
    #[arg(long)]
    #[serde(default)]
    pub allow_no_vad: bool,

    /// Report the work without loading a model or running inference.
    #[arg(long)]
    #[serde(default)]
    pub dry_run: bool,
}

fn default_model() -> String {
    "whisper-large-v3-turbo".to_string()
}

/// So [`crate::ops::run`] inherits the CLI's defaults — including `allow_no_vad: false`,
/// which is the documented mitigation for Whisper hallucinating over dead air and must
/// not become opt-out by way of a second default.
impl Default for TranscribeArgs {
    fn default() -> Self {
        Self {
            source: None,
            model: default_model(),
            variant: None,
            language: None,
            limit: None,
            refresh: false,
            allow_no_vad: false,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    /// False when no VAD was installed and `--allow-no-vad` permitted the run anyway.
    pub vad: bool,
    /// Audio blobs found in the pool.
    pub audio_found: usize,
    /// Skipped because a transcript already existed.
    pub already_transcribed: usize,
    pub attempted: usize,
    pub transcribed: usize,
    pub failed: usize,
    pub audio_ms: u64,
    pub transcribed_chars: usize,
    pub items: Vec<TranscribedItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<TranscribeFailure>,
}

/// Transcribe collected audio with a local Whisper model.
#[op(long_running, group = "stage")]
pub async fn transcribe(
    ctx: &Ctx,
    args: TranscribeArgs,
    progress: &Progress,
) -> anyhow::Result<TranscribeReport> {
    let sources = match &args.source {
        Some(s) => vec![SourceId::new(s.clone())?],
        None => ctx.store.sources().await?,
    };

    let mut report = TranscribeReport {
        sources: sources.iter().map(|s| s.to_string()).collect(),
        model: args.model.clone(),
        variant: args.variant.clone(),
        vad: false,
        audio_found: 0,
        already_transcribed: 0,
        attempted: 0,
        transcribed: 0,
        failed: 0,
        audio_ms: 0,
        transcribed_chars: 0,
        items: Vec::new(),
        failures: Vec::new(),
    };

    // ---- work list -----------------------------------------------------------------
    // Built before any model is loaded, so `--dry-run` costs nothing and a run with no
    // work never pays the multi-second weight load.
    let mut todo: Vec<(SourceId, Resource, Observation)> = Vec::new();

    for source in &sources {
        let replay = ctx.store.replay(source).await?;
        // Keyed by tool: a text derivation of a video's *metadata* must not be mistaken
        // for a transcript of its audio.
        let transcribed = replay.derived_by(TOOL);

        for (resource, obs) in replay.latest_observations() {
            // The head, and only the head. This asks "is this audio?" of every blob in
            // the corpus, and `get_blob` answers it by reading the whole file and hashing
            // it — so building a work list over a store of PDFs used to read and hash
            // every PDF in it.
            let head = ctx.store.blob_head(&obs.blob_sha, SNIFF_BYTES).await?;
            if content_kind(&obs.meta, &head) != "audio" {
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

    if let Some(limit) = args.limit {
        todo.truncate(limit);
    }

    if args.dry_run {
        progress.say(format!("{} recordings would be transcribed", todo.len()));
        report.attempted = todo.len();
        return Ok(report);
    }

    if todo.is_empty() {
        progress.say("nothing to transcribe");
        return Ok(report);
    }

    // ---- resolve the model ---------------------------------------------------------
    let root = crate::models::models_dir()?;
    let transcriber = Transcriber::resolve(
        &root,
        &args.model,
        args.variant.as_deref(),
        args.language.clone(),
    )?;

    // A missing VAD is refused rather than silently downgraded. Whisper fabricates
    // sentences in proportion to non-vocal duration, and a council recording is mostly
    // non-vocal — a transcript produced without VAD is a different artifact, not a
    // slightly worse one.
    if !transcriber.has_vad() && !args.allow_no_vad {
        // The command comes from `models::resolve`, so it names the VAD the registry
        // actually pins rather than the one that was current when this was written.
        let fix = crate::models::REGISTRY
            .iter()
            .find(|s| s.role == crate::models::ModelRole::VoiceActivity)
            .and_then(|s| {
                crate::models::resolve(s.id, crate::models::ModelRole::VoiceActivity, None, &root)
                    .err()
                    .and_then(|e| e.fix().map(str::to_string))
            })
            .unwrap_or_else(|| "centinel models pull".to_string());

        anyhow::bail!(
            "no VAD weights installed. Whisper hallucinates over the dead air a council \
             recording is full of, so transcription without VAD is refused by default.\n  \
             fix:      {fix}\n  \
             override: --allow-no-vad (recorded on every derivation)"
        );
    }
    report.vad = transcriber.has_vad();
    report.variant = Some(transcriber.variant.clone());

    // ---- transcribe ----------------------------------------------------------------
    let tier = transcriber.tier();
    let total = todo.len() as u64;

    for (i, (source, resource, obs)) in todo.into_iter().enumerate() {
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
                    version: transcript.whisper_version.clone(),
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

/// Present so a reader grepping for the worker binary finds it named here too.
#[allow(dead_code)]
const _WORKER: &str = WORKER;

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// The counters, the realtime factor, and whether VAD was actually in the loop.
///
/// `vad: false` is rendered as a warning rather than a field. A run that skipped voice
/// activity detection under `--allow-no-vad` produces transcripts that are real but worse
/// — Whisper hallucinates into silence — and six months later the only record of *why* a
/// transcript is poor is this flag. It should be impossible to miss on the run that made it.
impl Render for TranscribeReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let model = match &self.variant {
            Some(v) => format!("{} · {v}", self.model),
            None => self.model.clone(),
        };
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

            if !self.vad {
                p.marked(
                    Mark::Warn,
                    p.paint("no VAD — Whisper saw the silence too", Ink::Dim),
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
            source: None,
            model: default_model(),
            variant: None,
            language: None,
            limit: None,
            refresh: false,
            allow_no_vad: false,
            dry_run: true,
        }
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

    /// The whole point of `--dry-run`: plan the work without a multi-gigabyte load. It
    /// must therefore not touch the model registry at all.
    #[tokio::test]
    async fn a_dry_run_finds_the_audio_without_loading_a_model() {
        let (_d, ctx) = ctx().await;
        observe(&ctx, "tampa", "https://youtube.com/watch?v=a", &m4a()).await;
        observe(
            &ctx,
            "tampa",
            "https://tampa.gov/x.pdf",
            b"%PDF-1.7 not audio",
        )
        .await;

        let report = transcribe(&ctx, args(), &Progress::none()).await.unwrap();
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

        let report = transcribe(&ctx, args(), &Progress::none()).await.unwrap();
        assert_eq!(report.already_transcribed, 1);
        assert_eq!(report.attempted, 0);

        // `--refresh` is the path after a tier change.
        let report = transcribe(
            &ctx,
            TranscribeArgs {
                refresh: true,
                ..args()
            },
            &Progress::none(),
        )
        .await
        .unwrap();
        assert_eq!(report.attempted, 1);
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

        let report = transcribe(&ctx, args(), &Progress::none()).await.unwrap();
        assert_eq!(report.already_transcribed, 0);
        assert_eq!(report.attempted, 1);
    }

    /// Refusing beats silently producing a hallucination-prone transcript. The check must
    /// happen after the work list, so an empty store does not demand weights it will not
    /// use — but before inference, so nothing is written under the wrong assumption.
    #[tokio::test]
    async fn transcription_without_vad_is_refused_by_default() {
        let (_d, ctx) = ctx().await;
        observe(&ctx, "tampa", "https://youtube.com/watch?v=a", &m4a()).await;

        let err = transcribe(
            &ctx,
            TranscribeArgs {
                dry_run: false,
                ..args()
            },
            &Progress::none(),
        )
        .await
        .unwrap_err()
        .to_string();

        // On a machine with no weights at all the model resolves first; either way the
        // message has to name a `models pull` that fixes it.
        assert!(err.contains("models pull"), "unhelpful refusal: {err}");
    }

    #[tokio::test]
    async fn an_empty_store_needs_no_weights_at_all() {
        let (_d, ctx) = ctx().await;
        let report = transcribe(
            &ctx,
            TranscribeArgs {
                dry_run: false,
                ..args()
            },
            &Progress::none(),
        )
        .await
        .unwrap();
        assert_eq!(report.attempted, 0);
    }
}
