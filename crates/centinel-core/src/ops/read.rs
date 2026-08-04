//! `read` — return the text of a collected document.
//!
//! The counterpart to `open`, and the one an **agent** actually wants. `open` launches
//! an application on somebody's screen; `read` returns characters. A model asked to
//! summarise a budget PDF cannot use a launched Preview window.
//!
//! Safe to expose remotely: it reads the store and runs nothing.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fetch::content_kind;
use crate::prelude::*;
use crate::store::LogRecord;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// A URL, a substring of one, or a blob hash — the same targets `search` reports.
    #[arg(value_name = "TARGET")]
    pub target: String,

    /// Maximum characters to return. 0 returns everything.
    ///
    /// Defaulted rather than unbounded because a 300-page budget PDF would otherwise
    /// arrive as one enormous tool result and consume a model's whole context.
    #[arg(long, default_value_t = 20_000)]
    #[serde(default = "default_max_chars")]
    pub max_chars: usize,

    /// Start at this character offset, for paging through a long document.
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,

    /// Restrict the search for `target` to one source.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,
}

fn default_max_chars() -> usize {
    20_000
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReadReport {
    pub url: String,
    pub source: String,
    /// The kind of the *original* document, even though the text is derived from it.
    pub kind: String,
    /// SHA-256 of the original bytes as served — the evidentiary anchor.
    pub blob_sha: String,
    pub observed_at: String,
    /// Which extraction pipeline produced this text.
    pub tool: String,
    pub text: String,
    /// Characters returned.
    pub chars: usize,
    /// Total characters available, so a caller knows whether to page.
    pub total_chars: usize,
    pub offset: usize,
    /// True when `offset + chars < total_chars`.
    pub truncated: bool,
    /// Other matches for an ambiguous target. The first was used.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_matches: Vec<String>,
}

/// Read the extracted text of a collected document.
#[op]
pub async fn read(ctx: &Ctx, args: ReadArgs) -> anyhow::Result<ReadReport> {
    let sources = match &args.source {
        Some(s) => vec![SourceId::new(s.clone())?],
        None => ctx.store.sources().await?,
    };

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
        "nothing in the store matches `{}`. Try `search` first, or pass a full URL.",
        args.target
    );

    // An exact URL never loses to a longer one that merely contains it.
    matches.sort_by_key(|(_, r, _)| (r.natural_key != args.target, r.natural_key.clone()));
    let (source, resource, obs) = matches.first().cloned().expect("non-empty");
    let other_matches = matches
        .iter()
        .skip(1)
        .take(10)
        .map(|(_, r, _)| r.natural_key.clone())
        .collect();

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
                "no extracted text for {} — run `extract` first",
                resource.natural_key
            )
        })?;

    let derived = ctx.store.get_blob(&derivation.to_sha).await?;
    let full = String::from_utf8_lossy(&derived);
    let total_chars = full.chars().count();

    let text: String = if args.max_chars == 0 {
        full.chars().skip(args.offset).collect()
    } else {
        full.chars()
            .skip(args.offset)
            .take(args.max_chars)
            .collect()
    };
    let chars = text.chars().count();

    // The original bytes decide the kind — the text is markdown either way, and the
    // caller wants to know it is reading a PDF.
    let original = ctx.store.get_blob(&obs.blob_sha).await?;
    let kind = content_kind(&obs.meta, &original).to_string();

    Ok(ReadReport {
        url: resource.natural_key,
        source: source.to_string(),
        kind,
        blob_sha: obs.blob_sha.to_string(),
        observed_at: obs.at.to_string(),
        tool: format!("{} {}", derivation.tool, derivation.version),
        text,
        chars,
        total_chars,
        offset: args.offset,
        truncated: args.offset + chars < total_chars,
        other_matches,
    })
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// A header of provenance, then the document.
///
/// The text is what was asked for, so it is printed unwrapped and unpainted — a terminal's
/// own wrapping preserves the line structure the extractor produced, and re-flowing it here
/// would silently destroy the paragraph breaks and timestamps that make a transcript
/// readable.
impl Render for ReadReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.title(&render::truncate(&self.url, p.width()), "")?;

        let provenance = format!(
            "{} · {} · {} · {}",
            self.source,
            self.kind,
            self.tool,
            render::short_time(&self.observed_at),
        );
        p.line(p.paint(&provenance, Ink::Dim))?;

        if !self.other_matches.is_empty() {
            let note = format!(
                "{} other {} matched; this is the first",
                self.other_matches.len(),
                if self.other_matches.len() == 1 { "address" } else { "addresses" },
            );
            p.marked(Mark::Warn, p.paint(&note, Ink::Dim))?;
        }

        p.blank()?;
        for line in self.text.lines() {
            p.line(line)?;
        }

        // Where the reader is in the document, and how to get the rest. Printed only when
        // there *is* a rest — a complete document needs no paging instructions.
        if self.truncated {
            p.blank()?;
            let position = format!(
                "{}–{} of {} characters",
                render::count(self.offset as u64),
                render::count((self.offset + self.chars) as u64),
                render::count(self.total_chars as u64),
            );
            p.line(p.paint(&position, Ink::Dim))?;
            let next = format!("--offset {}", self.offset + self.chars);
            p.line(p.paint(&next, Ink::Cyan))?;
        }
        Ok(())
    }
}
