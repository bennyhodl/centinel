//! Jobs: what this process is working on, as it happens, for whoever is watching.
//!
//! An op reports into [`Progress`] and never learns who listens. The CLI writes that
//! stream to its log and `/ops/{name}/stream` forwards it to the one caller that asked;
//! neither outlives the invocation, and a browser opened after a scheduled run started
//! has no way in. This module is the listener that does: every long-running invocation in the
//! process is a *job*, its events are folded into a small current state and a bounded
//! tail, and any number of subscribers can join at any point and read the same thing.
//!
//! ## Derived from `Progress`, not emitted beside it
//!
//! A stage already says everything a watcher needs: each page it fetched, each document
//! it read, how far through its work list it is. Asking it to say the same things again
//! to a second sink would be two vocabularies to keep in step. So a job is attached to the
//! [`Progress`] an op already holds ([`Job::watch`]) and every event is translated once:
//!
//! | [`ProgressEvent`] | [`JobEvent`] |
//! |---|---|
//! | carries an `item` | `item`: one finished unit of work, failures included |
//! | on [`TOTAL_TRACK`] | `step`: the stage the job is now in |
//! | carries `done` and `total` | `progress`: the count inside that stage |
//! | anything else | `note` |
//!
//! ## Topics, a snapshot, and a tail
//!
//! Every event names its job, and the job id is the topic: a subscriber that wants one
//! job filters on it. Each event is stamped with the server's clock and a sequence number
//! that only grows, so a client that sees an event twice — a snapshot and then the live
//! stream it overlapped — can tell. Counts are state, not history: `progress` events move
//! the job's numbers and are broadcast, but only the events a person reads as a log line
//! are kept in the tail, so two hundred ticks of a counter never push the last page that
//! failed out of it.
//!
//! ## Stopping one
//!
//! A job started with [`Jobs::start_stoppable`] carries the [`Canceller`] end of a
//! [`Cancel`] token, and [`Jobs::stop`] trips it by job id. The token travels to the op
//! the way `Progress` does, so the op never learns who asked; it stops at its next item
//! boundary, where everything so far is durable ([`Cancel`]). Held here rather than by
//! whoever started the job because the Stop button is on a page that found the job by
//! its id, and the id is the one handle the page and the server share.
//!
//! ## In memory, for this process
//!
//! Nothing here is written down. The record of what a run did is the journal and the log;
//! this is the window onto what it is doing. A restart empties it, and a `centinel run`
//! typed in another terminal is another process with its own.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::op::{Cancel, Canceller, ItemOutcome, Progress, ProgressEvent, TOTAL_TRACK};

/// Events kept per job for a subscriber that arrives late.
const TAIL: usize = 200;

/// Finished jobs kept, newest first, so a run that ended a minute ago still reads.
const FINISHED: usize = 20;

/// Events a slow subscriber may fall behind by before it is sent a fresh snapshot.
const CAPACITY: usize = 1024;

/// How a job ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Failed,
    /// Asked to stop. Not a fault; see [`crate::op::Cancelled`].
    Cancelled,
}

/// One thing that happened to a job.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobEvent {
    Started {
        /// What runs: the op name, or `classify` for a workspace run.
        kind: String,
        /// Who asked, or what it runs over.
        label: String,
        /// Whether [`Jobs::stop`] can end it. Said at the start, so a page that follows
        /// the stream knows whether to offer a Stop button without a second read.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        stoppable: bool,
    },
    /// The stage the job is now in: `tampa.gov · collect`, `embed`.
    Step {
        step: String,
    },
    /// How far through its current stage the job is.
    Progress {
        message: String,
        done: u64,
        total: u64,
        /// The item in hand, where the stage says so ahead of its outcome.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        current: Option<String>,
    },
    /// One unit of work finished. A failure is an item whose verdict produced nothing.
    Item {
        item: ItemOutcome,
    },
    Note {
        message: String,
    },
    Finished {
        outcome: Outcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

impl From<&ProgressEvent> for JobEvent {
    fn from(event: &ProgressEvent) -> Self {
        if let Some(item) = &event.item {
            return Self::Item { item: item.clone() };
        }
        if event.id.as_deref() == Some(TOTAL_TRACK) {
            return Self::Step {
                step: event.message.clone(),
            };
        }
        match (event.done, event.total) {
            (Some(done), Some(total)) => Self::Progress {
                message: event.message.clone(),
                done,
                total,
                current: event.current.clone(),
            },
            _ => Self::Note {
                message: event.message.clone(),
            },
        }
    }
}

/// A [`JobEvent`] as it goes out: which job, when, and in what order.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stamped {
    /// Grows by one per event across every job in the process.
    pub seq: u64,
    /// Milliseconds since the Unix epoch, on the server's clock.
    pub at: i64,
    pub job: String,
    #[serde(flatten)]
    pub event: JobEvent,
}

/// A job as it stands: the fold of every event it has had, and the tail of the readable
/// ones.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobState {
    pub id: String,
    pub kind: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stoppable: bool,
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
    /// The last event folded in, so a client can skip one it already has.
    pub seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    /// Items that produced something, over the whole job.
    pub ok: u64,
    /// Items that produced nothing, over the whole job.
    pub failed: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The latest readable events, oldest first.
    pub log: VecDeque<Stamped>,
}

impl JobState {
    fn new(event: &Stamped, kind: &str, label: &str, stoppable: bool) -> Self {
        Self {
            id: event.job.clone(),
            kind: kind.to_string(),
            label: label.to_string(),
            stoppable,
            started_at: event.at,
            finished_at: None,
            seq: 0,
            step: None,
            message: None,
            done: None,
            total: None,
            current: None,
            ok: 0,
            failed: 0,
            outcome: None,
            error: None,
            log: VecDeque::new(),
        }
    }

    /// Folds one event in. The web client folds the same events the same way, in
    /// `web/src/job-state.ts`; the two must agree.
    fn apply(&mut self, event: &Stamped) {
        self.seq = event.seq;
        match &event.event {
            JobEvent::Started { .. } | JobEvent::Note { .. } => {}
            JobEvent::Step { step } => {
                // A new stage counts a new work list; the last one's numbers would read
                // as this one's until its first tick.
                self.step = Some(step.clone());
                self.message = None;
                self.done = None;
                self.total = None;
                self.current = None;
            }
            JobEvent::Progress {
                message,
                done,
                total,
                current,
            } => {
                self.message = Some(message.clone());
                self.done = Some(*done);
                self.total = Some(*total);
                self.current = current.clone();
            }
            JobEvent::Item { item } => {
                if item.succeeded() {
                    self.ok += 1;
                } else {
                    self.failed += 1;
                }
            }
            JobEvent::Finished { outcome, error } => {
                self.finished_at = Some(event.at);
                self.outcome = Some(*outcome);
                self.error = error.clone();
                self.current = None;
            }
        }
        if !matches!(event.event, JobEvent::Progress { .. }) {
            self.log.push_back(event.clone());
            if self.log.len() > TAIL {
                self.log.pop_front();
            }
        }
    }
}

/// The process's jobs, and the channel their events go out on.
///
/// Cheap to clone; every clone is the same hub. Recording an event takes a short lock and
/// a channel send that never waits, so a stage reporting into it is not slowed by who is
/// watching or by nobody watching.
#[derive(Clone, Debug, Default)]
pub struct Jobs {
    inner: Arc<Hub>,
}

#[derive(Debug)]
struct Hub {
    tx: broadcast::Sender<Stamped>,
    board: Mutex<Board>,
    /// Numbers the ids [`Jobs::start`] hands out.
    started: AtomicU64,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            tx: broadcast::channel(CAPACITY).0,
            board: Mutex::default(),
            started: AtomicU64::new(0),
        }
    }
}

#[derive(Debug, Default)]
struct Board {
    seq: u64,
    /// In the order they started.
    active: Vec<JobState>,
    /// Newest first.
    finished: VecDeque<JobState>,
    /// The stop handle of each active job that has one, by id. Dropped as the job
    /// finishes: a stop asked of a finished job is nothing.
    stoppers: BTreeMap<String, Canceller>,
}

impl Board {
    fn snapshot(&self) -> Vec<JobState> {
        self.active
            .iter()
            .chain(self.finished.iter())
            .cloned()
            .collect()
    }
}

impl Jobs {
    /// Starts a job with an id of its own: the kind and a number unique in this process.
    pub fn start(&self, kind: &str, label: impl Into<String>) -> Job {
        let n = self.inner.started.fetch_add(1, Ordering::Relaxed) + 1;
        self.start_as(format!("{kind}-{n}"), kind, label)
    }

    /// Starts a job under an id the caller already has — a classifier run's, so the page
    /// that started it can find it.
    pub fn start_as(&self, id: impl Into<String>, kind: &str, label: impl Into<String>) -> Job {
        self.begin(id.into(), kind, label.into(), None)
    }

    /// [`Jobs::start_as`], for work that [`Jobs::stop`] may end. The token is the op's to
    /// poll; the handle that trips it stays here, under the job's id.
    pub fn start_stoppable(
        &self,
        id: impl Into<String>,
        kind: &str,
        label: impl Into<String>,
    ) -> (Job, Cancel) {
        let (canceller, cancel) = Cancel::channel();
        (self.begin(id.into(), kind, label.into(), Some(canceller)), cancel)
    }

    /// Asks a running job to stop at its next item boundary. False when no active job
    /// has this id, or the one that does was not started as stoppable.
    pub fn stop(&self, id: &str) -> bool {
        match self.board().stoppers.get(id) {
            Some(canceller) => {
                canceller.cancel();
                true
            }
            None => false,
        }
    }

    fn begin(&self, id: String, kind: &str, label: String, stopper: Option<Canceller>) -> Job {
        let id: Arc<str> = id.into();
        let stoppable = stopper.is_some();
        if let Some(canceller) = stopper {
            self.board().stoppers.insert(id.to_string(), canceller);
        }
        self.publish(
            &id,
            JobEvent::Started {
                kind: kind.to_string(),
                label,
                stoppable,
            },
        );
        Job {
            sink: JobSink {
                jobs: self.clone(),
                id,
            },
            finished: false,
        }
    }

    /// Every active job in the order it started, then the recently finished, newest first.
    pub fn snapshot(&self) -> Vec<JobState> {
        self.board().snapshot()
    }

    /// The snapshot and a receiver for everything after it, taken together: no event can
    /// land between the two, so a subscriber neither misses one nor sees one twice.
    pub fn subscribe(&self) -> (Vec<JobState>, broadcast::Receiver<Stamped>) {
        let board = self.board();
        (board.snapshot(), self.inner.tx.subscribe())
    }

    fn board(&self) -> std::sync::MutexGuard<'_, Board> {
        // A panic while folding one event must not stop every later job from reporting.
        self.inner
            .board
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Stamps, folds and sends one event. An event for a job that has already finished is
    /// dropped: a task the op left behind is not news about the job.
    fn publish(&self, job: &str, event: JobEvent) {
        let mut board = self.board();
        board.seq += 1;
        let stamped = Stamped {
            seq: board.seq,
            at: jiff::Timestamp::now().as_millisecond(),
            job: job.to_string(),
            event,
        };
        if let JobEvent::Started {
            kind,
            label,
            stoppable,
        } = &stamped.event
        {
            let mut state = JobState::new(&stamped, kind, label, *stoppable);
            state.apply(&stamped);
            board.active.push(state);
        } else {
            let Some(at) = board.active.iter().position(|j| j.id == job) else {
                return;
            };
            board.active[at].apply(&stamped);
            if matches!(stamped.event, JobEvent::Finished { .. }) {
                let done = board.active.remove(at);
                board.stoppers.remove(&done.id);
                board.finished.push_front(done);
                board.finished.truncate(FINISHED);
            }
        }
        // Sent under the lock, which is what lets `subscribe` promise no gap. No receiver
        // is the ordinary case, not an error.
        let _ = self.inner.tx.send(stamped);
    }
}

/// The half of a [`Job`] a [`Progress`] carries: it records, and cannot finish.
#[derive(Clone, Debug)]
pub(crate) struct JobSink {
    jobs: Jobs,
    id: Arc<str>,
}

impl JobSink {
    pub(crate) fn record(&self, event: &ProgressEvent) {
        self.jobs.publish(&self.id, JobEvent::from(event));
    }
}

/// A running job. Whoever started the work holds it and says how the work ended.
///
/// Dropped without [`Job::finish`] — a task that panicked, a future that was abandoned —
/// it finishes itself as failed, so a job never stays active forever on a page that is
/// waiting for it.
#[derive(Debug)]
pub struct Job {
    sink: JobSink,
    finished: bool,
}

impl Job {
    pub fn id(&self) -> &str {
        &self.sink.id
    }

    /// `progress`, reporting into this job as well as wherever it already went.
    pub fn watch(&self, progress: Progress) -> Progress {
        progress.reporting_to(self.sink.clone())
    }

    /// Records how the work ended. Call it after the op has returned, so every event the
    /// op sent is folded in before the job reads as finished.
    pub fn finish<T>(mut self, result: &anyhow::Result<T>) {
        let (outcome, error) = match result {
            Ok(_) => (Outcome::Ok, None),
            Err(e) if crate::op::is_cancelled(e) => (Outcome::Cancelled, None),
            Err(e) => (Outcome::Failed, Some(format!("{e:#}"))),
        };
        self.end(outcome, error);
    }

    fn end(&mut self, outcome: Outcome, error: Option<String>) {
        self.finished = true;
        self.sink
            .jobs
            .publish(&self.sink.id, JobEvent::Finished { outcome, error });
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if !self.finished {
            self.end(
                Outcome::Failed,
                Some("stopped without saying how it ended".into()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::{Unit, Verdict};

    fn page(address: &str, verdict: Verdict) -> ItemOutcome {
        ItemOutcome {
            address: address.into(),
            tag: "200".into(),
            verdict,
            noun: "requests".into(),
            bytes: 10,
            produced: None,
            millis: 5,
            detail: None,
            nested: false,
        }
    }

    fn kinds(events: &[Stamped]) -> Vec<&'static str> {
        events
            .iter()
            .map(|e| match e.event {
                JobEvent::Started { .. } => "started",
                JobEvent::Step { .. } => "step",
                JobEvent::Progress { .. } => "progress",
                JobEvent::Item { .. } => "item",
                JobEvent::Note { .. } => "note",
                JobEvent::Finished { .. } => "finished",
            })
            .collect()
    }

    /// What an op says into its `Progress` reaches a subscriber as the job's events, in
    /// the order it was said, each stamped later than the one before.
    #[tokio::test]
    async fn a_subscriber_receives_a_jobs_events_in_order() {
        let jobs = Jobs::default();
        let (before, mut rx) = jobs.subscribe();
        assert!(before.is_empty());

        let job = jobs.start("run", "schedule");
        let progress = job.watch(Progress::none());
        progress.track(TOTAL_TRACK, "tampa.gov · collect", 1, 9, Unit::Count);
        progress.step_on("0 stored, 0 failed", 0, 2, "https://tampa.gov/a");
        progress.item(page("https://tampa.gov/a", Verdict::Ok));
        progress.item(page("https://tampa.gov/b", Verdict::Fail));
        progress.say("2 resources");
        drop(progress);
        let id = job.id().to_string();
        job.finish(&anyhow::Ok(()));

        let mut seen = Vec::new();
        while let Ok(event) = rx.try_recv() {
            assert_eq!(event.job, id);
            seen.push(event);
        }
        assert_eq!(
            kinds(&seen),
            [
                "started", "step", "progress", "item", "item", "note", "finished"
            ]
        );
        assert!(seen.windows(2).all(|w| w[0].seq < w[1].seq));
        match &seen[2].event {
            JobEvent::Progress { current, total, .. } => {
                assert_eq!(current.as_deref(), Some("https://tampa.gov/a"));
                assert_eq!(*total, 2);
            }
            other => panic!("expected progress, got {other:?}"),
        }
    }

    /// A page opened halfway through a run reads where the run is — its stage, its count,
    /// the item in hand, its latest lines — rather than an empty log that fills from now.
    #[tokio::test]
    async fn a_late_subscriber_gets_the_current_state_and_the_recent_tail() {
        let jobs = Jobs::default();
        let job = jobs.start("run", "schedule");
        let progress = job.watch(Progress::none());
        progress.track(TOTAL_TRACK, "tampa.gov · collect", 1, 9, Unit::Count);
        for i in 0..(TAIL as u64 + 50) {
            let url = format!("https://tampa.gov/{i}");
            progress.step_on("stored", i, 1005, url.clone());
            progress.item(page(&url, Verdict::Ok));
        }

        let (snapshot, _rx) = jobs.subscribe();
        let state = &snapshot[0];
        assert_eq!(state.id, job.id());
        assert_eq!(state.step.as_deref(), Some("tampa.gov · collect"));
        assert_eq!(
            (state.done, state.total),
            (Some(TAIL as u64 + 49), Some(1005))
        );
        assert_eq!(state.current.as_deref(), Some("https://tampa.gov/249"));
        assert_eq!(state.ok, TAIL as u64 + 50);
        assert!(state.outcome.is_none());

        // The tail is bounded, holds no counter ticks, and ends on the latest item.
        assert_eq!(state.log.len(), TAIL);
        assert!(
            state
                .log
                .iter()
                .all(|e| !matches!(e.event, JobEvent::Progress { .. }))
        );
        match &state.log.back().unwrap().event {
            JobEvent::Item { item } => assert_eq!(item.address, "https://tampa.gov/249"),
            other => panic!("expected the latest item, got {other:?}"),
        }
    }

    /// A finished job leaves the active list but stays readable, and how it ended is said
    /// apart: a cancellation is not a failure.
    #[tokio::test]
    async fn finished_jobs_say_how_they_ended() {
        let jobs = Jobs::default();
        let cancelled = jobs.start("run", "schedule");
        let failed = jobs.start_as("run-abc", "classify", "web");
        let abandoned = jobs.start("embed", "cli");
        let running = jobs.start("collect", "cli");

        cancelled.finish::<()>(&Err(crate::op::Cancelled.into()));
        failed.finish::<()>(&Err(anyhow::anyhow!("Jev is down")));
        drop(abandoned);

        let snapshot = jobs.snapshot();
        assert_eq!(snapshot[0].id, running.id());
        assert!(snapshot[0].outcome.is_none());
        let ended: Vec<_> = snapshot[1..]
            .iter()
            .map(|j| (j.id.as_str(), j.outcome))
            .collect();
        assert_eq!(
            ended,
            [
                ("embed-2", Some(Outcome::Failed)),
                ("run-abc", Some(Outcome::Failed)),
                ("run-1", Some(Outcome::Cancelled)),
            ]
        );
        assert_eq!(snapshot[2].error.as_deref(), Some("Jev is down"));
    }

    /// A job started as stoppable is stopped by its id, which trips the token the op
    /// polls; a job that was not, or has finished, cannot be — and says so by its answer
    /// rather than by doing nothing.
    #[tokio::test]
    async fn a_stoppable_job_is_stopped_by_id_and_nothing_else_is() {
        let jobs = Jobs::default();
        let (_, mut rx) = jobs.subscribe();
        let (job, cancel) = jobs.start_stoppable("2026-10-13T00:00:00Z", "run", "tampa");
        let plain = jobs.start("embed", "cli");

        assert!(jobs.snapshot()[0].stoppable);
        assert!(!jobs.snapshot()[1].stoppable);
        assert!(!cancel.is_cancelled());
        assert!(!jobs.stop(plain.id()), "a plain job has no stop handle");
        assert!(!jobs.stop("nobody"));

        assert!(jobs.stop(job.id()));
        assert!(cancel.is_cancelled(), "the op's token must trip");
        assert!(cancel.check().is_err());

        job.finish::<()>(&Err(crate::op::Cancelled.into()));
        assert!(!jobs.stop("2026-10-13T00:00:00Z"), "a finished job cannot be stopped");
        let started = rx.try_recv().unwrap();
        assert!(
            matches!(started.event, JobEvent::Started { stoppable: true, .. }),
            "{started:?}"
        );
    }

    /// The shape on the wire is what the web client reads. `type` names the event and the
    /// envelope's fields sit beside the event's own.
    #[test]
    fn a_stamped_event_serializes_flat() {
        let event = Stamped {
            seq: 7,
            at: 1_760_000_000_000,
            job: "run-1".into(),
            event: JobEvent::Step {
                step: "embed".into(),
            },
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({ "seq": 7, "at": 1_760_000_000_000i64, "job": "run-1", "type": "step", "step": "embed" })
        );
    }
}
