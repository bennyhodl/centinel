//! The scheduler loop — the fourth surface, and the first nobody has to authenticate.
//!
//! `docs/SCHEDULING.md` §5, §9. Beside `http.rs` and `mcp.rs` because it is the same kind
//! of thing they are: something that drives the op registry and owns no domain logic. Every
//! question about *when* belongs to [`centinel_core::schedule`], which has no clock and is
//! therefore testable; this file is the part that sleeps.
//!
//! ```text
//!   due? ──▶ take run.lock ──▶ invoke `run` ──▶ append the attempt ──▶ release
//!      │           │
//!      │           └─ held ──▶ append a `skipped` attempt and carry on
//!      └─ no ──▶ sleep until the earliest next fire
//! ```
//!
//! ## The scheduler is not a consumer
//!
//! It fires ops whose `Reach` is `Operator` — the ones no HTTP or MCP caller can reach.
//! That is not a privilege this module holds; it is the operator's, delegated by a
//! `[[schedule]]` block in a file only they can write. Nothing here reads a request.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use centinel_core::config::{Config, ScheduleConfig};
use centinel_core::journal::{
    Attempt, Holder, Journal, Outcome, RunLock, Subtracted, Trigger, arithmetic,
};
use centinel_core::op::{self, Cancel, Canceller, Ctx};
use centinel_core::ops::RunReport;
use centinel_core::schedule::jitter_offset;
use jiff::{Timestamp, Unit};

/// How long the loop waits when nothing is due within the horizon.
///
/// Not a poll interval — every wake-up is computed from the next fire time. This only
/// bounds how long a `SIGHUP` reload can go unnoticed and how far a laptop's suspended
/// clock can drift before the loop re-derives its schedule.
const MAX_SLEEP_SECS: i64 = 60;

/// What `serve` says when it will not start.
const REFUSING: &str = "refusing to serve with a schedule that cannot run";

/// The scheduler, holding what it needs to fire.
#[derive(Debug)]
pub struct Scheduler {
    ctx: Arc<Ctx>,
    config: Config,
    config_path: Option<std::path::PathBuf>,
    /// Seeds the per-install jitter. The store root: the one string that identifies this
    /// corpus on this machine and survives a restart.
    node_seed: String,
    /// When each schedule last fired — **the instant every "is it due" question is asked
    /// relative to.**
    ///
    /// A schedule is due when its next occurrence *after its last fire* has passed. Asking
    /// for the next occurrence after *now* is the mistake this field exists to prevent:
    /// that answer is in the future by construction, so it is never `<= now` and the
    /// schedule never fires at all.
    ///
    /// Seeded from the journal at startup and updated as we fire, because the scheduler is
    /// the only writer and the alternative is re-reading the whole journal every wake-up.
    last_fire: HashMap<String, Timestamp>,
    /// The reference for a schedule that has never fired: this process's start.
    ///
    /// So a schedule added to a running server fires at its next occurrence rather than
    /// immediately — adding one is not a request to collect now.
    started: Timestamp,
}

/// One schedule that is due, and the instant it was due at.
struct Due<'a> {
    schedule: &'a ScheduleConfig,
    at: Timestamp,
    trigger: Trigger,
}

impl Scheduler {
    /// Builds a scheduler, refusing every schedule the operator got wrong.
    ///
    /// **Any failure here is fatal to `serve`.** A server that starts happily with a
    /// broken schedule collects nothing and says so nowhere; the operator finds out weeks
    /// later from an empty search result. Refusing is loud at the one moment it is cheap —
    /// while somebody is watching it start.
    pub fn new(ctx: Arc<Ctx>, config_path: Option<&str>) -> Result<Self> {
        // The context sits on the *load*, not on a second validation pass: reading a
        // config already validates it, so a `validate_schedules()` call here would be a
        // check that can never fire — and the operator would get the bare parse error with
        // no clue which command refused.
        let (config, path) = match config_path {
            Some(p) => {
                let path = std::path::PathBuf::from(p);
                let config = Config::from_file(&path).context(REFUSING)?;
                (config, Some(path))
            }
            None => (Config::load().context(REFUSING)?, Config::locate()),
        };

        let node_seed = ctx.store.root().display().to_string();
        Ok(Self {
            ctx,
            config,
            config_path: path,
            node_seed,
            last_fire: HashMap::new(),
            started: Timestamp::now(),
        })
    }

    /// Seeds [`Self::last_fire`] from the journal.
    ///
    /// Without this a restart would re-fire every schedule at its next occurrence even if
    /// one had just run — harmless for a subtraction-based pipeline, and still a crawl
    /// nobody asked for.
    async fn seed_last_fires(&mut self) {
        let journal = Journal::new(&self.ctx.store);
        for schedule in &self.config.schedules {
            if let Ok(Some(attempt)) = journal.last_for(&schedule.id).await
                && let Some(at) = attempt.started()
            {
                self.last_fire.insert(schedule.id.clone(), at);
            }
        }
    }

    /// The instant a schedule's next occurrence is measured from.
    fn reference(&self, schedule: &ScheduleConfig) -> Timestamp {
        self.last_fire
            .get(&schedule.id)
            .copied()
            .unwrap_or(self.started)
    }

    pub fn schedules(&self) -> &[ScheduleConfig] {
        &self.config.schedules
    }

    /// Re-reads the config, keeping the running one if the new one does not validate.
    ///
    /// The running configuration is known-good, and a typo must not disarm it. A restart
    /// is always correct and always sufficient; this exists so that `schedule set` against
    /// a live server is not one.
    pub fn reload(&mut self) -> Result<()> {
        let config = match &self.config_path {
            Some(path) => Config::from_file(path)?,
            None => Config::load()?,
        };
        config.validate_schedules()?;
        self.config = config;
        Ok(())
    }

    /// Runs until cancelled.
    pub async fn run(mut self, cancel: Cancel, mut reload: ReloadSignal) -> Result<()> {
        self.recover_interrupted().await;
        self.seed_last_fires().await;
        self.catch_up(&cancel).await;

        loop {
            if cancel.is_cancelled() {
                tracing::info!("scheduler stopping");
                return Ok(());
            }

            let now = Timestamp::now();
            let due: Vec<String> = self
                .due_at(now)
                .iter()
                .map(|d| d.schedule.id.clone())
                .collect();

            for id in due {
                // Re-resolved by id rather than held across the loop, because `fire` needs
                // `&mut self` to record the fire and a borrow of `self.config` would
                // outlive it.
                let Some(schedule) = self.config.schedule(&id).cloned() else {
                    continue;
                };
                self.fire(
                    &Due {
                        schedule: &schedule,
                        at: now,
                        trigger: Trigger::Schedule,
                    },
                    &cancel,
                )
                .await;
                if cancel.is_cancelled() {
                    return Ok(());
                }
            }

            // Every wake-up is computed, not polled. The cap only bounds how long a SIGHUP
            // can go unnoticed and how far a suspended laptop's clock can drift.
            let next = self
                .next_wake(Timestamp::now())
                .unwrap_or_else(|| Timestamp::now() + jiff::Span::new().seconds(MAX_SLEEP_SECS));
            self.sleep_until(next, &cancel, &mut reload).await;
        }
    }

    /// Turns a lock left by a dead process into an `interrupted` record.
    ///
    /// The attempt is appended at *finish*, so a killed run leaves no record — only the
    /// lock. Without this a crash is a gap in the journal, indistinguishable from a
    /// schedule that never fired.
    async fn recover_interrupted(&self) {
        let Some(holder) = RunLock::take_stale(&self.ctx.store) else {
            return;
        };
        tracing::warn!(
            pid = holder.pid,
            schedule = holder.schedule.as_deref().unwrap_or("-"),
            "reclaiming a lock left by a dead run"
        );
        let attempt = Attempt {
            run_id: holder.started_at.clone(),
            schedule: holder.schedule.clone(),
            trigger: holder.trigger,
            due_at: None,
            started_at: holder.started_at.clone(),
            finished_at: Timestamp::now().to_string(),
            outcome: Outcome::Interrupted,
            detail: Some(format!("process {} did not finish", holder.pid)),
            added: Default::default(),
            subtracted: Subtracted::default(),
            report: None,
        };
        if let Err(e) = Journal::new(&self.ctx.store).append(&attempt).await {
            tracing::warn!(error = %e, "could not record the interrupted run");
        }
    }

    /// Fires each schedule that is more than one interval overdue — **once**.
    ///
    /// Never a backlog. Six missed daily fires are one fire, because the pipeline is a
    /// subtraction and not a queue of deltas: six catch-up runs would find the same work,
    /// do it once, then do nothing five times, in a burst, against a city's web server.
    async fn catch_up(&mut self, cancel: &Cancel) {
        let now = Timestamp::now();

        // Decided in full before firing anything, so the loop below borrows nothing it
        // then needs to mutate — and so a schedule cannot be judged overdue against a
        // journal that this pass has already written to.
        let overdue: Vec<String> = self
            .config
            .schedules
            .iter()
            .filter(|s| s.is_enabled() && s.catches_up())
            .filter(|s| {
                let (Ok(cron), Ok(zone)) = (s.cron(), s.zone()) else {
                    return false;
                };
                let Some(interval) = cron.shortest_interval(now, &zone) else {
                    return false;
                };
                match self.last_fire.get(&s.id) {
                    Some(at) => {
                        let since = (now - *at).total(Unit::Second).unwrap_or(0.0);
                        since > interval.total(Unit::Second).unwrap_or(f64::MAX)
                    }
                    // Never fired. Not overdue: a schedule added a minute ago has not
                    // missed anything, and firing every new one at startup would turn
                    // adding a schedule into an immediate crawl nobody asked for.
                    None => false,
                }
            })
            .map(|s| s.id.clone())
            .collect();

        for id in overdue {
            let Some(schedule) = self.config.schedule(&id).cloned() else {
                continue;
            };
            tracing::info!(schedule = %id, "catching up a missed fire");
            self.fire(
                &Due {
                    schedule: &schedule,
                    at: now,
                    trigger: Trigger::CatchUp,
                },
                cancel,
            )
            .await;
            if cancel.is_cancelled() {
                return;
            }
        }
    }

    /// The earliest fire time across every enabled schedule.
    ///
    /// Measured from each schedule's own last fire, not from `now` — see
    /// [`Self::last_fire`]. `now` only bounds the answer: a schedule that is already
    /// overdue wakes the loop immediately rather than reporting a time in the past.
    fn next_wake(&self, now: Timestamp) -> Option<Timestamp> {
        self.config
            .schedules
            .iter()
            .filter(|s| s.is_enabled())
            .filter_map(|s| self.next_fire(s, self.reference(s)))
            .min()
            .map(|at| at.max(now))
    }

    /// One schedule's next fire, with jitter applied.
    ///
    /// Jitter is deterministic per install so `schedules` can print the real time. The
    /// offset is added *after* the cron time, so a `0 3 * * *` with five minutes of jitter
    /// fires somewhere in `03:00–03:05` and never before three.
    fn next_fire(&self, schedule: &ScheduleConfig, after: Timestamp) -> Option<Timestamp> {
        let cron = schedule.cron().ok()?;
        let zone = schedule.zone().ok()?;
        let offset = jitter_offset(&self.node_seed, &schedule.id, schedule.jitter_secs());
        // Searched from `after - offset`, so a fire whose jittered time is still ahead is
        // not skipped by having had its nominal time pass.
        let base = after - jiff::Span::new().seconds(offset as i64);
        Some(cron.next_after(base, &zone)? + jiff::Span::new().seconds(offset as i64))
    }

    /// Every schedule whose next occurrence *since it last fired* has passed.
    fn due_at(&self, now: Timestamp) -> Vec<Due<'_>> {
        self.config
            .schedules
            .iter()
            .filter(|s| s.is_enabled())
            .filter_map(|schedule| {
                let at = self.next_fire(schedule, self.reference(schedule))?;
                (at <= now).then_some(Due {
                    schedule,
                    at,
                    trigger: Trigger::Schedule,
                })
            })
            .collect()
    }

    /// Sleeps until an instant, waking early for a cancel or a reload.
    async fn sleep_until(&mut self, until: Timestamp, cancel: &Cancel, reload: &mut ReloadSignal) {
        let secs = (until - Timestamp::now())
            .total(Unit::Second)
            .unwrap_or(0.0)
            .clamp(0.0, MAX_SLEEP_SECS as f64);
        let nap = tokio::time::sleep(std::time::Duration::from_secs_f64(secs));

        tokio::select! {
            _ = nap => {}
            _ = cancel.cancelled() => {}
            _ = reload.recv() => {
                match self.reload() {
                    Ok(()) => tracing::info!(
                        schedules = self.config.schedules.len(),
                        "reloaded the config"
                    ),
                    // The running configuration is known-good; a typo must not disarm it.
                    Err(e) => tracing::error!(
                        error = %format!("{e:#}"),
                        "reload refused; keeping the running schedule"
                    ),
                }
            }
        }
    }

    /// Takes the lane and runs one schedule, recording the attempt either way.
    async fn fire(&mut self, due: &Due<'_>, cancel: &Cancel) {
        let started = Timestamp::now();
        // Recorded before the work, so a run that fails, hangs or is killed still moves
        // the schedule on. Otherwise one broken fire would be re-attempted every wake-up
        // for as long as it kept failing.
        self.last_fire.insert(due.schedule.id.clone(), started);
        let run_id = started.to_string();
        let args = match due.schedule.run_args() {
            Ok(args) => args,
            // Validated at startup, so reaching here means the config changed under a
            // reload that accepted it. Recorded rather than panicked.
            Err(e) => {
                self.record(Attempt {
                    run_id,
                    schedule: Some(due.schedule.id.clone()),
                    trigger: due.trigger,
                    due_at: Some(due.at.to_string()),
                    started_at: started.to_string(),
                    finished_at: Timestamp::now().to_string(),
                    outcome: Outcome::Failed,
                    detail: Some(format!("{e:#}")),
                    added: Default::default(),
                    subtracted: Subtracted::default(),
                    report: None,
                })
                .await;
                return;
            }
        };

        let holder = Holder {
            pid: std::process::id(),
            started_at: run_id.clone(),
            trigger: due.trigger,
            schedule: Some(due.schedule.id.clone()),
            args: one_line_args(&args),
        };

        // The lane. Held by another run — possibly a `centinel run` in another process —
        // means this fire is skipped, and **recorded as skipped**: "my cron never ran"
        // deserves an answer in the journal rather than a hunch.
        let lock = match RunLock::acquire(&self.ctx.store, &holder) {
            Ok(lock) => lock,
            Err(held) => {
                tracing::info!(schedule = %due.schedule.id, "skipping: {held}");
                self.record(Attempt {
                    run_id,
                    schedule: Some(due.schedule.id.clone()),
                    trigger: due.trigger,
                    due_at: Some(due.at.to_string()),
                    started_at: started.to_string(),
                    finished_at: Timestamp::now().to_string(),
                    outcome: Outcome::Skipped,
                    detail: Some(held.to_string()),
                    added: Default::default(),
                    subtracted: Subtracted::default(),
                    report: None,
                })
                .await;
                return;
            }
        };

        tracing::info!(schedule = %due.schedule.id, trigger = due.trigger.as_str(), "run started");

        let def = op::find("run").expect("`run` is always registered");
        let value = serde_json::to_value(&args).expect("RunArgs always serializes");
        let result = crate::logging::invoke_cancellable(
            "schedule",
            def,
            Arc::clone(&self.ctx),
            value,
            // No sink: a scheduled run has nobody watching a terminal, so its progress
            // goes to the log, which is the server's only way to speak.
            None,
            cancel.clone(),
        )
        .await;

        // Released before the record is written, so a slow append cannot hold the lane.
        drop(lock);

        let (outcome, detail, report) = match result {
            Ok(value) => match serde_json::from_value::<RunReport>(value) {
                Ok(report) => {
                    let outcome = if report.failed_stages > 0 {
                        Outcome::Partial
                    } else {
                        Outcome::Ok
                    };
                    (outcome, None, Some(report))
                }
                Err(e) => (
                    Outcome::Failed,
                    Some(format!("unreadable report: {e}")),
                    None,
                ),
            },
            // A cancellation is the operator stopping the process, not a broken source.
            // Filed as a failure it would count against the schedule and, over a week of
            // nightly restarts, bury a source that had genuinely stopped responding.
            Err(e) if op::is_cancelled(&e) => (Outcome::Interrupted, None, None),
            Err(e) => (Outcome::Failed, Some(format!("{e:#}")), None),
        };

        let (added, subtracted) = report
            .as_ref()
            .map(arithmetic)
            .unwrap_or_else(|| (Default::default(), Subtracted::default()));

        self.record(Attempt {
            run_id,
            schedule: Some(due.schedule.id.clone()),
            trigger: due.trigger,
            due_at: Some(due.at.to_string()),
            started_at: started.to_string(),
            finished_at: Timestamp::now().to_string(),
            outcome,
            detail,
            added,
            subtracted,
            report,
        })
        .await;
    }

    /// Appends an attempt, complaining loudly rather than failing the loop.
    ///
    /// A journal write that fails must not take the scheduler down: the next fire is worth
    /// more than the record of this one. It must also not be silent, or the corpus grows
    /// while its history quietly stops.
    async fn record(&self, attempt: Attempt) {
        tracing::info!(
            schedule = attempt.schedule.as_deref().unwrap_or("-"),
            outcome = attempt.outcome.as_str(),
            added = attempt.added.documents,
            "run finished"
        );
        if let Err(e) = Journal::new(&self.ctx.store).append(&attempt).await {
            tracing::error!(error = %format!("{e:#}"), "could not append to the run journal");
        }
    }
}

/// The arguments, for the lock file and for the log.
fn one_line_args(args: &centinel_core::ops::RunArgs) -> String {
    let mut parts = Vec::new();
    for source in &args.sources {
        parts.push(format!("--source {source}"));
    }
    for stage in &args.skip {
        parts.push(format!("--skip {}", stage.name()));
    }
    if let Some(limit) = args.limit {
        parts.push(format!("--limit {limit}"));
    }
    if args.refresh {
        parts.push("--refresh".into());
    }
    parts.join(" ")
}

/// A signal that the config should be re-read.
pub struct ReloadSignal(tokio::sync::mpsc::Receiver<()>);

impl ReloadSignal {
    pub fn channel() -> (tokio::sync::mpsc::Sender<()>, Self) {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        (tx, Self(rx))
    }

    async fn recv(&mut self) {
        // A closed channel must not resolve instantly forever — that would turn the
        // select below into a spin loop that never sleeps.
        if self.0.recv().await.is_none() {
            std::future::pending::<()>().await;
        }
    }
}

/// Starts the scheduler on **its own runtime and its own threads**.
///
/// `docs/SCHEDULING.md` §5.3. Not `tokio::spawn`: a spawned task shares the request path's
/// workers, and one stage that forgets `spawn_blocking` takes a worker for hours. Separate
/// runtimes make the read path's correctness independent of every stage's blocking
/// discipline instead of contingent on all of them staying right forever.
///
/// **Separation prevents starvation, not contention.** A request never waits on a worker
/// that will not yield for four hours; it can still be slower because the machine is
/// genuinely busy embedding. For that, these threads start at lower OS priority —
/// collection is throughput work and search is latency work, which is what a nice value is
/// for.
pub fn spawn(
    scheduler: Scheduler,
    reload: ReloadSignal,
) -> Result<(Canceller, std::thread::JoinHandle<()>)> {
    let (canceller, cancel) = Cancel::channel();

    // A quarter of the machine, and at least one. Collection is network-bound and spends
    // most of its wall clock asleep in a per-host pacer at one request per second; it does
    // not need the box to make its deadline.
    let workers = (std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        / 4)
    .max(1);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .thread_name("centinel-sched")
        .on_thread_start(lower_priority)
        .enable_all()
        .build()
        .context("building the scheduler runtime")?;

    tracing::info!(
        workers,
        schedules = scheduler.schedules().len(),
        "scheduler started on its own runtime"
    );

    let handle = std::thread::Builder::new()
        .name("centinel-sched".into())
        .spawn(move || {
            runtime.block_on(async move {
                if let Err(e) = scheduler.run(cancel, reload).await {
                    tracing::error!(error = %format!("{e:#}"), "scheduler stopped");
                }
            });
        })
        .context("starting the scheduler thread")?;

    Ok((canceller, handle))
}

/// Drops a scheduler thread to background priority.
///
/// Best-effort by design: failing to renice is not a reason to refuse to collect, and the
/// starvation guarantee comes from the separate runtime rather than from this.
#[cfg(unix)]
fn lower_priority() {
    // SAFETY: `setpriority` on the calling thread with a positive value can only lower
    // this thread's scheduling priority. It cannot raise it without privilege, and it
    // affects no other process.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }
}

#[cfg(not(unix))]
fn lower_priority() {}

#[cfg(test)]
mod tests {
    use super::*;
    use centinel_core::store::Store;

    async fn scheduler(schedules: &str) -> (tempfile::TempDir, tempfile::TempDir, Scheduler) {
        let store_dir = tempfile::tempdir().unwrap();
        let store = Store::open(store_dir.path()).await.unwrap();
        let ctx = Arc::new(Ctx::new(store));

        let config_dir = tempfile::tempdir().unwrap();
        let path = config_dir.path().join("centinel.toml");
        std::fs::write(
            &path,
            format!("[[source]]\nid = \"tampa\"\nsite = \"https://tampa.gov\"\n\n{schedules}"),
        )
        .unwrap();

        let s = Scheduler::new(ctx, Some(&path.display().to_string())).unwrap();
        (store_dir, config_dir, s)
    }

    #[tokio::test]
    async fn a_broken_schedule_refuses_to_start() {
        let store_dir = tempfile::tempdir().unwrap();
        let store = Store::open(store_dir.path()).await.unwrap();
        let ctx = Arc::new(Ctx::new(store.clone()));
        let ctx2 = Arc::new(Ctx::new(store));

        let config_dir = tempfile::tempdir().unwrap();
        let path = config_dir.path().join("centinel.toml");
        std::fs::write(
            &path,
            "[[source]]\nid = \"tampa\"\nsite = \"https://tampa.gov\"\n\n\
             [[schedule]]\nid = \"x\"\ncron = \"@daily\"\nsources = [\"orlando\"]\n",
        )
        .unwrap();

        let err = Scheduler::new(ctx, Some(&path.display().to_string()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("refusing to serve"), "{err}");
        // And it has to name what it refused over, or the operator is left guessing which
        // of twenty blocks is the broken one.
        let full = format!(
            "{:#}",
            Scheduler::new(ctx2, Some(&path.display().to_string())).unwrap_err()
        );
        assert!(full.contains("orlando"), "{full}");
    }

    /// Jitter must land inside the window and never before the nominal minute — a
    /// schedule that says 3am must not fire at 02:57.
    #[tokio::test]
    async fn the_next_fire_carries_jitter_and_never_precedes_the_cron_time() {
        let (_s, _c, sched) = scheduler(
            "[[schedule]]\nid = \"daily\"\ncron = \"0 3 * * *\"\ntz = \"UTC\"\njitter_secs = 300\n",
        )
        .await;

        let now: Timestamp = "2026-08-06T00:00:00Z".parse().unwrap();
        let next = sched.next_fire(&sched.config.schedules[0], now).unwrap();

        let nominal: Timestamp = "2026-08-06T03:00:00Z".parse().unwrap();
        assert!(next >= nominal, "fired before its own cron time: {next}");
        assert!(
            next < nominal + jiff::Span::new().seconds(300),
            "jitter escaped its window: {next}"
        );
    }

    /// Zero jitter has to mean exactly on the minute, or `--jitter-secs 0` is a lie.
    #[tokio::test]
    async fn zero_jitter_fires_exactly_on_the_minute() {
        let (_s, _c, sched) = scheduler(
            "[[schedule]]\nid = \"daily\"\ncron = \"0 3 * * *\"\ntz = \"UTC\"\njitter_secs = 0\n",
        )
        .await;
        let now: Timestamp = "2026-08-06T00:00:00Z".parse().unwrap();
        assert_eq!(
            sched.next_fire(&sched.config.schedules[0], now).unwrap(),
            "2026-08-06T03:00:00Z".parse::<Timestamp>().unwrap()
        );
    }

    #[tokio::test]
    async fn a_disabled_schedule_is_never_due_and_never_wakes_the_loop() {
        let (_s, _c, sched) =
            scheduler("[[schedule]]\nid = \"daily\"\ncron = \"@daily\"\nenabled = false\n").await;
        let now = Timestamp::now();
        assert!(sched.due_at(now).is_empty());
        assert!(sched.next_wake(now).is_none());
    }

    /// The loop wakes for the *earliest* fire, not the first schedule in the file.
    #[tokio::test]
    async fn the_wake_time_is_the_earliest_across_schedules() {
        let (_s, _c, mut sched) = scheduler(
            "[[schedule]]\nid = \"monthly\"\ncron = \"0 2 1 * *\"\ntz = \"UTC\"\njitter_secs = 0\n\n\
             [[schedule]]\nid = \"daily\"\ncron = \"0 3 * * *\"\ntz = \"UTC\"\njitter_secs = 0\n",
        )
        .await;

        // The reference point every "when next" question is measured from. Set
        // explicitly, because that is the whole correction here: measuring from *now*
        // yields an instant that is in the future by construction, so nothing is ever due.
        let now: Timestamp = "2026-08-06T00:00:00Z".parse().unwrap();
        sched.started = now;

        assert_eq!(
            sched.next_wake(now).unwrap(),
            "2026-08-06T03:00:00Z".parse::<Timestamp>().unwrap(),
            "the monthly schedule came first in the file but fires later"
        );
    }

    /// The bug this whole reference-point design exists to prevent, asserted directly:
    /// asking for the next occurrence after *now* gives an instant that is in the future
    /// by construction, so `at <= now` is never true and **nothing ever fires**. It looks
    /// exactly like a working scheduler — it logs that it started, it sleeps, and it
    /// collects nothing, forever.
    #[tokio::test]
    async fn a_schedule_whose_time_has_passed_is_due() {
        let (_s, _c, mut sched) = scheduler(
            "[[schedule]]\nid = \"minutely\"\ncron = \"* * * * *\"\ntz = \"UTC\"\njitter_secs = 0\n",
        )
        .await;

        // Last fired five minutes ago, so its next occurrence is four minutes behind us.
        let now = Timestamp::now();
        sched
            .last_fire
            .insert("minutely".into(), now - jiff::Span::new().minutes(5));

        let due = sched.due_at(now);
        assert_eq!(due.len(), 1, "an overdue schedule was not due");
        assert!(
            sched.next_wake(now).unwrap() <= now,
            "the loop would have slept"
        );

        // And having just fired, it is not due again in the same minute.
        sched.last_fire.insert("minutely".into(), now);
        assert!(
            sched.due_at(now).is_empty(),
            "a schedule fired again in the minute it just ran"
        );
    }

    /// A failing run must still move the schedule on, or one broken source is re-attempted
    /// at every wake-up instead of at its cadence.
    #[tokio::test]
    async fn a_fire_advances_the_reference_even_when_the_run_fails() {
        let (_s, _c, mut sched) =
            scheduler("[[schedule]]\nid = \"daily\"\ncron = \"@daily\"\nsources = [\"tampa\"]\n")
                .await;
        assert!(sched.last_fire.is_empty());

        // The lane is held, so the fire is skipped — the cheapest way to reach `fire`
        // without a network.
        let _held = RunLock::acquire(
            &sched.ctx.store,
            &Holder {
                pid: std::process::id(),
                started_at: Timestamp::now().to_string(),
                trigger: Trigger::Manual,
                schedule: None,
                args: String::new(),
            },
        )
        .unwrap();

        let schedule = sched.config.schedules[0].clone();
        sched
            .fire(
                &Due {
                    schedule: &schedule,
                    at: Timestamp::now(),
                    trigger: Trigger::Schedule,
                },
                &Cancel::none(),
            )
            .await;

        assert!(
            sched.last_fire.contains_key("daily"),
            "a skipped fire left the schedule due forever"
        );
    }

    /// A held lane must produce a *record*, not silence. "My cron never ran" deserves an
    /// answer in the journal rather than a hunch.
    #[tokio::test]
    async fn a_fire_that_finds_the_lane_held_records_itself_as_skipped() {
        let (_s, _c, mut sched) =
            scheduler("[[schedule]]\nid = \"daily\"\ncron = \"@daily\"\nsources = [\"tampa\"]\n")
                .await;

        // Somebody else's run — a `centinel run` in another process, as far as this knows.
        let _held = RunLock::acquire(
            &sched.ctx.store,
            &Holder {
                pid: std::process::id(),
                started_at: Timestamp::now().to_string(),
                trigger: Trigger::Manual,
                schedule: None,
                args: String::new(),
            },
        )
        .unwrap();

        let schedule = sched.config.schedules[0].clone();
        let due = Due {
            schedule: &schedule,
            at: Timestamp::now(),
            trigger: Trigger::Schedule,
        };
        sched.fire(&due, &Cancel::none()).await;

        let attempts = Journal::new(&sched.ctx.store).read().await.unwrap();
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].outcome, Outcome::Skipped);
        assert!(
            !attempts[0].outcome.is_failure(),
            "a busy lane is not a fault"
        );
        assert!(
            attempts[0].detail.as_ref().unwrap().contains("in flight"),
            "{:?}",
            attempts[0].detail
        );
    }

    /// A crash leaves only the lock. Turning it into a record is what makes the crash
    /// visible instead of a gap.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_lock_from_a_dead_run_becomes_an_interrupted_record() {
        let (_s, _c, sched) = scheduler("[[schedule]]\nid = \"daily\"\ncron = \"@daily\"\n").await;

        std::fs::write(
            sched.ctx.store.lock_path(),
            serde_json::to_vec_pretty(&Holder {
                pid: 0x00ff_fffe,
                started_at: "2026-08-06T03:00:00Z".into(),
                trigger: Trigger::Schedule,
                schedule: Some("daily".into()),
                args: String::new(),
            })
            .unwrap(),
        )
        .unwrap();

        sched.recover_interrupted().await;

        let attempts = Journal::new(&sched.ctx.store).read().await.unwrap();
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].outcome, Outcome::Interrupted);
        assert_eq!(attempts[0].schedule.as_deref(), Some("daily"));
        assert!(
            RunLock::current(&sched.ctx.store).is_none(),
            "the lock was kept"
        );
    }

    /// Adding a schedule must not start a crawl. Catch-up is for a fire that was *missed*,
    /// and a schedule created a minute ago has missed nothing.
    #[tokio::test]
    async fn a_schedule_that_has_never_fired_is_not_caught_up() {
        let (_s, _c, mut sched) =
            scheduler("[[schedule]]\nid = \"daily\"\ncron = \"@daily\"\nsources = [\"tampa\"]\n")
                .await;

        sched.catch_up(&Cancel::none()).await;
        assert!(
            Journal::new(&sched.ctx.store)
                .read()
                .await
                .unwrap()
                .is_empty(),
            "a brand-new schedule fired at startup"
        );
    }

    /// A reload that does not validate keeps the running configuration. The alternative is
    /// a typo disarming a server that was collecting correctly.
    #[tokio::test]
    async fn a_reload_that_fails_keeps_the_running_schedule() {
        let store_dir = tempfile::tempdir().unwrap();
        let store = Store::open(store_dir.path()).await.unwrap();
        let ctx = Arc::new(Ctx::new(store));

        let config_dir = tempfile::tempdir().unwrap();
        let path = config_dir.path().join("centinel.toml");
        std::fs::write(
            &path,
            "[[source]]\nid = \"tampa\"\nsite = \"https://tampa.gov\"\n\n\
             [[schedule]]\nid = \"daily\"\ncron = \"@daily\"\n",
        )
        .unwrap();

        let mut sched = Scheduler::new(ctx, Some(&path.display().to_string())).unwrap();
        assert_eq!(sched.schedules().len(), 1);

        std::fs::write(&path, "this is not toml at all\n").unwrap();
        assert!(sched.reload().is_err());
        assert_eq!(
            sched.schedules().len(),
            1,
            "a broken edit disarmed the running server"
        );
        assert_eq!(sched.schedules()[0].id, "daily");
    }

    #[test]
    fn the_arguments_read_back_as_the_command_somebody_would_type() {
        use centinel_core::ops::{RunArgs, Stage};
        let line = one_line_args(&RunArgs {
            sources: vec!["tampa".into()],
            skip: vec![Stage::Embed],
            limit: Some(500),
            refresh: true,
            ..Default::default()
        });
        assert_eq!(line, "--source tampa --skip embed --limit 500 --refresh");
    }
}
