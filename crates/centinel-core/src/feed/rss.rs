//! RSS 2.0, RSS 1.0 (RDF) and Atom: the XML feeds.
//!
//! One file for the three because they differ in element names and nothing else this
//! module asks of them. Elements are matched on their local name, as the sitemap parser
//! does: `content:encoded` is `encoded`, and a feed that forgot a namespace is still read.

use super::{Body, Entries, Entry, Format, Readable, body_from, capped, fragment_for};

/// Lines a whole feed's text lists before it stops. The entries are each stored on their
/// own; the list is a table of contents, not the content.
const MAX_LISTED: usize = 500;

/// The format a whole XML feed's root names, when it names one.
///
/// Atom's `<feed>` is a common word, so it is only Atom when the Atom namespace is in the
/// head. RSS's `<rss>` and RSS 1.0's `<rdf:RDF>` are not.
pub(super) fn sniff(head: &str) -> Option<Format> {
    match root_name(head)? {
        "rss" => Some(Format::Rss),
        "rdf:RDF" => Some(Format::Rss),
        "feed" if head.contains("http://www.w3.org/2005/Atom") => Some(Format::Atom),
        _ => None,
    }
}

/// The first element name in a document's head, past the declaration, comments,
/// processing instructions and a doctype.
fn root_name(head: &str) -> Option<&str> {
    let mut rest = head;
    loop {
        let open = rest.find('<')?;
        rest = &rest[open + 1..];
        match rest.as_bytes().first()? {
            b'?' | b'!' => rest = &rest[rest.find('>')? + 1..],
            _ => {
                let end = rest
                    .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                    .unwrap_or(rest.len());
                return Some(&rest[..end]);
            }
        }
    }
}

pub(super) fn entries(bytes: &[u8]) -> anyhow::Result<Entries> {
    // Splitting must never replace bytes and then claim the replacement was served.
    // Keep unsupported encodings in the original response without inventing entries.
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let text = std::str::from_utf8(bytes)?;
    let doc = parse(text)?;
    let root = doc.root_element();
    let (format, container, noun) = channel(root)?;

    let mut out: Vec<Entry> = Vec::new();
    for node in container
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == noun)
    {
        // The entry as served: the element's own span of the response.
        let served = text[node.range()].as_bytes();
        let link = link_of(node, format);
        let id = match format {
            Format::Atom => child_text(node, "id"),
            _ => child_text(node, "guid"),
        }
        .or_else(|| link.clone());
        let fragment = fragment_for(id.as_deref(), served);
        // A feed that repeats an id has one entry under it, as the publisher said.
        if out.iter().any(|e| e.fragment == fragment) {
            continue;
        }
        out.push(Entry {
            fragment,
            bytes: served.to_vec(),
            link,
            title: child_text(node, "title"),
        });
    }

    let (entries, dropped) = capped(out);
    Ok(Entries {
        format,
        title: child_text(container, "title")
            .or_else(|| child(root, "channel").and_then(|c| child_text(c, "title"))),
        entries,
        dropped,
    })
}

pub(super) fn read(bytes: &[u8]) -> anyhow::Result<Readable> {
    let text = decode(bytes);
    // A whole feed parses as it is. An entry cut out of one may use prefixes only the
    // feed's root declared — `content:encoded`, `dc:creator` — so it is read inside a
    // wrapper that declares whatever prefixes it uses.
    let wrapped;
    let doc = match parse(&text) {
        Ok(doc) => doc,
        Err(_) => {
            wrapped = wrap(&text);
            parse(&wrapped)?
        }
    };
    let mut root = doc.root_element();
    if root.tag_name().name() == WRAPPER {
        root = root
            .children()
            .find(|n| n.is_element())
            .ok_or_else(|| anyhow::anyhow!("an empty entry"))?;
    }

    match root.tag_name().name() {
        "item" => Ok(entry(root, Format::Rss)),
        "entry" => Ok(entry(root, Format::Atom)),
        _ => {
            let (format, container, noun) = channel(root)?;
            let channel = match format {
                Format::Rss => child(root, "channel").unwrap_or(container),
                _ => container,
            };
            let mut fields = Vec::new();
            if let Some(link) = link_of(channel, format) {
                fields.push(("Link", link));
            }
            let description =
                child_text(channel, "description").or_else(|| child_text(channel, "subtitle"));
            let listed: Vec<String> = container
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == noun)
                .take(MAX_LISTED)
                .map(|n| {
                    let mut line = child_text(n, "title").unwrap_or_else(|| "(untitled)".into());
                    if let Some(date) = date_of(n) {
                        line.push_str(&format!(" — {date}"));
                    }
                    if let Some(link) = link_of(n, format) {
                        line.push_str(&format!(" — {link}"));
                    }
                    line
                })
                .collect();
            Ok(Readable {
                title: child_text(channel, "title"),
                fields,
                body: description.map(body_from),
                listed,
            })
        }
    }
}

/// The element that holds the entries, the format, and what one entry is called.
fn channel<'a, 'i>(
    root: roxmltree::Node<'a, 'i>,
) -> anyhow::Result<(Format, roxmltree::Node<'a, 'i>, &'static str)> {
    match root.tag_name().name() {
        "rss" => Ok((
            Format::Rss,
            child(root, "channel").ok_or_else(|| anyhow::anyhow!("an RSS feed with no channel"))?,
            "item",
        )),
        // RSS 1.0 lists its items beside the channel, not inside it.
        "RDF" => Ok((Format::Rss, root, "item")),
        "feed" => Ok((Format::Atom, root, "entry")),
        other => anyhow::bail!("unexpected root element `{other}`: not RSS or Atom"),
    }
}

fn entry(node: roxmltree::Node<'_, '_>, format: Format) -> Readable {
    let mut fields = Vec::new();
    if let Some(date) = date_of(node) {
        fields.push(("Published", date));
    }
    let author = child_text(node, "creator").or_else(|| {
        child(node, "author").and_then(|a| child_text(a, "name").or_else(|| text_of(a)))
    });
    if let Some(author) = author {
        fields.push(("Author", author));
    }
    let categories: Vec<String> = node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "category")
        .filter_map(|n| text_of(n).or_else(|| n.attribute("term").map(str::to_string)))
        .collect();
    if !categories.is_empty() {
        fields.push(("Categories", categories.join(", ")));
    }
    if let Some(link) = link_of(node, format) {
        fields.push(("Link", link));
    }

    // The fullest body the entry carries: the article, then its summary.
    let body = match format {
        Format::Atom => child(node, "content")
            .and_then(atom_content)
            .or_else(|| child_text(node, "summary").map(body_from)),
        _ => child_text(node, "encoded")
            .map(Body::Html)
            .or_else(|| child_text(node, "description").map(body_from)),
    };

    Readable {
        title: child_text(node, "title"),
        fields,
        body,
        listed: Vec::new(),
    }
}

/// An Atom `<content>`: escaped markup for `type="html"`, inline markup for `xhtml`.
fn atom_content(node: roxmltree::Node<'_, '_>) -> Option<Body> {
    match node.attribute("type") {
        Some("xhtml") => {
            let first = node.children().find(|n| n.is_element())?;
            let last = node.children().rfind(|n| n.is_element())?;
            let doc = node.document().input_text();
            Some(Body::Html(
                doc[first.range().start..last.range().end].to_string(),
            ))
        }
        Some("html") => text_of(node).map(Body::Html),
        _ => text_of(node).map(body_from),
    }
}

fn date_of(node: roxmltree::Node<'_, '_>) -> Option<String> {
    ["pubDate", "published", "date", "updated"]
        .into_iter()
        .find_map(|name| child_text(node, name))
}

/// The address an item or a feed is about.
///
/// RSS writes it as the text of `<link>`; Atom as the `href` of a `<link>` whose `rel` is
/// `alternate` or absent. Atom's other links — `self`, `edit`, `replies` — are not it.
fn link_of(node: roxmltree::Node<'_, '_>, format: Format) -> Option<String> {
    let links = node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "link");
    match format {
        Format::Atom => links
            .filter(|n| matches!(n.attribute("rel"), None | Some("alternate")))
            .find_map(|n| n.attribute("href"))
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty()),
        _ => links.filter_map(text_of).next(),
    }
}

fn child<'a, 'i>(node: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == name)
}

fn child_text(node: roxmltree::Node<'_, '_>, name: &str) -> Option<String> {
    child(node, name).and_then(text_of)
}

/// Every piece of text under a node, joined — CDATA included, which is how most feeds
/// carry their markup.
fn text_of(node: roxmltree::Node<'_, '_>) -> Option<String> {
    let text: String = node
        .descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

fn parse(text: &str) -> anyhow::Result<roxmltree::Document<'_>> {
    // A feed may carry a doctype — RSS 0.91 shipped with one. roxmltree still refuses
    // the entity expansions that make a doctype dangerous.
    let opts = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    roxmltree::Document::parse_with_options(text, opts)
        .map_err(|e| anyhow::anyhow!("not valid XML: {e}"))
}

const WRAPPER: &str = "centinel-entry";

/// An entry inside an element that declares every prefix it uses, so it parses alone.
///
/// The namespaces are placeholders. Nothing here matches on a namespace — elements are
/// read by local name — so what a prefix stood for in the feed does not change a word of
/// what is read.
fn wrap(entry: &str) -> String {
    let mut prefixes: Vec<&str> = Vec::new();
    let mut rest = entry;
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('>') else { break };
        let tag = &rest[..close];
        rest = &rest[close + 1..];
        for token in tag.trim_start_matches('/').split_whitespace() {
            let name = token.split('=').next().unwrap_or_default();
            if let Some((prefix, _)) = name.split_once(':')
                && is_name(prefix)
                && !matches!(prefix, "xml" | "xmlns")
                && !prefixes.contains(&prefix)
            {
                prefixes.push(prefix);
            }
        }
    }
    let declared: String = prefixes
        .iter()
        .map(|p| format!(" xmlns:{p}=\"urn:centinel:{p}\""))
        .collect();
    format!("<{WRAPPER}{declared}>{entry}</{WRAPPER}>")
}

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Medium's shape, trimmed: `text/xml`, namespaced bodies in CDATA, a guid that is an
    /// address.
    const MEDIUM: &str = r#"<?xml version="1.0" encoding="UTF-8"?><rss xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:content="http://purl.org/rss/1.0/modules/content/" version="2.0"><channel><title><![CDATA[Stories by Pinellas DSA on Medium]]></title><link>https://medium.com/@dsa-pinellas</link><item><title><![CDATA[Why We Organize]]></title><link>https://dsa-pinellas.medium.com/why-we-organize-1a2b</link><guid isPermaLink="false">https://medium.com/p/1a2b</guid><category><![CDATA[labor]]></category><dc:creator><![CDATA[Pinellas DSA]]></dc:creator><pubDate>Tue, 14 Oct 2026 18:00:00 GMT</pubDate><content:encoded><![CDATA[<h3>Why We Organize</h3><p>Rent in Pinellas rose 40% in three years.</p>]]></content:encoded></item><item><title><![CDATA[Bus Fare]]></title><link>https://dsa-pinellas.medium.com/bus-fare-3c4d</link><guid isPermaLink="false">https://medium.com/p/3c4d</guid><pubDate>Mon, 06 Oct 2026 12:00:00 GMT</pubDate><content:encoded><![CDATA[<p>PSTA fares.</p>]]></content:encoded></item></channel></rss>"#;

    /// Each entry is the bytes the feed served for it, under the id the publisher gave it.
    #[test]
    fn a_feed_splits_into_the_entries_it_served() {
        let got = entries(MEDIUM.as_bytes()).unwrap();
        assert_eq!(got.format, Format::Rss);
        assert_eq!(
            got.title.as_deref(),
            Some("Stories by Pinellas DSA on Medium")
        );
        assert_eq!(
            got.entries
                .iter()
                .map(|e| e.fragment.as_str())
                .collect::<Vec<_>>(),
            [
                "entry=https://medium.com/p/1a2b",
                "entry=https://medium.com/p/3c4d"
            ]
        );
        let first = std::str::from_utf8(&got.entries[0].bytes).unwrap();
        assert!(first.starts_with("<item><title>") && first.ends_with("</item>"));
        assert!(
            MEDIUM.contains(first),
            "a slice of the response, not a rewrite"
        );
        assert_eq!(
            got.entries[0].link.as_deref(),
            Some("https://dsa-pinellas.medium.com/why-we-organize-1a2b")
        );
    }

    /// The slice uses prefixes only the feed's root declared, and is still read whole.
    #[test]
    fn an_entry_cut_from_its_feed_reads_alone() {
        let got = entries(MEDIUM.as_bytes()).unwrap();
        let read = read(&got.entries[0].bytes).unwrap();
        assert_eq!(read.title.as_deref(), Some("Why We Organize"));
        assert!(
            read.fields
                .contains(&("Author", "Pinellas DSA".to_string()))
        );
        assert!(read.fields.contains(&(
            "Link",
            "https://dsa-pinellas.medium.com/why-we-organize-1a2b".to_string()
        )));
        match read.body {
            Some(Body::Html(html)) => assert!(html.contains("Rent in Pinellas rose 40%")),
            other => panic!("the article body was not read: {other:?}"),
        }
    }

    #[test]
    fn atom_is_read_by_the_same_rules() {
        let atom = r#"<?xml version="1.0"?><feed xmlns="http://www.w3.org/2005/Atom"><title>Notes</title><entry><id>tag:a.org,2026:1</id><title>Budget</title><link rel="self" href="https://a.org/self"/><link href="https://a.org/budget"/><updated>2026-10-01T00:00:00Z</updated><content type="html">&lt;p&gt;The budget passed.&lt;/p&gt;</content></entry></feed>"#;
        assert_eq!(
            super::super::Format::sniff(atom.as_bytes()),
            Some(Format::Atom)
        );
        let got = entries(atom.as_bytes()).unwrap();
        assert_eq!(got.entries[0].fragment, "entry=tag:a.org,2026:1");
        assert_eq!(got.entries[0].link.as_deref(), Some("https://a.org/budget"));
        let read = read(&got.entries[0].bytes).unwrap();
        assert_eq!(
            read.body,
            Some(Body::Html("<p>The budget passed.</p>".into()))
        );
    }

    /// A feed's own text lists what it holds and carries none of the bodies, which are
    /// stored once each, as entries.
    #[test]
    fn a_whole_feed_reads_as_a_list_of_its_entries() {
        let read = read(MEDIUM.as_bytes()).unwrap();
        assert_eq!(
            read.title.as_deref(),
            Some("Stories by Pinellas DSA on Medium")
        );
        assert_eq!(read.listed.len(), 2);
        assert!(read.listed[0].starts_with("Why We Organize — Tue, 14 Oct 2026"));
        assert!(read.body.is_none());
    }

    /// `<feed>` is a common word; only the Atom namespace makes it a feed.
    #[test]
    fn a_feed_element_without_atom_is_not_a_feed() {
        assert_eq!(
            super::super::Format::sniff(b"<?xml version=\"1.0\"?><feed><x/></feed>"),
            None
        );
        assert_eq!(
            super::super::Format::sniff(b"<?xml version=\"1.0\"?><urlset/>"),
            None,
            "a sitemap is not a feed"
        );
    }
}
