//! `discover` — turn a domain into the full set of addresses it declares.
//!
//! This is the op that makes a corpus possible. Before it, collection meant typing URLs
//! by hand; after it, a `.gov` site's entire declared surface is one command and a
//! handful of polite requests.
//!
//! The output is a [`DiscoveryRun`] — a complete snapshot, not a delta. Snapshots are
//! what make a shrinking corpus visible: a run that finds 400 URLs where the last found
//! 12,000 means something broke, and that is a collection-quality signal worth having
//! even when nobody is asking questions about change.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::discovery::{Discoverer, DiscoveryLimits};
use crate::policy::HostPolicy;
use crate::prelude::*;
use crate::store::LogRecord;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct DiscoverArgs {
    /// Source id to file this run under, e.g. `tampa`.
    #[arg(long)]
    pub source: String,

    /// Any URL on the site. Only the origin is used.
    #[arg(long)]
    pub site: String,

    /// Requests per second. The default is deliberately slow.
    #[arg(long, default_value_t = 1.0)]
    #[serde(default = "default_rps")]
    pub rps: f64,

    /// Maximum sitemap documents to fetch.
    #[arg(long, default_value_t = 200)]
    #[serde(default = "default_max_sitemaps")]
    pub max_sitemaps: usize,

    /// Maximum URLs to retain.
    #[arg(long, default_value_t = 500_000)]
    #[serde(default = "default_max_urls")]
    pub max_urls: usize,

    /// How many discovered URLs to include in the report.
    #[arg(long, default_value_t = 10)]
    #[serde(default = "default_sample")]
    pub sample: usize,

    /// Discover without writing a DiscoveryRun to the log.
    #[arg(long)]
    #[serde(default)]
    pub dry_run: bool,
}

fn default_rps() -> f64 {
    1.0
}
fn default_max_sitemaps() -> usize {
    200
}
fn default_max_urls() -> usize {
    500_000
}
fn default_sample() -> usize {
    10
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DiscoverReport {
    pub source: String,
    pub site: String,
    pub urls_found: usize,
    /// Sitemap documents actually fetched — provenance for a small result.
    pub sitemaps_fetched: Vec<String>,
    /// False when `robots.txt` was unreachable and rules were assumed rather than read.
    pub robots_declared: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crawl_delay_secs: Option<f64>,
    /// URLs the site's own `robots.txt` told us not to fetch.
    pub disallowed: usize,
    /// Counted against the previous run. A large negative swing usually means a
    /// truncated crawl, not a shrinking website.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_run_urls: Option<usize>,
    pub warnings: Vec<String>,
    pub sample: Vec<String>,
    pub written_to_log: bool,
}

/// Discover every URL a site declares via robots.txt and its sitemaps.
#[op(long_running)]
pub async fn discover(
    ctx: &Ctx,
    args: DiscoverArgs,
    progress: &Progress,
) -> anyhow::Result<DiscoverReport> {
    let source = SourceId::new(args.source.clone())?;

    let policy = HostPolicy {
        max_requests_per_second: args.rps,
        ..Default::default()
    };
    let limits = DiscoveryLimits {
        max_sitemaps: args.max_sitemaps,
        max_urls: args.max_urls,
        ..Default::default()
    };

    let found = Discoverer::new(policy, limits)?
        .discover(&args.site, progress)
        .await?;

    // Compare against the previous snapshot before writing this one.
    let previous_run_urls = ctx
        .store
        .read_log(&source)
        .await?
        .iter()
        .filter_map(|r| match r {
            LogRecord::DiscoveryRun(d) => Some(d.resources.len()),
            _ => None,
        })
        .next_back();

    let resources: Vec<Resource> = found
        .entries
        .iter()
        .map(|e| Resource::new(source.clone(), e.loc.clone()))
        .collect();

    let sample = found
        .entries
        .iter()
        .take(args.sample)
        .map(|e| e.loc.clone())
        .collect();

    let written_to_log = if args.dry_run {
        false
    } else {
        let run = DiscoveryRun {
            source: source.clone(),
            at: found.at.unwrap_or_else(jiff::Timestamp::now),
            resources,
            method: "sitemap".to_string(),
        };
        ctx.store
            .append(&source, &LogRecord::DiscoveryRun(run))
            .await?;
        true
    };

    progress.say(format!("{} URLs discovered", found.entries.len()));

    Ok(DiscoverReport {
        source: args.source,
        site: args.site,
        urls_found: found.entries.len(),
        sitemaps_fetched: found.sitemaps_fetched,
        robots_declared: found.robots_declared,
        crawl_delay_secs: found.crawl_delay.map(|d| d.as_secs_f64()),
        disallowed: found.disallowed,
        previous_run_urls,
        warnings: found.warnings,
        sample,
        written_to_log,
    })
}
