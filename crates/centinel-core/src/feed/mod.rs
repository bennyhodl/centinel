//! A **feed**: RSS, Atom, or an iCalendar file — a document that is a list of other
//! documents, and carries them.
//!
//! A feed is not only a set of links. An RSS item from Medium carries the whole article in
//! `content:encoded`; a Google Calendar `.ics` carries every event's description, place and
//! time. When the article pages answer 403 to everything but a browser, the feed is the
//! only place the text can be read from at all — measured on a Medium publication whose 33
//! sitemap addresses all refused while its feed served ten full bodies.
//!
//! ## One response, many entries, and nothing invented
//!
//! A feed is fetched once, and that response is stored as it came. Each entry is then
//! stored as **its own artifact**, at a sub-address of the feed (`…/feed#entry=<id>`),
//! holding the entry's own bytes **sliced out of that response** — never a page assembled
//! to look as though it were fetched from the article's address. The article address is
//! kept, as `link` on the entry's record and as a line of its text, so a search hit leads
//! to it; the provenance says, truthfully, that the words came from the feed.
//!
//! The entry's identity is what the publisher gave it: an RSS `guid`, an Atom `id`, an
//! iCalendar `UID` with its `RECURRENCE-ID`. A recurring event's moved instance is a
//! different entry from the series it belongs to, and has to be.
//!
//! ## A feed is a window
//!
//! An RSS feed lists the latest few entries, not the archive. Ten items is ten items, and
//! a report that read it as "the blog" would be wrong about everything older. Every count
//! taken here says it is a window.

mod ics;
mod rss;

use url::Url;

/// What a feed is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Rss,
    Atom,
    Calendar,
}

impl Format {
    /// What a whole feed is, by its root: `<rss>`, `<rdf:RDF>`, an Atom `<feed>`, or
    /// `BEGIN:VCALENDAR`.
    ///
    /// Root sniffing rather than the content-type, because the commonest feed on the web is
    /// served as `text/xml`, and a calendar as `text/plain` or `application/octet-stream`.
    /// An entry cut out of a feed is not a feed and does not answer here; it is recorded
    /// with its format's content-type instead (see [`Self::mime`]).
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        let head = head(bytes);
        if head.starts_with("BEGIN:VCALENDAR") {
            return Some(Self::Calendar);
        }
        rss::sniff(&head)
    }

    /// The content-type an entry cut from a feed of this format is recorded with.
    pub fn mime(self) -> &'static str {
        match self {
            Self::Rss => "application/rss+xml",
            Self::Atom => "application/atom+xml",
            Self::Calendar => "text/calendar",
        }
    }

    /// What one entry is called: `item`, `entry`, `event`.
    pub fn noun(self) -> &'static str {
        match self {
            Self::Rss => "item",
            Self::Atom => "entry",
            Self::Calendar => "event",
        }
    }

    /// Whether a feed in this format is a window onto the latest entries, rather than the
    /// whole set. True of RSS and Atom as published; a calendar file is conventionally
    /// the whole calendar.
    pub fn is_window(self) -> bool {
        !matches!(self, Self::Calendar)
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Rss => "RSS",
            Self::Atom => "Atom",
            Self::Calendar => "iCalendar",
        })
    }
}

/// How many entries one feed may contribute before the rest are counted and dropped.
///
/// A calendar of nine hundred events is ordinary; one of a million is a generator gone
/// wrong. The count is kept, so the cap never reads as a short feed.
pub const MAX_ENTRIES: usize = 10_000;

/// One entry, cut out of a feed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The fragment this entry is stored under: `entry=<id>`, percent-encoded where the id
    /// holds a character a fragment cannot.
    pub fragment: String,
    /// The entry's own bytes, as the feed served them: the `<item>…</item>` element or the
    /// `BEGIN:VEVENT … END:VEVENT` block.
    pub bytes: Vec<u8>,
    /// The address the entry is about — an article, an event page — when it names one.
    pub link: Option<String>,
    /// Its title, for a report.
    pub title: Option<String>,
}

/// A feed, split.
#[derive(Clone, Debug)]
pub struct Entries {
    pub format: Format,
    /// The feed's own name, when it gives one.
    pub title: Option<String>,
    pub entries: Vec<Entry>,
    /// Entries past [`MAX_ENTRIES`], counted.
    pub dropped: usize,
}

/// Every entry in a whole feed, each with the bytes it was served as.
///
/// An error for bytes that are not a feed, or that will not parse: the feed's own
/// response is stored either way, so nothing is lost by refusing to split it.
pub fn entries(bytes: &[u8]) -> anyhow::Result<Entries> {
    match Format::sniff(bytes) {
        Some(Format::Calendar) => ics::entries(bytes),
        Some(_) => rss::entries(bytes),
        None => anyhow::bail!("not RSS, Atom or iCalendar"),
    }
}

/// What a person reads of a feed or an entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Readable {
    pub title: Option<String>,
    /// Labelled facts, in order: `Published`, `When`, `Where`, `Link`.
    pub fields: Vec<(&'static str, String)>,
    pub body: Option<Body>,
    /// For a whole feed: one line per entry it lists.
    pub listed: Vec<String>,
}

/// An entry's body, as the feed wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    /// Markup, which extraction turns into markdown.
    Html(String),
    Text(String),
}

/// The readable parts of a whole feed or of one entry.
///
/// A whole feed reads as its name and a list of what it holds, deliberately without the
/// bodies: each body is its own artifact, and putting every article into the feed's text
/// too would make every search hit on one of them come back twice.
pub fn read(bytes: &[u8]) -> anyhow::Result<Readable> {
    match head(bytes).starts_with("BEGIN:") {
        true => ics::read(bytes),
        false => rss::read(bytes),
    }
}

/// The start of a document as text, past a byte-order mark and leading whitespace.
fn head(bytes: &[u8]) -> String {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]);
    head.trim_start_matches('\u{feff}').trim_start().to_string()
}

/// Text that may be markup — an RSS `description` and a Google Calendar event's
/// `DESCRIPTION` usually are.
fn body_from(text: String) -> Body {
    match text.contains('<') && text.contains('>') {
        true => Body::Html(text),
        false => Body::Text(text),
    }
}

/// A feed a page says it publishes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Published {
    pub url: Url,
    pub format: Format,
    /// How the page said so: `link rel=alternate`, `.ics link`, `Google Calendar embed`.
    pub how: &'static str,
}

/// The feeds a page declares.
///
/// Three declarations, each a standard or a product's documented form, and none guessed:
///
/// - `<link rel="alternate" type="application/rss+xml|atom+xml" href>` — feed autodiscovery,
///   the way every CMS announces its feed.
/// - a link to a `.ics` file, or a `webcal:` link — how a page offers a calendar to
///   subscribe to.
/// - a Google Calendar embed — `calendar.google.com/calendar/embed?src=<calendar id>` —
///   whose public iCalendar address Google documents as
///   `calendar/ical/<calendar id>/public/basic.ics`. The id is read from the embed; nothing
///   is invented, and a private calendar answers 404 to the address, which is reported as
///   what it is.
///
/// Resolution and the same-host question are the caller's. This only reads markup.
pub fn published(html: &str, base: &Url) -> Vec<Published> {
    let scan = crate::html::Scan::new(html);
    let mut out: Vec<Published> = Vec::new();
    let mut push = |url: Url, format: Format, how: &'static str| {
        if !out.iter().any(|p| p.url == url) {
            out.push(Published { url, format, how });
        }
    };

    for tag in scan.tags(&["link"]) {
        let rel = tag.attr("rel").unwrap_or_default().to_ascii_lowercase();
        if !rel.split_whitespace().any(|r| r == "alternate") {
            continue;
        }
        let format = match tag.attr("type").map(|t| t.trim().to_ascii_lowercase()) {
            Some(t) if t == "application/rss+xml" => Format::Rss,
            Some(t) if t == "application/atom+xml" => Format::Atom,
            Some(t) if t == "text/calendar" => Format::Calendar,
            _ => continue,
        };
        if let Some(url) = tag.attr("href").and_then(|h| join(base, h)) {
            push(url, format, "link rel=alternate");
        }
    }

    for tag in scan.tags(&["a"]) {
        let Some(href) = tag.attr("href") else {
            continue;
        };
        let href = href.trim();
        // `webcal:` is `https:` with a hint to open a calendar application.
        let webcal = href
            .as_bytes()
            .get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case(b"webcal:"));
        let href = match webcal {
            true => format!("https:{}", &href[7..]),
            false => href.to_string(),
        };
        if let Some(url) = join(base, &href)
            && url.path().to_ascii_lowercase().ends_with(".ics")
        {
            push(url, Format::Calendar, ".ics link");
        }
    }

    for tag in scan.tags(&["iframe"]) {
        let Some(url) = tag.attr("src").and_then(|s| join(base, s)) else {
            continue;
        };
        for ics in google_calendar_ics(&url) {
            push(ics, Format::Calendar, "Google Calendar embed");
        }
    }
    out
}

fn join(base: &Url, href: &str) -> Option<Url> {
    let mut url = base.join(&crate::html::unescape(href)).ok()?;
    url.set_fragment(None);
    matches!(url.scheme(), "http" | "https").then_some(url)
}

/// The public iCalendar address of every calendar a Google Calendar embed shows.
///
/// An embed may show several calendars, one `src=` each. Keyed on the product's own
/// address, as a strategy keys on a product: every organisation that embeds a Google
/// Calendar is read the same way.
fn google_calendar_ics(embed: &Url) -> Vec<Url> {
    if embed.host_str() != Some("calendar.google.com") || !embed.path().starts_with("/calendar/") {
        return Vec::new();
    }
    embed
        .query_pairs()
        .filter(|(k, _)| k == "src")
        .filter(|(_, id)| !id.trim().is_empty())
        .filter_map(|(_, id)| {
            let encoded: String =
                url::form_urlencoded::byte_serialize(id.trim().as_bytes()).collect();
            Url::parse(&format!(
                "https://calendar.google.com/calendar/ical/{encoded}/public/basic.ics"
            ))
            .ok()
        })
        .collect()
}

/// An id as a fragment value: what a fragment may hold stays, the rest is percent-encoded.
///
/// An RSS `guid` is usually an address (`https://medium.com/p/1a2b`) and a `UID` is often
/// `abc@google.com`; both survive readable. A space or a `#` would not, and is encoded.
fn fragment_value(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for b in id.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => out.push(b as char),
            b'-' | b'.' | b'_' | b'~' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+'
            | b',' | b';' | b'=' | b':' | b'@' | b'/' | b'?' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The fragment for an entry: its publisher-given id, or — for an entry that has none —
/// a digest of its own bytes, which is stable for as long as the entry is unchanged.
fn fragment_for(id: Option<&str>, bytes: &[u8]) -> String {
    match id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => format!("entry={}", fragment_value(id)),
        None => {
            let sha = crate::domain::BlobSha::from_bytes(bytes);
            format!("entry=sha256:{}", &sha.as_str()[..16])
        }
    }
}

/// Splits `entries` at [`MAX_ENTRIES`], counting what did not fit.
fn capped(mut entries: Vec<Entry>) -> (Vec<Entry>, usize) {
    let dropped = entries.len().saturating_sub(MAX_ENTRIES);
    entries.truncate(MAX_ENTRIES);
    (entries, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://www.pinellasdsa.org/calendar").unwrap()
    }

    /// The three declarations a page makes, each read as the feed it names.
    #[test]
    fn a_page_declares_its_feeds_three_ways() {
        let html = r#"<html><head>
            <link rel="alternate" type="application/rss+xml" title="Blog" href="/feed">
            <link rel="stylesheet" href="/s.css">
            </head><body>
            <a href="webcal://www.pinellasdsa.org/events.ics">Subscribe</a>
            <iframe src="https://calendar.google.com/calendar/embed?src=gqol8ujletle8qvphl2a7dvgjs%40group.calendar.google.com&amp;ctz=America%2FNew_York"></iframe>
            </body></html>"#;

        let got = published(html, &base());
        let urls: Vec<(&str, Format)> = got.iter().map(|p| (p.url.as_str(), p.format)).collect();
        assert_eq!(
            urls,
            [
                ("https://www.pinellasdsa.org/feed", Format::Rss),
                ("https://www.pinellasdsa.org/events.ics", Format::Calendar),
                (
                    "https://calendar.google.com/calendar/ical/gqol8ujletle8qvphl2a7dvgjs%40group.calendar.google.com/public/basic.ics",
                    Format::Calendar
                ),
            ]
        );
    }

    /// Nothing is guessed: a page that declares no feed has none, however blog-like it is.
    #[test]
    fn a_page_that_declares_nothing_publishes_nothing() {
        let html = r#"<a href="/blog">Blog</a><a href="/feed-the-hungry">Food drive</a>
            <iframe src="https://www.youtube.com/embed/x"></iframe>"#;
        assert!(published(html, &base()).is_empty());
    }

    #[test]
    fn an_id_survives_as_a_readable_fragment() {
        assert_eq!(
            fragment_for(Some("https://medium.com/p/1a2b"), b""),
            "entry=https://medium.com/p/1a2b"
        );
        assert_eq!(fragment_for(Some("a b#c"), b""), "entry=a%20b%23c");
        assert!(fragment_for(None, b"<item/>").starts_with("entry=sha256:"));
    }
}
