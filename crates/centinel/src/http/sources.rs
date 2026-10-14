//! `/workspace/sources` — the Sources page.
//!
//! | Route | Purpose |
//! |---|---|
//! | `GET /workspace/sources` | every source (and whether a run is collecting it), and the run holding the lane if one is |
//! | `POST /workspace/sources` | define a source (`source add`'s arguments) |
//! | `GET /workspace/sources/{id}` | its definition, its list row, its log's account of it, its runs |
//! | `GET /workspace/sources/{id}/crumbs` | the off-host links its pages carry, from the ledger |
//! | `POST /workspace/sources/{id}/crumbs/rescan` | write the ledger again from the blobs, as a job |
//! | `POST /workspace/sources/{id}/enabled` | `{ "enabled": bool }` |
//! | `POST /workspace/sources/{id}/run` | run it in the store's one lane; 409 when the lane is held |
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
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use centinel_core::catalog::Sources;
use centinel_core::journal::{Journal, RunLock, Trigger};
use centinel_core::op::{Cancel, Ctx, Progress};
use centinel_core::ops::{
    self, AddArgs, CrumbsArgs, ListArgs, RunArgs, SourceAction, SourceArgs, SourceReport, Stage,
};
use serde::Deserialize;
use serde_json::json;

use super::{spawn_job, workspace_error};
use crate::lane::{Lane, Launch};

/// Runs shown on a source's page, newest first.
const RUNS_SHOWN: usize = 20;

pub fn routes() -> axum::Router<Arc<Ctx>> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/workspace/sources", get(list).post(add))
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
    match ops::listed(&ctx, None).await {
        Ok(sources) => Json(json!({
            "sources": sources,
            "running": RunLock::current(&ctx.store),
        }))
        .into_response(),
        Err(error) => workspace_error(error),
    }
}

async fn add(State(ctx): State<Arc<Ctx>>, Json(args): Json<AddArgs>) -> Response {
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
        // The runs that covered this source, newest first — including one stopped before it
        // reported, which its arguments still name (`Attempt::covers`).
        let mut runs: Vec<_> = Journal::new(&ctx.store)
            .read()
            .await?
            .into_iter()
            .filter(|a| a.covers(&id))
            .collect();
        runs.reverse();
        runs.truncate(RUNS_SHOWN);
        anyhow::Ok(json!({
            "definition": definition,
            // The row the list shows for it, built by the same function.
            "source": ops::listed(&ctx, Some(&id)).await?.pop(),
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
async fn rescan_crumbs(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>) -> Response {
    if let Err(error) = Sources::new(&ctx.store).require(&id).await {
        return workspace_error(error);
    }
    let label = id.clone();
    spawn_job(&ctx, "crumbs", label, |ctx, _, progress, cancel| async move {
        let args = CrumbsArgs {
            source: Some(id),
            rescan: true,
            ..Default::default()
        };
        ops::crumbs(&ctx, args, &progress, &cancel).await
    })
}

#[derive(Deserialize)]
struct Enabled {
    enabled: bool,
}

async fn enabled(
    State(ctx): State<Arc<Ctx>>,
    Path(id): Path<String>,
    Json(body): Json<Enabled>,
) -> Response {
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
    Path(id): Path<String>,
    body: Option<Json<RunBody>>,
) -> Response {
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

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use centinel_core::config::SourceConfig;
    use centinel_core::journal::Holder;
    use centinel_core::store::Store;
    use serde_json::Value;
    use tower::ServiceExt;

    use super::*;

    /// Two sources, `b` disabled, behind the admin router, with a run over every enabled
    /// source holding the lane.
    async fn app() -> (tempfile::TempDir, axum::Router, RunLock) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let sources = Sources::new(&store);
        sources.add(&SourceConfig::site("a", "https://a.gov")).await.unwrap();
        sources.add(&SourceConfig::site("b", "https://b.gov")).await.unwrap();
        sources.set_enabled("b", false).await.unwrap();
        let lane = RunLock::acquire(
            &store,
            &Holder {
                pid: std::process::id(),
                started_at: jiff::Timestamp::now().to_string(),
                trigger: Trigger::Manual,
                schedule: None,
                sources: Vec::new(),
                args: String::new(),
            },
        )
        .unwrap();
        let app = crate::http::router(Arc::new(Ctx::new(store)))
            .layer(axum::Extension(crate::access::Role::Admin));
        (dir, app, lane)
    }

    async fn get(app: &axum::Router, path: &str) -> Value {
        let resp = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{path}");
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// The page reads `collecting` instead of working out from the lane which sources a run
    /// is walking, and a source's own page shows the row the list shows.
    #[tokio::test]
    async fn the_list_and_a_sources_page_agree_on_whether_a_run_is_collecting_it() {
        let (_d, app, _lane) = app().await;

        let list = get(&app, "/workspace/sources").await;
        let collecting: Vec<(&str, bool)> = list["sources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s["id"].as_str().unwrap(), s["collecting"].as_bool().unwrap()))
            .collect();
        assert_eq!(collecting, [("a", true), ("b", false)], "a bare run skips a disabled source");

        for row in list["sources"].as_array().unwrap() {
            let page = get(&app, &format!("/workspace/sources/{}", row["id"].as_str().unwrap())).await;
            assert_eq!(&page["source"], row);
        }
    }
}
