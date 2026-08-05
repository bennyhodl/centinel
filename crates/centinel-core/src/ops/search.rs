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
    /// Content hash of the passage — the key the vector cache is written under, and what
    /// makes the same text appearing under two addresses one row rather than two.
    pub chunk_hash: String,
    /// Character span within the derived text.
    pub char_start: usize,
    pub char_end: usize,
    /// Other addresses carrying this identical passage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_at: Vec<AlsoAt>,
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

/// How many duplicate placements are listed before the rest become a count.
///
/// Shared boilerplate — a council's standard notice paragraph — can appear on hundreds of
/// pages, and a result that printed all of them would bury the passage it is about.
const ALSO_SHOWN: usize = 3;

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

                // Each with its own handle. A count alone told the reader two more
                // documents carry this passage and gave them no way to reach either —
                // and the hash cannot be guessed from the one above, because a different
                // address is a different document with its own bytes.
                if !self.also_at.is_empty() {
                    let also = format!(
                        "also at {}",
                        render::plural(self.also_at.len(), "address", "addresses")
                    );
                    p.line(p.paint(&also, Ink::Dim))?;
                    for other in self.also_at.iter().take(ALSO_SHOWN) {
                        let hash = p.paint(&render::short_sha(&other.blob_sha), Ink::Cyan);
                        let where_ =
                            render::truncate_start(&other.url, p.width().saturating_sub(20));
                        p.line(format!("  {hash}  {}", p.paint(&where_, Ink::Dim)))?;
                    }
                    if self.also_at.len() > ALSO_SHOWN {
                        let more = format!("  … and {} more", self.also_at.len() - ALSO_SHOWN);
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

    fn sha(seed: &str) -> String {
        seed.repeat(6)[..64].to_string()
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
            also_at,
        }
    }

    fn report(results: Vec<SearchResult>) -> SearchReport {
        SearchReport {
            query: "budget".into(),
            method: "bm25".into(),
            results,
            total_chunks_indexed: 12_400,
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
        assert_eq!(got.chunk_hash, r.chunk_hash, "the vector cache key");
        assert_ne!(
            got.derived_sha, got.blob_sha,
            "the span does not index the bytes as served"
        );
    }
}
