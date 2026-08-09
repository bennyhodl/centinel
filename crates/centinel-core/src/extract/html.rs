//! HTML to text: which region of a page is the document, and how it becomes markdown.
//!
//! Three readers, tried in the order [`super::readers_for`] lists them, each answering the
//! same question with less and less of the page's own help:
//!
//! 1. [`html_marked`] — the region the page declares to be its content.
//! 2. [`html_readability`] — a guess, scored by text density.
//! 3. [`html_whole_page`] — everything but the scripts, which is still content worth
//!    having on a listing page that has no article at all.
//!
//! Everything else here is the markdown conversion those three share: the converter
//! policy, the table handler, the anchor and image handlers, and the title rule.
//!
//! ## Why it is one module
//!
//! Because the policy is coupled, and the coupling is invisible when it is spread out.
//! [`region_converter`] drops the address out of an `<a>`; [`whole_page_is_better`] decides
//! whether a page is a menu by measuring how much of it *is* addresses. Do the first
//! everywhere and the second stops working — a navigation menu reads as an innocent
//! bulleted list. The two are forty lines apart here, which is where a coupling like that
//! belongs. They were in different modules for one commit and needed a test to hold them
//! together.

use super::{DOM_SMOOTHIE_VERSION, Extracted, Extraction, HTMD_VERSION, Reader};

/// Readability output shorter than this is short enough to say so in a note.
///
/// **It used to be a refusal, and that lost the answer.** The reasoning was that a listing
/// page has no article for readability to find, so a thin result meant "try the whole
/// page". Measured against 300 documents from six city and county sites, twelve fall below
/// this line — and on eleven of them readability had found exactly the right thing:
///
/// ```text
/// clevelandohio.gov/…/designated-landmarks/denison-cemetery   123 chars
///
///   ## Landmark Details
///   1835
///   W 23rd Street and Garden Avenue
///   Architect  N/A
/// ```
///
/// That is the entire record the page exists to publish. The floor threw it away and kept
/// the whole page instead: 29,099 characters, 83% of it link text, of which three such
/// pages supplied 105 of that corpus's 174 chunks and outranked the Police Division page on
/// a search for `police`.
///
/// So a short article is now kept and **noted**. The fallback still exists for the case it
/// was actually built for — readability finding nothing at all, where the whole page is the
/// only text there is — and [`produced_text`] already routes that correctly, because empty
/// text is not text.
///
/// What catches the other failure on this same template, where readability picks a *wrong*
/// dense region rather than a small right one, is not a length at all. `czech-sokol-hall`
/// yields 378 characters of City Hall's address and office hours, comfortably above any
/// floor; [`crate::verdict`] calls it at 73% link text. Length was never the question.
const SHORT_ARTICLE_CHARS: usize = 200;

/// Skipping these matters: htmd otherwise serialises inline JSON-LD and drupalSettings
/// into the markdown, tripling the output with machine noise.
fn markdown_converter() -> htmd::HtmlToMarkdown {
    htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "noscript", "svg", "form"])
        .add_handler(vec!["table"], table_to_markdown)
        .build()
}

/// The converter [`html_marked`] uses: chrome dropped, and an `<a>` reduced to its text.
///
/// **Scoped to the marked region on purpose, and the reason is a fallback it would break.**
/// Stripping addresses everywhere makes a navigation menu stop looking like navigation —
/// `[Department](/d/1)` becomes `Department`, so the link-share test that
/// [`whole_page_is_better`] uses to refuse a menu sees an innocent bulleted list and takes
/// it. The measure and the markup are coupled, and only a region the page itself has
/// declared to be content is somewhere that coupling can be given up.
///
/// It is also the right boundary on its own terms. [`html_whole_page`] exists so that *a
/// listing page with no article is still content worth having*, and on such a page the
/// links **are** the content; inside a marked region they are addressing beside it.
fn region_converter(skip: &[&'static str]) -> htmd::HtmlToMarkdown {
    let mut tags = vec!["script", "style", "noscript", "svg", "form"];
    tags.extend_from_slice(skip);
    htmd::HtmlToMarkdown::builder()
        .skip_tags(tags)
        .add_handler(vec!["table"], table_to_markdown)
        .add_handler(vec!["a"], anchor_text_only)
        .add_handler(vec!["img"], image_alt_only)
        .build()
}

/// An `<a>` becomes its text. The address does not reach the corpus.
///
/// The anchor text is content — *Meeting Agendas*, *Andrew Mitermiler* — and the URL beside
/// it is machine addressing that nothing searches for and nothing should embed. Measured
/// over the derived text of five sites, the URLs inside `<a>` are **55%** of every character
/// `medinaco.org` produced, 23% of `dunedin.gov`, 13% of `clevelandohio.gov`:
///
/// ```text
/// [Google Calendar](https://www.google.com/calendar/event?action=TEMPLATE&dates=…)
///  ^^^^^^^^^^^^^^^ 15 characters of content   ^^^^^^^^^^ 250 characters of noise
/// ```
///
/// **Nothing is lost, and that is a property of the store rather than a hope.** The derived
/// text is derived; every `href` stays in the raw HTML blob, which is truth and immutable.
/// Every consumer of those links already reads the blob and not this text:
/// `enclosure::documents` finds the PDFs a page encloses from the raw bytes, and
/// `ops::investigate::crumbs_on` counts off-host hosts the same way. A crumb table, when it
/// is built, rebuilds from the same blobs.
///
/// Its companion is [`image_alt_only`], which does the same for `<img>`.
fn anchor_text_only(
    handlers: &dyn htmd::element_handler::Handlers,
    element: htmd::Element<'_>,
) -> Option<htmd::element_handler::HandlerResult> {
    let mut out = String::new();
    for child in element.node.children.borrow().iter() {
        if let Some(result) = handlers.handle(child) {
            out.push_str(&result.content);
        }
    }
    Some(out.into())
}

/// An `<img>` becomes its `alt`, by the same rule as [`anchor_text_only`].
///
/// The alt text is what a person would read aloud — *Police Lights*, *Cyber Security* — and
/// the `src` beside it is an address for a file this corpus does not hold. On
/// `clevelandohio.gov` those addresses are a further 12% of every derived character, and on
/// `medinaco.org` every event page carried the URL of a loading spinner.
///
/// Kept rather than dropped whole, because alt text is the accessible description of the
/// image and sometimes the only caption a page gives. Where it is empty this leaves nothing,
/// which is the right answer for a decorative image that declared itself as one.
///
/// This also reaches the [`Extraction::strip_data_uris`] case at its source for any document
/// read from a marked region: a base64 `data:` image never becomes text to begin with. That
/// pass stays, because the readers outside this converter still need it.
fn image_alt_only(
    _handlers: &dyn htmd::element_handler::Handlers,
    element: htmd::Element<'_>,
) -> Option<htmd::element_handler::HandlerResult> {
    let alt = element
        .attrs
        .iter()
        .find(|a| a.name.local.as_ref() == "alt")
        .map(|a| a.value.trim().to_string())
        .unwrap_or_default();
    Some(alt.into())
}

/// Every `<table>` as a markdown table, whether or not it declared a header.
///
/// `htmd` writes one only when the table has a `<th>` or a `<thead>` somewhere. Anything
/// else falls to a handler that concatenates the cells with **no separator at all** — no
/// pipe, no space, no row break. On the CTTV caption index that turned fifty rows into a
/// single 10,736-character line:
///
/// ```text
/// [Transcript #2693](…)8/3/2026Tampa City Council Special Discussion [▶ Watch](…)[Transcript #2692](…)…
/// ```
///
/// `8/3/2026Tampa City Council Special Discussion` — the date fused to the meeting name,
/// and every row boundary gone.
///
/// *Why it is worth owning the tag rather than working around the fallback:* a `.gov` site
/// is made of tables — budget line items, salary schedules, permit registers, election
/// returns, bid tabulations — and a headerless one is the common case, not the exotic one.
/// On every table in the corpus the number currently fuses to the label it belongs to, so
/// any chunk drawn from such a page mixes dozens of unrelated records and can never be
/// searched apart. It is the widest-reaching defect in `docs/FIELD-NOTES.md` and the only
/// one that improves documents already collected.
fn table_to_markdown(
    handlers: &dyn htmd::element_handler::Handlers,
    element: htmd::Element<'_>,
) -> Option<htmd::element_handler::HandlerResult> {
    let mut rows = table_rows(handlers, element.node);
    let width = rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);

    // A `<table>` used for layout holds no rows at all. Nothing here can improve on what
    // htmd already does with one, and inventing an empty table for it would be worse.
    if width == 0 {
        return handlers.fallback(element);
    }

    // A markdown table needs a header row. The table's own if it declared one; an empty
    // one if it did not — promoting the first row of data would silently spend a record on
    // the header of every table that never had one, and on a permit register that record
    // is somebody's permit.
    let header = match rows.first().is_some_and(|r| r.all_header) {
        true => rows.remove(0).cells,
        false => Vec::new(),
    };

    let mut out = String::from("\n\n");
    push_row(&mut out, &header, width);
    out.push('|');
    for _ in 0..width {
        out.push_str(" --- |");
    }
    out.push('\n');
    for row in &rows {
        push_row(&mut out, &row.cells, width);
    }
    out.push('\n');

    Some(out.into())
}

/// One `<tr>`, converted.
struct TableRow {
    cells: Vec<String>,
    /// Every cell was a `<th>`, so this row is the table's own header.
    all_header: bool,
}

/// The rows of a table, from `<tr>` directly under it or under a `<thead>`/`<tbody>`/`<tfoot>`.
///
/// Two levels deep and no further, which is what keeps a nested table's rows out of its
/// parent: html5ever puts a nested `<table>` inside the `<td>` that held it, so the only
/// way to reach those rows is through a cell — and a cell is converted whole, by this same
/// handler, one level down.
fn table_rows(
    handlers: &dyn htmd::element_handler::Handlers,
    table: &std::rc::Rc<htmd::Node>,
) -> Vec<TableRow> {
    let mut rows = Vec::new();
    for child in table.children.borrow().iter() {
        match tag_name(child) {
            Some("tr") => rows.push(table_row(handlers, child)),
            Some("thead" | "tbody" | "tfoot") => {
                for row in child.children.borrow().iter() {
                    if tag_name(row) == Some("tr") {
                        rows.push(table_row(handlers, row));
                    }
                }
            }
            _ => {}
        }
    }
    rows
}

fn table_row(
    handlers: &dyn htmd::element_handler::Handlers,
    tr: &std::rc::Rc<htmd::Node>,
) -> TableRow {
    let mut cells = Vec::new();
    let mut all_header = true;
    for cell in tr.children.borrow().iter() {
        let tag = match tag_name(cell) {
            Some(tag @ ("td" | "th")) => tag,
            _ => continue,
        };
        all_header &= tag == "th";
        cells.push(cell_text(
            &handlers.handle(cell).map(|r| r.content).unwrap_or_default(),
        ));
    }
    TableRow {
        all_header: all_header && !cells.is_empty(),
        cells,
    }
}

fn tag_name(node: &std::rc::Rc<htmd::Node>) -> Option<&str> {
    match &node.data {
        markup5ever_rcdom::NodeData::Element { name, .. } => Some(name.local.as_ref()),
        _ => None,
    }
}

/// One cell, flattened onto one line.
///
/// A markdown table row is a line, so a cell holding a list or a paragraph break has to
/// give up its own line breaks or it ends the row early. A literal `|` is escaped for the
/// same reason: unescaped, it would open a column that is not there and shift every value
/// to its right into the wrong one.
fn cell_text(raw: &str) -> String {
    raw.replace('|', "\\|")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_row(out: &mut String, cells: &[String], width: usize) {
    out.push('|');
    for i in 0..width {
        out.push(' ');
        out.push_str(cells.get(i).map_or("", String::as_str));
        out.push_str(" |");
    }
    out.push('\n');
}

/// Content markers, **outermost first**, and the order is load-bearing.
///
/// `<main>` contains `<article>` on the Cleveland landmark template, and the landmark
/// record sits between the two — at byte 112,264 where `<main>` opens at 109,469 and
/// `<article>` at 113,024. Taking the most specific marker would miss the one fact the page
/// exists to publish. So the rule is the widest region the page marks as its own, and the
/// narrower ones are there for pages that mark nothing else.
const MARKERS: &[Marker] = &[
    Marker::new("main", &["<main"]),
    Marker::new("[role=main]", &["role=\"main\"", "role='main'"]),
    Marker::new(
        "#main-content",
        &["id=\"main-content\"", "id='main-content'"],
    ),
    Marker::new(
        ".main-content",
        &["class=\"main-content\"", "class='main-content'"],
    ),
    Marker::new("article", &["<article"]),
];

/// A content marker: the selector that finds the region, and the substrings that prove it
/// is worth parsing for.
///
/// The two travel together because they were two lists once, and the second was missing
/// `.main-content` and single-quoted `id='main-content'` — so those selectors could never
/// be reached, and the guard silently decided which markers existed. Paired here, a marker
/// that cannot be cheaply detected is a compile-time omission rather than dead markup.
struct Marker {
    selector: &'static str,
    needles: &'static [&'static str],
}

impl Marker {
    const fn new(selector: &'static str, needles: &'static [&'static str]) -> Self {
        Self { selector, needles }
    }
}

/// Dropped inside the region. Within a marked region these are sub-navigation, and the
/// page has already told us the region is the document.
const CHROME: &[&str] = &["nav", "header", "footer"];

/// The region the page marks as its own content, as markdown.
///
/// **Why it runs before readability rather than after it.** Readability is a guess about
/// where the content is, and this is the page's own answer. `dom_smoothie` scores blocks by
/// text density, which works on an article and fails on a page whose content is a short
/// fact block — there the densest non-navigation block is often a contact panel.
/// `docs/FIELD-NOTES.md` entry 5's landmark template shows both failures on adjacent pages
/// of identical markup, which is what rules out keying anything on the site.
///
/// The marked region recovers what density scoring lost, and drops what it wrongly kept:
///
/// | page | fact | readability | marked |
/// |---|---|---|---|
/// | `czech-sokol-hall` | `Mitermiler`, `Clark Avenue` | absent | **present** |
/// | `czech-sokol-hall` | City Hall's phone number | present | **gone** |
/// | `black-history-boston` | `Melnea Cass`, `Reggie Lewis` | absent | **present** |
///
/// Measured over the six sites in entry 5, **298 of 300** documents carry a marker. It is a
/// rule about HTML rather than about a vendor, which is why it is a reader for the kind and
/// not a per-site hook.
///
/// **The cost, and what pays it.** A marked region is broader than readability's pick: it
/// includes in-page sub-navigation, so a page readability already handled cleanly comes out
/// noisier — the Cleveland police page goes from 4,248 characters at 10% link text to
/// 12,478 at 50%. That is the deliberate trade. Sub-navigation repeats across a source, so
/// [`crate::boilerplate`] removes it corpus-wide. Content that was never extracted cannot be
/// recovered by any later stage; chrome that was extracted can be dropped by one.
///
/// **It does not judge the region it found.** A page that marks nothing, or marks a region
/// holding no text, produces nothing here — and the readers below get their turn, by the
/// same [`produced_text`] rule every other reader is held to.
pub(super) fn html_marked(bytes: &[u8]) -> Extracted {
    let html = String::from_utf8_lossy(bytes);

    // The cheap guard, before any parse. `recognise`/`read` used to be two calls for this,
    // and the substring pass was the whole reason: a DOM parse per document would be paid
    // on the 2 in 300 that mark nothing. One lowercase copy still buys that.
    let lower = html.to_ascii_lowercase();
    let candidates: Vec<&Marker> = MARKERS
        .iter()
        .filter(|m| m.needles.iter().any(|n| lower.contains(n)))
        .collect();
    if candidates.is_empty() {
        return Extracted::Unextractable {
            reason: "the page marks no content region".into(),
        };
    }
    drop(lower);

    let doc = dom_query::Document::from(html.as_ref());
    let converter = region_converter(CHROME);
    let Some((body, selector)) = candidates.iter().find_map(|m| {
        let node = doc
            .try_select(m.selector)
            .and_then(|s| s.nodes().first().cloned())?;
        let md = converter.convert(&node.html()).ok()?.trim().to_string();
        (!md.is_empty()).then_some((md, m.selector))
    }) else {
        return Extracted::Unextractable {
            reason: "the page marks a content region and it holds no text".into(),
        };
    };

    // The same title rule the other HTML readers use, so a document does not change heading
    // path depending on which reader spoke. `<title>`/`og:title` rather than the region's
    // own first heading, because the region often opens with a section name — `Landmark
    // Details` — and not the page's subject.
    let title = html_title(&html);
    Extracted::Text(Extraction {
        text: with_title(title.as_deref(), &body),
        title,
        tool: Reader::Marked.name().into(),
        version: HTMD_VERSION.into(),
        notes: vec![format!("read the region marked `{selector}`")],
    })
}

/// `dom_smoothie` for the article, `htmd` for the markdown.
///
/// "Found an article too short to be one" is a refusal here rather than a note, because
/// that is how [`derive`] learns to try the next reader. `MIN_READABLE_CHARS` of output is
/// the line, and the count travels in the reason so it reaches the record either way.
pub(super) fn html_readability(bytes: &[u8], url: Option<&str>) -> Extracted {
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
    // Tested on the markdown rather than on the text below, because `with_title` would
    // otherwise make a page with a title and no body look like a successful read of its own
    // name — and the whole page, which is where the body would have to come from, would
    // never be reached.
    if md.trim().is_empty() {
        return Extracted::Unextractable {
            reason: "readability found no article; kept the full page instead".into(),
        };
    }

    let chars = md.chars().count();
    // A short article gives way to the whole page only when the whole page is worth
    // having. See `SHORT_ARTICLE_CHARS` for why the length alone decided this once and
    // lost eleven landmark records to do it.
    if chars < SHORT_ARTICLE_CHARS && whole_page_is_better(bytes, &md) {
        return Extracted::Unextractable {
            reason: format!("readability found only {chars} chars; kept the full page instead"),
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
        // Kept, and said out loud. A short article is sometimes the whole record a page
        // publishes and sometimes a sign the reader landed in the wrong element; this note
        // is what lets the two be told apart afterwards, on the document rather than by
        // re-deriving it. See `SHORT_ARTICLE_CHARS`.
        notes: match chars < SHORT_ARTICLE_CHARS {
            true => vec![format!(
                "readability found a short article — {chars} chars; kept it rather than \
                 the whole page"
            )],
            false => vec![],
        },
    })
}

/// Would keeping the whole page beat keeping the short article readability found?
///
/// **A fallback has to be an improvement.** It was assumed to be one, and on the Cleveland
/// landmark template it was the opposite: readability's 123 characters are the landmark's
/// year, street and architect, and the whole page is 29,099 characters of site navigation.
/// Three such pages supplied 105 of 174 chunks in that corpus and outranked the page that
/// actually is about the police on a search for `police`.
///
/// The test is [`crate::verdict`]'s link share, which is the measure that survived
/// validation — 7 flagged, 0 false positives across 100 documents — rather than a new one
/// invented for this decision. Applied to the *candidate*, not to the document: the
/// question is what the whole page would put in the index if it won.
///
/// It answers yes for the case the fallback was built for. The CTTV caption index is a
/// `<table>` of 2,606 transcripts with no `<th>`; readability finds no article in it, the
/// whole page renders as rows of date and meeting name at well under half link text, and
/// that is real content this must not throw away.
///
/// Costs one extra markdown conversion, and only on a short article — twelve documents in
/// three hundred across the six sites this was measured on.
fn whole_page_is_better(bytes: &[u8], article: &str) -> bool {
    // Nothing to protect. A short article that is *itself* a list of links is a listing
    // page seen from the other side, and on those the whole page is the better answer for
    // the reason it always was: it renders the table. The rule is not "short text wins", it
    // is "do not trade real text for navigation" — and here there is no real text to trade.
    if crate::verdict::ReadQuality::measure(bytes, article).link_share > crate::verdict::LINK_SHARE
    {
        return true;
    }

    let whole = html_whole_page(bytes);
    let Some(text) = whole.text().filter(|t| !t.trim().is_empty()) else {
        // Nothing there either, so there is nothing to prefer it for.
        return false;
    };
    crate::verdict::ReadQuality::measure(bytes, text).link_share <= crate::verdict::LINK_SHARE
}

/// The whole page, minus scripts. Worse for search, but a listing page with no article is
/// still content worth having.
pub(super) fn html_whole_page(bytes: &[u8]) -> Extracted {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
