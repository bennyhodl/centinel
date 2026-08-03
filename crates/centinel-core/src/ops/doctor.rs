//! `doctor` — is this machine able to run Centinel?
//!
//! SPEC §3 accepts a real install bar: Rust shells out to poppler, tesseract and yt-dlp
//! rather than running a second language runtime, and downloads model weights. That
//! trade is only honest if the missing-dependency case is *loud*, which is what this op
//! is for.
//!
//! Weights are reported here **beside the binaries**, because SPEC §3.2 says missing
//! weights are fatal "exactly like a missing binary". They are also the reason this op
//! matters remotely: [`crate::ops::models`] is host-local, so `doctor` is the only way an
//! agent or an HTTP caller can learn that search is about to fail for want of a model.
//!
//! Presence is judged by file size, never by re-hashing — `doctor` runs before commands
//! and must stay instant. `models verify` is the op that reads every byte.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::models::{self, ModelRole};
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

/// A model's weights, as a host dependency.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Weights {
    pub id: String,
    pub role: ModelRole,
    /// What this model is needed for — so a missing one is actionable, not just red.
    pub purpose: String,
    /// Weights gate search, exactly as a missing binary gates its pipeline stage (§3.2).
    pub required: bool,
    pub installed: bool,
    /// The variant that would be loaded. `None` when nothing is installed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    pub bytes_present: u64,
    /// Size of the installed variant, or of the one a plain `pull` would fetch.
    pub bytes_total: u64,
    /// An interrupted download is waiting to resume — re-running `pull` continues it.
    pub resumable: bool,
    /// The command that fixes this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DoctorReport {
    pub store_root: PathBuf,
    /// Blobs in the pool. Counted by walking `blobs/`, so this is O(corpus) — fine at
    /// spine scale, and a reason to move it behind a flag before the corpus is large.
    pub blob_count: u64,
    pub sources: Vec<String>,
    pub binaries: Vec<Binary>,
    /// Where weights live. Outside the store, because they are neither corpus nor
    /// provenance and an `rsync`-able store should not carry 1.7 GB of ONNX.
    pub models_dir: PathBuf,
    pub models: Vec<Weights>,
    /// True when every *required* binary is present.
    pub binaries_ready: bool,
    /// True when every *required* model is installed.
    pub models_ready: bool,
    /// True when both are. Reported separately as well, because a machine can crawl and
    /// extract with no weights at all — it simply cannot search.
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

    let models_dir = models::models_dir()?;
    let models: Vec<Weights> = models::REGISTRY
        .iter()
        .map(|spec| weights(spec, &models_dir))
        .collect();

    let binaries_ready = binaries.iter().all(|b| !b.required || b.found());
    let models_ready = models.iter().all(|m| !m.required || m.installed);

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
        models_dir,
        models,
        binaries_ready,
        models_ready,
        ready: binaries_ready && models_ready,
    })
}

/// Reports one model's weights as a host dependency.
fn weights(spec: &'static models::ModelSpec, root: &std::path::Path) -> Weights {
    let status = models::status(spec, root);
    // The variant on disk if there is one, else the one a plain `pull` would fetch —
    // so `bytes_total` answers "how big is this" both before and after installing.
    let variant = status.active().unwrap_or_else(|| status.default_variant());

    Weights {
        id: status.id.clone(),
        role: status.role,
        purpose: match status.role {
            ModelRole::Embedding => "the vector half of hybrid search".to_string(),
            ModelRole::Reranker => "reranks retrieved passages; always on".to_string(),
        },
        // Missing weights are fatal exactly like a missing binary (§3.2). They gate
        // search, not collection — which is why `models_ready` is reported on its own.
        required: true,
        installed: status.installed,
        variant: status.active().map(|v| v.variant.clone()),
        bytes_present: variant.bytes_present,
        bytes_total: variant.bytes_total,
        resumable: status.resumable(),
        fix: (!status.installed).then(|| {
            if status.resumable() {
                format!("centinel models pull {} # resumes", status.id)
            } else {
                format!("centinel models pull {}", status.id)
            }
        }),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ModelSpec;

    fn embedder() -> &'static ModelSpec {
        models::require("qwen3-embedding-4b").unwrap()
    }

    /// Fakes an installed variant: every file at its pinned length.
    ///
    /// `set_len` rather than writing bytes — these are 600 MB files, and the filesystem
    /// gives us a sparse one for free. That works precisely *because* `doctor` judges
    /// presence by size; `models verify` is the op that would read the bytes and reject
    /// these, which is the division of labour being relied on here.
    fn install(spec: &'static ModelSpec, variant: &str, root: &std::path::Path) {
        let v = spec.variant(Some(variant)).unwrap();
        for file in spec.files_for(v) {
            let path = spec.dir(root).join(file.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::File::create(&path)
                .unwrap()
                .set_len(file.size)
                .unwrap();
        }
    }

    #[test]
    fn an_empty_cache_reports_a_missing_model_with_the_command_that_fixes_it() {
        let dir = tempfile::tempdir().unwrap();
        let w = weights(embedder(), dir.path());

        assert!(!w.installed);
        assert!(
            w.required,
            "§3.2: missing weights are fatal like a missing binary"
        );
        assert_eq!(w.variant, None);
        assert_eq!(w.bytes_present, 0);
        assert!(
            w.bytes_total > 0,
            "the size must be known before installing"
        );
        assert_eq!(
            w.fix.as_deref(),
            Some("centinel models pull qwen3-embedding-4b")
        );
    }

    #[test]
    fn an_installed_model_reports_its_variant_and_no_fix() {
        let dir = tempfile::tempdir().unwrap();
        let spec = embedder();
        install(spec, "q8_0", dir.path());

        let w = weights(spec, dir.path());
        assert!(w.installed);
        assert_eq!(w.variant.as_deref(), Some("q8_0"));
        assert_eq!(w.bytes_present, w.bytes_total);
        assert_eq!(w.fix, None, "nothing to fix");
        assert!(!w.resumable);
    }

    /// Pulling `q4f16` instead of the default is a working install. A readiness check
    /// that only looked at the default variant would call this machine broken.
    #[test]
    fn a_non_default_variant_still_counts_as_installed() {
        let dir = tempfile::tempdir().unwrap();
        let spec = embedder();
        install(spec, "q4_k_m", dir.path());

        let w = weights(spec, dir.path());
        assert!(w.installed);
        assert_eq!(w.variant.as_deref(), Some("q4_k_m"));
        // The reported size is the installed variant's, not the default's.
        assert_eq!(
            w.bytes_total,
            spec.total_size(spec.variant(Some("q4_k_m")).unwrap())
        );
    }

    /// The interrupted-download case: `doctor` should say the pull will resume, not
    /// silently show it as absent.
    #[test]
    fn a_partial_download_is_reported_as_resumable() {
        let dir = tempfile::tempdir().unwrap();
        let spec = embedder();
        let variant = spec.variant(None).unwrap();

        let target = spec.dir(dir.path()).join(variant.files[0].path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(crate::models::download::part_path(&target), vec![0u8; 8192]).unwrap();

        let w = weights(spec, dir.path());
        assert!(!w.installed);
        assert!(w.resumable);
        assert_eq!(w.bytes_present, 8192, "partial bytes count toward progress");
        assert!(
            w.fix.as_deref().unwrap().contains("resumes"),
            "the hint should say re-running continues rather than restarts: {:?}",
            w.fix
        );
    }

    #[tokio::test]
    async fn the_report_separates_binary_readiness_from_model_readiness() {
        let store = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(crate::store::Store::open(store.path()).await.unwrap());
        let report = doctor(
            &ctx,
            DoctorArgs {
                skip_blob_count: true,
            },
        )
        .await
        .unwrap();

        // Both models are in the registry and both are required.
        assert_eq!(report.models.len(), models::REGISTRY.len());
        assert!(report.models.iter().all(|m| m.required));

        // A machine can crawl and extract with no weights; it just cannot search. The
        // two flags exist so that distinction survives into the report.
        assert_eq!(
            report.ready,
            report.binaries_ready && report.models_ready,
            "`ready` must be the conjunction, not an independent judgement"
        );
        assert!(
            report.models.iter().any(|m| m.role == ModelRole::Embedding)
                && report.models.iter().any(|m| m.role == ModelRole::Reranker),
            "search needs one of each"
        );
    }
}
