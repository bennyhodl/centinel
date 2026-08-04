//! Turning stored bytes into text.
//!
//! Extraction reads from the blob pool, never from the network. That is what makes
//! re-derivation possible: when a better PDF library lands, every document in the
//! corpus can be re-extracted without touching a single `.gov` server (SPEC §4.3).
//!
//! ## The HTML pipeline is two steps, and that was measured
//!
//! `htmd` is a *serializer* — excellent at HTML→markdown, but it keeps whatever you
//! hand it. Run directly on a real `tampa.gov` page:
//!
//! | Stage | Bytes |
//! |---|---|
//! | raw HTML | 91,444 |
//! | `htmd` with default options | 33,998 — includes JSON-LD and `drupalSettings` |
//! | `htmd` skipping `script`/`style` | 11,003 — language picker, menus, footer |
//! | **`dom_smoothie` → `htmd`** | **900 — the actual content** |
//!
//! That 11 KB of chrome is byte-identical across all 11,476 Tampa pages. Indexing it
//! would not merely waste space: it would make every page look like every other page
//! to an embedding model, which is precisely the failure semantic search cannot survive.

use std::io::Cursor;

use serde::{Deserialize, Serialize};

/// Versions of the extraction tools, recorded on every [`crate::domain::Derivation`].
///
/// Manually synced with `Cargo.toml` — a wart, but a deliberate one: deriving them at
/// build time needs a build script, and being wrong here only costs an unnecessary
/// re-extraction, never a wrong answer.
const HTMD_VERSION: &str = "0.5.5";
const DOM_SMOOTHIE_VERSION: &str = "0.18.0";
const PDF_INSPECTOR_VERSION: &str = "0.1.7";
const CALAMINE_VERSION: &str = "0.35.0";

/// Readability output shorter than this is treated as a failure to find an article.
///
/// Listing and index pages are real `.gov` content but have no "article" for Readability
/// to find, and it returns near-nothing on them. Falling back keeps those pages.
const MIN_READABLE_CHARS: usize = 200;

/// Text derived from a blob, with the provenance needed to explain it later.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Extraction {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Which pipeline produced this. Distinguishes `dom_smoothie+htmd` from bare `htmd`,
    /// because the two produce very different text from the same bytes.
    pub tool: String,
    pub version: String,
    /// Anything a reader of the output should know — a fallback taken, encoding trouble.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// What extraction produced.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Extracted {
    /// Text was derived.
    Text(Extraction),
    /// Text was derived, but some of the document is images we cannot read.
    Partial {
        #[serde(flatten)]
        extraction: Extraction,
        /// 1-indexed pages that are scans. OCR would need `pdftoppm` + `tesseract`.
        pages_needing_ocr: Vec<u32>,
    },
    /// This format has no text to extract. Recorded rather than silently skipped, so
    /// the corpus can say *"we have this and it is not searchable"* — which is a
    /// different and more honest claim than *"we have nothing"*.
    Unextractable { reason: String },
}

impl Extracted {
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(e) => Some(&e.text),
            Self::Partial { extraction, .. } => Some(&extraction.text),
            Self::Unextractable { .. } => None,
        }
    }

    pub fn tool(&self) -> Option<(&str, &str)> {
        match self {
            Self::Text(e) | Self::Partial { extraction: e, .. } => Some((&e.tool, &e.version)),
            Self::Unextractable { .. } => None,
        }
    }
}

/// Extracts text from bytes, dispatching on the content kind from [`crate::fetch`].
pub fn extract(kind: &str, bytes: &[u8], url: Option<&str>, title: Option<&str>) -> Extracted {
    match kind {
        "html" => extract_html(bytes, url),
        "pdf" => extract_pdf(bytes),
        "spreadsheet" => extract_spreadsheet(bytes),
        "captions" => extract_captions(bytes, title),
        "text" | "csv" | "json" | "xml" => match std::str::from_utf8(bytes) {
            Ok(s) => Extracted::Text(Extraction {
                text: s.to_string(),
                title: None,
                tool: "passthrough".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                notes: vec![],
            }),
            Err(e) => Extracted::Unextractable {
                reason: format!("declared {kind} but not valid UTF-8: {e}"),
            },
        },
        other => Extracted::Unextractable {
            reason: format!("no extractor for content kind `{other}`"),
        },
    }
}

/// A YouTube `json3` caption track, rendered as timestamped markdown.
///
/// Deliberately **not** the passthrough the `json` arm would apply. Indexing the raw
/// document would put `wireMagic`, `acAsrConf` and 4,250 newline markers into the search
/// corpus, and the words a searcher wants would be a minority of the text.
///
/// **The `title` is load-bearing and comes from outside the bytes.** A recording titled
/// *"Mayor Jane Castor 2026 Budget Presentation"* contains the word "Castor" exactly zero
/// times — nobody says the mayor's surname aloud. Without the title in the text, the most
/// identifying fact about a meeting is absent from the index and the obvious query finds
/// nothing. As an `# H1` it becomes the chunker's heading path, so **every** chunk of the
/// meeting carries it.
fn extract_captions(bytes: &[u8], title: Option<&str>) -> Extracted {
    match crate::captions::parse_json3(bytes) {
        Ok(caps) => Extracted::Text(Extraction {
            text: caps.to_markdown(title),
            title: title.map(str::to_string),
            // Named for what produced the *captions*, not for what parsed them: these are
            // YouTube's ASR, and a reader of the provenance needs to know the words were
            // never transcribed locally. `whisper.cpp` on the same audio is a different
            // tool at a different quality, and the two must not look alike in the log.
            tool: "youtube-asr-json3".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            notes: vec![format!(
                "{} cues from {} events; {} carried no text",
                caps.cues.len(),
                caps.events,
                caps.empty_events
            )],
        }),
        Err(e) => Extracted::Unextractable {
            reason: format!("caption track could not be parsed: {e}"),
        },
    }
}

/// `dom_smoothie` for the article, `htmd` for the markdown, bare `htmd` as a fallback.
fn extract_html(bytes: &[u8], url: Option<&str>) -> Extracted {
    let html = String::from_utf8_lossy(bytes);

    // Skipping these matters: htmd otherwise serialises inline JSON-LD and
    // drupalSettings into the markdown, tripling the output with machine noise.
    let converter = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "noscript", "svg", "form"])
        .build();

    let mut notes = Vec::new();

    if let Ok(mut readability) = dom_smoothie::Readability::new(html.as_ref(), url, None) {
        if let Ok(article) = readability.parse() {
            let inner = article.content.to_string();
            if let Ok(md) = converter.convert(&inner) {
                let md = md.trim().to_string();
                if md.chars().count() >= MIN_READABLE_CHARS {
                    return Extracted::Text(Extraction {
                        text: md,
                        title: Some(article.title.to_string()).filter(|t| !t.is_empty()),
                        tool: "dom_smoothie+htmd".into(),
                        version: format!("{DOM_SMOOTHIE_VERSION}+{HTMD_VERSION}"),
                        notes,
                    });
                }
                notes.push(format!(
                    "readability found only {} chars; kept the full page instead",
                    md.chars().count()
                ));
            }
        } else {
            notes.push("readability could not parse this page".into());
        }
    }

    // Fallback: the whole page, minus scripts. Worse for search, but a listing page
    // with no article is still content worth having.
    match converter.convert(&html) {
        Ok(md) => Extracted::Text(Extraction {
            text: md.trim().to_string(),
            title: None,
            tool: "htmd".into(),
            version: HTMD_VERSION.into(),
            notes,
        }),
        Err(e) => Extracted::Unextractable {
            reason: format!("html conversion failed: {e}"),
        },
    }
}

fn extract_pdf(bytes: &[u8]) -> Extracted {
    let result = match pdf_inspector::process_pdf_mem(bytes) {
        Ok(r) => r,
        Err(e) => {
            return Extracted::Unextractable {
                reason: format!("pdf parse failed: {e}"),
            };
        }
    };

    let mut notes = Vec::new();
    if result.has_encoding_issues {
        // The library's own guidance is to fall back to OCR here; we record it so the
        // page can be re-derived once OCR exists rather than quietly trusting garbage.
        notes.push("broken font encodings detected — text may be garbled".into());
    }
    notes.push(format!("{} pages", result.page_count));

    let text = result.markdown.unwrap_or_default().trim().to_string();

    let extraction = Extraction {
        text,
        title: result.title.filter(|t| !t.trim().is_empty()),
        tool: "pdf-inspector".into(),
        version: PDF_INSPECTOR_VERSION.into(),
        notes,
    };

    if result.pages_needing_ocr.is_empty() {
        if extraction.text.is_empty() {
            return Extracted::Unextractable {
                reason: "pdf produced no text and no pages were flagged for OCR".into(),
            };
        }
        Extracted::Text(extraction)
    } else {
        // Per-page routing rather than all-or-nothing: a 300-page budget PDF with two
        // scanned exhibits still yields 298 pages of searchable text.
        Extracted::Partial {
            extraction,
            pages_needing_ocr: result.pages_needing_ocr,
        }
    }
}

/// Spreadsheets as tab-separated rows under per-sheet headings.
///
/// Deliberately not markdown tables: `.gov` budget sheets are routinely 40 columns wide,
/// and a markdown table that wide is unreadable to a person and useless to a chunker.
/// **What a "chunk" of a spreadsheet should be is still an open question** — this makes
/// the content searchable without pretending to have answered it.
fn extract_spreadsheet(bytes: &[u8]) -> Extracted {
    use calamine::Reader;

    let mut workbook = match calamine::open_workbook_auto_from_rs(Cursor::new(bytes.to_vec())) {
        Ok(w) => w,
        Err(e) => {
            return Extracted::Unextractable {
                reason: format!("spreadsheet parse failed: {e}"),
            };
        }
    };

    let mut out = String::new();
    let mut sheets = 0usize;
    for name in workbook.sheet_names().to_owned() {
        let Ok(range) = workbook.worksheet_range(&name) else {
            continue;
        };
        sheets += 1;
        out.push_str(&format!("## {name}\n\n"));
        for row in range.rows() {
            let cells: Vec<String> = row.iter().map(|c| c.to_string()).collect();
            if cells.iter().all(|c| c.trim().is_empty()) {
                continue;
            }
            out.push_str(&cells.join("\t"));
            out.push('\n');
        }
        out.push('\n');
    }

    if out.trim().is_empty() {
        return Extracted::Unextractable {
            reason: "spreadsheet contained no readable cells".into(),
        };
    }

    Extracted::Text(Extraction {
        text: out.trim().to_string(),
        title: None,
        tool: "calamine".into(),
        version: CALAMINE_VERSION.into(),
        notes: vec![format!("{sheets} sheets")],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page shaped like Tampa's: heavy chrome wrapped around a small article.
    const DRUPAL_ISH: &str = r#"<!DOCTYPE html><html><head>
        <title>Fashion Week Tampa Bay | City of Tampa</title>
        <script type="application/ld+json">{"@context":"https://schema.org"}</script>
        <style>.nav{color:red}</style>
        </head><body>
        <nav><a href="/accessibility">Accessibility</a><a href="/news">Newsroom</a>
          <ul><li>English</li><li>Spanish</li><li>German</li><li>Portuguese</li></ul></nav>
        <main><article>
          <h1>Fashion Week Tampa Bay</h1>
          <p>The City of Tampa proclaims that Fashion Week Tampa Bay shall be observed
             throughout the city, recognising the designers and businesses who have made
             the region a centre for creative industry and economic development.</p>
          <p>This proclamation was entered into the record on September 20, 2017 by the
             office of the Mayor, and copies were distributed to the participating
             organisations and to the members of City Council.</p>
        </article></main>
        <footer>Was this page helpful? Do not submit personal information.</footer>
        </body></html>"#;

    #[test]
    fn html_extraction_drops_chrome_and_keeps_the_article() {
        let out = extract(
            "html",
            DRUPAL_ISH.as_bytes(),
            Some("https://www.tampa.gov/x"),
            None,
        );
        let text = out.text().expect("should extract");

        assert!(text.contains("Fashion Week Tampa Bay"));
        assert!(text.contains("creative industry"));

        // The measured failure mode: chrome identical across every page.
        assert!(!text.contains("Newsroom"), "nav leaked into the text");
        assert!(!text.contains("Portuguese"), "language picker leaked");
        assert!(!text.contains("schema.org"), "JSON-LD leaked");
        assert!(!text.contains("color:red"), "CSS leaked");

        let (tool, _) = out.tool().unwrap();
        assert_eq!(tool, "dom_smoothie+htmd");
    }

    #[test]
    fn a_page_with_no_article_falls_back_rather_than_being_lost() {
        // A listing page: all links, no prose. Readability finds nothing to keep.
        let listing = r#"<html><body><h1>Documents</h1>
            <ul><li><a href="/a.pdf">Budget A</a></li><li><a href="/b.pdf">Budget B</a></li></ul>
            </body></html>"#;
        let out = extract("html", listing.as_bytes(), None, None);
        let text = out.text().expect("must not be dropped");

        assert!(text.contains("Budget A"), "listing content was lost");
        let (tool, _) = out.tool().unwrap();
        assert_eq!(tool, "htmd", "should have fallen back");
    }

    #[test]
    fn scripts_and_styles_never_reach_the_text_even_on_the_fallback_path() {
        let noisy = r#"<html><body><script>var drupalSettings={"a":1}</script>
            <style>body{margin:0}</style><p>Short.</p></body></html>"#;
        let text = extract("html", noisy.as_bytes(), None, None)
            .text()
            .unwrap()
            .to_string();
        assert!(!text.contains("drupalSettings"));
        assert!(!text.contains("margin"));
    }

    #[test]
    fn unknown_formats_are_recorded_not_guessed() {
        // .dwg and .dgn CAD files really are in the Hillsborough County corpus.
        let out = extract("other", b"\x00\x01AC1027", None, None);
        assert!(matches!(out, Extracted::Unextractable { .. }));
        assert!(out.text().is_none());
    }

    #[test]
    fn a_corrupt_pdf_is_unextractable_rather_than_a_panic() {
        let out = extract("pdf", b"%PDF-1.7\nnot really a pdf", None, None);
        assert!(matches!(out, Extracted::Unextractable { .. }));
    }

    #[test]
    fn plain_text_passes_through() {
        let out = extract("text", b"just some text", None, None);
        assert_eq!(out.text(), Some("just some text"));
    }

    #[test]
    fn invalid_utf8_claiming_to_be_text_is_rejected() {
        let out = extract("text", &[0xff, 0xfe, 0x00], None, None);
        assert!(matches!(out, Extracted::Unextractable { .. }));
    }

    fn caption_track() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "wireMagic": "pb3",
            "events": [
                { "tStartMs": 1000, "dDurationMs": 4000, "segs": [
                    {"utf8": "I am proud to present the city's FY 2026 budget."}
                ]}
            ]
        }))
        .unwrap()
    }

    /// Measured on a real recording titled *"Mayor Jane Castor 2026 Budget Presentation"*:
    /// the surname "Castor" appears **zero** times in three hours of speech, because
    /// nobody says the mayor's name aloud. The title is the most identifying fact about
    /// the document and it exists only outside the bytes, so extraction has to be handed
    /// it or the obvious query finds nothing.
    #[test]
    fn a_caption_track_carries_the_title_that_is_never_spoken() {
        let out = extract(
            "captions",
            &caption_track(),
            None,
            Some("Mayor Jane Castor 2026 Budget Presentation"),
        );
        let text = out.text().expect("captions should extract");

        assert!(
            text.starts_with("# Mayor Jane Castor 2026 Budget Presentation\n"),
            "the title must lead, so the chunker adopts it as the heading path: {text}"
        );
        assert!(text.contains("[00:00:01] I am proud to present"));
        assert!(
            !text.contains("wireMagic"),
            "the raw document must not reach the index"
        );
    }

    #[test]
    fn a_caption_track_without_a_title_still_extracts() {
        let out = extract("captions", &caption_track(), None, None);
        let text = out.text().expect("captions should extract");
        assert!(!text.starts_with('#'));
        assert!(text.contains("[00:00:01] I am proud to present"));
    }

    /// A caption track is `application/json`, and so is a video's metadata document. The
    /// passthrough would index `wireMagic` and 4,250 newline markers as if they were
    /// speech, so the sniff — not the declared type — has to pick the extractor.
    #[test]
    fn a_caption_track_is_not_treated_as_plain_json() {
        use std::collections::BTreeMap;
        let mut meta = BTreeMap::new();
        meta.insert("content-type".to_string(), "application/json".to_string());
        assert_eq!(
            crate::fetch::content_kind(&meta, &caption_track()),
            "captions"
        );
        assert_eq!(
            crate::fetch::content_kind(&meta, br#"{"id":"abc","title":"a video"}"#),
            "json"
        );
    }
}
