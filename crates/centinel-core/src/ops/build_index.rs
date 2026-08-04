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

    let db_path = ctx.store.root().join("centinel.db");
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
