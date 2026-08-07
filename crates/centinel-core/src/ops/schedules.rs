//! `schedules` and `history` — what the watchman is set to do, and what he did.
//!
//! **The useful half of the ask, and the whole of what a consumer gets.** An agent can now
//! ask when the corpus was last collected, whether the last attempt failed, and how much
//! came in — and qualify its answer accordingly: *"the last collection of `tampa-gov` was
//! nine days ago and it was blocked."*
//!
//! It is the same honesty as reporting `vectors_indexed` beside `total_chunks_indexed`: an
//! absent stage is a different answer, not a slower one, and a stale corpus is a different
//! answer from a current one.
//!
//! What it cannot do is send the watchman out. Both ops are `Public` and read-only; every
//! op that causes collection is `Operator` and invisible from here (`op::Reach`).
//!
//! ## Why these are ops and not routes
//!
//! "Routes are the registry" is the first line of `crates/centinel/src/http.rs`. Honouring
//! it means the operator gets `centinel schedules` with a real renderer and a model gets an
//! MCP tool, from one definition, with no third code path to keep in step.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::journal::{Added, Attempt, Holder, Journal, Outcome, RunLock, Subtracted, Trigger};
use crate::prelude::*;

// ── schedules ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ScheduleStatus {
    pub id: String,
    /// The expression as the operator wrote it — `@daily` stays `@daily`.
    pub cron: String,
    /// The IANA zone it fires in, resolved: an absent `tz` reports the host's name rather
    /// than leaving a reader to guess which machine's idea of 3am this is.
    pub tz: String,
    pub enabled: bool,
    /// Sources it runs, or empty for every enabled source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip: Vec<String>,
    /// Re-fetches and re-derives the whole corpus at every fire. Its own field because it
    /// is the most expensive thing a `[[schedule]]` block can say, and a reader asking
    /// "why does this take four hours" should not have to open the config to find out.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub refresh: bool,
    /// When it next fires, with jitter applied — the real time, not the nominal one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_fire: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fire: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_outcome: Option<Outcome>,
    /// Consecutive failing attempts. Skips and interruptions do not count — see
    /// [`Outcome::is_failure`].
    pub consecutive_failures: u32,
    /// Set when a run for *this* schedule is in flight right now.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub running: bool,
    /// Why this schedule cannot run, when validation refused it.
    ///
    /// Present rather than fatal here: `serve` refuses to start on one of these, and this
    /// op has to be able to *show* the operator what it refused over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SchedulesReport {
    /// The config file these came from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    pub schedules: Vec<ScheduleStatus>,
    /// The run in flight, whichever schedule it belongs to — including a `centinel run`
    /// somebody typed. Read from `run.lock`, so this answer is identical on every surface
    /// and correct even when no server is running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<Holder>,
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct SchedulesArgs {
    /// Validate every schedule and fail if any is broken, rather than reporting.
    ///
    /// Exactly the check `centinel serve` performs before binding, so an operator can run
    /// it after editing the config instead of finding out at the next restart.
    #[arg(long)]
    #[serde(default)]
    pub check: bool,

    /// Config file. Defaults to the usual search path.
    #[arg(long, value_name = "FILE")]
    #[serde(default)]
    pub config: Option<String>,
}

/// Show configured schedules, when each next fires, and how the last one went.
#[op(group = "corpus")]
pub async fn schedules(ctx: &Ctx, args: SchedulesArgs) -> anyhow::Result<SchedulesReport> {
    let (config, path) = super::load_config(args.config.as_deref())?;

    if args.check {
        config.validate_schedules()?;
    }

    let journal = Journal::new(&ctx.store);
    let attempts = journal.read().await?;
    let running = RunLock::current(&ctx.store);
    let now = jiff::Timestamp::now();

    let mut out = Vec::with_capacity(config.schedules.len());
    for schedule in &config.schedules {
        // Everything about *this* schedule, newest first, so the last outcome and the
        // failure streak come out of one pass.
        let mine: Vec<&Attempt> = attempts
            .iter()
            .filter(|a| a.schedule.as_deref() == Some(schedule.id.as_str()))
            .collect();
        let last = mine.first();
        let consecutive_failures = mine
            .iter()
            // A skip or an interruption is neither a success nor a failure, so it must
            // not *break* a streak either — a nightly restart would otherwise hide a
            // source that has been failing for a week.
            .filter(|a| a.outcome != Outcome::Skipped && a.outcome != Outcome::Interrupted)
            .take_while(|a| a.outcome.is_failure())
            .count() as u32;

        // A broken schedule is reported, not raised: this is the op an operator reaches
        // for *because* something is wrong.
        let mut problem = None;
        let mut next_fire = None;
        let mut tz = schedule.tz.clone().unwrap_or_default();

        match (schedule.cron(), schedule.zone()) {
            (Ok(cron), Ok(zone)) => {
                tz = zone.iana_name().unwrap_or("local").to_string();
                if schedule.is_enabled() {
                    next_fire = cron.next_after(now, &zone).map(|at| {
                        let jitter = crate::schedule::jitter_offset(
                            &node_seed(ctx),
                            &schedule.id,
                            schedule.jitter_secs(),
                        );
                        local(at + jiff::Span::new().seconds(jitter as i64), &zone)
                    });
                    if next_fire.is_none() {
                        problem = Some(format!("`{}` parses but never occurs", schedule.cron));
                    }
                }
            }
            (Err(e), _) | (_, Err(e)) => problem = Some(format!("{e:#}")),
        }

        if let Err(e) = schedule.run_args() {
            problem.get_or_insert_with(|| format!("{e:#}"));
        }
        for id in &schedule.sources {
            if config.source(id).is_none() {
                problem.get_or_insert_with(|| format!("source `{id}` has no [[source]] block"));
            }
        }

        out.push(ScheduleStatus {
            id: schedule.id.clone(),
            cron: schedule.cron.clone(),
            tz,
            enabled: schedule.is_enabled(),
            sources: schedule.sources.clone(),
            skip: schedule.skip.clone(),
            refresh: schedule.refresh,
            next_fire,
            // Rendered in the schedule's zone, not the journal's UTC. An operator who
            // wrote "3am New York" and reads back "07:04" concludes it is broken — and
            // the whole reason `tz` is a name is that they reason in local time.
            last_fire: last.map(|a| {
                a.started()
                    .map(|at| local(at, &zone_or_utc(schedule)))
                    .unwrap_or_else(|| a.started_at.clone())
            }),
            last_outcome: last.map(|a| a.outcome),
            consecutive_failures,
            running: running
                .as_ref()
                .is_some_and(|h| h.schedule.as_deref() == Some(schedule.id.as_str())),
            problem,
        });
    }

    Ok(SchedulesReport {
        config: path.map(|p| p.display().to_string()),
        schedules: out,
        running,
    })
}

/// An instant as RFC 3339 in a named zone — local wall clock, with the offset that makes
/// it unambiguous.
///
/// Deliberately not the bracketed RFC 9557 form: the offset carries everything a reader or
/// a `Date` constructor needs, and the brackets break both.
fn local(at: jiff::Timestamp, zone: &jiff::tz::TimeZone) -> String {
    at.to_zoned(zone.clone())
        .strftime("%Y-%m-%dT%H:%M:%S%:z")
        .to_string()
}

/// A schedule's zone, falling back to UTC only when it does not resolve — in which case
/// `problem` is already set and the timestamps are the least of it.
fn zone_or_utc(schedule: &crate::config::ScheduleConfig) -> jiff::tz::TimeZone {
    schedule.zone().unwrap_or(jiff::tz::TimeZone::UTC)
}

/// The per-install seed jitter is derived from.
///
/// The store root, because it is the one string that identifies this corpus on this
/// machine and is stable across restarts. Two operators running the same config against
/// their own stores get different offsets, which is the entire point.
fn node_seed(ctx: &Ctx) -> String {
    ctx.store.root().display().to_string()
}

// ── history ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct HistoryReport {
    pub attempts: Vec<Attempt>,
    /// Attempts the filters matched but `--limit` cut. Named rather than silent: a bounded
    /// list that does not say it is bounded reads as the whole history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<usize>,
    /// Set when `--run` matched more than one attempt by prefix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ambiguous: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct HistoryArgs {
    /// One run, by id or any unambiguous prefix of one.
    ///
    /// Prints the whole embedded report rather than the summary line. The rule the rest of
    /// the tool follows: anything Centinel prints, Centinel takes back.
    #[arg(long, value_name = "ID")]
    #[serde(default)]
    pub run: Option<String>,

    /// Limit to one schedule.
    #[arg(long, value_name = "ID")]
    #[serde(default)]
    pub schedule: Option<String>,

    /// Limit to attempts that touched one source.
    #[arg(long, value_name = "ID")]
    #[serde(default)]
    pub source: Option<String>,

    /// Only attempts that failed.
    #[arg(long)]
    #[serde(default)]
    pub failed: bool,

    /// Only attempts started at or after this RFC 3339 instant.
    #[arg(long, value_name = "TIMESTAMP")]
    #[serde(default)]
    pub since: Option<String>,

    /// Most recent attempts to show.
    #[arg(long, default_value_t = 20)]
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    20
}

/// Show what scheduled and manual runs did, newest first.
#[op(group = "corpus")]
pub async fn history(ctx: &Ctx, args: HistoryArgs) -> anyhow::Result<HistoryReport> {
    let mut attempts = Journal::new(&ctx.store).read().await?;

    // A run id short-circuits every other filter: it is a request for one record.
    if let Some(prefix) = &args.run {
        let matched: Vec<Attempt> = attempts.into_iter().filter(|a| a.matches(prefix)).collect();
        if matched.len() > 1 {
            return Ok(HistoryReport {
                ambiguous: Some(matched.iter().map(|a| a.run_id.clone()).collect()),
                attempts: Vec::new(),
                truncated: None,
            });
        }
        anyhow::ensure!(!matched.is_empty(), "no run matching `{prefix}`");
        return Ok(HistoryReport {
            attempts: matched,
            truncated: None,
            ambiguous: None,
        });
    }

    if let Some(id) = &args.schedule {
        attempts.retain(|a| a.schedule.as_deref() == Some(id.as_str()));
    }
    if let Some(id) = &args.source {
        attempts.retain(|a| {
            a.report
                .as_ref()
                .is_some_and(|r| r.sources.iter().any(|s| s.source == *id))
        });
    }
    if args.failed {
        attempts.retain(|a| a.outcome.is_failure());
    }
    if let Some(since) = &args.since {
        let since: jiff::Timestamp = since
            .parse()
            .map_err(|e| anyhow::anyhow!("`{since}` is not an RFC 3339 timestamp: {e}"))?;
        attempts.retain(|a| a.started().is_some_and(|t| t >= since));
    }

    let truncated = attempts.len().saturating_sub(args.limit);
    attempts.truncate(args.limit);

    Ok(HistoryReport {
        attempts,
        truncated: (truncated > 0).then_some(truncated),
        ambiguous: None,
    })
}

// ── rendering ─────────────────────────────────────────────────────────────────

impl Render for SchedulesReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        if self.schedules.is_empty() {
            p.line(p.paint("No schedules configured.", Ink::Dim))?;
            return p.note("centinel schedule set");
        }

        // The lane first, because "why has nothing fired" is usually answered here.
        if let Some(holder) = &self.running {
            p.marked(Mark::Warn, p.paint(&holder.describe(), Ink::Dim))?;
            p.blank()?;
        }

        for (i, schedule) in self.schedules.iter().enumerate() {
            if i > 0 {
                p.blank()?;
            }
            schedule.render(p)?;
        }
        Ok(())
    }
}

impl Render for ScheduleStatus {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let target = if self.sources.is_empty() {
            "every source".to_string()
        } else {
            self.sources.join(", ")
        };
        p.title(&self.id, &target)?;

        p.nest(|p| {
            let mut cadence = format!("{} {}", self.cron, p.paint(&self.tz, Ink::Dim));
            if !self.enabled {
                cadence.push_str(&p.paint("  (disabled)", Ink::Dim));
            }
            p.line(cadence)?;

            // The expensive settings, on their own line, because the config is the only
            // other place they are written down.
            let mut costs = Vec::new();
            if self.refresh {
                costs.push("refresh: re-fetches and re-derives everything, every fire".to_string());
            }
            if !self.skip.is_empty() {
                costs.push(format!("skips {}", self.skip.join(", ")));
            }
            if !costs.is_empty() {
                p.wrapped(&costs.join(" · "), Ink::Dim)?;
            }

            if let Some(problem) = &self.problem {
                p.marked(Mark::Bad, p.paint(problem, Ink::Dim))?;
                // Said here rather than left to be discovered at the next restart.
                return p.note("serve will refuse to start until this is fixed");
            }

            if self.running {
                p.marked(Mark::Warn, p.paint("running now", Ink::Dim))?;
            } else if let Some(next) = &self.next_fire {
                p.kv("next", 6, p.paint(&render::short_time(next), Ink::Dim))?;
            } else if self.enabled {
                p.kv("next", 6, p.paint("never", Ink::Dim))?;
            }

            match (&self.last_fire, self.last_outcome) {
                (Some(last), Some(outcome)) => {
                    let mark = outcome_mark(outcome);
                    let mut text = format!(
                        "{} {}",
                        render::short_time(last),
                        p.paint(outcome.as_str(), mark.ink())
                    );
                    if self.consecutive_failures > 1 {
                        text.push_str(&p.paint(
                            &format!(" · {} in a row", self.consecutive_failures),
                            Ink::Dim,
                        ));
                    }
                    p.kv("last", 6, text)?;
                }
                // Never having fired is a fact worth stating plainly. It is also what a
                // schedule added five minutes ago looks like, so it is not marked bad.
                _ => p.kv("last", 6, p.paint("never fired", Ink::Dim))?,
            }
            Ok(())
        })
    }
}

impl Render for HistoryReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        if let Some(ids) = &self.ambiguous {
            p.line("That prefix matches several runs:")?;
            return p.nest(|p| {
                for id in ids {
                    p.line(p.paint(id, Ink::Dim))?;
                }
                Ok(())
            });
        }

        if self.attempts.is_empty() {
            return p.line(p.paint("No runs recorded.", Ink::Dim));
        }

        // One attempt asked for by id gets the whole report; a list gets the lines.
        if self.attempts.len() == 1 && self.truncated.is_none() {
            let attempt = &self.attempts[0];
            attempt.render(p)?;
            if let Some(report) = &attempt.report {
                p.blank()?;
                return p.nest(|p| report.render(p));
            }
            return Ok(());
        }

        for attempt in &self.attempts {
            attempt.render(p)?;
        }
        if let Some(more) = self.truncated {
            let text = format!(
                "… and {} earlier, raise --limit to see them",
                render::count(more as u64)
            );
            p.line(p.paint(&text, Ink::Dim))?;
        }
        Ok(())
    }
}

impl Render for Attempt {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let mark = outcome_mark(self.outcome);
        let who = self
            .schedule
            .clone()
            .unwrap_or_else(|| self.trigger.as_str().to_string());

        let head = format!(
            "{:<12} {:<11} {}",
            p.paint(&render::short_time(&self.started_at), Ink::Bold),
            p.paint(self.outcome.as_str(), mark.ink()),
            render::truncate(&who, p.width().saturating_sub(28)),
        );
        p.marked(mark, head)?;

        p.nest(|p| {
            let mut parts = vec![arithmetic_line(&self.added, &self.subtracted, p)];
            if let Some(detail) = &self.detail {
                parts.push(render::one_line(detail));
            }
            // The handle. `history --run <this>` takes it back by prefix.
            parts.push(p.paint(&self.run_id, Ink::Dim));
            p.wrapped(&parts.join(" · "), Ink::Dim)
        })
    }
}

/// The additions, then the three subtractions — **never a sum of them**.
///
/// A blocked address folded into absence reports a live page as deleted, which is the
/// mistake [`crate::domain::Liveness::Blocked`] exists to prevent. So each is its own term,
/// and the renderer has no arithmetic to get wrong.
fn arithmetic_line(added: &Added, subtracted: &Subtracted, p: &Painter<'_>) -> String {
    let mut parts = Vec::new();

    if added.documents > 0 {
        parts.push(format!("+{} collected", render::count(added.documents)));
    }
    if added.derived > 0 {
        parts.push(format!("+{} derived", render::count(added.derived)));
    }
    if added.chunks > 0 {
        parts.push(format!("+{} embedded", render::count(added.chunks)));
    }
    if subtracted.vanished > 0 {
        parts.push(format!("{} vanished", render::count(subtracted.vanished)));
    }
    if subtracted.gone > 0 {
        parts.push(format!("{} gone", render::count(subtracted.gone)));
    }
    if subtracted.blocked > 0 {
        // Amber: a wall of blocks is the one subtraction that means "look at this", and
        // also the one most easily misread as absence.
        parts.push(p.paint(
            &format!("{} blocked", render::count(subtracted.blocked)),
            Ink::Yellow,
        ));
    }
    if subtracted.errored > 0 {
        parts.push(format!("{} errored", render::count(subtracted.errored)));
    }

    if parts.is_empty() {
        // The commonest outcome, and the one a watchman exists to produce. It needs
        // saying: an empty line here reads as a run that did not happen.
        return "nothing changed".to_string();
    }
    parts.join(" · ")
}

fn outcome_mark(outcome: Outcome) -> Mark {
    match outcome {
        Outcome::Ok => Mark::Ok,
        Outcome::Partial | Outcome::Skipped | Outcome::Interrupted => Mark::Warn,
        Outcome::Failed => Mark::Bad,
    }
}

/// So the journal's own types render wherever they appear.
impl Render for Trigger {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.line(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{Journal, Trigger};
    use crate::store::Store;

    async fn ctx() -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        (dir, Ctx::new(store))
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

    fn config_with(schedules: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("centinel.toml");
        std::fs::write(
            &path,
            format!("[[source]]\nid = \"tampa\"\nsite = \"https://tampa.gov\"\n\n{schedules}"),
        )
        .unwrap();
        let p = path.display().to_string();
        (dir, p)
    }

    #[tokio::test]
    async fn an_empty_store_reports_no_schedules_rather_than_failing() {
        let (_d, ctx) = ctx().await;
        let (_c, config) = config_with("");
        let report = schedules(
            &ctx,
            SchedulesArgs {
                config: Some(config),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(report.schedules.is_empty());
        assert!(report.running.is_none());
    }

    #[tokio::test]
    async fn a_schedule_reports_its_cadence_and_next_fire() {
        let (_d, ctx) = ctx().await;
        let (_c, config) = config_with(
            "[[schedule]]\nid = \"daily\"\ncron = \"0 3 * * *\"\ntz = \"America/New_York\"\n\
             sources = [\"tampa\"]\n",
        );

        let report = schedules(
            &ctx,
            SchedulesArgs {
                config: Some(config),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let s = &report.schedules[0];
        assert_eq!(s.id, "daily");
        assert_eq!(s.tz, "America/New_York");
        assert!(s.enabled);
        assert!(s.next_fire.is_some(), "an enabled schedule must say when");
        assert!(s.last_fire.is_none(), "nothing has fired yet");
        assert_eq!(s.consecutive_failures, 0);
        assert!(s.problem.is_none());
    }

    /// A broken schedule must be *shown*, not raised — this is the op somebody reaches for
    /// because `serve` refused to start.
    #[tokio::test]
    async fn a_broken_schedule_is_reported_rather_than_raised() {
        let (_d, ctx) = ctx().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("centinel.toml");
        // Written past `Config::parse`'s validation, which is what `serve` runs.
        std::fs::write(
            &path,
            "[[source]]\nid = \"tampa\"\nsite = \"https://tampa.gov\"\n\n\
             [[schedule]]\nid = \"broken\"\ncron = \"0 0 30 2 *\"\n",
        )
        .unwrap();

        let report = schedules(
            &ctx,
            SchedulesArgs {
                config: Some(path.display().to_string()),
                check: false,
            },
        )
        .await;

        // `load_config` validates, so a file this broken cannot even be read — which is
        // the point: `--check` and `serve` agree, and both refuse.
        assert!(
            report.is_err(),
            "a never-occurring cron must not load silently"
        );
    }

    /// Skips and interruptions must neither count towards a streak nor break one. A
    /// nightly restart would otherwise hide a source failing every night.
    #[tokio::test]
    async fn the_failure_streak_ignores_skips_and_shutdowns() {
        let (_d, ctx) = ctx().await;
        let (_c, config) = config_with("[[schedule]]\nid = \"daily\"\ncron = \"@daily\"\n");

        let j = Journal::new(&ctx.store);
        // Oldest first: two failures, then a shutdown, then another failure.
        for (id, outcome) in [
            ("2026-08-01T03:00:00Z", Outcome::Failed),
            ("2026-08-02T03:00:00Z", Outcome::Failed),
            ("2026-08-03T03:00:00Z", Outcome::Interrupted),
            ("2026-08-04T03:00:00Z", Outcome::Skipped),
            ("2026-08-05T03:00:00Z", Outcome::Failed),
        ] {
            j.append(&attempt(id, Some("daily"), outcome))
                .await
                .unwrap();
        }

        let report = schedules(
            &ctx,
            SchedulesArgs {
                config: Some(config),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let s = &report.schedules[0];
        assert_eq!(
            s.consecutive_failures, 3,
            "the shutdown and the skip should be invisible to the streak"
        );
        assert_eq!(s.last_outcome, Some(Outcome::Failed));
        // Compared as an *instant*: `last_fire` is rendered in the schedule's zone, which
        // here is whatever the test machine's is. Asserting the string would pass in UTC
        // and fail everywhere else.
        assert_eq!(
            s.last_fire
                .as_deref()
                .map(|t| t.parse::<jiff::Timestamp>().unwrap()),
            Some("2026-08-05T03:00:00Z".parse::<jiff::Timestamp>().unwrap())
        );
    }

    #[tokio::test]
    async fn a_run_in_flight_is_reported_from_the_lock() {
        let (_d, ctx) = ctx().await;
        let (_c, config) = config_with("[[schedule]]\nid = \"daily\"\ncron = \"@daily\"\n");

        let _lock = crate::journal::RunLock::acquire(
            &ctx.store,
            &Holder {
                pid: std::process::id(),
                started_at: jiff::Timestamp::now().to_string(),
                trigger: Trigger::Schedule,
                schedule: Some("daily".into()),
                args: String::new(),
            },
        )
        .unwrap();

        let report = schedules(
            &ctx,
            SchedulesArgs {
                config: Some(config),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert!(report.running.is_some(), "the lane is held");
        assert!(report.schedules[0].running, "and this schedule holds it");
    }

    #[tokio::test]
    async fn history_filters_and_bounds_itself_out_loud() {
        let (_d, ctx) = ctx().await;
        let j = Journal::new(&ctx.store);
        for (id, schedule, outcome) in [
            ("2026-08-01T03:00:00Z", Some("daily"), Outcome::Ok),
            ("2026-08-02T03:00:00Z", Some("daily"), Outcome::Failed),
            ("2026-08-03T03:00:00Z", Some("weekly"), Outcome::Ok),
            ("2026-08-04T03:00:00Z", None, Outcome::Ok),
        ] {
            j.append(&attempt(id, schedule, outcome)).await.unwrap();
        }

        let all = history(
            &ctx,
            HistoryArgs {
                limit: 20,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(all.attempts.len(), 4);
        assert_eq!(
            all.attempts[0].run_id, "2026-08-04T03:00:00Z",
            "newest first"
        );

        let daily = history(
            &ctx,
            HistoryArgs {
                schedule: Some("daily".into()),
                limit: 20,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(daily.attempts.len(), 2);

        let failed = history(
            &ctx,
            HistoryArgs {
                failed: true,
                limit: 20,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(failed.attempts.len(), 1);

        let since = history(
            &ctx,
            HistoryArgs {
                since: Some("2026-08-03T00:00:00Z".into()),
                limit: 20,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(since.attempts.len(), 2);

        // A bounded list that does not say it is bounded reads as the whole history.
        let capped = history(
            &ctx,
            HistoryArgs {
                limit: 2,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(capped.attempts.len(), 2);
        assert_eq!(capped.truncated, Some(2));
    }

    /// The handle rule: what `history` prints, `history --run` takes back, by prefix.
    #[tokio::test]
    async fn a_run_resolves_by_prefix_and_says_when_it_is_ambiguous() {
        let (_d, ctx) = ctx().await;
        let j = Journal::new(&ctx.store);
        j.append(&attempt("2026-08-06T03:00:11Z", None, Outcome::Ok))
            .await
            .unwrap();
        j.append(&attempt("2026-08-06T03:45:02Z", None, Outcome::Ok))
            .await
            .unwrap();

        let one = history(
            &ctx,
            HistoryArgs {
                run: Some("2026-08-06T03:00".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(one.attempts.len(), 1);

        let ambiguous = history(
            &ctx,
            HistoryArgs {
                run: Some("2026-08-06T03".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(ambiguous.ambiguous.as_ref().unwrap().len(), 2);
        assert!(ambiguous.attempts.is_empty());

        let missing = history(
            &ctx,
            HistoryArgs {
                run: Some("1999".into()),
                ..Default::default()
            },
        )
        .await;
        assert!(missing.is_err());
    }

    /// A quiet run is the commonest outcome. An empty line here would read as a run that
    /// never happened, which is the exact confusion the journal exists to end.
    #[test]
    fn a_run_that_changed_nothing_says_so() {
        let mut buf: Vec<u8> = Vec::new();
        let mut p = Painter::new(&mut buf, false, 100);
        let line = arithmetic_line(&Added::default(), &Subtracted::default(), &p);
        assert_eq!(line, "nothing changed");
        let _ = &mut p;
    }

    /// Three terms, never a sum — a blocked address folded into absence would report a
    /// live page as deleted.
    #[test]
    fn the_subtractions_render_as_separate_terms() {
        let mut buf: Vec<u8> = Vec::new();
        let p = Painter::new(&mut buf, false, 100);
        let line = arithmetic_line(
            &Added {
                documents: 4,
                derived: 3,
                chunks: 12,
            },
            &Subtracted {
                vanished: 7,
                gone: 2,
                blocked: 40,
                errored: 1,
            },
            &p,
        );
        assert!(line.contains("7 vanished"), "{line}");
        assert!(line.contains("2 gone"), "{line}");
        assert!(line.contains("40 blocked"), "{line}");
        assert!(line.contains("1 errored"), "{line}");
        assert!(!line.contains("49"), "the three were summed: {line}");
        assert!(!line.contains("50"), "the four were summed: {line}");
    }
}
