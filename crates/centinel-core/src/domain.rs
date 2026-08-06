//! The domain model, per SPEC §4.
//!
//! Two ideas carry most of the weight here, and both are deliberate:
//!
//! 1. [`Source`] is a **trait**, not an entity with a `kind` field. Acquisition varies;
//!    nothing downstream of it does. Variation stays quarantined at the edge (§4.1).
//! 2. A [`Resource`] is an **address**, not a thing in the world. The same meeting reachable
//!    four ways is four Resources, and the model makes no claim they are related (§4.2).

use std::collections::BTreeMap;
use std::fmt;

use futures::future::BoxFuture;
use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stable identifier for a configured Source, e.g. `hillsboroughcounty`.
///
/// Used as a directory name in the log and `current/` trees, so it is constrained
/// to characters that are safe on every filesystem Centinel targets.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceId(String);

impl SourceId {
    /// Rejects anything that would escape a directory or collide case-insensitively.
    pub fn new(raw: impl Into<String>) -> Result<Self, InvalidSourceId> {
        let raw = raw.into();
        if raw.is_empty() || raw.len() > 64 {
            return Err(InvalidSourceId(raw));
        }
        if !raw
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(InvalidSourceId(raw));
        }
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid source id `{0}`: expected 1-64 chars of [a-z0-9_-] (it becomes a directory name)")]
pub struct InvalidSourceId(String);

/// The SHA-256 of a blob's **raw bytes** — archive identity (§5.3).
///
/// Distinct from [`Fingerprint`] on purpose: this one proves what the server actually served.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BlobSha(String);

impl BlobSha {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        Self(hex::encode(Sha256::digest(bytes)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Reconstructs from a stored hex string. Round-trips with [`Self::as_str`].
    pub fn from_hex(hex: impl Into<String>) -> Result<Self, MalformedHash> {
        let hex = hex.into();
        if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(MalformedHash(hex));
        }
        Ok(Self(hex.to_ascii_lowercase()))
    }

    /// Whether these are the bytes of nothing at all.
    ///
    /// A [`Derivation`] always has bytes — "we tried and there was nothing to get" is an
    /// [`Underivable`], which is a different record for a reason. So this address appearing
    /// as a Derivation's `to_sha` is not a derivation; it is a verdict filed under the
    /// wrong name, and every predicate that reads "a Derivation exists" has to say so.
    ///
    /// A constant would be a second copy of `sha2`'s answer, so it is computed once from
    /// the same function everything else uses.
    pub fn is_of_nothing(&self) -> bool {
        static NOTHING: std::sync::LazyLock<BlobSha> =
            std::sync::LazyLock::new(|| BlobSha::from_bytes(&[]));
        self == &*NOTHING
    }
}

impl fmt::Display for BlobSha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The hash of **normalized** content — the change signal (§5.3).
///
/// A page whose only variation is a rotated CSRF token yields a new [`BlobSha`] but an
/// unchanged `Fingerprint`: archived faithfully, no [`ChangeEvent`].
///
/// The normalization rules themselves are **not settled** — they belong to change
/// detection (ticket #7). [`normalize_placeholder`] is a deliberately dumb stand-in.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Fingerprint(String);

impl Fingerprint {
    pub fn from_normalized(normalized: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        Self(hex::encode(Sha256::digest(normalized)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn from_hex(hex: impl Into<String>) -> Result<Self, MalformedHash> {
        let hex = hex.into();
        if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(MalformedHash(hex));
        }
        Ok(Self(hex.to_ascii_lowercase()))
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("malformed hash `{0}`: expected 64 hex characters")]
pub struct MalformedHash(String);

/// Placeholder normalization: collapse ASCII whitespace runs and trim.
///
/// **This is not the real rule set.** Ticket #7 owns that (CSRF tokens, "last updated"
/// stamps, session ids, nonces). Until then this is honest about being naive rather
/// than pretending to a sophistication it does not have.
pub fn normalize_placeholder(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut in_space = false;
    for &b in bytes {
        if b.is_ascii_whitespace() {
            in_space = true;
        } else {
            if in_space && !out.is_empty() {
                out.push(b' ');
            }
            in_space = false;
            out.push(b);
        }
    }
    out
}

/// An **address** at which bytes were reachable — not a thing in the world (§4.2).
///
/// `natural_key` is whatever the Source uses to address the resource: a URL for a
/// crawled site, a vendor GUID for an API, a video id for YouTube.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Resource {
    pub source: SourceId,
    pub natural_key: String,
}

impl Resource {
    pub fn new(source: SourceId, natural_key: impl Into<String>) -> Self {
        Self {
            source,
            natural_key: natural_key.into(),
        }
    }
}

impl fmt::Display for Resource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.source, self.natural_key)
    }
}

/// Liveness of a Resource. Failures mutate this **in place** rather than appending
/// Observations, because an Observation always has bytes (§4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Liveness {
    /// Fetched successfully on the last attempt.
    Live,
    /// Authoritatively absent — 404/410.
    Gone,
    /// Refused in a way that is *not* evidence of absence: WAF 403, 429, robots denial.
    ///
    /// This variant exists because a CloudFront/Akamai 403 would otherwise be
    /// indistinguishable from "the site didn't change" — measured live on `phila.gov`
    /// and `sec.gov` (§4.4). Treating it as `Gone` would silently corrupt the record.
    Blocked,
    /// Transport or server fault — timeout, 5xx, TLS failure.
    Error,
}

impl fmt::Display for Liveness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Live => "live",
            Self::Gone => "gone",
            Self::Blocked => "blocked",
            Self::Error => "error",
        };
        f.pad(s)
    }
}

impl Liveness {
    /// The glyph this state reads as.
    ///
    /// `Blocked` is amber rather than red on purpose, and it is the whole reason this
    /// mapping lives beside the enum instead of in a renderer. Painting a refusal the
    /// same colour as a 404 would undo, at the last possible moment, the distinction the
    /// model spends §4.4 protecting: a page that refuses you is not a page that is gone.
    pub fn mark(&self) -> crate::render::Mark {
        use crate::render::Mark;
        match self {
            Self::Live => Mark::Ok,
            Self::Blocked => Mark::Warn,
            Self::Gone | Self::Error => Mark::Bad,
        }
    }
}

/// Per-Resource liveness state (§4.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceStatus {
    pub resource: Resource,
    pub state: Liveness,
    /// When the Resource *entered* `state`. Unchanged while the state persists, so
    /// "blocked since" is answerable without scanning history.
    pub since: Timestamp,
    pub last_checked: Timestamp,
    pub consecutive_failures: u32,
    /// Last transport detail — HTTP status, error string. Diagnostic, not authoritative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ResourceStatus {
    pub fn new_live(resource: Resource, at: Timestamp) -> Self {
        Self {
            resource,
            state: Liveness::Live,
            since: at,
            last_checked: at,
            consecutive_failures: 0,
            detail: None,
        }
    }

    /// Applies an outcome, preserving `since` when the state is unchanged.
    pub fn apply(&mut self, state: Liveness, at: Timestamp, detail: Option<String>) {
        if self.state != state {
            self.state = state;
            self.since = at;
        }
        self.last_checked = at;
        self.consecutive_failures = if state == Liveness::Live {
            0
        } else {
            self.consecutive_failures.saturating_add(1)
        };
        self.detail = detail;
    }
}

/// One **successful** fetch, always backed by a Blob (§4.4).
///
/// There is no failure variant by construction. That is the point.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub resource: Resource,
    /// Raw bytes as served — evidentiary identity.
    pub blob_sha: BlobSha,
    /// Normalized content — the change signal.
    pub fingerprint: Fingerprint,
    pub at: Timestamp,
    /// Transport metadata worth keeping: content-type, etag, vendor last-modified.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, String>,
}

/// A full snapshot of the Resource set a discovery pass observed (§4.3).
///
/// A **sitemap is one of these**, not a separate entity. Resources appearing and
/// vanishing between runs is the discovery delta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryRun {
    pub source: SourceId,
    pub at: Timestamp,
    pub resources: Vec<Resource>,
    /// How discovery was performed — `sitemap`, `odata`, `playlist`. Provenance for
    /// interpreting a suspiciously small snapshot.
    pub method: String,
}

/// Where a derived artifact sits inside its parent (§4.3).
///
/// Anchors vary **within** the Derivation rather than across entity types, which is
/// what lets one re-derivation path serve PDFs, audio, and HTML alike.
///
/// Not `Eq`: PDF bounding boxes are floats. Compare anchors structurally with
/// `PartialEq`, and never use one as a map key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Anchor {
    /// PDF page region. `bbox` is `[x0, y0, x1, y1]` in PDF user space.
    PdfRegion {
        page: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bbox: Option<[f32; 4]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        char_span: Option<(usize, usize)>,
    },
    /// Audio/video time range, milliseconds from start.
    TimeRange { start_ms: u64, end_ms: u64 },
    /// Character span in a text derivation.
    CharSpan { start: usize, end: usize },
}

/// Which local model tier produced an artifact.
///
/// Part of provenance because **output quality varies by machine** (SPEC §2.1): the
/// same audio on a laptop and a workstation yields different transcripts. Recording
/// the tier is what makes "the source changed" mechanically distinguishable from
/// "this ran on a weaker machine" (§4.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelTier {
    /// e.g. `whisper-large-v3`, `qwen3-embedding-0.6b`.
    pub model_id: String,
    /// Quantization or variant, e.g. `q5_k_m`, `f16`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// A `Blob → Blob` edge: HTML→markdown, PDF→text, scanned→OCR, audio→transcript (§4.3).
///
/// Carrying `tool`, `version` **and** `model_tier` is what makes phantom diffs solvable.
/// The *policy* for what to do about them belongs to change detection (#7).
///
/// Not `Eq`, because [`Anchor`] is not.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Derivation {
    pub from_sha: BlobSha,
    pub to_sha: BlobSha,
    /// e.g. `pdf-inspector`, `tesseract`, `whisper-rs`.
    pub tool: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_tier: Option<ModelTier>,
    pub at: Timestamp,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<Anchor>,
}

/// A derivation that was **attempted and produced nothing**.
///
/// The peer of [`ResourceStatus`] on the derivation side, and it exists for the same
/// reason. A [`Derivation`] is a `Blob → Blob` edge and therefore always has bytes,
/// exactly as an [`Observation`] always does (§4.4) — so "we tried, and there was nothing
/// to get" has nowhere to live unless something like this holds it.
///
/// Without it, the skip predicate for extraction can only ever be *"a Derivation exists"*,
/// which is never true for a blob nothing can extract. So every audio file in a corpus was
/// read, hashed and re-attempted on every single run, forever, to reach the same
/// conclusion.
///
/// `tool` and `version` are carried for the same reason a Derivation carries them (§4.6):
/// this records that **one pipeline at one version** made nothing of these bytes, which
/// is a permanent fact about that version and no claim at all about the next one. Bumping
/// the version is how a better extractor gets another go.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Underivable {
    pub from_sha: BlobSha,
    pub tool: String,
    pub version: String,
    /// Why, in the pipeline's own words — `no extractor for kind audio`.
    pub reason: String,
    pub at: Timestamp,
}

/// What changed between two Observations of the same Resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// First Observation of this Resource.
    Appeared,
    /// Fingerprint differs from the previous Observation.
    Modified,
    /// Present in an earlier DiscoveryRun, absent from the latest.
    Vanished,
}

/// A materialized, **rebuildable** index over Observations (§4.5).
///
/// Truth is `obs[n-1].fingerprint != obs[n].fingerprint`; this table exists so search
/// can retrieve *over changes*. VersionRAG measured naive RAG at 0% on "what was
/// removed" queries unless change is an indexed object — hence materializing it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeEvent {
    pub resource: Resource,
    pub kind: ChangeKind,
    pub at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_fingerprint: Option<Fingerprint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_fingerprint: Option<Fingerprint>,
}

/// What a Source reports as its change signal — asserted or computed (§4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChangeSignal {
    /// The vendor told us, e.g. Legistar's `MatterLastModifiedUtc`. Cheap and precise,
    /// but **asserted** — trusting it is a decision owned by #7.
    Asserted { last_modified: Timestamp },
    /// We computed it by hashing normalized content. Always available, never wrong,
    /// but requires actually fetching the bytes.
    Computed { fingerprint: Fingerprint },
    /// The Source cannot say. Caller must fetch to find out.
    Unknown,
}

/// Bytes fetched from a Resource, before they become a [`Blob`].
#[derive(Clone, Debug)]
pub struct Fetched {
    pub bytes: Vec<u8>,
    pub meta: BTreeMap<String, String>,
}

/// An acquisition that failed, already classified.
///
/// Carries [`Liveness`] rather than an error type, because the caller's job is to record
/// *what kind* of failure this was, not to propagate it. A WAF 403 and a 404 are the same
/// `Result::Err` and completely different facts.
///
/// One type for every acquisition path on purpose. HTTP and `yt-dlp` arrived at this
/// shape independently — `{state, detail}`, each with its own `classify` — and having two
/// names for it meant the shared loop below could not exist.
#[derive(Clone, Debug)]
pub struct Refusal {
    pub state: Liveness,
    pub detail: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.detail, self.state)
    }
}

impl std::error::Error for Refusal {}

/// One artifact acquired from a Resource, at **its own address**.
///
/// A page is one of these. A video is up to three — metadata, captions, audio — because
/// those are three addresses that change independently, not three fields of one thing
/// (§4.2). This is the shape that lets one acquisition loop serve both: it never learns
/// how many artifacts an address holds, only what came back.
#[derive(Clone, Debug)]
pub struct Acquired {
    /// Where these bytes were served. Equal to the Resource for a single-artifact
    /// address; a sub-resource of it otherwise.
    pub resource: Resource,
    pub fetched: Fetched,
}

/// A line of provenance a Source wants shown, and how it should read.
///
/// Exists so a report can print what only the Source knows — which sitemaps were walked,
/// which channel tabs returned nothing — without the report learning what a sitemap or a
/// tab is. A third Source kind explains itself through this and edits no renderer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Note {
    /// The short left-hand label: `sitemaps`, `/streams`, `robots.txt`.
    pub label: String,
    pub detail: String,
    /// How it reads. `Warn` is for a fact that is not an error but would explain a
    /// wrong-looking answer — a tab that returned nothing, rules that were assumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<NoteMark>,
}

/// [`Note`]'s severity, as a serializable peer of [`crate::render::Mark`].
///
/// Separate from `Mark` because a Note crosses the wire to MCP and HTTP, and `Mark` is a
/// painting decision that does not serialize.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoteMark {
    Ok,
    Warn,
    Bad,
}

impl Note {
    pub fn new(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            detail: detail.into(),
            mark: None,
        }
    }

    pub fn marked(label: impl Into<String>, detail: impl Into<String>, mark: NoteMark) -> Self {
        Self {
            label: label.into(),
            detail: detail.into(),
            mark: Some(mark),
        }
    }

    /// `Ok` when the fact is unremarkable, `Warn` when it would explain a small answer.
    pub fn ok_or_warn(label: impl Into<String>, detail: impl Into<String>, ok: bool) -> Self {
        Self::marked(
            label,
            detail,
            if ok { NoteMark::Ok } else { NoteMark::Warn },
        )
    }
}

impl NoteMark {
    pub fn mark(self) -> crate::render::Mark {
        use crate::render::Mark;
        match self {
            Self::Ok => Mark::Ok,
            Self::Warn => Mark::Warn,
            Self::Bad => Mark::Bad,
        }
    }
}

/// What one enumeration pass found, plus everything that would explain a wrong count.
///
/// A snapshot is trusted or it is not, and the fields that decide that are all negative
/// space — what was assumed rather than read, what was refused, what was skipped. Those
/// travel with the resources rather than beside them so a caller cannot report the count
/// without having been handed the caveats.
#[derive(Clone, Debug, Default)]
pub struct Enumeration {
    /// The complete set this pass observed. A snapshot, never a delta (§4.3).
    pub resources: Vec<Resource>,
    /// Non-fatal problems. A partial enumeration with recorded warnings is far more
    /// useful than a hard failure, so nothing here aborts a pass.
    pub warnings: Vec<String>,
    /// Provenance worth showing a person.
    pub notes: Vec<Note>,
    /// The same provenance for a machine, named as the Source names it.
    pub figures: BTreeMap<String, u64>,
}

/// How a Source is acquired, as it appears in reports and on the wire.
///
/// A closed enum because it is a *label*: the open axis is [`Source::method`], which is a
/// string precisely so a new discovery technique needs no variant here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Site,
    Channel,
}

impl SourceKind {
    /// The single spelling of these words. Previously written out at four call sites,
    /// where a fifth kind would have had to find all four.
    pub fn label(self) -> &'static str {
        match self {
            Self::Site => "site",
            Self::Channel => "channel",
        }
    }
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.label())
    }
}

/// Acquisition. The **only** place the Source kinds differ (§4.1).
///
/// Dyn-compatible on purpose — Sources are constructed from config at runtime, so callers
/// hold `Box<dyn Source>`. That is why these return [`BoxFuture`] rather than `async fn`.
///
/// ## Why `acquire` yields many artifacts
///
/// An earlier shape had `fetch(&Resource) -> Fetched`: one address, one blob. Nothing
/// could implement it. A YouTube video is one address holding up to three artifacts, and
/// whether the third is fetched depends on whether the second came back — so the kinds
/// went around the trait rather than through it, and their shared machinery got written
/// twice. Returning [`Acquired`] is what makes both expressible, and therefore what makes
/// [`crate::acquire`] able to exist.
///
/// ## The pacing is inside
///
/// Politeness is not on this interface. A crawled site paces per host; a channel waits
/// between videos against a bot wall. Both are properties of *how* the Source acquires,
/// so both live behind [`Self::acquire`] rather than being a dial the caller has to know
/// to turn.
pub trait Source: Send + Sync {
    fn id(&self) -> &SourceId;

    /// The label this Source reads as.
    fn kind(&self) -> SourceKind;

    /// How this Source enumerates — `sitemap`, `odata`, `playlist`. Recorded on the
    /// [`DiscoveryRun`] as provenance, and the discriminator that later recovers a
    /// Source's kind from the store alone.
    fn method(&self) -> &'static str;

    /// The address this Source was configured from — a site origin, a channel URL.
    fn target(&self) -> &str;

    /// Whether acquiring this Source can produce audio.
    ///
    /// Asked once, before a run, to decide whether the corpus-wide transcribe stage is
    /// worth counting into the progress total. Cheaper than discovering it afterwards
    /// from the store, and honest about being a declaration rather than a measurement.
    fn yields_audio(&self) -> bool {
        false
    }

    /// Enumerate the full Resource set: a sitemap crawl, a paged OData query, a playlist
    /// listing. Returns a complete snapshot with its own caveats attached.
    fn enumerate<'a>(
        &'a self,
        progress: &'a crate::op::Progress,
    ) -> BoxFuture<'a, anyhow::Result<Enumeration>>;

    /// Acquire everything one address holds.
    ///
    /// An empty `Vec` is a legitimate success meaning "nothing to store here"; a
    /// [`Refusal`] is the address declining to be read, which is what liveness records.
    fn acquire<'a>(
        &'a self,
        resource: &'a Resource,
        progress: &'a crate::op::Progress,
    ) -> BoxFuture<'a, Result<Vec<Acquired>, Refusal>>;

    /// The address whose presence in the log proves this Resource was acquired.
    ///
    /// Defaults to the Resource itself. A channel overrides it, because a video is
    /// "fetched" when its metadata is stored — captions and audio are separate addresses
    /// that may legitimately be absent, and keying resumption on them would re-fetch the
    /// whole catalogue every run.
    fn marker(&self, resource: &Resource) -> Resource {
        resource.clone()
    }

    /// Anything worth saying about a finished collection that only this Source knows.
    ///
    /// The counterpart to [`Enumeration::notes`] on the acquisition side: a channel says
    /// how many videos it found no captions for, without `collect` learning what a
    /// caption is.
    fn remarks(&self, _stored_parts: &BTreeMap<String, usize>, _attempted: usize) -> Vec<Note> {
        Vec::new()
    }

    /// Ask whether a Resource changed **without** fetching it, when the Source can.
    ///
    /// Default is [`ChangeSignal::Unknown`]: correct for crawled sites, overridden by
    /// [`ChangeSignal::Asserted`] for vendor APIs that expose a `LastModifiedUtc`.
    fn change_signal<'a>(
        &'a self,
        _resource: &'a Resource,
    ) -> BoxFuture<'a, anyhow::Result<ChangeSignal>> {
        Box::pin(async { Ok(ChangeSignal::Unknown) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_id_rejects_path_traversal_and_case() {
        assert!(SourceId::new("hillsboroughcounty").is_ok());
        assert!(SourceId::new("tampa-fl_2").is_ok());
        assert!(SourceId::new("../etc").is_err());
        assert!(
            SourceId::new("Tampa").is_err(),
            "uppercase would collide on case-insensitive filesystems"
        );
        assert!(SourceId::new("").is_err());
    }

    #[test]
    fn hashes_round_trip_through_hex() {
        let sha = BlobSha::from_bytes(b"hello");
        assert_eq!(BlobSha::from_hex(sha.as_str()).unwrap(), sha);
        assert!(BlobSha::from_hex("nope").is_err());
    }

    /// The §5.3 property: cosmetic variation must move `blob_sha` but not `fingerprint`.
    #[test]
    fn whitespace_variation_changes_blob_sha_but_not_fingerprint() {
        let a = b"<p>Council   meeting</p>\n";
        let b = b"<p>Council meeting</p>";

        assert_ne!(BlobSha::from_bytes(a), BlobSha::from_bytes(b));
        assert_eq!(
            Fingerprint::from_normalized(&normalize_placeholder(a)),
            Fingerprint::from_normalized(&normalize_placeholder(b)),
        );
    }

    #[test]
    fn status_preserves_since_across_repeated_same_state() {
        let r = Resource::new(SourceId::new("x").unwrap(), "https://x/1");
        let t0: Timestamp = "2026-01-01T00:00:00Z".parse().unwrap();
        let t1: Timestamp = "2026-01-02T00:00:00Z".parse().unwrap();
        let t2: Timestamp = "2026-01-03T00:00:00Z".parse().unwrap();

        let mut s = ResourceStatus::new_live(r, t0);
        s.apply(Liveness::Blocked, t1, Some("403".into()));
        assert_eq!(s.since, t1);
        assert_eq!(s.consecutive_failures, 1);

        s.apply(Liveness::Blocked, t2, Some("403".into()));
        assert_eq!(s.since, t1, "still blocked since t1, not t2");
        assert_eq!(s.consecutive_failures, 2);

        s.apply(Liveness::Live, t2, None);
        assert_eq!(s.since, t2);
        assert_eq!(s.consecutive_failures, 0);
    }
}
