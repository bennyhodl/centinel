//! `download` — return a collected document's bytes, for saving somewhere else.
//!
//! The third retrieval verb. `read` returns extracted text and `open` launches an
//! application on this host; neither moves the document itself. An agent on the far end
//! of MCP that has found a budget PDF wants the PDF — to save it, attach it, hand it to
//! its own tooling — and the only channel a tool result offers is JSON, which has no
//! bytes. So the bytes travel base64-encoded in the report.
//!
//! **Paged, like `read`.** A tool result travels through a model's context and nearly
//! every client caps one, so an unbounded default would make the commonest document the
//! one that cannot be fetched. `offset`/`max_bytes` slice the *raw* bytes; a caller
//! decodes each page and concatenates in offset order, and `data_sha` is the hash the
//! reassembly must come out to. A caller whose transport has no such cap — `POST
//! /ops/download` from a script — passes `max_bytes: 0` and gets the whole document in
//! one answer.
//!
//! **The original is the default.** The bytes as served are the document; the extraction
//! is a lossy summary of them, and `read` is the verb for it. A target that names a
//! *derived* hash gets that blob, because anything Centinel prints, Centinel takes back
//! — a hash that resolved to the other half would make the round trip a lie.
//!
//! Safe to expose remotely for the same reason `read` is: it reads the store and runs
//! nothing.

use base64::Engine as _;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::content::ContentKind;
use crate::materialize;
use crate::ops::target::resolve;
use crate::prelude::*;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct DownloadArgs {
    /// A URL, a substring of one, or a blob hash — the same targets `search` reports,
    /// including the short hash it prints.
    #[arg(value_name = "TARGET")]
    pub target: String,

    /// Maximum raw bytes per call. 0 returns the whole document.
    ///
    /// Defaulted small because the encoded bytes ride a tool result through a model's
    /// context, and most MCP clients cap one — an oversized answer is refused whole,
    /// which reads as a document that cannot be fetched at all. Page with `offset`,
    /// or pass 0 on a transport without the cap.
    #[arg(long, default_value_t = 32_768)]
    #[serde(default = "default_max_bytes")]
    pub max_bytes: usize,

    /// Start at this byte offset, for paging through a large document.
    #[arg(long, default_value_t = 0)]
    #[serde(default)]
    pub offset: usize,

    /// Restrict the search for `target` to one source.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,
}

fn default_max_bytes() -> usize {
    32_768
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DownloadReport {
    pub url: String,
    pub source: String,
    /// The kind of the bytes in `data` — `markdown` when a derived blob was named.
    pub kind: String,
    /// SHA-256 of the original bytes as served — the evidentiary anchor.
    pub blob_sha: String,
    /// SHA-256 of the whole document `data` is a slice of. Equal to `blob_sha` unless
    /// this is derived text — and the hash a reassembly of every page must come out to.
    pub data_sha: String,
    /// True when `target` named a derived blob rather than the bytes as served.
    pub derived: bool,
    /// A name to save the file under, with a real extension — the same name
    /// `current/` would give it, so the bytes arrive wearing a name their handler
    /// recognises.
    pub filename: String,
    /// The `content-type` the server declared, verbatim. Absent when it declared none,
    /// and always absent for derived text — no server ever served that, and a guess
    /// read off a filename must not wear a header's clothes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    pub observed_at: String,
    /// How `data` is encoded. Always `base64`, stated so the report explains itself.
    pub encoding: String,
    /// The bytes, base64-encoded — this page of them, when the document is larger
    /// than `max_bytes`.
    pub data: String,
    /// Raw bytes in this page.
    pub bytes: usize,
    /// Raw bytes in the whole document, so a caller knows whether to page.
    pub total_bytes: usize,
    /// The byte offset `data` starts at.
    pub offset: usize,
    /// True when `offset + bytes < total_bytes`.
    pub truncated: bool,
    /// Other matches for an ambiguous target. The first was used.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_matches: Vec<String>,
}

/// Download a collected document's bytes, base64-encoded, for saving elsewhere.
#[op(group = "corpus")]
pub async fn download(ctx: &Ctx, args: DownloadArgs) -> anyhow::Result<DownloadReport> {
    let found = resolve(ctx, &args.target, args.source.as_deref()).await?;
    let (source, resource, obs) = (found.source, found.resource, found.observation);
    let other_matches = found.other_matches;

    // A target that named a derived blob gets that blob. Everything else gets the
    // original — `read` already serves the caller who wants text.
    let (data_sha, derived) = match found.matched_derived {
        Some(sha) => (sha, true),
        None => (obs.blob_sha.clone(), false),
    };

    // The whole blob, verified against its address — these bytes leave the machine and
    // will be saved as the document, which is exactly the case `blob_head` is not for.
    let whole = ctx.store.get_blob(&data_sha).await?;
    let total_bytes = whole.len();

    let kind = if derived {
        ContentKind::Markdown
    } else {
        ContentKind::classify(&obs.meta, &whole)
    };

    // Named by the one place that knows how to name a blob, so `download` and
    // `open` cannot disagree about what this file is called.
    let head = &whole[..whole.len().min(16)];
    let filename = materialize::relative_path(&resource.natural_key, kind, head)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .expect("relative_path always ends in a file name");

    let media_type = if derived {
        None
    } else {
        obs.meta.get("content-type").cloned()
    };

    let start = args.offset.min(total_bytes);
    let end = if args.max_bytes == 0 {
        total_bytes
    } else {
        total_bytes.min(start.saturating_add(args.max_bytes))
    };
    let page = &whole[start..end];

    Ok(DownloadReport {
        url: resource.natural_key,
        source: source.to_string(),
        kind: kind.to_string(),
        blob_sha: obs.blob_sha.to_string(),
        data_sha: data_sha.to_string(),
        derived,
        filename,
        media_type,
        observed_at: obs.at.to_string(),
        encoding: "base64".to_string(),
        data: base64::engine::general_purpose::STANDARD.encode(page),
        bytes: page.len(),
        total_bytes,
        offset: start,
        truncated: start + page.len() < total_bytes,
        other_matches,
    })
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// The provenance and the name — never the payload.
///
/// Base64 on a terminal is noise to a person, and the person at a terminal already has
/// the better verb: `open --print-path` puts these same bytes on this machine's disk
/// under this same name. What the CLI form is for is checking what a remote caller
/// would get, so it prints everything about the bytes except the bytes.
impl Render for DownloadReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.title(&render::truncate(&self.url, p.width()), "")?;

        let hash = p.paint(&render::short_sha(&self.blob_sha), Ink::Cyan);
        let mut provenance = vec![self.source.clone(), self.kind.clone()];
        if let Some(mt) = &self.media_type {
            provenance.push(mt.clone());
        }
        provenance.push(render::short_time(&self.observed_at));
        p.line(format!(
            "{hash} · {}",
            p.paint(&provenance.join(" · "), Ink::Dim)
        ))?;

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
        p.line(format!(
            "{} · {} · base64 in `data`",
            self.filename,
            render::bytes(self.total_bytes as u64),
        ))?;

        // Where the caller is in the document, and how to get the rest — the same
        // footer `read` prints, in bytes rather than characters.
        if self.truncated {
            p.blank()?;
            let position = format!(
                "{}–{} of {} bytes",
                render::count(self.offset as u64),
                render::count((self.offset + self.bytes) as u64),
                render::count(self.total_bytes as u64),
            );
            p.line(p.paint(&position, Ink::Dim))?;
            let next = format!("--offset {}", self.offset + self.bytes);
            p.line(p.paint(&next, Ink::Cyan))?;
        }

        p.blank()?;
        let local = format!(
            "on this machine: centinel open {} --print-path",
            render::short_sha(&self.data_sha),
        );
        p.line(p.paint(&local, Ink::Dim))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Derivation;
    use crate::store::{LogRecord, Store};
    use std::collections::BTreeMap;

    const PDF: &[u8] = b"%PDF-1.7 pretend agenda bytes";
    const TEXT: &str = "# Agenda\n\nItem one. Item two.";

    fn decode(data: &str) -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .expect("`data` must always be valid base64")
    }

    /// A store holding one PDF, served with a declared type, and its extracted text.
    ///
    /// Returns `(ctx, original sha, derived sha)` — the two hashes the retrieval
    /// commands print between them.
    async fn corpus(dir: &std::path::Path) -> (Ctx, BlobSha, BlobSha) {
        let store = Store::open(dir.join("store")).await.unwrap();
        let id = SourceId::new("tampa").unwrap();

        let mut meta = BTreeMap::new();
        meta.insert("content-type".to_string(), "application/pdf".to_string());
        let obs = store
            .record_observation(
                &Resource::new(id.clone(), "https://tampa.gov/agenda.pdf"),
                PDF,
                jiff::Timestamp::now(),
                meta,
            )
            .await
            .unwrap();

        let derived = store.put_blob(TEXT.as_bytes()).await.unwrap();
        store
            .append(
                &id,
                &LogRecord::Derivation(Derivation {
                    from_sha: obs.blob_sha.clone(),
                    to_sha: derived.clone(),
                    tool: "pdf-inspector".into(),
                    version: "0.1".into(),
                    model_tier: None,
                    at: jiff::Timestamp::now(),
                    anchors: Vec::new(),
                }),
            )
            .await
            .unwrap();

        (Ctx::new(store), obs.blob_sha, derived)
    }

    fn args(target: &str) -> DownloadArgs {
        DownloadArgs {
            target: target.into(),
            max_bytes: 32_768,
            offset: 0,
            source: None,
        }
    }

    #[tokio::test]
    async fn downloading_returns_the_bytes_as_served() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, original, _) = corpus(dir.path()).await;

        let r = download(&ctx, args("agenda.pdf")).await.unwrap();
        assert_eq!(decode(&r.data), PDF, "the bytes must survive the trip");
        assert_eq!(r.kind, "pdf");
        assert_eq!(r.filename, "agenda.pdf");
        assert_eq!(r.encoding, "base64");
        assert_eq!(r.blob_sha, original.to_string());
        assert_eq!(
            r.data_sha, r.blob_sha,
            "an original download is anchored to itself"
        );
        assert!(!r.derived);
        assert!(!r.truncated);
    }

    /// The header is evidence and travels verbatim; where none was declared, none is
    /// invented — a filename's opinion must not wear a header's clothes.
    #[tokio::test]
    async fn the_media_type_is_the_served_header_or_absent() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _, _) = corpus(dir.path()).await;

        let r = download(&ctx, args("agenda.pdf")).await.unwrap();
        assert_eq!(r.media_type.as_deref(), Some("application/pdf"));

        let store = Store::open(dir.path().join("bare")).await.unwrap();
        store
            .record_observation(
                &Resource::new(SourceId::new("bare").unwrap(), "https://x.gov/y.pdf"),
                PDF,
                jiff::Timestamp::now(),
                Default::default(),
            )
            .await
            .unwrap();
        let r = download(&Ctx::new(store), args("y.pdf")).await.unwrap();
        assert_eq!(r.media_type, None, "no header, no claim");
        assert_eq!(r.kind, "pdf", "the magic bytes still name the kind");
    }

    /// A derived hash gets the derived bytes — anything Centinel prints, Centinel
    /// takes back, and it hands over the thing the hash identified.
    #[tokio::test]
    async fn a_derived_hash_downloads_the_derived_text() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, original, derived) = corpus(dir.path()).await;

        let r = download(&ctx, args(&derived.as_str()[..12])).await.unwrap();
        assert_eq!(decode(&r.data), TEXT.as_bytes());
        assert!(r.derived);
        assert_eq!(r.kind, "markdown");
        assert_eq!(r.data_sha, derived.to_string());
        assert_eq!(
            r.blob_sha,
            original.to_string(),
            "the evidentiary anchor still names the bytes as served"
        );
        assert_eq!(r.media_type, None, "no server ever served derived text");
        assert!(
            r.filename.ends_with(".md"),
            "named as the markdown it is: {}",
            r.filename
        );
    }

    /// Both hashes the report carries are targets `download` itself accepts.
    #[tokio::test]
    async fn every_hash_download_reports_can_be_typed_back() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _, _) = corpus(dir.path()).await;
        let first = download(&ctx, args("agenda.pdf")).await.unwrap();

        let again = download(&ctx, args(&first.blob_sha[..12])).await.unwrap();
        assert_eq!(decode(&again.data), PDF);

        let derived = download(&ctx, args(&first.data_sha[..12])).await.unwrap();
        assert_eq!(decode(&derived.data), PDF, "equal hashes, equal bytes");
    }

    /// Pages decode and concatenate to exactly the document, and the footer numbers
    /// tell the caller where it is — the property reassembly stands on.
    #[tokio::test]
    async fn paging_reassembles_to_the_exact_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _, _) = corpus(dir.path()).await;

        let head = download(
            &ctx,
            DownloadArgs {
                max_bytes: 8,
                ..args("agenda.pdf")
            },
        )
        .await
        .unwrap();
        assert_eq!(head.bytes, 8);
        assert_eq!(head.total_bytes, PDF.len());
        assert!(head.truncated);

        // 0 means everything, from wherever the caller left off.
        let tail = download(
            &ctx,
            DownloadArgs {
                offset: 8,
                max_bytes: 0,
                ..args("agenda.pdf")
            },
        )
        .await
        .unwrap();
        assert!(!tail.truncated);

        let mut whole = decode(&head.data);
        whole.extend(decode(&tail.data));
        assert_eq!(whole, PDF);
    }

    /// An offset past the end is empty and final, not an error — the shape a paging
    /// loop that overshoots by one page lands on.
    #[tokio::test]
    async fn an_offset_past_the_end_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _, _) = corpus(dir.path()).await;

        let r = download(
            &ctx,
            DownloadArgs {
                offset: PDF.len() + 100,
                ..args("agenda.pdf")
            },
        )
        .await
        .unwrap();
        assert_eq!(r.bytes, 0);
        assert!(!r.truncated);
    }

    // ── rendering ──────────────────────────────────────────────────────────────

    fn render_to_string(report: &DownloadReport) -> String {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    fn report() -> DownloadReport {
        DownloadReport {
            url: "https://tampa.gov/agenda.pdf".into(),
            source: "tampa".into(),
            kind: "pdf".into(),
            blob_sha: "3f8a1c9d0b7e".repeat(6)[..64].to_string(),
            data_sha: "3f8a1c9d0b7e".repeat(6)[..64].to_string(),
            derived: false,
            filename: "agenda.pdf".into(),
            media_type: Some("application/pdf".into()),
            observed_at: "2026-08-04T10:00:00Z".into(),
            encoding: "base64".into(),
            data: base64::engine::general_purpose::STANDARD.encode(PDF),
            bytes: 8,
            total_bytes: 4_100_000,
            offset: 0,
            truncated: true,
            other_matches: Vec::new(),
        }
    }

    /// The header leads with the handle, the footer says how to get the rest, and the
    /// payload never reaches the terminal.
    #[test]
    fn the_render_carries_the_handle_and_never_the_payload() {
        let r = report();
        let out = render_to_string(&r);
        assert!(out.contains("3f8a1c9d0b7e"), "{out}");
        assert!(out.contains("agenda.pdf"), "{out}");
        assert!(out.contains("--offset 8"), "{out}");
        assert!(
            !out.contains(&r.data),
            "base64 is for the wire, not the terminal"
        );
    }

    /// A complete document needs no paging instructions.
    #[test]
    fn an_untruncated_download_offers_no_paging() {
        let mut r = report();
        r.truncated = false;
        assert!(!render_to_string(&r).contains("--offset"));
    }

    /// The guard the erased render path stands on: the report must parse from its own
    /// serialized form, or the CLI fails after the work is done.
    #[test]
    fn the_report_round_trips_through_json() {
        let r = report();
        let json = serde_json::to_value(&r).unwrap();
        let back: DownloadReport = serde_json::from_value(json).unwrap();
        assert_eq!(back.data, r.data);
        assert_eq!(back.data_sha, r.data_sha);
        assert_eq!(back.total_bytes, r.total_bytes);
    }
}
