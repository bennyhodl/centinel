//! `list` — what is in the store, and what state it is in.
//!
//! Reads only truth (`log/`), never a derived index. That is deliberate: it means this
//! op still works with `centinel.db` and `index/` deleted, which is the property SPEC
//! §5 claims and this is the cheapest place to keep honest.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SourceSummary {
    pub source: String,
    pub resources: usize,
    pub observations: usize,
    /// Liveness counts, keyed by `live` / `gone` / `blocked` / `error`.
    pub liveness: BTreeMap<String, usize>,
    /// Addresses not currently `Live`. Capped by `--max-problems`, because a source
    /// that WAF-blocked wholesale would otherwise dump thousands of identical lines.
    pub problems: Vec<Problem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problems_truncated: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Problem {
    pub natural_key: String,
    pub state: Liveness,
    pub since: String,
    pub consecutive_failures: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListReport {
    pub store_root: String,
    pub sources: Vec<SourceSummary>,
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct ListArgs {
    /// Limit to one source. Omit to list all.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,

    /// Maximum non-live addresses to enumerate per source.
    #[arg(long, default_value_t = 20)]
    #[serde(default = "default_max_problems")]
    pub max_problems: usize,
}

fn default_max_problems() -> usize {
    20
}

/// List sources in the store with resource counts and liveness.
#[op]
pub async fn list(ctx: &Ctx, args: ListArgs) -> anyhow::Result<ListReport> {
    let sources = match &args.source {
        Some(s) => vec![SourceId::new(s.clone())?],
        None => ctx.store.sources().await?,
    };

    let mut out = Vec::with_capacity(sources.len());
    for source in sources {
        let statuses = ctx.store.statuses(&source).await?;
        let observations = ctx
            .store
            .read_log(&source)
            .await?
            .iter()
            .filter(|r| matches!(r, LogRecord::Observation(_)))
            .count();

        let mut liveness: BTreeMap<String, usize> = BTreeMap::new();
        let mut problems = Vec::new();
        for status in statuses.values() {
            *liveness.entry(status.state.to_string()).or_default() += 1;
            if status.state != Liveness::Live {
                problems.push(Problem {
                    natural_key: status.resource.natural_key.clone(),
                    state: status.state,
                    since: status.since.to_string(),
                    consecutive_failures: status.consecutive_failures,
                    detail: status.detail.clone(),
                });
            }
        }

        // Longest-standing failures first — a page blocked for a month matters more
        // than one that failed in this run.
        problems.sort_by(|a, b| {
            b.consecutive_failures
                .cmp(&a.consecutive_failures)
                .then(a.natural_key.cmp(&b.natural_key))
        });
        let total_problems = problems.len();
        let truncated = total_problems.saturating_sub(args.max_problems);
        problems.truncate(args.max_problems);

        out.push(SourceSummary {
            source: source.to_string(),
            resources: statuses.len(),
            observations,
            liveness,
            problems,
            problems_truncated: (truncated > 0).then_some(truncated),
        });
    }

    Ok(ListReport {
        store_root: ctx.store.root().display().to_string(),
        sources: out,
    })
}
