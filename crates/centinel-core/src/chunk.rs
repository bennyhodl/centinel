//! Splitting derived text into retrievable passages.
//!
//! **A chunk's identity is the hash of its text** (SPEC §6). Not its position, not its
//! document, not a row id. Two consequences fall out of that one decision:
//!
//! - Re-collecting a page whose footer changed re-chunks it, but every unchanged chunk
//!   hashes the same — so only genuinely new text is ever embedded. The expensive work
//!   is proportional to what changed, not to what was recollected.
//! - The same boilerplate appearing on a thousand pages is **one** chunk with a thousand
//!   placements, rather than a thousand near-identical vectors crowding the index.
//!
//! Chunking is markdown-aware because everything upstream produces markdown: `htmd` for
//! HTML, `pdf-inspector` for PDFs. Headings are the natural seam, and carrying the
//! heading path into the chunk text gives an embedding model context it would otherwise
//! have to guess at.

use serde::{Deserialize, Serialize};

/// Target chunk size in characters.
///
/// Qwen3-Embedding-0.6B accepts 32K tokens, so this is far below any model limit. The
/// binding constraint is retrieval quality, not capacity: a chunk large enough to hold
/// several topics dilutes its own embedding and returns vague matches.
pub const DEFAULT_TARGET_CHARS: usize = 1200;

/// Characters of the previous chunk repeated at the start of the next.
///
/// Guards the seam. A sentence split across a boundary is otherwise unfindable from
/// either side.
pub const DEFAULT_OVERLAP_CHARS: usize = 150;

/// Chunks shorter than this are folded into their neighbour rather than kept.
///
/// A stray heading or a two-word line is not a retrievable passage.
pub const MIN_CHUNK_CHARS: usize = 80;

#[derive(Clone, Debug)]
pub struct ChunkConfig {
    pub target_chars: usize,
    pub overlap_chars: usize,
    pub min_chars: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            target_chars: DEFAULT_TARGET_CHARS,
            overlap_chars: DEFAULT_OVERLAP_CHARS,
            min_chars: MIN_CHUNK_CHARS,
        }
    }
}

/// One retrievable passage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    /// `sha256(text)` — the chunk's identity (SPEC §6).
    pub chunk_hash: String,
    /// The passage, including its heading path prefix.
    pub text: String,
    /// Position within the document, for reassembling reading order.
    pub ordinal: usize,
    /// Markdown heading trail, e.g. `Budget > Capital Projects`. Empty at the top level.
    pub heading: String,
    /// Character span within the derived text, so a hit can be located in the original.
    pub char_start: usize,
    pub char_end: usize,
}

impl Chunk {
    /// `pub(crate)` so other modules' tests can build a chunk with a real `chunk_hash`
    /// rather than fabricating one — a fabricated hash would make a cache-hit test pass
    /// for the wrong reason.
    #[cfg(test)]
    pub(crate) fn new(text: String, ordinal: usize, heading: String, char_start: usize) -> Self {
        let source_len = text.chars().count();
        Self::spanning(text, ordinal, heading, char_start, source_len)
    }

    /// For text that is longer than the source it came from.
    ///
    /// A heading path prefix and a repeated column header are both *context*: they are
    /// written into the chunk but were read once, elsewhere. Measuring the span from the
    /// emitted text walks `char_end` past the end of the document and makes a hit
    /// impossible to locate in the original — which is the only thing the span is for.
    fn spanning(
        text: String,
        ordinal: usize,
        heading: String,
        char_start: usize,
        source_len: usize,
    ) -> Self {
        use sha2::{Digest, Sha256};
        Self {
            chunk_hash: hex::encode(Sha256::digest(text.as_bytes())),
            text,
            ordinal,
            heading,
            char_start,
            char_end: char_start + source_len,
        }
    }
}

/// A markdown block: a heading, or a run of body text under one.
#[derive(Debug)]
struct Block {
    heading_path: String,
    text: String,
    char_start: usize,
}

/// Splits markdown into passages.
pub fn chunk_markdown(markdown: &str, config: &ChunkConfig) -> Vec<Chunk> {
    let blocks = split_into_blocks(markdown);

    let mut chunks = Vec::new();
    let mut ordinal = 0usize;

    for block in blocks {
        for (text, start, source_len) in pack(&block.text, block.char_start, config) {
            // Prefixing the heading path is what lets a chunk stand alone. "Total:
            // $4.2M" is meaningless; "Budget > Capital Projects\n\nTotal: $4.2M" is
            // retrievable.
            let text = if block.heading_path.is_empty() {
                text
            } else {
                format!("{}\n\n{}", block.heading_path, text)
            };
            chunks.push(Chunk::spanning(
                text,
                ordinal,
                block.heading_path.clone(),
                start,
                source_len,
            ));
            ordinal += 1;
        }
    }

    // A document with no headings and less than one chunk of text still yields one
    // chunk; a genuinely empty document yields none.
    chunks
}

/// Groups lines under their heading trail.
fn split_into_blocks(markdown: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    // Index i holds the most recent heading at level i+1.
    let mut trail: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_start = 0usize;
    let mut chars_seen = 0usize;

    let flush = |blocks: &mut Vec<Block>, trail: &[String], text: &mut String, start: usize| {
        if !text.trim().is_empty() {
            blocks.push(Block {
                heading_path: trail.join(" > "),
                text: text.trim().to_string(),
                char_start: start,
            });
        }
        text.clear();
    };

    for line in markdown.lines() {
        let line_chars = line.chars().count() + 1; // +1 for the newline

        if let Some((level, title)) = parse_heading(line) {
            flush(&mut blocks, &trail, &mut current, current_start);
            trail.truncate(level.saturating_sub(1));
            while trail.len() < level.saturating_sub(1) {
                trail.push(String::new());
            }
            trail.push(title);
            current_start = chars_seen + line_chars;
        } else {
            if current.is_empty() {
                current_start = chars_seen;
            }
            current.push_str(line);
            current.push('\n');
        }
        chars_seen += line_chars;
    }
    flush(&mut blocks, &trail, &mut current, current_start);

    blocks
}

/// Recognises `#`-style headings. Setext headings are not produced by our extractors.
fn parse_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = trimmed[hashes..].trim();
    // `#hashtag` is not a heading; ATX headings require a space.
    if rest.is_empty() || !trimmed[hashes..].starts_with(char::is_whitespace) {
        return None;
    }
    Some((hashes, rest.to_string()))
}

/// Packs paragraphs into target-sized pieces with overlap.
///
/// Returns `(text, char_start, source_len)`. The last two describe the **source**, and
/// only match the text's own length when nothing was carried onto the piece — see
/// [`Chunk::spanning`].
fn pack(text: &str, base_offset: usize, config: &ChunkConfig) -> Vec<(String, usize, usize)> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    if text.chars().count() <= config.target_chars {
        let len = text.chars().count();
        return vec![(text.to_string(), base_offset, len)];
    }

    let mut out: Vec<(String, usize, usize)> = Vec::new();
    let mut current = String::new();
    let mut current_start = base_offset;
    let mut offset = base_offset;

    // Overlap text is repeated, but it is repeated *from the source at that position*,
    // so a packed piece's span is simply its own length. Only the oversized path below
    // emits text the document does not hold at `char_start`.
    let packed = |s: &str| {
        let t = s.trim().to_string();
        let len = t.chars().count();
        (t, len)
    };

    for para in text.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            offset += 2;
            continue;
        }
        let para_len = para.chars().count();

        // A single paragraph over target — a CSV, a PDF table, a wall of text. How it
        // is cut depends on whether it holds records; see [`split_oversized`].
        if para_len > config.target_chars {
            if !current.trim().is_empty() {
                let (t, n) = packed(&current);
                out.push((t, current_start, n));
                current.clear();
            }
            for piece in split_oversized(para, config.target_chars) {
                out.push((piece.text, offset, piece.source_len));
                // Advances by what the piece *consumed*, not by what it emitted. A
                // repeated header is context, not new text, and charging it to the
                // offset would push every later `char_start` off the source.
                offset += piece.source_len;
            }
            current_start = offset;
            continue;
        }

        if current.chars().count() + para_len > config.target_chars && !current.trim().is_empty() {
            let (t, n) = packed(&current);
            out.push((t, current_start, n));
            let tail = tail_chars(&current, config.overlap_chars);
            current_start = offset.saturating_sub(tail.chars().count());
            current = tail;
        }

        if !current.is_empty() && !current.ends_with('\n') {
            current.push_str("\n\n");
        }
        current.push_str(para);
        offset += para_len + 2;
    }

    if !current.trim().is_empty() {
        let (t, n) = packed(&current);
        out.push((t, current_start, n));
    }

    // Fold a runt tail into its predecessor rather than emitting it alone.
    if out.len() > 1
        && out
            .last()
            .is_some_and(|(t, _, _)| t.chars().count() < config.min_chars)
    {
        let (runt, _, runt_len) = out.pop().expect("len > 1");
        if let Some((prev, _, prev_len)) = out.last_mut() {
            prev.push_str("\n\n");
            prev.push_str(&runt);
            *prev_len += runt_len;
        }
    }

    out
}

/// One piece of an oversized paragraph.
///
/// `text` and `source_len` differ whenever a header is repeated: the header is context
/// carried onto the piece, not text the piece consumed from the document.
struct Piece {
    text: String,
    source_len: usize,
}

/// Splits a paragraph that is over target.
///
/// **A record is never cut.** A CSV row sliced down the middle is not a record, and a row
/// without its column names cannot be read — a hit on `4307 N Troy St` is unusable if
/// nothing in the passage says which column an address sits in. So a paragraph that holds
/// records is split on line boundaries with its header repeated on every piece, and
/// anything else falls back to the blind character split this has always done.
///
/// Two sightings earned this: `publicrec.hillsclerk.com`'s daily filings, where 5,500 of
/// 5,524 chunks carried no column names and several began mid-field, and the bulk data
/// CSV before it. `docs/FIELD-NOTES.md` — *a record set is not a document*.
fn split_oversized(para: &str, target: usize) -> Vec<Piece> {
    match header_of(para) {
        Some((header, consumed)) => split_records(para, header, consumed, target),
        None => para
            .chars()
            .collect::<Vec<char>>()
            .chunks(target)
            .map(|c| {
                let text: String = c.iter().collect();
                let source_len = text.chars().count();
                Piece { text, source_len }
            })
            .collect(),
    }
}

/// Whether a paragraph holds records — delimited text, or a markdown table.
///
/// Exposed for [`crate::boilerplate`], which must leave record sets alone: a row that
/// repeats across documents is data, not chrome.
pub(crate) fn holds_records(para: &str) -> bool {
    header_of(para.trim()).is_some()
}

/// The column names of a record block, and the bytes they occupy.
///
/// Two shapes, both already in the store:
///
/// - a **markdown table**, which `htmd` and `pdf-inspector` both emit with a `|---|` rule
///   under the names — the rule is part of the header, or the repeat is not a table;
/// - **delimited text** straight from `passthrough`, recognised by every row carrying the
///   same field count as the first.
///
/// Returning `None` is the ordinary answer. Prose must fall through untouched.
fn header_of(para: &str) -> Option<(&str, usize)> {
    let mut lines = para.split_inclusive('\n');
    let first = lines.next()?;
    let second = lines.next()?;
    let names = first.trim_end_matches(['\n', '\r']);
    let rule = second.trim_end_matches(['\n', '\r']);

    if names.trim_start().starts_with('|') && is_table_rule(rule) {
        return Some((
            &para[..first.len() + rule.len()],
            first.len() + second.len(),
        ));
    }

    // Quote-aware, because a court case title is `"Desir, Mildred vs Tampa General"` and
    // counting raw commas would call that four fields.
    for sep in [',', '\t', ';'] {
        let n = fields(names, sep);
        if n < 3 {
            continue;
        }
        let sample: Vec<&str> = para
            .lines()
            .skip(1)
            .filter(|l| !l.trim().is_empty())
            .take(5)
            .collect();
        if sample.len() >= 2 && sample.iter().all(|l| fields(l, sep) == n) {
            return Some((names, first.len()));
        }
    }
    None
}

/// Field count, ignoring separators inside double quotes.
fn fields(line: &str, sep: char) -> usize {
    let mut n = 1;
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c == sep && !quoted => n += 1,
            _ => {}
        }
    }
    n
}

fn is_table_rule(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// Packs whole rows under a repeated header.
///
/// No overlap here, unlike [`pack`]. Overlap guards a sentence split across a seam; rows
/// are already whole, and repeating them would put near-identical passages in the index —
/// the thing chunk-by-hash exists to avoid.
fn split_records(para: &str, header: &str, consumed: usize, target: usize) -> Vec<Piece> {
    // Counted once, on the first piece, because that is where the header is read from.
    let head_source = para[..consumed].chars().count();
    let head_chars = header.chars().count();

    let mut out: Vec<Piece> = Vec::new();
    let mut rows = String::new();
    let mut rows_len = 0usize;

    let flush = |rows: &mut String, rows_len: &mut usize, out: &mut Vec<Piece>| {
        if rows.trim().is_empty() {
            return;
        }
        let first = out.is_empty();
        out.push(Piece {
            text: format!("{header}\n{}", rows.trim_end()),
            source_len: *rows_len + if first { head_source } else { 0 },
        });
        rows.clear();
        *rows_len = 0;
    };

    // `split_inclusive` keeps each newline with its row, so `source_len` is exact rather
    // than off by one wherever the document does not end in one.
    for line in para[consumed..].split_inclusive('\n') {
        let line_chars = line.chars().count();
        // A row that alone exceeds the target still goes out whole. Cutting it would
        // defeat the only rule this function has.
        if !rows.is_empty() && head_chars + rows.chars().count() + line_chars > target {
            flush(&mut rows, &mut rows_len, &mut out);
        }
        rows.push_str(line);
        rows_len += line_chars;
    }
    flush(&mut rows, &mut rows_len, &mut out);
    out
}

fn tail_chars(s: &str, n: usize) -> String {
    let count = s.chars().count();
    s.chars().skip(count.saturating_sub(n)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ChunkConfig {
        ChunkConfig::default()
    }

    #[test]
    fn chunk_identity_is_the_hash_of_its_text() {
        let a = chunk_markdown("# T\n\nSome content here that is long enough.", &cfg());
        let b = chunk_markdown("# T\n\nSome content here that is long enough.", &cfg());
        assert_eq!(a[0].chunk_hash, b[0].chunk_hash);

        let c = chunk_markdown("# T\n\nDifferent content entirely, also long.", &cfg());
        assert_ne!(a[0].chunk_hash, c[0].chunk_hash);
    }

    /// The property that makes re-collection cheap: an unchanged section keeps its
    /// hash even when the rest of the document changed.
    #[test]
    fn unchanged_sections_keep_their_hash_across_edits() {
        let long = "Council considered the rezoning application at length. ".repeat(30);
        let v1 = format!("# Minutes\n\n{long}\n\n## Footer\n\nRevised 2026-01-01");
        let v2 = format!("# Minutes\n\n{long}\n\n## Footer\n\nRevised 2026-08-03");

        let a = chunk_markdown(&v1, &cfg());
        let b = chunk_markdown(&v2, &cfg());

        let shared: Vec<_> = a
            .iter()
            .filter(|c| b.iter().any(|o| o.chunk_hash == c.chunk_hash))
            .collect();
        assert!(
            !shared.is_empty(),
            "the unchanged body must survive a footer edit"
        );
    }

    #[test]
    fn heading_path_is_carried_into_the_chunk() {
        let md = "# Budget\n\n## Capital Projects\n\nTotal allocation is $4.2M for the year.";
        let chunks = chunk_markdown(md, &cfg());
        let c = chunks.last().unwrap();

        assert_eq!(c.heading, "Budget > Capital Projects");
        assert!(
            c.text.starts_with("Budget > Capital Projects"),
            "a bare number is not retrievable without its heading: {:?}",
            c.text
        );
        assert!(c.text.contains("$4.2M"));
    }

    #[test]
    fn long_documents_split_with_overlap() {
        let para = "The commission reviewed the proposal in detail. ".repeat(20);
        let md = format!("# Doc\n\n{para}\n\n{para}\n\n{para}");
        let chunks = chunk_markdown(&md, &cfg());

        assert!(chunks.len() > 1, "should have split");
        for c in &chunks {
            // Allow for the heading prefix on top of the target.
            assert!(
                c.text.chars().count() <= DEFAULT_TARGET_CHARS + 200,
                "chunk of {} chars is too big",
                c.text.chars().count()
            );
        }
        assert!(chunks.windows(2).all(|w| w[0].ordinal < w[1].ordinal));
    }

    #[test]
    fn an_oversized_single_paragraph_is_split_rather_than_emitted_whole() {
        // A PDF table often arrives as one enormous line.
        let wall = "x".repeat(10_000);
        let chunks = chunk_markdown(&format!("# T\n\n{wall}"), &cfg());
        assert!(chunks.len() > 5);
        assert!(
            chunks
                .iter()
                .all(|c| c.text.chars().count() <= DEFAULT_TARGET_CHARS + 200)
        );
    }

    #[test]
    fn empty_and_whitespace_documents_yield_no_chunks() {
        assert!(chunk_markdown("", &cfg()).is_empty());
        assert!(chunk_markdown("   \n\n  \n", &cfg()).is_empty());
    }

    #[test]
    fn a_document_with_no_headings_still_chunks() {
        let chunks = chunk_markdown("Just a plain paragraph of reasonable length here.", &cfg());
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].heading.is_empty());
    }

    #[test]
    fn hashtags_are_not_headings() {
        assert!(parse_heading("#notaheading").is_none());
        assert!(parse_heading("####### too many").is_none());
        assert_eq!(parse_heading("## Real"), Some((2, "Real".to_string())));
    }

    #[test]
    fn deeper_headings_nest_and_siblings_replace() {
        let md = "# A\n\nbody one is here and long enough to keep\n\n\
                  ## B\n\nbody two is here and long enough to keep\n\n\
                  ## C\n\nbody three is here and long enough to keep";
        let chunks = chunk_markdown(md, &cfg());
        let headings: Vec<_> = chunks.iter().map(|c| c.heading.as_str()).collect();
        assert!(headings.contains(&"A"));
        assert!(headings.contains(&"A > B"));
        assert!(
            headings.contains(&"A > C"),
            "sibling must replace, not nest"
        );
    }

    #[test]
    fn char_spans_are_ordered_and_within_the_document() {
        let md = format!("# T\n\n{}", "sentence here. ".repeat(200));
        let chunks = chunk_markdown(&md, &cfg());
        let len = md.chars().count();
        for c in &chunks {
            assert!(c.char_start <= c.char_end);
            assert!(c.char_start <= len, "span starts past the document");
        }
    }

    // ── record blocks ─────────────────────────────────────────────────────────
    //
    // The shapes below are `publicrec.hillsclerk.com`'s, copied rather than invented:
    // the daily civil filings CSV and a Registry balances table out of `pdf-inspector`.

    const CSV_HEADER: &str = "CaseCategory,CaseTypeDescription,CaseNumber,Title,FilingDate,\
                              PartyType,FirstName,MiddleName,LastName/CompanyName,\
                              PartyAddress,Attorney";

    fn filings(rows: usize) -> String {
        let row = "\"CV\",\"Professional Malpractice Business\",\"26-CA-007814\",\
                   \"Desir, Mildred Sephora vs Tampa General Hospital\",\"07/19/2026\",\
                   \"Plaintiff\",\"Mildred\",\"Sephora\",\"Desir\",\
                   \"4307 N Troy St, Tampa, FL 33610\",\"No Attorney\"";
        std::iter::once(CSV_HEADER.to_string())
            .chain((0..rows).map(|_| row.to_string()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn every_chunk_of_a_csv_carries_the_column_names() {
        let chunks = chunk_markdown(&filings(200), &cfg());
        assert!(chunks.len() > 1, "200 rows must not fit in one chunk");
        for c in &chunks {
            assert!(
                c.text.contains("PartyAddress"),
                "a chunk without column names cannot be read:\n{}",
                &c.text[..c.text.len().min(120)]
            );
        }
    }

    #[test]
    fn a_csv_row_is_never_cut_in_half() {
        for c in chunk_markdown(&filings(200), &cfg()) {
            for line in c.text.lines().skip(1) {
                assert!(
                    line.starts_with("\"CV\"") && line.ends_with("\"No Attorney\""),
                    "a record was split: {line}"
                );
            }
        }
    }

    /// The header repeats in the text but is read once from the document, so spans must
    /// stay inside it. Charging the repeat to the offset walks them off the end.
    #[test]
    fn a_repeated_header_does_not_push_spans_past_the_document() {
        let csv = filings(200);
        let len = csv.chars().count();
        let chunks = chunk_markdown(&csv, &cfg());
        assert!(
            chunks.last().expect("chunks").char_end <= len,
            "spans ran past a {len}-character document"
        );
        for pair in chunks.windows(2) {
            assert!(
                pair[0].char_start <= pair[1].char_start,
                "spans went backwards"
            );
        }
    }

    /// A quoted comma inside a case title must not be counted as a field separator, or
    /// the header is never recognised and nothing above fires.
    #[test]
    fn a_comma_inside_a_quoted_field_is_not_a_separator() {
        assert_eq!(fields(CSV_HEADER, ','), 11);
        assert_eq!(
            fields(
                "\"CV\",\"x\",\"26-CA-1\",\"Desir, Mildred vs Tampa General\",\"07/19/2026\",\
                 \"Plaintiff\",\"M\",\"S\",\"D\",\"4307 N Troy St, Tampa, FL\",\"No Attorney\"",
                ','
            ),
            11
        );
    }

    #[test]
    fn a_markdown_table_repeats_its_header_and_its_rule() {
        let mut md =
            String::from("|Case Number|Party Name|Increases|Decreases|\n|---|---|---|---|\n");
        for i in 0..200 {
            md.push_str(&format!(
                "|13-CP-{i:06}|MUNN, ARTHUR RENA|$1,634.09|$1,634.09|\n"
            ));
        }
        let chunks = chunk_markdown(&md, &cfg());
        assert!(chunks.len() > 1);
        for c in &chunks {
            assert!(
                c.text.contains("|Case Number|Party Name|"),
                "lost the header"
            );
            assert!(c.text.contains("|---|---|---|---|"), "lost the table rule");
        }
    }

    /// Prose is not a record set. A paragraph full of commas must not acquire a header
    /// row that nobody wrote, and must keep splitting the way it always has.
    #[test]
    fn prose_is_not_mistaken_for_a_record_block() {
        let one_line = "The clerk records deeds, liens, and judgments. ".repeat(80);
        assert!(header_of(&one_line).is_none(), "one line has no header");

        let many_lines: String = (0..40)
            .map(|i| format!("Line {i} lists deeds, liens{}\n", ",".repeat(i % 4)))
            .collect();
        assert!(
            header_of(&many_lines).is_none(),
            "inconsistent field counts are prose, not records"
        );

        let chunks = chunk_markdown(&one_line, &cfg());
        assert!(chunks.len() > 1, "an oversized paragraph still splits");
        assert!(
            chunks.iter().all(|c| !c.text.contains("The clerk records deeds, liens, and judgments. The clerk records deeds, liens, and judgments. The clerk")
                || c.text.chars().count() <= cfg().target_chars + 200),
            "prose chunks stayed near target"
        );
    }
}
