//! `marked` — the page says where its own content is, so read that.
//!
//! The first read strategy, and the one that decides what this registry is for.
//!
//! ## What it is keyed on, and why that is allowed
//!
//! [`Keyed::Standard`], the same key `crawl`'s `sitemap` uses. `<main>` and `role="main"`
//! are the HTML sectioning standard, not one vendor's markup — which is exactly why this
//! belongs here rather than beside a product: it is a rule about **HTML**, and any server
//! can satisfy it. Measured over the six sites in `docs/FIELD-NOTES.md` entry 5, **298 of
//! 300** documents carry one:
//!
//! ```text
//! main          198
//! [role=main]   100
//! none            2
//! ```
//!
//! ## Why it runs before readability rather than after it
//!
//! Because readability is a guess about where the content is, and this is the page's own
//! answer. `dom_smoothie` scores blocks by text density, which works on an article and
//! fails on a page whose content is a short fact block — there the densest non-navigation
//! block is often a contact panel. Entry 5's landmark template shows both failures on
//! adjacent pages of identical markup, which is what rules out keying anything on the site.
//!
//! The marked region recovers what density scoring lost, and drops what it wrongly kept:
//!
//! | page | fact | readability | marked |
//! |---|---|---|---|
//! | `czech-sokol-hall` | `Mitermiler`, `Clark Avenue` | absent | **present** |
//! | `czech-sokol-hall` | City Hall's phone number | present | **gone** |
//! | `black-history-boston` | `Melnea Cass`, `Reggie Lewis` | absent | **present** |
//! | `denison-cemetery` | `1835`, `Garden Avenue` | present | present |
//!
//! ## The cost, and what pays it
//!
//! A marked region is **broader** than readability's pick: it includes in-page
//! sub-navigation, so a page readability already handled cleanly comes out noisier — the
//! Cleveland police page goes from 4,248 characters at 10% link text to 12,478 at 50%.
//!
//! That is the deliberate trade. Sub-navigation repeats across a source, so
//! [`crate::boilerplate`] removes it corpus-wide, and removing repeated chrome is the job
//! that pass was written for — as opposed to compensating for a reader that picked the
//! wrong region, which is what it had been doing. Content that was never extracted cannot
//! be recovered by any later stage; chrome that was extracted can be dropped by one.
//!
//! **The links stay.** They are not waste: `enclosure::documents` scans the *raw bytes* of
//! the page rather than this text, so narrowing the read costs discovery nothing, and the
//! links that remain in the text are how a reader gets from a record to the document it
//! cites.
//!
//! ## What it does not do
//!
//! It does not judge the region it found. If the marked region yields nothing, this answers
//! nothing and `extract`'s reader list runs exactly as before — readability, then the whole
//! page. A strategy that recognised a document and then read nothing out of it does not get
//! to stop the readers below it, and `crate::extract::derive` enforces that.

use futures::future::BoxFuture;

use super::{Document, Strategy, StrategyDef};
use crate::content::ContentKind;
use crate::extract::{Extracted, Extraction};
use crate::strategies::{Keyed, Recognition};

pub struct Marked;

inventory::submit! { StrategyDef { name: "marked", it: &Marked } }

/// Content markers, **outermost first**, and the order is load-bearing.
///
/// `<main>` contains `<article>` on the Cleveland landmark template, and the landmark
/// record sits between the two — at byte 112,264 where `<main>` opens at 109,469 and
/// `<article>` at 113,024. Taking the most specific marker would miss the one fact the page
/// exists to publish. So the rule is the widest region the page marks as its own, and the
/// narrower ones are there for pages that mark nothing else.
const MARKERS: &[&str] = &[
    "main",
    "[role=main]",
    "#main-content",
    ".main-content",
    "article",
];

/// Dropped inside the region. See the module docs: within a marked region these are
/// sub-navigation, and the page has already told us the region is the document.
const CHROME: &[&str] = &["nav", "header", "footer"];

/// Cheap enough to run on every document.
///
/// A substring test rather than a parse, because `recognise` is asked of every document and
/// a DOM parse per candidate would be paid on documents this never reads. It over-answers —
/// a `<main>` inside a comment would pass — and that costs nothing, because [`Marked::read`]
/// does the real selection and answering nothing there falls through to the reader list.
fn marker_in(html: &str) -> Option<&'static str> {
    let lower = html.to_ascii_lowercase();
    [
        ("<main", "main"),
        ("role=\"main\"", "[role=main]"),
        ("role='main'", "[role=main]"),
        ("id=\"main-content\"", "#main-content"),
        ("<article", "article"),
    ]
    .into_iter()
    .find(|(needle, _)| lower.contains(needle))
    .map(|(_, name)| name)
}

/// The outermost marked region, as markdown, and the selector that found it.
fn read_region(html: &str) -> Option<(String, &'static str)> {
    let doc = dom_query::Document::from(html);
    let converter = crate::extract::markdown_converter_skipping(CHROME);

    for selector in MARKERS {
        let Some(node) = doc
            .try_select(selector)
            .and_then(|s| s.nodes().first().cloned())
        else {
            continue;
        };
        let Ok(md) = converter.convert(&node.html()) else {
            continue;
        };
        let md = md.trim().to_string();
        if !md.is_empty() {
            return Some((md, selector));
        }
    }
    None
}

impl Strategy for Marked {
    fn name(&self) -> &'static str {
        "marked"
    }

    fn recognise(&self, doc: &Document) -> Option<Recognition> {
        if doc.kind != ContentKind::Html {
            return None;
        }
        let marker = marker_in(&doc.text())?;
        Some(
            Recognition::new(self.name(), Keyed::Standard("HTML sectioning"))
                .seeing("marker", marker.to_string()),
        )
    }

    fn read<'a>(&'a self, doc: &'a Document<'a>) -> BoxFuture<'a, Extracted> {
        Box::pin(async move {
            let html = doc.text();
            let Some((body, selector)) = read_region(&html) else {
                return Extracted::Unextractable {
                    reason: "the page marks a content region and it holds no text".into(),
                };
            };

            // The same title rule the other HTML readers use, so a document does not change
            // heading path depending on which reader spoke. `<title>`/`og:title` rather
            // than the region's own first heading, because the region often opens with a
            // section name — `Landmark Details` — and not the page's subject.
            let title = crate::extract::html_title(&html);
            Extracted::Text(Extraction {
                text: crate::extract::with_title(title.as_deref(), &body),
                title,
                tool: "marked+htmd".into(),
                version: crate::extract::HTMD_VERSION.into(),
                notes: vec![format!("read the region marked `{selector}`")],
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(html: &str) -> Document<'_> {
        Document {
            kind: ContentKind::Html,
            bytes: html.as_bytes(),
            path: std::path::Path::new("/dev/null"),
            url: Some("https://x.gov/a"),
        }
    }

    /// The Cleveland landmark template, in miniature: navigation, then a marked region
    /// holding the record, then an `<article>` that does **not** contain it.
    #[tokio::test]
    async fn the_record_between_main_and_article_is_kept() {
        let nav: String = (0..40)
            .map(|i| format!("<li><a href=\"/d/{i}\">Department {i}</a></li>"))
            .collect();
        let html = format!(
            "<html><body><nav><ul>{nav}</ul></nav>\
             <main><div class=\"sidebar\"><h2>Landmark Details</h2><p>1890</p>\
             <p>4314 Clark Avenue</p><p>Architect</p><p>Andrew Mitermiler</p></div>\
             <article><p>Cleveland landmarks are designated by ordinance.</p></article>\
             </main><footer><a href=\"/x\">Contact</a></footer></body></html>"
        );

        let out = Marked.read(&doc(&html)).await;
        let text = out.text().expect("the region has text");
        assert!(text.contains("Mitermiler"), "the record was lost:\n{text}");
        assert!(text.contains("4314 Clark Avenue"), "{text}");
        assert!(
            !text.contains("Department 1"),
            "navigation outside the region leaked:\n{text}"
        );
        assert!(
            !text.contains("Contact"),
            "the footer inside the page leaked:\n{text}"
        );
    }

    /// `<main>` before `<article>`, which is the whole reason the order is written down.
    #[tokio::test]
    async fn the_widest_marked_region_wins_over_a_narrower_one_inside_it() {
        let html = "<html><body><main><p>Outer fact worth keeping.</p>\
                    <article><p>Inner prose.</p></article></main></body></html>";
        let text = Marked
            .read(&doc(html))
            .await
            .text()
            .expect("text")
            .to_string();
        assert!(text.contains("Outer fact"), "{text}");
        assert!(text.contains("Inner prose"), "{text}");
    }

    /// A table inside the region keeps its boundaries — the converter's handler travels.
    #[tokio::test]
    async fn a_headerless_table_inside_the_region_keeps_its_cells() {
        let html = "<html><body><main><table><tbody>\
                    <tr><td>Transcript #1</td><td>8/3/2026</td><td>Council</td></tr>\
                    </tbody></table></main></body></html>";
        let text = Marked
            .read(&doc(html))
            .await
            .text()
            .expect("text")
            .to_string();
        assert!(text.contains("| 8/3/2026 |"), "the table fused:\n{text}");
    }

    #[test]
    fn a_page_with_no_marker_is_not_recognised() {
        let html = "<html><body><div><p>Just a div.</p></div></body></html>";
        assert!(Marked.recognise(&doc(html)).is_none());
    }

    #[test]
    fn a_pdf_is_never_recognised_however_its_bytes_read() {
        let d = Document {
            kind: ContentKind::Pdf,
            bytes: b"%PDF-1.4 <main> not markup",
            path: std::path::Path::new("/dev/null"),
            url: None,
        };
        assert!(Marked.recognise(&d).is_none());
    }

    /// Recognition is deliberately loose and reading is where it is settled. A marker that
    /// holds nothing must not stop the readers below — `derive` continues past a strategy
    /// that produced no text, and this is the case that relies on it.
    #[tokio::test]
    async fn a_marked_region_holding_nothing_answers_nothing() {
        let html = "<html><body><main>   </main><p>Prose outside the region.</p></body></html>";
        assert!(
            Marked.recognise(&doc(html)).is_some(),
            "the marker is there"
        );
        let out = Marked.read(&doc(html)).await;
        assert!(out.text().is_none_or(str::is_empty), "{out:?}");
    }
}
