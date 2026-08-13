//! What this build came from.
//!
//! Two facts are stamped into the binary: the directory these sources sat in, and the
//! commit they were at. `centinel update` reads them back to answer *where would a newer
//! Centinel come from* — the same question `install.sh` answers at install time, and the
//! same way round: a clone updates the clone, anything else updates from the repository.
//!
//! **Stamped rather than looked for.** The alternative is to hunt for a checkout beside
//! the working directory, which finds whatever clone you happen to be standing in — not
//! the one that produced the binary you are running. A machine with two clones, or a
//! clone that was moved after installing, would then be told to rebuild something it does
//! not run.
//!
//! Neither stamp is required. Git may be absent, the sources may be a tarball, and both
//! cases are ordinary — `update` falls back to the repository, which is what it would do
//! for a `cargo install --git` build anyway. A build script that failed here would break
//! the build over a question that has a perfectly good "don't know".

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // `CARGO_MANIFEST_DIR` is `<sources>/crates/centinel-core`, wherever the sources are:
    // a person's clone, or cargo's own checkout under `$CARGO_HOME/git/checkouts`.
    // Telling those apart is `version::origin`'s job and needs the path either way.
    let manifest = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("cargo always sets CARGO_MANIFEST_DIR"),
    );
    let src = manifest
        .parent()
        .and_then(Path::parent)
        .unwrap_or(&manifest)
        .to_path_buf();

    println!("cargo::rustc-env=CENTINEL_BUILD_SRC={}", src.display());
    println!(
        "cargo::rustc-env=CENTINEL_BUILD_COMMIT={}",
        git(&src, &["rev-parse", "HEAD"]).unwrap_or_default()
    );

    // Whether a GitHub runner built this. A binary built there leaves with the runner's
    // directory stamped above, and that directory exists on no machine the binary lands
    // on — so without this third stamp, every downloaded release asset would read as
    // "sources lost" for the whole of its life. `version::origin` uses it only once the
    // source directory stops resolving; a checkout on the runner itself still answers as
    // the checkout it is.
    println!(
        "cargo::rustc-env=CENTINEL_BUILD_CI={}",
        std::env::var("GITHUB_ACTIONS").map_or("", |v| if v == "true" { "1" } else { "" })
    );
    println!("cargo::rerun-if-env-changed=GITHUB_ACTIONS");

    // Emitting any `rerun-if-changed` replaces cargo's default — rerun when a file in
    // this package changes — and that is what is wanted here: nothing above reads the
    // sources. What the stamps depend on is the git ref, so the git ref is what is
    // watched. Without this, the first build's commit would be stamped into every later
    // one, and `update` would report a binary as older than it is forever.
    println!("cargo::rerun-if-changed=build.rs");
    for path in ref_files(&src) {
        println!("cargo::rerun-if-changed={}", path.display());
    }
}

/// The files whose contents decide the commit above.
///
/// Only ones that exist: a `rerun-if-changed` naming a missing path makes cargo rerun the
/// script on every build, which is the one outcome worse than a stale stamp.
fn ref_files(src: &Path) -> Vec<PathBuf> {
    let Some(git_dir) = git(src, &["rev-parse", "--absolute-git-dir"]) else {
        return Vec::new();
    };
    let git_dir = PathBuf::from(git_dir);

    // `HEAD` covers a branch switch and holds the sha outright when it is detached; the
    // branch it names covers a commit, which moves the tip without touching `HEAD`;
    // `packed-refs` is where that tip lives once `git gc` has been through.
    let mut watched = vec![git_dir.join("HEAD"), git_dir.join("packed-refs")];
    if let Some(head_ref) = git(src, &["symbolic-ref", "--quiet", "HEAD"]) {
        watched.push(git_dir.join(head_ref));
    }
    watched.retain(|p| p.exists());
    watched
}

/// One git question, answered as trimmed stdout. `None` for every way it can fail —
/// git missing, not a checkout, a command that refused.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!text.is_empty()).then_some(text)
}
