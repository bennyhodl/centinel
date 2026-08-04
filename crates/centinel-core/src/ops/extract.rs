//! `extract` — derive text from everything collected.
//!
//! Reads the blob pool, never the network. Each derivation is a `Blob → Blob` edge
//! carrying the tool and version that made it (SPEC §4.3), so when the PDF library
//! improves the whole corpus can be re-derived offline.

use std::collections::{BTreeMap, HashSet};

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::extract::{Extracted, extract as extract_bytes};
use crate::fetch::content_kind;
use crate::prelude::*;
use crate::store::LogRecord;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct ExtractArgs {
    /// Source to extract. Omit to extract every source in the store.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,

    /// Stop after this many documents.
    #[arg(long)]
    #[serde(default)]
    pub limit: Option<usize>,

    /// Re-derive documents that already have a derivation.
    ///
    /// The path to take after upgrading an extraction tool: derivations record their
    /// tool and version, but re-deriving is the only way to act on that.
    #[arg(long)]
    #[serde(default)]
    pub refresh: bool,

    /// Only extract this content kind — `html`, `pdf`, `spreadsheet`.
    #[arg(long)]
    #[serde(default)]
    pub kind: Option<String>,

    /// Documents to show in the report.
    #[arg(long, default_value_t = 5)]
    #[serde(default = "default_sample")]
    pub sample: usize,
}

fn default_sample() -> usize {
    5
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExtractSample {
    pub url: String,
    pub kind: String,
    pub tool: String,
    pub chars: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// First line or so of the derived text — enough to eyeball quality.
    pub preview: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Unreadable {
    pub url: String,
    pub kind: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExtractReport {
    pub sources: Vec<String>,
    pub observations: usize,
    /// Skipped because a derivation already existed.
    pub already_derived: usize,
    pub attempted: usize,
    pub extracted: usize,
    /// Extracted, but some pages are scans needing OCR we cannot perform.
    pub needs_ocr: usize,
    pub unextractable: usize,
    pub chars_of_text: usize,
    /// Which pipeline handled how many documents.
    pub by_tool: BTreeMap<String, usize>,
    pub by_kind: BTreeMap<String, usize>,
    /// Pages whose scans we could not read. Requires `pdftoppm` + `tesseract`.
    pub ocr_pages_pending: usize,
    pub unreadable: Vec<Unreadable>,
    pub sample: Vec<ExtractSample>,
}

/// Derive searchable text from collected documents.
#[op(long_running)]
pub async fn extract(
    ctx: &Ctx,
    args: ExtractArgs,
    progress: &Progress,
) -> anyhow::Result<ExtractReport> {
    let sources = match &args.source {
        Some(s) => vec![SourceId::new(s.clone())?],
        None => ctx.store.sources().await?,
    };

    let mut report = ExtractReport {
        sources: sources.iter().map(|s| s.to_string()).collect(),
        observations: 0,
        already_derived: 0,
        attempted: 0,
        extracted: 0,
        needs_ocr: 0,
        unextractable: 0,
        chars_of_text: 0,
        by_tool: BTreeMap::new(),
        by_kind: BTreeMap::new(),
        ocr_pages_pending: 0,
        unreadable: Vec::new(),
        sample: Vec::new(),
    };

    for source in &sources {
        let log = ctx.store.read_log(source).await?;

        // Blobs that already have a derivation. Keyed by source blob only: the HTML
        // pipeline picks between two tools at runtime, so the tool is not known until
        // after extraction and cannot be part of a skip key. `--refresh` is the
        // deliberate escape hatch after a tool upgrade.
        let derived: HashSet<_> = log
            .iter()
            .filter_map(|r| match r {
                LogRecord::Derivation(d) => Some(d.from_sha.clone()),
                _ => None,
            })
            .collect();

        let latest = ctx.store.latest_observations(source).await?;
        report.observations += latest.len();

        let total = latest.len() as u64;
        for (i, (resource, obs)) in latest.iter().enumerate() {
            if let Some(limit) = args.limit
                && report.attempted >= limit
            {
                break;
            }
            if !args.refresh && derived.contains(&obs.blob_sha) {
                report.already_derived += 1;
                continue;
            }

            if i % 25 == 0 {
                progress.step(format!("{} extracted", report.extracted), i as u64, total);
            }

            let bytes = ctx.store.get_blob(&obs.blob_sha).await?;
            let kind = content_kind(&obs.meta, &bytes);
            if let Some(want) = &args.kind
                && want != kind
            {
                continue;
            }
            report.attempted += 1;
            *report.by_kind.entry(kind.to_string()).or_default() += 1;

            // The Source's own title, where it has one. A YouTube recording's title is
            // never spoken aloud, so it reaches the text only from here — see
            // `extract_captions`.
            let outcome = extract_bytes(
                kind,
                &bytes,
                Some(&resource.natural_key),
                obs.meta.get("title").map(String::as_str),
            );

            match &outcome {
                Extracted::Unextractable { reason } => {
                    report.unextractable += 1;
                    if report.unreadable.len() < 20 {
                        report.unreadable.push(Unreadable {
                            url: resource.natural_key.clone(),
                            kind: kind.to_string(),
                            reason: reason.clone(),
                        });
                    }
                    continue;
                }
                Extracted::Partial {
                    pages_needing_ocr, ..
                } => {
                    report.needs_ocr += 1;
                    report.ocr_pages_pending += pages_needing_ocr.len();
                }
                Extracted::Text(_) => {}
            }

            let (tool, version) = outcome.tool().expect("non-unextractable has a tool");
            let text = outcome.text().unwrap_or_default();

            // The derived text becomes a blob of its own, so it is versioned and
            // deduplicated exactly like the source bytes.
            let to_sha = ctx.store.put_blob(text.as_bytes()).await?;

            ctx.store
                .append(
                    source,
                    &LogRecord::Derivation(Derivation {
                        from_sha: obs.blob_sha.clone(),
                        to_sha,
                        tool: tool.to_string(),
                        version: version.to_string(),
                        model_tier: None,
                        at: Timestamp::now(),
                        anchors: vec![],
                    }),
                )
                .await?;

            report.extracted += 1;
            report.chars_of_text += text.chars().count();
            *report.by_tool.entry(tool.to_string()).or_default() += 1;

            if report.sample.len() < args.sample {
                let title = match &outcome {
                    Extracted::Text(e) | Extracted::Partial { extraction: e, .. } => {
                        e.title.clone()
                    }
                    Extracted::Unextractable { .. } => None,
                };
                report.sample.push(ExtractSample {
                    url: resource.natural_key.clone(),
                    kind: kind.to_string(),
                    tool: tool.to_string(),
                    chars: text.chars().count(),
                    title,
                    preview: text
                        .chars()
                        .take(180)
                        .collect::<String>()
                        .replace('\n', " "),
                });
            }
        }
    }

    progress.say(format!("{} documents extracted", report.extracted));
    Ok(report)
}
