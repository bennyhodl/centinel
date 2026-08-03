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
    fn new(text: String, ordinal: usize, heading: String, char_start: usize) -> Self {
        use sha2::{Digest, Sha256};
        let char_end = char_start + text.chars().count();
        Self {
            chunk_hash: hex::encode(Sha256::digest(text.as_bytes())),
            text,
            ordinal,
            heading,
            char_start,
            char_end,
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
        for (text, start) in pack(&block.text, block.char_start, config) {
            // Prefixing the heading path is what lets a chunk stand alone. "Total:
            // $4.2M" is meaningless; "Budget > Capital Projects\n\nTotal: $4.2M" is
            // retrievable.
            let text = if block.heading_path.is_empty() {
                text
            } else {
                format!("{}\n\n{}", block.heading_path, text)
            };
            chunks.push(Chunk::new(text, ordinal, block.heading_path.clone(), start));
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

/// Packs paragraphs into target-sized pieces with overlap, returning `(text, char_start)`.
fn pack(text: &str, base_offset: usize, config: &ChunkConfig) -> Vec<(String, usize)> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    if text.chars().count() <= config.target_chars {
        return vec![(text.to_string(), base_offset)];
    }

    let mut out: Vec<(String, usize)> = Vec::new();
    let mut current = String::new();
    let mut current_start = base_offset;
    let mut offset = base_offset;

    for para in text.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            offset += 2;
            continue;
        }
        let para_len = para.chars().count();

        // A single paragraph over target — a PDF table, a wall of text — is split on
        // character count. Ugly, but better than emitting one 40KB chunk.
        if para_len > config.target_chars {
            if !current.trim().is_empty() {
                out.push((current.trim().to_string(), current_start));
                current.clear();
            }
            for piece in split_oversized(para, config.target_chars) {
                let piece_len = piece.chars().count();
                out.push((piece, offset));
                offset += piece_len;
            }
            current_start = offset;
            continue;
        }

        if current.chars().count() + para_len > config.target_chars && !current.trim().is_empty() {
            out.push((current.trim().to_string(), current_start));
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
        out.push((current.trim().to_string(), current_start));
    }

    // Fold a runt tail into its predecessor rather than emitting it alone.
    if out.len() > 1
        && out
            .last()
            .is_some_and(|(t, _)| t.chars().count() < config.min_chars)
    {
        let (runt, _) = out.pop().expect("len > 1");
        if let Some((prev, _)) = out.last_mut() {
            prev.push_str("\n\n");
            prev.push_str(&runt);
        }
    }

    out
}

fn split_oversized(para: &str, target: usize) -> Vec<String> {
    let chars: Vec<char> = para.chars().collect();
    chars
        .chunks(target)
        .map(|c| c.iter().collect::<String>())
        .collect()
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
}
