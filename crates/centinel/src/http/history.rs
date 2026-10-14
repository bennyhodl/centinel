//! Administrator collection history. Reruns repeat source scope with current defaults;
//! the journal does not retain the original pipeline arguments.
use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use centinel_core::{
    journal::{Attempt, Outcome, RunLock},
    op::Ctx,
    ops::{self, HistoryArgs, RunArgs},
};
use serde::Deserialize;
use serde_json::json;

use super::{sources::launch_run, workspace_error};

pub fn routes() -> axum::Router<Arc<Ctx>> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/workspace/history", get(list))
        .route("/workspace/history/{id}", get(detail))
        .route("/workspace/history/{id}/rerun", post(rerun))
}

#[derive(Default, Deserialize)]
struct Filters {
    source: Option<String>,
    schedule: Option<String>,
    outcome: Option<String>,
    page: Option<usize>,
    page_size: Option<usize>,
}

async fn list(State(ctx): State<Arc<Ctx>>, Query(filters): Query<Filters>) -> Response {
    if filters.outcome.as_deref().is_some_and(|value| {
        !matches!(
            value,
            "" | "ok" | "failed" | "partial" | "interrupted" | "skipped"
        )
    }) {
        return workspace_error(anyhow::anyhow!("unknown collection outcome"));
    }
    let result = ops::history(
        &ctx,
        HistoryArgs {
            source: filters.source.filter(|s| !s.is_empty()),
            schedule: filters.schedule.filter(|s| !s.is_empty()),
            limit: usize::MAX,
            ..Default::default()
        },
    )
    .await;
    match result {
        Ok(report) => {
            let attempts: Vec<_> = report
                .attempts
                .into_iter()
                .filter(|attempt| match filters.outcome.as_deref() {
                    Some("failed") => matches!(
                        attempt.outcome,
                        Outcome::Failed | Outcome::Partial | Outcome::Interrupted
                    ),
                    Some(value) if !value.is_empty() => attempt.outcome.as_str() == value,
                    _ => true,
                })
                .collect();
            let total = attempts.len();
            let page_size = filters.page_size.unwrap_or(25).clamp(1, 100);
            let page = filters.page.unwrap_or(1).max(1);
            let offset = (page - 1).saturating_mul(page_size);
            let attempts: Vec<_> = attempts.into_iter().skip(offset).take(page_size).collect();
            Json(json!({ "attempts": attempts, "total": total, "page": page, "page_size": page_size, "running": RunLock::current(&ctx.store) })).into_response()
        }
        Err(error) => workspace_error(error),
    }
}

async fn resolve(ctx: &Ctx, id: String) -> Result<Attempt, Response> {
    match ops::history(
        ctx,
        HistoryArgs {
            run: Some(id),
            ..Default::default()
        },
    )
    .await
    {
        Ok(report) => {
            if let Some(ids) = report.ambiguous {
                return Err((StatusCode::CONFLICT, Json(json!({ "error": "Run prefix is ambiguous. Use a longer id.", "ambiguous": ids }))).into_response());
            }
            Ok(report
                .attempts
                .into_iter()
                .next()
                .expect("history resolves one attempt"))
        }
        Err(error) => {
            // A missing handle is distinct from an unreadable journal.
            if error.to_string().starts_with("no run matching") {
                Err((
                    StatusCode::NOT_FOUND,
                    Json(json!({"error": error.to_string()})),
                )
                    .into_response())
            } else {
                Err(workspace_error(error))
            }
        }
    }
}

async fn detail(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>) -> Response {
    match resolve(&ctx, id).await {
        Ok(attempt) => Json(json!({ "attempt": attempt })).into_response(),
        Err(response) => response,
    }
}

async fn rerun(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>) -> Response {
    let attempt = match resolve(&ctx, id).await {
        Ok(attempt) => attempt,
        Err(response) => return response,
    };
    launch_run(
        &ctx,
        RunArgs {
            sources: attempt.sources,
            ..Default::default()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use centinel_core::{
        journal::{Journal, Trigger},
        store::Store,
    };
    use tower::ServiceExt;

    async fn fixture() -> (tempfile::TempDir, Arc<Ctx>, axum::Router) {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(Ctx::new(Store::open(dir.path()).await.unwrap()));
        for (day, outcome, schedule, source) in [
            (1, Outcome::Ok, None, "a"),
            (2, Outcome::Partial, Some("daily"), "a"),
            (3, Outcome::Interrupted, Some("daily"), "b"),
            (4, Outcome::Failed, None, "a"),
        ] {
            let at = format!("2026-08-0{day}T00:00:00Z");
            Journal::new(&ctx.store)
                .append(&Attempt {
                    run_id: at.clone(),
                    started_at: at,
                    finished_at: format!("2026-08-0{day}T00:00:12Z"),
                    schedule: schedule.map(str::to_owned),
                    sources: vec![source.into()],
                    trigger: if schedule.is_some() {
                        Trigger::Schedule
                    } else {
                        Trigger::Manual
                    },
                    due_at: None,
                    outcome,
                    detail: Some("fixture detail".into()),
                    added: centinel_core::journal::Added {
                        documents: day,
                        derived: 0,
                        chunks: 0,
                    },
                    subtracted: Default::default(),
                    report: None,
                })
                .await
                .unwrap();
        }
        let app = crate::http::router(ctx.clone());
        (dir, ctx, app)
    }

    async fn request(
        app: &axum::Router,
        method: &str,
        path: &str,
    ) -> (StatusCode, serde_json::Value) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("host", "127.0.0.1:8787")
                    .header("origin", "http://127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn stored_history_filters_pages_and_resolves_handles() {
        let (_dir, _ctx, app) = fixture().await;
        let (status, value) = request(
            &app,
            "GET",
            "/workspace/history?outcome=failed&page_size=1&page=2",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["total"], 3);
        assert_eq!(value["attempts"][0]["outcome"], "interrupted");
        let (_, value) = request(
            &app,
            "GET",
            "/workspace/history?source=a&schedule=daily&outcome=failed",
        )
        .await;
        assert_eq!(value["total"], 1);
        assert_eq!(value["attempts"][0]["outcome"], "partial");
        assert_eq!(value["attempts"][0]["added"]["documents"], 2);
        let (_, value) = request(&app, "GET", "/workspace/history/2026-08-02").await;
        assert_eq!(value["attempt"]["detail"], "fixture detail");
        assert_eq!(
            request(&app, "GET", "/workspace/history/2026-08").await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            request(&app, "GET", "/workspace/history/missing").await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(&app, "GET", "/workspace/history?outcome=unknown")
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn rerun_uses_the_lane_and_records_a_new_manual_attempt() {
        let (_dir, ctx, app) = fixture().await;
        let held = crate::lane::Lane::acquire(
            &ctx,
            RunArgs::default(),
            crate::lane::Launch {
                surface: "web",
                trigger: Trigger::Manual,
                schedule: None,
                due_at: None,
            },
        )
        .unwrap();
        assert_eq!(
            request(&app, "POST", "/workspace/history/2026-08-02/rerun")
                .await
                .0,
            StatusCode::CONFLICT
        );
        drop(held);
        let (status, value) = request(&app, "POST", "/workspace/history/2026-08-02/rerun").await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let id = value["run_id"].as_str().unwrap();
        // The historical source is absent from this temporary store. The real pipeline
        // fails locally before network work and still journals the new manual attempt.
        for _ in 0..200 {
            let attempts = Journal::new(&ctx.store).read().await.unwrap();
            if let Some(attempt) = attempts.iter().find(|a| a.run_id == id) {
                assert_eq!(attempt.sources, ["a"]);
                assert_eq!(attempt.trigger, Trigger::Manual);
                assert_eq!(attempt.schedule, None);
                assert_eq!(attempt.outcome, Outcome::Failed);
                assert!(RunLock::current(&ctx.store).is_none());
                assert!(
                    ctx.jobs
                        .snapshot()
                        .iter()
                        .any(|job| job.id == id && job.outcome.is_some())
                );
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("rerun did not finish and append its journal record");
    }

    #[tokio::test]
    async fn collection_history_is_admin_only_at_the_authenticated_boundary() {
        let (_dir, _ctx, app) = fixture().await;
        let app = app.layer(axum::middleware::from_fn_with_state(
            "secret".to_string(),
            crate::access::authenticate,
        ));
        for (method, path, role, origin, expected) in [
            (
                "GET",
                "/workspace/history",
                "admin",
                "http://127.0.0.1:8787",
                StatusCode::OK,
            ),
            (
                "GET",
                "/workspace/history",
                "user",
                "http://127.0.0.1:8787",
                StatusCode::FORBIDDEN,
            ),
            (
                "GET",
                "/workspace/history/2026-08-02",
                "user",
                "http://127.0.0.1:8787",
                StatusCode::FORBIDDEN,
            ),
            (
                "POST",
                "/workspace/history/2026-08-02/rerun",
                "user",
                "http://127.0.0.1:8787",
                StatusCode::FORBIDDEN,
            ),
            (
                "POST",
                "/workspace/history/2026-08-02/rerun",
                "admin",
                "https://other.example",
                StatusCode::FORBIDDEN,
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header("x-centinel-secret", "secret")
                        .header("x-centinel-role", role)
                        .header("host", "127.0.0.1:8787")
                        .header("origin", origin)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{role} {method} {path}");
        }
    }
}
