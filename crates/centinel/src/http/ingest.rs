//! Browser-only URL acquisition. Shares the collection lock without starting a full run.
use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use centinel_core::{
    catalog::Sources,
    op::Ctx,
    ops::{self, IngestArgs},
};
use serde::Deserialize;
use serde_json::json;

use super::{maintenance::spawn_maintenance, workspace_error};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IngestBody {
    source: String,
    urls: Vec<String>,
}

pub fn routes() -> axum::Router<Arc<Ctx>> {
    axum::Router::new().route("/workspace/ingest", axum::routing::post(ingest))
}

async fn ingest(State(ctx): State<Arc<Ctx>>, Json(body): Json<IngestBody>) -> Response {
    let validation = async {
        Sources::new(&ctx.store).require(&body.source).await?;
        anyhow::ensure!(
            !body.urls.is_empty(),
            "Enter at least one HTTP or HTTPS URL."
        );
        for address in &body.urls {
            let url = reqwest::Url::parse(address)?;
            anyhow::ensure!(
                matches!(url.scheme(), "http" | "https")
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none(),
                "Use an HTTP or HTTPS URL without credentials: {address}"
            );
        }
        anyhow::Ok(())
    }
    .await;
    if let Err(error) = validation {
        return workspace_error(error);
    }
    spawn_maintenance(
        &ctx,
        "ingest",
        vec![body.source.clone()],
        move |ctx, _, progress, cancel| async move {
            progress.say("Fetch URLs. Extraction and indexing must run afterwards to make their content searchable.");
            let args: IngestArgs =
                serde_json::from_value(json!({ "source": body.source, "urls": body.urls }))?;
            let report = ops::ingest(&ctx, args, &progress, &cancel).await?;
            progress.say(format!(
                "{} stored, {} changed, {} failed",
                report.stored, report.changed, report.failed
            ));
            anyhow::ensure!(
                report.failed == 0,
                "{} URLs failed to fetch. Stored URLs were retained.",
                report.failed
            );
            Ok(report)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use centinel_core::{
        config::SourceConfig,
        journal::{Holder, RunLock, Trigger},
        op::Cancel,
        store::Store,
    };
    use tower::ServiceExt;

    async fn post(app: &axum::Router, value: serde_json::Value) -> Response {
        app.clone()
            .oneshot(
                Request::post("/workspace/ingest")
                    .header("content-type", "application/json")
                    .header("host", "127.0.0.1:8787")
                    .header("origin", "http://127.0.0.1:8787")
                    .body(Body::from(value.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn ingestion_validates_requests_stores_http_bytes_and_releases_the_lane() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(Ctx::new(Store::open(dir.path()).await.unwrap()));
        Sources::new(&ctx.store)
            .add(&SourceConfig::site("fixture", "http://127.0.0.1/"))
            .await
            .unwrap();
        let app = crate::http::router(Arc::clone(&ctx))
            .layer(axum::Extension(crate::access::Role::Admin));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/agenda", listener.local_addr().unwrap());
        let bytes = b"<html><body>Council agenda fixture</body></html>";
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let (fixture_entered, fixture_release) = (Arc::clone(&entered), Arc::clone(&release));
        let (shutdown, cancel) = Cancel::channel();
        let fixture = tokio::spawn(async move {
            axum::serve(
                listener,
                axum::Router::new()
                    .route(
                        "/agenda",
                        axum::routing::get(move || async move {
                            ([("content-type", "text/html")], bytes.as_slice())
                        }),
                    )
                    .route(
                        "/slow",
                        axum::routing::get(move || {
                            let (entered, release) =
                                (Arc::clone(&fixture_entered), Arc::clone(&fixture_release));
                            async move {
                                entered.notify_one();
                                release.notified().await;
                                ([("content-type", "text/html")], bytes.as_slice())
                            }
                        }),
                    ),
            )
            .with_graceful_shutdown(async move { cancel.cancelled().await })
            .await
            .unwrap();
        });

        for body in [
            json!({"source":"fixture", "urls": []}),
            json!({"source":"missing", "urls": [url]}),
            json!({"source":"fixture", "urls": [url, "file:///tmp/local"]}),
            json!({"source":"fixture", "urls": ["https://user:secret@example.com"]}),
            json!({"source":"fixture", "urls": [url], "user_agent":"custom"}),
        ] {
            assert!(post(&app, body).await.status().is_client_error());
        }
        assert!(
            ctx.jobs.snapshot().is_empty(),
            "bad input must not start jobs"
        );
        let reader =
            crate::http::router(Arc::clone(&ctx)).layer(axum::Extension(crate::access::Role::User));
        assert_eq!(
            post(&reader, json!({"source":"fixture", "urls":[url]}))
                .await
                .status(),
            StatusCode::FORBIDDEN
        );

        let held = RunLock::acquire(
            &ctx.store,
            &Holder {
                operation: None,
                pid: std::process::id(),
                started_at: jiff::Timestamp::now().to_string(),
                trigger: Trigger::Manual,
                schedule: None,
                sources: vec!["fixture".into()],
                args: "run".into(),
            },
        )
        .unwrap();
        assert_eq!(
            post(&app, json!({"source":"fixture", "urls":[url]}))
                .await
                .status(),
            StatusCode::CONFLICT
        );
        drop(held);

        let response = post(&app, json!({"source":"fixture", "urls":[url]})).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let response: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        let id = response["id"].as_str().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if ctx
                    .jobs
                    .snapshot()
                    .iter()
                    .any(|job| job.id == id && job.outcome.is_some())
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let job = ctx
            .jobs
            .snapshot()
            .into_iter()
            .find(|job| job.id == id)
            .unwrap();
        assert_eq!(job.outcome, Some(centinel_core::jobs::Outcome::Ok));
        assert_eq!(job.ok, 1);
        let replay = ctx
            .store
            .replay(&centinel_core::domain::SourceId::new("fixture").unwrap())
            .await
            .unwrap();
        let observations = replay.latest_observations();
        assert_eq!(observations.len(), 1);
        let observation = observations.values().next().unwrap();
        assert_eq!(
            ctx.store.get_blob(&observation.blob_sha).await.unwrap(),
            bytes
        );
        assert!(RunLock::current(&ctx.store).is_none());
        // Stop while a request is in flight. Its bytes survive, but the next URL is never fetched.
        let slow_url = url.replace("/agenda", "/slow");
        let next_url = url.replace("/agenda", "/not-fetched");
        let response = post(
            &app,
            json!({"source":"fixture", "urls":[slow_url, next_url]}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let response: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        let id = response["id"].as_str().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), entered.notified())
            .await
            .unwrap();
        assert!(
            RunLock::current(&ctx.store).is_some(),
            "held until the in-flight fetch finishes"
        );
        let stopped = app
            .clone()
            .oneshot(
                Request::post(format!("/workspace/jobs/{id}/stop"))
                    .header("host", "127.0.0.1:8787")
                    .header("origin", "http://127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stopped.status(), StatusCode::OK);
        release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if ctx
                    .jobs
                    .snapshot()
                    .iter()
                    .any(|job| job.id == id && job.outcome.is_some())
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let job = ctx
            .jobs
            .snapshot()
            .into_iter()
            .find(|job| job.id == id)
            .unwrap();
        assert_eq!(job.outcome, Some(centinel_core::jobs::Outcome::Cancelled));
        let replay = ctx
            .store
            .replay(&centinel_core::domain::SourceId::new("fixture").unwrap())
            .await
            .unwrap();
        assert_eq!(replay.latest_observations().len(), 2);
        assert!(
            !replay
                .statuses()
                .keys()
                .any(|resource| resource.natural_key == next_url)
        );
        assert!(RunLock::current(&ctx.store).is_none());
        shutdown.cancel();
        fixture.await.unwrap();
    }
}
