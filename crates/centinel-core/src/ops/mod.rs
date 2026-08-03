//! The verbs.
//!
//! Each op is an ordinary async function wearing `#[op]`. Adding one here makes it
//! appear on the CLI, in `tools/list` over MCP, and at `POST /ops/<name>` — with no
//! edit to any central list. That absence is the whole point of the mechanism (#9).
//!
//! Ops are deliberately thin: argument validation, a call into [`crate::store`] or a
//! [`crate::domain::Source`], and a serializable result. Behaviour that deserves tests
//! belongs in the library, not in an op body.

mod doctor;
mod ingest;
mod list;

pub use doctor::{Binary, DoctorArgs, DoctorReport, doctor};
pub use ingest::{DEFAULT_USER_AGENT, IngestArgs, IngestOutcome, IngestReport, ingest};
pub use list::{ListArgs, ListReport, Problem, SourceSummary, list};
