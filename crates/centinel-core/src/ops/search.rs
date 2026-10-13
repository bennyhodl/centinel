//! `search` — ask the corpus a question.
//!
//! **Hybrid**, per SPEC §6: two arms, fused with Reciprocal Rank Fusion, then reranked.
//!
//! ```text
//!   query
//!     ├─ BM25   (SQLite FTS5)                 → top 100
//!     └─ vector ([inference.embed] + Lance)   → top 100
//!           └─ RRF fuse (k=60)                → top 40
//!                 └─ [inference.rerank]       → top n
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
//! ## A search says what ran, not how big the corpus is
//!
//! An absent arm is named in [`SearchReport::no_vectors`] and an absent reranker in
//! [`SearchReport::no_rerank`], rather than either being passed over: a one-armed search
//! is a different answer, not a slower one. What the report does **not** carry is how much
//! of the corpus the vector arm could see. It used to — a chunk count beside the vector
//! count, printed as a share — and the denominator was a `COUNT(*)` over every chunk,
//! which measured 99 s cold at 21.7 million chunks and timed MCP searches out. The number
//! was also wrong in the ways that matter: it ignored `--source` and `--tag`, counted
//! stale vectors as coverage, and read as zero whenever the vector arm failed for any
//! reason. A query that audits the corpus to decorate its result is the design fault;
//! `centinel embed --dry-run` is where coverage is computed, as the set difference it
//! actually is.
//!
//! Every result carries its provenance: source, address, the observation time, the tool
//! that derived the text, and the character span within it (SPEC §6).

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::InferenceConfig;
use crate::index::{Filter, Hit, Index, Placement};
use crate::inference::Capability;
use crate::prelude::*;
use crate::vectors::{Nearest, Space, VectorTable};
use crate::workspace::DocumentId;

/// How deep each arm reaches before fusion (SPEC §6).
const ARM_DEPTH: usize = 100;

/// RRF's rank offset. 60 is the value SPEC §6 pins, from the original TREC work.
///
/// It is what stops rank 1 from dominating: with `k = 60` the gap between rank 1 and
/// rank 2 is small, so agreement between the arms matters more than either arm's
/// confidence — which is the whole reason to fuse rather than to pick.
const RRF_K: f64 = 60.0;

/// How many fused candidates reach the reranker (SPEC §6: "top 30–40").
///
/// The first stage only has to get the right passage into this window; it does not have
/// to rank it. That is why a cheap arm that over-fetches plus a good reranker beats an
/// expensive retriever alone, and why the window is wider than any `--limit` anyone
/// types.
const RERANK_DEPTH: usize = 40;

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

    /// Only documents carrying this tag. Repeatable; every one must be present. A tag is
    /// a yes-or-no question's id, or `choice:option` — `centinel questions` lists them.
    #[arg(long = "tag", value_name = "TAG")]
    #[serde(default)]
    pub tags: Vec<String>,

    /// Leave out documents carrying this tag. Repeatable.
    #[arg(long = "not-tag", value_name = "TAG")]
    #[serde(default)]
    pub not_tags: Vec<String>,

    /// Characters of matched passage to return. 0 returns the whole chunk.
    #[arg(long, default_value_t = 400)]
    #[serde(default = "default_snippet")]
    pub snippet_chars: usize,

    /// Scan every vector instead of using the approximate index. Slower, but useful
    /// for comparing retrieval recall on a large corpus.
    #[arg(long)]
    #[serde(default)]
    pub exact: bool,
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
    /// What the classifier workspace's policy says the cited document is: a yes-or-no
    /// question's id, or `choice:option`. The same words `--tag` takes, so a reader can
    /// turn what they see on one result into the filter for the next search.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
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
    let inference = crate::config::Config::load()?.inference;
    search_with(ctx, args, &inference).await
}

/// [`search`], against the servers `inference` names.
pub(crate) async fn search_with(
    ctx: &Ctx,
    args: SearchArgs,
    inference: &InferenceConfig,
) -> anyhow::Result<SearchReport> {
    // A tag nobody defined is a typo, and a typo that returned nothing would read as a
    // corpus with nothing in it. Checked against the saved questions as they are, before
    // anything is opened or loaded.
    if !args.tags.is_empty() || !args.not_tags.is_empty() {
        let saved = crate::workspace::saved_questions_at(&ctx.store.workspace_questions_path())?;
        let known = crate::workspace::known_tags(&saved);
        for tag in args.tags.iter().chain(&args.not_tags) {
            anyhow::ensure!(
                known.contains(tag),
                "no saved question defines the tag `{tag}` — a tag is a yes-or-no \
                 question's id or `choice:option`; `centinel questions` lists them"
            );
        }
    }

    // Checked before the vector arm so a missing index fails immediately rather than
    // after a round trip to the embedding server. This is a path, not a connection.
    let index_path = ctx.store.require_index()?;
    let started = std::time::Instant::now();

    // A server that is down or an unbuilt table is a normal state, not a failure: the
    // corpus is keyword-searchable long before it is embedded. It degrades to one arm and
    // says so, rather than returning an error a reader cannot act on.
    let filter = Filter {
        source: args.source.as_deref(),
        tags: &args.tags,
        not_tags: &args.not_tags,
    };
    let (vector, no_vectors) = match vector_arm(ctx, inference, &args, &filter).await {
        Ok(hits) => (hits, None),
        Err(reason) => (Vec::new(), Some(reason)),
    };
    match &no_vectors {
        None => tracing::debug!(
            hits = vector.len(),
            ms = started.elapsed().as_millis() as u64,
            "vector arm"
        ),
        Some(reason) => tracing::debug!(reason, "no vector arm"),
    }

    // Every SQLite call is inside here. `Index` owns a `rusqlite::Connection` and so is
    // not `Send`; one merely *alive* across an `await` makes this op's future
    // un-spawnable, whether or not it is used again. Confining it to a sync function is
    // what keeps that impossible rather than merely currently-true.
    //
    // It retrieves to `RERANK_DEPTH`, not to `args.limit`: the reranker's whole value is
    // reordering a set larger than the one returned (§6.3).
    let mut hits = retrieve(&index_path, &args, &filter, vector)?;
    tracing::debug!(
        candidates = hits.len(),
        ms = started.elapsed().as_millis() as u64,
        "retrieved"
    );

    let no_rerank = rerank_arm(inference, &args.query, &mut hits).await.err();
    if let Some(reason) = &no_rerank {
        tracing::debug!(reason, "not reranked");
    }
    hits.truncate(args.limit);

    let method = method(no_vectors.is_none(), no_rerank.is_none());
    // The one line a server's log keeps per search: what was asked, under which filter,
    // what came back and how long it took. The hits themselves are the caller's.
    tracing::info!(
        query = %args.query,
        source = args.source.as_deref().unwrap_or("*"),
        tags = ?args.tags,
        not_tags = ?args.not_tags,
        results = hits.len(),
        %method,
        ms = started.elapsed().as_millis() as u64,
        "searched"
    );

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
                tags: hit.tags,
            })
        })
        .collect();

    Ok(SearchReport {
        query: args.query,
        method,
        results,
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

/// Everything that touches SQLite: the keyword arm, the fusion, and the tags.
///
/// One function so that `Index` — which is not `Send` — is created and dropped without
/// an `await` anywhere near it. One filter for both arms — the keyword arm applies it in
/// SQL here, the vector arm applied it inside Lance — so the two never disagree about
/// which documents are in play.
fn retrieve(
    index_path: &std::path::Path,
    args: &SearchArgs,
    filter: &Filter<'_>,
    vector: Vec<Nearest>,
) -> anyhow::Result<Vec<Hit>> {
    let index = Index::open(index_path)?;

    // Both arms reach `ARM_DEPTH`, not `args.limit`: fusion decides the top n, and an
    // arm that only ever returned ten could not lift a result the other missed.
    let keyword = index.search(&args.query, ARM_DEPTH, filter)?;

    let depth = RERANK_DEPTH.max(args.limit);
    let mut hits = fuse(&index, keyword, &vector, depth, filter)?;
    // The tags ride with the hit, read off the document its first placement cites — the
    // one the result will name — so a reader sees why a filtered hit qualified and what
    // else it is.
    for hit in &mut hits {
        if let Some(primary) = hit.placements.first() {
            hit.tags =
                index.document_tags(&primary.source, &primary.resource, &primary.derived_sha)?;
        }
    }
    Ok(hits)
}

/// Reorders `hits` in place with the cross-encoder, best first.
///
/// `Err(String)` is an ordinary absence — no rerank server answering — reported the same
/// way the vector arm's is. Reranking is always on (§6.3), but "always on" is a rule about
/// there being no flag to turn it off, not a promise that a machine whose rerank server
/// is down should refuse to search at all.
///
/// The RRF order is left untouched on failure, which is the honest fallback: it is the
/// best ordering available without the model, and [`SearchReport::method`] will not
/// claim it was reranked.
async fn rerank_arm(
    inference: &InferenceConfig,
    query: &str,
    hits: &mut [Hit],
) -> Result<(), String> {
    let endpoint = inference.endpoint(Capability::Rerank);
    // Asked before the empty check. Whether the reranker is there is a fact about the
    // setup, not about this query's hits: a search that found nothing has to say the
    // reranker is missing too, or the next search finds something and is worse for it
    // without a word. Presence only — nothing is scored until there are hits.
    endpoint.reachable().await?;
    if hits.is_empty() {
        return Ok(());
    }
    let documents: Vec<String> = hits.iter().map(|h| h.text.clone()).collect();
    let scores = crate::rerank::Reranker::new(endpoint)
        .map_err(|e| format!("{e:#}"))?
        .score(query, &documents)
        .await
        .map_err(|e| format!("{e:#}"))?;
    apply_scores(hits, &scores)
}

/// Puts the reranker's scores onto the hits and sorts by them.
///
/// Separate from [`rerank_arm`] because this is the part with a decision in it, and a
/// test of it should not need a server to reach the decision.
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

/// The vector arm's hits, or the reason there wasn't one.
///
/// Returns `Err(String)` for an ordinary absence — no table yet, a table in the older
/// layout, a table in another space, no server answering — because to the caller those
/// are the same fact: this query ran on one arm, and here is what to do about it. A real
/// fault is reported the same way, since a search that fails outright is worse than one
/// that answers with BM25 and says it did.
async fn vector_arm(
    ctx: &Ctx,
    inference: &InferenceConfig,
    args: &SearchArgs,
    filter: &Filter<'_>,
) -> Result<Vec<Nearest>, String> {
    let table = VectorTable::open_existing(&ctx.store.vectors_db())
        .await
        .map_err(|e| format!("{e:#}"))?;
    if table.is_empty().await.map_err(|e| format!("{e:#}"))? {
        return Err("the vector table is empty — run `centinel embed`".into());
    }

    // A query is only comparable to the table if it is embedded in the table's space. A
    // configured server that is not the one the table records is named, not queried: its
    // vectors would rank the corpus in a space it was never written in.
    let endpoint = inference.endpoint(Capability::Embed);
    let space = Space::of(&endpoint);
    if table.space() != &space {
        return Err(crate::vectors::mismatch(Some(table.space()), &space));
    }
    let vector = crate::embed::Embedder::new(
        endpoint,
        &inference.embed.query_prefix,
        crate::spend::Ledger::new(&ctx.store),
    )
    .map_err(|e| format!("{e:#}"))?
    .with_dims(table.dims())
    .embed_query(&args.query)
    .await
    .map_err(|e| format!("{e:#}"))?;

    // The filter rides inside the query, so the arm asks for exactly its depth in
    // distinct chunks and gets them, however narrow the filter.
    if args.exact {
        table.nearest_exact(&vector, ARM_DEPTH, filter).await
    } else {
        table.nearest(&vector, ARM_DEPTH, filter).await
    }
    .map_err(|e| format!("{e:#}"))
}

/// Puts the document a hit was admitted through first among its placements, so the
/// result cites the document the filter chose rather than whichever sorted first.
fn cite_first(placements: &mut [Placement], document: &DocumentId) {
    if let Some(at) = placements.iter().position(|p| {
        p.source == document.source
            && p.resource == document.resource
            && p.derived_sha == document.derived_sha
    }) {
        placements[..=at].rotate_right(1);
    }
}

/// Reciprocal Rank Fusion — `Σ 1 / (k + rank)`, over the union of both arms.
///
/// Rank-based on purpose: the two arms produce scores on incomparable scales (FTS5's
/// negated BM25 against a cosine similarity), and any attempt to normalise them into
/// one number is a hidden weighting. Ranks are what the arms genuinely agree on.
///
/// Keyword hits arrive already carrying their text and placements, so only the
/// vector-only survivors are looked up — which is why the lookup is here, after the
/// cut, rather than when the vector arm returned — under the same filter, so a shared
/// chunk never cites a document the filter left out. A chunk the vector arm found cites
/// the document it was admitted through first.
fn fuse(
    index: &Index,
    keyword: Vec<Hit>,
    vector: &[Nearest],
    limit: usize,
    filter: &Filter<'_>,
) -> anyhow::Result<Vec<Hit>> {
    let mut scores: HashMap<&str, f64> = HashMap::new();
    for (rank, hit) in keyword.iter().enumerate() {
        *scores.entry(hit.chunk_hash.as_str()).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
    }
    let mut admitted: HashMap<&str, &DocumentId> = HashMap::new();
    for (rank, near) in vector.iter().enumerate() {
        *scores.entry(near.chunk_hash.as_str()).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
        admitted
            .entry(near.chunk_hash.as_str())
            .or_insert(&near.document);
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
        let mut hit = match carried.get(hash) {
            Some(hit) => Hit {
                score,
                ..(*hit).clone()
            },
            None => {
                // The two stores drift legitimately: a rebuilt index, or `clear_source`,
                // leaves vectors for chunks SQLite no longer has until the next `embed`
                // prunes them. So an unresolvable hash is dropped rather than raised —
                // one stale vector must not fail a whole query — and it is the same rule
                // the keyword path applies to a hit with no placement, for the same
                // reason: a result nothing can cite is worse than no result.
                let placements = index.placements_of(hash, filter)?;
                if placements.is_empty() {
                    tracing::debug!(chunk = %hash, "a vector has no chunk in the index");
                    continue;
                }
                Hit {
                    chunk_hash: hash.to_string(),
                    text: index.chunk_texts(&[hash.to_string()])?.remove(0),
                    score,
                    placements,
                    tags: Vec::new(),
                }
            }
        };
        if let Some(document) = admitted.get(hash) {
            cite_first(&mut hit.placements, document);
        }
        out.push(hit);
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
            "{} · {}",
            render::plural(self.results.len(), "result", "results"),
            self.method,
        );
        p.title(&self.query, &aside)?;

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

                // In the words `--tag` takes, so the next search can be typed off this one.
                if !self.tags.is_empty() {
                    let tags = format!("tags  {}", self.tags.join(" · "));
                    p.line(p.paint(&tags, Ink::Dim))?;
                }

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
            tags: Vec::new(),
        }
    }

    /// Both arms ran and nothing is missing, so the rendering assertions that are about
    /// something else have no warning in their way.
    fn report(results: Vec<SearchResult>) -> SearchReport {
        SearchReport {
            query: "budget".into(),
            method: "bm25+vector→rrf".into(),
            results,
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
            tags: Vec::new(),
        }
    }

    /// A vector-arm hit, admitted through a document the fusion tests never cite.
    fn near(hash: &str, similarity: f32) -> Nearest {
        Nearest {
            chunk_hash: sha(hash),
            document: DocumentId {
                source: "test".into(),
                resource: format!("https://example.gov/{hash}"),
                derived_sha: "1".repeat(64),
            },
            similarity,
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
        let vector = vec![near("vec", 0.99), near("both", 0.98)];

        let fused = fuse(&index, keyword, &vector, 3, &Default::default()).unwrap();
        assert_eq!(fused[0].chunk_hash, sha("both"), "{fused:?}");
        assert!(fused[0].score > fused[1].score);
    }

    /// A chunk only the vector arm found still has to arrive with its text, or the
    /// result is a citation-less score.
    #[test]
    fn a_vector_only_hit_is_given_its_text_and_placements() {
        let index = indexed(&[("a", "the budget for fiscal year 2026")]);
        let hash = index.chunk_hashes().unwrap().remove(0);

        let fused = fuse(
            &index,
            Vec::new(),
            &[Nearest {
                chunk_hash: hash.clone(),
                document: DocumentId {
                    source: "test".into(),
                    resource: "https://example.gov/a".into(),
                    derived_sha: "1".repeat(64),
                },
                similarity: 0.9,
            }],
            5,
            &Default::default(),
        )
        .unwrap();
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

        let fused = fuse(&index, keyword, &[], 10, &Default::default()).unwrap();
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
            &[near("a", 0.5), near("c", 0.5), near("b", 0.5)],
            3,
            &Default::default(),
        )
        .unwrap();
        for _ in 0..8 {
            let again = fuse(
                &index,
                vec![hit("b", 1.0), hit("a", 1.0), hit("c", 1.0)],
                &[near("a", 0.5), near("c", 0.5), near("b", 0.5)],
                3,
                &Default::default(),
            )
            .unwrap();
            let a: Vec<&String> = first.iter().map(|h| &h.chunk_hash).collect();
            let b: Vec<&String> = again.iter().map(|h| &h.chunk_hash).collect();
            assert_eq!(a, b);
        }
    }

    // ── saying what actually ran ──────────────────────────────────────────────────

    /// One arm is a different answer, not a slower one.
    #[test]
    fn a_missing_vector_arm_is_named_rather_than_left_to_inference() {
        let mut r = report(vec![result(Vec::new())]);
        r.method = "bm25".into();
        r.no_vectors = Some("the vector table is empty — run `centinel embed`".into());

        let out = render_to_string(&r);
        assert!(out.contains("keyword search only"), "{out}");
        assert!(out.contains("centinel embed"), "{out}");
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

    /// Whether the reranker is there is a fact about the setup, not about the hits. A
    /// search that found nothing used to answer `→rerank` with no reranker anywhere,
    /// because the empty check came before the reranker was looked for.
    #[tokio::test]
    async fn an_empty_result_still_says_the_reranker_is_missing() {
        let config = crate::inference::fake::nothing_listening();
        let mut none: Vec<Hit> = Vec::new();
        let err = rerank_arm(&config, "anything", &mut none)
            .await
            .unwrap_err();
        assert!(
            err.contains(&config.base_url),
            "names where it looked: {err}"
        );
    }

    /// Nothing to reorder is not a failure, and it must not cost a request either.
    #[tokio::test]
    async fn reranking_nothing_asks_the_server_nothing() {
        let server = crate::inference::fake::serve().await;
        let mut none: Vec<Hit> = Vec::new();
        assert!(
            rerank_arm(&server.config(), "anything", &mut none)
                .await
                .is_ok()
        );
        assert_eq!(server.hits("rerank"), 0);
    }

    /// A store with `passages` indexed under `tampa`, one document each.
    async fn corpus(passages: &[&str]) -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path()).await.unwrap();
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
        (dir, Ctx::new(store))
    }

    fn ask(query: &str) -> SearchArgs {
        SearchArgs {
            query: query.into(),
            limit: 3,
            source: None,
            tags: Vec::new(),
            not_tags: Vec::new(),
            snippet_chars: 0,
            exact: false,
        }
    }

    const PASSAGES: [&str; 3] = [
        "PWSName: Tampa Water Department. Analyte results for UCMR 5 monitoring, \
         fourth quarter, all sample points within the maximum contaminant level.",
        "Chapter 12 — standards for the keeping of bees within residential zones, \
         including hive setbacks and swarm control.",
        "Notice of public hearing on the proposed vacation of a platted alley south \
         of Columbus Drive.",
    ];

    // ── the whole pipeline ────────────────────────────────────────────────────────

    /// Both arms, fusion and the reranker, end to end against a server: the query is
    /// embedded there, the table answers, the server reorders, and the report says all
    /// three ran. `fuse` is tested on hand-built ranks and `VectorTable` on hand-built
    /// vectors; this is the seam neither reaches.
    #[tokio::test]
    async fn a_query_reaches_both_servers_and_comes_back_reranked() {
        let server = crate::inference::fake::serve().await;
        let (_dir, ctx) = corpus(&PASSAGES).await;
        super::super::embed::embed_with(
            &ctx,
            Default::default(),
            &server.config(),
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();

        let report = search_with(&ctx, ask("bees hive swarm"), &server.config())
            .await
            .unwrap();
        assert!(report.no_vectors.is_none(), "{:?}", report.no_vectors);
        assert!(report.no_rerank.is_none(), "{:?}", report.no_rerank);
        assert_eq!(report.method, "bm25+vector→rrf→rerank");
        assert!(
            report.results[0].text.contains("bees"),
            "{:#?}",
            report.results
        );
        assert_eq!(server.hits("rerank"), 1);
    }

    /// A table in one space and a server in another: the vector arm says so and stays
    /// out, rather than ranking the corpus with vectors it was never written in.
    #[tokio::test]
    async fn a_server_in_another_space_is_named_not_queried() {
        let wrote = crate::inference::fake::serve().await;
        let (_dir, ctx) = corpus(&PASSAGES).await;
        super::super::embed::embed_with(
            &ctx,
            Default::default(),
            &wrote.config(),
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();

        let other = crate::inference::fake::serve().await;
        let report = search_with(&ctx, ask("bees"), &other.config())
            .await
            .unwrap();
        let reason = report.no_vectors.expect("the vector arm stayed out");
        assert!(reason.contains("--adopt"), "{reason}");
        assert_eq!(other.hits("embeddings"), 0, "the query was not sent");
        assert!(
            report.results[0].text.contains("bees"),
            "BM25 still answers"
        );
    }

    /// The claim the vector arm exists for, against real models: the passage that answers
    /// shares no content word with the query. Opt-in, because it needs the servers
    /// `[inference]` names to be running: `CENTINEL_TEST_INFERENCE=1 cargo test`.
    #[tokio::test]
    async fn a_query_reaches_the_passage_that_answers_it() {
        if std::env::var("CENTINEL_TEST_INFERENCE").is_err() {
            return;
        }
        let inference = crate::config::Config::load().unwrap().inference;
        let (_dir, ctx) = corpus(&PASSAGES).await;
        super::super::embed::embed_with(
            &ctx,
            Default::default(),
            &inference,
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();

        // No shared content word with the water passage — "drinking" and "sampling"
        // appear nowhere in it. BM25 alone cannot reach it.
        let report = search_with(&ctx, ask("drinking water sampling results"), &inference)
            .await
            .unwrap();
        assert!(report.no_vectors.is_none(), "{:?}", report.no_vectors);
        assert!(report.method.contains("vector"), "{}", report.method);
        assert!(
            report.results[0].text.contains("UCMR 5"),
            "the water passage has to win despite sharing no word with the query: {:#?}",
            report.results.iter().map(|r| &r.text).collect::<Vec<_>>()
        );
    }

    /// The tags are printed the way `--tag` takes them, so one result teaches the filter
    /// for the next search.
    #[test]
    fn a_result_prints_its_tags_in_the_words_the_filter_takes() {
        let mut tagged = result(Vec::new());
        tagged.tags = vec!["budget".into(), "record_type:minutes".into()];
        let out = render_to_string(&report(vec![tagged]));
        assert!(out.contains("tags  budget · record_type:minutes"), "{out}");

        let out = render_to_string(&report(vec![result(Vec::new())]));
        assert!(
            !out.contains("tags"),
            "an untagged result has no tag line: {out}"
        );
    }

    /// A tag nobody defined is an error, not an empty result — and it is refused before
    /// the index is opened or a model loaded.
    #[tokio::test]
    async fn an_unknown_tag_is_refused_before_anything_is_opened() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path()).await.unwrap();
        let ctx = Ctx::new(store);
        let error = search_with(
            &ctx,
            SearchArgs {
                tags: vec!["nope".into()],
                ..ask("budget")
            },
            &InferenceConfig::default(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("`nope`"), "{error}");
        assert!(error.contains("centinel questions"), "{error}");
        assert!(
            !ctx.store.index_path().exists(),
            "refused before an index was opened or created"
        );
    }

    #[test]
    fn no_results_says_so_plainly() {
        let out = render_to_string(&report(Vec::new()));
        assert!(out.contains("Nothing matched"), "{out}");
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
