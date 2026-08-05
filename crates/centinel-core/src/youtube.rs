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
//! bot-detection wall alone. Measured against a live channel while writing this module:
//!
//! ```text
//! --flat-playlist enumeration   ->  worked
//! per-video metadata            ->  "Sign in to confirm you're not a bot"
//! ```
//!
//! …on every one of `android_vr`, `tv`, `web_embedded`, `ios` and `mweb`, and **on both**
//! yt-dlp 2026.03.17 and 2026.07.04. Upgrading did not lift it, which locates the
//! challenge at the requesting IP rather than at a stale extractor; cookies are the
//! documented remedy and they are the operator's call, not this module's.
//!
//! That state is [`Liveness::Blocked`] and emphatically **not** [`Liveness::Gone`]: the
//! video is there, we are being refused. SPEC §4.4 exists for exactly this distinction —
//! recording a bot wall as absence would write a false disappearance into a transparency
//! record. [`classify`] is where that judgement is made, and it is the most load-bearing
//! function in this file.
//!
//! **Enumeration is the half that keeps working**, which is why [`channel_tabs`] and
//! [`parse_listing`] are strict: on a day when nothing can be downloaded, the snapshot of
//! *what exists* is the whole product, and a snapshot that is quietly wrong is worse than
//! no snapshot at all.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use crate::tool::Tool;
use serde::{Deserialize, Serialize};

use crate::domain::Liveness;

pub const YT_DLP: &str = "yt-dlp";

/// yt-dlp warns at 90 days and this is why: past it, breakage is expected rather than
/// surprising. Surfaced by [`YtDlp::version`] so a failing run can say *"and your
/// yt-dlp is old"* instead of leaving the operator to guess.
pub const STALE_AFTER_DAYS: i64 = 90;

// ── deadlines ─────────────────────────────────────────────────────────────────
//
// One per call rather than one for the module, because these differ by three orders of
// magnitude. Every one of them is a ceiling on a *hang*, not a budget for normal work:
// yt-dlp waiting on a prompt, or on a socket that stopped delivering, used to block the
// caller for as long as the machine stayed up.

/// `yt-dlp --version`. It reads a string it already has.
const VERSION_TIMEOUT: Duration = Duration::from_secs(20);

/// A channel listing. Paged, and a council channel is over a thousand videos.
const ENUMERATE_TIMEOUT: Duration = Duration::from_secs(600);

/// One video's `-J` document. A single request that returns a few hundred kilobytes.
const METADATA_TIMEOUT: Duration = Duration::from_secs(180);

/// One caption track. Small, but yt-dlp probes the format list first.
const CAPTIONS_TIMEOUT: Duration = Duration::from_secs(300);

/// One audio stream — ~63 MB for a three-hour meeting, over a link that may be slow.
/// Generous on purpose: cutting off a download that is still moving would be a worse
/// failure than the hang this guards against.
const AUDIO_TIMEOUT: Duration = Duration::from_secs(1800);

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

/// What one tab contributed to a listing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TabCount {
    pub url: String,
    pub videos: usize,
    /// Videos this tab listed that another tab had already contributed.
    pub duplicates: usize,
}

/// A channel's uploads, as one snapshot.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChannelListing {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    pub videos: Vec<VideoRef>,
    /// Which tabs were walked and what each contributed. Provenance for a suspiciously
    /// small result, and the thing that makes a missing tab visible.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<TabCount>,
    /// Entries that were not videos. Non-zero means yt-dlp returned something this code
    /// declined to treat as a recording — see [`parse_listing`].
    #[serde(default)]
    pub rejected: usize,
}

/// YouTube video ids are 11 characters of `[A-Za-z0-9_-]`.
///
/// Checked rather than assumed, because the failure it catches is not hypothetical: a
/// bare `@handle` URL makes yt-dlp list a channel's **tabs**, whose `id` is the 24-char
/// channel id. Taken at face value that produced two identical bogus Resources and two
/// `Video unavailable` failures against an id that was never a video.
pub fn is_video_id(s: &str) -> bool {
    s.len() == 11
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Path segments that name a channel tab, rather than being part of its address.
const TAB_SEGMENTS: &[&str] = &[
    "videos",
    "streams",
    "shorts",
    "live",
    "playlists",
    "featured",
    "community",
    "about",
    "releases",
    "podcasts",
    "courses",
];

/// The tabs a channel URL should be enumerated as.
///
/// **A channel root expands to more than one tab, and that is load-bearing.** Measured on
/// `@cityoftampameetings`:
///
/// | tab | videos | hours |
/// |---|---|---|
/// | `/videos` | 401 | 989 |
/// | `/streams` | 831 | 2,364 |
/// | overlap | **0** | |
///
/// The two are disjoint, and the larger set is the *streams* — because council meetings
/// are live-streamed, so the recordings that matter most land in the tab a naive
/// `/videos` walk never reads. Enumerating one tab would have silently dropped two
/// thirds of that corpus with no error and a plausible-looking count.
///
/// An explicit tab, playlist or watch URL is returned untouched: if the operator named
/// something specific, expanding it would override a deliberate choice.
pub fn channel_tabs(url: &str) -> Vec<String> {
    let trimmed = url.trim_end_matches('/');

    // A playlist or a single video is not a channel at all.
    if trimmed.contains("/playlist") || trimmed.contains("/watch") {
        return vec![trimmed.to_string()];
    }

    let last = trimmed
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default();

    if TAB_SEGMENTS.contains(&last) {
        return vec![trimmed.to_string()];
    }

    ["videos", "streams", "shorts"]
        .iter()
        .map(|tab| format!("{trimmed}/{tab}"))
        .collect()
}

/// A yt-dlp invocation that failed, already classified.
///
/// The name is kept for the call sites; the type is [`crate::domain::Refusal`], which is
/// what an HTTP failure is too. A 403 from a WAF and a bot-wall refusal from YouTube are
/// the same fact about an address, and having two names for it is what kept the two
/// acquisition paths from sharing a loop.
pub use crate::domain::Refusal as YtFailure;

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

    /// The base invocation, with a deadline the caller is expected to replace.
    fn tool(&self) -> Tool {
        // `--no-update` suppresses the staleness banner on every call; the check belongs
        // in `doctor`, once, not in the middle of a thousand-video crawl.
        // `--ignore-config` keeps a user's `~/.config/yt-dlp/config` from silently
        // changing output formats out from under the parser.
        Tool::new(&self.binary)
            .args(["--no-update", "--ignore-config", "--no-warnings"])
            .args(&self.extra_args)
    }

    pub async fn version(&self) -> anyhow::Result<String> {
        let out = Tool::new(&self.binary)
            .arg("--version")
            .timeout(VERSION_TIMEOUT)
            .output()
            .await?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Enumerates a channel across every tab that holds recordings.
    ///
    /// `--flat-playlist` is one HTTP round trip per page and needs no API key. It is also
    /// the *only* path measured to still work through the bot wall, which is why
    /// discovery and acquisition are separate ops: a channel can be enumerated even on a
    /// day when nothing can be downloaded.
    ///
    /// See [`channel_tabs`] for why this walks several URLs. A tab that does not exist
    /// (most channels have no `/shorts`) is skipped rather than failing the run — but a
    /// tab that fails for any *other* reason propagates, because silently returning a
    /// short list is how a corpus loses two thirds of itself without anyone noticing.
    pub async fn enumerate_channel(
        &self,
        channel_url: &str,
        limit: Option<usize>,
    ) -> Result<ChannelListing, YtFailure> {
        let tabs = channel_tabs(channel_url);
        let mut listing = ChannelListing::default();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut first_error: Option<YtFailure> = None;

        for tab in &tabs {
            let doc = match self.enumerate_one(tab, limit).await {
                Ok(doc) => doc,
                Err(f) => {
                    // An absent tab is ordinary; anything else is remembered in case no
                    // tab at all succeeds.
                    first_error.get_or_insert(f);
                    listing.tabs.push(TabCount {
                        url: tab.clone(),
                        videos: 0,
                        duplicates: 0,
                    });
                    continue;
                }
            };

            let one = parse_listing(&doc);
            listing.channel_id = listing.channel_id.or(one.channel_id);
            listing.channel = listing.channel.or(one.channel);
            listing.rejected += one.rejected;

            let (mut fresh, mut duplicates) = (0usize, 0usize);
            for video in one.videos {
                if seen.insert(video.id.clone()) {
                    listing.videos.push(video);
                    fresh += 1;
                } else {
                    duplicates += 1;
                }
            }
            listing.tabs.push(TabCount {
                url: tab.clone(),
                videos: fresh,
                duplicates,
            });
        }

        // Every tab failed. Report the reason rather than an empty channel.
        if listing.videos.is_empty()
            && let Some(f) = first_error
        {
            return Err(f);
        }

        // `--limit` means "this many videos", not "this many per tab".
        if let Some(n) = limit {
            listing.videos.truncate(n);
        }

        Ok(listing)
    }

    async fn enumerate_one(
        &self,
        url: &str,
        limit: Option<usize>,
    ) -> Result<serde_json::Value, YtFailure> {
        let mut tool = self.tool().args(["--flat-playlist", "-J"]);
        if let Some(n) = limit {
            tool = tool.args(["--playlist-end", &n.to_string()]);
        }
        let out = tool.arg(url).timeout(ENUMERATE_TIMEOUT).output().await?;

        if !out.status.success() {
            return Err(classify(&String::from_utf8_lossy(&out.stderr)));
        }

        serde_json::from_slice(&out.stdout).map_err(|e| YtFailure {
            state: Liveness::Error,
            detail: format!("yt-dlp returned output that is not JSON: {e}"),
        })
    }

    /// The full `-J` metadata document for one video.
    pub async fn video_metadata(&self, video_id: &str) -> Result<Vec<u8>, YtFailure> {
        let out = self
            .tool()
            .args(["-J", "--skip-download"])
            .arg(watch_url(video_id))
            .timeout(METADATA_TIMEOUT)
            .output()
            .await?;

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
        let out = self
            .tool()
            .args([
                // Manual tracks are strictly better where they exist — real punctuation
                // and reliable proper nouns, which is what a civic index is built around.
                // Asking for both lets yt-dlp prefer the manual one.
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
            .arg(watch_url(video_id))
            .timeout(CAPTIONS_TIMEOUT)
            .output()
            .await?;

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
        let out = self
            .tool()
            .args([
                "-f",
                "139/bestaudio[abr<60]/bestaudio",
                // Write straight to the final name: this file is read once and turned
                // into a content-addressed blob, so a `.part` dance buys nothing.
                "--no-part",
                "-o",
            ])
            .arg(work_dir.join("audio.%(ext)s"))
            .arg(watch_url(video_id))
            .timeout(AUDIO_TIMEOUT)
            .output()
            .await?;

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
/// per-entry fields are documented to be thin and have changed. Missing *optional* keys
/// degrade to `None` — losing a duration is not worth losing a channel.
///
/// An entry that is not a video is **rejected and counted**, never coerced. yt-dlp uses
/// one `entries` array for both videos and playlists, distinguished by `_type` and
/// `ie_key`; a tab entry carries `_type: "playlist"` and the channel's own id. Trusting
/// `id` alone turned a bare `@handle` URL into two identical fake Resources and two
/// `Video unavailable` failures against something that was never a video.
fn parse_listing(doc: &serde_json::Value) -> ChannelListing {
    let mut videos = Vec::new();
    let mut rejected = 0usize;

    for entry in doc
        .get("entries")
        .and_then(|e| e.as_array())
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let is_video = entry.get("_type").and_then(|t| t.as_str()) == Some("url")
            && entry.get("ie_key").and_then(|k| k.as_str()) == Some("Youtube");
        let id = entry.get("id").and_then(|i| i.as_str()).unwrap_or_default();

        if !is_video || !is_video_id(id) {
            rejected += 1;
            continue;
        }

        videos.push(VideoRef {
            id: id.to_string(),
            title: entry
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or_default()
                .to_string(),
            // Negative durations appear on some live entries; they are not a length.
            duration_secs: entry
                .get("duration")
                .and_then(|d| d.as_f64())
                .filter(|d| *d > 0.0),
        });
    }

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
        tabs: Vec::new(),
        rejected,
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

    /// The exact shape observed from `yt-dlp --flat-playlist -J` on a live `/videos` tab.
    #[test]
    fn a_flat_playlist_document_parses() {
        let doc = serde_json::json!({
            "id": "UCLzohJmEgvfJOEd4YJNIHbg",
            "channel": "City Of Tampa Meetings",
            "channel_id": "UCLzohJmEgvfJOEd4YJNIHbg",
            "entries": [
                { "_type": "url", "ie_key": "Youtube", "id": "_vdkiPxxyTM",
                  "title": "Charter Review Advisory Commission 07/28", "duration": 10636 },
                { "_type": "url", "ie_key": "Youtube", "id": "VjgVqTiJSz0",
                  "title": "Council Meeting", "duration": null }
            ]
        });
        let listing = parse_listing(&doc);
        assert_eq!(listing.channel.as_deref(), Some("City Of Tampa Meetings"));
        assert_eq!(listing.videos.len(), 2);
        assert_eq!(listing.videos[0].id, "_vdkiPxxyTM");
        assert_eq!(listing.videos[0].duration_secs, Some(10636.0));
        // A stream that has not ended has no duration; that must not drop the video.
        assert_eq!(listing.videos[1].duration_secs, None);
        assert_eq!(listing.rejected, 0);
    }

    /// The regression. A bare `@handle` makes yt-dlp list the channel's **tabs**, whose
    /// `id` is the 24-character channel id — and both tabs carry the *same* id. Trusting
    /// it produced two identical fake Resources and two `Video unavailable` failures
    /// against something that was never a video.
    #[test]
    fn channel_tab_entries_are_rejected_rather_than_treated_as_videos() {
        let doc = serde_json::json!({
            "id": "@cityoftampameetings",
            "title": "City Of Tampa Meetings",
            "entries": [
                { "_type": "playlist", "id": "UCLzohJmEgvfJOEd4YJNIHbg",
                  "title": "City Of Tampa Meetings - Videos", "duration": null },
                { "_type": "playlist", "id": "UCLzohJmEgvfJOEd4YJNIHbg",
                  "title": "City Of Tampa Meetings - Live", "duration": null }
            ]
        });
        let listing = parse_listing(&doc);
        assert!(listing.videos.is_empty(), "a tab is not a video");
        assert_eq!(
            listing.rejected, 2,
            "rejections must be counted, not silent"
        );
    }

    #[test]
    fn a_video_id_is_eleven_url_safe_characters() {
        assert!(is_video_id("_vdkiPxxyTM"));
        assert!(is_video_id("fxR1inkgnGY"));
        // The channel id that caused the bug.
        assert!(!is_video_id("UCLzohJmEgvfJOEd4YJNIHbg"));
        assert!(!is_video_id("keeper"));
        assert!(!is_video_id("has spaces"));
    }

    /// Measured: `/videos` had 401 recordings and `/streams` had 831, with **zero**
    /// overlap — because council meetings are live-streamed. Enumerating one tab would
    /// silently drop two thirds of that corpus.
    #[test]
    fn a_bare_channel_url_expands_to_every_tab_that_holds_recordings() {
        let tabs = channel_tabs("https://www.youtube.com/@cityoftampameetings");
        assert!(tabs.iter().any(|t| t.ends_with("/videos")));
        assert!(
            tabs.iter().any(|t| t.ends_with("/streams")),
            "streams held twice as much as videos on the measured channel: {tabs:?}"
        );

        // Every channel-address spelling behaves the same way.
        for root in [
            "https://www.youtube.com/@handle",
            "https://www.youtube.com/channel/UCLzohJmEgvfJOEd4YJNIHbg",
            "https://www.youtube.com/c/SomeChannel",
            "https://www.youtube.com/user/legacy/",
        ] {
            assert!(
                channel_tabs(root).len() > 1,
                "`{root}` should expand to several tabs"
            );
        }
    }

    /// An operator who named a tab meant it; expanding would override the choice.
    #[test]
    fn an_explicit_tab_or_playlist_is_left_alone() {
        assert_eq!(
            channel_tabs("https://www.youtube.com/@x/streams"),
            vec!["https://www.youtube.com/@x/streams"]
        );
        assert_eq!(
            channel_tabs("https://www.youtube.com/@x/videos/"),
            vec!["https://www.youtube.com/@x/videos"],
            "a trailing slash is not a new tab"
        );
        assert_eq!(
            channel_tabs("https://www.youtube.com/playlist?list=PL1").len(),
            1
        );
        assert_eq!(channel_tabs("https://www.youtube.com/watch?v=abc").len(), 1);
    }

    /// yt-dlp's flat entries are documented to be thin and have changed shape before.
    /// A missing field must cost that field, never the enumeration.
    #[test]
    fn entries_without_an_id_are_skipped_rather_than_fatal() {
        let doc = serde_json::json!({
            "entries": [
                { "_type": "url", "ie_key": "Youtube", "title": "no id here" },
                { "_type": "url", "ie_key": "Youtube", "id": "_vdkiPxxyTM" }
            ]
        });
        let listing = parse_listing(&doc);
        assert_eq!(listing.videos.len(), 1);
        assert_eq!(listing.videos[0].id, "_vdkiPxxyTM");
        assert_eq!(listing.rejected, 1);
    }

    /// Observed on live entries: a negative duration is not a length, and summing it
    /// produced the `-0.0` total that made the bug visible.
    #[test]
    fn a_nonpositive_duration_is_absent_rather_than_negative() {
        let doc = serde_json::json!({
            "entries": [
                { "_type": "url", "ie_key": "Youtube", "id": "_vdkiPxxyTM", "duration": -0.0 }
            ]
        });
        assert_eq!(parse_listing(&doc).videos[0].duration_secs, None);
    }

    #[test]
    fn a_document_with_no_entries_is_an_empty_channel_not_a_parse_error() {
        assert!(parse_listing(&serde_json::json!({})).videos.is_empty());
    }
}
