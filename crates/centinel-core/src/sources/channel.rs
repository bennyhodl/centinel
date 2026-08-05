//! A YouTube channel as a [`Source`].
//!
//! SPEC §4 makes a channel a **peer** of a crawled site, differing only in acquisition:
//! enumeration is a playlist listing rather than a sitemap walk, and acquisition is a
//! `yt-dlp` subprocess rather than an HTTP GET. Everything downstream — blobs, the log,
//! chunking, embedding, search — is the shared model.
//!
//! That claim used to be made in a doc comment while a separate `youtube` op carried its
//! own copy of the resume logic, the liveness handling and the counters. This file is the
//! claim actually being true: what is here is the part that is genuinely different, and
//! nothing else.
//!
//! ## One address, three artifacts
//!
//! A video's metadata, captions and audio are three **addresses**, not one thing with
//! three fields (§4.2). They change independently — a title edit, a re-run of YouTube's
//! ASR, and a re-encode are different events — so each gets its own Observation history.
//! Returning all three from one [`Source::acquire`] is why the trait yields
//! [`Acquired`] rather than a single blob.
//!
//! ## Enumerate and acquire are separate on purpose
//!
//! Measured while writing this: on a stale yt-dlp, `--flat-playlist` enumeration worked
//! while every per-video call hit the bot wall. Splitting them means a blocked day still
//! produces a `DiscoveryRun` — the record of *what existed* — and acquisition resumes
//! later against it.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::future::BoxFuture;

use crate::domain::{
    Acquired, Enumeration, Fetched, Note, NoteMark, Refusal, Resource, Source, SourceId, SourceKind,
};
use crate::op::Progress;
use crate::youtube::{self, Part, YtDlp};

/// When to spend ~63 MB on a video's audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AudioPolicy {
    Never,
    /// Only for videos YouTube has no caption track for.
    ///
    /// **The default, and the one the measurements point at.** Sampling 42 recordings
    /// from a real council channel: 39 had auto-captions and 3 had `automatic_captions:
    /// 0` — YouTube simply never ran ASR on them. Those three were ordinary public 2–3
    /// hour meetings, indistinguishable from the rest, so the gap cannot be predicted
    /// from metadata and cannot be ignored: without audio they are permanently missing
    /// from the index while every meeting around them is searchable.
    #[default]
    IfNoCaptions,
    Always,
}

/// Soft failures kept for the report, bounded so a channel-wide outage cannot bury the
/// count that matters.
const MAX_REMARKS: usize = 10;

pub struct ChannelSource {
    id: SourceId,
    channel: String,
    yt: YtDlp,
    lang: String,
    captions: bool,
    audio: AudioPolicy,
    /// Stop enumerating after this many videos, newest first.
    limit: Option<usize>,
    /// Seconds between videos. yt-dlp against YouTube is rate-limited by bot detection
    /// long before politeness, so this is a real dial rather than a courtesy.
    delay: Duration,
    /// Whether a video has been acquired yet, so the delay does not precede the first.
    started: AtomicBool,
    /// Failures that did not cost us the video — a caption track that would not download
    /// while the metadata did. Not worth a `Refusal` (the video *was* acquired), too
    /// diagnostic to drop, so they come back through [`Source::remarks`].
    partial: Mutex<Vec<String>>,
}

impl ChannelSource {
    pub fn new(id: SourceId, channel: impl Into<String>, yt_dlp_args: Vec<String>) -> Self {
        Self {
            id,
            channel: channel.into(),
            yt: YtDlp::new(youtube::YT_DLP, yt_dlp_args),
            lang: "en".to_string(),
            captions: true,
            audio: AudioPolicy::default(),
            limit: None,
            delay: Duration::from_secs_f64(2.0),
            started: AtomicBool::new(false),
            partial: Mutex::new(Vec::new()),
        }
    }

    pub fn with_lang(mut self, lang: impl Into<String>) -> Self {
        self.lang = lang.into();
        self
    }

    pub fn with_audio(mut self, audio: AudioPolicy) -> Self {
        self.audio = audio;
        self
    }

    pub fn with_captions(mut self, captions: bool) -> Self {
        self.captions = captions;
        self
    }

    pub fn with_limit(mut self, limit: Option<usize>) -> Self {
        self.limit = limit;
        self
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    fn note_partial(&self, detail: String) {
        let mut partial = self.partial.lock().expect("remark list is never poisoned");
        if partial.len() < MAX_REMARKS {
            partial.push(detail);
        }
    }

    /// One artifact, addressed as a sub-resource of the video.
    fn artifact(&self, video_id: &str, part: Part, bytes: Vec<u8>, title: &str) -> Acquired {
        Acquired {
            resource: Resource::new(self.id.clone(), youtube::sub_resource(video_id, part)),
            fetched: Fetched {
                bytes,
                meta: youtube::observation_meta(part, &[("title", title)]),
            },
        }
    }
}

impl Source for ChannelSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn kind(&self) -> SourceKind {
        SourceKind::Channel
    }

    fn method(&self) -> &'static str {
        "playlist"
    }

    fn target(&self) -> &str {
        &self.channel
    }

    fn yields_audio(&self) -> bool {
        self.audio != AudioPolicy::Never
    }

    fn enumerate<'a>(
        &'a self,
        progress: &'a Progress,
    ) -> BoxFuture<'a, anyhow::Result<Enumeration>> {
        Box::pin(async move {
            let version = self.yt.version().await?;
            progress.say(format!("listing {} with yt-dlp {version}", self.channel));

            let listing = self
                .yt
                .enumerate_channel(&self.channel, self.limit)
                .await
                .map_err(|f| {
                    anyhow::anyhow!(
                        "{f}\n\
                         Enumeration is normally the path that survives the bot wall. If it \
                         did not, the documented workarounds are a newer yt-dlp and browser \
                         cookies:\n  \
                         centinel discover --source {} --channel {} \
                         --yt-dlp-arg=--cookies-from-browser=brave\n\
                         (note the `=`; `--yt-dlp-arg` takes the whole flag as one token)",
                        self.id,
                        self.channel,
                    )
                })?;

            let seconds: f64 = listing.videos.iter().filter_map(|v| v.duration_secs).sum();

            let mut notes = vec![
                Note::new("yt-dlp", version),
                Note::new("recording", format!("{:.1} hours", seconds / 3600.0)),
            ];
            // A council channel keeps its meetings in /streams, disjointly from /videos.
            // A tab that silently returned nothing is the difference between a complete
            // archive and a convincing partial one, so each tab states what it gave.
            for tab in &listing.tabs {
                notes.push(Note::ok_or_warn(
                    tab.url.rsplit('/').next().unwrap_or(&tab.url).to_string(),
                    format!(
                        "{} videos{}",
                        tab.videos,
                        if tab.duplicates > 0 {
                            format!(" ({} already seen)", tab.duplicates)
                        } else {
                            String::new()
                        }
                    ),
                    tab.videos > 0,
                ));
            }
            if listing.rejected > 0 {
                notes.push(Note::marked(
                    "rejected",
                    format!("{} entries that were not videos", listing.rejected),
                    NoteMark::Ok,
                ));
            }

            Ok(Enumeration {
                resources: listing
                    .videos
                    .iter()
                    .map(|v| Resource::new(self.id.clone(), youtube::watch_url(&v.id)))
                    .collect(),
                warnings: Vec::new(),
                notes,
                figures: BTreeMap::from([
                    ("rejected".to_string(), listing.rejected as u64),
                    ("tabs".to_string(), listing.tabs.len() as u64),
                    ("duration_secs".to_string(), seconds as u64),
                ]),
            })
        })
    }

    fn acquire<'a>(
        &'a self,
        resource: &'a Resource,
        progress: &'a Progress,
    ) -> BoxFuture<'a, Result<Vec<Acquired>, Refusal>> {
        Box::pin(async move {
            let Some(video_id) = video_id_of(&resource.natural_key) else {
                return Err(Refusal {
                    state: crate::domain::Liveness::Error,
                    detail: format!("`{}` is not a video address", resource.natural_key),
                });
            };

            if self.started.swap(true, Ordering::Relaxed) && !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }

            // A scratch directory that is removed with the video. yt-dlp writes files, the
            // store wants bytes, and nothing should outlive the conversion.
            let work = tempfile::tempdir().map_err(|e| Refusal {
                state: crate::domain::Liveness::Error,
                detail: format!("cannot create a working directory: {e}"),
            })?;

            let mut out = Vec::new();

            // ---- metadata ----------------------------------------------------------
            // The only artifact whose absence is a refusal: without it there is no video.
            let json = self.yt.video_metadata(&video_id).await?;
            let title = title_of(&json).unwrap_or_default();
            out.push(self.artifact(&video_id, Part::Metadata, json, &title));

            // ---- captions ----------------------------------------------------------
            let mut got_captions = false;
            if self.captions {
                match self.yt.captions(&video_id, &self.lang, work.path()).await {
                    // No track in this language is a fact, not a failure. Recording
                    // nothing is the honest outcome — an empty blob would claim we have
                    // captions.
                    Ok(None) => {}
                    Ok(Some(bytes)) => {
                        out.push(self.artifact(&video_id, Part::Captions, bytes, &title));
                        got_captions = true;
                    }
                    Err(f) => self.note_partial(format!("{video_id}: captions — {f}")),
                }
            }

            // ---- audio -------------------------------------------------------------
            // A caption *fetch failure* is not the same as YouTube having no track, and
            // only the latter should trigger a download — but both leave `got_captions`
            // false, so the fallback errs toward fetching. Spending 63 MB on a video that
            // turns out to have captions is cheaper than leaving a meeting out of the
            // index.
            let want_audio = match self.audio {
                AudioPolicy::Always => true,
                AudioPolicy::IfNoCaptions => !got_captions,
                AudioPolicy::Never => false,
            };
            if want_audio {
                progress.say(format!("{video_id} · audio"));
                match self.yt.audio(&video_id, work.path()).await {
                    Ok(audio) => out.push(self.artifact(&video_id, Part::Audio, audio, &title)),
                    Err(f) => self.note_partial(format!("{video_id}: audio — {f}")),
                }
            }

            Ok(out)
        })
    }

    /// A video is acquired when its **metadata** is stored.
    ///
    /// Not its captions or audio: those are separate addresses that may legitimately
    /// never exist, and keying resumption on them would re-fetch the whole catalogue
    /// every single run.
    fn marker(&self, resource: &Resource) -> Resource {
        match video_id_of(&resource.natural_key) {
            Some(id) => Resource::new(
                resource.source.clone(),
                youtube::sub_resource(&id, Part::Metadata),
            ),
            None => resource.clone(),
        }
    }

    fn remarks(&self, parts: &BTreeMap<String, usize>, attempted: usize) -> Vec<Note> {
        let mut notes = Vec::new();

        // The Whisper work-list. Measured at ~7% of a real council channel, unpredictable
        // from metadata, and invisible to search until transcribed — so it is stated as a
        // number and a command rather than left to be inferred from a parts table.
        let captioned = parts
            .get(Part::Captions.as_str())
            .copied()
            .unwrap_or_default();
        let stored = parts
            .get(Part::Metadata.as_str())
            .copied()
            .unwrap_or_default();
        if stored > captioned {
            let gap = stored - captioned;
            let audio = parts.get(Part::Audio.as_str()).copied().unwrap_or_default();
            notes.push(Note::marked(
                "no captions",
                if audio >= gap {
                    format!("{gap} without captions — audio was fetched for transcription")
                } else {
                    format!(
                        "{gap} without captions and no audio — \
                         centinel collect --source {} --audio-if-no-captions",
                        self.id
                    )
                },
                NoteMark::Warn,
            ));
        }

        for detail in self
            .partial
            .lock()
            .expect("remark list is never poisoned")
            .iter()
        {
            notes.push(Note::marked("partial", detail.clone(), NoteMark::Warn));
        }

        let _ = attempted;
        notes
    }
}

/// Whether this Source's provenance says a stored source is a channel.
///
/// The discriminator is the `DiscoveryRun::method` §4.3 records for exactly this kind of
/// question. The natural-key fallback covers a source collected with `ingest`, which
/// writes Observations and never a DiscoveryRun — and it lives here, beside the adapter
/// that knows what a YouTube address looks like, rather than in whatever op happened to
/// need the answer.
pub fn claims(method: &str, natural_keys: &[&str]) -> bool {
    if method == "playlist" {
        return true;
    }
    method.is_empty()
        && natural_keys
            .iter()
            .any(|k| k.contains("youtube.com/") || k.contains("youtu.be/"))
}

/// `https://www.youtube.com/watch?v=ID` → `ID`.
pub fn video_id_of(url: &str) -> Option<String> {
    url.split("v=")
        .nth(1)
        .map(|rest| rest.split(['&', '#']).next().unwrap_or(rest).to_string())
        .filter(|id| !id.is_empty())
}

fn title_of(metadata_json: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(metadata_json)
        .ok()?
        .get("title")?
        .as_str()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> ChannelSource {
        ChannelSource::new(
            SourceId::new("tampa-council").unwrap(),
            "https://www.youtube.com/@CityofTampa",
            Vec::new(),
        )
    }

    #[test]
    fn a_channel_declares_what_it_is_without_running_yt_dlp() {
        let s = source();
        assert_eq!(s.kind(), SourceKind::Channel);
        assert_eq!(s.method(), "playlist");
        assert_eq!(s.target(), "https://www.youtube.com/@CityofTampa");
        assert!(
            s.yields_audio(),
            "the default policy fetches audio for uncaptioned videos"
        );
        assert!(!s.with_audio(AudioPolicy::Never).yields_audio());
    }

    /// The line that keeps a whole catalogue from being re-fetched every run.
    #[test]
    fn a_video_is_acquired_when_its_metadata_is() {
        let s = source();
        let video = Resource::new(
            s.id().clone(),
            "https://www.youtube.com/watch?v=fxR1inkgnGY",
        );
        assert_eq!(
            s.marker(&video).natural_key,
            "https://www.youtube.com/watch?v=fxR1inkgnGY#metadata"
        );
    }

    #[test]
    fn an_address_that_is_not_a_video_falls_back_to_itself() {
        let s = source();
        let odd = Resource::new(s.id().clone(), "https://tampa.gov/not-a-video");
        assert_eq!(s.marker(&odd), odd);
    }

    #[test]
    fn a_video_id_survives_the_round_trip_through_a_watch_url() {
        assert_eq!(
            video_id_of("https://www.youtube.com/watch?v=fxR1inkgnGY").as_deref(),
            Some("fxR1inkgnGY")
        );
    }

    /// Sub-resource keys carry a `#part` fragment, and the id must not absorb it.
    #[test]
    fn a_sub_resource_key_still_yields_the_bare_video_id() {
        let key = youtube::sub_resource("fxR1inkgnGY", Part::Captions);
        assert_eq!(video_id_of(&key).as_deref(), Some("fxR1inkgnGY"));
    }

    #[test]
    fn extra_query_parameters_do_not_leak_into_the_id() {
        assert_eq!(
            video_id_of("https://www.youtube.com/watch?v=abc123&t=42s").as_deref(),
            Some("abc123")
        );
        assert_eq!(video_id_of("https://tampa.gov/not-a-video"), None);
    }

    #[test]
    fn a_title_is_read_from_the_metadata_document() {
        let json = br#"{"id":"abc","title":"City Council Meeting"}"#;
        assert_eq!(title_of(json).as_deref(), Some("City Council Meeting"));
        assert_eq!(title_of(b"not json"), None);
    }

    /// The transcription work-list has to be a number and a command, not a gap in a table.
    #[test]
    fn the_caption_gap_is_stated_and_says_what_to_do() {
        let s = source();
        let parts = BTreeMap::from([
            (Part::Metadata.as_str().to_string(), 42),
            (Part::Captions.as_str().to_string(), 39),
        ]);
        let notes = s.remarks(&parts, 42);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].detail.contains('3'), "{:?}", notes[0]);
        assert!(
            notes[0].detail.contains("--audio-if-no-captions"),
            "{:?}",
            notes[0]
        );
        assert_eq!(notes[0].mark, Some(NoteMark::Warn));
    }

    /// When the audio was already fetched, the work-list is filled and the note must not
    /// tell someone to go and fill it again.
    #[test]
    fn a_gap_already_covered_by_audio_says_so_instead() {
        let s = source();
        let parts = BTreeMap::from([
            (Part::Metadata.as_str().to_string(), 42),
            (Part::Captions.as_str().to_string(), 39),
            (Part::Audio.as_str().to_string(), 3),
        ]);
        let notes = s.remarks(&parts, 42);
        assert!(notes[0].detail.contains("transcription"), "{:?}", notes[0]);
        assert!(
            !notes[0].detail.contains("centinel collect"),
            "{:?}",
            notes[0]
        );
    }

    #[test]
    fn a_fully_captioned_channel_has_nothing_to_remark_on() {
        let s = source();
        let parts = BTreeMap::from([
            (Part::Metadata.as_str().to_string(), 12),
            (Part::Captions.as_str().to_string(), 12),
        ]);
        assert!(s.remarks(&parts, 12).is_empty());
    }

    /// A caption track that would not download does not cost us the video, but it must
    /// not vanish from the record either.
    #[test]
    fn a_partial_failure_comes_back_as_a_remark() {
        let s = source();
        s.note_partial("abc: captions — HTTP 429".into());
        let notes = s.remarks(&BTreeMap::new(), 1);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].detail.contains("429"), "{:?}", notes[0]);
    }

    #[test]
    fn the_remark_list_is_bounded() {
        let s = source();
        for i in 0..50 {
            s.note_partial(format!("video {i} failed"));
        }
        assert_eq!(s.remarks(&BTreeMap::new(), 50).len(), MAX_REMARKS);
    }

    // ── recovering a kind from the store ───────────────────────────────────────

    #[test]
    fn a_recorded_playlist_method_is_conclusive() {
        assert!(claims("playlist", &[]));
        assert!(!claims("sitemap", &["https://www.youtube.com/watch?v=x"]));
    }

    /// `ingest` writes Observations and never a DiscoveryRun, so there is no method to
    /// read — the addresses are all that is left to go on.
    #[test]
    fn without_a_method_the_addresses_decide() {
        assert!(claims("", &["https://www.youtube.com/watch?v=abc"]));
        assert!(claims("", &["https://youtu.be/abc"]));
        assert!(!claims("", &["https://www.tampa.gov/a"]));
        assert!(!claims("", &[]));
    }
}
