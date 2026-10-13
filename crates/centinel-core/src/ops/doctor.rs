//! `doctor` — is this machine able to run Centinel?
//!
//! SPEC §3 accepts a real install bar: Rust shells out to poppler, tesseract and yt-dlp
//! rather than running a second language runtime, and models run on servers the operator
//! starts. That trade is only honest if the missing-dependency case is *loud*, which is
//! what this op is for.
//!
//! The inference servers are reported here **beside the binaries**, because a server that
//! is not answering stops a stage exactly like a missing binary does. They are also the
//! reason this op matters remotely: `doctor` is the way an agent or an HTTP caller learns
//! that search is about to fall back to keywords for want of an embedding server.
//!
//! ## Nothing here walks the corpus
//!
//! `doctor` is a question about the **machine**, and it must answer at the speed of a
//! `command -v`: it is the first thing typed when something is already wrong, and often
//! the thing typed before every other command. So presence is judged by file size, never
//! by re-hashing; the probes run at once rather than in a queue behind a Python
//! interpreter's start-up; and the store is asked only what one `readdir` answers.
//!
//! Counting the pool used to happen here, and it is the one thing on this report that grows
//! with the corpus — a second of directory walking, every time, to print a number nobody
//! ran `doctor` to see. [`crate::ops::status`] is the one that counts what is stored. A
//! server is asked whether it is listening — one TCP connect each, all at once — and never
//! sent a request, which would cost a model call.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use std::time::Duration;

use crate::config::InferenceConfig;
use crate::inference::{Capability, Endpoint};
use crate::prelude::*;
use crate::tool::Tool;
use crate::vectors::{Space, VectorTable};

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

/// A pipeline stage that a set of servers gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    /// Hybrid search: the embedder and the reranker.
    Search,
    /// Turning audio into a transcript.
    Transcription,
}

impl Gate {
    fn of(capability: Capability) -> Self {
        match capability {
            Capability::Embed | Capability::Rerank => Self::Search,
            Capability::Transcribe => Self::Transcription,
        }
    }
}

impl std::fmt::Display for Gate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(match self {
            Self::Search => "search",
            Self::Transcription => "transcription",
        })
    }
}

/// One `[inference]` capability, as a host dependency.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Service {
    /// `embed`, `rerank` or `transcribe` — the `[inference.…]` table that configures it.
    pub capability: String,
    /// Which pipeline stage stops without it. A crawl-only machine needs no transcription
    /// server, and reporting it unready for want of one would be crying wolf.
    pub gates: Gate,
    /// What it is needed for — so a missing one is actionable, not just red.
    pub purpose: String,
    pub base_url: String,
    pub model: String,
    pub ready: bool,
    /// Why it is not: nothing listening, or a named key not set. Says what to do about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// The stored vectors, and whether the configured embedding server is the one that wrote
/// them.
///
/// A server can be up and still be the wrong one: a table written by another model, or by
/// the in-process embedder of an earlier release, refuses its queries, and search answers on
/// keywords alone. That is the state an upgrade leaves a store in, and nothing else on this
/// report would show it. One manifest read, so it costs `doctor` milliseconds.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct VectorsStatus {
    pub path: PathBuf,
    /// What the table records. Absent when it could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub written_by: Option<Space>,
    /// What `[inference.embed]` names. Absent when the config did not load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured: Option<Space>,
    pub ready: bool,
    /// Why not, and what to run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// Whether one pipeline stage's servers are all answering.
///
/// Reported per gate rather than as a single flag because the two stages fail
/// independently and for different people: a machine crawling `.gov` sitemaps never
/// transcribes, and one transcribing a backlog may not have an embedding server yet.
/// Collapsing them would make `doctor` say "not ready" to someone whose pipeline works.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GateStatus {
    pub gate: Gate,
    pub ready: bool,
    /// What is unavailable while this gate is shut.
    pub blocks: String,
    /// Capabilities not answering.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DoctorReport {
    pub store_root: PathBuf,
    /// The config file in effect, or `None` when none was found and defaults are in use.
    ///
    /// Reported because the config is what names the store root, so "which store am I
    /// looking at" and "which file decided that" are one question. A run collecting into
    /// a root nobody expected is otherwise silent — the store is simply empty, and
    /// nothing on screen says which of four paths was read to get there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<PathBuf>,
    /// Where a config file is looked for, nearest first. Shown only when none was found.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config_searched: Vec<PathBuf>,
    /// Why the config file did not load. Reported rather than raised: `doctor` is what
    /// somebody runs *because* something is wrong, and a config that no longer parses is
    /// the most common reason after an upgrade. Reaches the report over HTTP and MCP, and
    /// on the CLI under `--root`; without it, the CLI reads the config to find the store
    /// first and stops there with the same message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_error: Option<String>,
    /// The Sources the store holds a log for — one `readdir`, and the cheapest evidence
    /// that the root above is the one being collected into. **How much** is in it is
    /// [`crate::ops::status`]'s question, and answering it here cost a walk of the pool.
    pub sources: Vec<String>,
    pub binaries: Vec<Binary>,
    /// The `[inference]` servers, one per capability. Empty when the config did not load.
    pub services: Vec<Service>,
    /// The vector table, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vectors: Option<VectorsStatus>,
    /// Per-stage readiness. The field to look at when `ready` is false — it says which
    /// half of the pipeline still works.
    pub gates: Vec<GateStatus>,
    /// True when every *required* binary is present.
    pub binaries_ready: bool,
    /// True when every server is answering.
    pub services_ready: bool,
    /// True when the config loads and both of the above hold. Reported separately as well,
    /// because a machine can crawl and extract with no server at all — it simply cannot
    /// search semantically or transcribe.
    pub ready: bool,
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct DoctorArgs {}

/// Report host readiness: required binaries, inference servers, store location.
#[op(group = "host")]
pub async fn doctor(ctx: &Ctx, _args: DoctorArgs) -> anyhow::Result<DoctorReport> {
    // At once, not in turn. Every probe is a process spawn and they wait on nothing but
    // each other — and `yt-dlp` is a Python program whose interpreter takes longer to
    // start than the other five answer in together, so a queue behind it is most of what
    // this op used to cost.
    let mut binaries: Vec<Binary> = futures::future::join_all([
        // Both of these are §3.1 requirements for a pipeline nobody has written. `extract`
        // counts `pages_needing_ocr` and stops there; neither binary has a call site.
        // Reporting them as required told a working machine it was broken, every time.
        probe(
            "pdftoppm",
            Need::Planned,
            "will rasterise PDF pages for OCR — ticket #12",
        ),
        probe(
            "tesseract",
            Need::Planned,
            "will OCR scanned documents — ticket #12",
        ),
        // Optional, not planned: `extract` calls this one. Without it a PDF whose text
        // layer `pdf-inspector` cannot decode yields nothing instead of its text — a
        // degraded stage, which is exactly what Optional means.
        probe(
            "pdftotext",
            Need::Optional,
            "second reader for PDFs the primary makes nothing of",
        ),
        probe("yt-dlp", Need::Required, "YouTube acquisition"),
        probe(
            "ffmpeg",
            Need::Required,
            "decodes audio to 16 kHz WAV for the transcription server",
        ),
    ])
    .await;
    binaries.sort_by(|a, b| a.need.cmp(&b.need).then(a.name.cmp(&b.name)));

    let (services, configured, config_error) = match crate::config::Config::load() {
        Ok(config) => (
            survey(&config.inference).await,
            Some(Space::of(&config.inference.endpoint(Capability::Embed))),
            None,
        ),
        Err(e) => (Vec::new(), None, Some(format!("{e:#}"))),
    };
    let vectors = vectors_status(&ctx.store, configured).await;
    let gates = gate_statuses(&services, vectors.as_ref());

    let binaries_ready = binaries.iter().all(|b| !b.gates_readiness() || b.found());
    let services_ready = config_error.is_none() && gates.iter().all(|g| g.ready);

    let sources = ctx
        .store
        .sources()
        .await?
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    let config = crate::config::Config::locate();
    let config_searched = match config {
        Some(_) => Vec::new(),
        None => crate::config::Config::search_paths(),
    };

    Ok(DoctorReport {
        store_root: ctx.store.root().to_path_buf(),
        config,
        config_searched,
        config_error,
        sources,
        binaries,
        services,
        vectors,
        gates,
        binaries_ready,
        services_ready,
        ready: binaries_ready && services_ready,
    })
}

/// Every capability's server, asked at once whether it is there.
pub(crate) async fn survey(inference: &InferenceConfig) -> Vec<Service> {
    futures::future::join_all(
        Capability::ALL
            .into_iter()
            .map(|capability| service(inference.endpoint(capability))),
    )
    .await
}

/// One server, as a host dependency: a key it is owed, and something listening.
async fn service(endpoint: Endpoint) -> Service {
    let problem = match endpoint.key() {
        Err(e) => Some(format!("{e:#}")),
        Ok(_) => endpoint.reachable().await.err(),
    };
    Service {
        capability: endpoint.capability.name().to_string(),
        gates: Gate::of(endpoint.capability),
        purpose: match endpoint.capability {
            Capability::Embed => "the vector half of hybrid search",
            Capability::Rerank => "reranks retrieved passages; always on",
            Capability::Transcribe => "turns meeting audio into a timestamped transcript",
        }
        .to_string(),
        base_url: endpoint.base_url,
        model: endpoint.model,
        ready: problem.is_none(),
        problem,
    }
}

/// The vector table's space against the configured one, or `None` when there is no table.
async fn vectors_status(store: &Store, configured: Option<Space>) -> Option<VectorsStatus> {
    let path = store.vectors_path();
    if !path.exists() {
        return None;
    }
    let (written_by, problem) = match VectorTable::open_existing(&store.vectors_db()).await {
        Err(e) => (None, Some(format!("{e:#}"))),
        Ok(table) => {
            let written_by = table.space().clone();
            let problem = match &configured {
                Some(configured) if *configured != written_by => Some(format!(
                    "[inference.embed] names {configured}, so search cannot use these \
                     vectors. If it serves the same model, `centinel embed --adopt --dry-run` \
                     measures whether it reproduces them"
                )),
                _ => None,
            };
            (Some(written_by), problem)
        }
    };
    Some(VectorsStatus {
        path,
        ready: problem.is_none() && configured.is_some(),
        written_by,
        configured,
        problem,
    })
}

/// Rolls the per-server view up into per-stage readiness. Search also needs the vector table
/// to be in the configured server's space, when there is a table.
fn gate_statuses(services: &[Service], vectors: Option<&VectorsStatus>) -> Vec<GateStatus> {
    [Gate::Search, Gate::Transcription]
        .into_iter()
        .map(|gate| {
            let mut missing: Vec<String> = services
                .iter()
                .filter(|s| s.gates == gate && !s.ready)
                .map(|s| s.capability.clone())
                .collect();
            if gate == Gate::Search && vectors.is_some_and(|v| !v.ready) {
                missing.push("vectors".to_string());
            }
            GateStatus {
                gate,
                // No services at all is a config that did not load: nothing is known to
                // answer, so nothing is ready.
                ready: !services.is_empty() && missing.is_empty(),
                blocks: match gate {
                    Gate::Search => "`centinel embed` and the vector half of `centinel search`",
                    Gate::Transcription => "`centinel transcribe`",
                }
                .to_string(),
                missing,
            }
        })
        .collect()
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
/// SPEC §3 accepts that this project shells out to four binaries and sends its models'
/// work to servers, on the condition that the missing-dependency case is *loud*. A wall
/// of JSON is not loud — it is a wall, and a person scanning it for `"path": null` is
/// doing the work the report was supposed to do for them. So the verdict comes first,
/// every row carries a glyph, and what to do sits directly under the thing it fixes.
impl Render for DoctorReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let verdict = if self.ready {
            p.paint("ready", Ink::Green)
        } else if self.binaries_ready || self.services_ready {
            p.paint("partly ready", Ink::Yellow)
        } else {
            p.paint("not ready", Ink::Red)
        };
        let corpus = format!(
            "{} · {}",
            self.store_root.display(),
            render::plural(self.sources.len(), "source", "sources"),
        );
        p.line(format!("{verdict}  {}", p.paint(&corpus, Ink::Dim)))?;

        // Directly under the root, because it is the line that explains it. When no file
        // was found the search list goes in its place: "which four paths did you look
        // in" is the only useful next question, and answering it here saves a trip to
        // the documentation.
        let provenance = match &self.config {
            Some(path) => format!("config {}", path.display()),
            None => format!(
                "no config file — looked in {}",
                self.config_searched
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        p.line(p.paint(&provenance, Ink::Dim))?;
        if let Some(error) = &self.config_error {
            p.marked(Mark::Bad, p.paint(&render::one_line(error), Ink::Red))?;
        }

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
                // do.
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

        p.section("inference")?;
        p.nest(|p| {
            let mut table = Table::bare(&[
                Align::Left,
                Align::Left,
                Align::Left,
                Align::Left,
                Align::Left,
            ]);
            for s in &self.services {
                table.push(vec![
                    Cell::mark(Mark::from_ok(s.ready)),
                    Cell::plain(&s.capability),
                    Cell::new(&s.model, if s.ready { Ink::Cyan } else { Ink::Red }),
                    Cell::dim(&s.base_url),
                    Cell::dim(s.gates.to_string()),
                ]);
            }
            p.table(&table)?;
            // Below the table rather than in it: the sentence that says what to do is worth
            // the width. It repeats the capability's name because a table renders as one
            // unit, and the row it belongs to may be three lines up.
            for s in self.services.iter().filter(|s| !s.ready) {
                if let Some(problem) = &s.problem {
                    let note = format!("{}  {}", s.capability, p.paint(problem, Ink::Dim));
                    p.marked(Mark::Bad, note)?;
                }
            }
            Ok(())
        })?;

        if let Some(v) = &self.vectors {
            p.section("vectors")?;
            p.nest(|p| {
                let written_by = match &v.written_by {
                    Some(space) => space.to_string(),
                    None => "unreadable".to_string(),
                };
                let line = format!("chunks.lance  {}", p.paint(&written_by, Ink::Dim));
                p.marked(Mark::from_ok(v.ready), line)?;
                if let Some(problem) = &v.problem {
                    p.nest(|p| p.wrapped(&render::one_line(problem), Ink::Dim))?;
                }
                Ok(())
            })?;
        }

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
            config: Some("/tmp/centinel.toml".into()),
            config_searched: vec![],
            sources: vec![],
            config_error: None,
            binaries: vec![bin],
            services: vec![],
            vectors: None,
            gates: vec![],
            binaries_ready: true,
            services_ready: true,
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
        assert!(out.contains("/tmp/centinel.toml"), "{out}");
    }

    /// With no config file, the search list stands in for it. A bare "no config file"
    /// leaves the one actionable fact — *which paths* — off the screen, which is how
    /// somebody ends up with a config the binary was never going to read.
    #[test]
    fn a_missing_config_names_where_it_was_looked_for() {
        let report = DoctorReport {
            store_root: "/tmp/store".into(),
            config: None,
            config_searched: vec![
                "centinel.toml".into(),
                "/home/x/.centinel/centinel.toml".into(),
            ],
            sources: vec![],
            config_error: None,
            binaries: vec![],
            services: vec![],
            vectors: None,
            gates: vec![],
            binaries_ready: true,
            services_ready: true,
            ready: true,
        };
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("no config file"), "{out}");
        assert!(out.contains("/home/x/.centinel/centinel.toml"), "{out}");
    }
    /// An `[inference]` with embed and rerank at a port nothing listens on and
    /// transcription at `transcribe`.
    fn split(transcribe: &str) -> InferenceConfig {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        crate::config::Config::parse(&format!(
            "[inference]\nbase_url = \"http://127.0.0.1:{port}/v1\"\n\
             [inference.transcribe]\nmodel = \"whisper-1\"\nbase_url = \"{transcribe}\"\n"
        ))
        .unwrap()
        .inference
    }

    /// The gates shut independently: no search server shuts search and leaves a
    /// transcription server that answers open — and the shut one says where it looked.
    #[tokio::test]
    async fn the_gates_open_independently() {
        let server = crate::inference::fake::serve().await;
        let config = split(&server.base_url);
        let services = survey(&config).await;
        let gates = gate_statuses(&services, None);

        let search = gates.iter().find(|g| g.gate == Gate::Search).unwrap();
        assert!(!search.ready);
        assert_eq!(search.missing, vec!["embed", "rerank"]);
        let transcription = gates
            .iter()
            .find(|g| g.gate == Gate::Transcription)
            .unwrap();
        assert!(transcription.ready, "{transcription:?}");

        let embed = services.iter().find(|s| s.capability == "embed").unwrap();
        let problem = embed.problem.as_deref().unwrap();
        assert!(problem.contains(&config.base_url), "{problem}");
    }

    /// A key the config names and the environment lacks is the problem reported — not a
    /// connection that would have been refused for want of it.
    #[tokio::test]
    async fn an_unset_key_is_the_problem_named() {
        let server = crate::inference::fake::serve().await;
        let mut config = server.config();
        config.api_key_env = Some("CENTINEL_TEST_KEY_THAT_IS_NEVER_SET".into());
        let services = survey(&config).await;
        assert!(services.iter().all(|s| !s.ready));
        assert!(
            services[0]
                .problem
                .as_deref()
                .unwrap()
                .contains("$CENTINEL_TEST_KEY_THAT_IS_NEVER_SET")
        );
    }

    /// A config that does not load shuts every gate: nothing is known to answer.
    #[test]
    fn no_services_is_no_gate_open() {
        assert!(gate_statuses(&[], None).iter().all(|g| !g.ready));
    }

    /// The upgrade case: a table the in-process embedder wrote, a server that answers, and
    /// search on keywords alone. The server row is green; this row is what says why, and
    /// what to run.
    #[tokio::test]
    async fn a_table_in_another_space_shuts_search_and_names_adopt() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path()).await.unwrap();
        let in_process = Space {
            model: "qwen3-embedding-4b".into(),
            endpoint: None,
        };
        VectorTable::open(&store.vectors_db(), &in_process, 4)
            .await
            .unwrap();
        let server = crate::inference::fake::serve().await;
        let configured = Space::of(&server.endpoint(Capability::Embed));

        let status = vectors_status(&store, Some(configured.clone()))
            .await
            .unwrap();
        assert!(!status.ready);
        assert_eq!(status.written_by, Some(in_process));
        let problem = status.problem.as_deref().unwrap();
        assert!(
            problem.contains("centinel embed --adopt --dry-run"),
            "{problem}"
        );

        let services = survey(&server.config()).await;
        let search = gate_statuses(&services, Some(&status))
            .into_iter()
            .find(|g| g.gate == Gate::Search)
            .unwrap();
        assert!(!search.ready);
        assert_eq!(search.missing, vec!["vectors"]);

        // The configured server's own table is ready, and no table at all says nothing.
        let ours = tempfile::tempdir().unwrap();
        let ours = crate::store::Store::open(ours.path()).await.unwrap();
        assert!(
            vectors_status(&ours, Some(configured.clone()))
                .await
                .is_none()
        );
        VectorTable::open(&ours.vectors_db(), &configured, 4)
            .await
            .unwrap();
        assert!(vectors_status(&ours, Some(configured)).await.unwrap().ready);
    }

    #[tokio::test]
    async fn the_report_separates_binary_readiness_from_server_readiness() {
        let store = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(crate::store::Store::open(store.path()).await.unwrap());
        let report = doctor(&ctx, DoctorArgs {}).await.unwrap();

        // A machine can crawl and extract with no server; it just cannot search
        // semantically. The two flags exist so that distinction survives into the report.
        assert_eq!(
            report.ready,
            report.binaries_ready && report.services_ready,
            "`ready` must be the conjunction, not an independent judgement"
        );
        // One row per capability when the config loads, none when it does not — and
        // then the reason is on the report instead.
        assert_eq!(
            report.services.len(),
            if report.config_error.is_none() { 3 } else { 0 }
        );
    }
}
