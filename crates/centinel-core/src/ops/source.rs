//! `source` — what the corpus is made of.
//!
//! The config file is the list of things Centinel collects, so this is the op that
//! changes what a bare `centinel run` does. It exists rather than leaving people to edit
//! TOML because the two mistakes that file invites — a duplicate id, and an id that is
//! not a legal directory name — are both silent until a run is well underway.
//!
//! ## Adding is not collecting
//!
//! `source add` writes one block and stops. `--run` chains straight into the pipeline for
//! the case where that was obviously the intent, but it is opt-in: a command whose name
//! is "add" should not, by default, start an hour of network traffic against a city.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::{self, Acquisition, Config, SourceConfig};
use crate::prelude::*;

use super::run::{RunArgs, RunReport, SourceKind};

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct SourceArgs {
    #[command(subcommand)]
    pub action: SourceAction,
}

#[derive(Clone, Debug, clap::Subcommand, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum SourceAction {
    /// Add a source to the config.
    Add(AddArgs),
    /// List configured sources, and any the store holds that the config does not.
    List(ListArgs),
    /// Add every source the store already holds but the config does not name.
    Adopt(AdoptArgs),
    /// Remove a source from the config. The collected data is left alone.
    Remove(RemoveArgs),
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct AddArgs {
    /// Source id, e.g. `tampa`. Becomes a directory name under `log/`.
    pub id: String,

    /// Any URL on the site. Only the origin is used.
    #[arg(long, conflicts_with = "channel")]
    #[serde(default)]
    pub site: Option<String>,

    /// A channel URL — `https://www.youtube.com/@CityofTampa`.
    #[arg(long)]
    #[serde(default)]
    pub channel: Option<String>,

    /// Requests per second for this source. Omit to inherit `[defaults]`.
    #[arg(long)]
    #[serde(default)]
    pub rps: Option<f64>,

    /// Only collect addresses whose URL contains this substring. Repeatable.
    #[arg(long = "match")]
    #[serde(default)]
    pub matches: Vec<String>,

    /// Extra arguments for yt-dlp, e.g. `--yt-dlp-arg=--cookies-from-browser=brave`.
    #[arg(long = "yt-dlp-arg", allow_hyphen_values = true)]
    #[serde(default)]
    pub yt_dlp_args: Vec<String>,

    /// Write the block but leave it out of a bare `centinel run`.
    #[arg(long)]
    #[serde(default)]
    pub disabled: bool,

    /// Add it, then run the full pipeline for it immediately.
    #[arg(long)]
    #[serde(default)]
    pub run: bool,

    /// Config file to write. Defaults to the one in effect, else `./centinel.toml`.
    #[arg(long, value_name = "FILE")]
    #[serde(default)]
    pub config: Option<String>,
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct ListArgs {
    /// Config file to read.
    #[arg(long, value_name = "FILE")]
    #[serde(default)]
    pub config: Option<String>,

    /// Only list what the config names, ignoring the store.
    #[arg(long)]
    #[serde(default)]
    pub configured_only: bool,
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct AdoptArgs {
    /// Config file to write.
    #[arg(long, value_name = "FILE")]
    #[serde(default)]
    pub config: Option<String>,
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct RemoveArgs {
    /// Source id to remove.
    pub id: String,

    /// Config file to edit.
    #[arg(long, value_name = "FILE")]
    #[serde(default)]
    pub config: Option<String>,
}

/// One source, from the config, the store, or both.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ConfiguredSource {
    pub id: String,
    pub kind: SourceKind,
    /// `None` only for an untracked source whose address the store cannot reconstruct.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub enabled: bool,
    /// Resources the store holds under this id. Zero means it is configured but
    /// uncollected — the state a fresh `source add` leaves behind.
    pub resources: usize,
    /// Whether `centinel.toml` names it.
    ///
    /// False means the store has been collecting it — someone ran `centinel discover
    /// --source …` directly — but a bare `centinel run` will not touch it, because
    /// the config is the statement of intent and this source is not in it.
    pub tracked: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum SourceReport {
    Add {
        source: String,
        kind: SourceKind,
        target: String,
        /// The file that was written.
        config: String,
        /// Present when `--run` was given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run: Option<Box<RunReport>>,
    },
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        config: Option<String>,
        /// Config sources in file order, then any the store holds that it does not name.
        sources: Vec<ConfiguredSource>,
    },
    Adopt {
        config: String,
        adopted: Vec<ConfiguredSource>,
        /// Held by the store, but with no recoverable address to write down.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped: Vec<String>,
    },
    Remove {
        source: String,
        config: String,
    },
}

/// Add, list and remove the sources `centinel run` walks.
#[op(long_running, group = "pipeline")]
pub async fn source(
    ctx: &Ctx,
    args: SourceArgs,
    progress: &Progress,
) -> anyhow::Result<SourceReport> {
    match args.action {
        SourceAction::Add(a) => add(ctx, a, progress).await,
        SourceAction::List(a) => list(ctx, a).await,
        SourceAction::Adopt(a) => adopt(ctx, a).await,
        SourceAction::Remove(a) => remove(a),
    }
}

async fn add(ctx: &Ctx, args: AddArgs, progress: &Progress) -> anyhow::Result<SourceReport> {
    let path = match &args.config {
        Some(p) => std::path::PathBuf::from(p),
        None => Config::write_path(),
    };

    // Neither target given: the store may already know. Someone who ran `centinel
    // discover --source tampa --site …` and now wants it in the config should not have
    // to retype an address the log has been holding all along.
    let (mut site, mut channel) = (args.site.clone(), args.channel.clone());
    if site.is_none() && channel.is_none() {
        let id = SourceId::new(args.id.clone())?;
        match infer_from_store(ctx, &id).await? {
            Some(Inferred {
                kind,
                target: Some(target),
            }) => {
                progress.say(format!("inferred {target} from the store"));
                match kind {
                    SourceKind::Site => site = Some(target),
                    SourceKind::Channel => channel = Some(target),
                }
            }
            Some(Inferred { target: None, .. }) => anyhow::bail!(
                "the store holds `{}` but not enough to say where it came from; \
                 give --site or --channel",
                args.id
            ),
            None => {}
        }
    }

    let source = SourceConfig {
        id: args.id.clone(),
        site,
        channel,
        enabled: args.disabled.then_some(false),
        rps: args.rps,
        matches: args.matches.clone(),
        yt_dlp_args: args.yt_dlp_args.clone(),
        // Left unset so the pipeline's own default applies. Writing it here would bake
        // today's answer into the file and make a later change to that default invisible.
        audio_if_no_captions: None,
        lang: None,
    };

    let (kind, target) = match source.acquisition()? {
        Acquisition::Site(url) => (SourceKind::Site, url.to_string()),
        Acquisition::Channel(url) => (SourceKind::Channel, url.to_string()),
    };

    config::append_source(&path, &source)?;
    progress.say(format!("added {} to {}", args.id, path.display()));

    let run = if args.run {
        let report = super::run::run(
            ctx,
            RunArgs {
                sources: vec![args.id.clone()],
                config: Some(path.display().to_string()),
                ..Default::default()
            },
            progress,
        )
        .await?;
        Some(Box::new(report))
    } else {
        None
    };

    Ok(SourceReport::Add {
        source: args.id,
        kind,
        target,
        config: path.display().to_string(),
        run,
    })
}

async fn list(ctx: &Ctx, args: ListArgs) -> anyhow::Result<SourceReport> {
    let (config, path) = match &args.config {
        Some(p) => {
            let path = std::path::PathBuf::from(p);
            (Config::from_file(&path)?, Some(path))
        }
        None => (Config::load()?, Config::locate()),
    };

    let mut sources = Vec::with_capacity(config.sources.len());
    for source in &config.sources {
        let (kind, target) = match source.acquisition()? {
            Acquisition::Site(url) => (SourceKind::Site, url.to_string()),
            Acquisition::Channel(url) => (SourceKind::Channel, url.to_string()),
        };
        // A configured source need not exist in the store yet, so this counts rather
        // than requiring — that gap is exactly what the column is for.
        let resources = match SourceId::new(source.id.clone()) {
            Ok(id) => ctx.store.statuses(&id).await.map(|s| s.len()).unwrap_or(0),
            Err(_) => 0,
        };
        sources.push(ConfiguredSource {
            id: source.id.clone(),
            kind,
            target: Some(target),
            enabled: source.is_enabled(),
            resources,
            tracked: true,
        });
    }

    // Anything the store has been collecting that the config never named. Listing only
    // the config would answer "what did I declare"; the question being asked is "what
    // is here", and a source collected by hand is very much here.
    if !args.configured_only {
        for (id, inferred) in untracked(ctx, &config).await? {
            sources.push(ConfiguredSource {
                id: id.to_string(),
                kind: inferred.kind,
                target: inferred.target,
                // Not "disabled" — nothing turned it off. `run` ignores it because the
                // config does not mention it, which `tracked` is what says.
                enabled: false,
                resources: ctx.store.statuses(&id).await.map(|s| s.len()).unwrap_or(0),
                tracked: false,
            });
        }
    }

    Ok(SourceReport::List {
        config: path.map(|p| p.display().to_string()),
        sources,
    })
}

/// What the store already knows about a source, for one the config has not been told
/// about.
///
/// Everything here is read back out of `log/<source>/` — no network, no guessing from
/// the id. A source that was collected has necessarily recorded how it was reached, so
/// re-deriving its config block is reading, not inventing.
#[derive(Clone, Debug)]
struct Inferred {
    kind: SourceKind,
    /// `None` when the store proves the source exists but cannot say where from.
    target: Option<String>,
}

/// Reconstructs a source's config block from its log, or `None` if the log is empty.
///
/// The discriminator is `DiscoveryRun::method` — `playlist` for a channel, anything else
/// for a crawled site — which SPEC §4.3 records as provenance for exactly this kind of
/// question. Falling back to the natural keys covers a source collected with `ingest`,
/// which writes observations and never a DiscoveryRun.
async fn infer_from_store(ctx: &Ctx, id: &SourceId) -> anyhow::Result<Option<Inferred>> {
    let log = ctx.store.read_log(id).await?;
    if log.is_empty() {
        return Ok(None);
    }

    let method = log
        .iter()
        .rev()
        .find_map(|r| match r {
            LogRecord::DiscoveryRun(d) => Some(d.method.clone()),
            _ => None,
        })
        .unwrap_or_default();

    // Natural keys, newest last — a site's origin and a channel's video ids both come
    // from here.
    let keys: Vec<&str> = log
        .iter()
        .filter_map(|r| match r {
            LogRecord::Observation(o) => Some(o.resource.natural_key.as_str()),
            LogRecord::DiscoveryRun(d) => d.resources.first().map(|r| r.natural_key.as_str()),
            _ => None,
        })
        .collect();

    let looks_like_youtube = keys
        .iter()
        .any(|k| k.contains("youtube.com/") || k.contains("youtu.be/"));

    if method == "playlist" || (method.is_empty() && looks_like_youtube) {
        return Ok(Some(Inferred {
            kind: SourceKind::Channel,
            target: channel_url(ctx, &log).await,
        }));
    }

    Ok(Some(Inferred {
        kind: SourceKind::Site,
        target: keys.iter().find_map(|k| origin_of(k)),
    }))
}

/// The channel a stored recording came from.
///
/// Not in the log: a `DiscoveryRun` records the videos, not the channel they were listed
/// from. It *is* in the `yt-dlp -J` document archived beside each video, so this reads
/// one of those blobs back. That is the whole argument for keeping originals (§5.4) —
/// the metadata was retained without knowing this question would be asked.
async fn channel_url(ctx: &Ctx, log: &[LogRecord]) -> Option<String> {
    let metadata_part = format!("#{}", crate::youtube::Part::Metadata.as_str());
    let sha = log.iter().rev().find_map(|r| match r {
        LogRecord::Observation(o) if o.resource.natural_key.ends_with(&metadata_part) => {
            Some(o.blob_sha.clone())
        }
        _ => None,
    })?;

    let bytes = ctx.store.get_blob(&sha).await.ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;

    // `channel_url` is the canonical `/channel/UC…`; `uploader_url` is the `@handle`
    // form, which is what a person would have typed and reads better in the config.
    for key in ["uploader_url", "channel_url"] {
        if let Some(url) = json.get(key).and_then(|v| v.as_str())
            && !url.is_empty()
        {
            return Some(url.to_string());
        }
    }
    json.get("channel_id")
        .and_then(|v| v.as_str())
        .map(|id| format!("https://www.youtube.com/channel/{id}"))
}

/// `https://www.tampa.gov/some/page?x=1#frag` → `https://www.tampa.gov`.
fn origin_of(natural_key: &str) -> Option<String> {
    let url = url::Url::parse(natural_key).ok()?;
    let origin = url.origin().ascii_serialization();
    // Opaque origins (`data:`, `file:`) serialize to this and are not a site.
    (origin != "null").then_some(origin)
}

/// Every source the store holds that the config does not name.
async fn untracked(ctx: &Ctx, config: &Config) -> anyhow::Result<Vec<(SourceId, Inferred)>> {
    let mut out = Vec::new();
    for id in ctx.store.sources().await? {
        if config.source(id.as_str()).is_some() {
            continue;
        }
        if let Some(inferred) = infer_from_store(ctx, &id).await? {
            out.push((id, inferred));
        }
    }
    Ok(out)
}

async fn adopt(ctx: &Ctx, args: AdoptArgs) -> anyhow::Result<SourceReport> {
    let path = match &args.config {
        Some(p) => std::path::PathBuf::from(p),
        None => Config::write_path(),
    };
    let config = match Config::from_file(&path) {
        Ok(c) => c,
        Err(_) if !path.exists() => Config::default(),
        Err(e) => return Err(e),
    };

    let mut adopted = Vec::new();
    let mut skipped = Vec::new();
    for (id, inferred) in untracked(ctx, &config).await? {
        let Some(target) = inferred.target else {
            // The store has it but cannot say where from. Naming it is better than
            // writing a block that would fail on the next run.
            skipped.push(id.to_string());
            continue;
        };
        let source = match inferred.kind {
            SourceKind::Site => SourceConfig::site(id.to_string(), &target),
            SourceKind::Channel => SourceConfig::channel(id.to_string(), &target),
        };
        config::append_source(&path, &source)?;
        adopted.push(ConfiguredSource {
            id: id.to_string(),
            kind: inferred.kind,
            target: Some(target),
            enabled: true,
            resources: ctx.store.statuses(&id).await.map(|s| s.len()).unwrap_or(0),
            tracked: true,
        });
    }

    Ok(SourceReport::Adopt {
        config: path.display().to_string(),
        adopted,
        skipped,
    })
}

fn remove(args: RemoveArgs) -> anyhow::Result<SourceReport> {
    let path = match &args.config {
        Some(p) => std::path::PathBuf::from(p),
        None => Config::locate().ok_or_else(|| {
            anyhow::anyhow!(
                "no config file found; looked in {}",
                Config::search_paths()
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?,
    };
    config::remove_source(&path, &args.id)?;
    Ok(SourceReport::Remove {
        source: args.id,
        config: path.display().to_string(),
    })
}

// ── rendering ─────────────────────────────────────────────────────────────────

impl Render for SourceReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        match self {
            Self::Add {
                source,
                kind,
                target,
                config,
                run,
            } => {
                p.marked(
                    Mark::Ok,
                    format!(
                        "{} {} {}",
                        p.paint(source, Ink::Bold),
                        p.paint(kind_label(*kind), Ink::Dim),
                        render::truncate(target, p.width().saturating_sub(30)),
                    ),
                )?;
                p.nest(|p| p.line(p.paint(&format!("written to {config}"), Ink::Dim)))?;

                match run {
                    Some(report) => {
                        p.blank()?;
                        report.render(p)
                    }
                    // The command that turns a config entry into a corpus. Printed
                    // because `add` deliberately does not do it.
                    None => {
                        p.blank()?;
                        p.note(format!("centinel run --source {source}"))
                    }
                }
            }

            Self::Adopt {
                config,
                adopted,
                skipped,
            } => {
                if adopted.is_empty() && skipped.is_empty() {
                    return p.line(
                        p.paint("Nothing to adopt — the config already names every source in the store.", Ink::Dim),
                    );
                }

                for s in adopted {
                    p.marked(
                        Mark::Ok,
                        format!(
                            "{} {} {}",
                            p.paint(&format!("{:<20}", s.id), Ink::Bold),
                            p.paint(&format!("{:<8}", kind_label(s.kind)), Ink::Dim),
                            render::truncate(
                                s.target.as_deref().unwrap_or(""),
                                p.width().saturating_sub(34)
                            ),
                        ),
                    )?;
                }

                for id in skipped {
                    p.marked(
                        Mark::Warn,
                        format!(
                            "{} {}",
                            p.paint(&format!("{id:<20}"), Ink::Dim),
                            p.paint(
                                "the store cannot say where this came from — add it with \
                                 --site or --channel",
                                Ink::Dim
                            ),
                        ),
                    )?;
                }

                if !adopted.is_empty() {
                    p.blank()?;
                    p.line(p.paint(&format!("written to {config}"), Ink::Dim))?;
                    p.note("centinel run")?;
                }
                Ok(())
            }

            Self::Remove { source, config } => {
                p.marked(
                    Mark::Ok,
                    format!("removed {} from {config}", p.paint(source, Ink::Bold)),
                )?;
                // Removing intent does not remove evidence. Saying so here is cheaper
                // than someone discovering the blobs later and wondering.
                p.nest(|p| {
                    p.line(p.paint(
                        "collected data is untouched; the store still holds it",
                        Ink::Dim,
                    ))
                })
            }

            Self::List { config, sources } => {
                if sources.is_empty() {
                    p.line(p.paint("No sources configured.", Ink::Dim))?;
                    p.blank()?;
                    return p.note("centinel source add tampa --site https://www.tampa.gov");
                }

                // The state column earns its place only when something is in a state.
                // Kept unconditionally it is an empty column, and an empty column is a
                // double gutter that makes the row look misaligned.
                let needs_state = sources.iter().any(|s| !s.tracked || !s.enabled);

                let mut columns: Vec<(&str, Align)> = vec![
                    ("", Align::Left),
                    ("source", Align::Left),
                    ("kind", Align::Left),
                    ("resources", Align::Right),
                ];
                if needs_state {
                    columns.push(("", Align::Left));
                }
                columns.push(("target", Align::Left));

                let mut table = Table::new(&columns);
                for s in sources {
                    // Neither a disabled source nor an untracked one is broken, so
                    // neither gets a cross. They get no tick, and a word saying which.
                    let live = s.tracked && s.enabled;
                    let mark = if live { Mark::Ok } else { Mark::None };
                    let ink = if live { Ink::Plain } else { Ink::Dim };
                    let state = match (s.tracked, s.enabled) {
                        (false, _) => "untracked",
                        (true, false) => "disabled",
                        (true, true) => "",
                    };
                    let resources = if s.resources == 0 {
                        "—".to_string()
                    } else {
                        render::count(s.resources as u64)
                    };
                    let mut row = vec![
                        Cell::mark(mark),
                        Cell::new(&s.id, ink),
                        Cell::dim(kind_label(s.kind)),
                        Cell::new(resources, Ink::Dim),
                    ];
                    if needs_state {
                        row.push(Cell::new(state, Ink::Yellow));
                    }
                    row.push(Cell::new(
                        s.target
                            .as_deref()
                            .map(|t| render::truncate(t, 46))
                            .unwrap_or_else(|| "address unknown".to_string()),
                        if s.target.is_some() { ink } else { Ink::Dim },
                    ));
                    table.push(row);
                }
                p.table(&table)?;

                let untracked = sources.iter().filter(|s| !s.tracked).count();
                if untracked > 0 {
                    p.blank()?;
                    p.line(p.paint(
                        &format!(
                            "{} in the store but not in the config — `centinel run` skips {}.",
                            render::plural(untracked, "source is", "sources are"),
                            if untracked == 1 { "it" } else { "them" },
                        ),
                        Ink::Dim,
                    ))?;
                    p.note("centinel source adopt")?;
                }

                if let Some(path) = config {
                    p.blank()?;
                    p.line(p.paint(path, Ink::Dim))?;
                }
                Ok(())
            }
        }
    }
}

fn kind_label(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Site => "site",
        SourceKind::Channel => "channel",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_to_string(report: &SourceReport) -> String {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn adding_without_run_prints_the_command_that_runs_it() {
        let out = render_to_string(&SourceReport::Add {
            source: "tampa".into(),
            kind: SourceKind::Site,
            target: "https://www.tampa.gov".into(),
            config: "centinel.toml".into(),
            run: None,
        });
        assert!(out.contains("tampa"), "{out}");
        assert!(out.contains("centinel run --source tampa"), "{out}");
    }

    /// Removing a source removes intent, not evidence — and the output has to say so,
    /// because the opposite assumption is the one that gets made.
    #[test]
    fn removing_says_the_data_survives() {
        let out = render_to_string(&SourceReport::Remove {
            source: "tampa".into(),
            config: "centinel.toml".into(),
        });
        assert!(out.contains("untouched"), "{out}");
    }

    fn configured(id: &str, resources: usize) -> ConfiguredSource {
        ConfiguredSource {
            id: id.into(),
            kind: SourceKind::Site,
            target: Some(format!("https://{id}.gov")),
            enabled: true,
            resources,
            tracked: true,
        }
    }

    #[test]
    fn an_uncollected_source_is_visibly_uncollected() {
        let out = render_to_string(&SourceReport::List {
            config: Some("centinel.toml".into()),
            sources: vec![configured("tampa", 1847), configured("pinellas", 0)],
        });
        assert!(out.contains("1,847"), "{out}");
        assert!(out.contains('—'), "uncollected must be visible: {out}");
    }

    /// The gap this feature exists for: a source collected by hand is in the store and
    /// not in the config, so `run` silently ignores it. It has to be visible and it has
    /// to say what to do about it.
    #[test]
    fn an_untracked_source_is_shown_and_named_as_such() {
        let mut loose = configured("hillsborough", 412);
        loose.tracked = false;
        loose.enabled = false;

        let out = render_to_string(&SourceReport::List {
            config: Some("centinel.toml".into()),
            sources: vec![configured("tampa", 1847), loose],
        });
        assert!(out.contains("hillsborough"), "{out}");
        assert!(out.contains("untracked"), "{out}");
        assert!(out.contains("centinel source adopt"), "{out}");
        assert!(
            out.contains("`centinel run` skips it"),
            "the consequence has to be stated: {out}"
        );
    }

    /// With nothing loose, the prompt must not appear — a footer that is always there
    /// is a footer nobody reads.
    #[test]
    fn a_fully_tracked_store_gets_no_adopt_prompt() {
        let out = render_to_string(&SourceReport::List {
            config: Some("centinel.toml".into()),
            sources: vec![configured("tampa", 1847)],
        });
        assert!(!out.contains("adopt"), "{out}");
        assert!(!out.contains("untracked"), "{out}");
    }

    #[test]
    fn adopting_reports_what_it_wrote_and_what_it_could_not() {
        let out = render_to_string(&SourceReport::Adopt {
            config: "centinel.toml".into(),
            adopted: vec![configured("hillsborough", 412)],
            skipped: vec!["mystery".into()],
        });
        assert!(out.contains("hillsborough"), "{out}");
        assert!(out.contains("mystery"), "{out}");
        assert!(out.contains("--site or --channel"), "{out}");
        assert!(out.contains("centinel run"), "{out}");
    }

    #[test]
    fn adopting_nothing_says_so_plainly() {
        let out = render_to_string(&SourceReport::Adopt {
            config: "centinel.toml".into(),
            adopted: vec![],
            skipped: vec![],
        });
        assert!(out.contains("Nothing to adopt"), "{out}");
    }

    #[test]
    fn an_origin_is_taken_from_any_natural_key() {
        assert_eq!(
            origin_of("https://www.tampa.gov/some/page?x=1#frag").as_deref(),
            Some("https://www.tampa.gov")
        );
        assert_eq!(
            origin_of("http://example.gov:8080/a").as_deref(),
            Some("http://example.gov:8080")
        );
        assert_eq!(origin_of("not a url"), None);
        assert_eq!(origin_of("data:text/plain,hi"), None);
    }

    #[test]
    fn an_empty_config_points_at_the_command_that_fixes_it() {
        let out = render_to_string(&SourceReport::List {
            config: None,
            sources: vec![],
        });
        assert!(out.contains("centinel source add"), "{out}");
    }

    #[test]
    fn the_report_round_trips_through_json() {
        let report = SourceReport::List {
            config: Some("centinel.toml".into()),
            sources: vec![ConfiguredSource {
                id: "tampa".into(),
                kind: SourceKind::Site,
                target: Some("https://www.tampa.gov".into()),
                enabled: false,
                resources: 3,
                tracked: true,
            }],
        };
        let json = serde_json::to_value(&report).unwrap();
        let back: SourceReport = serde_json::from_value(json).unwrap();
        match back {
            SourceReport::List { sources, .. } => {
                assert_eq!(sources[0].id, "tampa");
                assert!(!sources[0].enabled);
            }
            _ => panic!("wrong variant"),
        }
    }

    /// `add` writes the block and stops unless told otherwise. The whole-pipeline
    /// version is one flag away, and that asymmetry is deliberate.
    #[tokio::test]
    async fn adding_writes_the_config_and_does_not_collect() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("centinel.toml");
        let store = crate::store::Store::open(dir.path().join("store"))
            .await
            .unwrap();
        let ctx = Ctx::new(store);

        let report = add(
            &ctx,
            AddArgs {
                id: "tampa".into(),
                site: Some("https://www.tampa.gov".into()),
                config: Some(path.display().to_string()),
                ..Default::default()
            },
            &Progress::none(),
        )
        .await
        .unwrap();

        match report {
            SourceReport::Add { source, run, .. } => {
                assert_eq!(source, "tampa");
                assert!(run.is_none(), "add must not collect without --run");
            }
            _ => panic!("wrong variant"),
        }

        let config = Config::from_file(&path).unwrap();
        assert_eq!(config.sources.len(), 1);
        assert_eq!(
            config.sources[0].site.as_deref(),
            Some("https://www.tampa.gov")
        );
    }

    #[tokio::test]
    async fn adding_a_source_with_neither_target_is_refused_before_it_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("centinel.toml");
        let store = crate::store::Store::open(dir.path().join("store"))
            .await
            .unwrap();
        let ctx = Ctx::new(store);

        let err = add(
            &ctx,
            AddArgs {
                id: "tampa".into(),
                config: Some(path.display().to_string()),
                ..Default::default()
            },
            &Progress::none(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("neither"), "{err}");
        assert!(!path.exists(), "a refused add must not create a config");
    }

    #[tokio::test]
    async fn listing_reports_configured_sources_the_store_has_never_seen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("centinel.toml");
        std::fs::write(
            &path,
            "[[source]]\nid = \"tampa\"\nsite = \"https://www.tampa.gov\"\n",
        )
        .unwrap();
        let store = crate::store::Store::open(dir.path().join("store"))
            .await
            .unwrap();
        let ctx = Ctx::new(store);

        let report = list(
            &ctx,
            ListArgs {
                config: Some(path.display().to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        match report {
            SourceReport::List { sources, .. } => {
                assert_eq!(sources.len(), 1);
                assert_eq!(sources[0].resources, 0);
                assert!(sources[0].enabled);
                assert!(sources[0].tracked);
            }
            _ => panic!("wrong variant"),
        }
    }

    // ── inferring from the store ───────────────────────────────────────────────

    /// A store with one source that was collected without ever being configured.
    async fn store_with_a_crawled_site(dir: &std::path::Path) -> Ctx {
        let store = crate::store::Store::open(dir.join("store")).await.unwrap();
        let id = SourceId::new("hillsborough").unwrap();
        store
            .append(
                &id,
                &LogRecord::DiscoveryRun(crate::domain::DiscoveryRun {
                    source: id.clone(),
                    at: jiff::Timestamp::now(),
                    resources: vec![Resource::new(
                        id.clone(),
                        "https://www.hillsboroughcounty.org/en/residents",
                    )],
                    method: "sitemap".into(),
                }),
            )
            .await
            .unwrap();
        Ctx::new(store)
    }

    #[tokio::test]
    async fn a_crawled_site_is_recovered_from_its_discovery_run() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = store_with_a_crawled_site(dir.path()).await;

        let got = infer_from_store(&ctx, &SourceId::new("hillsborough").unwrap())
            .await
            .unwrap()
            .expect("the store holds this source");
        assert_eq!(got.kind, SourceKind::Site);
        assert_eq!(got.target.as_deref(), Some("https://www.hillsboroughcounty.org"));
    }

    /// The channel URL is not in the log — it is in the archived `yt-dlp -J` document,
    /// which is exactly the "keep the original" argument paying off.
    #[tokio::test]
    async fn a_channel_is_recovered_from_the_archived_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().join("store"))
            .await
            .unwrap();
        let id = SourceId::new("tampa-council").unwrap();

        store
            .append(
                &id,
                &LogRecord::DiscoveryRun(crate::domain::DiscoveryRun {
                    source: id.clone(),
                    at: jiff::Timestamp::now(),
                    resources: vec![Resource::new(
                        id.clone(),
                        "https://www.youtube.com/watch?v=abc123",
                    )],
                    method: "playlist".into(),
                }),
            )
            .await
            .unwrap();

        let metadata = serde_json::json!({
            "title": "Council Meeting",
            "channel_id": "UCLzohJmEgvfJOEd4YJNIHbg",
            "channel_url": "https://www.youtube.com/channel/UCLzohJmEgvfJOEd4YJNIHbg",
            "uploader_url": "https://www.youtube.com/@CityofTampa",
        });
        let key = crate::youtube::sub_resource("abc123", crate::youtube::Part::Metadata);
        store
            .record_observation(
                &Resource::new(id.clone(), &key),
                metadata.to_string().as_bytes(),
                jiff::Timestamp::now(),
                Default::default(),
            )
            .await
            .unwrap();

        let ctx = Ctx::new(store);
        let got = infer_from_store(&ctx, &id).await.unwrap().unwrap();
        assert_eq!(got.kind, SourceKind::Channel);
        // The handle form, not the /channel/UC… one — it is what a person would type.
        assert_eq!(
            got.target.as_deref(),
            Some("https://www.youtube.com/@CityofTampa")
        );
    }

    #[tokio::test]
    async fn a_source_the_store_has_never_heard_of_infers_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = store_with_a_crawled_site(dir.path()).await;
        assert!(
            infer_from_store(&ctx, &SourceId::new("nobody").unwrap())
                .await
                .unwrap()
                .is_none()
        );
    }

    /// The whole point: listing surfaces what was collected outside the config.
    #[tokio::test]
    async fn listing_includes_sources_the_config_never_named() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = store_with_a_crawled_site(dir.path()).await;
        let path = dir.path().join("centinel.toml");
        std::fs::write(&path, "[[source]]\nid = \"tampa\"\nsite = \"https://t.gov\"\n").unwrap();

        let report = list(
            &ctx,
            ListArgs {
                config: Some(path.display().to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        match report {
            SourceReport::List { sources, .. } => {
                assert_eq!(sources.len(), 2, "{sources:?}");
                assert_eq!(sources[0].id, "tampa");
                assert!(sources[0].tracked);
                assert_eq!(sources[1].id, "hillsborough");
                assert!(!sources[1].tracked, "the loose one must be marked");
                assert_eq!(
                    sources[1].target.as_deref(),
                    Some("https://www.hillsboroughcounty.org")
                );
            }
            _ => panic!("wrong variant"),
        }
    }

    #[tokio::test]
    async fn configured_only_ignores_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = store_with_a_crawled_site(dir.path()).await;
        let path = dir.path().join("centinel.toml");
        std::fs::write(&path, "[[source]]\nid = \"tampa\"\nsite = \"https://t.gov\"\n").unwrap();

        let report = list(
            &ctx,
            ListArgs {
                config: Some(path.display().to_string()),
                configured_only: true,
            },
        )
        .await
        .unwrap();
        match report {
            SourceReport::List { sources, .. } => assert_eq!(sources.len(), 1),
            _ => panic!("wrong variant"),
        }
    }

    /// Adding by id alone, for the very common "I already crawled it, now track it" case.
    #[tokio::test]
    async fn adding_without_a_target_takes_it_from_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = store_with_a_crawled_site(dir.path()).await;
        let path = dir.path().join("centinel.toml");

        add(
            &ctx,
            AddArgs {
                id: "hillsborough".into(),
                config: Some(path.display().to_string()),
                ..Default::default()
            },
            &Progress::none(),
        )
        .await
        .unwrap();

        let config = Config::from_file(&path).unwrap();
        assert_eq!(
            config.sources[0].site.as_deref(),
            Some("https://www.hillsboroughcounty.org")
        );
    }

    #[tokio::test]
    async fn adopting_writes_every_loose_source_and_is_then_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = store_with_a_crawled_site(dir.path()).await;
        let path = dir.path().join("centinel.toml");

        let report = adopt(
            &ctx,
            AdoptArgs {
                config: Some(path.display().to_string()),
            },
        )
        .await
        .unwrap();
        match report {
            SourceReport::Adopt {
                adopted, skipped, ..
            } => {
                assert_eq!(adopted.len(), 1);
                assert_eq!(adopted[0].id, "hillsborough");
                assert!(adopted[0].tracked);
                assert!(skipped.is_empty());
            }
            _ => panic!("wrong variant"),
        }

        // Idempotent: everything is tracked now, so a second pass has nothing to do.
        let again = adopt(
            &ctx,
            AdoptArgs {
                config: Some(path.display().to_string()),
            },
        )
        .await
        .unwrap();
        match again {
            SourceReport::Adopt { adopted, .. } => assert!(adopted.is_empty()),
            _ => panic!("wrong variant"),
        }
        assert_eq!(Config::from_file(&path).unwrap().sources.len(), 1);
    }

    #[tokio::test]
    async fn removing_takes_it_out_of_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("centinel.toml");
        std::fs::write(
            &path,
            "[[source]]\nid = \"a\"\nsite = \"https://a.gov\"\n\n\
             [[source]]\nid = \"b\"\nsite = \"https://b.gov\"\n",
        )
        .unwrap();

        remove(RemoveArgs {
            id: "a".into(),
            config: Some(path.display().to_string()),
        })
        .unwrap();

        let config = Config::from_file(&path).unwrap();
        assert_eq!(config.sources.len(), 1);
        assert_eq!(config.sources[0].id, "b");
    }
}
