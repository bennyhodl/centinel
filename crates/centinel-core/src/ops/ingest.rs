//! `ingest` — fetch addresses into the store.
//!
//! This op is where three separate spec decisions become visible at once:
//!
//! - **An Observation always has bytes** (§4.4). A failed fetch appends no Observation;
//!   it mutates [`ResourceStatus`] instead.
//! - **`Blocked` is not `Gone`** (§4.4). A CloudFront/Akamai 403 is not evidence of
//!   absence, and conflating them would silently corrupt the record.
//! - **Two hashes** (§5.3). `blob_sha` proves what the server served; `fingerprint`
//!   answers whether it meaningfully changed.

use std::collections::BTreeMap;
use std::time::Duration;

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::prelude::*;
use crate::store::LogRecord;

/// A descriptive User-Agent with a contact address.
///
/// Not cosmetic. The crawling research measured `sec.gov` returning 403 to a default
/// agent and 200 to a descriptive one — the single highest-yield politeness lever found.
/// The real per-host policy table (UA, rate cap, contact) is owned by ticket #4; this is
/// the default until that exists.
pub const DEFAULT_USER_AGENT: &str =
    "Centinel/0.1 (civic transparency archiver; +https://github.com/bennyhodl/centinel)";

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct IngestArgs {
    /// Source id to file these observations under, e.g. `hillsboroughcounty`.
    #[arg(long)]
    pub source: String,

    /// URL to fetch. Repeat for several.
    #[arg(long = "url", required = true, num_args = 1..)]
    pub urls: Vec<String>,

    /// User-Agent header. A descriptive one measurably reduces WAF 403s.
    #[arg(long, default_value = DEFAULT_USER_AGENT)]
    #[serde(default = "default_ua")]
    pub user_agent: String,

    /// Per-request timeout in seconds.
    #[arg(long, default_value_t = 30)]
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_ua() -> String {
    DEFAULT_USER_AGENT.to_string()
}

fn default_timeout() -> u64 {
    30
}

/// What happened at one address.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum IngestOutcome {
    /// Bytes retrieved and stored.
    Stored {
        url: String,
        blob_sha: String,
        fingerprint: String,
        bytes: usize,
        /// `false` when the fingerprint matched the previous Observation — archived
        /// faithfully, but not a change (§5.3).
        changed: bool,
        /// True when this address had never been observed before.
        first_seen: bool,
    },
    /// Fetch failed. No Observation was written; liveness was updated instead.
    Failed {
        url: String,
        state: Liveness,
        detail: String,
        consecutive_failures: u32,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct IngestReport {
    pub source: String,
    pub attempted: usize,
    pub stored: usize,
    pub changed: usize,
    pub failed: usize,
    pub outcomes: Vec<IngestOutcome>,
}

/// Fetch one or more URLs into the content-addressed store.
#[op(long_running)]
pub async fn ingest(
    ctx: &Ctx,
    args: IngestArgs,
    progress: &Progress,
) -> anyhow::Result<IngestReport> {
    let source = SourceId::new(args.source.clone())?;

    let client = reqwest::Client::builder()
        .user_agent(&args.user_agent)
        .timeout(Duration::from_secs(args.timeout_secs))
        .build()?;

    // Liveness is replayed once up front so `since` and `consecutive_failures` carry
    // across runs rather than resetting on every invocation.
    let mut statuses = ctx.store.statuses(&source).await?;

    let total = args.urls.len() as u64;
    let mut outcomes = Vec::with_capacity(args.urls.len());
    let (mut stored, mut changed, mut failed) = (0usize, 0usize, 0usize);

    for (i, url) in args.urls.iter().enumerate() {
        progress.step(format!("fetching {url}"), i as u64, total);

        let resource = Resource::new(source.clone(), url.clone());
        let at = Timestamp::now();

        match fetch(&client, url).await {
            Ok(FetchOk { bytes, meta }) => {
                let n = bytes.len();
                let (obs, previous) = ctx.store.observe(&resource, &bytes, at, meta).await?;

                let first_seen = previous.is_none();
                let did_change = previous.as_ref() != Some(&obs.fingerprint);

                // A success clears whatever failure state the address was in.
                statuses
                    .entry(resource.clone())
                    .and_modify(|s| s.apply(Liveness::Live, at, None))
                    .or_insert_with(|| ResourceStatus::new_live(resource.clone(), at));

                stored += 1;
                if did_change {
                    changed += 1;
                }
                outcomes.push(IngestOutcome::Stored {
                    url: url.clone(),
                    blob_sha: obs.blob_sha.to_string(),
                    fingerprint: obs.fingerprint.to_string(),
                    bytes: n,
                    changed: did_change,
                    first_seen,
                });
            }
            Err(FetchErr { state, detail }) => {
                let entry = statuses
                    .entry(resource.clone())
                    .or_insert_with(|| ResourceStatus::new_live(resource.clone(), at));
                entry.apply(state, at, Some(detail.clone()));
                let consecutive_failures = entry.consecutive_failures;

                // No Observation — liveness carries the failure instead (§4.4).
                ctx.store
                    .append(&source, &LogRecord::Status(entry.clone()))
                    .await?;

                failed += 1;
                outcomes.push(IngestOutcome::Failed {
                    url: url.clone(),
                    state,
                    detail,
                    consecutive_failures,
                });
            }
        }
    }

    progress.step("done", total, total);

    Ok(IngestReport {
        source: args.source,
        attempted: args.urls.len(),
        stored,
        changed,
        failed,
        outcomes,
    })
}

struct FetchOk {
    bytes: Vec<u8>,
    meta: BTreeMap<String, String>,
}

struct FetchErr {
    state: Liveness,
    detail: String,
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<FetchOk, FetchErr> {
    let resp = client.get(url).send().await.map_err(|e| FetchErr {
        state: Liveness::Error,
        detail: e.to_string(),
    })?;

    let status = resp.status();
    if !status.is_success() {
        return Err(FetchErr {
            state: classify(status.as_u16()),
            detail: format!("HTTP {status}"),
        });
    }

    // Kept on the Observation because they are the cheap change signals a later
    // conditional-request implementation (#7) will want, and they cannot be recovered
    // after the fact.
    let mut meta = BTreeMap::new();
    for header in ["content-type", "etag", "last-modified"] {
        if let Some(v) = resp.headers().get(header)
            && let Ok(s) = v.to_str()
        {
            meta.insert(header.to_string(), s.to_string());
        }
    }
    meta.insert("http_status".into(), status.as_u16().to_string());
    // The post-redirect URL, which is what the bytes actually came from.
    meta.insert("final_url".into(), resp.url().to_string());

    let bytes = resp.bytes().await.map_err(|e| FetchErr {
        state: Liveness::Error,
        detail: format!("body read failed: {e}"),
    })?;

    Ok(FetchOk {
        bytes: bytes.to_vec(),
        meta,
    })
}

/// Maps an HTTP status onto liveness.
///
/// The 403 → [`Liveness::Blocked`] mapping is the load-bearing one. Both `phila.gov`
/// and `sec.gov` were measured returning WAF 403s with no `Retry-After`; classifying
/// those as `Gone` would record a live page as deleted.
fn classify(status: u16) -> Liveness {
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
        // The distinction the whole discovery-delta story rests on.
        assert_eq!(classify(403), Liveness::Blocked);
        assert_eq!(classify(429), Liveness::Blocked);
        assert_eq!(classify(404), Liveness::Gone);
        assert_eq!(classify(410), Liveness::Gone);
        assert_eq!(classify(500), Liveness::Error);
        assert_eq!(classify(503), Liveness::Error);
    }

    #[test]
    fn default_user_agent_is_descriptive_and_contactable() {
        assert!(DEFAULT_USER_AGENT.contains("Centinel"));
        assert!(
            DEFAULT_USER_AGENT.contains('+'),
            "a contact URL is what flips sec.gov from 403 to 200"
        );
    }
}
