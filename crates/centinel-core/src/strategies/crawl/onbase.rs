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
//! ## Bounding a walk
//!
//! The seed bounds it, as a path bounds `links`. Handed the application's front door, the
//! walk covers every meeting since [`EARLIEST`]. Handed a search page —
//! `…/Meetings/Search?dropid=11&dropsv=08/22/2026&dropev=11/20/2026` — it covers that range,
//! and the first answer is the seed already in hand.

use std::fmt;

use futures::future::BoxFuture;
use jiff::civil::Date;
use serde::Deserialize;
use url::Url;

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
            pass.push(first, 0);

            let (mut meetings, mut documents, mut windows) = (0u64, 0u64, 0u64);
            while let Some((window, depth)) = pass.next_to_visit() {
                pass.visiting(format!("meetings {window}"));
                let body = match (depth, seeded) {
                    (0, Some(_)) => seed.page.bytes.clone(),
                    _ => match pass.walk().get(window.search(&root).as_str()).await {
                        Ok(f) => f.bytes,
                        Err(refusal) => {
                            pass.refused(&window, &refusal);
                            continue;
                        }
                    },
                };
                let text = String::from_utf8_lossy(&body);
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
                            pass.push(newer, depth + 1);
                            pass.push(older, depth + 1);
                            continue;
                        }
                        None => pass.short(format!(
                            "{window}: one day holds more meetings than the search answers \
                             ({}); the rest of that day is not collected",
                            results.meetings.len()
                        )),
                    }
                }

                windows += 1;
                for meeting in &results.meetings {
                    meetings += 1;
                    for (doc_type, published) in meeting.published() {
                        if !published {
                            continue;
                        }
                        if pass.full() {
                            break;
                        }
                        documents += 1;
                        pass.keep(document(&root, meeting.id, doc_type), &seed.robots);
                    }
                }
            }

            pass.figure("meetings", meetings);
            pass.figure("documents", documents);
            pass.figure("windows", windows);
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
}

/// A range of days, both ends included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Window {
    from: Date,
    to: Date,
}

impl Window {
    /// The search for this window, as the application's own form builds it.
    fn search(&self, root: &Url) -> String {
        format!(
            "{root}Meetings/Search?dropid=11&dropsv={}&dropev={}",
            self.from.strftime("%m/%d/%Y"),
            self.to.strftime("%m/%d/%Y")
        )
    }

    /// The two halves, older first, or `None` for a single day.
    fn halves(&self) -> Option<(Window, Window)> {
        let days = self.from.until(self.to).ok()?.get_days();
        if days < 1 {
            return None;
        }
        let mid = self
            .from
            .checked_add(jiff::Span::new().days(i64::from(days / 2)))
            .ok()?;
        let next = mid.tomorrow().ok()?;
        Some((
            Window {
                from: self.from,
                to: mid,
            },
            Window {
                from: next,
                to: self.to,
            },
        ))
    }
}

impl fmt::Display for Window {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} to {}", self.from, self.to)
    }
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

    /// A search seed is a window already answered: no request, and an address per document
    /// the meeting publishes — never one it does not.
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
        let crawl = Fake::new([]);

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
        assert_eq!(crawl.requests(), 0, "the seed answered");
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
        assert_eq!(crawl.requests(), 2);
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

    #[test]
    fn halving_covers_every_day_once() {
        let w = Window {
            from: jiff::civil::date(2024, 1, 1),
            to: jiff::civil::date(2024, 1, 4),
        };
        let (a, b) = w.halves().unwrap();
        assert_eq!((a.from, a.to), (w.from, jiff::civil::date(2024, 1, 2)));
        assert_eq!((b.from, b.to), (jiff::civil::date(2024, 1, 3), w.to));
        let day = Window {
            from: w.from,
            to: w.from,
        };
        assert!(day.halves().is_none());
    }
}
