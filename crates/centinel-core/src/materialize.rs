//! Giving blobs human-usable names.
//!
//! The blob pool is deliberately dumb — content-addressed, no names, no extensions. That
//! is what lets one PDF on two sites store once, and what makes integrity checkable. But
//! `ac1f68d5…` is not something you can double-click, and no application will guess that
//! it is a PDF.
//!
//! So `current/` (SPEC §5) is the **view**: a URL-mirroring tree of hardlinks into the
//! pool, with real filenames and real extensions. It carries no information the log does
//! not already hold, which is why deleting it costs nothing.
//!
//! ```text
//! current/tampa/www.tampa.gov/city-council.html
//! current/tampa/www.tampa.gov/city-council.md          <- the derived text
//! current/hillsborough/assets.contentstack.io/…/Lobbyist_Report.pdf
//! ```

use std::path::{Path, PathBuf};

use crate::domain::{BlobSha, SourceId};
use crate::store::Store;

/// File extension for a content kind.
///
/// Every kind [`crate::fetch::content_kind`] can return needs an arm here. A kind that
/// falls through to `bin` produces a file no application will open, which defeats the
/// entire purpose of `current/` — the point of this tree is that the bytes arrive
/// wearing a name their handler recognises.
///
/// `head` is the first bytes of the blob, and matters only for kinds the vocabulary
/// deliberately generalises: `audio` is one word for five containers. Pass an empty
/// slice when the bytes are not to hand; the answer degrades to the commonest container
/// rather than to `bin`.
pub fn extension_for(kind: &str, head: &[u8]) -> &'static str {
    match kind {
        "pdf" => "pdf",
        "html" => "html",
        "markdown" => "md",
        "spreadsheet" => "xlsx",
        "csv" => "csv",
        "json" => "json",
        // A json3 caption track is JSON however machine-shaped it reads, and JSON opens
        // in every editor and browser on the machine. `.bin` opens in none of them.
        "captions" => "json",
        "xml" => "xml",
        "text" => "txt",
        // `content_kind` folds .doc into .docx, and the older format is vanishing.
        "document" => "docx",
        // yt-dlp's default for this project's sources, and so the safest fallback when
        // the head was not read or the container is one we do not know.
        "audio" => crate::fetch::audio_container(head).unwrap_or("m4a"),
        "zip-container" => "zip",
        _ => "bin",
    }
}

/// Every container signature this file decides on fits in the first dozen bytes.
const HEAD_BYTES: usize = 16;

/// Bytes enough to recognise a container by.
///
/// Read from the blob rather than passed in, because the caller holding a 200 MB audio
/// blob's *hash* should not have to hold its contents to give it a filename.
async fn head_of(path: &Path) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;

    let file = tokio::fs::File::open(path).await?;
    let mut head = Vec::with_capacity(HEAD_BYTES);
    file.take(HEAD_BYTES as u64).read_to_end(&mut head).await?;
    Ok(head)
}

/// Filesystem components cap at 255 bytes; leave room for an extension and suffix.
const MAX_COMPONENT: usize = 120;

/// Replaces anything that would confuse a filesystem or escape the tree.
fn sanitize(segment: &str) -> String {
    let cleaned: String = segment
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    // `.` and `..` would escape or collide; a leading dot would hide the file.
    let cleaned = match cleaned.trim_matches('.') {
        "" => "_".to_string(),
        s => s.to_string(),
    };

    if cleaned.len() <= MAX_COMPONENT {
        return cleaned;
    }
    // Truncate on a char boundary and disambiguate with a hash of the original.
    use sha2::{Digest, Sha256};
    let tag = hex::encode(Sha256::digest(segment.as_bytes()));
    let mut head: String = cleaned.chars().take(MAX_COMPONENT - 9).collect();
    head.push('~');
    head.push_str(&tag[..8]);
    head
}

/// Maps a URL onto a relative path under `current/<source>/`.
///
/// Query strings are folded into the filename as a short hash rather than dropped.
/// `.gov` agenda systems are routinely query-string addressed — `MeetingView.aspx?id=1`
/// and `?id=2` are different documents, and collapsing them would silently overwrite.
pub fn relative_path(url: &str, kind: &str, head: &[u8]) -> PathBuf {
    let ext = extension_for(kind, head);

    // A hostless "URL" is an opaque identifier, not an address — `legistar:matter:5107`
    // parses fine but has no host and no meaningful path structure. Treat the whole
    // string as one filename rather than inventing a directory tree for it.
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string));

    let (Some(host), Ok(parsed)) = (host, url::Url::parse(url)) else {
        return PathBuf::from(format!("{}.{ext}", sanitize(url)));
    };

    let mut path = PathBuf::new();
    path.push(sanitize(&host));

    let segments: Vec<&str> = parsed.path().split('/').filter(|s| !s.is_empty()).collect();

    let (dirs, last) = match segments.split_last() {
        Some((last, dirs)) => (dirs, (*last).to_string()),
        // A bare origin, e.g. https://x.gov/
        None => (&[][..], "index".to_string()),
    };
    for d in dirs {
        path.push(sanitize(d));
    }

    // Preserve an existing correct extension rather than doubling it up.
    let stem = match last.rsplit_once('.') {
        Some((stem, e)) if e.eq_ignore_ascii_case(ext) => stem.to_string(),
        _ => last.clone(),
    };
    let mut name = sanitize(&stem);

    if let Some(query) = parsed.query().filter(|q| !q.is_empty()) {
        use sha2::{Digest, Sha256};
        let tag = hex::encode(Sha256::digest(query.as_bytes()));
        name.push('~');
        name.push_str(&tag[..8]);
    }

    path.push(format!("{name}.{ext}"));
    path
}

/// Where a document lands, given the `current/` tree it lands in.
///
/// Takes the tree rather than the store root, so the one place that knows `current/` is
/// spelled `current/` stays [`Store::current_dir`].
pub fn target_path(
    current: &Path,
    source: &SourceId,
    url: &str,
    kind: &str,
    head: &[u8],
) -> PathBuf {
    current
        .join(source.as_str())
        .join(relative_path(url, kind, head))
}

/// Links a blob into `current/` under a usable name, returning the path.
///
/// Hardlinks so the bytes exist once. Falls back to a copy when hardlinking is refused
/// — a store on a filesystem that does not support them still materialises, just less
/// cheaply.
pub async fn materialize(
    store: &Store,
    source: &SourceId,
    url: &str,
    blob_sha: &BlobSha,
    kind: &str,
) -> anyhow::Result<PathBuf> {
    let src = store.blob_path_of(blob_sha);
    let dest = target_path(
        &store.current_dir(),
        source,
        url,
        kind,
        &head_of(&src).await?,
    );

    if tokio::fs::try_exists(&dest).await? {
        return Ok(dest);
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Verifies the hash on the way through, so materialising cannot propagate a
    // corrupt blob into something a person will read and trust.
    let bytes = store.get_blob(blob_sha).await?;

    match tokio::fs::hard_link(&src, &dest).await {
        Ok(()) => Ok(dest),
        Err(_) => {
            tokio::fs::write(&dest, &bytes).await?;
            Ok(dest)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(url: &str, kind: &str) -> String {
        relative_path(url, kind, b"")
            .to_string_lossy()
            .replace('\\', "/")
    }

    #[test]
    fn mirrors_host_and_path_with_a_real_extension() {
        assert_eq!(
            p("https://www.tampa.gov/city-council", "html"),
            "www.tampa.gov/city-council.html"
        );
        assert_eq!(
            p("https://hcfl.gov/departments/budget/2026", "pdf"),
            "hcfl.gov/departments/budget/2026.pdf"
        );
    }

    #[test]
    fn an_existing_correct_extension_is_not_doubled() {
        assert_eq!(
            p("https://x.gov/docs/Report.pdf", "pdf"),
            "x.gov/docs/Report.pdf"
        );
        // A wrong one is kept as part of the stem — the server's kind wins.
        assert_eq!(
            p("https://x.gov/docs/Report.aspx", "html"),
            "x.gov/docs/Report.aspx.html"
        );
    }

    /// The `.gov` case that would otherwise silently overwrite: same path, different
    /// query, genuinely different documents.
    #[test]
    fn query_strings_do_not_collide() {
        let a = p("https://x.gov/MeetingView.aspx?MeetingID=1", "html");
        let b = p("https://x.gov/MeetingView.aspx?MeetingID=2", "html");
        assert_ne!(a, b, "distinct meetings must not share a file");
        assert!(a.starts_with("x.gov/MeetingView.aspx~"));
    }

    #[test]
    fn a_bare_origin_becomes_index() {
        assert_eq!(
            p("https://www.phila.gov/", "html"),
            "www.phila.gov/index.html"
        );
        assert_eq!(
            p("https://www.phila.gov", "html"),
            "www.phila.gov/index.html"
        );
    }

    /// What matters is that no path *component* is `..` or absolute — a filename that
    /// merely contains dots is harmless. `_.._.._etc_passwd` is one safe file, not an
    /// escape.
    #[test]
    fn path_traversal_cannot_escape_the_tree() {
        for target in [
            "https://x.gov/../../etc/passwd",
            "../../../etc/passwd",
            "/etc/passwd",
            "..",
            "https://x.gov/a/../../../../b",
        ] {
            let path = relative_path(target, "text", b"");
            assert!(path.is_relative(), "{target} produced an absolute path");
            for component in path.components() {
                assert!(
                    matches!(component, std::path::Component::Normal(_)),
                    "{target} produced a traversing component: {path:?}"
                );
                let s = component.as_os_str().to_string_lossy();
                assert_ne!(s, "..", "{target} kept a parent-dir component");
            }
        }
        // The host still anchors a real URL.
        assert!(p("https://x.gov/../../etc/passwd", "text").starts_with("x.gov/"));
    }

    #[test]
    fn absurdly_long_segments_are_truncated_and_disambiguated() {
        let long = "a".repeat(400);
        let one = p(&format!("https://x.gov/{long}1"), "pdf");
        let two = p(&format!("https://x.gov/{long}2"), "pdf");

        let name = one.rsplit('/').next().unwrap();
        assert!(name.len() < 200, "component still too long: {}", name.len());
        assert_ne!(one, two, "truncation must not collapse distinct URLs");
    }

    #[test]
    fn non_url_targets_still_get_a_filename() {
        assert_eq!(
            p("legistar:matter:5107", "json"),
            "legistar_matter_5107.json"
        );
    }

    #[test]
    fn extensions_cover_the_kinds_the_fetcher_reports() {
        assert_eq!(extension_for("pdf", b""), "pdf");
        assert_eq!(extension_for("markdown", b""), "md");
        assert_eq!(extension_for("spreadsheet", b""), "xlsx");
        assert_eq!(extension_for("other", b""), "bin");
    }

    /// The gap that shipped: `captions` had no arm, so every YouTube caption track
    /// materialised as `watch~ec1ba331.bin` and opened in nothing.
    ///
    /// Kept as a list rather than a spot-check because the failure mode is silent —
    /// `content_kind` gains a word, `extension_for` does not, and nobody finds out until
    /// they try to open one.
    #[test]
    fn no_kind_the_fetcher_reports_falls_through_to_bin() {
        for kind in [
            "pdf",
            "html",
            "text",
            "json",
            "captions",
            "xml",
            "csv",
            "spreadsheet",
            "document",
            "audio",
            "zip-container",
            "markdown",
        ] {
            assert_ne!(
                extension_for(kind, b""),
                "bin",
                "`{kind}` materialises as .bin — add an arm to extension_for"
            );
        }
    }

    /// One word, five containers: a player refuses a WebM called `.m4a`, so the head
    /// decides. An unread head still beats `.bin`.
    #[test]
    fn audio_is_named_by_its_container() {
        assert_eq!(extension_for("audio", b"\x00\x00\x00\x20ftypM4A "), "m4a");
        assert_eq!(extension_for("audio", &[0x1A, 0x45, 0xDF, 0xA3]), "webm");
        assert_eq!(extension_for("audio", b"OggS\0\x02"), "ogg");
        assert_eq!(extension_for("audio", b"ID3\x04\0\0"), "mp3");
        assert_eq!(extension_for("audio", b""), "m4a");
    }

    #[test]
    fn a_caption_track_is_named_as_the_json_it_is() {
        assert_eq!(
            p(
                "https://www.youtube.com/watch?v=VPMDoKtJQG8#captions.json3",
                "captions"
            ),
            "www.youtube.com/watch~92aa30ca.json"
        );
    }
}
