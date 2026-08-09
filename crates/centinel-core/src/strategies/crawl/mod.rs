//! Recognising a site, and enumerating what it holds.
//!
//! The **crawl** side of [`super`]: where are the addresses. What the text at those
//! addresses turns out to say is [`crate::extract`]'s question, answered by a list of
//! readers keyed on the content kind, and a strategy here has no opinion on it.
//!
//! A crawl strategy is a pair: it recognises a shape, and it enumerates the shape it
//! recognised. The pairing is the design. You cannot add a strategy without teaching it to
//! recognise itself, and you cannot recognise a shape you cannot then handle — without
//! that invariant the registry rots into a set of confident half-answers, and a confident
//! half-answer is the most expensive failure this pipeline has.
//!
//! `docs/FIELD-NOTES.md` entry 1 is what one costs: 75 Resources, 75 successful
//! acquisitions, liveness `live` on every one, and the corpus gained 75 copies of a
//! navigation menu reading "Preview link expired". A wrong recognition is silent and it
//! produces a run that looks perfect.
//!
//! ## The unit of contribution is a strategy, never a site
//!
//! A strategy keys on a **product**, a **framework**, a **server default**, or a
//! **standard**. Never on a jurisdiction. Every one of those ships to many cities, which
//! is what makes the work amortise: teaching Centinel to recognise Hyland OnBase collects
//! every city running OnBase, where teaching it Tampa collects Tampa.
//!
//! [`super::Keyed`] has no `Jurisdiction` variant, so that rule is enforced by the type rather
//! than by review.
//!
//! ## What a strategy does not own
//!
//! It owns [`Strategy::enumerate`], and the part of acquisition that decides *which
//! address form holds the document* ([`Strategy::addresses_are`]). It owns nothing after
//! that. There is deliberately **no per-site extraction hook**: ship one and the
//! table-fusing defect gets worked around in forty site plugins instead of fixed once, and
//! then the framework fix cannot land because forty plugins depend on the broken
//! behaviour. That is the fork cost this module exists to avoid, moved one layer down and
//! made permanent.
//!
//! The evidence says the seam is in the right place. Of the four sites walked by hand in
//! `docs/FIELD-NOTES.md`, **zero** needed a site-specific extractor; every extraction
//! fault found was a framework defect that any site triggers.
//!
//! ## A strategy fetches, and that is a correction
//!
//! `docs/STRATEGIES.md` §9 specified a strategy as a pure function over the seed bytes,
//! on the reasoning that a strategy which never fetches cannot hammer a host. The
//! *conclusion* was right and the *mechanism* was wrong: it was written before either of
//! the first two strategies existed, and neither can hold to it. A sitemap walk fetches
//! `robots.txt` and up to two hundred sitemap documents; a directory-index walk recurses
//! into subdirectories. Both are enumeration, and enumeration is the stage the field notes
//! say all the variance lives in.
//!
//! So the goal is kept and the mechanism is replaced. A strategy fetches only through
//! [`Walk`] — one paced GET and a budget — and everything a strategy must not own stays
//! behind it:
//!
//! 1. The [`Pacer`], `robots.txt` rules and [`HostPolicy`] stay with the host. A strategy
//!    cannot hammer a site because it does not hold the throttle.
//! 2. A strategy returns **addresses**, not [`crate::domain::Resource`]s. The host decides what a
//!    Resource is, so canonicalisation stays in one place and no strategy can write a
//!    false record.
//! 3. It is testable without a network. A [`Walk`] backed by a map of URL → bytes is the
//!    whole harness, and the bytes are blobs the store already holds.
//! 4. The budget is the host's, so a truncated enumeration is a warning the strategy
//!    *wrote* rather than a short list nobody can tell from a complete one.
//!
//! What this costs is the one-shot subprocess boundary §9 ranked first: a fetching
//! strategy needs request/response over stdio rather than bytes-in-JSON-out. A strategy
//! that never calls [`Walk`] keeps the cheap boundary. That is a real trade and it is
//! recorded rather than hidden.
//!
//! [`Pacer`]: crate::policy::Pacer
//! [`HostPolicy`]: crate::policy::HostPolicy

pub mod listing;
pub mod sitemap;

use super::Recognition;

use std::collections::{BTreeMap, HashSet};

use futures::future::BoxFuture;

use crate::discovery::Robots;
use crate::domain::{Fetched, Note, Refusal};
use crate::op::Progress;

/// What every recogniser is given, fetched once by the host.
///
/// A bundle rather than a single page because `robots.txt` is wanted by more than one
/// recogniser and costs one request. Fetching it once here is what keeps
/// [`Strategy::recognise`] a pure, cheap function that the whole registry can run —
/// recognition that could fetch would cost one request per registered strategy, and would
/// grow more expensive with every strategy merged.
pub struct Seed {
    /// The landing page, as served. `meta["final_url"]` is where the bytes came from,
    /// which is not always where they were asked for.
    pub page: Fetched,
    /// `/robots.txt` for the page's origin.
    ///
    /// Never an `Option`: an unreachable `robots.txt` still produces rules, because
    /// [`Robots::unreachable`] applies the host policy's fallback. `robots.declared` is
    /// how a strategy tells rules that were **read** from rules that were **assumed**,
    /// and the difference belongs in a Note rather than in a missing value.
    pub robots: Robots,
}

impl Seed {
    /// Where the bytes actually came from, which is what a recogniser must key on.
    ///
    /// The visible URL is not the fetchable one — two sightings in the field notes, and
    /// both seeds handed over were wrappers that redirected elsewhere.
    pub fn final_url(&self) -> Option<url::Url> {
        let served = self.page.meta.get("final_url")?;
        url::Url::parse(served).ok()
    }

    /// The landing page as text, for a recogniser that tests markup.
    pub fn text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.page.bytes)
    }
}

/// What a strategy is permitted to do to a host.
///
/// The narrowest interface that admits the strategies that exist: one paced GET, and an
/// honest answer about whether there is budget left for another. See the module docs for
/// why this exists at all.
pub trait Walk: Send + Sync {
    /// One GET, already paced, already counted against the budget.
    fn get<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<Fetched, Refusal>>;

    /// Whether another [`Self::get`] will do work.
    ///
    /// A strategy checks this and stops **with a warning**. A strategy that ignores it
    /// gets refusals instead of documents, which is the same truncation reported worse.
    fn may_fetch(&self) -> bool;

    /// The total requests this enumeration allows. For the warning text, so a person
    /// reads "stopped at 200" rather than "stopped".
    fn budget(&self) -> usize;

    /// How many addresses this enumeration will keep.
    ///
    /// A separate bound from [`Self::budget`] because they defend against different
    /// things: the budget bounds what we do to a *host*, and this bounds what a host can
    /// do to *us*. A sitemap is attacker-or-accident controlled — a self-referential
    /// index, a million-entry urlset, and a redirect chain between hosts are all things a
    /// real site has done by mistake.
    ///
    /// A strategy stops here **and writes a warning**, rather than returning a short list
    /// the host would have to truncate silently.
    fn max_addresses(&self) -> usize;

    /// So a paced walk is progress rather than apparent silence. At one request per
    /// second a large site is minutes of nothing, and a caller cannot otherwise tell
    /// politeness from a hang.
    fn progress(&self) -> &Progress;
}

/// One enumeration pass, as a strategy reports it.
///
/// Addresses and not [`crate::domain::Resource`]s: a Resource is an address plus an
/// identity, and identity is the host's to decide. A strategy that could mint Resources
/// could mint two identities for one document, which entry 2's `DownloadFileBytes` — whose
/// path segment the server ignores entirely — makes very easy to do by accident.
#[derive(Clone, Debug, Default)]
pub struct Enumerated {
    /// Every address this pass found, in first-seen order. Absolute or relative to the
    /// seed; the host resolves them.
    pub addresses: Vec<String>,
    /// What the source itself says it holds, when it says anything.
    ///
    /// The number that makes "collected the site" and "collected 4% of it" tell apart.
    /// Entry 2 caps at 100 in silence; entry 3 prints *"2606 items in 53 pages"* in its
    /// footer. Same defect from both ends, and from a search box they look identical.
    ///
    /// **No strategy writes this yet**, so it is structurally `None` and the reporting
    /// hanging off it is unreachable. Neither shape that declares a total has a strategy:
    /// a `<sitemapindex>` states no count, and a directory listing states no count. §10.3's
    /// `index` — where the address set is on the page rather than in a link — is the first
    /// one that will, because a paged result set is where the footer lives. Kept rather
    /// than deleted because the readers are three lines each and the writer is the next
    /// strategy; recorded here so nobody reads a live path off a field nothing sets.
    pub declared_total: Option<u64>,
    /// Provenance worth showing a person. A strategy explains itself through these and
    /// edits no renderer.
    pub notes: Vec<Note>,
    /// Non-fatal problems. A partial enumeration with recorded warnings is far more useful
    /// than a hard failure, so nothing here aborts a pass.
    pub warnings: Vec<String>,
    /// The walk stopped at one of its own ceilings, so [`Self::addresses`] is a floor and
    /// never a total.
    ///
    /// A flag rather than a phrase in [`Self::warnings`], because a reader that greps for
    /// `"stopped at"` is a reader that silently starts lying the day the wording changes —
    /// and it did. `investigate` computed completeness that way, and `dunedin.gov` printed
    /// a checkmark beside 500 addresses against a real 1,625: the walk stopped at the cap
    /// on its final iteration, so no *next* iteration ran to write the phrase.
    ///
    /// Set by the strategy, which is the only thing that knows what its ceilings were.
    pub truncated: bool,
    /// The same provenance for a machine, named as the strategy names it.
    pub figures: BTreeMap<String, u64>,
}

/// One enumeration in progress: the queue, the ceilings, and what has been kept.
///
/// **Why this is not the strategy's business.** A strategy differs from another strategy in
/// how it parses a page and what it queues next. It does not differ in what a ceiling means,
/// and it must not: a walk that stops early and a source that is small look identical, and
/// §4.3 is written against exactly that. Written per strategy, that rule is wrong per
/// strategy — and it was, twice over. Both had to learn separately that the address cap has
/// to be re-tested *after* the loop, because a walk that fills its ceiling on its last
/// iteration has no next iteration to notice; `dunedin.gov` printed a checkmark beside 500
/// addresses against a real 1,625 while only one of them knew.
///
/// So the two strategies now carry the ~80 lines that differ between them, and this carries
/// the ones that did not: budget, address cap, `truncated`, dedup, loop protection, depth,
/// the `robots.txt` exclusion count, and the progress arithmetic.
///
/// [`Walk`] stays the narrow trait it was — one paced GET and the ceilings — because it is
/// what the *host* implements and what a test substitutes. This is a helper over it, which
/// is why it is a struct and not more trait methods for every implementor to get right.
pub struct Pass<'a, T> {
    walk: &'a dyn Walk,
    /// What the warnings call the thing being walked: `surface`, `tree`. The one word that
    /// genuinely differed between the two copies of these sentences.
    noun: &'static str,
    /// How deep the queue may nest. A sitemap index nests shallowly and a directory tree
    /// does not, so this is per strategy — passed rather than shadowed by two constants
    /// under one name.
    max_depth: usize,
    queue: Vec<(T, usize)>,
    visited: HashSet<String>,
    seen: HashSet<String>,
    visits: u64,
    disallowed: u64,
    out: Enumerated,
}

impl<'a, T: std::fmt::Display> Pass<'a, T> {
    pub fn new(walk: &'a dyn Walk, noun: &'static str, max_depth: usize) -> Self {
        Self {
            walk,
            noun,
            max_depth,
            queue: Vec::new(),
            visited: HashSet::new(),
            seen: HashSet::new(),
            visits: 0,
            disallowed: 0,
            out: Enumerated::default(),
        }
    }

    /// One paced GET. The strategy parses what comes back; it never chooses when to stop.
    pub fn walk(&self) -> &dyn Walk {
        self.walk
    }

    /// Somewhere else to look, at a depth.
    pub fn push(&mut self, item: T, depth: usize) {
        self.queue.push((item, depth));
    }

    /// The next place to visit, or `None` because a ceiling was reached or nothing is left.
    ///
    /// Loop protection and the depth limit are handled here rather than reported: a
    /// self-referential index is a thing real sites serve, and skipping one is not news.
    ///
    /// Not `next`: this is not an [`Iterator`], and cannot be. The loop body needs the
    /// `Pass` for [`Self::keep`] and [`Self::push`] while it holds an item, which is
    /// exactly the borrow `for` would have taken.
    pub fn next_to_visit(&mut self) -> Option<(T, usize)> {
        loop {
            if !self.walk.may_fetch() {
                self.out.truncated = true;
                self.out.warnings.push(format!(
                    "stopped at the {}-request budget; the {} is larger than this run captured",
                    self.walk.budget(),
                    self.noun
                ));
                return None;
            }
            // Here rather than only where an address is kept, so the walk stops instead of
            // reading the rest of the source to throw it away. The warning belongs to
            // `finish`, which catches this and the walk that ends holding its ceiling.
            if self.full() {
                return None;
            }

            let (item, depth) = self.queue.pop()?;
            if !self.visited.insert(item.to_string()) {
                continue;
            }
            if depth > self.max_depth {
                self.out
                    .warnings
                    .push(format!("depth limit reached, skipping {item}"));
                continue;
            }
            return Some((item, depth));
        }
    }

    /// Narrate one visit, and count it. So a paced walk reads as progress rather than a hang.
    pub fn visiting(&mut self, label: impl Into<String>) {
        self.walk.progress().step(
            label.into(),
            self.visits,
            self.visits + self.queue.len() as u64 + 1,
        );
        self.visits += 1;
    }

    /// How many places have been visited. The strategy's own figure, under its own name.
    pub fn visits(&self) -> u64 {
        self.visits
    }

    /// Whether the address cap is reached, so an inner loop stops offering.
    pub fn full(&self) -> bool {
        self.out.addresses.len() >= self.walk.max_addresses()
    }

    /// Keep an address, unless the host's own rules exclude it or it is already held.
    ///
    /// Dedup is on the **full** address including any query string: stripping it would
    /// collapse distinct `.gov` agenda pages into one.
    pub fn keep(&mut self, addr: impl Into<String>, robots: &Robots) {
        let addr = addr.into();
        if !robots.allowed(&addr) {
            self.disallowed += 1;
            return;
        }
        if self.seen.insert(addr.clone()) {
            self.out.addresses.push(addr);
        }
    }

    /// A refusal at one address, which ends that branch and not the walk.
    pub fn refused(&mut self, what: &impl std::fmt::Display, refusal: &Refusal) {
        self.out.warnings.push(format!("{what}: {refusal}"));
    }

    pub fn note(&mut self, note: Note) {
        self.out.notes.push(note);
    }

    pub fn warn(&mut self, message: String) {
        self.out.warnings.push(message);
    }

    pub fn figure(&mut self, key: &str, value: u64) {
        self.out.figures.insert(key.to_string(), value);
    }

    /// What the source itself says it holds. See [`Enumerated::declared_total`].
    pub fn declares(&mut self, total: u64) {
        self.out.declared_total = Some(total);
    }

    /// The pass, with its ceilings accounted for.
    ///
    /// The post-loop cap test lives here because both strategies had to discover it
    /// separately: a walk that ends holding exactly its ceiling cannot know whether the
    /// source stopped there or it did, so it says the cautious thing. That direction is
    /// forced — a truncated snapshot reading as complete is the failure §4.3 exists to
    /// prevent, and a complete one reading as truncated costs a sentence.
    pub fn finish(mut self) -> Enumerated {
        if self.full() {
            self.out.truncated = true;
            self.out.warnings.push(format!(
                "stopped at {} addresses; the {} is larger than this run captured",
                self.walk.max_addresses(),
                self.noun
            ));
        }
        if self.disallowed > 0 {
            self.out.notes.push(Note::marked(
                "disallowed",
                format!(
                    "{} excluded by the site's own rules",
                    crate::render::count(self.disallowed)
                ),
                crate::domain::NoteMark::Ok,
            ));
        }
        self.out
            .figures
            .insert("disallowed".into(), self.disallowed);
        self.out
    }
}

/// Whether a strategy named pages or documents.
///
/// Read by acquisition to decide whether to scan a fetched page for enclosed documents.
/// Without it, `acquire` would have to test the strategy's *name*, which is exactly the
/// `match` this module exists to prevent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Addresses {
    /// A page, which may still hide a document. `sitemap` names these: a CMS serves a
    /// wrapper around a PDF often enough that the scan pays for itself.
    Pages,
    /// The document itself. Nothing is left to find, and scanning anyway is what invented
    /// `/251agendaonline/.pdf?documentType=` — an address naming no document, on a host
    /// where every dead address answers 200.
    Documents,
}

/// A recogniser and the enumeration it unlocks.
pub trait Strategy: Send + Sync {
    /// Recorded on the [`crate::domain::DiscoveryRun`] as `method`, and the discriminator
    /// that later recovers this strategy from the store alone.
    fn name(&self) -> &'static str;

    /// What did you see, and how sure are you?
    ///
    /// **`None` is a first-class answer and the common one.** A registry that always
    /// answers is lying, and this is the same admission the reader list already makes:
    /// *a fallback is not a second guess at the same question; it is the admission that
    /// the first tool's silence was never evidence.*
    fn recognise(&self, seed: &Seed) -> Option<Recognition>;

    /// Produce the complete address set.
    ///
    /// A snapshot, never a delta. Fetches go through `crawl` or they do not happen.
    fn enumerate<'a>(
        &'a self,
        seed: &'a Seed,
        crawl: &'a dyn Walk,
    ) -> BoxFuture<'a, anyhow::Result<Enumerated>>;

    /// Did this strategy name pages, or documents? See [`Addresses`].
    fn addresses_are(&self) -> Addresses {
        Addresses::Pages
    }
}

/// One registered strategy.
///
/// Held as a `&'static dyn Strategy` rather than a constructor because a strategy is
/// stateless by construction — everything it could hold state about (pacing, budget,
/// progress) lives behind [`Walk`].
pub struct StrategyDef {
    pub name: &'static str,
    pub it: &'static (dyn Strategy + Sync),
}

/// Its name, which is the only part of a strategy that has a printable value.
impl std::fmt::Debug for StrategyDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("StrategyDef").field(&self.name).finish()
    }
}

/// By name, which `tests::no_two_strategies_share_a_name` proves is an identity.
impl PartialEq for StrategyDef {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for StrategyDef {}

inventory::collect!(StrategyDef);

/// Every registered strategy, in a stable order.
///
/// **The registry holds no `match` and no list literal.** Each element tests itself, which
/// is [`crate::content::ContentKind::from_magic`] one level up — magic bytes are a list of
/// *(signature → kind)* where each element answers for itself, and a strategy registry is
/// magic bytes for websites.
///
/// This codebase has now made that call four times, and `CONTEXT.md` records the first
/// three: **Source** is a trait rather than a `kind` field, **ContentKind** is one table
/// rather than five, and `readers_for` is an ordered list rather than a fourth mechanism.
/// The failure mode avoided is identical each time — *"adding a kind meant ten edits and
/// the compiler asked for none."*
pub fn all() -> Vec<&'static StrategyDef> {
    let mut v: Vec<&'static StrategyDef> = inventory::iter::<StrategyDef>.into_iter().collect();
    v.sort_by_key(|s| s.name);
    v
}

/// The strategy of this name, or an error naming the ones that exist.
pub fn by_name(name: &str) -> anyhow::Result<&'static StrategyDef> {
    all().into_iter().find(|s| s.name == name).ok_or_else(|| {
        let known: Vec<&str> = all().iter().map(|s| s.name).collect();
        anyhow::anyhow!(
            "no strategy named `{name}` — this build has {}",
            known.join(", ")
        )
    })
}

/// Everything that recognised this seed, **most specific first**.
///
/// Every strategy is asked, rather than stopping at the first hit. Recognition is pure and
/// cheap over an already-fetched seed, so asking all of them costs nothing — and what the
/// runners-up saw is evidence the operator wants. A site that answers to both a product
/// and a standard is a site where the choice between them matters.
/// Each answer paired with **the strategy that gave it**.
///
/// The pairing is free here — this function is iterating the registry, so it is holding
/// the [`StrategyDef`] at the moment the [`Recognition`] comes back. It used to drop it and
/// return the name alone, and every caller that wanted to *run* the winner looked the name
/// back up: four `by_name` round-trips for a value the registry had already found, and the
/// two production ones disagreed about what an impossible lookup failure meant — one
/// returned an error, the other silently substituted the fallback.
pub fn recognise(seed: &Seed) -> Vec<(&'static StrategyDef, Recognition)> {
    let mut hits: Vec<_> = all()
        .iter()
        .filter_map(|def| def.it.recognise(seed).map(|r| (*def, r)))
        .collect();
    // Specificity first; name second, so a tie is stable across runs and machines rather
    // than dependent on link order.
    hits.sort_by_key(|(_, r)| (r.keyed_on.specificity(), r.strategy));
    hits
}

/// The strategy that should run for this seed, and why, if any recognised it.
pub fn best(seed: &Seed) -> Option<(&'static StrategyDef, Recognition)> {
    recognise(seed).into_iter().next()
}

/// What runs when nothing recognised the seed.
///
/// A guess, and it should be read as one. [`sitemap::Sitemap`] is the best available
/// default because a `.gov` site usually has a sitemap whether or not it declares one, and
/// because it is what every source already in the store was collected with.
///
/// **Reaching this is the event a Lead records.** `docs/STRATEGIES.md` §17: a sitemap walk
/// that ran because `robots.txt` declared an index is a recognition, and a sitemap walk
/// that ran because nothing else spoke is a fallback. The two produce identical
/// `DiscoveryRun`s today, and telling them apart is what makes it possible to find the
/// hosts worth writing a strategy for.
pub fn fallback() -> &'static StrategyDef {
    by_name("sitemap").expect("the sitemap strategy is compiled in")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::strategies::Keyed;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub fn seed(body: &str, url: &str) -> Seed {
        Seed {
            page: Fetched {
                bytes: body.as_bytes().to_vec(),
                meta: BTreeMap::from([("final_url".to_string(), url.to_string())]),
            },
            robots: Robots::unreachable(Default::default()),
        }
    }

    /// A seed whose `robots.txt` was read, for the recognisers that key on one.
    pub fn seed_with_robots(url: &str, robots_txt: &str) -> Seed {
        Seed {
            robots: Robots::parse("centinel", robots_txt.as_bytes()),
            ..seed("<html/>", url)
        }
    }

    /// A host that answers from a map, counts what was asked of it, and never touches a
    /// network.
    ///
    /// This is the whole test harness a strategy needs, and it is the practical payoff of
    /// putting fetching behind [`Walk`] rather than letting a strategy hold a client. The
    /// bodies a real fixture would use are blobs the store already holds.
    pub struct Fake {
        pages: BTreeMap<String, String>,
        seen: AtomicUsize,
        budget: usize,
        max_addresses: usize,
        progress: Progress,
    }

    impl Fake {
        pub fn new<const N: usize>(pages: [(&str, String); N]) -> Self {
            Self {
                pages: pages.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
                seen: AtomicUsize::new(0),
                budget: 100,
                max_addresses: 10_000,
                progress: Progress::none(),
            }
        }

        pub fn with_budget(mut self, n: usize) -> Self {
            self.budget = n;
            self
        }

        /// The address ceiling, so a test can reach it without a fixture of ten thousand
        /// URLs. The two ceilings are separate faults and want separate tests: a walk can
        /// run out of requests with room for addresses, or fill on addresses inside one
        /// cheap document — and the second is the one that used to go unreported.
        pub fn with_max_addresses(mut self, n: usize) -> Self {
            self.max_addresses = n;
            self
        }

        /// How many requests the strategy actually made.
        pub fn requests(&self) -> usize {
            self.seen.load(Ordering::Relaxed)
        }
    }

    impl Walk for Fake {
        fn get<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<Fetched, Refusal>> {
            Box::pin(async move {
                self.seen.fetch_add(1, Ordering::Relaxed);
                let body = self.pages.get(url).ok_or_else(|| Refusal {
                    state: crate::domain::Liveness::Gone,
                    detail: "HTTP 404 Not Found".into(),
                })?;
                Ok(Fetched {
                    bytes: body.as_bytes().to_vec(),
                    meta: BTreeMap::from([("final_url".to_string(), url.to_string())]),
                })
            })
        }

        fn may_fetch(&self) -> bool {
            self.requests() < self.budget
        }

        fn budget(&self) -> usize {
            self.budget
        }

        fn max_addresses(&self) -> usize {
            self.max_addresses
        }

        fn progress(&self) -> &Progress {
            &self.progress
        }
    }

    /// The invariant §4 states, checked against every strategy in the build rather than
    /// against the two that exist today. A strategy whose Recognition names a different
    /// strategy would be attributed to the wrong one in the log, and nothing else would
    /// notice.
    #[test]
    fn every_strategy_names_itself_in_its_own_recognition() {
        for def in all() {
            assert_eq!(
                def.name,
                def.it.name(),
                "the registry and the strategy disagree about its name"
            );
        }
    }

    #[test]
    fn no_two_strategies_share_a_name() {
        let mut names: Vec<&str> = all().iter().map(|s| s.name).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(before, names.len(), "a duplicate name is an ambiguous log");
    }

    #[test]
    fn the_registry_is_populated_and_says_what_it_holds() {
        assert!(!all().is_empty());
        let err = by_name("no-such-thing").unwrap_err().to_string();
        assert!(
            err.contains("sitemap"),
            "the error must list what exists: {err}"
        );
    }

    /// Entry 2's host serves both a robots.txt and a vendor application. Both answer, the
    /// sitemap answer is true and nearly worthless, and the product must win.
    #[test]
    fn a_product_outranks_a_standard_that_is_also_correct() {
        let mut order = [
            Keyed::Standard("sitemap.xml"),
            Keyed::Product("Hyland OnBase"),
            Keyed::ServerDefault("IIS directory index"),
            Keyed::Framework("ASP.NET WebForms"),
        ];
        order.sort_by_key(|k| k.specificity());
        assert_eq!(
            order.map(|k| k.kind()),
            ["product", "framework", "server default", "standard"]
        );
    }

    /// A seed reports where the bytes came from, not where they were asked for.
    #[test]
    fn the_seed_reads_the_address_it_was_served_from() {
        let s = seed("<html/>", "https://example.gov/landed/here");
        assert_eq!(s.final_url().unwrap().path(), "/landed/here");

        let blank = Seed {
            page: Fetched {
                bytes: b"<html/>".to_vec(),
                meta: BTreeMap::new(),
            },
            robots: Robots::unreachable(Default::default()),
        };
        assert!(
            blank.final_url().is_none(),
            "no address is not a bad address"
        );
    }

    /// Nothing here recognises an ordinary page, and that is the point of `Option`.
    #[test]
    fn a_page_nothing_recognises_produces_no_answer_at_all() {
        let s = seed(
            "<html><body><p>hello</p></body></html>",
            "https://example.gov/",
        );
        assert!(best(&s).is_none());
    }
}
