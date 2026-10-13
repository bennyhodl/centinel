//! Audio to transcript, through a transcription server.
//!
//! ## The pipeline
//!
//! ```text
//! blob (m4a/webm/wav)  --ffmpeg-->  16 kHz mono WAV  --POST /audio/transcriptions-->  segments
//! ```
//!
//! The request is OpenAI's, asked for `verbose_json` so the answer carries timed segments:
//! whisper.cpp's server (started with `--inference-path /v1/audio/transcriptions`),
//! faster-whisper's, LocalAI and OpenAI's own all speak it. Whisper's own tuning — VAD,
//! beam size, threads, GPU — is the server's, set once for its machine. **Run it with
//! VAD.** Whisper fabricates sentences in proportion to the silence it is fed, and a
//! council recording is mostly silence; a server without VAD produces a different artifact,
//! not a slightly worse one.
//!
//! ## Why ffmpeg is still here
//!
//! A server decodes what it was built to decode — whisper.cpp's reads WAV unless built
//! with ffmpeg — and YouTube serves m4a and webm. Decoding here to the one format every
//! server reads, at the 16 kHz mono Whisper resamples to anyway, is also what keeps the
//! upload small: an hour of it is 115 MB, against twice that at the source's rate. The WAV
//! is written to a scratch file rather than streamed, because a request that fails is sent
//! again, and a pipe can be read once.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::inference::{Client, Endpoint, Fatal};
use crate::op::Progress;
use crate::tool::Tool;

/// The program that decodes audio into the WAV the server is sent.
pub const DECODER: &str = "ffmpeg";

/// Whisper's fixed input rate. Resampling happens once, in ffmpeg.
pub const SAMPLE_RATE: u32 = 16_000;

/// Bytes of 16-bit mono WAV per second of audio, for sizing a request's deadline.
const BYTES_PER_SECOND: u64 = SAMPLE_RATE as u64 * 2;

/// How long ffmpeg may take to decode one recording. A three-hour meeting decodes in
/// well under a minute; this is a hang, not a slow machine.
const DECODE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// A transcription request's deadline: this, plus [`REALTIME_ALLOWANCE`] times the audio.
///
/// Sized by the audio rather than fixed, because a server answers nothing until it has
/// transcribed the whole recording — a three-hour meeting on a laptop's CPU is hours of
/// legitimate silence on the wire, and a deadline that fits it would guard nothing for a
/// thirty-second clip.
const BASE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Seconds of wall clock allowed per second of audio. Three times slower than realtime is
/// whisper-large on a CPU with nothing to spare; slower than that, something is wrong.
const REALTIME_ALLOWANCE: u64 = 3;

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

/// What the server returned.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transcript {
    /// As the server named it: `en` from whisper.cpp, `english` from OpenAI.
    pub language: String,
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

/// The server's answer, in the parts of `verbose_json` this reads. Unknown fields are
/// ignored; `segments` absent means the server did not honour `verbose_json`.
#[derive(Deserialize)]
struct VerboseJson {
    #[serde(default)]
    language: Option<String>,
    /// Seconds.
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    segments: Option<Vec<VerboseSegment>>,
}

#[derive(Deserialize)]
struct VerboseSegment {
    /// Seconds.
    start: f64,
    end: f64,
    text: String,
    #[serde(default)]
    no_speech_prob: Option<f32>,
}

/// Seconds as the milliseconds a [`Segment`] carries.
fn ms(seconds: f64) -> i64 {
    (seconds * 1000.0).round() as i64
}

/// Holds a response to the shape a transcript needs: timed segments.
///
/// A server that answered with text and no segments is refused rather than stored as one
/// untimed paragraph — every citation into it would point at `00:00:00`, and §6.4's whole
/// claim is that a hit is a timestamp. It is [`Fatal`] because every later recording would
/// come back the same.
fn parse_response(
    bytes: &[u8],
    requested_language: Option<&str>,
    audio_ms: u64,
) -> anyhow::Result<Transcript> {
    let response: VerboseJson = serde_json::from_slice(bytes).map_err(|e| {
        let head: String = String::from_utf8_lossy(bytes).chars().take(200).collect();
        anyhow::anyhow!("the transcription response did not parse: {e} — starts `{head}`")
    })?;
    let segments = response.segments.ok_or_else(|| {
        Fatal(
            "the transcription server answered without timed segments — it does not honour \
             response_format=verbose_json, and an untimed transcript cannot be cited"
                .to_string(),
        )
    })?;
    let segments: Vec<Segment> = segments
        .into_iter()
        .map(|s| Segment {
            start_ms: ms(s.start),
            end_ms: ms(s.end),
            text: s.text,
            no_speech_prob: s.no_speech_prob.unwrap_or(0.0),
        })
        .collect();
    let duration_ms = response
        .duration
        .map(|d| ms(d).max(0) as u64)
        .or_else(|| segments.last().map(|s| s.end_ms.max(0) as u64))
        .unwrap_or(audio_ms);
    Ok(Transcript {
        language: response
            .language
            .or_else(|| requested_language.map(str::to_string))
            .unwrap_or_else(|| "unknown".to_string()),
        duration_ms,
        segments,
    })
}

/// A transcription server, and the decoder that prepares audio for it.
#[derive(Debug)]
pub struct Transcriber {
    endpoint: Endpoint,
    /// The decoder, so a test can put something predictable in ffmpeg's place.
    decoder: PathBuf,
    pub language: Option<String>,
}

impl Transcriber {
    /// Checks the key now, so a missing one fails before the first recording is decoded.
    pub fn new(endpoint: Endpoint, language: Option<String>) -> anyhow::Result<Self> {
        endpoint.key()?;
        Ok(Self {
            endpoint,
            decoder: PathBuf::from(DECODER),
            language,
        })
    }

    /// The same transcriber with another program in ffmpeg's place.
    pub fn with_decoder(mut self, decoder: impl Into<PathBuf>) -> Self {
        self.decoder = decoder.into();
        self
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// The tier that produced a transcript, for [`crate::domain::Derivation`]: the model,
    /// and the server it ran on in the variant's place — `host:port`, never a path or a key.
    pub fn tier(&self) -> crate::domain::ModelTier {
        crate::domain::ModelTier {
            model_id: self.endpoint.model.clone(),
            variant: Some(self.endpoint.host()),
        }
    }

    /// Decodes `audio` and has it transcribed.
    ///
    /// `audio` is a path into the blob pool, so it has no extension — ffmpeg sniffs the
    /// container, which it does reliably for the m4a and webm YouTube serves.
    pub async fn transcribe(
        &self,
        audio: &Path,
        progress: &Progress,
    ) -> anyhow::Result<Transcript> {
        let scratch = tempfile::tempdir()?;
        let wav = scratch.path().join("audio.wav");
        let rate = SAMPLE_RATE.to_string();
        Tool::new(&self.decoder)
            .args(["-nostdin", "-loglevel", "error", "-y", "-i"])
            .arg(audio)
            .args(["-ar", &rate, "-ac", "1", "-c:a", "pcm_s16le", "-f", "wav"])
            .arg(&wav)
            .timeout(DECODE_TIMEOUT)
            .success()
            .await
            .map_err(|e| anyhow::anyhow!("{e} — it decodes audio for transcription"))?;

        let bytes = tokio::fs::metadata(&wav).await?.len();
        let seconds = bytes.saturating_sub(44) / BYTES_PER_SECOND;
        progress.say(format!(
            "{} of audio to {}",
            crate::render::duration(seconds as f64),
            self.endpoint
        ));

        // Built per recording, because the deadline is the recording's.
        let client = Client::new(
            self.endpoint.clone(),
            BASE_TIMEOUT + Duration::from_secs(REALTIME_ALLOWANCE * seconds),
        )?;
        let model = self.endpoint.model.clone();
        let language = self.language.clone();
        let response = client
            .post_form(|| {
                let wav = wav.clone();
                let model = model.clone();
                let language = language.clone();
                async move {
                    let file = tokio::fs::File::open(&wav).await?;
                    let part = reqwest::multipart::Part::stream_with_length(file, bytes)
                        .file_name("audio.wav")
                        .mime_str("audio/wav")?;
                    let mut form = reqwest::multipart::Form::new()
                        .text("model", model)
                        .text("response_format", "verbose_json")
                        .text("timestamp_granularities[]", "segment")
                        .part("file", part);
                    if let Some(language) = language {
                        form = form.text("language", language);
                    }
                    Ok::<_, anyhow::Error>(form)
                }
            })
            .await?;
        parse_response(&response, self.language.as_deref(), seconds * 1000)
    }
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
            language: "en".into(),
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

    // ── the request ───────────────────────────────────────────────────────────

    /// Seconds become milliseconds, and the server's own duration and language win.
    #[test]
    fn a_verbose_json_answer_becomes_timed_segments() {
        let body = serde_json::json!({
            "language": "en",
            "duration": 4273.5,
            "segments": [
                {"start": 0.0, "end": 1.82, "text": " Order.", "no_speech_prob": 0.01},
                {"start": 4271.0, "end": 4273.5, "text": " Item four."},
            ],
        });
        let t = parse_response(&serde_json::to_vec(&body).unwrap(), Some("es"), 0).unwrap();
        assert_eq!(t.language, "en");
        assert_eq!(t.duration_ms, 4_273_500);
        assert_eq!(t.segments[0].end_ms, 1_820);
        assert_eq!(t.segments[1].start_ms, 4_271_000);
        assert_eq!(t.segments[1].no_speech_prob, 0.0);
    }

    /// Plain `json` — text, no segments — is a server ignoring the request. Stored, every
    /// citation into it would read `00:00:00`.
    #[test]
    fn an_answer_without_segments_is_refused_for_good() {
        let body = serde_json::json!({"text": "The council meeting will come to order."});
        let err = parse_response(&serde_json::to_vec(&body).unwrap(), None, 0).unwrap_err();
        assert!(crate::inference::is_fatal(&err), "{err}");
        assert!(err.to_string().contains("verbose_json"), "{err}");
    }

    #[cfg(unix)]
    mod pipeline {
        use super::*;
        use crate::inference::Capability;

        fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        /// A decoder that writes some bytes to its last argument, where ffmpeg writes.
        fn decoder(dir: &Path) -> PathBuf {
            script(
                dir,
                "decoder",
                "for last; do :; done\nprintf 'RIFF-pretend-wav-bytes' > \"$last\"",
            )
        }

        /// The whole request: decoded, uploaded as a form the server accepts, and answered
        /// with segments at their offsets.
        #[tokio::test]
        async fn audio_is_decoded_uploaded_and_comes_back_timed() {
            let server = crate::inference::fake::serve().await;
            let dir = tempfile::tempdir().unwrap();
            let got = Transcriber::new(server.endpoint(Capability::Transcribe), Some("en".into()))
                .unwrap()
                .with_decoder(decoder(dir.path()))
                .transcribe(Path::new("/dev/null"), &Progress::none())
                .await
                .unwrap();

            assert_eq!(server.hits("audio/transcriptions"), 1);
            assert_eq!(got.segments.len(), 2);
            assert_eq!(got.segments[1].start_ms, 4_271_000);
            assert!(
                got.to_markdown(None).contains("[01:11:11] Item four"),
                "{}",
                got.to_markdown(None)
            );
        }

        /// ffmpeg's own words, not the server's confusion at an empty upload.
        #[tokio::test]
        async fn a_failing_decoder_says_why_and_sends_nothing() {
            let server = crate::inference::fake::serve().await;
            let dir = tempfile::tempdir().unwrap();
            let failing = script(
                dir.path(),
                "decoder",
                "echo 'Invalid data found when processing input' >&2\nexit 1",
            );
            let err = Transcriber::new(server.endpoint(Capability::Transcribe), None)
                .unwrap()
                .with_decoder(failing)
                .transcribe(Path::new("/tmp/not-audio"), &Progress::none())
                .await
                .unwrap_err()
                .to_string();
            assert!(err.contains("Invalid data"), "{err}");
            assert!(err.contains("decodes audio"), "{err}");
            assert_eq!(server.hits("audio/transcriptions"), 0);
        }

        #[tokio::test]
        async fn a_missing_decoder_names_what_it_was_for() {
            let server = crate::inference::fake::serve().await;
            let err = Transcriber::new(server.endpoint(Capability::Transcribe), None)
                .unwrap()
                .with_decoder("centinel-no-such-decoder")
                .transcribe(Path::new("/dev/null"), &Progress::none())
                .await
                .unwrap_err()
                .to_string();
            assert!(err.contains("not installed"), "{err}");
            assert!(err.contains("decodes audio"), "{err}");
        }
    }
}
