//! **Content kind**: one word for what a blob *is*, and every projection off it.
//!
//! Deliberately coarser than a format. `document` covers Word, PowerPoint, OpenDocument,
//! RTF and EPUB, because extraction asks all five the same question. It is decided from a
//! 4 KB head, so it can only ever answer what the first bytes prove: a `.docx` and a
//! `.pptx` are both `zip-container` until something reads the ZIP central directory at the
//! *end* of the file. The precise format is a **different question, answered later**, by
//! `extract_document`, which holds the whole verified blob.
//!
//! ## Why this is one module
//!
//! The vocabulary answers four questions — *what is this?*, *what would a server have
//! called it?*, *what should the file be named?*, *is it worth fetching on its own?* — and
//! they were answered by five tables that no compiler related to each other:
//!
//! | | |
//! |---|---|
//! | `fetch` | content-type → kind, magic → kind, extension → content-type |
//! | `materialize` | kind → extension, magic → extension |
//! | `enclosure` | a thirteen-extension allowlist |
//!
//! Adding one kind meant ten edits and the compiler asked for none of them. Miss the
//! `materialize` arm and the kind lands in `current/` as `.bin`, which is the gap that
//! shipped for `captions`; miss the `enclosure` arm and the document at the end of the
//! link is never fetched at all. Both failures are silent, and both look exactly like a
//! site that had nothing.
//!
//! Now there is [`FORMATS`] — one row per (extension, content-type) pair — and everything
//! else reads off it. A new kind is a variant, and every projection that must grow stops
//! compiling until it does.

use std::collections::BTreeMap;
use std::path::Path;

/// How many leading bytes [`ContentKind::classify`] can need.
///
/// Sized by the deepest sniff it performs — the `json3` marker scan. A caller holding
/// this much can classify a blob without reading the whole thing, which is the difference
/// between building a transcription work list and reading the entire corpus to build one.
pub const SNIFF_BYTES: usize = crate::captions::SNIFF_BYTES;

/// The [MS-CFB] signature every legacy Office file opens with.
const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// One word for what a blob is.
///
/// [`ContentKind::Other`] is an answer, not a failure: it says the bytes were looked at
/// and matched nothing, which is different from not having looked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContentKind {
    Html,
    Pdf,
    Text,
    /// Produced by extraction, never served. Has no content-type here because no server
    /// ever declares one for it — see [`FORMATS`]'s note on `mime: None`.
    Markdown,
    /// A YouTube `json3` track. Served as ordinary JSON, so it is *sniffed* rather than
    /// trusted to a content-type — which also means blobs collected before this kind
    /// existed are recognised on the next `extract`.
    Captions,
    Json,
    Xml,
    Csv,
    Spreadsheet,
    /// Word, PowerPoint, OpenDocument, RTF and EPUB. One word for eight formats, because
    /// `anydoc` asks all eight the same question.
    Document,
    /// A ZIP whose payload is undecided. `xlsx` and `docx` are both this until the central
    /// directory at the end of the file is read, which a head read cannot reach.
    ZipContainer,
    Audio,
    Other,
}

use ContentKind::*;

/// One (extension, content-type) pair, and the kind both of them name.
///
/// **Order is load-bearing, twice.** The first row for a kind gives that kind its
/// canonical extension — the one [`ContentKind::extension`] writes when nothing more
/// specific is known — so `docx` precedes `doc` and `m4a` precedes `mp3`. And an
/// extension claimed by two kinds goes to whichever appears first, which is how a `.md`
/// file on disk reads as [`Text`] (what a server would call it) while [`Markdown`] still
/// materialises as `.md`.
///
/// `mime: None` means no server declares this — it is an extension we write, not one we
/// are ever told. Such a row is skipped by [`ContentKind::declared_type_for_path`], so a
/// local `.zip` still infers nothing rather than inferring something unrecognised.
struct Format {
    ext: &'static str,
    mime: Option<&'static str>,
    kind: ContentKind,
}

const fn f(ext: &'static str, mime: &'static str, kind: ContentKind) -> Format {
    Format {
        ext,
        mime: Some(mime),
        kind,
    }
}

const fn written(ext: &'static str, kind: ContentKind) -> Format {
    Format {
        ext,
        mime: None,
        kind,
    }
}

/// The table. Everything in this module is a projection of it.
const FORMATS: &[Format] = &[
    f("html", "text/html", Html),
    f("htm", "text/html", Html),
    f("xhtml", "text/html", Html),
    f("pdf", "application/pdf", Pdf),
    f("txt", "text/plain", Text),
    // A `.md` file on disk is plain text as far as any server is concerned; the `Markdown`
    // kind below is what *we* write, and it claims the same extension one row later.
    f("md", "text/plain", Text),
    f("markdown", "text/plain", Text),
    written("md", Markdown),
    f("json", "application/json", Json),
    f("xml", "application/xml", Xml),
    f("csv", "text/csv", Csv),
    f(
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Spreadsheet,
    ),
    f("xls", "application/vnd.ms-excel", Spreadsheet),
    f(
        "ods",
        "application/vnd.oasis.opendocument.spreadsheet",
        Spreadsheet,
    ),
    f(
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Document,
    ),
    f("doc", "application/msword", Document),
    f("ppt", "application/vnd.ms-powerpoint", Document),
    f(
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        Document,
    ),
    f("odt", "application/vnd.oasis.opendocument.text", Document),
    f(
        "odp",
        "application/vnd.oasis.opendocument.presentation",
        Document,
    ),
    f("rtf", "application/rtf", Document),
    f("epub", "application/epub+zip", Document),
    f("m4a", "audio/mpeg", Audio),
    f("mp3", "audio/mpeg", Audio),
    f("wav", "audio/mpeg", Audio),
    f("ogg", "audio/mpeg", Audio),
    f("opus", "audio/mpeg", Audio),
    f("webm", "audio/mpeg", Audio),
    written("zip", ZipContainer),
];

/// Content-types that name a kind but no extension of ours.
///
/// Aliases, in other words. A server may say `text/xml` where we would write `.xml`, and
/// `application/xhtml+xml` where we would write `.html`; neither earns a row in
/// [`FORMATS`] because neither is an extension this codebase ever writes.
const ALIASES: &[(&str, ContentKind)] = &[
    ("application/xhtml+xml", Html),
    ("text/xml", Xml),
    ("text/rtf", Document),
];

impl ContentKind {
    /// Every kind, for tests that must not silently stop covering one.
    pub const ALL: &'static [ContentKind] = &[
        Html,
        Pdf,
        Text,
        Markdown,
        Captions,
        Json,
        Xml,
        Csv,
        Spreadsheet,
        Document,
        ZipContainer,
        Audio,
        Other,
    ];

    /// Kinds worth fetching at their own address when a page points at one.
    ///
    /// The set `extract` has a reader for, and no wider: fetching bytes no stage can turn
    /// into text spends a request to store something nothing will search.
    pub const ENCLOSABLE: &'static [ContentKind] = &[Pdf, Document, Spreadsheet, Csv];

    /// The word this kind is recorded as. Records hold the string, not the variant.
    pub fn as_str(self) -> &'static str {
        match self {
            Html => "html",
            Pdf => "pdf",
            Text => "text",
            Markdown => "markdown",
            Captions => "captions",
            Json => "json",
            Xml => "xml",
            Csv => "csv",
            Spreadsheet => "spreadsheet",
            Document => "document",
            ZipContainer => "zip-container",
            Audio => "audio",
            Other => "other",
        }
    }

    /// Reads a kind back out of a record.
    ///
    /// `None` for a word this build does not know, which is a real case on an
    /// append-only log: a store written by a newer build holds kinds this one has never
    /// heard of, and guessing [`Other`] for them would quietly relabel evidence.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.as_str() == s)
    }

    /// A coarse kind, from the `content-type` header with a magic-byte fallback.
    ///
    /// Hosts mislabel constantly — `.gov` servers routinely serve PDFs as
    /// `application/octet-stream` — so a declared type that means nothing falls through
    /// to the bytes rather than being believed.
    pub fn classify(meta: &BTreeMap<String, String>, bytes: &[u8]) -> Self {
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

        if let Some(kind) = Self::from_mime(&declared) {
            // The one declared type that cannot be trusted on its own: a caption track is
            // served as ordinary JSON, so the header cannot tell it apart from a vendor
            // API response.
            return match kind {
                Json if crate::captions::looks_like_json3(bytes) => Captions,
                kind => kind,
            };
        }
        if declared.starts_with("audio/") {
            return Audio;
        }
        if let Some(kind) = Self::from_magic(bytes) {
            return kind;
        }

        // Both signals came back empty. The served address's own name is the last thing
        // left, and it is better than [`Other`].
        Self::from_served_name(meta).unwrap_or(Other)
    }

    /// The kind the served address's filename names, for bytes that proved nothing.
    ///
    /// Reached **only** where the declared type named no kind and the first bytes could
    /// not tell either, which is exactly the `.csv` case: ordinary text has no magic, and
    /// `application/octet-stream` is not an opinion about the content at all — it is IIS's
    /// default for an extension missing from its MIME map. Measured on the Hillsborough
    /// clerk's file server: **over 300 CSV files, more than 2.2 GB**, every one collected
    /// and every one recorded Underivable, in silence, because no reader claims [`Other`].
    ///
    /// *Why this does not reopen what [`Self::declared_type_for_path`] refuses.* That rule
    /// keeps a filename's opinion out of the place a server's header belongs. Here there is
    /// no header worth the word and no evidence in the bytes, so nothing is being
    /// displaced — and the name being read is the **server's own address**, not one a
    /// caller supplied. Every server that declares something real is unaffected, because
    /// this is never reached when one does.
    fn from_served_name(meta: &BTreeMap<String, String>) -> Option<Self> {
        let served = url::Url::parse(meta.get("final_url")?).ok()?;
        Self::from_path(Path::new(served.path()))
    }

    /// The kind a content-type names, exactly.
    pub fn from_mime(declared: &str) -> Option<Self> {
        FORMATS
            .iter()
            .find(|f| f.mime == Some(declared))
            .map(|f| f.kind)
            .or_else(|| {
                ALIASES
                    .iter()
                    .find(|(m, _)| *m == declared)
                    .map(|(_, k)| *k)
            })
    }

    /// The kind the first bytes prove, where they prove one.
    ///
    /// Ordered by how specific the signature is. `zip-container` is deliberately last of
    /// the containers, because it is the least informative answer that is still true.
    pub fn from_magic(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"%PDF-") {
            return Some(Pdf);
        }
        // The two document signatures that fit in a head. An OLE compound file is a
        // legacy `.doc`, `.ppt` or `.xls`, and which one is written in a directory sector
        // that can sit anywhere in the file — so this says `document` and extraction,
        // holding the whole blob, sorts the spreadsheets back out.
        if bytes.starts_with(b"{\\rtf") || bytes.starts_with(&OLE_MAGIC) {
            return Some(Document);
        }
        if bytes.starts_with(b"PK\x03\x04") {
            return Some(ZipContainer);
        }
        if audio_container(bytes).is_some() {
            return Some(Audio);
        }
        let head = &bytes[..bytes.len().min(256)];
        let head = String::from_utf8_lossy(head)
            .trim_start()
            .to_ascii_lowercase();
        if head.starts_with("<!doctype html") || head.starts_with("<html") {
            return Some(Html);
        }
        None
    }

    /// The kind a filename names, where the extension is one we know.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())?
            .to_ascii_lowercase();
        FORMATS.iter().find(|f| f.ext == ext).map(|f| f.kind)
    }

    /// The `content-type` a server would have declared for a file with this name.
    ///
    /// A file read off disk arrives with no headers, so [`Self::classify`] has only magic
    /// bytes to go on — and for the formats whose first bytes are indistinguishable from
    /// plain text that is not enough. A `.csv` sniffs to [`Other`] and no extractor claims
    /// it, even though the very same bytes are read fine the moment a server calls them
    /// `text/csv`.
    ///
    /// Deliberately **not** consulted for anything fetched. A server's own header is
    /// evidence; a filename is a guess, and a guess belongs only where there is nothing
    /// else.
    pub fn declared_type_for_path(path: &Path) -> Option<&'static str> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())?
            .to_ascii_lowercase();
        FORMATS.iter().find(|f| f.ext == ext).and_then(|f| f.mime)
    }

    /// The file extension this kind materialises under.
    ///
    /// Every kind needs an answer. One that fell through to `bin` would produce a file no
    /// application will open, which defeats the entire purpose of `current/` — the point
    /// of that tree is that the bytes arrive wearing a name their handler recognises.
    ///
    /// `head` is the first bytes of the blob, and matters only for the kinds this
    /// vocabulary deliberately generalises: `audio` is one word for five containers, and
    /// every player refuses a WebM called `.m4a`. Pass an empty slice when the bytes are
    /// not to hand; the answer degrades to the commonest container rather than to `bin`.
    pub fn extension(self, head: &[u8]) -> &'static str {
        match self {
            // One word, eight formats. The head settles the two containers that announce
            // themselves in their first bytes; the rest hide their identity at the end of
            // the file, past anything this function holds.
            Document => document_container(head).unwrap_or_else(|| self.canonical_extension()),
            // yt-dlp's default for this project's sources, and so the safest fallback when
            // the head was not read or the container is one we do not know.
            Audio => audio_container(head).unwrap_or_else(|| self.canonical_extension()),
            // A json3 caption track is JSON however machine-shaped it reads, and JSON
            // opens in every editor and browser on the machine. `.bin` opens in none.
            Captions => "json",
            // Looked at, matched nothing. There is no name that would help.
            Other => "bin",
            kind => kind.canonical_extension(),
        }
    }

    /// The first extension [`FORMATS`] gives this kind.
    ///
    /// Panics for a kind with no row, which is why [`Self::extension`] answers those
    /// before delegating here — and why that match is exhaustive rather than defaulted.
    fn canonical_extension(self) -> &'static str {
        FORMATS
            .iter()
            .find(|f| f.kind == self)
            .map(|f| f.ext)
            .unwrap_or("bin")
    }

    /// Every extension that names a kind worth fetching at its own address.
    ///
    /// [`crate::enclosure`]'s allowlist, read off the table rather than retyped beside it.
    pub fn enclosable_extensions() -> impl Iterator<Item = &'static str> {
        FORMATS
            .iter()
            .filter(|f| Self::ENCLOSABLE.contains(&f.kind))
            .map(|f| f.ext)
    }
}

impl std::fmt::Display for ContentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which document container, as a file extension, where the first bytes can say.
///
/// RTF announces itself in five bytes. An OLE compound file announces only that it is
/// one: whether it holds Word, PowerPoint or Excel is written in a directory sector that
/// can sit anywhere in the file, so `.doc` is the answer — the commonest of the three on
/// a `.gov` server, and one Word will at least open. `.docx` on those bytes opens in
/// nothing, because Word checks the container before the extension.
fn document_container(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(b"{\\rtf") {
        return Some("rtf");
    }
    if head.starts_with(&OLE_MAGIC) {
        return Some("doc");
    }
    None
}

/// Which audio container, as a file extension — the kind sniff asked a finer question.
///
/// [`ContentKind::classify`] answers with one word, `audio`, because that is the
/// distinction `transcribe` needs. Naming a file needs the next one down.
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

    fn classify(ct: &str, bytes: &[u8]) -> ContentKind {
        ContentKind::classify(&meta(ct), bytes)
    }

    // ── the table answers every question ──────────────────────────────────────

    /// The gap that shipped: `captions` had no arm in the old `extension_for`, so every
    /// YouTube caption track materialised as `watch~ec1ba331.bin` and opened in nothing.
    ///
    /// This is now a loop over [`ContentKind::ALL`] rather than a hand-kept list, so a
    /// new variant is covered the moment it exists.
    #[test]
    fn every_kind_materialises_as_something_openable() {
        for kind in ContentKind::ALL {
            if *kind == Other {
                continue;
            }
            assert_ne!(
                kind.extension(b""),
                "bin",
                "`{kind}` materialises as .bin — give it a row in FORMATS"
            );
        }
    }

    /// A round trip through the recorded word, for every kind. A variant whose `as_str`
    /// collides with another's would silently relabel blobs on the way back in.
    #[test]
    fn every_kind_survives_being_written_down() {
        for kind in ContentKind::ALL {
            assert_eq!(ContentKind::parse(kind.as_str()), Some(*kind));
        }
        assert_eq!(ContentKind::parse("chartreuse"), None);
    }

    /// The drift guard the two tables used to need, now structural: an extension that
    /// infers a content-type the classifier does not recognise is a silent no-op that
    /// looks like support.
    #[test]
    fn every_inferred_type_is_one_the_classifier_knows() {
        for format in FORMATS {
            let Some(mime) = format.mime else { continue };
            assert_eq!(
                classify(mime, b""),
                format.kind,
                "`.{}` infers `{mime}`, which does not classify as {}",
                format.ext,
                format.kind
            );
        }
    }

    /// `enclosure`'s allowlist used to be thirteen extensions retyped beside the table.
    #[test]
    fn the_enclosable_set_is_the_readable_set() {
        let got: BTreeMap<&str, ()> = ContentKind::enclosable_extensions()
            .map(|e| (e, ()))
            .collect();
        let want = [
            "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "rtf", "odt", "ods", "odp", "epub",
            "csv",
        ];
        assert_eq!(got.len(), want.len(), "got {:?}", got.keys());
        for ext in want {
            assert!(got.contains_key(ext), "`.{ext}` is no longer enclosable");
        }
    }

    // ── classification, unchanged ─────────────────────────────────────────────

    #[test]
    fn content_type_header_is_used_when_present() {
        assert_eq!(classify("text/html; charset=utf-8", b""), Html);
        assert_eq!(classify("application/pdf", b""), Pdf);
        assert_eq!(
            classify(
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                b""
            ),
            Spreadsheet
        );
    }

    #[test]
    fn magic_bytes_override_a_useless_content_type() {
        // .gov servers routinely serve PDFs as octet-stream.
        assert_eq!(classify("application/octet-stream", b"%PDF-1.7\n..."), Pdf);
        assert_eq!(
            classify("application/octet-stream", b"<!DOCTYPE html><html>"),
            Html
        );
        assert_eq!(
            classify("application/octet-stream", b"PK\x03\x04junk"),
            ZipContainer
        );
        assert_eq!(
            classify("application/octet-stream", br"{\rtf1\ansi"),
            Document
        );
        assert_eq!(classify("application/octet-stream", &OLE_MAGIC), Document);
    }

    /// The 2.2 GB case. IIS serves `.csv` as `application/octet-stream`, which asserts
    /// nothing, and a CSV's first bytes are ordinary text — so both signals come back
    /// empty and the served address is the only evidence left.
    fn served(url: &str, ct: &str, bytes: &[u8]) -> ContentKind {
        let mut meta = meta(ct);
        meta.insert("final_url".into(), url.into());
        ContentKind::classify(&meta, bytes)
    }

    #[test]
    fn a_csv_served_as_octet_stream_is_read_off_the_address_it_came_from() {
        assert_eq!(
            served(
                "https://publicrec.hillsclerk.com/Probate/dailyfilings/ProbateFiling_20260806.csv",
                "application/octet-stream",
                b"CaseNbr,Party,Judge\n25-CA-012120,DOE,SMITH\n",
            ),
            Csv,
            "the largest category on that server, lost in silence"
        );
    }

    /// The rule this must not reopen: a server that declares something real still wins,
    /// and so do the bytes. The address is consulted last or not at all.
    #[test]
    fn a_real_declaration_and_real_magic_both_outrank_the_address() {
        // The header names a kind — the `.csv` in the address is not consulted.
        assert_eq!(
            served("https://x.gov/report.csv", "text/html", b"anything"),
            Html
        );
        // The header says nothing, but the bytes do.
        assert_eq!(
            served(
                "https://x.gov/report.csv",
                "application/octet-stream",
                b"%PDF-1.7\n"
            ),
            Pdf
        );
    }

    #[test]
    fn an_address_that_names_nothing_still_reaches_other() {
        // A directory, an unknown extension, and the invented address from a `<script>`
        // whose filename has no stem at all.
        for url in [
            "https://x.gov/Civil/bulkdata/",
            "https://x.gov/plans.dwg",
            "https://x.gov/251agendaonline/.pdf",
        ] {
            assert_eq!(
                served(url, "application/octet-stream", b"\x00\x01\x02"),
                Other,
                "{url}"
            );
        }
        // And a missing `final_url` is simply no evidence.
        assert_eq!(classify("application/octet-stream", b"\x00\x01\x02"), Other);
    }

    /// Every office type a `.gov` server labels correctly reaches one of two words.
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
            assert_eq!(classify(ct, b""), Document, "{ct}");
        }
        for ct in [
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "application/vnd.ms-excel",
            "application/vnd.oasis.opendocument.spreadsheet",
        ] {
            assert_eq!(classify(ct, b""), Spreadsheet, "{ct}");
        }
    }

    #[test]
    fn unknown_content_is_labelled_other_not_guessed() {
        assert_eq!(
            ContentKind::classify(&BTreeMap::new(), b"\x00\x01\x02\x03"),
            Other
        );
    }

    #[test]
    fn classify_does_not_panic_on_short_bodies() {
        assert_eq!(ContentKind::classify(&BTreeMap::new(), b""), Other);
        assert_eq!(ContentKind::classify(&BTreeMap::new(), b"%"), Other);
    }

    // ── a filename, where the bytes cannot say ────────────────────────────────

    /// A `.csv` is the case this exists for: its first bytes are ordinary text, so magic
    /// bytes alone reach `other` and no extractor claims it — while the identical bytes
    /// are read fine the moment a server calls them `text/csv`.
    #[test]
    fn an_extension_answers_where_the_bytes_cannot() {
        let bytes = b"district,population\nEast,41200\n";
        assert_eq!(ContentKind::classify(&BTreeMap::new(), bytes), Other);

        let declared = ContentKind::declared_type_for_path(Path::new("districts.csv")).unwrap();
        assert_eq!(classify(declared, bytes), Csv);
    }

    /// Nothing to infer from is not the same as inferring nothing.
    #[test]
    fn an_unknown_extension_infers_nothing() {
        assert_eq!(ContentKind::declared_type_for_path(Path::new("blob")), None);
        assert_eq!(
            ContentKind::declared_type_for_path(Path::new("archive.dwg")),
            None
        );
        // Case is a filename's business, not a format's.
        assert_eq!(
            ContentKind::declared_type_for_path(Path::new("REPORT.PDF")),
            Some("application/pdf")
        );
    }

    /// An extension we write but are never told stays uninferrable — the answer a caller
    /// needs in order to fall back to the bytes rather than to a wrong header.
    #[test]
    fn an_extension_no_server_declares_infers_nothing() {
        assert_eq!(
            ContentKind::declared_type_for_path(Path::new("a.zip")),
            None
        );
        assert_eq!(
            ContentKind::from_path(Path::new("a.zip")),
            Some(ZipContainer)
        );
    }

    /// `.md` is claimed by two kinds. On disk it is what a server would call it; the
    /// `Markdown` kind is what extraction writes.
    #[test]
    fn markdown_reads_as_text_and_writes_as_md() {
        assert_eq!(ContentKind::from_path(Path::new("notes.md")), Some(Text));
        assert_eq!(
            ContentKind::declared_type_for_path(Path::new("notes.md")),
            Some("text/plain")
        );
        assert_eq!(Markdown.extension(b""), "md");
    }

    // ── containers ────────────────────────────────────────────────────────────

    /// One word, five containers: a player refuses a WebM called `.m4a`, so the head
    /// decides. An unread head still beats `.bin`.
    #[test]
    fn audio_is_named_by_its_container() {
        assert_eq!(Audio.extension(b"\x00\x00\x00\x20ftypM4A "), "m4a");
        assert_eq!(Audio.extension(&[0x1A, 0x45, 0xDF, 0xA3]), "webm");
        assert_eq!(Audio.extension(b"OggS\0\x02"), "ogg");
        assert_eq!(Audio.extension(b"ID3\x04\0\0"), "mp3");
        assert_eq!(Audio.extension(b""), "m4a");
    }

    #[test]
    fn a_document_is_named_by_its_container_where_it_says() {
        assert_eq!(Document.extension(br"{\rtf1\ansi"), "rtf");
        assert_eq!(Document.extension(&OLE_MAGIC), "doc");
        assert_eq!(Document.extension(b""), "docx");
    }

    #[test]
    fn the_plain_kinds_name_themselves() {
        assert_eq!(Pdf.extension(b""), "pdf");
        assert_eq!(Html.extension(b""), "html");
        assert_eq!(Text.extension(b""), "txt");
        assert_eq!(Spreadsheet.extension(b""), "xlsx");
        assert_eq!(Captions.extension(b""), "json");
        assert_eq!(Other.extension(b""), "bin");
    }
}
