//! Compare exact and indexed search on a reproducible synthetic corpus, plain and
//! filtered to one source of ten.
//! cargo run --release -p centinel-core --example vector_bench -- 100000 2560
//! Reports warm query latency and recall separately from index construction time. The
//! filtered figure is the one to read before narrowing a search by source: an IVF
//! prefilter skips distance work for the rows it masks but still reads the probed
//! partitions, so expect it near the plain latency, not a tenth of it.
use std::time::Instant;

use centinel_core::index::Filter;
use centinel_core::store::Store;
use centinel_core::vectors::{Nearest, Row, Space, VectorLock, VectorTable};
use centinel_core::workspace::{DocState, DocumentId};

/// Sources the rows are spread over, so a filter to one keeps a tenth.
const SOURCES: usize = 10;

fn normalize(values: &[f32]) -> Vec<f32> {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    values.iter().map(|value| value / norm).collect()
}

/// How many of `found` the exact search also found.
fn recall(found: &[Nearest], expected: &[Nearest]) -> usize {
    found
        .iter()
        .filter(|hit| expected.iter().any(|e| e.chunk_hash == hit.chunk_hash))
        .count()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let rows = args
        .first()
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(10000usize);
    let dims = args
        .get(1)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(64usize);
    anyhow::ensure!(
        rows >= 4096 && dims > 0,
        "use at least 4096 rows and a positive width"
    );
    let dir = tempfile::tempdir()?;
    let space = Space {
        model: "synthetic-benchmark".to_string(),
        endpoint: None,
    };
    let table = VectorTable::open(dir.path(), &space, dims).await?;
    let lock = VectorLock::take(&Store::at(dir.path())).await?;
    let mut state = 42u64;
    let mut vector = || {
        normalize(
            &(0..dims)
                .map(|_| {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    (state >> 32) as u32 as f32 / u32::MAX as f32 - 0.5
                })
                .collect::<Vec<_>>(),
        )
    };
    for start in (0..rows).step_by(1000) {
        let batch: Vec<Row> = (start..(start + 1000).min(rows))
            .map(|i| Row {
                chunk_hash: format!("{i:064x}"),
                document: DocumentId {
                    source: format!("source-{}", i % SOURCES),
                    resource: format!("https://example.gov/{i}"),
                    derived_sha: format!("{i:064x}"),
                },
                state: DocState::default(),
                vector: vector(),
            })
            .collect();
        table.append(&lock, &batch).await?;
    }
    let queries: Vec<_> = (0..20).map(|_| vector()).collect();
    let plain = Filter::default();
    let one_source = Filter {
        source: Some("source-3"),
        ..Filter::default()
    };

    table.nearest(&queries[0], 10, &plain).await?;
    let started = Instant::now();
    let mut exact = Vec::new();
    for query in &queries {
        exact.push(table.nearest(query, 10, &plain).await?);
    }
    let exact_time = started.elapsed();
    let started = Instant::now();
    let mut exact_filtered = Vec::new();
    for query in &queries {
        exact_filtered.push(table.nearest(query, 10, &one_source).await?);
    }
    let exact_filtered_time = started.elapsed();

    let started = Instant::now();
    table.maintain(&lock, true).await?;
    let build_time = started.elapsed();
    table.nearest(&queries[0], 10, &plain).await?;

    let started = Instant::now();
    let mut matched = 0;
    for (query, expected) in queries.iter().zip(&exact) {
        matched += recall(&table.nearest(query, 10, &plain).await?, expected);
    }
    let indexed_time = started.elapsed();
    let started = Instant::now();
    let mut matched_filtered = 0;
    for (query, expected) in queries.iter().zip(&exact_filtered) {
        matched_filtered += recall(&table.nearest(query, 10, &one_source).await?, expected);
    }
    let indexed_filtered_time = started.elapsed();

    println!("{rows} rows, {dims} dimensions, {} queries", queries.len());
    println!("index build: {build_time:.2?}");
    println!(
        "plain:              exact {:.2?}/query; indexed {:.2?}/query; recall@10: {:.1}%",
        exact_time / queries.len() as u32,
        indexed_time / queries.len() as u32,
        matched as f64 / (queries.len() * 10) as f64 * 100.0
    );
    println!(
        "one source of {SOURCES}: exact {:.2?}/query; indexed {:.2?}/query; recall@10: {:.1}%",
        exact_filtered_time / queries.len() as u32,
        indexed_filtered_time / queries.len() as u32,
        matched_filtered as f64 / (queries.len() * 10) as f64 * 100.0
    );
    Ok(())
}
