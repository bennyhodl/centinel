//! HTTP fetching, shared by every op that pulls bytes.
//!
//! One code path on purpose. `ingest` and `collect` must classify a WAF 403 identically
//! and record identical transport metadata, or the archive's provenance depends on which
//! command happened to be used — a difference no consumer could see or correct for.

use std::collections::BTreeMap;

use crate::domain::{Fetched, Liveness};
use crate::policy::HostPolicy;

/// A fetch that failed. Carries liveness rather than an error type, because the caller's
/// job is to record *what kind* of failure this was, not to propagate it.
#[derive(Clone, Debug)]
pub struct FetchFailure {
    pub state: Liveness,
    pub detail: String,
}

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

/// A coarse content kind, from the `content-type` header with a magic-byte fallback.
///
/// Deliberately coarse: acquisition should not hold opinions about formats. This exists
/// so `collect` can *report* what it gathered — knowing a run pulled 400 PDFs is what
/// makes the extraction stage plannable.
pub fn content_kind(meta: &BTreeMap<String, String>, bytes: &[u8]) -> &'static str {
    let declared = meta
        .get("content-type")
        .map(|s| {
            s.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        })
        .unwrap_or_default();

    match declared.as_str() {
        "text/html" | "application/xhtml+xml" => return "html",
        "application/pdf" => return "pdf",
        "text/plain" => return "text",
        "application/json" => return "json",
        "text/xml" | "application/xml" => return "xml",
        "text/csv" => return "csv",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        | "application/vnd.ms-excel" => return "spreadsheet",
        "application/msword"
        | "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            return "document";
        }
        _ => {}
    }

    // Hosts mislabel constantly — .gov servers routinely serve PDFs as
    // application/octet-stream. Magic bytes are the tiebreak.
    if bytes.starts_with(b"%PDF-") {
        return "pdf";
    }
    // ZIP magic: xlsx/docx are zip containers.
    if bytes.starts_with(b"PK\x03\x04") {
        return "zip-container";
    }
    let head = &bytes[..bytes.len().min(256)];
    let head = String::from_utf8_lossy(head)
        .trim_start()
        .to_ascii_lowercase();
    if head.starts_with("<!doctype html") || head.starts_with("<html") {
        return "html";
    }
    "other"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(ct: &str) -> BTreeMap<String, String> {
        BTreeMap::from([("content-type".to_string(), ct.to_string())])
    }

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

    #[test]
    fn content_type_header_is_used_when_present() {
        assert_eq!(content_kind(&meta("text/html; charset=utf-8"), b""), "html");
        assert_eq!(content_kind(&meta("application/pdf"), b""), "pdf");
        assert_eq!(
            content_kind(
                &meta("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
                b""
            ),
            "spreadsheet"
        );
    }

    #[test]
    fn magic_bytes_override_a_useless_content_type() {
        // .gov servers routinely serve PDFs as octet-stream.
        assert_eq!(
            content_kind(&meta("application/octet-stream"), b"%PDF-1.7\n..."),
            "pdf"
        );
        assert_eq!(
            content_kind(&meta("application/octet-stream"), b"<!DOCTYPE html><html>"),
            "html"
        );
        assert_eq!(
            content_kind(&meta("application/octet-stream"), b"PK\x03\x04junk"),
            "zip-container"
        );
    }

    #[test]
    fn unknown_content_is_labelled_other_not_guessed() {
        assert_eq!(content_kind(&BTreeMap::new(), b"\x00\x01\x02\x03"), "other");
    }

    #[test]
    fn content_kind_does_not_panic_on_short_bodies() {
        assert_eq!(content_kind(&BTreeMap::new(), b""), "other");
        assert_eq!(content_kind(&BTreeMap::new(), b"%"), "other");
    }
}
