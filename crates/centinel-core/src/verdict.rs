//! What we think of a read.
//!
//! Every tool here reported `hillsclerk.com/marriage-license-application-success-kiosk` as
//! a success: HTTP 200, `html`, a title, 23,213 characters. The page's content is one
//! sentence; the rest is the site menu. `check` said nothing, `investigate` said nothing,
//! and `run` counted it toward the corpus.
//!
//! That silence had two causes. `investigate` only measured a seed when **nothing**
//! recognised it, on the assumption that a recognised site needs no explaining — and this
//! site was recognised, cleanly, by `sitemap`. And even ungated, none of the measures
//! looked at the failure: the page passes characters-per-KB, passes the script share, and
//! declares a sitemap.
//!
//! So recognition and read quality are separate questions, and only the first had an
//! answer. This is the second one. It asks nothing about the site and everything about the
//! text that came out.
//!
//! ## Characters per KB is measured and never judged
//!
//! `docs/STRATEGIES.md` §17 proposed it as the test for a bad read, and it does not work.
//! Run against the fifty hillsclerk documents it fires on **forty-two, of which forty-one
//! are good pages** — and the sign is backwards: the seven ruined reads sit between 111.8
//! and 117.0 characters per KB, and the healthy ones between 2.4 and 2.7.
//!
//! The reason is that a modern template weighs 200 KB whatever it holds, so the ratio
//! measures the CMS and not the read. It only ever looked useful because the two points
//! that set it — an OnBase search page at 7.6 and a bare IIS directory listing at ~980 —
//! are different kinds of document, not a good read and a bad one.
//!
//! What it was meant to catch has a better test elsewhere. A page whose content is built
//! at run time is caught by the **script share** in `investigate`, on the same OnBase
//! evidence: 87.5 KB of script in a 93.8 KB page. So the number is still reported here,
//! because it costs nothing and someone may yet find the shape it belongs to, and it
//! raises nothing.
//!
//! ## Link share counts the address as well as the anchor text
//!
//! `links_in` measures a whole `[text](url)` span, URL included, and that is worth knowing
//! before reading a figure from it:
//!
//! ```text
//! [Google Calendar](https://www.google.com/calendar/event?action=TEMPLATE&dates=…)
//!  ^^^^^^^^^^^^^^^ 15 characters of anchor text     ^^^^^^^^^^ 250 characters of address
//! ```
//!
//! On `medinaco.org` that put the figure at 71% — of which 10% was anchor text and 59% was
//! addresses — and it flagged 50 documents of 50, wrongly. The fix was not to reweigh this:
//! it was to stop putting addresses in the corpus at all.
//! [`crate::extract::Reader::Marked`] reduces an `<a>` inside a marked region to its text,
//! so those documents now measure what the words say rather than how long the URLs are.
//!
//! The judgement is kept, because the case it was validated on is still reachable. A page
//! with no content marker falls to the whole-page reader, which keeps its links, and that
//! is precisely the read this was measured against — 7 flagged, 0 false positives over 100
//! documents.
//!
//! **But say plainly what that leaves.** A document read from a marked region has no
//! markdown links at all — measured, 0 across 213 documents from three sites — so
//! [`ReadQuality::link_share`] is structurally zero there and this finding can never fire on one.
//! The menu question has not been answered for those documents; it has stopped being
//! askable in these terms, because the addresses that made a menu legible as a menu are no
//! longer in the text. A marked region that is genuinely navigation now reads as a short
//! list of words, and nothing here objects to it.
//!
//! Two things still do, and they are counts rather than judgements: [`crate::boilerplate`]
//! removes a line repeated across the source, and `ops::build_index` reports a document that
//! produced no chunk. Both are reported per run. If a marked region full of navigation turns
//! out to be a real shape in the field, it will need its own measure and that measure will
//! need validating — this one cannot be stretched to cover it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Above this share of link text, a reader returned a menu rather than a page.
///
/// Half, and the margin is nowhere near it. On `hillsclerk.com` the seven documents whose
/// reader failed sit between 82.9% and 85.1%; the worst of the forty-three that worked is
/// 49.2%, and the median is far below that.
pub const LINK_SHARE: f64 = 0.5;

/// A link longer than this is a runaway scan, not a link.
///
/// Bounds the search so a stray `[` in a large document cannot cost a pass over the rest
/// of it. No real markdown link comes close.
const MAX_LINK_CHARS: usize = 2048;

/// What the text being measured **is**, which decides which findings can be asked of it.
///
/// An argument rather than something a caller edits out afterwards. `investigate` used to
/// take the full judgement and then clear `findings` when the seed was a directory index —
/// which suppressed the printed line and also wrote a softened verdict into `--json`, where
/// a reader saw no findings beside a `link_share` of 0.62. A question that does not apply
/// should not be asked; it should not be asked and then unasked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    /// Prose or markup the corpus will hold. Every finding applies.
    Document,
    /// An index whose **links** are the documents — a directory listing, a paged result.
    /// It is a page of links on purpose: `publicrec.hillsclerk.com/Civil/` reads as 62%
    /// link text and is working perfectly. Its text is not the corpus; the files it names
    /// are. Numbers only.
    Index,
    /// Bytes that were never markup — a PDF, a spreadsheet, a caption track. *Nothing was
    /// read* still means something here and is still asked. *This is a menu* does not, and
    /// neither does characters-per-KB, whose denominator is fonts and images.
    NotMarkup,
}

impl Read {
    /// What the content kind alone can say.
    ///
    /// Markup and plain text are documents; everything else was never markup, so the menu
    /// question cannot be asked of it. A caller that knows more — that this particular page
    /// is an index — says so instead of asking here.
    pub fn of(kind: crate::content::ContentKind) -> Self {
        use crate::content::ContentKind::*;
        match kind {
            Html | Markdown | Text => Self::Document,
            _ => Self::NotMarkup,
        }
    }
}

/// What a read produced, measured against what it was given.
///
/// Named for the question rather than for the answer, because [`crate::op::Verdict`] is a
/// different type with the same old name in the same crate — so this one could only be
/// spelled fully qualified in the one file that used both.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReadQuality {
    pub chars: usize,
    pub bytes: usize,
    /// Characters of text per KB of source. Meaningful for HTML, where the bytes are
    /// markup; not for a PDF, whose size is mostly fonts and images.
    pub chars_per_kb: f64,
    pub links: usize,
    /// Share of the text that sits inside a markdown link, `0.0`–`1.0`.
    pub link_share: f64,
    /// What is wrong with it, in words. Empty means the read looks ordinary — or that
    /// nothing was judged, which is what [`Self::measure`] alone produces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<String>,
}

impl ReadQuality {
    /// The numbers, and no opinion about them.
    ///
    /// What a *decision* wants: [`crate::extract`] picks between two readers on
    /// `link_share` and has no use for a sentence about a menu.
    pub fn measure(bytes: &[u8], derived: &str) -> Self {
        let chars = derived.chars().count();
        let kb = (bytes.len() as f64 / 1024.0).max(1.0 / 1024.0);
        let (link_chars, links) = links_in(derived);
        Self {
            chars,
            bytes: bytes.len(),
            chars_per_kb: chars as f64 / kb,
            links,
            link_share: match chars {
                0 => 0.0,
                n => link_chars as f64 / n as f64,
            },
            findings: Vec::new(),
        }
    }

    /// The numbers, plus what is worth saying about them for this kind of text.
    pub fn judged(mut self, read: Read) -> Self {
        if self.chars == 0 {
            self.findings.push(format!(
                "nothing was read — {} in, no text out",
                crate::render::bytes(self.bytes as u64)
            ));
        }

        // The menu case. Measured on the output, so it holds for any reader: a passage
        // that is mostly link text is a navigation tree, whatever produced it. Asked only
        // of text that is meant to *be* the document, and only where links were ever a
        // thing the bytes could contain.
        if read == Read::Document && self.link_share > LINK_SHARE {
            self.findings.push(format!(
                "{:.0}% of the text is link text — {} links in {} chars. \
                 This is a menu, not a page.",
                self.link_share * 100.0,
                self.links,
                self.chars
            ));
        }

        self
    }

    /// Both at once, for the callers that always want the judgement.
    pub fn on(bytes: &[u8], derived: &str, read: Read) -> Self {
        Self::measure(bytes, derived).judged(read)
    }

    /// Whether this read is worth someone's attention.
    pub fn is_poor(&self) -> bool {
        !self.findings.is_empty()
    }
}

/// Characters inside markdown links, and how many there are.
///
/// Deliberately the simple, non-nested reading — `[text](url)` where the text holds no
/// `]`. An image inside a link, `[![alt](img)](url)`, counts the inner one, which is the
/// same answer the analysis that set [`LINK_SHARE`] arrived at.
fn links_in(md: &str) -> (usize, usize) {
    let c: Vec<char> = md.chars().collect();
    let mut i = 0;
    let mut chars = 0;
    let mut count = 0;

    while i < c.len() {
        if c[i] != '[' {
            i += 1;
            continue;
        }
        let limit = (i + MAX_LINK_CHARS).min(c.len());
        let Some(close) = (i + 1..limit).find(|&j| c[j] == ']') else {
            i += 1;
            continue;
        };
        if close + 1 >= c.len() || c[close + 1] != '(' {
            i += 1;
            continue;
        }
        let Some(end) = (close + 2..limit).find(|&j| c[j] == ')') else {
            i += 1;
            continue;
        };
        chars += end - i + 1;
        count += 1;
        i = end + 1;
    }

    (chars, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(items: usize) -> String {
        (0..items)
            .map(|i| format!("*   [Service {i}](https://x.gov/services#{i} \"Service {i}\")\n"))
            .collect()
    }

    #[test]
    fn a_page_that_is_mostly_menu_is_called_one() {
        let text = format!(
            "# Thanks\n\n{}\n**Your application was submitted.**",
            menu(40)
        );
        let v = ReadQuality::on(&vec![0u8; 200_000], &text, Read::Document);
        assert!(v.link_share > LINK_SHARE, "share was {:.2}", v.link_share);
        assert!(v.is_poor());
        assert!(
            v.findings.iter().any(|f| f.contains("This is a menu")),
            "{:?}",
            v.findings
        );
    }

    /// Prose with ordinary references must not be called a menu, or the measure fires on
    /// every page with a footer and stops meaning anything.
    #[test]
    fn prose_with_a_few_links_is_not_a_menu() {
        let text = format!(
            "{} See [the form](https://x.gov/f) and [the fee schedule](https://x.gov/s).",
            "The clerk records deeds and judgments for the county. ".repeat(40)
        );
        let v = ReadQuality::on(&vec![0u8; 4_000], &text, Read::Document);
        assert!(v.link_share < LINK_SHARE, "share was {:.2}", v.link_share);
        assert!(!v.is_poor(), "{:?}", v.findings);
    }

    /// The measure that had to be withdrawn, kept as a test so it cannot come back.
    ///
    /// Both of these are genuinely low on characters per KB and neither is a bad read:
    /// `meet-clerk-crist` is 2,495 characters of biography inside a 205 KiB template, and
    /// `RegistryReadme.pdf` is 2,058 correctly-read characters in 122 KiB of PDF. Across
    /// the fifty hillsclerk documents the old test fired forty-two times and was right
    /// once, so the ratio is reported and never judged.
    #[test]
    fn a_low_character_ratio_is_not_a_finding() {
        for (bytes, chars) in [(205_000, 2_495), (124_680, 2_058), (94_125, 695)] {
            let v = ReadQuality::on(&vec![0u8; bytes], &"x ".repeat(chars / 2), Read::Document);
            assert!(v.chars_per_kb < 60.0, "the ratio is genuinely low");
            assert!(
                !v.is_poor(),
                "{bytes} bytes / {chars} chars raised {:?}",
                v.findings
            );
        }
    }

    #[test]
    fn a_read_that_produced_nothing_says_so() {
        let v = ReadQuality::on(&vec![0u8; 50_000], "", Read::Document);
        assert!(v.findings.iter().any(|f| f.contains("nothing was read")));
        assert_eq!(v.link_share, 0.0, "no division by zero");
    }

    #[test]
    fn an_unclosed_bracket_does_not_run_away() {
        let text = format!("[{}", "x".repeat(10_000));
        let v = ReadQuality::on(text.as_bytes(), &text, Read::Document);
        assert_eq!(v.links, 0);
    }

    /// A directory index **is** a page of links, so the menu finding is not asked of it —
    /// and the numbers still stand, because withdrawing the question is not the same as
    /// claiming the read was ordinary. `investigate` used to clear `findings` after the
    /// fact, which suppressed the line and left `--json` saying the second thing.
    #[test]
    fn an_index_keeps_its_numbers_and_is_not_judged_a_menu() {
        let text = "[One](/1) [Two](/2) [Three](/3) [Four](/4)".repeat(20);
        let judged = ReadQuality::on(text.as_bytes(), &text, Read::Index);

        assert!(judged.link_share > LINK_SHARE, "the measure still ran");
        assert!(judged.links > 0);
        assert!(
            judged.findings.is_empty(),
            "an index was called a menu: {:?}",
            judged.findings
        );
        assert!(!judged.is_poor());
    }

    /// The same text, when it is supposed to *be* the document, is still a menu.
    #[test]
    fn the_same_text_as_a_document_is_still_a_menu() {
        let text = "[One](/1) [Two](/2) [Three](/3) [Four](/4)".repeat(20);
        let judged = ReadQuality::on(text.as_bytes(), &text, Read::Document);
        assert!(
            judged.findings.iter().any(|f| f.contains("menu")),
            "{judged:?}"
        );
    }

    /// A PDF's size is fonts and images, and its text has no markdown links in it — so the
    /// menu question is meaningless rather than merely unlikely. An empty read is not.
    #[test]
    fn bytes_that_were_never_markup_are_asked_only_whether_anything_came_out() {
        let text = "[a](/1)".repeat(200);
        let judged = ReadQuality::on(text.as_bytes(), &text, Read::NotMarkup);
        assert!(judged.findings.is_empty(), "{:?}", judged.findings);

        let empty = ReadQuality::on(&vec![0u8; 50_000], "", Read::NotMarkup);
        assert!(
            empty
                .findings
                .iter()
                .any(|f| f.contains("nothing was read"))
        );
    }

    /// The kind alone decides, where a caller knows nothing more.
    #[test]
    fn a_content_kind_answers_for_itself() {
        use crate::content::ContentKind;
        assert_eq!(Read::of(ContentKind::Html), Read::Document);
        assert_eq!(Read::of(ContentKind::Pdf), Read::NotMarkup);
        assert_eq!(Read::of(ContentKind::Spreadsheet), Read::NotMarkup);
        assert_eq!(Read::of(ContentKind::Captions), Read::NotMarkup);
    }

    /// Measuring is not judging, and a decision wants only the first. `extract` picks
    /// between two readers on `link_share` and must not pay for a sentence nobody reads.
    #[test]
    fn measuring_alone_raises_nothing() {
        let v = ReadQuality::measure(&vec![0u8; 50_000], "");
        assert!(v.findings.is_empty());
        assert!(!v.is_poor(), "an unjudged read is not a poor one");
    }
}
