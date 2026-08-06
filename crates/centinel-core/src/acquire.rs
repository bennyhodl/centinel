//! Walking a [`Source`] through discovery and acquisition.
//!
//! This is the machinery that used to be written twice — once for crawled sites in
//! `collect`, once for YouTube channels in `youtube fetch`. Both computed their work list
//! the same way, turned failures into [`ResourceStatus`] the same way, and kept the same
//! counters; they differed only in what "fetch one" meant, which is exactly the variation
//! [`Source`] exists to quarantine (SPEC §4.1).
//!
//! ## Resumability is a consequence, not a feature
//!
//! There is no checkpoint file. "What still needs collecting" is
//!
//! ```text
//! latest DiscoveryRun's resources  −  resources whose marker is already observed
//! ```
//!
//! Kill it at URL 4,000 and re-run; it starts at 4,001. That falls out of files-being-
//! truth (SPEC §5) rather than being engineered — and it now falls out *once*, for every
//! Source kind, rather than being re-derived by each of them.
//!
//! The **marker** is the one place resumption varies: a page is collected when the page
//! is observed, a video when its *metadata* is observed. Captions and audio are separate
//! addresses that may legitimately never exist, and keying resumption on them would
//! re-fetch a whole catalogue every run. [`Source::marker`] is that single line.

use std::collections::{BTreeMap, HashMap};

use jiff::Timestamp;

use crate::domain::{DiscoveryRun, Fingerprint, Liveness, Note, Resource, ResourceStatus, Source};
use crate::op::Progress;
use crate::store::{LogRecord, Store};

// ── discovery ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct DiscoverOpts {
    /// Enumerate without writing a DiscoveryRun to the log.
    pub dry_run: bool,
    /// How many natural keys to carry back for the report.
    pub sample: usize,
}

/// What one discovery pass did.
#[derive(Clone, Debug)]
pub struct Discovered {
    pub found: usize,
    /// Addresses no previous snapshot contained.
    ///
    /// A **set difference**, not a difference of counts. A site that swapped fifty pages
    /// for fifty others moved by fifty and its counts did not move at all, which is
    /// exactly the run someone needs told about.
    pub new: usize,
    /// The previous snapshot's size, for the delta. A large negative swing is the
    /// signature of a truncated crawl rather than a shrinking source.
    pub previous_run: Option<usize>,
    pub notes: Vec<Note>,
    pub warnings: Vec<String>,
    pub figures: BTreeMap<String, u64>,
    pub sample: Vec<String>,
    pub written_to_log: bool,
}

/// Enumerates a Source and records the snapshot.
///
/// The previous snapshot is read *before* this one is written, so the delta compares
/// against history rather than against itself.
pub async fn discover(
    store: &Store,
    source: &dyn Source,
    opts: &DiscoverOpts,
    progress: &Progress,
) -> anyhow::Result<Discovered> {
    let id = source.id().clone();
    let enumeration = source.enumerate(progress).await?;

    let previous: Option<Vec<Resource>> = store
        .replay(&id)
        .await?
        .latest_discovery()
        .map(|d| d.resources.clone());

    let previous_run = previous.as_ref().map(Vec::len);
    let known: std::collections::HashSet<&str> = previous
        .iter()
        .flatten()
        .map(|r| r.natural_key.as_str())
        .collect();
    let new = enumeration
        .resources
        .iter()
        .filter(|r| !known.contains(r.natural_key.as_str()))
        .count();

    let sample: Vec<String> = enumeration
        .resources
        .iter()
        .take(opts.sample)
        .map(|r| r.natural_key.clone())
        .collect();

    let found = enumeration.resources.len();

    let written_to_log = if opts.dry_run {
        false
    } else {
        store
            .append(
                &id,
                &LogRecord::DiscoveryRun(DiscoveryRun {
                    source: id.clone(),
                    at: Timestamp::now(),
                    resources: enumeration.resources,
                    // §4.3 records this as provenance for a suspiciously small snapshot.
                    // It is the Source's own word for what it did, not a guess made here.
                    method: source.method().to_string(),
                }),
            )
            .await?;
        true
    };

    progress.say(format!("{found} resources discovered ({new} new)"));

    Ok(Discovered {
        found,
        new,
        previous_run,
        notes: enumeration.notes,
        warnings: enumeration.warnings,
        figures: enumeration.figures,
        sample,
        written_to_log,
    })
}

// ── acquisition ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct CollectOpts {
    /// Stop after this many addresses.
    pub limit: Option<usize>,
    /// Re-acquire addresses already in the store instead of skipping them.
    pub refresh: bool,
    /// Only acquire addresses whose natural key contains one of these substrings.
    pub matches: Vec<String>,
    /// Drop any single artifact larger than this.
    pub max_bytes: u64,
    /// Failures to carry back in the report.
    pub max_failures: usize,
}

impl Default for CollectOpts {
    fn default() -> Self {
        Self {
            limit: None,
            refresh: false,
            matches: Vec::new(),
            max_bytes: 256 * 1024 * 1024,
            max_failures: 20,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Failure {
    pub natural_key: String,
    pub state: Liveness,
    pub detail: String,
}

/// What one acquisition pass did.
#[derive(Clone, Debug, Default)]
pub struct Collected {
    /// Addresses in the most recent DiscoveryRun.
    pub discovered: usize,
    /// Skipped because the store already had them.
    pub already_had: usize,
    /// Excluded by `matches`.
    pub filtered_out: usize,
    pub attempted: usize,
    /// Addresses that yielded at least one artifact.
    pub stored: usize,
    /// Stored, and something under that address differed from last time.
    pub changed: usize,
    /// Addresses that refused, or whose every artifact was dropped.
    pub failed: usize,
    /// Failures that were refusals rather than absence. A non-zero count with zero
    /// successes is a wall, and reads nothing like an empty source unless it is counted.
    pub blocked: usize,
    pub bytes: u64,
    /// Still unacquired. Non-zero means re-running continues where this stopped.
    pub remaining: usize,
    /// What was gathered, by content kind — the input to planning extraction.
    pub by_kind: BTreeMap<String, usize>,
    /// What was gathered, by artifact. A crawled page yields one `document`; a video
    /// yields `metadata`, `captions.json3` and sometimes `audio`. A gap between them is
    /// how "this video has no captions" becomes visible without `collect` knowing what a
    /// caption is.
    pub parts: BTreeMap<String, usize>,
    pub failures: Vec<Failure>,
    pub failures_truncated: Option<usize>,
    /// What the Source itself wanted said about the result.
    pub remarks: Vec<Note>,
}

/// The `parts` key for an address that holds exactly one artifact.
const WHOLE: &str = "document";

/// Acquires everything the latest DiscoveryRun found and the store lacks.
///
/// Errors from one address never stop the pass: a refusal is a fact about that address,
/// recorded as liveness, and the remaining thousand are unaffected. Only a failure to
/// read the log at all aborts.
pub async fn collect(
    store: &Store,
    source: &dyn Source,
    opts: &CollectOpts,
    progress: &Progress,
) -> anyhow::Result<Collected> {
    let id = source.id().clone();

    // One pass over the log for the work list, the resume state and the change baseline.
    let replay = store.replay(&id).await?;

    let discovered: Vec<Resource> = replay
        .latest_discovery()
        .map(|d| d.resources.clone())
        .unwrap_or_default();

    anyhow::ensure!(
        !discovered.is_empty(),
        "no discovery run for `{id}` — run `centinel discover --source {id}` first"
    );

    let mut seen: HashMap<Resource, Fingerprint> = HashMap::new();
    let mut statuses: BTreeMap<Resource, ResourceStatus> = BTreeMap::new();
    for rec in replay.records() {
        match rec {
            LogRecord::Observation(o) => {
                seen.insert(o.resource.clone(), o.fingerprint.clone());
            }
            LogRecord::Status(s) => {
                statuses.insert(s.resource.clone(), s.clone());
            }
            _ => {}
        }
    }

    // ---- the work list -------------------------------------------------------------
    let mut report = Collected {
        discovered: discovered.len(),
        ..Default::default()
    };
    let mut todo: Vec<Resource> = Vec::new();

    for r in &discovered {
        if !opts.matches.is_empty() && !opts.matches.iter().any(|m| r.natural_key.contains(m)) {
            report.filtered_out += 1;
            continue;
        }
        if !opts.refresh && seen.contains_key(&source.marker(r)) {
            report.already_had += 1;
            continue;
        }
        todo.push(r.clone());
    }

    let total_todo = todo.len();
    if let Some(limit) = opts.limit {
        todo.truncate(limit);
    }

    // ---- acquire -------------------------------------------------------------------
    let total = todo.len() as u64;
    for (i, resource) in todo.iter().enumerate() {
        // Every resource, not every twenty-fifth. The throttle was harmless when the bar
        // was the only output; beside a request log that moves on every fetch it made the
        // bar visibly disagree with the tally under it — 25/500 sitting still while the
        // line beneath counted past a hundred requests. `indicatif` rate-limits its own
        // redraws, so the cost of an event the renderer discards is a channel send.
        progress.step(
            format!("{} stored, {} failed", report.stored, report.failed),
            i as u64,
            total,
        );

        let at = Timestamp::now();
        report.attempted += 1;

        match source.acquire(resource, progress).await {
            Ok(artifacts) => {
                let mut stored_here = 0usize;
                let mut changed_here = false;

                for artifact in artifacts {
                    let bytes = &artifact.fetched.bytes;
                    if bytes.len() as u64 > opts.max_bytes {
                        push_failure(
                            &mut report,
                            opts.max_failures,
                            Failure {
                                natural_key: artifact.resource.natural_key.clone(),
                                state: Liveness::Error,
                                detail: format!(
                                    "artifact is {} MB, over the {} MB ceiling",
                                    bytes.len() / (1024 * 1024),
                                    opts.max_bytes / (1024 * 1024),
                                ),
                            },
                        );
                        continue;
                    }

                    let kind = crate::fetch::content_kind(&artifact.fetched.meta, bytes);
                    *report.by_kind.entry(kind.to_string()).or_default() += 1;
                    *report
                        .parts
                        .entry(part_of(resource, &artifact.resource))
                        .or_default() += 1;
                    report.bytes += bytes.len() as u64;

                    let obs = store
                        .record_observation(
                            &artifact.resource,
                            bytes,
                            at,
                            artifact.fetched.meta.clone(),
                        )
                        .await?;

                    // Against the preloaded map, not a fresh log scan per address.
                    if seen.get(&artifact.resource) != Some(&obs.fingerprint) {
                        changed_here = true;
                    }
                    seen.insert(artifact.resource.clone(), obs.fingerprint);
                    stored_here += 1;
                }

                if stored_here == 0 {
                    // Every artifact was dropped, or the Source had nothing to store.
                    // Not a refusal, so liveness is left alone — but it is not a
                    // collection either, and counting it as one would overstate the run.
                    report.failed += 1;
                    continue;
                }

                report.stored += 1;
                if changed_here {
                    report.changed += 1;
                }

                // A success clears whatever failure state the address was in.
                if let Some(st) = statuses.get_mut(resource)
                    && st.state != Liveness::Live
                {
                    st.apply(Liveness::Live, at, None);
                    store.append(&id, &LogRecord::Status(st.clone())).await?;
                }
            }

            Err(refusal) => {
                // No Observation — an Observation always has bytes, so liveness carries
                // the failure instead (§4.4).
                let st = statuses
                    .entry(resource.clone())
                    .or_insert_with(|| ResourceStatus::new_live(resource.clone(), at));
                st.apply(refusal.state, at, Some(refusal.detail.clone()));
                store.append(&id, &LogRecord::Status(st.clone())).await?;

                report.failed += 1;
                if refusal.state == Liveness::Blocked {
                    report.blocked += 1;
                }
                push_failure(
                    &mut report,
                    opts.max_failures,
                    Failure {
                        natural_key: resource.natural_key.clone(),
                        state: refusal.state,
                        detail: refusal.detail,
                    },
                );
            }
        }
    }

    report.remaining = total_todo.saturating_sub(report.stored);
    report.remarks = source.remarks(&report.parts, report.attempted);

    progress.step(
        format!("{} stored, {} failed", report.stored, report.failed),
        total,
        total,
    );
    Ok(report)
}

/// Names the artifact an acquired address is, relative to the address it came from.
///
/// `…/watch?v=ID` + `…/watch?v=ID#captions.json3` → `captions.json3`. An address that
/// holds one artifact is [`WHOLE`], so the breakdown reads as a table in both cases
/// rather than as a bare number for one kind and a table for the other.
fn part_of(parent: &Resource, acquired: &Resource) -> String {
    if acquired.natural_key == parent.natural_key {
        return WHOLE.to_string();
    }
    acquired
        .natural_key
        .strip_prefix(parent.natural_key.as_str())
        .map(|rest| rest.trim_start_matches('#').to_string())
        .filter(|rest| !rest.is_empty())
        .unwrap_or_else(|| WHOLE.to_string())
}

/// Keeps the failure list bounded. A wholesale block would otherwise produce thousands of
/// identical lines and bury the count that matters.
fn push_failure(report: &mut Collected, max: usize, failure: Failure) {
    if report.failures.len() < max {
        report.failures.push(failure);
    } else {
        *report.failures_truncated.get_or_insert(0) += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Acquired, Enumeration, Fetched, Refusal, SourceId, SourceKind};
    use futures::future::BoxFuture;
    use std::sync::Mutex;

    /// What acquiring one address does: yields these `(part suffix, bytes)` pairs, or
    /// refuses.
    type Outcome = Result<Vec<(String, Vec<u8>)>, Refusal>;

    /// A Source whose behaviour is a script, so the loop can be tested without a network.
    ///
    /// This is the point of the seam being real: every property below — resumption,
    /// liveness on refusal, the change baseline, multi-artifact addresses — used to be
    /// reachable only by standing up HTTP or `yt-dlp`, which is why none of them had a
    /// test.
    struct Scripted {
        id: SourceId,
        resources: Vec<Resource>,
        /// natural key → what acquiring it does.
        script: HashMap<String, Outcome>,
        marker_part: Option<String>,
        calls: Mutex<Vec<String>>,
    }

    impl Scripted {
        fn new(id: &str, keys: &[&str]) -> Self {
            let id = SourceId::new(id).unwrap();
            Self {
                resources: keys.iter().map(|k| Resource::new(id.clone(), *k)).collect(),
                id,
                script: HashMap::new(),
                marker_part: None,
                calls: Mutex::new(Vec::new()),
            }
        }

        /// One artifact at the address itself.
        fn yields(mut self, key: &str, body: &str) -> Self {
            self.script.insert(
                key.to_string(),
                Ok(vec![(String::new(), body.as_bytes().to_vec())]),
            );
            self
        }

        /// Several artifacts at sub-addresses of the same address.
        fn yields_parts(mut self, key: &str, parts: &[(&str, &str)]) -> Self {
            self.script.insert(
                key.to_string(),
                Ok(parts
                    .iter()
                    .map(|(p, b)| (format!("#{p}"), b.as_bytes().to_vec()))
                    .collect()),
            );
            self
        }

        fn refuses(mut self, key: &str, state: Liveness, detail: &str) -> Self {
            self.script.insert(
                key.to_string(),
                Err(Refusal {
                    state,
                    detail: detail.to_string(),
                }),
            );
            self
        }

        fn marked_by(mut self, part: &str) -> Self {
            self.marker_part = Some(format!("#{part}"));
            self
        }

        fn acquired(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Source for Scripted {
        fn id(&self) -> &SourceId {
            &self.id
        }
        fn kind(&self) -> SourceKind {
            SourceKind::Site
        }
        fn method(&self) -> &'static str {
            "scripted"
        }
        fn target(&self) -> &str {
            "https://example.gov"
        }

        fn enumerate<'a>(&'a self, _p: &'a Progress) -> BoxFuture<'a, anyhow::Result<Enumeration>> {
            Box::pin(async move {
                Ok(Enumeration {
                    resources: self.resources.clone(),
                    notes: vec![Note::new("scripted", "no network was touched")],
                    figures: BTreeMap::from([("scripted".to_string(), 1)]),
                    ..Default::default()
                })
            })
        }

        fn acquire<'a>(
            &'a self,
            resource: &'a Resource,
            _p: &'a Progress,
        ) -> BoxFuture<'a, Result<Vec<Acquired>, Refusal>> {
            Box::pin(async move {
                self.calls
                    .lock()
                    .unwrap()
                    .push(resource.natural_key.clone());
                match self.script.get(&resource.natural_key) {
                    Some(Ok(parts)) => Ok(parts
                        .iter()
                        .map(|(suffix, bytes)| Acquired {
                            resource: Resource::new(
                                resource.source.clone(),
                                format!("{}{suffix}", resource.natural_key),
                            ),
                            fetched: Fetched {
                                bytes: bytes.clone(),
                                meta: BTreeMap::new(),
                            },
                        })
                        .collect()),
                    Some(Err(r)) => Err(r.clone()),
                    None => Ok(Vec::new()),
                }
            })
        }

        fn marker(&self, resource: &Resource) -> Resource {
            match &self.marker_part {
                Some(part) => Resource::new(
                    resource.source.clone(),
                    format!("{}{part}", resource.natural_key),
                ),
                None => resource.clone(),
            }
        }

        fn remarks(&self, parts: &BTreeMap<String, usize>, attempted: usize) -> Vec<Note> {
            let captioned = parts.get("captions").copied().unwrap_or(0);
            if attempted > captioned {
                vec![Note::new(
                    "gap",
                    format!("{} uncaptioned", attempted - captioned),
                )]
            } else {
                Vec::new()
            }
        }
    }

    async fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path()).await.unwrap();
        (dir, s)
    }

    #[tokio::test]
    async fn discovery_writes_a_snapshot_under_the_sources_own_method() {
        let (_d, store) = store().await;
        let src = Scripted::new("x", &["https://x.gov/a", "https://x.gov/b"]);

        let out = discover(
            &store,
            &src,
            &DiscoverOpts {
                sample: 5,
                ..Default::default()
            },
            &Progress::none(),
        )
        .await
        .unwrap();

        assert_eq!(out.found, 2);
        assert_eq!(out.new, 2, "everything is new the first time");
        assert_eq!(out.previous_run, None, "nothing preceded this one");
        assert!(out.written_to_log);
        assert_eq!(out.sample.len(), 2);
        assert_eq!(out.notes[0].label, "scripted");

        // The method on the record is the Source's own word, not the caller's guess.
        let replay = store.replay(src.id()).await.unwrap();
        assert_eq!(replay.discovery_method(), "scripted");
    }

    #[tokio::test]
    async fn a_second_discovery_reports_the_delta_against_the_first() {
        let (_d, store) = store().await;
        let first = Scripted::new("x", &["https://x.gov/a", "https://x.gov/b"]);
        discover(&store, &first, &DiscoverOpts::default(), &Progress::none())
            .await
            .unwrap();

        let second = Scripted::new("x", &["https://x.gov/a"]);
        let out = discover(&store, &second, &DiscoverOpts::default(), &Progress::none())
            .await
            .unwrap();

        assert_eq!(out.found, 1);
        assert_eq!(out.new, 0);
        assert_eq!(
            out.previous_run,
            Some(2),
            "a shrinking snapshot is the signature of a truncated pass"
        );
    }

    /// The count that a difference of counts cannot see: a source that swapped every
    /// address for a different one moved entirely, and both snapshots hold two.
    #[tokio::test]
    async fn churn_is_visible_even_when_the_count_does_not_move() {
        let (_d, store) = store().await;
        let before = Scripted::new("x", &["https://x.gov/a", "https://x.gov/b"]);
        discover(&store, &before, &DiscoverOpts::default(), &Progress::none())
            .await
            .unwrap();

        let after = Scripted::new("x", &["https://x.gov/c", "https://x.gov/d"]);
        let out = discover(&store, &after, &DiscoverOpts::default(), &Progress::none())
            .await
            .unwrap();

        assert_eq!(out.found, 2);
        assert_eq!(out.previous_run, Some(2), "the counts are identical");
        assert_eq!(out.new, 2, "and yet nothing is the same");
    }

    #[tokio::test]
    async fn a_dry_run_enumerates_and_writes_nothing() {
        let (_d, store) = store().await;
        let src = Scripted::new("x", &["https://x.gov/a"]);
        let out = discover(
            &store,
            &src,
            &DiscoverOpts {
                dry_run: true,
                sample: 1,
            },
            &Progress::none(),
        )
        .await
        .unwrap();

        assert_eq!(out.found, 1);
        assert!(!out.written_to_log);
        assert!(store.read_log(src.id()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn collecting_without_a_discovery_run_says_what_to_do() {
        let (_d, store) = store().await;
        let src = Scripted::new("x", &["https://x.gov/a"]);
        let err = collect(&store, &src, &CollectOpts::default(), &Progress::none())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("centinel discover"), "{err}");
    }

    /// The property the whole append-only design was bought for, now testable without a
    /// network: a second pass acquires nothing.
    #[tokio::test]
    async fn a_second_collection_acquires_nothing() {
        let (_d, store) = store().await;
        let src = Scripted::new("x", &["https://x.gov/a", "https://x.gov/b"])
            .yields("https://x.gov/a", "alpha")
            .yields("https://x.gov/b", "beta");

        discover(&store, &src, &DiscoverOpts::default(), &Progress::none())
            .await
            .unwrap();

        let first = collect(&store, &src, &CollectOpts::default(), &Progress::none())
            .await
            .unwrap();
        assert_eq!(first.stored, 2);
        assert_eq!(first.changed, 2, "first sight of an address is a change");
        assert_eq!(first.already_had, 0);
        assert_eq!(src.acquired().len(), 2);

        let again = collect(&store, &src, &CollectOpts::default(), &Progress::none())
            .await
            .unwrap();
        assert_eq!(again.stored, 0);
        assert_eq!(again.already_had, 2);
        assert_eq!(
            src.acquired().len(),
            2,
            "the second pass must not touch the network"
        );
    }

    #[tokio::test]
    async fn refresh_reacquires_and_reports_unchanged_content_as_unchanged() {
        let (_d, store) = store().await;
        let src = Scripted::new("x", &["https://x.gov/a"]).yields("https://x.gov/a", "alpha");
        discover(&store, &src, &DiscoverOpts::default(), &Progress::none())
            .await
            .unwrap();
        collect(&store, &src, &CollectOpts::default(), &Progress::none())
            .await
            .unwrap();

        let out = collect(
            &store,
            &src,
            &CollectOpts {
                refresh: true,
                ..Default::default()
            },
            &Progress::none(),
        )
        .await
        .unwrap();

        assert_eq!(out.stored, 1, "refresh re-acquires");
        assert_eq!(out.changed, 0, "the bytes were identical");
    }

    /// A refusal is a fact about one address, recorded as liveness, and it must not stop
    /// the addresses after it.
    #[tokio::test]
    async fn a_refusal_becomes_liveness_and_the_pass_continues() {
        let (_d, store) = store().await;
        let src = Scripted::new(
            "x",
            &["https://x.gov/a", "https://x.gov/b", "https://x.gov/c"],
        )
        .refuses("https://x.gov/a", Liveness::Blocked, "HTTP 403")
        .yields("https://x.gov/b", "beta")
        .refuses("https://x.gov/c", Liveness::Gone, "HTTP 404");

        discover(&store, &src, &DiscoverOpts::default(), &Progress::none())
            .await
            .unwrap();
        let out = collect(&store, &src, &CollectOpts::default(), &Progress::none())
            .await
            .unwrap();

        assert_eq!(out.attempted, 3);
        assert_eq!(out.stored, 1);
        assert_eq!(out.failed, 2);
        assert_eq!(out.blocked, 1, "a 403 is blocked, a 404 is not");

        let statuses = store.statuses(src.id()).await.unwrap();
        let state = |k: &str| {
            statuses
                .get(&Resource::new(src.id().clone(), k))
                .map(|s| s.state)
        };
        assert_eq!(state("https://x.gov/a"), Some(Liveness::Blocked));
        assert_eq!(state("https://x.gov/c"), Some(Liveness::Gone));
        assert_eq!(
            state("https://x.gov/b"),
            Some(Liveness::Live),
            "an Observation is itself evidence of liveness"
        );
    }

    /// A blocked address that later succeeds must stop reading as blocked.
    #[tokio::test]
    async fn a_later_success_clears_a_previous_refusal() {
        let (_d, store) = store().await;
        let blocked = Scripted::new("x", &["https://x.gov/a"]).refuses(
            "https://x.gov/a",
            Liveness::Blocked,
            "HTTP 403",
        );
        discover(
            &store,
            &blocked,
            &DiscoverOpts::default(),
            &Progress::none(),
        )
        .await
        .unwrap();
        collect(&store, &blocked, &CollectOpts::default(), &Progress::none())
            .await
            .unwrap();

        let ok = Scripted::new("x", &["https://x.gov/a"]).yields("https://x.gov/a", "alpha");
        collect(&store, &ok, &CollectOpts::default(), &Progress::none())
            .await
            .unwrap();

        let statuses = store.statuses(ok.id()).await.unwrap();
        assert_eq!(
            statuses[&Resource::new(ok.id().clone(), "https://x.gov/a")].state,
            Liveness::Live
        );
    }

    /// One address, three artifacts, each with its own Observation history — the shape
    /// no `fetch(&Resource) -> Fetched` could express.
    #[tokio::test]
    async fn one_address_can_hold_several_artifacts() {
        let (_d, store) = store().await;
        let key = "https://youtube.test/watch?v=abc";
        let src = Scripted::new("c", &[key])
            .yields_parts(
                key,
                &[
                    ("metadata", r#"{"title":"Council"}"#),
                    ("captions", "1\nhello"),
                    ("audio", "OggS pretend this is a stream"),
                ],
            )
            .marked_by("metadata");

        discover(&store, &src, &DiscoverOpts::default(), &Progress::none())
            .await
            .unwrap();
        let out = collect(&store, &src, &CollectOpts::default(), &Progress::none())
            .await
            .unwrap();

        assert_eq!(out.stored, 1, "one address");
        assert_eq!(out.parts["metadata"], 1);
        assert_eq!(out.parts["captions"], 1);
        assert_eq!(out.parts["audio"], 1);

        // Each artifact is its own address with its own history (§4.2).
        let log = store.read_log(src.id()).await.unwrap();
        let observed: Vec<String> = log
            .iter()
            .filter_map(|r| match r {
                LogRecord::Observation(o) => Some(o.resource.natural_key.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(observed.len(), 3, "{observed:?}");
        assert!(observed.iter().all(|k| k.starts_with(key)));
    }

    /// Resumption keys on the marker, not on every artifact — otherwise a video whose
    /// captions never existed would be re-fetched forever.
    #[tokio::test]
    async fn resumption_keys_on_the_marker_artifact() {
        let (_d, store) = store().await;
        let key = "https://youtube.test/watch?v=abc";
        // Metadata comes back; captions never do.
        let src = Scripted::new("c", &[key])
            .yields_parts(key, &[("metadata", r#"{"title":"Council"}"#)])
            .marked_by("metadata");

        discover(&store, &src, &DiscoverOpts::default(), &Progress::none())
            .await
            .unwrap();
        collect(&store, &src, &CollectOpts::default(), &Progress::none())
            .await
            .unwrap();
        let again = collect(&store, &src, &CollectOpts::default(), &Progress::none())
            .await
            .unwrap();

        assert_eq!(again.already_had, 1);
        assert_eq!(src.acquired().len(), 1, "the video was acquired once");
    }

    #[tokio::test]
    async fn the_source_gets_the_last_word_on_its_own_result() {
        let (_d, store) = store().await;
        let key = "https://youtube.test/watch?v=abc";
        let src = Scripted::new("c", &[key])
            .yields_parts(key, &[("metadata", "{}")])
            .marked_by("metadata");
        discover(&store, &src, &DiscoverOpts::default(), &Progress::none())
            .await
            .unwrap();

        let out = collect(&store, &src, &CollectOpts::default(), &Progress::none())
            .await
            .unwrap();
        assert_eq!(out.remarks[0].detail, "1 uncaptioned");
    }

    #[tokio::test]
    async fn a_limit_leaves_the_rest_for_the_next_pass() {
        let (_d, store) = store().await;
        let src = Scripted::new(
            "x",
            &["https://x.gov/a", "https://x.gov/b", "https://x.gov/c"],
        )
        .yields("https://x.gov/a", "a")
        .yields("https://x.gov/b", "b")
        .yields("https://x.gov/c", "c");
        discover(&store, &src, &DiscoverOpts::default(), &Progress::none())
            .await
            .unwrap();

        let out = collect(
            &store,
            &src,
            &CollectOpts {
                limit: Some(1),
                ..Default::default()
            },
            &Progress::none(),
        )
        .await
        .unwrap();
        assert_eq!(out.stored, 1);
        assert_eq!(out.remaining, 2, "re-running continues from here");
    }

    #[tokio::test]
    async fn matches_filter_the_work_list_without_touching_the_source() {
        let (_d, store) = store().await;
        let src = Scripted::new("x", &["https://x.gov/a.pdf", "https://x.gov/b.html"])
            .yields("https://x.gov/a.pdf", "%PDF-1.7")
            .yields("https://x.gov/b.html", "<html>");
        discover(&store, &src, &DiscoverOpts::default(), &Progress::none())
            .await
            .unwrap();

        let out = collect(
            &store,
            &src,
            &CollectOpts {
                matches: vec![".pdf".into()],
                ..Default::default()
            },
            &Progress::none(),
        )
        .await
        .unwrap();

        assert_eq!(out.filtered_out, 1);
        assert_eq!(out.stored, 1);
        assert_eq!(src.acquired(), ["https://x.gov/a.pdf"]);
        assert_eq!(
            out.by_kind["pdf"], 1,
            "content kind is sniffed, not declared"
        );
    }

    #[tokio::test]
    async fn an_oversized_artifact_is_dropped_with_a_reason() {
        let (_d, store) = store().await;
        let src = Scripted::new("x", &["https://x.gov/big"])
            .yields("https://x.gov/big", &"x".repeat(4096));
        discover(&store, &src, &DiscoverOpts::default(), &Progress::none())
            .await
            .unwrap();

        let out = collect(
            &store,
            &src,
            &CollectOpts {
                max_bytes: 1024,
                ..Default::default()
            },
            &Progress::none(),
        )
        .await
        .unwrap();

        assert_eq!(out.stored, 0);
        assert_eq!(out.failed, 1);
        assert!(
            out.failures[0].detail.contains("over the"),
            "{:?}",
            out.failures
        );
    }

    #[tokio::test]
    async fn the_failure_list_is_bounded_and_says_how_much_it_dropped() {
        let (_d, store) = store().await;
        let keys: Vec<String> = (0..10).map(|i| format!("https://x.gov/{i}")).collect();
        let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        let mut src = Scripted::new("x", &refs);
        for k in &keys {
            src = src.refuses(k, Liveness::Blocked, "HTTP 403");
        }
        discover(&store, &src, &DiscoverOpts::default(), &Progress::none())
            .await
            .unwrap();

        let out = collect(
            &store,
            &src,
            &CollectOpts {
                max_failures: 3,
                ..Default::default()
            },
            &Progress::none(),
        )
        .await
        .unwrap();

        assert_eq!(out.failed, 10);
        assert_eq!(out.failures.len(), 3);
        assert_eq!(out.failures_truncated, Some(7));
    }

    #[test]
    fn an_artifact_is_named_relative_to_the_address_it_came_from() {
        let id = SourceId::new("x").unwrap();
        let parent = Resource::new(id.clone(), "https://y/watch?v=abc");
        let whole = Resource::new(id.clone(), "https://y/watch?v=abc");
        let part = Resource::new(id, "https://y/watch?v=abc#captions.json3");

        assert_eq!(part_of(&parent, &whole), WHOLE);
        assert_eq!(part_of(&parent, &part), "captions.json3");
    }
}
