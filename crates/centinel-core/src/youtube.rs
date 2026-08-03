//! YouTube acquisition, through `yt-dlp`.
//!
//! ## There is no other way, and that is a researched conclusion
//!
//! `captions.download` in the official Data API *"requires the user to have permission to
//! edit the video"*. For a channel you do not own the sanctioned API cannot return
//! caption text at all, so every transcript path in every language is either a `yt-dlp`
//! wrapper or a reimplementation of YouTube's private `timedtext`/InnerTube endpoints —
//! there is no third category. In Rust the only crate that attempts the second is a
//! single `0.1.0` from May 2023 with 2,690 lifetime downloads. So `yt-dlp` is not an
//! implementation detail to be removed later; it is a **permanent runtime dependency**,
//! and this module treats it as one: pinned, version-probed, and parsed as untrusted.
//!
//! ## Failure is normal here, and it is not absence
//!
//! yt-dlp shipped 26 releases in 2025 in emergency clusters, with 185 open issues on the
//! bot-detection wall alone. Measured against a live channel while writing this module,
//! with yt-dlp 2026.03.17 (4½ months stale):
//!
//! ```text
//! --flat-playlist enumeration   ->  worked
//! per-video metadata            ->  "Sign in to confirm you're not a bot"
//! ```
//!
//! …on every one of `android_vr`, `tv`, `web_embedded`, `ios` and `mweb`. That is
//! [`Liveness::Blocked`] and emphatically **not** [`Liveness::Gone`]: the video is there,
//! we are being refused. SPEC §4.4 exists for exactly this distinction — recording a
//! bot-wall as absence would write a false disappearance into a transparency record.
//! [`classify`] is where that judgement is made, and it is the most load-bearing function
//! in this file.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::domain::Liveness;

pub const YT_DLP: &str = "yt-dlp";

/// yt-dlp warns at 90 days and this is why: past it, breakage is expected rather than
/// surprising. Surfaced by [`YtDlp::version`] so a failing run can say *"and your
/// yt-dlp is old"* instead of leaving the operator to guess.
pub const STALE_AFTER_DAYS: i64 = 90;

/// The canonical address of a video. Used as a [`crate::domain::Resource`] natural key,
/// so it is worth having exactly one spelling of it.
pub fn watch_url(video_id: &str) -> String {
    format!("https://www.youtube.com/watch?v={video_id}")
}

/// A citable link into a moment of a recording — §6.4's entire value proposition.
pub fn watch_url_at(video_id: &str, ms: u64) -> String {
    format!("{}&t={}s", watch_url(video_id), ms / 1000)
}

/// Sub-resources of a video.
///
/// A video's metadata, its captions and its audio are three **addresses**, not one thing
/// with three fields (SPEC §4.2). They change independently — a title edit, a re-run of
/// YouTube's ASR, and a re-encode are different events — so each gets its own
/// Observation history rather than being folded into a single fingerprint that could not
/// say which of them moved.
pub fn sub_resource(video_id: &str, part: Part) -> String {
    format!("{}#{}", watch_url(video_id), part.as_str())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The `yt-dlp -J` document.
    Metadata,
    /// A caption track, archived as `json3`.
    Captions,
    /// The audio stream.
    Audio,
}

impl Part {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Metadata => "metadata",
            Self::Captions => "captions.json3",
            Self::Audio => "audio",
        }
    }
}

/// One video, as flat enumeration reports it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoRef {
    pub id: String,
    pub title: String,
    /// Seconds. Absent for a live stream that has not ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
}

/// A channel's uploads, as one snapshot.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChannelListing {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    pub videos: Vec<VideoRef>,
}

/// A yt-dlp invocation that failed, already classified.
#[derive(Clone, Debug)]
pub struct YtFailure {
    pub state: Liveness,
    pub detail: String,
}

impl std::fmt::Display for YtFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.detail, self.state)
    }
}

impl std::error::Error for YtFailure {}

/// Reads yt-dlp's stderr and decides what it means for the record.
///
/// The ordering matters. Bot-detection and rate limiting are checked **before** the
/// generic "unavailable" phrasing, because yt-dlp prints both in some failures and only
/// the first is the truth about the resource. Getting this backwards would mark a live
/// video `Gone`.
pub fn classify(stderr: &str) -> YtFailure {
    let lower = stderr.to_ascii_lowercase();

    // Refused, not absent. The single most common failure in practice.
    const BLOCKED: &[&str] = &[
        "sign in to confirm",
        "confirm you're not a bot",
        "confirm you’re not a bot",
        "http error 429",
        "too many requests",
        "blocked it in your country",
        "this video is not available from your location",
        "requested format is not available", // usually a PO-token-gated format list
        "po token",
    ];
    // Genuinely gone, or genuinely never public to us.
    const GONE: &[&str] = &[
        "video unavailable",
        "this video has been removed",
        "private video",
        "members-only",
        "http error 404",
        "does not exist",
        "account associated with this video has been terminated",
    ];

    let first_line = stderr
        .lines()
        .find(|l| l.contains("ERROR"))
        .unwrap_or_else(|| stderr.lines().next_back().unwrap_or("yt-dlp failed"))
        .trim()
        .to_string();

    let state = if BLOCKED.iter().any(|needle| lower.contains(needle)) {
        Liveness::Blocked
    } else if GONE.iter().any(|needle| lower.contains(needle)) {
        Liveness::Gone
    } else {
        Liveness::Error
    };

    YtFailure {
        state,
        detail: first_line,
    }
}

/// A configured `yt-dlp`.
#[derive(Clone, Debug)]
pub struct YtDlp {
    binary: String,
    /// Passed through verbatim, e.g. `--cookies-from-browser firefox`. The escape hatch
    /// for the bot wall, which no amount of code here can argue with.
    extra_args: Vec<String>,
}

impl Default for YtDlp {
    fn default() -> Self {
        Self {
            binary: YT_DLP.to_string(),
            extra_args: Vec::new(),
        }
    }
}

impl YtDlp {
    pub fn new(binary: impl Into<String>, extra_args: Vec<String>) -> Self {
        Self {
            binary: binary.into(),
            extra_args,
        }
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.binary);
        // `--no-update` suppresses the staleness banner on every call; the check belongs
        // in `doctor`, once, not in the middle of a thousand-video crawl.
        // `--ignore-config` keeps a user's `~/.config/yt-dlp/config` from silently
        // changing output formats out from under the parser.
        cmd.args(["--no-update", "--ignore-config", "--no-warnings"]);
        cmd.args(&self.extra_args);
        cmd
    }

    pub async fn version(&self) -> anyhow::Result<String> {
        let out = Command::new(&self.binary)
            .arg("--version")
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("cannot run {}: {e}", self.binary))?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Enumerates a channel's uploads without fetching any video.
    ///
    /// `--flat-playlist` is one HTTP round trip per page and needs no API key. It is also
    /// the *only* path measured to still work through the bot wall, which is why
    /// discovery and acquisition are separate ops: a channel can be enumerated even on a
    /// day when nothing can be downloaded.
    pub async fn enumerate_channel(
        &self,
        channel_url: &str,
        limit: Option<usize>,
    ) -> Result<ChannelListing, YtFailure> {
        let mut cmd = self.command();
        cmd.args(["--flat-playlist", "-J"]);
        if let Some(n) = limit {
            cmd.args(["--playlist-end", &n.to_string()]);
        }
        cmd.arg(channel_url);

        let out = cmd.output().await.map_err(|e| YtFailure {
            state: Liveness::Error,
            detail: format!("cannot run {}: {e}", self.binary),
        })?;

        if !out.status.success() {
            return Err(classify(&String::from_utf8_lossy(&out.stderr)));
        }

        let doc: serde_json::Value =
            serde_json::from_slice(&out.stdout).map_err(|e| YtFailure {
                state: Liveness::Error,
                detail: format!("yt-dlp returned output that is not JSON: {e}"),
            })?;

        Ok(parse_listing(&doc))
    }

    /// The full `-J` metadata document for one video.
    pub async fn video_metadata(&self, video_id: &str) -> Result<Vec<u8>, YtFailure> {
        let mut cmd = self.command();
        cmd.args(["-J", "--skip-download"]).arg(watch_url(video_id));

        let out = cmd.output().await.map_err(|e| YtFailure {
            state: Liveness::Error,
            detail: format!("cannot run {}: {e}", self.binary),
        })?;

        if !out.status.success() {
            return Err(classify(&String::from_utf8_lossy(&out.stderr)));
        }
        Ok(out.stdout)
    }

    /// Downloads a caption track as `json3`.
    ///
    /// `json3` and not `srt`/`vtt`: it is the only format carrying **per-word** offsets
    /// (`segs[].tOffsetMs`), and the others are lossy conversions that collapse to cue
    /// timing. Archive the richest form; anything else can be derived from it later.
    ///
    /// `Ok(None)` means the video genuinely has no track in that language — a fact worth
    /// recording, and different from a failure to fetch one.
    pub async fn captions(
        &self,
        video_id: &str,
        lang: &str,
        work_dir: &Path,
    ) -> Result<Option<Vec<u8>>, YtFailure> {
        let mut cmd = self.command();
        cmd.args([
            // Manual tracks are strictly better where they exist — real punctuation and
            // reliable proper nouns, which is what a civic index is built around. Asking
            // for both lets yt-dlp prefer the manual one.
            "--write-subs",
            "--write-auto-subs",
            "--sub-langs",
            lang,
            "--sub-format",
            "json3",
            "--skip-download",
            "-o",
        ])
        .arg(work_dir.join("cap"))
        .arg(watch_url(video_id));

        let out = cmd.output().await.map_err(|e| YtFailure {
            state: Liveness::Error,
            detail: format!("cannot run {}: {e}", self.binary),
        })?;

        if !out.status.success() {
            return Err(classify(&String::from_utf8_lossy(&out.stderr)));
        }

        Ok(first_file_with_extension(work_dir, "json3"))
    }

    /// Downloads the audio stream.
    ///
    /// Prefers itag 139 — 48 kbps HE-AAC at 22.05 kHz, ~2.6x smaller than the
    /// alternatives. Whisper resamples everything to 16 kHz mono anyway, so fetching the
    /// 129 kbps stereo 44.1 kHz track means downloading four times the bytes in order to
    /// throw three quarters of them away. The fallbacks matter because 139 is not offered
    /// on every video.
    pub async fn audio(&self, video_id: &str, work_dir: &Path) -> Result<Vec<u8>, YtFailure> {
        let mut cmd = self.command();
        cmd.args([
            "-f",
            "139/bestaudio[abr<60]/bestaudio",
            // Write straight to the final name: this file is read once and turned into a
            // content-addressed blob, so a `.part` dance buys nothing.
            "--no-part",
            "-o",
        ])
        .arg(work_dir.join("audio.%(ext)s"))
        .arg(watch_url(video_id));

        let out = cmd.output().await.map_err(|e| YtFailure {
            state: Liveness::Error,
            detail: format!("cannot run {}: {e}", self.binary),
        })?;

        if !out.status.success() {
            return Err(classify(&String::from_utf8_lossy(&out.stderr)));
        }

        first_file_named(work_dir, "audio").ok_or_else(|| YtFailure {
            state: Liveness::Error,
            detail: "yt-dlp reported success but wrote no audio file".to_string(),
        })
    }
}

/// Pulls the fields we rely on out of a `--flat-playlist -J` document.
///
/// Hand-written rather than derived, because this is a **scraped** surface: yt-dlp's
/// per-entry fields are documented to be thin and have changed. Missing keys degrade to
/// `None` instead of failing the whole enumeration — losing a duration is not worth
/// losing a channel.
fn parse_listing(doc: &serde_json::Value) -> ChannelListing {
    let videos = doc
        .get("entries")
        .and_then(|e| e.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| {
                    let id = e.get("id")?.as_str()?.to_string();
                    Some(VideoRef {
                        title: e
                            .get("title")
                            .and_then(|t| t.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        duration_secs: e.get("duration").and_then(|d| d.as_f64()),
                        id,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    ChannelListing {
        channel_id: doc
            .get("channel_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        channel: doc
            .get("channel")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        videos,
    }
}

/// Metadata worth carrying onto an [`crate::domain::Observation`].
pub fn observation_meta(part: Part, extra: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut meta = BTreeMap::new();
    meta.insert(
        "content-type".to_string(),
        match part {
            Part::Metadata | Part::Captions => "application/json".to_string(),
            // Deliberately generic: the blob's magic bytes are what `transcribe` keys on,
            // and claiming `audio/mp4` for a stream that turned out to be webm would be
            // a small lie in the record.
            Part::Audio => "audio/*".to_string(),
        },
    );
    meta.insert("source-tool".to_string(), YT_DLP.to_string());
    for (k, v) in extra {
        meta.insert((*k).to_string(), (*v).to_string());
    }
    meta
}

fn first_file_with_extension(dir: &Path, ext: &str) -> Option<Vec<u8>> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == ext) {
            return std::fs::read(&path).ok();
        }
    }
    None
}

fn first_file_named(dir: &Path, stem: &str) -> Option<Vec<u8>> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_stem().is_some_and(|s| s == stem) {
            return std::fs::read(&path).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_video_has_one_canonical_address() {
        assert_eq!(
            watch_url("fxR1inkgnGY"),
            "https://www.youtube.com/watch?v=fxR1inkgnGY"
        );
    }

    /// §6.4: a hit has to become a link into the moment. 4,271,000 ms is 4271 seconds.
    #[test]
    fn a_timestamp_becomes_a_citable_link() {
        assert_eq!(
            watch_url_at("fxR1inkgnGY", 4_271_000),
            "https://www.youtube.com/watch?v=fxR1inkgnGY&t=4271s"
        );
    }

    /// Three addresses, not one row with three columns (§4.2).
    #[test]
    fn the_parts_of_a_video_are_distinct_addresses() {
        let parts = [Part::Metadata, Part::Captions, Part::Audio].map(|p| sub_resource("abc", p));
        let unique: std::collections::HashSet<_> = parts.iter().collect();
        assert_eq!(unique.len(), 3, "sub-resources collided: {parts:?}");
        assert!(parts.iter().all(|p| p.starts_with(&watch_url("abc"))));
    }

    /// The verbatim message from a live run against a real channel on 2026-08-03 with
    /// yt-dlp 2026.03.17. Marking this `Gone` would write a false disappearance into a
    /// transparency record — the video is there, we are being refused.
    #[test]
    fn the_bot_wall_is_blocked_not_gone() {
        let stderr = "WARNING: [youtube] No title found in player responses\n\
             ERROR: [youtube] fxR1inkgnGY: Sign in to confirm you’re not a bot. \
             Use --cookies-from-browser or --cookies for the authentication.";
        let f = classify(stderr);
        assert_eq!(f.state, Liveness::Blocked);
        assert!(f.detail.contains("Sign in to confirm"));
    }

    #[test]
    fn a_removed_video_is_gone() {
        let f = classify("ERROR: [youtube] abc: Video unavailable. This video has been removed");
        assert_eq!(f.state, Liveness::Gone);
    }

    #[test]
    fn a_private_video_is_gone_and_a_rate_limit_is_blocked() {
        assert_eq!(
            classify("ERROR: [youtube] abc: Private video. Sign in if you've been granted access")
                .state,
            Liveness::Gone
        );
        assert_eq!(
            classify("ERROR: unable to download: HTTP Error 429: Too Many Requests").state,
            Liveness::Blocked
        );
    }

    /// yt-dlp prints both phrasings in some bot-wall failures. Blocked must win, or a
    /// live video gets recorded as removed.
    #[test]
    fn blocked_wins_when_both_phrasings_appear() {
        let f = classify(
            "ERROR: [youtube] abc: Video unavailable. Sign in to confirm you're not a bot",
        );
        assert_eq!(
            f.state,
            Liveness::Blocked,
            "a bot wall that also says `unavailable` is still a bot wall"
        );
    }

    #[test]
    fn an_unrecognised_failure_is_an_error_not_a_guess() {
        let f = classify("ERROR: something nobody has seen before");
        assert_eq!(f.state, Liveness::Error);
    }

    /// The exact shape observed from `yt-dlp --flat-playlist -J` on a live channel.
    #[test]
    fn a_flat_playlist_document_parses() {
        let doc = serde_json::json!({
            "id": "UCx4-4RHo_bhTMQJNh6Du0AA",
            "channel": "City of Tampa",
            "channel_id": "UCx4-4RHo_bhTMQJNh6Du0AA",
            "entries": [
                { "id": "fxR1inkgnGY", "title": "Chief Bercaw Off the Clock",
                  "duration": 351.0, "url": "https://www.youtube.com/watch?v=fxR1inkgnGY" },
                { "id": "second", "title": "Council Meeting", "duration": null }
            ]
        });
        let listing = parse_listing(&doc);
        assert_eq!(listing.channel.as_deref(), Some("City of Tampa"));
        assert_eq!(listing.videos.len(), 2);
        assert_eq!(listing.videos[0].id, "fxR1inkgnGY");
        assert_eq!(listing.videos[0].duration_secs, Some(351.0));
        // A live stream has no duration; that must not drop the video.
        assert_eq!(listing.videos[1].duration_secs, None);
    }

    /// yt-dlp's flat entries are documented to be thin and have changed shape before.
    /// A missing field must cost that field, never the enumeration.
    #[test]
    fn entries_without_an_id_are_skipped_rather_than_fatal() {
        let doc = serde_json::json!({
            "entries": [ { "title": "no id here" }, { "id": "keeper" } ]
        });
        let listing = parse_listing(&doc);
        assert_eq!(listing.videos.len(), 1);
        assert_eq!(listing.videos[0].id, "keeper");
    }

    #[test]
    fn a_document_with_no_entries_is_an_empty_channel_not_a_parse_error() {
        assert!(parse_listing(&serde_json::json!({})).videos.is_empty());
    }
}
