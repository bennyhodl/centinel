//! Jobs: what every process on a store is working on, as it happens, for whoever is
//! watching.
//!
//! An op reports into [`Progress`] and never learns who listens. The CLI writes that
//! stream to its log and `/ops/{name}/stream` forwards it to the one caller that asked;
//! neither outlives the invocation, and a browser opened after a scheduled run started
//! has no way in. This module is the listener that does: every long-running invocation is
//! a *job*, its events are folded into a small current state and a bounded tail, and any
//! number of subscribers can join at any point and read the same thing.
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
//! job filters on it. Each event is stamped with its writer's clock and a sequence number
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
//! ## On disk, for every process on the store
//!
//! A `centinel run` typed in a terminal is another process from the server serving the
//! page, and the page should show it all the same — the reason `run.lock` is a file. So a
//! board made with [`Jobs::on_disk`] keeps each of its jobs in the store's `jobs/`, one
//! file per job holding the job's [`JobState`] as it stands, and twice a second
//! ([`LOOK_EVERY`]) reads every other board's files into its own. Nothing downstream — the
//! event stream, the web page — knows which process runs a job.
//!
//! **Another board's job here is a copy of its file.** The file is the state; the events
//! this board sends for it are a projection for subscribers: the tail entries it has not
//! sent yet, and one `progress` when the count moved. Its `ok` and `failed` are taken from
//! the file, because the writer counted every item and a tail that rolls over between two
//! looks — more than [`TAIL`] log lines in half a second — holds only the last of them. A
//! page that followed such a burst counts fewer until its next snapshot. Reading a file
//! again changes nothing, so a look that fails loses nothing.
//!
//! The one thing a reader cannot do is trip another process's token, so stopping one is a
//! request: [`Jobs::stop`] leaves a file beside the job's, and the process running it
//! takes the request on its next look and stops the job itself. A process that dies
//! leaves a job with no ending; a reader that finds its writer gone finishes it as failed,
//! the same rule a dropped [`Job`] follows. Files are removed by one rule every board
//! applies alike, so a restarted server still lists what finished before it.
//!
//! The reader starts with the first job a board runs or the first caller to watch it, so a
//! command that does neither never touches the store. On a platform without a liveness
//! check (not unix) a writer is never known to be gone: a job it left unfinished stays
//! running on every board and its file is never removed. Finished jobs are pruned there as
//! anywhere.
//!
//! This is still a window, not a record. What a run did is the journal and the log.

mod shelf;

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once, Weak};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::op::{Cancel, Canceller, ItemOutcome, Progress, ProgressEvent, TOTAL_TRACK};
use shelf::{Seen, Shelf, Shelved};

/// How often a board on disk writes its jobs that have moved, reads every other board's,
/// and takes the stop requests made of its own.
const LOOK_EVERY: Duration = Duration::from_millis(500);

/// Events kept per job for a subscriber that arrives late.
///
/// Also what bounds a job's file on disk, which is rewritten at most once a [`LOOK_EVERY`]
/// while the job moves: raising it costs every reader that much more each look.
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
    /// Grows by one per event across every job on the board that sent it.
    pub seq: u64,
    /// Milliseconds since the Unix epoch, on the clock of the process that ran the job.
    pub at: i64,
    pub job: String,
    /// The part of the job this came from, when the job does several at once: a source
    /// in a `run`. See [`crate::op::Progress::part`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
    #[serde(flatten)]
    pub event: JobEvent,
}

/// One part of a job that does several at once, as it stands: a source in a `run`, with
/// its own stage, its own count and the item it has in hand.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct JobPart {
    pub name: String,
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
    pub ok: u64,
    pub failed: u64,
}

impl JobPart {
    /// Folds one of this part's events in, as [`JobState::apply`] folds a job's.
    fn apply(&mut self, event: &JobEvent) {
        match event {
            JobEvent::Step { step } => {
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
            JobEvent::Item { item } => match item.succeeded() {
                true => self.ok += 1,
                false => self.failed += 1,
            },
            JobEvent::Started { .. } | JobEvent::Note { .. } | JobEvent::Finished { .. } => {}
        }
    }
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
    /// The parts of a job that does several at once, in the order each first spoke. Empty
    /// for a job that is one piece of work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<JobPart>,
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
            parts: Vec::new(),
            log: VecDeque::new(),
        }
    }

    /// Folds one event in. The web client folds the same events the same way, in
    /// `web/src/job-state.ts`; the two must agree.
    ///
    /// An event from a part moves that part: its stage and its count are its own, and the
    /// job's are left to the stages the job runs as one. Its items still count toward the
    /// job, which counts every item whoever finished it.
    fn apply(&mut self, event: &Stamped) {
        self.seq = event.seq;
        if let Some(name) = &event.part {
            let at = match self.parts.iter().position(|part| &part.name == name) {
                Some(at) => at,
                None => {
                    self.parts.push(JobPart {
                        name: name.clone(),
                        ..Default::default()
                    });
                    self.parts.len() - 1
                }
            };
            self.parts[at].apply(&event.event);
        }
        match &event.event {
            JobEvent::Started { .. } | JobEvent::Note { .. } => {}
            JobEvent::Step { .. } | JobEvent::Progress { .. } if event.part.is_some() => {}
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

/// The id [`Jobs::start`] gives a job: the kind, the instant, this process, and a count
/// in it — so neither two boards in one process nor a process that inherits a dead one's
/// pid can mint one already in use.
fn fresh_id(kind: &str) -> String {
    static MINTED: AtomicU64 = AtomicU64::new(0);
    format!(
        "{kind}-{}-{}-{}",
        jiff::Timestamp::now().as_nanosecond(),
        std::process::id(),
        MINTED.fetch_add(1, Ordering::Relaxed) + 1
    )
}

/// Every job on the store, as one process sees it, and the channel their events go out on.
///
/// Cheap to clone; every clone is the same hub. Recording an event takes a short lock and
/// a channel send that never waits, so a stage reporting into it is not slowed by who is
/// watching or by nobody watching. The disk is written after the lock is released, when a
/// job starts or ends; in between, the next look writes what changed.
#[derive(Clone, Debug)]
pub struct Jobs {
    inner: Arc<Hub>,
}

#[derive(Debug)]
struct Hub {
    tx: broadcast::Sender<Stamped>,
    board: Mutex<Board>,
    /// Where this board's jobs are written and every other board's are read.
    shelf: Shelf,
    /// This board's name on the shelf, so it knows its own files.
    name: String,
    /// Held for each write to the shelf, which reads the job's state inside it: whichever
    /// write comes last is of the latest state.
    pen: Mutex<()>,
    /// The reader, started by the first job this board runs or the first caller to watch
    /// it, so a process that does neither never touches the shelf.
    reading: Once,
}

#[derive(Debug, Default)]
struct Board {
    seq: u64,
    /// In the order this board learned of them.
    active: Vec<JobState>,
    /// Newest ending first; see [`Board::finish`].
    finished: VecDeque<JobState>,
    /// The stop handle of each active job that has one, by id. Dropped as the job
    /// finishes: a stop asked of a finished job is nothing.
    stoppers: BTreeMap<String, Canceller>,
    /// The version — the job's `seq` — of each of this board's own jobs that the shelf
    /// last took. A job is owed while it stands at a later one, so a write acknowledges
    /// only what it wrote: an older write finishing late cannot clear a newer state, and
    /// one the store refused stays owed without anything to put back.
    written: BTreeMap<String, u64>,
    /// This board's finished jobs that fell off [`Board::finished`] while still owed: what
    /// the list shows and what the shelf is still owed are two questions, and the second
    /// must not be answered by the first. Empty unless the store is refusing writes, when
    /// it holds every job finished since, each retried — and warned of — every look.
    unwritten: BTreeMap<String, JobState>,
    /// Other boards' jobs, by id: whose they are, and the last of their events folded here.
    foreign: BTreeMap<String, Foreign>,
}

#[derive(Debug)]
struct Foreign {
    pid: u32,
    /// The writer's `seq` as of the last read: its tail past this is news.
    seen: u64,
}

impl Board {
    fn snapshot(&self) -> Vec<JobState> {
        self.active
            .iter()
            .chain(self.finished.iter())
            .cloned()
            .collect()
    }

    fn find(&self, id: &str) -> Option<&JobState> {
        self.active
            .iter()
            .chain(self.finished.iter())
            .find(|j| j.id == id)
    }

    /// Whether a job is this board's own, to write to the shelf.
    fn is_mine(&self, id: &str) -> bool {
        !self.foreign.contains_key(id)
    }

    fn is_owed(&self, job: &JobState) -> bool {
        self.is_mine(&job.id) && job.seq > self.written.get(&job.id).copied().unwrap_or(0)
    }

    /// This board's jobs the shelf does not yet have as they stand.
    fn owed(&self) -> Vec<String> {
        self.active
            .iter()
            .chain(self.finished.iter())
            .chain(self.unwritten.values())
            .filter(|job| self.is_owed(job))
            .map(|job| job.id.clone())
            .collect()
    }

    /// One of this board's jobs as it stands: on the board, or past it and still owed.
    fn latest(&self, id: &str) -> Option<&JobState> {
        self.find(id).or_else(|| self.unwritten.get(id))
    }

    /// The shelf has taken `version` of a job.
    fn wrote(&mut self, id: &str, version: u64) {
        let written = self.written.entry(id.to_string()).or_default();
        *written = (*written).max(version);
        if self.unwritten.get(id).is_some_and(|job| job.seq <= version) {
            self.unwritten.remove(id);
            self.written.remove(id);
        }
    }

    fn find_mut(&mut self, id: &str) -> Option<&mut JobState> {
        self.active
            .iter_mut()
            .chain(self.finished.iter_mut())
            .find(|j| j.id == id)
    }

    /// Stamps and folds one event. `None` for an event about a job that has already
    /// finished: a task the op left behind is not news about the job.
    fn fold(
        &mut self,
        job: &str,
        at: i64,
        part: Option<String>,
        event: JobEvent,
    ) -> Option<Stamped> {
        if !matches!(event, JobEvent::Started { .. }) && !self.active.iter().any(|j| j.id == job) {
            return None;
        }
        self.seq += 1;
        let stamped = Stamped {
            seq: self.seq,
            at,
            job: job.to_string(),
            part,
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
            self.active.push(state);
            return Some(stamped);
        }
        let at = self
            .active
            .iter()
            .position(|j| j.id == job)
            .expect("checked above");
        self.active[at].apply(&stamped);
        if matches!(stamped.event, JobEvent::Finished { .. }) {
            let done = self.active.remove(at);
            self.stoppers.remove(&done.id);
            self.finish(done);
        }
        Some(stamped)
    }

    /// Files a finished job by when it ended, newest first, and keeps the last
    /// [`FINISHED`] — the order and the cut `web/src/job-state.ts` makes, so a page and a
    /// snapshot keep the same jobs. Two that ended in the same millisecond go by which
    /// this board heard of last.
    fn finish(&mut self, done: JobState) {
        let key = |j: &JobState| (std::cmp::Reverse(j.finished_at), std::cmp::Reverse(j.seq));
        let at = self.finished.partition_point(|j| key(j) < key(&done));
        self.finished.insert(at, done);
        while self.finished.len() > FINISHED {
            if let Some(gone) = self.finished.pop_back() {
                if self.is_owed(&gone) {
                    self.unwritten.insert(gone.id.clone(), gone);
                } else {
                    self.written.remove(&gone.id);
                    self.foreign.remove(&gone.id);
                }
            }
        }
    }
}

impl Jobs {
    /// A board that writes its jobs into `dir` and reads every other board's from it. See
    /// the module's "On disk" section.
    pub fn on_disk(dir: PathBuf) -> Self {
        Self {
            inner: Arc::new(Hub {
                tx: broadcast::channel(CAPACITY).0,
                board: Mutex::default(),
                shelf: Shelf::new(dir),
                name: fresh_id("board"),
                pen: Mutex::default(),
                reading: Once::new(),
            }),
        }
    }

    /// Starts a job with an id of its own ([`fresh_id`]).
    pub fn start(&self, kind: &str, label: impl Into<String>) -> Job {
        self.start_as(fresh_id(kind), kind, label)
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
        (
            self.begin(id.into(), kind, label.into(), Some(canceller)),
            cancel,
        )
    }

    /// Asks a running job to stop at its next item boundary. False when no active job
    /// has this id, or the one that does was not started as stoppable.
    ///
    /// Another process's job is asked through the store, and stops when that process next
    /// looks — within [`LOOK_EVERY`] of its next item boundary.
    pub fn stop(&self, id: &str) -> bool {
        let board = self.board();
        if let Some(canceller) = board.stoppers.get(id) {
            canceller.cancel();
            return true;
        }
        let elsewhere = board.foreign.contains_key(id)
            && board.active.iter().any(|j| j.id == id && j.stoppable);
        drop(board);
        elsewhere && self.inner.shelf.ask_to_stop(id)
    }

    fn begin(&self, id: String, kind: &str, label: String, stopper: Option<Canceller>) -> Job {
        self.read_the_shelf();
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

    /// Every active job in the order this board learned of it, then the recently
    /// finished, newest first.
    pub fn snapshot(&self) -> Vec<JobState> {
        self.read_the_shelf();
        self.board().snapshot()
    }

    /// The snapshot and a receiver for everything after it, taken together: no event can
    /// land between the two, so a subscriber neither misses one nor sees one twice.
    pub fn subscribe(&self) -> (Vec<JobState>, broadcast::Receiver<Stamped>) {
        self.read_the_shelf();
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

    /// Starts the reader behind a board on disk, once.
    ///
    /// The first look happens here, before the caller goes on, so the first snapshot a
    /// restarted server gives already holds what every other process left on the shelf.
    fn read_the_shelf(&self) {
        self.inner.reading.call_once(|| {
            let mut seen = BTreeMap::new();
            self.look(&mut seen);
            let hub = Arc::downgrade(&self.inner);
            if let Err(error) = std::thread::Builder::new()
                .name("jobs".into())
                .spawn(move || watch(hub, seen))
            {
                tracing::warn!(%error, "could not start reading other processes' jobs");
            }
        });
    }

    /// Stamps, folds and sends one event, and marks one of this board's own jobs as owed
    /// to the shelf: written at once when it starts or ends, at the next look otherwise.
    fn publish(&self, job: &str, event: JobEvent) {
        self.publish_at(job, jiff::Timestamp::now().as_millisecond(), None, event);
    }

    /// [`Jobs::publish`], at a time the caller knows better than the clock: when another
    /// process's job last moved, for one that board ends on its behalf.
    fn publish_at(&self, job: &str, at: i64, part: Option<String>, event: JobEvent) {
        let now = matches!(event, JobEvent::Started { .. } | JobEvent::Finished { .. });
        let mine = {
            let mut board = self.board();
            let Some(stamped) = board.fold(job, at, part, event) else {
                return;
            };
            // The fold moved the job to a later version, which is what makes it owed.
            let mine = board.is_mine(job);
            // Sent under the lock, which is what lets `subscribe` promise no gap. No
            // receiver is the ordinary case, not an error.
            let _ = self.inner.tx.send(stamped);
            mine
        };
        if mine && now {
            self.write(job);
        }
    }

    /// Writes one of this board's jobs as it stands now, and acknowledges that version.
    /// A store that refused it leaves it owed for the next look; a change made while the
    /// write is in flight is a later version, and owed in turn.
    fn write(&self, id: &str) {
        let shelf = &self.inner.shelf;
        let _pen = self
            .inner
            .pen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Still owed, checked under the pen: a write the look listed may already have
        // been made by the job's own ending, and writing it again could only acknowledge
        // a job this board has since let go of.
        let state = {
            let board = self.board();
            board.latest(id).filter(|job| board.is_owed(job)).cloned()
        };
        let Some(state) = state else { return };
        let version = state.seq;
        let written = shelf.write(&Shelved {
            pid: std::process::id(),
            board: self.inner.name.clone(),
            state,
        });
        if written {
            self.board().wrote(id, version);
        }
    }

    /// One look at the store: takes the stop requests made of this board's jobs, writes
    /// the ones that have moved, folds in every other board's, ends the ones whose writer
    /// is gone, and prunes.
    fn look(&self, seen: &mut BTreeMap<PathBuf, Seen>) {
        let shelf = &self.inner.shelf;
        let stoppable: Vec<String> = self.board().stoppers.keys().cloned().collect();
        for id in stoppable {
            if shelf.take_stop_request(&id) {
                self.stop(&id);
            }
        }
        let owed = self.board().owed();
        for id in owed {
            self.write(&id);
        }

        let Some(mut look) = shelf.look(seen) else {
            return;
        };
        for shelved in std::mem::take(&mut look.changed) {
            if shelved.board != self.inner.name {
                self.adopt(shelved);
            }
        }
        let now = jiff::Timestamp::now().as_millisecond();
        let ended: Vec<(String, i64, String)> = {
            let board = self.board();
            board
                .active
                .iter()
                .filter_map(|job| {
                    let foreign = board.foreign.get(&job.id)?;
                    if !look.present.contains(&job.id) {
                        Some((now, "its record was removed from the store".to_string()))
                    } else if let Some((_, at)) = look.orphaned.iter().find(|(id, _)| *id == job.id)
                    {
                        Some((
                            *at,
                            format!("process {} exited without saying how it ended", foreign.pid),
                        ))
                    } else {
                        None
                    }
                    .map(|(at, why)| (job.id.clone(), at, why))
                })
                .collect()
        };
        for (id, at, why) in ended {
            self.publish_at(
                &id,
                at,
                None,
                JobEvent::Finished {
                    outcome: Outcome::Failed,
                    error: Some(why),
                },
            );
        }
        shelf.prune(seen, &look);
    }

    /// Takes another board's job as its file has it: the events this board has not yet
    /// sent go out, and the job here becomes the writer's copy.
    fn adopt(&self, shelved: Shelved) {
        let Shelved {
            pid, state: theirs, ..
        } = shelved;
        let id = theirs.id.clone();
        let mut board = self.board();
        let known = board.find(&id).is_some();
        if known && !board.foreign.contains_key(&id) {
            // This board's own job, under a file another board wrote: not news.
            return;
        }
        let seen = board.foreign.get(&id).map(|f| f.seen);
        let news = |e: &&Stamped| seen.is_none_or(|s| e.seq > s);

        let mut events: Vec<(i64, Option<String>, JobEvent)> = Vec::new();
        let starts_in_tail = theirs
            .log
            .front()
            .is_some_and(|e| matches!(e.event, JobEvent::Started { .. }));
        if !known && !starts_in_tail {
            // Older than its tail: say it started, then what the tail still holds.
            events.push((
                theirs.started_at,
                None,
                JobEvent::Started {
                    kind: theirs.kind.clone(),
                    label: theirs.label.clone(),
                    stoppable: theirs.stoppable,
                },
            ));
        }
        let mut ending = None;
        for e in theirs.log.iter().filter(news) {
            match e.event {
                JobEvent::Finished { .. } => ending = Some((e.at, e.event.clone())),
                _ => events.push((e.at, e.part.clone(), e.event.clone())),
            }
        }
        board.foreign.insert(
            id.clone(),
            Foreign {
                pid,
                seen: theirs.seq,
            },
        );
        for (at, part, event) in events {
            if let Some(stamped) = board.fold(&id, at, part, event) {
                let _ = self.inner.tx.send(stamped);
            }
        }
        // The stage and the count inside it, which a rolled tail may no longer carry.
        let latest = theirs.log.back().map_or(theirs.started_at, |e| e.at);
        if let Some(step) = &theirs.step
            && board
                .active
                .iter()
                .any(|ours| ours.id == id && ours.step.as_ref() != Some(step))
            && let Some(stamped) =
                board.fold(&id, latest, None, JobEvent::Step { step: step.clone() })
        {
            let _ = self.inner.tx.send(stamped);
        }
        let progress = match (&theirs.message, theirs.done, theirs.total) {
            (Some(message), Some(done), Some(total)) => Some(JobEvent::Progress {
                message: message.clone(),
                done,
                total,
                current: theirs.current.clone(),
            }),
            _ => None,
        };
        if let Some(progress) = progress
            && board.active.iter().any(|ours| {
                ours.id == id
                    && (
                        ours.message.as_ref(),
                        ours.done,
                        ours.total,
                        ours.current.as_ref(),
                    ) != (
                        theirs.message.as_ref(),
                        theirs.done,
                        theirs.total,
                        theirs.current.as_ref(),
                    )
            })
            && let Some(stamped) = board.fold(&id, latest, None, progress)
        {
            let _ = self.inner.tx.send(stamped);
        }
        // The same for each part: its stage came with the tail, its count did not.
        for part in &theirs.parts {
            let (Some(message), Some(done), Some(total)) = (&part.message, part.done, part.total)
            else {
                continue;
            };
            let moved = board
                .find(&id)
                .is_some_and(|ours| !ours.parts.contains(part));
            let progress = JobEvent::Progress {
                message: message.clone(),
                done,
                total,
                current: part.current.clone(),
            };
            if moved
                && let Some(stamped) = board.fold(&id, latest, Some(part.name.clone()), progress)
            {
                let _ = self.inner.tx.send(stamped);
            }
        }
        if let Some((at, event)) = ending
            && let Some(stamped) = board.fold(&id, at, None, event)
        {
            let _ = self.inner.tx.send(stamped);
        }
        // The file is the state. The events above told subscribers what moved; what the
        // job stands at is the writer's, which counted every item where a tail that rolled
        // over between two looks holds only the last of them.
        if let Some(ours) = board.find_mut(&id) {
            ours.ok = theirs.ok;
            ours.failed = theirs.failed;
            ours.parts = theirs.parts;
            ours.step = theirs.step;
            ours.message = theirs.message;
            ours.done = theirs.done;
            ours.total = theirs.total;
            ours.current = theirs.current;
        }
    }
}

/// The reader behind [`Jobs::on_disk`], after its first look: one every [`LOOK_EVERY`] for
/// as long as the board exists. On a thread of its own rather than a task, so a board
/// needs no runtime to be made and keeps reading whichever runtime its jobs run on.
fn watch(hub: Weak<Hub>, mut seen: BTreeMap<PathBuf, Seen>) {
    loop {
        std::thread::sleep(LOOK_EVERY);
        let Some(inner) = hub.upgrade() else { return };
        Jobs { inner }.look(&mut seen);
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
        self.jobs.publish_at(
            &self.id,
            jiff::Timestamp::now().as_millisecond(),
            event.part.clone(),
            JobEvent::from(event),
        );
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

    /// A run over several sources is followed source by source. What a part says moves
    /// that part and leaves the job's own stage and count alone, every item still counts
    /// toward the job, and each line of the log says which part it came from.
    #[tokio::test]
    async fn a_job_in_parts_keeps_each_parts_stage_and_count() {
        let dir = tempfile::tempdir().unwrap();
        let jobs = Jobs::on_disk(dir.path().to_path_buf());
        let job = jobs.start("run", "tampa, orlando");
        let progress = job.watch(Progress::none());
        let (tampa, orlando) = (progress.part("tampa"), progress.part("orlando"));

        tampa.track(TOTAL_TRACK, "tampa · collect", 0, 6, Unit::Count);
        orlando.track(TOTAL_TRACK, "orlando · discover", 1, 6, Unit::Count);
        tampa.step_on("1 stored, 0 failed", 1, 10, "https://tampa.gov/a");
        tampa.item(page("https://tampa.gov/a", Verdict::Ok));
        orlando.item(page("https://orlando.gov/x", Verdict::Fail));

        let (states, _) = jobs.subscribe();
        let state = &states[0];
        assert_eq!(
            (state.step.as_deref(), state.done, state.total),
            (None, None, None)
        );
        assert_eq!((state.ok, state.failed), (1, 1));
        assert_eq!(
            state.parts,
            [
                JobPart {
                    name: "tampa".into(),
                    step: Some("tampa · collect".into()),
                    message: Some("1 stored, 0 failed".into()),
                    done: Some(1),
                    total: Some(10),
                    current: Some("https://tampa.gov/a".into()),
                    ok: 1,
                    failed: 0,
                },
                JobPart {
                    name: "orlando".into(),
                    step: Some("orlando · discover".into()),
                    failed: 1,
                    ..Default::default()
                },
            ]
        );
        let said_by: Vec<Option<&str>> = state.log.iter().map(|e| e.part.as_deref()).collect();
        assert_eq!(
            said_by,
            [
                None,
                Some("tampa"),
                Some("orlando"),
                Some("tampa"),
                Some("orlando")
            ]
        );
        job.finish(&anyhow::Ok(()));
    }

    /// What an op says into its `Progress` reaches a subscriber as the job's events, in
    /// the order it was said, each stamped later than the one before.
    #[tokio::test]
    async fn a_subscriber_receives_a_jobs_events_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let jobs = Jobs::on_disk(dir.path().to_path_buf());
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
        let dir = tempfile::tempdir().unwrap();
        let jobs = Jobs::on_disk(dir.path().to_path_buf());
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
        let dir = tempfile::tempdir().unwrap();
        let jobs = Jobs::on_disk(dir.path().to_path_buf());
        let cancelled = jobs.start("run", "schedule");
        let failed = jobs.start_as("run-abc", "classify", "web");
        let abandoned = jobs.start("embed", "cli");
        let running = jobs.start("collect", "cli");
        let (cancelled_id, abandoned_id) = (cancelled.id().to_string(), abandoned.id().to_string());
        assert_ne!(cancelled_id, abandoned_id);

        cancelled.finish::<()>(&Err(crate::op::Cancelled.into()));
        failed.finish::<()>(&Err(anyhow::anyhow!("Jev is down")));
        drop(abandoned);

        let snapshot = jobs.snapshot();
        assert_eq!(snapshot[0].id, running.id());
        assert!(snapshot[0].outcome.is_none());
        let ended: Vec<_> = snapshot[1..]
            .iter()
            .map(|j| (j.id.clone(), j.outcome))
            .collect();
        assert_eq!(
            ended,
            [
                (abandoned_id, Some(Outcome::Failed)),
                ("run-abc".into(), Some(Outcome::Failed)),
                (cancelled_id, Some(Outcome::Cancelled)),
            ]
        );
        assert_eq!(snapshot[2].error.as_deref(), Some("Jev is down"));
    }

    /// A job started as stoppable is stopped by its id, which trips the token the op
    /// polls; a job that was not, or has finished, cannot be — and says so by its answer
    /// rather than by doing nothing.
    #[tokio::test]
    async fn a_stoppable_job_is_stopped_by_id_and_nothing_else_is() {
        let dir = tempfile::tempdir().unwrap();
        let jobs = Jobs::on_disk(dir.path().to_path_buf());
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
        assert!(
            !jobs.stop("2026-10-13T00:00:00Z"),
            "a finished job cannot be stopped"
        );
        let started = rx.try_recv().unwrap();
        assert!(
            matches!(
                started.event,
                JobEvent::Started {
                    stoppable: true,
                    ..
                }
            ),
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
            part: None,
            event: JobEvent::Step {
                step: "embed".into(),
            },
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({ "seq": 7, "at": 1_760_000_000_000i64, "job": "run-1", "type": "step", "step": "embed" })
        );
    }

    /// Waits for a board to read what it should, up to a few dozen looks.
    fn eventually(jobs: &Jobs, what: &str, ready: impl Fn(&[JobState]) -> bool) -> Vec<JobState> {
        for _ in 0..60 {
            let snapshot = jobs.snapshot();
            if ready(&snapshot) {
                return snapshot;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("{what}: {:#?}", jobs.snapshot());
    }

    fn now() -> i64 {
        jiff::Timestamp::now().as_millisecond()
    }

    /// A job as another process left it: started at `at`, `ok` items in, and finished at
    /// `finished` if it did.
    fn state(id: &str, at: i64, ok: u64, finished: Option<i64>) -> JobState {
        let mut seq = 0;
        let mut stamp = |at: i64, event: JobEvent| {
            seq += 1;
            Stamped {
                seq,
                at,
                job: id.into(),
                part: None,
                event,
            }
        };
        let started = stamp(
            at,
            JobEvent::Started {
                kind: "embed".into(),
                label: "cli".into(),
                stoppable: false,
            },
        );
        let mut job = JobState::new(&started, "embed", "cli", false);
        job.apply(&started);
        for n in 0..ok {
            let url = format!("https://tampa.gov/{n}");
            job.apply(&stamp(
                at,
                JobEvent::Item {
                    item: page(&url, Verdict::Ok),
                },
            ));
        }
        if let Some(finished) = finished {
            job.apply(&stamp(
                finished,
                JobEvent::Finished {
                    outcome: Outcome::Ok,
                    error: None,
                },
            ));
        }
        job
    }

    /// Writes `state` into `dir` the way the board of process `pid` would.
    fn leave(dir: &std::path::Path, pid: u32, state: JobState) {
        shelf::Shelf::new(dir.to_path_buf()).write(&shelf::Shelved {
            pid,
            board: format!("board-of-{pid}"),
            state,
        });
    }

    /// A process that has exited, for a writer that is gone.
    fn gone_pid() -> u32 {
        let mut gone = std::process::Command::new("true").spawn().unwrap();
        let pid = gone.id();
        gone.wait().unwrap();
        pid
    }

    /// The whole of the shared board, between two boards on one store — two processes, or
    /// two boards in one, which is the same thing to the store: a job one board runs shows
    /// on the other with its stage, counts, item in hand and start, keeps moving there, is
    /// stopped from there by asking, and ends there the way it ended.
    #[test]
    fn a_job_one_board_runs_is_watched_and_stopped_from_another() {
        let dir = tempfile::tempdir().unwrap();
        let runner = Jobs::on_disk(dir.path().to_path_buf());
        let watcher = Jobs::on_disk(dir.path().to_path_buf());
        let (_, mut rx) = watcher.subscribe();

        let (job, cancel) = runner.start_stoppable("2026-11-04T12:00:00Z", "run", "tampa");
        let progress = job.watch(Progress::none());
        progress.track(TOTAL_TRACK, "tampa.gov · collect", 1, 9, Unit::Count);
        progress.item(page("https://tampa.gov/a", Verdict::Ok));
        progress.step_on("1 stored", 1, 1005, "https://tampa.gov/b");

        let seen = eventually(&watcher, "the other board's job", |s| {
            s.first().is_some_and(|j| j.ok == 1 && j.done == Some(1))
        });
        let theirs = &runner.snapshot()[0];
        let ours = &seen[0];
        assert_eq!(
            (ours.id.as_str(), ours.kind.as_str(), ours.label.as_str()),
            ("2026-11-04T12:00:00Z", "run", "tampa")
        );
        assert_eq!(ours.step.as_deref(), Some("tampa.gov · collect"));
        assert_eq!(ours.current.as_deref(), Some("https://tampa.gov/b"));
        assert_eq!(ours.total, Some(1005));
        assert_eq!(ours.started_at, theirs.started_at, "the runner's clock");
        assert!(ours.stoppable && ours.outcome.is_none());
        let first = rx.try_recv().unwrap();
        assert!(matches!(first.event, JobEvent::Started { .. }), "{first:?}");

        progress.item(page("https://tampa.gov/b", Verdict::Fail));
        eventually(&watcher, "the next item", |s| s[0].failed == 1);

        assert!(!watcher.stop("nobody"));
        assert!(watcher.stop(&ours.id), "Stop reaches the other board");
        for _ in 0..60 {
            if cancel.is_cancelled() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(cancel.is_cancelled(), "the runner's own token trips");

        drop(progress);
        job.finish::<()>(&Err(crate::op::Cancelled.into()));
        let ended = eventually(&watcher, "the ending", |s| s[0].outcome.is_some());
        assert_eq!(ended[0].outcome, Some(Outcome::Cancelled));
        assert!(
            !watcher.stop(&ended[0].id),
            "a finished job cannot be stopped"
        );
    }

    /// A job never stays running on a board because its writer went quiet: one whose
    /// process is gone, and one whose file was removed, both end as failed, each saying
    /// why. One that said how it ended is not mistaken for either.
    #[test]
    fn a_job_whose_writer_is_gone_ends_failed() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        let alive = writer.id();
        leave(dir.path(), alive, state("orphan", now(), 3, None));
        leave(dir.path(), alive, state("removed", now(), 1, None));
        leave(dir.path(), gone_pid(), state("done", now(), 2, Some(now())));
        // Left an hour ago by a process that is already gone.
        leave(
            dir.path(),
            gone_pid(),
            state("crashed", now() - 3_700_000, 4, None),
        );
        let an_hour_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(dir.path().join("crashed.json"))
            .unwrap()
            .set_modified(an_hour_ago)
            .unwrap();

        let board = Jobs::on_disk(dir.path().to_path_buf());
        // No waiting: a board's first snapshot — a restarted server's — already holds what
        // is on the shelf, and a job whose process died before it looked is already over,
        // dated when it last moved rather than when this board noticed.
        let running = board.snapshot();
        assert_eq!(running.len(), 4, "{running:#?}");
        assert_eq!(running.iter().filter(|j| j.outcome.is_none()).count(), 2);
        let crashed = running.iter().find(|j| j.id == "crashed").unwrap();
        assert_eq!(crashed.outcome, Some(Outcome::Failed));
        let moved = an_hour_ago
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        assert_eq!(crashed.finished_at, Some(moved));
        assert_eq!(
            running.iter().find(|j| j.id == "orphan").unwrap().ok,
            3,
            "the writer's count"
        );

        std::fs::remove_file(dir.path().join("removed.json")).unwrap();
        writer.kill().unwrap();
        writer.wait().unwrap();
        let ended = eventually(&board, "all ended", |s| {
            s.iter().all(|j| j.outcome.is_some())
        });
        let why = |id: &str| {
            let job = ended.iter().find(|j| j.id == id).unwrap();
            (job.outcome, job.error.clone().unwrap_or_default())
        };
        assert_eq!(why("orphan").0, Some(Outcome::Failed));
        assert!(
            why("orphan").1.contains("exited without saying"),
            "{:?}",
            why("orphan")
        );
        assert_eq!(why("removed").0, Some(Outcome::Failed));
        assert!(why("removed").1.contains("removed"), "{:?}", why("removed"));
        assert_eq!(why("done"), (Some(Outcome::Ok), String::new()));
    }

    /// A file read twice is one job: the second read sends nothing and counts nothing
    /// again, which is what lets a look that failed simply be tried again.
    #[test]
    fn reading_a_file_again_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = state("again", now(), 3, None);
        leave(dir.path(), std::process::id(), elsewhere.clone());
        let board = Jobs::on_disk(dir.path().to_path_buf());
        eventually(&board, "the job", |s| s.first().is_some_and(|j| j.ok == 3));
        let (_, mut rx) = board.subscribe();

        // The same state, as a new file in its place.
        leave(dir.path(), std::process::id(), elsewhere);
        std::thread::sleep(LOOK_EVERY * 3);
        let snapshot = board.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!((snapshot[0].ok, snapshot[0].log.len()), (3, 4));
        assert!(rx.try_recv().is_err(), "nothing new was sent");
    }

    /// The finished jobs a board lists are the newest by when they ended, whatever order
    /// their files arrived in — the twenty the page keeps too — and the store keeps every
    /// file until it is both out of those twenty and past [`shelf::GRACE`], so no reader
    /// that is running misses an ending.
    #[test]
    fn the_newest_endings_are_kept_and_the_rest_removed_in_time() {
        let dir = tempfile::tempdir().unwrap();
        let pid = gone_pid();
        let recent = now() - 60_000;
        let old = now() - 3_600_000;
        // 22 recent and 3 old endings, written newest-last and oldest-in-the-middle.
        let mut endings: Vec<(String, i64)> = (0..22)
            .map(|n| (format!("recent-{n:02}"), recent + n * 1000))
            .collect();
        endings.insert(5, ("old-a".into(), old));
        endings.insert(11, ("old-b".into(), old - 1000));
        endings.insert(17, ("old-c".into(), old - 2000));
        for (id, at) in &endings {
            leave(dir.path(), pid, state(id, at - 500, 1, Some(*at)));
        }

        let board = Jobs::on_disk(dir.path().to_path_buf());
        let files = || std::fs::read_dir(dir.path()).unwrap().count();
        let seen = eventually(&board, "the newest twenty, the old files gone", |s| {
            s.len() == FINISHED && files() == 22
        });
        let kept: Vec<&str> = seen.iter().map(|j| j.id.as_str()).collect();
        let newest: Vec<String> = (2..22).rev().map(|n| format!("recent-{n:02}")).collect();
        assert_eq!(kept, newest, "newest ending first");
    }

    /// A job is owed to the store until the store has it. One that could not be written —
    /// the directory not a directory — is written once it can be, ending and all, and
    /// another board then reads how it ended.
    #[test]
    fn a_job_the_store_refused_is_written_when_it_can_be() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("jobs");
        std::fs::write(&dir, b"not a directory").unwrap();
        let runner = Jobs::on_disk(dir.clone());
        let job = runner.start("embed", "cli");
        let id = job.id().to_string();
        job.finish(&anyhow::Ok(()));

        std::fs::remove_file(&dir).unwrap();
        let watcher = Jobs::on_disk(dir);
        let seen = eventually(&watcher, "the ending, written late", |s| {
            s.iter().any(|j| j.id == id && j.outcome.is_some())
        });
        assert_eq!(seen[0].outcome, Some(Outcome::Ok));
    }

    /// A board that first reads a job long after it started — its tail now items only —
    /// still shows the stage it is in and how far through it, because the file is the
    /// state, not the tail.
    #[test]
    fn a_late_reader_takes_the_stage_from_the_file_not_the_tail() {
        let dir = tempfile::tempdir().unwrap();
        let runner = Jobs::on_disk(dir.path().to_path_buf());
        let job = runner.start("embed", "cli");
        let progress = job.watch(Progress::none());
        progress.track(TOTAL_TRACK, "embed", 1, 1, Unit::Count);
        for n in 0..(TAIL as u64 + 50) {
            let url = format!("https://tampa.gov/{n}");
            progress.step_on("embedding", n, 5000, url.clone());
            progress.item(page(&url, Verdict::Ok));
        }
        let theirs = runner.snapshot()[0].clone();
        assert!(
            theirs
                .log
                .iter()
                .all(|e| matches!(e.event, JobEvent::Item { .. })),
            "the tail has rolled past the start and the stage"
        );

        let watcher = Jobs::on_disk(dir.path().to_path_buf());
        let seen = eventually(&watcher, "the job, read late", |s| {
            s.iter().any(|j| j.id == theirs.id && j.done == theirs.done)
        });
        let ours = seen.iter().find(|j| j.id == theirs.id).unwrap();
        assert_eq!(ours.step.as_deref(), Some("embed"));
        assert_eq!((ours.done, ours.total), (theirs.done, Some(5000)));
        assert_eq!(ours.current, theirs.current);
        assert_eq!(
            ours.ok,
            TAIL as u64 + 50,
            "the writer's count, not the tail's"
        );
        drop(progress);
        job.finish(&anyhow::Ok(()));
    }

    /// A write acknowledges the version it wrote and nothing later. The write of a job's
    /// running state that lands after the job has ended — and fallen off the finished list
    /// behind twenty newer endings — leaves the ending owed, and only the ending's own
    /// write settles it.
    #[test]
    fn a_late_acknowledgement_of_an_older_write_leaves_the_ending_owed() {
        let mut board = Board::default();
        let started = |board: &mut Board, id: &str, at: i64| {
            board.fold(
                id,
                at,
                None,
                JobEvent::Started {
                    kind: "embed".into(),
                    label: "cli".into(),
                    stoppable: false,
                },
            );
        };
        let ended = |board: &mut Board, id: &str, at: i64| {
            board.fold(
                id,
                at,
                None,
                JobEvent::Finished {
                    outcome: Outcome::Ok,
                    error: None,
                },
            );
        };
        started(&mut board, "a", 1);
        let running = board.latest("a").unwrap().seq;
        ended(&mut board, "a", 2);
        for n in 0..FINISHED {
            let id = format!("later-{n}");
            started(&mut board, &id, 3);
            ended(&mut board, &id, 4);
        }
        assert!(board.find("a").is_none(), "off the finished list");

        board.wrote("a", running);
        assert!(
            board.owed().contains(&"a".to_string()),
            "the ending is still owed"
        );
        let ending = board.latest("a").unwrap().seq;
        board.wrote("a", ending);
        assert!(!board.owed().contains(&"a".to_string()));
        assert!(
            board.latest("a").is_none(),
            "nothing kept for it once written"
        );
    }
}
