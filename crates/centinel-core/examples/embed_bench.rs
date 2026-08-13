//! Measures embedding throughput on this machine.
//!
//! ```console
//! cargo run --release --example embed_bench
//! cargo run --release --example embed_bench -- qwen3-embedding-0.6b
//! ```
//!
//! Exists because SPEC §6.2 sizes the embedder on *where the cost lands* — hours per
//! corpus — and that argument only holds if the hours are measurable. It is also how a
//! CUDA or DGX Spark host gets compared to an Apple Silicon one without re-deriving a
//! benchmark each time: same text, same chunk size, same separation check.
//!
//! The separation check is not decoration. Wrong pooling or a misapplied instruction
//! prefix still produce unit vectors at full speed, so a throughput number alone can
//! look excellent while the vectors are meaningless.

use std::time::Instant;

use centinel_core::embed::{Embedder, cosine};
use centinel_core::models;

/// `chunk::DEFAULT_TARGET_CHARS`, so the number reflects real indexing work.
const CHUNK_CHARS: usize = 1200;
const RUNS: u32 = 8;

fn main() -> anyhow::Result<()> {
    let model_id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "qwen3-embedding-4b".to_string());

    let root = models::models_dir()?;
    let load = Instant::now();
    let embedder = Embedder::load(&root, &model_id, None)?;
    let load = load.elapsed();

    println!(
        "model:      {} ({})",
        embedder.model_id(),
        embedder.variant()
    );
    println!("dims:       {}", embedder.dims());
    println!("load:       {load:.2?}");

    // Does it actually retrieve? A fast wrong answer is still wrong.
    let query = embedder.embed_query("how much did the city spend on lobbying")?;
    let docs = embedder.embed_documents(&[
        "The registered lobbyist meeting log for the fourth quarter reports expenditures \
         of $48,000 on outside government-relations counsel.",
        "Solid waste collection occurs weekly on Tuesdays. Place bins at the curb by 6am.",
    ])?;
    let (relevant, irrelevant) = (cosine(&query, &docs[0]), cosine(&query, &docs[1]));
    println!(
        "separation: {relevant:.4} relevant vs {irrelevant:.4} irrelevant  ({:+.4})",
        relevant - irrelevant
    );

    let chunk: String = "The Board of County Commissioners approved the fiscal year budget \
         appropriation for stormwater infrastructure improvements in the amount of \
         $4,275,000, allocated across drainage rehabilitation, culvert replacement, and \
         the Sweetwater Creek watershed study. Staff recommends approval. "
        .repeat(4)
        .chars()
        .take(CHUNK_CHARS)
        .collect();

    // One untimed pass first: the first call warms Metal's pipeline cache, and including
    // that in the mean would flatter or penalise depending on RUNS.
    embedder.embed_documents(&[&chunk])?;
    println!("\nchunk:      {} chars", chunk.len());

    // Batch size matters twice over. A context — and its KV cache — is built per *call*,
    // not per text, so one chunk at a time pays that setup on every chunk. And a batch is
    // one forward pass over all of its chunks, so one chunk at a time also runs a GPU that
    // could carry thirty-two at once. Indexing goes through the batched path, so that is
    // the honest number.
    //
    // The sweep reaches 128 because that is the ceiling `--batch auto` will pick. Where a
    // machine's curve flattens is the number worth putting in its `centinel.toml`.
    let mut best = (1usize, 0.0f64);
    for batch in [1usize, 8, 32, 128] {
        let texts: Vec<&str> = std::iter::repeat_n(chunk.as_str(), batch).collect();
        let started = Instant::now();
        for _ in 0..RUNS {
            embedder.embed_documents(&texts)?;
        }
        let per = started.elapsed().as_secs_f64() / (RUNS as usize * batch) as f64;
        let per_sec = 1.0 / per;
        println!(
            "  batch {batch:>3}:  {:>7.1?}ms per chunk  ({per_sec:.1} chunks/sec)",
            per * 1000.0
        );
        if per_sec > best.1 {
            best = (batch, per_sec);
        }
    }

    println!("\nat {:.1} chunks/sec (batch {}):", best.1, best.0);
    for (n, label) in [(5_000u64, "current corpus"), (200_000, "full Tampa site")] {
        let minutes = n as f64 / best.1 / 60.0;
        if minutes >= 60.0 {
            println!("  {label:16} {n:>7} chunks -> {:.1} hours", minutes / 60.0);
        } else {
            println!("  {label:16} {n:>7} chunks -> {minutes:.1} min");
        }
    }

    Ok(())
}
