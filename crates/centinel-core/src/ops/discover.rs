//! `discover` — turn a source into the full set of addresses it declares.
//!
//! This is the op that makes a corpus possible. Before it, collection meant typing URLs
//! by hand; after it, a `.gov` site's entire declared surface — or a council channel's
//! entire back catalogue — is one command.
//!
//! The output is a [`DiscoveryRun`]: a complete snapshot, not a delta. Snapshots are what
//! make a shrinking corpus visible — a run that finds 400 addresses where the last found
//! 12,000 means something broke, and that is a collection-quality signal worth having
//! even when nobody is asking questions about change.
//!
//! ## One verb, whatever the source is
//!
//! A sitemap walk and a playlist listing are the same act, and this op does not know
//! which one it asked for. [`crate::sources`] decides that from the `[[source]]` block;
//! everything below talks to [`Source::enumerate`] and renders whatever provenance came
//! back with it.
//!
//! [`Source::enumerate`]: crate::domain::Source::enumerate
//! [`DiscoveryRun`]: crate::domain::DiscoveryRun

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::acquire::{self, DiscoverOpts};
use crate::discovery::DiscoveryLimits;
use crate::prelude::*;
use crate::sources::Overrides;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct DiscoverArgs {
    /// Source id to file this run under, e.g. `tampa`.
    ///
    /// With no `--site`/`--channel`, the address comes from the `[[source]]` block, or
    /// from what the store already holds for this id.
    #[arg(long)]
    pub source: String,

    /// Any URL on a website. Only the origin is used.
    #[arg(long, conflicts_with = "channel")]
    #[serde(default)]
    pub site: Option<String>,

    /// A channel URL — `https://www.youtube.com/@CityofTampa`.
    #[arg(long)]
    #[serde(default)]
    pub channel: Option<String>,

    /// Requests per second. Omit to inherit the config. Website sources only.
    #[arg(long)]
    #[serde(default)]
    pub rps: Option<f64>,

    /// Stop after this many addresses, newest first where a source has an order.
    #[arg(long)]
    #[serde(default)]
    pub limit: Option<usize>,

    /// Maximum sitemap documents to fetch. Website sources only.
    #[arg(long, default_value_t = 200)]
    #[serde(default = "default_max_sitemaps")]
    pub max_sitemaps: usize,

    /// Maximum addresses to retain. Website sources only.
    #[arg(long, default_value_t = 500_000)]
    #[serde(default = "default_max_urls")]
    pub max_urls: usize,

    /// Extra arguments for yt-dlp. Channel sources only.
    #[arg(long = "yt-dlp-arg", allow_hyphen_values = true)]
    #[serde(default)]
    pub yt_dlp_args: Vec<String>,

    /// How many discovered addresses to include in the report.
    #[arg(long, default_value_t = 10)]
    #[serde(default = "default_sample")]
    pub sample: usize,

    /// Discover without writing a DiscoveryRun to the log.
    #[arg(long)]
    #[serde(default)]
    pub dry_run: bool,

    /// Config file. Defaults to the usual search path.
    #[arg(long, value_name = "FILE")]
    #[serde(default)]
    pub config: Option<String>,
}

fn default_max_sitemaps() -> usize {
    200
}
fn default_max_urls() -> usize {
    500_000
}
fn default_sample() -> usize {
    10
}

/// So a caller inside the library — [`crate::ops::run`] — gets the same limits the CLI
/// does, rather than a second set that drifts from the `default_value_t` above.
impl Default for DiscoverArgs {
    fn default() -> Self {
        Self {
            source: String::new(),
            site: None,
            channel: None,
            rps: None,
            limit: None,
            max_sitemaps: default_max_sitemaps(),
            max_urls: default_max_urls(),
            yt_dlp_args: Vec::new(),
            sample: default_sample(),
            dry_run: false,
            config: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DiscoverReport {
    pub source: String,
    pub kind: SourceKind,
    /// The site or channel this was enumerated from.
    pub target: String,
    /// How it was enumerated — `sitemap`, `playlist`. Provenance for a small result.
    pub method: String,
    pub found: usize,
    /// Addresses no previous snapshot contained. A **set difference**: a source that
    /// swapped fifty pages for fifty others moved by fifty while its count stood still.
    pub new: usize,
    /// Counted against the previous run. A large negative swing usually means a
    /// truncated pass, not a shrinking source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_run: Option<usize>,
    /// What the Source wanted said about how it got here — sitemaps walked, tabs read,
    /// rules assumed rather than obeyed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    /// The same provenance for a machine, named as the Source names it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub figures: BTreeMap<String, u64>,
    pub warnings: Vec<String>,
    pub sample: Vec<String>,
    pub written_to_log: bool,
}

/// Enumerate every address a source declares.
#[op(long_running, reach = "operator", group = "stage")]
pub async fn discover(
    ctx: &Ctx,
    args: DiscoverArgs,
    progress: &Progress,
) -> anyhow::Result<DiscoverReport> {
    let (config, _) = super::load_config(args.config.as_deref())?;

    let over = Overrides {
        rps: args.rps,
        yt_dlp_args: args.yt_dlp_args.clone(),
        limit: args.limit,
        limits: Some(DiscoveryLimits {
            max_sitemaps: args.max_sitemaps,
            max_urls: args.max_urls,
        }),
        ..Default::default()
    };

    let source = super::resolve_source(
        ctx,
        &config,
        &args.source,
        args.site.as_deref(),
        args.channel.as_deref(),
        &over,
    )
    .await?;

    let out = acquire::discover(
        &ctx.store,
        source.as_ref(),
        &DiscoverOpts {
            dry_run: args.dry_run,
            sample: args.sample,
        },
        progress,
    )
    .await?;

    Ok(DiscoverReport {
        source: args.source,
        kind: source.kind(),
        target: source.target().to_string(),
        method: source.method().to_string(),
        found: out.found,
        new: out.new,
        previous_run: out.previous_run,
        notes: out.notes,
        figures: out.figures,
        warnings: out.warnings,
        sample: out.sample,
        written_to_log: out.written_to_log,
    })
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// The count, and everything that would explain a wrong one.
///
/// A discovery run is trusted or it is not, and the facts that decide that are all
/// negative space: whether rules were actually read or merely assumed, which tabs returned
/// nothing, how the count moved against the previous run. Those arrive as [`Note`]s from
/// the Source itself, so this renderer paints them without knowing what a sitemap or a
/// channel tab is — which is what lets a third Source kind explain itself here for free.
impl Render for DiscoverReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.title(&self.source, &self.target)?;
        p.nest(|p| {
            let count = p.paint(&render::count(self.found as u64), Ink::Bold);
            let label = p.paint(
                &format!(
                    "{} discovered · {} new",
                    if self.found == 1 {
                        "address"
                    } else {
                        "addresses"
                    },
                    render::count(self.new as u64)
                ),
                Ink::Dim,
            );
            p.line(format!("{count} {label}"))?;

            if let Some(previous) = self.previous_run {
                let delta = self.found as i64 - previous as i64;
                let text = format!(
                    "{}{} against the previous run's {}",
                    if delta >= 0 { "+" } else { "" },
                    delta,
                    render::count(previous as u64),
                );
                // A shrinking snapshot is the signature of a truncated one, so it is amber
                // rather than a neutral figure.
                if delta < 0 {
                    p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
                } else {
                    p.line(p.paint(&text, Ink::Dim))?;
                }
            }

            p.blank()?;
            render_notes(p, &self.notes)?;

            p.marked(
                Mark::from_ok(self.written_to_log),
                p.paint(
                    if self.written_to_log {
                        "written to the log"
                    } else {
                        "not written — preview only"
                    },
                    Ink::Dim,
                ),
            )?;

            for warning in &self.warnings {
                p.marked(Mark::Warn, p.paint(&render::one_line(warning), Ink::Dim))?;
            }

            if !self.sample.is_empty() {
                p.section("sample")?;
                for key in &self.sample {
                    let text = render::truncate_start(key, p.width());
                    p.line(p.paint(&text, Ink::Dim))?;
                }
            }
            Ok(())
        })
    }
}

/// Width of the note label column. Fits `crawl-delay` plus a gutter.
const NOTE_COL: usize = 13;

/// Paints a Source's provenance.
///
/// Shared with `collect`, because a note means the same thing whichever half of
/// acquisition produced it.
pub(super) fn render_notes(p: &mut Painter<'_>, notes: &[Note]) -> std::io::Result<()> {
    for note in notes {
        let label = format!("{:<NOTE_COL$}", note.label);
        let width = p.width().saturating_sub(NOTE_COL + 2);
        let line = format!(
            "{}{}",
            p.paint(&label, Ink::Label),
            p.paint(&render::truncate_start(&note.detail, width), Ink::Dim),
        );
        match note.mark {
            Some(mark) => p.marked(mark.mark(), line)?,
            None => p.line(format!("  {line}"))?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_to_string(report: &DiscoverReport) -> String {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    fn report(found: usize, new: usize, previous: Option<usize>) -> DiscoverReport {
        DiscoverReport {
            source: "tampa".into(),
            kind: SourceKind::Site,
            target: "https://www.tampa.gov".into(),
            method: "sitemap".into(),
            found,
            new,
            previous_run: previous,
            notes: Vec::new(),
            figures: BTreeMap::new(),
            warnings: Vec::new(),
            sample: Vec::new(),
            written_to_log: true,
        }
    }

    #[test]
    fn the_count_and_what_is_new_both_lead() {
        let out = render_to_string(&report(12_000, 40, None));
        assert!(out.contains("12,000"), "{out}");
        assert!(out.contains("40 new"), "{out}");
    }

    /// A shrinking snapshot is usually a truncated pass, so it must not read as a neutral
    /// figure.
    #[test]
    fn a_shrinking_snapshot_is_flagged() {
        let out = render_to_string(&report(400, 0, Some(12_000)));
        assert!(out.contains("-11600"), "{out}");
        assert!(out.contains('!') || out.contains('⚠'), "unmarked: {out}");
    }

    /// The renderer paints notes it does not understand — which is how a third Source
    /// kind explains itself without editing this file.
    #[test]
    fn a_sources_own_provenance_is_shown_verbatim() {
        let mut r = report(831, 831, None);
        r.kind = SourceKind::Channel;
        r.method = "playlist".into();
        r.notes = vec![
            Note::ok_or_warn("streams", "831 videos", true),
            Note::ok_or_warn("shorts", "0 videos", false),
            Note::new("yt-dlp", "2026.01.15"),
        ];

        let out = render_to_string(&r);
        assert!(out.contains("streams"), "{out}");
        assert!(out.contains("831 videos"), "{out}");
        assert!(out.contains("2026.01.15"), "{out}");
        assert!(out.contains("shorts"), "an empty tab is the point: {out}");
    }

    #[test]
    fn a_dry_run_says_it_wrote_nothing() {
        let mut r = report(10, 10, None);
        r.written_to_log = false;
        assert!(render_to_string(&r).contains("preview only"));
    }

    #[test]
    fn the_report_round_trips_through_json() {
        let mut r = report(12, 3, Some(9));
        r.notes = vec![Note::ok_or_warn("robots.txt", "read", true)];
        r.figures = BTreeMap::from([("disallowed".into(), 4)]);

        let json = serde_json::to_value(&r).unwrap();
        let back: DiscoverReport = serde_json::from_value(json).unwrap();
        assert_eq!(back.found, 12);
        assert_eq!(back.new, 3);
        assert_eq!(back.notes[0].label, "robots.txt");
        assert_eq!(back.figures["disallowed"], 4);
    }
}
