//! `collect` — fetch everything a [`DiscoveryRun`] found.
//!
//! This is what turns a list of addresses into a corpus.
//!
//! ## Resumability is not a feature here, it is a consequence
//!
//! At a polite rate a city is an hour of work, so interruption is normal rather than
//! exceptional. But there is no checkpoint file: the log already records every
//! Observation, so "what still needs collecting" is
//!
//! ```text
//! latest DiscoveryRun's resources  −  resources already observed
//! ```
//!
//! Kill it at URL 4,000 and re-run; it starts at 4,001. That falls out of files-being-
//! truth rather than being engineered, which is the property SPEC §5 was bought for.

use std::collections::{BTreeMap, HashMap};

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fetch::{Fetcher, content_kind};
use crate::policy::{HostPolicy, Pacer};
use crate::prelude::*;
use crate::store::LogRecord;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct CollectArgs {
    /// Source to collect, as used by `discover`.
    #[arg(long)]
    pub source: String,

    /// Stop after this many fetches. The way to try a site before committing an hour.
    #[arg(long)]
    #[serde(default)]
    pub limit: Option<usize>,

    /// Requests per second, per host.
    #[arg(long, default_value_t = 1.0)]
    #[serde(default = "default_rps")]
    pub rps: f64,

    /// Re-fetch addresses already in the store instead of skipping them.
    #[arg(long)]
    #[serde(default)]
    pub refresh: bool,

    /// Only collect addresses whose URL contains this substring. Repeatable.
    ///
    /// Deliberately a substring rather than a regex: this is a coarse "just the PDFs"
    /// filter for exploration, not the boundary policy that ticket #4 owns.
    #[arg(long = "match")]
    #[serde(default)]
    pub matches: Vec<String>,

    /// Skip bodies larger than this many megabytes.
    #[arg(long, default_value_t = 256)]
    #[serde(default = "default_max_mb")]
    pub max_mb: u64,

    /// Failures to include in the report.
    #[arg(long, default_value_t = 20)]
    #[serde(default = "default_max_failures")]
    pub max_failures: usize,
}

fn default_rps() -> f64 {
    1.0
}
fn default_max_mb() -> u64 {
    256
}
fn default_max_failures() -> usize {
    20
}

/// So [`crate::ops::run`] inherits the CLI's limits instead of restating them.
impl Default for CollectArgs {
    fn default() -> Self {
        Self {
            source: String::new(),
            limit: None,
            rps: default_rps(),
            refresh: false,
            matches: Vec::new(),
            max_mb: default_max_mb(),
            max_failures: default_max_failures(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CollectFailure {
    pub url: String,
    pub state: Liveness,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CollectReport {
    pub source: String,
    /// Addresses in the most recent DiscoveryRun.
    pub discovered: usize,
    /// Skipped because the store already had them.
    pub already_had: usize,
    /// Excluded by `--match`.
    pub filtered_out: usize,
    pub attempted: usize,
    pub stored: usize,
    /// Stored, and the content differed from the previous Observation.
    pub changed: usize,
    pub failed: usize,
    pub bytes: u64,
    /// Still uncollected. Non-zero means re-running continues where this stopped.
    pub remaining: usize,
    /// What was actually gathered — the input to planning extraction.
    pub by_kind: BTreeMap<String, usize>,
    pub failures: Vec<CollectFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failures_truncated: Option<usize>,
}

/// Fetch every address the latest discovery run found, skipping what is already stored.
#[op(long_running, group = "stage")]
pub async fn collect(
    ctx: &Ctx,
    args: CollectArgs,
    progress: &Progress,
) -> anyhow::Result<CollectReport> {
    let source = SourceId::new(args.source.clone())?;

    // One pass over the log for both the work-list and the resume state.
    let log = ctx.store.read_log(&source).await?;

    let discovered: Vec<Resource> = log
        .iter()
        .filter_map(|r| match r {
            LogRecord::DiscoveryRun(d) => Some(d.resources.clone()),
            _ => None,
        })
        .next_back()
        .unwrap_or_default();

    anyhow::ensure!(
        !discovered.is_empty(),
        "no discovery run for `{source}` — run `centinel discover --source {source} --site <url>` first"
    );

    let mut seen: HashMap<Resource, Fingerprint> = HashMap::new();
    let mut statuses = BTreeMap::new();
    for rec in &log {
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

    // ---- build the work list -------------------------------------------------------
    let mut filtered_out = 0usize;
    let mut already_had = 0usize;
    let mut todo: Vec<Resource> = Vec::new();

    for r in &discovered {
        if !args.matches.is_empty() && !args.matches.iter().any(|m| r.natural_key.contains(m)) {
            filtered_out += 1;
            continue;
        }
        if !args.refresh && seen.contains_key(r) {
            already_had += 1;
            continue;
        }
        todo.push(r.clone());
    }

    let total_todo = todo.len();
    if let Some(limit) = args.limit {
        todo.truncate(limit);
    }

    // ---- collect -------------------------------------------------------------------
    let policy = HostPolicy {
        max_requests_per_second: args.rps,
        ..Default::default()
    };
    let fetcher = Fetcher::new(&policy)?;

    // Per-host pacing, because a discovery run routinely spans hosts — hcfl.gov's
    // sitemap is advertised by hillsboroughcounty.org. One shared limiter would
    // needlessly throttle a second host; one per host would be a way to hammer them.
    let mut pacers: HashMap<String, Pacer> = HashMap::new();
    let max_bytes = args.max_mb.saturating_mul(1024 * 1024);

    let mut report = CollectReport {
        source: args.source.clone(),
        discovered: discovered.len(),
        already_had,
        filtered_out,
        attempted: 0,
        stored: 0,
        changed: 0,
        failed: 0,
        bytes: 0,
        remaining: 0,
        by_kind: BTreeMap::new(),
        failures: Vec::new(),
        failures_truncated: None,
    };

    let total = todo.len() as u64;
    for (i, resource) in todo.iter().enumerate() {
        let host = url::Url::parse(&resource.natural_key)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();

        pacers
            .entry(host.clone())
            .or_insert_with(|| Pacer::new(policy.min_interval(None)))
            .wait()
            .await;

        if i % 25 == 0 || i + 1 == todo.len() {
            progress.step(
                format!("{} stored, {} failed", report.stored, report.failed),
                i as u64,
                total,
            );
        }

        let at = Timestamp::now();
        report.attempted += 1;

        match fetcher.get(&resource.natural_key).await {
            Ok(fetched) => {
                if fetched.bytes.len() as u64 > max_bytes {
                    report.failed += 1;
                    push_failure(
                        &mut report,
                        args.max_failures,
                        CollectFailure {
                            url: resource.natural_key.clone(),
                            state: Liveness::Error,
                            detail: format!(
                                "body is {} MB, over --max-mb {}",
                                fetched.bytes.len() / (1024 * 1024),
                                args.max_mb
                            ),
                        },
                    );
                    continue;
                }

                let kind = content_kind(&fetched.meta, &fetched.bytes);
                *report.by_kind.entry(kind.to_string()).or_default() += 1;
                report.bytes += fetched.bytes.len() as u64;

                let obs = ctx
                    .store
                    .record_observation(resource, &fetched.bytes, at, fetched.meta)
                    .await?;

                // Compared against the preloaded map, not a fresh log scan.
                if seen.get(resource) != Some(&obs.fingerprint) {
                    report.changed += 1;
                }
                seen.insert(resource.clone(), obs.fingerprint);
                report.stored += 1;

                // A success clears any previous failure state.
                if let Some(st) = statuses.get_mut(resource)
                    && st.state != Liveness::Live
                {
                    st.apply(Liveness::Live, at, None);
                    ctx.store
                        .append(&source, &LogRecord::Status(st.clone()))
                        .await?;
                }
            }
            Err(failure) => {
                let st = statuses
                    .entry(resource.clone())
                    .or_insert_with(|| ResourceStatus::new_live(resource.clone(), at));
                st.apply(failure.state, at, Some(failure.detail.clone()));

                // No Observation — liveness carries the failure instead (§4.4).
                ctx.store
                    .append(&source, &LogRecord::Status(st.clone()))
                    .await?;

                report.failed += 1;
                push_failure(
                    &mut report,
                    args.max_failures,
                    CollectFailure {
                        url: resource.natural_key.clone(),
                        state: failure.state,
                        detail: failure.detail,
                    },
                );
            }
        }
    }

    report.remaining = total_todo.saturating_sub(report.stored);
    progress.step(
        format!("{} stored, {} failed", report.stored, report.failed),
        total,
        total,
    );
    Ok(report)
}

/// Keeps the failure list bounded. A wholesale WAF block would otherwise produce
/// thousands of identical lines and bury the count that matters.
fn push_failure(report: &mut CollectReport, max: usize, failure: CollectFailure) {
    if report.failures.len() < max {
        report.failures.push(failure);
    } else {
        *report.failures_truncated.get_or_insert(0) += 1;
    }
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// What the run did, what it left, and what refused it.
///
/// `remaining` is promoted out of the counter column into a line of its own, because it is
/// the only figure here that is an *instruction*: non-zero means running the same command
/// again continues from where this stopped, and a person who misses that re-crawls from
/// the beginning.
impl Render for CollectReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.title(&self.source, &render::bytes(self.bytes))?;
        p.nest(|p| {
            p.figures(&[
                (self.discovered as u64, "discovered"),
                (self.already_had as u64, "already had"),
                (self.filtered_out as u64, "filtered out"),
                (self.attempted as u64, "attempted"),
                (self.stored as u64, "stored"),
                (self.changed as u64, "changed"),
                (self.failed as u64, "failed"),
            ])?;

            if !self.by_kind.is_empty() {
                p.section("by kind")?;
                let mut table = Table::bare(&[Align::Right, Align::Left]);
                for (kind, n) in &self.by_kind {
                    table.push(vec![
                        Cell::new(render::count(*n as u64), Ink::Bold),
                        Cell::dim(kind),
                    ]);
                }
                p.table(&table)?;
            }

            if !self.failures.is_empty() {
                p.section("failures")?;
                for failure in &self.failures {
                    failure.render(p)?;
                }
                if let Some(more) = self.failures_truncated {
                    let text = format!("… and {} more", render::count(more as u64));
                    p.line(p.paint(&text, Ink::Dim))?;
                }
            }

            if self.remaining > 0 {
                p.blank()?;
                let text = format!(
                    "{} still uncollected — re-run to continue",
                    render::count(self.remaining as u64)
                );
                p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
            }
            Ok(())
        })
    }
}

impl Render for CollectFailure {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let mark = self.state.mark();
        let state = format!("{:<8}", self.state);
        let head = format!(
            "{}{}",
            p.paint(&state, mark.ink()),
            render::truncate(&self.url, p.width().saturating_sub(12)),
        );
        p.marked(mark, head)?;
        p.nest(|p| p.wrapped(&render::one_line(&self.detail), Ink::Dim))
    }
}
