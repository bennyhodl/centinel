//! `open` — read a collected document in a real application.
//!
//! Search tells you a passage exists and cites the bytes it came from. This is how you
//! actually look at it: the blob is linked into `current/` under a usable name with a
//! real extension, then handed to whichever application the config nominates for that
//! kind of file.
//!
//! Both halves of a document are reachable — the **original** bytes as served, and the
//! **derived** markdown. Reading the extraction next to its source is the fastest way to
//! judge whether extraction is doing a good job.
//!
//! The original is the default, because for almost every kind it is the better artefact:
//! a browser renders the HTML, Acrobat renders the PDF, a player plays the audio, and
//! the extraction is a lossy summary of any of them. The exception is a caption track,
//! which is a machine interchange format whose only human content is the text inside it.
//! Those open as their transcript unless `--original` says otherwise.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::{Config, SYSTEM_DEFAULT};
use crate::content::ContentKind;
use crate::materialize::materialize;
use crate::ops::target::resolve;
use crate::prelude::*;
use crate::tool::Tool;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct OpenArgs {
    /// A URL, a substring of one, or a blob hash — the short hash `search` prints is enough.
    #[arg(value_name = "TARGET")]
    pub target: String,

    /// Open the extracted text rather than the original bytes.
    #[arg(long)]
    #[serde(default)]
    pub derived: bool,

    /// Open the bytes as served, even for a kind that defaults to its extracted text.
    #[arg(long, conflicts_with = "derived")]
    #[serde(default)]
    pub original: bool,

    /// Application or `command {path}` template, overriding the config.
    #[arg(long = "with")]
    #[serde(default)]
    pub with: Option<String>,

    /// Materialise and print the path without launching anything.
    ///
    /// The scripting form, and the safe one for a server: `open` launches a GUI
    /// application on the machine running it, which is rarely what a remote caller wants.
    #[arg(long)]
    #[serde(default)]
    pub print_path: bool,

    /// Restrict the search for `target` to one source.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct OpenReport {
    pub url: String,
    pub source: String,
    pub kind: String,
    /// The materialised path under `current/`.
    pub path: String,
    /// SHA-256 of the original bytes as served — the evidentiary anchor.
    ///
    /// The same field, meaning the same thing, as on a search result and a read report.
    /// It used to be whichever blob `open` happened to hand the OS, which meant a
    /// transcript reported a hash that was **not** an Observation and that nothing would
    /// take back.
    pub blob_sha: String,
    /// The blob actually put on disk. Equal to `blob_sha` unless this is derived text.
    pub opened_sha: String,
    pub bytes: usize,
    /// Whether this is the original bytes or the extracted text.
    pub derived: bool,
    /// The command run, or `null` with `--print-path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened_with: Option<String>,
    /// Other matches, when `target` was ambiguous. The first was used.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_matches: Vec<String>,
}

/// Open a collected document in an application.
///
/// **CLI only.** This launches a GUI application on the host and accepts a command
/// template, so exposing it over MCP or HTTP would be arbitrary command execution
/// against a server that has no authentication. Agents wanting the *content* of a
/// document should call `read`, which returns text and touches nothing.
#[op(reach = "host", group = "corpus")]
pub async fn open(ctx: &Ctx, args: OpenArgs) -> anyhow::Result<OpenReport> {
    let found = resolve(ctx, &args.target, args.source.as_deref()).await?;
    let (source, resource, obs) = (found.source, found.resource, found.observation);
    let other_matches = found.other_matches;

    // ---- pick the half --------------------------------------------------------------
    //
    // For nearly every kind the original is the thing you want: a browser renders the
    // HTML, Acrobat renders the PDF, a player plays the audio. A json3 caption track is
    // the exception, and the only one — it is a timing structure with words threaded
    // through it, and the words are the entire reason anybody opens one. Defaulting it
    // to the served bytes hands over five megabytes of machine JSON and calls it done.
    //
    // A target that named a derived blob decides this by itself: someone typing back a
    // hash `open` printed is asking for the thing that hash identified, and handing them
    // the other half would make the round trip a lie.
    //
    // Classification needs the bytes, so it is skipped when something already decided.
    let named = found.matched_derived.clone();
    let settled = args.derived || args.original || named.is_some();
    let original_bytes = if settled {
        None
    } else {
        Some(ctx.store.get_blob(&obs.blob_sha).await?)
    };
    let reads_as_machine_format = original_bytes
        .as_deref()
        .is_some_and(|bytes| ContentKind::classify(&obs.meta, bytes) == ContentKind::Captions);
    // `--original` is an instruction and beats every inference, including a hash that
    // named the extraction.
    let derived = !args.original && (args.derived || named.is_some() || reads_as_machine_format);

    // ---- pick the blob --------------------------------------------------------------
    let (opened_sha, kind) = if derived {
        // The exact blob asked for, when one was named; otherwise the newest extraction,
        // because a re-run with a better tool supersedes an older one.
        let to_sha = match named {
            Some(sha) => sha,
            None => {
                ctx.store
                    .latest_derivation(&source, &obs.blob_sha)
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "no extracted text for {} — run `centinel extract` first, or \
                         `--original` to open the bytes as served",
                            resource.natural_key
                        )
                    })?
                    .to_sha
            }
        };
        (to_sha, ContentKind::Markdown)
    } else {
        // Read once: the default path already fetched these to classify them.
        let bytes = match original_bytes {
            Some(bytes) => bytes,
            None => ctx.store.get_blob(&obs.blob_sha).await?,
        };
        (
            obs.blob_sha.clone(),
            ContentKind::classify(&obs.meta, &bytes),
        )
    };

    let path = materialize(
        &ctx.store,
        &source,
        &resource.natural_key,
        &opened_sha,
        kind,
    )
    .await?;
    let bytes = tokio::fs::metadata(&path).await?.len() as usize;

    // ---- launch ---------------------------------------------------------------------
    let opened_with = if args.print_path {
        None
    } else {
        let config = Config::load()?;
        let opener = args
            .with
            .clone()
            .unwrap_or_else(|| config.open.opener_for(kind.as_str()).to_string());
        Some(launch(&opener, &path).await?)
    };

    Ok(OpenReport {
        url: resource.natural_key,
        source: source.to_string(),
        kind: kind.to_string(),
        path: path.display().to_string(),
        blob_sha: obs.blob_sha.to_string(),
        opened_sha: opened_sha.to_string(),
        bytes,
        derived,
        opened_with,
        other_matches,
    })
}

/// Runs the configured opener, returning the command for the report.
///
/// Three forms, distinguished without configuration ceremony:
/// - `"system"` — hand it to the OS default handler
/// - anything containing `{path}` — a command template
/// - anything else — an application name
async fn launch(opener: &str, path: &std::path::Path) -> anyhow::Result<String> {
    let p = path.to_string_lossy().to_string();

    let (program, argv) = if opener == SYSTEM_DEFAULT || opener.is_empty() {
        (system_opener().to_string(), vec![p.clone()])
    } else if opener.contains("{path}") {
        let mut parts = opener
            .split_whitespace()
            .map(|t| t.replace("{path}", &p))
            .collect::<Vec<_>>();
        anyhow::ensure!(!parts.is_empty(), "empty command template");
        let program = parts.remove(0);
        (program, parts)
    } else if cfg!(target_os = "macos") {
        // `open -a` is how macOS names an application rather than a binary.
        (
            "open".to_string(),
            vec!["-a".into(), opener.into(), p.clone()],
        )
    } else {
        (opener.to_string(), vec![p.clone()])
    };

    // `interactive` rather than `output`: the opener may be a person's editor, so it
    // inherits the terminal and gets no deadline. It is still `tokio::process` and still
    // awaited — the previous version blocked a runtime thread for as long as the
    // application stayed open.
    let tool = Tool::new(&program).args(&argv);
    let status = tool.interactive().await?;
    anyhow::ensure!(
        status.success(),
        "`{program}` exited with {status}; is the application installed?"
    );

    Ok(tool.display())
}

/// The command this platform opens a file with.
///
/// `pub(crate)` so `check` suggests the same one rather than spelling a second opinion
/// about what a Linux desktop uses.
pub(crate) fn system_opener() -> &'static str {
    if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    }
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// One line of confirmation, because the result of `open` is a window appearing.
///
/// The path is the exception: with `--print-path` it *is* the output, so it is printed
/// plainly enough to be copied or piped into another command.
///
/// Both hashes are shown when they differ, and **both resolve**. The lead hash is the
/// original bytes, so the line means the same thing it does under `search` and `read`;
/// the second is the extraction that was actually opened. Printing only the second — as
/// this did — put a hash on screen that `open` itself would then refuse, because a
/// derived blob is not an Observation and nothing was looking for one.
impl Render for OpenReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        match &self.opened_with {
            Some(cmd) => {
                p.marked(Mark::Ok, p.paint(cmd, Ink::Cyan))?;
                p.nest(|p| p.line(p.paint(&self.path, Ink::Dim)))?;
            }
            None => p.line(&self.path)?,
        }

        let hash = p.paint(&render::short_sha(&self.blob_sha), Ink::Cyan);
        let what = format!(
            "{} · {} · {}{}",
            self.source,
            self.kind,
            render::bytes(self.bytes as u64),
            if self.derived {
                " · extracted text"
            } else {
                ""
            },
        );
        p.nest(|p| p.line(format!("{hash} · {}", p.paint(&what, Ink::Dim))))?;

        // Say so when the extracted text was opened, name the hash of the thing on disk,
        // and name the way back. Silently handing over a *different file* than the one
        // whose hash was typed is the kind of helpfulness that costs trust the first time
        // somebody notices.
        if self.derived {
            p.nest(|p| {
                let opened = p.paint(&render::short_sha(&self.opened_sha), Ink::Cyan);
                let label = p.paint("the extracted text itself", Ink::Dim);
                p.line(format!("{opened} · {label}"))?;
                let flag = p.paint("--original", Ink::Cyan);
                let rest = p.paint("opens the bytes as served", Ink::Dim);
                p.line(format!("{flag}  {rest}"))
            })?;
        }

        if !self.other_matches.is_empty() {
            p.nest(|p| {
                let note = format!(
                    "{} other matched; this is the first",
                    render::plural(self.other_matches.len(), "address", "addresses")
                );
                p.line(p.paint(&note, Ink::Yellow))
            })?;
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

    /// A `json3` track, recognisable to the sniff that decides a caption opens as text.
    const CAPTIONS: &[u8] =
        br#"{"wireMagic":"pb3","events":[{"tStartMs":0,"segs":[{"utf8":"good evening"}]}]}"#;

    /// A store holding one document and its extracted text.
    ///
    /// The meta matters: `content_kind` only sniffs for `json3` behind a declared
    /// `application/json`, which is what `youtube::observation_meta` records — so a
    /// fixture without it would classify a real caption track as `other` and never take
    /// the branch this file is about.
    async fn corpus(dir: &std::path::Path, original: &[u8], key: &str) -> (Ctx, OpenArgs) {
        let store = Store::open(dir.join("store")).await.unwrap();
        let id = SourceId::new("tampa").unwrap();

        let meta = std::collections::BTreeMap::from([(
            "content-type".to_string(),
            "application/json".to_string(),
        )]);
        let obs = store
            .record_observation(
                &Resource::new(id.clone(), key),
                original,
                jiff::Timestamp::now(),
                meta,
            )
            .await
            .unwrap();

        let to_sha = store.put_blob(b"# Agenda\n\ngood evening").await.unwrap();
        store
            .append(
                &id,
                &LogRecord::Derivation(Derivation {
                    from_sha: obs.blob_sha,
                    to_sha,
                    tool: "captions".into(),
                    version: "0.1".into(),
                    model_tier: None,
                    at: jiff::Timestamp::now(),
                    anchors: Vec::new(),
                }),
            )
            .await
            .unwrap();

        let args = OpenArgs {
            target: key.to_string(),
            derived: false,
            original: false,
            with: None,
            // Never launch anything from a test — and this is the flag a server would use
            // for the same reason.
            print_path: true,
            source: None,
        };
        (Ctx::new(store), args)
    }

    /// The defect: a caption track opens as its transcript by default, and the hash that
    /// went on screen was the transcript's — which nothing would take back, because a
    /// derived blob is not an Observation.
    #[tokio::test]
    async fn every_hash_open_prints_can_be_typed_back() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, args) = corpus(
            dir.path(),
            CAPTIONS,
            "https://www.youtube.com/watch?v=abc#captions.json3",
        )
        .await;

        let report = open(&ctx, args).await.unwrap();
        assert!(report.derived, "a caption track opens as its transcript");
        assert_ne!(report.blob_sha, report.opened_sha);

        for printed in [&report.blob_sha, &report.opened_sha] {
            let short = &printed[..12];
            let found = target::resolve(&ctx, short, None)
                .await
                .unwrap_or_else(|e| panic!("`open` printed `{short}` and then refused it: {e}"));
            assert_eq!(
                found.resource.natural_key,
                "https://www.youtube.com/watch?v=abc#captions.json3"
            );
        }
    }

    /// `blob_sha` means the same thing here as on a search result and a read report: the
    /// bytes as served. It used to be whichever half `open` happened to hand the OS.
    #[tokio::test]
    async fn blob_sha_is_the_original_whichever_half_was_opened() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, args) = corpus(dir.path(), b"%PDF-1.7 pretend", "https://tampa.gov/a.pdf").await;

        let plain = open(&ctx, args.clone()).await.unwrap();
        assert!(!plain.derived);
        assert_eq!(plain.blob_sha, plain.opened_sha);

        let derived = open(
            &ctx,
            OpenArgs {
                derived: true,
                ..args
            },
        )
        .await
        .unwrap();
        assert!(derived.derived);
        assert_eq!(
            derived.blob_sha, plain.blob_sha,
            "the anchor does not move when the opened half does"
        );
        assert_ne!(derived.opened_sha, derived.blob_sha);
    }

    /// Typing back a hash must open the thing that hash identified, or the round trip is
    /// a lie.
    #[tokio::test]
    async fn a_derived_hash_opens_that_extraction() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, args) = corpus(dir.path(), b"%PDF-1.7 pretend", "https://tampa.gov/a.pdf").await;

        let first = open(
            &ctx,
            OpenArgs {
                derived: true,
                ..args.clone()
            },
        )
        .await
        .unwrap();

        let again = open(
            &ctx,
            OpenArgs {
                target: first.opened_sha[..12].to_string(),
                ..args
            },
        )
        .await
        .unwrap();

        assert!(again.derived, "the hash named the extraction");
        assert_eq!(again.opened_sha, first.opened_sha);
        assert_eq!(again.path, first.path);
    }

    /// `--original` is an instruction and beats every inference, including a hash that
    /// named the extraction.
    #[tokio::test]
    async fn original_wins_over_a_derived_target() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, args) = corpus(dir.path(), CAPTIONS, "https://y.test/watch?v=a#captions").await;

        let derived = open(&ctx, args.clone()).await.unwrap();
        let forced = open(
            &ctx,
            OpenArgs {
                target: derived.opened_sha[..12].to_string(),
                original: true,
                ..args
            },
        )
        .await
        .unwrap();

        assert!(!forced.derived);
        assert_eq!(forced.opened_sha, forced.blob_sha);
        assert_eq!(forced.kind, "captions");
    }

    #[tokio::test]
    async fn a_document_with_no_extraction_says_which_command_makes_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).await.unwrap();
        let id = SourceId::new("tampa").unwrap();
        store
            .record_observation(
                &Resource::new(id, "https://tampa.gov/a.pdf"),
                b"%PDF-1.7 pretend",
                jiff::Timestamp::now(),
                Default::default(),
            )
            .await
            .unwrap();
        let ctx = Ctx::new(store);

        let err = open(
            &ctx,
            OpenArgs {
                target: "a.pdf".into(),
                derived: true,
                original: false,
                with: None,
                print_path: true,
                source: None,
            },
        )
        .await
        .map(|_| ())
        .unwrap_err()
        .to_string();
        assert!(err.contains("centinel extract"), "{err}");
        assert!(err.contains("--original"), "{err}");
    }

    // ── rendering ──────────────────────────────────────────────────────────────

    fn render_to_string(report: &OpenReport) -> String {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    fn report(derived: bool) -> OpenReport {
        OpenReport {
            url: "https://tampa.gov/a.pdf".into(),
            source: "tampa".into(),
            kind: if derived { "markdown" } else { "pdf" }.into(),
            path: "/store/current/tampa/a.pdf".into(),
            blob_sha: "3f8a1c9d0b7e".repeat(6)[..64].to_string(),
            opened_sha: if derived {
                "9b2e4a1f0c33".repeat(6)[..64].to_string()
            } else {
                "3f8a1c9d0b7e".repeat(6)[..64].to_string()
            },
            bytes: 4096,
            derived,
            opened_with: Some("open -a Preview /store/current/tampa/a.pdf".into()),
            other_matches: Vec::new(),
        }
    }

    /// Both hashes on screen, because both resolve and they identify different bytes.
    #[test]
    fn opening_the_extraction_shows_both_hashes_and_the_way_back() {
        let out = render_to_string(&report(true));
        assert!(out.contains("3f8a1c9d0b7e"), "the anchor: {out}");
        assert!(out.contains("9b2e4a1f0c33"), "what was opened: {out}");
        assert!(out.contains("--original"), "{out}");
    }

    /// One hash when there is only one thing to name.
    #[test]
    fn opening_the_original_shows_one_hash() {
        let out = render_to_string(&report(false));
        assert!(out.contains("3f8a1c9d0b7e"), "{out}");
        assert!(!out.contains("--original"), "nothing to offer: {out}");
    }

    #[test]
    fn the_report_round_trips_through_json() {
        let r = report(true);
        let json = serde_json::to_value(&r).unwrap();
        let back: OpenReport = serde_json::from_value(json).unwrap();
        assert_eq!(back.blob_sha, r.blob_sha);
        assert_eq!(back.opened_sha, r.opened_sha);
        assert!(back.derived);
    }
}
