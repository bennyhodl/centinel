//! A log of every request an op made, and what each one returned.
//!
//! `investigate` and `check` answer a question about a host in a handful of requests, and
//! when the answer is "nothing here collects well" the next question is always *what did
//! the site actually say?* — the status, the type, how many bytes, where it sent us. That is
//! what a person (or an agent improving a scraper) needs in hand, and it is only knowable at
//! the moment of the request: a 403's size and a redirect's target are gone once the walk
//! has folded them into a warning.
//!
//! ## One recorder, at the one place requests are made
//!
//! Every request either op makes goes through [`SiteSource`](crate::sources::SiteSource),
//! so that is where [`RequestLog::timed`] wraps the fetch. Neither op counts its own
//! requests, and a new strategy that fetches through [`Walk`] is recorded without knowing
//! the log exists. `check` makes two sources (the document, then an enumeration) and gives
//! both the same log, which is why [`RequestLog`] is a shared handle rather than a `Vec`
//! each op returns.
//!
//! Recording is **opt-in** ([`SiteSource::with_log`](crate::sources::SiteSource::with_log)):
//! a collect of a hundred thousand pages would otherwise hold a hundred thousand records in
//! memory for a report nobody asked for.
//!
//! [`Walk`]: crate::strategies::crawl::Walk

use std::future::Future;
use std::sync::{Arc, Mutex};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::domain::{Fetched, Refusal};
use crate::fetch::status_of;
use crate::render::{self, Align, Cell, Ink, Painter, Table};

/// Which part of an op made a request.
///
/// A closed set because the answer is always one of the places a [`SiteSource`] fetches,
/// and a person reading the log wants to tell "the site's own sitemap" from "the page the
/// op was pointed at" without parsing a path.
///
/// [`SiteSource`]: crate::sources::SiteSource
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    /// `/robots.txt`, read before anything recognises the site.
    Robots,
    /// The address as given: what every recogniser keys on.
    Seed,
    /// A fetch made by a strategy walking the site, named so a sitemap's requests read
    /// apart from a directory listing's.
    Probe { strategy: String },
    /// The document `check` was pointed at.
    Page,
    /// A document the page carries at its own address, fetched because `check` follows them.
    Enclosure,
}

impl std::fmt::Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Robots => f.write_str("robots.txt"),
            Self::Seed => f.write_str("seed"),
            Self::Probe { strategy } => write!(f, "probe ({strategy})"),
            Self::Page => f.write_str("page"),
            Self::Enclosure => f.write_str("enclosure"),
        }
    }
}

/// One request, and what came back.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RequestRecord {
    pub step: Step,
    pub method: String,
    /// As asked for.
    pub url: String,
    /// Absent when no answer arrived at all — a timeout or a DNS failure — which is a
    /// different fact from any status and is never shown as one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// The `content-type` the server declared. Absent on a refusal: the fetcher does not
    /// read the body of a non-success, so neither the type nor the size of a 403's page is
    /// known here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    pub bytes: u64,
    /// The request alone, without any wait for the pacer.
    pub elapsed_ms: u64,
    /// When it was sent, in Unix milliseconds, so two logs from one investigation sort into
    /// one timeline.
    pub at_ms: i64,
    /// Where the redirects ended, when they moved it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirected_to: Option<String>,
    /// Why it was refused, in the fetcher's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RequestRecord {
    fn of(step: Step, url: &str, at_ms: i64, elapsed_ms: u64, result: &Result<Fetched, Refusal>) -> Self {
        let base = Self {
            step,
            method: "GET".into(),
            url: url.to_string(),
            status: status_of(result),
            content_type: None,
            bytes: 0,
            elapsed_ms,
            at_ms,
            redirected_to: None,
            error: None,
        };
        match result {
            Ok(fetched) => Self {
                content_type: fetched.meta.get("content-type").cloned(),
                bytes: fetched.bytes.len() as u64,
                redirected_to: fetched
                    .meta
                    .get("final_url")
                    .filter(|served| served.as_str() != url)
                    .cloned(),
                ..base
            },
            Err(refusal) => Self {
                error: Some(refusal.detail.clone()),
                ..base
            },
        }
    }

    /// Whether the server answered with a success status.
    pub fn ok(&self) -> bool {
        self.error.is_none()
    }
}

/// Every request one op made, shared by whatever made them.
#[derive(Clone, Debug, Default)]
pub struct RequestLog(Arc<Mutex<Vec<RequestRecord>>>);

impl RequestLog {
    /// Runs one fetch and records it, returning the fetch's own answer untouched.
    pub async fn timed(
        &self,
        step: Step,
        url: &str,
        fetch: impl Future<Output = Result<Fetched, Refusal>>,
    ) -> Result<Fetched, Refusal> {
        let at_ms = jiff::Timestamp::now().as_millisecond();
        let started = std::time::Instant::now();
        let result = fetch.await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        self.0
            .lock()
            .expect("the request log is never poisoned")
            .push(RequestRecord::of(step, url, at_ms, elapsed_ms, &result));
        result
    }

    /// What has been recorded so far, in the order the requests were sent.
    pub fn records(&self) -> Vec<RequestRecord> {
        let mut records = self
            .0
            .lock()
            .expect("the request log is never poisoned")
            .clone();
        records.sort_by_key(|r| r.at_ms);
        records
    }
}

/// The log as a table, for the end of a report: one row per request, offsets from the first.
///
/// Shared by `investigate` and `check` so the two read alike, and so a request that
/// refused is marked the same way in both.
pub fn render(records: &[RequestRecord], p: &mut Painter<'_>) -> std::io::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    p.section("requests")?;
    let first = records.iter().map(|r| r.at_ms).min().unwrap_or(0);
    let mut table = Table::bare(&[
        Align::Right,
        Align::Left,
        Align::Left,
        Align::Left,
        Align::Right,
        Align::Right,
        Align::Left,
    ]);
    for r in records {
        let at = format!("+{:.2}s", (r.at_ms - first) as f64 / 1000.0);
        let status = r.status.map_or("—".to_string(), |s| s.to_string());
        let sized = match r.content_type.is_some() || r.bytes > 0 {
            true => render::bytes(r.bytes),
            false => String::new(),
        };
        let aside = match (&r.redirected_to, &r.error) {
            (Some(to), _) => format!("→ {to}"),
            (None, Some(error)) if r.status.is_none() => error.clone(),
            _ => r.content_type.clone().unwrap_or_default(),
        };
        table.push(vec![
            Cell::dim(at),
            Cell::new(status, if r.ok() { Ink::Plain } else { Ink::Red }),
            Cell::dim(r.step.to_string()),
            Cell::plain(render::truncate(&r.url, 60)),
            Cell::dim(sized),
            Cell::dim(format!("{} ms", r.elapsed_ms)),
            Cell::dim(render::one_line(&aside)),
        ]);
    }
    p.nest(|p| p.table(&table))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Liveness;

    fn refused(status: Option<u16>, detail: &str) -> RequestRecord {
        RequestRecord::of(
            Step::Seed,
            "https://x.gov/",
            0,
            5,
            &Err(Refusal {
                state: Liveness::Error,
                detail: detail.into(),
                status,
            }),
        )
    }

    /// A timeout and a 500 are different facts. A record that put `0` or `500` where there
    /// was no answer would make the log say something the site never did.
    #[test]
    fn a_request_that_never_got_an_answer_carries_no_status() {
        let timed_out = refused(None, "error sending request: operation timed out");
        assert_eq!(timed_out.status, None);
        assert!(!timed_out.ok());
        assert!(timed_out.error.as_deref().unwrap().contains("timed out"));

        assert_eq!(refused(Some(503), "HTTP 503 Service Unavailable").status, Some(503));
    }

    fn rendered(records: &[RequestRecord]) -> String {
        let mut buf = Vec::new();
        render(records, &mut Painter::new(&mut buf, false, 120)).unwrap();
        String::from_utf8(buf).unwrap()
    }

    /// The table a person reads at the end of `investigate`: offsets from the first request,
    /// a redirect's target, and a refusal's reason where there is no status to show.
    #[test]
    fn the_table_shows_offsets_redirects_and_refusals() {
        let mut moved = RequestRecord::of(
            Step::Seed,
            "https://x.gov/old",
            1_000,
            64,
            &Ok(Fetched {
                bytes: vec![0; 2048],
                meta: [
                    ("http_status".to_string(), "200".to_string()),
                    ("content-type".to_string(), "text/html".to_string()),
                    ("final_url".to_string(), "https://x.gov/new".to_string()),
                ]
                .into(),
            }),
        );
        moved.at_ms = 1_000;
        let mut failed = refused(None, "error sending request: operation timed out");
        failed.at_ms = 1_420;

        let out = rendered(&[moved, failed]);
        assert!(out.contains("+0.00s") && out.contains("+0.42s"), "{out}");
        assert!(out.contains("→ https://x.gov/new"), "{out}");
        assert!(out.contains("operation timed out"), "{out}");
        assert_eq!(rendered(&[]), "", "an op that fetched nothing prints no table");
    }
}
