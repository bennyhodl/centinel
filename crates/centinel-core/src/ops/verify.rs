//! `verify`: audit `vectors.lance`, and repair it.
//!
//! ## Why a fragment can go bad without Lance ever seeing a bad write
//!
//! A `.lance` dataset is a manifest plus data files (see [`crate::vectors`] for why that
//! is treated as a plain, copyable directory). The manifest is small and rewritten
//! atomically; a data file is neither. On storage that silently drops writes under load
//! (the field case this was built for was a USB drive), a data file can end up the
//! right *length*, because the filesystem reserved the space, while its bytes never
//! landed. Lance's reader then has no footer, no magic, nothing: every full scan that
//! touches that fragment fails, including `embed`'s pre-flight read of `chunk_hash`,
//! which blocks the whole stage on one bad file among thousands of good ones.
//!
//! ## Why the repair is "drop the fragment", not "recover the bytes"
//!
//! There is nothing to recover: the bytes that should have landed never did. But
//! nothing needs recovering, either: a vector is `chunk_hash -> chunk text (SQLite) ->
//! vector`, computed fresh by `embed` every time, and `embed`'s work list is already
//! "chunks indexed, minus chunks already in the table" (see that module). Once a
//! fragment is out of the manifest, its chunk hashes simply stop appearing in
//! [`crate::vectors::VectorTable::hashes`], fall back onto that work list, and the next
//! `centinel embed` re-embeds them. So this touches only the manifest, which fragment
//! ids it lists, and never a data file's bytes.
//!
//! ## Why the check reads a fragment rather than trusting its footer
//!
//! A trailing-bytes check (does this file end in Lance's magic?) is cheap and usually
//! right, but "usually" is not the guarantee this exists to give: a footer can look
//! intact on a file whose body is not. The check here is a real decode: Lance is asked
//! to actually read `chunk_hash` back out of the fragment, because that is the same
//! read `embed` and `search` do, and the only test that answers the question those
//! commands actually ask.

use std::path::PathBuf;
use std::sync::Arc;

use lance::Dataset;
use lance::dataset::fragment::FileFragment;
use lance::dataset::transaction::{Operation, Transaction};
use lance::dataset::write::CommitBuilder;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::prelude::*;
use crate::vectors::HASH_COLUMN;

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct VerifyArgs {
    /// Drop unreadable fragments from the manifest and commit a new dataset version.
    ///
    /// Without this flag `verify` only reads: nothing is committed, and nothing is
    /// created when `vectors.lance` does not exist yet, the same rule
    /// `centinel embed --dry-run` follows. The dropped chunks are not lost; the next
    /// `centinel embed` sees their hashes fall out of the table and re-embeds them.
    #[arg(long)]
    #[serde(default)]
    pub repair: bool,
}

/// One fragment `verify` could not read back.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct UnreadableFragment {
    /// The fragment id, for `--repair` and for cross-referencing the manifest by hand.
    pub id: u64,
    /// Data file paths, relative to `vectors.lance/`.
    pub data_files: Vec<String>,
    /// From the manifest, not measured: a corrupt file cannot be reasked how long it
    /// claims to be, and the manifest's word is stale in exactly the way that matters:
    /// it is what `embed` will start believing again once the fragment is dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_rows: Option<usize>,
    /// What Lance said when the fragment-scoped scan tried to decode `chunk_hash`.
    pub error: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct VerifyReport {
    /// `vectors.lance/`.
    pub vectors: PathBuf,
    pub fragments: usize,
    pub readable: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreadable: Vec<UnreadableFragment>,
    /// Rows dropped this run. `Some` exactly when `--repair` was given, `None` on a
    /// read-only run, so a report can tell "nothing was broken" from "nothing was
    /// asked to be fixed" without a caller cross-checking `unreadable` first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repaired: Option<usize>,
}

/// Audit `vectors.lance` for fragments Lance cannot decode, and with `--repair`, drop
/// them from the manifest.
///
/// Opens the Lance dataset directly at [`crate::store::Store::vectors_path`] rather than
/// through [`crate::vectors::VectorTable`]: that type exists to guard one model's
/// `chunk_hash -> vector` mapping, and has no reason to expose the fragment-level access
/// a manifest repair needs. A `.lance` directory is meant to be opened this way; see
/// [`crate::vectors`]'s own case for why it is treated as an ordinary directory rather
/// than something only one module may touch.
///
/// `reach = "operator"`: `--repair` commits a new dataset version. That is a decision
/// for the CLI or the scheduler to make about this machine's own corpus, not something
/// an HTTP or MCP caller should be able to trigger.
#[op(long_running, reach = "operator", group = "stage")]
pub async fn verify(
    ctx: &Ctx,
    args: VerifyArgs,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<VerifyReport> {
    let vectors = ctx.store.vectors_path();

    // Nothing to open is not a fault: `verify` before the first `embed` is a
    // reasonable thing to run, and creating a table just to report it empty is exactly
    // the failure `embed --dry-run` already refuses.
    if !vectors.exists() {
        return Ok(VerifyReport {
            vectors,
            fragments: 0,
            readable: 0,
            unreadable: Vec::new(),
            repaired: args.repair.then_some(0),
        });
    }

    progress.say("opening the vector dataset");
    let uri = vectors
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("{} is not valid UTF-8", vectors.display()))?;
    let dataset = Arc::new(Dataset::open(uri).await?);

    let fragments = dataset.get_fragments();
    progress.say(format!(
        "reading {} back out of {} fragments",
        HASH_COLUMN,
        fragments.len()
    ));
    let (readable, unreadable) = audit_fragments(&fragments, progress, cancel).await?;

    let repaired = if args.repair {
        cancel.check()?;
        Some(repair(&dataset, &unreadable, progress).await?)
    } else {
        None
    };

    Ok(VerifyReport {
        vectors,
        fragments: fragments.len(),
        readable,
        unreadable,
        repaired,
    })
}

/// Reads every fragment's `chunk_hash` column back out, and splits the result into a
/// readable count and the fragments that were not.
///
/// A free function rather than inlined in [`verify`] so that function's own job, decide
/// whether to open, audit, repair, stays readable as three calls rather than one loop
/// with the decision logic wrapped around it.
async fn audit_fragments(
    fragments: &[FileFragment],
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<(usize, Vec<UnreadableFragment>)> {
    let mut readable = 0usize;
    let mut unreadable = Vec::new();
    for (i, fragment) in fragments.iter().enumerate() {
        cancel.check()?;
        // Frequent enough that a run over a large corpus is not silent, cheap enough
        // that it costs nothing against thousands of fragment scans.
        if i > 0 && i % 500 == 0 {
            progress.say(format!("checked {i}/{} fragments", fragments.len()));
        }
        match fragment_read_error(fragment).await {
            None => readable += 1,
            Some(error) => unreadable.push(unreadable_fragment(fragment, error)),
        }
    }
    Ok((readable, unreadable))
}

/// `None` for a fragment that decodes cleanly, `Some(message)` for one that does not.
///
/// The read is scoped to this one fragment, a `Scanner` built from it rather than from
/// the dataset, so a bad fragment among thousands costs one fragment's worth of I/O to
/// convict, not a full-table scan repeated per candidate.
async fn fragment_read_error(fragment: &FileFragment) -> Option<String> {
    let mut scanner = fragment.scan();
    if let Err(e) = scanner.project(&[HASH_COLUMN]) {
        return Some(e.to_string());
    }
    scanner.try_into_batch().await.err().map(|e| e.to_string())
}

fn unreadable_fragment(fragment: &FileFragment, error: String) -> UnreadableFragment {
    UnreadableFragment {
        id: fragment.metadata().id,
        data_files: fragment
            .metadata()
            .files
            .iter()
            .map(|f| format!("data/{}", f.path))
            .collect(),
        physical_rows: fragment.metadata().physical_rows,
        error,
    }
}

/// `--repair`'s decision of whether there is a commit to make, plus the commit itself.
///
/// Returns `0` without touching the dataset when nothing was unreadable: a `--repair`
/// run on a clean table commits no version, exactly as an `embed` run with nothing left
/// to do writes nothing.
async fn repair(
    dataset: &Arc<Dataset>,
    unreadable: &[UnreadableFragment],
    progress: &Progress,
) -> anyhow::Result<usize> {
    if unreadable.is_empty() {
        return Ok(0);
    }
    progress.say(format!(
        "dropping {} unreadable fragment(s)",
        unreadable.len()
    ));
    drop_fragments(dataset, unreadable).await
}

/// Commits an `Operation::Delete` removing `fragments` from the manifest, and returns
/// the row count dropped.
///
/// `predicate` is descriptive rather than a real filter: these rows were never
/// evaluated against one, they were named by fragment id because nothing could read them
/// to be filtered. `updated_fragments` stays empty: this is a pure removal, not a
/// rewrite, so no fragment needs a new deletion file alongside it.
async fn drop_fragments(
    dataset: &Arc<Dataset>,
    fragments: &[UnreadableFragment],
) -> anyhow::Result<usize> {
    let ids: Vec<u64> = fragments.iter().map(|f| f.id).collect();
    let rows = fragments.iter().filter_map(|f| f.physical_rows).sum();

    let transaction = Transaction::new(
        dataset.version().version,
        Operation::Delete {
            updated_fragments: Vec::new(),
            deleted_fragment_ids: ids.clone(),
            predicate: format!(
                "centinel verify --repair: {} unreadable fragment(s) {:?}",
                ids.len(),
                ids
            ),
        },
        None,
    );
    CommitBuilder::new(dataset.clone())
        .execute(transaction)
        .await?;
    Ok(rows)
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
            p.paint("corrupt", Ink::Red)
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
            render_repair_note(p, self.unreadable.len(), self.repaired)
        })
    }
}

/// One fragment's line, plus its data file paths indented under it.
fn render_fragment(p: &mut Painter<'_>, frag: &UnreadableFragment) -> std::io::Result<()> {
    let rows = frag
        .physical_rows
        .map(|n| render::plural(n, "row", "rows"))
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

/// What to do next: the row count dropped, or the flag that would drop them.
fn render_repair_note(
    p: &mut Painter<'_>,
    unreadable: usize,
    repaired: Option<usize>,
) -> std::io::Result<()> {
    match repaired {
        Some(rows) => {
            let text = format!(
                "dropped {}. Run `centinel embed` to re-embed {}",
                render::plural(unreadable, "fragment", "fragments"),
                render::plural(rows, "row", "rows"),
            );
            p.marked(Mark::Ok, p.paint(&text, Ink::Green))
        }
        None => p.marked(
            Mark::Warn,
            p.paint("run `centinel verify --repair` to drop them", Ink::Dim),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::vectors::VectorTable;

    const DIMS: usize = 4;

    fn hash(byte: u8) -> String {
        hex::encode([byte; 32])
    }

    async fn seeded_ctx() -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let ctx = Ctx::new(store);
        let table = VectorTable::open(&ctx.store.vectors_db(), "test-model", DIMS)
            .await
            .unwrap();
        // Two separate calls, so the table holds two fragments: one to corrupt, one to
        // prove survives untouched.
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

    /// Overwrites the first fragment's sole data file with bytes of the same length:
    /// the exact shape of the USB failure this op exists for: right length, wrong (here,
    /// entirely absent) content. Returns the corrupted fragment's id.
    async fn corrupt_first_fragment(ctx: &Ctx) -> u64 {
        let vectors = ctx.store.vectors_path();
        let dataset = Dataset::open(vectors.to_str().unwrap()).await.unwrap();
        let bad = dataset.get_fragments().into_iter().next().unwrap();
        let file = &bad.metadata().files[0];
        let path = vectors.join("data").join(&file.path);
        let len = std::fs::metadata(&path).unwrap().len() as usize;
        std::fs::write(&path, vec![0xAAu8; len]).unwrap();
        bad.metadata().id
    }

    #[tokio::test]
    async fn a_missing_table_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let ctx = Ctx::new(store);

        let report = verify(
            &ctx,
            VerifyArgs { repair: false },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();

        assert_eq!(report.fragments, 0);
        assert!(report.unreadable.is_empty());
        assert!(!ctx.store.vectors_path().exists(), "verify creates nothing");
    }

    #[tokio::test]
    async fn a_clean_table_has_no_unreadable_fragments() {
        let (_dir, ctx) = seeded_ctx().await;

        let report = verify(
            &ctx,
            VerifyArgs { repair: false },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();

        assert_eq!(report.fragments, 2);
        assert_eq!(report.readable, 2);
        assert!(report.unreadable.is_empty());
        assert!(report.repaired.is_none());
    }

    #[tokio::test]
    async fn a_corrupted_fragment_is_named_and_the_rest_stay_readable() {
        let (_dir, ctx) = seeded_ctx().await;
        let bad_id = corrupt_first_fragment(&ctx).await;

        let report = verify(
            &ctx,
            VerifyArgs { repair: false },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();

        assert_eq!(report.fragments, 2);
        assert_eq!(report.readable, 1, "the untouched fragment still reads");
        assert_eq!(report.unreadable.len(), 1);
        assert_eq!(report.unreadable[0].id, bad_id);
        assert_eq!(report.unreadable[0].physical_rows, Some(1));
        assert!(
            !report.unreadable[0].data_files.is_empty(),
            "the bad file's path is reported"
        );
        assert!(report.repaired.is_none(), "read-only without --repair");
    }

    #[tokio::test]
    async fn repair_drops_the_bad_fragment_and_embed_can_see_the_gap() {
        let (_dir, ctx) = seeded_ctx().await;
        corrupt_first_fragment(&ctx).await;

        let report = verify(
            &ctx,
            VerifyArgs { repair: true },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();

        assert_eq!(report.unreadable.len(), 1);
        assert_eq!(report.repaired, Some(1), "one row in the dropped fragment");

        // The table now opens and reads cleanly: `open_existing`'s guard, and every
        // full scan `embed`'s pre-flight makes, both go through this same call.
        let reopened = VectorTable::open_existing(&ctx.store.vectors_db())
            .await
            .unwrap();
        let hashes = reopened.hashes().await.unwrap();
        assert_eq!(hashes.len(), 1, "only the untouched fragment's row remains");
        assert!(hashes.contains(&hash(2)));
        assert!(
            !hashes.contains(&hash(1)),
            "the dropped chunk falls back onto embed's work list, not into a ghost row"
        );
    }

    #[tokio::test]
    async fn repair_with_nothing_unreadable_commits_no_version() {
        let (_dir, ctx) = seeded_ctx().await;

        let report = verify(
            &ctx,
            VerifyArgs { repair: true },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();

        assert_eq!(report.repaired, Some(0));
        let reopened = VectorTable::open_existing(&ctx.store.vectors_db())
            .await
            .unwrap();
        assert_eq!(reopened.len().await.unwrap(), 2, "nothing was dropped");
    }
}
