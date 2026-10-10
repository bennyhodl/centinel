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
//!
//! ## `--server`: this page, another machine's corpus
//!
//! `centinel web --server https://box.tailnet.ts.net` serves the page embedded in this
//! binary on loopback and forwards everything else to that server, so the page keeps
//! calling its own origin and the remote needs no CORS. No local store is opened. Writes
//! pass the local same-origin check here and are sent on with the remote's own origin,
//! which the remote accepts when it is the tailnet address it published (or loopback,
//! over an SSH tunnel) — the same writes a browser opened on that address could make.

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
        tracing::info!(url = %format!("http://{bind}"), "already serving; opening the browser");
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
/// embedded at compile time. The Start build output streams to the terminal.
///
/// Only a source build can do this. The checkout path is the one Cargo saw at compile
/// time, so a release download — built on another machine — finds no `web/` there and
/// is told so before anything else happens. The embedded page is never modified: a
/// rebuilt bundle lives in `web-dist/` on disk and is read into memory for this
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
    tracing::info!(root = %root.display(), "rebuilding the web workspace");
    let status = std::process::Command::new("npm")
        .args(["run", "build"])
        .current_dir(&root)
        .env("CENTINEL_VERSION", version)
        .status()
        .with_context(|| "could not start `npm`; install Node.js 22.12 or newer")?;
    if !status.success() {
        bail!("`npm run build` failed in {}", root.display());
    }
    let dist = root.join("web-dist");
    let files = crate::bundle::files(&dist)
        .with_context(|| format!("reading {}", dist.display()))?
        .into_iter()
        .map(|(route, mime, path)| {
            std::fs::read(&path)
                .map(|bytes| (route, mime, bytes))
                .with_context(|| format!("reading {}", path.display()))
        })
        .collect::<Result<Vec<_>>>()?;
    let page = files
        .iter()
        .find(|file| file.0 == "/web")
        .with_context(|| format!("{} has no index.html", dist.display()))?;
    let stamped = super::http::stamped_version(std::str::from_utf8(&page.2).unwrap_or(""));
    match stamped.as_deref() {
        Some(stamped) if stamped == version => {}
        Some(stamped) => bail!(
            "the rebuilt page is stamped v{stamped} but this centinel is v{version}; \
             vite.config.ts and Cargo.toml disagree"
        ),
        None => bail!("the rebuilt page carries no centinel-version stamp"),
    }
    let bytes: usize = files.iter().map(|file| file.2.len()).sum();
    super::http::serve_bundle_from(files);
    tracing::info!(version, bytes, from = %dist.display(), "web workspace rebuilt");
    Ok(())
}

/// The server `centinel web --server` forwards to.
struct Remote {
    /// `https://box.tailnet.ts.net`, with no path.
    origin: String,
    client: reqwest::Client,
}

/// Serves the workspace page on `bind` against the centinel at `server`, and opens it.
pub async fn open_remote(server: &str, bind: &str) -> Result<()> {
    check_bundle()?;
    let url = reqwest::Url::parse(server).with_context(|| {
        format!("`{server}` is not a URL; give one like https://box.tailnet.ts.net")
    })?;
    let origin = url.origin().ascii_serialization();
    // No overall timeout: `/workspace/jobs/events` stays open for as long as the page does.
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()?;

    // The page is this binary's; the API is the server's. They have to be one release.
    let system: serde_json::Value = client
        .get(format!("{origin}/workspace/system"))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .with_context(|| format!("could not reach a centinel at {origin}"))?
        .json()
        .await
        .with_context(|| format!("{origin} is not a centinel server"))?;
    if system["product"] != "centinel" || system["api_version"] != 1 {
        bail!("{origin} is not a compatible centinel server");
    }
    let binary = env!("CARGO_PKG_VERSION");
    if system["version"] != binary {
        bail!(
            "{origin} runs centinel v{}, and this page is v{binary}; run the same release on \
             both machines",
            system["version"].as_str().unwrap_or("?")
        );
    }

    let listener = super::http::listen(bind)
        .await
        .context("pass --bind with a free port, e.g. --bind 127.0.0.1:8788")?;
    let local = listener.local_addr()?;
    let app = super::http::page_routes()
        .fallback(forward)
        .with_state(Arc::new(Remote {
            origin: origin.clone(),
            client,
        }));
    tracing::info!(server = %origin, store = system["store_root"].as_str().unwrap_or("?"), "connected");
    tracing::info!(url = %format!("http://{local}/web"), "serving the workspace page");
    spawn_browser(format!("http://{local}/web"));
    axum::serve(listener, app).await?;
    Ok(())
}

/// Headers that describe one hop and are not forwarded either way.
const HOP_BY_HOP: [&str; 9] = [
    "connection",
    "keep-alive",
    "proxy-connection",
    "transfer-encoding",
    "te",
    "trailer",
    "upgrade",
    "host",
    "content-length",
];

/// Everything that is not the page goes to the remote: the API, `/mcp`, `/health`.
async fn forward(
    axum::extract::State(remote): axum::extract::State<Arc<Remote>>,
    req: axum::extract::Request,
) -> axum::response::Response {
    use axum::http::header;
    use axum::response::IntoResponse;

    let (parts, body) = req.into_parts();
    let path = parts.uri.path_and_query().map_or("/", |p| p.as_str());
    let mut out = remote
        .client
        .request(parts.method.clone(), format!("{}{path}", remote.origin));
    for (name, value) in &parts.headers {
        // The client negotiates its own encoding and hands back plain bytes.
        let skip = HOP_BY_HOP.contains(&name.as_str())
            || name == header::ORIGIN
            || name == header::ACCEPT_ENCODING;
        if !skip {
            out = out.header(name, value);
        }
    }
    // A write the page made from this origin is a write from the remote's own.
    if super::http::same_origin(&parts.headers) {
        out = out.header(header::ORIGIN, &remote.origin);
    }
    // Buffered: what the page sends is a question or a review, never a stream.
    let body = match axum::body::to_bytes(body, 16 << 20).await {
        Ok(body) => body,
        Err(_) => return axum::http::StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    if !body.is_empty() {
        out = out.body(body);
    }

    match out.send().await {
        Ok(resp) => {
            let mut response = axum::http::Response::builder().status(resp.status());
            for (name, value) in resp.headers() {
                if !HOP_BY_HOP.contains(&name.as_str()) && name != header::CONTENT_ENCODING {
                    response = response.header(name, value);
                }
            }
            response
                .body(axum::body::Body::from_stream(resp.bytes_stream()))
                .unwrap_or_else(|_| axum::http::StatusCode::BAD_GATEWAY.into_response())
        }
        Err(error) => {
            tracing::warn!(%error, server = %remote.origin, "forwarding failed");
            (
                axum::http::StatusCode::BAD_GATEWAY,
                axum::Json(serde_json::json!({
                    "error": format!("could not reach the centinel at {}: {error}", remote.origin)
                })),
            )
                .into_response()
        }
    }
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
            Err(e) => tracing::warn!(error = %e, "could not open a browser; visit {url}"),
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
        Err(error) => tracing::warn!(%error, "could not open a browser; visit {url}"),
    }
}
