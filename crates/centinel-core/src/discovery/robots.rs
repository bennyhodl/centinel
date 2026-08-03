//! `robots.txt` handling.
//!
//! Thin wrapper over [`texting_robots`] — the same parser Firecrawl's Rust crawl core
//! uses, chosen for its stated goal of *"a thorough test suite tested against real world
//! data across millions of sites"*.
//!
//! Two things the crate deliberately leaves to the caller, both handled here:
//!
//! 1. **It does not fetch or cache.** Fetching lives in [`super::Discoverer`].
//! 2. **It does not strip a UTF-8 BOM.** A BOM before `User-agent:` becomes part of the
//!    first token and silently voids the whole group, so it is stripped below.

use std::time::Duration;

use texting_robots::Robot;

/// What to do when `robots.txt` cannot be fetched.
///
/// This is a real decision, not a detail: `phila.gov/robots.txt` returns a CloudFront
/// **403** while the site itself serves 200. Treating an unreachable `robots.txt` as
/// "disallow everything" would silently collect nothing from exactly the hosts most
/// worth watching.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnreachableRobots {
    /// Proceed as if no rules were declared. RFC 9309's guidance for 4xx.
    #[default]
    Allow,
    /// Collect nothing. RFC 9309 permits this for 5xx ("unavailable" status).
    Deny,
}

/// Parsed rules for one host, as they apply to **our** user-agent.
#[derive(Debug)]
pub struct Robots {
    robot: Option<Robot>,
    fallback: UnreachableRobots,
    /// True when rules were actually parsed, rather than assumed.
    pub declared: bool,
}

impl Robots {
    /// Parses `robots.txt` bytes for the given agent.
    ///
    /// Takes bytes, not `&str`, because some hosts return non-text here — the correct
    /// signature for reality rather than for the spec.
    pub fn parse(agent: &str, body: &[u8]) -> Self {
        let body = body.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(body);
        match Robot::new(agent, body) {
            Ok(robot) => Self {
                robot: Some(robot),
                fallback: UnreachableRobots::Allow,
                declared: true,
            },
            // A malformed robots.txt is not permission to ignore the host's wishes, but
            // it is also not a reason to abandon the crawl. Treat it as undeclared.
            Err(e) => {
                tracing::warn!(error = %e, "robots.txt did not parse; treating as undeclared");
                Self::unreachable(UnreachableRobots::Allow)
            }
        }
    }

    /// Rules for a host whose `robots.txt` could not be fetched.
    pub fn unreachable(fallback: UnreachableRobots) -> Self {
        Self {
            robot: None,
            fallback,
            declared: false,
        }
    }

    /// Whether we may fetch this URL.
    pub fn allowed(&self, url: &str) -> bool {
        match &self.robot {
            Some(r) => r.allowed(url),
            None => self.fallback == UnreachableRobots::Allow,
        }
    }

    /// The host's declared `Crawl-delay`, if any.
    ///
    /// Honoured as a **floor** on the request interval — a host asking to be crawled
    /// slowly gets that, even when our own rate cap would allow faster.
    pub fn crawl_delay(&self) -> Option<Duration> {
        self.robot
            .as_ref()
            .and_then(|r| r.delay)
            .filter(|d| d.is_finite() && *d > 0.0)
            .map(Duration::from_secs_f32)
    }

    /// `Sitemap:` lines. These are the entry point for discovery, and they routinely
    /// point at a **different host** than the one serving `robots.txt`.
    pub fn sitemaps(&self) -> &[String] {
        self.robot.as_ref().map(|r| &r.sitemaps[..]).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "Centinel";

    #[test]
    fn reads_sitemaps_including_cross_host_ones() {
        // The shape measured at hillsboroughcounty.org: the sitemap lives on hcfl.gov.
        let txt = b"User-agent: *\nAllow: /\nSitemap: https://hcfl.gov/sitemap\n";
        let r = Robots::parse(UA, txt);
        assert_eq!(r.sitemaps(), &["https://hcfl.gov/sitemap".to_string()]);
        assert!(r.declared);
    }

    #[test]
    fn honours_disallow_for_our_agent() {
        let txt = b"User-agent: *\nDisallow: /private/\n";
        let r = Robots::parse(UA, txt);
        assert!(r.allowed("https://x.gov/public/page"));
        assert!(!r.allowed("https://x.gov/private/page"));
    }

    #[test]
    fn reads_crawl_delay_as_a_floor() {
        let txt = b"User-agent: *\nCrawl-delay: 10\nDisallow:\n";
        assert_eq!(
            Robots::parse(UA, txt).crawl_delay(),
            Some(Duration::from_secs(10))
        );
    }

    #[test]
    fn absent_crawl_delay_is_none_not_zero() {
        let txt = b"User-agent: *\nDisallow:\n";
        assert!(Robots::parse(UA, txt).crawl_delay().is_none());
    }

    /// The documented `texting_robots` gap: it does not strip a BOM, so without our
    /// own strip the first `User-agent` token is `\u{feff}User-agent` and the group is
    /// silently ignored — meaning a real `Disallow` would be missed.
    #[test]
    fn strips_the_utf8_bom_before_parsing() {
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(b"User-agent: *\nDisallow: /private/\n");

        let r = Robots::parse(UA, &with_bom);
        assert!(
            !r.allowed("https://x.gov/private/page"),
            "a BOM must not void the rules"
        );
    }

    #[test]
    fn unreachable_robots_defaults_to_allow() {
        // phila.gov serves a WAF 403 on robots.txt while the site itself returns 200.
        let r = Robots::unreachable(UnreachableRobots::Allow);
        assert!(r.allowed("https://www.phila.gov/anything"));
        assert!(!r.declared, "we must not claim rules we never read");
        assert!(r.sitemaps().is_empty());
    }

    #[test]
    fn unreachable_robots_can_be_configured_to_deny() {
        let r = Robots::unreachable(UnreachableRobots::Deny);
        assert!(!r.allowed("https://www.phila.gov/anything"));
    }
}
