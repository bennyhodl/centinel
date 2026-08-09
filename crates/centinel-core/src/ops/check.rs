//! `check` — what does extraction make of this?
//!
//! Give it a link or a file. It fetches, runs the extractor, writes both the bytes as
//! served and the text that came out into a temporary directory, and tells you how to
//! look at each. Nothing is stored, indexed, embedded or logged.
//!
//! ## Why it touches no store
//!
//! The question here is about the *extractor*, not about the corpus: whether a `.gov`
//! page's headings survived `dom_smoothie`, whether a PDF's text layer came out as prose
//! or as ligature soup, what a spreadsheet turns into. None of that needs an Observation,
//! and filing one would mean a run of experiments left a source in the archive that no
//! `[[source]]` block ever asked for. So there is no `--root`, no log line, no blob — two
//! ordinary files in `$TMPDIR` and the commands to open them.
//!
//! That also makes it safe to point at anything. A link somebody sent you, a file off a
//! USB stick, a page you are not sure you want to collect yet: `check` answers what the
//! pipeline would make of it without deciding to keep it.
//!
//! ## It runs the real extractor
//!
//! Text comes from [`crate::extract::derive`] — the same function `extract` calls, so the
//! reader that speaks here is the reader that would speak on a real run, including the
//! poppler fallback for a PDF the primary makes nothing of. A tool that ran its own
//! extractor would answer a question nobody asked.
//!
//! Enclosures are followed for the same reason `collect` follows them: on a CMS the page
//! is often a wrapper and the document is one address away, and *"what would this link
//! give me"* has to include the thing the link is actually for.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::content::ContentKind;
use crate::extract::{self, Extracted};
use crate::policy::{DEFAULT_USER_AGENT, HostPolicy};
use crate::prelude::*;
use crate::sources::SiteSource;
use crate::verdict::Verdict;

/// Lines of extracted text shown inline before the paths.
const DEFAULT_HEAD: usize = 20;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct CheckArgs {
    /// A URL to fetch, or the path of a local file to read.
    ///
    /// Anything parsing as an `http`/`https` URL is fetched; everything else is a path.
    #[arg(value_name = "TARGET")]
    pub target: String,

    /// Do not fetch the documents the page carries at their own address.
    ///
    /// Enclosures are followed by default because `collect` follows them, and a page whose
    /// own text is a print notice with the document one address away is the case most
    /// worth seeing.
    #[arg(long)]
    #[serde(default)]
    pub no_enclosures: bool,

    /// Write the files here instead of a fresh directory under `$TMPDIR`.
    ///
    /// For keeping two runs side by side to diff them after changing an extractor.
    #[arg(long, value_name = "DIR")]
    #[serde(default)]
    pub out: Option<String>,

    /// Print the whole extracted text to stdout rather than its first lines.
    #[arg(long)]
    #[serde(default)]
    pub print: bool,

    /// Lines of extracted text to show inline.
    #[arg(long, default_value_t = DEFAULT_HEAD)]
    #[serde(default = "default_head")]
    pub head: usize,

    /// User-Agent header. A descriptive one measurably reduces WAF 403s.
    #[arg(long, default_value = DEFAULT_USER_AGENT)]
    #[serde(default = "default_ua")]
    pub user_agent: String,

    /// Per-request timeout in seconds.
    #[arg(long, default_value_t = 30)]
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Also ask what **enumeration** would make of this address.
    ///
    /// `--strategy` alone asks the registry who recognises it. `--strategy=listing` forces
    /// one, which is how you find out *why* a recogniser said nothing. Either way the
    /// strategy runs and the addresses it found are reported.
    ///
    /// The `=` is required when naming one. Without it `--strategy https://x.gov/` would
    /// read the address as the strategy's name and then complain that no target was given,
    /// which is a confusing answer to a reasonable thing to type.
    ///
    /// Off by default. Extraction is the question `check` was built for, and enumeration
    /// costs requests that a person asking about one document did not ask for.
    #[arg(
        long,
        value_name = "NAME",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = AUTO,
    )]
    #[serde(default)]
    pub strategy: Option<String>,
}

/// `--strategy` with no name: ask the registry rather than naming one.
const AUTO: &str = "auto";

/// Requests one `check --strategy` may spend, and addresses it will keep.
///
/// Far below a real run's, because this is a probe. Pointing it at a directory index would
/// otherwise walk the tree — `publicrec.hillsclerk.com` is ~1,500 files — to answer a
/// question about the first page. A strategy that runs out says so in its own warning,
/// which is printed, so a truncated probe never reads like a small site.
const PROBE_REQUESTS: usize = 25;
const PROBE_ADDRESSES: usize = 500;

fn default_head() -> usize {
    DEFAULT_HEAD
}
fn default_ua() -> String {
    DEFAULT_USER_AGENT.to_string()
}
fn default_timeout() -> u64 {
    30
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// One document, as served and as extracted.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Checked {
    pub address: String,
    /// True for a document the page carried rather than the address that was typed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub enclosed: bool,

    pub bytes: usize,
    /// The `content-type` the server declared, or the one inferred from a local file's
    /// extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_type: Option<String>,
    /// True when `declared_type` came from a filename rather than from a server. The two
    /// are not the same evidence.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub type_inferred: bool,
    /// What [`ContentKind::classify`] decided, which is what picks the extractor.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<String>,
    /// Where the bytes really came from, when a redirect moved it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_url: Option<String>,

    /// The bytes as served, on disk. Open this to see what the extractor was given.
    pub as_served: PathBuf,
    /// The extracted text, on disk. Absent when nothing could be extracted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracted: Option<PathBuf>,

    /// Which reader spoke, with its version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// The document's own name, as the extractor found it. Written into the text as an
    /// `# H1`, so its absence is a finding rather than a blank field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub chars: usize,
    /// What we think of the read.
    ///
    /// A count of characters is not an opinion about them. This command reported
    /// `hillsclerk.com/marriage-license-application-success-kiosk` as 23,213 characters
    /// with a title and a tool, and every one of those facts was true of a page that is
    /// 84% navigation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    /// The primary reader found no text and the fallback did.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recovered_by_fallback: bool,
    /// Pages that are scans no reader here can decode.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pages_needing_ocr: usize,
    /// Anything the extractor wanted a reader of its output to know.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Why nothing came out. A fact about the format, not a fault in the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unextractable: Option<String>,

    /// The first `--head` lines of the extracted text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub preview: String,
    /// The whole text, with `--print`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CheckReport {
    /// What was typed.
    pub target: String,
    /// Where the files were written.
    pub dir: PathBuf,
    /// True when `dir` is a temporary directory this run created.
    pub temporary: bool,
    pub documents: Vec<Checked>,
    /// Documents the page named that this run did not fetch — dropped by the per-page cap,
    /// or refused. A silent cap reads exactly like a page that carried nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_fetched: Vec<String>,
    /// What `--strategy` found. Absent unless it was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enumeration: Option<Enumerated>,
    /// The command this platform opens a file with.
    pub opener: String,
    pub elapsed_secs: f64,
}

/// What enumeration made of the address, as a probe rather than a run.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Enumerated {
    /// The strategy that ran. With `--strategy` alone this is what the registry chose,
    /// and with a name it is what was forced.
    pub strategy: String,
    /// Whether the strategy that ran also **recognised** the address.
    ///
    /// False is the interesting answer, and it has two shapes: a forced strategy that
    /// does not fit, or nothing recognising anything and the sitemap fallback running
    /// because it is the best available guess. The warnings say which.
    pub recognised: bool,
    /// How many addresses came back. A count, because the sample below is a sample.
    pub addresses: usize,
    /// The first few, to read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sample: Vec<String>,
    /// The strategy's own account of itself — the recognition evidence first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    /// Everything that would explain a wrong count, including a probe that ran out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Addresses printed before the list is cut off.
const SAMPLE: usize = 10;

/// See what extraction makes of one link or file. Nothing is stored.
///
/// Fetches the address (or reads the file), runs the same extractor a real run would, and
/// writes both the bytes as served and the text that came out into a temporary directory
/// — then tells you how to open each. Touches no store, so there is no `--root`.
///
/// `reach = "host"` because it reads filesystem paths and writes files on the machine it
/// runs on: over HTTP or MCP that is arbitrary file access, and on a schedule it is a 3am
/// run filling `$TMPDIR` on somebody's behalf. This is a thing a person asks for while
/// looking at the answer, which is exactly what `Reach::Host` means.
#[op(long_running, reach = "host", group = "corpus")]
pub async fn check(
    _ctx: &Ctx,
    args: CheckArgs,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<CheckReport> {
    let started = std::time::Instant::now();

    // The directory outlives this process on purpose: its whole output is files somebody
    // is about to open. `TempDir` is still what creates it, for the unique name and the
    // right permissions — `keep` is what stops the guard removing it on drop.
    let (dir, temporary) = match &args.out {
        Some(out) => {
            let dir = crate::config::expand_tilde(out);
            std::fs::create_dir_all(&dir)
                .map_err(|e| anyhow::anyhow!("creating {}: {e}", dir.display()))?;
            (dir, false)
        }
        None => (
            tempfile::Builder::new()
                .prefix("centinel-check-")
                .tempdir()?
                .keep(),
            true,
        ),
    };

    let (acquired, not_fetched) = match Target::of(&args.target) {
        Target::Url(url) => acquire_url(&url, &args, progress).await?,
        Target::File(path) => (vec![acquire_file(&path).await?], Vec::new()),
    };

    let total = acquired.len() as u64;
    let mut documents = Vec::with_capacity(acquired.len());

    for (i, item) in acquired.into_iter().enumerate() {
        // The item boundary: one document's two files are written before the next is read,
        // so stopping here leaves whole files rather than a half-written one.
        cancel.check()?;
        progress.step(item.address.clone(), i as u64, total);

        let kind = ContentKind::classify(&item.meta, &item.bytes);
        let stem = format!("{:02}-{}", i + 1, slug(&item.address));

        // The bytes land on disk before anything reads them, and with an extension that
        // matches what they *are* rather than what the address called them: every viewer
        // dispatches on the extension, so a PDF served from a URL ending `/view` opens in
        // a text editor unless this says otherwise.
        let as_served = dir.join(format!("{stem}.{}", extension_for(kind, &item)));
        tokio::fs::write(&as_served, &item.bytes)
            .await
            .map_err(|e| anyhow::anyhow!("writing {}: {e}", as_served.display()))?;

        let mut doc = Checked {
            address: item.address.clone(),
            enclosed: item.enclosed,
            bytes: item.bytes.len(),
            declared_type: item.meta.get("content-type").cloned(),
            type_inferred: item.type_inferred,
            kind: kind.to_string(),
            http_status: item.meta.get("http_status").cloned(),
            final_url: item
                .meta
                .get("final_url")
                .filter(|u| *u != &item.address)
                .cloned(),
            as_served: as_served.clone(),
            extracted: None,
            tool: None,
            title: None,
            chars: 0,
            verdict: None,
            recovered_by_fallback: false,
            pages_needing_ocr: 0,
            notes: Vec::new(),
            unextractable: None,
            preview: String::new(),
            text: None,
        };

        // The same call `extract` makes, including the fallback a PDF gets when the
        // primary reader comes back empty. It wants a path, and the bytes are already at
        // one — which is the other reason they are written first.
        let derived = extract::derive(
            kind,
            &item.bytes,
            &as_served,
            Some(&item.address),
            item.meta.get("title").map(String::as_str),
        )
        .await;
        doc.recovered_by_fallback = derived.recovered_by_fallback;

        let (extraction, pages_needing_ocr) = match derived.outcome {
            Extracted::Unextractable { reason } => {
                doc.unextractable = Some(reason);
                documents.push(doc);
                continue;
            }
            Extracted::Text(e) => (e, Vec::new()),
            Extracted::Partial {
                extraction,
                pages_needing_ocr,
            } => (extraction, pages_needing_ocr),
        };

        // `.md` because that is what the extractors produce and what the chunker reads:
        // the headings in this file are the heading path every chunk of this document
        // would carry, so it is worth opening in something that renders them.
        let extracted = dir.join(format!("{stem}.md"));
        tokio::fs::write(&extracted, extraction.text.as_bytes())
            .await
            .map_err(|e| anyhow::anyhow!("writing {}: {e}", extracted.display()))?;

        doc.extracted = Some(extracted);
        doc.tool = Some(format!("{} {}", extraction.tool, extraction.version));
        doc.title = extraction.title.clone();
        doc.chars = extraction.text.chars().count();
        doc.verdict = Some(Verdict::on(&item.bytes, &extraction.text));
        doc.pages_needing_ocr = pages_needing_ocr.len();
        doc.notes = extraction.notes.clone();
        doc.preview = extraction
            .text
            .lines()
            .take(args.head)
            .collect::<Vec<_>>()
            .join("\n");
        if args.print {
            doc.text = Some(extraction.text.clone());
        }
        documents.push(doc);
    }

    // After extraction, because extraction is the question this command was built for and
    // a strategy that runs long should not delay the answer somebody asked for.
    let enumeration = match (&args.strategy, Target::of(&args.target)) {
        (Some(name), Target::Url(url)) => Some(enumerate_with(name, &url, &args, progress).await?),
        // A local file has no host to enumerate, and no address for a relative link to
        // resolve against. Saying so beats reporting an empty result.
        (Some(_), Target::File(_)) => {
            anyhow::bail!("--strategy needs a URL: enumeration is a walk of a host")
        }
        (None, _) => None,
    };

    Ok(CheckReport {
        target: args.target,
        dir,
        temporary,
        documents,
        not_fetched,
        enumeration,
        opener: super::open::system_opener().to_string(),
        elapsed_secs: started.elapsed().as_secs_f64(),
    })
}

/// Runs one enumeration against the address and reports what it found.
///
/// Goes through [`SiteSource`] rather than calling the strategy directly, so this is the
/// path a real run takes: the same seed, the same recognition, the same fallback when
/// nothing speaks, and the same pacing. A probe that used its own shortcut would answer a
/// question nobody asked.
///
/// It costs one more fetch of the address than the extraction above, because enumeration
/// builds its own seed and the two are not the same bytes — a redirect, or a page served
/// differently to a second request, is a thing worth seeing rather than hiding.
async fn enumerate_with(
    name: &str,
    url: &str,
    args: &CheckArgs,
    progress: &Progress,
) -> anyhow::Result<Enumerated> {
    let named = match name {
        AUTO => None,
        other => Some(crate::strategies::by_name(other)?),
    };
    let id = SourceId::new("check".to_string())?;
    let policy = HostPolicy {
        user_agent: args.user_agent.clone(),
        timeout: std::time::Duration::from_secs(args.timeout_secs),
        ..Default::default()
    };
    let site = SiteSource::new(
        id,
        url,
        policy,
        crate::discovery::DiscoveryLimits {
            max_sitemaps: PROBE_REQUESTS,
            max_urls: PROBE_ADDRESSES,
        },
    )?
    .with_strategy(named);

    let found = site.enumerate(progress).await?;

    // `enumerate` writes exactly one `strategy` note, and marks it `Warn` when nothing
    // recognised the address. Reading the mark back is what keeps the two answers from
    // drifting apart — there is no second place that decides what "recognised" means.
    let recognised = found
        .notes
        .iter()
        .find(|n| n.label == "strategy")
        .is_some_and(|n| n.mark != Some(NoteMark::Warn));

    Ok(Enumerated {
        strategy: site.method().to_string(),
        recognised,
        addresses: found.resources.len(),
        sample: found
            .resources
            .iter()
            .take(SAMPLE)
            .map(|r| r.natural_key.clone())
            .collect(),
        notes: found.notes,
        warnings: found.warnings,
    })
}

// -----------------------------------------------------------------------------------------
// Acquisition
// -----------------------------------------------------------------------------------------

/// What the `TARGET` argument turned out to be.
enum Target {
    Url(String),
    File(PathBuf),
}

impl Target {
    /// A URL if it parses as `http`/`https`, otherwise a path.
    ///
    /// Scheme rather than "does this file exist": a mistyped path must report a missing
    /// file, not be handed to a fetcher that will report a DNS failure about it.
    fn of(target: &str) -> Self {
        match url::Url::parse(target) {
            Ok(u) if matches!(u.scheme(), "http" | "https") => Self::Url(target.to_string()),
            _ => Self::File(crate::config::expand_tilde(target)),
        }
    }
}

/// One thing to run through the extractor.
struct Item {
    address: String,
    bytes: Vec<u8>,
    meta: BTreeMap<String, String>,
    enclosed: bool,
    type_inferred: bool,
}

/// Fetches a URL the way `collect` does — through the real adapter.
///
/// [`SiteSource`] is what resolves enclosures and classifies refusals, so both are the
/// ones a real run would produce. The [`SourceId`] it is built with is inert: nothing here
/// writes a record, and the id never reaches disk.
async fn acquire_url(
    url: &str,
    args: &CheckArgs,
    progress: &Progress,
) -> anyhow::Result<(Vec<Item>, Vec<String>)> {
    let id = SourceId::new("check".to_string())?;
    let policy = HostPolicy {
        user_agent: args.user_agent.clone(),
        timeout: std::time::Duration::from_secs(args.timeout_secs),
        // One address, so no pacing is owed to anyone: the limiter exists to keep a
        // thousand-page crawl polite, and waiting a second before a single request buys a
        // host nothing.
        max_requests_per_second: 0.0,
        ..Default::default()
    };
    let site = SiteSource::new(id.clone(), url, policy, Default::default())?;
    let resource = Resource::new(id.clone(), url.to_string());

    let acquired = site
        .acquire(&resource, progress)
        .await
        .map_err(|refusal| anyhow::anyhow!("{url}: {} — {}", refusal.state, refusal.detail))?;

    // The Source's own account of what it could not deliver, read back through the trait
    // rather than reached for inside the adapter.
    let not_fetched: Vec<String> = site
        .remarks(&BTreeMap::new(), 1)
        .into_iter()
        .filter(|n| matches!(n.mark, Some(NoteMark::Warn)))
        .map(|n| n.detail)
        .collect();

    let mut out = Vec::with_capacity(acquired.len());
    for (i, a) in acquired.into_iter().enumerate() {
        // The page is what was typed; anything after it came out of the page.
        let enclosed = i > 0;
        if enclosed && args.no_enclosures {
            continue;
        }
        out.push(Item {
            address: a.resource.natural_key,
            bytes: a.fetched.bytes,
            meta: a.fetched.meta,
            enclosed,
            type_inferred: false,
        });
    }
    Ok((out, not_fetched))
}

/// Reads a local file into the same shape a fetch produces.
///
/// No enclosures: a file's relative links resolve against nothing, and following the
/// absolute ones would make a local check reach the network without being asked.
async fn acquire_file(path: &Path) -> anyhow::Result<Item> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    let mut meta = BTreeMap::new();
    // **Only when the bytes cannot answer.** Nothing served this file, so there is no
    // header — but a filename is a weaker claim than magic bytes, not a stronger one, and
    // `content_kind` consults a declared type *first*. Handing it one unconditionally
    // would let `report.txt` override a `%PDF-` header and read a budget as plain text,
    // which is the opposite of the tiebreak that exists because hosts mislabel constantly.
    //
    // So this fills the one gap it was written for: `.csv`, `.txt`, `.json` and `.xml`,
    // whose first bytes are indistinguishable from any other text, reach `other` on their
    // own and no extractor claims them.
    let inferred = match ContentKind::classify(&BTreeMap::new(), &bytes) {
        ContentKind::Other => ContentKind::declared_type_for_path(&absolute),
        _ => None,
    };
    if let Some(ct) = inferred {
        meta.insert("content-type".to_string(), ct.to_string());
    }

    Ok(Item {
        address: absolute.display().to_string(),
        bytes,
        meta,
        enclosed: false,
        type_inferred: inferred.is_some(),
    })
}

// -----------------------------------------------------------------------------------------
// Naming the files
// -----------------------------------------------------------------------------------------

/// A filename stem from an address: its last meaningful path segment.
///
/// Only ever cosmetic — the number in front of it is what makes a name unique — so an
/// address that yields nothing usable gets `document` rather than an error.
fn slug(address: &str) -> String {
    let tail = address
        .split(['?', '#'])
        .next()
        .unwrap_or(address)
        .trim_end_matches('/')
        .rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or("document");

    // The extension is decided from the bytes, so whatever the address called itself is
    // dropped here rather than kept beside a second one.
    let stem: String = Path::new(tail)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(tail)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    let trimmed = stem.trim_matches('-');
    match trimmed.is_empty() {
        true => "document".into(),
        // Long enough to recognise, short enough to type.
        false => trimmed.chars().take(48).collect(),
    }
}

/// The extension that makes a viewer open this file as what it is.
///
/// From the content kind rather than from the address, because the address is exactly what
/// gets it wrong: a CMS serves a PDF from a URL ending `/view`, and a file named for the
/// URL opens in a text editor showing `%PDF-1.7`. `zip-container` and `document` are as far
/// as classification got — see [`ContentKind`] — so they take the address's own extension
/// when it has one, and fall back to a container that at least opens.
fn extension_for(kind: ContentKind, item: &Item) -> String {
    use ContentKind::*;

    // What the kind itself says, where the kind is specific enough to be trusted. The
    // answers come from [`ContentKind::extension`] — the same table `materialize` writes
    // `current/` with — so the two cannot name the same blob differently. What differs is
    // only *when* the kind is trusted, which is this function's own business.
    //
    // Exhaustive on purpose: a new kind has to say which side of that line it falls on.
    let from_kind = match kind {
        Html | Pdf | Text | Csv | Json | Captions | Xml => Some(kind.extension(&item.bytes)),
        // Knowable from the bytes, and the one kind where guessing wrong means no player
        // opens it at all — so an unrecognised container defers to the address instead of
        // taking `materialize`'s commonest-container guess.
        Audio => crate::content::audio_container(&item.bytes),
        // As far as classification got. The kind alone would name every deck `.docx`, a
        // file PowerPoint refuses; the address carries what the bytes cannot.
        Document | Spreadsheet | ZipContainer | Markdown | Other => None,
    };
    if let Some(ext) = from_kind {
        return ext.to_string();
    }

    address_extension(&item.address)
        .map(str::to_string)
        .unwrap_or_else(|| {
            match kind {
                Spreadsheet => "xlsx",
                Document | ZipContainer => "docx",
                _ => "bin",
            }
            .to_string()
        })
}

/// The address's own extension, when it has a plausible one.
fn address_extension(address: &str) -> Option<&str> {
    let path = address.split(['?', '#']).next().unwrap_or(address);
    let ext = Path::new(path).extension()?.to_str()?;
    // A URL path segment can hold a dot without an extension following it — `/agenda.2026`
    // is a date, and naming a file `agenda.2026` opens it in nothing. Short, alphanumeric
    // and holding at least one letter is as far as a syntactic rule can get; the kinds
    // that matter are answered from the bytes above and never reach here.
    let plausible = (1..=5).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && ext.chars().any(|c| c.is_ascii_alphabetic());
    plausible.then_some(ext)
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// One block per document: what it is, what read it, what came out, and where to look.
///
/// The text goes on screen before the paths, because most of the time the first twenty
/// lines answer the question and nothing needs opening at all. The paths are the
/// escape hatch for when they do not.
impl Render for CheckReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.title(
            &render::truncate(&self.target, p.width().saturating_sub(12)),
            &render::duration(self.elapsed_secs),
        )?;

        p.nest(|p| {
            for (i, doc) in self.documents.iter().enumerate() {
                if i > 0 {
                    p.blank()?;
                }
                doc.render(p)?;
                p.nest(|p| self.commands_for(doc, p))?;
            }

            if !self.not_fetched.is_empty() {
                p.section("not fetched")?;
                for note in &self.not_fetched {
                    p.marked(Mark::Warn, p.paint(&render::one_line(note), Ink::Dim))?;
                }
            }

            if let Some(e) = &self.enumeration {
                p.blank()?;
                e.render(p)?;
            }

            if self.temporary {
                p.blank()?;
                let note = format!("files are in {} — nothing was stored", self.dir.display());
                p.wrapped(&note, Ink::Dim)?;
            }
            Ok(())
        })
    }
}

impl CheckReport {
    /// The two commands, spelled out ready to paste.
    ///
    /// Printed in full rather than relative to the directory named at the bottom: a
    /// command you have to assemble from two places on screen is one you retype wrong.
    fn commands_for(&self, doc: &Checked, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.blank()?;
        if let Some(extracted) = &doc.extracted {
            p.kv(
                "read",
                6,
                p.paint(&format!("less {}", extracted.display()), Ink::Bold),
            )?;
        }
        p.kv(
            "open",
            6,
            p.paint(
                &format!("{} {}", self.opener, doc.as_served.display()),
                Ink::Bold,
            ),
        )
    }
}

impl Render for Enumerated {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.section("enumeration")?;
        p.nest(|p| {
            let mark = match self.recognised {
                true => Mark::Ok,
                false => Mark::Warn,
            };
            p.marked(
                mark,
                p.paint(
                    &format!(
                        "{} — {} address(es)",
                        self.strategy,
                        render::count(self.addresses as u64)
                    ),
                    Ink::Bold,
                ),
            )?;

            // The recognition evidence, then whatever the strategy wanted said. Both
            // arrive as Notes, so a new strategy explains itself and edits nothing here.
            for note in &self.notes {
                let ink = match note.mark {
                    Some(NoteMark::Warn) | Some(NoteMark::Bad) => Ink::Plain,
                    _ => Ink::Dim,
                };
                p.kv(
                    &note.label,
                    12,
                    p.paint(&render::one_line(&note.detail), ink),
                )?;
            }

            for w in &self.warnings {
                p.marked(Mark::Warn, p.paint(&render::one_line(w), Ink::Dim))?;
            }

            if !self.sample.is_empty() {
                p.blank()?;
                for address in &self.sample {
                    p.wrapped(address, Ink::Dim)?;
                }
                // A sample that reads like a total is the same lie a silent cap tells.
                if self.addresses > self.sample.len() {
                    p.wrapped(
                        &format!("… and {} more", self.addresses - self.sample.len()),
                        Ink::Dim,
                    )?;
                }
            }
            Ok(())
        })
    }
}

impl Render for Checked {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let label = render::truncate(&self.address, p.width().saturating_sub(14));
        let head = match self.enclosed {
            // Named as an enclosure, because a PDF appearing under a page's check is a
            // finding about the page and looks like a second target otherwise.
            true => format!(
                "{} {}",
                p.paint("enclosed", Ink::Dim),
                p.paint(&label, Ink::Bold)
            ),
            false => p.paint(&label, Ink::Bold),
        };
        p.line(head)?;

        p.nest(|p| {
            // What it is, and what it said it was. The two disagreeing is the classifier
            // working: `.gov` servers routinely serve PDFs as application/octet-stream.
            let mut facts = vec![self.kind.clone(), render::bytes(self.bytes as u64)];
            if let Some(status) = &self.http_status {
                facts.push(format!("HTTP {status}"));
            }
            if let Some(declared) = &self.declared_type {
                facts.push(match self.type_inferred {
                    true => format!("{declared} (from the filename)"),
                    false => format!("served as {declared}"),
                });
            }
            p.wrapped(&facts.join("  ·  "), Ink::Dim)?;

            if let Some(final_url) = &self.final_url {
                let note = format!("redirected to {final_url}");
                p.wrapped(&note, Ink::Dim)?;
            }

            if let Some(reason) = &self.unextractable {
                p.marked(Mark::Bad, p.paint("no text came out", Ink::Plain))?;
                return p.nest(|p| p.wrapped(&render::one_line(reason), Ink::Dim));
            }

            let tool = self.tool.as_deref().unwrap_or("unknown");
            let mut read_by = format!("{tool}  ·  {} of text", render::count(self.chars as u64));
            // Stated whether or not it is a problem. A share nobody can see is a measure
            // nobody can calibrate, and this one was chosen from a corpus, not a guess.
            if let Some(v) = &self.verdict
                && v.links > 0
            {
                read_by.push_str(&format!("  ·  {:.0}% link text", v.link_share * 100.0));
            }
            // A read with a finding against it is not a tick. This line printed `✓` over
            // a page that is 84% navigation, and the tick was the whole problem.
            let poor = self.verdict.as_ref().is_some_and(Verdict::is_poor);
            let mark = match poor {
                true => Mark::Warn,
                false => Mark::Ok,
            };
            p.marked(mark, p.paint(&read_by, Ink::Dim))?;

            p.nest(|p| {
                if let Some(v) = &self.verdict {
                    for f in &v.findings {
                        p.marked(Mark::Warn, p.paint(f, Ink::Plain))?;
                    }
                }
                match &self.title {
                    Some(title) => p.line(p.paint(&format!("title  {title}"), Ink::Plain))?,
                    // The title is written into the text as an `# H1` and becomes every
                    // chunk's heading path, so its absence is a finding, not a blank.
                    None => p.marked(Mark::Warn, p.paint("no title found", Ink::Dim))?,
                }
                if self.recovered_by_fallback {
                    p.marked(
                        Mark::Warn,
                        p.paint(
                            "the primary reader found nothing; the fallback read it",
                            Ink::Dim,
                        ),
                    )?;
                }
                if self.pages_needing_ocr > 0 {
                    let text = format!(
                        "{} pages are scans no reader here can read",
                        render::count(self.pages_needing_ocr as u64)
                    );
                    p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
                }
                for note in &self.notes {
                    p.note(p.paint(&render::one_line(note), Ink::Dim))?;
                }
                Ok(())
            })?;

            // The text itself. `--print` gives all of it; otherwise the head, which is
            // where a bad extraction is visible — the chrome, the ligature soup, the
            // print notice where the document should be.
            let body = self.text.as_deref().unwrap_or(&self.preview);
            if !body.trim().is_empty() {
                p.blank()?;
                for line in body.lines() {
                    p.line(p.paint(&render::truncate(line, p.width()), Ink::Plain))?;
                }
                // Said only when there is more, so a short document prints no ellipsis.
                if self.text.is_none() && self.chars > self.preview.chars().count() {
                    p.line(p.paint("…  (--print for all of it)", Ink::Dim))?;
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    /// A store nothing should touch, and a directory to put files in. Every test here
    /// checks a local file: the fetched half runs through `SiteSource`, which has its own
    /// tests, and the half with no adapter behind it is the half worth pinning down.
    async fn fixture() -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).await.unwrap();
        (dir, Ctx::new(store))
    }

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> String {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path.display().to_string()
    }

    fn args(target: &str) -> CheckArgs {
        CheckArgs {
            target: target.to_string(),
            no_enclosures: false,
            out: None,
            print: false,
            head: default_head(),
            user_agent: default_ua(),
            timeout_secs: default_timeout(),
            strategy: None,
        }
    }

    const HTML: &[u8] = b"<html><head><title>Regular Council Meeting</title></head><body>\
        <article><h1>Regular Council Meeting</h1>\
        <h2>Consent Agenda</h2><p>Approval of the purchase order for street resurfacing on \
        Kennedy Boulevard in the amount of $1,240,000. The council will consider the item \
        without discussion unless a member asks that it be pulled.</p></article></body></html>";

    /// Two files, and the second holds what the extractor produced. That pair is the
    /// whole output of this command.
    #[tokio::test]
    async fn both_the_bytes_and_the_text_land_on_disk() {
        let (dir, ctx) = fixture().await;
        let path = write(&dir, "agenda.html", HTML);

        let report = check(&ctx, args(&path), &Progress::none(), &Cancel::none())
            .await
            .unwrap();
        assert_eq!(report.documents.len(), 1);
        let doc = &report.documents[0];

        assert_eq!(std::fs::read(&doc.as_served).unwrap(), HTML, "as served");

        let extracted = doc.extracted.as_ref().expect("html yields text");
        let text = std::fs::read_to_string(extracted).unwrap();
        assert!(text.contains("Kennedy Boulevard"), "{text}");
        assert_eq!(
            text.chars().count(),
            doc.chars,
            "the count matches the file"
        );
        assert_eq!(doc.title.as_deref(), Some("Regular Council Meeting"));
        assert!(doc.tool.is_some());
        assert!(report.temporary);
    }

    /// **The promise.** `check` answers what extraction makes of something without
    /// deciding to keep it, so a run of experiments must leave nothing behind — no
    /// Observation, no blob, no source the archive never asked for.
    #[tokio::test]
    async fn nothing_reaches_the_store() {
        let (dir, ctx) = fixture().await;
        let path = write(&dir, "agenda.html", HTML);

        check(&ctx, args(&path), &Progress::none(), &Cancel::none())
            .await
            .unwrap();

        assert!(ctx.store.sources().await.unwrap().is_empty(), "no source");
        assert_eq!(ctx.store.count_blobs().await.unwrap(), 0, "no blob");
        assert!(!ctx.store.index_path().exists(), "no index");
        assert!(!ctx.store.vectors_path().exists(), "no vectors");
    }

    /// The extension is what every viewer dispatches on, so it has to say what the bytes
    /// *are*. A CMS serving a PDF from a URL ending `/view` is the case that gets it
    /// wrong, and a file named for the address opens in a text editor showing `%PDF-1.7`.
    #[tokio::test]
    async fn the_extension_comes_from_the_bytes_not_the_name() {
        let (dir, ctx) = fixture().await;
        // Named `.txt`, and a PDF.
        let path = write(
            &dir,
            "report.txt",
            b"%PDF-1.4\n1 0 obj<</Type/Catalog>>endobj\n",
        );

        let report = check(&ctx, args(&path), &Progress::none(), &Cancel::none())
            .await
            .unwrap();
        let doc = &report.documents[0];
        assert_eq!(doc.kind, "pdf", "magic bytes beat the filename");
        assert_eq!(
            doc.as_served.extension().and_then(|e| e.to_str()),
            Some("pdf"),
            "so it opens in a PDF viewer, not an editor"
        );
        assert!(
            !doc.type_inferred,
            "the bytes answered; nothing was guessed"
        );
    }

    /// The one gap an extension fills: a `.csv`'s first bytes are indistinguishable from
    /// any other text, so magic bytes alone reach `other` and no extractor claims it.
    #[tokio::test]
    async fn an_extension_answers_only_where_the_bytes_cannot() {
        let (dir, ctx) = fixture().await;
        let path = write(&dir, "districts.csv", b"district,population\nEast,41200\n");

        let report = check(&ctx, args(&path), &Progress::none(), &Cancel::none())
            .await
            .unwrap();
        let doc = &report.documents[0];
        assert_eq!(doc.kind, "csv");
        assert!(doc.type_inferred, "and the report says it was a guess");
        assert_eq!(doc.declared_type.as_deref(), Some("text/csv"));
    }

    /// Nothing to extract is a fact about the format, and it leaves no text file to
    /// promise one — a path printed for a file that does not exist is worse than none.
    #[tokio::test]
    async fn nothing_extractable_writes_no_text_file_and_says_why() {
        let (dir, ctx) = fixture().await;
        // Ogg magic. `content_kind` sniffs these as audio, which no extractor claims.
        let path = write(
            &dir,
            "meeting.bin",
            b"OggS\x00\x02 pretend this is three hours",
        );

        let report = check(&ctx, args(&path), &Progress::none(), &Cancel::none())
            .await
            .unwrap();
        let doc = &report.documents[0];
        assert_eq!(doc.kind, "audio");
        assert!(doc.extracted.is_none());
        assert!(doc.unextractable.is_some());
        // The bytes are still written: seeing what the extractor was handed is half the
        // point, and never more so than when it made nothing of it.
        assert!(doc.as_served.exists());
        assert_eq!(
            doc.as_served.extension().and_then(|e| e.to_str()),
            Some("ogg"),
            "the container is knowable from the bytes, and a wrong one plays nowhere"
        );
    }

    /// `--out` is how two runs sit side by side to be diffed after changing an extractor.
    #[tokio::test]
    async fn out_writes_where_it_is_told() {
        let (dir, ctx) = fixture().await;
        let path = write(&dir, "agenda.html", HTML);
        let out = dir.path().join("somewhere/else");

        let report = check(
            &ctx,
            CheckArgs {
                out: Some(out.display().to_string()),
                ..args(&path)
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();

        assert_eq!(report.dir, out);
        assert!(
            !report.temporary,
            "a named directory is not this run's to remove"
        );
        assert!(report.documents[0].as_served.starts_with(&out));
    }

    /// A path that is not there is a missing file, not a hostname. Handing it to a
    /// fetcher would report a DNS failure about a typo.
    #[tokio::test]
    async fn a_missing_path_says_so() {
        let (_d, ctx) = fixture().await;
        let err = check(
            &ctx,
            args("/nowhere/at/all.pdf"),
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .map(|_| ())
        .unwrap_err()
        .to_string();
        assert!(err.contains("all.pdf"), "{err}");
    }

    /// Extraction is what this command is for. Enumeration costs requests, so it happens
    /// only when it is asked for.
    #[tokio::test]
    async fn nothing_is_enumerated_unless_a_strategy_is_asked_for() {
        let (d, ctx) = fixture().await;
        let target = write(&d, "page.html", HTML);
        let report = check(&ctx, args(&target), &Progress::none(), &Cancel::none())
            .await
            .unwrap();
        assert!(report.enumeration.is_none());
    }

    /// A local file has no host to walk and no address for a relative link to resolve
    /// against, so an empty result would be a wrong answer rather than a small one.
    #[tokio::test]
    async fn a_strategy_probe_against_a_local_file_says_why_it_cannot() {
        let (d, ctx) = fixture().await;
        let target = write(&d, "page.html", HTML);
        let err = check(
            &ctx,
            CheckArgs {
                strategy: Some(AUTO.into()),
                ..args(&target)
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .map(|_| ())
        .unwrap_err()
        .to_string();
        assert!(err.contains("needs a URL"), "{err}");
    }

    /// The name is checked before anything is fetched, and the error says what exists —
    /// a typo must not read like a site that enumerates to nothing.
    #[tokio::test]
    async fn an_unknown_strategy_names_the_ones_this_build_has() {
        let err = crate::strategies::by_name("onbase")
            .map(|_| ())
            .unwrap_err()
            .to_string();
        assert!(err.contains("listing") && err.contains("sitemap"), "{err}");
    }

    #[test]
    fn a_url_is_fetched_and_everything_else_is_a_path() {
        assert!(matches!(Target::of("https://tampa.gov/a"), Target::Url(_)));
        assert!(matches!(Target::of("http://tampa.gov/a"), Target::Url(_)));
        assert!(matches!(Target::of("./agenda.pdf"), Target::File(_)));
        assert!(matches!(Target::of("/srv/agenda.pdf"), Target::File(_)));
        // Not a scheme this fetches, so it is a path — and will fail as one.
        assert!(matches!(Target::of("ftp://x/a"), Target::File(_)));
    }

    /// The name only has to be recognisable and typeable; uniqueness is the number in
    /// front of it. So an address with nothing usable in it must still yield something.
    #[test]
    fn a_filename_is_recovered_from_whatever_the_address_offers() {
        assert_eq!(
            slug("https://tampa.gov/document/hurricane-guide"),
            "hurricane-guide"
        );
        assert_eq!(slug("https://tampa.gov/files/agenda.pdf"), "agenda");
        // A query string is a viewer instruction, not part of the name.
        assert_eq!(slug("https://x.gov/view?id=4102&page=2"), "view");
        assert_eq!(slug("https://tampa.gov/budget/"), "budget");
        assert_eq!(slug("/srv/corpus/minutes.docx"), "minutes");
        // A bare origin has only its host to offer, which is still recognisable.
        assert_eq!(slug("https://tampa.gov"), "tampa");
        assert_eq!(slug("https://tampa.gov/"), "tampa");
        assert_eq!(slug("///"), "document");
    }

    #[test]
    fn an_extension_that_is_not_one_is_not_taken_for_one() {
        assert_eq!(
            address_extension("https://x.gov/a/minutes.pdf"),
            Some("pdf")
        );
        assert_eq!(
            address_extension("https://x.gov/a/report.pdf?v=2"),
            Some("pdf")
        );
        // A date, not an extension. Naming a file `agenda.2026` opens it in nothing.
        assert_eq!(address_extension("https://x.gov/agenda.2026"), None);
        assert_eq!(address_extension("https://x.gov/view"), None);
        // Too long to be one — a path segment that merely contains a dot.
        assert_eq!(address_extension("https://x.gov/a.presentation"), None);
    }
}
