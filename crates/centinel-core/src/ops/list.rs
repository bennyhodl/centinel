//! `list` — what is in the store, and what state it is in.
//!
//! Reads only truth (`log/`), never a derived index. That is deliberate: it means this
//! op still works with `centinel.db` and `index/` deleted, which is the property SPEC
//! §5 claims and this is the cheapest place to keep honest.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::prelude::*;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SourceSummary {
    pub source: String,
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

/// List sources in the store with resource counts and liveness.
#[op(group = "corpus")]
pub async fn list(ctx: &Ctx, args: ListArgs) -> anyhow::Result<ListReport> {
    let sources = match &args.source {
        Some(s) => vec![SourceId::new(s.clone())?],
        None => ctx.store.sources().await?,
    };

    let mut out = Vec::with_capacity(sources.len());
    for source in sources {
        let replay = ctx.store.replay(&source).await?;
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
            resources: statuses.len(),
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
            return p.note("centinel discover --source <name> --site <url>");
        }

        for (i, source) in self.sources.iter().enumerate() {
            if i > 0 {
                p.blank()?;
            }
            source.render(p)?;
        }
        Ok(())
    }
}

impl Render for SourceSummary {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let counts = format!(
            "{} · {}",
            render::plural(self.resources, "resource", "resources"),
            render::plural(self.observations, "observation", "observations"),
        );
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
