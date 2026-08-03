//! `doctor` — is this machine able to run Centinel?
//!
//! SPEC §3 accepts a real install bar: Rust shells out to poppler, tesseract and yt-dlp
//! rather than running a second language runtime. That trade is only honest if the
//! missing-binary case is *loud*, which is what this op is for.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::prelude::*;

/// A subprocess dependency Centinel shells out to.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Binary {
    pub name: String,
    /// Required binaries gate the pipeline stage that needs them; optional ones degrade it.
    pub required: bool,
    /// What this binary is needed for — so a missing one is actionable, not just red.
    pub purpose: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl Binary {
    fn found(&self) -> bool {
        self.path.is_some()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DoctorReport {
    pub store_root: PathBuf,
    /// Blobs in the pool. Counted by walking `blobs/`, so this is O(corpus) — fine at
    /// spine scale, and a reason to move it behind a flag before the corpus is large.
    pub blob_count: u64,
    pub sources: Vec<String>,
    pub binaries: Vec<Binary>,
    /// True when every *required* binary is present.
    pub ready: bool,
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct DoctorArgs {
    /// Skip counting blobs, which walks the whole pool.
    #[arg(long)]
    #[serde(default)]
    pub skip_blob_count: bool,
}

/// Report host readiness: required binaries, store location, corpus size.
#[op]
pub async fn doctor(ctx: &Ctx, args: DoctorArgs) -> anyhow::Result<DoctorReport> {
    let mut binaries = vec![
        probe(
            "pdftoppm",
            true,
            "rasterises PDF pages for OCR — Rust cannot do this natively",
        )
        .await,
        probe("tesseract", true, "OCR for scanned documents").await,
        probe("yt-dlp", true, "YouTube acquisition").await,
        probe("ffmpeg", false, "audio extraction for transcription").await,
    ];
    binaries.sort_by(|a, b| b.required.cmp(&a.required).then(a.name.cmp(&b.name)));

    let ready = binaries.iter().all(|b| !b.required || b.found());

    let sources = ctx
        .store
        .sources()
        .await?
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    let blob_count = if args.skip_blob_count {
        0
    } else {
        count_blobs(ctx.store.root()).await?
    };

    Ok(DoctorReport {
        store_root: ctx.store.root().to_path_buf(),
        blob_count,
        sources,
        binaries,
        ready,
    })
}

/// Locates a binary and asks it for its version.
///
/// Version strings are captured rather than parsed: SPEC §3 pins *minimum* versions,
/// but the pinning table is owned by ticket #11 and does not exist yet. Recording the
/// raw string now means the check can be added later without another round of probing.
async fn probe(name: &str, required: bool, purpose: &str) -> Binary {
    let path = which(name).await;
    let version = if path.is_some() {
        version_of(name).await
    } else {
        None
    };
    Binary {
        name: name.to_string(),
        required,
        purpose: purpose.to_string(),
        path,
        version,
    }
}

async fn which(name: &str) -> Option<String> {
    let out = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}

async fn version_of(name: &str) -> Option<String> {
    // poppler's tools print their version to stderr under `-v`; most others use
    // `--version` on stdout. Try both rather than special-casing per tool.
    for arg in ["--version", "-v"] {
        let Ok(out) = tokio::process::Command::new(name).arg(arg).output().await else {
            continue;
        };
        let merged = if out.stdout.is_empty() {
            &out.stderr
        } else {
            &out.stdout
        };
        if let Some(line) = String::from_utf8_lossy(merged).lines().next() {
            let line = line.trim();
            if !line.is_empty() {
                return Some(line.to_string());
            }
        }
    }
    None
}

/// Walks `blobs/ab/cd/*`, counting files.
async fn count_blobs(root: &std::path::Path) -> anyhow::Result<u64> {
    let blobs = root.join("blobs");
    let mut count = 0u64;

    let mut lvl1 = match tokio::fs::read_dir(&blobs).await {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    while let Some(a) = lvl1.next_entry().await? {
        if !a.file_type().await?.is_dir() {
            continue;
        }
        let mut lvl2 = tokio::fs::read_dir(a.path()).await?;
        while let Some(b) = lvl2.next_entry().await? {
            if !b.file_type().await?.is_dir() {
                continue;
            }
            let mut lvl3 = tokio::fs::read_dir(b.path()).await?;
            while let Some(f) = lvl3.next_entry().await? {
                // Skip in-flight `.<sha>.tmp` writes.
                if f.file_type().await?.is_file()
                    && !f.file_name().to_string_lossy().starts_with('.')
                {
                    count += 1;
                }
            }
        }
    }
    Ok(count)
}
