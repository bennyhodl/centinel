//! Browser-session-only schedule administration. The operator registry stays unchanged.
use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use centinel_core::{
    config::ScheduleConfig,
    journal::Trigger,
    op::Ctx,
    ops::{self, RmArgs, ScheduleAction, SetArgs},
};
use serde::Deserialize;
use serde_json::json;

use super::{workspace_error, workspace_response};
use crate::{
    lane::{Lane, Launch},
    schedule_control::ScheduleControl,
};

type Control = Extension<Arc<ScheduleControl>>;

pub fn routes() -> axum::Router<Arc<Ctx>> {
    axum::Router::new()
        .route("/workspace/schedules", get(list).post(create))
        .route(
            "/workspace/schedules/{id}",
            axum::routing::put(update).delete(remove),
        )
        .route("/workspace/schedules/{id}/enabled", post(enabled))
        .route("/workspace/schedules/{id}/run", post(run))
}

async fn list(State(ctx): State<Arc<Ctx>>, Extension(control): Control) -> Response {
    workspace_response(async {
        let config = control.snapshot();
        let report = ops::schedules_for(&ctx, &config).await?;
        let mut rows = Vec::new();
        for (definition, status) in config.schedules.iter().zip(report.schedules) {
            let zone = definition.zone()?;
            let jitter = jiff::Span::new().seconds(centinel_core::schedule::jitter_offset(
                &ctx.store.root().display().to_string(), &definition.id, definition.jitter_secs()
            ) as i64);
            let next: Vec<_> = definition.cron()?.next_n(jiff::Timestamp::now() - jitter, &zone, 3)
                .into_iter().map(|at| (at + jitter).to_zoned(zone.clone()).strftime("%Y-%m-%dT%H:%M:%S%:z").to_string()).collect();
            rows.push(json!({ "definition": definition, "status": status, "next": next }));
        }
        anyhow::Ok(json!({ "schedules": rows, "running": report.running,
            "scheduler": { "enabled": control.is_running(), "disabled_reason": control.disabled_reason() } }))
    }.await)
}

fn saved(control: &ScheduleControl) -> Response {
    Json(json!({ "saved": true, "applied": control.is_running(), "disabled_reason": control.disabled_reason() })).into_response()
}

async fn create(
    State(ctx): State<Arc<Ctx>>,
    Extension(control): Control,
    Json(body): Json<ScheduleConfig>,
) -> Response {
    match control.edit(&ctx, ScheduleAction::Set(body.into())).await {
        Ok(()) => saved(&control),
        Err(error) => workspace_error(error),
    }
}

async fn update(
    State(ctx): State<Arc<Ctx>>,
    Extension(control): Control,
    Path(id): Path<String>,
    Json(body): Json<ScheduleConfig>,
) -> Response {
    if body.id != id || control.snapshot().schedule(&id).is_none() {
        return workspace_error(anyhow::anyhow!(
            "schedule id must match an existing schedule"
        ));
    }
    match control
        .edit(
            &ctx,
            ScheduleAction::Set(SetArgs {
                replace: true,
                ..body.into()
            }),
        )
        .await
    {
        Ok(()) => saved(&control),
        Err(error) => workspace_error(error),
    }
}

async fn remove(
    State(ctx): State<Arc<Ctx>>,
    Extension(control): Control,
    Path(id): Path<String>,
) -> Response {
    match control
        .edit(&ctx, ScheduleAction::Rm(RmArgs { id, config: None }))
        .await
    {
        Ok(()) => saved(&control),
        Err(error) => workspace_error(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Enabled {
    enabled: bool,
}

async fn enabled(
    State(ctx): State<Arc<Ctx>>,
    Extension(control): Control,
    Path(id): Path<String>,
    Json(body): Json<Enabled>,
) -> Response {
    match control.set_enabled(&ctx, &id, body.enabled).await {
        Ok(()) => saved(&control),
        Err(error) => workspace_error(error),
    }
}

async fn run(
    State(ctx): State<Arc<Ctx>>,
    Extension(control): Control,
    Path(id): Path<String>,
) -> Response {
    let Some(schedule) = control.snapshot().schedule(&id).cloned() else {
        return workspace_error(anyhow::anyhow!("no schedule `{id}`"));
    };
    let args = match schedule.run_args() {
        Ok(args) => args,
        Err(error) => return workspace_error(error),
    };
    let lane = match Lane::acquire(
        &ctx,
        args,
        Launch {
            surface: "web",
            trigger: Trigger::Manual,
            schedule: Some(id),
            due_at: None,
        },
    ) {
        Ok(lane) => lane,
        Err(held) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({ "error": held.to_string(), "running": held.0 })),
            )
                .into_response();
        }
    };
    match lane.spawn() {
        Ok(run_id) => (StatusCode::ACCEPTED, Json(json!({ "run_id": run_id }))).into_response(),
        Err(error) => workspace_error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use centinel_core::{
        config::{Config, SourceConfig},
        journal::{Holder, Journal, RunLock},
        store::Store,
    };
    use serde_json::Value;
    use tower::ServiceExt;

    async fn request(
        app: &axum::Router,
        method: &str,
        path: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("host", "127.0.0.1:8787")
                    .header("origin", "http://127.0.0.1:8787")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// Exercises HTTP, the explicit server config, live scheduler reload and persistence.
    /// A failed edit cannot remove a schedule or change what the running loop will use.
    #[tokio::test]
    async fn saves_apply_to_the_running_scheduler_and_pause_survives_reload() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).await.unwrap();
        centinel_core::catalog::Sources::new(&store)
            .add(&SourceConfig::site("fixture", "http://127.0.0.1:1"))
            .await
            .unwrap();
        let ctx = Arc::new(Ctx::new(store));
        let path = dir.path().join("active.toml");
        std::fs::write(&path, "# active server config\n").unwrap();
        let mut scheduler = crate::schedule::Scheduler::new(ctx.clone(), path.to_str())
            .await
            .unwrap();
        let control = scheduler.control();
        let app = crate::http::router_with_schedules(ctx.clone(), control.clone());
        let body = json!({ "id": "daily", "cron": "@yearly", "tz": "UTC", "sources": ["fixture"], "jitter_secs": 0 });
        let (status, result) = request(&app, "POST", "/workspace/schedules", body.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["applied"], false);
        scheduler.reload().await.unwrap();
        assert_eq!(scheduler.schedules()[0].id, "daily");
        let (tx, reload) = crate::schedule::ReloadSignal::channel();
        let (cancel, thread) = crate::schedule::spawn(scheduler, reload).unwrap();
        let (_, list) = request(&app, "GET", "/workspace/schedules", Value::Null).await;
        assert_eq!(list["scheduler"]["enabled"], true);
        assert_eq!(list["schedules"][0]["next"].as_array().unwrap().len(), 3);

        let before = std::fs::read(&path).unwrap();
        for invalid in [
            json!({"cron": "bad"}),
            json!({"sources": ["missing"]}),
            json!({"config": "/tmp/untrusted.toml"}),
        ] {
            let mut edit = body.clone();
            edit.as_object_mut()
                .unwrap()
                .extend(invalid.as_object().unwrap().clone());
            let (status, _) = request(&app, "PUT", "/workspace/schedules/daily", edit).await;
            assert!(status.is_client_error());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(control.snapshot().schedules[0].cron, "@yearly");
        }
        let (status, saved) = request(
            &app,
            "PUT",
            "/workspace/schedules/daily",
            json!({"id":"daily", "cron":"@monthly", "tz":"UTC", "sources":["fixture"]}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(saved["applied"], true);
        assert_eq!(control.snapshot().schedules[0].cron, "@monthly");
        let (_, saved) = request(
            &app,
            "POST",
            "/workspace/schedules/daily/enabled",
            json!({"enabled":false}),
        )
        .await;
        assert_eq!(saved["applied"], true);
        assert!(!control.snapshot().schedules[0].is_enabled());
        assert!(!Config::from_file(&path).unwrap().schedules[0].is_enabled());
        control.reload(&ctx).await.unwrap();
        assert!(!control.snapshot().schedules[0].is_enabled());
        let (_, saved) = request(
            &app,
            "POST",
            "/workspace/schedules/daily/enabled",
            json!({"enabled":true}),
        )
        .await;
        assert_eq!(saved["applied"], true);
        assert!(control.snapshot().schedules[0].is_enabled());
        let (status, _) = request(&app, "DELETE", "/workspace/schedules/daily", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert!(control.snapshot().schedules.is_empty());
        assert!(Config::from_file(&path).unwrap().schedules.is_empty());
        cancel.cancel();
        drop(tx);
        thread.join().unwrap();
        assert!(!control.is_running());
    }

    /// A manual scheduled run must contend with every other collector and record its
    /// schedule in the real job and journal. All stages are skipped against a local fixture.
    #[tokio::test]
    async fn run_now_uses_the_collection_lane_and_journals_the_schedule() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).await.unwrap();
        centinel_core::catalog::Sources::new(&store)
            .add(&SourceConfig::site("fixture", "http://127.0.0.1:1"))
            .await
            .unwrap();
        let ctx = Arc::new(Ctx::new(store));
        let path = dir.path().join("active.toml");
        std::fs::write(&path, "[[schedule]]\nid = 'daily'\ncron = '@daily'\nsources = ['fixture']\nskip = ['discover', 'collect', 'extract', 'transcribe', 'index', 'embed']\nenabled = false\n").unwrap();
        let control = ScheduleControl::load(path.to_str(), "disabled by --no-schedule").unwrap();
        let app = crate::http::router_with_schedules(ctx.clone(), control);
        let held = RunLock::acquire(
            &ctx.store,
            &Holder {
                pid: std::process::id(),
                started_at: jiff::Timestamp::now().to_string(),
                trigger: Trigger::Manual,
                schedule: None,
                sources: vec![],
                args: String::new(),
            },
        )
        .unwrap();
        let (status, _) =
            request(&app, "POST", "/workspace/schedules/daily/run", Value::Null).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(ctx.jobs.snapshot().is_empty());
        drop(held);
        let (status, run) =
            request(&app, "POST", "/workspace/schedules/daily/run", Value::Null).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let id = run["run_id"].as_str().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Some(attempt) = Journal::new(&ctx.store).last_for("daily").await.unwrap() {
                    assert_eq!(attempt.run_id, id);
                    assert_eq!(attempt.trigger, Trigger::Manual);
                    assert_eq!(attempt.outcome, centinel_core::journal::Outcome::Ok);
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(ctx.jobs.snapshot().iter().any(|job| job.id == id));
        assert!(RunLock::current(&ctx.store).is_none());
    }
}
