//! Scanning markup for the few things this codebase asks of it.
//!
//! A scan rather than a parse: every question here is local — *what does this `<meta>`
//! say*, *what does this `<a>` point at* — and the answer does not change with a malformed
//! table three elements up. A real parser is the right tool for reading a document, and
//! `dom_smoothie` is where that happens; this is for reading the markup *about* the
//! document.
//!
//! ## Why one module
//!
//! There were two of these, in `extract` and in `enclosure`, with the same function names
//! and the same doc comments — and they had drifted. `enclosure`'s [`Tag::attr`] checks
//! that the name it matched is a whole attribute rather than the tail of another, and has
//! a test proving `data` must not match inside `formdata`. `extract`'s copy had neither,
//! and it is the one reading `content=` off a `<meta>` tag — so `data-content="…"` on the
//! same element answered in `og:title`'s place. One copy carried the fix and the other
//! carried the bug, which is what two copies of a scanner are for.
//!
//! ## One lowercased copy
//!
//! `to_ascii_lowercase` touches only ASCII bytes, so byte offsets stay aligned with the
//! original and a match found in the lowercased copy slices the real casing out of the
//! real string. That is what makes the scan case-insensitive without a case-insensitive
//! search, and it costs one page-sized allocation — which is why [`Scan`] is a value the
//! caller holds rather than a set of free functions that each made their own. A `.gov`
//! page is around 91 KB and collect plus extract asked four separate questions of it.

/// One page, lowercased once, ready to be asked several questions.
pub struct Scan<'a> {
    html: &'a str,
    lower: String,
}

/// One `<a …>` and the markup it wraps, up to its `</a>`.
///
/// The text is what a person reads as the link, and the only place a page says *who* an
/// address belongs to: `<a href="mailto:jdoe@tampagov.net">Jane Doe</a>`.
pub struct Link<'s> {
    pub tag: Tag<'s>,
    /// The markup between the tag and its close, in its original casing.
    pub inner: &'s str,
}

/// One `<name …>`, as found.
pub struct Tag<'s> {
    /// Lowercased, because that is what a caller matches on.
    pub name: &'s str,
    /// The whole tag, in its original casing — attribute *values* are data.
    pub raw: &'s str,
    lower: &'s str,
}

impl<'a> Scan<'a> {
    pub fn new(html: &'a str) -> Self {
        Self {
            lower: html.to_ascii_lowercase(),
            html,
        }
    }

    /// Every `<name …>` whose name is wanted, in document order.
    pub fn tags(&self, want: &[&str]) -> Vec<Tag<'_>> {
        self.spans(want).into_iter().map(|(tag, _)| tag).collect()
    }

    /// Every `<a …>`, with what it wraps.
    ///
    /// A link ends at its `</a>`, or at the next `<a` when a page never closed it — a
    /// missing close must not turn one link's text into the rest of the page. Both are
    /// whole tag names: `</abbr>` inside a link is not its end.
    pub fn links(&self) -> Vec<Link<'_>> {
        self.spans(&["a"])
            .into_iter()
            .map(|(tag, close)| {
                let start = close + 1;
                let rest = &self.lower[start..];
                let end = [find_tag(rest, "</a"), find_tag(rest, "<a")]
                    .into_iter()
                    .flatten()
                    .min()
                    .map_or(self.html.len(), |i| start + i);
                Link {
                    tag,
                    inner: &self.html[start..end],
                }
            })
            .collect()
    }

    /// Each wanted tag, with the offset of the `>` that closes it.
    fn spans(&self, want: &[&str]) -> Vec<(Tag<'_>, usize)> {
        let mut out = Vec::new();
        let mut from = 0;

        while let Some(open) = self.lower[from..].find('<').map(|i| i + from) {
            let after = open + 1;
            let name_end = self.lower[after..]
                .find(|c: char| !c.is_ascii_alphanumeric())
                .map(|i| after + i)
                .unwrap_or(self.lower.len());
            let Some(close) = self.lower[open..].find('>').map(|i| i + open) else {
                break;
            };
            let name = &self.lower[after..name_end.min(close)];
            if want.contains(&name) {
                out.push((
                    Tag {
                        name,
                        raw: &self.html[open..close],
                        lower: &self.lower[open..close],
                    },
                    close,
                ));
            }
            from = close + 1;
        }
        out
    }

    /// The body of every `<script>` block, in its original casing.
    pub fn scripts(&self) -> Vec<&'a str> {
        let mut out = Vec::new();
        let mut from = 0;

        while let Some(open) = self.lower[from..].find("<script").map(|i| i + from) {
            let Some(body_start) = self.lower[open..].find('>').map(|i| open + i + 1) else {
                break;
            };
            let body_end = self.lower[body_start..]
                .find("</script")
                .map(|i| body_start + i)
                .unwrap_or(self.html.len());
            out.push(&self.html[body_start..body_end]);
            from = body_end;
        }
        out
    }

    /// The document's own name.
    ///
    /// `og:title` first, because a `<title>` is usually the page name plus the site name
    /// and only the first half is the document. Readability strips that suffix itself when
    /// it can match the `<h1>`; this runs where it could not, so it prefers the tag that
    /// never carries the suffix over guessing at a separator.
    pub fn title(&self) -> Option<String> {
        let raw = self.og_title().or_else(|| self.tag_title())?;
        let title = unescape(raw.trim());
        (!title.is_empty()).then_some(title)
    }

    fn og_title(&self) -> Option<&str> {
        self.tags(&["meta"])
            .into_iter()
            .find(|t| t.lower.contains("\"og:title\"") || t.lower.contains("'og:title'"))
            .and_then(|t| t.attr("content"))
    }

    fn tag_title(&self) -> Option<&'a str> {
        let open = self.lower.find("<title")?;
        let start = self.lower[open..].find('>').map(|i| open + i + 1)?;
        let end = self.lower[start..].find("</title").map(|i| i + start)?;
        Some(&self.html[start..end])
    }
}

impl<'s> Tag<'s> {
    /// The value of `name="…"`, single- or double-quoted.
    ///
    /// Tied to the page rather than to the tag, so a caller may drop the `Tag` and keep
    /// the value — which is what reading one attribute out of a list of tags looks like.
    pub fn attr(&self, name: &str) -> Option<&'s str> {
        let mut from = 0;
        while let Some(at) = self.lower[from..].find(name).map(|i| i + from) {
            // A whole attribute, not a suffix of another: `data` must not match inside
            // `formdata`, and `content` must not match inside `data-content`.
            let boundary = at == 0
                || !self.lower.as_bytes()[at - 1].is_ascii_alphanumeric()
                    && self.lower.as_bytes()[at - 1] != b'-';
            let rest = &self.raw[at + name.len()..];
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
}

/// Where `opening` — `<a` or `</a` — starts a tag of exactly that name in lowercased
/// markup: not `<abbr>`, `<area>` or `</abbr>`, which begin the same way.
fn find_tag(lower: &str, opening: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = lower[from..].find(opening).map(|i| i + from) {
        match lower.as_bytes().get(at + opening.len()) {
            Some(c) if c.is_ascii_whitespace() || matches!(c, b'>' | b'/') => return Some(at),
            _ => from = at + opening.len(),
        }
    }
    None
}

/// Tags that sit inside a run of text rather than break it: `O<span>'Neil</span>` is one
/// word on the page, and a space where the tag was would make it two.
const INLINE: &[&str] = &[
    "a", "abbr", "b", "bdi", "bdo", "cite", "code", "data", "dfn", "em", "font", "i", "kbd",
    "mark", "q", "s", "samp", "small", "span", "strong", "sub", "sup", "time", "u", "var",
];

/// What a person reads in a fragment of markup: tags dropped — an inline one joining the
/// text either side, any other breaking it — every character reference decoded, and runs
/// of whitespace one space.
pub fn text(fragment: &str) -> String {
    let mut out = String::with_capacity(fragment.len());
    let mut rest = fragment;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let tag = &rest[open + 1..];
        let Some(close) = tag.find('>') else {
            rest = "";
            break;
        };
        let name: String = tag[..close]
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if !INLINE.contains(&name.as_str()) {
            out.push(' ');
        }
        rest = &tag[close + 1..];
    }
    out.push_str(rest);
    html_escape::decode_html_entities(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// One quoted run in a fragment of script, and whether the script was building it.
pub struct Quoted<'a> {
    pub text: &'a str,
    /// A `+` sits immediately either side of this literal, so it is one **piece** of a
    /// string the browser assembles at run time — never a whole value.
    ///
    /// The distinction is what separates a viewer's configuration from a URL template:
    ///
    /// ```js
    /// var pdfURL = "https://www.tampa.gov/…/20220301_Irish.pdf"   // a whole address
    /// link.attr("href", "/Documents/DownloadFile/"
    ///     + encodeURIComponent(doc.UrlFriendlyName)
    ///     + ".pdf?documentType=" + doc.MeetingDocumentType)        // pieces, with holes
    /// ```
    ///
    /// A caller that resolves the second against the page's base gets a plausible-looking
    /// address that names no document — and on a host that answers HTTP 200 for one, that
    /// is an error page stored as content on every page collected.
    pub concatenated: bool,
}

/// Every single- or double-quoted run in a fragment of script.
pub fn quoted_strings(script: &str) -> Vec<Quoted<'_>> {
    let mut out = Vec::new();
    let bytes = script.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let quote = bytes[i];
        if quote == b'"' || quote == b'\'' {
            if let Some(end) = script[i + 1..].find(quote as char) {
                let close = i + 1 + end;
                out.push(Quoted {
                    text: &script[i + 1..close],
                    concatenated: joins_before(bytes, i) || joins_after(bytes, close + 1),
                });
                i = close + 1;
                continue;
            }
            break;
        }
        i += 1;
    }
    out
}

/// Whether the nearest non-space byte before `at` is a `+`.
///
/// Bytes rather than chars, and safe on UTF-8 by construction: a continuation byte is
/// neither a space nor a `+`, so the walk stops on it exactly as it would on any other
/// character.
fn joins_before(bytes: &[u8], at: usize) -> bool {
    bytes[..at].iter().rev().find(|b| !b.is_ascii_whitespace()) == Some(&b'+')
}

fn joins_after(bytes: &[u8], from: usize) -> bool {
    bytes
        .get(from..)
        .and_then(|rest| rest.iter().find(|b| !b.is_ascii_whitespace()))
        == Some(&b'+')
}

/// The entities that appear in real titles and real URLs.
///
/// The union of two lists that were kept separately, each with a doc comment justifying
/// its own subset. `&amp;` is the one that has to be here: a query string in an attribute
/// is escaped, and joining it unescaped yields a different address. The rest are what a
/// CMS puts in a `<title>`. Anything rarer is left alone rather than half-decoded.
pub fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&apos;", "'")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_reads_as_its_text() {
        let scan = Scan::new(
            r#"<a href="/council/viera"><span>Council&nbsp;Member</span>
                 Luis <b>Viera</b></a><abbr>x</abbr>"#,
        );
        let links = scan.links();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].tag.attr("href"), Some("/council/viera"));
        assert_eq!(text(links[0].inner), "Council Member Luis Viera");
    }

    /// `</abbr>` inside a link is not the link's end: the name after it is the point.
    #[test]
    fn a_link_ends_at_its_own_close_and_not_at_one_that_starts_the_same() {
        let scan = Scan::new(r#"<a href="/staff"><abbr>Dr.</abbr> Jane Doe</a><area>"#);
        assert_eq!(text(scan.links()[0].inner), "Dr. Jane Doe");
    }

    /// Inline markup inside a word leaves it one word, a block break makes two, and every
    /// character reference reads as the character it names.
    #[test]
    fn text_reads_as_the_page_renders_it() {
        assert_eq!(text("Mary O<span>'Neil</span>"), "Mary O'Neil");
        assert_eq!(text("Jos&#233; N&uacute;&ntilde;ez"), "José Núñez");
        assert_eq!(text("Jane<br>Doe"), "Jane Doe");
        assert_eq!(text("<b>Luis</b>&nbsp;Viera"), "Luis Viera");
    }

    /// A page that never closes a link must not hand that link the text of every link
    /// after it — and `<abbr>` is not the next link.
    #[test]
    fn an_unclosed_link_ends_where_the_next_one_opens() {
        let scan = Scan::new(r#"<a href="/a">Jane Doe <abbr>JD</abbr><a href="/b">Bob Roe</a>"#);
        let texts: Vec<String> = scan.links().iter().map(|a| text(a.inner)).collect();
        assert_eq!(texts, ["Jane Doe JD", "Bob Roe"]);
    }

    #[test]
    fn tags_are_found_whatever_their_casing() {
        let scan = Scan::new(r#"<A HREF="/a.pdf">x</A><a href="/b.pdf">y</a>"#);
        let found = scan.tags(&["a"]);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "a");
        // The value keeps the casing it was served with.
        assert_eq!(found[0].attr("href"), Some("/a.pdf"));
        assert_eq!(found[1].attr("href"), Some("/b.pdf"));
    }

    /// The bug the two copies disagreed about. `enclosure`'s scanner refused a suffix
    /// match; `extract`'s accepted one, and `extract`'s is the one reading `content=`.
    #[test]
    fn an_attribute_is_not_matched_inside_a_longer_one() {
        let scan = Scan::new(r#"<object formdata="/wrong.pdf" data="/right.pdf">"#);
        let tags = scan.tags(&["object"]);
        assert_eq!(tags[0].attr("data"), Some("/right.pdf"));

        // The same rule in the other direction: a prefix is not a match either.
        let scan = Scan::new(r#"<embed data-src="/x.pdf">"#);
        assert_eq!(scan.tags(&["embed"])[0].attr("src"), None);
    }

    /// The same bug, in the place it actually bit: a `<meta>` carrying both.
    #[test]
    fn a_meta_title_is_not_read_out_of_data_content() {
        let scan = Scan::new(
            r#"<html><head>
               <meta property="og:title" data-content="analytics junk" content="The Real Title">
               <title>Ignored</title></head></html>"#,
        );
        assert_eq!(scan.title().as_deref(), Some("The Real Title"));
    }

    #[test]
    fn the_title_tag_answers_when_there_is_no_og_title() {
        let scan = Scan::new("<html><head><title>Budget &amp; Finance</title></head></html>");
        assert_eq!(scan.title().as_deref(), Some("Budget & Finance"));
    }

    #[test]
    fn a_blank_title_is_no_title() {
        assert_eq!(Scan::new("<title>   </title>").title(), None);
        assert_eq!(Scan::new("<html></html>").title(), None);
    }

    /// The literals of a script, without their concatenation flags.
    fn literals(script: &str) -> Vec<&str> {
        quoted_strings(script).into_iter().map(|q| q.text).collect()
    }

    #[test]
    fn script_bodies_come_back_in_their_original_casing() {
        let scan = Scan::new(r#"<SCRIPT>var pdfURL = "/Docs/A.PDF";</SCRIPT>"#);
        let bodies = scan.scripts();
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].contains("/Docs/A.PDF"));
        assert_eq!(literals(bodies[0]), vec!["/Docs/A.PDF"]);
    }

    /// A whole value against a piece of one. Only the first is an address.
    #[test]
    fn a_literal_in_a_plus_expression_is_marked_as_assembled() {
        let found = quoted_strings(
            r#"var whole = "/a.pdf";
               var built = "/DownloadFile/" + name + ".pdf?documentType=" + type;"#,
        );
        let flags: Vec<_> = found.iter().map(|q| (q.text, q.concatenated)).collect();
        assert_eq!(
            flags,
            vec![
                ("/a.pdf", false),
                ("/DownloadFile/", true),
                (".pdf?documentType=", true),
            ]
        );
    }

    /// The `+` is found across a line break, because that is how the sighting was written.
    #[test]
    fn whitespace_does_not_hide_the_join() {
        let found = quoted_strings("f(\n  \"/x/\"\n  + a\n  + \".pdf?t=\"\n)");
        assert!(found.iter().all(|q| q.concatenated), "a join was missed");

        // And a comma is not a join: an argument list is not a concatenation.
        let found = quoted_strings(r#"f("/a.pdf", "/b.pdf")"#);
        assert!(found.iter().all(|q| !q.concatenated));
    }

    /// Non-ASCII either side of a literal must not be read as a join, and must not panic.
    #[test]
    fn a_multibyte_neighbour_is_not_a_join() {
        let found = quoted_strings("var s = \"café\"; var t = «\"/a.pdf\"»;");
        assert!(found.iter().all(|q| !q.concatenated));
    }

    /// One allocation per page, not one per question asked of it.
    #[test]
    fn one_scan_answers_several_questions() {
        let scan = Scan::new(
            r#"<html><head><meta property="og:title" content="Proclamation">
               </head><body><a href="/a.pdf">a</a>
               <script>var pdfURL = "/b.pdf";</script></body></html>"#,
        );
        assert_eq!(scan.title().as_deref(), Some("Proclamation"));
        assert_eq!(scan.tags(&["a"])[0].attr("href"), Some("/a.pdf"));
        assert_eq!(literals(scan.scripts()[0]), vec!["/b.pdf"]);
    }

    #[test]
    fn unescape_covers_both_lists_it_was_made_of() {
        assert_eq!(unescape("a &amp; b"), "a & b");
        assert_eq!(unescape("a &#38; b"), "a & b");
        assert_eq!(unescape("&lt;tag&gt;"), "<tag>");
        assert_eq!(unescape("&quot;quoted&quot;"), "\"quoted\"");
        assert_eq!(unescape("it&#039;s"), "it's");
        assert_eq!(unescape("it&apos;s"), "it's");
        // Anything rarer is left alone rather than half-decoded.
        assert_eq!(unescape("&nbsp;x"), "&nbsp;x");
    }

    #[test]
    fn an_unterminated_tag_does_not_hang_or_panic() {
        assert!(Scan::new("<a href=\"/a.pdf\"").tags(&["a"]).is_empty());
        assert!(Scan::new("<script>var x = \"").scripts().len() <= 1);
        assert_eq!(Scan::new("<title>no end").title(), None);
    }
}
