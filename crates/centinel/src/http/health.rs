//! Read-only diagnostics for the administrator's System page.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    response::{IntoResponse, Response},
    routing::get,
};
use centinel_core::{
    journal::RunLock,
    op::Ctx,
    ops::{self, DoctorArgs, StatusArgs},
};
use serde_json::json;

pub fn routes() -> Router<Arc<Ctx>> {
    Router::new().route("/workspace/admin/health", get(health))
}

async fn health(State(ctx): State<Arc<Ctx>>) -> Response {
    // Keep readiness and corpus accounting with their existing owners. In particular,
    // status counts distinct current blobs, not every historical file in the store.
    match tokio::try_join!(
        ops::doctor(&ctx, DoctorArgs::default()),
        ops::status(&ctx, StatusArgs::default()),
    ) {
        Ok((doctor, corpus)) => Json(json!({
            "checked_at": jiff::Timestamp::now().to_string(),
            "doctor": doctor,
            "corpus": corpus,
            "running": RunLock::current(&ctx.store),
            // Configured schedules do not prove the scheduler is running. Its runtime
            // owner must supply this before we can report running or disabled.
            "scheduler": { "status": "unknown", "detail": "Scheduler runtime status is not available." },
        })).into_response(),
        Err(error) => super::workspace_error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use centinel_core::{
        journal::{Holder, Trigger},
        prelude::{Resource, SourceId, Store},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_is_admin_only_and_reports_stored_bytes_and_the_collection_lane() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(Ctx::new(Store::open(dir.path()).await.unwrap()));
        for url in ["https://city.gov/a", "https://city.gov/b"] {
            ctx.store
                .record_observation(
                    &Resource::new(SourceId::new("city").unwrap(), url),
                    b"shared document",
                    jiff::Timestamp::now(),
                    Default::default(),
                )
                .await
                .unwrap();
        }
        let app = super::super::router(Arc::clone(&ctx)).layer(
            axum::middleware::from_fn_with_state("secret".to_string(), crate::access::authenticate),
        );
        let request = |secret: &str, role: &str| {
            Request::get("/workspace/admin/health")
                .header("x-centinel-secret", secret)
                .header("x-centinel-role", role)
                .body(Body::empty())
                .unwrap()
        };
        for (secret, role, status) in [
            ("", "admin", StatusCode::UNAUTHORIZED),
            ("secret", "user", StatusCode::FORBIDDEN),
        ] {
            assert_eq!(
                app.clone()
                    .oneshot(request(secret, role))
                    .await
                    .unwrap()
                    .status(),
                status
            );
        }
        let lock = RunLock::acquire(
            &ctx.store,
            &Holder {
                pid: std::process::id(),
                started_at: jiff::Timestamp::now().to_string(),
                trigger: Trigger::Manual,
                schedule: None,
                sources: vec!["city".into()],
                args: "--sources city".into(),
            },
        )
        .unwrap();
        let response = app
            .clone()
            .oneshot(request("secret", "admin"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let report: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await.unwrap())
                .unwrap();
        assert_eq!(report["corpus"]["total"], 2);
        assert_eq!(
            report["corpus"]["bytes"], 15,
            "shared blobs occupy disk once"
        );
        assert_eq!(report["corpus"]["missing_blobs"], 0);
        assert_eq!(report["doctor"]["store_root"], dir.path().to_str().unwrap());
        assert!(
            report["doctor"]["binaries"]
                .as_array()
                .unwrap()
                .iter()
                .all(|binary| binary["purpose"].as_str().is_some_and(|s| !s.is_empty()))
        );
        assert!(report["doctor"]["ready"].is_boolean());
        assert_eq!(report["running"]["sources"], json!(["city"]));
        assert_eq!(report["scheduler"]["status"], "unknown");
        drop(lock);
        let response = app.oneshot(request("secret", "admin")).await.unwrap();
        let report: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await.unwrap())
                .unwrap();
        assert!(
            report["running"].is_null(),
            "refresh reflects the released lane"
        );
    }
}
