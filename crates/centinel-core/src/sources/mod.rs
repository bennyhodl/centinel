//! Building a [`Source`] from what is known about it.
//!
//! This module is the **only** place that decides which acquisition a source gets. Every
//! other caller — `run`, `discover`, `collect`, `source list`, `source adopt` — asks for a
//! `Box<dyn Source>` and then talks to the trait.
//!
//! That is the whole point of the seam. Before, the site/channel distinction was made
//! again at nine sites: four arms of a `match (stage, acquisition)` in `run`, three
//! copies of `Acquisition → (kind, target)`, two spellings of the kind's label, plus a
//! substring test on URLs. Adding SPEC §4.1's third kind meant finding all of them.
//! Adding it now means one file next to [`site`] and [`channel`], one variant on
//! [`Acquisition`], and one arm below.
//!
//! ## Intent and evidence are different questions
//!
//! [`from_config`] answers "what did the operator declare"; [`infer`] answers "what has
//! the store been collecting". They can disagree — a source crawled by hand is evidence
//! with no intent behind it — and the config is not the place to look for the second.

pub mod channel;
pub mod site;

use crate::config::{Acquisition, Defaults, SourceConfig};
use crate::discovery::DiscoveryLimits;
use crate::domain::{Source, SourceId, SourceKind};
use crate::policy::HostPolicy;
use crate::store::{LogRecord, Replay, Store};

pub use channel::{AudioPolicy, ChannelSource};
pub use site::SiteSource;

/// Per-invocation overrides for a Source built from config.
///
/// Every field is optional and loses to nothing: the config block wins over
/// `[defaults]`, and an explicit flag wins over both. Present so a person can try
/// something once without editing the file they will run from cron.
#[derive(Clone, Debug, Default)]
pub struct Overrides {
    pub rps: Option<f64>,
    pub lang: Option<String>,
    pub audio: Option<AudioPolicy>,
    pub yt_dlp_args: Vec<String>,
    pub limits: Option<DiscoveryLimits>,
    /// Stop enumerating after this many addresses. A URL ceiling for a crawled site, a
    /// `--playlist-end` for a channel — the same instruction in each Source's idiom.
    pub limit: Option<usize>,
}

/// Builds the Source a `[[source]]` block declares.
///
/// The one match on [`Acquisition`] outside the config module itself.
pub fn from_config(
    cfg: &SourceConfig,
    defaults: &Defaults,
    over: &Overrides,
) -> anyhow::Result<Box<dyn Source>> {
    let id = SourceId::new(cfg.id.clone())?;

    match cfg.acquisition()? {
        Acquisition::Site(url) => {
            let policy = HostPolicy {
                max_requests_per_second: over.rps.or(cfg.rps).unwrap_or(defaults.rps),
                ..Default::default()
            };
            let mut limits = over.limits.clone().unwrap_or_default();
            if let Some(n) = over.limit {
                limits.max_urls = limits.max_urls.min(n);
            }
            // A named strategy resolves here. An unnamed one cannot: recognition needs a
            // seed, and this function never fetches. `None` therefore means "ask the
            // registry once the page is in hand", which `SiteSource::enumerate` does.
            let named = cfg
                .strategy
                .as_deref()
                .map(crate::strategies::crawl::by_name)
                .transpose()?;
            Ok(Box::new(
                SiteSource::new(id, url, policy, limits)?.with_strategy(named),
            ))
        }

        Acquisition::Channel(url) => {
            let mut args = cfg.yt_dlp_args.clone();
            args.extend(over.yt_dlp_args.iter().cloned());

            let lang = over
                .lang
                .clone()
                .or_else(|| cfg.lang.clone())
                .unwrap_or_else(|| defaults.lang.clone());

            // Defaults on, for channels, everywhere. Videos YouTube never captioned are
            // ~7% of a real council channel, unpredictable from metadata, and permanently
            // missing from search without audio — so the default is the one that leaves
            // no silent holes.
            let audio =
                over.audio
                    .unwrap_or_else(|| match cfg.audio_if_no_captions.unwrap_or(true) {
                        true => AudioPolicy::IfNoCaptions,
                        false => AudioPolicy::Never,
                    });

            Ok(Box::new(
                ChannelSource::new(id, url, args)
                    .with_lang(lang)
                    .with_audio(audio)
                    .with_limit(over.limit),
            ))
        }
    }
}

/// What the store already knows about a source.
///
/// Everything here is read back out of `log/<source>/` — no network, no guessing from the
/// id. A source that was collected has necessarily recorded how it was reached, so
/// re-deriving its config block is reading, not inventing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inferred {
    pub kind: SourceKind,
    /// `None` when the store proves the source exists but cannot say where from.
    pub target: Option<String>,
    /// The `DiscoveryRun::method` last recorded — the strategy that collected this.
    ///
    /// `None` when this build does not have a strategy by that name, which is what an
    /// older store or a strategy since removed looks like. Not an error: the source is
    /// still collectable, it simply is not pinned to something that no longer exists.
    pub strategy: Option<&'static crate::strategies::crawl::StrategyDef>,
}

impl Inferred {
    /// The `[[source]]` block this would be written as.
    pub fn to_config(&self, id: &SourceId) -> Option<SourceConfig> {
        let target = self.target.as_ref()?;
        Some(match self.kind {
            SourceKind::Site => SourceConfig {
                // What the store says collected it last, so writing the block down does
                // not quietly change how the next run enumerates.
                strategy: self.strategy.map(|s| s.name.to_string()),
                ..SourceConfig::site(id.to_string(), target)
            },
            SourceKind::Channel => SourceConfig::channel(id.to_string(), target),
        })
    }
}

/// Reconstructs a source's kind and address from its log, or `None` if the log is empty.
///
/// The discriminator is the `DiscoveryRun::method` recorded as provenance (§4.3); each
/// adapter owns the question of whether a method is its own. The natural-key fallback
/// covers a source collected with `ingest`, which writes Observations and never a
/// DiscoveryRun.
pub async fn infer(store: &Store, id: &SourceId) -> anyhow::Result<Option<Inferred>> {
    infer_from(store, &store.replay(id).await?).await
}

/// [`infer`], against a log the caller has already read.
///
/// For a caller that also wants the resource count, which is another view of the same
/// pass. Asking for both used to read the log twice.
pub async fn infer_from(store: &Store, replay: &Replay) -> anyhow::Result<Option<Inferred>> {
    if replay.is_empty() {
        return Ok(None);
    }

    // Natural keys, newest last — a site's origin and a channel's video ids both come
    // from here.
    let keys: Vec<&str> = replay
        .records()
        .iter()
        .filter_map(|r| match r {
            LogRecord::Observation(o) => Some(o.resource.natural_key.as_str()),
            LogRecord::DiscoveryRun(d) => d.resources.first().map(|r| r.natural_key.as_str()),
            _ => None,
        })
        .collect();

    if channel::claims(replay.discovery_method(), &keys) {
        return Ok(Some(Inferred {
            kind: SourceKind::Channel,
            target: channel_url(store, replay.records()).await,
            strategy: None,
        }));
    }

    Ok(Some(Inferred {
        kind: SourceKind::Site,
        target: keys.iter().find_map(|k| origin_of(k)),
        // The same `method` string that names the kind also names the strategy. A method
        // this build has no strategy for is simply not pinned — see [`Inferred::strategy`].
        strategy: crate::strategies::crawl::by_name(replay.discovery_method()).ok(),
    }))
}

/// Builds the Source for an id the config does not name, using what the store remembers.
///
/// The path that lets `centinel collect --source hillsborough` work for something
/// collected by hand — the config is a statement of intent, and its absence is not
/// evidence that the source does not exist.
pub async fn from_store(
    store: &Store,
    id: &SourceId,
    defaults: &Defaults,
    over: &Overrides,
) -> anyhow::Result<Box<dyn Source>> {
    let inferred = infer(store, id).await?.ok_or_else(|| {
        anyhow::anyhow!(
            "nothing in the config or the store names `{id}` — add it with \
             `centinel source add {id} --site <url>` or `--channel <url>`"
        )
    })?;

    // The address is only needed to *enumerate*. Acquisition works from the addresses the
    // DiscoveryRun already holds, so an unrecoverable target is not fatal here.
    let target = inferred.target.unwrap_or_default();
    let cfg = match inferred.kind {
        SourceKind::Site => SourceConfig {
            // The store is the authority here. A source discovered with `listing` and
            // then collected in a second process must acquire as `listing` too, or
            // acquisition would scan 6 GB of CSV for enclosed documents.
            strategy: inferred.strategy.map(|s| s.name.to_string()),
            ..SourceConfig::site(id.to_string(), target)
        },
        SourceKind::Channel => SourceConfig::channel(id.to_string(), target),
    };
    from_config(&cfg, defaults, over)
}

/// The channel a stored recording came from.
///
/// Not in the log: a `DiscoveryRun` records the videos, not the channel they were listed
/// from. It *is* in the `yt-dlp -J` document archived beside each video, so this reads one
/// of those blobs back. That is the whole argument for keeping originals (§5.4) — the
/// metadata was retained without knowing this question would be asked.
async fn channel_url(store: &Store, log: &[LogRecord]) -> Option<String> {
    let metadata_part = format!("#{}", crate::youtube::Part::Metadata.as_str());
    let sha = log.iter().rev().find_map(|r| match r {
        LogRecord::Observation(o) if o.resource.natural_key.ends_with(&metadata_part) => {
            Some(o.blob_sha.clone())
        }
        _ => None,
    })?;

    let bytes = store.get_blob(&sha).await.ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;

    // `channel_url` is the canonical `/channel/UC…`; `uploader_url` is the `@handle`
    // form, which is what a person would have typed and reads better in the config.
    for key in ["uploader_url", "channel_url"] {
        if let Some(url) = json.get(key).and_then(|v| v.as_str())
            && !url.is_empty()
        {
            return Some(url.to_string());
        }
    }
    json.get("channel_id")
        .and_then(|v| v.as_str())
        .map(|id| format!("https://www.youtube.com/channel/{id}"))
}

/// `https://www.tampa.gov/some/page?x=1#frag` → `https://www.tampa.gov`.
pub fn origin_of(natural_key: &str) -> Option<String> {
    let url = url::Url::parse(natural_key).ok()?;
    let origin = url.origin().ascii_serialization();
    // Opaque origins (`data:`, `file:`) serialize to this and are not a site.
    (origin != "null").then_some(origin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DiscoveryRun, Resource};

    fn defaults() -> Defaults {
        Defaults::default()
    }

    #[test]
    fn a_site_block_builds_a_site() {
        let cfg = SourceConfig::site("tampa", "https://www.tampa.gov");
        let s = from_config(&cfg, &defaults(), &Overrides::default()).unwrap();
        assert_eq!(s.kind(), SourceKind::Site);
        assert_eq!(s.method(), "sitemap");
        assert_eq!(s.target(), "https://www.tampa.gov");
    }

    #[test]
    fn a_channel_block_builds_a_channel_that_fetches_audio_by_default() {
        let cfg = SourceConfig::channel("council", "https://www.youtube.com/@X");
        let s = from_config(&cfg, &defaults(), &Overrides::default()).unwrap();
        assert_eq!(s.kind(), SourceKind::Channel);
        assert_eq!(s.method(), "playlist");
        assert!(
            s.yields_audio(),
            "an uncaptioned meeting is invisible to search without it"
        );
    }

    #[test]
    fn a_block_can_turn_the_audio_fallback_off() {
        let cfg = SourceConfig {
            audio_if_no_captions: Some(false),
            ..SourceConfig::channel("council", "https://www.youtube.com/@X")
        };
        let s = from_config(&cfg, &defaults(), &Overrides::default()).unwrap();
        assert!(!s.yields_audio());
    }

    /// An explicit flag beats the block, which beats `[defaults]`.
    #[test]
    fn an_override_wins_over_the_block() {
        let cfg = SourceConfig {
            audio_if_no_captions: Some(false),
            ..SourceConfig::channel("council", "https://www.youtube.com/@X")
        };
        let s = from_config(
            &cfg,
            &defaults(),
            &Overrides {
                audio: Some(AudioPolicy::Always),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(s.yields_audio());
    }

    #[test]
    fn a_block_naming_neither_target_is_refused_here_too() {
        let cfg = SourceConfig {
            id: "x".into(),
            ..Default::default()
        };
        let err = from_config(&cfg, &defaults(), &Overrides::default())
            .map(|_| ())
            .unwrap_err()
            .to_string();
        assert!(err.contains("neither"), "{err}");
    }

    #[test]
    fn an_origin_is_taken_from_any_natural_key() {
        assert_eq!(
            origin_of("https://www.tampa.gov/some/page?x=1#frag").as_deref(),
            Some("https://www.tampa.gov")
        );
        assert_eq!(
            origin_of("http://example.gov:8080/a").as_deref(),
            Some("http://example.gov:8080")
        );
        assert_eq!(origin_of("not a url"), None);
        assert_eq!(origin_of("data:text/plain,hi"), None);
    }

    // ── inference ──────────────────────────────────────────────────────────────

    async fn store_with_a_crawled_site(dir: &std::path::Path) -> Store {
        let store = Store::open(dir.join("store")).await.unwrap();
        let id = SourceId::new("hillsborough").unwrap();
        store
            .append(
                &id,
                &LogRecord::DiscoveryRun(DiscoveryRun {
                    source: id.clone(),
                    at: jiff::Timestamp::now(),
                    resources: vec![Resource::new(
                        id.clone(),
                        "https://www.hillsboroughcounty.org/en/residents",
                    )],
                    method: "sitemap".into(),
                }),
            )
            .await
            .unwrap();
        store
    }

    #[tokio::test]
    async fn a_crawled_site_is_recovered_from_its_discovery_run() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_a_crawled_site(dir.path()).await;

        let got = infer(&store, &SourceId::new("hillsborough").unwrap())
            .await
            .unwrap()
            .expect("the store holds this source");
        assert_eq!(got.kind, SourceKind::Site);
        assert_eq!(
            got.target.as_deref(),
            Some("https://www.hillsboroughcounty.org")
        );
    }

    /// The channel URL is not in the log — it is in the archived `yt-dlp -J` document,
    /// which is exactly the "keep the original" argument paying off.
    #[tokio::test]
    async fn a_channel_is_recovered_from_the_archived_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).await.unwrap();
        let id = SourceId::new("tampa-council").unwrap();

        store
            .append(
                &id,
                &LogRecord::DiscoveryRun(DiscoveryRun {
                    source: id.clone(),
                    at: jiff::Timestamp::now(),
                    resources: vec![Resource::new(
                        id.clone(),
                        "https://www.youtube.com/watch?v=abc123",
                    )],
                    method: "playlist".into(),
                }),
            )
            .await
            .unwrap();

        let metadata = serde_json::json!({
            "title": "Council Meeting",
            "channel_id": "UCLzohJmEgvfJOEd4YJNIHbg",
            "channel_url": "https://www.youtube.com/channel/UCLzohJmEgvfJOEd4YJNIHbg",
            "uploader_url": "https://www.youtube.com/@CityofTampa",
        });
        let key = crate::youtube::sub_resource("abc123", crate::youtube::Part::Metadata);
        store
            .record_observation(
                &Resource::new(id.clone(), &key),
                metadata.to_string().as_bytes(),
                jiff::Timestamp::now(),
                Default::default(),
            )
            .await
            .unwrap();

        let got = infer(&store, &id).await.unwrap().unwrap();
        assert_eq!(got.kind, SourceKind::Channel);
        // The handle form, not the /channel/UC… one — it is what a person would type.
        assert_eq!(
            got.target.as_deref(),
            Some("https://www.youtube.com/@CityofTampa")
        );
    }

    #[tokio::test]
    async fn a_source_the_store_has_never_heard_of_infers_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_a_crawled_site(dir.path()).await;
        assert!(
            infer(&store, &SourceId::new("nobody").unwrap())
                .await
                .unwrap()
                .is_none()
        );
    }

    /// The path that makes a hand-collected source runnable without a config block.
    #[tokio::test]
    async fn a_source_can_be_built_from_the_store_alone() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_a_crawled_site(dir.path()).await;

        let s = from_store(
            &store,
            &SourceId::new("hillsborough").unwrap(),
            &defaults(),
            &Overrides::default(),
        )
        .await
        .unwrap();
        assert_eq!(s.kind(), SourceKind::Site);
        assert_eq!(s.target(), "https://www.hillsboroughcounty.org");
    }

    #[tokio::test]
    async fn an_unknown_source_says_how_to_add_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_a_crawled_site(dir.path()).await;
        let err = from_store(
            &store,
            &SourceId::new("nobody").unwrap(),
            &defaults(),
            &Overrides::default(),
        )
        .await
        .map(|_| ())
        .unwrap_err()
        .to_string();
        assert!(err.contains("centinel source add"), "{err}");
    }

    #[test]
    fn an_inferred_source_writes_the_block_its_kind_calls_for() {
        let id = SourceId::new("council").unwrap();
        let channel = Inferred {
            kind: SourceKind::Channel,
            target: Some("https://www.youtube.com/@X".into()),
            strategy: None,
        };
        assert_eq!(
            channel.to_config(&id).unwrap().channel.as_deref(),
            Some("https://www.youtube.com/@X")
        );

        let unknown = Inferred {
            kind: SourceKind::Site,
            target: None,
            strategy: None,
        };
        assert!(
            unknown.to_config(&id).is_none(),
            "a block with no address would fail on the next run"
        );
    }

    /// A source discovered with `listing` must be *acquired* as `listing` in a later
    /// process, or acquisition would scan 6 GB of CSV looking for enclosed documents.
    #[tokio::test]
    async fn the_strategy_that_collected_a_source_is_recovered_from_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).await.unwrap();
        let id = SourceId::new("publicrec").unwrap();
        store
            .append(
                &id,
                &LogRecord::DiscoveryRun(DiscoveryRun {
                    source: id.clone(),
                    at: jiff::Timestamp::now(),
                    resources: vec![Resource::new(
                        id.clone(),
                        "https://publicrec.hillsclerk.com/Civil/a.csv",
                    )],
                    method: "listing".into(),
                }),
            )
            .await
            .unwrap();

        let got = infer(&store, &id).await.unwrap().unwrap();
        assert_eq!(got.kind, SourceKind::Site);
        assert_eq!(got.strategy.map(|s| s.name), Some("listing"));
        assert_eq!(
            got.to_config(&id).unwrap().strategy.as_deref(),
            Some("listing")
        );

        let s = from_store(&store, &id, &defaults(), &Overrides::default())
            .await
            .unwrap();
        assert_eq!(s.method(), "listing", "and it acquires as what it was");
    }

    /// A method this build has no strategy for is not an error. An older store, or a
    /// strategy since removed, still names a source that is perfectly collectable.
    #[tokio::test]
    async fn a_method_this_build_does_not_know_leaves_the_source_unpinned() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).await.unwrap();
        let id = SourceId::new("old").unwrap();
        store
            .append(
                &id,
                &LogRecord::DiscoveryRun(DiscoveryRun {
                    source: id.clone(),
                    at: jiff::Timestamp::now(),
                    resources: vec![Resource::new(id.clone(), "https://a.gov/x")],
                    method: "some-retired-strategy".into(),
                }),
            )
            .await
            .unwrap();

        let got = infer(&store, &id).await.unwrap().unwrap();
        assert!(got.strategy.is_none());
        assert!(got.to_config(&id).unwrap().strategy.is_none());
    }
}
