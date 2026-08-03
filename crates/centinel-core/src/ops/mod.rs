//! The verbs.
//!
//! Each op is an ordinary async function wearing `#[op]`. Adding one here makes it
//! appear on the CLI, in `tools/list` over MCP, and at `POST /ops/<name>` — with no
//! edit to any central list. That absence is the whole point of the mechanism (#9).
//!
//! Ops are deliberately thin: argument validation, a call into [`crate::store`] or a
//! [`crate::domain::Source`], and a serializable result. Behaviour that deserves tests
//! belongs in the library, not in an op body.

mod build_index;
mod collect;
mod discover;
mod doctor;
mod embed;
mod extract;
mod ingest;
mod list;
mod models;
mod open;
mod read;
mod search;
mod transcribe;
mod youtube;

pub use models::{
    FetchedFile, FileCheck, ModelsAction, ModelsArgs, ModelsReport, Orphan, PruneArgs, PullArgs,
    RemoveArgs, VerifyArgs, models,
};
pub use open::{OpenArgs, OpenReport, open};
pub use read::{ReadArgs, ReadReport, read};

pub use build_index::{IndexArgs, IndexReport, index};
pub use collect::{CollectArgs, CollectFailure, CollectReport, collect};
pub use discover::{DiscoverArgs, DiscoverReport, discover};
pub use doctor::{Binary, DoctorArgs, DoctorReport, GateStatus, Weights, doctor};
pub use embed::{EmbedArgs, EmbedReport, Skipped, embed};
pub use extract::{ExtractArgs, ExtractReport, ExtractSample, Unreadable, extract};
pub use ingest::{IngestArgs, IngestOutcome, IngestReport, ingest};
pub use list::{ListArgs, ListReport, Problem, SourceSummary, list};
pub use search::{SearchArgs, SearchReport, SearchResult, search};
pub use transcribe::{
    TranscribeArgs, TranscribeFailure, TranscribeReport, TranscribedItem, transcribe,
};
pub use youtube::{
    DiscoverChannelArgs, FetchArgs, VideoOutcome, YoutubeAction, YoutubeArgs, YoutubeReport,
    youtube,
};
