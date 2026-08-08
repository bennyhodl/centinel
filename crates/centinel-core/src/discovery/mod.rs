//! The two documents a site publishes about itself: `robots.txt` and `sitemap.xml`.
//!
//! Parsing only. Nothing here fetches — [`crate::strategies::sitemap`] owns the walk, and
//! it fetches through [`crate::strategies::Crawl`] so that pacing and the request budget
//! stay with the host.
//!
//! Discovery is deliberately **separate from fetching**. A run answers "what exists", and
//! answering it is cheap: `tampa.gov`'s entire 12,000-URL surface is seven HTTP requests.
//! Deciding which of those to collect, and how often, is a different problem.
//!
//! ## Where `Discoverer` went
//!
//! It used to live here and hold a `reqwest::Client`, a `Pacer` and the walk. The walk is
//! now `strategies::sitemap`, which is the same breadth-first traversal with the same
//! limits and the same loop protection — and keeping a second copy of it here would have
//! been the fork cost the strategy registry exists to avoid, shipped on day one.

pub mod robots;
pub mod sitemap;

pub use robots::{Robots, UnreachableRobots};
pub use sitemap::{SitemapDoc, SitemapEntry, SitemapRef};

/// Bounds on one enumeration pass.
///
/// Every one of these exists because a sitemap is attacker-or-accident controlled: a
/// self-referential index, a million-entry urlset, or a chain of redirects between hosts
/// are all things a real site has done by mistake.
#[derive(Clone, Debug)]
pub struct DiscoveryLimits {
    /// Requests one enumeration may spend.
    ///
    /// Named for sitemaps because the sitemap walk was the only strategy that existed
    /// when `--max-sitemaps` was added, and it has always meant this. The flag keeps the
    /// name: one sitting in somebody's cron entry is worth more than the tidier word.
    pub max_sitemaps: usize,
    /// Total URLs to retain.
    pub max_urls: usize,
}

impl Default for DiscoveryLimits {
    fn default() -> Self {
        Self {
            // tampa.gov needs 7 (one index + six children). 200 leaves room for large
            // multi-department sites without permitting an unbounded walk.
            max_sitemaps: 200,
            max_urls: 500_000,
        }
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
        assert!(l.max_urls >= 12_000);
    }
}
