//! `civicweb` — an iCompass CivicWeb portal's meetings: each published meeting's agenda and
//! minutes, and the documents its agenda links for each item.
//!
//! Keyed on a **product**, at two populated sightings: Oldsmar (`myoldsmar.civicweb.net`)
//! and Largo (`largo.civicweb.net`). A third, Madeira Beach (`madeirabeachfl.civicweb.net`),
//! is the same product with nothing published (`docs/FIELD-NOTES.md` entry 21). iCompass
//! serves every portal at `https://{client}.civicweb.net/`, and every portal page loads the
//! product's icons from `i.civicweb.net`. The welcome text that says "CivicWeb" is the
//! client's to edit, and two of the three front pages do not carry it.
//!
//! ## Where the meetings are
//!
//! The portal's calendar script reads a JSON service on the portal's own host:
//!
//! - `Services/MeetingsService.svc/meetings?from=YYYY-MM-DD&to=YYYY-MM-DD` — every meeting in
//!   the range, in one answer, newest first: its `Id`, `MeetingDateTime` and `Published`.
//!   `from=1900-01-01&to=9999-12-31` answered all 954 of Oldsmar's meetings, 2016 to 2027.
//! - `Services/MeetingsService.svc/meetings/{Id}/meetingDocuments` — the meeting's published
//!   documents, each with an `Id`, its output (`Type` 1 agenda, 2 minutes) and its rendition
//!   (`DocumentType`). An HTML rendition carries the whole agenda in `Html`, and the agenda
//!   links each item's documents.
//!
//! A meeting the calendar marks unpublished answered `[]` every time it was asked, at all
//! three portals, so it is not opened.
//!
//! ## Which address holds each document
//!
//! `/document/{id}/`. The agenda links an item's document as
//! `/document/257868/2026%20Janice%20Miller….pdf?handle=BB3444…`, and only the id decides it:
//! another name, no name, no `handle`, or a `handle` of zeros answered the same bytes. Without
//! the trailing slash the server answers a 301 to it. An id the portal never listed answers
//! 200 with the sign-in page, so only ids the portal named are written. A Word document is
//! served as the product's HTML rendering of it.
//!
//! Of a meeting's own documents the PDF renditions are kept: the agenda (at Oldsmar and Largo
//! the packet, the agenda with every item's report and attachment, 141 pages for one council
//! meeting) and the minutes, published or adopted. The HTML renditions are not: each came
//! with a PDF of the same output in every meeting sampled, and its text is in that PDF.
//!
//! The agenda links an item's report and its attachments, and a few links are to the
//! Document Center, `/filepro/documents/{folder}/?preview={id}`: the document is
//! `/document/{id}/`. Both are kept. The packet binds the same documents into one file,
//! which is what the city published; an attachment is one document under its own name.
//!
//! A document the product holds back has a `PublicReleaseDelayDateString`, and the portal's
//! script shows a notice in its place. None was seen. One that comes is not kept, nor is
//! anything it links, and the walk counts it.
//!
//! ## Order, and what a short walk loses
//!
//! The calendar first, one request. Then the published meetings **newest first**, one
//! request each, every document of a meeting kept from its one answer. A walk that runs out
//! of requests has everything of the newest meetings and nothing of the oldest.
//!
//! ## Bounding a walk
//!
//! The seed bounds it. Handed any page of the portal, the walk asks the calendar for every
//! meeting it holds. Handed the calendar address with a range —
//! `https://largo.civicweb.net/Services/MeetingsService.svc/meetings?from=2026-08-22&to=2026-11-20`
//! — it covers that range. The request budget (`--max-sitemaps`) applies inside it.
//!
//! ## robots.txt
//!
//! Every portal serves the same file: `User-agent: *` / `Disallow: /`, and six named search
//! engines admitted to everything but the product's assets, with `Crawl-delay: 120`. Every
//! request and address is on the portal's host and checked against the seed's rules, so
//! unless the source sets `ignore_robots` nothing is asked and the run says why.

use std::collections::BTreeMap;
use std::fmt;

use futures::future::BoxFuture;
use jiff::civil::Date;
use serde::Deserialize;
use url::Url;

use super::window::Window;
use super::{Addresses, Enumerated, Pass, Seed, Strategy, StrategyDef, Walk};
use crate::domain::Note;
use crate::strategies::{Keyed, Recognition};

pub struct CivicWeb;

inventory::submit! { StrategyDef { name: "civicweb", it: &CivicWeb } }

/// The product, as a recognition names it.
const PRODUCT: &str = "iCompass CivicWeb";

/// Every portal's host ends here.
const DOMAIN: &str = ".civicweb.net";

/// Hosts under the domain that are not a client's portal: the vendor's site and the asset
/// host.
const NOT_PORTALS: [&str; 2] = ["www", "i"];

/// What every portal page carries: its icons, from the product's asset host.
const ASSETS: &str = "//i.civicweb.net/";

/// The calendar service, on the portal's host.
const CALENDAR: &str = "Services/MeetingsService.svc/meetings";

/// The calendar is depth 0, a meeting's documents 1.
const MAX_DEPTH: usize = 1;

/// The PDF renditions of a meeting's own documents: the agenda (open, closed), the
/// minutes (open, closed) and the adopted minutes (open, closed), as the portal's page
/// numbers them.
const PDF: [u32; 6] = [4, 8, 10, 12, 53, 54];

/// The figures a walk reports, in the order they are written.
const FIGURES: [&str; 7] = [
    "meetings",
    "published",
    "opened",
    "agendas",
    "minutes",
    "attachments",
    "held back",
];

impl Strategy for CivicWeb {
    fn name(&self) -> &'static str {
        "civicweb"
    }

    fn recognise(&self, seed: &Seed) -> Option<Recognition> {
        let at = seed.final_url()?;
        let root = root_of(&at)?;
        let window = window_of(&at);
        // A portal page is confirmed on its bytes; a name under the domain is not a portal.
        if window.is_none() && !seed.text().contains(ASSETS) {
            return None;
        }
        let calendar = calendar_at(&root, window.unwrap_or_else(everything));
        let mut r = Recognition::new(self.name(), Keyed::Product(PRODUCT))
            .seeing("portal", root.host_str().unwrap_or_default().to_string())
            .seeing("calendar", calendar.clone())
            .seeing(
                "documents",
                "each published meeting's agenda and minutes, and every document its agenda \
                 links",
            );
        if let Some(window) = window {
            r = r.seeing("window", window.to_string());
        }
        if !seed.robots.allowed(&calendar) {
            r = r.warning(
                "robots",
                "robots.txt turns away every crawler but six named search engines; nothing is \
                 collected unless the source sets `ignore_robots` (`source add --ignore-robots`)",
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
            let root =
                root_of(&at).ok_or_else(|| anyhow::anyhow!("{at} is not a CivicWeb portal"))?;
            let window = window_of(&at);

            let mut pass = Pass::new(crawl, "calendar", MAX_DEPTH);
            pass.note(Note::new("portal", root.to_string()));
            if let Some(w) = window {
                pass.note(Note::new("window", w.to_string()));
            }
            pass.push(Ask::calendar(&root, window.unwrap_or_else(everything)), 0);

            let mut tally: BTreeMap<&'static str, u64> = BTreeMap::new();
            let mut blocked = 0u64;
            while let Some((ask, depth)) = pass.next_to_visit() {
                pass.visiting(ask.to_string());
                if !seed.robots.allowed(ask.url.as_str()) {
                    blocked += 1;
                    continue;
                }
                let body = match pass.walk().get(ask.url.as_str()).await {
                    Ok(f) => f.bytes,
                    Err(refusal) => {
                        pass.refused(&ask, &refusal);
                        continue;
                    }
                };
                match ask.kind {
                    Kind::Calendar => {
                        let Ok(mut meetings) = serde_json::from_slice::<Vec<Meeting>>(&body) else {
                            pass.warn(format!("{ask}: the answer is not a list of meetings"));
                            continue;
                        };
                        pass.read_surface();
                        // The calendar answered newest first at every portal; sorted here so
                        // the order a short walk depends on is this walk's, not the server's.
                        meetings.sort_by(|a, b| b.at.cmp(&a.at));
                        *tally.entry("meetings").or_default() += meetings.len() as u64;
                        for meeting in meetings.iter().filter(|m| m.published) {
                            *tally.entry("published").or_default() += 1;
                            pass.push(Ask::meeting(&root, meeting.id), depth + 1);
                        }
                    }
                    Kind::Meeting => {
                        let Ok(documents) = serde_json::from_slice::<Vec<Document>>(&body) else {
                            pass.warn(format!("{ask}: the answer is not a meeting's documents"));
                            continue;
                        };
                        *tally.entry("opened").or_default() += 1;
                        let (released, held): (Vec<Document>, Vec<Document>) =
                            documents.into_iter().partition(Document::released);
                        *tally.entry("held back").or_default() += held.len() as u64;
                        for doc in released.iter().filter(|d| PDF.contains(&d.rendition)) {
                            if pass.full() {
                                break;
                            }
                            *tally.entry(doc.figure()).or_default() += 1;
                            pass.keep(document(&root, doc.id), &seed.robots);
                        }
                        for id in attachments(&released, &root) {
                            if pass.full() {
                                break;
                            }
                            *tally.entry("attachments").or_default() += 1;
                            pass.keep(document(&root, id), &seed.robots);
                        }
                    }
                }
            }

            if blocked > 0 {
                pass.warn(format!(
                    "{}'s robots.txt disallows {} request(s) this walk makes; if the documents \
                     are public records, `source add --ignore-robots` rules that it does not bind",
                    root.host_str().unwrap_or_default(),
                    crate::render::count(blocked)
                ));
            }
            for key in FIGURES {
                pass.figure(key, tally.get(key).copied().unwrap_or(0));
            }
            Ok(pass.finish())
        })
    }

    /// Every address is a document.
    fn addresses_are(&self) -> Addresses {
        Addresses::Documents
    }
}

/// The portal a seed is on: `https://myoldsmar.civicweb.net/`, from any address on it.
fn root_of(at: &Url) -> Option<Url> {
    let host = at.host_str()?.to_ascii_lowercase();
    let client = host.strip_suffix(DOMAIN)?;
    if client.is_empty() || NOT_PORTALS.contains(&client) {
        return None;
    }
    let mut root = at.clone();
    root.set_path("/");
    root.set_query(None);
    root.set_fragment(None);
    Some(root)
}

/// The range a calendar address asks for, when the seed is one.
fn window_of(at: &Url) -> Option<Window> {
    if !at.path().trim_matches('/').eq_ignore_ascii_case(CALENDAR) {
        return None;
    }
    let query: BTreeMap<_, _> = at.query_pairs().collect();
    let day = |key: &str| query.get(key)?.parse::<Date>().ok();
    let (from, to) = (day("from")?, day("to")?);
    (from <= to).then_some(Window { from, to })
}

/// Every meeting the calendar holds: the widest range it was measured to answer.
fn everything() -> Window {
    Window {
        from: jiff::civil::date(1900, 1, 1),
        to: jiff::civil::date(9999, 12, 31),
    }
}

fn calendar_at(root: &Url, window: Window) -> String {
    format!("{root}{CALENDAR}?from={}&to={}", window.from, window.to)
}

/// A document's one address; see the module docs for why the id alone.
fn document(root: &Url, id: u64) -> String {
    format!("{root}document/{id}/")
}

/// The id of the document an agenda link names, on the portal's host: `/document/{id}/…`,
/// or a Document Center preview, `/filepro/documents/{folder}/?preview={id}`.
fn document_id(link: &Url, root: &Url) -> Option<u64> {
    if link.host_str() != root.host_str() {
        return None;
    }
    let segments: Vec<&str> = link.path_segments()?.collect();
    match segments.as_slice() {
        [document, id, ..] if document.eq_ignore_ascii_case("document") => id.parse().ok(),
        [filepro, documents, ..]
            if filepro.eq_ignore_ascii_case("filepro")
                && documents.eq_ignore_ascii_case("documents") =>
        {
            link.query_pairs()
                .find(|(k, _)| k == "preview")
                .and_then(|(_, v)| v.parse().ok())
        }
        _ => None,
    }
}

/// The documents a meeting's HTML renditions link, in the order linked, each once.
///
/// Read from the anchors, never a scan of the text.
fn attachments(documents: &[Document], root: &Url) -> Vec<u64> {
    let mut ids: Vec<u64> = Vec::new();
    for html in documents.iter().filter_map(|d| d.html.as_deref()) {
        let scan = crate::html::Scan::new(html);
        let base = scan.base(root);
        for id in scan
            .tags(&["a"])
            .into_iter()
            .filter_map(|tag| base.join(tag.attr("href")?))
            .filter_map(|link| document_id(&link, root))
        {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// One request this walk makes.
#[derive(Clone, Debug)]
struct Ask {
    url: Url,
    kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// The calendar: every meeting in a range.
    Calendar,
    /// One meeting's documents.
    Meeting,
}

impl Ask {
    fn calendar(root: &Url, window: Window) -> Self {
        Self {
            url: Url::parse(&calendar_at(root, window)).expect("a portal address extends"),
            kind: Kind::Calendar,
        }
    }

    fn meeting(root: &Url, id: u64) -> Self {
        Self {
            url: root
                .join(&format!("{CALENDAR}/{id}/meetingDocuments"))
                .expect("a relative path joins"),
            kind: Kind::Meeting,
        }
    }
}

/// How it reads in a warning, and what [`Pass`] dedups on.
impl fmt::Display for Ask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.url.as_str())
    }
}

/// One meeting on the calendar.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Meeting {
    id: u64,
    /// `2026-11-17 19:00`, which sorts as it reads.
    #[serde(rename = "MeetingDateTime")]
    at: String,
    published: bool,
}

/// One of a meeting's published documents.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Document {
    id: u64,
    /// 1 an agenda, 2 minutes.
    #[serde(rename = "Type")]
    output: u32,
    /// Which rendition: see [`PDF`].
    #[serde(rename = "DocumentType")]
    rendition: u32,
    /// The whole document, for an HTML rendition.
    #[serde(default)]
    html: Option<String>,
    /// Set while the product holds the document back.
    #[serde(default)]
    public_release_delay_date_string: Option<String>,
}

impl Document {
    fn released(&self) -> bool {
        self.public_release_delay_date_string
            .as_deref()
            .is_none_or(str::is_empty)
    }

    /// The figure it is counted under.
    fn figure(&self) -> &'static str {
        match self.output {
            2 => "minutes",
            _ => "agendas",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::Robots;
    use crate::strategies::crawl::tests::{Fake, seed};

    const PORTAL: &str = "https://myoldsmar.civicweb.net/portal/";

    /// Oldsmar's portal page as served on 2026-11-23, cut to its icons, title, one
    /// navigation link and the footer link; the tags around them are closed by hand.
    const PORTAL_PAGE: &str = r#"<!DOCTYPE html>
<html id="ctl00_HtmlTag" prefix="fb: http://www.facebook.com/2008/fbml" lang="en-US" xml:lang="en-US"><head><link id="ctl00_favicon" rel="icon" href="https://i.civicweb.net/Global/Images/icon.png" /><link rel="apple-touch-icon" href="https://i.civicweb.net/Global/Images/MobileIcons/ic-icon-57.png" /><title>
	The City of Oldsmar - Home
</title></head><body>
<a href="/Portal/MeetingTypeList.aspx">Meetings</a>
<a id="ctl00_ICompassLink" href="http://www.icompasstech.com/" target="iCompass">©  iCompass - A Diligent Brand 2026</a></body></html>"#;

    const CALENDAR_ALL: &str = "https://myoldsmar.civicweb.net/Services/MeetingsService.svc/meetings?from=1900-01-01&to=9999-12-31";

    /// Oldsmar's calendar as it answered on 2026-11-23, cut to four of its 954 meetings,
    /// in the order served: a scheduled council meeting, the November 17 and February 17
    /// council meetings, and a 2024 committee meeting never published.
    const CALENDAR_ANSWER: &str = r#"[
{"ExternalCalendar":false,"Id":13060,"MeetingDate":"2027-12-21","MeetingDateTime":"2027-12-21 19:00","MeetingLocation":"Oldsmar Council Chamber","MeetingTime":"07:00 PM","Name":"CITY COUNCIL - 21 Dec 2027","Published":false,"TypeId":10,"VideoIcon":""},
{"ExternalCalendar":false,"Id":12968,"MeetingDate":"2026-11-17","MeetingDateTime":"2026-11-17 19:00","MeetingLocation":"Oldsmar Council Chamber","MeetingTime":"07:00 PM","Name":"CITY COUNCIL - 17 Nov 2026","Published":true,"TypeId":10,"VideoIcon":""},
{"ExternalCalendar":false,"Id":12950,"MeetingDate":"2026-02-17","MeetingDateTime":"2026-02-17 19:00","MeetingLocation":"Oldsmar Council Chamber","MeetingTime":"07:00 PM","Name":"CITY COUNCIL - 17 Feb 2026","Published":true,"TypeId":10,"VideoIcon":""},
{"ExternalCalendar":false,"Id":12833,"MeetingDate":"2024-08-20","MeetingDateTime":"2024-08-20 09:30","MeetingLocation":"Oldsmar Council Chamber","MeetingTime":"09:30 AM","Name":"TECHNICAL REVIEW COMMITTEE MINUTES- 20 Aug 2024","Published":false,"TypeId":1024,"VideoIcon":""}]"#;

    /// Meeting 12968's documents: the HTML agenda and the agenda PDF (the 141-page packet).
    /// The agenda's cover is emptied and its `Html` cut to three of its eighteen item
    /// links, each in the markup it was served in.
    const MEETING_12968: &str = r#"[
{"AgendaCover":"","DocumentType":1,"Html":"<p style=\"margin-top:0pt; margin-bottom:1pt; font-size:12pt\"><a target=\"AgendaAttachment\" href=\"/document/257868/2026%20Janice%20Miller%20Citizens%20Academy%20Graduation%20.pdf?handle=BB3444B050F447189A6F31F3ACBD66C0\" style=\"text-decoration:none\" target=\"AgendaAttachment\"><span style=\"text-decoration:underline;\">Agenda Item Report (AIR-4136) - Pdf</span></a></p><p style=\"margin-top:0pt; margin-bottom:1pt; font-size:12pt\"><a target=\"AgendaAttachment\" href=\"/document/257949/Consider%20approval%20of%20minutes%20of%20November%203,%20202.pdf?handle=F944F813414F45F7B6C84BD9CC1B6469\" style=\"text-decoration:none\" target=\"AgendaAttachment\"><span style=\"text-decoration:underline;\">Agenda Item Report (AIR-3752) - Pdf</span></a></p><p style=\"margin-top:0pt; margin-bottom:1pt; font-size:12pt\"><a target=\"AgendaAttachment\" href=\"/document/257141/PUBLIC%20HEARING%20and%20First%20Reading%20of%20Ordinance%202.pdf?handle=E3FEEAA1FFF34734958080E30A0937C1\" style=\"text-decoration:none\" target=\"AgendaAttachment\"><span style=\"text-decoration:underline;\">Agenda Item Report (AIR-4105) - Pdf</span></a></p>","Id":258034,"IsPublic":true,"MeetingId":12968,"Name":"CITY COUNCIL - 17 Nov 2026","PublicReleaseDelayDateString":"","Type":1},
{"AgendaCover":"","DocumentType":4,"Html":"","Id":258033,"IsPublic":false,"MeetingId":12968,"Name":"CITY COUNCIL - 17 Nov 2026","PublicReleaseDelayDateString":"","Type":1}]"#;

    /// Meeting 12950's documents: the HTML agenda cut to three of its 21 item links, the
    /// agenda PDF and the minutes PDF.
    const MEETING_12950: &str = r#"[
{"AgendaCover":"","DocumentType":1,"Html":"<p style=\"margin-top:0pt; margin-bottom:1pt; font-size:12pt\"><a target=\"AgendaAttachment\" href=\"/document/246100/Consider%20approval%20of%20minutes%20of%20February%203,%20202.pdf?handle=A1BC350EABBA4205A45DACC145723E4B\" style=\"text-decoration:none\" target=\"AgendaAttachment\"><span style=\"text-decoration:underline;\">Agenda Item Report (AIR-3733) - Pdf</span></a></p><p style=\"margin-top:0pt; margin-bottom:1pt; font-size:12pt\"><a target=\"AgendaAttachment\" href=\"/document/246103/Consider%20approval%20of%20minutes%20of%20January%2027,%20202.pdf?handle=62972BD687FB4D28BE9B4F1F5F854A70\" style=\"text-decoration:none\" target=\"AgendaAttachment\"><span style=\"text-decoration:underline;\">Agenda Item Report (AIR-3812) - Pdf</span></a></p><p style=\"margin-top:0pt; margin-bottom:1pt; font-size:12pt\"><a target=\"AgendaAttachment\" href=\"/document/246021/Consider%20approving%20payment%20to%20legal%20counsel%20for.pdf?handle=FC4F4CA2A5A44C2B9F44C05214B0E260\" style=\"text-decoration:none\" target=\"AgendaAttachment\"><span style=\"text-decoration:underline;\">Agenda Item Report (AIR-3821) - Pdf</span></a></p>","Id":246252,"IsPublic":true,"MeetingId":12950,"Name":"CITY COUNCIL - 17 Feb 2026","PublicReleaseDelayDateString":"","Type":1},
{"AgendaCover":"","DocumentType":4,"Html":"","Id":246251,"IsPublic":false,"MeetingId":12950,"Name":"CITY COUNCIL - 17 Feb 2026","PublicReleaseDelayDateString":"","Type":1},
{"AgendaCover":"","DocumentType":10,"Html":"","Id":247621,"IsPublic":false,"MeetingId":12950,"Name":"CITY COUNCIL - 17 Feb 2026","PublicReleaseDelayDateString":"","Type":2}]"#;

    /// The robots.txt every portal served on 2026-11-23, cut to the catch-all group and
    /// Googlebot's, three of whose ten `Disallow` lines are kept.
    const ROBOTS: &str = "User-agent: *  \nDisallow: /\n\nUser-agent: Googlebot\nDisallow: /FileStorage/\nDisallow: /WorkSpace/\nDisallow: /Content/\nCrawl-delay: 120\n";

    fn root() -> Url {
        Url::parse("https://myoldsmar.civicweb.net/").unwrap()
    }

    fn doc(id: u64) -> String {
        document(&root(), id)
    }

    fn served() -> Fake {
        Fake::new([
            (CALENDAR_ALL, CALENDAR_ANSWER.to_string()),
            (
                Ask::meeting(&root(), 12968).url.as_str(),
                MEETING_12968.to_string(),
            ),
            (
                Ask::meeting(&root(), 12950).url.as_str(),
                MEETING_12950.to_string(),
            ),
        ])
    }

    /// The calendar, then each published meeting newest first: its agenda and minutes
    /// PDFs, then every document its agenda links. The HTML agenda itself is not kept, and
    /// an unpublished meeting, scheduled or past, is never opened.
    #[tokio::test]
    async fn the_calendar_then_each_published_meeting_newest_first() {
        let crawl = served();

        let got = CivicWeb
            .enumerate(&seed(PORTAL_PAGE, PORTAL), &crawl)
            .await
            .unwrap();

        assert_eq!(
            got.addresses,
            [
                doc(258033),
                doc(257868),
                doc(257949),
                doc(257141),
                doc(246251),
                doc(247621),
                doc(246100),
                doc(246103),
                doc(246021),
            ]
        );
        assert_eq!(crawl.requests(), 3, "the calendar and two meetings");
        assert_eq!(got.figures["meetings"], 4);
        assert_eq!(got.figures["published"], 2);
        assert_eq!(got.figures["agendas"], 2);
        assert_eq!(got.figures["minutes"], 1);
        assert_eq!(got.figures["attachments"], 6);
        assert!(
            !got.truncated && !got.discovery_failed && got.warnings.is_empty(),
            "{:?}",
            got.warnings
        );
    }

    /// A budget that runs out keeps everything of the newest meeting and says it stopped.
    #[tokio::test]
    async fn a_walk_out_of_budget_keeps_the_newest_meetings() {
        let crawl = served().with_budget(2);

        let got = CivicWeb
            .enumerate(&seed(PORTAL_PAGE, PORTAL), &crawl)
            .await
            .unwrap();

        assert_eq!(
            got.addresses,
            [doc(258033), doc(257868), doc(257949), doc(257141)]
        );
        assert!(got.truncated, "{:?}", got.warnings);
    }

    /// One address per document. The name and `handle` an agenda writes are decorative —
    /// each of these answered the same 100,575 bytes — and a Document Center preview names
    /// the same kind of document. Largo's agenda for May 21, 2024 (meeting 14253, as served
    /// on 2026-11-23, cut to three of its fourteen links with the markup around each and to
    /// the fields read) links an item report, the previous meeting's minutes, and a
    /// Document Center preview.
    #[test]
    fn a_document_has_one_address_whatever_the_link_says() {
        let at = |path: &str| root().join(path).unwrap();
        for link in [
            "/document/257868/2026%20Janice%20Miller%20Citizens%20Academy%20Graduation%20.pdf?handle=BB3444B050F447189A6F31F3ACBD66C0",
            "/document/257868/x.pdf?handle=BB3444B050F447189A6F31F3ACBD66C0",
            "/document/257868/2026%20Janice%20Miller%20Citizens%20Academy%20Graduation%20.pdf?handle=00000000000000000000000000000000",
            "/document/257868",
            "/document/257868/",
        ] {
            assert_eq!(document_id(&at(link), &root()), Some(257868), "{link}");
        }
        assert_eq!(
            document_id(&at("https://largo.civicweb.net/document/257868/"), &root()),
            None,
            "another portal's document is not this one's"
        );

        let largo = Url::parse("https://largo.civicweb.net/").unwrap();
        let docs: Vec<Document> = serde_json::from_str(
            r#"[
{"DocumentType":1,"Html":"<p style=\"margin-top:0pt; margin-bottom:1pt; font-size:10pt\"><a target=\"AgendaAttachment\" href=\"/document/308011/Recognition%20Of%20The%202024%20Solid%20Waste%20Association.pdf?handle=DEF3D73D1DDB405D9DC317D17B3181E2\" title=\"ItemAttachment308011\" style=\"text-decoration:none\" target=\"AgendaAttachment\"><span style=\"text-decoration:underline;\">Agenda Memo #24-183 - Pdf</span></a></p><p style=\"margin-top:0pt; margin-bottom:1pt; font-size:10pt\"><a target=\"AgendaAttachment\" href=\"/document/308725/20240507_Minutes.pdf?handle=AA56A49F20D447C48B9841893D5C5D56\" title=\"ItemAttachment308725\" style=\"text-decoration:none\" target=\"AgendaAttachment\"><span style=\"text-decoration:underline;\">20240507_Minutes</span></a></p><p style=\"margin-top:0pt; margin-bottom:1pt; font-size:10pt\"><a target=\"AgendaAttachment\" href=\"/filepro/documents/303857/?preview=308726\" style=\"text-decoration:none\" target=\"AgendaAttachment\"><span style=\"text-decoration:underline;\">Largo - Document Center (civicweb.net)</span></a></p>","Id":308730,"PublicReleaseDelayDateString":"","Type":1},
{"DocumentType":4,"Html":"","Id":308729,"PublicReleaseDelayDateString":"","Type":1}]"#,
        )
        .unwrap();
        let ids = attachments(&docs, &largo);
        assert_eq!(ids, [308011, 308725, 308726]);
        assert_eq!(
            document(&largo, ids[2]),
            "https://largo.civicweb.net/document/308726/"
        );
    }

    /// A calendar address with a range is its own bound: the walk asks for that range.
    #[tokio::test]
    async fn a_calendar_seed_asks_for_its_range() {
        let at = "https://largo.civicweb.net/Services/MeetingsService.svc/meetings?from=2026-08-22&to=2026-11-20";
        let crawl = Fake::new([(at, "[]".to_string())]);

        let got = CivicWeb.enumerate(&seed("[]", at), &crawl).await.unwrap();

        assert_eq!(crawl.requests(), 1);
        assert!(!got.discovery_failed, "{:?}", got.warnings);
        assert!(got.notes.iter().any(|n| n.label == "window"));
    }

    /// Every portal's robots.txt turns this crawler away. Honoured, nothing is asked and
    /// the run says why; overruled by the operator, the walk is the same as with no rules.
    #[tokio::test]
    async fn robots_txt_turns_the_walk_away_unless_overruled() {
        let robots = || Robots::parse("centinel", ROBOTS.as_bytes());

        let honoured = Seed {
            robots: robots(),
            ..seed(PORTAL_PAGE, PORTAL)
        };
        let crawl = served();
        let got = CivicWeb.enumerate(&honoured, &crawl).await.unwrap();
        assert_eq!(crawl.requests(), 0);
        assert!(got.addresses.is_empty());
        assert!(
            got.warnings.iter().any(|w| w.contains("--ignore-robots")),
            "{:?}",
            got.warnings
        );
        assert!(
            CivicWeb
                .recognise(&honoured)
                .is_some_and(|r| r.warnings.iter().any(|n| n.label == "robots"))
        );

        let overruled = Seed {
            robots: robots().overruled(),
            ..seed(PORTAL_PAGE, PORTAL)
        };
        let got = CivicWeb.enumerate(&overruled, &served()).await.unwrap();
        assert_eq!(got.addresses.len(), 9);
    }

    /// Recognised on a portal page by the product's asset host, and at the calendar address
    /// by its path. A page on another host, the vendor's own site, or a portal host serving
    /// something else is not a portal.
    #[test]
    fn a_portal_is_recognised_on_its_bytes() {
        let r = CivicWeb
            .recognise(&seed(PORTAL_PAGE, PORTAL))
            .expect("a CivicWeb portal");
        assert_eq!(r.keyed_on, Keyed::Product(PRODUCT));
        assert!(
            r.evidence
                .iter()
                .any(|n| n.detail.contains("myoldsmar.civicweb.net"))
        );

        let range = "https://largo.civicweb.net/Services/MeetingsService.svc/meetings?from=2026-08-22&to=2026-11-20";
        let r = CivicWeb.recognise(&seed("[]", range)).expect("a calendar");
        assert!(r.evidence.iter().any(|n| n.label == "window"));

        for not in [
            "https://madeirabeachfl.gov/meetings/",
            "https://www.civicweb.net/",
            "https://i.civicweb.net/Global/Images/icon.png",
        ] {
            assert!(
                CivicWeb.recognise(&seed(PORTAL_PAGE, not)).is_none(),
                "{not}"
            );
        }
        assert!(
            CivicWeb
                .recognise(&seed("<html>parked</html>", PORTAL))
                .is_none(),
            "a portal is confirmed on its bytes"
        );
    }
}
