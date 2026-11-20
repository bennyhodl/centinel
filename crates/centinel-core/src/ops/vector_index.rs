//! `vector-index`: build the vector index `[vectors]` names.
//!
//! The one place an index is built. A build reads every vector and trains on as many as
//! the memory budget holds, which on a large table is long, so it is never a side effect:
//! `embed` keeps an index there is up to date, and says when the table has none or has
//! another than `[vectors]` names, and this is what it says to run. Everything this op knows
//! about Lance it knows through [`crate::vectors::VectorTable`]; it takes the table's lock,
//! asks for a rebuild, and reports what it did.

use std::path::PathBuf;
use std::time::Instant;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::VectorIndexKind;
use crate::prelude::*;
use crate::vectors::{ANN_MIN_ROWS, Indexing, VectorLock, VectorTable};

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct VectorIndexArgs {}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct VectorIndexReport {
    /// `chunks.lance/`.
    pub vectors: PathBuf,
    pub rows: usize,
    /// The index `[vectors]` names: `IVF_PQ`, `IVF_FLAT`, or `no index`.
    pub configured: String,
    /// The index this run built, when it built one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built: Option<String>,
    /// What the table has now, in a sentence.
    pub outcome: String,
    pub elapsed_secs: f64,
}

/// Build the vector index `[vectors]` names, replacing the one there, or drop it for `none`.
///
/// `reach = "operator"`: a build reads every vector and holds the table's lock while it
/// trains, so appends wait behind it. When to pay for that is the operator's call.
#[op(long_running, reach = "operator", group = "stage")]
pub async fn vector_index(
    ctx: &Ctx,
    _args: VectorIndexArgs,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<VectorIndexReport> {
    cancel.check()?;
    let config = crate::config::Config::load()?.vectors;
    let table = VectorTable::open_existing(&ctx.store.vectors_db())
        .await?
        .configured(&config);
    progress.say(match config.index {
        VectorIndexKind::None => "dropping the vector index".to_string(),
        kind => format!("building the {kind} vector index; it reads every vector"),
    });
    let started = Instant::now();
    let lock = VectorLock::take(&ctx.store).await?;
    let done = table.maintain(&lock, Indexing::Rebuild).await?;
    drop(lock);
    let rows = table.len().await?;
    let outcome = match (done.index_built, config.index) {
        (Some(kind), _) => format!("{kind} over {rows} vectors"),
        (None, VectorIndexKind::None) => {
            "no index: every search compares with every vector".to_string()
        }
        (None, kind) => format!(
            "no {kind} index: {rows} vectors is under the {ANN_MIN_ROWS} an index is worth, \
             and comparing with every one is fast at that size"
        ),
    };
    Ok(VectorIndexReport {
        vectors: ctx.store.vectors_path(),
        rows,
        configured: config.index.to_string(),
        built: done.index_built.map(|kind| kind.to_string()),
        outcome,
        elapsed_secs: started.elapsed().as_secs_f64(),
    })
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

impl Render for VectorIndexReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.title("vector index", &self.vectors.display().to_string())?;
        p.nest(|p| {
            let mark = if self.built.is_some() {
                Mark::Ok
            } else {
                Mark::Warn
            };
            p.marked(mark, &self.outcome)?;
            p.line(p.paint(&render::duration(self.elapsed_secs), Ink::Dim))
        })
    }
}
