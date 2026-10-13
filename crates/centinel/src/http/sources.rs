//! `/workspace/sources` — the Sources page.
//!
//! | Route | Purpose |
//! |---|---|
//! | `GET /workspace/sources` | every source, and the run holding the lane if one is |
//! | `POST /workspace/sources` | define a source (`source add`'s arguments) |
//! | `GET /workspace/sources/{id}` | its definition, its log's account of it, its runs |
//! | `GET /workspace/sources/{id}/crumbs` | the off-host links its pages carry, from the ledger |
//! | `POST /workspace/sources/{id}/crumbs/rescan` | write the ledger again from the blobs, as a job |
//! | `POST /workspace/sources/{id}/enabled` | `{ "enabled": bool }` |
//! | `POST /workspace/sources/{id}/run` | run it in the store's one lane; 409 when the lane is held |
//! | `POST /workspace/sources/try` | `{ "url" }`: investigate it and check it, as a job |
//! | `GET /workspace/sources/tries/{id}` | what a try found, once it has finished |
//!
//! ## Who may call these
//!
//! An administrator, from the browser. `access.rs` refuses every `/workspace` write to a
//! user, and the gateway forwards `/workspace` only for a browser session — an agent's MCP
//! token never reaches here. That is the line: **a signed-in administrator is the operator;
//! an agent is not.** The registry is unchanged — `run`, `investigate` and `check` are still
//! unreachable over `/ops` and MCP — and these routes call the library directly.
//!
//! A run started here takes the same lane a scheduled run takes ([`crate::lane`]), so a
//! second click while one is running is a 409, not a second crawl of a city. It answers
//! with the run id, which is also the job's id: the page opens the live drawer on it, and
//! `POST /workspace/jobs/{id}/stop` ends it.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use centinel_core::catalog::Sources;
use centinel_core::journal::{Journal, RunLock, Trigger};
use centinel_core::op::{Cancel, Ctx, Progress};
use centinel_core::ops::{
    self, AddArgs, CrumbsArgs, ListArgs, RunArgs, SourceAction, SourceArgs, SourceReport, Stage,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{forbidden_origin, same_origin, workspace_error};
use crate::lane::{Lane, Launch};

/// Runs shown on a source's page, newest first.
const RUNS_SHOWN: usize = 20;
/// Lines of extracted text a try keeps per document.
const TRY_HEAD: usize = 40;

pub fn routes() -> axum::Router<Arc<Ctx>> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/workspace/sources", get(list).post(add))
        .route("/workspace/sources/try", post(try_address))
        .route("/workspace/sources/tries/{id}", get(tried))
        .route("/workspace/sources/{id}", get(detail))
        .route("/workspace/sources/{id}/crumbs", get(crumbs))
        .route("/workspace/sources/{id}/crumbs/rescan", post(rescan_crumbs))
        .route("/workspace/sources/{id}/enabled", post(enabled))
        .route("/workspace/sources/{id}/run", post(run))
}

async fn source_op(ctx: &Ctx, action: SourceAction) -> anyhow::Result<SourceReport> {
    ops::source(ctx, SourceArgs { action }, &Progress::none(), &Cancel::none()).await
}

async fn list(State(ctx): State<Arc<Ctx>>) -> Response {
    match source_op(&ctx, SourceAction::List).await {
        Ok(SourceReport::List { sources }) => Json(json!({
            "sources": sources,
            "running": RunLock::current(&ctx.store),
        }))
        .into_response(),
        Ok(_) => unreachable!("`source list` answers with a list"),
        Err(error) => workspace_error(error),
    }
}

async fn add(State(ctx): State<Arc<Ctx>>, headers: HeaderMap, Json(args): Json<AddArgs>) -> Response {
    if !same_origin(&headers) {
        return forbidden_origin();
    }
    // `--run` is a separate request — the Run button — so the lane rules apply to it.
    let args = AddArgs { run: false, ..args };
    match source_op(&ctx, SourceAction::Add(args)).await {
        Ok(report) => (StatusCode::CREATED, Json(report)).into_response(),
        Err(error) => workspace_error(error),
    }
}

async fn detail(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>) -> Response {
    let result = async {
        let definition = Sources::new(&ctx.store).require(&id).await?;
        let summary = ops::list(
            &ctx,
            ListArgs {
                source: Some(id.clone()),
                max_problems: 50,
            },
        )
        .await?
        .sources
        .into_iter()
        .next();
        // Runs whose report names this source, newest first. A run that failed before it
        // produced a report names no source, so it shows on the Runs page rather than here.
        let mut runs: Vec<_> = Journal::new(&ctx.store)
            .read()
            .await?
            .into_iter()
            .filter(|a| {
                a.report
                    .as_ref()
                    .is_some_and(|r| r.sources.iter().any(|s| s.source == id))
            })
            .collect();
        runs.reverse();
        runs.truncate(RUNS_SHOWN);
        anyhow::Ok(json!({
            "definition": definition,
            "summary": summary,
            "runs": runs,
            "running": RunLock::current(&ctx.store),
        }))
    }
    .await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => workspace_error(error),
    }
}

/// Hosts named on the page before the list is cut off.
const CRUMBS_SHOWN: usize = 200;

/// The ledger, read. Opens no blob: a page with no row is counted on the report, and the
/// button that writes it down is [`rescan_crumbs`].
async fn crumbs(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>) -> Response {
    let args = CrumbsArgs {
        source: Some(id),
        max: CRUMBS_SHOWN,
        ..Default::default()
    };
    match ops::crumbs(&ctx, args, &Progress::none(), &Cancel::none()).await {
        Ok(report) => Json(report).into_response(),
        Err(error) => workspace_error(error),
    }
}

/// `centinel crumbs --rescan --source {id}`, as a job: every HTML blob of the source is
/// read once and its row written down, after which the ledger answers alone. The one-time
/// backfill for a source collected before the ledger existed, and the repair for a ledger
/// that was damaged — explicit, because reading nineteen thousand blobs is a cost somebody
/// should choose rather than one a page click incurs.
async fn rescan_crumbs(
    State(ctx): State<Arc<Ctx>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden_origin();
    }
    if let Err(error) = Sources::new(&ctx.store).require(&id).await {
        return workspace_error(error);
    }
    let (job, cancel) = ctx.jobs.start_stoppable(
        format!("crumbs-{}", jiff::Timestamp::now().as_millisecond()),
        "crumbs",
        id.clone(),
    );
    let job_id = job.id().to_string();
    let worker = Arc::clone(&ctx);
    tokio::spawn(async move {
        let progress = job.watch(Progress::none());
        let args = CrumbsArgs {
            source: Some(id),
            rescan: true,
            ..Default::default()
        };
        let result = ops::crumbs(&worker, args, &progress, &cancel).await;
        job.finish(&result);
    });
    (StatusCode::ACCEPTED, Json(json!({ "id": job_id }))).into_response()
}

#[derive(Deserialize)]
struct Enabled {
    enabled: bool,
}

async fn enabled(
    State(ctx): State<Arc<Ctx>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Enabled>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden_origin();
    }
    let action = match body.enabled {
        true => SourceAction::Enable(ops::IdArgs { id }),
        false => SourceAction::Disable(ops::IdArgs { id }),
    };
    match source_op(&ctx, action).await {
        Ok(report) => Json(report).into_response(),
        Err(error) => workspace_error(error),
    }
}

/// What the Run button sends. Absent fields run the whole pipeline.
#[derive(Default, Deserialize)]
struct RunBody {
    #[serde(default)]
    skip: Vec<Stage>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    refresh: bool,
}

async fn run(
    State(ctx): State<Arc<Ctx>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Option<Json<RunBody>>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden_origin();
    }
    if let Err(error) = Sources::new(&ctx.store).require(&id).await {
        return workspace_error(error);
    }
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let args = RunArgs {
        sources: vec![id],
        skip: body.skip,
        limit: body.limit,
        refresh: body.refresh,
        ..Default::default()
    };
    let launch = Launch {
        surface: "web",
        trigger: Trigger::Manual,
        schedule: None,
        due_at: None,
    };
    let lane = match Lane::acquire(&ctx, args, launch) {
        Ok(lane) => lane,
        Err(held) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({ "error": held.to_string(), "running": held.0 })),
            )
                .into_response();
        }
    };
    // The run id is the job's id: the page opens the drawer on it and Stop names it.
    let run_id = lane.run_id();
    // On threads of its own, as the scheduler runs: one stage that blocks must not take a
    // request worker for an hour.
    let spawned = std::thread::Builder::new()
        .name(format!("run-{run_id}"))
        .spawn(move || match tokio::runtime::Runtime::new() {
            Ok(runtime) => {
                runtime.block_on(lane.run(None, Cancel::none()));
            }
            Err(e) => tracing::error!(error = %e, "could not start a runtime for a web run"),
        });
    match spawned {
        Ok(_) => (StatusCode::ACCEPTED, Json(json!({ "run_id": run_id }))).into_response(),
        Err(e) => workspace_error(anyhow::anyhow!("could not start the run: {e}")),
    }
}

#[derive(Deserialize)]
struct TryBody {
    url: String,
}

/// Investigates an address and checks what extraction makes of it, as one job, and files
/// both reports under `tries/<job>/` in the store.
///
/// Filed rather than returned because a try takes tens of seconds and the person may leave
/// the page; filed in the store so the report, and the bytes `check` saved beside it, are
/// still there to hand to an agent tomorrow. Nothing here is a Source or an Observation —
/// `tries/` is outside the log, and deleting it loses nothing the corpus holds.
async fn try_address(
    State(ctx): State<Arc<Ctx>>,
    headers: HeaderMap,
    Json(body): Json<TryBody>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden_origin();
    }
    // An address, never a path: `check` reads local files, and this is not a file reader.
    let url = match reqwest::Url::parse(body.url.trim()) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => url,
        _ => return workspace_error(anyhow::anyhow!("`{}` is not an http(s) address", body.url)),
    };
    // Named by the instant rather than the process's counter, so a try filed before a
    // restart is not overwritten by the first one after it.
    let job = ctx.jobs.start_as(
        format!("try-{}", jiff::Timestamp::now().as_millisecond()),
        "try",
        url.as_str(),
    );
    let id = job.id().to_string();
    let dir = ctx.store.root().join("tries").join(&id);
    let worker = Arc::clone(&ctx);
    tokio::spawn(async move {
        let progress = job.watch(Progress::none());
        let result = tried_report(&worker, url.as_str(), &dir, &progress).await;
        if let Err(error) = &result {
            tracing::warn!(address = %url, error = %format!("{error:#}"), "try failed");
        }
        job.finish(&result);
    });
    (StatusCode::ACCEPTED, Json(json!({ "id": id }))).into_response()
}

async fn tried_report(
    ctx: &Ctx,
    url: &str,
    dir: &std::path::Path,
    progress: &Progress,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let started = jiff::Timestamp::now();

    progress.say("investigating");
    let investigate = ops::investigate(
        ctx,
        serde_json::from_value(json!({ "target": url }))?,
        progress,
        &Cancel::none(),
    )
    .await;

    progress.say("checking what extraction makes of it");
    let check = ops::check(
        ctx,
        serde_json::from_value(json!({
            "target": url,
            "out": dir.join("files"),
            "head": TRY_HEAD,
            "strategy": "auto",
        }))?,
        progress,
        &Cancel::none(),
    )
    .await;

    // Each half is kept whatever became of the other: a site that refuses the probe may
    // still serve its front page, and that difference is the finding.
    let report = json!({
        "address": url,
        "started_at": started.to_string(),
        "finished_at": jiff::Timestamp::now().to_string(),
        "investigate": as_outcome(investigate),
        "check": as_outcome(check),
    });
    std::fs::write(dir.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

fn as_outcome<T: serde::Serialize>(result: anyhow::Result<T>) -> Value {
    match result {
        Ok(report) => json!({ "report": report }),
        Err(error) => json!({ "error": format!("{error:#}") }),
    }
}

async fn tried(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>) -> Response {
    // A job id is `try-<n>`; anything else is not a name this route wrote.
    if !id.starts_with("try-") || !id[4..].chars().all(|c| c.is_ascii_digit()) {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "no such try" }))).into_response();
    }
    let path = ctx.store.root().join("tries").join(&id).join("report.json");
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) => Json(value).into_response(),
            Err(e) => workspace_error(anyhow::anyhow!("{} does not parse: {e}", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": "not finished, or no such try" })))
                .into_response()
        }
        Err(e) => workspace_error(e.into()),
    }
}
