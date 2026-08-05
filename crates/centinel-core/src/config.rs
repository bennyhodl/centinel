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

/// Unknown keys are rejected rather than ignored.
///
/// The alternative is worse than it sounds for a config-driven tool: `[[sources]]` with
/// the plural typed by reflex would parse cleanly, contribute nothing, and leave
/// `centinel run` reporting "no sources configured" at someone looking straight at the
/// source they just added.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
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
}

/// Which application opens which kind of document.
///
/// Keys are the content kinds from [`crate::fetch::content_kind`] — `pdf`, `html`,
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
    /// 3. `~/.config/centinel/config.toml` — per-user
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
        Ok(())
    }

    /// The config file in effect, or `None` when none exists and defaults are in use.
    pub fn locate() -> Option<PathBuf> {
        Self::search_paths().into_iter().find(|p| p.is_file())
    }

    /// Where a config file would be *written*: the one in effect, else `./centinel.toml`.
    ///
    /// Not `~/.config`: a store defaults to `.centinel` in the working directory, so the
    /// sources feeding it belong beside it and travel with the project.
    pub fn write_path() -> PathBuf {
        Self::locate().unwrap_or_else(|| {
            std::env::var("CENTINEL_CONFIG")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(DEFAULT_FILENAME))
        })
    }

    /// Where [`Self::load`] looks, in order. Exposed so `doctor` can report it.
    pub fn search_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(explicit) = std::env::var("CENTINEL_CONFIG") {
            paths.push(PathBuf::from(explicit));
        }
        paths.push(PathBuf::from(DEFAULT_FILENAME));
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(
                PathBuf::from(home)
                    .join(".config")
                    .join("centinel")
                    .join("config.toml"),
            );
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
    let ranges = source_block_ranges(&lines);
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
fn source_block_ranges(lines: &[&str]) -> Vec<(usize, usize)> {
    let is_header = |l: &str| {
        let t = l.trim_start();
        t.starts_with('[') && !t.starts_with('#')
    };
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == "[[source]]" {
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
fn block_declares(block: &[&str], id: &str) -> bool {
    let text = block.join("\n");
    toml::from_str::<Config>(&text)
        .ok()
        .and_then(|c| c.sources.first().map(|s| s.id == id))
        .unwrap_or(false)
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
    }

    #[test]
    fn search_paths_are_ordered_nearest_first() {
        let paths = Config::search_paths();
        let idx = |needle: &str| {
            paths
                .iter()
                .position(|p| p.to_string_lossy().contains(needle))
        };
        assert!(idx("centinel.toml").unwrap() < idx(".config").unwrap());
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
