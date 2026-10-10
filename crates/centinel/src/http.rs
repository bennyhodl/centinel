//! HTTP, derived from the op registry.
//!
//! Like [`crate::mcp`], this module names no individual op. Routes are the registry.
//!
//! | Route | Purpose |
//! |---|---|
//! | `GET /health` | liveness |
//! | `GET /ops` | the registry, with JSON Schema per op |
//! | `POST /ops/{name}` | invoke, JSON in / JSON out |
//! | `POST /ops/{name}/stream` | invoke with SSE progress, then the result |
//! | `POST /mcp` | MCP JSON-RPC over HTTP |
//! | `GET /workspace/jobs` | what this process is working on, and what it just finished |
//! | `GET /workspace/jobs/events` | the same, then every job event as it happens (SSE) |
//!
//! ## Long-running operations
//!
//! Ticket #9 called this the hardest case, and the three surfaces genuinely differ:
//! the CLI prints progress to stderr, MCP waits and returns once, and HTTP offers
//! `/stream`. What does *not* differ is the op — it emits [`Progress`] events and never
//! learns who invoked it.
//!
//! `/stream` holds the connection open rather than returning a job id. That is honest
//! for the spine and wrong for a multi-hour crawl; a durable job store belongs with
//! scheduling (ticket #7), which owns resumability.
//!
//! What a watcher wants from a multi-hour crawl is not its result but its progress, and
//! that is `/workspace/jobs/events`: every long-running op this process runs — a scheduled
//! `run`, a classifier run started from the page — is a job in [`centinel_core::jobs`],
//! and the stream opens on a snapshot of all of them before it follows. SSE rather than a
//! WebSocket because nothing travels the other way: it is plain HTTP through any proxy
//! in front of this one, and `EventSource` reconnects on its own.
//!
//! ## Access control
//!
//! There is none, which is why the default bind is loopback. SPEC §8 lists server
//! access control as unspecified, and inventing a scheme here would foreclose that
//! decision. Binding to a non-loopback address logs a warning rather than silently
//! exposing the store.

use std::convert::Infallible;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use centinel_core::jobs::JobState;
use centinel_core::op::{self, Cancel, Ctx, Progress};
use centinel_core::workspace::{
    DocumentQuery, ReadQuery, RestoreRequest, Review, ReviewQuery, RunDetailQuery, RunQuery,
    RunRequest, Workspace,
};
use futures::stream::Stream;
use serde_json::{Value, json};

include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

/// Identifies the binary this process runs: the executable's size and modification
/// time. Two `centinel web` invocations from the same build agree on it; a rebuild
/// changes it, so a server left running from before the build is recognised as stale
/// even though its version string still matches.
pub fn build_id() -> &'static str {
    static ID: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        let meta = std::env::current_exe().and_then(std::fs::metadata);
        match meta {
            Ok(meta) => {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                format!("{}-{modified}", meta.len())
            }
            Err(_) => "unknown".to_string(),
        }
    });
    &ID
}

/// A bundle rebuilt by `centinel web --rebuild`, served in place of the embedded one
/// for the life of this process, as `(route, content type, bytes)`. Set once, before
/// the server starts.
static REBUILT: std::sync::OnceLock<Vec<(String, &'static str, Vec<u8>)>> =
    std::sync::OnceLock::new();

/// Serves `files` instead of the embedded bundle. A second call is ignored.
pub fn serve_bundle_from(files: Vec<(String, &'static str, Vec<u8>)>) {
    let _ = REBUILT.set(files);
}

/// The content type and bytes this process serves at `route`: from the rebuilt bundle
/// when there is one, else from the copy embedded at compile time.
fn asset(route: &str) -> Option<(&'static str, &'static [u8])> {
    if let Some(files) = REBUILT.get() {
        return files
            .iter()
            .find(|file| file.0 == route)
            .map(|file| (file.1, file.2.as_slice()));
    }
    WEB_ASSETS
        .iter()
        .find(|asset| asset.0 == route)
        .map(|asset| (asset.1, asset.2))
}

/// The version stamped into the served workspace page by the Start root route.
///
/// The build script refuses a bundle whose stamp differs from the crate, so on a binary
/// built from source this always equals `CARGO_PKG_VERSION`. A release download cannot
/// rebuild the bundle, so [`crate::web::check_bundle`] only checks it, early.
pub fn web_version() -> Option<String> {
    stamped_version(std::str::from_utf8(asset("/web")?.1).ok()?)
}

/// The `centinel-version` meta tag in a page, as the Start root route writes it.
pub fn stamped_version(html: &str) -> Option<String> {
    let start = html.find("name=\"centinel-version\"")?;
    let rest = &html[start..];
    let content = rest.find("content=\"")? + "content=\"".len();
    let rest = &rest[content..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Serves until the process is asked to stop.
///
/// [`serve_until`] with a signal that never arrives — for a caller with nothing to wind
/// down afterwards.
pub async fn serve(ctx: Arc<Ctx>, bind: &str) -> Result<()> {
    serve_until(ctx, bind, std::future::pending()).await
}

/// Serves until `shutdown` resolves, then stops accepting and returns.
///
/// The signal is a parameter rather than a `SIGTERM` handler installed here, because what
/// happens *after* the socket closes is the caller's business: `main` uses the return to
/// cancel an in-flight scheduled run at its next item boundary and let the scheduler write
/// its `interrupted` record. Without a graceful return the process is simply killed, the
/// journal keeps no record of the run that was in flight, and the only evidence is a stale
/// `run.lock` for the next startup to reclaim.
pub async fn serve_until(
    ctx: Arc<Ctx>,
    bind: &str,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let store = ctx.store.root().display().to_string();
    let app = router(ctx);

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    let addr = listener.local_addr()?;

    if !addr.ip().is_loopback() {
        tracing::warn!(
            %addr,
            "reachable off-host with no authentication — access control is unspecified (SPEC §8)"
        );
    }

    // The first lines of the log, and what they are for: an operator can tell, before
    // sending a single request, which store this is, what it answers, and whether the
    // log they are watching is on at all.
    tracing::info!(url = %format!("http://{addr}"), store = %store, "listening");
    tracing::info!(
        ops = op::remote_ops().len(),
        tools = op::mcp_tools().len(),
        web = web_version().as_deref().unwrap_or("absent"),
        "serving /ops, /ops/{{name}}[/stream], /mcp and /web"
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;

    tracing::info!("http server stopped");
    Ok(())
}

fn router(ctx: Arc<Ctx>) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/web", get(web_ui))
        .route("/web/", get(web_ui))
        .route("/web/{*path}", get(web_ui))
        .route("/web/assets/{*path}", get(web_asset))
        .route("/workspace/documents", get(workspace_documents))
        .route("/workspace/system", get(workspace_system))
        .route("/workspace/document", get(workspace_document))
        .route("/workspace/original", get(workspace_original))
        .route(
            "/workspace/questions",
            get(workspace_questions).put(workspace_save_questions),
        )
        .route("/workspace/presets", get(workspace_presets))
        .route("/workspace/runs", get(workspace_runs).post(workspace_run))
        .route("/workspace/runs/{id}", get(workspace_run_detail))
        .route("/workspace/runs/{id}/commit", post(workspace_commit))
        .route("/workspace/restore", post(workspace_restore))
        .route("/workspace/review/queue", get(workspace_review_queue))
        .route("/workspace/review", post(workspace_review))
        .route("/workspace/evaluation", get(workspace_evaluation))
        .route("/workspace/jobs", get(workspace_jobs))
        .route("/workspace/jobs/events", get(workspace_job_events))
        .route("/ops", get(list_ops))
        .route("/ops/{name}", post(invoke))
        .route("/ops/{name}/stream", post(invoke_streaming))
        .route("/mcp", post(mcp_over_http))
        .layer(axum::middleware::from_fn(log_request))
        .with_state(ctx)
}

/// One line per request, once it is answered: what was asked, what came back, how long.
///
/// Every route, including the ones that return a stream — for those the line is written
/// when the headers go out, which is when the request was *answered* even if the body
/// runs for an hour. The query string is kept because on this API it is the question:
/// `/workspace/documents?source=tampa&tag=junk` is a search, and a line without it would
/// say only that someone searched. What an op was asked and how it went is the
/// invocation's own lines, under [`crate::logging::invoke`]; this is the transport's.
async fn log_request(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req.uri().query().map(str::to_string);
    let started = std::time::Instant::now();

    let response = next.run(req).await;

    let status = response.status().as_u16();
    let ms = started.elapsed().as_millis() as u64;
    let target = match &query {
        Some(q) => format!("{path}?{q}"),
        None => path,
    };
    // A 5xx is this process's fault and is worth seeing without raising the level; a
    // 4xx is the caller's, and the route that refused it has already said why.
    if status >= 500 {
        tracing::error!(status, ms, "{method} {target}");
    } else {
        tracing::info!(status, ms, "{method} {target}");
    }
    response
}

/// The bundled classifier workspace.
///
/// Embedded at build time: no asset directory to ship beside the binary and no build
/// step on the user's machine. Every path under `/web` that is not an asset gets the
/// shell, and the router takes it from there. It talks to the ops API on this origin,
/// so nothing here learns a route by name — the UI calls `/ops/read` and `/ops/search`
/// the same way the CLI does.
async fn web_ui() -> Response {
    asset_response("/web")
}

async fn web_asset(Path(path): Path<String>) -> Response {
    asset_response(&format!("/web/assets/{path}"))
}

fn asset_response(route: &str) -> Response {
    match asset(route) {
        Some((mime, bytes)) => ([(axum::http::header::CONTENT_TYPE, mime)], bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn workspace_documents(
    State(ctx): State<Arc<Ctx>>,
    Query(query): Query<DocumentQuery>,
) -> Response {
    workspace_response(Workspace::new(&ctx.store).documents(query))
}

async fn workspace_document(
    State(ctx): State<Arc<Ctx>>,
    Query(query): Query<ReadQuery>,
) -> Response {
    workspace_response(Workspace::new(&ctx.store).read(query).await)
}

#[derive(serde::Deserialize)]
struct OriginalQuery {
    /// A blob hash, full or short, as the reader holds it.
    blob: String,
    source: Option<String>,
    /// Ask the browser to save the file rather than show it.
    #[serde(default)]
    download: bool,
}

/// A document's original bytes, for the reader to show and to save.
///
/// Collected HTML is another site's page served from this origin, beside the workspace's
/// writes, so nothing served here may run: every response carries a sandbox policy and
/// `nosniff`. A PDF is the one exception, because the browser's own viewer will not open
/// a sandboxed document, and a PDF viewer runs no page script on this origin.
async fn workspace_original(
    State(ctx): State<Arc<Ctx>>,
    Query(query): Query<OriginalQuery>,
) -> Response {
    use axum::http::header;
    use centinel_core::content::ContentKind;

    let doc = match centinel_core::ops::original(&ctx, &query.blob, query.source.as_deref()).await {
        Ok(doc) => doc,
        Err(error) => return workspace_error(error),
    };
    let content_type = doc
        .media_type
        .clone()
        .or_else(|| {
            ContentKind::declared_type_for_path(std::path::Path::new(&doc.filename))
                .map(str::to_string)
        })
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let name: String = doc
        .filename
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .filter(|c| *c != '"' && *c != '\\')
        .collect();
    let disposition = format!(
        "{}; filename=\"{name}\"",
        if query.download {
            "attachment"
        } else {
            "inline"
        }
    );
    let mut response = (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, disposition),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        doc.bytes,
    )
        .into_response();
    if doc.kind != ContentKind::Pdf {
        response.headers_mut().insert(
            header::CONTENT_SECURITY_POLICY,
            axum::http::HeaderValue::from_static("sandbox"),
        );
    }
    response
}

/// The `snapshot` event: the jobs as they stand, or the one job asked for.
fn job_snapshot(all: Vec<JobState>, wanted: Option<&str>) -> Event {
    let jobs: Vec<JobState> = all
        .into_iter()
        .filter(|job| wanted.is_none_or(|id| id == job.id))
        .collect();
    Event::default()
        .event("snapshot")
        .json_data(json!({ "jobs": jobs }))
        .expect("a job snapshot always serializes")
}

async fn workspace_system(State(ctx): State<Arc<Ctx>>) -> Response {
    let root =
        std::fs::canonicalize(ctx.store.root()).unwrap_or_else(|_| ctx.store.root().to_path_buf());
    Json(json!({
        "product": "centinel",
        "api_version": 1,
        "version": env!("CARGO_PKG_VERSION"),
        "web_version": web_version(),
        "build_id": build_id(),
        "store_root": root,
        // Where an agent reaches MCP when this server sits behind another address. Unset,
        // the page offers the address it was loaded from.
        "public_url": std::env::var("CENTINEL_PUBLIC_URL").ok().filter(|url| !url.trim().is_empty()),
    }))
    .into_response()
}

async fn workspace_questions(State(ctx): State<Arc<Ctx>>) -> Response {
    match Workspace::new(&ctx.store).questions() {
        Ok(questions) => Json(json!({ "questions": questions })).into_response(),
        Err(error) => workspace_error(error),
    }
}

async fn workspace_review_queue(
    State(ctx): State<Arc<Ctx>>,
    Query(query): Query<ReviewQuery>,
) -> Response {
    workspace_response(Workspace::new(&ctx.store).review_queue(query))
}

/// A person's verdicts on one document. A write: it can restore or exclude the document
/// and it puts tags on it, so it wants the same origin every other write wants.
async fn workspace_review(
    State(ctx): State<Arc<Ctx>>,
    headers: HeaderMap,
    Json(review): Json<Review>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden_origin();
    }
    workspace_response(Workspace::new(&ctx.store).review(review))
}

async fn workspace_evaluation(State(ctx): State<Arc<Ctx>>) -> Response {
    workspace_response(Workspace::new(&ctx.store).evaluation())
}

/// The shipped question groups, for the Add menu. The saved set is seeded from these on
/// first use, so this is what a page offers back rather than what it starts from.
async fn workspace_presets() -> Response {
    Json(json!({ "presets": centinel_core::workspace::presets() })).into_response()
}

async fn workspace_save_questions(
    State(ctx): State<Arc<Ctx>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden_origin();
    }
    let questions =
        match serde_json::from_value(body.get("questions").cloned().unwrap_or(Value::Null)) {
            Ok(questions) => questions,
            Err(error) => return workspace_error(error.into()),
        };
    match Workspace::new(&ctx.store).save_questions(questions) {
        Ok(questions) => Json(json!({ "questions": questions })).into_response(),
        Err(error) => workspace_error(error),
    }
}

async fn workspace_runs(State(ctx): State<Arc<Ctx>>, Query(query): Query<RunQuery>) -> Response {
    workspace_response(Workspace::new(&ctx.store).runs(query))
}

async fn workspace_run_detail(
    State(ctx): State<Arc<Ctx>>,
    Path(id): Path<String>,
    Query(query): Query<RunDetailQuery>,
) -> Response {
    workspace_response(Workspace::new(&ctx.store).run_detail(&id, query))
}

async fn workspace_run(
    State(ctx): State<Arc<Ctx>>,
    headers: HeaderMap,
    Json(request): Json<RunRequest>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden_origin();
    }
    // Answer with the run as started, then score it on a task of its own, as a job under
    // the run's id. The browser follows it on the job stream and reads the answers from
    // the run detail; a request held open for a thousand documents sat behind proxies,
    // browser limits, and a person wondering whether anything was happening.
    let prepared = match Workspace::new(&ctx.store).prepare(request) {
        Ok(prepared) => prepared,
        Err(error) => return workspace_error(error),
    };
    let started = prepared.run().clone();
    let job = ctx.jobs.start_as(
        started.id.clone(),
        "classify",
        format!("{} documents · {}", started.document_count, started.model),
    );
    let worker = Arc::clone(&ctx);
    tokio::spawn(async move {
        let id = prepared.run().id.clone();
        let progress = job.watch(Progress::none());
        let result = Workspace::new(&worker.store)
            .execute_with(prepared, &progress, &Cancel::none())
            .await;
        match &result {
            Ok(run) => tracing::info!(
                run = %run.id,
                documents = run.results.len(),
                errors = run.errors,
                duration_ms = run.duration_ms.unwrap_or_default(),
                "classifier run finished"
            ),
            Err(error) => {
                tracing::warn!(run = %id, error = %format!("{error:#}"), "classifier run failed")
            }
        }
        job.finish(&result);
    });
    (StatusCode::ACCEPTED, Json(started)).into_response()
}

#[derive(serde::Deserialize)]
struct JobsQuery {
    /// One job's id. Absent is every job.
    job: Option<String>,
}

/// Every active job, then the recently finished, each with its latest log lines.
async fn workspace_jobs(State(ctx): State<Arc<Ctx>>) -> Response {
    Json(json!({ "jobs": ctx.jobs.snapshot() })).into_response()
}

/// A `snapshot` event with the jobs as they stand, then one `job` event per thing that
/// happens to them, for one job when `?job=` names it.
///
/// A subscriber that falls too far behind is sent a fresh snapshot rather than dropped:
/// the events it missed are already folded into the state it is sent, and each carries a
/// `seq` so the client skips any it then sees twice.
async fn workspace_job_events(
    State(ctx): State<Arc<Ctx>>,
    Query(query): Query<JobsQuery>,
) -> Response {
    use tokio::sync::broadcast::error::RecvError;

    let jobs = ctx.jobs.clone();
    let (first, mut rx) = jobs.subscribe();
    let wanted = query.job;
    let stream = async_stream::stream! {
        yield Ok(job_snapshot(first, wanted.as_deref()));
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if wanted.as_deref().is_none_or(|id| id == event.job) {
                        yield Ok(Event::default()
                            .event("job")
                            .json_data(&event)
                            .expect("a job event always serializes"));
                    }
                }
                Err(RecvError::Lagged(_)) => yield Ok(job_snapshot(jobs.snapshot(), wanted.as_deref())),
                Err(RecvError::Closed) => break,
            }
        }
    };

    let mut response =
        Sse::new(Box::pin(stream)
            as std::pin::Pin<
                Box<dyn Stream<Item = Result<Event, Infallible>> + Send>,
            >)
        .keep_alive(KeepAlive::default())
        .into_response();
    // A proxy that buffers responses would hold every event until the stream ends.
    response.headers_mut().insert(
        "x-accel-buffering",
        axum::http::HeaderValue::from_static("no"),
    );
    response
}

async fn workspace_commit(
    State(ctx): State<Arc<Ctx>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !same_origin(&headers) {
        return forbidden_origin();
    }
    workspace_response(Workspace::new(&ctx.store).commit(&id))
}

async fn workspace_restore(
    State(ctx): State<Arc<Ctx>>,
    headers: HeaderMap,
    Json(request): Json<RestoreRequest>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden_origin();
    }
    workspace_response(Workspace::new(&ctx.store).restore(request))
}

fn workspace_response<T: serde::Serialize>(result: anyhow::Result<T>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => workspace_error(error),
    }
}

fn workspace_error(error: anyhow::Error) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": format!("{error:#}") })),
    )
        .into_response()
}

/// Workspace writes can cause paid inference or change corpus usage. A browser must send
/// the same authority in `Origin` and `Host`, which blocks a page on another origin from
/// using the loopback server as its write target.
fn same_origin(headers: &HeaderMap) -> bool {
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok());
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok());
    let origin_uri = origin.and_then(|value| value.parse::<Uri>().ok());
    let origin_host = origin_uri
        .as_ref()
        .and_then(|uri| uri.authority().map(|a| a.as_str()));
    let loopback = host
        .and_then(|authority| authority.parse::<axum::http::uri::Authority>().ok())
        .is_some_and(|authority| {
            matches!(
                authority.host(),
                "localhost" | "127.0.0.1" | "[::1]" | "::1"
            )
        });
    if loopback
        && origin_uri.as_ref().and_then(Uri::scheme_str) == Some("http")
        && host.is_some()
        && host == origin_host
    {
        return true;
    }
    false
}

fn forbidden_origin() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({ "error": "workspace mutations require a same-origin Origin and Host" })),
    )
        .into_response()
}

/// The registry as JSON — the same information the CLI turns into help text and MCP
/// turns into a tool list.
async fn list_ops() -> Json<Value> {
    let ops: Vec<Value> = op::remote_ops()
        .into_iter()
        .map(|def| {
            json!({
                "name": def.name,
                "about": def.about,
                "long_running": def.long_running,
                "mcp": def.mcp,
                "schema": (def.schema)(),
            })
        })
        .collect();
    tracing::debug!(count = ops.len(), "listing ops");
    Json(json!({ "ops": ops }))
}

async fn invoke(
    State(ctx): State<Arc<Ctx>>,
    Path(name): Path<String>,
    body: Option<Json<Value>>,
) -> Response {
    let Some(def) = remote_op(&name) else {
        return not_found(&name);
    };
    // An absent body is an empty argument set, so zero-arg ops work with `curl -X POST`.
    let args = body.map(|Json(v)| v).unwrap_or_else(|| json!({}));

    // No sink: this route returns once and has nowhere to put progress, so `logging`
    // sends it to the log rather than dropping it. `/stream` below is the route that
    // does have somewhere better.
    match crate::logging::invoke("http", def, ctx, args, None).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => op_error(e),
    }
}

/// Streams progress as SSE, then a terminal `result` or `error` event.
async fn invoke_streaming(
    State(ctx): State<Arc<Ctx>>,
    Path(name): Path<String>,
    body: Option<Json<Value>>,
) -> Response {
    let Some(def) = remote_op(&name) else {
        return not_found(&name);
    };
    let args = body.map(|Json(v)| v).unwrap_or_else(|| json!({}));

    let (progress, mut rx) = Progress::channel();
    let handle = tokio::spawn(async move {
        crate::logging::invoke("http-stream", def, ctx, args, Some(progress)).await
    });

    let stream = async_stream::stream! {
        while let Some(ev) = rx.recv().await {
            yield Ok(Event::default()
                .event("progress")
                .json_data(&ev)
                .expect("ProgressEvent always serializes"));
        }

        let event = match handle.await {
            Ok(Ok(value)) => Event::default().event("result").json_data(&value),
            Ok(Err(e)) => Event::default()
                .event("error")
                .json_data(json!({ "error": format!("{e:#}") })),
            Err(join) => Event::default()
                .event("error")
                .json_data(json!({ "error": format!("op task panicked: {join}") })),
        };
        yield Ok(event.expect("terminal event always serializes"));
    };

    Sse::new(Box::pin(stream)
        as std::pin::Pin<
            Box<dyn Stream<Item = Result<Event, Infallible>> + Send>,
        >)
    .keep_alive(KeepAlive::default())
    .into_response()
}

/// MCP over HTTP, delegating to the same handler stdio uses.
async fn mcp_over_http(State(ctx): State<Arc<Ctx>>, Json(req): Json<Value>) -> Response {
    // The dispatch itself logs the method; this only records which transport it arrived on.
    tracing::debug!("mcp over http");
    match crate::mcp::handle(&ctx, req).await {
        Some(resp) => Json(resp).into_response(),
        // A notification: accepted, nothing to say.
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// Resolves an op, refusing everything beyond [`op::Reach::Public`].
///
/// Two kinds are refused here and they fail for different reasons. A `Host` op acts on
/// the machine it runs on — launching a GUI, running a configured command — and remotely
/// that is command execution against a server with no authentication. An `Operator` op
/// *causes collection*, and this server may report on the record but never grow it: the
/// authority to start a crawl comes from the operator's config file, not from a request.
///
/// Either way it is invisible here rather than merely refused.
fn remote_op(name: &str) -> Option<&'static op::OpDef> {
    op::find(name).filter(|d| d.reach.is_remote())
}

fn not_found(name: &str) -> Response {
    // Warn, not debug: over HTTP this is either a typo or a client built against a
    // different build, and both are worth seeing without raising the level.
    tracing::warn!(op = %name, "no such op, or host-local");
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": format!("unknown op `{name}`") })),
    )
        .into_response()
}

/// Op failures are 400, not 500.
///
/// Nearly every failure reachable here is a bad argument or an unreachable upstream —
/// caller-actionable. A genuine internal fault shows up as a panic, which axum already
/// turns into a 500.
fn op_error(e: anyhow::Error) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": format!("{e:#}") })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use centinel_core::store::Store;
    use tower::ServiceExt;

    async fn app() -> (tempfile::TempDir, Router) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        (dir, router(Arc::new(Ctx::new(store))))
    }

    async fn body_json(resp: Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn ops_endpoint_exposes_every_remote_op() {
        let (_d, app) = app().await;
        let resp = app
            .oneshot(Request::get("/ops").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        let ops = json["ops"].as_array().unwrap();
        assert_eq!(ops.len(), op::remote_ops().len());
        assert!(ops.iter().any(|o| o["name"] == "doctor"));
    }

    /// `Host` ops act on the machine they run on: `open` launches a configured command,
    /// `models` pulls gigabytes into a local cache. Over HTTP — which has no
    /// authentication — those are remote command execution and remote disk exhaustion.
    /// `Operator` ops *cause collection*, which over the same surface is an unbounded
    /// crawl against a city's web server, attributed to whoever runs this store.
    ///
    /// Written over the whole registry rather than named ops, so a future non-`Public`
    /// op is covered the day it is added rather than the day someone remembers to.
    #[tokio::test]
    async fn ops_beyond_public_reach_are_neither_listed_nor_invokable() {
        let (_d, app) = app().await;

        let local: Vec<&str> = op::all()
            .into_iter()
            .filter(|d| !d.reach.is_remote())
            .map(|d| d.name)
            .collect();
        assert!(!local.is_empty(), "the guard would pass vacuously");

        let listed = body_json(
            app.clone()
                .oneshot(Request::get("/ops").body(Body::empty()).unwrap())
                .await
                .unwrap(),
        )
        .await;
        for name in &local {
            assert!(
                !listed["ops"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|o| o["name"] == *name),
                "`{name}` must not appear in the remote registry"
            );
        }

        // Not merely hidden — calling one directly must fail too.
        for name in &local {
            let resp = app
                .clone()
                .oneshot(
                    Request::post(format!("/ops/{name}"))
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"target":"x","with":"sh -c whoami"}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "`{name}` is reachable over HTTP"
            );
        }
    }

    /// A fresh store answers its first question read with the shipped defaults, and the
    /// same defaults are offered as presets to add back after an edit.
    #[tokio::test]
    async fn a_fresh_workspace_has_the_default_questions_and_offers_them_as_presets() {
        let (_d, app) = app().await;
        let questions = body_json(
            app.clone()
                .oneshot(
                    Request::get("/workspace/questions")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        let ids: Vec<&str> = questions["questions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|q| q["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids[0], "page_kind");
        assert!(
            ids.contains(&"record_type") && ids.contains(&"body"),
            "{ids:?}"
        );

        let presets = body_json(
            app.oneshot(
                Request::get("/workspace/presets")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        let groups = presets["presets"].as_array().unwrap();
        assert_eq!(groups[0]["id"], "junk");
        assert_eq!(groups[0]["questions"][0]["id"], "page_kind");
    }

    #[tokio::test]
    async fn invoking_an_op_with_no_body_works() {
        let (_d, app) = app().await;
        let resp = app
            .oneshot(Request::post("/ops/list").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_json(resp).await["sources"].is_array());
    }

    #[tokio::test]
    async fn unknown_op_is_404() {
        let (_d, app) = app().await;
        let resp = app
            .oneshot(Request::post("/ops/nope").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// A bad argument is the caller's mistake, not a fault. Asked of a `Public` op —
    /// which is now the only kind reachable here at all, so a writing op used as the
    /// example would test the 404 above instead of the 400 this one is about.
    #[tokio::test]
    async fn bad_arguments_are_400_not_500() {
        let (_d, app) = app().await;
        let resp = app
            .oneshot(
                Request::post("/ops/list")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"source":"NOT VALID"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn mcp_over_http_shares_the_stdio_handler() {
        let (_d, app) = app().await;
        let resp = app
            .oneshot(
                Request::post("/mcp")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert_eq!(
            json["result"]["tools"].as_array().unwrap().len(),
            op::mcp_tools().len()
        );
    }

    #[tokio::test]
    async fn web_serves_the_embedded_ui() {
        let (_d, app) = app().await;
        let resp = app
            .clone()
            .oneshot(Request::get("/web").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()["content-type"],
            "text/html; charset=utf-8",
            "the browser has to be told what it received"
        );
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        let shell = std::str::from_utf8(&bytes).unwrap();
        assert!(shell.contains("Centinel"), "the embedded document arrived");
        let start = shell
            .find("/web/assets/")
            .expect("the shell loads its assets");
        let end = start + shell[start..].find('"').unwrap();
        let asset = app
            .clone()
            .oneshot(
                Request::get(&shell[start..end])
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(asset.status(), StatusCode::OK, "{}", &shell[start..end]);
        assert_ne!(asset.headers()["content-type"], "text/html; charset=utf-8");
        for path in ["/web/", "/web/classifiers", "/web/runs", "/web/review"] {
            assert_eq!(
                app.clone()
                    .oneshot(Request::get(path).body(Body::empty()).unwrap())
                    .await
                    .unwrap()
                    .status(),
                StatusCode::OK,
                "SPA route {path}"
            );
        }
    }

    /// The review surfaces read before anything is written: an empty store has an empty
    /// evaluation and no queue, and a review from another origin is refused.
    #[tokio::test]
    async fn review_surfaces_answer_on_an_empty_store_and_refuse_foreign_writes() {
        let (_d, app) = app().await;
        let evaluation = body_json(
            app.clone()
                .oneshot(
                    Request::get("/workspace/evaluation")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(evaluation["reviews"], 0);
        assert!(evaluation["questions"].is_array());

        let resp = app
            .clone()
            .oneshot(
                Request::post("/workspace/review")
                    .header("content-type", "application/json")
                    .header("host", "127.0.0.1:8787")
                    .header("origin", "http://attacker.example")
                    .body(Body::from(
                        r#"{"source":"s","resource":"r","derived_sha":"d","verdicts":{}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    /// A page opened after a run started is told about it at once: the list names every
    /// job, and the stream opens on the one asked for as it stands — its count and the
    /// page in hand — before anything new happens.
    #[tokio::test]
    async fn the_job_stream_opens_on_the_job_asked_for_as_it_stands() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(Ctx::new(Store::open(dir.path()).await.unwrap()));
        let app = router(Arc::clone(&ctx));
        let wanted = ctx.jobs.start("run", "schedule");
        let _other = ctx.jobs.start("embed", "schedule");
        wanted.watch(Progress::none()).step_on(
            "0 stored, 0 failed",
            0,
            1005,
            "https://www.tampa.gov/a",
        );

        let listed = body_json(
            app.clone()
                .oneshot(Request::get("/workspace/jobs").body(Body::empty()).unwrap())
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(listed["jobs"].as_array().unwrap().len(), 2);

        let resp = app
            .oneshot(
                Request::get(format!("/workspace/jobs/events?job={}", wanted.id()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.headers()["content-type"], "text/event-stream");
        let mut body = resp.into_body().into_data_stream();
        let frame = futures::StreamExt::next(&mut body).await.unwrap().unwrap();
        let text = std::str::from_utf8(&frame).unwrap();
        assert!(text.contains("event: snapshot"), "{text}");
        let data: Value =
            serde_json::from_str(text.lines().find_map(|l| l.strip_prefix("data: ")).unwrap())
                .unwrap();
        let jobs = data["jobs"].as_array().unwrap();
        assert_eq!(jobs.len(), 1, "only the job asked for");
        assert_eq!(jobs[0]["id"], wanted.id());
        assert_eq!(jobs[0]["current"], "https://www.tampa.gov/a");
        assert_eq!(jobs[0]["total"], 1005);
    }

    #[test]
    fn workspace_writes_require_same_loopback_origin() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "127.0.0.1:8787".parse().unwrap());
        headers.insert("origin", "http://127.0.0.1:8787".parse().unwrap());
        assert!(same_origin(&headers));

        headers.insert("origin", "http://attacker.example".parse().unwrap());
        assert!(!same_origin(&headers));

        headers.insert("host", "attacker.example".parse().unwrap());
        headers.insert("origin", "http://attacker.example".parse().unwrap());
        assert!(
            !same_origin(&headers),
            "matching attacker headers are not loopback"
        );
    }
}
