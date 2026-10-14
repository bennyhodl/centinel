//! Browser-only, administrator source stage actions. The registry keeps Operator reach.
use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    response::Response,
};
use centinel_core::{
    catalog::Sources,
    op::{Cancel, Ctx, Progress},
    ops::{self, EmbedArgs, ExtractArgs, IndexArgs, TranscribeArgs},
};
use serde::Deserialize;

use super::{maintenance::spawn_maintenance, workspace_error};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Stage {
    Extract,
    Transcribe,
    Index,
    Embed,
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::Extract => "extract",
            Self::Transcribe => "transcribe",
            Self::Index => "index",
            Self::Embed => "embed",
        }
    }

    async fn run(
        self,
        ctx: &Ctx,
        source: String,
        refresh: bool,
        progress: &Progress,
        cancel: &Cancel,
    ) -> anyhow::Result<()> {
        match self {
            Self::Extract => {
                ops::extract(
                    ctx,
                    ExtractArgs {
                        source: Some(source),
                        refresh,
                        ..Default::default()
                    },
                    progress,
                    cancel,
                )
                .await?;
            }
            Self::Transcribe => {
                let config = centinel_core::config::Config::load()?;
                let definition = Sources::new(&ctx.store).require(&source).await?;
                ops::transcribe(
                    ctx,
                    TranscribeArgs {
                        source: Some(source),
                        language: Some(definition.lang.unwrap_or(config.defaults.lang)),
                        refresh,
                        ..Default::default()
                    },
                    progress,
                    cancel,
                )
                .await?;
            }
            Self::Index => {
                ops::index(
                    ctx,
                    IndexArgs {
                        source: Some(source),
                        ..Default::default()
                    },
                    progress,
                    cancel,
                )
                .await?;
            }
            Self::Embed => {
                ops::embed(
                    ctx,
                    EmbedArgs {
                        source: Some(source),
                        ..Default::default()
                    },
                    progress,
                    cancel,
                )
                .await?;
            }
        }
        Ok(())
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct StageBody {
    #[serde(default)]
    refresh: bool,
}

pub fn routes() -> axum::Router<Arc<Ctx>> {
    axum::Router::new().route(
        "/workspace/sources/{id}/stages/{stage}",
        axum::routing::post(start),
    )
}

async fn start(
    State(ctx): State<Arc<Ctx>>,
    Path((id, stage)): Path<(String, Stage)>,
    body: Option<Json<StageBody>>,
) -> Response {
    let body = body.map(|Json(body)| body).unwrap_or_default();
    if body.refresh && !matches!(stage, Stage::Extract | Stage::Transcribe) {
        return workspace_error(anyhow::anyhow!(
            "refresh is supported only for extract and transcribe"
        ));
    }
    if let Err(error) = Sources::new(&ctx.store).require(&id).await {
        return workspace_error(error);
    }
    spawn_maintenance(
        &ctx,
        stage.name(),
        vec![id.clone()],
        move |ctx, _, progress, cancel| async move {
            stage.run(&ctx, id, body.refresh, &progress, &cancel).await
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use centinel_core::{
        config::SourceConfig,
        domain::{Resource, SourceId},
        journal::{Journal, Trigger},
        store::Store,
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    async fn app() -> (tempfile::TempDir, Arc<Ctx>, axum::Router) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let ctx = Arc::new(Ctx::new(store));
        for source in ["a", "b"] {
            Sources::new(&ctx.store)
                .add(&SourceConfig::site(source, format!("https://{source}.gov")))
                .await
                .unwrap();
            let text = "The county commission approved drainage repairs and a public hearing on the proposed budget. ".repeat(12);
            let html = format!(
                "<html><body><main><h1>Commission minutes</h1><p>{text}</p></main></body></html>"
            );
            ctx.store
                .record_observation(
                    &Resource::new(
                        SourceId::new(source).unwrap(),
                        "https://example.gov/minutes",
                    ),
                    html.as_bytes(),
                    jiff::Timestamp::now(),
                    Default::default(),
                )
                .await
                .unwrap();
        }
        let app =
            crate::http::router(Arc::clone(&ctx)).layer(axum::middleware::from_fn_with_state(
                "test-secret".to_string(),
                crate::access::authenticate,
            ));
        (dir, ctx, app)
    }

    async fn post(
        app: &axum::Router,
        path: &str,
        body: Value,
        role: &str,
        origin: &str,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::post(path)
                    .header("host", "127.0.0.1:8787")
                    .header("origin", origin)
                    .header("x-centinel-secret", "test-secret")
                    .header("x-centinel-role", role)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn admin(app: &axum::Router, path: &str, body: Value) -> axum::response::Response {
        post(app, path, body, "admin", "http://127.0.0.1:8787").await
    }

    async fn value(response: axum::response::Response) -> Value {
        serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await.unwrap()).unwrap()
    }

    async fn finished(ctx: &Ctx, id: &str) -> centinel_core::jobs::JobState {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Some(job) = ctx
                    .jobs
                    .snapshot()
                    .into_iter()
                    .find(|job| job.id == id && job.outcome.is_some())
                {
                    return job;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("stage job must finish")
    }

    /// Actual browser request, scoped storage writes, job events, and no invented full run.
    #[tokio::test]
    async fn extract_request_derives_only_its_source_and_finishes_a_stoppable_job() {
        let (_dir, ctx, app) = app().await;
        let response = admin(&app, "/workspace/sources/a/stages/extract", json!({})).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let id = value(response).await["id"].as_str().unwrap().to_string();
        let job = finished(&ctx, &id).await;
        assert_eq!(
            job.outcome,
            Some(centinel_core::jobs::Outcome::Ok),
            "{:?}",
            job.error
        );
        assert_eq!(
            (job.kind.as_str(), job.label.as_str(), job.stoppable),
            ("extract", "a", true)
        );
        assert!(job.total.is_some(), "progress reaches the drawer");
        let a = ctx
            .store
            .replay(&SourceId::new("a").unwrap())
            .await
            .unwrap();
        let b = ctx
            .store
            .replay(&SourceId::new("b").unwrap())
            .await
            .unwrap();
        let derived = a
            .derivations()
            .next()
            .expect("the real extractor wrote a derivation");
        let text = ctx.store.get_blob(&derived.to_sha).await.unwrap();
        assert!(
            String::from_utf8(text)
                .unwrap()
                .contains("county commission")
        );
        assert_eq!(b.derivations().count(), 0);
        assert!(Journal::new(&ctx.store).read().await.unwrap().is_empty());
        assert!(centinel_core::journal::RunLock::current(&ctx.store).is_none());

        let response = admin(&app, "/workspace/sources/a/stages/index", json!({})).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let id = value(response).await["id"].as_str().unwrap().to_string();
        assert_eq!(
            finished(&ctx, &id).await.outcome,
            Some(centinel_core::jobs::Outcome::Ok)
        );
        let index = centinel_core::index::Index::open(ctx.store.require_index().unwrap()).unwrap();
        let hits = index.search("drainage", 10, &Default::default()).unwrap();
        assert!(
            !hits.is_empty(),
            "the real index stage makes extracted text searchable"
        );
        assert!(
            hits.iter()
                .flat_map(|hit| &hit.placements)
                .all(|placement| placement.source == "a")
        );
        drop(index);

        // These still invoke the real ops. No audio and no chunks in b means they must
        // finish locally, even though a has indexed chunks awaiting paid inference.
        for stage in ["transcribe", "embed"] {
            let response = admin(
                &app,
                &format!("/workspace/sources/b/stages/{stage}"),
                json!({}),
            )
            .await;
            assert_eq!(response.status(), StatusCode::ACCEPTED);
            let id = value(response).await["id"].as_str().unwrap().to_string();
            let job = finished(&ctx, &id).await;
            assert_eq!(
                job.outcome,
                Some(centinel_core::jobs::Outcome::Ok),
                "{:?}",
                job.error
            );
        }
        assert!(!ctx.store.vectors_path().exists());
        assert!(Journal::new(&ctx.store).read().await.unwrap().is_empty());
    }

    /// The route must reject unsafe input and callers before starting any job.
    #[tokio::test]
    async fn stage_requests_accept_only_admin_same_origin_source_scoped_arguments() {
        let (_dir, ctx, app) = app().await;
        for (path, body, expected) in [
            (
                "/workspace/sources/a/stages/collect",
                json!({}),
                StatusCode::BAD_REQUEST,
            ),
            (
                "/workspace/sources/missing/stages/extract",
                json!({}),
                StatusCode::BAD_REQUEST,
            ),
            (
                "/workspace/sources/a/stages/embed",
                json!({"refresh":true}),
                StatusCode::BAD_REQUEST,
            ),
            (
                "/workspace/sources/a/stages/index",
                json!({"refresh":true}),
                StatusCode::BAD_REQUEST,
            ),
            (
                "/workspace/sources/a/stages/extract",
                json!({"source":"b"}),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                "/workspace/sources/a/stages/extract",
                json!({"refresh":"yes"}),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
        ] {
            assert_eq!(admin(&app, path, body).await.status(), expected, "{path}");
        }
        let path = "/workspace/sources/a/stages/extract";
        assert_eq!(
            post(&app, path, json!({}), "user", "http://127.0.0.1:8787")
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            post(&app, path, json!({}), "admin", "https://evil.example")
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        assert!(ctx.jobs.snapshot().is_empty());
    }

    /// Stage work and full runs reserve the same lane before responding, and Stop releases
    /// it without writing an interrupted collection attempt for a stage.
    #[tokio::test]
    async fn stages_conflict_with_runs_and_each_other_and_cancellation_releases_the_lane() {
        use crate::lane::{Lane, Launch};
        let (_dir, ctx, app) = app().await;
        let args = || ops::RunArgs {
            sources: vec!["a".into()],
            ..Default::default()
        };
        let launch = || Launch {
            surface: "schedule",
            trigger: Trigger::Schedule,
            schedule: Some("daily".into()),
            due_at: None,
        };
        let run = Lane::acquire(&ctx, args(), launch()).unwrap();
        assert_eq!(
            admin(&app, "/workspace/sources/a/stages/extract", json!({}))
                .await
                .status(),
            StatusCode::CONFLICT
        );
        drop(run);

        // This single-thread runtime processes the two immediate requests before the
        // spawned worker is polled, so Stop reaches the first real mutation boundary.
        let response = admin(&app, "/workspace/sources/a/stages/extract", json!({})).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let id = value(response).await["id"].as_str().unwrap().to_string();
        assert!(Lane::acquire(&ctx, args(), launch()).is_err());
        assert_eq!(
            admin(&app, "/workspace/sources/b/stages/index", json!({}))
                .await
                .status(),
            StatusCode::CONFLICT
        );
        let holder = centinel_core::journal::RunLock::current(&ctx.store).unwrap();
        assert_eq!(holder.operation.as_deref(), Some("extract"));
        assert!(!holder.covers("a", true), "a stage is not collection");
        assert_eq!(
            admin(&app, &format!("/workspace/jobs/{id}/stop"), json!({}))
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            finished(&ctx, &id).await.outcome,
            Some(centinel_core::jobs::Outcome::Cancelled)
        );
        assert_eq!(
            ctx.store
                .replay(&SourceId::new("a").unwrap())
                .await
                .unwrap()
                .derivations()
                .count(),
            0
        );
        assert!(centinel_core::journal::RunLock::current(&ctx.store).is_none());
        assert!(Journal::new(&ctx.store).read().await.unwrap().is_empty());
        let next = Lane::acquire(&ctx, args(), launch()).expect("collection can resume");
        drop(next);
    }
}
