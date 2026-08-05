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

/// Extensions worth their own blob.
///
/// The set [`crate::extract`] already has a reader for, and no wider: fetching bytes no
/// stage can turn into text spends a request to store something nothing will search.
const DOCUMENT_EXTENSIONS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "rtf", "odt", "ods", "odp", "epub", "csv",
];

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

    for candidate in tag_targets(html).into_iter().chain(script_targets(html)) {
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
    let joined = base.join(&unescape(candidate)).ok()?;
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
fn is_document(path: &str) -> bool {
    let last = path.rsplit('/').next().unwrap_or(path);
    let Some((_, ext)) = last.rsplit_once('.') else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    DOCUMENT_EXTENSIONS.contains(&ext.as_str())
}

/// `<embed src>`, `<object data>`, `<iframe src>`, `<a href>`.
fn tag_targets(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (name, tag) in tags(html, &["embed", "object", "iframe", "a"]) {
        let attribute = match name.as_str() {
            "object" => "data",
            "a" => "href",
            _ => "src",
        };
        if let Some(value) = attr(&tag, attribute) {
            out.push(value.to_string());
        }
    }
    out
}

/// Quoted document URLs inside `<script>` blocks — the runtime viewer's configuration.
fn script_targets(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;

    while let Some(open) = lower[from..].find("<script").map(|i| i + from) {
        let Some(body_start) = lower[open..].find('>').map(|i| open + i + 1) else {
            break;
        };
        let body_end = lower[body_start..]
            .find("</script")
            .map(|i| body_start + i)
            .unwrap_or(html.len());

        for quoted in quoted_strings(&html[body_start..body_end]) {
            // Checked before resolving so a script full of ordinary strings costs a
            // suffix test rather than a URL parse each.
            if is_document(quoted.split(['#', '?']).next().unwrap_or(quoted)) {
                out.push(quoted.to_string());
            }
        }
        from = body_end;
    }
    out
}

/// Every single- or double-quoted run in a fragment of script.
fn quoted_strings(script: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = script.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let quote = bytes[i];
        if quote == b'"' || quote == b'\'' {
            if let Some(end) = script[i + 1..].find(quote as char) {
                out.push(&script[i + 1..i + 1 + end]);
                i += end + 2;
                continue;
            }
            break;
        }
        i += 1;
    }
    out
}

/// Every `<name …>` in `html` whose name is wanted, as `(name, whole tag)`.
///
/// A scan rather than a parse: this asks one question of the markup, and the answer does
/// not change with a malformed table three elements up. `to_ascii_lowercase` keeps byte
/// offsets aligned with the original, so the returned slices carry the real casing.
fn tags(html: &str, want: &[&str]) -> Vec<(String, String)> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;

    while let Some(open) = lower[from..].find('<').map(|i| i + from) {
        let after = open + 1;
        let name_end = lower[after..]
            .find(|c: char| !c.is_ascii_alphanumeric())
            .map(|i| after + i)
            .unwrap_or(lower.len());
        let Some(close) = lower[open..].find('>').map(|i| i + open) else {
            break;
        };
        let name = &lower[after..name_end.min(close)];
        if want.contains(&name) {
            out.push((name.to_string(), html[open..close].to_string()));
        }
        from = close + 1;
    }
    out
}

/// The value of `name="…"`, single- or double-quoted.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find(name).map(|i| i + from) {
        // A whole attribute, not a suffix of another: `data` must not match `formdata`.
        let boundary = at == 0
            || !lower.as_bytes()[at - 1].is_ascii_alphanumeric()
                && lower.as_bytes()[at - 1] != b'-';
        let rest = &tag[at + name.len()..];
        if boundary && let Some(eq) = rest.find('=') {
            let after = rest[eq + 1..].trim_start();
            if let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') {
                let value = &after[1..];
                if let Some(end) = value.find(quote) {
                    return Some(&value[..end]);
                }
            }
        }
        from = at + name.len();
    }
    None
}

/// The entities that appear in real URLs. `&amp;` is the one that matters — a query
/// string in an attribute is escaped, and joining it unescaped yields a different address.
fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&quot;", "\"")
        .trim()
        .to_string()
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
    #[test]
    fn an_attribute_is_matched_whole() {
        assert_eq!(
            attr(r#"<object formdata="/x.pdf" data="/y.pdf""#, "data"),
            Some("/y.pdf")
        );
        assert_eq!(attr(r#"<embed data-src="/x.pdf""#, "src"), None);
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
