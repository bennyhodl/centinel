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
use crate::policy::{DEFAULT_MAX_BYTES, HostPolicy};

/// A fetch that failed.
///
/// The name is kept for the call sites; the type is [`crate::domain::Refusal`], which is
/// what `yt-dlp` failures are too. These were written twice and were always the same
/// thing — `{state, detail}`, each with its own `classify` — and one shared acquisition
/// loop cannot exist while a refusal has two types.
pub use crate::domain::Refusal as FetchFailure;

/// Somewhere bytes come from.
///
/// The seam a Source's tests substitute at, and the reason it exists: everything above the
/// fetch in [`crate::sources::site`] — choosing a strategy, building a seed from a front
/// door that refused, turning a strategy's addresses into Resources — was reachable only
/// over a socket. So it went untested, and `docs/FIELD-NOTES.md` records what was wrong
/// with it: a truncation flag dropped on the way out, a degraded seed nothing exercised,
/// and an integration test that stood up a `TcpListener` and hand-wrote HTTP to get at any
/// of it.
///
/// One method, because one method is all a Source ever asked for. `CONTEXT.md` states the
/// rule this satisfies — *the interface is the test surface* — and the branch that wrote it
/// down applied it to [`crate::strategies::crawl::Walk`] and not here.
pub trait Fetch: Send + Sync + std::fmt::Debug {
    /// GETs a URL, classifying any non-success status into a [`Liveness`].
    fn get<'a>(
        &'a self,
        url: &'a str,
    ) -> futures::future::BoxFuture<'a, Result<Fetched, FetchFailure>>;
}

/// An HTTP client configured by [`HostPolicy`]. The production [`Fetch`].
#[derive(Clone, Debug)]
pub struct Fetcher {
    client: reqwest::Client,
    max_bytes: u64,
}

impl Fetch for Fetcher {
    fn get<'a>(
        &'a self,
        url: &'a str,
    ) -> futures::future::BoxFuture<'a, Result<Fetched, FetchFailure>> {
        Box::pin(Fetcher::get(self, url))
    }
}

impl Fetcher {
    pub fn new(policy: &HostPolicy) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .user_agent(&policy.user_agent)
                .timeout(policy.timeout)
                .build()?,
            max_bytes: policy.max_bytes,
        })
    }

    /// GETs a URL, classifying any non-success status into a [`Liveness`].
    ///
    /// Room in the [memory budget](crate::memory) is taken before the request is sent, the
    /// ceiling's worth until the server says how long the body is (see [`Self::body`]). A
    /// run walks sixteen hosts at once, and the ceiling bounds one body, not the sixteen:
    /// on the field VM four hosts reading documents together took a run from 200 to 712 MB.
    /// Waiting before the request costs only time. Waiting with a response open would spend
    /// the request's timeout on the wait, and a page would be recorded as an error because
    /// the machine was busy.
    ///
    /// The room taken is at most [`DEFAULT_MAX_BYTES`], even under a raised ceiling. Raising
    /// it is for the few large documents worth having (`collect --max-mb 2048`), and
    /// reserving the raised figure would make every 50 KB page in the same pass wait for
    /// 2 GiB. The ceiling itself is still enforced on the body as it streams; a document
    /// past the default is simply held without all of its room reserved.
    pub async fn get(&self, url: &str) -> Result<Fetched, FetchFailure> {
        let mut held = crate::memory::budget()
            .reserve(self.max_bytes.min(DEFAULT_MAX_BYTES))
            .await;
        let started = std::time::Instant::now();
        let resp = self.client.get(url).send().await.map_err(|e| {
            let ms = started.elapsed().as_millis() as u64;
            tracing::debug!(url, error = %e, ms, "GET failed");
            FetchFailure {
                state: Liveness::Error,
                detail: e.to_string(),
                status: None,
                too_large: None,
            }
        })?;

        // The call as the wire saw it: the status, where the redirects ended, the type
        // claimed. What the op makes of it is reported as the item's outcome.
        let status = resp.status();
        tracing::debug!(
            url,
            status = status.as_u16(),
            final_url = %resp.url(),
            content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("-"),
            ms = started.elapsed().as_millis() as u64,
            "GET"
        );
        if !status.is_success() {
            return Err(FetchFailure {
                state: classify(status.as_u16()),
                detail: format!("HTTP {status}"),
                status: Some(status.as_u16()),
                too_large: None,
            });
        }
        // A success status is not the last word. Plenty of platforms answer 200 and hand
        // over an error page, and the only remaining evidence is where they sent us.
        if let Some(state) = error_redirect(url, resp.url()) {
            return Err(FetchFailure {
                state,
                detail: format!("HTTP {status}, redirected to {}", resp.url()),
                status: Some(status.as_u16()),
                too_large: None,
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

        let bytes = self.body(resp, &mut held).await?;
        Ok(Fetched { bytes, meta })
    }

    /// Reads a body into memory, refusing one over the ceiling before it is all there.
    ///
    /// When the server states the length, an oversized body is refused before a byte of
    /// it is read. Otherwise it is read a piece at a time into one buffer and refused the
    /// moment it passes the ceiling. Reading it whole first cost a body twice over, once
    /// as the frames arrived and once as one contiguous copy, and checked the ceiling only
    /// after: an 830 MB slideshow took a run on a 3 GB machine to 2.2 GB and the kernel
    /// killed it.
    ///
    /// `held` is the room [`Self::get`] took at the ceiling. A declared length gives back
    /// what it does not need, so a server that says how long its bodies are lets more of
    /// them be read at once. One that does not, which is any compressed page, keeps the
    /// ceiling until its body is read, since nothing short of reading it says how much it
    /// holds. Growing the room as the body grows would mean waiting with room already held,
    /// and two bodies doing that can wait on each other for good.
    async fn body(
        &self,
        mut resp: reqwest::Response,
        held: &mut crate::memory::Reservation<'_>,
    ) -> Result<Vec<u8>, FetchFailure> {
        let ceiling = crate::render::bytes(self.max_bytes);
        let status = resp.status().as_u16();
        let refuse = |detail: String, bytes: u64| FetchFailure {
            state: Liveness::Error,
            detail,
            status: Some(status),
            too_large: Some(bytes),
        };

        let declared = resp.content_length();
        if let Some(len) = declared
            && len > self.max_bytes
        {
            return Err(refuse(
                format!(
                    "{} declared, over the {ceiling} ceiling",
                    crate::render::bytes(len)
                ),
                len,
            ));
        }
        if let Some(len) = declared {
            held.shrink_to(len);
        }

        let mut body = Vec::with_capacity(declared.unwrap_or(0) as usize);
        while let Some(chunk) = resp.chunk().await.map_err(|e| FetchFailure {
            state: Liveness::Error,
            detail: format!("body read failed: {e}"),
            status: None,
            too_large: None,
        })? {
            if (body.len() + chunk.len()) as u64 > self.max_bytes {
                // Also a server whose declared length was short of the truth.
                let read = (body.len() + chunk.len()) as u64;
                return Err(refuse(format!("over the {ceiling} ceiling"), read));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

/// The HTTP status a fetch ended with: the one it succeeded with, or the one the server
/// refused it with. `None` when the server never answered.
///
/// "The server said 404" and "we never reached the server" are different facts that a bare
/// message would show alike. Both sides are read here, from where each keeps it, so no
/// consumer parses a status out of prose or out of the metadata on its own.
pub fn status_of(result: &Result<Fetched, FetchFailure>) -> Option<u16> {
    match result {
        Ok(fetched) => fetched.meta.get("http_status").and_then(|s| s.parse().ok()),
        Err(refusal) => refusal.status,
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

    /// A success carries the status in its metadata and a refusal carries it as a number;
    /// one that never reached the server has none.
    #[test]
    fn the_status_of_a_fetch_is_read_from_wherever_it_ended() {
        let fetched = Fetched {
            bytes: Vec::new(),
            meta: BTreeMap::from([("http_status".to_string(), "200".to_string())]),
        };
        assert_eq!(status_of(&Ok(fetched)), Some(200));
        let refused = |status| FetchFailure {
            state: Liveness::Gone,
            detail: "x".into(),
            status,
            too_large: None,
        };
        assert_eq!(status_of(&Err(refused(Some(404)))), Some(404));
        assert_eq!(status_of(&Err(refused(None))), None);
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
