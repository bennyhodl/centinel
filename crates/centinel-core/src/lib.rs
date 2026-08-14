//! Centinel's library core.
//!
//! A data-collection toolkit for `.gov` web surfaces and YouTube channels. The library
//! is the product; the CLI, the HTTP server and the MCP server are thin consumers of
//! the [`op`] registry (SPEC §1).
//!
//! ```text
//!   domain  — the nouns (§4).       Source is a trait; a Resource is an address.
//!   sources — the adapters (§4.1).  The only code that knows a site from a channel.
//!   acquire — discover and collect. One loop over any Source, whatever its kind.
//!   store   — files are truth (§5). CAS blob pool + append-only JSONL log.
//!   op      — the registry (#9).    One definition → CLI + MCP + HTTP.
//!   ops     — the verbs.            Individual operations, registered by #[op].
//!   render  — how a report reads.   The terminal's idiom for the same report.
//! ```
//!
//! ## What is not here yet
//!
//! Search and retrieval (SPEC §6) is built: both arms, RRF fusion and the reranker.
//! What is missing is **coverage** — a corpus is keyword-searchable as soon as it is
//! indexed and semantically searchable only after `embed`, which is hours. `search` says
//! which of the two a given answer came from rather than letting the difference pass.
//!
//! Seven decisions in SPEC §8 remain open; nothing in this crate should quietly assume
//! an answer to one.

// So the `#[op]` macro's `::centinel_core::…` paths resolve inside this crate too,
// exactly as they do for downstream users.
extern crate self as centinel_core;

pub mod acquire;
pub mod boilerplate;
pub mod captions;
pub mod chunk;
pub mod config;
pub mod content;
pub mod crumbs;
pub mod discovery;
pub mod domain;
pub mod embed;
pub mod enclosure;
pub mod extract;
pub mod fetch;
pub mod html;
pub mod index;
pub mod journal;
pub mod materialize;
pub mod models;
pub mod op;
pub mod ops;
pub mod policy;
pub mod remote;
pub mod render;
pub mod rerank;
pub mod schedule;
pub mod sources;
pub mod store;
pub mod strategies;
pub mod tool;
pub mod transcribe;
pub mod vectors;
pub mod verdict;
pub mod version;
pub mod youtube;

/// The `#[op]` attribute. Lives in the macro namespace, so it does not collide with
/// the [`op`] module in the type namespace.
pub use centinel_macros::op;

/// The imports an op definition needs.
pub mod prelude {
    pub use crate::discovery::DiscoveryLimits;
    pub use crate::domain::{
        Acquired, Anchor, BlobSha, ChangeEvent, ChangeKind, ChangeSignal, Derivation, DiscoveryRun,
        Enumeration, Fetched, Fingerprint, Liveness, ModelTier, Note, NoteMark, Observation,
        Refusal, Resource, ResourceStatus, Source, SourceId, SourceKind,
    };
    pub use crate::op::{Cancel, Ctx, Progress, ProgressEvent, Unit};
    pub use crate::policy::{HostPolicy, PolicyTable};
    pub use crate::render::{self, Align, Cell, Ink, Mark, Painter, Render, Table};
    pub use crate::store::{LogRecord, Store};
    pub use centinel_macros::op;
}
