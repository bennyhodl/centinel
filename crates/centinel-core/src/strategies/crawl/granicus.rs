//! `granicus` — the agendas and minutes a Granicus publisher lists.
//!
//! Keyed on a **product**. Granicus hosts council meetings for most of Tampa Bay at
//! `{client}.granicus.com`: Treasure Island, Safety Harbor and Hillsborough County publish
//! agendas there, and Clearwater, St. Petersburg and Gulfport their video. Each client
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
//! ## What the meeting voted on: the agenda manager's documents
//!
//! A client that builds its agendas in Granicus's agenda manager — Safety Harbor, Doral —
//! answers the viewer with a 302 to `GeneratedAgendaViewer.php` with the same query: the
//! agenda as HTML, each item's supporting documents linked as
//! `MetaViewer.php?view_id=V&clip_id=C&meta_id=M`, which answers with the PDF. A client
//! that uploads finished agendas (Treasure Island) answers `GeneratedAgendaViewer.php` with
//! a 302 to `panes/EmptyTemplate.php` — 299 bytes, "No Template provided." — so asking for
//! the generated page directly costs one small request where following the viewer would
//! download the agenda PDF only to learn there is nothing inside it.
//!
//! `meta_id` alone decides the file: at both clients, the document came back byte for byte
//! with `view_id` empty or missing, `clip_id` missing or another clip's, and nothing but
//! `meta_id`; without it the answer is empty. So a document is written as
//! `MetaViewer.php?meta_id=M` — one document, one address, whichever agenda links it.
//!
//! The viewers are kept first, off the seed alone. Then the generated agendas are opened
//! **newest meeting first** — by the date each publisher row hides in a
//! `<span style="display:none;">` epoch, since a publisher lists each body in its own
//! table — so a walk that runs out of requests has every agenda and minutes and the newest
//! meetings' documents. A feed has no such date and is already newest first.
//!
//! The newest [`PROBE`] agendas decide whether the rest are worth opening: if none of them
//! links a document, the client is taken not to use the agenda manager, and the walk stops
//! with a note rather than spending one request per meeting on empty templates — 827 of
//! them at Treasure Island, against a 200-request budget, and a run that would then read as
//! truncated when nothing was missed.
//!
//! ## robots.txt
//!
//! Every `*.granicus.com` host serves `User-agent: *` / `Disallow: /` and admits Googlebot.
//! Unless a source's definition sets `ignore_robots`, nothing is kept, no generated agenda
//! is asked for, and the run says why.

use std::collections::HashMap;

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

/// How many of the newest agendas must link no document before the rest are not opened.
/// See the module docs.
const PROBE: u64 = 3;

/// The agenda manager's rendering of an agenda, at the viewer's query.
const GENERATED: &str = "/GeneratedAgendaViewer.php";

/// Where a document of the agenda manager is served.
const META_VIEWER: &str = "/MetaViewer.php";

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
            for url in &found {
                if pass.full() {
                    break;
                }
                pass.keep(url.to_string(), &seed.robots);
            }
            pass.note(Note::new("publisher", at.to_string()));

            // Then each agenda's generated page, newest meeting first. Every one is on the
            // seed's host, so the seed's rules are the ones that apply to the request.
            for page in generated_agendas(seed, &at, &found) {
                if seed.robots.allowed(page.as_str()) {
                    pass.push(page.to_string(), 0);
                }
            }
            let (mut opened, mut documents) = (0u64, 0u64);
            loop {
                if opened == PROBE && documents == 0 && pass.pending() > 0 {
                    pass.note(Note::new(
                        "agenda manager",
                        format!(
                            "the newest {PROBE} agendas link no documents, so the client is \
                             taken not to build its agendas in Granicus and the rest are not \
                             opened"
                        ),
                    ));
                    break;
                }
                let Some((page, _)) = pass.next_to_visit() else {
                    break;
                };
                pass.visiting(format!("agenda {page}"));
                opened += 1;
                let body = match pass.walk().get(&page).await {
                    Ok(f) => f.bytes,
                    Err(refusal) => {
                        pass.refused(&page, &refusal);
                        continue;
                    }
                };
                let at_page = Url::parse(&page)?;
                for address in documents_in(&String::from_utf8_lossy(&body), &at_page) {
                    if pass.full() {
                        break;
                    }
                    documents += 1;
                    pass.keep(address, &seed.robots);
                }
            }
            pass.figure("generated", opened);
            pass.figure("documents", documents);
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

/// Each agenda viewer's generated page, newest meeting first.
///
/// Dated by the publisher row each viewer sits in; a viewer with no date (every one, when
/// the seed is a feed) keeps its listed place after the dated ones. Repeats are left to
/// the walk, which visits an address once.
fn generated_agendas(seed: &Seed, at: &Url, viewers: &[Url]) -> Vec<Url> {
    let held = held_on(&seed.text(), at);
    let mut agendas: Vec<(Option<i64>, usize, Url)> = viewers
        .iter()
        .enumerate()
        .filter(|(_, u)| u.path() == VIEWERS[0].0)
        .map(|(listed, u)| {
            let mut page = u.clone();
            page.set_path(GENERATED);
            (held.get(u.as_str()).copied(), listed, page)
        })
        .collect();
    agendas.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    agendas.into_iter().map(|(_, _, page)| page).collect()
}

/// The meeting date of each agenda viewer a publisher page links, from the epoch its row
/// hides before the visible date: `<span style="display:none;">1794902400</span>Nov 17, 2026`.
fn held_on(page: &str, at: &Url) -> HashMap<String, i64> {
    let base = crate::html::Scan::new(page).base(at);
    let mut held = HashMap::new();
    for row in page.split("<tr").skip(1) {
        let row = row.split("</tr>").next().unwrap_or(row);
        let Some(epoch) = row
            .split_once("display:none;\">")
            .and_then(|(_, rest)| rest.split('<').next())
            .and_then(|digits| digits.trim().parse::<i64>().ok())
        else {
            continue;
        };
        for tag in crate::html::Scan::new(row).tags(&["a"]) {
            if let Some(mut u) = tag.attr("href").and_then(|href| base.join(href)) {
                u.set_fragment(None);
                held.insert(u.to_string(), epoch);
            }
        }
    }
    held
}

/// The documents a generated agenda links, each at its one address,
/// `MetaViewer.php?meta_id=M` on the agenda's host, in the order listed.
fn documents_in(page: &str, at: &Url) -> Vec<String> {
    let scan = crate::html::Scan::new(page);
    let base = scan.base(at);
    let mut out: Vec<String> = Vec::new();
    for tag in scan.tags(&["a"]) {
        let Some(link) = tag.attr("href").and_then(|href| base.join(href)) else {
            continue;
        };
        if link.host_str() != at.host_str() || link.path() != META_VIEWER {
            continue;
        }
        let Some(meta) = link
            .query_pairs()
            .find(|(k, _)| k == "meta_id")
            .and_then(|(_, v)| v.parse::<u64>().ok())
        else {
            continue;
        };
        let mut address = at.clone();
        address.set_path(META_VIEWER);
        address.set_query(Some(&format!("meta_id={meta}")));
        address.set_fragment(None);
        let address = address.to_string();
        if !out.contains(&address) {
            out.push(address);
        }
    }
    out
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

    /// Treasure Island's generated agenda, as `panes/EmptyTemplate.php` serves it after the
    /// 302: the client uploads finished agendas and builds none in Granicus.
    const EMPTY_TEMPLATE: &str = r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN">
<html><body><table height=50% width=100% border=0><tr height=100%>
  <td width=100% height=100% align=center valign=middle>
    <font face="arial"><strong>No Template provided.</strong></font>
  </td></tr></table></body></html>"#;

    fn generated(host: &str, view: u32, clip: u32) -> String {
        format!("https://{host}/GeneratedAgendaViewer.php?view_id={view}&clip_id={clip}")
    }

    #[tokio::test]
    async fn a_publisher_page_names_its_agendas_and_minutes_without_a_request() {
        let s = seed(PUBLISHER, PAGE);
        let r = Granicus.recognise(&s).expect("a Granicus publisher");
        assert_eq!(r.keyed_on, Keyed::Product("Granicus"));

        let ti = "treasureislandfl.granicus.com";
        let crawl = Fake::new([
            (generated(ti, 1, 1045).as_str(), EMPTY_TEMPLATE.to_string()),
            (generated(ti, 1, 1009).as_str(), EMPTY_TEMPLATE.to_string()),
        ]);
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
        assert_eq!(got.figures["documents"], 0);
        assert!(!got.truncated && !got.discovery_failed);
        assert_eq!(
            crawl.requests(),
            2,
            "the publisher was in hand; each agenda's generated page is asked for once, and \
             an empty template names no document"
        );
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
        let crawl = Fake::new([]);
        let got = Granicus.enumerate(&honoured, &crawl).await.unwrap();
        assert!(got.addresses.is_empty());
        assert_eq!(got.figures["disallowed"], 3);
        assert_eq!(
            crawl.requests(),
            0,
            "no generated agenda is asked for either"
        );

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

    /// Safety Harbor's publisher, recorded 2026-11-21 and cut to three rows in two bodies'
    /// tables. The CRA's table comes first, so listed order is not meeting order; each row
    /// hides its date as an epoch.
    const SH_PUBLISHER: &str = r#"<html><body>
<table class="listingTable"><tbody>
<tr class="even"><td class="listItem" headers="Name" id="COMMUNITY-REDEVELOPMENT-AGENCY" scope="row">COMMUNITY REDEVELOPMENT AGENCY</td>
<td class="listItem" headers="Date COMMUNITY-REDEVELOPMENT-AGENCY"><span style="display:none;">1758524400</span>Sep 22, 2025</td>
<td class="listItem"><a href="//safetyharbor.granicus.com/AgendaViewer.php?view_id=6&clip_id=2988" target="_blank">Agenda</a></td>
<td class="listItem" headers="VideoLink COMMUNITY-REDEVELOPMENT-AGENCY"><a href="javascript:void(0);" onClick="window.open('//safetyharbor.granicus.com/MediaPlayer.php?view_id=6&clip_id=2988')">Video</a></td></tr>
</tbody></table>
<table class="listingTable"><tbody>
<tr class="even"><td class="listItem" headers="Name" id="CITY-COMMISSION" scope="row">CITY COMMISSION</td>
<td class="listItem" headers="Date CITY-COMMISSION"><span style="display:none;">1793606400</span>Nov  2, 2026</td>
<td class="listItem"><a href="//safetyharbor.granicus.com/AgendaViewer.php?view_id=6&clip_id=3176" target="_blank">Agenda</a></td></tr>
<tr class="odd"><td class="listItem" headers="Name" id="CITY-COMMISSION" scope="row">CITY COMMISSION</td>
<td class="listItem" headers="Date CITY-COMMISSION"><span style="display:none;">1794902400</span>Nov 17, 2026</td>
<td class="listItem"><a href="//safetyharbor.granicus.com/AgendaViewer.php?view_id=6&clip_id=3190" target="_blank">Agenda</a></td></tr>
</tbody></table></body></html>"#;

    const SH_PAGE: &str = "https://safetyharbor.granicus.com/ViewPublisher.php?view_id=6";
    const SH: &str = "safetyharbor.granicus.com";

    /// Clip 3190's generated agenda, cut to two consent items. The second document link is
    /// repeated with another clip's id, as a document linked from two agendas would be; that
    /// repeat is constructed, the rest is as served.
    const SH_AGENDA_3190: &str = r#"<html><head><title>City Commission Agenda</title></head><body>
<div align=center><strong><font face=Arial>MEETING OF TUESDAY, NOVEMBER 17, 2026</font></strong></div>
<div align=justify><strong>Approval of the November 2, 2026 City Commission meeting minutes.</strong></div>
<blockquote dir="ltr"><div align=justify><strong><a href="https://safetyharbor.granicus.com/MetaViewer.php?view_id=6&clip_id=3190&meta_id=199147"  name="document199147" class="Document Document2" onclick="">LINK TO CONSENT #1: BACKUP MATERIALS</a></strong></div></blockquote>
<div align=justify><strong>Acceptance of City of Safety Harbor Annual Comprehensive Financial Report FYE September 30, 2025.</strong></div>
<blockquote dir="ltr"><div align=justify><strong><a href="https://safetyharbor.granicus.com/MetaViewer.php?view_id=6&clip_id=3190&meta_id=199149"  name="document199149" class="Document Document2" onclick="">LINK TO CONSENT #2: BACKUP MATERIALS</a></strong></div></blockquote>
<a href="https://safetyharbor.granicus.com/MetaViewer.php?view_id=6&clip_id=3176&meta_id=199149">LINK TO CONSENT #2: BACKUP MATERIALS</a>
</body></html>"#;

    /// Clip 2988's (the CRA, September 2025), cut to one item.
    const SH_AGENDA_2988: &str = r#"<html><head><title>CRA Agenda</title></head><body>
<div align=justify><strong><a href="https://safetyharbor.granicus.com/MetaViewer.php?view_id=6&clip_id=2988&meta_id=189377"  name="document189377" class="Document Document1" onclick="">LINK TO ITEM #2: BACKUP MATERIALS - MEMO</a></strong></div>
</body></html>"#;

    fn meta(id: u64) -> String {
        format!("https://{SH}/MetaViewer.php?meta_id={id}")
    }

    fn viewer(clip: u32) -> String {
        format!("https://{SH}/AgendaViewer.php?view_id=6&clip_id={clip}")
    }

    /// The agenda manager's documents, read from each generated agenda, newest meeting
    /// first whatever table its body is listed in, and each at its one address.
    #[tokio::test]
    async fn each_generated_agenda_gives_its_documents_newest_meeting_first() {
        let s = seed(SH_PUBLISHER, SH_PAGE);
        let crawl = Fake::new([
            (generated(SH, 6, 3190).as_str(), SH_AGENDA_3190.to_string()),
            (generated(SH, 6, 3176).as_str(), EMPTY_TEMPLATE.to_string()),
            (generated(SH, 6, 2988).as_str(), SH_AGENDA_2988.to_string()),
        ]);
        let got = Granicus.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                viewer(2988),
                viewer(3176),
                viewer(3190),
                meta(199147),
                meta(199149),
                meta(189377),
            ],
            "the viewers as listed, then Nov 17's documents, then Sep 22's"
        );
        assert_eq!(got.figures["generated"], 3);
        assert_eq!(got.figures["documents"], 3);
        assert_eq!(crawl.requests(), 3, "one request per agenda");
        assert!(
            !got.truncated && got.warnings.is_empty(),
            "{:?}",
            got.warnings
        );
    }

    /// Out of requests, the walk has every viewer and the newest meeting's documents.
    #[tokio::test]
    async fn a_walk_out_of_budget_has_the_newest_meetings_documents() {
        let s = seed(SH_PUBLISHER, SH_PAGE);
        let crawl = Fake::new([
            (generated(SH, 6, 3190).as_str(), SH_AGENDA_3190.to_string()),
            (generated(SH, 6, 2988).as_str(), SH_AGENDA_2988.to_string()),
        ])
        .with_budget(1);
        let got = Granicus.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                viewer(2988),
                viewer(3176),
                viewer(3190),
                meta(199147),
                meta(199149)
            ]
        );
        assert!(got.truncated, "two agendas were never opened");
        assert_eq!(crawl.requests(), 1);
    }

    /// A client that builds no agenda in Granicus is found out by its newest few, and the
    /// rest of its meetings cost nothing — nor does the run read as truncated.
    #[tokio::test]
    async fn a_client_without_the_agenda_manager_is_not_walked_past_the_newest_few() {
        let rows: String = (1..=6)
            .map(|clip| {
                format!(
                    "<tr><td><span style=\"display:none;\">{}</span>Day {clip}</td>\
                     <td><a href=\"//treasureislandfl.granicus.com/AgendaViewer.php?view_id=1&clip_id={clip}\">Agenda</a></td></tr>",
                    1_700_000_000 + clip * 86_400
                )
            })
            .collect();
        let s = seed(
            &format!("<html><body><table>{rows}</table></body></html>"),
            PAGE,
        );
        let ti = "treasureislandfl.granicus.com";
        let crawl = Fake::new([
            (generated(ti, 1, 6).as_str(), EMPTY_TEMPLATE.to_string()),
            (generated(ti, 1, 5).as_str(), EMPTY_TEMPLATE.to_string()),
            (generated(ti, 1, 4).as_str(), EMPTY_TEMPLATE.to_string()),
        ]);
        let got = Granicus.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(got.addresses.len(), 6, "every viewer is still kept");
        assert_eq!(crawl.requests(), 3, "the newest three, and no more");
        assert_eq!(got.figures["generated"], 3);
        assert!(got.notes.iter().any(|n| n.label == "agenda manager"));
        assert!(!got.truncated, "nothing was missed");
    }

    /// `meta_id` alone decides the file, so every spelling of a link is one address; links
    /// that are not a document on this host are not kept.
    #[test]
    fn a_document_has_one_address_whatever_its_view_or_clip() {
        let at = Url::parse(&generated(SH, 6, 3190)).unwrap();
        let page = r#"
            <a href="https://safetyharbor.granicus.com/MetaViewer.php?view_id=6&clip_id=3190&meta_id=7">a</a>
            <a href="MetaViewer.php?view_id=&clip_id=3190&meta_id=7">relative, empty view</a>
            <a href="//safetyharbor.granicus.com/MetaViewer.php?meta_id=7#page=2">bare, fragment</a>
            <a href="https://safetyharbor.granicus.com/MetaViewer.php?view_id=6&clip_id=3190&meta_id=8">b</a>
            <a href="https://safetyharbor.granicus.com/MetaViewer.php?view_id=6&clip_id=3190">no meta_id</a>
            <a href="https://cityofdoral.granicus.com/MetaViewer.php?view_id=2&clip_id=68&meta_id=9770">another client</a>
            <a href="https://safetyharbor.granicus.com/MediaPlayer.php?view_id=6&clip_id=3190&meta_id=9">player</a>"#;
        assert_eq!(documents_in(page, &at), [meta(7), meta(8)]);
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
