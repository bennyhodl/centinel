//! Where this binary came from, and what a newer one would be.
//!
//! `install.sh` decides one thing before it does anything else: *a curl pipe installs from
//! git, a clone installs the clone*. This module is the same decision at the other end of
//! the binary's life, read off the stamps `build.rs` left rather than off the working
//! directory — see that file for why the difference matters.
//!
//! Nothing here touches the network or spawns a process. It answers *which* of the two
//! authorities is this build's, and how two version strings compare; asking either of them
//! anything is [`crate::ops::update`]'s job.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// This build's version. Every crate in the workspace shares it, so the number the
/// library reports and the number `centinel --version` prints cannot drift.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The repository, read off the manifest rather than written out a second time. It is
/// already the one `install.sh` clones and the one `cargo install --git` fetches.
pub const REPO: &str = env!("CARGO_PKG_REPOSITORY");

/// Where the sources were when this binary was built, and what they were. See `build.rs`.
const BUILD_SRC: &str = env!("CENTINEL_BUILD_SRC");
const BUILD_COMMIT: &str = env!("CENTINEL_BUILD_COMMIT");

/// What this build knows about itself.
#[derive(Clone, Debug)]
pub struct Build {
    pub version: &'static str,
    /// The directory these sources sat in. It may no longer exist; [`Build::origin`] is
    /// what decides whether it is anything.
    pub src: PathBuf,
    /// The commit those sources were at. `None` when git could not say — a tarball, or a
    /// machine with no git on it.
    pub commit: Option<String>,
}

impl Build {
    /// This binary's own provenance.
    pub fn current() -> Self {
        Self {
            version: VERSION,
            src: PathBuf::from(BUILD_SRC),
            commit: (!BUILD_COMMIT.is_empty()).then(|| BUILD_COMMIT.to_string()),
        }
    }

    pub fn origin(&self) -> Origin {
        origin(&self.src)
    }
}

/// Which authority decides what a newer Centinel is for this build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// A clone somebody made. The one origin whose update begins with a `git pull`,
    /// because it is the one whose sources a person can hold and change.
    Clone { path: PathBuf },
    /// Cargo's own checkout — what `cargo install --git` leaves behind, which is what the
    /// curl pipe leaves behind. It looks exactly like a clone and can never be treated as
    /// one: it is pinned to a single revision and its `origin` is a bare mirror inside
    /// `$CARGO_HOME`, so a `git pull` there fetches from this machine and finds what this
    /// machine already had. The repository is this build's authority, not the directory.
    Cargo { path: PathBuf },
    /// Nothing on this machine to point at: the sources were moved, deleted, or were
    /// never a checkout. The repository answers, and the path is kept so the report can
    /// say which directory it looked for.
    Unknown { path: PathBuf, reason: String },
}

impl Origin {
    /// The directory this origin names, whatever it turned out to be.
    pub fn path(&self) -> &Path {
        match self {
            Self::Clone { path } | Self::Cargo { path } | Self::Unknown { path, .. } => path,
        }
    }
}

/// Classifies the directory a build came from.
///
/// The order matters and is the whole of the function: cargo's checkout carries the same
/// workspace and the same `.git` a clone does, so it has to be ruled out **first**. Read
/// the other way round, every pipe install on earth would be told to `git pull` a
/// directory whose upstream is a copy of itself.
pub fn origin(src: &Path) -> Origin {
    let path = src.to_path_buf();

    if is_cargo_checkout(src) {
        return Origin::Cargo { path };
    }
    if !src.join("crates/centinel/Cargo.toml").is_file() {
        return Origin::Unknown {
            path,
            reason: "the sources this was built from are not there any more".to_string(),
        };
    }
    if !src.join(".git").exists() {
        return Origin::Unknown {
            path,
            reason: "the sources are there, but they are not a checkout".to_string(),
        };
    }
    Origin::Clone { path }
}

/// Whether a directory is one cargo made for itself.
///
/// Two independent signs, either sufficient. `.cargo-ok` is the file cargo writes into a
/// checkout once it is complete, and the path is where it puts every one of them —
/// checked against `$CARGO_HOME` rather than by looking for `git/checkouts` anywhere in
/// the path, which would misread a clone somebody keeps in `~/git/checkouts`.
fn is_cargo_checkout(src: &Path) -> bool {
    if src.join(".cargo-ok").is_file() {
        return true;
    }
    cargo_checkouts().is_some_and(|dir| src.starts_with(dir))
}

fn cargo_checkouts() -> Option<PathBuf> {
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))?;
    Some(home.join("git").join("checkouts"))
}

/// `owner`, `repo` — the two segments GitHub's API is addressed by.
pub fn slug() -> Option<(&'static str, &'static str)> {
    let rest = REPO.trim_end_matches('/').trim_end_matches(".git");
    let mut segments = rest.rsplit('/');
    let repo = segments.next()?;
    let owner = segments.next()?;
    (!repo.is_empty() && !owner.is_empty() && owner != "https:").then_some((owner, repo))
}

/// The latest published release, as GitHub answers it.
pub fn releases_api() -> Option<String> {
    slug()
        .map(|(owner, repo)| format!("https://api.github.com/repos/{owner}/{repo}/releases/latest"))
}

/// `install.sh` **at a given revision**, which is the copy that installs that revision.
///
/// Pinned rather than read off the default branch: the script that installs a release is
/// the one that shipped with it, and a tag is a thing somebody can go and read.
pub fn install_script(rev: &str) -> Option<String> {
    slug().map(|(owner, repo)| {
        format!("https://raw.githubusercontent.com/{owner}/{repo}/{rev}/install.sh")
    })
}

/// The version a release tag names.
///
/// Deliberately strict: `vX.Y.Z` and nothing else. This repository carries two tags from
/// before the workspace existed — `v0.1.0-hermes`, `v0.2.0-pi` — which mark lineages
/// rather than versions of it, and reading one as a release would offer an "update" to
/// something that is not this program. A tag that does not parse is reported as it is
/// spelled and compared against nothing.
pub fn release_version(tag: &str) -> Option<&str> {
    let version = tag.strip_prefix('v')?;
    parts(version).map(|_| version)
}

/// Whether `candidate` is a later version than `installed`. Neither being parseable is a
/// no, because "I cannot tell" must never read as "yes, update".
pub fn is_newer(candidate: &str, installed: &str) -> bool {
    compare(candidate, installed) == Some(Ordering::Greater)
}

/// Two dotted versions, compared field by field so `1.100` beats `1.9`.
pub fn compare(a: &str, b: &str) -> Option<Ordering> {
    Some(parts(a)?.cmp(&parts(b)?))
}

fn parts(v: &str) -> Option<(u64, u64, u64)> {
    let mut fields = v.split('.');
    let major = fields.next()?.parse().ok()?;
    let minor = fields.next()?.parse().ok()?;
    let patch = fields.next()?.parse().ok()?;
    // Three fields exactly. A trailing `-rc1` or a fourth number is not a release of this
    // workspace, and guessing at what it means is how a pre-release is offered as an
    // upgrade over the version somebody is running.
    fields.next().is_none().then_some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The workspace this file is in. The stamp has to survive being read back, or
    /// `update` starts from a path that was never a directory.
    #[test]
    fn this_build_knows_where_it_came_from() {
        let build = Build::current();
        assert!(!build.version.is_empty());
        assert!(
            build.src.is_absolute(),
            "the stamp is a relative path: {}",
            build.src.display()
        );
        assert_eq!(slug(), Some(("bennyhodl", "centinel")), "{REPO}");
    }

    /// Cargo's checkout carries a full workspace and a `.git`, so the layout test alone
    /// calls it a clone — and then `update` would offer to pull a mirror of this machine.
    #[test]
    fn cargo_s_own_checkout_is_never_a_clone() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path();
        std::fs::create_dir_all(src.join("crates/centinel")).unwrap();
        std::fs::write(src.join("crates/centinel/Cargo.toml"), "[package]").unwrap();
        std::fs::create_dir_all(src.join(".git")).unwrap();

        assert!(matches!(origin(src), Origin::Clone { .. }));

        // The file cargo writes when it has finished making a checkout.
        std::fs::write(src.join(".cargo-ok"), "ok").unwrap();
        assert!(matches!(origin(src), Origin::Cargo { .. }));
    }

    #[test]
    fn sources_that_are_gone_are_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("moved-away");
        assert!(matches!(origin(&missing), Origin::Unknown { .. }));

        // A workspace with no checkout in it — a tarball, or a clone whose `.git` was
        // deleted. There is nothing to pull, and that is a fact rather than a fault.
        std::fs::create_dir_all(missing.join("crates/centinel")).unwrap();
        std::fs::write(missing.join("crates/centinel/Cargo.toml"), "[package]").unwrap();
        match origin(&missing) {
            Origin::Unknown { reason, .. } => {
                assert!(reason.contains("not a checkout"), "{reason}")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn versions_compare_field_by_field() {
        assert!(is_newer("0.6.0", "0.5.0"));
        assert!(is_newer("1.0.0", "0.99.99"));
        // The reason this is not a string comparison.
        assert!(is_newer("0.100.0", "0.9.0"));
        assert!(!is_newer("0.5.0", "0.5.0"));
        assert!(!is_newer("0.4.9", "0.5.0"));
    }

    /// "I cannot tell" must never read as "yes, update".
    #[test]
    fn an_unreadable_version_is_never_newer() {
        assert!(!is_newer("nightly", "0.5.0"));
        assert!(!is_newer("0.6", "0.5.0"));
        assert!(!is_newer("0.6.0-rc1", "0.5.0"));
        assert_eq!(compare("0.6.0", "not-a-version"), None);
    }

    /// The two tags that predate this workspace mark lineages, not releases of it.
    #[test]
    fn only_a_plain_v_x_y_z_tag_is_a_release() {
        assert_eq!(release_version("v0.5.0"), Some("0.5.0"));
        assert_eq!(release_version("v0.1.0-hermes"), None);
        assert_eq!(release_version("v0.2.0-pi"), None);
        assert_eq!(release_version("0.5.0"), None);
        assert_eq!(release_version("latest"), None);
    }

    /// Both URLs are built from the manifest's repository, so a fork is addressed
    /// correctly without a second place to edit.
    #[test]
    fn the_urls_follow_the_manifest() {
        assert_eq!(
            releases_api().as_deref(),
            Some("https://api.github.com/repos/bennyhodl/centinel/releases/latest")
        );
        assert_eq!(
            install_script("v0.6.0").as_deref(),
            Some("https://raw.githubusercontent.com/bennyhodl/centinel/v0.6.0/install.sh")
        );
    }
}
