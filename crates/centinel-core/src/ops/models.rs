//! `models` — inspect, fetch, verify and remove model weights.
//!
//! SPEC §3.2 makes this an **explicit operator action**: weights are never fetched as a
//! side effect of something else, so a scheduled 3am crawl can fail on a missing model
//! but can never decide to download 1.7 GB on its own.
//!
//! ## Why this is `local_only`
//!
//! It is the same argument that made `open` host-local, one step weaker. `models pull`
//! writes to a machine-local cache outside the store and can be made to pull gigabytes;
//! over an HTTP server that (SPEC §8) has no authentication, that is an unauthenticated
//! disk-and-bandwidth exhaustion primitive. The *useful* remote question — "are the
//! weights present?" — belongs in `doctor` beside the binary probes, which is where
//! SPEC §3.2 puts weights conceptually. Nothing here is needed by a model.
//!
//! ## The subcommand shape
//!
//! This is the one op with nested subcommands (`models pull`, `models list`). SPEC §3.2
//! names `centinel models pull` directly, and the alternative — flattening to `pull` and
//! `list` at top level — would spend two generic verbs on one narrow subsystem.

use std::path::PathBuf;
use std::time::Instant;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::models::download::{Downloader, FileJob, Outcome, Overall, part_path, sha256_file};
use crate::models::{self, ModelSpec, ModelStatus, Variant};
use crate::policy::HostPolicy;
use crate::prelude::*;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct ModelsArgs {
    #[command(subcommand)]
    pub action: ModelsAction,
}

#[derive(Clone, Debug, clap::Subcommand, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum ModelsAction {
    /// Show every model in the registry and what is on disk.
    List,
    /// Download weights. Resumes an interrupted pull.
    Pull(PullArgs),
    /// Re-hash installed files against their pinned digests.
    Verify(VerifyArgs),
    /// Delete downloaded weights.
    Remove(RemoveArgs),
    /// Delete cached weights the registry no longer references.
    Prune(PruneArgs),
    /// Print the weights cache directory.
    Dir,
}

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct PullArgs {
    /// Model id. Omit to pull the default variant of every model.
    #[arg(value_name = "MODEL")]
    #[serde(default)]
    pub model: Option<String>,

    /// Quantization to fetch. Only meaningful with an explicit model.
    #[arg(long)]
    #[serde(default)]
    pub variant: Option<String>,

    /// Re-download files already present, discarding any partial transfer.
    #[arg(long)]
    #[serde(default)]
    pub force: bool,
}

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct VerifyArgs {
    /// Model id. Omit to verify everything installed.
    #[arg(value_name = "MODEL")]
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct RemoveArgs {
    /// Model id whose weights to delete.
    #[arg(value_name = "MODEL")]
    pub model: String,

    /// Delete only this quantization, leaving any others in place.
    #[arg(long)]
    #[serde(default)]
    pub variant: Option<String>,
}

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct PruneArgs {
    /// Actually delete. Without this, `prune` only reports what it would remove.
    ///
    /// A preview by default because this walks the cache and deletes directories: the
    /// files are re-downloadable, but a bug in the walk would be expensive and silent.
    #[arg(long)]
    #[serde(default)]
    pub delete: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct FetchedFile {
    pub model: String,
    pub path: String,
    pub bytes: u64,
    /// Byte offset an interrupted transfer resumed from. 0 for a fresh download.
    pub resumed_from: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct FileCheck {
    pub model: String,
    pub path: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum ModelsReport {
    List {
        dir: PathBuf,
        models: Vec<ModelStatus>,
    },
    Pull {
        dir: PathBuf,
        fetched: Vec<FetchedFile>,
        /// Files that were already present.
        skipped: Vec<String>,
        bytes_fetched: u64,
        elapsed_secs: f64,
        models: Vec<ModelStatus>,
    },
    Verify {
        dir: PathBuf,
        checked: Vec<FileCheck>,
        ok: bool,
    },
    Remove {
        removed: Vec<PathBuf>,
        bytes_freed: u64,
    },
    Prune {
        /// Cache directories no longer named by the registry.
        orphaned: Vec<Orphan>,
        bytes: u64,
        /// False when this was a preview.
        deleted: bool,
    },
    Dir {
        dir: PathBuf,
    },
}

/// Inspect, fetch, verify and remove model weights.
#[op(long_running, local_only, group = "host")]
pub async fn models(
    _ctx: &Ctx,
    args: ModelsArgs,
    progress: &Progress,
) -> anyhow::Result<ModelsReport> {
    let dir = models::models_dir()?;

    match args.action {
        ModelsAction::Dir => Ok(ModelsReport::Dir { dir }),
        ModelsAction::List => Ok(ModelsReport::List {
            models: models::REGISTRY
                .iter()
                .map(|s| models::status(s, &dir))
                .collect(),
            dir,
        }),
        ModelsAction::Pull(pull) => run_pull(pull, dir, progress).await,
        ModelsAction::Verify(verify) => run_verify(verify, dir, progress).await,
        ModelsAction::Remove(remove) => run_remove(remove, dir).await,
        ModelsAction::Prune(prune) => run_prune(prune, dir).await,
    }
}

/// Resolves which (model, variant) pairs an argument set names.
///
/// With no model, every model's default variant — the "just make search work" path.
/// `--variant` without a model is refused rather than guessed: an embedder and a
/// reranker publish different quantization ladders, and silently applying one name to
/// whichever models happen to have it would produce a mismatched pair.
fn selection(
    model: Option<&str>,
    variant: Option<&str>,
) -> anyhow::Result<Vec<(&'static ModelSpec, &'static Variant)>> {
    match model {
        Some(id) => {
            let spec = models::require(id)?;
            Ok(vec![(spec, spec.variant(variant)?)])
        }
        None => {
            anyhow::ensure!(
                variant.is_none(),
                "--variant needs a model; it does not mean the same files on every model"
            );
            models::REGISTRY
                .iter()
                .map(|spec| spec.variant(None).map(|v| (spec, v)))
                .collect()
        }
    }
}

async fn run_pull(
    args: PullArgs,
    dir: PathBuf,
    progress: &Progress,
) -> anyhow::Result<ModelsReport> {
    let targets = selection(args.model.as_deref(), args.variant.as_deref())?;

    // Planned up front so the aggregate bar has a real denominator from the first byte,
    // rather than growing as files are discovered.
    let mut jobs: Vec<(&'static ModelSpec, FileJob)> = Vec::new();
    for (spec, variant) in &targets {
        let model_dir = spec.dir(&dir);
        for file in spec.files_for(variant) {
            jobs.push((
                spec,
                FileJob {
                    url: spec.url_for(file),
                    dest: model_dir.join(file.path),
                    file,
                    bar_id: format!("{}:{}", spec.id, file.path),
                    label: format!("{} {}", spec.id, file.path),
                },
            ));
        }
    }

    let mut overall = Overall::new(jobs.iter().map(|(_, j)| j.file.size).sum());
    progress.say(format!(
        "pulling {} file(s), {} total",
        jobs.len(),
        human_bytes(overall.total)
    ));

    // The same descriptive, contactable User-Agent every other fetch uses. Hugging Face
    // does not require it, but there is no reason for our downloads to be anonymous when
    // our crawls are not.
    let downloader = Downloader::new(&HostPolicy::default().user_agent, args.force)?;

    let started = Instant::now();
    let mut fetched = Vec::new();
    let mut skipped = Vec::new();
    let mut bytes_fetched = 0u64;

    // Sequential on purpose. Parallel transfers would finish sooner on a fast link and
    // make the failure story worse on a slow one, which is the link that matters here:
    // one file in flight means one `.part` to reason about after a Ctrl-C.
    for (spec, job) in &jobs {
        match downloader.fetch(job, progress, &mut overall).await? {
            Outcome::Present => skipped.push(job.file.path.to_string()),
            Outcome::Downloaded {
                bytes,
                resumed_from,
            } => {
                bytes_fetched += bytes;
                fetched.push(FetchedFile {
                    model: spec.id.to_string(),
                    path: job.file.path.to_string(),
                    bytes,
                    resumed_from,
                });
            }
        }
    }

    Ok(ModelsReport::Pull {
        fetched,
        skipped,
        bytes_fetched,
        elapsed_secs: started.elapsed().as_secs_f64(),
        models: targets
            .iter()
            .map(|(spec, _)| models::status(spec, &dir))
            .collect(),
        dir,
    })
}

/// Re-hashes what is on disk.
///
/// Separate from `list` because it is the expensive half: `list` answers "is it here?"
/// from file sizes in microseconds, and this answers "is it *right*?" by reading every
/// byte. Conflating them would put a 1.7 GB read behind a status command.
async fn run_verify(
    args: VerifyArgs,
    dir: PathBuf,
    progress: &Progress,
) -> anyhow::Result<ModelsReport> {
    let specs: Vec<&'static ModelSpec> = match args.model.as_deref() {
        Some(id) => vec![models::require(id)?],
        None => models::REGISTRY.iter().collect(),
    };

    let mut checked = Vec::new();
    for spec in specs {
        let model_dir = spec.dir(&dir);
        for variant in spec.variants {
            for file in spec.files_for(variant) {
                let path = model_dir.join(file.path);
                // Only what is installed. A model the operator never pulled is not a
                // failure, and reporting it as one would make `ok` meaningless.
                let Ok(meta) = tokio::fs::metadata(&path).await else {
                    continue;
                };
                // Same file, two variants (the shared tokenizer) — check it once.
                if checked
                    .iter()
                    .any(|c: &FileCheck| c.model == spec.id && c.path == file.path)
                {
                    continue;
                }

                progress.say(format!("hashing {} {}", spec.id, file.path));
                let problem = if meta.len() != file.size {
                    Some(format!("size {}, expected {}", meta.len(), file.size))
                } else {
                    match sha256_file(&path).await {
                        Ok(actual) if actual == file.sha256 => None,
                        Ok(actual) => Some(format!("digest {actual}, expected {}", file.sha256)),
                        Err(e) => Some(e.to_string()),
                    }
                };
                checked.push(FileCheck {
                    model: spec.id.to_string(),
                    path: file.path.to_string(),
                    ok: problem.is_none(),
                    problem,
                });
            }
        }
    }

    Ok(ModelsReport::Verify {
        ok: checked.iter().all(|c| c.ok),
        checked,
        dir,
    })
}

async fn run_remove(args: RemoveArgs, dir: PathBuf) -> anyhow::Result<ModelsReport> {
    let spec = models::require(&args.model)?;
    let model_dir = spec.dir(&dir);

    let mut removed = Vec::new();
    let mut bytes_freed = 0u64;

    // Deletes named files, never the directory — because a directory is keyed by
    // `<repo>/<revision>` and **two models can share one**: `whisper-tiny` and
    // `whisper-large-v3-turbo` are both files in `ggerganov/whisper.cpp`. A
    // `remove_dir_all` here would take the sibling model with it.
    let targets: Vec<&'static models::Variant> = match args.variant.as_deref() {
        Some(name) => vec![spec.variant(Some(name))?],
        None => spec.variants.iter().collect(),
    };

    for variant in targets {
        for file in variant.files {
            let target = model_dir.join(file.path);
            for path in [part_path(&target), target] {
                if let Ok(meta) = tokio::fs::metadata(&path).await {
                    bytes_freed += meta.len();
                    tokio::fs::remove_file(&path).await?;
                    removed.push(path);
                }
            }
        }
    }

    // Tidy up only if nothing else is left — which is exactly the shared-directory test.
    if let Ok(mut entries) = tokio::fs::read_dir(&model_dir).await
        && entries.next_entry().await?.is_none()
    {
        tokio::fs::remove_dir(&model_dir).await?;
        removed.push(model_dir);
    }

    Ok(ModelsReport::Remove {
        removed,
        bytes_freed,
    })
}

/// Weights on disk that the registry no longer names.
///
/// Two ways to become one: the registry stops shipping a model, or its pinned revision
/// is bumped and the old commit's directory is left behind. Both are expected — pinning
/// by revision is what makes an interrupted upgrade safe (see [`crate::models`]) — so
/// something has to collect them eventually.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Orphan {
    pub path: PathBuf,
    pub bytes: u64,
    /// Why it is unreferenced, as far as we can tell.
    pub reason: String,
}

async fn run_prune(args: PruneArgs, dir: PathBuf) -> anyhow::Result<ModelsReport> {
    // Every directory the current registry would use.
    let referenced: std::collections::HashSet<PathBuf> =
        models::REGISTRY.iter().map(|s| s.dir(&dir)).collect();
    let known_repos: std::collections::HashSet<&str> =
        models::REGISTRY.iter().map(|s| s.repo).collect();

    let mut orphaned = Vec::new();
    let mut bytes = 0u64;

    // The cache is `<root>/<owner>/<name>/<revision>/`, so a revision directory sits
    // exactly three levels down. Walking to a fixed depth rather than recursing means a
    // stray file somewhere in the tree can never be mistaken for a model to delete.
    for owner in read_dirs(&dir).await? {
        for name in read_dirs(&owner).await? {
            let repo = format!(
                "{}/{}",
                owner.file_name().unwrap_or_default().to_string_lossy(),
                name.file_name().unwrap_or_default().to_string_lossy()
            );
            for revision in read_dirs(&name).await? {
                if referenced.contains(&revision) {
                    continue;
                }
                let size = dir_size(&revision).await.unwrap_or(0);
                bytes += size;
                orphaned.push(Orphan {
                    reason: if known_repos.contains(repo.as_str()) {
                        format!("{repo} is pinned to a different revision")
                    } else {
                        format!("{repo} is no longer in the registry")
                    },
                    path: revision,
                    bytes: size,
                });
            }
        }
    }

    if args.delete {
        for orphan in &orphaned {
            tokio::fs::remove_dir_all(&orphan.path).await?;
            // Leave no empty `<owner>/<name>` shells behind. Fails harmlessly when a
            // sibling revision is still present, which is the intended guard.
            if let Some(parent) = orphan.path.parent() {
                let _ = tokio::fs::remove_dir(parent).await;
                if let Some(grandparent) = parent.parent() {
                    let _ = tokio::fs::remove_dir(grandparent).await;
                }
            }
        }
    }

    Ok(ModelsReport::Prune {
        orphaned,
        bytes,
        deleted: args.delete,
    })
}

/// Immediate subdirectories, treating a missing directory as empty.
async fn read_dirs(dir: &std::path::Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut out = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_dir() {
            out.push(entry.path());
        }
    }
    out.sort();
    Ok(out)
}

async fn dir_size(dir: &std::path::Path) -> anyhow::Result<u64> {
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let meta = entry.metadata().await?;
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    Ok(total)
}

/// Binary units, matching what the CLI's progress bars render.
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// Six subcommands, six shapes.
///
/// The `action` discriminant is dropped everywhere — it tells a JSON consumer which
/// variant it holds, and tells a person who typed `models list` that they typed
/// `models list`. So is `dir` on most variants: the weights cache is a fixed location,
/// and `models dir` exists for the one time anybody needs it.
impl Render for ModelsReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        match self {
            ModelsReport::Dir { dir } => p.line(dir.display().to_string()),

            ModelsReport::List { models, dir } => {
                let installed = models.iter().filter(|m| m.installed).count();
                let on_disk: u64 = models
                    .iter()
                    .filter_map(|m| m.active())
                    .map(|v| v.bytes_present)
                    .sum();
                p.title(
                    &format!("{installed} of {} installed", models.len()),
                    &render::bytes(on_disk),
                )?;
                p.line(p.paint(&dir.display().to_string(), Ink::Dim))?;

                for model in models {
                    p.blank()?;
                    model.render(p)?;
                }
                Ok(())
            }

            ModelsReport::Pull {
                fetched,
                skipped,
                bytes_fetched,
                elapsed_secs,
                ..
            } => {
                let rate = if *elapsed_secs > 0.0 {
                    format!(
                        " at {}/s",
                        render::bytes((*bytes_fetched as f64 / elapsed_secs) as u64)
                    )
                } else {
                    String::new()
                };
                p.title(
                    &format!("{} fetched", render::bytes(*bytes_fetched)),
                    &format!("{}{rate}", render::duration(*elapsed_secs)),
                )?;

                p.nest(|p| {
                    for file in fetched {
                        let name = std::path::Path::new(&file.path)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| file.path.clone());
                        let mut note = render::bytes(file.bytes);
                        // A resumed transfer is the feature that makes a 4 GB pull over a
                        // bad connection survivable, and the only proof it worked is here.
                        if file.resumed_from > 0 {
                            note.push_str(&format!(
                                ", resumed from {}",
                                render::bytes(file.resumed_from)
                            ));
                        }
                        p.marked(
                            Mark::Ok,
                            format!("{name}  {}", p.paint(&note, Ink::Dim)),
                        )?;
                    }
                    for name in skipped {
                        let text = format!("{name}  already present");
                        p.line(format!("{}  {}", p.paint("·", Ink::Dim), p.paint(&text, Ink::Dim)))?;
                    }
                    Ok(())
                })
            }

            ModelsReport::Verify { checked, ok, .. } => {
                let failed = checked.iter().filter(|c| !c.ok).count();
                let verdict = if *ok {
                    p.paint("every digest matches", Ink::Green)
                } else {
                    p.paint(
                        &format!("{} of {} failed", failed, checked.len()),
                        Ink::Red,
                    )
                };
                p.line(verdict)?;

                p.nest(|p| {
                    for check in checked {
                        // A passing digest is a line nobody reads; a failing one is the
                        // whole reason the command exists. Only the failures get detail.
                        let name = std::path::Path::new(&check.path)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| check.path.clone());
                        p.marked(
                            Mark::from_ok(check.ok),
                            format!("{}  {}", check.model, p.paint(&name, Ink::Dim)),
                        )?;
                        if let Some(problem) = &check.problem {
                            p.nest(|p| p.wrapped(&render::one_line(problem), Ink::Red))?;
                        }
                    }
                    Ok(())
                })
            }

            ModelsReport::Remove {
                removed,
                bytes_freed,
            } => {
                p.title(&format!("{} freed", render::bytes(*bytes_freed)), "")?;
                p.nest(|p| {
                    for path in removed {
                        let text = render::truncate_start(&path.display().to_string(), p.width());
                        p.line(p.paint(&text, Ink::Dim))?;
                    }
                    Ok(())
                })
            }

            ModelsReport::Prune {
                orphaned,
                bytes,
                deleted,
            } => {
                if orphaned.is_empty() {
                    return p.marked(Mark::Ok, p.paint("nothing orphaned", Ink::Dim));
                }
                let verb = if *deleted { "freed" } else { "would free" };
                p.title(
                    &format!("{verb} {}", render::bytes(*bytes)),
                    &render::plural(orphaned.len(), "directory", "directories"),
                )?;
                p.nest(|p| {
                    for orphan in orphaned {
                        let text = render::truncate_start(
                            &orphan.path.display().to_string(),
                            p.width(),
                        );
                        p.line(p.paint(&text, Ink::Dim))?;
                        p.nest(|p| {
                            let note = format!(
                                "{}  {}",
                                render::bytes(orphan.bytes),
                                render::one_line(&orphan.reason)
                            );
                            p.line(p.paint(&note, Ink::Dim))
                        })?;
                    }
                    if !*deleted {
                        p.blank()?;
                        p.line(p.paint("centinel models prune --delete", Ink::Cyan))?;
                    }
                    Ok(())
                })
            }
        }
    }
}

impl Render for ModelStatus {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let mark = Mark::from_ok(self.installed);
        let name = p.paint(&self.id, Ink::Bold);
        let role = p.paint(&format!("{} · {}", self.role, self.license), Ink::Dim);
        p.marked(mark, format!("{name}  {role}"))?;

        p.nest(|p| {
            p.nest(|p| {
                p.wrapped(&self.about, Ink::Dim)?;

                let mut table = Table::bare(&[
                    Align::Left,
                    Align::Left,
                    Align::Right,
                    Align::Left,
                ]);
                for variant in &self.variants {
                    // Three states, not two: a variant with bytes on disk but not all of
                    // them is a resumable pull, and calling it "missing" would tell
                    // someone to start a 3 GB download that is already half done.
                    let mark = if variant.installed {
                        Mark::Ok
                    } else if variant.bytes_present > 0 {
                        Mark::Warn
                    } else {
                        Mark::None
                    };
                    let size = if variant.installed || variant.bytes_present == 0 {
                        render::bytes(variant.bytes_total)
                    } else {
                        format!(
                            "{} / {}",
                            render::bytes(variant.bytes_present),
                            render::bytes(variant.bytes_total)
                        )
                    };
                    let ink = if variant.installed { Ink::Plain } else { Ink::Dim };
                    table.push(vec![
                        Cell::mark(mark),
                        Cell::new(
                            &variant.variant,
                            if variant.installed { Ink::Plain } else { Ink::Dim },
                        ),
                        Cell::new(size, ink),
                        Cell::dim(if variant.is_default { "default" } else { "" }),
                    ]);
                }
                p.table(&table)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses a command line exactly the way the CLI does: clap → JSON → args struct.
    ///
    /// That JSON hop is not incidental. `run_op` routes CLI arguments through the same
    /// JSON the HTTP and MCP surfaces send, so a nested subcommand has to survive being
    /// serialized as a tagged enum and read back. This is the one op where that could
    /// break, and it would break at runtime rather than at compile time.
    fn parse(argv: &[&str]) -> anyhow::Result<ModelsArgs> {
        let def = crate::op::find("models").expect("models must be registered");
        let cmd = (def.augment_clap)(clap::Command::new("models"));
        let matches = cmd.try_get_matches_from(argv)?;
        let json = (def.args_from_matches)(&matches)?;
        Ok(serde_json::from_value(json)?)
    }

    #[test]
    fn every_subcommand_survives_the_json_round_trip() {
        assert!(matches!(
            parse(&["models", "list"]).unwrap().action,
            ModelsAction::List
        ));
        assert!(matches!(
            parse(&["models", "dir"]).unwrap().action,
            ModelsAction::Dir
        ));
        assert!(matches!(
            parse(&["models", "verify"]).unwrap().action,
            ModelsAction::Verify(VerifyArgs { model: None })
        ));

        match parse(&["models", "pull"]).unwrap().action {
            ModelsAction::Pull(a) => {
                assert_eq!(a.model, None);
                assert_eq!(a.variant, None);
                assert!(!a.force);
            }
            other => panic!("wrong action: {other:?}"),
        }

        match parse(&[
            "models",
            "pull",
            "qwen3-embedding-4b",
            "--variant",
            "q4_k_m",
            "--force",
        ])
        .unwrap()
        .action
        {
            ModelsAction::Pull(a) => {
                assert_eq!(a.model.as_deref(), Some("qwen3-embedding-4b"));
                assert_eq!(a.variant.as_deref(), Some("q4_k_m"));
                assert!(a.force);
            }
            other => panic!("wrong action: {other:?}"),
        }

        match parse(&[
            "models",
            "remove",
            "qwen3-reranker-0.6b",
            "--variant",
            "q8_0",
        ])
        .unwrap()
        .action
        {
            ModelsAction::Remove(a) => {
                assert_eq!(a.model, "qwen3-reranker-0.6b");
                assert_eq!(a.variant.as_deref(), Some("q8_0"));
            }
            other => panic!("wrong action: {other:?}"),
        }
    }

    #[test]
    fn a_bare_models_invocation_asks_for_a_subcommand() {
        assert!(
            parse(&["models"]).is_err(),
            "`models` alone must print help, not silently do something"
        );
    }

    /// `remove` takes a required model, unlike `pull` and `verify`. Deleting weights is
    /// not a thing to do to everything at once by omission.
    #[test]
    fn remove_requires_a_model() {
        assert!(parse(&["models", "remove"]).is_err());
    }

    #[test]
    fn no_model_selects_every_default_variant() {
        let picked = selection(None, None).unwrap();
        assert_eq!(picked.len(), models::REGISTRY.len());
        for (spec, variant) in picked {
            assert_eq!(variant.name, spec.default_variant);
        }
    }

    #[test]
    fn a_variant_without_a_model_is_refused_rather_than_guessed() {
        let err = selection(None, Some("q8_0")).unwrap_err().to_string();
        assert!(err.contains("--variant needs a model"), "{err}");
    }

    #[test]
    fn an_explicit_variant_is_honoured() {
        let picked = selection(Some("qwen3-embedding-4b"), Some("q4_k_m")).unwrap();
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].1.name, "q4_k_m");
    }

    #[test]
    fn an_unknown_model_names_the_registry() {
        let err = selection(Some("bert"), None).unwrap_err().to_string();
        assert!(err.contains("qwen3-embedding-4b"), "{err}");
    }

    #[test]
    fn human_bytes_uses_binary_units() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(1023), "1023 B");
        assert_eq!(human_bytes(1024), "1.00 KiB");
        assert_eq!(human_bytes(613_527_539), "585.11 MiB");
        assert_eq!(human_bytes(1_219_344_796), "1.14 GiB");
    }

    /// The cache root is passed in rather than resolved from `$CENTINEL_MODELS`.
    /// The environment is process-global and these tests run in parallel, so exactly one
    /// test in the crate is allowed to mutate it.
    #[tokio::test]
    async fn removing_weights_that_were_never_pulled_is_a_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let report = run_remove(
            RemoveArgs {
                model: "qwen3-embedding-4b".into(),
                variant: None,
            },
            tmp.path().to_path_buf(),
        )
        .await
        .unwrap();

        match report {
            ModelsReport::Remove {
                removed,
                bytes_freed,
            } => {
                assert!(removed.is_empty());
                assert_eq!(bytes_freed, 0);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    /// Writes a stand-in for every file of a variant. Contents are irrelevant — these
    /// tests are about which paths get deleted, not what is in them.
    fn place(spec: &'static ModelSpec, variant: &str, root: &std::path::Path, bytes: &[u8]) {
        for file in spec.files_for(spec.variant(Some(variant)).unwrap()) {
            let path = spec.dir(root).join(file.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
        }
    }

    /// Deleting one quantization must leave the others alone. Under ONNX this test was
    /// about a shared tokenizer; GGUF has no sidecars, so the thing worth protecting is
    /// simply the other variants sitting in the same directory.
    #[tokio::test]
    async fn removing_a_variant_leaves_other_variants_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let spec = models::require("qwen3-embedding-4b").unwrap();

        place(spec, "q8_0", &root, b"x");
        place(spec, "q4_k_m", &root, b"x");

        run_remove(
            RemoveArgs {
                model: spec.id.into(),
                variant: Some("q8_0".into()),
            },
            root.clone(),
        )
        .await
        .unwrap();

        let gone = spec
            .dir(&root)
            .join(spec.variant(Some("q8_0")).unwrap().files[0].path);
        let kept = spec
            .dir(&root)
            .join(spec.variant(Some("q4_k_m")).unwrap().files[0].path);
        assert!(!gone.exists(), "the named variant should be deleted");
        assert!(kept.exists(), "an unnamed variant must survive");
    }

    #[tokio::test]
    async fn removing_a_whole_model_takes_the_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let spec = models::require("qwen3-embedding-4b").unwrap();
        let model_dir = spec.dir(&root);

        place(spec, spec.default_variant, &root, b"1234");
        // Derived rather than hardcoded, so changing the registry cannot silently
        // invalidate the assertion.
        let expected: u64 = spec
            .files_for(spec.variant(None).unwrap())
            .map(|_| 4u64)
            .sum();

        let report = run_remove(
            RemoveArgs {
                model: spec.id.into(),
                variant: None,
            },
            root,
        )
        .await
        .unwrap();

        assert!(!model_dir.exists());
        match report {
            ModelsReport::Remove { bytes_freed, .. } => assert_eq!(bytes_freed, expected),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    /// Fabricates a cache directory for a repo/revision pair that no registry entry names.
    fn orphan(root: &std::path::Path, repo: &str, revision: &str, bytes: usize) {
        let dir = root.join(repo).join(revision);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("weights.gguf"), vec![0u8; bytes]).unwrap();
    }

    #[tokio::test]
    async fn prune_previews_by_default_and_deletes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        orphan(
            &root,
            "onnx-community/Qwen3-Embedding-0.6B-ONNX",
            "abc123",
            2048,
        );

        let report = run_prune(PruneArgs { delete: false }, root.clone())
            .await
            .unwrap();

        match report {
            ModelsReport::Prune {
                orphaned,
                bytes,
                deleted,
            } => {
                assert!(!deleted);
                assert_eq!(orphaned.len(), 1);
                assert_eq!(bytes, 2048);
                assert!(orphaned[0].reason.contains("no longer in the registry"));
            }
            other => panic!("wrong variant: {other:?}"),
        }
        assert!(
            root.join("onnx-community/Qwen3-Embedding-0.6B-ONNX/abc123")
                .exists(),
            "a preview must not delete"
        );
    }

    #[tokio::test]
    async fn prune_with_delete_removes_the_directory_and_its_empty_parents() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        orphan(
            &root,
            "onnx-community/Qwen3-Reranker-0.6B-ONNX",
            "def456",
            1024,
        );

        run_prune(PruneArgs { delete: true }, root.clone())
            .await
            .unwrap();

        assert!(
            !root.join("onnx-community").exists(),
            "empty shells should go too"
        );
    }

    /// A registry model at its pinned revision must survive, or prune would delete the
    /// weights it was run to tidy up around.
    #[tokio::test]
    async fn prune_never_touches_a_referenced_revision() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let spec = models::require("qwen3-embedding-4b").unwrap();
        place(spec, spec.default_variant, &root, b"live");

        let report = run_prune(PruneArgs { delete: true }, root.clone())
            .await
            .unwrap();

        match report {
            ModelsReport::Prune { orphaned, .. } => assert!(orphaned.is_empty(), "{orphaned:?}"),
            other => panic!("wrong variant: {other:?}"),
        }
        assert!(spec.dir(&root).exists(), "the pinned revision must survive");
    }

    /// Bumping a pin leaves the old commit behind on purpose — that is what makes an
    /// interrupted upgrade safe. Prune is how it eventually gets collected.
    #[tokio::test]
    async fn a_stale_revision_of_a_current_model_is_pruned_and_says_why() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let spec = models::require("qwen3-embedding-4b").unwrap();

        place(spec, spec.default_variant, &root, b"live");
        orphan(
            &root,
            spec.repo,
            "0000000000000000000000000000000000000000",
            512,
        );

        let report = run_prune(PruneArgs { delete: true }, root.clone())
            .await
            .unwrap();

        match report {
            ModelsReport::Prune { orphaned, .. } => {
                assert_eq!(orphaned.len(), 1);
                assert!(
                    orphaned[0].reason.contains("different revision"),
                    "{}",
                    orphaned[0].reason
                );
            }
            other => panic!("wrong variant: {other:?}"),
        }
        assert!(
            spec.dir(&root).exists(),
            "the current revision must survive"
        );
    }

    #[tokio::test]
    async fn pruning_an_empty_cache_is_a_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let report = run_prune(PruneArgs { delete: true }, tmp.path().to_path_buf())
            .await
            .unwrap();
        match report {
            ModelsReport::Prune {
                orphaned, bytes, ..
            } => {
                assert!(orphaned.is_empty());
                assert_eq!(bytes, 0);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    /// `verify` reports; it never repairs.
    ///
    /// This exercises the **size** branch rather than the digest branch: the smallest
    /// file in the GGUF registry is 610 MiB, and a test that writes one to reach the
    /// hashing path would be an I/O benchmark. Digest comparison itself is covered
    /// end-to-end by `tests/download_resume.rs`, against a real server.
    #[tokio::test]
    async fn verify_reports_a_corrupt_file_without_touching_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let spec = models::require("qwen3-embedding-4b").unwrap();
        let file = spec.variant(None).unwrap().files[0].path;

        place(spec, spec.default_variant, &root, b"truncated");
        let target = spec.dir(&root).join(file);

        let report = run_verify(VerifyArgs { model: None }, root, &Progress::none())
            .await
            .unwrap();

        match report {
            ModelsReport::Verify { ok, checked, .. } => {
                assert!(!ok);
                let check = checked
                    .iter()
                    .find(|c| c.path == file)
                    .expect("the present file must be checked");
                assert!(
                    check.problem.as_ref().unwrap().contains("size"),
                    "{:?}",
                    check.problem
                );
            }
            other => panic!("wrong variant: {other:?}"),
        }
        assert!(target.exists(), "verify reports, it does not repair");
    }

    #[tokio::test]
    async fn verify_ignores_models_that_were_never_pulled() {
        let tmp = tempfile::tempdir().unwrap();
        let report = run_verify(
            VerifyArgs { model: None },
            tmp.path().to_path_buf(),
            &Progress::none(),
        )
        .await
        .unwrap();

        match report {
            ModelsReport::Verify { ok, checked, .. } => {
                assert!(checked.is_empty());
                assert!(ok, "an empty cache is not a failed verification");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
