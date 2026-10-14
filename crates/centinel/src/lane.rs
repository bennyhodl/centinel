//! The store's one run lane, shared by the scheduler and the web's Run button.
//!
//! A run takes the lane ([`RunLock`]) or does not run at all, and every run that took it is
//! written to the journal. The scheduler used to own both halves inline; the Sources page
//! starts runs too, and a second copy of "take the lock, invoke, release, record" is the
//! copy that forgets one of the four. So they live here, once.
//!
//! ## Collection authority
//!
//! Only two callers: a `[[schedule]]` the operator wrote, and an administrator signed in
//! to the web (`/workspace/sources/{id}/run`, admin-only in `access.rs`, browser session
//! only at the gateway). Neither MCP nor `/ops` can reach this — `run` stays
//! `Reach::Operator` in the registry.
//!
//! ## One id: the journal's, the job's, the page's
//!
//! A run is a job in [`centinel_core::jobs`] under its journal run id, labelled with the
//! sources it walks, from the moment the lane is taken. The page that clicked Run gets
//! that id back and opens the drawer on it; `POST /workspace/jobs/{id}/stop` trips the
//! job's token, and the op stops at its next item boundary and is journaled
//! `interrupted`. A process shutdown ends a run the same way, through the same token:
//! [`Lane::run`] takes the caller's token only to forward it to the job's.

use std::sync::Arc;

use centinel_core::jobs::Job;
use centinel_core::journal::{
    Attempt, Held, Holder, Journal, Outcome, RunLock, Subtracted, Trigger, arithmetic,
};
use centinel_core::op::{self, Cancel, Ctx, Progress};
use centinel_core::ops::{RunArgs, RunReport};
use jiff::Timestamp;

/// Who started a run, for the lock file and the journal.
pub struct Launch {
    /// The surface the op is invoked from, for the log: `schedule`, `web`.
    pub surface: &'static str,
    pub trigger: Trigger,
    /// The `[[schedule]]` that fired, if one did.
    pub schedule: Option<String>,
    /// When it was nominally due, for a scheduled run.
    pub due_at: Option<String>,
}

/// The lane, held. Dropping it without [`Lane::run`] releases it, records nothing, and
/// ends the job as failed — a job that stopped without saying how.
pub struct Lane {
    ctx: Arc<Ctx>,
    lock: RunLock,
    launch: Launch,
    args: RunArgs,
    started: Timestamp,
    job: Job,
    /// The token the op polls. Tripped by [`centinel_core::jobs::Jobs::stop`] under the
    /// job's id, which is how both the Stop button and a shutdown reach it.
    cancel: Cancel,
}

impl Lane {
    /// Takes the lane for these arguments, or says who holds it.
    pub fn acquire(ctx: &Arc<Ctx>, args: RunArgs, launch: Launch) -> Result<Self, Held> {
        let started = Timestamp::now();
        let holder = Holder {
            pid: std::process::id(),
            started_at: started.to_string(),
            trigger: launch.trigger,
            schedule: launch.schedule.clone(),
            sources: args.sources.clone(),
            args: one_line_args(&args),
        };
        let lock = RunLock::acquire(&ctx.store, &holder)?;
        let (job, cancel) = ctx
            .jobs
            .start_stoppable(started.to_string(), "run", label(&args));
        Ok(Self {
            ctx: Arc::clone(ctx),
            lock,
            launch,
            args,
            started,
            job,
            cancel,
        })
    }

    /// When it started: the journal's handle for it, and the job's id.
    pub fn run_id(&self) -> String {
        self.started.to_string()
    }

    /// Runs, releases the lane, and records the attempt. Progress goes to `progress` when
    /// somebody is watching, else to the log. `shutdown` is the caller's own reason to
    /// stop — the scheduler's, on `SIGTERM` — and is forwarded to the job's token, so
    /// there is one way a run ends early and it is journaled `interrupted` either way.
    pub async fn run(self, progress: Option<Progress>, shutdown: Cancel) -> Attempt {
        let Self {
            ctx,
            lock,
            launch,
            args,
            started,
            job,
            cancel,
        } = self;
        tracing::info!(
            schedule = launch.schedule.as_deref().unwrap_or("-"),
            trigger = launch.trigger.as_str(),
            args = %one_line_args(&args),
            "run started"
        );

        let def = op::find("run").expect("`run` is always registered");
        let value = serde_json::to_value(&args).expect("RunArgs always serializes");
        let run_id = started.to_string();
        let work = crate::logging::invoke_as(
            launch.surface,
            def,
            Arc::clone(&ctx),
            value,
            progress,
            cancel,
            Some(job),
        );
        tokio::pin!(work);
        let result = tokio::select! {
            result = &mut work => result,
            // Asked to stop from outside the job: trip its token and wait for it to reach
            // an item boundary, rather than abandon the future mid-append. Pends forever
            // on `Cancel::none()`, so a web run simply runs.
            _ = shutdown.cancelled() => {
                ctx.jobs.stop(&run_id);
                work.await
            }
        };

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
            // A cancellation is somebody stopping the process, not a broken source.
            // Filed as a failure it would count against the schedule and, over a week of
            // nightly restarts, bury a source that had genuinely stopped responding.
            Err(e) if op::is_cancelled(&e) => (Outcome::Interrupted, None, None),
            Err(e) => (Outcome::Failed, Some(format!("{e:#}")), None),
        };

        let (added, subtracted) = report
            .as_ref()
            .map(arithmetic)
            .unwrap_or_else(|| (Default::default(), Subtracted::default()));

        let attempt = Attempt {
            run_id: started.to_string(),
            schedule: launch.schedule,
            sources: args.sources.clone(),
            trigger: launch.trigger,
            due_at: launch.due_at,
            started_at: started.to_string(),
            finished_at: Timestamp::now().to_string(),
            outcome,
            detail,
            added,
            subtracted,
            report,
        };
        record(&ctx, &attempt).await;
        attempt
    }
}

/// Appends an attempt, complaining loudly rather than failing the caller.
///
/// A journal write that fails must not take the scheduler down: the next fire is worth
/// more than the record of this one. It must also not be silent, or the corpus grows
/// while its history quietly stops.
pub async fn record(ctx: &Ctx, attempt: &Attempt) {
    tracing::info!(
        schedule = attempt.schedule.as_deref().unwrap_or("-"),
        outcome = attempt.outcome.as_str(),
        added = attempt.added.documents,
        "run finished"
    );
    if let Err(e) = Journal::new(&ctx.store).append(attempt).await {
        tracing::error!(error = %format!("{e:#}"), "could not append to the run journal");
    }
}

/// What the job is called: the sources it walks, or every enabled one.
fn label(args: &RunArgs) -> String {
    match args.sources.as_slice() {
        [] => "every enabled source".to_string(),
        named => named.join(", "),
    }
}

/// The arguments, for the lock file and for the log.
pub fn one_line_args(args: &RunArgs) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use centinel_core::catalog::Sources;
    use centinel_core::config::SourceConfig;
    use centinel_core::store::Store;

    fn launch() -> Launch {
        Launch {
            surface: "web",
            trigger: Trigger::Manual,
            schedule: None,
            due_at: None,
        }
    }

    /// The whole of problem three: a run started from the web is a stoppable job under
    /// its run id, labelled with its source; stopping it by that id ends the op at its
    /// next boundary, and the journal says `interrupted`, not `failed`.
    #[tokio::test]
    async fn a_web_run_stopped_by_its_job_id_is_journaled_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        Sources::new(&store)
            .add(&SourceConfig::site("tampa", "https://127.0.0.1:9/"))
            .await
            .unwrap();
        let ctx = Arc::new(Ctx::new(store));
        let args = RunArgs {
            sources: vec!["tampa".into()],
            ..Default::default()
        };

        let lane = Lane::acquire(&ctx, args, launch()).unwrap();
        let id = lane.run_id();
        let job = &ctx.jobs.snapshot()[0];
        assert_eq!(
            (job.id.as_str(), job.kind.as_str(), job.label.as_str()),
            (id.as_str(), "run", "tampa")
        );
        assert!(job.stoppable);

        // Stopped before it runs: the op's first boundary check ends it before a single
        // request is made, which is also what keeps this test off the network.
        assert!(ctx.jobs.stop(&id));
        let attempt = lane.run(None, Cancel::none()).await;
        assert_eq!(attempt.outcome, Outcome::Interrupted);
        assert_eq!(attempt.run_id, id);

        let journaled = Journal::new(&ctx.store).read().await.unwrap();
        assert_eq!(journaled.len(), 1);
        assert_eq!(journaled[0].outcome, Outcome::Interrupted);
        assert_eq!(
            ctx.jobs.snapshot()[0].outcome,
            Some(centinel_core::jobs::Outcome::Cancelled)
        );
        assert!(
            RunLock::current(&ctx.store).is_none(),
            "the lane is released"
        );
    }

    /// The scheduler's shutdown reaches the op through the job's token too: one way a run
    /// ends early, journaled the same way.
    #[tokio::test]
    async fn a_shutdown_ends_a_run_through_its_job() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        Sources::new(&store)
            .add(&SourceConfig::site("tampa", "https://127.0.0.1:9/"))
            .await
            .unwrap();
        let ctx = Arc::new(Ctx::new(store));
        let (shutdown, cancel) = Cancel::channel();
        shutdown.cancel();
        let lane = Lane::acquire(
            &ctx,
            RunArgs {
                sources: vec!["tampa".into()],
                ..Default::default()
            },
            launch(),
        )
        .unwrap();
        let attempt = lane.run(None, cancel).await;
        assert_eq!(attempt.outcome, Outcome::Interrupted);
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
