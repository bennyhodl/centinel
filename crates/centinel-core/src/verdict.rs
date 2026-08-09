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
//! [`links_in`] measures a whole `[text](url)` span, URL included, and that is worth knowing
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
//! [`crate::strategies::read::marked`] reduces an `<a>` inside a marked region to its text,
//! so those documents now measure what the words say rather than how long the URLs are.
//!
//! The judgement is kept, because the case it was validated on is still reachable. A page
//! with no content marker falls to the whole-page reader, which keeps its links, and that
//! is precisely the read this was measured against — 7 flagged, 0 false positives over 100
//! documents.

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

/// What a read produced, measured against what it was given.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Verdict {
    pub chars: usize,
    pub bytes: usize,
    /// Characters of text per KB of source. Meaningful for HTML, where the bytes are
    /// markup; not for a PDF, whose size is mostly fonts and images.
    pub chars_per_kb: f64,
    pub links: usize,
    /// Share of the text that sits inside a markdown link, `0.0`–`1.0`.
    pub link_share: f64,
    /// What is wrong with it, in words. Empty means the read looks ordinary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<String>,
}

impl Verdict {
    pub fn on(bytes: &[u8], derived: &str) -> Self {
        let chars = derived.chars().count();
        let kb = (bytes.len() as f64 / 1024.0).max(1.0 / 1024.0);
        let chars_per_kb = chars as f64 / kb;
        let (link_chars, links) = links_in(derived);
        let link_share = match chars {
            0 => 0.0,
            n => link_chars as f64 / n as f64,
        };

        let mut findings = Vec::new();

        if chars == 0 {
            findings.push(format!(
                "nothing was read — {} in, no text out",
                crate::render::bytes(bytes.len() as u64)
            ));
        }

        // The menu case. Measured on the output, so it holds for any reader: a passage
        // that is mostly link text is a navigation tree, whatever produced it.
        if link_share > LINK_SHARE {
            findings.push(format!(
                "{:.0}% of the text is link text — {links} links in {chars} chars. \
                 This is a menu, not a page.",
                link_share * 100.0
            ));
        }

        Self {
            chars,
            bytes: bytes.len(),
            chars_per_kb,
            links,
            link_share,
            findings,
        }
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
        let v = Verdict::on(&vec![0u8; 200_000], &text);
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
        let v = Verdict::on(&vec![0u8; 4_000], &text);
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
            let v = Verdict::on(&vec![0u8; bytes], &"x ".repeat(chars / 2));
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
        let v = Verdict::on(&vec![0u8; 50_000], "");
        assert!(v.findings.iter().any(|f| f.contains("nothing was read")));
        assert_eq!(v.link_share, 0.0, "no division by zero");
    }

    #[test]
    fn an_unclosed_bracket_does_not_run_away() {
        let text = format!("[{}", "x".repeat(10_000));
        let v = Verdict::on(text.as_bytes(), &text);
        assert_eq!(v.links, 0);
    }
}
