//! Configuration.
//!
//! SPEC §8 lists the concrete config schema as unspecified, and this does **not** settle
//! it. It adds the one section `open` needs, in a file shaped so the rest can grow into
//! it later. Treat `[open]` as settled and everything else as unwritten.
//!
//! ## Why this does not live in the store
//!
//! Which application opens a PDF is a property of *your machine*, not of the corpus.
//! The store is `rsync`-able and meant to be handed to other people (SPEC §5.4); baking
//! one operator's choice of PDF reader into it would travel badly. So config is looked
//! up outside the store, and the store stays portable.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The sentinel meaning "let the operating system decide".
pub const SYSTEM_DEFAULT: &str = "system";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub open: OpenConfig,
}

/// Which application opens which kind of document.
///
/// Keys are the content kinds from [`crate::fetch::content_kind`] — `pdf`, `html`,
/// `markdown`, `spreadsheet`, `text`.
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

impl Config {
    /// Loads the first config file found, or defaults.
    ///
    /// Precedence, nearest first:
    /// 1. `$CENTINEL_CONFIG`
    /// 2. `./centinel.toml` — per-project
    /// 3. `~/.config/centinel/config.toml` — per-user
    pub fn load() -> anyhow::Result<Self> {
        for path in Self::search_paths() {
            if path.is_file() {
                return Self::from_file(&path);
            }
        }
        Ok(Self::default())
    }

    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))
    }

    /// Where [`Self::load`] looks, in order. Exposed so `doctor` can report it.
    pub fn search_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(explicit) = std::env::var("CENTINEL_CONFIG") {
            paths.push(PathBuf::from(explicit));
        }
        paths.push(PathBuf::from("centinel.toml"));
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

    /// A starter file, written by `centinel open --write-config`.
    pub fn example() -> &'static str {
        r#"# Centinel configuration.
#
# Only [open] is currently read. The rest of the schema is not yet specified.

[open]
# Either an application name, or a command template containing {path}.
#   pdf      = "Adobe Acrobat"
#   markdown = "Obsidian"
#   html     = "Safari"
#   text     = "nvim {path}"
#
# "system" hands the file to the OS default handler.
default = "system"
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Config {
        toml::from_str(s).unwrap()
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
    }

    #[test]
    fn command_templates_are_stored_verbatim() {
        let c = parse("[open]\ntext = \"nvim {path}\"");
        assert_eq!(c.open.opener_for("text"), "nvim {path}");
    }

    #[test]
    fn the_example_config_parses() {
        let c: Config = toml::from_str(Config::example()).unwrap();
        assert_eq!(c.open.opener_for("pdf"), SYSTEM_DEFAULT);
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
}
