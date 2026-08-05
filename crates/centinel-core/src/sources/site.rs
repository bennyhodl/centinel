//! A crawled website as a [`Source`].
//!
//! Enumeration is `robots.txt` → sitemaps → the declared URL set; acquisition is an HTTP
//! GET. Both were already implemented in [`crate::discovery`] and [`crate::fetch`] — this
//! is the ~120 lines that put them behind the trait, which is roughly the amount of code
//! `collect` and `discover` used to spend knowing they were talking to a website.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;

use crate::discovery::{Discoverer, DiscoveryLimits};
use crate::domain::{
    Acquired, Enumeration, Note, NoteMark, Refusal, Resource, Source, SourceId, SourceKind,
};
use crate::fetch::Fetcher;
use crate::op::Progress;
use crate::policy::{HostPolicy, Pacer};

pub struct SiteSource {
    id: SourceId,
    /// Any URL on the site. Only the origin is used.
    site: String,
    policy: HostPolicy,
    discoverer: Discoverer,
    fetcher: Fetcher,
    /// One limiter per host, created on first sight.
    ///
    /// Per host and not per run, because a discovery run routinely spans hosts —
    /// `hcfl.gov`'s sitemap is advertised by `hillsboroughcounty.org`. One shared limiter
    /// would needlessly throttle the second host; no limiter would be a way to hammer
    /// both. Held in a `Mutex` rather than threaded through the caller because pacing is
    /// this Source's business, not its caller's.
    pacers: Mutex<HashMap<String, Arc<Pacer>>>,
}

impl SiteSource {
    pub fn new(
        id: SourceId,
        site: impl Into<String>,
        policy: HostPolicy,
        limits: DiscoveryLimits,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            id,
            site: site.into(),
            fetcher: Fetcher::new(&policy)?,
            discoverer: Discoverer::new(policy.clone(), limits)?,
            policy,
            pacers: Mutex::new(HashMap::new()),
        })
    }

    /// The limiter for a URL's host, created on first sight.
    fn pacer_for(&self, url: &str) -> Arc<Pacer> {
        let host = url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        let mut pacers = self.pacers.lock().expect("pacer map is never poisoned");
        Arc::clone(
            pacers
                .entry(host)
                .or_insert_with(|| Arc::new(Pacer::new(self.policy.min_interval(None)))),
        )
    }
}

impl Source for SiteSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn kind(&self) -> SourceKind {
        SourceKind::Site
    }

    fn method(&self) -> &'static str {
        "sitemap"
    }

    fn target(&self) -> &str {
        &self.site
    }

    fn enumerate<'a>(
        &'a self,
        progress: &'a Progress,
    ) -> BoxFuture<'a, anyhow::Result<Enumeration>> {
        Box::pin(async move {
            let found = self.discoverer.discover(&self.site, progress).await?;

            // Everything that would explain a wrong count, stated as provenance rather
            // than left for the reader to reconstruct from a bare number.
            let mut notes = vec![Note::ok_or_warn(
                "robots.txt",
                if found.robots_declared {
                    "read".to_string()
                } else {
                    "unreachable — rules were assumed, not read".to_string()
                },
                found.robots_declared,
            )];
            if let Some(delay) = found.crawl_delay {
                notes.push(Note::new(
                    "crawl-delay",
                    format!("{}s declared by the host", delay.as_secs_f64()),
                ));
            }
            if found.disallowed > 0 {
                notes.push(Note::marked(
                    "disallowed",
                    format!(
                        "{} excluded by the site's own rules",
                        crate::render::count(found.disallowed as u64)
                    ),
                    NoteMark::Ok,
                ));
            }
            for sitemap in &found.sitemaps_fetched {
                notes.push(Note::new("sitemap", sitemap));
            }

            let figures = BTreeMap::from([
                ("disallowed".to_string(), found.disallowed as u64),
                (
                    "sitemaps_fetched".to_string(),
                    found.sitemaps_fetched.len() as u64,
                ),
                ("robots_declared".to_string(), found.robots_declared as u64),
            ]);

            Ok(Enumeration {
                resources: found
                    .entries
                    .iter()
                    .map(|e| Resource::new(self.id.clone(), e.loc.clone()))
                    .collect(),
                warnings: found.warnings,
                notes,
                figures,
            })
        })
    }

    fn acquire<'a>(
        &'a self,
        resource: &'a Resource,
        _progress: &'a Progress,
    ) -> BoxFuture<'a, Result<Vec<Acquired>, Refusal>> {
        Box::pin(async move {
            self.pacer_for(&resource.natural_key).wait().await;
            let fetched = self.fetcher.get(&resource.natural_key).await?;
            // A page is one artifact at its own address.
            Ok(vec![Acquired {
                resource: resource.clone(),
                fetched,
            }])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> SiteSource {
        SiteSource::new(
            SourceId::new("tampa").unwrap(),
            "https://www.tampa.gov",
            HostPolicy::default(),
            DiscoveryLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn a_site_declares_what_it_is_without_being_asked_to_crawl() {
        let s = source();
        assert_eq!(s.kind(), SourceKind::Site);
        assert_eq!(s.method(), "sitemap");
        assert_eq!(s.target(), "https://www.tampa.gov");
        assert!(!s.yields_audio(), "a crawled site never produces audio");
    }

    /// A page is collected when the page is observed — there is no sub-address to key on.
    #[test]
    fn the_marker_for_a_page_is_the_page() {
        let s = source();
        let r = Resource::new(s.id().clone(), "https://www.tampa.gov/a");
        assert_eq!(s.marker(&r), r);
    }

    /// One limiter per host, so a sitemap that spans hosts neither throttles the second
    /// nor hammers it.
    #[test]
    fn pacing_is_per_host_and_shared_across_calls() {
        let s = source();
        let a1 = s.pacer_for("https://www.tampa.gov/one");
        let a2 = s.pacer_for("https://www.tampa.gov/two");
        let b = s.pacer_for("https://hcfl.gov/one");

        assert!(Arc::ptr_eq(&a1, &a2), "same host must share a limiter");
        assert!(!Arc::ptr_eq(&a1, &b), "a second host gets its own");
    }

    /// A natural key that is not a URL must not panic the pacer — it gets the empty-host
    /// limiter and the fetch fails on its own terms.
    #[test]
    fn an_unparseable_address_still_paces() {
        let s = source();
        let _ = s.pacer_for("not a url");
    }
}
