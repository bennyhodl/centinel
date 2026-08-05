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

use crate::models::{self, Gate, ModelRole};
use std::time::Duration;

use crate::prelude::*;
use crate::tool::Tool;

/// What a missing binary actually costs.
/// Ordered so `Required` sorts first: the rows that can stop a run lead the table.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Need {
    /// Code calls it, and a pipeline stage stops without it.
    Required,
    /// Code calls it, and a stage degrades without it.
    Optional,
    /// **Nothing calls it yet.** SPEC §3.1 lists it because the pipeline that will need
    /// it is specified; that pipeline is not built.
    ///
    /// This variant exists because `pdftoppm` and `tesseract` were reported as required
    /// with zero call sites between them, so a correctly installed machine — one able to
    /// do everything this code can do — was told it was **not ready**. A readiness check
    /// that is wrong in the pessimistic direction is not the safe kind of wrong: it is
    /// the kind people learn to ignore.
    Planned,
}

/// A subprocess dependency Centinel shells out to.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Binary {
    pub name: String,
    pub need: Need,
    /// What this binary is needed for — so a missing one is actionable, not just red.
    pub purpose: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Set when the binary is present but old enough that breakage is expected rather
    /// than surprising.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale: Option<String>,
}

impl Binary {
    fn found(&self) -> bool {
        self.path.is_some()
    }

    /// Whether this binary's absence should make the machine read as not ready.
    fn gates_readiness(&self) -> bool {
        self.need == Need::Required
    }
}

/// A model's weights, as a host dependency.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Weights {
    pub id: String,
    pub role: ModelRole,
    /// Which pipeline stage stops without it. Weights are fatal like a missing binary
    /// (§3.2), but not to the same things — a crawl-only machine needs no Whisper.
    pub gates: Gate,
    /// What this model is needed for — so a missing one is actionable, not just red.
    pub purpose: String,
    /// True when the gate needs this model's **role** filled (§3.2) — which every role in
    /// the registry is. It does not mean *this* model: the registry carries alternates
    /// (`whisper-tiny`, `qwen3-embedding-0.6b`), and any one installed model satisfies
    /// its role. [`GateStatus`] is where that rollup happens.
    pub required: bool,
    pub installed: bool,
    /// The variant that would be loaded. `None` when nothing is installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    pub bytes_present: u64,
    /// Size of the installed variant, or of the one a plain `pull` would fetch.
    pub bytes_total: u64,
    /// An interrupted download is waiting to resume — re-running `pull` continues it.
    pub resumable: bool,
    /// The command that fixes this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

/// Whether one pipeline stage's weights are all present.
///
/// Reported per gate rather than as a single flag because the two stages fail
/// independently and for different people: a machine crawling `.gov` sitemaps never
/// touches Whisper, and one transcribing a backlog offline may not have embedded yet.
/// Collapsing them would make `doctor` say "not ready" to someone whose pipeline works.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GateStatus {
    pub gate: Gate,
    pub ready: bool,
    /// What is unavailable while this gate is shut.
    pub blocks: String,
    /// Model ids still to pull.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    /// The command that opens it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// Per-stage readiness. The field to look at when `ready` is false — it says which
    /// half of the pipeline still works.
    pub gates: Vec<GateStatus>,
    /// True when every *required* binary is present.
    pub binaries_ready: bool,
    /// True when every *required* model is installed.
    pub models_ready: bool,
    /// True when both are. Reported separately as well, because a machine can crawl and
    /// extract with no weights at all — it simply cannot search or transcribe.
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
#[op(group = "host")]
pub async fn doctor(ctx: &Ctx, args: DoctorArgs) -> anyhow::Result<DoctorReport> {
    let mut binaries = vec![
        // Both of these are §3.1 requirements for a pipeline nobody has written. `extract`
        // counts `pages_needing_ocr` and stops there; neither binary has a call site.
        // Reporting them as required told a working machine it was broken, every time.
        probe(
            "pdftoppm",
            Need::Planned,
            "will rasterise PDF pages for OCR — ticket #12",
        )
        .await,
        probe(
            "tesseract",
            Need::Planned,
            "will OCR scanned documents — ticket #12",
        )
        .await,
        probe("yt-dlp", Need::Required, "YouTube acquisition").await,
        probe(
            "ffmpeg",
            Need::Required,
            "decodes audio to 16kHz mono PCM for transcription",
        )
        .await,
        worker_probe(),
    ];
    binaries.sort_by(|a, b| a.need.cmp(&b.need).then(a.name.cmp(&b.name)));

    let models_dir = models::models_dir()?;
    let models: Vec<Weights> = models::REGISTRY
        .iter()
        .map(|spec| weights(spec, &models_dir))
        .collect();
    let gates = gate_statuses(&models);

    let binaries_ready = binaries.iter().all(|b| !b.gates_readiness() || b.found());
    let models_ready = gates.iter().all(|g| g.ready);

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
        ctx.store.count_blobs().await?
    };

    Ok(DoctorReport {
        store_root: ctx.store.root().to_path_buf(),
        blob_count,
        sources,
        binaries,
        models_dir,
        models,
        gates,
        binaries_ready,
        models_ready,
        ready: binaries_ready && models_ready,
    })
}

/// Rolls the per-model view up into per-stage readiness.
///
/// A gate opens when every **role** behind it has *some* installed model — not when
/// every model is installed. The registry deliberately carries alternates (`whisper-tiny`
/// beside `whisper-large-v3-turbo`, `qwen3-embedding-0.6b` beside the 4B), and demanding
/// all of them would report a working machine as broken.
fn gate_statuses(models: &[Weights]) -> Vec<GateStatus> {
    [Gate::Search, Gate::Transcription]
        .into_iter()
        .map(|gate| {
            // For each unfilled role, name the model a user should actually pull: the
            // first the registry lists, which is the preferred one.
            let missing: Vec<String> = ModelRole::ALL
                .into_iter()
                .filter(|role| role.gates() == gate)
                .filter(|role| {
                    !models
                        .iter()
                        .any(|m| m.role == *role && m.required && m.installed)
                })
                .filter_map(|role| {
                    models
                        .iter()
                        .find(|m| m.role == role && m.required)
                        .map(|m| m.id.clone())
                })
                .collect();

            GateStatus {
                gate,
                ready: missing.is_empty(),
                blocks: match gate {
                    Gate::Search => "`centinel embed` and the vector half of `centinel search`",
                    Gate::Transcription => "`centinel transcribe`",
                }
                .to_string(),
                // `models pull` takes one model, so a two-model gap is two commands.
                // Chained rather than listed, because the point is to be pasted. Each
                // command comes from the model's own `fix`, which `models::resolve`
                // wrote — so this cannot drift from what the loader would have said.
                fix: (!missing.is_empty()).then(|| {
                    missing
                        .iter()
                        .filter_map(|id| {
                            models
                                .iter()
                                .find(|m| &m.id == id)
                                .and_then(|m| m.fix.clone())
                        })
                        .collect::<Vec<_>>()
                        .join(" && ")
                }),
                missing,
            }
        })
        .collect()
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
        gates: status.role.gates(),
        purpose: match status.role {
            ModelRole::Embedding => "the vector half of hybrid search",
            ModelRole::Reranker => "reranks retrieved passages; always on",
            ModelRole::Transcription => "turns meeting audio into a timestamped transcript",
            ModelRole::VoiceActivity => {
                "finds the speech, so the transcriber never decodes dead air"
            }
        }
        .to_string(),
        // Missing weights are fatal exactly like a missing binary (§3.2) — but only to
        // their own gate, which is why readiness is reported per stage rather than once.
        required: true,
        installed: status.installed,
        variant: status.active().map(|v| v.variant.clone()),
        bytes_present: variant.bytes_present,
        bytes_total: variant.bytes_total,
        resumable: status.resumable(),
        // Not formatted here. `models::resolve` owns the one spelling of the
        // instruction, and asking it is how this stays the same string the loader
        // would have printed.
        fix: models::resolve(&status.id, status.role, None, root)
            .err()
            .and_then(|e| e.fix().map(str::to_string)),
    }
}

/// Locates the transcription worker.
///
/// Unlike the others this one is *ours* — `cargo build` produces it beside `centinel`.
/// It is reported here anyway because it can genuinely be absent: it links whisper.cpp
/// and so needs a C++ toolchain, which means `cargo build -p centinel` alone leaves it
/// out. Probed by path rather than by `command -v`, since it is normally a sibling of
/// the running executable and not on `PATH` at all.
fn worker_probe() -> Binary {
    let path = crate::transcribe::worker_path().ok();
    Binary {
        name: crate::transcribe::WORKER.to_string(),
        need: Need::Required,
        purpose: "runs whisper.cpp in its own process, out of llama.cpp's ggml".to_string(),
        // Not run for a version: it loads no model to answer, but it is still a process
        // spawn on an op that must stay instant.
        version: None,
        stale: None,
        path: path.map(|p| p.display().to_string()),
    }
}

/// Locates a binary and asks it for its version.
///
/// Version strings are captured rather than parsed: SPEC §3 pins *minimum* versions,
/// but the pinning table is owned by ticket #11 and does not exist yet. Recording the
/// raw string now means the check can be added later without another round of probing.
async fn probe(name: &str, need: Need, purpose: &str) -> Binary {
    let path = which(name).await;
    let version = if path.is_some() {
        version_of(name).await
    } else {
        None
    };
    let stale = version.as_deref().and_then(|v| staleness(name, v));
    Binary {
        name: name.to_string(),
        need,
        purpose: purpose.to_string(),
        path,
        version,
        stale,
    }
}

/// Whether a present binary is old enough to expect trouble from.
///
/// Only `yt-dlp` answers, and only because it is the one dependency whose staleness is a
/// **predictable** failure: it shipped 26 releases in 2025 in emergency clusters and warns
/// at ninety days. `crate::youtube::STALE_AFTER_DAYS` was written down with a comment
/// saying `doctor` would surface it, and then nothing read it — so a run failing against
/// the bot wall could not say *"and your yt-dlp is four months old"*, which is the first
/// thing anybody should check.
fn staleness(name: &str, version: &str) -> Option<String> {
    if name != crate::youtube::YT_DLP {
        return None;
    }
    let days = crate::youtube::staleness_days(version, jiff::Timestamp::now())?;
    (days > crate::youtube::STALE_AFTER_DAYS).then(|| {
        format!(
            "released {days} days ago; yt-dlp warns past {} and breakage is expected",
            crate::youtube::STALE_AFTER_DAYS
        )
    })
}

/// A probe's deadline.
///
/// `doctor` exists to answer "is this machine ready" quickly. A binary that cannot say
/// its own name in ten seconds is a finding in itself, and hanging here would stall the
/// one command someone runs *because* something is already wrong.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

async fn which(name: &str) -> Option<String> {
    let out = Tool::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .timeout(PROBE_TIMEOUT)
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
    // `--version` on stdout. Try both rather than special-casing per tool — and let
    // `Output::first_line` be the one place that knows they disagree about the stream.
    for arg in ["--version", "-v"] {
        let Ok(out) = Tool::new(name)
            .arg(arg)
            .timeout(PROBE_TIMEOUT)
            .output()
            .await
        else {
            continue;
        };
        if let Some(line) = out.first_line() {
            return Some(line);
        }
    }
    None
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// The install bar, made loud.
///
/// SPEC §3 accepts that this project shells out to four binaries and downloads gigabytes
/// of weights, on the condition that the missing-dependency case is *loud*. A wall of
/// JSON is not loud — it is a wall, and a person scanning it for `"path": null` is doing
/// the work the report was supposed to do for them. So the verdict comes first, every row
/// carries a glyph, and the fix command sits directly under the thing it fixes.
impl Render for DoctorReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let verdict = if self.ready {
            p.paint("ready", Ink::Green)
        } else if self.binaries_ready || self.models_ready {
            p.paint("partly ready", Ink::Yellow)
        } else {
            p.paint("not ready", Ink::Red)
        };
        let corpus = format!(
            "{} · {} · {}",
            self.store_root.display(),
            render::plural(self.blob_count as usize, "blob", "blobs"),
            render::plural(self.sources.len(), "source", "sources"),
        );
        p.line(format!("{verdict}  {}", p.paint(&corpus, Ink::Dim)))?;

        p.section("binaries")?;
        p.nest(|p| {
            let mut table = Table::bare(&[Align::Left, Align::Left, Align::Left, Align::Left]);
            for bin in &self.binaries {
                let found = bin.found();
                // Three states, because a missing binary costs three different things.
                // A binary nothing calls yet is not a fault, and painting it red was how
                // `doctor` told a correctly installed machine it was broken.
                let mark = match (found, bin.need) {
                    (true, _) if bin.stale.is_some() => Mark::Warn,
                    (true, _) => Mark::Ok,
                    (false, Need::Required) => Mark::Bad,
                    (false, Need::Optional) => Mark::Warn,
                    (false, Need::Planned) => Mark::None,
                };
                // A binary that answered but reports no version is *present*. Saying
                // "missing" next to a green tick is the one thing this column must never
                // do — `centinel-whisper` has no `--version` and is found by path alone.
                let version = match (&bin.version, found) {
                    (Some(raw), _) => short_version(raw),
                    (None, true) => String::new(),
                    // A binary nothing calls yet is not missing in the sense that
                    // matters, and the word belongs to the rows that can stop a run.
                    (None, false) if bin.need == Need::Planned => "not needed yet".into(),
                    (None, false) => "missing".to_string(),
                };
                let ink = match (found, bin.need) {
                    (true, _) => Ink::Cyan,
                    (false, Need::Required) => Ink::Red,
                    (false, _) => Ink::Dim,
                };
                table.push(vec![
                    Cell::mark(mark),
                    Cell::plain(&bin.name),
                    Cell::new(version, ink),
                    Cell::dim(render::truncate(&bin.purpose, 52)),
                ]);
            }
            p.table(&table)?;

            // Below the table rather than in it: a stale binary is present and working,
            // and the sentence that explains why it might not be is worth the width.
            for bin in self.binaries.iter().filter(|b| b.stale.is_some()) {
                let note = format!("{} {}", bin.name, bin.stale.as_deref().unwrap_or_default());
                p.marked(Mark::Warn, p.paint(&note, Ink::Dim))?;
            }
            Ok(())
        })?;

        p.section("models")?;
        p.nest(|p| {
            let mut table = Table::bare(&[
                Align::Left,
                Align::Left,
                Align::Left,
                Align::Right,
                Align::Left,
            ]);
            for m in &self.models {
                // A partially-downloaded model is neither present nor absent, and saying
                // "missing" would hide the fact that re-running `pull` resumes it.
                let mark = if m.installed {
                    Mark::Ok
                } else if m.resumable {
                    Mark::Warn
                } else {
                    Mark::Bad
                };
                let size = if m.installed || !m.resumable {
                    render::bytes(m.bytes_total)
                } else {
                    format!(
                        "{} / {}",
                        render::bytes(m.bytes_present),
                        render::bytes(m.bytes_total)
                    )
                };
                table.push(vec![
                    Cell::mark(mark),
                    Cell::plain(&m.id),
                    Cell::dim(m.variant.clone().unwrap_or_default()),
                    Cell::new(size, if m.installed { Ink::Plain } else { Ink::Dim }),
                    Cell::dim(m.gates.to_string()),
                ]);
            }
            p.table(&table)
        })?;

        p.section("gates")?;
        p.nest(|p| {
            for gate in &self.gates {
                let mark = Mark::from_ok(gate.ready);
                let head = if gate.ready {
                    gate.gate.to_string()
                } else {
                    format!(
                        "{:<16}{}",
                        gate.gate,
                        p.paint(&format!("blocks {}", gate.blocks), Ink::Dim)
                    )
                };
                p.marked(mark, head)?;
                if let Some(fix) = &gate.fix {
                    p.nest(|p| {
                        let painted = p.paint(fix, Ink::Cyan);
                        p.line(format!("  {painted}"))
                    })?;
                }
            }
            Ok(())
        })
    }
}

/// `ffmpeg version 7.1.1 Copyright (c) 2000-2025 …` is a banner, not a version.
///
/// Probes capture whatever the tool prints, because a banner is the honest record of what
/// answered. Displaying it whole would give one column of a five-column table eighty
/// characters, so the first token that looks like a version wins, and the raw string stays
/// one `--json` away.
fn short_version(raw: &str) -> String {
    let first_line = raw.lines().next().unwrap_or(raw);
    first_line
        .split_whitespace()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()) && w.contains('.'))
        .map(|w| w.trim_end_matches(',').to_string())
        .unwrap_or_else(|| render::truncate(first_line, 24))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary(name: &str, need: Need, found: bool, version: Option<&str>) -> Binary {
        Binary {
            name: name.into(),
            need,
            purpose: "does a thing".into(),
            path: found.then(|| format!("/usr/bin/{name}")),
            version: version.map(str::to_string),
            stale: None,
        }
    }

    /// The defect. `pdftoppm` and `tesseract` are SPEC §3.1 requirements for a pipeline
    /// nobody has written — no call site exists for either — so requiring them told a
    /// machine that can do everything this code does that it was not ready.
    #[test]
    fn a_binary_nothing_calls_yet_does_not_block_readiness() {
        let binaries = [
            binary("pdftoppm", Need::Planned, false, None),
            binary("tesseract", Need::Planned, false, None),
            binary("yt-dlp", Need::Required, true, Some("2026.07.04")),
            binary("ffmpeg", Need::Required, true, Some("7.1.1")),
        ];
        assert!(binaries.iter().all(|b| !b.gates_readiness() || b.found()));
    }

    #[test]
    fn a_binary_that_is_called_and_absent_still_blocks_it() {
        let binaries = [
            binary("pdftoppm", Need::Planned, false, None),
            binary("yt-dlp", Need::Required, false, None),
        ];
        assert!(!binaries.iter().all(|b| !b.gates_readiness() || b.found()));
    }

    /// The rows that can stop a run lead the table.
    #[test]
    fn required_binaries_sort_above_planned_ones() {
        let mut binaries = [
            binary("pdftoppm", Need::Planned, false, None),
            binary("yt-dlp", Need::Required, true, None),
        ];
        binaries.sort_by(|a, b| a.need.cmp(&b.need).then(a.name.cmp(&b.name)));
        assert_eq!(binaries[0].name, "yt-dlp");
    }

    /// `STALE_AFTER_DAYS` was written down with a comment saying `doctor` would surface
    /// it, and then nothing read it.
    #[test]
    fn an_old_yt_dlp_is_named_as_old() {
        // Well past the ninety-day line, whenever this test runs.
        let old = staleness(crate::youtube::YT_DLP, "2019.01.01").unwrap();
        assert!(old.contains("90"), "{old}");
        assert!(old.contains("days ago"), "{old}");
    }

    #[test]
    fn a_fresh_yt_dlp_and_every_other_binary_say_nothing() {
        let today = jiff::Timestamp::now()
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date();
        let fresh = format!("{}.{:02}.{:02}", today.year(), today.month(), today.day());
        assert_eq!(staleness(crate::youtube::YT_DLP, &fresh), None);

        // Only yt-dlp answers: it is the one dependency whose staleness is a predictable
        // failure rather than a guess about somebody's package manager.
        assert_eq!(staleness("ffmpeg", "1.0.0"), None);
        assert_eq!(staleness(crate::youtube::YT_DLP, "nightly"), None);
    }

    /// A stale binary is present and working. Painting it as missing would be a lie, and
    /// saying nothing would waste the one check that explains a bot-wall failure.
    #[test]
    fn a_stale_binary_reads_as_a_warning_not_a_fault() {
        let mut bin = binary("yt-dlp", Need::Required, true, Some("2019.01.01"));
        bin.stale = staleness(crate::youtube::YT_DLP, "2019.01.01");
        assert!(bin.stale.is_some());
        assert!(bin.found(), "it is installed and it works");

        let report = DoctorReport {
            store_root: "/tmp/store".into(),
            blob_count: 0,
            sources: vec![],
            binaries: vec![bin],
            models_dir: "/tmp/models".into(),
            models: vec![],
            gates: vec![],
            binaries_ready: true,
            models_ready: true,
            ready: true,
        };
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("ready"), "{out}");
        assert!(out.contains("days ago"), "the warning is shown: {out}");
    }
    use crate::models::ModelSpec;

    fn embedder() -> &'static ModelSpec {
        models::require("qwen3-embedding-4b").unwrap()
    }

    /// The whole registry as `doctor` would see it against `root`.
    fn survey(root: &std::path::Path) -> Vec<Weights> {
        models::REGISTRY.iter().map(|s| weights(s, root)).collect()
    }

    fn gate(root: &std::path::Path, want: Gate) -> GateStatus {
        gate_statuses(&survey(root))
            .into_iter()
            .find(|g| g.gate == want)
            .expect("every gate is reported")
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

    /// The two stages fail for different people and must fail independently: a machine
    /// crawling `.gov` sitemaps never loads Whisper, and one transcribing a backlog
    /// offline may not have embedded anything yet. One flag would tell both of them
    /// they are broken.
    #[test]
    fn the_gates_open_independently() {
        let dir = tempfile::tempdir().unwrap();

        assert!(!gate(dir.path(), Gate::Search).ready);
        assert!(!gate(dir.path(), Gate::Transcription).ready);

        install(
            models::require("qwen3-embedding-4b").unwrap(),
            "q8_0",
            dir.path(),
        );
        install(
            models::require("qwen3-reranker-0.6b").unwrap(),
            "q8_0",
            dir.path(),
        );

        let search = gate(dir.path(), Gate::Search);
        assert!(search.ready, "still missing: {:?}", search.missing);
        assert_eq!(search.fix, None);

        let transcription = gate(dir.path(), Gate::Transcription);
        assert!(!transcription.ready, "no whisper weights were installed");
        assert!(transcription.blocks.contains("transcribe"));
    }

    /// The registry carries alternates on purpose — `whisper-tiny` for a smoke test,
    /// `qwen3-embedding-0.6b` as an escape hatch. A gate that demanded every model would
    /// call a working machine broken.
    #[test]
    fn any_one_model_of_a_role_opens_its_gate() {
        let dir = tempfile::tempdir().unwrap();

        // The 39M smoke-test model, not the 874 MB default.
        install(models::require("whisper-tiny").unwrap(), "q5_1", dir.path());
        install(models::require("silero-vad").unwrap(), "v5.1.2", dir.path());

        let g = gate(dir.path(), Gate::Transcription);
        assert!(
            g.ready,
            "an alternate must satisfy its role: {:?}",
            g.missing
        );
    }

    /// A gap has to be fixable by pasting, not by reading a list and reassembling it.
    /// `models pull` takes one model, so two missing roles are two commands.
    #[test]
    fn a_shut_gate_names_the_preferred_model_and_a_runnable_command() {
        let dir = tempfile::tempdir().unwrap();
        let g = gate(dir.path(), Gate::Transcription);

        assert_eq!(g.missing, vec!["whisper-large-v3-turbo", "silero-vad"]);
        assert_eq!(
            g.fix.as_deref(),
            Some("centinel models pull whisper-large-v3-turbo && centinel models pull silero-vad")
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
