//! `status` — how much is in the corpus, and what kind of thing it is.
//!
//! The question `doctor` used to answer with one number and a walk of the whole pool. It
//! is asked of the **corpus** rather than the machine, and it is asked often: collection
//! is the point of this project, so "did that run add anything, and to what" is the
//! measure of a day's work.
//!
//! ## Counted per address, off the log
//!
//! One row per Source, one column per content kind, and the unit is a stored **address** —
//! the latest Observation of each Resource. Not blobs: the pool holds derived text beside
//! collected bytes and pools identical files across Sources, so a blob count answers a
//! question about disk rather than about coverage. Not Observations either, for the
//! opposite reason — a page collected weekly for a year is one document with fifty-two
//! versions, and counting them would report a corpus fifty times the size of the one a
//! search can see.
//!
//! Reads only truth (`log/`), like [`crate::ops::list`]: no index, no blob is read, and the
//! kind comes off the record via [`ContentKind::from_record`]. That is what keeps this a
//! read of a few JSONL files rather than an open per document — see that function for the
//! one thing the unread bytes would have settled.
//!
//! The same PDF collected from two Sources counts twice, once in each row, because two
//! governments publishing the same document is two publications. The pool still stores it
//! once — which is where the size column parts company with the counts, and says so.
//!
//! ## The size is the pool's answer, not the log's
//!
//! Bytes come from one `stat` per distinct blob, so `size` is what those documents occupy
//! rather than what the servers said they would. It is deliberately **not** what `du`
//! prints for the store: superseded versions of a changed page, the text `extract` derived
//! from every one of these, and the index are all on that disk and none of them is a
//! document this corpus collected.
//!
//! It is also the one figure a hole in the pool changes silently, so a blob the log names
//! and the pool does not hold is counted and reported rather than skipped.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::content::ContentKind;
use crate::prelude::*;

/// What one Source holds.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SourceCount {
    pub source: String,
    /// Addresses by content kind, keyed by the word the record holds — `html`, `pdf`,
    /// `document`. Kinds this Source has none of are absent rather than zero.
    pub kinds: BTreeMap<String, u64>,
    /// Stored addresses, whatever their kind. The sum of `kinds`.
    pub total: u64,
    /// What those addresses occupy in the pool, counting shared bytes once.
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct StatusReport {
    /// Which store answered. Not rendered — a person typed `--root` or accepted the
    /// default a second ago; an HTTP caller has no other way to know.
    pub store_root: String,
    pub sources: Vec<SourceCount>,
    /// The corpus-wide roll-up: every row above, summed per kind.
    pub kinds: BTreeMap<String, u64>,
    pub total: u64,
    /// What the whole corpus occupies. Distinct blobs, so two Sources holding the same
    /// document contribute one file here and one document each above — the only figure on
    /// this report that can come to less than the rows it sits under, and it does so
    /// because that is what is on the disk.
    pub bytes: u64,
    /// Blobs the log names that the pool does not hold. Zero on a whole corpus.
    ///
    /// Reported because it is the one thing that makes `bytes` quietly wrong: bytes that
    /// are not there occupy nothing, so a corpus with a hole in it reads as a smaller
    /// corpus rather than as a damaged one.
    #[serde(default)]
    pub missing_blobs: u64,
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct StatusArgs {
    /// Limit to one source. Omit to count all of them.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,
}

/// Count what is stored, by source and content kind, and what it occupies on disk.
#[op(group = "corpus")]
pub async fn status(ctx: &Ctx, args: StatusArgs) -> anyhow::Result<StatusReport> {
    let sources = match &args.source {
        Some(s) => vec![SourceId::new(s.clone())?],
        None => ctx.store.sources().await?,
    };

    let mut rows: Vec<(String, BTreeMap<String, u64>, BTreeSet<BlobSha>)> =
        Vec::with_capacity(sources.len());
    let mut corpus: BTreeMap<String, u64> = BTreeMap::new();
    let mut every_blob: BTreeSet<BlobSha> = BTreeSet::new();

    for source in sources {
        let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
        let mut blobs: BTreeSet<BlobSha> = BTreeSet::new();
        // The latest Observation per Resource — one entry per address, however many times
        // it has been re-collected.
        for observation in ctx.store.latest_observations(&source).await?.values() {
            let kind = ContentKind::from_record(&observation.meta);
            *kinds.entry(kind.as_str().to_string()).or_default() += 1;
            *corpus.entry(kind.as_str().to_string()).or_default() += 1;
            // A set, because the pool is content-addressed: two addresses serving
            // byte-identical documents are one file, and counting the second would
            // invent disk that nothing occupies.
            blobs.insert(observation.blob_sha.clone());
        }
        every_blob.extend(blobs.iter().cloned());
        rows.push((source.to_string(), kinds, blobs));
    }

    // One `stat` per distinct blob in the corpus, however many rows name it.
    let sizes = ctx.store.blob_sizes(&every_blob).await?;
    let bytes_of =
        |blobs: &BTreeSet<BlobSha>| -> u64 { blobs.iter().filter_map(|sha| sizes.get(sha)).sum() };

    let sources: Vec<SourceCount> = rows
        .into_iter()
        .map(|(source, kinds, blobs)| SourceCount {
            source,
            total: kinds.values().sum(),
            bytes: bytes_of(&blobs),
            kinds,
        })
        .collect();

    Ok(StatusReport {
        store_root: ctx.store.root().display().to_string(),
        total: sources.iter().map(|r| r.total).sum(),
        // The union, not the sum of the rows: bytes two Sources share are one file.
        bytes: sizes.values().sum(),
        missing_blobs: (every_blob.len() - sizes.len()) as u64,
        sources,
        kinds: corpus,
    })
}

impl StatusReport {
    /// The kind columns, in reading order.
    ///
    /// Only the kinds this corpus actually holds. The vocabulary has thirteen and a city's
    /// website has three of them, so a fixed set would be ten columns of zeros pushing the
    /// total off the right of the screen.
    ///
    /// Ordered by [`ContentKind::ALL`] rather than by count, so a column does not move
    /// between two runs of the same command — the reason to run this twice is to compare
    /// the numbers, and a table that re-orders itself defeats that.
    fn columns(&self) -> Vec<&str> {
        let mut cols: Vec<&str> = ContentKind::ALL
            .iter()
            .map(|k| k.as_str())
            .filter(|k| self.kinds.contains_key(*k))
            .collect();
        // A log written by a newer build can hold a word this one has never heard of. It
        // is still an address that was collected, so it gets a column of its own at the
        // end — folding it into `other` would report a kind that was never recorded.
        cols.extend(
            self.kinds
                .keys()
                .map(String::as_str)
                .filter(|k| ContentKind::parse(k).is_none()),
        );
        cols
    }
}

/// One row per Source, the grand total last.
///
/// A matrix rather than a block per Source: the question underneath "how much is stored"
/// is almost always *which of these is thin*, and that comparison is a column to run an
/// eye down. The totals row is separated by a blank line rather than a rule, which is the
/// same answer the rest of these reports give to the same question.
impl Render for StatusReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        if self.sources.is_empty() {
            p.line(p.paint("No sources yet.", Ink::Dim))?;
            return p.note("centinel discover --source <name> --site <url>");
        }

        let cols = self.columns();
        let mut header: Vec<(&str, Align)> = vec![("source", Align::Left)];
        header.extend(cols.iter().map(|c| (*c, Align::Right)));
        header.push(("total", Align::Right));
        header.push(("size", Align::Right));

        let mut table = Table::new(&header);
        for row in &self.sources {
            table.push(cells(
                &row.source,
                Ink::Plain,
                &cols,
                &row.kinds,
                row.total,
                row.bytes,
            ));
        }

        // A store with one Source is its own total, and a second row repeating it teaches
        // the reader that the last line is chrome.
        if self.sources.len() > 1 {
            // Renders as a blank line: every cell is empty, so the joined row trims away.
            table.push(
                std::iter::repeat_with(|| Cell::plain(""))
                    .take(cols.len() + 3)
                    .collect(),
            );
            table.push(cells(
                "all",
                Ink::Bold,
                &cols,
                &self.kinds,
                self.total,
                self.bytes,
            ));
        }
        p.table(&table)?;

        // Under the figure it makes wrong, because bytes that are not there occupy
        // nothing: a corpus with a hole in it otherwise reads as a smaller corpus.
        if self.missing_blobs > 0 {
            let note = format!(
                "the log names {} the pool does not hold — the sizes above are short by \
                 whatever they held",
                render::plural(self.missing_blobs as usize, "blob", "blobs"),
            );
            p.marked(Mark::Warn, p.paint(&note, Ink::Dim))?;
        }
        Ok(())
    }
}

/// One row: the label, a figure per kind column, the total and what it occupies.
fn cells(
    label: &str,
    ink: Ink,
    cols: &[&str],
    kinds: &BTreeMap<String, u64>,
    total: u64,
    bytes: u64,
) -> Vec<Cell> {
    let mut row = vec![Cell::new(label, ink)];
    row.extend(cols.iter().map(|col| match kinds.get(*col) {
        // A dash, not a zero. In a wide sparse table a column of zeros reads as data and
        // hides the figures that are; this says "none of that here" at a glance.
        None | Some(0) => Cell::dim("—"),
        Some(n) => Cell::new(render::count(*n), ink),
    }));
    let empty = total == 0;
    row.push(Cell::new(
        render::count(total),
        if empty { Ink::Dim } else { Ink::Bold },
    ));
    row.push(Cell::new(
        render::bytes(bytes),
        if empty { Ink::Dim } else { Ink::Plain },
    ));
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Timestamp;

    /// A store holding `(source, address, content-type)`, in that order.
    ///
    /// Each address stores its own URL as its bytes, so every document has a size and no
    /// two of them collide in the pool by accident. The `TempDir` comes back with the
    /// `Ctx` because dropping it deletes the store out from under the test.
    async fn store_with(rows: &[(&str, &str, &str)]) -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(Store::open(dir.path()).await.unwrap());

        for (source, url, content_type) in rows {
            record(&ctx, source, url, content_type, url.as_bytes()).await;
        }
        (dir, ctx)
    }

    /// One Observation, with the bytes named — for the tests that are about size.
    async fn record(ctx: &Ctx, source: &str, url: &str, content_type: &str, bytes: &[u8]) {
        let resource = Resource::new(SourceId::new(source.to_string()).unwrap(), url);
        let meta = BTreeMap::from([
            ("content-type".to_string(), content_type.to_string()),
            ("final_url".to_string(), url.to_string()),
        ]);
        ctx.store
            .record_observation(&resource, bytes, Timestamp::now(), meta)
            .await
            .unwrap();
    }

    fn rendered(report: &StatusReport) -> String {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    #[tokio::test]
    async fn counts_are_per_source_and_the_last_row_is_the_corpus() {
        let (_dir, ctx) = store_with(&[
            ("tampa", "https://tampa.gov/a", "text/html"),
            ("tampa", "https://tampa.gov/b", "text/html"),
            ("tampa", "https://tampa.gov/budget.pdf", "application/pdf"),
            ("hillsclerk", "https://hillsclerk.com/x", "text/html"),
        ])
        .await;

        let report = status(&ctx, StatusArgs::default()).await.unwrap();

        let tampa = &report.sources[1];
        assert_eq!(tampa.source, "tampa");
        assert_eq!(tampa.kinds["html"], 2);
        assert_eq!(tampa.kinds["pdf"], 1);
        assert_eq!(tampa.total, 3);

        assert_eq!(report.kinds["html"], 3, "both sources roll up");
        assert_eq!(report.total, 4);
        assert_eq!(
            report.total,
            report.sources.iter().map(|s| s.total).sum::<u64>(),
            "the grand total is the rows, not a second count"
        );

        let out = rendered(&report);
        assert!(out.contains("html") && out.contains("pdf"), "{out}");
        assert!(out.contains("all"), "the totals row is drawn: {out}");
        assert!(out.contains("size"), "the disk column is drawn: {out}");
    }

    /// The size is measured, not declared: it comes from the pool, one `stat` per blob.
    #[tokio::test]
    async fn the_size_is_what_the_documents_occupy() {
        let (_dir, ctx) = store_with(&[]).await;
        record(
            &ctx,
            "tampa",
            "https://tampa.gov/a",
            "text/html",
            &[7u8; 300],
        )
        .await;
        record(
            &ctx,
            "tampa",
            "https://tampa.gov/budget.pdf",
            "application/pdf",
            &[9u8; 700],
        )
        .await;

        let report = status(&ctx, StatusArgs::default()).await.unwrap();
        assert_eq!(report.sources[0].bytes, 1000);
        assert_eq!(report.bytes, 1000);
        assert_eq!(report.missing_blobs, 0);
        assert!(
            rendered(&report).contains("1000 B"),
            "{}",
            rendered(&report)
        );
    }

    /// The pool is content-addressed, so two addresses serving identical bytes are one
    /// file. Counting the second would invent disk that nothing occupies — and it is why
    /// the corpus total can come to less than the rows above it.
    #[tokio::test]
    async fn bytes_shared_between_two_addresses_are_counted_once() {
        let (_dir, ctx) = store_with(&[]).await;
        let same = &[4u8; 500];
        record(&ctx, "tampa", "https://tampa.gov/a", "text/html", same).await;
        record(&ctx, "tampa", "https://tampa.gov/b", "text/html", same).await;
        record(
            &ctx,
            "hillsclerk",
            "https://hillsclerk.com/x",
            "text/html",
            same,
        )
        .await;

        let report = status(&ctx, StatusArgs::default()).await.unwrap();

        assert_eq!(report.total, 3, "three addresses were collected");
        assert_eq!(report.sources[1].bytes, 500, "tampa's two are one file");
        assert_eq!(report.bytes, 500, "and all three are that same file");
        assert!(
            report.bytes < report.sources.iter().map(|s| s.bytes).sum::<u64>(),
            "the corpus is smaller than its rows add to, which is the disk's answer"
        );
    }

    /// A hole in the pool changes the size and nothing else, so it has to be said. Bytes
    /// that are not there occupy nothing, and a damaged corpus would otherwise read as a
    /// smaller one.
    #[tokio::test]
    async fn a_blob_the_pool_lost_is_reported_rather_than_quietly_dropped() {
        let (_dir, ctx) = store_with(&[]).await;
        record(
            &ctx,
            "tampa",
            "https://tampa.gov/a",
            "text/html",
            &[1u8; 400],
        )
        .await;
        record(
            &ctx,
            "tampa",
            "https://tampa.gov/b",
            "text/html",
            &[2u8; 600],
        )
        .await;

        let lost = ctx.store.blob_path_of(&BlobSha::from_bytes(&[2u8; 600]));
        std::fs::remove_file(&lost).unwrap();

        let report = status(&ctx, StatusArgs::default()).await.unwrap();
        assert_eq!(report.total, 2, "the record still holds both addresses");
        assert_eq!(report.bytes, 400, "only the bytes that are there");
        assert_eq!(report.missing_blobs, 1);
        assert!(
            rendered(&report).contains("the pool does not hold"),
            "{}",
            rendered(&report)
        );
    }

    /// The property that makes this a measure of collection rather than of activity: a
    /// page collected every week for a year is one document, not fifty-two.
    #[tokio::test]
    async fn re_collecting_an_address_does_not_grow_the_count() {
        let (_dir, ctx) = store_with(&[
            ("tampa", "https://tampa.gov/a", "text/html"),
            ("tampa", "https://tampa.gov/a", "text/html"),
            ("tampa", "https://tampa.gov/a", "text/html"),
        ])
        .await;

        let report = status(&ctx, StatusArgs::default()).await.unwrap();
        assert_eq!(report.total, 1);
        assert_eq!(report.sources[0].kinds["html"], 1);
    }

    /// The 2.2 GB case, counted. IIS serves `.csv` as `application/octet-stream`, and a
    /// census that believed the header would report the largest category on that server
    /// as `other`.
    #[tokio::test]
    async fn a_mislabelled_document_is_counted_as_what_its_address_names() {
        let (_dir, ctx) = store_with(&[(
            "publicrec",
            "https://publicrec.hillsclerk.com/Civil/undisposed/CivilUndisposed.csv",
            "application/octet-stream",
        )])
        .await;

        let report = status(&ctx, StatusArgs::default()).await.unwrap();
        assert_eq!(report.kinds["csv"], 1);
    }

    #[tokio::test]
    async fn one_source_can_be_asked_on_its_own() {
        let (_dir, ctx) = store_with(&[
            ("tampa", "https://tampa.gov/a", "text/html"),
            ("hillsclerk", "https://hillsclerk.com/x", "text/html"),
        ])
        .await;

        let report = status(
            &ctx,
            StatusArgs {
                source: Some("tampa".into()),
            },
        )
        .await
        .unwrap();

        assert_eq!(report.sources.len(), 1);
        assert_eq!(report.total, 1);
        // One row, so the totals line would only repeat it.
        assert!(!rendered(&report).contains("all"));
    }

    #[tokio::test]
    async fn an_empty_store_says_so_and_names_the_command_that_fills_it() {
        let (_dir, ctx) = store_with(&[]).await;
        let report = status(&ctx, StatusArgs::default()).await.unwrap();

        assert_eq!(report.total, 0);
        assert!(report.sources.is_empty());
        let out = rendered(&report);
        assert!(out.contains("No sources yet"), "{out}");
        assert!(out.contains("centinel discover"), "{out}");
    }

    /// Only the kinds present, in the vocabulary's order — and a word from a newer build
    /// still gets a column rather than being dropped from a count of what was collected.
    #[test]
    fn the_columns_are_the_kinds_this_corpus_holds() {
        let report = StatusReport {
            store_root: "/tmp/store".into(),
            sources: vec![],
            kinds: BTreeMap::from([
                ("pdf".to_string(), 3),
                ("html".to_string(), 9),
                ("hologram".to_string(), 1),
            ]),
            total: 13,
            bytes: 4096,
            missing_blobs: 0,
        };

        assert_eq!(report.columns(), vec!["html", "pdf", "hologram"]);
    }
}
