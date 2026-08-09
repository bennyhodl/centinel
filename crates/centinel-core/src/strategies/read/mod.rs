//! Recognising a document, and reading it.
//!
//! The **read** side of [`super`]: what does this document say. [`super::crawl`] found the
//! address; nothing it learned along the way says how to get the text out.
//!
//! ## Why this is a separate registry and not a hook on a crawl strategy
//!
//! Because the two do not line up. `hillsclerk.com` needs no crawl strategy at all — the
//! sitemap standard enumerates it perfectly — and it is the site whose reads are ruined.
//! `publicrec.hillsclerk.com` needs a crawl strategy, `listing`, and its reads are fine. A
//! read hook hanging off a crawl strategy could serve neither.
//!
//! ## What a read strategy is for, and what it is not for
//!
//! [`crate::extract`] already dispatches on [`ContentKind`] to a list of readers, primary
//! then fallback. That list is the right answer for almost everything and this registry
//! does not replace it. A read strategy exists for the case the content kind cannot
//! express: bytes that are HTML by every measure, where *which part* of them is the
//! document depends on a product or a framework.
//!
//! It is **not** a per-site extraction hook. `super`'s rule holds on this side without
//! change — key on a product, a framework, a server default or a standard, never a
//! jurisdiction — and [`super::Keyed`] enforces it by having no variant for one. The reason is
//! the same one `crawl` was built for: ship a per-site hook and the next framework defect
//! gets worked around in forty places instead of fixed once, and then the fix cannot land
//! because forty workarounds depend on the bug.
//!
//! ## It is empty, and that is the design working
//!
//! No strategy is registered. Every extraction fault found in `docs/FIELD-NOTES.md` — the
//! fused table, the spelled-out image, the `data:` URI, the octet-stream PDF — was a
//! framework defect that any site triggers, and each was fixed in the framework. The two
//! outstanding read faults are the same: navigation returned instead of an article is
//! handled corpus-wide by [`crate::boilerplate`], and text hidden behind a `var pdfURL` is
//! an enclosure question.
//!
//! So this is a place, deliberately unfurnished. The registry is consulted on every
//! derivation, a registered strategy wins over the content kind's readers, and the
//! two-sighting rule in `docs/FIELD-NOTES.md` decides when the first one is written. An
//! empty registry that is wired up costs one `is_empty` check per document; an unwired one
//! costs a refactor at exactly the moment somebody has a shape to add.

use futures::future::BoxFuture;

use super::Recognition;
use crate::content::ContentKind;
use crate::extract::Extracted;

/// A document, as the readers are given it.
///
/// The bytes **and** a path, because some readers shell out to a tool that needs a file.
/// Both, rather than one derived from the other, because the caller already has both.
pub struct Document<'a> {
    pub kind: ContentKind,
    pub bytes: &'a [u8],
    pub path: &'a std::path::Path,
    /// Where it came from, where that is known. A recogniser may key on a host or a path
    /// shape — `*.hylandcloud.com` is a product, not a jurisdiction.
    pub url: Option<&'a str>,
}

impl Document<'_> {
    /// The bytes as text, lossily. Recognisers work on this; nothing else should.
    pub fn text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(self.bytes)
    }
}

/// Recognise a document, and read the one you recognised.
///
/// The same pairing [`super::crawl::Strategy`] enforces, for the same reason: a recogniser
/// that cannot then handle what it claimed leaves the pipeline holding a confident
/// half-answer, which is the most expensive failure this codebase has.
pub trait Strategy: Send + Sync {
    fn name(&self) -> &'static str;

    /// Pure over bytes already in hand. **A read strategy never fetches** — unlike
    /// [`super::crawl`], where that rule had to be relaxed because a walk is fetching by
    /// definition, reading is not, and there is no reason to give this side a budget.
    fn recognise(&self, doc: &Document) -> Option<Recognition>;

    /// The text, instead of what the content kind would have produced.
    fn read<'a>(&'a self, doc: &'a Document<'a>) -> BoxFuture<'a, Extracted>;
}

/// A registered read strategy.
pub struct StrategyDef {
    pub name: &'static str,
    pub it: &'static (dyn Strategy + Sync),
}

impl std::fmt::Debug for StrategyDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

impl PartialEq for StrategyDef {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for StrategyDef {}

inventory::collect!(StrategyDef);

pub fn all() -> Vec<&'static StrategyDef> {
    let mut defs: Vec<_> = inventory::iter::<StrategyDef>().collect();
    defs.sort_by_key(|d| d.name);
    defs
}

pub fn by_name(name: &str) -> anyhow::Result<&'static StrategyDef> {
    all().into_iter().find(|d| d.name == name).ok_or_else(|| {
        match all().iter().map(|d| d.name).collect::<Vec<_>>() {
            names if names.is_empty() => anyhow::anyhow!("no read strategy `{name}`; none exist"),
            names => anyhow::anyhow!("no read strategy `{name}`; known: {}", names.join(", ")),
        }
    })
}

/// Everything that recognises this document, most specific first.
///
/// Every strategy is asked, exactly as on the crawl side, because recognition is pure over
/// bytes in hand and a document can satisfy more than one. Precedence is
/// [`super::Keyed::specificity`]: a product beats a standard every server can satisfy.
pub fn recognise(doc: &Document) -> Vec<Recognition> {
    let mut hits: Vec<Recognition> = all().iter().filter_map(|d| d.it.recognise(doc)).collect();
    hits.sort_by_key(|r| (r.keyed_on.specificity(), r.strategy));
    hits
}

/// The strategy that should read this document, if any recognised it.
pub fn best(doc: &Document) -> Option<&'static StrategyDef> {
    recognise(doc)
        .first()
        .and_then(|r| by_name(r.strategy).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Registered only in a test build, and recognising only a marker no real document
    /// carries — so the empty-registry tests below still describe the shipped registry
    /// while the wiring is proved against a real one.
    const MARKER: &str = "__centinel_test_read__";

    struct Marked;

    inventory::submit! { StrategyDef { name: "test-marker", it: &Marked } }

    impl Strategy for Marked {
        fn name(&self) -> &'static str {
            "test-marker"
        }

        fn recognise(&self, doc: &Document) -> Option<Recognition> {
            doc.text()
                .contains(MARKER)
                .then(|| Recognition::new(self.name(), super::super::Keyed::Product("Marked")))
        }

        fn read<'a>(&'a self, _doc: &'a Document<'a>) -> BoxFuture<'a, Extracted> {
            Box::pin(async {
                Extracted::Text(crate::extract::Extraction {
                    text: "read by the strategy".to_string(),
                    title: None,
                    tool: "test-marker".to_string(),
                    version: "0".to_string(),
                    notes: Vec::new(),
                })
            })
        }
    }

    /// The point of wiring an empty registry: when something *is* registered, it runs
    /// instead of the reader the content kind would have chosen. Without this the whole
    /// module is a claim rather than a mechanism.
    #[tokio::test]
    async fn a_recognised_document_is_read_by_its_strategy_and_not_by_the_kind() {
        let html = format!("<html><body><p>{MARKER} and some prose.</p></body></html>");
        let file = tempfile::NamedTempFile::new().expect("temp");
        std::fs::write(file.path(), &html).expect("write");

        let derived =
            crate::extract::derive(ContentKind::Html, html.as_bytes(), file.path(), None, None)
                .await;

        let text = derived.outcome.text().unwrap_or_default();
        assert_eq!(text, "read by the strategy", "the kind's reader won");
        assert!(!derived.recovered_by_fallback);
    }

    /// The same bytes without the marker take the ordinary path, which is what every
    /// document in the corpus does today.
    #[tokio::test]
    async fn an_unrecognised_document_still_takes_the_readers_for_its_kind() {
        let html = "<html><body><article><p>Ordinary prose about the county clerk and \
                    the records it keeps for the public.</p></article></body></html>";
        let file = tempfile::NamedTempFile::new().expect("temp");
        std::fs::write(file.path(), html).expect("write");

        let derived =
            crate::extract::derive(ContentKind::Html, html.as_bytes(), file.path(), None, None)
                .await;

        let text = derived.outcome.text().unwrap_or_default();
        assert!(text.contains("county clerk"), "got: {text}");
        assert_ne!(text, "read by the strategy");
    }

    /// The registry is empty on purpose — see the module docs — so this asserts the shape
    /// rather than a population. If a strategy is ever added, its name must match its
    /// registration or `best` returns `None` for a document something recognised.
    #[test]
    fn every_registration_agrees_with_the_name_its_strategy_reports() {
        for def in all() {
            assert_eq!(
                def.name,
                def.it.name(),
                "`{}` is registered under a name it does not answer to",
                def.name
            );
        }
    }

    #[test]
    fn an_unknown_name_says_what_is_known() {
        let err = by_name("onbase").unwrap_err().to_string();
        assert!(err.contains("onbase"), "{err}");
    }

    #[test]
    fn nothing_recognises_an_ordinary_page() {
        let doc = Document {
            kind: ContentKind::Html,
            bytes: b"<html><body><p>An ordinary page.</p></body></html>",
            path: std::path::Path::new("/dev/null"),
            url: Some("https://x.gov/"),
        };
        assert!(recognise(&doc).is_empty());
        assert!(best(&doc).is_none());
    }
}
