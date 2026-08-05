//! The verbs.
//!
//! Each op is an ordinary async function wearing `#[op]`. Adding one here makes it
//! appear on the CLI, in `tools/list` over MCP, and at `POST /ops/<name>` — with no
//! edit to any central list. That absence is the whole point of the mechanism (#9).
//!
//! Ops are deliberately thin: argument validation, a call into [`crate::store`] or
//! [`crate::acquire`] against a [`crate::domain::Source`], and a serializable result.
//! Behaviour that deserves tests belongs in the library, not in an op body.
//!
//! ## No op knows a site from a channel
//!
//! `discover` and `collect` name what happens, not how. For a website they are a sitemap
//! walk and HTTP GETs; for a channel a playlist listing and `yt-dlp`. Which one you get is
//! decided once, by [`crate::sources::from_config`], from the `[[source]]` block — so
//! there is no `youtube` verb, and adding a third Source kind adds no verb either.

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
mod run;
mod search;
mod source;
mod target;
mod transcribe;

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
pub use run::{RunArgs, RunReport, SourceRun, Stage, StageRun, StageStatus, run};
pub use search::{AlsoAt, SearchArgs, SearchReport, SearchResult, search};
pub use source::{
    AddArgs, AdoptArgs, ConfiguredSource, RemoveArgs as SourceRemoveArgs, SourceAction, SourceArgs,
    SourceReport, source,
};
pub use transcribe::{
    TranscribeArgs, TranscribeFailure, TranscribeReport, TranscribedItem, transcribe,
};

// ── shared plumbing ───────────────────────────────────────────────────────────

use std::path::PathBuf;

use crate::config::Config;
use crate::domain::{Source, SourceId};
use crate::op::Ctx;
use crate::sources::{self, Overrides};

/// The config in effect, and the file it came from.
pub(crate) fn load_config(explicit: Option<&str>) -> anyhow::Result<(Config, Option<PathBuf>)> {
    match explicit {
        Some(p) => {
            let path = PathBuf::from(p);
            Ok((Config::from_file(&path)?, Some(path)))
        }
        None => Ok((Config::load()?, Config::locate())),
    }
}

/// The Source a `--source` argument names.
///
/// Three answers in order, and the order is the point:
///
/// 1. an explicit `--site`/`--channel` — someone typing an address is an instruction;
/// 2. the `[[source]]` block — the standing statement of intent;
/// 3. whatever the store remembers — evidence, for something collected by hand.
///
/// Nothing in this function knows what a site or a channel *is*; it decides which
/// description to hand [`sources::from_config`] and stops there.
pub(crate) async fn resolve_source(
    ctx: &Ctx,
    config: &Config,
    id: &str,
    site: Option<&str>,
    channel: Option<&str>,
    over: &Overrides,
) -> anyhow::Result<Box<dyn Source>> {
    let source_id = SourceId::new(id.to_string())?;

    if let (Some(_), Some(_)) = (site, channel) {
        anyhow::bail!("give --site or --channel, not both; a source is one or the other");
    }

    // 1. An address typed on the command line, merged onto the block's other settings so
    //    `--site` retypes the address and not the crawl rate.
    if site.is_some() || channel.is_some() {
        let mut cfg = config.source(id).cloned().unwrap_or_default();
        cfg.id = id.to_string();
        cfg.site = site.map(str::to_string);
        cfg.channel = channel.map(str::to_string);
        return sources::from_config(&cfg, &config.defaults, over);
    }

    // 2. The config.
    if let Some(cfg) = config.source(id) {
        return sources::from_config(cfg, &config.defaults, over);
    }

    // 3. The store.
    sources::from_store(&ctx.store, &source_id, &config.defaults, over).await
}
