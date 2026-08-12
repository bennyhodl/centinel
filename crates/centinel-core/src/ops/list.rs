//! `list` — what is in the store, and what state it is in.
//!
//! Reads only truth (`log/`), never a derived index. That is deliberate: it means this
//! op still works with `centinel.db` and `index/` deleted, which is the property SPEC
//! §5 claims and this is the cheapest place to keep honest.
//!
//! ## Every source, not every source with something in it
//!
//! The rows are the config's sources first, then whatever else the store holds — and a
//! source with nothing collected gets a row saying so. It used to list the store alone,
//! which is a directory per source under `log/`, and a source nothing has run yet has no
//! directory: so the command an operator types straight after `source add` answered *"No
//! sources yet"*, about a config file naming five of them. Told that, the reasonable
//! reading is that the add did not work.
//!
//! The union is also what makes the two failures distinguishable. *Configured and
//! uncollected* is a run waiting to happen; *collected and unconfigured* is a source
//! `centinel run` will silently skip. Listing one side only cannot say either.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SourceSummary {
    pub source: String,
    /// Whether `centinel.toml` names it. False means the store has been collecting it and
    /// a bare `centinel run` will not — the config is the statement of intent, and this
    /// source is not in it. `centinel source list` is where that gap is acted on.
    pub tracked: bool,
    /// Addresses the log knows about. Zero for a source that has been added and not yet
    /// run, which is a state with a row rather than a state with no row.
    pub resources: usize,
    pub observations: usize,
    /// Liveness counts, keyed by `live` / `gone` / `blocked` / `error`.
    pub liveness: BTreeMap<String, usize>,
    /// Addresses not currently `Live`. Capped by `--max-problems`, because a source
    /// that WAF-blocked wholesale would otherwise dump thousands of identical lines.
    pub problems: Vec<Problem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problems_truncated: Option<usize>,
}

impl SourceSummary {
    /// Whether the log has anything to say about this source at all.
    ///
    /// Resources rather than observations: a source that was discovered and not collected
    /// has addresses and no bytes, which is a different — and reportable — state from a
    /// source nothing has ever run against.
    pub fn holds_nothing(&self) -> bool {
        self.resources == 0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Problem {
    pub natural_key: String,
    pub state: Liveness,
    pub since: String,
    pub consecutive_failures: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListReport {
    pub store_root: String,
    pub sources: Vec<SourceSummary>,
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct ListArgs {
    /// Limit to one source. Omit to list all.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,

    /// Maximum non-live addresses to enumerate per source.
    #[arg(long, default_value_t = 20)]
    #[serde(default = "default_max_problems")]
    pub max_problems: usize,

    /// Config file the source list is read from. Defaults to the one in effect.
    #[arg(long, value_name = "FILE")]
    #[serde(default)]
    pub config: Option<String>,
}

fn default_max_problems() -> usize {
    20
}

/// List every source — configured or collected — with resource counts and liveness.
#[op(group = "corpus")]
pub async fn list(ctx: &Ctx, args: ListArgs) -> anyhow::Result<ListReport> {
    let (config, _) = super::load_config(args.config.as_deref())?;

    let sources = match &args.source {
        Some(s) => vec![SourceId::new(s.clone())?],
        None => named(ctx, &config).await?,
    };

    let mut out = Vec::with_capacity(sources.len());
    for source in sources {
        let tracked = config.source(source.as_str()).is_some();
        let replay = ctx.store.replay(&source).await?;
        // Statuses alone under-count: an address enumerated and never fetched is in a
        // DiscoveryRun and nowhere else, and it is exactly the "discovered, not
        // collected" state the resources column exists to show.
        let resources = replay.resources().len();
        let statuses = replay.statuses();
        let observations = replay
            .records()
            .iter()
            .filter(|r| matches!(r, LogRecord::Observation(_)))
            .count();

        let mut liveness: BTreeMap<String, usize> = BTreeMap::new();
        let mut problems = Vec::new();
        for status in statuses.values() {
            *liveness.entry(status.state.to_string()).or_default() += 1;
            if status.state != Liveness::Live {
                problems.push(Problem {
                    natural_key: status.resource.natural_key.clone(),
                    state: status.state,
                    since: status.since.to_string(),
                    consecutive_failures: status.consecutive_failures,
                    detail: status.detail.clone(),
                });
            }
        }

        // Longest-standing failures first — a page blocked for a month matters more
        // than one that failed in this run.
        problems.sort_by(|a, b| {
            b.consecutive_failures
                .cmp(&a.consecutive_failures)
                .then(a.natural_key.cmp(&b.natural_key))
        });
        let total_problems = problems.len();
        let truncated = total_problems.saturating_sub(args.max_problems);
        problems.truncate(args.max_problems);

        out.push(SourceSummary {
            source: source.to_string(),
            tracked,
            resources,
            observations,
            liveness,
            problems,
            problems_truncated: (truncated > 0).then_some(truncated),
        });
    }

    Ok(ListReport {
        store_root: ctx.store.root().display().to_string(),
        sources: out,
    })
}

/// Every source there is: the config's, in the order they are written, then the store's.
///
/// Config order rather than alphabetical, because that file is a thing a person wrote and
/// the order they wrote it in is information. The store's own list is sorted, and its
/// entries are appended rather than merged in, so the two halves of the answer — declared,
/// and merely present — stay visibly apart.
///
/// A config id the store could never hold is skipped: `SourceId` refuses what is not a
/// legal directory name, and there is no log to read for a name no log can be written
/// under. `run` is where that block earns its error, naming the id and the rule.
async fn named(ctx: &Ctx, config: &Config) -> anyhow::Result<Vec<SourceId>> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(config.sources.len());

    for source in &config.sources {
        if let Ok(id) = SourceId::new(source.id.clone())
            && seen.insert(id.clone())
        {
            out.push(id);
        }
    }
    for id in ctx.store.sources().await? {
        if seen.insert(id.clone()) {
            out.push(id);
        }
    }
    Ok(out)
}

/// One block per source: the counts, the liveness roll-up, then the addresses that are
/// not `Live`.
///
/// `store_root` is not printed. An HTTP caller needs to be told which store answered; a
/// person typed `--root` or accepted the default two seconds ago, and repeating it back
/// is the kind of line that trains people to stop reading output.
impl Render for ListReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        if self.sources.is_empty() {
            p.line(p.paint("No sources yet.", Ink::Dim))?;
            return p.note("centinel source add <name> --site <url>");
        }

        for (i, source) in self.sources.iter().enumerate() {
            if i > 0 {
                p.blank()?;
            }
            source.render(p)?;
        }

        // The rows above say *what is here*; this says what to do about the ones that are
        // here and empty. Printed once, at the bottom, rather than under every row: a
        // freshly written config is all empty rows, and a command repeated eight times is
        // a wall rather than an instruction.
        let empty: Vec<&SourceSummary> =
            self.sources.iter().filter(|s| s.holds_nothing()).collect();
        let Some(first) = empty.first() else {
            return Ok(());
        };
        p.blank()?;
        p.line(p.paint(
            &format!(
                "{} nothing yet.",
                render::plural(empty.len(), "source holds", "sources hold"),
            ),
            Ink::Dim,
        ))?;
        p.note(match empty.len() {
            1 => format!("centinel run --source {}", first.source),
            _ => "centinel run".to_string(),
        })
    }
}

impl Render for SourceSummary {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        // The aside is the answer to "what state is this in", so for an empty source it
        // has to say the state rather than count it. `0 resources · 0 observations` is
        // two facts a reader has to add up into the one word that follows.
        let counts = match (self.holds_nothing(), self.tracked) {
            (false, _) => format!(
                "{} · {}",
                render::plural(self.resources, "resource", "resources"),
                render::plural(self.observations, "observation", "observations"),
            ),
            (true, true) => "in centinel.toml, nothing collected yet".to_string(),
            // No config block and no records either: an empty directory under `log/`,
            // which is what an interrupted first run leaves behind.
            (true, false) => "nothing collected yet, and not in centinel.toml".to_string(),
        };
        p.title(&self.source, &counts)?;

        p.nest(|p| {
            // Live first, then the rest — the reading order is "is this healthy, and if
            // not, how much of it isn't".
            let mut states: Vec<(&String, &usize)> = self.liveness.iter().collect();
            states.sort_by_key(|(name, _)| (name.as_str() != "live", name.as_str()));

            let roll_up: Vec<String> = states
                .iter()
                .map(|(name, n)| {
                    format!(
                        "{} {} {}",
                        liveness_mark(name).painted(p),
                        p.paint(&render::count(**n as u64), Ink::Bold),
                        p.paint(name, Ink::Dim),
                    )
                })
                .collect();
            if !roll_up.is_empty() {
                p.line(roll_up.join("   "))?;
            }

            if self.problems.is_empty() {
                return Ok(());
            }
            p.blank()?;
            for problem in &self.problems {
                problem.render(p)?;
            }
            if let Some(more) = self.problems_truncated {
                let text = format!(
                    "… and {} more, raise --max-problems to see them",
                    render::count(more as u64)
                );
                p.line(p.paint(&text, Ink::Dim))?;
            }
            Ok(())
        })
    }
}

impl Render for Problem {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let mark = self.state.mark();
        let state = format!("{:<8}", self.state);
        let head = format!(
            "{}{}",
            p.paint(&state, mark.ink()),
            render::truncate(&self.natural_key, p.width().saturating_sub(12)),
        );
        p.marked(mark, head)?;

        // Consecutive failures and `since` are the difference between "this broke in the
        // run you just watched" and "this has been broken for a month", which is the only
        // thing that decides whether it is worth your afternoon.
        let mut aside = format!(
            "{} since {}",
            render::plural(self.consecutive_failures as usize, "failure", "failures"),
            render::short_time(&self.since),
        );
        if let Some(detail) = &self.detail {
            aside.push_str(" · ");
            aside.push_str(&render::one_line(detail));
        }
        p.nest(|p| p.wrapped(&aside, Ink::Dim))
    }
}

/// The glyph for a liveness *name*, for the roll-up where the state arrives as a map key
/// rather than as a [`Liveness`].
fn liveness_mark(name: &str) -> Mark {
    match name {
        "live" => Mark::Ok,
        "blocked" => Mark::Warn,
        _ => Mark::Bad,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(config: &std::path::Path) -> ListArgs {
        ListArgs {
            config: Some(config.display().to_string()),
            max_problems: default_max_problems(),
            ..Default::default()
        }
    }

    /// A store that has been collecting one source, which nothing ever configured.
    async fn store_holding(dir: &std::path::Path, id: &str) -> Ctx {
        let store = crate::store::Store::open(dir.join("store")).await.unwrap();
        let source = SourceId::new(id).unwrap();
        store
            .append(
                &source,
                &LogRecord::DiscoveryRun(crate::domain::DiscoveryRun {
                    source: source.clone(),
                    at: jiff::Timestamp::now(),
                    resources: vec![Resource::new(source.clone(), "https://x.gov/a")],
                    method: "sitemap".into(),
                }),
            )
            .await
            .unwrap();
        Ctx::new(store)
    }

    /// The state a `source add` leaves behind, and the whole reason the config is read
    /// here: the store is a directory per source under `log/`, a source nothing has run
    /// has no directory, and listing the store alone answered *"No sources yet"* at
    /// somebody who had just added five.
    #[tokio::test]
    async fn a_configured_source_the_store_has_never_seen_is_still_listed() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("centinel.toml");
        std::fs::write(
            &config,
            "[[source]]\nid = \"tampa\"\nsite = \"https://www.tampa.gov\"\n",
        )
        .unwrap();
        let store = crate::store::Store::open(dir.path().join("store"))
            .await
            .unwrap();

        let report = list(&Ctx::new(store), args(&config)).await.unwrap();

        assert_eq!(report.sources.len(), 1, "{:?}", report.sources);
        assert_eq!(report.sources[0].source, "tampa");
        assert_eq!(report.sources[0].resources, 0);
        assert!(report.sources[0].tracked);
    }

    /// Both halves of the answer, and in that order: what was declared, then what is
    /// merely present. A source collected by hand is very much here.
    #[tokio::test]
    async fn the_config_leads_and_anything_else_the_store_holds_follows() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = store_holding(dir.path(), "hillsborough").await;
        let config = dir.path().join("centinel.toml");
        std::fs::write(
            &config,
            "[[source]]\nid = \"tampa\"\nsite = \"https://www.tampa.gov\"\n",
        )
        .unwrap();

        let report = list(&ctx, args(&config)).await.unwrap();

        let ids: Vec<&str> = report.sources.iter().map(|s| s.source.as_str()).collect();
        assert_eq!(ids, ["tampa", "hillsborough"], "{ids:?}");
        assert!(report.sources[0].tracked);
        assert!(report.sources[0].holds_nothing());
        assert!(!report.sources[1].tracked);
        assert_eq!(report.sources[1].resources, 1);
    }

    /// A source in both is one row. It was listed twice for one commit, because the
    /// config half and the store half were collected independently.
    #[tokio::test]
    async fn a_source_that_is_configured_and_collected_appears_once() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = store_holding(dir.path(), "tampa").await;
        let config = dir.path().join("centinel.toml");
        std::fs::write(
            &config,
            "[[source]]\nid = \"tampa\"\nsite = \"https://www.tampa.gov\"\n",
        )
        .unwrap();

        let report = list(&ctx, args(&config)).await.unwrap();
        assert_eq!(report.sources.len(), 1, "{:?}", report.sources);
        assert!(report.sources[0].tracked);
        assert_eq!(report.sources[0].resources, 1);
    }

    // ── rendering ─────────────────────────────────────────────────────────────────

    fn render_to_string(report: &ListReport) -> String {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    fn summary(id: &str, resources: usize) -> SourceSummary {
        SourceSummary {
            source: id.into(),
            tracked: true,
            resources,
            observations: resources,
            liveness: match resources {
                0 => BTreeMap::new(),
                n => BTreeMap::from([("live".to_string(), n)]),
            },
            problems: Vec::new(),
            problems_truncated: None,
        }
    }

    fn report(sources: Vec<SourceSummary>) -> ListReport {
        ListReport {
            store_root: "/tmp/store".into(),
            sources,
        }
    }

    /// `0 resources · 0 observations` is two facts a reader has to add up. The row has to
    /// say the state, and then say what turns it into a corpus.
    #[test]
    fn an_uncollected_source_says_so_and_names_the_command_that_collects_it() {
        let out = render_to_string(&report(vec![summary("tampa", 0)]));
        assert!(out.contains("tampa"), "{out}");
        assert!(out.contains("nothing collected yet"), "{out}");
        assert!(out.contains("centinel run --source tampa"), "{out}");
    }

    /// One command, once, at the bottom. A freshly written config is all empty rows, and
    /// eight copies of the same line is a wall rather than an instruction.
    #[test]
    fn several_uncollected_sources_get_one_line_between_them() {
        let out = render_to_string(&report(vec![summary("tampa", 0), summary("pinellas", 0)]));
        assert!(out.contains("2 sources hold nothing yet"), "{out}");
        assert!(!out.contains("--source tampa"), "{out}");
        assert_eq!(out.matches("centinel run").count(), 1, "{out}");
    }

    /// The footer is for a store with a hole in it. A collected corpus must not carry a
    /// standing instruction to go and collect it.
    #[test]
    fn a_collected_corpus_gets_no_footer() {
        let out = render_to_string(&report(vec![summary("tampa", 1847)]));
        assert!(out.contains("1,847 resources"), "{out}");
        assert!(!out.contains("centinel run"), "{out}");
    }

    /// The gap the other way round, which the aside is the only thing that reports here.
    #[test]
    fn an_empty_source_the_config_never_named_says_that_too() {
        let loose = SourceSummary {
            tracked: false,
            ..summary("hillsborough", 0)
        };
        let out = render_to_string(&report(vec![loose]));
        assert!(out.contains("not in centinel.toml"), "{out}");
    }
}
