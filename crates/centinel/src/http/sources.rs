//! `/workspace/sources` — the Sources page.
//!
//! | Route | Purpose |
//! |---|---|
//! | `GET /workspace/sources` | every source (and whether a run is collecting it), and the run holding the lane if one is |
//! | `POST /workspace/sources` | define a source (`source add`'s arguments) |
//! | `GET /workspace/sources/{id}` | its definition, its list row, its log's account of it, its runs |
//! | `PUT /workspace/sources/{id}` | replace editable collection settings; identity and yt-dlp arguments stay intact |
//! | `GET /workspace/sources/{id}/crumbs` | the off-host links its pages carry, from the ledger |
//! | `POST /workspace/sources/{id}/crumbs/rescan` | write the ledger again from the blobs, as a job |
//! | `POST /workspace/sources/{id}/enabled` | `{ "enabled": bool }` |
//! | `POST /workspace/sources/{id}/run` | run it in the store's one lane; 409 when the lane is held |
//! | `POST /workspace/sources/{id}/stages/{stage}` | process stored material in that lane; see [`super::stages`] |
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

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use centinel_core::catalog::{SourceSettings, Sources};
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
        .route("/workspace/sources/{id}", get(detail).put(update))
        .route("/workspace/sources/{id}/crumbs", get(crumbs))
        .route("/workspace/sources/{id}/crumbs/rescan", post(rescan_crumbs))
        .route("/workspace/sources/{id}/enabled", post(enabled))
        .route("/workspace/sources/{id}/run", post(run))
}

async fn source_op(ctx: &Ctx, action: SourceAction) -> anyhow::Result<SourceReport> {
    ops::source(
        ctx,
        SourceArgs { action },
        &Progress::none(),
        &Cancel::none(),
    )
    .await
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
        let defaults = centinel_core::config::Config::load()?.defaults;
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
            "settings_options": {
                "strategies": centinel_core::strategies::crawl::all().iter().map(|s| s.name).collect::<Vec<_>>(),
                "defaults": { "rps": defaults.rps, "lang": defaults.lang },
            },
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

async fn update(
    State(ctx): State<Arc<Ctx>>,
    Path(id): Path<String>,
    Json(settings): Json<SourceSettings>,
) -> Response {
    match source_op(
        &ctx,
        SourceAction::Update(ops::SourceUpdateArgs { id, settings }),
    )
    .await
    {
        Ok(report) => Json(report).into_response(),
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
    spawn_job(
        &ctx,
        "crumbs",
        label,
        |ctx, _, progress, cancel| async move {
            let args = CrumbsArgs {
                source: Some(id),
                rescan: true,
                ..Default::default()
            };
            ops::crumbs(&ctx, args, &progress, &cancel).await
        },
    )
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
    launch_run(&ctx, args)
}

/// Starts an administrator collection through the shared lane and job lifecycle.
pub(super) fn launch_run(ctx: &Arc<Ctx>, args: RunArgs) -> Response {
    let launch = Launch {
        surface: "web",
        trigger: Trigger::Manual,
        schedule: None,
        due_at: None,
    };
    let lane = match Lane::acquire(ctx, args, launch) {
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
    match lane.spawn() {
        Ok(run_id) => (StatusCode::ACCEPTED, Json(json!({ "run_id": run_id }))).into_response(),
        Err(error) => workspace_error(error),
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

    async fn put(app: &axum::Router, id: &str, body: Value, origin: &str) -> Response {
        app.clone()
            .oneshot(
                Request::put(format!("/workspace/sources/{id}"))
                    .header("host", "localhost:8787")
                    .header("origin", origin)
                    .header("content-type", "application/json")
                    .header("x-centinel-secret", "secret")
                    .header("x-centinel-role", "admin")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn settings_save_and_clear_through_the_admin_router_and_survive_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let original = SourceConfig {
            enabled: Some(false),
            yt_dlp_args: vec!["--cookies-from-browser=brave".into()],
            ..SourceConfig::channel("council", "https://www.youtube.com/@Council")
        };
        Sources::new(&store).add(&original).await.unwrap();
        let app = crate::http::router(Arc::new(Ctx::new(store))).layer(
            axum::middleware::from_fn_with_state("secret".to_string(), crate::access::authenticate),
        );
        let settings = json!({
            "rps": 0.25, "matches": ["watch?v="], "strategy": null,
            "lang": "es", "audio_if_no_captions": false,
        });
        let saved = put(&app, "council", settings, "http://localhost:8787").await;
        assert_eq!(saved.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(saved.into_body(), 1 << 20)
            .await
            .unwrap();
        let report: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(report, json!({"action": "update", "source": "council"}));

        let page = get(&app, "/workspace/sources/council").await;
        let definition = &page["definition"];
        assert_eq!(definition["rps"], 0.25);
        assert_eq!(definition["matches"], json!(["watch?v="]));
        assert_eq!(definition["lang"], "es");
        assert_eq!(definition["audio_if_no_captions"], false);
        assert_eq!(definition["id"], "council");
        assert_eq!(definition["channel"], "https://www.youtube.com/@Council");
        assert_eq!(definition["enabled"], false);
        assert_eq!(
            definition["yt_dlp_args"],
            json!(["--cookies-from-browser=brave"])
        );
        assert!(
            page["settings_options"]["strategies"]
                .as_array()
                .unwrap()
                .contains(&json!("listing"))
        );

        let reopened = Store::open(dir.path()).await.unwrap();
        let read = Sources::new(&reopened).require("council").await.unwrap();
        assert_eq!(read.lang.as_deref(), Some("es"));
        assert_eq!(read.audio_if_no_captions, Some(false));
        assert_eq!(read.rps, Some(0.25));
        let cleared = put(
            &app,
            "council",
            json!({
                "rps": null, "matches": [], "strategy": null,
                "lang": null, "audio_if_no_captions": null,
            }),
            "http://localhost:8787",
        )
        .await;
        assert_eq!(cleared.status(), StatusCode::OK);
        let cleared = Sources::new(&reopened).require("council").await.unwrap();
        assert_eq!(
            serde_json::to_value(cleared).unwrap(),
            serde_json::to_value(original).unwrap()
        );
    }

    #[tokio::test]
    async fn settings_reject_identity_and_process_options_and_require_admin_and_same_origin() {
        let (dir, app, _lane) = app().await;
        let before = get(&app, "/workspace/sources/a").await["definition"].clone();
        for (key, value) in [
            ("id", json!("renamed")),
            ("site", json!("https://elsewhere.gov")),
            ("channel", json!("https://www.youtube.com/@Other")),
            ("enabled", json!(false)),
            ("yt_dlp_args", json!(["--exec=touch /tmp/unsafe"])),
        ] {
            let mut body = json!({"matches": []});
            body[key] = value;
            assert_eq!(
                put(&app, "a", body, "http://localhost:8787").await.status(),
                StatusCode::UNPROCESSABLE_ENTITY
            );
        }
        assert_eq!(
            put(&app, "a", json!({"matches": []}), "http://elsewhere.test")
                .await
                .status(),
            StatusCode::FORBIDDEN
        );

        let store = Store::open(dir.path()).await.unwrap();
        let reader = crate::http::router(Arc::new(Ctx::new(store))).layer(
            axum::middleware::from_fn_with_state("secret".to_string(), crate::access::authenticate),
        );
        let request = Request::put("/workspace/sources/a")
            .header("host", "localhost:8787")
            .header("origin", "http://localhost:8787")
            .header("content-type", "application/json")
            .header("x-centinel-secret", "secret")
            .header("x-centinel-role", "user")
            .body(Body::from("{\"matches\":[]}"))
            .unwrap();
        assert_eq!(
            reader.oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            get(&app, "/workspace/sources/a").await["definition"],
            before
        );
    }

    /// Two sources, `b` disabled, behind the admin router, with a run over every enabled
    /// source holding the lane.
    async fn app() -> (tempfile::TempDir, axum::Router, RunLock) {
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
        let lane = RunLock::acquire(
            &store,
            &Holder {
                pid: std::process::id(),
                operation: None,
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
            .oneshot(
                Request::get(path)
                    .header("x-centinel-secret", "secret")
                    .header("x-centinel-role", "admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{path}");
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
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
            .map(|s| {
                (
                    s["id"].as_str().unwrap(),
                    s["collecting"].as_bool().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            collecting,
            [("a", true), ("b", false)],
            "a bare run skips a disabled source"
        );

        for row in list["sources"].as_array().unwrap() {
            let page = get(
                &app,
                &format!("/workspace/sources/{}", row["id"].as_str().unwrap()),
            )
            .await;
            assert_eq!(&page["source"], row);
        }
    }
}
