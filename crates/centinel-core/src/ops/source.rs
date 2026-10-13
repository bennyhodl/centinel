//! `source` — what the corpus is made of.
//!
//! The store's list of sources, read and changed through [`crate::catalog::Sources`], so
//! this op and the web's Sources page change one list. It exists rather than leaving
//! people to write `source.toml` files because the two mistakes that invites — a
//! duplicate id, and an id that is not a legal directory name — are both silent until a
//! run is well underway.
//!
//! ## Adding is not collecting
//!
//! `source add` defines the source and stops. `--run` chains straight into the pipeline
//! for the case where that was obviously the intent, but it is opt-in: a command whose
//! name is "add" should not, by default, start an hour of network traffic against a city.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::catalog::Sources;
use crate::config::SourceConfig;
use crate::prelude::*;
use crate::sources;

use super::run::{RunArgs, RunReport};

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct SourceArgs {
    #[command(subcommand)]
    pub action: SourceAction,
}

#[derive(Clone, Debug, clap::Subcommand, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum SourceAction {
    /// Define a source in the store.
    Add(AddArgs),
    /// List the store's sources.
    List,
    /// Include a source in a bare `centinel run` again.
    Enable(IdArgs),
    /// Leave a source out of a bare `centinel run`. Naming it still runs it.
    Disable(IdArgs),
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

    /// Pin the enumeration strategy, e.g. `--strategy=listing`. `centinel investigate`
    /// prints this flag, already filled in, when something recognised the address.
    ///
    /// Omit it and the registry is asked on every run, which is the right default. Pin one
    /// when `centinel investigate` showed you the evidence and you accepted it: a pinned
    /// strategy that later stops recognising its own site still runs, and says so, rather
    /// than switching silently to a weaker one and collecting a front page.
    #[arg(long, value_name = "NAME")]
    #[serde(default)]
    pub strategy: Option<String>,

    /// Define it but leave it out of a bare `centinel run`.
    #[arg(long)]
    #[serde(default)]
    pub disabled: bool,

    /// Add it, then run the full pipeline for it immediately.
    #[arg(long)]
    #[serde(default)]
    pub run: bool,
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct IdArgs {
    /// Source id.
    pub id: String,
}

/// One source the store defines.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListedSource {
    pub id: String,
    pub kind: SourceKind,
    pub target: String,
    pub enabled: bool,
    /// Addresses the log knows under this id, discovered or collected — the same count
    /// `centinel list` reports. Zero means defined and never run, the state a fresh
    /// `source add` leaves behind.
    pub resources: usize,
    /// Addresses per liveness state (`live`, `gone`, `blocked`, `error`). Their sum is what
    /// has been fetched; `resources` less that sum is what discovery found and collection has
    /// not reached yet.
    #[serde(default)]
    pub liveness: std::collections::BTreeMap<String, usize>,
    /// The latest run that covered it, from the journal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<LastRun>,
}

/// A run, as a row of the source list needs it.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct LastRun {
    pub started_at: String,
    pub outcome: crate::journal::Outcome,
    pub trigger: crate::journal::Trigger,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    /// Documents it newly stored, across every source it ran.
    pub documents: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum SourceReport {
    Add {
        source: String,
        kind: SourceKind,
        target: String,
        /// Present when `--run` was given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run: Option<Box<RunReport>>,
    },
    List {
        /// By id.
        sources: Vec<ListedSource>,
    },
    Toggle {
        source: String,
        enabled: bool,
    },
}

/// Add, list, enable and disable the sources `centinel run` walks.
#[op(long_running, reach = "operator", group = "pipeline")]
pub async fn source(
    ctx: &Ctx,
    args: SourceArgs,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<SourceReport> {
    match args.action {
        SourceAction::Add(a) => add(ctx, a, progress, cancel).await,
        SourceAction::List => list(ctx).await,
        SourceAction::Enable(a) => toggle(ctx, &a.id, true).await,
        SourceAction::Disable(a) => toggle(ctx, &a.id, false).await,
    }
}

async fn add(
    ctx: &Ctx,
    args: AddArgs,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<SourceReport> {
    // Checked before anything is written. A name nothing answers to would sit in the
    // definition and fail on the first run, and the error there names an address rather
    // than a typo.
    if let Some(name) = &args.strategy {
        crate::strategies::crawl::by_name(name)?;
    }

    // Neither target given: the log may already know. Someone who ran `centinel discover
    // --source tampa --site …` should not have to retype an address the log has been
    // holding all along.
    let (mut site, mut channel) = (args.site.clone(), args.channel.clone());
    if site.is_none() && channel.is_none() {
        let id = SourceId::new(args.id.clone())?;
        if let Some(inferred) = sources::infer(&ctx.store, &id).await? {
            let recovered = inferred.to_config(&id).ok_or_else(|| {
                anyhow::anyhow!(
                    "the store holds `{}` but not enough to say where it came from; \
                     give --site or --channel",
                    args.id
                )
            })?;
            progress.say(format!(
                "inferred {} from the store",
                recovered.acquisition()?.target()
            ));
            site = recovered.site;
            channel = recovered.channel;
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
        // today's answer into the definition and make a later change invisible.
        audio_if_no_captions: None,
        lang: None,
        // Absent unless asked for. An absent key means "ask the registry every run", which
        // is right for a command that has fetched nothing and seen no evidence. `--strategy`
        // is how an operator who *has* seen it — from `centinel investigate` — says so.
        strategy: args.strategy.clone(),
    };

    let acquisition = source.acquisition()?;
    let (kind, target) = (acquisition.kind(), acquisition.target().to_string());

    Sources::new(&ctx.store).add(&source).await?;
    progress.say(format!("added {}", args.id));

    let run = if args.run {
        let report = super::run::run(
            ctx,
            RunArgs {
                sources: vec![args.id.clone()],
                ..Default::default()
            },
            progress,
            cancel,
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
        run,
    })
}

async fn list(ctx: &Ctx) -> anyhow::Result<SourceReport> {
    let mut out = Vec::new();
    let summaries = crate::summary::Summaries::new(&ctx.store);
    // Newest first, read once: every row looks for its latest run in the same list.
    let mut attempts = crate::journal::Journal::new(&ctx.store).read().await?;
    attempts.reverse();
    for source in Sources::new(&ctx.store).list().await? {
        let acquisition = source.acquisition()?;
        // Every address the log knows, discovered or collected — the count `centinel list`
        // and the web's source page give, so no two surfaces disagree about one source.
        // From the summary rather than a replay: it is the replay's own number, kept
        // beside the log and rebuilt when the log grows, and the list is asked every few
        // seconds by a page that replaying sixteen logs made wait over a second.
        let summary = summaries.get(&SourceId::new(source.id.clone())?).await?;
        let last_run = attempts.iter().find(|a| a.covers(&source.id)).map(|a| LastRun {
            started_at: a.started_at.clone(),
            outcome: a.outcome,
            trigger: a.trigger,
            schedule: a.schedule.clone(),
            documents: a.added.documents,
        });
        out.push(ListedSource {
            id: source.id.clone(),
            kind: acquisition.kind(),
            target: acquisition.target().to_string(),
            enabled: source.is_enabled(),
            resources: summary.resources,
            liveness: summary.liveness,
            last_run,
        });
    }
    Ok(SourceReport::List { sources: out })
}

async fn toggle(ctx: &Ctx, id: &str, enabled: bool) -> anyhow::Result<SourceReport> {
    let source = Sources::new(&ctx.store).set_enabled(id, enabled).await?;
    Ok(SourceReport::Toggle {
        source: source.id,
        enabled,
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
                run,
            } => {
                p.marked(
                    Mark::Ok,
                    format!(
                        "{} {} {}",
                        p.paint(source, Ink::Bold),
                        p.paint(kind.label(), Ink::Dim),
                        render::truncate(target, p.width().saturating_sub(30)),
                    ),
                )?;
                match run {
                    Some(report) => {
                        p.blank()?;
                        report.render(p)
                    }
                    // The command that turns a definition into a corpus. Printed
                    // because `add` deliberately does not do it.
                    None => {
                        p.blank()?;
                        p.note(format!("centinel run --source {source}"))
                    }
                }
            }

            Self::Toggle { source, enabled } => p.marked(
                Mark::Ok,
                format!(
                    "{} {}",
                    p.paint(source, Ink::Bold),
                    match enabled {
                        true => "is in a bare `centinel run` again",
                        false => "is left out of a bare `centinel run`; naming it still runs it",
                    }
                ),
            ),

            Self::List { sources } => {
                if sources.is_empty() {
                    p.line(p.paint("No sources in this store.", Ink::Dim))?;
                    p.blank()?;
                    return p.note("centinel source add tampa --site https://www.tampa.gov");
                }

                // The state column earns its place only when something is in a state.
                // Kept unconditionally it is an empty column, and an empty column is a
                // double gutter that makes the row look misaligned.
                let needs_state = sources.iter().any(|s| !s.enabled);

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
                    // A disabled source is not broken, so it gets no cross. It gets no
                    // tick, and a word saying why.
                    let mark = if s.enabled { Mark::Ok } else { Mark::None };
                    let ink = if s.enabled { Ink::Plain } else { Ink::Dim };
                    let resources = if s.resources == 0 {
                        "—".to_string()
                    } else {
                        render::count(s.resources as u64)
                    };
                    let mut row = vec![
                        Cell::mark(mark),
                        Cell::new(&s.id, ink),
                        Cell::dim(s.kind.label()),
                        Cell::new(resources, Ink::Dim),
                    ];
                    if needs_state {
                        row.push(Cell::new(if s.enabled { "" } else { "disabled" }, Ink::Yellow));
                    }
                    row.push(Cell::new(render::truncate(&s.target, 46), ink));
                    table.push(row);
                }
                p.table(&table)
            }
        }
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
            run: None,
        });
        assert!(out.contains("tampa"), "{out}");
        assert!(out.contains("centinel run --source tampa"), "{out}");
    }

    fn listed(id: &str, resources: usize) -> ListedSource {
        ListedSource {
            id: id.into(),
            kind: SourceKind::Site,
            target: format!("https://{id}.gov"),
            enabled: true,
            resources,
            liveness: Default::default(),
            last_run: None,
        }
    }

    #[test]
    fn an_uncollected_source_is_visibly_uncollected() {
        let out = render_to_string(&SourceReport::List {
            sources: vec![listed("tampa", 1847), listed("pinellas", 0)],
        });
        assert!(out.contains("1,847"), "{out}");
        assert!(out.contains('—'), "uncollected must be visible: {out}");
    }

    #[test]
    fn a_disabled_source_says_so() {
        let mut off = listed("pinellas", 3);
        off.enabled = false;
        let out = render_to_string(&SourceReport::List {
            sources: vec![listed("tampa", 1847), off],
        });
        assert!(out.contains("disabled"), "{out}");
    }

    #[test]
    fn an_empty_store_points_at_the_command_that_fixes_it() {
        let out = render_to_string(&SourceReport::List { sources: vec![] });
        assert!(out.contains("centinel source add"), "{out}");
    }

    #[test]
    fn the_report_round_trips_through_json() {
        let mut off = listed("tampa", 3);
        off.enabled = false;
        let json = serde_json::to_value(SourceReport::List { sources: vec![off] }).unwrap();
        let back: SourceReport = serde_json::from_value(json).unwrap();
        match back {
            SourceReport::List { sources } => {
                assert_eq!(sources[0].id, "tampa");
                assert!(!sources[0].enabled);
            }
            _ => panic!("wrong variant"),
        }
    }

    async fn empty_store(dir: &std::path::Path) -> Ctx {
        Ctx::new(crate::store::Store::open(dir.join("store")).await.unwrap())
    }

    async fn add_site(ctx: &Ctx, id: &str, site: Option<&str>, strategy: Option<&str>) -> anyhow::Result<SourceReport> {
        add(
            ctx,
            AddArgs {
                id: id.into(),
                site: site.map(str::to_string),
                strategy: strategy.map(str::to_string),
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
    }

    /// `add` defines the source and stops unless told otherwise. The whole-pipeline
    /// version is one flag away, and that asymmetry is deliberate.
    #[tokio::test]
    async fn adding_defines_the_source_and_does_not_collect() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = empty_store(dir.path()).await;

        match add_site(&ctx, "tampa", Some("https://www.tampa.gov"), None).await.unwrap() {
            SourceReport::Add { source, run, .. } => {
                assert_eq!(source, "tampa");
                assert!(run.is_none(), "add must not collect without --run");
            }
            _ => panic!("wrong variant"),
        }

        let defined = Sources::new(&ctx.store).get("tampa").await.unwrap().unwrap();
        assert_eq!(defined.site.as_deref(), Some("https://www.tampa.gov"));
        assert_eq!(ctx.store.statuses(&SourceId::new("tampa").unwrap()).await.unwrap().len(), 0);
    }

    /// The key had no writer: `investigate` is documented as writing nothing and `source
    /// add` had no flag, so pinning a strategy meant editing a file by hand.
    #[tokio::test]
    async fn a_strategy_an_operator_accepted_can_be_pinned_when_the_source_is_added() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = empty_store(dir.path()).await;
        add_site(&ctx, "hillsclerk", Some("https://publicrec.hillsclerk.com/Civil/"), Some("listing"))
            .await
            .unwrap();
        let defined = Sources::new(&ctx.store).get("hillsclerk").await.unwrap().unwrap();
        assert_eq!(defined.strategy.as_deref(), Some("listing"));
    }

    /// Checked before anything is written. A name nothing answers to would otherwise sit
    /// in the definition and fail on the first run, where the error names an address.
    #[tokio::test]
    async fn a_strategy_nothing_answers_to_is_refused_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = empty_store(dir.path()).await;
        let err = add_site(&ctx, "tampa", Some("https://www.tampa.gov"), Some("onbase"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("onbase"), "{err}");
        assert!(Sources::new(&ctx.store).get("tampa").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn adding_a_source_with_neither_target_is_refused_before_it_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = empty_store(dir.path()).await;
        let err = add_site(&ctx, "tampa", None, None).await.unwrap_err();
        assert!(err.to_string().contains("neither"), "{err}");
        assert!(Sources::new(&ctx.store).list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn listing_reports_a_defined_source_the_store_has_never_collected() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = empty_store(dir.path()).await;
        add_site(&ctx, "tampa", Some("https://www.tampa.gov"), None).await.unwrap();

        match list(&ctx).await.unwrap() {
            SourceReport::List { sources } => {
                assert_eq!(sources.len(), 1);
                assert_eq!(sources[0].resources, 0);
                assert!(sources[0].enabled);
            }
            _ => panic!("wrong variant"),
        }
    }

    /// A store with one source that was collected before definitions existed.
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

    /// Adding by id alone, for the "I already crawled it, now keep it" case.
    #[tokio::test]
    async fn adding_without_a_target_takes_it_from_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = store_with_a_crawled_site(dir.path()).await;
        add_site(&ctx, "hillsborough", None, None).await.unwrap();
        let defined = Sources::new(&ctx.store).get("hillsborough").await.unwrap().unwrap();
        assert_eq!(defined.site.as_deref(), Some("https://www.hillsboroughcounty.org"));
    }

    /// The gap the old config left: a source collected by hand was in the store and not in
    /// the list. Importing at start closes it, so listing shows it with its address.
    #[tokio::test]
    async fn a_source_collected_by_hand_is_listed_after_import() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = store_with_a_crawled_site(dir.path()).await;
        Sources::new(&ctx.store).import(&crate::config::Config::default()).await.unwrap();

        match list(&ctx).await.unwrap() {
            SourceReport::List { sources } => {
                assert_eq!(sources.len(), 1, "{sources:?}");
                assert_eq!(sources[0].target, "https://www.hillsboroughcounty.org");
            }
            _ => panic!("wrong variant"),
        }
    }

    /// The count comes out of a summary, and `centinel list` replays the log. The two
    /// must be the same number at every moment — after a discovery, after a collect that
    /// re-observes an address, and after the log grows under a summary already written.
    #[tokio::test]
    async fn listing_counts_what_centinel_list_counts() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = store_with_a_crawled_site(dir.path()).await;
        Sources::new(&ctx.store).import(&crate::config::Config::default()).await.unwrap();
        let id = SourceId::new("hillsborough").unwrap();

        async fn counts(ctx: &Ctx) -> (usize, usize) {
            let listed = match list(ctx).await.unwrap() {
                SourceReport::List { sources } => sources[0].resources,
                _ => panic!("wrong variant"),
            };
            let replayed = crate::ops::list(ctx, crate::ops::ListArgs::default())
                .await
                .unwrap()
                .sources[0]
                .resources;
            (listed, replayed)
        }
        assert_eq!(counts(&ctx).await, (1, 1));

        // One address observed twice and one new one: two addresses, not three.
        for address in [
            "https://www.hillsboroughcounty.org/en/residents",
            "https://www.hillsboroughcounty.org/en/residents",
            "https://www.hillsboroughcounty.org/en/budget",
        ] {
            ctx.store
                .record_observation(
                    &Resource::new(id.clone(), address),
                    address.as_bytes(),
                    jiff::Timestamp::now(),
                    Default::default(),
                )
                .await
                .unwrap();
        }
        assert_eq!(counts(&ctx).await, (2, 2));
    }

    #[tokio::test]
    async fn disabling_leaves_a_source_out_of_a_bare_run_and_enabling_brings_it_back() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = empty_store(dir.path()).await;
        add_site(&ctx, "tampa", Some("https://www.tampa.gov"), None).await.unwrap();

        toggle(&ctx, "tampa", false).await.unwrap();
        assert!(Sources::new(&ctx.store).selected(&[]).await.unwrap().is_empty());
        toggle(&ctx, "tampa", true).await.unwrap();
        assert_eq!(Sources::new(&ctx.store).selected(&[]).await.unwrap().len(), 1);
    }
}
