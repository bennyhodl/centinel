//! Configuration.
//!
//! SPEC §1 calls Centinel "generic, config-driven — Tampa is the first config file, not
//! an assumption". This module holds what is true of a *machine*: where its store is,
//! its defaults, how it opens files, and the `[[schedule]]` blocks it fires. What the
//! corpus is made of — its sources — lives in the store; see [`crate::catalog`].
//!
//! ## Why config is looked up outside the store
//!
//! Which application opens a PDF is a property of *your machine*, not of the corpus. The
//! store is `rsync`-able and meant to be handed to other people (SPEC §5.4); baking one
//! operator's choice of PDF reader into it would travel badly.
//!
//! This module is also where the store root is decided, which is the one thing config
//! says about the store rather than about the machine or the corpus. See [`default_root`]
//! for why the answer is in `$HOME` and not in the working directory.
//!
//! Sources used to live here, as `[[source]]` blocks, on the argument that a source is an
//! intent to collect and a recipient of your store should not inherit one. Two lists
//! drifted instead — the config's and the store's — so sources moved into the store, and
//! the intent that fires on its own, a `[[schedule]]`, stayed here. A legacy block is
//! still parsed, so [`crate::catalog::Sources::import`] can carry it into the store once.
//!
//! ## Why writes are textual rather than a serialize round-trip
//!
//! `centinel schedule add` edits a file a person wrote and will read again. Serializing a
//! [`Config`] back over it would parse-and-reprint — silently deleting every comment and
//! reordering every key. So [`append_schedule`] appends and [`remove_schedule`] excises
//! lines, and both re-parse the result to prove the edit did what it claimed before it
//! reaches disk.

use std::path::{Path, PathBuf};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The sentinel meaning "let the operating system decide".
pub const SYSTEM_DEFAULT: &str = "system";

/// The config file created when none exists.
pub const DEFAULT_FILENAME: &str = "centinel.toml";

/// The store directory, relative to `$HOME` unless something names another root.
pub const DEFAULT_DIRNAME: &str = ".centinel";

/// `$HOME`, or `None` where there is none to have.
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Expands a leading `~/`, and nothing else.
///
/// A shell expands `~` before the process sees it; a path read out of a TOML file never
/// went through one, and `root = "~/corpora"` would otherwise create a directory called
/// `~`. `~user` is deliberately not handled — resolving another account's home means
/// reading the password database, and it is not a thing anyone writes here.
pub fn expand_tilde(path: &str) -> PathBuf {
    let Some(home) = home() else {
        return PathBuf::from(path);
    };
    match path.strip_prefix('~') {
        Some("") => home,
        Some(rest) => match rest.strip_prefix('/') {
            Some(rest) => home.join(rest),
            // `~foo` is a literal directory name, not a home to expand.
            None => PathBuf::from(path),
        },
        None => PathBuf::from(path),
    }
}

/// The store root when nothing names one: `~/.centinel`.
///
/// In `$HOME` rather than the working directory, because a store is a corpus you keep,
/// not an artefact of the directory you happened to be standing in. A working-directory
/// default put a separate `.centinel` under every shell that ever ran the binary — each
/// one its own blob pool, its own log and its own index, none of them answering a search
/// against the others, and all of them invisible until somebody noticed the corpus was
/// empty from one directory up.
///
/// Falls back to a relative `.centinel` only where there is no home to put it in.
pub fn default_root() -> PathBuf {
    match home() {
        Some(home) => home.join(DEFAULT_DIRNAME),
        None => PathBuf::from(DEFAULT_DIRNAME),
    }
}

/// Where a config file is written when none exists: `~/.centinel/centinel.toml`.
///
/// Beside the *default* store, not beside the configured one. The config is what says
/// where the store is, so looking for it under a root it has not been read to name yet
/// is circular.
pub fn default_config_path() -> PathBuf {
    default_root().join(DEFAULT_FILENAME)
}

/// Unknown keys are rejected rather than ignored.
///
/// The alternative is worse than it sounds for a config-driven tool: `[[sources]]` with
/// the plural typed by reflex would parse cleanly, contribute nothing, and leave
/// `centinel run` reporting "no sources configured" at someone looking straight at the
/// source they just added.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Where the store lives. `~/` is expanded; a relative path is relative to the
    /// working directory. Absent means [`default_root`].
    ///
    /// Declared first because TOML puts every top-level key above the first table, so
    /// this is where a reader will look for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,

    #[serde(default)]
    pub open: OpenConfig,

    #[serde(default)]
    pub defaults: Defaults,

    /// `[inference]` — the servers that embed, rerank and transcribe. See
    /// [`crate::inference`].
    #[serde(default)]
    pub inference: InferenceConfig,

    /// `[classify]` — the Jev model `centinel classify` sends documents to when the
    /// command names none.
    ///
    /// Classifying is never scheduled: it sends corpus text off the machine, and only a
    /// person starts it, from the command or the web's Classify button.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classify: Option<ClassifyConfig>,

    /// Legacy `[[source]]` blocks. Read only so [`crate::catalog::Sources::import`] can
    /// carry them into the store; nothing else reads them, because the store owns the
    /// list now.
    ///
    /// `rename` because TOML spells a list of tables `[[source]]`.
    #[serde(default, rename = "source", skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceConfig>,

    /// The cadences `centinel serve` fires runs on, in file order.
    ///
    /// In this file rather than the store because **this is where the authority to
    /// collect on its own comes from**. A scheduled run is not the server deciding to
    /// crawl; it is this file, which only the operator can write, executed later. Nothing
    /// arriving over HTTP or MCP can add a block here, which is the whole of the access
    /// story in `docs/SCHEDULING.md` §1.1.
    #[serde(default, rename = "schedule")]
    pub schedules: Vec<ScheduleConfig>,
}

/// The `[classify]` block: what `centinel classify` sends documents to.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassifyConfig {
    /// The model, such as `jev-1.13.0`. `centinel classify --model` overrides it for one
    /// run.
    pub model: String,

    /// Documents in flight to Jev at once. Eight unless set; at most thirty-two.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<usize>,
}

/// Which application opens which kind of document.
///
/// Keys are the content kinds from [`crate::content::ContentKind`] — `pdf`, `html`,
/// `markdown`, `spreadsheet`, `document`, `text`, `json`, `captions`, `audio`.
///
/// A value is either an **application name** (`"Adobe Acrobat"`) or a **command
/// template** containing `{path}` (`"nvim {path}"`). The distinction is the presence of
/// `{path}`, so the common case stays a bare name.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OpenConfig {
    #[serde(flatten)]
    pub by_kind: BTreeMap<String, String>,
}

impl OpenConfig {
    /// The opener for a kind, falling back to `default`, then to the system handler.
    pub fn opener_for(&self, kind: &str) -> &str {
        self.by_kind
            .get(kind)
            .or_else(|| self.by_kind.get("default"))
            .map(String::as_str)
            .unwrap_or(SYSTEM_DEFAULT)
    }
}

/// Settings every source inherits unless it overrides them.
///
/// Present so a config with twenty sources states the crawl rate once. Each field has a
/// matching `Option` on [`SourceConfig`]; the resolution is always "source, else this".
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    /// Requests per second, per host. Deliberately slow.
    #[serde(default = "default_rps")]
    pub rps: f64,

    /// Caption and transcription language.
    #[serde(default = "default_lang")]
    pub lang: String,
}

fn default_rps() -> f64 {
    1.0
}
fn default_lang() -> String {
    "en".to_string()
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            rps: default_rps(),
            lang: default_lang(),
        }
    }
}

/// The `[inference]` block: where models run. Every capability uses `base_url` unless its
/// own table names another, and the same for `api_key_env`.
///
/// Absent, it points at a server on this machine on llama-server's default port — the
/// setup the install docs walk through — so a machine with nothing configured fails with
/// "nothing is answering at http://127.0.0.1:8080/v1" rather than with a missing key.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceConfig {
    #[serde(default = "default_base_url")]
    pub base_url: String,

    /// The environment variable holding the API key, for a server that wants one.
    ///
    /// The variable's name, never the key: this file is committed, pasted into issues and
    /// printed by `doctor`, and a secret that lives here eventually does all three.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,

    #[serde(default)]
    pub embed: EmbedConfig,

    #[serde(default = "default_rerank")]
    pub rerank: ModelConfig,

    #[serde(default = "default_transcribe")]
    pub transcribe: ModelConfig,
}

/// `[inference.embed]`.
///
/// A written table must name `model` and `query_prefix` both: the prefix is a property of
/// the model, and a default carried over from another model is retrieval made quietly
/// worse — see [`crate::embed`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbedConfig {
    /// The model the server is asked for. Changing it is a new vector space: `embed`
    /// refuses to mix it into the stored one until `--adopt` has measured they agree.
    pub model: String,

    /// What a search query is prefixed with. `""` for a symmetric model.
    pub query_prefix: String,

    /// Chunks per request. `--batch` overrides it for one run.
    #[serde(default = "default_embed_batch")]
    pub batch: usize,

    /// Requests in flight at once. What hides the round trip; more mostly exercises the
    /// server's queue.
    #[serde(default = "default_embed_concurrency")]
    pub concurrency: usize,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
}

/// `[inference.rerank]` and `[inference.transcribe]`: a model, and optionally its own
/// server.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub model: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
}

/// Where a server started with no port flag listens: llama-server's default.
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8080/v1";

/// Requests in flight are capped here, past which a run is load-testing the server.
pub const MAX_EMBED_CONCURRENCY: usize = 32;

fn default_base_url() -> String {
    DEFAULT_BASE_URL.to_string()
}
fn default_embed_batch() -> usize {
    64
}
fn default_embed_concurrency() -> usize {
    4
}
fn default_rerank() -> ModelConfig {
    ModelConfig {
        model: "qwen3-reranker-0.6b".to_string(),
        base_url: None,
        api_key_env: None,
    }
}
fn default_transcribe() -> ModelConfig {
    ModelConfig {
        model: "whisper-large-v3-turbo".to_string(),
        base_url: None,
        api_key_env: None,
    }
}

impl Default for EmbedConfig {
    fn default() -> Self {
        Self {
            model: "qwen3-embedding-4b".to_string(),
            query_prefix: crate::embed::QWEN3_QUERY_PREFIX.to_string(),
            batch: default_embed_batch(),
            concurrency: default_embed_concurrency(),
            base_url: None,
            api_key_env: None,
        }
    }
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            api_key_env: None,
            embed: EmbedConfig::default(),
            rerank: default_rerank(),
            transcribe: default_transcribe(),
        }
    }
}

impl InferenceConfig {
    /// Every URL parses and every count is in range — checked when the file is read, not
    /// an hour into a run at the first request.
    fn validate(&self) -> anyhow::Result<()> {
        let urls = [
            Some(&self.base_url),
            self.embed.base_url.as_ref(),
            self.rerank.base_url.as_ref(),
            self.transcribe.base_url.as_ref(),
        ];
        for url in urls.into_iter().flatten() {
            let parsed = url::Url::parse(url)
                .map_err(|e| anyhow::anyhow!("[inference] base_url `{url}` is not a URL: {e}"))?;
            anyhow::ensure!(
                matches!(parsed.scheme(), "http" | "https"),
                "[inference] base_url `{url}` is not http or https"
            );
        }
        anyhow::ensure!(
            self.embed.batch > 0,
            "[inference.embed] batch = 0 embeds nothing"
        );
        anyhow::ensure!(
            (1..=MAX_EMBED_CONCURRENCY).contains(&self.embed.concurrency),
            "[inference.embed] concurrency must be 1 to {MAX_EMBED_CONCURRENCY}"
        );
        Ok(())
    }
}

/// The `[defaults]` keys that moved to `[inference]`, each with what to write instead.
///
/// Read off the raw table before the typed parse, because `deny_unknown_fields` would
/// otherwise answer a config that worked yesterday with "unknown field `embed_model`" —
/// true, and no help to the person who has to rewrite it.
fn retired_keys(text: &str) -> anyhow::Result<()> {
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        // The typed parse reports a malformed file better than this would.
        return Ok(());
    };
    let Some(defaults) = table.get("defaults").and_then(|d| d.as_table()) else {
        return Ok(());
    };
    let mut fixes = Vec::new();
    if let Some(model) = defaults.get("embed_model").and_then(|m| m.as_str()) {
        let replacement = match model.strip_prefix("openrouter/") {
            Some(slug) => embed_table(Some(OPENROUTER_BASE_URL), Some(OPENROUTER_KEY), slug),
            None => embed_table(None, None, model),
        };
        fixes.push(format!(
            "`embed_model` moved to [inference.embed]; the model now runs on a server:\n{replacement}"
        ));
    }
    if defaults.contains_key("embed_batch") {
        fixes.push(
            "`embed_batch` is gone: the server sizes its own batches. [inference.embed] \
             batch is chunks per request, 64 unless set"
                .to_string(),
        );
    }
    if let Some(model) = defaults.get("transcribe_model").and_then(|m| m.as_str()) {
        fixes.push(format!(
            "`transcribe_model` moved:\n[inference.transcribe]\nmodel = \"{model}\""
        ));
    }
    anyhow::ensure!(
        fixes.is_empty(),
        "[defaults] holds settings that moved to [inference] — remove them and write:\n\n{}",
        fixes.join("\n\n")
    );
    Ok(())
}

/// OpenRouter's API root and the variable its key conventionally lives in — what a store
/// embedded through an `openrouter/…` id is pointed back at.
pub(crate) const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
pub(crate) const OPENROUTER_KEY: &str = "OPENROUTER_API_KEY";

/// An `[inference.embed]` table to print for pasting — complete, so it parses.
///
/// The one writer of such a table, for the two messages that hand one over: a retired
/// `[defaults] embed_model`, and an `embed --adopt` refusal naming the server that wrote a
/// table. The query prefix is guessed from the model's name, which is the one thing a
/// message can do that the config file must not: the operator reads it before using it.
pub(crate) fn embed_table(
    base_url: Option<&str>,
    api_key_env: Option<&str>,
    model: &str,
) -> String {
    let mut out = String::from("[inference.embed]\n");
    if let Some(base_url) = base_url {
        out.push_str(&format!("base_url = {}\n", quote(base_url)));
    }
    if let Some(var) = api_key_env {
        out.push_str(&format!("api_key_env = {}\n", quote(var)));
    }
    let prefix = if model.contains("qwen3-embedding") {
        crate::embed::QWEN3_QUERY_PREFIX
    } else {
        ""
    };
    out.push_str(&format!(
        "model = {}\nquery_prefix = {}",
        quote(model),
        quote(prefix)
    ));
    out
}

/// One `[[source]]` block.
///
/// `site` and `channel` are the whole of the website/YouTube distinction, mirroring
/// SPEC §4.1: the two Source kinds are peers that differ only in **acquisition**, so the
/// config difference is one key and everything downstream is shared.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {
    /// Becomes a directory name under `log/`, so it is a [`crate::domain::SourceId`].
    pub id: String,

    /// Any URL on the site. Only the origin is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,

    /// A channel URL — `https://www.youtube.com/@CityofTampa`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,

    /// Skip this source without deleting the block. Defaults to enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// Overrides [`Defaults::rps`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rps: Option<f64>,

    /// Only collect addresses whose URL contains one of these substrings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matches: Vec<String>,

    /// Extra arguments for yt-dlp — the escape hatch for the bot wall.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub yt_dlp_args: Vec<String>,

    /// Fetch audio only for videos YouTube never captioned. Defaults on for a channel;
    /// see `centinel collect --help`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_if_no_captions: Option<bool>,

    /// Overrides [`Defaults::lang`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,

    /// The collection strategy the operator accepted for this source.
    ///
    /// Website sources only. Absent means *ask the registry every run*, which is the
    /// right default for a source added before any strategy existed.
    ///
    /// Present means the operator saw the evidence once and said yes. It is not a lock:
    /// the strategy is still asked to recognise the site on every run, and a strategy that
    /// stops recognising the address it was accepted for produces a **warning** rather
    /// than a silent switch to a weaker one. Sites change, and an empty corpus with a
    /// green run is the failure this key exists to make loud.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
}

/// One cadence, and the `run` it fires.
///
/// **A schedule is a saved `centinel run` invocation plus a cadence, and nothing more.**
/// It introduces no pipeline semantics: it does not decide what to collect, does not diff
/// anything, and keeps no state. Every stage already skips work it has done, so the
/// scheduler's entire job is to call `run` at the right moments.
///
/// ## Why the run options are spelled out rather than flattened
///
/// `#[serde(flatten)]` of [`crate::ops::run::RunArgs`] would keep this in step with `run`
/// automatically — and would silently disable `deny_unknown_fields`, which serde cannot
/// apply to a struct containing a flattened field. That trade is wrong in this file. A
/// `soruces = ["tampa"]` typo would then parse cleanly and produce a schedule that fires
/// on time, collects nothing, and reports success forever — the same class of silence
/// `[[sources]]` typed by reflex already produces, and the reason this module denies
/// unknown keys at all.
///
/// The cost is a line here when `run` grows a flag. `run_options_stay_in_step_with_run`
/// is the test that turns that into a failure rather than an omission.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleConfig {
    /// Names this schedule in the journal, in reports, and on the command line.
    pub id: String,

    /// A 5-field cron expression, or a `@daily` shorthand.
    pub cron: String,

    /// IANA zone name — `America/New_York`, not `-05:00`.
    ///
    /// `.gov` publishing is a business-hours phenomenon in a specific city and an operator
    /// reasons in local time. A **name** rather than an offset so the schedule keeps
    /// meaning "3am there" across a DST boundary; absent, the host zone is used and
    /// recorded, so a server that changes machines does not quietly shift its collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tz: Option<String>,

    /// How far after the nominal time this may fire. Defaults to five minutes.
    ///
    /// The licence is MIT and forks are the point — other cities run their own instance.
    /// Twenty installs sharing a default `0 3 * * *`, against the handful of vendor
    /// platforms `.gov` sites share, is a synchronised flood from a project whose stated
    /// stance is politeness. The offset is deterministic per install, so the fire time
    /// stays predictable to its own operator; see [`crate::schedule::jitter_offset`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter_secs: Option<u64>,

    /// Skip this schedule without deleting the block. Defaults to enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// Fire once on startup when the last attempt is older than one interval. Defaults on.
    ///
    /// **Once, never a backlog.** Six missed daily fires are one fire, because the
    /// pipeline is a subtraction and not a queue of deltas: six catch-up runs would do the
    /// same work once and then nothing five times, in a burst, against a city's server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catch_up: Option<bool>,

    // ── the `run` invocation ──────────────────────────────────────────────────
    /// Sources to run. Empty means every enabled source, exactly as a bare `run` does.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,

    /// Stages to skip. `skip = ["embed"]` is the common one — collect often, embed at
    /// night; the peer block that skips discover and collect does the derivation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip: Vec<String>,

    /// Stop collection after this many addresses, per source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,

    /// Redo work already done, at every fire.
    ///
    /// Legal, because "the extractor improved — re-read everything monthly" is a real
    /// intent. It is also the single most expensive thing this file can express, so
    /// `schedules` gives it its own column and the loader says so at startup. Refusing it
    /// would push the operator into a shell script, where nothing renders it at all.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub refresh: bool,
}

/// How a source is acquired — the one axis on which the Source kinds differ.
///
/// The **only** enum in the codebase that names the kinds. Matching on it belongs here
/// and in [`crate::sources::from_config`], which turns one into a live
/// [`crate::domain::Source`]; anywhere else, ask the Source. That rule is what makes a
/// third kind a new file rather than a hunt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Acquisition<'a> {
    /// Sitemap walk, then HTTP GETs.
    Site(&'a str),
    /// Playlist listing, then `yt-dlp`.
    Channel(&'a str),
}

impl Acquisition<'_> {
    /// The label this reads as, without building anything that could touch a network.
    ///
    /// Present so `source list` can name twenty sources without constructing twenty HTTP
    /// clients to ask each one what it is.
    pub fn kind(&self) -> crate::domain::SourceKind {
        use crate::domain::SourceKind;
        match self {
            Self::Site(_) => SourceKind::Site,
            Self::Channel(_) => SourceKind::Channel,
        }
    }

    /// The address, whichever key carried it.
    pub fn target(&self) -> &str {
        match self {
            Self::Site(url) | Self::Channel(url) => url,
        }
    }
}

impl SourceConfig {
    /// A website source.
    pub fn site(id: impl Into<String>, site: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            site: Some(site.into()),
            ..Default::default()
        }
    }

    /// A YouTube channel source.
    pub fn channel(id: impl Into<String>, channel: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            channel: Some(channel.into()),
            ..Default::default()
        }
    }

    /// Rejects a block that names neither or both.
    ///
    /// Both is an error rather than a precedence rule: a block carrying a site *and* a
    /// channel is someone editing one source into another and stopping halfway, and
    /// picking a winner would collect half of what they meant under a name that says the
    /// other thing.
    pub fn acquisition(&self) -> anyhow::Result<Acquisition<'_>> {
        match (&self.site, &self.channel) {
            (Some(site), None) => Ok(Acquisition::Site(site)),
            (None, Some(channel)) => Ok(Acquisition::Channel(channel)),
            (None, None) => anyhow::bail!(
                "source `{}` has neither `site` nor `channel` — one of them says how to \
                 acquire it",
                self.id
            ),
            (Some(_), Some(_)) => anyhow::bail!(
                "source `{}` has both `site` and `channel`; a source is one or the other",
                self.id
            ),
        }
    }

    /// Refuses a definition no run could use: no `site` or `channel` (or both), or a rate
    /// that is not positive. The one check, for a `[[source]]` block and for a definition
    /// the store is about to write.
    pub fn validate(&self) -> anyhow::Result<()> {
        self.acquisition()?;
        if let Some(rps) = self.rps {
            anyhow::ensure!(
                rps.is_finite() && rps > 0.0,
                "source `{}`: rps must be finite and greater than zero",
                self.id
            );
        }
        Ok(())
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    /// The host this source collects from: `tampa.gov` for `https://tampa.gov/a`, and
    /// `www.youtube.com` for a channel. What the web shows an icon for.
    pub fn host(&self) -> Option<String> {
        crate::crumbs::host_of(self.acquisition().ok()?.target())
    }

    /// The host of the site this source collects, which a channel does not have:
    /// `youtube.com` is not a channel's. What a crumb pointing there is answered by
    /// ([`crate::crumbs::claimants`]).
    pub fn site_host(&self) -> Option<String> {
        self.site.as_deref().and_then(crate::crumbs::host_of)
    }
}

/// The jitter every schedule gets unless it says otherwise. See [`ScheduleConfig::jitter_secs`].
pub const DEFAULT_JITTER_SECS: u64 = 300;

impl ScheduleConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    pub fn catches_up(&self) -> bool {
        self.catch_up.unwrap_or(true)
    }

    pub fn jitter_secs(&self) -> u64 {
        self.jitter_secs.unwrap_or(DEFAULT_JITTER_SECS)
    }

    /// The parsed cadence.
    pub fn cron(&self) -> anyhow::Result<crate::schedule::Cron> {
        crate::schedule::Cron::parse(&self.cron)
            .map_err(|e| anyhow::anyhow!("schedule `{}`: {e}", self.id))
    }

    /// The zone this fires in, defaulting to the host's.
    ///
    /// A name that does not resolve is an error rather than a fallback to UTC: silently
    /// collecting five hours from when the operator meant is worse than not starting.
    pub fn zone(&self) -> anyhow::Result<jiff::tz::TimeZone> {
        match &self.tz {
            Some(name) => jiff::tz::TimeZone::get(name).map_err(|e| {
                anyhow::anyhow!(
                    "schedule `{}`: `{name}` is not an IANA time zone: {e}",
                    self.id
                )
            }),
            None => Ok(jiff::tz::TimeZone::system()),
        }
    }

    /// The `run` invocation this schedule stands for.
    pub fn run_args(&self) -> anyhow::Result<crate::ops::RunArgs> {
        let mut skip = Vec::with_capacity(self.skip.len());
        for name in &self.skip {
            skip.push(parse_stage(name).ok_or_else(|| {
                anyhow::anyhow!(
                    "schedule `{}`: `{name}` is not a stage; expected one of: {}",
                    self.id,
                    STAGE_NAMES.join(", ")
                )
            })?);
        }
        Ok(crate::ops::RunArgs {
            sources: self.sources.clone(),
            skip,
            limit: self.limit,
            refresh: self.refresh,
            // Neither belongs to a cadence: a scheduled dry run would fire on time and do
            // nothing forever, and the config path is the file this block was read from.
            dry_run: false,
            config: None,
        })
    }

    /// The block `schedule set` writes.
    pub fn to_toml_block(&self) -> String {
        let mut s = String::from("[[schedule]]\n");
        s.push_str(&format!("id = {}\n", quote(&self.id)));
        s.push_str(&format!("cron = {}\n", quote(&self.cron)));
        if let Some(tz) = &self.tz {
            s.push_str(&format!("tz = {}\n", quote(tz)));
        }
        if !self.sources.is_empty() {
            s.push_str(&format!("sources = {}\n", quote_list(&self.sources)));
        }
        if !self.skip.is_empty() {
            s.push_str(&format!("skip = {}\n", quote_list(&self.skip)));
        }
        if let Some(limit) = self.limit {
            s.push_str(&format!("limit = {limit}\n"));
        }
        if self.refresh {
            s.push_str("refresh = true\n");
        }
        if let Some(jitter) = self.jitter_secs {
            s.push_str(&format!("jitter_secs = {jitter}\n"));
        }
        if let Some(enabled) = self.enabled {
            s.push_str(&format!("enabled = {enabled}\n"));
        }
        if let Some(catch_up) = self.catch_up {
            s.push_str(&format!("catch_up = {catch_up}\n"));
        }
        s
    }
}

/// The stage names a `skip` list may use, in pipeline order.
pub const STAGE_NAMES: [&str; 6] = [
    "discover",
    "collect",
    "extract",
    "transcribe",
    "index",
    "embed",
];

fn parse_stage(name: &str) -> Option<crate::ops::Stage> {
    use crate::ops::Stage;
    Some(match name {
        "discover" => Stage::Discover,
        "collect" => Stage::Collect,
        "extract" => Stage::Extract,
        "transcribe" => Stage::Transcribe,
        "index" => Stage::Index,
        "embed" => Stage::Embed,
        _ => return None,
    })
}

/// A TOML basic string. Escapes what the grammar requires and nothing else.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn quote_list(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| quote(s)).collect();
    format!("[{}]", inner.join(", "))
}

impl Config {
    /// Loads the first config file found, or defaults.
    ///
    /// Precedence, nearest first:
    /// 1. `$CENTINEL_CONFIG`
    /// 2. `./centinel.toml` — per-project
    /// 3. `~/.centinel/centinel.toml` — beside the default store
    /// 4. `~/.config/centinel/config.toml` — per-user
    pub fn load() -> anyhow::Result<Self> {
        match Self::locate() {
            Some(path) => Self::from_file(&path),
            None => Ok(Self::default()),
        }
    }

    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
    }

    pub fn parse(text: &str) -> anyhow::Result<Self> {
        retired_keys(text)?;
        let config: Self = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    /// Catches the mistakes that would otherwise surface hours into a run.
    ///
    /// Every check here is one a `centinel run` would hit *after* doing real work on the
    /// sources listed above the broken one — a bad id at position nine is discovered
    /// nine crawls in. Validating the whole file up front turns that into an error
    /// before the first request.
    pub fn validate(&self) -> anyhow::Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        for source in &self.sources {
            crate::domain::SourceId::new(source.id.clone())?;
            source.validate()?;
            if !seen.insert(source.id.as_str()) {
                anyhow::bail!(
                    "source `{}` is defined twice; ids are directory names and must be unique",
                    source.id
                );
            }
        }
        anyhow::ensure!(
            self.defaults.rps > 0.0,
            "defaults.rps must be greater than zero"
        );
        self.inference.validate()?;
        self.validate_schedules()?;
        Ok(())
    }

    /// Checks every `[[schedule]]` block against this config.
    ///
    /// Separate from the rest of [`Config::validate`] so `serve` can name schedules as the
    /// reason it refused to start, and so `schedules --check` can run exactly this.
    ///
    /// **Every failure here is fatal to `serve`.** A server that starts happily with a
    /// broken schedule collects nothing and says so nowhere; the operator finds out weeks
    /// later, from an empty search result. Refusing is loud at the one moment it is
    /// cheap — while somebody is watching it start.
    pub fn validate_schedules(&self) -> anyhow::Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        for schedule in &self.schedules {
            anyhow::ensure!(
                !schedule.id.trim().is_empty(),
                "a [[schedule]] block has no id; it is how the journal and every report \
                 name this cadence"
            );
            if !seen.insert(schedule.id.as_str()) {
                anyhow::bail!(
                    "schedule `{}` is defined twice; ids name a cadence and must be unique",
                    schedule.id
                );
            }

            let cron = schedule.cron()?;
            let zone = schedule.zone()?;
            schedule.run_args()?;

            // An expression that parses but never occurs — `0 0 30 2 *`, the 30th of
            // February. Caught here rather than discovered by a schedule that has quietly
            // never fired.
            anyhow::ensure!(
                cron.next_after(jiff::Timestamp::now(), &zone).is_some(),
                "schedule `{}`: `{}` parses but never occurs",
                schedule.id,
                schedule.cron
            );

            // Whether each named source exists is a question for the store, not this
            // file: see `catalog::Sources::check_schedules`, which `serve` and `schedule
            // add` ask before anything fires.
        }
        Ok(())
    }

    /// The `[[schedule]]` block with this id.
    pub fn schedule(&self, id: &str) -> Option<&ScheduleConfig> {
        self.schedules.iter().find(|s| s.id == id)
    }

    /// The config file in effect, or `None` when none exists and defaults are in use.
    pub fn locate() -> Option<PathBuf> {
        Self::search_paths().into_iter().find(|p| p.is_file())
    }

    /// The store root this config asks for, else [`default_root`].
    ///
    /// The last word on the subject unless `--root` or `$CENTINEL_ROOT` was given, both
    /// of which the caller applies ahead of this — someone typing a path is an
    /// instruction, and a config file is a standing preference.
    pub fn store_root(&self) -> PathBuf {
        self.root
            .as_deref()
            .map(expand_tilde)
            .unwrap_or_else(default_root)
    }

    /// Where a config file would be *written*: the one in effect, else
    /// [`default_config_path`].
    ///
    /// Beside the default store rather than in the working directory, because
    /// `centinel source add` typed from anywhere has to reach the config that feeds the
    /// store the same command collects into. A working-directory default wrote a file
    /// the next run — from one directory up — could not find, and reported "no sources
    /// configured" at somebody looking straight at the block they had just added.
    pub fn write_path() -> PathBuf {
        Self::locate().unwrap_or_else(|| {
            std::env::var("CENTINEL_CONFIG")
                .map(PathBuf::from)
                .unwrap_or_else(|_| default_config_path())
        })
    }

    /// Where [`Self::load`] looks, in order. Exposed so `doctor` can report it.
    pub fn search_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(explicit) = std::env::var("CENTINEL_CONFIG") {
            paths.push(PathBuf::from(explicit));
        }
        paths.push(PathBuf::from(DEFAULT_FILENAME));
        if let Some(home) = home() {
            // Beside the default store, so the corpus and the statement of what feeds it
            // sit in one directory.
            paths.push(home.join(DEFAULT_DIRNAME).join(DEFAULT_FILENAME));
            paths.push(home.join(".config").join("centinel").join("config.toml"));
        }
        paths
    }

    /// A starter file, written by `centinel open --write-config`.
    pub fn example() -> &'static str {
        EXAMPLE
    }
}

/// The file `centinel schedule add` creates when none exists, and what ships as
/// `centinel.toml.example`.
pub const EXAMPLE: &str = r#"# Centinel configuration — what is true of this machine.
#
# The sources a corpus is made of live in the store, not here: add them with
# `centinel source add` or from the web's Sources page. `centinel run` walks every enabled
# one: discover, collect, extract, index, then one corpus-wide embed. Every stage skips
# work it has already done, so running it twice is cheap.

# Where the store lives. Defaults to ~/.centinel, so every run collects into one corpus
# whatever directory it was started from. `--root` and $CENTINEL_ROOT override it.
#   root = "~/.centinel"

[defaults]
# Requests per second, per host. Deliberately slow.
rps = 1.0
lang = "en"

# The servers that embed, rerank and transcribe. Centinel runs no model itself: each is
# a request to the server named here, which holds the weights and its own tuning. Every
# capability uses `base_url` unless its own table names another. A server on this machine
# keeps everything here; one elsewhere receives chunk text, queries and audio.
[inference]
base_url = "http://127.0.0.1:8080/v1"
# For a server that wants a key: the name of the variable holding it, never the key.
#   api_key_env = "OPENROUTER_API_KEY"

[inference.embed]
model = "qwen3-embedding-4b"
# Qwen3-Embedding wants queries wrapped like this and documents bare. A symmetric model
# (OpenAI's) wants "". Changing the model is a new vector space.
query_prefix = "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:"
# Chunks per request, and requests in flight.
batch = 64
concurrency = 4

[inference.rerank]
model = "qwen3-reranker-0.6b"

[inference.transcribe]
model = "whisper-large-v3-turbo"

# The Jev model `centinel classify` scores documents with when no --model is given.
# Classifying is never part of `centinel run`: document text leaves this machine for
# api.typesafe.ai only when a person starts it, and $TYPESAFE_API_KEY must be set.
#   [classify]
#   model = "jev-1.13.0"

# A cadence `centinel serve` fires `centinel run` on. Here rather than in the store, so
# a store handed to someone else collects nothing on its own.
#   [[schedule]]
#   id = "nightly"
#   cron = "0 3 * * *"
#   sources = ["tampa"]

[open]
# Either an application name, or a command template containing {path}.
#   pdf      = "Adobe Acrobat"
#   markdown = "Obsidian"
#   html     = "Safari"
#   text     = "nvim {path}"
#
# "system" hands the file to the OS default handler.
default = "system"
"#;

/// Appends a `[[schedule]]` block, validated against the file it is joining.
///
/// Validated against the *whole* config rather than in isolation, because an id already
/// taken parses cleanly on its own and is silent until 3am. Whether its sources exist is
/// checked by the caller, against the store.
pub fn append_schedule(path: &Path, schedule: &ScheduleConfig) -> anyhow::Result<()> {
    save_schedule(path, schedule, false)
}

/// Validates and atomically writes an addition or replacement, preserving other blocks.
pub fn save_schedule(path: &Path, schedule: &ScheduleConfig, replace: bool) -> anyhow::Result<()> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => EXAMPLE.to_string(),
        Err(e) => anyhow::bail!("reading {}: {e}", path.display()),
    };

    let before = Config::parse(&existing).map_err(|e| {
        anyhow::anyhow!(
            "{} does not parse, so it cannot be edited safely: {e}",
            path.display()
        )
    })?;
    if !replace && before.schedule(&schedule.id).is_some() {
        anyhow::bail!(
            "schedule `{}` is already in {}; edit it there or remove it first",
            schedule.id,
            path.display()
        );
    }

    // Checked before writing, against the config as it will be.
    let mut proposed = before.clone();
    proposed.schedules.retain(|s| s.id != schedule.id);
    proposed.schedules.push(schedule.clone());
    proposed.validate_schedules()?;

    let mut text = existing;
    if before.schedule(&schedule.id).is_some() {
        text = without_schedule(path, &text, &schedule.id)?;
    }
    if !text.is_empty() {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
    }
    text.push_str(&schedule.to_toml_block());

    let mut expected: Vec<String> = before
        .schedules
        .iter()
        .filter(|s| s.id != schedule.id)
        .map(|s| s.id.clone())
        .collect();
    expected.push(schedule.id.clone());
    verify_schedule_edit(path, &text, &expected)?;
    crate::store::write_atomically(path, &text).map_err(Into::into)
}

/// Removes the `[[schedule]]` block with this id.
pub fn remove_schedule(path: &Path, id: &str) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    let before = Config::parse(&text).map_err(|e| {
        anyhow::anyhow!(
            "{} does not parse, so it cannot be edited safely: {e}",
            path.display()
        )
    })?;
    anyhow::ensure!(
        before.schedule(id).is_some(),
        "no schedule `{id}` in {}",
        path.display()
    );

    let out = without_schedule(path, &text, id)?;
    let expected: Vec<String> = before
        .schedules
        .iter()
        .filter(|s| s.id != id)
        .map(|s| s.id.clone())
        .collect();
    verify_schedule_edit(path, &out, &expected)?;
    crate::store::write_atomically(path, &out).map_err(Into::into)
}

fn without_schedule(path: &Path, text: &str, id: &str) -> anyhow::Result<String> {
    let lines: Vec<&str> = text.lines().collect();
    let (start, end) = block_ranges(&lines, "[[schedule]]")
        .into_iter()
        .find(|(s, e)| block_declares(&lines[*s..*e], id))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "schedule `{id}` parses out of {} but its `[[schedule]]` block could not \
                 be located; edit the file by hand",
                path.display()
            )
        })?;

    let kept: Vec<&str> = lines[..start]
        .iter()
        .chain(lines[end..].iter())
        .copied()
        .collect();
    let mut out = kept.join("\n");
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }

    Ok(out)
}

/// Re-parses an edited file and checks it holds exactly the schedules expected.
fn verify_schedule_edit(path: &Path, text: &str, expected: &[String]) -> anyhow::Result<()> {
    let after = Config::parse(text).map_err(|e| {
        anyhow::anyhow!(
            "editing {} would have produced a file that does not parse ({e}); it was \
             left unchanged",
            path.display()
        )
    })?;
    let got: Vec<String> = after.schedules.iter().map(|s| s.id.clone()).collect();
    anyhow::ensure!(
        got == expected,
        "editing {} would have left schedules {got:?} rather than {expected:?}; it was \
         left unchanged",
        path.display()
    );
    Ok(())
}

/// Line ranges `[start, end)` of each top-level block under this header.
fn block_ranges(lines: &[&str], header: &str) -> Vec<(usize, usize)> {
    let is_header = |l: &str| {
        let t = l.trim_start();
        t.starts_with('[') && !t.starts_with('#')
    };
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == header {
            let mut end = i + 1;
            while end < lines.len() && !is_header(lines[end]) {
                end += 1;
            }
            ranges.push((i, end));
            i = end;
        } else {
            i += 1;
        }
    }
    ranges
}

/// Whether a `[[schedule]]` block's lines declare this id, parsed as TOML rather than
/// matched as text.
fn block_declares(block: &[&str], id: &str) -> bool {
    let text = block.join("\n");
    toml::from_str::<Config>(&text)
        .is_ok_and(|parsed| parsed.schedules.first().is_some_and(|s| s.id == id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Config {
        Config::parse(s).unwrap()
    }

    #[test]
    fn maps_kinds_to_applications() {
        let c = parse(
            r#"
            [open]
            pdf = "Adobe Acrobat"
            markdown = "Obsidian"
        "#,
        );
        assert_eq!(c.open.opener_for("pdf"), "Adobe Acrobat");
        assert_eq!(c.open.opener_for("markdown"), "Obsidian");
    }

    #[test]
    fn falls_back_to_default_then_to_the_system_handler() {
        let c = parse(
            r#"
            [open]
            pdf = "Adobe Acrobat"
            default = "Zed"
        "#,
        );
        assert_eq!(c.open.opener_for("spreadsheet"), "Zed");

        let bare = parse("[open]\npdf = \"Preview\"");
        assert_eq!(bare.open.opener_for("html"), SYSTEM_DEFAULT);
    }

    #[test]
    fn an_empty_or_absent_config_is_valid() {
        assert_eq!(parse("").open.opener_for("pdf"), SYSTEM_DEFAULT);
        assert_eq!(Config::default().open.opener_for("pdf"), SYSTEM_DEFAULT);
        assert!(parse("").sources.is_empty());
    }

    #[test]
    fn command_templates_are_stored_verbatim() {
        let c = parse("[open]\ntext = \"nvim {path}\"");
        assert_eq!(c.open.opener_for("text"), "nvim {path}");
    }

    #[test]
    fn the_example_config_parses() {
        let c = parse(Config::example());
        assert_eq!(c.open.opener_for("pdf"), SYSTEM_DEFAULT);
        // Every source in it is commented out — it is a template, not a corpus.
        assert!(c.sources.is_empty());
        assert_eq!(c.defaults.rps, 1.0);
        // And so is `root`: a starter file must not pin the store somewhere the default
        // would not have put it.
        assert!(c.root.is_none());
        // The starter file spells out exactly what an absent `[inference]` means, so
        // writing it changes nothing — the query prefix included, newline and all.
        let absent = InferenceConfig::default();
        assert_eq!(c.inference.base_url, absent.base_url);
        assert_eq!(c.inference.embed.model, absent.embed.model);
        assert_eq!(c.inference.embed.query_prefix, absent.embed.query_prefix);
        assert_eq!(c.inference.rerank.model, absent.rerank.model);
        assert_eq!(c.inference.transcribe.model, absent.transcribe.model);
    }

    #[test]
    fn search_paths_are_ordered_nearest_first() {
        let paths = Config::search_paths();
        let idx = |needle: &str| {
            paths
                .iter()
                .position(|p| p.to_string_lossy().contains(needle))
        };
        // The per-project file, then the one beside the default store, then `~/.config`.
        assert_eq!(idx("centinel.toml"), Some(0));
        assert!(
            idx(".centinel").unwrap() < idx(".config").unwrap(),
            "{paths:?}"
        );
    }

    // ── the store root ────────────────────────────────────────────────────────

    /// The whole point of the change: a store belongs to the person, not to whichever
    /// directory the binary was started from.
    #[test]
    fn the_default_root_is_under_home() {
        let home = home().expect("tests run with a home");
        assert_eq!(default_root(), home.join(".centinel"));
        assert_eq!(default_config_path(), home.join(".centinel/centinel.toml"));
        assert_eq!(Config::default().store_root(), home.join(".centinel"));
    }

    #[test]
    fn the_config_names_the_root_and_tildes_are_expanded() {
        let home = home().expect("tests run with a home");
        assert_eq!(
            parse("root = \"~/corpora/tampa\"\n").store_root(),
            home.join("corpora/tampa")
        );
        assert_eq!(
            parse("root = \"/mnt/big/centinel\"\n").store_root(),
            PathBuf::from("/mnt/big/centinel")
        );
        // A relative root stays relative — someone who wrote one meant it.
        assert_eq!(
            parse("root = \".centinel\"\n").store_root(),
            PathBuf::from(".centinel")
        );
    }

    /// Only a leading `~` that is the whole segment. `~snapshots` is a directory name.
    #[test]
    fn tilde_expansion_stops_at_the_first_segment() {
        let home = home().expect("tests run with a home");
        assert_eq!(expand_tilde("~"), home);
        assert_eq!(expand_tilde("~/"), home);
        assert_eq!(expand_tilde("~snapshots"), PathBuf::from("~snapshots"));
        assert_eq!(expand_tilde("a/~/b"), PathBuf::from("a/~/b"));
    }

    /// `root` sits beside the tables it does not belong to, so it has to survive both a
    /// file that has them and one that does not.
    #[test]
    fn root_coexists_with_the_rest_of_the_file() {
        let c = parse(
            r#"
            root = "/srv/corpus"

            [defaults]
            rps = 0.5

            [[source]]
            id = "tampa"
            site = "https://www.tampa.gov"
        "#,
        );
        assert_eq!(c.store_root(), PathBuf::from("/srv/corpus"));
        assert_eq!(c.defaults.rps, 0.5);
        assert_eq!(c.sources.len(), 1);
    }

    /// The block that lets `run` send text to Jev. Absent is the default and means the
    /// stage is skipped; present, it names the model and nothing else is required.
    #[test]
    fn a_classify_block_names_the_model_and_is_otherwise_absent() {
        let c = parse("[classify]\nmodel = \"jev-1.13.0\"\n");
        let block = c.classify.as_ref().expect("classify block");
        assert_eq!(block.model, "jev-1.13.0");
        assert_eq!(block.concurrency, None);

        let c = parse("[classify]\nmodel = \"jev-1.13.0\"\nconcurrency = 16\n");
        assert_eq!(c.classify.unwrap().concurrency, Some(16));

        assert!(parse("[defaults]\nrps = 1.0\n").classify.is_none());
        assert!(
            Config::parse("[classify]\nmodle = \"jev\"\n").is_err(),
            "a misspelt key is refused, not ignored"
        );
    }

    /// A config file predating `[[source]]` must keep working untouched.
    #[test]
    fn an_open_only_config_still_loads() {
        let c = parse("[open]\npdf = \"Preview\"\n");
        assert!(c.sources.is_empty());
        assert_eq!(c.inference.embed.model, "qwen3-embedding-4b");
    }

    #[test]
    fn sources_carry_their_acquisition() {
        let c = parse(
            r#"
            [[source]]
            id = "tampa"
            site = "https://www.tampa.gov"

            [[source]]
            id = "tampa-council"
            channel = "https://www.youtube.com/@CityofTampa"
        "#,
        );
        assert_eq!(c.sources.len(), 2);
        assert_eq!(
            c.sources[0].acquisition().unwrap(),
            Acquisition::Site("https://www.tampa.gov")
        );
        assert!(matches!(
            c.sources[1].acquisition().unwrap(),
            Acquisition::Channel(_)
        ));
    }

    /// The typo this whole `deny_unknown_fields` decision exists for.
    #[test]
    fn a_misspelled_table_is_an_error_not_silence() {
        let err = Config::parse("[[sources]]\nid = \"tampa\"\nsite = \"https://x.gov\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("sources"), "{err}");
    }

    // ── schedules ─────────────────────────────────────────────────────────────

    fn with_schedule(block: &str) -> Result<Config, anyhow::Error> {
        let text = format!("[[source]]\nid = \"tampa\"\nsite = \"https://tampa.gov\"\n\n{block}");
        let config = Config::parse(&text)?;
        config.validate()?;
        Ok(config)
    }

    #[test]
    fn a_schedule_block_parses_into_a_run_invocation() {
        let config = with_schedule(
            "[[schedule]]\nid = \"tampa-daily\"\ncron = \"0 3 * * *\"\n\
             tz = \"America/New_York\"\nsources = [\"tampa\"]\nskip = [\"embed\"]\n",
        )
        .unwrap();

        let s = &config.schedules[0];
        assert_eq!(s.id, "tampa-daily");
        assert!(s.is_enabled(), "enabled defaults on");
        assert!(s.catches_up(), "catch_up defaults on");
        assert_eq!(s.jitter_secs(), DEFAULT_JITTER_SECS);

        let args = s.run_args().unwrap();
        assert_eq!(args.sources, ["tampa"]);
        assert_eq!(args.skip, [crate::ops::Stage::Embed]);
        assert!(
            !args.dry_run,
            "a scheduled dry run would fire and do nothing forever"
        );
    }

    /// The reason this block spells the run options out instead of flattening `RunArgs`:
    /// a flattened struct cannot deny unknown fields, and a schedule that silently drops
    /// `soruces` fires on time, collects nothing, and reports success.
    #[test]
    fn a_misspelled_key_in_a_schedule_is_an_error_not_silence() {
        let err =
            with_schedule("[[schedule]]\nid = \"x\"\ncron = \"@daily\"\nsoruces = [\"tampa\"]\n")
                .unwrap_err()
                .to_string();
        assert!(err.contains("soruces"), "{err}");
    }

    /// The guard that turns "a new `run` flag was not made schedulable" from an omission
    /// into a failing test. Every field of `RunArgs` is either carried by a schedule or
    /// listed here with a reason it cannot be.
    #[test]
    fn run_options_stay_in_step_with_run() {
        let run = serde_json::to_value(crate::ops::RunArgs::default()).unwrap();
        let schedule = serde_json::to_value(ScheduleConfig {
            // Every optional field set, so serialization skips none of them.
            limit: Some(1),
            refresh: true,
            sources: vec!["x".into()],
            skip: vec!["embed".into()],
            ..Default::default()
        })
        .unwrap();

        // `dry_run` fires on time and does nothing; `config` is the file the block was
        // read out of. Neither is a cadence's business.
        const NOT_SCHEDULABLE: [&str; 2] = ["dry_run", "config"];

        for key in run.as_object().unwrap().keys() {
            if NOT_SCHEDULABLE.contains(&key.as_str()) {
                continue;
            }
            assert!(
                schedule.get(key).is_some(),
                "`run` has `{key}` and a [[schedule]] block cannot express it — add the \
                 field to ScheduleConfig, or add it to NOT_SCHEDULABLE with a reason"
            );
        }
    }

    /// Syntactically valid, and it never happens. Caught at load rather than by a
    /// schedule that has quietly never fired.
    #[test]
    fn a_cron_that_never_occurs_is_refused() {
        let err = with_schedule("[[schedule]]\nid = \"x\"\ncron = \"0 0 30 2 *\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("never occurs"), "{err}");
    }

    #[test]
    fn a_bad_cron_zone_or_stage_names_itself() {
        let err = with_schedule("[[schedule]]\nid = \"x\"\ncron = \"nope\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("cron"), "{err}");

        let err =
            with_schedule("[[schedule]]\nid = \"x\"\ncron = \"@daily\"\ntz = \"Mars/Olympus\"\n")
                .unwrap_err()
                .to_string();
        assert!(err.contains("IANA"), "{err}");

        let err =
            with_schedule("[[schedule]]\nid = \"x\"\ncron = \"@daily\"\nskip = [\"embedd\"]\n")
                .unwrap_err()
                .to_string();
        assert!(err.contains("embedd"), "{err}");
        assert!(
            err.contains("transcribe"),
            "the error should list the real stages: {err}"
        );
    }

    #[test]
    fn two_schedules_cannot_share_an_id() {
        let err = with_schedule(
            "[[schedule]]\nid = \"x\"\ncron = \"@daily\"\n\n\
             [[schedule]]\nid = \"x\"\ncron = \"@weekly\"\n",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("twice"), "{err}");
    }

    /// The round trip `schedule set` depends on: what it writes must read back as what
    /// it was asked to write.
    #[test]
    fn a_written_block_parses_back_to_itself() {
        let written = ScheduleConfig {
            id: "tampa-daily".into(),
            cron: "0 3 * * *".into(),
            tz: Some("America/New_York".into()),
            sources: vec!["tampa".into()],
            skip: vec!["embed".into()],
            limit: Some(500),
            refresh: true,
            jitter_secs: Some(0),
            enabled: Some(false),
            catch_up: Some(false),
        };
        // With the source it names, because `parse` validates and a schedule naming
        // nothing is exactly what validation refuses.
        let text = format!(
            "[[source]]\nid = \"tampa\"\nsite = \"https://tampa.gov\"\n\n{}",
            written.to_toml_block()
        );
        let back = Config::parse(&text).unwrap();
        let got = &back.schedules[0];
        assert_eq!(got.id, written.id);
        assert_eq!(got.cron, written.cron);
        assert_eq!(got.tz, written.tz);
        assert_eq!(got.sources, written.sources);
        assert_eq!(got.skip, written.skip);
        assert_eq!(got.limit, written.limit);
        assert!(got.refresh);
        assert_eq!(got.jitter_secs(), 0);
        assert!(!got.is_enabled());
        assert!(!got.catches_up());
    }

    #[test]
    fn appending_and_removing_a_schedule_leaves_the_rest_of_the_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("centinel.toml");
        std::fs::write(
            &path,
            "# a comment somebody wrote\n[[source]]\nid = \"tampa\"\nsite = \"https://tampa.gov\"\n",
        )
        .unwrap();

        append_schedule(
            &path,
            &ScheduleConfig {
                id: "daily".into(),
                cron: "0 3 * * *".into(),
                sources: vec!["tampa".into()],
                ..Default::default()
            },
        )
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# a comment somebody wrote"), "{text}");
        assert_eq!(Config::parse(&text).unwrap().schedules.len(), 1);

        remove_schedule(&path, "daily").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# a comment somebody wrote"), "{text}");
        assert!(Config::parse(&text).unwrap().schedules.is_empty());
        assert_eq!(
            Config::parse(&text).unwrap().sources.len(),
            1,
            "removing a schedule must not touch the sources"
        );
    }

    #[test]
    fn a_source_naming_neither_or_both_is_rejected() {
        let err = Config::parse("[[source]]\nid = \"x\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("neither"), "{err}");

        let err = Config::parse(
            "[[source]]\nid = \"x\"\nsite = \"https://x.gov\"\nchannel = \"https://y\"\n",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("both"), "{err}");
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let err = Config::parse(
            r#"
            [[source]]
            id = "tampa"
            site = "https://a.gov"

            [[source]]
            id = "tampa"
            site = "https://b.gov"
        "#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("twice"), "{err}");
    }

    /// An id becomes a directory name, so the domain rule applies at parse time.
    #[test]
    fn an_id_that_is_not_a_source_id_is_rejected() {
        let err = Config::parse("[[source]]\nid = \"../etc\"\nsite = \"https://x.gov\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid source id"), "{err}");
    }

    #[test]
    fn defaults_apply_when_the_table_is_absent_or_partial() {
        let c = parse("[defaults]\nrps = 0.5\n");
        assert_eq!(c.defaults.rps, 0.5);
        assert_eq!(c.defaults.lang, "en");
    }

    /// A written `[inference.embed]` must say what it prefixes queries with. Carrying the
    /// Qwen3 default onto another model would degrade retrieval with no error anywhere.
    #[test]
    fn an_embed_table_must_state_its_query_prefix() {
        let err = Config::parse("[inference.embed]\nmodel = \"text-embedding-3-small\"\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("query_prefix"), "{err}");
        let c =
            parse("[inference.embed]\nmodel = \"text-embedding-3-small\"\nquery_prefix = \"\"\n");
        assert_eq!(c.inference.embed.query_prefix, "");
        assert_eq!(c.inference.embed.batch, 64, "the rest still default");
    }

    /// Refused at the file, not an hour into a run at the first request.
    #[test]
    fn inference_settings_are_checked_when_the_file_is_read() {
        assert!(Config::parse("[inference]\nbase_url = \"localhost:8080\"\n").is_err());
        assert!(Config::parse("[inference]\nbase_url = \"ftp://gpu/v1\"\n").is_err());
        let embed = |extra: &str| {
            Config::parse(&format!(
                "[inference.embed]\nmodel = \"m\"\nquery_prefix = \"\"\n{extra}"
            ))
        };
        assert!(embed("batch = 0\n").is_err());
        assert!(embed("concurrency = 0\n").is_err());
        assert!(embed("concurrency = 33\n").is_err());
        assert!(embed("batch = 16\nconcurrency = 8\n").is_ok());
    }

    /// A config that worked yesterday is told what to write today, in the words to paste —
    /// not "unknown field `embed_model`".
    #[test]
    fn retired_defaults_name_their_replacement() {
        let err = Config::parse(
            "[defaults]\nembed_model = \"openrouter/qwen/qwen3-embedding-8b\"\n\
             embed_batch = \"auto\"\ntranscribe_model = \"whisper-tiny\"\n",
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("base_url = \"https://openrouter.ai/api/v1\""),
            "{err}"
        );
        assert!(err.contains("model = \"qwen/qwen3-embedding-8b\""), "{err}");
        assert!(
            err.contains("api_key_env = \"OPENROUTER_API_KEY\""),
            "{err}"
        );
        assert!(err.contains("embed_batch"), "{err}");
        assert!(
            err.contains("[inference.transcribe]\nmodel = \"whisper-tiny\""),
            "{err}"
        );

        // The suggestion parses, which is the whole point of printing it.
        let suggested = err.split("\n\n").nth(1).unwrap();
        let suggested = suggested.split_once('\n').unwrap().1;
        assert!(Config::parse(suggested).is_ok(), "{suggested}");
    }

    #[test]
    fn a_zero_crawl_rate_is_rejected() {
        assert!(Config::parse("[defaults]\nrps = 0.0\n").is_err());
        assert!(
            Config::parse("[[source]]\nid = \"a\"\nsite = \"https://a.gov\"\nrps = -1.0\n")
                .is_err()
        );
    }
}
