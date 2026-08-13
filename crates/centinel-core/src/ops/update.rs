//! `update` — is there a newer Centinel, and installing it.
//!
//! Two authorities, asked in that order: **the repo, then GitHub**.
//!
//! The repo is the clone this binary was built from, and it is asked first because it is
//! the one that can be ahead of everything else — a contributor's checkout holds commits
//! no release carries, and it is what `./install.sh` in that directory would install. It
//! is only asked at all when there is one: [`crate::version::origin`] is what decides
//! that, off a stamp `build.rs` left, and cargo's own checkout is not a clone however
//! much it looks like one.
//!
//! GitHub is asked always, because it is the authority for every install that has no
//! clone behind it, and because a clone that has not been fetched from in a month still
//! wants to be told a release happened. The two answers do not compete: master ahead of
//! the last tag is the ordinary state of a clone, and both facts go in the report.
//!
//! ## It installs, and it installs the way you installed
//!
//! `centinel update` installs unless `--check` says otherwise, and what it runs is
//! `install.sh` — the same script for a clone as for a pipe, because everything an
//! install decides is decided there: a release binary or a build, the accelerator, the
//! CPU tuning, and Centinel's **two binaries into one directory**. Reproducing any of
//! that here would be a second copy of the installer, wrong the first time the real one
//! changed. So this op decides *which* sources and *where*, and the script decides
//! everything else.
//!
//! The clone case runs the script that is already on disk. The pipe case fetches the one
//! **at the release tag** — the copy that shipped with the thing being installed, at an
//! address a person can go and read — and says so on stderr before it runs it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fetch::Fetcher;
use crate::prelude::*;
use crate::tool::Tool;
use crate::version::{self, Build, Origin};

/// A git question is local and should answer instantly; a fetch talks to a server.
///
/// Both are bounded because [`Tool`] gives a child no stdin: git asking for a passphrase
/// it can never be given would otherwise wait for a person who cannot answer.
const GIT_TIMEOUT: Duration = Duration::from_secs(20);
const FETCH_TIMEOUT: Duration = Duration::from_secs(120);

/// GitHub's API, and a download of one small script. Short: this op is typed by somebody
/// waiting for an answer, and an unreachable host must not look like a hang.
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

/// Which authority this build's update comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InstalledFrom {
    /// A clone on this machine.
    Clone,
    /// Cargo's checkout of the repository — `cargo install --git`, which is what the curl
    /// pipe leaves behind when it builds.
    Git,
    /// A prebuilt binary — a release asset, which is what the curl pipe leaves behind
    /// when a release carries one this host can run.
    Release,
    /// Neither could be found. The repository still answers; nothing local does.
    Unknown,
}

/// Where this binary came from, as the report states it.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Provenance {
    pub installed_from: InstalledFrom,
    /// The directory the sources were in when this was built. Present even when it is
    /// nothing any more, because "I looked here" is the first thing to check.
    pub path: String,
    /// The commit those sources were at. Absent when git could not say at build time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Why that directory is not a clone, when it is not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// What the clone says.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RepoCheck {
    pub path: String,
    /// The checked-out branch. Absent when `HEAD` is detached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The commit the checkout is on now, in full. The report prints an abbreviation.
    pub head: String,
    /// Commits the checkout holds that this binary was not built from — work that is on
    /// disk and not in the program you are running.
    ///
    /// `None` when the build commit is not in this history at all, which happens after a
    /// rebase or when the binary was built somewhere else. Not zero: "nothing new" and
    /// "these two are unrelated" are different answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_build: Option<usize>,
    /// Commits the upstream branch holds that the checkout does not. `None` when the
    /// branch tracks nothing, or when the fetch below failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behind_origin: Option<usize>,
    /// Uncommitted changes in the working tree. Not a fault — it is a contributor's
    /// ordinary state — but it decides whether a pull may be attempted.
    pub dirty: bool,
    /// Whether `git fetch` reached the remote. A stale `behind_origin` is worth less than
    /// no answer, so the report says which one it is holding.
    pub fetched: bool,
}

impl RepoCheck {
    /// Commits this checkout can reach that the running binary was not built from. The
    /// two ranges are disjoint by construction — `build..HEAD` and `HEAD..upstream` —
    /// so they add.
    pub fn pending(&self) -> usize {
        self.since_build.unwrap_or(0) + self.behind_origin.unwrap_or(0)
    }

    /// Whether a `git pull` is part of the update.
    pub fn needs_pull(&self) -> bool {
        self.behind_origin.unwrap_or(0) > 0
    }
}

/// What GitHub says.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReleaseCheck {
    pub tag: String,
    /// The version the tag names, when it names one. See
    /// [`crate::version::release_version`] for why a tag may not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
    pub url: String,
    /// Later than the running version. False whenever that could not be decided.
    pub newer: bool,
}

/// What an install actually did.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Applied {
    /// The command that ran, for the record and for anybody repeating it by hand.
    pub command: String,
    pub from: String,
    /// What the installed binary answers now. `None` when it could not be asked — which
    /// is not a failed install, only an unconfirmed one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct UpdateReport {
    /// The version that is running. After an install it is the version that *was*
    /// running — [`Applied::to`] carries the new one.
    pub installed: String,
    pub provenance: Provenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<RepoCheck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<ReleaseCheck>,
    /// Anything that could not be asked, and why. A question that went unanswered is not
    /// the same as a question answered "no", and this op must never conflate them: an
    /// unreachable GitHub reporting "up to date" is the one output that would make the
    /// command not worth running.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
    /// Whether either authority answered at all.
    ///
    /// The peer of `newer`, and the reason it is a separate field: `newer: false` with
    /// nothing answered is *nobody said*, not *nothing is newer*. Collapsed into one
    /// boolean, an unreachable GitHub and a current install are the same output — and the
    /// second is the one people act on.
    pub answered: bool,
    /// Something newer exists.
    pub newer: bool,
    /// The command that installs it, whether or not this run ran it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied: Option<Applied>,
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct UpdateArgs {
    /// Report what is newer and stop, without building anything.
    #[arg(long)]
    #[serde(default)]
    pub check: bool,
}

/// Check the repo and GitHub for a newer Centinel, and install it.
///
/// `reach = "host"` for the same reason as `models`: this op replaces the binaries on the
/// machine it runs on. Nothing arriving on a socket may cause that, and neither may the
/// scheduler — an update that lands at 3am mid-collection is the definition of an ambush.
#[op(reach = "host", group = "host")]
pub async fn update(_ctx: &Ctx, args: UpdateArgs) -> anyhow::Result<UpdateReport> {
    let build = Build::current();
    let origin = build.origin();
    let mut notes = Vec::new();

    // 1. The repo — only when there is one, and only ever the one that built this.
    let repo = match &origin {
        Origin::Clone { path } => Some(check_repo(path, &build, &mut notes).await),
        _ => None,
    };

    // 2. GitHub. Asked even with a clone in hand: a release is a fact about the project,
    //    not about this machine.
    let release = check_release(&mut notes).await;

    let newer =
        repo.as_ref().is_some_and(|r| r.pending() > 0) || release.as_ref().is_some_and(|r| r.newer);
    let fix = fix_command(&origin, repo.as_ref(), release.as_ref());

    let mut report = UpdateReport {
        installed: build.version.to_string(),
        provenance: provenance(&origin, &build),
        answered: repo.is_some() || release.is_some(),
        repo,
        release,
        notes,
        newer,
        fix,
        applied: None,
    };

    if args.check || !report.newer {
        return Ok(report);
    }

    report.applied = Some(apply(&origin, &report).await?);
    Ok(report)
}

// -----------------------------------------------------------------------------------------
// The repo
// -----------------------------------------------------------------------------------------

/// Fetches, then asks the checkout three questions.
///
/// A failed fetch is a note rather than an error: the local answers — a dirty tree,
/// commits made since the build — are worth having on a laptop with no network, and they
/// are the half most likely to be the reason somebody typed this.
async fn check_repo(path: &Path, build: &Build, notes: &mut Vec<Note>) -> RepoCheck {
    let fetched = match git(
        path,
        &["fetch", "--quiet", "--tags", "origin"],
        FETCH_TIMEOUT,
    )
    .await
    {
        Ok(_) => true,
        Err(e) => {
            notes.push(Note::marked(
                "fetch",
                format!("{e} — `behind origin` is what your last fetch knew"),
                NoteMark::Warn,
            ));
            false
        }
    };

    // `--abbrev-ref` answers the literal string `HEAD` when it is detached, which names
    // no branch and must not be printed as one.
    let branch = ask(path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .await
        .filter(|b| b.as_str() != "HEAD");
    let head = ask(path, &["rev-parse", "HEAD"]).await.unwrap_or_default();
    let dirty = ask(path, &["status", "--porcelain"])
        .await
        .is_some_and(|s| !s.is_empty());

    let since_build = match &build.commit {
        Some(commit) => {
            let counted = count(path, &format!("{commit}..HEAD")).await;
            if counted.is_none() {
                notes.push(Note::marked(
                    "build commit",
                    format!(
                        "{} is not in this checkout's history — it was rebased away, or this \
                         binary was built somewhere else",
                        render::short_sha(commit)
                    ),
                    NoteMark::Warn,
                ));
            }
            counted
        }
        None => {
            notes.push(Note::marked(
                "build commit",
                "this build carries no commit, so what is new since it cannot be counted"
                    .to_string(),
                NoteMark::Warn,
            ));
            None
        }
    };

    let behind_origin = count(path, "HEAD..@{upstream}").await;
    if behind_origin.is_none() && fetched {
        notes.push(Note::marked(
            "upstream",
            "this branch tracks nothing, so origin was not compared".to_string(),
            NoteMark::Warn,
        ));
    }

    RepoCheck {
        path: path.display().to_string(),
        branch,
        head,
        since_build,
        behind_origin,
        dirty,
        fetched,
    }
}

/// One git question against a checkout. Stdout, trimmed.
async fn git(dir: &Path, args: &[&str], timeout: Duration) -> anyhow::Result<String> {
    let out = Tool::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .timeout(timeout)
        .success()
        .await?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

/// [`git`], for a question whose failure is an answer: a range that names an unknown
/// commit, a branch that tracks nothing.
async fn ask(dir: &Path, args: &[&str]) -> Option<String> {
    git(dir, args, GIT_TIMEOUT).await.ok()
}

/// How many commits are in a range, or `None` when the range does not resolve.
async fn count(dir: &Path, range: &str) -> Option<usize> {
    ask(dir, &["rev-list", "--count", range])
        .await?
        .parse()
        .ok()
}

// -----------------------------------------------------------------------------------------
// GitHub
// -----------------------------------------------------------------------------------------

/// The shape of `releases/latest` this op reads. Everything else GitHub sends is ignored
/// by construction — `serde` drops unknown fields — so a change to the rest of that
/// payload cannot break the check.
#[derive(Deserialize)]
struct LatestRelease {
    tag_name: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    html_url: String,
}

/// The latest published release, or a note saying why there is no answer.
async fn check_release(notes: &mut Vec<Note>) -> Option<ReleaseCheck> {
    let url = version::releases_api()?;
    let policy = HostPolicy {
        timeout: HTTP_TIMEOUT,
        ..Default::default()
    };
    let fetcher = match Fetcher::new(&policy) {
        Ok(f) => f,
        Err(e) => {
            notes.push(Note::marked("github", e.to_string(), NoteMark::Warn));
            return None;
        }
    };

    let body = match fetcher.get(&url).await {
        Ok(fetched) => fetched.bytes,
        Err(refusal) => {
            // A 404 here is "no release has been published", which is a real state of a
            // young repository and not a fault of this machine.
            notes.push(Note::marked(
                "github",
                format!("{url} — {refusal}"),
                NoteMark::Warn,
            ));
            return None;
        }
    };

    let release: LatestRelease = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            notes.push(Note::marked(
                "github",
                format!("the release feed did not parse: {e}"),
                NoteMark::Warn,
            ));
            return None;
        }
    };

    // Named `released` rather than `version`: this module reads its helpers off
    // `version::`, and a binding of that name in scope is a trap for the next reader.
    let released = version::release_version(&release.tag_name).map(str::to_string);
    if released.is_none() {
        notes.push(Note::marked(
            "github",
            format!(
                "the latest release is tagged `{}`, which is not a vX.Y.Z release of this \
                 workspace — nothing was compared against it",
                release.tag_name
            ),
            NoteMark::Warn,
        ));
    }

    let newer = released
        .as_deref()
        .is_some_and(|v| version::is_newer(v, version::VERSION));

    Some(ReleaseCheck {
        url: if release.html_url.is_empty() {
            version::REPO.to_string()
        } else {
            release.html_url
        },
        tag: release.tag_name,
        version: released,
        published: release.published_at,
        newer,
    })
}

// -----------------------------------------------------------------------------------------
// Installing
// -----------------------------------------------------------------------------------------

/// The command that installs what was found — printed whether or not this run runs it,
/// because `--check` has to leave somebody with something to type.
fn fix_command(
    origin: &Origin,
    repo: Option<&RepoCheck>,
    release: Option<&ReleaseCheck>,
) -> Option<String> {
    match origin {
        Origin::Clone { path } => {
            let pull = repo.is_some_and(RepoCheck::needs_pull);
            let dir = path.display();
            Some(if pull {
                format!("cd {dir} && git pull && ./install.sh")
            } else {
                format!("cd {dir} && ./install.sh")
            })
        }
        // The line from the README, pinned to the release when there is one: a tag is
        // reviewable and `master` is whatever it happens to be at the moment of the pipe.
        _ => {
            let rev = release
                .filter(|r| r.newer)
                .map(|r| r.tag.as_str())
                .unwrap_or("master");
            let url = version::install_script(rev)?;
            Some(match release.filter(|r| r.newer) {
                Some(r) => format!(
                    "curl --proto '=https' --tlsv1.2 -sSf {url} | sh -s -- --tag {}",
                    r.tag
                ),
                None => format!("curl --proto '=https' --tlsv1.2 -sSf {url} | sh"),
            })
        }
    }
}

/// Builds and installs, by running `install.sh`.
async fn apply(origin: &Origin, report: &UpdateReport) -> anyhow::Result<Applied> {
    let bin_dir = install_dir();
    let (script, tag, _scratch) = match origin {
        Origin::Clone { path } => {
            if let Some(repo) = &report.repo
                && repo.needs_pull()
            {
                anyhow::ensure!(
                    !repo.dirty,
                    "{} has uncommitted changes and is {} commits behind origin. Commit or \
                     stash them and run this again, or `centinel update --check` to see the \
                     state without touching it.",
                    path.display(),
                    repo.behind_origin.unwrap_or(0),
                );
                git(path, &["pull", "--ff-only"], FETCH_TIMEOUT)
                    .await
                    .map_err(|e| anyhow::anyhow!("pulling {}: {e}", path.display()))?;
            }
            (path.join("install.sh"), None, None)
        }
        // No script on disk that would build anything but the revision it came with, so
        // the one for the release being installed is fetched. See this module's header.
        _ => {
            let release = report.release.as_ref().filter(|r| r.newer).ok_or_else(|| {
                anyhow::anyhow!(
                    "this build has no clone behind it, so a release is the only thing it can \
                     be updated to — and GitHub did not name one"
                )
            })?;
            let (path, scratch) = download_installer(&release.tag).await?;
            (path, Some(release.tag.clone()), Some(scratch))
        }
    };

    let mut tool = Tool::new("sh").arg(&script);
    if let Some(tag) = &tag {
        tool = tool.arg("--tag").arg(tag);
    }
    if let Some(dir) = &bin_dir {
        tool = tool.arg("--bin-dir").arg(dir);
    }

    let command = tool.display();
    // Printed rather than logged. The CLI runs ops with logging off (see the binary's
    // `logging` module), and a command that is about to compile and replace the program
    // you are running has to name itself on the way past.
    eprintln!("\n  {command}\n");

    let status = tool.interactive().await?;
    anyhow::ensure!(
        status.success(),
        "install.sh exited with {status}. Nothing was replaced unless it says otherwise; \
         re-run `{command}` to see the whole build.",
    );

    Ok(Applied {
        command,
        from: report.installed.clone(),
        to: installed_version(bin_dir.as_deref()).await,
    })
}

/// Fetches `install.sh` at a revision into a directory that dies with this call.
///
/// A temporary directory, not the checkout: the script must not find a workspace beside
/// itself, or it builds *that* — which is the revision being replaced.
async fn download_installer(rev: &str) -> anyhow::Result<(PathBuf, tempfile::TempDir)> {
    let url = version::install_script(rev)
        .ok_or_else(|| anyhow::anyhow!("no repository in this build's manifest"))?;
    let policy = HostPolicy {
        timeout: HTTP_TIMEOUT,
        ..Default::default()
    };
    let fetched = Fetcher::new(&policy)?
        .get(&url)
        .await
        .map_err(|refusal| anyhow::anyhow!("fetching {url}: {refusal}"))?;

    // Said out loud, at the moment it is true: this is a script from the network that is
    // about to run on this machine, and the address it came from is the only thing that
    // makes that reviewable.
    eprintln!("\n  installer  {url}");

    let scratch = tempfile::tempdir()?;
    let path = scratch.path().join("install.sh");
    tokio::fs::write(&path, &fetched.bytes).await?;
    Ok((path, scratch))
}

/// Where the two binaries should land: beside the one that is running.
///
/// `None` unless that directory is named `bin`, because `install.sh` installs through
/// cargo and cargo owns the layout under its root — the script refuses anything else, and
/// refusing here first keeps a `target/release` build from being told so at the end of a
/// ten-minute compile. With `None` the script picks `$CARGO_HOME/bin`, which is where it
/// would have put them originally.
fn install_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.ends_with("bin").then(|| dir.to_path_buf())
}

/// What the installed binary answers now.
///
/// Asked of the destination when one was named, and of `PATH` otherwise — which is the
/// binary the operator's next command will reach, and therefore the honest answer to
/// "what am I running now".
async fn installed_version(bin_dir: Option<&Path>) -> Option<String> {
    let program = match bin_dir {
        Some(dir) => dir.join("centinel"),
        None => PathBuf::from("centinel"),
    };
    let out = Tool::new(program)
        .arg("--version")
        .timeout(GIT_TIMEOUT)
        .output()
        .await
        .ok()?;
    // `centinel 0.6.0` — the version alone, so the report reads as a version and not as
    // a sentence.
    let line = out.first_line()?;
    line.split_whitespace().next_back().map(str::to_string)
}

fn provenance(origin: &Origin, build: &Build) -> Provenance {
    let (installed_from, detail) = match origin {
        Origin::Clone { .. } => (InstalledFrom::Clone, None),
        Origin::Cargo { .. } => (
            InstalledFrom::Git,
            Some("cargo's own checkout of the repository — pinned, and not a clone to pull".into()),
        ),
        Origin::Release { .. } => (
            InstalledFrom::Release,
            Some("a prebuilt binary off a release — no sources here, and none needed".into()),
        ),
        Origin::Unknown { reason, .. } => (InstalledFrom::Unknown, Some(reason.clone())),
    };
    Provenance {
        installed_from,
        path: origin.path().display().to_string(),
        commit: build.commit.clone(),
        detail,
    }
}

// -----------------------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------------------

/// The verdict first, then the two authorities in the order they were asked.
///
/// A version number on its own says nothing — 0.5.0 is either current or four months
/// stale depending on facts this report is holding — so the first line is the answer and
/// the sections underneath are the evidence for it.
impl Render for UpdateReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let head = match &self.applied {
            Some(a) => format!(
                "centinel {} → {}",
                a.from,
                a.to.as_deref().unwrap_or("installed")
            ),
            None => format!("centinel {}", self.installed),
        };
        p.title(&head, &self.provenance.line())?;

        if !self.newer {
            p.blank()?;
            if self.answered {
                p.marked(Mark::Ok, p.paint("up to date", Ink::Green))?;
            } else {
                // The one line this report must never get wrong. Nothing answered, so
                // "up to date" would be an assurance nobody gave.
                p.marked(
                    Mark::Warn,
                    p.paint(
                        "nothing answered — nobody said this is current",
                        Ink::Yellow,
                    ),
                )?;
            }
        }

        if let Some(repo) = &self.repo {
            p.section("repo")?;
            p.nest(|p| repo.render(p))?;
        }

        if let Some(release) = &self.release {
            p.section("github")?;
            p.nest(|p| release.render(p))?;
        }

        if !self.notes.is_empty() {
            p.blank()?;
            for note in &self.notes {
                let mark = note.mark.map(NoteMark::mark).unwrap_or(Mark::None);
                let text = format!("{}  {}", note.label, p.paint(&note.detail, Ink::Dim));
                p.marked(mark, text)?;
            }
        }

        // Under everything, because it is what to do about all of it. After an install it
        // is what just ran, and repeating it as an instruction would read as a failure.
        if let Some(fix) = &self.fix
            && self.newer
            && self.applied.is_none()
        {
            p.blank()?;
            p.line(p.paint(fix, Ink::Cyan))?;
        }
        Ok(())
    }
}

impl Provenance {
    /// The dim aside beside the version: which authority answers for this build.
    fn line(&self) -> String {
        match self.installed_from {
            InstalledFrom::Clone => format!("clone · {}", self.path),
            InstalledFrom::Git => "installed from git".to_string(),
            // No path on purpose: the stamp is the runner's directory, which names
            // nothing on this machine and reads as a place to go looking.
            InstalledFrom::Release => "release binary".to_string(),
            InstalledFrom::Unknown => format!("unknown · {}", self.path),
        }
    }
}

impl Render for RepoCheck {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let at = format!(
            "{} {}",
            self.branch.as_deref().unwrap_or("detached"),
            render::short_sha(&self.head),
        );
        p.line(p.paint(&at, Ink::Dim))?;

        // `None` prints nothing on purpose: it means the question did not resolve, which
        // the note carrying *why* has already said. Printing it as `0 commits` would turn
        // an unanswered question into a reassuring answer.
        if let Some(n) = self.since_build
            && n > 0
        {
            p.marked(
                Mark::Warn,
                format!(
                    "{} in this checkout that this binary was not built from",
                    render::plural(n, "commit", "commits")
                ),
            )?;
        }
        if let Some(n) = self.behind_origin
            && n > 0
        {
            p.marked(
                Mark::Warn,
                format!("{} behind origin", render::plural(n, "commit", "commits")),
            )?;
        }
        if self.pending() == 0 && self.fetched {
            p.marked(
                Mark::Ok,
                p.paint("nothing to pull, nothing to rebuild", Ink::Dim),
            )?;
        }
        if self.dirty {
            // Not a fault. It is said because it is the one thing that stops a pull.
            p.marked(
                Mark::None,
                p.paint("uncommitted changes in the working tree", Ink::Dim),
            )?;
        }
        Ok(())
    }
}

impl Render for ReleaseCheck {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let published = match &self.published {
            Some(ts) => format!("released {}", render::short_time(ts)),
            None => String::new(),
        };
        let mark = if self.newer { Mark::Warn } else { Mark::Ok };
        p.marked(
            mark,
            format!("{}  {}", self.tag, p.paint(&published, Ink::Dim)),
        )?;
        p.nest(|p| p.line(p.paint(&self.url, Ink::Dim)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Not named `render`: that is the module the renderers above read their helpers
    /// from, and a function of the same name in this scope is a trap for the next test.
    fn painted(report: &UpdateReport) -> String {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    fn repo(since_build: Option<usize>, behind: Option<usize>, dirty: bool) -> RepoCheck {
        RepoCheck {
            path: "/home/ben/centinel".to_string(),
            branch: Some("master".to_string()),
            head: "a56c1ea2a92c75bb0d31b3bd3e5d028a812a8c62".to_string(),
            since_build,
            behind_origin: behind,
            dirty,
            fetched: true,
        }
    }

    fn release(tag: &str, newer: bool) -> ReleaseCheck {
        ReleaseCheck {
            tag: tag.to_string(),
            version: version::release_version(tag).map(str::to_string),
            published: Some("2026-08-09T12:00:00Z".to_string()),
            url: format!("https://github.com/bennyhodl/centinel/releases/tag/{tag}"),
            newer,
        }
    }

    fn report(repo: Option<RepoCheck>, release: Option<ReleaseCheck>) -> UpdateReport {
        let origin = Origin::Clone {
            path: PathBuf::from("/home/ben/centinel"),
        };
        let build = Build {
            version: "0.5.0",
            src: PathBuf::from("/home/ben/centinel"),
            commit: Some("3f2b19c0000000000000000000000000000000".to_string()),
            ci: false,
        };
        let newer = repo.as_ref().is_some_and(|r| r.pending() > 0)
            || release.as_ref().is_some_and(|r| r.newer);
        UpdateReport {
            installed: "0.5.0".to_string(),
            provenance: provenance(&origin, &build),
            fix: fix_command(&origin, repo.as_ref(), release.as_ref()),
            answered: repo.is_some() || release.is_some(),
            repo,
            release,
            notes: Vec::new(),
            newer,
            applied: None,
        }
    }

    /// The failure this report exists to avoid. Nothing answered — no clone, GitHub
    /// unreachable — and the version alone is evidence of nothing. The note that says why
    /// has to reach the page as well, or the verdict is unactionable.
    #[test]
    fn nothing_answered_never_reads_as_up_to_date() {
        let mut r = report(None, None);
        r.notes
            .push(Note::marked("github", "connection refused", NoteMark::Warn));
        let out = painted(&r);
        assert!(!r.answered);
        assert!(!out.contains("up to date"), "{out}");
        assert!(out.contains("nothing answered"), "{out}");
        assert!(out.contains("github"), "{out}");
        assert!(out.contains("connection refused"), "{out}");
    }

    /// The two ranges are disjoint, so what a rebuild would pick up is their sum. Counting
    /// only one of them tells a contributor with local commits that they are current.
    #[test]
    fn pending_counts_both_what_is_local_and_what_is_upstream() {
        assert_eq!(repo(Some(5), Some(2), false).pending(), 7);
        assert_eq!(repo(Some(0), Some(0), false).pending(), 0);
        // An unrelated build commit is not zero commits — it is no answer.
        assert_eq!(repo(None, Some(3), false).pending(), 3);
        assert!(!repo(Some(4), Some(0), false).needs_pull());
        assert!(repo(Some(0), Some(1), false).needs_pull());
    }

    #[test]
    fn a_current_clone_says_so_and_offers_nothing_to_type() {
        let out = painted(&report(
            Some(repo(Some(0), Some(0), false)),
            Some(release("v0.5.0", false)),
        ));
        assert!(out.contains("up to date"), "{out}");
        assert!(!out.contains("install.sh"), "{out}");
    }

    /// The clone is ahead of the last release, which is the ordinary state of a
    /// contributor's checkout: both facts are true and both are reported.
    #[test]
    fn commits_and_a_release_are_reported_side_by_side() {
        let out = painted(&report(
            Some(repo(Some(7), Some(2), false)),
            Some(release("v0.6.0", true)),
        ));
        assert!(out.contains("7 commits"), "{out}");
        assert!(out.contains("2 commits behind origin"), "{out}");
        assert!(out.contains("v0.6.0"), "{out}");
        assert!(!out.contains("up to date"), "{out}");
    }

    /// A clone updates through the clone, and the pull is named only when there is
    /// something to pull.
    #[test]
    fn the_fix_for_a_clone_is_the_clone() {
        let origin = Origin::Clone {
            path: PathBuf::from("/home/ben/centinel"),
        };
        let behind = fix_command(&origin, Some(&repo(Some(0), Some(2), false)), None).unwrap();
        assert_eq!(behind, "cd /home/ben/centinel && git pull && ./install.sh");

        let local_only = fix_command(&origin, Some(&repo(Some(3), Some(0), false)), None).unwrap();
        assert_eq!(local_only, "cd /home/ben/centinel && ./install.sh");
    }

    /// Without a clone the pipe is the fix, pinned to the release it installs rather than
    /// to whatever `master` holds when somebody pastes it.
    #[test]
    fn the_fix_without_a_clone_is_the_pipe_at_the_tag() {
        let origin = Origin::Unknown {
            path: PathBuf::from("/nowhere"),
            reason: "gone".to_string(),
        };
        let fix = fix_command(&origin, None, Some(&release("v0.6.0", true))).unwrap();
        assert!(
            fix.contains("/bennyhodl/centinel/v0.6.0/install.sh"),
            "{fix}"
        );
        assert!(fix.ends_with("--tag v0.6.0"), "{fix}");

        // Nothing newer: the line still has to be one somebody can paste.
        let fix = fix_command(&origin, None, Some(&release("v0.5.0", false))).unwrap();
        assert!(fix.contains("/master/install.sh"), "{fix}");
    }

    /// A downloaded release binary stamps the runner's directory, which exists on no
    /// machine it lands on. The report names what the binary *is* rather than pointing
    /// at `/home/runner` and calling the sources lost — and its update is the pipe at
    /// the tag, like every install with no clone behind it.
    #[test]
    fn a_release_binary_names_itself() {
        let runner = PathBuf::from("/home/runner/work/centinel/centinel");
        let origin = Origin::Release {
            path: runner.clone(),
        };
        let build = Build {
            version: "0.7.0",
            src: runner,
            commit: Some("3f2b19c0000000000000000000000000000000".to_string()),
            ci: true,
        };
        let p = provenance(&origin, &build);
        assert!(matches!(p.installed_from, InstalledFrom::Release));
        assert_eq!(p.line(), "release binary");

        let fix = fix_command(&origin, None, Some(&release("v0.8.0", true))).unwrap();
        assert!(
            fix.contains("/bennyhodl/centinel/v0.8.0/install.sh"),
            "{fix}"
        );
        assert!(fix.ends_with("--tag v0.8.0"), "{fix}");
    }

    /// An install would replace the two binaries, so the destination must be a directory
    /// cargo will accept — `install.sh` refuses anything not named `bin`, and learning
    /// that after a ten-minute compile is the whole point of checking here.
    #[test]
    fn a_destination_is_only_offered_when_cargo_would_take_it() {
        // `install_dir` reads this process's own executable, which under `cargo test`
        // lives in `target/debug/deps` — so the answer here is None, and that is the
        // behaviour being pinned: a build tree is never named as an install root.
        assert_eq!(install_dir(), None);
    }
}
