//! Brute-force vector search over the embedding cache.
//!
//! ```console
//! cargo run --release --example vector_search -- "drinking water sampling results"
//! ```
//!
//! Two things this exists to demonstrate, both of which are load-bearing claims made
//! elsewhere and neither of which should be taken on faith:
//!
//! 1. **The cache is usable on its own.** It is Tier A — bytes, portable, independent of
//!    any search backend (SPEC §5.2). If a linear scan over it answers queries, then
//!    LanceDB really is a deferrable optimisation rather than a prerequisite.
//! 2. **Vectors close the vocabulary gap.** `search "drinking water sampling results"`
//!    returns nothing from FTS5 on this corpus: the water report says `PWSName`,
//!    `Analyte`, `UCMR 5`, and the only chunk containing "drinking" is a tax table about
//!    *Drinking Places (Alcoholic Beverages)*. BM25 is behaving correctly and is still
//!    useless, which is the case the vector arm is for.
//!
//! Not the shipped search path — that is hybrid, RRF-fused and reranked (§6.1).

use std::time::Instant;

use centinel_core::embed::{Embedder, cosine};
use centinel_core::index::Index;
use centinel_core::{models, vectors::VectorCache};

fn main() -> anyhow::Result<()> {
    let query = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "drinking water sampling results".to_string());
    let root = std::path::PathBuf::from(
        std::env::var("CENTINEL_ROOT").unwrap_or_else(|_| ".centinel".into()),
    );

    let model = "qwen3-embedding-4b";
    let dims = models::require(model)?.dims.unwrap() as usize;
    let cache = VectorCache::open(&root, model, dims)?;

    let load = Instant::now();
    let stored = cache.load_all()?;
    let load = load.elapsed();
    anyhow::ensure!(
        !stored.is_empty(),
        "nothing cached yet — run `centinel embed`"
    );
    println!(
        "cache:  {} vectors × {dims} dims  ({:.1} MiB, loaded in {load:.2?})",
        stored.len(),
        (stored.len() * dims * 4) as f64 / (1 << 20) as f64
    );

    let embedder = Embedder::load(&models::models_dir()?, model, None)?;
    let q = embedder.embed_query(&query)?;

    // The scan the whole argument rests on. Vectors are L2-normalized, so cosine is a
    // dot product and this is one pass of multiply-accumulate over contiguous memory.
    let scan = Instant::now();
    let mut scored: Vec<(f32, &String)> = stored
        .iter()
        .map(|(hash, v)| (cosine(&q, v), hash))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let scan = scan.elapsed();

    println!("scan:   {scan:.2?} for {} vectors\n", stored.len());
    println!("query:  {query:?}\n");

    let index = Index::open(root.join("centinel.db"))?;
    for (score, hash) in scored.iter().take(5) {
        let text = index.chunk_texts(std::slice::from_ref(*hash))?;
        let snippet: String = text[0]
            .chars()
            .filter(|c| *c != '\n')
            .take(96)
            .collect::<String>()
            .trim()
            .to_string();
        let where_from = index
            .placements_of(hash)?
            .first()
            .map(|p| p.resource.clone())
            .unwrap_or_default();
        println!("  {score:.4}  {snippet}…");
        println!("          {where_from}");
    }

    Ok(())
}
