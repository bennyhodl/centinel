//! `collect` — acquire everything a [`DiscoveryRun`] found.
//!
//! This is what turns a list of addresses into a corpus. What "acquire one" means is the
//! Source's business — an HTTP GET for a page, three `yt-dlp` calls for a video — and
//! nothing in this file knows which it got.
//!
//! ## Resumability is not a feature here, it is a consequence
//!
//! At a polite rate a city is an hour of work, so interruption is normal rather than
//! exceptional. But there is no checkpoint file: the log already records every
//! Observation, so "what still needs collecting" falls out of it. Kill it at address
//! 4,000 and re-run; it starts at 4,001. See [`crate::acquire`], which owns that loop for
//! every Source kind.
//!
//! [`DiscoveryRun`]: crate::domain::DiscoveryRun

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::acquire::{self, CollectOpts};
use crate::prelude::*;
use crate::sources::{AudioPolicy, Overrides};

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct CollectArgs {
    /// Source to collect, as used by `discover`.
    #[arg(long)]
    pub source: String,

    /// Stop after this many addresses. The way to try a source before committing an hour.
    #[arg(long)]
    #[serde(default)]
    pub limit: Option<usize>,

    /// Requests per second, per host. Omit to inherit the config. Website sources only.
    #[arg(long)]
    #[serde(default)]
    pub rps: Option<f64>,

    /// Re-acquire addresses already in the store instead of skipping them.
    #[arg(long)]
    #[serde(default)]
    pub refresh: bool,

    /// Only collect addresses containing this substring. Repeatable.
    ///
    /// Deliberately a substring rather than a regex: this is a coarse "just the PDFs"
    /// filter for exploration, not the scope policy that ticket #4 owns.
    #[arg(long = "match")]
    #[serde(default)]
    pub matches: Vec<String>,

    /// Skip artifacts larger than this many megabytes.
    #[arg(long, default_value_t = 256)]
    #[serde(default = "default_max_mb")]
    pub max_mb: u64,

    /// Failures to include in the report.
    #[arg(long, default_value_t = 20)]
    #[serde(default = "default_max_failures")]
    pub max_failures: usize,

    /// Caption language. Channel sources only; omit to inherit the config.
    #[arg(long)]
    #[serde(default)]
    pub lang: Option<String>,

    /// Download audio for every video. Channel sources only. ~63 MB per 3-hour meeting.
    #[arg(long, conflicts_with_all = ["audio_if_no_captions", "no_audio"])]
    #[serde(default)]
    pub audio: bool,

    /// Download audio only for videos with no caption track. Channel sources only.
    ///
    /// **The default.** Measured at ~7% of a real council channel: ordinary public
    /// meetings YouTube simply never ran ASR on, indistinguishable from the rest and
    /// permanently missing from search without this.
    #[arg(long, conflicts_with = "no_audio")]
    #[serde(default)]
    pub audio_if_no_captions: bool,

    /// Never download audio. Channel sources only.
    #[arg(long)]
    #[serde(default)]
    pub no_audio: bool,

    /// Extra arguments for yt-dlp. Channel sources only.
    #[arg(long = "yt-dlp-arg", allow_hyphen_values = true)]
    #[serde(default)]
    pub yt_dlp_args: Vec<String>,

    /// Config file. Defaults to the usual search path.
    #[arg(long, value_name = "FILE")]
    #[serde(default)]
    pub config: Option<String>,
}

fn default_max_mb() -> u64 {
    256
}
fn default_max_failures() -> usize {
    20
}

/// So [`crate::ops::run`] inherits the CLI's limits instead of restating them.
impl Default for CollectArgs {
    fn default() -> Self {
        Self {
            source: String::new(),
            limit: None,
            rps: None,
            refresh: false,
            matches: Vec::new(),
            max_mb: default_max_mb(),
            max_failures: default_max_failures(),
            lang: None,
            audio: false,
            audio_if_no_captions: false,
            no_audio: false,
            yt_dlp_args: Vec::new(),
            config: None,
        }
    }
}

impl CollectArgs {
    /// The audio policy these flags ask for, or `None` to leave it to the config.
    fn audio_policy(&self) -> Option<AudioPolicy> {
        match (self.audio, self.audio_if_no_captions, self.no_audio) {
            (true, _, _) => Some(AudioPolicy::Always),
            (_, true, _) => Some(AudioPolicy::IfNoCaptions),
            (_, _, true) => Some(AudioPolicy::Never),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CollectFailure {
    /// The address that failed.
    pub natural_key: String,
    pub state: Liveness,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CollectReport {
    pub source: String,
    pub kind: SourceKind,
    /// Addresses in the most recent DiscoveryRun.
    pub discovered: usize,
    /// Skipped because the store already had them.
    pub already_had: usize,
    /// Excluded by `--match`.
    pub filtered_out: usize,
    pub attempted: usize,
    pub stored: usize,
    /// Stored, and something under that address differed from the previous Observation.
    pub changed: usize,
    pub failed: usize,
    /// Failures that were refusals rather than absence. A non-zero count with zero
    /// successes is a wall, and reads exactly like an empty source unless it is named.
    pub blocked: usize,
    pub bytes: u64,
    /// Still uncollected. Non-zero means re-running continues where this stopped.
    pub remaining: usize,
    /// What was gathered, by content kind — the input to planning extraction.
    pub by_kind: BTreeMap<String, usize>,
    /// What was gathered, by artifact. One address can hold several: a video's metadata,
    /// captions and audio are three addresses with three histories (§4.2).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parts: BTreeMap<String, usize>,
    /// What the Source itself wanted said about the result.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    pub failures: Vec<CollectFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failures_truncated: Option<usize>,
}

/// Acquire every address the latest discovery run found, skipping what is already stored.
#[op(long_running, reach = "operator", group = "stage")]
pub async fn collect(
    ctx: &Ctx,
    args: CollectArgs,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<CollectReport> {
    let (config, _) = super::load_config(args.config.as_deref())?;

    let over = Overrides {
        rps: args.rps,
        lang: args.lang.clone(),
        audio: args.audio_policy(),
        yt_dlp_args: args.yt_dlp_args.clone(),
        ..Default::default()
    };

    // No `--site`/`--channel` here: collection works from the addresses the DiscoveryRun
    // already holds, so the address a source was enumerated from is not needed again.
    let source = super::resolve_source(ctx, &config, &args.source, None, None, &over).await?;

    let out = acquire::collect(
        &ctx.store,
        source.as_ref(),
        &CollectOpts {
            limit: args.limit,
            refresh: args.refresh,
            matches: args.matches.clone(),
            max_bytes: args.max_mb.saturating_mul(1024 * 1024),
            max_failures: args.max_failures,
            cancel: cancel.clone(),
        },
        progress,
    )
    .await?;

    Ok(CollectReport {
        source: args.source,
        kind: source.kind(),
        discovered: out.discovered,
        already_had: out.already_had,
        filtered_out: out.filtered_out,
        attempted: out.attempted,
        stored: out.stored,
        changed: out.changed,
        failed: out.failed,
        blocked: out.blocked,
        bytes: out.bytes,
        remaining: out.remaining,
        by_kind: out.by_kind,
        parts: out.parts,
        notes: out.remarks,
        failures: out
            .failures
            .into_iter()
            .map(|f| CollectFailure {
                natural_key: f.natural_key,
                state: f.state,
                detail: f.detail,
            })
            .collect(),
        failures_truncated: out.failures_truncated,
    })
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// What the run did, what it left, and what refused it.
///
/// `remaining` is promoted out of the counter column into a line of its own, because it is
/// the only figure here that is an *instruction*: non-zero means running the same command
/// again continues from where this stopped, and a person who misses that re-crawls from
/// the beginning.
impl Render for CollectReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.title(&self.source, &render::bytes(self.bytes))?;
        p.nest(|p| {
            p.figures(&[
                (self.discovered as u64, "discovered"),
                (self.already_had as u64, "already had"),
                (self.filtered_out as u64, "filtered out"),
                (self.attempted as u64, "attempted"),
                (self.stored as u64, "stored"),
                (self.changed as u64, "changed"),
                (self.failed as u64, "failed"),
            ])?;

            // Only when an address held more than one thing. For a crawled site this is a
            // single row saying "document", which is a row nobody needs.
            if self.parts.len() > 1 {
                p.section("by artifact")?;
                let mut table = Table::bare(&[Align::Right, Align::Left]);
                for (part, n) in &self.parts {
                    table.push(vec![
                        Cell::new(render::count(*n as u64), Ink::Bold),
                        Cell::dim(part),
                    ]);
                }
                p.table(&table)?;
            }

            if !self.by_kind.is_empty() {
                p.section("by kind")?;
                let mut table = Table::bare(&[Align::Right, Align::Left]);
                for (kind, n) in &self.by_kind {
                    table.push(vec![
                        Cell::new(render::count(*n as u64), Ink::Bold),
                        Cell::dim(kind),
                    ]);
                }
                p.table(&table)?;
            }

            // A wall of refusals with nothing stored looks exactly like an empty source
            // unless something says so out loud.
            if self.blocked > 0 {
                p.blank()?;
                let text = if self.stored == 0 {
                    format!(
                        "{} refused and nothing stored — this is a block, not an empty source",
                        render::count(self.blocked as u64)
                    )
                } else {
                    format!("{} refused", render::count(self.blocked as u64))
                };
                p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
            }

            if !self.notes.is_empty() {
                p.blank()?;
                super::discover::render_notes(p, &self.notes)?;
            }

            if !self.failures.is_empty() {
                p.section("failures")?;
                for failure in &self.failures {
                    failure.render(p)?;
                }
                if let Some(more) = self.failures_truncated {
                    let text = format!("… and {} more", render::count(more as u64));
                    p.line(p.paint(&text, Ink::Dim))?;
                }
            }

            if self.remaining > 0 {
                p.blank()?;
                let text = format!(
                    "{} still uncollected — re-run to continue",
                    render::count(self.remaining as u64)
                );
                p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
            }
            Ok(())
        })
    }
}

impl Render for CollectFailure {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let mark = self.state.mark();
        let state = format!("{:<8}", self.state);
        let head = format!(
            "{}{}",
            p.paint(&state, mark.ink()),
            render::truncate(&self.natural_key, p.width().saturating_sub(12)),
        );
        p.marked(mark, head)?;
        p.nest(|p| p.wrapped(&render::one_line(&self.detail), Ink::Dim))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_to_string(report: &CollectReport) -> String {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    fn report() -> CollectReport {
        CollectReport {
            source: "tampa".into(),
            kind: SourceKind::Site,
            discovered: 100,
            already_had: 0,
            filtered_out: 0,
            attempted: 100,
            stored: 100,
            changed: 12,
            failed: 0,
            blocked: 0,
            bytes: 4096,
            remaining: 0,
            by_kind: BTreeMap::from([("pdf".into(), 40), ("html".into(), 60)]),
            parts: BTreeMap::from([("document".into(), 100)]),
            notes: Vec::new(),
            failures: Vec::new(),
            failures_truncated: None,
        }
    }

    /// The only figure here that is an instruction.
    #[test]
    fn remaining_work_says_what_to_do_about_it() {
        let mut r = report();
        r.stored = 40;
        r.remaining = 60;
        assert!(render_to_string(&r).contains("re-run to continue"));
    }

    /// The bot wall and an empty channel produce identical counters, so the difference
    /// has to be stated.
    #[test]
    fn a_wholesale_block_does_not_read_as_an_empty_source() {
        let mut r = report();
        r.kind = SourceKind::Channel;
        r.stored = 0;
        r.failed = 100;
        r.blocked = 100;
        let out = render_to_string(&r);
        assert!(out.contains("this is a block"), "{out}");
    }

    /// One row saying "document" is a row nobody needs; three rows are the shape of a
    /// video and worth the space.
    #[test]
    fn the_artifact_table_appears_only_when_an_address_held_several() {
        assert!(!render_to_string(&report()).contains("by artifact"));

        let mut r = report();
        r.parts = BTreeMap::from([
            ("metadata".into(), 42),
            ("captions.json3".into(), 39),
            ("audio".into(), 3),
        ]);
        let out = render_to_string(&r);
        assert!(out.contains("by artifact"), "{out}");
        assert!(out.contains("captions.json3"), "{out}");
    }

    /// The Source gets the last word, and the renderer prints it without understanding it.
    #[test]
    fn a_sources_own_remarks_are_shown() {
        let mut r = report();
        r.notes = vec![Note::marked(
            "no captions",
            "3 without captions — audio was fetched for transcription",
            NoteMark::Warn,
        )];
        let out = render_to_string(&r);
        assert!(out.contains("no captions"), "{out}");
        assert!(out.contains("transcription"), "{out}");
    }

    #[test]
    fn failures_are_bounded_and_say_how_many_were_dropped() {
        let mut r = report();
        r.failures = vec![CollectFailure {
            natural_key: "https://x.gov/a".into(),
            state: Liveness::Blocked,
            detail: "HTTP 403".into(),
        }];
        r.failures_truncated = Some(412);
        let out = render_to_string(&r);
        assert!(out.contains("403"), "{out}");
        assert!(out.contains("412 more"), "{out}");
    }

    #[test]
    fn the_report_round_trips_through_json() {
        let mut r = report();
        r.notes = vec![Note::new("x", "y")];
        let json = serde_json::to_value(&r).unwrap();
        let back: CollectReport = serde_json::from_value(json).unwrap();
        assert_eq!(back.stored, 100);
        assert_eq!(back.by_kind["pdf"], 40);
        assert_eq!(back.notes[0].label, "x");
    }

    // ── flags ──────────────────────────────────────────────────────────────────

    /// No flag means "leave it to the config", which is what makes the config the
    /// standing answer rather than something a bare re-run silently overrides.
    #[test]
    fn audio_flags_resolve_to_a_policy_or_defer() {
        assert_eq!(CollectArgs::default().audio_policy(), None);
        assert_eq!(
            CollectArgs {
                audio: true,
                ..Default::default()
            }
            .audio_policy(),
            Some(AudioPolicy::Always)
        );
        assert_eq!(
            CollectArgs {
                audio_if_no_captions: true,
                ..Default::default()
            }
            .audio_policy(),
            Some(AudioPolicy::IfNoCaptions)
        );
        assert_eq!(
            CollectArgs {
                no_audio: true,
                ..Default::default()
            }
            .audio_policy(),
            Some(AudioPolicy::Never)
        );
    }
}
