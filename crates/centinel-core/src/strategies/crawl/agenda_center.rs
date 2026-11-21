//! `agenda-center` — CivicPlus Agenda Center's agendas, packets and minutes, and the
//! documents attached to each agenda item.
//!
//! Keyed on a **product**, at two populated sightings: Indian Shores
//! (`myindianshores.com/AgendaCenter`) and Lake Wales (`lakewalesfl.gov/AgendaCenter`).
//! CivicPlus serves the module at `/AgendaCenter` on every CivicEngage site, so it is
//! recognised by the page it serves and not by the host.
//!
//! ## Where the meetings are
//!
//! The front door renders one year per category, that category's newest. Every other year
//! is behind `changeYear(year, catID)`, a script that POSTs to
//! `/AgendaCenter/UpdateCategoryList` (`docs/FIELD-NOTES.md` entry 9). The module's own
//! search is a GET, and a search bounded to one calendar year answers every category's
//! meetings in that year on one page:
//!
//! ```text
//! AgendaCenter/Search/?term=&CIDs=all&startDate=01/01/2025&endDate=12/31/2025&dateRange=&dateSelector=
//! ```
//!
//! A search over several years answers as the front door does, one year per category, so
//! the walk asks one year at a time. The years to ask are the ones the seed's year tabs
//! name. At Lake Wales the 2026 search named exactly the 2026 files the front door did, and
//! the 2019 search exactly what `UpdateCategoryList` answers for the city commission's 2019;
//! neither answer is paged or capped.
//!
//! ## Which files
//!
//! A meeting's row links its files under `/AgendaCenter/ViewFile/`:
//!
//! - `Agenda/_MMDDYYYY-ID`, the agenda PDF;
//! - `Agenda/_MMDDYYYY-ID?packet=true`, the packet: the agenda with every item's documents;
//! - `Minutes/_MMDDYYYY-ID`, the minutes;
//! - `Agenda/_MMDDYYYY-ID?html=true`, the agenda as HTML, where it was built in the product
//!   rather than uploaded as a finished file.
//!
//! Only the id and the flag decide the file: `_01012000-1765` and `1765` answer the same
//! bytes as `_11172026-1765`. So each is written with the id alone, and a meeting moved to
//! another day keeps its address. An id that does not exist answers a real 404.
//!
//! ## What the council voted on: each item's documents
//!
//! The HTML agenda links each item's documents as `ViewFile/Item/ITEM?fileID=FILE`. Both
//! numbers decide the file, since another item's id, or no `fileID`, answers 404, and any
//! other key is ignored. Those are kept. The HTML agenda itself is not: it is the agenda
//! PDF's text again. An agenda uploaded as a finished file has no HTML form and no items to
//! open, which is every Indian Shores agenda.
//!
//! The packet binds the same documents into one PDF, 28.8 MB for one Lake Wales commission
//! meeting. Both are kept: the packet is what the city published, and an attachment is one
//! document under its own name.
//!
//! An HTML agenda costs a request, so every listing is read first, then the HTML agendas
//! **newest first**. A walk that runs out of requests has every agenda, packet and minutes in
//! its range, and the newest meetings' attachments.
//!
//! ## Bounding a walk
//!
//! The seed bounds it. Handed the front door, the walk asks every year the module lists.
//! Handed a search with a range —
//! `…/AgendaCenter/Search/?term=&CIDs=all&startDate=08/22/2026&endDate=11/20/2026` — it asks
//! the years that range touches, each cut to the range, and a range inside one year is the
//! seed already in hand. The request budget (`--max-sitemaps`) applies inside that range.
//!
//! ## robots.txt
//!
//! Every CivicPlus host seen serves the same template: nothing under `/AgendaCenter` is
//! disallowed, and `Crawl-delay: 10` applies to every crawler. The host's pacer honours it,
//! so a walk is ten seconds a request, and the recognition says so.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;

use futures::future::BoxFuture;
use jiff::civil::Date;
use url::Url;

use super::window::Window;
use super::{Addresses, Enumerated, Pass, Seed, Strategy, StrategyDef, Walk};
use crate::domain::Note;
use crate::strategies::{Keyed, Recognition};

pub struct AgendaCenter;

inventory::submit! { StrategyDef { name: "agenda-center", it: &AgendaCenter } }

/// The product, as a recognition names it.
const PRODUCT: &str = "CivicPlus Agenda Center";

/// The address a listing's script posts a year to. On the front door and on every search
/// answer, empty or not.
const LISTING: &str = "/AgendaCenter/UpdateCategoryList";

/// The call each year tab makes: `changeYear(2025, 4, 'a1')`.
const YEAR_TAB: &str = "changeYear(";

/// Every request is at depth 0: a listing names files, and so does an HTML agenda.
const MAX_DEPTH: usize = 0;

/// The figures a walk reports, in the order they are written.
const FIGURES: [&str; 6] = [
    "years",
    "agendas",
    "packets",
    "minutes",
    "html",
    "attachments",
];

impl Strategy for AgendaCenter {
    fn name(&self) -> &'static str {
        "agenda-center"
    }

    fn recognise(&self, seed: &Seed) -> Option<Recognition> {
        let at = seed.final_url()?;
        let page = seed.text();
        if !listing_at(&at) || !page.contains(LISTING) {
            return None;
        }
        let years = years_in(&page);
        let files = files_in(&page, &at);
        // A module with no category in it is not one this can collect. Seven of the eight
        // on the box are that, and their meetings are on another product.
        if years.is_empty() && files.is_empty() {
            return None;
        }
        let mut r = Recognition::new(self.name(), Keyed::Product(PRODUCT))
            .seeing(
                "module",
                format!("{} lists meetings by category and year", at.path()),
            )
            .seeing("files", format!("{} linked on this page", files.len()));
        if let (Some(first), Some(last)) = (years.first(), years.last()) {
            r = r.seeing(
                "years",
                format!("{} listed, {first} to {last}", years.len()),
            );
        }
        if let Some(window) = window_of(&at) {
            r = r.seeing("window", window.to_string());
        }
        if let Some(delay) = seed.robots.crawl_delay() {
            r = r.warning(
                "pace",
                format!(
                    "robots.txt asks {} s between requests; a walk is one request per year \
                     and one per HTML agenda, at that pace",
                    delay.as_secs()
                ),
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
            let root = at.join("/AgendaCenter/")?;
            let seeded = window_of(&at);

            let mut pass = Pass::new(crawl, "agenda center", MAX_DEPTH);
            pass.note(Note::new("module", root.to_string()));
            if let Some(window) = seeded {
                pass.note(Note::new("window", window.to_string()));
            }
            pass.push(Ask::Seed, 0);

            // A file is linked from its row and again from the row's download menu, and a
            // year's rows are on the front door and in that year's search: each is taken once.
            let mut seen: HashSet<File> = HashSet::new();
            // Read once every listing is; sorted oldest first, so `pop` takes the newest.
            let mut agendas: Vec<Html> = Vec::new();
            let mut tally: BTreeMap<&'static str, u64> = BTreeMap::new();
            loop {
                let Some((ask, _)) = pass.next_to_visit() else {
                    // Nothing queued: read the next HTML agenda. A ceiling leaves the queue
                    // holding what it stopped before, and ends the walk here.
                    if pass.pending() == 0
                        && let Some(html) = agendas.pop()
                    {
                        pass.push(Ask::Agenda(html), 0);
                        continue;
                    }
                    break;
                };
                pass.visiting(ask.to_string());
                let (body, from) = match ask.url(&root) {
                    None => (seed.page.bytes.clone(), at.clone()),
                    Some(url) => match pass.walk().get(&url).await {
                        Ok(f) => (f.bytes, Url::parse(&url)?),
                        Err(refusal) => {
                            pass.refused(&ask, &refusal);
                            continue;
                        }
                    },
                };
                let text = String::from_utf8_lossy(&body);

                let listing = !matches!(ask, Ask::Agenda(_));
                if listing {
                    if !text.contains(LISTING) {
                        pass.warn(format!("{ask}: the answer is not an Agenda Center listing"));
                        continue;
                    }
                    pass.read_surface();
                }
                match &ask {
                    Ask::Seed => {
                        for window in years_to_ask(&text, seeded) {
                            pass.push(Ask::Year(window), 0);
                        }
                    }
                    Ask::Year(_) => *tally.entry("years").or_default() += 1,
                    Ask::Agenda(_) => *tally.entry("html").or_default() += 1,
                }

                for file in files_in(&text, &from) {
                    // An HTML agenda is followed from a listing only, which keeps the walk
                    // inside the years it asked for.
                    if !listing && matches!(file, File::Html(_)) {
                        continue;
                    }
                    if pass.full() {
                        break;
                    }
                    if !seen.insert(file.clone()) {
                        continue;
                    }
                    match file {
                        File::Html(html) => agendas.push(html),
                        File::Doc(doc) => {
                            *tally.entry(doc.figure()).or_default() += 1;
                            pass.keep(doc.address(&root), &seed.robots);
                        }
                    }
                }
                agendas.sort();
            }

            for key in FIGURES {
                pass.figure(key, tally.get(key).copied().unwrap_or(0));
            }
            Ok(pass.finish())
        })
    }

    /// Each address is the file itself.
    fn addresses_are(&self) -> Addresses {
        Addresses::Documents
    }
}

/// One request this walk makes.
#[derive(Clone, Debug)]
enum Ask {
    /// The listing the seed is, already in hand.
    Seed,
    /// Every category's meetings in one year, cut to the seed's range.
    Year(Window),
    /// One agenda as HTML, for the documents its items link.
    Agenda(Html),
}

impl Ask {
    /// Where to ask, or `None` for the seed, which was fetched before the walk began.
    fn url(&self, root: &Url) -> Option<String> {
        match self {
            Self::Seed => None,
            Self::Year(window) => Some(search_at(root, window)),
            Self::Agenda(html) => Some(html.at.to_string()),
        }
    }
}

/// How it reads in a warning, and what [`Pass`] dedups on.
impl fmt::Display for Ask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Seed => write!(f, "the listing in hand"),
            Self::Year(window) => write!(f, "meetings {window}"),
            Self::Agenda(html) => write!(f, "HTML agenda {}", html.id),
        }
    }
}

/// One file the module serves, as a link names it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum File {
    /// A document, kept.
    Doc(Doc),
    /// An agenda's HTML form, read for its items' documents and not kept.
    Html(Html),
}

/// A document, by the numbers that decide it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Doc {
    Agenda(u64),
    Packet(u64),
    Minutes(u64),
    Attachment { item: u64, file: u64 },
}

impl Doc {
    /// Its one address. See the module docs for why the day is left out.
    fn address(&self, root: &Url) -> String {
        match self {
            Self::Agenda(id) => format!("{root}ViewFile/Agenda/{id}"),
            Self::Packet(id) => format!("{root}ViewFile/Agenda/{id}?packet=true"),
            Self::Minutes(id) => format!("{root}ViewFile/Minutes/{id}"),
            Self::Attachment { item, file } => {
                format!("{root}ViewFile/Item/{item}?fileID={file}")
            }
        }
    }

    /// The figure it is counted under.
    fn figure(&self) -> &'static str {
        match self {
            Self::Agenda(_) => "agendas",
            Self::Packet(_) => "packets",
            Self::Minutes(_) => "minutes",
            Self::Attachment { .. } => "attachments",
        }
    }
}

/// An agenda's HTML form, at the address its row links. Ordered by the day the meeting is
/// held, then by id, so the newest sorts last.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Html {
    held: Option<Date>,
    id: u64,
    at: Url,
}

impl File {
    /// The file a link names, or `None` for any other address: a stylesheet
    /// (`ViewFile/Agenda/1765?css=true`), a list of previous versions, a page.
    fn of(link: &Url) -> Option<File> {
        let mut path = link.path_segments()?;
        if path.next()? != "AgendaCenter" || path.next()? != "ViewFile" {
            return None;
        }
        let (kind, name) = (path.next()?, path.next()?);
        if path.next().is_some() {
            return None;
        }
        let query: BTreeMap<_, _> = link.query_pairs().collect();
        if kind == "Item" {
            let file = query.get("fileID")?.parse().ok()?;
            let item = name.parse().ok()?;
            return Some(File::Doc(Doc::Attachment { item, file }));
        }
        let (id, held) = named(name)?;
        let flag = |key: &str| query.len() == 1 && query.get(key).is_some_and(|v| &**v == "true");
        Some(match kind {
            "Agenda" if query.is_empty() => File::Doc(Doc::Agenda(id)),
            "Agenda" if flag("packet") => File::Doc(Doc::Packet(id)),
            "Agenda" if flag("html") => File::Html(Html {
                held,
                id,
                at: link.clone(),
            }),
            "Minutes" if query.is_empty() => File::Doc(Doc::Minutes(id)),
            _ => return None,
        })
    }
}

/// `_11172026-1765` is meeting 1765, held November 17, 2026; `1765` is the same meeting, its
/// day not said.
fn named(name: &str) -> Option<(u64, Option<Date>)> {
    match name.strip_prefix('_').and_then(|n| n.split_once('-')) {
        Some((day, id)) => Some((id.parse().ok()?, held_on(day))),
        None => Some((name.parse().ok()?, None)),
    }
}

/// `11172026`, as the module writes a meeting's day into its file names.
fn held_on(mmddyyyy: &str) -> Option<Date> {
    if mmddyyyy.len() != 8 {
        return None;
    }
    let (month, day, year) = (
        mmddyyyy.get(0..2)?,
        mmddyyyy.get(2..4)?,
        mmddyyyy.get(4..8)?,
    );
    Date::new(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?).ok()
}

/// Every file a page links on its own host, in the order linked.
///
/// Read from the page's anchors, never a scan of its text. The host test drops what an HTML
/// agenda links on the product's other name (`fl-lakewales.civicplus.com`).
fn files_in(page: &str, at: &Url) -> Vec<File> {
    let scan = crate::html::Scan::new(page);
    let base = scan.base(at);
    scan.tags(&["a"])
        .into_iter()
        .filter_map(|tag| base.join(tag.attr("href")?))
        .filter(|u| u.host_str() == at.host_str())
        .filter_map(|u| File::of(&u))
        .collect()
}

/// The years a listing's tabs name: `changeYear(2025, 4, 'a1')` names 2025.
fn years_in(page: &str) -> BTreeSet<i16> {
    page.match_indices(YEAR_TAB)
        .filter_map(|(i, _)| {
            let (year, _) = page[i + YEAR_TAB.len()..].split_once(',')?;
            year.trim().parse().ok()
        })
        .collect()
}

/// The searches a seed's year tabs call for, newest first: each year cut to the seed's
/// range, less the one search the seed itself already is.
fn years_to_ask(page: &str, seeded: Option<Window>) -> Vec<Window> {
    years_in(page)
        .into_iter()
        .rev()
        .filter_map(Window::year)
        .filter_map(|year| match seeded {
            Some(range) => year.within(&range),
            None => Some(year),
        })
        .filter(|window| Some(*window) != seeded)
        .collect()
}

/// Whether an address is a listing: the front door, or a search.
fn listing_at(at: &Url) -> bool {
    let path = at.path().trim_end_matches('/');
    ["/AgendaCenter", "/AgendaCenter/Search"]
        .iter()
        .any(|p| path.eq_ignore_ascii_case(p))
}

/// The search for a window, as the module's own form builds it.
fn search_at(root: &Url, window: &Window) -> String {
    format!(
        "{root}Search/?term=&CIDs=all&startDate={}&endDate={}&dateRange=&dateSelector=",
        window.from.strftime("%m/%d/%Y"),
        window.to.strftime("%m/%d/%Y")
    )
}

/// The range a search address asks for, when the seed is one.
fn window_of(at: &Url) -> Option<Window> {
    if !at
        .path()
        .trim_end_matches('/')
        .eq_ignore_ascii_case("/AgendaCenter/Search")
    {
        return None;
    }
    let query: BTreeMap<_, _> = at.query_pairs().collect();
    let day = |key: &str| Date::strptime("%m/%d/%Y", query.get(key)?.as_bytes()).ok();
    let (from, to) = (day("startDate")?, day("endDate")?);
    (from <= to).then_some(Window { from, to })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::Robots;
    use crate::strategies::crawl::tests::{Fake, seed};

    const ROOT: &str = "https://www.lakewalesfl.gov/AgendaCenter/";
    const FRONT_DOOR: &str = "https://www.lakewalesfl.gov/AgendaCenter";

    /// The script line every listing carries, front door or search answer.
    const SCRIPT: &str = "<script type=\"text/javascript\">
	$.ajax({ type: 'POST', url: '/AgendaCenter/UpdateCategoryList', data: { year: year, catID: catID } });
</script>";

    /// Meeting 1762's row as Lake Wales served it on 2026-11-21, cut to its links: the HTML
    /// agenda, the minutes, and a download menu repeating the HTML beside the PDF, the
    /// packet and the previous versions.
    const ROW_1762: &str = r#"<tr id="row1762d3e76a92" class="catAgendaRow"><td>
		<h3 class="noMargin" id="h411032026-1762"><a id="_11032026-1762" name="_11032026-1762"></a>
			<strong aria-label="Agenda for November 3, 2026"><abbr title="November">Nov</abbr> 3, 2026</strong></h3>
	<p><a id="11032026-1762" name="1762" href="/AgendaCenter/ViewFile/Agenda/_11032026-1762?html=true" target="_blank">Agenda for the City Commission meeting on Tuesday November 3, 2026 at 6:00 p.m.</a></p></td>
		<td class="minutes"><a href="/AgendaCenter/ViewFile/Minutes/_11032026-1762" target="_blank"><img src="/Areas/AgendaCenter/Assets/Images/HomeIconMinutes.png" alt="Minutes"></a></td>
	<td class="downloads"><ol>
		<li><a class="html" href="/AgendaCenter/ViewFile/Agenda/_11032026-1762?html=true" id="anch11762d3e76a92" target="_blank">HTML</a></li>
		<li><a class="pdf" href="/AgendaCenter/ViewFile/Agenda/_11032026-1762" id="anch21762d3e76a92" target="_blank">PDF</a></li>
		<li><a class="pdf" href="/AgendaCenter/ViewFile/Agenda/_11032026-1762?packet=true" id="anch31762d3e76a92" target="_blank">Packet</a></li>
		<li><a id="anch41762d3e76a92" class="previous" href="/AgendaCenter/PreviousVersions/_11032026-1762">Previous Versions</a></li>
	</ol></td></tr>"#;

    /// Meeting 1765's row: built in the product, no minutes yet.
    const ROW_1765: &str = r#"<tr id="row1765b8c47dee" class="catAgendaRow"><td>
	<p><a id="11172026-1765" name="1765" href="/AgendaCenter/ViewFile/Agenda/_11172026-1765?html=true" target="_blank">Agenda for the Commission Meeting on Tuesday November 17, 2026 at 6:00 p.m.</a></p></td>
		<td class="minutes"></td>
	<td class="downloads"><ol>
		<li><a class="html" href="/AgendaCenter/ViewFile/Agenda/_11172026-1765?html=true" target="_blank">HTML</a></li>
		<li><a class="pdf" href="/AgendaCenter/ViewFile/Agenda/_11172026-1765" target="_blank">PDF</a></li>
		<li><a class="pdf" href="/AgendaCenter/ViewFile/Agenda/_11172026-1765?packet=true" target="_blank">Packet</a></li>
	</ol></td></tr>"#;

    /// Meeting 1684's row, 2025: an agenda uploaded as a finished PDF, and its minutes.
    const ROW_1684: &str = r#"<tr id="row1684a2917bb3" class="catAgendaRow"><td>
	<p><a id="11172025-1684" name="1684" href="/AgendaCenter/ViewFile/Agenda/_11172025-1684" target="_blank">Citizens and Police Community Relations Advisory Committee</a></p></td>
		<td class="minutes"><a href="/AgendaCenter/ViewFile/Minutes/_11172025-1684" target="_blank"><img src="/Areas/AgendaCenter/Assets/Images/HomeIconMinutes.png" alt="Minutes"></a></td>
	<td class="downloads"><ol>
		<li><a class="pdf" href="/AgendaCenter/ViewFile/Agenda/_11172025-1684" target="_blank">Agenda</a></li>
	</ol></td></tr>"#;

    /// A listing: one category, the year tabs given, the rows given.
    fn listing(years: &[i16], rows: &[&str]) -> String {
        let tabs: Vec<String> = years
            .iter()
            .enumerate()
            .map(|(i, y)| {
                format!(
                    r#"<li><a id="a{i}4" aria-label="City Commission Regular Meeting {y}" href="javascript:changeYear({y}, 4, 'a{i}')">{y}</a></li>"#
                )
            })
            .collect();
        format!(
            r#"<html><head><title>Agenda Center • Lake Wales, FL • CivicEngage</title>{SCRIPT}</head><body>
<div class="listing listingCollapse noHeader" id="cat4">
<h2 aria-controls="category-panel-4">City Commission Regular Meeting</h2>
<span id="section4"><ul class="years">{}</ul>
<table id="table4" summary="List of Agendas"><tbody>{}</tbody></table></span></div>
</body></html>"#,
            tabs.join(""),
            rows.join("")
        )
    }

    /// Meeting 1765's HTML agenda, cut to its stylesheet, a link to minutes on the
    /// product's other host name, and two items' documents.
    const HTML_1765: &str = r#"<html><head><title>Agenda - 11/17/2026</title>
<link rel="stylesheet" type="text/css" href="/AgendaCenter/ViewFile/Agenda/1765?css=true"/></head><body>
<div class="item level1"><div class="content"><h1 class="title">PUBLIC PARTICIPATION</h1>
<div class="documents"><strong>Documents:</strong><ol style="list-style:none;">
<li><a target="_blank" class="file" href="/AgendaCenter/ViewFile/Item/18371?fileID=23863">Speaker Cards Lake Wales.pdf</a></li>
</ol></div></div></div>
<div class="item level1"><div class="content"><h1 class="title">APPROVAL OF MINUTES</h1>
<p><a href="https://fl-lakewales.civicplus.com/AgendaCenter/ViewFile/Minutes/_11112026-1764" target="_blank" rel="noopener">November 11, 2026</a></p></div></div>
<div class="item level2"><div class="content">
<div class="documents"><strong>Documents:</strong><ol style="list-style:none;">
<li><a target="_blank" class="file" href="/AgendaCenter/ViewFile/Item/18398?fileID=23880">Memo for ILA amendment for CEI for RSH trail.pdf</a></li>
<li><a target="_blank" class="file" href="/AgendaCenter/ViewFile/Item/18398?fileID=23881">ILA Amendment.pdf</a></li>
</ol></div></div></div></body></html>"#;

    /// An HTML agenda whose items carry no documents, as the Airport Authority's do.
    const HTML_BARE: &str = r#"<html><body><div class="item level1"><div class="content"><h1 class="title">CALL TO ORDER</h1><div class="documents"></div></div></div></body></html>"#;

    fn year(y: i16) -> String {
        format!(
            "{ROOT}Search/?term=&CIDs=all&startDate=01/01/{y}&endDate=12/31/{y}&dateRange=&dateSelector="
        )
    }

    fn html(day: &str, id: u64) -> String {
        format!("{ROOT}ViewFile/Agenda/_{day}-{id}?html=true")
    }

    fn file(path: &str) -> String {
        format!("{ROOT}ViewFile/{path}")
    }

    /// The front door's tabs name each year to ask, newest first. Every listing is read
    /// before any HTML agenda, and every file has one address however often it is linked.
    /// The HTML agendas come last, newest first, and give their items' documents and
    /// nothing else: not the stylesheet, not the minutes linked on another host name.
    #[tokio::test]
    async fn the_front_door_walks_every_year_then_the_items() {
        let s = seed(&listing(&[2026, 2025], &[ROW_1762]), FRONT_DOOR);
        let crawl = Fake::new([
            (year(2026).as_str(), listing(&[2026], &[ROW_1765, ROW_1762])),
            (year(2025).as_str(), listing(&[2025], &[ROW_1684])),
            (html("11172026", 1765).as_str(), HTML_1765.to_string()),
            (html("11032026", 1762).as_str(), HTML_BARE.to_string()),
        ]);

        let got = AgendaCenter.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                file("Minutes/1762"),
                file("Agenda/1762"),
                file("Agenda/1762?packet=true"),
                file("Agenda/1765"),
                file("Agenda/1765?packet=true"),
                file("Agenda/1684"),
                file("Minutes/1684"),
                file("Item/18371?fileID=23863"),
                file("Item/18398?fileID=23880"),
                file("Item/18398?fileID=23881"),
            ]
        );
        assert_eq!(
            crawl.requests(),
            4,
            "two year searches and two HTML agendas; the front door was in hand"
        );
        assert_eq!(got.figures["years"], 2);
        assert_eq!(got.figures["agendas"], 3);
        assert_eq!(got.figures["packets"], 2);
        assert_eq!(got.figures["minutes"], 2);
        assert_eq!(got.figures["html"], 2);
        assert_eq!(got.figures["attachments"], 3);
        assert!(
            !got.truncated && !got.discovery_failed && got.warnings.is_empty(),
            "{:?}",
            got.warnings
        );
    }

    /// A search inside one year is the only search its range needs, and it is in hand: the
    /// only requests are the HTML agendas.
    #[tokio::test]
    async fn a_search_seed_inside_one_year_is_its_own_answer() {
        let at = format!(
            "{ROOT}Search/?term=&CIDs=all&startDate=08/22/2026&endDate=11/20/2026&dateRange=&dateSelector="
        );
        let s = seed(&listing(&[2026], &[ROW_1765, ROW_1762]), &at);
        let crawl = Fake::new([
            (html("11172026", 1765).as_str(), HTML_1765.to_string()),
            (html("11032026", 1762).as_str(), HTML_BARE.to_string()),
        ]);

        let got = AgendaCenter.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(crawl.requests(), 2, "no search was asked");
        assert_eq!(got.figures["years"], 0);
        assert_eq!(got.addresses.len(), 8, "{:?}", got.addresses);
        assert!(!got.truncated, "{:?}", got.warnings);
    }

    /// A range across years asks each year it touches, cut to the range, newest first. A
    /// year the tabs do not name is not asked.
    #[test]
    fn a_range_asks_the_years_it_touches_cut_to_the_range() {
        let range = Window {
            from: jiff::civil::date(2025, 8, 22),
            to: jiff::civil::date(2026, 11, 20),
        };
        let page = listing(&[2026, 2025, 2024], &[]);
        assert_eq!(
            years_to_ask(&page, Some(range)),
            [
                Window {
                    from: jiff::civil::date(2026, 1, 1),
                    to: range.to,
                },
                Window {
                    from: range.from,
                    to: jiff::civil::date(2025, 12, 31),
                },
            ]
        );
        assert_eq!(
            years_to_ask(&listing(&[2026, 2018], &[]), None),
            [Window::year(2026).unwrap(), Window::year(2018).unwrap()],
            "the front door asks the years it lists and none between"
        );
    }

    /// The newest meeting's HTML agenda is read first, so a budget that runs out keeps
    /// every listed file and the newest meeting's attachments, and says it stopped.
    #[tokio::test]
    async fn a_walk_out_of_budget_has_the_newest_meetings_attachments() {
        let s = seed(&listing(&[2026], &[ROW_1762, ROW_1765]), FRONT_DOOR);
        let crawl = Fake::new([
            (year(2026).as_str(), listing(&[2026], &[ROW_1765, ROW_1762])),
            (html("11172026", 1765).as_str(), HTML_1765.to_string()),
            (html("11032026", 1762).as_str(), HTML_BARE.to_string()),
        ])
        .with_budget(2);

        let got = AgendaCenter.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(crawl.requests(), 2, "1762's HTML agenda was never fetched");
        assert_eq!(got.figures["attachments"], 3);
        assert!(got.addresses.contains(&file("Minutes/1762")));
        assert!(got.truncated, "{:?}", got.warnings);
    }

    /// The day in a file's name is decoration and the id is the document, measured at Lake
    /// Wales and Indian Shores. A link that names no file this keeps is not one.
    #[test]
    fn a_document_has_one_address_whatever_day_it_names() {
        let root = Url::parse(ROOT).unwrap();
        let of = |path: &str| File::of(&Url::parse(&file(path)).unwrap());
        let address = |path: &str| match of(path) {
            Some(File::Doc(doc)) => Some(doc.address(&root)),
            _ => None,
        };
        for linked in [
            "Agenda/_11172026-1765",
            "Agenda/_01012000-1765",
            "Agenda/1765",
        ] {
            assert_eq!(address(linked), Some(file("Agenda/1765")), "{linked}");
        }
        assert_eq!(
            address("Item/18398?fileID=23880&isAttachment=True"),
            Some(file("Item/18398?fileID=23880")),
            "a key the server ignores is not part of the address"
        );
        assert_eq!(of("Agenda/1765?css=true"), None, "a stylesheet");
        assert_eq!(of("Item/18398"), None, "no fileID answers 404");
        assert!(
            Url::parse(&format!("{ROOT}PreviousVersions/_11032026-1762"))
                .ok()
                .and_then(|u| File::of(&u))
                .is_none()
        );
        assert!(matches!(
            of("Agenda/_11172026-1765?html=true"),
            Some(File::Html(Html { held: Some(day), id: 1765, .. })) if day == jiff::civil::date(2026, 11, 17)
        ));
    }

    /// Recognised on the page the module serves, at its own path, and only when it lists
    /// something. The empty module seven of the eight box sources serve is not recognised,
    /// and a host's crawl delay is said up front.
    #[test]
    fn the_module_is_recognised_when_it_lists_meetings() {
        let robots = "User-agent: *\nDisallow: /admin\nCrawl-delay: 10\n";
        let s = Seed {
            robots: Robots::parse("centinel", robots.as_bytes()),
            ..seed(&listing(&[2026, 2025], &[ROW_1762]), FRONT_DOOR)
        };
        let r = AgendaCenter
            .recognise(&s)
            .expect("a populated Agenda Center");
        assert_eq!(r.keyed_on, Keyed::Product(PRODUCT));
        assert!(
            r.warnings.iter().any(|n| n.label == "pace"),
            "{:?}",
            r.warnings
        );

        // Seminole's module on 2026-11-21: the script and the search form, no category.
        let empty = format!(
            r#"<html><head>{SCRIPT}</head><body><div id="AgendaCenterContent" class="agendaCenterContent newCP moduleContentNew ">
<p>View current agendas and minutes for all boards and commissions.</p>
<div id="categoryList" style="display:none; " class="categoryList"><ol><li><label for="allCategories"><input type="checkbox" id="allCategories" checked>All Categories</label></li></ol></div>
</div></body></html>"#
        );
        assert!(
            AgendaCenter
                .recognise(&seed(&empty, "https://www.myseminole.com/AgendaCenter"))
                .is_none()
        );

        let page = listing(&[2026], &[ROW_1762]);
        assert!(
            AgendaCenter
                .recognise(&seed(
                    &page,
                    "https://www.lakewalesfl.gov/301/Agendas-Minutes"
                ))
                .is_none(),
            "the module lives at its own path"
        );
    }
}
