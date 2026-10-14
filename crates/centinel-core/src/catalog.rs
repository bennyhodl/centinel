//! The sources a store holds, and what each one is.
//!
//! **One owner.** A source is defined by `log/<id>/source.toml`, beside the log it feeds.
//! Every surface — the CLI, the web, MCP, the scheduler — asks this module what the
//! sources are, so two lists can no longer disagree. There used to be two: `[[source]]`
//! blocks in a config found by a search path that starts in the working directory, and
//! the store's own `log/` directories. A source collected by hand was "untracked", a
//! source removed from the config stayed searchable, and `source adopt` existed only to
//! copy one list into the other.
//!
//! ## Why the store, when the config once argued the opposite
//!
//! The config held sources so that handing someone a store would not hand them an
//! *intent to collect*. That property survives the move: what fires on its own is a
//! `[[schedule]]`, and schedules stay in the config, on the operator's machine. A
//! received store lists its sources and collects nothing until its new owner types a
//! command. What the store gains is that its list describes its data, wherever the
//! command was typed from.
//!
//! ## Old configs
//!
//! [`Sources::import`] writes each legacy `[[source]]` block, and an inferred definition
//! for each log directory that has none, once. It never overwrites a definition the store
//! already holds, so running it at every start is safe and cheap.

use std::collections::BTreeMap;

use crate::config::{Config, ScheduleConfig, SourceConfig};
use crate::domain::SourceId;
use crate::store::Store;

/// Editable collection settings. Replacing them never changes acquisition identity or
/// operator-only yt-dlp arguments. Omitted overrides inherit the pipeline's defaults.
#[derive(
    Clone, Debug, Default, clap::Args, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SourceSettings {
    /// Requests per second. Omit to inherit [defaults].
    #[arg(long)]
    pub rps: Option<f64>,
    /// Only collect URLs containing one of these substrings. Repeatable; omit for all.
    #[arg(long = "match")]
    #[serde(default)]
    pub matches: Vec<String>,
    /// Website discovery strategy. Omit to recognise it on each run.
    #[arg(long)]
    pub strategy: Option<String>,
    /// Channel caption and transcription language. Omit to inherit [defaults].
    #[arg(long)]
    pub lang: Option<String>,
    /// Fetch audio for videos without captions. Omit for the channel default (true).
    #[arg(long, action = clap::ArgAction::Set)]
    pub audio_if_no_captions: Option<bool>,
}

/// The store's sources. Cheap to make; holds nothing but the store.
pub struct Sources<'a> {
    store: &'a Store,
}

/// What [`Sources::import`] wrote.
#[derive(Debug, Default)]
pub struct Imported {
    /// Ids taken from `[[source]]` blocks in the config.
    pub from_config: Vec<String>,
    /// Ids whose definition was inferred from what their log holds.
    pub from_log: Vec<String>,
}

impl<'a> Sources<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// Every defined source, by id.
    ///
    /// A log directory with no definition is not listed: [`Sources::import`] writes one
    /// for every log that says where it came from, so what is left is a directory nothing
    /// can collect again.
    pub async fn list(&self) -> anyhow::Result<Vec<SourceConfig>> {
        let mut out = Vec::new();
        for id in self.store.sources().await? {
            if let Some(source) = self.read(&id)? {
                out.push(source);
            }
        }
        Ok(out)
    }

    /// Each defined source's id and the host it collects from.
    ///
    /// The one mapping the web needs to draw a source's icon: a source is named by its
    /// id everywhere in the page and by its host only here. A user may read it, which
    /// the full definitions are not for.
    pub async fn hosts(&self) -> anyhow::Result<BTreeMap<String, String>> {
        Ok(self
            .list()
            .await?
            .into_iter()
            .filter_map(|source| Some((source.id.clone(), source.host()?)))
            .collect())
    }

    /// The source with this id, if the store defines one.
    pub async fn get(&self, id: &str) -> anyhow::Result<Option<SourceConfig>> {
        match SourceId::new(id.to_string()) {
            Ok(id) => self.read(&id),
            Err(_) => Ok(None),
        }
    }

    /// The source with this id, or an error that names the ones there are.
    pub async fn require(&self, id: &str) -> anyhow::Result<SourceConfig> {
        if let Some(source) = self.get(id).await? {
            return Ok(source);
        }
        let all = self.list().await?;
        let close = all.iter().find(|s| {
            s.id.eq_ignore_ascii_case(id) || s.id.starts_with(id) || id.starts_with(&s.id)
        });
        anyhow::bail!(
            "no source `{id}` in {}{}",
            self.store.root().display(),
            match (close, all.is_empty()) {
                (Some(s), _) => format!(" (did you mean `{}`?)", s.id),
                (None, true) =>
                    " — it has no sources yet; add one with `centinel source add`".to_string(),
                (None, false) => String::new(),
            }
        )
    }

    /// The sources a run walks: every enabled one, or exactly those named.
    ///
    /// Naming a disabled source runs it. `enabled = false` is a default, and an explicit
    /// argument is an instruction.
    pub async fn selected(&self, only: &[String]) -> anyhow::Result<Vec<SourceConfig>> {
        if only.is_empty() {
            return Ok(self
                .list()
                .await?
                .into_iter()
                .filter(SourceConfig::is_enabled)
                .collect());
        }
        let mut out = Vec::with_capacity(only.len());
        for id in only {
            out.push(self.require(id).await?);
        }
        Ok(out)
    }

    /// Defines a new source. Refuses an id the store already defines.
    pub async fn add(&self, source: &SourceConfig) -> anyhow::Result<()> {
        let id = SourceId::new(source.id.clone())?;
        anyhow::ensure!(
            self.read(&id)?.is_none(),
            "source `{id}` already exists in {}",
            self.store.root().display()
        );
        self.write(&id, source)
    }

    /// Turns a source on or off. Off means a bare `run` skips it; naming it still runs it.
    pub async fn set_enabled(&self, id: &str, enabled: bool) -> anyhow::Result<SourceConfig> {
        let mut source = self.require(id).await?;
        source.enabled = (!enabled).then_some(false);
        self.write(&SourceId::new(source.id.clone())?, &source)?;
        Ok(source)
    }

    /// Replaces collection settings, preserving every other field of the definition.
    /// All validation happens before the atomic write, so a rejected edit changes nothing.
    pub async fn update(&self, id: &str, settings: SourceSettings) -> anyhow::Result<SourceConfig> {
        let mut source = self.require(id).await?;
        if let Some(name) = &settings.strategy {
            crate::strategies::crawl::by_name(name)?;
        }
        anyhow::ensure!(
            settings
                .matches
                .iter()
                .all(|s| !s.trim().is_empty() && !s.chars().any(char::is_control)),
            "URL filters must be nonempty substrings without control characters"
        );
        if let Some(lang) = &settings.lang {
            anyhow::ensure!(
                !lang.is_empty() && lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "language must be a language code such as en or en-US"
            );
        }
        source.rps = settings.rps;
        source.matches = settings.matches;
        source.strategy = settings.strategy;
        source.lang = settings.lang;
        source.audio_if_no_captions = settings.audio_if_no_captions;
        self.write(&SourceId::new(source.id.clone())?, &source)?;
        Ok(source)
    }

    /// The first source this schedule names that the store does not define.
    pub async fn missing_from(&self, schedule: &ScheduleConfig) -> anyhow::Result<Option<String>> {
        for id in &schedule.sources {
            if self.get(id).await?.is_none() {
                return Ok(Some(id.clone()));
            }
        }
        Ok(None)
    }

    /// Refuses a config whose schedules name a source the store does not define.
    ///
    /// A source id that names nothing is the mistake a schedule invites most: it fires on
    /// time, collects nothing, and reports success. So `serve` asks this before it starts
    /// and `schedule add` before it writes.
    pub async fn check_schedules(&self, config: &Config) -> anyhow::Result<()> {
        for schedule in &config.schedules {
            if let Some(id) = self.missing_from(schedule).await? {
                anyhow::bail!(
                    "schedule `{}` names source `{id}`, which {} does not hold",
                    schedule.id,
                    self.store.root().display()
                );
            }
        }
        Ok(())
    }

    /// Writes a definition for every legacy `[[source]]` block, then for every log
    /// directory that has none but can say where it came from. Never overwrites.
    pub async fn import(&self, config: &Config) -> anyhow::Result<Imported> {
        let mut imported = Imported::default();
        for source in &config.sources {
            let id = SourceId::new(source.id.clone())?;
            if self.read(&id)?.is_none() {
                source.acquisition()?;
                self.write(&id, source)?;
                imported.from_config.push(source.id.clone());
            }
        }
        for id in self.store.sources().await? {
            if self.store.definition_path(&id).exists() {
                continue;
            }
            let replay = self.store.replay(&id).await?;
            if let Some(source) = crate::sources::infer_from(self.store, &replay)
                .await?
                .and_then(|inferred| inferred.to_config(&id))
            {
                self.write(&id, &source)?;
                imported.from_log.push(id.to_string());
            }
        }
        Ok(imported)
    }

    fn read(&self, id: &SourceId) -> anyhow::Result<Option<SourceConfig>> {
        let path = self.store.definition_path(id);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => anyhow::bail!("reading {}: {e}", path.display()),
        };
        let source: SourceConfig = toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("{} does not parse: {e}", path.display()))?;
        anyhow::ensure!(
            source.id == id.as_str(),
            "{} defines `{}`, but it sits in the directory for `{id}`",
            path.display(),
            source.id
        );
        Ok(Some(source))
    }

    /// Atomic, so a crash cannot leave half a definition.
    fn write(&self, id: &SourceId, source: &SourceConfig) -> anyhow::Result<()> {
        source.validate()?;
        crate::store::write_atomically(&self.store.definition_path(id), toml::to_string(source)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejected_settings_leave_the_definition_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let sources = Sources::new(&store);
        let original = SourceConfig {
            rps: Some(0.5),
            matches: vec!["/agenda/".into()],
            strategy: Some("listing".into()),
            ..SourceConfig::site("city", "https://city.gov")
        };
        sources.add(&original).await.unwrap();
        let path = store.definition_path(&SourceId::new("city").unwrap());
        let before = std::fs::read(&path).unwrap();
        let invalid = [
            (
                SourceSettings {
                    rps: Some(0.0),
                    ..Default::default()
                },
                "rps",
            ),
            (
                SourceSettings {
                    rps: Some(-1.0),
                    ..Default::default()
                },
                "rps",
            ),
            (
                SourceSettings {
                    rps: Some(f64::INFINITY),
                    ..Default::default()
                },
                "finite",
            ),
            (
                SourceSettings {
                    rps: Some(f64::NAN),
                    ..Default::default()
                },
                "finite",
            ),
            (
                SourceSettings {
                    matches: vec![" ".into()],
                    ..Default::default()
                },
                "filters",
            ),
            (
                SourceSettings {
                    matches: vec!["agenda\nminutes".into()],
                    ..Default::default()
                },
                "filters",
            ),
            (
                SourceSettings {
                    strategy: Some("not-a-strategy".into()),
                    ..Default::default()
                },
                "strategy",
            ),
            (
                SourceSettings {
                    lang: Some("../../tmp".into()),
                    ..Default::default()
                },
                "language",
            ),
            (
                SourceSettings {
                    lang: Some("".into()),
                    ..Default::default()
                },
                "language",
            ),
        ];
        for (settings, message) in invalid {
            let error = sources.update("city", settings).await.unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
        assert!(
            sources
                .update("missing", SourceSettings::default())
                .await
                .is_err()
        );
        assert_eq!(sources.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_source_added_is_listed_and_survives_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        Sources::new(&store)
            .add(&SourceConfig::site("tampa", "https://www.tampa.gov"))
            .await
            .unwrap();

        let reopened = Store::open(dir.path()).await.unwrap();
        let listed = Sources::new(&reopened).list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].site.as_deref(), Some("https://www.tampa.gov"));
    }

    #[tokio::test]
    async fn adding_an_id_twice_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let sources = Sources::new(&store);
        sources
            .add(&SourceConfig::site("tampa", "https://www.tampa.gov"))
            .await
            .unwrap();
        let err = sources
            .add(&SourceConfig::site("tampa", "https://other.gov"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[tokio::test]
    async fn a_disabled_source_is_skipped_unless_named() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let sources = Sources::new(&store);
        sources
            .add(&SourceConfig::site("a", "https://a.gov"))
            .await
            .unwrap();
        sources
            .add(&SourceConfig::site("b", "https://b.gov"))
            .await
            .unwrap();
        sources.set_enabled("b", false).await.unwrap();

        let all: Vec<String> = sources
            .selected(&[])
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(all, ["a"]);
        let named = sources.selected(&["b".into()]).await.unwrap();
        assert_eq!(named[0].id, "b");
    }

    #[tokio::test]
    async fn import_takes_config_blocks_once_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let sources = Sources::new(&store);
        sources
            .add(&SourceConfig::site("tampa", "https://www.tampa.gov"))
            .await
            .unwrap();

        let config = Config::parse(
            "[[source]]\nid = \"tampa\"\nsite = \"https://elsewhere.gov\"\n\n\
             [[source]]\nid = \"hillsborough\"\nsite = \"https://hcfl.gov\"\n",
        )
        .unwrap();
        let first = sources.import(&config).await.unwrap();
        assert_eq!(first.from_config, ["hillsborough"]);
        let again = sources.import(&config).await.unwrap();
        assert!(again.from_config.is_empty());

        let tampa = sources.get("tampa").await.unwrap().unwrap();
        assert_eq!(
            tampa.site.as_deref(),
            Some("https://www.tampa.gov"),
            "the store's own definition wins"
        );
    }

    #[tokio::test]
    async fn a_schedule_naming_a_source_the_store_lacks_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let sources = Sources::new(&store);
        sources
            .add(&SourceConfig::site("tampa", "https://www.tampa.gov"))
            .await
            .unwrap();

        let ok =
            Config::parse("[[schedule]]\nid = \"x\"\ncron = \"@daily\"\nsources = [\"tampa\"]\n")
                .unwrap();
        sources.check_schedules(&ok).await.unwrap();

        let bad =
            Config::parse("[[schedule]]\nid = \"x\"\ncron = \"@daily\"\nsources = [\"orlando\"]\n")
                .unwrap();
        let err = sources.check_schedules(&bad).await.unwrap_err().to_string();
        assert!(err.contains("orlando"), "{err}");
    }

    /// Every key a definition can carry comes back out of `source.toml` unchanged.
    #[tokio::test]
    async fn a_definition_round_trips_every_field() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let written = SourceConfig {
            enabled: Some(false),
            rps: Some(0.25),
            matches: vec!["/agenda/".into(), "budget".into()],
            yt_dlp_args: vec!["--cookies-from-browser=brave".into()],
            audio_if_no_captions: Some(true),
            lang: Some("es".into()),
            strategy: Some("listing".into()),
            ..SourceConfig::site("tampa", r#"https://x.gov/a"b\c"#)
        };
        Sources::new(&store).add(&written).await.unwrap();
        let read = Sources::new(&store).get("tampa").await.unwrap().unwrap();
        assert_eq!(
            toml::to_string(&read).unwrap(),
            toml::to_string(&written).unwrap()
        );
    }

    #[tokio::test]
    async fn an_unknown_id_names_a_close_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let sources = Sources::new(&store);
        sources
            .add(&SourceConfig::site("tampa", "https://www.tampa.gov"))
            .await
            .unwrap();
        let err = sources.require("tamp").await.unwrap_err();
        assert!(err.to_string().contains("did you mean `tampa`"), "{err}");
    }
}
