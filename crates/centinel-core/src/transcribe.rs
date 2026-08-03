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
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::models::{self, ModelSpec};
use crate::op::Progress;

/// The worker binary's name, and the `$CENTINEL_WHISPER_BIN` override.
pub const WORKER: &str = "centinel-whisper";
pub const ENV_WORKER: &str = "CENTINEL_WHISPER_BIN";

/// Whisper's fixed input rate. Resampling happens once, in ffmpeg.
pub const SAMPLE_RATE: u32 = 16_000;

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
        let mut out = String::new();
        if let Some(title) = title {
            out.push_str("# ");
            out.push_str(title);
            out.push_str("\n\n");
        }
        for seg in &self.segments {
            out.push_str(&format!("[{}] {}\n", hms(seg.start_ms), seg.text));
        }
        out
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

/// Resolved weights plus the worker that will use them.
#[derive(Debug)]
pub struct Transcriber {
    worker: PathBuf,
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
            model,
            vad_model,
            spec,
            variant,
            language,
        })
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
        let mut ffmpeg = Command::new("ffmpeg")
            .args([
                // Never let ffmpeg consume our stdin; it inherits a terminal otherwise
                // and can swallow keystrokes or block.
                "-nostdin",
                "-loglevel",
                "error",
                "-i",
            ])
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
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!("cannot run ffmpeg ({e}) — it decodes audio for transcription")
            })?;

        let mut worker = Command::new(&self.worker);
        worker
            .arg("--model")
            .arg(&self.model)
            .arg("--progress")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(vad) = &self.vad_model {
            worker.arg("--vad-model").arg(vad);
        }
        if let Some(lang) = &self.language {
            worker.arg("--language").arg(lang);
        }
        let mut worker = worker
            .spawn()
            .map_err(|e| anyhow::anyhow!("cannot run {}: {e}", self.worker.display()))?;

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
        let progress = progress.clone();
        let diagnostics = tokio::spawn(async move {
            let mut lines = BufReader::new(worker_err).lines();
            let mut kept = Vec::new();
            while let Ok(Some(line)) = lines.next_line().await {
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

        let output = worker.wait_with_output().await?;
        let stderr_lines = diagnostics.await.unwrap_or_default();
        let copied = pump.await?;

        // ffmpeg's own failure is the more useful message when both ends fail: a worker
        // starved of audio only ever reports "no audio on stdin".
        let ff = ffmpeg.wait_with_output().await?;
        if !ff.status.success() {
            anyhow::bail!(
                "ffmpeg could not decode {}: {}",
                audio.display(),
                String::from_utf8_lossy(&ff.stderr).trim()
            );
        }
        copied.map_err(|e| anyhow::anyhow!("piping audio to {WORKER}: {e}"))?;

        anyhow::ensure!(
            output.status.success(),
            "{WORKER} failed ({}): {}",
            output.status,
            stderr_lines.join("; ")
        );

        serde_json::from_slice(&output.stdout).map_err(|e| {
            anyhow::anyhow!(
                "{WORKER} returned output that is not a transcript ({e}): {}",
                String::from_utf8_lossy(&output.stdout)
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
        assert!(md.contains("[00:00:00] The council meeting will come to order."));
        assert!(md.contains("[01:11:11] The first item is the drinking water sampling report."));

        // Chunked anywhere, a chunk still opens with a timestamp.
        for line in md.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            assert!(line.starts_with('['), "untimestamped line: {line}");
        }
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
