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
}
