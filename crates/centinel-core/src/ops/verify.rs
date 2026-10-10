//! `verify`: read the vector table back, and drop what cannot be read.
//!
//! Storage that silently drops a write leaves a Lance data file of the right length with
//! nothing in it, and every full scan of the table then fails — `embed`'s pre-flight read
//! of the hashes first, which blocks the stage on one file among thousands of good ones.
//! The field case was a USB drive: 36 of 25,003 data files.
//!
//! Everything this op knows about Lance it knows through [`crate::vectors::VectorTable`],
//! which owns the table: `audit` reads every fragment back and names the ones that fail,
//! `repair` drops them from the manifest in one commit. This file opens the table, asks,
//! and renders the answer. A dropped fragment's chunks are not lost — their hashes fall
//! back onto `embed`'s work list — but the disk that lost the writes is not fixed by any
//! of this, and the report says so.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::prelude::*;
use crate::vectors::{UnreadableFragment, VectorRepair, VectorTable};

// `VerifyTableArgs` rather than `VerifyArgs`, which `centinel models verify` already owns.
#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct VerifyTableArgs {
    /// Drop the unreadable fragments from the table and commit a new version.
    ///
    /// Without this `verify` only reads: nothing is committed, and nothing is created when
    /// `vectors.lance` does not exist yet. The dropped chunks are not lost — the next
    /// `centinel embed` sees their hashes fall out of the table and re-embeds them. Check
    /// the storage before you let it.
    #[arg(long)]
    #[serde(default)]
    pub repair: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct VerifyReport {
    /// `vectors.lance/`.
    pub vectors: PathBuf,
    pub fragments: usize,
    pub readable: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreadable: Vec<UnreadableFragment>,
    /// `Some` exactly when `--repair` was given, so a report can tell "nothing was broken"
    /// from "nothing was asked to be fixed" without cross-checking `unreadable` first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repaired: Option<VectorRepair>,
}

/// Read every vector fragment back, and with `--repair` drop the ones that cannot be.
///
/// `reach = "operator"`: `--repair` commits a new version of a table that costs a day to
/// rebuild. That is a decision for whoever runs this machine, not for an HTTP or MCP
/// caller.
#[op(long_running, reach = "operator", group = "corpus")]
pub async fn verify(
    ctx: &Ctx,
    args: VerifyTableArgs,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<VerifyReport> {
    let vectors = ctx.store.vectors_path();

    // Nothing to open is not a fault — `verify` before the first `embed` is a reasonable
    // thing to run — and the one thing this must not do about it is create a table to
    // report it empty. The rule `embed --dry-run` follows, for the same reason.
    if !vectors.exists() {
        return Ok(VerifyReport {
            vectors,
            fragments: 0,
            readable: 0,
            unreadable: Vec::new(),
            repaired: args.repair.then_some(VectorRepair::default()),
        });
    }

    let table = VectorTable::open_existing(&ctx.store.vectors_db()).await?;
    let audit = table.audit(progress, cancel).await?;
    let repaired = match args.repair {
        true => Some(table.repair(&audit, cancel).await?),
        false => None,
    };

    Ok(VerifyReport {
        vectors,
        fragments: audit.fragments,
        readable: audit.readable(),
        unreadable: audit.unreadable,
        repaired,
    })
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

impl Render for VerifyReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let verdict = if self.unreadable.is_empty() {
            p.paint("clean", Ink::Green)
        } else if self.repaired.is_some() {
            p.paint("repaired", Ink::Yellow)
        } else {
            p.paint("unreadable", Ink::Red)
        };
        p.line(format!(
            "{verdict}  {}",
            p.paint(&self.vectors.display().to_string(), Ink::Dim)
        ))?;

        p.nest(|p| {
            p.figures(&[
                (self.fragments as u64, "fragments"),
                (self.readable as u64, "readable"),
                (self.unreadable.len() as u64, "unreadable"),
            ])?;

            if self.unreadable.is_empty() {
                return Ok(());
            }

            p.section("unreadable fragments")?;
            for frag in &self.unreadable {
                render_fragment(p, frag)?;
            }

            p.blank()?;
            match &self.repaired {
                Some(done) => render_repaired(p, done),
                None => p.marked(
                    Mark::Warn,
                    p.paint("run `centinel verify --repair` to drop them", Ink::Dim),
                ),
            }
        })
    }
}

/// One fragment's line, with its data files indented under it.
fn render_fragment(p: &mut Painter<'_>, frag: &UnreadableFragment) -> std::io::Result<()> {
    let rows = frag
        .physical_rows
        .map(|n| format!("about {}", render::plural(n, "row", "rows")))
        .unwrap_or_else(|| "unknown row count".to_string());
    let text = format!(
        "fragment {} \u{00b7} {rows} \u{00b7} {}",
        frag.id,
        render::one_line(&frag.error)
    );
    p.marked(Mark::Bad, p.paint(&text, Ink::Dim))?;
    for file in &frag.data_files {
        p.line(p.paint(&format!("    {file}"), Ink::Dim))?;
    }
    Ok(())
}

/// What the repair did and what it did not: the table reads again, and the next `embed`
/// re-embeds what was dropped — onto the same storage, unless the operator looks first.
fn render_repaired(p: &mut Painter<'_>, done: &VectorRepair) -> std::io::Result<()> {
    let text = format!(
        "dropped {} (about {}); the next `centinel embed` re-embeds them",
        render::plural(done.fragments_dropped, "fragment", "fragments"),
        render::plural(done.estimated_rows, "row", "rows"),
    );
    p.marked(Mark::Ok, p.paint(&text, Ink::Green))?;
    p.marked(
        Mark::Warn,
        p.paint(
            "check the storage before you do: the table is usable again, \
             the disk that lost the writes is not fixed",
            Ink::Dim,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::Chunk;
    use crate::index::{Index, Placement};
    use crate::ops::{EmbedArgs, embed};
    use crate::store::Store;

    const DIMS: usize = 4;

    fn hash(byte: u8) -> String {
        hex::encode([byte; 32])
    }

    fn args(repair: bool) -> VerifyTableArgs {
        VerifyTableArgs { repair }
    }

    /// A store whose vector table holds two fragments: one to damage, one to prove
    /// untouched.
    async fn two_fragments() -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let ctx = Ctx::new(store);
        let table = VectorTable::open(&ctx.store.vectors_db(), "test-model", DIMS)
            .await
            .unwrap();
        table
            .append(&[(hash(1), vec![1.0, 0.0, 0.0, 0.0])])
            .await
            .unwrap();
        table
            .append(&[(hash(2), vec![0.0, 1.0, 0.0, 0.0])])
            .await
            .unwrap();
        (dir, ctx)
    }

    /// Overwrites one data file with filler of the same length — the shape of the field
    /// failure: a file of the right size holding nothing Lance can read. Returns the path
    /// the report should name, relative to `vectors.lance/`.
    fn damage_a_data_file(ctx: &Ctx) -> String {
        let data = ctx.store.vectors_path().join("data");
        let mut files: Vec<_> = std::fs::read_dir(&data)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        files.sort();
        let file = &files[0];
        let len = std::fs::metadata(file).unwrap().len() as usize;
        std::fs::write(file, vec![0xAA; len]).unwrap();
        format!("data/{}", file.file_name().unwrap().to_str().unwrap())
    }

    /// `.manifest` files under the table — one per version Lance holds, so an unchanged
    /// count is an unchanged table.
    fn versions(ctx: &Ctx) -> usize {
        std::fs::read_dir(ctx.store.vectors_path().join("_versions"))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "manifest")
            })
            .count()
    }

    #[tokio::test]
    async fn a_missing_table_is_reported_empty_and_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(Store::open(dir.path()).await.unwrap());

        let report = verify(&ctx, args(true), &Progress::none(), &Cancel::none())
            .await
            .unwrap();

        assert_eq!(report.fragments, 0);
        assert!(report.unreadable.is_empty());
        assert_eq!(report.repaired, Some(VectorRepair::default()));
        assert!(!ctx.store.vectors_path().exists(), "verify creates nothing");
    }

    #[tokio::test]
    async fn a_clean_table_is_left_at_its_version() {
        let (_dir, ctx) = two_fragments().await;
        let before = versions(&ctx);

        let report = verify(&ctx, args(true), &Progress::none(), &Cancel::none())
            .await
            .unwrap();

        assert_eq!(report.fragments, 2);
        assert_eq!(report.readable, 2);
        assert!(report.unreadable.is_empty());
        assert_eq!(report.repaired, Some(VectorRepair::default()));
        assert_eq!(versions(&ctx), before, "a clean repair commits nothing");
    }

    #[tokio::test]
    async fn a_damaged_fragment_is_named_and_the_rest_still_read() {
        let (_dir, ctx) = two_fragments().await;
        let damaged = damage_a_data_file(&ctx);
        let before = versions(&ctx);

        let report = verify(&ctx, args(false), &Progress::none(), &Cancel::none())
            .await
            .unwrap();

        assert_eq!(report.fragments, 2);
        assert_eq!(report.readable, 1, "the untouched fragment still reads");
        assert_eq!(report.unreadable.len(), 1);
        let bad = &report.unreadable[0];
        assert!(bad.id < 2, "a fragment the table has: {bad:?}");
        assert_eq!(
            bad.data_files,
            vec![damaged],
            "the file is named, relative to the table"
        );
        assert_eq!(bad.physical_rows, Some(1));
        assert!(!bad.error.is_empty(), "Lance's own words are kept");
        assert!(report.repaired.is_none(), "read-only without --repair");
        assert_eq!(versions(&ctx), before, "a read-only run commits nothing");
    }

    /// The whole point of the repair, end to end: `embed` was blocked by the scan, and
    /// afterwards it has the dropped chunk back on its work list rather than a ghost row.
    #[tokio::test]
    async fn repair_puts_the_dropped_chunks_back_on_embeds_work_list() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let mut index = Index::open(store.index_path()).unwrap();
        for i in 0..3 {
            let chunk = Chunk::new(format!("passage number {i}"), i, String::new(), 0);
            index
                .insert(
                    &chunk,
                    &Placement {
                        source: "test".into(),
                        resource: format!("https://example.gov/{i}"),
                        blob_sha: "0".repeat(64),
                        derived_sha: "1".repeat(64),
                        ordinal: i,
                        heading: String::new(),
                        char_start: chunk.char_start,
                        char_end: chunk.char_end,
                        observed_at: "2026-01-01T00:00:00Z".into(),
                        tool: "test".into(),
                        title: None,
                    },
                )
                .unwrap();
        }
        let hashes = index.chunk_hashes().unwrap();
        let ctx = Ctx::new(store);

        // Every chunk embedded, in two fragments; then one fragment's file goes bad.
        let table = VectorTable::open(&ctx.store.vectors_db(), "qwen3-embedding-4b", 2560)
            .await
            .unwrap();
        let row = |h: &String| -> (String, Vec<f32>) { (h.clone(), vec![0.0; 2560]) };
        table.append(&[row(&hashes[0])]).await.unwrap();
        table
            .append(&hashes[1..].iter().map(row).collect::<Vec<_>>())
            .await
            .unwrap();
        damage_a_data_file(&ctx);

        let plan = EmbedArgs {
            model: Some("qwen3-embedding-4b".to_string()),
            dry_run: true,
            ..Default::default()
        };
        let err = embed(&ctx, plan.clone(), &Progress::none(), &Cancel::none())
            .await
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("centinel verify"),
            "embed's pre-flight names the way out: {err:#}"
        );

        let report = verify(&ctx, args(true), &Progress::none(), &Cancel::none())
            .await
            .unwrap();
        assert_eq!(report.unreadable.len(), 1);
        let done = report.repaired.unwrap();
        assert_eq!(done.fragments_dropped, 1);

        let plan = embed(&ctx, plan, &Progress::none(), &Cancel::none())
            .await
            .unwrap();
        assert_eq!(plan.indexed, 3);
        assert_eq!(plan.already_embedded, 3 - done.estimated_rows);
        assert_eq!(
            plan.remaining, done.estimated_rows,
            "the dropped chunks are work again, not ghost rows"
        );
        assert_eq!(plan.stale, 0, "nothing the index lacks was left behind");
    }
}
