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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Markdown heading trail the passage sits under.
    #[serde(skip_serializing_if = "String::is_empty")]
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
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
#[op]
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
