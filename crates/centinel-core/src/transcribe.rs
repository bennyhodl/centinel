//! Audio to transcript, across a process boundary.
//!
//! ## Why a subprocess, when `whisper-rs` is an FFI binding
//!
//! Because the alternative is silently wrong output. whisper.cpp and llama.cpp each
//! vendor `ggml` and export the same ~534 `ggml_*` symbols; linked into one binary the
//! linker keeps one copy, and the two versions differ. Measured on identical audio and
//! weights:
//!
//! | binary | result |
//! |---|---|
//! | `whisper-rs` alone | 2 segments, correct text |
//! | `whisper-rs` + `llama-cpp-2` | **0 segments**, every logit `p=0.000` |
//!
//! No crash, no link error, no warning. Since `centinel` must link llama.cpp for search,
//! whisper.cpp lives in [`centinel-whisper`], a sibling binary from the same workspace,
//! and the two meet over a pipe. SPEC §2.3 permits exactly this — a one-shot subprocess,
//! not a long-lived second-language service.
//!
//! ## The pipeline
//!
//! ```text
//! blob (m4a/webm/wav)  --ffmpeg-->  f32le 16kHz mono  --pipe-->  centinel-whisper  --> JSON
//! ```
//!
//! Both hops stream. A 3-hour meeting is ~691 MB of `f32` PCM, and materialising that as
//! a temp file would be the largest write in the whole pipeline for no benefit.
//!
//! [`centinel-whisper`]: https://github.com/bennyhodl/centinel

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::models::{self, ModelSpec};
use crate::op::Progress;
use crate::tool::{Pipes, Tool};

/// The worker binary's name, and the `$CENTINEL_WHISPER_BIN` override.
pub const WORKER: &str = "centinel-whisper";
pub const ENV_WORKER: &str = "CENTINEL_WHISPER_BIN";

/// The program that decodes audio into the PCM the worker reads.
pub const DECODER: &str = "ffmpeg";

/// Whisper's fixed input rate. Resampling happens once, in ffmpeg.
pub const SAMPLE_RATE: u32 = 16_000;

/// How long the worker may say nothing before it is treated as wedged.
///
/// The guard is **inactivity**, not total time. A three-hour meeting on a laptop is hours
/// of legitimate work, so a wall-clock deadline would either cut off real transcriptions
/// or be so large it guarded nothing. The worker runs with `--progress` and reports on
/// stderr, so silence is the signal — and this is sized to sit above a cold load of a
/// multi-gigabyte model, which is the longest quiet stretch a healthy run has.
pub const STALL_TIMEOUT: Duration = Duration::from_secs(600);

/// One span of speech.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    /// The model's own estimate that this span is silence. Kept, not applied — a reader
    /// auditing a suspicious passage should see what the model thought, and a threshold
    /// applied here would vanish from the record.
    #[serde(default)]
    pub no_speech_prob: f32,
}

/// What the worker returned.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transcript {
    pub whisper_version: String,
    pub language: String,
    /// Whether VAD ran. Provenance, because a transcript produced without it carries a
    /// materially different hallucination risk (see [`crate::models`]'s `silero-vad`).
    pub vad: bool,
    pub sample_count: usize,
    pub duration_ms: u64,
    pub segments: Vec<Segment>,
}

impl Transcript {
    /// Renders the transcript as timestamped markdown.
    ///
    /// SPEC §6.4: *"Store timestamps per chunk unconditionally. They are the transcript's
    /// page numbers — they turn a hit into a `watch?v=X&t=4271s` citation, which is the
    /// entire value proposition."*
    ///
    /// Putting the timestamp **in the text** rather than only in a sidecar is what makes
    /// that survive: the existing chunker splits derived markdown without knowing this
    /// document is a transcript, so any chunk it produces still opens with the timestamp
    /// of the speech it contains. No chunking special case, no anchor table to join.
    pub fn to_markdown(&self, title: Option<&str>) -> String {
        render_markdown(
            self.segments
                .iter()
                .map(|s| (s.start_ms, s.end_ms, s.text.as_str())),
            title,
        )
    }

    /// Time ranges for [`crate::domain::Derivation::anchors`] — the structured form of
    /// what [`Self::to_markdown`] writes into the prose.
    pub fn anchors(&self) -> Vec<crate::domain::Anchor> {
        self.segments
            .iter()
            .map(|s| crate::domain::Anchor::TimeRange {
                start_ms: s.start_ms.max(0) as u64,
                end_ms: s.end_ms.max(0) as u64,
            })
            .collect()
    }

    /// Spoken words, with the timestamps stripped.
    pub fn plain_text(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Characters to accumulate before a paragraph may end.
///
/// Cue-per-line is unreadable and wasteful at caption granularity: a 3-hour meeting is
/// ~8,500 cues of about four seconds each, so one line apiece would spend more of the
/// document on timestamps than on speech. Grouping to a paragraph puts a citable offset
/// roughly every minute, which is still finer than the ~1,200-character chunks §6.5 asks
/// for — so every chunk keeps a timestamp without the document drowning in them.
const PARAGRAPH_CHARS: usize = 600;

/// Silence that forces a paragraph break regardless of length.
///
/// **The correctness rule, not a formatting preference.** A paragraph carries one
/// timestamp, so everything inside it is cited at that offset. Two short passages an hour
/// apart — ordinary once VAD has removed the silence between them — would otherwise share
/// a paragraph, and the second half would be cited 71 minutes from where it was said.
/// That is precisely the claim §6.4 exists to make trustworthy.
const GAP_BREAK_MS: i64 = 10_000;

/// Renders timestamped cues as markdown paragraphs, each opening with its offset.
///
/// Shared by Whisper output and by YouTube captions **on purpose**: a passage retrieved
/// from a machine transcript and one retrieved from a caption track should be the same
/// shape downstream, so chunking, embedding and citation need no idea which produced it.
/// The provenance difference lives on the `Derivation`, where it belongs.
pub fn render_markdown<'a>(
    cues: impl IntoIterator<Item = (i64, i64, &'a str)>,
    title: Option<&str>,
) -> String {
    let mut out = String::new();
    if let Some(title) = title {
        out.push_str("# ");
        out.push_str(title);
        out.push_str("\n\n");
    }

    let mut para = String::new();
    let mut para_start: Option<i64> = None;
    let mut previous_end: Option<i64> = None;

    let flush = |out: &mut String, para: &mut String, start: &mut Option<i64>| {
        if !para.is_empty() {
            out.push_str(&format!(
                "[{}] {}\n\n",
                hms(start.take().unwrap_or(0)),
                para
            ));
            para.clear();
        }
    };

    for (start_ms, end_ms, text) in cues {
        let text = text.trim();
        if text.is_empty() {
            continue;
        }

        // A long silence ends the paragraph before this cue joins it.
        if previous_end.is_some_and(|prev| start_ms - prev > GAP_BREAK_MS) {
            flush(&mut out, &mut para, &mut para_start);
        }

        if para_start.is_none() {
            para_start = Some(start_ms);
        } else {
            para.push(' ');
        }
        para.push_str(text);
        previous_end = Some(end_ms);

        // Break on a sentence end once long enough, or hard-cap so an ASR track with no
        // punctuation at all — which is the norm for auto-captions — still breaks.
        let long_enough = para.len() >= PARAGRAPH_CHARS;
        let ends_sentence = para.ends_with('.') || para.ends_with('?') || para.ends_with('!');
        if (long_enough && ends_sentence) || para.len() >= PARAGRAPH_CHARS * 2 {
            flush(&mut out, &mut para, &mut para_start);
        }
    }

    flush(&mut out, &mut para, &mut para_start);
    out
}

/// `4271000` → `01:11:11`. The `&t=` form a citation needs.
fn hms(ms: i64) -> String {
    let total = (ms.max(0) / 1000) as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

/// Locates the worker binary.
///
/// Beside the running executable first, because that is where `cargo build` and every
/// packaging of this workspace put it. `$CENTINEL_WHISPER_BIN` wins for the odd layout,
/// and `PATH` is the last resort.
pub fn worker_path() -> anyhow::Result<PathBuf> {
    if let Some(explicit) = std::env::var_os(ENV_WORKER).filter(|v| !v.is_empty()) {
        let p = PathBuf::from(explicit);
        anyhow::ensure!(
            p.is_file(),
            "{ENV_WORKER} points at {}, which is not a file",
            p.display()
        );
        return Ok(p);
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(sibling) = exe.parent().map(|d| d.join(WORKER))
        && sibling.is_file()
    {
        return Ok(sibling);
    }

    which(WORKER).ok_or_else(|| {
        anyhow::anyhow!(
            "cannot find `{WORKER}`. It ships with centinel — build it with \
             `cargo build --release -p centinel-whisper`, or set {ENV_WORKER}"
        )
    })
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// Resolved weights plus the two programs that will use them.
#[derive(Debug)]
pub struct Transcriber {
    worker: PathBuf,
    /// The decoder, so a test can put something predictable in ffmpeg's place.
    decoder: PathBuf,
    /// How long the worker may say nothing. A field rather than a constant so the guard
    /// itself can be tested without a ten-minute test.
    stall_timeout: Duration,
    model: PathBuf,
    vad_model: Option<PathBuf>,
    pub spec: &'static ModelSpec,
    pub variant: String,
    pub language: Option<String>,
}

impl Transcriber {
    /// Resolves the worker, the Whisper weights and the VAD weights.
    ///
    /// The VAD is optional here and *not* optional in practice: [`Self::transcribe`]
    /// records `vad: false` on the transcript so a run without it stays visible in the
    /// record rather than being indistinguishable after the fact.
    pub fn resolve(
        root: &Path,
        model_id: &str,
        variant: Option<&str>,
        language: Option<String>,
    ) -> anyhow::Result<Self> {
        let spec = models::require(model_id)?;
        anyhow::ensure!(
            spec.role == models::ModelRole::Transcription,
            "`{model_id}` is a {} model, not a transcriber",
            spec.role
        );

        let (model, variant) = installed_file(spec, variant, root)?;

        // Any installed VAD, since the registry pins one version at a time.
        let vad_model = models::REGISTRY
            .iter()
            .filter(|s| s.role == models::ModelRole::VoiceActivity)
            .find_map(|s| installed_file(s, None, root).ok().map(|(p, _)| p));

        Ok(Self {
            worker: worker_path()?,
            decoder: PathBuf::from(DECODER),
            stall_timeout: STALL_TIMEOUT,
            model,
            vad_model,
            spec,
            variant,
            language,
        })
    }

    /// Builds a transcriber from explicit program paths, with no model registry lookup.
    ///
    /// The seam that makes [`Self::transcribe`] reachable from a test. That function is a
    /// hundred lines of process choreography — two children, a pipe between them, a
    /// progress stream and five separate failure paths — and none of it could be
    /// exercised, because the only way to build a `Transcriber` demanded several
    /// gigabytes of installed weights and a compiled worker binary.
    ///
    /// Also the honest answer for an unusual install, which is why `worker_path` already
    /// reads `$CENTINEL_WHISPER_BIN` for the same reason.
    pub fn with_binaries(
        worker: impl Into<PathBuf>,
        decoder: impl Into<PathBuf>,
        model: impl Into<PathBuf>,
        spec: &'static ModelSpec,
    ) -> Self {
        Self {
            worker: worker.into(),
            decoder: decoder.into(),
            stall_timeout: STALL_TIMEOUT,
            model: model.into(),
            vad_model: None,
            spec,
            variant: "test".to_string(),
            language: None,
        }
    }

    /// How long the worker may say nothing before it is treated as wedged.
    pub fn with_stall_timeout(mut self, stall_timeout: Duration) -> Self {
        self.stall_timeout = stall_timeout;
        self
    }

    /// True when a VAD was found. A caller should refuse or warn — see the module docs.
    pub fn has_vad(&self) -> bool {
        self.vad_model.is_some()
    }

    /// The tier that produced a transcript, for [`crate::domain::Derivation`].
    pub fn tier(&self) -> crate::domain::ModelTier {
        crate::domain::ModelTier {
            model_id: self.spec.id.to_string(),
            variant: Some(self.variant.clone()),
        }
    }

    /// Decodes `audio` and transcribes it.
    ///
    /// `audio` is a path into the blob pool, so it has no extension — ffmpeg sniffs the
    /// container, which it does reliably for the m4a and webm YouTube serves.
    pub async fn transcribe(
        &self,
        audio: &Path,
        progress: &Progress,
    ) -> anyhow::Result<Transcript> {
        // `-nostdin` is belt and braces: `Tool` denies stdin to every child it starts,
        // and ffmpeg that reads an inherited terminal swallows keystrokes.
        let mut ffmpeg = Tool::new(&self.decoder)
            .args(["-nostdin", "-loglevel", "error", "-i"])
            .arg(audio)
            .args([
                "-f",
                "f32le",
                "-acodec",
                "pcm_f32le",
                "-ar",
                &SAMPLE_RATE.to_string(),
                "-ac",
                "1",
                "-",
            ])
            .spawn(Pipes::read())
            .map_err(|e| anyhow::anyhow!("{e} — it decodes audio for transcription"))?;

        let mut worker = Tool::new(&self.worker)
            .arg("--model")
            .arg(&self.model)
            .arg("--progress");
        if let Some(vad) = &self.vad_model {
            worker = worker.arg("--vad-model").arg(vad);
        }
        if let Some(lang) = &self.language {
            worker = worker.arg("--language").arg(lang);
        }
        let mut worker = worker.spawn(Pipes::duplex())?;

        // Pump ffmpeg's PCM into the worker. Both ends are children of this process, so
        // nothing buffers the full ~691 MB of a 3-hour meeting.
        let mut pcm_out = ffmpeg.stdout.take().expect("ffmpeg stdout was piped");
        let mut pcm_in = worker.stdin.take().expect("worker stdin was piped");
        let pump = tokio::spawn(async move {
            let copied = tokio::io::copy(&mut pcm_out, &mut pcm_in).await;
            // Dropping the handle closes the pipe, which is what tells the worker the
            // audio is complete. Without it the worker waits on stdin forever.
            drop(pcm_in);
            copied
        });

        // Progress arrives on the worker's stderr as `progress <percent>`.
        let worker_err = worker.stderr.take().expect("worker stderr was piped");
        let label = format!(
            "transcribing with {} {}",
            self.spec.id,
            if self.vad_model.is_some() {
                "+vad"
            } else {
                "(no vad)"
            }
        );
        // The same stream is the diagnostics *and* the heartbeat. Any line at all means
        // the worker is alive; silence past `STALL_TIMEOUT` means it is not, and `stall`
        // is the only thing this task sends — a normal end of stderr never fires it.
        let progress = progress.clone();
        let stall_timeout = self.stall_timeout;
        let (stall, stalled) = tokio::sync::oneshot::channel();
        let diagnostics = tokio::spawn(async move {
            let mut lines = BufReader::new(worker_err).lines();
            let mut kept = Vec::new();
            let mut stall = Some(stall);
            loop {
                let line = match tokio::time::timeout(stall_timeout, lines.next_line()).await {
                    Ok(Ok(Some(line))) => line,
                    // Stderr closed, or the read failed. Either way the worker is done
                    // talking, and how it exited is the exit status's business.
                    Ok(_) => break,
                    Err(_) => {
                        if let Some(stall) = stall.take() {
                            let _ = stall.send(());
                        }
                        break;
                    }
                };
                if let Some(pct) = line.strip_prefix("progress ")
                    && let Ok(pct) = pct.trim().parse::<u64>()
                {
                    progress.step(label.clone(), pct, 100);
                    continue;
                }
                // Keep the rest for the error message if the worker fails.
                if kept.len() < 40 {
                    kept.push(line);
                }
            }
            kept
        });

        // Kept as a `Child` rather than consumed by `wait_with_output`, so there is still
        // something to kill if it wedges.
        let mut worker_out = worker.stdout.take().expect("worker stdout was piped");
        let collected = tokio::spawn(async move {
            let mut buf = Vec::new();
            tokio::io::copy(&mut worker_out, &mut buf)
                .await
                .map(|_| buf)
        });

        let status = tokio::select! {
            status = worker.wait() => status?,
            // `Ok(())` and not `_`: the diagnostics task drops this sender when stderr
            // closes normally, which resolves the receiver as `Err`. Matching that too
            // would call every successful transcription a hang.
            Ok(()) = stalled => {
                // Dropping `worker` would kill it anyway — `Tool` sets `kill_on_drop` —
                // but an explicit kill means the process is gone before this returns
                // rather than whenever the runtime gets round to the drop.
                let _ = worker.kill().await;
                anyhow::bail!(
                    "{WORKER} produced no output for {:?} and was stopped. It was \
                     transcribing {}. A wedged worker usually means a corrupt model file \
                     — `centinel models verify` checks them.",
                    self.stall_timeout,
                    audio.display(),
                );
            }
        };
        let stdout = collected.await??;
        let stderr_lines = diagnostics.await.unwrap_or_default();
        let copied = pump.await?;

        // ffmpeg's own failure is the more useful message when both ends fail: a worker
        // starved of audio only ever reports "no audio on stdin".
        let ff = ffmpeg.wait_with_output().await?;
        if !ff.status.success() {
            anyhow::bail!(
                "{} could not decode {}: {}",
                self.decoder.display(),
                audio.display(),
                String::from_utf8_lossy(&ff.stderr).trim()
            );
        }
        copied.map_err(|e| anyhow::anyhow!("piping audio to {WORKER}: {e}"))?;

        anyhow::ensure!(
            status.success(),
            "{WORKER} failed ({status}): {}",
            stderr_lines.join("; ")
        );

        serde_json::from_slice(&stdout).map_err(|e| {
            anyhow::anyhow!(
                "{WORKER} returned output that is not a transcript ({e}): {}",
                String::from_utf8_lossy(&stdout)
                    .chars()
                    .take(200)
                    .collect::<String>()
            )
        })
    }
}

/// The on-disk path of an installed variant, preferring the default.
fn installed_file(
    spec: &'static ModelSpec,
    variant: Option<&str>,
    root: &Path,
) -> anyhow::Result<(PathBuf, String)> {
    let status = models::status(spec, root);
    let chosen = match variant {
        Some(name) => status
            .variants
            .iter()
            .find(|v| v.variant == name)
            .filter(|v| v.installed)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "`{}` variant `{name}` is not installed — `centinel models pull {} --variant {name}`",
                    spec.id,
                    spec.id
                )
            })?,
        None => status.active().ok_or_else(|| {
            anyhow::anyhow!(
                "`{}` is not installed — `centinel models pull {}`",
                spec.id,
                spec.id
            )
        })?,
    };

    let file = spec
        .variant(Some(&chosen.variant))?
        .files
        .first()
        .expect("every variant has a file");
    Ok((spec.dir(root).join(file.path), chosen.variant.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_render_as_a_citable_offset() {
        assert_eq!(hms(0), "00:00:00");
        assert_eq!(hms(4_271_000), "01:11:11");
        // A negative timestamp is whisper reporting nonsense; clamp rather than panic.
        assert_eq!(hms(-5), "00:00:00");
    }

    fn transcript() -> Transcript {
        Transcript {
            whisper_version: "1.8.3".into(),
            language: "en".into(),
            vad: true,
            sample_count: 16_000,
            duration_ms: 1_000,
            segments: vec![
                Segment {
                    start_ms: 0,
                    end_ms: 1_820,
                    text: "The council meeting will come to order.".into(),
                    no_speech_prob: 0.01,
                },
                Segment {
                    start_ms: 4_271_000,
                    end_ms: 4_275_000,
                    text: "The first item is the drinking water sampling report.".into(),
                    no_speech_prob: 0.02,
                },
            ],
        }
    }

    /// §6.4's whole value proposition: a hit must become `watch?v=X&t=4271s`. That only
    /// survives chunking if the timestamp is *in the text*, because the chunker does not
    /// know this document is a transcript.
    #[test]
    fn every_line_carries_the_timestamp_that_makes_it_citable() {
        let md = transcript().to_markdown(Some("Council Meeting"));

        assert!(md.starts_with("# Council Meeting\n\n"));

        // Chunked anywhere, a chunk still opens with a timestamp.
        for line in md.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            assert!(line.starts_with('['), "untimestamped line: {line}");
        }
    }

    /// A paragraph carries **one** timestamp, so everything in it is cited at that
    /// offset. These two passages are short enough to group by length but sit 71 minutes
    /// apart — grouping them would cite the second an hour from where it was said, which
    /// is the exact claim §6.4 exists to make trustworthy.
    #[test]
    fn a_silence_splits_a_paragraph_even_when_it_is_short() {
        let md = transcript().to_markdown(None);

        assert!(
            md.contains("[00:00:00] The council meeting will come to order."),
            "{md}"
        );
        assert!(
            md.contains("[01:11:11] The first item is the drinking water sampling report."),
            "the second passage must keep its own offset, not inherit 00:00:00:\n{md}"
        );
    }

    /// The other half of the rule: contiguous speech must *not* be split, or a 3-hour
    /// meeting becomes thousands of one-line paragraphs and the timestamps outweigh the
    /// words.
    #[test]
    fn contiguous_speech_groups_into_one_paragraph() {
        let contiguous = Transcript {
            segments: (0..6)
                .map(|i| Segment {
                    start_ms: i * 4_000,
                    end_ms: (i + 1) * 4_000,
                    text: "the commission discussed the matter".into(),
                    no_speech_prob: 0.0,
                })
                .collect(),
            ..transcript()
        };

        let paragraphs: Vec<_> = contiguous
            .to_markdown(None)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string)
            .collect();
        assert_eq!(paragraphs.len(), 1, "{paragraphs:#?}");
        assert!(paragraphs[0].starts_with("[00:00:00] "));
    }

    #[test]
    fn anchors_mirror_the_segments() {
        let t = transcript();
        let anchors = t.anchors();
        assert_eq!(anchors.len(), t.segments.len());
        assert_eq!(
            anchors[1],
            crate::domain::Anchor::TimeRange {
                start_ms: 4_271_000,
                end_ms: 4_275_000
            }
        );
    }

    // ── the process pipeline ───────────────────────────────────────────────────
    //
    // Both ends are stand-in scripts. What is under test is the choreography — two
    // children, a pipe between them, a progress stream, a stall guard and five failure
    // paths — not whisper.cpp, which has its own tests and its own repository.
    //
    // None of this was reachable before: the only way to build a `Transcriber` demanded
    // several gigabytes of installed weights and a compiled worker binary, so a hundred
    // lines of the riskiest code in the crate had no test at all.

    #[cfg(unix)]
    mod pipeline {
        use super::*;
        use std::path::Path;

        /// A transcript the worker script can print, and this test can recognise.
        const TRANSCRIPT: &str = r#"{"whisper_version":"test","language":"en","vad":false,
            "sample_count":16000,"duration_ms":1000,
            "segments":[{"start_ms":0,"end_ms":1000,
            "text":"the council meeting will come to order","no_speech_prob":0.0}]}"#;

        fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        /// A decoder that ignores ffmpeg's arguments and emits some bytes.
        fn decoder(dir: &Path, body: &str) -> PathBuf {
            script(dir, "decoder", body)
        }

        fn transcriber(worker: PathBuf, decoder: PathBuf) -> Transcriber {
            Transcriber::with_binaries(
                worker,
                decoder,
                "/dev/null",
                models::require("whisper-large-v3-turbo").unwrap(),
            )
        }

        #[tokio::test]
        async fn audio_reaches_the_worker_and_a_transcript_comes_back() {
            let dir = tempfile::tempdir().unwrap();
            let dec = decoder(dir.path(), "printf 'pretend-pcm-bytes'");
            // Proves the pipe carried the decoder's bytes: the worker refuses if the
            // audio it was fed is not what the decoder produced.
            let worker = script(
                dir.path(),
                "worker",
                &format!(
                    "audio=$(cat)\n\
                     [ \"$audio\" = 'pretend-pcm-bytes' ] || {{ echo \"got: $audio\" >&2; exit 9; }}\n\
                     echo 'progress 50' >&2\n\
                     echo 'progress 100' >&2\n\
                     cat <<'EOF'\n{TRANSCRIPT}\nEOF"
                ),
            );

            let got = transcriber(worker, dec)
                .transcribe(Path::new("/dev/null"), &Progress::none())
                .await
                .unwrap();

            assert_eq!(got.segments.len(), 1);
            assert_eq!(
                got.segments[0].text,
                "the council meeting will come to order"
            );
            assert!(!got.vad);
        }

        /// A worker that fails must surface what it said, not a generic exit code.
        #[tokio::test]
        async fn a_failing_worker_reports_its_own_diagnostics() {
            let dir = tempfile::tempdir().unwrap();
            let dec = decoder(dir.path(), "printf 'x'");
            let worker = script(
                dir.path(),
                "worker",
                "cat > /dev/null\necho 'failed to load model: bad magic' >&2\nexit 4",
            );

            let err = transcriber(worker, dec)
                .transcribe(Path::new("/dev/null"), &Progress::none())
                .await
                .unwrap_err()
                .to_string();
            assert!(err.contains("bad magic"), "{err}");
            assert!(err.contains(WORKER), "{err}");
        }

        /// A worker starved of audio only ever reports "no audio on stdin", so the
        /// decoder's own failure is the more useful message when both ends fail.
        #[tokio::test]
        async fn a_failing_decoder_beats_the_worker_it_starved() {
            let dir = tempfile::tempdir().unwrap();
            let dec = decoder(
                dir.path(),
                "echo 'Invalid data found when processing input' >&2\nexit 1",
            );
            let worker = script(
                dir.path(),
                "worker",
                "cat > /dev/null\necho 'no audio on stdin' >&2\nexit 1",
            );

            let err = transcriber(worker, dec)
                .transcribe(Path::new("/tmp/not-audio"), &Progress::none())
                .await
                .unwrap_err()
                .to_string();
            assert!(err.contains("could not decode"), "{err}");
            assert!(err.contains("Invalid data"), "{err}");
        }

        /// The hazard this whole change is about: a wedged worker used to block the
        /// caller for as long as the machine stayed up.
        #[tokio::test]
        async fn a_worker_that_goes_quiet_is_stopped_and_says_why() {
            let dir = tempfile::tempdir().unwrap();
            let dec = decoder(dir.path(), "printf 'x'");
            let worker = script(dir.path(), "worker", "cat > /dev/null\nsleep 60");

            let started = std::time::Instant::now();
            let err = transcriber(worker, dec)
                .with_stall_timeout(Duration::from_millis(400))
                .transcribe(Path::new("/dev/null"), &Progress::none())
                .await
                .unwrap_err()
                .to_string();

            assert!(err.contains("produced no output"), "{err}");
            assert!(err.contains("models verify"), "the fix is named: {err}");
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the stall guard never fired: {:?}",
                started.elapsed()
            );
        }

        /// Progress is life. A worker that is slow but talking must not be killed.
        #[tokio::test]
        async fn a_slow_worker_that_keeps_talking_is_left_alone() {
            let dir = tempfile::tempdir().unwrap();
            let dec = decoder(dir.path(), "printf 'x'");
            // The first line comes before `cat`, so the guard is measuring gaps between
            // progress reports rather than how long this machine takes to start a shell.
            let worker = script(
                dir.path(),
                "worker",
                &format!(
                    "echo 'loading model' >&2\n\
                     cat > /dev/null\n\
                     i=0\n\
                     while [ $i -lt 20 ]; do echo \"progress $i\" >&2; sleep 0.15; i=$((i+1)); done\n\
                     cat <<'EOF'\n{TRANSCRIPT}\nEOF"
                ),
            );

            // Three seconds of work under a two-second guard. That is only survivable
            // because every line resets the timer — which is the claim. The guard is well
            // above this machine's worst measured process-start latency under a parallel
            // test run, so what fails here is the reset, not the scheduler.
            let started = std::time::Instant::now();
            let got = transcriber(worker, dec)
                .with_stall_timeout(Duration::from_secs(2))
                .transcribe(Path::new("/dev/null"), &Progress::none())
                .await
                .unwrap();

            assert_eq!(got.segments.len(), 1);
            assert!(
                started.elapsed() > Duration::from_millis(2_500),
                "the worker finished too fast to have out-lived the guard: {:?}",
                started.elapsed()
            );
        }

        #[tokio::test]
        async fn output_that_is_not_a_transcript_is_quoted_back() {
            let dir = tempfile::tempdir().unwrap();
            let dec = decoder(dir.path(), "printf 'x'");
            let worker = script(
                dir.path(),
                "worker",
                "cat > /dev/null\necho 'Segmentation fault'",
            );

            let err = transcriber(worker, dec)
                .transcribe(Path::new("/dev/null"), &Progress::none())
                .await
                .unwrap_err()
                .to_string();
            assert!(err.contains("not a transcript"), "{err}");
            assert!(err.contains("Segmentation fault"), "{err}");
        }

        #[tokio::test]
        async fn a_missing_decoder_names_what_it_was_for() {
            let dir = tempfile::tempdir().unwrap();
            let worker = script(dir.path(), "worker", "cat > /dev/null");

            let err = transcriber(worker, PathBuf::from("centinel-no-such-decoder"))
                .transcribe(Path::new("/dev/null"), &Progress::none())
                .await
                .unwrap_err()
                .to_string();
            assert!(err.contains("not installed"), "{err}");
            assert!(err.contains("decodes audio"), "{err}");
        }
    }

    #[test]
    fn a_transcriber_refuses_a_model_that_does_not_transcribe() {
        let dir = tempfile::tempdir().unwrap();
        let err = Transcriber::resolve(dir.path(), "qwen3-embedding-4b", None, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a transcriber"), "{err}");
    }

    #[test]
    fn an_uninstalled_model_names_the_command_that_fixes_it() {
        let dir = tempfile::tempdir().unwrap();
        let err = Transcriber::resolve(dir.path(), "whisper-large-v3-turbo", None, None)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("models pull") || err.contains(WORKER),
            "unhelpful error: {err}"
        );
    }
}
