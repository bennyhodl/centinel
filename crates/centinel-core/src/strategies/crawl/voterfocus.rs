//! `voterfocus` — VoterFocus Campaign Finance: the campaign treasurer's reports a county
//! Supervisor of Elections publishes for local candidates, political committees and parties.
//!
//! Keyed on a **product**. Hillsborough, Pinellas, Pasco and Manatee publish through it
//! (`docs/FIELD-NOTES.md` entry 20), as do Hernando, Polk and Sarasota, all on one host:
//! `www.voterfocus.com/CampaignFinance/candidate_pr.php?c=<county>`. It is recognised by the
//! page it serves, so a county the notes never saw is collected the same way.
//!
//! ## Where the filers are
//!
//! A county's page lists one **reporting group** at a time: an election cycle, a municipal
//! election, or the committees or parties reporting through a date. The groups are the
//! options of a GET form, `candidate_pr.php?c=<county>&el=<group>`, and each answers every
//! filer in it on one page: Hillsborough's 27 groups list 2,264 filers, back to 2000. No
//! filer was listed in two groups.
//!
//! ## What each filer holds, and what decides it
//!
//! A listing links each filer's page, `op=cv&e=<group>&c=<county>&ca=<filer>`. Only `c` and
//! `ca` decide what it says: `e` is copied into the links it carries, and a report link
//! without the right `e` names no PDF. So the page is asked with its group. It holds the
//! filer's totals per report, the qualifying documents as PDFs (`pdf_<county>/doc_…pdf`,
//! among them the Form 1 or Form 6 a candidate files to qualify), and one link per report:
//!
//! ```text
//! candidate_pr.php?op=rp&e=27&c=hillsborough&ca=2621&sdc=1039&rellevel=4&dhc=13282&committee=N
//! ```
//!
//! `c`, `ca` and `dhc` decide the report's rows; another filer's `ca` answers an empty
//! report, status 200. The report page links the PDF as filed,
//! `pdf_hillsborough/e27c2621_Q2_dhc13282sdc1039.pdf`, and links it only when `e` and `sdc`
//! are in the query too. So a report is written with `op`, `e`, `c`, `ca`, `sdc` and `dhc`,
//! read from the link, and `rellevel` and `committee` are dropped. Acquisition takes the
//! PDFs off the filer and report pages as enclosures.
//!
//! Every transaction a filer reported is also one CSV,
//! `export.php?op=CFINANCE&cand_id=<filer>&dhc=0&county=<county>`, which is kept for a filer
//! with at least one report. It is served as `application/vnd.ms-excel`.
//!
//! ## Bounding a walk
//!
//! A filer's page costs a request, so every listing is read first, and the filers' pages in
//! the order the groups are listed, the current group first. A walk out of budget has every
//! filer's page and loses the reports of the oldest groups first. The seed bounds it: a
//! county's page covers every group, and `…&el=<group>` covers that group alone.
//!
//! ## Personal data
//!
//! A report names each contributor with a street address and an occupation, and a
//! candidate's page and the candidate CSV give a home address, an email and a telephone.
//! They are collected as the county publishes them, and the recognition says so.
//!
//! ## robots.txt
//!
//! `www.voterfocus.com/robots.txt` answered 503 "temporarily unavailable for maintenance" on
//! every request on 2026-11-22, while every page answered 200. An unreachable robots.txt is
//! the host policy's call, so nothing here decides it.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt;

use futures::future::BoxFuture;
use url::Url;

use super::{Addresses, Enumerated, Pass, Seed, Strategy, StrategyDef, Walk};
use crate::domain::Note;
use crate::strategies::{Keyed, Recognition};

pub struct VoterFocus;

inventory::submit! { StrategyDef { name: "voterfocus", it: &VoterFocus } }

/// The product, as a recognition names it.
const PRODUCT: &str = "VoterFocus Campaign Finance";

/// The script every listing, and nothing else, is served from.
const PAGE: &str = "candidate_pr.php";

/// The reporting-group menu. On every listing, and on no filer or report page.
const GROUPS: &str = "ElectionSelect";

/// Every request is at depth 0: a listing names filers, and a filer page names reports.
const MAX_DEPTH: usize = 0;

/// The figures a walk reports, in the order they are written.
const FIGURES: [&str; 4] = ["groups", "filers", "reports", "exports"];

impl Strategy for VoterFocus {
    fn name(&self) -> &'static str {
        "voterfocus"
    }

    fn recognise(&self, seed: &Seed) -> Option<Recognition> {
        let at = seed.final_url()?;
        let page = seed.text();
        let county = county_of(&at)?;
        if !listing_at(&at) || !page.contains(GROUPS) {
            return None;
        }
        let groups = groups_in(&page);
        if groups.is_empty() {
            return None;
        }
        let filers = filers_in(&page, &at);
        let mut r = Recognition::new(self.name(), Keyed::Product(PRODUCT))
            .seeing("county", county)
            .seeing(
                "groups",
                format!("{} reporting groups listed", groups.len()),
            )
            .seeing("filers", format!("{} on this page", filers.len()))
            .warning(
                "personal data",
                "reports name each contributor with a street address; candidate pages give \
                 a home address, email and telephone",
            );
        if let Some(group) = group_of(&at) {
            r = r.seeing("group", format!("this group only ({group})"));
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
            let county =
                county_of(&at).ok_or_else(|| anyhow::anyhow!("the seed names no county (`c=`)"))?;
            let root = at.join("./")?;
            let one_group = group_of(&at).is_some();

            let mut pass = Pass::new(crawl, "campaign finance", MAX_DEPTH);
            pass.note(Note::new("county", county.clone()));
            pass.push(Ask::Seed, 0);

            let mut seen: HashSet<u64> = HashSet::new();
            // Read once every listing is, in the order they were listed.
            let mut filers: VecDeque<Filer> = VecDeque::new();
            let mut tally: BTreeMap<&'static str, u64> = BTreeMap::new();
            loop {
                let Some((ask, _)) = pass.next_to_visit() else {
                    if pass.pending() == 0
                        && let Some(filer) = filers.pop_front()
                    {
                        pass.push(Ask::Filer(filer), 0);
                        continue;
                    }
                    break;
                };
                pass.visiting(ask.to_string());
                let (body, from) = match ask.url(&root, &county) {
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

                if let Ask::Filer(filer) = &ask {
                    let reports = reports_in(&text, &from, filer.ca);
                    if reports.is_empty() {
                        continue;
                    }
                    *tally.entry("exports").or_default() += 1;
                    pass.keep(filer.export(&root, &county), &seed.robots);
                    for report in reports {
                        *tally.entry("reports").or_default() += 1;
                        pass.keep(report.address(&root, &county), &seed.robots);
                    }
                    continue;
                }

                if !text.contains(GROUPS) {
                    pass.warn(format!("{ask}: the answer is not a VoterFocus listing"));
                    continue;
                }
                pass.read_surface();
                *tally.entry("groups").or_default() += 1;
                if matches!(ask, Ask::Seed) && !one_group {
                    let here = selected_in(&text);
                    for group in groups_in(&text) {
                        if Some(group) != here {
                            pass.push(Ask::Group(group), 0);
                        }
                    }
                }
                for filer in filers_in(&text, &from) {
                    if pass.full() {
                        break;
                    }
                    if !seen.insert(filer.ca) {
                        continue;
                    }
                    *tally.entry("filers").or_default() += 1;
                    pass.keep(filer.page(&root, &county), &seed.robots);
                    filers.push_back(filer);
                }
            }

            for key in FIGURES {
                pass.figure(key, tally.get(key).copied().unwrap_or(0));
            }
            Ok(pass.finish())
        })
    }

    /// A filer's page and a report's page each enclose PDFs, which acquisition takes.
    fn addresses_are(&self) -> Addresses {
        Addresses::Pages
    }
}

/// One request this walk makes.
#[derive(Clone, Debug)]
enum Ask {
    /// The listing the seed is, already in hand.
    Seed,
    /// Every filer in one reporting group.
    Group(u64),
    /// One filer's page, for its reports.
    Filer(Filer),
}

impl Ask {
    /// Where to ask, or `None` for the seed, which was fetched before the walk began.
    fn url(&self, root: &Url, county: &str) -> Option<String> {
        match self {
            Self::Seed => None,
            Self::Group(group) => Some(format!("{root}{PAGE}?c={county}&el={group}")),
            Self::Filer(filer) => Some(filer.page(root, county)),
        }
    }
}

/// How it reads in a warning, and what [`Pass`] dedups on.
impl fmt::Display for Ask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Seed => write!(f, "the listing in hand"),
            Self::Group(group) => write!(f, "reporting group {group}"),
            Self::Filer(filer) => write!(f, "filer {} in group {}", filer.ca, filer.group),
        }
    }
}

/// A candidate, committee or party, as a listing links it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Filer {
    /// The reporting group it is listed in, which its report links need.
    group: u64,
    ca: u64,
}

impl Filer {
    /// Its page, asked with its group so the report links carry it.
    fn page(&self, root: &Url, county: &str) -> String {
        format!(
            "{root}{PAGE}?op=cv&e={}&c={county}&ca={}",
            self.group, self.ca
        )
    }

    /// Every transaction it reported, as one CSV.
    fn export(&self, root: &Url, county: &str) -> String {
        format!(
            "{root}export.php?op=CFINANCE&cand_id={}&dhc=0&county={county}",
            self.ca
        )
    }
}

/// One campaign treasurer's report, by the keys its page needs to link its PDF.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Report {
    group: u64,
    ca: u64,
    sdc: u64,
    dhc: u64,
}

impl Report {
    fn address(&self, root: &Url, county: &str) -> String {
        format!(
            "{root}{PAGE}?op=rp&e={}&c={county}&ca={}&sdc={}&dhc={}",
            self.group, self.ca, self.sdc, self.dhc
        )
    }
}

/// The number a query names under `key`.
fn number(query: &BTreeMap<String, String>, key: &str) -> Option<u64> {
    query.get(key)?.parse().ok()
}

/// Every `<a href>` on the page that is this product's script, with its query, in order.
fn links_to_page(page: &str, at: &Url) -> Vec<BTreeMap<String, String>> {
    let scan = crate::html::Scan::new(page);
    let base = scan.base(at);
    scan.tags(&["a"])
        .into_iter()
        .filter_map(|tag| base.join(tag.attr("href")?))
        .filter(|u| u.host_str() == at.host_str() && u.path().ends_with(PAGE))
        .map(|u| u.query_pairs().into_owned().collect())
        .collect()
}

/// Every filer a listing links, in the order listed.
fn filers_in(page: &str, at: &Url) -> Vec<Filer> {
    links_to_page(page, at)
        .into_iter()
        .filter(|q| q.get("op").is_some_and(|op| op == "cv"))
        .filter_map(|q| {
            Some(Filer {
                group: number(&q, "e")?,
                ca: number(&q, "ca")?,
            })
        })
        .collect()
}

/// Every report a filer's page links for that filer, in the order listed.
fn reports_in(page: &str, at: &Url, ca: u64) -> Vec<Report> {
    links_to_page(page, at)
        .into_iter()
        .filter(|q| q.get("op").is_some_and(|op| op == "rp"))
        .filter_map(|q| {
            Some(Report {
                group: number(&q, "e")?,
                ca: number(&q, "ca").filter(|n| *n == ca)?,
                sdc: number(&q, "sdc")?,
                dhc: number(&q, "dhc")?,
            })
        })
        .collect()
}

/// The `<option>`s of the reporting-group menu, in the order the county lists them.
fn groups_in(page: &str) -> Vec<u64> {
    options_in(page)
        .into_iter()
        .map(|(group, _)| group)
        .collect()
}

/// The group the listing answered, which its menu marks `selected`.
fn selected_in(page: &str) -> Option<u64> {
    options_in(page)
        .into_iter()
        .find(|(_, selected)| *selected)
        .map(|(group, _)| group)
}

/// Each option of the menu, and whether it is the one selected.
fn options_in(page: &str) -> Vec<(u64, bool)> {
    let Some(start) = page.find(GROUPS) else {
        return Vec::new();
    };
    let menu = &page[start..];
    let menu = &menu[..menu.find("</select>").unwrap_or(menu.len())];
    crate::html::Scan::new(menu)
        .tags(&["option"])
        .into_iter()
        .filter_map(|tag| {
            let group = tag.attr("value")?.trim().parse().ok()?;
            Some((group, tag.raw.to_ascii_lowercase().contains("selected")))
        })
        .collect()
}

/// The county the address asks for: `c=hillsborough`. Written back into every address the
/// walk builds, so only a plain name is taken.
fn county_of(at: &Url) -> Option<String> {
    at.query_pairs()
        .find(|(k, _)| k == "c")
        .map(|(_, v)| v.into_owned())
        .filter(|c| {
            !c.is_empty()
                && c.chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        })
}

/// The one group a seed asks for: `el=24`.
fn group_of(at: &Url) -> Option<u64> {
    at.query_pairs()
        .find(|(k, _)| k == "el")
        .and_then(|(_, v)| v.parse().ok())
}

/// Whether an address is a listing: the script, asked for a county and no filer.
fn listing_at(at: &Url) -> bool {
    at.path().ends_with(&format!("/CampaignFinance/{PAGE}"))
        && !at.query_pairs().any(|(k, _)| k == "op" || k == "ca")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::crawl::tests::{Fake, seed};

    const ROOT: &str = "https://www.voterfocus.com/CampaignFinance/";
    const FRONT_DOOR: &str =
        "https://www.voterfocus.com/CampaignFinance/candidate_pr.php?c=hillsborough";

    /// Hillsborough's menu as served on 2026-11-22, cut to four of its 27 groups.
    fn menu(selected: u64) -> String {
        let option = |value: u64, name: &str| {
            let mark = if value == selected { " selected" } else { "" };
            format!("<option value='{value}'{mark}>{name}</option>")
        };
        format!(
            r#"<form action="/CampaignFinance/candidate_pr.php" method="GET">
<label for="ElectionSelect">Current reporting group (Election/Committees):</label>
<select name="el" id="ElectionSelect" style="width: 80%">{}{}{}{}</select></form>"#,
            option(3, "Political Committee Reports Through (12/31/2028)"),
            option(27, "2026 Candidate Election Cycle (11/3/2026)"),
            option(24, "2024 Candidate Election Cycle (11/5/2024)"),
            option(5, "General Election of 11/7/2000 (11/7/2001)"),
        )
    }

    /// A listing row, as Hillsborough serves it, cut to its link.
    fn row(group: u64, ca: u64, committee: char) -> String {
        format!(
            r#"<div class="col-xs-12 detailrow candidate clickme" role="row">
    <a class="rowlink" href="/CampaignFinance/candidate_pr.php?op=cv&e={group}&c=hillsborough&ca={ca}&rellevel=4&committee={committee}">
        <div class="col-xs-7 no-gutter bold cf_indent" role="gridcell">Filer {ca}</div></a></div>"#
        )
    }

    fn listing(selected: u64, rows: &[String]) -> String {
        format!(
            "<html><head><title>Candidate Reports</title></head><body>{}{}</body></html>",
            menu(selected),
            rows.join("")
        )
    }

    /// Filer 2621's page with its group, cut to two of its 21 reports, one qualifying
    /// document and the export link.
    const FILER_2621: &str = r#"<html class="CandidateReports CandidateInfo"><body>
<a href="export.php?op=CFINANCE&cand_id=2621&dhc=0&county=hillsborough">Export All</a>
<a href="/CampaignFinance/candidate_pr.php?op=rp&amp;e=27&amp;c=hillsborough&amp;ca=2621&amp;sdc=1039&amp;rellevel=4&amp;dhc=13282&amp;committee=">Q2<br>(4/1/2025 - 6/30/2025) </a>
<a href="/CampaignFinance/candidate_pr.php?op=rp&amp;e=27&amp;c=hillsborough&amp;ca=2621&amp;sdc=1040&amp;rellevel=4&amp;dhc=13376&amp;committee=">Q3<br>(7/1/2025 - 9/30/2025) </a>
<a href="?op=rp_all&e=1&c=hillsborough&ca=2621&cname=Gwen Myers&coffice=County Commissioner, Dist. 3">List Contributions and Expenditures by Year</a>
<a href="./pdf_hillsborough/doc_2621_20260611111456_Financial_Disclosure.pdf">Financial_Disclosure</a>
</body></html>"#;

    /// A filer that filed no report: the page lists qualifying documents only.
    const FILER_2709: &str = r#"<html class="CandidateReports CandidateInfo"><body>
<a href="export.php?op=CFINANCE&cand_id=2709&dhc=0&county=hillsborough">Export All</a>
<a href="./pdf_hillsborough/doc_2709_20260612090000_Candidate_Oath.pdf">Candidate_Oath</a>
</body></html>"#;

    /// Committee 2920's page, in the committees' group.
    const FILER_2920: &str = r#"<html class="CandidateReports CandidateInfo"><body>
<a href="/CampaignFinance/candidate_pr.php?op=rp&amp;e=3&amp;c=hillsborough&amp;ca=2920&amp;sdc=1083&amp;rellevel=4&amp;dhc=14017&amp;committee=Y">G6<br>(10/17/2026 - 10/29/2026) </a>
</body></html>"#;

    fn group(el: u64) -> String {
        format!("{ROOT}candidate_pr.php?c=hillsborough&el={el}")
    }

    fn page(el: u64, ca: u64) -> String {
        format!("{ROOT}candidate_pr.php?op=cv&e={el}&c=hillsborough&ca={ca}")
    }

    fn report(el: u64, ca: u64, sdc: u64, dhc: u64) -> String {
        format!("{ROOT}candidate_pr.php?op=rp&e={el}&c=hillsborough&ca={ca}&sdc={sdc}&dhc={dhc}")
    }

    fn export(ca: u64) -> String {
        format!("{ROOT}export.php?op=CFINANCE&cand_id={ca}&dhc=0&county=hillsborough")
    }

    /// The front door lists every group in its menu, and each is asked once, the group in
    /// hand not again. Every filer's page is kept as it is listed; the filers' pages are read
    /// after every listing, current group first, for their reports and their CSV. A filer
    /// with no report has no CSV.
    #[tokio::test]
    async fn the_front_door_walks_every_group_then_each_filer() {
        let s = seed(
            &listing(27, &[row(27, 2621, 'N'), row(27, 2709, 'N')]),
            FRONT_DOOR,
        );
        let crawl = Fake::new([
            (group(3).as_str(), listing(3, &[row(3, 2920, 'Y')])),
            (group(24).as_str(), listing(24, &[])),
            (group(5).as_str(), listing(5, &[])),
            (page(27, 2621).as_str(), FILER_2621.to_string()),
            (page(27, 2709).as_str(), FILER_2709.to_string()),
            (page(3, 2920).as_str(), FILER_2920.to_string()),
        ]);

        let got = VoterFocus.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                page(27, 2621),
                page(27, 2709),
                page(3, 2920),
                export(2621),
                report(27, 2621, 1039, 13282),
                report(27, 2621, 1040, 13376),
                export(2920),
                report(3, 2920, 1083, 14017),
            ]
        );
        assert_eq!(
            crawl.requests(),
            6,
            "three other groups and three filers; the front door was in hand"
        );
        assert_eq!(got.figures["groups"], 4);
        assert_eq!(got.figures["filers"], 3);
        assert_eq!(got.figures["reports"], 3);
        assert_eq!(got.figures["exports"], 2);
        assert!(
            !got.truncated && !got.discovery_failed && got.warnings.is_empty(),
            "{:?}",
            got.warnings
        );
    }

    /// A seed that names a group is that group: no other listing is asked.
    #[tokio::test]
    async fn a_seed_naming_a_group_walks_that_group_alone() {
        let at = format!("{FRONT_DOOR}&el=3");
        let s = seed(&listing(3, &[row(3, 2920, 'Y')]), &at);
        let crawl = Fake::new([(page(3, 2920).as_str(), FILER_2920.to_string())]);

        let got = VoterFocus.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(crawl.requests(), 1, "only the filer's page");
        assert_eq!(
            got.addresses,
            [page(3, 2920), export(2920), report(3, 2920, 1083, 14017)]
        );
    }

    /// Out of requests, the walk has every listed filer's page and the reports of the
    /// filers read first, and says it stopped.
    #[tokio::test]
    async fn a_walk_out_of_budget_keeps_every_filer_page() {
        let at = format!("{FRONT_DOOR}&el=27");
        let s = seed(&listing(27, &[row(27, 2621, 'N'), row(27, 2709, 'N')]), &at);
        let crawl = Fake::new([
            (page(27, 2621).as_str(), FILER_2621.to_string()),
            (page(27, 2709).as_str(), FILER_2709.to_string()),
        ])
        .with_budget(1);

        let got = VoterFocus.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(crawl.requests(), 1);
        assert!(got.addresses.contains(&page(27, 2709)));
        assert!(got.addresses.contains(&report(27, 2621, 1040, 13376)));
        assert!(got.truncated, "{:?}", got.warnings);
    }

    /// A report is written with the keys its page needs to link the filed PDF — measured:
    /// without `e` or `sdc` the page names no PDF — and without the two it does not need. A
    /// link to another filer's report is not this filer's.
    #[test]
    fn a_report_keeps_the_keys_its_pdf_needs() {
        let at = Url::parse(&page(27, 2621)).unwrap();
        let root = Url::parse(ROOT).unwrap();
        let got: Vec<String> = reports_in(FILER_2621, &at, 2621)
            .iter()
            .map(|r| r.address(&root, "hillsborough"))
            .collect();
        assert_eq!(
            got,
            [report(27, 2621, 1039, 13282), report(27, 2621, 1040, 13376)]
        );
        assert!(reports_in(FILER_2621, &at, 2622).is_empty());
    }

    /// Recognised on a county's listing, by its menu. Not on a filer's page, which has no
    /// menu, and not on a page that names no county.
    #[test]
    fn a_county_listing_is_recognised() {
        let s = seed(&listing(27, &[row(27, 2621, 'N')]), FRONT_DOOR);
        let r = VoterFocus.recognise(&s).expect("a county's listing");
        assert_eq!(r.keyed_on, Keyed::Product(PRODUCT));
        assert!(
            r.warnings.iter().any(|n| n.label == "personal data"),
            "{:?}",
            r.warnings
        );

        assert!(
            VoterFocus
                .recognise(&seed(FILER_2621, &page(27, 2621)))
                .is_none()
        );
        assert!(
            VoterFocus
                .recognise(&seed(
                    &listing(27, &[]),
                    "https://www.voterfocus.com/CampaignFinance/candidate_pr.php"
                ))
                .is_none(),
            "no county"
        );
    }
}
