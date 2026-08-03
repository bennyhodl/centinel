//! Site discovery: `robots.txt` → sitemaps → the full URL set.
//!
//! This is the layer that turns a bare domain into a [`crate::domain::DiscoveryRun`] —
//! a complete snapshot of what a site says it has.
//!
//! Discovery is deliberately **separate from fetching**. A run answers "what exists",
//! and answering it is cheap: `tampa.gov`'s entire 12,000-URL surface is seven HTTP
//! requests. Deciding which of those to collect, and how often, is a different problem.

pub mod robots;
pub mod sitemap;

use std::collections::HashSet;
use std::time::Duration;

use jiff::Timestamp;
use url::Url;

use crate::policy::{HostPolicy, Pacer};

pub use robots::{Robots, UnreachableRobots};
pub use sitemap::{SitemapDoc, SitemapEntry, SitemapRef};

/// Bounds on a discovery run.
///
/// Every one of these exists because a sitemap is attacker-or-accident controlled: a
/// self-referential index, a million-entry urlset, or a chain of redirects between
/// hosts are all things a real site has done by mistake.
#[derive(Clone, Debug)]
pub struct DiscoveryLimits {
    /// Total sitemap documents to fetch.
    pub max_sitemaps: usize,
    /// Index nesting depth. `index → index → urlset` is legal, so this is not 1.
    pub max_depth: usize,
    /// Total URLs to retain.
    pub max_urls: usize,
}

impl Default for DiscoveryLimits {
    fn default() -> Self {
        Self {
            // tampa.gov needs 7 (one index + six children). 200 leaves room for large
            // multi-department sites without permitting an unbounded walk.
            max_sitemaps: 200,
            max_depth: 5,
            max_urls: 500_000,
        }
    }
}

/// The result of one discovery pass.
#[derive(Clone, Debug, Default)]
pub struct SiteDiscovery {
    /// Every URL the site declares, deduplicated, in first-seen order.
    pub entries: Vec<SitemapEntry>,
    /// Sitemap documents actually fetched — provenance for a suspiciously small result.
    pub sitemaps_fetched: Vec<String>,
    /// False when `robots.txt` was unreachable and rules were assumed.
    pub robots_declared: bool,
    /// The host's declared `Crawl-delay`, if any.
    pub crawl_delay: Option<Duration>,
    /// URLs excluded by `robots.txt`.
    pub disallowed: usize,
    /// Non-fatal problems. A partial discovery with recorded warnings is far more
    /// useful than a hard failure, so nothing here aborts the run.
    pub warnings: Vec<String>,
    pub at: Option<Timestamp>,
}

/// Walks a site's declared surface.
pub struct Discoverer {
    client: reqwest::Client,
    policy: HostPolicy,
    limits: DiscoveryLimits,
}

impl Discoverer {
    pub fn new(policy: HostPolicy, limits: DiscoveryLimits) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(&policy.user_agent)
            .timeout(policy.timeout)
            .build()?;
        Ok(Self {
            client,
            policy,
            limits,
        })
    }

    /// Discovers everything `site_url`'s origin declares.
    ///
    /// Order of operations: `robots.txt` first (it names the sitemaps and the delay),
    /// then a breadth-first walk of every sitemap it points at.
    ///
    /// Progress matters here: at the default 1 req/sec a large site is minutes of
    /// apparent silence, and a caller cannot otherwise tell politeness from a hang.
    pub async fn discover(
        &self,
        site_url: &str,
        progress: &crate::op::Progress,
    ) -> anyhow::Result<SiteDiscovery> {
        let base = Url::parse(site_url)?;
        let mut out = SiteDiscovery {
            at: Some(Timestamp::now()),
            ..Default::default()
        };

        // ---- robots.txt ----------------------------------------------------------
        progress.say(format!(
            "reading robots.txt for {}",
            base.host_str().unwrap_or("?")
        ));
        let robots_url = base.join("/robots.txt")?;
        let robots = match self.get(robots_url.as_str()).await {
            Ok(body) => robots::Robots::parse(&self.policy.user_agent, &body),
            Err(e) => {
                // Measured on phila.gov: CloudFront 403 on robots.txt, 200 on the site.
                out.warnings
                    .push(format!("robots.txt unreachable ({e}); assuming no rules"));
                robots::Robots::unreachable(self.policy.unreachable_robots)
            }
        };
        out.robots_declared = robots.declared;
        out.crawl_delay = robots.crawl_delay();

        let pacer = Pacer::new(self.policy.min_interval(out.crawl_delay));

        // ---- seeds ---------------------------------------------------------------
        // Prefer what robots.txt declares; these routinely point at another host.
        let mut queue: Vec<(String, usize)> = robots
            .sitemaps()
            .iter()
            .map(|s| (s.clone(), 0usize))
            .collect();

        if queue.is_empty() {
            let guess = base.join("/sitemap.xml")?;
            out.warnings.push(format!(
                "robots.txt declared no sitemap; trying {guess} by convention"
            ));
            queue.push((guess.to_string(), 0));
        }

        // ---- breadth-first walk --------------------------------------------------
        let mut visited: HashSet<String> = HashSet::new();
        let mut seen_urls: HashSet<String> = HashSet::new();

        while let Some((loc, depth)) = queue.pop() {
            if out.sitemaps_fetched.len() >= self.limits.max_sitemaps {
                out.warnings.push(format!(
                    "stopped at max_sitemaps={}; the surface is larger than this run captured",
                    self.limits.max_sitemaps
                ));
                break;
            }
            // Loop protection. Self-referential indexes exist in the wild.
            if !visited.insert(loc.clone()) {
                continue;
            }
            if depth > self.limits.max_depth {
                out.warnings
                    .push(format!("depth limit reached, skipping {loc}"));
                continue;
            }

            progress.step(
                format!("sitemap {loc}"),
                out.sitemaps_fetched.len() as u64,
                (out.sitemaps_fetched.len() + queue.len() + 1) as u64,
            );

            pacer.wait().await;
            let body = match self.get(&loc).await {
                Ok(b) => b,
                Err(e) => {
                    out.warnings.push(format!("{loc}: {e}"));
                    continue;
                }
            };
            out.sitemaps_fetched.push(loc.clone());

            match sitemap::parse(&body) {
                Ok(SitemapDoc::Index(refs)) => {
                    for r in refs {
                        queue.push((r.loc, depth + 1));
                    }
                }
                Ok(SitemapDoc::UrlSet(entries)) => {
                    for e in entries {
                        if out.entries.len() >= self.limits.max_urls {
                            out.warnings
                                .push(format!("stopped at max_urls={}", self.limits.max_urls));
                            break;
                        }
                        if !robots.allowed(&e.loc) {
                            out.disallowed += 1;
                            continue;
                        }
                        // Dedup on the full URL *including* query string — stripping it
                        // would collapse distinct .gov agenda pages into one.
                        if seen_urls.insert(e.loc.clone()) {
                            out.entries.push(e);
                        }
                    }
                }
                Err(e) => out.warnings.push(format!("{loc}: {e}")),
            }
        }

        Ok(out)
    }

    /// A single GET, erroring on any non-success status.
    async fn get(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        let resp = self.client.get(url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("HTTP {status}");
        }
        Ok(resp.bytes().await?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_accommodate_the_measured_tampa_shape() {
        // One index plus six query-string children, ~2,000 URLs each.
        let l = DiscoveryLimits::default();
        assert!(l.max_sitemaps >= 7);
        assert!(l.max_depth >= 2, "index → index → urlset is legal");
        assert!(l.max_urls >= 12_000);
    }

    #[test]
    fn discovery_defaults_to_an_empty_but_valid_snapshot() {
        let d = SiteDiscovery::default();
        assert!(d.entries.is_empty());
        assert!(!d.robots_declared, "must not claim rules we never read");
    }
}
