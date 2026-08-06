//! `extract` — derive text from everything collected.
//!
//! Reads the blob pool, never the network. Each derivation is a `Blob → Blob` edge
//! carrying the tool and version that made it (SPEC §4.3), so when the PDF library
//! improves the whole corpus can be re-derived offline.

use std::collections::{BTreeMap, HashSet};

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::domain::Underivable;
use crate::extract::{self, Extracted, extract as extract_bytes};
use crate::fetch::{SNIFF_BYTES, content_kind};
use crate::op::{ItemOutcome, Verdict};
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

/// So [`crate::ops::run`] inherits the CLI's defaults instead of restating them.
impl Default for ExtractArgs {
    fn default() -> Self {
        Self {
            source: None,
            limit: None,
            refresh: false,
            kind: None,
            sample: default_sample(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExtractSample {
    pub url: String,
    pub kind: String,
    pub tool: String,
    pub chars: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// Skipped because this pipeline version already found nothing to extract here.
    ///
    /// Counted apart from `unextractable` so a second run reads as quiet: the same audio
    /// file is not news twice.
    #[serde(default)]
    pub already_unextractable: usize,
    pub attempted: usize,
    pub extracted: usize,
    /// The primary reader found no text and a fallback did. Worth its own figure: it is
    /// the measure of how much the primary is missing, and the number to watch after any
    /// change to it.
    #[serde(default)]
    pub recovered_by_fallback: usize,
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
#[op(long_running, group = "stage")]
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
        already_unextractable: 0,
        attempted: 0,
        extracted: 0,
        recovered_by_fallback: 0,
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
        let replay = ctx.store.replay(source).await?;

        // Blobs that already have a derivation. Keyed by source blob only: the HTML
        // pipeline picks between two tools at runtime, so the tool is not known until
        // after extraction and cannot be part of a skip key. `--refresh` is the
        // deliberate escape hatch after a tool upgrade.
        //
        // A Derivation whose bytes are the empty blob is not a derivation — see
        // `BlobSha::is_of_nothing`. Older runs wrote 490 of them for PDFs whose every page
        // was flagged for OCR, and this predicate called each one done: the blob could
        // never be re-read, and the pipeline-version switch could not reach it either,
        // because that switch is carried on an Underivable. Excluding them here is what
        // makes those blobs outstanding again, without rewriting an append-only log.
        let derived: HashSet<_> = replay
            .derivations()
            .filter(|d| !d.to_sha.is_of_nothing())
            .map(|d| &d.from_sha)
            .collect();
        // The other half of "already done". A blob nothing can extract never gains a
        // Derivation, so without this it is read, hashed and re-attempted on every run
        // for the life of the corpus — which for a channel means re-reading every audio
        // file twice a day to reach the same conclusion.
        let given_up = replay.underivable_by(extract::PIPELINE, extract::PIPELINE_VERSION);

        let latest = replay.latest_observations();
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
            if !args.refresh && given_up.contains(&obs.blob_sha) {
                report.already_unextractable += 1;
                continue;
            }

            // Every item, not every twenty-fifth: the bar sits directly above a tally that
            // moves on each one, and the two drifting apart is what made the collect
            // display look broken.
            progress.step(format!("{} extracted", report.extracted), i as u64, total);
            let started = std::time::Instant::now();

            // The head decides the kind, and the kind decides whether the rest is worth
            // reading. `get_blob` reads the whole file and verifies it, which is the
            // right thing to do before extracting and the wrong thing to do before
            // finding out there is nothing to extract.
            let head = ctx.store.blob_head(&obs.blob_sha, SNIFF_BYTES).await?;
            let kind = content_kind(&obs.meta, &head).to_string();
            if let Some(want) = &args.kind
                && want != &kind
            {
                continue;
            }
            report.attempted += 1;

            let bytes = ctx.store.get_blob(&obs.blob_sha).await?;
            let kind = kind.as_str();
            *report.by_kind.entry(kind.to_string()).or_default() += 1;

            // The Source's own title, where it has one. A YouTube recording's title is
            // never spoken aloud, so it reaches the text only from here — see
            // `extract_captions`.
            let mut outcome = extract_bytes(
                kind,
                &bytes,
                Some(&resource.natural_key),
                obs.meta.get("title").map(String::as_str),
            );

            // **A Derivation always has bytes.** An extractor that parsed the document and
            // came back with nothing has produced a verdict, not a derivation, and the two
            // are separate records because only the verdict carries a pipeline version —
            // the one thing that lets a better reader have another go. Writing the empty
            // blob as a Derivation instead put 490 PDFs beyond the reach of a version bump,
            // marked complete and absent from every search.
            //
            // Enforced here rather than in the arm that got it wrong, because this is where
            // the record is written: any extractor returning `Partial` with an empty
            // extraction is the same mistake, and there is now one place that catches it.
            if outcome.text().is_some_and(|t| t.trim().is_empty()) {
                // One more reader before giving up — see `pdf_text_via_poppler` for why a
                // PDF specifically deserves it.
                let recovered = match kind {
                    "pdf" => {
                        extract::pdf_text_via_poppler(&ctx.store.blob_path_of(&obs.blob_sha)).await
                    }
                    _ => None,
                };
                outcome = match recovered {
                    Some(extraction) => {
                        report.recovered_by_fallback += 1;
                        Extracted::Text(extraction)
                    }
                    None => Extracted::Unextractable {
                        reason: no_text_reason(&outcome),
                    },
                };
            }
            let outcome = outcome;

            let item = |verdict, produced, detail| ItemOutcome {
                address: resource.natural_key.clone(),
                // The content kind, because it is what decides which reader ran and
                // therefore what a surprising result is a surprise *about*.
                tag: kind.to_string(),
                verdict,
                noun: "documents".into(),
                bytes: bytes.len() as u64,
                produced,
                millis: started.elapsed().as_millis() as u64,
                detail,
                nested: false,
            };

            match &outcome {
                Extracted::Unextractable { reason } => {
                    report.unextractable += 1;
                    // Nothing to read is a fact about the format, not a fault in the run:
                    // a `.dwg` has no text and never will.
                    // `Some(0)` rather than `None`: it produced nothing, which is a
                    // measurement. `None` means the stage does not measure output at all,
                    // and mixing the two inside one stage breaks the column.
                    progress.item(item(Verdict::Missing, Some(0), Some(reason.clone())));
                    if report.unreadable.len() < 20 {
                        report.unreadable.push(Unreadable {
                            url: resource.natural_key.clone(),
                            kind: kind.to_string(),
                            reason: reason.clone(),
                        });
                    }
                    // Written down, so the next run skips it instead of learning the same
                    // thing again. Carries the pipeline version, so a later one that can
                    // read this kind is not bound by this verdict.
                    ctx.store
                        .append(
                            source,
                            &LogRecord::Underivable(Underivable {
                                from_sha: obs.blob_sha.clone(),
                                tool: extract::PIPELINE.to_string(),
                                version: extract::PIPELINE_VERSION.to_string(),
                                reason: reason.clone(),
                                at: Timestamp::now(),
                            }),
                        )
                        .await?;
                    continue;
                }
                Extracted::Partial {
                    extraction,
                    pages_needing_ocr,
                } => {
                    report.needs_ocr += 1;
                    report.ocr_pages_pending += pages_needing_ocr.len();
                    // Text came out, and some of the document is images nothing can read
                    // until OCR exists. Worth a line that reads differently.
                    progress.item(item(
                        Verdict::Warn,
                        Some(extraction.text.chars().count() as u64),
                        Some(format!(
                            "{} page{} need OCR",
                            pages_needing_ocr.len(),
                            if pages_needing_ocr.len() == 1 {
                                ""
                            } else {
                                "s"
                            }
                        )),
                    ));
                }
                Extracted::Text(e) => {
                    progress.item(item(Verdict::Ok, Some(e.text.chars().count() as u64), None))
                }
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

/// Why an extraction that parsed cleanly still yielded nothing.
///
/// Carries the OCR page count into the [`Underivable`]'s reason, because that is now the
/// only record of it: the verdict replaces the `Partial` that used to hold the list, and a
/// future OCR pipeline should be able to see from the log which blobs are waiting for it.
fn no_text_reason(outcome: &Extracted) -> String {
    match outcome {
        Extracted::Partial {
            pages_needing_ocr, ..
        } => format!(
            "parsed but holds no readable text; {} page{} {} images no reader here can read",
            pages_needing_ocr.len(),
            if pages_needing_ocr.len() == 1 { "" } else { "s" },
            if pages_needing_ocr.len() == 1 { "is" } else { "are" },
        ),
        _ => "parsed but holds no text".into(),
    }
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// The counters, which pipeline handled what, and a look at the text.
///
/// The sample is the point. Extraction is the stage that fails *quietly* — a PDF that
/// yields three characters of ligature soup counts as extracted and passes every counter
/// in this report. Printing the first line of a few documents is the cheapest way for a
/// person to catch that, so it is rendered rather than filed under `--json`.
impl Render for ExtractReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.title(
            &self.sources.join(", "),
            &format!("{} of text", render::count(self.chars_of_text as u64)),
        )?;
        p.nest(|p| {
            p.figures(&[
                (self.observations as u64, "observations"),
                (self.already_derived as u64, "already derived"),
                (self.already_unextractable as u64, "already unextractable"),
                (self.attempted as u64, "attempted"),
                (self.extracted as u64, "extracted"),
                (self.recovered_by_fallback as u64, "read by the fallback"),
                (self.needs_ocr as u64, "need OCR"),
                (self.unextractable as u64, "unextractable"),
            ])?;

            if self.ocr_pages_pending > 0 {
                p.blank()?;
                let text = format!(
                    "{} scanned pages need pdftoppm + tesseract",
                    render::count(self.ocr_pages_pending as u64)
                );
                p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
            }

            if !self.by_tool.is_empty() {
                p.section("by tool")?;
                let mut table = Table::bare(&[Align::Right, Align::Left]);
                for (tool, n) in &self.by_tool {
                    table.push(vec![
                        Cell::new(render::count(*n as u64), Ink::Bold),
                        Cell::dim(tool),
                    ]);
                }
                p.table(&table)?;
            }

            if !self.unreadable.is_empty() {
                p.section("unreadable")?;
                for item in &self.unreadable {
                    item.render(p)?;
                }
            }

            if !self.sample.is_empty() {
                p.section("sample")?;
                for (i, item) in self.sample.iter().enumerate() {
                    if i > 0 {
                        p.blank()?;
                    }
                    item.render(p)?;
                }
            }
            Ok(())
        })
    }
}

impl Render for Unreadable {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let head = format!(
            "{:<6}{}",
            self.kind,
            render::truncate(&self.url, p.width().saturating_sub(10)),
        );
        p.marked(Mark::Bad, head)?;
        p.nest(|p| p.wrapped(&render::one_line(&self.reason), Ink::Dim))
    }
}

impl Render for ExtractSample {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let label = self.title.as_deref().unwrap_or(&self.url);
        let head = p.paint(
            &render::truncate(label, p.width().saturating_sub(22)),
            Ink::Bold,
        );
        let aside = p.paint(
            &format!("{} · {}", self.tool, render::count(self.chars as u64)),
            Ink::Dim,
        );
        p.line(format!("{head}  {aside}"))?;
        p.nest(|p| p.wrapped(&render::one_line(&self.preview), Ink::Dim))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    /// Ogg magic bytes. `content_kind` sniffs these, so this classifies as audio and no
    /// extractor claims it — which is the case this module used to re-learn every run.
    const AUDIO: &[u8] = b"OggS\x00\x02 pretend this is a three hour meeting";

    async fn store_with(bytes: &[u8], key: &str) -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).await.unwrap();
        store
            .record_observation(
                &Resource::new(SourceId::new("tampa").unwrap(), key),
                bytes,
                jiff::Timestamp::now(),
                Default::default(),
            )
            .await
            .unwrap();
        (dir, Ctx::new(store))
    }

    fn args() -> ExtractArgs {
        ExtractArgs {
            source: None,
            limit: None,
            refresh: false,
            kind: None,
            sample: 5,
        }
    }

    /// The defect: nothing recorded that a blob had been given up on, so the next run
    /// read it, hashed it and reached the same conclusion — forever, twice a day.
    #[tokio::test]
    async fn a_blob_nothing_can_extract_is_attempted_once() {
        let (_d, ctx) = store_with(AUDIO, "https://y.test/watch?v=a#audio").await;

        let first = extract(&ctx, args(), &Progress::none()).await.unwrap();
        assert_eq!(first.attempted, 1);
        assert_eq!(first.unextractable, 1);
        assert_eq!(first.already_unextractable, 0);
        assert_eq!(first.unreadable.len(), 1, "and it is reported once");

        let again = extract(&ctx, args(), &Progress::none()).await.unwrap();
        assert_eq!(again.attempted, 0, "the second run must not try again");
        assert_eq!(again.unextractable, 0);
        assert_eq!(again.already_unextractable, 1);
        assert!(again.unreadable.is_empty(), "not news twice");
    }

    /// A one-page PDF carrying no text at all. Whether `pdf-inspector` calls this a parse
    /// failure or a page needing OCR is its business; either way nothing readable comes out.
    const BLANK_PDF: &[u8] = b"%PDF-1.4\n\
        1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
        2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
        3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>endobj\n\
        trailer<</Root 1 0 R>>\n%%EOF\n";

    /// **The invariant.** A `Derivation` always has bytes, so an extraction that produced
    /// nothing must be recorded as an `Underivable` — the record that carries a pipeline
    /// version and can therefore be revisited.
    ///
    /// Asserted on the log rather than on a counter, and without caring which arm the
    /// reader took: the defect was that one arm wrote the empty blob as a derivation, and
    /// the point of enforcing it at the write site is that *no* arm can.
    #[tokio::test]
    async fn an_extraction_that_yields_nothing_is_never_a_derivation() {
        let (_d, ctx) = store_with(BLANK_PDF, "https://tampa.test/blank.pdf").await;

        let report = extract(&ctx, args(), &Progress::none()).await.unwrap();
        assert_eq!(report.attempted, 1);
        assert_eq!(report.unextractable, 1, "nothing readable came out");
        assert_eq!(report.extracted, 0, "and nothing was derived");

        let replay = ctx
            .store
            .replay(&SourceId::new("tampa").unwrap())
            .await
            .unwrap();
        assert!(
            replay.derivations().next().is_none(),
            "the empty blob must never be recorded as derived text"
        );
        assert_eq!(
            replay
                .underivable_by(extract::PIPELINE, extract::PIPELINE_VERSION)
                .len(),
            1,
            "the verdict is filed where a version bump can reach it"
        );
    }

    /// The repair for the 490 already in a real store. The log is append-only, so the
    /// mis-filed Derivations cannot be removed — the predicate has to stop believing them.
    #[tokio::test]
    async fn a_derivation_of_nothing_leaves_the_blob_outstanding() {
        let (_d, ctx) = store_with(AUDIO, "https://y.test/watch?v=a#audio").await;
        let source = SourceId::new("tampa").unwrap();

        // Exactly what an older run wrote: a Derivation whose bytes are the empty blob.
        let obs_sha = {
            let replay = ctx.store.replay(&source).await.unwrap();
            let (_, obs) = replay
                .latest_observations()
                .into_iter()
                .next()
                .expect("the store was just given one observation");
            obs.blob_sha
        };
        let nothing = ctx.store.put_blob(b"").await.unwrap();
        assert!(nothing.is_of_nothing(), "the sentinel is the hash of no bytes");
        ctx.store
            .append(
                &source,
                &LogRecord::Derivation(crate::domain::Derivation {
                    from_sha: obs_sha,
                    to_sha: nothing,
                    tool: "pdf-inspector".into(),
                    version: "0.1.7".into(),
                    model_tier: None,
                    at: Timestamp::now(),
                    anchors: vec![],
                }),
            )
            .await
            .unwrap();

        let report = extract(&ctx, args(), &Progress::none()).await.unwrap();
        assert_eq!(
            report.already_derived, 0,
            "an empty derivation is not a derivation"
        );
        assert_eq!(report.attempted, 1, "so the blob is read again");
    }

    /// The verdict belongs to one pipeline version, and `--refresh` is the way to ignore
    /// it without waiting for a new one.
    #[tokio::test]
    async fn refresh_reopens_a_question_a_previous_run_closed() {
        let (_d, ctx) = store_with(AUDIO, "https://y.test/watch?v=a#audio").await;
        extract(&ctx, args(), &Progress::none()).await.unwrap();

        let forced = extract(
            &ctx,
            ExtractArgs {
                refresh: true,
                ..args()
            },
            &Progress::none(),
        )
        .await
        .unwrap();
        assert_eq!(forced.attempted, 1);
        assert_eq!(forced.already_unextractable, 0);
    }

    /// The record names the pipeline and its version, so a later one is not bound by it.
    #[tokio::test]
    async fn the_verdict_is_recorded_against_the_pipeline_that_reached_it() {
        let (_d, ctx) = store_with(AUDIO, "https://y.test/watch?v=a#audio").await;
        extract(&ctx, args(), &Progress::none()).await.unwrap();

        let replay = ctx
            .store
            .replay(&SourceId::new("tampa").unwrap())
            .await
            .unwrap();
        assert_eq!(
            replay
                .underivable_by(extract::PIPELINE, extract::PIPELINE_VERSION)
                .len(),
            1
        );
        assert!(
            replay.underivable_by(extract::PIPELINE, "99").is_empty(),
            "a later pipeline version gets its own go"
        );
    }

    /// An extractable document still extracts, and is skipped by its Derivation rather
    /// than by a verdict.
    #[tokio::test]
    async fn an_extractable_document_is_derived_and_then_skipped() {
        let (_d, ctx) = store_with(
            b"<html><body><h1>Agenda</h1><p>Item one.</p></body></html>",
            "https://tampa.gov/a",
        )
        .await;

        let first = extract(&ctx, args(), &Progress::none()).await.unwrap();
        assert_eq!(first.extracted, 1);
        assert_eq!(first.unextractable, 0);

        let again = extract(&ctx, args(), &Progress::none()).await.unwrap();
        assert_eq!(again.already_derived, 1);
        assert_eq!(again.already_unextractable, 0);
        assert_eq!(again.attempted, 0);
    }
}
