//! The run journal — one record per **attempt**, and the lock that serialises them.
//!
//! `docs/SCHEDULING.md` §5.2 and §6.
//!
//! ## Why an attempt and not a run
//!
//! A fire that was skipped because the lane was busy is a record. So is one that shutdown
//! interrupted. Those three outcomes — quiet, skipped, interrupted — are the three most
//! likely answers to "why is this corpus stale", and dropping them leaves the question
//! unanswerable from the record. Only failures would be visible, and a schedule that never
//! fires produces none of those either.
//!
//! ## Why the whole report is embedded
//!
//! A few kilobytes, per-source and per-stage, carrying `summary` and `error` separately as
//! `StageRun` already does. Every surface knows how to render it. A summarised copy would
//! be a second vocabulary for the same facts, and the first thing anyone asks of a failed
//! run is the detail a summary dropped.

use std::path::Path;

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::ops::RunReport;
use crate::store::Store;

/// What caused an attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// A `[[schedule]]` block came due.
    Schedule,
    /// Somebody typed `centinel run`.
    Manual,
    /// The server started and found a schedule more than one interval overdue.
    CatchUp,
}

impl Trigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
            Self::Manual => "manual",
            Self::CatchUp => "catch-up",
        }
    }
}

/// How an attempt ended.
///
/// Five, and the last three all produced no work for reasons that read completely
/// differently. Collapsing them into "failed" would report a shutdown and a busy lane as
/// broken collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Ran, and every stage that was attempted succeeded.
    Ok,
    /// Ran, and some stages failed. Half a corpus collected is still half a corpus.
    Partial,
    /// Did not get as far as a report.
    Failed,
    /// Never started: the lane was held by another run.
    Skipped,
    /// Started, and was asked to stop. **Not a fault** — every stage is resumable, so the
    /// next fire continues from exactly here.
    Interrupted,
}

impl Outcome {
    /// Whether this attempt reflects badly on the schedule, for `consecutive_failures`.
    ///
    /// `Skipped` and `Interrupted` do not: one is the queue working as designed and the
    /// other is the operator stopping the process. Counting either would let a nightly
    /// restart look like a source that had stopped responding.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed | Self::Partial)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Interrupted => "interrupted",
        }
    }
}

/// What entered the corpus.
///
/// A new address and a **new version of a known address** are both additions, because
/// every version is retained. **A page changing is an addition, never a subtraction** —
/// the previous version is still there, still addressable, still searchable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Added {
    /// Observations stored: new addresses and new versions of known ones.
    pub documents: u64,
    /// Documents that yielded text this run.
    pub derived: u64,
    /// Chunks embedded this run.
    pub chunks: u64,
}

/// What stopped appearing, or started refusing — **never what was removed.**
///
/// > A Centinel corpus never loses anything. No blob is deleted, no Observation is
/// > retracted, no log line is rewritten.
///
/// So a "subtraction" is not a deletion, and the three below are three different facts.
/// Summing them is how a live page gets recorded as deleted: `Blocked` exists precisely
/// because a CloudFront 403 would otherwise be indistinguishable from a page that did not
/// change, and reading it as absence would log a live page as gone.
///
/// The renderer prints three columns and never adds them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Subtracted {
    /// In the previous DiscoveryRun, absent from this one. The site stopped *listing* it;
    /// it may still be served.
    pub vanished: u64,
    /// Fetched, and the server said 404 or 410.
    pub gone: u64,
    /// Refused in a way that is not evidence of absence — WAF 403, 429, robots, the bot
    /// wall. **Evidence about the request, not about the page.**
    pub blocked: u64,
    /// A transport fault: a timeout, a 500, a hang that had to be killed. Evidence about
    /// this machine, and not a subtraction at all — carried here so a reader scanning for
    /// what broke finds it beside the others rather than nowhere.
    pub errored: u64,
}

impl Subtracted {
    /// Whether anything at all moved. Deliberately not a sum — see the type's own note.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Reads the two halves off a finished report.
///
/// One place, so no surface computes its own. `RunReport` already carries every number:
/// the additions as headline counts, the subtractions as per-stage figures that `discover`
/// and `collect` write.
pub fn arithmetic(report: &RunReport) -> (Added, Subtracted) {
    use crate::ops::Stage;

    let figure = |stage: Stage, key: &str| -> u64 {
        report
            .sources
            .iter()
            .flat_map(|s| s.stages.iter())
            .filter(|s| s.stage == stage)
            .filter_map(|s| s.figures.get(key))
            .sum()
    };

    let derived = report
        .derive
        .iter()
        .filter(|s| matches!(s.stage, Stage::Extract | Stage::Transcribe))
        .map(|s| s.new)
        .sum();

    let added = Added {
        documents: report.new_documents,
        derived,
        chunks: report.new_chunks,
    };

    let subtracted = Subtracted {
        vanished: figure(Stage::Discover, "vanished"),
        gone: figure(Stage::Collect, "gone"),
        blocked: figure(Stage::Collect, "blocked"),
        errored: figure(Stage::Collect, "errored"),
    };

    (added, subtracted)
}

/// One attempt, appended when it finishes.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Attempt {
    /// The instant it started, which is also its handle.
    ///
    /// Unique within a store because the lane is single, and it sorts in the order the
    /// runs happened. `history --run 2026-08-06T07` resolves it by prefix, which is the
    /// rule the rest of the tool follows: anything Centinel prints, Centinel takes back.
    pub run_id: String,
    /// The `[[schedule]]` that fired, or `None` for a manual run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    pub trigger: Trigger,
    /// When it was nominally due, with jitter applied. Absent for a manual run, which is
    /// due the moment it is typed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_at: Option<String>,
    pub started_at: String,
    pub finished_at: String,
    pub outcome: Outcome,
    /// Why it was skipped, or what interrupted it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub added: Added,
    #[serde(default)]
    pub subtracted: Subtracted,
    /// The report, verbatim. Absent when the attempt never produced one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<RunReport>,
}

impl Attempt {
    /// Whether this attempt's id starts with `prefix` — the git-style resolution rule.
    pub fn matches(&self, prefix: &str) -> bool {
        self.run_id.starts_with(prefix)
    }

    pub fn started(&self) -> Option<Timestamp> {
        self.started_at.parse().ok()
    }

    /// Seconds from start to finish, for the report.
    pub fn elapsed_secs(&self) -> f64 {
        match (
            self.started_at.parse::<Timestamp>(),
            self.finished_at.parse::<Timestamp>(),
        ) {
            (Ok(a), Ok(b)) => (b - a).total(jiff::Unit::Second).unwrap_or(0.0),
            _ => 0.0,
        }
    }
}

/// Reads and appends the run journal.
///
/// A thin thing on purpose: the paths belong to [`Store`], and everything else here is one
/// append and one scan.
pub struct Journal<'a> {
    store: &'a Store,
}

impl<'a> Journal<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// Appends one attempt. Opened, written, flushed and closed per call, exactly as the
    /// log is, so a crash cannot lose a buffered record.
    pub async fn append(&self, attempt: &Attempt) -> anyhow::Result<()> {
        let at: Timestamp = attempt
            .started_at
            .parse()
            .unwrap_or_else(|_| Timestamp::now());
        let path = self.store.runs_path(at);
        let dir = path.parent().expect("a runs path always has a parent");
        tokio::fs::create_dir_all(dir).await?;

        let mut line = serde_json::to_vec(attempt)?;
        line.push(b'\n');

        let mut f = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        f.write_all(&line).await?;
        f.flush().await?;
        Ok(())
    }

    /// Every attempt, newest first.
    ///
    /// Reads the whole journal. At one record per fire per schedule this is kilobytes a
    /// day, and the alternative — an index over it — would be a derived thing to keep in
    /// step with the only copy of a fact.
    pub async fn read(&self) -> anyhow::Result<Vec<Attempt>> {
        let dir = self.store.runs_dir();
        let mut months: Vec<std::path::PathBuf> = Vec::new();

        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            let p = entry.path();
            if p.extension().is_some_and(|x| x == "jsonl") {
                months.push(p);
            }
        }
        months.sort();

        let mut out = Vec::new();
        for month in months {
            let text = tokio::fs::read_to_string(&month).await?;
            for (n, line) in text.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<Attempt>(line) {
                    Ok(a) => out.push(a),
                    // One unreadable line must not hide the rest of the history. It is
                    // also not silent: a journal is small enough that a warning here will
                    // be seen, unlike one in a million-line log.
                    Err(e) => tracing::warn!(
                        file = %month.display(),
                        line = n + 1,
                        error = %e,
                        "skipping an unreadable journal record"
                    ),
                }
            }
        }
        out.sort_by(|a, b| b.run_id.cmp(&a.run_id));
        Ok(out)
    }

    /// The most recent attempt for one schedule.
    pub async fn last_for(&self, schedule: &str) -> anyhow::Result<Option<Attempt>> {
        Ok(self
            .read()
            .await?
            .into_iter()
            .find(|a| a.schedule.as_deref() == Some(schedule)))
    }
}

// ── the lock ──────────────────────────────────────────────────────────────────

/// What a held lock says about the run holding it.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Holder {
    pub pid: u32,
    pub started_at: String,
    pub trigger: Trigger,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    /// The run's arguments, one line, for the message a refused second run prints.
    #[serde(default)]
    pub args: String,
}

impl Holder {
    /// How this reads when a second run is refused.
    pub fn describe(&self) -> String {
        let since = self
            .started_at
            .parse::<Timestamp>()
            .map(|t| {
                let mins = (Timestamp::now() - t)
                    .total(jiff::Unit::Minute)
                    .unwrap_or(0.0)
                    .round() as i64;
                format!("{mins} minutes ago")
            })
            .unwrap_or_else(|_| self.started_at.clone());
        match &self.schedule {
            Some(id) => format!(
                "a run has been in flight since {since} (pid {}, schedule `{id}`)",
                self.pid
            ),
            None => format!("a run has been in flight since {since} (pid {})", self.pid),
        }
    }

    /// Whether the process that wrote this is still alive.
    ///
    /// `kill(pid, 0)` sends no signal and reports whether the pid is addressable. It can
    /// be wrong in one direction — a recycled pid now belonging to something else reads
    /// as alive — which is why `--force` exists and why the answer is only ever used to
    /// decide whether to *reclaim*, never to decide whether to keep running.
    #[cfg(unix)]
    pub fn is_alive(&self) -> bool {
        // SAFETY: `kill` with signal 0 performs the permission and existence check and
        // delivers nothing. It cannot affect this process or any other.
        unsafe { libc::kill(self.pid as libc::pid_t, 0) == 0 }
    }

    /// Without a portable liveness check, a lock is only ever released by its owner.
    ///
    /// The consequence is that a killed run on this platform needs `--force`, which is a
    /// worse day than on unix and a better one than reclaiming a lock from a process that
    /// is still writing to the store.
    #[cfg(not(unix))]
    pub fn is_alive(&self) -> bool {
        true
    }
}

/// The single lane, held for the length of one run.
///
/// Released on drop. A process that dies without dropping leaves the file behind, which is
/// the case [`RunLock::acquire`] reclaims — and the evidence [`RunLock::take_stale`] turns
/// into an `interrupted` record.
#[derive(Debug)]
pub struct RunLock {
    path: std::path::PathBuf,
}

/// Why the lane could not be taken.
#[derive(Debug)]
pub struct Held(pub Holder);

impl std::fmt::Display for Held {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.describe())
    }
}

impl std::error::Error for Held {}

impl RunLock {
    /// Takes the lane, or reports who has it.
    ///
    /// `create_new` is the whole mechanism: on POSIX it is an atomic
    /// create-if-absent, so two processes racing cannot both win. A lock left by a dead
    /// process is reclaimed once, and only once — if the retry also loses, somebody else
    /// won the race fairly and this waits its turn.
    pub fn acquire(store: &Store, holder: &Holder) -> std::result::Result<Self, Held> {
        let path = store.lock_path();
        match Self::try_create(&path, holder) {
            Ok(lock) => Ok(lock),
            Err(existing) => {
                if existing.is_alive() {
                    return Err(Held(existing));
                }
                // Stale. Reclaim, then try exactly once more.
                let _ = std::fs::remove_file(&path);
                Self::try_create(&path, holder).map_err(Held)
            }
        }
    }

    fn try_create(path: &Path, holder: &Holder) -> std::result::Result<Self, Holder> {
        use std::io::Write;
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut f) => {
                let body =
                    serde_json::to_vec_pretty(holder).expect("a Holder is always serializable");
                let _ = f.write_all(&body);
                let _ = f.flush();
                Ok(Self {
                    path: path.to_path_buf(),
                })
            }
            Err(_) => Err(Self::read(path).unwrap_or_else(|| Holder {
                // A lock file that exists but cannot be read is still a lock. Reporting it
                // as unheld would let a second run start beside the first.
                pid: 0,
                started_at: Timestamp::now().to_string(),
                trigger: Trigger::Manual,
                schedule: None,
                args: "unreadable lock file".into(),
            })),
        }
    }

    /// Who holds the lane right now, without taking it.
    ///
    /// What `schedules` reports as "running now" — and because it reads the same file the
    /// CLI writes, both surfaces answer identically, including when no server is running.
    pub fn read(path: &Path) -> Option<Holder> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Whoever holds the lane in this store, if anyone.
    pub fn current(store: &Store) -> Option<Holder> {
        Self::read(&store.lock_path())
    }

    /// Reclaims a lock whose owner is dead, returning what it knew.
    ///
    /// Called at startup: the record is appended at *finish*, so a killed process leaves
    /// no attempt behind — only this. Turning it into an `interrupted` record is what
    /// makes a crash visible instead of a gap.
    pub fn take_stale(store: &Store) -> Option<Holder> {
        let path = store.lock_path();
        let holder = Self::read(&path)?;
        if holder.is_alive() {
            return None;
        }
        std::fs::remove_file(&path).ok()?;
        Some(holder)
    }

    /// Releases the lane early. Dropping does the same.
    pub fn release(self) {}
}

impl Drop for RunLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        (dir, store)
    }

    fn holder(schedule: Option<&str>) -> Holder {
        Holder {
            pid: std::process::id(),
            started_at: Timestamp::now().to_string(),
            trigger: Trigger::Schedule,
            schedule: schedule.map(str::to_string),
            args: "--source tampa".into(),
        }
    }

    fn attempt(id: &str, schedule: Option<&str>, outcome: Outcome) -> Attempt {
        Attempt {
            run_id: id.into(),
            schedule: schedule.map(str::to_string),
            trigger: Trigger::Schedule,
            due_at: None,
            started_at: id.into(),
            finished_at: id.into(),
            outcome,
            detail: None,
            added: Added::default(),
            subtracted: Subtracted::default(),
            report: None,
        }
    }

    #[tokio::test]
    async fn an_empty_journal_is_an_ordinary_state() {
        let (_d, store) = store().await;
        assert!(Journal::new(&store).read().await.unwrap().is_empty());
    }

    /// Newest first, and across month boundaries — the ordering every report depends on.
    #[tokio::test]
    async fn attempts_read_back_newest_first_across_months() {
        let (_d, store) = store().await;
        let j = Journal::new(&store);
        for id in [
            "2026-07-31T03:00:00Z",
            "2026-08-01T03:00:00Z",
            "2026-08-06T03:00:00Z",
        ] {
            j.append(&attempt(id, Some("daily"), Outcome::Ok))
                .await
                .unwrap();
        }

        let all = j.read().await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].run_id, "2026-08-06T03:00:00Z");
        assert_eq!(all[2].run_id, "2026-07-31T03:00:00Z");

        // Two months, two files.
        let mut files: Vec<_> = std::fs::read_dir(store.runs_dir())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        files.sort();
        assert_eq!(files, ["2026-07.jsonl", "2026-08.jsonl"]);
    }

    /// The whole point of recording attempts rather than runs: a fire that did nothing is
    /// the common outcome, and it has to be distinguishable from no fire at all.
    #[tokio::test]
    async fn a_skipped_fire_is_a_record_and_does_not_count_as_a_failure() {
        let (_d, store) = store().await;
        let j = Journal::new(&store);
        j.append(&attempt(
            "2026-08-06T03:00:00Z",
            Some("daily"),
            Outcome::Skipped,
        ))
        .await
        .unwrap();

        let last = j.last_for("daily").await.unwrap().unwrap();
        assert_eq!(last.outcome, Outcome::Skipped);
        assert!(!last.outcome.is_failure());
        assert!(
            !Outcome::Interrupted.is_failure(),
            "a shutdown is not a fault"
        );
        assert!(Outcome::Partial.is_failure());
    }

    #[tokio::test]
    async fn last_for_ignores_other_schedules_and_manual_runs() {
        let (_d, store) = store().await;
        let j = Journal::new(&store);
        j.append(&attempt("2026-08-06T01:00:00Z", Some("daily"), Outcome::Ok))
            .await
            .unwrap();
        j.append(&attempt(
            "2026-08-06T02:00:00Z",
            Some("weekly"),
            Outcome::Ok,
        ))
        .await
        .unwrap();
        j.append(&attempt("2026-08-06T03:00:00Z", None, Outcome::Ok))
            .await
            .unwrap();

        let last = j.last_for("daily").await.unwrap().unwrap();
        assert_eq!(last.run_id, "2026-08-06T01:00:00Z");
    }

    /// One corrupt line must not hide the history behind it.
    #[tokio::test]
    async fn an_unreadable_record_does_not_hide_the_rest() {
        let (_d, store) = store().await;
        let j = Journal::new(&store);
        j.append(&attempt("2026-08-06T01:00:00Z", Some("daily"), Outcome::Ok))
            .await
            .unwrap();

        let path = store.runs_path("2026-08-06T01:00:00Z".parse().unwrap());
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("{ this is not json\n");
        std::fs::write(&path, text).unwrap();

        assert_eq!(j.read().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_run_id_resolves_by_prefix() {
        let a = attempt("2026-08-06T03:00:11Z", None, Outcome::Ok);
        assert!(a.matches("2026-08-06T03"));
        assert!(a.matches("2026-08-06T03:00:11Z"));
        assert!(!a.matches("2026-08-07"));
    }

    // ── the lock ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn the_lane_admits_one_holder_and_names_it_to_the_second() {
        let (_d, store) = store().await;
        let first = RunLock::acquire(&store, &holder(Some("daily"))).unwrap();

        let refused = RunLock::acquire(&store, &holder(None)).unwrap_err();
        let message = refused.to_string();
        assert!(message.contains("in flight"), "{message}");
        assert!(
            message.contains("daily"),
            "the holder must be named: {message}"
        );

        // And the same fact is readable without taking the lane — this is what
        // `schedules` prints as "running now".
        assert!(RunLock::current(&store).is_some());

        drop(first);
        assert!(RunLock::current(&store).is_none());
        RunLock::acquire(&store, &holder(None)).expect("the lane was not released");
    }

    /// A killed run leaves the file behind. Reclaiming it is what stops one crash from
    /// wedging every future fire.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_lock_held_by_a_dead_process_is_reclaimed() {
        let (_d, store) = store().await;
        let dead = Holder {
            // Above any real pid, and never allocated: `kill(0)` on it fails.
            pid: 0x00ff_fffe,
            ..holder(Some("daily"))
        };
        std::fs::write(store.lock_path(), serde_json::to_vec_pretty(&dead).unwrap()).unwrap();

        // The evidence survives the reclaim, so a crash becomes an `interrupted` record
        // rather than a silent gap in the journal.
        let recovered = RunLock::take_stale(&store).expect("a dead holder must be reclaimed");
        assert_eq!(recovered.schedule.as_deref(), Some("daily"));
        assert!(RunLock::current(&store).is_none());

        RunLock::acquire(&store, &holder(None)).expect("the stale lock was not cleared");
    }

    /// The opposite mistake, and the more dangerous one: reclaiming a lock from a process
    /// that is still writing to the store.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_lock_held_by_a_live_process_is_left_alone() {
        let (_d, store) = store().await;
        let _held = RunLock::acquire(&store, &holder(None)).unwrap();
        assert!(
            RunLock::take_stale(&store).is_none(),
            "a live holder's lock was reclaimed"
        );
    }

    /// An unreadable lock file is still a lock; treating it as absent would start a second
    /// run beside the first.
    #[tokio::test]
    async fn an_unreadable_lock_is_still_held() {
        let (_d, store) = store().await;
        std::fs::write(store.lock_path(), b"not json at all").unwrap();
        assert!(RunLock::acquire(&store, &holder(None)).is_err());
    }

    /// The end of the path the previous two files start: `discover` and `collect` count
    /// the refusals apart, `run` carries them as figures, and this reads them back without
    /// any surface doing its own arithmetic.
    #[test]
    fn the_arithmetic_comes_off_the_report_with_the_subtractions_still_apart() {
        use crate::ops::{RunReport, SourceRun, Stage, StageRun};
        use std::collections::BTreeMap;

        let stage = |stage: Stage, new: u64, figures: &[(&str, u64)]| StageRun {
            stage,
            status: crate::ops::StageStatus::Ran,
            summary: String::new(),
            figures: figures
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect::<BTreeMap<_, _>>(),
            new,
            elapsed_secs: 0.0,
        };

        let report = RunReport {
            config: None,
            sources: vec![SourceRun {
                source: "tampa".into(),
                kind: crate::domain::SourceKind::Site,
                target: "https://tampa.gov".into(),
                stages: vec![
                    stage(Stage::Discover, 4, &[("found", 100), ("vanished", 7)]),
                    stage(
                        Stage::Collect,
                        4,
                        &[("stored", 4), ("blocked", 40), ("gone", 2), ("errored", 1)],
                    ),
                ],
                elapsed_secs: 0.0,
            }],
            derive: vec![
                stage(Stage::Extract, 3, &[]),
                stage(Stage::Transcribe, 1, &[]),
                stage(Stage::Embed, 12, &[]),
            ],
            new_documents: 4,
            new_chunks: 12,
            failed_stages: 0,
            elapsed_secs: 0.0,
            dry_run: false,
        };

        let (added, subtracted) = arithmetic(&report);
        assert_eq!(added.documents, 4);
        assert_eq!(added.derived, 4, "extract and transcribe both derive text");
        assert_eq!(added.chunks, 12);

        assert_eq!(subtracted.vanished, 7);
        assert_eq!(subtracted.gone, 2);
        assert_eq!(subtracted.blocked, 40);
        assert_eq!(subtracted.errored, 1);
    }

    #[test]
    fn the_three_subtractions_are_never_summed() {
        let s = Subtracted {
            vanished: 3,
            gone: 2,
            blocked: 40,
            errored: 1,
        };
        assert!(!s.is_empty());
        // There is deliberately no `total()`. A blocked address counted as absence
        // reports a live page as deleted — the mistake `Liveness::Blocked` exists to
        // prevent. If this test ever needs updating to accommodate a sum, the sum is
        // the bug.
        assert!(Subtracted::default().is_empty());
    }
}
