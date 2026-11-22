//! `wmis` — the Southwest Florida Water Management District's environmental resource
//! permits (ERP), and every document filed on each, from the District's Water Management
//! Information System.
//!
//! Keyed on a **product**, at one sighting: WMIS is the District's own system and serves no
//! other agency (`docs/FIELD-NOTES.md` entry 18). Its half that finds the permits is not
//! District-specific: South Florida WMD's map service names each permit's record page the
//! same way, in a field of every feature.
//!
//! ## Which permits
//!
//! WMIS's own search is an ASP.NET WebForms postback. The District's map service is not:
//! its ERP layer (`…/arcgis10/rest/services/Permits/Permits/MapServer/3`) answers a GET
//! query, and every feature carries `ERP_EXT_URL`, the application's WMIS page:
//!
//! ```text
//! https://www38.swfwmd.state.fl.us/Erp/ERP/Entry/ERP.aspx?id=1002708
//! ```
//!
//! The seed is a query on that layer, and its `where` and `geometry` are the bound: a date
//! on `APPLICATION_RECEIVED_DT`, a county's envelope, a permit type. The walk keeps every
//! other parameter the seed sets and owns the ones that make it a walk: the fields it reads,
//! the order — **newest first**, by the day the application was received — and the paging.
//! A page holds at most 1,000 features and says `exceededTransferLimit` while more remain,
//! so no answer is cut short in silence. An application is drawn as up to two features (its
//! project activity area and its total land area), and is opened once.
//!
//! ## Which documents
//!
//! The application's WMIS page lists every document on file in one grid, unpaged — 88 on
//! one application seen. Each row names its document by an opaque token, read by a script:
//!
//! ```text
//! onclick="javascript:ViewDocument([{"Key":"BigSix","Value":"p2avH3Qj…"},…]);"
//! ```
//!
//! and the script opens `Erp/Common/Controls/ExportDocument.aspx?OpaqueId=TOKEN`, which
//! answers a 302 to a file whose name is new on every request
//! (`Erp/Export/ViewDoc/0ks53xpt.pdf`, then `…/miq2q4wa.pdf`). The token is the stable
//! address: the same on every fetch of the page, the same bytes on every request, from
//! either WMIS host, and through the water use permits' path (`/Support/…`) as well. A token
//! altered by one character answers 500. So a document is written as `ExportDocument.aspx`
//! with that one key, on the host the permit page was read from. The page itself is not
//! kept: about 650 KB, two thirds of it `__VIEWSTATE`, and what it says is in the
//! application and the permit, which are among its documents.
//!
//! Water use permits carry their documents behind a postback (the record page's Documents
//! tab), so this walk does not collect them. That wants a [`Walk`] that can post a form,
//! which it cannot.
//!
//! ## Order, and what a short walk loses
//!
//! Every page of the query is read first, then the applications are opened one at a time,
//! newest first. A walk that runs out of requests holds every document of the newest
//! applications it opened, and says `truncated`. The request budget (`--max-sitemaps`)
//! applies inside the seed's bound.
//!
//! ## Hours
//!
//! The District says the map service is available 6 A.M. to 10 P.M. daily; the
//! recognition says so, because a run scheduled outside those hours fails at its first
//! request.

use std::collections::{HashSet, VecDeque};
use std::fmt;

use futures::future::BoxFuture;
use serde::Deserialize;
use url::Url;

use super::{Addresses, Enumerated, Pass, Seed, Strategy, StrategyDef, Walk};
use crate::domain::Note;
use crate::strategies::{Keyed, Recognition};

pub struct Wmis;

inventory::submit! { StrategyDef { name: "wmis", it: &Wmis } }

/// The product, as a recognition names it.
const PRODUCT: &str = "SWFWMD Water Management Information System";

/// The field naming each application's WMIS page.
const RECORD: &str = "ERP_EXT_URL";

/// The field naming the application.
const APPLICATION: &str = "ERP_APPLICATION_ID";

/// Newest first: the day the application was received, then the newer application.
const ORDER: &str = "APPLICATION_RECEIVED_DT DESC,ERP_APPLICATION_ID DESC";

/// The layer's `maxRecordCount`.
const PAGE: usize = 1000;

/// The query parameters the walk sets, replacing the seed's: what to read, in what order,
/// and which page.
const OWNED: [&str; 9] = [
    "f",
    "outFields",
    "orderByFields",
    "returnGeometry",
    "resultOffset",
    "resultRecordCount",
    "returnCountOnly",
    "returnIdsOnly",
    "returnDistinctValues",
];

/// Where an application's WMIS page is, on either WMIS host.
const PERMIT_PAGE: &str = "/erp/erp/entry/erp.aspx";

/// What a document token opens, written as the permit page's own script writes it.
const EXPORT: &str = "/Erp/Common/Controls/ExportDocument.aspx";

/// A query page is depth 0, an application's page depth 1.
const MAX_DEPTH: usize = 1;

const HOURS: &str = "the District serves this map service 6 A.M. to 10 P.M. daily; a run \
                     outside those hours fails at its first request";

impl Strategy for Wmis {
    fn name(&self) -> &'static str {
        "wmis"
    }

    fn recognise(&self, seed: &Seed) -> Option<Recognition> {
        let at = seed.final_url()?;
        if !is_query(&at) {
            return None;
        }
        let answer: Answer = serde_json::from_slice(&seed.page.bytes).ok()?;
        // The layer is known by its fields, which a query answers even when no feature
        // matched; a feature's link to WMIS confirms it.
        let named = answer.fields.iter().any(|f| f.name == RECORD);
        let permits = answer.permits();
        if !named && permits.is_empty() {
            return None;
        }
        let mut r = Recognition::new(self.name(), Keyed::Product(PRODUCT))
            .seeing(
                "layer",
                format!(
                    "{} answers features naming each permit's WMIS page",
                    at.path()
                ),
            )
            .seeing(
                "documents",
                "from each application's WMIS page, newest application first",
            );
        if let Some(bound) = bound_of(&at) {
            r = r.seeing("bound", bound);
        }
        Some(r.warning("hours", HOURS))
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

            let mut pass = Pass::new(crawl, "permit list", MAX_DEPTH);
            pass.note(Note::new("layer", layer_of(&at)));
            if let Some(bound) = bound_of(&at) {
                pass.note(Note::new("bound", bound));
            }
            pass.push(Ask::Query(page_at(&at, 0)), 0);

            // Applications, newest first, opened once every page of the query is read.
            let mut permits: VecDeque<Url> = VecDeque::new();
            let mut listed: HashSet<u64> = HashSet::new();
            let (mut queries, mut opened, mut documents) = (0, 0, 0);
            loop {
                let Some((ask, depth)) = pass.next_to_visit() else {
                    // Nothing queued: open the next application. A ceiling leaves the queue
                    // holding what it stopped before, and ends the walk here.
                    if pass.pending() == 0
                        && let Some(page) = permits.pop_front()
                    {
                        pass.push(Ask::Permit(page), 1);
                        continue;
                    }
                    break;
                };
                pass.visiting(ask.to_string());
                let url = ask.url();
                let rules = pass.walk().robots(url.as_str()).await;
                if !rules.allowed(url.as_str()) {
                    pass.warn(format!("{ask}: disallowed by its host's robots.txt"));
                    continue;
                }
                let body = match pass.walk().get(url.as_str()).await {
                    Ok(f) => f.bytes,
                    Err(refusal) => {
                        pass.refused(&ask, &refusal);
                        continue;
                    }
                };
                match &ask {
                    Ask::Query(page) => {
                        // An ArcGIS server answers a query it cannot run with 200 and an
                        // `error` object, not a status.
                        let answer = match serde_json::from_slice::<Answer>(&body) {
                            Ok(a) if a.error.is_none() => a,
                            _ => {
                                pass.warn(format!("{ask}: the answer is not a feature set"));
                                continue;
                            }
                        };
                        pass.read_surface();
                        queries += 1;
                        for (id, record) in answer.permits() {
                            if listed.insert(id) {
                                permits.push_back(record);
                            }
                        }
                        if answer.exceeded_transfer_limit {
                            let next = offset_of(page) + answer.features.len();
                            pass.push(Ask::Query(page_at(&at, next)), depth);
                        }
                    }
                    Ask::Permit(page) => {
                        opened += 1;
                        for token in tokens_in(&String::from_utf8_lossy(&body)) {
                            if pass.full() {
                                break;
                            }
                            documents += 1;
                            let address = document_at(page, &token);
                            let rules = pass.walk().robots(&address).await;
                            pass.keep(address, &rules);
                        }
                    }
                }
            }

            pass.figure("queries", queries);
            pass.figure("permits", listed.len() as u64);
            pass.figure("opened", opened);
            pass.figure("documents", documents);
            Ok(pass.finish())
        })
    }

    /// Every address is a document.
    fn addresses_are(&self) -> Addresses {
        Addresses::Documents
    }
}

/// One request this walk makes.
#[derive(Clone, Debug)]
enum Ask {
    /// One page of the seed's query.
    Query(Url),
    /// One application's WMIS page, for its documents.
    Permit(Url),
}

impl Ask {
    fn url(&self) -> &Url {
        match self {
            Self::Query(u) | Self::Permit(u) => u,
        }
    }
}

/// How it reads in a warning, and what [`Pass`] dedups on.
impl fmt::Display for Ask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query(u) => write!(f, "permits from {}", offset_of(u)),
            Self::Permit(u) => write!(f, "permit page {u}"),
        }
    }
}

/// An ArcGIS query answer, cut to what the walk reads.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Answer {
    #[serde(default)]
    fields: Vec<Field>,
    #[serde(default)]
    features: Vec<Feature>,
    #[serde(default)]
    exceeded_transfer_limit: bool,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct Field {
    name: String,
}

#[derive(Debug, Deserialize)]
struct Feature {
    #[serde(default)]
    attributes: serde_json::Map<String, serde_json::Value>,
}

impl Answer {
    /// Each feature's application and WMIS page, in the order answered. A feature naming no
    /// WMIS page is not one.
    fn permits(&self) -> Vec<(u64, Url)> {
        self.features
            .iter()
            .filter_map(|f| {
                let id = f.attributes.get(APPLICATION)?.as_u64()?;
                let page = Url::parse(f.attributes.get(RECORD)?.as_str()?).ok()?;
                let wmis = page.path().eq_ignore_ascii_case(PERMIT_PAGE);
                wmis.then_some((id, page))
            })
            .collect()
    }
}

/// Whether an address is a query on a map or feature service layer.
fn is_query(at: &Url) -> bool {
    let path = at.path().to_ascii_lowercase();
    path.ends_with("/query") && (path.contains("/mapserver/") || path.contains("/featureserver/"))
}

/// The layer a query asks, for the note.
fn layer_of(at: &Url) -> String {
    let mut layer = at.clone();
    layer.set_query(None);
    layer.to_string().trim_end_matches("/query").to_string()
}

/// The seed's bound, as a person reads it: its `where` and its `geometry`.
fn bound_of(at: &Url) -> Option<String> {
    let parts: Vec<String> = at
        .query_pairs()
        .filter(|(k, v)| (k == "where" || k == "geometry") && !v.is_empty() && v != "1=1")
        .map(|(k, v)| format!("{k} {v}"))
        .collect();
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// The seed's query at one page: its own parameters, less the ones the walk owns, then the
/// walk's. A seed with no `where` asks for everything.
fn page_at(seed: &Url, offset: usize) -> Url {
    let mut page = seed.clone();
    let kept: Vec<(String, String)> = seed
        .query_pairs()
        .filter(|(k, _)| !OWNED.iter().any(|o| k == o))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let has_where = kept.iter().any(|(k, _)| k == "where");
    {
        let mut q = page.query_pairs_mut();
        q.clear();
        if !has_where {
            q.append_pair("where", "1=1");
        }
        for (k, v) in &kept {
            q.append_pair(k, v);
        }
        q.append_pair("outFields", &format!("{APPLICATION},{RECORD}"))
            .append_pair("orderByFields", ORDER)
            .append_pair("returnGeometry", "false")
            .append_pair("resultOffset", &offset.to_string())
            .append_pair("resultRecordCount", &PAGE.to_string())
            .append_pair("f", "json");
    }
    page
}

/// Which page of the query an address asks for.
fn offset_of(page: &Url) -> usize {
    page.query_pairs()
        .find(|(k, _)| k == "resultOffset")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0)
}

/// Every document token a WMIS page's grid names, in the order listed.
///
/// Read from each `<img>`'s `onclick`, the call `ViewDocument([{"Key":"BigSix",…}])`, and
/// never from a scan of the page's text. A token is base64 in its URL-safe alphabet; one
/// with any other character would need escaping the page never does, and is not kept.
fn tokens_in(page: &str) -> Vec<String> {
    #[derive(Deserialize)]
    struct Pair {
        #[serde(rename = "Key")]
        key: String,
        #[serde(rename = "Value")]
        value: String,
    }
    let scan = crate::html::Scan::new(page);
    scan.tags(&["img"])
        .into_iter()
        .filter_map(|tag| {
            let call = crate::html::unescape(tag.attr("onclick")?);
            let args = call.split_once("ViewDocument(")?.1;
            let list = &args[args.find('[')?..=args.rfind(']')?];
            let pairs: Vec<Pair> = serde_json::from_str(list).ok()?;
            pairs
                .into_iter()
                .find(|p| p.key == "BigSix")
                .map(|p| p.value)
        })
        .filter(|t| {
            !t.is_empty()
                && t.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'='))
        })
        .collect()
}

/// A document's one address: the token, on the host its permit page was read from.
fn document_at(page: &Url, token: &str) -> String {
    format!(
        "{}{EXPORT}?OpaqueId={token}",
        page.origin().ascii_serialization()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::crawl::tests::{Fake, seed};

    /// A query for Hillsborough's envelope since November 19, 2026, as an operator writes
    /// it. Its `outFields` and `f` are the walk's to set.
    const SEED: &str = "https://www25.swfwmd.state.fl.us/arcgis10/rest/services/Permits/Permits/MapServer/3/query?where=APPLICATION_RECEIVED_DT+%3E%3D+DATE+%272026-11-19%27&geometry=-82.70%2C27.646%2C-82.054%2C28.173&geometryType=esriGeometryEnvelope&inSR=4326&spatialRel=esriSpatialRelIntersects&outFields=*&f=json";

    const WMIS: &str = "https://www38.swfwmd.state.fl.us";

    /// The answer's head as the layer served it on 2026-11-22.
    const FIELDS: &str = r#""displayFieldName":"PERMITTEE_NAME","fieldAliases":{"ERP_APPLICATION_ID":"Application ID","ERP_EXT_URL":"WMIS Hyperlink"},"fields":[{"name":"ERP_APPLICATION_ID","type":"esriFieldTypeInteger","alias":"Application ID"},{"name":"ERP_EXT_URL","type":"esriFieldTypeString","alias":"WMIS Hyperlink","length":197}]"#;

    /// One feature, as the layer serves it.
    fn feature(id: u64) -> String {
        format!(
            r#"{{"attributes":{{"ERP_APPLICATION_ID":{id},"ERP_EXT_URL":"{WMIS}/Erp/ERP/Entry/ERP.aspx?id={id}"}}}}"#
        )
    }

    /// A page of features, saying whether more remain.
    fn answer(ids: &[u64], more: bool) -> String {
        let features: Vec<String> = ids.iter().map(|id| feature(*id)).collect();
        let tail = if more {
            r#","exceededTransferLimit":true"#
        } else {
            ""
        };
        format!(r#"{{{FIELDS},"features":[{}]{tail}}}"#, features.join(","))
    }

    fn permit(id: u64) -> String {
        format!("{WMIS}/Erp/ERP/Entry/ERP.aspx?id={id}")
    }

    fn document(token: &str) -> String {
        format!("{WMIS}/Erp/Common/Controls/ExportDocument.aspx?OpaqueId={token}")
    }

    /// One documents-grid row as application 1002708's page served it on 2026-11-22: the
    /// view icon carrying the token, the type, the day, the download icon.
    fn row(token: &str, kind: &str) -> String {
        format!(
            r##"<tr align="left" valign="top" id="row$1783018612007" bgcolor="#EFF3FB">
								<td align="center"><font face="Arial,Helvetica,sans-serif" color="Black"><img title="Click to see the document" src="../../Common/Images/IconPdf.gif" onclick="javascript:ViewDocument([{{&quot;Key&quot;:&quot;BigSix&quot;,&quot;Value&quot;:&quot;{token}&quot;}},{{&quot;Key&quot;:&quot;MimeType&quot;,&quot;Value&quot;:&quot;../../Common/Images/IconPdf.gif&quot;}}]);" /></font></td><td align="left"><font face="Arial,Helvetica,sans-serif" color="Black">{kind}</font></td><td align="left"><font face="Arial,Helvetica,sans-serif" color="Black">11/20/2026 3:09:47 AM</font></td><td align="center"><font face="Arial,Helvetica,sans-serif" color="Black"><img title="Click here to download the document" src="../../Common/Controls/Flex/Images/download.png" onclick="javascript:DownloadDocument('1783018612007');" /></font></td>
							</tr>"##
        )
    }

    /// A permit page cut to its documents grid, and the script that opens a token.
    fn page(rows: &[String]) -> String {
        format!(
            r##"<html><head><title>
	Environmental Resource Permit Apps
</title></head><body><form method="post" action="./ERP.aspx?id=1002708" id="form1">
<input type="hidden" name="__VIEWSTATE" id="__VIEWSTATE" value="V+n4eJak3WrVH3NdzCc4m/QY1JWOs0JNcuo1d0QyxKYlzMZG" />
<script type="text/javascript">
    function eD(OpaqueId) {{
	    var appRoot = '/Erp/';
	    window.open(appRoot + 'Common/Controls/ExportDocument.aspx?OpaqueId=' + OpaqueId);
    }}
</script>
<table class="flexTable flexTable-bordered flexTable-condensed noWrapCell" id="Detail1_AjaxPermitTabs_TabPanel8_ERPDocuments_doc_DocsGrid_customGrid" width="100%">
<tr align="left" valign="top" id="row$" bgcolor="#507CD1"><th align="left" scope="col">Doc</th><th>Document Type</th><th>Date</th><th>Download</th></tr>{}
</table></form></body></html>"##,
            rows.join("")
        )
    }

    const APPLICATION_1002708: &str = "p2avH3Qj9SNwKvPrJjl3UlOnHUFJSHB_ZHW4eYdXU1hacvOFUHpwlW-a7kSkDBQrewtwbMaqrZ2j2zD4VcClY41WROxpLGYgmpCn-psUBWc=";
    const AUTHORIZATION_1002708: &str = "p2avH3Qj9SNwKvPrJjl3UlOnHUFJSHB_ZHW4eYdXU1huKGksJdcppV1sxGlci36O228pUbp37xIU_m3maW4w3MuzaXTsFt1bo5i8llTAYM8=";
    const DRAINAGE_1002708: &str = "p2avH3Qj9SNwKvPrJjl3UlOnHUFJSHB_ZHW4eYdXU1ivrJTvJTNBuS1e4Aii9cSs7xVazKv1kwlISvEOPlJ1I2vtkQY-iH-zkN9ZHVRLW20=";
    const PERMIT_921913: &str = "p2avH3Qj9SNwKvPrJjl3UlOnHUFJSHB_ZHW4eYdXU1gvzaZ20jqbd2KJ9GDWOy-axr0za1_c_pwQ2GL_5V0gEozGYsTeGqcVVtvYuzBHDnI=";

    fn query(offset: usize) -> String {
        page_at(&Url::parse(SEED).unwrap(), offset).to_string()
    }

    /// The walk keeps the seed's bound and sets what it reads, in what order, and which
    /// page: newest application first, a thousand at a time, no geometry.
    #[test]
    fn a_page_keeps_the_seed_bound_and_owns_order_and_paging() {
        assert_eq!(
            query(1000),
            "https://www25.swfwmd.state.fl.us/arcgis10/rest/services/Permits/Permits/MapServer/3/query?where=APPLICATION_RECEIVED_DT+%3E%3D+DATE+%272026-11-19%27&geometry=-82.70%2C27.646%2C-82.054%2C28.173&geometryType=esriGeometryEnvelope&inSR=4326&spatialRel=esriSpatialRelIntersects&outFields=ERP_APPLICATION_ID%2CERP_EXT_URL&orderByFields=APPLICATION_RECEIVED_DT+DESC%2CERP_APPLICATION_ID+DESC&returnGeometry=false&resultOffset=1000&resultRecordCount=1000&f=json"
        );
        let everything = Url::parse(
            "https://www25.swfwmd.state.fl.us/arcgis10/rest/services/Permits/Permits/MapServer/3/query?f=json",
        )
        .unwrap();
        assert!(
            page_at(&everything, 0)
                .query()
                .unwrap()
                .starts_with("where=1%3D1&"),
            "a seed with no where asks for everything"
        );
    }

    /// Every page of the query first, then each application newest first, each opened once
    /// though the layer draws it twice; every document has one address, its token.
    #[tokio::test]
    async fn the_walk_reads_every_page_then_each_application_newest_first() {
        let s = seed(&answer(&[1002708], false), SEED);
        let crawl = Fake::new([
            (
                query(0).as_str(),
                answer(&[1002708, 1002708, 1002703], true),
            ),
            (query(3).as_str(), answer(&[921913], false)),
            (
                permit(1002708).as_str(),
                page(&[
                    row(APPLICATION_1002708, "Application"),
                    row(AUTHORIZATION_1002708, "Application Authorization"),
                    row(DRAINAGE_1002708, "Calculations/Drainage Information"),
                ]),
            ),
            (permit(1002703).as_str(), page(&[])),
            (
                permit(921913).as_str(),
                page(&[row(PERMIT_921913, "Permit")]),
            ),
        ]);

        let got = Wmis.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                document(APPLICATION_1002708),
                document(AUTHORIZATION_1002708),
                document(DRAINAGE_1002708),
                document(PERMIT_921913),
            ]
        );
        assert_eq!(
            crawl.requests(),
            5,
            "two query pages and three permit pages"
        );
        assert_eq!(got.figures["queries"], 2);
        assert_eq!(got.figures["permits"], 3);
        assert_eq!(got.figures["opened"], 3);
        assert_eq!(got.figures["documents"], 4);
        assert!(
            !got.truncated && !got.discovery_failed && got.warnings.is_empty(),
            "{:?}",
            got.warnings
        );
    }

    /// A budget that runs out holds every document of the newest application and says it
    /// stopped; the older application is never opened.
    #[tokio::test]
    async fn a_walk_out_of_budget_has_the_newest_applications_documents() {
        let s = seed(&answer(&[1002708], false), SEED);
        let crawl = Fake::new([
            (query(0).as_str(), answer(&[1002708, 921913], false)),
            (
                permit(1002708).as_str(),
                page(&[row(APPLICATION_1002708, "Application")]),
            ),
            (
                permit(921913).as_str(),
                page(&[row(PERMIT_921913, "Permit")]),
            ),
        ])
        .with_budget(2);

        let got = Wmis.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(got.addresses, [document(APPLICATION_1002708)]);
        assert!(got.truncated, "{:?}", got.warnings);
    }

    /// An ArcGIS server answers a query it cannot run with 200 and an error object: that is
    /// no surface read, and the run says discovery failed rather than found nothing.
    #[tokio::test]
    async fn an_error_answer_is_a_failed_discovery_not_an_empty_one() {
        let s = seed(&answer(&[], false), SEED);
        let crawl = Fake::new([(
            query(0).as_str(),
            r#"{"error":{"code":400,"message":"Failed to execute query.","details":[]}}"#
                .to_string(),
        )]);

        let got = Wmis.enumerate(&s, &crawl).await.unwrap();

        assert!(got.addresses.is_empty());
        assert!(got.discovery_failed, "{:?}", got.warnings);
        assert!(
            got.warnings.iter().any(|w| w.contains("not a feature set")),
            "{:?}",
            got.warnings
        );
    }

    /// Recognised on a query of a layer whose features name WMIS pages, even one that
    /// matched nothing; the service's hours are said up front. Another layer, or the layer's
    /// description rather than a query, is not recognised.
    #[test]
    fn a_query_naming_wmis_pages_is_recognised() {
        let r = Wmis
            .recognise(&seed(&answer(&[1002708], false), SEED))
            .expect("the ERP layer's query");
        assert_eq!(r.keyed_on, Keyed::Product(PRODUCT));
        assert!(
            r.warnings.iter().any(|n| n.label == "hours"),
            "{:?}",
            r.warnings
        );
        assert!(
            Wmis.recognise(&seed(&answer(&[], false), SEED)).is_some(),
            "a query that matched nothing still names the field"
        );

        // The water use permits layer: its features link WupSimple.aspx, not an ERP page.
        let wup = r#"{"fields":[{"name":"WUP_EXT_URL"}],"features":[{"attributes":{"WUP_EXT_URL":"https://www38.swfwmd.state.fl.us/Support/WUP/WupSimple.aspx?st=8833&rev=11"}}]}"#;
        assert!(
            Wmis.recognise(&seed(wup, &SEED.replace("/3/", "/5/")))
                .is_none()
        );
        assert!(Wmis
            .recognise(&seed(
                &answer(&[1002708], false),
                "https://www25.swfwmd.state.fl.us/arcgis10/rest/services/Permits/Permits/MapServer/3?f=json",
            ))
            .is_none());
    }

    /// A token is read off the grid's view call and nothing else: not the download call's
    /// collection id, not the page's own script.
    #[test]
    fn tokens_come_from_the_view_call_only() {
        let p = page(&[
            row(APPLICATION_1002708, "Application"),
            row("not a token", "Broken"),
        ]);
        assert_eq!(tokens_in(&p), [APPLICATION_1002708]);
    }

    /// A permit page on a host whose robots.txt disallows it is said and not read: the
    /// rules are the permit host's, not the map service's.
    #[tokio::test]
    async fn a_disallowed_permit_host_is_reported_not_read() {
        let s = seed(&answer(&[1002708], false), SEED);
        let crawl = Fake::new([(query(0).as_str(), answer(&[1002708], false))])
            .with_robots("www38.swfwmd.state.fl.us", "User-agent: *\nDisallow: /\n");

        let got = Wmis.enumerate(&s, &crawl).await.unwrap();

        assert!(got.addresses.is_empty());
        assert_eq!(crawl.requests(), 1, "the permit page was not fetched");
        assert!(
            got.warnings.iter().any(|w| w.contains("robots.txt")),
            "{:?}",
            got.warnings
        );
    }
}
