//! Centinel's library core.
//!
//! A data-collection toolkit for `.gov` web surfaces and YouTube channels. The library
//! is the product; the CLI, the HTTP server and the MCP server are thin consumers of
//! the [`op`] registry (SPEC §1).
//!
//! ```text
//!   domain  — the nouns (§4).       Source is a trait; a Resource is an address.
//!   store   — files are truth (§5). CAS blob pool + append-only JSONL log.
//!   op      — the registry (#9).    One definition → CLI + MCP + HTTP.
//!   ops     — the verbs.            Individual operations, registered by #[op].
//! ```
//!
//! ## What is not here yet
//!
//! Search and retrieval (SPEC §6) is specified but unimplemented — it needs LanceDB and
//! the Qwen3 model pair, neither of which the spine requires. Seven decisions in SPEC §8
//! remain open; nothing in this crate should quietly assume an answer to one.

// So the `#[op]` macro's `::centinel_core::…` paths resolve inside this crate too,
// exactly as they do for downstream users.
extern crate self as centinel_core;

pub mod domain;
pub mod op;
pub mod ops;
pub mod store;

/// The `#[op]` attribute. Lives in the macro namespace, so it does not collide with
/// the [`op`] module in the type namespace.
pub use centinel_macros::op;

/// The imports an op definition needs.
pub mod prelude {
    pub use crate::domain::{
        Anchor, BlobSha, ChangeEvent, ChangeKind, ChangeSignal, Derivation, DiscoveryRun, Fetched,
        Fingerprint, Liveness, ModelTier, Observation, Resource, ResourceStatus, Source, SourceId,
    };
    pub use crate::op::{Ctx, Progress, ProgressEvent};
    pub use crate::store::{LogRecord, Store};
    pub use centinel_macros::op;
}
