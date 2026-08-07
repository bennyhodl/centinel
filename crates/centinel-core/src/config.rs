//! Configuration.
//!
//! SPEC §1 calls Centinel "generic, config-driven — Tampa is the first config file, not
//! an assumption". This module is where that stops being aspirational: `[[source]]`
//! blocks name what the corpus is made of, and [`crate::ops::run`] walks them.
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
//! Sources are the interesting case, because they arguably *are* corpus. They live here
//! anyway: a source is a statement of **intent to collect**, and intent is not something
//! a recipient of your blobs should silently inherit. Handing someone the store gives
//! them what you gathered; handing them the config asks their machine to go crawling.
//!
//! ## Why writes are textual rather than a serialize round-trip
//!
//! `centinel source add` edits a file a person wrote and will read again. Serializing a
//! [`Config`] back over it would parse-and-reprint — silently deleting every comment and
//! reordering every key. So [`append_source`] appends and [`remove_source`] excises
//! lines, and both re-parse the result to prove the edit did what it claimed before it
//! reaches disk (see [`verify_edit`]).

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

    /// The sources a bare `centinel run` walks, in file order.
    ///
    /// `rename` because TOML spells a list of tables `[[source]]` — singular reads
    /// correctly at each block, where the plural would not.
    #[serde(default, rename = "source")]
    pub sources: Vec<SourceConfig>,

    /// The cadences `centinel serve` fires runs on, in file order.
    ///
    /// Beside the sources they name, and in the same file, because **this is where the
    /// authority to collect comes from**. A scheduled run is not the server deciding to
    /// crawl; it is this file, which only the operator can write, executed later. Nothing
    /// arriving over HTTP or MCP can add a block here, which is the whole of the access
    /// story in `docs/SCHEDULING.md` §1.1.
    #[serde(default, rename = "schedule")]
    pub schedules: Vec<ScheduleConfig>,
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

    /// The embedding model `run` uses for its single corpus-wide pass.
    #[serde(default = "default_embed_model")]
    pub embed_model: String,

    /// The Whisper model `run` uses for sources that produce audio.
    #[serde(default = "default_transcribe_model")]
    pub transcribe_model: String,

    /// Caption and transcription language.
    #[serde(default = "default_lang")]
    pub lang: String,
}

fn default_rps() -> f64 {
    1.0
}
fn default_embed_model() -> String {
    "qwen3-embedding-4b".to_string()
}
fn default_transcribe_model() -> String {
    "whisper-large-v3-turbo".to_string()
}
fn default_lang() -> String {
    "en".to_string()
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            rps: default_rps(),
            embed_model: default_embed_model(),
            transcribe_model: default_transcribe_model(),
            lang: default_lang(),
        }
    }
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

    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    /// The TOML block this source serializes to, ready to append.
    ///
    /// Hand-written rather than `toml::to_string`, so `id` leads and the acquisition key
    /// follows it — the two lines a person actually reads — instead of arriving in
    /// whatever order the serializer chose.
    pub fn to_toml_block(&self) -> String {
        let mut s = String::from("[[source]]\n");
        s.push_str(&format!("id = {}\n", quote(&self.id)));
        if let Some(site) = &self.site {
            s.push_str(&format!("site = {}\n", quote(site)));
        }
        if let Some(channel) = &self.channel {
            s.push_str(&format!("channel = {}\n", quote(channel)));
        }
        if let Some(enabled) = self.enabled {
            s.push_str(&format!("enabled = {enabled}\n"));
        }
        if let Some(rps) = self.rps {
            s.push_str(&format!("rps = {rps}\n"));
        }
        if let Some(lang) = &self.lang {
            s.push_str(&format!("lang = {}\n", quote(lang)));
        }
        if let Some(audio) = self.audio_if_no_captions {
            s.push_str(&format!("audio_if_no_captions = {audio}\n"));
        }
        if !self.matches.is_empty() {
            s.push_str(&format!("matches = {}\n", quote_list(&self.matches)));
        }
        if !self.yt_dlp_args.is_empty() {
            s.push_str(&format!(
                "yt_dlp_args = {}\n",
                quote_list(&self.yt_dlp_args)
            ));
        }
        s
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
            source.acquisition()?;
            if !seen.insert(source.id.as_str()) {
                anyhow::bail!(
                    "source `{}` is defined twice; ids are directory names and must be unique",
                    source.id
                );
            }
            if let Some(rps) = source.rps {
                anyhow::ensure!(
                    rps > 0.0,
                    "source `{}`: rps must be greater than zero",
                    source.id
                );
            }
        }
        anyhow::ensure!(
            self.defaults.rps > 0.0,
            "defaults.rps must be greater than zero"
        );
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

            // A source id that names nothing is the mistake this file invites most: the
            // schedule fires on time, collects nothing, and reports success.
            for id in &schedule.sources {
                anyhow::ensure!(
                    self.source(id).is_some(),
                    "schedule `{}` names source `{id}`, which has no [[source]] block",
                    schedule.id
                );
            }
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

    pub fn source(&self, id: &str) -> Option<&SourceConfig> {
        self.sources.iter().find(|s| s.id == id)
    }

    /// The sources a run walks: every enabled one, or exactly those named.
    ///
    /// Naming a disabled source runs it. `enabled = false` is a default, and an explicit
    /// argument is an instruction — the person typing the id can see the block.
    pub fn selected(&self, only: &[String]) -> anyhow::Result<Vec<&SourceConfig>> {
        if only.is_empty() {
            return Ok(self.sources.iter().filter(|s| s.is_enabled()).collect());
        }
        let mut out = Vec::with_capacity(only.len());
        for id in only {
            let source = self.source(id).ok_or_else(|| {
                anyhow::anyhow!(
                    "no source `{id}` in {}{}",
                    Self::locate()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "the config".into()),
                    self.nearby(id)
                )
            })?;
            out.push(source);
        }
        Ok(out)
    }

    /// " (did you mean `tampa`?)", or empty. Config ids are typed by hand, and a
    /// transposed character should not read as "that source does not exist".
    fn nearby(&self, id: &str) -> String {
        let close = self.sources.iter().find(|s| {
            s.id.eq_ignore_ascii_case(id) || s.id.starts_with(id) || id.starts_with(&s.id)
        });
        match close {
            Some(s) => format!(" (did you mean `{}`?)", s.id),
            None if self.sources.is_empty() => " — it has no sources yet".to_string(),
            None => String::new(),
        }
    }

    /// A starter file, written by `centinel open --write-config`.
    pub fn example() -> &'static str {
        EXAMPLE
    }
}

/// The file `centinel source add` creates when none exists, and what ships as
/// `centinel.toml.example`.
pub const EXAMPLE: &str = r#"# Centinel configuration.
#
# `centinel run` walks every [[source]] below: discover, collect, extract, index, then
# one corpus-wide embed. Every stage skips work it has already done, so running it twice
# is cheap and running it from cron is the intended use.

# Where the store lives. Defaults to ~/.centinel, so every run collects into one corpus
# whatever directory it was started from. `--root` and $CENTINEL_ROOT override it.
#   root = "~/.centinel"

[defaults]
# Requests per second, per host. Deliberately slow.
rps = 1.0
embed_model = "qwen3-embedding-4b"
transcribe_model = "whisper-large-v3-turbo"
lang = "en"

# A website. `site` is any URL on it; only the origin is used.
#   [[source]]
#   id = "tampa"
#   site = "https://www.tampa.gov"

# A YouTube channel — a peer of a website, differing only in how it is acquired.
#   [[source]]
#   id = "tampa-council"
#   channel = "https://www.youtube.com/@CityofTampa"
#   audio_if_no_captions = true

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

/// Appends a `[[source]]` block, creating the file from [`EXAMPLE`] if absent.
///
/// Appending — rather than reserializing — is what keeps a hand-written config
/// hand-written. Everything above the new block is byte-identical afterwards.
pub fn append_source(path: &Path, source: &SourceConfig) -> anyhow::Result<()> {
    source.acquisition()?;
    crate::domain::SourceId::new(source.id.clone())?;

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
    if before.source(&source.id).is_some() {
        anyhow::bail!(
            "source `{}` is already in {}; edit it there or remove it first",
            source.id,
            path.display()
        );
    }

    let mut text = existing;
    if !text.is_empty() {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
    }
    text.push_str(&source.to_toml_block());

    let mut expected: Vec<String> = before.sources.iter().map(|s| s.id.clone()).collect();
    expected.push(source.id.clone());
    verify_edit(path, &text, &expected)?;
    write_atomically(path, &text)
}

/// Removes the `[[source]]` block with this id.
///
/// Excises the header line through the line before the next top-level table, which takes
/// the block's own keys and its trailing blank lines and nothing else. Comments *above*
/// the header are left alone: they may describe the block or may be a section banner,
/// and deleting a person's prose on a guess is the worse error.
pub fn remove_source(path: &Path, id: &str) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    let before = Config::parse(&text).map_err(|e| {
        anyhow::anyhow!(
            "{} does not parse, so it cannot be edited safely: {e}",
            path.display()
        )
    })?;
    anyhow::ensure!(
        before.source(id).is_some(),
        "no source `{id}` in {}",
        path.display()
    );

    let lines: Vec<&str> = text.lines().collect();
    let ranges = block_ranges(&lines, "[[source]]");
    let (start, end) = ranges
        .into_iter()
        .find(|(s, e)| block_declares(&lines[*s..*e], id))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "source `{id}` parses out of {} but its `[[source]]` block could not be \
                 located; edit the file by hand",
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

    let expected: Vec<String> = before
        .sources
        .iter()
        .filter(|s| s.id != id)
        .map(|s| s.id.clone())
        .collect();
    verify_edit(path, &out, &expected)?;
    write_atomically(path, &out)
}

/// Proves a textual edit produced the source list it claimed, before it reaches disk.
///
/// This is what makes line-splicing an acceptable way to edit TOML. The splice is a
/// guess about the grammar; this re-parses the result and checks it against the answer
/// computed from the *parsed* config, so a mis-split fails loudly with the file
/// untouched instead of quietly dropping the block below it.
/// Appends a `[[schedule]]` block, validated against the file it is joining.
///
/// Validated against the *whole* config rather than in isolation, because the mistakes
/// this block invites are relational: a `sources` entry naming no `[[source]]`, and an id
/// already taken. Both parse cleanly on their own and are silent until 3am.
pub fn append_schedule(path: &Path, schedule: &ScheduleConfig) -> anyhow::Result<()> {
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
    if before.schedule(&schedule.id).is_some() {
        anyhow::bail!(
            "schedule `{}` is already in {}; edit it there or remove it first",
            schedule.id,
            path.display()
        );
    }

    // Checked before writing, against the config as it will be.
    let mut proposed = before.clone();
    proposed.schedules.push(schedule.clone());
    proposed.validate_schedules()?;

    let mut text = existing;
    if !text.is_empty() {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
    }
    text.push_str(&schedule.to_toml_block());

    let mut expected: Vec<String> = before.schedules.iter().map(|s| s.id.clone()).collect();
    expected.push(schedule.id.clone());
    verify_schedule_edit(path, &text, &expected)?;
    write_atomically(path, &text)
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

    let expected: Vec<String> = before
        .schedules
        .iter()
        .filter(|s| s.id != id)
        .map(|s| s.id.clone())
        .collect();
    verify_schedule_edit(path, &out, &expected)?;
    write_atomically(path, &out)
}

/// [`verify_edit`], for the schedule list.
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

fn verify_edit(path: &Path, text: &str, expected: &[String]) -> anyhow::Result<()> {
    let after = Config::parse(text).map_err(|e| {
        anyhow::anyhow!(
            "editing {} would have produced a file that does not parse ({e}); it was \
             left unchanged",
            path.display()
        )
    })?;
    let got: Vec<String> = after.sources.iter().map(|s| s.id.clone()).collect();
    anyhow::ensure!(
        got == expected,
        "editing {} would have left sources {got:?} rather than {expected:?}; it was \
         left unchanged",
        path.display()
    );
    Ok(())
}

/// Writes via a sibling temp file and a rename, so an interrupted write cannot leave a
/// half-written config where a whole one was.
fn write_atomically(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("creating {}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).map_err(|e| anyhow::anyhow!("writing {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| anyhow::anyhow!("replacing {}: {e}", path.display()))?;
    Ok(())
}

/// Line ranges `[start, end)` of each top-level `[[source]]` block.
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

/// Whether a block's lines declare this id, parsed as TOML rather than matched as text.
///
/// Asks both lists, because one function serves `[[source]]` and `[[schedule]]` and only
/// one of them can be present in a single block's worth of lines.
fn block_declares(block: &[&str], id: &str) -> bool {
    let text = block.join("\n");
    let Ok(parsed) = toml::from_str::<Config>(&text) else {
        return false;
    };
    parsed.sources.first().is_some_and(|s| s.id == id)
        || parsed.schedules.first().is_some_and(|s| s.id == id)
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

    /// A config file predating `[[source]]` must keep working untouched.
    #[test]
    fn an_open_only_config_still_loads() {
        let c = parse("[open]\npdf = \"Preview\"\n");
        assert!(c.sources.is_empty());
        assert_eq!(c.defaults.embed_model, "qwen3-embedding-4b");
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

    #[test]
    fn a_schedule_naming_a_source_that_does_not_exist_is_refused() {
        let err =
            with_schedule("[[schedule]]\nid = \"x\"\ncron = \"@daily\"\nsources = [\"orlando\"]\n")
                .unwrap_err()
                .to_string();
        assert!(err.contains("orlando"), "{err}");
        assert!(err.contains("[[source]]"), "{err}");
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

    /// The relational check has to happen against the file being joined, not in isolation.
    #[test]
    fn appending_a_schedule_for_an_unknown_source_is_refused_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("centinel.toml");
        std::fs::write(
            &path,
            "[[source]]\nid = \"tampa\"\nsite = \"https://tampa.gov\"\n",
        )
        .unwrap();

        let err = append_schedule(
            &path,
            &ScheduleConfig {
                id: "daily".into(),
                cron: "0 3 * * *".into(),
                sources: vec!["orlando".into()],
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("orlando"), "{err}");

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("schedule"),
            "the file was written anyway: {text}"
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
    fn selection_defaults_to_every_enabled_source() {
        let c = parse(
            r#"
            [[source]]
            id = "a"
            site = "https://a.gov"

            [[source]]
            id = "b"
            site = "https://b.gov"
            enabled = false
        "#,
        );
        let ids: Vec<&str> = c
            .selected(&[])
            .unwrap()
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids, ["a"]);

        // Naming a disabled source explicitly runs it — that is an instruction.
        let ids: Vec<&str> = c
            .selected(&["b".into()])
            .unwrap()
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids, ["b"]);
    }

    #[test]
    fn selecting_an_unknown_source_suggests_a_near_miss() {
        let c = parse("[[source]]\nid = \"tampa\"\nsite = \"https://t.gov\"\n");
        let err = c.selected(&["Tampa".into()]).unwrap_err().to_string();
        assert!(err.contains("did you mean `tampa`"), "{err}");
    }

    #[test]
    fn defaults_apply_when_the_table_is_absent_or_partial() {
        let c = parse("[defaults]\nrps = 0.5\n");
        assert_eq!(c.defaults.rps, 0.5);
        assert_eq!(c.defaults.lang, "en");
    }

    #[test]
    fn a_zero_crawl_rate_is_rejected() {
        assert!(Config::parse("[defaults]\nrps = 0.0\n").is_err());
        assert!(
            Config::parse("[[source]]\nid = \"a\"\nsite = \"https://a.gov\"\nrps = -1.0\n")
                .is_err()
        );
    }

    // ── writing ────────────────────────────────────────────────────────────────

    fn temp(text: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("centinel.toml");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    /// The property the whole textual-edit decision buys.
    #[test]
    fn appending_preserves_comments_and_everything_above_it() {
        let original = "# my notes, hard won\n\n[open]\npdf = \"Preview\"  # inline, too\n";
        let (_d, path) = temp(original);

        append_source(&path, &SourceConfig::site("tampa", "https://www.tampa.gov")).unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.starts_with(original),
            "the prefix was rewritten:\n{after}"
        );
        assert!(after.contains("# my notes, hard won"));
        assert!(after.contains("# inline, too"));

        let c = Config::parse(&after).unwrap();
        assert_eq!(c.sources.len(), 1);
        assert_eq!(c.sources[0].id, "tampa");
        assert_eq!(c.open.opener_for("pdf"), "Preview");
    }

    #[test]
    fn appending_to_a_missing_file_creates_one_from_the_example() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("centinel.toml");
        append_source(
            &path,
            &SourceConfig::channel("council", "https://youtube.com/@x"),
        )
        .unwrap();

        let c = Config::from_file(&path).unwrap();
        assert_eq!(c.sources.len(), 1);
        assert!(std::fs::read_to_string(&path).unwrap().contains("[open]"));
    }

    #[test]
    fn appending_a_duplicate_is_refused() {
        let (_d, path) = temp("[[source]]\nid = \"tampa\"\nsite = \"https://t.gov\"\n");
        let err = append_source(&path, &SourceConfig::site("tampa", "https://other.gov"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("already in"), "{err}");
    }

    #[test]
    fn removing_takes_the_block_and_leaves_its_neighbours() {
        let (_d, path) = temp(
            "[open]\npdf = \"Preview\"\n\n\
             [[source]]\nid = \"a\"\nsite = \"https://a.gov\"\n\n\
             [[source]]\nid = \"b\"\nsite = \"https://b.gov\"\nrps = 0.5\n\n\
             [[source]]\nid = \"c\"\nsite = \"https://c.gov\"\n",
        );
        remove_source(&path, "b").unwrap();

        let c = Config::from_file(&path).unwrap();
        let ids: Vec<&str> = c.sources.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["a", "c"]);
        assert_eq!(c.open.opener_for("pdf"), "Preview");

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("b.gov"), "{text}");
        assert!(text.contains("a.gov") && text.contains("c.gov"), "{text}");
    }

    #[test]
    fn removing_the_last_source_leaves_a_valid_file() {
        let (_d, path) = temp("[[source]]\nid = \"only\"\nsite = \"https://x.gov\"\n");
        remove_source(&path, "only").unwrap();
        assert!(Config::from_file(&path).unwrap().sources.is_empty());
    }

    #[test]
    fn removing_an_absent_source_is_an_error() {
        let (_d, path) = temp("[[source]]\nid = \"a\"\nsite = \"https://a.gov\"\n");
        assert!(remove_source(&path, "zzz").is_err());
    }

    /// A `[[source]]` followed by another table — the splice has to stop at the header
    /// rather than swallowing it. The verify step is what makes that safe.
    #[test]
    fn a_source_followed_by_another_table_splices_correctly() {
        let (_d, path) = temp(
            "[[source]]\nid = \"a\"\nsite = \"https://a.gov\"\n\n\
             [defaults]\nrps = 2.0\n\n\
             [[source]]\nid = \"b\"\nsite = \"https://b.gov\"\n",
        );
        remove_source(&path, "a").unwrap();
        let c = Config::from_file(&path).unwrap();
        assert_eq!(c.sources.len(), 1);
        assert_eq!(c.sources[0].id, "b");
        assert_eq!(c.defaults.rps, 2.0);
    }

    #[test]
    fn add_then_remove_returns_the_file_to_its_content() {
        let original = "[open]\npdf = \"Preview\"\n";
        let (_d, path) = temp(original);
        append_source(&path, &SourceConfig::site("tampa", "https://t.gov")).unwrap();
        remove_source(&path, "tampa").unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(Config::parse(&after).unwrap().sources.is_empty());
        assert!(after.starts_with(original), "{after:?}");
    }

    #[test]
    fn a_round_trip_through_the_block_writer_preserves_every_field() {
        let source = SourceConfig {
            id: "tampa-council".into(),
            site: None,
            channel: Some("https://www.youtube.com/@CityofTampa".into()),
            enabled: Some(false),
            rps: Some(0.25),
            matches: vec!["/agenda/".into(), "budget".into()],
            yt_dlp_args: vec!["--cookies-from-browser=brave".into()],
            audio_if_no_captions: Some(true),
            lang: Some("es".into()),
        };
        let c = Config::parse(&source.to_toml_block()).unwrap();
        let got = &c.sources[0];
        assert_eq!(got.id, "tampa-council");
        assert_eq!(
            got.channel.as_deref(),
            Some("https://www.youtube.com/@CityofTampa")
        );
        assert_eq!(got.enabled, Some(false));
        assert_eq!(got.rps, Some(0.25));
        assert_eq!(got.matches, ["/agenda/", "budget"]);
        assert_eq!(got.yt_dlp_args, ["--cookies-from-browser=brave"]);
        assert_eq!(got.audio_if_no_captions, Some(true));
        assert_eq!(got.lang.as_deref(), Some("es"));
        assert!(!got.is_enabled());
    }

    /// Quoting has to survive a URL carrying characters the grammar cares about.
    #[test]
    fn block_values_are_escaped() {
        let s = SourceConfig::site("x", r#"https://x.gov/a"b\c"#);
        let c = Config::parse(&s.to_toml_block()).unwrap();
        assert_eq!(c.sources[0].site.as_deref(), Some(r#"https://x.gov/a"b\c"#));
    }

    /// The guard that makes splicing acceptable: a bad edit must not reach disk.
    #[test]
    fn a_verify_mismatch_refuses_the_write() {
        let path = Path::new("centinel.toml");
        let err = verify_edit(
            path,
            "[[source]]\nid=\"a\"\nsite=\"https://a.gov\"\n",
            &["b".into()],
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("left unchanged"), "{err}");
    }
}
