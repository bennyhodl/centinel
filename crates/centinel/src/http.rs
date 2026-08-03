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
//! ## Access control
//!
//! There is none, which is why the default bind is loopback. SPEC §8 lists server
//! access control as unspecified, and inventing a scheme here would foreclose that
//! decision. Binding to a non-loopback address logs a warning rather than silently
//! exposing the store.

use std::convert::Infallible;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use centinel_core::op::{self, Ctx, Progress};
use futures::stream::Stream;
use serde_json::{Value, json};

pub async fn serve(ctx: Arc<Ctx>, bind: &str) -> Result<()> {
    let app = router(ctx);

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    let addr = listener.local_addr()?;

    if !addr.ip().is_loopback() {
        tracing::warn!(
            %addr,
            "serving on a non-loopback address with no authentication — \
             access control is unspecified (SPEC §8)"
        );
        eprintln!("warning: {addr} is reachable off-host and centinel has no authentication yet");
    }

    eprintln!("centinel serving on http://{addr}");
    eprintln!("  GET  /ops                  list operations");
    eprintln!("  POST /ops/{{name}}           invoke");
    eprintln!("  POST /ops/{{name}}/stream    invoke with progress (SSE)");
    eprintln!("  POST /mcp                  MCP over HTTP");

    axum::serve(listener, app).await?;
    Ok(())
}

fn router(ctx: Arc<Ctx>) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/ops", get(list_ops))
        .route("/ops/{name}", post(invoke))
        .route("/ops/{name}/stream", post(invoke_streaming))
        .route("/mcp", post(mcp_over_http))
        .with_state(ctx)
}

/// The registry as JSON — the same information the CLI turns into help text and MCP
/// turns into a tool list.
async fn list_ops() -> Json<Value> {
    let ops: Vec<Value> = op::all()
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
    Json(json!({ "ops": ops }))
}

async fn invoke(
    State(ctx): State<Arc<Ctx>>,
    Path(name): Path<String>,
    body: Option<Json<Value>>,
) -> Response {
    let Some(def) = op::find(&name) else {
        return not_found(&name);
    };
    // An absent body is an empty argument set, so zero-arg ops work with `curl -X POST`.
    let args = body.map(|Json(v)| v).unwrap_or_else(|| json!({}));

    match (def.invoke)(ctx, args, Progress::none()).await {
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
    let Some(def) = op::find(&name) else {
        return not_found(&name);
    };
    let args = body.map(|Json(v)| v).unwrap_or_else(|| json!({}));

    let (progress, mut rx) = Progress::channel();
    let handle = tokio::spawn(async move { (def.invoke)(ctx, args, progress).await });

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
    match crate::mcp::handle(&ctx, req).await {
        Some(resp) => Json(resp).into_response(),
        // A notification: accepted, nothing to say.
        None => StatusCode::ACCEPTED.into_response(),
    }
}

fn not_found(name: &str) -> Response {
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
    async fn ops_endpoint_exposes_the_whole_registry() {
        let (_d, app) = app().await;
        let resp = app
            .oneshot(Request::get("/ops").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        let ops = json["ops"].as_array().unwrap();
        assert_eq!(ops.len(), op::all().len());
        assert!(ops.iter().any(|o| o["name"] == "doctor"));
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

    #[tokio::test]
    async fn bad_arguments_are_400_not_500() {
        let (_d, app) = app().await;
        let resp = app
            .oneshot(
                Request::post("/ops/ingest")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"source":"NOT VALID","urls":[]}"#))
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
}
