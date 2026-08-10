//! A **crumb**: an off-host link recorded, and **not followed**.
//!
//! One Source per exact host is the rule `docs/FIELD-NOTES.md` arrived at — a domain is not
//! a Source — and following these automatically is how the walk of one site becomes a walk
//! of the internet. So an off-host link is counted, named, and left for the operator to
//! promote. **The recursion is cut by a person, one time per host** rather than once per
//! page, and it is fractal: every Source promoted this way walks its own host and drops its
//! own crumbs.
//!
//! ## A crumb is derived; the ruling on it is truth
//!
//! A crumb is a link read out of a page, and that page is a blob. So nothing here is stored:
//! [`Trail`] rebuilds the whole set from `blobs/` whenever it is asked, which is the same
//! guarantee [`crate::enclosure`] relies on and the reason `extract` may drop an `href` from
//! the derived text without losing it.
//!
//! What cannot be rebuilt is the operator saying **no**. Nothing in a page records that a
//! person looked at `facebook.com` and refused it, so [`Decision`] is truth and is appended
//! to the store. Without it every pass re-offers every host already rejected, which is the
//! one fault that would make the list not worth reading twice.
//!
//! ## One scan, two callers
//!
//! `investigate` runs this over a single seed it just fetched; `crumbs` runs it over every
//! HTML blob a Source has collected. They differ in where the bytes come from and in nothing
//! else, so they share the scan rather than each keeping one — `crate::html` exists because
//! two copies of a scanner drifted, one carrying a fix and the other carrying the bug.

use std::collections::{BTreeMap, BTreeSet};

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::render::Mark;
use crate::store::Store;

/// Pages named per crumb before the rest are counted and dropped.
///
/// A host linked from a thousand pages is a footer link, and the thousandth address that
/// carries it says nothing the tenth did not. The page *count* is kept whole — see
/// [`Crumb::pages`] — so the cap can never read like a host that was linked ten times.
pub const MAX_CARRIERS: usize = 10;

/// A page that carried a crumb.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Carrier {
    /// The address whose markup held the link.
    pub address: String,
    /// When that page was observed.
    ///
    /// Absent when the page was fetched rather than read out of the store: `investigate`
    /// holds a seed and has no Observation to date it by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// The blob the link was read out of, short form.
    ///
    /// So a crumb that looks wrong can be traced to the page that dropped it — anything
    /// Centinel prints, Centinel takes back. Absent for the same reason [`Self::at`] is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

impl Carrier {
    /// A page known by address alone — a seed, fetched now and stored nowhere.
    pub fn at_address(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            at: None,
            blob: None,
        }
    }
}

/// Whether a crumb still wants a decision.
///
/// Assigned by whoever holds the store, because two of the three answers are facts about the
/// store rather than about the page. [`Self::Open`] is what an unchecked crumb reads as, and
/// it is the honest default: `investigate` consults no rulings, so every crumb it names is
/// one nothing has yet been said about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    /// Nothing has been decided, and nothing collects it. The only kind that wants a person.
    #[default]
    Open,
    /// The operator refused it. Out of the list unless the list is asked for all of them.
    Ignored,
    /// A Source in this store already collects this host, so the promotion has happened.
    Collected,
}

impl Standing {
    /// So the default stays off the wire.
    pub fn is_open(&self) -> bool {
        matches!(self, Self::Open)
    }

    /// The glyph in the leftmost column. A crumb waiting on a person is not a fault, so
    /// `Open` carries no mark at all — the marked rows are the ones already dealt with.
    pub fn mark(&self) -> Mark {
        match self {
            Self::Open => Mark::None,
            Self::Ignored => Mark::None,
            Self::Collected => Mark::Ok,
        }
    }

    /// How it reads at the end of the row. Empty for the common case, which needs no word.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Open => "",
            Self::Ignored => "ignored",
            Self::Collected => "already a source",
        }
    }
}

/// One off-host host, and what points at it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Crumb {
    pub host: String,
    /// How many links pointed the same way.
    pub links: usize,
    /// How many pages carried them.
    ///
    /// Kept apart from [`Self::links`] because the two answer different questions. One page
    /// linking a host twenty times is a widget in a template; twenty pages linking it once
    /// each is a system the site depends on — and only the second is worth a Source.
    pub pages: usize,
    /// One target address on that host, to look at.
    pub example: String,
    /// Whether anything has been decided about it. See [`Standing`].
    #[serde(default, skip_serializing_if = "Standing::is_open")]
    pub standing: Standing,
}

/// One pass over pages, gathering the links that leave the host.
///
/// Holds no store and does no I/O: a page's markup and the address it came from is the whole
/// input. That is what lets one implementation serve a single fetched seed and a corpus of
/// blobs, and it is what makes the scan testable from a string.
#[derive(Debug, Default)]
pub struct Trail {
    by_host: BTreeMap<String, Tally>,
    pages: usize,
}

/// What one host has accumulated across the pages read so far.
#[derive(Debug)]
struct Tally {
    links: usize,
    pages: usize,
    example: String,
    carried_by: Vec<Carrier>,
    /// Carrying pages past [`MAX_CARRIERS`]. Counted rather than discarded, because a
    /// silent cap reads exactly like a host that only two pages linked.
    dropped: usize,
}

impl Trail {
    /// Reads one page: every `<a href>` whose target leaves the page's own host.
    ///
    /// The comparison is against the **page's** host and not a Source's, so a page served
    /// from somewhere a redirect landed is measured from where its own links resolve. Hosts
    /// are compared exactly, because "one Source per exact host" is the rule that bounds the
    /// walk — `www.example.gov` and `example.gov` are two answers to "what is this site",
    /// and collapsing them here would quietly decide it.
    pub fn read(&mut self, html: &str, page: Carrier) {
        self.pages += 1;

        let Ok(base) = url::Url::parse(&page.address) else {
            return;
        };
        let here = base.host_str().unwrap_or_default().to_string();

        // Which hosts this page has already contributed to, so `pages` counts pages and not
        // links — a template linking one host from four places is one page.
        let mut counted: BTreeSet<String> = BTreeSet::new();

        for tag in crate::html::Scan::new(html).tags(&["a"]) {
            let Some(href) = tag.attr("href") else {
                continue;
            };
            let Ok(target) = base.join(&crate::html::unescape(href)) else {
                continue;
            };
            let Some(host) = target.host_str() else {
                continue;
            };
            // `mailto:` and `tel:` carry no host worth walking, and neither does a link to
            // the page's own host — that is what `enumerate` already covers.
            if host == here || !matches!(target.scheme(), "http" | "https") {
                continue;
            }

            let tally = self
                .by_host
                .entry(host.to_string())
                .or_insert_with(|| Tally {
                    links: 0,
                    pages: 0,
                    example: target.to_string(),
                    carried_by: Vec::new(),
                    dropped: 0,
                });
            tally.links += 1;

            if counted.insert(host.to_string()) {
                tally.pages += 1;
                match tally.carried_by.len() < MAX_CARRIERS {
                    true => tally.carried_by.push(page.clone()),
                    false => tally.dropped += 1,
                }
            }
        }
    }

    /// How many pages were read, whatever they held.
    pub fn pages_read(&self) -> usize {
        self.pages
    }

    /// Every host found, **most-linked first**.
    ///
    /// A host named twenty times is a system; one named once is a footer link to the state
    /// portal. Ties break on the name so the order is the same on every machine and in every
    /// run rather than dependent on how a map happened to iterate.
    pub fn crumbs(&self) -> Vec<Crumb> {
        let mut out: Vec<Crumb> = self
            .by_host
            .iter()
            .map(|(host, tally)| Crumb {
                host: host.clone(),
                links: tally.links,
                pages: tally.pages,
                example: tally.example.clone(),
                standing: Standing::default(),
            })
            .collect();
        out.sort_by(|a, b| b.links.cmp(&a.links).then(a.host.cmp(&b.host)));
        out
    }

    /// The pages that carried one host, and how many were past the cap.
    ///
    /// Not on [`Crumb`], because only one caller wants it: a list of two hundred hosts each
    /// carrying ten addresses is a report nobody reads and a `--json` payload nobody parses.
    pub fn carriers_of(&self, host: &str) -> (Vec<Carrier>, usize) {
        match self.by_host.get(host) {
            Some(tally) => (tally.carried_by.clone(), tally.dropped),
            None => (Vec::new(), 0),
        }
    }
}

// ── the operator's ruling ─────────────────────────────────────────────────────

/// What the operator decided about a host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Ruling {
    /// Never offer this host again. A social network, a font CDN, a state portal.
    Ignore,
    /// Take back an earlier refusal, and let it be offered again.
    Allow,
}

impl Ruling {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ignore => "ignore",
            Self::Allow => "allow",
        }
    }
}

/// One ruling, appended when the operator makes it.
///
/// **Truth, and the fourth thing that is.** `blobs/` and `log/` are what the world served,
/// `runs/` is what this machine did, and this is what the operator decided. It is derived
/// from none of them: a crumb rebuilds from the blob it was read out of, and a refusal
/// cannot — no page records that a person said no.
///
/// **Corpus-wide, which is why it is not a `LogRecord`.** `log/` is per Source, and "this
/// host is not a Source" is not a fact about Tampa. Filed per Source, the hosts every city
/// links would have to be refused once per city, and a Source added next year would re-offer
/// every one of them.
///
/// Append-only, and the newest ruling for a host wins. That is what makes [`Ruling::Allow`]
/// able to reverse a refusal without erasing the fact that it was made.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Decision {
    pub host: String,
    pub ruling: Ruling,
    pub at: Timestamp,
    /// Why, when the operator says. Read back on the line that reports the refusal, because
    /// *"a vendor login"* is the difference between a decision and a mystery in six months.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Reads and appends the operator's rulings.
///
/// A thin thing on purpose, exactly like [`crate::journal::Journal`]: the path belongs to
/// [`Store`], and everything here is one append and one scan.
pub struct Decisions<'a> {
    store: &'a Store,
}

impl<'a> Decisions<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// Appends one ruling. Opened, written, flushed and closed per call, as the log is, so a
    /// crash cannot lose a buffered decision.
    pub async fn append(&self, decision: &Decision) -> anyhow::Result<()> {
        let path = self.store.decisions_path();
        if let Some(dir) = path.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }

        let mut line = serde_json::to_vec(decision)?;
        line.push(b'\n');

        let mut f = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        f.write_all(&line).await?;
        f.flush().await?;
        Ok(())
    }

    /// Every ruling ever made, in the order they were made.
    ///
    /// An empty file and an absent one are the same ordinary answer: nobody has decided
    /// anything yet.
    pub async fn read(&self) -> anyhow::Result<Vec<Decision>> {
        let path = self.store.decisions_path();
        let text = match tokio::fs::read_to_string(&path).await {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };

        let mut out = Vec::new();
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Decision>(line) {
                Ok(d) => out.push(d),
                // One unreadable line must not hide the rulings around it. It is also not
                // silent: this file holds dozens of lines for the life of a corpus, so a
                // warning here will be seen.
                Err(e) => tracing::warn!(
                    file = %path.display(),
                    line = n + 1,
                    error = %e,
                    "skipping an unreadable decision"
                ),
            }
        }
        Ok(out)
    }

    /// The ruling in force per host — the newest one for each.
    pub async fn current(&self) -> anyhow::Result<BTreeMap<String, Decision>> {
        let mut map: BTreeMap<String, Decision> = BTreeMap::new();
        for decision in self.read().await? {
            match map.get(&decision.host) {
                Some(prev) if prev.at > decision.at => {}
                _ => {
                    map.insert(decision.host.clone(), decision);
                }
            }
        }
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "https://www.tampa.gov/clerk/records";

    fn read(html: &str) -> Vec<Crumb> {
        let mut trail = Trail::default();
        trail.read(html, Carrier::at_address(PAGE));
        trail.crumbs()
    }

    /// The shape the whole feature exists for: a link that leaves the host is named, and
    /// one that does not is `enumerate`'s business.
    #[test]
    fn a_link_that_leaves_the_host_is_named_and_one_that_stays_is_not() {
        let crumbs = read(
            r#"<a href="/clerk/agendas">ours</a>
               <a href="https://www.tampa.gov/other">also ours</a>
               <a href="https://publicrec.hillsclerk.com/Civil/">records</a>"#,
        );
        assert_eq!(crumbs.len(), 1);
        assert_eq!(crumbs[0].host, "publicrec.hillsclerk.com");
        assert_eq!(crumbs[0].example, "https://publicrec.hillsclerk.com/Civil/");
    }

    /// Links and pages are two questions. A template linking one host four times from one
    /// page is one page, and reporting four would make a widget look like a system.
    #[test]
    fn links_are_counted_per_link_and_pages_per_page() {
        let mut trail = Trail::default();
        let widget = r#"<a href="https://hover.hillsclerk.com/a">a</a>
                        <a href="https://hover.hillsclerk.com/b">b</a>"#;
        trail.read(widget, Carrier::at_address(PAGE));
        trail.read(widget, Carrier::at_address("https://www.tampa.gov/two"));

        let crumbs = trail.crumbs();
        assert_eq!(crumbs[0].links, 4);
        assert_eq!(crumbs[0].pages, 2);
        assert_eq!(trail.pages_read(), 2);
    }

    /// Most-linked first: a host named twenty times is a system, one named once is a footer.
    #[test]
    fn the_most_linked_host_is_first_and_ties_break_on_the_name() {
        let crumbs = read(
            r#"<a href="https://b.example.com/1">1</a>
               <a href="https://a.example.com/1">1</a>
               <a href="https://busy.example.com/1">1</a>
               <a href="https://busy.example.com/2">2</a>"#,
        );
        let order: Vec<&str> = crumbs.iter().map(|c| c.host.as_str()).collect();
        assert_eq!(
            order,
            ["busy.example.com", "a.example.com", "b.example.com"]
        );
    }

    /// A host is not compared loosely. `www.` is a different exact host, and deciding
    /// otherwise here would decide what a Source is.
    #[test]
    fn a_host_is_matched_exactly() {
        let crumbs = read(r#"<a href="https://tampa.gov/x">bare</a>"#);
        assert_eq!(crumbs.len(), 1, "www.tampa.gov and tampa.gov are two hosts");
        assert_eq!(crumbs[0].host, "tampa.gov");
    }

    #[test]
    fn a_scheme_with_no_host_to_walk_is_not_a_crumb() {
        assert!(
            read(
                r##"<a href="mailto:clerk@tampa.gov">mail</a>
                    <a href="tel:+18135551234">call</a>
                    <a href="javascript:void(0)">nothing</a>
                    <a href="#top">up</a>"##
            )
            .is_empty()
        );
    }

    /// The entity that has to be decoded: a query string in an attribute is escaped, and
    /// joining it unescaped yields a different address.
    #[test]
    fn an_escaped_query_string_resolves_to_one_address() {
        let crumbs = read(r#"<a href="https://x.example.com/s?a=1&amp;b=2">search</a>"#);
        assert_eq!(crumbs[0].example, "https://x.example.com/s?a=1&b=2");
    }

    /// A crumb from a corpus knows which pages dropped it, and says how many it did not name.
    #[test]
    fn the_carrying_pages_are_named_up_to_the_cap_and_then_counted() {
        let mut trail = Trail::default();
        for i in 0..MAX_CARRIERS + 4 {
            trail.read(
                r#"<a href="https://vendor.example.com/login">login</a>"#,
                Carrier {
                    address: format!("https://www.tampa.gov/page/{i}"),
                    at: Some("2026-08-01T00:00:00Z".into()),
                    blob: Some("a1b2c3d4".into()),
                },
            );
        }

        let crumbs = trail.crumbs();
        assert_eq!(crumbs[0].pages, MAX_CARRIERS + 4, "the count stays whole");

        let (carriers, dropped) = trail.carriers_of("vendor.example.com");
        assert_eq!(carriers.len(), MAX_CARRIERS);
        assert_eq!(dropped, 4);
        assert_eq!(carriers[0].blob.as_deref(), Some("a1b2c3d4"));

        // A host nothing linked is an empty answer, not a missing one.
        assert_eq!(trail.carriers_of("nobody.example.com"), (Vec::new(), 0));
    }

    #[test]
    fn a_page_with_no_address_or_no_links_contributes_nothing() {
        let mut trail = Trail::default();
        trail.read(
            r#"<a href="https://x.example.com/">x</a>"#,
            Carrier::at_address("not a url"),
        );
        trail.read("<p>prose</p>", Carrier::at_address(PAGE));
        assert!(trail.crumbs().is_empty());
        assert_eq!(trail.pages_read(), 2, "a page read is a page read");
    }

    #[test]
    fn malformed_markup_does_not_panic() {
        for html in ["<a href=", "<a href='unclosed", "<<>><a", ""] {
            let _ = read(html);
        }
    }

    // ── rulings ───────────────────────────────────────────────────────────────

    async fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        (dir, store)
    }

    fn decision(host: &str, ruling: Ruling, at: &str) -> Decision {
        Decision {
            host: host.into(),
            ruling,
            at: at.parse().unwrap(),
            note: None,
        }
    }

    #[tokio::test]
    async fn nothing_decided_is_an_ordinary_state() {
        let (_d, store) = store().await;
        assert!(Decisions::new(&store).read().await.unwrap().is_empty());
        assert!(Decisions::new(&store).current().await.unwrap().is_empty());
    }

    /// The newest ruling wins, and the earlier one is still on disk — which is what lets a
    /// refusal be taken back without the record of it being erased.
    #[tokio::test]
    async fn allowing_a_host_reverses_the_refusal_and_keeps_it() {
        let (_d, store) = store().await;
        let decisions = Decisions::new(&store);

        decisions
            .append(&decision(
                "facebook.com",
                Ruling::Ignore,
                "2026-08-01T00:00:00Z",
            ))
            .await
            .unwrap();
        decisions
            .append(&decision(
                "facebook.com",
                Ruling::Allow,
                "2026-08-02T00:00:00Z",
            ))
            .await
            .unwrap();

        let current = decisions.current().await.unwrap();
        assert_eq!(current["facebook.com"].ruling, Ruling::Allow);
        assert_eq!(decisions.read().await.unwrap().len(), 2, "both are kept");
    }

    /// A ruling written out of order must not win on file position alone.
    #[tokio::test]
    async fn the_newest_ruling_wins_whatever_order_it_was_written_in() {
        let (_d, store) = store().await;
        let decisions = Decisions::new(&store);

        decisions
            .append(&decision(
                "x.example.com",
                Ruling::Allow,
                "2026-08-09T00:00:00Z",
            ))
            .await
            .unwrap();
        decisions
            .append(&decision(
                "x.example.com",
                Ruling::Ignore,
                "2026-08-01T00:00:00Z",
            ))
            .await
            .unwrap();

        let current = decisions.current().await.unwrap();
        assert_eq!(current["x.example.com"].ruling, Ruling::Allow);
    }

    /// One corrupt line must not hide the rulings around it.
    #[tokio::test]
    async fn an_unreadable_ruling_does_not_hide_the_rest() {
        let (_d, store) = store().await;
        let decisions = Decisions::new(&store);
        decisions
            .append(&decision(
                "a.example.com",
                Ruling::Ignore,
                "2026-08-01T00:00:00Z",
            ))
            .await
            .unwrap();

        let path = store.decisions_path();
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("{ not json\n");
        std::fs::write(&path, text).unwrap();

        assert_eq!(decisions.read().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_note_survives_the_round_trip() {
        let (_d, store) = store().await;
        let decisions = Decisions::new(&store);
        decisions
            .append(&Decision {
                note: Some("a vendor login, nothing to read".into()),
                ..decision("vendor.example.com", Ruling::Ignore, "2026-08-01T00:00:00Z")
            })
            .await
            .unwrap();

        let current = decisions.current().await.unwrap();
        assert_eq!(
            current["vendor.example.com"].note.as_deref(),
            Some("a vendor login, nothing to read")
        );
    }
}
