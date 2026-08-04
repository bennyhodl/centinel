//! `open` — read a collected document in a real application.
//!
//! Search tells you a passage exists and cites the bytes it came from. This is how you
//! actually look at it: the blob is linked into `current/` under a usable name with a
//! real extension, then handed to whichever application the config nominates for that
//! kind of file.
//!
//! Both halves of a document are reachable — the **original** bytes as served, and the
//! **derived** markdown. Reading the extraction next to its source is the fastest way to
//! judge whether extraction is doing a good job.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::{Config, SYSTEM_DEFAULT};
use crate::fetch::content_kind;
use crate::materialize::materialize;
use crate::prelude::*;
use crate::store::LogRecord;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct OpenArgs {
    /// A URL, a substring of one, or a blob hash.
    #[arg(value_name = "TARGET")]
    pub target: String,

    /// Open the extracted text rather than the original bytes.
    #[arg(long)]
    #[serde(default)]
    pub derived: bool,

    /// Application or `command {path}` template, overriding the config.
    #[arg(long = "with")]
    #[serde(default)]
    pub with: Option<String>,

    /// Materialise and print the path without launching anything.
    ///
    /// The scripting form, and the safe one for a server: `open` launches a GUI
    /// application on the machine running it, which is rarely what a remote caller wants.
    #[arg(long)]
    #[serde(default)]
    pub print_path: bool,

    /// Restrict the search for `target` to one source.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct OpenReport {
    pub url: String,
    pub source: String,
    pub kind: String,
    /// The materialised path under `current/`.
    pub path: String,
    pub blob_sha: String,
    pub bytes: usize,
    /// Whether this is the original bytes or the extracted text.
    pub derived: bool,
    /// The command run, or `null` with `--print-path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened_with: Option<String>,
    /// Other matches, when `target` was ambiguous. The first was used.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_matches: Vec<String>,
}

/// Open a collected document in an application.
///
/// **CLI only.** This launches a GUI application on the host and accepts a command
/// template, so exposing it over MCP or HTTP would be arbitrary command execution
/// against a server that has no authentication. Agents wanting the *content* of a
/// document should call `read`, which returns text and touches nothing.
#[op(local_only)]
pub async fn open(ctx: &Ctx, args: OpenArgs) -> anyhow::Result<OpenReport> {
    let sources = match &args.source {
        Some(s) => vec![SourceId::new(s.clone())?],
        None => ctx.store.sources().await?,
    };

    // ---- resolve the target ---------------------------------------------------------
    let looks_like_hash =
        args.target.len() == 64 && args.target.chars().all(|c| c.is_ascii_hexdigit());

    let mut matches: Vec<(SourceId, Resource, Observation)> = Vec::new();
    for source in &sources {
        for (resource, obs) in ctx.store.latest_observations(source).await? {
            let hit = if looks_like_hash {
                obs.blob_sha.as_str() == args.target
            } else {
                resource.natural_key == args.target || resource.natural_key.contains(&args.target)
            };
            if hit {
                matches.push((source.clone(), resource, obs));
            }
        }
    }

    anyhow::ensure!(
        !matches.is_empty(),
        "nothing in the store matches `{}`. Try `centinel search` first, or pass a full URL.",
        args.target
    );

    // Prefer an exact URL match over a substring one, so a precise target is never
    // shadowed by a longer URL that happens to contain it.
    matches.sort_by_key(|(_, r, _)| (r.natural_key != args.target, r.natural_key.clone()));
    let (source, resource, obs) = matches.first().cloned().expect("non-empty");
    let other_matches = matches
        .iter()
        .skip(1)
        .take(10)
        .map(|(_, r, _)| r.natural_key.clone())
        .collect();

    // ---- pick the blob --------------------------------------------------------------
    let (blob_sha, kind) = if args.derived {
        let derivation = ctx
            .store
            .read_log(&source)
            .await?
            .into_iter()
            .filter_map(|r| match r {
                LogRecord::Derivation(d) if d.from_sha == obs.blob_sha => Some(d),
                _ => None,
            })
            .next_back()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no extracted text for {} — run `centinel extract` first",
                    resource.natural_key
                )
            })?;
        (derivation.to_sha, "markdown".to_string())
    } else {
        let bytes = ctx.store.get_blob(&obs.blob_sha).await?;
        (
            obs.blob_sha.clone(),
            content_kind(&obs.meta, &bytes).to_string(),
        )
    };

    let path = materialize(&ctx.store, &source, &resource.natural_key, &blob_sha, &kind).await?;
    let bytes = tokio::fs::metadata(&path).await?.len() as usize;

    // ---- launch ---------------------------------------------------------------------
    let opened_with = if args.print_path {
        None
    } else {
        let config = Config::load()?;
        let opener = args
            .with
            .clone()
            .unwrap_or_else(|| config.open.opener_for(&kind).to_string());
        Some(launch(&opener, &path)?)
    };

    Ok(OpenReport {
        url: resource.natural_key,
        source: source.to_string(),
        kind,
        path: path.display().to_string(),
        blob_sha: blob_sha.to_string(),
        bytes,
        derived: args.derived,
        opened_with,
        other_matches,
    })
}

/// Runs the configured opener, returning the command for the report.
///
/// Three forms, distinguished without configuration ceremony:
/// - `"system"` — hand it to the OS default handler
/// - anything containing `{path}` — a command template
/// - anything else — an application name
fn launch(opener: &str, path: &std::path::Path) -> anyhow::Result<String> {
    let p = path.to_string_lossy().to_string();

    let (program, argv) = if opener == SYSTEM_DEFAULT || opener.is_empty() {
        (system_opener().to_string(), vec![p.clone()])
    } else if opener.contains("{path}") {
        let mut parts = opener
            .split_whitespace()
            .map(|t| t.replace("{path}", &p))
            .collect::<Vec<_>>();
        anyhow::ensure!(!parts.is_empty(), "empty command template");
        let program = parts.remove(0);
        (program, parts)
    } else if cfg!(target_os = "macos") {
        // `open -a` is how macOS names an application rather than a binary.
        (
            "open".to_string(),
            vec!["-a".into(), opener.into(), p.clone()],
        )
    } else {
        (opener.to_string(), vec![p.clone()])
    };

    let status = std::process::Command::new(&program)
        .args(&argv)
        .status()
        .map_err(|e| anyhow::anyhow!("could not run `{program}`: {e}"))?;
    anyhow::ensure!(
        status.success(),
        "`{program}` exited with {status}; is the application installed?"
    );

    Ok(std::iter::once(program)
        .chain(argv)
        .collect::<Vec<_>>()
        .join(" "))
}

fn system_opener() -> &'static str {
    if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    }
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// One line of confirmation, because the result of `open` is a window appearing.
///
/// The path is the exception: with `--print-path` it *is* the output, so it is printed
/// plainly enough to be copied or piped into another command.
impl Render for OpenReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        match &self.opened_with {
            Some(cmd) => {
                p.marked(Mark::Ok, p.paint(cmd, Ink::Cyan))?;
                p.nest(|p| p.line(p.paint(&self.path, Ink::Dim)))?;
            }
            None => p.line(&self.path)?,
        }

        let what = format!(
            "{} · {} · {}{}",
            self.source,
            self.kind,
            render::bytes(self.bytes as u64),
            if self.derived { " · extracted text" } else { "" },
        );
        p.nest(|p| p.line(p.paint(&what, Ink::Dim)))?;

        if !self.other_matches.is_empty() {
            p.nest(|p| {
                let note = format!(
                    "{} other matched; this is the first",
                    render::plural(self.other_matches.len(), "address", "addresses")
                );
                p.line(p.paint(&note, Ink::Yellow))
            })?;
        }
        Ok(())
    }
}
