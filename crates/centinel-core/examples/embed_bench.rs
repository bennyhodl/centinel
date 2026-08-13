//! Measures embedding throughput on this machine.
//!
//! ```console
//! cargo run --release --example embed_bench
//! cargo run --release --example embed_bench -- --sweep
//! cargo run --release --example embed_bench -- qwen3-embedding-0.6b
//! ```
//!
//! Exists because SPEC §6.2 sizes the embedder on *where the cost lands* — hours per
//! corpus — and that argument only holds if the hours are measurable. It is also how a
//! CUDA or DGX Spark host gets compared to an Apple Silicon one without re-deriving a
//! benchmark each time: same text, same chunk size, same checks.
//!
//! Two checks guard the numbers, because a fast wrong answer is still wrong. The
//! *separation* check catches a broken recipe — wrong pooling or a misapplied
//! instruction prefix produce unit vectors at full speed that mean nothing. The *parity*
//! check catches a lever that moved the vectors: every configuration must land within
//! cosine [`PARITY`] of the plain one-at-a-time path, or its speed does not count.
//!
//! The default run walks batch widths at the standing defaults. `--sweep` walks
//! batch × ubatch × flash attention, and is the run that picks a machine's numbers —
//! where the curve flattens is what belongs in that machine's `centinel.toml`.

use std::time::Instant;

use centinel_core::embed::{Embedder, FlashAttention, SessionOptions, cosine};
use centinel_core::models;

/// `chunk::DEFAULT_TARGET_CHARS`, so the number reflects real indexing work.
const CHUNK_CHARS: usize = 1200;
const RUNS: u32 = 8;
/// The sweep has many cells, so each gets fewer runs than the default curve.
const SWEEP_RUNS: u32 = 4;
/// How close to the plain path every configuration's vectors must land.
const PARITY: f32 = 0.999;

fn main() -> anyhow::Result<()> {
    let mut sweep = false;
    let mut model_id = "qwen3-embedding-4b".to_string();
    for arg in std::env::args().skip(1) {
        if arg == "--sweep" {
            sweep = true;
        } else {
            model_id = arg;
        }
    }

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

    // Distinct texts of one length, as a real batch carries: identical texts would let
    // a crossed `seq_id` — every chunk handed chunk 0's vector — read as full speed and
    // perfect parity.
    let chunks: Vec<String> = (0..128).map(probe).collect();
    println!(
        "\nchunk:      {} chars \u{00d7} {}",
        chunks[0].len(),
        chunks.len()
    );

    // One untimed pass first: the first call warms Metal's pipeline cache, and including
    // that in the mean would flatter or penalise depending on the run count.
    embedder.embed_documents(&[&chunks[0]])?;

    let best = if sweep {
        run_sweep(&embedder, &chunks)?
    } else {
        run_curve(&embedder, &chunks)?
    };

    parity(&embedder, &chunks)?;

    println!("\nat {:.1} chunks/sec (batch {}):", best.1, best.0.batch);
    for (n, label) in [
        (200_000u64, "full Tampa site"),
        (1_000_000, "a million chunks"),
    ] {
        let minutes = n as f64 / best.1 / 60.0;
        if minutes >= 60.0 {
            println!("  {label:16} {n:>7} chunks -> {:.1} hours", minutes / 60.0);
        } else {
            println!("  {label:16} {n:>7} chunks -> {minutes:.1} min");
        }
    }

    Ok(())
}

/// One synthetic agenda item, distinct by index, cut to [`CHUNK_CHARS`].
fn probe(i: usize) -> String {
    format!(
        "Agenda item {i}: The Board of County Commissioners approved the fiscal year \
         budget appropriation for stormwater infrastructure improvements in the amount \
         of $4,{:03},000, allocated across drainage rehabilitation, culvert replacement, \
         and the Sweetwater Creek watershed study. Staff recommends approval and the \
         item was adopted on the consent agenda without further discussion. ",
        i % 1000
    )
    .repeat(4)
    .chars()
    .take(CHUNK_CHARS)
    .collect()
}

/// Chunks per second for one configuration, warmed and averaged.
fn throughput(
    embedder: &Embedder,
    opts: SessionOptions,
    chunks: &[String],
    runs: u32,
) -> anyhow::Result<f64> {
    let texts = &chunks[..opts.batch];
    let mut session = embedder.session(opts)?;
    session.embed(texts)?;
    let started = Instant::now();
    for _ in 0..runs {
        session.embed(texts)?;
    }
    Ok((runs as usize * texts.len()) as f64 / started.elapsed().as_secs_f64())
}

/// The default curve: batch widths at the standing defaults. Where it flattens is the
/// number worth putting in this machine's `centinel.toml`.
fn run_curve(embedder: &Embedder, chunks: &[String]) -> anyhow::Result<(SessionOptions, f64)> {
    let mut best = (SessionOptions::default(), 0.0f64);
    for batch in [1usize, 8, 32, 128] {
        let opts = SessionOptions {
            batch,
            ..SessionOptions::default()
        };
        let per_sec = throughput(embedder, opts, chunks, RUNS)?;
        println!(
            "  batch {batch:>3}:  {:>7.1}ms per chunk  ({per_sec:.1} chunks/sec)",
            1000.0 / per_sec
        );
        if per_sec > best.1 {
            best = (opts, per_sec);
        }
    }
    Ok(best)
}

/// The full grid. A cell that fails — a policy this backend refuses, a width it cannot
/// hold — is printed and skipped, because the grid exists to map this machine, holes
/// included.
fn run_sweep(embedder: &Embedder, chunks: &[String]) -> anyhow::Result<(SessionOptions, f64)> {
    let mut best = (SessionOptions::default(), 0.0f64);
    println!("\nsweep ({SWEEP_RUNS} runs per cell):");
    for flash in [FlashAttention::Disabled, FlashAttention::Auto] {
        for ubatch in [1024u32, 2048, 4096] {
            for batch in [32usize, 64, 128] {
                let opts = SessionOptions {
                    batch,
                    ubatch,
                    flash,
                };
                match throughput(embedder, opts, chunks, SWEEP_RUNS) {
                    Ok(per_sec) => {
                        println!(
                            "  fa {flash:<8?} ubatch {ubatch:>4}  batch {batch:>3}:  \
                             {per_sec:6.1} chunks/sec"
                        );
                        if per_sec > best.1 {
                            best = (opts, per_sec);
                        }
                    }
                    Err(e) => println!(
                        "  fa {flash:<8?} ubatch {ubatch:>4}  batch {batch:>3}:  failed — {e}"
                    ),
                }
            }
        }
    }
    println!(
        "\nbest: fa {:?} · ubatch {} · batch {} — only if it also passes parity below",
        best.0.flash, best.0.ubatch, best.0.batch
    );
    Ok(best)
}

/// Every lever must move the clock and not the vectors: each policy's session output is
/// held against the plain one-at-a-time path, worst pair reported.
fn parity(embedder: &Embedder, chunks: &[String]) -> anyhow::Result<()> {
    let texts = &chunks[..16];
    let mut solo = Vec::with_capacity(texts.len());
    for text in texts {
        solo.push(
            embedder
                .embed_documents(std::slice::from_ref(text))?
                .remove(0),
        );
    }

    println!("\nparity vs the one-at-a-time path (threshold {PARITY}):");
    for flash in [FlashAttention::Disabled, FlashAttention::Auto] {
        let mut session = embedder.session(SessionOptions {
            batch: texts.len(),
            flash,
            ..SessionOptions::default()
        })?;
        let grouped = session.embed(texts)?;
        let worst = grouped
            .iter()
            .zip(&solo)
            .map(|(a, b)| cosine(a, b))
            .fold(f32::INFINITY, f32::min);
        let verdict = if worst >= PARITY {
            "ok"
        } else {
            "FAIL — do not run this policy on this machine"
        };
        println!("  fa {flash:<8?} worst {worst:.5}  {verdict}");
    }
    Ok(())
}
