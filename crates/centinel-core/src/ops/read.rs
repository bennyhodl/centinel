//! `read` — return the text of a collected document.
//!
//! The counterpart to `open`, and the one an **agent** actually wants. `open` launches
//! an application on somebody's screen; `read` returns characters. A model asked to
//! summarise a budget PDF cannot use a launched Preview window.
//!
//! Safe to expose remotely: it reads the store and runs nothing.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::content::ContentKind;
use crate::ops::target::resolve;
use crate::prelude::*;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// A URL, a substring of one, or a blob hash — the same targets `search` reports,
    /// including the short hash it prints.
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
    /// SHA-256 of the text itself. Also a valid target: `read` and `open` take it back.
    pub derived_sha: String,
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
#[op(group = "corpus")]
pub async fn read(ctx: &Ctx, args: ReadArgs) -> anyhow::Result<ReadReport> {
    let found = resolve(ctx, &args.target, args.source.as_deref()).await?;
    let (source, resource, obs) = (found.source, found.resource, found.observation);
    let other_matches = found.other_matches;

    // From the log `resolve` already read, not a second pass over it.
    let derivation = found
        .replay
        .latest_derivation(&obs.blob_sha)
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
    let kind = ContentKind::classify(&obs.meta, &original).to_string();

    Ok(ReadReport {
        url: resource.natural_key,
        source: source.to_string(),
        kind,
        blob_sha: obs.blob_sha.to_string(),
        derived_sha: derivation.to_sha.to_string(),
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
/// The header leads with the blob hash for the same reason `search` does: it is what you
/// pass to `open` to see this document in an application, or back to `read` to page
/// through it, and a citation you have to reconstruct is a citation nobody follows.
///
/// The text is what was asked for, so it is printed unwrapped and unpainted — a terminal's
/// own wrapping preserves the line structure the extractor produced, and re-flowing it here
/// would silently destroy the paragraph breaks and timestamps that make a transcript
/// readable.
impl Render for ReadReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.title(&render::truncate(&self.url, p.width()), "")?;

        let hash = p.paint(&render::short_sha(&self.blob_sha), Ink::Cyan);
        let provenance = format!(
            "{} · {} · {} · {}",
            self.source,
            self.kind,
            self.tool,
            render::short_time(&self.observed_at),
        );
        p.line(format!("{hash} · {}", p.paint(&provenance, Ink::Dim)))?;

        if !self.other_matches.is_empty() {
            let note = format!(
                "{} other {} matched; this is the first",
                self.other_matches.len(),
                if self.other_matches.len() == 1 {
                    "address"
                } else {
                    "addresses"
                },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Derivation;
    use crate::ops::target;
    use crate::store::{LogRecord, Store};

    const TEXT: &str = "# Agenda\n\nItem one. Item two. Item three.";

    /// A store holding one PDF and its extracted text.
    async fn corpus(dir: &std::path::Path) -> Ctx {
        let store = Store::open(dir.join("store")).await.unwrap();
        let id = SourceId::new("tampa").unwrap();

        let obs = store
            .record_observation(
                &Resource::new(id.clone(), "https://tampa.gov/agenda.pdf"),
                b"%PDF-1.7 pretend",
                jiff::Timestamp::now(),
                Default::default(),
            )
            .await
            .unwrap();

        let to_sha = store.put_blob(TEXT.as_bytes()).await.unwrap();
        store
            .append(
                &id,
                &LogRecord::Derivation(Derivation {
                    from_sha: obs.blob_sha,
                    to_sha,
                    tool: "pdf-inspector".into(),
                    version: "0.1".into(),
                    model_tier: None,
                    at: jiff::Timestamp::now(),
                    anchors: Vec::new(),
                }),
            )
            .await
            .unwrap();

        Ctx::new(store)
    }

    fn args(target: &str) -> ReadArgs {
        ReadArgs {
            target: target.into(),
            max_chars: 20_000,
            offset: 0,
            source: None,
        }
    }

    #[tokio::test]
    async fn reading_returns_the_text_and_the_kind_of_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = corpus(dir.path()).await;

        let r = read(&ctx, args("agenda.pdf")).await.unwrap();
        assert_eq!(r.text, TEXT);
        assert_eq!(
            r.kind, "pdf",
            "the caller wants to know it is reading a PDF"
        );
        assert_eq!(r.tool, "pdf-inspector 0.1");
        assert!(!r.truncated);
    }

    /// Both hashes the report carries are targets `read` itself accepts.
    #[tokio::test]
    async fn every_hash_read_reports_can_be_typed_back() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = corpus(dir.path()).await;
        let first = read(&ctx, args("agenda.pdf")).await.unwrap();

        assert_ne!(first.blob_sha, first.derived_sha);
        for printed in [&first.blob_sha, &first.derived_sha] {
            let again = read(&ctx, args(&printed[..12])).await.unwrap();
            assert_eq!(again.text, TEXT, "`{printed}` did not come back");
        }

        // And the same hashes resolve for `open`, which shares the resolver.
        assert!(
            target::resolve(&ctx, &first.derived_sha[..12], None)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn paging_reports_where_it_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = corpus(dir.path()).await;

        let head = read(
            &ctx,
            ReadArgs {
                max_chars: 8,
                ..args("agenda.pdf")
            },
        )
        .await
        .unwrap();
        assert_eq!(head.chars, 8);
        assert_eq!(head.total_chars, TEXT.chars().count());
        assert!(head.truncated);

        let tail = read(
            &ctx,
            ReadArgs {
                offset: 8,
                max_chars: 0,
                ..args("agenda.pdf")
            },
        )
        .await
        .unwrap();
        assert!(!tail.truncated);
        assert_eq!(format!("{}{}", head.text, tail.text), TEXT);
    }

    #[tokio::test]
    async fn a_document_with_no_extraction_says_which_command_makes_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).await.unwrap();
        store
            .record_observation(
                &Resource::new(SourceId::new("tampa").unwrap(), "https://tampa.gov/x.pdf"),
                b"%PDF-1.7 pretend",
                jiff::Timestamp::now(),
                Default::default(),
            )
            .await
            .unwrap();

        let err = read(&Ctx::new(store), args("x.pdf"))
            .await
            .map(|_| ())
            .unwrap_err()
            .to_string();
        assert!(err.contains("extract"), "{err}");
    }

    // ── rendering ──────────────────────────────────────────────────────────────

    fn render_to_string(report: &ReadReport) -> String {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    fn report() -> ReadReport {
        ReadReport {
            url: "https://tampa.gov/agenda.pdf".into(),
            source: "tampa".into(),
            kind: "pdf".into(),
            blob_sha: "3f8a1c9d0b7e".repeat(6)[..64].to_string(),
            derived_sha: "9b2e4a1f0c33".repeat(6)[..64].to_string(),
            observed_at: "2026-08-04T10:00:00Z".into(),
            tool: "pdf-inspector 0.1".into(),
            text: TEXT.into(),
            chars: 8,
            total_chars: 41,
            offset: 0,
            truncated: true,
            other_matches: Vec::new(),
        }
    }

    /// The header leads with the handle, and the footer says how to get the rest.
    #[test]
    fn the_header_leads_with_a_hash_that_resolves() {
        let out = render_to_string(&report());
        assert!(out.contains("3f8a1c9d0b7e"), "{out}");
        assert!(out.contains("pdf-inspector 0.1"), "{out}");
        assert!(out.contains("--offset 8"), "{out}");
    }

    /// A complete document needs no paging instructions.
    #[test]
    fn an_untruncated_read_offers_no_paging() {
        let mut r = report();
        r.truncated = false;
        assert!(!render_to_string(&r).contains("--offset"));
    }

    #[test]
    fn the_report_round_trips_through_json() {
        let r = report();
        let json = serde_json::to_value(&r).unwrap();
        let back: ReadReport = serde_json::from_value(json).unwrap();
        assert_eq!(back.blob_sha, r.blob_sha);
        assert_eq!(back.derived_sha, r.derived_sha);
        assert_eq!(back.total_chars, 41);
    }
}
