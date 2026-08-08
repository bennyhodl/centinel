//! HTTP fetching, shared by every op that pulls bytes.
//!
//! One code path on purpose. `ingest` and `collect` must classify a WAF 403 identically
//! and record identical transport metadata, or the archive's provenance depends on which
//! command happened to be used — a difference no consumer could see or correct for.
//!
//! *What a blob is* lives in [`crate::content`], not here. This module knows how to ask a
//! server for bytes and what a refusal means; it holds no opinion about formats.

use std::collections::BTreeMap;

use crate::domain::{Fetched, Liveness};
use crate::policy::HostPolicy;

/// A fetch that failed.
///
/// The name is kept for the call sites; the type is [`crate::domain::Refusal`], which is
/// what `yt-dlp` failures are too. These were written twice and were always the same
/// thing — `{state, detail}`, each with its own `classify` — and one shared acquisition
/// loop cannot exist while a refusal has two types.
pub use crate::domain::Refusal as FetchFailure;

/// An HTTP client configured by [`HostPolicy`].
#[derive(Clone, Debug)]
pub struct Fetcher {
    client: reqwest::Client,
}

impl Fetcher {
    pub fn new(policy: &HostPolicy) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .user_agent(&policy.user_agent)
                .timeout(policy.timeout)
                .build()?,
        })
    }

    /// GETs a URL, classifying any non-success status into a [`Liveness`].
    pub async fn get(&self, url: &str) -> Result<Fetched, FetchFailure> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| FetchFailure {
                state: Liveness::Error,
                detail: e.to_string(),
            })?;

        let status = resp.status();
        if !status.is_success() {
            return Err(FetchFailure {
                state: classify(status.as_u16()),
                detail: format!("HTTP {status}"),
            });
        }
        // A success status is not the last word. Plenty of platforms answer 200 and hand
        // over an error page, and the only remaining evidence is where they sent us.
        if let Some(state) = error_redirect(url, resp.url()) {
            return Err(FetchFailure {
                state,
                detail: format!("HTTP {status}, redirected to {}", resp.url()),
            });
        }

        // Captured because they are the cheap conditional-request signals a later pass
        // will want, and because they cannot be recovered after the fact.
        let mut meta = BTreeMap::new();
        for header in ["content-type", "etag", "last-modified"] {
            if let Some(v) = resp.headers().get(header)
                && let Ok(s) = v.to_str()
            {
                meta.insert(header.to_string(), s.to_string());
            }
        }
        meta.insert("http_status".into(), status.as_u16().to_string());
        // The post-redirect URL — where the bytes actually came from.
        meta.insert("final_url".into(), resp.url().to_string());

        let bytes = resp.bytes().await.map_err(|e| FetchFailure {
            state: Liveness::Error,
            detail: format!("body read failed: {e}"),
        })?;

        Ok(Fetched {
            bytes: bytes.to_vec(),
            meta,
        })
    }
}

/// Path segments that name a missing document, and ones that name a fault.
///
/// Matched whole and lowercased, never as a substring: a city's `/services/error-reporting`
/// page is a real page about reporting errors, and refusing it would be the same mistake in
/// the opposite direction.
const MISSING: &[&str] = &[
    "notfound",
    "not-found",
    "pagenotfound",
    "page-not-found",
    "404",
];
const FAULTED: &[&str] = &["error", "errors", "internalserver", "servererror", "500"];

/// Whether a 200 was really the server saying no.
///
/// **The inverse of [`Liveness::Blocked`], and it costs more.** `Blocked` exists so a live
/// page is never recorded as deleted. This exists so a *deleted* page is never recorded as
/// live — because on Hyland OnBase a missing document and a server crash both come back
/// `200 text/html` reading *"The web page that you have requested is not available"*:
///
/// | Requested | Final URL | Status |
/// |---|---|---|
/// | `/251agendaonline/.pdf?documentType=` | `/251agendaonline/Error/NotFound?aspxerrorpath=…` | **200** |
/// | `…/ViewAgenda?meetingId=2500&doctype=` | `/251agendaonline/Error/InternalServer?aspxerrorpath=…` | **200** |
///
/// Left unasked, [`Liveness::Gone`] can never fire on such a host: every dead address
/// becomes a successful acquisition with real bytes, and a document titled *"Error - OnBase
/// Agenda Online"* enters the search index. ASP.NET, ColdFusion and most CMS platforms
/// answer 200 on error far more often than they should.
///
/// **The redirect is the evidence, not the path.** A page that lives at `/error` and was
/// asked for at `/error` was not redirected anywhere, so nothing here fires on it. Only a
/// server that moved us to a path naming an error has said anything.
///
/// `Gone` needs the page to say *not found*; anything else that merely names an error is
/// [`Liveness::Error`], which is not evidence of absence. Guessing `Gone` from a 500 would
/// log a live document as deleted, which is the whole failure this is here to prevent.
fn error_redirect(requested: &str, final_url: &url::Url) -> Option<Liveness> {
    let requested = url::Url::parse(requested).ok()?;
    if requested.path() == final_url.path() {
        return None;
    }

    let segments: Vec<String> = final_url
        .path_segments()
        .into_iter()
        .flatten()
        .map(str::to_ascii_lowercase)
        .collect();
    let names = |set: &[&str]| segments.iter().any(|s| set.contains(&s.as_str()));

    if names(MISSING) {
        return Some(Liveness::Gone);
    }
    if names(FAULTED) {
        return Some(Liveness::Error);
    }
    // ASP.NET's own marker for "the path you asked for threw". It says a fault happened
    // without saying which, so it can only ever reach the answer that claims less.
    final_url
        .query()
        .is_some_and(|q| q.to_ascii_lowercase().contains("aspxerrorpath="))
        .then_some(Liveness::Error)
}

/// Maps an HTTP status onto liveness.
///
/// The 403 → [`Liveness::Blocked`] mapping is the load-bearing one. Both `phila.gov` and
/// `sec.gov` were measured returning WAF 403s with no `Retry-After`; classifying those as
/// `Gone` would record a live page as deleted.
pub fn classify(status: u16) -> Liveness {
    match status {
        404 | 410 => Liveness::Gone,
        401 | 403 | 429 => Liveness::Blocked,
        _ => Liveness::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waf_403_is_blocked_not_gone() {
        assert_eq!(classify(403), Liveness::Blocked);
        assert_eq!(classify(429), Liveness::Blocked);
        assert_eq!(classify(401), Liveness::Blocked);
        assert_eq!(classify(404), Liveness::Gone);
        assert_eq!(classify(410), Liveness::Gone);
        assert_eq!(classify(500), Liveness::Error);
        assert_eq!(classify(503), Liveness::Error);
    }

    fn redirected(from: &str, to: &str) -> Option<Liveness> {
        error_redirect(from, &url::Url::parse(to).unwrap())
    }

    /// Both sightings, verbatim. Left unasked, each is an Observation with real bytes and
    /// a document titled "Error - OnBase Agenda Online" in the search index.
    #[test]
    fn a_two_hundred_that_landed_on_an_error_page_is_a_refusal() {
        assert_eq!(
            redirected(
                "https://tampagov.hylandcloud.com/251agendaonline/.pdf?documentType=",
                "https://tampagov.hylandcloud.com/251agendaonline/Error/NotFound?aspxerrorpath=/251agendaonline/.pdf",
            ),
            Some(Liveness::Gone),
            "the page said not found, and that is a fact about the address"
        );
        assert_eq!(
            redirected(
                "https://tampagov.hylandcloud.com/251agendaonline/Documents/ViewAgenda?meetingId=2500",
                "https://tampagov.hylandcloud.com/251agendaonline/Error/InternalServer?aspxerrorpath=/x",
            ),
            Some(Liveness::Error),
            "a crash is not evidence the document is absent"
        );
    }

    /// The mistake this must not make: refusing a live page.
    #[test]
    fn an_ordinary_page_is_never_refused_for_what_its_path_says() {
        // No redirect at all — much the commonest case.
        assert_eq!(
            redirected("https://x.gov/budget.pdf", "https://x.gov/budget.pdf"),
            None
        );
        // A page that is genuinely about errors, asked for by name.
        assert_eq!(
            redirected(
                "https://x.gov/services/error",
                "https://x.gov/services/error"
            ),
            None
        );
        // A redirect to a real page whose segment merely contains the word.
        assert_eq!(
            redirected(
                "https://x.gov/report",
                "https://x.gov/services/error-reporting"
            ),
            None,
            "a segment is matched whole, never as a substring"
        );
        // http → https on the same path is not a redirect to anywhere.
        assert_eq!(
            redirected(
                "http://x.gov/a/404-report.pdf",
                "https://x.gov/a/404-report.pdf"
            ),
            None
        );
    }

    /// ASP.NET names the fault in its query when the path does not.
    #[test]
    fn the_platform_marker_answers_where_the_path_is_silent() {
        assert_eq!(
            redirected(
                "https://x.gov/a.pdf",
                "https://x.gov/oops?aspxerrorpath=/a.pdf"
            ),
            Some(Liveness::Error),
            "it proves a fault without proving absence"
        );
        assert_eq!(
            redirected("https://x.gov/a.pdf", "https://x.gov/somewhere-else"),
            None,
            "an ordinary redirect is an ordinary redirect"
        );
    }

    #[test]
    fn an_unparseable_request_url_is_simply_no_evidence() {
        assert_eq!(
            redirected("not a url", "https://x.gov/Error/NotFound"),
            None
        );
    }
}
