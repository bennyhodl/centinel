//! Per-host crawl policy.
//!
//! The crawling research checked every candidate library in Rust, Python and TypeScript
//! and found **none** of them provide this. Politeness config — who we say we are, how
//! fast we go, how to reach us — is application code in every ecosystem. So it lives here.
//!
//! The defaults are not arbitrary. A descriptive User-Agent carrying a contact URL was
//! the single highest-yield politeness lever measured: it flipped `sec.gov` from 403 to
//! 200, and (verified during this build) `phila.gov` too.
//!
//! Ticket [#4](https://github.com/bennyhodl/centinel/issues/4) owns the fuller policy
//! question — robots stance, boundary rules, what is captured versus merely mapped.
//! This is the shape those decisions will fill in, not a claim to have made them.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::time::Duration;

use crate::discovery::robots::UnreachableRobots;

/// A descriptive User-Agent with a contact address.
pub const DEFAULT_USER_AGENT: &str =
    "Centinel/0.1 (civic transparency archiver; +https://github.com/bennyhodl/centinel)";

/// How to treat one host.
#[derive(Clone, Debug)]
pub struct HostPolicy {
    pub user_agent: String,
    /// Where an administrator should write if we are causing trouble. Folded into the
    /// User-Agent by default; separate here so it can be surfaced in logs and reports.
    pub contact: Option<String>,
    /// Ceiling on request rate. Deliberately slow — this is an archiver, not a race.
    pub max_requests_per_second: f64,
    /// Whether a declared `Crawl-delay` may slow us below `max_requests_per_second`.
    /// It may never speed us up.
    pub respect_crawl_delay: bool,
    /// What to do when `robots.txt` cannot be fetched at all.
    pub unreachable_robots: UnreachableRobots,
    pub timeout: Duration,
}

impl Default for HostPolicy {
    fn default() -> Self {
        Self {
            user_agent: DEFAULT_USER_AGENT.to_string(),
            contact: Some("https://github.com/bennyhodl/centinel".to_string()),
            // One request per second. Slow enough that no `.gov` administrator will
            // ever notice us, which is the entire objective.
            max_requests_per_second: 1.0,
            respect_crawl_delay: true,
            unreachable_robots: UnreachableRobots::Allow,
            timeout: Duration::from_secs(30),
        }
    }
}

impl HostPolicy {
    /// The minimum interval between requests, combining our cap with the host's wishes.
    ///
    /// Takes the **slower** of the two. A `Crawl-delay` can only ever slow us down —
    /// a host declaring `Crawl-delay: 0` does not license a flood.
    pub fn min_interval(&self, declared_delay: Option<Duration>) -> Duration {
        let ours = if self.max_requests_per_second > 0.0 {
            Duration::from_secs_f64(1.0 / self.max_requests_per_second)
        } else {
            Duration::from_secs(1)
        };
        match declared_delay.filter(|_| self.respect_crawl_delay) {
            Some(theirs) => ours.max(theirs),
            None => ours,
        }
    }
}

/// Policy for every host, with per-host overrides.
#[derive(Clone, Debug, Default)]
pub struct PolicyTable {
    default: HostPolicy,
    hosts: BTreeMap<String, HostPolicy>,
}

impl PolicyTable {
    pub fn new(default: HostPolicy) -> Self {
        Self {
            default,
            hosts: BTreeMap::new(),
        }
    }

    /// Overrides policy for one host. Host matching is exact and case-insensitive;
    /// there is no wildcard, because a wildcard that silently widened a rate cap
    /// across an entire TLD would be a hazard rather than a convenience.
    pub fn set(&mut self, host: impl Into<String>, policy: HostPolicy) -> &mut Self {
        self.hosts.insert(host.into().to_ascii_lowercase(), policy);
        self
    }

    pub fn for_host(&self, host: &str) -> &HostPolicy {
        self.hosts
            .get(&host.to_ascii_lowercase())
            .unwrap_or(&self.default)
    }
}

/// Paces requests to one host.
///
/// Wraps a GCRA limiter rather than sleeping a fixed interval, so a burst that arrives
/// after an idle period does not all fire at once.
pub struct Pacer {
    limiter: governor::DefaultDirectRateLimiter,
}

impl Pacer {
    pub fn new(interval: Duration) -> Self {
        // A zero or absurd interval would panic inside `Quota`; clamp instead, because
        // a misconfigured policy should crawl politely, not crash.
        let interval = interval.clamp(Duration::from_millis(1), Duration::from_secs(3600));
        let quota = governor::Quota::with_period(interval)
            .expect("interval is clamped above zero")
            .allow_burst(NonZeroU32::new(1).expect("1 is nonzero"));
        Self {
            limiter: governor::RateLimiter::direct(quota),
        }
    }

    /// Waits until the next request is permitted.
    pub async fn wait(&self) {
        self.limiter.until_ready().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_user_agent_is_descriptive_and_contactable() {
        assert!(DEFAULT_USER_AGENT.contains("Centinel"));
        assert!(
            DEFAULT_USER_AGENT.contains('+'),
            "a contact URL is what flips sec.gov and phila.gov from 403 to 200"
        );
    }

    #[test]
    fn crawl_delay_can_slow_us_but_never_speed_us_up() {
        let p = HostPolicy::default(); // 1 rps → 1s
        assert_eq!(p.min_interval(None), Duration::from_secs(1));
        assert_eq!(
            p.min_interval(Some(Duration::from_secs(10))),
            Duration::from_secs(10),
            "a slower declared delay wins"
        );
        assert_eq!(
            p.min_interval(Some(Duration::from_millis(1))),
            Duration::from_secs(1),
            "a faster declared delay must not raise our ceiling"
        );
    }

    #[test]
    fn crawl_delay_can_be_ignored_by_policy() {
        let p = HostPolicy {
            respect_crawl_delay: false,
            ..Default::default()
        };
        assert_eq!(
            p.min_interval(Some(Duration::from_secs(60))),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn per_host_override_applies_only_to_that_host() {
        let mut table = PolicyTable::default();
        table.set(
            "slow.gov",
            HostPolicy {
                max_requests_per_second: 0.1,
                ..Default::default()
            },
        );

        assert_eq!(table.for_host("slow.gov").max_requests_per_second, 0.1);
        assert_eq!(table.for_host("SLOW.GOV").max_requests_per_second, 0.1);
        assert_eq!(table.for_host("other.gov").max_requests_per_second, 1.0);
    }

    #[tokio::test]
    async fn pacer_enforces_the_interval() {
        let pacer = Pacer::new(Duration::from_millis(50));
        let start = std::time::Instant::now();
        pacer.wait().await; // burst of 1, immediate
        pacer.wait().await; // must wait
        assert!(
            start.elapsed() >= Duration::from_millis(40),
            "second request came too fast: {:?}",
            start.elapsed()
        );
    }
}
