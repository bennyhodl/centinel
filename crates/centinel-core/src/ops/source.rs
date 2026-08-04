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
    /// List configured sources and whether anything has been collected for them.
    List(ListArgs),
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

/// One configured source, with what the store has for it.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ConfiguredSource {
    pub id: String,
    pub kind: SourceKind,
    pub target: String,
    pub enabled: bool,
    /// Resources the store holds under this id. Zero means it is configured but
    /// uncollected — the state a fresh `source add` leaves behind.
    pub resources: usize,
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
        sources: Vec<ConfiguredSource>,
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
        SourceAction::Remove(a) => remove(a),
    }
}

async fn add(ctx: &Ctx, args: AddArgs, progress: &Progress) -> anyhow::Result<SourceReport> {
    let path = match &args.config {
        Some(p) => std::path::PathBuf::from(p),
        None => Config::write_path(),
    };

    let source = SourceConfig {
        id: args.id.clone(),
        site: args.site.clone(),
        channel: args.channel.clone(),
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
            target,
            enabled: source.is_enabled(),
            resources,
        });
    }

    Ok(SourceReport::List {
        config: path.map(|p| p.display().to_string()),
        sources,
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

                let mut table = Table::new(&[
                    ("", Align::Left),
                    ("source", Align::Left),
                    ("kind", Align::Left),
                    ("resources", Align::Right),
                    ("target", Align::Left),
                ]);
                for s in sources {
                    // A disabled source is not a broken one — it renders as absence of a
                    // tick rather than as a cross.
                    let mark = if s.enabled { Mark::Ok } else { Mark::None };
                    let ink = if s.enabled { Ink::Plain } else { Ink::Dim };
                    let resources = if s.resources == 0 {
                        "—".to_string()
                    } else {
                        render::count(s.resources as u64)
                    };
                    table.push(vec![
                        Cell::mark(mark),
                        Cell::new(&s.id, ink),
                        Cell::dim(kind_label(s.kind)),
                        Cell::new(resources, Ink::Dim),
                        Cell::new(render::truncate(&s.target, 46), ink),
                    ]);
                }
                p.table(&table)?;

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

    #[test]
    fn an_uncollected_source_is_visibly_uncollected() {
        let out = render_to_string(&SourceReport::List {
            config: Some("centinel.toml".into()),
            sources: vec![
                ConfiguredSource {
                    id: "tampa".into(),
                    kind: SourceKind::Site,
                    target: "https://www.tampa.gov".into(),
                    enabled: true,
                    resources: 1847,
                },
                ConfiguredSource {
                    id: "pinellas".into(),
                    kind: SourceKind::Site,
                    target: "https://pinellas.gov".into(),
                    enabled: true,
                    resources: 0,
                },
            ],
        });
        assert!(out.contains("1,847"), "{out}");
        assert!(out.contains('—'), "uncollected must be visible: {out}");
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
                target: "https://www.tampa.gov".into(),
                enabled: false,
                resources: 3,
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
            },
        )
        .await
        .unwrap();

        match report {
            SourceReport::List { sources, .. } => {
                assert_eq!(sources.len(), 1);
                assert_eq!(sources[0].resources, 0);
                assert!(sources[0].enabled);
            }
            _ => panic!("wrong variant"),
        }
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
