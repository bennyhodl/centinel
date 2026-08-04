//! `youtube` — a channel as a Source.
//!
//! SPEC §4 makes `YouTubeChannel` a peer of `CrawledSite`, differing only in acquisition:
//! *discover* is a playlist listing rather than a sitemap walk, and *fetch* is a `yt-dlp`
//! subprocess rather than an HTTP GET. Everything downstream — blobs, the log, chunking,
//! embedding, search — is the shared model, which is the §4.1 promise that variation
//! stays quarantined at the edge.
//!
//! ## Why this is not `discover` and `collect`
//!
//! Those two ops speak HTTP: `collect` parses a natural key as a URL, paces per host and
//! reads `content-type`. A YouTube video is three addresses behind one subprocess with
//! its own failure vocabulary. Bolting that onto `collect` would put a `if source is
//! youtube` branch in the middle of the crawler — exactly the variation §4.1 wants
//! confined to acquisition. So the shapes are shared and the verbs are separate.
//!
//! ## Enumerate and acquire are separate on purpose
//!
//! Measured while writing this: on a stale yt-dlp, `--flat-playlist` enumeration worked
//! while every per-video call hit the bot wall. Splitting them means a blocked day still
//! produces a `DiscoveryRun` — the record of *what existed* — and acquisition resumes
//! later against it.

use std::collections::HashSet;

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::prelude::*;
use crate::store::LogRecord;
use crate::youtube::{self, Part, YtDlp};

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct YoutubeArgs {
    #[command(subcommand)]
    pub action: YoutubeAction,
}

#[derive(Clone, Debug, clap::Subcommand, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum YoutubeAction {
    /// List a channel's uploads into a DiscoveryRun. No video is fetched.
    Discover(DiscoverChannelArgs),
    /// Fetch metadata, captions and audio for discovered videos.
    Fetch(FetchArgs),
}

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct DiscoverChannelArgs {
    /// Source id to file this channel under, e.g. `tampa-council`.
    #[arg(long)]
    pub source: String,

    /// Channel URL — `https://www.youtube.com/@CityofTampa` or a `/videos` tab.
    #[arg(long)]
    pub channel: String,

    /// Stop after this many videos, newest first.
    #[arg(long)]
    #[serde(default)]
    pub limit: Option<usize>,

    /// Extra arguments for yt-dlp, e.g. `--yt-dlp-arg=--cookies-from-browser=brave`.
    ///
    /// The escape hatch for the bot wall, which no amount of code here can argue with.
    /// `allow_hyphen_values` because every argument worth passing starts with `--`, and
    /// clap would otherwise read it as an unknown flag of our own.
    #[arg(long = "yt-dlp-arg", allow_hyphen_values = true)]
    #[serde(default)]
    pub yt_dlp_args: Vec<String>,
}

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct FetchArgs {
    /// Source to fetch, as used by `youtube discover`.
    #[arg(long)]
    pub source: String,

    /// Stop after this many videos.
    #[arg(long)]
    #[serde(default)]
    pub limit: Option<usize>,

    /// Caption language.
    #[arg(long, default_value = "en")]
    #[serde(default = "default_lang")]
    pub lang: String,

    /// Download audio for every video. ~63 MB per 3-hour meeting.
    #[arg(long)]
    #[serde(default)]
    pub audio: bool,

    /// Download audio only for videos YouTube has no caption track for.
    ///
    /// **The recommended mode, and the one the measurements point at.** Sampling 42
    /// recordings from a real council channel: 39 had auto-captions and 3 had
    /// `automatic_captions: 0` — YouTube simply never ran ASR on them. Those three were
    /// ordinary public 2–3 hour meetings, indistinguishable from the rest, so the gap
    /// cannot be predicted from metadata and cannot be ignored: without audio they are
    /// permanently missing from the index while every meeting around them is searchable.
    ///
    /// This fetches audio for exactly that ~7%, turning a whole-catalogue transcription
    /// job into a bounded one.
    #[arg(long)]
    #[serde(default)]
    pub audio_if_no_captions: bool,

    /// Skip captions.
    #[arg(long)]
    #[serde(default)]
    pub no_captions: bool,

    /// Re-fetch videos already in the store.
    #[arg(long)]
    #[serde(default)]
    pub refresh: bool,

    /// Seconds to wait between videos. yt-dlp against YouTube is rate-limited by
    /// bot detection long before politeness, so this is a real dial.
    #[arg(long, default_value_t = 2.0)]
    #[serde(default = "default_delay")]
    pub delay_secs: f64,

    /// Extra arguments for yt-dlp.
    #[arg(long = "yt-dlp-arg")]
    #[serde(default)]
    pub yt_dlp_args: Vec<String>,
}

fn default_lang() -> String {
    "en".to_string()
}
fn default_delay() -> f64 {
    2.0
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct VideoOutcome {
    pub video_id: String,
    pub title: String,
    /// Which parts were stored: `metadata`, `captions.json3`, `audio`.
    pub stored: Vec<String>,
    pub bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<Liveness>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum YoutubeReport {
    Discover {
        source: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        channel: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        channel_id: Option<String>,
        yt_dlp_version: String,
        /// Every video in this snapshot. The DiscoveryRun records **all** of them —
        /// discovery is a full snapshot, not a delta (§4.3).
        videos: usize,
        /// How many of those no previous run had seen. A counter for the operator, not a
        /// filter: `videos` is what was stored.
        new_videos: usize,
        /// Which channel tabs were walked, and what each contributed. A council channel
        /// keeps its meetings in `/streams`, disjointly from `/videos`, so this is the
        /// line that shows a tab silently returning nothing.
        tabs: Vec<crate::youtube::TabCount>,
        /// Entries yt-dlp returned that were not videos.
        rejected: usize,
        total_duration_secs: f64,
        total_duration_hours: f64,
    },
    Fetch {
        source: String,
        yt_dlp_version: String,
        discovered: usize,
        already_had: usize,
        attempted: usize,
        stored: usize,
        failed: usize,
        bytes: u64,
        /// Failures that were refusals rather than absence. A non-zero count with zero
        /// successes is the bot wall, and the report should make that legible instead of
        /// looking like an empty channel.
        blocked: usize,
        /// Videos this run stored no caption track for — YouTube has none, or the fetch
        /// failed. **This is the Whisper work-list.** Measured at ~7% of a real council
        /// channel, unpredictable from metadata, and invisible to search until
        /// transcribed. `--audio-if-no-captions` is how it gets filled.
        no_captions: usize,
        videos: Vec<VideoOutcome>,
    },
}

/// Enumerate and fetch a YouTube channel.
#[op(long_running)]
pub async fn youtube(
    ctx: &Ctx,
    args: YoutubeArgs,
    progress: &Progress,
) -> anyhow::Result<YoutubeReport> {
    match args.action {
        YoutubeAction::Discover(a) => discover_channel(ctx, a, progress).await,
        YoutubeAction::Fetch(a) => fetch(ctx, a, progress).await,
    }
}

async fn discover_channel(
    ctx: &Ctx,
    args: DiscoverChannelArgs,
    progress: &Progress,
) -> anyhow::Result<YoutubeReport> {
    let source = SourceId::new(args.source.clone())?;
    let yt = YtDlp::new(youtube::YT_DLP, args.yt_dlp_args.clone());
    let version = yt.version().await?;

    progress.say(format!("listing {} with yt-dlp {version}", args.channel));

    let listing = yt
        .enumerate_channel(&args.channel, args.limit)
        .await
        .map_err(|f| {
            anyhow::anyhow!(
                "{f}\n\
                 Enumeration is normally the path that survives the bot wall. If it did \
                 not, the documented workarounds are a newer yt-dlp and browser cookies:\n  \
                 centinel youtube discover --source {} --channel {} \
                 --yt-dlp-arg=--cookies-from-browser=brave\n\
                 (note the `=`; `--yt-dlp-arg` takes the whole flag as one token)",
                args.source,
                args.channel,
            )
        })?;

    // What previous runs already knew, so the report can say what is genuinely new.
    let seen: HashSet<String> = ctx
        .store
        .read_log(&source)
        .await?
        .iter()
        .filter_map(|r| match r {
            LogRecord::DiscoveryRun(d) => Some(d.resources.clone()),
            _ => None,
        })
        .flatten()
        .map(|r| r.natural_key)
        .collect();

    let resources: Vec<Resource> = listing
        .videos
        .iter()
        .map(|v| Resource::new(source.clone(), youtube::watch_url(&v.id)))
        .collect();

    let new_videos = resources
        .iter()
        .filter(|r| !seen.contains(&r.natural_key))
        .count();

    ctx.store
        .append(
            &source,
            &LogRecord::DiscoveryRun(DiscoveryRun {
                source: source.clone(),
                at: Timestamp::now(),
                resources,
                // §4 names this Source's discovery method; recorded as provenance for a
                // suspiciously small snapshot.
                method: "playlist".to_string(),
            }),
        )
        .await?;

    let seconds: f64 = listing.videos.iter().filter_map(|v| v.duration_secs).sum();
    progress.say(format!(
        "{} videos, {:.0} hours ({} new)",
        listing.videos.len(),
        seconds / 3600.0,
        new_videos
    ));

    Ok(YoutubeReport::Discover {
        source: args.source,
        channel: listing.channel,
        channel_id: listing.channel_id,
        yt_dlp_version: version,
        videos: listing.videos.len(),
        new_videos,
        tabs: listing.tabs,
        rejected: listing.rejected,
        total_duration_secs: seconds,
        total_duration_hours: (seconds / 36.0).round() / 100.0,
    })
}

async fn fetch(ctx: &Ctx, args: FetchArgs, progress: &Progress) -> anyhow::Result<YoutubeReport> {
    let source = SourceId::new(args.source.clone())?;
    let yt = YtDlp::new(youtube::YT_DLP, args.yt_dlp_args.clone());
    let version = yt.version().await?;

    let log = ctx.store.read_log(&source).await?;

    let discovered: Vec<Resource> = log
        .iter()
        .filter_map(|r| match r {
            LogRecord::DiscoveryRun(d) => Some(d.resources.clone()),
            _ => None,
        })
        .next_back()
        .unwrap_or_default();

    anyhow::ensure!(
        !discovered.is_empty(),
        "no discovery run for `{source}` — run \
         `centinel youtube discover --source {source} --channel <url>` first"
    );

    // The metadata part is the marker for "this video has been fetched"; captions and
    // audio are separate addresses with their own histories.
    let seen: HashSet<String> = log
        .iter()
        .filter_map(|r| match r {
            LogRecord::Observation(o) => Some(o.resource.natural_key.clone()),
            _ => None,
        })
        .collect();

    let mut statuses = ctx.store.statuses(&source).await?;

    let mut todo: Vec<Resource> = Vec::new();
    let mut already_had = 0usize;
    for r in &discovered {
        let marker = format!("{}#{}", r.natural_key, Part::Metadata.as_str());
        if !args.refresh && seen.contains(&marker) {
            already_had += 1;
            continue;
        }
        todo.push(r.clone());
    }
    if let Some(limit) = args.limit {
        todo.truncate(limit);
    }

    let mut report_videos = Vec::new();
    let (mut stored, mut failed, mut blocked, mut bytes) = (0usize, 0usize, 0usize, 0u64);
    let mut no_captions = 0usize;
    let total = todo.len() as u64;
    let delay = std::time::Duration::from_secs_f64(args.delay_secs.max(0.0));

    for (i, resource) in todo.iter().enumerate() {
        let Some(video_id) = video_id_of(&resource.natural_key) else {
            continue;
        };
        progress.step(format!("{}/{} {video_id}", i + 1, total), i as u64, total);

        if i > 0 && !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }

        // Each video gets a scratch directory that is removed with it. yt-dlp writes
        // files, the store wants bytes, and nothing should outlive the conversion.
        let work = tempfile::tempdir()?;
        let mut outcome = VideoOutcome {
            video_id: video_id.clone(),
            title: String::new(),
            stored: Vec::new(),
            bytes: 0,
            failed: None,
            state: None,
        };

        // ---- metadata ---------------------------------------------------------------
        match yt.video_metadata(&video_id).await {
            Ok(json) => {
                outcome.title = title_of(&json).unwrap_or_default();
                let meta = youtube::observation_meta(Part::Metadata, &[("title", &outcome.title)]);
                let key = youtube::sub_resource(&video_id, Part::Metadata);
                record(ctx, &source, &key, &json, meta).await?;
                outcome.bytes += json.len() as u64;
                outcome.stored.push(Part::Metadata.as_str().to_string());
            }
            Err(f) => {
                // No Observation — liveness carries the failure instead (§4.4), and it
                // is attached to the *video*, not to a sub-resource we never reached.
                let at = Timestamp::now();
                let st = statuses
                    .entry(resource.clone())
                    .or_insert_with(|| ResourceStatus::new_live(resource.clone(), at));
                st.apply(f.state, at, Some(f.detail.clone()));
                ctx.store
                    .append(&source, &LogRecord::Status(st.clone()))
                    .await?;

                failed += 1;
                if f.state == Liveness::Blocked {
                    blocked += 1;
                }
                outcome.state = Some(f.state);
                outcome.failed = Some(f.detail);
                report_videos.push(outcome);
                continue;
            }
        }

        // ---- captions ---------------------------------------------------------------
        let mut got_captions = false;
        if !args.no_captions {
            match yt.captions(&video_id, &args.lang, work.path()).await {
                // No track in this language is a fact, not a failure. Recording nothing
                // is the honest outcome — an empty blob would claim we have captions.
                Ok(None) => {}
                Ok(Some(bytes_json)) => {
                    let meta = youtube::observation_meta(
                        Part::Captions,
                        &[("language", &args.lang), ("title", &outcome.title)],
                    );
                    let key = youtube::sub_resource(&video_id, Part::Captions);
                    record(ctx, &source, &key, &bytes_json, meta).await?;
                    outcome.bytes += bytes_json.len() as u64;
                    outcome.stored.push(Part::Captions.as_str().to_string());
                    got_captions = true;
                }
                Err(f) => outcome.failed = Some(format!("captions: {f}")),
            }
        }
        if !got_captions {
            no_captions += 1;
        }

        // ---- audio ------------------------------------------------------------------
        // A caption *fetch failure* is not the same as YouTube having no track, and only
        // the latter should trigger a download — but both leave `got_captions` false, so
        // the fallback errs toward fetching. Spending 63 MB on a video that turns out to
        // have captions is cheaper than leaving a meeting out of the index.
        if args.audio || (args.audio_if_no_captions && !got_captions) {
            match yt.audio(&video_id, work.path()).await {
                Ok(audio) => {
                    let meta = youtube::observation_meta(Part::Audio, &[("title", &outcome.title)]);
                    let key = youtube::sub_resource(&video_id, Part::Audio);
                    record(ctx, &source, &key, &audio, meta).await?;
                    outcome.bytes += audio.len() as u64;
                    outcome.stored.push(Part::Audio.as_str().to_string());
                }
                Err(f) => outcome.failed = Some(format!("audio: {f}")),
            }
        }

        // A success on any part clears whatever failure state the video was in.
        let at = Timestamp::now();
        statuses
            .entry(resource.clone())
            .and_modify(|s| s.apply(Liveness::Live, at, None))
            .or_insert_with(|| ResourceStatus::new_live(resource.clone(), at));

        stored += 1;
        bytes += outcome.bytes;
        report_videos.push(outcome);
    }

    progress.step(format!("{stored} stored, {failed} failed"), total, total);

    Ok(YoutubeReport::Fetch {
        source: args.source,
        yt_dlp_version: version,
        discovered: discovered.len(),
        already_had,
        attempted: todo.len(),
        stored,
        failed,
        bytes,
        blocked,
        no_captions,
        videos: report_videos,
    })
}

async fn record(
    ctx: &Ctx,
    source: &SourceId,
    natural_key: &str,
    bytes: &[u8],
    meta: std::collections::BTreeMap<String, String>,
) -> anyhow::Result<()> {
    let resource = Resource::new(source.clone(), natural_key);
    ctx.store
        .record_observation(&resource, bytes, Timestamp::now(), meta)
        .await?;
    Ok(())
}

/// `https://www.youtube.com/watch?v=ID` → `ID`.
fn video_id_of(url: &str) -> Option<String> {
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
}
