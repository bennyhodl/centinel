//! `search` — ask the corpus a question.
//!
//! Currently **BM25 only**. SPEC §6 specifies hybrid retrieval — BM25 and vector search
//! fused with RRF, then reranked — and this is the first of those two arms. It is not a
//! placeholder: on the BRIGHT benchmark BM25 scores 13.7 against BGE-large's 13.8, so
//! keyword search is a real baseline rather than a warm-up.
//!
//! Every result carries its provenance: source, address, the observation time, the tool
//! that derived the text, and the character span within it (SPEC §6).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::index::Index;
use crate::prelude::*;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// What to search for.
    #[arg(value_name = "QUERY")]
    pub query: String,

    /// Maximum results.
    #[arg(long, short = 'n', default_value_t = 10)]
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Restrict to one source.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,

    /// Characters of matched passage to return. 0 returns the whole chunk.
    #[arg(long, default_value_t = 400)]
    #[serde(default = "default_snippet")]
    pub snippet_chars: usize,
}

fn default_limit() -> usize {
    10
}
fn default_snippet() -> usize {
    400
}

/// One ranked passage, with everything needed to cite it.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchResult {
    pub rank: usize,
    pub score: f64,
    /// The passage itself.
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Markdown heading trail the passage sits under.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub heading: String,
    pub source: String,
    /// Where it came from.
    pub url: String,
    /// When we observed it.
    pub observed_at: String,
    /// Which extraction pipeline produced this text.
    pub tool: String,
    /// SHA-256 of the original bytes as served — the evidentiary anchor.
    pub blob_sha: String,
    /// Character span within the derived text.
    pub char_start: usize,
    pub char_end: usize,
    /// Other addresses carrying this identical passage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_at: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchReport {
    pub query: String,
    /// How retrieval was performed. Will become `bm25+vector→rrf→rerank`.
    pub method: String,
    pub results: Vec<SearchResult>,
    pub total_chunks_indexed: usize,
}

/// Search the corpus for a passage.
#[op(group = "corpus")]
pub async fn search(ctx: &Ctx, args: SearchArgs) -> anyhow::Result<SearchReport> {
    let db_path = ctx.store.root().join("centinel.db");
    anyhow::ensure!(
        db_path.exists(),
        "no index at {} — run `centinel index` first",
        db_path.display()
    );

    let index = Index::open(&db_path)?;
    let hits = index.search(&args.query, args.limit, args.source.as_deref())?;

    let results = hits
        .into_iter()
        .enumerate()
        .filter_map(|(i, hit)| {
            // A chunk always has at least one placement; one without is an index bug,
            // and dropping it is better than emitting a citation-less result.
            let primary = hit.placements.first()?;
            let also_at = hit
                .placements
                .iter()
                .skip(1)
                .map(|p| p.resource.clone())
                .collect();

            let text = if args.snippet_chars == 0 || hit.text.chars().count() <= args.snippet_chars
            {
                hit.text
            } else {
                let mut s: String = hit.text.chars().take(args.snippet_chars).collect();
                s.push('…');
                s
            };

            Some(SearchResult {
                rank: i + 1,
                score: hit.score,
                text,
                title: primary.title.clone(),
                heading: primary.heading.clone(),
                source: primary.source.clone(),
                url: primary.resource.clone(),
                observed_at: primary.observed_at.clone(),
                tool: primary.tool.clone(),
                blob_sha: primary.blob_sha.clone(),
                char_start: primary.char_start,
                char_end: primary.char_end,
                also_at,
            })
        })
        .collect();

    Ok(SearchReport {
        query: args.query,
        method: "bm25".into(),
        results,
        total_chunks_indexed: index.stats()?.chunks,
    })
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// A ranked list, read top to bottom.
///
/// The passage is the answer, so it gets the width and the plain ink; everything else is
/// provenance and sits dim around it. The one piece of provenance promoted to the same
/// line as the title is the **source**, because "which city said this" changes what the
/// passage means and the others do not.
///
/// The blob hash *is* printed, short, in cyan — because it is not only provenance, it is
/// the handle. A result you cannot act on is a dead end, and `centinel open <hash>` or
/// `centinel read <hash>` is what turns a passage back into the document it came from.
/// Twelve hex characters is the shortest form both commands accept.
///
/// The character span is not printed. That one really is for a verifier, who should be
/// reading `--json` and hashing the blob rather than eyeballing offsets in a terminal.
impl Render for SearchReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let aside = format!(
            "{} · {} · {} indexed",
            render::plural(self.results.len(), "result", "results"),
            self.method,
            render::plural(self.total_chunks_indexed, "chunk", "chunks"),
        );
        p.title(&self.query, &aside)?;

        if self.results.is_empty() {
            p.blank()?;
            return p.line(p.paint("Nothing matched.", Ink::Dim));
        }

        for result in &self.results {
            p.blank()?;
            result.render(p)?;
        }
        Ok(())
    }
}

impl Render for SearchResult {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        // The heading trail beats the document title: it says where *in* the document the
        // passage sits, which is the more specific of the two and never wrong when both
        // are present.
        let named = !self.heading.is_empty() || self.title.is_some();
        let label = if !self.heading.is_empty() {
            &self.heading
        } else {
            self.title.as_deref().unwrap_or(&self.url)
        };

        let rank = p.paint(&format!("{:>2}", self.rank), Ink::Dim);
        let name = p.paint(&render::truncate(label, p.width().saturating_sub(24)), Ink::Bold);
        let score = p.paint(&format!("{} · {:.2}", self.source, self.score), Ink::Dim);
        p.line(format!("{rank}  {name}  {score}"))?;

        p.nest(|p| {
            p.nest(|p| {
                p.wrapped(&self.text, Ink::Plain)?;
                // An untitled passage already used its URL as the headline. Printing it
                // again underneath is the JSON habit — repeating a field because the
                // structure has a slot for it.
                //
                // The hash leads the line because it is the one thing here you type back.
                let hash = p.paint(&render::short_sha(&self.blob_sha), Ink::Cyan);
                let provenance = if named {
                    format!(
                        "{}  ·  {}",
                        render::truncate_start(&self.url, p.width().saturating_sub(39)),
                        render::short_time(&self.observed_at),
                    )
                } else {
                    render::short_time(&self.observed_at)
                };
                p.line(format!("{hash}  ·  {}", p.paint(&provenance, Ink::Dim)))?;
                if !self.also_at.is_empty() {
                    let also = format!(
                        "also at {}",
                        render::plural(self.also_at.len(), "address", "addresses")
                    );
                    p.line(p.paint(&also, Ink::Dim))?;
                }
                Ok(())
            })
        })
    }
}
