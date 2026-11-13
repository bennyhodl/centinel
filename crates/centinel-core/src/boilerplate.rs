//! Text that repeats across a source's documents.
//!
//! ## Why this is not a strategy
//!
//! A crawl strategy recognises a *site*: a sitemap index, a directory listing, a product
//! that serves its records a certain way. Chrome is not a property of a site. It is a
//! property of a **corpus** — a line that turns up on document after document is
//! navigation whatever the host is built with — so it is found by counting, not by
//! recognising, and it needs no site rules. `docs/FIELD-NOTES.md` settled that principle
//! before there was anything to apply it to.
//!
//! ## What went wrong without it
//!
//! `hillsclerk.com` produced fifty documents. On forty-three, the reader found the article
//! and threw the menu away. On seven it found nothing and returned the whole page, so
//! `marriage-license-application-success-kiosk` — whose content is the single sentence
//! *"Thanks! Your marriage license application has been successfully submitted"* — went
//! into the index as 23,213 characters of navigation. Those seven pages were 61% of every
//! character the source contributed.
//!
//! Chunk-by-hash was supposed to absorb this: identical boilerplate on a thousand pages
//! is meant to be one chunk with a thousand placements. It did not fire once — 311 chunks
//! across 311 placements — because every chunk carries its heading path, and the heading
//! path carries the page title. The same menu under `# General FAQs` and `# Email Us`
//! hashes differently. The defence exists; a difference of a few characters upstream
//! disables it.
//!
//! ## Records are exempt, and that is not a detail
//!
//! Repetition inside a record set is *data*. On `publicrec.hillsclerk.com` the line
//! `|||CLERK|08/13/2018||60,364.22||60,364.22|` appears in five documents because a court
//! registry balance did not move for five days, and the daily filings share their column
//! header across thirty files. Counting either as chrome would delete records and undo
//! [`crate::chunk`]'s guarantee that every passage carries its column names. So a
//! paragraph that holds records is passed through whole, learned from and stripped never.
//!
//! ## What learning may hold
//!
//! The first learner kept every distinct line of the sample as a `String`. A website's
//! distinct lines are its menu, its footer and a few thousand sentences; a bulk data file's
//! are every row. On `publicrec.hillsclerk.com` the first 300 documents came to 804 MB of
//! distinct rows, the map grew to twice that, and `index` was killed before it had found a
//! single line of chrome. What it held grew with the bytes of the source, and nothing about
//! a source says how many bytes that will be.
//!
//! So what it holds is bounded by line counts the learner sets, not by the bytes it reads:
//!
//! - **A hash per line, not the line.** The first reading counts a `u64` for each line,
//!   which costs the same for a 12-character menu item and a 4 KB row. The few lines whose
//!   hash reaches [`MIN_DOCUMENTS`] are counted again on a second reading by their text,
//!   and only that count decides what is chrome. Two lines sharing a hash can make a
//!   candidate of a line that is not chrome, never chrome of one that is not.
//! - **The edges of a document, not all of it.** Each document gives at most
//!   [`EDGE_LINES`] lines from its top and as many from its bottom, which is where menus,
//!   banners and footers sit. A document of up to twice that is counted whole, so a
//!   sidebar in the middle of a page is caught exactly as before.
//!
//! Together with [`LEARN_SAMPLE`] the first reading holds at most a million hashes for any
//! source, a few tens of megabytes, against the 1.6 GB publicrec reached. The second
//! reading keeps the text of lines whose hash was in five documents, so at most a fifth
//! of that many lines, each as long as it is. That is a bound on lines, not bytes, but it
//! only holds what genuinely repeats: on a website, the chrome itself. A data file's
//! repeated rows mostly sit in record paragraphs, which are never counted.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, RandomState};

/// How many of a source's documents a line must appear in before it is chrome.
///
/// Five, chosen from the corpus rather than assumed. At **two**, hillsclerk.com's
/// *"Thanks! Your marriage license application has been successfully submitted to the
/// Clerk!"* appears on two pages — it is the entire content of one of them — and would be
/// deleted. At **ten**, nothing is caught at all: the menu that ruined seven pages appears
/// on exactly those seven, because the reader removed it everywhere it worked. The
/// distribution in between is empty, so the choice is wide rather than delicate.
pub const MIN_DOCUMENTS: usize = 5;

/// Shorter lines are structure, not chrome.
///
/// `*` and `Choose` cost two characters to keep across fifty documents, and removing them
/// can leave a list or a table rule that no longer parses.
pub const MIN_LINE_CHARS: usize = 12;

/// Documents read before the pattern is taken as known.
///
/// Chrome is chrome; a five-hundred document sample settles it, and a corpus of a hundred
/// thousand pages should not be held in memory to learn what the first few hundred say.
pub const LEARN_SAMPLE: usize = 500;

/// Lines counted from each end of a document.
///
/// Set well above any page, so a page is always counted whole and a sidebar in its middle
/// is caught. In a development store of 239 derived texts, the median has 26 lines long
/// enough to count and council meeting transcripts reach about a thousand. Past two
/// thousand there were only caption files, tens of thousands of lines each, and a data
/// file is longer still; whatever chrome those carry, a running header or a footer, sits
/// at their ends.
pub const EDGE_LINES: usize = 1_000;

/// The chrome of one source.
#[derive(Clone, Debug, Default)]
pub struct Boilerplate {
    lines: HashSet<String>,
}

/// Accumulates line counts without holding the documents.
///
/// It reads the sample once, or twice when a line repeated: see the module notes for why.
/// Each reading is ended with [`Learner::finish`].
#[derive(Debug)]
pub struct Learner {
    /// One seed for both readings, so the second finds the hashes the first counted.
    hasher: RandomState,
    /// Documents in this reading.
    read: usize,
    reading: Reading,
}

#[derive(Debug)]
enum Reading {
    /// Every counted line, by hash: how many documents it was in.
    Counting(HashMap<u64, u32>),
    /// The lines whose hash reached the floor, by their text.
    Confirming {
        repeated: HashSet<u64>,
        lines: HashMap<String, usize>,
    },
}

/// What a finished reading of the sample gives.
#[derive(Debug)]
pub enum Learned {
    Chrome(Boilerplate),
    /// Some lines repeated. Read the same documents into this learner, then finish it.
    ReadAgain(Learner),
}

impl Default for Learner {
    fn default() -> Self {
        Self {
            hasher: RandomState::new(),
            read: 0,
            reading: Reading::Counting(HashMap::new()),
        }
    }
}

impl Learner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Counts each distinct line **once**, however often the document repeats it. A page
    /// that lists the same disclaimer twenty times is one page, not twenty.
    pub fn add(&mut self, text: &str) {
        if self.read >= LEARN_SAMPLE {
            return;
        }
        self.read += 1;
        match &mut self.reading {
            Reading::Counting(seen) => {
                for line in counted_lines(text) {
                    *seen.entry(self.hasher.hash_one(line)).or_insert(0) += 1;
                }
            }
            Reading::Confirming { repeated, lines } => {
                for line in counted_lines(text) {
                    if repeated.contains(&self.hasher.hash_one(line)) {
                        *lines.entry(line.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    /// What this reading is doing, for progress: the sample is read once to count and,
    /// when a line repeated, again to confirm.
    pub fn reading(&self) -> &'static str {
        match self.reading {
            Reading::Counting(_) => "reading",
            Reading::Confirming { .. } => "confirming",
        }
    }

    pub fn finish(self) -> Learned {
        match self.reading {
            Reading::Counting(seen) => {
                // Below the floor, "it appears in every document" is a statement about two
                // documents, and two documents agreeing is a coincidence rather than a
                // pattern.
                if self.read < MIN_DOCUMENTS {
                    return Learned::Chrome(Boilerplate::default());
                }
                let repeated: HashSet<u64> = seen
                    .into_iter()
                    .filter(|(_, documents)| *documents as usize >= MIN_DOCUMENTS)
                    .map(|(hash, _)| hash)
                    .collect();
                // A source of data is usually done here, having repeated nothing, and is
                // spared the second reading.
                if repeated.is_empty() {
                    return Learned::Chrome(Boilerplate::default());
                }
                Learned::ReadAgain(Learner {
                    hasher: self.hasher,
                    read: 0,
                    reading: Reading::Confirming {
                        repeated,
                        lines: HashMap::new(),
                    },
                })
            }
            Reading::Confirming { lines, .. } => Learned::Chrome(Boilerplate {
                lines: lines
                    .into_iter()
                    .filter(|(_, seen)| *seen >= MIN_DOCUMENTS)
                    .map(|(line, _)| line)
                    .collect(),
            }),
        }
    }
}

impl Boilerplate {
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Removes the chrome, keeping every offset traceable to the document it came from.
    pub fn strip(&self, text: &str) -> Stripped {
        if self.lines.is_empty() {
            return Stripped {
                text: text.to_string(),
                marks: Vec::new(),
                lines_dropped: 0,
                chars_dropped: 0,
            };
        }

        let mut out = String::with_capacity(text.len());
        let mut marks: Vec<(usize, usize)> = Vec::new();
        let mut origin = 0usize;
        let mut kept = 0usize;
        let mut lines_dropped = 0usize;
        let mut chars_dropped = 0usize;

        // `split_inclusive` on both levels keeps every newline where it was, so blank
        // lines and paragraph seams survive and the offsets below stay exact.
        for para in text.split_inclusive("\n\n") {
            let records = crate::chunk::holds_records(para);
            for line in para.split_inclusive('\n') {
                let chars = line.chars().count();
                if !records && self.lines.contains(line.trim()) {
                    lines_dropped += 1;
                    chars_dropped += chars;
                } else {
                    // A mark only where the gap changed — one per run of kept lines.
                    if marks.last().map(|(k, o)| o - k) != Some(origin - kept) {
                        marks.push((kept, origin));
                    }
                    out.push_str(line);
                    kept += chars;
                }
                origin += chars;
            }
        }

        Stripped {
            text: out,
            marks,
            lines_dropped,
            chars_dropped,
        }
    }
}

/// The text of one document with its chrome removed.
#[derive(Clone, Debug)]
pub struct Stripped {
    pub text: String,
    /// `(offset in `text`, the same position in the document)`, ascending. Sparse: one
    /// entry per run of kept lines.
    marks: Vec<(usize, usize)>,
    pub lines_dropped: usize,
    pub chars_dropped: usize,
}

impl Stripped {
    /// Where `offset` in [`Self::text`] sits in the document it was stripped from.
    ///
    /// A chunk's span is recorded in the document's own coordinates, not the stripped
    /// text's, because the stripped text is never written anywhere — a span measured
    /// against it would point into something nobody can open.
    pub fn origin(&self, offset: usize) -> usize {
        match self.marks.binary_search_by_key(&offset, |(kept, _)| *kept) {
            Ok(i) => self.marks[i].1,
            // `i` is where it *would* insert, so the run it belongs to is the one before.
            Err(0) => offset,
            Err(i) => {
                let (kept, origin) = self.marks[i - 1];
                origin + (offset - kept)
            }
        }
    }

    pub fn dropped_anything(&self) -> bool {
        self.lines_dropped > 0
    }
}

/// Lines eligible to be counted as chrome: long enough to matter, and outside records.
fn prose_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split("\n\n")
        .filter(|para| !crate::chunk::holds_records(para))
        .flat_map(|para| para.lines())
        .map(str::trim)
        .filter(|line| line.chars().count() >= MIN_LINE_CHARS)
}

/// The lines one document is counted by: its distinct prose lines, from each end.
///
/// Both readings go through here, so the second counts by text exactly the lines the first
/// counted by hash.
fn counted_lines(text: &str) -> HashSet<&str> {
    let lines: Vec<&str> = prose_lines(text).collect();
    let edge = lines.len().min(EDGE_LINES);
    lines[..edge]
        .iter()
        .chain(&lines[lines.len() - edge..])
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seven pages of menu, one page of content, in the shape hillsclerk.com produced.
    fn menu() -> String {
        (0..12)
            .map(|i| format!("*   [Service {i}](https://x.gov/services#{i} \"Service {i}\")\n"))
            .collect()
    }

    fn pages(n: usize) -> Vec<String> {
        (0..n)
            .map(|i| format!("# Page {i} - The Clerk\n\n{}\nSomething only page {i} says, at length enough to count.\n", menu()))
            .collect()
    }

    fn learn(docs: &[String]) -> Boilerplate {
        learn_holding(docs).0
    }

    /// Learns the way `index` does, reading the sample as often as the learner asks, and
    /// reports the most entries it held at the end of any reading.
    fn learn_holding(docs: &[String]) -> (Boilerplate, usize) {
        let mut l = Learner::new();
        let mut held = 0;
        loop {
            for d in docs {
                l.add(d);
            }
            held = held.max(match &l.reading {
                Reading::Counting(seen) => seen.len(),
                Reading::Confirming { repeated, lines } => repeated.len() + lines.len(),
            });
            match l.finish() {
                Learned::Chrome(b) => return (b, held),
                Learned::ReadAgain(again) => l = again,
            }
        }
    }

    #[test]
    fn a_menu_on_every_page_is_chrome_and_the_content_is_not() {
        let docs = pages(7);
        let b = learn(&docs);
        assert_eq!(b.len(), 12, "every menu line, and nothing else");

        let s = b.strip(&docs[0]);
        assert!(!s.text.contains("Service 4"), "the menu survived");
        assert!(
            s.text.contains("Something only page 0 says"),
            "the content was taken with it"
        );
        assert_eq!(s.lines_dropped, 12);
    }

    #[test]
    fn four_documents_are_not_enough_to_call_anything_chrome() {
        let b = learn(&pages(4));
        assert!(b.is_empty(), "four agreeing documents is a coincidence");
    }

    /// Fault 11. Every row of a data file is distinct, so the first learner held the whole
    /// source and `index` was killed on publicrec. What it holds now follows how many
    /// documents it read, not how long they are, and the lines at their edges still count.
    #[test]
    fn what_the_learner_holds_does_not_grow_with_the_documents() {
        let documents = MIN_DOCUMENTS + 5;
        let files = |rows: usize| -> Vec<String> {
            (0..documents)
                .map(|d| {
                    let body: String = (0..rows)
                        .map(|r| format!("Filing {d}-{r} entered on the docket\n"))
                        .collect();
                    format!("Clerk of the Circuit Court public data\n{body}Generated nightly by the records system\n")
                })
                .collect()
        };

        let long_files = files(40 * EDGE_LINES);
        let (short, short_held) = learn_holding(&files(4 * EDGE_LINES));
        let (long, long_held) = learn_holding(&long_files);
        assert_eq!(short_held, long_held, "ten times the rows took more memory");
        assert!(
            long_held <= documents * 2 * EDGE_LINES,
            "held {long_held} entries for {documents} documents"
        );

        // The header and the footer are at the edges, and are what repeats.
        assert_eq!(short.len(), 2, "{:?}", short.lines);
        assert_eq!(long.len(), 2, "{:?}", long.lines);
        let s = long.strip(&long_files[0]);
        assert_eq!(s.lines_dropped, 2);
    }

    /// Seven pages with the menu between `around` lines of prose above and below it.
    fn menu_amid(around: usize) -> Vec<String> {
        (0..7)
            .map(|i| {
                let prose = |part: &str| -> String {
                    (0..around)
                        .map(|j| {
                            format!("Page {i} {part} paragraph {j} says something only it says.\n")
                        })
                        .collect()
                };
                format!(
                    "# Page {i}\n\n{}\n{}\n{}",
                    prose("opening"),
                    menu(),
                    prose("closing")
                )
            })
            .collect()
    }

    /// Counting the edges must not lose what counting the whole page caught. A page of
    /// exactly twice the edge, with its menu across the seam, is covered only by both edges
    /// together: the menu's first six lines by the top, its last six by the bottom.
    #[test]
    fn a_menu_in_the_middle_of_a_long_page_is_still_chrome() {
        let docs = menu_amid(EDGE_LINES - 6);

        let b = learn(&docs);
        assert_eq!(b.len(), 12, "every menu line, and nothing else");
        let s = b.strip(&docs[0]);
        assert!(!s.text.contains("Service 4"), "the menu survived");
        assert!(
            s.text.contains("closing paragraph 0 says"),
            "the content after it was taken too"
        );
    }

    /// The price of the bound, pinned. A line beyond both edges is never counted, so a menu
    /// in the middle of a document longer than twice the edge is not learned, however many
    /// documents carry it.
    #[test]
    fn a_line_past_both_edges_is_not_learned() {
        assert!(learn(&menu_amid(EDGE_LINES)).is_empty());
    }

    /// The publicrec case. A balance that does not move for five days repeats, and so
    /// does the column header of thirty daily filings. Both are records.
    #[test]
    fn repetition_inside_a_record_set_is_data() {
        let header = "CaseCategory,CaseNumber,Title,FilingDate,PartyType";
        let row = "\"CV\",\"26-CA-1\",\"Smith vs Jones\",\"07/19/2026\",\"Plaintiff\"";
        let docs: Vec<String> = (0..8)
            .map(|i| format!("{header}\n{row}\n{row}\n\"CV\",\"26-CA-{i}\",\"A vs B\",\"07/1{i}/2026\",\"Plaintiff\"\n"))
            .collect();

        let b = learn(&docs);
        assert!(
            b.is_empty(),
            "a record set taught the learner chrome: {:?}",
            b.lines
        );
        let s = b.strip(&docs[0]);
        assert!(s.text.contains(header), "the column names were stripped");
        assert_eq!(s.chars_dropped, 0);
    }

    /// The whole point of the sparse mark list: a hit must still be findable in the
    /// document, which is the only copy anyone can open.
    #[test]
    fn offsets_still_point_into_the_document_they_came_from() {
        let docs = pages(7);
        let b = learn(&docs);
        let s = b.strip(&docs[0]);

        let needle = "Something only page 0 says";
        let at = s.text.find(needle).expect("content survived");
        let at = s.text[..at].chars().count();

        let origin = s.origin(at);
        let doc: Vec<char> = docs[0].chars().collect();
        let there: String = doc[origin..origin + needle.chars().count()]
            .iter()
            .collect();
        assert_eq!(there, needle, "the offset pointed at the wrong place");
    }

    #[test]
    fn nothing_learned_means_nothing_touched() {
        let b = Boilerplate::default();
        let doc = "# A page\n\nWith some text on it that nothing else shares.\n";
        let s = b.strip(doc);
        assert_eq!(s.text, doc);
        assert_eq!(s.origin(17), 17);
        assert!(!s.dropped_anything());
    }
}
