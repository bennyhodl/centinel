//! `legistar` — a Legistar client's meetings, through Legistar's public Web API.
//!
//! Keyed on a **product**. In Tampa Bay, Hillsborough County, Pinellas County, Clearwater
//! and Belleair publish their agendas, minutes and legislation on `{client}.legistar.com`.
//! The site is ASP.NET WebForms, and its calendar changes year by postback, so following
//! its links collects 83 to 240 addresses and never finishes. The same records are served
//! by `https://webapi.legistar.com/v1/{client}/…`, an OData API that answered without a
//! token for all four clients on 2026-11-20.
//!
//! ## What it collects, per meeting
//!
//! - the agenda and the minutes, `EventAgendaFile` and `EventMinutesFile`: PDFs on
//!   `{client}.legistar1.com` or `legistar2.granicus.com`;
//! - the meeting's items as the API serves them —
//!   `events/{id}/eventitems?AgendaNote=1&MinutesNote=1&Attachments=1` — which carry each
//!   item's title, file number, action, mover, seconder and result, so the votes are
//!   searchable and not only printed inside a PDF;
//! - every attachment those items name: the staff reports, resolutions and ordinances
//!   themselves, `MatterAttachmentHyperlink`.
//!
//! Meetings are read newest first, every page of them before any meeting's items, so a
//! walk that runs out of requests has kept every agenda and minutes and lost only the
//! oldest attachments.
//!
//! ## robots.txt, per host
//!
//! The documents are not on the seed's host, and their hosts disallow everything:
//! `*.legistar1.com` and `legistar2.granicus.com` serve `User-agent: *` / `Disallow: /`.
//! The API and `{client}.legistar.com` serve no robots.txt. So every address is checked
//! against the rules of the host it is on ([`Walk::robots`]), and what was excluded is said
//! by host. Unless the source's definition sets `ignore_robots`, the walk keeps the meeting
//! items and nothing on a document host.
//!
//! ## Bounding a walk
//!
//! The seed bounds it. Handed `{client}.legistar.com`, the walk covers every meeting the
//! client publishes. Handed the API's own events address with an OData `$filter` —
//! `https://webapi.legistar.com/v1/hillsboroughcounty/events?$filter=EventDate ge datetime'2026-08-22'` —
//! it covers the meetings that filter selects.
//!
//! ## A client that needs a token
//!
//! Some clients publish to token holders only, and the API answers 403 ("Token is
//! required"). Centinel holds no token, so that is a discovery failure, said in those words.

use std::collections::BTreeMap;
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

/// Pages of meetings are depth 0; one meeting's items are depth 1.
const MAX_DEPTH: usize = 1;

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
            .seeing("api", format!("{API}{client}/events"));
        if let Some(filter) = filter {
            r = r.seeing("filter", filter);
        }
        Some(r.warning(
            "robots",
            "the agendas, minutes and attachments are on hosts whose robots.txt disallows \
             every crawler; without `ignore_robots` only the meeting items are collected",
        ))
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

            // Held back until every page of meetings is read; see the module docs.
            let mut items: Vec<Ask> = Vec::new();
            let mut tally = Tally::default();
            while let Some((ask, depth)) = pass.next_to_visit() {
                pass.visiting(ask.to_string());
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
                        // The meetings already read still have items worth asking for.
                        release(&mut pass, &mut items, depth);
                        continue;
                    }
                };
                match ask.kind {
                    Kind::Events => {
                        let Ok(events) = serde_json::from_slice::<Vec<Event>>(&body) else {
                            pass.warn(format!("{ask}: the answer is not a list of meetings"));
                            release(&mut pass, &mut items, depth);
                            continue;
                        };
                        pass.read_surface();
                        tally.events += events.len() as u64;
                        let skip = ask.skip + events.len();
                        for event in &events {
                            for file in [&event.event_agenda_file, &event.event_minutes_file]
                                .into_iter()
                                .flatten()
                            {
                                keep(&mut pass, &mut tally, file).await;
                            }
                            let listed = Ask::items(&api, event.event_id);
                            keep(&mut pass, &mut tally, &listed.url).await;
                            items.push(listed);
                        }
                        match events.len() >= PAGE {
                            true => pass.push(Ask::events(&api, filter.as_deref(), skip), depth),
                            false => release(&mut pass, &mut items, depth),
                        }
                    }
                    Kind::Items => {
                        let Ok(listed) = serde_json::from_slice::<Vec<Item>>(&body) else {
                            pass.warn(format!("{ask}: the answer is not a list of items"));
                            continue;
                        };
                        tally.items += listed.len() as u64;
                        for attachment in listed
                            .iter()
                            .flat_map(|i| i.event_item_matter_attachments.iter().flatten())
                        {
                            if let Some(link) = &attachment.matter_attachment_hyperlink {
                                tally.attachments += 1;
                                keep(&mut pass, &mut tally, link).await;
                            }
                        }
                    }
                }
            }

            for (host, n) in &tally.excluded {
                pass.warn(format!(
                    "{host}'s robots.txt disallows the {} document(s) this client keeps \
                     there; if the documents are public records, `source add \
                     --ignore-robots` rules that it does not bind",
                    crate::render::count(*n)
                ));
            }
            pass.figure("meetings", tally.events);
            pass.figure("items", tally.items);
            pass.figure("attachments", tally.attachments);
            Ok(pass.finish())
        })
    }

    /// Every address is a document or an API record: nothing inside one is a page to scan.
    fn addresses_are(&self) -> Addresses {
        Addresses::Documents
    }
}

/// The meetings are all listed: ask for their items, which are one level down.
fn release(pass: &mut Pass<'_, Ask>, items: &mut Vec<Ask>, depth: usize) {
    for ask in items.drain(..) {
        pass.push(ask, depth + 1);
    }
}

/// Keep an address if its own host's rules allow it, counting by host what they did not.
async fn keep(pass: &mut Pass<'_, Ask>, tally: &mut Tally, address: &str) {
    if pass.full() {
        return;
    }
    let rules = pass.walk().robots(address).await;
    if !rules.allowed(address) {
        let host = Url::parse(address)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        *tally.excluded.entry(host).or_default() += 1;
    }
    pass.keep(address, &rules);
}

#[derive(Default)]
struct Tally {
    events: u64,
    items: u64,
    attachments: u64,
    /// Addresses a document host's robots.txt excluded, by host.
    excluded: BTreeMap<String, u64>,
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

/// One request this walk makes.
#[derive(Clone, Debug)]
struct Ask {
    url: String,
    kind: Kind,
    /// Rows of meetings before this page, for the next one.
    skip: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Events,
    Items,
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
            url: url.to_string(),
            kind: Kind::Events,
            skip,
        }
    }

    /// One meeting's items, with their notes and attachments.
    fn items(api: &Url, event: u64) -> Self {
        Self {
            url: format!("{api}events/{event}/eventitems?AgendaNote=1&MinutesNote=1&Attachments=1"),
            kind: Kind::Items,
            skip: 0,
        }
    }
}

impl fmt::Display for Ask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.url)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Event {
    event_id: u64,
    #[serde(default)]
    event_agenda_file: Option<String>,
    #[serde(default)]
    event_minutes_file: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Item {
    #[serde(default)]
    event_item_matter_attachments: Option<Vec<Attachment>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Attachment {
    #[serde(default)]
    matter_attachment_hyperlink: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::crawl::tests::{Fake, seed};

    const SITE: &str = "https://hillsboroughcounty.legistar.com/Calendar.aspx";
    const DOCS: &str = "hillsboroughcounty.legistar1.com";
    const NO_CRAWLERS: &str = "User-agent: * \nDisallow: / \n";

    /// Two meetings as the API answered on 2026-11-20, cut to the fields that matter and a
    /// few that do not. The second has no minutes yet.
    const EVENTS: &str = r#"[
      {"EventId":1398,"EventGuid":"7D85F3E0-9F69-4A6B-8803-715CDF34D637","EventBodyName":"Zoning Hearing Master","EventDate":"2026-11-16T00:00:00","EventTime":"6:00 PM","EventAgendaStatusName":"Final-revised","EventAgendaFile":"https://hillsboroughcounty.legistar1.com/hillsboroughcounty/meetings/2026/11/1398_A_Zoning_Hearing_Master_26-11-16_Agenda.pdf","EventMinutesFile":"https://hillsboroughcounty.legistar1.com/hillsboroughcounty/meetings/2026/11/1398_M_Zoning_Hearing_Master_26-11-16_Results.pdf","EventInSiteURL":"https://hillsboroughcounty.legistar.com/MeetingDetail.aspx?LEGID=1398&GID=761&G=3144BC14-0EA0-4A0F-BC83-C4F8B333D0E8","EventItems":[]},
      {"EventId":1391,"EventBodyName":"BOCC Land Use","EventDate":"2026-11-10T00:00:00","EventAgendaFile":"https://hillsboroughcounty.legistar1.com/hillsboroughcounty/meetings/2026/11/1391_A_BOCC_Land_Use_26-11-10_Agenda.pdf","EventMinutesFile":null,"EventItems":[]}
    ]"#;

    /// One item with a matter and an attachment, and a page break with neither.
    const ITEMS: &str = r#"[
      {"EventItemId":132602,"EventItemEventId":1398,"EventItemTitle":"page break","EventItemMatterId":null,"EventItemMatterAttachments":[]},
      {"EventItemId":132610,"EventItemEventId":1398,"EventItemAgendaNumber":"A.1.","EventItemActionName":"Continued","EventItemActionText":"Continued to the Zoning Hearing Master due back on 12/14/2026","EventItemTitle":"RZ-PD 26-0706 SAMIA ASSET MANAGEMENT, LLC.","EventItemMatterFile":"RZ-PD 26-0706","EventItemMatterAttachments":[{"MatterAttachmentId":7880,"MatterAttachmentName":"26-0706","MatterAttachmentHyperlink":"https://hillsboroughcounty.legistar1.com/hillsboroughcounty/attachments/ae602428-8df6-4abe-ba24-23a9d874af2f.pdf","MatterAttachmentIsHyperlink":false}]}
    ]"#;

    fn api() -> Url {
        Url::parse("https://webapi.legistar.com/v1/hillsboroughcounty/").unwrap()
    }

    fn calendar() -> Seed {
        seed(
            r#"<html><head><title>Hillsborough County - Calendar</title></head><body><form id="aspnetForm"><input type="hidden" name="__VIEWSTATE" value="x"/><a href="MeetingDetail.aspx?LEGID=1398&amp;GID=761">Meeting details</a><span>Legistar</span></form></body></html>"#,
            SITE,
        )
    }

    fn served(robots: Option<&str>) -> Fake {
        let fake = Fake::new([
            (
                Ask::events(&api(), None, 0).url.as_str(),
                EVENTS.to_string(),
            ),
            (Ask::items(&api(), 1398).url.as_str(), ITEMS.to_string()),
            (Ask::items(&api(), 1391).url.as_str(), "[]".to_string()),
        ]);
        match robots {
            Some(txt) => fake.with_robots(DOCS, txt),
            None => fake,
        }
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

    /// Where no host objects, every document a meeting names is kept, newest meeting first,
    /// and attachments after every meeting.
    #[tokio::test]
    async fn every_meeting_gives_its_agenda_minutes_items_and_attachments() {
        let crawl = served(None);

        let got = Legistar.enumerate(&calendar(), &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                "https://hillsboroughcounty.legistar1.com/hillsboroughcounty/meetings/2026/11/1398_A_Zoning_Hearing_Master_26-11-16_Agenda.pdf",
                "https://hillsboroughcounty.legistar1.com/hillsboroughcounty/meetings/2026/11/1398_M_Zoning_Hearing_Master_26-11-16_Results.pdf",
                "https://webapi.legistar.com/v1/hillsboroughcounty/events/1398/eventitems?AgendaNote=1&MinutesNote=1&Attachments=1",
                "https://hillsboroughcounty.legistar1.com/hillsboroughcounty/meetings/2026/11/1391_A_BOCC_Land_Use_26-11-10_Agenda.pdf",
                "https://webapi.legistar.com/v1/hillsboroughcounty/events/1391/eventitems?AgendaNote=1&MinutesNote=1&Attachments=1",
                "https://hillsboroughcounty.legistar1.com/hillsboroughcounty/attachments/ae602428-8df6-4abe-ba24-23a9d874af2f.pdf",
            ]
        );
        assert_eq!(got.figures["meetings"], 2);
        assert_eq!(got.figures["items"], 2);
        assert_eq!(got.figures["attachments"], 1);
        assert!(
            !got.truncated && !got.discovery_failed,
            "{:?}",
            got.warnings
        );
        assert_eq!(crawl.requests(), 3, "one page of meetings and two of items");
    }

    /// The document host turns crawlers away. The meeting items, on the API, are kept; the
    /// documents are not, and the run says where and how many.
    #[tokio::test]
    async fn a_document_host_that_disallows_everything_is_said_by_name() {
        let crawl = served(Some(NO_CRAWLERS));

        let got = Legistar.enumerate(&calendar(), &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                "https://webapi.legistar.com/v1/hillsboroughcounty/events/1398/eventitems?AgendaNote=1&MinutesNote=1&Attachments=1",
                "https://webapi.legistar.com/v1/hillsboroughcounty/events/1391/eventitems?AgendaNote=1&MinutesNote=1&Attachments=1",
            ]
        );
        assert_eq!(got.figures["disallowed"], 4);
        assert!(
            got.warnings.iter().any(|w| w.starts_with(
                "hillsboroughcounty.legistar1.com's robots.txt disallows the 4 document(s)"
            )),
            "{:?}",
            got.warnings
        );
    }

    /// A full page means another may follow, and it is read before any meeting's items.
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
                r#"[{"EventId":1,"EventAgendaFile":null}]"#.to_string(),
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
            "the items were not reached, and that is a floor"
        );
        assert!(
            got.addresses.contains(&Ask::items(&api(), 1).url),
            "the oldest meeting is known though its items were not read"
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
