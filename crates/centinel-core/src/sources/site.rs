//! A crawled website as a [`Source`].
//!
//! Enumeration is a **strategy** — see [`crate::strategies`] — and acquisition is an HTTP
//! GET. This file is what puts both behind the trait, which is roughly the amount of code
//! `collect` and `discover` used to spend knowing they were talking to a website.
//!
//! ## What this Source owns, and what the strategy owns
//!
//! Everything a strategy must not be trusted with lives here: the [`Pacer`], the
//! [`HostPolicy`], the request budget, `robots.txt`, and the decision about what counts as
//! a [`Resource`]. A strategy is handed a [`Seed`] and a [`Crawl`], and it hands back
//! addresses.
//!
//! That split is why a strategy cannot hammer a host and cannot write a false record. It
//! is also why the strategy that ran is recorded rather than assumed: [`Source::method`]
//! reports what actually spoke, so the store alone recovers it later.
//!
//! ## Choosing one
//!
//! A `[[source]]` block may pin a strategy, which means an operator saw the evidence and
//! accepted it. A pinned strategy is still asked to recognise the site on every run, and
//! one that stops recognising the address it was accepted for produces a **warning**. It
//! does not produce a silent switch to a weaker strategy and an empty corpus.
//!
//! With nothing pinned the registry is asked. When nothing answers, the sitemap walk runs
//! anyway — it is the best available guess and it is what every source in the store was
//! collected with — and the run says so, because a fallback and a recognition currently
//! produce identical records and only one of them is worth investigating.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures::future::BoxFuture;

use crate::discovery::{DiscoveryLimits, Robots};
use crate::domain::{
    Acquired, Enumeration, Fetched, Liveness, Note, NoteMark, Refusal, Resource, Source, SourceId,
    SourceKind,
};
use crate::enclosure;
use crate::fetch::Fetcher;
use crate::op::{ItemOutcome, Progress, Verdict};
use crate::policy::{HostPolicy, Pacer};
use crate::strategies::{self, Addresses, Crawl, Recognition, Seed, StrategyDef};

pub struct SiteSource {
    id: SourceId,
    /// Any URL on the site. Only the origin is used.
    site: String,
    policy: HostPolicy,
    limits: DiscoveryLimits,
    fetcher: Fetcher,
    /// The strategy the `[[source]]` block pinned, if it pinned one.
    named: Option<&'static StrategyDef>,
    /// The strategy that actually ran.
    ///
    /// Set during [`Source::enumerate`], which is before `discover` writes the
    /// [`crate::domain::DiscoveryRun`] — so the run records what spoke rather than what
    /// was hoped for.
    spoke: OnceLock<&'static StrategyDef>,
    /// The host's declared `Crawl-delay`, once `robots.txt` has been read.
    ///
    /// Honoured as a floor on the request interval: a host asking to be crawled slowly
    /// gets that, even where our own rate cap would allow faster.
    crawl_delay: OnceLock<Option<Duration>>,
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

/// How a refusal reads on the progress line.
///
/// From the [`Liveness`] the fetcher already decided, not from a second reading of the
/// status code. This used to re-derive its own answer out of the message text —
/// `(400..500).contains(&s) && s != 429` — and reach a different one: that calls a WAF 403
/// `Missing`, where [`crate::fetch::classify`] calls it `Blocked`. The difference between
/// those two is the entire reason `Blocked` exists, and the correctly classified state was
/// sitting on the same `Refusal` two lines above, unused.
///
/// A 403 shown as *gone* is a live page reported deleted, on the one line an operator
/// watches to decide whether a crawl is working.
fn verdict_for(state: Liveness) -> Verdict {
    match state {
        // Gone, and that is a fact about the address. Routine on a corpus full of links
        // to files migrated years ago.
        Liveness::Gone => Verdict::Missing,
        // Blocked or errored: about this run, or about this host, and in neither case
        // evidence that the document is not there.
        Liveness::Blocked | Liveness::Error => Verdict::Fail,
        // A refusal is never live, but the type permits saying so and a silent `Missing`
        // would be the same lie in a rarer form.
        Liveness::Live => Verdict::Fail,
    }
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
            policy,
            limits,
            named: None,
            spoke: OnceLock::new(),
            crawl_delay: OnceLock::new(),
            pacers: Mutex::new(HashMap::new()),
            partial: Mutex::new(Vec::new()),
            enclosed: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
        })
    }

    /// Pins the strategy an operator accepted, or the one the store remembers.
    pub fn with_strategy(mut self, named: Option<&'static StrategyDef>) -> Self {
        self.named = named;
        self
    }

    /// The strategy in force: what ran, else what was pinned, else nothing yet.
    fn strategy(&self) -> Option<&'static StrategyDef> {
        self.spoke.get().or(self.named.as_ref()).copied()
    }

    /// Whether a fetched page is worth looking inside.
    ///
    /// A `sitemap` names pages, and a page is often a wrapper around a PDF its CMS renders
    /// in a viewer — so the scan pays for itself. A `listing` names the documents
    /// themselves: scanning those finds nothing, and it is how
    /// `/251agendaonline/.pdf?documentType=` was invented, an address naming no document
    /// on a host where every dead address answers 200.
    ///
    /// Defaults to scanning, which is what every source in the store was collected with.
    fn scans_for_enclosures(&self) -> bool {
        !matches!(
            self.strategy().map(|s| s.it.addresses_are()),
            Some(Addresses::Documents)
        )
    }

    /// Fetches the landing page and `robots.txt` — everything a recogniser is allowed to
    /// see, and one request more than discovery used to cost.
    ///
    /// A landing page that refuses is **not** fatal. `Discoverer` never fetched one at
    /// all, so failing here would break every site that 403s its front door and serves a
    /// perfectly good sitemap. The seed is built with empty bytes and a warning instead:
    /// recognisers that need markup answer `None`, and the ones that only need
    /// `robots.txt` are unaffected.
    ///
    /// Public so `investigate` can ask the registry itself rather than only being told
    /// which strategy won. Two requests are enough to run every recogniser in the build,
    /// and paying for them twice to see the runners-up would be the wrong trade.
    pub async fn seed(&self, progress: &Progress) -> anyhow::Result<(Seed, Vec<String>)> {
        let base = url::Url::parse(&self.site)?;
        let mut warnings = Vec::new();

        progress.say(format!(
            "reading robots.txt for {}",
            base.host_str().unwrap_or("?")
        ));
        let robots_url = base.join("/robots.txt")?;
        self.pacer_for(robots_url.as_str()).wait().await;
        let robots = match self.fetcher.get(robots_url.as_str()).await {
            Ok(f) => Robots::parse(&self.policy.user_agent, &f.bytes),
            Err(e) => {
                // Measured on phila.gov: CloudFront 403 on robots.txt, 200 on the site.
                warnings.push(format!("robots.txt unreachable ({e}); assuming no rules"));
                Robots::unreachable(self.policy.unreachable_robots)
            }
        };

        // From here on, honour what the host asked for. The pacers built for the two
        // requests above are dropped rather than reused, because a cached limiter with no
        // delay in it would silently outrank the `Crawl-delay` we just read.
        let _ = self.crawl_delay.set(robots.crawl_delay());
        if robots.crawl_delay().is_some() {
            self.pacers
                .lock()
                .expect("pacer map is never poisoned")
                .clear();
        }

        self.pacer_for(&self.site).wait().await;
        let page = match self.fetcher.get(&self.site).await {
            Ok(f) => f,
            Err(refusal) => {
                warnings.push(format!(
                    "{} — {refusal}; recognition ran on robots.txt alone",
                    self.site
                ));
                Fetched {
                    bytes: Vec::new(),
                    meta: BTreeMap::from([("final_url".to_string(), self.site.clone())]),
                }
            }
        };

        Ok((Seed { page, robots }, warnings))
    }

    /// Runs one strategy against a seed already in hand, and records that it spoke.
    ///
    /// Split out of [`Source::enumerate`] so `investigate` can recognise, choose from the
    /// full list, and then enumerate — without fetching the seed a second time.
    pub async fn run(
        &self,
        chosen: &'static StrategyDef,
        seed: &Seed,
        progress: &Progress,
    ) -> anyhow::Result<crate::strategies::Enumerated> {
        let _ = self.spoke.set(chosen);
        progress.say(format!("enumerating with `{}`", chosen.name));

        let crawl = SiteCrawl {
            site: self,
            progress,
            spent: AtomicUsize::new(0),
            // `--max-sitemaps` names the only strategy that existed when the flag was
            // added. It has always meant "requests this enumeration may spend", which is
            // what every strategy needs, so it is reused rather than renamed — a flag
            // sitting in somebody's cron entry is not worth the tidier name.
            budget: self.limits.max_sitemaps,
            max_addresses: self.limits.max_urls,
        };
        chosen.it.enumerate(seed, &crawl).await
    }

    /// Which strategy runs, and on what evidence.
    ///
    /// Three outcomes, and the third is the one worth naming. A pinned strategy that no
    /// longer recognises its own site still runs — the operator asked for it, and refusing
    /// would collect nothing — but the disagreement is a warning, because a vendor
    /// shipping a new version is exactly how a healthy-looking run starts returning an
    /// empty corpus.
    fn choose(
        &self,
        seed: &Seed,
        warnings: &mut Vec<String>,
    ) -> (&'static StrategyDef, Option<Recognition>) {
        if let Some(named) = self.named {
            let recognition = named.it.recognise(seed);
            if recognition.is_none() {
                // Not "no longer": this same path serves `check --strategy=<name>`, where
                // the strategy was forced by hand and never recognised the address at all.
                // The sentence has to be true for both, so it states what is observed and
                // leaves the operator to know which of the two they are looking at.
                warnings.push(format!(
                    "`{}` does not recognise {} — it was run anyway, and the count below \
                     may be wrong. If it used to, the site changed.",
                    named.name, self.site
                ));
            }
            return (named, recognition);
        }

        match strategies::best(seed) {
            Some(r) => (
                strategies::by_name(r.strategy).unwrap_or_else(|_| strategies::fallback()),
                Some(r),
            ),
            // Nothing spoke. The sitemap walk is the best available guess, and saying so
            // is what separates a fallback from a recognition — today they write identical
            // records, and only one of them is worth a person's attention.
            None => {
                warnings.push(format!(
                    "no strategy recognised {}; walking it as a sitemap",
                    self.site
                ));
                (strategies::fallback(), None)
            }
        }
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
                // must not be shown as though they were the same kind of answer. The code
                // is a *label* here and nothing turns on it — which is the whole reason
                // reading it back out of prose is tolerable.
                let status = status_in(&refusal.detail);
                ItemOutcome {
                    address: url.to_string(),
                    tag: status.map_or_else(|| "—".into(), |s| s.to_string()),
                    verdict: verdict_for(refusal.state),
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
        if crate::content::ContentKind::classify(&fetched.meta, &fetched.bytes)
            != crate::content::ContentKind::Html
        {
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
        let declared = self.crawl_delay.get().copied().flatten();
        let mut pacers = self.pacers.lock().expect("pacer map is never poisoned");
        Arc::clone(
            pacers
                .entry(host)
                .or_insert_with(|| Arc::new(Pacer::new(self.policy.min_interval(declared)))),
        )
    }
}

/// The host, as a strategy is allowed to see it.
///
/// Holds the budget rather than delegating it, so "how many requests has this enumeration
/// spent" is one counter in one place instead of a number each strategy tracks its own way
/// and reports differently.
struct SiteCrawl<'a> {
    site: &'a SiteSource,
    progress: &'a Progress,
    spent: AtomicUsize,
    budget: usize,
    max_addresses: usize,
}

impl Crawl for SiteCrawl<'_> {
    /// Paced, counted, and reported by the strategy rather than here.
    ///
    /// Deliberately not [`SiteSource::fetch_reporting`]: that emits one item line per
    /// request, which is right for acquisition — where each line is a document somebody
    /// wanted — and wrong for enumeration, where two hundred sitemap fetches would bury
    /// the run. A strategy narrates its own walk through [`Crawl::progress`].
    fn get<'b>(&'b self, url: &'b str) -> BoxFuture<'b, Result<Fetched, Refusal>> {
        Box::pin(async move {
            self.spent.fetch_add(1, Ordering::Relaxed);
            self.site.pacer_for(url).wait().await;
            self.site.fetcher.get(url).await
        })
    }

    fn may_fetch(&self) -> bool {
        self.spent.load(Ordering::Relaxed) < self.budget
    }

    fn budget(&self) -> usize {
        self.budget
    }

    fn max_addresses(&self) -> usize {
        self.max_addresses
    }

    fn progress(&self) -> &Progress {
        self.progress
    }
}

impl Source for SiteSource {
    fn id(&self) -> &SourceId {
        &self.id
    }

    fn kind(&self) -> SourceKind {
        SourceKind::Site
    }

    /// What actually spoke, falling back to what was pinned and then to the default.
    ///
    /// Read after [`Source::enumerate`], which is when `discover` writes the run — so a
    /// `DiscoveryRun` records the strategy that produced it. `sources::infer` reads it
    /// back, which is how a source collected by hand recovers its own strategy.
    fn method(&self) -> &'static str {
        self.strategy().map_or("sitemap", |s| s.name)
    }

    fn target(&self) -> &str {
        &self.site
    }

    fn enumerate<'a>(
        &'a self,
        progress: &'a Progress,
    ) -> BoxFuture<'a, anyhow::Result<Enumeration>> {
        Box::pin(async move {
            let (seed, mut warnings) = self.seed(progress).await?;

            let (chosen, recognition) = self.choose(&seed, &mut warnings);
            let found = self.run(chosen, &seed, progress).await?;

            // Recognition first, because it is what the operator checks before trusting
            // the count under it.
            let mut notes = Vec::new();
            match &recognition {
                Some(r) => {
                    notes.push(Note::marked(
                        "strategy",
                        format!("{} — {}", r.strategy, r.keyed_on),
                        NoteMark::Ok,
                    ));
                    notes.extend(r.evidence.iter().cloned());
                    notes.extend(r.warnings.iter().cloned());
                }
                None => notes.push(Note::marked(
                    "strategy",
                    format!("{} — nothing recognised this address", chosen.name),
                    NoteMark::Warn,
                )),
            }
            notes.extend(found.notes);
            warnings.extend(found.warnings);

            let mut figures = found.figures;
            if let Some(total) = found.declared_total {
                // The number that tells "collected the site" from "collected 4% of it".
                notes.push(Note::ok_or_warn(
                    "declared",
                    format!(
                        "the source names {} item(s); this pass found {}",
                        crate::render::count(total),
                        crate::render::count(found.addresses.len() as u64)
                    ),
                    found.addresses.len() as u64 >= total,
                ));
                figures.insert("declared_total".to_string(), total);
            }

            // Addresses become Resources **here**, and nowhere else. A strategy that could
            // mint them could mint two identities for one document.
            let base = seed.final_url();
            let resources = found
                .addresses
                .iter()
                .filter_map(|a| {
                    let absolute = match &base {
                        Some(b) => b.join(a).ok()?.to_string(),
                        None => a.clone(),
                    };
                    Some(Resource::new(self.id.clone(), absolute))
                })
                .collect();

            Ok(Enumeration {
                resources,
                warnings,
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
            // The branch is on what the strategy *named*, never on which strategy it was.
            let enclosed = match self.scans_for_enclosures() {
                true => self.enclosures(&fetched, &base),
                false => Vec::new(),
            };

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

    /// Nothing has spoken yet, so the run reads as what it would fall back to. This is
    /// also every source added before any strategy existed.
    #[test]
    fn an_unpinned_source_reads_as_a_sitemap_until_something_recognises_it() {
        let s = source();
        assert_eq!(s.method(), "sitemap");
        assert!(
            s.scans_for_enclosures(),
            "the default must stay what the store was collected with"
        );
    }

    /// A pinned strategy is recorded before it runs, so `discover` writes what the
    /// operator accepted rather than a name the log has to guess at afterwards.
    #[test]
    fn a_pinned_strategy_is_what_the_run_records_and_how_it_acquires() {
        let listing = crate::strategies::by_name("listing").unwrap();
        let s = source().with_strategy(Some(listing));

        assert_eq!(s.method(), "listing");
        assert!(
            !s.scans_for_enclosures(),
            "a file in a directory index IS the document; scanning 6 GB of CSV for \
             enclosed documents finds nothing"
        );
    }

    /// The strategy that actually ran outranks the one that was pinned, because a run
    /// must record what happened rather than what was asked for.
    #[test]
    fn what_spoke_outranks_what_was_pinned() {
        let s = source().with_strategy(Some(crate::strategies::by_name("listing").unwrap()));
        s.spoke
            .set(crate::strategies::by_name("sitemap").unwrap())
            .unwrap();
        assert_eq!(s.method(), "sitemap");
        assert!(s.scans_for_enclosures());
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

    /// The mistake `Blocked` exists to prevent, one level up from the record.
    ///
    /// A WAF 403 used to read as `Missing` on the progress line, because the verdict was
    /// re-derived from the status code rather than taken from the state `fetch::classify`
    /// had already decided — and `400..500` catches 403 alongside 404.
    #[test]
    fn a_blocked_page_is_never_shown_as_gone() {
        assert_eq!(verdict_for(Liveness::Blocked), Verdict::Fail);
        assert_eq!(verdict_for(Liveness::Error), Verdict::Fail);
        assert_eq!(verdict_for(Liveness::Gone), Verdict::Missing);
    }

    /// The two classifications now agree by construction, so this walks the codes that
    /// used to divide them.
    #[test]
    fn every_refused_status_reads_the_same_way_it_is_recorded() {
        for (status, expected) in [
            (404, Verdict::Missing),
            (410, Verdict::Missing),
            // The three that the old `400..500 && != 429` rule got wrong.
            (401, Verdict::Fail),
            (403, Verdict::Fail),
            (429, Verdict::Fail),
            (500, Verdict::Fail),
            (503, Verdict::Fail),
        ] {
            assert_eq!(
                verdict_for(crate::fetch::classify(status)),
                expected,
                "HTTP {status}"
            );
        }
    }
}
