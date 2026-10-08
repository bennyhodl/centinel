//! `centinel web` — the corpus workspace in a browser.
//!
//! The HTTP API serves corpus reads and the workspace's guarded classifier writes. This
//! module adds the browser lifecycle: start that API when needed, or reuse the compatible
//! server that already owns the same corpus.
//!
//! ## Why a separate command rather than a flag on `serve`
//!
//! `serve` arms the scheduler; `web` must not. A person opening the workspace to review
//! documents did not ask for a collection run, and a command named for *looking* that
//! *collects* would be the second meaning hiding in one verb. So this is its own
//! subcommand with the scheduler always off. Its writes are limited to classifier truth
//! and reversible usage decisions; it never starts acquisition.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use centinel_core::op::Ctx;

/// Serves the workspace and opens it in the operator's browser.
///
/// The scheduler never starts here: `--no-schedule` is not offered because there is
/// nothing it could switch back on. An occupied port is reused only when its product,
/// API version, binary version, and canonical store root all match. Every other occupant
/// is refused before a browser opens.
pub async fn open(ctx: Arc<Ctx>, bind: &str, rebuilt: bool) -> Result<()> {
    check_bundle()?;
    let url = format!("http://{bind}/web");
    let system_url = format!("http://{bind}/workspace/system");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(750))
        .build()?;
    if let Ok(response) = client.get(&system_url).send().await {
        let status = response.status();
        let body: serde_json::Value = response.json().await.with_context(|| {
            format!(
                "{bind} is occupied by a server that is not a compatible Centinel server ({status})"
            )
        })?;
        let expected_root = std::fs::canonicalize(ctx.store.root())
            .unwrap_or_else(|_| ctx.store.root().to_path_buf());
        let compatible = body["product"] == "centinel"
            && body["api_version"] == 1
            && body["version"] == env!("CARGO_PKG_VERSION")
            && body["store_root"].as_str() == expected_root.to_str();
        if !status.is_success() || !compatible {
            bail!("{bind} is occupied by a different server, Centinel version, or corpus root");
        }
        // Same version is not the same build. A server started before `cargo build`
        // reports this version and this root, and would serve the old code and the old
        // page to a person who just rebuilt to see the new ones.
        if rebuilt {
            bail!(
                "{bind} is held by a running centinel, which would serve its own page \
                 instead of the one just rebuilt; stop it and run this command again"
            );
        }
        if body["build_id"] != super::http::build_id() {
            bail!(
                "{bind} is held by an older build of centinel v{}; stop that `centinel web` \
                 (or `centinel serve`) and run this command again",
                env!("CARGO_PKG_VERSION")
            );
        }
        eprintln!("centinel web already serving on http://{bind}; opening the browser");
        open_browser(url).await;
        return Ok(());
    }
    spawn_browser(url);
    super::http::serve(ctx, bind).await
}

/// The workspace page must carry this binary's version. It is checked before a port is
/// probed or a browser is opened, because neither can be undone by a person who then
/// sees a page from another release. After `--rebuild` this checks the rebuilt page.
///
/// A source build cannot reach here mismatched: `build.rs` refuses the bundle first. A
/// release download has no `web/` tree to rebuild from, so all this can do is say so.
pub fn check_bundle() -> Result<()> {
    let binary = env!("CARGO_PKG_VERSION");
    match super::http::web_version() {
        Some(web) if web == binary => {
            tracing::info!(version = binary, "web workspace bundle matches the binary");
            Ok(())
        }
        Some(web) => bail!(
            "the embedded web workspace is v{web} but this centinel is v{binary}; \
             rebuild from source with `cargo build`, or download the v{binary} release"
        ),
        None => bail!(
            "the embedded web workspace carries no version stamp; \
             rebuild from source with `cargo build`, or download the v{binary} release"
        ),
    }
}

/// `--rebuild`: rebuilds the web workspace from the source checkout this binary was
/// compiled in, checks its version stamp, and serves that bundle instead of the one
/// embedded at compile time. The Vite output streams to the terminal.
///
/// Only a source build can do this. The checkout path is the one Cargo saw at compile
/// time, so a release download — built on another machine — finds no `web/` there and
/// is told so before anything else happens. The embedded page is never modified: a
/// rebuilt bundle lives in `web-dist/` on disk and is served from memory for this
/// process only. The next `cargo build` embeds it for good.
pub fn rebuild_bundle() -> Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let version = env!("CARGO_PKG_VERSION");
    if !root.join("web").is_dir() || !root.join("package.json").is_file() {
        bail!(
            "cannot rebuild the web workspace: no source checkout at {}. Only a centinel \
             built from source can rebuild its page; a release download already carries \
             the v{version} page",
            root.display()
        );
    }
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    eprintln!("rebuilding the web workspace in {}", root.display());
    let status = std::process::Command::new("npm")
        .args(["run", "build"])
        .current_dir(&root)
        .env("CENTINEL_VERSION", version)
        .status()
        .with_context(|| "could not start `npm`; install Node.js 20.19 or newer")?;
    if !status.success() {
        bail!("`npm run build` failed in {}", root.display());
    }
    let dist = root.join("web-dist");
    let page = std::fs::read(dist.join("index.html"))
        .with_context(|| format!("reading {}", dist.join("index.html").display()))?;
    let stamped = super::http::stamped_version(std::str::from_utf8(&page).unwrap_or(""));
    match stamped.as_deref() {
        Some(stamped) if stamped == version => {}
        Some(stamped) => bail!(
            "the rebuilt page is stamped v{stamped} but this centinel is v{version}; \
             vite.config.ts and Cargo.toml disagree"
        ),
        None => bail!("the rebuilt page carries no centinel-version stamp"),
    }
    let bytes = page.len();
    super::http::serve_page_from(page);
    eprintln!(
        "web workspace v{version}: {bytes} bytes rebuilt from {}",
        dist.display()
    );
    Ok(())
}

fn spawn_browser(url: String) {
    let opener = centinel_core::ops::system_opener().to_string();
    tokio::spawn(async move {
        // The listener is usually up in milliseconds; the delay is for the gap between
        // spawning this and `axum::serve` accepting, so a slow first bind does not 404.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        match centinel_core::tool::Tool::new(&opener)
            .arg(&url)
            .output()
            .await
        {
            Ok(_) => {}
            Err(e) => eprintln!("could not open a browser ({e}); visit {url}"),
        }
    });
}

async fn open_browser(url: String) {
    let opener = centinel_core::ops::system_opener().to_string();
    match centinel_core::tool::Tool::new(&opener)
        .arg(&url)
        .output()
        .await
    {
        Ok(_) => {}
        Err(error) => eprintln!("could not open a browser ({error}); visit {url}"),
    }
}
