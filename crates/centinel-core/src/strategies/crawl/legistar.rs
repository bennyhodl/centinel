//! `legistar` — a Legistar client's meetings: which meetings there are from Legistar's
//! public Web API, and their documents from the client's own InSite site.
//!
//! Keyed on a **product**. In Tampa Bay, Hillsborough County, Pinellas County, Clearwater
//! and Belleair publish their agendas, minutes and legislation on `{client}.legistar.com`.
//! The site is ASP.NET WebForms, and its calendar changes year by postback, so following
//! its links collects 83 to 240 addresses and never finishes. The same meetings are served
//! by `https://webapi.legistar.com/v1/{client}/…`, an OData API that answered without a
//! token for all four clients on 2026-11-20.
//!
//! ## What it collects, per meeting
//!
//! - the meeting's items as the API serves them —
//!   `events/{id}/eventitems?AgendaNote=1&MinutesNote=1&Attachments=1` — which carry each
//!   item's title, file number, action, mover, seconder and result, so the votes are
//!   searchable and not only printed inside a PDF;
//! - the agenda, the minutes, and anything else the clerk attached to the meeting, from the
//!   meeting's InSite page;
//! - every attachment of every item's legislation: the staff reports, resolutions and
//!   ordinances themselves.
//!
//! ## Where the documents are
//!
//! Not where the API says. It names them on `{client}.legistar1.com` or
//! `legistar2.granicus.com`, and both serve `User-agent: *` / `Disallow: /`, so a walk that
//! honoured them collected the items and nothing else (`docs/FIELD-NOTES.md` entries 7 and
//! 8). InSite serves the same files and no robots.txt, but under its own ids, which only
//! its own pages carry: the API's `MatterAttachmentId` and GUID open nothing there. So the
//! documents are read two pages down:
//!
//! 1. The meeting's page, `MeetingDetail.aspx`, at the address the API gives as
//!    `EventInSiteURL`. It links the agenda (`View.ashx?M=A`), the minutes (`M=M`), a
//!    meeting extra such as a change sheet (`M=E1`), the meeting's own attachments
//!    (`M=AO`), and each item's legislation, `LegislationDetail.aspx?ID=…&GUID=…`.
//! 2. Each legislation page lists the matter's attachments, `View.ashx?M=F&ID=…&GUID=…`.
//!    No other page names them: the meeting page links none, and an item's
//!    `HistoryDetail.aspx` none.
//!
//! `M`, `ID` and `GUID` decide the file; the client's `G` and an attachment's `N` (its name,
//! base64) do not. So a document is written with those three keys alone — one document, one
//! address. Drop any of the three, or change one, and the server redirects to
//! `Confirmation.aspx?M1=Gone`; unlike OnBase, no partial query answers with the wrong
//! file. A legislation page needs its `ID` and `GUID` and nothing else.
//!
//! The accessible-HTML renditions of the agenda and minutes (`M=AADA`, `M=MADA`) are the
//! same text as the PDFs at about three times the size, and are not kept. Nor are the
//! API's `EventAgendaFile`, `EventMinutesFile` and `MatterAttachmentHyperlink`: InSite
//! serves the same documents, and a source that ruled `ignore_robots` would hold each twice.
//!
//! ## Order, and what a short walk loses
//!
//! Every page of meetings is read first, and every meeting's items kept from it. Then
//! meetings are opened **one at a time, newest first**, each page followed by its
//! legislation. A walk that runs out of requests has every meeting's items, every document
//! of the newest meetings it finished, and at most one meeting part-read. A matter heard at
//! several meetings is read once.
//!
//! ## robots.txt, per host
//!
//! The walk touches three hosts: the API, InSite, and none other. Every request and every
//! address is checked against the rules of the host it is on ([`Walk::robots`]), and what
//! was excluded is said by host. None of the three serves a robots.txt today.
//!
//! ## Bounding a walk
//!
//! The seed bounds it. Handed `{client}.legistar.com`, the walk covers every meeting the
//! client publishes. Handed the API's own events address with an OData `$filter` —
//! `https://webapi.legistar.com/v1/hillsboroughcounty/events?$filter=EventDate ge datetime'2026-08-22'` —
//! it covers the meetings that filter selects. The request budget (`--max-sitemaps`) still
//! applies inside that range, and the legislation spends it: one request per meeting and
//! one per matter. A Hillsborough month is about three meetings and 66 matters, 70 requests;
//! all 269 of its meetings since 2020 are about 6,200 (`docs/FIELD-NOTES.md` entry 8).
//!
//! ## A client that needs a token
//!
//! Some clients publish to token holders only, and the API answers 403 ("Token is
//! required"). Centinel holds no token, so that is a discovery failure, said in those words.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use futures::future::BoxFuture;
use serde::Deserialize;
use url::Url;

use super::{Addresses, Enumerated, Pass, Seed, Strategy, StrategyDef, Walk};
use crate::domain::Note;
use crate::strategies::{Keyed, Recognition};

pub struct Legistar;

inventory::submit! { StrategyDef { name: "legistar", it: &Legistar } }

/// The API every client is served from.
const API: &str = "https://webapi.legistar.com/v1/";

/// The most rows one OData answer holds. A full page means there may be another.
const PAGE: usize = 1000;

/// Pages of meetings are depth 0, a meeting's own page 1, its legislation 2.
const MAX_DEPTH: usize = 2;

/// The keys that choose a document, and a legislation page; see the module docs.
const DOCUMENT: [&str; 3] = ["M", "ID", "GUID"];
const LEGISLATION: [&str; 2] = ["ID", "GUID"];

/// The agenda and minutes again, as accessible HTML; see the module docs.
const RENDITIONS: [&str; 2] = ["AADA", "MADA"];

/// What the operator is told when the API asks for a token.
const TOKEN: &str = "this client publishes to token holders only, and Centinel holds no token";

impl Strategy for Legistar {
    fn name(&self) -> &'static str {
        "legistar"
    }

    fn recognise(&self, seed: &Seed) -> Option<Recognition> {
        let at = seed.final_url()?;
        let (client, filter) = client_of(&at)?;
        let host = at.host_str()?;
        let api = host == "webapi.legistar.com";
        // A Legistar site is confirmed on its bytes; a parked name is not a client.
        if !api && !looks_like_legistar(&seed.text()) {
            return None;
        }
        let mut r = Recognition::new(self.name(), Keyed::Product("Legistar"))
            .seeing("client", format!("`{client}`, from {host}"))
            .seeing("api", format!("{API}{client}/events"))
            .seeing(
                "documents",
                "from each meeting's InSite page and each item's legislation",
            );
        if let Some(filter) = filter {
            r = r.seeing("filter", filter);
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
            let (client, filter) = client_of(&at)
                .ok_or_else(|| anyhow::anyhow!("{at} does not name a Legistar client"))?;
            let api = Url::parse(API)?.join(&format!("{client}/"))?;

            let mut pass = Pass::new(crawl, "calendar", MAX_DEPTH);
            pass.note(Note::new("client", client.clone()));
            if let Some(f) = &filter {
                pass.note(Note::new("filter", f.clone()));
            }
            pass.push(Ask::events(&api, filter.as_deref(), 0), 0);

            // Meeting pages, newest first, opened once every page of meetings is read; see
            // the module docs.
            let mut meetings: VecDeque<Url> = VecDeque::new();
            let mut tally = Tally::default();
            loop {
                let Some((ask, depth)) = pass.next_to_visit() else {
                    // Nothing queued: open the next meeting. A ceiling leaves the queue
                    // holding what it stopped before, and ends the walk here.
                    if pass.pending() == 0
                        && let Some(page) = meetings.pop_front()
                    {
                        pass.push(Ask::meeting(page), 1);
                        continue;
                    }
                    break;
                };
                pass.visiting(ask.to_string());
                if !pass
                    .walk()
                    .robots(ask.url.as_str())
                    .await
                    .allowed(ask.url.as_str())
                {
                    tally.exclude(ask.url.as_str());
                    continue;
                }
                let body = match pass.walk().get(ask.url.as_str()).await {
                    Ok(f) => f.bytes,
                    Err(refusal) => {
                        if ask.kind == Kind::Events && refusal.status == Some(403) {
                            pass.warn(format!(
                                "the Legistar Web API refused `{client}` (HTTP 403): {TOKEN}"
                            ));
                        } else {
                            pass.refused(&ask, &refusal);
                        }
                        continue;
                    }
                };
                match ask.kind {
                    Kind::Events => {
                        let Ok(events) = serde_json::from_slice::<Vec<Event>>(&body) else {
                            pass.warn(format!("{ask}: the answer is not a list of meetings"));
                            continue;
                        };
                        pass.read_surface();
                        tally.meetings += events.len() as u64;
                        for event in &events {
                            keep(&mut pass, &mut tally, &items(&api, event.event_id)).await;
                            meetings.extend(
                                event
                                    .event_in_site_url
                                    .as_deref()
                                    .and_then(|u| Url::parse(u).ok()),
                            );
                        }
                        if events.len() >= PAGE {
                            let skip = ask.skip + events.len();
                            let next = Ask::events(&api, filter.as_deref(), skip);
                            pass.push(next, depth);
                        }
                    }
                    Kind::Meeting => {
                        let found = links_in(&String::from_utf8_lossy(&body), &ask.url);
                        for address in &found.documents {
                            tally.documents += 1;
                            keep(&mut pass, &mut tally, address).await;
                        }
                        for matter in found.legislation {
                            tally.matters += 1;
                            pass.push(Ask::legislation(matter), depth + 1);
                        }
                    }
                    Kind::Legislation => {
                        let found = links_in(&String::from_utf8_lossy(&body), &ask.url);
                        for address in &found.documents {
                            tally.attachments += 1;
                            keep(&mut pass, &mut tally, address).await;
                        }
                    }
                }
            }

            for (host, n) in &tally.excluded {
                pass.warn(format!(
                    "{host}'s robots.txt disallows {} address(es) this walk reads or keeps \
                     there; if the documents are public records, `source add \
                     --ignore-robots` rules that it does not bind",
                    crate::render::count(*n)
                ));
            }
            pass.figure("meetings", tally.meetings);
            pass.figure("documents", tally.documents);
            pass.figure("matters", tally.matters);
            pass.figure("attachments", tally.attachments);
            Ok(pass.finish())
        })
    }

    /// Every address is a document or an API record: nothing inside one is a page to scan.
    fn addresses_are(&self) -> Addresses {
        Addresses::Documents
    }
}

/// Keep an address if its own host's rules allow it, counting by host what they did not.
async fn keep(pass: &mut Pass<'_, Ask>, tally: &mut Tally, address: &str) {
    if pass.full() {
        return;
    }
    let rules = pass.walk().robots(address).await;
    if !rules.allowed(address) {
        tally.exclude(address);
    }
    pass.keep(address, &rules);
}

#[derive(Default)]
struct Tally {
    meetings: u64,
    /// The agendas, minutes and other documents meeting pages link.
    documents: u64,
    /// Legislation named by meeting pages, counted at each meeting that names it.
    matters: u64,
    attachments: u64,
    /// Addresses a host's robots.txt excluded, by host: pages not read and documents not
    /// kept alike.
    excluded: BTreeMap<String, u64>,
}

impl Tally {
    fn exclude(&mut self, address: &str) {
        let host = Url::parse(address)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        *self.excluded.entry(host).or_default() += 1;
    }
}

/// The client a seed names, and the OData filter it carries if it is the API itself.
///
/// `hillsboroughcounty.legistar.com` → `hillsboroughcounty`, which is the API's own name for
/// the client in every case seen. `webapi.legistar.com/v1/pinellas/events?$filter=…` →
/// `pinellas`, and the filter.
fn client_of(at: &Url) -> Option<(String, Option<String>)> {
    let host = at.host_str()?.to_ascii_lowercase();
    if host == "webapi.legistar.com" {
        let mut segments = at.path_segments()?;
        if segments.next()? != "v1" {
            return None;
        }
        let client = segments.next().filter(|c| !c.is_empty())?;
        if segments.next()? != "events" {
            return None;
        }
        let filter = at
            .query_pairs()
            .find(|(k, _)| k == "$filter")
            .map(|(_, v)| v.into_owned());
        return Some((client.to_string(), filter));
    }
    let client = host.strip_suffix(".legistar.com")?;
    if client.is_empty() || client.contains('.') || client == "www" {
        return None;
    }
    Some((client.to_string(), None))
}

/// What only a Legistar InSite page carries.
fn looks_like_legistar(page: &str) -> bool {
    ["MeetingDetail.aspx", "LegislationDetail.aspx", "Legistar"]
        .iter()
        .any(|marker| page.contains(marker))
}

/// One meeting's items, with their notes and attachments. Kept, and not read: the
/// attachments it names are on robots-disallowed hosts, and InSite serves them.
fn items(api: &Url, event: u64) -> String {
    format!("{api}events/{event}/eventitems?AgendaNote=1&MinutesNote=1&Attachments=1")
}

/// What an InSite page links, read from its anchors.
#[derive(Debug, Default, PartialEq)]
struct Links {
    /// `View.ashx` documents, cut to [`DOCUMENT`], without the [`RENDITIONS`].
    documents: Vec<String>,
    /// `LegislationDetail.aspx` pages, cut to [`LEGISLATION`].
    legislation: Vec<Url>,
}

/// The documents and legislation an InSite page links, on its own host.
///
/// Read from the page's anchors, never a scan of its text. A legislation page links the
/// other meetings its matter was heard at; those are not read here, because every meeting
/// a walk covers comes from the API.
fn links_in(page: &str, at: &Url) -> Links {
    let scan = crate::html::Scan::new(page);
    let base = scan.base(at);
    let mut found = Links::default();
    for link in scan
        .tags(&["a"])
        .into_iter()
        .filter_map(|tag| base.join(tag.attr("href")?))
        .filter(|u| u.host_str() == at.host_str())
    {
        let name = link.path().rsplit('/').next().unwrap_or_default();
        if name.eq_ignore_ascii_case("View.ashx") {
            let rendition = link
                .query_pairs()
                .any(|(k, v)| k == "M" && RENDITIONS.contains(&&*v));
            if !rendition && let Some(doc) = cut(&link, &DOCUMENT) {
                found.documents.push(doc.into());
            }
        } else if name.eq_ignore_ascii_case("LegislationDetail.aspx") {
            found.legislation.extend(cut(&link, &LEGISLATION));
        }
    }
    found
}

/// The link with only `keys` in its query, in that order; `None` unless it carries them all.
fn cut(link: &Url, keys: &[&str]) -> Option<Url> {
    let query: BTreeMap<_, _> = link.query_pairs().collect();
    let pairs = keys
        .iter()
        .map(|k| Some((*k, query.get(*k)?.clone())))
        .collect::<Option<Vec<_>>>()?;
    let mut url = link.clone();
    url.set_fragment(None);
    url.set_query(None);
    url.query_pairs_mut().extend_pairs(pairs);
    Some(url)
}

/// One request this walk makes.
#[derive(Clone, Debug)]
struct Ask {
    url: Url,
    kind: Kind,
    /// Rows of meetings before this page, for the next one.
    skip: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A page of meetings from the API.
    Events,
    /// One meeting's InSite page, for its documents and legislation.
    Meeting,
    /// One matter's InSite page, for its attachments.
    Legislation,
}

impl Ask {
    /// One page of meetings, newest first.
    fn events(api: &Url, filter: Option<&str>, skip: usize) -> Self {
        let mut url = api.join("events").expect("a relative path joins");
        {
            let mut query = url.query_pairs_mut();
            if let Some(filter) = filter {
                query.append_pair("$filter", filter);
            }
            query.append_pair("$orderby", "EventDate desc,EventId desc");
            query.append_pair("$top", &PAGE.to_string());
            if skip > 0 {
                query.append_pair("$skip", &skip.to_string());
            }
        }
        Self {
            url,
            kind: Kind::Events,
            skip,
        }
    }

    fn meeting(url: Url) -> Self {
        Self {
            url,
            kind: Kind::Meeting,
            skip: 0,
        }
    }

    fn legislation(url: Url) -> Self {
        Self {
            url,
            kind: Kind::Legislation,
            skip: 0,
        }
    }
}

/// How it reads in a warning, and what [`Pass`] dedups on.
impl fmt::Display for Ask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.url.as_str())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Event {
    event_id: u64,
    /// The meeting's InSite page.
    #[serde(default, rename = "EventInSiteURL")]
    event_in_site_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::crawl::tests::{Fake, seed};

    const SITE: &str = "https://hillsboroughcounty.legistar.com/Calendar.aspx";
    const INSITE: &str = "hillsboroughcounty.legistar.com";
    const NO_CRAWLERS: &str = "User-agent: * \nDisallow: / \n";

    /// Two meetings as the API answered on 2026-11-20, cut to the fields that matter and a
    /// few that do not. Both name their InSite page; the agenda and minutes they name are on
    /// a host that disallows every crawler.
    const EVENTS: &str = r#"[
      {"EventId":1398,"EventGuid":"7D85F3E0-9F69-4A6B-8803-715CDF34D637","EventBodyName":"Zoning Hearing Master","EventDate":"2026-11-16T00:00:00","EventTime":"6:00 PM","EventAgendaStatusName":"Final-revised","EventAgendaFile":"https://hillsboroughcounty.legistar1.com/hillsboroughcounty/meetings/2026/11/1398_A_Zoning_Hearing_Master_26-11-16_Agenda.pdf","EventMinutesFile":"https://hillsboroughcounty.legistar1.com/hillsboroughcounty/meetings/2026/11/1398_M_Zoning_Hearing_Master_26-11-16_Results.pdf","EventInSiteURL":"https://hillsboroughcounty.legistar.com/MeetingDetail.aspx?LEGID=1398&GID=761&G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8","EventItems":[]},
      {"EventId":1391,"EventBodyName":"BOCC Land Use","EventDate":"2026-11-10T00:00:00","EventAgendaFile":"https://hillsboroughcounty.legistar1.com/hillsboroughcounty/meetings/2026/11/1391_A_BOCC_Land_Use_26-11-10_Agenda.pdf","EventMinutesFile":null,"EventInSiteURL":"https://hillsboroughcounty.legistar.com/MeetingDetail.aspx?LEGID=1391&GID=761&G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8","EventItems":[]}
    ]"#;

    const MEETING_1398: &str = "https://hillsboroughcounty.legistar.com/MeetingDetail.aspx?LEGID=1398&GID=761&G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8";
    const MEETING_1391: &str = "https://hillsboroughcounty.legistar.com/MeetingDetail.aspx?LEGID=1391&GID=761&G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8";

    /// Meeting 1398's page as InSite served it on 2026-11-21, cut to its document links,
    /// two of its 32 legislation rows, and the links beside them that are neither: sign-in,
    /// video, action details.
    const PAGE_1398: &str = r##"<html><head><title>Hillsborough County - Meeting of Zoning Hearing Master</title></head><body><form id="aspnetForm">
<a id="ctl00_hypSignIn" href="SignIn.aspx?ReturnUrl=TXlBY2NvdW50LmFzcHg%3d&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8">Sign In</a>
<img id="ctl00_ContentPlaceHolder1_imgAgendaMain" src="Images/PDF.gif" alt="Agenda" border="0" /> <a id="ctl00_ContentPlaceHolder1_hypAgenda" href="View.ashx?M=A&amp;ID=1351680&amp;GUID=137FE552-0FD6-46F6-8DEC-3B993A5F1AE1&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8" target="_blank"><font face="Tahoma" color="Blue" size="2">Agenda</font></a> <img id="ctl00_ContentPlaceHolder1_imgAgendaMainHTML" src="Images/HTML.gif" alt="Accessible Agenda" border="0" /> <a id="ctl00_ContentPlaceHolder1_hypAgendaHTML" href="View.ashx?M=AADA&amp;ID=1351680&amp;GUID=137FE552-0FD6-46F6-8DEC-3B993A5F1AE1&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8" target="_blank"><font face="Tahoma" color="Blue" size="2">Accessible Agenda</font></a>
<a id="ctl00_ContentPlaceHolder1_hypMinutes" href="View.ashx?M=M&amp;ID=1351680&amp;GUID=137FE552-0FD6-46F6-8DEC-3B993A5F1AE1&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8" target="_blank"><font face="Tahoma" color="Blue" size="2">Minutes</font></a> <a id="ctl00_ContentPlaceHolder1_hypMinutesHTML" href="View.ashx?M=MADA&amp;ID=1351680&amp;GUID=137FE552-0FD6-46F6-8DEC-3B993A5F1AE1&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8" target="_blank"><font face="Tahoma" color="Blue" size="2">Accessible Minutes</font></a>
<span id="ctl00_ContentPlaceHolder1_lblExtra1"><font face="Tahoma" color="Navy" size="2">Meeting Extra1:</font></span> <img id="ctl00_ContentPlaceHolder1_imgExtra1" src="Images/PDF.gif" alt="Change Sheet" border="0" /> <a id="ctl00_ContentPlaceHolder1_hypExtra1" href="View.ashx?M=E1&amp;ID=1351680&amp;GUID=137FE552-0FD6-46F6-8DEC-3B993A5F1AE1&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8"><font face="Tahoma" color="Blue" size="2">Change Sheet</font></a>
<a id="ctl00_ContentPlaceHolder1_hypVideo" data-event-id="7d85f3e0-9f69-4a6b-8803-715cdf34d637" class="videolink" onclick="window.open(&#39;Video.aspx?Mode=Granicus&amp;ID1=1401&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8&amp;Mode2=Video&#39;,&#39;video&#39;);return false;" href="#">Video</a>
<table id="ctl00_ContentPlaceHolder1_ucAttachments_tblAttachments"><tr><td><span id="ctl00_ContentPlaceHolder1_ucAttachments_lblAttachmentsPrompt"><font face="Tahoma" color="Navy" size="2">Attachments:</font></span></td><td><span id="ctl00_ContentPlaceHolder1_ucAttachments_lblAttachments"><font face="Tahoma" color="Black" size="2"><a href="View.ashx?M=AO&amp;ID=184351&amp;GUID=d4892956-f878-4170-b935-dc361baa6988&amp;N=UVIgQ29kZQ%3d%3d&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8" style="color: blue">QR Code</a></font></span></td></tr></table>
<table class="rgMasterTable" id="ctl00_ContentPlaceHolder1_gridMain_ctl00">
<tr class="rgRow" valign="top" ID="ctl00_ContentPlaceHolder1_gridMain_ctl00__0"><td nowrap="nowrap"><font face="Tahoma" size="2"> <a id="ctl00_ContentPlaceHolder1_gridMain_ctl00_ctl04_hypFile" href="LegislationDetail.aspx?ID=8136335&amp;GUID=9539EB69-30E9-45D9-867E-84538AC3777F&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8&amp;Options=&amp;Search="><font face="Tahoma" color="Blue" size="2">RZ-PD 26-0706</font></a> </font></td><td><font face="Tahoma" size="2">RZ-PD 26-0706 SAMIA ASSET MANAGEMENT, LLC.</font></td><td><font face="Tahoma" size="2"> <a id="ctl00_ContentPlaceHolder1_gridMain_ctl00_ctl04_hypDetails" onclick="var obj = radopen(&#39;HistoryDetail.aspx?ID=38080769&amp;GUID=75AEB8A6-95BA-451B-B6D9-0E1A27FECDE4&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8&#39;, &#39;HistoryDetail&#39;); focusRadWindow(obj); return false;" href="#"><font face="Tahoma" color="Blue" size="2">Action&nbsp;details</font></a> </font></td></tr>
<tr class="rgAltRow" valign="top" ID="ctl00_ContentPlaceHolder1_gridMain_ctl00__23"> <td nowrap="nowrap"><font face="Tahoma" size="2"> <a id="ctl00_ContentPlaceHolder1_gridMain_ctl00_ctl50_hypFile" href="LegislationDetail.aspx?ID=8035225&amp;GUID=C79D9B49-7E21-4E3A-AFE0-A1E3C09757CA&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8&amp;Options=&amp;Search="><font face="Tahoma" color="Blue" size="2">MM 26-0546</font></a> </font></td><td><font face="Tahoma" size="2">D.1.</font></td><td><font face="Tahoma" size="2">Consent Agenda</font></td><td><font face="Tahoma" size="2">Application Number: MM 26-0546 Applicant: NORTH TAMPA CHRUCH OF CHRIST, INC.</font></td><td><font face="Tahoma" size="2">Heard and Pending Recommendation</font></td></tr>
</table></form></body></html>"##;

    /// Meeting 1391's page, cut the same way to its agenda and one legislation row. The row
    /// is RZ-PD 26-0706's, which 1398 also names: a matter is listed at every meeting that
    /// hears it, and its own page lists five. (1391's real rows are other matters; this one
    /// is placed here to show it is read once.)
    const PAGE_1391: &str = r##"<html><body><form id="aspnetForm">
<a id="ctl00_ContentPlaceHolder1_hypAgenda" href="View.ashx?M=A&amp;ID=1351673&amp;GUID=3E7EB9D4-EE90-48DB-B25A-EE8D89242068&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8" target="_blank"><font face="Tahoma" color="Blue" size="2">Agenda</font></a>
<a id="ctl00_ContentPlaceHolder1_gridMain_ctl00_ctl04_hypFile" href="LegislationDetail.aspx?ID=8136335&amp;GUID=9539EB69-30E9-45D9-867E-84538AC3777F&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8&amp;Options=&amp;Search="><font face="Tahoma" color="Blue" size="2">RZ-PD 26-0706</font></a>
</form></body></html>"##;

    /// RZ-PD 26-0706's page, 2026-11-21, cut to two of its five attachments and the links
    /// beside them that are not attachments: a meeting it was heard at, its report, its
    /// feed. The attachment links' ampersands are raw, as served.
    const LEGISLATION_8136335: &str = r##"<html><body><form id="aspnetForm">
<span id="ctl00_ContentPlaceHolder1_lblFile2"><font face="Tahoma" color="Black" size="2">RZ-PD 26-0706</font></span>
<a href="Feed.ashx?M=LD&amp;ID=8136335&amp;GUID=9539EB69-30E9-45D9-867E-84538AC3777F">RSS</a>
<a href="ViewReport.ashx?M=R&amp;N=Master&amp;GID=761&amp;ID=8136335&amp;GUID=9539EB69-30E9-45D9-867E-84538AC3777F&amp;Extra=WithText">Details Report</a>
<table id="ctl00_ContentPlaceHolder1_tblAttachments" border="0" cellpadding="2" cellspacing="2" width="100%"><tr><td style="width: 150px"><span id="ctl00_ContentPlaceHolder1_lblAttachments"><font face="Tahoma" color="Navy" size="2">Attachments:</font></span></td>
<td><span id="ctl00_ContentPlaceHolder1_lblAttachments2"><font face="Tahoma" color="Black" size="2">1. <a href="View.ashx?M=F&ID=15691876&GUID=A1DDDE62-1A65-4FB3-84FD-AA75A1FE3602" style="color: blue">26-0706</a>, 2. <a href="View.ashx?M=F&ID=15775270&GUID=CC2D410C-BDE7-478C-815F-325D05A55329" style="color: blue">26-0706</a></font></span></td></tr></table>
<a href="MeetingDetail.aspx?ID=1351679&amp;GUID=6850B025-24DB-4324-ACA7-370743E0BBF1&amp;Options=&amp;Search=">Zoning Hearing Master</a>
</form></body></html>"##;

    /// MM 26-0546's page, cut to two of its six attachments; served with the client's `G`.
    const LEGISLATION_8035225: &str = r##"<html><body><form id="aspnetForm">
<table id="ctl00_ContentPlaceHolder1_tblAttachments"><tr><td><span id="ctl00_ContentPlaceHolder1_lblAttachments2"><font face="Tahoma" color="Black" size="2">1. <a href="View.ashx?M=F&ID=15494315&GUID=9535DD88-60C6-46F2-A779-5B7A82939128&G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8" style="color: blue">26-0546</a>, 2. <a href="View.ashx?M=F&ID=15586871&GUID=C1F54577-8780-4370-BB97-3F6A4BBD62E3&G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8" style="color: blue">26-0546</a></font></span></td></tr></table>
</form></body></html>"##;

    fn api() -> Url {
        Url::parse("https://webapi.legistar.com/v1/hillsboroughcounty/").unwrap()
    }

    fn insite(path: &str) -> String {
        format!("https://{INSITE}/{path}")
    }

    fn legislation(id: u64, guid: &str) -> String {
        insite(&format!("LegislationDetail.aspx?ID={id}&GUID={guid}"))
    }

    fn calendar() -> Seed {
        seed(
            r#"<html><head><title>Hillsborough County - Calendar</title></head><body><form id="aspnetForm"><input type="hidden" name="__VIEWSTATE" value="x"/><a href="MeetingDetail.aspx?LEGID=1398&amp;GID=761">Meeting details</a><span>Legistar</span></form></body></html>"#,
            SITE,
        )
    }

    /// Every page the two meetings lead to, as served.
    fn served() -> Fake {
        Fake::new([
            (
                Ask::events(&api(), None, 0).url.as_str(),
                EVENTS.to_string(),
            ),
            (MEETING_1398, PAGE_1398.to_string()),
            (MEETING_1391, PAGE_1391.to_string()),
            (
                legislation(8136335, "9539EB69-30E9-45D9-867E-84538AC3777F").as_str(),
                LEGISLATION_8136335.to_string(),
            ),
            (
                legislation(8035225, "C79D9B49-7E21-4E3A-AFE0-A1E3C09757CA").as_str(),
                LEGISLATION_8035225.to_string(),
            ),
        ])
    }

    /// What meeting 1398 gives: its items, its documents, and its two matters' attachments.
    fn of_1398() -> Vec<String> {
        vec![
            insite("View.ashx?M=A&ID=1351680&GUID=137FE552-0FD6-46F6-8DEC-3B993A5F1AE1"),
            insite("View.ashx?M=M&ID=1351680&GUID=137FE552-0FD6-46F6-8DEC-3B993A5F1AE1"),
            insite("View.ashx?M=E1&ID=1351680&GUID=137FE552-0FD6-46F6-8DEC-3B993A5F1AE1"),
            insite("View.ashx?M=AO&ID=184351&GUID=d4892956-f878-4170-b935-dc361baa6988"),
            insite("View.ashx?M=F&ID=15691876&GUID=A1DDDE62-1A65-4FB3-84FD-AA75A1FE3602"),
            insite("View.ashx?M=F&ID=15775270&GUID=CC2D410C-BDE7-478C-815F-325D05A55329"),
            insite("View.ashx?M=F&ID=15494315&GUID=9535DD88-60C6-46F2-A779-5B7A82939128"),
            insite("View.ashx?M=F&ID=15586871&GUID=C1F54577-8780-4370-BB97-3F6A4BBD62E3"),
        ]
    }

    #[test]
    fn a_legistar_site_and_its_api_name_the_same_client() {
        let r = Legistar.recognise(&calendar()).expect("a Legistar site");
        assert_eq!(r.keyed_on, Keyed::Product("Legistar"));
        assert!(
            r.evidence
                .iter()
                .any(|n| n.detail.contains("`hillsboroughcounty`"))
        );

        let filtered = "https://webapi.legistar.com/v1/pinellas/events?$filter=EventDate ge datetime'2026-08-22'";
        let at = Url::parse(filtered).unwrap();
        assert_eq!(
            client_of(&at),
            Some((
                "pinellas".to_string(),
                Some("EventDate ge datetime'2026-08-22'".to_string())
            ))
        );
        assert!(Legistar.recognise(&seed("[]", filtered)).is_some());

        assert!(client_of(&Url::parse("https://www.legistar.com/").unwrap()).is_none());
        assert!(
            Legistar
                .recognise(&seed(
                    "<html>parked</html>",
                    "https://clearwater.legistar.com/"
                ))
                .is_none(),
            "a Legistar host is confirmed on its bytes"
        );
    }

    /// Every meeting's items first. Then each meeting's page, newest first, gives its
    /// documents and its legislation, and each legislation page its attachments — read on
    /// InSite, never on the document host the API names. A matter at two meetings is read
    /// once.
    #[tokio::test]
    async fn every_meeting_gives_its_agenda_minutes_items_and_attachments() {
        let crawl = served().with_robots("hillsboroughcounty.legistar1.com", NO_CRAWLERS);

        let got = Legistar.enumerate(&calendar(), &crawl).await.unwrap();

        let mut want = vec![items(&api(), 1398), items(&api(), 1391)];
        want.extend(of_1398());
        want.push(insite(
            "View.ashx?M=A&ID=1351673&GUID=3E7EB9D4-EE90-48DB-B25A-EE8D89242068",
        ));
        assert_eq!(got.addresses, want);
        assert_eq!(got.figures["meetings"], 2);
        assert_eq!(got.figures["documents"], 5);
        assert_eq!(got.figures["matters"], 3, "RZ-PD 26-0706 at both meetings");
        assert_eq!(got.figures["attachments"], 4);
        assert_eq!(
            got.figures["disallowed"], 0,
            "nothing on legistar1 is offered"
        );
        assert!(
            !got.truncated && got.warnings.is_empty(),
            "{:?}",
            got.warnings
        );
        assert_eq!(
            crawl.requests(),
            5,
            "one page of meetings, two meeting pages, two legislation pages"
        );
    }

    /// InSite turns crawlers away. Its pages are not read, the items on the API are kept,
    /// and the run says where and how many.
    #[tokio::test]
    async fn a_document_host_that_disallows_everything_is_said_by_name() {
        let crawl = served().with_robots(INSITE, NO_CRAWLERS);

        let got = Legistar.enumerate(&calendar(), &crawl).await.unwrap();

        assert_eq!(got.addresses, [items(&api(), 1398), items(&api(), 1391)]);
        assert_eq!(crawl.requests(), 1, "no disallowed page is fetched");
        assert!(
            got.warnings.iter().any(|w| w.starts_with(
                "hillsboroughcounty.legistar.com's robots.txt disallows 2 address(es)"
            )),
            "{:?}",
            got.warnings
        );
    }

    /// The newest meeting is opened first and finished before the next is begun, so a
    /// budget that runs out keeps every meeting's items and the newest one's documents,
    /// and says it stopped.
    #[tokio::test]
    async fn a_walk_out_of_budget_has_the_newest_meetings_documents() {
        let crawl = served().with_budget(4);

        let got = Legistar.enumerate(&calendar(), &crawl).await.unwrap();

        let mut want = vec![items(&api(), 1398), items(&api(), 1391)];
        want.extend(of_1398());
        assert_eq!(got.addresses, want);
        assert_eq!(crawl.requests(), 4, "1391's page was never fetched");
        assert!(got.truncated, "{:?}", got.warnings);
    }

    /// A full page means another may follow, and it is read before any meeting's page.
    #[tokio::test]
    async fn a_full_page_of_meetings_is_followed_by_the_next() {
        let page: Vec<String> = (0..PAGE)
            .map(|i| format!(r#"{{"EventId":{},"EventAgendaFile":null}}"#, 5000 + i))
            .collect();
        let crawl = Fake::new([
            (
                Ask::events(&api(), None, 0).url.as_str(),
                format!("[{}]", page.join(",")),
            ),
            (
                Ask::events(&api(), None, PAGE).url.as_str(),
                format!(
                    r#"[{{"EventId":1,"EventAgendaFile":null,"EventInSiteURL":"{MEETING_1391}"}}]"#
                ),
            ),
        ])
        .with_budget(2);

        let got = Legistar.enumerate(&calendar(), &crawl).await.unwrap();

        assert_eq!(
            got.figures["meetings"],
            PAGE as u64 + 1,
            "both pages were read"
        );
        assert!(
            got.truncated,
            "the meeting's page was not reached, and that is a floor"
        );
        assert!(
            got.addresses.contains(&items(&api(), 1)),
            "the oldest meeting's items are kept though its page was not read"
        );
    }

    /// `M`, `ID` and `GUID` are the document; the client's `G` and the name `N` are
    /// decoration. A link short of a key opens nothing and is not kept, and neither are the
    /// accessible renditions, links to other hosts, or InSite's reports.
    #[test]
    fn a_document_has_one_address_whatever_its_decoration() {
        let at = Url::parse(MEETING_1398).unwrap();
        let page = r#"
            <a href="View.ashx?M=AO&amp;ID=184351&amp;GUID=d4892956-f878-4170-b935-dc361baa6988&amp;N=UVIgQ29kZQ%3d%3d&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8">QR Code</a>
            <a href="View.ashx?M=F&ID=15691876&GUID=A1DDDE62-1A65-4FB3-84FD-AA75A1FE3602&G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8">26-0706</a>
            <a href="View.ashx?M=F&ID=15691876">no GUID</a>
            <a href="View.ashx?M=MADA&amp;ID=1351680&amp;GUID=137FE552-0FD6-46F6-8DEC-3B993A5F1AE1">Accessible Minutes</a>
            <a href="https://pinellas.legistar.com/View.ashx?M=F&ID=16134949&GUID=475FA843-2B4D-44AA-9211-3F71F5230A48">another client</a>
            <a href="ViewReport.ashx?M=R&amp;N=Text&amp;ID=6861872&amp;GUID=B725F827-E45E-441C-8B09-FECA8DBA92C5">Text</a>
            <a href="LegislationDetail.aspx?ID=8035225&amp;GUID=C79D9B49-7E21-4E3A-AFE0-A1E3C09757CA&amp;G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8&amp;Options=&amp;Search=">MM 26-0546</a>
            <a href="LegislationDetail.aspx?ID=8035225">no GUID</a>
        "#;

        assert_eq!(
            links_in(page, &at),
            Links {
                documents: vec![
                    insite("View.ashx?M=AO&ID=184351&GUID=d4892956-f878-4170-b935-dc361baa6988"),
                    insite("View.ashx?M=F&ID=15691876&GUID=A1DDDE62-1A65-4FB3-84FD-AA75A1FE3602"),
                ],
                legislation: vec![
                    Url::parse(&legislation(
                        8035225,
                        "C79D9B49-7E21-4E3A-AFE0-A1E3C09757CA"
                    ))
                    .unwrap()
                ],
            }
        );
    }

    /// No token, no meetings — and the reason, not a 403.
    #[tokio::test]
    async fn a_client_that_needs_a_token_says_so() {
        struct Forbidden(Fake);
        impl Walk for Forbidden {
            fn get<'a>(
                &'a self,
                _url: &'a str,
            ) -> BoxFuture<'a, Result<crate::domain::Fetched, crate::domain::Refusal>> {
                Box::pin(async {
                    Err(crate::domain::Refusal {
                        state: crate::domain::Liveness::Blocked,
                        detail: "HTTP 403 Forbidden".into(),
                        status: Some(403),
                        too_large: None,
                    })
                })
            }
            fn may_fetch(&self) -> bool {
                self.0.may_fetch()
            }
            fn budget(&self) -> usize {
                self.0.budget()
            }
            fn max_addresses(&self) -> usize {
                self.0.max_addresses()
            }
            fn progress(&self) -> &crate::op::Progress {
                self.0.progress()
            }
            fn robots<'a>(
                &'a self,
                url: &'a str,
            ) -> BoxFuture<'a, std::sync::Arc<crate::discovery::Robots>> {
                self.0.robots(url)
            }
        }
        let nyc = seed(
            "<html><span>Legistar</span></html>",
            "https://nyc.legistar.com/Calendar.aspx",
        );

        let got = Legistar
            .enumerate(&nyc, &Forbidden(Fake::new([])))
            .await
            .unwrap();

        assert!(got.addresses.is_empty());
        assert!(got.discovery_failed);
        assert!(
            got.warnings
                .iter()
                .any(|w| w.contains("refused `nyc` (HTTP 403)") && w.contains("token")),
            "{:?}",
            got.warnings
        );
    }
}
