//! Compare exact and indexed search on a reproducible synthetic corpus.
//! cargo run --release -p centinel-core --example vector_bench -- 100000 2560
//! Reports warm query latency and recall separately from index construction time.
use std::time::Instant;

use centinel_core::vectors::VectorTable;

fn normalize(values: &[f32]) -> Vec<f32> {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    values.iter().map(|value| value / norm).collect()
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
    let table = VectorTable::open(dir.path(), "synthetic-benchmark", dims).await?;
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
        let entries: Vec<_> = (start..(start + 1000).min(rows))
            .map(|i| (format!("{i:064x}"), vector()))
            .collect();
        table.append(&entries).await?;
    }
    let queries: Vec<_> = (0..20).map(|_| vector()).collect();
    table.nearest(&queries[0], 10).await?;
    let started = Instant::now();
    let mut exact = Vec::new();
    for query in &queries {
        exact.push(table.nearest(query, 10).await?);
    }
    let exact_time = started.elapsed();
    let started = Instant::now();
    table.maintain(true).await?;
    let build_time = started.elapsed();
    table.nearest(&queries[0], 10).await?;
    let started = Instant::now();
    let mut matched = 0;
    for (query, expected) in queries.iter().zip(&exact) {
        let found = table.nearest(query, 10).await?;
        matched += found
            .iter()
            .filter(|(hash, _)| expected.iter().any(|(h, _)| h == hash))
            .count();
    }
    let indexed_time = started.elapsed();
    println!("{rows} rows, {dims} dimensions, {} queries", queries.len());
    println!("index build: {build_time:.2?}");
    println!(
        "exact: {:.2?}/query; indexed: {:.2?}/query; recall@10: {:.1}%",
        exact_time / queries.len() as u32,
        indexed_time / queries.len() as u32,
        matched as f64 / (queries.len() * 10) as f64 * 100.0
    );
    Ok(())
}
