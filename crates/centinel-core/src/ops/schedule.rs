//! `schedule` — writing the cadences `serve` fires.
//!
//! The peer of [`crate::ops::source`], and it exists for the same reason: this edits a file
//! a person wrote and will read again, and the mistakes that file invites here are silent
//! until 3am. A `sources` entry naming no `[[source]]` block, and `0 3 * * 1` written when
//! `0 3 1 * *` was meant — Mondays instead of the 1st — both parse cleanly and both produce
//! a schedule that looks fine and does the wrong thing, or nothing.
//!
//! ## `Operator`, because this is the file authority comes from
//!
//! A scheduled run is the operator's instruction, executed later. This op writes those
//! instructions, so reachable over HTTP it would be privilege escalation with extra
//! steps — an unauthenticated caller adding a block that a server then fires forever.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::{self, Config, ScheduleConfig};
use crate::prelude::*;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct ScheduleArgs {
    #[command(subcommand)]
    pub action: ScheduleAction,
}

#[derive(Clone, Debug, clap::Subcommand, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum ScheduleAction {
    /// Write a schedule into the config.
    Set(SetArgs),
    /// Remove a schedule. Collected data and history are left alone.
    Rm(RmArgs),
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct SetArgs {
    /// Schedule id, e.g. `tampa-daily`. Names it in the journal and in every report.
    ///
    /// Optional on a terminal: with nothing given, the CLI asks. It is required
    /// everywhere else — see the module doc on why the op itself never prompts.
    #[arg(value_name = "ID")]
    #[serde(default)]
    pub id: Option<String>,

    /// A 5-field cron expression, or a shorthand like `@daily`.
    #[arg(long, value_name = "EXPR")]
    #[serde(default)]
    pub cron: Option<String>,

    /// IANA zone name. Defaults to the host's.
    #[arg(long, value_name = "ZONE")]
    #[serde(default)]
    pub tz: Option<String>,

    /// Source to run. Repeatable. Omit for every enabled source.
    #[arg(long = "source", value_name = "ID")]
    #[serde(default)]
    pub sources: Vec<String>,

    /// Stage to skip. Repeatable.
    #[arg(long, value_name = "STAGE")]
    #[serde(default)]
    pub skip: Vec<String>,

    /// Stop collection after this many addresses, per source.
    #[arg(long)]
    #[serde(default)]
    pub limit: Option<usize>,

    /// Re-fetch and re-derive everything at every fire. Expensive, and deliberate.
    #[arg(long)]
    #[serde(default)]
    pub refresh: bool,

    /// Seconds of jitter. Zero fires exactly on the minute.
    #[arg(long, value_name = "SECONDS")]
    #[serde(default)]
    pub jitter_secs: Option<u64>,

    /// Write the block but leave it disarmed.
    #[arg(long)]
    #[serde(default)]
    pub disabled: bool,

    /// Do not fire on startup when overdue.
    #[arg(long)]
    #[serde(default)]
    pub no_catch_up: bool,

    /// Replace an existing schedule with this id.
    #[arg(long)]
    #[serde(default)]
    pub replace: bool,

    /// Config file to write. Defaults to the one in effect, else `./centinel.toml`.
    #[arg(long, value_name = "FILE")]
    #[serde(default)]
    pub config: Option<String>,
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct RmArgs {
    /// Schedule id to remove.
    pub id: String,

    /// Config file to edit.
    #[arg(long, value_name = "FILE")]
    #[serde(default)]
    pub config: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum ScheduleReport {
    Set {
        schedule: String,
        cron: String,
        tz: String,
        /// The file that was written.
        config: String,
        /// The next few fire times, in the schedule's own zone.
        ///
        /// Printed on the way out for the same reason the selector previews them on the
        /// way in: three dates settle whether `0 3 * * 1` meant Mondays or the 1st, and
        /// this is the last cheap moment to notice it did not.
        next: Vec<String>,
        /// Whether a running server has yet to pick this up.
        needs_reload: bool,
    },
    Rm {
        schedule: String,
        config: String,
        needs_reload: bool,
    },
}

/// Write and remove the cadences `centinel serve` fires runs on.
#[op(reach = "operator", group = "pipeline")]
pub async fn schedule(ctx: &Ctx, args: ScheduleArgs) -> anyhow::Result<ScheduleReport> {
    match args.action {
        ScheduleAction::Set(a) => set(ctx, a).await,
        ScheduleAction::Rm(a) => rm(ctx, a).await,
    }
}

async fn set(ctx: &Ctx, args: SetArgs) -> anyhow::Result<ScheduleReport> {
    let path = match &args.config {
        Some(p) => std::path::PathBuf::from(p),
        None => Config::write_path(),
    };

    // Required here rather than in the argument type, because on a terminal the CLI fills
    // these in by asking. The op never prompts: one that did would block an MCP call until
    // the client timed out and hang a script forever, with no output explaining why.
    let id = args.id.clone().ok_or_else(|| {
        anyhow::anyhow!("a schedule needs an id; give one, or run this on a terminal to be asked")
    })?;
    let cron = args.cron.clone().ok_or_else(|| {
        anyhow::anyhow!("a schedule needs a cadence: --cron \"0 3 * * *\", or @daily")
    })?;

    let block = ScheduleConfig {
        id: id.clone(),
        cron,
        tz: args.tz.clone(),
        jitter_secs: args.jitter_secs,
        enabled: args.disabled.then_some(false),
        catch_up: args.no_catch_up.then_some(false),
        sources: args.sources.clone(),
        skip: args.skip.clone(),
        limit: args.limit,
        refresh: args.refresh,
    };

    // Replacing is remove-then-append rather than an in-place rewrite, so the one code
    // path that validates an addition is the one that runs.
    if args.replace && path.exists() {
        let existing = Config::from_file(&path)?;
        if existing.schedule(&id).is_some() {
            config::remove_schedule(&path, &id)?;
        }
    }

    config::append_schedule(&path, &block)?;

    // Read back out of the file rather than off the struct: what the operator will get is
    // what the file says, and the two have now been through a parser.
    let written = Config::from_file(&path)?;
    let saved = written
        .schedule(&id)
        .ok_or_else(|| anyhow::anyhow!("wrote {} but `{id}` is not in it", path.display()))?;
    let zone = saved.zone()?;
    let next = saved
        .cron()?
        .next_n(jiff::Timestamp::now(), &zone, 3)
        .into_iter()
        .map(|at| {
            at.to_zoned(zone.clone())
                .strftime("%a %e %b %H:%M %Z")
                .to_string()
        })
        .collect();

    Ok(ScheduleReport::Set {
        schedule: id,
        cron: saved.cron.clone(),
        tz: zone.iana_name().unwrap_or("local").to_string(),
        config: path.display().to_string(),
        next,
        needs_reload: server_may_be_running(ctx),
    })
}

async fn rm(ctx: &Ctx, args: RmArgs) -> anyhow::Result<ScheduleReport> {
    let path = match &args.config {
        Some(p) => std::path::PathBuf::from(p),
        None => Config::locate().unwrap_or_else(Config::write_path),
    };
    config::remove_schedule(&path, &args.id)?;

    Ok(ScheduleReport::Rm {
        schedule: args.id,
        config: path.display().to_string(),
        needs_reload: server_may_be_running(ctx),
    })
}

/// Whether to tell the operator their edit has not taken effect yet.
///
/// There is no way to ask "is a server running against this store" that is both cheap and
/// correct, so this answers the honest, useless-to-lie-about version: **always true**. A
/// line that is sometimes wrong in the direction of "check your server" costs a glance; the
/// other direction costs a fortnight of a schedule the operator believes is armed.
fn server_may_be_running(_ctx: &Ctx) -> bool {
    true
}

impl Render for ScheduleReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        match self {
            Self::Set {
                schedule,
                cron,
                tz,
                config,
                next,
                needs_reload,
            } => {
                p.title(schedule, &format!("{cron} {tz}"))?;
                p.nest(|p| {
                    if !next.is_empty() {
                        p.kv("next", 6, p.paint(&next.join(" · "), Ink::Dim))?;
                    }
                    p.marked(Mark::Ok, p.paint(&format!("wrote {config}"), Ink::Dim))
                })?;
                if *needs_reload {
                    p.note("a running server has not picked this up; send SIGHUP or restart")?;
                }
                Ok(())
            }
            Self::Rm {
                schedule,
                config,
                needs_reload,
            } => {
                p.marked(
                    Mark::Ok,
                    format!("removed {} from {config}", p.paint(schedule, Ink::Bold)),
                )?;
                p.nest(|p| p.wrapped("collected data and run history are untouched", Ink::Dim))?;
                if *needs_reload {
                    p.note("a running server has not picked this up; send SIGHUP or restart")?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    async fn ctx() -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        (dir, Ctx::new(store))
    }

    fn config_file(dir: &tempfile::TempDir) -> String {
        let path = dir.path().join("centinel.toml");
        std::fs::write(
            &path,
            "# somebody's comment\n[[source]]\nid = \"tampa\"\nsite = \"https://tampa.gov\"\n",
        )
        .unwrap();
        path.display().to_string()
    }

    fn set_args(config: &str) -> SetArgs {
        SetArgs {
            id: Some("daily".into()),
            cron: Some("0 3 * * *".into()),
            tz: Some("America/New_York".into()),
            sources: vec!["tampa".into()],
            config: Some(config.into()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn setting_writes_a_block_and_previews_the_next_fires() {
        let (_d, ctx) = ctx().await;
        let dir = tempfile::tempdir().unwrap();
        let config = config_file(&dir);

        let report = schedule(
            &ctx,
            ScheduleArgs {
                action: ScheduleAction::Set(set_args(&config)),
            },
        )
        .await
        .unwrap();

        let ScheduleReport::Set { next, tz, .. } = &report else {
            panic!("expected a Set report");
        };
        assert_eq!(tz, "America/New_York");
        assert_eq!(next.len(), 3, "three dates settle Mondays vs the 1st");

        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.contains("# somebody's comment"), "{text}");
        assert!(text.contains("[[schedule]]"), "{text}");
    }

    /// The op is reachable from MCP's schema and from a script, and neither can answer a
    /// prompt. It has to fail with what is missing rather than wait on stdin.
    #[tokio::test]
    async fn a_missing_id_or_cadence_is_an_error_not_a_prompt() {
        let (_d, ctx) = ctx().await;
        let dir = tempfile::tempdir().unwrap();
        let config = config_file(&dir);

        let err = schedule(
            &ctx,
            ScheduleArgs {
                action: ScheduleAction::Set(SetArgs {
                    id: None,
                    ..set_args(&config)
                }),
            },
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("id"), "{err}");

        let err = schedule(
            &ctx,
            ScheduleArgs {
                action: ScheduleAction::Set(SetArgs {
                    cron: None,
                    ..set_args(&config)
                }),
            },
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("cadence"), "{err}");
    }

    #[tokio::test]
    async fn a_second_schedule_with_the_same_id_needs_replace() {
        let (_d, ctx) = ctx().await;
        let dir = tempfile::tempdir().unwrap();
        let config = config_file(&dir);

        let args = ScheduleArgs {
            action: ScheduleAction::Set(set_args(&config)),
        };
        schedule(&ctx, args.clone()).await.unwrap();

        let err = schedule(&ctx, args).await.unwrap_err().to_string();
        assert!(err.contains("already"), "{err}");

        // With `--replace` it is one block, not two.
        schedule(
            &ctx,
            ScheduleArgs {
                action: ScheduleAction::Set(SetArgs {
                    cron: Some("@weekly".into()),
                    replace: true,
                    ..set_args(&config)
                }),
            },
        )
        .await
        .unwrap();

        let parsed = Config::from_file(std::path::Path::new(&config)).unwrap();
        assert_eq!(parsed.schedules.len(), 1);
        assert_eq!(parsed.schedules[0].cron, "@weekly");
    }

    /// The relational mistake this op exists to catch, checked before anything is written.
    #[tokio::test]
    async fn a_schedule_naming_an_unknown_source_is_refused_and_writes_nothing() {
        let (_d, ctx) = ctx().await;
        let dir = tempfile::tempdir().unwrap();
        let config = config_file(&dir);

        let err = schedule(
            &ctx,
            ScheduleArgs {
                action: ScheduleAction::Set(SetArgs {
                    sources: vec!["orlando".into()],
                    ..set_args(&config)
                }),
            },
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("orlando"), "{err}");

        let text = std::fs::read_to_string(&config).unwrap();
        assert!(
            !text.contains("[[schedule]]"),
            "it was written anyway: {text}"
        );
    }

    #[tokio::test]
    async fn removing_leaves_the_sources_and_the_comments() {
        let (_d, ctx) = ctx().await;
        let dir = tempfile::tempdir().unwrap();
        let config = config_file(&dir);

        schedule(
            &ctx,
            ScheduleArgs {
                action: ScheduleAction::Set(set_args(&config)),
            },
        )
        .await
        .unwrap();

        schedule(
            &ctx,
            ScheduleArgs {
                action: ScheduleAction::Rm(RmArgs {
                    id: "daily".into(),
                    config: Some(config.clone()),
                }),
            },
        )
        .await
        .unwrap();

        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.contains("# somebody's comment"), "{text}");
        let parsed = Config::parse(&text).unwrap();
        assert!(parsed.schedules.is_empty());
        assert_eq!(parsed.sources.len(), 1);
    }
}
