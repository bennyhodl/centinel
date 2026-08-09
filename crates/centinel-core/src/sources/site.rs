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
//! a [`Resource`]. A strategy is handed a [`Seed`] and a [`Walk`], and it hands back
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
use crate::fetch::{Fetch, Fetcher};
use crate::op::{Cancel, ItemOutcome, Progress, Verdict};
use crate::policy::{HostPolicy, Pacer};
use crate::strategies::Recognition;
use crate::strategies::crawl::{self, Addresses, Seed, StrategyDef, Walk};

pub struct SiteSource {
    id: SourceId,
    /// Any URL on the site. Only the origin is used.
    site: String,
    policy: HostPolicy,
    limits: DiscoveryLimits,
    /// Where bytes come from. A trait rather than the client, so a test can answer from a
    /// map and reach everything above the fetch — which is where this branch's defects
    /// were, and why they survived.
    fetcher: Arc<dyn Fetch>,
    /// The strategy the `[[source]]` block pinned, if it pinned one.
    named: Option<&'static StrategyDef>,
    /// The strategy that actually ran.
    ///
    /// Set during [`Source::enumerate`], which is before `discover` writes the
    /// [`crate::domain::DiscoveryRun`] — so the run records what spoke rather than what
    /// was hoped for.
    spoke: OnceLock<&'static StrategyDef>,
    /// Whether anything recognised the address, as against merely running on it.
    ///
    /// Set by [`Self::choose`], which is the one place that decides it. It used to be
    /// recovered by reading the mark back off the `strategy` note, which made a rendering
    /// decision load-bearing for a machine answer: renaming the label, or adding a second
    /// note under it, silently reported every address as recognised.
    recognised: OnceLock<bool>,
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
    /// Whether the op that owns this Source has been asked to stop.
    ///
    /// Read by [`SiteCrawl::may_fetch`], so a walk stops between requests rather than
    /// running its whole budget out. A paced 25-request probe is minutes of `^C` doing
    /// nothing otherwise, and the walk is the only long-running part `investigate` has.
    /// A cancelled walk reports `truncated`, which it is.
    cancel: Cancel,
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
            fetcher: Arc::new(Fetcher::new(&policy)?),
            policy,
            limits,
            named: None,
            spoke: OnceLock::new(),
            recognised: OnceLock::new(),
            crawl_delay: OnceLock::new(),
            cancel: Cancel::none(),
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

    /// Lets an op stop this Source's walk between requests.
    pub fn with_cancel(mut self, cancel: Cancel) -> Self {
        self.cancel = cancel;
        self
    }

    /// Answers requests from something other than the network.
    ///
    /// The seam the tests below use. Pacing, `robots.txt`, the budget and the ceiling all
    /// stay exactly where they are — only the bytes are substituted — so a test exercises
    /// the real `seed`, the real `choose`, and the real conversion to an `Enumeration`.
    pub fn with_fetcher(mut self, fetcher: Arc<dyn Fetch>) -> Self {
        self.fetcher = fetcher;
        self
    }

    /// The strategy in force: what ran, else what was pinned, else nothing yet.
    fn strategy(&self) -> Option<&'static StrategyDef> {
        self.spoke.get().or(self.named.as_ref()).copied()
    }

    /// Whether anything recognised the address, once [`Self::choose`] has run.
    ///
    /// `None` before it has. A forced strategy that does not recognise its own site is
    /// `Some(false)`, which is the answer `check --strategy=<name>` needs and the one a
    /// note's mark could not tell apart from "the label moved".
    pub(crate) fn recognised(&self) -> Option<bool> {
        self.recognised.get().copied()
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
    ) -> anyhow::Result<crate::strategies::crawl::Enumerated> {
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
    ///
    /// Public so `investigate` asks the same question `run` asks. It used to answer it
    /// itself, from `crawl::best` and a local `by_fallback` flag — which meant a source
    /// with a pinned strategy was told one thing by `investigate` and collected another.
    pub(crate) fn choose(
        &self,
        seed: &Seed,
        warnings: &mut Vec<String>,
    ) -> (&'static StrategyDef, Option<Recognition>) {
        let (chosen, recognition) = self.decide(seed, warnings);
        let _ = self.recognised.set(recognition.is_some());
        (chosen, recognition)
    }

    fn decide(
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

        match crawl::best(seed) {
            Some((def, r)) => (def, Some(r)),
            // Nothing spoke. The sitemap walk is the best available guess, and the
            // difference between a guess and a recognition is worth a person's attention,
            // because the two write identical records.
            //
            // Not a warning from here, though: the returned `None` *is* the fact, and each
            // caller says it in its own terms. `enumerate` writes the prose below;
            // `investigate` renders it structurally, with `run` would do the same beside
            // it, which is the sentence that actually helps. A warning pushed here would
            // have been a second copy in whichever of the two also rendered it.
            None => (crawl::fallback(), None),
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

impl Walk for SiteCrawl<'_> {
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

    /// Cancellation reads as a ceiling, and that is the honest shape.
    ///
    /// A walk stopped part way has produced a partial snapshot either way, and every
    /// strategy already handles this answer by setting `truncated` and saying so. The op
    /// then turns the stop into a `Cancelled` at its own boundary.
    fn may_fetch(&self) -> bool {
        !self.site.cancel.is_cancelled() && self.spent.load(Ordering::Relaxed) < self.budget
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
            if recognition.is_none() && self.named.is_none() {
                warnings.push(format!(
                    "no strategy recognised {}; walking it as a {}",
                    self.site, chosen.name
                ));
            }
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
                truncated: found.truncated,
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

    /// A host that answers from a map and never touches a network.
    ///
    /// The peer of `strategies::crawl::tests::Fake` one layer down: that one substitutes
    /// what a *strategy* is allowed to do, and this substitutes where the Source's own two
    /// requests — the front door and `robots.txt` — come from. Together they make
    /// `enumerate` testable end to end without a socket.
    #[derive(Debug, Default)]
    struct Scripted {
        pages: std::collections::BTreeMap<String, Result<Vec<u8>, Liveness>>,
    }

    impl Scripted {
        fn serving(mut self, url: &str, body: &str) -> Self {
            self.pages
                .insert(url.to_string(), Ok(body.as_bytes().to_vec()));
            self
        }

        fn refusing(mut self, url: &str, state: Liveness) -> Self {
            self.pages.insert(url.to_string(), Err(state));
            self
        }

        fn arc(self) -> Arc<dyn Fetch> {
            Arc::new(self)
        }
    }

    impl Fetch for Scripted {
        fn get<'a>(
            &'a self,
            url: &'a str,
        ) -> BoxFuture<'a, Result<crate::domain::Fetched, crate::fetch::FetchFailure>> {
            Box::pin(async move {
                match self.pages.get(url) {
                    Some(Ok(bytes)) => Ok(Fetched {
                        bytes: bytes.clone(),
                        meta: BTreeMap::from([("final_url".to_string(), url.to_string())]),
                    }),
                    Some(Err(state)) => Err(Refusal {
                        state: *state,
                        detail: format!("scripted {state}"),
                    }),
                    None => Err(Refusal {
                        state: Liveness::Gone,
                        detail: "HTTP 404 Not Found".into(),
                    }),
                }
            })
        }
    }

    /// One sitemap holding more addresses than the ceiling allows.
    fn crowded_sitemap(n: usize) -> String {
        let urls: String = (0..n)
            .map(|i| format!("<url><loc>https://www.tampa.gov/p/{i}</loc></url>"))
            .collect();
        format!("<?xml version=\"1.0\"?><urlset>{urls}</urlset>")
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
        let listing = crate::strategies::crawl::by_name("listing").unwrap();
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
        let s = source().with_strategy(Some(crate::strategies::crawl::by_name("listing").unwrap()));
        s.spoke
            .set(crate::strategies::crawl::by_name("sitemap").unwrap())
            .unwrap();
        assert_eq!(s.method(), "sitemap");
        assert!(s.scans_for_enclosures());
    }

    // ── choose ────────────────────────────────────────────────────────────────────
    //
    // `choose` takes a Seed, so none of this needs a network. It went untested while it
    // was private, and three callers grew their own answer to the question it settles.

    /// Nothing spoke, so the sitemap walk runs as the best available guess — and the
    /// difference between a guess and a recognition is *returned*, not narrated.
    ///
    /// `choose` used to push the prose itself, which meant `investigate` — which renders
    /// the same fact structurally, with `run` would do the same beside it — had to either
    /// print it twice or throw the warning list away. The `None` is the fact; each caller
    /// says it in its own terms.
    #[test]
    fn an_unrecognised_address_falls_back_and_says_so_by_returning_nothing() {
        let s = source();
        let seed = crate::strategies::crawl::tests::seed("<html/>", "https://www.tampa.gov/");
        let mut warnings = Vec::new();

        let (chosen, recognition) = s.choose(&seed, &mut warnings);

        assert_eq!(chosen.name, "sitemap", "the fallback is the sitemap walk");
        assert!(recognition.is_none());
        assert_eq!(s.recognised(), Some(false));
        assert!(
            warnings.is_empty(),
            "the fallback is not a warning from here: {warnings:?}"
        );
    }

    /// The headline safety feature: a pinned strategy that no longer recognises its own
    /// site still runs — refusing would collect nothing — and the disagreement is a
    /// warning, because a vendor shipping a new version is how a healthy-looking run
    /// starts returning an empty corpus.
    #[test]
    fn a_pin_that_no_longer_recognises_runs_anyway_and_warns() {
        let listing = crate::strategies::crawl::by_name("listing").unwrap();
        let s = source().with_strategy(Some(listing));
        // An ordinary CMS page: nothing a directory index would ever key on.
        let seed = crate::strategies::crawl::tests::seed(
            "<html><body><main>a proclamation</main></body></html>",
            "https://www.tampa.gov/news",
        );
        let mut warnings = Vec::new();

        let (chosen, recognition) = s.choose(&seed, &mut warnings);

        assert_eq!(chosen.name, "listing", "the operator asked for it");
        assert!(recognition.is_none());
        assert_eq!(
            s.recognised(),
            Some(false),
            "the count that follows may be wrong, and a machine has to be able to tell"
        );
        assert!(
            warnings.iter().any(|w| w.contains("does not recognise")),
            "{warnings:?}"
        );
    }

    /// A recognition is the answer `check` used to recover by grepping a note's mark.
    #[test]
    fn a_recognised_address_records_that_it_was_recognised() {
        let s = source();
        let seed = crate::strategies::crawl::tests::seed_with_robots(
            "https://www.tampa.gov/",
            "Sitemap: https://www.tampa.gov/sitemap.xml\n",
        );
        let mut warnings = Vec::new();

        let (chosen, recognition) = s.choose(&seed, &mut warnings);

        assert_eq!(chosen.name, "sitemap");
        assert!(recognition.is_some());
        assert_eq!(s.recognised(), Some(true));
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// Before `choose` has run there is no answer, and `None` is not `false`: a caller
    /// that read a bare `bool` here would report every un-enumerated source unrecognised.
    #[test]
    fn recognition_is_unanswered_until_choose_has_run() {
        assert_eq!(source().recognised(), None);
    }

    // ── enumerate, end to end ─────────────────────────────────────────────────────
    //
    // The band between the fetch and the report. Everything here was unreachable without a
    // socket until `Fetch` became a trait, and everything here had a defect.

    fn site_with(pages: Scripted, limits: DiscoveryLimits) -> SiteSource {
        SiteSource::new(
            SourceId::new("tampa").unwrap(),
            "https://www.tampa.gov/",
            HostPolicy {
                // Fast, not zero: `min_interval` reads `0.0` as *unset* and returns one
                // second, so the obvious spelling would make every test here sleep once per
                // request after the first. The limiter is still exercised, at 1 ms.
                max_requests_per_second: 1000.0,
                ..Default::default()
            },
            limits,
        )
        .unwrap()
        .with_fetcher(pages.arc())
    }

    /// §4.3, across the seam it was being dropped at.
    ///
    /// Both strategies set `truncated` and `SiteSource::enumerate` did not copy it, so the
    /// only reader was `investigate` — and `discover` and `run`, the two commands that
    /// actually write a `DiscoveryRun`, still inferred it from a shrinking delta.
    #[tokio::test]
    async fn a_walk_that_stopped_on_its_ceiling_says_so_in_the_enumeration() {
        let s = site_with(
            Scripted::default()
                .serving(
                    "https://www.tampa.gov/robots.txt",
                    "Sitemap: https://www.tampa.gov/sitemap.xml\n",
                )
                .serving("https://www.tampa.gov/", "<html/>")
                .serving("https://www.tampa.gov/sitemap.xml", &crowded_sitemap(50)),
            DiscoveryLimits {
                max_sitemaps: 25,
                max_urls: 10,
            },
        );

        let out = s.enumerate(&Progress::none()).await.unwrap();

        assert_eq!(out.resources.len(), 10, "the ceiling held");
        assert!(
            out.truncated,
            "the count is a floor and the enumeration does not say so"
        );
    }

    /// And the converse, so the flag is not simply always true.
    #[tokio::test]
    async fn a_walk_that_reached_the_end_is_not_marked_truncated() {
        let s = site_with(
            Scripted::default()
                .serving(
                    "https://www.tampa.gov/robots.txt",
                    "Sitemap: https://www.tampa.gov/sitemap.xml\n",
                )
                .serving("https://www.tampa.gov/", "<html/>")
                .serving("https://www.tampa.gov/sitemap.xml", &crowded_sitemap(3)),
            DiscoveryLimits {
                max_sitemaps: 25,
                max_urls: 500,
            },
        );

        let out = s.enumerate(&Progress::none()).await.unwrap();

        assert_eq!(out.resources.len(), 3);
        assert!(
            !out.truncated,
            "a complete snapshot was reported as a floor"
        );
    }

    /// A front door that refuses is not fatal, and the run says which happened.
    ///
    /// `Discoverer` never fetched a landing page at all, so failing here would break every
    /// site that 403s its front door and serves a perfectly good sitemap. Documented at
    /// length on `seed` and untested until the fetch had a seam.
    #[tokio::test]
    async fn a_front_door_that_refuses_still_enumerates_from_robots() {
        let s = site_with(
            Scripted::default()
                .serving(
                    "https://www.tampa.gov/robots.txt",
                    "Sitemap: https://www.tampa.gov/sitemap.xml\n",
                )
                .refusing("https://www.tampa.gov/", Liveness::Blocked)
                .serving("https://www.tampa.gov/sitemap.xml", &crowded_sitemap(4)),
            DiscoveryLimits::default(),
        );

        let out = s.enumerate(&Progress::none()).await.unwrap();

        assert_eq!(out.resources.len(), 4, "the sitemap was still walked");
        assert!(
            out.warnings.iter().any(|w| w.contains("robots.txt alone")),
            "the degraded seed was not reported: {:?}",
            out.warnings
        );
        assert_eq!(s.recognised(), Some(true), "robots.txt declared a sitemap");
    }

    /// Addresses become Resources here and nowhere else, resolved against where the bytes
    /// were actually served rather than against the address that was typed.
    #[tokio::test]
    async fn relative_addresses_become_resources_against_the_served_url() {
        let s = site_with(
            Scripted::default()
                .serving(
                    "https://www.tampa.gov/robots.txt",
                    "Sitemap: https://www.tampa.gov/sitemap.xml\n",
                )
                .serving("https://www.tampa.gov/", "<html/>")
                .serving(
                    "https://www.tampa.gov/sitemap.xml",
                    "<?xml version=\"1.0\"?><urlset><url><loc>/agenda/1</loc></url></urlset>",
                ),
            DiscoveryLimits::default(),
        );

        let out = s.enumerate(&Progress::none()).await.unwrap();

        assert_eq!(
            out.resources
                .iter()
                .map(|r| r.natural_key.as_str())
                .collect::<Vec<_>>(),
            ["https://www.tampa.gov/agenda/1"],
        );
        assert!(out.resources.iter().all(|r| r.source == *s.id()));
    }

    /// Nothing recognised it, the fallback walked it anyway, and the report says which —
    /// because a fallback and a recognition otherwise write identical records.
    #[tokio::test]
    async fn a_fallback_walk_is_recorded_as_a_fallback() {
        let s = site_with(
            Scripted::default()
                .refusing("https://www.tampa.gov/robots.txt", Liveness::Gone)
                .serving("https://www.tampa.gov/", "<html/>")
                .serving("https://www.tampa.gov/sitemap.xml", &crowded_sitemap(2)),
            DiscoveryLimits::default(),
        );

        let out = s.enumerate(&Progress::none()).await.unwrap();

        assert_eq!(out.resources.len(), 2);
        assert_eq!(s.recognised(), Some(false));
        assert!(
            out.warnings
                .iter()
                .any(|w| w.contains("no strategy recognised")),
            "{:?}",
            out.warnings
        );
        assert!(
            out.notes
                .iter()
                .any(|n| n.label == "strategy" && n.mark == Some(NoteMark::Warn)),
            "{:?}",
            out.notes
        );
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
