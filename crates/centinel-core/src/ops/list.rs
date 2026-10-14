//! `list` — what is in the store, and what state it is in.
//!
//! Reads only truth (`log/`), never a derived index. That is deliberate: it means this
//! op still works with `centinel.db` and `index/` deleted, which is the property SPEC
//! §5 claims and this is the cheapest place to keep honest.
//!
//! ## Every source, not every source with something in it
//!
//! A source with nothing collected gets a row saying so. `source add` writes the
//! definition into `log/<id>/`, so a source nothing has run yet still has a directory and
//! still has a row — the command an operator types straight after adding one must not
//! answer *"No sources yet"*.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::prelude::*;
use crate::summary::Summary;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SourceSummary {
    pub source: String,
    /// Whether the store defines it (`log/<id>/source.toml`). False only for a log nothing
    /// can say the address of, which nothing can collect again.
    pub defined: bool,
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
    /// The latest enumeration of the source, if one ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_discovery: Option<Discovered>,
}

/// One `DiscoveryRun`, summarised: when, by which method, and how many addresses it saw.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Discovered {
    pub at: String,
    pub method: String,
    pub addresses: usize,
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
}

fn default_max_problems() -> usize {
    20
}

/// List every source in the store, with resource counts and liveness.
#[op(group = "corpus")]
pub async fn list(ctx: &Ctx, args: ListArgs) -> anyhow::Result<ListReport> {
    let catalog = crate::catalog::Sources::new(&ctx.store);
    let sources = match &args.source {
        Some(s) => vec![SourceId::new(s.clone())?],
        None => ctx.store.sources().await?,
    };

    let mut out = Vec::with_capacity(sources.len());
    for source in sources {
        let defined = catalog.get(source.as_str()).await?.is_some();
        let replay = ctx.store.replay(&source).await?;
        // The counts are the summary's own fold ([`Summary::of`]), so this and the cached
        // `source list` can never count differently; unstamped, because nothing is written.
        let summary = Summary::of(&replay, BTreeMap::new());
        let last_discovery = replay.records().iter().rev().find_map(|r| match r {
            LogRecord::DiscoveryRun(d) => Some(Discovered {
                at: d.at.to_string(),
                method: d.method.clone(),
                addresses: d.resources.len(),
            }),
            _ => None,
        });

        let mut problems = Vec::new();
        for status in replay.statuses().values() {
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
            defined,
            resources: summary.resources,
            observations: summary.observations,
            liveness: summary.liveness,
            problems,
            problems_truncated: (truncated > 0).then_some(truncated),
            last_discovery,
        });
    }

    Ok(ListReport {
        store_root: ctx.store.root().display().to_string(),
        sources: out,
    })
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
        // freshly added store is all empty rows, and a command repeated eight times is
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
        let counts = match (self.holds_nothing(), self.defined) {
            (false, _) => format!(
                "{} · {}",
                render::plural(self.resources, "resource", "resources"),
                render::plural(self.observations, "observation", "observations"),
            ),
            (true, true) => "defined, nothing collected yet".to_string(),
            // No definition and no records either: an empty directory under `log/`,
            // which is what an interrupted first run leaves behind.
            (true, false) => "nothing collected yet, and no definition".to_string(),
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

    fn args() -> ListArgs {
        ListArgs {
            max_problems: default_max_problems(),
            ..Default::default()
        }
    }

    /// A store that has been collecting one source, from before definitions existed.
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

    /// The state a `source add` leaves behind: defined, never run. It must still have a
    /// row — listing it as absent reads as an add that did not work.
    #[tokio::test]
    async fn a_defined_source_the_store_has_never_collected_is_still_listed() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().join("store"))
            .await
            .unwrap();
        crate::catalog::Sources::new(&store)
            .add(&crate::config::SourceConfig::site(
                "tampa",
                "https://www.tampa.gov",
            ))
            .await
            .unwrap();

        let report = list(&Ctx::new(store), args()).await.unwrap();

        assert_eq!(report.sources.len(), 1, "{:?}", report.sources);
        assert_eq!(report.sources[0].source, "tampa");
        assert_eq!(report.sources[0].resources, 0);
        assert!(report.sources[0].defined);
    }

    /// A log with no definition is still listed, and marked, until `import` gives it one.
    #[tokio::test]
    async fn a_log_without_a_definition_is_listed_and_marked() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = store_holding(dir.path(), "hillsborough").await;

        let report = list(&ctx, args()).await.unwrap();
        assert_eq!(report.sources.len(), 1);
        assert!(!report.sources[0].defined);
        assert_eq!(report.sources[0].resources, 1);

        crate::catalog::Sources::new(&ctx.store)
            .import(&crate::config::Config::default())
            .await
            .unwrap();
        let after = list(&ctx, args()).await.unwrap();
        assert!(after.sources[0].defined);
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
            defined: true,
            resources,
            observations: resources,
            liveness: match resources {
                0 => BTreeMap::new(),
                n => BTreeMap::from([("live".to_string(), n)]),
            },
            problems: Vec::new(),
            problems_truncated: None,
            last_discovery: None,
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

    /// One command, once, at the bottom. A freshly added store is all empty rows, and
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

    /// An empty directory with no definition, which the aside is the only thing that
    /// reports here.
    #[test]
    fn an_empty_source_with_no_definition_says_that_too() {
        let loose = SourceSummary {
            defined: false,
            ..summary("hillsborough", 0)
        };
        let out = render_to_string(&report(vec![loose]));
        assert!(out.contains("no definition"), "{out}");
    }
}
