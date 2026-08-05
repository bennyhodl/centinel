//! The on-disk store, per SPEC §5.
//!
//! ```text
//! <root>/
//!   blobs/ab/cd/abcd1234…      TRUTH    immutable, content-addressed, pooled across Sources
//!   log/<source>/YYYY-MM.jsonl TRUTH    append-only
//!   current/<source>/…         DERIVED  URL-mirroring tree. Regenerable.
//!   cache/embeddings/          DURABLE  Tier A
//!   centinel.db                DERIVED  SQLite: metadata + FTS5
//!   index/                     DERIVED  LanceDB
//! ```
//!
//! Only `blobs/` and `log/` are truth. Everything else is rebuildable from them, which
//! is what makes the index disposable and the corpus `rsync`-able (§5.4).
//!
//! Blobs are **pooled across Sources** — the same PDF on two `.gov` sites stores once.
//! Logs and trees are **per-Source**, so a single city's corpus stays separable.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::domain::{
    BlobSha, Derivation, DiscoveryRun, Fingerprint, Liveness, Observation, Resource,
    ResourceStatus, SourceId,
};

/// One line of a `log/<source>/YYYY-MM.jsonl` file.
///
/// Internally tagged, so the on-disk shape matches SPEC §5 literally:
/// `{"type":"observation","resource":{…},"blob_sha":"…","fingerprint":"…","at":"…"}`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LogRecord {
    Observation(Observation),
    DiscoveryRun(DiscoveryRun),
    Status(ResourceStatus),
    Derivation(Derivation),
}

impl LogRecord {
    pub fn at(&self) -> Timestamp {
        match self {
            Self::Observation(o) => o.at,
            Self::DiscoveryRun(d) => d.at,
            Self::Status(s) => s.last_checked,
            Self::Derivation(d) => d.at,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("blob {0} not found in the pool")]
    BlobNotFound(BlobSha),

    #[error(
        "blob {sha} is corrupt: bytes on disk hash to {actual}. \
         The pool is content-addressed, so this means the file was modified in place."
    )]
    BlobCorrupt { sha: BlobSha, actual: BlobSha },

    #[error("malformed log line at {path}:{line}: {source}")]
    MalformedLogLine {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

type Result<T> = std::result::Result<T, StoreError>;

fn io_at(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> StoreError {
    let path = path.into();
    move |source| StoreError::Io { path, source }
}

/// A handle on a Centinel store root.
///
/// Cheap to clone — it is just a path.
#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Opens (and creates, if absent) a store at `root`.
    pub async fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        for sub in ["blobs", "log", "current", "cache/embeddings", "index"] {
            let p = root.join(sub);
            tokio::fs::create_dir_all(&p).await.map_err(io_at(&p))?;
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    // ---- blobs: the content-addressed pool -------------------------------------------

    /// `blobs/ab/cd/abcd1234…` — two levels of fan-out keep directory sizes sane at
    /// corpus scale (GovScape indexed 10M PDFs; a flat directory would not survive it).
    fn blob_path(&self, sha: &BlobSha) -> PathBuf {
        let s = sha.as_str();
        self.root
            .join("blobs")
            .join(&s[0..2])
            .join(&s[2..4])
            .join(s)
    }

    /// Writes bytes into the pool and returns their address.
    ///
    /// Idempotent: an existing blob with the same hash is left untouched rather than
    /// rewritten, because blobs are immutable by construction.
    pub async fn put_blob(&self, bytes: &[u8]) -> Result<BlobSha> {
        let sha = BlobSha::from_bytes(bytes);
        let dest = self.blob_path(&sha);

        if tokio::fs::try_exists(&dest).await.map_err(io_at(&dest))? {
            return Ok(sha);
        }

        let dir = dest.parent().expect("blob path always has a parent");
        tokio::fs::create_dir_all(dir).await.map_err(io_at(dir))?;

        // Write-then-rename: a torn write must never be visible at a content address.
        let tmp = dir.join(format!(".{}.tmp", sha.as_str()));
        tokio::fs::write(&tmp, bytes).await.map_err(io_at(&tmp))?;
        tokio::fs::rename(&tmp, &dest).await.map_err(io_at(&dest))?;

        Ok(sha)
    }

    /// The pool path for a blob, whether or not it exists.
    ///
    /// Exposed so `current/` can hardlink into the pool rather than copying.
    pub fn blob_path_of(&self, sha: &BlobSha) -> PathBuf {
        self.blob_path(sha)
    }

    pub async fn has_blob(&self, sha: &BlobSha) -> Result<bool> {
        let p = self.blob_path(sha);
        tokio::fs::try_exists(&p).await.map_err(io_at(&p))
    }

    /// Reads a blob, **verifying** the bytes still hash to their address.
    ///
    /// The check is not paranoia: this is an evidentiary archive, and silent bit-rot
    /// or an in-place edit would undermine every claim built on it.
    pub async fn get_blob(&self, sha: &BlobSha) -> Result<Vec<u8>> {
        let p = self.blob_path(sha);
        let bytes = match tokio::fs::read(&p).await {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(StoreError::BlobNotFound(sha.clone()));
            }
            Err(e) => return Err(StoreError::Io { path: p, source: e }),
        };

        let actual = BlobSha::from_bytes(&bytes);
        if &actual != sha {
            return Err(StoreError::BlobCorrupt {
                sha: sha.clone(),
                actual,
            });
        }
        Ok(bytes)
    }

    // ---- log: append-only truth ------------------------------------------------------

    /// Monthly partitioning keeps any single file small enough to rewrite or inspect by
    /// hand, and makes "what happened in March" a file read rather than a scan.
    fn log_path(&self, source: &SourceId, at: Timestamp) -> PathBuf {
        let zoned = at.to_zoned(TimeZone::UTC);
        let month = format!("{:04}-{:02}", zoned.year(), zoned.month());
        self.root
            .join("log")
            .join(source.as_str())
            .join(format!("{month}.jsonl"))
    }

    /// Appends one record. The file is opened, written, flushed and closed per call —
    /// slower than holding a handle, but it means a crash cannot lose buffered truth.
    pub async fn append(&self, source: &SourceId, record: &LogRecord) -> Result<()> {
        let path = self.log_path(source, record.at());
        let dir = path.parent().expect("log path always has a parent");
        tokio::fs::create_dir_all(dir).await.map_err(io_at(dir))?;

        let mut line = serde_json::to_vec(record).expect("LogRecord is always serializable");
        line.push(b'\n');

        let mut f = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .map_err(io_at(&path))?;
        f.write_all(&line).await.map_err(io_at(&path))?;
        f.flush().await.map_err(io_at(&path))?;
        Ok(())
    }

    /// Reads every record for a Source, in month order.
    ///
    /// Returns an empty vec for an unknown Source rather than erroring — "no log yet"
    /// is an ordinary state, not a fault.
    pub async fn read_log(&self, source: &SourceId) -> Result<Vec<LogRecord>> {
        let dir = self.root.join("log").join(source.as_str());
        let mut months: Vec<PathBuf> = Vec::new();

        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(StoreError::Io {
                    path: dir,
                    source: e,
                });
            }
        };
        while let Some(entry) = entries.next_entry().await.map_err(io_at(&dir))? {
            let p = entry.path();
            if p.extension().is_some_and(|x| x == "jsonl") {
                months.push(p);
            }
        }
        months.sort();

        let mut out = Vec::new();
        for path in months {
            let text = tokio::fs::read_to_string(&path)
                .await
                .map_err(io_at(&path))?;
            for (i, line) in text.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let rec =
                    serde_json::from_str(line).map_err(|source| StoreError::MalformedLogLine {
                        path: path.clone(),
                        line: i + 1,
                        source,
                    })?;
                out.push(rec);
            }
        }
        Ok(out)
    }

    /// Lists every Source that has a log directory.
    pub async fn sources(&self) -> Result<Vec<SourceId>> {
        let dir = self.root.join("log");
        let mut out = Vec::new();
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => {
                return Err(StoreError::Io {
                    path: dir,
                    source: e,
                });
            }
        };
        while let Some(entry) = entries.next_entry().await.map_err(io_at(&dir))? {
            if let Some(name) = entry.file_name().to_str()
                && let Ok(id) = SourceId::new(name)
            {
                out.push(id);
            }
        }
        out.sort();
        Ok(out)
    }

    // ---- derived views ---------------------------------------------------------------

    /// Replays the log into current per-Resource liveness (§4.4).
    ///
    /// Derived, never stored as truth — which is why a corrupted view costs a replay
    /// rather than an investigation.
    pub async fn statuses(&self, source: &SourceId) -> Result<BTreeMap<Resource, ResourceStatus>> {
        let mut map: BTreeMap<Resource, ResourceStatus> = BTreeMap::new();

        for rec in self.read_log(source).await? {
            match rec {
                // A successful Observation implies Live, even with no Status record.
                LogRecord::Observation(o) => {
                    map.entry(o.resource.clone())
                        .and_modify(|s| s.apply(Liveness::Live, o.at, None))
                        .or_insert_with(|| ResourceStatus::new_live(o.resource.clone(), o.at));
                }
                LogRecord::Status(s) => {
                    map.insert(s.resource.clone(), s);
                }
                LogRecord::DiscoveryRun(_) | LogRecord::Derivation(_) => {}
            }
        }
        Ok(map)
    }

    /// The most recent Observation per Resource, by log order.
    pub async fn latest_observations(
        &self,
        source: &SourceId,
    ) -> Result<BTreeMap<Resource, Observation>> {
        let mut map: BTreeMap<Resource, Observation> = BTreeMap::new();
        for rec in self.read_log(source).await? {
            if let LogRecord::Observation(o) = rec {
                match map.get(&o.resource) {
                    Some(prev) if prev.at > o.at => {}
                    _ => {
                        map.insert(o.resource.clone(), o);
                    }
                }
            }
        }
        Ok(map)
    }

    /// The newest Derivation taking `from` as its input, or `None`.
    ///
    /// "The extracted text of this document" is the question `read` and `open` both open
    /// with, and both used to answer it by filtering the log inline — same filter, same
    /// `next_back`, two copies. Newest wins because a re-extraction with a better tool
    /// supersedes an older one, and the Derivation carries the tool and version that say
    /// which is which (§4.6).
    pub async fn latest_derivation(
        &self,
        source: &SourceId,
        from: &BlobSha,
    ) -> Result<Option<Derivation>> {
        Ok(self
            .read_log(source)
            .await?
            .into_iter()
            .filter_map(|r| match r {
                LogRecord::Derivation(d) if &d.from_sha == from => Some(d),
                _ => None,
            })
            .next_back())
    }

    /// Every derived blob this source holds, mapped back to the blob it came from.
    ///
    /// The reverse of [`Self::latest_derivation`], and the reason a hash printed by
    /// `open --derived` can be typed back in. A derived blob is not an Observation — no
    /// server ever served it — so nothing that looked only at Observations could resolve
    /// one, and every transcript hash Centinel printed was a dead end.
    pub async fn derived_from(&self, source: &SourceId) -> Result<BTreeMap<BlobSha, BlobSha>> {
        let mut map = BTreeMap::new();
        for rec in self.read_log(source).await? {
            if let LogRecord::Derivation(d) = rec {
                map.insert(d.to_sha, d.from_sha);
            }
        }
        Ok(map)
    }

    /// The full Observation history of one Resource, oldest first.
    pub async fn history(&self, resource: &Resource) -> Result<Vec<Observation>> {
        let mut out: Vec<Observation> = self
            .read_log(&resource.source)
            .await?
            .into_iter()
            .filter_map(|r| match r {
                LogRecord::Observation(o) if &o.resource == resource => Some(o),
                _ => None,
            })
            .collect();
        out.sort_by_key(|o| o.at);
        Ok(out)
    }

    /// Records an Observation: blob into the pool, line into the log.
    ///
    /// Convenience wrapper that looks up the previous fingerprint first. **Scans the
    /// whole log**, so it is fine for a handful of URLs and quadratic for a corpus —
    /// bulk callers should preload with [`Self::latest_observations`] and use
    /// [`Self::record_observation`] instead.
    pub async fn observe(
        &self,
        resource: &Resource,
        bytes: &[u8],
        at: Timestamp,
        meta: BTreeMap<String, String>,
    ) -> Result<(Observation, Option<Fingerprint>)> {
        let previous = self
            .history(resource)
            .await?
            .last()
            .map(|o| o.fingerprint.clone());
        let obs = self.record_observation(resource, bytes, at, meta).await?;
        Ok((obs, previous))
    }

    /// Records an Observation without consulting history.
    ///
    /// The bulk path. Collecting 11,476 URLs through [`Self::observe`] would read the
    /// log 11,476 times; preloading with [`Self::latest_observations`] and calling this
    /// reads it once. Comparing fingerprints is then the caller's job — which it can do
    /// from the preloaded map.
    pub async fn record_observation(
        &self,
        resource: &Resource,
        bytes: &[u8],
        at: Timestamp,
        meta: BTreeMap<String, String>,
    ) -> Result<Observation> {
        let blob_sha = self.put_blob(bytes).await?;
        let fingerprint =
            Fingerprint::from_normalized(&crate::domain::normalize_placeholder(bytes));

        let obs = Observation {
            resource: resource.clone(),
            blob_sha,
            fingerprint,
            at,
            meta,
        };
        self.append(&resource.source, &LogRecord::Observation(obs.clone()))
            .await?;
        Ok(obs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Liveness;

    async fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path()).await.unwrap();
        (dir, s)
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[tokio::test]
    async fn blob_round_trips_and_dedupes() {
        let (_d, s) = store().await;

        let a = s.put_blob(b"council agenda").await.unwrap();
        let b = s.put_blob(b"council agenda").await.unwrap();
        assert_eq!(a, b, "identical bytes must land at one address");

        assert_eq!(s.get_blob(&a).await.unwrap(), b"council agenda");
        assert!(s.has_blob(&a).await.unwrap());
    }

    #[tokio::test]
    async fn get_blob_detects_tampering() {
        let (_d, s) = store().await;
        let sha = s.put_blob(b"original").await.unwrap();

        // Simulate an in-place edit, which content-addressing must never accept silently.
        tokio::fs::write(s.blob_path(&sha), b"tampered")
            .await
            .unwrap();

        assert!(matches!(
            s.get_blob(&sha).await,
            Err(StoreError::BlobCorrupt { .. })
        ));
    }

    #[tokio::test]
    async fn missing_blob_is_a_distinct_error() {
        let (_d, s) = store().await;
        let ghost = BlobSha::from_bytes(b"never stored");
        assert!(matches!(
            s.get_blob(&ghost).await,
            Err(StoreError::BlobNotFound(_))
        ));
    }

    #[tokio::test]
    async fn log_is_append_only_and_month_partitioned() {
        let (_d, s) = store().await;
        let src = SourceId::new("hillsboroughcounty").unwrap();
        let r = Resource::new(src.clone(), "https://x/1");

        s.observe(&r, b"v1", ts("2026-01-15T00:00:00Z"), BTreeMap::new())
            .await
            .unwrap();
        s.observe(&r, b"v2", ts("2026-02-15T00:00:00Z"), BTreeMap::new())
            .await
            .unwrap();

        assert!(
            s.root()
                .join("log/hillsboroughcounty/2026-01.jsonl")
                .exists()
        );
        assert!(
            s.root()
                .join("log/hillsboroughcounty/2026-02.jsonl")
                .exists()
        );
        assert_eq!(s.read_log(&src).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn observe_reports_previous_fingerprint_for_change_detection() {
        let (_d, s) = store().await;
        let src = SourceId::new("x").unwrap();
        let r = Resource::new(src, "https://x/1");

        let (_, prev) = s
            .observe(&r, b"hello", ts("2026-01-01T00:00:00Z"), BTreeMap::new())
            .await
            .unwrap();
        assert!(prev.is_none(), "first observation has no predecessor");

        // Whitespace-only change: new blob, same fingerprint → not a ChangeEvent (§5.3).
        let (obs2, prev2) = s
            .observe(&r, b"hello  ", ts("2026-01-02T00:00:00Z"), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(prev2.as_ref(), Some(&obs2.fingerprint));

        let (obs3, prev3) = s
            .observe(&r, b"goodbye", ts("2026-01-03T00:00:00Z"), BTreeMap::new())
            .await
            .unwrap();
        assert_ne!(prev3.as_ref(), Some(&obs3.fingerprint));
    }

    #[tokio::test]
    async fn status_replay_survives_blocked_then_recovered() {
        let (_d, s) = store().await;
        let src = SourceId::new("phila").unwrap();
        let r = Resource::new(src.clone(), "https://phila.gov/x");

        s.observe(&r, b"page", ts("2026-01-01T00:00:00Z"), BTreeMap::new())
            .await
            .unwrap();

        let mut blocked = ResourceStatus::new_live(r.clone(), ts("2026-01-01T00:00:00Z"));
        blocked.apply(
            Liveness::Blocked,
            ts("2026-01-02T00:00:00Z"),
            Some("403 CloudFront".into()),
        );
        s.append(&src, &LogRecord::Status(blocked)).await.unwrap();

        let st = s.statuses(&src).await.unwrap();
        assert_eq!(st[&r].state, Liveness::Blocked);
        assert_eq!(st[&r].consecutive_failures, 1);

        // A later success must clear the block.
        s.observe(&r, b"page", ts("2026-01-03T00:00:00Z"), BTreeMap::new())
            .await
            .unwrap();
        let st = s.statuses(&src).await.unwrap();
        assert_eq!(st[&r].state, Liveness::Live);
        assert_eq!(st[&r].consecutive_failures, 0);
    }

    #[tokio::test]
    async fn unknown_source_reads_empty_rather_than_erroring() {
        let (_d, s) = store().await;
        let src = SourceId::new("never-seen").unwrap();
        assert!(s.read_log(&src).await.unwrap().is_empty());
        assert!(s.statuses(&src).await.unwrap().is_empty());
        assert!(s.sources().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn blobs_pool_across_sources_but_logs_stay_separate() {
        let (_d, s) = store().await;
        let a = SourceId::new("tampa").unwrap();
        let b = SourceId::new("hillsborough").unwrap();
        let ra = Resource::new(a.clone(), "https://tampa.gov/doc.pdf");
        let rb = Resource::new(b.clone(), "https://hcfl.gov/doc.pdf");

        let (oa, _) = s
            .observe(
                &ra,
                b"same pdf",
                ts("2026-01-01T00:00:00Z"),
                BTreeMap::new(),
            )
            .await
            .unwrap();
        let (ob, _) = s
            .observe(
                &rb,
                b"same pdf",
                ts("2026-01-01T00:00:00Z"),
                BTreeMap::new(),
            )
            .await
            .unwrap();

        assert_eq!(oa.blob_sha, ob.blob_sha, "one PDF, one blob");
        assert_eq!(s.read_log(&a).await.unwrap().len(), 1);
        assert_eq!(s.read_log(&b).await.unwrap().len(), 1);
        assert_eq!(s.sources().await.unwrap(), vec![b, a]);
    }
}
