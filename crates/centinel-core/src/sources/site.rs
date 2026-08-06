//! A crawled website as a [`Source`].
//!
//! Enumeration is `robots.txt` → sitemaps → the declared URL set; acquisition is an HTTP
//! GET. Both were already implemented in [`crate::discovery`] and [`crate::fetch`] — this
//! is the ~120 lines that put them behind the trait, which is roughly the amount of code
//! `collect` and `discover` used to spend knowing they were talking to a website.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;

use crate::discovery::{Discoverer, DiscoveryLimits};
use crate::domain::{
    Acquired, Enumeration, Note, NoteMark, Refusal, Resource, Source, SourceId, SourceKind,
};
use crate::enclosure;
use crate::fetch::Fetcher;
use crate::op::{ItemOutcome, Progress, Verdict};
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
    /// Documents that refused, so a run can say so without failing over them.
    partial: Mutex<Vec<String>>,
    enclosed: AtomicUsize,
    dropped: AtomicUsize,
}

/// How many refused documents a report names before it stops listing them.
const MAX_REMARKS: usize = 10;

/// The status code out of a `Fetcher` refusal, when it carried one.
///
/// [`crate::fetch`] renders a non-success as `HTTP 404 Not Found`, so the code is
/// recoverable — and worth recovering, because "the server answered 404" and "we never
/// reached the server" are different facts that a bare message would show alike.
fn status_in(detail: &str) -> Option<u16> {
    detail
        .strip_prefix("HTTP ")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
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
            partial: Mutex::new(Vec::new()),
            enclosed: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
        })
    }

    fn note_partial(&self, detail: String) {
        let mut partial = self.partial.lock().expect("remark list is never poisoned");
        if partial.len() < MAX_REMARKS {
            partial.push(detail);
        }
    }

    /// Paces, fetches, and reports the outcome either way.
    ///
    /// The pacer wait is inside the timing on purpose. At one request per second the wait
    /// *is* most of the elapsed time, and a duration that excluded it would show a run
    /// sprinting while the operator watched it sit still for two hours.
    async fn fetch_reporting(
        &self,
        url: &str,
        enclosed: bool,
        progress: &Progress,
    ) -> Result<crate::domain::Fetched, Refusal> {
        let started = std::time::Instant::now();
        self.pacer_for(url).wait().await;
        let result = self.fetcher.get(url).await;
        let millis = started.elapsed().as_millis() as u64;

        progress.item(match &result {
            Ok(fetched) => ItemOutcome {
                address: url.to_string(),
                tag: fetched
                    .meta
                    .get("http_status")
                    .cloned()
                    .unwrap_or_else(|| "200".into()),
                verdict: Verdict::Ok,
                noun: "requests".into(),
                bytes: fetched.bytes.len() as u64,
                produced: None,
                millis,
                detail: None,
                nested: enclosed,
            },
            Err(refusal) => {
                // `HTTP 404` is a status; a timeout or a DNS failure is not, and the two
                // must not be shown as though they were the same kind of answer.
                let status = status_in(&refusal.detail);
                ItemOutcome {
                    address: url.to_string(),
                    tag: status.map_or_else(|| "—".into(), |s| s.to_string()),
                    // A 429 is the host asking for room, and a 4xx is an address that is
                    // gone; neither is the run breaking. A 5xx or no answer at all is.
                    verdict: match status {
                        // Gone, and that is a fact about the address. Routine on a corpus
                        // full of links to files migrated years ago.
                        Some(s) if (400..500).contains(&s) && s != 429 => Verdict::Missing,
                        // A 429, a 5xx, or no answer at all is about this run.
                        _ => Verdict::Fail,
                    },
                    noun: "requests".into(),
                    bytes: 0,
                    produced: None,
                    millis,
                    detail: Some(refusal.detail.clone()),
                    nested: enclosed,
                }
            }
        });

        result
    }

    /// The documents a fetched page encloses, and nothing for anything that is not a page.
    ///
    /// Gated on the content kind rather than on the extension in the URL: a `.gov` CMS
    /// serves plenty of HTML from paths that end in neither `.html` nor a slash, and a PDF
    /// must never be scanned as though it were markup.
    fn enclosures(&self, fetched: &crate::domain::Fetched, base: &str) -> Vec<String> {
        if crate::fetch::content_kind(&fetched.meta, &fetched.bytes) != "html" {
            return Vec::new();
        }
        let html = String::from_utf8_lossy(&fetched.bytes);
        let found = enclosure::documents(&html, base, enclosure::MAX_PER_PAGE);

        self.enclosed.fetch_add(found.urls.len(), Ordering::Relaxed);
        self.dropped.fetch_add(found.dropped, Ordering::Relaxed);
        found.urls
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

    /// The page, and any document it encloses.
    ///
    /// A page used to be one artifact, which is true right up until the page is a wrapper
    /// around a PDF its CMS renders in a viewer. Then the bytes we stored are a date and a
    /// print notice, the document is at an address nothing fetched, and the corpus holds a
    /// page that looks collected and carries nothing.
    ///
    /// So a page is now up to `1 + n` artifacts, which is the shape a video has had all
    /// along — one address holding metadata, captions and audio. The marker stays the page
    /// ([`Source::marker`]'s default), so a document that 404s does not make the run
    /// re-fetch the page forever; it is recorded as a remark and the page stands.
    fn acquire<'a>(
        &'a self,
        resource: &'a Resource,
        progress: &'a Progress,
    ) -> BoxFuture<'a, Result<Vec<Acquired>, Refusal>> {
        Box::pin(async move {
            // Reported before it is returned, so a refused page is a line in the stream
            // like any other rather than a gap the operator has to infer.
            let page = self
                .fetch_reporting(&resource.natural_key, false, progress)
                .await;
            let fetched = page?;

            // Where the bytes actually came from, so a document relative to a redirected
            // page resolves against the page it really is.
            let base = fetched
                .meta
                .get("final_url")
                .cloned()
                .unwrap_or_else(|| resource.natural_key.clone());
            let enclosed = self.enclosures(&fetched, &base);

            let mut out = vec![Acquired {
                resource: resource.clone(),
                fetched,
            }];

            for url in enclosed {
                match self.fetch_reporting(&url, true, progress).await {
                    Ok(document) => out.push(Acquired {
                        resource: Resource::new(self.id.clone(), url),
                        fetched: document,
                    }),
                    // A document that refuses is evidence about the document, not about
                    // the page that named it. One site's broken attachment must not cancel
                    // the page it hangs off, for the same reason one source's WAF block
                    // does not cancel the nineteen behind it.
                    Err(refusal) => self.note_partial(format!("{url} — {}", refusal.detail)),
                }
            }

            Ok(out)
        })
    }

    fn remarks(&self, _parts: &BTreeMap<String, usize>, _attempted: usize) -> Vec<Note> {
        let partial = self.partial.lock().expect("remark list is never poisoned");
        let mut notes = Vec::new();

        let enclosed = self.enclosed.load(Ordering::Relaxed);
        if enclosed > 0 {
            notes.push(Note::new(
                "documents",
                format!(
                    "{} fetched from inside pages",
                    crate::render::count(enclosed as u64)
                ),
            ));
        }

        // A cap that says nothing reads exactly like a page that had nothing to give.
        let dropped = self.dropped.load(Ordering::Relaxed);
        if dropped > 0 {
            notes.push(Note::marked(
                "documents",
                format!(
                    "{dropped} past the {} per page were left to the sitemap",
                    enclosure::MAX_PER_PAGE
                ),
                NoteMark::Warn,
            ));
        }

        for detail in partial.iter() {
            notes.push(Note::marked("document", detail.clone(), NoteMark::Warn));
        }
        notes
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
