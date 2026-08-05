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
//!
//! ## The layout is named here and nowhere else
//!
//! The tree above used to be a description of what callers happened to agree on. Six
//! other files joined their own paths onto the root — `centinel.db` in three of them,
//! only one of which checked whether the file existed — so a change to this layout would
//! have been a silent divergence rather than a compile error.
//!
//! ## The read side is one pass, and says so
//!
//! Everything derived from a Source's log — liveness, the latest Observation per
//! Resource, what has been derived from what — comes out of [`Replay`], which reads and
//! parses the log **once**. The convenience methods on [`Store`] are each one pass of
//! their own, which is fine for one question and wrong for three.
//!
//! Three was the normal case. `resolve` walked every log twice per source, `extract`,
//! `list` and `transcribe` twice each, and no call site could see that it was paying for
//! more than one: reading a single page out of a five-source store cost eleven full log
//! reads before anything opened.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::domain::{
    BlobSha, Derivation, DiscoveryRun, Fingerprint, Liveness, Observation, Resource,
    ResourceStatus, SourceId, Underivable,
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
    /// A derivation that was attempted and produced nothing. Kept because the alternative
    /// is attempting it again on every run for the life of the corpus.
    Underivable(Underivable),
}

impl LogRecord {
    pub fn at(&self) -> Timestamp {
        match self {
            Self::Observation(o) => o.at,
            Self::DiscoveryRun(d) => d.at,
            Self::Status(s) => s.last_checked,
            Self::Derivation(d) => d.at,
            Self::Underivable(u) => u.at,
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

    #[error("no index at {} — run `centinel index` first", path.display())]
    NoIndex { path: PathBuf },

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

type Result<T> = std::result::Result<T, StoreError>;

/// The SQLite file, named once.
pub const INDEX_FILE: &str = "centinel.db";

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

    /// A handle on a store that already exists, creating nothing.
    ///
    /// For readers, and synchronous — [`Self::open`] creates the tree and so has to be
    /// awaited, which put the layout out of reach of anything not already in an async
    /// context. That is one of the reasons callers went around this module and spelled
    /// the paths out themselves.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    // ---- the layout ------------------------------------------------------------------
    //
    // Every path under the root is named here and nowhere else. It was named in six other
    // files, which meant the tree in this module's header was a description of what
    // callers happened to agree on rather than a thing anything enforced — and only one
    // of the three callers that opened `centinel.db` checked whether it existed.

    /// `blobs/` — the content-addressed pool.
    pub fn blobs_dir(&self) -> PathBuf {
        self.root.join("blobs")
    }

    /// `current/` — the URL-mirroring tree. Derived, and safe to delete.
    pub fn current_dir(&self) -> PathBuf {
        self.root.join("current")
    }

    /// `cache/embeddings/` — durable vectors, keyed by chunk hash.
    pub fn vector_cache_dir(&self) -> PathBuf {
        self.root.join("cache").join("embeddings")
    }

    /// `centinel.db` — the SQLite metadata and FTS5 index. Derived, and rebuildable.
    pub fn index_path(&self) -> PathBuf {
        self.root.join(INDEX_FILE)
    }

    /// The index path, or an error naming the command that builds one.
    ///
    /// For readers. `index` itself wants [`Self::index_path`], because creating the file
    /// is its job.
    pub fn require_index(&self) -> Result<PathBuf> {
        let path = self.index_path();
        if !path.exists() {
            return Err(StoreError::NoIndex { path });
        }
        Ok(path)
    }

    /// Counts blobs in the pool.
    ///
    /// Knows that a `.<sha>.tmp` file is a write in flight rather than a blob, because
    /// [`Self::put_blob`] is what creates them. `doctor` used to re-derive that rule from
    /// the other side of the module, which meant a change to the write convention would
    /// have silently mis-counted rather than failed.
    pub async fn count_blobs(&self) -> Result<u64> {
        let blobs = self.blobs_dir();
        let mut count = 0u64;

        let mut lvl1 = match tokio::fs::read_dir(&blobs).await {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => {
                return Err(StoreError::Io {
                    path: blobs,
                    source: e,
                });
            }
        };
        while let Some(a) = lvl1.next_entry().await.map_err(io_at(&blobs))? {
            if !a.file_type().await.map_err(io_at(a.path()))?.is_dir() {
                continue;
            }
            let mut lvl2 = tokio::fs::read_dir(a.path())
                .await
                .map_err(io_at(a.path()))?;
            while let Some(b) = lvl2.next_entry().await.map_err(io_at(a.path()))? {
                if !b.file_type().await.map_err(io_at(b.path()))?.is_dir() {
                    continue;
                }
                let mut lvl3 = tokio::fs::read_dir(b.path())
                    .await
                    .map_err(io_at(b.path()))?;
                while let Some(f) = lvl3.next_entry().await.map_err(io_at(b.path()))? {
                    if f.file_type().await.map_err(io_at(f.path()))?.is_file()
                        && !f.file_name().to_string_lossy().starts_with('.')
                    {
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
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

    /// Reads at most `limit` bytes from the front of a blob, **without** verifying it.
    ///
    /// For classification, which needs a few hundred bytes and reads every blob in the
    /// corpus to build a work list. [`Self::get_blob`] cannot serve that: it reads the
    /// whole file and hashes it, so asking "is this audio?" about a store of PDFs meant
    /// reading and hashing every PDF — gigabytes of work to answer a question the first
    /// four kilobytes settle.
    ///
    /// The absent verification is the trade and it is stated here rather than assumed: a
    /// partial read cannot be checked against a whole-file digest. Anything that will be
    /// *shown to a person* or written back into the record must go through `get_blob`.
    pub async fn blob_head(&self, sha: &BlobSha, limit: usize) -> Result<Vec<u8>> {
        use tokio::io::AsyncReadExt;

        let p = self.blob_path(sha);
        let mut file = match tokio::fs::File::open(&p).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(StoreError::BlobNotFound(sha.clone()));
            }
            Err(e) => return Err(StoreError::Io { path: p, source: e }),
        };

        let mut buf = vec![0u8; limit];
        let mut read = 0usize;
        while read < limit {
            let n = file.read(&mut buf[read..]).await.map_err(io_at(&p))?;
            if n == 0 {
                break;
            }
            read += n;
        }
        buf.truncate(read);
        Ok(buf)
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

    /// Replays a Source's whole log into memory, once.
    ///
    /// **The read side of this module.** Every question below — what is live, what was
    /// last observed, what has been derived from what — is answered from one pass, and
    /// the convenience methods that follow are each one pass of their own.
    ///
    /// That distinction is the whole point. Answering three questions about a source used
    /// to mean reading and parsing its log three times, and no call site could see that
    /// it was doing so: `resolve` walked every log twice per source, `extract`, `list`
    /// and `transcribe` twice each. Reading a single page out of a five-source store cost
    /// eleven full log reads.
    pub async fn replay(&self, source: &SourceId) -> Result<Replay> {
        Ok(Replay {
            source: source.clone(),
            records: self.read_log(source).await?,
        })
    }

    /// Replays a Source's liveness. One pass — see [`Self::replay`] to ask more than one
    /// question for the price of one.
    pub async fn statuses(&self, source: &SourceId) -> Result<BTreeMap<Resource, ResourceStatus>> {
        Ok(self.replay(source).await?.statuses())
    }

    /// The most recent Observation per Resource. One pass.
    pub async fn latest_observations(
        &self,
        source: &SourceId,
    ) -> Result<BTreeMap<Resource, Observation>> {
        Ok(self.replay(source).await?.latest_observations())
    }

    /// The newest Derivation taking `from` as its input. One pass.
    pub async fn latest_derivation(
        &self,
        source: &SourceId,
        from: &BlobSha,
    ) -> Result<Option<Derivation>> {
        Ok(self.replay(source).await?.latest_derivation(from).cloned())
    }

    /// The full Observation history of one Resource, oldest first. One pass.
    pub async fn history(&self, resource: &Resource) -> Result<Vec<Observation>> {
        Ok(self
            .replay(&resource.source)
            .await?
            .history(resource)
            .into_iter()
            .cloned()
            .collect())
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

/// One Source's log, read once and answerable many times.
///
/// Holds the records; every method here is an in-memory scan over them, which is free
/// beside the disk read and the JSON parse that produced them. Nothing is cached or
/// invalidated, because a `Replay` is a **snapshot**: it answers what the log said when
/// it was read, and a caller that needs to see an append it just made takes a new one.
#[derive(Clone, Debug)]
pub struct Replay {
    source: SourceId,
    records: Vec<LogRecord>,
}

impl Replay {
    pub fn source(&self) -> &SourceId {
        &self.source
    }

    /// Every record, in log order. The escape hatch for a question this type does not
    /// yet answer — and the signal that it should learn to.
    pub fn records(&self) -> &[LogRecord] {
        &self.records
    }

    /// True when this Source has no log at all. Distinct from "collected nothing".
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Current per-Resource liveness (§4.4).
    ///
    /// Derived, never stored as truth — which is why a corrupted view costs a replay
    /// rather than an investigation.
    pub fn statuses(&self) -> BTreeMap<Resource, ResourceStatus> {
        let mut map: BTreeMap<Resource, ResourceStatus> = BTreeMap::new();
        for rec in &self.records {
            match rec {
                // A successful Observation implies Live, even with no Status record.
                LogRecord::Observation(o) => {
                    map.entry(o.resource.clone())
                        .and_modify(|s| s.apply(Liveness::Live, o.at, None))
                        .or_insert_with(|| ResourceStatus::new_live(o.resource.clone(), o.at));
                }
                LogRecord::Status(s) => {
                    map.insert(s.resource.clone(), s.clone());
                }
                LogRecord::DiscoveryRun(_)
                | LogRecord::Derivation(_)
                | LogRecord::Underivable(_) => {}
            }
        }
        map
    }

    /// The most recent Observation per Resource, by timestamp then log order.
    pub fn latest_observations(&self) -> BTreeMap<Resource, Observation> {
        let mut map: BTreeMap<Resource, Observation> = BTreeMap::new();
        for rec in &self.records {
            if let LogRecord::Observation(o) = rec {
                match map.get(&o.resource) {
                    Some(prev) if prev.at > o.at => {}
                    _ => {
                        map.insert(o.resource.clone(), o.clone());
                    }
                }
            }
        }
        map
    }

    /// Every address this Source has ever successfully fetched.
    ///
    /// The resume question, and cheaper than [`Self::latest_observations`] when only
    /// membership is wanted.
    pub fn observed(&self) -> std::collections::HashSet<&str> {
        self.records
            .iter()
            .filter_map(|r| match r {
                LogRecord::Observation(o) => Some(o.resource.natural_key.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The newest snapshot of what this Source declares it has.
    pub fn latest_discovery(&self) -> Option<&DiscoveryRun> {
        self.records
            .iter()
            .filter_map(|r| match r {
                LogRecord::DiscoveryRun(d) => Some(d),
                _ => None,
            })
            .next_back()
    }

    /// How this Source was most recently enumerated — `sitemap`, `playlist` — or empty.
    ///
    /// The provenance §4.3 records, and the discriminator that recovers a Source's kind
    /// from the store alone.
    pub fn discovery_method(&self) -> &str {
        self.latest_discovery()
            .map(|d| d.method.as_str())
            .unwrap_or_default()
    }

    /// The newest Derivation taking `from` as its input.
    ///
    /// Newest wins because a re-extraction with a better tool supersedes an older one,
    /// and the Derivation carries the tool and version that say which is which (§4.6).
    pub fn latest_derivation(&self, from: &BlobSha) -> Option<&Derivation> {
        self.derivations().rfind(|d| &d.from_sha == from)
    }

    pub fn derivations(&self) -> impl DoubleEndedIterator<Item = &Derivation> {
        self.records.iter().filter_map(|r| match r {
            LogRecord::Derivation(d) => Some(d),
            _ => None,
        })
    }

    /// Blobs this tool has already derived something from.
    ///
    /// Keyed by tool because a text derivation of a video's *metadata* must not be
    /// mistaken for a transcript of its audio.
    pub fn derived_by(&self, tool: &str) -> std::collections::HashSet<&BlobSha> {
        self.derivations()
            .filter(|d| d.tool == tool)
            .map(|d| &d.from_sha)
            .collect()
    }

    /// Blobs this pipeline at this version already gave up on.
    ///
    /// Keyed by version as well as tool, so bumping the version re-attempts everything a
    /// previous one could not read, and nothing else.
    pub fn underivable_by(&self, tool: &str, version: &str) -> std::collections::HashSet<&BlobSha> {
        self.records
            .iter()
            .filter_map(|r| match r {
                LogRecord::Underivable(u) if u.tool == tool && u.version == version => {
                    Some(&u.from_sha)
                }
                _ => None,
            })
            .collect()
    }

    /// Every derived blob, mapped back to the blob it came from.
    ///
    /// The reverse of [`Self::latest_derivation`], and what lets a hash printed for an
    /// extraction be typed back in. A derived blob is not an Observation — no server ever
    /// served it — so nothing that looked only at Observations could resolve one.
    pub fn derived_from(&self) -> BTreeMap<&BlobSha, &BlobSha> {
        self.derivations()
            .map(|d| (&d.to_sha, &d.from_sha))
            .collect()
    }

    /// The full Observation history of one Resource, oldest first.
    pub fn history(&self, resource: &Resource) -> Vec<&Observation> {
        let mut out: Vec<&Observation> = self
            .records
            .iter()
            .filter_map(|r| match r {
                LogRecord::Observation(o) if &o.resource == resource => Some(o),
                _ => None,
            })
            .collect();
        out.sort_by_key(|o| o.at);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Liveness;

    // ── the read side ──────────────────────────────────────────────────────────

    /// One source holding a page, its extraction, a refusal and a snapshot.
    async fn corpus(dir: &std::path::Path) -> (Store, SourceId, BlobSha, BlobSha) {
        let store = Store::open(dir).await.unwrap();
        let id = SourceId::new("tampa").unwrap();
        let page = Resource::new(id.clone(), "https://tampa.gov/agenda.pdf");
        let gone = Resource::new(id.clone(), "https://tampa.gov/removed.pdf");

        store
            .append(
                &id,
                &LogRecord::DiscoveryRun(DiscoveryRun {
                    source: id.clone(),
                    at: ts("2026-01-01T00:00:00Z"),
                    resources: vec![page.clone(), gone.clone()],
                    method: "sitemap".into(),
                }),
            )
            .await
            .unwrap();

        let obs = store
            .record_observation(&page, b"%PDF-1.7 one", ts("2026-01-02T00:00:00Z"), meta())
            .await
            .unwrap();

        let text = store.put_blob(b"# Agenda").await.unwrap();
        store
            .append(
                &id,
                &LogRecord::Derivation(Derivation {
                    from_sha: obs.blob_sha.clone(),
                    to_sha: text.clone(),
                    tool: "pdf-inspector".into(),
                    version: "0.1".into(),
                    model_tier: None,
                    at: ts("2026-01-03T00:00:00Z"),
                    anchors: Vec::new(),
                }),
            )
            .await
            .unwrap();

        let mut status = ResourceStatus::new_live(gone, ts("2026-01-02T00:00:00Z"));
        status.apply(
            Liveness::Gone,
            ts("2026-01-04T00:00:00Z"),
            Some("HTTP 404".into()),
        );
        store.append(&id, &LogRecord::Status(status)).await.unwrap();

        (store, id, obs.blob_sha, text)
    }

    fn meta() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    /// The point of the type: every view below used to cost its own disk read and JSON
    /// parse, and no call site could see that it was paying for several.
    #[tokio::test]
    async fn one_replay_answers_every_question() {
        let dir = tempfile::tempdir().unwrap();
        let (store, id, original, derived) = corpus(dir.path()).await;
        let replay = store.replay(&id).await.unwrap();

        assert!(!replay.is_empty());
        assert_eq!(replay.discovery_method(), "sitemap");
        assert_eq!(replay.latest_discovery().unwrap().resources.len(), 2);
        assert_eq!(replay.latest_observations().len(), 1);
        assert_eq!(replay.observed().len(), 1);

        let statuses = replay.statuses();
        assert_eq!(statuses.len(), 2, "one observed, one refused");
        assert!(statuses.values().any(|s| s.state == Liveness::Gone));

        assert_eq!(replay.latest_derivation(&original).unwrap().to_sha, derived);
        assert_eq!(replay.derived_from()[&derived], &original);
        assert!(replay.derived_by("pdf-inspector").contains(&original));
    }

    /// A `Replay` answers what the log said when it was read. Nothing invalidates it,
    /// so a caller that appends and expects to see the append must take a fresh one.
    #[tokio::test]
    async fn a_replay_is_a_snapshot_not_a_live_view() {
        let dir = tempfile::tempdir().unwrap();
        let (store, id, _, _) = corpus(dir.path()).await;
        let before = store.replay(&id).await.unwrap();

        store
            .record_observation(
                &Resource::new(id.clone(), "https://tampa.gov/new.pdf"),
                b"%PDF-1.7 two",
                ts("2026-02-01T00:00:00Z"),
                meta(),
            )
            .await
            .unwrap();

        assert_eq!(before.latest_observations().len(), 1, "the snapshot held");
        assert_eq!(
            store.replay(&id).await.unwrap().latest_observations().len(),
            2
        );
    }

    /// A text derivation of a video's *metadata* must not read as a transcript of its
    /// audio, which is why the skip key is the tool and not just the blob.
    #[tokio::test]
    async fn derivations_are_keyed_by_the_tool_that_made_them() {
        let dir = tempfile::tempdir().unwrap();
        let (store, id, original, _) = corpus(dir.path()).await;
        let replay = store.replay(&id).await.unwrap();

        assert!(replay.derived_by("pdf-inspector").contains(&original));
        assert!(replay.derived_by("whisper-rs").is_empty());
    }

    /// Newest wins: a re-extraction with a better tool supersedes an older one.
    #[tokio::test]
    async fn the_newest_derivation_of_a_blob_is_the_one_returned() {
        let dir = tempfile::tempdir().unwrap();
        let (store, id, original, first) = corpus(dir.path()).await;

        let better = store.put_blob(b"# Agenda\n\nwith tables").await.unwrap();
        store
            .append(
                &id,
                &LogRecord::Derivation(Derivation {
                    from_sha: original.clone(),
                    to_sha: better.clone(),
                    tool: "pdf-inspector".into(),
                    version: "0.2".into(),
                    model_tier: None,
                    at: ts("2026-03-01T00:00:00Z"),
                    anchors: Vec::new(),
                }),
            )
            .await
            .unwrap();

        let replay = store.replay(&id).await.unwrap();
        let latest = replay.latest_derivation(&original).unwrap();
        assert_eq!(latest.to_sha, better);
        assert_eq!(latest.version, "0.2");
        // Both are still addressable — the record is append-only.
        assert_eq!(replay.derived_from().len(), 2);
        assert!(replay.derived_from().contains_key(&first));
    }

    #[tokio::test]
    async fn a_source_with_no_log_replays_to_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let replay = store
            .replay(&SourceId::new("nobody").unwrap())
            .await
            .unwrap();

        assert!(replay.is_empty());
        assert_eq!(replay.discovery_method(), "");
        assert!(replay.latest_discovery().is_none());
        assert!(replay.statuses().is_empty());
    }

    // ── blobs ──────────────────────────────────────────────────────────────────

    /// The read that makes classification cheap. `get_blob` reads the whole file and
    /// hashes it, so asking "is this audio?" of a corpus of PDFs used to read every one.
    #[tokio::test]
    async fn a_head_read_stops_at_the_length_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let big = vec![b'x'; 100_000];
        let sha = store.put_blob(&big).await.unwrap();

        assert_eq!(store.blob_head(&sha, 16).await.unwrap().len(), 16);
        assert_eq!(store.blob_head(&sha, 100_000).await.unwrap().len(), 100_000);
        // Asking for more than there is returns what there is, not an error.
        assert_eq!(store.blob_head(&sha, 200_000).await.unwrap().len(), 100_000);
        assert_eq!(store.blob_head(&sha, 4).await.unwrap(), b"xxxx");
    }

    #[tokio::test]
    async fn a_head_read_of_a_missing_blob_says_which_blob() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let absent = BlobSha::from_bytes(b"never stored");

        let err = store.blob_head(&absent, 16).await.unwrap_err();
        assert!(matches!(err, StoreError::BlobNotFound(_)), "{err}");
    }

    /// `put_blob` writes `.<sha>.tmp` and renames. The counter has to know that, and
    /// `doctor` used to re-derive the rule from the other side of the module.
    #[tokio::test]
    async fn counting_blobs_ignores_a_write_in_flight() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        assert_eq!(store.count_blobs().await.unwrap(), 0);

        let sha = store.put_blob(b"a real blob").await.unwrap();
        assert_eq!(store.count_blobs().await.unwrap(), 1);

        // A torn write, left exactly where `put_blob` would leave one.
        let pool = store.blob_path_of(&sha);
        let tmp = pool
            .parent()
            .unwrap()
            .join(format!(".{}.tmp", BlobSha::from_bytes(b"half-written")));
        tokio::fs::write(&tmp, b"half").await.unwrap();

        assert_eq!(
            store.count_blobs().await.unwrap(),
            1,
            "a write in flight is not a blob"
        );
    }

    /// SPEC §5 names every one of these paths, and this module is now the only place
    /// that spells them. A caller that re-derived one — and six did — could drift from
    /// the layout without anything failing.
    #[tokio::test]
    async fn the_layout_is_the_one_spec_5_names() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path()).await.unwrap();

        assert!(s.blobs_dir().ends_with("blobs"));
        assert!(s.current_dir().ends_with("current"));
        assert!(s.vector_cache_dir().ends_with("cache/embeddings"));
        assert!(s.index_path().ends_with("centinel.db"));
        assert!(
            s.blob_path_of(&BlobSha::from_bytes(b"x"))
                .starts_with(s.blobs_dir())
        );
    }

    /// A reader gets a handle without the tree being created underneath it.
    #[test]
    fn a_read_only_handle_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("absent");
        let s = Store::at(&root);
        assert_eq!(s.index_path(), root.join("centinel.db"));
        assert!(!root.exists(), "`at` must not create anything");
    }

    /// Readers get a path or an instruction, never a missing-file error from SQLite.
    #[tokio::test]
    async fn asking_for_a_missing_index_names_the_command_that_builds_one() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path()).await.unwrap();

        let err = s.require_index().map(|_| ()).unwrap_err().to_string();
        assert!(err.contains("centinel index"), "{err}");

        std::fs::write(s.index_path(), b"").unwrap();
        assert!(s.require_index().is_ok());
    }

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
