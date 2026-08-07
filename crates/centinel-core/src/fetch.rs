//! HTTP fetching, shared by every op that pulls bytes.
//!
//! One code path on purpose. `ingest` and `collect` must classify a WAF 403 identically
//! and record identical transport metadata, or the archive's provenance depends on which
//! command happened to be used — a difference no consumer could see or correct for.

use std::collections::BTreeMap;

use crate::domain::{Fetched, Liveness};
use crate::policy::HostPolicy;

/// A fetch that failed.
///
/// The name is kept for the call sites; the type is [`crate::domain::Refusal`], which is
/// what `yt-dlp` failures are too. These were written twice and were always the same
/// thing — `{state, detail}`, each with its own `classify` — and one shared acquisition
/// loop cannot exist while a refusal has two types.
pub use crate::domain::Refusal as FetchFailure;

/// An HTTP client configured by [`HostPolicy`].
#[derive(Clone, Debug)]
pub struct Fetcher {
    client: reqwest::Client,
}

impl Fetcher {
    pub fn new(policy: &HostPolicy) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .user_agent(&policy.user_agent)
                .timeout(policy.timeout)
                .build()?,
        })
    }

    /// GETs a URL, classifying any non-success status into a [`Liveness`].
    pub async fn get(&self, url: &str) -> Result<Fetched, FetchFailure> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| FetchFailure {
                state: Liveness::Error,
                detail: e.to_string(),
            })?;

        let status = resp.status();
        if !status.is_success() {
            return Err(FetchFailure {
                state: classify(status.as_u16()),
                detail: format!("HTTP {status}"),
            });
        }

        // Captured because they are the cheap conditional-request signals a later pass
        // will want, and because they cannot be recovered after the fact.
        let mut meta = BTreeMap::new();
        for header in ["content-type", "etag", "last-modified"] {
            if let Some(v) = resp.headers().get(header)
                && let Ok(s) = v.to_str()
            {
                meta.insert(header.to_string(), s.to_string());
            }
        }
        meta.insert("http_status".into(), status.as_u16().to_string());
        // The post-redirect URL — where the bytes actually came from.
        meta.insert("final_url".into(), resp.url().to_string());

        let bytes = resp.bytes().await.map_err(|e| FetchFailure {
            state: Liveness::Error,
            detail: format!("body read failed: {e}"),
        })?;

        Ok(Fetched {
            bytes: bytes.to_vec(),
            meta,
        })
    }
}

/// Maps an HTTP status onto liveness.
///
/// The 403 → [`Liveness::Blocked`] mapping is the load-bearing one. Both `phila.gov` and
/// `sec.gov` were measured returning WAF 403s with no `Retry-After`; classifying those as
/// `Gone` would record a live page as deleted.
pub fn classify(status: u16) -> Liveness {
    match status {
        404 | 410 => Liveness::Gone,
        401 | 403 | 429 => Liveness::Blocked,
        _ => Liveness::Error,
    }
}

/// How many leading bytes [`content_kind`] can need.
///
/// Sized by the deepest sniff it performs — the `json3` marker scan. A caller holding
/// this much can classify a blob without reading the whole thing, which is the difference
/// between building a transcription work list and reading the entire corpus to build one.
pub const SNIFF_BYTES: usize = crate::captions::SNIFF_BYTES;

/// The [MS-CFB] signature every legacy Office file opens with.
const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// A coarse content kind, from the `content-type` header with a magic-byte fallback.
///
/// Deliberately coarse: acquisition should not hold opinions about formats. This exists
/// so `collect` can *report* what it gathered — knowing a run pulled 400 PDFs is what
/// makes the extraction stage plannable.
pub fn content_kind(meta: &BTreeMap<String, String>, bytes: &[u8]) -> &'static str {
    let declared = meta
        .get("content-type")
        .map(|s| {
            s.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        })
        .unwrap_or_default();

    match declared.as_str() {
        "text/html" | "application/xhtml+xml" => return "html",
        "application/pdf" => return "pdf",
        "text/plain" => return "text",
        // A caption track is served as ordinary JSON, so the declared type cannot tell
        // it apart from a vendor API response. Sniffed rather than trusted to the
        // `content-type` we wrote — which also means blobs collected before this
        // existed are recognised on the next `extract`.
        "application/json" => {
            return if crate::captions::looks_like_json3(bytes) {
                "captions"
            } else {
                "json"
            };
        }
        "text/xml" | "application/xml" => return "xml",
        "text/csv" => return "csv",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        | "application/vnd.ms-excel"
        | "application/vnd.oasis.opendocument.spreadsheet" => return "spreadsheet",
        // One word for eight formats, because extraction asks one question of all of
        // them and `anydoc` answers it. Which of the eight this is gets decided from the
        // whole bytes at extraction time, not from a header a server filled in by guess.
        "application/msword"
        | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        | "application/vnd.ms-powerpoint"
        | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        | "application/vnd.oasis.opendocument.text"
        | "application/vnd.oasis.opendocument.presentation"
        | "application/rtf"
        | "text/rtf"
        | "application/epub+zip" => {
            return "document";
        }
        _ => {}
    }

    if declared.starts_with("audio/") {
        return "audio";
    }

    // Hosts mislabel constantly — .gov servers routinely serve PDFs as
    // application/octet-stream. Magic bytes are the tiebreak.
    if bytes.starts_with(b"%PDF-") {
        return "pdf";
    }
    // The two document signatures that fit in a head. An OLE compound file is a legacy
    // `.doc`, `.ppt` or `.xls`, and which one is written in a directory sector that can
    // sit anywhere in the file — so this says `document` and extraction, holding the
    // whole blob, sorts the spreadsheets back out.
    if bytes.starts_with(b"{\\rtf") || bytes.starts_with(&OLE_MAGIC) {
        return "document";
    }
    // ZIP magic: xlsx/docx are zip containers. Which one is in the central directory at
    // the *end* of the file, which a head read cannot reach, so this is as far as
    // classification gets and `extract` finishes the job.
    if bytes.starts_with(b"PK\x03\x04") {
        return "zip-container";
    }
    if is_audio(bytes) {
        return "audio";
    }
    let head = &bytes[..bytes.len().min(256)];
    let head = String::from_utf8_lossy(head)
        .trim_start()
        .to_ascii_lowercase();
    if head.starts_with("<!doctype html") || head.starts_with("<html") {
        return "html";
    }
    "other"
}

/// The `content-type` a server would have declared for a file with this name.
///
/// A file read off disk arrives with no headers, so [`content_kind`] has only magic bytes
/// to go on — and for the formats whose first bytes are indistinguishable from plain text
/// that is not enough. A `.csv` sniffs to `other` and no extractor claims it, even though
/// the very same bytes are read fine the moment a server calls them `text/csv`. Left
/// alone, a tool for checking what the pipeline does with a local file would report a
/// failure that belongs to the file having no headers rather than to the pipeline.
///
/// Lives beside [`content_kind`] because it is the same table read backwards: an
/// extension here that maps to a type `content_kind` does not recognise is a silent
/// no-op, and the two drifting apart is only visible from one place if they are written
/// in one place.
///
/// Deliberately **not** consulted for anything fetched. A server's own header is
/// evidence; a filename is a guess, and a guess belongs only where there is nothing else.
pub fn declared_type_for_path(path: &std::path::Path) -> Option<&'static str> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();

    Some(match ext.as_str() {
        "html" | "htm" | "xhtml" => "text/html",
        "pdf" => "application/pdf",
        "txt" | "md" | "markdown" => "text/plain",
        "json" => "application/json",
        "xml" => "application/xml",
        "csv" => "text/csv",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xls" => "application/vnd.ms-excel",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "rtf" => "application/rtf",
        "epub" => "application/epub+zip",
        "mp3" | "m4a" | "wav" | "ogg" | "opus" | "webm" => "audio/mpeg",
        _ => return None,
    })
}

/// Container sniffing for the audio YouTube actually serves.
///
/// Magic bytes rather than the declared type, because blobs are content-addressed and a
/// blob path has no extension — this is how `transcribe` finds its work list, and how
/// ffmpeg's job is confirmed before it is asked to do it.
fn is_audio(bytes: &[u8]) -> bool {
    audio_container(bytes).is_some()
}

/// Which container, as a file extension — the same sniff asked a more specific question.
///
/// `content_kind` answers with one word, `audio`, because that is the distinction
/// `transcribe` needs. Naming a file needs the next one down: every player refuses a
/// WebM called `.m4a`, so `open` cannot round five containers to one extension.
///
/// `None` means the bytes are not audio at all, which is a different answer from "audio
/// of a container I do not recognise" — the caller decides what to do with each.
pub fn audio_container(bytes: &[u8]) -> Option<&'static str> {
    // ISO base media (m4a): `....ftyp` — the size field comes first.
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        // `ftypmp42`/`ftypM4A ` are audio; `ftypisom` is usually video, but a
        // video-carrying blob is still something ffmpeg can pull an audio track from.
        return Some("m4a");
    }
    // Matroska / WebM.
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some("webm");
    }
    // RIFF....WAVE
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
        return Some("wav");
    }
    // Ogg (Opus/Vorbis).
    if bytes.starts_with(b"OggS") {
        return Some("ogg");
    }
    // MP3: an ID3 tag, or a bare frame sync.
    if bytes.starts_with(b"ID3") {
        return Some("mp3");
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0 {
        return Some("mp3");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(ct: &str) -> BTreeMap<String, String> {
        BTreeMap::from([("content-type".to_string(), ct.to_string())])
    }

    /// The drift guard the two tables exist to need.
    ///
    /// [`declared_type_for_path`] is only useful if every type it names is one
    /// [`content_kind`] recognises; an extension mapped to a string the classifier has
    /// never heard of is a silent no-op that looks like support.
    #[test]
    fn every_inferred_type_is_one_the_classifier_knows() {
        for name in [
            "a.html",
            "a.htm",
            "a.xhtml",
            "a.pdf",
            "a.txt",
            "a.md",
            "a.markdown",
            "a.json",
            "a.xml",
            "a.csv",
            "a.xlsx",
            "a.xls",
            "a.ods",
            "a.doc",
            "a.docx",
            "a.ppt",
            "a.pptx",
            "a.odt",
            "a.odp",
            "a.rtf",
            "a.epub",
            "a.mp3",
            "a.m4a",
            "a.wav",
            "a.ogg",
            "a.opus",
            "a.webm",
        ] {
            let declared = declared_type_for_path(std::path::Path::new(name))
                .unwrap_or_else(|| panic!("{name} has no inferred type"));
            assert_ne!(
                content_kind(&meta(declared), b""),
                "other",
                "{name} infers `{declared}`, which the classifier does not recognise"
            );
        }
    }

    /// A `.csv` is the case this exists for: its first bytes are ordinary text, so magic
    /// bytes alone reach `other` and no extractor claims it — while the identical bytes
    /// are read fine the moment a server calls them `text/csv`.
    #[test]
    fn an_extension_answers_where_the_bytes_cannot() {
        let bytes = b"district,population\nEast,41200\n";
        assert_eq!(content_kind(&BTreeMap::new(), bytes), "other");

        let declared = declared_type_for_path(std::path::Path::new("districts.csv")).unwrap();
        assert_eq!(content_kind(&meta(declared), bytes), "csv");
    }

    /// Nothing to infer from is not the same as inferring nothing, and a caller has to be
    /// able to tell — a file with no extension gets no header rather than a wrong one.
    #[test]
    fn an_unknown_extension_infers_nothing() {
        assert_eq!(declared_type_for_path(std::path::Path::new("blob")), None);
        assert_eq!(
            declared_type_for_path(std::path::Path::new("archive.dwg")),
            None
        );
        // Case is a filename's business, not a format's.
        assert_eq!(
            declared_type_for_path(std::path::Path::new("REPORT.PDF")),
            Some("application/pdf")
        );
    }

    #[test]
    fn waf_403_is_blocked_not_gone() {
        assert_eq!(classify(403), Liveness::Blocked);
        assert_eq!(classify(429), Liveness::Blocked);
        assert_eq!(classify(401), Liveness::Blocked);
        assert_eq!(classify(404), Liveness::Gone);
        assert_eq!(classify(410), Liveness::Gone);
        assert_eq!(classify(500), Liveness::Error);
        assert_eq!(classify(503), Liveness::Error);
    }

    #[test]
    fn content_type_header_is_used_when_present() {
        assert_eq!(content_kind(&meta("text/html; charset=utf-8"), b""), "html");
        assert_eq!(content_kind(&meta("application/pdf"), b""), "pdf");
        assert_eq!(
            content_kind(
                &meta("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
                b""
            ),
            "spreadsheet"
        );
    }

    #[test]
    fn magic_bytes_override_a_useless_content_type() {
        // .gov servers routinely serve PDFs as octet-stream.
        assert_eq!(
            content_kind(&meta("application/octet-stream"), b"%PDF-1.7\n..."),
            "pdf"
        );
        assert_eq!(
            content_kind(&meta("application/octet-stream"), b"<!DOCTYPE html><html>"),
            "html"
        );
        assert_eq!(
            content_kind(&meta("application/octet-stream"), b"PK\x03\x04junk"),
            "zip-container"
        );
        // The two document containers whose identity fits in a head.
        assert_eq!(
            content_kind(&meta("application/octet-stream"), br"{\rtf1\ansi"),
            "document"
        );
        assert_eq!(
            content_kind(&meta("application/octet-stream"), &OLE_MAGIC),
            "document"
        );
    }

    /// Every office type a `.gov` server labels correctly reaches one of two words.
    /// A type with no arm falls through to `other` and the blob is never read.
    #[test]
    fn every_office_content_type_reaches_an_extractor() {
        for ct in [
            "application/msword",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "application/vnd.ms-powerpoint",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "application/vnd.oasis.opendocument.text",
            "application/vnd.oasis.opendocument.presentation",
            "application/rtf",
            "text/rtf",
            "application/epub+zip",
        ] {
            assert_eq!(content_kind(&meta(ct), b""), "document", "{ct}");
        }
        for ct in [
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "application/vnd.ms-excel",
            "application/vnd.oasis.opendocument.spreadsheet",
        ] {
            assert_eq!(content_kind(&meta(ct), b""), "spreadsheet", "{ct}");
        }
    }

    #[test]
    fn unknown_content_is_labelled_other_not_guessed() {
        assert_eq!(content_kind(&BTreeMap::new(), b"\x00\x01\x02\x03"), "other");
    }

    #[test]
    fn content_kind_does_not_panic_on_short_bodies() {
        assert_eq!(content_kind(&BTreeMap::new(), b""), "other");
        assert_eq!(content_kind(&BTreeMap::new(), b"%"), "other");
    }
}
