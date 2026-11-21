//! `onbase-agenda` — Hyland OnBase Agenda Online's meetings, and the documents each one
//! publishes.
//!
//! Keyed on a **product**, at three sightings: Tampa (`tampagov.hylandcloud.com/251agendaonline/`),
//! Manatee County (`agendaonline.mymanatee.org/OnBaseAgendaOnline/`) and Osceola County
//! (`docs.osceola.org/OnBaseAgendaOnline`). Hosted or on a county's own server, the
//! application is the same, so it is recognised by what it serves and not by where.
//!
//! ## Where the meetings are
//!
//! Not in a link. The home page passes its meeting list to a script —
//! `showSearchResults(new SearchResults({"Meetings":[…]}))` — and the browser builds every
//! address from it (`docs/FIELD-NOTES.md` entry 2). The same literal comes back from the
//! search page, which is a plain GET:
//!
//! ```text
//! Meetings/Search?dropid=11&dropsv=08/22/2026&dropev=11/20/2026
//! ```
//!
//! `dropid=11` is "custom date range". Each meeting carries its id, name, time, and a flag
//! per document saying whether it is published.
//!
//! ## The search answers at most a hundred, and now says so
//!
//! Entry 2 measured the cap — all of 2024 answered 100 meetings, its two halves 77 and 57 —
//! and recorded that the answer carried no marker. It carries one now: a capped answer
//! sets `"WarningCode":"SEARCHLIMITHIT"`. So the walk asks for a window and halves any
//! window that comes back capped, newest half first, until every answer is whole. A window
//! one day wide that is still capped is reported short rather than trusted.
//!
//! ## Which documents
//!
//! `Documents/DownloadFileBytes/<name>.pdf?documentType=N&meetingId=ID` serves the PDF, and
//! only the query decides which: the name is decorative, so it is written as the meeting id
//! and one document has one address. `N` is the product's own `PublishDocType`: 1 agenda,
//! 2 minutes, 3 summary. An address is made only where the meeting says the document is
//! published, because asking for one that is not answers 200 with an HTML error page typed
//! `application/pdf`.
//!
//! The PDF over the `ViewAgenda` HTML, for entry 2's reason: the HTML export inlines the
//! letterhead as base64, and the PDF read more real text out of the same minutes.
//!
//! ## What the council voted on: each item's supporting documents
//!
//! The agreements, contracts and staff reports are attached to agenda items, not to the
//! meeting, and the agenda PDF carries only their titles. They are three requests down
//! (`docs/FIELD-NOTES.md` entry 8):
//!
//! 1. `Documents/ViewAgenda?meetingId=ID&doctype=1`, the agenda as HTML, names each item as
//!    `loadAgendaItem(ITEM,false)`; `true` marks a section, and no section seen at Tampa,
//!    Manatee or Osceola carried a document, so sections are not asked for.
//! 2. `Meetings/ViewMeetingAgendaItem?meetingId=ID&itemId=ITEM&isSection=false&type=agenda`
//!    lists the item's "Supporting Documents", each linked as
//!    `Documents/DownloadFile/<name>.pdf?documentType=1&meetingId=…&itemId=…&publishId=…&isSection=False&isAttachment=True`.
//! 3. That address is a "Downloading, Please wait…" page whose script swaps in
//!    `DownloadFileBytes`. The same query on `DownloadFileBytes` is the file.
//!
//! `documentType`, `meetingId`, `itemId` and `publishId` decide which file; the name and the
//! two flags do not. So an attachment is written with the publish id as its name and those
//! four keys alone — one document, one address. Drop `itemId` and the server answers with
//! the meeting's agenda instead, so the four are never trimmed further.
//!
//! The items cost one request each, which is most of a walk: a Tampa council agenda runs to
//! about a hundred items. (An agenda uploaded as a finished document — every Tampa agenda
//! before 2024 — names none, and costs its one request.) So the meetings' own documents are
//! all kept first, from the search answers alone, and then meetings are opened **one at a
//! time, newest first**, each agenda followed by its items. A walk that runs out of requests
//! has every agenda, minutes and summary, every attachment of the newest meetings it
//! finished, and at most one meeting part-read.
//!
//! ## Bounding a walk
//!
//! The seed bounds it, as a path bounds `links`. Handed the application's front door, the
//! walk covers every meeting since [`EARLIEST`]. Handed a search page —
//! `…/Meetings/Search?dropid=11&dropsv=08/22/2026&dropev=11/20/2026` — it covers that range,
//! and the first answer is the seed already in hand. The request budget (`--max-sitemaps`)
//! still applies inside that range, and an item walk spends it: May 2026 at Tampa is 17
//! agendas and 232 items, 249 requests, more than the default budget of 200.

use std::fmt;

use futures::future::BoxFuture;
use jiff::civil::Date;
use serde::Deserialize;
use url::Url;

use super::window::Window;
use super::{Addresses, Enumerated, Pass, Seed, Strategy, StrategyDef, Walk};
use crate::domain::Note;
use crate::strategies::{Keyed, Recognition};

pub struct OnBaseAgenda;

inventory::submit! { StrategyDef { name: "onbase-agenda", it: &OnBaseAgenda } }

/// The product names itself in every page's `<title>`: `Home - OnBase Agenda Online`.
const PRODUCT: &str = "OnBase Agenda Online";

/// The two addresses the application's scripts build, either of which gives away where the
/// application is mounted: `/251agendaonline/`, `/OnBaseAgendaOnline/`.
const MOUNTED: [&str; 2] = ["/Meetings/ViewMeeting?id=", "/Meetings/Search?dropid="];

/// Where a walk of the whole application starts. Before any deployment seen; an empty
/// window costs one request and finds nothing.
const EARLIEST: (i16, i8, i8) = (2000, 1, 1);

/// How far ahead to look: meetings are published before they are held.
const AHEAD_DAYS: i64 = 366;

/// The answer the search gives when it stopped at its cap.
const CAPPED: &str = "SEARCHLIMITHIT";

/// The cap itself, for a deployment too old to say it hit one.
const CAP: usize = 100;

/// Halvings of the first window. Twenty-seven years halve to one day in fourteen.
const MAX_DEPTH: usize = 24;

impl Strategy for OnBaseAgenda {
    fn name(&self) -> &'static str {
        "onbase-agenda"
    }

    fn recognise(&self, seed: &Seed) -> Option<Recognition> {
        let at = seed.final_url()?;
        let page = seed.text();
        if !page.contains(PRODUCT) {
            return None;
        }
        let root = mounted_at(&page, &at)?;
        let mut r = Recognition::new(self.name(), Keyed::Product("Hyland OnBase Agenda Online"))
            .seeing("title", format!("the page names itself {PRODUCT}"))
            .seeing("application", format!("mounted at {root}"));
        if let Some(results) = results_in(&page) {
            r = r.seeing(
                "meetings",
                format!("{} in the list the page carries", results.meetings.len()),
            );
        }
        let window = window_of(&at).unwrap_or_else(whole);
        Some(r.seeing("window", window.to_string()))
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
            let page = seed.text();
            let root = mounted_at(&page, &at).ok_or_else(|| {
                anyhow::anyhow!("{at} does not say where OnBase Agenda Online is mounted")
            })?;
            // A search the seed already answered is not asked again.
            let seeded = window_of(&at);
            let first = seeded.unwrap_or_else(whole);

            let mut pass = Pass::new(crawl, "meeting search", MAX_DEPTH);
            pass.note(Note::new("application", root.to_string()));
            pass.note(Note::new("window", first.to_string()));
            pass.push(Ask::Search(first), 0);

            // Meetings whose agenda is published, opened once every search is answered;
            // see the module docs. Sorted oldest first, so `pop` takes the newest.
            let mut agendas: Vec<(Option<jiff::Timestamp>, u64)> = Vec::new();
            let mut tally = Tally::default();
            loop {
                let Some((ask, depth)) = pass.next_to_visit() else {
                    // Nothing queued: open the next meeting. A ceiling leaves the queue
                    // holding what it stopped before, and ends the walk here.
                    if pass.pending() == 0
                        && let Some((_, meeting)) = agendas.pop()
                    {
                        pass.push(Ask::Agenda(meeting), 0);
                        continue;
                    }
                    break;
                };
                pass.visiting(match &ask {
                    Ask::Search(window) => format!("meetings {window}"),
                    _ => ask.to_string(),
                });
                let body = match (&ask, depth, seeded) {
                    (Ask::Search(_), 0, Some(_)) => seed.page.bytes.clone(),
                    _ => match pass.walk().get(ask.url(&root).as_str()).await {
                        Ok(f) => f.bytes,
                        Err(refusal) => {
                            pass.refused(&ask, &refusal);
                            continue;
                        }
                    },
                };
                let text = String::from_utf8_lossy(&body);
                match &ask {
                    Ask::Search(window) => {
                        let Some(results) = results_in(&text) else {
                            pass.warn(format!("{window}: the answer carries no meeting list"));
                            continue;
                        };
                        if results.error_occurred {
                            pass.warn(format!(
                                "{window}: the search reported an error ({})",
                                results.error_code.as_deref().unwrap_or("no code")
                            ));
                            continue;
                        }
                        pass.read_surface();

                        if results.capped() {
                            match window.halves() {
                                Some((older, newer)) => {
                                    pass.push(Ask::Search(newer), depth + 1);
                                    pass.push(Ask::Search(older), depth + 1);
                                    continue;
                                }
                                None => pass.short(format!(
                                    "{window}: one day holds more meetings than the search \
                                     answers ({}); the rest of that day is not collected",
                                    results.meetings.len()
                                )),
                            }
                        }

                        tally.windows += 1;
                        for meeting in &results.meetings {
                            tally.meetings += 1;
                            if meeting.is_agenda_available {
                                agendas.push((meeting.held(), meeting.id));
                            }
                            for (doc_type, published) in meeting.published() {
                                if !published {
                                    continue;
                                }
                                if pass.full() {
                                    break;
                                }
                                tally.documents += 1;
                                pass.keep(document(&root, meeting.id, doc_type), &seed.robots);
                            }
                        }
                        agendas.sort();
                    }
                    &Ask::Agenda(meeting) => {
                        // A bid opening's agenda can hold no items at all, so an empty
                        // list is a figure and not a warning.
                        let items = items_in(&text);
                        tally.agendas += 1;
                        tally.items += items.len() as u64;
                        for item in items {
                            pass.push(Ask::Item { meeting, item }, 0);
                        }
                    }
                    Ask::Item { .. } => {
                        let at = Url::parse(&ask.url(&root))?;
                        for address in attachments_in(&text, &at, &root) {
                            if pass.full() {
                                break;
                            }
                            tally.attachments += 1;
                            pass.keep(address, &seed.robots);
                        }
                    }
                }
            }

            pass.figure("meetings", tally.meetings);
            pass.figure("documents", tally.documents);
            pass.figure("windows", tally.windows);
            pass.figure("agendas", tally.agendas);
            pass.figure("items", tally.items);
            pass.figure("attachments", tally.attachments);
            Ok(pass.finish())
        })
    }

    /// Each address is the PDF itself, and scanning one is how entry 2's invented address
    /// came about.
    fn addresses_are(&self) -> Addresses {
        Addresses::Documents
    }
}

/// `…/Documents/DownloadFileBytes/<id>.pdf?documentType=N&meetingId=<id>`.
///
/// The filename segment is ignored by the server, so it is the meeting id rather than the
/// meeting's name: a clerk renaming *Proposed* minutes to *Approved* must not move the
/// document to a new address.
fn document(root: &Url, meeting: u64, doc_type: u8) -> String {
    format!(
        "{root}Documents/DownloadFileBytes/{meeting}.pdf?documentType={doc_type}&meetingId={meeting}"
    )
}

/// One request this walk makes.
#[derive(Clone, Copy, Debug)]
enum Ask {
    /// The meetings held in a range of days.
    Search(Window),
    /// One meeting's agenda as HTML, for the items it names.
    Agenda(u64),
    /// One item's page, for its supporting documents.
    Item { meeting: u64, item: u64 },
}

impl Ask {
    fn url(&self, root: &Url) -> String {
        match self {
            Self::Search(window) => search_at(root, window),
            Self::Agenda(meeting) => {
                format!("{root}Documents/ViewAgenda?meetingId={meeting}&doctype=1")
            }
            Self::Item { meeting, item } => format!(
                "{root}Meetings/ViewMeetingAgendaItem?meetingId={meeting}&itemId={item}\
                 &isSection=false&type=agenda"
            ),
        }
    }
}

/// How it reads in a warning, and what [`Pass`] dedups on.
impl fmt::Display for Ask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // The window alone, as a refused search has always been reported.
            Self::Search(window) => write!(f, "{window}"),
            Self::Agenda(meeting) => write!(f, "agenda of meeting {meeting}"),
            Self::Item { meeting, item } => write!(f, "item {item} of meeting {meeting}"),
        }
    }
}

#[derive(Default)]
struct Tally {
    meetings: u64,
    documents: u64,
    windows: u64,
    agendas: u64,
    items: u64,
    attachments: u64,
}

/// The items an agenda names, in agenda order: `loadAgendaItem(23596,false)`.
///
/// Sections, `loadAgendaItem(10996,true)`, are left out; see the module docs.
fn items_in(agenda: &str) -> Vec<u64> {
    const CALL: &str = "loadAgendaItem(";
    let mut seen = std::collections::HashSet::new();
    agenda
        .match_indices(CALL)
        .filter_map(|(i, _)| {
            let rest = &agenda[i + CALL.len()..];
            let (id, flag) = rest.split_once(',')?;
            let id = id.trim().parse::<u64>().ok()?;
            flag.trim_start().starts_with("false)").then_some(id)
        })
        .filter(|id| seen.insert(*id))
        .collect()
}

/// Each supporting document an item page links, as the address that serves the file.
///
/// Read from the page's anchors, never a scan of its text, and only the links into this
/// application's `Documents/DownloadFile/`. See the module docs for why the address is
/// rewritten to `DownloadFileBytes` and cut to four keys.
fn attachments_in(page: &str, at: &Url, root: &Url) -> Vec<String> {
    let scan = crate::html::Scan::new(page);
    let base = scan.base(at);
    let under = format!("{}Documents/DownloadFile/", root.path());
    scan.tags(&["a"])
        .into_iter()
        .filter_map(|tag| base.join(tag.attr("href")?))
        .filter(|u| u.host_str() == root.host_str() && u.path().starts_with(&under))
        .filter_map(|u| attachment(root, &u))
        .collect()
}

/// `…/Documents/DownloadFileBytes/<publish>.pdf?documentType=N&meetingId=M&itemId=I&publishId=P`,
/// from the `DownloadFile` link an item page carries. `None` unless all four keys are there
/// and numeric.
fn attachment(root: &Url, link: &Url) -> Option<String> {
    let query: std::collections::BTreeMap<_, _> = link.query_pairs().collect();
    let key = |k: &str| query.get(k)?.parse::<u64>().ok();
    let (doc_type, meeting, item, publish) = (
        key("documentType")?,
        key("meetingId")?,
        key("itemId")?,
        key("publishId")?,
    );
    Some(format!(
        "{root}Documents/DownloadFileBytes/{publish}.pdf?documentType={doc_type}\
         &meetingId={meeting}&itemId={item}&publishId={publish}"
    ))
}

/// Where the application is mounted, read off the addresses its scripts build.
///
/// `"/251agendaonline/Meetings/ViewMeeting?id="` gives `/251agendaonline/`. A parse of one
/// quoted string, never a scan for something URL-shaped: entry 2 is what a scan of this
/// page's scripts produces.
fn mounted_at(page: &str, at: &Url) -> Option<Url> {
    MOUNTED.iter().find_map(|marker| {
        let i = page.find(marker)?;
        let start = page[..i].rfind(['"', '\''])? + 1;
        let prefix = &page[start..i];
        if !prefix.starts_with('/') || prefix.contains(char::is_whitespace) {
            return None;
        }
        at.join(&format!("{prefix}/")).ok()
    })
}

/// The meeting list a page carries, if it carries one.
fn results_in(page: &str) -> Option<Results> {
    const CALL: &str = "new SearchResults(";
    let start = page
        .match_indices(CALL)
        .map(|(i, _)| i + CALL.len())
        .find(|&i| page[i..].starts_with('{'))?;
    // The literal is followed by `))` and the rest of the script, so only its first value
    // is read.
    serde_json::Deserializer::from_str(&page[start..])
        .into_iter::<Results>()
        .next()?
        .ok()
}

/// What the search answers, as far as this walk needs it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Results {
    #[serde(default)]
    meetings: Vec<Meeting>,
    #[serde(default)]
    warning_code: Option<String>,
    #[serde(default)]
    error_occurred: bool,
    #[serde(default)]
    error_code: Option<String>,
}

impl Results {
    /// The search stopped at its cap. Said by the answer where the deployment says it, and
    /// inferred from the count where it does not.
    fn capped(&self) -> bool {
        self.warning_code.as_deref() == Some(CAPPED) || self.meetings.len() >= CAP
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Meeting {
    #[serde(rename = "ID")]
    id: u64,
    #[serde(default)]
    is_agenda_available: bool,
    #[serde(default)]
    is_minutes_available: bool,
    #[serde(default)]
    is_summary_available: bool,
    /// When it is held, `2026-10-15T09:00:00-04:00`. Kept as text, so a deployment that
    /// writes it another way loses only the ordering and not the meeting.
    #[serde(default)]
    time: Option<String>,
}

impl Meeting {
    /// Each document type, numbered as the product's `PublishDocType` numbers it, and
    /// whether this meeting publishes it.
    fn published(&self) -> [(u8, bool); 3] {
        [
            (1, self.is_agenda_available),
            (2, self.is_minutes_available),
            (3, self.is_summary_available),
        ]
    }

    /// When it is held, for opening the newest meetings first.
    fn held(&self) -> Option<jiff::Timestamp> {
        self.time.as_deref()?.parse().ok()
    }
}

/// The search for a window, as the application's own form builds it.
fn search_at(root: &Url, window: &Window) -> String {
    format!(
        "{root}Meetings/Search?dropid=11&dropsv={}&dropev={}",
        window.from.strftime("%m/%d/%Y"),
        window.to.strftime("%m/%d/%Y")
    )
}

/// Every meeting the application could hold: [`EARLIEST`] to a year from today.
fn whole() -> Window {
    let today = jiff::Zoned::now().date();
    let (y, m, d) = EARLIEST;
    Window {
        from: jiff::civil::date(y, m, d),
        to: today.saturating_add(jiff::Span::new().days(AHEAD_DAYS)),
    }
}

/// The range a search address asks for, when the seed is one.
fn window_of(at: &Url) -> Option<Window> {
    if !at.path().ends_with("/Meetings/Search") {
        return None;
    }
    let query: std::collections::BTreeMap<_, _> = at.query_pairs().collect();
    if query.get("dropid").map(|v| &**v) != Some("11") {
        return None;
    }
    let day = |key: &str| Date::strptime("%m/%d/%Y", query.get(key)?.as_bytes()).ok();
    let (from, to) = (day("dropsv")?, day("dropev")?);
    (from <= to).then_some(Window { from, to })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::crawl::tests::{Fake, seed};

    const ROOT: &str = "https://tampagov.hylandcloud.com/251agendaonline/";

    /// A search answer as the application serves it, cut to the parts this reads, with
    /// the meetings given and the warning set or not.
    fn answer(meetings: &[(u64, bool, bool, bool)], capped: bool) -> String {
        let list: Vec<String> = meetings
            .iter()
            .map(|(id, agenda, minutes, summary)| {
                format!(
                    r#"{{"ID":{id},"Name":"City Council Regular - October 15, 2026","MeetingTypeName":"Council Regular","IsAgendaAvailable":{agenda},"IsAgendaPacketAvailable":false,"IsSummaryAvailable":{summary},"IsMinutesAvailable":{minutes},"Time":"2026-10-15T09:00:00-04:00","Documents":[],"AgendaUniqueName":"City_Council_Regular_-_October_15,_2026_{id}_Agenda","LatestDocumentType":2}}"#
                )
            })
            .collect();
        let warning = match capped {
            true => r#""WarningOccurred":true,"WarningCode":"SEARCHLIMITHIT""#,
            false => r#""WarningOccurred":false,"WarningCode":"""#,
        };
        format!(
            r##"<html><head><title>Search Meeting Content - OnBase Agenda Online</title></head><body>
<script>
	function createMeetingLink(meeting, type, docType, titleFormat, text, linkSize) {{
		return $("<a>").attr("href", "/251agendaonline/Meetings/ViewMeeting?id=" + meeting.ID + "&doctype=" + docType);
	}}
	$(function () {{
		$.post($("#getMoreResultsForm")[0].getAttribute("action")).done(function (data) {{
			let searchResults = new SearchResults(data);
		}});
		showSearchResults(new SearchResults({{"MeetingTypes":[{{"ID":"104","Name":"Council Regular"}}],"DateRangeOptionID":11,"Meetings":[{}],"NoMoreResults":false,"ErrorOccurred":false,"ErrorCode":"",{warning},"HasError":false}}));
	}});
</script></body></html>"##,
            list.join(",")
        )
    }

    fn search(from: &str, to: &str) -> String {
        format!("{ROOT}Meetings/Search?dropid=11&dropsv={from}&dropev={to}")
    }

    fn agenda_of(meeting: u64) -> String {
        format!("{ROOT}Documents/ViewAgenda?meetingId={meeting}&doctype=1")
    }

    fn item_of(meeting: u64, item: u64) -> String {
        format!(
            "{ROOT}Meetings/ViewMeetingAgendaItem?meetingId={meeting}&itemId={item}&isSection=false&type=agenda"
        )
    }

    /// An agenda that names no items, as a bid opening's does.
    const NO_ITEMS: &str = "<html><body><p>Bid Opening</p></body></html>";

    /// Meeting 2824's agenda as Tampa served it on 2026-11-21, cut to a section and two
    /// items. The item ids sit in `javascript:` links; nothing else on the page names them.
    const AGENDA_2824: &str = r#"<div><h3><a href="javascript:loadAgendaItem(10996,true);">Consent Agenda</a></h3>
<td><h4><a name="I23596"></a><a href="javascript:loadAgendaItem(23596,false);" style="text-decoration:none"><span>File No. DEO26-23596</span></a></h4><h4><span>Resolution</span><span> approving a Subdivision Agreement … Water Street Tampa Subphase 3A</span></h4></td>
<td><h4><a name="I23601"></a><a href="javascript:loadAgendaItem(23601,false);"><span>File No. CM26-23601</span></a></h4></td>
<td><a href="javascript:loadAgendaItem(23596,false);">again, in the outline</a></td></div>"#;

    /// Item 23596's page, cut to its navigation and its two supporting documents, links
    /// escaped as served.
    const ITEM_23596: &str = r##"<html><head><title> - OnBase Agenda Online</title></head><body>
<a href="/251agendaonline/Meetings" id="lnkNavMeetings">Meetings</a>
<h2 class="header item-view-title">Item Details</h2><span class="item-view-title-text">File No. DEO26-23596</span>
<h2 class="header item-view-header">Supporting Documents</h2>
<a href="/251agendaonline/Documents/DownloadFile/Summary%20Sheet-%20COVER%20SHEET.pdf?documentType=1&amp;meetingId=2824&amp;itemId=23596&amp;publishId=192327&amp;isSection=False&amp;isAttachment=True" id="lnkAttachment_6885432" target="_blank">Summary Sheet- COVER SHEET</a>
<a href="/251agendaonline/Documents/DownloadFile/RESO%20AND%20AGMT%20-%20SUBDIVISION%20FPT-24-0000003%20-%20WATER%20STREET%203A.PDF.pdf?documentType=1&amp;meetingId=2824&amp;itemId=23596&amp;publishId=192328&amp;isSection=False&amp;isAttachment=True" id="lnkAttachment_6885433" target="_blank">RESO AND AGMT - SUBDIVISION FPT-24-0000003 - WATER STREET 3A.PDF</a>
<a href="#" onclick="focusBackToTreeItem(); return false;">Back to Meeting Outline</a>
</body></html>"##;

    /// An item with nothing attached.
    const ITEM_BARE: &str =
        r#"<html><body><h2 class="header item-view-title">Item Details</h2></body></html>"#;

    fn attached(meeting: u64, item: u64, publish: u64) -> String {
        format!(
            "{ROOT}Documents/DownloadFileBytes/{publish}.pdf?documentType=1&meetingId={meeting}&itemId={item}&publishId={publish}"
        )
    }

    #[test]
    fn the_application_is_recognised_wherever_it_is_mounted() {
        let s = seed(&answer(&[(2936, true, true, false)], false), ROOT);
        let r = OnBaseAgenda.recognise(&s).expect("OnBase Agenda Online");
        assert_eq!(r.keyed_on, Keyed::Product("Hyland OnBase Agenda Online"));
        assert!(
            r.evidence
                .iter()
                .any(|n| n.detail == format!("mounted at {ROOT}")),
            "{:?}",
            r.evidence
        );

        // Manatee County runs it on its own host, under another name.
        let manatee = answer(&[], false).replace("/251agendaonline/", "/OnBaseAgendaOnline/");
        let s = seed(
            &manatee,
            "https://agendaonline.mymanatee.org/OnBaseAgendaOnline/",
        );
        assert!(OnBaseAgenda.recognise(&s).is_some());

        let s = seed("<html><title>Agenda Center</title></html>", ROOT);
        assert!(OnBaseAgenda.recognise(&s).is_none());
    }

    /// A search seed is a window already answered: no search is asked, and there is an
    /// address per document the meeting publishes — never one it does not. The only
    /// requests are the agendas of the two meetings that publish one, read for items.
    #[tokio::test]
    async fn a_search_seed_is_its_own_first_answer() {
        let at = search("08/22/2026", "11/20/2026");
        let s = seed(
            &answer(
                &[
                    (2936, true, true, false),
                    (3007, false, false, false),
                    (2950, true, false, true),
                ],
                false,
            ),
            &at,
        );
        let crawl = Fake::new([
            (agenda_of(2936).as_str(), NO_ITEMS.to_string()),
            (agenda_of(2950).as_str(), NO_ITEMS.to_string()),
        ]);

        let got = OnBaseAgenda.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                format!("{ROOT}Documents/DownloadFileBytes/2936.pdf?documentType=1&meetingId=2936"),
                format!("{ROOT}Documents/DownloadFileBytes/2936.pdf?documentType=2&meetingId=2936"),
                format!("{ROOT}Documents/DownloadFileBytes/2950.pdf?documentType=1&meetingId=2950"),
                format!("{ROOT}Documents/DownloadFileBytes/2950.pdf?documentType=3&meetingId=2950"),
            ]
        );
        assert_eq!(got.figures["meetings"], 3);
        assert_eq!(
            crawl.requests(),
            2,
            "no search, since the seed answered it; one agenda per meeting publishing one"
        );
        assert!(!got.truncated && !got.discovery_failed);
    }

    /// The search answers at most a hundred and says so. A capped window is halved, and the
    /// halves are asked in turn until neither is capped.
    #[tokio::test]
    async fn a_capped_window_is_halved_until_every_answer_is_whole() {
        let at = search("01/01/2024", "12/31/2024");
        let capped: Vec<_> = (1..=100).map(|id| (id, true, false, false)).collect();
        let s = seed(&answer(&capped, true), &at);
        let crawl = Fake::new([
            (
                search("01/01/2024", "07/01/2024").as_str(),
                answer(&[(117, true, true, false)], false),
            ),
            (
                search("07/02/2024", "12/31/2024").as_str(),
                answer(&[(2555, true, false, false)], false),
            ),
            (agenda_of(117).as_str(), NO_ITEMS.to_string()),
            (agenda_of(2555).as_str(), NO_ITEMS.to_string()),
        ]);

        let got = OnBaseAgenda.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                format!("{ROOT}Documents/DownloadFileBytes/2555.pdf?documentType=1&meetingId=2555"),
                format!("{ROOT}Documents/DownloadFileBytes/117.pdf?documentType=1&meetingId=117"),
                format!("{ROOT}Documents/DownloadFileBytes/117.pdf?documentType=2&meetingId=117"),
            ],
            "newer half first, and nothing from the capped answer itself"
        );
        assert_eq!(
            crawl.requests(),
            4,
            "two searches, then the two meetings' agendas"
        );
        assert_eq!(got.figures["windows"], 2);
        assert!(!got.truncated, "{:?}", got.warnings);
    }

    /// One day still capped cannot be narrowed, and the count is a floor.
    #[tokio::test]
    async fn a_day_that_is_still_capped_is_reported_short() {
        let at = search("10/15/2026", "10/15/2026");
        let capped: Vec<_> = (1..=100).map(|id| (id, true, false, false)).collect();
        let s = seed(&answer(&capped, true), &at);

        let got = OnBaseAgenda.enumerate(&s, &Fake::new([])).await.unwrap();

        assert_eq!(got.addresses.len(), 100);
        assert!(got.truncated);
        assert!(
            got.warnings
                .iter()
                .any(|w| w.contains("one day holds more")),
            "{:?}",
            got.warnings
        );
    }

    /// The front door walks every meeting the application could hold, asking the search
    /// rather than trusting the page's own short list.
    #[tokio::test]
    async fn the_front_door_asks_the_search_for_everything() {
        let s = seed(&answer(&[(2936, true, false, false)], false), ROOT);
        let crawl = Fake::new([]);
        let got = OnBaseAgenda.enumerate(&s, &crawl).await.unwrap();
        assert_eq!(crawl.requests(), 1, "one search for the whole range");
        assert!(got.discovery_failed, "the search was not answered");
        assert!(
            got.warnings.iter().any(|w| w.starts_with("2000-01-01 to ")),
            "{:?}",
            got.warnings
        );
    }

    /// The agreements are attached to items. Every meeting's own documents come first, then
    /// the agenda is read for its items and each item for its supporting documents — the
    /// sections are not asked for, and an item named twice is opened once.
    #[tokio::test]
    async fn each_item_gives_its_supporting_documents() {
        let at = search("05/01/2026", "05/31/2026");
        let s = seed(&answer(&[(2824, true, true, false)], false), &at);
        let crawl = Fake::new([
            (agenda_of(2824).as_str(), AGENDA_2824.to_string()),
            (item_of(2824, 23596).as_str(), ITEM_23596.to_string()),
            (item_of(2824, 23601).as_str(), ITEM_BARE.to_string()),
        ]);

        let got = OnBaseAgenda.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                format!("{ROOT}Documents/DownloadFileBytes/2824.pdf?documentType=1&meetingId=2824"),
                format!("{ROOT}Documents/DownloadFileBytes/2824.pdf?documentType=2&meetingId=2824"),
                attached(2824, 23596, 192327),
                attached(2824, 23596, 192328),
            ]
        );
        assert_eq!(got.figures["items"], 2);
        assert_eq!(got.figures["attachments"], 2);
        assert_eq!(crawl.requests(), 3, "one agenda and two items");
        assert!(
            !got.truncated && got.warnings.is_empty(),
            "{:?}",
            got.warnings
        );
    }

    /// The newest meeting is opened first and finished before the next is begun, so a
    /// budget that runs out keeps every meeting's documents and the newest one's
    /// attachments, and says it stopped.
    #[tokio::test]
    async fn a_walk_out_of_budget_has_the_newest_meetings_attachments() {
        let at = search("05/01/2026", "05/31/2026");
        // 2700 is listed first and held a month earlier.
        let page = answer(
            &[(2700, true, false, false), (2824, true, false, false)],
            false,
        )
        .replacen("2026-10-15T09:00:00", "2026-09-15T09:00:00", 1);
        let s = seed(&page, &at);
        let crawl = Fake::new([
            (agenda_of(2824).as_str(), AGENDA_2824.to_string()),
            (item_of(2824, 23596).as_str(), ITEM_23596.to_string()),
            (item_of(2824, 23601).as_str(), ITEM_BARE.to_string()),
            (agenda_of(2700).as_str(), AGENDA_2824.to_string()),
        ])
        .with_budget(3);

        let got = OnBaseAgenda.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                format!("{ROOT}Documents/DownloadFileBytes/2700.pdf?documentType=1&meetingId=2700"),
                format!("{ROOT}Documents/DownloadFileBytes/2824.pdf?documentType=1&meetingId=2824"),
                attached(2824, 23596, 192327),
                attached(2824, 23596, 192328),
            ]
        );
        assert_eq!(crawl.requests(), 3, "2700's agenda was never fetched");
        assert!(got.truncated, "{:?}", got.warnings);
    }

    /// The name and the two flags are decoration; the four keys are the document. Without
    /// `itemId` the server answers with the meeting's agenda, so such a link is not kept.
    #[test]
    fn an_attachment_has_one_address_whatever_its_name() {
        let root = Url::parse(ROOT).unwrap();
        let link = |q: &str| {
            Url::parse(&format!(
                "{ROOT}Documents/DownloadFile/Any%20Name.PDF.pdf?{q}"
            ))
            .unwrap()
        };
        assert_eq!(
            attachment(
                &root,
                &link(
                    "documentType=1&meetingId=2824&itemId=23596&publishId=192328&isSection=False&isAttachment=True"
                )
            ),
            Some(attached(2824, 23596, 192328))
        );
        assert_eq!(
            attachment(
                &root,
                &link("documentType=1&meetingId=2824&publishId=192328&isAttachment=True")
            ),
            None
        );
    }
}
