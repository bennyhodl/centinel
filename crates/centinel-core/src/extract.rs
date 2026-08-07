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

use crate::content::ContentKind;

/// Versions of the extraction tools, recorded on every [`crate::domain::Derivation`].
///
/// Manually synced with `Cargo.toml` — a wart, but a deliberate one: deriving them at
/// build time needs a build script, and being wrong here only costs an unnecessary
/// re-extraction, never a wrong answer.
const HTMD_VERSION: &str = "0.5.5";
const DOM_SMOOTHIE_VERSION: &str = "0.18.0";
const PDF_INSPECTOR_VERSION: &str = "0.1.7";
const CALAMINE_VERSION: &str = "0.36.1";
const ANYDOC_VERSION: &str = "0.1.3";

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

/// The extraction pipeline's own name, for recording that it could make nothing of a
/// blob.
///
/// The individual tools name themselves on a successful [`crate::domain::Derivation`];
/// a failure belongs to the dispatcher that chose between them, because the reason is
/// almost always that no tool was chosen at all.
pub const PIPELINE: &str = "centinel-extract";

/// Bumped when the pipeline learns to read a kind it previously could not.
///
/// Recorded on every [`crate::domain::Underivable`], and therefore the switch that makes
/// a corpus re-attempt blobs an earlier version gave up on. Deliberately not the crate
/// version: a patch release that changes nothing about extraction should not re-read
/// every audio file in the archive.
///
/// `2` — `document` and `zip-container` became readable, so every Word file, deck and
/// e-book an earlier run wrote off as unreadable is worth another attempt.
///
/// `3` — a PDF that `pdf-inspector` makes nothing of is now offered to `pdftotext`, which
/// reads a text layer the first reader misses on a third of them.
pub const PIPELINE_VERSION: &str = "3";

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

    /// Records what the readers before this one came up against.
    ///
    /// A verdict carries its own reason and takes none of these: they are the story of how
    /// a *successful* extraction was reached.
    fn note_all(&mut self, notes: &[String]) {
        if let Self::Text(e) | Self::Partial { extraction: e, .. } = self {
            // Ahead of the reader's own notes: they are what happened first.
            let mut all = notes.to_vec();
            all.append(&mut e.notes);
            e.notes = all;
        }
    }
}

/// The blob a reader is given.
///
/// Both forms, because a reader is either an in-process parser or a child process, and
/// which one a kind needs is the reader's business rather than the caller's. The caller
/// has the blob at its content address either way.
pub struct Blob<'a> {
    pub bytes: &'a [u8],
    pub path: &'a std::path::Path,
    pub url: Option<&'a str>,
    pub title: Option<&'a str>,
}

/// One tool that can turn a blob into text.
///
/// A **primary and fallback reader** used to be three different things in this file: a
/// `bool` for PDF, a free-text note for HTML, and a re-route for documents. So a kind that
/// wanted two readers had to invent a fourth mechanism, and — worse — each pair decided
/// for itself what "produced nothing" meant. That decision is exactly the one the PDF pair
/// got wrong: `derive` returned before the fallback whenever the primary said
/// `Unextractable`, which is precisely what `extract_pdf` emits for a PDF whose text layer
/// `pdf-inspector` cannot see. Poppler reads a third of those.
///
/// Now the order is data, [`produced_text`] is written once, and the record names whoever
/// spoke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reader {
    /// `dom_smoothie` for the article, `htmd` for the markdown.
    Readability,
    /// The whole page minus scripts. Worse for search, but a listing page with no article
    /// is still content worth having.
    WholePage,
    PdfInspector,
    /// A child process. Reads a text layer `pdf-inspector` misses on a third of the PDFs
    /// it makes nothing of.
    Poppler,
    AnyDoc,
    Spreadsheet,
    Captions,
    /// The bytes, as text. For the kinds that are already text.
    Passthrough,
}

impl Reader {
    /// The name that reaches the record when this reader is the one that spoke.
    pub fn name(self) -> &'static str {
        match self {
            Self::Readability => "dom_smoothie+htmd",
            Self::WholePage => "htmd",
            Self::PdfInspector => "pdf-inspector",
            Self::Poppler => PDFTOTEXT,
            Self::AnyDoc => "anydoc",
            Self::Spreadsheet => "calamine",
            Self::Captions => "youtube-asr-json3",
            Self::Passthrough => "passthrough",
        }
    }

    /// Whether running this one means starting a child process.
    ///
    /// [`extract`] is synchronous and answers for the in-process pipeline alone; only
    /// [`derive`] can reach the rest.
    fn spawns(self) -> bool {
        matches!(self, Self::Poppler)
    }

    fn read_in_process(self, blob: &Blob<'_>) -> Extracted {
        match self {
            Self::Readability => html_readability(blob.bytes, blob.url),
            Self::WholePage => html_whole_page(blob.bytes),
            Self::PdfInspector => extract_pdf(blob.bytes),
            Self::AnyDoc => extract_document(blob.bytes),
            Self::Spreadsheet => extract_spreadsheet(blob.bytes),
            Self::Captions => extract_captions(blob.bytes, blob.title),
            Self::Passthrough => passthrough(blob.bytes),
            Self::Poppler => unreachable!("a spawning reader is never read in process"),
        }
    }

    async fn read(self, blob: &Blob<'_>) -> Extracted {
        match self {
            Self::Poppler => match pdf_text_via_poppler(blob.path).await {
                Some(extraction) => Extracted::Text(extraction),
                None => Extracted::Unextractable {
                    reason: "poppler found no text either".into(),
                },
            },
            reader => reader.read_in_process(blob),
        }
    }
}

/// The readers for a kind, in the order they are tried.
///
/// Adding a kind means adding a row here; adding a second reader to an existing kind means
/// adding an element. Neither needs a new field, a new flag, or a new idea about what
/// failure means.
pub fn readers_for(kind: ContentKind) -> &'static [Reader] {
    use ContentKind::*;
    match kind {
        Html => &[Reader::Readability, Reader::WholePage],
        Pdf => &[Reader::PdfInspector, Reader::Poppler],
        Spreadsheet => &[Reader::Spreadsheet],
        // Two kinds, one reader. `document` is what the `content-type` declared;
        // `zip-container` is what the magic bytes could tell on their own, which for a
        // `.docx` served as `application/octet-stream` is only "this is a zip". Both reach
        // the same question, and `anydoc` is where it gets answered.
        Document | ZipContainer => &[Reader::AnyDoc],
        Captions => &[Reader::Captions],
        Text | Csv | Json | Xml => &[Reader::Passthrough],
        // Nothing to read. `audio` goes to `transcribe`; `markdown` is already derived
        // text; `other` is bytes nothing here claims.
        Markdown | Audio | Other => &[],
    }
}

/// Whether a reader actually produced something to keep.
///
/// **The one definition**, which is the point. Written per pair, it can be wrong per pair,
/// and it was: the PDF pair's version returned early on `Unextractable` — the very verdict
/// its fallback exists to answer — so `pdftotext` was unreachable for the 168 PDFs of 490
/// that have a text layer `pdf-inspector` cannot see.
fn produced_text(outcome: &Extracted) -> bool {
    outcome.text().is_some_and(|t| !t.trim().is_empty())
}

/// An extraction and how it was reached.
///
/// [`derive`]'s answer, kept apart from [`Extracted`] because "the fallback spoke" is a
/// fact about the *run* rather than about the text — it belongs in a report, and never on
/// the [`crate::domain::Derivation`], which already names whichever tool produced the
/// bytes it carries.
#[derive(Clone, Debug)]
pub struct Derived {
    pub outcome: Extracted,
    /// A reader after the first is the one that spoke. The measure of how much the primary
    /// is missing, and the number to watch after any change to it.
    ///
    /// True for every kind with a fallback, not just PDF — which is what makes the HTML
    /// pair's rate visible at all. It used to be readable only by counting `by_tool`.
    pub recovered_by_fallback: bool,
}

/// Extracts text from bytes, using every reader for the kind that runs in process.
///
/// Synchronous, and therefore blind to any reader that spawns a child. That is the whole
/// difference between this and [`derive`]: callers that only have bytes get the answer the
/// in-process pipeline can give.
pub fn extract(
    kind: ContentKind,
    bytes: &[u8],
    url: Option<&str>,
    title: Option<&str>,
) -> Extracted {
    let blob = Blob {
        bytes,
        path: std::path::Path::new(""),
        url,
        title,
    };
    let mut carried: Vec<(Reader, String)> = Vec::new();
    for reader in readers_for(kind).iter().copied().filter(|r| !r.spawns()) {
        let mut outcome = reader.read_in_process(&blob);
        if produced_text(&outcome) {
            outcome.note_all(&notes_of(&carried));
            return outcome;
        }
        carried.push((reader, no_text_reason(&outcome)));
    }
    Extracted::Unextractable {
        reason: give_up_reason(kind, &carried),
    }
}

/// The extraction the **record** takes, as against the one a reader produced.
///
/// [`extract`] answers "what did the in-process readers make of these bytes". This answers
/// the question the log asks, and the two differ in exactly one place: a reader that parsed
/// cleanly and came back with nothing has produced a *verdict*, not a derivation. Every
/// reader for the kind is tried, including the ones that spawn, and anything still empty
/// becomes an [`Extracted::Unextractable`] so the answer lands where a pipeline-version
/// bump can revisit it. A `Derivation` always has bytes.
///
/// *Why it is here and not in the op that writes the record:* it was in the op, which
/// meant it was reachable only by a corpus-wide `extract`. Anything else deriving one
/// document — `check` — would have re-implemented it, and a QA tool that runs a different
/// extractor from the pipeline it is checking answers a question nobody asked.
pub async fn derive(
    kind: ContentKind,
    bytes: &[u8],
    path: &std::path::Path,
    url: Option<&str>,
    title: Option<&str>,
) -> Derived {
    let blob = Blob {
        bytes,
        path,
        url,
        title,
    };
    let mut carried: Vec<(Reader, String)> = Vec::new();

    for (i, reader) in readers_for(kind).iter().copied().enumerate() {
        let mut outcome = reader.read(&blob).await;
        if produced_text(&outcome) {
            // What the readers before it came up against. On HTML this is the note the
            // old code wrote by hand — "readability found only 90 chars" — and on PDF it
            // is the page count that used to be lost on the way to the fallback.
            outcome.note_all(&notes_of(&carried));
            return Derived {
                outcome,
                recovered_by_fallback: i > 0,
            };
        }
        carried.push((reader, no_text_reason(&outcome)));
    }

    Derived {
        outcome: Extracted::Unextractable {
            reason: give_up_reason(kind, &carried),
        },
        recovered_by_fallback: false,
    }
}

/// What the readers that came up short had to say, each named.
///
/// Notes go on a *successful* extraction, so the name is the point: the text came from the
/// second reader and this is why the first did not give it.
fn notes_of(carried: &[(Reader, String)]) -> Vec<String> {
    carried
        .iter()
        .map(|(reader, reason)| format!("{}: {reason}", reader.name()))
        .collect()
}

/// Why nothing was derived, after every reader for the kind had a turn.
fn give_up_reason(kind: ContentKind, carried: &[(Reader, String)]) -> String {
    match carried {
        // No reader was even tried, which is a fact about the kind rather than the bytes.
        [] => format!("no reader for content kind `{kind}`"),
        // One reader, one story. Naming it would only repeat what `PIPELINE` already
        // records, and this keeps the wording a verdict had before there was a list.
        [(_, reason)] => reason.clone(),
        many => notes_of(many).join("; "),
    }
}

/// The bytes, as text, for the kinds that already are text.
fn passthrough(bytes: &[u8]) -> Extracted {
    match std::str::from_utf8(bytes) {
        Ok(s) => Extracted::Text(Extraction {
            text: s.to_string(),
            title: None,
            tool: Reader::Passthrough.name().into(),
            version: env!("CARGO_PKG_VERSION").into(),
            notes: vec![],
        }),
        Err(e) => Extracted::Unextractable {
            reason: format!("declared text but not valid UTF-8: {e}"),
        },
    }
}

/// Why an extraction that parsed cleanly still yielded nothing.
///
/// Carries the OCR page count into the [`crate::domain::Underivable`]'s reason, because
/// that is now the only record of it: the verdict replaces the `Partial` that used to hold
/// the list, and a future OCR pipeline should be able to see from the log which blobs are
/// waiting for it.
fn no_text_reason(outcome: &Extracted) -> String {
    match outcome {
        Extracted::Partial {
            pages_needing_ocr, ..
        } => format!(
            "parsed but holds no readable text; {} page{} {} images no reader here can read",
            pages_needing_ocr.len(),
            if pages_needing_ocr.len() == 1 {
                ""
            } else {
                "s"
            },
            if pages_needing_ocr.len() == 1 {
                "is"
            } else {
                "are"
            },
        ),
        // A reader that already said why keeps its own words.
        Extracted::Unextractable { reason } => reason.clone(),
        Extracted::Text(_) => "parsed but holds no text".into(),
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

/// Skipping these matters: htmd otherwise serialises inline JSON-LD and drupalSettings
/// into the markdown, tripling the output with machine noise.
fn markdown_converter() -> htmd::HtmlToMarkdown {
    htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "noscript", "svg", "form"])
        .build()
}

/// `dom_smoothie` for the article, `htmd` for the markdown.
///
/// "Found an article too short to be one" is a refusal here rather than a note, because
/// that is how [`derive`] learns to try the next reader. `MIN_READABLE_CHARS` of output is
/// the line, and the count travels in the reason so it reaches the record either way.
fn html_readability(bytes: &[u8], url: Option<&str>) -> Extracted {
    let html = String::from_utf8_lossy(bytes);

    let Ok(mut readability) = dom_smoothie::Readability::new(html.as_ref(), url, None) else {
        return Extracted::Unextractable {
            reason: "readability could not read this page".into(),
        };
    };
    let Ok(article) = readability.parse() else {
        return Extracted::Unextractable {
            reason: "readability could not parse this page".into(),
        };
    };
    let md = match markdown_converter().convert(article.content.as_ref()) {
        Ok(md) => md.trim().to_string(),
        Err(e) => {
            return Extracted::Unextractable {
                reason: format!("html conversion failed: {e}"),
            };
        }
    };
    if md.chars().count() < MIN_READABLE_CHARS {
        return Extracted::Unextractable {
            reason: format!(
                "readability found only {} chars; kept the full page instead",
                md.chars().count()
            ),
        };
    }

    let title = Some(article.title.to_string())
        .filter(|t| !t.is_empty())
        .or_else(|| html_title(&html));
    Extracted::Text(Extraction {
        text: with_title(title.as_deref(), &md),
        title,
        tool: Reader::Readability.name().into(),
        version: format!("{DOM_SMOOTHIE_VERSION}+{HTMD_VERSION}"),
        notes: vec![],
    })
}

/// The whole page, minus scripts. Worse for search, but a listing page with no article is
/// still content worth having.
fn html_whole_page(bytes: &[u8]) -> Extracted {
    let html = String::from_utf8_lossy(bytes);
    match markdown_converter().convert(&html) {
        Ok(md) => {
            let title = html_title(&html);
            Extracted::Text(Extraction {
                text: with_title(title.as_deref(), md.trim()),
                title,
                tool: Reader::WholePage.name().into(),
                version: HTMD_VERSION.into(),
                notes: vec![],
            })
        }
        Err(e) => Extracted::Unextractable {
            reason: format!("html conversion failed: {e}"),
        },
    }
}

/// Puts the document title into the text, as an `# H1`, unless it is already the first
/// heading.
///
/// The reasoning is [`extract_captions`]', and HTML makes the case sharper. A `.gov` CMS
/// puts the subject of a page in `<title>`, `og:title` and `<h1>` and **nowhere in the
/// body**, so what Readability hands back is an article that never names itself. Nine
/// hundred Tampa proclamation pages extracted to a date and a print notice: collected,
/// indexed, and unreachable by the one query anybody would type, because the words
/// *Irish American Heritage Month* were in none of them.
///
/// As an `# H1` it becomes the chunker's heading path, so every chunk of the document
/// carries it — which is worth more than the title field, since only the text is searched.
fn with_title(title: Option<&str>, body: &str) -> String {
    let Some(title) = title.map(str::trim).filter(|t| !t.is_empty()) else {
        return body.to_string();
    };

    let already_leads = body
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|first| first.trim_start().trim_start_matches('#').trim())
        .is_some_and(|heading| heading.eq_ignore_ascii_case(title));

    match already_leads {
        true => body.to_string(),
        false => format!("# {title}\n\n{body}"),
    }
}

/// The page's own title, for when Readability could not name it.
///
/// `og:title` first, because a `<title>` is usually the page name plus the site name and
/// only the first half is the document. Readability strips that suffix itself when it can
/// match the `<h1>`; this runs where it could not, so it prefers the tag that never
/// carries the suffix over guessing at a separator.
fn html_title(html: &str) -> Option<String> {
    crate::html::Scan::new(html).title()
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

/// The name `pdftotext` records itself under. Poppler's version is asked for at runtime.
pub const PDFTOTEXT: &str = "pdftotext";

/// How long one PDF gets. A deadline, not a stall timeout: this call has a known shape,
/// and the largest document in a real `.gov` corpus is under a hundred megabytes.
const PDFTOTEXT_DEADLINE: std::time::Duration = std::time::Duration::from_secs(120);

/// A second reader for a PDF the first one made nothing of.
///
/// `pdf-inspector` returns no text, and flags every page for OCR, on documents that
/// **do** carry a text layer: measured on the tampa corpus, 168 of the 490 PDFs it
/// emptied are read by `pdftotext` — an executive order, signed board minutes, a
/// 315,000-character area action plan. Flagging a page for OCR is a claim about what the
/// reader could decode, not about what the page holds, and the two are not the same.
///
/// Only ever a *fallback*. `pdf-inspector` stays the primary because it produces markdown
/// with headings, and headings are the chunk heading path; `pdftotext` produces flat text.
/// A worse shape beats no text at all, and nothing else beats a better shape.
///
/// Takes a **path** rather than bytes because the caller already has the blob on disk at
/// its content address, and handing a child process a path it can read is cheaper and
/// simpler than a temporary file. `None` when poppler is absent, when it fails, or when it
/// agrees there is nothing to read — all three leave the caller's verdict unchanged.
pub async fn pdf_text_via_poppler(path: &std::path::Path) -> Option<Extraction> {
    let out = crate::tool::Tool::new(PDFTOTEXT)
        // `-q` so poppler's warnings about malformed xref tables stay out of the text;
        // `-enc UTF-8` because the default is locale-dependent and this is an archive.
        .args([
            std::ffi::OsStr::new("-q"),
            std::ffi::OsStr::new("-enc"),
            std::ffi::OsStr::new("UTF-8"),
            path.as_os_str(),
            std::ffi::OsStr::new("-"),
        ])
        .timeout(PDFTOTEXT_DEADLINE)
        .success()
        .await;

    let bytes = match out {
        Ok(bytes) => bytes,
        // Every failure here is the same decision for the caller — there is no text to be
        // had from this reader — but they are very different facts, so they are logged
        // apart. A missing binary is evidence about this machine; `doctor` reports it.
        Err(e) => {
            tracing::debug!(error = %e, path = %path.display(), "pdftotext produced nothing");
            return None;
        }
    };

    let text = String::from_utf8_lossy(&bytes).trim().to_string();
    if text.is_empty() {
        return None;
    }

    Some(Extraction {
        text,
        // Poppler carries no document title through `pdftotext`, and inventing one from
        // the filename would put a guess where the record expects the document's own name.
        title: None,
        tool: PDFTOTEXT.into(),
        version: poppler_version().await.unwrap_or_else(|| "unknown".into()),
        // Why the primary came up short is `derive`'s to record, and it says so in the
        // primary's own words rather than in a sentence written here about it.
        notes: vec![],
    })
}

/// Poppler's version, for the Derivation's record. Cached: it is one more child process
/// per PDF otherwise, and the answer cannot change inside a run.
async fn poppler_version() -> Option<String> {
    static VERSION: tokio::sync::OnceCell<Option<String>> = tokio::sync::OnceCell::const_new();
    VERSION
        .get_or_init(|| async {
            // `pdftotext -v` writes its banner to stderr and exits non-zero on some
            // builds, so the output is taken as data rather than gated on success.
            let out = crate::tool::Tool::new(PDFTOTEXT)
                .arg("-v")
                .timeout(std::time::Duration::from_secs(10))
                .output()
                .await
                .ok()?;
            let banner = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stderr),
                String::from_utf8_lossy(&out.stdout)
            );
            banner
                .split_whitespace()
                .skip_while(|w| !w.eq_ignore_ascii_case("version"))
                .nth(1)
                .map(str::to_string)
        })
        .await
        .clone()
}

/// Word files, decks, OpenDocument, RTF and EPUB, through `anydoc`.
///
/// One arm for eight formats because they are one pipeline: every parser produces the
/// same document model and one serializer renders it, so a `.docx` and a `.pptx` are not
/// two tools in the record's sense — they are one tool given different bytes. The format
/// goes in the notes, which is where a reader who needs it can find it.
///
/// **The format is decided here rather than by [`ContentKind::classify`], and it has
/// to be.** Telling a `.docx` from a `.pptx` means reading the ZIP central directory,
/// which sits at the *end* of the file; classification holds a 4 KB head and cannot see
/// it. Extraction holds the whole verified blob, so this is the first point at which the
/// question can be answered honestly rather than guessed from a URL.
fn extract_document(bytes: &[u8]) -> Extracted {
    let Some(format) = anydoc::Format::from_bytes(bytes) else {
        // A `.zip` of photographs is a real thing to find on a `.gov` server, and it is
        // not a failure. Saying so is what stops the next run reading it again.
        return Extracted::Unextractable {
            reason: "container holds no document format we can read".into(),
        };
    };

    match format {
        // `anydoc` reads these through `calamine` and renders markdown tables.
        // `extract_spreadsheet` is the same reader with the output shape this corpus
        // needs, so the route back is deliberate — see its own note on 40-column sheets.
        anydoc::Format::Excel | anydoc::Format::Ods => return extract_spreadsheet(bytes),
        // The dispatcher sends PDFs to `extract_pdf` and never here. Guarded rather than
        // assumed, because `anydoc`'s PDF path collapses `pdf-inspector`'s result to a
        // `String`: a PDF arriving through this arm would lose its per-page OCR routing,
        // and lose it silently.
        anydoc::Format::Pdf => return extract_pdf(bytes),
        _ => {}
    }

    match anydoc::to_markdown_bytes(bytes, format) {
        Ok(md) => {
            let text = md.trim().to_string();
            if text.is_empty() {
                // An empty deck parses perfectly and holds no words. That is a fact about
                // the document, not a fault in the reader, and it belongs in the record.
                return Extracted::Unextractable {
                    reason: format!("{format:?} parsed but holds no text"),
                };
            }
            Extracted::Text(Extraction {
                text,
                // `anydoc` renders the document's own title into the body when it has
                // one; there is no separate title on the markdown it returns.
                title: None,
                tool: "anydoc".into(),
                version: ANYDOC_VERSION.into(),
                notes: vec![format!("{format:?} detected from content")],
            })
        }
        Err(e) => Extracted::Unextractable {
            reason: format!("{format:?} extraction failed: {e}"),
        },
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
            ContentKind::Html,
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
        let out = extract(ContentKind::Html, listing.as_bytes(), None, None);
        let text = out.text().expect("must not be dropped");

        assert!(text.contains("Budget A"), "listing content was lost");
        let (tool, _) = out.tool().unwrap();
        assert_eq!(tool, "htmd", "should have fallen back");
    }

    #[test]
    fn scripts_and_styles_never_reach_the_text_even_on_the_fallback_path() {
        let noisy = r#"<html><body><script>var drupalSettings={"a":1}</script>
            <style>body{margin:0}</style><p>Short.</p></body></html>"#;
        let text = extract(ContentKind::Html, noisy.as_bytes(), None, None)
            .text()
            .unwrap()
            .to_string();
        assert!(!text.contains("drupalSettings"));
        assert!(!text.contains("margin"));
    }

    /// A real `tampa.gov` proclamation, reduced to the shape that made 918 of them
    /// unfindable: the subject is in `<title>`, `og:title` and `<h1>`, and the body that
    /// Readability keeps is a date and the notice shown when the page is printed.
    const PROCLAMATION: &str = r#"<html><head>
        <title>Irish American Heritage Month | City of Tampa</title>
        <meta property="og:title" content="Irish American Heritage Month" />
        </head><body>
        <h1>Irish American Heritage Month</h1>
        <div class="field__item"><div class="field__label">Date Added</div>
        <time datetime="2022-03-01T00:00:00Z">Tuesday, March 1, 2022</time></div>
        <div class="pdf-reader"><div class="d-none d-print-block">
        <h2>Use the print buttons in the Preview</h2>
        <p>To properly print this document, hover your mouse over the document PREVIEW
        area and controls will appear. There you can DOWNLOAD or PRINT this document.</p>
        <p>Was this page helpful? Thanks for letting us know! Help us improve by leaving a
        quick comment. Tell us what was confusing, missing or inaccurate about this page.</p>
        </div></div></body></html>"#;

    /// The one query anybody would type. Before the title was written into the text, the
    /// words were in no chunk of the document and this found nothing.
    #[test]
    fn a_page_whose_subject_is_only_in_its_title_still_carries_it() {
        let out = extract(
            ContentKind::Html,
            PROCLAMATION.as_bytes(),
            Some("https://www.tampa.gov/proclamation/irish-american-heritage-month"),
            None,
        );
        let text = out.text().expect("should extract");

        assert!(
            text.contains("Irish American Heritage Month"),
            "the subject of the page is absent from its own text: {text}"
        );
        assert!(
            text.starts_with("# Irish American Heritage Month"),
            "as an H1, so the chunker puts it in every chunk's heading path: {text}"
        );
    }

    /// Listing pages take the fallback path, and they have titles too.
    #[test]
    fn the_fallback_path_recovers_a_title_as_well() {
        let listing = r#"<html><head><title>Bid Opportunities | City of Tampa</title>
            <meta property="og:title" content="Bid Opportunities" /></head><body>
            <h1>Documents</h1><ul><li><a href="/a.pdf">Budget A</a></li></ul>
            </body></html>"#;
        let out = extract(ContentKind::Html, listing.as_bytes(), None, None);
        assert_eq!(out.tool().unwrap().0, "htmd", "should have fallen back");
        assert!(out.text().unwrap().starts_with("# Bid Opportunities"));
    }

    /// `og:title` first: a `<title>` is usually the page plus the site, and only the first
    /// half is the document. Repeating the site name on every chunk of every page is the
    /// boilerplate problem this change exists to reduce.
    #[test]
    fn og_title_wins_over_the_title_tag_and_its_site_suffix() {
        assert_eq!(
            html_title(
                r#"<title>Fee Schedule | City of Tampa</title>
                   <meta property="og:title" content="Fee Schedule">"#
            ),
            Some("Fee Schedule".into())
        );
        assert_eq!(
            html_title("<title>Fee Schedule | City of Tampa</title>"),
            Some("Fee Schedule | City of Tampa".into()),
            "with no og:title it is taken whole rather than split on a guessed separator"
        );
        assert_eq!(html_title("<html><body>no title</body></html>"), None);
        assert_eq!(html_title("<title>   </title>"), None);
    }

    #[test]
    fn a_title_entity_is_decoded_before_it_becomes_a_heading() {
        assert_eq!(
            html_title(r#"<meta property="og:title" content="Parks &amp; Recreation">"#),
            Some("Parks & Recreation".into())
        );
    }

    #[test]
    fn a_title_already_leading_the_body_is_not_repeated() {
        assert_eq!(
            with_title(Some("Agenda"), "# Agenda\n\nThe body."),
            "# Agenda\n\nThe body.",
            "readability already put it there"
        );
        assert_eq!(
            with_title(Some("Agenda"), "The body."),
            "# Agenda\n\nThe body."
        );
        assert_eq!(with_title(None, "The body."), "The body.");
        assert_eq!(with_title(Some("  "), "The body."), "The body.");
    }

    #[test]
    fn unknown_formats_are_recorded_not_guessed() {
        // .dwg and .dgn CAD files really are in the Hillsborough County corpus.
        let out = extract(ContentKind::Other, b"\x00\x01AC1027", None, None);
        assert!(matches!(out, Extracted::Unextractable { .. }));
        assert!(out.text().is_none());
    }

    /// A zip package, built part by part so the test says what makes it a `.docx`.
    fn zip_of(parts: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;

        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in parts {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn docx(body: &str) -> Vec<u8> {
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
        </Relationships>"#;
        let document = format!(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                 <w:body>{body}</w:body>
               </w:document>"#
        );
        zip_of(&[
            ("_rels/.rels", rels),
            ("word/document.xml", document.as_bytes()),
        ])
    }

    fn paragraph(text: &str) -> String {
        format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
    }

    /// The formats this pipeline could not read at all until anydoc landed. RTF is the
    /// one that needs no container, so the assertion sits beside its own input.
    #[test]
    fn an_rtf_document_extracts_and_says_which_tool_read_it() {
        let rtf = br"{\rtf1\ansi\deff0 {\fonttbl{\f0 Times;}}
            \b Ordinance 2026-114\b0\par
            The City Council hereby amends the zoning code.\par}";

        let out = extract(ContentKind::Document, rtf, None, None);
        let text = out.text().expect("rtf should extract");

        assert!(text.contains("Ordinance 2026-114"), "{text}");
        assert!(text.contains("amends the zoning code"), "{text}");

        let (tool, version) = out.tool().unwrap();
        assert_eq!(tool, "anydoc");
        assert_eq!(version, ANYDOC_VERSION);
    }

    /// The measured shape of the gap: `.gov` servers serve `.docx` as
    /// `application/octet-stream`, so the header says nothing and the magic bytes say
    /// only "a zip". Both used to end as `Unextractable`.
    #[test]
    fn a_document_served_as_a_bare_zip_still_extracts() {
        use std::collections::BTreeMap;

        let bytes = docx(&paragraph("Notice of public hearing on the FY2027 budget."));

        assert_eq!(
            crate::content::ContentKind::classify(&BTreeMap::new(), &bytes),
            crate::content::ContentKind::ZipContainer,
            "the head cannot tell a .docx from any other zip, and must not pretend to"
        );

        let out = extract(ContentKind::ZipContainer, &bytes, None, None);
        let text = out
            .text()
            .expect("a docx must not be lost for want of a header");
        assert!(text.contains("public hearing"), "{text}");
        assert_eq!(out.tool().unwrap().0, "anydoc");
    }

    /// The routing that keeps one decision from quietly overriding another: anydoc reads
    /// workbooks through calamine and renders markdown tables, and a 40-column budget
    /// sheet must not become one. Arriving as an unlabelled zip changes nothing.
    #[test]
    fn a_workbook_is_routed_back_to_the_spreadsheet_shape() {
        let rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
        </Relationships>"#;
        let workbook =
            br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
                xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
            <sheets><sheet name="General Fund" sheetId="1" r:id="rId1"/></sheets>
        </workbook>"#;
        let wb_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
        </Relationships>"#;
        let sheet =
            br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
            <sheetData>
              <row r="1"><c r="A1" t="inlineStr"><is><t>Department</t></is></c>
                         <c r="B1" t="inlineStr"><is><t>Adopted</t></is></c></row>
            </sheetData>
        </worksheet>"#;
        let bytes = zip_of(&[
            ("_rels/.rels", rels),
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", wb_rels),
            ("xl/worksheets/sheet1.xml", sheet),
        ]);

        let out = extract(ContentKind::ZipContainer, &bytes, None, None);
        let text = out.text().expect("a workbook should extract");

        assert_eq!(
            out.tool().unwrap().0,
            "calamine",
            "workbooks belong to the spreadsheet path, whichever kind they arrived as"
        );
        assert!(
            text.contains("Department\tAdopted"),
            "tab-separated, not a markdown table: {text}"
        );
    }

    /// A zip of photographs is a real thing to find on a `.gov` server. Recording that we
    /// hold it and cannot read it is what stops every later run reading it again.
    #[test]
    fn a_zip_that_holds_no_document_is_recorded_rather_than_retried_forever() {
        let bytes = zip_of(&[("site-plan.dwg", b"\x00\x01AC1027")]);
        let out = extract(ContentKind::ZipContainer, &bytes, None, None);

        assert!(matches!(out, Extracted::Unextractable { .. }));
        assert!(out.text().is_none());
    }

    /// A PDF must never reach the anydoc arm, whatever a server called it: anydoc's PDF
    /// path discards `pages_needing_ocr`, so a mislabelled scan would lose its per-page
    /// routing without saying so.
    #[test]
    fn a_pdf_mislabelled_as_a_document_keeps_the_pdf_path() {
        let out = extract(
            ContentKind::Document,
            b"%PDF-1.7\nnot really a pdf",
            None,
            None,
        );
        match out {
            Extracted::Unextractable { reason } => {
                assert!(reason.starts_with("pdf parse failed"), "{reason}");
            }
            other => panic!("a PDF must be read as a PDF: {other:?}"),
        }
    }

    #[test]
    fn a_corrupt_pdf_is_unextractable_rather_than_a_panic() {
        let out = extract(ContentKind::Pdf, b"%PDF-1.7\nnot really a pdf", None, None);
        assert!(matches!(out, Extracted::Unextractable { .. }));
    }

    #[test]
    fn plain_text_passes_through() {
        let out = extract(ContentKind::Text, b"just some text", None, None);
        assert_eq!(out.text(), Some("just some text"));
    }

    #[test]
    fn invalid_utf8_claiming_to_be_text_is_rejected() {
        let out = extract(ContentKind::Text, &[0xff, 0xfe, 0x00], None, None);
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
            ContentKind::Captions,
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
        let out = extract(ContentKind::Captions, &caption_track(), None, None);
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
            crate::content::ContentKind::classify(&meta, &caption_track()),
            crate::content::ContentKind::Captions
        );
        assert_eq!(
            crate::content::ContentKind::classify(&meta, br#"{"id":"abc","title":"a video"}"#),
            crate::content::ContentKind::Json
        );
    }

    // ── the reader list ───────────────────────────────────────────────────────

    /// The bug this list exists to make impossible.
    ///
    /// `derive` used to return before the fallback whenever the primary said
    /// `Unextractable` — which is precisely what `extract_pdf` says about a PDF whose text
    /// layer `pdf-inspector` cannot see. 168 of 490 real PDFs are that case, and poppler
    /// reads them.
    ///
    /// Asserts that poppler was *asked*, not what it found: whether the binary is
    /// installed is a fact about the machine, and a test that turned on it would fail on
    /// half the boxes that run it. Both readers naming themselves in the verdict is the
    /// invariant either way.
    #[tokio::test]
    async fn a_refusing_primary_does_not_stop_the_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scan.pdf");
        let bytes = b"%PDF-1.7\nnot really a pdf";
        std::fs::write(&path, bytes).unwrap();

        let derived = derive(ContentKind::Pdf, bytes, &path, None, None).await;

        let Extracted::Unextractable { reason } = derived.outcome else {
            panic!("a corrupt pdf cannot yield text");
        };
        assert!(
            reason.contains("pdf-inspector"),
            "the primary is not named: {reason}"
        );
        assert!(
            reason.contains(PDFTOTEXT),
            "the fallback was never reached: {reason}"
        );
    }

    /// "Produced nothing" is one definition now, and this is it. Written per pair it can
    /// be wrong per pair, and it was.
    #[test]
    fn a_verdict_and_a_blank_both_count_as_nothing() {
        assert!(!produced_text(&Extracted::Unextractable {
            reason: "x".into()
        }));
        assert!(!produced_text(&Extracted::Text(Extraction {
            text: "   \n ".into(),
            title: None,
            tool: "t".into(),
            version: "1".into(),
            notes: vec![],
        })));
        assert!(produced_text(&Extracted::Text(Extraction {
            text: "a word".into(),
            title: None,
            tool: "t".into(),
            version: "1".into(),
            notes: vec![],
        })));
    }

    /// A kind with no reader says so as a fact about the kind, not about the bytes.
    #[test]
    fn a_kind_with_no_reader_says_which_kind() {
        let out = extract(ContentKind::Audio, b"\x00\x01", None, None);
        match out {
            Extracted::Unextractable { reason } => assert!(reason.contains("audio"), "{reason}"),
            other => panic!("audio has no reader here: {other:?}"),
        }
    }

    /// Every kind the classifier can return either has a reader or is deliberately
    /// unreadable — and the deliberate ones are the two that go elsewhere plus `other`.
    #[test]
    fn only_the_kinds_that_go_elsewhere_have_no_reader() {
        for kind in ContentKind::ALL {
            let has = !readers_for(*kind).is_empty();
            let expected = !matches!(
                kind,
                ContentKind::Audio | ContentKind::Markdown | ContentKind::Other
            );
            assert_eq!(has, expected, "{kind} has the wrong reader list");
        }
    }

    /// The fallback's own account of why the primary came up short reaches the record.
    /// It used to be a sentence written by hand inside the HTML reader.
    #[test]
    fn the_winner_carries_what_the_readers_before_it_said() {
        // Too little article for readability, so the whole-page reader takes it.
        let out = extract(
            ContentKind::Html,
            b"<html><body><p>hi</p></body></html>",
            None,
            None,
        );
        let Extracted::Text(e) = out else {
            panic!("the whole page is still content");
        };
        assert_eq!(e.tool, Reader::WholePage.name());
        assert!(
            e.notes.iter().any(|n| n.contains("readability")),
            "{:?}",
            e.notes
        );
    }
}
