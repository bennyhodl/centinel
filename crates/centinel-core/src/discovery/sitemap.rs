//! Sitemap parsing.
//!
//! Written rather than taken from a crate because the Rust ecosystem genuinely has no
//! parser: `sitemap-rs` says verbatim *"This library **cannot** parse sitemaps of any
//! kind"*, and the one crate that can is from 2020 and UTF-8-only. Firecrawl reached the
//! same conclusion — their Rust crawl core parses sitemaps with `roxmltree` directly.
//!
//! This module is **pure**: bytes in, structure out, no network. Every quirk below was
//! measured against live `.gov` hosts and is a test at the bottom of this file.
//!
//! | Quirk | Where it was seen |
//! |---|---|
//! | `<sitemapindex>` nesting (index → index → urlset is legal) | spec |
//! | query-string `<loc>` values | `tampa.gov/sitemap.xml?page=1` … `?page=6` |
//! | `<?xml-stylesheet?>` PI before the root element | `tampa.gov` (Drupal `simple_sitemap`) |
//! | cross-host sitemap references | `hillsboroughcounty.org` → `hcfl.gov/sitemap` |
//! | gzip, sometimes with a lying `content-type` | common |
//! | UTF-8 BOM before the declaration | common |

use jiff::Timestamp;
use jiff::tz::TimeZone;

/// Refuse to decompress beyond this. A sitemap is text; anything larger is a zip bomb
/// or a mistake, and either way we should not hold it in memory.
const MAX_DECOMPRESSED: usize = 128 * 1024 * 1024;

/// One `<url>` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct SitemapEntry {
    pub loc: String,
    pub lastmod: Option<Timestamp>,
    pub changefreq: Option<String>,
    pub priority: Option<f32>,
}

/// One `<sitemap>` entry from an index — a pointer to another sitemap.
#[derive(Clone, Debug, PartialEq)]
pub struct SitemapRef {
    pub loc: String,
    pub lastmod: Option<Timestamp>,
}

/// What a sitemap document turned out to be.
///
/// The caller cannot know in advance: `robots.txt` advertises a URL, not a kind, and
/// `hillsboroughcounty.org` advertises one with no `.xml` extension at all.
#[derive(Clone, Debug, PartialEq)]
pub enum SitemapDoc {
    UrlSet(Vec<SitemapEntry>),
    Index(Vec<SitemapRef>),
}

impl SitemapDoc {
    pub fn len(&self) -> usize {
        match self {
            Self::UrlSet(v) => v.len(),
            Self::Index(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SitemapError {
    #[error("gzip decompression failed: {0}")]
    Gunzip(#[source] std::io::Error),

    #[error("sitemap exceeds {MAX_DECOMPRESSED} bytes decompressed")]
    TooLarge,

    #[error("not valid XML: {0}")]
    Xml(#[source] roxmltree::Error),

    #[error(
        "expected a sitemap but got an HTML page. \
         WAFs commonly serve a block page with a 200 status, so this often means \
         blocked rather than missing."
    )]
    LooksLikeHtml,

    #[error(
        "unexpected root element `{0}`: expected `urlset` or `sitemapindex`. \
         Some hosts serve an HTML error page with a 200 status."
    )]
    UnexpectedRoot(String),
}

/// Parses sitemap bytes into structure.
///
/// Handles gzip transparently by sniffing magic bytes rather than trusting
/// `content-type` — hosts serve `.xml.gz` as `text/xml` routinely.
pub fn parse(bytes: &[u8]) -> Result<SitemapDoc, SitemapError> {
    let decompressed;
    let raw = if is_gzip(bytes) {
        decompressed = gunzip(bytes)?;
        &decompressed[..]
    } else {
        bytes
    };

    let text = decode(raw);

    // Checked before parsing because roxmltree rejects HTML with `DtdDetected`, which
    // tells an operator nothing about what actually happened.
    if looks_like_html(&text) {
        return Err(SitemapError::LooksLikeHtml);
    }

    // roxmltree skips processing instructions and comments before the root element, so
    // Drupal's `<?xml-stylesheet?>` needs no special handling here.
    let doc = roxmltree::Document::parse(&text).map_err(SitemapError::Xml)?;
    let root = doc.root_element();

    // Match on the local name: real sitemaps are namespaced, hand-rolled ones often
    // are not, and neither is worth rejecting over.
    match root.tag_name().name() {
        "urlset" => Ok(SitemapDoc::UrlSet(
            root.children()
                .filter(|n| n.is_element() && n.tag_name().name() == "url")
                .filter_map(parse_url_entry)
                .collect(),
        )),
        "sitemapindex" => Ok(SitemapDoc::Index(
            root.children()
                .filter(|n| n.is_element() && n.tag_name().name() == "sitemap")
                .filter_map(parse_sitemap_ref)
                .collect(),
        )),
        other => Err(SitemapError::UnexpectedRoot(other.to_string())),
    }
}

fn is_gzip(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x1f, 0x8b])
}

/// Sniffs for an HTML document.
///
/// Only the leading bytes are examined, so a sitemap that legitimately mentions `<html`
/// inside a `<loc>` is not misclassified.
fn looks_like_html(text: &str) -> bool {
    let head = text.trim_start();
    let head = &head[..head.len().min(512)].to_ascii_lowercase();
    head.starts_with("<!doctype html") || head.starts_with("<html")
}

fn gunzip(bytes: &[u8]) -> Result<Vec<u8>, SitemapError> {
    use std::io::Read;
    let mut out = Vec::new();
    // `take` bounds the read so a zip bomb fails rather than exhausting memory.
    flate2::read::GzDecoder::new(bytes)
        .take(MAX_DECOMPRESSED as u64 + 1)
        .read_to_end(&mut out)
        .map_err(SitemapError::Gunzip)?;
    if out.len() > MAX_DECOMPRESSED {
        return Err(SitemapError::TooLarge);
    }
    Ok(out)
}

/// Strips a UTF-8 BOM and decodes lossily.
///
/// Lossy because a sitemap that is 99% valid UTF-8 with one bad byte should still yield
/// its URLs — dropping thousands of addresses over one mojibake character would be the
/// wrong trade for an archiver.
fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

fn child_text<'a>(node: &roxmltree::Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == name)
        .and_then(|n| n.text())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn parse_url_entry(node: roxmltree::Node<'_, '_>) -> Option<SitemapEntry> {
    // A `<url>` without a `<loc>` is meaningless; skip rather than fail the document.
    let loc = child_text(&node, "loc")?.to_string();
    Some(SitemapEntry {
        loc,
        lastmod: child_text(&node, "lastmod").and_then(parse_w3c_datetime),
        changefreq: child_text(&node, "changefreq").map(str::to_string),
        priority: child_text(&node, "priority").and_then(|s| s.parse().ok()),
    })
}

fn parse_sitemap_ref(node: roxmltree::Node<'_, '_>) -> Option<SitemapRef> {
    let loc = child_text(&node, "loc")?.to_string();
    Some(SitemapRef {
        loc,
        lastmod: child_text(&node, "lastmod").and_then(parse_w3c_datetime),
    })
}

/// Parses a W3C datetime, which the sitemap spec allows at several precisions.
///
/// `2026-08-02`, `2026-08-02T10:30:00Z` and `2026-08-02T10:30:00+02:00` are all legal.
/// A date without a time is treated as midnight UTC.
fn parse_w3c_datetime(s: &str) -> Option<Timestamp> {
    if let Ok(ts) = s.parse::<Timestamp>() {
        return Some(ts);
    }
    if let Ok(date) = s.parse::<jiff::civil::Date>() {
        return date.to_zoned(TimeZone::UTC).ok().map(|z| z.timestamp());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact shape measured at `www.tampa.gov/sitemap.xml`: a Drupal
    /// `simple_sitemap` index whose children are **query-string** URLs, preceded by an
    /// `<?xml-stylesheet?>` PI.
    const TAMPA_INDEX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<?xml-stylesheet type="text/xsl" href="/sitemap.xsl"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sitemap><loc>https://www.tampa.gov/sitemap.xml?page=1</loc><lastmod>2026-07-30T12:00:00+00:00</lastmod></sitemap>
  <sitemap><loc>https://www.tampa.gov/sitemap.xml?page=2</loc></sitemap>
</sitemapindex>"#;

    const URLSET: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>https://www.tampa.gov/city-council</loc>
    <lastmod>2026-07-15</lastmod>
    <changefreq>weekly</changefreq>
    <priority>0.8</priority>
  </url>
  <url><loc>https://www.tampa.gov/agenda?meeting=1042</loc></url>
</urlset>"#;

    #[test]
    fn parses_a_sitemapindex_and_keeps_query_strings() {
        let SitemapDoc::Index(refs) = parse(TAMPA_INDEX.as_bytes()).unwrap() else {
            panic!("expected an index");
        };
        assert_eq!(refs.len(), 2);
        // The measured failure mode: a "strip query params" normalizer run before
        // sitemap fetching would collapse all six children into one URL.
        assert_eq!(refs[0].loc, "https://www.tampa.gov/sitemap.xml?page=1");
        assert_eq!(refs[1].loc, "https://www.tampa.gov/sitemap.xml?page=2");
        assert!(refs[0].lastmod.is_some());
        assert!(refs[1].lastmod.is_none());
    }

    #[test]
    fn stylesheet_processing_instruction_is_not_mistaken_for_the_root() {
        assert!(matches!(
            parse(TAMPA_INDEX.as_bytes()).unwrap(),
            SitemapDoc::Index(_)
        ));
    }

    #[test]
    fn parses_a_urlset_with_optional_fields() {
        let SitemapDoc::UrlSet(entries) = parse(URLSET.as_bytes()).unwrap() else {
            panic!("expected a urlset");
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].changefreq.as_deref(), Some("weekly"));
        assert_eq!(entries[0].priority, Some(0.8));
        assert!(entries[0].lastmod.is_some(), "date-only lastmod must parse");

        // Query-string page URLs are extremely common on .gov agenda systems.
        assert_eq!(entries[1].loc, "https://www.tampa.gov/agenda?meeting=1042");
        assert!(entries[1].changefreq.is_none());
    }

    #[test]
    fn handles_gzip_regardless_of_content_type() {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(URLSET.as_bytes()).unwrap();
        let gz = enc.finish().unwrap();

        assert!(is_gzip(&gz));
        assert_eq!(parse(&gz).unwrap().len(), 2);
    }

    #[test]
    fn strips_a_utf8_bom() {
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(URLSET.as_bytes());
        assert_eq!(parse(&with_bom).unwrap().len(), 2);
    }

    #[test]
    fn accepts_sitemaps_without_a_namespace() {
        let bare = r#"<urlset><url><loc>https://x.gov/a</loc></url></urlset>"#;
        assert_eq!(parse(bare.as_bytes()).unwrap().len(), 1);
    }

    #[test]
    fn cross_host_references_pass_through_unchanged() {
        // hillsboroughcounty.org advertises a sitemap on a different host, with no
        // `.xml` extension. Rewriting or rejecting it would lose the whole corpus.
        let x = r#"<sitemapindex><sitemap><loc>https://hcfl.gov/sitemap</loc></sitemap></sitemapindex>"#;
        let SitemapDoc::Index(refs) = parse(x.as_bytes()).unwrap() else {
            panic!("expected an index");
        };
        assert_eq!(refs[0].loc, "https://hcfl.gov/sitemap");
    }

    /// A WAF block page served with a 200 status. The operator needs to know they were
    /// blocked, not that a DTD was detected.
    #[test]
    fn an_html_error_page_served_with_200_says_so_plainly() {
        for page in [
            &b"<!DOCTYPE html><html><body>Request blocked.</body></html>"[..],
            &b"<html><head><title>403</title></head></html>"[..],
            &b"\n  <!doctype HTML>\n<html>"[..],
        ] {
            assert!(
                matches!(parse(page).unwrap_err(), SitemapError::LooksLikeHtml),
                "should be recognised as HTML: {:?}",
                String::from_utf8_lossy(page)
            );
        }
    }

    #[test]
    fn a_loc_mentioning_html_is_not_mistaken_for_an_html_page() {
        let x = r#"<urlset><url><loc>https://x.gov/a.html</loc></url></urlset>"#;
        assert_eq!(parse(x.as_bytes()).unwrap().len(), 1);
    }

    #[test]
    fn well_formed_xml_with_the_wrong_root_names_that_root() {
        let x = r#"<rss version="2.0"><channel/></rss>"#;
        assert!(
            matches!(parse(x.as_bytes()).unwrap_err(), SitemapError::UnexpectedRoot(r) if r == "rss")
        );
    }

    #[test]
    fn entries_without_a_loc_are_skipped_not_fatal() {
        let x = r#"<urlset>
            <url><lastmod>2026-01-01</lastmod></url>
            <url><loc>https://x.gov/good</loc></url>
        </urlset>"#;
        let SitemapDoc::UrlSet(entries) = parse(x.as_bytes()).unwrap() else {
            panic!("expected a urlset");
        };
        assert_eq!(
            entries.len(),
            1,
            "one bad entry must not lose the good ones"
        );
    }

    #[test]
    fn malformed_xml_is_an_error() {
        assert!(matches!(
            parse(b"<urlset><url>").unwrap_err(),
            SitemapError::Xml(_)
        ));
    }

    #[test]
    fn w3c_datetimes_parse_at_every_legal_precision() {
        assert!(parse_w3c_datetime("2026-08-02").is_some());
        assert!(parse_w3c_datetime("2026-08-02T10:30:00Z").is_some());
        assert!(parse_w3c_datetime("2026-08-02T10:30:00+02:00").is_some());
        assert!(parse_w3c_datetime("whenever").is_none());
    }
}
