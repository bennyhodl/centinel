//! `ingest` — fetch addresses into the store.
//!
//! This op is where three separate spec decisions become visible at once:
//!
//! - **An Observation always has bytes** (§4.4). A failed fetch appends no Observation;
//!   it mutates [`ResourceStatus`] instead.
//! - **`Blocked` is not `Gone`** (§4.4). A CloudFront/Akamai 403 is not evidence of
//!   absence, and conflating them would silently corrupt the record.
//! - **Two hashes** (§5.3). `blob_sha` proves what the server served; `fingerprint`
//!   answers whether it meaningfully changed.

use std::time::Duration;

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fetch::{FetchFailure, Fetcher};
use crate::policy::{DEFAULT_USER_AGENT, HostPolicy};
use crate::prelude::*;
use crate::store::LogRecord;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct IngestArgs {
    /// Source id to file these observations under, e.g. `hillsboroughcounty`.
    #[arg(long)]
    pub source: String,

    /// URL to fetch. Repeat for several.
    #[arg(long = "url", required = true, num_args = 1..)]
    pub urls: Vec<String>,

    /// User-Agent header. A descriptive one measurably reduces WAF 403s.
    #[arg(long, default_value = DEFAULT_USER_AGENT)]
    #[serde(default = "default_ua")]
    pub user_agent: String,

    /// Per-request timeout in seconds.
    #[arg(long, default_value_t = 30)]
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_ua() -> String {
    DEFAULT_USER_AGENT.to_string()
}

fn default_timeout() -> u64 {
    30
}

/// What happened at one address.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum IngestOutcome {
    /// Bytes retrieved and stored.
    Stored {
        url: String,
        blob_sha: String,
        fingerprint: String,
        bytes: usize,
        /// `false` when the fingerprint matched the previous Observation — archived
        /// faithfully, but not a change (§5.3).
        changed: bool,
        /// True when this address had never been observed before.
        first_seen: bool,
    },
    /// Fetch failed. No Observation was written; liveness was updated instead.
    Failed {
        url: String,
        state: Liveness,
        detail: String,
        consecutive_failures: u32,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct IngestReport {
    pub source: String,
    pub attempted: usize,
    pub stored: usize,
    pub changed: usize,
    pub failed: usize,
    pub outcomes: Vec<IngestOutcome>,
}

/// Fetch one or more URLs into the content-addressed store.
#[op(long_running, reach = "operator", group = "stage")]
pub async fn ingest(
    ctx: &Ctx,
    args: IngestArgs,
    progress: &Progress,
) -> anyhow::Result<IngestReport> {
    let source = SourceId::new(args.source.clone())?;

    let fetcher = Fetcher::new(&HostPolicy {
        user_agent: args.user_agent.clone(),
        timeout: Duration::from_secs(args.timeout_secs),
        ..Default::default()
    })?;

    // One replay, two questions. Liveness is read up front so `since` and
    // `consecutive_failures` carry across runs rather than resetting on every invocation;
    // the fingerprints are here for the same reason `acquire` preloads them — the
    // alternative is `Store::observe`, which reads the whole log again per URL, so a
    // hundred addresses read it a hundred times.
    let replay = ctx.store.replay(&source).await?;
    let mut statuses = replay.statuses();
    let mut fingerprints: std::collections::BTreeMap<Resource, Fingerprint> = replay
        .latest_observations()
        .into_iter()
        .map(|(resource, obs)| (resource, obs.fingerprint))
        .collect();

    let total = args.urls.len() as u64;
    let mut outcomes = Vec::with_capacity(args.urls.len());
    let (mut stored, mut changed, mut failed) = (0usize, 0usize, 0usize);

    for (i, url) in args.urls.iter().enumerate() {
        progress.step(format!("fetching {url}"), i as u64, total);

        let resource = Resource::new(source.clone(), url.clone());
        let at = Timestamp::now();

        match fetcher.get(url).await {
            Ok(Fetched { bytes, meta }) => {
                let n = bytes.len();
                let obs = ctx
                    .store
                    .record_observation(&resource, &bytes, at, meta)
                    .await?;

                // The map is the history: whatever it held for this address is what the
                // last run saw, and the new fingerprint replaces it for the next URL.
                let previous = fingerprints.insert(resource.clone(), obs.fingerprint.clone());
                let first_seen = previous.is_none();
                let did_change = previous.as_ref() != Some(&obs.fingerprint);

                // A success clears whatever failure state the address was in.
                statuses
                    .entry(resource.clone())
                    .and_modify(|s| s.apply(Liveness::Live, at, None))
                    .or_insert_with(|| ResourceStatus::new_live(resource.clone(), at));

                stored += 1;
                if did_change {
                    changed += 1;
                }
                outcomes.push(IngestOutcome::Stored {
                    url: url.clone(),
                    blob_sha: obs.blob_sha.to_string(),
                    fingerprint: obs.fingerprint.to_string(),
                    bytes: n,
                    changed: did_change,
                    first_seen,
                });
            }
            Err(FetchFailure { state, detail }) => {
                let entry = statuses
                    .entry(resource.clone())
                    .or_insert_with(|| ResourceStatus::new_live(resource.clone(), at));
                entry.apply(state, at, Some(detail.clone()));
                let consecutive_failures = entry.consecutive_failures;

                // No Observation — liveness carries the failure instead (§4.4).
                ctx.store
                    .append(&source, &LogRecord::Status(entry.clone()))
                    .await?;

                failed += 1;
                outcomes.push(IngestOutcome::Failed {
                    url: url.clone(),
                    state,
                    detail,
                    consecutive_failures,
                });
            }
        }
    }

    progress.step("done", total, total);

    Ok(IngestReport {
        source: args.source,
        attempted: args.urls.len(),
        stored,
        changed,
        failed,
        outcomes,
    })
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// The four counters, then one line per URL.
///
/// `changed` is the counter worth looking at. `stored` says bytes arrived; `changed` says
/// the record moved — and the gap between them is §5.3's whole point, that a re-fetch of
/// an unchanged page is archived faithfully without being an event.
impl Render for IngestReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.title(&self.source, "")?;
        p.nest(|p| {
            p.figures(&[
                (self.attempted as u64, "attempted"),
                (self.stored as u64, "stored"),
                (self.changed as u64, "changed"),
                (self.failed as u64, "failed"),
            ])?;

            if self.outcomes.is_empty() {
                return Ok(());
            }
            p.blank()?;
            for outcome in &self.outcomes {
                outcome.render(p)?;
            }
            Ok(())
        })
    }
}

impl Render for IngestOutcome {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        match self {
            IngestOutcome::Stored {
                url,
                blob_sha,
                bytes,
                changed,
                first_seen,
                ..
            } => {
                // An unchanged re-fetch is a success that did nothing, and dimming it is
                // how a run over a hundred stable pages shows its six real changes.
                let ink = if *changed { Ink::Plain } else { Ink::Dim };
                let head = p.paint(&render::truncate(url, p.width().saturating_sub(4)), ink);
                p.marked(Mark::Ok, head)?;
                let note = format!(
                    "{}  {}  {}",
                    render::short_sha(blob_sha),
                    render::bytes(*bytes as u64),
                    if *first_seen {
                        "first seen"
                    } else if *changed {
                        "changed"
                    } else {
                        "unchanged"
                    },
                );
                p.nest(|p| p.line(p.paint(&note, Ink::Dim)))
            }
            IngestOutcome::Failed {
                url,
                state,
                detail,
                consecutive_failures,
            } => {
                let mark = state.mark();
                let state_text = format!("{state:<8}");
                let head = format!(
                    "{}{}",
                    p.paint(&state_text, mark.ink()),
                    render::truncate(url, p.width().saturating_sub(12)),
                );
                p.marked(mark, head)?;
                let note = format!(
                    "{} · {}",
                    render::plural(*consecutive_failures as usize, "failure", "failures"),
                    render::one_line(detail),
                );
                p.nest(|p| p.wrapped(&note, Ink::Dim))
            }
        }
    }
}
