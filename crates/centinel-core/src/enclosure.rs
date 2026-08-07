//! An **enclosure**: a document a page carries at its own address, rather than contains.
//!
//! Named to keep its distance from [`crate::embed`], which is vectors and a different
//! stage entirely.
//!
//! A `.gov` page is often a wrapper. The readable content is a PDF the CMS renders in a
//! viewer, and the HTML around it is a date, a title, and the notice shown when the viewer
//! cannot draw. Extraction can only ever read the wrapper, because the document is at its
//! own address and nothing fetched it — so the page enters the corpus looking collected
//! and carrying nothing. Measured on `tampa.gov`: 915 of 1005 pages, 913 distinct PDFs,
//! none of them in the store.
//!
//! ## Three ways a page points at its document, and why all three are needed
//!
//! | | |
//! |---|---|
//! | `<embed>`, `<object>`, `<iframe>` | the declared way |
//! | `<a href>` to a document | attachments — an RFQ's drawings, a packet's exhibits |
//! | a document URL quoted inside `<script>` | a viewer that injects at runtime |
//!
//! The third looks like the least principled and is the one that matters most. Tampa runs
//! PDFObject, so the served HTML has **zero** `<embed>` tags — the address exists only as
//!
//! ```text
//! var pdfURL = "https://www.tampa.gov/sites/default/files/proclamation/2022/2022...pdf#view=Fit"
//! ```
//!
//! and no DOM query reaches it. Rather than name PDFObject, this takes any quoted URL in a
//! script that ends in an extension [`crate::extract`] has a reader for. A viewer's
//! configuration is the one place such a URL reliably appears, and a false positive costs
//! one extra document that is genuinely published on the site.
//!
//! ## What it deliberately does not do
//!
//! **One level.** The page's own HTML is scanned; nothing that comes back is. This finds
//! the document a page is *about*, not a crawl frontier — `enumerate` owns that, and a
//! second level would make acquisition a recursive crawler with no snapshot to bound it.
//!
//! **Same host only.** A Source is a site, and `robots.txt` was read for the site. An
//! iframe pointing somewhere else is a different publisher who has not been asked.

use std::collections::BTreeSet;

use crate::content::ContentKind;

/// How many documents one page may contribute, before the rest are dropped and counted.
///
/// A page carrying more than this is a listing, and a listing's documents belong to
/// `enumerate` — they are addresses in their own right, and the sitemap is where a
/// complete set of addresses comes from. Measured on `tampa.gov`, the busiest real page
/// carries ten.
pub const MAX_PER_PAGE: usize = 25;

/// What a page pointed at, and what did not fit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Enclosures {
    /// Absolute, de-duplicated, in the order they appear.
    pub urls: Vec<String>,
    /// Dropped by [`MAX_PER_PAGE`]. Counted rather than discarded, because a silent cap
    /// reads exactly like a page that had nothing.
    pub dropped: usize,
}

/// Every document `html` embeds, resolved against `base`.
pub fn documents(html: &str, base: &str, limit: usize) -> Enclosures {
    let Ok(base) = url::Url::parse(base) else {
        return Enclosures::default();
    };

    let mut seen = BTreeSet::new();
    let mut urls = Vec::new();
    let mut dropped = 0;

    // One lowercased copy of the page, asked both questions. They used to make one each.
    let scan = crate::html::Scan::new(html);

    for candidate in tag_targets(&scan).into_iter().chain(script_targets(&scan)) {
        let Some(absolute) = resolve(&base, &candidate) else {
            continue;
        };
        if !seen.insert(absolute.clone()) {
            continue;
        }
        match urls.len() < limit {
            true => urls.push(absolute),
            false => dropped += 1,
        }
    }

    Enclosures { urls, dropped }
}

/// Absolute, same-host, and something we could read. `None` for anything else.
fn resolve(base: &url::Url, candidate: &str) -> Option<String> {
    let joined = base.join(&crate::html::unescape(candidate)).ok()?;
    if !matches!(joined.scheme(), "http" | "https") {
        return None;
    }
    if joined.host_str() != base.host_str() {
        return None;
    }
    if !is_document(joined.path()) {
        return None;
    }
    // The fragment is a viewer instruction — `#view=Fit&toolbar=1` — not part of the
    // address. Keeping it would store one blob per zoom setting.
    let mut clean = joined;
    clean.set_fragment(None);
    Some(clean.to_string())
}

/// Whether a URL path ends in an extension we have a reader for.
///
/// The set is read off [`ContentKind::ENCLOSABLE`] rather than retyped here. Fetching
/// bytes no stage can turn into text spends a request to store something nothing will
/// search — and a list kept beside the table it must agree with drifts from it silently,
/// in the direction of a document at the end of a link that is never fetched at all.
fn is_document(path: &str) -> bool {
    let last = path.rsplit('/').next().unwrap_or(path);
    let Some((_, ext)) = last.rsplit_once('.') else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    ContentKind::enclosable_extensions().any(|e| e == ext)
}

/// `<embed src>`, `<object data>`, `<iframe src>`, `<a href>`.
fn tag_targets(scan: &crate::html::Scan<'_>) -> Vec<String> {
    let mut out = Vec::new();
    for tag in scan.tags(&["embed", "object", "iframe", "a"]) {
        let attribute = match tag.name {
            "object" => "data",
            "a" => "href",
            _ => "src",
        };
        if let Some(value) = tag.attr(attribute) {
            out.push(value.to_string());
        }
    }
    out
}

/// Quoted document URLs inside `<script>` blocks — the runtime viewer's configuration.
fn script_targets(scan: &crate::html::Scan<'_>) -> Vec<String> {
    let mut out = Vec::new();
    for body in scan.scripts() {
        for quoted in crate::html::quoted_strings(body) {
            // Checked before resolving so a script full of ordinary strings costs a
            // suffix test rather than a URL parse each.
            if is_document(quoted.split(['#', '?']).next().unwrap_or(quoted)) {
                out.push(quoted.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://www.tampa.gov/proclamation/irish-american-heritage-month";

    fn found(html: &str) -> Vec<String> {
        documents(html, BASE, MAX_PER_PAGE).urls
    }

    /// The case that motivated this: PDFObject, so no `<embed>` exists to find. Verbatim
    /// from a collected `tampa.gov` blob, entities and fragment included.
    #[test]
    fn a_viewer_that_injects_at_runtime_is_still_found() {
        let html = r#"<div class='pdf-reader'><div id="pdf_reader"></div>
            <script>
            var options = { height: "980px", PDFJS_URL: "/libraries/pdfjs-full/web/viewer.html" };
            var pdfURL = "https://www.tampa.gov/sites/default/files/proclamation/2022/20220301_Irish.pdf#view=Fit&amp;toolbar=1"
            </script></div>"#;

        assert_eq!(
            found(html),
            vec!["https://www.tampa.gov/sites/default/files/proclamation/2022/20220301_Irish.pdf"],
            "the viewer's own html file is not a document, and the fragment is not an address"
        );
    }

    #[test]
    fn the_declared_tags_are_found_too() {
        let html = r#"<embed src="/a.pdf"><object data="/b.docx"></object>
            <iframe src="/c.xlsx"></iframe>"#;
        assert_eq!(
            found(html),
            vec![
                "https://www.tampa.gov/a.pdf",
                "https://www.tampa.gov/b.docx",
                "https://www.tampa.gov/c.xlsx",
            ]
        );
    }

    /// An RFQ's attachments are the content of the page, and they are ordinary links.
    #[test]
    fn attachments_linked_from_the_page_are_documents() {
        let html = r#"<p>Protected Attached Files:</p>
            <ul><li><a href="/sites/default/files/rfq/drawings1.pdf">Drawings</a> (89.63 MB)</li></ul>
            <a href="/contract-administration/rfq">Back to listing</a>"#;
        assert_eq!(
            found(html),
            vec!["https://www.tampa.gov/sites/default/files/rfq/drawings1.pdf"],
            "an ordinary page link is not a document"
        );
    }

    #[test]
    fn a_document_is_named_once_however_many_ways_the_page_points_at_it() {
        let html = r#"<a href="/a.pdf">download</a><embed src="/a.pdf">
            <script>var pdfURL = "/a.pdf#page=2"</script>"#;
        assert_eq!(found(html), vec!["https://www.tampa.gov/a.pdf"]);
    }

    /// A Source is a site, and `robots.txt` was read for that site.
    #[test]
    fn another_publishers_document_is_left_alone() {
        let html = r#"<iframe src="https://docs.google.com/viewer/x.pdf"></iframe>
            <a href="https://example.com/other.pdf">elsewhere</a>
            <a href="/ours.pdf">ours</a>"#;
        assert_eq!(found(html), vec!["https://www.tampa.gov/ours.pdf"]);
    }

    #[test]
    fn formats_nothing_can_read_are_not_fetched() {
        let html = r#"<a href="/plans.dwg">CAD</a><a href="/logo.png">image</a>
            <a href="/page.html">page</a><a href="/notes.PDF">shouty</a>"#;
        assert_eq!(
            found(html),
            vec!["https://www.tampa.gov/notes.PDF"],
            "the extension is matched case-insensitively; .dwg has no reader"
        );
    }

    /// A silent cap reads exactly like a page that had nothing to give.
    #[test]
    fn a_listing_page_is_capped_and_says_how_many_it_dropped() {
        let html: String = (0..40)
            .map(|i| format!(r#"<a href="/doc{i}.pdf">d</a>"#))
            .collect();
        let out = documents(&html, BASE, MAX_PER_PAGE);
        assert_eq!(out.urls.len(), MAX_PER_PAGE);
        assert_eq!(out.dropped, 40 - MAX_PER_PAGE);
    }

    #[test]
    fn a_page_with_no_documents_yields_none() {
        assert!(found("<p>Just prose, and <a href='/about'>a link</a>.</p>").is_empty());
        assert!(found("").is_empty());
        assert!(
            documents("<embed src='/a.pdf'>", "not a url", 10)
                .urls
                .is_empty()
        );
    }

    /// `data` must not match inside `formdata`, or a form would contribute an address.
    ///
    /// Asked through `documents` rather than of the scanner directly: the scanner is
    /// shared now, and what this file is entitled to assert is what it does with one.
    /// The scanner's own version of this guarantee lives in `html`.
    #[test]
    fn an_attribute_is_matched_whole() {
        let found = documents(
            r#"<object formdata="/wrong.pdf" data="/right.pdf"></object>"#,
            "https://x.gov/page",
            10,
        );
        assert_eq!(found.urls, vec!["https://x.gov/right.pdf"]);

        // And `data-src` is not `src`.
        assert!(
            documents(r#"<embed data-src="/x.pdf">"#, "https://x.gov/page", 10)
                .urls
                .is_empty()
        );
    }

    #[test]
    fn malformed_markup_does_not_panic_or_hang() {
        for html in [
            "<embed src=",
            "<embed src='unclosed",
            "<script>var a = 'x",
            "<<<>>><embed",
            "<script>",
            "<a href='/a.pdf'",
        ] {
            let _ = documents(html, BASE, 10);
        }
    }
}
