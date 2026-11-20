//! `granicus` — the agendas and minutes a Granicus publisher lists.
//!
//! Keyed on a **product**. Granicus hosts council meetings for most of Tampa Bay at
//! `{client}.granicus.com`: Treasure Island and Hillsborough County publish agendas there,
//! and Clearwater, St. Petersburg, Safety Harbor and Gulfport their video. Each client
//! serves the same two pages for a view:
//!
//! - `ViewPublisher.php?view_id=N`, a table of every meeting the view holds. Each row links
//!   `//{client}.granicus.com/AgendaViewer.php?view_id=N&clip_id=M` and, where one was
//!   kept, `MinutesViewer.php?…` — scheme-relative, which resolves like any other link.
//! - `ViewPublisherRSS.php?view_id=N&mode=agendas`, the latest hundred agendas, each item's
//!   `<link>` the same `AgendaViewer.php` address.
//!
//! A viewer address answers with the document itself: Treasure Island's redirects to a PDF
//! on `granicus_production_attachments.s3.amazonaws.com`. So this names **documents**, and
//! reads them straight off the seed in hand without a request. Following them as `links`
//! does instead fetches every PDF during discovery only to learn its address, and again to
//! collect it — 827 agendas on Treasure Island's page, against a 200-request budget.
//!
//! A client that keeps its agendas elsewhere still links the viewer. Hillsborough's answers
//! 200 with "This agenda is not currently published", because the agendas are in Legistar;
//! that is the publisher's statement, collected as such.
//!
//! ## robots.txt
//!
//! Every `*.granicus.com` host serves `User-agent: *` / `Disallow: /` and admits Googlebot.
//! Unless a source's definition sets `ignore_robots`, nothing is kept and the run says why.

use futures::future::BoxFuture;
use url::Url;

use super::{Addresses, Enumerated, Pass, Seed, Strategy, StrategyDef, Walk};
use crate::domain::Note;
use crate::feed::Format;
use crate::strategies::{Keyed, Recognition};

pub struct Granicus;

inventory::submit! { StrategyDef { name: "granicus", it: &Granicus } }

/// The pages a publisher is read from.
const PUBLISHERS: [&str; 2] = ["/ViewPublisher.php", "/ViewPublisherRSS.php"];

/// The viewers that serve a meeting's documents, and what each serves.
const VIEWERS: [(&str, &str); 2] = [
    ("/AgendaViewer.php", "agendas"),
    ("/MinutesViewer.php", "minutes"),
];

impl Strategy for Granicus {
    fn name(&self) -> &'static str {
        "granicus"
    }

    fn recognise(&self, seed: &Seed) -> Option<Recognition> {
        let url = seed.final_url()?;
        let host = url.host_str()?;
        if !host.ends_with(".granicus.com") || !PUBLISHERS.contains(&url.path()) {
            return None;
        }
        // Confirmed on the bytes, not only the address: a publisher with nothing in it is
        // not one this can collect.
        let found = viewers(seed, &url);
        if found.is_empty() {
            return None;
        }
        let mut r = Recognition::new(self.name(), Keyed::Product("Granicus"))
            .seeing("host", format!("{host} is *.granicus.com"))
            .seeing("publisher", format!("{} lists meetings", url.path()));
        for (path, noun) in VIEWERS {
            let n = found.iter().filter(|u| u.path() == path).count();
            if n > 0 {
                r = r.seeing(noun, format!("{n} {path} link(s)"));
            }
        }
        if Format::sniff(&seed.page.bytes).is_some() {
            r = r.warning(
                "window",
                "the feed lists the latest agendas; ViewPublisher.php for the same view \
                 lists them all",
            );
        }
        Some(r)
    }

    fn enumerate<'a>(
        &'a self,
        seed: &'a Seed,
        crawl: &'a dyn Walk,
    ) -> BoxFuture<'a, anyhow::Result<Enumerated>> {
        Box::pin(async move {
            let at = seed
                .final_url()
                .ok_or_else(|| anyhow::anyhow!("the seed does not record where it was served"))?;
            let mut pass = Pass::<String>::new(crawl, "publisher", 0);
            if seed.page.bytes.is_empty() {
                pass.warn(format!("{at}: the publisher could not be read"));
                return Ok(pass.finish());
            }
            pass.read_surface();

            let found = viewers(seed, &at);
            for (path, noun) in VIEWERS {
                let n = found.iter().filter(|u| u.path() == path).count();
                pass.figure(noun, n as u64);
            }
            for url in found {
                if pass.full() {
                    break;
                }
                pass.keep(url.to_string(), &seed.robots);
            }
            pass.note(Note::new("publisher", at.to_string()));
            Ok(pass.finish())
        })
    }

    /// A viewer address is the document, so acquisition has nothing to scan for.
    fn addresses_are(&self) -> Addresses {
        Addresses::Documents
    }
}

/// Every viewer address the seed links, on the seed's own host, in the order listed.
///
/// From the feed's item links when the seed is a feed, and from the page's anchors when it
/// is the publisher page. Never a scan of the text: the page's scripts build player links
/// that are not documents.
fn viewers(seed: &Seed, at: &Url) -> Vec<Url> {
    let candidates: Vec<Url> = match Format::sniff(&seed.page.bytes) {
        Some(_) => crate::feed::entries(&seed.page.bytes)
            .map(|split| {
                split
                    .entries
                    .into_iter()
                    .filter_map(|e| e.link)
                    .filter_map(|link| at.join(&link).ok())
                    .collect()
            })
            .unwrap_or_default(),
        None => {
            let html = seed.text();
            let scan = crate::html::Scan::new(&html);
            let base = scan.base(at);
            scan.tags(&["a"])
                .into_iter()
                .filter_map(|tag| tag.attr("href").and_then(|href| base.join(href)))
                .collect()
        }
    };
    candidates
        .into_iter()
        .filter(|u| u.host_str() == at.host_str())
        .filter(|u| VIEWERS.iter().any(|(path, _)| u.path() == *path))
        .map(|mut u| {
            u.set_fragment(None);
            u
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::crawl::tests::{Fake, seed};

    /// Treasure Island's publisher, recorded 2026-11-20 and cut to two rows. The links are
    /// scheme-relative and unescaped, as served.
    const PUBLISHER: &str = r#"<html><body><table>
        <tr><td class="listItem" headers="Date Board-of-Commissioners">November&nbsp;17,&nbsp;2026</td>
        <td class="listItem"><a href="//treasureislandfl.granicus.com/AgendaViewer.php?view_id=1&clip_id=1045" target="_blank">Agenda</a></td>
        <td class="listItem">&nbsp;</td>
        <td class="listItem"><a href="javascript:void(0);" onClick="window.open('//treasureislandfl.granicus.com/MediaPlayer.php?view_id=1&clip_id=1045')">Video</a></td></tr>
        <tr><td class="listItem"><a href="//treasureislandfl.granicus.com/AgendaViewer.php?view_id=1&clip_id=1009" target="_blank">Agenda</a></td>
        <td class="listItem"><a href="//treasureislandfl.granicus.com/MinutesViewer.php?view_id=1&clip_id=1009&doc_id=4bd7" target="_blank">Minutes</a></td>
        <td class="listItem"><a href="https://treasureislandfl.gov/">City</a></td></tr>
        </table><a href="//treasureislandfl.granicus.com/ViewPublisherRSS.php?view_id=1&mode=agendas">RSS</a></body></html>"#;

    /// The agenda feed for the same view: a non-permalink guid, a `gran:` element in its
    /// own namespace, and the address again inside CDATA.
    const FEED: &str = r#"<?xml version="1.0" ?>
<?xml-stylesheet href="browserfriendlyRSS_Layout.xslt" type="text/xsl" media="screen"?>
<rss version="2.0" xmlns:dc="https://purl.org/dc/elements/1.1/" xmlns:gran="https://www.granicus.com/schema/rss-supplements" >
<channel>
  <title>Treasure Island, FL: Treasure Island FL Content (Agenda Feed)</title>
  <link>https://treasureislandfl.granicus.com/ViewPublisher.php?view_id=1</link>
<item>
  <guid isPermaLink="false">dd4b32f0-7e3f-47c3-afb3-aec8b5b0e144</guid>
  <title>Board of Commissioners - Nov 17, 2026</title>
  <pubDate>Tue, 17 Nov 2026 02:00:00 -0800</pubDate>
  <gran:pubDateParts yr='2026' mo='11' day='17' hr='14' min='00' sec='0' tz='8' />
  <link>https://treasureislandfl.granicus.com/AgendaViewer.php?view_id=1&amp;clip_id=1045</link>
  <description><![CDATA[<p>Access it here:<br /><a href="https://treasureislandfl.granicus.com/AgendaViewer.php?view_id=1&amp;clip_id=1045">https://treasureislandfl.granicus.com/AgendaViewer.php?view_id=1&amp;clip_id=1045</a></p>]]></description>
</item>
<item>
  <guid isPermaLink="false">9a287c53-b593-4a1b-99b3-f7a38f9105c8</guid>
  <title>Board of Commissioners - Oct 20, 2026</title>
  <link>https://treasureislandfl.granicus.com/AgendaViewer.php?view_id=1&amp;clip_id=1042</link>
</item>
</channel></rss>"#;

    const PAGE: &str = "https://treasureislandfl.granicus.com/ViewPublisher.php?view_id=1";
    const RSS: &str =
        "https://treasureislandfl.granicus.com/ViewPublisherRSS.php?view_id=1&mode=agendas";

    #[tokio::test]
    async fn a_publisher_page_names_its_agendas_and_minutes_without_a_request() {
        let s = seed(PUBLISHER, PAGE);
        let r = Granicus.recognise(&s).expect("a Granicus publisher");
        assert_eq!(r.keyed_on, Keyed::Product("Granicus"));

        let crawl = Fake::new([]);
        let got = Granicus.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                "https://treasureislandfl.granicus.com/AgendaViewer.php?view_id=1&clip_id=1045",
                "https://treasureislandfl.granicus.com/AgendaViewer.php?view_id=1&clip_id=1009",
                "https://treasureislandfl.granicus.com/MinutesViewer.php?view_id=1&clip_id=1009&doc_id=4bd7",
            ],
            "the viewers only: no player, no feed, no other host"
        );
        assert_eq!(got.figures["agendas"], 2);
        assert_eq!(got.figures["minutes"], 1);
        assert!(!got.truncated && !got.discovery_failed);
        assert_eq!(crawl.requests(), 0, "the publisher was in hand");
    }

    /// The feed's items carry the same addresses, and it outranks the `feed` standard
    /// because the product is more specific.
    #[tokio::test]
    async fn the_agenda_feed_names_the_same_viewers() {
        let s = seed(FEED, RSS);
        let (best, r) = crate::strategies::crawl::best(&s).unwrap();
        assert_eq!(best.name, "granicus");
        assert!(r.warnings.iter().any(|n| n.label == "window"));

        let got = Granicus.enumerate(&s, &Fake::new([])).await.unwrap();
        assert_eq!(
            got.addresses,
            [
                "https://treasureislandfl.granicus.com/AgendaViewer.php?view_id=1&clip_id=1045",
                "https://treasureislandfl.granicus.com/AgendaViewer.php?view_id=1&clip_id=1042",
            ]
        );
    }

    /// The host's robots.txt turns every crawler away. Honoured, nothing is kept.
    #[tokio::test]
    async fn robots_txt_keeps_every_viewer_out_unless_overruled() {
        let robots = "User-agent: Googlebot\nDisallow: /JSON.php\nUser-agent: *\nDisallow: /";
        let honoured = Seed {
            robots: crate::discovery::Robots::parse("centinel", robots.as_bytes()),
            ..seed(PUBLISHER, PAGE)
        };
        let got = Granicus.enumerate(&honoured, &Fake::new([])).await.unwrap();
        assert!(got.addresses.is_empty());
        assert_eq!(got.figures["disallowed"], 3);

        let overruled = Seed {
            robots: crate::discovery::Robots::parse("centinel", robots.as_bytes()).overruled(),
            ..seed(PUBLISHER, PAGE)
        };
        let got = Granicus
            .enumerate(&overruled, &Fake::new([]))
            .await
            .unwrap();
        assert_eq!(got.addresses.len(), 3);
    }

    #[test]
    fn a_page_elsewhere_is_not_a_publisher() {
        assert!(
            Granicus
                .recognise(&seed(PUBLISHER, "https://www.treasureislandfl.gov/agendas"))
                .is_none(),
            "the product lives on its own host"
        );
        assert!(
            Granicus
                .recognise(&seed("<html><body>no meetings</body></html>", PAGE))
                .is_none(),
            "a publisher with nothing in it"
        );
    }
}
