//! `civicclerk` — a CivicClerk tenant's meetings: each meeting's agenda, packet, minutes and
//! notices, and the documents attached to each agenda item.
//!
//! Keyed on a **product**, at three sightings: Seminole (`seminolefl`), St. Pete Beach
//! (`stpetebeachfl`) and Tarpon Springs (`tarponspringsfl`). CivicPlus serves every tenant's
//! portal at `https://{tenant}.portal.civicclerk.com/`, a JavaScript application whose page
//! holds no meeting, and the meetings from a public OData API at
//! `https://{tenant}.api.civicclerk.com/v1/`, which answered without a token for all three on
//! 2026-11-22. The portal names the API that way: `https://[TENANT].api.civicclerk.com/v1`,
//! the tenant being the portal host's first label. The older `{tenant}.civicclerk.com`
//! addresses a city's own pages still link redirect to the portal.
//!
//! ## Where the meetings are
//!
//! `Events`, newest first, fifteen to a page whatever `$top` asks, each page naming the next
//! as `@odata.nextLink`. An event lists its published files (`publishedFiles`: agenda, agenda
//! packet, minutes, notice, other), each with a `fileId`, and names its agenda as `agendaId`.
//! `Meetings/{agendaId}` holds the agenda's items, nested as `childItems`, each with the
//! documents attached to it (`attachmentsList`).
//!
//! ## Which address holds each document
//!
//! The JSON's own links do not: an attachment's `pdfVersionFullPath` and an item report's
//! `pdfMediaFullPath` are Azure blob URLs signed for a day, and the same blob unsigned answers
//! 404. The portal downloads through the API instead (`docs/FIELD-NOTES.md` entry 10):
//!
//! - `Meetings/GetMeetingFileStream(fileId=N,plainText=false)` — a published file. Both keys,
//!   in that order: without `plainText`, or with the two swapped, it answers 404;
//!   `plainText=true` answers the file's text instead. An id the API never listed answers
//!   200 `application/pdf` with no bytes, so only listed ids are written.
//! - `Meetings/GetAttachmentFile(fileId=N)` — an item's attachment, by the attachment's `id`.
//!   The PDF whatever was uploaded: a Word document, a spreadsheet or a JPEG comes back as
//!   its PDF rendition. An unknown id answers 404.
//!
//! An item's reports (the memo the clerk's template writes) have no such route: the signed
//! URL is their only address, so they are not kept. Where the tenant publishes a packet, the
//! packet binds them.
//!
//! ## Order, and what a short walk loses
//!
//! Every page of events is read first, and every meeting's published files kept from it.
//! Then meetings are opened **newest first**, one request each, for their items'
//! attachments. An event with no published file is not opened: its agenda is unpublished and
//! `Meetings/{agendaId}` answers no items. A walk that runs out of requests has every
//! agenda, packet and minutes in its range, and the newest meetings' attachments.
//!
//! ## Bounding a walk
//!
//! The seed bounds it. Handed the portal, the walk covers every event the tenant publishes,
//! scheduled ones included. Handed the API's own events address with an OData `$filter` —
//! `https://tarponspringsfl.api.civicclerk.com/v1/Events?$filter=startDateTime ge 2026-08-22`
//! — it covers the events that filter selects; `nextLink` carries the filter. The request
//! budget (`--max-sitemaps`) applies inside that range.
//!
//! ## robots.txt
//!
//! The portal and the API serve Cloudflare's managed template: `User-agent: *` / `Allow: /`
//! with `Content-Signal: search=yes,ai-train=no,use=reference`, AI training crawlers
//! disallowed by name, no `Crawl-delay`. Every request and address is checked against the
//! API host's rules, read through [`Walk::robots`].

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt;

use futures::future::BoxFuture;
use serde::Deserialize;
use url::Url;

use super::{Addresses, Enumerated, Pass, Seed, Strategy, StrategyDef, Walk};
use crate::domain::Note;
use crate::strategies::{Keyed, Recognition};

pub struct CivicClerk;

inventory::submit! { StrategyDef { name: "civicclerk", it: &CivicClerk } }

/// The product, as a recognition names it.
const PRODUCT: &str = "CivicClerk";

/// Every tenant's hosts end here.
const DOMAIN: &str = "civicclerk.com";

/// Pages of events are depth 0, a meeting's agenda 1.
const MAX_DEPTH: usize = 1;

/// The figures a walk reports, in the order they are written.
const FIGURES: [&str; 7] = [
    "meetings",
    "agendas",
    "packets",
    "minutes",
    "other",
    "opened",
    "attachments",
];

impl Strategy for CivicClerk {
    fn name(&self) -> &'static str {
        "civicclerk"
    }

    fn recognise(&self, seed: &Seed) -> Option<Recognition> {
        let at = seed.final_url()?;
        let (tenant, filter) = tenant_of(&at)?;
        // The portal is confirmed on its bytes; a name under the domain is not a tenant.
        if !is_api(&at) && !seed.text().contains(PRODUCT) {
            return None;
        }
        let mut r = Recognition::new(self.name(), Keyed::Product(PRODUCT))
            .seeing(
                "tenant",
                format!("`{tenant}`, from {}", at.host_str().unwrap_or_default()),
            )
            .seeing("api", format!("{}Events", api_of(&tenant)))
            .seeing(
                "documents",
                "each meeting's published files, and each agenda item's attachments",
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
            let (tenant, filter) = tenant_of(&at)
                .ok_or_else(|| anyhow::anyhow!("{at} does not name a CivicClerk tenant"))?;
            let api = Url::parse(&api_of(&tenant))?;
            let rules = crawl.robots(api.as_str()).await;

            let mut pass = Pass::new(crawl, "calendar", MAX_DEPTH);
            pass.note(Note::new("tenant", tenant.clone()));
            if let Some(f) = &filter {
                pass.note(Note::new("filter", f.clone()));
            }
            pass.push(Ask::events(&api, filter.as_deref()), 0);

            // Agendas to open, newest first, once every page of events is read.
            let mut agendas: VecDeque<u64> = VecDeque::new();
            let mut queued: HashSet<u64> = HashSet::new();
            let mut tally: BTreeMap<&'static str, u64> = BTreeMap::new();
            let mut blocked = 0u64;
            loop {
                let Some((ask, depth)) = pass.next_to_visit() else {
                    // Nothing queued: open the next agenda. A ceiling leaves the queue
                    // holding what it stopped before, and ends the walk here.
                    if pass.pending() == 0
                        && let Some(id) = agendas.pop_front()
                    {
                        pass.push(Ask::meeting(&api, id), 1);
                        continue;
                    }
                    break;
                };
                pass.visiting(ask.to_string());
                if !rules.allowed(ask.url.as_str()) {
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
                    Kind::Events => {
                        let Ok(page) = serde_json::from_slice::<Page>(&body) else {
                            pass.warn(format!("{ask}: the answer is not a page of events"));
                            continue;
                        };
                        pass.read_surface();
                        *tally.entry("meetings").or_default() += page.value.len() as u64;
                        for event in &page.value {
                            for file in &event.published_files {
                                if pass.full() {
                                    break;
                                }
                                *tally.entry(file.figure()).or_default() += 1;
                                pass.keep(meeting_file(&api, file.file_id), &rules);
                            }
                            if event.agenda_id > 0
                                && !event.published_files.is_empty()
                                && queued.insert(event.agenda_id)
                            {
                                agendas.push_back(event.agenda_id);
                            }
                        }
                        match page.next.map(|n| Url::parse(&n)) {
                            Some(Ok(next)) if next.host_str() == api.host_str() => {
                                pass.push(Ask::page(next), depth);
                            }
                            Some(_) => pass.warn(format!(
                                "{ask}: the next page is not on {}; the calendar stops here",
                                api.host_str().unwrap_or_default()
                            )),
                            None => {}
                        }
                    }
                    Kind::Meeting => {
                        let Ok(meeting) = serde_json::from_slice::<Meeting>(&body) else {
                            pass.warn(format!("{ask}: the answer is not an agenda"));
                            continue;
                        };
                        *tally.entry("opened").or_default() += 1;
                        let mut ids = Vec::new();
                        for item in &meeting.items {
                            item.attachments(&mut ids);
                        }
                        for id in ids {
                            if pass.full() {
                                break;
                            }
                            *tally.entry("attachments").or_default() += 1;
                            pass.keep(attachment(&api, id), &rules);
                        }
                    }
                }
            }

            if blocked > 0 {
                pass.warn(format!(
                    "{}'s robots.txt disallows {} request(s) this walk makes; if the documents \
                     are public records, `source add --ignore-robots` rules that it does not bind",
                    api.host_str().unwrap_or_default(),
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

/// The tenant a seed names, and the OData filter it carries if it is the API itself.
///
/// `tarponspringsfl.portal.civicclerk.com` → `tarponspringsfl`, as the portal's own script
/// derives it. `tarponspringsfl.api.civicclerk.com/v1/Events?$filter=…` → the same tenant,
/// and the filter.
fn tenant_of(at: &Url) -> Option<(String, Option<String>)> {
    let host = at.host_str()?.to_ascii_lowercase();
    let (tenant, service) = host
        .strip_suffix(DOMAIN)?
        .strip_suffix('.')?
        .split_once('.')?;
    if tenant.is_empty() || tenant == "www" {
        return None;
    }
    match service {
        "portal" => Some((tenant.to_string(), None)),
        "api" => {
            let mut segments = at.path_segments()?;
            if segments.next()? != "v1" || !segments.next()?.eq_ignore_ascii_case("Events") {
                return None;
            }
            let filter = at
                .query_pairs()
                .find(|(k, _)| k == "$filter")
                .map(|(_, v)| v.into_owned());
            Some((tenant.to_string(), filter))
        }
        _ => None,
    }
}

fn is_api(at: &Url) -> bool {
    at.host_str()
        .is_some_and(|h| h.to_ascii_lowercase().ends_with(&format!(".api.{DOMAIN}")))
}

/// The API a tenant is served from.
fn api_of(tenant: &str) -> String {
    format!("https://{tenant}.api.{DOMAIN}/v1/")
}

/// A published file's one address; see the module docs for why both keys, in this order.
fn meeting_file(api: &Url, file: u64) -> String {
    format!("{api}Meetings/GetMeetingFileStream(fileId={file},plainText=false)")
}

/// An item attachment's one address.
fn attachment(api: &Url, id: u64) -> String {
    format!("{api}Meetings/GetAttachmentFile(fileId={id})")
}

/// One request this walk makes.
#[derive(Clone, Debug)]
struct Ask {
    url: Url,
    kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A page of events.
    Events,
    /// One agenda, for its items' attachments.
    Meeting,
}

impl Ask {
    /// The first page of events, newest first.
    fn events(api: &Url, filter: Option<&str>) -> Self {
        let mut url = api.join("Events").expect("a relative path joins");
        {
            let mut query = url.query_pairs_mut();
            if let Some(filter) = filter {
                query.append_pair("$filter", filter);
            }
            query.append_pair("$orderby", "startDateTime desc");
        }
        Self::page(url)
    }

    /// A page of events at the address the previous page named.
    fn page(url: Url) -> Self {
        Self {
            url,
            kind: Kind::Events,
        }
    }

    fn meeting(api: &Url, agenda: u64) -> Self {
        Self {
            url: api
                .join(&format!("Meetings/{agenda}"))
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

/// A page of `Events`.
#[derive(Debug, Deserialize)]
struct Page {
    value: Vec<Event>,
    #[serde(default, rename = "@odata.nextLink")]
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Event {
    /// 0 for an event with no agenda.
    #[serde(default)]
    agenda_id: u64,
    #[serde(default)]
    published_files: Vec<PublishedFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishedFile {
    file_id: u64,
    /// `Agenda`, `Agenda Packet`, `Minutes`, `Notice`, `Other`.
    #[serde(rename = "type")]
    kind: String,
}

impl PublishedFile {
    /// The figure it is counted under.
    fn figure(&self) -> &'static str {
        match self.kind.as_str() {
            "Agenda" => "agendas",
            "Agenda Packet" => "packets",
            "Minutes" => "minutes",
            _ => "other",
        }
    }
}

/// `Meetings/{agendaId}`: the agenda's items.
#[derive(Debug, Deserialize)]
struct Meeting {
    #[serde(default)]
    items: Vec<Item>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    #[serde(default)]
    attachments_list: Vec<Attachment>,
    #[serde(default)]
    child_items: Vec<Item>,
}

impl Item {
    /// Its attachments' ids, then its children's, in agenda order.
    fn attachments(&self, out: &mut Vec<u64>) {
        out.extend(self.attachments_list.iter().map(|a| a.id));
        for child in &self.child_items {
            child.attachments(out);
        }
    }
}

#[derive(Debug, Deserialize)]
struct Attachment {
    id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::crawl::tests::{Fake, seed};

    const PORTAL: &str = "https://tarponspringsfl.portal.civicclerk.com/";
    const API: &str = "https://tarponspringsfl.api.civicclerk.com/v1/";

    /// The portal's page as served on 2026-11-22, cut to its head: no meeting in it.
    const PORTAL_PAGE: &str = r#"<!doctype html><html lang="en"><head><meta charset="utf-8" />
<meta name="description" content="CivicClerk events and agendas for citizens" />
<title>Public Portal • CivicClerk</title>
<script type="module" crossorigin src="/assets/index-Cf9Ink4-.js"></script>
</head><body><noscript>You need to enable JavaScript to run this app.</noscript><div id="root"></div></body></html>"#;

    /// Tarpon Springs' events as the API answered on 2026-11-22, cut to the fields that
    /// matter: two scheduled events from the first page, one with an agenda id and no
    /// files and one with neither, then a cancelled meeting and the November 17 commission
    /// meeting from page 9, and page 9's own `nextLink`.
    const PAGE_1: &str = r#"{"@odata.context":"https://tarponspringsfl.api.civicclerk.com/v1/$metadata#Events","value":[
{"id":1365,"eventName":"Pending Items","startDateTime":"2027-12-31T18:00:00Z","agendaId":461,"publishedFiles":[]},
{"id":1639,"eventName":"Board of Commissioners Regular Session","startDateTime":"2027-12-28T18:00:00Z","agendaId":0,"publishedFiles":[]},
{"id":1331,"eventName":"Board of Adjustment Meeting - CANCELLED","startDateTime":"2026-11-18T18:30:00Z","agendaId":436,"publishedFiles":[]},
{"id":1443,"eventName":"Board of Commissioners Regular Session","startDateTime":"2026-11-17T18:00:00Z","agendaId":515,"publishedFiles":[{"id":0,"fileId":3802,"type":"Agenda","name":"November 17, 2026 BOC Regular Session","url":"stream/TARPONSPRINGSFL/fc8bedb0-0172-4557-a17a-e357dca591c1.pdf","fileType":1},{"id":0,"fileId":3806,"type":"Agenda Packet","name":"November 17, 2026 BOC Regular Session Packet","url":"stream/TARPONSPRINGSFL/0d6c69bb-000f-44d4-aace-6c1315b59b34.pdf","fileType":2}]}],
"@odata.nextLink":"https://tarponspringsfl.api.civicclerk.com/v1/Events?$orderby=startDateTime%20desc&$skiptoken=startDateTime-2026-11-17T18%3A00%3A00Z,id-1443"}"#;

    const PAGE_2_AT: &str = "https://tarponspringsfl.api.civicclerk.com/v1/Events?$orderby=startDateTime%20desc&$skiptoken=startDateTime-2026-11-17T18%3A00%3A00Z,id-1443";

    /// Page 10's first event, the last page here: no `nextLink`.
    const PAGE_2: &str = r#"{"@odata.context":"https://tarponspringsfl.api.civicclerk.com/v1/$metadata#Events","value":[
{"id":1324,"eventName":"Planning & Zoning Meeting","startDateTime":"2026-11-16T18:30:00Z","agendaId":460,"publishedFiles":[{"id":0,"fileId":3784,"type":"Agenda","name":"Planning and Zoning Board Agenda","url":"stream/TARPONSPRINGSFL/baadeb41-2b83-41fa-a849-8695f75b7cbd.pdf","fileType":1},{"id":0,"fileId":3785,"type":"Agenda Packet","name":"Planning and Zoning Board Agenda Packet","url":"stream/TARPONSPRINGSFL/3d2c1bfc-7c9a-4e2d-b8a9-b9003b932e13.pdf","fileType":2}]}]}"#;

    /// Agenda 515, cut to three sections: a proclamation's one attachment, a purchase with a
    /// report and two of its seven attachments, and an amendment whose attachments are on
    /// it and on its two ordinances, nested a level down. The signed blob links are cut
    /// short.
    const MEETING_515: &str = r#"{"@odata.context":"https://tarponspringsfl.api.civicclerk.com/v1/$metadata#Meetings/$entity","id":515,"agendaIsPublish":true,"publishedFiles":[{"id":0,"fileId":3802,"type":"Agenda","name":"November 17, 2026 BOC Regular Session","url":null,"fileType":0}],"items":[
{"id":9367,"agendaObjectItemName":"PROCLAMATIONS","isSection":1,"attachmentsList":[],"reportsList":[],"childItems":[
 {"id":10487,"agendaObjectItemName":"Epilepsy Awareness Month","isSection":0,"attachmentsList":[{"id":7842,"agendaObjectItemId":10487,"fileName":"Epilepsy Awareness Month - 2026","contentType":"application/pdf","isPublished":true,"mediaFullPath":"stream/TARPONSPRINGSFL/8389de5ade744d68926ca0afb3ef5c63.pdf","pdfVersionFullPath":"https://civicclerk.blob.core.windows.net/stream/TARPONSPRINGSFL/8389de5ade744d68926ca0afb3ef5c63.pdf?sv=2026-06-06&se=2026-11-23T21%3A25%3A01Z&sr=b&sp=r&sig=x"}],"reportsList":[],"childItems":[]}]},
{"id":9370,"agendaObjectItemName":"CONSENT AGENDA","isSection":1,"attachmentsList":[],"reportsList":[],"childItems":[
 {"id":11194,"agendaObjectItemName":"Award File No. 270043-C-PH, Purchase of Chevrolet Silverado","isSection":0,"attachmentsList":[{"id":7840,"fileName":"F.Y. 2027 F.D. Silverado Signed","isPublished":true},{"id":7787,"fileName":"F.D. Silverado Background","isPublished":true}],"reportsList":[{"id":2753,"agendaObjItemReportName":"Department Memo","pdfMediaFullPath":"https://civicclerk.blob.core.windows.net/stream/TARPONSPRINGSFL/c9f74f77-c7cd-4701-b271-5d85bb3b94ec.pdf?sv=2026-06-06&se=2026-11-23T21%3A25%3A01Z&sr=b&sp=r&sig=x"}],"childItems":[]}]},
{"id":9374,"agendaObjectItemName":"ORDINANCES AND RESOLUTIONS","isSection":1,"attachmentsList":[],"reportsList":[],"childItems":[
 {"id":10645,"agendaObjectItemName":"Land Development Code (LDC) Articles X and XII Amendments","isSection":0,"attachmentsList":[{"id":7702,"fileName":"(1) Staff Presentation","isPublished":true},{"id":7764,"fileName":"(2) Legal Public Notice","isPublished":true}],"reportsList":[{"id":2734,"agendaObjItemReportName":"BOC Memorandum"}],"childItems":[
  {"id":10646,"agendaObjectItemName":"Ordinance 2026-17: LDC Article X Amendments","attachmentsList":[{"id":7704,"fileName":"(1) Draft Ordinance #2026-17_Article X Amendments","isPublished":true}],"reportsList":[],"childItems":[]},
  {"id":10647,"agendaObjectItemName":"Ordinance 2026-18: LDC Article XII Amendments","attachmentsList":[{"id":7706,"fileName":"(1) Draft Ordinance #2026-18_Art XII Amendments","isPublished":true}],"reportsList":[],"childItems":[]}]}]}]}"#;

    /// Agenda 460, cut to one item and one of its ten attachments.
    const MEETING_460: &str = r#"{"id":460,"agendaIsPublish":true,"items":[
{"id":9500,"agendaObjectItemName":"PUBLIC HEARINGS","isSection":1,"attachmentsList":[],"reportsList":[],"childItems":[
 {"id":10637,"agendaObjectItemName":"Resolution 2026-33; (Quasi-Judicial) Conditional Use","attachmentsList":[{"id":7674,"fileName":"(1) Staff Presentation","isPublished":true}],"reportsList":[],"childItems":[]}]}]}"#;

    fn api() -> Url {
        Url::parse(API).unwrap()
    }

    fn file(id: u64) -> String {
        meeting_file(&api(), id)
    }

    fn att(id: u64) -> String {
        attachment(&api(), id)
    }

    fn served() -> Fake {
        Fake::new([
            (Ask::events(&api(), None).url.as_str(), PAGE_1.to_string()),
            (PAGE_2_AT, PAGE_2.to_string()),
            (
                Ask::meeting(&api(), 515).url.as_str(),
                MEETING_515.to_string(),
            ),
            (
                Ask::meeting(&api(), 460).url.as_str(),
                MEETING_460.to_string(),
            ),
        ])
    }

    /// Every page of events first, each meeting's published files kept from it. Then each
    /// agenda with a published file, newest first, gives its items' attachments at every
    /// depth, and nothing else: not a report, not a signed link. An event with no published
    /// file is never opened.
    #[tokio::test]
    async fn every_page_then_every_published_agendas_attachments() {
        let crawl = served();

        let got = CivicClerk
            .enumerate(&seed(PORTAL_PAGE, PORTAL), &crawl)
            .await
            .unwrap();

        assert_eq!(
            got.addresses,
            [
                file(3802),
                file(3806),
                file(3784),
                file(3785),
                att(7842),
                att(7840),
                att(7787),
                att(7702),
                att(7764),
                att(7704),
                att(7706),
                att(7674),
            ]
        );
        assert_eq!(
            crawl.requests(),
            4,
            "two pages of events and two agendas; 461 and 436 list no file"
        );
        assert_eq!(got.figures["meetings"], 5);
        assert_eq!(got.figures["agendas"], 2);
        assert_eq!(got.figures["packets"], 2);
        assert_eq!(got.figures["minutes"], 0);
        assert_eq!(got.figures["opened"], 2);
        assert_eq!(got.figures["attachments"], 8);
        assert!(
            !got.truncated && !got.discovery_failed && got.warnings.is_empty(),
            "{:?}",
            got.warnings
        );
    }

    /// The newest agenda is opened first, so a budget that runs out keeps every meeting's
    /// published files and the newest meeting's attachments, and says it stopped.
    #[tokio::test]
    async fn a_walk_out_of_budget_has_the_newest_meetings_attachments() {
        let crawl = served().with_budget(3);

        let got = CivicClerk
            .enumerate(&seed(PORTAL_PAGE, PORTAL), &crawl)
            .await
            .unwrap();

        assert_eq!(crawl.requests(), 3, "agenda 460 was never fetched");
        assert!(got.addresses.contains(&file(3785)), "every page was read");
        assert!(got.addresses.contains(&att(7706)));
        assert!(!got.addresses.contains(&att(7674)));
        assert!(got.truncated, "{:?}", got.warnings);
    }

    /// A filtered events address is its own bound: the walk asks the API with that filter,
    /// newest first, and follows the pages it names.
    #[tokio::test]
    async fn a_filtered_events_seed_asks_with_its_filter() {
        let at = "https://tarponspringsfl.api.civicclerk.com/v1/Events?$filter=startDateTime%20ge%202026-11-16";
        let first = Ask::events(&api(), Some("startDateTime ge 2026-11-16"));
        let crawl = Fake::new([(first.url.as_str(), PAGE_2.to_string())]).with_budget(1);

        let got = CivicClerk.enumerate(&seed("{}", at), &crawl).await.unwrap();

        assert_eq!(
            first.url.as_str(),
            "https://tarponspringsfl.api.civicclerk.com/v1/Events?%24filter=startDateTime+ge+2026-11-16&%24orderby=startDateTime+desc",
            "the form-encoded keys the API was measured to accept"
        );
        assert_eq!(got.addresses, [file(3784), file(3785)]);
        assert!(got.notes.iter().any(|n| n.label == "filter"));
    }

    /// Recognised at the portal on its bytes, and at the API's events address with or
    /// without a filter. Any other host, or a portal host serving something else, is not a
    /// tenant.
    #[test]
    fn a_portal_and_its_api_name_the_same_tenant() {
        let r = CivicClerk
            .recognise(&seed(PORTAL_PAGE, PORTAL))
            .expect("a CivicClerk portal");
        assert_eq!(r.keyed_on, Keyed::Product(PRODUCT));
        assert!(
            r.evidence
                .iter()
                .any(|n| n.detail.contains("`tarponspringsfl`"))
        );

        let filtered =
            "https://seminolefl.api.civicclerk.com/v1/Events?$filter=startDateTime ge 2026-08-22";
        assert_eq!(
            tenant_of(&Url::parse(filtered).unwrap()),
            Some((
                "seminolefl".to_string(),
                Some("startDateTime ge 2026-08-22".to_string())
            ))
        );
        assert!(CivicClerk.recognise(&seed("{}", filtered)).is_some());

        for not in [
            "https://www.civicclerk.com/",
            "https://civicclerk.com/",
            "https://seminolefl.api.civicclerk.com/v1/Meetings/165",
            "https://civicclerk.blob.core.windows.net/stream/SEMINOLEFL/x.pdf",
        ] {
            assert!(
                CivicClerk.recognise(&seed(PORTAL_PAGE, not)).is_none(),
                "{not}"
            );
        }
        assert!(
            CivicClerk
                .recognise(&seed("<html>parked</html>", PORTAL))
                .is_none(),
            "a portal is confirmed on its bytes"
        );
    }
}
