//! What one Source's log adds up to, written down so a list of sources costs a `stat`.
//!
//! `source list` and the web's Sources page ask one question of every log: how many
//! addresses does it know. Answered from `log/` that is a parse of the whole log — 36 MB
//! for a city of 19,000 pages — once per source on every list, and the page took 1.4 s
//! to print sixteen numbers that had not changed since yesterday. `crumbs` asked a
//! second question of every log, which hosts it had observed, and replayed them all
//! again to answer it.
//!
//! ## A replay, cached by the log's size
//!
//! A [`Summary`] is the replay's own counts, written beside the length of every month
//! file they were read from. The log is append-only, so a file's length is its version:
//! while the lengths match, the summary *is* the replay; when they do not, the log has
//! grown, and the next reader replays it and writes the summary again. Nothing else
//! writes it, so there is no second owner of the number — `centinel list` replays and
//! reports the same count the summary carries, because both are the same fold over the
//! same records.
//!
//! **Derived, and safe to delete.** It lives in `summaries/`, outside `log/`, and the
//! first read after a deletion rebuilds it. The reader rebuilds it rather than
//! [`Store::append`] maintaining it because the counts are sets — a Resource observed
//! twice is one address — and keeping a set current on every append would mean
//! rewriting it once per line, which is quadratic over a collect. Rebuilding on read
//! costs one replay per source per change, and a source changes once per run.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::domain::{Liveness, SourceId};
use crate::store::{Replay, Store};

/// One Source's log, added up.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// The length of every month file this was computed from, by file name. The summary
    /// answers for the log while these still match it.
    pub log: BTreeMap<String, u64>,
    /// Addresses the log knows — [`Replay::resources`], the count `list` reports.
    pub resources: usize,
    pub observations: usize,
    /// Addresses per liveness state, keyed by `live` / `gone` / `blocked` / `error`.
    pub liveness: BTreeMap<String, usize>,
    /// How the source was most recently enumerated; empty if never.
    pub discovery_method: String,
    /// Hosts this Source's Observations answer for, so a crumb pointing at one is already
    /// a Source. Empty for a channel: `youtube.com` holds every channel there is, so one
    /// channel's log says nothing about a crumb pointing at another
    /// ([`crate::sources::channel::claims`]).
    pub claimed_hosts: BTreeSet<String>,
}

impl Summary {
    /// Folds a replay into its summary, stamped with the log sizes it was read from.
    pub fn of(replay: &Replay, log: BTreeMap<String, u64>) -> Self {
        let observed: Vec<&str> = replay.observed().into_iter().collect();
        let claimed_hosts = match crate::sources::channel::claims(replay.discovery_method(), &observed) {
            true => BTreeSet::new(),
            false => observed
                .iter()
                .filter_map(|key| crate::crumbs::host_of(key))
                .collect(),
        };
        let mut liveness: BTreeMap<String, usize> = BTreeMap::new();
        for status in replay.statuses().values() {
            *liveness.entry(status.state.to_string()).or_default() += 1;
        }
        Self {
            log,
            resources: replay.resources().len(),
            observations: replay
                .records()
                .iter()
                .filter(|r| matches!(r, crate::store::LogRecord::Observation(_)))
                .count(),
            liveness,
            discovery_method: replay.discovery_method().to_string(),
            claimed_hosts,
        }
    }

    /// Addresses currently live.
    pub fn live(&self) -> usize {
        self.liveness
            .get(&Liveness::Live.to_string())
            .copied()
            .unwrap_or_default()
    }
}

/// Reads and rebuilds summaries. A thin thing, like [`crate::journal::Journal`]: the
/// path belongs to [`Store`], and everything here is one small read and, when the log
/// has moved, one replay.
pub struct Summaries<'a> {
    store: &'a Store,
}

impl<'a> Summaries<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// The summary for this Source's log as it is now.
    ///
    /// A `stat` of each month file and a read of one small JSON when nothing has changed;
    /// a replay and a write when something has. A summary that cannot be read — absent,
    /// or from a build that wrote a different shape — is simply one that has to be
    /// rebuilt, because that is what derived means.
    pub async fn get(&self, source: &SourceId) -> anyhow::Result<Summary> {
        let log = self.store.log_sizes(source).await?;
        if let Some(cached) = self.read(source).await
            && cached.log == log
        {
            return Ok(cached);
        }
        let summary = Summary::of(&self.store.replay(source).await?, log);
        self.write(source, &summary).await?;
        Ok(summary)
    }

    async fn read(&self, source: &SourceId) -> Option<Summary> {
        let bytes = tokio::fs::read(self.store.summary_path(source)).await.ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Write-then-rename, so two readers rebuilding at once leave one whole file.
    async fn write(&self, source: &SourceId, summary: &Summary) -> anyhow::Result<()> {
        let path = self.store.summary_path(source);
        let dir = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("a summary always has a parent directory"))?;
        tokio::fs::create_dir_all(dir).await?;
        let tmp = dir.join(format!(".{source}.tmp"));
        tokio::fs::write(&tmp, serde_json::to_vec(summary)?).await?;
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DiscoveryRun, Resource};
    use crate::store::LogRecord;
    use std::collections::BTreeMap;

    async fn store() -> (tempfile::TempDir, Store, SourceId) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        (dir, store, SourceId::new("tampa").unwrap())
    }

    async fn discover(store: &Store, id: &SourceId, addresses: &[&str]) {
        store
            .append(
                id,
                &LogRecord::DiscoveryRun(DiscoveryRun {
                    source: id.clone(),
                    at: jiff::Timestamp::now(),
                    resources: addresses.iter().map(|a| Resource::new(id.clone(), *a)).collect(),
                    method: "sitemap".into(),
                }),
            )
            .await
            .unwrap();
    }

    async fn observe(store: &Store, id: &SourceId, address: &str) {
        store
            .record_observation(
                &Resource::new(id.clone(), address),
                address.as_bytes(),
                jiff::Timestamp::now(),
                BTreeMap::new(),
            )
            .await
            .unwrap();
    }

    /// The whole point: the summary is the replay's own number, and it follows the log.
    /// An append makes it stale, and the next read says what the log says.
    #[tokio::test]
    async fn a_summary_is_the_replay_and_follows_the_log() {
        let (_d, store, id) = store().await;
        discover(&store, &id, &["https://x.gov/a", "https://x.gov/b"]).await;
        observe(&store, &id, "https://x.gov/a").await;

        let summaries = Summaries::new(&store);
        let first = summaries.get(&id).await.unwrap();
        assert_eq!((first.resources, first.observations, first.live()), (2, 1, 1));
        assert_eq!(first.claimed_hosts, BTreeSet::from(["x.gov".to_string()]));
        assert!(store.summary_path(&id).exists(), "written down for the next reader");

        // Observed again: one more observation, the same address.
        observe(&store, &id, "https://x.gov/a").await;
        observe(&store, &id, "https://x.gov/c").await;
        let second = summaries.get(&id).await.unwrap();
        assert_eq!((second.resources, second.observations), (3, 3));
        assert_eq!(second, Summary::of(&store.replay(&id).await.unwrap(), store.log_sizes(&id).await.unwrap()));
    }

    /// A summary on disk is only trusted while the log it names is the log on disk. A
    /// file from an older build, or one somebody edited, is rebuilt rather than believed.
    #[tokio::test]
    async fn a_summary_that_does_not_match_the_log_is_rebuilt_not_believed() {
        let (_d, store, id) = store().await;
        discover(&store, &id, &["https://x.gov/a"]).await;
        let summaries = Summaries::new(&store);
        summaries.get(&id).await.unwrap();

        let path = store.summary_path(&id);
        let mut forged: Summary = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        forged.resources = 999;
        std::fs::write(&path, serde_json::to_vec(&forged).unwrap()).unwrap();
        assert_eq!(summaries.get(&id).await.unwrap().resources, 999, "the sizes match, so it is trusted");

        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(summaries.get(&id).await.unwrap().resources, 1, "unreadable means rebuilt");
    }

    /// Deleting `summaries/` loses nothing: it is derived, and the next read rebuilds it.
    #[tokio::test]
    async fn a_deleted_summary_directory_is_rebuilt_on_read() {
        let (_d, store, id) = store().await;
        discover(&store, &id, &["https://x.gov/a"]).await;
        Summaries::new(&store).get(&id).await.unwrap();
        std::fs::remove_dir_all(store.summaries_dir()).unwrap();
        assert_eq!(Summaries::new(&store).get(&id).await.unwrap().resources, 1);
    }

    /// A channel claims no host: one source for a channel says nothing about a crumb
    /// pointing at another channel on the same host.
    #[tokio::test]
    async fn a_channel_claims_no_host() {
        let (_d, store, id) = store().await;
        observe(&store, &id, "https://www.youtube.com/watch?v=abc").await;
        let summary = Summaries::new(&store).get(&id).await.unwrap();
        assert!(summary.claimed_hosts.is_empty(), "{summary:?}");
    }

    #[tokio::test]
    async fn a_source_with_no_log_sums_to_nothing() {
        let (_d, store, id) = store().await;
        let summary = Summaries::new(&store).get(&id).await.unwrap();
        assert_eq!(summary, Summary::default());
    }
}
