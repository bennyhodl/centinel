//! `search` — ask the corpus a question.
//!
//! **Hybrid**, per SPEC §6: two arms, fused with Reciprocal Rank Fusion, then reranked.
//!
//! ```text
//!   query
//!     ├─ BM25   (SQLite FTS5)          → top 100
//!     └─ vector (the embedder + Lance) → top 100
//!           └─ RRF fuse (k=60)         → top 40
//!                 └─ Qwen3-Reranker    → top n
//! ```
//!
//! Neither arm is a warm-up. On the BRIGHT benchmark BM25 scores 13.7 against BGE-large's
//! 13.8, and §6.4 keeps it because names, motions, ordinance numbers and dollar figures —
//! what people actually search meeting records for — are exact tokens that vector search
//! fails hardest on. The vector arm is for the opposite case: `"drinking water sampling
//! results"` matches nothing in FTS5 on this corpus, because the water report says
//! `PWSName`, `Analyte` and `UCMR 5`.
//!
//! Reranking is always on (§6.3): one command, one answer, and no fast path that
//! silently returns a worse ordering. The measured gap is why — BM25 goes from **14.8 to
//! 33.4** nDCG@10 when reranked, and reranked BM25 beats a reasoning-tuned dense
//! retriever used alone. The first stage only has to get the right passage into the top
//! forty; it does not have to rank it.
//!
//! ## A rank is a position in a pool, and says nothing about the pool
//!
//! RRF weights by rank alone, so the vector arm's rank 1 counts exactly as much whether
//! it was drawn from four hundred thousand vectors or from two thousand. A partly
//! embedded corpus therefore does **not** degrade gently — it promotes confident results
//! from a small pool. So the report carries [`SearchReport::vectors_indexed`] beside the
//! chunk count, and both are printed: a reader can see how much of the corpus the vector
//! arm could actually see. An absent arm is named in [`SearchReport::no_vectors`] and an
//! absent reranker in [`SearchReport::no_rerank`], rather than either being passed over.
//!
//! Every result carries its provenance: source, address, the observation time, the tool
//! that derived the text, and the character span within it (SPEC §6).

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::index::{Hit, Index};
use crate::prelude::*;
use crate::vectors::VectorTable;

/// How deep each arm reaches before fusion (SPEC §6).
const ARM_DEPTH: usize = 100;

/// RRF's rank offset. 60 is the value SPEC §6 pins, from the original TREC work.
///
/// It is what stops rank 1 from dominating: with `k = 60` the gap between rank 1 and
/// rank 2 is small, so agreement between the arms matters more than either arm's
/// confidence — which is the whole reason to fuse rather than to pick.
const RRF_K: f64 = 60.0;

/// How much deeper the vector arm reaches when `--source` is set.
///
/// Lance carries no source column, so the filter is applied after retrieval and a
/// plain top-100 could come back nearly empty on a corpus one source dominates. This
/// over-fetches instead. It can still under-fill, which is a known limit of the
/// post-filter rather than a bug in it.
const SOURCE_OVERFETCH: usize = 5;

/// How many fused candidates reach the reranker (SPEC §6: "top 30–40").
///
/// The first stage only has to get the right passage into this window; it does not have
/// to rank it. That is why a cheap arm that over-fetches plus a good reranker beats an
/// expensive retriever alone, and why the window is wider than any `--limit` anyone
/// types.
const RERANK_DEPTH: usize = 40;

/// The cross-encoder. One model, because the registry's alternates fill the same role
/// and picking between them is `models`' business, not a search flag's.
const RERANKER: &str = "qwen3-reranker-0.6b";

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

/// Another address carrying this identical passage.
///
/// Carries its own hash. It used to be a bare URL, which made "also at 2 addresses" a
/// dead end: the reader was told two more documents contain this text and given no way to
/// reach either. A different address is a different document — its own bytes, its own
/// history — so the handle cannot be inferred from the one above it.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AlsoAt {
    pub source: String,
    pub url: String,
    /// SHA-256 of the original bytes at *that* address.
    pub blob_sha: String,
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
    /// SHA-256 of the derived text `char_start`/`char_end` index into.
    ///
    /// Without it the span is uninterpretable: it is an offset into an extraction, and
    /// nothing else in the result said which one. A valid target for `read` and `open`.
    pub derived_sha: String,
    /// Content hash of the passage — the key the vector table is written under, and what
    /// makes the same text appearing under two addresses one row rather than two.
    pub chunk_hash: String,
    /// Character span within the derived text.
    pub char_start: usize,
    pub char_end: usize,
    /// Other addresses carrying this identical passage, capped at [`ALSO_CARRIED`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_at: Vec<AlsoAt>,
    /// How many there are in total, which `also_at` may not list in full.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub also_at_total: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchReport {
    pub query: String,
    /// Which stages actually ran: `bm25`, `bm25→rerank`, `bm25+vector→rrf`, or
    /// `bm25+vector→rrf→rerank`. Assembled from what ran, never hard-coded.
    pub method: String,
    pub results: Vec<SearchResult>,
    pub total_chunks_indexed: usize,
    /// How many of those chunks have a vector.
    ///
    /// Beside the chunk count because RRF cannot tell a small pool from a large one, so
    /// the reader has to. See this module's header.
    #[serde(default)]
    pub vectors_indexed: usize,
    /// Why the vector arm did not run. `None` when it did.
    ///
    /// A one-armed search is a different answer, not a slower one, so it is said rather
    /// than inferred from `method`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_vectors: Option<String>,
    /// Why the results were not reranked. `None` when they were.
    ///
    /// §6.3 makes reranking always on because the measured gap is large — BM25 goes from
    /// 14.8 to 33.4 nDCG@10 — so an unreranked ordering is materially worse and has to
    /// say so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_rerank: Option<String>,
}

/// Search the corpus for a passage.
#[op(group = "corpus")]
pub async fn search(ctx: &Ctx, args: SearchArgs) -> anyhow::Result<SearchReport> {
    // Checked before the vector arm so a missing index fails immediately rather than
    // after a multi-gigabyte model load. This is a path, not a connection.
    let index_path = ctx.store.require_index()?;

    // A missing model or an unbuilt table is a normal state, not a failure: the corpus
    // is keyword-searchable long before it is embedded. It degrades to one arm and says
    // so, rather than returning an error a reader cannot act on.
    let (vector, vectors_indexed, no_vectors) = match vector_arm(ctx, &args).await {
        Ok(arm) => (arm.hits, arm.stored, None),
        Err(reason) => (Vec::new(), 0, Some(reason)),
    };

    // Every SQLite call is inside here. `Index` owns a `rusqlite::Connection` and so is
    // not `Send`; one merely *alive* across an `await` makes this op's future
    // un-spawnable, whether or not it is used again. Confining it to a sync function is
    // what keeps that impossible rather than merely currently-true.
    //
    // It retrieves to `RERANK_DEPTH`, not to `args.limit`: the reranker's whole value is
    // reordering a set larger than the one returned (§6.3).
    let (mut hits, total_chunks_indexed) = retrieve(&index_path, &args, vector)?;

    let no_rerank = rerank_arm(&args.query, &mut hits).await.err();
    hits.truncate(args.limit);

    let method = method(no_vectors.is_none(), no_rerank.is_none());

    let results = hits
        .into_iter()
        .enumerate()
        .filter_map(|(i, hit)| {
            // A chunk always has at least one placement; one without is an index bug,
            // and dropping it is better than emitting a citation-less result.
            let primary = hit.placements.first()?;
            let also_at_total = hit.placements.len().saturating_sub(1);
            let also_at = hit
                .placements
                .iter()
                .skip(1)
                .take(ALSO_CARRIED)
                .map(|p| AlsoAt {
                    source: p.source.clone(),
                    url: p.resource.clone(),
                    blob_sha: p.blob_sha.clone(),
                })
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
                derived_sha: primary.derived_sha.clone(),
                chunk_hash: hit.chunk_hash.clone(),
                char_start: primary.char_start,
                char_end: primary.char_end,
                also_at,
                also_at_total,
            })
        })
        .collect();

    Ok(SearchReport {
        query: args.query,
        method,
        results,
        total_chunks_indexed,
        vectors_indexed,
        no_vectors,
        no_rerank,
    })
}

/// The name of what actually ran, assembled from what actually ran.
///
/// Built rather than hard-coded at four call sites, because `method` is the field a
/// reader trusts to know which pipeline produced an ordering — a stale literal there is
/// worse than no field at all.
fn method(vectors: bool, reranked: bool) -> String {
    let mut parts = String::from("bm25");
    if vectors {
        parts.push_str("+vector→rrf");
    }
    if reranked {
        parts.push_str("→rerank");
    }
    parts
}

/// Everything that touches SQLite: the keyword arm, the vector arm's source filter, the
/// fusion, and the corpus size.
///
/// One function so that `Index` — which is not `Send` — is created and dropped without
/// an `await` anywhere near it.
fn retrieve(
    index_path: &std::path::Path,
    args: &SearchArgs,
    mut vector: Vec<(String, f32)>,
) -> anyhow::Result<(Vec<Hit>, usize)> {
    let index = Index::open(index_path)?;

    // Both arms reach `ARM_DEPTH`, not `args.limit`: fusion decides the top n, and an
    // arm that only ever returned ten could not lift a result the other missed.
    let keyword = index.search(&args.query, ARM_DEPTH, args.source.as_deref())?;

    // The vector arm's `--source` post-filter. One query for the whole candidate set,
    // not one per candidate.
    if let Some(source) = args.source.as_deref() {
        let hashes: Vec<String> = vector.iter().map(|(h, _)| h.clone()).collect();
        let kept = index.in_source(&hashes, source)?;
        vector.retain(|(h, _)| kept.contains(h));
        vector.truncate(ARM_DEPTH);
    }

    let depth = RERANK_DEPTH.max(args.limit);
    let hits = fuse(&index, keyword, &vector, depth)?;
    // `chunk_count`, not `stats` — see its doc comment. `stats` sums a text column, which
    // cost six seconds per query on the Tampa corpus for a number in the report footer.
    Ok((hits, index.chunk_count()?))
}

/// Reorders `hits` in place with the cross-encoder, best first.
///
/// `Err(String)` is an ordinary absence — no weights installed — reported the same way
/// the vector arm's is. Reranking is always on (§6.3), but "always on" is a rule about
/// there being no flag to turn it off, not a promise that a machine with no reranker
/// weights should refuse to search at all.
///
/// The RRF order is left untouched on failure, which is the honest fallback: it is the
/// best ordering available without the model, and [`SearchReport::method`] will not
/// claim it was reranked.
async fn rerank_arm(query: &str, hits: &mut [Hit]) -> Result<(), String> {
    if hits.is_empty() {
        return Ok(());
    }

    let query = query.to_string();
    let documents: Vec<String> = hits.iter().map(|h| h.text.clone()).collect();
    // Weights load and inference are both blocking, and both are seconds.
    let scores = tokio::task::spawn_blocking(move || {
        let root = crate::models::models_dir()?;
        crate::rerank::Reranker::load(&root, RERANKER, None)?.score(&query, &documents)
    })
    .await
    .map_err(|e| format!("{e}"))?
    .map_err(|e| format!("{e:#}"))?;

    apply_scores(hits, &scores)
}

/// Puts the reranker's scores onto the hits and sorts by them.
///
/// Separate from [`rerank_arm`] because this is the part with a decision in it, and a
/// test of it should not have to load two gigabytes of weights to reach the decision.
fn apply_scores(hits: &mut [Hit], scores: &[f32]) -> Result<(), String> {
    if scores.len() != hits.len() {
        return Err(format!(
            "the reranker returned {} scores for {} passages",
            scores.len(),
            hits.len()
        ));
    }

    // The score replaces RRF's, because it is the one the ordering now reflects and a
    // result showing a score it was not ranked by is a lie in the one column a reader
    // uses to judge confidence.
    for (hit, score) in hits.iter_mut().zip(scores) {
        hit.score = *score as f64;
    }
    // The chunk hash breaks ties, for the same reason it does in `fuse`.
    hits.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.chunk_hash.cmp(&b.chunk_hash))
    });
    Ok(())
}

/// What the vector arm produced, and how much of the corpus it could see.
struct VectorArm {
    hits: Vec<(String, f32)>,
    stored: usize,
}

/// The vector arm, or the reason there wasn't one.
///
/// Returns `Err(String)` for an ordinary absence — no table yet, no weights installed —
/// because to the caller those are the same fact: this query ran on one arm, and here is
/// what to do about it. A real fault is logged and reported the same way, since a search
/// that fails outright is worse than one that answers with BM25 and says it did.
async fn vector_arm(ctx: &Ctx, args: &SearchArgs) -> Result<VectorArm, String> {
    // The model is a property of the table, not of this caller — see
    // `VectorTable::open_existing`.
    let table = VectorTable::open_existing(&ctx.store.vectors_db())
        .await
        .map_err(|e| format!("{e:#}"))?;
    let stored = table.len().await.map_err(|e| format!("{e:#}"))?;
    if stored == 0 {
        return Err("the vector table is empty — run `centinel embed`".into());
    }

    let query = args.query.clone();
    let model = table.model_id().to_string();
    // The table's model id says where the query must be embedded — a corpus embedded
    // through OpenRouter can only be searched through OpenRouter, because its vectors
    // live in that model's space and no local weights produce them.
    let vector = match crate::remote::backend_for(&model).map_err(|e| format!("{e:#}"))? {
        crate::remote::EmbeddingBackend::Remote(spec) => {
            // The query is the one piece of text this sends off the machine, and it is
            // sent because the operator embedded the corpus remotely — the same consent,
            // read back off the table.
            crate::remote::RemoteEmbedder::new(spec)
                .map_err(|e| format!("{e:#}"))?
                .embed_query(&query)
                .await
                .map_err(|e| format!("{e:#}"))?
        }
        crate::remote::EmbeddingBackend::Local(_) => {
            // Loading weights and running inference are both blocking, and the model is
            // gigabytes — a short CLI run pays this per query; `serve` and `mcp` pay it
            // once.
            tokio::task::spawn_blocking(move || {
                let root = crate::models::models_dir()?;
                crate::embed::Embedder::load(&root, &model, None)?.embed_query(&query)
            })
            .await
            .map_err(|e| format!("{e}"))?
            .map_err(|e| format!("{e:#}"))?
        }
    };

    let depth = match args.source {
        Some(_) => ARM_DEPTH * SOURCE_OVERFETCH,
        None => ARM_DEPTH,
    };
    // The `--source` post-filter is the caller's, because it needs SQLite and this
    // function is the async half.
    let hits = table
        .nearest(&vector, depth)
        .await
        .map_err(|e| format!("{e:#}"))?;

    Ok(VectorArm { hits, stored })
}

/// Reciprocal Rank Fusion — `Σ 1 / (k + rank)`, over the union of both arms.
///
/// Rank-based on purpose: the two arms produce scores on incomparable scales (FTS5's
/// negated BM25 against a cosine similarity), and any attempt to normalise them into
/// one number is a hidden weighting. Ranks are what the arms genuinely agree on.
///
/// Keyword hits arrive already carrying their text and placements, so only the
/// vector-only survivors are looked up — which is why the lookup is here, after the
/// cut, rather than when the vector arm returned.
fn fuse(
    index: &Index,
    keyword: Vec<Hit>,
    vector: &[(String, f32)],
    limit: usize,
) -> anyhow::Result<Vec<Hit>> {
    let mut scores: HashMap<&str, f64> = HashMap::new();
    for (rank, hit) in keyword.iter().enumerate() {
        *scores.entry(hit.chunk_hash.as_str()).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
    }
    for (rank, (hash, _)) in vector.iter().enumerate() {
        *scores.entry(hash.as_str()).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
    }

    let mut ranked: Vec<(&str, f64)> = scores.into_iter().collect();
    // The hash breaks ties, so a query run twice returns the same order. A `HashMap`
    // iterates arbitrarily, and without this two equal-scoring chunks would swap places
    // between runs — which reads as the corpus having changed.
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    ranked.truncate(limit);

    let carried: HashMap<&str, &Hit> = keyword.iter().map(|h| (h.chunk_hash.as_str(), h)).collect();

    let mut out = Vec::with_capacity(ranked.len());
    for (hash, score) in ranked {
        match carried.get(hash) {
            Some(hit) => out.push(Hit {
                score,
                ..(*hit).clone()
            }),
            None => {
                // The two stores drift legitimately: a rebuilt index, or `clear_source`,
                // leaves vectors for chunks SQLite no longer has, and `embed` never
                // removes them. So an unresolvable hash is dropped rather than raised —
                // one stale vector must not fail a whole query — and it is the same rule
                // the keyword path applies to a hit with no placement, for the same
                // reason: a result nothing can cite is worse than no result.
                let placements = index.placements_of(hash)?;
                if placements.is_empty() {
                    tracing::debug!(chunk = %hash, "a vector has no chunk in the index");
                    continue;
                }
                out.push(Hit {
                    chunk_hash: hash.to_string(),
                    text: index.chunk_texts(&[hash.to_string()])?.remove(0),
                    score,
                    placements,
                });
            }
        }
    }
    Ok(out)
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

        // How much of the corpus the vector arm could see. Printed whenever it is not
        // all of it, because RRF gives a rank from a thin pool the weight of a rank from
        // a whole one — so a reader who is not told will read ten confident results as
        // ten results from the corpus.
        if self.no_vectors.is_none() && self.vectors_indexed < self.total_chunks_indexed {
            let share = if self.total_chunks_indexed == 0 {
                0.0
            } else {
                100.0 * self.vectors_indexed as f64 / self.total_chunks_indexed as f64
            };
            // `<0.1%` rather than `0.0%`. A barely-started corpus is the case this line
            // exists for, and rounding its share to zero reads as "no vectors at all" —
            // which is a different fact, and one `no_vectors` already carries.
            let share = match share {
                s if s > 0.0 && s < 0.1 => "<0.1".to_string(),
                s => format!("{s:.1}"),
            };
            let text = format!(
                "the vector arm saw {} of {} chunks ({share}%) — run `centinel embed` for the rest",
                render::count(self.vectors_indexed as u64),
                render::count(self.total_chunks_indexed as u64),
            );
            p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
        }
        if let Some(reason) = &self.no_vectors {
            let text = format!("keyword search only — {reason}");
            p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
        }
        // Reranking is the larger of the two quality steps — BM25 alone measures 14.8
        // nDCG@10 and reranked BM25 measures 33.4 — so its absence is the more important
        // of the two to say out loud, not the lesser.
        if let Some(reason) = &self.no_rerank {
            let text = format!("not reranked — {reason}");
            p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
        }

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

/// How many duplicate placements are listed before the rest become a count.
///
/// Shared boilerplate — a council's standard notice paragraph — can appear on hundreds of
/// pages, and a result that printed all of them would bury the passage it is about.
const ALSO_SHOWN: usize = 3;

/// How many duplicate placements the *result* carries, terminal or not.
///
/// [`ALSO_SHOWN`] only ever governed the terminal, so `--json` and the MCP tool kept the
/// whole list. One Tampa boilerplate passage sat at 630 addresses and turned a five-result
/// search into 108 KB, of which 105 KB was one result's `also_at` — enough to blow an
/// agent's tool-result budget on a query that matched almost nothing worth reading. The
/// count is what the reader actually needs; the addresses are reachable by the chunk hash.
const ALSO_CARRIED: usize = 8;

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
        let name = p.paint(
            &render::truncate(label, p.width().saturating_sub(24)),
            Ink::Bold,
        );
        // Four decimals, not two. The score is no longer a raw BM25 figure in the units
        // of 8.5 — it is whichever of RRF or the reranker last touched it, and both are
        // bounded by 1. RRF separates adjacent ranks in the fourth decimal (1/61 against
        // 1/62), and reranker probabilities saturate near the top, so `{:.2}` printed
        // `1.00` against `1.00` for a result set the model had actually ordered.
        let score = p.paint(&format!("{} · {:.4}", self.source, self.score), Ink::Dim);
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

                // Each with its own handle. A count alone told the reader two more
                // documents carry this passage and gave them no way to reach either —
                // and the hash cannot be guessed from the one above, because a different
                // address is a different document with its own bytes.
                if !self.also_at.is_empty() {
                    // The total, not the length of the list: `also_at` is capped at
                    // `ALSO_CARRIED`, and reporting its length would understate a passage
                    // that sits on six hundred pages as one that sits on eight.
                    let total = self.also_at_total.max(self.also_at.len());
                    let also = format!("also at {}", render::plural(total, "address", "addresses"));
                    p.line(p.paint(&also, Ink::Dim))?;
                    for other in self.also_at.iter().take(ALSO_SHOWN) {
                        let hash = p.paint(&render::short_sha(&other.blob_sha), Ink::Cyan);
                        let where_ =
                            render::truncate_start(&other.url, p.width().saturating_sub(20));
                        p.line(format!("  {hash}  {}", p.paint(&where_, Ink::Dim)))?;
                    }
                    if total > ALSO_SHOWN {
                        let more = format!("  … and {} more", total - ALSO_SHOWN);
                        p.line(p.paint(&more, Ink::Dim))?;
                    }
                }
                Ok(())
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_to_string(report: &SearchReport) -> String {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    /// A 64-character stand-in for a hash, from a seed of any length — the fusion tests
    /// name their chunks `a`, `kw`, `both`, and a fixed repeat count only fits one width.
    fn sha(seed: &str) -> String {
        seed.repeat(64_usize.div_ceil(seed.len()))[..64].to_string()
    }

    fn result(also_at: Vec<AlsoAt>) -> SearchResult {
        SearchResult {
            rank: 1,
            score: 8.5,
            text: "The budget for fiscal year 2026 is adopted.".into(),
            title: Some("Council Agenda".into()),
            heading: "Item 4 · Budget".into(),
            source: "tampa".into(),
            url: "https://tampa.gov/agenda.pdf".into(),
            observed_at: "2026-08-04T10:00:00Z".into(),
            tool: "pdf-inspector 0.1".into(),
            blob_sha: sha("3f8a1c9d0b7e"),
            derived_sha: sha("9b2e4a1f0c33"),
            chunk_hash: sha("aa11bb22cc33"),
            char_start: 100,
            char_end: 143,
            also_at_total: also_at.len(),
            also_at,
        }
    }

    /// A fully-embedded corpus, so the coverage warning stays out of the way of the
    /// rendering assertions that are about something else.
    fn report(results: Vec<SearchResult>) -> SearchReport {
        SearchReport {
            query: "budget".into(),
            method: "bm25+vector→rrf".into(),
            results,
            total_chunks_indexed: 12_400,
            vectors_indexed: 12_400,
            no_vectors: None,
            no_rerank: None,
        }
    }

    /// The handle leads the provenance line, because it is the one thing here you type
    /// back into `open` or `read`.
    #[test]
    fn a_result_leads_its_provenance_with_the_handle() {
        let out = render_to_string(&report(vec![result(Vec::new())]));
        assert!(out.contains("3f8a1c9d0b7e"), "{out}");
        assert!(
            out.contains("Item 4 · Budget"),
            "the heading beats the title: {out}"
        );
        assert!(out.contains("fiscal year 2026"), "{out}");
    }

    /// The defect: a count told the reader two more documents carry this passage and gave
    /// them no way to reach either.
    #[test]
    fn every_duplicate_placement_carries_its_own_handle() {
        let out = render_to_string(&report(vec![result(vec![
            AlsoAt {
                source: "pinellas".into(),
                url: "https://pinellas.gov/minutes.pdf".into(),
                blob_sha: sha("1111aaaa2222"),
            },
            AlsoAt {
                source: "hillsborough".into(),
                url: "https://hcfl.gov/notice.html".into(),
                blob_sha: sha("3333bbbb4444"),
            },
        ])]));

        assert!(out.contains("also at 2 addresses"), "{out}");
        assert!(out.contains("1111aaaa2222"), "{out}");
        assert!(out.contains("3333bbbb4444"), "{out}");
        assert!(out.contains("pinellas.gov/minutes.pdf"), "{out}");
    }

    /// Shared boilerplate can appear on hundreds of pages; listing them all would bury
    /// the passage the result is about.
    #[test]
    fn a_long_duplicate_list_is_capped_and_says_how_many_it_dropped() {
        let many: Vec<AlsoAt> = (0..9)
            .map(|i| AlsoAt {
                source: "tampa".into(),
                url: format!("https://tampa.gov/page-{i}.html"),
                blob_sha: sha(&format!("{i}{i}{i}{i}aaaa2222")),
            })
            .collect();

        let out = render_to_string(&report(vec![result(many)]));
        assert!(out.contains("also at 9 addresses"), "{out}");
        assert!(out.contains("and 6 more"), "{out}");
    }

    // ── fusion ────────────────────────────────────────────────────────────────────

    fn hit(hash: &str, score: f64) -> Hit {
        Hit {
            chunk_hash: sha(hash),
            text: format!("text of {hash}"),
            score,
            placements: Vec::new(),
        }
    }

    /// An index holding exactly the chunks a test names, so `fuse` can look up the ones
    /// only the vector arm found.
    fn indexed(texts: &[(&str, &str)]) -> Index {
        let mut index = Index::in_memory().unwrap();
        for (hash, text) in texts {
            let chunk = crate::chunk::Chunk::new(text.to_string(), 0, String::new(), 0);
            // `Chunk::new` hashes its own text, so the row is inserted and then read back
            // under the hash the test wants by way of a placement-free direct write.
            index
                .insert(
                    &chunk,
                    &crate::index::Placement {
                        source: "test".into(),
                        resource: format!("https://example.gov/{hash}"),
                        blob_sha: "0".repeat(64),
                        derived_sha: "1".repeat(64),
                        ordinal: 0,
                        heading: String::new(),
                        char_start: 0,
                        char_end: text.len(),
                        observed_at: "2026-01-01T00:00:00Z".into(),
                        tool: "test".into(),
                        title: None,
                    },
                )
                .unwrap();
        }
        index
    }

    /// The property RRF exists for: a chunk both arms rank highly beats one that either
    /// arm ranks first alone.
    #[test]
    fn agreement_between_the_arms_beats_confidence_in_one() {
        let index = Index::in_memory().unwrap();
        // `both` is second in each arm; `kw` and `vec` are first in one and absent from
        // the other.
        let keyword = vec![hit("kw", 9.0), hit("both", 8.0)];
        let vector = vec![(sha("vec"), 0.99), (sha("both"), 0.98)];

        let fused = fuse(&index, keyword, &vector, 3).unwrap();
        assert_eq!(fused[0].chunk_hash, sha("both"), "{fused:?}");
        assert!(fused[0].score > fused[1].score);
    }

    /// A chunk only the vector arm found still has to arrive with its text, or the
    /// result is a citation-less score.
    #[test]
    fn a_vector_only_hit_is_given_its_text_and_placements() {
        let index = indexed(&[("a", "the budget for fiscal year 2026")]);
        let hash = index.chunk_hashes().unwrap().remove(0);

        let fused = fuse(&index, Vec::new(), &[(hash.clone(), 0.9)], 5).unwrap();
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].chunk_hash, hash);
        assert_eq!(fused[0].text, "the budget for fiscal year 2026");
        assert_eq!(fused[0].placements.len(), 1, "it can be cited");
    }

    /// With one arm empty, fusion has to be the identity on the other — otherwise a
    /// corpus with no vectors would return a different order than BM25 alone.
    #[test]
    fn one_empty_arm_preserves_the_other_arms_order() {
        let index = Index::in_memory().unwrap();
        let keyword = vec![hit("a", 9.0), hit("b", 8.0), hit("c", 7.0)];

        let fused = fuse(&index, keyword, &[], 10).unwrap();
        let order: Vec<&str> = fused.iter().map(|h| h.chunk_hash.as_str()).collect();
        assert_eq!(order, vec![sha("a"), sha("b"), sha("c")]);
    }

    /// A `HashMap` iterates arbitrarily, so equal scores must be broken deterministically
    /// or the same query returns a different order between runs.
    #[test]
    fn equal_scores_break_ties_the_same_way_every_time() {
        let index = Index::in_memory().unwrap();
        let first = fuse(
            &index,
            vec![hit("b", 1.0), hit("a", 1.0), hit("c", 1.0)],
            &[(sha("a"), 0.5), (sha("c"), 0.5), (sha("b"), 0.5)],
            3,
        )
        .unwrap();
        for _ in 0..8 {
            let again = fuse(
                &index,
                vec![hit("b", 1.0), hit("a", 1.0), hit("c", 1.0)],
                &[(sha("a"), 0.5), (sha("c"), 0.5), (sha("b"), 0.5)],
                3,
            )
            .unwrap();
            let a: Vec<&String> = first.iter().map(|h| &h.chunk_hash).collect();
            let b: Vec<&String> = again.iter().map(|h| &h.chunk_hash).collect();
            assert_eq!(a, b);
        }
    }

    // ── saying what actually ran ──────────────────────────────────────────────────

    /// The defect this guards: RRF weights a rank from a 2,309-vector pool exactly as it
    /// weights a rank from a 397,830-vector one, so a partly embedded corpus returns
    /// confident results and looks identical to a complete one.
    #[test]
    fn a_partly_embedded_corpus_says_how_much_the_vector_arm_saw() {
        let mut r = report(vec![result(Vec::new())]);
        r.vectors_indexed = 2_309;
        r.total_chunks_indexed = 397_830;

        let out = render_to_string(&r);
        assert!(out.contains("2,309"), "{out}");
        assert!(out.contains("397,830"), "{out}");
        assert!(
            out.contains("0.6%"),
            "the share, not just the counts: {out}"
        );
        assert!(out.contains("centinel embed"), "names the fix: {out}");
    }

    /// A barely-started corpus is the case this warning exists for, and `0.0%` reads as
    /// "no vectors at all" — a different fact, and one `no_vectors` already carries.
    /// Measured on the real store at 110 of 397,830.
    #[test]
    fn a_barely_started_corpus_does_not_round_its_share_to_zero() {
        let mut r = report(vec![result(Vec::new())]);
        r.vectors_indexed = 110;
        r.total_chunks_indexed = 397_830;

        let out = render_to_string(&r);
        assert!(out.contains("<0.1%"), "{out}");
        assert!(!out.contains("0.0%"), "{out}");
        assert!(out.contains("110"), "the count is still exact: {out}");
    }

    /// A fully embedded corpus has nothing to warn about, and a warning printed every
    /// time is a warning nobody reads.
    #[test]
    fn a_fully_embedded_corpus_prints_no_coverage_warning() {
        let out = render_to_string(&report(vec![result(Vec::new())]));
        assert!(!out.contains("the vector arm saw"), "{out}");
    }

    /// One arm is a different answer, not a slower one.
    #[test]
    fn a_missing_vector_arm_is_named_rather_than_left_to_inference() {
        let mut r = report(vec![result(Vec::new())]);
        r.method = "bm25".into();
        r.no_vectors = Some("the vector table is empty — run `centinel embed`".into());

        let out = render_to_string(&r);
        assert!(out.contains("keyword search only"), "{out}");
        assert!(out.contains("centinel embed"), "{out}");
        assert!(
            !out.contains("the vector arm saw"),
            "an absent arm is not a thin one: {out}"
        );
    }

    /// The larger of the two quality steps, so its absence is the more important to say.
    #[test]
    fn an_unreranked_ordering_says_so() {
        let mut r = report(vec![result(Vec::new())]);
        r.method = "bm25+vector→rrf".into();
        r.no_rerank = Some("no weights for `qwen3-reranker-0.6b`".into());

        let out = render_to_string(&r);
        assert!(out.contains("not reranked"), "{out}");
        assert!(out.contains("qwen3-reranker-0.6b"), "{out}");
    }

    /// Both score sources are bounded by 1 and separate in the third or fourth decimal —
    /// RRF because 1/61 and 1/62 are close, the reranker because it saturates — so two
    /// decimals printed one number for every row of an ordering the model had worked to
    /// produce.
    #[test]
    fn adjacent_scores_stay_distinguishable() {
        let mut a = result(Vec::new());
        a.score = 0.99711;
        let mut b = result(Vec::new());
        b.score = 0.99541;
        b.rank = 2;

        let out = render_to_string(&report(vec![a, b]));
        assert!(out.contains("0.9971"), "{out}");
        assert!(out.contains("0.9954"), "{out}");

        // And the RRF case, where the separation is one decimal further out.
        let mut c = result(Vec::new());
        c.score = 1.0 / 61.0;
        let out = render_to_string(&report(vec![c]));
        assert!(out.contains("0.0164"), "{out}");
    }

    /// `method` is the field a reader trusts to know which pipeline produced an order,
    /// so it is assembled from what ran rather than written out per call site.
    #[test]
    fn the_method_names_exactly_what_ran() {
        assert_eq!(method(true, true), "bm25+vector→rrf→rerank");
        assert_eq!(method(true, false), "bm25+vector→rrf");
        assert_eq!(method(false, true), "bm25→rerank");
        assert_eq!(method(false, false), "bm25");
    }

    /// The reranker's whole purpose: an order the first stage got wrong is corrected.
    #[test]
    fn reranking_reorders_and_carries_its_own_scores() {
        let mut hits = vec![hit("a", 0.9), hit("b", 0.5), hit("c", 0.1)];
        // The first stage put `a` first; the cross-encoder says `c`.
        apply_scores(&mut hits, &[0.10, 0.42, 0.98]).unwrap();

        let order: Vec<&str> = hits.iter().map(|h| h.chunk_hash.as_str()).collect();
        assert_eq!(order, vec![sha("c"), sha("b"), sha("a")]);
        assert!(
            (hits[0].score - 0.98).abs() < 1e-6,
            "the score shown is the one it was ranked by: {:?}",
            hits[0].score
        );
    }

    /// A mismatch means the scores cannot be trusted to belong to these passages, so the
    /// order is left alone rather than paired up arbitrarily.
    #[test]
    fn a_score_count_mismatch_is_refused_and_changes_nothing() {
        let mut hits = vec![hit("a", 0.9), hit("b", 0.5)];
        let err = apply_scores(&mut hits, &[0.1]).unwrap_err();

        assert!(err.contains("1 scores for 2 passages"), "{err}");
        assert_eq!(hits[0].chunk_hash, sha("a"), "untouched");
        assert_eq!(hits[0].score, 0.9);
    }

    /// Nothing to reorder is not a failure — an empty result set must not report the
    /// reranker as missing.
    #[tokio::test]
    async fn reranking_nothing_is_not_a_failure() {
        let mut none: Vec<Hit> = Vec::new();
        assert!(rerank_arm("anything", &mut none).await.is_ok());
    }

    // ── the whole pipeline, on real weights ───────────────────────────────────────

    /// Both arms, fusion and the reranker against the actual models. Opt-in, because it
    /// loads several gigabytes: `CENTINEL_TEST_MODELS=1 cargo test`.
    ///
    /// This is the seam the unit tests above cannot reach. `fuse` is tested on
    /// hand-built ranks and `VectorTable` on hand-built vectors, but nothing else checks
    /// that a *query string* reaches the embedder, the table and SQLite and comes back
    /// as the passage a person would have picked — which is the only claim `search`
    /// actually makes.
    #[tokio::test]
    async fn a_query_reaches_the_passage_that_answers_it() {
        if std::env::var("CENTINEL_TEST_MODELS").is_err() {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path()).await.unwrap();

        // The vocabulary-gap case the vector arm exists for: the passage that answers
        // the query shares no content word with it.
        let passages = [
            "PWSName: Tampa Water Department. Analyte results for UCMR 5 monitoring, \
             fourth quarter, all sample points within the maximum contaminant level.",
            "Chapter 12 — standards for the keeping of bees within residential zones, \
             including hive setbacks and swarm control.",
            "Notice of public hearing on the proposed vacation of a platted alley south \
             of Columbus Drive.",
        ];
        {
            let mut index = Index::open(store.index_path()).unwrap();
            for (i, text) in passages.iter().enumerate() {
                let chunk = crate::chunk::Chunk::new(text.to_string(), i, String::new(), 0);
                index
                    .insert(
                        &chunk,
                        &crate::index::Placement {
                            source: "tampa".into(),
                            resource: format!("https://tampa.gov/{i}"),
                            blob_sha: "0".repeat(64),
                            derived_sha: "1".repeat(64),
                            ordinal: i,
                            heading: String::new(),
                            char_start: 0,
                            char_end: text.len(),
                            observed_at: "2026-01-01T00:00:00Z".into(),
                            tool: "test".into(),
                            title: None,
                        },
                    )
                    .unwrap();
            }
        }

        let ctx = Ctx::new(store.clone());
        super::super::embed(
            &ctx,
            super::super::EmbedArgs::default(),
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();

        // No shared content word with the water passage — "drinking" and "sampling"
        // appear nowhere in it. BM25 alone cannot reach it.
        let report = search(
            &ctx,
            SearchArgs {
                query: "drinking water sampling results".into(),
                limit: 3,
                source: None,
                snippet_chars: 0,
            },
        )
        .await
        .unwrap();

        assert!(report.no_vectors.is_none(), "{:?}", report.no_vectors);
        assert_eq!(report.vectors_indexed, 3);
        assert!(report.method.contains("vector"), "{}", report.method);
        assert!(
            report.results[0].text.contains("UCMR 5"),
            "the water passage has to win despite sharing no word with the query: {:#?}",
            report.results.iter().map(|r| &r.text).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_results_says_so_plainly() {
        let out = render_to_string(&report(Vec::new()));
        assert!(out.contains("Nothing matched"), "{out}");
        assert!(out.contains("12,400"), "the corpus size is context: {out}");
    }

    /// The span is an offset into a specific extraction, and the result has to say which.
    #[test]
    fn the_span_names_the_blob_it_indexes() {
        let r = result(Vec::new());
        let json = serde_json::to_value(report(vec![r.clone()])).unwrap();
        let back: SearchReport = serde_json::from_value(json).unwrap();
        let got = &back.results[0];

        assert_eq!(got.derived_sha, r.derived_sha);
        assert_eq!(got.char_start, 100);
        assert_eq!(got.char_end, 143);
        assert_eq!(got.chunk_hash, r.chunk_hash, "the vector table key");
        assert_ne!(
            got.derived_sha, got.blob_sha,
            "the span does not index the bytes as served"
        );
    }
}
