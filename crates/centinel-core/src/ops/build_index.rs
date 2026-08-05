//! `index` — chunk derived text into the searchable index.
//!
//! Reads derivations from the log, chunks their text, and writes chunks plus placements
//! into `centinel.db`. Entirely derived: `centinel index --rebuild` after `rm centinel.db`
//! reproduces it from the blob pool.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::chunk::{ChunkConfig, chunk_markdown};
use crate::index::{Index, Placement};
use crate::prelude::*;
use crate::store::LogRecord;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct IndexArgs {
    /// Source to index. Omit for all.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,

    /// Re-index from scratch. The index is derived, so this costs only time.
    ///
    /// Scoped by `--source`: with one named, only that source is cleared. The path after
    /// changing an extractor, when the old chunks would otherwise linger beside the new.
    #[arg(long)]
    #[serde(default)]
    pub rebuild: bool,

    /// Target chunk size in characters.
    #[arg(long, default_value_t = crate::chunk::DEFAULT_TARGET_CHARS)]
    #[serde(default = "default_target")]
    pub target_chars: usize,

    /// Characters of overlap between adjacent chunks.
    #[arg(long, default_value_t = crate::chunk::DEFAULT_OVERLAP_CHARS)]
    #[serde(default = "default_overlap")]
    pub overlap_chars: usize,
}

fn default_target() -> usize {
    crate::chunk::DEFAULT_TARGET_CHARS
}
fn default_overlap() -> usize {
    crate::chunk::DEFAULT_OVERLAP_CHARS
}

/// So [`crate::ops::run`] chunks exactly as the CLI does. Chunk geometry is baked into
/// every `chunk_hash`, so a second set of defaults here would silently re-embed the
/// corpus the first time the two disagreed.
impl Default for IndexArgs {
    fn default() -> Self {
        Self {
            source: None,
            rebuild: false,
            target_chars: default_target(),
            overlap_chars: default_overlap(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct IndexReport {
    pub sources: Vec<String>,
    pub derivations: usize,
    /// Skipped because this derived text was already chunked in.
    pub already_indexed: usize,
    pub documents_indexed: usize,
    pub chunks_written: usize,
    /// Chunks whose text was already present under another placement — the
    /// boilerplate-collapsing property.
    pub chunks_deduplicated: usize,
    pub total_chunks: usize,
    pub total_placements: usize,
    pub total_chars: usize,
}

/// Chunk extracted text into the search index.
#[op(long_running, group = "stage")]
pub async fn index(ctx: &Ctx, args: IndexArgs, progress: &Progress) -> anyhow::Result<IndexReport> {
    let sources = match &args.source {
        Some(s) => vec![SourceId::new(s.clone())?],
        None => ctx.store.sources().await?,
    };

    let db_path = ctx.store.index_path();
    let mut index = Index::open(&db_path)?;

    if args.rebuild {
        match &args.source {
            Some(_) => {
                for source in &sources {
                    index.clear_source(source.as_str())?;
                }
            }
            None => index.clear()?,
        }
    }
    // After any clearing, not before: what licenses a change of geometry is that no
    // chunk built at the old one survives, and only the clearing decides that.
    settle_geometry(&index, &args)?;

    let config = ChunkConfig {
        target_chars: args.target_chars,
        overlap_chars: args.overlap_chars,
        ..Default::default()
    };

    let mut report = IndexReport {
        sources: sources.iter().map(|s| s.to_string()).collect(),
        derivations: 0,
        already_indexed: 0,
        documents_indexed: 0,
        chunks_written: 0,
        chunks_deduplicated: 0,
        total_chunks: 0,
        total_placements: 0,
        total_chars: 0,
    };

    for source in &sources {
        let log = ctx.store.read_log(source).await?;

        // A blob can sit at more than one address — the same PDF linked from two pages.
        // Every address is a legitimate citation, so all of them become placements.
        let mut addresses: HashMap<BlobSha, Vec<(String, String)>> = HashMap::new();
        for rec in &log {
            if let LogRecord::Observation(o) = rec {
                addresses
                    .entry(o.blob_sha.clone())
                    .or_default()
                    .push((o.resource.natural_key.clone(), o.at.to_string()));
            }
        }

        let derivations: Vec<_> = log
            .iter()
            .filter_map(|r| match r {
                LogRecord::Derivation(d) => Some(d.clone()),
                _ => None,
            })
            .collect();
        report.derivations += derivations.len();

        let total = derivations.len() as u64;
        for (i, d) in derivations.iter().enumerate() {
            if i % 25 == 0 {
                progress.step(format!("{} chunks", report.chunks_written), i as u64, total);
            }
            if !args.rebuild && index.has_derived(d.to_sha.as_str())? {
                report.already_indexed += 1;
                continue;
            }

            let bytes = ctx.store.get_blob(&d.to_sha).await?;
            let text = String::from_utf8_lossy(&bytes);
            if text.trim().is_empty() {
                continue;
            }

            // The extraction pipeline does not record a title on the Derivation, so it
            // is recovered from the first heading — which is where both `htmd` and
            // `pdf-inspector` put it.
            let title = first_heading(&text);
            let chunks = chunk_markdown(&text, &config);
            if chunks.is_empty() {
                continue;
            }
            report.documents_indexed += 1;

            let places = addresses.get(&d.from_sha).cloned().unwrap_or_default();
            for chunk in &chunks {
                let before = index.stats()?.chunks;
                for (resource, observed_at) in &places {
                    index.insert(
                        chunk,
                        &Placement {
                            source: source.to_string(),
                            resource: resource.clone(),
                            blob_sha: d.from_sha.to_string(),
                            derived_sha: d.to_sha.to_string(),
                            ordinal: chunk.ordinal,
                            heading: chunk.heading.clone(),
                            char_start: chunk.char_start,
                            char_end: chunk.char_end,
                            observed_at: observed_at.clone(),
                            tool: format!("{} {}", d.tool, d.version),
                            title: title.clone(),
                        },
                    )?;
                }
                if index.stats()?.chunks == before {
                    report.chunks_deduplicated += 1;
                } else {
                    report.chunks_written += 1;
                }
            }
        }
    }

    let stats = index.stats()?;
    report.total_chunks = stats.chunks;
    report.total_placements = stats.placements;
    report.total_chars = stats.chars;

    progress.say(format!("{} chunks indexed", report.total_chunks));
    Ok(report)
}

/// Refuses a change of chunk geometry that would leave two sets of chunks side by side.
///
/// A `chunk_hash` is the hash of the chunk's **text**, and the geometry decides the text.
/// Re-chunking at a different size therefore produces an entirely different set of
/// hashes: the old chunks stay in the index, the new ones join them, and `embed` — whose
/// work list is "indexed hashes minus cached hashes" — re-embeds the whole corpus while
/// the old vectors sit in an append-only cache file that nothing will ever read again.
///
/// None of that fails. It is hours of GPU time and a doubled index, and the only sign is
/// a number the operator was not watching. So it is refused, and the refusal names the
/// flag that makes it legal.
/// Called **after** any `--rebuild` clearing, because the question is whether a chunk
/// built at the old geometry survives — not which flag was passed. A `--rebuild --source`
/// that happens to empty the index licenses a change for the same reason a full one does,
/// and one that leaves another source's chunks behind does not.
fn settle_geometry(index: &Index, args: &IndexArgs) -> anyhow::Result<()> {
    let asked = (args.target_chars, args.overlap_chars);

    match index.geometry()? {
        Some(recorded) if recorded == asked => {}
        // Nothing left to mix with.
        _ if index.stats()?.chunks == 0 => index.set_geometry(asked.0, asked.1)?,

        Some(recorded) => anyhow::bail!(
            "this index was built at target_chars={}, overlap_chars={}; this run asks for \
             {}, {}. Chunk geometry decides the chunk text, and the text is what a \
             chunk_hash hashes — so the old chunks would stay beside the new ones and \
             every vector in the cache would be orphaned. Re-run with `--rebuild` to \
             replace the index, or drop the flags to keep it.",
            recorded.0,
            recorded.1,
            asked.0,
            asked.1,
        ),

        // No recorded geometry and chunks already present: an index from before this was
        // written. Its chunks can only be assumed to use the defaults, so that is the
        // one thing this may adopt without being told.
        None => {
            anyhow::ensure!(
                asked
                    == (
                        crate::chunk::DEFAULT_TARGET_CHARS,
                        crate::chunk::DEFAULT_OVERLAP_CHARS
                    ),
                "this index records no chunk geometry, so its existing chunks can only be \
                 assumed to use the defaults ({}, {}). Re-run with `--rebuild` to build it \
                 at {}, {} instead.",
                crate::chunk::DEFAULT_TARGET_CHARS,
                crate::chunk::DEFAULT_OVERLAP_CHARS,
                asked.0,
                asked.1,
            );
            index.set_geometry(asked.0, asked.1)?;
        }
    }
    Ok(())
}

/// First markdown heading, used as a document title.
fn first_heading(text: &str) -> Option<String> {
    text.lines()
        .take(40)
        .find_map(|l| {
            let t = l.trim_start();
            t.starts_with('#')
                .then(|| t.trim_start_matches('#').trim().to_string())
        })
        .filter(|t| !t.is_empty())
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// The counters, with deduplication called out.
///
/// `chunks_deduplicated` is the interesting one and the reason it gets a sentence rather
/// than a row: it is boilerplate collapsing. A council site repeats the same navigation
/// header on nine hundred pages, and the figure that shows the index refusing to store it
/// nine hundred times is the one that explains why `total_chunks` is smaller than a person
/// expects.
impl Render for IndexReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.title(
            &self.sources.join(", "),
            &format!("{} of text", render::count(self.total_chars as u64)),
        )?;
        p.nest(|p| {
            p.figures(&[
                (self.derivations as u64, "derivations"),
                (self.already_indexed as u64, "already indexed"),
                (self.documents_indexed as u64, "documents indexed"),
                (self.chunks_written as u64, "chunks written"),
                (self.chunks_deduplicated as u64, "chunks deduplicated"),
            ])?;

            p.blank()?;
            let totals = format!(
                "{} in the index, at {}",
                render::plural(self.total_chunks, "chunk", "chunks"),
                render::plural(self.total_placements, "placement", "placements"),
            );
            p.line(p.paint(&totals, Ink::Dim))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{DEFAULT_OVERLAP_CHARS, DEFAULT_TARGET_CHARS, chunk_markdown};
    use crate::index::Placement;

    fn geometry(target: usize, overlap: usize) -> IndexArgs {
        IndexArgs {
            target_chars: target,
            overlap_chars: overlap,
            ..Default::default()
        }
    }

    /// An index holding one document's chunks at the default geometry.
    fn populated() -> Index {
        let mut index = Index::in_memory().unwrap();
        let text = "# Agenda\n\n".to_string() + &"The council discussed the matter. ".repeat(80);
        for chunk in chunk_markdown(&text, &ChunkConfig::default()) {
            index
                .insert(
                    &chunk,
                    &Placement {
                        source: "tampa".into(),
                        resource: "https://tampa.gov/a".into(),
                        blob_sha: "aa".repeat(32),
                        derived_sha: "bb".repeat(32),
                        ordinal: chunk.ordinal,
                        heading: chunk.heading.clone(),
                        char_start: chunk.char_start,
                        char_end: chunk.char_end,
                        observed_at: "2026-08-03T00:00:00Z".into(),
                        tool: "htmd 0.5".into(),
                        title: None,
                    },
                )
                .unwrap();
        }
        assert!(index.stats().unwrap().chunks > 1);
        index
    }

    /// The same, under two sources, so clearing one leaves the other's chunks.
    fn two_sources() -> Index {
        let mut index = populated();
        let text = "# Minutes\n\n".to_string() + &"The board approved the item. ".repeat(80);
        for chunk in chunk_markdown(&text, &ChunkConfig::default()) {
            index
                .insert(
                    &chunk,
                    &Placement {
                        source: "pinellas".into(),
                        resource: "https://pinellas.gov/m".into(),
                        blob_sha: "cc".repeat(32),
                        derived_sha: "dd".repeat(32),
                        ordinal: chunk.ordinal,
                        heading: chunk.heading.clone(),
                        char_start: chunk.char_start,
                        char_end: chunk.char_end,
                        observed_at: "2026-08-03T00:00:00Z".into(),
                        tool: "htmd 0.5".into(),
                        title: None,
                    },
                )
                .unwrap();
        }
        index
    }

    #[test]
    fn an_empty_index_adopts_whatever_geometry_it_is_given() {
        let index = Index::in_memory().unwrap();
        settle_geometry(&index, &geometry(800, 100)).unwrap();
        assert_eq!(index.geometry().unwrap(), Some((800, 100)));
    }

    #[test]
    fn the_same_geometry_twice_is_not_a_change() {
        let index = populated();
        settle_geometry(
            &index,
            &geometry(DEFAULT_TARGET_CHARS, DEFAULT_OVERLAP_CHARS),
        )
        .unwrap();
        settle_geometry(
            &index,
            &geometry(DEFAULT_TARGET_CHARS, DEFAULT_OVERLAP_CHARS),
        )
        .unwrap();
    }

    /// The silent bill this guard exists to refuse: different geometry means different
    /// chunk text, different hashes, two sets of chunks and a whole corpus re-embedded.
    #[test]
    fn changing_the_geometry_under_a_populated_index_is_refused() {
        let index = populated();
        settle_geometry(
            &index,
            &geometry(DEFAULT_TARGET_CHARS, DEFAULT_OVERLAP_CHARS),
        )
        .unwrap();

        let err = settle_geometry(&index, &geometry(800, 100))
            .unwrap_err()
            .to_string();
        assert!(err.contains("--rebuild"), "{err}");
        assert!(err.contains("orphaned"), "the cost is stated: {err}");
        assert_eq!(
            index.geometry().unwrap(),
            Some((DEFAULT_TARGET_CHARS, DEFAULT_OVERLAP_CHARS)),
            "a refused change must not be recorded"
        );
    }

    /// A rebuild that empties the index licenses a change, because nothing built at the
    /// old geometry survives to be mixed with.
    #[test]
    fn clearing_the_index_licenses_a_change_of_geometry() {
        let mut index = populated();
        settle_geometry(
            &index,
            &geometry(DEFAULT_TARGET_CHARS, DEFAULT_OVERLAP_CHARS),
        )
        .unwrap();

        index.clear().unwrap();
        settle_geometry(&index, &geometry(800, 100)).unwrap();
        assert_eq!(index.geometry().unwrap(), Some((800, 100)));
    }

    /// `--rebuild --source tampa` that leaves another source's chunks behind does not.
    /// The flag is not what decides this; the surviving chunks are.
    #[test]
    fn a_scoped_rebuild_that_leaves_chunks_behind_does_not() {
        let mut index = two_sources();
        settle_geometry(
            &index,
            &geometry(DEFAULT_TARGET_CHARS, DEFAULT_OVERLAP_CHARS),
        )
        .unwrap();

        index.clear_source("tampa").unwrap();
        assert!(
            index.stats().unwrap().chunks > 0,
            "pinellas is still in there"
        );

        let err = settle_geometry(&index, &geometry(800, 100))
            .unwrap_err()
            .to_string();
        assert!(err.contains("--rebuild"), "{err}");
    }

    /// An index written before the geometry was recorded. Its chunks can only be assumed
    /// to use the defaults, so that is the only thing it may silently adopt.
    #[test]
    fn an_index_with_no_recorded_geometry_may_only_assume_the_defaults() {
        let index = populated();
        assert_eq!(index.geometry().unwrap(), None);

        let err = settle_geometry(&index, &geometry(800, 100))
            .unwrap_err()
            .to_string();
        assert!(err.contains("records no chunk geometry"), "{err}");

        settle_geometry(
            &index,
            &geometry(DEFAULT_TARGET_CHARS, DEFAULT_OVERLAP_CHARS),
        )
        .unwrap();
        assert_eq!(
            index.geometry().unwrap(),
            Some((DEFAULT_TARGET_CHARS, DEFAULT_OVERLAP_CHARS))
        );
    }

    // ── re-extraction ──────────────────────────────────────────────────────────

    /// A store holding one page and `n` successive extractions of it, each different.
    async fn store_with_extractions(texts: &[&str]) -> (tempfile::TempDir, Ctx) {
        use crate::domain::Derivation;
        use crate::store::{LogRecord, Store};

        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).await.unwrap();
        let id = SourceId::new("tampa").unwrap();

        let obs = store
            .record_observation(
                &Resource::new(id.clone(), "https://tampa.gov/agenda"),
                b"<html>the original bytes</html>",
                jiff::Timestamp::now(),
                Default::default(),
            )
            .await
            .unwrap();

        for (i, text) in texts.iter().enumerate() {
            let to_sha = store.put_blob(text.as_bytes()).await.unwrap();
            store
                .append(
                    &id,
                    &LogRecord::Derivation(Derivation {
                        from_sha: obs.blob_sha.clone(),
                        to_sha,
                        tool: "htmd".into(),
                        version: format!("0.{i}"),
                        model_tier: None,
                        at: jiff::Timestamp::now(),
                        anchors: Vec::new(),
                    }),
                )
                .await
                .unwrap();
        }
        (dir, Ctx::new(store))
    }

    fn body(word: &str) -> String {
        format!(
            "# Agenda\n\n{}",
            format!("The council discussed the {word}. ").repeat(80)
        )
    }

    /// The defect `run --refresh` now avoids. Re-extracting produces a *new* derived
    /// blob, so its chunks are added — and nothing removes the previous extraction's, so
    /// search answers from both.
    #[tokio::test]
    async fn re_extraction_leaves_the_old_chunks_unless_the_index_is_rebuilt() {
        let (_dir, ctx) = store_with_extractions(&[&body("budget"), &body("zoning")]).await;

        let added = index(&ctx, IndexArgs::default(), &Progress::none())
            .await
            .unwrap();
        assert_eq!(added.derivations, 2);
        let both = added.total_chunks;
        assert!(both > 2, "{both} chunks");

        // Both extractions are in there, which is the corpus answering twice.
        let idx = Index::open(ctx.store.index_path()).unwrap();
        assert!(!idx.search("budget", 5, None).unwrap().is_empty());
        assert!(!idx.search("zoning", 5, None).unwrap().is_empty());

        // What `run --refresh` now asks for.
        let rebuilt = index(
            &ctx,
            IndexArgs {
                rebuild: true,
                ..Default::default()
            },
            &Progress::none(),
        )
        .await
        .unwrap();
        assert_eq!(
            rebuilt.total_chunks, both,
            "a rebuild replaces the index rather than adding to it"
        );
    }

    #[test]
    fn title_comes_from_the_first_heading() {
        assert_eq!(
            first_heading("## Lobbyist Meeting Log\n\nbody"),
            Some("Lobbyist Meeting Log".into())
        );
        assert_eq!(first_heading("no headings here"), None);
        assert_eq!(
            first_heading("#\n\nbody"),
            None,
            "empty heading is not a title"
        );
    }
}
