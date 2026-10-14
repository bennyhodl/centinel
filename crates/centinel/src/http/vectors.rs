//! Browser-admin vector maintenance. Reports survive the job's bounded event history.
//! Repair accepts a report id, never fragment ids or filesystem paths, and the core
//! rechecks the reviewed audit under its writer lock. No archive files are removed.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use centinel_core::op::Ctx;
use centinel_core::ops::{self, EmbedArgs, VerifyReport, VerifyTableArgs};
use centinel_core::store::write_atomically;
use centinel_core::vectors::VectorLock;
use serde::Deserialize;
use serde_json::json;

use super::{maintenance::spawn_maintenance, workspace_error};

pub fn routes() -> axum::Router<Arc<Ctx>> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/workspace/vectors/verify", post(verify))
        .route("/workspace/vectors/repair", post(repair))
        .route("/workspace/vectors/embed", post(embed))
        .route("/workspace/vectors/reports/{id}", get(read))
}

async fn verify(State(ctx): State<Arc<Ctx>>) -> Response {
    spawn_maintenance(
        &ctx,
        "vectors-verify",
        Vec::new(),
        |ctx, id, progress, cancel| async move {
            let _vectors = VectorLock::take(&ctx.store).await?;
            let report = ops::verify(&ctx, VerifyTableArgs::default(), &progress, &cancel).await?;
            cancel.check()?;
            file_report(&ctx, &id, &report)
        },
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reviewed {
    report_id: String,
}

async fn repair(State(ctx): State<Arc<Ctx>>, Json(body): Json<Reviewed>) -> Response {
    let reviewed = match load_report(&ctx, &body.report_id) {
        Ok(report) if report.repaired.is_none() && !report.unreadable.is_empty() => report,
        Ok(_) => {
            return workspace_error(anyhow::anyhow!(
                "choose a verification report with unreadable fragments"
            ));
        }
        Err(error) => return workspace_error(error),
    };
    spawn_maintenance(
        &ctx,
        "vectors-repair",
        Vec::new(),
        |ctx, id, progress, cancel| async move {
            let report = ops::repair_verified(&ctx, &reviewed, &progress, &cancel).await?;
            // The repair committed. File its receipt even if Stop arrived after the commit.
            file_report(&ctx, &id, &report)
        },
    )
}

async fn embed(State(ctx): State<Arc<Ctx>>, Json(body): Json<Reviewed>) -> Response {
    match load_report(&ctx, &body.report_id) {
        Ok(report)
            if report
                .repaired
                .is_some_and(|done| done.fragments_dropped > 0) => {}
        Ok(_) => {
            return workspace_error(anyhow::anyhow!(
                "choose a completed repair report before re-embedding"
            ));
        }
        Err(error) => return workspace_error(error),
    }
    spawn_maintenance(
        &ctx,
        "vectors-embed",
        Vec::new(),
        |ctx, _, progress, cancel| async move {
            ops::embed(&ctx, EmbedArgs::default(), &progress, &cancel).await
        },
    )
}

fn is_report_id(id: &str) -> bool {
    ["vectors-verify-", "vectors-repair-"].iter().any(|prefix| {
        id.strip_prefix(prefix)
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
    })
}

fn load_report(ctx: &Ctx, id: &str) -> anyhow::Result<VerifyReport> {
    anyhow::ensure!(is_report_id(id), "no such vector report");
    Ok(serde_json::from_slice(&std::fs::read(
        ctx.store.vector_report_path(id),
    )?)?)
}

fn file_report(ctx: &Ctx, id: &str, report: &VerifyReport) -> anyhow::Result<()> {
    let path = ctx.store.vector_report_path(id);
    write_atomically(&path, serde_json::to_vec_pretty(report)?)?;
    Ok(())
}

async fn read(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>) -> Response {
    if is_report_id(&id) {
        let path = ctx.store.vector_report_path(&id);
        if path.exists() {
            return match load_report(&ctx, &id) {
                Ok(report) => Json(json!({ "id": id, "report": report })).into_response(),
                Err(error) => workspace_error(error),
            };
        }
        if ctx
            .jobs
            .snapshot()
            .iter()
            .any(|job| job.id == id && job.outcome.is_none())
        {
            return (
                StatusCode::ACCEPTED,
                Json(json!({ "id": id, "report": null })),
            )
                .into_response();
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "no completed vector report for this job" })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use centinel_core::jobs::Outcome;
    use centinel_core::journal::{Holder, RunLock, Trigger};
    use centinel_core::store::Store;
    use centinel_core::vectors::{Row, Space, VectorLock, VectorTable};
    use centinel_core::workspace::{DocState, DocumentId};
    use serde_json::Value;
    use tower::ServiceExt;

    fn app(ctx: Arc<Ctx>) -> axum::Router {
        crate::http::router(ctx).layer(axum::middleware::from_fn_with_state(
            "test-secret".to_string(),
            crate::access::authenticate,
        ))
    }

    async fn send(
        app: &axum::Router,
        role: &str,
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
                    .header("x-centinel-secret", "test-secret")
                    .header("x-centinel-role", role)
                    .header("host", "127.0.0.1:8787")
                    .header("origin", "http://127.0.0.1:8787")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn start(app: &axum::Router, action: &str, body: Value) -> String {
        let (status, result) = send(
            app,
            "admin",
            "POST",
            &format!("/workspace/vectors/{action}"),
            body,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{result}");
        result["id"].as_str().unwrap().to_string()
    }

    async fn finished(ctx: &Ctx, id: &str) -> centinel_core::jobs::JobState {
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            loop {
                if let Some(job) = ctx
                    .jobs
                    .snapshot()
                    .into_iter()
                    .find(|job| job.id == id && job.outcome.is_some())
                {
                    return job;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("maintenance finished within 20 seconds")
    }

    fn row(byte: u8) -> Row {
        Row {
            chunk_hash: format!("{byte:064x}"),
            document: DocumentId {
                source: "fixture".into(),
                resource: format!("https://fixture.gov/{byte}"),
                derived_sha: format!("{byte:064x}"),
            },
            state: DocState::default(),
            vector: vec![1.0, 0.0, 0.0, 0.0],
        }
    }

    async fn fixture() -> (
        tempfile::TempDir,
        Arc<Ctx>,
        VectorTable,
        std::path::PathBuf,
        Vec<u8>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(Ctx::new(Store::open(dir.path()).await.unwrap()));
        let table = VectorTable::open(
            &ctx.store.vectors_db(),
            &Space {
                model: "fixture".into(),
                endpoint: None,
            },
            4,
        )
        .await
        .unwrap();
        let lock = VectorLock::take(&ctx.store).await.unwrap();
        table.append(&lock, &[row(1)]).await.unwrap();
        table.append(&lock, &[row(2)]).await.unwrap();
        drop(lock);
        let mut files = std::fs::read_dir(ctx.store.vectors_path().join("data"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        files.sort();
        let damaged = files.remove(0);
        let original = std::fs::read(&damaged).unwrap();
        std::fs::write(&damaged, vec![0xAA; original.len()]).unwrap();
        (dir, ctx, table, damaged, original)
    }

    fn versions(ctx: &Ctx) -> usize {
        std::fs::read_dir(ctx.store.vectors_path().join("_versions"))
            .unwrap()
            .count()
    }

    /// The transport's durable receipt and approval flow are not covered by the core
    /// audit tests: a job can finish while losing its report, or repair can call the
    /// wrong arguments. The real damaged table must stay unchanged until approval.
    #[tokio::test]
    async fn audit_is_read_only_and_a_reviewed_repair_preserves_the_archive_and_good_rows() {
        let (_dir, ctx, _table, damaged, _original) = fixture().await;
        let blob = ctx
            .store
            .put_blob(b"archived document stays intact")
            .await
            .unwrap();
        let before = versions(&ctx);
        let router = app(Arc::clone(&ctx));
        let id = start(&router, "verify", Value::Null).await;
        assert_eq!(finished(&ctx, &id).await.outcome, Some(Outcome::Ok));
        let (status, result) = send(
            &router,
            "admin",
            "GET",
            &format!("/workspace/vectors/reports/{id}"),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let report = &result["report"];
        assert_eq!(report["fragments"], 2);
        assert_eq!(report["readable"], 1);
        assert_eq!(report["unreadable"].as_array().unwrap().len(), 1);
        assert_eq!(
            report["unreadable"][0]["data_files"][0],
            format!("data/{}", damaged.file_name().unwrap().to_str().unwrap())
        );
        assert!(
            report["unreadable"][0]["error"]
                .as_str()
                .is_some_and(|error| !error.is_empty())
        );
        assert!(report["repaired"].is_null());
        assert_eq!(versions(&ctx), before, "audit commits no table version");

        let repair_id = start(&router, "repair", json!({ "report_id": id })).await;
        assert_eq!(finished(&ctx, &repair_id).await.outcome, Some(Outcome::Ok));
        let (_, receipt) = send(
            &router,
            "admin",
            "GET",
            &format!("/workspace/vectors/reports/{repair_id}"),
            Value::Null,
        )
        .await;
        assert_eq!(receipt["report"]["repaired"]["fragments_dropped"], 1);
        assert_eq!(receipt["report"]["repaired"]["estimated_rows"], 1);
        let reopened = VectorTable::open_existing(&ctx.store.vectors_db())
            .await
            .unwrap();
        assert_eq!(
            reopened.keys(|_| {}).await.unwrap(),
            1,
            "the other fragment remains usable"
        );
        assert!(
            damaged.exists(),
            "repair changes the manifest, not archived or data files"
        );
        assert_eq!(
            ctx.store.get_blob(&blob).await.unwrap(),
            b"archived document stays intact"
        );

        // A fresh process has no job events; the report still reads from storage.
        let restarted = app(Arc::new(Ctx::new(
            Store::open(ctx.store.root()).await.unwrap(),
        )));
        let (_, stored) = send(
            &restarted,
            "admin",
            "GET",
            &format!("/workspace/vectors/reports/{repair_id}"),
            Value::Null,
        )
        .await;
        assert_eq!(stored, receipt);
        let again = start(&router, "repair", json!({ "report_id": id })).await;
        let failed = finished(&ctx, &again).await;
        assert_eq!(failed.outcome, Some(Outcome::Failed));
        assert!(
            failed.error.unwrap().contains("changed"),
            "the old approval cannot be reused"
        );
    }

    /// Even unchanged table versions can have different read findings if storage was
    /// restored or damaged after the audit. Both changes must invalidate approval.
    #[tokio::test]
    async fn repair_rejects_a_changed_version_or_changed_read_findings_without_a_commit() {
        for change_version in [true, false] {
            let (_dir, ctx, table, damaged, original) = fixture().await;
            let router = app(Arc::clone(&ctx));
            let id = start(&router, "verify", Value::Null).await;
            assert_eq!(finished(&ctx, &id).await.outcome, Some(Outcome::Ok));
            if change_version {
                let lock = VectorLock::take(&ctx.store).await.unwrap();
                table.append(&lock, &[row(3)]).await.unwrap();
            } else {
                std::fs::write(&damaged, original).unwrap();
            }
            let before = versions(&ctx);
            let repair = start(&router, "repair", json!({ "report_id": id })).await;
            let failed = finished(&ctx, &repair).await;
            assert_eq!(failed.outcome, Some(Outcome::Failed));
            assert!(failed.error.unwrap().contains("changed"));
            assert_eq!(
                versions(&ctx),
                before,
                "a stale audit cannot delete anything"
            );
        }
    }

    /// A queued audit waits behind a real core writer lock. Stopping it must release
    /// the collection lane without filing a partial report or opening a table.
    #[tokio::test]
    async fn stopping_an_audit_waiting_for_a_vector_writer_releases_the_lane() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(Ctx::new(Store::open(dir.path()).await.unwrap()));
        let router = app(Arc::clone(&ctx));
        let writer = VectorLock::take(&ctx.store).await.unwrap();
        let id = start(&router, "verify", Value::Null).await;
        let (status, pending) = send(
            &router,
            "admin",
            "GET",
            &format!("/workspace/vectors/reports/{id}"),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(pending["report"].is_null());
        let (status, _) = send(
            &router,
            "admin",
            "POST",
            "/workspace/vectors/verify",
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = send(
            &router,
            "admin",
            "POST",
            &format!("/workspace/jobs/{id}/stop"),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        drop(writer);
        assert_eq!(finished(&ctx, &id).await.outcome, Some(Outcome::Cancelled));
        let (status, _) = send(
            &router,
            "admin",
            "GET",
            &format!("/workspace/vectors/reports/{id}"),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(!ctx.store.vectors_path().exists());
        let next = start(&router, "verify", Value::Null).await;
        assert_eq!(finished(&ctx, &next).await.outcome, Some(Outcome::Ok));
    }

    /// These routes are browser-admin only. A read-only user cannot retrieve diagnostic
    /// paths, start paid embedding, or mutate the table. A held lane refuses all work.
    #[tokio::test]
    async fn maintenance_enforces_admin_origin_report_ids_and_the_collection_lane() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(Ctx::new(Store::open(dir.path()).await.unwrap()));
        let router = app(Arc::clone(&ctx));
        for (method, path) in [
            ("POST", "verify"),
            ("POST", "repair"),
            ("POST", "embed"),
            ("GET", "reports/vectors-verify-1"),
        ] {
            let (status, _) = send(
                &router,
                "user",
                method,
                &format!("/workspace/vectors/{path}"),
                json!({ "report_id": "vectors-verify-1" }),
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        }
        let response = router
            .clone()
            .oneshot(
                Request::post("/workspace/vectors/verify")
                    .header("x-centinel-secret", "test-secret")
                    .header("x-centinel-role", "admin")
                    .header("host", "127.0.0.1:8787")
                    .header("origin", "https://other.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        for id in ["..", "vectors-verify-1/../../centinel.db", "run-1"] {
            let (status, _) = send(
                &router,
                "admin",
                "POST",
                "/workspace/vectors/repair",
                json!({ "report_id": id }),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }
        let (status, _) = send(
            &router,
            "admin",
            "GET",
            "/workspace/vectors/reports/..%2F..%2Fcentinel.db",
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send(
            &router,
            "admin",
            "POST",
            "/workspace/vectors/repair",
            json!({ "report_id": "vectors-verify-1", "fragments": [0] }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "fragment instructions are not accepted"
        );
        let lock = RunLock::acquire(
            &ctx.store,
            &Holder {
                pid: std::process::id(),
                started_at: jiff::Timestamp::now().to_string(),
                trigger: Trigger::Manual,
                schedule: None,
                sources: Vec::new(),
                args: String::new(),
                operation: None,
            },
        )
        .unwrap();
        let (status, _) = send(
            &router,
            "admin",
            "POST",
            "/workspace/vectors/verify",
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(
            ctx.jobs.snapshot().is_empty(),
            "refused requests start no work"
        );
        drop(lock);
        let id = start(&router, "verify", Value::Null).await;
        assert_eq!(finished(&ctx, &id).await.outcome, Some(Outcome::Ok));
        assert!(
            !ctx.store.vectors_path().exists(),
            "auditing an empty store creates no vector table"
        );
        let (status, _) = send(
            &router,
            "admin",
            "POST",
            "/workspace/vectors/embed",
            json!({ "report_id": id }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "embedding requires a repair receipt"
        );
    }
}
