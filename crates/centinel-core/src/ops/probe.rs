//! What `check --strategy` and `investigate` both do before they ask their own question.
//!
//! Two ops point a strategy at one address and report what came back. They ask genuinely
//! different things of it — `check` asks *what would this one document extract to*,
//! `investigate` asks *who recognises this host and how much is behind it* — and neither is
//! a superset of the other, so they stay two ops.
//!
//! Everything **around** the question was copied, though, and that is what lives here: the
//! ceiling, the two network flags, and the throwaway [`SiteSource`] both build. Duplicated,
//! the two drifted in the way duplicated constants always do — the sample cap was 5 in one
//! and 10 in the other for no reason anybody recorded — and the ceiling is the one number
//! where drift is not cosmetic, because a probe that stops early and a site that is small
//! look identical.

use crate::discovery::DiscoveryLimits;
use crate::domain::SourceId;
use crate::policy::{DEFAULT_USER_AGENT, HostPolicy};
use crate::sources::SiteSource;

/// Requests a probe may spend, and addresses it will keep.
///
/// Deliberately small, and small for the same reason in both ops: this is a question asked
/// while deciding whether a host is worth collecting, often about ten hosts in a row. A
/// walk that takes minutes is one nobody runs twice. Pointing it at a directory index would
/// otherwise walk the tree — `publicrec.hillsclerk.com` is ~1,500 files — to answer a
/// question about the first page.
///
/// A probe that fills its ceiling reports `truncated`, so a floor is never printed as a
/// total.
pub(super) const REQUESTS: usize = 25;
pub(super) const ADDRESSES: usize = 500;

/// The two flags any op that fetches on an operator's behalf has to offer.
///
/// Flattened into both `CheckArgs` and `InvestigateArgs` rather than spelled out in each.
/// They were byte-identical clap blocks with byte-identical `serde` default functions, and
/// a third op would have made three.
#[derive(Clone, Debug, clap::Args, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct NetArgs {
    /// User-Agent header. A descriptive one measurably reduces WAF 403s.
    #[arg(long, default_value = DEFAULT_USER_AGENT)]
    #[serde(default = "default_ua")]
    pub user_agent: String,

    /// Per-request timeout in seconds.
    #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS)]
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

const DEFAULT_TIMEOUT_SECS: u64 = 30;

fn default_ua() -> String {
    DEFAULT_USER_AGENT.to_string()
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECS
}

impl Default for NetArgs {
    fn default() -> Self {
        Self {
            user_agent: default_ua(),
            timeout_secs: default_timeout(),
        }
    }
}

impl NetArgs {
    pub(super) fn policy(&self) -> HostPolicy {
        HostPolicy {
            user_agent: self.user_agent.clone(),
            timeout: std::time::Duration::from_secs(self.timeout_secs),
            ..Default::default()
        }
    }
}

/// A throwaway [`SiteSource`] bounded by the probe ceiling.
///
/// `id` names the op in anything that reads a `SourceId` back, and nothing is written under
/// it — neither op touches a store.
pub(super) fn site(id: &str, url: &str, net: &NetArgs) -> anyhow::Result<SiteSource> {
    SiteSource::new(
        SourceId::new(id.to_string())?,
        url,
        net.policy(),
        DiscoveryLimits {
            max_sitemaps: REQUESTS,
            max_urls: ADDRESSES,
        },
    )
}
