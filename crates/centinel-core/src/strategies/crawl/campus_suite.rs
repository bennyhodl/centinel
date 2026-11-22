//! `campus-suite` — a Campus Suite site's pages, and every document its file lists name.
//!
//! Keyed on a **product**. Campus Suite (sold now as SchoolStatus Sites) builds the websites
//! of most community development districts in Hillsborough County: of 24 districts sampled
//! on 2026-11-22, 14 run it, managed by Rizzetta, Inframark and Breeze alike
//! (`docs/FIELD-NOTES.md` entry 13). Every page it serves is assembled by its own script,
//! `Campussuite.Widgets.run({…})`, which is what recognition keys on.
//!
//! ## Why a site walk is not enough
//!
//! A district publishes its agendas, minutes and budgets through the product's file
//! manager, at `/files/documents/<folder>/<name>.pdf` on its own host, and lists them with a
//! `FileList` widget: every folder expanded inline, every file a plain anchor, oldest
//! folder first. `/documents` lists 39 to 262 files at the 14 districts sampled, and
//! `/meetings` most of them again. No site serves a sitemap or a robots.txt that declares
//! one, so `links` walks them — and the enclosure scan keeps at most
//! [`crate::enclosure::MAX_PER_PAGE`] documents from a page, in page order. At South Fork
//! III that is the first 25 of 102: the 2021 agendas, and none since.
//!
//! A page that lists a hundred documents is a listing, and a listing's documents are
//! addresses. So this walks the site as `links` does and keeps every document the pages
//! link under `/files/` on the site's host, wherever on the page the link is.
//!
//! ## Newest first
//!
//! A file list says when each file was uploaded — `12.25 MB, uploaded October 8, 2026
//! 10:26 AM` inside its anchor — and the documents are kept newest upload first, so a
//! collect cut short holds the latest meetings. A document no list dates (an inline link
//! in a page's text) keeps its place after every dated one. Files migrated in bulk share
//! the day they were moved, which still sorts them behind everything uploaded since.
//!
//! ## What it does not walk
//!
//! **The calendar.** `/calendar` answers with the month's page, which links the month
//! before and the month after, forever, and an event page per meeting. Each event links
//! its agenda as an attachment on `campussuite-storage.s3.amazonaws.com` — and at seven
//! districts every one of those 27 attachments was a copy of a file the site serves under
//! `/files/`. So the calendar is not followed, the storage copies are not kept, and each
//! walk counts any attachment whose name matches no kept document, and warns, rather than
//! assume the copy is always there.
//!
//! ## Cost
//!
//! One request per page. South Fork III and K-Bar Ranch II are each 17 pages after the
//! seed, holding 109 and 189 documents.

use std::collections::{HashMap, HashSet};

use futures::future::BoxFuture;
use jiff::civil::Date;
use url::Url;

use super::links::pages_in;
use super::scope::Scope;
use super::{Addresses, Enumerated, Pass, Seed, Strategy, StrategyDef, Walk};
use crate::content::ContentKind;
use crate::domain::Note;
use crate::strategies::{Keyed, Recognition};

pub struct CampusSuite;

inventory::submit! { StrategyDef { name: "campus-suite", it: &CampusSuite } }

/// The product, as a recognition names it.
const PRODUCT: &str = "Campus Suite";

/// The call that assembles every page the product serves, one per widget.
const MARKER: &str = "Campussuite.Widgets.run(";

/// Where the file manager serves a site's documents, on the site's own host.
const FILES: &str = "/files/";

/// The calendar module, which is not walked. See the module docs.
const CALENDAR: &str = "/calendar";

/// Where the calendar keeps its attachments.
const STORAGE: &str = "campussuite-storage.s3.amazonaws.com";

/// Clicks from the seed, as `links` counts them. Every page of the sites measured is one
/// click from the front door's menu; the calendar, the one module that links onward
/// forever, is not followed, and the request budget bounds the rest.
const MAX_DEPTH: usize = 4;

/// Month names as a file list writes them.
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

impl Strategy for CampusSuite {
    fn name(&self) -> &'static str {
        "campus-suite"
    }

    fn recognise(&self, seed: &Seed) -> Option<Recognition> {
        let at = seed.final_url()?;
        let page = seed.text();
        if !page.contains(MARKER) {
            return None;
        }
        let linked = read(&page, &at).documents.len();
        let mut r = Recognition::new(self.name(), Keyed::Product(PRODUCT))
            .seeing("widgets", "the page is assembled by Campussuite.Widgets")
            .seeing(
                "files",
                format!("{linked} document(s) under {FILES} linked on this page"),
            );
        if let Some(delay) = seed.robots.crawl_delay() {
            r = r.warning(
                "pace",
                format!(
                    "robots.txt asks {} s between requests; a walk is one request per page",
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
            let mut start = seed
                .final_url()
                .ok_or_else(|| anyhow::anyhow!("the seed does not record where it was served"))?;
            start.set_fragment(None);
            let scope = Scope::of(&start);
            let robots = &seed.robots;

            let mut pass = Pass::new(crawl, "site", MAX_DEPTH);
            pass.note(Note::new("scope", scope.to_string()));
            pass.keep(start.to_string(), robots);
            pass.push(start.to_string(), 0);

            // Every document in the order first linked, each with the latest upload day any
            // list gave it; and the names of the calendar's storage copies.
            let mut documents: Vec<Document> = Vec::new();
            let mut held: HashMap<String, usize> = HashMap::new();
            let mut stored: HashSet<String> = HashSet::new();
            while let Some((page, depth)) = pass.next_to_visit() {
                pass.visiting(format!("page {page}"));

                // The seed's bytes are what recognition read; empty means it refused.
                let fetched = match depth {
                    0 if seed.page.bytes.is_empty() => {
                        pass.warn(format!("{page}: the seed could not be read"));
                        continue;
                    }
                    0 => seed.page.clone(),
                    _ => match pass.walk().get(&page).await {
                        Ok(f) => f,
                        Err(refusal) => {
                            pass.refused(&page, &refusal);
                            continue;
                        }
                    },
                };
                if ContentKind::classify(&fetched.meta, &fetched.bytes) != ContentKind::Html {
                    continue;
                }
                pass.read_surface();

                let served = fetched
                    .meta
                    .get("final_url")
                    .and_then(|u| Url::parse(u).ok())
                    .unwrap_or_else(|| start.clone());
                let html = String::from_utf8_lossy(&fetched.bytes);

                let found = read(&html, &served);
                stored.extend(found.stored);
                for doc in found.documents {
                    match held.get(&doc.address).copied() {
                        Some(i) => documents[i].uploaded = documents[i].uploaded.max(doc.uploaded),
                        None => {
                            held.insert(doc.address.clone(), documents.len());
                            documents.push(doc);
                        }
                    }
                }

                for target in pages_in(&html, &served) {
                    if !scope.contains(&target) || in_calendar(&target) {
                        continue;
                    }
                    if pass.full() {
                        break;
                    }
                    let address = target.to_string();
                    if robots.allowed(&address) {
                        pass.push(address.clone(), depth + 1);
                    }
                    pass.keep(address, robots);
                }
            }

            let names: HashSet<String> = documents
                .iter()
                .filter_map(|d| Url::parse(&d.address).ok().and_then(|u| name_of(&u)))
                .collect();
            let only_stored = stored.iter().filter(|n| !names.contains(*n)).count() as u64;

            // Newest upload first; undated after, in the order linked (the sort is stable).
            documents.sort_by_key(|d| std::cmp::Reverse(d.uploaded));
            let total = documents.len() as u64;
            for doc in documents {
                if pass.full() {
                    break;
                }
                pass.keep(doc.address, robots);
            }

            let pages = pass.visits();
            pass.figure("pages_visited", pages);
            pass.figure("documents", total);
            pass.figure("storage_copies", stored.len() as u64);
            pass.figure("storage_only", only_stored);
            if only_stored > 0 {
                pass.warn(format!(
                    "{} calendar attachment(s) on {STORAGE} match no file the site lists, and \
                     were not kept",
                    crate::render::count(only_stored)
                ));
            }
            Ok(pass.finish())
        })
    }

    /// Pages and documents both, and neither needs an enclosure scan: the documents a page
    /// links under `/files/` were read off it here, all of them rather than the first 25.
    fn addresses_are(&self) -> Addresses {
        Addresses::Documents
    }

    fn first_address_is_the_seed(&self) -> bool {
        true
    }
}

/// One document a page links, and the day a file list says it was uploaded.
#[derive(Debug)]
struct Document {
    address: String,
    uploaded: Option<Date>,
}

/// What one page holds: its documents on the site's host, and the file names of the
/// calendar attachments it links on the storage host.
#[derive(Debug, Default)]
struct Found {
    documents: Vec<Document>,
    stored: Vec<String>,
}

/// Every document a page links under `/files/` on its own host, and the storage copies.
///
/// Read from the page's anchors. A file list's anchor wraps the file's title and its
/// `uploaded …` line, so the day comes from the same link as the address.
fn read(html: &str, page: &Url) -> Found {
    let scan = crate::html::Scan::new(html);
    let base = scan.base(page);
    let mut found = Found::default();
    for link in scan.links() {
        let Some(mut url) = link.tag.attr("href").and_then(|href| base.join(href)) else {
            continue;
        };
        url.set_fragment(None);
        if url.host_str() == Some(STORAGE) {
            if url.path().contains("/file/")
                && let Some(name) = name_of(&url)
            {
                found.stored.push(name);
            }
            continue;
        }
        if url.host_str() != page.host_str()
            || !url.path().starts_with(FILES)
            || !crate::enclosure::is_document(url.path())
        {
            continue;
        }
        found.documents.push(Document {
            address: url.to_string(),
            uploaded: uploaded(&crate::html::text(link.inner)),
        });
    }
    found
}

/// `… 12.25 MB, uploaded October 8, 2026 10:26 AM` is October 8, 2026.
fn uploaded(text: &str) -> Option<Date> {
    let (_, rest) = text.split_once("uploaded ")?;
    let mut words = rest.split_whitespace();
    let month = words.next()?;
    let month = MONTHS.iter().position(|m| *m == month)? + 1;
    let day = words.next()?.trim_end_matches(',').parse().ok()?;
    let year = words.next()?.parse().ok()?;
    Date::new(year, i8::try_from(month).ok()?, day).ok()
}

/// A file's name as its address writes it, lowercased: how a storage copy is matched to
/// the file the site serves.
fn name_of(url: &Url) -> Option<String> {
    let name = url.path_segments()?.next_back()?;
    (!name.is_empty()).then(|| name.to_lowercase())
}

/// Whether an address is the calendar module or under it.
fn in_calendar(url: &Url) -> bool {
    let path = url.path();
    path == CALENDAR || path.starts_with("/calendar/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::crawl::tests::{Fake, seed};

    const SITE: &str = "https://www.southfork3cdd.org";

    /// The widget call every page ends each widget with.
    const WIDGET: &str = r#"<script type="text/javascript">
        Campussuite.Widgets.run({
    "styles": [],
    "scripts": [],
    "plugin": "FileList",
    "pluginId": "cms-widget-d22d068c-fa23-48bf-8a68-491372416aac",
    "pluginOptions": {}
});
    </script>"#;

    /// South Fork III's front door on 2026-11-22, cut to its menu, a news post, an inline
    /// document, the calendar widget's meetings and an off-host link.
    fn front_door() -> String {
        format!(
            r#"<html><body><div data-cms-widget="widgets/MegaMenu/MegaMenu">
<a class="cs-mmenu-li-item-link" href="/" aria-expanded="false">Home</a>
<a class="cs-mmenu-li-item-link" href="/calendar" aria-expanded="false">Events</a>
<a class="cs-mmenu-li-item-link" href="/documents" aria-expanded="false">Documents</a>
<a href="/about-the-cdd">About the CDD</a>
<a href="/meetings">Meetings</a></div>
<a href="https://www.southfork3cdd.org/news/1823957/pond-bank-info">Pond bank Info</a>
<a href="https://www.southfork3cdd.org/files/documents/Community%20Documents/stormwater_systems.pdf" target="_blank">Storm Water Pond Maintenance</a>
<a href="https://www.southfork3cdd.org/calendar/9300669/cdd-board-meeting-dec-2026">CDD Board Meeting - Dec. 2026</a>
<a class="ada-footer__logo" href="https://www.schoolstatus.com/products/sites-and-apps" target="_blank">SchoolStatus Sites</a>
{WIDGET}</body></html>"#
        )
    }

    /// One file-list entry, as the widget writes it.
    fn entry(href: &str, title: &str, modified: &str) -> String {
        format!(
            r#"<div class="cs-file__li cs-file__li--file">
							<a href="{href}" target="_self">
								<div class="cs-file__li-inner">
									<em class="cs-file__li-icon fa fa-file-pdf-o"></em>
									<div class="cs-file__li-title">{title}</div>
																			<div class="cs-file__li-modified">{modified}</div>
																	</div>
															</a>
						</div>"#
        )
    }

    const OCT: &str = "//www.southfork3cdd.org/files/documents/Board Documents/Agendas/2026-2027/2026-10-15 SOUTH FORK 111 CDD AGENDA PACKAGE.pdf";
    const NOV: &str = "//www.southfork3cdd.org/files/documents/Board Documents/Agendas/2026-2027/2026-11-19 SOUTH FORK 111 CDD AGENDA PACKAGE.pdf";
    const OLD: &str = "//www.southfork3cdd.org/files/documents/Board Documents/Agendas/2021-2022/2021-10-07 SFIII 501 Agenda.pdf";

    /// `/meetings`: the calendar widget's last meeting, with its storage copy, then the
    /// file list oldest folder first.
    fn meetings() -> String {
        format!(
            r#"<html><body><div data-cms-widget="widgets/ModuleCalendarList/ModuleCalendarList">
<h3 class="cs-li-default-title"><a href="https://www.southfork3cdd.org/calendar/9300668/cdd-board-meeting-nov-2026">CDD Board Meeting - Nov. 2026</a></h3>
<p class="meta-event-attachment">
                <a href="https://campussuite-storage.s3.amazonaws.com/prod/1559147/5140a5aa-5025-11ed-b288-0e84392e8069/3169153/c8d4ea68-df64-11f1-baa8-0affc7f3df5d/file/2026-11-19 SOUTH FORK 111 CDD AGENDA PACKAGE.pdf">2026-11-19 SOUTH FORK 111 CDD AGENDA PACKAGE.pdf</a>
            </p></div>
<div data-cms-widget="widgets/FileList/FileList"><div class="cs-files">
<div class="cs-file__li cs-file__li--folder"><div class="cs-file__li-title">2021-2022</div></div>
{}
<div class="cs-file__li cs-file__li--folder"><div class="cs-file__li-title">2026-2027</div></div>
{}
{}
</div></div>{WIDGET}</body></html>"#,
            entry(
                OLD,
                "2021-10-07 SFIII 501 Agenda.pdf",
                "192.49 KB, uploaded October 21, 2022 12:08 PM (updated October 25, 2022 9:38 AM) "
            ),
            entry(
                OCT,
                "2026-10-15 SOUTH FORK 111 CDD AGENDA PACKAGE.pdf",
                "12.25 MB, uploaded October 8, 2026 10:26 AM"
            ),
            entry(
                NOV,
                "2026-11-19 SOUTH FORK 111 CDD AGENDA PACKAGE.pdf",
                "14.69 MB, uploaded November 13, 2026 8:18 AM"
            ),
        )
    }

    /// `/documents`: the same file list, its November entry already percent-encoded, and
    /// a budget filed elsewhere.
    fn documents() -> String {
        format!(
            r#"<html><body><div data-cms-widget="widgets/FileList/FileList">{}{}</div>{WIDGET}</body></html>"#,
            entry(
                "https://www.southfork3cdd.org/files/documents/Board%20Documents/Agendas/2026-2027/2026-11-19%20SOUTH%20FORK%20111%20CDD%20AGENDA%20PACKAGE.pdf",
                "2026-11-19 SOUTH FORK 111 CDD AGENDA PACKAGE.pdf",
                "14.69 MB, uploaded November 13, 2026 8:18 AM"
            ),
            entry(
                "//www.southfork3cdd.org/files/documents/Financial Documents/FY2027/FY 2027 Adopted Budget.pdf",
                "FY 2027 Adopted Budget.pdf",
                "301.12 KB, uploaded August 22, 2026 2:41 PM"
            ),
        )
    }

    fn page(body: &str) -> String {
        format!("<html><body>{body}{WIDGET}</body></html>")
    }

    fn at(path: &str) -> String {
        format!("{SITE}{path}")
    }

    /// The site's pages are walked and kept, the calendar is not, and every document under
    /// `/files/` is kept once, newest upload first, the undated inline link last. The
    /// storage copy of the November agenda is not kept, and is counted.
    #[tokio::test]
    async fn every_page_and_every_listed_document_newest_first() {
        let s = seed(&front_door(), &at("/"));
        let crawl = Fake::new([
            (at("/documents").as_str(), documents()),
            (
                at("/about-the-cdd").as_str(),
                page("<p>Established by ordinance.</p>"),
            ),
            (at("/meetings").as_str(), meetings()),
            (
                at("/news/1823957/pond-bank-info").as_str(),
                page("<p>Pond banks.</p>"),
            ),
        ]);

        let got = CampusSuite.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                at("/"),
                at("/documents"),
                at("/about-the-cdd"),
                at("/meetings"),
                at("/news/1823957/pond-bank-info"),
                at(
                    "/files/documents/Board%20Documents/Agendas/2026-2027/2026-11-19%20SOUTH%20FORK%20111%20CDD%20AGENDA%20PACKAGE.pdf"
                ),
                at(
                    "/files/documents/Board%20Documents/Agendas/2026-2027/2026-10-15%20SOUTH%20FORK%20111%20CDD%20AGENDA%20PACKAGE.pdf"
                ),
                at(
                    "/files/documents/Financial%20Documents/FY2027/FY%202027%20Adopted%20Budget.pdf"
                ),
                at(
                    "/files/documents/Board%20Documents/Agendas/2021-2022/2021-10-07%20SFIII%20501%20Agenda.pdf"
                ),
                at("/files/documents/Community%20Documents/stormwater_systems.pdf"),
            ]
        );
        assert_eq!(
            crawl.requests(),
            4,
            "each page once; the seed was in hand and the calendar is never asked"
        );
        assert_eq!(got.figures["documents"], 5);
        assert_eq!(got.figures["storage_copies"], 1);
        assert_eq!(got.figures["storage_only"], 0);
        assert!(
            !got.truncated && !got.discovery_failed && got.warnings.is_empty(),
            "{:?}",
            got.warnings
        );
    }

    /// A calendar attachment that names no file the site lists is the one the walk would
    /// lose, so it says so.
    #[tokio::test]
    async fn a_storage_attachment_with_no_file_on_the_site_is_reported() {
        let only = r#"<a href="https://campussuite-storage.s3.amazonaws.com/prod/1/2/3/4/file/2026-12-17 Special Meeting Agenda.pdf">2026-12-17 Special Meeting Agenda.pdf</a>"#;
        let s = seed(&page(only), &at("/"));
        let crawl = Fake::new([]);

        let got = CampusSuite.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(got.addresses, [at("/")]);
        assert_eq!(got.figures["storage_only"], 1);
        assert!(
            got.warnings.iter().any(|w| w.contains("match no file")),
            "{:?}",
            got.warnings
        );
    }

    /// Recognised by the product's own widget script, and nothing else: a WordPress
    /// district's front door, which links PDFs too, is not.
    #[test]
    fn recognised_by_the_widget_script() {
        let r = CampusSuite
            .recognise(&seed(&front_door(), &at("/")))
            .expect("a Campus Suite page");
        assert_eq!(r.keyed_on, Keyed::Product(PRODUCT));

        let wordpress = r#"<html><head><link rel="stylesheet" href="https://wynnmereeastcdd.com/wp-content/themes/cdd/style.css"></head>
<body><a href="http://wynnmereeastcdd.com/wp-content/uploads/2026/10/2026-10-26-Wynnmere-East-CDD-Final-Agenda.pdf">Agenda</a></body></html>"#;
        assert!(
            CampusSuite
                .recognise(&seed(wordpress, "https://wynnmereeastcdd.com/"))
                .is_none()
        );
    }

    #[test]
    fn a_file_list_line_dates_its_upload() {
        assert_eq!(
            uploaded("2026-10-15 AGENDA PACKAGE.pdf 12.25 MB, uploaded October 8, 2026 10:26 AM"),
            Some(jiff::civil::date(2026, 10, 8))
        );
        assert_eq!(
            uploaded(
                "192.49 KB, uploaded October 21, 2022 12:08 PM (updated October 25, 2022 9:38 AM)"
            ),
            Some(jiff::civil::date(2022, 10, 21)),
            "the upload, not the later update"
        );
        assert_eq!(uploaded("Storm Water Pond Maintenance"), None);
    }
}
