//! Model weights — the registry, and where they live on disk.
//!
//! SPEC §3.2: weights are fetched by an **explicit** `centinel models pull`, and missing
//! weights are fatal exactly like a missing binary. Nothing here downloads implicitly;
//! no multi-gigabyte transfer may ambush a scheduled 3am run.
//!
//! ## Everything is pinned
//!
//! A [`ModelSpec`] pins the repository, a **commit revision**, and a SHA-256 for every
//! file. Pinning the revision is what makes the digests meaningful: `main` moves, and a
//! digest checked against a moving target is theatre. The digests come from Hugging
//! Face's LFS `oid`, which is the SHA-256 of the file content — verified against a real
//! download, including one reassembled from two separate range requests, since resuming
//! is the whole point of [`download`].
//!
//! ## Why not the store
//!
//! SPEC §5.4 wants the store `rsync`-able and complete on its own. Weights are neither
//! corpus nor provenance — they are a machine-local cache, re-fetchable at any time and
//! byte-identical everywhere. So they live in the OS cache directory, like [`crate::config`]
//! lives outside the store, and a copied corpus does not drag 1.7 GB of ONNX with it.
//!
//! ## Fixing the models is deliberate — for search
//!
//! SPEC §6.2: hardware tiering selects *quantization*, never the model. Two installs
//! that picked different embedding models would produce incompatible vector spaces —
//! corpora that could not be compared or merged, which is fatal when forks are the point.
//! So search has one embedder and one reranker, and [`Variant`] is the only axis that
//! varies by machine.
//!
//! **Transcription is the exception, and the domain model already knew it.** A transcript
//! is text, not a vector: a smaller Whisper yields a worse transcript, never an
//! incomparable one. So model size is a legitimate tier here, and
//! [`crate::domain::ModelTier`] is recorded on every derivation precisely so §4.6's
//! *"this ran on a weaker machine with a smaller whisper tier"* stays answerable.

pub mod download;

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Overrides the cache directory.
pub const ENV_MODELS_DIR: &str = "CENTINEL_MODELS";

/// What a model is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    /// First-stage dense retrieval.
    Embedding,
    /// Second-stage reranking, always on (SPEC §6.3).
    Reranker,
    /// Speech to text.
    Transcription,
    /// Voice activity detection — finds the speech in a recording so the transcriber
    /// never sees the silence between it.
    VoiceActivity,
}

impl ModelRole {
    /// Which pipeline stage stops working without this model.
    ///
    /// Weights are fatal like a missing binary (SPEC §3.2), but they are not fatal to
    /// the *same things*: a machine that only crawls `.gov` sites needs no Whisper
    /// weights, and reporting it unready would be crying wolf — which is exactly what
    /// §3.2's "loud" requirement cannot afford. So readiness is reported per gate.
    pub fn gates(&self) -> Gate {
        match self {
            Self::Embedding | Self::Reranker => Gate::Search,
            Self::Transcription | Self::VoiceActivity => Gate::Transcription,
        }
    }
}

/// A pipeline stage that a set of weights gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    /// Hybrid search: the embedder and the reranker.
    Search,
    /// Turning audio into a transcript: Whisper and its VAD.
    Transcription,
}

impl std::fmt::Display for Gate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(match self {
            Self::Search => "search",
            Self::Transcription => "transcription",
        })
    }
}

impl std::fmt::Display for ModelRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(match self {
            Self::Embedding => "embedding",
            Self::Reranker => "reranker",
            Self::Transcription => "transcription",
            Self::VoiceActivity => "voice-activity",
        })
    }
}

/// One file to fetch, pinned by size and digest.
///
/// `path` is repo-relative **and** disk-relative — the on-disk tree mirrors the repo.
#[derive(Clone, Copy, Debug)]
pub struct ModelFile {
    pub path: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// A quantization of a model.
///
/// Usually one GGUF file, which is self-contained — tokenizer, config and weights in a
/// single blob, which is why nothing here carries a sidecar `tokenizer.json`. A slice
/// rather than a single file because large models are published as shards.
#[derive(Clone, Copy, Debug)]
pub struct Variant {
    pub name: &'static str,
    pub about: &'static str,
    pub files: &'static [ModelFile],
}

impl Variant {
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// A model, its provenance, and every quantization we know how to fetch.
#[derive(Clone, Copy, Debug)]
pub struct ModelSpec {
    /// Stable, lowercase. What `models pull` takes and what a [`crate::domain::ModelTier`]
    /// records, so a derivation names something resolvable years later.
    pub id: &'static str,
    pub role: ModelRole,
    pub about: &'static str,
    /// Hugging Face repository holding the GGUF files.
    pub repo: &'static str,
    /// A commit SHA, never a branch. See the module docs.
    pub revision: &'static str,
    /// The first-party model [`Self::repo`] was converted from, when it is a conversion.
    ///
    /// Qwen publishes GGUF for the embedder but **not** the reranker, so reranker weights
    /// come from a community conversion. Recording the base model keeps that visible
    /// rather than implied — a transparency tool should not be vague about its own
    /// chain of custody (SPEC §6.2.1).
    pub converted_from: Option<&'static str>,
    /// SPDX identifier. SPEC §3.5 binds every operator and fork, so this is load-bearing.
    pub license: &'static str,
    /// Output dimensions, for embedders. Part of §5.2's cache key `(chunk_hash,
    /// model_id, dims)`; `None` for a reranker, which emits a score rather than a vector.
    pub dims: Option<u32>,
    pub default_variant: &'static str,
    pub variants: &'static [Variant],
}

impl ModelSpec {
    /// Resolves a variant name, defaulting to [`Self::default_variant`].
    pub fn variant(&self, name: Option<&str>) -> anyhow::Result<&'static Variant> {
        let wanted = name.unwrap_or(self.default_variant);
        self.variants
            .iter()
            .find(|v| v.name == wanted)
            .ok_or_else(|| {
                let known: Vec<_> = self.variants.iter().map(|v| v.name).collect();
                anyhow::anyhow!(
                    "`{}` has no variant `{wanted}` — try one of: {}",
                    self.id,
                    known.join(", ")
                )
            })
    }

    /// Every file a variant needs.
    pub fn files_for(&self, variant: &'static Variant) -> impl Iterator<Item = &'static ModelFile> {
        variant.files.iter()
    }

    /// Where this model's files go: `<root>/<repo>/<revision>/`.
    ///
    /// Keyed by revision so bumping the pin downloads alongside the old weights rather
    /// than clobbering them — an interrupted upgrade never leaves a half-new model.
    pub fn dir(&self, root: &Path) -> PathBuf {
        root.join(self.repo).join(self.revision)
    }

    /// The download URL for one file.
    pub fn url_for(&self, file: &ModelFile) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.revision, file.path
        )
    }

    /// Total bytes for a variant, including shared files.
    pub fn total_size(&self, variant: &'static Variant) -> u64 {
        self.files_for(variant).map(|f| f.size).sum()
    }
}

/// Every model Centinel knows how to fetch.
///
/// Each variant is a single self-contained file — weights, tokenizer and config in one
/// blob. The container differs by runtime: llama.cpp reads **GGUF**, whisper.cpp still
/// reads the older **GGML** `.bin` and never migrated. Digests are Hugging Face's LFS
/// `oid`, which is the SHA-256 of the file content; that equivalence was checked against
/// a real download, including one reassembled from two separate range requests.
///
/// **Sizes are asymmetric on purpose** (SPEC §6.2). The embedder is fixed at 4B because
/// its cost is paid once per corpus, in hours; the reranker scales with the host because
/// its cost is paid once per query, in milliseconds.
pub static REGISTRY: &[ModelSpec] = &[
    ModelSpec {
        id: "qwen3-embedding-4b",
        role: ModelRole::Embedding,
        about: "Dense retrieval. 2560-dim Matryoshka, 32K context. The default.",
        repo: "Qwen/Qwen3-Embedding-4B-GGUF",
        revision: "f4602530db1d980e16da9d7d3a70294cf5c190be",
        // First-party: Qwen publish this GGUF themselves.
        converted_from: None,
        license: "Apache-2.0",
        dims: Some(2560),
        // Q8_0 over Q4_K_M: the embedder runs once per corpus, so its quantization is
        // amortised over hours of work rather than paid per query. Buying back quality
        // for 1.7 GB of disk is the easy side of that trade.
        default_variant: "q8_0",
        variants: &[
            Variant {
                name: "q8_0",
                about: "8-bit. Near-lossless; the default.",
                files: &[ModelFile {
                    path: "Qwen3-Embedding-4B-Q8_0.gguf",
                    size: 4_279_660_224,
                    sha256: "b60ae5ce2dd6a0b77f82cadf21def1f310a3e10cde380ad0081b07a9d416949d",
                }],
            },
            Variant {
                name: "q6_k",
                about: "6-bit. Smaller, very close to Q8_0.",
                files: &[ModelFile {
                    path: "Qwen3-Embedding-4B-Q6_K.gguf",
                    size: 3_305_684_256,
                    sha256: "7e7693eb2503fff2050a0bd45ce7e4f08c617a3bbe0b5ee25896113d97a9fe51",
                }],
            },
            Variant {
                name: "q5_k_m",
                about: "5-bit. For a machine that cannot hold Q8_0.",
                files: &[ModelFile {
                    path: "Qwen3-Embedding-4B-Q5_K_M.gguf",
                    size: 2_888_936_736,
                    sha256: "9fd05563211c2d69d74abb8769fa92983a102d11575b2517a119b0037dff217c",
                }],
            },
            Variant {
                name: "q4_k_m",
                about: "4-bit. The floor.",
                files: &[ModelFile {
                    path: "Qwen3-Embedding-4B-Q4_K_M.gguf",
                    size: 2_496_703_776,
                    sha256: "2b0cf8f17b4c723c27303015383c27ec4bf2d8314bb677d05e920dd70bb0f16b",
                }],
            },
            Variant {
                name: "f16",
                about: "Half precision. Unquantized reference.",
                files: &[ModelFile {
                    path: "Qwen3-Embedding-4B-f16.gguf",
                    size: 8_049_889_824,
                    sha256: "e8b4e85c8fcc26079d27418cf8d6a16df1a09890cba0966324a97280f91e782c",
                }],
            },
        ],
    },
    ModelSpec {
        id: "qwen3-embedding-0.6b",
        role: ModelRole::Embedding,
        about: "Dense retrieval, 1024-dim. Faster, materially weaker; a different vector space.",
        repo: "Qwen/Qwen3-Embedding-0.6B-GGUF",
        revision: "370f27d7550e0def9b39c1f16d3fbaa13aa67728",
        converted_from: None,
        license: "Apache-2.0",
        dims: Some(1024),
        default_variant: "q8_0",
        // Retained as an escape hatch, not a hardware tier. MTEB English Retrieval puts
        // it at 61.83 against 4B's 68.46, and its vectors live in a different space, so
        // switching costs a full re-embed (§6.2). Useful for a fast smoke test of the
        // pipeline; not for a corpus meant to be searched.
        variants: &[
            Variant {
                name: "q8_0",
                about: "8-bit. Near-lossless.",
                files: &[ModelFile {
                    path: "Qwen3-Embedding-0.6B-Q8_0.gguf",
                    size: 639_150_592,
                    sha256: "06507c7b42688469c4e7298b0a1e16deff06caf291cf0a5b278c308249c3e439",
                }],
            },
            Variant {
                name: "f16",
                about: "Half precision. Unquantized reference.",
                files: &[ModelFile {
                    path: "Qwen3-Embedding-0.6B-f16.gguf",
                    size: 1_197_629_632,
                    sha256: "421a27e58d165478cc7acb984a688c2aa41404968b0203e7cd743ece44c54340",
                }],
            },
        ],
    },
    ModelSpec {
        id: "qwen3-reranker-0.6b",
        role: ModelRole::Reranker,
        about: "Second-stage reranking. 32K context.",
        // Qwen publish no reranker GGUF, so this is a community conversion — by the
        // llama.cpp organisation itself, which is the closest thing to first-party
        // available. The ONNX weights this replaced were community conversions too, so
        // provenance is unchanged rather than degraded (SPEC §6.2.1).
        repo: "ggml-org/Qwen3-Reranker-0.6B-Q8_0-GGUF",
        revision: "a02f48bb4f057028298c21fa033da2b30d7742d5",
        converted_from: Some("Qwen/Qwen3-Reranker-0.6B"),
        license: "Apache-2.0",
        dims: None,
        default_variant: "q8_0",
        variants: &[Variant {
            name: "q8_0",
            about: "8-bit. The only published conversion.",
            files: &[ModelFile {
                path: "qwen3-reranker-0.6b-q8_0.gguf",
                size: 639_153_184,
                sha256: "22c9979ce4fbcdc5acdc310c6641c32797eff1aa980b8f7a2db8a8ea23429a48",
            }],
        }],
    },
    ModelSpec {
        id: "whisper-large-v3-turbo",
        role: ModelRole::Transcription,
        about: "Speech to text. Near-large accuracy at ~8x the speed. The default.",
        repo: "ggerganov/whisper.cpp",
        revision: "5359861c739e955e79d9a303bcbc70fb988958b1",
        converted_from: Some("openai/whisper"),
        license: "MIT",
        dims: None,
        // Q8_0 over the f16: near-lossless at half the download, and unlike the embedder
        // there is no vector space to protect — a transcript is text, and a weaker tier
        // yields a worse transcript rather than an incomparable one.
        default_variant: "q8_0",
        variants: &[
            Variant {
                name: "q8_0",
                about: "8-bit. Near-lossless; the default.",
                files: &[ModelFile {
                    path: "ggml-large-v3-turbo-q8_0.bin",
                    size: 874_188_075,
                    sha256: "317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1",
                }],
            },
            Variant {
                name: "q5_0",
                about: "5-bit. For a machine that cannot hold Q8_0.",
                files: &[ModelFile {
                    path: "ggml-large-v3-turbo-q5_0.bin",
                    size: 574_041_195,
                    sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
                }],
            },
            Variant {
                name: "f16",
                about: "Half precision. Unquantized reference.",
                files: &[ModelFile {
                    path: "ggml-large-v3-turbo.bin",
                    size: 1_624_555_275,
                    sha256: "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
                }],
            },
        ],
    },
    ModelSpec {
        id: "whisper-tiny",
        role: ModelRole::Transcription,
        // Retained for the same reason as `qwen3-embedding-0.6b`: a 32 MB download and
        // seconds of inference make the pipeline testable end to end. Its transcripts
        // are not fit for an archive, which is why [`crate::domain::ModelTier`] records
        // which tier ran — §4.6's "this ran on a weaker machine".
        about: "Speech to text, 39M params. A smoke test for the pipeline, not an archive.",
        repo: "ggerganov/whisper.cpp",
        revision: "5359861c739e955e79d9a303bcbc70fb988958b1",
        converted_from: Some("openai/whisper"),
        license: "MIT",
        dims: None,
        default_variant: "q5_1",
        variants: &[
            Variant {
                name: "q5_1",
                about: "5-bit. 32 MB.",
                files: &[ModelFile {
                    path: "ggml-tiny-q5_1.bin",
                    size: 32_152_673,
                    sha256: "818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7",
                }],
            },
            Variant {
                name: "f16",
                about: "Half precision.",
                files: &[ModelFile {
                    path: "ggml-tiny.bin",
                    size: 77_691_713,
                    sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
                }],
            },
        ],
    },
    ModelSpec {
        id: "silero-vad",
        role: ModelRole::VoiceActivity,
        // The single most on-mission model in this registry. Koenecke et al. (FAccT 2024)
        // measured Whisper fabricating whole sentences in ~1% of transcriptions, 38% of
        // those containing explicit harms, and found the effect tracks **non-vocal
        // duration**. A gavel-to-gavel council recording is close to a worst case for
        // that variable: roll call, recesses, waiting for a speaker to reach the podium.
        // An invented sentence, timestamped and archived as a public record, is worse
        // than no transcript. VAD is how the silence never reaches the decoder.
        about: "Voice activity detection. Keeps Whisper from hallucinating over dead air.",
        repo: "ggml-org/whisper-vad",
        revision: "9ffd54a1e1ee413ddf265af9913beaf518d1639b",
        converted_from: Some("snakers4/silero-vad"),
        license: "MIT",
        dims: None,
        // A model version rather than a quantization — the one place `Variant` carries a
        // different axis. Pinned to the release whisper.cpp's own README documents;
        // `ggml-silero-v6.2.0.bin` exists upstream and is unbenchmarked here.
        default_variant: "v5.1.2",
        variants: &[Variant {
            name: "v5.1.2",
            about: "885 KB. The version whisper.cpp documents.",
            files: &[ModelFile {
                path: "ggml-silero-v5.1.2.bin",
                size: 885_098,
                sha256: "29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf",
            }],
        }],
    },
];

/// Looks up a model by id.
pub fn find(id: &str) -> Option<&'static ModelSpec> {
    REGISTRY.iter().find(|m| m.id == id)
}

/// Looks up a model by id, with a message naming the alternatives.
pub fn require(id: &str) -> anyhow::Result<&'static ModelSpec> {
    find(id).ok_or_else(|| {
        let known: Vec<_> = REGISTRY.iter().map(|m| m.id).collect();
        anyhow::anyhow!(
            "unknown model `{id}` — the registry holds: {}",
            known.join(", ")
        )
    })
}

/// Resolves the weights cache directory. **Does not create it.**
///
/// Order: `$CENTINEL_MODELS`, then the platform cache directory. Written by hand rather
/// than with a `dirs`-style crate because it is fifteen lines and the fallback behaviour
/// on a machine with no `HOME` is a decision worth owning.
///
/// Resolving without creating is what lets `doctor` — a report, which should not leave
/// anything behind — ask where weights would be. The download path creates parents per
/// file anyway, so nothing needs this to have run first.
pub fn models_dir() -> anyhow::Result<PathBuf> {
    Ok(match std::env::var_os(ENV_MODELS_DIR) {
        Some(explicit) if !explicit.is_empty() => PathBuf::from(explicit),
        _ => platform_cache_dir()?.join("centinel").join("models"),
    })
}

fn platform_cache_dir() -> anyhow::Result<PathBuf> {
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| anyhow::anyhow!("HOME is unset; set {ENV_MODELS_DIR}"))?;
        return Ok(PathBuf::from(home).join("Library").join("Caches"));
    }
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(xdg));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
        anyhow::anyhow!("neither XDG_CACHE_HOME nor HOME is set; set {ENV_MODELS_DIR}")
    })?;
    Ok(PathBuf::from(home).join(".cache"))
}

/// What is on disk for one variant.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct VariantStatus {
    pub variant: String,
    pub about: String,
    /// True when every file is present at its pinned size.
    pub installed: bool,
    /// True when this is what `models pull` fetches with no `--variant`.
    pub is_default: bool,
    pub bytes_total: u64,
    /// Bytes already on disk, counting partial downloads. What makes a resumed pull
    /// legible before it starts.
    pub bytes_present: u64,
    /// Files with an interrupted download waiting to resume.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resumable: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

/// What is on disk for one model.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ModelStatus {
    pub id: String,
    pub role: ModelRole,
    pub about: String,
    pub repo: String,
    pub revision: String,
    pub license: String,
    pub dir: PathBuf,
    /// True when at least one variant is fully present.
    pub installed: bool,
    pub variants: Vec<VariantStatus>,
}

impl ModelStatus {
    /// The variant that would actually be loaded: the default if it is installed,
    /// otherwise whichever other one is. A machine that pulled `q4f16` instead of the
    /// default is ready, and a readiness check that said otherwise would be wrong.
    pub fn active(&self) -> Option<&VariantStatus> {
        self.variants
            .iter()
            .find(|v| v.installed && v.is_default)
            .or_else(|| self.variants.iter().find(|v| v.installed))
    }

    /// What `models pull` with no `--variant` would fetch.
    pub fn default_variant(&self) -> &VariantStatus {
        self.variants
            .iter()
            .find(|v| v.is_default)
            .expect("every spec has a default variant, checked in tests")
    }

    /// True when some variant has an interrupted download waiting to resume.
    pub fn resumable(&self) -> bool {
        self.variants.iter().any(|v| !v.resumable.is_empty())
    }
}

/// Inspects the cache for one model.
///
/// Presence is judged by **size**, not by re-hashing: `models list` must stay instant,
/// and a 1.2 GB SHA-256 per invocation would make it seconds. `models verify` is the
/// op that re-hashes, and it is separate for exactly that reason.
pub fn status(spec: &'static ModelSpec, root: &Path) -> ModelStatus {
    let dir = spec.dir(root);

    let variants: Vec<VariantStatus> = spec
        .variants
        .iter()
        .map(|v| {
            let mut bytes_present = 0u64;
            let mut missing = Vec::new();
            let mut resumable = Vec::new();

            for file in spec.files_for(v) {
                let path = dir.join(file.path);
                match std::fs::metadata(&path) {
                    Ok(m) if m.len() == file.size => bytes_present += file.size,
                    _ => {
                        missing.push(file.path.to_string());
                        if let Ok(part) = std::fs::metadata(download::part_path(&path))
                            && part.len() > 0
                        {
                            bytes_present += part.len().min(file.size);
                            resumable.push(file.path.to_string());
                        }
                    }
                }
            }

            VariantStatus {
                variant: v.name.to_string(),
                about: v.about.to_string(),
                installed: missing.is_empty(),
                is_default: v.name == spec.default_variant,
                bytes_total: spec.total_size(v),
                bytes_present,
                resumable,
                missing,
            }
        })
        .collect();

    ModelStatus {
        id: spec.id.to_string(),
        role: spec.role,
        about: spec.about.to_string(),
        repo: spec.repo.to_string(),
        revision: spec.revision.to_string(),
        license: spec.license.to_string(),
        installed: variants.iter().any(|v| v.installed),
        dir,
        variants,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hugging Face orgs that publish their own weights. Anything outside this list is a
    /// conversion and must name its base — see [`ModelSpec::converted_from`].
    const FIRST_PARTY_ORGS: &[&str] = &["Qwen"];

    fn by_role(role: ModelRole) -> Vec<&'static ModelSpec> {
        REGISTRY.iter().filter(|m| m.role == role).collect()
    }

    #[test]
    fn search_has_both_halves_available() {
        assert!(!by_role(ModelRole::Embedding).is_empty());
        assert!(!by_role(ModelRole::Reranker).is_empty());
    }

    /// Transcription needs a Whisper *and* a VAD. Without the VAD the transcriber runs,
    /// but it runs over dead air — the documented hallucination case (see `silero-vad`).
    #[test]
    fn transcription_has_both_halves_available() {
        assert!(!by_role(ModelRole::Transcription).is_empty());
        assert!(!by_role(ModelRole::VoiceActivity).is_empty());
    }

    /// Every gate must be reachable, or `doctor` would report readiness for a stage no
    /// model in the registry can actually serve.
    #[test]
    fn every_gate_has_weights_behind_it() {
        for gate in [Gate::Search, Gate::Transcription] {
            assert!(
                REGISTRY.iter().any(|m| m.role.gates() == gate),
                "nothing in the registry serves `{gate}`"
            );
        }
    }

    /// §5.2 keys the embedding cache `(chunk_hash, model_id, dims)`. Everything else
    /// emits a score, a transcript or a segment boundary, so it has no place in that key
    /// — and an embedder missing its dimensions could not be cached correctly.
    #[test]
    fn only_embedders_declare_dimensions() {
        for spec in REGISTRY {
            match spec.role {
                ModelRole::Embedding => {
                    assert!(spec.dims.is_some(), "{} must declare dims", spec.id)
                }
                _ => assert!(
                    spec.dims.is_none(),
                    "{} does not embed, so it has no dimensions",
                    spec.id
                ),
            }
        }
    }

    /// The registry offers several embedders, and they are **not** interchangeable tiers:
    /// different sizes produce different-width vectors in unrelated spaces, so switching
    /// costs a full re-embed (§6.2). Distinct dimensions are what make that failure loud
    /// rather than silent — a mismatched vector cannot even be compared by accident.
    #[test]
    fn embedders_occupy_distinct_vector_spaces() {
        let embedders = by_role(ModelRole::Embedding);
        let mut dims: Vec<u32> = embedders.iter().filter_map(|m| m.dims).collect();
        let total = dims.len();
        dims.sort_unstable();
        dims.dedup();
        assert_eq!(total, dims.len(), "two embedders share a width: {dims:?}");
    }

    /// A conversion must name what it was converted from. SPEC §6.2.1 accepts community
    /// weights precisely *because* the chain of custody stays visible — a transparency
    /// tool should not be vague about its own provenance.
    ///
    /// Most of the registry is a conversion: Qwen publish no reranker GGUF, whisper.cpp's
    /// GGML files are converted from `openai/whisper`, and the VAD from `snakers4/silero-vad`.
    #[test]
    fn conversions_name_a_distinct_base_model() {
        for spec in REGISTRY {
            let org = spec.repo.split('/').next().expect("repo is `org/name`");
            let first_party = FIRST_PARTY_ORGS.contains(&org);

            match spec.converted_from {
                None => assert!(
                    first_party,
                    "{} comes from `{org}`, which does not publish it — name the base model",
                    spec.id
                ),
                Some(base) => {
                    assert!(
                        !first_party,
                        "{} is first-party; it converts nothing",
                        spec.id
                    );
                    assert!(
                        base.contains('/'),
                        "{} names `{base}`, which is not an `org/name` reference",
                        spec.id
                    );
                    assert_ne!(base, spec.repo, "{} cannot be its own base", spec.id);
                }
            }
        }
    }

    #[test]
    fn every_pinned_file_has_a_full_sha256_and_a_nonzero_size() {
        for spec in REGISTRY {
            for variant in spec.variants {
                for file in spec.files_for(variant) {
                    assert_eq!(
                        file.sha256.len(),
                        64,
                        "{}/{} has a malformed digest",
                        spec.id,
                        file.path
                    );
                    assert!(
                        file.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                        "{}/{} digest is not hex",
                        spec.id,
                        file.path
                    );
                    assert!(file.size > 0, "{}/{} has no size", spec.id, file.path);
                }
            }
        }
    }

    /// A branch name would make the digests meaningless — upstream can rewrite `main`.
    #[test]
    fn revisions_are_commit_shas_not_branches() {
        for spec in REGISTRY {
            assert_eq!(
                spec.revision.len(),
                40,
                "{} is not pinned to a commit",
                spec.id
            );
            assert!(spec.revision.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn every_model_has_a_reachable_default_variant() {
        for spec in REGISTRY {
            let v = spec.variant(None).expect("default variant must resolve");
            assert_eq!(v.name, spec.default_variant);
        }
    }

    #[test]
    fn unknown_variants_name_the_alternatives() {
        let spec = require("qwen3-embedding-4b").unwrap();
        let err = spec.variant(Some("q3_k_xxl")).unwrap_err().to_string();
        assert!(
            err.contains("q8_0"),
            "error should list what is available: {err}"
        );
    }

    #[test]
    fn ids_and_variant_names_are_unique() {
        for spec in REGISTRY {
            let n = REGISTRY.iter().filter(|m| m.id == spec.id).count();
            assert_eq!(n, 1, "duplicate model id `{}`", spec.id);
            for v in spec.variants {
                let n = spec.variants.iter().filter(|o| o.name == v.name).count();
                assert_eq!(n, 1, "duplicate variant `{}` on `{}`", v.name, spec.id);
            }
        }
    }

    /// SPEC §3.5 rejects non-redistributable weights outright; a fork ships these.
    ///
    /// An allow-list rather than a deny-list: a new entry carrying `CC-BY-NC-4.0` or an
    /// OpenRAIL threshold should fail here by default, not pass because nobody thought to
    /// ban it. §3.5 rejected Jina's reranker on exactly that licence.
    #[test]
    fn every_model_is_permissively_licensed() {
        const PERMISSIVE: &[&str] = &["Apache-2.0", "MIT", "BSD-3-Clause"];
        for spec in REGISTRY {
            assert!(
                PERMISSIVE.contains(&spec.license),
                "{} is `{}`, which is not on the redistributable list {PERMISSIVE:?}",
                spec.id,
                spec.license
            );
        }
    }

    #[test]
    fn urls_pin_the_revision_not_a_branch() {
        for spec in REGISTRY {
            let file = &spec.variant(None).unwrap().files[0];
            let url = spec.url_for(file);
            assert!(
                url.contains(spec.revision),
                "url must pin the commit: {url}"
            );
            assert!(!url.contains("/main/"));
        }
    }

    /// Both containers are self-contained — weights, tokenizer and config in one blob —
    /// so a variant is a single file and there are no sidecars to keep beside it.
    ///
    /// The extension is checked because it is what tells the two runtimes apart: a `.gguf`
    /// handed to whisper.cpp, or a `.bin` handed to llama.cpp, fails at load time with a
    /// magic-number error rather than anywhere useful.
    #[test]
    fn a_variant_is_one_self_contained_file_in_its_runtime_s_container() {
        for spec in REGISTRY {
            let expected = match spec.role {
                ModelRole::Embedding | ModelRole::Reranker => ".gguf",
                ModelRole::Transcription | ModelRole::VoiceActivity => ".bin",
            };
            for variant in spec.variants {
                assert_eq!(
                    variant.files.len(),
                    1,
                    "{}/{} is sharded; the loader would need to know",
                    spec.id,
                    variant.name
                );
                assert!(
                    variant.files[0].path.ends_with(expected),
                    "{}/{} is `{}`, but a {} model must be {expected}",
                    spec.id,
                    variant.name,
                    variant.files[0].path,
                    spec.role
                );
            }
        }
    }

    #[test]
    fn an_empty_cache_reports_nothing_installed() {
        let dir = tempfile::tempdir().unwrap();
        let spec = require("qwen3-embedding-4b").unwrap();
        let st = status(spec, dir.path());
        assert!(!st.installed);
        assert!(st.variants.iter().all(|v| v.bytes_present == 0));
        assert!(st.variants.iter().all(|v| !v.missing.is_empty()));
    }

    #[test]
    fn a_part_file_counts_toward_bytes_present_and_is_reported_resumable() {
        let dir = tempfile::tempdir().unwrap();
        let spec = require("qwen3-embedding-4b").unwrap();
        let variant = spec.variant(None).unwrap();

        let target = spec.dir(dir.path()).join(variant.files[0].path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(download::part_path(&target), vec![0u8; 4096]).unwrap();

        let st = status(spec, dir.path());
        let v = st
            .variants
            .iter()
            .find(|v| v.variant == spec.default_variant)
            .unwrap();
        assert_eq!(v.bytes_present, 4096);
        assert!(!v.installed);
        assert_eq!(v.resumable, vec![variant.files[0].path.to_string()]);
    }

    /// A file of the wrong length is not "present" — that is how a truncated download
    /// stops masquerading as a working model.
    #[test]
    fn a_wrong_sized_file_is_missing_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let spec = require("qwen3-embedding-4b").unwrap();
        for file in spec.files_for(spec.variant(None).unwrap()) {
            let p = spec.dir(dir.path()).join(file.path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"truncated").unwrap();
        }
        let st = status(spec, dir.path());
        assert!(!st.installed);
    }

    /// The **only** test in this crate that mutates the environment.
    ///
    /// `$CENTINEL_MODELS` is process-global and the harness runs tests in parallel, so a
    /// second env-mutating test would race this one instead of failing honestly. Both
    /// properties are asserted here rather than split across two tests for that reason;
    /// everything else takes a cache root as an argument.
    #[test]
    fn the_env_var_overrides_the_platform_cache_dir_and_resolving_creates_nothing() {
        let parent = tempfile::tempdir().unwrap();
        let target = parent.path().join("not-yet");

        // SAFETY: no other test in this crate reads or writes this variable.
        unsafe { std::env::set_var(ENV_MODELS_DIR, &target) };
        let resolved = models_dir().unwrap();
        unsafe { std::env::remove_var(ENV_MODELS_DIR) };

        assert_eq!(resolved, target, "the override must win");
        // `doctor` is a report and must leave nothing behind.
        assert!(!target.exists(), "resolving must not have side effects");
    }
}
