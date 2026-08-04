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

/// So a caller inside the library — [`crate::ops::run`] — gets the same limits the CLI
/// does, rather than a second set that drifts from the `default_value_t` above.
impl Default for DiscoverArgs {
    fn default() -> Self {
        Self {
            source: String::new(),
            site: String::new(),
            rps: default_rps(),
            max_sitemaps: default_max_sitemaps(),
            max_urls: default_max_urls(),
            sample: default_sample(),
            dry_run: false,
        }
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crawl_delay_secs: Option<f64>,
    /// URLs the site's own `robots.txt` told us not to fetch.
    pub disallowed: usize,
    /// Counted against the previous run. A large negative swing usually means a
    /// truncated crawl, not a shrinking website.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_run_urls: Option<usize>,
    pub warnings: Vec<String>,
    pub sample: Vec<String>,
    pub written_to_log: bool,
}

/// Discover every URL a site declares via robots.txt and its sitemaps.
#[op(long_running, group = "stage")]
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

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// The URL count, and everything that would explain a wrong one.
///
/// A discovery run is trusted or it is not, and the fields that decide that are all
/// negative space: whether `robots.txt` was actually read or merely assumed, how the count
/// moved against the previous run, what the site told us not to fetch. A large negative
/// swing usually means a truncated crawl rather than a shrinking website, so the delta is
/// rendered as a warning rather than as a number.
impl Render for DiscoverReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.title(&self.source, &self.site)?;
        p.nest(|p| {
            let count = p.paint(&render::count(self.urls_found as u64), Ink::Bold);
            let label = p.paint("URLs discovered", Ink::Dim);
            p.line(format!("{count} {label}"))?;

            if let Some(previous) = self.previous_run_urls {
                let delta = self.urls_found as i64 - previous as i64;
                let text = format!(
                    "{}{} against the previous run's {}",
                    if delta >= 0 { "+" } else { "" },
                    delta,
                    render::count(previous as u64),
                );
                // A shrinking crawl is the signature of a truncated one, so it is amber
                // rather than a neutral figure.
                if delta < 0 {
                    p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
                } else {
                    p.line(p.paint(&text, Ink::Dim))?;
                }
            }

            p.blank()?;
            p.marked(
                Mark::from_ok(self.robots_declared),
                p.paint(
                    if self.robots_declared {
                        "robots.txt read"
                    } else {
                        "robots.txt unreachable — rules assumed, not read"
                    },
                    Ink::Dim,
                ),
            )?;
            if self.disallowed > 0 {
                let text = format!(
                    "{} excluded by the site's own rules",
                    render::count(self.disallowed as u64)
                );
                p.line(format!("  {}", p.paint(&text, Ink::Dim)))?;
            }
            if let Some(delay) = self.crawl_delay_secs {
                let text = format!("crawl-delay {delay}s");
                p.line(format!("  {}", p.paint(&text, Ink::Dim)))?;
            }
            p.marked(
                Mark::from_ok(self.written_to_log),
                p.paint(
                    if self.written_to_log {
                        "written to the log"
                    } else {
                        "not written — preview only"
                    },
                    Ink::Dim,
                ),
            )?;

            if !self.sitemaps_fetched.is_empty() {
                p.section("sitemaps")?;
                for sitemap in &self.sitemaps_fetched {
                    let text = render::truncate_start(sitemap, p.width());
                    p.line(p.paint(&text, Ink::Dim))?;
                }
            }

            for warning in &self.warnings {
                p.marked(Mark::Warn, p.paint(&render::one_line(warning), Ink::Dim))?;
            }

            if !self.sample.is_empty() {
                p.section("sample")?;
                for url in &self.sample {
                    let text = render::truncate_start(url, p.width());
                    p.line(p.paint(&text, Ink::Dim))?;
                }
            }
            Ok(())
        })
    }
}
